//! FitsChan — FITS header I/O for AST objects.
//!
//! A [`FitsChan`] stores a sequence of FITS header cards and provides the
//! ability to read AST objects from them (using the Native encoding stored
//! in COMMENT cards) and to write AST objects into the header.

pub mod card;
pub mod card_list;
mod card_value;
pub(crate) mod dump;
pub(crate) mod enc_aips;
pub(crate) mod enc_class;
pub(crate) mod enc_dss;
pub(crate) mod enc_iraf;
pub(crate) mod enc_native;
pub(crate) mod enc_pc;
pub(crate) mod epoch;
pub(crate) mod fits_match;
pub(crate) mod fits_store;
pub mod format;
pub(crate) mod fset_to_store;
pub(crate) mod guess_encoding;
mod options;
pub(crate) mod source_sink;
pub(crate) mod spec_trans;
pub(crate) mod warning;
pub(crate) mod wcs_celestial;
pub(crate) mod wcs_decompose;
pub(crate) mod wcs_keywords;
pub(crate) mod wcs_mark;
pub(crate) mod wcs_matrix;
pub(crate) mod wcs_read;
pub(crate) mod wcs_spectral;
pub(crate) mod wcs_tab;
pub(crate) mod wcs_write;

pub use card::CardType;
use card::{FitsCard, FitsValue};
use card_list::CardList;
use card_value::{Got, Want};
use source_sink::{NotCopied, SourceCell};
use warning::{Warning, Warnings};
pub use wcs_tab::TableSource;

use std::sync::Arc;

use crate::channel::{Channel, ChannelConfig, Sink};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::intramap::with_registry;
use crate::mappings::{IntraRegistry, IntraTransform};
use crate::serial::node::Node;
use crate::serial::{self, AnyObject};

/// C's `astChrMatchN`: whether `a` and `b` match in their first `n`
/// characters, ignoring case, the end of one string matching white space
/// in the other.
pub(crate) fn chr_match_n(a: &str, b: &str, n: usize) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    let mut nc = 0;
    while (i < a.len() || j < b.len()) && nc < n {
        nc += 1;
        let ca = a.get(i).copied();
        let cb = b.get(j).copied();
        let ok = match (ca, cb) {
            (None, Some(c)) | (Some(c), None) => c.is_ascii_whitespace(),
            (Some(x), Some(y)) => x.eq_ignore_ascii_case(&y),
            (None, None) => true,
        };
        if !ok {
            return false;
        }
        if ca.is_some() {
            i += 1;
        }
        if cb.is_some() {
            j += 1;
        }
    }
    true
}

/// C's `astChrSplitC`: the words of `s` separated by `delim`, in which a
/// backslash before `delim` makes it part of the word in place of the
/// backslash.
fn chr_split_c(s: &str, delim: char) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut escaped = false;
    for c in s.chars() {
        if c == delim {
            if escaped {
                word.pop();
                word.push(delim);
                escaped = false;
            } else {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            }
        }
    }
    words.push(word);
    words
}

/// C's `astChrMatch`: [`chr_match_n`] over the whole of both strings.
pub(crate) fn chr_match(a: &str, b: &str) -> bool {
    chr_match_n(a, b, usize::MAX)
}

/// Which Frames a FITS-WCS write describes with alternate axes: the values
/// of a FitsChan's [AltAxes](FitsChan::alt_axes) attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AltAxes {
    /// Every Frame other than the base and current Frames (the default).
    #[default]
    All,
    /// Only Frames whose Ident is a single upper case letter, `A` to `Z`.
    Ident,
    /// No Frame.
    None,
}

impl AltAxes {
    /// The value's name: `ALL`, `IDENT` or `NONE`.
    // C: fitschan.c `xaltax`.
    pub fn name(self) -> &'static str {
        match self {
            AltAxes::All => "ALL",
            AltAxes::Ident => "IDENT",
            AltAxes::None => "NONE",
        }
    }

    /// The value `name` names, ignoring case.
    pub fn from_name(name: &str) -> Option<Self> {
        [AltAxes::All, AltAxes::Ident, AltAxes::None]
            .into_iter()
            .find(|a| a.name().eq_ignore_ascii_case(name))
    }
}

/// The conventions a [`FitsChan`] uses to describe AST objects as FITS
/// header cards: the values of its [Encoding](FitsChan::encoding)
/// attribute.
///
/// The encodings differ in the objects they can describe, in how many
/// descriptions one set of cards can hold, and in which other software can
/// read the cards.
/// They are not mutually exclusive: the same object may be written to one
/// set of cards in several encodings, which makes it more likely that other
/// software can read one of them.
/// Mixing the foreign encodings (all but `Native`) in one FitsChan is best
/// avoided, as their keywords may clash.
///
/// Every encoding but `Native` describes a single [`FrameSet`] whose base
/// Frame is the pixel coordinate system and whose current Frame is the
/// world coordinate system.
/// A read of one of them returns `None` unless the first card is current,
/// and a successful read consumes the cards it used and leaves the
/// FitsChan at end-of-file, so a second read returns `None` even after
/// rewinding.
/// A write first simplifies the FrameSet to see whether the encoding can
/// describe it.
/// If it can, the description replaces any description already written in
/// that encoding, leaving other cards alone; cards consumed by an earlier
/// read are put back where they were.
/// If it cannot, or the object is not a FrameSet, nothing is written,
/// [`Channel::write`] returns 0 and there is no error.
///
/// [`FrameSet`]: crate::frames::FrameSet
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// `NATIVE`: any AST object, in keywords private to AST that follow the
    /// FITS standard and are chosen not to clash with other keywords.
    ///
    /// Any number of objects may be written to the same cards, one after
    /// another, which makes a FITS header an almost loss-less way of
    /// passing AST objects between programs that use AST.
    /// A write inserts the description in front of the current card and
    /// never overwrites an earlier one; writing one description inside
    /// another is not supported.
    /// It always writes the object and returns 1, barring errors.
    /// A read starts at the current card, passes over cards until the start
    /// of the next description (as the Channel's Skip attribute, true by
    /// default for a FitsChan, allows), and returns the object described, of
    /// whatever class it is.
    /// The cards of the description are removed, and the card after them is
    /// current, so repeated reads return the objects in the order they were
    /// written, until none is left before end-of-file.
    Native,
    /// `FITS-WCS`: the conventions of the FITS world coordinate system
    /// papers by Greisen, Calabretta and others, which any FITS-WCS
    /// software should understand.
    ///
    /// The FrameSet's current Frame gives the primary axis descriptions,
    /// and its other Frames, apart from the base Frame, may give alternate
    /// axis descriptions (see [AltAxes](FitsChan::alt_axes)); a read makes a
    /// Frame for each alternate axis description it finds.
    /// The rotation and scaling of the pixel axes are written with `CDi_j`
    /// keywords, or `PCi_j` and `CDELTi`, as
    /// [CDMatrix](FitsChan::cd_matrix) chooses.
    /// A write puts its cards where cards consumed by an earlier read were,
    /// otherwise after any other FITS-WCS cards, and otherwise in front of
    /// the current card.
    ///
    /// Beyond the standard:
    ///
    /// - The `TPN` projection, a tangent plane projection with polynomial
    ///   distortion terms, is written as `-TAN` with its parameters in `QVi_m`
    ///   keywords in place of `PVi_m`, which other software ignores, reading
    ///   a plain tangent plane projection.
    ///   How a `-TAN` header with `PVi_m` keywords is read depends on
    ///   [PolyTan](FitsChan::poly_tan).
    /// - `AZ--` and `EL--` in CTYPE describe azimuth and elevation, for an
    ///   observer whose position is given by the `OBSGEO-X/Y/Z` keywords.
    ///   The model includes diurnal aberration but not refraction or polar
    ///   motion.
    /// - A SkyFrame giving offset coordinates (see its SkyRefIs attribute)
    ///   is written as two axis descriptions: one of the offsets, with axis
    ///   codes `OFLN` and `OFLT`, and one of the absolute coordinates, which
    ///   also holds the AST keywords (`SREF1/2`, `SREFP1/2` and `SREFIS`)
    ///   that let a read make the original SkyFrame again.
    /// - SIP distortion is read and, with [SipOK](FitsChan::sip_ok),
    ///   written.
    /// - The `-TAB` algorithm is used as [TabOK](FitsChan::tab_ok) allows.
    FitsWcs,
    /// `FITS-PC`: an earlier draft of the FITS-WCS conventions, which
    /// describes the rotation and scaling of the pixel axes with `CDELTi` and
    /// `PCiiijjj` keywords.
    ///
    /// It is used as `FitsWcs` is, with the same restrictions, and is
    /// provided for existing data and software; `FitsWcs` is to be
    /// preferred.
    FitsPc,
    /// `FITS-IRAF`: the conventions of "World Coordinate Systems
    /// Representations Within the FITS Format" by Hanisch and Wells (1988),
    /// used by IRAF.
    ///
    /// It is used as `FitsWcs` is, with these further restrictions:
    ///
    /// - The only celestial coordinate systems are equatorial, galactic and
    ///   ecliptic.
    /// - A sky projection can be written only if its projection parameters
    ///   all have their default values.
    /// - There are no alternate axis descriptions: a write uses only the
    ///   base and current Frames.
    ///
    /// The rotation is written with `CDi_j` keywords; the `CDiiijjj` form is
    /// read but never written.
    /// IRAF's `ZPX` and `TNX` projections are read, but written as the `ZPN`
    /// and distorted `TAN` projections.
    /// A `ZPX` projection's `lngcor` and `latcor` correction surfaces are
    /// not supported, nor are `TNX` surfaces other than cubic polynomials
    /// with half cross-terms; a projection that uses them is read without
    /// them, which may be inaccurate, with a `Tnx` or `Zpx` warning.
    FitsIraf,
    /// `FITS-AIPS`: the conventions introduced by AIPS, which describe the
    /// scale and rotation of each axis with `CDELTi` and `CROTAi` keywords.
    ///
    /// It is used as `FitsWcs` is, with these further restrictions:
    ///
    /// - The only celestial coordinate systems are equatorial, galactic and
    ///   ecliptic.
    /// - A spectral axis must be frequency, radio velocity or optical
    ///   velocity, sampled linearly in frequency, in the LSRK, LSRD,
    ///   barycentric or geocentric standard of rest.
    /// - A sky projection can be written only if its projection parameters
    ///   all have their default values, and the AIT, SFL and MER
    ///   projections only if the longitude and latitude CRVAL are zero.
    /// - There are no alternate axis descriptions: a write uses only the
    ///   base and current Frames.
    /// - With more than 2 axes, any rotation must be confined to the
    ///   celestial plane and have no shear.
    FitsAips,
    /// `FITS-AIPS++`: the conventions of the AIPS++ project, which extend
    /// `FitsAips` with some features of `FitsIraf` and `FitsPc`.
    ///
    /// Any celestial projection `FitsPc` supports may be used, including
    /// those with projection parameters, and the rotation and scaling may be
    /// given by `CDi_j` keywords.
    /// A write uses `CROTAi` and `CDELTi` where it can, and `CDi_j`
    /// otherwise.
    FitsAipsPP,
    /// `FITS-CLASS`: the conventions of CLASS, a package for reducing
    /// single-dish radio and sub-mm spectra (the "CLASS FITS format" section
    /// of its documentation,
    /// <http://www.iram.fr/IRAMFR/GILDAS/doc/html/class-html/class.html>).
    ///
    /// It is like `FitsAips`, with these restrictions:
    ///
    /// - A SpecFrame read from a header has its ObsLat, ObsLon and ObsAlt
    ///   unset, since CLASS does not record the observer's position, so
    ///   conversions to or from the topocentric standard of rest are
    ///   inaccurate (by up to about 0.5 km/s) unless they are set.
    /// - To be written, the current Frame must have at least 3 axes, one of
    ///   them a linear spectral axis, which is written as frequency.
    /// - There must be a pair of celestial axes, either RA and Dec, in
    ///   FK4 B1950 or FK5 J2000, or galactic longitude and latitude.
    /// - Only the TAN, ARC, STG, AIT, SFL and SIN projections can be used:
    ///   for AIT and SFL the reference point must be at the origin of
    ///   longitude and latitude, and for SIN both projection parameters must
    ///   be zero.
    /// - The celestial axes may not be rotated, unless they span a single
    ///   pixel.
    /// - The frequency axis is written in the source's rest frame, as
    ///   offsets from the rest frequency, so the source velocity
    ///   (SpecFrame's SourceVel and SourceVRF) must be set.
    ///
    /// To make the header, a write may change a copy of the current Frame,
    /// with the Mapping to match: reordering the world axes, changing the
    /// spectral system, standard of rest, celestial system and equinox, and
    /// changing axis units.
    FitsClass,
    /// `DSS`: the conventions the Space Telescope Science Institute
    /// developed for the plate solutions of the Digitized Sky Survey,
    /// widely available and understood by many established programs.
    ///
    /// The cards describe a polynomial relating pixel positions on a
    /// digitized photographic plate to right ascension and declination.
    /// To be written, the FrameSet must simplify to the DSS model: an FK5
    /// current Frame, and a tangent plane projection with polynomial
    /// corrections DSS can describe (a DssMap, or a WcsMap of type TAN or
    /// TPN), with north parallel to the second pixel axis.
    Dss,
}

impl Encoding {
    /// The encoding's name: `NATIVE`, `FITS-PC`, `DSS`, `FITS-WCS`,
    /// `FITS-IRAF`, `FITS-AIPS`, `FITS-AIPS++` or `FITS-CLASS`.
    // C: fitschan.c `xencod`.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Native => "NATIVE",
            Encoding::FitsPc => "FITS-PC",
            Encoding::Dss => "DSS",
            Encoding::FitsWcs => "FITS-WCS",
            Encoding::FitsIraf => "FITS-IRAF",
            Encoding::FitsAips => "FITS-AIPS",
            Encoding::FitsAipsPP => "FITS-AIPS++",
            Encoding::FitsClass => "FITS-CLASS",
        }
    }

    /// The encoding `name` names, ignoring case.
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Encoding::Native,
            Encoding::FitsPc,
            Encoding::Dss,
            Encoding::FitsWcs,
            Encoding::FitsIraf,
            Encoding::FitsAips,
            Encoding::FitsAipsPP,
            Encoding::FitsClass,
        ]
        .into_iter()
        .find(|e| e.name().eq_ignore_ascii_case(name))
    }
}

/// Default number of significant figures for FITS floating-point output.
///
/// Matches C `AST__DBL_DIG` (the default of the `FitsDigits` attribute,
/// fitschan.c:43079), which is 17 when AST is built in C11 mode — enough to
/// round-trip an IEEE double exactly. Callers that want the FITS-standard
/// 20-column limit honoured set a smaller value explicitly (the wcsconv
/// fixtures use `FitsDigits=8` for the FITS encodings).
const DEFAULT_FITS_DIGITS: i32 = 17;

/// A [`Channel`] that represents AST objects as FITS header cards.
///
/// A FitsChan holds a list of 80-character FITS header cards.
/// Writing an object to it with [`Channel::write`] adds a description of
/// the object made of header cards, if the object can be described, and
/// reading from it with [`Channel::read`] makes a new object from such a
/// description.
///
/// # The cards
///
/// Any sequence of cards may be held.
/// An `END` card is not needed: [`from_header`](Self::from_header) and
/// [`put_cards`](Self::put_cards) keep one they are given as an ordinary
/// commentary card, and [`to_header`](Self::to_header) adds one only when
/// there is none.
/// The [Card](Self::card) attribute picks out a current card, to which
/// most operations apply, so the cards can be visited in any order:
/// [`find_fits`](Self::find_fits) searches for a keyword,
/// [`put_fits`](Self::put_fits), [`put_cards`](Self::put_cards) and the
/// `set_fits_*` methods add or replace cards, the `get_fits_*` methods
/// return values, and [`del_fits`](Self::del_fits) deletes the current
/// card.
///
/// A [source](Self::set_source) fills the FitsChan with cards the first
/// time it is used; without one, it is empty until cards are put in it, by
/// the methods above, by a write, or by setting the
/// [SourceFile](Self::set_source_file) attribute to read them from a text
/// file.
/// When the FitsChan is dropped, the cards in it are written to the file
/// named by [SinkFile](Self::sink_file), if set, or else to the
/// [sink](Self::set_sink); with neither, they are lost, so take a copy
/// first if they are needed (with [`to_header`](Self::to_header), for
/// instance).
/// No `END` card is written to a sink unless one is held.
///
/// # Encodings
///
/// There are several conventions, or encodings, for describing coordinate
/// systems with FITS header cards.
/// The [Encoding](Self::encoding) attribute chooses the one a read or write
/// uses, and by default it is chosen from the cards already present.
/// The encodings differ in the objects they can describe, in how many
/// descriptions one FitsChan can hold, and in which other software can read
/// them; [`Encoding`] describes each.
///
/// Reading is destructive: the cards that describe the object read are
/// removed from the FitsChan, though [`retain_fits`](Self::retain_fits)
/// keeps a card from being removed.
/// A read that fails leaves the cards as they were, unless
/// [Clean](Self::clean) is set.
/// An encoding that holds a single description (all but `NATIVE`) has a
/// write replace any description already there; with `NATIVE`, descriptions
/// are written one after another and are read back in the same order.
///
/// # Attributes
///
/// Besides those of every [`Channel`], a FitsChan has the attributes
/// [AllWarnings](Self::all_warning_conditions),
/// [AltAxes](Self::alt_axes), [Card](Self::card),
/// [CardComm](Self::card_comm), [CardName](Self::card_name),
/// [CardType](Self::card_type), [CarLin](Self::car_lin),
/// [CDMatrix](Self::cd_matrix), [Clean](Self::clean),
/// [DefB1950](Self::def_b1950), [Encoding](Self::encoding),
/// [FitsAxisOrder](Self::fits_axis_order), [FitsDigits](Self::fits_digits),
/// [FitsRounding](Self::fits_rounding), [FitsTol](Self::fits_tol),
/// [ForceTab](Self::force_tab), [IgnoreBadAlt](Self::ignore_bad_alt),
/// [Iwc](Self::iwc), [Ncard](Self::ncard), [Nkey](Self::nkey),
/// [PolyTan](Self::poly_tan), [SinkFile](Self::sink_file),
/// [SipOK](Self::sip_ok), [SipReplace](Self::sip_replace),
/// [SourceFile](Self::source_file), [TabOK](Self::tab_ok) and
/// [Warnings](Self::warning_conditions).
///
/// # Example
///
/// ```
/// use ast_core::fitschan::FitsChan;
/// use ast_core::Channel;
///
/// let header = "SIMPLE  =                    T / Fits standard\n\
///               NAXIS   =                    2 / Number of axes\n\
///               END";
/// let mut fc = FitsChan::from_header(header).unwrap();
/// assert_eq!(fc.ncard().unwrap(), 3); // SIMPLE, NAXIS, END
/// assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(2));
/// ```
pub struct FitsChan {
    config: ChannelConfig,
    cards: CardList,
    /// Explicit `Encoding` set by the user via [`FitsChan::set_encoding`].
    /// `None` means "not set" — [`FitsChan::encoding`] auto-detects from
    /// the cards on demand. Mirrors C's `astTestEncoding` /
    /// `astGetEncoding` (fitschan.c:12560). The `DefB1950` default also
    /// keys off whether the encoding has been explicitly set
    /// (fitschan.c:42419).
    encoding: Option<Encoding>,
    warnings: Vec<String>,
    fits_digits: i32,
    /// The FitsRounding attribute, `None` when unset.
    fits_rounding: Option<i32>,
    /// The CDMatrix attribute, `None` when unset.
    cd_matrix: Option<bool>,
    /// The SipReplace attribute, `None` when unset.
    sip_replace: Option<bool>,
    /// The CarLin attribute, `None` when unset.
    car_lin: Option<bool>,
    /// The FitsTol attribute, `None` when unset.
    fits_tol: Option<f64>,
    /// The DefB1950 attribute, `None` when unset.
    def_b1950: Option<bool>,
    /// The PolyTan attribute, `None` when unset.
    poly_tan: Option<i32>,
    /// The IgnoreBadAlt attribute, `None` when unset.
    ignore_bad_alt: Option<bool>,
    /// The Warnings attribute, `None` when unset.
    warning_conditions: Option<String>,
    /// The AltAxes attribute, `None` when unset.
    alt_axes: Option<AltAxes>,
    /// The Iwc attribute, `None` when unset.
    iwc: Option<bool>,
    /// The FitsAxisOrder attribute, `None` when unset.
    fits_axis_order: Option<String>,
    /// The transformations IntraMaps read here can name.
    intra: IntraRegistry,
    /// The last sequence number given to each keyword base by the NATIVE
    /// writer (C's `keyseq`), kept until the cards are emptied.
    keyseq: std::collections::HashMap<String, usize>,
    /// The FitsTables stored for the `-TAB` algorithm, keyed by extension
    /// name (C's `tables`). `None` until a table is first stored, as C
    /// creates the KeyMap then; a dump holds the KeyMap whenever it
    /// exists, even when empty.
    tables: Option<crate::keymap::KeyMap>,
    /// The Clean attribute, when set.
    clean: Option<bool>,
    /// The SipOK attribute, when set.
    sip_ok: Option<bool>,
    /// The Object-class Ident attribute, when set.
    ident: Option<String>,
    /// The Object-class UseDefs attribute, when set: only through
    /// `FITSCHAN_OPTIONS`, since nothing a FitsChan does reads it; its dump
    /// holds it.
    use_defs: Option<bool>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The TabOK attribute, when set.
    tab_ok: Option<i32>,
    /// The ForceTab attribute, when set.
    force_tab: Option<i32>,
    /// The source cards are read from on first use, and what a clone has
    /// read from it.
    source: SourceCell,
    /// The sink the cards are written to when dropped.
    sink: NotCopied<Box<dyn Sink>>,
    /// The SourceFile attribute, when set.
    source_file: NotCopied<String>,
    /// The SinkFile attribute, when set.
    sink_file: NotCopied<String>,
    /// The source of the tables a `-TAB` header refers to, which a copy
    /// does not inherit (C's `tabsource`).
    table_source: NotCopied<Box<dyn TableSource>>,
}

impl FitsChan {
    /// Create an empty FitsChan, with the attribute settings in the
    /// `FITSCHAN_OPTIONS` environment variable, if any.
    ///
    /// The variable holds attribute settings separated by commas, such as
    /// `Encoding=FITS-WCS, FitsDigits=-15`, applied in turn to every new
    /// FitsChan, so that a program can be given its defaults without being
    /// changed.
    /// A setting is the attribute's name, in any case, then `=` and the
    /// value, which is taken exactly as written after the `=`.
    /// A string value may be enclosed in double quotes, inside which a
    /// comma does not end the setting.
    /// The variable is read when a thread makes its first FitsChan, and the
    /// value then is used for every FitsChan the thread makes.
    ///
    /// Further attributes are set with their setters, a source of cards with
    /// [`set_source`](Self::set_source) and a sink for them with
    /// [`set_sink`](Self::set_sink).
    /// With the [Encoding](Self::encoding) unset, the encoding is chosen
    /// from the cards when it is needed: `NATIVE` while there are none.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badat`] if `FITSCHAN_OPTIONS` names an attribute a
    ///   FitsChan does not have, or gives an Encoding or AltAxes value that
    ///   is not one of its names.
    /// - [`ErrorCode::Nowrt`] if it sets a read-only attribute.
    /// - The error of the setter a setting is given to, such as
    ///   [`set_warning_conditions`](Self::set_warning_conditions) for an
    ///   unknown condition.
    ///
    /// [`ErrorCode::Badat`]: crate::error::ErrorCode::Badat
    /// [`ErrorCode::Nowrt`]: crate::error::ErrorCode::Nowrt
    // C: astFitsChan applies the variable through astEnvSet before its own
    // options, which the Rust API sets through the typed setters.
    #[doc(alias = "astFitsChan")]
    pub fn new() -> AstResult<Self> {
        let mut fc = Self::bare();
        if let Some(settings) = options::env_options() {
            options::apply(&mut fc, &settings)?;
        }
        Ok(fc)
    }

    /// An empty FitsChan with no attribute set, ignoring
    /// `FITSCHAN_OPTIONS`, as C's loader and copy make one.
    pub(crate) fn bare() -> Self {
        Self {
            config: ChannelConfig::fitschan(),
            cards: CardList::new(),
            encoding: None,
            warnings: Vec::new(),
            intra: IntraRegistry::new(),
            fits_digits: DEFAULT_FITS_DIGITS,
            fits_rounding: None,
            cd_matrix: None,
            sip_replace: None,
            car_lin: None,
            fits_tol: None,
            def_b1950: None,
            poly_tan: None,
            ignore_bad_alt: None,
            warning_conditions: None,
            alt_axes: None,
            iwc: None,
            fits_axis_order: None,
            keyseq: std::collections::HashMap::new(),
            tables: None,
            clean: None,
            sip_ok: None,
            ident: None,
            use_defs: None,
            id: Default::default(),
            tab_ok: None,
            force_tab: None,
            source: SourceCell::default(),
            sink: NotCopied::default(),
            source_file: NotCopied::default(),
            sink_file: NotCopied::default(),
            table_source: NotCopied::default(),
        }
    }

    /// Create a FitsChan from FITS header text, with the first card current.
    ///
    /// The FitsChan is made by [`new`](Self::new), so it has the attribute
    /// settings in `FITSCHAN_OPTIONS`, and the cards are then stored.
    /// The text is split into lines and each is parsed as a FITS card.
    /// An `END` card has no `=` in column 9, so it is stored as an ordinary
    /// commentary card, just like `HISTORY`, `COMMENT` or a blank keyword.
    /// A line that cannot be read as a card, one whose keyword name (its
    /// first 8 characters) holds an `=` before any other illegal character,
    /// is left out.
    /// The warnings for illegal keyword names and values are stored as
    /// [`put_fits`](Self::put_fits) stores them.
    ///
    /// # Errors
    ///
    /// - The errors of [`new`](Self::new).
    /// - [`ErrorCode::Badin`] for a card with a warning, when
    ///   `FITSCHAN_OPTIONS` sets [Strict](ChannelConfig::strict), as for
    ///   [`put_fits`](Self::put_fits).
    ///
    /// [`ErrorCode::Badin`]: crate::error::ErrorCode::Badin
    pub fn from_header(text: &str) -> AstResult<Self> {
        let mut fc = Self::new()?;
        for line in text.lines() {
            let Ok((card, bad_value)) = FitsCard::split(line) else {
                continue;
            };
            // The cards are appended, so the warnings go in front of the
            // end-of-file, where C's are while `astPutFits` appends.
            fc.cards.set_position(usize::MAX);
            fc.split_warnings(line, bad_value, "astPutFits")?;
            // The setter `PutFits` hands the card to splits its keyword name
            // again and warns again of an illegal name; that block matches
            // the first, so adds no cards.
            fc.split_warnings(line, false, "astPutFits")?;
            fc.cards.push(card);
        }
        fc.cards.clear_card();
        Ok(fc)
    }

    // -----------------------------------------------------------------------
    // The current card
    // -----------------------------------------------------------------------

    /// The Card attribute: the index of the current card, the first card
    /// being 1.
    ///
    /// The current card is the one that [`del_fits`](Self::del_fits),
    /// [`find_fits`](Self::find_fits), [`put_fits`](Self::put_fits) and the
    /// other card methods act on.
    /// Setting Card moves to any card; reading it tells where the FitsChan
    /// is.
    /// Clearing it rewinds the FitsChan, so that the first card is current.
    /// Cards consumed by a read are not counted.
    /// A FitsChan past its last card is at end-of-file, and its Card is one
    /// more than [Ncard](Self::ncard).
    ///
    /// Default: 1, the first card, or 0 when there are no cards.
    ///
    /// # Errors
    ///
    /// Passes on the error of storing the cards of a
    /// [source](Self::set_source) not yet read, as for
    /// [`put_fits`](Self::put_fits).
    #[doc(alias = "Card")]
    pub fn card(&mut self) -> AstResult<usize> {
        self.read_from_source()?;
        Ok(self.cards.get_card())
    }

    /// Set [Card](Self::card): rewind, then move forward `icard - 1` cards,
    /// stopping at end-of-file.
    /// A value below 1 makes the first card current.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    pub fn set_card(&mut self, icard: i64) -> AstResult<()> {
        self.read_from_source()?;
        self.cards.set_card(icard);
        Ok(())
    }

    /// Clear [Card](Self::card), rewinding the FitsChan so that the first
    /// card is current.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    pub fn clear_card(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        self.cards.clear_card();
        Ok(())
    }

    /// Whether [Card](Self::card) is set, which is whether the current card
    /// is other than the first.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    pub fn test_card(&mut self) -> AstResult<bool> {
        self.read_from_source()?;
        Ok(self.cards.test_card())
    }

    /// The read-only CardName attribute: the keyword of the current card,
    /// eight spaces for a blank keyword, or `None` at end-of-file.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "CardName")]
    pub fn card_name(&mut self) -> AstResult<Option<String>> {
        self.read_from_source()?;
        Ok(self.cards.current().map(|c| c.c_name().to_string()))
    }

    /// The read-only CardComm attribute: the comment of the current card, or
    /// the text of a commentary card.
    ///
    /// It is empty when the card has no comment, and at end-of-file.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "CardComm")]
    pub fn card_comm(&mut self) -> AstResult<String> {
        self.read_from_source()?;
        Ok(self
            .cards
            .current()
            .and_then(|c| c.c_comment())
            .unwrap_or("")
            .to_string())
    }

    /// The read-only CardType attribute: the data type of the value of the
    /// current card, or [`CardType::NoType`] at end-of-file.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "CardType")]
    pub fn card_type(&mut self) -> AstResult<CardType> {
        self.read_from_source()?;
        Ok(self
            .cards
            .current()
            .map_or(CardType::NoType, |c| c.card_type()))
    }

    /// The read-only Ncard attribute: the number of cards in the FitsChan,
    /// not counting those a read consumed.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "Ncard")]
    pub fn ncard(&mut self) -> AstResult<usize> {
        self.read_from_source()?;
        Ok(self.cards.ncard())
    }

    /// The read-only Nkey attribute: the number of different keywords among
    /// the cards a read has not consumed.
    ///
    /// It equals [Ncard](Self::ncard) when no keyword appears twice, and is
    /// smaller otherwise.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "Nkey")]
    pub fn nkey(&mut self) -> AstResult<usize> {
        self.read_from_source()?;
        Ok(self.cards.nkey())
    }

    // -----------------------------------------------------------------------
    // Card manipulation API
    // -----------------------------------------------------------------------

    /// C's `Warn`: unless Clean is set, a warning whose condition the
    /// Warnings attribute names is added to the Channel's warnings and stored
    /// as an `ASTWARN` block in front of the current card.
    /// The Channel's warnings take it only when ReportLevel is at least 1,
    /// as C's `astAddWarning` reports a FitsChan warning at level 1; the
    /// `ASTWARN` block is stored whatever the ReportLevel.
    /// With Strict set and ReportLevel at least 1, such a warning is instead
    /// the error [`ErrorCode::Badin`] naming `method`, and nothing is
    /// stored.
    fn warn(&mut self, warning: Warning, method: &'static str) -> AstResult<()> {
        if self.clean()
            || warning.text.is_empty()
            || !warning::reports(self.warning_conditions(), warning.condition)
        {
            return Ok(());
        }
        if self.config.report_level() >= 1 {
            if self.config.strict() {
                return Err(warning::strict_error(
                    warning.method.unwrap_or(method),
                    &warning.text,
                ));
            }
            self.cards.push_astwarn(&warning.text);
            self.warnings.push(warning.text);
        } else {
            self.cards.push_astwarn(&warning.text);
        }
        Ok(())
    }

    /// An empty list for the warnings `method` raises, which with Strict in
    /// force makes a warning [`warn`](Self::warn) would store an error at
    /// the point it is raised, as C's `Warn` makes it.
    fn warn_list(&self, method: &'static str) -> Warnings {
        if self.config.strict() && !self.clean() && self.config.report_level() >= 1 {
            Warnings::strict(self.warning_conditions(), method)
        } else {
            Warnings::new()
        }
    }

    /// The warnings C's `Split` gives for `card`, of an illegal keyword name
    /// and then of a value it cannot type, for `method`.
    fn split_warnings(
        &mut self,
        card: &str,
        bad_value: bool,
        method: &'static str,
    ) -> AstResult<()> {
        if let Some(text) = crate::fitschan::card::check_fits_name(card) {
            self.warn(Warning::new("badkeyname", text), method)?;
        }
        if bad_value {
            self.warn(
                Warning::new(
                    "badkeyvalue",
                    crate::fitschan::card::bad_value_warning(card),
                ),
                method,
            )?;
        }
        Ok(())
    }

    /// The keyword and comment C's `Split` finds when it reads a setter's or
    /// getter's keyword name as a card: the first 8 characters, in their own
    /// case (`NewCard` upper-cases them when a card is stored), and any text
    /// after them. As with any card, an illegal name or value is warned of.
    fn split_name(&mut self, name: &str, method: &'static str) -> AstResult<(String, String)> {
        let (card, bad_value) = FitsCard::split(name)?;
        self.split_warnings(name, bad_value, method)?;
        let comment = match &card.value {
            FitsValue::Comment(text) => text.trim().to_string(),
            _ => card.comment.clone().unwrap_or_default(),
        };
        let raw: String = name.chars().take(8).collect();
        let keyword = raw.trim_end().to_string();
        debug_assert!(keyword.eq_ignore_ascii_case(&card.keyword));
        Ok((keyword, comment))
    }

    /// Mark the current card to be kept when a read would otherwise remove
    /// it.
    ///
    /// A successful read removes the cards it used to make the object it
    /// returns, unless they are marked by this method; cards it did not use
    /// are kept whether marked or not.
    /// The current card is not changed, and at end-of-file this does
    /// nothing.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "astRetainFits")]
    pub fn retain_fits(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        self.cards.protect_current();
        Ok(())
    }

    /// Whether keyword `name` has a value: `None` if the keyword is not
    /// present, `Some(false)` if it is present with no value, and
    /// `Some(true)` if it has a value.
    ///
    /// With `name` `None`, the current card is tested, and at end-of-file
    /// the result is `None`.
    /// A commentary card counts as having a value.
    /// `name` is read as for [`get_fits_f`](Self::get_fits_f), and the card
    /// is searched for in the same way, but the current card is left as it
    /// was.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bdfts`] if the keyword name in `name` (its first 8
    ///   characters) holds an `=` before any other illegal character.
    /// - [`ErrorCode::Bdfmt`] if `name` holds an illegal field specifier
    ///   (see [`find_fits`](Self::find_fits)).
    /// - As for [`card`](Self::card).
    #[doc(alias = "astTestFits")]
    pub fn test_fits(&mut self, name: Option<&str>) -> AstResult<Option<bool>> {
        self.read_from_source()?;
        let lname = match name {
            Some(name) => Some(self.split_name(name, "astTestFits")?.0),
            None => None,
        };
        let icard = self.cards.get_card();
        let found = match &lname {
            Some(n) => self.cards.search_card(n, "astTestFits"),
            None => Ok(true),
        };
        let result = found.map(|found| {
            if !found {
                return None;
            }
            match self.cards.current().map(|c| c.card_type()) {
                None | Some(CardType::NoType) => None,
                Some(t) => Some(t != CardType::Undef),
            }
        });
        self.cards.set_card(icard as i64);
        result
    }

    /// Delete every card that describes WCS information in any of the
    /// encodings.
    ///
    /// Each object a read can find is read with [Clean](Self::clean) set and
    /// discarded, which removes the cards it used even when the read fails,
    /// and then every remaining card whose keyword is a WCS keyword is
    /// removed as a read removes it.
    /// The first remaining card is then current.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card); a read that fails is not an error.
    #[doc(alias = "astPurgeWCS")]
    pub fn purge_wcs(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        let old = self.clean;
        self.clean = Some(true);
        self.cards.clear_card();
        while let Ok(Some(_)) = self.read() {
            self.cards.clear_card();
        }
        self.cards.consume_wcs_cards();
        self.cards.clear_card();
        self.clean = old;
        Ok(())
    }

    /// Store a FITS header card.
    ///
    /// The card is inserted in front of the current card, or, with
    /// `overwrite`, replaces it; either way the card after the one stored
    /// becomes current.
    /// At end-of-file the card is appended.
    /// Only the first 80 characters of `card` are used, and trailing
    /// newlines and other non-printing characters are ignored.
    /// A blank comment, when overwriting, keeps the comment of the card
    /// replaced.
    ///
    /// A keyword name with an illegal character, or a value whose type
    /// cannot be determined, is stored with a `BadKeyName` or `BadKeyValue`
    /// warning (see [Warnings](Self::warning_conditions)); a value whose
    /// type cannot be determined gives a commentary card holding just the
    /// keyword.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bdfts`] if the keyword name of `card` (its first 8
    ///   characters) holds an `=` before any other illegal character.
    /// - [`ErrorCode::Badin`] for a `BadKeyName` or `BadKeyValue` warning,
    ///   when [Strict](ChannelConfig::strict) is set; nothing is then
    ///   stored.
    /// - As for [`card`](Self::card).
    #[doc(alias = "astPutFits")]
    pub fn put_fits(&mut self, card: &str, overwrite: bool) -> AstResult<()> {
        self.read_from_source()?;
        let (parsed, bad_value) = FitsCard::split(card)?;
        self.split_warnings(card, bad_value, "astPutFits")?;
        match parsed.value {
            // `PutFits` hands a commentary card to `SetFitsCom`.
            FitsValue::Comment(ref text) => {
                let name: String = card.chars().take(8).collect();
                self.set_fits_com(&name, text, overwrite)?;
            }
            _ => {
                // The `astSetFits<X>` the card is handed to splits its
                // keyword name again, warning again of an illegal name.
                self.split_warnings(card, false, "astPutFits")?;
                let comment = parsed.comment.clone().unwrap_or_default();
                self.store_card(parsed.keyword, parsed.value, &comment, overwrite);
            }
        }
        Ok(())
    }

    /// Replace every card with `cards`, and rewind.
    ///
    /// `cards` holds the header cards concatenated, 80 characters each with
    /// no separators or newlines between them; the last may be shorter.
    /// This is the form CFITSIO's `fits_hdr2str` produces.
    /// Every card already in the FitsChan is deleted first, as by
    /// [`empty_fits`](Self::empty_fits), and the first card is current at
    /// the end, so a read can follow at once.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bdfts`] if the keyword name of a card (its first 8
    ///   characters) holds an `=` before any other illegal character; the
    ///   cards before it have been stored.
    /// - As for [`card`](Self::card).
    #[doc(alias = "astPutCards")]
    pub fn put_cards(&mut self, cards: &str) -> AstResult<()> {
        self.empty_fits()?;
        let bytes = cards.as_bytes();
        for chunk in bytes.chunks(80) {
            let card = String::from_utf8_lossy(chunk);
            self.put_fits(&card, true)?;
        }
        self.cards.clear_card();
        Ok(())
    }

    /// Search forward from the current card for one whose keyword matches
    /// the template `name`, and return the card's text.
    ///
    /// The search starts at the current card, so rewind first (clear
    /// [Card](Self::card)) to search the whole FitsChan.
    /// If a card is found it becomes current, or with `inc` the card after
    /// it does, and its text is returned, formatted with
    /// [FitsDigits](Self::fits_digits) and
    /// [FitsRounding](Self::fits_rounding).
    /// If none is found the result is `None` and the FitsChan is left at
    /// end-of-file.
    ///
    /// # Keyword templates
    ///
    /// The match ignores case, and trailing spaces in `name` are ignored.
    /// Literal characters must match the keyword exactly.
    /// A field specifier, `%` followed by an optional single digit giving a
    /// width and then a type character, matches a range of characters:
    ///
    /// - `%c` matches upper case letters.
    /// - `%d` matches decimal digits.
    /// - `%f` matches any character allowed in a FITS keyword: upper case
    ///   letters, digits, underscores and hyphens.
    ///
    /// With no width the field matches one or more characters, with width 0
    /// it matches zero or more, and otherwise exactly that many.
    /// The template `%f` matches any keyword, including a blank one, so
    /// `find_fits("%f", true)` returns the current card and moves on to the
    /// next.
    /// A blank template, its trailing spaces being ignored, matches no
    /// keyword, not even a blank one.
    /// For instance, `BitPix` matches `BITPIX` only, `crpix%1d` matches
    /// `CRPIX` followed by one digit, `P%c` matches `P` followed by one or
    /// more letters, and `E%0f` matches any keyword beginning with `E`.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bdfmt`] if `name` holds an illegal field type or
    ///   width, or ends in an incomplete field specifier.
    /// - [`ErrorCode::Bdfts`] if the card found cannot be formatted: a
    ///   string value too long for a card, or a floating value that cannot
    ///   be encoded.
    /// - As for [`card`](Self::card).
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::fitschan::FitsChan;
    ///
    /// let mut fc = FitsChan::from_header(
    ///     "CRVAL1  =                 10.5\nCRVAL2  =                 -3.0\nEND",
    /// )?;
    /// let card = fc.find_fits("CRVAL%1d", true)?.expect("a CRVAL card");
    /// assert!(card.starts_with("CRVAL1  ="));
    /// assert_eq!(fc.card()?, 2);
    /// assert!(fc.find_fits("CRVAL%1d", true)?.unwrap().starts_with("CRVAL2"));
    /// assert_eq!(fc.find_fits("CRVAL%1d", true)?, None);
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    #[doc(alias = "astFindFits")]
    pub fn find_fits(&mut self, name: &str, inc: bool) -> AstResult<Option<String>> {
        self.read_from_source()?;
        let lname = name.trim_end_matches(' ');
        let found = if lname == "%f" || lname == "%0f" {
            !self.cards.at_eof()
        } else {
            self.cards.find_key_card(lname, "astFindFits")?
        };
        if !found {
            return Ok(None);
        }
        let text = self
            .cards
            .current()
            .expect("a card was found")
            .format_with_digits_and_rounding(self.fits_digits, self.fits_rounding() as usize)?;
        if inc {
            self.cards.move_card(1);
        }
        Ok(Some(text))
    }

    /// Delete the current card, making the next card current.
    ///
    /// The current card is chosen with [Card](Self::card), or by keyword
    /// with [`find_fits`](Self::find_fits).
    /// Deleting the last card leaves the FitsChan at end-of-file, and at
    /// end-of-file this does nothing.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "astDelFits")]
    pub fn del_fits(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        self.cards.delete_card();
        Ok(())
    }

    /// Delete every card, including those a read consumed, and every stored
    /// table.
    ///
    /// Unlike [`write_fits`](Self::write_fits), this does not write the
    /// cards to the sink or [SinkFile](Self::sink_file) first.
    /// The warnings of the last read or write
    /// ([`warning_texts`](Channel::warning_texts)) are kept.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "astEmptyFits")]
    pub fn empty_fits(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        self.empty_fits_inner();
        Ok(())
    }

    /// C's `EmptyFits` once the source has been read.
    fn empty_fits_inner(&mut self) {
        self.cards = CardList::new();
        self.keyseq.clear();
        self.tables = None;
    }

    // -----------------------------------------------------------------------
    // FitsTables
    // -----------------------------------------------------------------------

    /// Store a copy of `table`, a FITS binary table, under the extension
    /// name `extnam`, replacing any table stored under that name.
    ///
    /// The tables stored provide the look-up tables that a FITS-WCS header
    /// using the `-TAB` algorithm refers to, which a read needs (see
    /// [TabOK](Self::tab_ok)).
    /// Changing `table` afterwards does not change the copy stored.
    /// Since which tables a read will need is often not known in advance, a
    /// [table source](Self::set_table_source) can supply them as the read
    /// asks for them instead.
    /// [`get_tables`](Self::get_tables) returns the tables stored, and
    /// [`put_tables`](Self::put_tables) stores several at once.
    ///
    /// # Errors
    ///
    /// Passes on the error of
    /// [`KeyMap::put_0a`](crate::keymap::KeyMap::put_0a) storing the table.
    /// The FitsChan never locks its KeyMap of tables, so in practice there is
    /// none.
    #[doc(alias = "astPutTable")]
    pub fn put_table(
        &mut self,
        table: &crate::fitstable::FitsTable,
        extnam: &str,
    ) -> AstResult<()> {
        self.tables
            .get_or_insert_with(crate::keymap::KeyMap::new)
            .put_0a(extnam, AnyObject::FitsTable(Box::new(table.clone())), None)
    }

    /// Store a copy of each FitsTable in `tables` under its key, as
    /// [`put_table`](Self::put_table) does.
    ///
    /// Each key is the name of the FITS extension the table was read from,
    /// and each entry should hold a single
    /// [`FitsTable`](crate::fitstable::FitsTable); other entries are ignored.
    /// The tables are added to those already stored, replacing any stored
    /// under the same name.
    ///
    /// # Errors
    ///
    /// Passes on the errors of reading `tables` and of
    /// [`put_table`](Self::put_table).
    #[doc(alias = "astPutTables")]
    pub fn put_tables(&mut self, tables: &crate::keymap::KeyMap) -> AstResult<()> {
        for ientry in 0..tables.map_size() {
            let key = tables.map_key(ientry)?;
            if tables.map_type(&key)? == crate::keymap::AstType::Object
                && tables.map_length(&key)? == 1
                && let Some(Some(AnyObject::FitsTable(table))) = tables.get_0a(&key)?
            {
                self.put_table(&table, &key)?;
            }
        }
        Ok(())
    }

    /// Register `source` to supply the FITS binary tables a `-TAB` header
    /// refers to, as a read needs them, replacing any source registered
    /// before.
    ///
    /// A read of a header that describes axes with the `-TAB` algorithm
    /// needs the tables holding their look-up values, and reads such axes
    /// only when [TabOK](Self::tab_ok) is positive.
    /// The tables can be stored beforehand with
    /// [`put_table`](Self::put_table) or [`put_tables`](Self::put_tables),
    /// but that needs the extension names, which are held in the header and
    /// so are often not known in advance.
    /// A source is instead asked for each table by the read, with the
    /// extension's name, version and level, when the table is needed.
    /// A table it supplies is stored under the extension name asked for,
    /// replacing any stored there, and used; when it supplies none, a table
    /// already stored under that name is used.
    /// Data the source needs, such as an open file, can be captured by the
    /// closure or held by the type that implements [`TableSource`].
    /// A copy of the FitsChan has no source.
    #[doc(alias = "astTableSource")]
    pub fn set_table_source(&mut self, source: impl TableSource + 'static) {
        self.table_source.0 = Some(Box::new(source));
    }

    /// Remove any [table source](Self::set_table_source).
    #[doc(alias = "astTableSource")]
    pub fn clear_table_source(&mut self) {
        self.table_source.0 = None;
    }

    /// A copy of the stored tables, each a
    /// [`FitsTable`](crate::fitstable::FitsTable) keyed by the name of the
    /// FITS extension it belongs in, or `None` when there are none.
    ///
    /// Tables are stored by [`put_table`](Self::put_table),
    /// [`put_tables`](Self::put_tables) and a
    /// [table source](Self::set_table_source), and by a write of a FrameSet
    /// that needs a look-up table to describe an axis.
    /// Such a write, with [TabOK](Self::tab_ok) positive, uses the `-TAB`
    /// algorithm and stores the table under the extension name `WCS-TAB`;
    /// see [TabOK](Self::tab_ok) for how to write it out.
    #[doc(alias = "astGetTables")]
    pub fn get_tables(&self) -> Option<crate::keymap::KeyMap> {
        self.tables.as_ref().filter(|t| t.map_size() > 0).cloned()
    }

    /// Remove the tables named in `key`, if they are stored.
    ///
    /// `key` is a single name or a list of names separated by commas, in
    /// which `\,` stands for a comma within a name.
    /// A blank `key` removes every table.
    /// A name is matched as a [`KeyMap`](crate::keymap::KeyMap) key is,
    /// case included and trailing spaces ignored, and a name with no table
    /// is not an error.
    ///
    /// # Errors
    ///
    /// Passes on the error of
    /// [`KeyMap::map_remove`](crate::keymap::KeyMap::map_remove), of which
    /// there is none in practice.
    #[doc(alias = "astRemoveTables")]
    pub fn remove_tables(&mut self, key: &str) -> AstResult<()> {
        let Some(tables) = self.tables.as_mut() else {
            return Ok(());
        };
        if crate::keymap::table::chr_len(key) == 0 {
            while tables.map_size() > 0 {
                let first = tables.map_key(0)?;
                tables.map_remove(&first)?;
            }
        } else {
            for word in chr_split_c(key, ',') {
                tables.map_remove(&word)?;
            }
        }
        Ok(())
    }

    /// Format the cards as a FITS header string.
    ///
    /// Each card a read has not consumed is formatted as an 80-character
    /// line, with [FitsDigits](Self::fits_digits) and
    /// [FitsRounding](Self::fits_rounding), and the lines are joined with
    /// newlines.
    /// An `END` card held in the FitsChan, as [`from_header`](Self::from_header)
    /// and [`put_cards`](Self::put_cards) keep one, is written where it is.
    /// A final `END` card is added only when the FitsChan holds none, as for
    /// a FitsChan filled by the `set_fits_*` methods.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Bdfts`] if a card cannot be formatted: a string value
    /// too long for a card, or a floating value that cannot be encoded.
    pub fn to_header(&self) -> AstResult<String> {
        let visible: Vec<&FitsCard> = self
            .cards
            .iter()
            .filter(|card| !card.is_consumed())
            .collect();
        let has_end = visible.iter().any(|card| card.keyword == "END");
        let mut lines = visible
            .iter()
            .map(|card| {
                card.format_with_digits_and_rounding(
                    self.fits_digits,
                    self.fits_rounding() as usize,
                )
            })
            .collect::<AstResult<Vec<String>>>()?;
        if !has_end {
            lines.push(format!("{:<80}", "END"));
        }
        Ok(lines.join("\n"))
    }

    // -----------------------------------------------------------------------
    // Typed getters
    // -----------------------------------------------------------------------

    /// C's `MAKE_FGET`: find the card with keyword `name` (the card after
    /// the current one if it has that keyword, otherwise the first that does,
    /// `name` being a template as for [`find_fits`](Self::find_fits)) and
    /// make it current, or, with no `name`, use the current card; then
    /// convert its value. A missing keyword leaves the FitsChan at
    /// end-of-file and gives `None`.
    fn get_value(
        &mut self,
        name: Option<&str>,
        want: Want,
        method: &'static str,
    ) -> AstResult<Option<Got>> {
        self.read_from_source()?;
        let lname = match name {
            Some(name) => {
                let (lname, _) = self.split_name(name, method)?;
                Some(lname)
            }
            None => None,
        };
        let context = |e: AstError, lname: &Option<String>| match lname {
            Some(n) if !n.is_empty() => AstError::new(
                e.code(),
                format!("{e}\n{method}(FitsChan): Cannot get value for FITS keyword '{n}'."),
            ),
            _ => e,
        };
        if let Some(n) = &lname
            && !self
                .cards
                .search_card(n, method)
                .map_err(|e| context(e, &lname))?
        {
            return Ok(None);
        }
        let current = self.cards.current();
        let card_name = current.map(|c| c.c_name().to_string()).unwrap_or_default();
        match card_value::cnv_type(
            current.map(|c| &c.value),
            want,
            self.fits_rounding() as usize,
            &card_name,
        )
        .map_err(|e| context(e, &lname))?
        {
            Some(got) => Ok(Some(got)),
            None => Err(context(
                AstError::new(
                    ErrorCode::Ftcnv,
                    format!(
                        "{method}(FitsChan): Cannot convert FITS keyword '{card_name}' to {}.",
                        want.c_name()
                    ),
                ),
                &lname,
            )),
        }
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as an `i32`.
    ///
    /// The card is found, and the value converted, as by
    /// [`get_fits_f`](Self::get_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`get_fits_f`](Self::get_fits_f).
    #[doc(alias = "astGetFits")]
    pub fn get_fits_i(&mut self, name: Option<&str>) -> AstResult<Option<i32>> {
        Ok(match self.get_value(name, Want::Int, "astGetFitsI")? {
            Some(Got::Int(v)) => Some(v),
            _ => None,
        })
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as an `i64`.
    ///
    /// The card is found, and the value converted, as by
    /// [`get_fits_f`](Self::get_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`get_fits_f`](Self::get_fits_f).
    #[doc(alias = "astGetFits")]
    pub fn get_fits_k(&mut self, name: Option<&str>) -> AstResult<Option<i64>> {
        Ok(match self.get_value(name, Want::KInt, "astGetFitsK")? {
            Some(Got::KInt(v)) => Some(v),
            _ => None,
        })
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as an `f64`.
    ///
    /// `name` is read as a header card: its first 8 characters are the
    /// keyword, and the rest, up to 80 characters in all, is ignored, so a
    /// whole card may be given.
    /// It may also be a template, as for [`find_fits`](Self::find_fits).
    /// The card after the current card is tried first, and if it does not
    /// have the keyword the FitsChan is searched from its first card, so
    /// reading the cards in the order they are stored is quickest.
    /// The card found becomes current, and its value is returned, converted
    /// to `f64` if it is of another type.
    /// If no card has the keyword, the result is `None` and the FitsChan is
    /// left at end-of-file.
    /// [`test_fits`](Self::test_fits) tells whether a keyword has a value
    /// before asking for it.
    ///
    /// A keyword name with an illegal character is not an error, but gives
    /// a `BadKeyName` warning (see [Warnings](Self::warning_conditions)).
    ///
    /// Each FITS data type has its own method:
    ///
    /// | Method | Type returned | FITS data type |
    /// |---|---|---|
    /// | [`get_fits_cf`](Self::get_fits_cf) | `(f64, f64)` | complex floating point (`CF`) |
    /// | [`get_fits_ci`](Self::get_fits_ci) | `(i32, i32)` | complex integer (`CI`) |
    /// | `get_fits_f` | `f64` | floating point (`F`) |
    /// | [`get_fits_i`](Self::get_fits_i) | `i32` | integer (`I`) |
    /// | [`get_fits_k`](Self::get_fits_k) | `i64` | 64-bit integer (`K`) |
    /// | [`get_fits_l`](Self::get_fits_l) | `bool` | logical (`L`) |
    /// | [`get_fits_s`](Self::get_fits_s) | `String` | string (`S`) |
    /// | [`get_fits_cn`](Self::get_fits_cn) | `String` | `CONTINUE` string (`CN`) |
    ///
    /// Complex values are returned as their real and imaginary parts.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Fundef`] if the card has no value.
    /// - [`ErrorCode::Ftcnv`] if the value cannot be converted, if the card
    ///   is a commentary card, or if `name` is `None` at end-of-file.
    /// - [`ErrorCode::Bdfts`] if the keyword name in `name` (its first 8
    ///   characters) holds an `=` before any other illegal character.
    /// - [`ErrorCode::Bdfmt`] if `name` holds an illegal field specifier.
    /// - As for [`card`](Self::card).
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::fitschan::FitsChan;
    ///
    /// let mut fc = FitsChan::from_header(
    ///     "NAXIS   =                    2\nCDELT1  =                 0.25\nEND",
    /// )?;
    /// assert_eq!(fc.get_fits_f(Some("CDELT1"))?, Some(0.25));
    /// assert_eq!(fc.get_fits_f(Some("NAXIS"))?, Some(2.0));
    /// assert_eq!(fc.get_fits_i(Some("NAXIS"))?, Some(2));
    /// assert_eq!(fc.get_fits_f(Some("CRVAL1"))?, None);
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    ///
    /// # See also
    ///
    /// `astGetFits<X>`, one function per FITS data type `<X>`, as in the
    /// table above.
    #[doc(alias = "astGetFits")]
    pub fn get_fits_f(&mut self, name: Option<&str>) -> AstResult<Option<f64>> {
        Ok(match self.get_value(name, Want::Float, "astGetFitsF")? {
            Some(Got::Float(v)) => Some(v),
            _ => None,
        })
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as a string.
    ///
    /// The card is found, and the value converted, as by
    /// [`get_fits_f`](Self::get_fits_f).
    /// The FITS standard pads a string shorter than 8 characters with
    /// spaces to 8, so a string of 8 characters or fewer, trailing spaces
    /// included, is returned without its trailing spaces; a longer string
    /// keeps them.
    ///
    /// # Errors
    ///
    /// As for [`get_fits_f`](Self::get_fits_f).
    #[doc(alias = "astGetFits")]
    pub fn get_fits_s(&mut self, name: Option<&str>) -> AstResult<Option<String>> {
        Ok(match self.get_value(name, Want::String, "astGetFitsS")? {
            Some(Got::Str(v)) => Some(if v.len() <= 8 {
                v.trim_end_matches(' ').to_string()
            } else {
                v
            }),
            _ => None,
        })
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as a `CONTINUE` string: a string written without an equals
    /// sign, as the continuation of a long string is.
    ///
    /// The card is found, and the value converted, as by
    /// [`get_fits_f`](Self::get_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`get_fits_f`](Self::get_fits_f).
    #[doc(alias = "astGetFits")]
    pub fn get_fits_cn(&mut self, name: Option<&str>) -> AstResult<Option<String>> {
        Ok(
            match self.get_value(name, Want::Continue, "astGetFitsCN")? {
                Some(Got::Str(v)) => Some(v),
                _ => None,
            },
        )
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as a logical value.
    ///
    /// The card is found, and the value converted, as by
    /// [`get_fits_f`](Self::get_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`get_fits_f`](Self::get_fits_f).
    #[doc(alias = "astGetFits")]
    pub fn get_fits_l(&mut self, name: Option<&str>) -> AstResult<Option<bool>> {
        Ok(match self.get_value(name, Want::Logical, "astGetFitsL")? {
            Some(Got::Logical(v)) => Some(v),
            _ => None,
        })
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as a complex floating value: its real and imaginary parts.
    ///
    /// The card is found, and the value converted, as by
    /// [`get_fits_f`](Self::get_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`get_fits_f`](Self::get_fits_f).
    #[doc(alias = "astGetFits")]
    pub fn get_fits_cf(&mut self, name: Option<&str>) -> AstResult<Option<(f64, f64)>> {
        Ok(
            match self.get_value(name, Want::ComplexF, "astGetFitsCF")? {
                Some(Got::ComplexF(re, im)) => Some((re, im)),
                _ => None,
            },
        )
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as a complex integer value: its real and imaginary parts.
    ///
    /// The card is found, and the value converted, as by
    /// [`get_fits_f`](Self::get_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`get_fits_f`](Self::get_fits_f).
    #[doc(alias = "astGetFits")]
    pub fn get_fits_ci(&mut self, name: Option<&str>) -> AstResult<Option<(i32, i32)>> {
        Ok(
            match self.get_value(name, Want::ComplexI, "astGetFitsCI")? {
                Some(Got::ComplexI(re, im)) => Some((re, im)),
                _ => None,
            },
        )
    }

    // -----------------------------------------------------------------------
    // Typed setters
    // -----------------------------------------------------------------------

    /// C's `MAKE_FSET`: store a card with keyword `name` and `value`.
    ///
    /// `name` is read as C reads a card, so its first 8 characters are the
    /// keyword and, when `comment` is blank, any text after them the
    /// comment. A comment still blank keeps the comment of the card
    /// replaced when overwriting. The card is inserted in front of the
    /// current card or replaces it, as for [`put_fits`](Self::put_fits).
    fn set_value(
        &mut self,
        name: &str,
        value: FitsValue,
        comment: &str,
        overwrite: bool,
        method: &'static str,
    ) -> AstResult<()> {
        self.read_from_source()?;
        let (lname, lcom) = self.split_name(name, method)?;
        let com = if comment.trim().is_empty() {
            lcom
        } else {
            comment.to_string()
        };
        self.store_card(lname, value, &com, overwrite);
        Ok(())
    }

    /// [`CardList::put`], which takes the comment of the card replaced when
    /// the new one is blank. Leading and trailing spaces are removed from
    /// the comment, as C's `NewCard` does for a card with a value.
    fn store_card(&mut self, keyword: String, value: FitsValue, comment: &str, overwrite: bool) {
        let com = comment.trim().to_string();
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value,
            comment: (!com.is_empty()).then_some(com),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.cards.put(card, overwrite);
    }

    /// Store an integer value under keyword `name`, inserting a card or
    /// overwriting the current card as for [`set_fits_f`](Self::set_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`set_fits_f`](Self::set_fits_f).
    #[doc(alias = "astSetFits")]
    pub fn set_fits_i(
        &mut self,
        name: &str,
        value: i32,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(
            name,
            FitsValue::Int(i64::from(value)),
            comment,
            overwrite,
            "astSetFitsI",
        )
    }

    /// Store a 64-bit integer value under keyword `name`, inserting a card or
    /// overwriting the current card as for [`set_fits_f`](Self::set_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`set_fits_f`](Self::set_fits_f).
    #[doc(alias = "astSetFits")]
    pub fn set_fits_k(
        &mut self,
        name: &str,
        value: i64,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(
            name,
            FitsValue::KInt(value),
            comment,
            overwrite,
            "astSetFitsK",
        )
    }

    /// Store a floating value under keyword `name`, at the current card.
    ///
    /// `name` is read as a header card: its first 8 characters are the
    /// keyword, and any text after them, up to 80 characters in all, is
    /// the comment if `comment` is blank.
    /// If `comment` is blank and `name` holds no comment either, the card
    /// keeps the comment of the card it overwrites, if any; otherwise it has
    /// none.
    ///
    /// Without `overwrite`, the new card is inserted in front of the current
    /// card, which stays current.
    /// With `overwrite`, the new card replaces the current card, and the
    /// card after it becomes current.
    /// Either way, at end-of-file the card is appended, and the FitsChan
    /// stays at end-of-file.
    /// To give an existing keyword a new value, find its card with
    /// [`find_fits`](Self::find_fits), then overwrite it.
    ///
    /// The value is formatted with [FitsDigits](Self::fits_digits) and
    /// [FitsRounding](Self::fits_rounding) when the card is written out.
    /// A keyword name with an illegal character is not an error, but gives
    /// a `BadKeyName` warning (see [Warnings](Self::warning_conditions)).
    ///
    /// Each FITS data type has its own method:
    ///
    /// | Method | Type stored | FITS data type |
    /// |---|---|---|
    /// | [`set_fits_cf`](Self::set_fits_cf) | `(f64, f64)` | complex floating point (`CF`) |
    /// | [`set_fits_ci`](Self::set_fits_ci) | `(i32, i32)` | complex integer (`CI`) |
    /// | `set_fits_f` | `f64` | floating point (`F`) |
    /// | [`set_fits_i`](Self::set_fits_i) | `i32` | integer (`I`) |
    /// | [`set_fits_k`](Self::set_fits_k) | `i64` | 64-bit integer (`K`) |
    /// | [`set_fits_l`](Self::set_fits_l) | `bool` | logical (`L`) |
    /// | [`set_fits_s`](Self::set_fits_s) | `&str` | string (`S`) |
    /// | [`set_fits_cn`](Self::set_fits_cn) | `&str` | `CONTINUE` string (`CN`) |
    ///
    /// Complex values are given as their real and imaginary parts.
    /// [`set_fits_u`](Self::set_fits_u) stores a keyword with no value, and
    /// [`set_fits_cm`](Self::set_fits_cm) a commentary card with a blank
    /// keyword.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bdfts`] if the keyword name in `name` (its first 8
    ///   characters) holds an `=` before any other illegal character.
    /// - As for [`card`](Self::card).
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::fitschan::FitsChan;
    ///
    /// let mut fc = FitsChan::new().unwrap();
    /// fc.set_fits_f("CDELT1", 0.5, "Degrees per pixel", false)?;
    /// fc.set_fits_s("CTYPE1", "RA---TAN", "", false)?;
    /// // Overwrite the first card, keeping its comment.
    /// fc.clear_card()?;
    /// fc.set_fits_f("CDELT1", 0.25, "", true)?;
    /// assert_eq!(fc.get_fits_f(Some("CDELT1"))?, Some(0.25));
    /// assert_eq!(fc.card_comm()?, "Degrees per pixel");
    /// assert_eq!(fc.ncard()?, 2);
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    ///
    /// # See also
    ///
    /// `astSetFits<X>`, one function per FITS data type `<X>`, as in the
    /// table above.
    #[doc(alias = "astSetFits")]
    pub fn set_fits_f(
        &mut self,
        name: &str,
        value: f64,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(
            name,
            FitsValue::Float(value),
            comment,
            overwrite,
            "astSetFitsF",
        )
    }

    /// Store a string value under keyword `name`, inserting a card or
    /// overwriting the current card as for [`set_fits_f`](Self::set_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`set_fits_f`](Self::set_fits_f).
    #[doc(alias = "astSetFits")]
    pub fn set_fits_s(
        &mut self,
        name: &str,
        value: &str,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(
            name,
            FitsValue::String(value.to_string()),
            comment,
            overwrite,
            "astSetFitsS",
        )
    }

    /// Store a `CONTINUE` string under keyword `name`, a string written
    /// without an equals sign, inserting a card or overwriting the current
    /// card as for [`set_fits_f`](Self::set_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`set_fits_f`](Self::set_fits_f).
    #[doc(alias = "astSetFits")]
    pub fn set_fits_cn(
        &mut self,
        name: &str,
        value: &str,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(
            name,
            FitsValue::Continue(value.to_string()),
            comment,
            overwrite,
            "astSetFitsCN",
        )
    }

    /// Store a logical value under keyword `name`, inserting a card or
    /// overwriting the current card as for [`set_fits_f`](Self::set_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`set_fits_f`](Self::set_fits_f).
    #[doc(alias = "astSetFits")]
    pub fn set_fits_l(
        &mut self,
        name: &str,
        value: bool,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(
            name,
            FitsValue::Logical(value),
            comment,
            overwrite,
            "astSetFitsL",
        )
    }

    /// Store a complex floating value, given as its real and imaginary
    /// parts, under keyword `name`, inserting a card or overwriting
    /// the current card as for [`set_fits_f`](Self::set_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`set_fits_f`](Self::set_fits_f).
    #[doc(alias = "astSetFits")]
    pub fn set_fits_cf(
        &mut self,
        name: &str,
        value: (f64, f64),
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(
            name,
            FitsValue::ComplexFloat(value.0, value.1),
            comment,
            overwrite,
            "astSetFitsCF",
        )
    }

    /// Store a complex integer value, given as its real and imaginary
    /// parts, under keyword `name`, inserting a card or overwriting
    /// the current card as for [`set_fits_f`](Self::set_fits_f).
    ///
    /// # Errors
    ///
    /// As for [`set_fits_f`](Self::set_fits_f).
    #[doc(alias = "astSetFits")]
    pub fn set_fits_ci(
        &mut self,
        name: &str,
        value: (i32, i32),
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(
            name,
            FitsValue::ComplexInt(i64::from(value.0), i64::from(value.1)),
            comment,
            overwrite,
            "astSetFitsCI",
        )
    }

    /// Store keyword `name` with no value, inserting a card or overwriting
    /// the current card as for [`set_fits_f`](Self::set_fits_f).
    ///
    /// The keyword's card has an equals sign but no value, which
    /// [`test_fits`](Self::test_fits) reports as `Some(false)` and the
    /// `get_fits_*` methods as [`ErrorCode::Fundef`].
    ///
    /// # Errors
    ///
    /// As for [`set_fits_f`](Self::set_fits_f).
    #[doc(alias = "astSetFitsU")]
    pub fn set_fits_u(&mut self, name: &str, comment: &str, overwrite: bool) -> AstResult<()> {
        self.set_value(name, FitsValue::Undef, comment, overwrite, "astSetFitsU")
    }

    /// Store a commentary card, one with a blank keyword and no equals sign,
    /// holding the text `comment`.
    ///
    /// The card is inserted in front of the current card, or overwrites it,
    /// as for [`set_fits_f`](Self::set_fits_f).
    /// A blank `comment` gives a wholly blank card.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "astSetFitsCM")]
    pub fn set_fits_cm(&mut self, comment: &str, overwrite: bool) -> AstResult<()> {
        self.set_fits_com("", comment, overwrite)
    }

    /// C's `SetFitsCom`: store a commentary card with keyword `name`. When
    /// overwriting a card with the same keyword, the card keeps its type and
    /// value and takes the new comment.
    fn set_fits_com(&mut self, name: &str, comment: &str, overwrite: bool) -> AstResult<()> {
        self.read_from_source()?;
        let (lname, _) = self.split_name(name, "astSetFitsCom")?;
        let com = if comment.trim().is_empty() {
            None
        } else {
            Some(comment.to_string())
        };
        let same = self.cards.current().is_some_and(|c| c.keyword == lname);
        let card = if !overwrite || !same {
            FitsCard {
                keyword: lname.to_uppercase(),
                value: FitsValue::Comment(com.unwrap_or_default()),
                comment: None,
                flags: crate::fitschan::card::CardFlags::default(),
            }
        } else {
            let old = self.cards.current().expect("an overwritten card").clone();
            match old.value {
                FitsValue::Comment(_) => FitsCard {
                    value: FitsValue::Comment(com.unwrap_or_default()),
                    ..old
                },
                _ => FitsCard {
                    comment: com.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()),
                    ..old
                },
            }
        };
        self.cards.ins_card(card, overwrite);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Attribute accessors
    // -----------------------------------------------------------------------

    /// Set [Encoding](Self::encoding).
    pub fn set_encoding(&mut self, enc: Encoding) {
        self.encoding = Some(enc);
    }

    /// Clear [Encoding](Self::encoding), so that it is chosen from the cards
    /// again.
    pub fn clear_encoding(&mut self) {
        self.encoding = None;
    }

    /// Whether [Encoding](Self::encoding) has been set.
    pub fn test_encoding(&self) -> bool {
        self.encoding.is_some()
    }

    /// The Encoding attribute: the conventions a read or write uses to
    /// describe objects as FITS header cards.
    ///
    /// [`Encoding`] describes each encoding.
    /// Setting it ensures that encoding is used whatever cards are already
    /// present.
    /// When writing, choose the encoding by the software expected to read
    /// the header; if that is not known, writing the object several times,
    /// in different encodings, may help.
    ///
    /// Default: chosen, each time it is asked for, from the keywords of
    /// the cards a read has not consumed, so that a read or write follows
    /// the conventions already in use.
    /// The first of these that holds gives the encoding:
    ///
    /// 1. A keyword beginning `BEGAST`: `NATIVE`.
    /// 2. A `DELTAV` keyword and one of `VELO-OBS`, `VELO-HEL`, `VELO-EAR`,
    ///    `VELO-LSR` or `VLSR`: `FITS-CLASS`.
    /// 3. A CTYPE keyword holding a spectral axis type as AIPS and AIPS++
    ///    write it (such as `FELO-LSR`): `FITS-AIPS++` if there is also a
    ///    `CDi_j`, `PROJPi`, `LONPOLE` or `LATPOLE` keyword, and `FITS-AIPS`
    ///    otherwise.
    /// 4. A `PLTRAH` keyword: `DSS`.
    /// 5. A keyword of the form `PCiiijjj`: `FITS-PC`.
    /// 6. A keyword of the form `CDiiijjj`: `FITS-IRAF`.
    /// 7. A `CDi_j` keyword: `FITS-IRAF` if there is also a `RADECSYS`
    ///    keyword without `RADESYS`, a `PROJPi` keyword without any `PVi_m`,
    ///    or a `CjVALi` keyword, and `FITS-WCS` otherwise.
    /// 8. No `CDi_j` or `PCi_j` keyword, and a `RADECSYS` keyword without
    ///    `RADESYS`, a `PROJPi` keyword without any `PVi_m`, or a `CjVALi`
    ///    keyword: `FITS-PC`.
    /// 9. A `CROTAi` keyword: `FITS-AIPS`.
    /// 10. A `CRVALi` keyword: `FITS-WCS`.
    /// 11. Otherwise, as for an empty FitsChan: `NATIVE`.
    ///
    /// Each case but 1 and 4 also needs a CTYPE, CRPIX or CRVAL keyword,
    /// for the primary or an alternate axis description, and is passed
    /// over without one.
    /// For the default, a [source](Self::set_source) not yet read is read
    /// first, so its cards are among those the default is chosen from.
    ///
    /// Reading a FrameSet usually consumes the keywords the default was
    /// chosen from, so the default after a read is usually different; to
    /// write in the encoding that was read, ask for the default before the
    /// read and set it.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card).
    #[doc(alias = "Encoding")]
    pub fn encoding(&mut self) -> AstResult<Encoding> {
        match self.encoding {
            Some(enc) => Ok(enc),
            None => {
                self.read_from_source()?;
                Ok(guess_encoding::guess_encoding(&self.cards))
            }
        }
    }

    /// Set [FitsDigits](Self::fits_digits).
    pub fn set_fits_digits(&mut self, digits: i32) {
        self.fits_digits = digits;
    }

    /// The FitsDigits attribute: the number of significant decimal digits a
    /// floating point value is formatted with in a header card.
    ///
    /// The default loses no information from an `f64`, which is usually
    /// what is wanted.
    /// The FITS standard, however, recommends that a value, with its sign,
    /// decimal point and exponent, be no wider than 20 characters.
    /// To keep to that, set a negative value such as -15: its absolute
    /// value is then the most digits used, and fewer are used where the
    /// value would otherwise be too wide, so the number used depends on the
    /// value.
    ///
    /// It applies whenever cards are formatted: by
    /// [`find_fits`](Self::find_fits), [`to_header`](Self::to_header) and
    /// [`show_fits`](Self::show_fits), and when the cards are written to the
    /// sink.
    ///
    /// Default: 17.
    #[doc(alias = "FitsDigits")]
    pub fn fits_digits(&self) -> i32 {
        self.fits_digits
    }

    /// Whether [FitsDigits](Self::fits_digits) differs from its default.
    ///
    /// This tests the value rather than whether one was set, so a
    /// FitsDigits set to 17 tests as unset.
    pub fn test_fits_digits(&self) -> bool {
        self.fits_digits != DEFAULT_FITS_DIGITS
    }

    /// Clear [FitsDigits](Self::fits_digits), restoring its default.
    pub fn clear_fits_digits(&mut self) {
        self.fits_digits = DEFAULT_FITS_DIGITS;
    }

    /// Set [FitsRounding](Self::fits_rounding).
    /// A negative value is stored as 0.
    pub fn set_fits_rounding(&mut self, fitsrnd: i32) {
        self.fits_rounding = Some(fitsrnd.max(0));
    }

    /// The FitsRounding attribute: how a floating point value is rounded
    /// when it is formatted in a header card.
    ///
    /// The value is first formatted with [FitsDigits](Self::fits_digits)
    /// digits.
    /// If that holds a run of 4 or more adjacent `9`s or `0`s that extends
    /// beyond the significant digit FitsRounding gives, it is cut at the
    /// start of the run, the last digit kept being rounded up for a run of
    /// `9`s.
    /// With FitsRounding 10, for instance, only runs extending beyond the
    /// tenth significant digit are rounded.
    ///
    /// Default: 10.
    #[doc(alias = "FitsRounding")]
    pub fn fits_rounding(&self) -> i32 {
        self.fits_rounding.unwrap_or(10)
    }

    /// Whether [FitsRounding](Self::fits_rounding) has been set.
    pub fn test_fits_rounding(&self) -> bool {
        self.fits_rounding.is_some()
    }

    /// Clear [FitsRounding](Self::fits_rounding), restoring its default.
    pub fn clear_fits_rounding(&mut self) {
        self.fits_rounding = None;
    }

    /// The Clean attribute: whether a read that fails still removes the
    /// cards it used.
    ///
    /// A successful read always removes the cards that described the object
    /// it returns.
    /// When a read fails, because cards have illegal values or required
    /// cards are missing, no cards are removed unless Clean is true, in
    /// which case the cards used in the attempt are removed.
    /// This cleans a FitsChan of WCS cards even when they do not form a
    /// legal description, as [`purge_wcs`](Self::purge_wcs) does.
    /// While Clean is true no warnings are recorded, as cards or in
    /// [`warning_texts`](Channel::warning_texts), whatever
    /// [Warnings](Self::warning_conditions) holds.
    ///
    /// Default: false.
    #[doc(alias = "Clean")]
    pub fn clean(&self) -> bool {
        self.clean.unwrap_or(false)
    }

    /// Set [Clean](Self::clean).
    pub fn set_clean(&mut self, clean: bool) {
        self.clean = Some(clean);
    }

    /// Whether [Clean](Self::clean) is set.
    pub fn test_clean(&self) -> bool {
        self.clean.is_some()
    }

    /// Clear [Clean](Self::clean).
    pub fn clear_clean(&mut self) {
        self.clean = None;
    }

    /// The SipOK attribute: whether a FITS-WCS write may describe
    /// distortion with the SIP keywords, a scheme first defined for the
    /// Spitzer Space Telescope
    /// (<http://irsa.ipac.caltech.edu/data/SPITZER/docs/files/spitzer/shupeADASS.pdf>).
    ///
    /// A read of a FITS-WCS header with SIP keywords always includes a
    /// PolyMap for them, whatever SipOK holds, just before the MatrixMap of
    /// the PC or CD matrix.
    ///
    /// A write includes SIP keywords when SipOK is true and the pixel to
    /// sky Mapping can be arranged as the SIP scheme needs: a PolyMap
    /// somewhere before the WcsMap, with only a shift of origin, scaling or
    /// rotation between them, and only a shift of origin and scaling before
    /// the PolyMap.
    /// Scaling before the PolyMap and shifts after it are absorbed into a
    /// modified PolyMap, and any shift, scaling or rotation in the PolyMap
    /// itself is moved out, a shift into the Mapping that gives CRPIX and
    /// scaling or rotation into the one that gives the CD matrix.
    /// SIP keywords are written only for the axes of a SkyFrame.
    /// When SipOK is false, or the Mapping cannot be so arranged, the write
    /// uses a linear approximation to the pixel to intermediate world
    /// coordinates Mapping, and writes nothing if that departs from
    /// linearity by more than [FitsTol](Self::fits_tol).
    ///
    /// SIP is independent of the TPN and TPV distortion schemes (see
    /// [PolyTan](Self::poly_tan)); a header could hold both, though that is
    /// unlikely.
    ///
    /// Default: true.
    #[doc(alias = "SipOK")]
    pub fn sip_ok(&self) -> bool {
        self.sip_ok.unwrap_or(true)
    }

    /// Set [SipOK](Self::sip_ok).
    pub fn set_sip_ok(&mut self, sip_ok: bool) {
        self.sip_ok = Some(sip_ok);
    }

    /// Whether [SipOK](Self::sip_ok) is set.
    pub fn test_sip_ok(&self) -> bool {
        self.sip_ok.is_some()
    }

    /// Clear [SipOK](Self::sip_ok).
    pub fn clear_sip_ok(&mut self) {
        self.sip_ok = None;
    }

    /// The attributes a write reads while building a FitsStore.
    fn write_attrs(&self) -> fset_to_store::WriteAttrs {
        fset_to_store::WriteAttrs {
            fits_tol: self.fits_tol(),
            sip_ok: self.sip_ok(),
            timesys: self.cards.get_string("TIMESYS").map(|v| {
                v.trim_end_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b')
                    .to_string()
            }),
            // C's `Write` gives `FsetToStore` its method and class names
            // the wrong way round, so a warning made an error by Strict names
            // the method "FitsChan".
            warnings: std::rc::Rc::new(std::cell::RefCell::new(self.warn_list("FitsChan"))),
            alt_axes: self.alt_axes(),
            fits_axis_order: self.fits_axis_order().to_string(),
            crval_source: Some(std::rc::Rc::new(self.clone())),
            tab_ok: self.tab_ok(),
            force_tab: self.force_tab() != 0,
            tables: Default::default(),
        }
    }

    /// Set [CDMatrix](Self::cd_matrix).
    pub fn set_cd_matrix(&mut self, use_cd: bool) {
        self.cd_matrix = Some(use_cd);
    }

    /// Whether [CDMatrix](Self::cd_matrix) has been set.
    pub fn test_cd_matrix(&self) -> bool {
        self.cd_matrix.is_some()
    }

    /// Clear [CDMatrix](Self::cd_matrix), restoring its default.
    pub fn clear_cd_matrix(&mut self) {
        self.cd_matrix = None;
    }

    /// Set [SipReplace](Self::sip_replace).
    pub fn set_sip_replace(&mut self, v: bool) {
        self.sip_replace = Some(v);
    }

    /// The SipReplace attribute: whether a read of a FITS-WCS header with
    /// SIP keywords replaces the inverse transformation they describe.
    ///
    /// When true, the SIP keywords describing the inverse transformation,
    /// from world to pixel coordinates, are ignored, and a new inverse is
    /// found by fitting the forward transformation.
    /// When false, the inverse keywords are used as they are.
    /// The SIP scheme is described at
    /// <http://irsa.ipac.caltech.edu/data/SPITZER/docs/files/spitzer/shupeADASS.pdf>.
    ///
    /// Default: true.
    #[doc(alias = "SipReplace")]
    pub fn sip_replace(&self) -> bool {
        self.sip_replace.unwrap_or(true)
    }

    /// Whether [SipReplace](Self::sip_replace) has been set.
    pub fn test_sip_replace(&self) -> bool {
        self.sip_replace.is_some()
    }

    /// Clear [SipReplace](Self::sip_replace), restoring its default.
    pub fn clear_sip_replace(&mut self) {
        self.sip_replace = None;
    }

    /// Set [CarLin](Self::car_lin).
    pub fn set_car_lin(&mut self, v: bool) {
        self.car_lin = Some(v);
    }

    /// The CarLin attribute: whether a read of a foreign encoding takes a
    /// `CAR` (plate carrée) projection to be a linear transformation from
    /// pixel to celestial coordinates.
    ///
    /// FITS-WCS paper II ("Representation of Celestial Coordinates in
    /// FITS", Calabretta and Greisen) makes a `CAR` projection a linear
    /// Mapping from pixel coordinates to native spherical coordinates, which
    /// a 3-D rotation then takes to celestial coordinates, so that the whole
    /// Mapping is not linear.
    /// When CarLin is false the header is read that way.
    /// When it is true the rotation is left out, which suits some older data
    /// that claim a `CAR` projection but do not follow the paper.
    ///
    /// Default: false.
    #[doc(alias = "CarLin")]
    pub fn car_lin(&self) -> bool {
        self.car_lin.unwrap_or(false)
    }

    /// Whether [CarLin](Self::car_lin) has been set.
    pub fn test_car_lin(&self) -> bool {
        self.car_lin.is_some()
    }

    /// Clear [CarLin](Self::car_lin), restoring its default.
    pub fn clear_car_lin(&mut self) {
        self.car_lin = None;
    }

    /// Set [FitsTol](Self::fits_tol).
    /// A negative value is stored as 0.
    pub fn set_fits_tol(&mut self, tol: f64) {
        self.fits_tol = Some(tol.max(0.0));
    }

    /// The FitsTol attribute: the largest departure from linearity, in
    /// pixels, that a write of a foreign encoding allows on any axis of the
    /// Mapping from pixel to intermediate world coordinates.
    ///
    /// If the Mapping is linear to within FitsTol, a linear approximation to
    /// it gives the keyword values written (CRPIX, CD, PC, CDELT and the
    /// like); if not, the write fails: [`Channel::write`] writes nothing and
    /// returns 0.
    ///
    /// Default: 0.1, a tenth of a pixel.
    #[doc(alias = "FitsTol")]
    pub fn fits_tol(&self) -> f64 {
        self.fits_tol.unwrap_or(fset_to_store::FITS_TOL_DEFAULT)
    }

    /// Whether [FitsTol](Self::fits_tol) has been set.
    pub fn test_fits_tol(&self) -> bool {
        self.fits_tol.is_some()
    }

    /// Clear [FitsTol](Self::fits_tol), restoring its default.
    pub fn clear_fits_tol(&mut self) {
        self.fits_tol = None;
    }

    /// Set [DefB1950](Self::def_b1950).
    pub fn set_def_b1950(&mut self, v: bool) {
        self.def_b1950 = Some(v);
    }

    /// The DefB1950 attribute: whether a read of a foreign encoding takes
    /// RA and Dec axes with no reference frame or equinox in the header to
    /// be FK4 B1950, rather than ICRS.
    ///
    /// It is used only when the header has RA and Dec axes and says
    /// nothing of their reference frame or equinox.
    ///
    /// Default: false when the [Encoding](Self::encoding) is `FITS-WCS`,
    /// and true for every other encoding.
    /// When the Encoding is not set, its default is used, for which a
    /// [source](Self::set_source) not yet read is read first.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card), when DefB1950 is not set.
    #[doc(alias = "DefB1950")]
    pub fn def_b1950(&mut self) -> AstResult<bool> {
        match self.def_b1950 {
            Some(v) => Ok(v),
            None => Ok(self.encoding()? != Encoding::FitsWcs),
        }
    }

    /// Whether [DefB1950](Self::def_b1950) has been set.
    pub fn test_def_b1950(&self) -> bool {
        self.def_b1950.is_some()
    }

    /// Clear [DefB1950](Self::def_b1950), restoring its default.
    pub fn clear_def_b1950(&mut self) {
        self.def_b1950 = None;
    }

    /// Set [PolyTan](Self::poly_tan).
    pub fn set_poly_tan(&mut self, v: i32) {
        self.poly_tan = Some(v);
    }

    /// The PolyTan attribute: how a read of a foreign encoding treats a
    /// `TAN` projection with `PVi_m` keywords.
    ///
    /// An early draft of FITS-WCS paper II gave the `PVi_m` keywords of a
    /// `TAN` projection the coefficients of a polynomial distortion of the
    /// intermediate world coordinates.
    /// The published standard dropped the convention, and gives the first
    /// five `PVi_m` keywords of the longitude axis other meanings, but it is
    /// now in the registry of FITS conventions
    /// (<http://fits.gsfc.nasa.gov/registry/tpvwcs.html>) and is used by the
    /// SCAMP program and the Dark Energy Camera.
    /// A projection read under the convention is the AST `TPN` projection.
    ///
    /// - 0: the header follows the published standard.
    /// - Positive: the header follows the distorted `TAN` convention.
    /// - Negative: the header decides: the distorted `TAN` convention is used
    ///   if there are `PVi_m` keywords for the latitude axis, or for the
    ///   longitude axis with `m` greater than 4, and the standard otherwise.
    ///   Distortion coefficients that are all zero on either axis are then
    ///   ignored, with a `BadPV` warning.
    ///
    /// Default: -1.
    #[doc(alias = "PolyTan")]
    pub fn poly_tan(&self) -> i32 {
        self.poly_tan.unwrap_or(-1)
    }

    /// Whether [PolyTan](Self::poly_tan) has been set.
    pub fn test_poly_tan(&self) -> bool {
        self.poly_tan.is_some()
    }

    /// Clear [PolyTan](Self::poly_tan), restoring its default.
    pub fn clear_poly_tan(&mut self) {
        self.poly_tan = None;
    }

    /// Set [IgnoreBadAlt](Self::ignore_bad_alt).
    pub fn set_ignore_bad_alt(&mut self, v: bool) {
        self.ignore_bad_alt = Some(v);
    }

    /// The IgnoreBadAlt attribute: whether a read leaves out an alternate
    /// FITS-WCS axis description it cannot read, rather than failing.
    ///
    /// An alternate axis description may be unreadable because it uses a
    /// feature of FITS-WCS that is not supported, or is malformed.
    /// When IgnoreBadAlt is false the read then fails with an error.
    /// When it is true the description is left out, with a `BadAlt` warning
    /// (see [Warnings](Self::warning_conditions)), and the FrameSet holds the
    /// axes that could be read.
    ///
    /// Default: false.
    #[doc(alias = "IgnoreBadAlt")]
    pub fn ignore_bad_alt(&self) -> bool {
        self.ignore_bad_alt.unwrap_or(false)
    }

    /// Whether [IgnoreBadAlt](Self::ignore_bad_alt) has been set.
    pub fn test_ignore_bad_alt(&self) -> bool {
        self.ignore_bad_alt.is_some()
    }

    /// Clear [IgnoreBadAlt](Self::ignore_bad_alt), restoring its default.
    pub fn clear_ignore_bad_alt(&mut self) {
        self.ignore_bad_alt = None;
    }

    /// Set [Warnings](Self::warning_conditions): the names of the
    /// conditions to report, separated by spaces, in any case.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Attin`] if a name is not one of
    /// [AllWarnings](Self::all_warning_conditions); the attribute is then
    /// left as it was.
    pub fn set_warning_conditions(&mut self, value: &str) -> AstResult<()> {
        warning::good_warns(value)?;
        self.warning_conditions = Some(value.to_string());
        Ok(())
    }

    /// The Warnings attribute: the conditions met when reading or writing
    /// an object or keyword that are reported as warnings.
    ///
    /// It is a list of condition names separated by spaces;
    /// [AllWarnings](Self::all_warning_conditions) lists every condition and
    /// what it means.
    /// A warning for a condition it names is added to the warnings
    /// [`warning_texts`](Channel::warning_texts) gives, and stored in the
    /// FitsChan as one or more `ASTWARN` cards.
    /// After a read or write, the `ASTWARN` cards can be found with
    /// [`find_fits`](Self::find_fits) and shown to the user; they stay in
    /// any header written out unless deleted with
    /// [`del_fits`](Self::del_fits).
    /// A warning for a condition it does not name is not recorded at all,
    /// and while [Clean](Self::clean) is true none is.
    /// With [Strict](ChannelConfig::strict) set, a warning for a condition
    /// it names is instead the error [`ErrorCode::Badin`], which ends the
    /// method that met it.
    ///
    /// Default: `"BadKeyName BadKeyValue Tnx Zpx BadPV BadCel BadMat
    /// BadCTYPE"`.
    #[doc(alias = "Warnings")]
    pub fn warning_conditions(&self) -> &str {
        self.warning_conditions
            .as_deref()
            .unwrap_or(warning::DEFAULT_WARNINGS)
    }

    /// Whether [Warnings](Self::warning_conditions) has been set.
    pub fn test_warning_conditions(&self) -> bool {
        self.warning_conditions.is_some()
    }

    /// Clear [Warnings](Self::warning_conditions), restoring its default.
    pub fn clear_warning_conditions(&mut self) {
        self.warning_conditions = None;
    }

    /// Set [AltAxes](Self::alt_axes).
    pub fn set_alt_axes(&mut self, v: AltAxes) {
        self.alt_axes = Some(v);
    }

    /// The AltAxes attribute: which Frames of the FrameSet a write
    /// describes with FITS-WCS alternate axis descriptions.
    ///
    /// The current Frame always gives the primary axis descriptions, and the
    /// base Frame never gives an alternate one.
    /// [`AltAxes`] describes the values.
    /// It is used only when the [Encoding](Self::encoding) is, or defaults
    /// to, `FITS-WCS` or `FITS-PC`.
    ///
    /// Default: [`AltAxes::All`].
    #[doc(alias = "AltAxes")]
    pub fn alt_axes(&self) -> AltAxes {
        self.alt_axes.unwrap_or_default()
    }

    /// Whether [AltAxes](Self::alt_axes) has been set.
    pub fn test_alt_axes(&self) -> bool {
        self.alt_axes.is_some()
    }

    /// Clear [AltAxes](Self::alt_axes), restoring its default.
    pub fn clear_alt_axes(&mut self) {
        self.alt_axes = None;
    }

    /// Set [Iwc](Self::iwc).
    pub fn set_iwc(&mut self, v: bool) {
        self.iwc = Some(v);
    }

    /// The Iwc attribute: whether a FrameSet read from a foreign encoding
    /// includes Frames of FITS-WCS intermediate world coordinates.
    ///
    /// When true, each axis description read adds such a Frame, with Domain
    /// `IWC` for the primary axis description and `IWCa` for alternate
    /// description `a`.
    /// FITS-WCS paper I defines intermediate world coordinates as a
    /// Cartesian system with one axis per world axis, the result of the
    /// matrix given by the `PCi_j`, `CDi_j` or similar keywords.
    /// For a 2-D celestial projection, for instance, they are offsets in
    /// degrees from the reference point in the plane of projection.
    ///
    /// Default: false.
    #[doc(alias = "Iwc")]
    pub fn iwc(&self) -> bool {
        self.iwc.unwrap_or(false)
    }

    /// Whether [Iwc](Self::iwc) has been set.
    pub fn test_iwc(&self) -> bool {
        self.iwc.is_some()
    }

    /// Clear [Iwc](Self::iwc), restoring its default.
    pub fn clear_iwc(&mut self) {
        self.iwc = None;
    }

    /// Set [FitsAxisOrder](Self::fits_axis_order).
    /// The value is checked when it is used, by a write.
    pub fn set_fits_axis_order(&mut self, v: &str) {
        self.fits_axis_order = Some(v.to_string());
    }

    /// The FitsAxisOrder attribute: the order of the WCS axes in a
    /// FITS-WCS header that a write makes.
    ///
    /// - `<auto>`: the i'th WCS axis of the header is the WCS axis most
    ///   nearly parallel to the i'th pixel axis.
    /// - `<copy>`: the i'th WCS axis of the header is the i'th axis of the
    ///   current Frame.
    /// - A list of axis Symbols separated by spaces: the axes of the current
    ///   Frame appear in the order their Symbols appear in the list.
    ///   Words that are not the Symbol of a current Frame axis are ignored,
    ///   but a write fails with [`ErrorCode::Attin`] if the list leaves out
    ///   an axis or names one twice.
    ///
    /// Default: `<auto>`.
    #[doc(alias = "FitsAxisOrder")]
    pub fn fits_axis_order(&self) -> &str {
        self.fits_axis_order.as_deref().unwrap_or("<auto>")
    }

    /// Whether [FitsAxisOrder](Self::fits_axis_order) has been set.
    pub fn test_fits_axis_order(&self) -> bool {
        self.fits_axis_order.is_some()
    }

    /// Clear [FitsAxisOrder](Self::fits_axis_order), restoring its default.
    pub fn clear_fits_axis_order(&mut self) {
        self.fits_axis_order = None;
    }

    /// Set [TabOK](Self::tab_ok).
    pub fn set_tab_ok(&mut self, v: i32) {
        self.tab_ok = Some(v);
    }

    /// The TabOK attribute: whether the `-TAB` algorithm of FITS-WCS paper
    /// III is supported, and if so the version number of the tables a write
    /// makes.
    ///
    /// Axes described with the `-TAB` algorithm, which have `-TAB` in their
    /// CTYPE, take their values from a look-up table held in a FITS binary
    /// table.
    /// When TabOK is zero or negative, the algorithm is not supported.
    /// A write never uses it, and a FrameSet that needs it is not written:
    /// [`Channel::write`] returns 0.
    /// A read does not look up tables, and takes the `-TAB` code in CTYPE as
    /// it stands:
    ///
    /// - a spectral axis, such as `WAVE-TAB`, makes the read fail with
    ///   [`ErrorCode::Bdfts`], as `-TAB` is not a spectral algorithm it can
    ///   implement;
    /// - celestial axes, such as `RA---TAB` and `DEC--TAB`, make it fail with
    ///   [`ErrorCode::Bdfts`], as `TAB` is not a projection;
    /// - any other axis, such as `FOO--TAB`, is read with no error, without
    ///   a look-up table.
    ///
    /// When TabOK is positive, a write uses the algorithm for an axis if no
    /// other FITS-WCS algorithm can describe it (or whenever it can, with
    /// [ForceTab](Self::force_tab)).
    /// The table of coordinates and index vectors is stored in the FitsChan
    /// as a [`FitsTable`](crate::fitstable::FitsTable) under the extension
    /// name `WCS-TAB`, which the caller should look for with
    /// [`get_tables`](Self::get_tables) after the write and copy, data and
    /// headers, into a new FITS binary table extension.
    /// The coordinates are in a column named `COORDS` and any index vectors,
    /// made only where needed, in columns named `INDEX<i>`, `<i>` being the
    /// FITS-WCS axis.
    /// The table's `EXTVER` is TabOK, which lets several such extensions
    /// share a name; no `EXTLEVEL` is stored, so it should be taken as 1.
    ///
    /// When TabOK is positive, a read makes a FrameSet from a header that
    /// uses the algorithm, provided the tables it needs are available:
    /// stored beforehand with [`put_table`](Self::put_table) or
    /// [`put_tables`](Self::put_tables), or supplied when needed by a
    /// [table source](Self::set_table_source).
    /// Only 1-dimensional (separable) `-TAB` axes can be read.
    ///
    /// Default: 0.
    #[doc(alias = "TabOK")]
    pub fn tab_ok(&self) -> i32 {
        self.tab_ok.unwrap_or(0)
    }

    /// Whether [TabOK](Self::tab_ok) has been set.
    pub fn test_tab_ok(&self) -> bool {
        self.tab_ok.is_some()
    }

    /// Clear [TabOK](Self::tab_ok), restoring its default.
    pub fn clear_tab_ok(&mut self) {
        self.tab_ok = None;
    }

    /// Set [ForceTab](Self::force_tab).
    pub fn set_force_tab(&mut self, v: i32) {
        self.force_tab = Some(v);
    }

    /// The ForceTab attribute, a flag: whether a FITS-WCS write uses the
    /// `-TAB` algorithm wherever it can.
    ///
    /// It has an effect only when [TabOK](Self::tab_ok) is positive.
    /// When it is non-zero, the write uses the `-TAB` algorithm even where
    /// another algorithm could describe an axis; when it is zero, only where
    /// no other algorithm can.
    ///
    /// Default: 0.
    #[doc(alias = "ForceTab")]
    pub fn force_tab(&self) -> i32 {
        self.force_tab.unwrap_or(0)
    }

    /// Whether [ForceTab](Self::force_tab) has been set.
    pub fn test_force_tab(&self) -> bool {
        self.force_tab.is_some()
    }

    /// Clear [ForceTab](Self::force_tab), restoring its default.
    pub fn clear_force_tab(&mut self) {
        self.force_tab = None;
    }

    /// The Ident attribute: a permanent string identifying this FitsChan.
    ///
    /// A clone keeps it, and it is written to and read back from a Native
    /// dump.
    /// Default: an empty string.
    #[doc(alias = "Ident")]
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set [`ident`](Self::ident).
    pub fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    /// Whether [`ident`](Self::ident) is set.
    pub fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    /// Clear [`ident`](Self::ident).
    pub fn clear_ident(&mut self) {
        self.ident = None;
    }

    /// The ID attribute: a string identifying this FitsChan, which the library
    /// does not use.
    ///
    /// It is written to and read back from a Native dump, but unlike
    /// [`ident`](Self::ident) a clone does not keep it.
    /// Default: an empty string.
    // C's `astCopy` does not copy the ID.
    #[doc(alias = "ID")]
    pub fn id(&self) -> &str {
        self.id.get()
    }

    /// Set [`id`](Self::id).
    pub fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }

    /// Whether [`id`](Self::id) is set.
    pub fn test_id(&self) -> bool {
        self.id.test()
    }

    /// Clear [`id`](Self::id).
    pub fn clear_id(&mut self) {
        self.id.clear();
    }

    /// The read-only AllWarnings attribute: the name of every condition
    /// [Warnings](Self::warning_conditions) accepts, in lower case, with a
    /// space before and after each.
    ///
    /// The names are not case sensitive.
    /// The conditions are:
    ///
    /// - `BadAlt`: a read of a foreign encoding leaves out an alternate axis
    ///   description it cannot read, because
    ///   [IgnoreBadAlt](Self::ignore_bad_alt) is true (when it is false the
    ///   read fails instead).
    /// - `BadCel`: a read of a foreign encoding finds an unknown celestial
    ///   coordinate system in the CTYPE keywords.
    /// - `BadCTYPE`: a read of a foreign encoding finds an illegal algorithm
    ///   code in a CTYPE keyword that can be converted to an equivalent legal
    ///   code.
    /// - `BadKeyName`: a keyword name holds a character the FITS standard
    ///   does not allow.
    /// - `BadKeyValue`: the value of a keyword cannot be determined from its
    ///   card.
    /// - `BadLat`: a read of a foreign encoding finds a reference point
    ///   whose latitude is greater than 90 degrees in absolute value; 90
    ///   degrees is used.
    /// - `BadMat`: the matrix taking pixel offsets to intermediate world
    ///   coordinates, given by keywords such as `PCi_j`, `CDi_j` or `CROTA`,
    ///   cannot be inverted, for instance because a row or column is all
    ///   zero.
    ///   FITS-WCS paper I requires it to be invertible, and many operations,
    ///   such as plotting a grid, need its inverse.
    /// - `BadPV`: a read of a foreign encoding finds a `PVi_m` keyword for a
    ///   projection parameter the CTYPE projection does not use, or `PV`
    ///   values otherwise unsuited to the projection.
    /// - `BadVal`: a read of a foreign encoding cannot convert the value of a
    ///   keyword to the type expected, as when a string is given for a
    ///   floating value or the keyword has no value, being a commentary
    ///   card.
    /// - `Distortion`: a read of a foreign encoding finds a CTYPE keyword
    ///   naming a distortion code, in the "4-3-3" form of FITS-WCS paper
    ///   IV, that is not supported; the code is ignored.
    /// - `NoCTYPE`: a read of a foreign encoding uses a default CTYPE, as the
    ///   header has none.
    /// - `NoEquinox`: a read of a foreign encoding uses a default equinox, as
    ///   the header has none.
    /// - `NoRadesys`: a read of a foreign encoding uses a default reference
    ///   frame for an equatorial coordinate system, as the header has none.
    /// - `NoLonpole`: a read of a foreign encoding uses a default `LONPOLE`,
    ///   as the header has none.
    /// - `NoLatpole`: a read of a foreign encoding uses a default `LATPOLE`,
    ///   as the header has none.
    /// - `NoMjd-obs`: a read of a foreign encoding uses a default date of
    ///   observation, as the header has none.
    /// - `Tnx`: a read finds an IRAF `TNX` projection with terms that are not
    ///   supported; they are ignored, so the FrameSet may be inaccurate.
    /// - `Zpx`: a read finds an IRAF `ZPX` projection with `lngcor` or
    ///   `latcor` correction terms, which are not supported; they are
    ///   ignored, so the FrameSet may be inaccurate.
    #[doc(alias = "AllWarnings")]
    pub fn all_warning_conditions(&self) -> &'static str {
        warning::all_warnings()
    }

    /// C's `ReadCrval` (fitschan.c:26137): the reference point that the
    /// `CRVAL` cards of axis description `s` give, transformed into `wcsfrm`,
    /// or `None` if there is none. A copy of the FitsChan, with every card
    /// made available again and the encoding left to be guessed, is read; the
    /// Frame whose Ident is `s` must have as many axes as there are CRVAL
    /// cards, and a conversion to `wcsfrm` must exist.
    pub(crate) fn read_crval(
        &self,
        s: char,
        wcsfrm: &dyn crate::frames::Frame,
    ) -> Option<Vec<f64>> {
        use crate::frames::FrameRef;
        let mut fc = self.clone();
        fc.encoding = None;
        fc.cards.reset_used();
        let template = if s == ' ' {
            "CRVAL%d".to_string()
        } else {
            format!("CRVAL%d{s}")
        };
        let (n, _, hi) = fc.cards.key_fields(&template, 1);
        if n == 0 {
            return None;
        }
        let hii = usize::try_from(hi[0]).ok()?;
        let mut crval = Vec::with_capacity(hii);
        for iax in 1..=hii {
            let key = if s == ' ' {
                format!("CRVAL{iax}")
            } else {
                format!("CRVAL{iax}{s}")
            };
            crval.push(fc.cards.get_float(&key)?);
        }
        fc.cards.clear_card();
        let Ok(Some(AnyObject::FrameSet(mut fs))) = fc.read() else {
            return None;
        };
        let mut found = false;
        for ifr in 1..=fs.nframe() {
            fs.set_current(FrameRef::Index(ifr)).ok()?;
            let frame = fs.current();
            if frame.test_ident() && frame.ident() == s.to_string() {
                found = true;
                break;
            }
        }
        if !found || fs.current().naxes() != hii {
            return None;
        }
        let domain = wcsfrm.domain().to_string();
        let tfs = crate::frames::find_frame::convert(&fs, wcsfrm, &[domain.as_str()]).ok()??;
        let map = tfs.mapping(FrameRef::Base, FrameRef::Current).ok()?;
        let out = map
            .transform(&crate::coords::PointSet::from_rows([crval]), true)
            .ok()?;
        Some(out.point(0).to_vec())
    }

    /// The attributes the header translations of a read depend on.
    fn trans_attrs(&self) -> spec_trans::TransAttrs {
        spec_trans::TransAttrs {
            car_lin: self.car_lin(),
            poly_tan: self.poly_tan(),
        }
    }

    /// The CDMatrix attribute: whether a FITS-WCS write describes the
    /// linear transformation from pixel to intermediate world coordinates,
    /// the scaling, rotation and shear of the pixel axes, with `CDi_j`
    /// keywords.
    ///
    /// When true the transformation is a square matrix of `CDi_j`
    /// keywords, `i` being an intermediate world coordinate axis and `j` a
    /// pixel axis.
    /// When false it is a square matrix of `PCi_j` keywords with a scaling
    /// for each axis in `CDELTi` keywords.
    /// FITS-WCS paper II ("Representation of Celestial Coordinates in
    /// FITS", Calabretta and Greisen) describes both.
    ///
    /// Default: chosen, each time it is asked for, from the cards: true if
    /// the FitsChan holds a `CDi_j` card a read has not consumed, and false
    /// otherwise.
    /// Reading a FrameSet usually consumes the `CDi_j` cards, so the default
    /// after a read is usually false even when the header had them; to keep
    /// the value, ask for the default before the read and set it.
    /// As for [Encoding](Self::encoding), a [source](Self::set_source) not
    /// yet read is read first; the card current before that read is current
    /// after, so a FitsChan whose source is read this way is left at
    /// end-of-file.
    ///
    /// # Errors
    ///
    /// As for [`card`](Self::card), when CDMatrix is not set.
    #[doc(alias = "CDMatrix")]
    pub fn cd_matrix(&mut self) -> AstResult<bool> {
        match self.cd_matrix {
            Some(v) => Ok(v),
            None => {
                // C saves the current card before the search reads the
                // source and puts it back after, so a FitsChan at
                // end-of-file, as one is before its source is read, is left
                // there.
                let card0 = self.cards.position();
                let at_eof = card0 >= self.cards.len();
                self.read_from_source()?;
                let found = self.cards.key_fields("CD%1d_%1d", 0).0 > 0;
                self.cards
                    .set_position(if at_eof { usize::MAX } else { card0 });
                Ok(found)
            }
        }
    }

    /// Expose the internal card list for use in unit tests within this crate.
    #[cfg(test)]
    pub(crate) fn cards_for_test(&self) -> &CardList {
        &self.cards
    }

    // -----------------------------------------------------------------------
    // Native encoding helpers
    // -----------------------------------------------------------------------

    /// Store an object in the keyword-card NATIVE form: a leading banner of
    /// COMMENT cards, the BEGAST/ENDAST/ISA/`<NAME>_xx` data cards, and a
    /// trailing banner. This is the C library's default NATIVE storage
    /// (fitschan.c::Write, line 40206).
    ///
    /// The block is prepended to the existing cards, mirroring C `astWrite`
    /// which rewinds the card pointer before writing so the AST description
    /// precedes the retained original header cards.
    fn store_keyword_native(&mut self, obj: &AnyObject, node: &Node) -> AstResult<()> {
        let comm = self.config.comment();
        let class = node.class.as_str();
        let is_frameset = matches!(obj, AnyObject::FrameSet(_));

        let banner = |prefix: &str, middle: &str, suffix: &str| FitsCard {
            keyword: "COMMENT".to_string(),
            value: FitsValue::Comment(enc_native::make_banner(prefix, middle, suffix)),
            comment: None,
            flags: crate::fitschan::card::CardFlags::default(),
        };

        let mut block: Vec<FitsCard> = Vec::new();

        // Leading banner: blank card, divider, (FrameSet: two info lines),
        // "Beginning of AST data for <class> object", dotted divider.
        if comm {
            block.push(FitsCard {
                keyword: "        ".to_string(),
                value: FitsValue::Comment(String::new()),
                comment: None,
                flags: crate::fitschan::card::CardFlags::default(),
            });
            block.push(banner(
                "++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++",
                "",
                "",
            ));
            if is_frameset {
                block.push(banner("WCS information in AST format", "", ""));
                block.push(banner("See http://www.starlink.ac.uk/ast/", "", ""));
            }
            block.push(banner("Beginning of AST data for ", class, " object"));
            block.push(banner(
                "................................................................",
                "",
                "",
            ));
        }

        // Data cards.
        let cards = &self.cards;
        let taken = |k: &str| cards.iter().any(|c| c.keyword == k);
        let (digits, rounding) = (self.fits_digits, self.fits_rounding() as usize);
        let format = |c: &FitsCard| c.format_with_digits_and_rounding(digits, rounding);
        let options = enc_native::NativeWriteOptions {
            with_comments: comm,
            full: self.config.full(),
            format: &format,
        };
        enc_native::write_keyword_native(&mut block, node, &options, &mut self.keyseq, &taken)?;

        // Trailing banner: dotted divider, "End of AST data for <class>
        // object", dashed divider.
        if comm {
            block.push(banner(
                "................................................................",
                "",
                "",
            ));
            block.push(banner("End of AST data for ", class, " object"));
            block.push(banner(
                "----------------------------------------------------------------",
                "",
                "",
            ));
        }

        self.cards.insert_at_current(block);
        Ok(())
    }

    /// Read the FrameSet a foreign encoding other than DSS describes, as C
    /// reads all of them (fitschan.c `FitsToStore`): extract every axis
    /// description from `trans`, the header with `SpecTrans`'s translations,
    /// and make a FrameSet from them. `cursors` receives, for each axis
    /// description read, the index in `warnings` of its first warning and its
    /// number of WCS axes (see [`Self::push_read_warnings`]).
    fn decode_wcs_stores(
        &mut self,
        trans: &CardList,
        def_b1950: bool,
        cursors: &mut wcs_read::ReadCursors,
        warnings: &mut Warnings,
    ) -> AstResult<Option<AnyObject>> {
        let mut stores = wcs_keywords::extract_wcs_keywords_with_alts(trans)?;
        for st in stores
            .primary
            .iter_mut()
            .chain(stores.alts.iter_mut().map(|(_, st)| st))
        {
            st.sip_replace = self.sip_replace();
            st.car_lin = self.car_lin();
        }
        let (ignore_bad_alt, iwc) = (self.ignore_bad_alt(), self.iwc());
        let mut tabs = wcs_tab::TabReader {
            tab_ok: self.tab_ok(),
            tables: &mut self.tables,
            source: self.table_source.0.as_mut(),
        };
        wcs_read::decode_wcs_with_alts_inner(
            &stores,
            def_b1950,
            ignore_bad_alt,
            iwc,
            &self.cards,
            &mut tabs,
            warnings,
            cursors,
        )
    }

    /// The end of a read of an encoding other than NATIVE or DSS, mirroring
    /// C's `Read` and its `FixUsed`: on success consume the cards the read
    /// used and store its warnings; on failure consume them only when Clean
    /// is set.
    ///
    /// The cards used are those SpecTrans used (`spec_names`), every card
    /// C's `WcsFcRead` sweep marks ([`wcs_mark::mark_wcs_cards`]), which
    /// covers every copy of a repeated WCS keyword whose value converts, and
    /// the other names the extraction recorded on `trans`, each marking
    /// every matching card as C's `MarkAllCards` does. The sweep is also C's
    /// extraction, so it fails where C's read fails at a card with no value,
    /// and then the cards after that card, and the extraction's names, are
    /// not used.
    /// C's `WcsFcRead` sweep, after the warnings `SpecTrans` raised are
    /// stored: flag every WCS card whose value converts as provisionally
    /// used, warning in front of each card it cannot convert.
    /// It fails where C's read fails at a card with no value, or, with
    /// Strict in force, at the first warning the Warnings attribute names.
    fn sweep_foreign(
        &mut self,
        entry: usize,
        trans_warnings: spec_trans::TransWarnings,
        trans: &CardList,
    ) -> AstResult<()> {
        let translated = trans.take_written_names();
        let trans_issued = self.push_trans_warnings(entry, trans_warnings);
        let sweep_policy = self.warn_list("astRead");
        let (sweep_warnings, swept) =
            wcs_mark::mark_wcs_cards(&mut self.cards, &translated, &sweep_policy);
        let sweep_issued = self.push_sweep_warnings(sweep_warnings);
        trans_issued.and(swept).and(sweep_issued)
    }

    /// The end of a read of an encoding other than NATIVE or DSS, mirroring
    /// C's `Read` and its `FixUsed`, once [`Self::sweep_foreign`] has given
    /// `swept` and, when it succeeded, the FrameSet has been made: on
    /// success consume the cards the read used and store its warnings; on
    /// failure consume them only when Clean is set.
    ///
    /// The cards used are those SpecTrans used (`spec_names`), every card
    /// the sweep marked, which covers every copy of a repeated WCS keyword
    /// whose value converts, and the other names the extraction recorded on
    /// `trans`, each marking every matching card as C's `MarkAllCards` does.
    /// When the sweep failed, the cards after the one it failed at, and the
    /// extraction's names, are not used.
    fn finish_foreign_read(
        &mut self,
        decoded: AstResult<Option<AnyObject>>,
        swept: AstResult<()>,
        read_warnings: Warnings,
        cursors: wcs_read::ReadCursors,
        trans: &CardList,
        spec_names: Vec<String>,
    ) -> AstResult<Option<AnyObject>> {
        // A keyword `WcsFcRead` reads is consumed card by card by the sweep,
        // only where its value converts.
        let extract_names: Vec<String> = trans
            .take_consumed_names()
            .into_iter()
            .filter(|name| !wcs_mark::is_wcs_keyword(name))
            .collect();
        let clean = self.clean();
        let mark = |cards: &mut CardList, names: &[String]| {
            for name in names {
                cards.mark_all_cards(name);
            }
        };
        match (decoded, swept) {
            (Ok(Some(obj)), Ok(())) => {
                mark(&mut self.cards, &spec_names);
                mark(&mut self.cards, &extract_names);
                self.cards.fix_used(true, false);
                self.push_read_warnings(read_warnings, &cursors)?;
                Ok(Some(obj))
            }
            (decoded, swept) => {
                let sweep_failed = swept.is_err();
                if clean {
                    mark(&mut self.cards, &spec_names);
                    if !sweep_failed {
                        mark(&mut self.cards, &extract_names);
                    }
                    self.cards.fix_used(true, false);
                } else {
                    self.cards.fix_used(false, false);
                }
                // The warnings raised before the error are kept, as C's
                // `Warn` stores each as it is raised. C's sweep runs before
                // the object is made, so when it fails only the header
                // translations have warned, and its error is the one
                // reported.
                if !sweep_failed {
                    self.push_read_warnings(read_warnings, &cursors)?;
                }
                // With no pixel axes C's `MakeFitsFrameSet` gives nothing,
                // which `Read` treats as a failure without an error.
                match (swept, decoded) {
                    (Err(e), _) => Err(e),
                    (Ok(()), Err(e)) => Err(e),
                    (Ok(()), Ok(None)) => Ok(None),
                    (Ok(()), Ok(Some(_))) => unreachable!("handled above"),
                }
            }
        }
    }

    /// Store the `ASTWARN` blocks `SpecTrans` raised, those raised before its
    /// `QV` scan in front of the card at `entry`, where the read started, and
    /// the rest after the last card: the scan walks off the end of the list
    /// and C `InsCard` appends when there is no current card
    /// (fitschan.c:18892-18903).
    fn push_trans_warnings(
        &mut self,
        entry: usize,
        warnings: spec_trans::TransWarnings,
    ) -> AstResult<()> {
        if self.clean() {
            return Ok(());
        }
        if !warnings.entry.is_empty() {
            self.cards.set_position(entry);
            for w in warnings.entry {
                self.warn(w, "astRead")?;
            }
        }
        if !warnings.end.is_empty() {
            self.cards.set_position(self.cards.len());
            for w in warnings.end {
                self.warn(w, "astRead")?;
            }
        }
        Ok(())
    }

    /// Store the `ASTWARN` blocks `WcsFcRead`'s sweep raised, each in front
    /// of the card at its index, where C's cursor is when it warns.
    fn push_sweep_warnings(&mut self, warnings: Vec<(usize, Warning)>) -> AstResult<()> {
        if self.clean() {
            return Ok(());
        }
        let mut added = 0;
        for (index, w) in warnings {
            let before = self.cards.len();
            self.cards.set_position(index + added);
            self.warn(w, "astRead")?;
            added += self.cards.len() - before;
        }
        Ok(())
    }

    /// Store the `ASTWARN` blocks the decoding raised, each in front of the
    /// card current in C when it was raised (see [`wcs_read::ReadCursors`]).
    /// C's `DistortMaps` reads NAXIS1 to NAXIS<n> as each axis description is
    /// read, `n` its number of WCS axes, leaving the cursor
    /// `move_cursor_to_last_naxis` reproduces; reading IRAF WAT cards leaves
    /// it at end-of-file.
    fn push_read_warnings(
        &mut self,
        warnings: Warnings,
        cursors: &[(usize, Option<usize>)],
    ) -> AstResult<()> {
        if self.clean() {
            return Ok(());
        }
        let mut current = None;
        for (index, w) in warnings.into_iter().enumerate() {
            let cursor = cursors
                .iter()
                .rev()
                .find(|(start, _)| *start <= index)
                .map_or(Some(0), |&(_, c)| c);
            if current != Some(cursor) {
                match cursor {
                    Some(n) => move_cursor_to_last_naxis(&mut self.cards, n),
                    None => self.cards.set_position(self.cards.len()),
                }
                current = Some(cursor);
            }
            self.warn(w, "astRead")?;
        }
        Ok(())
    }
}

impl Clone for FitsChan {
    /// A copy of the FitsChan, holding the cards of any source not yet
    /// read, but with no source, sink, table source, SourceFile or SinkFile
    /// of its own, so it never writes the cards a second time.
    // C: `astCopy`, which reads the source before FitsChan's `Copy`.
    fn clone(&self) -> Self {
        let copy = Self {
            config: self.config.clone(),
            cards: self.cards.clone(),
            encoding: self.encoding,
            warnings: self.warnings.clone(),
            fits_digits: self.fits_digits,
            fits_rounding: self.fits_rounding,
            cd_matrix: self.cd_matrix,
            sip_replace: self.sip_replace,
            car_lin: self.car_lin,
            fits_tol: self.fits_tol,
            def_b1950: self.def_b1950,
            poly_tan: self.poly_tan,
            ignore_bad_alt: self.ignore_bad_alt,
            warning_conditions: self.warning_conditions.clone(),
            alt_axes: self.alt_axes,
            iwc: self.iwc,
            fits_axis_order: self.fits_axis_order.clone(),
            intra: self.intra.clone(),
            keyseq: self.keyseq.clone(),
            tables: self.tables.clone(),
            clean: self.clean,
            sip_ok: self.sip_ok,
            ident: self.ident.clone(),
            use_defs: self.use_defs,
            id: Default::default(),
            tab_ok: self.tab_ok,
            force_tab: self.force_tab,
            source: self.source.clone(),
            sink: self.sink.clone(),
            source_file: self.source_file.clone(),
            sink_file: self.sink_file.clone(),
            table_source: self.table_source.clone(),
        };
        self.copy_reading_source(copy)
    }
}

impl std::fmt::Debug for FitsChan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FitsChan")
            .field("config", &self.config)
            .field("ident", &self.ident)
            .field("cards", &self.cards)
            .field("encoding", &self.encoding)
            .field("sink", &self.sink.0.is_some())
            .field("source_file", &self.source_file.0)
            .field("sink_file", &self.sink_file.0)
            .finish_non_exhaustive()
    }
}

impl Channel for FitsChan {
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.warnings.clear();
        self.read_from_source()?;

        let enc = self.encoding()?;
        let def_b1950 = self.def_b1950()?;
        // The encodings other than NATIVE hold one FrameSet, read only when
        // the first card is current; a successful read leaves the FitsChan at
        // end-of-file and any other rewinds it (fitschan.c `Read`).
        if enc != Encoding::Native && self.cards.test_card() {
            return Ok(None);
        }
        let mut read_encoded = || -> AstResult<Option<AnyObject>> {
            match enc {
                Encoding::Native => {
                    // C reads the object through the Channel class from the
                    // current card on (`GetNextData`), passing over cards
                    // until a `BEGAST` card; with none, it reaches
                    // end-of-file. With Skip zero, each card passed is
                    // marked as used, and any but a commentary card is an
                    // error, after which the card following it is current.
                    // C's `Read` then removes the marked cards with Clean
                    // set, and otherwise makes them available again.
                    let start = self.cards.position();
                    let skip = self.config.skip();
                    let clean = self.clean();
                    let mark_from_start = |cards: &mut CardList, end: usize| {
                        for i in start..end {
                            cards.mark_at(i);
                        }
                    };
                    if !skip {
                        match enc_native::unskipped_begin(&self.cards, start) {
                            Ok(_) => {}
                            Err((i, e)) => {
                                self.cards.set_position(i + 1);
                                if clean {
                                    mark_from_start(&mut self.cards, i + 1);
                                    self.cards.fix_used(true, true);
                                }
                                return Err(e);
                            }
                        }
                    }
                    let Some((first, last)) = enc_native::native_span(&self.cards, start) else {
                        if !skip && clean {
                            let end = self.cards.len();
                            mark_from_start(&mut self.cards, end);
                        }
                        self.cards.set_position(usize::MAX);
                        if !skip && clean {
                            self.cards.fix_used(true, true);
                        }
                        return Ok(None);
                    };
                    if !skip {
                        mark_from_start(&mut self.cards, first);
                    }
                    let text =
                        enc_native::cards_to_native_text(&self.cards.sub_list(first..=last))?;
                    let node = serial::native_read::parse_native(&text)?;
                    let obj = with_registry(&self.intra, || serial::any_object_from_node(&node))?;

                    // C `Read` calls `FixUsed( this, 0, 1, remove )` with
                    // `remove` set for NATIVE encoding (fitschan.c:25923-25929),
                    // so the cards a successful native read consumed are deleted
                    // rather than merely flagged; the card after them is
                    // current.
                    for i in first..=last {
                        self.cards.mark_at(i);
                    }
                    self.cards.mark_com_block(first, -1);
                    self.cards.mark_com_block(last, 1);
                    self.cards.set_position(last + 1);
                    self.cards.fix_used(true, true);

                    Ok(Some(obj))
                }
                Encoding::FitsWcs => {
                    // Run SpecTrans first to normalise non-standard
                    // keywords. Although FITS-WCS is the canonical form,
                    // SpecTrans translations 7-10, 17-22, 24 still apply
                    // (EPOCH/EQUINOX, AIPS spectral CTYPE, RESTFREQ, CUNIT
                    // case, IRAF mini-WCS strip, etc.). Returns a
                    // separate "trans" CardList so the original
                    // self.cards layout is preserved for write-time
                    // round-trip — mirrors fitschan.c:11524-11532 which
                    // creates a temporary FitsChan `ret` and deletes it
                    // after WcsToStore.
                    // C's `Warn` calls inside `SpecTrans` target the source chan
                    // rather than the translation chan, so their text comes back
                    // out to be stored on `self.cards`.
                    let entry = self.cards.position();
                    let mut trans_warnings = spec_trans::TransWarnings::default();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        Encoding::FitsWcs,
                        self.trans_attrs(),
                        &mut trans_warnings,
                        &self.warn_list("astRead"),
                    )?;
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    // C's `WcsFcRead` sweeps the cards before the FrameSet
                    // is made, and makes nothing when the sweep fails.
                    let swept = self.sweep_foreign(entry, trans_warnings, &trans);
                    let mut read_warnings = self.warn_list("astRead");
                    let mut cursors = Vec::new();
                    let decoded = match swept {
                        Ok(()) => self.decode_wcs_stores(
                            &trans,
                            def_b1950,
                            &mut cursors,
                            &mut read_warnings,
                        ),
                        Err(_) => Ok(None),
                    };
                    self.finish_foreign_read(
                        decoded,
                        swept,
                        read_warnings,
                        cursors,
                        &trans,
                        spec_names,
                    )
                }
                Encoding::FitsPc => {
                    // Run SpecTrans first to normalise non-standard
                    // keywords (Cnxxx alt-axis form, CDjjjiii, CROTA,
                    // PROJP, etc.) into their canonical equivalents.
                    // After SpecTrans, alt-axis cards are in the *A..*Z
                    // form, so we route through the multi-plane
                    // FitsWcs decoder to pick up secondary planes.
                    let entry = self.cards.position();
                    let mut trans_warnings = spec_trans::TransWarnings::default();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        Encoding::FitsPc,
                        self.trans_attrs(),
                        &mut trans_warnings,
                        &self.warn_list("astRead"),
                    )?;
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    // C's `WcsFcRead` sweeps the cards before the FrameSet
                    // is made, and makes nothing when the sweep fails.
                    let swept = self.sweep_foreign(entry, trans_warnings, &trans);
                    let mut read_warnings = self.warn_list("astRead");
                    let mut cursors = Vec::new();
                    let decoded = match swept {
                        Ok(()) => self.decode_wcs_stores(
                            &trans,
                            def_b1950,
                            &mut cursors,
                            &mut read_warnings,
                        ),
                        Err(_) => Ok(None),
                    };
                    self.finish_foreign_read(
                        decoded,
                        swept,
                        read_warnings,
                        cursors,
                        &trans,
                        spec_names,
                    )
                }
                Encoding::FitsIraf => {
                    let entry = self.cards.position();
                    let mut trans_warnings = spec_trans::TransWarnings::default();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        Encoding::FitsIraf,
                        self.trans_attrs(),
                        &mut trans_warnings,
                        &self.warn_list("astRead"),
                    )?;
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    // C's `WcsFcRead` sweeps the cards before the FrameSet
                    // is made, and makes nothing when the sweep fails.
                    let swept = self.sweep_foreign(entry, trans_warnings, &trans);
                    let mut read_warnings = self.warn_list("astRead");
                    let mut cursors = Vec::new();
                    let decoded = match swept {
                        Ok(()) => self.decode_wcs_stores(
                            &trans,
                            def_b1950,
                            &mut cursors,
                            &mut read_warnings,
                        ),
                        Err(_) => Ok(None),
                    };
                    self.finish_foreign_read(
                        decoded,
                        swept,
                        read_warnings,
                        cursors,
                        &trans,
                        spec_names,
                    )
                }
                Encoding::FitsAips | Encoding::FitsAipsPP => {
                    let entry = self.cards.position();
                    let mut trans_warnings = spec_trans::TransWarnings::default();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        enc,
                        self.trans_attrs(),
                        &mut trans_warnings,
                        &self.warn_list("astRead"),
                    )?;
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    // C's `WcsFcRead` sweeps the cards before the FrameSet
                    // is made, and makes nothing when the sweep fails.
                    let swept = self.sweep_foreign(entry, trans_warnings, &trans);
                    let mut read_warnings = self.warn_list("astRead");
                    let mut cursors = Vec::new();
                    let decoded = match swept {
                        Ok(()) => self.decode_wcs_stores(
                            &trans,
                            def_b1950,
                            &mut cursors,
                            &mut read_warnings,
                        ),
                        Err(_) => Ok(None),
                    };
                    self.finish_foreign_read(
                        decoded,
                        swept,
                        read_warnings,
                        cursors,
                        &trans,
                        spec_names,
                    )
                }
                Encoding::FitsClass => {
                    let entry = self.cards.position();
                    let mut trans_warnings = spec_trans::TransWarnings::default();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        Encoding::FitsClass,
                        self.trans_attrs(),
                        &mut trans_warnings,
                        &self.warn_list("astRead"),
                    )?;
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    // C's `WcsFcRead` sweeps the cards before the FrameSet
                    // is made, and makes nothing when the sweep fails.
                    let swept = self.sweep_foreign(entry, trans_warnings, &trans);
                    let mut read_warnings = self.warn_list("astRead");
                    let mut cursors = Vec::new();
                    let decoded = match swept {
                        Ok(()) => self.decode_wcs_stores(
                            &trans,
                            def_b1950,
                            &mut cursors,
                            &mut read_warnings,
                        ),
                        Err(_) => Ok(None),
                    };
                    self.finish_foreign_read(
                        decoded,
                        swept,
                        read_warnings,
                        cursors,
                        &trans,
                        spec_names,
                    )
                }
                Encoding::Dss => {
                    // Dss is a separate format; SpecTrans's translations
                    // are mostly inapplicable. Keep the existing reader. The
                    // reader records consumed names directly on `self.cards`
                    // (no SpecTrans copy), so promote them in place.
                    let mut read_warnings = self.warn_list("astRead");
                    let mut cursors = Vec::new();
                    let decoded = (|| -> AstResult<AnyObject> {
                        let store = enc_dss::read_from_cards(&self.cards)?;
                        cursors.push((0, Some(store.naxis)));
                        wcs_read::decode_wcs_inner(
                            &store,
                            store.naxis,
                            def_b1950,
                            self.iwc(),
                            &self.cards,
                            ' ',
                            None,
                            &mut read_warnings,
                            &mut cursors,
                        )
                    })();
                    // The DSS reader records the cards it used on `self.cards`.
                    let clean = self.clean();
                    match decoded {
                        Ok(obj) => {
                            self.push_read_warnings(read_warnings, &cursors)?;
                            for name in self.cards.take_consumed_names() {
                                self.cards.mark_card(&name);
                            }
                            self.cards.fix_used(true, false);
                            Ok(Some(obj))
                        }
                        Err(e) => {
                            let names = self.cards.take_consumed_names();
                            if clean {
                                for name in names {
                                    self.cards.mark_card(&name);
                                }
                                self.cards.fix_used(true, false);
                            }
                            self.push_read_warnings(read_warnings, &cursors)?;
                            Err(e)
                        }
                    }
                }
            }
        };
        let result = read_encoded();
        if enc != Encoding::Native {
            if matches!(result, Ok(Some(_))) {
                self.cards.set_position(usize::MAX);
            } else {
                self.cards.clear_card();
            }
        }
        result
    }

    fn write(&mut self, obj: &AnyObject) -> AstResult<usize> {
        self.warnings.clear();
        self.read_from_source()?;

        // Remember the entry card so a failed write can put it back, and
        // start flagging every card this call adds so a failed write can
        // remove them again. Mirrors C `Write` (fitschan.c:40754-40758).
        let card0 = self.cards.position();
        self.cards.begin_marking_new();
        let result = self.write_inner(obj);

        // A write that failed, for either reason, leaves the header exactly
        // as it was found: the cards it had added are removed and the entry
        // card restored. Mirrors fitschan.c:40878-40894.
        let wrote = matches!(result, Ok(n) if n > 0);
        self.cards.fix_new(card_list::NewScope::Write, !wrote);
        self.cards.fix_new(card_list::NewScope::Version, !wrote);
        self.cards.end_marking_new();
        if !wrote {
            self.cards.set_position(card0);
        }
        result
    }

    fn register_intra(&mut self, transform: Arc<dyn IntraTransform>) -> AstResult<()> {
        self.intra.register(transform)
    }

    fn warning_texts(&self) -> &[String] {
        &self.warnings
    }

    fn config(&self) -> &ChannelConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut ChannelConfig {
        &mut self.config
    }
}

impl FitsChan {
    /// The body of [`Channel::write`], run inside its card-rollback frame.
    fn write_inner(&mut self, obj: &AnyObject) -> AstResult<usize> {
        let enc = self.encoding()?;
        match enc {
            Encoding::Native => {
                let node = serial::any_object_to_node(obj)?;
                self.store_keyword_native(obj, &node)?;
                // The native encoding can represent every AST class.
                // Mirrors fitschan.c:40822.
                Ok(1)
            }
            _ => {
                // For all FITS-WCS encodings, preserve the relative position
                // of any WCS card that already exists in the input header by
                // letting the encoder overwrite it in place. Every other card
                // is kept, those a read consumed staying hidden, as C keeps
                // them.
                // The image dimensions, from NAXIS1 to NAXIS<nin> for the
                // FrameSet's pixel axes, as C's `Write` reads them
                // (fitschan.c:40937-40945), before positioning the cursor.
                let dims = match obj {
                    AnyObject::FrameSet(fs) => {
                        extract_naxis_dims(&self.cards, crate::mappings::Mapping::nin(fs))
                    }
                    _ => Vec::new(),
                };

                self.cards.position_write_cursor();

                let attrs = self.write_attrs();
                let write_warnings = attrs.warnings.clone();
                let write_tables = attrs.tables.clone();
                // C's `Write` reads NAXIS1 to NAXIS<nin> with `astGetFitsF`
                // before `FsetToStore`, which leaves the cursor on the last
                // found, or at end-of-file, and there `Warn` stores the
                // warnings `FsetToStore` raises. The card is found now and
                // again by keyword and occurrence once the header is written.
                let warn_card = match obj {
                    AnyObject::FrameSet(fs) => {
                        let card0 = self.cards.position();
                        for i in 1..=crate::mappings::Mapping::nin(fs) {
                            let _ = self.cards.search_card(&format!("NAXIS{i}"), "astGetFitsF");
                        }
                        let at = self.cards.position();
                        self.cards.set_position(card0);
                        self.cards.card_occurrence(at)
                    }
                    _ => None,
                };
                let cd_matrix = self.cd_matrix()?;
                let result = match enc {
                    Encoding::FitsWcs => {
                        wcs_write::encode_wcs(obj, &mut self.cards, cd_matrix, &dims, attrs)
                    }
                    Encoding::FitsIraf => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsIraf,
                        attrs,
                        enc_iraf::write_to_cards,
                    ),
                    Encoding::FitsPc => wcs_write::encode_pc(obj, &mut self.cards, &dims, attrs),
                    Encoding::FitsAips => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsAips,
                        attrs,
                        enc_aips::write_to_cards,
                    ),
                    Encoding::FitsAipsPP => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsAipsPP,
                        attrs,
                        enc_aips::write_to_cards_pp,
                    ),
                    Encoding::FitsClass => {
                        let ok = wcs_write::encode_wcs_per_plane(
                            obj,
                            &mut self.cards,
                            &dims,
                            Encoding::FitsClass,
                            attrs,
                            enc_class::write_to_cards,
                        )?;
                        // Then run the SpecFrame velocity computation
                        // that needs the FrameSet directly. Mirrors
                        // fitschan.c:6930-6985.
                        if ok {
                            if let AnyObject::FrameSet(fs) = obj {
                                enc_class::write_velocities_from_frameset(fs, &mut self.cards)?;
                                enc_class::write_azel_from_frameset(fs, &mut self.cards)?;
                            }
                        }
                        Ok(ok)
                    }
                    Encoding::Dss => wcs_write::encode_wcs_to_store(
                        obj,
                        &mut self.cards,
                        &dims,
                        attrs,
                        enc_dss::write_to_cards,
                    ),
                    Encoding::Native => unreachable!(),
                };

                // C's `Warn` stores each warning as `FsetToStore` raises it,
                // before any card of the header is written.
                let warnings = write_warnings.take();
                if !warnings.is_empty() {
                    let at = warn_card
                        .and_then(|(keyword, n)| self.cards.occurrence_index(&keyword, n))
                        .unwrap_or(self.cards.len());
                    self.cards.set_position(at);
                    for w in warnings {
                        self.warn(w, "FitsChan")?;
                    }
                }

                let ret = usize::from(result?);
                // The table any `-TAB` axes were described with is moved to
                // the FitsChan, where `get_tables` finds it, whether or not
                // the header could be written (fitschan.c `FitsFromStore`).
                if let Some(table) = write_tables.borrow_mut().take() {
                    self.tables
                        .get_or_insert_with(crate::keymap::KeyMap::new)
                        .put_0a(
                            fset_to_store::map_tab::TABEXTNAME,
                            AnyObject::FitsTable(Box::new(table)),
                            None,
                        )?;
                }
                // A successful write leaves the current card at end-of-file,
                // so a subsequent write appends after this object. Mirrors
                // fitschan.c:40865.
                if ret > 0 {
                    self.cards.set_position(usize::MAX);
                }
                Ok(ret)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Look up `NAXIS1`…`NAXIS<naxes>` on `cards` as C `DistortMaps`'s opening
/// loop does (fitschan.c:8903-8907), from the end-of-file cursor `WcsFcRead`'s
/// card loop leaves:
///
/// ```c
/// for( i = 0; i < naxes; i++ ){
///    if( !astGetFitsF( this, FormatKey( "NAXIS", i + 1, -1, ' ', status ),
///                      dim + i ) ) dim[ i ] = AST__BAD;
/// }
/// ```
///
/// The values themselves reach the `FitsStore` through `extract_wcs_keywords`;
/// what this reproduces is the *side effect*. `astGetFitsF` finds its card via
/// `SearchCard`, which makes it current, and unlike every other read on the
/// path it does not go through `GetValue`, so it does not reinstate the
/// original card index. The cursor it leaves behind — `NAXIS<naxes>`, or
/// end-of-file when that card is missing — is where C `Warn` then inserts
/// any `ASTWARN` block.
///
/// C reads these off the FitsChan itself rather than the `SpecTrans` copy, so
/// the cursor this moves is the one the warning cards are written against.
fn move_cursor_to_last_naxis(cards: &mut CardList, naxes: usize) {
    cards.set_position(cards.len());
    for i in 1..=naxes {
        let _ = cards.search_card(&format!("NAXIS{i}"), "astGetFitsF");
    }
}

/// The size of each of `naxis` pixel axes: the value of `NAXIS<i>` as a
/// floating point number, or `None` if there is none, as C's `Write` reads
/// them with `astGetFitsF` (fitschan.c:40941-40944).
fn extract_naxis_dims(cards: &CardList, naxis: usize) -> Vec<Option<f64>> {
    (1..=naxis)
        .map(|i| cards.get_float(&format!("NAXIS{i}")))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::ZoomMap;

    #[test]
    fn new_fitschan_is_empty() {
        let mut fc = FitsChan::new().unwrap();
        assert_eq!(fc.ncard().unwrap(), 0);
        // No explicit encoding set; auto-detect on an empty chan
        // returns Native per C `astGetEncoding` (fitschan.c:12603).
        assert_eq!(fc.encoding().unwrap(), Encoding::Native);
        assert!(!fc.test_encoding());
        assert_eq!(fc.fits_digits(), DEFAULT_FITS_DIGITS);
    }

    #[test]
    fn from_header_creates_fitschan() {
        let header = "SIMPLE  =                    T / Fits standard\n\
                      BITPIX  =                  -32 / Bits per pixel\n\
                      NAXIS   =                    2 / Number of axes\n\
                      END";
        let mut fc = FitsChan::from_header(header).unwrap();
        assert_eq!(fc.ncard().unwrap(), 4); // END is stored as a commentary card
    }

    #[test]
    fn put_fits_and_find_fits() {
        let mut fc = FitsChan::new().unwrap();
        fc.put_fits("NAXIS   =                    2 / Number of axes", false)
            .unwrap();
        fc.put_fits("CRVAL1  =     266.416666666667 / Reference value", false)
            .unwrap();
        assert_eq!(fc.ncard().unwrap(), 2);

        fc.clear_card().unwrap();
        let found = fc.find_fits("CRVAL1", true).unwrap();
        assert!(found.is_some());
        let card_str = found.unwrap();
        assert!(card_str.contains("266.41666666666"));
    }

    #[test]
    fn set_fits_f_and_get_fits_f() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_f("CRVAL1", 123.456, "Test value", false)
            .unwrap();
        let val = fc.get_fits_f(Some("CRVAL1")).unwrap().unwrap();
        assert!((val - 123.456).abs() < 1e-10);
    }

    #[test]
    fn set_fits_i_and_get_fits_i() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_i("NAXIS", 3, "Number of axes", false).unwrap();
        assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(3));
    }

    #[test]
    fn set_fits_s_and_get_fits_s() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_s("CTYPE1", "RA---TAN", "Axis type", false)
            .unwrap();
        assert_eq!(
            fc.get_fits_s(Some("CTYPE1")).unwrap(),
            Some("RA---TAN".to_string())
        );
    }

    #[test]
    fn set_fits_l_and_get_fits_l() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_l("SIMPLE", true, "Standard FITS", false)
            .unwrap();
        assert_eq!(fc.get_fits_l(Some("SIMPLE")).unwrap(), Some(true));
    }

    #[test]
    fn ncard_tracks_insertions() {
        let mut fc = FitsChan::new().unwrap();
        assert_eq!(fc.ncard().unwrap(), 0);
        fc.set_fits_i("A", 1, "", false).unwrap();
        assert_eq!(fc.ncard().unwrap(), 1);
        fc.set_fits_i("B", 2, "", false).unwrap();
        assert_eq!(fc.ncard().unwrap(), 2);
    }

    #[test]
    fn empty_fits_clears_all() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_i("NAXIS", 2, "", false).unwrap();
        fc.set_fits_i("NAXIS1", 100, "", false).unwrap();
        assert_eq!(fc.ncard().unwrap(), 2);
        fc.empty_fits().unwrap();
        assert_eq!(fc.ncard().unwrap(), 0);
    }

    #[test]
    fn overwrite_replaces_existing() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_i("NAXIS", 2, "Original", false).unwrap();
        // Overwriting replaces the current card.
        fc.clear_card().unwrap();
        fc.set_fits_i("NAXIS", 3, "Updated", true).unwrap();
        assert_eq!(fc.ncard().unwrap(), 1); // still only one card
        assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(3));
    }

    #[test]
    fn encoding_accessors() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_encoding(Encoding::Native);
        assert_eq!(fc.encoding().unwrap(), Encoding::Native);
    }

    #[test]
    fn fits_digits_accessors() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_digits(15);
        assert_eq!(fc.fits_digits(), 15);
        fc.set_fits_digits(-3);
        assert_eq!(fc.fits_digits(), -3);
    }

    #[test]
    fn del_fits_removes_current() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_i("A", 1, "", false).unwrap();
        fc.set_fits_i("B", 2, "", false).unwrap();
        fc.set_fits_i("C", 3, "", false).unwrap();
        fc.clear_card().unwrap();
        // Position at first card, delete it.
        fc.del_fits().unwrap();
        assert_eq!(fc.ncard().unwrap(), 2);
    }

    #[test]
    fn native_round_trip() {
        // Write a ZoomMap via Native encoding, then read it back.
        let zm = ZoomMap::new(2, 3.5).unwrap();
        let obj = AnyObject::ZoomMap(zm);

        let mut fc = FitsChan::new().unwrap();
        fc.set_encoding(Encoding::Native);
        fc.write(&obj).expect("write should succeed");

        // Should have stored COMMENT cards.
        assert!(fc.ncard().unwrap() > 0);

        // Read it back.
        fc.clear_card().unwrap();
        let result = fc.read().expect("read should succeed");
        assert!(result.is_some());
        let read_obj = result.unwrap();
        let mapping = read_obj.as_mapping().expect("should be a Mapping");
        assert_eq!(mapping.nin(), 2);
    }

    #[test]
    fn set_fits_u_stores_undef() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_u("BLANK", "Undefined", false).unwrap();
        let card = fc.cards.find("BLANK").unwrap();
        assert_eq!(card.value, FitsValue::Undef);
    }

    #[test]
    fn set_fits_cm_stores_comment() {
        let mut fc = FitsChan::new().unwrap();
        fc.set_fits_cm("This is a test", false).unwrap();
        assert_eq!(fc.ncard().unwrap(), 1);
        // C's astSetFitsCM stores a commentary card with a blank keyword.
        fc.clear_card().unwrap();
        assert_eq!(fc.card_type().unwrap(), CardType::Comment);
        assert_eq!(fc.card_name().unwrap().as_deref(), Some("        "));
        assert_eq!(fc.card_comm().unwrap(), "This is a test");
    }

    #[test]
    fn put_cards_multi_line() {
        let mut fc = FitsChan::new().unwrap();
        // Cards of 80 characters with no separators; the last may be short.
        let cards = format!(
            "{:<80}{:<80}END",
            "SIMPLE  =                    T / Fits standard",
            "NAXIS   =                    2 / Number of axes"
        );
        fc.put_cards(&cards).unwrap();
        assert_eq!(fc.ncard().unwrap(), 3); // END is stored as a commentary card
        assert_eq!(fc.card().unwrap(), 1);
    }

    #[test]
    fn rewind_resets_position() {
        let mut fc = FitsChan::from_header(
            "NAXIS   =                    2\nNAXIS1  =                  100\nEND",
        )
        .unwrap();
        // Advance position.
        fc.find_fits("NAXIS1", true).unwrap();
        fc.clear_card().unwrap();
        // Should be able to find from beginning again.
        let found = fc.find_fits("NAXIS", true).unwrap();
        assert!(found.is_some());
    }

    // -----------------------------------------------------------------------
    // Read-consumption tracking (C "USED card" mechanism)
    // -----------------------------------------------------------------------

    /// A minimal FITS-WCS header with a celestial plane plus structural and
    /// observation-detail cards. The WCS-description cards (CTYPE/CRVAL/CRPIX/
    /// CDELT) are consumed by the read; SIMPLE/BITPIX/NAXIS* (structural) and
    /// MJD-OBS (observation detail, read mark=0) survive into the output.
    fn wcs_header_with_mjdobs() -> &'static str {
        "SIMPLE  =                    T / Fits standard\n\
         BITPIX  =                  -32 / Bits per pixel\n\
         NAXIS   =                    2 / Number of axes\n\
         NAXIS1  =                  100 / Axis 1 length\n\
         NAXIS2  =                  100 / Axis 2 length\n\
         MJD-OBS =          51544.50000 / MJD of observation\n\
         CTYPE1  = 'RA---TAN'           / X type\n\
         CTYPE2  = 'DEC--TAN'           / Y type\n\
         CRVAL1  =                 10.0 / Reference value\n\
         CRVAL2  =                 20.0 / Reference value\n\
         CRPIX1  =                 50.5 / Reference pixel\n\
         CRPIX2  =                 50.5 / Reference pixel\n\
         CDELT1  =                 -0.1 / Degrees/pixel\n\
         CDELT2  =                  0.1 / Degrees/pixel\n\
         END"
    }

    #[test]
    fn read_consumes_wcs_cards_but_keeps_obs_and_structural() {
        let mut fc = FitsChan::from_header(wcs_header_with_mjdobs()).unwrap();
        fc.read().unwrap().expect("reads a FrameSet");
        let out = fc.to_header().unwrap();
        // WCS-description cards are consumed → excluded from output.
        for kw in [
            "CTYPE1", "CTYPE2", "CRVAL1", "CRVAL2", "CRPIX1", "CRPIX2", "CDELT1", "CDELT2",
        ] {
            assert!(
                !out.contains(kw),
                "consumed WCS card {kw} should not appear in output:\n{out}"
            );
        }
        // Structural + observation-detail cards survive.
        for kw in ["SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "MJD-OBS"] {
            assert!(out.contains(kw), "{kw} should survive the read:\n{out}");
        }
    }

    #[test]
    fn rewrite_after_read_re_emits_wcs_cards() {
        // Read FITS-WCS (consumes WCS cards), then write FITS-WCS back: the
        // re-written cards must reappear in the output (the rewrite clears the
        // consumed flag).
        let mut fc = FitsChan::from_header(wcs_header_with_mjdobs()).unwrap();
        let obj = fc.read().unwrap().expect("reads a FrameSet");
        fc.set_encoding(Encoding::FitsWcs);
        fc.write(&obj).unwrap();
        let out = fc.to_header().unwrap();
        assert!(
            out.contains("CRVAL1"),
            "re-written CRVAL1 should appear in output:\n{out}"
        );
        assert!(
            out.contains("CTYPE1"),
            "re-written CTYPE1 should appear:\n{out}"
        );
    }

    #[test]
    fn protected_card_survives_read() {
        let mut fc = FitsChan::from_header(wcs_header_with_mjdobs()).unwrap();
        // Protect CRVAL1 before reading; it must survive even though the read
        // would otherwise consume it (foundation for astRetainFits).
        assert!(fc.cards.set_protected("CRVAL1"));
        fc.read().unwrap().expect("reads a FrameSet");
        let out = fc.to_header().unwrap();
        assert!(
            out.contains("CRVAL1"),
            "protected CRVAL1 should survive the read:\n{out}"
        );
        // A sibling unprotected WCS card is still consumed.
        assert!(!out.contains("CRVAL2"), "CRVAL2 should be consumed:\n{out}");
    }
}

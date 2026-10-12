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
pub(crate) mod wcs_write;

pub use card::CardType;
use card::{FitsCard, FitsValue};
use card_list::CardList;
use card_value::{Got, Want};
use source_sink::{NotCopied, SourceCell};
use warning::{Warning, Warnings};

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

/// C's `astChrMatch`: [`chr_match_n`] over the whole of both strings.
pub(crate) fn chr_match(a: &str, b: &str) -> bool {
    chr_match_n(a, b, usize::MAX)
}

/// Which Frames a FITS-WCS write describes with alternate axes: the
/// FitsChan's AltAxes attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AltAxes {
    /// Every Frame other than the base and current Frames (the default).
    #[default]
    All,
    /// Only Frames whose Ident is a single upper case letter.
    Ident,
    /// No Frame.
    None,
}

impl AltAxes {
    /// The value's name (fitschan.c `xaltax`): `ALL`, `IDENT` or `NONE`.
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

/// The encoding format used by a [`FitsChan`] to store AST objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// AST Native text format stored in COMMENT cards.
    Native,
    /// FITS-WCS Paper I/II/III encoding.
    FitsWcs,
    /// FITS-WCS using PCi_j matrix notation.
    FitsPc,
    /// IRAF-style WCS encoding.
    FitsIraf,
    /// AIPS-style WCS encoding (CDELTi + CROTA).
    FitsAips,
    /// AIPS++ WCS encoding.
    FitsAipsPP,
    /// CLASS (Grenoble) WCS encoding.
    FitsClass,
    /// DSS (Digitized Sky Survey) plate solution encoding.
    Dss,
}

impl Encoding {
    /// The encoding's name (fitschan.c `xencod`): `NATIVE`, `FITS-PC`,
    /// `DSS`, `FITS-WCS`, `FITS-IRAF`, `FITS-AIPS`, `FITS-AIPS++` or
    /// `FITS-CLASS`.
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
/// fitschan.c:43015), which is 17 when AST is built in C11 mode — enough to
/// round-trip an IEEE double exactly. Callers that want the FITS-standard
/// 20-column limit honoured set a smaller value explicitly (the wcsconv
/// fixtures use `FitsDigits=8` for the FITS encodings).
const DEFAULT_FITS_DIGITS: i32 = 17;

/// A channel that reads and writes AST objects via FITS header cards.
///
/// FitsChan stores a list of FITS header cards and can encode/decode AST
/// objects in various formats. The Native encoding stores AST objects as
/// COMMENT cards with special markers.
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
    /// `astGetEncoding` (fitschan.c:12518). The `DefB1950` default also
    /// keys off whether the encoding has been explicitly set
    /// (fitschan.c:42355).
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
    /// The Clean attribute, when set.
    clean: Option<bool>,
    /// The SipOK attribute, when set.
    sip_ok: Option<bool>,
    /// The Object-class Ident attribute, when set.
    ident: Option<String>,
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
}

impl FitsChan {
    /// Create an empty FitsChan with no explicit encoding.
    ///
    /// The encoding is left unset — [`FitsChan::encoding`] returns the
    /// auto-detected encoding (default `FitsWcs` for an empty chan).
    /// Mirrors C `astFitsChan(...)` followed by `astTestEncoding == 0`.
    pub fn new() -> Self {
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
            clean: None,
            sip_ok: None,
            ident: None,
            id: Default::default(),
            tab_ok: None,
            force_tab: None,
            source: SourceCell::default(),
            sink: NotCopied::default(),
            source_file: NotCopied::default(),
            sink_file: NotCopied::default(),
        }
    }

    /// Create a FitsChan from FITS header text, with the first card current.
    ///
    /// The text is split into lines and each is parsed as a FITS card. An
    /// `END` card has no `=` in column 9, so it is stored as an ordinary
    /// commentary card, just like `HISTORY`, `COMMENT` or a blank keyword.
    /// Unparseable lines are silently ignored.
    pub fn from_header(text: &str) -> AstResult<Self> {
        let (mut cards, warnings) = CardList::from_header_with_warnings(text)?;
        cards.clear_card();
        Ok(Self {
            config: ChannelConfig::fitschan(),
            cards,
            encoding: None,
            warnings,
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
            clean: None,
            sip_ok: None,
            ident: None,
            id: Default::default(),
            tab_ok: None,
            force_tab: None,
            source: SourceCell::default(),
            sink: NotCopied::default(),
            source_file: NotCopied::default(),
            sink_file: NotCopied::default(),
        })
    }

    // -----------------------------------------------------------------------
    // The current card
    // -----------------------------------------------------------------------

    /// The `Card` attribute: the 1-based index of the current card, counting
    /// only cards not consumed by a read; one more than [`ncard`](Self::ncard)
    /// at end-of-file, and 0 when there are no cards.
    pub fn card(&mut self) -> AstResult<usize> {
        self.read_from_source()?;
        Ok(self.cards.get_card())
    }

    /// Set `Card`: rewind, then move forward `icard - 1` cards, stopping at
    /// end-of-file. A value below 1 makes the first card current.
    pub fn set_card(&mut self, icard: i64) -> AstResult<()> {
        self.read_from_source()?;
        self.cards.set_card(icard);
        Ok(())
    }

    /// Clear `Card`: make the first card current (C's "rewind").
    pub fn clear_card(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        self.cards.clear_card();
        Ok(())
    }

    /// Whether `Card` is set, which C defines as the current card not being
    /// the first.
    pub fn test_card(&mut self) -> AstResult<bool> {
        self.read_from_source()?;
        Ok(self.cards.test_card())
    }

    /// The `CardName` attribute: the keyword of the current card (eight
    /// spaces for a blank keyword), or `None` at end-of-file.
    pub fn card_name(&mut self) -> AstResult<Option<String>> {
        self.read_from_source()?;
        Ok(self.cards.current().map(|c| c.c_name().to_string()))
    }

    /// The `CardComm` attribute: the comment of the current card, or the text
    /// of a commentary card; empty when there is none.
    pub fn card_comm(&mut self) -> AstResult<String> {
        self.read_from_source()?;
        Ok(self
            .cards
            .current()
            .and_then(|c| c.c_comment())
            .unwrap_or("")
            .to_string())
    }

    /// The `CardType` attribute: the data type of the current card, or
    /// [`CardType::NoType`] at end-of-file.
    pub fn card_type(&mut self) -> AstResult<CardType> {
        self.read_from_source()?;
        Ok(self
            .cards
            .current()
            .map_or(CardType::NoType, |c| c.card_type()))
    }

    /// The `Ncard` attribute: the number of cards, not counting those a read
    /// consumed.
    pub fn ncard(&mut self) -> AstResult<usize> {
        self.read_from_source()?;
        Ok(self.cards.ncard())
    }

    /// The `Nkey` attribute: the number of different keywords among the cards
    /// a read has not consumed.
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
    fn warn(&mut self, warning: Warning) {
        if self.clean()
            || warning.text.is_empty()
            || !warning::reports(self.warning_conditions(), warning.condition)
        {
            return;
        }
        self.cards.push_astwarn(&warning.text);
        self.warnings.push(warning.text);
    }

    /// The warnings C's `Split` gives for `card`, of an illegal keyword name
    /// and then of a value it cannot type.
    fn split_warnings(&mut self, card: &str, bad_value: bool) {
        if let Some(text) = crate::fitschan::card::check_fits_name(card) {
            self.warn(Warning::new("badkeyname", text));
        }
        if bad_value {
            self.warn(Warning::new(
                "badkeyvalue",
                crate::fitschan::card::bad_value_warning(card),
            ));
        }
    }

    /// The keyword and comment C's `Split` finds when it reads a setter's or
    /// getter's keyword name as a card: the first 8 characters, in their own
    /// case (`NewCard` upper-cases them when a card is stored), and any text
    /// after them. As with any card, an illegal name or value is warned of.
    fn split_name(&mut self, name: &str) -> AstResult<(String, String)> {
        let (card, bad_value) = FitsCard::split(name)?;
        self.split_warnings(name, bad_value);
        let comment = match &card.value {
            FitsValue::Comment(text) => text.trim().to_string(),
            _ => card.comment.clone().unwrap_or_default(),
        };
        let raw: String = name.chars().take(8).collect();
        let keyword = raw.trim_end().to_string();
        debug_assert!(keyword.eq_ignore_ascii_case(&card.keyword));
        Ok((keyword, comment))
    }

    /// Mark the current card to be kept when a read would otherwise consume
    /// it (C's `astRetainFits`). Does nothing at end-of-file.
    pub fn retain_fits(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        self.cards.protect_current();
        Ok(())
    }

    /// Whether keyword `name`, or the current card when `name` is `None`, is
    /// present (C's `astTestFits`): `None` if not, otherwise whether it has a
    /// value. The card is searched for as by [`get_fits_f`](Self::get_fits_f),
    /// and the current card is left as it was.
    pub fn test_fits(&mut self, name: Option<&str>) -> AstResult<Option<bool>> {
        self.read_from_source()?;
        let lname = match name {
            Some(name) => Some(self.split_name(name)?.0),
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

    /// Delete the WCS a read can find, and every WCS-related card left over
    /// (C's `astPurgeWCS`): every object is read with Clean set, discarding
    /// it, then each remaining card whose keyword is a WCS keyword is marked
    /// as used, which hides it. The first card is then current.
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

    /// Store a FITS header card (C's `astPutFits`).
    ///
    /// The card is inserted in front of the current card, or, with
    /// `overwrite`, replaces it; either way the card after it becomes
    /// current. At end-of-file the card is appended. A blank comment, when
    /// overwriting, keeps the comment of the card replaced.
    pub fn put_fits(&mut self, card: &str, overwrite: bool) -> AstResult<()> {
        self.read_from_source()?;
        let (parsed, bad_value) = FitsCard::split(card)?;
        self.split_warnings(card, bad_value);
        match parsed.value {
            // `PutFits` hands a commentary card to `SetFitsCom`.
            FitsValue::Comment(ref text) => {
                let name: String = card.chars().take(8).collect();
                self.set_fits_com(&name, text, overwrite)?;
            }
            _ => {
                // The `astSetFits<X>` the card is handed to splits its
                // keyword name again, warning again of an illegal name.
                self.split_warnings(card, false);
                let comment = parsed.comment.clone().unwrap_or_default();
                self.store_card(parsed.keyword, parsed.value, &comment, overwrite);
            }
        }
        Ok(())
    }

    /// Replace every card with `cards`, a string holding header cards of 80
    /// characters each with no separators (the last may be shorter), and
    /// make the first card current (C's `astPutCards`). Storing stops at the
    /// first card that cannot be read, whose error is returned.
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
    /// `name` (C's `astFindFits`), and return the card's text.
    ///
    /// `name` is a template in which `%d` matches digits, `%c` upper-case
    /// letters and `%f` any keyword character, each with an optional width
    /// (`%1d`; `%0d` for zero or more); `%f` or `%0f` alone matches the
    /// current card. Trailing spaces in `name` are ignored. The card found
    /// becomes current, or the next one with `inc`; if none is found the
    /// FitsChan is left at end-of-file. An illegal field type is
    /// AST__BDFMT.
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

    /// Delete the current card, making the next card current (C's
    /// `astDelFits`). Does nothing at end-of-file.
    pub fn del_fits(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        self.cards.delete_card();
        Ok(())
    }

    /// Delete every card, including those a read consumed, without writing
    /// them to the sink (C's `astEmptyFits`).
    pub fn empty_fits(&mut self) -> AstResult<()> {
        self.read_from_source()?;
        self.empty_fits_inner();
        Ok(())
    }

    /// C's `EmptyFits` once the source has been read.
    fn empty_fits_inner(&mut self) {
        self.cards = CardList::new();
        self.keyseq.clear();
    }

    /// Format all stored cards as a FITS header string.
    ///
    /// Each card is formatted as an 80-character line, joined with newlines.
    /// `END` cards already present in the card list (stored verbatim by
    /// `from_header`/`put_cards`) are written out in their original
    /// position, matching C `WriteToSink`, which never synthesises one on a
    /// read-then-rewrite round trip. A trailing `END` is appended only when
    /// the card list has none at all — e.g. a `FitsChan` built purely from
    /// typed setters. A string value too long for its card is AST__BDFTS.
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
        method: &str,
    ) -> AstResult<Option<Got>> {
        self.read_from_source()?;
        let lname = match name {
            Some(name) => {
                let (lname, _) = self.split_name(name)?;
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
    /// `None`, as an `int` (C's `astGetFitsI`); the search is as for
    /// [`get_fits_f`](Self::get_fits_f). A value that cannot be converted is
    /// AST__FTCNV, an undefined one AST__FUNDEF.
    pub fn get_fits_i(&mut self, name: Option<&str>) -> AstResult<Option<i32>> {
        Ok(match self.get_value(name, Want::Int, "astGetFitsI")? {
            Some(Got::Int(v)) => Some(v),
            _ => None,
        })
    }

    /// As [`get_fits_i`](Self::get_fits_i) for a 64-bit integer (C's
    /// `astGetFitsK`).
    pub fn get_fits_k(&mut self, name: Option<&str>) -> AstResult<Option<i64>> {
        Ok(match self.get_value(name, Want::KInt, "astGetFitsK")? {
            Some(Got::KInt(v)) => Some(v),
            _ => None,
        })
    }

    /// The value of keyword `name`, or of the current card when `name` is
    /// `None`, as a floating value (C's `astGetFitsF`).
    ///
    /// The card searched for is the one after the current card if it has
    /// that keyword, otherwise the first that does (`name` may be a
    /// template, as for [`find_fits`](Self::find_fits)); it becomes current.
    /// A missing keyword gives `None` and leaves the FitsChan at
    /// end-of-file. A value that cannot be converted is AST__FTCNV, an
    /// undefined one AST__FUNDEF.
    pub fn get_fits_f(&mut self, name: Option<&str>) -> AstResult<Option<f64>> {
        Ok(match self.get_value(name, Want::Float, "astGetFitsF")? {
            Some(Got::Float(v)) => Some(v),
            _ => None,
        })
    }

    /// As [`get_fits_f`](Self::get_fits_f) for a string (C's
    /// `astGetFitsS`). A string of 8 characters or fewer loses its trailing
    /// spaces.
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

    /// As [`get_fits_f`](Self::get_fits_f) for a CONTINUE (long) string
    /// (C's `astGetFitsCN`).
    pub fn get_fits_cn(&mut self, name: Option<&str>) -> AstResult<Option<String>> {
        Ok(
            match self.get_value(name, Want::Continue, "astGetFitsCN")? {
                Some(Got::Str(v)) => Some(v),
                _ => None,
            },
        )
    }

    /// As [`get_fits_f`](Self::get_fits_f) for a logical value (C's
    /// `astGetFitsL`).
    pub fn get_fits_l(&mut self, name: Option<&str>) -> AstResult<Option<bool>> {
        Ok(match self.get_value(name, Want::Logical, "astGetFitsL")? {
            Some(Got::Logical(v)) => Some(v),
            _ => None,
        })
    }

    /// As [`get_fits_f`](Self::get_fits_f) for a complex floating value,
    /// real part first (C's `astGetFitsCF`).
    pub fn get_fits_cf(&mut self, name: Option<&str>) -> AstResult<Option<(f64, f64)>> {
        Ok(
            match self.get_value(name, Want::ComplexF, "astGetFitsCF")? {
                Some(Got::ComplexF(re, im)) => Some((re, im)),
                _ => None,
            },
        )
    }

    /// As [`get_fits_f`](Self::get_fits_f) for a complex integer value,
    /// real part first (C's `astGetFitsCI`).
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
    ) -> AstResult<()> {
        self.read_from_source()?;
        let (lname, lcom) = self.split_name(name)?;
        let com = if comment.trim().is_empty() {
            lcom
        } else {
            comment.to_string()
        };
        self.store_card(lname, value, &com, overwrite);
        Ok(())
    }

    /// `InsCard` with a comment that, when blank and overwriting, is taken
    /// from the card replaced. Leading and trailing spaces are removed from
    /// the comment, as C's `NewCard` does for a card with a value.
    fn store_card(&mut self, keyword: String, value: FitsValue, comment: &str, overwrite: bool) {
        let mut com = comment.trim().to_string();
        if com.is_empty() && overwrite {
            com = self
                .cards
                .current()
                .and_then(|c| c.c_comment())
                .unwrap_or("")
                .trim()
                .to_string();
        }
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value,
            comment: (!com.is_empty()).then_some(com),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.cards.ins_card(card, overwrite);
    }

    /// Store an integer (C's `astSetFitsI`). The card is inserted in front of
    /// the current card, or replaces it with `overwrite`, and the card after
    /// it becomes current. The first 8 characters of `name` are the keyword;
    /// with a blank `comment` any further text is the comment, and failing
    /// that, when overwriting, the comment of the card replaced.
    pub fn set_fits_i(
        &mut self,
        name: &str,
        value: i32,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(name, FitsValue::Int(i64::from(value)), comment, overwrite)
    }

    /// As [`set_fits_i`](Self::set_fits_i) for a 64-bit integer (C's
    /// `astSetFitsK`).
    pub fn set_fits_k(
        &mut self,
        name: &str,
        value: i64,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(name, FitsValue::KInt(value), comment, overwrite)
    }

    /// As [`set_fits_i`](Self::set_fits_i) for a floating value (C's
    /// `astSetFitsF`). The value is formatted with the FitsDigits and
    /// FitsRounding attributes when the card is written.
    pub fn set_fits_f(
        &mut self,
        name: &str,
        value: f64,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(name, FitsValue::Float(value), comment, overwrite)
    }

    /// As [`set_fits_i`](Self::set_fits_i) for a string (C's
    /// `astSetFitsS`).
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
        )
    }

    /// As [`set_fits_i`](Self::set_fits_i) for a CONTINUE (long) string
    /// (C's `astSetFitsCN`).
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
        )
    }

    /// As [`set_fits_i`](Self::set_fits_i) for a logical value (C's
    /// `astSetFitsL`).
    pub fn set_fits_l(
        &mut self,
        name: &str,
        value: bool,
        comment: &str,
        overwrite: bool,
    ) -> AstResult<()> {
        self.set_value(name, FitsValue::Logical(value), comment, overwrite)
    }

    /// As [`set_fits_i`](Self::set_fits_i) for a complex floating value,
    /// real part first (C's `astSetFitsCF`).
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
        )
    }

    /// As [`set_fits_i`](Self::set_fits_i) for a complex integer value, real
    /// part first (C's `astSetFitsCI`).
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
        )
    }

    /// As [`set_fits_i`](Self::set_fits_i) for a keyword with no value (C's
    /// `astSetFitsU`).
    pub fn set_fits_u(&mut self, name: &str, comment: &str, overwrite: bool) -> AstResult<()> {
        self.set_value(name, FitsValue::Undef, comment, overwrite)
    }

    /// Store a commentary card with a blank keyword and the text `comment`
    /// (C's `astSetFitsCM`), inserted or overwriting as for
    /// [`set_fits_i`](Self::set_fits_i).
    pub fn set_fits_cm(&mut self, comment: &str, overwrite: bool) -> AstResult<()> {
        self.set_fits_com("", comment, overwrite)
    }

    /// C's `SetFitsCom`: store a commentary card with keyword `name`. When
    /// overwriting a card with the same keyword, the card keeps its type and
    /// value and takes the new comment.
    fn set_fits_com(&mut self, name: &str, comment: &str, overwrite: bool) -> AstResult<()> {
        self.read_from_source()?;
        let (lname, _) = self.split_name(name)?;
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

    /// Set the encoding format explicitly.
    ///
    /// After this call [`FitsChan::test_encoding`] returns `true` and
    /// [`FitsChan::encoding`] returns the supplied value verbatim.
    /// Mirrors C `astSetEncoding`.
    pub fn set_encoding(&mut self, enc: Encoding) {
        self.encoding = Some(enc);
    }

    /// Clear the explicit encoding setting.
    ///
    /// After this call [`FitsChan::test_encoding`] returns `false` and
    /// [`FitsChan::encoding`] auto-detects from the cards on demand.
    /// Mirrors C `astClearEncoding`.
    pub fn clear_encoding(&mut self) {
        self.encoding = None;
    }

    /// Whether the encoding has been explicitly set. Mirrors C
    /// `astTestEncoding`.
    pub fn test_encoding(&self) -> bool {
        self.encoding.is_some()
    }

    /// Return the current encoding.
    ///
    /// If the user explicitly set the encoding via
    /// [`FitsChan::set_encoding`], that value is returned. Otherwise the
    /// encoding is auto-detected from the header cards on each call —
    /// no internal state is mutated. Mirrors C `astGetEncoding`
    /// (fitschan.c:12518) which lazily evaluates the encoding from the
    /// cards when no explicit value has been set via `astSetEncoding`.
    pub fn encoding(&self) -> Encoding {
        self.encoding
            .unwrap_or_else(|| guess_encoding::guess_encoding(&self.cards))
    }

    /// Set the FitsDigits attribute: the number of significant digits a
    /// floating point value is written with. A negative value `-n` means
    /// at most `n`, fewer if the value would otherwise be wider than the 20
    /// columns FITS allows it.
    pub fn set_fits_digits(&mut self, digits: i32) {
        self.fits_digits = digits;
    }

    /// The FitsDigits attribute (default 17).
    pub fn fits_digits(&self) -> i32 {
        self.fits_digits
    }

    /// Whether FitsDigits differs from its default. As in C, which tests
    /// the value rather than whether one was set, a FitsDigits set to 17
    /// tests as unset.
    pub fn test_fits_digits(&self) -> bool {
        self.fits_digits != DEFAULT_FITS_DIGITS
    }

    /// Clear FitsDigits, restoring its default.
    pub fn clear_fits_digits(&mut self) {
        self.fits_digits = DEFAULT_FITS_DIGITS;
    }

    /// Set the FitsRounding attribute: the number of leading significant
    /// digits of a floating point value that are never changed when long
    /// runs of 0s or 9s after them are rounded out. A negative value is
    /// stored as 0, as C stores it.
    pub fn set_fits_rounding(&mut self, fitsrnd: i32) {
        self.fits_rounding = Some(fitsrnd.max(0));
    }

    /// The FitsRounding attribute (default 10).
    pub fn fits_rounding(&self) -> i32 {
        self.fits_rounding.unwrap_or(10)
    }

    /// Whether FitsRounding has been set.
    pub fn test_fits_rounding(&self) -> bool {
        self.fits_rounding.is_some()
    }

    /// Clear FitsRounding, restoring its default.
    pub fn clear_fits_rounding(&mut self) {
        self.fits_rounding = None;
    }

    /// The Clean attribute: whether a read that fails still removes the
    /// cards it used, and, as C does, whether warnings are suppressed. The
    /// default is false.
    pub fn clean(&self) -> bool {
        self.clean.unwrap_or(false)
    }

    /// Set Clean.
    pub fn set_clean(&mut self, clean: bool) {
        self.clean = Some(clean);
    }

    /// Whether Clean is set.
    pub fn test_clean(&self) -> bool {
        self.clean.is_some()
    }

    /// Clear Clean.
    pub fn clear_clean(&mut self) {
        self.clean = None;
    }

    /// The SipOK attribute: whether a FITS-WCS write may describe
    /// distortion with SIP coefficients. Without them, or with another
    /// encoding, the write uses a linear approximation if FitsTol allows.
    /// The default is true.
    pub fn sip_ok(&self) -> bool {
        self.sip_ok.unwrap_or(true)
    }

    /// Set SipOK.
    pub fn set_sip_ok(&mut self, sip_ok: bool) {
        self.sip_ok = Some(sip_ok);
    }

    /// Whether SipOK is set.
    pub fn test_sip_ok(&self) -> bool {
        self.sip_ok.is_some()
    }

    /// Clear SipOK.
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
            warnings: Default::default(),
            alt_axes: self.alt_axes(),
            fits_axis_order: self.fits_axis_order().to_string(),
            crval_source: Some(std::rc::Rc::new(self.clone())),
        }
    }

    /// Set the CDMatrix attribute: whether a FITS-WCS write describes the
    /// linear transformation with CDi_j keywords rather than PCi_j and
    /// CDELTi.
    pub fn set_cd_matrix(&mut self, use_cd: bool) {
        self.cd_matrix = Some(use_cd);
    }

    /// Whether CDMatrix has been set.
    pub fn test_cd_matrix(&self) -> bool {
        self.cd_matrix.is_some()
    }

    /// Clear CDMatrix, restoring its default.
    pub fn clear_cd_matrix(&mut self) {
        self.cd_matrix = None;
    }

    /// Set the SipReplace attribute: whether reading a SIP header replaces
    /// any inverse it supplies by a polynomial fitted to the forward
    /// transformation.
    pub fn set_sip_replace(&mut self, v: bool) {
        self.sip_replace = Some(v);
    }

    /// The SipReplace attribute (default true).
    pub fn sip_replace(&self) -> bool {
        self.sip_replace.unwrap_or(true)
    }

    /// Whether SipReplace has been set.
    pub fn test_sip_replace(&self) -> bool {
        self.sip_replace.is_some()
    }

    /// Clear SipReplace, restoring its default.
    pub fn clear_sip_replace(&mut self) {
        self.sip_replace = None;
    }

    /// Set the CarLin attribute: whether a `-CAR` projection is read as a
    /// linear transformation from pixel to sky coordinates rather than as
    /// a spherical Cartesian projection.
    pub fn set_car_lin(&mut self, v: bool) {
        self.car_lin = Some(v);
    }

    /// The CarLin attribute (default false).
    pub fn car_lin(&self) -> bool {
        self.car_lin.unwrap_or(false)
    }

    /// Whether CarLin has been set.
    pub fn test_car_lin(&self) -> bool {
        self.car_lin.is_some()
    }

    /// Clear CarLin, restoring its default.
    pub fn clear_car_lin(&mut self) {
        self.car_lin = None;
    }

    /// Set the FitsTol attribute: the maximum departure from linearity, in
    /// pixels, that a Mapping may show and still be described by the
    /// linear FITS-WCS keywords (CRPIX, CD, PC, CDELT), with or without SIP
    /// distortion. A Mapping that departs further has no FITS-WCS
    /// representation and [`Channel::write`] reports that it wrote
    /// nothing. A negative value is stored as 0, as C stores it.
    pub fn set_fits_tol(&mut self, tol: f64) {
        self.fits_tol = Some(tol.max(0.0));
    }

    /// The FitsTol attribute (default 0.1).
    pub fn fits_tol(&self) -> f64 {
        self.fits_tol.unwrap_or(fset_to_store::FITS_TOL_DEFAULT)
    }

    /// Whether FitsTol has been set.
    pub fn test_fits_tol(&self) -> bool {
        self.fits_tol.is_some()
    }

    /// Clear FitsTol, restoring its default.
    pub fn clear_fits_tol(&mut self) {
        self.fits_tol = None;
    }

    /// Set the DefB1950 attribute: whether a celestial coordinate system
    /// read from a header with neither RADESYS nor EQUINOX is FK4 B1950
    /// rather than ICRS.
    pub fn set_def_b1950(&mut self, v: bool) {
        self.def_b1950 = Some(v);
    }

    /// The DefB1950 attribute. Unless set, it is true when the Encoding is
    /// not FITS-WCS (C's `GetDefB1950`).
    pub fn def_b1950(&self) -> bool {
        self.def_b1950
            .unwrap_or_else(|| self.encoding() != Encoding::FitsWcs)
    }

    /// Whether DefB1950 has been set.
    pub fn test_def_b1950(&self) -> bool {
        self.def_b1950.is_some()
    }

    /// Clear DefB1950, restoring its default.
    pub fn clear_def_b1950(&mut self) {
        self.def_b1950 = None;
    }

    /// Set the PolyTan attribute: whether a `-TAN` projection with PVi_m
    /// distortion terms is read as the polynomial (`-TPN`) form of the
    /// distorted TAN convention. Zero reads it as plain TAN, ignoring the
    /// PV terms; a positive value as `-TPN`; a negative value decides from
    /// the cards, using `-TPN` when there are latitude PV terms, or
    /// longitude PV terms with m above 4, not all zero.
    pub fn set_poly_tan(&mut self, v: i32) {
        self.poly_tan = Some(v);
    }

    /// The PolyTan attribute (default -1).
    pub fn poly_tan(&self) -> i32 {
        self.poly_tan.unwrap_or(-1)
    }

    /// Whether PolyTan has been set.
    pub fn test_poly_tan(&self) -> bool {
        self.poly_tan.is_some()
    }

    /// Clear PolyTan, restoring its default.
    pub fn clear_poly_tan(&mut self) {
        self.poly_tan = None;
    }

    /// Set the IgnoreBadAlt attribute: whether a FITS-WCS read leaves out an
    /// alternate axis description that cannot be read, rather than failing.
    pub fn set_ignore_bad_alt(&mut self, v: bool) {
        self.ignore_bad_alt = Some(v);
    }

    /// The IgnoreBadAlt attribute (default false).
    pub fn ignore_bad_alt(&self) -> bool {
        self.ignore_bad_alt.unwrap_or(false)
    }

    /// Whether IgnoreBadAlt has been set.
    pub fn test_ignore_bad_alt(&self) -> bool {
        self.ignore_bad_alt.is_some()
    }

    /// Clear IgnoreBadAlt, restoring its default.
    pub fn clear_ignore_bad_alt(&mut self) {
        self.ignore_bad_alt = None;
    }

    /// Set the Warnings attribute: the conditions, separated by white space
    /// and in any case, whose warnings a read or write records (C's
    /// `SetWarnings`). An unknown condition is AST__ATTIN, and leaves the
    /// attribute as it was.
    pub fn set_warning_conditions(&mut self, value: &str) -> AstResult<()> {
        warning::good_warns(value)?;
        self.warning_conditions = Some(value.to_string());
        Ok(())
    }

    /// The Warnings attribute, as set, or by default
    /// `"BadKeyName BadKeyValue Tnx Zpx BadPV BadCel BadMat BadCTYPE"`.
    ///
    /// A warning for a condition it names is stored as `ASTWARN` cards in
    /// front of the current card and added to the warnings
    /// [`warning_texts`](Channel::warning_texts) reports.
    pub fn warning_conditions(&self) -> &str {
        self.warning_conditions
            .as_deref()
            .unwrap_or(warning::DEFAULT_WARNINGS)
    }

    /// Whether Warnings has been set.
    pub fn test_warning_conditions(&self) -> bool {
        self.warning_conditions.is_some()
    }

    /// Clear Warnings, restoring its default.
    pub fn clear_warning_conditions(&mut self) {
        self.warning_conditions = None;
    }

    /// Set the AltAxes attribute: which Frames a write describes with
    /// alternate axes.
    pub fn set_alt_axes(&mut self, v: AltAxes) {
        self.alt_axes = Some(v);
    }

    /// The AltAxes attribute (default [`AltAxes::All`]).
    pub fn alt_axes(&self) -> AltAxes {
        self.alt_axes.unwrap_or_default()
    }

    /// Whether AltAxes has been set.
    pub fn test_alt_axes(&self) -> bool {
        self.alt_axes.is_some()
    }

    /// Clear AltAxes, restoring its default.
    pub fn clear_alt_axes(&mut self) {
        self.alt_axes = None;
    }

    /// Set the Iwc attribute: whether a FrameSet read with a foreign
    /// encoding includes a Frame of FITS-WCS intermediate world coordinates
    /// for each axis description, with Domain `IWC`, or `IWCa` for
    /// alternate axes `a`.
    pub fn set_iwc(&mut self, v: bool) {
        self.iwc = Some(v);
    }

    /// The Iwc attribute (default false).
    pub fn iwc(&self) -> bool {
        self.iwc.unwrap_or(false)
    }

    /// Whether Iwc has been set.
    pub fn test_iwc(&self) -> bool {
        self.iwc.is_some()
    }

    /// Clear Iwc, restoring its default.
    pub fn clear_iwc(&mut self) {
        self.iwc = None;
    }

    /// Set the FitsAxisOrder attribute: the order of the WCS axes in a
    /// FITS-WCS header a write makes. `<auto>` (the default) puts each WCS
    /// axis with the pixel axis it is most nearly parallel to; `<copy>`
    /// keeps the order of the current Frame's axes; otherwise the value is a
    /// list of axis Symbols, separated by white space, giving that order. A
    /// write fails with AST__ATTIN if the list leaves out an axis or names
    /// one twice; words naming no axis are ignored.
    pub fn set_fits_axis_order(&mut self, v: &str) {
        self.fits_axis_order = Some(v.to_string());
    }

    /// The FitsAxisOrder attribute (default `<auto>`).
    pub fn fits_axis_order(&self) -> &str {
        self.fits_axis_order.as_deref().unwrap_or("<auto>")
    }

    /// Whether FitsAxisOrder has been set.
    pub fn test_fits_axis_order(&self) -> bool {
        self.fits_axis_order.is_some()
    }

    /// Clear FitsAxisOrder, restoring its default.
    pub fn clear_fits_axis_order(&mut self) {
        self.fits_axis_order = None;
    }

    /// Set the TabOK attribute: a positive value allows the `-TAB`
    /// algorithm and is the EXTVER of the binary table a write makes for
    /// it. The `-TAB` algorithm is not yet ported, so the value is held,
    /// copied and dumped but changes no read or write.
    pub fn set_tab_ok(&mut self, v: i32) {
        self.tab_ok = Some(v);
    }

    /// The TabOK attribute (default 0).
    pub fn tab_ok(&self) -> i32 {
        self.tab_ok.unwrap_or(0)
    }

    /// Whether TabOK has been set.
    pub fn test_tab_ok(&self) -> bool {
        self.tab_ok.is_some()
    }

    /// Clear TabOK, restoring its default.
    pub fn clear_tab_ok(&mut self) {
        self.tab_ok = None;
    }

    /// Set the ForceTab attribute: whether a write uses the `-TAB`
    /// algorithm whenever TabOK allows it. As for
    /// [`set_tab_ok`](Self::set_tab_ok), the value changes no read or
    /// write yet.
    pub fn set_force_tab(&mut self, v: i32) {
        self.force_tab = Some(v);
    }

    /// The ForceTab attribute (default 0).
    pub fn force_tab(&self) -> i32 {
        self.force_tab.unwrap_or(0)
    }

    /// Whether ForceTab has been set.
    pub fn test_force_tab(&self) -> bool {
        self.force_tab.is_some()
    }

    /// Clear ForceTab, restoring its default.
    pub fn clear_force_tab(&mut self) {
        self.force_tab = None;
    }

    /// The Object-class `Ident` attribute, blank unless set.
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set `Ident`.
    pub fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    /// Whether `Ident` is set.
    pub fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    /// Clear `Ident`.
    pub fn clear_ident(&mut self) {
        self.ident = None;
    }

    /// The Object-class `ID` attribute, blank unless set. Unlike `Ident`, a
    /// clone does not inherit it, as C's `astCopy` does not copy it.
    pub fn id(&self) -> &str {
        self.id.get()
    }

    /// Set `ID`.
    pub fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }

    /// Whether `ID` is set.
    pub fn test_id(&self) -> bool {
        self.id.test()
    }

    /// Clear `ID`.
    pub fn clear_id(&mut self) {
        self.id.clear();
    }

    /// The read-only AllWarnings attribute: every condition the Warnings
    /// attribute accepts, in lower case, with a space before and after each.
    pub fn all_warning_conditions(&self) -> &'static str {
        warning::all_warnings()
    }

    /// C's `ReadCrval` (fitschan.c:26094): the reference point that the
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
        use crate::frames::{Frame, FrameRef};
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
        let tfs = Frame::convert_to(&fs, wcsfrm, &[domain.as_str()]).ok()??;
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

    /// The CDMatrix attribute. Unless set, it is true when the FitsChan
    /// holds a `CDi_j` card that a read has not consumed (C's
    /// `GetCDMatrix`).
    pub fn cd_matrix(&self) -> bool {
        self.cd_matrix
            .unwrap_or_else(|| self.cards.key_fields("CD%1d_%1d", 0).0 > 0)
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
        &self,
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
        wcs_read::decode_wcs_with_alts_inner(
            &stores,
            def_b1950,
            self.ignore_bad_alt(),
            self.iwc(),
            &self.cards,
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
    #[allow(clippy::too_many_arguments)]
    fn finish_foreign_read(
        &mut self,
        decoded: AstResult<Option<AnyObject>>,
        entry: usize,
        trans_warnings: spec_trans::TransWarnings,
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
        let translated = trans.take_written_names();
        let clean = self.clean();
        let mark = |cards: &mut CardList, names: &[String]| {
            for name in names {
                cards.mark_all_cards(name);
            }
        };
        // `SpecTrans` warns before `WcsFcRead` sweeps the cards, which warns
        // in front of each card it cannot convert.
        self.push_trans_warnings(entry, trans_warnings);
        let (sweep_warnings, swept) = wcs_mark::mark_wcs_cards(&mut self.cards, &translated);
        self.push_sweep_warnings(sweep_warnings);
        match (decoded, swept) {
            (Ok(Some(obj)), Ok(())) => {
                mark(&mut self.cards, &spec_names);
                mark(&mut self.cards, &extract_names);
                self.cards.fix_used(true, false);
                self.push_read_warnings(read_warnings, &cursors);
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
                    self.push_read_warnings(read_warnings, &cursors);
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
    /// (fitschan.c:18850-18861).
    fn push_trans_warnings(&mut self, entry: usize, warnings: spec_trans::TransWarnings) {
        if self.clean() {
            return;
        }
        if !warnings.entry.is_empty() {
            self.cards.set_position(entry);
            for w in warnings.entry {
                self.warn(w);
            }
        }
        if !warnings.end.is_empty() {
            self.cards.set_position(self.cards.len());
            for w in warnings.end {
                self.warn(w);
            }
        }
    }

    /// Store the `ASTWARN` blocks `WcsFcRead`'s sweep raised, each in front
    /// of the card at its index, where C's cursor is when it warns.
    fn push_sweep_warnings(&mut self, warnings: Vec<(usize, Warning)>) {
        if self.clean() {
            return;
        }
        let mut added = 0;
        for (index, w) in warnings {
            let before = self.cards.len();
            self.cards.set_position(index + added);
            self.warn(w);
            added += self.cards.len() - before;
        }
    }

    /// Store the `ASTWARN` blocks the decoding raised, each in front of the
    /// card current in C when it was raised (see [`wcs_read::ReadCursors`]).
    /// C's `DistortMaps` reads NAXIS1 to NAXIS<n> as each axis description is
    /// read, `n` its number of WCS axes, leaving the cursor
    /// `move_cursor_to_last_naxis` reproduces; reading IRAF WAT cards leaves
    /// it at end-of-file.
    fn push_read_warnings(&mut self, warnings: Warnings, cursors: &[(usize, Option<usize>)]) {
        if self.clean() {
            return;
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
            self.warn(w);
        }
    }
}

impl Clone for FitsChan {
    /// C's `astCopy`: read the source, then copy everything but the source
    /// and sink, which a copy does not inherit.
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
            clean: self.clean,
            sip_ok: self.sip_ok,
            ident: self.ident.clone(),
            id: Default::default(),
            tab_ok: self.tab_ok,
            force_tab: self.force_tab,
            source: self.source.clone(),
            sink: self.sink.clone(),
            source_file: self.source_file.clone(),
            sink_file: self.sink_file.clone(),
        };
        self.copy_reading_source(copy)
    }
}

impl Default for FitsChan {
    fn default() -> Self {
        Self::new()
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

        let enc = self.encoding();
        let def_b1950 = self.def_b1950();
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
                    // `remove` set for NATIVE encoding (fitschan.c:25880-25886),
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
                    // round-trip — mirrors fitschan.c:11482-11490 which
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
                    );
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    let mut read_warnings: Warnings = Vec::new();
                    let mut cursors = Vec::new();
                    let decoded =
                        self.decode_wcs_stores(&trans, def_b1950, &mut cursors, &mut read_warnings);
                    self.finish_foreign_read(
                        decoded,
                        entry,
                        trans_warnings,
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
                    );
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    let mut read_warnings: Warnings = Vec::new();
                    let mut cursors = Vec::new();
                    let decoded =
                        self.decode_wcs_stores(&trans, def_b1950, &mut cursors, &mut read_warnings);
                    self.finish_foreign_read(
                        decoded,
                        entry,
                        trans_warnings,
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
                    );
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    let mut read_warnings: Warnings = Vec::new();
                    let mut cursors = Vec::new();
                    let decoded =
                        self.decode_wcs_stores(&trans, def_b1950, &mut cursors, &mut read_warnings);
                    self.finish_foreign_read(
                        decoded,
                        entry,
                        trans_warnings,
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
                    );
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    let mut read_warnings: Warnings = Vec::new();
                    let mut cursors = Vec::new();
                    let decoded =
                        self.decode_wcs_stores(&trans, def_b1950, &mut cursors, &mut read_warnings);
                    self.finish_foreign_read(
                        decoded,
                        entry,
                        trans_warnings,
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
                    );
                    // The cards SpecTrans used, which C marks before WcsFcRead runs.
                    let spec_names = trans.take_consumed_names();
                    let mut read_warnings: Warnings = Vec::new();
                    let mut cursors = Vec::new();
                    let decoded =
                        self.decode_wcs_stores(&trans, def_b1950, &mut cursors, &mut read_warnings);
                    self.finish_foreign_read(
                        decoded,
                        entry,
                        trans_warnings,
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
                    let mut read_warnings: Warnings = Vec::new();
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
                            &mut read_warnings,
                            &mut cursors,
                        )
                    })();
                    // The DSS reader records the cards it used on `self.cards`.
                    let clean = self.clean();
                    match decoded {
                        Ok(obj) => {
                            self.push_read_warnings(read_warnings, &cursors);
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
                            self.push_read_warnings(read_warnings, &cursors);
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
        // remove them again. Mirrors C `Write` (fitschan.c:40690-40694).
        let card0 = self.cards.position();
        self.cards.begin_marking_new();
        let result = self.write_inner(obj);

        // A write that failed, for either reason, leaves the header exactly
        // as it was found: the cards it had added are removed and the entry
        // card restored. Mirrors fitschan.c:40814-40830.
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
        let enc = self.encoding();
        match enc {
            Encoding::Native => {
                let node = serial::any_object_to_node(obj)?;
                self.store_keyword_native(obj, &node)?;
                // The native encoding can represent every AST class.
                // Mirrors fitschan.c:40758.
                Ok(1)
            }
            _ => {
                // For all FITS-WCS encodings, preserve the relative position
                // of any WCS card that already exists in the input header by
                // letting the encoder overwrite it in place. Every other card
                // is kept, those a read consumed staying hidden, as C keeps
                // them.
                // Extract NAXISn dims (in WCS axis order) from the input
                // header before positioning the cursor. The encoder uses these
                // for FindBasisVectors-style central-pixel selection; mirrors
                // fitschan.c::AddVersion's `dim` parameter.
                let dims = extract_naxis_dims(&self.cards);

                self.cards.position_write_cursor();

                let attrs = self.write_attrs();
                let write_warnings = attrs.warnings.clone();
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
                let cd_matrix = self.cd_matrix();
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
                        // fitschan.c:6888-6943.
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
                        self.warn(w);
                    }
                }

                let ret = usize::from(result?);
                // A successful write leaves the current card at end-of-file,
                // so a subsequent write appends after this object. Mirrors
                // fitschan.c:40801.
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
/// loop does (fitschan.c:8861-8865), from the end-of-file cursor `WcsFcRead`'s
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

/// Extract NAXISn dimensions from a card list, in WCS axis order
/// (`NAXIS1` → index 0, `NAXIS2` → index 1, …). The total axis count
/// comes from the `NAXIS` keyword when present; otherwise we scan
/// NAXIS{i} keywords directly until one is missing (some `.head`
/// fixtures omit NAXIS but include NAXIS1, NAXIS2, …). Missing or
/// non-positive dimensions become `None`. Mirrors the `dim` array
/// C's `astWriteFitsChan` builds before calling `AddVersion`.
fn extract_naxis_dims(cards: &CardList) -> Vec<Option<f64>> {
    let mut dims = Vec::new();
    if let Some(n) = cards.get_int("NAXIS")
        && n > 0
    {
        for i in 1..=(n as usize) {
            dims.push(
                cards
                    .get_int(&format!("NAXIS{i}"))
                    .filter(|&d| d > 0)
                    .map(|d| d as f64),
            );
        }
    } else {
        for i in 1.. {
            match cards.get_int(&format!("NAXIS{i}")) {
                Some(d) if d > 0 => dims.push(Some(d as f64)),
                Some(_) => dims.push(None),
                None => break,
            }
        }
    }
    dims
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
        let mut fc = FitsChan::new();
        assert_eq!(fc.ncard().unwrap(), 0);
        // No explicit encoding set; auto-detect on an empty chan
        // returns Native per C `astGetEncoding` (fitschan.c:12561).
        assert_eq!(fc.encoding(), Encoding::Native);
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
        let mut fc = FitsChan::new();
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
        let mut fc = FitsChan::new();
        fc.set_fits_f("CRVAL1", 123.456, "Test value", false)
            .unwrap();
        let val = fc.get_fits_f(Some("CRVAL1")).unwrap().unwrap();
        assert!((val - 123.456).abs() < 1e-10);
    }

    #[test]
    fn set_fits_i_and_get_fits_i() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("NAXIS", 3, "Number of axes", false).unwrap();
        assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(3));
    }

    #[test]
    fn set_fits_s_and_get_fits_s() {
        let mut fc = FitsChan::new();
        fc.set_fits_s("CTYPE1", "RA---TAN", "Axis type", false)
            .unwrap();
        assert_eq!(
            fc.get_fits_s(Some("CTYPE1")).unwrap(),
            Some("RA---TAN".to_string())
        );
    }

    #[test]
    fn set_fits_l_and_get_fits_l() {
        let mut fc = FitsChan::new();
        fc.set_fits_l("SIMPLE", true, "Standard FITS", false)
            .unwrap();
        assert_eq!(fc.get_fits_l(Some("SIMPLE")).unwrap(), Some(true));
    }

    #[test]
    fn ncard_tracks_insertions() {
        let mut fc = FitsChan::new();
        assert_eq!(fc.ncard().unwrap(), 0);
        fc.set_fits_i("A", 1, "", false).unwrap();
        assert_eq!(fc.ncard().unwrap(), 1);
        fc.set_fits_i("B", 2, "", false).unwrap();
        assert_eq!(fc.ncard().unwrap(), 2);
    }

    #[test]
    fn empty_fits_clears_all() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("NAXIS", 2, "", false).unwrap();
        fc.set_fits_i("NAXIS1", 100, "", false).unwrap();
        assert_eq!(fc.ncard().unwrap(), 2);
        fc.empty_fits().unwrap();
        assert_eq!(fc.ncard().unwrap(), 0);
    }

    #[test]
    fn overwrite_replaces_existing() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("NAXIS", 2, "Original", false).unwrap();
        // Overwriting replaces the current card.
        fc.clear_card().unwrap();
        fc.set_fits_i("NAXIS", 3, "Updated", true).unwrap();
        assert_eq!(fc.ncard().unwrap(), 1); // still only one card
        assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(3));
    }

    #[test]
    fn encoding_accessors() {
        let mut fc = FitsChan::new();
        fc.set_encoding(Encoding::Native);
        assert_eq!(fc.encoding(), Encoding::Native);
    }

    #[test]
    fn fits_digits_accessors() {
        let mut fc = FitsChan::new();
        fc.set_fits_digits(15);
        assert_eq!(fc.fits_digits(), 15);
        fc.set_fits_digits(-3);
        assert_eq!(fc.fits_digits(), -3);
    }

    #[test]
    fn del_fits_removes_current() {
        let mut fc = FitsChan::new();
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

        let mut fc = FitsChan::new();
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
        let mut fc = FitsChan::new();
        fc.set_fits_u("BLANK", "Undefined", false).unwrap();
        let card = fc.cards.find("BLANK").unwrap();
        assert_eq!(card.value, FitsValue::Undef);
    }

    #[test]
    fn set_fits_cm_stores_comment() {
        let mut fc = FitsChan::new();
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
        let mut fc = FitsChan::new();
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

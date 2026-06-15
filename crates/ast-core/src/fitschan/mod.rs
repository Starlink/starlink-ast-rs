//! FitsChan — FITS header I/O for AST objects.
//!
//! A [`FitsChan`] stores a sequence of FITS header cards and provides the
//! ability to read AST objects from them (using the Native encoding stored
//! in COMMENT cards) and to write AST objects into the header.

pub mod card;
pub mod card_list;
pub(crate) mod enc_aips;
pub(crate) mod enc_class;
pub(crate) mod enc_dss;
pub(crate) mod enc_iraf;
pub(crate) mod enc_native;
pub(crate) mod enc_pc;
pub(crate) mod fits_store;
pub mod format;
pub(crate) mod fset_to_store;
pub(crate) mod guess_encoding;
pub(crate) mod spec_trans;
pub(crate) mod wcs_celestial;
pub(crate) mod wcs_decompose;
pub(crate) mod wcs_keywords;
pub(crate) mod wcs_matrix;
pub(crate) mod wcs_read;
pub(crate) mod wcs_spectral;
pub(crate) mod wcs_write;

use card::{FitsCard, FitsValue};
use card_list::CardList;

use crate::channel::{Channel, ChannelConfig};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::node::Node;
use crate::serial::{self, AnyObject};

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

/// Default number of significant figures for FITS floating-point output.
///
/// Matches C `AST__DBL_DIG` (the default of the `FitsDigits` attribute,
/// fitschan.c:42946), which is 17 when AST is built in C11 mode — enough to
/// round-trip an IEEE double exactly. Callers that want the FITS-standard
/// 20-column limit honoured set a smaller value explicitly (the wcsconv
/// fixtures use `FitsDigits=8` for the FITS encodings).
const DEFAULT_FITS_DIGITS: usize = 17;

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
/// assert_eq!(fc.ncard(), 2);
/// assert_eq!(fc.get_fits_i("NAXIS"), Some(2));
/// ```
pub struct FitsChan {
    config: ChannelConfig,
    cards: CardList,
    /// Explicit `Encoding` set by the user via [`FitsChan::set_encoding`].
    /// `None` means "not set" — [`FitsChan::encoding`] auto-detects from
    /// the cards on demand. Mirrors C's `astTestEncoding` /
    /// `astGetEncoding` (fitschan.c:12466). The `DefB1950` default also
    /// keys off whether the encoding has been explicitly set
    /// (fitschan.c:42286).
    encoding: Option<Encoding>,
    warnings: Vec<String>,
    fits_digits: usize,
    /// Number of leading significant digits protected from rounding by
    /// the formatter (mirrors C's `FitsRounding` attribute, default 10).
    fits_rounding: usize,
    /// Whether to use CD matrix notation (default false).
    cd_matrix: bool,
    /// The SipReplace attribute (fitschan.c default 1): when reading a
    /// SIP header, replace any supplied inverse by an astPolyTran fit
    /// (falling back to the iterative inverse if the fit fails).
    sip_replace: bool,
}

impl FitsChan {
    /// Create an empty FitsChan with no explicit encoding.
    ///
    /// The encoding is left unset — [`FitsChan::encoding`] returns the
    /// auto-detected encoding (default `FitsWcs` for an empty chan).
    /// Mirrors C `astFitsChan(...)` followed by `astTestEncoding == 0`.
    pub fn new() -> Self {
        Self {
            config: ChannelConfig::default(),
            cards: CardList::new(),
            encoding: None,
            warnings: Vec::new(),
            fits_digits: DEFAULT_FITS_DIGITS,
            fits_rounding: 10,
            cd_matrix: false,
            sip_replace: true,
        }
    }

    /// Create a FitsChan from FITS header text.
    ///
    /// The text is split into lines and each is parsed as a FITS card.
    /// END cards are skipped; unparseable lines are silently ignored.
    pub fn from_header(text: &str) -> AstResult<Self> {
        let cards = CardList::from_header(text)?;
        Ok(Self {
            config: ChannelConfig::default(),
            cards,
            encoding: None,
            warnings: Vec::new(),
            fits_digits: DEFAULT_FITS_DIGITS,
            fits_rounding: 10,
            cd_matrix: false,
            sip_replace: true,
        })
    }

    // -----------------------------------------------------------------------
    // Card manipulation API
    // -----------------------------------------------------------------------

    /// Parse and store an 80-character FITS card string.
    ///
    /// If `overwrite` is true and a card with the same keyword already exists,
    /// it is replaced. Otherwise the card is inserted at the current position.
    pub fn put_fits(&mut self, card: &str, overwrite: bool) -> AstResult<()> {
        let parsed = FitsCard::parse(card)?;
        // Emit ASTWARN cards for an illegal keyword name, mirroring C
        // `CheckFitsName`/`Warn` inside `astPutFits` (fitschan.c:32259).
        if let Some(text) = crate::fitschan::card::check_fits_name(card) {
            self.cards.push_astwarn(&text);
        }
        self.cards.put(parsed, overwrite);
        Ok(())
    }

    /// Store multiple FITS cards from a multi-line string.
    ///
    /// Each line is parsed and appended. This is equivalent to calling
    /// `put_fits` for each line with `overwrite = false`.
    pub fn put_cards(&mut self, cards: &str) -> AstResult<()> {
        for line in cards.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match FitsCard::parse(line) {
                Ok(card) => {
                    if card.keyword == "END" {
                        continue;
                    }
                    // Emit ASTWARN cards for an illegal keyword name before the
                    // offending card, mirroring C `CheckFitsName`/`Warn` in
                    // `astPutFits` (fitschan.c:32259).
                    if let Some(text) = card::check_fits_name(line) {
                        self.cards.push_astwarn(&text);
                    }
                    self.cards.push(card);
                }
                Err(_) => continue,
            }
        }
        Ok(())
    }

    /// Search for a FITS card by keyword.
    ///
    /// Returns the formatted 80-character card string if found.
    /// If `inc` is true, the position is advanced past the found card;
    /// if false, it remains at the found card.
    ///
    /// The search always starts from the current position and wraps to the
    /// beginning if not found from the current position forward.
    pub fn find_fits(&mut self, name: &str, inc: bool) -> Option<String> {
        // Search forward from current position.
        if self.cards.find_next(name).is_some() {
            let found_pos = self.cards.position(); // one past the found card
            let card_idx = found_pos - 1;
            let formatted = self.cards.iter().nth(card_idx).unwrap().format();
            if !inc {
                self.cards.set_position(card_idx);
            }
            return Some(formatted);
        }
        // If not found from current position, try from beginning.
        let saved_pos = self.cards.position();
        self.cards.rewind();
        if self.cards.find_next(name).is_some() {
            let found_pos = self.cards.position(); // one past the found card
            // Only accept if we found it before the original position.
            if found_pos <= saved_pos {
                let card_idx = found_pos - 1;
                let formatted = self.cards.iter().nth(card_idx).unwrap().format();
                if !inc {
                    self.cards.set_position(card_idx);
                }
                return Some(formatted);
            }
        }
        // Not found at all — restore position.
        self.cards.set_position(saved_pos);
        None
    }

    /// Delete the card at the current position.
    pub fn del_fits(&mut self) -> AstResult<()> {
        if self.cards.at_eof() {
            return Err(AstError::new(
                ErrorCode::Bdfts,
                "no card at current position to delete",
            ));
        }
        self.cards.delete_at_position();
        Ok(())
    }

    /// Remove all cards from the FitsChan.
    pub fn empty_fits(&mut self) {
        self.cards = CardList::new();
    }

    /// Return the number of cards currently stored.
    pub fn ncard(&self) -> usize {
        self.cards.len()
    }

    /// Format all stored cards as a FITS header string.
    ///
    /// Each card is formatted as an 80-character line followed by a newline.
    /// A trailing `END` card is always appended — `from_header` strips it on
    /// read, so producing it here keeps the output a complete FITS header
    /// and the round-trip idempotent.
    pub fn to_header(&self) -> String {
        let mut lines: Vec<String> = self
            .cards
            .iter()
            .map(|card| card.format_with_digits_and_rounding(self.fits_digits, self.fits_rounding))
            .collect();
        lines.push(format!("{:<80}", "END"));
        lines.join("\n")
    }

    // -----------------------------------------------------------------------
    // Typed getters
    // -----------------------------------------------------------------------

    /// Get the integer value of a FITS keyword.
    pub fn get_fits_i(&self, name: &str) -> Option<i64> {
        self.cards.get_int(name)
    }

    /// Get the floating-point value of a FITS keyword.
    pub fn get_fits_f(&self, name: &str) -> Option<f64> {
        self.cards.get_float(name)
    }

    /// Get the string value of a FITS keyword.
    pub fn get_fits_s(&self, name: &str) -> Option<String> {
        self.cards.get_string(name)
    }

    /// Get the logical value of a FITS keyword.
    pub fn get_fits_l(&self, name: &str) -> Option<bool> {
        self.cards.get_logical(name)
    }

    // -----------------------------------------------------------------------
    // Typed setters
    // -----------------------------------------------------------------------

    /// Set an integer-valued FITS keyword.
    pub fn set_fits_i(&mut self, name: &str, value: i64, comment: &str, overwrite: bool) {
        let card = FitsCard {
            keyword: name.to_uppercase(),
            value: FitsValue::Int(value),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.cards.put(card, overwrite);
    }

    /// Set a floating-point-valued FITS keyword.
    ///
    /// The value is formatted using [`format_fits_double`](format::format_fits_double)
    /// with the current `fits_digits` setting, but stored as a typed Float value.
    pub fn set_fits_f(&mut self, name: &str, value: f64, comment: &str, overwrite: bool) {
        let card = FitsCard {
            keyword: name.to_uppercase(),
            value: FitsValue::Float(value),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.cards.put(card, overwrite);
    }

    /// Set a string-valued FITS keyword.
    pub fn set_fits_s(&mut self, name: &str, value: &str, comment: &str, overwrite: bool) {
        let card = FitsCard {
            keyword: name.to_uppercase(),
            value: FitsValue::String(value.to_string()),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.cards.put(card, overwrite);
    }

    /// Set a logical-valued FITS keyword.
    pub fn set_fits_l(&mut self, name: &str, value: bool, comment: &str, overwrite: bool) {
        let card = FitsCard {
            keyword: name.to_uppercase(),
            value: FitsValue::Logical(value),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.cards.put(card, overwrite);
    }

    /// Set an undefined-valued FITS keyword (keyword with no value).
    pub fn set_fits_u(&mut self, name: &str, comment: &str, overwrite: bool) {
        let card = FitsCard {
            keyword: name.to_uppercase(),
            value: FitsValue::Undef,
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.cards.put(card, overwrite);
    }

    /// Set a comment-only FITS card.
    pub fn set_fits_cm(&mut self, comment: &str, overwrite: bool) {
        let card = FitsCard {
            keyword: "COMMENT".to_string(),
            value: FitsValue::Comment(format!(" {comment}")),
            comment: None,
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.cards.put(card, overwrite);
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
    /// (fitschan.c:12466) which lazily evaluates the encoding from the
    /// cards when no explicit value has been set via `astSetEncoding`.
    pub fn encoding(&self) -> Encoding {
        self.encoding
            .unwrap_or_else(|| guess_encoding::guess_encoding(&self.cards))
    }

    /// Set the number of significant digits for floating-point output.
    pub fn set_fits_digits(&mut self, digits: usize) {
        self.fits_digits = digits.clamp(1, 17);
    }

    /// Return the current number of significant digits for floating-point output.
    pub fn fits_digits(&self) -> usize {
        self.fits_digits
    }

    /// Set the FitsRounding attribute (number of leading significant
    /// digits protected from sequence-rounding by the float formatter).
    /// Default 10. Setting to 0 protects only the first significant
    /// digit (matching C's `fitsrounding < 1 → 1` clamp).
    pub fn set_fits_rounding(&mut self, fitsrnd: usize) {
        self.fits_rounding = fitsrnd;
    }

    /// Return the current FitsRounding value.
    pub fn fits_rounding(&self) -> usize {
        self.fits_rounding
    }

    /// Set whether to use CD matrix notation for FITS-WCS output.
    ///
    /// When true, the output uses CDi_j keywords; when false (the default),
    /// it uses the PCi_j + CDELTi form.
    pub fn set_cd_matrix(&mut self, use_cd: bool) {
        self.cd_matrix = use_cd;
    }

    /// Set the SipReplace attribute (mirrors C `SipReplace`, default
    /// true): controls whether reading a SIP header refits the inverse
    /// via `PolyMap::poly_tran`.
    pub fn set_sip_replace(&mut self, v: bool) {
        self.sip_replace = v;
    }

    /// Return whether CD matrix notation is enabled.
    pub fn cd_matrix(&self) -> bool {
        self.cd_matrix
    }

    /// Reset the card position to the beginning.
    pub fn rewind(&mut self) {
        self.cards.rewind();
    }

    /// Expose the internal card list for use in unit tests within this crate.
    #[cfg(test)]
    pub(crate) fn cards_for_test(&self) -> &CardList {
        &self.cards
    }

    // -----------------------------------------------------------------------
    // Native encoding helpers
    // -----------------------------------------------------------------------

    /// Extract AST native text from COMMENT cards.
    ///
    /// Scans from the current position looking for the begin/end markers,
    /// collecting the native text lines in between.
    fn extract_native_text(&mut self) -> Option<String> {
        self.cards.rewind();

        // Find the begin marker.
        let mut found_begin = false;
        while !self.cards.at_eof() {
            if let Some(card) = self.cards.find_next("COMMENT") {
                if let FitsValue::Comment(text) = &card.value {
                    if text.contains("AST +++") {
                        found_begin = true;
                        break;
                    }
                }
            } else {
                break;
            }
        }

        if !found_begin {
            return None;
        }

        // Collect lines until end marker.
        let mut lines = Vec::new();
        loop {
            if self.cards.at_eof() {
                break;
            }
            let pos = self.cards.position();
            if let Some(card) = self.cards.find_next("COMMENT") {
                if let FitsValue::Comment(text) = &card.value {
                    if text.contains("AST ---") {
                        break;
                    }
                    // Extract the native text between " AST " prefix and " AST" suffix.
                    let line = extract_ast_line(text);
                    lines.push(line);
                }
            } else {
                // Restore position if find_next failed.
                self.cards.set_position(pos);
                break;
            }
        }

        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    /// Store an object in the keyword-card NATIVE form: a leading banner of
    /// COMMENT cards, the BEGAST/ENDAST/ISA/`<NAME>_xx` data cards, and a
    /// trailing banner. This is the C library's default NATIVE storage
    /// (fitschan.c::Write, line 40206).
    ///
    /// The block is prepended to the existing cards, mirroring C `astWrite`
    /// which rewinds the card pointer before writing so the AST description
    /// precedes the retained original header cards.
    fn store_keyword_native(&mut self, obj: &AnyObject, node: &Node) {
        let comm = self.config.comment;
        let class = node.class.as_str();
        let is_frameset = matches!(obj, AnyObject::FrameSet(_));

        let banner = |prefix: &str, middle: &str, suffix: &str| FitsCard {
            keyword: "COMMENT".to_string(),
            value: FitsValue::Comment(enc_native::make_banner(prefix, middle, suffix)),
            comment: None,
            used: false,
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
                used: false,
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
        enc_native::write_keyword_native(&mut block, node, comm);

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

        self.cards.insert_front(block);
    }
}

impl Default for FitsChan {
    fn default() -> Self {
        Self::new()
    }
}

impl Channel for FitsChan {
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.warnings.clear();

        let enc = self.encoding();
        match enc {
            Encoding::Native => {
                // Two storage variants: keyword-card (BEGAST_xx / ENDAST_xx)
                // and COMMENT-card (`COMMENT  AST +++  AST` markers).
                // The keyword-card variant takes precedence when present.
                let text = if enc_native::has_keyword_native(&self.cards) {
                    enc_native::cards_to_native_text(&self.cards)?
                } else {
                    match self.extract_native_text() {
                        Some(t) => t,
                        None => return Ok(None),
                    }
                };
                if text.trim().is_empty() {
                    return Ok(None);
                }
                let node = serial::native_read::parse_native(&text)?;
                let obj = serial::any_object_from_node(&node)?;
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
                // round-trip — mirrors fitschan.c:11430-11438 which
                // creates a temporary FitsChan `ret` and deletes it
                // after WcsToStore.
                let trans = spec_trans::spec_trans(&self.cards, Encoding::FitsWcs);
                let (mut primary, mut alts) = wcs_keywords::extract_wcs_keywords_with_alts(&trans)?;
                primary.sip_replace = self.sip_replace;
                for (_, st) in &mut alts {
                    st.sip_replace = self.sip_replace;
                }
                let obj = wcs_read::decode_wcs_with_alts(&primary, &alts)?;
                Ok(Some(obj))
            }
            Encoding::FitsPc => {
                // Run SpecTrans first to normalise non-standard
                // keywords (Cnxxx alt-axis form, CDjjjiii, CROTA,
                // PROJP, etc.) into their canonical equivalents.
                // After SpecTrans, alt-axis cards are in the *A..*Z
                // form, so we route through the multi-plane
                // FitsWcs decoder to pick up secondary planes.
                let trans = spec_trans::spec_trans(&self.cards, Encoding::FitsPc);
                let (mut primary, mut alts) = wcs_keywords::extract_wcs_keywords_with_alts(&trans)?;
                primary.sip_replace = self.sip_replace;
                for (_, st) in &mut alts {
                    st.sip_replace = self.sip_replace;
                }
                let obj = wcs_read::decode_wcs_with_alts_def_b1950(&primary, &alts, true)?;
                Ok(Some(obj))
            }
            Encoding::FitsIraf => {
                let trans = spec_trans::spec_trans(&self.cards, Encoding::FitsIraf);
                let (mut primary, mut alts) = wcs_keywords::extract_wcs_keywords_with_alts(&trans)?;
                primary.sip_replace = self.sip_replace;
                for (_, st) in &mut alts {
                    st.sip_replace = self.sip_replace;
                }
                let obj = wcs_read::decode_wcs_with_alts_def_b1950(&primary, &alts, true)?;
                Ok(Some(obj))
            }
            Encoding::FitsAips | Encoding::FitsAipsPP => {
                let trans = spec_trans::spec_trans(&self.cards, enc);
                let store = enc_aips::read_from_cards(&trans)?;
                let obj = wcs_read::decode_wcs_with_def_b1950(&store, true)?;
                Ok(Some(obj))
            }
            Encoding::FitsClass => {
                let trans = spec_trans::spec_trans(&self.cards, Encoding::FitsClass);
                let store = enc_class::read_from_cards(&trans)?;
                let obj = wcs_read::decode_wcs_with_def_b1950(&store, true)?;
                Ok(Some(obj))
            }
            Encoding::Dss => {
                // Dss is a separate format; SpecTrans's translations
                // are mostly inapplicable. Keep the existing reader.
                let store = enc_dss::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs_with_def_b1950(&store, true)?;
                Ok(Some(obj))
            }
        }
    }

    fn write(&mut self, obj: &AnyObject) -> AstResult<()> {
        self.warnings.clear();

        let enc = self.encoding();
        match enc {
            Encoding::Native => {
                let node = serial::any_object_to_node(obj)?;
                self.store_keyword_native(obj, &node);
                Ok(())
            }
            _ => {
                // For all FITS-WCS encodings, preserve the relative position
                // of any WCS card that already exists in the input header by
                // letting the encoder overwrite it in place. We mark every
                // card as `used = false`; the encoder's set_*() helpers flip
                // touched cards to `used = true`. After encoding, any WCS
                // card still unused is a leftover for a keyword the new
                // encoding does not emit (e.g. CDELT after a CD-matrix write)
                // and gets swept. Non-WCS cards (SIMPLE, NAXIS, COMMENT, …)
                // are preserved unconditionally.
                // Extract NAXISn dims (in WCS axis order) from the input
                // header before marking cards unused. The encoder uses these
                // for FindBasisVectors-style central-pixel selection; mirrors
                // fitschan.c::AddVersion's `dim` parameter.
                let dims = extract_naxis_dims(&self.cards);

                self.cards.mark_all_unused();

                let result = match enc {
                    Encoding::FitsWcs => {
                        wcs_write::encode_wcs(obj, &mut self.cards, self.cd_matrix, &dims)
                    }
                    Encoding::FitsIraf => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsIraf,
                        enc_iraf::write_to_cards,
                    ),
                    Encoding::FitsPc => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsPc,
                        enc_pc::write_to_cards,
                    ),
                    Encoding::FitsAips => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsAips,
                        enc_aips::write_to_cards,
                    ),
                    Encoding::FitsAipsPP => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsAipsPP,
                        enc_aips::write_to_cards_pp,
                    ),
                    Encoding::FitsClass => {
                        wcs_write::encode_wcs_per_plane(
                            obj,
                            &mut self.cards,
                            &dims,
                            Encoding::FitsClass,
                            enc_class::write_to_cards,
                        )?;
                        // Then run the SpecFrame velocity computation
                        // that needs the FrameSet directly. Mirrors
                        // fitschan.c:6843-6898.
                        if let AnyObject::FrameSet(fs) = obj {
                            enc_class::write_velocities_from_frameset(fs, &mut self.cards)?;
                            enc_class::write_azel_from_frameset(fs, &mut self.cards)?;
                        }
                        Ok(())
                    }
                    Encoding::Dss => wcs_write::encode_wcs_to_store(
                        obj,
                        &mut self.cards,
                        &dims,
                        enc_dss::write_to_cards,
                    ),
                    Encoding::Native => unreachable!(),
                };

                self.cards.sweep_unused_wcs();
                result
            }
        }
    }

    fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn config(&self) -> &ChannelConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut ChannelConfig {
        &mut self.config
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

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

/// Extract the AST native text line from a COMMENT card's text field.
///
/// The C library stores lines as: " AST    <native text>     AST"
/// where the text is padded/trimmed between the " AST " bookends.
fn extract_ast_line(comment_text: &str) -> String {
    // The comment text (after "COMMENT" keyword) looks like:
    //   " AST    Begin ZoomMap                                       AST"
    // We want to extract "   Begin ZoomMap" (trimming the leading " AST" and trailing " AST").
    let text = comment_text.trim_end();

    // Strip trailing " AST" suffix.
    let without_suffix = text.strip_suffix(" AST").unwrap_or(text);

    // Strip leading " AST" prefix (space + "AST").
    let without_prefix = without_suffix
        .strip_prefix(" AST")
        .unwrap_or(without_suffix);

    without_prefix.to_string()
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
        let fc = FitsChan::new();
        assert_eq!(fc.ncard(), 0);
        // No explicit encoding set; auto-detect on an empty chan
        // returns Native per C `astGetEncoding` (fitschan.c:12509).
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
        let fc = FitsChan::from_header(header).unwrap();
        assert_eq!(fc.ncard(), 3); // END is skipped
    }

    #[test]
    fn put_fits_and_find_fits() {
        let mut fc = FitsChan::new();
        fc.put_fits("NAXIS   =                    2 / Number of axes", false)
            .unwrap();
        fc.put_fits("CRVAL1  =     266.416666666667 / Reference value", false)
            .unwrap();
        assert_eq!(fc.ncard(), 2);

        fc.rewind();
        let found = fc.find_fits("CRVAL1", true);
        assert!(found.is_some());
        let card_str = found.unwrap();
        assert!(card_str.contains("266.41666666666"));
    }

    #[test]
    fn set_fits_f_and_get_fits_f() {
        let mut fc = FitsChan::new();
        fc.set_fits_f("CRVAL1", 123.456, "Test value", false);
        let val = fc.get_fits_f("CRVAL1").unwrap();
        assert!((val - 123.456).abs() < 1e-10);
    }

    #[test]
    fn set_fits_i_and_get_fits_i() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("NAXIS", 3, "Number of axes", false);
        assert_eq!(fc.get_fits_i("NAXIS"), Some(3));
    }

    #[test]
    fn set_fits_s_and_get_fits_s() {
        let mut fc = FitsChan::new();
        fc.set_fits_s("CTYPE1", "RA---TAN", "Axis type", false);
        assert_eq!(fc.get_fits_s("CTYPE1"), Some("RA---TAN".to_string()));
    }

    #[test]
    fn set_fits_l_and_get_fits_l() {
        let mut fc = FitsChan::new();
        fc.set_fits_l("SIMPLE", true, "Standard FITS", false);
        assert_eq!(fc.get_fits_l("SIMPLE"), Some(true));
    }

    #[test]
    fn ncard_tracks_insertions() {
        let mut fc = FitsChan::new();
        assert_eq!(fc.ncard(), 0);
        fc.set_fits_i("A", 1, "", false);
        assert_eq!(fc.ncard(), 1);
        fc.set_fits_i("B", 2, "", false);
        assert_eq!(fc.ncard(), 2);
    }

    #[test]
    fn empty_fits_clears_all() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("NAXIS", 2, "", false);
        fc.set_fits_i("NAXIS1", 100, "", false);
        assert_eq!(fc.ncard(), 2);
        fc.empty_fits();
        assert_eq!(fc.ncard(), 0);
    }

    #[test]
    fn overwrite_replaces_existing() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("NAXIS", 2, "Original", false);
        fc.set_fits_i("NAXIS", 3, "Updated", true);
        assert_eq!(fc.ncard(), 1); // still only one card
        assert_eq!(fc.get_fits_i("NAXIS"), Some(3));
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
        // Clamped to valid range.
        fc.set_fits_digits(0);
        assert_eq!(fc.fits_digits(), 1);
        fc.set_fits_digits(20);
        assert_eq!(fc.fits_digits(), 17);
    }

    #[test]
    fn del_fits_removes_current() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("A", 1, "", false);
        fc.set_fits_i("B", 2, "", false);
        fc.set_fits_i("C", 3, "", false);
        fc.rewind();
        // Position at first card, delete it.
        fc.del_fits().unwrap();
        assert_eq!(fc.ncard(), 2);
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
        assert!(fc.ncard() > 0);

        // Read it back.
        fc.rewind();
        let result = fc.read().expect("read should succeed");
        assert!(result.is_some());
        let read_obj = result.unwrap();
        let mapping = read_obj.as_mapping().expect("should be a Mapping");
        assert_eq!(mapping.nin(), 2);
    }

    #[test]
    fn extract_ast_line_works() {
        let input = " AST   Begin ZoomMap                                             AST";
        let line = extract_ast_line(input);
        assert!(line.contains("Begin ZoomMap"));
    }

    #[test]
    fn set_fits_u_stores_undef() {
        let mut fc = FitsChan::new();
        fc.set_fits_u("BLANK", "Undefined", false);
        let card = fc.cards.find("BLANK").unwrap();
        assert_eq!(card.value, FitsValue::Undef);
    }

    #[test]
    fn set_fits_cm_stores_comment() {
        let mut fc = FitsChan::new();
        fc.set_fits_cm("This is a test", false);
        assert_eq!(fc.ncard(), 1);
        let card = fc.cards.find("COMMENT").unwrap();
        if let FitsValue::Comment(text) = &card.value {
            assert!(text.contains("This is a test"));
        } else {
            panic!("Expected Comment value");
        }
    }

    #[test]
    fn put_cards_multi_line() {
        let mut fc = FitsChan::new();
        let cards = "SIMPLE  =                    T / Fits standard\n\
                     NAXIS   =                    2 / Number of axes\n\
                     END";
        fc.put_cards(cards).unwrap();
        assert_eq!(fc.ncard(), 2); // END is skipped
    }

    #[test]
    fn rewind_resets_position() {
        let mut fc = FitsChan::from_header(
            "NAXIS   =                    2\nNAXIS1  =                  100\nEND",
        )
        .unwrap();
        // Advance position.
        fc.find_fits("NAXIS1", true);
        fc.rewind();
        // Should be able to find from beginning again.
        let found = fc.find_fits("NAXIS", true);
        assert!(found.is_some());
    }
}

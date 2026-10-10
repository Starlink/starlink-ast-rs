//! FitsChan — FITS header I/O for AST objects.
//!
//! A [`FitsChan`] stores a sequence of FITS header cards and provides the
//! ability to read AST objects from them (using the Native encoding stored
//! in COMMENT cards) and to write AST objects into the header.

pub mod card;
pub mod card_list;
mod card_value;
pub(crate) mod enc_aips;
pub(crate) mod enc_class;
pub(crate) mod enc_dss;
pub(crate) mod enc_iraf;
pub(crate) mod enc_native;
pub(crate) mod enc_pc;
pub(crate) mod fits_match;
pub(crate) mod fits_store;
pub mod format;
pub(crate) mod fset_to_store;
pub(crate) mod guess_encoding;
pub(crate) mod spec_trans;
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

use std::sync::Arc;

use crate::channel::{Channel, ChannelConfig};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::intramap::with_registry;
use crate::mappings::{IntraRegistry, IntraTransform};
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
/// assert_eq!(fc.ncard(), 3); // SIMPLE, NAXIS, END
/// assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(2));
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
    /// The CarLin attribute (fitschan.c default 0): when true a `-CAR`
    /// projection is read as a simple linear pixel-to-sky mapping rather
    /// than as a spherical Cartesian projection.
    car_lin: bool,
    /// The FitsTol attribute (fitschan.c default 0.1): the maximum
    /// departure from linearity, in pixels, that a Mapping may show and
    /// still be described by the linear FITS-WCS keywords.
    fits_tol: f64,
    /// The transformations IntraMaps read here can name.
    intra: IntraRegistry,
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
            intra: IntraRegistry::new(),
            fits_digits: DEFAULT_FITS_DIGITS,
            fits_rounding: 10,
            cd_matrix: false,
            sip_replace: true,
            car_lin: false,
            fits_tol: fset_to_store::FITS_TOL_DEFAULT,
        }
    }

    /// Create a FitsChan from FITS header text, with the first card current.
    ///
    /// The text is split into lines and each is parsed as a FITS card. An
    /// `END` card has no `=` in column 9, so it is stored as an ordinary
    /// commentary card, just like `HISTORY`, `COMMENT` or a blank keyword.
    /// Unparseable lines are silently ignored.
    pub fn from_header(text: &str) -> AstResult<Self> {
        let mut cards = CardList::from_header(text)?;
        cards.clear_card();
        Ok(Self {
            config: ChannelConfig::default(),
            cards,
            encoding: None,
            warnings: Vec::new(),
            intra: IntraRegistry::new(),
            fits_digits: DEFAULT_FITS_DIGITS,
            fits_rounding: 10,
            cd_matrix: false,
            sip_replace: true,
            car_lin: false,
            fits_tol: fset_to_store::FITS_TOL_DEFAULT,
        })
    }

    // -----------------------------------------------------------------------
    // The current card
    // -----------------------------------------------------------------------

    /// The `Card` attribute: the 1-based index of the current card, counting
    /// only cards not consumed by a read; one more than [`ncard`](Self::ncard)
    /// at end-of-file, and 0 when there are no cards.
    pub fn card(&mut self) -> usize {
        self.cards.get_card()
    }

    /// Set `Card`: rewind, then move forward `icard - 1` cards, stopping at
    /// end-of-file. A value below 1 makes the first card current.
    pub fn set_card(&mut self, icard: i64) {
        self.cards.set_card(icard);
    }

    /// Clear `Card`: make the first card current (C's "rewind").
    pub fn clear_card(&mut self) {
        self.cards.clear_card();
    }

    /// Whether `Card` is set, which C defines as the current card not being
    /// the first.
    pub fn test_card(&mut self) -> bool {
        self.cards.test_card()
    }

    /// The `CardName` attribute: the keyword of the current card (eight
    /// spaces for a blank keyword), or `None` at end-of-file.
    pub fn card_name(&self) -> Option<String> {
        self.cards.current().map(|c| c.c_name().to_string())
    }

    /// The `CardComm` attribute: the comment of the current card, or the text
    /// of a commentary card; empty when there is none.
    pub fn card_comm(&self) -> String {
        self.cards
            .current()
            .and_then(|c| c.c_comment())
            .unwrap_or("")
            .to_string()
    }

    /// The `CardType` attribute: the data type of the current card, or
    /// [`CardType::NoType`] at end-of-file.
    pub fn card_type(&self) -> CardType {
        self.cards
            .current()
            .map_or(CardType::NoType, |c| c.card_type())
    }

    /// The `Ncard` attribute: the number of cards, not counting those a read
    /// consumed.
    pub fn ncard(&self) -> usize {
        self.cards.ncard()
    }

    /// The `Nkey` attribute: the number of different keywords among the cards
    /// a read has not consumed.
    pub fn nkey(&self) -> usize {
        self.cards.nkey()
    }

    // -----------------------------------------------------------------------
    // Card manipulation API
    // -----------------------------------------------------------------------

    /// Store a FITS header card (C's `astPutFits`).
    ///
    /// The card is inserted in front of the current card, or, with
    /// `overwrite`, replaces it; either way the card after it becomes
    /// current. At end-of-file the card is appended. A blank comment, when
    /// overwriting, keeps the comment of the card replaced.
    pub fn put_fits(&mut self, card: &str, overwrite: bool) -> AstResult<()> {
        let parsed = FitsCard::parse(card)?;
        // Emit ASTWARN cards for an illegal keyword name, mirroring C
        // `CheckFitsName`/`Warn` inside `astPutFits` (fitschan.c:32259).
        if let Some(text) = crate::fitschan::card::check_fits_name(card) {
            self.cards.push_astwarn(&text);
        }
        match parsed.value {
            // `PutFits` hands a commentary card to `SetFitsCom`.
            FitsValue::Comment(ref text) => {
                let text = text.trim_end().to_string();
                self.set_fits_com(&parsed.keyword, &text, overwrite)?;
            }
            _ => {
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
        self.empty_fits();
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
            .format_with_digits_and_rounding(self.fits_digits, self.fits_rounding);
        if inc {
            self.cards.move_card(1);
        }
        Ok(Some(text))
    }

    /// Delete the current card, making the next card current (C's
    /// `astDelFits`). Does nothing at end-of-file.
    pub fn del_fits(&mut self) {
        self.cards.delete_card();
    }

    /// Delete every card, and the warnings recorded by reads (C's
    /// `astEmptyFits`).
    pub fn empty_fits(&mut self) {
        self.cards = CardList::new();
        self.warnings.clear();
    }

    /// Format all stored cards as a FITS header string.
    ///
    /// Each card is formatted as an 80-character line, joined with newlines.
    /// `END` cards already present in the card list (stored verbatim by
    /// `from_header`/`put_cards`) are written out in their original
    /// position, matching C `WriteToSink`, which never synthesises one on a
    /// read-then-rewrite round trip. A trailing `END` is appended only when
    /// the card list has none at all — e.g. a `FitsChan` built purely from
    /// typed setters.
    pub fn to_header(&self) -> String {
        let visible: Vec<&FitsCard> = self
            .cards
            .iter()
            .filter(|card| !card.is_consumed())
            .collect();
        let has_end = visible.iter().any(|card| card.keyword == "END");
        let mut lines: Vec<String> = visible
            .iter()
            .map(|card| card.format_with_digits_and_rounding(self.fits_digits, self.fits_rounding))
            .collect();
        if !has_end {
            lines.push(format!("{:<80}", "END"));
        }
        lines.join("\n")
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
        let lname = match name {
            Some(name) => {
                let (lname, _) = split_name(name)?;
                if let Some(text) = crate::fitschan::card::check_fits_name(name) {
                    self.cards.push_astwarn(&text);
                }
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
            self.fits_rounding,
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
        let (lname, lcom) = split_name(name)?;
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
            used: false,
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
        let (lname, _) = split_name(name)?;
        let com = if comment.trim().is_empty() {
            None
        } else {
            Some(comment.to_string())
        };
        let same = self.cards.current().is_some_and(|c| c.keyword == lname);
        let card = if !overwrite || !same {
            FitsCard {
                keyword: lname,
                value: FitsValue::Comment(com.unwrap_or_default()),
                comment: None,
                used: false,
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

    /// Set the CarLin attribute (mirrors C `CarLin`, default false): when
    /// true, a `-CAR` projection is read as a simple linear pixel-to-sky
    /// mapping rather than as a spherical Cartesian projection, and the
    /// MSX-CAR `CRPIX` wrap in `spec_trans` is suppressed.
    pub fn set_car_lin(&mut self, v: bool) {
        self.car_lin = v;
    }

    /// Return whether CarLin is enabled.
    pub fn car_lin(&self) -> bool {
        self.car_lin
    }

    /// Set the FitsTol attribute (mirrors C `FitsTol`, default 0.1): the
    /// maximum departure from linearity, in pixels, that a Mapping may
    /// show and still be described by the linear FITS-WCS keywords
    /// (CRPIX, CD, PC, CDELT), with or without SIP distortion. A Mapping
    /// that departs further has no FITS-WCS representation and
    /// [`Channel::write`] reports that it wrote nothing.
    pub fn set_fits_tol(&mut self, tol: f64) {
        self.fits_tol = tol;
    }

    /// Return the current FitsTol value.
    pub fn fits_tol(&self) -> f64 {
        self.fits_tol
    }

    /// Return whether CD matrix notation is enabled.
    pub fn cd_matrix(&self) -> bool {
        self.cd_matrix
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
        let comm = self.config.comment();
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

        self.cards.insert_at_current(block);
    }

    /// Flag the cards a completed FITS-WCS-family read consumed, then promote
    /// them to `Used` so they are excluded from later output. Mirrors C's
    /// read-completion `FixUsed(promote)` (fitschan.c:16431) over the two
    /// sources of consumption:
    ///
    ///  * [`wcs_mark::mark_wcs_cards`] sweeps every card, as C `WcsFcRead`
    ///    (fitschan.c:36233) does — so every copy of a repeated WCS keyword is
    ///    consumed, not merely the first.
    ///  * names recorded on the SpecTrans-derived `trans` list (by the
    ///    `*_marked` getters and `record_consumed`) mark *every* matching card,
    ///    as C `GetValue2` → `MarkAllCards` (fitschan.c:23105) does. The value
    ///    a translation used is still the first match; the marking is what has
    ///    to cover them all. Names with no matching card — keywords SpecTrans
    ///    synthesised — are silently ignored.
    fn finish_read_consumption(&mut self, trans: &CardList) -> AstResult<()> {
        wcs_mark::mark_wcs_cards(&mut self.cards)?;
        for name in trans.take_consumed_names() {
            self.cards.mark_all_cards(&name);
        }
        self.cards.fix_used(true, false);
        Ok(())
    }

    /// Store the `ASTWARN` blocks a read produced, each against the card
    /// cursor C holds when its `Warn` call is reached.
    ///
    /// The two groups land in different places. `SpecTrans` warns while the
    /// cursor is at end-of-file — its keyword searches walk off the end of the
    /// list and are not rewound (e.g. the `QV` scan preceding `GetUsedPolyTan`,
    /// fitschan.c:31849-31898) — and C `InsCard` appends when there is no
    /// current card (fitschan.c:18796-18807), so those blocks go after the last
    /// card. `WcsToStore`'s warnings are written against `NAXIS<naxes>`, the
    /// cursor `move_cursor_to_last_naxis` reproduces, so those go in front of
    /// it. `SpecTrans` runs to completion before `WcsToStore` starts
    /// (fitschan.c:11469-11473), which is the order they are stored in.
    fn push_read_warnings(
        &mut self,
        trans_warnings: Vec<String>,
        read_warnings: Vec<String>,
        naxes: usize,
    ) {
        // Each warning goes two places, as in C: `Warn` (fitschan.c:35052)
        // calls `astAddWarning`, which appends the text to the parent
        // Channel's warning list (channel.c:151) for `astWarnings` to report,
        // and separately inserts the ASTWARN card. Recording only the card
        // would leave `warning_texts` / `warnings` permanently empty.
        if !trans_warnings.is_empty() {
            self.cards.set_position(self.cards.len());
            for w in trans_warnings {
                self.cards.push_astwarn(&w);
                self.warnings.push(w);
            }
        }
        move_cursor_to_last_naxis(&mut self.cards, naxes);
        for w in read_warnings {
            self.cards.push_astwarn(&w);
            self.warnings.push(w);
        }
    }
}

/// The keyword and comment C's `Split` finds when it reads a setter's or
/// getter's keyword name as a card: the first 8 characters, in their own
/// case (`NewCard` upper-cases them when a card is stored), and any text
/// after them.
fn split_name(name: &str) -> AstResult<(String, String)> {
    let card = FitsCard::parse(name)?;
    let comment = match &card.value {
        FitsValue::Comment(text) => text.trim().to_string(),
        _ => card.comment.clone().unwrap_or_default(),
    };
    let raw: String = name.chars().take(8).collect();
    let keyword = raw.trim_end().to_string();
    debug_assert!(keyword.eq_ignore_ascii_case(&card.keyword));
    Ok((keyword, comment))
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
        // The encodings other than NATIVE hold one FrameSet, read only when
        // the first card is current; a successful read leaves the FitsChan at
        // end-of-file and any other rewinds it (fitschan.c `Read`).
        if enc != Encoding::Native && self.cards.test_card() {
            return Ok(None);
        }
        let mut read_encoded = || -> AstResult<Option<AnyObject>> {
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
                    let obj = with_registry(&self.intra, || serial::any_object_from_node(&node))?;

                    // C `Read` calls `FixUsed( this, 0, 1, remove )` with
                    // `remove` set for NATIVE encoding (fitschan.c:25811-25817),
                    // so the cards a successful native read consumed are deleted
                    // rather than merely flagged.
                    if let Some((first, last)) = enc_native::native_span(&self.cards) {
                        for i in first..=last {
                            self.cards.mark_at(i);
                        }
                        self.cards.mark_com_block(first, -1);
                        self.cards.mark_com_block(last, 1);
                        self.cards.fix_used(true, true);
                    }

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
                    // C's `Warn` calls inside `SpecTrans` target the source chan
                    // rather than the translation chan, so their text comes back
                    // out to be stored on `self.cards`.
                    let mut trans_warnings: Vec<String> = Vec::new();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        Encoding::FitsWcs,
                        self.car_lin,
                        &mut trans_warnings,
                    );
                    let (mut primary, mut alts) =
                        wcs_keywords::extract_wcs_keywords_with_alts(&trans)?;
                    primary.sip_replace = self.sip_replace;
                    primary.car_lin = self.car_lin;
                    for (_, st) in &mut alts {
                        st.sip_replace = self.sip_replace;
                        st.car_lin = self.car_lin;
                    }
                    let naxes = primary.naxis;
                    let (obj, warnings) =
                        wcs_read::decode_wcs_with_alts_inner(&primary, &alts, false, &self.cards)?;
                    self.push_read_warnings(trans_warnings, warnings, naxes);
                    self.finish_read_consumption(&trans)?;
                    Ok(Some(obj))
                }
                Encoding::FitsPc => {
                    // Run SpecTrans first to normalise non-standard
                    // keywords (Cnxxx alt-axis form, CDjjjiii, CROTA,
                    // PROJP, etc.) into their canonical equivalents.
                    // After SpecTrans, alt-axis cards are in the *A..*Z
                    // form, so we route through the multi-plane
                    // FitsWcs decoder to pick up secondary planes.
                    let mut trans_warnings: Vec<String> = Vec::new();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        Encoding::FitsPc,
                        self.car_lin,
                        &mut trans_warnings,
                    );
                    let (mut primary, mut alts) =
                        wcs_keywords::extract_wcs_keywords_with_alts(&trans)?;
                    primary.sip_replace = self.sip_replace;
                    primary.car_lin = self.car_lin;
                    for (_, st) in &mut alts {
                        st.sip_replace = self.sip_replace;
                        st.car_lin = self.car_lin;
                    }
                    let naxes = primary.naxis;
                    let (obj, warnings) =
                        wcs_read::decode_wcs_with_alts_inner(&primary, &alts, true, &self.cards)?;
                    self.push_read_warnings(trans_warnings, warnings, naxes);
                    self.finish_read_consumption(&trans)?;
                    Ok(Some(obj))
                }
                Encoding::FitsIraf => {
                    let mut trans_warnings: Vec<String> = Vec::new();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        Encoding::FitsIraf,
                        self.car_lin,
                        &mut trans_warnings,
                    );
                    let (mut primary, mut alts) =
                        wcs_keywords::extract_wcs_keywords_with_alts(&trans)?;
                    primary.sip_replace = self.sip_replace;
                    primary.car_lin = self.car_lin;
                    for (_, st) in &mut alts {
                        st.sip_replace = self.sip_replace;
                        st.car_lin = self.car_lin;
                    }
                    let naxes = primary.naxis;
                    let (obj, warnings) =
                        wcs_read::decode_wcs_with_alts_inner(&primary, &alts, true, &self.cards)?;
                    self.push_read_warnings(trans_warnings, warnings, naxes);
                    self.finish_read_consumption(&trans)?;
                    Ok(Some(obj))
                }
                Encoding::FitsAips | Encoding::FitsAipsPP => {
                    let mut trans_warnings: Vec<String> = Vec::new();
                    let trans =
                        spec_trans::spec_trans(&self.cards, enc, self.car_lin, &mut trans_warnings);
                    let store = enc_aips::read_from_cards(&trans)?;
                    let naxes = store.naxis;
                    let (obj, warnings) =
                        wcs_read::decode_wcs_inner(&store, true, &self.cards, ' ')?;
                    self.push_read_warnings(trans_warnings, warnings, naxes);
                    self.finish_read_consumption(&trans)?;
                    Ok(Some(obj))
                }
                Encoding::FitsClass => {
                    let mut trans_warnings: Vec<String> = Vec::new();
                    let trans = spec_trans::spec_trans(
                        &self.cards,
                        Encoding::FitsClass,
                        self.car_lin,
                        &mut trans_warnings,
                    );
                    let store = enc_class::read_from_cards(&trans)?;
                    let naxes = store.naxis;
                    let (obj, warnings) =
                        wcs_read::decode_wcs_inner(&store, true, &self.cards, ' ')?;
                    self.push_read_warnings(trans_warnings, warnings, naxes);
                    self.finish_read_consumption(&trans)?;
                    Ok(Some(obj))
                }
                Encoding::Dss => {
                    // Dss is a separate format; SpecTrans's translations
                    // are mostly inapplicable. Keep the existing reader. The
                    // reader records consumed names directly on `self.cards`
                    // (no SpecTrans copy), so promote them in place.
                    let store = enc_dss::read_from_cards(&self.cards)?;
                    let naxes = store.naxis;
                    let (obj, warnings) =
                        wcs_read::decode_wcs_inner(&store, true, &self.cards, ' ')?;
                    self.push_read_warnings(Vec::new(), warnings, naxes);
                    for name in self.cards.take_consumed_names() {
                        self.cards.mark_card(&name);
                    }
                    self.cards.fix_used(true, false);
                    Ok(Some(obj))
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

        // Remember the entry card so a failed write can put it back, and
        // start flagging every card this call adds so a failed write can
        // remove them again. Mirrors C `Write` (fitschan.c:40621-40625).
        let card0 = self.cards.position();
        self.cards.begin_marking_new();
        let result = self.write_inner(obj);

        // A write that failed, for either reason, leaves the header exactly
        // as it was found: the cards it had added are removed and the entry
        // card restored. Mirrors fitschan.c:40745-40761.
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
                self.store_keyword_native(obj, &node);
                // The native encoding can represent every AST class.
                // Mirrors fitschan.c:40689.
                Ok(1)
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
                    Encoding::FitsWcs => wcs_write::encode_wcs(
                        obj,
                        &mut self.cards,
                        self.cd_matrix,
                        &dims,
                        self.fits_tol,
                    ),
                    Encoding::FitsIraf => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsIraf,
                        self.fits_tol,
                        enc_iraf::write_to_cards,
                    ),
                    Encoding::FitsPc => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsPc,
                        self.fits_tol,
                        enc_pc::write_to_cards,
                    ),
                    Encoding::FitsAips => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsAips,
                        self.fits_tol,
                        enc_aips::write_to_cards,
                    ),
                    Encoding::FitsAipsPP => wcs_write::encode_wcs_per_plane(
                        obj,
                        &mut self.cards,
                        &dims,
                        Encoding::FitsAipsPP,
                        self.fits_tol,
                        enc_aips::write_to_cards_pp,
                    ),
                    Encoding::FitsClass => {
                        let ok = wcs_write::encode_wcs_per_plane(
                            obj,
                            &mut self.cards,
                            &dims,
                            Encoding::FitsClass,
                            self.fits_tol,
                            enc_class::write_to_cards,
                        )?;
                        // Then run the SpecFrame velocity computation
                        // that needs the FrameSet directly. Mirrors
                        // fitschan.c:6843-6898.
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
                        self.fits_tol,
                        enc_dss::write_to_cards,
                    ),
                    Encoding::Native => unreachable!(),
                };

                self.cards.sweep_unused_wcs();
                let ret = usize::from(result?);
                // A successful write leaves the current card at end-of-file,
                // so a subsequent write appends after this object. Mirrors
                // fitschan.c:40732.
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

/// Look up `NAXIS1`…`NAXIS<naxes>` on `cards`, leaving the cursor on the last
/// one found. Mirrors C `DistortMaps`'s opening loop (fitschan.c:8809-8813):
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
/// original card index. The cursor it leaves behind — `NAXIS<naxes>` — is
/// where C `Warn` then inserts any `ASTWARN` block.
///
/// C reads these off the FitsChan itself rather than the `SpecTrans` copy, so
/// the cursor this moves is the one the warning cards are written against.
fn move_cursor_to_last_naxis(cards: &mut CardList, naxes: usize) {
    for i in 1..=naxes {
        let _ = cards.get_float_at_cursor(&format!("NAXIS{i}"));
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
        assert_eq!(fc.ncard(), 4); // END is stored as a commentary card
    }

    #[test]
    fn put_fits_and_find_fits() {
        let mut fc = FitsChan::new();
        fc.put_fits("NAXIS   =                    2 / Number of axes", false)
            .unwrap();
        fc.put_fits("CRVAL1  =     266.416666666667 / Reference value", false)
            .unwrap();
        assert_eq!(fc.ncard(), 2);

        fc.clear_card();
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
        assert_eq!(fc.ncard(), 0);
        fc.set_fits_i("A", 1, "", false).unwrap();
        assert_eq!(fc.ncard(), 1);
        fc.set_fits_i("B", 2, "", false).unwrap();
        assert_eq!(fc.ncard(), 2);
    }

    #[test]
    fn empty_fits_clears_all() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("NAXIS", 2, "", false).unwrap();
        fc.set_fits_i("NAXIS1", 100, "", false).unwrap();
        assert_eq!(fc.ncard(), 2);
        fc.empty_fits();
        assert_eq!(fc.ncard(), 0);
    }

    #[test]
    fn overwrite_replaces_existing() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("NAXIS", 2, "Original", false).unwrap();
        // Overwriting replaces the current card.
        fc.clear_card();
        fc.set_fits_i("NAXIS", 3, "Updated", true).unwrap();
        assert_eq!(fc.ncard(), 1); // still only one card
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
        // Clamped to valid range.
        fc.set_fits_digits(0);
        assert_eq!(fc.fits_digits(), 1);
        fc.set_fits_digits(20);
        assert_eq!(fc.fits_digits(), 17);
    }

    #[test]
    fn del_fits_removes_current() {
        let mut fc = FitsChan::new();
        fc.set_fits_i("A", 1, "", false).unwrap();
        fc.set_fits_i("B", 2, "", false).unwrap();
        fc.set_fits_i("C", 3, "", false).unwrap();
        fc.clear_card();
        // Position at first card, delete it.
        fc.del_fits();
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
        fc.clear_card();
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
        fc.set_fits_u("BLANK", "Undefined", false).unwrap();
        let card = fc.cards.find("BLANK").unwrap();
        assert_eq!(card.value, FitsValue::Undef);
    }

    #[test]
    fn set_fits_cm_stores_comment() {
        let mut fc = FitsChan::new();
        fc.set_fits_cm("This is a test", false).unwrap();
        assert_eq!(fc.ncard(), 1);
        // C's astSetFitsCM stores a commentary card with a blank keyword.
        fc.clear_card();
        assert_eq!(fc.card_type(), CardType::Comment);
        assert_eq!(fc.card_name().as_deref(), Some("        "));
        assert_eq!(fc.card_comm(), "This is a test");
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
        assert_eq!(fc.ncard(), 3); // END is stored as a commentary card
        assert_eq!(fc.card(), 1);
    }

    #[test]
    fn rewind_resets_position() {
        let mut fc = FitsChan::from_header(
            "NAXIS   =                    2\nNAXIS1  =                  100\nEND",
        )
        .unwrap();
        // Advance position.
        fc.find_fits("NAXIS1", true).unwrap();
        fc.clear_card();
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
        let out = fc.to_header();
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
        let out = fc.to_header();
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
        let out = fc.to_header();
        assert!(
            out.contains("CRVAL1"),
            "protected CRVAL1 should survive the read:\n{out}"
        );
        // A sibling unprotected WCS card is still consumed.
        assert!(!out.contains("CRVAL2"), "CRVAL2 should be consumed:\n{out}");
    }
}

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
pub(crate) mod enc_pc;
pub(crate) mod fits_store;
pub mod format;
pub(crate) mod guess_encoding;
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
const DEFAULT_FITS_DIGITS: usize = 8;

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
    encoding: Encoding,
    warnings: Vec<String>,
    fits_digits: usize,
    /// Whether to use CD matrix notation (default false).
    cd_matrix: bool,
}

impl FitsChan {
    /// Create an empty FitsChan with default encoding (FitsWcs).
    pub fn new() -> Self {
        Self {
            config: ChannelConfig::default(),
            cards: CardList::new(),
            encoding: Encoding::FitsWcs,
            warnings: Vec::new(),
            fits_digits: DEFAULT_FITS_DIGITS,
            cd_matrix: false,
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
            encoding: Encoding::FitsWcs,
            warnings: Vec::new(),
            fits_digits: DEFAULT_FITS_DIGITS,
            cd_matrix: false,
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
        };
        self.cards.put(card, overwrite);
    }

    // -----------------------------------------------------------------------
    // Attribute accessors
    // -----------------------------------------------------------------------

    /// Set the encoding format.
    pub fn set_encoding(&mut self, enc: Encoding) {
        self.encoding = enc;
    }

    /// Return the current encoding format.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Auto-detect the encoding format from the current header cards.
    ///
    /// Examines keyword signatures to determine which WCS encoding the
    /// header uses. Returns the detected encoding and updates the internal
    /// encoding attribute.
    pub fn guess_encoding(&mut self) -> Encoding {
        let enc = guess_encoding::guess_encoding(&self.cards);
        self.encoding = enc;
        enc
    }

    /// Set the number of significant digits for floating-point output.
    pub fn set_fits_digits(&mut self, digits: usize) {
        self.fits_digits = digits.clamp(1, 17);
    }

    /// Return the current number of significant digits for floating-point output.
    pub fn fits_digits(&self) -> usize {
        self.fits_digits
    }

    /// Reset the card position to the beginning.
    pub fn rewind(&mut self) {
        self.cards.rewind();
    }

    // -----------------------------------------------------------------------
    // Native encoding helpers
    // -----------------------------------------------------------------------

    /// Marker line for beginning of AST native data in COMMENT cards.
    const AST_BEGIN_MARKER: &'static str = " AST +++++++++++++++++++++++++++++++++ AST";

    /// Marker line for end of AST native data in COMMENT cards.
    const AST_END_MARKER: &'static str = " AST --------------------------------- AST";

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

    /// Store AST native text as COMMENT cards with markers.
    fn store_native_text(&mut self, text: &str) {
        // Begin marker.
        let begin_card = FitsCard {
            keyword: "COMMENT".to_string(),
            value: FitsValue::Comment(Self::AST_BEGIN_MARKER.to_string()),
            comment: None,
            used: false,
        };
        self.cards.push(begin_card);

        // Each line of native text becomes a COMMENT card.
        for line in text.lines() {
            let padded = format_ast_comment_line(line);
            let card = FitsCard {
                keyword: "COMMENT".to_string(),
                value: FitsValue::Comment(padded),
                comment: None,
                used: false,
            };
            self.cards.push(card);
        }

        // End marker.
        let end_card = FitsCard {
            keyword: "COMMENT".to_string(),
            value: FitsValue::Comment(Self::AST_END_MARKER.to_string()),
            comment: None,
            used: false,
        };
        self.cards.push(end_card);
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

        match self.encoding {
            Encoding::Native => {
                let text = match self.extract_native_text() {
                    Some(t) => t,
                    None => return Ok(None),
                };
                let node = serial::native_read::parse_native(&text)?;
                let obj = serial::any_object_from_node(&node)?;
                Ok(Some(obj))
            }
            Encoding::FitsWcs => {
                let store = wcs_keywords::extract_wcs_keywords(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::FitsPc => {
                let store = enc_pc::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::FitsIraf => {
                let store = enc_iraf::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::FitsAips | Encoding::FitsAipsPP => {
                let store = enc_aips::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::FitsClass => {
                let store = enc_class::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::Dss => {
                let store = enc_dss::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
        }
    }

    fn write(&mut self, obj: &AnyObject) -> AstResult<()> {
        self.warnings.clear();

        match self.encoding {
            Encoding::Native => {
                let node = serial::any_object_to_node(obj)?;
                let options = serial::native_write::RenderOptions {
                    comment: self.config.comment,
                    full: self.config.full,
                    indent: self.config.indent,
                };
                let text = serial::native_write::render_native_with_options(&node, &options)?;
                self.store_native_text(&text);
                Ok(())
            }
            Encoding::FitsWcs => wcs_write::encode_wcs(obj, &mut self.cards, self.cd_matrix),
            Encoding::FitsIraf => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, enc_iraf::write_to_cards)
            }
            Encoding::FitsPc => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, enc_pc::write_to_cards)
            }
            Encoding::FitsAips => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, enc_aips::write_to_cards)
            }
            Encoding::FitsAipsPP => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, enc_aips::write_to_cards_pp)
            }
            Encoding::FitsClass => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, enc_class::write_to_cards)
            }
            Encoding::Dss => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, enc_dss::write_to_cards)
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

/// Format a native text line as a COMMENT card value.
///
/// Produces: " AST <line padded to fill 70 chars> AST"
fn format_ast_comment_line(line: &str) -> String {
    // COMMENT card text area is 72 chars (80 - 8 for "COMMENT ").
    // Format: " AST" + content (up to 64 chars) + " AST"
    // Total: 4 + content + 4 = 72 max.
    let max_content = 64;
    let content = if line.len() > max_content {
        &line[..max_content]
    } else {
        line
    };

    // Pad content to max_content.
    let padded = format!("{content:<max_content$}");
    format!(" AST{padded} AST")
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
        assert_eq!(fc.encoding(), Encoding::FitsWcs);
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
    fn format_ast_comment_line_works() {
        let line = "   Begin ZoomMap";
        let formatted = format_ast_comment_line(line);
        assert!(formatted.starts_with(" AST"));
        assert!(formatted.ends_with(" AST"));
        assert!(formatted.contains("Begin ZoomMap"));
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

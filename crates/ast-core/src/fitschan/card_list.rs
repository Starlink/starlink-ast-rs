//! Ordered collection of FITS header cards with position tracking.

use super::card::{FitsCard, FitsValue};
use crate::error::AstResult;

/// An ordered collection of [`FitsCard`]s with position-based iteration.
///
/// CardList maintains a current position pointer that advances during
/// search operations, mimicking the C library's "current card" semantics.
#[derive(Debug, Clone)]
pub struct CardList {
    cards: Vec<FitsCard>,
    position: usize,
}

impl CardList {
    /// Create an empty card list.
    pub fn new() -> Self {
        Self {
            cards: Vec::new(),
            position: 0,
        }
    }

    /// Return the number of cards in the list.
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// Return true if the list contains no cards.
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// Append a card at the end of the list.
    pub fn push(&mut self, card: FitsCard) {
        self.cards.push(card);
    }

    /// Insert or replace a card.
    ///
    /// If `overwrite` is true and a card with the same keyword already exists,
    /// the first matching card is replaced. Otherwise, the card is inserted
    /// at the current position.
    pub fn put(&mut self, card: FitsCard, overwrite: bool) {
        if overwrite {
            // Search for existing card with same keyword.
            let kw_upper = card.keyword.to_uppercase();
            if let Some(idx) = self
                .cards
                .iter()
                .position(|c| c.keyword.eq_ignore_ascii_case(&kw_upper))
            {
                self.cards[idx] = card;
                return;
            }
        }
        // Insert at current position.
        let pos = self.position.min(self.cards.len());
        self.cards.insert(pos, card);
    }

    /// Find the first card matching the given keyword (case-insensitive).
    pub fn find(&self, keyword: &str) -> Option<&FitsCard> {
        self.cards
            .iter()
            .find(|c| c.keyword.eq_ignore_ascii_case(keyword))
    }

    /// Search forward from the current position for a card matching keyword.
    ///
    /// If found, advances position past the result and returns a reference.
    /// If not found, position is unchanged.
    pub fn find_next(&mut self, keyword: &str) -> Option<&FitsCard> {
        let start = self.position;
        for i in start..self.cards.len() {
            if self.cards[i].keyword.eq_ignore_ascii_case(keyword) {
                self.position = i + 1;
                return Some(&self.cards[i]);
            }
        }
        None
    }

    /// Get the integer value of the first card matching keyword.
    pub fn get_int(&self, keyword: &str) -> Option<i64> {
        self.find(keyword).and_then(|c| match &c.value {
            FitsValue::Int(v) => Some(*v),
            FitsValue::Float(v) => Some(*v as i64),
            _ => None,
        })
    }

    /// Get the floating-point value of the first card matching keyword.
    pub fn get_float(&self, keyword: &str) -> Option<f64> {
        self.find(keyword).and_then(|c| match &c.value {
            FitsValue::Float(v) => Some(*v),
            FitsValue::Int(v) => Some(*v as f64),
            _ => None,
        })
    }

    /// Get the string value of the first card matching keyword.
    pub fn get_string(&self, keyword: &str) -> Option<String> {
        self.find(keyword).and_then(|c| match &c.value {
            FitsValue::String(s) => Some(s.clone()),
            _ => None,
        })
    }

    /// Get the logical value of the first card matching keyword.
    pub fn get_logical(&self, keyword: &str) -> Option<bool> {
        self.find(keyword).and_then(|c| match &c.value {
            FitsValue::Logical(v) => Some(*v),
            _ => None,
        })
    }

    /// Delete the first card matching keyword. Returns true if a card was deleted.
    pub fn delete(&mut self, keyword: &str) -> bool {
        if let Some(idx) = self
            .cards
            .iter()
            .position(|c| c.keyword.eq_ignore_ascii_case(keyword))
        {
            self.cards.remove(idx);
            // Adjust position if needed.
            if self.position > idx && self.position > 0 {
                self.position -= 1;
            }
            true
        } else {
            false
        }
    }

    /// Delete the card at the current position.
    ///
    /// If the position is past the end, this is a no-op.
    pub fn delete_at_position(&mut self) {
        if self.position < self.cards.len() {
            self.cards.remove(self.position);
        }
    }

    /// Reset the position to the beginning of the list.
    pub fn rewind(&mut self) {
        self.position = 0;
    }

    /// Set the current position.
    pub fn set_position(&mut self, pos: usize) {
        self.position = pos;
    }

    /// Return the current position.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Return true if the position is at or past the end of the list.
    pub fn at_eof(&self) -> bool {
        self.position >= self.cards.len()
    }

    /// Set an integer keyword value (insert or overwrite existing).
    pub(crate) fn set_int(&mut self, keyword: &str, value: i64, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::Int(value),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
        };
        self.put(card, true);
    }

    /// Set a float keyword value (insert or overwrite existing).
    pub(crate) fn set_float(&mut self, keyword: &str, value: f64, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::Float(value),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
        };
        self.put(card, true);
    }

    /// Set a string keyword value (insert or overwrite existing).
    pub(crate) fn set_string(&mut self, keyword: &str, value: &str, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::String(value.to_string()),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
        };
        self.put(card, true);
    }

    /// Return an iterator over all cards.
    pub fn iter(&self) -> impl Iterator<Item = &FitsCard> {
        self.cards.iter()
    }

    /// Parse multi-line FITS header text into a CardList.
    ///
    /// Each line is parsed as a FITS card. Lines that fail to parse are
    /// skipped with a warning (not an error), following the C library's
    /// permissive behaviour.
    pub fn from_header(text: &str) -> AstResult<Self> {
        let mut list = Self::new();
        for line in text.lines() {
            // Skip completely empty lines.
            if line.trim().is_empty() {
                continue;
            }
            match FitsCard::parse(line) {
                Ok(card) => {
                    // Skip END cards — they're structural markers, not data.
                    if card.keyword == "END" {
                        continue;
                    }
                    list.push(card);
                }
                Err(_) => {
                    // Permissive: skip unparseable lines.
                    continue;
                }
            }
        }
        Ok(list)
    }
}

impl Default for CardList {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> &'static str {
        "SIMPLE  =                    T / Fits standard\n\
         BITPIX  =                  -32 / Bits per pixel\n\
         NAXIS   =                    2 / Number of axes\n\
         NAXIS1  =                  100 / Length of axis 1\n\
         NAXIS2  =                  200 / Length of axis 2\n\
         CRVAL1  =     266.416666666667 / Reference value\n\
         CTYPE1  = 'RA---TAN'           / Axis type\n\
         END"
    }

    #[test]
    fn from_header_parses_cards() {
        let list = CardList::from_header(sample_header()).unwrap();
        // END is skipped, so we should have 7 cards.
        assert_eq!(list.len(), 7);
        assert!(!list.is_empty());
    }

    #[test]
    fn find_locates_keyword_case_insensitive() {
        let list = CardList::from_header(sample_header()).unwrap();
        let card = list.find("naxis").unwrap();
        assert_eq!(card.keyword, "NAXIS");
        assert_eq!(card.value, FitsValue::Int(2));

        let card = list.find("CTYPE1").unwrap();
        assert_eq!(card.value, FitsValue::String("RA---TAN".into()));
    }

    #[test]
    fn find_returns_none_for_missing() {
        let list = CardList::from_header(sample_header()).unwrap();
        assert!(list.find("MISSING").is_none());
    }

    #[test]
    fn put_with_overwrite_replaces() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        let new_card = FitsCard {
            keyword: "NAXIS".to_string(),
            value: FitsValue::Int(3),
            comment: Some("Changed".to_string()),
            used: false,
        };
        list.put(new_card, true);
        let card = list.find("NAXIS").unwrap();
        assert_eq!(card.value, FitsValue::Int(3));
        assert_eq!(card.comment, Some("Changed".to_string()));
    }

    #[test]
    fn put_without_overwrite_inserts() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        let original_len = list.len();
        list.set_position(2);
        let new_card = FitsCard {
            keyword: "NEWKEY".to_string(),
            value: FitsValue::Int(99),
            comment: None,
            used: false,
        };
        list.put(new_card, false);
        assert_eq!(list.len(), original_len + 1);
        // Inserted at position 2.
        assert_eq!(list.cards[2].keyword, "NEWKEY");
    }

    #[test]
    fn delete_removes_card() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        let original_len = list.len();
        assert!(list.delete("NAXIS1"));
        assert_eq!(list.len(), original_len - 1);
        assert!(list.find("NAXIS1").is_none());
    }

    #[test]
    fn delete_returns_false_for_missing() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        assert!(!list.delete("MISSING"));
    }

    #[test]
    fn get_int_extracts_value() {
        let list = CardList::from_header(sample_header()).unwrap();
        assert_eq!(list.get_int("NAXIS"), Some(2));
        assert_eq!(list.get_int("NAXIS1"), Some(100));
        assert_eq!(list.get_int("MISSING"), None);
    }

    #[test]
    fn get_float_extracts_value() {
        let list = CardList::from_header(sample_header()).unwrap();
        let crval = list.get_float("CRVAL1").unwrap();
        assert!((crval - 266.416666666667).abs() < 1e-10);
        // Integer card should also work via conversion.
        assert_eq!(list.get_float("NAXIS"), Some(2.0));
    }

    #[test]
    fn get_string_extracts_value() {
        let list = CardList::from_header(sample_header()).unwrap();
        assert_eq!(list.get_string("CTYPE1"), Some("RA---TAN".to_string()));
        assert_eq!(list.get_string("NAXIS"), None); // not a string
    }

    #[test]
    fn get_logical_extracts_value() {
        let list = CardList::from_header(sample_header()).unwrap();
        assert_eq!(list.get_logical("SIMPLE"), Some(true));
        assert_eq!(list.get_logical("NAXIS"), None); // not a logical
    }

    #[test]
    fn find_next_advances_position() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        list.rewind();
        // Find NAXIS (should be at index 2).
        let card = list.find_next("NAXIS").unwrap();
        assert_eq!(card.value, FitsValue::Int(2));
        // Position should be past NAXIS (index 3).
        assert_eq!(list.position(), 3);
        // find_next again from current position should not find plain NAXIS.
        assert!(list.find_next("NAXIS").is_none());
    }

    #[test]
    fn rewind_resets_position() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        list.set_position(5);
        assert_eq!(list.position(), 5);
        list.rewind();
        assert_eq!(list.position(), 0);
    }

    #[test]
    fn at_eof() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        assert!(!list.at_eof());
        list.set_position(list.len());
        assert!(list.at_eof());
    }

    #[test]
    fn delete_at_position() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        let original_len = list.len();
        // Position 0 is SIMPLE.
        list.set_position(0);
        list.delete_at_position();
        assert_eq!(list.len(), original_len - 1);
        // First card should now be BITPIX.
        assert_eq!(list.cards[0].keyword, "BITPIX");
    }

    #[test]
    fn iter_yields_all_cards() {
        let list = CardList::from_header(sample_header()).unwrap();
        let keywords: Vec<&str> = list.iter().map(|c| c.keyword.as_str()).collect();
        assert_eq!(
            keywords,
            vec![
                "SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "CRVAL1", "CTYPE1"
            ]
        );
    }

    #[test]
    fn from_header_skips_empty_lines() {
        let text = "NAXIS   =                    2 / Number of axes\n\n\nEND\n";
        let list = CardList::from_header(text).unwrap();
        assert_eq!(list.len(), 1);
    }
}

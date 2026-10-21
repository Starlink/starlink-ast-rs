//! Key ordering for a `KeyMap`.
//!
//! Ported from `CompareEntries`, `SortByInt` and `SortByString` in
//! `reference/starlink-ast/src/keymap.c`.

use std::cmp::Ordering;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::keymap::entry::MapEntry;
use crate::keymap::table;

/// The orders in which [`KeyMap::map_key`](crate::keymap::KeyMap::map_key)
/// and [`KeyMap::map_iterate`](crate::keymap::KeyMap::map_iterate) return
/// keys, chosen by the [SortBy](crate::keymap::KeyMap::sort_by) attribute.
///
/// The age orders measure a key's age in one of two ways.
/// `AgeUp` and `AgeDown` use the time a value was last stored under the
/// key, so storing a new value under an existing key makes it the newest.
/// `KeyAgeUp` and `KeyAgeDown` use the time the key was first stored, which
/// storing a new value under it does not change.
// "Up" orders the newest entry first and "Down" the oldest first, which
// reads backwards from the names but follows CompareEntries
// (keymap.c:1463).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    /// No order is asked for; keys are returned in the order of
    /// [`KeyUp`](Self::KeyUp).
    #[default]
    None,
    /// The key with the most recently stored value first.
    AgeUp,
    /// The key with the most recently stored value last.
    AgeDown,
    /// Alphabetical order, comparing the keys byte by byte, with `A...`
    /// first.
    /// A key holding a non-ASCII character sorts before one holding an ASCII
    /// character at the same position.
    KeyUp,
    /// Reverse alphabetical order, with `A...` last.
    KeyDown,
    /// The most recently added key first.
    KeyAgeUp,
    /// The most recently added key last.
    KeyAgeDown,
}

impl SortBy {
    /// The integer code for this order: 0 for [`None`](Self::None) to 6
    /// for [`KeyAgeDown`](Self::KeyAgeDown), in the order the variants are
    /// declared.
    // The codes are C's (keymap.c:239).
    pub fn code(self) -> i32 {
        match self {
            SortBy::None => 0,
            SortBy::AgeUp => 1,
            SortBy::AgeDown => 2,
            SortBy::KeyUp => 3,
            SortBy::KeyDown => 4,
            SortBy::KeyAgeUp => 5,
            SortBy::KeyAgeDown => 6,
        }
    }

    /// The order with integer code `code` (see [`code`](Self::code)), or
    /// `None` if `code` is not from 0 to 6.
    pub fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => SortBy::None,
            1 => SortBy::AgeUp,
            2 => SortBy::AgeDown,
            3 => SortBy::KeyUp,
            4 => SortBy::KeyDown,
            5 => SortBy::KeyAgeUp,
            6 => SortBy::KeyAgeDown,
            _ => return None,
        })
    }

    /// The name of this order, as a dumped KeyMap records it and as
    /// [`parse`](Self::parse) reads it.
    // SortByString (keymap.c:9742). SortByString can also fail, for an
    // integer `sortby` value that is none of the seven known codes, but this
    // method matches over the closed SortBy enum, so the failure case is not
    // representable.
    pub fn as_str(self) -> &'static str {
        match self {
            SortBy::None => "None",
            SortBy::AgeUp => "AgeUp",
            SortBy::AgeDown => "AgeDown",
            SortBy::KeyUp => "KeyUp",
            SortBy::KeyDown => "KeyDown",
            SortBy::KeyAgeUp => "KeyAgeUp",
            SortBy::KeyAgeDown => "KeyAgeDown",
        }
    }

    /// The order named by `text`, one of the names
    /// [`as_str`](Self::as_str) gives.
    ///
    /// Case and trailing white space are ignored, but leading white space
    /// is not.
    /// `method` names the operation in the error message.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Inter`] if `text` names no order.
    // SortByInt (keymap.c:9666), which compares with astChrMatch. `method` is
    // interpolated into the error message as C's `method` parameter is, for
    // the two call sites SortByInt has: astSetAttrib (keymap.c:9225) and
    // astRead (keymap.c:11019).
    pub fn parse(text: &str, method: &str) -> AstResult<Self> {
        let trimmed = text.trim_end();
        for candidate in [
            SortBy::None,
            SortBy::AgeUp,
            SortBy::AgeDown,
            SortBy::KeyAgeUp,
            SortBy::KeyAgeDown,
            SortBy::KeyUp,
            SortBy::KeyDown,
        ] {
            if trimmed.eq_ignore_ascii_case(candidate.as_str()) {
                return Ok(candidate);
            }
        }
        Err(AstError::new(
            ErrorCode::Inter,
            format!("{method}(KeyMap): Illegal SortBy value {text} encountered."),
        ))
    }
}

/// Order two entries under `sort_by`. Mirrors `CompareEntries`
/// (`keymap.c:1463`), except that `SortBy::None` orders by key rather
/// than leaving the pair equal; see `docs/issues/c-library-quirks.md`.
pub(crate) fn compare_entries(
    sort_by: SortBy,
    a: (&str, &MapEntry),
    b: (&str, &MapEntry),
) -> Ordering {
    let (key_a, ent_a) = a;
    let (key_b, ent_b) = b;
    match sort_by {
        // Newest first, so the comparison is reversed relative to the
        // counter.
        SortBy::AgeUp => ent_b.member.cmp(&ent_a.member),
        SortBy::AgeDown => ent_a.member.cmp(&ent_b.member),
        SortBy::KeyAgeUp => ent_b.keymember.cmp(&ent_a.keymember),
        SortBy::KeyAgeDown => ent_a.keymember.cmp(&ent_b.keymember),
        SortBy::KeyUp | SortBy::None => table::key_cmp(key_a, key_b),
        SortBy::KeyDown => table::key_cmp(key_b, key_a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::entry::{EntryValue, Scalar1D};

    #[test]
    fn codes_and_strings_match_c() {
        // keymap.c:239-245 and SortByString at :9742.
        let expected = [
            (SortBy::None, 0, "None"),
            (SortBy::AgeUp, 1, "AgeUp"),
            (SortBy::AgeDown, 2, "AgeDown"),
            (SortBy::KeyUp, 3, "KeyUp"),
            (SortBy::KeyDown, 4, "KeyDown"),
            (SortBy::KeyAgeUp, 5, "KeyAgeUp"),
            (SortBy::KeyAgeDown, 6, "KeyAgeDown"),
        ];
        for (variant, code, text) in expected {
            assert_eq!(variant.code(), code, "{text}");
            assert_eq!(SortBy::from_code(code), Some(variant), "{text}");
            assert_eq!(variant.as_str(), text);
            assert_eq!(SortBy::parse(text, "astRead").unwrap(), variant);
        }
        assert_eq!(SortBy::from_code(7), None);
    }

    #[test]
    fn parse_folds_case_and_trailing_space() {
        // astChrMatch folds case and tolerates trailing whitespace.
        assert_eq!(SortBy::parse("keyup", "astRead").unwrap(), SortBy::KeyUp);
        assert_eq!(
            SortBy::parse("KEYAGEDOWN  ", "astRead").unwrap(),
            SortBy::KeyAgeDown
        );
        assert_eq!(
            SortBy::parse("  none", "astRead").unwrap_err().code(),
            ErrorCode::Inter
        );

        let err = SortBy::parse("Sideways", "astRead").unwrap_err();
        assert_eq!(err.code(), ErrorCode::Inter);
        assert!(
            err.to_string()
                .contains("astRead(KeyMap): Illegal SortBy value Sideways encountered."),
            "{err}"
        );
    }

    #[test]
    fn age_up_puts_the_newest_first() {
        // CompareEntries returns 1 when first->member < second->member, so a
        // lower member index sorts later.
        let older = entry_with(1, 1);
        let newer = entry_with(9, 9);
        assert_eq!(
            compare_entries(SortBy::AgeUp, ("A", &older), ("B", &newer)),
            Ordering::Greater
        );
        assert_eq!(
            compare_entries(SortBy::AgeDown, ("A", &older), ("B", &newer)),
            Ordering::Less
        );
    }

    #[test]
    fn key_age_uses_the_key_counter_not_the_value_counter() {
        // A re-put advances member but not keymember, so KeyAgeUp is blind
        // to it while AgeUp is not.
        let a = entry_with(100, 1);
        let b = entry_with(2, 9);
        assert_eq!(
            compare_entries(SortBy::KeyAgeUp, ("A", &a), ("B", &b)),
            Ordering::Greater
        );
        assert_eq!(
            compare_entries(SortBy::AgeUp, ("A", &a), ("B", &b)),
            Ordering::Less
        );
    }

    #[test]
    fn key_up_and_key_down_are_key_cmp_either_way_round() {
        let a = entry_with(1, 1);
        let b = entry_with(2, 2);
        assert_eq!(
            compare_entries(SortBy::KeyUp, ("ALPHA", &a), ("BETA", &b)),
            Ordering::Less
        );
        assert_eq!(
            compare_entries(SortBy::KeyDown, ("ALPHA", &a), ("BETA", &b)),
            Ordering::Greater
        );
        // Trailing spaces are ignored, as KeyCmp does.
        assert_eq!(
            compare_entries(SortBy::KeyUp, ("ALPHA  ", &a), ("ALPHA", &b)),
            Ordering::Equal
        );
    }

    #[test]
    fn none_orders_alphabetically_unlike_c() {
        // C leaves SORTBY_NONE unordered; Rust sorts by KeyCmp. See
        // docs/issues/c-library-quirks.md.
        let a = entry_with(9, 9);
        let b = entry_with(1, 1);
        assert_eq!(
            compare_entries(SortBy::None, ("ALPHA", &a), ("BETA", &b)),
            Ordering::Less
        );
    }

    fn entry_with(member: u64, keymember: u64) -> MapEntry {
        MapEntry {
            comment: None,
            member,
            keymember,
            value: EntryValue::Int(Scalar1D::Scalar(0)),
        }
    }
}

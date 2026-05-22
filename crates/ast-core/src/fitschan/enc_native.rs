//! Keyword-card NATIVE encoding decoder.
//!
//! Translates BEGAST_xx / ENDAST_xx / ISA_xx / <NAME>_xx FITS keyword cards
//! into the line-oriented text format consumed by
//! [`crate::serial::native_read::parse_native`].
//!
//! Mirrors C library `fitschan.c::FindAst` (line 17820) and
//! `fitschan.c::CreateKeyword` (line 8209).

/// Characters used in the 2-character sequence suffix on every NATIVE
/// keyword. Matches `SEQ_CHARS` in `fitschan.c` (line 1377).
// Will be used by the keyword decoder in subsequent tasks.
#[allow(dead_code)]
pub(crate) const SEQ_CHARS: &str = "_ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Returns true if `c` is a valid sequence-suffix character.
// Will be used by the keyword decoder in subsequent tasks.
#[allow(dead_code)]
pub(crate) fn is_seq_char(c: char) -> bool {
    SEQ_CHARS.contains(c)
}

/// Returns true if `s` is exactly two SEQ_CHARS characters.
// Will be used by the keyword decoder in subsequent tasks.
#[allow(dead_code)]
pub(crate) fn is_two_char_seq_suffix(s: &str) -> bool {
    s.len() == 2 && s.chars().all(is_seq_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_chars_recognises_underscore_and_letters() {
        assert!(is_seq_char('_'));
        assert!(is_seq_char('A'));
        assert!(is_seq_char('Z'));
        assert!(!is_seq_char('a'));
        assert!(!is_seq_char('1'));
        assert!(!is_seq_char('0'));
    }

    #[test]
    fn two_char_seq_suffix_examples() {
        assert!(is_two_char_seq_suffix("_A"));
        assert!(is_two_char_seq_suffix("AB"));
        assert!(is_two_char_seq_suffix("__"));
        assert!(is_two_char_seq_suffix("ZZ"));
        assert!(!is_two_char_seq_suffix("A"));
        assert!(!is_two_char_seq_suffix("ABC"));
        assert!(!is_two_char_seq_suffix("a_"));
        assert!(!is_two_char_seq_suffix("0_"));
    }
}

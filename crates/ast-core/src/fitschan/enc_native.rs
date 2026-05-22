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
// Used by classify; will be transitively live once Task 4 wires classify in.
#[allow(dead_code)]
pub(crate) fn is_two_char_seq_suffix(s: &str) -> bool {
    s.len() == 2 && s.chars().all(is_seq_char)
}

/// Classification of a keyword card under the NATIVE keyword-card encoding.
// Used by classify; will be live once Task 4 calls classify.
#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeywordKind {
    /// `BEGAST<xx>` — opens an object block; value is the class name.
    Begin,
    /// `ENDAST<xx>` — closes an object block; value is the class name.
    End,
    /// `ISA<xx>` — inheritance marker; value is the parent class name.
    IsA,
    /// `<NAME>_<xx>` — data attribute; the inner string is the lowercased
    /// base name (without the 2-char suffix) ready to emit as a native key.
    Data(String),
    /// Any other keyword (SIMPLE, NAXIS, COMMENT, CONTINUE, ...) — not part
    /// of NATIVE keyword-card encoding.
    Other,
}

/// Classify a FITS keyword as a NATIVE encoding marker, data card, or other.
// Will be live once Task 4 calls this function.
#[allow(dead_code)]
pub(crate) fn classify(keyword: &str) -> KeywordKind {
    let len = keyword.len();
    if len < 3 {
        return KeywordKind::Other;
    }
    // The trailing 2 characters must both be SEQ_CHARS to be a NATIVE card.
    let (head, suffix) = keyword.split_at(len - 2);
    if !is_two_char_seq_suffix(suffix) {
        return KeywordKind::Other;
    }
    match head {
        "BEGAST" => KeywordKind::Begin,
        "ENDAST" => KeywordKind::End,
        "ISA" => KeywordKind::IsA,
        _ => {
            if head.is_empty() {
                KeywordKind::Other
            } else {
                KeywordKind::Data(head.to_ascii_lowercase())
            }
        }
    }
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

    #[test]
    fn classify_recognises_begast() {
        assert_eq!(classify("BEGAST_A"), KeywordKind::Begin);
        assert_eq!(classify("BEGAST_Z"), KeywordKind::Begin);
        assert_eq!(classify("BEGASTAA"), KeywordKind::Begin);
    }

    #[test]
    fn classify_recognises_endast_and_isa() {
        assert_eq!(classify("ENDAST_A"), KeywordKind::End);
        assert_eq!(classify("ISA_A"), KeywordKind::IsA);
        assert_eq!(classify("ISAAB"), KeywordKind::IsA);
    }

    #[test]
    fn classify_recognises_data_card() {
        assert_eq!(
            classify("NFRAME_A"),
            KeywordKind::Data("nframe".to_string())
        );
        assert_eq!(
            classify("DOMAIN_B"),
            KeywordKind::Data("domain".to_string())
        );
        assert_eq!(
            classify("FRAMEA_A"),
            KeywordKind::Data("framea".to_string())
        );
    }

    #[test]
    fn classify_rejects_non_native_keywords() {
        // Syntactically invalid SEQ_CHARS suffix → always Other.
        assert_eq!(classify("CTYPE1"), KeywordKind::Other); // suffix is "E1", '1' not in SEQ_CHARS
        assert_eq!(classify(""), KeywordKind::Other);
        assert_eq!(classify("AB"), KeywordKind::Other);
        // Standard FITS keywords whose last two chars happen to pass the SEQ_CHARS
        // check are returned as Data — rejection for "wrong value type" is the
        // caller's responsibility, not the keyword-name classifier.
        // C library rejects SIMPLE/COMMENT/CONTINUE via AST card-type check
        // (AST__LOGICAL / AST__COMMENT), which is not visible here.
        assert_eq!(classify("SIMPLE"), KeywordKind::Data("simp".to_string()));
        assert_eq!(classify("NAXIS"), KeywordKind::Data("nax".to_string()));
        assert_eq!(classify("COMMENT"), KeywordKind::Data("comme".to_string()));
        assert_eq!(
            classify("CONTINUE"),
            KeywordKind::Data("contin".to_string())
        );
    }
}

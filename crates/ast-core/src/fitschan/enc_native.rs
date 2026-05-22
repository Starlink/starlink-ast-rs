//! Keyword-card NATIVE encoding decoder.
//!
//! Translates BEGAST_xx / ENDAST_xx / ISA_xx / <NAME>_xx FITS keyword cards
//! into the line-oriented text format consumed by
//! [`crate::serial::native_read::parse_native`].
//!
//! Mirrors C library `fitschan.c::FindAst` (line 17820) and
//! `fitschan.c::CreateKeyword` (line 8209).

use super::card_list::CardList;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitschan::card::FitsValue;

/// Characters used in the 2-character sequence suffix on every NATIVE
/// keyword. Matches `SEQ_CHARS` in `fitschan.c` (line 1377).
// Will be transitively live once Task 8 wires has_keyword_native into FitsChan::read.
#[allow(dead_code)]
pub(crate) const SEQ_CHARS: &str = "_ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Returns true if `c` is a valid sequence-suffix character.
// Will be transitively live once Task 8 wires has_keyword_native into FitsChan::read.
#[allow(dead_code)]
pub(crate) fn is_seq_char(c: char) -> bool {
    SEQ_CHARS.contains(c)
}

/// Returns true if `s` is exactly two SEQ_CHARS characters.
// Will be transitively live once Task 8 wires has_keyword_native into FitsChan::read.
#[allow(dead_code)]
pub(crate) fn is_two_char_seq_suffix(s: &str) -> bool {
    s.len() == 2 && s.chars().all(is_seq_char)
}

/// Classification of a keyword card under the NATIVE keyword-card encoding.
// Will be transitively live once Task 8 wires has_keyword_native into FitsChan::read.
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
// Will be transitively live once Task 8 wires has_keyword_native into FitsChan::read.
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

/// Returns true if the card list contains at least one `BEGAST<xx>` keyword,
/// indicating keyword-card NATIVE encoding.
///
/// Mirrors the C library check at `fitschan.c::GetEncoding` line 12566:
/// `astKeyFields( this, "BEGAST%2f", 0, NULL, NULL )`.
// Will be live once Task 8 wires this into FitsChan::read.
#[allow(dead_code)]
pub(crate) fn has_keyword_native(cards: &CardList) -> bool {
    cards
        .iter()
        .any(|card| matches!(classify(&card.keyword), KeywordKind::Begin))
}

/// Result of formatting a card value for native-text emission.
#[derive(Debug)]
// Will be used by Task 6's cards_to_native_text.
#[allow(dead_code)]
pub(crate) enum FormattedValue {
    /// The card has a non-empty scalar value; emit `<key> = <text>`.
    Scalar(String),
    /// The card has an empty string value; emit `<key> =` and expect a
    /// child object to follow on subsequent cards (a `BEGAST<xx>` card).
    ObjectMarker,
}

/// Format a `FitsValue` for emission as native-text scalar.
///
/// Returns `None` if the value type is not representable in NATIVE encoding
/// (Comment, Logical, Undefined) — such cards should be ignored by the
/// keyword-card decoder.
// Will be used by Task 6's cards_to_native_text.
#[allow(dead_code)]
pub(crate) fn format_value(value: &FitsValue) -> Option<FormattedValue> {
    match value {
        FitsValue::String(s) => {
            let trimmed = s.trim_end();
            if trimmed.is_empty() {
                Some(FormattedValue::ObjectMarker)
            } else {
                // Quote the string for native text. Embedded double-quotes
                // are escaped by doubling (`""`), matching the convention
                // un-escaped by `parse_native::parse_scalar_value`. Backslash
                // is NOT a metacharacter in this format — it passes through
                // unchanged.
                let escaped = trimmed.replace('"', "\"\"");
                Some(FormattedValue::Scalar(format!("\"{escaped}\"")))
            }
        }
        FitsValue::Int(n) => Some(FormattedValue::Scalar(n.to_string())),
        FitsValue::Float(f) => Some(FormattedValue::Scalar(format_native_float(*f))),
        // Logical, Comment, Undefined, Continue, ComplexFloat, ComplexInt are not native scalars.
        _ => None,
    }
}

/// Format a float for NATIVE text emission. Mirrors the C library's choice
/// of `%.17g` (full double precision) so a NATIVE round-trip is value-exact.
// Will be used by Task 6's cards_to_native_text.
#[allow(dead_code)]
fn format_native_float(x: f64) -> String {
    if x.is_nan() {
        // Native format does not have a NaN literal; the AST library uses
        // the BAD_VALUE sentinel for missing data and never serialises NaN.
        // Emit a placeholder that downstream parsing will reject loudly.
        return "<bad>".to_string();
    }
    let s = format!("{x:.17e}");
    // Compress to a more compact form when round-tripping is preserved.
    // Match parse_native's tolerance for both `1.5` and `1.5e0` styles.
    s
}

/// Translate a card list with keyword-card NATIVE encoding into the
/// line-oriented text format consumed by [`crate::serial::native_read::parse_native`].
///
/// The card list is walked in order. Cards that are not part of NATIVE
/// keyword-card encoding (SIMPLE, NAXIS, COMMENT, etc.) are silently
/// skipped. Data cards whose value type is not representable as a NATIVE
/// scalar (Logical, Comment, Continue, Undef, complex) are also silently
/// skipped — this matches the C library's value-type gate in
/// `fitschan.c::FindAst` (line 17932) where `BEGAST`/`ENDAST`/`ISA`
/// require `AST__STRING` and data cards require
/// `AST__STRING | AST__INT | AST__FLOAT`.
///
/// Returns an error only if a `BEGAST`/`ENDAST`/`ISA` marker has a
/// non-string value (an outright invalid header).
// Will be transitively live once Task 8 wires this into FitsChan::read.
#[allow(dead_code)]
pub(crate) fn cards_to_native_text(cards: &CardList) -> AstResult<String> {
    let mut out = String::new();
    for card in cards.iter() {
        let kind = classify(&card.keyword);
        match kind {
            KeywordKind::Begin => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str(" Begin ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::End => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str(" End ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::IsA => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str("    IsA ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::Data(name) => match format_value(&card.value) {
                Some(FormattedValue::Scalar(text)) => {
                    out.push_str("    ");
                    out.push_str(&name);
                    out.push_str(" = ");
                    out.push_str(&text);
                    out.push('\n');
                }
                Some(FormattedValue::ObjectMarker) => {
                    out.push_str("    ");
                    out.push_str(&name);
                    out.push_str(" =\n");
                }
                None => {
                    // Non-scalar value type — skip silently. Matches C library
                    // behaviour in fitschan.c::FindAst line 17932 where the
                    // Data branch is gated on string/int/float value types.
                }
            },
            KeywordKind::Other => continue,
        }
    }
    Ok(out)
}

fn expect_class_name(value: &FitsValue, keyword: &str) -> AstResult<String> {
    match value {
        FitsValue::String(s) => Ok(s.trim().to_string()),
        _ => Err(AstError::new(
            ErrorCode::Rderr,
            format!("NATIVE keyword '{keyword}' must have string value, got {value:?}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::FitsChan;

    fn cards_from(text: &str) -> CardList {
        let fc = FitsChan::from_header(text).expect("from_header");
        fc.cards_for_test().clone()
    }

    #[test]
    fn detects_keyword_native_when_begast_present() {
        let header = "\
BEGAST_A= 'FrameSet'           / Set of inter-related coordinate systems
NFRAME_A=                    2 / Number of Frames in FrameSet
ENDAST_A= 'FrameSet'           / End of object definition
END
";
        let cards = cards_from(header);
        assert!(has_keyword_native(&cards));
    }

    #[test]
    fn rejects_pure_wcs_header() {
        let header = "\
SIMPLE  =                    T / file conforms to FITS standard
NAXIS   =                    2 / number of data axes
CTYPE1  = 'RA---TAN'           / coord type
CRPIX1  =                100.0 / reference pixel
CRVAL1  =                  0.0 / reference value
END
";
        let cards = cards_from(header);
        assert!(!has_keyword_native(&cards));
    }

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

    #[test]
    fn format_value_string_non_empty() {
        let v = FitsValue::String("FrameSet".to_string());
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => assert_eq!(s, "\"FrameSet\""),
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn format_value_string_empty_is_object_marker() {
        let v = FitsValue::String("        ".to_string());
        assert!(matches!(
            format_value(&v),
            Some(FormattedValue::ObjectMarker)
        ));
    }

    #[test]
    fn format_value_int() {
        let v = FitsValue::Int(42);
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => assert_eq!(s, "42"),
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn format_value_float_basic() {
        let v = FitsValue::Float(1.5);
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => {
                // Just confirm it's parseable by Rust as 1.5.
                let parsed: f64 = s.parse().expect("parses");
                assert_eq!(parsed, 1.5);
            }
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn format_value_string_with_embedded_quotes_roundtrips() {
        // The encoder doubles literal `"` so the parser's `""` -> `"` un-escape
        // recovers the original value.
        let v = FitsValue::String(r#"say "hi""#.to_string());
        let Some(FormattedValue::Scalar(s)) = format_value(&v) else {
            panic!("expected Scalar");
        };
        // Strip outer quotes, run the parser's inverse transform.
        assert!(s.starts_with('"') && s.ends_with('"'));
        let inner = &s[1..s.len() - 1];
        let decoded = inner.replace("\"\"", "\"");
        assert_eq!(decoded, r#"say "hi""#);
    }

    #[test]
    fn format_value_string_with_backslash_passes_through() {
        // Backslash is not a metacharacter — it must round-trip as-is.
        let v = FitsValue::String(r"a\b".to_string());
        let Some(FormattedValue::Scalar(s)) = format_value(&v) else {
            panic!("expected Scalar");
        };
        assert_eq!(s, r#""a\b""#);
    }

    #[test]
    fn cards_to_native_text_round_trips_a_minimal_frameset() {
        let header = "\
BEGAST_A= 'FrameSet'           / Set of inter-related coordinate systems
NFRAME_A=                    2 / Number of Frames in FrameSet
ENDAST_A= 'FrameSet'           / End of object definition
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        assert!(text.contains("Begin FrameSet"));
        assert!(text.contains("nframe = 2"));
        assert!(text.contains("End FrameSet"));
    }

    #[test]
    fn cards_to_native_text_emits_object_marker_for_empty_string() {
        let header = "\
BEGAST_A= 'FrameSet'
FRAMEA_A= '        '           / First component Frame
BEGAST_B= 'Frame   '
ENDAST_A= 'Frame   '
ENDAST_B= 'FrameSet'
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        // The empty-string FRAMEA_A must produce a child-object marker.
        assert!(text.contains("framea =\n"));
        // The next emitted line is `Begin Frame`.
        let pos_marker = text.find("framea =\n").expect("marker present");
        let after = &text[pos_marker + "framea =\n".len()..];
        assert!(
            after.trim_start().starts_with("Begin Frame"),
            "expected child Begin after object marker, got: {after}"
        );
    }

    #[test]
    fn cards_to_native_text_skips_non_scalar_data_cards() {
        // A card whose name happens to end in two SEQ_CHARS but whose value
        // is logical (e.g. SIMPLE = T in a header that also has BEGAST
        // somewhere) must be silently skipped, not error.
        let header = "\
SIMPLE  =                    T / not a NATIVE data card despite name suffix
BEGAST_A= 'FrameSet'
NFRAME_A=                    2
ENDAST_A= 'FrameSet'
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        // SIMPLE -> Data("simp") classification, but value is Logical -> skip.
        assert!(!text.contains("simp"));
        // FrameSet body still emitted.
        assert!(text.contains("Begin FrameSet"));
        assert!(text.contains("nframe = 2"));
    }
}

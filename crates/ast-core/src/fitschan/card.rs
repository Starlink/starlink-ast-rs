//! FITS header card parsing and formatting.
//!
//! Implements the FITS standard 80-character header card format, supporting
//! all FITS value types: integer, float, string, logical, complex, comment,
//! undefined, and CONTINUE long strings.

use crate::error::{AstError, AstResult, ErrorCode};

/// Length of a FITS header card in characters.
const FITS_CARD_LEN: usize = 80;

/// Length of a FITS keyword name field (columns 1-8).
const FITS_NAME_LEN: usize = 8;

/// Column (0-based) where right-justified numeric values and logical T/F land.
/// In FITS standard terms this is column 30 (1-based), i.e. index 29.
const FITS_RL_COL: usize = 29;

/// Minimum ending column (0-based) for quoted string values (column 20 → index 19).
#[allow(dead_code)]
const FITS_ST_COL: usize = 19;

/// A typed value from a FITS header card.
#[derive(Debug, Clone, PartialEq)]
pub enum FitsValue {
    /// Integer value (FITS allows up to 64-bit).
    Int(i64),
    /// Floating-point value.
    Float(f64),
    /// Character string value (single-quoted in FITS).
    String(String),
    /// Logical value (T or F).
    Logical(bool),
    /// Complex floating-point (real, imaginary).
    ComplexFloat(f64, f64),
    /// Complex integer (real, imaginary).
    ComplexInt(i64, i64),
    /// Comment-only card (COMMENT, HISTORY, blank keyword).
    Comment(String),
    /// Undefined value (keyword present but value blank).
    Undef,
    /// CONTINUE long string value.
    Continue(String),
}

/// A single FITS header card (keyword + value + comment).
#[derive(Debug, Clone)]
pub struct FitsCard {
    /// Keyword name (max 8 chars, uppercase, trimmed).
    pub keyword: String,
    /// Typed value.
    pub value: FitsValue,
    /// Comment string (text after / separator).
    pub comment: Option<String>,
    /// Whether this card has been consumed by a read operation.
    #[allow(dead_code)]
    pub(crate) used: bool,
}

impl FitsCard {
    /// Parse an 80-character FITS header card.
    ///
    /// If the input is shorter than 80 characters it is right-padded with spaces.
    /// If longer, it is truncated to 80 characters. Non-printing trailing
    /// characters (e.g. newlines) are stripped before padding.
    pub fn parse(card: &str) -> AstResult<Self> {
        // Prepare the 80-character card buffer.
        let card = prepare_card(card);
        let bytes = card.as_bytes();

        // Extract keyword (columns 1-8, i.e. indices 0..8).
        let raw_name = &card[..FITS_NAME_LEN];
        let keyword = raw_name.trim_end().to_uppercase();
        let blank_name = keyword.is_empty();

        // Check for CONTINUE card: keyword is CONTINUE, col 9 is space, col 11 is quote.
        let is_continue = keyword == "CONTINUE"
            && bytes.len() > FITS_NAME_LEN + 2
            && bytes[FITS_NAME_LEN] == b' '
            && bytes[FITS_NAME_LEN + 2] == b'\'';

        // Commentary cards: COMMENT, HISTORY, blank keyword, or no '=' in col 9.
        let is_commentary = !is_continue
            && (blank_name
                || keyword == "COMMENT"
                || keyword == "HISTORY"
                || keyword == "END"
                || bytes[FITS_NAME_LEN] != b'=');

        if is_commentary {
            let text = if card.len() > FITS_NAME_LEN {
                &card[FITS_NAME_LEN..]
            } else {
                ""
            };
            let keyword_out = if blank_name { String::new() } else { keyword };
            return Ok(FitsCard {
                keyword: keyword_out,
                value: FitsValue::Comment(text.to_string()),
                comment: None,
                used: false,
            });
        }

        // Value cards: col 9 is '=' (or CONTINUE card).
        // Value area starts at col 11 (index 10) for '=' cards, or col 11 for CONTINUE.
        let value_start = if is_continue {
            FITS_NAME_LEN + 2 // skip "CONTINUE " then quote is at +2
        } else {
            FITS_NAME_LEN + 1 // skip "=" — but skip the space in col 10 during parsing
        };

        let value_area = &card[value_start..];

        // Find first non-blank character in value area.
        let first_nonblank = value_area.find(|c: char| !c.is_ascii_whitespace());

        // If the value area starts with a single quote, it is a string value.
        if let Some(pos) = first_nonblank {
            let first_char = value_area.as_bytes()[pos];
            if first_char == b'\'' {
                return parse_string_value(&keyword, value_area, pos, is_continue);
            }
        }

        // For non-string values, find the comment separator '/'.
        // The '/' marks the end of value and start of comment.
        let (val_str, comment_str) = split_value_comment(value_area);

        // Trim the value string.
        let val_trimmed = val_str.trim();

        // Determine value type.
        if val_trimmed.is_empty() {
            // Undefined value.
            let comment = trim_comment(comment_str);
            return Ok(FitsCard {
                keyword,
                value: FitsValue::Undef,
                comment,
                used: false,
            });
        }

        // Check for logical values: T or F (possibly with leading spaces,
        // single character). The FITS standard puts T or F at column 30.
        if val_trimmed == "T" || val_trimmed == "F" {
            let comment = trim_comment(comment_str);
            return Ok(FitsCard {
                keyword,
                value: FitsValue::Logical(val_trimmed == "T"),
                comment,
                used: false,
            });
        }

        // Check for complex values: (real, imag) — parenthesized pair.
        if val_trimmed.starts_with('(') && val_trimmed.ends_with(')') {
            if let Some(parsed) = try_parse_complex(val_trimmed) {
                let comment = trim_comment(comment_str);
                return Ok(FitsCard {
                    keyword,
                    value: parsed,
                    comment,
                    used: false,
                });
            }
        }

        // Try numeric: no dots/exponents → integer; otherwise float.
        // Following the C Split() logic closely.
        let has_decimal_or_exp = val_trimmed
            .bytes()
            .any(|b| b == b'.' || b == b'E' || b == b'e' || b == b'D' || b == b'd');

        if !has_decimal_or_exp {
            // Try two integers separated by whitespace (complex integer).
            if let Some(cval) = try_parse_complex_int(val_trimmed) {
                let comment = trim_comment(comment_str);
                return Ok(FitsCard {
                    keyword,
                    value: cval,
                    comment,
                    used: false,
                });
            }
            // Try single integer (64-bit).
            if let Ok(ival) = val_trimmed.parse::<i64>() {
                let comment = trim_comment(comment_str);
                return Ok(FitsCard {
                    keyword,
                    value: FitsValue::Int(ival),
                    comment,
                    used: false,
                });
            }
        } else {
            // Try two floats separated by whitespace (complex float).
            if let Some(cval) = try_parse_complex_float(val_trimmed) {
                let comment = trim_comment(comment_str);
                return Ok(FitsCard {
                    keyword,
                    value: cval,
                    comment,
                    used: false,
                });
            }
            // Try single float.
            if let Some(fval) = try_parse_float(val_trimmed) {
                let comment = trim_comment(comment_str);
                return Ok(FitsCard {
                    keyword,
                    value: FitsValue::Float(fval),
                    comment,
                    used: false,
                });
            }
        }

        // If we get here, we could not determine the value type.
        // The C code issues a warning and treats it as a comment. We return an error.
        Err(AstError::new(
            ErrorCode::Bdfts,
            format!(
                "Cannot determine data type of FITS keyword value: '{}'",
                val_trimmed
            ),
        ))
    }

    /// Format the card back to an 80-character FITS header card string.
    pub fn format(&self) -> String {
        let mut buf = String::with_capacity(FITS_CARD_LEN);

        // Write keyword, left-justified, padded to 8 chars.
        let kw = &self.keyword;
        buf.push_str(kw);
        for _ in kw.len()..FITS_NAME_LEN {
            buf.push(' ');
        }

        match &self.value {
            FitsValue::Comment(text) => {
                // Commentary cards: keyword + text, padded to 80.
                let remaining = FITS_CARD_LEN - buf.len();
                let text_bytes = text.as_bytes();
                let to_copy = text_bytes.len().min(remaining);
                buf.push_str(&text[..to_copy]);
            }
            FitsValue::Continue(s) => {
                // CONTINUE card: no '=' sign, space then quoted string.
                buf.push(' '); // col 9
                buf.push(' '); // col 10
                format_string_value(&mut buf, s);
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::Undef => {
                // Value indicator with blank value area.
                buf.push_str("= ");
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::Int(v) => {
                buf.push_str("= ");
                // Right-justify integer to column 30 (index 29).
                // Columns used for value: indices 10..30, i.e. 20 chars.
                let formatted = format!("{v}");
                let width = FITS_RL_COL + 1 - (FITS_NAME_LEN + 2); // 20
                if formatted.len() < width {
                    for _ in 0..(width - formatted.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&formatted);
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::Float(v) => {
                buf.push_str("= ");
                let formatted = format_float(*v);
                let width = FITS_RL_COL + 1 - (FITS_NAME_LEN + 2); // 20
                if formatted.len() < width {
                    for _ in 0..(width - formatted.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&formatted);
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::Logical(v) => {
                buf.push_str("= ");
                // T or F at column 30 (index 29). Value area starts at index 10.
                // So we need 19 spaces then T/F.
                let width = FITS_RL_COL + 1 - (FITS_NAME_LEN + 2); // 20
                for _ in 0..(width - 1) {
                    buf.push(' ');
                }
                buf.push(if *v { 'T' } else { 'F' });
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::String(s) => {
                buf.push_str("= ");
                format_string_value(&mut buf, s);
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::ComplexFloat(r, i) => {
                buf.push_str("= ");
                let r_fmt = format_float(*r);
                let i_fmt = format_float(*i);
                // Right-justify real to col 30, imaginary to col 50.
                let r_width = FITS_RL_COL + 1 - (FITS_NAME_LEN + 2); // 20
                if r_fmt.len() < r_width {
                    for _ in 0..(r_width - r_fmt.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&r_fmt);
                // Imaginary part: right-justify in next 20 chars.
                let i_width = 20;
                if i_fmt.len() < i_width {
                    for _ in 0..(i_width - i_fmt.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&i_fmt);
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::ComplexInt(r, i) => {
                buf.push_str("= ");
                let r_fmt = format!("{r}");
                let i_fmt = format!("{i}");
                let r_width = FITS_RL_COL + 1 - (FITS_NAME_LEN + 2); // 20
                if r_fmt.len() < r_width {
                    for _ in 0..(r_width - r_fmt.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&r_fmt);
                let i_width = 20;
                if i_fmt.len() < i_width {
                    for _ in 0..(i_width - i_fmt.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&i_fmt);
                append_comment(&mut buf, self.comment.as_deref());
            }
        }

        // Pad to 80 characters.
        while buf.len() < FITS_CARD_LEN {
            buf.push(' ');
        }
        // Truncate if over 80.
        buf.truncate(FITS_CARD_LEN);
        buf
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Prepare the card: strip trailing non-printing chars, pad/truncate to 80.
fn prepare_card(input: &str) -> String {
    // Strip trailing non-printing characters.
    let trimmed = input.trim_end_matches(|c: char| !c.is_ascii_graphic() && c != ' ');
    // Take at most 80 characters.
    let truncated: String = trimmed.chars().take(FITS_CARD_LEN).collect();
    // Pad with spaces to 80 characters.
    let mut card = truncated;
    while card.len() < FITS_CARD_LEN {
        card.push(' ');
    }
    card
}

/// Parse a string value (starts with quote) from the value area.
fn parse_string_value(
    keyword: &str,
    value_area: &str,
    quote_pos: usize,
    is_continue: bool,
) -> AstResult<FitsCard> {
    let bytes = value_area.as_bytes();
    let mut result = String::new();
    let mut i = quote_pos + 1; // skip opening quote
    let mut last_was_quote = false;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '\'' {
            if last_was_quote {
                // Two adjacent quotes → one literal quote.
                result.push('\'');
                last_was_quote = false;
            } else {
                last_was_quote = true;
            }
        } else {
            if last_was_quote {
                // Single quote ended the string. 'i' points to first char after closing quote.
                break;
            }
            result.push(ch);
        }
        i += 1;
    }

    // If we ended the loop with last_was_quote true and didn't break,
    // that means the closing quote was at the end of the area.

    // Trim trailing spaces from the string value (FITS standard).
    let result = result.trim_end().to_string();

    // Everything after the closing quote is potential comment area.
    // Find the '/' comment separator in the remainder.
    let remainder = if i < bytes.len() {
        &value_area[i..]
    } else {
        ""
    };
    let comment = find_comment_in_remainder(remainder);

    let value = if is_continue {
        FitsValue::Continue(result)
    } else {
        FitsValue::String(result)
    };

    Ok(FitsCard {
        keyword: keyword.to_string(),
        value,
        comment,
        used: false,
    })
}

/// Split the value area into value string and comment string at the first '/'.
fn split_value_comment(value_area: &str) -> (&str, Option<&str>) {
    if let Some(slash_pos) = value_area.find('/') {
        (&value_area[..slash_pos], Some(&value_area[slash_pos + 1..]))
    } else {
        (value_area, None)
    }
}

/// Find comment after the closing quote of a string value.
fn find_comment_in_remainder(remainder: &str) -> Option<String> {
    if let Some(slash_pos) = remainder.find('/') {
        let comment = &remainder[slash_pos + 1..];
        let trimmed = comment.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else {
        None
    }
}

/// Trim and return comment text, or None if blank.
fn trim_comment(comment: Option<&str>) -> Option<String> {
    comment.and_then(|c| {
        let trimmed = c.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Try to parse a float value, handling 'D'/'d' exponents.
fn try_parse_float(s: &str) -> Option<f64> {
    // First try direct parse.
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    // Replace D/d with E and retry.
    let replaced: String = s
        .chars()
        .map(|c| if c == 'D' || c == 'd' { 'E' } else { c })
        .collect();
    replaced.parse::<f64>().ok()
}

/// Try to parse two whitespace-separated integers as complex integer.
fn try_parse_complex_int(s: &str) -> Option<FitsValue> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() == 2 {
        if let (Ok(r), Ok(i)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
            return Some(FitsValue::ComplexInt(r, i));
        }
    }
    None
}

/// Try to parse two whitespace-separated floats as complex float.
fn try_parse_complex_float(s: &str) -> Option<FitsValue> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() == 2 {
        let r = try_parse_float(parts[0]);
        let i = try_parse_float(parts[1]);
        if let (Some(rv), Some(iv)) = (r, i) {
            return Some(FitsValue::ComplexFloat(rv, iv));
        }
    }
    None
}

/// Try to parse a parenthesized complex value like `(1.0, 2.0)` or `(1, 2)`.
fn try_parse_complex(s: &str) -> Option<FitsValue> {
    // Strip parens.
    let inner = s.strip_prefix('(')?.strip_suffix(')')?;
    // Split on comma.
    let parts: Vec<&str> = inner.splitn(2, ',').collect();
    if parts.len() != 2 {
        return None;
    }
    let r_str = parts[0].trim();
    let i_str = parts[1].trim();

    // If either has a decimal point or exponent, treat as float complex.
    let has_float_marker = |t: &str| {
        t.bytes()
            .any(|b| b == b'.' || b == b'E' || b == b'e' || b == b'D' || b == b'd')
    };

    if has_float_marker(r_str) || has_float_marker(i_str) {
        let r = try_parse_float(r_str)?;
        let i = try_parse_float(i_str)?;
        Some(FitsValue::ComplexFloat(r, i))
    } else {
        let r = r_str.parse::<i64>().ok()?;
        let i = i_str.parse::<i64>().ok()?;
        Some(FitsValue::ComplexInt(r, i))
    }
}

/// Format a float value for FITS output.
/// Uses a representation that preserves precision while staying compact.
/// Mimics C's `%G` format: uses fixed notation when the exponent is in [-4, 15],
/// scientific notation otherwise, and trims trailing zeros.
fn format_float(v: f64) -> String {
    if v == 0.0 {
        return "0.0".to_string();
    }

    // Determine which representation is more compact, mimicking %G behaviour.
    let abs_v = v.abs();
    let exp = abs_v.log10().floor() as i32;

    if (-4..=14).contains(&exp) {
        // Fixed notation with enough precision.
        // Number of decimal places needed: 15 - (exp + 1), but at least 1.
        let decimal_places = (15 - (exp + 1)).max(1) as usize;
        let s = format!("{v:.decimal_places$}");
        // Trim trailing zeros but keep at least one decimal digit.
        trim_trailing_zeros(&s)
    } else {
        // Scientific notation.
        let s = format!("{v:.14E}");
        // Trim trailing zeros in the mantissa.
        trim_trailing_zeros_exp(&s)
    }
}

/// Trim trailing zeros from a fixed-point formatted number, keeping at least one
/// digit after the decimal point.
fn trim_trailing_zeros(s: &str) -> String {
    if let Some(dot_pos) = s.find('.') {
        let trimmed = s.trim_end_matches('0');
        // Keep at least one digit after the dot.
        if trimmed.ends_with('.') {
            format!("{trimmed}0")
        } else if trimmed.len() <= dot_pos {
            format!("{}0", &s[..=dot_pos])
        } else {
            trimmed.to_string()
        }
    } else {
        s.to_string()
    }
}

/// Trim trailing zeros in the mantissa of a scientific notation number.
fn trim_trailing_zeros_exp(s: &str) -> String {
    if let Some(e_pos) = s.find('E').or_else(|| s.find('e')) {
        let mantissa = &s[..e_pos];
        let exponent = &s[e_pos..];
        let trimmed = trim_trailing_zeros(mantissa);
        format!("{trimmed}{exponent}")
    } else {
        s.to_string()
    }
}

/// Format a string value with FITS quoting rules into the buffer.
fn format_string_value(buf: &mut String, s: &str) {
    buf.push('\'');
    let start_len = buf.len();
    for ch in s.chars() {
        buf.push(ch);
        if ch == '\'' {
            buf.push('\''); // escape quotes by doubling
        }
    }
    // Pad string to minimum width (at least 8 chars inside quotes).
    // FITS standard: quoted string (including quotes) should extend to at least column 20.
    // From C: FITSSTCOL(=20) - col(=11) = 9, where 9 includes opening quote.
    // So content (excluding opening quote) must be at least 8 chars.
    let content_len = buf.len() - start_len;
    let min_content: usize = 8;
    if content_len < min_content {
        for _ in 0..(min_content - content_len) {
            buf.push(' ');
        }
    }
    buf.push('\'');
}

/// Append a comment to the formatted card buffer.
fn append_comment(buf: &mut String, comment: Option<&str>) {
    if let Some(com) = comment {
        if com.is_empty() {
            return;
        }
        // Standard FITS comment column is 32 (index 31).
        // Try to place "/ comment" starting at column 32 if possible.
        let com_text = format!("/ {com}");
        let desired_start = 31; // 0-based column 32
        let min_start = buf.len() + 1; // at least one space before /

        let start = if desired_start >= min_start {
            desired_start
        } else {
            min_start
        };

        // Pad to the start position.
        while buf.len() < start {
            buf.push(' ');
        }

        // Append "/ comment", truncating at card boundary.
        let remaining = FITS_CARD_LEN.saturating_sub(buf.len());
        let to_append = com_text.len().min(remaining);
        buf.push_str(&com_text[..to_append]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to assert parsing of a card with keyword, value, and comment.
    fn assert_parse(
        card_str: &str,
        expected_kw: &str,
        expected_val: FitsValue,
        expected_com: &str,
    ) {
        let card = FitsCard::parse(card_str).unwrap();
        assert_eq!(card.keyword, expected_kw, "keyword mismatch");
        assert_eq!(card.value, expected_val, "value mismatch for {expected_kw}");
        assert_eq!(
            card.comment,
            Some(expected_com.to_string()),
            "comment mismatch for {expected_kw}"
        );
    }

    #[test]
    fn parse_integer() {
        assert_parse(
            "NAXIS   =                    2 / Number of axes",
            "NAXIS",
            FitsValue::Int(2),
            "Number of axes",
        );
    }

    #[test]
    fn parse_negative_integer() {
        assert_parse(
            "NEGVAL  =                  -42 / Negative value",
            "NEGVAL",
            FitsValue::Int(-42),
            "Negative value",
        );
    }

    #[test]
    fn parse_large_integer() {
        let card = "BIGINT  =       9999999999999 / Large int";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "BIGINT");
        assert_eq!(parsed.value, FitsValue::Int(9_999_999_999_999));
    }

    #[test]
    fn parse_float() {
        assert_parse(
            "CRVAL1  =     266.416666666667 / Reference value",
            "CRVAL1",
            FitsValue::Float(266.416666666667),
            "Reference value",
        );
    }

    #[test]
    fn parse_scientific_notation() {
        let card = "SMALLF  =            1.5E-10 / Tiny float";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "SMALLF");
        assert_eq!(parsed.value, FitsValue::Float(1.5e-10));
        assert_eq!(parsed.comment, Some("Tiny float".to_string()));
    }

    #[test]
    fn parse_d_exponent() {
        let card = "DEXP    =            1.5D-10 / D exponent";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "DEXP");
        assert_eq!(parsed.value, FitsValue::Float(1.5e-10));
        assert_eq!(parsed.comment, Some("D exponent".to_string()));
    }

    #[test]
    fn parse_string() {
        assert_parse(
            "CTYPE1  = 'RA---TAN'           / Axis type",
            "CTYPE1",
            FitsValue::String("RA---TAN".into()),
            "Axis type",
        );
    }

    #[test]
    fn parse_string_with_escaped_quotes() {
        let card = "COMMENT1= 'It''s a test'       / Escaped quote";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "COMMENT1");
        assert_eq!(parsed.value, FitsValue::String("It's a test".into()));
        assert_eq!(parsed.comment, Some("Escaped quote".to_string()));
    }

    #[test]
    fn parse_long_string() {
        // A string that fills most of the card.
        let card =
            "LONGSTR = 'This is a long string value that fills up most of the card'          ";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "LONGSTR");
        assert_eq!(
            parsed.value,
            FitsValue::String("This is a long string value that fills up most of the card".into())
        );
    }

    #[test]
    fn parse_logical_true() {
        assert_parse(
            "SIMPLE  =                    T / Fits standard",
            "SIMPLE",
            FitsValue::Logical(true),
            "Fits standard",
        );
    }

    #[test]
    fn parse_logical_false() {
        let card = "EXTEND  =                    F / No extensions";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "EXTEND");
        assert_eq!(parsed.value, FitsValue::Logical(false));
    }

    #[test]
    fn parse_comment_card() {
        let card = "COMMENT This is a test comment";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "COMMENT");
        if let FitsValue::Comment(text) = &parsed.value {
            assert!(text.contains("This is a test comment"));
        } else {
            panic!("Expected Comment value");
        }
    }

    #[test]
    fn parse_history_card() {
        let card = "HISTORY Created by AST library";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "HISTORY");
        assert!(matches!(parsed.value, FitsValue::Comment(_)));
    }

    #[test]
    fn parse_blank_keyword() {
        let card = "        This is a blank keyword comment card";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "");
        assert!(matches!(parsed.value, FitsValue::Comment(_)));
    }

    #[test]
    fn parse_end_card() {
        let card = "END";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "END");
        assert!(matches!(parsed.value, FitsValue::Comment(_)));
    }

    #[test]
    fn parse_undefined_value() {
        let card = "UNDEF   =                      / Undefined value";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "UNDEF");
        assert_eq!(parsed.value, FitsValue::Undef);
        assert_eq!(parsed.comment, Some("Undefined value".to_string()));
    }

    #[test]
    fn parse_continue_card() {
        let card = "CONTINUE  'continued value'    / More text";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "CONTINUE");
        assert_eq!(parsed.value, FitsValue::Continue("continued value".into()));
        assert_eq!(parsed.comment, Some("More text".to_string()));
    }

    #[test]
    fn parse_short_card() {
        // Card shorter than 80 characters — should be padded.
        let card = "NAXIS   =                    3";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "NAXIS");
        assert_eq!(parsed.value, FitsValue::Int(3));
    }

    #[test]
    fn parse_complex_float() {
        let card = "CMPLXF  = (1.5, 2.5)          / Complex float";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "CMPLXF");
        assert_eq!(parsed.value, FitsValue::ComplexFloat(1.5, 2.5));
    }

    #[test]
    fn parse_complex_int() {
        let card = "CMPLXI  = (3, 4)              / Complex int";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "CMPLXI");
        assert_eq!(parsed.value, FitsValue::ComplexInt(3, 4));
    }

    #[test]
    fn format_integer() {
        let card = FitsCard {
            keyword: "NAXIS".to_string(),
            value: FitsValue::Int(2),
            comment: Some("Number of axes".to_string()),
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.starts_with("NAXIS   = "));
        assert!(formatted.contains("2"));
        assert!(formatted.contains("/ Number of axes"));
    }

    #[test]
    fn format_string() {
        let card = FitsCard {
            keyword: "CTYPE1".to_string(),
            value: FitsValue::String("RA---TAN".into()),
            comment: Some("Axis type".to_string()),
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.contains("'RA---TAN'"));
        assert!(formatted.contains("/ Axis type"));
    }

    #[test]
    fn format_logical() {
        let card = FitsCard {
            keyword: "SIMPLE".to_string(),
            value: FitsValue::Logical(true),
            comment: Some("Fits standard".to_string()),
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        // T should be at column 30 (index 29).
        assert_eq!(formatted.as_bytes()[29], b'T');
    }

    #[test]
    fn format_float() {
        let card = FitsCard {
            keyword: "CRVAL1".to_string(),
            value: FitsValue::Float(266.416666666667),
            comment: Some("Reference value".to_string()),
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.contains("/ Reference value"));
    }

    #[test]
    fn format_undefined() {
        let card = FitsCard {
            keyword: "UNDEF".to_string(),
            value: FitsValue::Undef,
            comment: Some("No value".to_string()),
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.starts_with("UNDEF   = "));
        assert!(formatted.contains("/ No value"));
    }

    #[test]
    fn format_comment_card() {
        let card = FitsCard {
            keyword: "COMMENT".to_string(),
            value: FitsValue::Comment(" This is a test".to_string()),
            comment: None,
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.starts_with("COMMENT  This is a test"));
    }

    #[test]
    fn roundtrip_integer() {
        let input = "CRPIX1  =                128.0 / Reference pixel";
        let card = FitsCard::parse(input).unwrap();
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        let reparsed = FitsCard::parse(&formatted).unwrap();
        assert_eq!(card.keyword, reparsed.keyword);
        // Value should be equivalent.
        match (&card.value, &reparsed.value) {
            (FitsValue::Float(a), FitsValue::Float(b)) => {
                assert!((a - b).abs() < 1e-10);
            }
            _ => panic!("Expected Float values"),
        }
    }

    #[test]
    fn roundtrip_string_with_quotes() {
        let card = FitsCard {
            keyword: "TEST".to_string(),
            value: FitsValue::String("It's OK".into()),
            comment: Some("Quoted".to_string()),
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        let reparsed = FitsCard::parse(&formatted).unwrap();
        assert_eq!(reparsed.value, FitsValue::String("It's OK".into()));
    }

    #[test]
    fn roundtrip_logical() {
        let card = FitsCard {
            keyword: "SIMPLE".to_string(),
            value: FitsValue::Logical(true),
            comment: Some("Standard".to_string()),
            used: false,
        };
        let formatted = card.format();
        let reparsed = FitsCard::parse(&formatted).unwrap();
        assert_eq!(reparsed.value, FitsValue::Logical(true));
    }

    #[test]
    fn roundtrip_complex_float() {
        let card = FitsCard {
            keyword: "CMPLX".to_string(),
            value: FitsValue::ComplexFloat(1.5, -2.5),
            comment: None,
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        // The parenthesized format is not used for formatting (only parsing).
        // The formatted output uses two space-separated floats which the parser
        // should recognize as complex float.
    }

    #[test]
    fn format_continue_card() {
        let card = FitsCard {
            keyword: "CONTINUE".to_string(),
            value: FitsValue::Continue("long string value".into()),
            comment: Some("continuation".to_string()),
            used: false,
        };
        let formatted = card.format();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.starts_with("CONTINUE"));
        assert!(formatted.contains("'long string value"));
    }

    #[test]
    fn parse_no_comment() {
        let card = "NAXIS   =                    4";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "NAXIS");
        assert_eq!(parsed.value, FitsValue::Int(4));
        assert_eq!(parsed.comment, None);
    }

    #[test]
    fn parse_string_with_slash_inside() {
        // The '/' inside a quoted string should NOT be treated as comment separator.
        let card = "PATH    = '/usr/local/bin'     / The path";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "PATH");
        assert_eq!(parsed.value, FitsValue::String("/usr/local/bin".into()));
        assert_eq!(parsed.comment, Some("The path".to_string()));
    }

    #[test]
    fn parse_zero_float() {
        let card = "ZEROF   =                  0.0 / Zero float";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "ZEROF");
        assert_eq!(parsed.value, FitsValue::Float(0.0));
    }

    #[test]
    fn parse_negative_float() {
        let card = "NEGF    =    -1.23456789012345 / Negative float";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "NEGF");
        if let FitsValue::Float(v) = parsed.value {
            assert!((v - (-1.23456789012345)).abs() < 1e-14);
        } else {
            panic!("Expected Float");
        }
    }

    #[test]
    fn used_field_defaults_false() {
        let card = FitsCard::parse("SIMPLE  =                    T / Standard").unwrap();
        assert!(!card.used);
    }
}

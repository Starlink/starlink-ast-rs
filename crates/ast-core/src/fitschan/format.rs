//! FITS-specific double-precision number formatter.
//!
//! Implements formatting rules for floating-point values in FITS header
//! cards, producing compact representations that preserve the specified
//! number of significant figures.

/// Format a double for a FITS card value field.
///
/// Produces a compact string representation with the specified number of
/// significant figures. Trailing zeros after the decimal point are trimmed,
/// and the result is right-justified in a 20-character field (standard FITS
/// value width from columns 11-30).
///
/// Unlike the standard `%G` formatter, this applies FITS-specific rules:
/// - Uses fixed notation when the exponent is in [-4, precision-1]
/// - Uses scientific notation (E) otherwise
/// - Trims trailing zeros while preserving at least one decimal digit
/// - Ensures the result is valid FITS
///
/// # Arguments
/// * `value` - The value to format
/// * `digits` - Number of significant figures (typically 8-17)
pub fn format_fits_double(value: f64, digits: usize) -> String {
    let digits = digits.clamp(1, 17);
    let formatted = format_significant(value, digits);

    // Right-justify in a 20-character field.
    if formatted.len() < 20 {
        format!("{:>20}", formatted)
    } else {
        formatted
    }
}

/// Format a double with the specified number of significant figures.
///
/// Applies the C library's `round_fstring` post-processing with the
/// default `fitsrnd = 10` (10 leading significant digits protected
/// from rounding) — long sequences of trailing 0's or 9's beyond that
/// limit get rounded out. Use [`format_significant_rounded`] to specify
/// a different `fitsrnd`.
pub fn format_significant(value: f64, digits: usize) -> String {
    format_significant_rounded(value, digits, 10)
}

/// Like [`format_significant`] but with a caller-controlled
/// `fitsrnd` (the C library's `FitsRounding` attribute — number of
/// leading significant digits to protect from sequence-rounding).
///
/// `fitsrnd < 1` is treated as 1, matching fitschan.c:26354-26355.
pub fn format_significant_rounded(value: f64, digits: usize, fitsrnd: usize) -> String {
    let digits = digits.clamp(1, 17);

    if value == 0.0 {
        return "0.0".to_string();
    }

    if !value.is_finite() {
        // FITS doesn't support infinity/NaN, but format something.
        return format!("{value}");
    }

    let abs_v = value.abs();
    let exp = abs_v.log10().floor() as i32;

    // The C library uses %G semantics: fixed when exp in [-4, prec-1].
    // Special case from C's EncodeFloat: negative values between -0.001
    // and -0.0001 are forced to scientific notation because the fixed form
    // is one character longer than exponential, which matters at the FITS
    // 20-character field limit with the default 15-digit precision.
    let force_scientific = value > -0.001 && value < -0.0001;
    let use_fixed = !force_scientific && exp >= -4 && exp < (digits as i32);

    let initial = if use_fixed {
        let decimal_places = ((digits as i32) - (exp + 1)).max(1) as usize;
        let s = format!("{value:.decimal_places$}");
        trim_trailing_zeros(&s)
    } else {
        let mantissa_decimals = (digits - 1).max(1);
        let s = format!("{value:.mantissa_decimals$E}");
        let s = trim_trailing_zeros_exp(&s);
        c_style_exponent(&s)
    };
    // C's order is sprintf -> CheckZero -> strip the exponent's leading zero
    // (fitschan.c:9847, 9852, 9858), so the rounding runs on the two-digit
    // exponent form and the strip comes last.
    strip_exponent_leading_zero(&round_fstring(&initial, fitsrnd))
}

/// Round out long sequences of adjacent 0's or 9's in a formatted
/// floating-point string. Mirrors fitschan.c::RoundFString (line 26266+).
///
/// `fitsrnd` is the number of leading significant digits to protect
/// from rounding (treated as 1 if 0). A run of `NSEQ = 4` or more
/// adjacent 0's or 9's that ends beyond the protected zone gets
/// collapsed: a 9-run rounds up (carry-propagating into earlier digits,
/// inserting a leading 1 if the carry reaches the front), and a 0-run
/// is left in place but any trailing digits beyond it are zeroed.
/// Trailing zeros after a decimal point are trimmed (keeping at least
/// one).
pub(crate) fn round_fstring(text: &str, fitsrnd: usize) -> String {
    const NSEQ: usize = 4;
    let fitsrnd = fitsrnd.max(1);

    // Split into mantissa + exponent (if any).
    let (mantissa, exponent) = match text.find(['E', 'e']) {
        Some(i) => (&text[..i], &text[i..]),
        None => (text, ""),
    };

    // Walk the mantissa as bytes; we'll edit in-place. Add a leading
    // sentinel space so that a final carry-out has a place to land.
    let mut buf: Vec<u8> = std::iter::once(b' ').chain(mantissa.bytes()).collect();
    let n = buf.len();

    // Locate the decimal point and the rounding-protection cutoff
    // `prnd` (index of the first digit at which rounding may occur:
    // i.e. the `fitsrnd`-th significant digit from the left).
    let mut dot = None;
    let mut prnd = None;
    let mut started = false;
    let mut nsig = 0usize;
    for (i, &b) in buf.iter().enumerate() {
        if b == b'.' {
            dot = Some(i);
        } else if b.is_ascii_digit() {
            if b != b'0' {
                started = true;
            }
            if started {
                nsig += 1;
                if nsig == fitsrnd && prnd.is_none() {
                    prnd = Some(i);
                }
            }
        }
    }

    // Nothing to do if no significant digits or `fitsrnd` covers the
    // whole number (no rounding can occur past the protected zone).
    if !started {
        return mantissa
            .trim_start()
            .to_string()
            .replace(|c: char| c.is_ascii_digit(), "0");
    }
    let prnd = match prnd {
        Some(p) => p,
        None => return text.to_string(),
    };

    // Walk backward over the digits, tracking 9-runs and 0-runs.
    let mut seq9: Option<usize> = None;
    let mut seq0: Option<usize> = None;
    let mut seqlen = 0usize;
    let mut i = n;
    while i > 0 {
        i -= 1;
        let b = buf[i];

        if let Some(start) = seq9 {
            if b == b'9' {
                seqlen += 1;
            } else if b.is_ascii_digit() {
                if start > prnd && seqlen >= NSEQ {
                    // Replace digits at-or-after the run with 0; bump *c.
                    for byte in &mut buf[(i + 1)..n] {
                        if byte.is_ascii_digit() {
                            *byte = b'0';
                        }
                    }
                    buf[i] += 1;
                }
                match buf[i] {
                    b'9' => {
                        seq9 = Some(i);
                        seq0 = None;
                        seqlen = 1;
                    }
                    b'0' => {
                        seq0 = Some(i);
                        seq9 = None;
                        seqlen = 1;
                    }
                    _ => {
                        seq9 = None;
                        seqlen = 0;
                    }
                }
            }
        } else if let Some(start) = seq0 {
            if b == b'0' {
                seqlen += 1;
            } else if b.is_ascii_digit() {
                if start > prnd && seqlen >= NSEQ {
                    for byte in &mut buf[(i + 1)..n] {
                        if byte.is_ascii_digit() {
                            *byte = b'0';
                        }
                    }
                }
                if buf[i] == b'9' {
                    seq9 = Some(i);
                    seq0 = None;
                    seqlen = 1;
                } else {
                    seq0 = None;
                    seqlen = 0;
                }
            }
        } else if i < prnd {
            // Walked past the protection cutoff with no active run.
            break;
        } else if b == b'9' {
            seq9 = Some(i);
            seqlen = 1;
        } else if b == b'0' {
            seq0 = Some(i);
            seqlen = 1;
        }
    }

    // If a 9-run runs all the way to the front of the number, collapse
    // and prepend "1" (carry-out). The leading sentinel space gives us
    // room for that "1" without resizing.
    if let Some(start) = seq9
        && start > prnd
        && seqlen >= NSEQ
    {
        let mut first = true;
        for k in 0..n {
            if buf[k].is_ascii_digit() {
                buf[k] = b'0';
                if first {
                    // Shift any sign characters one slot left so we
                    // can place '1' just before this zero.
                    let mut e = 1usize;
                    while e < k {
                        buf[e - 1] = buf[e];
                        e += 1;
                    }
                    buf[k - 1] = b'1';
                    first = false;
                }
            }
        }
    }

    // Strip trailing zeros after a decimal point (keep one), but only
    // when there was a decimal point in the input. Mirrors C's
    // post-rounding cleanup at fitschan.c:26550-26562.
    if let Some(dot_idx) = dot {
        let mut end = n;
        while end > dot_idx + 1 {
            let prev = buf[end - 1];
            if prev == b'0' || prev == b' ' {
                end -= 1;
            } else {
                break;
            }
        }
        // Keep one zero immediately after the decimal point.
        if end == dot_idx + 1 && end < n {
            buf[end] = b'0';
            end += 1;
        }
        buf.truncate(end);
    }

    // Drop the leading sentinel space if it survived.
    let mantissa_out = if buf.first() == Some(&b' ') {
        std::str::from_utf8(&buf[1..]).unwrap_or("").to_string()
    } else {
        std::str::from_utf8(&buf).unwrap_or("").to_string()
    };
    format!("{mantissa_out}{exponent}")
}

/// Trim trailing zeros from a fixed-point formatted number, keeping at least
/// one digit after the decimal point.
fn trim_trailing_zeros(s: &str) -> String {
    if let Some(dot_pos) = s.find('.') {
        let trimmed = s.trim_end_matches('0');
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

/// Give the exponent the shape C's `%E` produces: an explicit sign and at
/// least two digits. Rust's `{:E}` omits `+` on positive exponents and never
/// zero-pads, so both have to be applied here for the subsequent
/// leading-zero strip to see the same text C's `EncodeFloat` does.
fn c_style_exponent(s: &str) -> String {
    let Some(e_pos) = s.find(['E', 'e']) else {
        return s.to_string();
    };
    let after_e = &s[e_pos + 1..];
    let (sign, digits) = match after_e.strip_prefix('-') {
        Some(d) => ('-', d),
        None => ('+', after_e.strip_prefix('+').unwrap_or(after_e)),
    };
    format!("{}E{sign}{digits:0>2}", &s[..e_pos])
}

/// Drop a redundant leading zero from a two-digit exponent, as C's
/// `EncodeFloat` does (fitschan.c:9875-9894).
///
/// C works in a fixed-width buffer: when there is a leading pad space it
/// absorbs the removal by shuffling the prefix one place right, and when there
/// is none it closes the gap and lets the string shorten. Either way the value
/// ends up right-justified in the field, so on this side — where the caller
/// pads to 20 columns afterwards — removing the zero is the whole rule.
///
/// Only one zero is ever removed: C matches `E-0`, `E+0` or `E0`, so a
/// three-digit exponent (which has no leading zero) is left alone.
fn strip_exponent_leading_zero(s: &str) -> String {
    let Some(e_pos) = s.find(['E', 'e']) else {
        return s.to_string();
    };
    let after_e = &s[e_pos + 1..];
    let sign_len = usize::from(after_e.starts_with(['+', '-']));
    let digits = &after_e[sign_len..];
    if digits.len() != 2 || !digits.starts_with('0') {
        return s.to_string();
    }
    format!("{}{}", &s[..e_pos + 1 + sign_len], &digits[1..])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_zero() {
        let s = format_fits_double(0.0, 8);
        assert_eq!(s.trim(), "0.0");
        assert_eq!(s.len(), 20); // right-justified in 20 chars
    }

    #[test]
    fn format_integer_like() {
        let s = format_significant(1.0, 8);
        assert_eq!(s, "1.0");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn format_simple_float() {
        let s = format_significant(3.14159265, 8);
        // 8 significant figures of pi: 3.1415927 (rounded)
        assert!(s.starts_with("3.141592"));
    }

    #[test]
    fn format_large_number() {
        let s = format_significant(1.23456789e+15, 8);
        // Should use scientific notation (exp=15, >= digits=8).
        assert!(s.contains('E'));
        // 8 significant figures: 1.2345679 (rounded from 1.23456789).
        assert!(s.starts_with("1.2345679"));
    }

    #[test]
    fn format_small_number() {
        let s = format_significant(1.5e-10, 8);
        // Should use scientific notation.
        assert!(s.contains('E'));
        assert!(s.starts_with("1.5E"));
    }

    #[test]
    fn format_negative() {
        let s = format_significant(-273.15, 6);
        assert!(s.starts_with("-273.15"));
    }

    #[test]
    fn format_fits_width() {
        // Result should be right-justified in 20 chars.
        let s = format_fits_double(42.0, 8);
        assert_eq!(s.len(), 20);
        assert!(s.starts_with(' '));
        assert!(s.trim() == "42.0");
    }

    #[test]
    fn format_preserves_significant_figures() {
        // Check that 8 significant figures are preserved for a typical WCS value.
        let val = 266.41666666666663;
        let s = format_significant(val, 15);
        let reparsed: f64 = s.parse().unwrap();
        // Should be equal to within 15 significant figures.
        let rel_err = ((val - reparsed) / val).abs();
        assert!(rel_err < 1e-14, "relative error too large: {rel_err}");
    }

    #[test]
    fn format_no_trailing_zeros() {
        let s = format_significant(1.5, 8);
        assert_eq!(s, "1.5");

        let s = format_significant(100.0, 8);
        assert_eq!(s, "100.0");
    }

    #[test]
    fn format_boundary_cases() {
        // Value near the fixed/scientific boundary.
        let s = format_significant(0.0001, 4);
        // exp = -4, positive, so stays fixed.
        assert!(s.contains("0.0001"));

        let s = format_significant(0.00001, 4);
        // exp = -5, which is < -4 so should be scientific.
        assert!(s.contains('E'));
    }

    #[test]
    fn format_negative_special_case() {
        // Negative values between -0.001 and -0.0001 use scientific notation
        // (matches C library's EncodeFloat special case).
        let s = format_significant(-0.00083332205, 8);
        assert_eq!(s, "-8.3332205E-4");

        let s = format_significant(-0.0002778, 4);
        assert_eq!(s, "-2.778E-4");

        // Positive value in same range stays fixed.
        let s = format_significant(0.00083332205, 8);
        assert_eq!(s, "0.00083332205");
    }

    #[test]
    fn format_explicit_exponent_sign() {
        // Positive exponents must have explicit + sign.
        let s = format_significant(2.3635893e8, 8);
        assert_eq!(s, "2.3635893E+8");

        let s = format_significant(1.5e10, 8);
        assert!(s.contains("E+"), "expected E+ in '{s}'");
    }

    /// Removing the exponent's redundant leading zero leaves no trace beyond
    /// the missing digit: the value is right-justified by the caller, so it
    /// never gains a compensating space (fitschan.c:9885-9893). A one- or
    /// three-digit exponent has nothing to strip and is left alone.
    #[test]
    fn exponent_zero_strip_leaves_no_padding() {
        for (value, digits) in [(-1.7453292519943296E-4, 17), (-7.67E-9, 3), (2.5E-5, 8)] {
            let s = format_significant(value, digits);
            assert!(!s.starts_with(' '), "unexpected leading space in '{s}'");
            assert_eq!(s.len() + 1, c_style_exponent(&s).len());
        }
        for (value, digits) in [(1.534E-10, 4), (1.5E10, 8), (2.5E-100, 8)] {
            let s = format_significant(value, digits);
            assert!(!s.starts_with(' '), "unexpected leading space in '{s}'");
            assert_eq!(s, c_style_exponent(&s));
        }
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn format_round_trip() {
        // Various values should round-trip through format+parse.
        let values = [
            1.0,
            -1.0,
            3.14159265358979,
            2.99792458e8,
            6.626e-34,
            1e-4,
            1e15,
            -0.001,
        ];
        for &val in &values {
            let s = format_significant(val, 15);
            let reparsed: f64 = s.parse().unwrap();
            let rel_err = if val == 0.0 {
                reparsed.abs()
            } else {
                ((val - reparsed) / val).abs()
            };
            assert!(
                rel_err < 1e-14,
                "round-trip failed for {val}: got {reparsed}, rel_err={rel_err}"
            );
        }
    }

    #[test]
    fn round_fstring_collapses_long_nines_with_low_protection() {
        // 0.99998719 with fitsrnd=1: the leading "9999" is a 4-nine
        // run beyond the protection limit and gets rounded to 1.0.
        // Mirrors C's RoundFString output for the timj.fits-pc fixture.
        let s = format_significant_rounded(0.99998719, 8, 1);
        assert_eq!(s, "1.0");
    }

    #[test]
    fn round_fstring_preserves_value_with_default_protection() {
        // With the default fitsrnd=10 (>= digit count), no sequence
        // can extend past the protection zone — value is preserved.
        let s = format_significant_rounded(0.99998719, 8, 10);
        assert_eq!(s, "0.99998719");
    }

    #[test]
    fn round_fstring_short_runs_unchanged() {
        // Three adjacent 9's are below the NSEQ=4 threshold.
        let s = format_significant_rounded(0.99987, 5, 1);
        assert_eq!(s, "0.99987");
    }

    #[test]
    fn round_fstring_carry_propagation_to_integer_digit() {
        // 99.99996 with fitsrnd=1: trailing "9999" runs to within the
        // first significant digit; carry propagates and we get 100.0.
        let s = format_significant_rounded(99.99996, 7, 1);
        assert_eq!(s, "100.0");
    }
}

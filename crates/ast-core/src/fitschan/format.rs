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
/// This is the unpadded version, useful for testing or when the FITS
/// field-width padding is not desired.
pub fn format_significant(value: f64, digits: usize) -> String {
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

    if use_fixed {
        let decimal_places = ((digits as i32) - (exp + 1)).max(1) as usize;
        let s = format!("{value:.decimal_places$}");
        trim_trailing_zeros(&s)
    } else {
        let mantissa_decimals = (digits - 1).max(1);
        let s = format!("{value:.mantissa_decimals$E}");
        let s = trim_trailing_zeros_exp(&s);
        ensure_exponent_sign(&s)
    }
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

/// Ensure the exponent has an explicit sign character (E+N or E-N).
/// C's %E always includes the sign; Rust's {:E} omits + for positive exponents.
fn ensure_exponent_sign(s: &str) -> String {
    if let Some(e_pos) = s.find('E').or_else(|| s.find('e')) {
        let after_e = &s[e_pos + 1..];
        if !after_e.starts_with('+') && !after_e.starts_with('-') {
            format!("{}E+{}", &s[..e_pos], after_e)
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    }
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

    #[test]
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
}

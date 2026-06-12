//! Port of `axis.c`'s `ParseAxisFormat` and the numeric-formatting
//! half of `AxisFormat`. Turns an Axis `Format` string plus a `Digits`
//! precision into the formatted coordinate string, reproducing C
//! `printf` semantics for the `%G` / `%E` / `%f` / `%d` subset that
//! AST uses. The default format is `"%1.<Digits>G"`.

// `format_value` is the public entry point; the lower-level primitives
// it composes are exercised through it.
#![allow(dead_code)]

/// Format `value` like C `printf("%1.<prec>G", value)`: `prec`
/// significant digits, `%E` notation when the decimal exponent is
/// `< -4` or `>= prec`, otherwise `%f` notation, with trailing zeros
/// removed and the exponent printed as `E±dd` (>= 2 digits). Mirrors
/// the C library / glibc `%G` conversion (axis.c:626 formats via this).
pub(crate) fn format_g(value: f64, prec: usize) -> String {
    let prec = prec.max(1);
    if value == 0.0 {
        return "0".to_string();
    }
    if !value.is_finite() {
        // glibc prints "inf"/"nan"; AST never formats these through a
        // good path, but stay deterministic.
        return if value.is_nan() {
            "nan".to_string()
        } else if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }

    // The decimal exponent that C uses to choose between %E and %f is the
    // one of the value after rounding to `prec` significant digits, so
    // derive it from the %E rendering rather than a raw log10 (which can
    // be off by one near a power of ten, e.g. 9.999999e6).
    let raw = format!("{value:.*e}", prec - 1);
    let exp: i32 = raw
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .expect("exp form");
    if exp < -4 || exp >= prec as i32 {
        format_e(value, prec - 1, true)
    } else {
        // Fixed notation: prec - 1 - exp fractional digits.
        let frac = (prec as i32 - 1 - exp).max(0) as usize;
        let s = format!("{value:.frac$}");
        trim_g_zeros(&s)
    }
}

/// Format like C `printf("%.<mant>E")`: one leading digit, `mant`
/// mantissa fraction digits, exponent `E±dd`. When `strip` is true
/// (the `%G` caller), trailing mantissa zeros are removed.
fn format_e(value: f64, mant: usize, strip: bool) -> String {
    // Rust's `{:e}` gives e.g. "1.234568e3" (lowercase, no sign, no
    // zero-pad). Re-render into C `%E` form.
    let raw = format!("{value:.mant$e}");
    let (mantissa, exp_str) = raw.split_once('e').expect("exp form");
    let exp: i32 = exp_str.parse().expect("exp int");
    let mantissa = if strip {
        trim_g_zeros(mantissa)
    } else {
        mantissa.to_string()
    };
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}E{sign}{:02}", exp.abs())
}

/// Remove trailing zeros after a decimal point, then a trailing bare
/// decimal point — the `%G` no-`#` behaviour. Leaves integers and
/// strings without a `.` untouched.
fn trim_g_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    let t = t.strip_suffix('.').unwrap_or(t);
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g_matches_c_examples() {
        // Verified against C printf("%1.7G", x).
        assert_eq!(format_g(0.0, 7), "0");
        assert_eq!(format_g(1234.5678, 7), "1234.568");
        assert_eq!(format_g(0.00012345678, 7), "0.0001234568");
        assert_eq!(format_g(123456789.0, 7), "1.234568E+08");
        assert_eq!(format_g(-9.5, 7), "-9.5");
        assert_eq!(format_g(1.0, 7), "1");
        assert_eq!(format_g(9.999999e6, 7), "9999999");
        assert_eq!(format_g(1e-5, 7), "1E-05");
        assert_eq!(format_g(0.1, 7), "0.1");
    }
}

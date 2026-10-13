//! C's formatting of floating point values in FITS header cards:
//! `EncodeFloat` and the `CheckZero` / `RoundFString` clean-up it applies.

use crate::frames::axis_format::sprintf_double;

/// The width of a value right-justified to column 30, C's
/// `FITSRLCOL - FITSNAMLEN - 2`.
pub(crate) const VALUE_WIDTH: usize = 20;

/// The columns available to a value starting in column 11, C's
/// `AST__FITSCHAN_FITSCARDLEN - col + 1`.
pub(crate) const VALUE_MAXWIDTH: usize = 70;

/// A floating point value as C's `EncodeFloat` formats it for the value of
/// a header card (right-justified to column 30), with FitsDigits `digits`
/// and FitsRounding `fitsrnd`, or `None` if it cannot be formatted in the
/// card (an infinity or NaN, or a value too wide).
pub fn encode_float(value: f64, digits: i32, fitsrnd: usize) -> Option<String> {
    encode_float_in(value, digits, VALUE_WIDTH, VALUE_MAXWIDTH, fitsrnd)
}

/// C's `EncodeFloat`: `value` right-justified in `width` columns, using no
/// more than `maxwidth`, or `None` if it cannot be.
///
/// The value is written with `%*.*G` to `|digits|` significant digits
/// (`%*.*E` for one between -0.001 and -0.0001, which is one character
/// shorter), then `CheckZero` cleans it, and a leading zero in the exponent
/// is removed. If that is wider than `width` and `digits` is negative, the
/// digits are reduced by the excess and the value written again. Finally a
/// decimal point is added if the value has none.
pub(crate) fn encode_float_in(
    value: f64,
    digits: i32,
    width: usize,
    maxwidth: usize,
    fitsrnd: usize,
) -> Option<String> {
    let mut ldigits = digits.abs();
    let mut buf: Vec<u8>;
    loop {
        let (conv, prec) = if value > -0.001 && value < -0.0001 {
            ('E', ldigits - 1)
        } else {
            ('G', ldigits)
        };
        // A negative precision is taken by C's printf as if it were omitted.
        let fmt = if prec < 0 {
            format!("%{width}{conv}")
        } else {
            format!("%{width}.{prec}{conv}")
        };
        buf = check_zero(&sprintf_double(&fmt, value), value, width, fitsrnd).into_bytes();

        // Remove a leading zero from the exponent. A leading space absorbs
        // the change and keeps the field width; otherwise the text shortens.
        let text = String::from_utf8_lossy(&buf).into_owned();
        let w = text
            .find("E-0")
            .map(|i| i + 2)
            .or_else(|| text.find("E+0").map(|i| i + 2))
            .or_else(|| text.find("E0").map(|i| i + 1));
        if let Some(w) = w {
            buf.remove(w);
            if buf.first() == Some(&b' ') {
                buf.insert(0, b' ');
            }
        }

        let ret = buf.len();
        if ret > width && digits < 0 {
            ldigits -= (ret - width) as i32;
        } else if ret > maxwidth {
            return None;
        } else {
            break;
        }
    }

    // Ensure there is a decimal point after the leading digits.
    let first = buf.iter().position(u8::is_ascii_digit)?;
    let n = buf[first..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if buf.get(first + n) != Some(&b'.') {
        if buf.starts_with(b"  ") {
            // Shift the leading characters two places left and append ".0"
            // to the digits.
            buf.drain(..2);
            buf.splice(first - 2 + n..first - 2 + n, *b".0");
        } else if buf.first() == Some(&b' ') {
            // Shift the leading characters one place left and append ".".
            buf.remove(0);
            buf.insert(first - 1 + n, b'.');
        } else {
            // With no leading blank, the value grows by the ".".
            if buf.len() + 1 > maxwidth {
                return None;
            }
            buf.insert(first + n, b'.');
        }
    }
    Some(String::from_utf8(buf).expect("printf output is ASCII"))
}

/// C's `CheckZero`: no minus sign on a zero value, and otherwise
/// `RoundFString` on the text, which `width` is the field width of.
fn check_zero(text: &str, value: f64, width: usize, fitsrnd: usize) -> String {
    if value == 0.0 {
        let lead = text.len() - text.trim_start().len();
        let mut out = text.to_string();
        if text[lead..].starts_with('-') {
            out.replace_range(lead..=lead, " ");
        }
        return out;
    }
    round_fstring_in(text, width, fitsrnd)
}

/// C's `RoundFString` on `text`, a value right-justified in a field of
/// `width` columns: the value with long runs of 0s and 9s rounded out
/// (see [`round_fstring`]), right-justified in `width` columns, or with no
/// leading blanks if it is wider.
///
/// C keeps the value unrounded if rounding would carry a digit into a
/// value with no leading blank to take it and make it longer.
fn round_fstring_in(text: &str, width: usize, fitsrnd: usize) -> String {
    let lead = text.len() - text.trim_start().len();
    let rounded = round_fstring(&text[lead..], fitsrnd);
    let content = if lead > 0 || rounded.len() <= text.len() {
        rounded
    } else {
        text.to_string()
    };
    format!("{content:>width$}")
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

    // With no non-zero digit, C gives the simplest zero.
    if !started {
        return if dot.is_some() { "0.0" } else { "0" }.to_string();
    }
    // `prnd` is `None` when `fitsrnd` covers every significant digit, and
    // nothing can be rounded.
    if let Some(prnd) = prnd {
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
    }

    // Strip trailing zeros after a decimal point (keep one), but only
    // when there was a decimal point in the input. Mirrors C's
    // post-rounding cleanup at fitschan.c:26644-26656.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The value as `encode_float` formats it, without its leading blanks.
    fn sig(value: f64, digits: i32) -> String {
        encode_float(value, digits, 10)
            .unwrap()
            .trim_start()
            .to_string()
    }

    #[test]
    fn format_zero() {
        let s = encode_float(0.0, 8, 10).unwrap();
        assert_eq!(s.trim(), "0.0");
        assert_eq!(s.len(), 20); // right-justified in 20 chars
    }

    #[test]
    fn format_integer_like() {
        let s = sig(1.0, 8);
        assert_eq!(s, "1.0");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn format_simple_float() {
        let s = sig(3.14159265, 8);
        // 8 significant figures of pi: 3.1415927 (rounded)
        assert!(s.starts_with("3.141592"));
    }

    #[test]
    fn format_large_number() {
        let s = sig(1.23456789e+15, 8);
        // Should use scientific notation (exp=15, >= digits=8).
        assert!(s.contains('E'));
        // 8 significant figures: 1.2345679 (rounded from 1.23456789).
        assert!(s.starts_with("1.2345679"));
    }

    #[test]
    fn format_small_number() {
        let s = sig(1.5e-10, 8);
        // Should use scientific notation.
        assert!(s.contains('E'));
        assert!(s.starts_with("1.5E"));
    }

    #[test]
    fn format_negative() {
        let s = sig(-273.15, 6);
        assert!(s.starts_with("-273.15"));
    }

    #[test]
    fn format_fits_width() {
        // Result should be right-justified in 20 chars.
        let s = encode_float(42.0, 8, 10).unwrap();
        assert_eq!(s.len(), 20);
        assert!(s.starts_with(' '));
        assert!(s.trim() == "42.0");
    }

    #[test]
    fn format_preserves_significant_figures() {
        // Check that 8 significant figures are preserved for a typical WCS value.
        let val = 266.41666666666663;
        let s = sig(val, 15);
        let reparsed: f64 = s.parse().unwrap();
        // Should be equal to within 15 significant figures.
        let rel_err = ((val - reparsed) / val).abs();
        assert!(rel_err < 1e-14, "relative error too large: {rel_err}");
    }

    #[test]
    fn format_no_trailing_zeros() {
        let s = sig(1.5, 8);
        assert_eq!(s, "1.5");

        let s = sig(100.0, 8);
        assert_eq!(s, "100.0");
    }

    #[test]
    fn format_boundary_cases() {
        // Value near the fixed/scientific boundary.
        let s = sig(0.0001, 4);
        // exp = -4, positive, so stays fixed.
        assert!(s.contains("0.0001"));

        let s = sig(0.00001, 4);
        // exp = -5, which is < -4 so should be scientific.
        assert!(s.contains('E'));
    }

    #[test]
    fn format_negative_special_case() {
        // Negative values between -0.001 and -0.0001 use scientific notation
        // (matches C library's EncodeFloat special case).
        let s = sig(-0.00083332205, 8);
        assert_eq!(s, "-8.3332205E-4");

        let s = sig(-0.0002778, 4);
        assert_eq!(s, "-2.778E-4");

        // Positive value in same range stays fixed.
        let s = sig(0.00083332205, 8);
        assert_eq!(s, "0.00083332205");
    }

    #[test]
    fn format_explicit_exponent_sign() {
        // Positive exponents must have explicit + sign.
        let s = sig(2.3635893e8, 8);
        assert_eq!(s, "2.3635893E+8");

        let s = sig(1.5e10, 8);
        assert!(s.contains("E+"), "expected E+ in '{s}'");
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
            let s = sig(val, 15);
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
        let s = encode_float(0.99998719, 8, 1)
            .unwrap()
            .trim_start()
            .to_string();
        assert_eq!(s, "1.0");
    }

    #[test]
    fn round_fstring_preserves_value_with_default_protection() {
        // With the default fitsrnd=10 (>= digit count), no sequence
        // can extend past the protection zone — value is preserved.
        let s = encode_float(0.99998719, 8, 10)
            .unwrap()
            .trim_start()
            .to_string();
        assert_eq!(s, "0.99998719");
    }

    #[test]
    fn round_fstring_short_runs_unchanged() {
        // Three adjacent 9's are below the NSEQ=4 threshold.
        let s = encode_float(0.99987, 5, 1)
            .unwrap()
            .trim_start()
            .to_string();
        assert_eq!(s, "0.99987");
    }

    #[test]
    fn round_fstring_carry_propagation_to_integer_digit() {
        // 99.99996 with fitsrnd=1: trailing "9999" runs to within the
        // first significant digit; carry propagates and we get 100.0.
        let s = encode_float(99.99996, 7, 1)
            .unwrap()
            .trim_start()
            .to_string();
        assert_eq!(s, "100.0");
    }
}

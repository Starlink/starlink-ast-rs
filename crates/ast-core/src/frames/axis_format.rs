//! Port of `axis.c`'s `ParseAxisFormat` and the numeric-formatting
//! half of `AxisFormat`. Turns an Axis `Format` string plus a `Digits`
//! precision into the formatted coordinate string, reproducing C
//! `printf` semantics for the `%G` / `%E` / `%f` / `%d` subset that
//! AST uses. The default format is `"%1.<Digits>G"`.

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

/// Parsed view of an Axis `Format` string. Mirrors the flags returned
/// by C `ParseAxisFormat` (axis.c:2015) plus the resolved precision and
/// conversion code needed to format a value.
pub(crate) struct ParsedFormat {
    /// Conversion code (`G`, `g`, `E`, `e`, `f`, `F`, `d`, `i`, ...).
    pub conv: char,
    /// Explicit precision from the Format string: `Some(n)` for `.N`,
    /// and `Some(digits)` for the `.*` wildcard (which C rewrites to an
    /// explicit `digits`). `None` when the spec carries no precision
    /// field at all, in which case the caller applies the C `printf`
    /// per-conversion default (6 fraction digits for the float codes).
    /// C `ParseAxisFormat` only ever injects a precision for `.*`; a
    /// precision-less spec is passed to `sprintf` unchanged.
    pub spec_prec: Option<usize>,
    /// Format the value as `10**x` using graphical escapes. Recognised
    /// by the parser; its graphical-escape rendering is supplied by the
    /// Plot layer rather than the numeric formatter.
    #[allow(dead_code)]
    pub log: bool,
    /// Cast the value to `int` before formatting (integer conv code).
    pub integ: bool,
}

/// Parse an Axis `Format` string into a [`ParsedFormat`], substituting
/// `digits` for a `.*` precision field. Mirrors C `ParseAxisFormat`.
pub(crate) fn parse_axis_format(fmt: &str, digits: usize) -> ParsedFormat {
    let mut log = false;
    let mut integ = false;
    let mut conv = 'G';
    let mut spec_prec = None;

    if let Some(perc) = fmt.find('%') {
        let rest = &fmt[perc + 1..];

        // Flags (printf + AST '&'). Mirrors the flag-scan loop.
        let mut idx_after_flags = 0;
        for (i, c) in rest.char_indices() {
            match c {
                '-' | '+' | ' ' | '#' => {}
                '&' => log = true,
                _ => {
                    idx_after_flags = i;
                    break;
                }
            }
            idx_after_flags = i + c.len_utf8();
        }

        let tail = &rest[idx_after_flags..];

        // Precision: look for ".<digits>" or ".*". `.*` => digits.
        if let Some(dot) = tail.find('.') {
            let after = &tail[dot + 1..];
            if after.starts_with('*') {
                spec_prec = Some(digits);
            } else {
                let num: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(p) = num.parse::<usize>() {
                    spec_prec = Some(p);
                }
            }
        }

        // Conversion code: first alphabetic char in the tail.
        if let Some(c) = tail.chars().find(|c| c.is_ascii_alphabetic()) {
            conv = c;
            if matches!(c, 'd' | 'i' | 'u' | 'o' | 'x' | 'X' | 'c') {
                integ = true;
            }
        }
    }

    ParsedFormat {
        conv,
        spec_prec,
        log,
        integ,
    }
}

/// Format `value` according to an Axis `Format` string and the default
/// `digits` precision. Mirrors the numeric path of C `AxisFormat`
/// (axis.c:626): default `%1.<digits>G`, integer-cast for integer
/// conversion codes, `%G`/`%E`/`%f` rendering. The `&` log flag is
/// parsed; its graphical-escape rendering is deferred (Plot, step 10),
/// so a log format currently falls back to formatting the raw value.
pub(crate) fn format_value(fmt: &str, digits: usize, value: f64) -> String {
    // A Format string with no '%' conversion has nothing for sprintf to
    // substitute, so C passes it straight to sprintf and the literal
    // string is returned with the value ignored (axis.c:780; verified
    // with `sprintf(buf, "3", x)` -> "3"). ParseAxisFormat (axis.c:2127)
    // returns the unaltered copy in this case.
    if !fmt.contains('%') {
        return fmt.to_string();
    }
    let p = parse_axis_format(fmt, digits);
    if p.integ {
        // C `AxisFormat` (axis.c:783) casts to a 32-bit `int`:
        // `sprintf( buf, fmt, (int) x )`. For doubles outside the
        // `int` range C's `(int)` cast is undefined behaviour, so no
        // particular overflow result is guaranteed; this mirrors the
        // 32-bit width and so differs from a 64-bit cast only for
        // |value| >= 2^31, which does not occur in AST coordinate
        // formatting.
        return format!("{}", value as i32);
    }
    // For the float conversion codes, a Format string with no explicit
    // precision uses C `printf`'s default of 6 fraction digits — C
    // `ParseAxisFormat` passes a precision-less spec to `sprintf`
    // unchanged (it injects a precision only for the `.*` wildcard), so
    // `"%E"`/`"%f"` format with 6 digits, NOT `Digits`. The default
    // Frame format always carries an explicit `.Digits` precision and so
    // never reaches this branch.
    let float_prec = p.spec_prec.unwrap_or(6);
    match p.conv {
        'G' | 'g' => format_g(value, float_prec),
        'E' | 'e' => format_e(value, float_prec, false),
        'f' | 'F' => format!("{value:.*}", float_prec),
        // Unknown/unsupported code: fall back to %G with the precision.
        _ => format_g(value, float_prec),
    }
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

    #[test]
    fn parse_default_and_flags() {
        let p = parse_axis_format("%1.7G", 7);
        assert_eq!(p.spec_prec, Some(7));
        assert_eq!(p.conv, 'G');
        assert!(!p.integ && !p.log);

        // ".*" precision is replaced by Digits.
        let p = parse_axis_format("%.*G", 5);
        assert_eq!(p.spec_prec, Some(5));

        // A precision-less float spec carries no precision; the caller
        // applies C printf's default of 6 (NOT Digits).
        let p = parse_axis_format("%E", 7);
        assert_eq!(p.spec_prec, None);
        assert_eq!(p.conv, 'E');

        // Integer conversion code.
        let p = parse_axis_format("%d", 7);
        assert!(p.integ);
        assert_eq!(p.conv, 'd');

        // '&' log flag.
        let p = parse_axis_format("%&G", 7);
        assert!(p.log);
    }

    #[test]
    fn format_value_dispatch() {
        assert_eq!(format_value("%1.7G", 7, 1234.5678), "1234.568");
        assert_eq!(format_value("%d", 7, 42.9), "42");
        assert_eq!(format_value("%.2f", 7, 3.14159), "3.14");
    }

    #[test]
    fn precision_less_float_defaults_to_six() {
        // A user-set, precision-less float Format uses C printf's
        // default of 6 fraction digits, NOT Digits. Verified against
        // `printf("%E\n", 5.0)` -> "5.000000E+00" and
        // `printf("%f\n", 5.0)` -> "5.000000".
        assert_eq!(format_value("%E", 7, 5.0), "5.000000E+00");
        assert_eq!(format_value("%f", 7, 5.0), "5.000000");
        // The default/unset Frame path carries an explicit precision and
        // is unaffected: `%1.7G` still gives 7 significant figures.
        assert_eq!(format_value("%1.7G", 7, 1234.5678), "1234.568");
    }

    #[test]
    fn integer_cast_is_32_bit() {
        // C casts to a 32-bit int: `printf("%d", -3.9)` -> "-3".
        assert_eq!(format_value("%d", 7, -3.9), "-3");
        assert_eq!(format_value("%d", 7, 42.9), "42");
    }
}

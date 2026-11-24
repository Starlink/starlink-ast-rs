// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Port of `axis.c`'s `ParseAxisFormat` and `AxisFormat`, which format an
//! axis value by handing the Axis `Format` string to C's `sprintf`, and of
//! the part of C's `printf` that call needs: a format with at most one
//! conversion, given an `int` or a `double`.
//!
//! `sprintf` is called with one argument, so `ParseAxisFormat` refuses a
//! Format that asks for anything else with `AST__FMTER`: more than one
//! conversion, a `*` width or precision, a length modifier other than an
//! `l` before a floating code, or a conversion that is neither floating
//! (`eEfFgGaA`) nor integer (`diuoxXc`). `%a` and `%A` write the value in
//! hexadecimal as the macOS C library does, and the `'` flag groups no
//! digits, as in the C locale.

use crate::error::{AstError, AstResult, ErrorCode};

/// The text `AxisFormat` writes between "10" and the exponent of a log
/// Format when graphical escapes are off, which is the default (axis.c:157,
/// object.c:418); with them on it writes `crate::escapes::EXDEL`.
pub(crate) const LOG_TXT: &str = "10^";

/// The escape sequence `AxisFormat` appends to a log Format, which resets
/// the graphical attributes (axis.c:794).
const LOG_END: &str = "%+";

/// `AST__AXIS_AXISFORMAT_BUFF_LEN` (axis.h:264).
const BUFF_LEN: usize = 127;

/// A Format string as `ParseAxisFormat` (axis.c:2015) returns it, with the
/// flags it reports.
pub(crate) struct ParsedFormat {
    /// The Format to hand to `sprintf`: the `&` flag removed, with `+` and
    /// space when `&` was present, and a `.*` precision replaced by Digits.
    pub fmt: Vec<u8>,
    /// Format the value as "10^x".
    pub log: bool,
    /// With `log`, put a `+` in front of a positive value.
    pub sign: bool,
    /// With `log` and no `sign`, put a space in front of a positive value.
    pub space: bool,
    /// The conversion code is an integer one, so the value is cast to
    /// `int`.
    pub integ: bool,
}

/// Whether `b` is a C `isspace` character.
pub(crate) fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// axis.c `ValidAxisFormat`: whether `fmt`, given to `sprintf` with the
/// axis value as its only argument, asks for nothing else. Text, `%%`, and
/// at most one conversion with flags, a width and a precision given as
/// digits, and a floating or integer code, an `l` allowed before a
/// floating one.
fn valid_axis_format(fmt: &[u8]) -> bool {
    let mut nconv = 0;
    let mut i = 0;
    while let Some(off) = fmt[i..].iter().position(|&b| b == b'%') {
        i += off + 1;
        if fmt.get(i) == Some(&b'%') {
            i += 1;
            continue;
        }
        while fmt.get(i).is_some_and(|b| b"-+ #0'".contains(b)) {
            i += 1;
        }
        while fmt.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if fmt.get(i) == Some(&b'.') {
            i += 1;
            while fmt.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        if fmt.get(i) == Some(&b'l') && fmt.get(i + 1).is_some_and(|b| b"eEfFgGaA".contains(b)) {
            i += 1;
        }
        if !fmt.get(i).is_some_and(|b| b"eEfFgGaAdiuoxXc".contains(b)) {
            return false;
        }
        i += 1;
        nconv += 1;
    }
    nconv <= 1
}

/// Port of `ParseAxisFormat` (axis.c).
///
/// # Errors
///
/// [`ErrorCode::Fmter`] if the Format asks `sprintf` for anything but the
/// one axis value.
pub(crate) fn parse_axis_format(fmt0: &str, digs: i32) -> AstResult<ParsedFormat> {
    let mut log = false;
    let mut sign = false;
    let mut lspace = false;
    let mut integ = false;

    // The Format without trailing white space (`astChrLen`).
    let bytes = fmt0.as_bytes();
    let len = bytes
        .iter()
        .rposition(|&b| !is_c_space(b))
        .map_or(0, |i| i + 1);
    let mut result = bytes[..len].to_vec();

    // Note the flags after the first '%'. Only with the '&' flag is the
    // Format changed: '&', '+' and ' ' are removed, '-' and '#' kept.
    if let Some(perc) = result.iter().position(|&b| b == b'%') {
        let (mut minus, mut plus, mut space, mut hash) = (false, false, false, false);
        let mut a = perc + 1;
        while a < result.len() {
            match result[a] {
                b'-' => minus = true,
                b'+' => plus = true,
                b' ' => space = true,
                b'#' => hash = true,
                b'&' => log = true,
                _ => break,
            }
            a += 1;
        }
        if log {
            sign = plus;
            lspace = space;
            let mut out = result[..=perc].to_vec();
            if minus {
                out.push(b'-');
            }
            if hash {
                out.push(b'#');
            }
            out.extend_from_slice(&result[a..]);
            result = out;
        }
    }

    // Skip the flags and field width after the first '%'. The next
    // alphabetic character is taken as the conversion code, and a ".*"
    // precision is given the Digits value. C searches for the code without
    // stopping at the end of the string; the port stops there.
    if let Some(perc) = result.iter().position(|&b| b == b'%') {
        let mut a = perc + 1;
        while a < result.len() && matches!(result[a], b'-' | b'+' | b' ' | b'#') {
            a += 1;
        }
        while a < result.len() && result[a].is_ascii_digit() {
            a += 1;
        }
        if let Some(&c) = result[a..].iter().find(|b| b.is_ascii_alphabetic()) {
            integ = b"diuoxXc".contains(&c);
        }
        if result.get(a) == Some(&b'.') && result.get(a + 1) == Some(&b'*') {
            let mut out = result[..a].to_vec();
            out.extend_from_slice(format!(".{digs}").as_bytes());
            out.extend_from_slice(&result[a + 2..]);
            result = out;
        }
    }

    if !valid_axis_format(&result) {
        return Err(AstError::new(
            ErrorCode::Fmter,
            format!(
                "astFormat: The Format string \"{fmt0}\" cannot be used to format an axis \
                 value. It should have no more than one conversion specification, for a \
                 floating point or integer value."
            ),
        ));
    }

    Ok(ParsedFormat {
        fmt: result,
        log,
        sign,
        space: lspace,
        integ,
    })
}

/// C's `(int) x`. The conversion is undefined outside the `int` range and
/// for NaN; this saturates, as the cast instruction does on AArch64.
fn c_int(x: f64) -> i32 {
    x as i32
}

/// Port of `AxisFormat` (axis.c:637): format `value` with the Format `fmt`
/// and the axis Digits `digits`.
pub(crate) fn format_value(fmt: &str, digits: i32, value: f64) -> AstResult<String> {
    if crate::mappings::helpers::is_bad(value) {
        return Ok("<bad>".to_string());
    }
    let p = parse_axis_format(fmt, digits)?;

    // Zero is formatted normally. Otherwise a log Format gives "10^x",
    // after a sign, with x formatted by the Format.
    let log = p.log && value != 0.0;
    let mut buf: Vec<u8> = Vec::new();
    let mut x = value;
    if log {
        if p.sign {
            buf.push(b'+');
        } else if p.space {
            buf.push(b' ');
        }
        let magnitude = |v: f64| if p.integ { f64::from(c_int(v)) } else { v };
        if value > 0.0 {
            x = crate::math::log10(magnitude(value));
        } else {
            x = crate::math::log10(magnitude(-value));
            buf = vec![b'-'];
        }
        // C: the exdel delimiter when escape sequences are on.
        if crate::escapes::escapes() {
            buf.extend_from_slice(crate::escapes::EXDEL.as_bytes());
        } else {
            buf.extend_from_slice(LOG_TXT.as_bytes());
        }
        if x.abs() < 1.0e-10 {
            x = 0.0;
        }
    }

    let arg = if p.integ {
        Arg::Int(c_int(x))
    } else {
        Arg::Double(x)
    };
    let prefix = buf.len();
    let (text, conversion) = sprintf_marked(&p.fmt, arg).map_err(|e| {
        AstError::new(
            ErrorCode::Fmter,
            format!(
                "astAxisFormat(Axis): Error formatting a coordinate value of {value}: {e}. \
                 The format string was \"{}\".",
                String::from_utf8_lossy(&p.fmt)
            ),
        )
    })?;
    buf.extend(text);
    if log {
        buf.extend_from_slice(LOG_END.as_bytes());
    }
    if buf.len() > BUFF_LEN {
        return Err(AstError::new(
            ErrorCode::Fmter,
            format!(
                "astAxisFormat(Axis): Internal buffer overflow while formatting a coordinate \
                 value of {} - result exceeds {BUFF_LEN} characters.\n\
                 The format string was \"{}\".",
                sprintf_double("%1.17G", value),
                String::from_utf8_lossy(&p.fmt)
            ),
        ));
    }

    // The result is a C string, so it ends at the first NUL, which `%c`
    // writes for a zero byte.
    if let Some(nul) = buf.iter().position(|&b| b == 0) {
        buf.truncate(nul);
    }
    // `%c` writes one byte of the value, padded with spaces, which outside
    // ASCII is held as the character of that code in ISO 8859-1; the rest
    // is the Format's own text, which is UTF-8.
    let byte = match conversion {
        Some((span, b'c')) => {
            (prefix + span.start..(prefix + span.end).min(buf.len())).find(|&i| buf[i] >= 0x80)
        }
        _ => None,
    };
    let Some(at) = byte else {
        return Ok(String::from_utf8_lossy(&buf).into_owned());
    };
    let mut out = String::from_utf8_lossy(&buf[..at]).into_owned();
    out.push(char::from(buf[at]));
    out.push_str(&String::from_utf8_lossy(&buf[at + 1..]));
    Ok(out)
}

/// The one argument `AxisFormat` passes to `sprintf`.
#[derive(Clone, Copy, Debug)]
enum Arg {
    Int(i32),
    Double(f64),
}

/// A parsed conversion specification (C99 7.19.6.1).
#[derive(Default)]
struct Spec {
    minus: bool,
    plus: bool,
    space: bool,
    zero: bool,
    alt: bool,
    width: usize,
    prec: Option<usize>,
}

/// C's `sprintf( buf, fmt, value )` for a format with one floating
/// conversion, such as `"%.17g"`.
pub(crate) fn sprintf_double(fmt: &str, value: f64) -> String {
    let bytes = sprintf(fmt.as_bytes(), Arg::Double(value)).expect("a valid double format");
    String::from_utf8(bytes).expect("printf output is ASCII")
}

/// C's `sprintf( buf, fmt, arg )` for a format with at most one
/// conversion. The error is a short reason; the caller reports it.
fn sprintf(fmt: &[u8], arg: Arg) -> Result<Vec<u8>, String> {
    sprintf_marked(fmt, arg).map(|(out, _)| out)
}

/// The byte range of the output a conversion wrote, and its character.
type Conversion = (std::ops::Range<usize>, u8);

/// [`sprintf`], with the [`Conversion`] in its output, or `None` for a
/// format with no conversion.
fn sprintf_marked(fmt: &[u8], arg: Arg) -> Result<(Vec<u8>, Option<Conversion>), String> {
    let mut out = Vec::new();
    let mut conversion = None;
    let mut used = false;
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' {
            out.push(fmt[i]);
            i += 1;
            continue;
        }
        i += 1;
        if fmt.get(i) == Some(&b'%') {
            out.push(b'%');
            i += 1;
            continue;
        }

        let mut spec = Spec::default();
        while let Some(&b) = fmt.get(i) {
            match b {
                b'-' => spec.minus = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'0' => spec.zero = true,
                b'#' => spec.alt = true,
                // Groups digits by the locale, which in C's has no groups.
                b'\'' => {}
                _ => break,
            }
            i += 1;
        }
        if fmt.get(i) == Some(&b'*') {
            return Err("a * field width needs a second argument".into());
        }
        while let Some(d) = fmt.get(i).filter(|b| b.is_ascii_digit()) {
            spec.width = spec
                .width
                .saturating_mul(10)
                .saturating_add(usize::from(d - b'0'));
            i += 1;
        }
        if fmt.get(i) == Some(&b'.') {
            i += 1;
            if fmt.get(i) == Some(&b'*') {
                return Err("a * precision needs a second argument".into());
            }
            let mut p = 0usize;
            while let Some(d) = fmt.get(i).filter(|b| b.is_ascii_digit()) {
                p = p.saturating_mul(10).saturating_add(usize::from(d - b'0'));
                i += 1;
            }
            spec.prec = Some(p);
        }
        // "l" has no effect on a floating conversion; any other length
        // modifier changes the argument type.
        let long = fmt.get(i) == Some(&b'l');
        if long {
            i += 1;
        } else if fmt.get(i).is_some_and(|b| b"hLqjzt".contains(b)) {
            return Err("a length modifier changes the argument type".into());
        }
        let Some(&conv) = fmt.get(i) else {
            return Err("the format ends inside a conversion".into());
        };
        i += 1;
        if used {
            return Err("a second conversion has no argument".into());
        }
        used = true;
        let start = out.len();

        match (conv, arg) {
            (b'd' | b'i' | b'u' | b'o' | b'x' | b'X', Arg::Int(v)) if !long => {
                out.extend(format_int(&spec, conv, v));
            }
            (b'c', Arg::Int(v)) if !long => {
                if spec.zero {
                    return Err("the 0 flag is undefined for %c".into());
                }
                // The int is converted to unsigned char.
                let body = [v as u8];
                out.extend(pad(&spec, &[], &body, false));
            }
            (b'e' | b'E' | b'f' | b'F' | b'g' | b'G', Arg::Double(v)) => {
                out.extend(format_double(&spec, conv, v));
            }
            (b'a' | b'A', Arg::Double(v)) => out.extend(format_hex(&spec, conv == b'A', v)),
            _ => {
                return Err(format!(
                    "%{} does not take the {} argument",
                    conv as char,
                    match arg {
                        Arg::Int(_) => "int",
                        Arg::Double(_) => "double",
                    }
                ));
            }
        }
        conversion = Some((start..out.len(), conv));
    }
    Ok((out, conversion))
}

/// The byte offsets of the minus signs in `text`, a value formatted with the
/// plain axis Format `fmt`: each `-` its conversion writes, which is the
/// value's sign or its exponent's, and in log form the `-` before "10".
/// A `-` in the Format's own text, or in the escape sequences of log form,
/// is not one.
pub(crate) fn plain_minus_signs(fmt: &str, text: &str) -> Vec<usize> {
    // Digits fills only a `.*` precision, which writes no text of its own.
    let Ok(p) = parse_axis_format(fmt, 0) else {
        return Vec::new();
    };
    // The Format's own text either side of its conversion is the same for
    // any value, so formatting zero finds it.
    let arg = if p.integ {
        Arg::Int(0)
    } else {
        Arg::Double(0.0)
    };
    let Ok((zero, Some((span, conv)))) = sprintf_marked(&p.fmt, arg) else {
        return Vec::new();
    };
    // %c writes a byte of the value, not a number.
    if conv == b'c' {
        return Vec::new();
    }
    let (before, after) = (&zero[..span.start], &zero[span.end..]);
    let mut signs = Vec::new();
    let b = text.as_bytes();
    // Log form: an optional sign, "10" and its delimiter, the conversion of
    // the exponent, and the escape that resets the attributes.
    let mut body = 0..b.len();
    if p.log && text.ends_with(LOG_END) {
        let lead = usize::from(matches!(b.first(), Some(b'-' | b'+' | b' ')));
        for delim in [crate::escapes::EXDEL, LOG_TXT] {
            if text[lead..].starts_with(delim) {
                if b[0] == b'-' {
                    signs.push(0);
                }
                body = lead + delim.len()..b.len() - LOG_END.len();
                break;
            }
        }
    }
    let t = &b[body.clone()];
    if !t.starts_with(before) || !t.ends_with(after) || t.len() < before.len() + after.len() {
        return signs;
    }
    let conv = body.start + before.len()..body.end - after.len();
    signs.extend(conv.clone().filter(|&i| b[i] == b'-'));
    signs
}

/// Pad `prefix` (sign and radix prefix) and `body` (digits) to the field
/// width: spaces on the right with `-`, zeros after the prefix when
/// `zero_pad`, else spaces on the left.
fn pad(spec: &Spec, prefix: &[u8], body: &[u8], zero_pad: bool) -> Vec<u8> {
    let len = prefix.len() + body.len();
    let fill = spec.width.saturating_sub(len);
    let mut out = Vec::with_capacity(len + fill);
    if spec.minus {
        out.extend_from_slice(prefix);
        out.extend_from_slice(body);
        out.resize(out.len() + fill, b' ');
    } else if zero_pad {
        out.extend_from_slice(prefix);
        out.resize(out.len() + fill, b'0');
        out.extend_from_slice(body);
    } else {
        out.resize(fill, b' ');
        out.extend_from_slice(prefix);
        out.extend_from_slice(body);
    }
    out
}

/// The sign C writes in front of a signed conversion.
fn sign_prefix(spec: &Spec, negative: bool) -> &'static [u8] {
    if negative {
        b"-"
    } else if spec.plus {
        b"+"
    } else if spec.space {
        b" "
    } else {
        b""
    }
}

/// `%d`, `%i`, `%u`, `%o`, `%x` and `%X` of an `int`.
fn format_int(spec: &Spec, conv: u8, v: i32) -> Vec<u8> {
    let (negative, digits) = match conv {
        b'd' | b'i' => (v < 0, i64::from(v).unsigned_abs().to_string()),
        b'u' => (false, (v as u32).to_string()),
        b'o' => (false, format!("{:o}", v as u32)),
        b'x' => (false, format!("{:x}", v as u32)),
        _ => (false, format!("{:X}", v as u32)),
    };
    // A precision is the minimum number of digits; zero with precision 0
    // gives no digits.
    let mut body = if spec.prec == Some(0) && v == 0 {
        String::new()
    } else {
        digits
    };
    if let Some(p) = spec.prec {
        if body.len() < p {
            body = format!("{}{body}", "0".repeat(p - body.len()));
        }
    }
    // '#' makes the first octal digit zero, and puts 0x or 0X before a
    // non-zero hexadecimal value.
    let mut prefix: Vec<u8> = Vec::new();
    match conv {
        b'd' | b'i' => prefix.extend_from_slice(sign_prefix(spec, negative)),
        b'o' if spec.alt && !body.starts_with('0') => body.insert(0, '0'),
        b'x' if spec.alt && v != 0 => prefix.extend_from_slice(b"0x"),
        b'X' if spec.alt && v != 0 => prefix.extend_from_slice(b"0X"),
        _ => {}
    }
    // '0' is ignored with a precision.
    let zero_pad = spec.zero && spec.prec.is_none();
    pad(spec, &prefix, body.as_bytes(), zero_pad)
}

/// `%e`, `%E`, `%f`, `%F`, `%g` and `%G` of a `double`.
fn format_double(spec: &Spec, conv: u8, v: f64) -> Vec<u8> {
    let upper = conv.is_ascii_uppercase();
    let prefix = sign_prefix(spec, v.is_sign_negative());
    let a = v.abs();
    if !a.is_finite() {
        let text = match (a.is_nan(), upper) {
            (true, false) => "nan",
            (true, true) => "NAN",
            (false, false) => "inf",
            (false, true) => "INF",
        };
        return pad(spec, prefix, text.as_bytes(), false);
    }
    let prec = spec.prec.unwrap_or(6);
    let body = match conv.to_ascii_lowercase() {
        b'e' => exp_form(a, prec, spec.alt, upper),
        b'f' => fixed_form(a, prec, spec.alt),
        _ => {
            // %g: P significant digits, in %e style when the exponent X of
            // the value rounded to P digits is below -4 or at least P, else
            // in %f style with P - 1 - X decimals; trailing zeros and a bare
            // decimal point removed unless '#'.
            let p = prec.max(1);
            let x = decimal_exponent(a, p - 1);
            let s = if x < -4 || x >= p as i32 {
                exp_form(a, p - 1, spec.alt, upper)
            } else {
                fixed_form(a, (p as i32 - 1 - x) as usize, spec.alt)
            };
            if spec.alt { s } else { strip_zeros(&s) }
        }
    };
    pad(spec, prefix, body.as_bytes(), spec.zero)
}

/// `%a` and `%A` of a `double`, as C11 specifies them (7.21.6.1) and, where
/// it leaves a choice, as the macOS C library writes them: a leading
/// hexadecimal digit of 1 (0 for zero), subnormal values normalized, the
/// fraction correctly rounded to the precision (a tie to even, where the
/// macOS library rounds a tie down), a carry kept in the leading digit
/// (`0x2p+3`), and a binary exponent of at least one digit.
fn format_hex(spec: &Spec, upper: bool, v: f64) -> Vec<u8> {
    let prefix: Vec<u8> = [
        sign_prefix(spec, v.is_sign_negative()),
        if upper { b"0X" } else { b"0x" },
    ]
    .concat();
    let bits = v.abs().to_bits();
    let (mut lead, mut frac, exp) = if bits == 0 {
        (0u64, 0u64, 0i64)
    } else {
        let biased = (bits >> 52) as i64;
        let mant = bits & ((1u64 << 52) - 1);
        if biased == 0 {
            // Shift the leading set bit into the implicit place.
            let shift = mant.leading_zeros() as i64 - 11;
            (1, (mant << shift) & ((1u64 << 52) - 1), -1022 - shift)
        } else {
            (1, mant, biased - 1023)
        }
    };
    // Thirteen hexadecimal digits hold the 52 bits of the fraction.
    let ndig = match spec.prec {
        None => {
            let mut n = 13;
            while n > 0 && frac & 0xf == 0 {
                frac >>= 4;
                n -= 1;
            }
            frac <<= 4 * (13 - n);
            n
        }
        Some(p) => p,
    };
    let mut digits = String::new();
    if ndig < 13 {
        let drop = 4 * (13 - ndig) as u32;
        let rem = frac & ((1u64 << drop) - 1);
        let mut keep = frac >> drop;
        // Correctly rounded in the default rounding mode, as C11 requires
        // (7.21.6.1): a tie goes to the even last digit, which is the
        // leading digit when no others are kept.
        let half = 1u64 << (drop - 1);
        let last_odd = if ndig == 0 {
            lead & 1 == 1
        } else {
            keep & 1 == 1
        };
        if rem > half || (rem == half && last_odd) {
            keep += 1;
            if ndig == 0 || keep >> (4 * ndig) != 0 {
                lead += 1;
                keep = 0;
            }
        }
        if ndig > 0 {
            digits = format!("{keep:0ndig$x}");
        }
    } else {
        digits = format!("{frac:013x}");
        digits.push_str(&"0".repeat(ndig - 13));
    }
    let point = if !digits.is_empty() || spec.alt {
        "."
    } else {
        ""
    };
    let sign = if exp < 0 { '-' } else { '+' };
    let mut body = format!("{lead}{point}{digits}p{sign}{}", exp.unsigned_abs());
    if upper {
        body = body.to_ascii_uppercase();
    }
    pad(spec, &prefix, body.as_bytes(), spec.zero)
}

/// The decimal exponent of `a` written with `decimals` digits after the
/// point in `%e` style.
fn decimal_exponent(a: f64, decimals: usize) -> i32 {
    let s = format!("{a:.decimals$e}");
    s.rsplit_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .expect("Rust's {:e} has an exponent")
}

/// `a` in `%e` style: one digit, `prec` decimals, then the exponent with a
/// sign and at least two digits. '#' keeps the point when `prec` is 0.
fn exp_form(a: f64, prec: usize, alt: bool, upper: bool) -> String {
    let s = format!("{a:.prec$e}");
    let (mantissa, exp) = s.rsplit_once('e').expect("Rust's {:e} has an exponent");
    let exp: i32 = exp.parse().expect("Rust's {:e} exponent is an integer");
    let point = if alt && prec == 0 { "." } else { "" };
    let e = if upper { 'E' } else { 'e' };
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}{point}{e}{sign}{:02}", exp.unsigned_abs())
}

/// `a` in `%f` style with `prec` decimals. '#' keeps the point when `prec`
/// is 0.
fn fixed_form(a: f64, prec: usize, alt: bool) -> String {
    let s = format!("{a:.prec$}");
    if alt && prec == 0 { s + "." } else { s }
}

/// Remove trailing zeros from the fraction of a `%g` result, and the point
/// if no fraction is left.
fn strip_zeros(s: &str) -> String {
    let (num, exp) = match s.find(['e', 'E']) {
        Some(i) => s.split_at(i),
        None => (s, ""),
    };
    let num = if num.contains('.') {
        num.trim_end_matches('0').trim_end_matches('.')
    } else {
        num
    };
    format!("{num}{exp}")
}

/// C's `strtod` on `s`, without skipping leading white space: the value of
/// the longest prefix that is a decimal or hexadecimal floating constant,
/// an infinity or a NaN, and its length, or `None` if no prefix is. An
/// exponent marker with no digits after it, or a "0x" with no hexadecimal
/// digits, is not part of the number.
///
/// C's `sscanf( "%lf" )` does this on macOS; the C standard instead makes
/// the input item the longest prefix of a possible number, so a library
/// following it fails on "1e+x" or "0x", where this reads "1" or "0".
pub(crate) fn c_strtod(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    let neg = match s.first() {
        Some(b'-') => {
            i = 1;
            true
        }
        Some(b'+') => {
            i = 1;
            false
        }
        _ => false,
    };
    let signed = |v: f64| if neg { -v } else { v };
    let rest = &s[i..];
    let starts_with_ci =
        |word: &[u8]| rest.len() >= word.len() && rest[..word.len()].eq_ignore_ascii_case(word);

    if starts_with_ci(b"infinity") {
        return Some((signed(f64::INFINITY), i + 8));
    }
    if starts_with_ci(b"inf") {
        return Some((signed(f64::INFINITY), i + 3));
    }
    if starts_with_ci(b"nan") {
        // "nan" may be followed by a parenthesized sequence of letters,
        // digits and underscores, taken only when it is closed.
        let mut end = i + 3;
        if s.get(end) == Some(&b'(') {
            let close = s[end + 1..]
                .iter()
                .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_'));
            if let Some(k) = close {
                if s[end + 1 + k] == b')' {
                    end += k + 2;
                }
            }
        }
        return Some((f64::NAN, end));
    }

    if rest.len() >= 2 && rest[0] == b'0' && (rest[1] | 0x20) == b'x' {
        if let Some((v, len)) = hex_float(&rest[2..]) {
            return Some((signed(v), i + 2 + len));
        }
        // "0x" with no hexadecimal digits is the number 0.
        return Some((signed(0.0), i + 1));
    }

    // Decimal: digits with an optional point, at least one digit, then an
    // optional exponent with at least one digit.
    let digits = |from: usize| s[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    let mut j = i + digits(i);
    let mut ndig = j - i;
    if s.get(j) == Some(&b'.') {
        let frac = digits(j + 1);
        ndig += frac;
        j += 1 + frac;
    }
    if ndig == 0 {
        return None;
    }
    if s.get(j).is_some_and(|b| (b | 0x20) == b'e') {
        let k = j + 1 + usize::from(matches!(s.get(j + 1), Some(b'+' | b'-')));
        let exp = digits(k);
        if exp > 0 {
            j = k + exp;
        }
    }
    // Rust's parse is correctly rounded, as strtod is.
    let text = std::str::from_utf8(&s[i..j]).expect("ASCII digits");
    let v: f64 = text.parse().expect("a decimal floating constant");
    Some((signed(v), j))
}

/// The value and length of the hexadecimal floating constant after "0x":
/// hexadecimal digits with an optional point, at least one digit, then an
/// optional binary exponent with at least one decimal digit. Correctly
/// rounded to nearest, ties to even, as strtod rounds.
fn hex_float(s: &[u8]) -> Option<(f64, usize)> {
    let hex = |from: usize| {
        s[from..]
            .iter()
            .take_while(|b| b.is_ascii_hexdigit())
            .count()
    };
    let int_len = hex(0);
    let mut j = int_len;
    let mut frac_len = 0;
    if s.get(j) == Some(&b'.') {
        frac_len = hex(j + 1);
        j += 1 + frac_len;
    }
    if int_len + frac_len == 0 {
        return None;
    }
    let mut exp2: i64 = 0;
    if s.get(j).is_some_and(|b| (b | 0x20) == b'p') {
        let k = j + 1 + usize::from(matches!(s.get(j + 1), Some(b'+' | b'-')));
        let n = s[k..].iter().take_while(|b| b.is_ascii_digit()).count();
        if n > 0 {
            for &d in &s[k..k + n] {
                exp2 = (exp2 * 10 + i64::from(d - b'0')).min(1 << 40);
            }
            if s[j + 1] == b'-' {
                exp2 = -exp2;
            }
            j = k + n;
        }
    }

    // The first 15 significant hexadecimal digits fit in 60 bits; any
    // non-zero digit after them only matters as a sticky bit for rounding.
    let mut mant: u64 = 0;
    let mut sticky = false;
    let mut kept = 0;
    let mut scale: i64 = 0;
    let int_digits = &s[..int_len];
    let frac_digits = if frac_len == 0 {
        &[][..]
    } else {
        &s[int_len + 1..int_len + 1 + frac_len]
    };
    for (k, &d) in int_digits.iter().chain(frac_digits).enumerate() {
        let v = u64::from((d as char).to_digit(16).expect("hex digit"));
        let in_frac = k >= int_len;
        if mant == 0 && v == 0 {
            if in_frac {
                scale -= 4;
            }
            continue;
        }
        if kept < 15 {
            mant = mant << 4 | v;
            kept += 1;
            if in_frac {
                scale -= 4;
            }
        } else {
            sticky |= v != 0;
            if !in_frac {
                scale += 4;
            }
        }
    }
    Some((round_to_f64(mant, sticky, exp2 + scale), j))
}

/// `mant * 2^exp2`, where `sticky` stands for further non-zero bits below
/// `mant`, rounded to the nearest double, ties to even.
fn round_to_f64(mant: u64, sticky: bool, exp2: i64) -> f64 {
    if mant == 0 {
        return 0.0;
    }
    let lz = i64::from(mant.leading_zeros());
    let m = u128::from(mant) << (64 + lz);
    // The value is in [2^e, 2^(e + 1)).
    let e = exp2 - lz + 63;
    if e > 1023 {
        return f64::INFINITY;
    }
    // Bits of precision available: 53, fewer for a subnormal.
    let p = if e >= -1022 { 53 } else { 53 - (-1022 - e) };
    if p < 0 {
        return 0.0;
    }
    let shift = 128 - p as u32;
    let (mut kept, round, rest) = if shift == 128 {
        (0u64, m >> 127 == 1, (m << 1) != 0 || sticky)
    } else {
        (
            (m >> shift) as u64,
            (m >> (shift - 1)) & 1 == 1,
            m & ((1u128 << (shift - 1)) - 1) != 0 || sticky,
        )
    };
    if round && (rest || kept & 1 == 1) {
        kept += 1;
    }
    if e >= -1022 {
        let mut e = e;
        if kept == 1 << 53 {
            kept >>= 1;
            e += 1;
        }
        if e > 1023 {
            return f64::INFINITY;
        }
        f64::from_bits(((e + 1023) as u64) << 52 | (kept & ((1 << 52) - 1)))
    } else {
        // A subnormal counts in units of 2^-1074; a carry to 2^52 is the
        // smallest normal number, which has the same bits.
        f64::from_bits(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `%a` and `%A`: the macOS C library's results (`snprintf` at the C
    /// library's reference build), each value given by its bits, except at
    /// an exact tie, which C11 rounds to even and the macOS library rounds
    /// down; those four are marked.
    #[test]
    fn hexadecimal_matches_c11_and_the_macos_c_library() {
        let cases: &[(&str, u64, &str)] = &[
            ("%a", 0x0000000000000000, "0x0p+0"),
            ("%a", 0x8000000000000000, "-0x0p+0"),
            ("%a", 0x3ff0000000000000, "0x1p+0"),
            ("%a", 0x4050500000000000, "0x1.05p+6"),
            ("%a", 0x3fb999999999999a, "0x1.999999999999ap-4"),
            ("%a", 0xc004000000000000, "-0x1.4p+1"),
            ("%a", 0x000012688b70e62b, "0x1.2688b70e62bp-1030"),
            ("%a", 0x0000000000000001, "0x1p-1074"),
            ("%a", 0x7fefffffffffffff, "0x1.fffffffffffffp+1023"),
            ("%a", 0x3fd5555555555555, "0x1.5555555555555p-2"),
            ("%a", 0x3fefffffffffffff, "0x1.fffffffffffffp-1"),
            ("%a", 0x402ff00000000000, "0x1.ffp+3"),
            ("%a", 0x3ff0800000000000, "0x1.08p+0"),
            ("%a", 0x3ff1800000000000, "0x1.18p+0"),
            ("%a", 0x3ff8000000000000, "0x1.8p+0"),
            ("%a", 0x3fffff8000000000, "0x1.fff8p+0"),
            ("%A", 0x0000000000000000, "0X0P+0"),
            ("%A", 0x8000000000000000, "-0X0P+0"),
            ("%A", 0x3ff0000000000000, "0X1P+0"),
            ("%A", 0x4050500000000000, "0X1.05P+6"),
            ("%A", 0x3fb999999999999a, "0X1.999999999999AP-4"),
            ("%A", 0xc004000000000000, "-0X1.4P+1"),
            ("%A", 0x000012688b70e62b, "0X1.2688B70E62BP-1030"),
            ("%A", 0x0000000000000001, "0X1P-1074"),
            ("%A", 0x7fefffffffffffff, "0X1.FFFFFFFFFFFFFP+1023"),
            ("%A", 0x3fd5555555555555, "0X1.5555555555555P-2"),
            ("%A", 0x3fefffffffffffff, "0X1.FFFFFFFFFFFFFP-1"),
            ("%A", 0x402ff00000000000, "0X1.FFP+3"),
            ("%A", 0x3ff0800000000000, "0X1.08P+0"),
            ("%A", 0x3ff1800000000000, "0X1.18P+0"),
            ("%A", 0x3ff8000000000000, "0X1.8P+0"),
            ("%A", 0x3fffff8000000000, "0X1.FFF8P+0"),
            ("%.0a", 0x0000000000000000, "0x0p+0"),
            ("%.0a", 0x8000000000000000, "-0x0p+0"),
            ("%.0a", 0x3ff0000000000000, "0x1p+0"),
            ("%.0a", 0x4050500000000000, "0x1p+6"),
            ("%.0a", 0x3fb999999999999a, "0x2p-4"),
            ("%.0a", 0xc004000000000000, "-0x1p+1"),
            ("%.0a", 0x000012688b70e62b, "0x1p-1030"),
            ("%.0a", 0x0000000000000001, "0x1p-1074"),
            ("%.0a", 0x7fefffffffffffff, "0x2p+1023"),
            ("%.0a", 0x3fd5555555555555, "0x1p-2"),
            ("%.0a", 0x3fefffffffffffff, "0x2p-1"),
            ("%.0a", 0x402ff00000000000, "0x2p+3"),
            ("%.0a", 0x3ff0800000000000, "0x1p+0"),
            ("%.0a", 0x3ff1800000000000, "0x1p+0"),
            ("%.0a", 0x3ff8000000000000, "0x2p+0"), /* tie; macOS 0x1p+0 */
            ("%.0a", 0x3fffff8000000000, "0x2p+0"),
            ("%.1a", 0x0000000000000000, "0x0.0p+0"),
            ("%.1a", 0x8000000000000000, "-0x0.0p+0"),
            ("%.1a", 0x3ff0000000000000, "0x1.0p+0"),
            ("%.1a", 0x4050500000000000, "0x1.0p+6"),
            ("%.1a", 0x3fb999999999999a, "0x1.ap-4"),
            ("%.1a", 0xc004000000000000, "-0x1.4p+1"),
            ("%.1a", 0x000012688b70e62b, "0x1.2p-1030"),
            ("%.1a", 0x0000000000000001, "0x1.0p-1074"),
            ("%.1a", 0x7fefffffffffffff, "0x2.0p+1023"),
            ("%.1a", 0x3fd5555555555555, "0x1.5p-2"),
            ("%.1a", 0x3fefffffffffffff, "0x2.0p-1"),
            ("%.1a", 0x402ff00000000000, "0x2.0p+3"),
            ("%.1a", 0x3ff0800000000000, "0x1.0p+0"),
            ("%.1a", 0x3ff1800000000000, "0x1.2p+0"), /* tie; macOS 0x1.1p+0 */
            ("%.1a", 0x3ff8000000000000, "0x1.8p+0"),
            ("%.1a", 0x3fffff8000000000, "0x2.0p+0"),
            ("%.3a", 0x0000000000000000, "0x0.000p+0"),
            ("%.3a", 0x8000000000000000, "-0x0.000p+0"),
            ("%.3a", 0x3ff0000000000000, "0x1.000p+0"),
            ("%.3a", 0x4050500000000000, "0x1.050p+6"),
            ("%.3a", 0x3fb999999999999a, "0x1.99ap-4"),
            ("%.3a", 0xc004000000000000, "-0x1.400p+1"),
            ("%.3a", 0x000012688b70e62b, "0x1.269p-1030"),
            ("%.3a", 0x0000000000000001, "0x1.000p-1074"),
            ("%.3a", 0x7fefffffffffffff, "0x2.000p+1023"),
            ("%.3a", 0x3fd5555555555555, "0x1.555p-2"),
            ("%.3a", 0x3fefffffffffffff, "0x2.000p-1"),
            ("%.3a", 0x402ff00000000000, "0x1.ff0p+3"),
            ("%.3a", 0x3ff0800000000000, "0x1.080p+0"),
            ("%.3a", 0x3ff1800000000000, "0x1.180p+0"),
            ("%.3a", 0x3ff8000000000000, "0x1.800p+0"),
            ("%.3a", 0x3fffff8000000000, "0x2.000p+0"), /* tie; macOS 0x1.fffp+0 */
            ("%.20a", 0x0000000000000000, "0x0.00000000000000000000p+0"),
            ("%.20a", 0x8000000000000000, "-0x0.00000000000000000000p+0"),
            ("%.20a", 0x3ff0000000000000, "0x1.00000000000000000000p+0"),
            ("%.20a", 0x4050500000000000, "0x1.05000000000000000000p+6"),
            ("%.20a", 0x3fb999999999999a, "0x1.999999999999a0000000p-4"),
            ("%.20a", 0xc004000000000000, "-0x1.40000000000000000000p+1"),
            (
                "%.20a",
                0x000012688b70e62b,
                "0x1.2688b70e62b000000000p-1030",
            ),
            (
                "%.20a",
                0x0000000000000001,
                "0x1.00000000000000000000p-1074",
            ),
            (
                "%.20a",
                0x7fefffffffffffff,
                "0x1.fffffffffffff0000000p+1023",
            ),
            ("%.20a", 0x3fd5555555555555, "0x1.55555555555550000000p-2"),
            ("%.20a", 0x3fefffffffffffff, "0x1.fffffffffffff0000000p-1"),
            ("%.20a", 0x402ff00000000000, "0x1.ff000000000000000000p+3"),
            ("%.20a", 0x3ff0800000000000, "0x1.08000000000000000000p+0"),
            ("%.20a", 0x3ff1800000000000, "0x1.18000000000000000000p+0"),
            ("%.20a", 0x3ff8000000000000, "0x1.80000000000000000000p+0"),
            ("%.20a", 0x3fffff8000000000, "0x1.fff80000000000000000p+0"),
            ("%20a", 0x0000000000000000, "              0x0p+0"),
            ("%20a", 0x8000000000000000, "             -0x0p+0"),
            ("%20a", 0x3ff0000000000000, "              0x1p+0"),
            ("%20a", 0x4050500000000000, "           0x1.05p+6"),
            ("%20a", 0x3fb999999999999a, "0x1.999999999999ap-4"),
            ("%20a", 0xc004000000000000, "           -0x1.4p+1"),
            ("%20a", 0x000012688b70e62b, "0x1.2688b70e62bp-1030"),
            ("%20a", 0x0000000000000001, "           0x1p-1074"),
            ("%20a", 0x7fefffffffffffff, "0x1.fffffffffffffp+1023"),
            ("%20a", 0x3fd5555555555555, "0x1.5555555555555p-2"),
            ("%20a", 0x3fefffffffffffff, "0x1.fffffffffffffp-1"),
            ("%20a", 0x402ff00000000000, "           0x1.ffp+3"),
            ("%20a", 0x3ff0800000000000, "           0x1.08p+0"),
            ("%20a", 0x3ff1800000000000, "           0x1.18p+0"),
            ("%20a", 0x3ff8000000000000, "            0x1.8p+0"),
            ("%20a", 0x3fffff8000000000, "         0x1.fff8p+0"),
            ("%-20a|", 0x0000000000000000, "0x0p+0              |"),
            ("%-20a|", 0x8000000000000000, "-0x0p+0             |"),
            ("%-20a|", 0x3ff0000000000000, "0x1p+0              |"),
            ("%-20a|", 0x4050500000000000, "0x1.05p+6           |"),
            ("%-20a|", 0x3fb999999999999a, "0x1.999999999999ap-4|"),
            ("%-20a|", 0xc004000000000000, "-0x1.4p+1           |"),
            ("%-20a|", 0x000012688b70e62b, "0x1.2688b70e62bp-1030|"),
            ("%-20a|", 0x0000000000000001, "0x1p-1074           |"),
            ("%-20a|", 0x7fefffffffffffff, "0x1.fffffffffffffp+1023|"),
            ("%-20a|", 0x3fd5555555555555, "0x1.5555555555555p-2|"),
            ("%-20a|", 0x3fefffffffffffff, "0x1.fffffffffffffp-1|"),
            ("%-20a|", 0x402ff00000000000, "0x1.ffp+3           |"),
            ("%-20a|", 0x3ff0800000000000, "0x1.08p+0           |"),
            ("%-20a|", 0x3ff1800000000000, "0x1.18p+0           |"),
            ("%-20a|", 0x3ff8000000000000, "0x1.8p+0            |"),
            ("%-20a|", 0x3fffff8000000000, "0x1.fff8p+0         |"),
            ("%+a", 0x0000000000000000, "+0x0p+0"),
            ("%+a", 0x8000000000000000, "-0x0p+0"),
            ("%+a", 0x3ff0000000000000, "+0x1p+0"),
            ("%+a", 0x4050500000000000, "+0x1.05p+6"),
            ("%+a", 0x3fb999999999999a, "+0x1.999999999999ap-4"),
            ("%+a", 0xc004000000000000, "-0x1.4p+1"),
            ("%+a", 0x000012688b70e62b, "+0x1.2688b70e62bp-1030"),
            ("%+a", 0x0000000000000001, "+0x1p-1074"),
            ("%+a", 0x7fefffffffffffff, "+0x1.fffffffffffffp+1023"),
            ("%+a", 0x3fd5555555555555, "+0x1.5555555555555p-2"),
            ("%+a", 0x3fefffffffffffff, "+0x1.fffffffffffffp-1"),
            ("%+a", 0x402ff00000000000, "+0x1.ffp+3"),
            ("%+a", 0x3ff0800000000000, "+0x1.08p+0"),
            ("%+a", 0x3ff1800000000000, "+0x1.18p+0"),
            ("%+a", 0x3ff8000000000000, "+0x1.8p+0"),
            ("%+a", 0x3fffff8000000000, "+0x1.fff8p+0"),
            ("% a", 0x0000000000000000, " 0x0p+0"),
            ("% a", 0x8000000000000000, "-0x0p+0"),
            ("% a", 0x3ff0000000000000, " 0x1p+0"),
            ("% a", 0x4050500000000000, " 0x1.05p+6"),
            ("% a", 0x3fb999999999999a, " 0x1.999999999999ap-4"),
            ("% a", 0xc004000000000000, "-0x1.4p+1"),
            ("% a", 0x000012688b70e62b, " 0x1.2688b70e62bp-1030"),
            ("% a", 0x0000000000000001, " 0x1p-1074"),
            ("% a", 0x7fefffffffffffff, " 0x1.fffffffffffffp+1023"),
            ("% a", 0x3fd5555555555555, " 0x1.5555555555555p-2"),
            ("% a", 0x3fefffffffffffff, " 0x1.fffffffffffffp-1"),
            ("% a", 0x402ff00000000000, " 0x1.ffp+3"),
            ("% a", 0x3ff0800000000000, " 0x1.08p+0"),
            ("% a", 0x3ff1800000000000, " 0x1.18p+0"),
            ("% a", 0x3ff8000000000000, " 0x1.8p+0"),
            ("% a", 0x3fffff8000000000, " 0x1.fff8p+0"),
            ("%#.0a", 0x0000000000000000, "0x0.p+0"),
            ("%#.0a", 0x8000000000000000, "-0x0.p+0"),
            ("%#.0a", 0x3ff0000000000000, "0x1.p+0"),
            ("%#.0a", 0x4050500000000000, "0x1.p+6"),
            ("%#.0a", 0x3fb999999999999a, "0x2.p-4"),
            ("%#.0a", 0xc004000000000000, "-0x1.p+1"),
            ("%#.0a", 0x000012688b70e62b, "0x1.p-1030"),
            ("%#.0a", 0x0000000000000001, "0x1.p-1074"),
            ("%#.0a", 0x7fefffffffffffff, "0x2.p+1023"),
            ("%#.0a", 0x3fd5555555555555, "0x1.p-2"),
            ("%#.0a", 0x3fefffffffffffff, "0x2.p-1"),
            ("%#.0a", 0x402ff00000000000, "0x2.p+3"),
            ("%#.0a", 0x3ff0800000000000, "0x1.p+0"),
            ("%#.0a", 0x3ff1800000000000, "0x1.p+0"),
            ("%#.0a", 0x3ff8000000000000, "0x2.p+0"), /* tie; macOS 0x1.p+0 */
            ("%#.0a", 0x3fffff8000000000, "0x2.p+0"),
            ("%020a", 0x0000000000000000, "0x000000000000000p+0"),
            ("%020a", 0x8000000000000000, "-0x00000000000000p+0"),
            ("%020a", 0x3ff0000000000000, "0x000000000000001p+0"),
            ("%020a", 0x4050500000000000, "0x000000000001.05p+6"),
            ("%020a", 0x3fb999999999999a, "0x1.999999999999ap-4"),
            ("%020a", 0xc004000000000000, "-0x000000000001.4p+1"),
            ("%020a", 0x000012688b70e62b, "0x1.2688b70e62bp-1030"),
            ("%020a", 0x0000000000000001, "0x000000000001p-1074"),
            ("%020a", 0x7fefffffffffffff, "0x1.fffffffffffffp+1023"),
            ("%020a", 0x3fd5555555555555, "0x1.5555555555555p-2"),
            ("%020a", 0x3fefffffffffffff, "0x1.fffffffffffffp-1"),
            ("%020a", 0x402ff00000000000, "0x000000000001.ffp+3"),
            ("%020a", 0x3ff0800000000000, "0x000000000001.08p+0"),
            ("%020a", 0x3ff1800000000000, "0x000000000001.18p+0"),
            ("%020a", 0x3ff8000000000000, "0x0000000000001.8p+0"),
            ("%020a", 0x3fffff8000000000, "0x0000000001.fff8p+0"),
            ("%'a", 0x0000000000000000, "0x0p+0"),
            ("%'a", 0x8000000000000000, "-0x0p+0"),
            ("%'a", 0x3ff0000000000000, "0x1p+0"),
            ("%'a", 0x4050500000000000, "0x1.05p+6"),
            ("%'a", 0x3fb999999999999a, "0x1.999999999999ap-4"),
            ("%'a", 0xc004000000000000, "-0x1.4p+1"),
            ("%'a", 0x000012688b70e62b, "0x1.2688b70e62bp-1030"),
            ("%'a", 0x0000000000000001, "0x1p-1074"),
            ("%'a", 0x7fefffffffffffff, "0x1.fffffffffffffp+1023"),
            ("%'a", 0x3fd5555555555555, "0x1.5555555555555p-2"),
            ("%'a", 0x3fefffffffffffff, "0x1.fffffffffffffp-1"),
            ("%'a", 0x402ff00000000000, "0x1.ffp+3"),
            ("%'a", 0x3ff0800000000000, "0x1.08p+0"),
            ("%'a", 0x3ff1800000000000, "0x1.18p+0"),
            ("%'a", 0x3ff8000000000000, "0x1.8p+0"),
            ("%'a", 0x3fffff8000000000, "0x1.fff8p+0"),
            ("%la", 0x0000000000000000, "0x0p+0"),
            ("%la", 0x8000000000000000, "-0x0p+0"),
            ("%la", 0x3ff0000000000000, "0x1p+0"),
            ("%la", 0x4050500000000000, "0x1.05p+6"),
            ("%la", 0x3fb999999999999a, "0x1.999999999999ap-4"),
            ("%la", 0xc004000000000000, "-0x1.4p+1"),
            ("%la", 0x000012688b70e62b, "0x1.2688b70e62bp-1030"),
            ("%la", 0x0000000000000001, "0x1p-1074"),
            ("%la", 0x7fefffffffffffff, "0x1.fffffffffffffp+1023"),
            ("%la", 0x3fd5555555555555, "0x1.5555555555555p-2"),
            ("%la", 0x3fefffffffffffff, "0x1.fffffffffffffp-1"),
            ("%la", 0x402ff00000000000, "0x1.ffp+3"),
            ("%la", 0x3ff0800000000000, "0x1.08p+0"),
            ("%la", 0x3ff1800000000000, "0x1.18p+0"),
            ("%la", 0x3ff8000000000000, "0x1.8p+0"),
            ("%la", 0x3fffff8000000000, "0x1.fff8p+0"),
        ];
        let failures: Vec<String> = cases
            .iter()
            .filter_map(|&(fmt, bits, want)| {
                let got = sprintf_double(fmt, f64::from_bits(bits));
                (got != want).then(|| format!("{fmt} {bits:#018x}: got {got}, want {want}"))
            })
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // In log form the signs are the value's, before "10", and the
    // exponent's, and not the `-` of the escape sequence after "10".
    #[test]
    fn minus_signs_of_log_form() {
        for escapes in [false, true] {
            let _guard = crate::escapes::set_escapes(escapes);
            let text = fmt("%&g", 7, -1.0e-5);
            let delim = if escapes {
                crate::escapes::EXDEL
            } else {
                LOG_TXT
            };
            assert!(text.starts_with(&format!("-{delim}")), "{text}");
            let exponent = 1 + delim.len() + text[1 + delim.len()..].find('-').unwrap();
            assert_eq!(plain_minus_signs("%&g", &text), vec![0, exponent], "{text}");
            let text = fmt("%&g", 7, 1.0e5);
            assert!(plain_minus_signs("%&g", &text).is_empty(), "{text}");
        }
    }

    fn fmt(f: &str, digits: i32, v: f64) -> String {
        format_value(f, digits, v).unwrap()
    }

    #[test]
    fn default_format_matches_c() {
        // Verified against C printf("%1.7G", x).
        assert_eq!(fmt("%1.7G", 7, 0.0), "0");
        assert_eq!(fmt("%1.7G", 7, 1234.5678), "1234.568");
        assert_eq!(fmt("%1.7G", 7, 0.00012345678), "0.0001234568");
        assert_eq!(fmt("%1.7G", 7, 123456789.0), "1.234568E+08");
        assert_eq!(fmt("%1.7G", 7, 9.999999e6), "9999999");
        assert_eq!(fmt("%1.7G", 7, 1e-5), "1E-05");
    }

    #[test]
    fn parse_rewrites_as_c_does() {
        let p = parse_axis_format("%.*G", 5).unwrap();
        assert_eq!(p.fmt, b"%.5G");
        assert!(!p.integ && !p.log);

        let p = parse_axis_format("%+-&#8.2f  ", 7).unwrap();
        assert_eq!(p.fmt, b"%-#8.2f");
        assert!(p.log && p.sign && !p.integ);

        // Without '&' the Format is unchanged apart from trailing space.
        let p = parse_axis_format("%+ d ", 7).unwrap();
        assert_eq!(p.fmt, b"%+ d");
        assert!(p.integ && !p.log);
    }

    #[test]
    fn formats_the_c_library_refuses_are_errors() {
        for f in ["%g %g", "%*g", "%.*g %d", "%ld", "%s", "%Lg", "%"] {
            let err = format_value(f, 7, 1.5).expect_err(f);
            assert_eq!(err.code(), ErrorCode::Fmter, "{f}");
            assert!(
                err.message().starts_with(&format!(
                    "astFormat: The Format string \"{f}\" cannot be used"
                )),
                "{f}: {}",
                err.message()
            );
        }
    }

    #[test]
    fn zero_byte_ends_the_string() {
        assert_eq!(fmt("ab%cde", 7, 0.0), "ab");
        assert_eq!(fmt("%3c|", 7, 0.0), "  ");
    }
}

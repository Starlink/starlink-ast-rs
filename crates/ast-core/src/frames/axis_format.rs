//! Port of `axis.c`'s `ParseAxisFormat` and `AxisFormat`, which format an
//! axis value by handing the Axis `Format` string to C's `sprintf`, and of
//! the part of C's `printf` that call needs: a format with at most one
//! conversion, given an `int` or a `double`.
//!
//! `sprintf` is called with one argument, so a Format whose conversions do
//! not consume exactly that argument has undefined behavior in C: a second
//! conversion, a `*` width or precision, a length modifier that changes the
//! argument type, a conversion of the wrong type (`%ld`, `%s`), or a
//! trailing `%`. Each is an `AST__FMTER` error here. `%a`, `%A` and the
//! `'` flag are defined but not ported, and are errors too.

use crate::error::{AstError, AstResult, ErrorCode};

/// The text `AxisFormat` writes between "10" and the exponent of a log
/// Format when graphical escapes are not retained, which is the default
/// (axis.c:157, object.c:418). Escapes are a Plot feature, not yet ported.
const LOG_TXT: &str = "10^";

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

/// Port of `ParseAxisFormat` (axis.c:2015).
pub(crate) fn parse_axis_format(fmt0: &str, digs: i32) -> ParsedFormat {
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

    ParsedFormat {
        fmt: result,
        log,
        sign,
        space: lspace,
        integ,
    }
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
    let p = parse_axis_format(fmt, digits);

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
        buf.extend_from_slice(LOG_TXT.as_bytes());
        if x.abs() < 1.0e-10 {
            x = 0.0;
        }
    }

    let arg = if p.integ {
        Arg::Int(c_int(x))
    } else {
        Arg::Double(x)
    };
    buf.extend(sprintf(&p.fmt, arg).map_err(|e| {
        AstError::new(
            ErrorCode::Fmter,
            format!(
                "astAxisFormat(Axis): Error formatting a coordinate value of {value}: {e}. \
                 The format string was \"{}\".",
                String::from_utf8_lossy(&p.fmt)
            ),
        )
    })?);
    if log {
        buf.extend_from_slice(LOG_END.as_bytes());
    }
    if buf.len() > BUFF_LEN {
        return Err(AstError::new(
            ErrorCode::Fmter,
            format!(
                "astAxisFormat(Axis): Internal buffer overflow while formatting a coordinate \
                 value of {value} - result exceeds {BUFF_LEN} characters."
            ),
        ));
    }

    // The result is a C string, so it ends at the first NUL, which `%c`
    // writes for a zero byte.
    if let Some(nul) = buf.iter().position(|&b| b == 0) {
        buf.truncate(nul);
    }
    String::from_utf8(buf).map_err(|_| {
        AstError::new(
            ErrorCode::Fmter,
            format!("astAxisFormat(Axis): formatting {value} with \"{fmt}\" gave a non-ASCII byte"),
        )
    })
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
    let mut out = Vec::new();
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
                b'\'' => return Err("the ' flag is not supported".into()),
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
            (b'a' | b'A', Arg::Double(_)) => return Err("%a is not supported".into()),
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
    }
    Ok(out)
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
        let p = parse_axis_format("%.*G", 5);
        assert_eq!(p.fmt, b"%.5G");
        assert!(!p.integ && !p.log);

        let p = parse_axis_format("%+-&#8.2f  ", 7);
        assert_eq!(p.fmt, b"%-#8.2f");
        assert!(p.log && p.sign && !p.integ);

        // Without '&' the Format is unchanged apart from trailing space.
        let p = parse_axis_format("%+ d ", 7);
        assert_eq!(p.fmt, b"%+ d");
        assert!(p.integ && !p.log);
    }

    #[test]
    fn undefined_formats_are_errors() {
        for f in ["%g %g", "%*g", "%.*g %d", "%ld", "%s", "%Lg", "%a", "%"] {
            let err = format_value(f, 7, 1.5).expect_err(f);
            assert_eq!(err.code(), ErrorCode::Fmter, "{f}");
        }
    }

    #[test]
    fn zero_byte_ends_the_string() {
        assert_eq!(fmt("ab%cde", 7, 0.0), "ab");
        assert_eq!(fmt("%3c|", 7, 0.0), "  ");
    }
}

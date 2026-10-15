//! `ConvertValue`: the KeyMap type-conversion matrix.
//!
//! Every `map_get`, `map_get_1` and `map_get_elem` call routes through
//! here, so the whole 9-by-9 matrix is present. A pairing C rejects
//! returns `Ok(None)`, which callers turn into `AST__MPGER`.

use crate::constants::BAD_VALUE;
use crate::error::AstResult;
use crate::keymap::entry::{AstType, EntryValue, Scalar1D};
use crate::serial::AnyObject;
use crate::serial::native_write::format_double_g_signed;

/// `AST__BAD_STRING`: how `AST__BAD` formats, and the only string
/// accepted as `AST__BAD` on the way back in.
pub(crate) const BAD_STRING: &str = "<bad>";

/// The number of significant digits used for a full-precision double.
/// C's `AST__DBL_DIG`.
const DBL_DIG: usize = 17;

/// `FLT_DIG`: the significant digits used for a float.
const FLT_DIG: usize = 6;

/// One value lifted out of an entry, tagged with its type.
///
/// This is what C's `void *raw` plus a separate `raw_type` amount to.
#[derive(Debug, Clone)]
pub(crate) enum RawValue {
    Int(i32),
    SInt(i16),
    KInt(i64),
    Byte(u8),
    Float(f32),
    Double(f64),
    String(String),
    /// `None` is C's NULL Object element, which `ConvertValue`'s
    /// Object-to-Object arm passes straight through (`keymap.c:2201`-2202).
    Object(Option<Box<AnyObject>>),
    Undef,
}

impl RawValue {
    pub(crate) fn ast_type(&self) -> AstType {
        match self {
            RawValue::Int(_) => AstType::Int,
            RawValue::SInt(_) => AstType::SInt,
            RawValue::KInt(_) => AstType::KInt,
            RawValue::Byte(_) => AstType::Byte,
            RawValue::Float(_) => AstType::Float,
            RawValue::Double(_) => AstType::Double,
            RawValue::String(_) => AstType::String,
            RawValue::Object(_) => AstType::Object,
            RawValue::Undef => AstType::Undef,
        }
    }
}

/// Format a double the way `ConvertValue` does.
///
/// C formats at both `AST__DBL_DIG - 3` and `AST__DBL_DIG` significant
/// digits and keeps the reduced form when full precision costs more than
/// nine extra characters. C's own comment explains the heuristic: "If
/// reducing the number of decimal places by three produces a saving of
/// 10 or more characters, assume the least significant two characters
/// are rounding error." (`keymap.c:1975-1988`.) This is C's own
/// heuristic, not one introduced by the port.
///
/// Uses [`format_double_g_signed`], not `format_double_g`: this arm has
/// no negative-zero fixup in C (unlike `WriteDouble`), so `-0.0` formats
/// as `"-0"`.
pub(crate) fn double_to_string(value: f64) -> String {
    if value == BAD_VALUE {
        return BAD_STRING.to_string();
    }
    let reduced = format_double_g_signed(value, DBL_DIG - 3);
    let full = format_double_g_signed(value, DBL_DIG);
    if full.len() as isize - reduced.len() as isize > 9 {
        reduced
    } else {
        full
    }
}

/// Format a float the way `ConvertValue` does: `%.*g` at `FLT_DIG`
/// significant digits, with no `AST__BAD` check and no negative-zero
/// fixup on this path (`keymap.c:2035-2037`), so `-0.0f` formats as
/// `"-0"`.
pub(crate) fn float_to_string(value: f32) -> String {
    format_double_g_signed(f64::from(value), FLT_DIG)
}

/// Scan a whole string as an `i64`, as C's `astSscanf( cval, " %d %n" )`
/// followed by the "did it consume everything" test does.
///
/// glibc's `%d` conversion does not accept C99 hex notation (unlike
/// `%lf`/`%f`), so `"0x10"` is rejected here exactly as it is in C — the
/// hex value comes back through the `%lf` fallback in
/// [`convert_value`] instead, matching C's own two-scan fallback chain.
///
/// A digit string too large for `i64` fails here rather than wrapping;
/// C's `%d` conversion is undefined for a value out of range for `int`
/// and glibc wraps instead of failing, see
/// docs/issues/c-library-quirks.md.
fn scan_int(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

/// Scan a whole string as an `f64`, as C's `astSscanf( cval, " %lf %n" )`
/// (or `"%f"` for the `Float` target) plus the consumed-everything test
/// does.
///
/// `str::parse` does not accept C99 hexadecimal floating-point literals
/// (`0x1.8p2`), so those are tried first via [`parse_hex_float`], which
/// covers the forms glibc's `strtod`/`sscanf("%lf")` accept: an optional
/// sign, `0x`/`0X`, hex digits with an optional `.`, and an optional
/// binary exponent (`p`/`P`, optional sign, decimal digits).
pub(crate) fn scan_double(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(v) = parse_hex_float(t) {
        return Some(v);
    }
    t.parse::<f64>().ok()
}

/// Parse a C99 hex-float literal, requiring the whole string to match.
///
/// Grammar: `["+"|"-"] "0x"|"0X" <hexdigits> ["." <hexdigits>]
/// ["p"|"P" ["+"|"-"] <digits>]`, with at least one hex digit or a `.`
/// after the `0x`. A `p`/`P` exponent marker with no digits following it
/// is not part of the number (the whole string then fails to match,
/// since the trailing marker is unconsumed) — this backtracking mirrors
/// how `strtod` handles an exponent marker it cannot complete.
///
/// glibc's `sscanf("%lf")` additionally refuses to attach a `p` exponent
/// to a mantissa that has no hex digits at all (a bare `.` with nothing
/// on either side): `"0x.p9"` scans only `"0x."` (value `0`) and leaves
/// `"p9"` unconsumed, so the whole-string test fails and the conversion
/// is rejected. A mantissa with at least one hex digit does take the
/// exponent (`"0x1.p9"`, `"0x.5p9"`). This function reproduces that by
/// only attempting to consume a `p` exponent when the mantissa contained
/// at least one hex digit.
fn parse_hex_float(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    let neg = match b.first() {
        Some(b'+') => {
            i += 1;
            false
        }
        Some(b'-') => {
            i += 1;
            true
        }
        _ => false,
    };
    if !(i + 1 < b.len() && b[i] == b'0' && (b[i + 1] == b'x' || b[i + 1] == b'X')) {
        return None;
    }
    i += 2;

    let int_start = i;
    while i < b.len() && b[i].is_ascii_hexdigit() {
        i += 1;
    }
    let int_end = i;

    let mut has_dot = false;
    let mut frac_start = i;
    let mut frac_end = i;
    if i < b.len() && b[i] == b'.' {
        has_dot = true;
        i += 1;
        frac_start = i;
        while i < b.len() && b[i].is_ascii_hexdigit() {
            i += 1;
        }
        frac_end = i;
    }

    if int_start == int_end && !has_dot {
        // Bare "0x" with no digits and no ".": not a number.
        return None;
    }

    let mut value = 0.0f64;
    for c in s[int_start..int_end].chars() {
        value = value * 16.0 + f64::from(c.to_digit(16).expect("checked is_ascii_hexdigit"));
    }
    let mut scale = 1.0f64 / 16.0;
    for c in s[frac_start..frac_end].chars() {
        value += f64::from(c.to_digit(16).expect("checked is_ascii_hexdigit")) * scale;
        scale /= 16.0;
    }

    // Optional binary exponent. A "p"/"P" that turns out not to be
    // followed by any exponent digits is not consumed, matching
    // `strtod`'s backtracking. glibc's `sscanf("%lf")` additionally
    // never attaches the exponent to a mantissa with no hex digits at
    // all (bare "0x." has no digits before or after the dot), so gate
    // the attempt on the mantissa having contributed at least one digit.
    let has_mantissa_digit = int_end > int_start || frac_end > frac_start;
    let mut exp: i64 = 0;
    if has_mantissa_digit && i < b.len() && (b[i] == b'p' || b[i] == b'P') {
        let mut j = i + 1;
        let exp_neg = match b.get(j) {
            Some(b'+') => {
                j += 1;
                false
            }
            Some(b'-') => {
                j += 1;
                true
            }
            _ => false,
        };
        let digits_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > digits_start {
            if let Ok(mag) = s[digits_start..j].parse::<i64>() {
                exp = if exp_neg { -mag } else { mag };
                i = j;
            }
        }
    }

    if i != b.len() {
        // Did not consume the whole string.
        return None;
    }

    if value == 0.0 {
        return Some(if neg { -0.0 } else { 0.0 });
    }
    // `powi` saturates to +/-inf or 0.0 for exponents this extreme,
    // matching `strtod`'s overflow/underflow behaviour.
    let exp_i32 = exp.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let result = value * crate::math::powi(2.0f64, exp_i32);
    Some(if neg { -result } else { result })
}

/// Convert a double to an integer type, saturating rather than wrapping and
/// giving a NaN a defined value.
///
/// Mirrors C's `DtoI` / `DtoS` / `DtoK` / `DtoB` (keymap.c:1732-1770). C moved
/// to `round()` from `(int)(x + 0.5)` so that negative values go to the nearest
/// integer rather than toward zero, and added the range check because an
/// out-of-range floating-point-to-integer cast is undefined (C11 6.3.1.4p1).
/// A NaN has no integer value, so it yields zero: arbitrary, but defined.
///
/// `Bounds` carries the target's limits as doubles. `i64::MAX` is not exactly
/// representable as a double, so the caller compares against 2^63 directly.
struct Bounds {
    min: f64,
    max: f64,
}

fn d_to_int(dval: f64, b: &Bounds) -> f64 {
    if dval.is_nan() {
        return 0.0;
    }
    let r = dval.round();
    if r >= b.max {
        b.max
    } else if r <= b.min {
        b.min
    } else {
        r
    }
}

fn d_to_i32(dval: f64) -> i32 {
    d_to_int(
        dval,
        &Bounds {
            min: f64::from(i32::MIN),
            max: f64::from(i32::MAX),
        },
    ) as i32
}

fn d_to_i16(dval: f64) -> i16 {
    d_to_int(
        dval,
        &Bounds {
            min: f64::from(i16::MIN),
            max: f64::from(i16::MAX),
        },
    ) as i16
}

fn d_to_u8(dval: f64) -> u8 {
    d_to_int(
        dval,
        &Bounds {
            min: 0.0,
            max: f64::from(u8::MAX),
        },
    ) as u8
}

fn d_to_i64(dval: f64) -> i64 {
    // 2^63 exactly; `i64::MAX as f64` rounds up to it, so comparing against
    // the power of two is what C does and what is correct here.
    const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;
    if dval.is_nan() {
        return 0;
    }
    let r = dval.round();
    if r >= TWO_POW_63 {
        i64::MAX
    } else if r <= -TWO_POW_63 {
        i64::MIN
    } else {
        r as i64
    }
}

/// Convert `raw` to `out_type`.
///
/// `Ok(None)` is C's `result == 0`: the conversion is not possible and no
/// error is raised. The caller decides whether that is an `AST__MPGER`.
pub(crate) fn convert_value(raw: &RawValue, out_type: AstType) -> AstResult<Option<RawValue>> {
    // UNDEF pairs only with UNDEF (keymap.c:1744-1749).
    if raw.ast_type() == AstType::Undef || out_type == AstType::Undef {
        return Ok(
            if raw.ast_type() == AstType::Undef && out_type == AstType::Undef {
                Some(RawValue::Undef)
            } else {
                None
            },
        );
    }

    // An Object converts only to an Object, and nothing else converts to
    // one (keymap.c:2168-2214, plus the OBJECTTYPE arms of every other
    // raw_type branch).
    if raw.ast_type() == AstType::Object || out_type == AstType::Object {
        return Ok(match (raw, out_type) {
            (RawValue::Object(o), AstType::Object) => Some(RawValue::Object(o.clone())),
            _ => None,
        });
    }

    // From a string: scan. Every numeric target requires the scan to
    // consume the whole string (keymap.c:2055-2166).
    if let RawValue::String(s) = raw {
        return Ok(match out_type {
            AstType::String => Some(RawValue::String(s.clone())),
            AstType::Double => {
                if s.trim() == BAD_STRING {
                    Some(RawValue::Double(BAD_VALUE))
                } else {
                    scan_double(s).map(RawValue::Double)
                }
            }
            // The float path scans "%f" only: no integer-first attempt,
            // and no BAD_STRING check (keymap.c:2138-2146).
            AstType::Float => scan_double(s).map(|d| RawValue::Float(d as f32)),
            // C scans "%d" first and falls back to "%lf" when that
            // overflows (keymap.c:2160-2179); `scan_int` parses the widest
            // integer, so the fallback is folded into the saturating narrow.
            AstType::Int => scan_int(s)
                .map(|i| i as f64)
                .or_else(|| scan_double(s))
                .map(|d| RawValue::Int(d_to_i32(d))),
            AstType::SInt => scan_int(s)
                .map(|i| i as f64)
                .or_else(|| scan_double(s))
                .map(|d| RawValue::SInt(d_to_i16(d))),
            AstType::KInt => scan_int(s)
                .or_else(|| scan_double(s).map(d_to_i64))
                .map(RawValue::KInt),
            AstType::Byte => scan_int(s)
                .map(|i| i as f64)
                .or_else(|| scan_double(s))
                .map(|d| RawValue::Byte(d_to_u8(d))),
            AstType::Object | AstType::Undef => None,
            AstType::BadType => {
                unreachable!("no getter ever requests conversion to BadType")
            }
        });
    }

    // Everything left is numeric to numeric or numeric to string. A
    // floating-point source is rounded to nearest and saturated at the
    // target's limits (keymap.c:1732-1770); an integer source is narrowed
    // through the same saturating path so the two agree.
    let (as_i64, as_f64, is_float) = match raw {
        RawValue::Int(v) => (i64::from(*v), f64::from(*v), false),
        RawValue::SInt(v) => (i64::from(*v), f64::from(*v), false),
        RawValue::KInt(v) => (*v, *v as f64, false),
        RawValue::Byte(v) => (i64::from(*v), f64::from(*v), false),
        RawValue::Float(v) => (0, f64::from(*v), true),
        RawValue::Double(v) => (0, *v, true),
        RawValue::String(_) | RawValue::Object(_) | RawValue::Undef => unreachable!(),
    };
    let int_src = if is_float { as_f64 } else { as_i64 as f64 };

    Ok(Some(match out_type {
        AstType::Int => RawValue::Int(d_to_i32(int_src)),
        AstType::SInt => RawValue::SInt(d_to_i16(int_src)),
        AstType::KInt => RawValue::KInt(if is_float { d_to_i64(as_f64) } else { as_i64 }),
        AstType::Byte => RawValue::Byte(d_to_u8(int_src)),
        AstType::Float => RawValue::Float(as_f64 as f32),
        AstType::Double => RawValue::Double(as_f64),
        AstType::String => RawValue::String(match raw {
            RawValue::Int(v) => v.to_string(),
            RawValue::SInt(v) => v.to_string(),
            RawValue::KInt(v) => v.to_string(),
            RawValue::Byte(v) => v.to_string(),
            RawValue::Float(v) => float_to_string(*v),
            RawValue::Double(v) => double_to_string(*v),
            _ => unreachable!(),
        }),
        AstType::Object | AstType::Undef | AstType::BadType => unreachable!(),
    }))
}

/// Lift element `index` (zero-based) out of an entry.
pub(crate) fn element_at(value: &EntryValue, index: usize) -> Option<RawValue> {
    fn pick<T: Clone>(s: &Scalar1D<T>, i: usize) -> Option<T> {
        s.as_slice().get(i).cloned()
    }
    match value {
        EntryValue::Int(s) => pick(s, index).map(RawValue::Int),
        EntryValue::SInt(s) => pick(s, index).map(RawValue::SInt),
        EntryValue::KInt(s) => pick(s, index).map(RawValue::KInt),
        EntryValue::Byte(s) => pick(s, index).map(RawValue::Byte),
        EntryValue::Float(s) => pick(s, index).map(RawValue::Float),
        EntryValue::Double(s) => pick(s, index).map(RawValue::Double),
        EntryValue::String(s) => pick(s, index).map(RawValue::String),
        EntryValue::Object(s) => pick(s, index).map(RawValue::Object),
        // An undefined entry has no values, but C still lets index 0
        // through and reports the type as AST__UNDEFTYPE.
        EntryValue::Undef { .. } => {
            if index == 0 {
                Some(RawValue::Undef)
            } else {
                None
            }
        }
    }
}

/// Unwrap a `RawValue` already known (by the caller having requested that
/// exact `AstType`) to hold an `Int`. `convert_value` never returns a
/// `RawValue` whose variant differs from the requested `out_type`, so a
/// mismatch here is a programming error.
pub(crate) fn unwrap_int(raw: RawValue) -> i32 {
    match raw {
        RawValue::Int(v) => v,
        other => unreachable!("expected RawValue::Int, got {other:?}"),
    }
}

/// See [`unwrap_int`].
pub(crate) fn unwrap_sint(raw: RawValue) -> i16 {
    match raw {
        RawValue::SInt(v) => v,
        other => unreachable!("expected RawValue::SInt, got {other:?}"),
    }
}

/// See [`unwrap_int`].
pub(crate) fn unwrap_kint(raw: RawValue) -> i64 {
    match raw {
        RawValue::KInt(v) => v,
        other => unreachable!("expected RawValue::KInt, got {other:?}"),
    }
}

/// See [`unwrap_int`].
pub(crate) fn unwrap_byte(raw: RawValue) -> u8 {
    match raw {
        RawValue::Byte(v) => v,
        other => unreachable!("expected RawValue::Byte, got {other:?}"),
    }
}

/// See [`unwrap_int`].
pub(crate) fn unwrap_float(raw: RawValue) -> f32 {
    match raw {
        RawValue::Float(v) => v,
        other => unreachable!("expected RawValue::Float, got {other:?}"),
    }
}

/// See [`unwrap_int`].
pub(crate) fn unwrap_double(raw: RawValue) -> f64 {
    match raw {
        RawValue::Double(v) => v,
        other => unreachable!("expected RawValue::Double, got {other:?}"),
    }
}

/// See [`unwrap_int`].
pub(crate) fn unwrap_string(raw: RawValue) -> String {
    match raw {
        RawValue::String(v) => v,
        other => unreachable!("expected RawValue::String, got {other:?}"),
    }
}

/// See [`unwrap_int`]. The inner `Option` is C's stored NULL Object
/// pointer, not a conversion failure.
pub(crate) fn unwrap_object(raw: RawValue) -> Option<AnyObject> {
    match raw {
        RawValue::Object(v) => v.map(|b| *b),
        other => unreachable!("expected RawValue::Object, got {other:?}"),
    }
}

#[cfg(test)]
impl PartialEq for RawValue {
    /// Structural comparison, for tests only. Two `Object` payloads never
    /// compare equal: `AnyObject` has no equality relation.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (RawValue::Int(a), RawValue::Int(b)) => a == b,
            (RawValue::SInt(a), RawValue::SInt(b)) => a == b,
            (RawValue::KInt(a), RawValue::KInt(b)) => a == b,
            (RawValue::Byte(a), RawValue::Byte(b)) => a == b,
            (RawValue::Float(a), RawValue::Float(b)) => a == b,
            (RawValue::Double(a), RawValue::Double(b)) => a == b,
            (RawValue::String(a), RawValue::String(b)) => a == b,
            (RawValue::Undef, RawValue::Undef) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(raw: RawValue, to: AstType) -> Option<RawValue> {
        convert_value(&raw, to).unwrap()
    }

    #[test]
    fn undef_converts_only_to_undef() {
        assert!(matches!(
            conv(RawValue::Undef, AstType::Undef),
            Some(RawValue::Undef)
        ));
        assert_eq!(conv(RawValue::Undef, AstType::Int), None);
        assert_eq!(conv(RawValue::Int(1), AstType::Undef), None);
    }

    #[test]
    fn numeric_narrowing_saturates() {
        // C narrows through DtoI / DtoS / DtoB / DtoK (keymap.c:1732-1770),
        // which clamp at the target's limits rather than wrapping.
        assert_eq!(
            conv(RawValue::Int(1000), AstType::Byte),
            Some(RawValue::Byte(u8::MAX))
        );
        assert_eq!(
            conv(RawValue::Int(-1), AstType::Byte),
            Some(RawValue::Byte(0))
        );
        assert_eq!(
            conv(RawValue::Int(70000), AstType::SInt),
            Some(RawValue::SInt(i16::MAX))
        );
        assert_eq!(
            conv(RawValue::KInt(i64::from(i32::MAX) + 1), AstType::Int),
            Some(RawValue::Int(i32::MAX))
        );
        // Widening is exact.
        assert_eq!(
            conv(RawValue::SInt(-5), AstType::KInt),
            Some(RawValue::KInt(-5))
        );
        assert_eq!(
            conv(RawValue::Byte(255), AstType::Int),
            Some(RawValue::Int(255))
        );
    }

    #[test]
    fn float_to_integer_rounds_to_nearest() {
        // C rounds with round(), which goes to the nearest integer in both
        // directions — half away from zero (keymap.c:1735, upstream
        // 7b084bbf). `(int)(dval + 0.5)` used to round -2.5 toward zero.
        assert_eq!(
            conv(RawValue::Double(2.5), AstType::Int),
            Some(RawValue::Int(3))
        );
        assert_eq!(
            conv(RawValue::Double(2.4), AstType::Int),
            Some(RawValue::Int(2))
        );
        assert_eq!(
            conv(RawValue::Double(-2.5), AstType::Int),
            Some(RawValue::Int(-3))
        );
        assert_eq!(
            conv(RawValue::Double(-2.6), AstType::Int),
            Some(RawValue::Int(-3))
        );
        assert_eq!(
            conv(RawValue::Float(2.5), AstType::KInt),
            Some(RawValue::KInt(3))
        );
    }

    #[test]
    fn double_to_string_drops_rounding_noise() {
        // C formats at DBL_DIG-3 and DBL_DIG significant digits, and keeps
        // the reduced form when full precision costs more than nine extra
        // characters.
        assert_eq!(double_to_string(1.0), "1");
        assert_eq!(double_to_string(-1.5), "-1.5");
        assert_eq!(double_to_string(0.0), "0");
        // 0.1 is not exactly representable: at full precision the string is
        // "0.10000000000000001", 16 characters longer than the reduced
        // form "0.1" — that saving is over C's nine-character threshold, so
        // the reduced form wins.
        assert_eq!(double_to_string(0.1), "0.1");
        assert_eq!(double_to_string(crate::constants::BAD_VALUE), BAD_STRING);
    }

    #[test]
    fn double_to_string_keeps_the_sign_of_negative_zero() {
        // Unlike WriteDouble (channel.c:4444-4452), ConvertValue's
        // Double-to-String arm (keymap.c:1975-1988) has no "-0" -> "0"
        // fixup, so -0.0 formats with its sign.
        assert_eq!(double_to_string(-0.0), "-0");
        assert_eq!(double_to_string(0.0), "0");
    }

    #[test]
    fn float_to_string_uses_six_significant_digits() {
        // C: sprintf( buff, "%.*g", FLT_DIG, fval ) with FLT_DIG == 6, and
        // no AST__BAD check on this path.
        assert_eq!(float_to_string(1.0), "1");
        assert_eq!(float_to_string(1.5), "1.5");
        assert_eq!(float_to_string(1.0 / 3.0), "0.333333");
    }

    #[test]
    fn float_to_string_keeps_the_sign_of_negative_zero() {
        // keymap.c:2035-2037 has no "-0" -> "0" fixup either.
        assert_eq!(float_to_string(-0.0f32), "-0");
        assert_eq!(float_to_string(0.0f32), "0");
    }

    #[test]
    fn int_to_string_is_plain_decimal() {
        assert_eq!(
            conv(RawValue::Int(-42), AstType::String),
            Some(RawValue::String("-42".to_string()))
        );
        assert_eq!(
            conv(RawValue::KInt(i64::MIN), AstType::String),
            Some(RawValue::String(i64::MIN.to_string()))
        );
        assert_eq!(
            conv(RawValue::Byte(7), AstType::String),
            Some(RawValue::String("7".to_string()))
        );
        assert_eq!(
            conv(RawValue::SInt(-7), AstType::String),
            Some(RawValue::String("-7".to_string()))
        );
    }

    #[test]
    fn string_to_numeric_tries_int_then_double() {
        assert_eq!(
            conv(RawValue::String("42".into()), AstType::Int),
            Some(RawValue::Int(42))
        );
        // Leading and trailing white space is skipped by the scan.
        assert_eq!(
            conv(RawValue::String("  42 ".into()), AstType::Int),
            Some(RawValue::Int(42))
        );
        // Not an integer, but a valid double: the fallback rounds it.
        assert_eq!(
            conv(RawValue::String("2.6".into()), AstType::Int),
            Some(RawValue::Int(3))
        );
        // Trailing junk means the scan did not consume the string.
        assert_eq!(conv(RawValue::String("42x".into()), AstType::Int), None);
        assert_eq!(conv(RawValue::String("".into()), AstType::Int), None);
        assert_eq!(
            conv(RawValue::String("hello".into()), AstType::Double),
            None
        );
        assert_eq!(
            conv(RawValue::String("1.5".into()), AstType::Double),
            Some(RawValue::Double(1.5))
        );
    }

    #[test]
    fn string_to_numeric_accepts_c99_hex_notation() {
        // C's "%d" scan of "0x10" only consumes "0" (rejecting the hex
        // notation), so the whole-string test fails and the "%lf"
        // fallback runs instead — "%lf" does accept hex floats, so the
        // value still comes through, rounded like any other double.
        assert_eq!(
            conv(RawValue::String("0x10".into()), AstType::Int),
            Some(RawValue::Int(16))
        );
        assert_eq!(
            conv(RawValue::String("0x10".into()), AstType::Double),
            Some(RawValue::Double(16.0))
        );
        assert_eq!(
            conv(RawValue::String("0x1p4".into()), AstType::Double),
            Some(RawValue::Double(16.0))
        );
        assert_eq!(
            conv(RawValue::String("0x1.8p2".into()), AstType::Double),
            Some(RawValue::Double(6.0))
        );
        assert_eq!(
            conv(RawValue::String("-0x10".into()), AstType::Double),
            Some(RawValue::Double(-16.0))
        );
        assert_eq!(
            conv(RawValue::String("0x1p4".into()), AstType::Float),
            Some(RawValue::Float(16.0))
        );
    }

    #[test]
    fn string_to_double_rejects_hex_exponent_on_bare_dot_mantissa() {
        // glibc's `sscanf("%lf")` never attaches a "p" exponent to a
        // mantissa with no hex digits at all: "0x.p9" scans only "0x."
        // (value 0) and leaves "p9" unconsumed, so the whole-string test
        // fails and the conversion is refused. A mantissa with at least
        // one hex digit, on either side of the dot, does take the
        // exponent.
        assert_eq!(
            conv(RawValue::String("0x.p9".into()), AstType::Double),
            None
        );
        assert_eq!(
            conv(RawValue::String("0x1.p9".into()), AstType::Double),
            Some(RawValue::Double(512.0))
        );
        assert_eq!(
            conv(RawValue::String("0x.5p9".into()), AstType::Double),
            Some(RawValue::Double(160.0))
        );
    }

    #[test]
    fn string_to_numeric_keeps_currently_correct_behaviours() {
        // Regression coverage for the forms that already worked, so the
        // hex-float addition to `scan_double` cannot silently narrow
        // what is accepted.
        assert_eq!(
            conv(RawValue::String("+5".into()), AstType::Int),
            Some(RawValue::Int(5))
        );
        assert_eq!(
            conv(RawValue::String("1e5".into()), AstType::Int),
            Some(RawValue::Int(100_000))
        );
        assert_eq!(
            conv(RawValue::String("2.6".into()), AstType::Int),
            Some(RawValue::Int(3))
        );
        assert_eq!(
            conv(RawValue::String("-2.5".into()), AstType::Int),
            Some(RawValue::Int(-3))
        );
        assert_eq!(
            conv(RawValue::String(" 42 ".into()), AstType::Int),
            Some(RawValue::Int(42))
        );
        assert_eq!(
            conv(RawValue::String(BAD_STRING.into()), AstType::Int),
            None
        );
        assert_eq!(
            conv(RawValue::String("  <bad> ".into()), AstType::Int),
            None
        );
        assert_eq!(
            conv(RawValue::String("inf".into()), AstType::Double),
            Some(RawValue::Double(f64::INFINITY))
        );
        assert!(matches!(
            conv(RawValue::String("nan".into()), AstType::Double),
            Some(RawValue::Double(d)) if d.is_nan()
        ));
        assert_eq!(conv(RawValue::String("1 2".into()), AstType::Int), None);
        assert_eq!(conv(RawValue::String("12abc".into()), AstType::Int), None);
        assert_eq!(conv(RawValue::String("   ".into()), AstType::Int), None);
        assert_eq!(conv(RawValue::String("".into()), AstType::Int), None);
    }

    #[test]
    fn string_to_double_accepts_the_bad_marker() {
        let got = conv(RawValue::String(BAD_STRING.into()), AstType::Double);
        assert_eq!(got, Some(RawValue::Double(crate::constants::BAD_VALUE)));
        // Only on the double path: the marker is not a float or an int.
        assert_eq!(
            conv(RawValue::String(BAD_STRING.into()), AstType::Float),
            None
        );
        assert_eq!(
            conv(RawValue::String(BAD_STRING.into()), AstType::Int),
            None
        );
    }

    #[test]
    fn string_to_float_has_no_integer_fallback() {
        // C scans "%f" only on this path, which still accepts "42"
        // because %f matches an integer literal.
        assert_eq!(
            conv(RawValue::String("42".into()), AstType::Float),
            Some(RawValue::Float(42.0))
        );
        assert_eq!(conv(RawValue::String("42x".into()), AstType::Float), None);
    }

    #[test]
    fn string_to_string_is_the_identity() {
        assert_eq!(
            conv(RawValue::String("Hello".into()), AstType::String),
            Some(RawValue::String("Hello".to_string()))
        );
    }

    #[test]
    fn object_converts_only_to_object() {
        // `RawValue::Object` wraps `Option<Box<AnyObject>>` (not a bare
        // `Box<AnyObject>`), matching `entry::EntryValue::Object`'s payload
        // — it is what makes a NULL Object element representable, per C's
        // `aval ? astClone(aval) : NULL` at keymap.c:2202.
        let obj = RawValue::Object(Some(Box::new(AnyObject::UnitMap(
            crate::mappings::UnitMap::new(1),
        ))));
        assert!(matches!(
            convert_value(&obj, AstType::Object).unwrap(),
            Some(RawValue::Object(_))
        ));
        for to in [
            AstType::Int,
            AstType::SInt,
            AstType::KInt,
            AstType::Byte,
            AstType::Float,
            AstType::Double,
            AstType::String,
            AstType::Undef,
        ] {
            assert_eq!(convert_value(&obj, to).unwrap(), None, "{to:?}");
        }
        // Nothing converts *to* an Object either.
        assert_eq!(conv(RawValue::Int(1), AstType::Object), None);
        assert_eq!(conv(RawValue::String("x".into()), AstType::Object), None);
    }

    #[test]
    fn element_at_indexes_scalars_and_vectors_zero_based() {
        let s = EntryValue::Int(Scalar1D::Scalar(7));
        assert_eq!(element_at(&s, 0), Some(RawValue::Int(7)));
        assert_eq!(element_at(&s, 1), None);

        let v = EntryValue::Double(Scalar1D::Vector(vec![1.0, 2.0, 3.0]));
        assert_eq!(element_at(&v, 0), Some(RawValue::Double(1.0)));
        assert_eq!(element_at(&v, 2), Some(RawValue::Double(3.0)));
        assert_eq!(element_at(&v, 3), None);

        // An undefined entry yields Undef at any in-range index; C treats
        // it as having `nel` elements with no values.
        assert_eq!(
            element_at(&EntryValue::Undef { nel: 0 }, 0),
            Some(RawValue::Undef)
        );
    }

    #[test]
    fn double_to_narrow_int_saturates_and_defines_nan() {
        // An out-of-range floating-point-to-integer cast is undefined in C
        // (C11 6.3.1.4p1), so the range check saturates instead
        // (keymap.c:1732-1770, upstream 6d114495). A NaN has no integer
        // value at all and yields a defined zero (4e5c33dc).
        let d = RawValue::Double(5_000_000_100.0);
        assert_eq!(conv(d.clone(), AstType::Int), Some(RawValue::Int(i32::MAX)));
        assert_eq!(
            conv(d.clone(), AstType::SInt),
            Some(RawValue::SInt(i16::MAX))
        );
        assert_eq!(
            conv(d.clone(), AstType::Byte),
            Some(RawValue::Byte(u8::MAX))
        );
        // The wide target is in range, so no clamp applies.
        assert_eq!(conv(d, AstType::KInt), Some(RawValue::KInt(5_000_000_100)));

        let neg = RawValue::Double(-5_000_000_100.0);
        assert_eq!(
            conv(neg.clone(), AstType::Int),
            Some(RawValue::Int(i32::MIN))
        );
        assert_eq!(conv(neg, AstType::Byte), Some(RawValue::Byte(0)));

        // 2^63 is not representable as an i64, so the KInt clamp is the one
        // that needs the power-of-two comparison rather than `i64::MAX as f64`.
        assert_eq!(
            conv(RawValue::Double(1.0e30), AstType::KInt),
            Some(RawValue::KInt(i64::MAX))
        );
        assert_eq!(
            conv(RawValue::Double(-1.0e30), AstType::KInt),
            Some(RawValue::KInt(i64::MIN))
        );

        let nan = RawValue::Double(f64::NAN);
        assert_eq!(conv(nan.clone(), AstType::Int), Some(RawValue::Int(0)));
        assert_eq!(conv(nan.clone(), AstType::SInt), Some(RawValue::SInt(0)));
        assert_eq!(conv(nan.clone(), AstType::KInt), Some(RawValue::KInt(0)));
        assert_eq!(conv(nan, AstType::Byte), Some(RawValue::Byte(0)));
    }

    #[test]
    fn string_to_int_overflow_saturates() {
        // C detects the "%d" scan overflowing and falls back to the "%lf"
        // scan, whose result then saturates (keymap.c:2160-2179, upstream
        // a76964c7). glibc used to wrap modulo 2^32 instead, giving
        // 1410065407 for the first of these and -1 for the second.
        assert_eq!(
            conv(RawValue::String("9999999999".into()), AstType::Int),
            Some(RawValue::Int(i32::MAX))
        );
        let nines38 = "9".repeat(38);
        assert_eq!(
            conv(RawValue::String(nines38), AstType::Int),
            Some(RawValue::Int(i32::MAX))
        );
    }
}

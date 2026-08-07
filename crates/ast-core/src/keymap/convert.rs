//! `ConvertValue`: the KeyMap type-conversion matrix.
//!
//! Every `map_get`, `map_get_1` and `map_get_elem` call routes through
//! here, so the whole 9-by-9 matrix is present. A pairing C rejects
//! returns `Ok(None)`, which callers turn into `AST__MPGER`.

use crate::constants::BAD_VALUE;
use crate::error::AstResult;
use crate::keymap::entry::{AstType, EntryValue, Scalar1D};
use crate::serial::AnyObject;
use crate::serial::native_write::format_double_g;

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
pub(crate) fn double_to_string(value: f64) -> String {
    if value == BAD_VALUE {
        return BAD_STRING.to_string();
    }
    let reduced = format_double_g(value, DBL_DIG - 3);
    let full = format_double_g(value, DBL_DIG);
    if full.len() as isize - reduced.len() as isize > 9 {
        reduced
    } else {
        full
    }
}

/// Format a float the way `ConvertValue` does: `%.*g` at `FLT_DIG`
/// significant digits, with no `AST__BAD` check on this path
/// (`keymap.c:2035-2037`).
pub(crate) fn float_to_string(value: f32) -> String {
    format_double_g(f64::from(value), FLT_DIG)
}

/// Scan a whole string as an `i64`, as C's `astSscanf( cval, " %d %n" )`
/// followed by the "did it consume everything" test does.
fn scan_int(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

/// Scan a whole string as an `f64`, as C's `astSscanf( cval, " %lf %n" )`
/// plus the consumed-everything test does.
fn scan_double(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

/// C's `(int)( x + 0.5 )`: add a half, then truncate toward zero. This is
/// not `round()` — the two disagree for negative half-integers.
fn round_c(x: f64) -> f64 {
    (x + 0.5).trunc()
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
            AstType::Int => scan_int(s)
                .map(|i| i as i32)
                .or_else(|| scan_double(s).map(|d| round_c(d) as i32))
                .map(RawValue::Int),
            AstType::SInt => scan_int(s)
                .map(|i| i as i16)
                .or_else(|| scan_double(s).map(|d| round_c(d) as i16))
                .map(RawValue::SInt),
            AstType::KInt => scan_int(s)
                .or_else(|| scan_double(s).map(|d| round_c(d) as i64))
                .map(RawValue::KInt),
            AstType::Byte => scan_int(s)
                .map(|i| i as u8)
                .or_else(|| scan_double(s).map(|d| round_c(d) as u8))
                .map(RawValue::Byte),
            AstType::Object | AstType::Undef => None,
        });
    }

    // Everything left is numeric to numeric or numeric to string. C uses
    // a plain cast in every numeric case, including the narrowing ones,
    // and rounds by adding a half when the source is floating point
    // (keymap.c:1751-2053).
    let (as_i64, as_f64, is_float) = match raw {
        RawValue::Int(v) => (i64::from(*v), f64::from(*v), false),
        RawValue::SInt(v) => (i64::from(*v), f64::from(*v), false),
        RawValue::KInt(v) => (*v, *v as f64, false),
        RawValue::Byte(v) => (i64::from(*v), f64::from(*v), false),
        RawValue::Float(v) => (0, f64::from(*v), true),
        RawValue::Double(v) => (0, *v, true),
        RawValue::String(_) | RawValue::Object(_) | RawValue::Undef => unreachable!(),
    };
    let int_src = if is_float {
        round_c(as_f64) as i64
    } else {
        as_i64
    };

    Ok(Some(match out_type {
        AstType::Int => RawValue::Int(int_src as i32),
        AstType::SInt => RawValue::SInt(int_src as i16),
        AstType::KInt => RawValue::KInt(int_src),
        AstType::Byte => RawValue::Byte(int_src as u8),
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
        AstType::Object | AstType::Undef => unreachable!(),
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
    fn numeric_narrowing_wraps_like_a_c_cast() {
        // C: *( (unsigned char *) out ) = ival; — a plain truncating cast,
        // not a range check.
        assert_eq!(
            conv(RawValue::Int(1000), AstType::Byte),
            Some(RawValue::Byte(232))
        );
        assert_eq!(
            conv(RawValue::Int(70000), AstType::SInt),
            Some(RawValue::SInt(4464))
        );
        assert_eq!(
            conv(RawValue::KInt(i64::from(i32::MAX) + 1), AstType::Int),
            Some(RawValue::Int(i32::MIN))
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
    fn float_to_integer_adds_a_half_then_truncates() {
        // C: (int)( dval + 0.5 ). This differs from round() below zero.
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
            Some(RawValue::Int(-2))
        );
        assert_eq!(
            conv(RawValue::Double(-2.6), AstType::Int),
            Some(RawValue::Int(-2))
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
    fn float_to_string_uses_six_significant_digits() {
        // C: sprintf( buff, "%.*g", FLT_DIG, fval ) with FLT_DIG == 6, and
        // no AST__BAD check on this path.
        assert_eq!(float_to_string(1.0), "1");
        assert_eq!(float_to_string(1.5), "1.5");
        assert_eq!(float_to_string(1.0 / 3.0), "0.333333");
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
}

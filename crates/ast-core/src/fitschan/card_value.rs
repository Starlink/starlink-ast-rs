//! Converting a card's value to the type a caller asks for: C's `CnvType`
//! (fitschan.c), used by the `astGetFits<X>` getters.

use super::card::FitsValue;
use super::format::round_fstring;
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::axis_format::{c_strtod, is_c_space, sprintf_double};

/// The text C stores for a bad floating value (fitschan.c `BAD_STRING`).
const BAD_STRING: &str = "<bad>";

/// The type a getter asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Want {
    Int,
    KInt,
    Float,
    String,
    Continue,
    Logical,
    ComplexF,
    ComplexI,
}

impl Want {
    /// C's name for the type in error messages (`type_names`).
    pub(crate) fn c_name(self) -> &'static str {
        match self {
            Want::Int => "integer",
            Want::KInt => "64 bit integer",
            Want::Float => "floating point",
            Want::String => "string",
            Want::Continue => "continuation string",
            Want::Logical => "logical",
            Want::ComplexF => "complex floating point",
            Want::ComplexI => "complex integer",
        }
    }
}

/// A converted value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Got {
    Int(i32),
    KInt(i64),
    Float(f64),
    Str(String),
    Logical(bool),
    ComplexF(f64, f64),
    ComplexI(i32, i32),
}

/// C's `CnvType` for a getter: the card's value (`None` when there is no
/// current card) converted to `want`. `Ok(None)` is C's failed conversion,
/// which the caller reports as AST__FTCNV; an undefined value is
/// AST__FUNDEF. `fitsrnd` is the FitsRounding attribute, used when a
/// floating value becomes a string.
pub(crate) fn cnv_type(
    value: Option<&FitsValue>,
    want: Want,
    fitsrnd: usize,
    name: &str,
) -> AstResult<Option<Got>> {
    let Some(value) = value else {
        // No current card: no data, and not a commentary card.
        return Ok(None);
    };
    let got = match value {
        FitsValue::Undef => {
            return Err(AstError::new(
                ErrorCode::Fundef,
                format!("The FITS keyword '{name}' has an undefined value."),
            ));
        }
        // A commentary card has no value to convert.
        FitsValue::Comment(_) => return Ok(None),
        FitsValue::Float(v) => from_float(*v, want, fitsrnd),
        FitsValue::String(s) | FitsValue::Continue(s) => from_string(s, want),
        FitsValue::Int(v) => from_int(*v as i32 as i64, want),
        FitsValue::KInt(v) => from_int(*v, want),
        FitsValue::Logical(b) => Some(match want {
            Want::Float => Got::Float(if *b { 1.0 } else { 0.0 }),
            Want::String | Want::Continue => Got::Str(if *b { "Y" } else { "N" }.to_string()),
            Want::Int => Got::Int(i32::from(*b)),
            Want::KInt => Got::KInt(i64::from(*b)),
            Want::Logical => Got::Logical(*b),
            Want::ComplexF => Got::ComplexF(if *b { 1.0 } else { 0.0 }, 0.0),
            Want::ComplexI => Got::ComplexI(i32::from(*b), 0),
        }),
        FitsValue::ComplexFloat(re, im) => Some(match want {
            Want::Float => Got::Float(*re),
            Want::String | Want::Continue => Got::Str(format!(
                "{} {}",
                float_text(*re, fitsrnd),
                float_text(*im, fitsrnd)
            )),
            Want::Int => Got::Int(*re as i32),
            Want::KInt => Got::KInt(*re as i64),
            Want::Logical => Got::Logical(*re != 0.0),
            Want::ComplexF => Got::ComplexF(*re, *im),
            Want::ComplexI => Got::ComplexI(*re as i32, *im as i32),
        }),
        FitsValue::ComplexInt(re, im) => {
            let (re, im) = (*re as i32, *im as i32);
            Some(match want {
                Want::Float => Got::Float(f64::from(re)),
                Want::String | Want::Continue => Got::Str(format!("{re} {im}")),
                Want::Int => Got::Int(re),
                Want::KInt => Got::KInt(i64::from(re)),
                Want::Logical => Got::Logical(re != 0),
                Want::ComplexF => Got::ComplexF(f64::from(re), f64::from(im)),
                Want::ComplexI => Got::ComplexI(re, im),
            })
        }
    };
    Ok(got)
}

/// A floating value as C writes it for a string: `%.17g`, then `CheckZero`
/// (no sign on zero, long runs of 0s and 9s rounded out).
fn float_text(v: f64, fitsrnd: usize) -> String {
    if v == BAD_VALUE {
        return BAD_STRING.to_string();
    }
    let text = sprintf_double("%.17g", v);
    if v == 0.0 {
        text.replacen('-', " ", 1)
    } else {
        round_fstring(&text, fitsrnd)
    }
}

fn from_float(v: f64, want: Want, fitsrnd: usize) -> Option<Got> {
    Some(match want {
        Want::Float => Got::Float(v),
        Want::String | Want::Continue => Got::Str(float_text(v, fitsrnd)),
        Want::Int => Got::Int(v as i32),
        Want::KInt => Got::KInt(v as i64),
        Want::Logical => Got::Logical(v != 0.0),
        Want::ComplexF => Got::ComplexF(v, 0.0),
        Want::ComplexI => Got::ComplexI(v as i32, 0),
    })
}

/// An integer held as C's `int` or `int64_t`. `Int` cards hold values that
/// fit an `int`, so the caller narrows them first.
fn from_int(v: i64, want: Want) -> Option<Got> {
    Some(match want {
        Want::Float => Got::Float(v as f64),
        Want::String | Want::Continue => Got::Str(v.to_string()),
        Want::Int => Got::Int(v as i32),
        Want::KInt => Got::KInt(v),
        Want::Logical => Got::Logical(v != 0),
        Want::ComplexF => Got::ComplexF(v as f64, 0.0),
        Want::ComplexI => Got::ComplexI(v as i32, 0),
    })
}

fn from_string(s: &str, want: Want) -> Option<Got> {
    match want {
        Want::Float => {
            let t = skip_space(s);
            if t.trim_end_matches(|c: char| is_c_space(c as u8)) == BAD_STRING {
                return Some(Got::Float(BAD_VALUE));
            }
            scan_all(s, 1, scan_double).map(|v| Got::Float(v[0]))
        }
        Want::String | Want::Continue => Some(Got::Str(s.chars().take(80).collect())),
        Want::Int => scan_all(s, 1, scan_int).map(|v| Got::Int(v[0] as i32)),
        Want::KInt => scan_all(s, 1, scan_int).map(|v| Got::KInt(v[0])),
        Want::Logical => {
            if let Some(v) = scan_all(s, 1, scan_int) {
                return Some(Got::Logical(v[0] as i32 != 0));
            }
            let b = skip_space(s).as_bytes();
            let first = b.first().copied().unwrap_or(0);
            let second = b.get(1).copied().unwrap_or(0);
            if matches!(first, b'y' | b'Y' | b't' | b'T')
                || (first == b'.' && matches!(second, b't' | b'T'))
            {
                Some(Got::Logical(true))
            } else if matches!(first, b'n' | b'N' | b'f' | b'F')
                || (first == b'.' && matches!(second, b'f' | b'F'))
            {
                Some(Got::Logical(false))
            } else {
                None
            }
        }
        Want::ComplexF => scan_all(s, 2, scan_double)
            .map(|v| Got::ComplexF(v[0], v[1]))
            .or_else(|| scan_all(s, 1, scan_double).map(|v| Got::ComplexF(v[0], 0.0))),
        Want::ComplexI => scan_all(s, 2, scan_int)
            .map(|v| Got::ComplexI(v[0] as i32, v[1] as i32))
            .or_else(|| scan_all(s, 1, scan_int).map(|v| Got::ComplexI(v[0] as i32, 0))),
    }
}

fn skip_space(s: &str) -> &str {
    s.trim_start_matches(|c: char| is_c_space(c as u8))
}

/// C's `sscanf( s, "%X %X ... %n" )` with `n` conversions followed by the
/// test that every character was used (`nc >= len`).
fn scan_all<T>(s: &str, n: usize, scan: fn(&str) -> Option<(T, usize)>) -> Option<Vec<T>> {
    let mut rest = s;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (v, used) = scan(rest)?;
        out.push(v);
        rest = skip_space(&rest[used..]);
    }
    rest.is_empty().then_some(out)
}

/// One `%lf` conversion: leading white space, then C's `strtod`.
fn scan_double(s: &str) -> Option<(f64, usize)> {
    let lead = s.len() - skip_space(s).len();
    let (v, n) = c_strtod(&s.as_bytes()[lead..])?;
    Some((v, lead + n))
}

/// One `%d` or `%PRId64` conversion: leading white space, an optional sign
/// and decimal digits. Like the C library's `strtoimax`, a value beyond the
/// 64-bit range saturates; a `%d` caller then narrows it to an `int` as C's
/// assignment does.
fn scan_int(s: &str) -> Option<(i64, usize)> {
    let b = s.as_bytes();
    let mut i = s.len() - skip_space(s).len();
    let neg = match b.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let start = i;
    let mut v: i128 = 0;
    while let Some(d) = b.get(i).filter(|c| c.is_ascii_digit()) {
        v = (v * 10 + i128::from(d - b'0')).min(i128::from(i64::MAX) + 1);
        i += 1;
    }
    if i == start {
        return None;
    }
    let v = if neg { -v } else { v };
    let v = v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64;
    Some((v, i))
}

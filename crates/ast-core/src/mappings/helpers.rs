//! Shared utilities for Mapping implementations.

use crate::constants::BAD_VALUE;

#[inline]
pub fn is_bad(v: f64) -> bool {
    v == BAD_VALUE || !v.is_finite()
}

#[inline]
pub fn point_has_bad(point: &[f64]) -> bool {
    point.iter().any(|&v| is_bad(v))
}

/// Compare two f64 values with AST's standard tolerance.
///
/// Matches the C library's `astEQUAL` macro:
/// `|a - b| <= max(1e5 * (|a| + |b|) * f64::EPSILON, 1e-12)`
///
/// Both BAD values are considered equal. One BAD + one non-BAD are not.
#[inline]
pub fn ast_equal(a: f64, b: f64) -> bool {
    if a == BAD_VALUE {
        return b == BAD_VALUE;
    }
    if b == BAD_VALUE {
        return false;
    }
    let tol = (1.0e5 * (a.abs() + b.abs()) * f64::EPSILON).max(1.0e-12);
    (a - b).abs() <= tol
}

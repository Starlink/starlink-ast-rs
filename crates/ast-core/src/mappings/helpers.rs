//! Shared utilities for Mapping implementations.

use crate::constants::BAD_VALUE;

#[inline]
pub fn is_bad(v: f64) -> bool {
    v == BAD_VALUE || v.is_nan()
}

#[inline]
pub fn point_has_bad(point: &[f64]) -> bool {
    point.iter().any(|&v| is_bad(v))
}

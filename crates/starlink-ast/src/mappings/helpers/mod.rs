// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Shared utilities for Mapping implementations.

pub mod analyse_poly;
pub(crate) mod poly_tran;
pub mod scale_poly_inputs;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, PermMap};

/// Whether `v` is bad: `BAD_VALUE` or not finite.
///
/// Both tests are always made (`|`, not `||`). With a short-circuit, the
/// compiler turns a kernel's `if bad { BAD_VALUE } else { .. }` select back
/// into a branch that skips the arithmetic, and the loop no longer vectorizes.
#[inline]
pub fn is_bad(v: f64) -> bool {
    (v == BAD_VALUE) | !v.is_finite()
}

#[inline]
pub fn point_has_bad(point: &[f64]) -> bool {
    point.iter().any(|&v| is_bad(v))
}

/// Compare two f64 values with a scalable tolerance.
///
/// Matches the C library's `astEQUALS(a, b, tol)` macro:
/// `|a - b| <= max(tol * ((|a| + |b|) * f64::EPSILON), 1e-12)`
///
/// The operands are scaled by `f64::EPSILON` before the tolerance multiplier
/// is applied. Multiplying first overflows for operands near the top of the
/// double range — `1e5 * f64::MAX` is infinity, and an infinite tolerance
/// makes every comparison report equality.
///
/// Both BAD values are considered equal. One BAD + one non-BAD are not.
#[inline]
pub fn ast_equals(a: f64, b: f64, tol: f64) -> bool {
    if a == BAD_VALUE {
        return b == BAD_VALUE;
    }
    if b == BAD_VALUE {
        return false;
    }
    let eps = (tol * ((a.abs() + b.abs()) * f64::EPSILON)).max(1.0e-12);
    (a - b).abs() <= eps
}

/// Compare two f64 values with AST's standard tolerance.
///
/// Matches the C library's `astEQUAL` macro (`astEQUALS(a, b, 1e5)`):
/// `|a - b| <= max(1e5 * ((|a| + |b|) * f64::EPSILON), 1e-12)`
///
/// Both BAD values are considered equal. One BAD + one non-BAD are not.
#[inline]
pub fn ast_equal(a: f64, b: f64) -> bool {
    ast_equals(a, b, 1.0e5)
}

/// Whether every entry of `in_axes` is in `0..nin` and none is repeated:
/// `Some(())` if so. C's base `MapSplit` splits a full selection only when
/// it is such a permutation (mapping.c), which [`full_set_split`] tests with
/// this.
pub(super) fn validate_in_axes(in_axes: &[usize], nin: usize) -> Option<()> {
    for (i, &ix) in in_axes.iter().enumerate() {
        if ix >= nin || in_axes[..i].contains(&ix) {
            return None;
        }
    }
    Some(())
}

/// The form of `s` that C's `astChrMatch` (memory.c) compares: matching is
/// case-insensitive and ignores trailing white space, as C's `isspace`
/// defines it. Matching the upper-cased result against upper-case names is
/// `astChrMatch` against those names.
pub(crate) fn chr_match_key(s: &str) -> String {
    s.trim_end_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r'])
        .to_ascii_uppercase()
}

/// C's base `MapSplit` (mapping.c): an index out of range is
/// `ErrorCode::Axiin`, and otherwise [`full_set_split`]. The classes whose C
/// `MapSplit` calls the parent first call this first.
pub(crate) fn base_map_split<M: Mapping + ?Sized>(
    this: &M,
    in_axes: &[usize],
) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
    // C allocates the returned output indices with astMalloc, which
    // gives NULL for none, so an empty selection gives no split.
    if in_axes.is_empty() {
        return Ok(None);
    }
    let mapnin = this.nin();
    if let Some(&bad) = in_axes.iter().find(|&&ix| ix >= mapnin) {
        return Err(AstError::new(
            ErrorCode::Axiin,
            format!(
                "astMapSplit: one of the supplied Mapping input indices has value {} \
                 which is invalid; it should be in the range 1 to {mapnin}",
                bad.wrapping_add(1) as isize,
            ),
        ));
    }
    Ok(full_set_split(this, in_axes))
}

/// Default `Mapping::map_split` body for the full-input case
/// (`in_axes.len() == this.nin()`). Mirrors `mapping.c::MapSplit`
/// (~line 7902): identity-perm fast path returns `this.clone_box()`,
/// other permutations build a `PermMap → this` composition and try
/// `simplified()`. Returns `None` for partial input (caller must
/// dispatch to its per-class partial-input logic).
pub(super) fn full_set_split<M: Mapping + ?Sized>(
    this: &M,
    in_axes: &[usize],
) -> Option<(Vec<usize>, DynMapping)> {
    let mapnin = this.nin();
    if in_axes.len() != mapnin {
        return None;
    }
    validate_in_axes(in_axes, mapnin)?;

    // The supplied `in_axes` array can act as the PermMap's "inperm"
    // array (the inverse-direction perm): for output i of the inverse
    // transform, take input axis `in_axes[i]` of the new mapping.
    // The forward direction needs the matching "outperm": for output i
    // of the forward (= original-mapping input axis i), take input
    // position `outperm[i]` of the new mapping. Build it by inversion:
    //     outperm[in_axes[iin]] = iin
    // This also lets us check that every original input axis is filled
    // exactly once and detect the identity-permutation fast path.
    let mut outperm_zb = vec![usize::MAX; mapnin];
    let mut perm_needed = false;
    for (iin, &iout) in in_axes.iter().enumerate() {
        outperm_zb[iout] = iin;
        if iout != iin {
            perm_needed = true;
        }
    }
    // (Validation above guarantees no duplicates / out-of-range, so
    //  every slot of outperm_zb is now filled. No need to re-check.)

    let nout = this.nout();
    let out_axes: Vec<usize> = (0..nout).collect();

    let sub: DynMapping = if !perm_needed {
        // No input permutation needed: returned mapping is just self.
        this.clone_box()
    } else {
        // 1-based perm entries (PermMap convention).
        let inperm: Vec<i64> = in_axes.iter().map(|&v| (v as i64) + 1).collect();
        let outperm: Vec<i64> = outperm_zb.iter().map(|&v| (v as i64) + 1).collect();
        let pm = PermMap::new(&inperm, &outperm, &[]).ok()?;
        let cmp = CmpMap::new(Box::new(pm), this.clone_box(), Composition::Series).ok()?;
        <dyn Mapping>::simplify(&cmp).ok()?
    };

    Some((out_axes, sub))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_equal_does_not_overflow_near_the_top_of_the_range() {
        // A Frame axis has Top/Bottom of +/-f64::MAX unless they are set, so
        // bounds tests routinely compare against these values. Scaling by the
        // tolerance multiplier first would make the tolerance infinite and
        // every such comparison would report equality.
        assert!(!ast_equal(f64::MAX, 0.0));
        assert!(!ast_equal(-f64::MAX, 0.0));
        assert!(!ast_equal(f64::MAX, -f64::MAX));
        assert!(ast_equal(f64::MAX, f64::MAX));
    }

    #[test]
    fn ast_equals_keeps_its_relative_tolerance() {
        // 1e5 * eps is about 2.2e-11 relative, so a value perturbed by 1e-13
        // of its own magnitude compares equal and one perturbed by 1e-9 does
        // not.
        let a = 1.4204e9;
        assert!(ast_equal(a, a * (1.0 + 1.0e-13)));
        assert!(!ast_equal(a, a * (1.0 + 1.0e-9)));
    }

    #[test]
    fn ast_equals_keeps_its_near_zero_floor() {
        assert!(ast_equal(0.0, 1.0e-13));
        assert!(!ast_equal(0.0, 1.0e-11));
    }

    #[test]
    fn ast_equals_bad_handling() {
        assert!(ast_equal(BAD_VALUE, BAD_VALUE));
        assert!(!ast_equal(BAD_VALUE, 0.0));
        assert!(!ast_equal(0.0, BAD_VALUE));
    }
}

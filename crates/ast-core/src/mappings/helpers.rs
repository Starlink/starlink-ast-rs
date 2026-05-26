//! Shared utilities for Mapping implementations.

use crate::constants::BAD_VALUE;
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, PermMap};

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

/// Validate that `in_axes` is a permutation of a subset of `0..nin` —
/// i.e., every entry is in range and no entry is duplicated. Returns
/// `Some(())` on success, `None` on any violation. Used by
/// `Mapping::map_split` impls to gate input validation up-front.
///
/// The Rust pattern rejects duplicates; the C `astMapSplit` permits
/// duplicates and produces a degenerate output. The Rust convention,
/// established in Task 1.1 across all per-class impls, is stricter.
pub(super) fn validate_in_axes(in_axes: &[usize], nin: usize) -> Option<()> {
    for (i, &ix) in in_axes.iter().enumerate() {
        if ix >= nin || in_axes[..i].contains(&ix) {
            return None;
        }
    }
    Some(())
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
        cmp.simplified().ok()?
    };

    Some((out_axes, sub))
}

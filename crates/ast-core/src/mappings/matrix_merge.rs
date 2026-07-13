//! Faithful port of C `matrixmap.c::MapMerge` and its swap helpers.
//!
//! This implements [`MatrixMap`](super::matrix::MatrixMap)'s per-class
//! `map_merge` (C `matrixmap.c:1653`). It owns the full strategy a nominated
//! MatrixMap uses to simplify a series/parallel sequence:
//!
//! 1. **Replace-with-simpler** — a square UNIT matrix becomes a `UnitMap`; a
//!    square DIAGONAL matrix with equal diagonal terms becomes a `ZoomMap`; a
//!    FULL square matrix whose off-diagonal terms are all exactly zero becomes
//!    a DIAGONAL `MatrixMap` (C matrixmap.c:1846-1906).
//! 2. **Direct merge** with a mergeable neighbour: another MatrixMap
//!    (`MatMat`), a ZoomMap (`MatZoom`), an invertible PermMap (`MatPerm`), a
//!    UnitMap, or — when the MatrixMap is diagonal — a WinMap (`MatWin2`).
//! 3. **Swap toward the nearest mergeable target** — compute the distance to
//!    the nearest mergeable map on each side (passing only over WinMap /
//!    PermMap), swap that direction via [`win_mat`](super::winmap_merge::win_mat)
//!    (matrix↔WinMap) or [`mat_perm_swap`] (matrix↔PermMap), then merge at the
//!    new adjacency.
//! 4. **Speculative swap** — even with no target, swap with a neighbour if
//!    either swapped map then simplifies to a different class.

use ndarray::Array2;

use crate::mappings::helpers::ast_equal;

use super::matrix::MatrixMap;
use super::simplify::{MergeCtx, default_map_merge};
use super::unit::UnitMap;
use super::zoom::ZoomMap;
use super::{DynMapping, Mapping};

/// Entry point: MatrixMap's per-class `map_merge`. Faithful port of
/// `matrixmap.c::MapMerge` (1653). `where_` is the nominated MatrixMap's
/// index in `ctx.list`; returns the index of the first modified element, or
/// `None`.
pub(crate) fn matrix_map_merge(
    mm: &MatrixMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    let eff_inv = ctx.entry(where_).1;

    // Effective input/output axis counts (C swaps nin/nout when the supplied
    // invert flag differs from the object's own Invert; our stored objects are
    // always forward, so the swap is driven purely by `eff_inv`).
    let (nin, nout) = if eff_inv {
        (mm.nout(), mm.nin())
    } else {
        (mm.nin(), mm.nout())
    };

    // ---- Step 1: replace-with-simpler (no reference to neighbours).
    if let Some(new) = replace_with_simpler(mm, eff_inv, nin, nout) {
        ctx.set_pair_single(where_, (new, false, false));
        return Some(where_);
    }

    // ---- Steps 2-4 (series only): direct merge / swap-and-seek. Delegated to
    // `default_map_merge` for now while this module is built up incrementally.
    // `default_map_merge` reproduces the direct pairwise merge via
    // `MatrixMap::try_merge` (MatMat / MatZoom / MatPerm / MatWin2).
    default_map_merge(ctx, where_, series)
}

/// Step 1: C matrixmap.c:1846-1906 — try to replace the MatrixMap with a
/// simpler Mapping without reference to its neighbours. Returns the
/// replacement (in forward state) or `None`.
fn replace_with_simpler(
    mm: &MatrixMap,
    eff_inv: bool,
    nin: usize,
    nout: usize,
) -> Option<DynMapping> {
    // The effective forward matrix (honouring the applied direction) and the
    // stored forward matrix. C reads `mm->f_matrix` for the structural tests
    // (uniformity / off-diagonal-zero) and picks the zoom factor from
    // `f_matrix`/`i_matrix` per `invert_list[where]`.
    let fwd = mm.forward_matrix();

    // Square UNIT matrix → UnitMap.
    if mm.is_unit_form() && nin == nout {
        return Some(Box::new(UnitMap::new(nin)));
    }

    // Square DIAGONAL matrix with equal diagonal terms → ZoomMap (unless the
    // diagonal is all zero / BAD).
    if mm.is_diagonal_form() && nin == nout && fwd[[0, 0]] != crate::constants::BAD_VALUE {
        // Uniformity is tested on the stored forward diagonal (C loops
        // `mm->f_matrix`).
        let n = fwd.nrows();
        let uniform = (1..n).all(|i| ast_equal(fwd[[i, i]], fwd[[i - 1, i - 1]]));
        if uniform {
            let factor = if eff_inv {
                mm.inverse_matrix()?[[0, 0]]
            } else {
                fwd[[0, 0]]
            };
            if factor != 0.0 {
                return ZoomMap::new(nin, factor)
                    .ok()
                    .map(|z| Box::new(z) as DynMapping);
            }
        }
    }

    // FULL square matrix whose off-diagonal terms are all *exactly* zero →
    // DIAGONAL MatrixMap built from the forward diagonal. C uses an exact
    // `!= 0.0` test (not `astEQUAL`) and constructs the new MatrixMap from
    // `mm->f_matrix` with Invert=0 (matrixmap.c:1885-1905).
    if mm.is_full_form() && nin == nout {
        let n = fwd.nrows();
        let mut off_diag_zero = true;
        'outer: for i in 0..n {
            for j in 0..n {
                if i != j && fwd[[i, j]] != 0.0 {
                    off_diag_zero = false;
                    break 'outer;
                }
            }
        }
        if off_diag_zero {
            let mut diag = Array2::<f64>::zeros((n, n));
            for i in 0..n {
                diag[[i, i]] = fwd[[i, i]];
            }
            return MatrixMap::new(diag).ok().map(|m| Box::new(m) as DynMapping);
        }
    }

    None
}

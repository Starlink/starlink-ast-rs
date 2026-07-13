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

use crate::coords::PointSet;
use crate::mappings::helpers::ast_equal;

use super::matrix::{MatrixMap, is_diagonal};
use super::perm::PermMap;
use super::simplify::{MergeCtx, default_map_merge};
use super::unit::UnitMap;
use super::winmap::{WinMap, compose_linear, extract_linear_params};
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

    // The neighbour-merge and swap steps only apply in series (C wraps the
    // whole block in `if( series )`). In parallel, C's MatrixMap MapMerge does
    // nothing beyond Part A. We still fall through to `default_map_merge`,
    // which is a no-op for a MatrixMap with no mergeable right neighbour and
    // preserves the parallel behaviour the rest of the engine relies on.
    if !series {
        return default_map_merge(ctx, where_, series);
    }

    // ---- Step 2 (series): direct merge with a mergeable neighbour.
    if let Some(result) = try_direct_merge(mm, ctx, where_) {
        return Some(result);
    }

    // ---- Steps 3 & 4 (swap-and-seek) are added in a later increment; until
    // then fall through to the shared pairwise merge / global swap helpers.
    default_map_merge(ctx, where_, series)
}

/// Effective forward matrix of a MatrixMap entry, honouring the applied
/// direction: the stored inverse when `inv`, else the stored forward matrix.
/// Returns `None` when an inverse is required but not defined.
fn eff_matrix(mm: &MatrixMap, inv: bool) -> Option<Array2<f64>> {
    if inv {
        mm.inverse_matrix().cloned()
    } else {
        Some(mm.forward_matrix().clone())
    }
}

/// Classes a MatrixMap can merge with *directly* (C matrixmap.c:1936-1975):
/// another MatrixMap, a ZoomMap, an invertible PermMap, a UnitMap, or — when
/// the MatrixMap is diagonal — a WinMap. `mm_diagonal` is whether the
/// nominated MatrixMap's effective forward matrix is diagonal.
fn is_mergeable(m: &dyn Mapping, mm_diagonal: bool) -> bool {
    if m.as_any().is::<MatrixMap>() || m.as_any().is::<ZoomMap>() || m.as_any().is::<UnitMap>() {
        return true;
    }
    if let Some(pm) = m.as_any().downcast_ref::<PermMap>() {
        return perm_ok(pm);
    }
    if m.as_any().is::<WinMap>() {
        return mm_diagonal;
    }
    false
}

/// Whether the MatrixMap's effective forward matrix (honouring `inv`) is
/// diagonal.
fn matrix_is_diagonal(mm: &MatrixMap, inv: bool) -> bool {
    match eff_matrix(mm, inv) {
        Some(m) => is_diagonal(&m),
        None => false,
    }
}

/// Step 2: try a direct merge of the nominated MatrixMap with whichever
/// neighbour is mergeable. C prefers the lower neighbour (class1) then the
/// upper (class2). Returns the modified index on success.
fn try_direct_merge(mm: &MatrixMap, ctx: &mut MergeCtx, where_: usize) -> Option<usize> {
    let n = ctx.len();
    let eff_inv = ctx.entry(where_).1;
    let mm_diagonal = matrix_is_diagonal(mm, eff_inv);

    // C first checks the merge classes (MatrixMap/ZoomMap/PermMap/UnitMap) on
    // either side, preferring the lower neighbour; only if neither side is one
    // of those and the MatrixMap is diagonal does it consider a WinMap.
    let mergeable = |idx: usize| -> bool { is_mergeable(ctx.entry(idx).0.as_ref(), mm_diagonal) };
    let lower_ok = where_ > 0 && mergeable(where_ - 1);
    let upper_ok = where_ + 1 < n && mergeable(where_ + 1);

    let (i1, i2) = if lower_ok {
        (where_ - 1, where_)
    } else if upper_ok {
        (where_, where_ + 1)
    } else {
        return None;
    };

    // `mm_first` is true when the nominated MatrixMap is the left operand
    // (i1 == where_): the matrix is applied first.
    let mm_first = i1 == where_;
    let (m1, inv1, _) = ctx.entry(i1);
    let (m2, inv2, _) = ctx.entry(i2);
    let merged = merge_matrix_pair(m1.as_ref(), *inv1, m2.as_ref(), *inv2, mm_first)?;
    ctx.replace_pair_with(i1, merged, false);
    Some(i1)
}

/// Merge an adjacent (map1, map2) pair, one of which is the nominated
/// MatrixMap, in series. `mm_first` records whether the MatrixMap is `map1`.
/// Dispatches to the C merge builder for the neighbour class.
fn merge_matrix_pair(
    map1: &dyn Mapping,
    inv1: bool,
    map2: &dyn Mapping,
    inv2: bool,
    mm_first: bool,
) -> Option<DynMapping> {
    // Identify the MatrixMap and the neighbour.
    let (mm, mm_inv, other, other_inv) = if mm_first {
        (map1.as_any().downcast_ref::<MatrixMap>()?, inv1, map2, inv2)
    } else {
        (map2.as_any().downcast_ref::<MatrixMap>()?, inv2, map1, inv1)
    };

    // MatMat (matrixmap.c:2525): result forward = B·A, where A is applied
    // first. mtr order follows the list order (map1 first, map2 second).
    if let Some(other_mm) = other.as_any().downcast_ref::<MatrixMap>() {
        let a = eff_matrix(map1.as_any().downcast_ref::<MatrixMap>()?, inv1)?;
        let b = eff_matrix(map2.as_any().downcast_ref::<MatrixMap>()?, inv2)?;
        let _ = other_mm;
        if b.ncols() != a.nrows() {
            return None;
        }
        let product = b.dot(&a);
        return mat_or_unit(product);
    }

    // MatZoom (matrixmap.c:3416): zoom commutes (z·I), so the result is the
    // effective matrix scaled by the effective zoom factor, regardless of
    // order.
    if let Some(zm) = other.as_any().downcast_ref::<ZoomMap>() {
        let m = eff_matrix(mm, mm_inv)?;
        let zfac = if other_inv {
            1.0 / zm.factor()
        } else {
            zm.factor()
        };
        // Zoom axis count must match the shared dimension.
        if zm.nin() != m.nrows() && zm.nin() != m.ncols() {
            return None;
        }
        let scaled = &m * zfac;
        return mat_or_unit(scaled);
    }

    // MatPerm (matrixmap.c:2608): merge with an invertible PermMap.
    if let Some(pm) = other.as_any().downcast_ref::<PermMap>() {
        if !perm_ok(pm) {
            return None;
        }
        return mat_perm(mm, mm_inv, pm, other_inv, mm_first);
    }

    // MatWin2 (matrixmap.c:3230): diagonal MatrixMap + WinMap → WinMap. Since
    // the matrix is diagonal, this reduces to a per-axis linear composition.
    if other.as_any().is::<WinMap>() {
        let m = eff_matrix(mm, mm_inv)?;
        if !is_diagonal(&m) {
            return None;
        }
        let n = m.nrows();
        let diag: Vec<f64> = (0..n).map(|i| m[[i, i]]).collect();
        let zeros = vec![0.0; n];
        let (wshifts, wscales) = extract_linear_params(other, other_inv)?;
        if wshifts.len() != n {
            return None;
        }
        return if mm_first {
            // matrix (diag) applied first, then WinMap:
            // out = wshift + wscale·(diag·x).
            compose_linear(&zeros, &diag, &wshifts, &wscales)
        } else {
            // WinMap applied first, then matrix (diag):
            // out = diag·(wshift + wscale·x).
            compose_linear(&wshifts, &wscales, &zeros, &diag)
        };
    }

    None
}

/// Turn a product matrix into a `UnitMap` (if identity) or a `MatrixMap`.
fn mat_or_unit(product: Array2<f64>) -> Option<DynMapping> {
    if super::matrix::is_identity_matrix_pub(&product) {
        return Some(Box::new(UnitMap::new(product.nrows())));
    }
    MatrixMap::new(product)
        .ok()
        .map(|m| Box::new(m) as DynMapping)
}

/// C `matrixmap.c::MatPerm` (2608): merge a MatrixMap and a (square,
/// invertible) PermMap into a single MatrixMap. `mm_first` is C's `mat1`:
/// true means the MatrixMap is applied first, then the PermMap.
///
/// The PermMap is represented as a 0/1 MatrixMap `P` whose rows are the unit
/// vectors transformed by the *inverse* PermMap (matrixmap.c:2696-2723), then
/// the result is `astMtrMult(mm, P)` (mat1) or `astMtrMult(P, mm)`.
fn mat_perm(
    mm: &MatrixMap,
    mm_inv: bool,
    pm: &PermMap,
    pm_inv: bool,
    mm_first: bool,
) -> Option<DynMapping> {
    let nax = if pm_inv { pm.nout() } else { pm.nin() };
    // PermOK guarantees nin == nout, so nax is unambiguous.

    // Build P by transforming the unit vectors e_0..e_{nax-1} through the
    // inverse PermMap (astTransform(pm, unit, 0)). With the PermMap applied in
    // its effective direction `pm_inv`, the "inverse" transform is
    // `forward = pm_inv` (i.e. inverse of the effective direction).
    let rows: Vec<Vec<f64>> = (0..nax)
        .map(|i| (0..nax).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    let unit = PointSet::from_rows(rows.iter().map(|r| r.as_slice()));
    // Effective forward direction of pm is `!pm_inv`; the inverse is `pm_inv`.
    let transformed = pm.transform(&unit, pm_inv).ok()?;

    // matrix[j][i] = transformed[i][j]  (matrixmap.c:2720-2722).
    let mut p = Array2::<f64>::zeros((nax, nax));
    for i in 0..nax {
        for j in 0..nax {
            p[[j, i]] = transformed.point(i)[j];
        }
    }

    let m = eff_matrix(mm, mm_inv)?;
    // astMtrMult(this, a) = a·this (this applied first). MatPerm:
    //   mat1  : result = astMtrMult(mm, P) = P·M  (M first, then P)
    //   !mat1 : result = astMtrMult(P, mm) = M·P  (P first, then M)
    let product = if mm_first {
        if p.ncols() != m.nrows() {
            return None;
        }
        p.dot(&m)
    } else {
        if m.ncols() != p.nrows() {
            return None;
        }
        m.dot(&p)
    };
    mat_or_unit(product)
}

/// C `matrixmap.c::PermOK` (4535): a PermMap can merge with a MatrixMap only
/// if it has equal input/output counts and mutually consistent forward and
/// inverse transforms. Verified — as in C — by a forward-then-inverse
/// round-trip on two distinct probe positions.
fn perm_ok(pm: &PermMap) -> bool {
    let nin = pm.nin();
    let nout = pm.nout();
    if nin != nout {
        return false;
    }
    // Two probe points: [1,2,3,...] and [0,-1,-2,...] (matrixmap.c:4601-4604).
    let p0: Vec<f64> = (0..nin).map(|i| (i + 1) as f64).collect();
    let p1: Vec<f64> = (0..nin).map(|i| -(i as f64)).collect();
    let pset = PointSet::from_rows([p0.as_slice(), p1.as_slice()]);
    let Ok(fwd) = pm.transform(&pset, true) else {
        return false;
    };
    let Ok(back) = pm.transform(&fwd, false) else {
        return false;
    };
    for i in 0..nin {
        if back.point(0)[i] != (i + 1) as f64 || back.point(1)[i] != -(i as f64) {
            return false;
        }
    }
    true
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

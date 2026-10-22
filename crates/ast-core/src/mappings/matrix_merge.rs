//! Faithful port of C `matrixmap.c::MapMerge` and its swap helpers.
//!
//! This implements [`MatrixMap`](super::matrix::MatrixMap)'s per-class
//! `map_merge` (C `matrixmap.c:1653`). It owns the full strategy a nominated
//! MatrixMap uses to simplify a series/parallel sequence:
//!
//! 1. **Replace-with-simpler** — a square UNIT matrix becomes a `UnitMap`; a
//!    square DIAGONAL matrix with equal diagonal terms becomes a `ZoomMap`; a
//!    FULL square matrix whose off-diagonal terms are all exactly zero becomes
//!    a DIAGONAL `MatrixMap` (C matrixmap.c:1857-1929). This is the only
//!    reduction a lone MatrixMap can undergo; C gives MatrixMap no `Simplify`
//!    vtab slot, so a MatrixMap nominated on its own arrives here too.
//! 2. **Direct merge** with a mergeable neighbor: another MatrixMap
//!    (`MatMat`), a ZoomMap (`MatZoom`), an invertible PermMap (`MatPerm`), a
//!    UnitMap, or — when the MatrixMap is diagonal — a WinMap (`MatWin2`).
//! 3. **Swap toward the nearest mergeable target** — compute the distance to
//!    the nearest mergeable map on each side (passing only over WinMap /
//!    PermMap), swap that direction via [`win_mat`](super::winmap_merge::win_mat)
//!    (matrix↔WinMap) or [`mat_perm_swap`] (matrix↔PermMap), then merge at the
//!    new adjacency.
//! 4. **Speculative swap** — even with no target, swap with a neighbor if
//!    either swapped map then simplifies to a different class.

use ndarray::Array2;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::mappings::helpers::{ast_equal, is_bad};

use super::matrix::{MatrixMap, is_diagonal};
use super::perm::PermMap;
use super::simplify::{MergeCtx, default_map_merge};
use super::unit::UnitMap;
use super::winmap::{WinMap, extract_linear_params};
use super::zoom::ZoomMap;
use super::{AstResult, DynMapping, Mapping};

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

    // ---- Step 1: replace-with-simpler (no reference to neighbors).
    if let Some(new) = replace_with_simpler(mm, eff_inv, nin, nout) {
        ctx.set_pair_single(where_, (new, false, false));
        return Some(where_);
    }

    // The neighbor-merge and swap steps only apply in series (C wraps the
    // whole block in `if( series )`). In parallel, C's MatrixMap MapMerge does
    // nothing beyond Part A. We still fall through to `default_map_merge`,
    // which is a no-op for a MatrixMap with no mergeable right neighbor and
    // preserves the parallel behavior the rest of the engine relies on.
    if !series {
        return default_map_merge(ctx, where_, series);
    }

    // ---- Step 2 (series): direct merge with a mergeable neighbor.
    if let Some(result) = try_direct_merge(mm, ctx, where_) {
        return Some(result);
    }

    // ---- Steps 3 & 4 (series): swap toward the nearest mergeable target,
    // then a speculative swap if no target exists.
    if let Some(result) = try_swap_strategy(mm, ctx, where_, series) {
        return Some(result);
    }

    // C's merge only ever considers a neighbor of a class it names:
    // MatrixMap, ZoomMap, UnitMap, an invertable PermMap, or — when this
    // MatrixMap is diagonal — a WinMap (matrixmap.c:1622-1660). Anything else
    // takes the `else` branch at matrixmap.c:1709, which keeps the MatrixMap
    // and its invert flag untouched and merges nothing.
    //
    // That is load-bearing rather than an optimization. A `ShiftMap` neighbor
    // is not on the list: C waits for the ShiftMap's own merge to turn it into
    // a WinMap first. Folding the pair here instead composes through the
    // MatrixMap's *effective* direction and materializes the inverse into the
    // stored values, so the Mapping serializes with the inverse matrix stored
    // forward where C stores the matrix and records `InvB` on the parent.
    //
    // The fallback stays for a neighbor C would accept, covering the swap
    // cases not yet ported.
    let mm_diagonal = matrix_is_diagonal(mm, eff_inv);
    let mergeable = |idx: usize| is_mergeable(ctx.entry(idx).0.as_ref(), mm_diagonal);
    let has_mergeable_neighbour =
        (where_ > 0 && mergeable(where_ - 1)) || (where_ + 1 < ctx.len() && mergeable(where_ + 1));
    if !has_mergeable_neighbour {
        return None;
    }
    default_map_merge(ctx, where_, series)
}

/// Effective forward matrix of a MatrixMap entry, honoring the applied
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

/// Whether the MatrixMap's effective forward matrix (honoring `inv`) is
/// diagonal.
fn matrix_is_diagonal(mm: &MatrixMap, inv: bool) -> bool {
    match eff_matrix(mm, inv) {
        Some(m) => is_diagonal(&m),
        None => false,
    }
}

/// Step 2: try a direct merge of the nominated MatrixMap with whichever
/// neighbor is mergeable. C prefers the lower neighbor (class1) then the
/// upper (class2). Returns the modified index on success.
fn try_direct_merge(mm: &MatrixMap, ctx: &mut MergeCtx, where_: usize) -> Option<usize> {
    let n = ctx.len();
    let eff_inv = ctx.entry(where_).1;
    let mm_diagonal = matrix_is_diagonal(mm, eff_inv);

    // C first checks the merge classes (MatrixMap/ZoomMap/PermMap/UnitMap) on
    // either side, preferring the lower neighbor; only if neither side is one
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

    // A UnitMap neighbor reaches C's final `else` (matrixmap.c:2029-2032),
    // which has no merge builder: it clones the nominated MatrixMap *keeping
    // its Invert flag* — unlike the MatMat / MatZoom / MatPerm / MatWin2
    // branches, which all force `invert = 0` — writes it to `i1` and drops the
    // other element. Removing the UnitMap leaves the MatrixMap at `i1` with its
    // flags untouched, which is the same outcome for either neighbor side.
    let neighbour = if mm_first { i2 } else { i1 };
    if ctx.entry(neighbour).0.as_any().is::<UnitMap>() {
        ctx.remove_at(neighbour);
        return Some(i1);
    }

    let (m1, inv1, _) = ctx.entry(i1);
    let (m2, inv2, _) = ctx.entry(i2);
    let merged = merge_matrix_pair(m1.as_ref(), *inv1, m2.as_ref(), *inv2, mm_first)?;
    ctx.replace_pair_with(i1, merged, false);
    Some(i1)
}

/// Merge an adjacent (map1, map2) pair, one of which is the nominated
/// MatrixMap, in series. `mm_first` records whether the MatrixMap is `map1`.
/// Dispatches to the C merge builder for the neighbor class.
fn merge_matrix_pair(
    map1: &dyn Mapping,
    inv1: bool,
    map2: &dyn Mapping,
    inv2: bool,
    mm_first: bool,
) -> Option<DynMapping> {
    // Identify the MatrixMap and the neighbor.
    let (mm, mm_inv, other, other_inv) = if mm_first {
        (map1.as_any().downcast_ref::<MatrixMap>()?, inv1, map2, inv2)
    } else {
        (map2.as_any().downcast_ref::<MatrixMap>()?, inv2, map1, inv1)
    };

    // MatMat (matrixmap.c MatMat): `astMtrMult` of the two MatrixMaps, each
    // in the direction it is applied, map1 first. `mtr_mult` forms the
    // product as C's `MtrMult` does, element by element in C's order, which
    // is what keeps the last digits C's.
    if other.as_any().is::<MatrixMap>() {
        let mut first = map1.as_any().downcast_ref::<MatrixMap>()?.clone();
        if inv1 {
            first.invert();
        }
        let mut second = map2.as_any().downcast_ref::<MatrixMap>()?.clone();
        if inv2 {
            second.invert();
        }
        if second.nin() != first.nout() {
            return None;
        }
        let product = first.mtr_mult(&second).ok()?;
        return mat_or_unit(product.forward_matrix().clone());
    }

    // MatZoom (matrixmap.c MatZoom): the product of the MatrixMap, in the
    // direction it is applied, and a diagonal MatrixMap holding the zoom
    // factor, which is a unit MatrixMap when the factor equals 1 within
    // astEQUAL. A ZoomMap nominated beside a MatrixMap merges instead through
    // `MatrixMap::mtr_zoom` (C's astMtrZoom, from zoommap.c MapMerge).
    if let Some(zm) = other.as_any().downcast_ref::<ZoomMap>() {
        let zfac = if other_inv {
            1.0 / zm.zoom()
        } else {
            zm.zoom()
        };
        let mut applied = mm.clone();
        if mm_inv {
            applied.invert();
        }
        let nrow = applied.nout();
        if zm.nin() != nrow {
            return None;
        }
        let zmm = MatrixMap::diagonal(nrow, nrow, &vec![zfac; nrow]).ok()?;
        let product = applied.mtr_mult(&zmm).ok()?;
        return mat_or_unit(product.forward_matrix().clone());
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
        // Compute the merged scale/shift terms exactly as C's MatWin2
        // (matrixmap.c:3340). For a diagonal matrix, astTranN reduces to a
        // per-axis multiply by the diagonal element.
        let (newscales, newshifts): (Vec<f64>, Vec<f64>) = if mm_first {
            // Matrix applied first, then WinMap: out = wscale·(diag·x) + wshift.
            // C: newscales = M·wscales, newshifts = wshifts (unchanged).
            (
                (0..n).map(|i| diag[i] * wscales[i]).collect(),
                wshifts.clone(),
            )
        } else {
            // WinMap applied first, then matrix: out = diag·(wscale·x + wshift).
            // C: newscales = M·wscales, newshifts = M·wshifts.
            (
                (0..n).map(|i| diag[i] * wscales[i]).collect(),
                (0..n).map(|i| diag[i] * wshifts[i]).collect(),
            )
        };
        let _ = zeros;

        // C constructs the result WinMap from corner points via astWinMap:
        //   ina = 0, inb = 1, outa = newshift, outb = newscale + newshift.
        // The WinMap constructor then recomputes the stored scale as
        //   (outb - outa)/(inb - ina),
        // carrying the rounding of the newscale+newshift add-then-subtract.
        let ina = vec![0.0; n];
        let inb = vec![1.0; n];
        let outa = newshifts;
        let outb: Vec<f64> = (0..n).map(|i| newscales[i] + outa[i]).collect();
        let wm = WinMap::from_corners(&ina, &inb, &outa, &outb).ok()?;
        let (fin_shifts, fin_scales) = (wm.shifts().to_vec(), wm.scales().to_vec());
        return super::winmap::finalize_linear(fin_shifts, fin_scales);
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

    // C `matrixmap.c:2735-2738`: `matrix[j][i] = ptr2[i][j]`, where `ptr2` is
    // AST's `[coord][point]` layout, so element `[i][j]` is coordinate `i` of
    // transformed point `j`. In the crate's `[point][coord]` PointSet that is
    // `transformed.point(j)[i]`.
    let mut p = Array2::<f64>::zeros((nax, nax));
    for i in 0..nax {
        for j in 0..nax {
            p[[j, i]] = transformed.point(j)[i];
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
        super::matrix::mtr_product(&p, &m)
    } else {
        if m.ncols() != p.nrows() {
            return None;
        }
        super::matrix::mtr_product(&m, &p)
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

/// Classes a MatrixMap can *swap* past while seeking a merge target:
/// WinMap or PermMap (matrixmap.c:2102-2104, 2136-2138).
fn is_swappable_class(m: &dyn Mapping) -> bool {
    m.as_any().is::<WinMap>() || m.as_any().is::<PermMap>()
}

/// `CanSwap` on the adjacent pair `(i1, i1 + 1)` of the working list.
fn can_swap_at(ctx: &MergeCtx, i1: usize) -> (bool, bool) {
    can_swap(
        ctx.entry(i1).0.as_ref(),
        ctx.entry(i1).1,
        ctx.entry(i1 + 1).0.as_ref(),
        ctx.entry(i1 + 1).1,
    )
}

/// C `matrixmap.c::CanSwap` (331): can the MatrixMap and its neighbor swap,
/// and would the swap make either intrinsically simpler (`simpler`)? One of
/// the pair is the MatrixMap. Returns `(can_swap, simpler)`.
///
/// WinMap → always swappable, never flagged simpler. PermMap → swappable iff
/// it is bi-directional (PermOK) with a square, invertible MatrixMap, and any
/// constants it feeds into/out of the MatrixMap map through "scaling" rows and
/// columns (a diagonal-only interaction). `simpler` iff the swap moves the
/// MatrixMap to the side where the PermMap changes dimension favorably.
fn can_swap(map1: &dyn Mapping, inv1: bool, map2: &dyn Mapping, inv2: bool) -> (bool, bool) {
    // Identify the MatrixMap and the non-MatrixMap partner.
    let (mat, mat_inv, nomat, nomat_inv, nomat_is_map1) = if map1.as_any().is::<MatrixMap>() {
        (map1, inv1, map2, inv2, false)
    } else {
        (map2, inv2, map1, inv1, true)
    };
    let Some(mm) = mat.as_any().downcast_ref::<MatrixMap>() else {
        return (false, false);
    };

    // WinMap: always swappable.
    if nomat.as_any().is::<WinMap>() {
        return (true, false);
    }

    // PermMap: bi-directional, square + invertible matrix, and scaling
    // row/col for constants fed to the matrix.
    if let Some(pm) = nomat.as_any().downcast_ref::<PermMap>() {
        let nax = if mat_inv { mm.nin() } else { mm.nout() };
        // MatrixMap must be square and invertible (or UNIT).
        let square = mm.nin() == mm.nout();
        let invertible = mm.is_unit_form() || mm.inverse_matrix().is_some();
        if !square || !invertible {
            return (false, false);
        }
        // Effective 0-based perm arrays (constants stay negative refs).
        let (outperm, inperm, _consts) = perm_get(pm, nomat_inv);
        let nin = inperm.len() as i64;
        let nout = outperm.len() as i64;

        // Bi-directionality of the links (matrixmap.c:472-492). PermOK already
        // guaranteed consistency, but C re-checks here.
        for (i, &p) in outperm.iter().enumerate() {
            if p >= 0 && p < nin && inperm[p as usize] != i as i64 {
                return (false, false);
            }
        }
        for (i, &p) in inperm.iter().enumerate() {
            if p >= 0 && p < nout && outperm[p as usize] != i as i64 {
                return (false, false);
            }
        }

        // Effective forward matrix, for the constant scaling-row/col check.
        let Some(eff) = eff_matrix(mm, mat_inv) else {
            return (false, false);
        };

        // If the PermMap is applied first (nomat == map1), its output axes
        // feed the matrix forward: each constant output must sit on a scaling
        // row/col. If applied second, its input axes feed the matrix inverse.
        if nomat_is_map1 {
            if nout != nax as i64 {
                return (false, false);
            }
            for (i, &p) in outperm.iter().enumerate() {
                if (p < 0 || p >= nin) && !scaling_row_col(&eff, i) {
                    return (false, false);
                }
            }
        } else {
            if nin != nax as i64 {
                return (false, false);
            }
            for (i, &p) in inperm.iter().enumerate() {
                if (p < 0 || p >= nout) && !scaling_row_col(&eff, i) {
                    return (false, false);
                }
            }
        }

        // simpler flag (matrixmap.c:555-561).
        let simpler = if nomat_is_map1 {
            nout > nin
        } else {
            nin > nout
        };
        return (true, simpler);
    }

    (false, false)
}

/// C `matrixmap.c::ScalingRowCol` (5275): whether row and column `axis` of a
/// square matrix are all (effectively) zero except for a non-zero diagonal
/// term. Uses the `DBL_EPSILON` magnitude test C uses on a FULL matrix; a
/// non-FULL (diagonal/unit) matrix trivially qualifies.
fn scaling_row_col(m: &Array2<f64>, axis: usize) -> bool {
    // A diagonal matrix is a simple scaling on every axis.
    if is_diagonal(m) {
        return true;
    }
    let n = m.nrows();
    if axis >= n {
        return false;
    }
    for i in 0..n {
        let el = m[[axis, i]].abs();
        if i == axis {
            if el <= f64::EPSILON {
                return false;
            }
        } else if el > f64::EPSILON {
            return false;
        }
    }
    for i in 0..n {
        let el = m[[i, axis]].abs();
        if i == axis {
            if el <= f64::EPSILON {
                return false;
            }
        } else if el > f64::EPSILON {
            return false;
        }
    }
    true
}

/// C `matrixmap.c::PermGet` (4055): return the PermMap's `(outperm, inperm,
/// consts)` as 0-based arrays for the given effective direction. Connected
/// links are 0-based indices; constant axes are encoded as `-(k+1)` referring
/// to `consts[k]`.
///
/// Ported faithfully from C: rather than reading the stored permutation
/// arrays, the arrays are *derived* by transforming probe positions through
/// the PermMap. This is self-consistent regardless of how the PermMap leaf is
/// stored (including a deferred-inverted leaf) and correctly distinguishes a
/// constant axis (both probe points map to the same value) from a copied axis.
///
/// `inv` is the effective Invert flag to apply to the PermMap (C's
/// `astSetInvert(map, inv)` before probing). The "forward" probe uses the
/// PermMap in that effective direction; the "inverse" probe uses the opposite.
fn perm_get(pm: &PermMap, inv: bool) -> (Vec<i64>, Vec<i64>, Vec<f64>) {
    // Clone and force the PermMap into the requested absolute Invert state,
    // matching C's `astSetInvert( map, inv )`. Probes then use the API-level
    // forward (direction 1) and inverse (direction 0) transforms.
    let mut probe = pm.clone();
    if probe.is_inverted() != inv {
        probe.invert();
    }
    let nin = probe.nin();
    let nout = probe.nout();

    let mut outprm = vec![0i64; nout];
    let mut inprm = vec![0i64; nin];
    let mut cnst: Vec<f64> = Vec::with_capacity(nout + nin);
    let mut nc = 0usize;

    // Input probes: point0 = [0,1,2,...], point1 = [-1,-1,...]. The first
    // enumerates the axes, the second detects constant output values.
    // Forward transform to determine the output axis permutation array.
    if nin > 0 {
        let p0: Vec<f64> = (0..nin).map(|i| i as f64).collect();
        let p1: Vec<f64> = vec![-1.0; nin];
        let pset1 = PointSet::from_rows([p0.as_slice(), p1.as_slice()]);
        if let Ok(pset2) = probe.transform(&pset1, true) {
            for (i, slot) in outprm.iter_mut().enumerate() {
                let op = pset2.point(0)[i];
                let cn = pset2.point(1)[i];
                if op == cn {
                    cnst.push(cn);
                    *slot = -((nc + 1) as i64);
                    nc += 1;
                } else {
                    *slot = op.round() as i64;
                }
            }
        }
    }

    // Output probes: point0 = [0,1,2,...], point1 = [-1,-1,...] over nout
    // coords. Inverse transform to determine the input axis permutation array.
    if nout > 0 {
        let q0: Vec<f64> = (0..nout).map(|i| i as f64).collect();
        let q1: Vec<f64> = vec![-1.0; nout];
        let pset2 = PointSet::from_rows([q0.as_slice(), q1.as_slice()]);
        if let Ok(pset1b) = probe.transform(&pset2, false) {
            for (i, slot) in inprm.iter_mut().enumerate() {
                let ip = pset1b.point(0)[i];
                let cn = pset1b.point(1)[i];
                if ip == cn {
                    cnst.push(cn);
                    *slot = -((nc + 1) as i64);
                    nc += 1;
                } else {
                    *slot = ip.round() as i64;
                }
            }
        }
    }

    (outprm, inprm, cnst)
}

/// Steps 3 & 4 (series): seek the nearest mergeable target on each side, swap
/// the MatrixMap toward it, and merge; failing that, do a speculative swap
/// kept iff a swapped map then simplifies. Faithful port of
/// matrixmap.c:2055-2273.
fn try_swap_strategy(
    mm: &MatrixMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    let n = ctx.len();
    let _ = mm;

    // swaphi/do2 (upper neighbor), swaplo/do1 (lower neighbor).
    let (swaphi, do2) = if where_ + 1 < n {
        can_swap_at(ctx, where_)
    } else {
        (false, false)
    };
    let (swaplo, do1) = if where_ > 0 {
        can_swap_at(ctx, where_ - 1)
    } else {
        (false, false)
    };

    // nstep2: distance forward to nearest mergeable target.
    let mut nstep2: i64 = -1;
    if swaphi {
        for i2 in (where_ + 1)..n {
            let (m, inv, _) = ctx.entry(i2);
            // C's forward seek merge classes: MatrixMap / ZoomMap / PermMap
            // (if PermOK) / UnitMap (matrixmap.c:2091-2094). Note: WinMap is
            // NOT a merge target on the forward walk (only swapped over).
            if is_seek_target(m.as_ref(), *inv) {
                nstep2 = (i2 - where_ - 1) as i64;
                break;
            }
            if !is_swappable_class(m.as_ref()) {
                break;
            }
        }
    }

    // nstep1: distance backward to nearest mergeable target.
    let mut nstep1: i64 = -1;
    if swaplo {
        for i1 in (0..where_).rev() {
            let (m, inv, _) = ctx.entry(i1);
            if is_seek_target(m.as_ref(), *inv) {
                nstep1 = (where_ - 1 - i1) as i64;
                break;
            }
            if !is_swappable_class(m.as_ref()) {
                break;
            }
        }
    }

    // Choose the swap direction (matrixmap.c:2147-2158).
    let (i1, swap_lower) = if do1 || (nstep1 != -1 && (nstep2 == -1 || nstep2 > nstep1)) {
        (where_.wrapping_sub(1), true)
    } else if do2 || nstep2 != -1 {
        (where_, false)
    } else {
        return try_speculative_swap(ctx, where_, swaphi, swaplo);
    };

    // The neighbor class for the chosen direction.
    let neigh_idx = if swap_lower { where_ - 1 } else { where_ + 1 };
    let nclass = swap_class_of(ctx.entry(neigh_idx).0.as_ref())?;

    // iwm = position of the MatrixMap within the (i1, i1+1) pair.
    let imm = where_ - i1;
    let pair = clone_pair(ctx, i1);
    let swapped = swap_pair(nclass, pair, imm);
    let swapped = ctx.settle(swapped)?;
    ctx.set_pair(i1, swapped[0].clone(), swapped[1].clone());

    // Merge at the new adjacency (matrixmap.c:2173-2193).
    if i1 == where_ && where_ + 1 < ctx.len() {
        let _ = ctx.merge_subrange(where_ + 1, 0, series);
    } else if where_ >= 2 {
        let _ = ctx.merge_subrange(where_ - 2, 0, series);
    }

    Some(i1)
}

/// Whether a neighbor is a *merge target* for the nstep seek: MatrixMap,
/// ZoomMap, UnitMap, or an invertible PermMap (matrixmap.c:2091-2094 /
/// 2128-2131). WinMap is only swapped over, never a target.
fn is_seek_target(m: &dyn Mapping, _inv: bool) -> bool {
    if m.as_any().is::<MatrixMap>() || m.as_any().is::<ZoomMap>() || m.as_any().is::<UnitMap>() {
        return true;
    }
    if let Some(pm) = m.as_any().downcast_ref::<PermMap>() {
        return perm_ok(pm);
    }
    false
}

/// Classes the MatrixMap can be swapped against.
#[derive(Clone, Copy, PartialEq)]
enum SwapClass {
    Win,
    Perm,
}

fn swap_class_of(m: &dyn Mapping) -> Option<SwapClass> {
    if m.as_any().is::<WinMap>() {
        Some(SwapClass::Win)
    } else if m.as_any().is::<PermMap>() {
        Some(SwapClass::Perm)
    } else {
        None
    }
}

/// Swap the MatrixMap (at index `imm` within the pair) against its neighbor.
/// Returns the swapped pair, or `None` if the swap is not yet supported.
/// An error raised by the swap is returned, as C leaves it in the global
/// status.
fn swap_pair(
    nclass: SwapClass,
    pair: [(DynMapping, bool, bool); 2],
    imm: usize,
) -> AstResult<Option<[(DynMapping, bool, bool); 2]>> {
    match nclass {
        // MatWin (matrixmap.c:3050) is the same swap as winmap's WinMat: it
        // absorbs the WinMap scale into the matrix, leaving a unit-scale
        // WinMap. `win_mat` takes the WinMap index, which is `1 - imm`.
        SwapClass::Win => super::winmap_merge::win_mat(pair, 1 - imm),
        SwapClass::Perm => mat_perm_swap(pair, imm),
    }
}

/// C `matrixmap.c::MatPermSwap` (2755): swap a MatrixMap and a PermMap,
/// producing an equivalent (PermMap, MatrixMap) pair in the opposite order.
/// `imm` is the index (0 or 1) of the MatrixMap within the pair.
///
/// The new MatrixMap is built by permuting the rows/columns of the supplied
/// matrix through the PermMap's connected links (constant axes become unit
/// rows), and the PermMap's constants are rescaled by the matrix's diagonal
/// scaling terms (which `CanSwap`/`ScalingRowCol` guaranteed exist). Both
/// results are returned simplified and forward, matching C.
fn mat_perm_swap(
    pair: [(DynMapping, bool, bool); 2],
    imm: usize,
) -> AstResult<Option<[(DynMapping, bool, bool); 2]>> {
    let mat_entry = &pair[imm];
    let perm_entry = &pair[1 - imm];
    let (Some(mm), Some(pm)) = (
        mat_entry.0.as_any().downcast_ref::<MatrixMap>(),
        perm_entry.0.as_any().downcast_ref::<PermMap>(),
    ) else {
        return Ok(None);
    };
    let mat_inv = mat_entry.1;
    let perm_inv = perm_entry.1;

    // Effective forward matrix of the MatrixMap (full form), row-major.
    // `CanSwap` admits only a MatrixMap with both matrices (matrixmap.c:466-469).
    let Some(matrix) = eff_matrix(mm, mat_inv) else {
        return Ok(None);
    };
    let nin_p = if perm_inv { pm.nout() } else { pm.nin() };
    let nout_p = if perm_inv { pm.nin() } else { pm.nout() };

    // 0-based perm arrays + constants (C PermGet).
    let (outperm, inperm, mut consts) = perm_get(pm, perm_inv);
    let nin = inperm.len(); // == nin_p
    let nout = outperm.len(); // == nout_p
    debug_assert_eq!(nin, nin_p);
    debug_assert_eq!(nout, nout_p);

    let out_mat: Array2<f64> = if imm == 0 {
        // MatrixMap applied first. New matrix is nout×nout.
        let mut om = Array2::<f64>::zeros((nout, nout));
        for i in 0..nout {
            let row = outperm[i];
            if row >= 0 && (row as usize) < nin {
                let row = row as usize;
                for k in 0..nout {
                    let col = outperm[k];
                    om[[i, k]] = if col >= 0 && (col as usize) < nin {
                        matrix[[row, col as usize]]
                    } else {
                        0.0
                    };
                }
            } else {
                // Constant output → unit row.
                om[[i, i]] = 1.0;
            }
        }
        // Rescale PermMap input constants by the matrix diagonal term
        // (divide, since the inverse matrix no longer scales them).
        for i in 0..nin {
            if inperm[i] < 0 {
                let ci = (-inperm[i] - 1) as usize;
                let c = consts[ci];
                if !is_bad(c) {
                    let matel = matrix[[i, i]];
                    consts[ci] = if matel != 0.0 && !is_bad(matel) {
                        c / matel
                    } else {
                        BAD_VALUE
                    };
                }
            }
        }
        om
    } else {
        // PermMap applied first. New matrix is nin×nin.
        let mut om = Array2::<f64>::zeros((nin, nin));
        for i in 0..nin {
            let row = inperm[i];
            if row >= 0 && (row as usize) < nout {
                let row = row as usize;
                for k in 0..nin {
                    let col = inperm[k];
                    om[[i, k]] = if col >= 0 && (col as usize) < nout {
                        matrix[[row, col as usize]]
                    } else {
                        0.0
                    };
                }
            } else {
                om[[i, i]] = 1.0;
            }
        }
        // Rescale PermMap output constants by the matrix diagonal term
        // (multiply, since the forward matrix no longer scales them).
        for i in 0..nout {
            if outperm[i] < 0 {
                let ci = (-outperm[i] - 1) as usize;
                let c = consts[ci];
                if !is_bad(c) {
                    let matel = matrix[[i, i]];
                    consts[ci] = if !is_bad(matel) { c * matel } else { BAD_VALUE };
                }
            }
        }
        om
    };

    let mmnew = MatrixMap::new(out_mat)?;

    // Rebuild the PermMap. Rust `PermMap::new` uses 1-based connected links
    // with negative constant refs; `perm_get` returned 0-based links, so add
    // 1 back to the non-negative entries.
    let to_one_based = |arr: &[i64]| -> Vec<i64> {
        arr.iter()
            .map(|&p| if p >= 0 { p + 1 } else { p })
            .collect()
    };
    let pmnew = PermMap::new(&to_one_based(&inperm), &to_one_based(&outperm), &consts)?;

    // Simplify both, recording their resulting Invert flags (C stores each in
    // its forward direction with the simplified Invert). The PermMap takes the
    // MatrixMap's slot (imm) and the MatrixMap takes the PermMap's slot.
    let sp = <dyn Mapping>::simplify(&pmnew)?;
    let sp_inv = sp.is_inverted();
    let mut sp_fwd = sp;
    if sp_inv {
        sp_fwd.invert();
    }
    sp_fwd.set_has_been_simplified(true);
    let sm = <dyn Mapping>::simplify(&mmnew)?;
    let sm_inv = sm.is_inverted();
    let mut sm_fwd = sm;
    if sm_inv {
        sm_fwd.invert();
    }
    sm_fwd.set_has_been_simplified(true);

    let mut out: [Option<(DynMapping, bool, bool)>; 2] = [None, None];
    out[imm] = Some((sp_fwd, sp_inv, false));
    out[1 - imm] = Some((sm_fwd, sm_inv, false));
    Ok(Some([out[0].take().unwrap(), out[1].take().unwrap()]))
}

/// Step 4: speculative swap with a neighbor, kept iff a swapped map then
/// simplifies to a different class (matrixmap.c:2201-2272).
fn try_speculative_swap(
    ctx: &mut MergeCtx,
    where_: usize,
    swaphi: bool,
    swaplo: bool,
) -> Option<usize> {
    for attempt in 0..2 {
        let (i1, swap_lower) = if attempt == 0 && swaplo {
            (where_ - 1, true)
        } else if attempt == 1 && swaphi {
            (where_, false)
        } else {
            continue;
        };

        let neigh_idx = if swap_lower { where_ - 1 } else { where_ + 1 };
        let Some(nclass) = swap_class_of(ctx.entry(neigh_idx).0.as_ref()) else {
            continue;
        };

        let imm = where_ - i1;
        let pair = clone_pair(ctx, i1);
        let swapped = match swap_pair(nclass, pair, imm) {
            Ok(Some(swapped)) => swapped,
            Ok(None) => continue,
            Err(e) => {
                ctx.fail(e);
                return None;
            }
        };

        // Keep the swap iff either swapped map simplifies to a different class
        // (C: astGetClass(smc) != astGetClass(mc)).
        let mut worthwhile = false;
        for entry in &swapped {
            match entry.0.simplify() {
                Ok(s) if s.as_any().type_id() != entry.0.as_any().type_id() => {
                    worthwhile = true;
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    ctx.fail(e);
                    return None;
                }
            }
        }

        if worthwhile {
            ctx.set_pair(i1, swapped[0].clone(), swapped[1].clone());
            return Some(i1);
        }
    }
    None
}

/// Clone the `(i1, i1+1)` pair without mutating the list.
fn clone_pair(ctx: &MergeCtx, i1: usize) -> [(DynMapping, bool, bool); 2] {
    let a = ctx.entry(i1);
    let b = ctx.entry(i1 + 1);
    [(a.0.clone_box(), a.1, a.2), (b.0.clone_box(), b.1, b.2)]
}

/// Step 1: C matrixmap.c:1857-1929 — try to replace the MatrixMap with a
/// simpler Mapping without reference to its neighbors. Returns the
/// replacement (in forward state) or `None`.
fn replace_with_simpler(
    mm: &MatrixMap,
    eff_inv: bool,
    nin: usize,
    nout: usize,
) -> Option<DynMapping> {
    // The effective forward matrix (honoring the applied direction) and the
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
    // `mm->f_matrix` with Invert=0 (matrixmap.c:1896-1918).
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
            let diag: Vec<f64> = (0..n).map(|i| fwd[[i, i]]).collect();
            return MatrixMap::diagonal(n, n, &diag)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }
    }

    None
}

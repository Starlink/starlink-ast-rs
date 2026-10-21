//! Faithful port of C `winmap.c::MapMerge` and its swap helpers.
//!
//! This implements [`WinMap`](super::winmap::WinMap)'s per-class
//! `map_merge` (C `winmap.c:884`). It owns the full strategy a nominated
//! WinMap uses to simplify a series/parallel sequence:
//!
//! 1. **Replace-with-simpler** — an all-zero-shift WinMap becomes a
//!    diagonal MatrixMap; an all-unit-scale WinMap becomes a ShiftMap
//!    (deferred to the end so the ShiftMap↔WinMap loop can't run forever).
//! 2. **Direct merge** with a mergeable neighbour (WinMap / ZoomMap /
//!    ShiftMap / UnitMap / diagonal MatrixMap), or the parallel-CmpMap
//!    distribution.
//! 3. **Swap toward the nearest mergeable target** — compute the distance
//!    to the nearest mergeable map on each side (passing only over
//!    MatrixMap / WcsMap / PermMap), swap that direction via
//!    [`win_mat`] / [`win_perm`] / [`win_wcs`], then merge at the new
//!    adjacency. `win_mat` absorbs the WinMap's scale into the matrix,
//!    leaving the WinMap scale exactly `1.0`.
//! 4. **Speculative swap** — even with no target, swap with a neighbour if
//!    the two neighbours can then merge, or either swapped map simplifies.
//!
//! The merge arithmetic (WinWin / WinZoom / WinShift / WinUnit /
//! WinDiagMat) is shared with `WinMap::try_merge` via
//! [`extract_linear_params`](super::winmap::extract_linear_params) and
//! [`compose_linear`](super::winmap::compose_linear); this module adds the
//! swap helpers and the orchestration that the global helpers only
//! approximated.

use crate::coords::PointSet;
use crate::mappings::helpers::ast_equal;

use super::matrix::{MatrixMap, is_diagonal};
use super::perm::PermMap;
use super::shift::ShiftMap;
use super::simplify::{MergeCtx, build_winperm_swap};
use super::unit::UnitMap;
use super::wcsmap::WcsMap;
use super::winmap::{WinMap, extract_linear_params};
use super::zoom::ZoomMap;
use super::{DynMapping, Mapping};

/// Classes a WinMap can merge with *directly* (C winmap.c:1145-1149):
/// another WinMap, a ZoomMap, a ShiftMap, a UnitMap, or a **diagonal**
/// MatrixMap. `inv` is the effective direction of `m`.
fn is_mergeable(m: &dyn Mapping, inv: bool) -> bool {
    if m.as_any().is::<WinMap>()
        || m.as_any().is::<ZoomMap>()
        || m.as_any().is::<ShiftMap>()
        || m.as_any().is::<UnitMap>()
    {
        return true;
    }
    if let Some(mm) = m.as_any().downcast_ref::<MatrixMap>() {
        return matrix_is_diagonal(mm, inv);
    }
    false
}

/// Whether a MatrixMap's effective forward matrix (honouring `inv`) is
/// diagonal. Mirrors `astIsDiagonal` applied to the supplied direction.
fn matrix_is_diagonal(mm: &MatrixMap, inv: bool) -> bool {
    let mat = if inv {
        match mm.inverse_matrix() {
            Some(m) => m,
            None => return false,
        }
    } else {
        mm.forward_matrix()
    };
    is_diagonal(mat)
}

/// Classes the WinMap can *swap* past while seeking a merge target:
/// MatrixMap, WcsMap, PermMap (C winmap.c:1465-1469).
fn is_swappable_class(m: &dyn Mapping) -> bool {
    m.as_any().is::<MatrixMap>() || m.as_any().is::<WcsMap>() || m.as_any().is::<PermMap>()
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

/// C `winmap.c::CanSwap` (258): can the WinMap and its neighbour swap, and
/// would the swap make either intrinsically simpler (`*simpler`)?
///
/// One of the pair must be the WinMap. Returns `(can_swap, simpler)`.
/// MatrixMap → always swappable. WcsMap → swappable iff the WinMap is a
/// unit transform on the WcsMap's celestial axes. PermMap → swappable iff
/// every connected axis link is bi-directional; `simpler` iff the swap
/// moves the WinMap to the side where the PermMap changes dimension
/// favourably.
fn can_swap(map1: &dyn Mapping, inv1: bool, map2: &dyn Mapping, inv2: bool) -> (bool, bool) {
    // Identify the non-WinMap partner.
    let (win, win_inv, nowin, nowin_is_map1) = if map1.as_any().is::<WinMap>() {
        (map1, inv1, map2, false)
    } else {
        (map2, inv2, map1, true)
    };

    if nowin.as_any().is::<MatrixMap>() {
        return (true, false);
    }

    if let Some(wcs) = nowin.as_any().downcast_ref::<WcsMap>() {
        // WinMap must be unit (shift 0, scale 1) on the celestial axes.
        let Some((shifts, scales)) = extract_linear_params(win, win_inv) else {
            return (false, false);
        };
        let axlon = wcs.lonax_0based();
        let axlat = wcs.latax_0based();
        let ok = axlon < shifts.len()
            && axlat < shifts.len()
            && shifts[axlon] == 0.0
            && scales[axlon] == 1.0
            && shifts[axlat] == 0.0
            && scales[axlat] == 1.0;
        return (ok, false);
    }

    if let Some(pm) = nowin.as_any().downcast_ref::<PermMap>() {
        let (outperm, inperm) = effective_perm(pm, perm_eff_inv(nowin_is_map1, inv1, inv2));
        let nin = inperm.len() as i64;
        let nout = outperm.len() as i64;
        // Bi-directionality check (winmap.c:387-417).
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
        // simpler: if PermMap precedes WinMap → nout > nin; else nin > nout
        // (winmap.c:419-427). `nowin == map1` means PermMap precedes WinMap.
        let simpler = if nowin_is_map1 {
            nout > nin
        } else {
            nin > nout
        };
        return (true, simpler);
    }

    (false, false)
}

/// The effective invert flag of the PermMap given which list slot it
/// occupies relative to the WinMap.
fn perm_eff_inv(nowin_is_map1: bool, inv1: bool, inv2: bool) -> bool {
    if nowin_is_map1 { inv1 } else { inv2 }
}

/// Return the PermMap's (outperm, inperm) as 0-based index arrays for the
/// given effective direction. Constants are encoded as negative entries in
/// the C convention; we preserve the raw values (negative = constant) but
/// shift connected links to 0-based. C `PermGet` returns 0-based arrays
/// with `outperm[i] = -1 - k` for constant `k`; here `outperm()`/`inperm()`
/// are stored 1-based for connected axes with negative constant refs, so we
/// translate.
fn effective_perm(pm: &PermMap, inv: bool) -> (Vec<i64>, Vec<i64>) {
    // Stored arrays use 1-based links, `0` for a BAD/unconnected axis, and
    // negative constant refs. C `PermGet` returns 0-based connected links and
    // encodes both constants *and* BAD axes as negative values, so CanSwap's
    // `outperm[i] >= 0 && outperm[i] < nin` test only ever fires on a genuine
    // connected link. Mirror that: connected `p >= 1` → 0-based `p - 1`; the
    // BAD marker `0` and existing negative constant refs both map to a
    // negative value that the `>= 0` test skips.
    let to_zero_based = |arr: &[i64]| -> Vec<i64> {
        arr.iter()
            .map(|&p| if p >= 1 { p - 1 } else { -1 })
            .collect()
    };
    let (out_raw, in_raw) = if inv {
        (pm.inperm().to_vec(), pm.outperm().to_vec())
    } else {
        (pm.outperm().to_vec(), pm.inperm().to_vec())
    };
    (to_zero_based(&out_raw), to_zero_based(&in_raw))
}

/// C `winmap.c::WinMat` (2793): swap a WinMap and a MatrixMap, absorbing
/// the WinMap's diagonal scale into the MatrixMap so the returned WinMap
/// has unit scale. `iwm` is the index (0 or 1) of the WinMap within the
/// pair. Returns the swapped `(first, second)` entries.
///
/// `y = M (s + diag(c) x)` (WinMap first, iwm==0) becomes
/// `M diag(c)` then a WinMap with scale 1 and shift `M s`.
pub(crate) fn win_mat(
    pair: [(DynMapping, bool, bool); 2],
    iwm: usize,
) -> Option<[(DynMapping, bool, bool); 2]> {
    let win = &pair[iwm];
    let mat_entry = &pair[1 - iwm];
    let mm = mat_entry.0.as_any().downcast_ref::<MatrixMap>()?;
    let mat_inv = mat_entry.1;

    // Effective WinMap terms (shift a, scale b) for its applied direction.
    let (a, b) = extract_linear_params(win.0.as_ref(), win.1)?;
    let nin = a.len();

    // Diagonal MatrixMap m1 holding the WinMap scale factors, as C makes it
    // (a unit MatrixMap when every scale equals 1 within astEQUAL).
    let m1_map = MatrixMap::diagonal(nin, nin, &b).ok()?;

    // The supplied MatrixMap in the direction it is applied, keeping its
    // storage form, as C uses it.
    let mut mm_map = mm.clone();
    if mat_inv {
        mm_map.invert();
    }

    // Shift terms as a single point (one row of `nin` coordinates).
    let pset1 = PointSet::from_rows([a.as_slice()]);

    let (m2_map, pset2): (MatrixMap, PointSet) = if iwm == 0 {
        // WinMap first, then MatrixMap. m2 = mm × m1 (mtr_mult returns a×self).
        let m2 = m1_map.mtr_mult(&mm_map).ok()?;
        // Transform shift point forward through the supplied matrix.
        let p2 = mm_map.transform(&pset1, true).ok()?;
        (m2, p2)
    } else {
        // MatrixMap first, then WinMap. m2 = m1 × mm.
        let m2 = mm_map.mtr_mult(&m1_map).ok()?;
        // Transform shift point through the *inverse* of the returned matrix.
        let p2 = m2.transform(&pset1, false).ok()?;
        (m2, p2)
    };

    // Returned WinMap: unit scales, shifts from pset2.
    let nout = pset2.ncoord();
    let new_shifts: Vec<f64> = (0..nout).map(|i| pset2.point(0)[i]).collect();
    let new_scales = vec![1.0; nout];
    let w1 = WinMap::new(&new_shifts, &new_scales).ok()?;

    // C simplifies both the returned WinMap and MatrixMap, and records each
    // one's resulting Invert flag. We do the same: simplify() may turn the
    // unit-scale WinMap into a ShiftMap, and the matrix into a ZoomMap etc.
    let sw1 = <dyn Mapping>::simplify(&w1).ok()?;
    let sw1_inv = sw1.is_inverted();
    let mut sw1_fwd = sw1;
    if sw1_inv {
        sw1_fwd.invert();
    }
    sw1_fwd.set_has_been_simplified(true);

    let sm2 = <dyn Mapping>::simplify(&m2_map).ok()?;
    let sm2_inv = sm2.is_inverted();
    let mut sm2_fwd = sm2;
    if sm2_inv {
        sm2_fwd.invert();
    }
    sm2_fwd.set_has_been_simplified(true);

    // The order is now swapped: the MatrixMap takes the WinMap's slot
    // (iwm) and the WinMap takes the matrix slot (1 - iwm).
    let mut out: [Option<(DynMapping, bool, bool)>; 2] = [None, None];
    out[iwm] = Some((sm2_fwd, sm2_inv, false));
    out[1 - iwm] = Some((sw1_fwd, sw1_inv, false));
    Some([out[0].take().unwrap(), out[1].take().unwrap()])
}

/// C `winmap.c::WinPerm` (2973): swap a WinMap and a PermMap. `iwm` is the
/// index of the WinMap within the pair.
fn win_perm(
    pair: [(DynMapping, bool, bool); 2],
    iwm: usize,
) -> Option<[(DynMapping, bool, bool); 2]> {
    let win = &pair[iwm];
    let perm_entry = &pair[1 - iwm];
    let pm = perm_entry.0.as_any().downcast_ref::<PermMap>()?;
    let perm_inv = perm_entry.1;

    let (shifts, scales) = extract_linear_params(win.0.as_ref(), win.1)?;
    let (outperm_eff, inperm_eff) = if perm_inv {
        (pm.inperm().to_vec(), pm.outperm().to_vec())
    } else {
        (pm.outperm().to_vec(), pm.inperm().to_vec())
    };
    let consts = pm.constants().to_vec();

    // `linear_iwm` for build_winperm_swap: 0 means Linear before PermMap.
    let linear_iwm = iwm;
    let (new_lin, new_pm) = build_winperm_swap(
        linear_iwm,
        &shifts,
        &scales,
        &outperm_eff,
        &inperm_eff,
        &consts,
    )?;

    // After the swap the PermMap occupies the WinMap's slot and vice-versa.
    // C simplifies both and records the resulting Invert flags.
    let sp1 = <dyn Mapping>::simplify(&new_pm).ok()?;
    let sp1_inv = sp1.is_inverted();
    let mut sp1_fwd = sp1;
    if sp1_inv {
        sp1_fwd.invert();
    }
    sp1_fwd.set_has_been_simplified(true);
    let sw1 = new_lin.simplify().ok()?;
    let sw1_inv = sw1.is_inverted();
    let mut sw1_fwd = sw1;
    if sw1_inv {
        sw1_fwd.invert();
    }
    sw1_fwd.set_has_been_simplified(true);

    let mut out: [Option<(DynMapping, bool, bool)>; 2] = [None, None];
    out[iwm] = Some((sp1_fwd, sp1_inv, false));
    out[1 - iwm] = Some((sw1_fwd, sw1_inv, false));
    Some([out[0].take().unwrap(), out[1].take().unwrap()])
}

/// C `winmap.c::WinWcs` (3833): swap a WinMap and a WcsMap. The values are
/// simply exchanged (CanSwap already guaranteed the WinMap is unit on the
/// celestial axes, so positional reorder preserves the transform).
fn win_wcs(pair: [(DynMapping, bool, bool); 2], _iwm: usize) -> [(DynMapping, bool, bool); 2] {
    let [a, b] = pair;
    [b, a]
}

/// Dispatch the swap of the WinMap (at index `iwm` within the pair)
/// against its neighbour of class `nclass`, returning the swapped pair.
fn swap_pair(
    nclass: SwapClass,
    pair: [(DynMapping, bool, bool); 2],
    iwm: usize,
) -> Option<[(DynMapping, bool, bool); 2]> {
    match nclass {
        SwapClass::Matrix => win_mat(pair, iwm),
        SwapClass::Perm => win_perm(pair, iwm),
        SwapClass::Wcs => Some(win_wcs(pair, iwm)),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SwapClass {
    Matrix,
    Perm,
    Wcs,
}

fn swap_class_of(m: &dyn Mapping) -> Option<SwapClass> {
    if m.as_any().is::<MatrixMap>() {
        Some(SwapClass::Matrix)
    } else if m.as_any().is::<PermMap>() {
        Some(SwapClass::Perm)
    } else if m.as_any().is::<WcsMap>() {
        Some(SwapClass::Wcs)
    } else {
        None
    }
}

/// Entry point: WinMap's per-class `map_merge`. Faithful port of
/// `winmap.c::MapMerge` (884). `where_` is the nominated WinMap's index in
/// `ctx.list`; returns the index of the first modified element, or `None`.
pub(crate) fn win_map_merge(
    win: &WinMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    let nin = win.nin();
    let win_inv = ctx.entry(where_).1;

    // ---- Step 1: replace-with-simpler (all shifts zero → diagonal matrix).
    // C uses the *raw* stored shift terms (newwm->a) for the zero test, then
    // builds a diagonal MatrixMap from the effective scale terms.
    let (eff_shifts, eff_scales) = extract_linear_params(ctx.entry(where_).0.as_ref(), win_inv)?;
    let all_shifts_zero = win.shifts().iter().all(|&a| ast_equal(a, 0.0));
    if all_shifts_zero {
        if let Ok(mm) = MatrixMap::diagonal(nin, nin, &eff_scales) {
            ctx.set_pair_single(where_, (Box::new(mm), false, false));
            return Some(where_);
        }
    }

    // ---- Step 2: direct merge with a mergeable neighbour.
    if let Some(result) = try_direct_merge(ctx, where_, series, win_inv) {
        return Some(result);
    }

    // ---- Step 2b (series): distribute the WinMap into a neighbouring
    // parallel CmpMap (C winmap.c:1264-1416).
    if series {
        if let Some(result) = try_distribute_into_parallel(ctx, where_, win_inv) {
            return Some(result);
        }
    }

    // ---- Steps 3 & 4 apply to series only.
    if series {
        if let Some(result) = try_swap_strategy(ctx, where_, series) {
            return Some(result);
        }
    }

    // ---- Final step: all-unit-scale WinMap → ShiftMap (deferred so the
    // ShiftMap↔WinMap loop can't run forever). Uses the *raw* scale terms.
    let all_scales_one = win.scales().iter().all(|&b| ast_equal(b, 1.0));
    if all_scales_one {
        let shifts: Vec<f64> = eff_shifts.clone();
        let sm = ShiftMap::new(shifts);
        ctx.set_pair_single(where_, (Box::new(sm), false, false));
        return Some(where_);
    }

    // ---- Scaffolding fallback (retired as neighbour classes gain their own
    // `map_merge` in later phases). C's WinMap::MapMerge declines here and
    // relies on the *neighbour's* MapMerge to absorb this WinMap when the
    // neighbour is later nominated (e.g. `dssmap.c::MapMerge` absorbing a
    // preceding WinMap). For classes that do not yet have their own
    // `map_merge`, that backward absorption is still provided by
    // `default_map_merge`'s reverse merge on the `(where_, where_+1)` pair.
    // Delegating to it here triggers that absorption at this WinMap's
    // nomination instead of the neighbour's — the nominate loop runs to a
    // fixpoint, so which nomination performs the merge does not change the
    // final result.
    super::simplify::default_map_merge(ctx, where_, series)
}

/// Step 2: try a direct merge of the nominated WinMap with whichever
/// neighbour is mergeable. C prefers the lower neighbour (class1) then the
/// upper (class2). Returns the modified index on success.
fn try_direct_merge(
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
    win_inv: bool,
) -> Option<usize> {
    let n = ctx.len();
    // Lower neighbour first (i1 = where-1), else upper (i2 = where+1).
    let lower_ok =
        where_ > 0 && is_mergeable(ctx.entry(where_ - 1).0.as_ref(), ctx.entry(where_ - 1).1);
    let upper_ok =
        where_ + 1 < n && is_mergeable(ctx.entry(where_ + 1).0.as_ref(), ctx.entry(where_ + 1).1);

    let (i1, i2) = if lower_ok {
        (where_ - 1, where_)
    } else if upper_ok {
        (where_, where_ + 1)
    } else {
        return None;
    };

    // Compose the two linear maps in series/parallel.
    let (m1, inv1, _) = ctx.entry(i1);
    let (m2, inv2, _) = ctx.entry(i2);
    let merged = merge_linear_pair(m1.as_ref(), *inv1, m2.as_ref(), *inv2, series, win_inv)?;
    ctx.replace_pair_with(i1, merged, false);
    Some(i1)
}

/// Step 2b (series): distribute the nominated WinMap into a neighbouring
/// parallel CmpMap. Faithful port of C `winmap.c::MapMerge` (1264-1416).
///
/// When the WinMap precedes the parallel CmpMap (`cmlow == false`) it acts on
/// the CmpMap's inputs, so it is split by each component's *input* count and
/// each branch becomes `sub_win ; component`. When the WinMap follows the
/// CmpMap (`cmlow == true`) it acts on the outputs, so it is split by each
/// component's *output* count and each branch becomes `component ; sub_win`.
///
/// The two per-branch series CmpMaps are each simplified; only if at least one
/// simplifies (C's `simpler` test, avoiding an infinite parallel↔series loop)
/// are they combined into a replacement parallel CmpMap that takes the slot of
/// the two merged mappings. The recombination back to the visible series form
/// happens when that parallel CmpMap is next nominated (C: CmpMap MapMerge's
/// self-simplify runs the "Series CmpMaps in parallel" merge, cmpmap.c).
fn try_distribute_into_parallel(ctx: &mut MergeCtx, where_: usize, win_inv: bool) -> Option<usize> {
    use super::cmpmap::{CmpMap, Composition};
    use super::simplify::reconstruct;

    let n = ctx.len();

    // Identify a parallel-CmpMap neighbour. C prefers the lower neighbour
    // (class1, cmlow == 1) then the upper (class2, cmlow == 0).
    let is_parallel = |m: &dyn Mapping| -> bool {
        m.as_any()
            .downcast_ref::<CmpMap>()
            .is_some_and(|c| c.composition() == Composition::Parallel)
    };
    let lower_par = where_ > 0 && is_parallel(ctx.entry(where_ - 1).0.as_ref());
    let upper_par = where_ + 1 < n && is_parallel(ctx.entry(where_ + 1).0.as_ref());

    let (i1, i2, cmlow) = if lower_par {
        (where_ - 1, where_, true)
    } else if upper_par {
        (where_, where_ + 1, false)
    } else {
        return None;
    };

    // Effective forward (shift a, scale b) terms of the nominated WinMap.
    let (a, b) = extract_linear_params(ctx.entry(where_).0.as_ref(), win_inv)?;

    // C `astDecompose` yields the parallel CmpMap's two direct components
    // (mc[0], mc[1]); the WinMap is split by each component's dimension.
    let par_idx = if cmlow { i1 } else { i2 };
    let par_inv = ctx.entry(par_idx).1;
    let cm = ctx.entry(par_idx).0.as_any().downcast_ref::<CmpMap>()?;
    // Direct children stripped to their forward orientation, each paired with
    // the absolute direction the CmpMap applies to it.
    //
    // C reads that direction straight off `astDecompose`, which returns the
    // recorded `invert1`/`invert2` snapshots (cmpmap.c:758-786), and then
    // *overwrites* each component's own stored `Invert` with it
    // (`astSetInvert( mc[ i ], ic[ i ] )`, winmap.c:1310-1314). So the
    // direction is the raw snapshot XOR the CmpMap's own inversion — the
    // component's stored `Invert` does not enter it. The
    // `inv_a != first.is_inverted()` form used elsewhere is the *relative*
    // flag of cmpmap.c:3862, which only becomes absolute once the component is
    // applied in its stored orientation rather than stripped forward.
    //
    // The component's stored `Invert` is still needed, though: C restores it
    // after building the branch (`astSetInvert( mc[ i ], inc[ i + 2 ] )`,
    // winmap.c:1379-1380) and, because the branch holds the same pointer, the
    // component inside the branch is restored with it — leaving the branch's
    // `InvA`/`InvB` carrying the effective direction and the component itself
    // serialised in its original orientation. So carry that orientation
    // alongside as the element's `orig_inv`.
    let cmp_inv = par_inv != cm.is_inverted();
    let child_a_inv = cmp_inv != cm.inv_a();
    let child_b_inv = cmp_inv != cm.inv_b();
    let child_a_orig = cm.first().is_inverted();
    let child_b_orig = cm.second().is_inverted();
    let mut child_a = cm.first().clone_box();
    if child_a.is_inverted() {
        child_a.invert();
    }
    let mut child_b = cm.second().clone_box();
    if child_b.is_inverted() {
        child_b.invert();
    }
    let components: [(DynMapping, bool, bool); 2] = [
        (child_a, child_a_inv, child_a_orig),
        (child_b, child_b_inv, child_b_orig),
    ];

    // Split-dimension for each component: outputs when the CmpMap is the lower
    // neighbour (WinMap acts on outputs), inputs otherwise.
    let split_dim = |m: &dyn Mapping, inv: bool| -> usize {
        // Effective outputs (cmlow) or inputs, honouring the component's
        // applied inversion.
        if cmlow {
            if inv { m.nin() } else { m.nout() }
        } else if inv {
            m.nout()
        } else {
            m.nin()
        }
    };
    let dim0 = split_dim(components[0].0.as_ref(), components[0].1);
    let dim1 = split_dim(components[1].0.as_ref(), components[1].1);
    if dim0 + dim1 != a.len() {
        return None;
    }

    // Build the two per-branch series CmpMaps and simplify each.
    let mut branches: Vec<(DynMapping, bool, bool)> = Vec::with_capacity(2);
    let mut simpler = false;
    for (idx, (comp, comp_inv, comp_orig)) in components.iter().enumerate() {
        let (start, dim) = if idx == 0 { (0, dim0) } else { (dim0, dim1) };
        let sub_win = WinMap::new(&a[start..start + dim], &b[start..start + dim]).ok()?;

        let mut comp_clone = comp.clone_box();
        if *comp_inv {
            comp_clone.invert();
        }

        let mut series_branch = if cmlow {
            CmpMap::new(comp_clone, Box::new(sub_win), Composition::Series)
        } else {
            CmpMap::new(Box::new(sub_win), comp_clone, Composition::Series)
        }
        .ok()?;
        // `CmpMap::new` has captured the effective direction into the branch's
        // `InvA`/`InvB`; now put the component back into the orientation it is
        // stored in, as C's `astSetInvert( mc[ i ], inc[ i + 2 ] )` restore
        // (winmap.c:1379-1380) does through the pointer the branch shares.
        if cmlow {
            // `C-ARTIFACT` (see cmpmap.rs's module docs): restores the
            // component's stored orientation for the dump only.
            series_branch.set_first_orientation(*comp_orig);
        } else {
            series_branch.set_second_orientation(*comp_orig);
        }

        let branch_dyn: &dyn Mapping = &series_branch;
        let (simplified, changed) = branch_dyn.simplify_reporting_change().ok()?;
        // C's test is `simpler = ( simp1 != nc[0] ) || ( simp2 != nc[1] )`
        // (winmap.c:1370) — pointer identity, which is what `changed` reports.
        if changed {
            simpler = true;
        }
        branches.push((simplified, false, false));
    }

    // Only commit the distribution if at least one branch simplified (C's
    // `simpler` guard against an infinite parallel↔series oscillation).
    if !simpler {
        return None;
    }

    // C commits `astCmpMap(simp1, simp2, 0)`, the parallel-of-series form
    // (winmap.c:1386). Its recombination into series form happens when that
    // CmpMap is next nominated, through CmpMap's own merge, so the order of
    // later merges, and which Mappings an oscillation freezes, follow C's.
    let new_par = reconstruct(branches, false).ok()?;
    ctx.replace_range_with(i1, i2, new_par);
    Some(i1)
}

/// Compose two linear mappings (one of which is the WinMap) into a single
/// mapping, in series or parallel. Reuses the shared linear arithmetic.
///
/// The product is always a `WinMap`, matching C: every direct-merge branch of
/// `winmap.c::MapMerge` installs the `AstWinMap *` returned by `WinWin` /
/// `WinZoom` / `WinShift` / `WinDiagMat`, and the collapse to a ShiftMap is
/// deferred to `MapMerge`'s final step.
fn merge_linear_pair(
    m1: &dyn Mapping,
    inv1: bool,
    m2: &dyn Mapping,
    inv2: bool,
    series: bool,
    _win_inv: bool,
) -> Option<DynMapping> {
    let (s1, c1) = extract_linear_params(m1, inv1)?;
    let (s2, c2) = extract_linear_params(m2, inv2)?;
    if series {
        // Equal-and-opposite WinWin cancellation is handled inside
        // compose_linear via its rounding snaps; for the general case
        // out = s2 + c2*(s1 + c1*x). Only the WinMap+WinMap pair goes
        // through C's WinWin and gets the snaps; the other neighbour
        // classes (WinShift/WinZoom/WinUnit/WinDiagMat) do not snap.
        if s1.len() != s2.len() {
            return None;
        }
        let both_win =
            m1.as_any().is::<super::winmap::WinMap>() && m2.as_any().is::<super::winmap::WinMap>();
        super::winmap::compose_linear_win(&s1, &c1, &s2, &c2, both_win)
    } else {
        // Parallel: concatenate.
        let mut shifts = s1;
        shifts.extend(s2);
        let mut scales = c1;
        scales.extend(c2);
        WinMap::new(&shifts, &scales)
            .ok()
            .map(|m| Box::new(m) as DynMapping)
    }
}

/// Steps 3 & 4 (series): seek the nearest mergeable target on each side,
/// swap the WinMap toward it, and merge; failing that, do a speculative
/// swap kept iff the neighbours then merge or a swapped map simplifies.
fn try_swap_strategy(ctx: &mut MergeCtx, where_: usize, series: bool) -> Option<usize> {
    let n = ctx.len();

    // ---- Determine swaphi/do2 (upper) and swaplo/do1 (lower).
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

    // ---- nstep2: distance forward to nearest mergeable target.
    let mut nstep2: i64 = -1;
    if swaphi {
        for i2 in (where_ + 1)..n {
            let (m, inv, _) = ctx.entry(i2);
            if is_mergeable(m.as_ref(), *inv) {
                nstep2 = (i2 - where_ - 1) as i64;
                break;
            }
            if !is_swappable_class(m.as_ref()) {
                break;
            }
        }
    }

    // ---- nstep1: distance backward to nearest mergeable target.
    let mut nstep1: i64 = -1;
    if swaplo {
        for i1 in (0..where_).rev() {
            let (m, inv, _) = ctx.entry(i1);
            if is_mergeable(m.as_ref(), *inv) {
                nstep1 = (where_ - 1 - i1) as i64;
                break;
            }
            if !is_swappable_class(m.as_ref()) {
                break;
            }
        }
    }

    // ---- Choose the swap direction (winmap.c:1510-1524).
    let (i1, swap_lower) = if do1 || (nstep1 != -1 && (nstep2 == -1 || nstep2 > nstep1)) {
        (where_.wrapping_sub(1), true)
    } else if do2 || nstep2 != -1 {
        (where_, false)
    } else {
        // No target — fall through to the speculative swap (step 4).
        return try_speculative_swap(ctx, where_, series, swaphi, swaplo);
    };

    // The neighbour class for the chosen direction.
    let neigh_idx = if swap_lower { where_ - 1 } else { where_ + 1 };
    let nclass = swap_class_of(ctx.entry(neigh_idx).0.as_ref())?;

    // Swap the chosen pair to bring the WinMap one step toward its target.
    // iwm = position of the WinMap within the (i1, i1+1) pair = where_ - i1.
    let iwm = where_ - i1;
    let pair = clone_pair(ctx, i1);
    let swapped = swap_pair(nclass, pair, iwm)?;
    ctx.set_pair(i1, swapped[0].clone(), swapped[1].clone());

    // Merge at the new adjacency (winmap.c:1537-1559). When merging upward,
    // the WinMap is now at where_+1's old slot; we nominate from `where_+1`
    // within the suffix starting at `where_+1`. When merging downward, the
    // suffix starts at `where_-2`.
    if i1 == where_ && where_ + 1 < ctx.len() {
        // Merged upwards: WinMap moved to where_+1.
        let _ = ctx.merge_subrange(where_ + 1, 0, series);
    } else if where_ >= 2 {
        // Merged downwards.
        let _ = ctx.merge_subrange(where_ - 2, 0, series);
    }

    Some(i1)
}

/// Step 4: speculative swap with a neighbour, kept iff the two neighbours
/// can then merge, or either swapped map simplifies (winmap.c:1565-1660).
fn try_speculative_swap(
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
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

        let iwm = where_ - i1;
        // Copy the pair (do not mutate the list yet).
        let pair = clone_pair(ctx, i1);
        let Some(swapped) = swap_pair(nclass, pair, iwm) else {
            continue;
        };

        // Determine if the swap is worthwhile: do the two outer neighbours
        // merge now, or does a swapped map simplify?
        let worthwhile = swap_is_worthwhile(ctx, where_, i1, swap_lower, &swapped, series);

        if worthwhile {
            ctx.set_pair(i1, swapped[0].clone(), swapped[1].clone());
            return Some(i1);
        }
    }
    None
}

/// Whether a speculative swap should be kept: the two neighbours surrounding
/// the swapped WinMap can merge, OR one of the swapped maps simplifies.
fn swap_is_worthwhile(
    ctx: &mut MergeCtx,
    where_: usize,
    _i1: usize,
    swap_lower: bool,
    swapped: &[(DynMapping, bool, bool); 2],
    series: bool,
) -> bool {
    // After swapping, the WinMap has moved to the outer side. Identify the
    // pair of mappings that are now adjacent (excluding the moved WinMap).
    let n = ctx.len();
    let neighbours: Option<((DynMapping, bool), (DynMapping, bool))> = if swap_lower {
        // Lower swap: swapped[1] (the moved-down neighbour) is now adjacent
        // to list[where_+1].
        if where_ + 1 < n {
            Some((
                (swapped[1].0.clone_box(), swapped[1].1),
                (ctx.entry(where_ + 1).0.clone_box(), ctx.entry(where_ + 1).1),
            ))
        } else {
            None
        }
    } else {
        // Upper swap: list[where_-1] is now adjacent to swapped[0].
        if where_ > 0 {
            Some((
                (ctx.entry(where_ - 1).0.clone_box(), ctx.entry(where_ - 1).1),
                (swapped[0].0.clone_box(), swapped[0].1),
            ))
        } else {
            None
        }
    };

    if let Some(((ma, ia), (mb, ib))) = neighbours {
        // Do the neighbours merge? Probe via a 2-element MergeCtx.
        let mut probe = MergeCtx::new(vec![(ma, ia, false), (mb, ib, false)]);
        let nominee = probe.entry(0).0.clone_box();
        let merged = super::simplify::dispatch_map_merge(nominee.as_ref(), &mut probe, 0, series);
        // C's probe shares the global status, so an error it raises fails the
        // enclosing simplification.
        if let Some(e) = probe.take_error() {
            ctx.fail(e);
            return false;
        }
        if merged.is_some() && probe.len() < 2 {
            return true;
        }
    }

    // Otherwise, does either swapped map simplify to a different class?
    for entry in swapped.iter() {
        if let Ok(simp) = entry.0.simplify() {
            if simplifies_to_different(entry.0.as_ref(), simp.as_ref()) {
                return true;
            }
        }
    }
    false
}

/// Whether `simp` is a structurally different (simpler) mapping than `orig`
/// — the Rust analogue of C's `simp != orig` pointer-inequality test
/// (winmap.c:1645-1656). A change of concrete class is the observable
/// signal that `astSimplify` produced something simpler (e.g. a unit-scale
/// WinMap collapsing to a ShiftMap, or a diagonal MatrixMap to a ZoomMap).
fn simplifies_to_different(orig: &dyn Mapping, simp: &dyn Mapping) -> bool {
    orig.as_any().type_id() != simp.as_any().type_id()
}

/// Clone the `(i1, i1+1)` pair without mutating the list.
fn clone_pair(ctx: &MergeCtx, i1: usize) -> [(DynMapping, bool, bool); 2] {
    [clone_entry(ctx.entry(i1)), clone_entry(ctx.entry(i1 + 1))]
}

/// Deep-clone a working-list entry.
fn clone_entry(e: &(DynMapping, bool, bool)) -> (DynMapping, bool, bool) {
    (e.0.clone_box(), e.1, e.2)
}

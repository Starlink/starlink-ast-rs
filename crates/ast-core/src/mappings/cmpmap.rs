//! Compound mapping (series and parallel composition).
//!
//! `CmpMap` composes two mappings either:
//!
//! - **Series**: apply `first`, then `second`. Requires `first.nout() ==
//!   second.nin()`.
//! - **Parallel**: apply `first` and `second` side-by-side to disjoint
//!   coordinate ranges, then concatenate their outputs.

use std::fmt;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{
    MappingToNode, Node, Serializable, mapping_from_node, read_mapping_base, write_mapping_base,
};

use super::{DynMapping, Mapping};

/// Split `child` in the requested direction. If the direction matches the
/// child's current orientation, just call its `map_split`. Otherwise,
/// clone + invert the child and split the inverted view — this lets the
/// per-class `map_split` handle the effective forward form without
/// needing a `forward` parameter on the trait.
///
/// Used by [`CmpMap::map_split_forward`].
fn split_child(
    child: &dyn Mapping,
    forward: bool,
    in_axes: &[usize],
) -> Option<(Vec<usize>, DynMapping)> {
    if forward {
        child.map_split_zero_based(in_axes)
    } else {
        let mut tmp = child.clone_box();
        tmp.invert();
        tmp.map_split_zero_based(in_axes)
    }
}

/// Determines whether two mappings are composed in series or in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composition {
    /// Apply `first` then `second`.  `first.nout()` must equal `second.nin()`.
    Series,
    /// Apply `first` and `second` to separate coordinate ranges and
    /// concatenate the results.
    Parallel,
}

/// A mapping formed by composing two component mappings.
///
/// See [`Composition`] for the two modes.
///
/// `inv_a`/`inv_b` record each child's Invert flag at the time the CmpMap
/// was constructed or loaded.  Children are stored with their native Invert
/// state; the effective transform direction for each child is computed at
/// runtime as `forward XOR (inv_a != child.is_inverted())`.
pub struct CmpMap {
    first: DynMapping,
    second: DynMapping,
    kind: Composition,
    inverted: bool,
    has_been_simplified: bool,
    /// Whether the forward transform is defined.
    pub(crate) tran_forward: bool,
    /// Whether the inverse transform is defined.
    pub(crate) tran_inverse: bool,
    /// Recorded Invert flag for first child (snapshot at construction/load).
    inv_a: bool,
    /// Recorded Invert flag for second child (snapshot at construction/load).
    inv_b: bool,
}

impl CmpMap {
    /// Create a new `CmpMap`.
    ///
    /// - `Series`: `first.nout()` must equal `second.nin()`.
    /// - `Parallel`: no dimension constraint.
    pub fn new(first: DynMapping, second: DynMapping, kind: Composition) -> AstResult<Self> {
        if kind == Composition::Series && first.nout() != second.nin() {
            return Err(AstError::new(
                ErrorCode::Innco,
                format!(
                    "series CmpMap: first.nout()={} but second.nin()={}",
                    first.nout(),
                    second.nin()
                ),
            ));
        }
        let inv_a = first.is_inverted();
        let inv_b = second.is_inverted();
        // Both the series forward path and the parallel forward path apply
        // both components forwards, so each direction is defined only when
        // it is defined for both components (cmpmap.c GetTranForward /
        // GetTranInverse).
        let tran_forward = first.has_forward() && second.has_forward();
        let tran_inverse = first.has_inverse() && second.has_inverse();
        Ok(Self {
            first,
            second,
            kind,
            inverted: false,
            has_been_simplified: false,
            tran_forward,
            tran_inverse,
            inv_a,
            inv_b,
        })
    }

    /// The first component mapping.
    pub(crate) fn first(&self) -> &dyn Mapping {
        self.first.as_ref()
    }

    /// The second component mapping.
    pub(crate) fn second(&self) -> &dyn Mapping {
        self.second.as_ref()
    }

    /// The composition mode (series or parallel).
    pub(crate) fn composition(&self) -> Composition {
        self.kind
    }

    /// Recorded Invert flag for first child.
    pub(crate) fn inv_a(&self) -> bool {
        self.inv_a
    }

    /// Recorded Invert flag for second child.
    pub(crate) fn inv_b(&self) -> bool {
        self.inv_b
    }

    /// Recursively clear redundant `InvA` / `InvB` flags on a parallel
    /// CmpMap whose corresponding child is a `UnitMap`. The C reference
    /// produces this canonical form: since `UnitMap` is its own inverse,
    /// the parent's external invert flag is observationally irrelevant
    /// and is dropped from the recorded snapshot, leaving the child's
    /// intrinsic `Invert` state untouched. Series CmpMaps are unaffected
    /// because the `simplify` merge loop already removes UnitMap entries
    /// from a series chain.
    pub(crate) fn normalize_unitmap_children(&mut self) {
        // Recurse into children first.
        if let Some(child_a) = self.first.as_any_mut().downcast_mut::<CmpMap>() {
            child_a.normalize_unitmap_children();
        }
        if let Some(child_b) = self.second.as_any_mut().downcast_mut::<CmpMap>() {
            child_b.normalize_unitmap_children();
        }

        if self.kind != Composition::Parallel {
            return;
        }

        if self.inv_a && self.first.as_any().is::<super::unit::UnitMap>() {
            self.inv_a = false;
        }
        if self.inv_b && self.second.as_any().is::<super::unit::UnitMap>() {
            self.inv_b = false;
        }
    }

    /// Recursively canonicalise CmpMaps so no nested CmpMap carries
    /// `inverted=true`. For each CmpMap with `inverted=true`, push the
    /// inversion into its recorded structure (`invert_into_structure`)
    /// and clear the flag. After this pass, every CmpMap in the tree
    /// has `inverted=false`. The parent's `inv_a` / `inv_b` snapshots
    /// are updated to maintain the runtime invariant
    /// `effective_inversion = inv_a XOR child.is_inverted()`. This is a
    /// serialisation-canonicalisation pass — runtime behaviour is
    /// unchanged. Mirrors the C reference's emission of a parallel
    /// CmpMap with an `InvA=1` CmpMap child as: outer parallel with
    /// no `InvA`, inner CmpMap with `inv_a`/`inv_b` flipped (and
    /// children swapped for series).
    pub(crate) fn canonicalize_inverted_cmpmaps(&mut self) {
        // Recurse first; children's is_inverted() may flip during the
        // call, requiring our inv_a/inv_b snapshots to flip in step.
        let first_was_inverted = self.first.is_inverted();
        if let Some(cm) = self.first.as_any_mut().downcast_mut::<CmpMap>() {
            cm.canonicalize_inverted_cmpmaps();
        }
        if first_was_inverted != self.first.is_inverted() {
            self.inv_a = !self.inv_a;
        }

        let second_was_inverted = self.second.is_inverted();
        if let Some(cm) = self.second.as_any_mut().downcast_mut::<CmpMap>() {
            cm.canonicalize_inverted_cmpmaps();
        }
        if second_was_inverted != self.second.is_inverted() {
            self.inv_b = !self.inv_b;
        }

        // Now canonicalise self.
        if self.inverted {
            self.invert_into_structure();
            self.inverted = false;
        }
    }

    /// Push the inversion semantics of "this CmpMap, used in its inverse
    /// direction" into the recorded structure, leaving `inverted` flag
    /// unchanged. For parallel CmpMaps this flips `inv_a` / `inv_b`. For
    /// series CmpMaps it additionally swaps `first` / `second` (since
    /// `(A;B)^-1 = B^-1; A^-1`). Mathematically equivalent to invert(),
    /// but produces the canonical serialised form the C reference emits
    /// when an inverted CmpMap appears as a child of a parallel
    /// reconstruct.
    pub(crate) fn invert_into_structure(&mut self) {
        match self.kind {
            Composition::Parallel => {
                self.inv_a = !self.inv_a;
                self.inv_b = !self.inv_b;
            }
            Composition::Series => {
                std::mem::swap(&mut self.first, &mut self.second);
                let (new_inv_a, new_inv_b) = (!self.inv_b, !self.inv_a);
                self.inv_a = new_inv_a;
                self.inv_b = new_inv_b;
            }
        }
    }

    /// Set the *stored* Invert orientation of the first child to `orig`,
    /// leaving `inv_a` untouched. The CmpMap's transform is unchanged: the
    /// child's effective direction is `inv_a != first.is_inverted()`, and the
    /// child's own Invert otherwise cancels (its `nin`/`nout` swap with the
    /// flag, offsetting the change). This only affects serialisation — the
    /// child then dumps in the `orig` orientation while `InvA` carries the
    /// effective direction, exactly as C `cmpmap.c` Simplify does (it captures
    /// `invert1` from the component, then restores the component's own Invert
    /// to its original value before dumping).
    pub(crate) fn set_first_orientation(&mut self, orig: bool) {
        if self.first.is_inverted() != orig {
            self.first.invert();
        }
    }

    /// Set the stored Invert orientation of the second child to `orig`,
    /// leaving `inv_b` untouched. See [`Self::set_first_orientation`].
    pub(crate) fn set_second_orientation(&mut self, orig: bool) {
        if self.second.is_inverted() != orig {
            self.second.invert();
        }
    }

    /// Whether the first child's current Invert differs from the recorded
    /// snapshot — if so, the effective transform direction must flip.
    fn first_invert_changed(&self) -> bool {
        self.inv_a != self.first.is_inverted()
    }

    /// Whether the second child's current Invert differs from the recorded
    /// snapshot.
    fn second_invert_changed(&self) -> bool {
        self.inv_b != self.second.is_inverted()
    }

    /// Effective Nin of first child when the CmpMap is used in direction `fwd`.
    fn effective_first_nin(&self, fwd: bool) -> usize {
        let fwd1 = fwd != self.first_invert_changed();
        if fwd1 {
            self.first.nin()
        } else {
            self.first.nout()
        }
    }

    /// Effective Nout of first child when the CmpMap is used in direction `fwd`.
    fn effective_first_nout(&self, fwd: bool) -> usize {
        let fwd1 = fwd != self.first_invert_changed();
        if fwd1 {
            self.first.nout()
        } else {
            self.first.nin()
        }
    }

    /// Effective Nin of second child when the CmpMap is used in direction `fwd`.
    fn effective_second_nin(&self, fwd: bool) -> usize {
        let fwd2 = fwd != self.second_invert_changed();
        if fwd2 {
            self.second.nin()
        } else {
            self.second.nout()
        }
    }

    /// Effective Nout of second child when the CmpMap is used in direction `fwd`.
    fn effective_second_nout(&self, fwd: bool) -> usize {
        let fwd2 = fwd != self.second_invert_changed();
        if fwd2 {
            self.second.nout()
        } else {
            self.second.nin()
        }
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    // ------------------------------------------------------------------ //
    // map_split helpers (forward decomposition + MapSplit2 fallback)      //
    // ------------------------------------------------------------------ //

    /// Forward Series/Parallel decomposition for [`Mapping::map_split`].
    ///
    /// Mirrors `cmpmap.c::MapSplit0` (~line 2405). For each child we
    /// compute its effective direction (folding `self.inverted` and any
    /// recorded `inv_a`/`inv_b` drift relative to the child's current
    /// `is_inverted` state), call `map_split` on the child in that
    /// direction, and compose the results. Returns `None` when the
    /// recursive decomposition can't find a clean partition; the caller
    /// then falls back to [`Self::map_split2`].
    fn map_split_forward(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        let fwd = !self.inverted;
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();

        match self.kind {
            Composition::Series => {
                // External in_axes feed into the child that flows first.
                // For a non-inverted CmpMap that's `first`; for an inverted
                // one (data flows second^-1 -> first^-1) it's `second`.
                let (first_in_flow, second_in_flow, dir1, dir2) = if fwd {
                    (self.first.as_ref(), self.second.as_ref(), fwd1, fwd2)
                } else {
                    (self.second.as_ref(), self.first.as_ref(), fwd2, fwd1)
                };

                let (mid_outs, sub1) = split_child(first_in_flow, dir1, in_axes)?;
                let (out_outs, sub2) = split_child(second_in_flow, dir2, &mid_outs)?;
                let composed = CmpMap::new(sub1, sub2, Composition::Series).ok()?;
                Some((out_outs, Box::new(composed)))
            }
            Composition::Parallel => {
                let nin1 = self.effective_first_nin(fwd);
                let nout1 = self.effective_first_nout(fwd);

                let n = in_axes.len();
                let mut sorted_orig: Vec<usize> = (0..n).collect();
                sorted_orig.sort_by_key(|&i| in_axes[i]);
                let do_perm = sorted_orig.iter().enumerate().any(|(k, &i)| k != i);
                let sorted_axes: Vec<usize> = sorted_orig.iter().map(|&i| in_axes[i]).collect();

                let mut first_inputs: Vec<usize> = Vec::new();
                let mut second_inputs: Vec<usize> = Vec::new();
                for &ax in &sorted_axes {
                    if ax < nin1 {
                        first_inputs.push(ax);
                    } else {
                        second_inputs.push(ax - nin1);
                    }
                }

                let mut combined_outs: Vec<usize> = Vec::new();
                let result_map: DynMapping =
                    match (first_inputs.is_empty(), second_inputs.is_empty()) {
                        (true, true) => return None,
                        (false, true) => {
                            let (outs, sub) =
                                split_child(self.first.as_ref(), fwd1, &first_inputs)?;
                            combined_outs.extend(outs);
                            sub
                        }
                        (true, false) => {
                            let (outs, sub) =
                                split_child(self.second.as_ref(), fwd2, &second_inputs)?;
                            combined_outs.extend(outs.iter().map(|&o| o + nout1));
                            sub
                        }
                        (false, false) => {
                            let (outs1, sub1) =
                                split_child(self.first.as_ref(), fwd1, &first_inputs)?;
                            combined_outs.extend(outs1);
                            let (outs2, sub2) =
                                split_child(self.second.as_ref(), fwd2, &second_inputs)?;
                            combined_outs.extend(outs2.iter().map(|&o| o + nout1));
                            Box::new(CmpMap::new(sub1, sub2, Composition::Parallel).ok()?)
                        }
                    };

                let final_map: DynMapping = if do_perm {
                    let outperm: Vec<i64> = sorted_orig.iter().map(|&i| (i as i64) + 1).collect();
                    let mut inperm: Vec<i64> = vec![0; n];
                    for (i, &orig) in sorted_orig.iter().enumerate() {
                        inperm[orig] = (i as i64) + 1;
                    }
                    let pmap = super::PermMap::new(&inperm, &outperm, &[]).ok()?;
                    Box::new(CmpMap::new(Box::new(pmap), result_map, Composition::Series).ok()?)
                } else {
                    result_map
                };

                Some((combined_outs, final_map))
            }
        }
    }

    /// Brute-force inverse-side fallback for [`Mapping::map_split`].
    ///
    /// Mirrors `cmpmap.c::MapSplit2` (~line 2214). Inverts the CmpMap,
    /// then enumerates every ordered tuple of `mout in 1..nout` distinct
    /// output indices. For each tuple, splits the inverted mapping with
    /// the tuple as input axes; if the result has nout matching the
    /// caller's `in_axes` exactly (same axes, same order), the tuple is
    /// the answer.
    ///
    /// Re-entry into `MapSplit2` is suppressed during the recursive call
    /// (mirrors C `MapSplit0(..., reentry=1)` at line 2333) — the inverted
    /// CmpMap's `map_split_forward` is invoked directly to avoid an
    /// infinite mutual recursion. Non-CmpMap mappings encountered during
    /// the recursion don't have a MapSplit2 fallback to avoid.
    ///
    /// O(sum_{m=1..nout-1} P(nout, m)) calls to `map_split` — slow but
    /// correct. Used only when [`Self::map_split_forward`] fails.
    fn map_split2(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        let nout = self.nout();
        if nout < 2 {
            return None;
        }
        let nin_req = in_axes.len();

        let mut inv = self.clone_box();
        inv.invert();

        // Try subset sizes mout in 1..nout (C: `mout < nout`). The full
        // nout-size subset would always be the trivial identity permutation
        // and is handled by the forward path.
        for mout in 1..nout {
            let mut out = vec![0_usize; mout];
            loop {
                // Skip subsets with duplicate indices (C lines 2319-2327).
                let has_dup = (1..mout).any(|i| (0..i).any(|j| out[i] == out[j]));
                if !has_dup {
                    // Recursive split of the inverted mapping. Mirrors
                    // cmpmap.c::MapSplit2 lines 2330-2336: on a CmpMap,
                    // call MapSplit0 with reentry=1 to skip the
                    // MapSplit2 fallback; on a non-CmpMap, dispatch
                    // through the protected astMapSplit (which has no
                    // MapSplit2 path of its own). Both `out` and the
                    // returned indices are 0-based here.
                    let recursive = if let Some(cm) = inv.as_any().downcast_ref::<CmpMap>() {
                        cm.map_split_forward(&out)
                    } else {
                        inv.map_split_zero_based(&out)
                    };

                    if let Some((result2, mut map2)) = recursive {
                        if result2.len() == nin_req
                            && result2.iter().zip(in_axes.iter()).all(|(a, b)| a == b)
                        {
                            map2.invert();
                            return Some((out, map2));
                        }
                    }
                }

                // Increment the subset counter (C lines 2367-2385).
                // out[0] increments first; carry rolls into out[1], etc.
                let mut i = 0;
                out[i] += 1;
                while out[i] == nout {
                    out[i] = 0;
                    i += 1;
                    if i >= mout {
                        break;
                    }
                    out[i] += 1;
                }
                if i >= mout {
                    break;
                }
            }
        }

        None
    }

    // ------------------------------------------------------------------ //
    // Series helpers                                                       //
    // ------------------------------------------------------------------ //

    fn series_transform(&self, pts: &PointSet, fwd: bool) -> AstResult<PointSet> {
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();
        if fwd {
            let mid = self.first.transform(pts, fwd1)?;
            self.second.transform(&mid, fwd2)
        } else {
            let mid = self.second.transform(pts, fwd2)?;
            self.first.transform(&mid, fwd1)
        }
    }

    fn series_transform_into(
        &self,
        pts: &PointSet,
        fwd: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();
        if fwd {
            let mid = self.first.transform(pts, fwd1)?;
            self.second.transform_into(&mid, fwd2, out)
        } else {
            let mid = self.second.transform(pts, fwd2)?;
            self.first.transform_into(&mid, fwd1, out)
        }
    }

    /// Same shape as `series_transform_into` but threads through each child
    /// via `transform_into_nan`, preserving the NaN-as-bad protocol across
    /// the chain. Each stage's `transform_into_nan` is its own opt-in: if a
    /// child hasn't overridden it, the trait default falls through to
    /// `transform_into`, which re-checks BAD locally — correct but skipping
    /// the boundary-amortization win for that one stage.
    fn series_transform_into_nan(
        &self,
        pts: &PointSet,
        fwd: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();
        let mid_n = if fwd {
            self.effective_first_nout(true)
        } else {
            self.effective_second_nin(true)
        };
        let mut mid = PointSet::zeros(pts.npoint(), mid_n);
        if fwd {
            self.first.transform_into_nan(pts, fwd1, &mut mid)?;
            self.second.transform_into_nan(&mid, fwd2, out)
        } else {
            self.second.transform_into_nan(pts, fwd2, &mut mid)?;
            self.first.transform_into_nan(&mid, fwd1, out)
        }
    }

    // ------------------------------------------------------------------ //
    // Parallel helpers                                                     //
    // ------------------------------------------------------------------ //

    fn parallel_transform(&self, pts: &PointSet, fwd: bool) -> AstResult<PointSet> {
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();

        let npoint = pts.npoint();
        let split = self.effective_first_nin(fwd);

        let pts_a = PointSet::from_ndarray(pts.view().slice(ndarray::s![.., ..split]).to_owned());
        let pts_b = PointSet::from_ndarray(pts.view().slice(ndarray::s![.., split..]).to_owned());

        let out_a = self.first.transform(&pts_a, fwd1)?;
        let out_b = self.second.transform(&pts_b, fwd2)?;

        let ncols_a = out_a.ncoord();
        let ncols_b = out_b.ncoord();
        let mut combined = PointSet::zeros(npoint, ncols_a + ncols_b);
        {
            let mut view = combined.view_mut();
            view.slice_mut(ndarray::s![.., ..ncols_a])
                .assign(&out_a.view());
            view.slice_mut(ndarray::s![.., ncols_a..])
                .assign(&out_b.view());
        }
        Ok(combined)
    }

    /// Parallel composition under the NaN-as-bad protocol. The two children
    /// see disjoint axis slices of the input; we route each slice through
    /// the child's `transform_into_nan` so the protocol is preserved across
    /// the parallel branch.
    fn parallel_transform_into_nan(
        &self,
        pts: &PointSet,
        fwd: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd1 = fwd != self.first_invert_changed();
        let fwd2 = fwd != self.second_invert_changed();

        let npoint = pts.npoint();
        let split = self.effective_first_nin(fwd);

        let pts_a = PointSet::from_ndarray(pts.view().slice(ndarray::s![.., ..split]).to_owned());
        let pts_b = PointSet::from_ndarray(pts.view().slice(ndarray::s![.., split..]).to_owned());

        let nout_a = if fwd1 {
            self.first.nout()
        } else {
            self.first.nin()
        };
        let nout_b = if fwd2 {
            self.second.nout()
        } else {
            self.second.nin()
        };
        let mut out_a = PointSet::zeros(npoint, nout_a);
        let mut out_b = PointSet::zeros(npoint, nout_b);
        self.first.transform_into_nan(&pts_a, fwd1, &mut out_a)?;
        self.second.transform_into_nan(&pts_b, fwd2, &mut out_b)?;

        let mut view = out.view_mut();
        view.slice_mut(ndarray::s![.., ..nout_a])
            .assign(&out_a.view());
        view.slice_mut(ndarray::s![.., nout_a..])
            .assign(&out_b.view());
        Ok(())
    }

    fn parallel_transform_into(
        &self,
        pts: &PointSet,
        fwd: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let result = self.parallel_transform(pts, fwd)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }
}

// Manual Debug impl because DynMapping (Box<dyn Mapping>) is not Debug.
impl fmt::Debug for CmpMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CmpMap")
            .field("kind", &self.kind)
            .field("nin", &self.nin())
            .field("nout", &self.nout())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for CmpMap {
    fn clone(&self) -> Self {
        CmpMap {
            first: self.first.clone(),
            second: self.second.clone(),
            kind: self.kind,
            inverted: self.inverted,
            has_been_simplified: self.has_been_simplified,
            tran_forward: self.tran_forward,
            tran_inverse: self.tran_inverse,
            inv_a: self.inv_a,
            inv_b: self.inv_b,
        }
    }
}

impl Mapping for CmpMap {
    fn nin(&self) -> usize {
        // The "forward nin" of the CmpMap (before considering self.inverted)
        // is determined by the children in the CmpMap's nominal forward direction.
        let fwd_nin = match self.kind {
            Composition::Series => self.effective_first_nin(true),
            Composition::Parallel => {
                self.effective_first_nin(true) + self.effective_second_nin(true)
            }
        };
        let fwd_nout = match self.kind {
            Composition::Series => self.effective_second_nout(true),
            Composition::Parallel => {
                self.effective_first_nout(true) + self.effective_second_nout(true)
            }
        };
        if self.inverted { fwd_nout } else { fwd_nin }
    }

    fn nout(&self) -> usize {
        let fwd_nin = match self.kind {
            Composition::Series => self.effective_first_nin(true),
            Composition::Parallel => {
                self.effective_first_nin(true) + self.effective_second_nin(true)
            }
        };
        let fwd_nout = match self.kind {
            Composition::Series => self.effective_second_nout(true),
            Composition::Parallel => {
                self.effective_first_nout(true) + self.effective_second_nout(true)
            }
        };
        if self.inverted { fwd_nin } else { fwd_nout }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
        // Inversion invalidates the "already simplified" flag (C astSetInvert clears IsSimple).
        self.has_been_simplified = false;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        match self.kind {
            Composition::Series => self.series_transform(pts, fwd),
            Composition::Parallel => self.parallel_transform(pts, fwd),
        }
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        match self.kind {
            Composition::Series => self.series_transform_into(pts, fwd, out),
            Composition::Parallel => self.parallel_transform_into(pts, fwd, out),
        }
    }

    /// NaN-as-bad threading. Recursively calls each child's
    /// `transform_into_nan`, allocating only intermediate buffers (no
    /// per-stage BAD scan).
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        match self.kind {
            Composition::Series => self.series_transform_into_nan(pts, fwd, out),
            Composition::Parallel => self.parallel_transform_into_nan(pts, fwd, out),
        }
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.has_been_simplified {
            return Ok(Box::new(self.clone()));
        }
        crate::mappings::simplify::simplify_mapping(self)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        // C `cmpmap.c` defines a chain-rule `Rate` override (sum of
        // component-rate products for series, owning-component dispatch with
        // exact 0.0 across-component for parallel) but leaves it *unregistered*
        // in the vtable: `astInitCmpMapVtab` comments out `mapping->Rate = Rate`
        // (cmpmap.c:892) with the note that the implementation "can be immensely
        // slow for complex Mappings ... until such time as I have time to sort
        // it out". So every `astRate` on a CmpMap in C falls through to the base
        // `Mapping::Rate`, which numerically differentiates the whole
        // composition. We mirror that exactly by using the generic numeric
        // estimator here; a faithful port of the override would *diverge* from
        // the C oracle (it was measured to shift the FELO/VOPT-F2W spectral
        // CDELT3 by ~1e-7, enough to flip a FITS comment), so the override is
        // deliberately not installed.
        //
        // TODO(revisit): the analytic chain rule could be both faster and more
        // accurate than whole-map numeric differentiation. If we ever decide to
        // diverge from C here, the override implementation is straightforward
        // (chain-rule series, dispatch parallel) — but it must be gated so the
        // oracle/fixture comparisons still pass, and benchmarked to confirm it
        // is not the "immensely slow" path the C author warned about.
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_cmp) = other.as_any().downcast_ref::<CmpMap>() else {
            return false;
        };
        if self.kind != other_cmp.kind {
            return false;
        }
        let series = self.kind == Composition::Series;
        // Flatten both into canonical component lists, accounting for
        // tree structure differences like (A;(B;C)) vs ((A;B);C).
        let Ok(list_self) = super::simplify::flatten(self, false, series) else {
            return false;
        };
        let Ok(list_other) = super::simplify::flatten(other_cmp, false, series) else {
            return false;
        };
        if list_self.len() != list_other.len() {
            return false;
        }
        list_self
            .iter()
            .zip(list_other.iter())
            .all(|((m1, inv1, _), (m2, inv2, _))| inv1 == inv2 && m1.equal(m2.as_ref()))
    }

    fn map_split_zero_based(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors cmpmap.c::MapSplit (~line 2788) -> MapSplit0 (~line 2405),
        // including the MapSplit2 brute-force inverse fallback (~line 2214)
        // when the forward decomposition fails.
        crate::mappings::helpers::validate_in_axes(in_axes, self.nin())?;
        if let Some(r) = self.map_split_forward(in_axes) {
            return Some(r);
        }
        self.map_split2(in_axes)
    }

    fn has_been_simplified(&self) -> bool {
        self.has_been_simplified
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.has_been_simplified = flag;
    }

    fn has_forward(&self) -> bool {
        if self.inverted {
            self.tran_inverse
        } else {
            self.tran_forward
        }
    }

    fn has_inverse(&self) -> bool {
        if self.inverted {
            self.tran_forward
        } else {
            self.tran_inverse
        }
    }

    fn dump_tran_flags(&self) -> (bool, bool) {
        (self.tran_forward, self.tran_inverse)
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl Serializable for CmpMap {
    fn class_name(&self) -> &'static str {
        "CmpMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Compound Mapping".to_string());
        write_mapping_base(&mut node, self);

        // Series is the default (1); only write for parallel (0).
        if self.kind == Composition::Parallel {
            node.push_int(
                "Series",
                0,
                Some("Component Mappings applied in parallel".to_string()),
            );
        }

        // Component invert flags (only if non-zero).
        if self.inv_a {
            node.push_int(
                "InvA",
                1,
                Some("First Mapping used in inverse direction".to_string()),
            );
        }
        if self.inv_b {
            node.push_int(
                "InvB",
                1,
                Some("Second Mapping used in inverse direction".to_string()),
            );
        }

        // Component mappings as nested objects.
        let node_a = self.first.to_node()?;
        let node_b = self.second.to_node()?;
        node.push_object("MapA", node_a, Some("First component Mapping".to_string()));
        node.push_object("MapB", node_b, Some("Second component Mapping".to_string()));

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);

        let is_series = node.get_int("Series", 1) != 0;
        let kind = if is_series {
            Composition::Series
        } else {
            Composition::Parallel
        };

        let inv_a = node.get_int("InvA", 0) != 0;
        let inv_b = node.get_int("InvB", 0) != 0;

        let node_a = node
            .get_object("MapA")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpMap: missing MapA"))?;
        let node_b = node
            .get_object("MapB")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpMap: missing MapB"))?;

        let map_a = mapping_from_node(node_a)?;
        let map_b = mapping_from_node(node_b)?;

        // Children keep their native Invert state as serialized.
        // inv_a/inv_b record the expected child state; the effective
        // transform direction is computed at runtime via
        // first_invert_changed() / second_invert_changed().
        let mut cmp = Self {
            first: map_a,
            second: map_b,
            kind,
            inverted: false,
            has_been_simplified: false,
            tran_forward: true,
            tran_inverse: true,
            inv_a,
            inv_b,
        };
        cmp.tran_forward = base.has_forward;
        cmp.tran_inverse = base.has_inverse;
        if base.inverted {
            cmp.invert();
        }

        cmp.set_has_been_simplified(base.has_been_simplified);
        Ok(cmp)
    }
}

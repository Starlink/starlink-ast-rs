//! Core simplification engine for mapping composition trees.
//!
//! This module implements the flatten/merge/reconstruct pipeline that
//! decomposes `CmpMap` trees into flat lists, attempts pairwise merges
//! of adjacent mappings, and reconstructs the result.
//!
//! The pipeline is:
//! 1. **Flatten**: recursively decompose a `CmpMap` tree into a flat
//!    list of `(DynMapping, bool)` pairs, where the bool indicates
//!    whether the mapping is effectively inverted in the composition.
//! 2. **Merge loop**: walk adjacent pairs, attempting `try_merge` in
//!    both directions until no further improvements are found.
//! 3. **Reconstruct**: build a new `CmpMap` tree (or single mapping)
//!    from the flat list.

use crate::error::AstResult;

use super::cmpmap::{CmpMap, Composition};
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// Recursively decompose a mapping into a flat list of leaf mappings.
///
/// Each element is `(mapping, effective_inv)` where:
/// - `mapping` is a cloned leaf mapping in its forward (non-inverted)
///   state — `is_inverted()` returns `false`.
/// - `effective_inv` is `true` if this mapping should be applied
///   inverted within the composition, `false` for forward.
///
/// `CmpMap` nodes whose composition matches `series` are absorbed
/// (their children are recursively flattened). Other mappings
/// (including `CmpMap` nodes of the other composition kind) are
/// treated as opaque leaves.
///
/// `inv` is the externally imposed inversion flag from a parent
/// `CmpMap` — `false` for the top-level call.
pub(crate) fn flatten(
    mapping: &dyn Mapping,
    inv: bool,
    series: bool,
) -> AstResult<Vec<(DynMapping, bool)>> {
    let target_kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };

    // Try to decompose if this is a CmpMap of the matching kind.
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == target_kind {
            // Combine external inversion with the CmpMap's own flag.
            // XOR: if both are inverted they cancel.
            let cmp_inv = inv != cmp.is_inverted();

            if !cmp_inv {
                // Forward: process first then second, children not
                // additionally inverted.
                let mut result = flatten(cmp.first(), false, series)?;
                result.extend(flatten(cmp.second(), false, series)?);
                return Ok(result);
            } else if series {
                // Inverted series: reverse child order, both inverted.
                let mut result = flatten(cmp.second(), true, series)?;
                result.extend(flatten(cmp.first(), true, series)?);
                return Ok(result);
            } else {
                // Inverted parallel: same order, both inverted.
                let mut result = flatten(cmp.first(), true, series)?;
                result.extend(flatten(cmp.second(), true, series)?);
                return Ok(result);
            }
        }
    }

    // Leaf: clone via simplified(), then normalise the invert flag.
    let mut cloned = mapping.simplified()?;
    let effective_inv = inv != cloned.is_inverted();
    // Reset the clone to forward state; the bool carries inversion.
    if cloned.is_inverted() {
        cloned.invert();
    }
    Ok(vec![(cloned, effective_inv)])
}

/// Walk adjacent pairs and attempt pairwise merges until no progress.
///
/// For each adjacent pair `(list[i], list[i+1])`, tries:
/// 1. `list[i].try_merge(&list[i+1], series, inv_i, inv_{i+1})`
/// 2. `list[i+1].try_merge(&list[i], series, inv_{i+1}, inv_i)`
///
/// On success the pair is replaced by the merged result (with
/// `effective_inv = false`, since the merged mapping is freshly
/// constructed in its forward state).
///
/// After the merge loop stabilises, `UnitMap` entries are removed
/// from series lists (they are identity and do not contribute).
pub(crate) fn merge_loop(list: &mut Vec<(DynMapping, bool)>, series: bool) {
    loop {
        let mut improved = false;
        let mut i = 0;

        while i + 1 < list.len() {
            let merged = {
                let (ref m1, inv1) = list[i];
                let (ref m2, inv2) = list[i + 1];

                // Try left merging into right.
                m1.try_merge(m2.as_ref(), series, inv1, inv2)
                    // Try right merging into left (swap roles).
                    .or_else(|| m2.try_merge(m1.as_ref(), series, inv2, inv1))
            };

            if let Some(m) = merged {
                list[i] = (m, false);
                list.remove(i + 1);
                improved = true;
                // Don't advance i — the new element might merge with
                // the next neighbour.
            } else {
                i += 1;
            }
        }

        if !improved {
            break;
        }
    }

    // Post-loop: strip UnitMaps from series lists.
    if series && list.len() > 1 {
        list.retain(|(m, _)| !m.as_any().is::<UnitMap>());
        // If everything was a UnitMap the list is now empty — put one
        // back so reconstruct has something to work with. The ncoord
        // is taken from any of the original UnitMaps, but since they
        // were all compatible in series they share the same ncoord.
        // We have already lost the ncoord info here, so use 0 as a
        // sentinel; the caller (simplify_mapping) handles the
        // all-UnitMap case before we get here via the existing
        // CmpMap::simplified() path.
        if list.is_empty() {
            // Shouldn't normally happen: CmpMap::simplified() already
            // collapses UnitMap pairs. If it does, push a 1-D UnitMap
            // as a safe fallback.
            list.push((Box::new(UnitMap::new(1)), false));
        }
    }
}

/// Reconstruct a `CmpMap` tree (or single mapping) from a flat list.
///
/// Each mapping's `is_inverted` flag is set from the stored
/// `effective_inv` before composition.
///
/// For a single-element list the mapping is returned directly (no
/// wrapping `CmpMap`).
pub(crate) fn reconstruct(list: Vec<(DynMapping, bool)>, series: bool) -> AstResult<DynMapping> {
    let kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };

    let mut iter = list.into_iter();
    let (mut acc, acc_inv) = iter.next().expect("reconstruct called with empty list");

    // Apply the stored inversion flag.
    if acc_inv {
        acc.invert();
    }

    for (mut m, m_inv) in iter {
        if m_inv {
            m.invert();
        }
        acc = Box::new(CmpMap::new(acc, m, kind)?);
    }

    Ok(acc)
}

/// Simplify a mapping using the flatten/merge/reconstruct pipeline.
///
/// For `CmpMap` inputs this decomposes the tree, attempts pairwise
/// merges, and rebuilds. For non-`CmpMap` inputs it delegates to the
/// mapping's own `simplified()` method.
pub(crate) fn simplify_mapping(mapping: &dyn Mapping) -> AstResult<DynMapping> {
    let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() else {
        return mapping.simplified();
    };

    let series = cmp.composition() == Composition::Series;

    let mut list = flatten(mapping, false, series)?;
    merge_loop(&mut list, series);
    reconstruct(list, series)
}

// ------------------------------------------------------------------ //
// Tests                                                               //
// ------------------------------------------------------------------ //

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::{ShiftMap, ZoomMap};

    // Helper: box a mapping as DynMapping.
    fn dyn_map(m: impl Mapping + 'static) -> DynMapping {
        Box::new(m)
    }

    // ------------------------------------------------------------ //
    // flatten                                                       //
    // ------------------------------------------------------------ //

    #[test]
    fn flatten_non_cmpmap_returns_single_element() {
        let z = ZoomMap::new(2, 3.0).unwrap();
        let list = flatten(&z, false, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].1); // not inverted
        assert_eq!(list[0].0.nin(), 2);
        assert_eq!(list[0].0.nout(), 2);
    }

    #[test]
    fn flatten_non_cmpmap_with_inv_flag() {
        let z = ZoomMap::new(2, 3.0).unwrap();
        let list = flatten(&z, true, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].1); // effectively inverted
        assert!(!list[0].0.is_inverted()); // mapping itself is normalised
    }

    #[test]
    fn flatten_non_cmpmap_with_self_inverted() {
        let mut z = ZoomMap::new(1, 5.0).unwrap();
        z.invert();
        assert!(z.is_inverted());
        // External inv=false, self.is_inverted()=true → effective_inv=true
        let list = flatten(&z, false, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].1); // effectively inverted
        assert!(!list[0].0.is_inverted()); // normalised
    }

    #[test]
    fn flatten_non_cmpmap_double_inversion_cancels() {
        let mut z = ZoomMap::new(1, 5.0).unwrap();
        z.invert();
        // External inv=true, self.is_inverted()=true → effective_inv=false
        let list = flatten(&z, true, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].1); // double inversion cancels
    }

    #[test]
    fn flatten_series_cmpmap_two_elements() {
        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let list = flatten(&cmp, false, true).unwrap();
        assert_eq!(list.len(), 2);
        assert!(!list[0].1);
        assert!(!list[1].1);
    }

    #[test]
    fn flatten_nested_series_three_elements() {
        // CmpMap(CmpMap(A, B, Series), C, Series) → [A, B, C]
        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let inner = CmpMap::new(a, b, Composition::Series).unwrap();
        let c = dyn_map(ZoomMap::new(1, 5.0).unwrap());
        let outer = CmpMap::new(dyn_map(inner), c, Composition::Series).unwrap();
        let list = flatten(&outer, false, true).unwrap();
        assert_eq!(list.len(), 3);
        // All forward
        assert!(!list[0].1);
        assert!(!list[1].1);
        assert!(!list[2].1);
    }

    #[test]
    fn flatten_parallel_cmpmap_not_absorbed_by_series() {
        // A parallel CmpMap is opaque when flattening for series.
        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let par = CmpMap::new(a, b, Composition::Parallel).unwrap();
        let list = flatten(&par, false, true).unwrap(); // flatten for series
        assert_eq!(list.len(), 1); // parallel treated as leaf
    }

    #[test]
    fn flatten_inverted_series_reverses_order() {
        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0, 20.0]));
        let mut cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        cmp.invert();
        let list = flatten(&cmp, false, true).unwrap();
        assert_eq!(list.len(), 2);
        // Inverted series: order reversed, both inverted.
        // The shift should come first (was second, now reversed).
        assert!(list[0].1); // inverted
        assert!(list[1].1); // inverted
        // First element should be the ShiftMap (originally second).
        assert!(list[0].0.as_any().is::<ShiftMap>());
        assert!(list[1].0.as_any().is::<ZoomMap>());
    }

    #[test]
    fn flatten_parallel_cmpmap_for_parallel() {
        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let cmp = CmpMap::new(a, b, Composition::Parallel).unwrap();
        // Flatten for parallel: absorbs the parallel CmpMap.
        let list = flatten(&cmp, false, false).unwrap();
        assert_eq!(list.len(), 2);
    }

    // ------------------------------------------------------------ //
    // merge_loop                                                    //
    // ------------------------------------------------------------ //

    #[test]
    fn merge_loop_no_merges_with_incompatible_types() {
        use crate::mappings::SphMap;

        // Types that have no cross-type merge rule should not merge.
        let a = dyn_map(SphMap::new());
        let b = dyn_map(ShiftMap::new([10.0, 20.0]));
        let mut list: Vec<(DynMapping, bool)> = vec![(a, false), (b, false)];
        merge_loop(&mut list, true);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn merge_loop_strips_unitmaps_from_series() {
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let u = dyn_map(UnitMap::new(1));
        let b = dyn_map(ShiftMap::new([5.0]));
        let mut list: Vec<(DynMapping, bool)> = vec![(a, false), (u, false), (b, false)];
        merge_loop(&mut list, true);
        // UnitMap should be stripped.
        assert_eq!(list.len(), 2);
        assert!(!list[0].0.as_any().is::<UnitMap>());
        assert!(!list[1].0.as_any().is::<UnitMap>());
    }

    #[test]
    fn merge_loop_does_not_strip_unitmaps_from_parallel() {
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let u = dyn_map(UnitMap::new(1));
        let mut list: Vec<(DynMapping, bool)> = vec![(a, false), (u, false)];
        merge_loop(&mut list, false);
        // Parallel: UnitMaps are NOT stripped (they contribute dimensions).
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn merge_loop_all_unitmaps_preserves_one() {
        let u1 = dyn_map(UnitMap::new(2));
        let u2 = dyn_map(UnitMap::new(2));
        let mut list: Vec<(DynMapping, bool)> = vec![(u1, false), (u2, false)];
        merge_loop(&mut list, true);
        // All UnitMaps stripped → fallback pushes one back.
        assert_eq!(list.len(), 1);
    }

    // ------------------------------------------------------------ //
    // reconstruct                                                   //
    // ------------------------------------------------------------ //

    #[test]
    fn reconstruct_single_element() {
        let z = dyn_map(ZoomMap::new(1, 4.0).unwrap());
        let result = reconstruct(vec![(z, false)], true).unwrap();
        assert_eq!(result.nin(), 1);
        assert!(!result.is_inverted());
    }

    #[test]
    fn reconstruct_single_element_inverted() {
        let z = dyn_map(ZoomMap::new(1, 4.0).unwrap());
        let result = reconstruct(vec![(z, true)], true).unwrap();
        assert!(result.is_inverted());
    }

    #[test]
    fn reconstruct_two_elements_series() {
        use crate::coords::PointSet;

        let z = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let s = dyn_map(ShiftMap::new([10.0]));
        let result = reconstruct(vec![(z, false), (s, false)], true).unwrap();
        // zoom(2) then shift(10): 5 → 10 → 20
        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 20.0).abs() < 1e-12);
    }

    #[test]
    fn reconstruct_three_elements_series() {
        use crate::coords::PointSet;

        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let c = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let result = reconstruct(vec![(a, false), (b, false), (c, false)], true).unwrap();
        // 5 → 10 → 20 → 60
        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 60.0).abs() < 1e-12);
    }

    #[test]
    fn reconstruct_two_elements_parallel() {
        use crate::coords::PointSet;

        let z = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let s = dyn_map(ShiftMap::new([100.0]));
        let result = reconstruct(vec![(z, false), (s, false)], false).unwrap();
        assert_eq!(result.nin(), 2);
        let pts = PointSet::from_rows([[2.0, 5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 105.0).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // simplify_mapping                                              //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_mapping_non_cmpmap_delegates() {
        let z = ZoomMap::new(1, 1.0).unwrap();
        // ZoomMap with factor 1.0 simplifies to UnitMap.
        let result = simplify_mapping(&z).unwrap();
        assert!(result.as_any().is::<UnitMap>());
    }

    #[test]
    fn simplify_mapping_cmpmap_roundtrip() {
        use crate::coords::PointSet;

        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // Must be functionally equivalent: 5 → 20.
        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 20.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_mapping_inverted_cmpmap() {
        use crate::coords::PointSet;

        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let mut cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        cmp.invert();
        // Inverted: inverse of (zoom(2) then shift(10)) = inv_shift(10) then inv_zoom(2)
        // = (x - 10) / 2
        // Apply forward to the simplified result (which encodes the inversion).
        let result = simplify_mapping(&cmp).unwrap();
        let pts = PointSet::from_rows([[20.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 5.0).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // try_merge: UnitMap removal                                    //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_unitmap_series_zoom_removes_unit() {
        // CmpMap(UnitMap(2), ZoomMap(2,3), Series) → ZoomMap(2,3)
        let a = dyn_map(UnitMap::new(2));
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<ZoomMap>());
        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);
    }

    // ------------------------------------------------------------ //
    // try_merge: ZoomMap merging                                    //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_zoom_series_merges_factors() {
        // CmpMap(ZoomMap(2,2), ZoomMap(2,3), Series) → ZoomMap(2,6)
        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<ZoomMap>());
        assert_eq!(result.nin(), 2);
        // Verify the merged factor via transform.
        let pts = crate::coords::PointSet::from_rows([[1.0, 2.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 12.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_zoom_series_inverse_becomes_unit() {
        // CmpMap(ZoomMap(2,2), ZoomMap(2,0.5), Series) → UnitMap(2)
        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ZoomMap::new(2, 0.5).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    // ------------------------------------------------------------ //
    // try_merge: ShiftMap merging                                   //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_shift_series_merges_offsets() {
        // CmpMap(ShiftMap([1,2]), ShiftMap([3,4]), Series) → ShiftMap([4,6])
        let a = dyn_map(ShiftMap::new(vec![1.0, 2.0]));
        let b = dyn_map(ShiftMap::new(vec![3.0, 4.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<ShiftMap>());
        let pts = crate::coords::PointSet::from_rows([[0.0, 0.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 4.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 6.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_shift_series_cancels_to_unit() {
        // CmpMap(ShiftMap([1,2]), ShiftMap([-1,-2]), Series) → UnitMap(2)
        let a = dyn_map(ShiftMap::new(vec![1.0, 2.0]));
        let b = dyn_map(ShiftMap::new(vec![-1.0, -2.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    // ------------------------------------------------------------ //
    // Transform equivalence: before/after simplification            //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_preserves_transform_zoom_series() {
        use crate::coords::PointSet;

        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[1.0, -2.0], [0.5, 100.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            for j in 0..2 {
                assert!(
                    (before.point(i)[j] - after.point(i)[j]).abs() < 1e-12,
                    "mismatch at point {} coord {}",
                    i,
                    j,
                );
            }
        }
    }

    #[test]
    fn simplify_preserves_transform_shift_series() {
        use crate::coords::PointSet;

        let a = dyn_map(ShiftMap::new(vec![10.0, 20.0]));
        let b = dyn_map(ShiftMap::new(vec![30.0, 40.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[1.0, 2.0], [5.0, -3.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            for j in 0..2 {
                assert!(
                    (before.point(i)[j] - after.point(i)[j]).abs() < 1e-12,
                    "mismatch at point {} coord {}",
                    i,
                    j,
                );
            }
        }
    }

    #[test]
    fn simplify_preserves_transform_inverse() {
        use crate::coords::PointSet;

        // Test that inverse transforms also agree.
        let a = dyn_map(ZoomMap::new(1, 4.0).unwrap());
        let b = dyn_map(ZoomMap::new(1, 5.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[100.0]]);
        let before = cmp.transform(&pts, false).unwrap();
        let after = simplified.transform(&pts, false).unwrap();
        assert!((before.point(0)[0] - after.point(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // try_merge: UnitMap parallel merge                             //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_unitmap_parallel_merges() {
        // CmpMap(UnitMap(2), UnitMap(3), Parallel) → UnitMap(5)
        let a = dyn_map(UnitMap::new(2));
        let b = dyn_map(UnitMap::new(3));
        let cmp = CmpMap::new(a, b, Composition::Parallel).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 5);
    }

    // ------------------------------------------------------------ //
    // try_merge: WinMap merging                                     //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_winmap_series_composes() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // WinMap(s=1,c=2) then WinMap(s=10,c=3)
        // Compose: out = 10 + 3*(1 + 2*x) = 13 + 6*x
        let a = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let b = dyn_map(WinMap::new(&[10.0], &[3.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 43.0).abs() < 1e-12); // 13 + 6*5 = 43
    }

    #[test]
    fn simplify_winmap_inverse_cancels_to_unit() {
        use crate::mappings::WinMap;

        // WinMap(s=3,c=2) then WinMap(s=3,c=2)^-1 should cancel.
        let a = dyn_map(WinMap::new(&[3.0], &[2.0]).unwrap());
        let mut b = WinMap::new(&[3.0], &[2.0]).unwrap();
        b.invert();
        let cmp = CmpMap::new(a, dyn_map(b), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 1);
    }

    #[test]
    fn simplify_winmap_preserves_transform() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        let a = dyn_map(WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap());
        let b = dyn_map(WinMap::new(&[5.0, 6.0], &[7.0, 8.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[1.0, -2.0], [0.5, 100.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            for j in 0..2 {
                assert!(
                    (before.point(i)[j] - after.point(i)[j]).abs() < 1e-10,
                    "mismatch at point {} coord {}",
                    i,
                    j,
                );
            }
        }
    }

    // ------------------------------------------------------------ //
    // try_merge: MatrixMap merging                                  //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_matrix_rot90_inverse_cancels() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // 90-degree rotation then its inverse = identity.
        let rot90 = array![[0.0_f64, -1.0], [1.0, 0.0]];
        let rot90_inv = array![[0.0_f64, 1.0], [-1.0, 0.0]];
        let a = dyn_map(MatrixMap::new(rot90).unwrap());
        let b = dyn_map(MatrixMap::new(rot90_inv).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_matrix_series_multiplies() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // Non-diagonal matrices that truly require MatrixMap after merging.
        let m1 = array![[1.0_f64, 1.0], [0.0, 1.0]]; // shear
        let m2 = array![[2.0_f64, 0.0], [0.0, 1.0]]; // non-uniform scale
        let a = dyn_map(MatrixMap::new(m1).unwrap());
        let b = dyn_map(MatrixMap::new(m2).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();
        assert!(simplified.as_any().is::<MatrixMap>());

        // m2 * m1 = [[2,2],[0,1]]; apply to (1,2): (2*1+2*2, 0*1+1*2) = (6, 2)
        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let out = simplified.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 2.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_matrix_preserves_transform() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        let m1 = array![[1.0_f64, 2.0], [3.0, 4.0]];
        let m2 = array![[5.0_f64, 6.0], [7.0, 8.0]];
        let a = dyn_map(MatrixMap::new(m1).unwrap());
        let b = dyn_map(MatrixMap::new(m2).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[1.0, -1.0], [3.0, 7.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            for j in 0..2 {
                assert!(
                    (before.point(i)[j] - after.point(i)[j]).abs() < 1e-10,
                    "mismatch at point {} coord {}",
                    i,
                    j,
                );
            }
        }
    }

    #[test]
    fn simplify_matrix_with_inversion_flag() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // MatrixMap then MatrixMap^-1 (via .invert()) should cancel.
        let m = array![[1.0_f64, 2.0], [3.0, 4.0]];
        let a = dyn_map(MatrixMap::new(m.clone()).unwrap());
        let mut b = MatrixMap::new(m).unwrap();
        b.invert();
        let cmp = CmpMap::new(a, dyn_map(b), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    // ------------------------------------------------------------ //
    // try_merge: SphMap inverse cancellation                        //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_sphmap_fwd_inv_cancels() {
        use crate::mappings::SphMap;

        // SphMap (fwd 3->2) then SphMap^-1 (inv 2->3) with unit_radius.
        let mut fwd = SphMap::new();
        fwd.set_unit_radius(true);
        let mut inv = SphMap::new();
        inv.set_unit_radius(true);
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 3);
    }

    #[test]
    fn simplify_sphmap_inv_fwd_cancels() {
        use crate::mappings::SphMap;

        // SphMap^-1 (inv 2->3) then SphMap (fwd 3->2) with default attrs.
        let mut inv = SphMap::new();
        inv.invert();
        let fwd = SphMap::new();
        let cmp = CmpMap::new(dyn_map(inv), dyn_map(fwd), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_sphmap_mismatched_attrs_no_merge() {
        use crate::mappings::SphMap;

        // Different unit_radius: should NOT cancel.
        let mut fwd = SphMap::new();
        fwd.set_unit_radius(true);
        let mut inv = SphMap::new();
        // inv keeps unit_radius=false (default)
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // Should NOT be a UnitMap.
        assert!(!result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // try_merge: WcsMap inverse cancellation                        //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_wcsmap_inverse_cancels() {
        use crate::mappings::{WcsMap, WcsProjection};

        let fwd = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let mut inv = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_wcsmap_different_proj_no_merge() {
        use crate::mappings::{WcsMap, WcsProjection};

        // Different projections should NOT cancel.
        let fwd = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let mut inv = WcsMap::new(2, WcsProjection::Sin, 1, 2).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(!result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // try_merge: SlaMap inverse cancellation                        //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_slamap_inverse_cancels() {
        use crate::mappings::{SlaCvtType, SlaMap};

        let mut fwd = SlaMap::new();
        fwd.add(SlaCvtType::Eqgal, &[]);
        let mut inv = SlaMap::new();
        inv.add(SlaCvtType::Eqgal, &[]);
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_slamap_different_steps_no_merge() {
        use crate::mappings::{SlaCvtType, SlaMap};

        // Different conversions should NOT cancel.
        let mut fwd = SlaMap::new();
        fwd.add(SlaCvtType::Eqgal, &[]);
        let mut inv = SlaMap::new();
        inv.add(SlaCvtType::Galeq, &[]);
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(!result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // try_merge: PolyMap inverse cancellation                       //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_polymap_inverse_cancels() {
        use crate::mappings::{PolyMap, PolyTerm};

        // Simple linear polynomial: y = 2*x
        let terms = vec![PolyTerm {
            coeff: 2.0,
            powers: vec![1],
            out_axis: 1,
        }];
        let fwd = PolyMap::new(1, 1, &terms, None).unwrap();
        let mut inv = PolyMap::new(1, 1, &terms, None).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 1);
    }

    // ------------------------------------------------------------ //
    // try_merge: PermMap inverse cancellation                       //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_permmap_inverse_cancels() {
        use crate::mappings::PermMap;

        // Swap axes: outperm=[2,1], inperm=[2,1].
        let fwd = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        let mut inv = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_permmap_different_perms_no_merge() {
        use crate::mappings::PermMap;

        let fwd = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        let mut inv = PermMap::new(&[1, 2], &[1, 2], &[]).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // Different perms: should NOT simplify to UnitMap.
        assert!(!result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: WinMap absorbs ZoomMap              //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_winmap_absorbs_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // WinMap(s=1,c=2) then ZoomMap(1,3): out = 3*(2*x + 1) = 6*x + 3
        let a = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let b = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        // 6*5 + 3 = 33
        assert!((out.point(0)[0] - 33.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_zoommap_then_winmap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // ZoomMap(1,3) then WinMap(s=1,c=2): out = 2*(3*x) + 1 = 6*x + 1
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let b = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 31.0).abs() < 1e-12); // 6*5 + 1

        // Also verify transform equivalence.
        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: WinMap absorbs ShiftMap             //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_winmap_absorbs_shiftmap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // WinMap(s=1,c=2) then ShiftMap([10]): out = 1*(2*x+1) + 10 = 2*x + 11
        let a = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let b = dyn_map(ShiftMap::new(vec![10.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 21.0).abs() < 1e-12); // 2*5 + 11
    }

    #[test]
    fn simplify_shiftmap_then_winmap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // ShiftMap([10]) then WinMap(s=1,c=2): out = 2*(x+10) + 1 = 2*x + 21
        let a = dyn_map(ShiftMap::new(vec![10.0]));
        let b = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 31.0).abs() < 1e-12); // 2*5 + 21

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: ZoomMap + ShiftMap                  //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_zoommap_then_shiftmap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // ZoomMap(1,3) then ShiftMap([10]): out = 3*x + 10
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let b = dyn_map(ShiftMap::new(vec![10.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // Should produce a WinMap (shift + scale).
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 25.0).abs() < 1e-12); // 3*5 + 10

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
    }

    #[test]
    fn simplify_shiftmap_then_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // ShiftMap([10]) then ZoomMap(1,3): out = 3*(x + 10) = 3*x + 30
        let a = dyn_map(ShiftMap::new(vec![10.0]));
        let b = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 45.0).abs() < 1e-12); // 3*5 + 30

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: with inversion                     //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_winmap_then_inv_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // WinMap(s=6,c=3) then ZoomMap(1,3)^-1: out = (1/3)*(3*x + 6) = x + 2
        let a = dyn_map(WinMap::new(&[6.0], &[3.0]).unwrap());
        let mut z = ZoomMap::new(1, 3.0).unwrap();
        z.invert();
        let cmp = CmpMap::new(a, dyn_map(z), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 7.0).abs() < 1e-12); // 5 + 2

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
    }

    #[test]
    fn simplify_inv_shiftmap_then_zoommap() {
        use crate::coords::PointSet;

        // ShiftMap([10])^-1 then ZoomMap(1,2): out = 2*(x - 10) = 2*x - 20
        let mut s = ShiftMap::new(vec![10.0]);
        s.invert();
        let z = ZoomMap::new(1, 2.0).unwrap();
        let cmp = CmpMap::new(dyn_map(s), dyn_map(z), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[15.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 10.0).abs() < 1e-12); // 2*15 - 20

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: multi-axis                         //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_cross_type_multi_axis() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // 2-axis: ShiftMap([1,2]) then ZoomMap(2,3) → WinMap with scales=[3,3], shifts=[3,6]
        let a = dyn_map(ShiftMap::new(vec![1.0, 2.0]));
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        // out = 3*(x + shift) = [3*(x1+1), 3*(x2+2)] = [3*x1+3, 3*x2+6]
        let pts = PointSet::from_rows([[10.0, 20.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 33.0).abs() < 1e-12); // 3*10 + 3
        assert!((out.point(0)[1] - 66.0).abs() < 1e-12); // 3*20 + 6

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
        assert!((before.point(0)[1] - out.point(0)[1]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: identity cancellation               //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_zoommap_inverse_via_winmap() {
        // ZoomMap(1,3) then WinMap(s=0,c=1/3): out = (1/3)*(3*x) = x → UnitMap
        use crate::mappings::WinMap;

        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let b = dyn_map(WinMap::new(&[0.0], &[1.0 / 3.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // Mixed chain: three different linear types                    //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_mixed_linear_chain() {
        use crate::coords::PointSet;

        // ShiftMap([1]) then ZoomMap(1,3) then ShiftMap([10])
        // Step 1: x + 1
        // Step 2: 3*(x+1) = 3*x + 3
        // Step 3: 3*x + 3 + 10 = 3*x + 13
        let a = dyn_map(ShiftMap::new(vec![1.0]));
        let b = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let inner = CmpMap::new(a, b, Composition::Series).unwrap();
        let c = dyn_map(ShiftMap::new(vec![10.0]));
        let outer = CmpMap::new(dyn_map(inner), c, Composition::Series).unwrap();
        let result = simplify_mapping(&outer).unwrap();

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 28.0).abs() < 1e-12); // 3*5 + 13

        // Verify it reduced to a single mapping (not a CmpMap).
        assert!(!result.as_any().is::<CmpMap>());
    }

    // ------------------------------------------------------------ //
    // MatrixMap self-reduction                                      //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_matrix_identity_reduces_to_unitmap() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        let m = array![[1.0_f64, 0.0], [0.0, 1.0]];
        let mm = MatrixMap::new(m).unwrap();
        let result = mm.simplified().unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_matrix_uniform_diagonal_reduces_to_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        let m = array![[3.0_f64, 0.0], [0.0, 3.0]];
        let mm = MatrixMap::new(m).unwrap();
        let result = mm.simplified().unwrap();
        assert!(result.as_any().is::<ZoomMap>());
        assert_eq!(result.nin(), 2);

        let pts = PointSet::from_rows([[2.0, 5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 15.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_matrix_nonuniform_diagonal_stays_matrix() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // Non-uniform diagonal: different scale per axis → stays MatrixMap.
        let m = array![[2.0_f64, 0.0], [0.0, 5.0]];
        let mm = MatrixMap::new(m).unwrap();
        let result = mm.simplified().unwrap();
        assert!(result.as_any().is::<MatrixMap>());
    }

    #[test]
    fn simplify_matrix_non_square_stays_matrix() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // Non-square matrix can't reduce.
        let m = array![[1.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let mm = MatrixMap::new(m).unwrap();
        let result = mm.simplified().unwrap();
        assert!(result.as_any().is::<MatrixMap>());
    }

    #[test]
    fn simplify_matrix_inverted_diagonal_reduces_to_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // An inverted uniform diagonal should reduce to an inverted ZoomMap.
        let m = array![[4.0_f64, 0.0], [0.0, 4.0]];
        let mut mm = MatrixMap::new(m).unwrap();
        mm.invert();
        let result = mm.simplified().unwrap();
        assert!(result.as_any().is::<ZoomMap>());
        assert!(result.is_inverted());

        // Forward transform of inverted ZoomMap(4) = divide by 4.
        let pts = PointSet::from_rows([[8.0, 12.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 2.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 3.0).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // ZoomMap absorbed into MatrixMap                               //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_zoommap_then_matrixmap() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // ZoomMap(2,3) then MatrixMap([[1,2],[3,4]]): result = 3 * M * x
        let a = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let m = array![[1.0_f64, 2.0], [3.0, 4.0]];
        let b = dyn_map(MatrixMap::new(m).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        // Should merge to a single MatrixMap.
        assert!(simplified.as_any().is::<MatrixMap>());

        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - after.point(0)[0]).abs() < 1e-12);
        assert!((before.point(0)[1] - after.point(0)[1]).abs() < 1e-12);
    }

    #[test]
    fn simplify_matrixmap_then_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // MatrixMap([[1,2],[3,4]]) then ZoomMap(2,3): result = 3 * M * x
        let m = array![[1.0_f64, 2.0], [3.0, 4.0]];
        let a = dyn_map(MatrixMap::new(m).unwrap());
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();
        assert!(simplified.as_any().is::<MatrixMap>());

        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - after.point(0)[0]).abs() < 1e-12);
        assert!((before.point(0)[1] - after.point(0)[1]).abs() < 1e-12);
    }

    #[test]
    fn simplify_matrixmap_then_inv_zoommap() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // MatrixMap(3*I) then ZoomMap(2,3)^-1: (1/3)*(3*I*x) = I*x → UnitMap
        let m = array![[3.0_f64, 0.0], [0.0, 3.0]];
        let a = dyn_map(MatrixMap::new(m).unwrap());
        let mut z = ZoomMap::new(2, 3.0).unwrap();
        z.invert();
        let cmp = CmpMap::new(a, dyn_map(z), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // MatrixMap(3I) simplifies to ZoomMap(3), then ZoomMap(3)*ZoomMap(1/3) = UnitMap
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_diagonal_matrix_series_reduces() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // Two uniform diagonal matrices: diag(2,2) * diag(3,3) = diag(6,6) → ZoomMap(2,6)
        let s2 = array![[2.0_f64, 0.0], [0.0, 2.0]];
        let s3 = array![[3.0_f64, 0.0], [0.0, 3.0]];
        let a = dyn_map(MatrixMap::new(s2).unwrap());
        let b = dyn_map(MatrixMap::new(s3).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        // Both MatrixMaps simplify to ZoomMaps, which then merge to ZoomMap(2,6).
        assert!(simplified.as_any().is::<ZoomMap>());

        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let out = simplified.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 12.0).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type: dimension mismatch gives no merge                //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_cross_type_dimension_mismatch_no_merge() {
        // ZoomMap(2-dim) + ShiftMap(3-dim): can't merge due to dimension mismatch.
        // These can't form a valid CmpMap in series either, but try_merge
        // should return None regardless.
        let z = ZoomMap::new(2, 3.0).unwrap();
        let s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
        let result = z.try_merge(&s, true, false, false);
        assert!(result.is_none());
    }
}

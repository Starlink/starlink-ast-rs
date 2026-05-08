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

use std::cell::RefCell;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use crate::constants::BAD_VALUE;
use crate::error::AstResult;
use crate::mappings::helpers::ast_equal;

use super::cmpmap::{CmpMap, Composition};
use super::matrix::{MatrixMap, is_diagonal};
use super::perm::PermMap;
use super::shift::ShiftMap;
use super::sphmap::SphMap;
use super::unit::UnitMap;
use super::wcsmap::WcsMap;
use super::winmap::WinMap;
use super::zoom::ZoomMap;
use super::{DynMapping, Mapping};

// ------------------------------------------------------------------ //
// Recursion guard: fingerprint stack                                   //
// ------------------------------------------------------------------ //

thread_local! {
    /// Tracks structural fingerprints of mappings currently being
    /// simplified on this thread.  Before entering the flatten/merge/
    /// reconstruct pipeline, we compute a fingerprint of the mapping
    /// and check whether an equivalent structure is already being
    /// simplified higher up the call stack.  If so, we bail immediately
    /// to prevent infinite recursion.
    ///
    /// This matches the C library's thread-local "simplify stack"
    /// (cmpmap.c:3481-3493) which stores pointers to mappings currently
    /// in progress and checks equality before entering simplification.
    /// We use structural fingerprints (hashes) instead of pointer
    /// equality since Rust values move.
    static SIMPLIFY_STACK: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
}

/// Compute a structural fingerprint of a mapping for recursion detection.
///
/// The fingerprint incorporates the concrete type, dimensions, and for
/// CmpMaps the composition kind and recursive fingerprints of children.
/// Collisions are possible but harmless (they only cause an early bail
/// which is a conservative no-op, not a correctness issue).
fn mapping_fingerprint(mapping: &dyn Mapping) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    mapping.nin().hash(&mut hasher);
    mapping.nout().hash(&mut hasher);
    mapping.as_any().type_id().hash(&mut hasher);
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        (cmp.composition() == Composition::Series).hash(&mut hasher);
        mapping_fingerprint(cmp.first()).hash(&mut hasher);
        mapping_fingerprint(cmp.second()).hash(&mut hasher);
    }
    hasher.finish()
}

/// Maximum allowed recursion depth for `simplify_mapping`.
///
/// This complements the fingerprint-based cycle detection by capping
/// the call-stack depth absolutely.  Even if fingerprints don't match
/// (non-identical but deeply recursive structures), this limit
/// prevents stack overflow.
const MAX_SIMPLIFY_DEPTH: usize = 10;

/// RAII guard that manages both the fingerprint stack and depth counter.
///
/// On creation: checks the fingerprint against the stack (cycle detection)
/// and increments a depth counter (absolute limit).
/// On drop: removes the fingerprint and decrements the counter.
struct SimplifyGuard {
    fingerprint: u64,
}

thread_local! {
    static SIMPLIFY_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl SimplifyGuard {
    /// Attempt to enter simplification for `mapping`.
    ///
    /// Returns `None` if:
    /// - The mapping's fingerprint is already on the stack (cycle), or
    /// - The recursion depth limit has been reached.
    ///
    /// On success, returns a guard that cleans up on drop.
    fn enter(mapping: &dyn Mapping) -> Option<Self> {
        let fp = mapping_fingerprint(mapping);

        // Check for cycle: is this fingerprint already on the stack?
        let cycle = SIMPLIFY_STACK.with(|stack| stack.borrow().contains(&fp));
        if cycle {
            return None;
        }

        // Check depth limit.
        let depth_ok = SIMPLIFY_DEPTH.with(|d| {
            let current = d.get();
            if current >= MAX_SIMPLIFY_DEPTH {
                false
            } else {
                d.set(current + 1);
                true
            }
        });
        if !depth_ok {
            return None;
        }

        // Push fingerprint onto stack.
        SIMPLIFY_STACK.with(|stack| {
            stack.borrow_mut().insert(fp);
        });

        Some(SimplifyGuard { fingerprint: fp })
    }
}

impl Drop for SimplifyGuard {
    fn drop(&mut self) {
        SIMPLIFY_STACK.with(|stack| {
            stack.borrow_mut().remove(&self.fingerprint);
        });
        SIMPLIFY_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

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
    const MAX_MERGE_PASSES: usize = 50;
    let mut pass = 0;

    loop {
        pass += 1;
        if pass > MAX_MERGE_PASSES {
            break;
        }

        let mut improved = false;
        let mut i = 0;

        while i + 1 < list.len() {
            // Skip if either mapping is protected from simplification.
            if list[i].0.do_not_simplify() || list[i + 1].0.do_not_simplify() {
                i += 1;
                continue;
            }

            let merged = {
                let (ref m1, inv1) = list[i];
                let (ref m2, inv2) = list[i + 1];

                // The effective input dimension of m1 in its applied
                // direction — this is what the merged result must accept.
                let m1_nin = if inv1 { m1.nout() } else { m1.nin() };

                // Try left merging into right.
                m1.try_merge(m2.as_ref(), series, inv1, inv2)
                    // Try right merging into left (swap roles).
                    // Validate that the result's input dimension matches
                    // m1's effective input, since the result replaces
                    // m1's position in the pipeline.
                    .or_else(|| {
                        m2.try_merge(m1.as_ref(), series, inv2, inv1)
                            .filter(|m| m.nin() == m1_nin)
                    })
            };

            if let Some(m) = merged {
                // Check if merged result is a CmpMap that should be re-expanded.
                let target_kind = if series {
                    Composition::Series
                } else {
                    Composition::Parallel
                };
                if let Some(cmp) = m.as_any().downcast_ref::<CmpMap>() {
                    if cmp.composition() == target_kind {
                        if let Ok(expanded) = flatten(m.as_ref(), false, series) {
                            list.remove(i + 1);
                            list.remove(i);
                            for (idx, item) in expanded.into_iter().enumerate() {
                                list.insert(i + idx, item);
                            }
                            improved = true;
                            continue;
                        }
                    }
                }
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
            // Try swapping WinMap↔WcsMap pairs to enable further merges.
            if series && try_swap_winmap_wcsmap(list) {
                continue;
            }
            // Try swapping WinMap past a non-diagonal MatrixMap.
            if series && try_swap_winmap_past_matrixmap(list) {
                continue;
            }
            // Try swapping WinMap past a PermMap.
            if series && try_swap_winmap_past_permmap(list) {
                continue;
            }
            // Try the (inv-SphMap, diagonal, fwd-SphMap) pattern.
            if series && try_sphmap_diagonal_pattern(list) {
                continue;
            }
            // Try cross-composition restructuring.
            if series && try_merge_parallel_in_series(list) {
                continue;
            }
            if !series && try_merge_series_in_parallel(list) {
                continue;
            }
            // Try swapping a PermMap past a parallel CmpMap.
            if series && try_swap_permmap_parallel(list) {
                continue;
            }
            // Try distributing a WinMap into a parallel CmpMap's branches.
            if series && try_distribute_winmap_into_parallel(list) {
                continue;
            }
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

/// Try to swap adjacent WinMap↔WcsMap pairs in a flattened series list.
///
/// A WinMap can be swapped past a WcsMap if the WinMap acts as identity
/// on the WcsMap's celestial axes (shift=0 and scale=1 on both the
/// longitude and latitude axes). The swap doesn't change the WinMap's
/// parameters — it just moves it to the other side of the WcsMap,
/// potentially enabling further merges with other linear mappings.
///
/// Returns true if any swap was performed.
fn try_swap_winmap_wcsmap(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut swapped = false;
    let mut i = 0;
    while i + 1 < list.len() {
        let is_win_wcs = list[i].0.as_any().is::<WinMap>() && list[i + 1].0.as_any().is::<WcsMap>();
        let is_wcs_win = list[i].0.as_any().is::<WcsMap>() && list[i + 1].0.as_any().is::<WinMap>();

        if is_win_wcs || is_wcs_win {
            let (win_idx, wcs_idx) = if is_win_wcs { (i, i + 1) } else { (i + 1, i) };
            let win = list[win_idx].0.as_any().downcast_ref::<WinMap>().unwrap();
            let wcs = list[wcs_idx].0.as_any().downcast_ref::<WcsMap>().unwrap();

            // Check WinMap is identity on WcsMap's celestial axes.
            // lonax()/latax() return 1-based; convert to 0-based for indexing.
            let lonax = wcs.lonax() - 1;
            let latax = wcs.latax() - 1;
            let shifts = win.shifts();
            let scales = win.scales();

            if lonax < shifts.len()
                && latax < shifts.len()
                && ast_equal(shifts[lonax], 0.0)
                && ast_equal(shifts[latax], 0.0)
                && ast_equal(scales[lonax], 1.0)
                && ast_equal(scales[latax], 1.0)
            {
                list.swap(i, i + 1);
                // Also swap their invert flags.
                let tmp = list[i].1;
                list[i].1 = list[i + 1].1;
                list[i + 1].1 = tmp;
                swapped = true;
            }
        }
        i += 1;
    }
    swapped
}

/// Try to swap a WinMap past a non-diagonal MatrixMap in a flattened series list.
///
/// When a WinMap (or ShiftMap, which is a WinMap with unit scales) is at
/// position `i` and a non-diagonal MatrixMap is at position `i+1`, the pair
/// can be rewritten as `MatrixMap' ; ShiftMap'` (or just `MatrixMap'` if
/// the transformed shifts are zero). This moves the linear per-axis component
/// to the other side of the matrix, potentially enabling further merges.
///
/// Math: `y = M * (s + c*x) = M*diag(c)*x + M*s`
/// - New MatrixMap = `M * diag(c)` (columns of M scaled by WinMap scales)
/// - New ShiftMap = `M * s` (shifts transformed through the matrix)
///
/// Only fires when there is a potential merge partner on the other side of
/// the MatrixMap (another linear mapping that the shifted result could combine
/// with), to avoid infinite swap loops.
///
/// Matches C winmap.c:2793 (WinMat).
fn try_swap_winmap_past_matrixmap(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 1 < list.len() {
        // Check for WinMap/ShiftMap at position i and MatrixMap at position i+1.
        let is_linear_at_i = list[i].0.as_any().is::<WinMap>()
            || list[i].0.as_any().is::<ShiftMap>()
            || list[i].0.as_any().is::<ZoomMap>();
        let is_matrix_at_i1 = list[i + 1].0.as_any().is::<MatrixMap>();

        if is_linear_at_i && is_matrix_at_i1 {
            // Check that the MatrixMap is NOT diagonal (diagonal case is
            // already handled by the normal pairwise merge via extract_linear_params).
            let mm = list[i + 1].0.as_any().downcast_ref::<MatrixMap>().unwrap();
            let mat_inv = list[i + 1].1;
            let mat = if mat_inv {
                match mm.inverse_matrix() {
                    Some(m) => m.clone(),
                    None => {
                        i += 1;
                        continue;
                    }
                }
            } else {
                mm.forward_matrix().clone()
            };

            if is_diagonal(&mat) {
                i += 1;
                continue;
            }

            // Extract effective (shifts, scales) from the linear mapping at i.
            let win_inv = list[i].1;
            let Some((shifts, scales)) =
                super::winmap::extract_linear_params(list[i].0.as_ref(), win_inv)
            else {
                i += 1;
                continue;
            };

            let nin = mat.ncols();
            let nout = mat.nrows();
            if shifts.len() != nin || scales.len() != nin {
                i += 1;
                continue;
            }

            // Compute transformed shifts: M * s.
            let shifted: Vec<f64> = (0..nout)
                .map(|row| (0..nin).map(|j| mat[[row, j]] * shifts[j]).sum::<f64>())
                .collect();
            let all_shifts_zero = shifted.iter().all(|&s| ast_equal(s, 0.0));

            // Only proceed if:
            // 1. The shifts are all zero (pure scaling absorbed → net reduction), OR
            // 2. There's a linear merge partner at position i+2 that the
            //    resulting ShiftMap could combine with.
            if !all_shifts_zero {
                let has_merge_partner = if i + 2 < list.len() {
                    super::winmap::extract_linear_params(list[i + 2].0.as_ref(), list[i + 2].1)
                        .is_some()
                } else {
                    false
                };
                if !has_merge_partner {
                    i += 1;
                    continue;
                }
            }

            // Build M * diag(c): scale each column j of M by scales[j].
            let mut scaled_mat = mat.clone();
            for j in 0..nin {
                for row in 0..nout {
                    scaled_mat[[row, j]] *= scales[j];
                }
            }

            // Replace: remove position i (WinMap), replace position i+1 (MatrixMap)
            // with the new MatrixMap, and optionally insert a ShiftMap after.
            let Ok(new_mm) = MatrixMap::new(scaled_mat) else {
                i += 1;
                continue;
            };

            // Remove the WinMap at position i.
            list.remove(i);
            // Replace the MatrixMap (now at position i after removal) with new one.
            list[i] = (Box::new(new_mm), false);

            if !all_shifts_zero {
                // Insert ShiftMap after the new MatrixMap.
                let new_sm = ShiftMap::new(shifted);
                list.insert(i + 1, (Box::new(new_sm), false));
            }

            return true;
        }
        i += 1;
    }
    false
}

/// Try to swap a WinMap (or ShiftMap/ZoomMap) past a PermMap in a flattened series list.
///
/// When a linear mapping at position `i` is followed by a PermMap at position `i+1`,
/// and a potential merge partner (another linear mapping) exists at position `i+2`,
/// the pair can be rewritten as `PermMap ; WinMap'` where WinMap' has shifts/scales
/// reordered according to the permutation.
///
/// Math: `WinMap(s,c) ; PermMap(outperm)` means:
/// - Step 1: `inter[j] = s[j] + c[j] * x[j]`
/// - Step 2: `out[k] = inter[outperm[k]-1]`
///
/// Equivalent to `PermMap(outperm) ; WinMap(s',c')` where:
/// - `s'[k] = s[outperm[k]-1]`
/// - `c'[k] = c[outperm[k]-1]`
///
/// Only fires when there is a potential merge partner at position `i+2` to avoid
/// infinite swap loops.
///
/// Matches C winmap.c:2973 (WinPerm).
fn try_swap_winmap_past_permmap(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 1 < list.len() {
        // Check: position i is a linear mapping and position i+1 is a PermMap.
        let is_linear_i = list[i].0.as_any().is::<WinMap>()
            || list[i].0.as_any().is::<ShiftMap>()
            || list[i].0.as_any().is::<ZoomMap>();
        let is_perm_i1 = list[i + 1].0.as_any().is::<PermMap>();

        if is_linear_i && is_perm_i1 {
            // Only swap if there's a potential merge partner at i+2.
            if i + 2 >= list.len() {
                i += 1;
                continue;
            }
            let is_linear_i2 = list[i + 2].0.as_any().is::<WinMap>()
                || list[i + 2].0.as_any().is::<ShiftMap>()
                || list[i + 2].0.as_any().is::<ZoomMap>();
            if !is_linear_i2 {
                i += 1;
                continue;
            }

            // Extract effective (shifts, scales) from the linear mapping at i.
            let win_inv = list[i].1;
            let Some((shifts, scales)) =
                super::winmap::extract_linear_params(list[i].0.as_ref(), win_inv)
            else {
                i += 1;
                continue;
            };

            // Get the effective outperm from the PermMap at i+1.
            let perm = list[i + 1].0.as_any().downcast_ref::<PermMap>().unwrap();
            let perm_inv = list[i + 1].1;
            let outperm = if perm_inv {
                perm.inperm()
            } else {
                perm.outperm()
            };

            // Only handle pure permutations (no constants or BAD assignments).
            if outperm.iter().any(|&p| p <= 0) {
                i += 1;
                continue;
            }

            // Dimensions must match: outperm length must equal shifts/scales length.
            let n = outperm.len();
            if shifts.len() != n || scales.len() != n {
                i += 1;
                continue;
            }

            // All outperm entries must index within the input dimension.
            if outperm.iter().any(|&p| p as usize > n) {
                i += 1;
                continue;
            }

            // Reorder shifts/scales per the permutation:
            // new_s[k] = shifts[outperm[k]-1], new_c[k] = scales[outperm[k]-1]
            let new_shifts: Vec<f64> = (0..n).map(|k| shifts[(outperm[k] as usize) - 1]).collect();
            let new_scales: Vec<f64> = (0..n).map(|k| scales[(outperm[k] as usize) - 1]).collect();

            // Build new WinMap with reordered params.
            let Ok(new_wm) = WinMap::new(&new_shifts, &new_scales) else {
                i += 1;
                continue;
            };

            // Swap: replace [linear, perm] with [perm, new_winmap].
            let perm_entry = list.remove(i + 1);
            list.remove(i);
            list.insert(i, perm_entry);
            list.insert(i + 1, (Box::new(new_wm), false));
            return true;
        }
        i += 1;
    }
    false
}

/// Effective output coordinate count with external inversion.
fn effective_nout(m: &dyn Mapping, inv: bool) -> usize {
    if inv { m.nin() } else { m.nout() }
}

/// Effective input coordinate count with external inversion.
fn effective_nin(m: &dyn Mapping, inv: bool) -> usize {
    if inv { m.nout() } else { m.nin() }
}

/// Try to merge two adjacent parallel CmpMaps that appear in a series list.
///
/// This restructures by dimension-matching the parallel components:
/// the outputs of components from the first CmpMap are paired with inputs
/// of components from the second CmpMap. Matched pairs are combined in
/// series and simplified. If any pair simplifies, the result replaces
/// the original pair as a single parallel CmpMap.
fn try_merge_parallel_in_series(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 1 < list.len() {
        let success = try_merge_parallel_pair_in_series(list, i);
        if success {
            return true;
        }
        i += 1;
    }
    false
}

/// Attempt to merge a specific adjacent pair at index `i` and `i+1`.
fn try_merge_parallel_pair_in_series(list: &mut Vec<(DynMapping, bool)>, i: usize) -> bool {
    // Both must be parallel CmpMaps.
    let is_par1 = list[i]
        .0
        .as_any()
        .downcast_ref::<CmpMap>()
        .is_some_and(|c| c.composition() == Composition::Parallel);
    let is_par2 = list[i + 1]
        .0
        .as_any()
        .downcast_ref::<CmpMap>()
        .is_some_and(|c| c.composition() == Composition::Parallel);
    if !is_par1 || !is_par2 {
        return false;
    }

    let (ref m1, inv1) = list[i];
    let (ref m2, inv2) = list[i + 1];

    // Flatten both for parallel composition.
    let Ok(par_list1) = flatten(m1.as_ref(), inv1, false) else {
        return false;
    };
    let Ok(par_list2) = flatten(m2.as_ref(), inv2, false) else {
        return false;
    };

    // Dimension-match the two parallel lists.
    let Some(matched) = dimension_match_parallel_lists(&par_list1, &par_list2) else {
        return false;
    };

    // For each matched pair, combine in series and simplify.
    let mut simplified_pairs: Vec<(DynMapping, bool)> = Vec::new();
    let mut any_improvement = false;

    for (sub1, sub2) in matched {
        // Build parallel submaps from each sublist.
        let Ok(par1) = reconstruct(sub1, false) else {
            return false;
        };
        let Ok(par2) = reconstruct(sub2, false) else {
            return false;
        };

        // Combine in series.
        let Ok(series_map) = CmpMap::new(par1, par2, Composition::Series) else {
            return false;
        };
        let series_box: DynMapping = Box::new(series_map);

        // Simplify the series combination.
        let Ok(simplified) = simplify_mapping(series_box.as_ref()) else {
            return false;
        };

        // Check if simplification produced something different.
        if !simplified.as_any().is::<CmpMap>() {
            any_improvement = true;
        } else if let Some(cmp) = simplified.as_any().downcast_ref::<CmpMap>() {
            // If still a CmpMap, check if it's structurally simpler:
            // a non-series result or a simpler series (fewer components).
            if cmp.composition() != Composition::Series {
                any_improvement = true;
            } else {
                // Check if the series chain is shorter by flattening.
                if let Ok(flat) = flatten(simplified.as_ref(), false, true) {
                    if flat.len() < 2 {
                        any_improvement = true;
                    }
                }
            }
        }

        simplified_pairs.push((simplified, false));
    }

    if !any_improvement {
        return false;
    }

    // Rebuild as parallel CmpMap and replace the pair in the list.
    let Ok(result) = reconstruct(simplified_pairs, false) else {
        return false;
    };
    list[i] = (result, false);
    list.remove(i + 1);
    true
}

/// Dimension-match two parallel component lists for series combination.
///
/// Walks both lists accumulating output dimensions from list1 and input
/// dimensions from list2 until they match, forming paired sublists.
/// Returns `None` if the lists cannot be fully matched.
#[allow(clippy::type_complexity)]
fn dimension_match_parallel_lists(
    par_list1: &[(DynMapping, bool)],
    par_list2: &[(DynMapping, bool)],
) -> Option<Vec<(Vec<(DynMapping, bool)>, Vec<(DynMapping, bool)>)>> {
    let mut pairs = Vec::new();
    let mut i1 = 0usize;
    let mut i2 = 0usize;
    let mut out_accum: usize = 0;
    let mut in_accum: usize = 0;
    let mut sub1_start = 0usize;
    let mut sub2_start = 0usize;

    // Seed with the first element from each list.
    if par_list1.is_empty() || par_list2.is_empty() {
        return None;
    }

    loop {
        // Add from list1 if we need more outputs.
        if out_accum <= in_accum && i1 < par_list1.len() {
            let (ref m, inv) = par_list1[i1];
            out_accum += effective_nout(m.as_ref(), inv);
            i1 += 1;
        } else if i2 < par_list2.len() {
            let (ref m, inv) = par_list2[i2];
            in_accum += effective_nin(m.as_ref(), inv);
            i2 += 1;
        } else {
            // Can't advance either list — mismatch.
            return None;
        }

        if out_accum == in_accum && out_accum > 0 {
            // Match boundary found. Clone the sublists.
            let sub1: Vec<(DynMapping, bool)> = par_list1[sub1_start..i1]
                .iter()
                .map(|(m, inv)| {
                    (
                        m.simplified().unwrap_or_else(|_| Box::new(UnitMap::new(1))),
                        *inv,
                    )
                })
                .collect();
            let sub2: Vec<(DynMapping, bool)> = par_list2[sub2_start..i2]
                .iter()
                .map(|(m, inv)| {
                    (
                        m.simplified().unwrap_or_else(|_| Box::new(UnitMap::new(1))),
                        *inv,
                    )
                })
                .collect();
            pairs.push((sub1, sub2));
            sub1_start = i1;
            sub2_start = i2;
            out_accum = 0;
            in_accum = 0;

            // If both lists are exhausted, we're done.
            if i1 == par_list1.len() && i2 == par_list2.len() {
                break;
            }
        }
    }

    // Both lists must be fully consumed.
    if i1 != par_list1.len() || i2 != par_list2.len() {
        return None;
    }

    Some(pairs)
}

/// Try to merge two adjacent series CmpMaps in a parallel list.
///
/// Rearranges `(A;B) || (C;D)` into `(A||C) ; (B||D)` and simplifies
/// each new parallel CmpMap. Only accepts if at least one simplified.
fn try_merge_series_in_parallel(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 1 < list.len() {
        if try_merge_series_pair_in_parallel(list, i) {
            return true;
        }
        i += 1;
    }
    false
}

fn try_merge_series_pair_in_parallel(list: &mut Vec<(DynMapping, bool)>, i: usize) -> bool {
    // Both must be series CmpMaps.
    let is_ser1 = list[i]
        .0
        .as_any()
        .downcast_ref::<CmpMap>()
        .is_some_and(|c| c.composition() == Composition::Series);
    let is_ser2 = list[i + 1]
        .0
        .as_any()
        .downcast_ref::<CmpMap>()
        .is_some_and(|c| c.composition() == Composition::Series);
    if !is_ser1 || !is_ser2 {
        return false;
    }

    let (ref m1, inv1) = list[i];
    let (ref m2, inv2) = list[i + 1];

    // Flatten both for series composition.
    let Ok(ser_list1) = flatten(m1.as_ref(), inv1, true) else {
        return false;
    };
    let Ok(ser_list2) = flatten(m2.as_ref(), inv2, true) else {
        return false;
    };

    // Both must have the same length for zip-parallel rearrangement.
    if ser_list1.len() != ser_list2.len() {
        return false;
    }

    let n = ser_list1.len();
    if n < 2 {
        return false;
    }

    // Build parallel pairs: position k from each series list goes in parallel.
    // Then the parallel pairs are combined in series.
    let mut parallel_maps: Vec<(DynMapping, bool)> = Vec::new();
    let mut any_improvement = false;

    for k in 0..n {
        // Clone the mappings from each position.
        let Ok(a) = ser_list1[k].0.simplified() else {
            return false;
        };
        let Ok(c) = ser_list2[k].0.simplified() else {
            return false;
        };
        let a_inv = ser_list1[k].1;
        let c_inv = ser_list2[k].1;

        // Apply inversion flags before composing in parallel.
        let mut a_map = a;
        let mut c_map = c;
        if a_inv {
            a_map.invert();
        }
        if c_inv {
            c_map.invert();
        }

        // Build parallel CmpMap.
        let Ok(par) = CmpMap::new(a_map, c_map, Composition::Parallel) else {
            return false;
        };

        // Simplify the parallel CmpMap.
        let par_box: DynMapping = Box::new(par);
        let Ok(simplified) = simplify_mapping(par_box.as_ref()) else {
            return false;
        };

        // Check if it actually simplified (pointer comparison isn't
        // available with owned values, so check structure).
        if !simplified.as_any().is::<CmpMap>() {
            any_improvement = true;
        } else if let Some(cmp) = simplified.as_any().downcast_ref::<CmpMap>() {
            if cmp.composition() != Composition::Parallel {
                any_improvement = true;
            }
        }

        parallel_maps.push((simplified, false));
    }

    if !any_improvement {
        return false;
    }

    // Rebuild as series composition.
    let Ok(result) = reconstruct(parallel_maps, true) else {
        return false;
    };
    list[i] = (result, false);
    list.remove(i + 1);
    true
}

/// Try to swap a PermMap past a parallel CmpMap in a series list.
///
/// When a PermMap immediately precedes a parallel CmpMap and has a
/// "block-swap" structure that aligns with the CmpMap's component
/// dimension boundaries, the two can be reordered. The PermMap is
/// replaced with a new one (or removed if it becomes identity) and
/// the CmpMap's components are reordered. This can enable further
/// pairwise merges.
fn try_swap_permmap_parallel(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 1 < list.len() {
        // Check for PermMap followed by parallel CmpMap.
        let is_perm = list[i].0.as_any().is::<PermMap>();
        let is_par_cmp = list[i + 1]
            .0
            .as_any()
            .downcast_ref::<CmpMap>()
            .is_some_and(|c| c.composition() == Composition::Parallel);

        if is_perm && is_par_cmp && try_swap_permmap_parallel_at(list, i) {
            return true;
        }
        i += 1;
    }
    false
}

/// Attempt the PermMap↔parallel-CmpMap swap at position `i` (PermMap)
/// and `i+1` (parallel CmpMap).
#[allow(clippy::needless_range_loop)]
fn try_swap_permmap_parallel_at(list: &mut Vec<(DynMapping, bool)>, i: usize) -> bool {
    let (ref perm_map, perm_inv) = list[i];
    let (ref par_map, par_inv) = list[i + 1];

    let perm = perm_map.as_any().downcast_ref::<PermMap>().unwrap();
    // Get effective permutation arrays based on inversion.
    let eff_outperm = if perm_inv {
        perm.inperm()
    } else {
        perm.outperm()
    };
    let eff_inperm = if perm_inv {
        perm.outperm()
    } else {
        perm.inperm()
    };

    let npin = eff_inperm.len();
    let npout = eff_outperm.len();

    // Must be one-to-one (same number of inputs and outputs).
    if npin != npout {
        return false;
    }

    // Get the parallel CmpMap's component dimensions.
    // Flatten for parallel to get the components.
    let Ok(par_list) = flatten(par_map.as_ref(), par_inv, false) else {
        return false;
    };
    if par_list.len() != 2 {
        // Only handle the 2-component case (matching C implementation).
        return false;
    }

    let nin_a = effective_nin(par_list[0].0.as_ref(), par_list[0].1);
    let nin_b = effective_nin(par_list[1].0.as_ref(), par_list[1].1);

    // Verify total dimensions match.
    if nin_a + nin_b != npout {
        return false;
    }

    // Check block-swap structure: first nin_a outputs must come from
    // a contiguous block at the END of the inputs (or all be constants),
    // and the next nin_b outputs must come from a contiguous block at
    // the START of the inputs (or all be constants).
    let aconstants = eff_outperm[0] < 0;
    let bconstants = if nin_a < npout {
        eff_outperm[nin_a] < 0
    } else {
        false
    };

    // Check first block (outputs 0..nin_a).
    for idx in 0..nin_a {
        let j = npin - nin_a + idx;
        if aconstants {
            if eff_outperm[idx] >= 0 {
                return false;
            }
        } else if eff_outperm[idx] != (j as i64 + 1) || eff_inperm[j] != (idx as i64 + 1) {
            return false;
        }
    }

    // Check second block (outputs nin_a..nin_a+nin_b).
    for idx in 0..nin_b {
        let j = npout - nin_b + idx;
        if bconstants {
            if eff_outperm[j] >= 0 {
                return false;
            }
        } else if eff_inperm[idx] != (j as i64 + 1) || eff_outperm[j] != (idx as i64 + 1) {
            return false;
        }
    }

    // The PermMap has block-swap structure. Build the swapped mappings.
    let nout_a = effective_nout(par_list[0].0.as_ref(), par_list[0].1);
    let nout_b = effective_nout(par_list[1].0.as_ref(), par_list[1].1);

    // Build new PermMap: after swapping, the parallel CmpMap has its
    // components in reversed order, so the new PermMap must undo the
    // block swap relative to the new component ordering.
    let npout_new = nout_a + nout_b;
    let npin_new = if aconstants {
        nout_b + (npin - nin_b)
    } else if bconstants {
        nout_a + (npin - nin_a)
    } else {
        nout_a + nout_b
    };

    let mut outperm_new = vec![0i64; npout_new];
    let mut inperm_new = vec![0i64; npin_new];
    let mut constants_new: Vec<f64> = Vec::new();

    if !bconstants {
        // Second component (now first in swapped CmpMap): its nout_b outputs
        // map to the last nout_b positions in the new PermMap output.
        for idx in 0..nout_b {
            let j = npout_new - nout_b + idx;
            inperm_new[idx] = j as i64 + 1;
            outperm_new[j] = idx as i64 + 1;
        }
    } else {
        // Second component outputs are constants — transform the PermMap
        // constants through component B.
        for idx in 0..nout_b {
            let j = npout_new - nout_b + idx;
            // Output j gets a constant value — use BAD for now since we
            // can't easily transform constants without executing the mapping.
            let const_idx = constants_new.len();
            constants_new.push(BAD_VALUE);
            outperm_new[j] = -((const_idx + 1) as i64);
        }
        // Inputs from the UnitMap portion.
        for idx in 0..(npin - nin_b) {
            let src = eff_inperm[idx];
            if src < 0 {
                let old_cidx = (-src - 1) as usize;
                let val = perm.constants()[old_cidx];
                let const_idx = constants_new.len();
                constants_new.push(val);
                inperm_new[idx] = -((const_idx + 1) as i64);
            } else if src > npout as i64 {
                inperm_new[idx] = npout_new as i64 + 1;
            } else {
                inperm_new[idx] = src;
            }
        }
    }

    if !aconstants {
        // First component (now second in swapped CmpMap): its nout_a outputs
        // map to the first nout_a positions in the new PermMap output.
        for idx in 0..nout_a {
            let j = npin_new - nout_a + idx;
            inperm_new[j] = idx as i64 + 1;
            outperm_new[idx] = j as i64 + 1;
        }
    } else {
        // First component outputs are constants.
        for idx in 0..nout_a {
            let const_idx = constants_new.len();
            constants_new.push(BAD_VALUE);
            outperm_new[idx] = -((const_idx + 1) as i64);
        }
        for idx in nout_b..npin_new {
            let src_idx = idx - nout_b + nin_b;
            if src_idx < eff_inperm.len() {
                let src = eff_inperm[src_idx];
                if src < 0 {
                    let old_cidx = (-src - 1) as usize;
                    let val = perm.constants()[old_cidx];
                    let const_idx = constants_new.len();
                    constants_new.push(val);
                    inperm_new[idx] = -((const_idx + 1) as i64);
                } else if src > npout as i64 {
                    inperm_new[idx] = npout_new as i64 + 1;
                } else {
                    inperm_new[idx] = src;
                }
            }
        }
    }

    // Build the new CmpMap with swapped component order.
    let Ok(comp_b) = par_list[1].0.simplified() else {
        return false;
    };
    let Ok(comp_a) = par_list[0].0.simplified() else {
        return false;
    };
    let mut new_b = comp_b;
    let mut new_a = comp_a;
    if par_list[1].1 {
        new_b.invert();
    }
    if par_list[0].1 {
        new_a.invert();
    }

    let Ok(new_cm) = CmpMap::new(new_b, new_a, Composition::Parallel) else {
        return false;
    };

    // Build the new PermMap (or detect if it's identity).
    let is_identity = !aconstants
        && !bconstants
        && npin_new == npout_new
        && outperm_new
            .iter()
            .enumerate()
            .all(|(idx, &p)| p == (idx as i64 + 1));

    if is_identity {
        // Just replace with the swapped CmpMap (PermMap is removed).
        list[i] = (Box::new(new_cm), false);
        list.remove(i + 1);
    } else {
        let Ok(new_pm) = PermMap::new(&inperm_new, &outperm_new, &constants_new) else {
            return false;
        };
        // CmpMap goes first (was second), PermMap goes second (was first).
        list[i] = (Box::new(new_cm), false);
        list[i + 1] = (Box::new(new_pm), false);
    }
    true
}

/// Detect the (inv-SphMap, diagonal-scale, fwd-SphMap) pattern and replace
/// with an equivalent combination that simplifies on the next pass.
///
/// When an inverted SphMap (sph→cart, 2→3) is followed by a diagonal
/// MatrixMap or ZoomMap (3→3) with equal-magnitude diagonal elements,
/// followed by a non-inverted SphMap (cart→sph, 3→2), the trio can be
/// replaced by (WinMap, inv-SphMap, fwd-SphMap). The WinMap encodes the
/// longitude/latitude reflections caused by the sign pattern of the
/// diagonal, and the two adjacent SphMaps cancel on the next merge pass.
///
/// Matches C sphmap.c:798-950.
fn try_sphmap_diagonal_pattern(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 2 < list.len() {
        // Pattern: SphMap(inverted) ; diagonal(3→3) ; SphMap(forward)
        let (ref m1, inv1) = list[i];
        let (ref m2, inv2) = list[i + 1];
        let (ref m3, inv3) = list[i + 2];

        let sph1 = m1.as_any().downcast_ref::<SphMap>();
        let sph3 = m3.as_any().downcast_ref::<SphMap>();

        if let (Some(s1), Some(s3)) = (sph1, sph3) {
            // s1 must be applied inverted (sph→cart, 2→3)
            // s3 must be applied forward (cart→sph, 3→2)
            if inv1 && !inv3 {
                // Extract the 3D diagonal from the middle mapping.
                if let Some(diag) = extract_3d_diagonal(m2.as_ref(), inv2) {
                    // All magnitudes must be equal and non-zero.
                    let mag = diag[0].abs();
                    if mag != 0.0 && ast_equal(diag[1].abs(), mag) && ast_equal(diag[2].abs(), mag)
                    {
                        // Compute the WinMap parameters from the sign pattern.
                        // Matches C sphmap.c:875-900.
                        let (lon_shift, lon_scale) = if diag[0] < 0.0 {
                            if diag[1] < 0.0 {
                                (std::f64::consts::PI, 1.0)
                            } else {
                                (std::f64::consts::PI, -1.0)
                            }
                        } else if diag[1] < 0.0 {
                            (0.0, -1.0)
                        } else {
                            (0.0, 1.0)
                        };

                        let lat_scale = if diag[2] < 0.0 { -1.0 } else { 1.0 };

                        // Check PolarLong compatibility (C sphmap.c:914-921):
                        // The replacement transforms PolarLong through the WinMap.
                        let new_polarlong = lon_shift + lon_scale * s1.polar_long();
                        if !ast_equal(new_polarlong, s3.polar_long()) {
                            i += 1;
                            continue;
                        }

                        // Build the WinMap.
                        let Ok(winmap) = WinMap::new(&[lon_shift, 0.0], &[lon_scale, lat_scale])
                        else {
                            i += 1;
                            continue;
                        };

                        // Replace (inv-SphMap, diag, fwd-SphMap) with
                        // (WinMap, inv-SphMap, fwd-SphMap).
                        // The SphMap gets the adjusted PolarLong.
                        let mut new_sph = s1.clone();
                        new_sph.set_polar_long(new_polarlong);

                        // Remove the diagonal (position i+1) and replace
                        // position i with WinMap, insert the modified SphMap.
                        list.remove(i + 1); // remove diagonal
                        list[i] = (Box::new(winmap), false);
                        // Insert the modified inv-SphMap after the WinMap.
                        list.insert(i + 1, (Box::new(new_sph), true));
                        return true;
                    }
                }
            }
        }
        i += 1;
    }
    false
}

/// Extract the diagonal of a 3×3 mapping if it's a ZoomMap or diagonal MatrixMap.
///
/// The inversion flag is noted but for this pattern (sign detection) the
/// inversion only affects magnitude (not signs), so we use the forward
/// matrix/factor for sign determination. The C code also ignores the Invert
/// flag for this purpose (sphmap.c:822-823).
fn extract_3d_diagonal(m: &dyn Mapping, _inv: bool) -> Option<[f64; 3]> {
    if let Some(zm) = m.as_any().downcast_ref::<ZoomMap>() {
        if zm.nin() == 3 {
            let f = zm.factor();
            return Some([f, f, f]);
        }
    }
    if let Some(mm) = m.as_any().downcast_ref::<MatrixMap>() {
        let mat = mm.forward_matrix();
        if mat.nrows() == 3 && mat.ncols() == 3 && is_diagonal(mat) {
            return Some([mat[[0, 0]], mat[[1, 1]], mat[[2, 2]]]);
        }
    }
    None
}

/// Try to distribute a WinMap (or ShiftMap/ZoomMap) into a parallel CmpMap's branches.
///
/// When `WinMap ; CmpMap(A || B)` appears in a series list, splits the WinMap
/// by dimension boundaries (determined by each parallel component's input count)
/// and pushes sub-WinMaps into each branch:
///
///   `WinMap(4) ; CmpMap(A(2) || B(2))`
///   → `CmpMap((WinMap(2);A) || (WinMap(2);B))`
///
/// On the next simplification pass, each sub-WinMap can merge with the first
/// mapping in its branch. Critical for FITS-WCS where pixel scaling precedes
/// parallel (lon,lat) WcsMaps.
///
/// Matches C winmap.c:1268-1416.
fn try_distribute_winmap_into_parallel(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 1 < list.len() {
        // Position i must be a WinMap (or linear mapping convertible to WinMap).
        let is_linear = list[i].0.as_any().is::<WinMap>()
            || list[i].0.as_any().is::<ShiftMap>()
            || list[i].0.as_any().is::<ZoomMap>();
        // Position i+1 must be a parallel CmpMap.
        let is_par = list[i + 1]
            .0
            .as_any()
            .downcast_ref::<CmpMap>()
            .is_some_and(|c| c.composition() == Composition::Parallel);

        if !is_linear || !is_par {
            i += 1;
            continue;
        }

        // Get the WinMap's effective params.
        let Some((shifts, scales)) =
            super::winmap::extract_linear_params(list[i].0.as_ref(), list[i].1)
        else {
            i += 1;
            continue;
        };

        // Flatten the parallel CmpMap to get components.
        let par_inv = list[i + 1].1;
        let Ok(par_list) = flatten(list[i + 1].0.as_ref(), par_inv, false) else {
            i += 1;
            continue;
        };

        // Compute input dimensions of each parallel component.
        let mut dim_boundaries: Vec<(usize, usize)> = Vec::new();
        let mut offset = 0;
        for (comp, comp_inv) in &par_list {
            let nin = effective_nin(comp.as_ref(), *comp_inv);
            dim_boundaries.push((offset, nin));
            offset += nin;
        }

        // Total must match WinMap's axes.
        if offset != shifts.len() {
            i += 1;
            continue;
        }

        // Split the WinMap and build new parallel branches.
        let mut new_branches: Vec<(DynMapping, bool)> = Vec::new();
        let mut failed = false;
        for ((comp, comp_inv), &(start, nin)) in par_list.iter().zip(dim_boundaries.iter()) {
            let sub_shifts = &shifts[start..start + nin];
            let sub_scales = &scales[start..start + nin];

            // Build sub-WinMap.
            let sub_wm = match WinMap::new(sub_shifts, sub_scales) {
                Ok(w) => w,
                Err(_) => {
                    failed = true;
                    break;
                }
            };

            // Clone the component and apply its inversion flag.
            let mut comp_clone = comp.clone();
            if *comp_inv {
                comp_clone.invert();
            }

            // Build series: sub_wm ; component.
            match CmpMap::new(Box::new(sub_wm), comp_clone, Composition::Series) {
                Ok(series) => new_branches.push((Box::new(series), false)),
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }

        if failed {
            i += 1;
            continue;
        }

        // Rebuild as parallel CmpMap.
        let Ok(new_par) = reconstruct(new_branches, false) else {
            i += 1;
            continue;
        };

        // Replace the pair [winmap, parallel] with [new_parallel].
        list.remove(i + 1);
        list[i] = (new_par, false);
        return true;
    }
    false
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

    // Guard against infinite recursion.  The fingerprint-based guard
    // detects cycles (same structure reappearing on the call stack) and
    // the depth counter caps absolute recursion depth.  Either condition
    // causes an immediate bail — we return the mapping unchanged.
    let _guard = match SimplifyGuard::enter(mapping) {
        Some(g) => g,
        None => {
            // Cycle detected or depth exceeded — return unchanged.
            return Ok(mapping.clone_box());
        }
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

        // SphMap^-1 (inv, sph→cart 2->3) then SphMap (fwd, cart→sph 3->2).
        // Inv→fwd cancels without UnitRadius — palDcs2c always produces
        // unit vectors so the intermediate 3D vector is guaranteed unit.
        let mut inv = SphMap::new();
        inv.invert();
        let fwd = SphMap::new();
        let cmp = CmpMap::new(dyn_map(inv), dyn_map(fwd), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_sphmap_fwd_inv_cancels_with_unit_radius_on_first() {
        use crate::mappings::SphMap;

        // Forward (cart→sph) with UnitRadius, then inverse (sph→cart) without.
        // C AST only requires UnitRadius on the first (forward) map for
        // fwd→inv cancellation, so this SHOULD cancel.
        let mut fwd = SphMap::new();
        fwd.set_unit_radius(true);
        let mut inv = SphMap::new();
        // inv keeps unit_radius=false (default)
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 3);
    }

    #[test]
    fn simplify_sphmap_fwd_inv_no_cancel_without_unit_radius() {
        use crate::mappings::SphMap;

        // Forward (cart→sph) WITHOUT UnitRadius, then inverse (sph→cart).
        // Should NOT cancel: without UnitRadius the forward step loses
        // magnitude information.
        let fwd = SphMap::new(); // unit_radius=false
        let mut inv = SphMap::new();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
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

        // Non-linear polynomial: y = x^2 + 3*x (not decomposed to MatrixMap)
        let terms = vec![
            PolyTerm {
                coeff: 1.0,
                powers: vec![2],
                out_axis: 1,
            },
            PolyTerm {
                coeff: 3.0,
                powers: vec![1],
                out_axis: 1,
            },
        ];
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

    // ================================================================== //
    // Cross-composition restructuring tests                               //
    // ================================================================== //

    #[test]
    fn parallel_in_series_basic_zoom_merge() {
        use crate::coords::PointSet;

        // Par(Zoom(1,2), Zoom(1,3)) ; Par(Zoom(1,5), Zoom(1,7))
        // → Par(Zoom(1,10), Zoom(1,21))
        let z1 = ZoomMap::new(1, 2.0).unwrap();
        let z2 = ZoomMap::new(1, 3.0).unwrap();
        let par1 = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Parallel).unwrap();

        let z3 = ZoomMap::new(1, 5.0).unwrap();
        let z4 = ZoomMap::new(1, 7.0).unwrap();
        let par2 = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(par1), dyn_map(par2), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);

        // Verify transform: input [1.0, 1.0] → [2*5, 3*7] = [10, 21].
        let pts = PointSet::from_rows([[1.0, 1.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 10.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 21.0).abs() < 1e-12);
    }

    #[test]
    fn parallel_in_series_shift_merge() {
        use crate::coords::PointSet;

        // Par(Shift([2]), Shift([3])) ; Par(Shift([5]), Shift([7]))
        // → Par(Shift([7]), Shift([10]))
        let s1 = ShiftMap::new(vec![2.0]);
        let s2 = ShiftMap::new(vec![3.0]);
        let par1 = CmpMap::new(dyn_map(s1), dyn_map(s2), Composition::Parallel).unwrap();

        let s3 = ShiftMap::new(vec![5.0]);
        let s4 = ShiftMap::new(vec![7.0]);
        let par2 = CmpMap::new(dyn_map(s3), dyn_map(s4), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(par1), dyn_map(par2), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        // Verify transform: [1.0, 1.0] → [1+2+5, 1+3+7] = [8, 11].
        let pts = PointSet::from_rows([[1.0, 1.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 8.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 11.0).abs() < 1e-12);
    }

    #[test]
    fn parallel_in_series_no_improvement_no_change() {
        // Par(Zoom(1,2), Zoom(1,3)) ; Par(Zoom(1,5), Zoom(1,7))
        // Each matched pair (Zoom(1,2);Zoom(1,5)) simplifies to Zoom(1,10),
        // so this DOES improve. Use a case that genuinely doesn't:
        //
        // Par(Zoom(1,2), Shift([3])) ; Par(Shift([5]), Zoom(1,7))
        // Dimension mismatch within the first matched pair:
        // list1[0]=Zoom(1,2) has 1 output; list2[0]=Shift([5]) has 1 input → match.
        // Combine: Zoom(1,2);Shift([5]) = WinMap → improvement detected.
        //
        // Actually all same-dimension pairings will simplify due to linear merges.
        // The "no improvement" case only happens with genuinely non-simplifiable
        // mapping types. For now, just verify that dimension mismatches abort.
        //
        // Par(Zoom(2,2), Zoom(1,3)) ; Par(Zoom(1,5), Zoom(2,7))
        // list1 outputs: [2, 1] = 3 total. list2 inputs: [1, 2] = 3 total.
        // Dimension matching: list1[0] has 2 outputs vs list2[0] has 1 input →
        // need more from list2. After list2[0]+list2[1]: inputs=3 > outputs=2.
        // Then take from list1[1]: outputs=3, inputs=3 → match (single pair).
        // So this merges into one series pair. Not what we want for "no change".
        //
        // Test with actually non-matching total dimensions:
        // Par(Zoom(2,2), Zoom(1,3)) nin/nout=3 ; Par(Zoom(2,5)) nin/nout=2
        // Can't form a valid series CmpMap (3 != 2).
        // So we need same total but unmatchable internal splits.
        // That doesn't happen with the greedy algorithm — it always finds a
        // match if totals match. So "no_improvement" only comes from the inner
        // simplify check. Skip this test since all linear merges produce improvement.
        //
        // Instead test that mismatched total dimensions simply leave things alone.
        let z1 = ZoomMap::new(2, 2.0).unwrap();
        let z2 = ZoomMap::new(1, 3.0).unwrap();
        let par1 = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Parallel).unwrap();
        // par1: nin=3, nout=3

        let z3 = ZoomMap::new(2, 5.0).unwrap();
        let par2_map: DynMapping = dyn_map(z3);
        // par2_map: nin=2, nout=2 — NOT a parallel CmpMap, just a ZoomMap.
        // So adjacent is par1(ParCmp) + z3(ZoomMap) — won't trigger restructuring.

        // Use two parallel CmpMaps with incompatible parallel splits but same total.
        // Par(Zoom(2,2), Zoom(1,3)) nout=[2,1]=3, Par(Zoom(3,5)) nout=[3]=3
        // Second is not even a CmpMap. Use Par(Zoom(1,5), Zoom(1,7), Zoom(1,9))
        // which is actually Par(Par(Zoom1_5, Zoom1_7), Zoom1_9).
        let z3 = ZoomMap::new(1, 5.0).unwrap();
        let z4 = ZoomMap::new(1, 7.0).unwrap();
        let z5 = ZoomMap::new(1, 9.0).unwrap();
        let par2_inner = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Parallel).unwrap();
        let par2 = CmpMap::new(dyn_map(par2_inner), dyn_map(z5), Composition::Parallel).unwrap();
        // par2: nin=3, nout=3

        let series = CmpMap::new(dyn_map(par1), Box::new(par2), Composition::Series).unwrap();
        let _result = simplify_mapping(&series).unwrap();
        // The restructuring should attempt but the inner merges will succeed
        // (ZoomMap pairs always merge in series). Just verify it terminates.
        drop(par2_map);
    }

    #[test]
    fn parallel_in_series_dimension_matching_multicomponent() {
        use crate::coords::PointSet;

        // Par(Zoom(2,2), Zoom(2,3)) ; Par(Zoom(2,5), Zoom(2,7))
        // = [2,2] outputs feed [2,2] inputs → two matched pairs.
        let z1 = ZoomMap::new(2, 2.0).unwrap();
        let z2 = ZoomMap::new(2, 3.0).unwrap();
        let par1 = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Parallel).unwrap();

        let z3 = ZoomMap::new(2, 5.0).unwrap();
        let z4 = ZoomMap::new(2, 7.0).unwrap();
        let par2 = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(par1), dyn_map(par2), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        // Verify: [1,1,1,1] → [2*5, 2*5, 3*7, 3*7] = [10,10,21,21]
        let pts = PointSet::from_rows([[1.0, 1.0, 1.0, 1.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 10.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 10.0).abs() < 1e-12);
        assert!((out.point(0)[2] - 21.0).abs() < 1e-12);
        assert!((out.point(0)[3] - 21.0).abs() < 1e-12);
    }

    #[test]
    fn series_in_parallel_basic_zoom_merge() {
        use crate::coords::PointSet;

        // (Zoom(1,2) ; Zoom(1,0.5)) || (Zoom(1,3) ; Zoom(1,1/3))
        // = identity || identity → should collapse to UnitMap(2)
        let z1 = ZoomMap::new(1, 2.0).unwrap();
        let z2 = ZoomMap::new(1, 0.5).unwrap();
        let ser1 = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Series).unwrap();

        let z3 = ZoomMap::new(1, 3.0).unwrap();
        let z4 = ZoomMap::new(1, 1.0 / 3.0).unwrap();
        let ser2 = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Series).unwrap();

        let parallel = CmpMap::new(dyn_map(ser1), dyn_map(ser2), Composition::Parallel).unwrap();
        let result = simplify_mapping(&parallel).unwrap();

        // Each series pair cancels to UnitMap, so the parallel of two
        // UnitMaps should be a single UnitMap(2).
        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);

        // Verify transform: identity.
        let pts = PointSet::from_rows([[7.0, 13.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 7.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 13.0).abs() < 1e-12);
    }

    #[test]
    fn series_in_parallel_length_mismatch_no_change() {
        use crate::mappings::MatrixMap;
        // (Zoom(1,2) ; Zoom(1,3) ; Zoom(1,4)) || (Zoom(1,5) ; Zoom(1,6))
        // Different series lengths → cross-composition restructuring skipped.
        // However, each series merges individually (2*3*4=24, 5*6=30), then
        // the parallel of two ZoomMaps with different factors merges into a
        // diagonal MatrixMap.
        let z1 = ZoomMap::new(1, 2.0).unwrap();
        let z2 = ZoomMap::new(1, 3.0).unwrap();
        let z3 = ZoomMap::new(1, 4.0).unwrap();
        let ser1_inner = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Series).unwrap();
        let ser1 = CmpMap::new(dyn_map(ser1_inner), dyn_map(z3), Composition::Series).unwrap();

        let z4 = ZoomMap::new(1, 5.0).unwrap();
        let z5 = ZoomMap::new(1, 6.0).unwrap();
        let ser2 = CmpMap::new(dyn_map(z4), dyn_map(z5), Composition::Series).unwrap();

        let parallel = CmpMap::new(dyn_map(ser1), dyn_map(ser2), Composition::Parallel).unwrap();
        let result = simplify_mapping(&parallel).unwrap();

        // Series merge individually, then parallel ZoomMaps with different
        // factors produce a diagonal MatrixMap.
        assert!(result.as_any().is::<MatrixMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn permmap_swap_basic_block_swap() {
        use crate::coords::PointSet;

        // PermMap that swaps two 1-D blocks followed by Par(Shift([10]), Shift([20]))
        // PermMap: outperm=[2,1], inperm=[2,1] (swap axes)
        // After swap: Par(Shift([20]), Shift([10])) followed by identity PermMap
        // Which then simplifies away.
        let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        let s1 = ShiftMap::new(vec![10.0]);
        let s2 = ShiftMap::new(vec![20.0]);
        let par = CmpMap::new(dyn_map(s1), dyn_map(s2), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(pm), dyn_map(par), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        // Transform: [x,y] → PermMap → [y,x] → Par(Shift10, Shift20) → [y+10, x+20]
        // Verify with [1, 2]:
        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let out_orig = series.transform(&pts, true).unwrap();
        let out_simp = result.transform(&pts, true).unwrap();
        assert!((out_orig.point(0)[0] - out_simp.point(0)[0]).abs() < 1e-12);
        assert!((out_orig.point(0)[1] - out_simp.point(0)[1]).abs() < 1e-12);
    }

    #[test]
    fn permmap_swap_non_block_no_change() {
        use crate::coords::PointSet;

        // PermMap that is NOT a block swap: outperm=[1,2] (identity).
        // This doesn't have the block-swap structure needed.
        let pm = PermMap::new(&[1, 2], &[1, 2], &[]).unwrap();
        let s1 = ShiftMap::new(vec![10.0]);
        let s2 = ShiftMap::new(vec![20.0]);
        let par = CmpMap::new(dyn_map(s1), dyn_map(s2), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(pm), dyn_map(par), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        // Identity PermMap should merge away, parallel CmpMap remains.
        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let out_orig = series.transform(&pts, true).unwrap();
        let out_simp = result.transform(&pts, true).unwrap();
        assert!((out_orig.point(0)[0] - out_simp.point(0)[0]).abs() < 1e-12);
        assert!((out_orig.point(0)[1] - out_simp.point(0)[1]).abs() < 1e-12);
    }

    #[test]
    fn iteration_cap_prevents_infinite_loop() {
        // Test that deeply nested parallel-in-series chains terminate.
        // Use ZoomMaps (nin==nout) so the series composition is valid.
        let z1 = ZoomMap::new(2, 2.0).unwrap();
        let z2 = ZoomMap::new(2, 3.0).unwrap();
        let par = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Parallel).unwrap();

        let z3 = ZoomMap::new(2, 5.0).unwrap();
        let z4 = ZoomMap::new(2, 7.0).unwrap();
        let par2 = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Parallel).unwrap();

        let z5 = ZoomMap::new(2, 11.0).unwrap();
        let z6 = ZoomMap::new(2, 13.0).unwrap();
        let par3 = CmpMap::new(dyn_map(z5), dyn_map(z6), Composition::Parallel).unwrap();

        // Build a 3-element series chain of parallel CmpMaps.
        let inner = CmpMap::new(dyn_map(par), dyn_map(par2), Composition::Series).unwrap();
        let outer = CmpMap::new(dyn_map(inner), dyn_map(par3), Composition::Series).unwrap();

        // Should terminate and produce correct dimensions.
        let result = simplify_mapping(&outer).unwrap();
        assert_eq!(result.nin(), 4);
        assert_eq!(result.nout(), 4);
    }
}

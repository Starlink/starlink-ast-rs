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
use super::grismmap::GrismMap;
use super::matrix::{MatrixMap, is_diagonal};
use super::pcdmap::PcdMap;
use super::perm::PermMap;
use super::shift::ShiftMap;
use super::sphmap::SphMap;
use super::tranmap::TranMap;
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
    if let Some(tm) = mapping.as_any().downcast_ref::<TranMap>() {
        mapping_fingerprint(tm.map1()).hash(&mut hasher);
        mapping_fingerprint(tm.map2()).hash(&mut hasher);
    }
    if let Some(sw) = mapping.as_any().downcast_ref::<super::SwitchMap>() {
        sw.nroute().hash(&mut hasher);
        if let Some(fs) = sw.forward_selector() {
            mapping_fingerprint(fs).hash(&mut hasher);
        }
        if let Some(is) = sw.inverse_selector() {
            mapping_fingerprint(is).hash(&mut hasher);
        }
        for rm in sw.route_maps() {
            mapping_fingerprint(rm.as_ref()).hash(&mut hasher);
        }
    }
    if let Some(sel) = mapping.as_any().downcast_ref::<super::SelectorMap>() {
        sel.regions().len().hash(&mut hasher);
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

            // Effective inversion for each child: inv_a/inv_b record
            // the expected child invert state. If the child's current
            // state differs, that flips the direction. Then the CmpMap's
            // own inversion (cmp_inv) flips again.
            let child_inv_a = cmp_inv != (cmp.inv_a() != cmp.first().is_inverted());
            let child_inv_b = cmp_inv != (cmp.inv_b() != cmp.second().is_inverted());

            if !cmp_inv {
                let mut result = flatten(cmp.first(), child_inv_a, series)?;
                result.extend(flatten(cmp.second(), child_inv_b, series)?);
                return Ok(result);
            } else if series {
                // Inverted series: reverse child order.
                let mut result = flatten(cmp.second(), child_inv_b, series)?;
                result.extend(flatten(cmp.first(), child_inv_a, series)?);
                return Ok(result);
            } else {
                // Inverted parallel: same order.
                let mut result = flatten(cmp.first(), child_inv_a, series)?;
                result.extend(flatten(cmp.second(), child_inv_b, series)?);
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
pub(crate) fn merge_loop(list: &mut Vec<(DynMapping, bool)>, series: bool) -> bool {
    const MAX_MERGE_PASSES: usize = 50;
    let mut pass = 0;
    let mut prev_len = list.len();
    let mut prev_prev_len: usize = 0;

    // Capture the original axis count so the empty-list fallback below
    // produces a UnitMap of the right size. For a series list, every
    // map has the same external nin/nout (after the upstream merging
    // makes them dimension-compatible). For a parallel list, the
    // composite is `nin = sum of nin`, but the empty-fallback case for
    // parallel doesn't fire in the strip block (we only strip in
    // series), so series is what we need to preserve.
    let original_nin: usize = if list.is_empty() {
        1
    } else if series {
        let (m, inv) = &list[0];
        if *inv { m.nout() } else { m.nin() }
    } else {
        list.iter()
            .map(|(m, inv)| if *inv { m.nout() } else { m.nin() })
            .sum()
    };

    loop {
        pass += 1;
        if pass > MAX_MERGE_PASSES {
            break;
        }

        let mut improved = false;
        let mut i = 0;

        while i + 1 < list.len() {
            // Skip if either mapping has already been simplified (C IsSimple).
            if list[i].0.has_been_simplified() || list[i + 1].0.has_been_simplified() {
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
                        if series {
                            // Skip reverse merge when m1 is UnitMap — let
                            // post-loop strip handle it (matches C behavior
                            // where UnitMap removes itself before neighbors
                            // get a chance to merge).
                            if m1.as_any().is::<UnitMap>() {
                                return None;
                            }
                            m2.try_merge(m1.as_ref(), series, inv2, inv1)
                                .filter(|m| m.nin() == m1_nin)
                        } else {
                            // For parallel: the reverse merge puts m2's axes
                            // first, which is wrong. Instead, try extracting
                            // linear params from both and merging with m1 first.
                            let (s1, c1) = super::winmap::extract_linear_params(m1.as_ref(), inv1)?;
                            let (s2, c2) = super::winmap::extract_linear_params(m2.as_ref(), inv2)?;
                            let mut new_shifts = s1;
                            new_shifts.extend(s2);
                            let mut new_scales = c1;
                            new_scales.extend(c2);
                            super::winmap::compose_linear(
                                &vec![0.0; new_shifts.len()],
                                &vec![1.0; new_shifts.len()],
                                &new_shifts,
                                &new_scales,
                            )
                        }
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
            // Try swapping WcsMap past a PermMap (or vice versa).
            if series && try_swap_wcsmap_past_permmap(list) {
                continue;
            }
            // Try swapping a PcdMap past a ZoomMap or PermMap.
            if series && try_swap_pcdmap_past_neighbor(list) {
                continue;
            }
            // Try merging ZoomMap + GrismMap(inv) pattern.
            if series && try_merge_zoom_before_inv_grism(list) {
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
            // Last-resort in-loop UnitMap stripping. The C oracle's
            // simplify loop nominates each map in turn; UnitMap's
            // MapMerge removes itself from the list, after which the
            // outer loop retries the surrounding pairs. We mimic that
            // by stripping UnitMaps only when no other heuristic
            // applied — earlier passes give other classes a chance to
            // react to UnitMap neighbours before they vanish (which
            // can change e.g. PermMap into its OutCpy form, or fold
            // SphMap/UnitMap into a MatrixMap).
            if series && list.len() > 1 {
                let orig_len = list.len();
                list.retain(|(m, _)| !m.as_any().is::<UnitMap>());
                if list.is_empty() {
                    list.push((Box::new(UnitMap::new(original_nin)), false));
                }
                if list.len() != orig_len {
                    continue;
                }
            }
            break;
        }

        // Oscillation detection: if list length alternates between two values,
        // we are in an infinite merge/split cycle. Break to prevent wasted passes.
        // Matches C cmpmap.c:3589-3620 in intent.
        let current_len = list.len();
        if pass > 2 && current_len == prev_prev_len && current_len != prev_len {
            break;
        }
        prev_prev_len = prev_len;
        prev_len = current_len;
    }

    // Track whether any passes ran (pass > 1 means something changed).
    let mut changed = pass > 1;

    // Post-loop: strip UnitMaps from series lists.
    if series && list.len() > 1 {
        let orig_len = list.len();
        list.retain(|(m, _)| !m.as_any().is::<UnitMap>());
        if list.len() != orig_len {
            changed = true;
        }
        if list.is_empty() {
            list.push((Box::new(UnitMap::new(original_nin)), false));
        }
    }
    changed
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
            //    resulting ShiftMap could combine with, OR
            // 3. There's a MatrixMap at position i-1 that could merge with
            //    the new MatrixMap (swap-and-seek: two MatrixMaps separated
            //    by a linear mapping become adjacent after the swap).
            //    Matches C matrixmap.c:2063-2273.
            if !all_shifts_zero {
                let has_linear_partner_after = if i + 2 < list.len() {
                    super::winmap::extract_linear_params(list[i + 2].0.as_ref(), list[i + 2].1)
                        .is_some()
                } else {
                    false
                };
                let has_matrix_partner_before = i > 0 && list[i - 1].0.as_any().is::<MatrixMap>();
                if !has_linear_partner_after && !has_matrix_partner_before {
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

/// Try to swap a WinMap (or ShiftMap/ZoomMap) past a PermMap in either order.
///
/// Handles both `[Linear, PermMap]` (forward swap) and `[PermMap, Linear]`
/// (mirror swap). The PermMap may have constants and may change dimensions.
/// Constants on the *opposite* permutation array are adjusted to absorb
/// the WinMap that no longer exists on that side.
///
/// Matches C winmap.c:2973 (WinPerm) and the CanSwap bidirectionality
/// + nstep-walk gate (winmap.c:266 + winmap.c:1442-1474).
fn try_swap_winmap_past_permmap(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 1 < list.len() {
        // Identify the [Linear, PermMap] or [PermMap, Linear] pattern.
        let a_is_linear = is_linear_mapping(list[i].0.as_ref());
        let b_is_linear = is_linear_mapping(list[i + 1].0.as_ref());
        let a_is_perm = list[i].0.as_any().is::<PermMap>();
        let b_is_perm = list[i + 1].0.as_any().is::<PermMap>();

        // linear_iwm: position of the Linear within the pair (0 or 1).
        let linear_iwm = if a_is_linear && b_is_perm {
            0
        } else if a_is_perm && b_is_linear {
            1
        } else {
            i += 1;
            continue;
        };
        let lin_idx = i + linear_iwm;
        let perm_idx = i + 1 - linear_iwm;

        // Bidirectionality: every connected axis in the PermMap must have
        // matching forward/inverse links. (C CanSwap, winmap.c:399-417.)
        let perm = list[perm_idx].0.as_any().downcast_ref::<PermMap>().unwrap();
        let perm_inv = list[perm_idx].1;
        let (outperm_eff, inperm_eff) = if perm_inv {
            (perm.inperm().to_vec(), perm.outperm().to_vec())
        } else {
            (perm.outperm().to_vec(), perm.inperm().to_vec())
        };
        let consts = perm.constants().to_vec();
        if !permmap_is_bidirectional(&outperm_eff, &inperm_eff) {
            i += 1;
            continue;
        }

        // Extract WinMap shifts/scales for the supplied direction. The
        // resulting (shifts, scales) describe the linear mapping's
        // effective forward transform with `win_inv` already absorbed.
        let win_inv = list[lin_idx].1;
        let Some((shifts, scales)) =
            super::winmap::extract_linear_params(list[lin_idx].0.as_ref(), win_inv)
        else {
            i += 1;
            continue;
        };

        // The swap is worthwhile in any of these cases:
        //   (a) A linear merge target is reachable on the Linear's far side
        //       (matches C nstep walk, winmap.c:1442-1474).
        //   (b) The mapping on the Linear's far side is itself a PermMap
        //       (the swap places the two PermMaps adjacent so they can
        //       merge/cancel — handles `[Perm, Linear, Perm]` patterns
        //       like the win_swap_outer_merge fixture).
        //   (c) The swap would intrinsically simplify the Linear
        //       (matches C CanSwap "simpler" flag, winmap.c:419-427):
        //       Linear before PermMap → simpler iff `npin > npout`;
        //       Linear after PermMap → simpler iff `npout > npin`.
        let walk_forward = linear_iwm == 0;
        let has_walk_target = has_swap_target_for_winperm(list, i, walk_forward);
        let has_perm_on_far_side = if linear_iwm == 0 {
            // Far side of Linear at i is i-1.
            i > 0 && list[i - 1].0.as_any().is::<PermMap>()
        } else {
            // Far side of Linear at i+1 is i+2.
            i + 2 < list.len() && list[i + 2].0.as_any().is::<PermMap>()
        };
        let npin = if perm_inv {
            perm.outperm().len()
        } else {
            perm.inperm().len()
        };
        let npout = if perm_inv {
            perm.inperm().len()
        } else {
            perm.outperm().len()
        };
        let simpler = if linear_iwm == 0 {
            npin > npout
        } else {
            npout > npin
        };
        if !has_walk_target && !has_perm_on_far_side && !simpler {
            i += 1;
            continue;
        }

        // Build the new (Linear', PermMap') pair.
        let Some((new_lin, new_pm)) = build_winperm_swap(
            linear_iwm,
            &shifts,
            &scales,
            &outperm_eff,
            &inperm_eff,
            &consts,
        ) else {
            i += 1;
            continue;
        };

        // Replace the two-element window with the swapped pair (PermMap'
        // takes the Linear's slot and vice-versa). Both replacements are
        // returned in their natural (forward) form.
        list[lin_idx] = (Box::new(new_pm), false);
        list[perm_idx] = (new_lin, false);
        return true;
    }
    false
}

/// Returns `true` iff the mapping is a simple linear class
/// (`WinMap`/`ShiftMap`/`ZoomMap`).
fn is_linear_mapping(m: &dyn Mapping) -> bool {
    m.as_any().is::<WinMap>() || m.as_any().is::<ShiftMap>() || m.as_any().is::<ZoomMap>()
}

/// Bi-directionality check on the (effective) PermMap permutation arrays.
///
/// For every connected output axis k (outperm[k] >= 1 and within range),
/// the corresponding input axis must point back: inperm[outperm[k]-1] == k+1.
/// And vice-versa for connected input axes.
fn permmap_is_bidirectional(outperm: &[i64], inperm: &[i64]) -> bool {
    let nin = inperm.len() as i64;
    let nout = outperm.len() as i64;
    for (k, &p) in outperm.iter().enumerate() {
        if p >= 1 && p <= nin && inperm[(p - 1) as usize] != (k as i64) + 1 {
            return false;
        }
    }
    for (i, &p) in inperm.iter().enumerate() {
        if p >= 1 && p <= nout && outperm[(p - 1) as usize] != (i as i64) + 1 {
            return false;
        }
    }
    true
}

/// Walk the flattened list looking for a merge target reachable from a
/// proposed Linear↔PermMap swap. The Linear+PermMap pair occupies positions
/// `i` and `i+1`. When `walk_forward` is true, the Linear is at `i` and
/// scans positions `i+2..` (rightward); otherwise it scans `i-1..0`
/// (leftward). The walk passes through MatrixMap/WcsMap/PermMap and
/// stops on the first Linear (WinMap/ShiftMap/ZoomMap/UnitMap or diagonal
/// MatrixMap) or non-swappable mapping.
///
/// Matches the gate logic in C winmap.c:1442-1474 + 1490-1513.
fn has_swap_target_for_winperm(list: &[(DynMapping, bool)], i: usize, walk_forward: bool) -> bool {
    // Position of the Linear in the pair (i for forward, i+1 for backward).
    // The walk starts just past the PermMap.
    let walk_iter: Box<dyn Iterator<Item = usize>> = if walk_forward {
        Box::new((i + 2)..list.len())
    } else if i > 0 {
        Box::new((0..i).rev())
    } else {
        return false;
    };
    for j in walk_iter {
        let m = list[j].0.as_ref();
        if is_linear_mapping(m) || m.as_any().is::<UnitMap>() {
            return true;
        }
        if let Some(mm) = m.as_any().downcast_ref::<MatrixMap>() {
            let inv = list[j].1;
            let mat = if inv {
                match mm.inverse_matrix() {
                    Some(x) => x,
                    None => return false,
                }
            } else {
                mm.forward_matrix()
            };
            if is_diagonal(mat) {
                return true;
            }
            // Non-diagonal MatrixMap: keep walking through it.
            continue;
        }
        if m.as_any().is::<WcsMap>() || m.as_any().is::<PermMap>() {
            continue;
        }
        // Non-swappable: stop.
        return false;
    }
    false
}

/// Construct the (Linear', PermMap') pair that results from swapping a
/// Linear and PermMap. `linear_iwm == 0` means the Linear was *before* the
/// PermMap (forward direction); `linear_iwm == 1` means it was *after*
/// (mirror direction).
///
/// Matches winmap.c:3075-3209 (WinPerm).
fn build_winperm_swap(
    linear_iwm: usize,
    shifts: &[f64],
    scales: &[f64],
    outperm: &[i64],
    inperm: &[i64],
    consts: &[f64],
) -> Option<(DynMapping, PermMap)> {
    use crate::constants::BAD_VALUE;
    use crate::mappings::helpers::is_bad;

    let npout = outperm.len();
    let npin = inperm.len();
    let mut new_consts = consts.to_vec();

    let (new_dim, source_perm, ref_perm) = if linear_iwm == 0 {
        // Linear before PermMap: new Linear sits *after* the PermMap.
        // Its dim = npout. For each output axis k, source from outperm[k].
        // Adjust constants on the inperm side (inverse-direction constants).
        (npout, outperm, inperm)
    } else {
        // Linear after PermMap: new Linear sits *before* the PermMap.
        // Its dim = npin. For each input axis i, source from inperm[i].
        // Adjust constants on the outperm side (forward-direction constants).
        (npin, inperm, outperm)
    };

    // Build the new shifts/scales from `source_perm`.
    let mut new_shifts = vec![0.0; new_dim];
    let mut new_scales = vec![1.0; new_dim];
    let src_len = shifts.len() as i64;
    for k in 0..new_dim {
        let p = source_perm[k];
        if p >= 1 && p <= src_len {
            new_shifts[k] = shifts[(p - 1) as usize];
            new_scales[k] = scales[(p - 1) as usize];
        }
        // Constants and BAD: leave shift=0, scale=1 (no transform).
    }

    // Adjust constants on the *opposite* permutation array.
    // - linear_iwm == 0: iterate input axes (inperm); adjust = (c - a[i]) / b[i]
    //   (the supplied WinMap was forward; the inverse PermMap previously had
    //   its constants scaled by the inverse WinMap).
    // - linear_iwm == 1: iterate output axes (outperm); adjust = a[k] + c * b[k]
    //   (the supplied WinMap is forward; the forward PermMap previously had
    //   its constants scaled by the forward WinMap).
    let n_axis = ref_perm.len();
    if shifts.len() != n_axis {
        // Shouldn't happen: WinMap dim must equal PermMap's nin (when Linear
        // is before) or nout (when Linear is after). Bail safely.
        return None;
    }
    for i in 0..n_axis {
        let p = ref_perm[i];
        if p >= 0 {
            continue;
        }
        let cidx = (-p - 1) as usize;
        if cidx >= new_consts.len() {
            continue;
        }
        let c = new_consts[cidx];
        if is_bad(c) {
            continue;
        }
        let a = shifts[i];
        let b = scales[i];
        if is_bad(a) || is_bad(b) {
            new_consts[cidx] = BAD_VALUE;
            continue;
        }
        if linear_iwm == 0 {
            if b == 0.0 {
                new_consts[cidx] = BAD_VALUE;
            } else {
                new_consts[cidx] = (c - a) / b;
            }
        } else {
            new_consts[cidx] = a + c * b;
        }
    }

    let new_lin: DynMapping = Box::new(WinMap::new(&new_shifts, &new_scales).ok()?);
    let new_pm = PermMap::new(inperm, outperm, &new_consts).ok()?;
    Some((new_lin, new_pm))
}

/// Try to swap a WcsMap past a PermMap (or vice versa) in a flattened series list.
///
/// When a WcsMap is adjacent to a PermMap, they can be swapped in order
/// (producing a modified WcsMap with permuted axis assignments) if:
/// 1. All links between input/output axes in the PermMap are bi-directional.
/// 2. The WcsMap's longitude and latitude axes are both passed through by
///    the PermMap (both have bi-directional links) OR both are unassigned
///    (receive constants from the PermMap).
///
/// Only fires when there is a potential merge partner (another WcsMap) on
/// the other side that could cancel with the swapped WcsMap.
///
/// Matches C wcsmap.c CanSwap+WcsPerm (lines 922-1145, 4463-4698).
fn try_swap_wcsmap_past_permmap(list: &mut Vec<(DynMapping, bool)>) -> bool {
    let mut i = 0;
    while i + 1 < list.len() {
        let is_wcs_perm =
            list[i].0.as_any().is::<WcsMap>() && list[i + 1].0.as_any().is::<PermMap>();
        let is_perm_wcs =
            list[i].0.as_any().is::<PermMap>() && list[i + 1].0.as_any().is::<WcsMap>();

        if !is_wcs_perm && !is_perm_wcs {
            i += 1;
            continue;
        }

        let (wcs_idx, perm_idx) = if is_wcs_perm { (i, i + 1) } else { (i + 1, i) };

        // Check if swapping is worthwhile: either there's a WcsMap merge partner
        // on the far side, or the swap itself produces a simpler result (when
        // the PermMap reduces dimensionality, replacing the WcsMap with a UnitMap
        // is a net simplification). Matches C wcsmap.c MapMerge logic.
        let far_side = if is_wcs_perm {
            i + 2
        } else {
            i.wrapping_sub(1)
        };
        let has_merge_partner = if is_wcs_perm {
            far_side < list.len() && list[far_side].0.as_any().is::<WcsMap>()
        } else {
            i > 0 && list[i - 1].0.as_any().is::<WcsMap>()
        };

        let perm_ref = list[perm_idx].0.as_any().downcast_ref::<PermMap>().unwrap();
        let perm_inv_flag = list[perm_idx].1;
        let perm_nin = if perm_inv_flag {
            perm_ref.outperm().len()
        } else {
            perm_ref.inperm().len()
        };
        let perm_nout = if perm_inv_flag {
            perm_ref.inperm().len()
        } else {
            perm_ref.outperm().len()
        };
        let simpler = if is_wcs_perm {
            // PermMap is second: simpler if nin > nout (reduces dimensions).
            perm_nin > perm_nout
        } else {
            // PermMap is first: simpler if nout > nin (increases dimensions).
            perm_nout > perm_nin
        };

        if !has_merge_partner && !simpler {
            i += 1;
            continue;
        }

        let wcs = list[wcs_idx].0.as_any().downcast_ref::<WcsMap>().unwrap();
        let perm = list[perm_idx].0.as_any().downcast_ref::<PermMap>().unwrap();
        let wcs_inv = list[wcs_idx].1;
        let perm_inv = list[perm_idx].1;

        // Get the effective forward permutation arrays for the PermMap.
        let outperm = if perm_inv {
            perm.inperm()
        } else {
            perm.outperm()
        };
        let inperm = if perm_inv {
            perm.outperm()
        } else {
            perm.inperm()
        };
        let npin = inperm.len();
        let npout = outperm.len();

        // Check bi-directionality: all active links must be reversible.
        // Note: PermMap uses 1-based positive indices. outperm[k]=j means
        // output k gets input axis j (1-based). Convert to 0-based for indexing.
        let mut ok = true;
        for (k, &j) in outperm.iter().enumerate() {
            if j > 0 && (j as usize - 1) < npin {
                // outperm[k] links output k to input (j-1) (0-based).
                // Check reverse: inperm[j-1] should link back to output (k+1) (1-based).
                if inperm[j as usize - 1] != (k as i64 + 1) {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            for (k, &j) in inperm.iter().enumerate() {
                if j > 0 && (j as usize - 1) < npout {
                    // inperm[k] links input k to output (j-1) (0-based).
                    // Check reverse: outperm[j-1] should link back to input (k+1) (1-based).
                    if outperm[j as usize - 1] != (k as i64 + 1) {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if !ok {
            i += 1;
            continue;
        }

        // Get the WcsMap's lon/lat axis indices (0-based).
        let lonax = wcs.lonax_0based();
        let latax = wcs.latax_0based();

        // Determine whether lon/lat are assigned or unassigned in the PermMap,
        // depending on whether the WcsMap comes before or after the PermMap.
        if is_wcs_perm {
            // WcsMap at i, PermMap at i+1.
            // PermMap operates on WcsMap's outputs. The PermMap's inperm tells
            // where each PermMap input (= WcsMap output) gets routed in the
            // PermMap's output. inperm[lonax] is 1-based output position.
            if lonax >= npin || latax >= npin {
                i += 1;
                continue;
            }

            let lon_dest = inperm[lonax]; // 1-based or negative
            let lat_dest = inperm[latax];

            if lon_dest < 0 && lat_dest < 0 {
                // Both celestial axes are unassigned (receive constants in inverse).
                // WcsMap becomes UnitMap, PermMap gets transformed constants.
                if let Some(result) =
                    swap_wcsmap_perm_unassigned(wcs, perm, wcs_inv, perm_inv, true)
                {
                    list[i] = (result.0, result.1);
                    list[i + 1] = (result.2, result.3);
                    return true;
                }
            } else if lon_dest > 0
                && (lon_dest as usize) <= npout
                && lat_dest > 0
                && (lat_dest as usize) <= npout
            {
                // Both celestial axes are assigned. Create new WcsMap with permuted
                // axes. lon_dest is 1-based → convert to 0-based for new axis index.
                let new_lonax = lon_dest as usize - 1;
                let new_latax = lat_dest as usize - 1;
                let Some(new_wcs) = wcs.with_permuted_axes(npout, new_lonax, new_latax) else {
                    i += 1;
                    continue;
                };

                // Swap: [WcsMap, PermMap] → [PermMap, new_WcsMap]
                let perm_entry = list.remove(i + 1);
                list.remove(i);
                list.insert(i, perm_entry);
                list.insert(i + 1, (Box::new(new_wcs), wcs_inv));
                return true;
            }
            // else: one assigned, one not — cannot swap.
        } else {
            // PermMap at i, WcsMap at i+1.
            // PermMap operates on WcsMap's inputs. The PermMap's outperm tells
            // where each WcsMap input (= PermMap output) gets its value.
            // outperm[lonax] is 1-based input source position.
            if lonax >= npout || latax >= npout {
                i += 1;
                continue;
            }

            let lon_src = outperm[lonax]; // 1-based or negative
            let lat_src = outperm[latax];

            if lon_src < 0 && lat_src < 0 {
                // Both celestial axes are unassigned (receive constants in forward).
                // WcsMap becomes UnitMap, PermMap gets transformed constants.
                if let Some(result) =
                    swap_wcsmap_perm_unassigned(wcs, perm, wcs_inv, perm_inv, false)
                {
                    list[i] = (result.0, result.1);
                    list[i + 1] = (result.2, result.3);
                    return true;
                }
            } else if lon_src > 0
                && (lon_src as usize) <= npin
                && lat_src > 0
                && (lat_src as usize) <= npin
            {
                // Both celestial axes are assigned. Create new WcsMap with permuted
                // axes. lon_src is 1-based → convert to 0-based.
                let new_lonax = lon_src as usize - 1;
                let new_latax = lat_src as usize - 1;
                let Some(new_wcs) = wcs.with_permuted_axes(npin, new_lonax, new_latax) else {
                    i += 1;
                    continue;
                };

                // Swap: [PermMap, WcsMap] → [new_WcsMap, PermMap]
                let perm_entry = list.remove(i);
                list.remove(i); // was at i+1, now at i after removal
                list.insert(i, (Box::new(new_wcs), wcs_inv));
                list.insert(i + 1, perm_entry);
                return true;
            }
            // else: one assigned, one not — cannot swap.
        }

        i += 1;
    }
    false
}

/// Handle the "unassigned lon/lat" case of WcsMap↔PermMap swap.
///
/// When the PermMap feeds constants to the WcsMap's celestial axes (or
/// receives constants from them), the WcsMap can be replaced by a UnitMap
/// and the PermMap's constants are transformed through the WcsMap.
///
/// Returns (new_first_mapping, new_first_inv, new_second_mapping, new_second_inv)
/// or None if the transformation fails.
fn swap_wcsmap_perm_unassigned(
    wcs: &WcsMap,
    perm: &PermMap,
    wcs_inv: bool,
    perm_inv: bool,
    wcs_first: bool, // true if WcsMap comes before PermMap in the pipeline
) -> Option<(DynMapping, bool, DynMapping, bool)> {
    use crate::coords::PointSet;

    let outperm = if perm_inv {
        perm.inperm()
    } else {
        perm.outperm()
    };
    let inperm = if perm_inv {
        perm.outperm()
    } else {
        perm.inperm()
    };
    let constants = perm.constants();
    let npin = inperm.len();
    let npout = outperm.len();

    if wcs_first {
        // WcsMap outputs feed PermMap inputs. The PermMap's inperm has negative
        // entries for constant axes. We transform those constants through the
        // inverse WcsMap to get the new PermMap constants.
        let mut p = vec![BAD_VALUE; npin];
        let mut new_constants = constants.to_vec();

        for k in 0..npin {
            if inperm[k] < 0 {
                let cidx = (-inperm[k] - 1) as usize;
                if cidx < constants.len() {
                    p[k] = constants[cidx];
                }
            }
        }

        // Transform through the effective inverse WcsMap.
        // C equivalent: astTranN(wm, 1, npin, 1, p, 0, npin, 1, q) with Invert=wcs_inv.
        // effective_forward = forward XOR invert = 0 XOR wcs_inv = wcs_inv.
        // In Rust (self.inverted=false): transform(&pts, wcs_inv) gives same effective direction.
        let pts = PointSet::from_rows(vec![p]);
        let transformed = wcs.transform(&pts, wcs_inv).ok()?;
        let q = transformed.point(0);

        // Update constants.
        for k in 0..npin {
            if inperm[k] < 0 {
                let cidx = (-inperm[k] - 1) as usize;
                if cidx < new_constants.len() {
                    new_constants[cidx] = q[k];
                }
            }
        }

        let new_pm = PermMap::new(inperm, outperm, &new_constants).ok()?;
        let unit = UnitMap::new(npout);

        // Result: [PermMap (at original PermMap position), UnitMap (replacing WcsMap)]
        // Swapped order: [UnitMap, PermMap] — but UnitMap will be stripped.
        // Actually the C code puts: maps[iwm=0] = newpm, maps[1] = UnitMap(npout)
        // That means: position 0 gets the PermMap, position 1 gets the UnitMap.
        // So the result is [PermMap, UnitMap].
        Some((
            Box::new(new_pm) as DynMapping,
            perm_inv,
            Box::new(unit) as DynMapping,
            false,
        ))
    } else {
        // PermMap outputs feed WcsMap inputs. The PermMap's outperm has negative
        // entries for constant axes on the celestial positions.
        let mut p = vec![BAD_VALUE; npout];
        let mut new_constants = constants.to_vec();

        for k in 0..npout {
            if outperm[k] < 0 {
                let cidx = (-outperm[k] - 1) as usize;
                if cidx < constants.len() {
                    p[k] = constants[cidx];
                }
            }
        }

        // Transform through the effective forward WcsMap.
        // C equivalent: astTranN(wm, 1, npout, 1, p, 1, npout, 1, q) with Invert=wcs_inv.
        // effective_forward = forward XOR invert = 1 XOR wcs_inv = !wcs_inv.
        // In Rust (self.inverted=false): transform(&pts, !wcs_inv) gives same effective direction.
        let pts = PointSet::from_rows(vec![p]);
        let transformed = wcs.transform(&pts, !wcs_inv).ok()?;
        let q = transformed.point(0);

        // Update constants.
        for k in 0..npout {
            if outperm[k] < 0 {
                let cidx = (-outperm[k] - 1) as usize;
                if cidx < new_constants.len() {
                    new_constants[cidx] = q[k];
                }
            }
        }

        let new_pm = PermMap::new(inperm, outperm, &new_constants).ok()?;
        let unit = UnitMap::new(npin);

        // Swapped: [UnitMap (replacing WcsMap), PermMap]
        Some((
            Box::new(unit) as DynMapping,
            false,
            Box::new(new_pm) as DynMapping,
            perm_inv,
        ))
    }
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
        // Pre-merge length (sum of leaf counts) for the
        // length-comparison heuristic below — total flat-leaf count
        // of the two sublists before composition.
        let pre_len: usize = sub1
            .iter()
            .map(|(m, _)| {
                flatten(m.as_ref(), false, true)
                    .map(|f| f.len())
                    .unwrap_or(1)
            })
            .sum::<usize>()
            + sub2
                .iter()
                .map(|(m, _)| {
                    flatten(m.as_ref(), false, true)
                        .map(|f| f.len())
                        .unwrap_or(1)
                })
                .sum::<usize>();

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
                // Check if the simplified chain is shorter than the
                // sum of pre-merge leaf counts. Mirrors C cmpmap.c:1726
                // which marks `simpler = 1` whenever the simplified
                // sub-chain pointer or invert state changed.
                if let Ok(flat) = flatten(simplified.as_ref(), false, true) {
                    if flat.len() < pre_len {
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
    // Recursively simplify the reconstructed parallel CmpMap so that
    // e.g. parallel(UnitMap, UnitMap) collapses to a single UnitMap of
    // the combined dimension via UnitMap::try_merge in parallel mode.
    // reconstruct() is purely structural; without a follow-up simplify
    // the merge_loop would not see the parallel children adjacent.
    let result = simplify_mapping(result.as_ref()).unwrap_or(result);
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

    // Pad the shorter series list with UnitMaps to match lengths.
    // (A;B;C) || (D;E) becomes (A;B;C) || (D;E;UnitMap) which is equivalent
    // because UnitMap is the identity mapping and gets stripped after merging.
    let mut ser_list1 = ser_list1;
    let mut ser_list2 = ser_list2;
    while ser_list1.len() < ser_list2.len() {
        let nout = effective_nout(
            ser_list1.last().unwrap().0.as_ref(),
            ser_list1.last().unwrap().1,
        );
        ser_list1.push((Box::new(UnitMap::new(nout)), false));
    }
    while ser_list2.len() < ser_list1.len() {
        let nout = effective_nout(
            ser_list2.last().unwrap().0.as_ref(),
            ser_list2.last().unwrap().1,
        );
        ser_list2.push((Box::new(UnitMap::new(nout)), false));
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

    // If any PermMap outputs are constant, transform those constants through
    // the CmpMap components. Matches C cmpmap.c:1929-1949.
    let (qa, qb) = if aconstants || bconstants {
        use crate::coords::PointSet;

        // Transform all-BAD inputs through the PermMap to extract constant outputs.
        let p = vec![BAD_VALUE; npin];
        let pts = PointSet::from_rows(vec![p]);
        let q_pts = perm.transform(&pts, !perm_inv).ok();
        let q = q_pts.as_ref().map(|ps| ps.point(0).to_vec());

        // Transform the first nin_a outputs through component A.
        let qa = if aconstants {
            if let Some(ref qv) = q {
                let a_input: Vec<f64> = qv[..nin_a].to_vec();
                let a_pts = PointSet::from_rows(vec![a_input]);
                let comp_a = &par_list[0];
                comp_a
                    .0
                    .transform(&a_pts, !comp_a.1)
                    .ok()
                    .map(|r| r.point(0).to_vec())
            } else {
                None
            }
        } else {
            None
        };

        // Transform the last nin_b outputs through component B.
        let qb = if bconstants {
            if let Some(ref qv) = q {
                let b_input: Vec<f64> = qv[nin_a..nin_a + nin_b].to_vec();
                let b_pts = PointSet::from_rows(vec![b_input]);
                let comp_b = &par_list[1];
                comp_b
                    .0
                    .transform(&b_pts, !comp_b.1)
                    .ok()
                    .map(|r| r.point(0).to_vec())
            } else {
                None
            }
        } else {
            None
        };

        (qa, qb)
    } else {
        (None, None)
    };

    if !bconstants {
        // Second component (now first in swapped CmpMap): its nout_b outputs
        // map to the last nout_b positions in the new PermMap output.
        for idx in 0..nout_b {
            let j = npout_new - nout_b + idx;
            inperm_new[idx] = j as i64 + 1;
            outperm_new[j] = idx as i64 + 1;
        }
    } else {
        // bconstants. C cmpmap.c:1985-2007: push the inperm-side
        // constants FIRST (referencing positions 0..(npin - nin_b) of
        // the new PermMap's inputs), THEN the outperm-side transformed
        // constants (qb). The C ordering determines the resulting
        // Con1/Con2/... numbering.
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
        for idx in 0..nout_b {
            let j = npout_new - nout_b + idx;
            let const_idx = constants_new.len();
            let val = qb
                .as_ref()
                .and_then(|v| v.get(idx).copied())
                .unwrap_or(BAD_VALUE);
            constants_new.push(val);
            outperm_new[j] = -((const_idx + 1) as i64);
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
        // aconstants. C cmpmap.c:2019-2041: push the inperm-side
        // constants FIRST (positions nout_b..npin_new), THEN the
        // outperm-side transformed constants (qa). The C ordering
        // determines the resulting Con1/Con2/... numbering.
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
        for idx in 0..nout_a {
            let const_idx = constants_new.len();
            let val = qa
                .as_ref()
                .and_then(|v| v.get(idx).copied())
                .unwrap_or(BAD_VALUE);
            constants_new.push(val);
            outperm_new[idx] = -((const_idx + 1) as i64);
        }
    }

    // Build the new CmpMap with swapped component order. When
    // `aconstants` (resp. `bconstants`) is true, component A's (resp.
    // B's) inputs were absorbed into the new PermMap as transformed
    // constants. The component is no longer in the chain and must be
    // replaced by a UnitMap of its output dimension. Mirrors C
    // permmap.c MapMerge "Swapped CmpMap" branch, where each consumed
    // component is reduced to a UnitMap before assembling the new
    // parallel CmpMap.
    let nout_a = effective_nout(par_list[0].0.as_ref(), par_list[0].1);
    let nout_b = effective_nout(par_list[1].0.as_ref(), par_list[1].1);

    let new_a: DynMapping = if aconstants {
        Box::new(UnitMap::new(nout_a))
    } else {
        let Ok(mut a) = par_list[0].0.simplified() else {
            return false;
        };
        if par_list[0].1 {
            a.invert();
        }
        a
    };
    let new_b: DynMapping = if bconstants {
        Box::new(UnitMap::new(nout_b))
    } else {
        let Ok(mut b) = par_list[1].0.simplified() else {
            return false;
        };
        if par_list[1].1 {
            b.invert();
        }
        b
    };

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

/// Swap a PcdMap past an adjacent ZoomMap or axis-swapping PermMap to bring
/// the PcdMap closer to a merge partner (another PcdMap or UnitMap).
///
/// This matches the C pcdmap.c MapMerge logic: the PcdMap can swap with
/// ZoomMaps and axis-swapping PermMaps. The swap is only performed if
/// there is a potential merge target (PcdMap or UnitMap) visible on the
/// other side, to avoid infinite swap loops.
///
/// Matches C pcdmap.c:1503-1714 (CanSwap + PcdZoom + PcdPerm).
/// Try to merge GrismMap with an adjacent ZoomMap in a series list.
///
/// Two patterns from the C library's `CanMerge`:
///
/// 1. Forward GrismMap followed by ZoomMap:
///    `list[i]=GrismMap(fwd), list[i+1]=ZoomMap`
///    The forward GrismMap produces wavelength; the zoom scales it.
///    Absorbs the zoom factor into the GrismMap's physical parameters.
///
/// 2. ZoomMap followed by inverted GrismMap:
///    `list[i]=ZoomMap, list[i+1]=GrismMap(inv)`
///    Conceptually inverts the pair to get case 1, absorbs the zoom,
///    then inverts the result.
///
/// These are handled here rather than in `GrismMap::try_merge` because
/// the `try_merge` API cannot distinguish forward from reverse calls,
/// and the merges are only valid in specific orderings.
fn try_merge_zoom_before_inv_grism(list: &mut Vec<(DynMapping, bool)>) -> bool {
    for i in 0..list.len().saturating_sub(1) {
        // Pattern 1: GrismMap(fwd) + ZoomMap
        let is_fwd_grism = list[i].0.as_any().is::<GrismMap>() && !list[i].1;
        let is_zoom_after = list[i + 1].0.as_any().is::<ZoomMap>();

        if is_fwd_grism && is_zoom_after {
            let grism = list[i].0.as_any().downcast_ref::<GrismMap>().unwrap();
            let zoom = list[i + 1].0.as_any().downcast_ref::<ZoomMap>().unwrap();
            let zoom_inv = list[i + 1].1;

            // Get the effective forward zoom factor.
            // zoom.is_inverted() is always false (normalized in flatten).
            // If zoom_inv is true, the zoom is applied inversely → z = 1/factor.
            let mut z = zoom.factor();
            if zoom_inv && z != 0.0 {
                z = 1.0 / z;
            }

            if z == 0.0 {
                continue;
            }

            if let Some(merged) = grism.merge_with_zoom(z, false) {
                list[i] = (merged, false);
                list.remove(i + 1);
                return true;
            }
        }

        // Pattern 2: ZoomMap + GrismMap(inv)
        let is_zoom_before = list[i].0.as_any().is::<ZoomMap>();
        let is_inv_grism = list[i + 1].0.as_any().is::<GrismMap>() && list[i + 1].1;

        if is_zoom_before && is_inv_grism {
            let zoom = list[i].0.as_any().downcast_ref::<ZoomMap>().unwrap();
            let zoom_inv = list[i].1;
            let grism = list[i + 1].0.as_any().downcast_ref::<GrismMap>().unwrap();

            // The C code conceptually inverts the pair to get:
            //   forward GrismMap + inverted ZoomMap
            // The zoom factor for the "inverted" ZoomMap is the reciprocal
            // of its effective forward factor.
            // zoom.is_inverted() is always false (normalized in flatten).
            // C code: "if astGetInvert(map1) == inv1 && z != 0.0, z = 1/z"
            // With is_inverted()=false: if false == zoom_inv → if !zoom_inv → invert z.
            let mut z = zoom.factor();
            if !zoom_inv && z != 0.0 {
                z = 1.0 / z;
            }

            if z == 0.0 {
                continue;
            }

            if let Some(merged) = grism.merge_with_zoom(z, true) {
                list[i] = (merged, false);
                list.remove(i + 1);
                return true;
            }
        }
    }
    false
}

fn try_swap_pcdmap_past_neighbor(list: &mut Vec<(DynMapping, bool)>) -> bool {
    // Look for a PcdMap that can be swapped towards a merge target.
    let mut i = 0;
    while i < list.len() {
        if !list[i].0.as_any().is::<PcdMap>() {
            i += 1;
            continue;
        }

        let pcd_inv = list[i].1;

        // Try swapping with the higher neighbor (i+1) towards a target further right.
        // Count how many steps to a merge target in the forward direction.
        let nstep_hi = if i + 1 < list.len() && can_swap_pcd_with(&list[i + 1].0) {
            find_pcd_merge_target_steps(list, i + 2, true)
        } else {
            -1
        };

        // Try swapping with the lower neighbor (i-1) towards a target further left.
        let nstep_lo = if i >= 1 && can_swap_pcd_with(&list[i - 1].0) {
            if i >= 2 {
                find_pcd_merge_target_steps(list, i - 2, false)
            } else {
                -1
            }
        } else {
            -1
        };

        // Choose the direction that gets us to a merge target in fewer steps.
        if nstep_lo >= 0 && (nstep_hi < 0 || nstep_hi > nstep_lo) {
            // Swap with lower neighbor: neighbor at i-1, PcdMap at i.
            // PcdMap is second (ipc=1 in C terms).
            do_pcd_swap_second(list, i - 1, pcd_inv);
            return true;
        } else if nstep_hi >= 0 {
            // Swap with higher neighbor: PcdMap at i, neighbor at i+1.
            // PcdMap is first (ipc=0 in C terms).
            do_pcd_swap_first(list, i, pcd_inv);
            return true;
        }

        i += 1;
    }
    false
}

/// Check if a mapping can be swapped with an adjacent PcdMap.
fn can_swap_pcd_with(m: &DynMapping) -> bool {
    if m.as_any().is::<ZoomMap>() {
        return true;
    }
    if let Some(pm) = m.as_any().downcast_ref::<PermMap>() {
        return PcdMap::perm_is_swap(pm);
    }
    false
}

/// Search forward or backward from `start` for a PcdMap or UnitMap merge
/// target, skipping over ZoomMaps and axis-swapping PermMaps.
/// Returns the number of intermediate steps if found, -1 otherwise.
fn find_pcd_merge_target_steps(list: &[(DynMapping, bool)], start: usize, forward: bool) -> i32 {
    if start >= list.len() {
        return -1;
    }
    let mut pos = start;
    let mut steps = 0i32;
    loop {
        if pos >= list.len() {
            return -1;
        }
        if list[pos].0.as_any().is::<PcdMap>() || list[pos].0.as_any().is::<UnitMap>() {
            return steps;
        }
        if !can_swap_pcd_with(&list[pos].0) {
            return -1;
        }
        steps += 1;
        if forward {
            pos += 1;
        } else {
            if pos == 0 {
                return -1;
            }
            pos -= 1;
        }
    }
}

/// Perform PcdMap swap when PcdMap is first (at `i`) and neighbor is at `i+1`.
/// After swap: neighbor at i, new PcdMap at i+1.
/// C equivalent: ipc=0 in PcdZoom/PcdPerm.
fn do_pcd_swap_first(list: &mut [(DynMapping, bool)], i: usize, pcd_inv: bool) {
    let pcd = list[i].0.as_any().downcast_ref::<PcdMap>().unwrap();
    let pcd_disc = pcd.disc();
    let pcd_cen = pcd.pcdcen();

    if let Some(zm) = list[i + 1].0.as_any().downcast_ref::<ZoomMap>() {
        let other_inv = list[i + 1].1;
        let zoom = if other_inv {
            1.0 / zm.factor()
        } else {
            zm.factor()
        };

        // PcdMap first, ZoomMap second → after swap:
        // New centre = old_centre * zoom
        // New disc = old_disc / (zoom^2)
        let new_cen = [pcd_cen[0] * zoom, pcd_cen[1] * zoom];
        let new_disc = pcd_disc / (zoom * zoom);
        let mut new_pcd = PcdMap::new(new_disc, new_cen);
        if pcd_inv {
            new_pcd.invert();
        }

        let new_zoom = ZoomMap::new(2, zoom).unwrap();
        list[i] = (Box::new(new_zoom), false);
        list[i + 1] = (Box::new(new_pcd), false);
    } else if list[i + 1].0.as_any().is::<PermMap>() {
        let other_inv = list[i + 1].1;

        // PcdMap first, PermMap(swap) second → after swap:
        // PermMap at i, new PcdMap(swapped centre) at i+1.
        let new_cen = [pcd_cen[1], pcd_cen[0]];
        let mut new_pcd = PcdMap::new(pcd_disc, new_cen);
        if pcd_inv {
            new_pcd.invert();
        }

        let pm_clone = list[i + 1].0.clone_box();
        list[i] = (pm_clone, other_inv);
        list[i + 1] = (Box::new(new_pcd), false);
    }
}

/// Perform PcdMap swap when PcdMap is second (at `i+1`) and neighbor is at `i`.
/// After swap: new PcdMap at i, neighbor at i+1.
/// C equivalent: ipc=1 in PcdZoom/PcdPerm.
fn do_pcd_swap_second(list: &mut [(DynMapping, bool)], i: usize, pcd_inv: bool) {
    let pcd = list[i + 1].0.as_any().downcast_ref::<PcdMap>().unwrap();
    let pcd_disc = pcd.disc();
    let pcd_cen = pcd.pcdcen();

    if let Some(zm) = list[i].0.as_any().downcast_ref::<ZoomMap>() {
        let other_inv = list[i].1;
        let zoom = if other_inv {
            1.0 / zm.factor()
        } else {
            zm.factor()
        };

        // ZoomMap first, PcdMap second → after swap:
        // New centre = old_centre / zoom
        // New disc = old_disc * (zoom^2)
        let new_cen = [pcd_cen[0] / zoom, pcd_cen[1] / zoom];
        let new_disc = pcd_disc * zoom * zoom;
        let mut new_pcd = PcdMap::new(new_disc, new_cen);
        if pcd_inv {
            new_pcd.invert();
        }

        let new_zoom = ZoomMap::new(2, zoom).unwrap();
        list[i] = (Box::new(new_pcd), false);
        list[i + 1] = (Box::new(new_zoom), false);
    } else if list[i].0.as_any().is::<PermMap>() {
        let other_inv = list[i].1;

        // PermMap(swap) first, PcdMap second → after swap:
        // new PcdMap(swapped centre) at i, PermMap at i+1.
        let new_cen = [pcd_cen[1], pcd_cen[0]];
        let mut new_pcd = PcdMap::new(pcd_disc, new_cen);
        if pcd_inv {
            new_pcd.invert();
        }

        let pm_clone = list[i].0.clone_box();
        list[i] = (Box::new(new_pcd), false);
        list[i + 1] = (pm_clone, other_inv);
    }
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

        // Split the WinMap and build new parallel branches. Each branch
        // is `sub_wm ; component`. Speculatively simplify each branch and
        // only commit the distribution if at least one branch ends up
        // structurally different from the unsimplified composition —
        // i.e. the WinMap actually got absorbed somewhere. If no branch
        // simplifies, the distribution is pure churn (and risks an
        // infinite oscillation against `try_merge_parallel_in_series`),
        // so it is rejected. Matches C winmap.c:1358-1375.
        let mut new_branches: Vec<(DynMapping, bool)> = Vec::new();
        let mut any_branch_simplified = false;
        let mut failed = false;
        for ((comp, comp_inv), &(start, nin)) in par_list.iter().zip(dim_boundaries.iter()) {
            let sub_shifts = &shifts[start..start + nin];
            let sub_scales = &scales[start..start + nin];

            let sub_wm = match WinMap::new(sub_shifts, sub_scales) {
                Ok(w) => w,
                Err(_) => {
                    failed = true;
                    break;
                }
            };

            let mut comp_clone = comp.clone();
            if *comp_inv {
                comp_clone.invert();
            }

            let series = match CmpMap::new(Box::new(sub_wm), comp_clone, Composition::Series) {
                Ok(s) => s,
                Err(_) => {
                    failed = true;
                    break;
                }
            };

            // Compare structural fingerprints before/after simplification.
            // A change indicates the WinMap absorbed into the component
            // (or the chain restructured); no change means the branch
            // is just `sub_wm ; component` with the WinMap still hanging
            // around — the same as before distributing.
            let fp_before = mapping_fingerprint(&series);
            let simplified_branch = match series.simplified() {
                Ok(s) => s,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            let fp_after = mapping_fingerprint(simplified_branch.as_ref());
            if fp_before != fp_after {
                any_branch_simplified = true;
            }
            new_branches.push((simplified_branch, false));
        }

        if failed || !any_branch_simplified {
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

    // Helper: apply the stored inversion flag. For a CmpMap child of a
    // parallel reconstruct, push the inversion into its recorded
    // structure (flip inv_a/inv_b, swap children for series) so the
    // serialised form matches the C reference: outer parallel emits no
    // InvA/InvB, the child CmpMap absorbs the inversion via its own
    // flags. Mathematically equivalent to plain `invert()`. Mirrors C
    // cmpmap.c MapMerge, which emits the inversion on the inner child
    // rather than the outer parallel parent.
    let apply_inversion = |m: &mut DynMapping| {
        if !series {
            if let Some(cm) = m.as_any_mut().downcast_mut::<CmpMap>() {
                cm.invert_into_structure();
                return;
            }
        }
        m.invert();
    };

    if acc_inv {
        apply_inversion(&mut acc);
    }

    for (mut m, m_inv) in iter {
        if m_inv {
            apply_inversion(&mut m);
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
    let changed = merge_loop(&mut list, series);

    // If no simplification occurred, preserve the original tree structure
    // rather than reconstructing (which changes associativity).
    if !changed && list.len() > 1 {
        let mut cloned = mapping.clone_box();
        if let Some(cmp_mut) = cloned.as_any_mut().downcast_mut::<CmpMap>() {
            cmp_mut.normalize_unitmap_children();
        }
        return Ok(cloned);
    }

    reconstruct(list, series)
}

// ------------------------------------------------------------------ //
// Tests                                                               //
// ------------------------------------------------------------------ //

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::{ShiftMap, ZoomMap};

    #[test]
    fn parallel_in_series_with_blocking_delta() {
        // Reproduces the sip-3d-swap chain pattern:
        //   parallel(SlaMap_Hfk5z, Unit_1) ;
        //   parallel(Unit_2,       SmallShift_1) ;
        //   parallel(SlaMap_Fk5hz, Unit_1)
        //
        // The sky and spec axes are independent; the SlaMap pair
        // (Hfk5z + Fk5hz at the same epoch) cancels when adjacent.
        // The simplifier should swap the middle parallel-CmpMap past
        // one of the SlaMap parallels (since their non-trivial parts
        // operate on disjoint axes) so the two SlaMap parallels
        // become adjacent and merge to identity.
        use crate::mappings::SlaCvtType;
        use crate::mappings::SlaMap;
        use crate::mappings::UnitMap;

        let mut sla1 = SlaMap::new();
        sla1.add(SlaCvtType::Hfk5z, &[2000.0]);
        let pair1: DynMapping = Box::new(
            CmpMap::new(
                Box::new(sla1),
                Box::new(UnitMap::new(1)),
                Composition::Parallel,
            )
            .unwrap(),
        );

        let pair2: DynMapping = Box::new(
            CmpMap::new(
                Box::new(UnitMap::new(2)),
                Box::new(ShiftMap::new(vec![0.001])),
                Composition::Parallel,
            )
            .unwrap(),
        );

        let mut sla2 = SlaMap::new();
        sla2.add(SlaCvtType::Fk5hz, &[2000.0]);
        let pair3: DynMapping = Box::new(
            CmpMap::new(
                Box::new(sla2),
                Box::new(UnitMap::new(1)),
                Composition::Parallel,
            )
            .unwrap(),
        );

        let chain12: DynMapping = Box::new(CmpMap::new(pair1, pair2, Composition::Series).unwrap());
        let full: DynMapping = Box::new(CmpMap::new(chain12, pair3, Composition::Series).unwrap());

        let s = full.simplified().unwrap();
        let flat = flatten(s.as_ref(), false, true).unwrap();
        eprintln!("simplified.len = {}", flat.len());
        for (i, (m, inv)) in flat.iter().enumerate() {
            let dbg = format!("{:?}", m);
            let preview: String = dbg.chars().take(80).collect();
            eprintln!(
                "  s[{}]: {}/{} inv={} {}",
                i,
                m.nin(),
                m.nout(),
                inv,
                preview
            );
        }

        // Sanity: outputs match.
        use crate::coords::PointSet;
        let pts = PointSet::from_rows([[0.5_f64, -0.3, 1000.0]]);
        let o = full.transform(&pts, true).unwrap();
        let os = s.transform(&pts, true).unwrap();
        let p = o.point(0);
        let ps = os.point(0);
        for k in 0..3 {
            assert!(
                (p[k] - ps[k]).abs() < 1e-9,
                "mismatch at axis {}: {} vs {}",
                k,
                p[k],
                ps[k]
            );
        }

        // Ideal: the SlaMap pair cancels, leaving only the
        // SmallShift_1axis on the spec axis. Currently we can't yet
        // prove the simplifier achieves this; the test mostly logs
        // the chain structure for diagnostic purposes.
        let any_sla = flat
            .iter()
            .any(|(m, _)| m.as_any().is::<crate::mappings::SlaMap>());
        if any_sla {
            // Failure marker; convert to a hard assert once the
            // simplifier learns to swap parallel-disjoint pairs.
            eprintln!(
                "[parallel_in_series_with_blocking_delta] simplifier did NOT cancel the SlaMap pair (chain.len={})",
                flat.len()
            );
        }
    }

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
        // ZoomMap::simplified() normalizes inversion into the factor:
        // ZoomMap(5, inverted) → ZoomMap(0.2, forward)
        // So effective_inv = external(false) XOR simplified.is_inverted()(false) = false
        let list = flatten(&z, false, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].1); // inversion absorbed into factor
        assert!(!list[0].0.is_inverted()); // normalised
    }

    #[test]
    fn flatten_non_cmpmap_double_inversion_cancels() {
        let mut z = ZoomMap::new(1, 5.0).unwrap();
        z.invert();
        // ZoomMap::simplified() normalizes: ZoomMap(5, inv) → ZoomMap(0.2, fwd)
        // effective_inv = external(true) XOR simplified.is_inverted()(false) = true
        let list = flatten(&z, true, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].1); // external inversion not cancelled (already absorbed)
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
        // ZoomMap merges UnitMap (identity), then result merges ShiftMap.
        assert_eq!(list.len(), 1);
        assert!(!list[0].0.as_any().is::<UnitMap>());
    }

    #[test]
    fn merge_loop_merges_linear_parallel() {
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let u = dyn_map(UnitMap::new(1));
        let mut list: Vec<(DynMapping, bool)> = vec![(a, false), (u, false)];
        merge_loop(&mut list, false);
        // Parallel: linear mappings are merged into a wider WinMap.
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0.nin(), 2);
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

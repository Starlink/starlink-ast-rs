//! Mesh-based overlap algorithm for determining how two Regions relate.
//!
//! This module provides the shared [`compute_overlap`] function that
//! implements the overlap logic described in the C library's `region.c`
//! `OverlapX` function. Each concrete Region's `overlap()` method
//! delegates here.

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::mappings::helpers::is_bad;

use super::{Overlap, Region};

/// Fraction of mesh points that must agree for a definitive "all inside"
/// or "all outside" classification. This accounts for floating-point
/// imprecision on boundary points — mesh points generated on the
/// boundary (e.g., by `reg_base_mesh`) may test as slightly inside or
/// outside due to floating-point arithmetic. The C library handles this
/// via uncertainty regions; we use a simpler threshold.
///
/// Set at 0.90 to tolerate up to 10% boundary-ambiguity noise while
/// still distinguishing genuine partial overlap (which typically shows
/// 30-60% of points inside).
const THRESHOLD: f64 = 0.90;

/// Compute the overlap relationship between two Regions.
///
/// The algorithm:
/// 1. Handle trivial NullRegion cases (empty or universal).
/// 2. Quick-reject via bounding boxes (for non-negated bounded regions).
/// 3. Get the boundary mesh of region B, test each point against A.
/// 4. If the result is clearly mixed, return `Partial`.
/// 5. Get the boundary mesh of region A, test against B.
/// 6. Classify the relationship based on the combined results.
///
/// Both regions must have the same number of axes. This function does
/// **not** attempt frame alignment — it assumes both regions are already
/// in compatible coordinate systems (for the initial implementation,
/// both should be in the same frame).
pub(crate) fn compute_overlap(region_a: &dyn Region, region_b: &dyn Region) -> AstResult<Overlap> {
    // --- Handle NullRegion cases ---
    // A non-negated NullRegion is empty (contains nothing).
    // A negated NullRegion is universal (contains everything).
    let a_is_empty = is_null_empty(region_a);
    let b_is_empty = is_null_empty(region_b);
    let a_is_universal = is_null_universal(region_a);
    let b_is_universal = is_null_universal(region_b);

    // Two empty regions: Same (both contain nothing).
    if a_is_empty && b_is_empty {
        return Ok(Overlap::Same);
    }
    // Two universal regions: Same (both contain everything).
    if a_is_universal && b_is_universal {
        return Ok(Overlap::Same);
    }
    // One empty, one not: None (the empty region doesn't overlap anything).
    if a_is_empty || b_is_empty {
        return Ok(Overlap::None);
    }
    // A is universal: B is entirely inside A.
    if a_is_universal {
        return Ok(Overlap::SecondInside);
    }
    // B is universal: A is entirely inside B.
    if b_is_universal {
        return Ok(Overlap::FirstInside);
    }

    // --- Quick bounding-box rejection ---
    // Only applicable if both regions are bounded and neither is negated.
    if region_a.is_bounded()
        && region_b.is_bounded()
        && !region_a.is_negated()
        && !region_b.is_negated()
    {
        if let (Ok((lb_a, ub_a)), Ok((lb_b, ub_b))) =
            (region_a.get_region_bounds(), region_b.get_region_bounds())
        {
            if bounding_boxes_disjoint(&lb_a, &ub_a, &lb_b, &ub_b) {
                return Ok(Overlap::None);
            }
        }
    }

    // --- Get mesh of region B and test against region A ---
    let mesh_b = region_b.get_region_mesh()?;
    let (b_inside_a, b_total) = count_inside(region_a, &mesh_b)?;

    // --- Get mesh of region A and test against region B ---
    let mesh_a = region_a.get_region_mesh()?;
    let (a_inside_b, a_total) = count_inside(region_b, &mesh_a)?;

    // --- Classify ---
    classify(a_inside_b, a_total, b_inside_a, b_total)
}

/// Classify the overlap relationship given mesh-test counts.
///
/// - `a_inside_b`: count of A's mesh points that are inside B.
/// - `a_total`: total valid points in A's mesh.
/// - `b_inside_a`: count of B's mesh points that are inside A.
/// - `b_total`: total valid points in B's mesh.
fn classify(
    a_inside_b: usize,
    a_total: usize,
    b_inside_a: usize,
    b_total: usize,
) -> AstResult<Overlap> {
    // Determine whether "essentially all" or "essentially none" of each
    // mesh is inside the other region.
    let all_a_in_b = is_all(a_inside_b, a_total);
    let none_a_in_b = is_none(a_inside_b, a_total);
    let all_b_in_a = is_all(b_inside_a, b_total);
    let none_b_in_a = is_none(b_inside_a, b_total);

    if all_a_in_b && all_b_in_a {
        // Both meshes entirely inside the other → Same.
        Ok(Overlap::Same)
    } else if all_a_in_b && none_b_in_a {
        // A's boundary is inside B, B's boundary is outside A → A inside B.
        Ok(Overlap::FirstInside)
    } else if all_b_in_a && none_a_in_b {
        // B's boundary is inside A, A's boundary is outside B → B inside A.
        Ok(Overlap::SecondInside)
    } else if none_a_in_b && none_b_in_a {
        // Neither mesh has points inside the other → no overlap.
        Ok(Overlap::None)
    } else {
        // Any other combination implies partial overlap.
        Ok(Overlap::Partial)
    }
}

/// Returns `true` if "essentially all" valid points tested inside.
///
/// When total is 0 (empty mesh), returns `true` (vacuously, all zero
/// points are inside).
fn is_all(inside: usize, total: usize) -> bool {
    if total == 0 {
        return true;
    }
    (inside as f64) / (total as f64) >= THRESHOLD
}

/// Returns `true` if "essentially none" of the valid points tested inside.
///
/// When total is 0 (empty mesh), returns `true` (vacuously, none of
/// zero points are inside).
fn is_none(inside: usize, total: usize) -> bool {
    if total == 0 {
        return true;
    }
    (inside as f64) / (total as f64) <= (1.0 - THRESHOLD)
}

/// Test whether a region is a non-negated NullRegion (empty set).
///
/// We detect this by checking: the region is not bounded, not negated,
/// and its mesh has no valid (non-BAD) points. This is the characteristic
/// of a NullRegion — it has no boundary and contains nothing.
fn is_null_empty(region: &dyn Region) -> bool {
    if region.is_negated() || region.is_bounded() {
        return false;
    }
    // Check if the mesh is empty or contains only BAD values.
    if let Ok(mesh) = region.get_region_mesh() {
        if mesh.npoint() == 0 {
            return true;
        }
        // Check if all points are BAD (NullRegion returns a single BAD point).
        for i in 0..mesh.npoint() {
            let point = mesh.point(i);
            if !point.iter().all(|&v| is_bad(v)) {
                return false;
            }
        }
        true
    } else {
        false
    }
}

/// Test whether a region is a negated NullRegion (universal set).
///
/// A negated NullRegion contains everything. We detect it the same way
/// as `is_null_empty` but with the negation flag set.
fn is_null_universal(region: &dyn Region) -> bool {
    if !region.is_negated() || region.is_bounded() {
        return false;
    }
    // Check if the mesh is empty or contains only BAD values.
    if let Ok(mesh) = region.get_region_mesh() {
        if mesh.npoint() == 0 {
            return true;
        }
        for i in 0..mesh.npoint() {
            let point = mesh.point(i);
            if !point.iter().all(|&v| is_bad(v)) {
                return false;
            }
        }
        true
    } else {
        false
    }
}

/// Test whether two axis-aligned bounding boxes are disjoint.
///
/// Returns `true` if the boxes do not overlap on any axis. Axes with
/// BAD bounds are treated as unbounded (never disjoint on that axis).
fn bounding_boxes_disjoint(lb_a: &[f64], ub_a: &[f64], lb_b: &[f64], ub_b: &[f64]) -> bool {
    let naxes = lb_a.len().min(lb_b.len());
    for i in 0..naxes {
        // Skip axes with BAD bounds (treat as unbounded).
        if is_bad(lb_a[i]) || is_bad(ub_a[i]) || is_bad(lb_b[i]) || is_bad(ub_b[i]) {
            continue;
        }
        // Check if separated on this axis.
        if ub_a[i] < lb_b[i] || ub_b[i] < lb_a[i] {
            return true;
        }
    }
    false
}

/// Count how many valid points in a mesh are inside a given region.
///
/// Returns `(count_inside, count_total_valid)` where `count_total_valid`
/// is the number of mesh points that have all non-BAD coordinates.
fn count_inside(region: &dyn Region, mesh: &PointSet) -> AstResult<(usize, usize)> {
    let npoint = mesh.npoint();
    let mut inside_count = 0usize;
    let mut valid_count = 0usize;

    for i in 0..npoint {
        let point = mesh.point(i);

        // Skip points with BAD coordinates.
        if point.iter().any(|&v| is_bad(v)) {
            continue;
        }

        valid_count += 1;
        if region.point_in_region(point)? {
            inside_count += 1;
        }
    }

    Ok((inside_count, valid_count))
}

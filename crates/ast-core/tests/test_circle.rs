//! Tests for the Circle region implementation.
//!
//! Covers: construction (radius and edge_point), point_in_region,
//! negation, Mapping::transform, Frame delegation, bounding box, mesh.

use crate::common::native_dump;
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{CircleBuilder, Region};

// ---- Construction ----

#[test]
fn construct_with_radius() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Verify naxes delegation.
    assert_eq!(circle.naxes(), 2);

    // Verify the circle parameters.
    let (centre, radius, _) = circle.circle_pars().unwrap();
    assert_eq!(centre.len(), 2);
    assert!((centre[0]).abs() < 1e-10);
    assert!((centre[1]).abs() < 1e-10);
    assert!((radius - 5.0).abs() < 1e-10);
}

#[test]
fn construct_with_edge_point() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![1.0, 2.0])
        .unwrap()
        .edge_point(vec![4.0, 6.0])
        .build()
        .unwrap();

    // Distance from (1,2) to (4,6) = sqrt(9+16) = 5.
    let (centre, radius, _) = circle.circle_pars().unwrap();
    assert!((centre[0] - 1.0).abs() < 1e-10);
    assert!((centre[1] - 2.0).abs() < 1e-10);
    assert!((radius - 5.0).abs() < 1e-10);
}

/// C stores a Circle built from a radius as center plus a circumference point
/// (`CircumPoint`, `circle.c:570`, called at `:2655`), and `CalcPars` then
/// defines the radius it reports as the geodesic distance back to that point.
/// `CircumPoint` offsets along the first axis *by the radius*, not by an
/// arbitrary unit, which is what makes that round-trip exact — and on a
/// SkyFrame a one-ulp error in the stored radius is enough to flip a
/// near-pole membership test.
#[test]
fn radius_round_trips_exactly_on_a_sky_frame() {
    use ast_core::frames::{SkyFrame, SkySystem};
    use std::f64::consts::FRAC_PI_2;

    let radius = 0.3;
    let circle = CircleBuilder::new(
        SkyFrame::new(SkySystem::Icrs),
        vec![0.0, FRAC_PI_2 - radius],
    )
    .unwrap()
    .radius(radius)
    .build()
    .unwrap();

    let (_, reported, _) = circle.circle_pars().unwrap();
    assert_eq!(
        reported, radius,
        "stored radius must round-trip bit-exactly, got {reported:.17e}"
    );

    // The pole is 0.30000000000000004441 from the center, which is one ulp
    // above the requested radius, so it lies strictly outside the circle — as
    // the C oracle reports.
    assert!(
        !circle.point_in_region(&[0.0, FRAC_PI_2]).unwrap(),
        "the north pole must lie outside the circle"
    );
}

#[test]
fn construct_requires_matching_axes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = CircleBuilder::new(frame, vec![1.0, 2.0, 3.0]);
    assert!(result.is_err());
}

#[test]
fn construct_requires_edge_or_radius() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = CircleBuilder::new(frame, vec![1.0, 2.0]).unwrap().build();
    assert!(result.is_err());
}

// ---- point_in_region ----

#[test]
fn point_inside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Origin is at the center — definitely inside.
    assert!(circle.point_in_region(&[0.0, 0.0]).unwrap());

    // Point at distance 3 — inside.
    assert!(circle.point_in_region(&[3.0, 0.0]).unwrap());
    assert!(circle.point_in_region(&[0.0, 4.0]).unwrap());

    // Point at distance 4.9 — inside.
    assert!(circle.point_in_region(&[4.9, 0.0]).unwrap());
}

#[test]
fn point_outside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Point at distance 6 — outside.
    assert!(!circle.point_in_region(&[6.0, 0.0]).unwrap());
    assert!(!circle.point_in_region(&[0.0, -6.0]).unwrap());

    // Point at distance ~7.07 — outside.
    assert!(!circle.point_in_region(&[5.0, 5.0]).unwrap());
}

#[test]
fn point_on_boundary_closed() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Default is closed: boundary points are inside.
    assert!(circle.is_closed());
    assert!(circle.point_in_region(&[5.0, 0.0]).unwrap());
    assert!(circle.point_in_region(&[0.0, 5.0]).unwrap());
    assert!(circle.point_in_region(&[3.0, 4.0]).unwrap()); // 3^2 + 4^2 = 25 = 5^2
}

#[test]
fn point_on_boundary_open() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    circle.set_closed(false);
    assert!(!circle.is_closed());

    // Boundary points are now outside.
    assert!(!circle.point_in_region(&[5.0, 0.0]).unwrap());
    assert!(!circle.point_in_region(&[3.0, 4.0]).unwrap());

    // But interior points are still inside.
    assert!(circle.point_in_region(&[4.99, 0.0]).unwrap());
}

#[test]
fn point_with_bad_value() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // BAD coordinates should return false (not inside).
    assert!(!circle.point_in_region(&[BAD_VALUE, 0.0]).unwrap());
    assert!(!circle.point_in_region(&[0.0, BAD_VALUE]).unwrap());
    assert!(!circle.point_in_region(&[BAD_VALUE, BAD_VALUE]).unwrap());
}

// ---- Negation ----

#[test]
fn negation_flips_membership() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Before negation: center is inside, far point is outside.
    assert!(circle.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(!circle.point_in_region(&[10.0, 0.0]).unwrap());

    // Negate.
    circle.negate();
    assert!(circle.is_negated());

    // After negation: center is outside, far point is inside.
    assert!(!circle.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(circle.point_in_region(&[10.0, 0.0]).unwrap());
}

#[test]
fn negated_copy() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let neg = circle.negated_copy();
    assert!(!circle.is_negated());
    assert!(neg.is_negated());

    // Original still works.
    assert!(circle.point_in_region(&[0.0, 0.0]).unwrap());
    // Negated copy: center is outside.
    assert!(!neg.point_in_region(&[0.0, 0.0]).unwrap());
}

// ---- Mapping::transform ----

#[test]
fn transform_passes_interior() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Points inside should pass through unchanged.
    let pts = PointSet::from_rows([[1.0, 1.0], [3.0, 4.0]]);
    let result = circle.transform(&pts, true).unwrap();

    assert_eq!(result.npoint(), 2);
    assert!((result.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 1.0).abs() < 1e-10);
    assert!((result.point(1)[0] - 3.0).abs() < 1e-10);
    assert!((result.point(1)[1] - 4.0).abs() < 1e-10);
}

#[test]
fn transform_masks_exterior() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Points outside should become BAD_VALUE.
    let pts = PointSet::from_rows([[10.0, 0.0], [0.0, -10.0]]);
    let result = circle.transform(&pts, true).unwrap();

    assert_eq!(result.npoint(), 2);
    assert_eq!(result.point(0)[0], BAD_VALUE);
    assert_eq!(result.point(0)[1], BAD_VALUE);
    assert_eq!(result.point(1)[0], BAD_VALUE);
    assert_eq!(result.point(1)[1], BAD_VALUE);
}

#[test]
fn transform_mixed() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Mix of inside and outside points.
    let pts = PointSet::from_rows([[1.0, 1.0], [10.0, 10.0], [0.0, 0.0]]);
    let result = circle.transform(&pts, true).unwrap();

    // Point 0: inside — passed through.
    assert!((result.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 1.0).abs() < 1e-10);

    // Point 1: outside — BAD.
    assert_eq!(result.point(1)[0], BAD_VALUE);
    assert_eq!(result.point(1)[1], BAD_VALUE);

    // Point 2: center — inside.
    assert!((result.point(2)[0]).abs() < 1e-10);
    assert!((result.point(2)[1]).abs() < 1e-10);
}

#[test]
fn transform_inverse_masks_as_forward() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Inverse transform always passes points through unchanged.
    let pts = PointSet::from_rows([[10.0, 10.0], [0.0, 0.0]]);

    // C's Region classes transform identically in both directions, so a
    // point outside the Region is bad on the inverse too.
    let inverse = circle.transform(&pts, false).unwrap();
    let forward = circle.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        assert_eq!(inverse.point(i), forward.point(i), "point {i}");
    }
    assert!(inverse.point(0).iter().all(|&v| v == BAD_VALUE));
}

// ---- Frame delegation ----

#[test]
fn frame_delegation() {
    let frame = BasicFrameBuilder::new(3)
        .title("Test Frame")
        .domain("TEST")
        .label(1, "X")
        .label(2, "Y")
        .label(3, "Z")
        .build()
        .unwrap();

    let circle = CircleBuilder::new(frame, vec![0.0, 0.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    assert_eq!(circle.naxes(), 3);
}

#[test]
fn frame_distance_delegation() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Circle's distance method should delegate to the underlying frame.
    let d = circle.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

// ---- Bounding box ----

#[test]
fn bounding_box_2d() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![1.0, 2.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    let (lower, upper) = circle.get_region_bounds().unwrap();
    assert_eq!(lower.len(), 2);
    assert_eq!(upper.len(), 2);

    // Lower bound: center - radius.
    assert!((lower[0] - (-2.0)).abs() < 1e-10);
    assert!((lower[1] - (-1.0)).abs() < 1e-10);

    // Upper bound: center + radius.
    assert!((upper[0] - 4.0).abs() < 1e-10);
    assert!((upper[1] - 5.0).abs() < 1e-10);
}

#[test]
fn bounding_box_3d() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![5.0, 10.0, -3.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let (lower, upper) = circle.get_region_bounds().unwrap();
    assert_eq!(lower.len(), 3);
    assert_eq!(upper.len(), 3);

    assert!((lower[0] - 3.0).abs() < 1e-10);
    assert!((lower[1] - 8.0).abs() < 1e-10);
    assert!((lower[2] - (-5.0)).abs() < 1e-10);
    assert!((upper[0] - 7.0).abs() < 1e-10);
    assert!((upper[1] - 12.0).abs() < 1e-10);
    assert!((upper[2] - (-1.0)).abs() < 1e-10);
}

// ---- Disc ----

// ---- Mesh ----

#[test]
fn mesh_2d_points_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    circle.set_mesh_size(100);
    let mesh = circle.base_mesh().unwrap();
    assert_eq!(mesh.npoint(), 100);
    assert_eq!(mesh.ncoord(), 2);

    // Every mesh point should be at distance ~5 from the center.
    for i in 0..mesh.npoint() {
        let p = mesh.point(i);
        let dist = (p[0] * p[0] + p[1] * p[1]).sqrt();
        assert!(
            (dist - 5.0).abs() < 1e-10,
            "mesh point {} at distance {} from centre (expected 5.0)",
            i,
            dist
        );
    }
}

#[test]
fn mesh_1d() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![10.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    let mesh = circle.base_mesh().unwrap();
    // 1-D circle boundary is just 2 points.
    assert_eq!(mesh.npoint(), 2);
    assert_eq!(mesh.ncoord(), 1);

    let p0 = mesh.point(0)[0];
    let p1 = mesh.point(1)[0];
    let (lo, hi) = if p0 < p1 { (p0, p1) } else { (p1, p0) };
    assert!((lo - 7.0).abs() < 1e-10);
    assert!((hi - 13.0).abs() < 1e-10);
}

// ---- Off-center circle ----

#[test]
fn off_centre_point_in_region() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![10.0, 20.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    // Center is inside.
    assert!(circle.point_in_region(&[10.0, 20.0]).unwrap());

    // Point at distance 2 from center.
    assert!(circle.point_in_region(&[12.0, 20.0]).unwrap());

    // Point at distance 4 from center — outside.
    assert!(!circle.point_in_region(&[14.0, 20.0]).unwrap());
}

// ---- Mapping properties ----

#[test]
fn mapping_nin_nout() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    assert_eq!(circle.nin(), 2);
    assert_eq!(circle.nout(), 2);
}

// ---- Region attributes ----

#[test]
fn region_attributes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Defaults.
    assert!(circle.is_closed());
    assert!(!circle.is_negated());
    assert!(circle.is_bounded().unwrap());
    assert!(circle.is_adaptive()); // C default is true
    assert_eq!(circle.mesh_size(), 200);
    assert!((circle.fill_factor() - 1.0).abs() < 1e-10);

    // Setters.
    circle.set_closed(false);
    assert!(!circle.is_closed());

    circle.set_mesh_size(500);
    assert_eq!(circle.mesh_size(), 500);

    circle.set_fill_factor(0.5).unwrap();
    assert!((circle.fill_factor() - 0.5).abs() < 1e-10);

    circle.set_adaptive(false);
    assert!(!circle.is_adaptive());
}

// ---- Clone ----

#[test]
fn clone_is_independent() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let mut clone = circle.clone();
    clone.negate();

    // Original unchanged.
    assert!(!circle.is_negated());
    assert!(clone.is_negated());

    // Both still functional.
    assert!(circle.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(!clone.point_in_region(&[0.0, 0.0]).unwrap());
}

// ---- Batch transform (large point sets) ----

#[test]
fn batch_transform_10k_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();

    // Create 10,000 points in a grid from -2 to 2
    let n = 100;
    let total = n * n;
    let mut pts = PointSet::zeros(total, 2);
    for i in 0..n {
        for j in 0..n {
            let idx = i * n + j;
            let x = -2.0 + 4.0 * i as f64 / (n - 1) as f64;
            let y = -2.0 + 4.0 * j as f64 / (n - 1) as f64;
            pts.point_mut(idx)[0] = x;
            pts.point_mut(idx)[1] = y;
        }
    }

    let out = circle.transform(&pts, true).unwrap();
    assert_eq!(out.npoint(), total);

    // Verify: points inside circle pass through, outside become BAD
    let mut inside_count = 0;
    let mut outside_count = 0;
    for i in 0..total {
        let p = pts.point(i);
        let dist = (p[0] * p[0] + p[1] * p[1]).sqrt();
        let o = out.point(i);
        if dist <= 1.0 {
            assert_eq!(o[0], p[0], "point {i} should pass through");
            assert_eq!(o[1], p[1]);
            inside_count += 1;
        } else {
            assert_eq!(o[0], BAD_VALUE, "point {i} should be BAD");
            assert_eq!(o[1], BAD_VALUE);
            outside_count += 1;
        }
    }

    // Sanity: we should have a reasonable split
    assert!(
        inside_count > 700,
        "expected >700 inside, got {inside_count}"
    );
    assert!(
        outside_count > 7000,
        "expected >7000 outside, got {outside_count}"
    );
}

// ---- Region integrity ----

#[test]
fn with_integrity_identity_change_preserves_points() {
    // Mutating a frame in a way that produces an identity mapping
    // (BasicFrame::map_from always returns UnitMap) should leave
    // defining points unchanged.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![3.0, 4.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Record original disc parameters.
    let (orig_centre, orig_radius) = circle.get_region_disc().unwrap();

    // Apply a no-op change via with_integrity.
    // BasicFrame::map_from always returns UnitMap, so the delta is identity.
    circle.with_integrity(|_frame| Ok(())).unwrap();

    // Verify defining points are unchanged.
    let (centre, radius) = circle.get_region_disc().unwrap();
    assert!((centre[0] - orig_centre[0]).abs() < 1e-10);
    assert!((centre[1] - orig_centre[1]).abs() < 1e-10);
    assert!((radius - orig_radius).abs() < 1e-10);

    // Region still works correctly.
    assert!(circle.point_in_region(&[3.0, 4.0]).unwrap());
    assert!(!circle.point_in_region(&[100.0, 100.0]).unwrap());
}

#[test]
fn with_integrity_non_adaptive_preserves_points() {
    // When adaptive is false, the mutation is applied to the current frame
    // and defining points remain numerically unchanged.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![3.0, 4.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    circle.set_adaptive(false);
    assert!(!circle.is_adaptive());

    // Record original disc parameters.
    let (orig_centre, orig_radius) = circle.get_region_disc().unwrap();

    // Apply a change via with_integrity.
    circle.with_integrity(|_frame| Ok(())).unwrap();

    // Defining points are unchanged (non-adaptive means no remapping).
    let (centre, radius) = circle.get_region_disc().unwrap();
    assert!((centre[0] - orig_centre[0]).abs() < 1e-10);
    assert!((centre[1] - orig_centre[1]).abs() < 1e-10);
    assert!((radius - orig_radius).abs() < 1e-10);
}

#[test]
fn with_integrity_invalidates_cache() {
    // After with_integrity, the mesh cache should be invalidated.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Force mesh computation to populate the cache.
    let _mesh = circle.base_mesh().unwrap();

    // Apply integrity change.
    circle.with_integrity(|_frame| Ok(())).unwrap();

    // The mesh should still be computable (cache was cleared but
    // the geometry is unchanged, so recomputation works fine).
    let mesh = circle.base_mesh().unwrap();
    assert_eq!(mesh.npoint(), 200); // default mesh_size

    // Verify mesh points are still valid (on the boundary).
    for i in 0..mesh.npoint() {
        let p = mesh.point(i);
        let dist = (p[0] * p[0] + p[1] * p[1]).sqrt();
        assert!(
            (dist - 5.0).abs() < 1e-10,
            "mesh point {} at distance {} (expected 5.0)",
            i,
            dist
        );
    }
}

#[test]
fn with_integrity_callable_multiple_times() {
    // Verify with_integrity can be called multiple times in sequence.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![1.0, 2.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    for _ in 0..5 {
        circle.with_integrity(|_frame| Ok(())).unwrap();
    }

    // Circle should still be functional.
    let (centre, radius, _) = circle.circle_pars().unwrap();
    assert!((centre[0] - 1.0).abs() < 1e-10);
    assert!((centre[1] - 2.0).abs() < 1e-10);
    assert!((radius - 3.0).abs() < 1e-10);
    assert!(circle.point_in_region(&[1.0, 2.0]).unwrap());
}

#[test]
fn with_integrity_error_propagates() {
    // Errors from the closure should propagate out.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let result = circle.with_integrity(|_frame| {
        Err(ast_core::error::AstError::new(
            ast_core::error::ErrorCode::Attin,
            "test error",
        ))
    });
    assert!(result.is_err());

    // Circle should still be functional after the failed integrity call.
    assert!(circle.point_in_region(&[0.0, 0.0]).unwrap());
}

// ---- reg_pins: on-boundary-within-uncertainty test (astRegPins port) ----

#[test]
fn reg_pins_circle_edge_on_boundary() {
    // Circle centered at origin, radius 5; default uncertainty band is tiny.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // A point on the circle (radius 5 along axis 1) is on the boundary.
    let on_edge = PointSet::from_rows([[5.0_f64, 0.0]]);
    assert!(c.reg_pins(&on_edge, None).unwrap());
}

#[test]
fn reg_pins_circle_interior_and_exterior_off_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // The center is interior, a far point is exterior; neither is on-boundary.
    let interior = PointSet::from_rows([[0.0_f64, 0.0]]);
    assert!(!c.reg_pins(&interior, None).unwrap());
    let exterior = PointSet::from_rows([[20.0_f64, 0.0]]);
    assert!(!c.reg_pins(&exterior, None).unwrap());
}

#[test]
fn reg_pins_circle_full_mesh_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let mesh = c.base_mesh().unwrap();
    assert!(c.reg_pins(&mesh, None).unwrap());
}

/// Ported from testregions.c `checkLoadedUncFrame`: an uncertainty set on a
/// Circle whose current Frame differs from its base Frame is dumped without a
/// Frame, and reading the dump gives it the Circle's base Frame (region.c
/// `Load`), so it keeps the Domain FOO.
#[test]
fn loaded_frameless_uncertainty_takes_the_base_frame() {
    use ast_core::frames::FrameRef;
    use ast_core::mappings::ZoomMap;
    use ast_core::{AnyObject, from_native_string};

    let foo = || {
        let mut f = BasicFrameBuilder::new(2).build().unwrap();
        f.set_domain("FOO");
        f
    };
    let mut bar = BasicFrameBuilder::new(2).build().unwrap();
    bar.set_domain("BAR");
    let unc = CircleBuilder::new(foo(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.001)
        .build()
        .unwrap();
    let circle = CircleBuilder::new(foo(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();
    let mut mapped = circle
        .reg_map_region(&ZoomMap::new(2, 2.0).unwrap(), &bar)
        .unwrap();
    mapped.set_uncertainty(Box::new(unc)).unwrap();

    let text = native_dump(&AnyObject::from_mapping(mapped.as_ref()).unwrap()).unwrap();
    let restored = from_native_string(&text).unwrap().into_region().unwrap();
    let unc = restored.get_unc_frm(FrameRef::Base).unwrap();
    assert_eq!(unc.get_region_frame().domain(), "FOO");
}

#[test]
fn copy_keeps_ident_and_own_use_defs_and_clears_report() {
    // A copy is C's astCopy, which keeps Ident and a UseDefs held by the
    // Region itself (which only a dump supplies), and clears Report
    // (mapping.c `Copy`).
    let mut circle = CircleBuilder::new(BasicFrameBuilder::new(2).build().unwrap(), vec![1.0, 2.0])
        .unwrap()
        .radius(0.5)
        .build()
        .unwrap();
    Frame::set_ident(&mut circle, "C1").unwrap();
    let text = native_dump(&ast_core::AnyObject::Circle(circle)).unwrap();
    let text = text.replacen(
        "    Ident = \"C1\" \t# Permanent Object identification string\n",
        "    Ident = \"C1\" \t# Permanent Object identification string\n    UseDfs = 0 \t# Use default values\n",
        1,
    );
    let mut circle = ast_core::from_native_string(&text)
        .unwrap()
        .into_region()
        .unwrap();
    assert!(!circle.use_defs());
    circle.set_report(true);

    let copy = circle.clone_region();
    assert_eq!(Frame::ident(copy.as_ref()), "C1");
    assert!(!copy.test_report());
    assert!(!copy.use_defs());
}

/// `get_unc` gives the uncertainty in the Region's current Frame, as
/// `astGetUnc` does, with `RegionFS` cleared so a dump of it carries its
/// Frame; with `def` false it gives `None` unless an uncertainty was set.
#[test]
fn get_unc_gives_the_uncertainty_in_the_current_frame() {
    use ast_core::mappings::ZoomMap;

    let foo = || {
        let mut f = BasicFrameBuilder::new(2).build().unwrap();
        f.set_domain("FOO");
        f
    };
    let mut bar = BasicFrameBuilder::new(2).build().unwrap();
    bar.set_domain("BAR");
    let circle = CircleBuilder::new(foo(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();
    assert!(circle.get_unc(false).unwrap().is_none());
    assert!(circle.get_unc(true).unwrap().is_some());

    let unc = CircleBuilder::new(foo(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.001)
        .build()
        .unwrap();
    let mut mapped = circle
        .reg_map_region(&ZoomMap::new(2, 2.0).unwrap(), &bar)
        .unwrap();
    mapped.set_uncertainty(Box::new(unc)).unwrap();

    let got = mapped
        .get_unc(false)
        .unwrap()
        .expect("an uncertainty was set");
    let (lbnd, ubnd) = got.get_region_bounds().unwrap();
    let want_l = [1.998, 0.998];
    let want_u = [2.002, 1.002];
    for i in 0..2 {
        assert!((lbnd[i] - want_l[i]).abs() < 1e-9, "lbnd {lbnd:?}");
        assert!((ubnd[i] - want_u[i]).abs() < 1e-9, "ubnd {ubnd:?}");
    }
    assert_eq!(got.get_region_frame().domain(), "BAR");
    assert!(!got.test_region_fs());
}

/// A Region given a FrameSet encapsulates the FrameSet's current Frame, and
/// one given a Region the Region's Frame: the values of a C-oracle probe of
/// `astCircle`.
#[test]
fn a_frameset_given_as_the_frame_contributes_its_current_frame() {
    use ast_core::AnyObject;
    use ast_core::frames::{FrameRef, FrameSet};
    use ast_core::mappings::ZoomMap;

    let frame = |domain: &str| BasicFrameBuilder::new(2).domain(domain).build().unwrap();
    let mut fs = FrameSet::new(Box::new(frame("X")));
    fs.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        Box::new(frame("Y")),
    )
    .unwrap();
    fn circle(f: impl Frame + 'static, r: f64) -> ast_core::regions::Circle {
        CircleBuilder::new(f, vec![0.0, 0.0])
            .unwrap()
            .radius(r)
            .build()
            .unwrap()
    }

    let on_fs = circle(fs.clone(), 1.0);
    assert_eq!(on_fs.domain(), "Y");
    assert_eq!(
        native_dump(&AnyObject::Circle(on_fs)).unwrap(),
        native_dump(&AnyObject::Circle(circle(frame("Y"), 1.0))).unwrap()
    );

    fs.invert();
    let on_inverted = circle(fs, 1.0);
    assert_eq!(on_inverted.domain(), "X");
    let on_circle = circle(on_inverted, 2.0);
    assert_eq!(on_circle.domain(), "X");
}

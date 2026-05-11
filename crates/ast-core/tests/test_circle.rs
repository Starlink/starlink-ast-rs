//! Tests for the Circle region implementation.
//!
//! Covers: construction (radius and edge_point), point_in_region,
//! negation, Mapping::transform, Frame delegation, bounding box, mesh.

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

    // Verify the disc parameters.
    let (centre, radius) = circle.get_region_disc().unwrap();
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
    let (centre, radius) = circle.get_region_disc().unwrap();
    assert!((centre[0] - 1.0).abs() < 1e-10);
    assert!((centre[1] - 2.0).abs() < 1e-10);
    assert!((radius - 5.0).abs() < 1e-10);
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

    // Origin is at the centre — definitely inside.
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

    // Before negation: centre is inside, far point is outside.
    assert!(circle.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(!circle.point_in_region(&[10.0, 0.0]).unwrap());

    // Negate.
    circle.negate();
    assert!(circle.is_negated());

    // After negation: centre is outside, far point is inside.
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
    // Negated copy: centre is outside.
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
    let pts = PointSet::from_rows(&[[1.0, 1.0], [3.0, 4.0]]);
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
    let pts = PointSet::from_rows(&[[10.0, 0.0], [0.0, -10.0]]);
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
    let pts = PointSet::from_rows(&[[1.0, 1.0], [10.0, 10.0], [0.0, 0.0]]);
    let result = circle.transform(&pts, true).unwrap();

    // Point 0: inside — passed through.
    assert!((result.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 1.0).abs() < 1e-10);

    // Point 1: outside — BAD.
    assert_eq!(result.point(1)[0], BAD_VALUE);
    assert_eq!(result.point(1)[1], BAD_VALUE);

    // Point 2: centre — inside.
    assert!((result.point(2)[0]).abs() < 1e-10);
    assert!((result.point(2)[1]).abs() < 1e-10);
}

#[test]
fn transform_inverse_passes_all() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    // Inverse transform always passes points through unchanged.
    let pts = PointSet::from_rows(&[[10.0, 10.0], [0.0, 0.0]]);
    let result = circle.transform(&pts, false).unwrap();

    assert!((result.point(0)[0] - 10.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 10.0).abs() < 1e-10);
    assert!((result.point(1)[0]).abs() < 1e-10);
    assert!((result.point(1)[1]).abs() < 1e-10);
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

    // Lower bound: centre - radius.
    assert!((lower[0] - (-2.0)).abs() < 1e-10);
    assert!((lower[1] - (-1.0)).abs() < 1e-10);

    // Upper bound: centre + radius.
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

#[test]
fn get_region_disc() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![3.0, 4.0])
        .unwrap()
        .radius(7.0)
        .build()
        .unwrap();

    let (centre, radius) = circle.get_region_disc().unwrap();
    assert!((centre[0] - 3.0).abs() < 1e-10);
    assert!((centre[1] - 4.0).abs() < 1e-10);
    assert!((radius - 7.0).abs() < 1e-10);
}

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
    let mesh = circle.get_region_mesh().unwrap();
    assert_eq!(mesh.npoint(), 100);
    assert_eq!(mesh.ncoord(), 2);

    // Every mesh point should be at distance ~5 from the centre.
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

    let mesh = circle.get_region_mesh().unwrap();
    // 1-D circle boundary is just 2 points.
    assert_eq!(mesh.npoint(), 2);
    assert_eq!(mesh.ncoord(), 1);

    let p0 = mesh.point(0)[0];
    let p1 = mesh.point(1)[0];
    let (lo, hi) = if p0 < p1 { (p0, p1) } else { (p1, p0) };
    assert!((lo - 7.0).abs() < 1e-10);
    assert!((hi - 13.0).abs() < 1e-10);
}

// ---- Off-centre circle ----

#[test]
fn off_centre_point_in_region() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![10.0, 20.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    // Centre is inside.
    assert!(circle.point_in_region(&[10.0, 20.0]).unwrap());

    // Point at distance 2 from centre.
    assert!(circle.point_in_region(&[12.0, 20.0]).unwrap());

    // Point at distance 4 from centre — outside.
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
    assert!(circle.is_bounded());
    assert!(!circle.is_adaptive());
    assert_eq!(circle.mesh_size(), 200);
    assert!((circle.fill_factor() - 1.0).abs() < 1e-10);

    // Setters.
    circle.set_closed(false);
    assert!(!circle.is_closed());

    circle.set_mesh_size(500);
    assert_eq!(circle.mesh_size(), 500);

    circle.set_fill_factor(0.5);
    assert!((circle.fill_factor() - 0.5).abs() < 1e-10);

    circle.set_adaptive(true);
    assert!(circle.is_adaptive());
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

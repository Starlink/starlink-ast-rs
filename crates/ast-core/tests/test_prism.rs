//! Tests for the Prism region implementation.
//!
//! Covers: construction, point membership, negation, bounding box,
//! batch transform, dimensionality checks.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{CircleBuilder, IntervalBuilder, PrismBuilder, Region};

// ---- Helpers ----

/// Create a 2D circle centred at (0, 0) with radius 5.
fn make_circle() -> ast_core::regions::Circle {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap()
}

/// Create a 1D interval [2, 8].
fn make_interval() -> ast_core::regions::Interval {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    IntervalBuilder::new(frame, vec![2.0], vec![8.0])
        .unwrap()
        .build()
        .unwrap()
}

// ---- Construction ----

#[test]
fn construct_cylinder_prism() {
    let circle = make_circle();
    let interval = make_interval();

    let prism = PrismBuilder::new(circle, interval)
        .unwrap()
        .build()
        .unwrap();

    // 2D circle + 1D interval = 3D prism
    assert_eq!(prism.naxes(), 3);
}

#[test]
fn naxes_is_sum() {
    let frame3 = BasicFrameBuilder::new(3).build().unwrap();
    let frame2 = BasicFrameBuilder::new(2).build().unwrap();

    let circle3 = CircleBuilder::new(frame3, vec![0.0, 0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let interval2 = IntervalBuilder::new(frame2, vec![0.0, 0.0], vec![1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(circle3, interval2)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(prism.naxes(), 5);
}

// ---- Point membership ----

#[test]
fn point_inside_both_regions() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    // (1, 1) is inside the circle (distance from origin = sqrt(2) < 5)
    // 5.0 is inside the interval [2, 8]
    assert!(prism.point_in_region(&[1.0, 1.0, 5.0]).unwrap());
}

#[test]
fn point_inside_circle_outside_interval() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    // (1, 1) is inside the circle
    // 10.0 is outside the interval [2, 8]
    assert!(!prism.point_in_region(&[1.0, 1.0, 10.0]).unwrap());
}

#[test]
fn point_outside_circle_inside_interval() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    // (10, 10) is outside the circle (distance = sqrt(200) > 5)
    // 5.0 is inside the interval [2, 8]
    assert!(!prism.point_in_region(&[10.0, 10.0, 5.0]).unwrap());
}

#[test]
fn point_outside_both_regions() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    // (10, 10) is outside the circle
    // 10.0 is outside the interval [2, 8]
    assert!(!prism.point_in_region(&[10.0, 10.0, 10.0]).unwrap());
}

#[test]
fn point_on_circle_boundary_inside() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    // (5, 0) is on the circle boundary, interval midpoint
    // Closed region includes boundary
    assert!(prism.point_in_region(&[5.0, 0.0, 5.0]).unwrap());
}

#[test]
fn bad_coordinate_means_outside() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    // A BAD coordinate in any position -> outside
    assert!(!prism.point_in_region(&[BAD_VALUE, 0.0, 5.0]).unwrap());
    assert!(!prism.point_in_region(&[0.0, BAD_VALUE, 5.0]).unwrap());
    assert!(!prism.point_in_region(&[0.0, 0.0, BAD_VALUE]).unwrap());
}

// ---- Negation ----

#[test]
fn negation_inverts_membership() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    let negated = prism.negated_copy();

    // Point inside the original -> outside negated
    assert!(prism.point_in_region(&[1.0, 1.0, 5.0]).unwrap());
    assert!(!negated.point_in_region(&[1.0, 1.0, 5.0]).unwrap());

    // Point outside the original -> inside negated
    assert!(!prism.point_in_region(&[10.0, 10.0, 10.0]).unwrap());
    assert!(negated.point_in_region(&[10.0, 10.0, 10.0]).unwrap());
}

#[test]
fn negate_in_place() {
    let mut prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    assert!(!prism.is_negated());
    assert!(prism.point_in_region(&[1.0, 1.0, 5.0]).unwrap());

    prism.negate();
    assert!(prism.is_negated());
    assert!(!prism.point_in_region(&[1.0, 1.0, 5.0]).unwrap());
}

// ---- Bounding box ----

#[test]
fn bounding_box_is_concatenation() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = prism.get_region_bounds().unwrap();

    assert_eq!(lbnd.len(), 3);
    assert_eq!(ubnd.len(), 3);

    // Circle bounds: [-5, -5] to [5, 5]
    assert!((lbnd[0] - (-5.0)).abs() < 1e-10);
    assert!((lbnd[1] - (-5.0)).abs() < 1e-10);
    assert!((ubnd[0] - 5.0).abs() < 1e-10);
    assert!((ubnd[1] - 5.0).abs() < 1e-10);

    // Interval bounds: [2] to [8]
    assert!((lbnd[2] - 2.0).abs() < 1e-10);
    assert!((ubnd[2] - 8.0).abs() < 1e-10);
}

// ---- Boundedness ----

#[test]
fn bounded_when_both_bounded() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    assert!(prism.is_bounded());
}

#[test]
fn unbounded_when_interval_unbounded() {
    let frame1d = BasicFrameBuilder::new(1).build().unwrap();
    // One-sided interval (lower bound only, upper unbounded)
    let interval = IntervalBuilder::new(frame1d, vec![2.0], vec![BAD_VALUE])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(make_circle(), interval)
        .unwrap()
        .build()
        .unwrap();

    assert!(!prism.is_bounded());
}

// ---- Batch transform (Mapping trait) ----

#[test]
fn forward_transform_filters_points() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    // Three points: inside, outside (bad circle), outside (bad interval)
    let pts = PointSet::from_rows(
        [
            [1.0, 1.0, 5.0].as_slice(),   // inside both
            [10.0, 10.0, 5.0].as_slice(), // outside circle
            [1.0, 1.0, 10.0].as_slice(),  // outside interval
        ]
        .iter()
        .copied(),
    );

    let out = prism.transform(&pts, true).unwrap();

    // First point passes through.
    let p0 = out.point(0);
    assert!((p0[0] - 1.0).abs() < 1e-10);
    assert!((p0[1] - 1.0).abs() < 1e-10);
    assert!((p0[2] - 5.0).abs() < 1e-10);

    // Second and third points are BAD.
    let p1 = out.point(1);
    assert_eq!(p1[0], BAD_VALUE);
    assert_eq!(p1[1], BAD_VALUE);
    assert_eq!(p1[2], BAD_VALUE);

    let p2 = out.point(2);
    assert_eq!(p2[0], BAD_VALUE);
    assert_eq!(p2[1], BAD_VALUE);
    assert_eq!(p2[2], BAD_VALUE);
}

#[test]
fn inverse_transform_passes_all() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows(
        [
            [10.0, 10.0, 10.0].as_slice(), // outside both
        ]
        .iter()
        .copied(),
    );

    let out = prism.transform(&pts, false).unwrap();
    let p0 = out.point(0);
    assert!((p0[0] - 10.0).abs() < 1e-10);
    assert!((p0[1] - 10.0).abs() < 1e-10);
    assert!((p0[2] - 10.0).abs() < 1e-10);
}

// ---- Sub-region access ----

#[test]
fn access_sub_regions() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(prism.region1().naxes(), 2);
    assert_eq!(prism.region2().naxes(), 1);
}

// ---- Frame properties ----

#[test]
fn frame_naxes_matches_combined() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    // As a Frame, naxes should be the combined count.
    let frame_naxes = Frame::naxes(&prism);
    assert_eq!(frame_naxes, 3);
}

// ---- Mapping properties ----

#[test]
fn mapping_nin_nout_match() {
    let prism = PrismBuilder::new(make_circle(), make_interval())
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(prism.nin(), 3);
    assert_eq!(prism.nout(), 3);
}

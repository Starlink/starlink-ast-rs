//! Tests for the CmpRegion implementation.
//!
//! Covers: AND (intersection), OR (union), negation, batch transform,
//! is_bounded logic, construction validation.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{
    BoxRegionBuilder, CircleBuilder, CmpOperator, CmpRegionBuilder, NullRegionBuilder, Region,
};

// ---- Construction ----

#[test]
fn construct_and_region() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(cmp.naxes(), 2);
    assert_eq!(cmp.operator(), CmpOperator::And);
}

#[test]
fn construct_or_region() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c1 = CircleBuilder::new(frame.clone(), vec![-5.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![5.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(cmp.naxes(), 2);
    assert_eq!(cmp.operator(), CmpOperator::Or);
}

/// Regions with different numbers of axes have no Mapping between their
/// Frames, so building the CmpRegion fails with `Intrd`, the error
/// `astCmpRegion` reports for a 2-D Circle and a 3-D Box (C-oracle probe).
#[test]
fn construct_mismatched_naxes_fails() {
    let frame2 = BasicFrameBuilder::new(2).build().unwrap();
    let frame3 = BasicFrameBuilder::new(3).build().unwrap();

    let c1 = CircleBuilder::new(frame2, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame3, vec![0.0, 0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();

    let err = CmpRegionBuilder::new(c1, c2, CmpOperator::And)
        .and_then(|b| b.build())
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Intrd);
}

// ---- AND (intersection) point tests ----

#[test]
fn and_circle_and_box() {
    // Circle of radius 5 centred at origin, AND box [-3,3] x [-3,3].
    // The intersection is the area inside both.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // Origin: inside both -> inside AND.
    assert!(cmp.point_in_region(&[0.0, 0.0]).unwrap());

    // (2, 2): inside circle (dist=2.83<5) and inside box -> inside.
    assert!(cmp.point_in_region(&[2.0, 2.0]).unwrap());

    // (4, 0): inside circle (dist=4<5) but outside box (4>3) -> outside AND.
    assert!(!cmp.point_in_region(&[4.0, 0.0]).unwrap());

    // (-4, 0): inside circle but outside box -> outside.
    assert!(!cmp.point_in_region(&[-4.0, 0.0]).unwrap());

    // (6, 0): outside both -> outside.
    assert!(!cmp.point_in_region(&[6.0, 0.0]).unwrap());
}

// ---- OR (union) point tests ----

#[test]
fn or_two_separated_circles() {
    // Two circles: centred at (-5,0) and (5,0), both radius 2.
    // They don't overlap.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c1 = CircleBuilder::new(frame.clone(), vec![-5.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![5.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    // Inside first circle.
    assert!(cmp.point_in_region(&[-5.0, 0.0]).unwrap());
    assert!(cmp.point_in_region(&[-4.0, 0.0]).unwrap());

    // Inside second circle.
    assert!(cmp.point_in_region(&[5.0, 0.0]).unwrap());
    assert!(cmp.point_in_region(&[6.0, 0.0]).unwrap());

    // Between the circles (origin): outside both -> outside OR.
    assert!(!cmp.point_in_region(&[0.0, 0.0]).unwrap());

    // Far away: outside both -> outside.
    assert!(!cmp.point_in_region(&[0.0, 10.0]).unwrap());
}

// ---- Negation ----

#[test]
fn negated_cmpregion() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let mut cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // Before negation: origin is inside.
    assert!(cmp.point_in_region(&[0.0, 0.0]).unwrap());
    // (4,0) is outside.
    assert!(!cmp.point_in_region(&[4.0, 0.0]).unwrap());

    // Negate.
    cmp.negate();
    assert!(cmp.is_negated());

    // After negation: origin is outside, (4,0) is inside.
    assert!(!cmp.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(cmp.point_in_region(&[4.0, 0.0]).unwrap());
}

#[test]
fn negated_copy() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    let neg = cmp.negated_copy();

    // Original: origin is inside (inside both circles).
    assert!(cmp.point_in_region(&[0.0, 0.0]).unwrap());
    // Negated copy: origin is outside.
    assert!(!neg.point_in_region(&[0.0, 0.0]).unwrap());

    // Far away point: original says outside, negated says inside.
    assert!(!cmp.point_in_region(&[10.0, 10.0]).unwrap());
    assert!(neg.point_in_region(&[10.0, 10.0]).unwrap());
}

// ---- Batch transform (Mapping) ----

#[test]
fn transform_forward_masks_outside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![2.0, 2.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // Points: (1,1) inside AND, (2.5, 0) inside circle but outside box, (5,5) outside both.
    let pts = PointSet::from_rows([
        [1.0, 1.0].as_slice(),
        [2.5, 0.0].as_slice(),
        [5.0, 5.0].as_slice(),
    ]);

    let result = cmp.transform(&pts, true).unwrap();
    assert_eq!(result.npoint(), 3);

    // Point 0: inside -> passes through.
    let p0 = result.point(0);
    assert!((p0[0] - 1.0).abs() < 1e-10);
    assert!((p0[1] - 1.0).abs() < 1e-10);

    // Point 1: outside AND -> BAD.
    let p1 = result.point(1);
    assert_eq!(p1[0], BAD_VALUE);
    assert_eq!(p1[1], BAD_VALUE);

    // Point 2: outside both -> BAD.
    let p2 = result.point(2);
    assert_eq!(p2[0], BAD_VALUE);
    assert_eq!(p2[1], BAD_VALUE);
}

#[test]
fn transform_inverse_masks_as_forward() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![2.0, 2.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[10.0, 10.0].as_slice(), [0.0, 0.0].as_slice()]);

    // C's Region classes transform identically in both directions, so a
    // point outside the Region is bad on the inverse too.
    let inverse = cmp.transform(&pts, false).unwrap();
    let forward = cmp.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        assert_eq!(inverse.point(i), forward.point(i), "point {i}");
    }
    assert!(inverse.point(0).iter().all(|&v| v == BAD_VALUE));
}

// ---- is_bounded logic ----

#[test]
fn and_bounded_if_either_bounded() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();

    // A bounded circle.
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    // A negated circle: the exterior of a disc is unbounded.
    let mut unbounded = CircleBuilder::new(frame.clone(), vec![10.0, 10.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    unbounded.negate();
    assert!(!unbounded.is_bounded().unwrap());

    // AND of bounded + unbounded -> bounded (intersection can't be larger
    // than the bounded component). C cmpregion.c GetBounded: result = 1 when
    // either component is bounded.
    let cmp = CmpRegionBuilder::new(circle, unbounded, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    assert!(cmp.is_bounded().unwrap());
}

#[test]
fn null_region_is_bounded() {
    // An un-negated NullRegion is the empty set, which C treats as bounded
    // (nullregion.c has no GetBounded override, so it inherits the base rule
    // region.c:3358 -> !Negated). Its negated form (the whole space) is not.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let null = NullRegionBuilder::new(frame.clone()).build().unwrap();
    assert!(null.is_bounded().unwrap());

    let mut neg_null = NullRegionBuilder::new(frame).build().unwrap();
    neg_null.negate();
    assert!(!neg_null.is_bounded().unwrap());
}

#[test]
fn and_unbounded_if_both_unbounded() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();

    // Two negated circles centred far apart: both exteriors are unbounded
    // and their overlap is Partial (C overlap code 4). C cmpregion.c
    // GetBounded: when neither AND component is bounded, the result is
    // unbounded for overlap codes 1/4/6.
    let mut a = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    a.negate();
    let mut b = CircleBuilder::new(frame, vec![10.0, 10.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    b.negate();

    let cmp = CmpRegionBuilder::new(a, b, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    assert!(!cmp.is_bounded().unwrap());
}

#[test]
fn or_bounded_only_if_both_bounded() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();

    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    // A negated circle is unbounded; OR with an unbounded component is
    // unbounded (C cmpregion.c GetBounded: result = 0 if either OR component
    // is unbounded).
    let mut unbounded = CircleBuilder::new(frame.clone(), vec![10.0, 10.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    unbounded.negate();

    let cmp = CmpRegionBuilder::new(circle.clone(), unbounded, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();
    assert!(!cmp.is_bounded().unwrap());

    // OR of two bounded -> bounded.
    let circle2 = CircleBuilder::new(frame, vec![1.0, 1.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let cmp2 = CmpRegionBuilder::new(circle, circle2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();
    assert!(cmp2.is_bounded().unwrap());
}

// ---- Bounding box ----

#[test]
fn and_bounding_box_is_intersection() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();

    // Circle centred at (0,0) with radius 5: box is [-5,5] x [-5,5].
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    // Box [-3,3] x [-3,3] (centre (0,0), corner (3,3)).
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = cmp.get_region_bounds().unwrap();
    // Intersection: max of lower bounds, min of upper bounds.
    // max(-5, -3) = -3, min(5, 3) = 3
    assert!((lbnd[0] - (-3.0)).abs() < 1e-10);
    assert!((lbnd[1] - (-3.0)).abs() < 1e-10);
    assert!((ubnd[0] - 3.0).abs() < 1e-10);
    assert!((ubnd[1] - 3.0).abs() < 1e-10);
}

#[test]
fn or_bounding_box_is_union() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();

    // Circle at (-5, 0) radius 2: box [-7,-3] x [-2,2].
    let c1 = CircleBuilder::new(frame.clone(), vec![-5.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    // Circle at (5, 0) radius 2: box [3,7] x [-2,2].
    let c2 = CircleBuilder::new(frame, vec![5.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = cmp.get_region_bounds().unwrap();
    // Union: min of lower bounds, max of upper bounds.
    // min(-7, 3) = -7, max(-3, 7) = 7
    assert!((lbnd[0] - (-7.0)).abs() < 1e-10);
    assert!((ubnd[0] - 7.0).abs() < 1e-10);
    // Y axis: min(-2,-2) = -2, max(2,2) = 2
    assert!((lbnd[1] - (-2.0)).abs() < 1e-10);
    assert!((ubnd[1] - 2.0).abs() < 1e-10);
}

// ---- BAD value handling ----

#[test]
fn bad_input_point_is_outside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // A point with BAD coordinate should be outside.
    assert!(!cmp.point_in_region(&[BAD_VALUE, 0.0]).unwrap());
    assert!(!cmp.point_in_region(&[0.0, BAD_VALUE]).unwrap());
}

// ---- Nested CmpRegion ----

#[test]
fn nested_cmpregion() {
    // (circle1 AND box) OR circle2 — test nested compound regions.
    let frame = BasicFrameBuilder::new(2).build().unwrap();

    let circle1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::new(frame.clone(), vec![0.0, 0.0], vec![2.0, 2.0])
        .unwrap()
        .build()
        .unwrap();
    let circle2 = CircleBuilder::new(frame, vec![10.0, 10.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();

    // Inner: circle1 AND box (intersection).
    let inner = CmpRegionBuilder::new(circle1, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // Outer: inner OR circle2 (union).
    let outer = CmpRegionBuilder::new(inner, circle2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    // (1,1): inside circle1 and box -> inside inner -> inside outer.
    assert!(outer.point_in_region(&[1.0, 1.0]).unwrap());

    // (2.5, 0): inside circle1 but outside box -> outside inner.
    // Also outside circle2 -> outside outer.
    assert!(!outer.point_in_region(&[2.5, 0.0]).unwrap());

    // (10.0, 10.0): centre of circle2 -> inside circle2 -> inside outer.
    assert!(outer.point_in_region(&[10.0, 10.0]).unwrap());

    // (20, 20): outside everything -> outside.
    assert!(!outer.point_in_region(&[20.0, 20.0]).unwrap());
}

// ---- from_dyn builder ----

#[test]
fn from_dyn_builder() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c1: Box<dyn Region> = Box::new(
        CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
            .unwrap()
            .radius(3.0)
            .build()
            .unwrap(),
    );
    let c2: Box<dyn Region> = Box::new(
        CircleBuilder::new(frame, vec![0.0, 0.0])
            .unwrap()
            .radius(1.0)
            .build()
            .unwrap(),
    );

    let cmp = CmpRegionBuilder::from_dyn(c1, c2, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // (0,0) is inside both -> inside.
    assert!(cmp.point_in_region(&[0.0, 0.0]).unwrap());
    // (2,0) is inside big circle but outside small -> outside AND.
    assert!(!cmp.point_in_region(&[2.0, 0.0]).unwrap());
}

// ---- Mesh generation ----

#[test]
fn and_mesh_is_nonempty_and_on_boundary() {
    // Intersection of a circle (radius 5) and a box [-3,3]x[-3,3].
    // The mesh should contain points that are on the boundary of the
    // intersection: circle boundary inside the box, OR box boundary
    // inside the circle.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::new(frame, vec![0.0, 0.0], vec![3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let mesh = cmp.base_mesh().unwrap();
    let npoint = mesh.npoint();

    // Mesh should not be empty.
    assert!(npoint > 0, "AND mesh should be non-empty");

    // All mesh points should have 2 coordinates and no BAD values.
    for i in 0..npoint {
        let p = mesh.point(i);
        assert_eq!(p.len(), 2);
        assert!(p[0] != BAD_VALUE && p[1] != BAD_VALUE);
    }

    // Mesh points should lie on the boundary of the intersection.
    // For the AND of circle(r=5) and box([-3,3]x[-3,3]):
    // - Points from the circle mesh that are inside the box (|x|<=3 && |y|<=3)
    // - Points from the box mesh that are inside the circle (x^2+y^2<=25)
    for i in 0..npoint {
        let p = mesh.point(i);
        let x = p[0];
        let y = p[1];
        let dist_sq = x * x + y * y;
        let in_circle = dist_sq <= 25.0 + 1e-6;
        let in_box = x.abs() <= 3.0 + 1e-6 && y.abs() <= 3.0 + 1e-6;
        // Each mesh point should be inside both regions (on the boundary
        // of one and inside the other).
        assert!(
            in_circle && in_box,
            "Mesh point ({}, {}) should be inside both regions",
            x,
            y
        );
    }
}

#[test]
fn or_mesh_is_nonempty_and_on_boundary() {
    // Union of two separated circles. The mesh should contain points
    // on the boundary of each circle that are OUTSIDE the other circle.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c1 = CircleBuilder::new(frame.clone(), vec![-5.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![5.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    let mesh = cmp.base_mesh().unwrap();
    let npoint = mesh.npoint();

    // Mesh should not be empty.
    assert!(npoint > 0, "OR mesh should be non-empty");

    // All mesh points should be on the boundary of the union:
    // boundary of c1 outside c2, or boundary of c2 outside c1.
    // Since these circles don't overlap, all boundary points of both
    // should appear in the mesh.
    for i in 0..npoint {
        let p = mesh.point(i);
        assert_eq!(p.len(), 2);
        assert!(p[0] != BAD_VALUE && p[1] != BAD_VALUE);
    }

    // For two separated circles, the mesh should contain points from
    // both circles' boundaries. Check we have points on both sides.
    let has_left = (0..npoint).any(|i| mesh.point(i)[0] < 0.0);
    let has_right = (0..npoint).any(|i| mesh.point(i)[0] > 0.0);
    assert!(has_left, "OR mesh should have points from left circle");
    assert!(has_right, "OR mesh should have points from right circle");
}

#[test]
fn and_mesh_overlapping_circles() {
    // Two overlapping circles: both centred on the y-axis, overlapping in
    // the middle. The AND mesh should only have points in the overlap zone.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c1 = CircleBuilder::new(frame.clone(), vec![0.0, -1.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![0.0, 1.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let mesh = cmp.base_mesh().unwrap();
    let npoint = mesh.npoint();
    assert!(
        npoint > 0,
        "Overlapping circles AND mesh should be non-empty"
    );

    // All points should be inside both circles.
    for i in 0..npoint {
        let p = mesh.point(i);
        let x = p[0];
        let y = p[1];
        let dist1_sq = x * x + (y + 1.0) * (y + 1.0);
        let dist2_sq = x * x + (y - 1.0) * (y - 1.0);
        assert!(
            dist1_sq <= 9.0 + 1e-6 && dist2_sq <= 9.0 + 1e-6,
            "AND mesh point ({}, {}) should be inside both circles (d1={}, d2={})",
            x,
            y,
            dist1_sq.sqrt(),
            dist2_sq.sqrt()
        );
    }
}

// ---- Accessor methods ----

#[test]
fn accessor_methods() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let c1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frame, vec![1.0, 1.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    // Check that region1/region2 accessors work.
    assert_eq!(cmp.region1().naxes(), 2);
    assert_eq!(cmp.region2().naxes(), 2);

    // Check the frame reports correct naxes.
    assert_eq!(cmp.naxes(), 2);
    assert_eq!(cmp.nin(), 2);
    assert_eq!(cmp.nout(), 2);
}

/// cmpregion.c `SetClosed` resets the cache, and `RegBaseMesh` keeps only
/// the component mesh points inside the other component, so the mesh of an
/// AND whose components share part of an edge depends on Closed.
#[test]
fn and_mesh_follows_closed_after_first_use() {
    let build = |closed: bool| {
        let frame = BasicFrameBuilder::new(2).build().unwrap();
        let a = BoxRegionBuilder::new(frame.clone(), vec![0.0, 0.0], vec![1.0, 1.0])
            .unwrap()
            .build()
            .unwrap();
        let b = BoxRegionBuilder::new(frame, vec![1.0, 0.0], vec![2.0, 1.0])
            .unwrap()
            .build()
            .unwrap();
        let mut cmp = CmpRegionBuilder::new(a, b, CmpOperator::And)
            .unwrap()
            .build()
            .unwrap();
        cmp.set_closed(closed);
        cmp
    };
    let fresh_open = build(false).base_mesh().unwrap();
    let mut warmed = build(true);
    let closed_mesh = warmed.base_mesh().unwrap();
    assert_ne!(
        closed_mesh.npoint(),
        fresh_open.npoint(),
        "the fixture must make Closed matter"
    );
    warmed.set_closed(false);
    assert_eq!(warmed.base_mesh().unwrap().npoint(), fresh_open.npoint());
}

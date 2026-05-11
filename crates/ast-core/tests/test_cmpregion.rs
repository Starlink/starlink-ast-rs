//! Tests for the CmpRegion implementation.
//!
//! Covers: AND (intersection), OR (union), negation, batch transform,
//! is_bounded logic, construction validation.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
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

    let result = CmpRegionBuilder::new(c1, c2, CmpOperator::And);
    assert!(result.is_err());
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
    let pts = PointSet::from_rows(
        [
            [1.0, 1.0].as_slice(),
            [2.5, 0.0].as_slice(),
            [5.0, 5.0].as_slice(),
        ]
        .into_iter(),
    );

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
fn transform_inverse_passes_all() {
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

    let pts = PointSet::from_rows([[10.0, 10.0].as_slice(), [0.0, 0.0].as_slice()].into_iter());

    // Inverse transform always passes points through unchanged.
    let result = cmp.transform(&pts, false).unwrap();
    let p0 = result.point(0);
    assert!((p0[0] - 10.0).abs() < 1e-10);
    assert!((p0[1] - 10.0).abs() < 1e-10);
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

    // A NullRegion (unbounded).
    let null = NullRegionBuilder::new(frame.clone()).build().unwrap();

    // AND of bounded + unbounded -> bounded (intersection can't be larger
    // than the bounded component).
    let cmp = CmpRegionBuilder::new(circle, null, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    assert!(cmp.is_bounded());
}

#[test]
fn and_unbounded_if_both_unbounded() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let null1 = NullRegionBuilder::new(frame.clone()).build().unwrap();
    let null2 = NullRegionBuilder::new(frame).build().unwrap();

    // AND of two unbounded -> unbounded.
    let cmp = CmpRegionBuilder::new(null1, null2, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    assert!(!cmp.is_bounded());
}

#[test]
fn or_bounded_only_if_both_bounded() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();

    let circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(3.0)
        .build()
        .unwrap();
    let null = NullRegionBuilder::new(frame.clone()).build().unwrap();

    // OR of bounded + unbounded -> unbounded.
    let cmp = CmpRegionBuilder::new(circle.clone(), null, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();
    assert!(!cmp.is_bounded());

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
    assert!(cmp2.is_bounded());
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

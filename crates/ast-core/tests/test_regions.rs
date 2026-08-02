//! Integration tests ported from reference/starlink-ast/ast_tester/testregions.c
//!
//! These test cross-class interactions (compound regions, prisms, bounds
//! computation through compound regions, overlap detection) on both
//! BasicFrame and SkyFrame. The SkyFrame tests verify that regions
//! correctly use great-circle (spherical) geometry for distance
//! computations, membership tests, and overlap detection.

use std::f64::consts::{FRAC_PI_2, PI};

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::fitschan::FitsChan;
use ast_core::frames::{
    BasicFrameBuilder, Frame, FrameRef, FrameSet, SkyFrame, SkySystem, pick_axes,
};
use ast_core::mappings::Mapping;
use ast_core::regions::{
    BoxRegion, BoxRegionBuilder, Circle, CircleBuilder, CmpOperator, CmpRegion, CmpRegionBuilder,
    EllipseBuilder, Interval, IntervalBuilder, Overlap, PickedAxes, PolygonBuilder, PrismBuilder,
    Region, pick_axes_region, remove_regions,
};

/// Check whether a value represents "unbounded below" (very large negative).
fn is_unbounded_lower(v: f64) -> bool {
    v < 0.99 * -1.7e308
}

/// Check whether a value represents "unbounded above" (very large positive).
fn is_unbounded_upper(v: f64) -> bool {
    v > 0.99 * 1.7e308
}

// ============================================================================
// generalChecks (lines 55-131 of testregions.c)
//
// Tests bounds of compound regions (CmpRegion OR/AND with Interval + Circle),
// bounds of Prisms composed of compound regions, and how negation affects
// bounds. Uses only BasicFrame.
// ============================================================================

#[test]
fn general_interval_bounds() {
    // C: lbnd[0] = 0.0, lbnd[1] = AST__BAD, ubnd[0] = AST__BAD, ubnd[1] = 0.0
    // Interval with: axis 1 >= 0.0, axis 2 <= 0.0
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let reg = IntervalBuilder::new(frm, vec![0.0, BAD_VALUE], vec![BAD_VALUE, 0.0])
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = reg.get_region_bounds().unwrap();

    // General 1: lbnd[0] == 0.0
    assert_eq!(lbnd[0], 0.0, "General 1");
    // General 2: lbnd[1] unbounded below
    assert!(is_unbounded_lower(lbnd[1]), "General 2");
    // General 3: ubnd[0] unbounded above
    assert!(is_unbounded_upper(ubnd[0]), "General 3");
    // General 4: ubnd[1] == 0.0
    assert_eq!(ubnd[1], 0.0, "General 4");
}

#[test]
fn general_circle_bounds_and_disc() {
    // C: circle centred at (0,0) with edge point at (1,0) -> radius 1
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frm, vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, 0.0])
        .build()
        .unwrap();

    let (lbnd, ubnd) = circle.get_region_bounds().unwrap();

    // General 5-8: bounds are [-1, -1] to [1, 1]
    assert_eq!(lbnd[0], -1.0, "General 5");
    assert_eq!(lbnd[1], -1.0, "General 6");
    assert_eq!(ubnd[0], 1.0, "General 7");
    assert_eq!(ubnd[1], 1.0, "General 8");

    // General 8a-8c: get_region_disc
    let (centre, radius) = circle.get_region_disc().unwrap();
    assert!(centre[0].abs() < 1.0e-5, "General 8a");
    assert!(centre[1].abs() < 1.0e-5, "General 8b");
    assert!((radius - 1.0).abs() < 1.0e-5, "General 8c");
}

#[test]
fn general_cmpregion_or_bounds() {
    // OR of Interval (axis 1 >= 0, axis 2 <= 0) with Circle (radius 1 at origin)
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frm.clone(), vec![0.0, BAD_VALUE], vec![BAD_VALUE, 0.0])
        .unwrap()
        .build()
        .unwrap();
    let circle = CircleBuilder::new(frm, vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, 0.0])
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(interval, circle, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = cmp.get_region_bounds().unwrap();

    // General 9: lbnd[0] == -1.0 (from circle's lower bound)
    assert_eq!(lbnd[0], -1.0, "General 9");
    // General 10: lbnd[1] unbounded below (from interval's axis 2)
    assert!(is_unbounded_lower(lbnd[1]), "General 10");
    // General 11: ubnd[0] unbounded above (from interval's axis 1)
    assert!(is_unbounded_upper(ubnd[0]), "General 11");
    // General 12: ubnd[1] == 1.0 (from circle's upper bound)
    assert_eq!(ubnd[1], 1.0, "General 12");
}

#[test]
fn general_prism_bounds() {
    // Prism: (Interval OR Circle in 2D) extruded by 1D interval [-1, 1]
    let frm2 = BasicFrameBuilder::new(2).build().unwrap();
    let interval2d = IntervalBuilder::new(frm2.clone(), vec![0.0, BAD_VALUE], vec![BAD_VALUE, 0.0])
        .unwrap()
        .build()
        .unwrap();
    let circle = CircleBuilder::new(frm2, vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, 0.0])
        .build()
        .unwrap();
    let cmp = CmpRegionBuilder::new(interval2d, circle, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval1d = IntervalBuilder::new(frm1, vec![-1.0], vec![1.0])
        .unwrap()
        .build()
        .unwrap();

    // Verify 1D interval bounds
    let (lb1d, ub1d) = interval1d.get_region_bounds().unwrap();
    assert_eq!(lb1d[0], -1.0, "General 13");
    assert_eq!(ub1d[0], 1.0, "General 14");

    // Prism = 2D CmpRegion + 1D Interval = 3D region
    let prism = PrismBuilder::new(cmp, interval1d).unwrap().build().unwrap();

    let (lbnd, ubnd) = prism.get_region_bounds().unwrap();

    // General 15: lbnd[0] == -1.0
    assert_eq!(lbnd[0], -1.0, "General 15");
    // General 16: lbnd[1] unbounded below
    assert!(is_unbounded_lower(lbnd[1]), "General 16");
    // General 17: ubnd[0] unbounded above
    assert!(is_unbounded_upper(ubnd[0]), "General 17");
    // General 18: ubnd[1] == 1.0
    assert_eq!(ubnd[1], 1.0, "General 18");
    // General 19: lbnd[2] == -1.0 (from 1D interval)
    assert_eq!(lbnd[2], -1.0, "General 19");
    // General 20: ubnd[2] == 1.0 (from 1D interval)
    assert_eq!(ubnd[2], 1.0, "General 20");
}

#[test]
fn general_negated_circle_or_bounds() {
    // When circle is negated, its complement is unbounded in all directions.
    // OR of Interval with negated Circle should be unbounded everywhere.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frm.clone(), vec![0.0, BAD_VALUE], vec![BAD_VALUE, 0.0])
        .unwrap()
        .build()
        .unwrap();
    let mut circle = CircleBuilder::new(frm, vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, 0.0])
        .build()
        .unwrap();
    circle.negate();

    let cmp = CmpRegionBuilder::new(interval, circle, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = cmp.get_region_bounds().unwrap();

    // General 21-24: all unbounded
    assert!(is_unbounded_lower(lbnd[0]), "General 21");
    assert!(is_unbounded_lower(lbnd[1]), "General 22");
    assert!(is_unbounded_upper(ubnd[0]), "General 23");
    assert!(is_unbounded_upper(ubnd[1]), "General 24");
}

#[test]
fn general_negated_circle_prism_bounds() {
    // Prism of (Interval OR negated Circle) + 1D interval
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frm.clone(), vec![0.0, BAD_VALUE], vec![BAD_VALUE, 0.0])
        .unwrap()
        .build()
        .unwrap();
    let mut circle = CircleBuilder::new(frm, vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, 0.0])
        .build()
        .unwrap();
    circle.negate();

    let cmp = CmpRegionBuilder::new(interval, circle, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval1d = IntervalBuilder::new(frm1, vec![-1.0], vec![1.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(cmp, interval1d).unwrap().build().unwrap();
    let (lbnd, ubnd) = prism.get_region_bounds().unwrap();

    // General 25-28: axes 1,2 unbounded
    assert!(is_unbounded_lower(lbnd[0]), "General 25");
    assert!(is_unbounded_lower(lbnd[1]), "General 26");
    assert!(is_unbounded_upper(ubnd[0]), "General 27");
    assert!(is_unbounded_upper(ubnd[1]), "General 28");
    // General 29-30: axis 3 bounded by 1D interval
    assert_eq!(lbnd[2], -1.0, "General 29");
    assert_eq!(ubnd[2], 1.0, "General 30");
}

#[test]
fn general_negated_circle_and_bounds() {
    // AND of Interval with negated Circle: the intersection is Interval minus
    // the interior of the circle. The bounds come from the Interval since the
    // negated circle doesn't constrain tighter.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frm.clone(), vec![0.0, BAD_VALUE], vec![BAD_VALUE, 0.0])
        .unwrap()
        .build()
        .unwrap();
    let mut circle = CircleBuilder::new(frm, vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, 0.0])
        .build()
        .unwrap();
    circle.negate();

    let cmp = CmpRegionBuilder::new(interval, circle, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = cmp.get_region_bounds().unwrap();

    // General 31: lbnd[0] == 0.0 (from interval lower bound on axis 1)
    assert_eq!(lbnd[0], 0.0, "General 31");
    // General 32: lbnd[1] unbounded below
    assert!(is_unbounded_lower(lbnd[1]), "General 32");
    // General 33: ubnd[0] unbounded above
    assert!(is_unbounded_upper(ubnd[0]), "General 33");
    // General 34: ubnd[1] == 0.0 (from interval upper bound on axis 2)
    assert_eq!(ubnd[1], 0.0, "General 34");
}

#[test]
fn general_negated_circle_and_prism_bounds() {
    // Prism of (Interval AND negated Circle) + 1D interval
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frm.clone(), vec![0.0, BAD_VALUE], vec![BAD_VALUE, 0.0])
        .unwrap()
        .build()
        .unwrap();
    let mut circle = CircleBuilder::new(frm, vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, 0.0])
        .build()
        .unwrap();
    circle.negate();

    let cmp = CmpRegionBuilder::new(interval, circle, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval1d = IntervalBuilder::new(frm1, vec![-1.0], vec![1.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(cmp, interval1d).unwrap().build().unwrap();
    let (lbnd, ubnd) = prism.get_region_bounds().unwrap();

    // General 35: lbnd[0] == 0.0
    assert_eq!(lbnd[0], 0.0, "General 35");
    // General 36: lbnd[1] unbounded below
    assert!(is_unbounded_lower(lbnd[1]), "General 36");
    // General 37: ubnd[0] unbounded above
    assert!(is_unbounded_upper(ubnd[0]), "General 37");
    // General 38: ubnd[1] == 0.0
    assert_eq!(ubnd[1], 0.0, "General 38");
    // General 39-40: axis 3 bounded
    assert_eq!(lbnd[2], -1.0, "General 39");
    assert_eq!(ubnd[2], 1.0, "General 40");
}

// ============================================================================
// checkCircle — 3D sphere tests (lines 1792-1898 of testregions.c)
//
// Uses BasicFrame(3) to test 3D circles (spheres), their transforms,
// and overlap detection.
// ============================================================================

#[test]
fn sphere_3d_transform() {
    // Create a 3D circle (sphere) centred at (1, 2, 3) with edge at (0, -1, -2).
    // The radius = distance from (1,2,3) to (0,-1,-2) = sqrt(1 + 9 + 25) = sqrt(35).
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let circle = CircleBuilder::new(frm.clone(), vec![1.0, 2.0, 3.0])
        .unwrap()
        .edge_point(vec![0.0, -1.0, -2.0])
        .build()
        .unwrap();

    let rad = (1.0_f64 + 9.0 + 25.0).sqrt(); // sqrt(35)

    // Point at 99.9% of radius along direction to (0,-1,-2): should be inside.
    let dir = [-1.0_f64, -3.0, -5.0];
    let dir_len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    let scale_in = rad * 0.999 / dir_len;
    let p_inside = [
        1.0 + dir[0] * scale_in,
        2.0 + dir[1] * scale_in,
        3.0 + dir[2] * scale_in,
    ];

    // Point at 100.1% of radius: should be outside.
    let scale_out = rad * 1.001 / dir_len;
    let p_outside = [
        1.0 + dir[0] * scale_out,
        2.0 + dir[1] * scale_out,
        3.0 + dir[2] * scale_out,
    ];

    let pts = PointSet::from_rows([p_inside, p_outside]);
    let result = circle.transform(&pts, true).unwrap();

    // Inside point passes through.
    assert_eq!(result.point(0)[0], p_inside[0], "Sphere: Error 1");
    assert_eq!(result.point(0)[1], p_inside[1], "Sphere: Error 2");
    assert_eq!(result.point(0)[2], p_inside[2], "Sphere: Error 2z");

    // Outside point becomes BAD.
    assert_eq!(result.point(1)[0], BAD_VALUE, "Sphere: Error 3");
    assert_eq!(result.point(1)[1], BAD_VALUE, "Sphere: Error 4");
    assert_eq!(result.point(1)[2], BAD_VALUE, "Sphere: Error 4z");
}

#[test]
fn sphere_3d_transform_opposite_direction() {
    // Test along direction from centre (1,2,3) towards (0,0,0).
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let circle = CircleBuilder::new(frm, vec![1.0, 2.0, 3.0])
        .unwrap()
        .edge_point(vec![0.0, -1.0, -2.0])
        .build()
        .unwrap();

    let rad = (1.0_f64 + 9.0 + 25.0).sqrt();

    // Direction from (1,2,3) towards (0,0,0)
    let dir = [-1.0_f64, -2.0, -3.0];
    let dir_len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();

    let scale_in = rad * 0.999 / dir_len;
    let p_inside = [
        1.0 + dir[0] * scale_in,
        2.0 + dir[1] * scale_in,
        3.0 + dir[2] * scale_in,
    ];

    let scale_out = rad * 1.001 / dir_len;
    let p_outside = [
        1.0 + dir[0] * scale_out,
        2.0 + dir[1] * scale_out,
        3.0 + dir[2] * scale_out,
    ];

    let pts = PointSet::from_rows([p_inside, p_outside]);
    let result = circle.transform(&pts, true).unwrap();

    // Inside passes through.
    assert_eq!(result.point(0)[0], p_inside[0], "Sphere: Error 5");
    assert_eq!(result.point(0)[1], p_inside[1], "Sphere: Error 6");
    assert_eq!(result.point(0)[2], p_inside[2], "Sphere: Error 6z");

    // Outside becomes BAD.
    assert_eq!(result.point(1)[0], BAD_VALUE, "Sphere: Error 7");
    assert_eq!(result.point(1)[1], BAD_VALUE, "Sphere: Error 8");
    assert_eq!(result.point(1)[2], BAD_VALUE, "Sphere: Error 8z");
}

#[test]
fn sphere_3d_overlap_same() {
    // Self-overlap should be Same.
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let circle = CircleBuilder::new(frm, vec![1.0, 2.0, 3.0])
        .unwrap()
        .edge_point(vec![0.0, -1.0, -2.0])
        .build()
        .unwrap();

    assert_eq!(
        circle.overlap(&circle).unwrap(),
        Overlap::Same,
        "Sphere: Error 10"
    );
}

#[test]
fn sphere_3d_overlap_first_inside() {
    // Smaller sphere inside larger one at same centre.
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let big = CircleBuilder::new(frm.clone(), vec![1.0, 2.0, 3.0])
        .unwrap()
        .edge_point(vec![0.0, -1.0, -2.0])
        .build()
        .unwrap();

    let small = CircleBuilder::new(frm, vec![1.0, 2.0, 3.0])
        .unwrap()
        .edge_point(vec![0.5, 0.0, -1.0])
        .build()
        .unwrap();

    // Small is entirely inside big -> FirstInside when small.overlap(big)
    assert_eq!(
        small.overlap(&big).unwrap(),
        Overlap::FirstInside,
        "Sphere: Error 11"
    );
    // big.overlap(small) -> SecondInside
    assert_eq!(
        big.overlap(&small).unwrap(),
        Overlap::SecondInside,
        "Sphere: Error 12"
    );
}

#[test]
fn sphere_3d_overlap_partial() {
    // Overlapping spheres: same centre axis 1 and 3, offset on axis 2.
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let c1 = CircleBuilder::new(frm.clone(), vec![1.0, 2.0, 3.0])
        .unwrap()
        .edge_point(vec![0.0, -1.0, -2.0])
        .build()
        .unwrap();

    // Sphere at (1, 0, 3) with same radius direction — overlaps partially.
    let c2 = CircleBuilder::new(frm, vec![1.0, 0.0, 3.0])
        .unwrap()
        .edge_point(vec![0.0, -1.0, -2.0])
        .build()
        .unwrap();

    assert_eq!(
        c1.overlap(&c2).unwrap(),
        Overlap::Partial,
        "Sphere: Error 13"
    );
}

#[test]
fn sphere_3d_overlap_none() {
    // Distant spheres: no overlap.
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let c1 = CircleBuilder::new(frm.clone(), vec![1.0, 2.0, 3.0])
        .unwrap()
        .edge_point(vec![0.0, -1.0, -2.0])
        .build()
        .unwrap();

    // Sphere at (1, 102, 3) — far away.
    let c2 = CircleBuilder::new(frm, vec![1.0, 102.0, 3.0])
        .unwrap()
        .edge_point(vec![0.0, 99.0, -2.0])
        .build()
        .unwrap();

    assert_eq!(c1.overlap(&c2).unwrap(), Overlap::None, "Sphere: Error 14");
}

#[test]
fn sphere_3d_overlap_touching_closed() {
    // Two spheres that touch at exactly one point (closed boundaries).
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let c1 = CircleBuilder::new(frm.clone(), vec![0.0, 0.0, 0.0])
        .unwrap()
        .edge_point(vec![0.0, 0.0, 1.0])
        .build()
        .unwrap();

    // Sphere at (2.000001, 0, 0) with radius ~1 — barely touching.
    let c2 = CircleBuilder::new(frm, vec![2.000001, 0.0, 0.0])
        .unwrap()
        .edge_point(vec![2.000001, 1.0, 0.0])
        .build()
        .unwrap();

    // Closed: touching counts as overlap (Partial).
    assert_eq!(
        c1.overlap(&c2).unwrap(),
        Overlap::Partial,
        "Sphere: Error 15"
    );
}

#[test]
fn sphere_3d_overlap_touching_open() {
    // Two spheres that touch at exactly one point with open boundaries.
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let mut c1 = CircleBuilder::new(frm.clone(), vec![0.0, 0.0, 0.0])
        .unwrap()
        .edge_point(vec![0.0, 0.0, 1.0])
        .build()
        .unwrap();

    // Sphere at (2.000001, 0, 0) with radius ~1.
    let mut c2 = CircleBuilder::new(frm, vec![2.000001, 0.0, 0.0])
        .unwrap()
        .edge_point(vec![2.000001, 1.0, 0.0])
        .build()
        .unwrap();

    c1.set_closed(false);
    c2.set_closed(false);

    // Open: touching does not count — no overlap.
    assert_eq!(c1.overlap(&c2).unwrap(), Overlap::None, "Sphere: Error 17");
}

#[test]
fn sphere_3d_overlap_clearly_separated() {
    // Two spheres separated by more than their combined radii.
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let c1 = CircleBuilder::new(frm.clone(), vec![0.0, 0.0, 0.0])
        .unwrap()
        .edge_point(vec![0.0, 0.0, 1.0])
        .build()
        .unwrap();

    // Sphere at (2.000004, 0, 0) with radius ~1.
    let c2 = CircleBuilder::new(frm, vec![2.000004, 0.0, 0.0])
        .unwrap()
        .edge_point(vec![2.000004, 1.0, 0.0])
        .build()
        .unwrap();

    assert_eq!(c1.overlap(&c2).unwrap(), Overlap::None, "Sphere: Error 18");
}

// ============================================================================
// checkInterval — BasicFrame portions
//
// Tests interval with unbounded axes, negation, overlap. We port the
// pure-BasicFrame 2D tests that don't require SkyFrame or SpecFrame.
// ============================================================================

#[test]
fn interval_2d_basic_membership_and_negation() {
    // 2D interval: axis 1 in [1, 5], axis 2 in [2, 8]
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let mut interval = IntervalBuilder::new(frm, vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    // Interior point.
    assert!(interval.point_in_region(&[3.0, 5.0]).unwrap());
    // Exterior point.
    assert!(!interval.point_in_region(&[0.0, 5.0]).unwrap());
    assert!(!interval.point_in_region(&[3.0, 9.0]).unwrap());

    // Boundary point (closed).
    assert!(interval.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(interval.point_in_region(&[5.0, 8.0]).unwrap());

    // Negate: interior becomes exterior and vice versa.
    interval.negate();
    assert!(!interval.point_in_region(&[3.0, 5.0]).unwrap());
    assert!(interval.point_in_region(&[0.0, 5.0]).unwrap());
    assert!(interval.point_in_region(&[3.0, 9.0]).unwrap());
}

#[test]
fn interval_2d_unbounded_membership() {
    // 2D interval: axis 1 >= 2.0 (no upper), axis 2 <= 5.0 (no lower)
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frm, vec![2.0, BAD_VALUE], vec![BAD_VALUE, 5.0])
        .unwrap()
        .build()
        .unwrap();

    // Inside: x >= 2 and y <= 5
    assert!(interval.point_in_region(&[3.0, 4.0]).unwrap());
    assert!(interval.point_in_region(&[100.0, -100.0]).unwrap());
    assert!(interval.point_in_region(&[2.0, 5.0]).unwrap());

    // Outside: x < 2 or y > 5
    assert!(!interval.point_in_region(&[1.0, 4.0]).unwrap());
    assert!(!interval.point_in_region(&[3.0, 6.0]).unwrap());
}

#[test]
fn interval_2d_overlap_basic() {
    // Two overlapping intervals.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let int1 = IntervalBuilder::new(frm.clone(), vec![0.0, 0.0], vec![4.0, 4.0])
        .unwrap()
        .build()
        .unwrap();
    let int2 = IntervalBuilder::new(frm, vec![2.0, 2.0], vec![6.0, 6.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(int1.overlap(&int2).unwrap(), Overlap::Partial);
}

#[test]
fn interval_2d_overlap_first_inside() {
    // int1 entirely inside int2.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let int1 = IntervalBuilder::new(frm.clone(), vec![2.0, 2.0], vec![3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();
    let int2 = IntervalBuilder::new(frm, vec![0.0, 0.0], vec![5.0, 5.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(int1.overlap(&int2).unwrap(), Overlap::FirstInside);
    assert_eq!(int2.overlap(&int1).unwrap(), Overlap::SecondInside);
}

#[test]
fn interval_2d_overlap_none() {
    // Non-overlapping intervals.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let int1 = IntervalBuilder::new(frm.clone(), vec![0.0, 0.0], vec![1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    let int2 = IntervalBuilder::new(frm, vec![5.0, 5.0], vec![6.0, 6.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(int1.overlap(&int2).unwrap(), Overlap::None);
}

#[test]
fn interval_2d_overlap_same() {
    // Identical intervals.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let int1 = IntervalBuilder::new(frm.clone(), vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();
    let int2 = IntervalBuilder::new(frm, vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(int1.overlap(&int2).unwrap(), Overlap::Same);
}

// ============================================================================
// checkBox — BasicFrame portions
//
// Tests Box with transforms and overlap using BasicFrame.
// ============================================================================

#[test]
fn box_transform_membership() {
    // Box from (1,2) to (5,8): centre (3,5), half-widths (2,3).
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let bx = BoxRegionBuilder::from_bounds(frm, &[1.0, 2.0], &[5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([
        [3.0, 5.0], // centre: inside
        [1.0, 2.0], // corner: inside (closed)
        [5.0, 8.0], // corner: inside (closed)
        [0.5, 5.0], // outside on axis 1
        [3.0, 8.5], // outside on axis 2
        [0.0, 0.0], // outside on both
    ]);

    let result = bx.transform(&pts, true).unwrap();

    // Inside points pass through.
    assert_eq!(result.point(0)[0], 3.0);
    assert_eq!(result.point(0)[1], 5.0);
    assert_eq!(result.point(1)[0], 1.0);
    assert_eq!(result.point(1)[1], 2.0);
    assert_eq!(result.point(2)[0], 5.0);
    assert_eq!(result.point(2)[1], 8.0);

    // Outside points become BAD.
    assert_eq!(result.point(3)[0], BAD_VALUE);
    assert_eq!(result.point(3)[1], BAD_VALUE);
    assert_eq!(result.point(4)[0], BAD_VALUE);
    assert_eq!(result.point(4)[1], BAD_VALUE);
    assert_eq!(result.point(5)[0], BAD_VALUE);
    assert_eq!(result.point(5)[1], BAD_VALUE);
}

#[test]
fn box_overlap_basic() {
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let b1 = BoxRegionBuilder::from_bounds(frm.clone(), &[0.0, 0.0], &[4.0, 4.0])
        .unwrap()
        .build()
        .unwrap();
    let b2 = BoxRegionBuilder::from_bounds(frm.clone(), &[2.0, 2.0], &[6.0, 6.0])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(b1.overlap(&b2).unwrap(), Overlap::Partial);

    // Non-overlapping.
    let b3 = BoxRegionBuilder::from_bounds(frm.clone(), &[10.0, 10.0], &[12.0, 12.0])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(b1.overlap(&b3).unwrap(), Overlap::None);

    // b1 inside b4.
    let b4 = BoxRegionBuilder::from_bounds(frm, &[-1.0, -1.0], &[5.0, 5.0])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(b1.overlap(&b4).unwrap(), Overlap::FirstInside);
    assert_eq!(b4.overlap(&b1).unwrap(), Overlap::SecondInside);
}

#[test]
fn box_overlap_same() {
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let b1 = BoxRegionBuilder::from_bounds(frm.clone(), &[1.0, 2.0], &[5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();
    let b2 = BoxRegionBuilder::from_bounds(frm, &[1.0, 2.0], &[5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(b1.overlap(&b2).unwrap(), Overlap::Same);
}

// ============================================================================
// checkCmpRegion — BasicFrame portion (lines 2335-2377 of testregions.c)
//
// Tests CmpRegion with Box and Circle using BasicFrame, including negation
// and compound AND/OR operations.
// ============================================================================

#[test]
fn cmpregion_basic_or_negated_and_box() {
    // Create:
    //   f1 = Frame(2)
    //   r1 = Box(f1, bounds [0,0] to [1,1])
    //   r2 = Circle(f1, centre (-1,0), edge (0,0)) -> radius 1
    //   r3 = Circle(f1, centre (1,0), edge (0,0)) -> radius 1 (touches r2 at origin)
    //   cr = CmpRegion(r2, r3, OR) -> union of two unit circles
    //   negate(cr)                  -> exterior of union
    //   cr2 = CmpRegion(cr, r1, AND) -> box minus the two circles
    let frm = BasicFrameBuilder::new(2).build().unwrap();

    let r1 = BoxRegionBuilder::from_bounds(frm.clone(), &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    let r2 = CircleBuilder::new(frm.clone(), vec![-1.0, 0.0])
        .unwrap()
        .edge_point(vec![0.0, 0.0])
        .build()
        .unwrap();

    let r3 = CircleBuilder::new(frm, vec![1.0, 0.0])
        .unwrap()
        .edge_point(vec![0.0, 0.0])
        .build()
        .unwrap();

    // cr = OR of the two circles, then negate
    let mut cr = CmpRegionBuilder::new(r2, r3, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();
    cr.negate();

    // cr2 = AND of negated-union with box
    let cr2 = CmpRegionBuilder::new(cr, r1, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // Test points:
    //   (0.0, 0.5)  — inside box but on boundary of circle r2 (at distance 1
    //                  from (-1,0)) AND on boundary of circle r3 (at distance 1
    //                  from (1,0)). For closed regions, boundary is inside the
    //                  circles, so negated(OR) excludes it. For the AND, the
    //                  point needs to be outside both circles.
    //   (0.2, 1.5)  — outside box (y > 1)
    //   (0.5, 0.5)  — inside box, distance to (-1,0) = sqrt(2.25+0.25)=1.58>1,
    //                  distance to (1,0) = sqrt(0.25+0.25)=0.707<1 -> inside r3
    //                  -> inside OR -> outside negated(OR) -> outside AND
    //   (-0.5, 0.5) — outside box (x < 0)

    // Actually let me recalculate:
    // (0, 0.5): dist to (-1,0) = sqrt(1+0.25) = 1.118 > 1 -> outside r2
    //           dist to (1,0) = sqrt(1+0.25) = 1.118 > 1 -> outside r3
    //           outside both circles -> outside OR -> inside negated(OR)
    //           inside box? x=0, y=0.5: yes (0<=x<=1, 0<=y<=1)
    //           inside AND: yes
    let pts = PointSet::from_rows([
        [0.0, 0.5],  // inside AND
        [0.2, 1.5],  // outside box
        [0.5, 0.5],  // inside r3 (dist to (1,0) = sqrt(0.25+0.25) = 0.707 < 1)
        [-0.5, 0.5], // outside box (x < 0)
    ]);

    let result = cr2.transform(&pts, true).unwrap();

    // Point 0 (0.0, 0.5): inside AND region (CmpRegion:Error 11-12)
    assert_eq!(result.point(0)[0], 0.0, "CmpRegion:Error 11");
    assert_eq!(result.point(0)[1], 0.5, "CmpRegion:Error 12");

    // Point 1 (0.2, 1.5): outside box -> outside AND (CmpRegion:Error 13-14)
    assert_eq!(result.point(1)[0], BAD_VALUE, "CmpRegion:Error 13");
    assert_eq!(result.point(1)[1], BAD_VALUE, "CmpRegion:Error 14");

    // Point 2 (0.5, 0.5): inside r3 -> inside OR -> outside negated(OR)
    // -> outside AND (CmpRegion:Error 15-16)
    assert_eq!(result.point(2)[0], BAD_VALUE, "CmpRegion:Error 15");
    assert_eq!(result.point(2)[1], BAD_VALUE, "CmpRegion:Error 16");

    // Point 3 (-0.5, 0.5): outside box -> outside AND (CmpRegion:Error 17-18)
    assert_eq!(result.point(3)[0], BAD_VALUE, "CmpRegion:Error 17");
    assert_eq!(result.point(3)[1], BAD_VALUE, "CmpRegion:Error 18");
}

#[test]
fn cmpregion_negated_reversal() {
    // Same setup as above, but negate the final result.
    // After negation, points that were inside become outside and vice-versa.
    let frm = BasicFrameBuilder::new(2).build().unwrap();

    let r1 = BoxRegionBuilder::from_bounds(frm.clone(), &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    let r2 = CircleBuilder::new(frm.clone(), vec![-1.0, 0.0])
        .unwrap()
        .edge_point(vec![0.0, 0.0])
        .build()
        .unwrap();

    let r3 = CircleBuilder::new(frm, vec![1.0, 0.0])
        .unwrap()
        .edge_point(vec![0.0, 0.0])
        .build()
        .unwrap();

    let mut cr = CmpRegionBuilder::new(r2, r3, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();
    cr.negate();

    let mut cr2 = CmpRegionBuilder::new(cr, r1, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    cr2.negate();

    let pts = PointSet::from_rows([
        [0.0, 0.5],  // was inside -> now outside
        [0.2, 1.5],  // was outside -> now inside
        [0.5, 0.5],  // was outside -> now inside
        [-0.5, 0.5], // was outside -> now inside
    ]);

    let result = cr2.transform(&pts, true).unwrap();

    // Point 0: was inside the un-negated region, now outside.
    assert_eq!(result.point(0)[0], BAD_VALUE, "CmpRegion:Error 19");
    assert_eq!(result.point(0)[1], BAD_VALUE, "CmpRegion:Error 20");

    // Point 1: was outside, now inside.
    assert_eq!(result.point(1)[0], 0.2, "CmpRegion:Error 21");
    assert_eq!(result.point(1)[1], 1.5, "CmpRegion:Error 22");

    // Point 2: was outside (inside r3), now inside.
    assert_eq!(result.point(2)[0], 0.5, "CmpRegion:Error 23");
    assert_eq!(result.point(2)[1], 0.5, "CmpRegion:Error 24");

    // Point 3: was outside (x < 0), now inside.
    assert_eq!(result.point(3)[0], -0.5, "CmpRegion:Error 25");
    assert_eq!(result.point(3)[1], 0.5, "CmpRegion:Error 26");
}

// ============================================================================
// checkPrism — BasicFrame portions
//
// Tests Prism with bounds computation and overlap using BasicFrame regions.
// ============================================================================

#[test]
fn prism_bounds_basic() {
    // Prism of 2D box + 1D interval = 3D region.
    let frm2 = BasicFrameBuilder::new(2).build().unwrap();
    let bx = BoxRegionBuilder::from_bounds(frm2, &[1.0, 2.0], &[5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval = IntervalBuilder::new(frm1, vec![10.0], vec![20.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(bx, interval).unwrap().build().unwrap();

    let (lbnd, ubnd) = prism.get_region_bounds().unwrap();

    assert_eq!(prism.naxes(), 3);
    assert_eq!(lbnd[0], 1.0);
    assert_eq!(lbnd[1], 2.0);
    assert_eq!(lbnd[2], 10.0);
    assert_eq!(ubnd[0], 5.0);
    assert_eq!(ubnd[1], 8.0);
    assert_eq!(ubnd[2], 20.0);
}

#[test]
fn prism_overlap_same() {
    // Prism overlapping with itself.
    let frm2 = BasicFrameBuilder::new(2).build().unwrap();
    let bx = BoxRegionBuilder::from_bounds(frm2, &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval = IntervalBuilder::new(frm1, vec![5.0], vec![6.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(bx, interval).unwrap().build().unwrap();

    assert_eq!(
        prism.overlap(&prism).unwrap(),
        Overlap::Same,
        "Prism self-overlap"
    );
}

#[test]
fn prism_overlap_second_inside() {
    // Prism with smaller interval: smaller prism inside larger.
    let frm2 = BasicFrameBuilder::new(2).build().unwrap();
    let bx = BoxRegionBuilder::from_bounds(frm2.clone(), &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    let bx2 = BoxRegionBuilder::from_bounds(frm2, &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let big_interval = IntervalBuilder::new(frm1.clone(), vec![5.0], vec![10.0])
        .unwrap()
        .build()
        .unwrap();
    let small_interval = IntervalBuilder::new(frm1, vec![6.0], vec![8.0])
        .unwrap()
        .build()
        .unwrap();

    let big_prism = PrismBuilder::new(bx, big_interval)
        .unwrap()
        .build()
        .unwrap();
    let small_prism = PrismBuilder::new(bx2, small_interval)
        .unwrap()
        .build()
        .unwrap();

    // big_prism.overlap(small_prism) -> SecondInside (small is inside big)
    assert_eq!(
        big_prism.overlap(&small_prism).unwrap(),
        Overlap::SecondInside,
        "Prism containment"
    );
    // small_prism.overlap(big_prism) -> FirstInside
    assert_eq!(
        small_prism.overlap(&big_prism).unwrap(),
        Overlap::FirstInside,
        "Prism containment reverse"
    );
}

#[test]
fn prism_overlap_partial() {
    // Prism with overlapping but not contained intervals.
    let frm2 = BasicFrameBuilder::new(2).build().unwrap();
    let bx1 = BoxRegionBuilder::from_bounds(frm2.clone(), &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    let bx2 = BoxRegionBuilder::from_bounds(frm2, &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let int1 = IntervalBuilder::new(frm1.clone(), vec![5.0], vec![10.0])
        .unwrap()
        .build()
        .unwrap();
    let int2 = IntervalBuilder::new(frm1, vec![8.0], vec![12.0])
        .unwrap()
        .build()
        .unwrap();

    let prism1 = PrismBuilder::new(bx1, int1).unwrap().build().unwrap();
    let prism2 = PrismBuilder::new(bx2, int2).unwrap().build().unwrap();

    assert_eq!(
        prism1.overlap(&prism2).unwrap(),
        Overlap::Partial,
        "Prism partial"
    );
    assert_eq!(
        prism2.overlap(&prism1).unwrap(),
        Overlap::Partial,
        "Prism partial reverse"
    );
}

#[test]
fn prism_overlap_none() {
    // Prisms with non-overlapping intervals.
    let frm2 = BasicFrameBuilder::new(2).build().unwrap();
    let bx1 = BoxRegionBuilder::from_bounds(frm2.clone(), &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    let bx2 = BoxRegionBuilder::from_bounds(frm2, &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let int1 = IntervalBuilder::new(frm1.clone(), vec![5.0], vec![6.0])
        .unwrap()
        .build()
        .unwrap();
    let int2 = IntervalBuilder::new(frm1, vec![7.0], vec![8.0])
        .unwrap()
        .build()
        .unwrap();

    let prism1 = PrismBuilder::new(bx1, int1).unwrap().build().unwrap();
    let prism2 = PrismBuilder::new(bx2, int2).unwrap().build().unwrap();

    assert_eq!(
        prism1.overlap(&prism2).unwrap(),
        Overlap::None,
        "Prism no overlap"
    );
    assert_eq!(
        prism2.overlap(&prism1).unwrap(),
        Overlap::None,
        "Prism no overlap reverse"
    );
}

#[test]
fn prism_membership_3d() {
    // Prism of 2D circle + 1D interval: cylinder.
    let frm2 = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frm2, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval = IntervalBuilder::new(frm1, vec![2.0], vec![8.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(circle, interval)
        .unwrap()
        .build()
        .unwrap();

    // Inside: in circle AND in interval
    assert!(prism.point_in_region(&[0.0, 0.0, 5.0]).unwrap());
    assert!(prism.point_in_region(&[3.0, 4.0, 2.0]).unwrap()); // dist=5, boundary
    assert!(prism.point_in_region(&[4.0, 0.0, 8.0]).unwrap());

    // Outside: outside circle
    assert!(!prism.point_in_region(&[6.0, 0.0, 5.0]).unwrap());
    // Outside: outside interval
    assert!(!prism.point_in_region(&[0.0, 0.0, 1.0]).unwrap());
    assert!(!prism.point_in_region(&[0.0, 0.0, 9.0]).unwrap());
    // Outside: both
    assert!(!prism.point_in_region(&[6.0, 0.0, 9.0]).unwrap());
}

// ============================================================================
// Additional cross-class integration tests
// ============================================================================

#[test]
fn cmpregion_and_overlap_same() {
    // Two identical AND regions should have Same overlap.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frm.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::from_bounds(frm.clone(), &[-3.0, -3.0], &[3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp1 = CmpRegionBuilder::new(circle.clone(), bx.clone(), CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let circle2 = CircleBuilder::new(frm.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx2 = BoxRegionBuilder::from_bounds(frm, &[-3.0, -3.0], &[3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();
    let cmp2 = CmpRegionBuilder::new(circle2, bx2, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(cmp1.overlap(&cmp2).unwrap(), Overlap::Same);
}

#[test]
fn cmpregion_or_membership() {
    // OR of two circles: points inside either circle should be inside.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let c1 = CircleBuilder::new(frm.clone(), vec![-3.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(frm, vec![3.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    // Inside c1 only.
    assert!(cmp.point_in_region(&[-3.0, 0.0]).unwrap());
    assert!(cmp.point_in_region(&[-2.0, 0.0]).unwrap());

    // Inside c2 only.
    assert!(cmp.point_in_region(&[3.0, 0.0]).unwrap());
    assert!(cmp.point_in_region(&[4.0, 0.0]).unwrap());

    // Outside both.
    assert!(!cmp.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(!cmp.point_in_region(&[0.0, 5.0]).unwrap());
}

#[test]
fn cmpregion_and_membership() {
    // AND of circle and box: points must be inside both.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frm.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::from_bounds(frm, &[-3.0, -3.0], &[3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // Inside both.
    assert!(cmp.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(cmp.point_in_region(&[2.0, 2.0]).unwrap());
    assert!(cmp.point_in_region(&[-3.0, 0.0]).unwrap());

    // Inside circle but outside box.
    assert!(!cmp.point_in_region(&[4.0, 0.0]).unwrap());
    assert!(!cmp.point_in_region(&[0.0, 4.0]).unwrap());

    // Inside box but outside circle.
    // Note: box corners at (3,3) have dist = sqrt(9+9) = 4.24 < 5,
    // so the box is entirely inside the circle. AND == box.
    assert!(cmp.point_in_region(&[3.0, 3.0]).unwrap());

    // Outside circle (and outside box).
    assert!(!cmp.point_in_region(&[6.0, 0.0]).unwrap());
}

#[test]
fn cmpregion_and_box_inside_circle() {
    // When box is entirely inside circle, AND == box.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frm.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::from_bounds(frm, &[-3.0, -3.0], &[3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // Since max corner distance = sqrt(9+9) = 4.24 < 5, box is inside circle.
    // Points on box boundary should be inside AND.
    assert!(cmp.point_in_region(&[3.0, 3.0]).unwrap());
    assert!(cmp.point_in_region(&[-3.0, -3.0]).unwrap());

    // Points outside box but inside circle should be outside AND.
    assert!(!cmp.point_in_region(&[4.0, 0.0]).unwrap());
    assert!(!cmp.point_in_region(&[0.0, -4.0]).unwrap());
}

#[test]
fn prism_with_cmpregion_bounds() {
    // Prism of (Circle AND Box) + 1D interval.
    let frm2 = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frm2.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::from_bounds(frm2, &[-3.0, -3.0], &[3.0, 3.0])
        .unwrap()
        .build()
        .unwrap();
    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval = IntervalBuilder::new(frm1, vec![100.0], vec![200.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(cmp, interval).unwrap().build().unwrap();

    assert_eq!(prism.naxes(), 3);

    let (lbnd, ubnd) = prism.get_region_bounds().unwrap();
    // The AND of circle(r=5) and box([-3,3]x[-3,3]) is the box (since box is inside circle).
    // So bounds should be [-3,-3,100] to [3,3,200].
    // But the bounds algorithm may report the tighter of the two components:
    // Circle bounds: [-5,-5] to [5,5], Box bounds: [-3,-3] to [3,3]
    // AND bounds = intersection = [-3,-3] to [3,3]
    assert!((lbnd[0] - (-3.0)).abs() < 1e-10, "Prism+CmpRegion lbnd[0]");
    assert!((lbnd[1] - (-3.0)).abs() < 1e-10, "Prism+CmpRegion lbnd[1]");
    assert!((lbnd[2] - 100.0).abs() < 1e-10, "Prism+CmpRegion lbnd[2]");
    assert!((ubnd[0] - 3.0).abs() < 1e-10, "Prism+CmpRegion ubnd[0]");
    assert!((ubnd[1] - 3.0).abs() < 1e-10, "Prism+CmpRegion ubnd[1]");
    assert!((ubnd[2] - 200.0).abs() < 1e-10, "Prism+CmpRegion ubnd[2]");
}

// ============================================================================
// SkyFrame-based region tests
//
// Ported from checkCircle, checkBox, checkEllipse, checkPolygon in
// reference/starlink-ast/ast_tester/testregions.c
//
// These test that regions correctly use spherical (great-circle) geometry
// when the encapsulated frame is a SkyFrame.
// ============================================================================

// ============================================================================
// Circle on SkyFrame — basic membership via transform
// (from checkCircle, lines 1675-1708 of testregions.c)
//
// A circle on the sky is a spherical cap. Points inside are those whose
// great-circle distance from the centre is less than the radius.
// ============================================================================

#[test]
fn sky_circle_basic_membership() {
    // Create a SkyFrame (default ICRS).
    let sky = SkyFrame::new(SkySystem::Icrs);

    // Centre at RA=1.2217305 rad (~70 deg), Dec=1.3962634 rad (~80 deg).
    let centre = [1.2217305, 1.3962634];
    // Edge point at (0.8, 0.8).
    let edge = [0.8, 0.8];

    let circle = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .edge_point(vec![edge[0], edge[1]])
        .build()
        .unwrap();

    // Compute the radius as great-circle distance from centre to edge.
    let rad = sky.distance(&centre, &edge).unwrap();
    assert!(rad > 0.0, "radius must be positive");

    // Get a point just inside (99.9% of radius along centre→edge direction).
    let p_inside = sky.offset(&centre, &edge, rad * 0.999).unwrap();
    // Get a point just outside (100.1% of radius along centre→edge direction).
    let p_outside = sky.offset(&centre, &edge, rad * 1.001).unwrap();

    // Transform through the circle: inside passes through, outside becomes BAD.
    let pts = PointSet::from_rows([[p_inside[0], p_inside[1]], [p_outside[0], p_outside[1]]]);
    let result = circle.transform(&pts, true).unwrap();

    // Inside point passes through.
    assert_eq!(
        result.point(0)[0],
        p_inside[0],
        "SkyCircle: inside point RA should pass"
    );
    assert_eq!(
        result.point(0)[1],
        p_inside[1],
        "SkyCircle: inside point Dec should pass"
    );

    // Outside point becomes BAD.
    assert_eq!(
        result.point(1)[0],
        BAD_VALUE,
        "SkyCircle: outside point RA should be BAD"
    );
    assert_eq!(
        result.point(1)[1],
        BAD_VALUE,
        "SkyCircle: outside point Dec should be BAD"
    );
}

#[test]
fn sky_circle_membership_opposite_direction() {
    // Test membership along a different direction from centre.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let centre = [1.2217305, 1.3962634];
    let edge = [0.8, 0.8];

    let circle = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .edge_point(vec![edge[0], edge[1]])
        .build()
        .unwrap();

    let rad = sky.distance(&centre, &edge).unwrap();

    // Test along a different direction (towards south pole area).
    let other_dir = [centre[0] + 0.5, centre[1] - 0.5];
    let p_inside = sky.offset(&centre, &other_dir, rad * 0.9).unwrap();
    let p_outside = sky.offset(&centre, &other_dir, rad * 1.1).unwrap();

    let pts = PointSet::from_rows([[p_inside[0], p_inside[1]], [p_outside[0], p_outside[1]]]);
    let result = circle.transform(&pts, true).unwrap();

    // Inside passes.
    assert_eq!(
        result.point(0)[0],
        p_inside[0],
        "SkyCircle opposite: inside RA"
    );
    assert_eq!(
        result.point(0)[1],
        p_inside[1],
        "SkyCircle opposite: inside Dec"
    );

    // Outside becomes BAD.
    assert_eq!(
        result.point(1)[0],
        BAD_VALUE,
        "SkyCircle opposite: outside RA"
    );
    assert_eq!(
        result.point(1)[1],
        BAD_VALUE,
        "SkyCircle opposite: outside Dec"
    );
}

#[test]
fn sky_circle_centre_is_inside() {
    // The centre of the circle is always inside.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.2217305, 1.3962634];
    let edge = [0.8, 0.8];

    let circle = CircleBuilder::new(sky, vec![centre[0], centre[1]])
        .unwrap()
        .edge_point(vec![edge[0], edge[1]])
        .build()
        .unwrap();

    assert!(
        circle.point_in_region(&centre).unwrap(),
        "Centre must be inside circle"
    );
}

#[test]
fn sky_circle_edge_is_inside_closed() {
    // The edge point should be inside a closed circle (on boundary).
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.2217305, 1.3962634];
    let edge = [0.8, 0.8];

    let circle = CircleBuilder::new(sky, vec![centre[0], centre[1]])
        .unwrap()
        .edge_point(vec![edge[0], edge[1]])
        .build()
        .unwrap();

    assert!(
        circle.point_in_region(&edge).unwrap(),
        "Edge point must be inside closed circle"
    );
}

#[test]
fn sky_circle_from_radius() {
    // Create circle using radius instead of edge point.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [0.0, 0.0]; // RA=0, Dec=0

    // 0.1 radian radius (about 5.7 degrees)
    let circle = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();

    // Point at 0.05 rad from centre (inside).
    let p_in = sky.offset(&centre, &[0.1, 0.0], 0.05).unwrap();
    assert!(
        circle.point_in_region(&p_in).unwrap(),
        "Point at half radius should be inside"
    );

    // Point at 0.15 rad from centre (outside).
    let p_out = sky.offset(&centre, &[0.1, 0.0], 0.15).unwrap();
    assert!(
        !circle.point_in_region(&p_out).unwrap(),
        "Point at 1.5x radius should be outside"
    );
}

// ============================================================================
// Circle on SkyFrame — overlap detection
// (from checkCircle, lines 1746-1764 of testregions.c)
// ============================================================================

#[test]
fn sky_circle_overlap_same() {
    // Two identical circles on the sky: overlap should be Same.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];

    let c1 = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .radius(0.2)
        .build()
        .unwrap();

    let c2 = CircleBuilder::new(sky, vec![centre[0], centre[1]])
        .unwrap()
        .radius(0.2)
        .build()
        .unwrap();

    assert_eq!(
        c1.overlap(&c2).unwrap(),
        Overlap::Same,
        "SkyCircle overlap: identical circles should be Same"
    );
}

#[test]
fn sky_circle_overlap_first_inside() {
    // Smaller circle at same centre is inside larger.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];

    let big = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .radius(0.5)
        .build()
        .unwrap();

    let small = CircleBuilder::new(sky, vec![centre[0], centre[1]])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();

    assert_eq!(
        small.overlap(&big).unwrap(),
        Overlap::FirstInside,
        "SkyCircle overlap: small inside big -> FirstInside"
    );
    assert_eq!(
        big.overlap(&small).unwrap(),
        Overlap::SecondInside,
        "SkyCircle overlap: big contains small -> SecondInside"
    );
}

#[test]
fn sky_circle_overlap_partial() {
    // Two overlapping circles offset in RA.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let c1 = CircleBuilder::new(sky.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.3)
        .build()
        .unwrap();

    // Shift centre in RA by less than sum of radii.
    let c2 = CircleBuilder::new(sky, vec![1.4, 0.5])
        .unwrap()
        .radius(0.3)
        .build()
        .unwrap();

    assert_eq!(
        c1.overlap(&c2).unwrap(),
        Overlap::Partial,
        "SkyCircle overlap: offset circles -> Partial"
    );
}

#[test]
fn sky_circle_overlap_none() {
    // Two circles far apart on the sky: no overlap.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let c1 = CircleBuilder::new(sky.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();

    // Opposite side of the sky.
    let c2 = CircleBuilder::new(sky, vec![PI, 0.0])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();

    assert_eq!(
        c1.overlap(&c2).unwrap(),
        Overlap::None,
        "SkyCircle overlap: distant circles -> None"
    );
}

// ============================================================================
// Circle on SkyFrame — bounds and disc
// (from checkCircle, lines 1637-1648 of testregions.c)
// ============================================================================

#[test]
fn sky_circle_disc() {
    // Circle at (0, 1.0) with radius 0.01.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [0.0, 1.0];
    let radius = 0.01;

    let circle = CircleBuilder::new(sky, vec![centre[0], centre[1]])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();

    let (disc_centre, disc_radius) = circle.get_region_disc().unwrap();

    // Disc centre should be near the circle's centre.
    assert!(
        (disc_centre[0] - centre[0]).abs() < 1.0e-5,
        "SkyCircle disc: centre RA"
    );
    assert!(
        (disc_centre[1] - centre[1]).abs() < 1.0e-5,
        "SkyCircle disc: centre Dec"
    );
    // Disc radius should be approximately the circle's radius.
    assert!(
        (disc_radius - radius).abs() < 1.0e-5,
        "SkyCircle disc: radius"
    );
}

#[test]
fn sky_circle_bounds_equator() {
    // Circle at equator, away from pole — bounds should be simple.
    // Circle at (1.0, 0.0) with radius 0.1 rad.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.0]; // RA=1 rad, Dec=0
    let radius = 0.1;

    let circle = CircleBuilder::new(sky, vec![centre[0], centre[1]])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();

    let (lbnd, ubnd) = circle.get_region_bounds().unwrap();

    // Bounds come from the great-circle mesh (circle.c::RegBaseMesh +
    // skyframe.c::NormBox), not a planar centre ± radius box. At the equator
    // the RA extent is ±radius (cos(dec)=1) and the Dec extent runs from
    // -radius to just under +radius: the mesh's northernmost sample sits a
    // hair inside the true top, so the upper Dec is ~0.09995 rather than 0.1.
    assert!(
        (lbnd[1] - (centre[1] - radius)).abs() < 1.0e-4,
        "SkyCircle equator bounds: lower Dec, got {}",
        lbnd[1]
    );
    assert!(
        (ubnd[1] - 0.09995).abs() < 1.0e-4,
        "SkyCircle equator bounds: upper Dec, got {}",
        ubnd[1]
    );
    // RA bounds — great-circle offset of radius due east/west of centre.
    assert!(
        (lbnd[0] - (centre[0] - radius)).abs() < 1.0e-4,
        "SkyCircle equator bounds: lower RA, got {}",
        lbnd[0]
    );
    assert!(
        (ubnd[0] - (centre[0] + radius)).abs() < 1.0e-4,
        "SkyCircle equator bounds: upper RA, got {}",
        ubnd[0]
    );
}

#[test]
fn sky_circle_bounds_near_pole() {
    // Circle centred near north pole: RA bounds should wrap to [0, 2*pi].
    // From checkCircle lines 1765-1771 of testregions.c.
    // Centre at (0.8, pi/2) — the north celestial pole with RA=0.8.
    // With radius 0.1, the circle extends from Dec = pi/2 - 0.1 to pi/2.
    // The RA bounds should be the full range [0, 2*pi] because the circle
    // straddles all longitudes near the pole.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [0.8, FRAC_PI_2];
    let radius = 0.1;

    let circle = CircleBuilder::new(sky, vec![centre[0], centre[1]])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();

    let (lbnd, ubnd) = circle.get_region_bounds().unwrap();

    // RA bounds should be [0, 2*pi] for a polar cap.
    assert!(
        lbnd[0] < 0.01,
        "SkyCircle pole bounds: lower RA should be ~0, got {}",
        lbnd[0]
    );
    assert!(
        ubnd[0] > 2.0 * PI - 0.01,
        "SkyCircle pole bounds: upper RA should be ~2*pi, got {}",
        ubnd[0]
    );

    // Dec bounds: lower = pi/2 - radius, upper = pi/2.
    assert!(
        (lbnd[1] - (FRAC_PI_2 - radius)).abs() < 1.0e-6,
        "SkyCircle pole bounds: lower Dec"
    );
    assert!(
        (ubnd[1] - FRAC_PI_2).abs() < 1.0e-6,
        "SkyCircle pole bounds: upper Dec"
    );
}

// ============================================================================
// Circle on SkyFrame — negated membership
// ============================================================================

#[test]
fn sky_circle_negated_membership() {
    // Negated circle: inside becomes outside and vice versa.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];
    let radius = 0.2;

    let mut circle = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();

    // Before negation: centre is inside.
    assert!(circle.point_in_region(&centre).unwrap());

    // After negation: centre is outside.
    circle.negate();
    assert!(!circle.point_in_region(&centre).unwrap());

    // A distant point should now be inside.
    let far_point = sky.offset(&centre, &[2.0, 0.0], radius * 2.0).unwrap();
    assert!(circle.point_in_region(&far_point).unwrap());
}

// ============================================================================
// Circle on SkyFrame — open boundary
// ============================================================================

#[test]
fn sky_circle_open_boundary() {
    // Open circle excludes boundary points.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];
    let edge = [1.1, 0.5];

    let mut circle = CircleBuilder::new(sky, vec![centre[0], centre[1]])
        .unwrap()
        .edge_point(vec![edge[0], edge[1]])
        .build()
        .unwrap();

    // Closed: edge is inside.
    assert!(
        circle.point_in_region(&edge).unwrap(),
        "Closed circle includes edge"
    );

    // Open: edge is outside.
    circle.set_closed(false);
    assert!(
        !circle.point_in_region(&edge).unwrap(),
        "Open circle excludes edge"
    );

    // Centre is still inside for open circle.
    assert!(
        circle.point_in_region(&centre).unwrap(),
        "Open circle includes centre"
    );
}

// ============================================================================
// Circle on SkyFrame — FK5 system
// (verify Circle works with different sky systems)
// ============================================================================

#[test]
fn sky_circle_fk5_membership() {
    // Circle on FK5 SkyFrame — same geometry, different system.
    let sky = SkyFrame::new(SkySystem::Fk5);

    let centre = [0.5, 0.3]; // RA ~29 deg, Dec ~17 deg
    let radius = 0.05; // ~2.9 degrees

    let circle = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();

    // Point at 80% of radius — inside.
    let p_in = sky.offset(&centre, &[0.6, 0.3], radius * 0.8).unwrap();
    assert!(
        circle.point_in_region(&p_in).unwrap(),
        "FK5 circle: inside point"
    );

    // Point at 120% of radius — outside.
    let p_out = sky.offset(&centre, &[0.6, 0.3], radius * 1.2).unwrap();
    assert!(
        !circle.point_in_region(&p_out).unwrap(),
        "FK5 circle: outside point"
    );
}

// ============================================================================
// Box on SkyFrame — basic membership
// (from checkBox, lines 1103+ of testregions.c)
//
// A Box on SkyFrame uses centre ± half-widths in angular coordinates.
// The membership test uses the frame's distance metric indirectly.
// ============================================================================

#[test]
fn sky_box_basic_membership() {
    // Box on SkyFrame centred at (1.0, 0.5), half-widths (0.1, 0.1).
    // This is a small patch away from the pole where the geometry is
    // approximately flat.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];
    let corner = [1.1, 0.6]; // centre + half-widths

    let bx = BoxRegionBuilder::new(sky, centre.to_vec(), corner.to_vec())
        .unwrap()
        .build()
        .unwrap();

    // Centre is inside.
    assert!(bx.point_in_region(&centre).unwrap(), "SkyBox: centre");
    // Corner is inside (closed).
    assert!(bx.point_in_region(&corner).unwrap(), "SkyBox: corner");
    // Opposite corner is inside.
    assert!(
        bx.point_in_region(&[0.9, 0.4]).unwrap(),
        "SkyBox: opposite corner"
    );
    // Point outside in RA.
    assert!(
        !bx.point_in_region(&[1.2, 0.5]).unwrap(),
        "SkyBox: outside RA"
    );
    // Point outside in Dec.
    assert!(
        !bx.point_in_region(&[1.0, 0.7]).unwrap(),
        "SkyBox: outside Dec"
    );
}

#[test]
fn sky_box_bounds() {
    // Box bounds should match the defining points.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];
    let corner = [1.1, 0.6];

    let bx = BoxRegionBuilder::new(sky, centre.to_vec(), corner.to_vec())
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = bx.get_region_bounds().unwrap();

    // Lower bound: centre - half-widths = (0.9, 0.4).
    assert!((lbnd[0] - 0.9).abs() < 1.0e-10, "SkyBox bounds: lower RA");
    assert!((lbnd[1] - 0.4).abs() < 1.0e-10, "SkyBox bounds: lower Dec");
    // Upper bound: centre + half-widths = (1.1, 0.6).
    assert!((ubnd[0] - 1.1).abs() < 1.0e-10, "SkyBox bounds: upper RA");
    assert!((ubnd[1] - 0.6).abs() < 1.0e-10, "SkyBox bounds: upper Dec");
}

#[test]
fn sky_box_overlap_same() {
    // Identical boxes should have Same overlap.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];
    let corner = [1.1, 0.6];

    let b1 = BoxRegionBuilder::new(sky.clone(), centre.to_vec(), corner.to_vec())
        .unwrap()
        .build()
        .unwrap();
    let b2 = BoxRegionBuilder::new(sky, centre.to_vec(), corner.to_vec())
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        b1.overlap(&b2).unwrap(),
        Overlap::Same,
        "SkyBox overlap: same"
    );
}

#[test]
fn sky_box_overlap_none() {
    // Two boxes far apart.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let b1 = BoxRegionBuilder::new(sky.clone(), vec![0.5, 0.3], vec![0.6, 0.4])
        .unwrap()
        .build()
        .unwrap();
    let b2 = BoxRegionBuilder::new(sky, vec![3.0, -0.5], vec![3.1, -0.4])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        b1.overlap(&b2).unwrap(),
        Overlap::None,
        "SkyBox overlap: none"
    );
}

#[test]
fn sky_box_overlap_partial() {
    // Two overlapping boxes.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let b1 = BoxRegionBuilder::new(sky.clone(), vec![1.0, 0.5], vec![1.1, 0.6])
        .unwrap()
        .build()
        .unwrap();
    // Overlaps in both RA and Dec.
    let b2 = BoxRegionBuilder::new(sky, vec![1.05, 0.55], vec![1.15, 0.65])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        b1.overlap(&b2).unwrap(),
        Overlap::Partial,
        "SkyBox overlap: partial"
    );
}

// ============================================================================
// Ellipse on SkyFrame — basic membership
// (from checkEllipse, lines 1967-1999 of testregions.c)
// ============================================================================

#[test]
fn sky_ellipse_basic_membership() {
    // Ellipse on SkyFrame at (1.0, 0.5) with semi-axes [0.1, 0.05]
    // and angle 0 (semi-major aligned with RA axis).
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];
    let semi_axes = [0.1, 0.05]; // a=0.1 rad along axis 1, b=0.05 along axis 2
    let angle = 0.0;

    let ellipse = EllipseBuilder::new(sky, centre.to_vec(), semi_axes, angle)
        .unwrap()
        .build()
        .unwrap();

    // Centre is inside.
    assert!(
        ellipse.point_in_region(&centre).unwrap(),
        "SkyEllipse: centre inside"
    );

    // Point at (1.05, 0.5) — along semi-major axis at 50% distance.
    assert!(
        ellipse.point_in_region(&[1.05, 0.5]).unwrap(),
        "SkyEllipse: halfway along major axis"
    );

    // Point at (1.0, 0.52) — along semi-minor axis at 40% of b.
    assert!(
        ellipse.point_in_region(&[1.0, 0.52]).unwrap(),
        "SkyEllipse: near centre along minor axis"
    );

    // Point at (1.15, 0.5) — beyond semi-major axis (0.15 > 0.1).
    assert!(
        !ellipse.point_in_region(&[1.15, 0.5]).unwrap(),
        "SkyEllipse: beyond major axis"
    );

    // Point at (1.0, 0.6) — beyond semi-minor axis (0.1 > 0.05).
    assert!(
        !ellipse.point_in_region(&[1.0, 0.6]).unwrap(),
        "SkyEllipse: beyond minor axis"
    );
}

#[test]
fn sky_ellipse_rotated_membership() {
    // Ellipse rotated by pi/4 — semi-major axis at 45 degrees.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.0]; // On equator for simpler geometry.
    let semi_axes = [0.1, 0.02]; // Elongated ellipse.
    let angle = std::f64::consts::FRAC_PI_4; // 45 degrees.

    let ellipse = EllipseBuilder::new(sky, centre.to_vec(), semi_axes, angle)
        .unwrap()
        .build()
        .unwrap();

    // Centre is inside.
    assert!(
        ellipse.point_in_region(&centre).unwrap(),
        "SkyEllipse rotated: centre inside"
    );

    // A point along the 45-degree diagonal at half the semi-major distance
    // should be inside.
    let diag = 0.05 / 2.0_f64.sqrt();
    assert!(
        ellipse
            .point_in_region(&[centre[0] + diag, centre[1] + diag])
            .unwrap(),
        "SkyEllipse rotated: along major diagonal"
    );
}

#[test]
fn sky_ellipse_disc() {
    // Disc of ellipse should encompass the semi-major axis.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [2.0, 0.3];
    let semi_axes = [0.1, 0.05];
    let angle = 0.0;

    let ellipse = EllipseBuilder::new(sky, centre.to_vec(), semi_axes, angle)
        .unwrap()
        .build()
        .unwrap();

    let (disc_centre, disc_radius) = ellipse.get_region_disc().unwrap();

    // Disc centre should be near the ellipse centre.
    assert!(
        (disc_centre[0] - centre[0]).abs() < 1.0e-3,
        "SkyEllipse disc: centre RA"
    );
    assert!(
        (disc_centre[1] - centre[1]).abs() < 1.0e-3,
        "SkyEllipse disc: centre Dec"
    );
    // Disc radius should be at least the semi-major axis.
    assert!(
        disc_radius >= semi_axes[0] * 0.99,
        "SkyEllipse disc: radius >= semi-major, got {}",
        disc_radius
    );
}

// ============================================================================
// Polygon on SkyFrame — membership
// (from checkPolygon, lines 644-750 of testregions.c)
//
// Note: Polygon reg_pins uses a 2D crossing-number algorithm that operates
// directly on coordinates. For small regions far from the pole, this works
// approximately correctly even on a SkyFrame. Near the pole or wrapping
// in RA, it will fail.
// ============================================================================

#[test]
fn sky_polygon_basic_membership() {
    // Small polygon on SkyFrame at moderate declination.
    // Vertices form a small quadrilateral far from the pole.
    let sky = SkyFrame::new(SkySystem::Icrs);

    // Square-ish polygon ~6 degrees wide at Dec ~30 degrees. Vertices are
    // ordered so that the enclosed patch (not its spherical complement) is the
    // interior: on the sky the winding that bounds the small region is the
    // reverse of the naive planar clockwise order. These membership results are
    // byte-for-byte those reported by the C Starlink AST Polygon Transform.
    let vertices = vec![
        vec![1.0, 0.5],
        vec![1.0, 0.6],
        vec![1.1, 0.6],
        vec![1.1, 0.5],
    ];

    let polygon = PolygonBuilder::new(sky, vertices).unwrap().build().unwrap();

    // Centre of polygon is inside.
    assert!(
        polygon.point_in_region(&[1.05, 0.55]).unwrap(),
        "SkyPolygon: centre inside"
    );

    // Corner is inside (closed).
    assert!(
        polygon.point_in_region(&[1.0, 0.5]).unwrap(),
        "SkyPolygon: corner inside (closed)"
    );

    // Point well outside.
    assert!(
        !polygon.point_in_region(&[2.0, 0.55]).unwrap(),
        "SkyPolygon: outside in RA"
    );
    assert!(
        !polygon.point_in_region(&[1.05, 0.8]).unwrap(),
        "SkyPolygon: outside in Dec"
    );
}

#[test]
fn sky_polygon_triangle_membership() {
    // Triangle on SkyFrame.
    let sky = SkyFrame::new(SkySystem::Icrs);

    // Vertices wound so the small triangular patch is the interior (see the
    // note on `sky_polygon_basic_membership`). C-verified membership.
    let vertices = vec![vec![0.5, 0.2], vec![0.6, 0.4], vec![0.7, 0.2]];

    let polygon = PolygonBuilder::new(sky, vertices).unwrap().build().unwrap();

    // Centroid is inside.
    assert!(
        polygon.point_in_region(&[0.6, 0.267]).unwrap(),
        "SkyPolygon triangle: centroid"
    );

    // Point outside the triangle but within bounding box.
    assert!(
        !polygon.point_in_region(&[0.5, 0.39]).unwrap(),
        "SkyPolygon triangle: outside near apex"
    );
}

#[test]
fn sky_polygon_bounds() {
    // Polygon bounds should encompass all vertices.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let vertices = vec![
        vec![1.0, 0.5],
        vec![1.1, 0.5],
        vec![1.1, 0.6],
        vec![1.0, 0.6],
    ];

    let polygon = PolygonBuilder::new(sky, vertices).unwrap().build().unwrap();

    let (lbnd, ubnd) = polygon.get_region_bounds().unwrap();

    assert!(lbnd[0] <= 1.0 + 1e-10, "SkyPolygon bounds: lbnd RA <= 1.0");
    assert!(lbnd[1] <= 0.5 + 1e-10, "SkyPolygon bounds: lbnd Dec <= 0.5");
    assert!(ubnd[0] >= 1.1 - 1e-10, "SkyPolygon bounds: ubnd RA >= 1.1");
    assert!(ubnd[1] >= 0.6 - 1e-10, "SkyPolygon bounds: ubnd Dec >= 0.6");
}

#[test]
fn sky_polygon_disc() {
    // Polygon disc should encompass the polygon.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let vertices = vec![
        vec![1.0, 0.5],
        vec![1.1, 0.5],
        vec![1.1, 0.6],
        vec![1.0, 0.6],
    ];

    let polygon = PolygonBuilder::new(sky.clone(), vertices)
        .unwrap()
        .build()
        .unwrap();

    let (disc_centre, disc_radius) = polygon.get_region_disc().unwrap();

    // The disc centre should be near the polygon centroid.
    assert!(
        (disc_centre[0] - 1.05).abs() < 0.1,
        "SkyPolygon disc: centre RA"
    );
    assert!(
        (disc_centre[1] - 0.55).abs() < 0.1,
        "SkyPolygon disc: centre Dec"
    );

    // The disc radius should be enough to encompass all vertices.
    // Distance from centroid to corner (1.0, 0.5):
    let corner_dist = sky.distance(&disc_centre, &[1.0, 0.5]).unwrap();
    assert!(
        disc_radius >= corner_dist * 0.99,
        "SkyPolygon disc: radius should encompass vertices"
    );
}

// ============================================================================
// CmpRegion on SkyFrame — compound region operations
// (from checkCmpRegion, line 2236+ of testregions.c)
// ============================================================================

#[test]
fn sky_cmpregion_and_circle_box() {
    // AND of a circle and a box on SkyFrame: points must be inside both.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let circle = CircleBuilder::new(sky.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.2)
        .build()
        .unwrap();

    // Box centred at same location, half-widths 0.15 in each axis.
    let bx = BoxRegionBuilder::new(sky, vec![1.0, 0.5], vec![1.15, 0.65])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle, bx, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    // Centre: inside both.
    assert!(
        cmp.point_in_region(&[1.0, 0.5]).unwrap(),
        "SkyCmpRegion AND: centre"
    );

    // Point inside box but outside circle (corner of box at distance
    // sqrt(0.15^2 + 0.15^2) ~ 0.212 > 0.2 from centre).
    // Actually in angular distance on sky this is approximate.
    // Use a point clearly outside the circle's radius.
    let far_corner = [1.14, 0.64];
    // Distance from (1.0, 0.5) to (1.14, 0.64):
    let dist_to_corner = SkyFrame::new(SkySystem::Icrs)
        .distance(&[1.0, 0.5], &far_corner)
        .unwrap();
    if dist_to_corner > 0.2 {
        // This corner is outside the circle, so outside the AND.
        assert!(
            !cmp.point_in_region(&far_corner).unwrap(),
            "SkyCmpRegion AND: box corner outside circle"
        );
    }

    // Point inside circle but outside box.
    // (0.85, 0.5): RA distance = 0.15 < radius 0.2, but outside box (box RA range is [0.85, 1.15]).
    // Wait, box centre 1.0, corner 1.15 means half-width 0.15 so range [0.85, 1.15].
    // So (0.84, 0.5) is outside box.
    assert!(
        !cmp.point_in_region(&[0.84, 0.5]).unwrap(),
        "SkyCmpRegion AND: inside circle but outside box"
    );
}

#[test]
fn sky_cmpregion_or_two_circles() {
    // OR of two circles on SkyFrame.
    let sky = SkyFrame::new(SkySystem::Icrs);

    let c1 = CircleBuilder::new(sky.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();

    let c2 = CircleBuilder::new(sky, vec![1.5, 0.5])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(c1, c2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();

    // Centre of c1 is inside OR.
    assert!(
        cmp.point_in_region(&[1.0, 0.5]).unwrap(),
        "SkyCmpRegion OR: centre of c1"
    );

    // Centre of c2 is inside OR.
    assert!(
        cmp.point_in_region(&[1.5, 0.5]).unwrap(),
        "SkyCmpRegion OR: centre of c2"
    );

    // Midpoint between the two centres — outside both circles (distance 0.25 > 0.1).
    assert!(
        !cmp.point_in_region(&[1.25, 0.5]).unwrap(),
        "SkyCmpRegion OR: midpoint outside both"
    );
}

// ============================================================================
// Circle on SkyFrame — large radius
// Test that a large spherical cap (> 90 degrees) works correctly.
// ============================================================================

#[test]
fn sky_circle_large_radius() {
    // A circle with radius > pi/2 is a "more-than-hemisphere" cap.
    // Points on the opposite side of the sphere can be inside.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [0.0, 0.0]; // RA=0, Dec=0
    let radius = 2.0; // > pi/2, about 115 degrees

    let circle = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();

    // Centre is inside.
    assert!(
        circle.point_in_region(&centre).unwrap(),
        "Large circle: centre"
    );

    // Point at 90 degrees away (pi/2 < radius) — inside.
    let p_90deg = [FRAC_PI_2, 0.0];
    let dist_90 = sky.distance(&centre, &p_90deg).unwrap();
    assert!(
        dist_90 < radius,
        "Distance to 90 deg point should be < radius"
    );
    assert!(
        circle.point_in_region(&p_90deg).unwrap(),
        "Large circle: point at 90 degrees is inside"
    );

    // Antipodal point at (pi, 0) — distance is pi ~3.14 > 2.0 — outside.
    let p_anti = [PI, 0.0];
    let dist_anti = sky.distance(&centre, &p_anti).unwrap();
    assert!(dist_anti > radius);
    assert!(
        !circle.point_in_region(&p_anti).unwrap(),
        "Large circle: antipodal point is outside"
    );
}

// ============================================================================
// Circle on SkyFrame — transform with multiple points
// Comprehensive test of batch transform through sky circle.
// ============================================================================

#[test]
fn sky_circle_batch_transform() {
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [2.0, -0.3]; // RA ~115 deg, Dec ~-17 deg
    let radius = 0.15;

    let circle = CircleBuilder::new(sky.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();

    // Generate test points at various fractions of the radius.
    let direction = [2.2, -0.2];
    let fracs = [0.0, 0.5, 0.99, 1.01, 1.5, 2.0];
    let points: Vec<[f64; 2]> = fracs
        .iter()
        .map(|&f| {
            let p = sky.offset(&centre, &direction, radius * f).unwrap();
            [p[0], p[1]]
        })
        .collect();

    let pts = PointSet::from_rows(points.iter().copied());
    let result = circle.transform(&pts, true).unwrap();

    // Points at fractions 0.0, 0.5, 0.99 are inside (pass through).
    for (i, &frac) in fracs.iter().enumerate().take(3) {
        assert_ne!(
            result.point(i)[0],
            BAD_VALUE,
            "Batch point {} (frac {}) should be inside",
            i,
            frac
        );
    }

    // Points at fractions 1.01, 1.5, 2.0 are outside (become BAD).
    for (i, &frac) in fracs.iter().enumerate().skip(3) {
        assert_eq!(
            result.point(i)[0],
            BAD_VALUE,
            "Batch point {} (frac {}) should be outside",
            i,
            frac
        );
    }
}

// ============================================================================
// Box on SkyFrame — negation
// ============================================================================

#[test]
fn sky_box_negated() {
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = [1.0, 0.5];
    let corner = [1.1, 0.6];

    let mut bx = BoxRegionBuilder::new(sky, centre.to_vec(), corner.to_vec())
        .unwrap()
        .build()
        .unwrap();

    // Before negation: centre is inside.
    assert!(bx.point_in_region(&centre).unwrap());
    // Distant point is outside.
    assert!(!bx.point_in_region(&[3.0, 0.0]).unwrap());

    // After negation: centre is outside.
    bx.negate();
    assert!(!bx.point_in_region(&centre).unwrap());
    // Distant point is inside.
    assert!(bx.point_in_region(&[3.0, 0.0]).unwrap());
}

// ============================================================================
// Prism with SkyFrame Circle — 3D region combining sky + spectral axis
// ============================================================================

#[test]
fn sky_circle_prism_with_interval() {
    // Prism: sky circle (2D) extruded by 1D interval.
    // This models a cone on the sky at all frequencies in a range.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let circle = CircleBuilder::new(sky.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();

    let frm1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval = IntervalBuilder::new(frm1, vec![100.0], vec![200.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(circle, interval)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(prism.naxes(), 3);

    // Inside both sky circle and interval.
    assert!(
        prism.point_in_region(&[1.0, 0.5, 150.0]).unwrap(),
        "Sky prism: inside"
    );

    // Inside circle, outside interval.
    assert!(
        !prism.point_in_region(&[1.0, 0.5, 250.0]).unwrap(),
        "Sky prism: outside interval"
    );

    // Outside circle, inside interval.
    let p_outside_sky = sky.offset(&[1.0, 0.5], &[1.5, 0.5], 0.15).unwrap();
    assert!(
        !prism
            .point_in_region(&[p_outside_sky[0], p_outside_sky[1], 150.0])
            .unwrap(),
        "Sky prism: outside circle"
    );
}

// ============================================================================
// Circle on SkyFrame — verify that great-circle distance is used
//
// Key test: on a SkyFrame, the distance is the great-circle distance.
// At high declination, a circle centred at Dec=80 deg should NOT use
// Euclidean coordinate distance (which would give the wrong shape due
// to RA coordinate compression near the pole).
// ============================================================================

#[test]
fn sky_circle_great_circle_distance_correctness() {
    // At Dec=80 degrees (1.396 rad), RA coordinates are compressed.
    // Two points separated by delta_RA=0.5 at Dec=80 have great-circle
    // distance much less than 0.5 radians (it is approx 0.5*cos(80) ~ 0.087).
    let sky = SkyFrame::new(SkySystem::Icrs);

    let p1 = [1.0, 1.396]; // RA=1.0, Dec=80 deg
    let p2 = [1.5, 1.396]; // RA=1.5, Dec=80 deg

    let gc_dist = sky.distance(&p1, &p2).unwrap();
    // Great-circle distance should be about 0.5 * cos(80 deg) ~ 0.087
    let expected = 0.5 * 1.396_f64.cos();
    assert!(
        (gc_dist - expected).abs() < 0.01,
        "Great-circle distance at Dec=80: expected ~{:.4}, got {:.4}",
        expected,
        gc_dist
    );

    // Circle centred at p1 with radius = gc_dist should include p2.
    let circle = CircleBuilder::new(sky, vec![p1[0], p1[1]])
        .unwrap()
        .edge_point(vec![p2[0], p2[1]])
        .build()
        .unwrap();

    // p2 is on the boundary (closed), so it's inside.
    assert!(
        circle.point_in_region(&p2).unwrap(),
        "Edge point is inside closed circle"
    );

    // A point at same RA offset but at Dec=0 would be at distance 0.5 rad,
    // which is much larger than gc_dist. If we used Euclidean coordinates
    // (wrong), that point would seem at the same distance as p2 — but with
    // great-circle, it's actually much farther.
    // A point at same RA offset but at lower Dec where it's truly farther away.
    let p4 = [1.5, 0.0]; // RA=1.5, Dec=0 — far from p1
    assert!(
        !circle.point_in_region(&p4).unwrap(),
        "Point at equator with same delta-RA is outside"
    );
}

// ============================================================================
// System-change and simplification tests
//
// These tests verify that regions correctly handle coordinate system changes
// via with_integrity, and that simplified() produces functionally equivalent
// regions. This mirrors the patterns in testregions.c where regions are
// created in one system and then the system attribute is changed.
// ============================================================================

#[test]
fn system_change_circle_membership() {
    // Create a Circle on SkyFrame(ICRS) at (1.0, 0.5) with radius 0.01 rad.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let mut circle = CircleBuilder::new(sky.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();

    // Change system to Galactic via with_integrity.
    circle
        .with_integrity(|frame| {
            let sky_ref = frame.as_any_mut().downcast_mut::<SkyFrame>().unwrap();
            sky_ref.set_system(SkySystem::Galactic);
            Ok(())
        })
        .unwrap();

    // Convert the centre point from ICRS to Galactic.
    let icrs_frame = SkyFrame::new(SkySystem::Icrs);
    let gal_frame = SkyFrame::new(SkySystem::Galactic);
    let conversion = icrs_frame.convert_to(&gal_frame, &[]).unwrap().unwrap();

    let centre_pts = PointSet::from_rows([[1.0, 0.5]]);
    let centre_gal = conversion.transform(&centre_pts, true).unwrap();
    let gal_centre = centre_gal.point(0);

    // The centre (in Galactic coords) should be inside the circle.
    assert!(
        circle.point_in_region(gal_centre).unwrap(),
        "Centre (converted to Galactic) should be inside the system-changed circle"
    );

    // A point far away in Galactic coords should be outside.
    let far_point = [gal_centre[0] + 1.0, gal_centre[1]];
    assert!(
        !circle.point_in_region(&far_point).unwrap(),
        "Point far from centre should be outside"
    );
}

#[test]
fn system_change_circle_simplify() {
    // Create a Circle on SkyFrame(ICRS) and change system to Galactic.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let mut circle = CircleBuilder::new(sky.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();

    circle
        .with_integrity(|frame| {
            let sky_ref = frame.as_any_mut().downcast_mut::<SkyFrame>().unwrap();
            sky_ref.set_system(SkySystem::Galactic);
            Ok(())
        })
        .unwrap();

    // Convert the centre point to Galactic.
    let icrs_frame = SkyFrame::new(SkySystem::Icrs);
    let gal_frame = SkyFrame::new(SkySystem::Galactic);
    let conversion = icrs_frame.convert_to(&gal_frame, &[]).unwrap().unwrap();

    let centre_pts = PointSet::from_rows([[1.0, 0.5]]);
    let centre_gal = conversion.transform(&centre_pts, true).unwrap();
    let gal_centre = centre_gal.point(0);

    // Simplify the circle (as a Mapping).
    let simplified = circle.simplified().unwrap();

    // The simplified mapping should still pass the Galactic centre through
    // (it is inside the region).
    let test_pts = PointSet::from_rows([[gal_centre[0], gal_centre[1]]]);
    let result = simplified.transform(&test_pts, true).unwrap();

    assert_ne!(
        result.point(0)[0],
        BAD_VALUE,
        "Simplified region should pass centre through (inside)"
    );
    assert!(
        (result.point(0)[0] - gal_centre[0]).abs() < 1.0e-10,
        "Simplified region should preserve coordinate value for interior point"
    );

    // A point far away should be masked (set to BAD).
    let far_pts = PointSet::from_rows([[gal_centre[0] + 1.0, gal_centre[1]]]);
    let far_result = simplified.transform(&far_pts, true).unwrap();
    assert_eq!(
        far_result.point(0)[0],
        BAD_VALUE,
        "Simplified region should mask exterior points"
    );
}

#[test]
fn system_change_circle_overlap_native_galactic() {
    // Overlap algorithm currently assumes mesh (base frame) and point_in_region
    // (current frame) are in the same coordinate system. This works when the
    // region is created directly in a system (base==current, identity mapping).
    //
    // Test: create two circles directly in Galactic and verify overlap is Same.
    let gal = SkyFrame::new(SkySystem::Galactic);
    let circle1 = CircleBuilder::new(gal.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();
    let circle2 = CircleBuilder::new(gal.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();

    let result = circle1.overlap(&circle2).unwrap();
    assert_eq!(
        result,
        Overlap::Same,
        "Identical circles in Galactic should overlap as Same"
    );

    // A smaller circle should be FirstInside.
    let small = CircleBuilder::new(gal.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.005)
        .build()
        .unwrap();
    let result2 = small.overlap(&circle1).unwrap();
    assert_eq!(
        result2,
        Overlap::FirstInside,
        "Smaller circle should be inside larger one"
    );
}

#[test]
fn system_change_box_membership() {
    // Create a BoxRegion on SkyFrame(ICRS).
    // Centre at (1.0, 0.5), corner at (1.005, 0.505) — a small box.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let mut bx = BoxRegionBuilder::new(sky.clone(), vec![1.0, 0.5], vec![1.005, 0.505])
        .unwrap()
        .build()
        .unwrap();

    // Verify centre is inside before system change.
    assert!(
        bx.point_in_region(&[1.0, 0.5]).unwrap(),
        "Centre should be inside the box (ICRS)"
    );

    // Change system to Galactic via with_integrity.
    bx.with_integrity(|frame| {
        let sky_ref = frame.as_any_mut().downcast_mut::<SkyFrame>().unwrap();
        sky_ref.set_system(SkySystem::Galactic);
        Ok(())
    })
    .unwrap();

    // Convert the centre from ICRS to Galactic.
    let icrs_frame = SkyFrame::new(SkySystem::Icrs);
    let gal_frame = SkyFrame::new(SkySystem::Galactic);
    let conversion = icrs_frame.convert_to(&gal_frame, &[]).unwrap().unwrap();

    let centre_pts = PointSet::from_rows([[1.0, 0.5]]);
    let centre_gal = conversion.transform(&centre_pts, true).unwrap();
    let gal_centre = centre_gal.point(0);

    // The centre (in Galactic coords) should be inside the box.
    assert!(
        bx.point_in_region(gal_centre).unwrap(),
        "Centre (converted to Galactic) should be inside the system-changed box"
    );

    // A point far away should be outside.
    let far_point = [gal_centre[0] + 1.0, gal_centre[1]];
    assert!(
        !bx.point_in_region(&far_point).unwrap(),
        "Point far from centre should be outside the box"
    );
}

#[test]
fn system_change_simplified_transform_consistent() {
    // After simplification, verify that the simplified region is functionally
    // equivalent to the original for multiple test points.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let mut circle = CircleBuilder::new(sky.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.02)
        .build()
        .unwrap();

    circle
        .with_integrity(|frame| {
            let sky_ref = frame.as_any_mut().downcast_mut::<SkyFrame>().unwrap();
            sky_ref.set_system(SkySystem::Galactic);
            Ok(())
        })
        .unwrap();

    // Convert several test points from ICRS to Galactic.
    let icrs_frame = SkyFrame::new(SkySystem::Icrs);
    let gal_frame = SkyFrame::new(SkySystem::Galactic);
    let conversion = icrs_frame.convert_to(&gal_frame, &[]).unwrap().unwrap();

    // Points: centre, slightly inside, slightly outside, far away.
    let test_icrs = PointSet::from_rows([
        [1.0, 0.5],   // centre
        [1.005, 0.5], // slightly offset in RA (should be inside r=0.02)
        [1.05, 0.5],  // further offset (likely outside)
        [2.0, 1.0],   // far away (definitely outside)
    ]);
    let test_gal = conversion.transform(&test_icrs, true).unwrap();

    // Transform through the original circle.
    let orig_result = circle.transform(&test_gal, true).unwrap();

    // Simplify and transform.
    let simplified = circle.simplified().unwrap();
    let simp_result = simplified.transform(&test_gal, true).unwrap();

    // Both should agree on which points are inside/outside.
    for i in 0..4 {
        let orig_bad = orig_result.point(i)[0] == BAD_VALUE;
        let simp_bad = simp_result.point(i)[0] == BAD_VALUE;
        assert_eq!(
            orig_bad, simp_bad,
            "Point {i}: original and simplified should agree on membership"
        );
        if !orig_bad {
            // Interior points should pass through with same values.
            assert!(
                (orig_result.point(i)[0] - simp_result.point(i)[0]).abs() < 1.0e-10,
                "Point {i}: interior coordinate values should match"
            );
        }
    }
}

#[test]
fn system_change_circle_serialization_roundtrip() {
    use ast_core::{AnyObject, from_native_string, to_native_string};

    // Create a Circle on SkyFrame(ICRS) and change system to Galactic.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let mut circle = CircleBuilder::new(sky.clone(), vec![1.0, 0.5])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();

    circle
        .with_integrity(|frame| {
            let sky_ref = frame.as_any_mut().downcast_mut::<SkyFrame>().unwrap();
            sky_ref.set_system(SkySystem::Galactic);
            Ok(())
        })
        .unwrap();

    // Serialize to native format.
    let obj = AnyObject::Circle(circle.clone());
    let text = to_native_string(&obj).unwrap();

    // Deserialize.
    let restored_obj = from_native_string(&text).unwrap();
    let restored = match restored_obj {
        AnyObject::Circle(c) => c,
        other => panic!("Expected Circle, got {:?}", other),
    };

    // Convert centre to Galactic for testing.
    let icrs_frame = SkyFrame::new(SkySystem::Icrs);
    let gal_frame = SkyFrame::new(SkySystem::Galactic);
    let conversion = icrs_frame.convert_to(&gal_frame, &[]).unwrap().unwrap();

    let centre_pts = PointSet::from_rows([[1.0, 0.5]]);
    let centre_gal = conversion.transform(&centre_pts, true).unwrap();
    let gal_centre = centre_gal.point(0);

    // The restored region should still accept Galactic coordinates.
    assert!(
        restored.point_in_region(gal_centre).unwrap(),
        "Restored circle should recognize centre (in Galactic) as inside"
    );

    // A far point should still be outside.
    let far_point = [gal_centre[0] + 1.0, gal_centre[1]];
    assert!(
        !restored.point_in_region(&far_point).unwrap(),
        "Restored circle should reject far point"
    );
}

// ============================================================================
// FitsChan-based region integration tests
// (from testregions.c lines 1619-1656)
//
// Read FITS WCS headers via FitsChan to create a SkyFrame, then create
// circles and verify bounds, disc, and membership.
// ============================================================================

/// Helper: create a FrameSet from a minimal TAN-projection WCS header.
///
/// The header produces a 2D SkyFrame (ICRS, RA/Dec in radians) as the
/// current frame of the returned FrameSet.
fn fitschan_tan_skyframe() -> FrameSet {
    let header = "\
NAXIS   =                    2
NAXIS1  =                  200
NAXIS2  =                  200
CTYPE1  = 'RA---TAN'
CTYPE2  = 'DEC--TAN'
CRPIX1  =                  100
CRPIX2  =                  100
CRVAL1  =          71.61972400
CRVAL2  =         -35.22878700
CDELT1  = -5.5555555555555E-04
CDELT2  =  5.5555555555555E-04
CROTA2  =              0.00000
";
    let mut fc = FitsChan::from_header(header).unwrap();
    fc.rewind();
    let obj = fc
        .read()
        .unwrap()
        .expect("FitsChan read should produce an object");
    match obj {
        AnyObject::FrameSet(fs) => fs,
        other => panic!("Expected FrameSet from FitsChan, got {:?}", other),
    }
}

#[test]
fn fitschan_sky_circle_bounds() {
    // C testregions.c lines 1637-1648:
    // Circle at centre (0.0, 1.0) with radius 0.01 rad on the SkyFrame
    // from a TAN-projection WCS header.
    let fs = fitschan_tan_skyframe();

    let circle = CircleBuilder::new(fs, vec![0.0, 1.0])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();

    let (lbnd, ubnd) = circle.get_region_bounds().unwrap();

    // Bounds come from the great-circle mesh (circle.c::RegBaseMesh +
    // skyframe.c::NormBox), so the RA extent carries the cos(dec) correction
    // (radius/cos(1.0) = 0.01850666) and the Dec upper carries the
    // great-circle correction (~1.00999499). Dec lower stays at 0.99.
    assert!(
        (lbnd[0] - (-0.01850666)).abs() < 1.0e-6,
        "fitschan circle lbnd[0]: got {}, expected -0.01850666",
        lbnd[0]
    );
    assert!(
        (lbnd[1] - 0.99).abs() < 1.0e-6,
        "fitschan circle lbnd[1]: got {}, expected 0.99",
        lbnd[1]
    );
    assert!(
        (ubnd[0] - 0.01850666).abs() < 1.0e-6,
        "fitschan circle ubnd[0]: got {}, expected 0.01850666",
        ubnd[0]
    );
    assert!(
        (ubnd[1] - 1.00999499).abs() < 1.0e-6,
        "fitschan circle ubnd[1]: got {}, expected 1.00999499",
        ubnd[1]
    );

    // Verify get_region_disc returns the expected centre and radius.
    let (disc_centre, disc_radius) = circle.get_region_disc().unwrap();
    assert!(
        disc_centre[0].abs() < 1.0e-5,
        "fitschan circle disc centre[0]: got {}, expected ~0.0",
        disc_centre[0]
    );
    assert!(
        (disc_centre[1] - 1.0).abs() < 1.0e-5,
        "fitschan circle disc centre[1]: got {}, expected ~1.0",
        disc_centre[1]
    );
    assert!(
        (disc_radius - 0.01).abs() < 1.0e-5,
        "fitschan circle disc radius: got {}, expected 0.01",
        disc_radius
    );
}

#[test]
fn fitschan_sky_circle_membership() {
    // Circle at (1.2217305, 1.3962634) with edge point at (0.8, 0.8).
    // Test membership using frame.distance / frame.offset to generate
    // points just inside and just outside.
    let fs = fitschan_tan_skyframe();

    let centre = [1.2217305, 1.3962634];
    let edge = [0.8, 0.8];

    // Compute radius as great-circle distance.
    let rad = fs.distance(&centre, &edge).unwrap();
    assert!(rad > 0.0, "radius must be positive");

    let circle = CircleBuilder::new(fs.clone(), vec![centre[0], centre[1]])
        .unwrap()
        .edge_point(vec![edge[0], edge[1]])
        .build()
        .unwrap();

    // Point at 0.999 * radius along centre->edge direction: inside.
    let p_inside = fs.offset(&centre, &edge, rad * 0.999).unwrap();
    // Point at 1.001 * radius: outside.
    let p_outside = fs.offset(&centre, &edge, rad * 1.001).unwrap();

    // Transform through the circle region.
    let pts = PointSet::from_rows([[p_inside[0], p_inside[1]], [p_outside[0], p_outside[1]]]);
    let result = circle.transform(&pts, true).unwrap();

    // Inside point passes through unchanged.
    assert_eq!(
        result.point(0)[0],
        p_inside[0],
        "fitschan membership: inside RA should pass"
    );
    assert_eq!(
        result.point(0)[1],
        p_inside[1],
        "fitschan membership: inside Dec should pass"
    );

    // Outside point becomes BAD.
    assert_eq!(
        result.point(1)[0],
        BAD_VALUE,
        "fitschan membership: outside RA should be BAD"
    );
    assert_eq!(
        result.point(1)[1],
        BAD_VALUE,
        "fitschan membership: outside Dec should be BAD"
    );
}

#[test]
fn fitschan_sky_circle_near_pole_bounds() {
    // C testregions.c lines 1649-1656:
    // Circle at (0.0, 1.57) rad with radius 0.01 — near north pole.
    // Bounds should wrap in RA: lbnd[0]=0.0, ubnd[0]=2*pi.
    // Dec bounds: lbnd[1] ~ 1.56, ubnd[1] = pi/2.
    let fs = fitschan_tan_skyframe();

    let circle = CircleBuilder::new(fs, vec![0.0, 1.57])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();

    let (lbnd, ubnd) = circle.get_region_bounds().unwrap();

    // Near the pole, the RA bounds should encompass the full range [0, 2*pi].
    assert!(
        lbnd[0].abs() < 1.0e-6,
        "near-pole circle lbnd[0]: got {}, expected 0.0",
        lbnd[0]
    );
    assert!(
        (ubnd[0] - 2.0 * PI).abs() < 1.0e-6,
        "near-pole circle ubnd[0]: got {}, expected 2*pi ({})",
        ubnd[0],
        2.0 * PI
    );

    // Dec bounds: lower ~ centre - radius = 1.57 - 0.01 = 1.56
    assert!(
        (lbnd[1] - 1.56).abs() < 1.0e-2,
        "near-pole circle lbnd[1]: got {}, expected ~1.56",
        lbnd[1]
    );
    // Upper capped at pi/2 (north pole).
    assert!(
        (ubnd[1] - FRAC_PI_2).abs() < 1.0e-6,
        "near-pole circle ubnd[1]: got {}, expected pi/2 ({})",
        ubnd[1],
        FRAC_PI_2
    );
}

// ============================================================================
// perm_axes tests (ported from testregions.c lines 821-854, 1028, 1936)
// ============================================================================

#[test]
fn perm_axes_box_swap_and_transform() {
    // Create a Box on a SkyFrame and permute axes [2, 1].
    // After permutation, axis 1 is latitude and axis 2 is longitude.
    let sky = SkyFrame::new(SkySystem::Galactic);
    let centre = vec![1.25, 0.75]; // [lon, lat]
    let corner = vec![1.5, 0.5]; // [lon, lat]
    let mut box1 = BoxRegionBuilder::new(sky, centre, corner)
        .unwrap()
        .build()
        .unwrap();

    // Verify a point inside before permutation.
    let pts_in = PointSet::from_rows(std::iter::once([1.3, 0.7].as_slice()));
    let result = box1.transform(&pts_in, true).unwrap();
    assert!(
        result.point(0)[0] != BAD_VALUE,
        "point should be inside before perm"
    );

    // Permute: swap axes.
    box1.perm_axes(&[2, 1]).unwrap();

    // After permutation, coordinates are [lat, lon]. The same geometric
    // point [lon=1.3, lat=0.7] is now provided as [0.7, 1.3].
    let pts_swapped = PointSet::from_rows(std::iter::once([0.7, 1.3].as_slice()));
    let result2 = box1.transform(&pts_swapped, true).unwrap();
    assert!(
        result2.point(0)[0] != BAD_VALUE,
        "point should be inside after perm (swapped coords)"
    );

    // The same coords in original order should now be OUTSIDE.
    let pts_wrong_order = PointSet::from_rows(std::iter::once([1.3, 0.7].as_slice()));
    let result3 = box1.transform(&pts_wrong_order, true).unwrap();
    assert_eq!(
        result3.point(0)[0],
        BAD_VALUE,
        "original-order coords should be outside after perm"
    );
}

#[test]
fn perm_axes_double_swap_is_identity() {
    // Applying [2, 1] twice should return to the original state.
    let sky = SkyFrame::new(SkySystem::Icrs);
    let centre = vec![1.0, 0.5];
    let corner = vec![1.2, 0.7];
    let mut box1 = BoxRegionBuilder::new(sky, centre, corner)
        .unwrap()
        .build()
        .unwrap();

    // Format axis 1 before any permutation.
    let fmt_before = box1.format(1, 0.25).unwrap();

    // Permute twice.
    box1.perm_axes(&[2, 1]).unwrap();
    box1.perm_axes(&[2, 1]).unwrap();

    // Format should be the same as before.
    let fmt_after = box1.format(1, 0.25).unwrap();
    assert_eq!(
        fmt_before, fmt_after,
        "double permutation should be identity"
    );

    // Verify transform still works correctly.
    let pts = PointSet::from_rows(std::iter::once([1.1, 0.6].as_slice()));
    let result = box1.transform(&pts, true).unwrap();
    assert!(
        result.point(0)[0] != BAD_VALUE,
        "centre point should still be inside after double perm"
    );
}

#[test]
fn perm_axes_format_changes_axis_type() {
    // After permuting a SkyFrame, axis 1 format should change from
    // HMS (longitude) to DMS (latitude).
    let sky = SkyFrame::new(SkySystem::Fk5);
    let centre = vec![1.0, 0.5];
    let corner = vec![1.2, 0.7];
    let mut box1 = BoxRegionBuilder::new(sky, centre, corner)
        .unwrap()
        .build()
        .unwrap();

    // Before permutation: axis 1 is RA (HMS format).
    let fmt1_before = box1.format(1, 1.0).unwrap();
    // axis 2 is Dec (DMS format).
    let fmt2_before = box1.format(2, 1.0).unwrap();

    // Permute.
    box1.perm_axes(&[2, 1]).unwrap();

    // After permutation: axis 1 should now be Dec (DMS), axis 2 should be RA (HMS).
    let fmt1_after = box1.format(1, 1.0).unwrap();
    let fmt2_after = box1.format(2, 1.0).unwrap();

    assert_eq!(
        fmt1_after, fmt2_before,
        "after perm, axis 1 format should match old axis 2"
    );
    assert_eq!(
        fmt2_after, fmt1_before,
        "after perm, axis 2 format should match old axis 1"
    );
}

#[test]
fn perm_axes_basicframe_box() {
    // Permute axes on a BasicFrame Box and verify transform.
    let frame = BasicFrameBuilder::new(3)
        .label(1, "X")
        .label(2, "Y")
        .label(3, "Z")
        .build()
        .unwrap();
    let centre = vec![5.0, 10.0, 15.0];
    let corner = vec![7.0, 12.0, 17.0];
    let mut box1 = BoxRegionBuilder::new(frame, centre, corner)
        .unwrap()
        .build()
        .unwrap();

    // Point inside: [6, 11, 16]
    let pts_in = PointSet::from_rows(std::iter::once([6.0, 11.0, 16.0].as_slice()));
    let result = box1.transform(&pts_in, true).unwrap();
    assert!(result.point(0)[0] != BAD_VALUE, "should be inside");

    // Permute [2, 3, 1]: new axis 1 = old axis 2, new axis 2 = old axis 3, new axis 3 = old axis 1
    box1.perm_axes(&[2, 3, 1]).unwrap();

    // The same geometric point in permuted order: [Y=11, Z=16, X=6]
    let pts_perm = PointSet::from_rows(std::iter::once([11.0, 16.0, 6.0].as_slice()));
    let result2 = box1.transform(&pts_perm, true).unwrap();
    assert!(
        result2.point(0)[0] != BAD_VALUE,
        "permuted coords should be inside"
    );

    // Original order should fail.
    let result3 = box1.transform(&pts_in, true).unwrap();
    assert_eq!(
        result3.point(0)[0],
        BAD_VALUE,
        "original-order coords should be outside after [2,3,1] perm"
    );
}

#[test]
fn perm_axes_circle_preserves_membership() {
    // Permute a Circle on SkyFrame and verify membership with swapped coords.
    let sky = SkyFrame::new(SkySystem::Galactic);
    let centre = vec![0.5, 0.3]; // [lon, lat]
    let mut circle = CircleBuilder::new(sky, centre)
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();

    // Point near centre is inside.
    assert!(circle.point_in_region(&[0.52, 0.31]).unwrap());

    // Permute.
    circle.perm_axes(&[2, 1]).unwrap();

    // Same point with swapped coords [lat, lon] should be inside.
    assert!(circle.point_in_region(&[0.31, 0.52]).unwrap());

    // Original order should be outside (0.52 as lat is far from 0.3 lat).
    assert!(!circle.point_in_region(&[0.52, 0.31]).unwrap());
}

// ============================================================================
// pick_axes tests (ported from testregions.c line 1933)
// ============================================================================

#[test]
fn pick_axes_skyframe_one_axis() {
    // Pick axis 1 (longitude) from a 2-axis SkyFrame.
    // Result should be a 1-axis BasicFrame.
    let sky = SkyFrame::new(SkySystem::Fk4);
    let (picked, mapping) = pick_axes(&sky, &[1]).unwrap();

    assert_eq!(picked.naxes(), 1);
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 1);

    // Forward transform: [lon, lat] → [lon]
    let pts = PointSet::from_rows(std::iter::once([1.5, 0.7].as_slice()));
    let result = mapping.transform(&pts, true).unwrap();
    assert!((result.point(0)[0] - 1.5).abs() < 1e-10);
}

#[test]
fn pick_axes_basicframe_subset() {
    // Pick axes [1, 3] from a 3-axis BasicFrame.
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let (picked, mapping) = pick_axes(&frame, &[1, 3]).unwrap();

    assert_eq!(picked.naxes(), 2);
    assert_eq!(mapping.nin(), 3);
    assert_eq!(mapping.nout(), 2);

    // Forward: [x, y, z] → [x, z]
    let pts = PointSet::from_rows(std::iter::once([10.0, 20.0, 30.0].as_slice()));
    let result = mapping.transform(&pts, true).unwrap();
    assert!((result.point(0)[0] - 10.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 30.0).abs() < 1e-10);

    // Inverse: [x, z] → [x, BAD, z]
    let pts2 = PointSet::from_rows(std::iter::once([10.0, 30.0].as_slice()));
    let result2 = mapping.transform(&pts2, false).unwrap();
    assert!((result2.point(0)[0] - 10.0).abs() < 1e-10);
    assert_eq!(result2.point(0)[1], BAD_VALUE);
    assert!((result2.point(0)[2] - 30.0).abs() < 1e-10);
}

// ============================================================================
// remove_regions tests (ported from testregions.c lines 2494-2542)
// ============================================================================

#[test]
fn remove_regions_from_frameset() {
    // Create a Circle and verify it clips points.
    let sf1 = SkyFrame::new(SkySystem::Icrs);
    let circle = CircleBuilder::new(sf1.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(0.001)
        .build()
        .unwrap();

    // Point inside (0, 0) passes through.
    let pts_in = PointSet::from_rows(std::iter::once([0.0, 0.0].as_slice()));
    let result = circle.transform(&pts_in, true).unwrap();
    assert!(result.point(0)[0] != BAD_VALUE, "centre should be inside");

    // Point outside (0.01, 0.01) is clipped.
    let pts_out = PointSet::from_rows(std::iter::once([0.01, 0.01].as_slice()));
    let result2 = circle.transform(&pts_out, true).unwrap();
    assert_eq!(
        result2.point(0)[0],
        BAD_VALUE,
        "distant point should be clipped"
    );

    // Now build a FrameSet that has the circle as part of its mapping.
    // Use convert to get ICRS→Galactic through the circle.
    let sf2 = SkyFrame::new(SkySystem::Galactic);
    let fs = sf1.convert_to(&sf2, &[]).unwrap().unwrap();

    // The FrameSet's mapping from base to current transforms coordinates.
    let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // Transform (0, 0) through the FrameSet's mapping.
    let result3 = map.transform(&pts_in, true).unwrap();
    assert!(
        result3.point(0)[0] != BAD_VALUE,
        "origin should transform through FrameSet"
    );

    // After remove_regions, the mapping should pass all points through
    // (no clipping behaviour from regions).
    let clean_map = remove_regions(map.as_ref());
    let result4 = clean_map.transform(&pts_out, true).unwrap();
    assert!(
        result4.point(0)[0] != BAD_VALUE,
        "remove_regions should eliminate clipping"
    );
}

#[test]
fn remove_regions_unit_passthrough() {
    // remove_regions on a non-region mapping returns an equivalent mapping.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let fs = FrameSet::new(Box::new(frame));
    let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();

    let clean = remove_regions(map.as_ref());
    assert_eq!(clean.nin(), 2);
    assert_eq!(clean.nout(), 2);

    let pts = PointSet::from_rows(std::iter::once([3.0, 4.0].as_slice()));
    let result = clean.transform(&pts, true).unwrap();
    assert!((result.point(0)[0] - 3.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 4.0).abs() < 1e-10);
}

// ============================================================================
// convex_hull tests (ported from testregions.c lines 2543-2584)
// ============================================================================

#[test]
fn convex_hull_basic() {
    use ast_core::regions::Polygon;

    // Reproduce the test from testregions.c checkConvex.
    let nx = 8usize;
    let ny = 7usize;
    let lbnd: [i64; 2] = [-10, 3];
    let mut array = vec![0.0f32; nx * ny];

    // Set pixels (using 1-based C indexing from the test, converted to 0-based).
    array[5] = 1.0; // col=5, row=0 → grid(-5, 3)
    array[6] = 1.0; // col=6, row=0 → grid(-4, 3)
    array[7] = 1.0; // col=7, row=0 → grid(-3, 3)
    array[6 + nx] = 1.0; // col=6, row=1 → grid(-4, 4)
    array[7 + nx] = 1.0; // col=7, row=1 → grid(-3, 4)
    array[1 + 2 * nx] = 1.0; // col=1, row=2 → grid(-9, 5)
    array[7 + 2 * nx] = 1.0; // col=7, row=2 → grid(-3, 5)
    array[3 * nx] = 1.0; // col=0, row=3 → grid(-10, 6)
    array[5 * nx] = 1.0; // col=0, row=5 → grid(-10, 8)
    array[1 + 5 * nx] = 1.0; // col=1, row=5 → grid(-9, 8)
    array[5 + 5 * nx] = 1.0; // col=5, row=5 → grid(-5, 8)

    let poly = Polygon::convex_hull_f32(1.0, true, &array, nx, ny, lbnd).unwrap();

    // The C test expects 7 hull vertices.
    let pts = poly.get_region_points().unwrap();
    assert_eq!(
        pts.npoint(),
        7,
        "expected 7 hull vertices, got {}",
        pts.npoint()
    );

    // Verify the hull vertices match expected values.
    // Note: vertex order may differ from C depending on algorithm.
    // Collect all vertices as a set and check they contain the expected points.
    let mut vertices: Vec<(i64, i64)> = (0..pts.npoint())
        .map(|i| {
            let p = pts.point(i);
            (p[0].round() as i64, p[1].round() as i64)
        })
        .collect();
    vertices.sort();

    let mut expected = vec![
        (-10, 6),
        (-10, 8),
        (-9, 5),
        (-5, 3),
        (-5, 8),
        (-3, 3),
        (-3, 5),
    ];
    expected.sort();

    assert_eq!(vertices, expected, "hull vertices don't match expected set");
}

// ============================================================================
// Uncertainty carry-forward through Prism::simplified
//
// No equal()-checking simplify fixture exercises this: Interval::equal ignores
// the uncertainty Region, so the corpus cannot detect whether an explicit
// uncertainty survives a Prism collapse. This standalone test pins the A1
// carry-forward behaviour directly.
//
// A Prism of two bounded 1-D Intervals collapses (prism.c::Simplify → parallel
// CmpMap → MergeInterval's Interval+Interval branch → reclassify to Box) to a
// single 2-D Region. Any uncertainty stored explicitly on the *Prism itself*
// must be transferred to the collapsed Region — C does this via
// `astTestUnc(reg)` / `astGetUnc` / `astSetUnc` (prism.c:3493-3497), mirrored
// in `Prism::simplified` by `set_uncertainty` after the collapse+map_region.
// ============================================================================

#[test]
fn prism_simplify_carries_explicit_uncertainty() {
    let frm_a = BasicFrameBuilder::new(1).build().unwrap();
    let interval_a = IntervalBuilder::new(frm_a, vec![0.0], vec![10.0])
        .unwrap()
        .build()
        .unwrap();

    let frm_b = BasicFrameBuilder::new(1).build().unwrap();
    let interval_b = IntervalBuilder::new(frm_b, vec![0.0], vec![20.0])
        .unwrap()
        .build()
        .unwrap();

    // Explicit 2-D uncertainty stored on the Prism: a small box spanning
    // [-0.1, 0.1] x [-0.2, 0.2] about the origin.
    let unc_frm = BasicFrameBuilder::new(2).build().unwrap();
    let unc = BoxRegionBuilder::new(unc_frm, vec![0.0, 0.0], vec![0.1, 0.2])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(interval_a, interval_b)
        .unwrap()
        .uncertainty(Box::new(unc))
        .build()
        .unwrap();

    let simplified = Mapping::simplified(&prism).unwrap();

    // The two bounded 1-D Intervals merge and reclassify to a single 2-D
    // Region (a BoxRegion, since both are bounded and closed — not a Prism).
    let any = simplified.as_ref().as_any();
    let carried = if let Some(bx) = any.downcast_ref::<BoxRegion>() {
        assert_eq!(bx.naxes(), 2, "collapsed Region should be 2-D");
        bx.get_uncertainty()
    } else if let Some(iv) = any.downcast_ref::<Interval>() {
        assert_eq!(iv.naxes(), 2, "collapsed Region should be 2-D");
        iv.get_uncertainty()
    } else {
        panic!("simplified Prism should collapse to a single 2-D Interval/Box");
    };

    // The explicit uncertainty stored on the Prism must survive the collapse
    // and still span both axes.
    let carried = carried.expect("explicit uncertainty must be carried forward");
    let (ulb, uub) = carried.get_region_bounds().unwrap();
    assert_eq!(ulb.len(), 2, "carried uncertainty should be 2-D");
    assert!(
        (uub[0] - ulb[0] - 0.2).abs() < 1e-9,
        "axis-0 uncertainty width"
    );
    assert!(
        (uub[1] - ulb[1] - 0.4).abs() < 1e-9,
        "axis-1 uncertainty width"
    );
}

// ============================================================================
// set_uncertainty (astSetUnc) computational parity
//
// The native round-trip corpus reads a pre-`SetUnc` object and re-serializes
// it, so it validates read/write symmetry but not the `set_uncertainty`
// computation (C `astLoadRegion` stores the `Unc` node directly, bypassing
// `astSetUnc`). These tests reconstruct the region and uncertainty from the
// same primitives the C fixture generator uses, invoke `set_uncertainty`, and
// compare the serialization byte-for-byte against the committed oracle `.map`,
// pinning the validate/remap/negate/RegionFS/recentre body directly.
// ============================================================================

fn setunc_fixture_map(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/native/reference/simplify_fixtures/{}.map",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

#[test]
fn set_uncertainty_unitmap_clears_region_fs() {
    // Oracle: setunc_unitmap_regionfs_omitted. Region and uncertainty share a
    // plain 2-D Frame, so the remap into the region's base Frame is a UnitMap.
    // C clears the uncertainty's RegionFS attribute (region.c:10869) so its
    // FrameSet is omitted from the dump, and re-centres it at the box centre
    // (5, 5).
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let mut region = BoxRegionBuilder::from_bounds(frm, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let ufrm = BasicFrameBuilder::new(2).build().unwrap();
    let unc = BoxRegionBuilder::from_bounds(ufrm, &[-0.5, -0.5], &[0.5, 0.5])
        .unwrap()
        .build()
        .unwrap();

    region.set_uncertainty(Box::new(unc));

    let got = ast_core::to_native_string(&AnyObject::BoxRegion(region)).unwrap();
    assert_eq!(got, setunc_fixture_map("setunc_unitmap_regionfs_omitted"));
}

#[test]
fn set_uncertainty_negates_unbounded() {
    // Oracle: setunc_unbounded_negated. A negated (hence unbounded) uncertainty
    // Box is supplied; C negates it again (region.c:10856) to restore a bounded
    // uncertainty before storing, then re-centres it at the box centre (5, 5).
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let mut region = BoxRegionBuilder::from_bounds(frm, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let ufrm = BasicFrameBuilder::new(2).build().unwrap();
    let mut unc = BoxRegionBuilder::from_bounds(ufrm, &[-1.0, -1.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    unc.negate();

    region.set_uncertainty(Box::new(unc));

    let got = ast_core::to_native_string(&AnyObject::BoxRegion(region)).unwrap();
    assert_eq!(got, setunc_fixture_map("setunc_unbounded_negated"));
}

// ============================================================================
// get_def_unc (astGetDefUnc) computational parity
//
// C `astGetUnc(this, 1)` returns a copy of the default uncertainty Region in
// the CURRENT Frame with RegionFS cleared (region.c:3784). The oracle `.map`
// fixtures store that Region. These tests reconstruct the parent Region from
// the same primitives the fixture generator uses, call `get_unc_frm(Current)`
// (equivalent to `astGetUnc` with a plain — hence UnitMap — FrameSet), and
// compare the serialization byte-for-byte, pinning the per-class default
// uncertainty rule directly.
// ============================================================================

fn defunc_fixture_map(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/native/reference/simplify_fixtures/{}.map",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// Serialize a default-uncertainty Region to native form. C `astGetUnc`
/// clears RegionFS on the returned copy; the parent Regions here all have a
/// plain (UnitMap) FrameSet, so `get_unc_frm(Current)` returns the base-Frame
/// uncertainty directly.
fn defunc_native(region: &dyn Region) -> String {
    let mut unc = region.get_unc_frm(FrameRef::Current).unwrap();
    // Mirror the public C `astGetUnc` (region.c:3808): clear RegionFS on the
    // returned copy so the uncertainty's Frame is always dumped, even when its
    // FrameSet was internally flagged for omission (as `set_uncertainty` does).
    unc.set_region_fs_flag(true);
    let any = if let Some(r) = unc.as_any().downcast_ref::<BoxRegion>() {
        AnyObject::BoxRegion(r.clone())
    } else if let Some(r) = unc.as_any().downcast_ref::<Circle>() {
        AnyObject::Circle(r.clone())
    } else if let Some(r) = unc.as_any().downcast_ref::<ast_core::regions::Prism>() {
        AnyObject::Prism(r.clone())
    } else if let Some(r) = unc.as_any().downcast_ref::<Interval>() {
        AnyObject::Interval(r.clone())
    } else if let Some(r) = unc.as_any().downcast_ref::<CmpRegion>() {
        AnyObject::CmpRegion(r.clone())
    } else {
        panic!("unexpected default-uncertainty Region class");
    };
    ast_core::to_native_string(&any).unwrap()
}

#[test]
fn get_def_unc_interval_unbounded() {
    // Oracle: defunc_interval_unbounded. A 2-D Interval with a bounded axis 1
    // ([2, 8]) and a half-open axis 2 (no upper limit) is not Box-equivalent,
    // so Interval::get_def_unc (interval.c:583) builds a Box covering 1.0E-6 of
    // its own limits: axis 1 half-width 3.0E-6 about centre 5, axis 2 collapses
    // to [0, 0].
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frm, vec![2.0, 3.0], vec![8.0, BAD_VALUE])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        defunc_native(&interval),
        defunc_fixture_map("defunc_interval_unbounded")
    );
}

#[test]
fn get_def_unc_nullregion_is_zero_circle() {
    // Oracle: defunc_nullregion_2d. NullRegion::get_def_unc (nullregion.c:170)
    // is a zero-radius Circle centred on the origin, not the base-class
    // bounding-box Box.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let null = ast_core::regions::NullRegionBuilder::new(frm)
        .build()
        .unwrap();

    assert_eq!(
        defunc_native(&null),
        defunc_fixture_map("defunc_nullregion_2d")
    );
}

#[test]
fn get_def_unc_prism_combines_components() {
    // Oracle: defunc_prism_box_interval. Prism::get_def_unc (prism.c:1207) is a
    // Prism of the two component default uncertainties (Box's and Interval's).
    let f2 = BasicFrameBuilder::new(2).build().unwrap();
    let boxr = BoxRegionBuilder::from_bounds(f2, &[0.0, 0.0], &[4.0, 6.0])
        .unwrap()
        .build()
        .unwrap();
    let f1 = BasicFrameBuilder::new(1).build().unwrap();
    let interval = IntervalBuilder::new(f1, vec![1.0], vec![5.0])
        .unwrap()
        .build()
        .unwrap();
    let prism = PrismBuilder::new(boxr, interval).unwrap().build().unwrap();

    assert_eq!(
        defunc_native(&prism),
        defunc_fixture_map("defunc_prism_box_interval")
    );
}

#[test]
fn get_def_unc_cmpregion_uses_component_uncertainty() {
    // Oracle: defunc_cmpregion_component_unc. CmpRegion::get_def_unc
    // (cmpregion.c:1090) returns the first component's uncertainty when that
    // component has an explicit (non-default) uncertainty, recentred by
    // set_uncertainty on the component's centre (5, 5).
    let f = BasicFrameBuilder::new(2).build().unwrap();
    let mut reg1 = BoxRegionBuilder::from_bounds(f, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();
    let ufrm = BasicFrameBuilder::new(2).build().unwrap();
    let unc = BoxRegionBuilder::from_bounds(ufrm, &[-0.2, -0.2], &[0.2, 0.2])
        .unwrap()
        .build()
        .unwrap();
    reg1.set_uncertainty(Box::new(unc));

    let f2 = BasicFrameBuilder::new(2).build().unwrap();
    let reg2 = BoxRegionBuilder::from_bounds(f2, &[5.0, 5.0], &[15.0, 15.0])
        .unwrap()
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(reg1, reg2, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        defunc_native(&cmp),
        defunc_fixture_map("defunc_cmpregion_component_unc")
    );
}

#[test]
fn pick_axes_all_axes_permuted_preserves_region_class() {
    // Region-level astPickAxes (region.c::PickAxes) picks current-Frame axes,
    // splits the current->base Mapping, and calls astRegBasePick to rebuild a
    // same-class Region over the corresponding base axes. When all axes are
    // selected in a permuted order, the base Region::RegBasePick copy path
    // (region.c:8342) applies: the result must remain a Region of the original
    // class, not degrade to a plain Frame.
    let frm = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frm, vec![3.0, 5.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();

    // Pick both current-Frame axes in swapped order (0-based indices).
    match pick_axes_region(&circle, &[1, 0]).unwrap() {
        PickedAxes::Region(r) => {
            assert!(
                r.as_any().downcast_ref::<Circle>().is_some(),
                "permuted all-axes pick must stay a Circle, got a different Region class"
            );
            assert_eq!(r.naxes(), 2, "picked Region should be 2-D");
        }
        PickedAxes::Frame(_) => {
            panic!("permuted all-axes pick degraded to a Frame; RegBasePick copy path not taken");
        }
    }
}

#[test]
fn pick_axes_cmpregion_subset_keeps_cmpregion() {
    // CmpRegion::reg_base_pick_axes (cmpregion.c:2096) picks the same base axes
    // from each sub-region and, when both yield Regions, rebuilds a CmpRegion
    // with the same operator. An AND of two 3-D Boxes picked on all three axes
    // (permuted) must therefore stay a CmpRegion.
    let frm = BasicFrameBuilder::new(3).build().unwrap();
    let box_a = BoxRegionBuilder::new(frm.clone(), vec![0.0, 0.0, 0.0], vec![10.0, 10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();
    let box_b = BoxRegionBuilder::new(frm, vec![2.0, 2.0, 2.0], vec![8.0, 8.0, 8.0])
        .unwrap()
        .build()
        .unwrap();
    let cmp = CmpRegionBuilder::new(box_a, box_b, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    match pick_axes_region(&cmp, &[2, 1, 0]).unwrap() {
        PickedAxes::Region(r) => {
            assert!(
                r.as_any().downcast_ref::<CmpRegion>().is_some(),
                "permuted all-axes pick of a CmpRegion must stay a CmpRegion"
            );
            assert_eq!(r.naxes(), 3, "picked CmpRegion should be 3-D");
        }
        PickedAxes::Frame(_) => {
            panic!("CmpRegion pick degraded to a Frame; override not taken");
        }
    }
}

// ============================================================================
// Parent-Region Simplify uncertainty-stability check (region.c:11042-11328).
//
// C `Region::Simplify` runs BEFORE each subclass's own simplify: it deep-copies
// the Region, simplifies the encapsulated FrameSet, deletes a DEFAULT
// uncertainty, and — for an EXPLICIT uncertainty — runs a stability check.
// The check simplifies the uncertainty Region; if the simplified form differs,
// it re-centres the simplified uncertainty at the lower and upper corners of a
// test box (the base bounding box when bounded) and compares the resulting
// widths per axis. When they agree to within 0.5% of their sum on every axis
// the simplified uncertainty is stable and is installed on the result.
//
// The simplify-regression harness cannot exercise this path: its Check 3
// (`mapping_to_native_string`) returns None for Region results, so the
// serialization of a simplified Region — and thus its uncertainty — is never
// compared. These fixtures were generated with `astMapRegion` so the supplied
// uncertainty carries a live base->current FrameSet (PIXEL -> ZOOMED etc); the
// `.simp` reference has that FrameSet stripped and the uncertainty points
// rescaled, which only the parent-Region stability check produces.
// ============================================================================

fn simpstab_fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/native/reference/simplify_fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// Load a `.map` fixture as a Region, simplify it, and serialize the whole
/// simplified Region to native form for byte-exact comparison with `.simp`.
fn simpstab_simplified_native(map_name: &str) -> String {
    let text = simpstab_fixture(map_name);
    let obj =
        ast_core::from_native_string(&text).unwrap_or_else(|e| panic!("parse {map_name}: {e}"));
    let mapping = obj
        .into_mapping()
        .unwrap_or_else(|| panic!("{map_name} is not a Mapping"));
    let simplified = mapping.simplify().expect("simplify failed");
    let r: &dyn Mapping = simplified.as_ref();
    let any = if let Some(b) = r.as_any().downcast_ref::<BoxRegion>() {
        AnyObject::BoxRegion(b.clone())
    } else if let Some(c) = r.as_any().downcast_ref::<Circle>() {
        AnyObject::Circle(c.clone())
    } else if let Some(i) = r.as_any().downcast_ref::<Interval>() {
        AnyObject::Interval(i.clone())
    } else if let Some(p) = r.as_any().downcast_ref::<ast_core::regions::Prism>() {
        AnyObject::Prism(p.clone())
    } else if let Some(cr) = r.as_any().downcast_ref::<CmpRegion>() {
        AnyObject::CmpRegion(cr.clone())
    } else {
        panic!("{map_name} simplified to an unexpected Region class");
    };
    ast_core::to_native_string(&any).unwrap()
}

#[test]
fn simplify_stability_box_linear_unc() {
    // Oracle: simpstab_box_linear_unc. A Box mapped through a ZoomMap (factor 2)
    // carries an uncertainty Box whose base->current FrameSet is that same
    // ZoomMap. Simplifying the uncertainty is a rigid rescale (linear), so the
    // stability widths agree and the rescaled uncertainty is installed on the
    // simplified Box, its FrameSet stripped.
    assert_eq!(
        simpstab_simplified_native("simpstab_box_linear_unc.map"),
        simpstab_fixture("simpstab_box_linear_unc.simp")
    );
}

#[test]
fn simplify_stability_box_nonlinear_unc() {
    // Oracle: simpstab_box_nonlinear_unc. The uncertainty is mapped through a
    // per-axis monotonic MathMap (squaring). astSimplify locally linearises and
    // collapses the uncertainty to a rigid UnitMap Box, so the stability widths
    // still agree and the collapsed uncertainty is installed.
    assert_eq!(
        simpstab_simplified_native("simpstab_box_nonlinear_unc.map"),
        simpstab_fixture("simpstab_box_nonlinear_unc.simp")
    );
}

#[test]
fn simplify_stability_permmap_slice_unc() {
    // Oracle: simpstab_permmap_slice_unc. The Region is mapped through a PermMap
    // that drops an axis (2-D -> 1-D), so nin != nout and the stability check
    // re-centres the uncertainty by transforming its current centre
    // forward-then-back before simplifying.
    assert_eq!(
        simpstab_simplified_native("simpstab_permmap_slice_unc.map"),
        simpstab_fixture("simpstab_permmap_slice_unc.simp")
    );
}

// ============================================================================
// Parallel-merge FillFactor / MeshSize scaling (box.c:2049-2085).
//
// When two Regions merge in a parallel CmpMap to form one higher-dimensional
// Region, the merged FillFactor is the product of the two source FillFactors
// (when either was set) and the merged MeshSize (when either was set) is the
// result's naxes-default scaled by the product of each source's used/default
// ratio. The per-naxes default is 2 (1-D), 200 (2-D), 2000 (>2-D).
// ============================================================================

#[test]
fn parallel_merge_scales_mesh_size() {
    use ast_core::mappings::{CmpMap, Composition};

    // Two 1-D Boxes, each with an explicit MeshSize.
    let f1 = BasicFrameBuilder::new(1).build().unwrap();
    let mut b1 = BoxRegionBuilder::from_bounds(f1, &[0.0], &[10.0])
        .unwrap()
        .build()
        .unwrap();
    b1.set_mesh_size(30);

    let f2 = BasicFrameBuilder::new(1).build().unwrap();
    let mut b2 = BoxRegionBuilder::from_bounds(f2, &[0.0], &[10.0])
        .unwrap()
        .build()
        .unwrap();
    b2.set_mesh_size(50);

    // Parallel merge -> 2-D Box.
    let cm = CmpMap::new(Box::new(b1), Box::new(b2), Composition::Parallel).unwrap();
    let simp = cm.simplified().unwrap();
    let region = simp
        .as_any()
        .downcast_ref::<BoxRegion>()
        .expect("parallel merge of two 1-D Boxes should be a 2-D Box");

    // (30/2) * (50/2) * 200 = 75000.
    assert!(region.test_mesh_size(), "merged MeshSize should be set");
    assert_eq!(region.mesh_size(), 75000);
}

#[test]
fn parallel_merge_scales_fill_factor() {
    use ast_core::mappings::{CmpMap, Composition};

    let f1 = BasicFrameBuilder::new(1).build().unwrap();
    let mut b1 = BoxRegionBuilder::from_bounds(f1, &[0.0], &[10.0])
        .unwrap()
        .build()
        .unwrap();
    b1.set_fill_factor(0.5);

    let f2 = BasicFrameBuilder::new(1).build().unwrap();
    let mut b2 = BoxRegionBuilder::from_bounds(f2, &[0.0], &[10.0])
        .unwrap()
        .build()
        .unwrap();
    b2.set_fill_factor(0.8);

    let cm = CmpMap::new(Box::new(b1), Box::new(b2), Composition::Parallel).unwrap();
    let simp = cm.simplified().unwrap();
    let region = simp
        .as_any()
        .downcast_ref::<BoxRegion>()
        .expect("parallel merge of two 1-D Boxes should be a 2-D Box");

    // 0.5 * 0.8 = 0.4.
    assert!(region.test_fill_factor(), "merged FillFactor should be set");
    assert!((region.fill_factor() - 0.4).abs() < 1e-12);
}

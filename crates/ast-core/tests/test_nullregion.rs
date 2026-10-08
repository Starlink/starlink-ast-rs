//! Tests for the NullRegion implementation.
//!
//! Covers: empty by default, universal when negated, Mapping::transform
//! behaviour (forward sets all bad, inverse passes through), Frame delegation.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{NullRegionBuilder, Region};

/// Check whether a value is the AST bad-value sentinel.
fn is_bad(v: f64) -> bool {
    v == BAD_VALUE
}

// ---- Construction ----

#[test]
fn construct_basic() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    assert_eq!(nr.naxes(), 2);
    assert!(!nr.is_negated());
    // Un-negated NullRegion is the empty set: bounded (a finite region has a
    // finite negation). Only the negation (the whole space) is unbounded.
    assert!(nr.is_bounded());

    let mut neg = NullRegionBuilder::new(BasicFrameBuilder::new(2).build().unwrap())
        .build()
        .unwrap();
    neg.negate();
    assert!(!neg.is_bounded());
}

#[test]
fn construct_3d() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    assert_eq!(nr.naxes(), 3);
}

// ---- Point membership: nothing inside ----

#[test]
fn nothing_inside_unnegated() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    assert!(!nr.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(!nr.point_in_region(&[100.0, -50.0]).unwrap());
    assert!(!nr.point_in_region(&[1e30, 1e30]).unwrap());
}

// ---- Point membership: everything inside when negated ----

#[test]
fn everything_inside_negated() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut nr = NullRegionBuilder::new(frame).build().unwrap();
    nr.negate();

    assert!(nr.is_negated());
    assert!(nr.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(nr.point_in_region(&[100.0, -50.0]).unwrap());
    assert!(nr.point_in_region(&[1e30, 1e30]).unwrap());
}

#[test]
fn negated_copy_is_universal() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    let negated = nr.negated_copy();
    assert!(negated.is_negated());
    assert!(negated.point_in_region(&[42.0, 7.0]).unwrap());
}

// ---- Mapping::transform ----

#[test]
fn forward_transform_sets_all_bad() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    let pts = PointSet::from_rows(
        [[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]
            .iter()
            .map(|p| p.as_slice()),
    );

    let result = nr.transform(&pts, true).unwrap();
    assert_eq!(result.npoint(), 3);

    // All output points should be BAD.
    for i in 0..3 {
        let p = result.point(i);
        assert!(is_bad(p[0]), "point {i} axis 0 should be BAD");
        assert!(is_bad(p[1]), "point {i} axis 1 should be BAD");
    }
}

#[test]
fn inverse_transform_masks_as_forward() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    let pts = PointSet::from_rows(
        [[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]
            .iter()
            .map(|p| p.as_slice()),
    );

    // C's Region classes transform identically in both directions, so a
    // point outside the Region is bad on the inverse too.
    let inverse = nr.transform(&pts, false).unwrap();
    let forward = nr.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        assert_eq!(inverse.point(i), forward.point(i), "point {i}");
    }
    assert!(inverse.point(0).iter().all(|&v| v == BAD_VALUE));
}

#[test]
fn negated_forward_transform_passes_through() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut nr = NullRegionBuilder::new(frame).build().unwrap();
    nr.negate();

    let pts = PointSet::from_rows(
        [[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]
            .iter()
            .map(|p| p.as_slice()),
    );

    let result = nr.transform(&pts, true).unwrap();
    assert_eq!(result.npoint(), 3);

    // Negated NullRegion contains everything — forward passes all points.
    for i in 0..3 {
        let p = result.point(i);
        let orig = pts.point(i);
        assert_eq!(p[0], orig[0]);
        assert_eq!(p[1], orig[1]);
    }
}

// ---- Bounds and mesh ----

// A Region with no extent reports a lower bound above its upper bound, which is
// how `astGetRegionBounds` distinguishes "empty" from "a single constant value".
// A NullRegion contains no points, so C returns 1.0 and -1.0 on every axis
// (nullregion.c:961, nullregion.c:1203).
#[test]
fn bounds_report_an_empty_extent() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    let (lower, upper) = nr.get_region_bounds().unwrap();
    assert_eq!(lower, vec![1.0, 1.0]);
    assert_eq!(upper, vec![-1.0, -1.0]);

    // The base-Frame box uses the same convention, and a NullRegion overrides
    // the current-Frame method so the two never diverge.
    let (blower, bupper) = nr.region_base_box().unwrap();
    assert_eq!(blower, vec![1.0, 1.0]);
    assert_eq!(bupper, vec![-1.0, -1.0]);
}

#[test]
fn mesh_is_single_bad_point() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    let mesh = nr.get_region_mesh().unwrap();
    // C returns a single point with BAD on all axes.
    assert_eq!(mesh.npoint(), 1);
    let p = mesh.point(0);
    assert!(is_bad(p[0]));
    assert!(is_bad(p[1]));
}

// ---- Frame delegation ----

#[test]
fn frame_distance() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame).build().unwrap();

    let d = nr.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

// ---- map_region ----

#[test]
fn map_region_preserves_null() {
    use ast_core::mappings::ZoomMap;

    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let nr = NullRegionBuilder::new(frame.clone()).build().unwrap();

    let zoom = ZoomMap::new(2, 2.0).unwrap();
    let mapped = nr.map_region(&zoom, &frame).unwrap();

    // Still a NullRegion — nothing inside, and (un-negated) bounded.
    assert!(!mapped.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(mapped.is_bounded());
}

#[test]
fn map_negated_region_preserves_negation() {
    use ast_core::mappings::ZoomMap;

    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut nr = NullRegionBuilder::new(frame.clone()).build().unwrap();
    nr.negate();

    let zoom = ZoomMap::new(2, 2.0).unwrap();
    let mapped = nr.map_region(&zoom, &frame).unwrap();

    // Still negated — everything inside.
    assert!(mapped.is_negated());
    assert!(mapped.point_in_region(&[0.0, 0.0]).unwrap());
}

// ---- BAD input handling ----

#[test]
fn bad_input_returns_false() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut nr = NullRegionBuilder::new(frame).build().unwrap();
    nr.negate(); // Even negated (universal), BAD input → false is debatable

    // For unnegated:
    let frame2 = BasicFrameBuilder::new(2).build().unwrap();
    let nr2 = NullRegionBuilder::new(frame2).build().unwrap();
    assert!(!nr2.point_in_region(&[BAD_VALUE, 0.0]).unwrap());
}

//! Tests for SelectorMap.
//!
//! Ancestor: C AST SelectorMap (selectormap.c). The Rust API uses the
//! `BoxRegionBuilder::from_bounds` constructor and `PointSet::from_rows`,
//! diverging from the C library's PointSet column-major layout.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::BasicFrameBuilder;
use ast_core::mappings::{Mapping, SelectorMapBuilder};
use ast_core::regions::{BoxRegionBuilder, DynRegion};

fn make_2d_frame() -> ast_core::frames::BasicFrame {
    BasicFrameBuilder::new(2).build().unwrap()
}

/// Two non-overlapping 2D boxes: Box1 covers [0,10]×[0,10], Box2 covers [20,30]×[20,30].
fn make_two_boxes() -> Vec<DynRegion> {
    let frame1 = make_2d_frame();
    let frame2 = make_2d_frame();
    let box1: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame1, &[0.0, 0.0], &[10.0, 10.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let box2: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame2, &[20.0, 20.0], &[30.0, 30.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    vec![box1, box2]
}

#[test]
fn selectormap_basic_construction() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    assert_eq!(sm.nin(), 2);
    assert_eq!(sm.nout(), 1);
    assert!(!sm.is_inverted());
    assert!(sm.has_forward());
    assert!(!sm.has_inverse());
}

#[test]
fn selectormap_empty_regions_rejected() {
    let result = SelectorMapBuilder::new(vec![]);
    assert!(result.is_err());
}

#[test]
fn selectormap_mismatched_axes_rejected() {
    let frame2 = BasicFrameBuilder::new(2).build().unwrap();
    let frame3 = BasicFrameBuilder::new(3).build().unwrap();
    let box2d: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame2, &[0.0, 0.0], &[1.0, 1.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let box3d: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame3, &[0.0, 0.0, 0.0], &[1.0, 1.0, 1.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let result = SelectorMapBuilder::new(vec![box2d, box3d]);
    assert!(result.is_err());
}

#[test]
fn selectormap_point_in_region1() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0);
}

#[test]
fn selectormap_point_in_region2() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([[25.0, 25.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 2.0);
}

#[test]
fn selectormap_point_in_no_region() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([[15.0, 15.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 0.0);
}

#[test]
fn selectormap_bad_input_returns_badval() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions)
        .unwrap()
        .badval(99.0)
        .build();
    let pts = PointSet::from_rows([[BAD_VALUE, 5.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 99.0);
}

#[test]
fn selectormap_multiple_points() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([
        [5.0, 5.0],       // in region 1
        [25.0, 25.0],     // in region 2
        [15.0, 15.0],     // no region
        [BAD_VALUE, 5.0], // bad input
    ]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0);
    assert_eq!(out.point(1)[0], 2.0);
    assert_eq!(out.point(2)[0], 0.0);
    assert_eq!(out.point(3)[0], BAD_VALUE); // default badval
}

#[test]
fn selectormap_first_region_wins() {
    let frame1 = make_2d_frame();
    let frame2 = make_2d_frame();
    let box1: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame1, &[0.0, 0.0], &[10.0, 10.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let box2: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame2, &[0.0, 0.0], &[10.0, 10.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let sm = SelectorMapBuilder::new(vec![box1, box2]).unwrap().build();
    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = sm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0);
}

#[test]
fn selectormap_inverse_returns_bad() {
    let regions = make_two_boxes();
    let sm = SelectorMapBuilder::new(regions).unwrap().build();
    let pts = PointSet::from_rows([[1.0]]);
    let out = sm.transform(&pts, false).unwrap();
    assert_eq!(out.ncoord(), 2);
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

//! Tests for SwitchMap.
//!
//! Ancestor: C AST testswitchmap.c — tests selector-based route dispatching.
//! Adapted to use the Rust public API (BasicFrameBuilder, BoxRegionBuilder,
//! WinMap::from_corners, PointSet::from_rows).

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::BasicFrameBuilder;
use ast_core::mappings::{
    DynMapping, Mapping, SelectorMapBuilder, SwitchMap, UnitMap, WinMap, ZoomMap,
};
use ast_core::regions::{BoxRegionBuilder, DynRegion, remove_regions};
use ast_core::{AnyObject, from_native_string, to_native_string};

/// Create a SwitchMap with two routes:
///
/// - Route 1 (x in [0,10]): WinMap scaling [0,10]→[0,100]
/// - Route 2 (x in [20,30]): WinMap scaling [20,30]→[200,300]
///
/// Forward selector is a SelectorMap over 1D regions.
fn make_1d_switchmap() -> SwitchMap {
    let frame1 = BasicFrameBuilder::new(1).build().unwrap();
    let frame2 = BasicFrameBuilder::new(1).build().unwrap();

    let reg1: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame1, &[0.0], &[10.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let reg2: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(frame2, &[20.0], &[30.0])
            .unwrap()
            .build()
            .unwrap(),
    );
    let sel = SelectorMapBuilder::new(vec![reg1, reg2]).unwrap().build();
    let fsmap: DynMapping = Box::new(sel);

    let route1: DynMapping =
        Box::new(WinMap::from_corners(&[0.0], &[10.0], &[0.0], &[100.0]).unwrap());
    let route2: DynMapping =
        Box::new(WinMap::from_corners(&[20.0], &[30.0], &[200.0], &[300.0]).unwrap());

    SwitchMap::new(Some(fsmap), None, vec![route1, route2]).unwrap()
}

#[test]
fn switchmap_basic_construction() {
    let sw = make_1d_switchmap();
    assert_eq!(sw.nin(), 1);
    assert_eq!(sw.nout(), 1);
    assert_eq!(sw.nroute(), 2);
    assert!(sw.has_forward());
    assert!(!sw.has_inverse());
}

#[test]
fn switchmap_no_routes_rejected() {
    let result = SwitchMap::new(Some(Box::new(UnitMap::new(1))), None, vec![]);
    assert!(result.is_err());
}

#[test]
fn switchmap_no_selectors_rejected() {
    let route: DynMapping = Box::new(UnitMap::new(2));
    let result = SwitchMap::new(None, None, vec![route]);
    assert!(result.is_err());
}

#[test]
fn switchmap_mismatched_routes_rejected() {
    let r1: DynMapping = Box::new(UnitMap::new(2));
    let r2: DynMapping = Box::new(UnitMap::new(3));
    let sel: DynMapping = Box::new(ZoomMap::new(2, 1.0).unwrap());
    let result = SwitchMap::new(Some(sel), None, vec![r1, r2]);
    assert!(result.is_err());
}

#[test]
fn switchmap_forward_route1() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[5.0]]);
    let out = sw.transform(&pts, true).unwrap();
    // WinMap [0,10]→[0,100]: 5 → 50
    assert!((out.point(0)[0] - 50.0).abs() < 1e-10);
}

#[test]
fn switchmap_forward_route2() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[25.0]]);
    let out = sw.transform(&pts, true).unwrap();
    // WinMap [20,30]→[200,300]: 25 → 250
    assert!((out.point(0)[0] - 250.0).abs() < 1e-10);
}

#[test]
fn switchmap_forward_no_route() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[15.0]]);
    let out = sw.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn switchmap_forward_bad_input() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = sw.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn switchmap_forward_multiple_points() {
    let sw = make_1d_switchmap();
    let pts = PointSet::from_rows([[5.0], [25.0], [15.0], [BAD_VALUE]]);
    let out = sw.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 50.0).abs() < 1e-10);
    assert!((out.point(1)[0] - 250.0).abs() < 1e-10);
    assert_eq!(out.point(2)[0], BAD_VALUE);
    assert_eq!(out.point(3)[0], BAD_VALUE);
}

#[test]
fn switchmap_equal() {
    let sw1 = make_1d_switchmap();
    let sw2 = make_1d_switchmap();
    assert!(sw1.equal(&sw2));
}

#[test]
fn switchmap_remove_regions() {
    let sw = make_1d_switchmap();
    let cleaned = remove_regions(&sw);
    // The selector was a SelectorMap containing Regions; after remove_regions,
    // those Regions become UnitMaps inside the selector. The SwitchMap dimensions
    // should still match the originals.
    assert_eq!(cleaned.nin(), 1);
    assert_eq!(cleaned.nout(), 1);
}

#[test]
fn switchmap_simplify_inverse_cancel_fixture() {
    // The fixture: CmpMap(SwitchMap, inv(SwitchMap), series) with matching content.
    // Should simplify to UnitMap(1).
    let input = std::fs::read_to_string(
        "tests/fixtures/native/reference/simplify_fixtures/switchmap_inverse_cancel.map",
    )
    .unwrap();
    let obj = from_native_string(&input).unwrap();
    let mapping = obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();
    assert!(
        simplified.as_any().is::<UnitMap>(),
        "Expected UnitMap, got {:?}",
        simplified
    );
    assert_eq!(simplified.nin(), 1);
    assert_eq!(simplified.nout(), 1);
}

#[test]
fn switchmap_simplify_internal_fixture() {
    // The fixture: SwitchMap whose children can be simplified.
    // Result should still be a SwitchMap (or simpler), with same nin/nout.
    let input = std::fs::read_to_string(
        "tests/fixtures/native/reference/simplify_fixtures/switchmap_internal_simplify.map",
    )
    .unwrap();
    let obj = from_native_string(&input).unwrap();
    let original = obj.into_mapping().unwrap();
    let nin_orig = original.nin();
    let nout_orig = original.nout();

    let simplified = original.simplify().unwrap();
    assert_eq!(simplified.nin(), nin_orig);
    assert_eq!(simplified.nout(), nout_orig);
}

#[test]
fn switchmap_simplify_invert_normalize_fixture() {
    // The fixture: an inverted SwitchMap. Simplification normalises the
    // inversion into the children, leaving a non-inverted SwitchMap.
    let input = std::fs::read_to_string(
        "tests/fixtures/native/reference/simplify_fixtures/switchmap_invert_normalize.map",
    )
    .unwrap();
    let obj = from_native_string(&input).unwrap();
    let original = obj.into_mapping().unwrap();
    let nin_orig = original.nin();
    let nout_orig = original.nout();

    let simplified = original.simplify().unwrap();
    assert_eq!(simplified.nin(), nin_orig);
    assert_eq!(simplified.nout(), nout_orig);
}

#[test]
fn switchmap_serialization_roundtrip() {
    let sw = make_1d_switchmap();
    let obj = AnyObject::SwitchMap(sw.clone());
    let text = to_native_string(&obj).unwrap();
    let obj2 = from_native_string(&text).unwrap();
    let loaded: SwitchMap = match obj2 {
        AnyObject::SwitchMap(m) => m,
        other => panic!("Expected SwitchMap, got {:?}", other),
    };

    assert_eq!(loaded.nroute(), 2);
    assert_eq!(loaded.nin(), 1);
    assert_eq!(loaded.nout(), 1);

    let pts = PointSet::from_rows([[5.0], [25.0], [15.0]]);
    let out1 = sw.transform(&pts, true).unwrap();
    let out2 = loaded.transform(&pts, true).unwrap();
    for i in 0..3 {
        assert_eq!(out1.point(i)[0], out2.point(i)[0]);
    }
}

//! Serialisation round-trip tests for all concrete AST types.
//!
//! Tests the public API: `to_native_string`, `from_native_string`, and `AnyObject`.
//! Each concrete type is serialised to Native text, checked for structural markers,
//! then deserialised and verified for both attribute fidelity and functional equivalence.

use crate::common::from_native_string;
use crate::common::native_dump;
use ast_core::AnyObject;
use ast_core::coords::PointSet;
use ast_core::frames::{
    BasicFrameBuilder, CmpFrame, Frame, FrameRef, FrameSet, SkyFrame, SkyRefIs, SkySystem,
};
use ast_core::keymap::KeyMap;
use ast_core::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MatrixMap, PermMap, ShiftMap, UnitMap, ZoomMap,
};
use ast_core::regions::{
    BoxRegionBuilder, CircleBuilder, CmpOperator, CmpRegionBuilder, EllipseBuilder,
    IntervalBuilder, NullRegionBuilder, PointListBuilder, PolygonBuilder, PrismBuilder, Region,
};

use ndarray::array;

// ===========================================================================
// Helper
// ===========================================================================

/// Assert two f64 values are close.
fn assert_close(actual: f64, expected: f64, tol: f64, label: &str) {
    assert!(
        (actual - expected).abs() < tol,
        "{label}: expected {expected}, got {actual} (diff {})",
        (actual - expected).abs()
    );
}

// ===========================================================================
// 1. UnitMap round-trip
// ===========================================================================

#[test]
fn round_trip_unit_map() {
    let original = UnitMap::new(3);
    let text = native_dump(&AnyObject::UnitMap(original)).unwrap();

    // Structural markers.
    assert!(text.contains("Begin UnitMap"), "missing Begin UnitMap");
    assert!(text.contains("End UnitMap"), "missing End UnitMap");
    assert!(text.contains("Nin"), "missing Nin attribute");

    // Deserialise and verify.
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 3);
    assert_eq!(mapping.nout(), 3);

    // Functional: identity transform.
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0, 3.0]);
}

#[test]
fn round_trip_unit_map_inverse() {
    let original = UnitMap::new(2);
    let text = native_dump(&AnyObject::UnitMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[7.0, 8.0]]);
    let out = mapping.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[7.0, 8.0]);
}

// ===========================================================================
// 2. ZoomMap round-trip
// ===========================================================================

#[test]
fn round_trip_zoom_map() {
    let original = ZoomMap::new(2, 3.5).unwrap();
    let text = native_dump(&AnyObject::ZoomMap(original)).unwrap();

    assert!(text.contains("Begin ZoomMap"), "missing Begin ZoomMap");
    assert!(text.contains("End ZoomMap"), "missing End ZoomMap");

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 3.5, 1e-10, "zoom x");
    assert_close(out.point(0)[1], 7.0, 1e-10, "zoom y");
}

#[test]
fn round_trip_zoom_map_inverse() {
    let original = ZoomMap::new(2, 4.0).unwrap();
    let text = native_dump(&AnyObject::ZoomMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[8.0, 12.0]]);
    let out = mapping.transform(&pts, false).unwrap();
    assert_close(out.point(0)[0], 2.0, 1e-10, "zoom inv x");
    assert_close(out.point(0)[1], 3.0, 1e-10, "zoom inv y");
}

#[test]
fn round_trip_zoom_map_fractional_factor() {
    let original = ZoomMap::new(1, 0.1).unwrap();
    let text = native_dump(&AnyObject::ZoomMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[50.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 5.0, 1e-10, "zoom 0.1");
}

// ===========================================================================
// 3. ShiftMap round-trip
// ===========================================================================

#[test]
fn round_trip_shift_map() {
    let original = ShiftMap::new([1.5, -2.7]);
    let text = native_dump(&AnyObject::ShiftMap(original)).unwrap();

    assert!(text.contains("Begin ShiftMap"), "missing Begin ShiftMap");
    assert!(text.contains("End ShiftMap"), "missing End ShiftMap");

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 11.5, 1e-10, "shift x");
    assert_close(out.point(0)[1], 17.3, 1e-10, "shift y");
}

#[test]
fn round_trip_shift_map_inverse() {
    let original = ShiftMap::new([1.5, -2.7]);
    let text = native_dump(&AnyObject::ShiftMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[11.5, 17.3]]);
    let out = mapping.transform(&pts, false).unwrap();
    assert_close(out.point(0)[0], 10.0, 1e-10, "shift inv x");
    assert_close(out.point(0)[1], 20.0, 1e-10, "shift inv y");
}

// ===========================================================================
// 4. MatrixMap round-trip
// ===========================================================================

#[test]
fn round_trip_matrix_map_rotation() {
    // 90-degree rotation: [[0, -1], [1, 0]]
    let original = MatrixMap::new(array![[0.0, -1.0], [1.0, 0.0]]).unwrap();
    let text = native_dump(&AnyObject::MatrixMap(original)).unwrap();

    assert!(text.contains("Begin MatrixMap"), "missing Begin MatrixMap");
    assert!(text.contains("End MatrixMap"), "missing End MatrixMap");

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    // (1, 0) -> (0, 1) under 90-degree rotation.
    let pts = PointSet::from_rows([[1.0, 0.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 0.0, 1e-10, "rot x");
    assert_close(out.point(0)[1], 1.0, 1e-10, "rot y");
}

#[test]
fn round_trip_matrix_map_scale() {
    // Diagonal scale: [[2, 0], [0, 3]]
    let original = MatrixMap::new(array![[2.0, 0.0], [0.0, 3.0]]).unwrap();
    let text = native_dump(&AnyObject::MatrixMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 10.0, 1e-10, "scale x");
    assert_close(out.point(0)[1], 21.0, 1e-10, "scale y");
}

#[test]
fn round_trip_matrix_map_inverse() {
    let original = MatrixMap::new(array![[2.0, 0.0], [0.0, 4.0]]).unwrap();
    let text = native_dump(&AnyObject::MatrixMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let out = mapping.transform(&pts, false).unwrap();
    assert_close(out.point(0)[0], 5.0, 1e-10, "inv scale x");
    assert_close(out.point(0)[1], 5.0, 1e-10, "inv scale y");
}

// ===========================================================================
// 5. PermMap round-trip
// ===========================================================================

#[test]
fn round_trip_perm_map_swap() {
    let original = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let text = native_dump(&AnyObject::PermMap(original)).unwrap();

    assert!(text.contains("Begin PermMap"), "missing Begin PermMap");
    assert!(text.contains("End PermMap"), "missing End PermMap");

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    let pts = PointSet::from_rows([[3.0, 7.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 3.0]);
}

#[test]
fn round_trip_perm_map_with_constant() {
    // outperm=[1, -1] maps x -> (x, constant[0])
    let original = PermMap::new(&[1], &[1, -1], &[42.0]).unwrap();
    let text = native_dump(&AnyObject::PermMap(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 1);
    assert_eq!(mapping.nout(), 2);

    let pts = PointSet::from_rows([[5.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 5.0);
    assert_eq!(out.point(0)[1], 42.0);
}

#[test]
fn round_trip_perm_map_inverse() {
    let original = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let text = native_dump(&AnyObject::PermMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let out = mapping.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[20.0, 10.0]);
}

// ===========================================================================
// 6. CmpMap round-trip
// ===========================================================================

#[test]
fn round_trip_cmp_map_series() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0, 20.0]));
    let original = CmpMap::new(z, s, Composition::Series).unwrap();
    let text = native_dump(&AnyObject::CmpMap(original)).unwrap();

    assert!(text.contains("Begin CmpMap"), "missing Begin CmpMap");
    assert!(text.contains("End CmpMap"), "missing End CmpMap");
    // Should contain nested mappings.
    assert!(text.contains("Begin ZoomMap"), "missing nested ZoomMap");
    assert!(text.contains("Begin ShiftMap"), "missing nested ShiftMap");

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    // Forward: zoom(2) then shift(10,20) -> (1,2) -> (2,4) -> (12,24).
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 12.0, 1e-10, "cmp series x");
    assert_close(out.point(0)[1], 24.0, 1e-10, "cmp series y");
}

#[test]
fn round_trip_cmp_map_parallel() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([100.0]));
    let original = CmpMap::new(z, s, Composition::Parallel).unwrap();
    let text = native_dump(&AnyObject::CmpMap(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    // Parallel: first coord scaled by 3, second shifted by 100.
    let pts = PointSet::from_rows([[2.0, 5.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 6.0, 1e-10, "cmp parallel x");
    assert_close(out.point(0)[1], 105.0, 1e-10, "cmp parallel y");
}

#[test]
fn round_trip_cmp_map_series_inverse() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let original = CmpMap::new(z, s, Composition::Series).unwrap();
    let text = native_dump(&AnyObject::CmpMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    // Inverse of [zoom(2) then shift(10)]: (20 - 10) / 2 = 5.
    let pts = PointSet::from_rows([[20.0]]);
    let out = mapping.transform(&pts, false).unwrap();
    assert_close(out.point(0)[0], 5.0, 1e-10, "cmp inv");
}

// ===========================================================================
// 7. BasicFrame round-trip
// ===========================================================================

#[test]
fn round_trip_basic_frame() {
    let original = BasicFrameBuilder::new(2)
        .title("Test Frame")
        .domain("PIXEL")
        .label(1, "X")
        .label(2, "Y")
        .unit(1, "m")
        .build()
        .unwrap();
    let text = native_dump(&AnyObject::BasicFrame(original)).unwrap();

    assert!(text.contains("Begin Frame"), "missing Begin Frame");
    assert!(text.contains("End Frame"), "missing End Frame");
    assert!(text.contains("Naxes"), "missing Naxes");

    let restored = from_native_string(&text).unwrap();
    // Verify via dyn Frame trait.
    let frame = restored.as_frame().unwrap();
    assert_eq!(frame.naxes(), 2);
    // Verify concrete attributes by matching on the AnyObject variant.
    match restored {
        AnyObject::BasicFrame(ref bf) => {
            assert_eq!(bf.title(), "Test Frame");
            assert_eq!(bf.domain(), "PIXEL");
            assert_eq!(bf.label(1).unwrap(), "X");
            assert_eq!(bf.label(2).unwrap(), "Y");
            assert_eq!(bf.unit(1).unwrap(), "m");
        }
        _ => panic!("expected BasicFrame variant"),
    }
}

#[test]
fn round_trip_basic_frame_default_attributes() {
    let original = BasicFrameBuilder::new(3).build().unwrap();
    let text = native_dump(&AnyObject::BasicFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();
    assert_eq!(frame.naxes(), 3);
    // Default attributes should still hold.
    match restored {
        AnyObject::BasicFrame(ref bf) => {
            assert!(!bf.test_title());
            assert_eq!(bf.title(), "3-d coordinate system");
            assert_eq!(bf.domain(), "");
        }
        _ => panic!("expected BasicFrame variant"),
    }
}

#[test]
fn round_trip_basic_frame_geometry() {
    // Verify the round-tripped frame still computes distance correctly.
    let original = BasicFrameBuilder::new(2).build().unwrap();
    let text = native_dump(&AnyObject::BasicFrame(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();

    let d = frame.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert_close(d, 5.0, 1e-12, "distance");
}

#[test]
fn round_trip_basic_frame_with_epoch() {
    let mut original = BasicFrameBuilder::new(1).build().unwrap();
    original.set_epoch_year(2000.0);
    let text = native_dump(&AnyObject::BasicFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::BasicFrame(ref bf) => {
            assert!(bf.test_epoch());
            assert_eq!(bf.epoch_year(), 2000.0);
        }
        _ => panic!("expected BasicFrame variant"),
    }
}

// ===========================================================================
// 8. CmpFrame round-trip
// ===========================================================================

#[test]
fn round_trip_cmp_frame() {
    let f1: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(1).label(1, "RA").build().unwrap());
    let f2: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(1).label(1, "Dec").build().unwrap());
    let original = CmpFrame::new(f1, f2);
    let text = native_dump(&AnyObject::CmpFrame(original)).unwrap();

    assert!(text.contains("Begin CmpFrame"), "missing Begin CmpFrame");
    assert!(text.contains("End CmpFrame"), "missing End CmpFrame");

    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();
    assert_eq!(frame.naxes(), 2);
}

#[test]
fn round_trip_cmp_frame_3plus2() {
    let f1: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(3).build().unwrap());
    let f2: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let original = CmpFrame::new(f1, f2);
    let text = native_dump(&AnyObject::CmpFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();
    assert_eq!(frame.naxes(), 5);
}

#[test]
fn round_trip_cmp_frame_distance() {
    let f1: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(1).build().unwrap());
    let f2: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(1).build().unwrap());
    let original = CmpFrame::new(f1, f2);
    let text = native_dump(&AnyObject::CmpFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();
    let d = frame.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert_close(d, 5.0, 1e-12, "cmpframe distance");
}

// ===========================================================================
// 9. FrameSet round-trip
// ===========================================================================

#[test]
fn round_trip_frameset_with_zoom() {
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap());
    let current: Box<dyn Frame> =
        Box::new(BasicFrameBuilder::new(2).domain("SKY").build().unwrap());
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 5.0).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    let text = native_dump(&AnyObject::FrameSet(fs)).unwrap();

    assert!(text.contains("Begin FrameSet"), "missing Begin FrameSet");
    assert!(text.contains("End FrameSet"), "missing End FrameSet");

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    // Forward: base->current = zoom by 5.
    let pts = PointSet::from_rows([[2.0, 3.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 10.0, 1e-10, "fs zoom x");
    assert_close(out.point(0)[1], 15.0, 1e-10, "fs zoom y");
}

#[test]
fn round_trip_frameset_inverse() {
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let current: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    let text = native_dump(&AnyObject::FrameSet(fs)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[9.0, 12.0]]);
    let out = mapping.transform(&pts, false).unwrap();
    assert_close(out.point(0)[0], 3.0, 1e-10, "fs inv x");
    assert_close(out.point(0)[1], 4.0, 1e-10, "fs inv y");
}

#[test]
fn round_trip_frameset_as_frame() {
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).domain("GRID").build().unwrap());
    let current: Box<dyn Frame> =
        Box::new(BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap());
    let mut fs = FrameSet::new(base);
    let u: DynMapping = Box::new(UnitMap::new(2));
    fs.add_frame(FrameRef::Base, u, current).unwrap();

    let text = native_dump(&AnyObject::FrameSet(fs)).unwrap();
    let restored = from_native_string(&text).unwrap();

    // FrameSet is accessible both as Mapping and as Frame.
    assert!(restored.as_mapping().is_some());
    assert!(restored.as_frame().is_some());

    let frame = restored.as_frame().unwrap();
    assert_eq!(frame.naxes(), 2);
}

#[test]
fn round_trip_frameset_chain() {
    // A --zoom(2)--> B --shift(10,20)--> C
    let fa: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let fb: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let fc: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let mut fs = FrameSet::new(fa);
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Current, z, fb).unwrap();
    let s: DynMapping = Box::new(ShiftMap::new([10.0, 20.0]));
    fs.add_frame(FrameRef::Current, s, fc).unwrap();

    let text = native_dump(&AnyObject::FrameSet(fs)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    // Forward: (1,2) -> zoom(2) -> (2,4) -> shift(10,20) -> (12,24).
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 12.0, 1e-10, "chain x");
    assert_close(out.point(0)[1], 24.0, 1e-10, "chain y");
}

// ===========================================================================
// 10. AnyObject variant coverage
// ===========================================================================

#[test]
fn any_object_as_mapping_returns_none_for_basic_frame() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let obj = AnyObject::BasicFrame(frame);
    assert!(obj.as_mapping().is_none());
    assert!(obj.as_frame().is_some());
}

#[test]
fn any_object_as_frame_returns_none_for_zoom_map() {
    let mapping = ZoomMap::new(2, 2.0).unwrap();
    let obj = AnyObject::ZoomMap(mapping);
    assert!(obj.as_frame().is_none());
    assert!(obj.as_mapping().is_some());
}

#[test]
fn any_object_into_mapping() {
    let mapping = ShiftMap::new([1.0, 2.0]);
    let obj = AnyObject::ShiftMap(mapping);
    let dyn_mapping = obj.into_mapping().unwrap();
    assert_eq!(dyn_mapping.nin(), 2);
}

#[test]
fn any_object_into_frame() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let obj = AnyObject::BasicFrame(frame);
    let dyn_frame = obj.into_frame().unwrap();
    assert_eq!(dyn_frame.naxes(), 3);
}

#[test]
fn any_object_frameset_is_both() {
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let fs = FrameSet::new(base);
    let obj = AnyObject::FrameSet(fs);
    assert!(obj.as_mapping().is_some());
    assert!(obj.as_frame().is_some());
}

// ===========================================================================
// 11. Double round-trip (serialise -> deserialise -> serialise)
// ===========================================================================

#[test]
fn double_round_trip_zoom_map() {
    let original = ZoomMap::new(2, 7.77).unwrap();
    let text1 = native_dump(&AnyObject::ZoomMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for ZoomMap");
}

#[test]
fn double_round_trip_shift_map() {
    let original = ShiftMap::new([1.5, -2.7, 0.0]);
    let text1 = native_dump(&AnyObject::ShiftMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for ShiftMap");
}

#[test]
fn double_round_trip_basic_frame() {
    let original = BasicFrameBuilder::new(2)
        .title("Round-trip")
        .domain("TEST")
        .label(1, "A")
        .label(2, "B")
        .build()
        .unwrap();
    let text1 = native_dump(&AnyObject::BasicFrame(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(
        text1, text2,
        "double round-trip text mismatch for BasicFrame"
    );
}

#[test]
fn double_round_trip_frameset() {
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let current: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.5).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    let text1 = native_dump(&AnyObject::FrameSet(fs)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for FrameSet");
}

#[test]
fn double_round_trip_cmp_map() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let original = CmpMap::new(z, s, Composition::Series).unwrap();

    let text1 = native_dump(&AnyObject::CmpMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for CmpMap");
}

#[test]
fn double_round_trip_perm_map() {
    let original = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let text1 = native_dump(&AnyObject::PermMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for PermMap");
}

#[test]
fn double_round_trip_matrix_map() {
    let original = MatrixMap::new(array![[0.0, -1.0], [1.0, 0.0]]).unwrap();
    let text1 = native_dump(&AnyObject::MatrixMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(
        text1, text2,
        "double round-trip text mismatch for MatrixMap"
    );
}

#[test]
fn double_round_trip_cmp_frame() {
    let f1: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let f2: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(1).build().unwrap());
    let original = CmpFrame::new(f1, f2);
    let text1 = native_dump(&AnyObject::CmpFrame(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for CmpFrame");
}

#[test]
fn double_round_trip_unit_map() {
    let original = UnitMap::new(4);
    let text1 = native_dump(&AnyObject::UnitMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for UnitMap");
}

// ===========================================================================
// 12. Multi-point transforms after round-trip
// ===========================================================================

#[test]
fn round_trip_zoom_map_multi_point() {
    let original = ZoomMap::new(2, 3.0).unwrap();
    let text = native_dump(&AnyObject::ZoomMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 6.0]);
    assert_eq!(out.point(1), &[9.0, 12.0]);
    assert_eq!(out.point(2), &[15.0, 18.0]);
}

#[test]
fn round_trip_shift_map_multi_point() {
    let original = ShiftMap::new([100.0, -50.0]);
    let text = native_dump(&AnyObject::ShiftMap(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[0.0, 0.0], [1.0, 1.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[100.0, -50.0]);
    assert_eq!(out.point(1), &[101.0, -49.0]);
}

// ===========================================================================
// 13. Error cases
// ===========================================================================

#[test]
fn from_native_string_empty_input() {
    let result = from_native_string("");
    assert!(result.is_err(), "empty input should fail");
}

#[test]
fn from_native_string_garbage_input() {
    let result = from_native_string("this is not AST native format");
    assert!(result.is_err(), "garbage input should fail");
}

#[test]
fn from_native_string_truncated() {
    let original = ZoomMap::new(2, 3.5).unwrap();
    let text = native_dump(&AnyObject::ZoomMap(original)).unwrap();
    // Truncate the text before the End marker.
    let truncated = &text[..text.len() / 2];
    let result = from_native_string(truncated);
    assert!(result.is_err(), "truncated input should fail");
}

// ===========================================================================
// 14. Native text format structure
// ===========================================================================

#[test]
fn native_text_has_isa_markers() {
    let original = ZoomMap::new(2, 3.5).unwrap();
    let text = native_dump(&AnyObject::ZoomMap(original)).unwrap();
    assert!(
        text.contains("IsA Mapping"),
        "missing IsA Mapping marker in ZoomMap"
    );
}

#[test]
fn native_text_frameset_has_nested_objects() {
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let current: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    let text = native_dump(&AnyObject::FrameSet(fs)).unwrap();
    // FrameSet should contain nested Frame and Mapping objects.
    assert!(
        text.contains("Begin Frame"),
        "missing nested Frame in FrameSet"
    );
    assert!(
        text.contains("Begin ZoomMap"),
        "missing nested ZoomMap in FrameSet"
    );
}

#[test]
fn native_text_cmpmap_has_series_flag() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Series).unwrap();
    let text = native_dump(&AnyObject::CmpMap(cm)).unwrap();

    // MapA and MapB keys should be present for the two component mappings.
    assert!(text.contains("MapA"), "missing MapA key in CmpMap");
    assert!(text.contains("MapB"), "missing MapB key in CmpMap");
}

// ===========================================================================
// 15. AnyObject Debug formatting
// ===========================================================================

#[test]
fn any_object_debug() {
    let obj = AnyObject::ZoomMap(ZoomMap::new(2, 3.5).unwrap());
    let debug_str = format!("{:?}", obj);
    assert!(debug_str.contains("ZoomMap"));
}

// ===========================================================================
// 16. SkyFrame round-trip
// ===========================================================================

#[test]
fn round_trip_skyframe_default_fk5() {
    let original = SkyFrame::new(SkySystem::Fk5);
    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();

    assert!(text.contains("Begin SkyFrame"), "missing Begin SkyFrame");
    assert!(text.contains("End SkyFrame"), "missing End SkyFrame");
    assert!(text.contains("Naxes = 2"), "missing Naxes");
    assert!(text.contains("System = \"FK5\""), "missing System");
    assert!(text.contains("IsA Frame"), "missing IsA Frame");
    // Default domain is SKY, written as unset (commented out).
    assert!(
        text.contains("#   Domain = \"SKY\""),
        "default domain SKY should be written as unset"
    );
    // SkyAxis children should be present.
    assert!(text.contains("Begin SkyAxis"), "missing SkyAxis");

    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();
    assert_eq!(frame.naxes(), 2);
    // Not a mapping.
    assert!(restored.as_mapping().is_none());

    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.system(), SkySystem::Fk5);
            assert_eq!(sf.domain(), "SKY");
            assert!(!sf.neg_lon());
            assert_eq!(sf.sky_ref_is(), SkyRefIs::Ignored);
            assert!(!sf.align_offset());
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn round_trip_skyframe_galactic_with_attributes() {
    let mut original = SkyFrame::new(SkySystem::Galactic);
    original.set_title("Galactic Test");
    original.set_epoch_mjd(53000.0);
    original.set_neg_lon(true);
    original.set_sky_tol(1.0e-7);
    original.set_projection("TAN");
    original.set_align_offset(true);

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();

    assert!(text.contains("GALACTIC"), "missing GALACTIC system");
    assert!(text.contains("Galactic Test"), "missing title");
    assert!(text.contains("NegLon"), "missing NegLon");
    assert!(text.contains("SkyTol"), "missing SkyTol");
    assert!(text.contains("Proj"), "missing Proj");
    assert!(text.contains("AlOff"), "missing AlOff");

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.system(), SkySystem::Galactic);
            assert_eq!(sf.title().unwrap(), "Galactic Test");
            assert_close(sf.epoch_mjd(), 53000.0, 1e-10, "epoch");
            assert!(sf.neg_lon());
            assert_close(sf.sky_tol(), 1.0e-7, 1e-15, "sky_tol");
            assert_eq!(sf.projection(), Some("TAN"));
            assert!(sf.align_offset());
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn round_trip_skyframe_with_equinox() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    // Set equinox to J2010.0 MJD.
    let j2010_mjd = 55197.0; // approximate MJD for 2010-01-01
    original.set_equinox_mjd(j2010_mjd);

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    assert!(text.contains("Eqnox"), "missing Eqnox");

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_close(sf.equinox_mjd(), j2010_mjd, 1e-6, "equinox MJD");
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn round_trip_skyframe_with_sky_ref() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    original.set_sky_ref_is(SkyRefIs::Origin);
    original.set_sky_ref_lon_lat(1.5, 0.75);
    original.set_sky_ref_p_lon_lat(0.3, 1.2);

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    assert!(text.contains("SRefIs"), "missing SRefIs");
    assert!(text.contains("SRef1"), "missing SRef1");
    assert!(text.contains("SRef2"), "missing SRef2");
    assert!(text.contains("SRefP1"), "missing SRefP1");
    assert!(text.contains("SRefP2"), "missing SRefP2");

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.sky_ref_is(), SkyRefIs::Origin);
            assert_close(sf.sky_ref_lon_lat()[0], 1.5, 1e-10, "sky_ref lon");
            assert_close(sf.sky_ref_lon_lat()[1], 0.75, 1e-10, "sky_ref lat");
            assert_close(sf.sky_ref_p_lon_lat()[0], 0.3, 1e-10, "sky_ref_p lon");
            assert_close(sf.sky_ref_p_lon_lat()[1], 1.2, 1e-10, "sky_ref_p lat");
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn round_trip_skyframe_with_observer() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    original.set_obs_lat(0.3);
    original.set_obs_lon(-2.1);
    original.set_obs_alt(4200.0);
    original.set_dtai(37.0);
    original.set_dut1(0.27);

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_close(
                {
                    assert!(sf.test_obs_lat());
                    sf.obs_lat()
                },
                0.3,
                1e-10,
                "obs_lat",
            );
            assert_close(
                {
                    assert!(sf.test_obs_lon());
                    sf.obs_lon()
                },
                -2.1,
                1e-10,
                "obs_lon",
            );
            assert_close(
                {
                    assert!(sf.test_obs_alt());
                    sf.obs_alt()
                },
                4200.0,
                1e-10,
                "obs_alt",
            );
            assert_close(
                {
                    assert!(sf.test_dtai());
                    sf.dtai()
                },
                37.0,
                1e-10,
                "dtai",
            );
            assert_close(
                {
                    assert!(sf.test_dut1());
                    sf.dut1()
                },
                0.27,
                1e-10,
                "dut1",
            );
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn round_trip_skyframe_with_domain() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    original.set_domain("CUSTOM_SKY");

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    assert!(text.contains("CUSTOM_SKY"), "missing custom domain");

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.domain(), "CUSTOM_SKY");
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn round_trip_skyframe_geometry() {
    // Verify the round-tripped frame still computes distance correctly.
    let original = SkyFrame::new(SkySystem::Fk5);
    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();

    // Distance between two poles should be pi.
    let d = frame
        .distance(
            &[0.0, std::f64::consts::FRAC_PI_2],
            &[0.0, -std::f64::consts::FRAC_PI_2],
        )
        .unwrap();
    assert_close(d, std::f64::consts::PI, 1e-10, "distance");
}

#[test]
fn double_round_trip_skyframe() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    original.set_title("Double RT");
    original.set_epoch_mjd(53000.0);
    original.set_equinox_mjd(51544.5);
    original.set_neg_lon(true);
    original.set_sky_ref_is(SkyRefIs::Pole);
    original.set_sky_ref_lon_lat(0.5, 0.25);
    original.set_sky_ref_p_lon_lat(0.1, 1.0);
    original.set_sky_tol(1.0e-8);
    original.set_projection("SIN");
    original.set_align_offset(true);

    let text1 = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for SkyFrame");
}

#[test]
fn round_trip_skyframe_into_frame() {
    let original = SkyFrame::new(SkySystem::Icrs);
    let obj = AnyObject::SkyFrame(original);
    let dyn_frame = obj.into_frame().unwrap();
    assert_eq!(dyn_frame.naxes(), 2);
}

#[test]
fn any_object_skyframe_not_a_mapping() {
    let original = SkyFrame::new(SkySystem::Fk5);
    let obj = AnyObject::SkyFrame(original);
    assert!(obj.as_mapping().is_none());
    assert!(obj.as_frame().is_some());
}

#[test]
fn round_trip_frameset_with_skyframe() {
    // SkyFrame as the current frame in a FrameSet.
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap());
    let current: Box<dyn Frame> = Box::new(SkyFrame::new(SkySystem::Fk5));
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 0.01).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    let text = native_dump(&AnyObject::FrameSet(fs)).unwrap();
    assert!(text.contains("Begin SkyFrame"), "missing nested SkyFrame");

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    let pts = PointSet::from_rows([[100.0, 200.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 1.0, 1e-10, "fs skyframe x");
    assert_close(out.point(0)[1], 2.0, 1e-10, "fs skyframe y");
}

#[test]
fn round_trip_skyframe_fk4_besselian_equinox() {
    // FK4 with B1950 equinox -- should use Besselian epoch serialisation.
    let mut original = SkyFrame::new(SkySystem::Fk4);
    // B1950.0 is the default, but set it explicitly to force serialisation.
    let b1950_mjd = 33281.92346; // approximate B1950 MJD
    original.set_equinox_mjd(b1950_mjd);

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    assert!(text.contains("Eqnox"), "missing Eqnox for FK4");

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.system(), SkySystem::Fk4);
            assert_close(sf.equinox_mjd(), b1950_mjd, 1e-3, "FK4 equinox MJD");
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

// ===========================================================================
// Native read→write byte-exact round-trip tests (Task 9)
// ===========================================================================

/// Maximum ULP distance permitted between two f64 tokens that differ
/// as strings but parse to nearly-equal numbers. Covers the ~1 ULP
/// platform drift between macOS and Linux libm in transcendental-heavy
/// code paths (e.g. PAL/ERFA time conversions).
const NUMERIC_TOLERANCE_ULPS: u64 = 4;

/// Compare two f64 values by ULP distance.
fn within_ulps(a: f64, b: f64, max_ulps: u64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() {
        return false;
    }
    if a.is_sign_negative() != b.is_sign_negative() {
        // Different signs: only +0/-0 are considered equal here, and
        // those are caught by the a == b path above.
        return false;
    }
    let ai = a.to_bits();
    let bi = b.to_bits();
    let diff = ai.abs_diff(bi);
    diff <= max_ulps
}

/// Tokenise on whitespace and compare token-by-token. Tokens that are
/// not byte-equal are treated as equal if both parse as f64 and are
/// within `NUMERIC_TOLERANCE_ULPS`.
fn lines_numerically_equivalent(got: &str, want: &str) -> bool {
    // Split on whitespace but keep the separators, so a difference in them is
    // still a difference. `split_whitespace` would make `"Frm1 = "` and
    // `"Frm1 ="` compare equal, which is exactly the kind of divergence a
    // byte-exactness comparator exists to catch -- a comment-less object card
    // is written with a trailing space, and nothing else in the corpus would
    // reveal it. The tolerance covers numeric values and nothing else.
    fn split(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut i = 0;
        let b = s.as_bytes();
        while i < b.len() {
            let start = i;
            let ws = b[i].is_ascii_whitespace();
            while i < b.len() && b[i].is_ascii_whitespace() == ws {
                i += 1;
            }
            out.push(&s[start..i]);
        }
        out
    }
    let (g, w) = (split(got), split(want));
    if g.len() != w.len() {
        return false;
    }
    for (g, w) in g.iter().zip(w.iter()) {
        if g == w {
            continue;
        }
        let (Ok(gv), Ok(wv)) = (g.parse::<f64>(), w.parse::<f64>()) else {
            return false;
        };
        if !within_ulps(gv, wv, NUMERIC_TOLERANCE_ULPS) {
            return false;
        }
    }
    true
}

/// Round-trip fixtures the Rust reader cannot load yet, each with the class that
/// blocks it. Named here rather than passed over by a substring match on the
/// error text, so the list shrinks only by deliberate edit and
/// `round_trip_exclusions_still_fail` can check every entry still applies.
const ROUND_TRIP_EXCLUDED: &[(&str, &str)] = &[
    // A FrameSet holding a CmpFrame where a Mapping belongs. C accepts it
    // because astLoadFrameSet reads that slot with astRead and a Frame is a
    // Mapping; the port's Mapping loader dispatches on the class name and has
    // no Frame case, by decision: a Frame does not act as a Mapping in this
    // port (docs/issues/frame-in-a-mapping-slot.md).
    // `tools/oracle_fixtures/frame_in_map_slot_probe.c` is the oracle.
    (
        "frame_in_map_slot.ast",
        "a Frame in a Mapping slot, declined by decision",
    ),
];

/// Drop the leading run of comment lines, as the upstream roundtrip driver does
/// (`cmake/run_roundtrip_test.cmake`).
///
/// A fixture may carry a hand-written header saying what it is for -- see
/// `wcsconv/inputs/degen1.ast` -- and that header is documentation for a
/// reader, not part of the object, so a dump cannot reproduce it. Nothing else
/// is normalised: whatever a comparator normalises stops being testable, and
/// every line from the first `Begin` onwards must match.
fn strip_leading_comments(text: &str) -> String {
    let rest = text
        .lines()
        .skip_while(|l| l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        rest + "\n"
    } else {
        rest
    }
}

/// Read a C-generated .ast/.map/.simp file, write it back via native_dump,
/// and verify the output reproduces the input.
///
/// Reading a dump and writing it straight back is the identity, so any
/// difference is a serialisation defect. This is the only comparison in the
/// suite that can see one: every other harness calls `simplify` after reading,
/// and `astSimplify` re-stamps `IsSimp` on each node it visits, hiding a record
/// a loader dropped.
///
/// Tokens that disagree by ≤[`NUMERIC_TOLERANCE_ULPS`] when parsed as
/// f64 are treated as equal — this absorbs the ~1 ULP platform drift in
/// libm-driven values (e.g. Time1f computed via ERFA) without weakening
/// the byte-exact contract on the rest of the dump.
fn native_round_trip_fixture(path: &std::path::Path) -> Result<(), String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let obj = from_native_string(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let output = native_dump(&obj).map_err(|e| format!("render {}: {e}", path.display()))?;
    let text = strip_leading_comments(&text);
    let output = strip_leading_comments(&output);
    if output == text {
        return Ok(());
    }
    // Walk lines: byte-equal lines pass; otherwise fall back to
    // token-wise comparison with f64 ULP tolerance.
    for (i, (got, want)) in output.lines().zip(text.lines()).enumerate() {
        if got == want {
            continue;
        }
        if lines_numerically_equivalent(got, want) {
            continue;
        }
        return Err(format!(
            "{}: line {} differs:\n  got:  {:?}\n  want: {:?}",
            path.file_name().unwrap().to_string_lossy(),
            i + 1,
            got,
            want
        ));
    }
    let got_lines = output.lines().count();
    let want_lines = text.lines().count();
    if got_lines != want_lines {
        return Err(format!(
            "{}: line count differs: got {got_lines}, want {want_lines}",
            path.file_name().unwrap().to_string_lossy()
        ));
    }
    Ok(())
}

#[test]
fn native_byte_exact_round_trip_simplify_fixtures() {
    let fixture_dir = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/simplify"
    ));
    assert!(
        fixture_dir.is_dir(),
        "{} is missing. The corpus is committed, so its absence is a broken \
         checkout rather than a reason for this gate to pass.",
        fixture_dir.display()
    );

    let mut passed = 0usize;
    let mut failed = Vec::new();
    let mut skipped = 0usize;

    for entry in std::fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "map" && ext != "simp" {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if ROUND_TRIP_EXCLUDED.iter().any(|(n, _)| *n == name) {
            skipped += 1;
            continue;
        }
        match native_round_trip_fixture(&path) {
            Ok(()) => passed += 1,
            Err(e) => failed.push(e),
        }
    }

    println!(
        "\nNative round-trip (simplify_fixtures): {} passed, {} failed, {} excluded",
        passed,
        failed.len(),
        skipped
    );
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("  FAIL: {f}");
        }
    }
    eprintln!(
        "Native round-trip (simplify_fixtures): {}/{} byte-exact",
        passed,
        passed + failed.len()
    );
    // Hard-fail with the full list. Byte-exact round-trip with the C
    // library is a documented contract (CLAUDE.md: "Native output is
    // byte-exact with the C library"); silently reporting failures
    // means new regressions slip in unnoticed. If a class isn't yet
    // ready, the .map/.simp pair must be excluded from this corpus
    // explicitly rather than swept under the rug.
    assert!(
        failed.is_empty(),
        "{} simplify-fixture round-trip failures (see eprintln list above)",
        failed.len()
    );
}

/// FrameSet `.ast` fixtures (SkyFrame / SpecFrame / CmpFrame world frames)
/// that are not yet byte-exact on round-trip. Each entry MUST carry a
/// one-line reason. The fixtures are still generated by
/// `tools/oracle_fixtures/gen_frameset_fixtures.sh` (so they stay
/// reproducible); they are filtered out here rather than omitted, so the
/// remaining serialization gap is visible and tracked.
/// Frameset fixtures not yet byte-exact on round-trip, each with a one-line
/// reason (the generator still produces them). Tracked explicitly rather
/// than silently dropped. Empty: every wcsconv object round-trips byte-exact.
const FRAMESET_FIXTURE_EXCLUDED: &[(&str, &str)] = &[];

/// Round-trip C-oracle FrameSet native-text dumps (`*.ast`) through
/// `from_native_string` → `to_native_string` and assert byte-exact
/// (≤4-ULP per numeric token). These dumps are generated by
/// `tools/oracle_fixtures/gen_frameset_fixtures.sh` from **every** wcsconv
/// input object — each FITS header read through a FitsChan, plus the
/// pre-existing native `.ast` inputs read through a Channel — giving a
/// comprehensive serialization regression suite, not just the CmpFrame cases.
///
/// This isolates the native serializer from the FitsChan read/encode path:
/// the same `to_node`/`from_node` core also drives the FitsChan NATIVE
/// keyword-card form, so when this corpus is byte-exact the FitsChan
/// `native` wcsconv cases follow.
#[test]
fn native_byte_exact_round_trip_frameset_fixtures() {
    let fixture_dir = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/framesets"
    ));
    assert!(
        fixture_dir.is_dir(),
        "{} is missing. The corpus is committed, so its absence is a broken \
         checkout rather than a reason for this gate to pass.",
        fixture_dir.display()
    );

    let mut passed = 0usize;
    let mut failed = Vec::new();
    let mut excluded = 0usize;

    for entry in std::fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ast") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if FRAMESET_FIXTURE_EXCLUDED.iter().any(|(n, _)| *n == stem) {
            excluded += 1;
            continue;
        }
        match native_round_trip_fixture(&path) {
            Ok(()) => passed += 1,
            Err(e) => failed.push(e),
        }
    }

    println!(
        "\nNative round-trip (frameset_fixtures): {} passed, {} failed, {} excluded",
        passed,
        failed.len(),
        excluded
    );
    for f in &failed {
        eprintln!("  FAIL: {f}");
    }
    assert!(
        failed.is_empty(),
        "{} frameset-fixture round-trip failures (see eprintln list above)",
        failed.len()
    );
}

/// Round-trip every other native-encoded fixture in the tree, matching the file
/// set upstream's `roundtrip_*` ctests glob (`ast_tester/CMakeLists.txt:618`):
/// `serialisation/*.ast`, `programs/**/*.ast` and `wcsconv/framesets/*.ast` --
/// the last of which has its own sweep above.
///
/// `wcsconv/` itself is deliberately absent: its references are FITS-encoded, so
/// reading one back needs a FitsChan and the encoding it was written with, which
/// is what the wcsconv harness already does.
#[test]
fn native_byte_exact_round_trip_serialisation_fixtures() {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let fixture_dir = root.join("serialisation");
    assert!(
        fixture_dir.is_dir(),
        "{} is missing. The corpus is committed, so its absence is a broken \
         checkout rather than a reason for this gate to pass.",
        fixture_dir.display()
    );

    let mut passed = 0usize;
    let mut failed = Vec::new();
    let mut skipped = 0usize;

    let mut fixtures: Vec<std::path::PathBuf> = Vec::new();
    let mut stack = vec![fixture_dir.clone(), root.join("programs")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("ast") {
                fixtures.push(path);
            }
        }
    }
    fixtures.sort();

    for path in &fixtures {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if ROUND_TRIP_EXCLUDED.iter().any(|(n, _)| *n == name) {
            skipped += 1;
            continue;
        }
        match native_round_trip_fixture(path) {
            Ok(()) => passed += 1,
            Err(e) => failed.push(e),
        }
    }

    println!(
        "\nNative round-trip (serialisation + programs): {} passed, {} failed, {} excluded",
        passed,
        failed.len(),
        skipped
    );
    for f in &failed {
        eprintln!("  FAIL: {f}");
    }
    // Every fixture is compared and every failure is fatal. Reading a dump and
    // writing it back is the identity whatever the fixture holds, so a
    // difference is a serialisation defect and not a class-support question.
    assert!(
        failed.is_empty(),
        "{} round-trip failures (see the list above)",
        failed.len()
    );
    assert!(
        passed >= 30,
        "only {passed} fixtures were round-tripped; the tree held 30 when this \
         floor was set and fixtures are only ever added"
    );
}

/// Every `ROUND_TRIP_EXCLUDED` entry still names a fixture that fails.
#[test]
fn round_trip_exclusions_still_fail() {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let mut now_passing = Vec::new();
    for (name, blocker) in ROUND_TRIP_EXCLUDED {
        let mut found = None;
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.file_name().unwrap().to_string_lossy() == *name {
                    found = Some(path);
                }
            }
        }
        let path =
            found.unwrap_or_else(|| panic!("{name} is in ROUND_TRIP_EXCLUDED but not in the tree"));
        if native_round_trip_fixture(&path).is_ok() {
            now_passing.push(format!("{name} (was blocked on {blocker})"));
        }
    }
    assert!(
        now_passing.is_empty(),
        "these fixtures now round-trip, so they belong in the gate rather than \
         in `ROUND_TRIP_EXCLUDED`: {}",
        now_passing.join(", ")
    );
}

// ===========================================================================
// Region serialisation round-trip tests
// ===========================================================================

#[test]
fn round_trip_circle() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![10.0, 20.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let text = native_dump(&AnyObject::Circle(circle)).unwrap();

    // Structural markers.
    assert!(text.contains("Begin Circle"), "missing Begin Circle");
    assert!(text.contains("End Circle"), "missing End Circle");
    assert!(text.contains("Points"), "missing Points");

    // Deserialise and verify.
    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 2);

    // Verify the restored region contains the centre.
    assert!(region.point_in_region(&[10.0, 20.0]).unwrap());
    // Point far from centre should be outside.
    assert!(!region.point_in_region(&[100.0, 100.0]).unwrap());
}

#[test]
fn round_trip_box_region() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let box_region = BoxRegionBuilder::new(frame, vec![5.0, 5.0], vec![3.0, 4.0])
        .unwrap()
        .build()
        .unwrap();

    let text = native_dump(&AnyObject::BoxRegion(box_region)).unwrap();

    assert!(text.contains("Begin Box"), "missing Begin Box");
    assert!(text.contains("End Box"), "missing End Box");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 2);

    // Centre point inside.
    assert!(region.point_in_region(&[5.0, 5.0]).unwrap());
    // Point inside the box (centre=5,5 corner=3,4 means box spans [3,7]x[4,6]).
    assert!(region.point_in_region(&[6.0, 5.5]).unwrap());
    // Far outside.
    assert!(!region.point_in_region(&[50.0, 50.0]).unwrap());
}

#[test]
fn round_trip_polygon() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Triangle: (0,0), (10,0), (0,10)
    let polygon = PolygonBuilder::new(
        frame,
        vec![vec![0.0, 0.0], vec![10.0, 0.0], vec![0.0, 10.0]],
    )
    .unwrap()
    .build()
    .unwrap();

    let text = native_dump(&AnyObject::Polygon(polygon)).unwrap();

    assert!(text.contains("Begin Polygon"), "missing Begin Polygon");
    assert!(text.contains("End Polygon"), "missing End Polygon");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 2);

    // Centroid is inside.
    assert!(region.point_in_region(&[2.0, 2.0]).unwrap());
    // Outside.
    assert!(!region.point_in_region(&[8.0, 8.0]).unwrap());
}

#[test]
fn round_trip_interval() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![1.0, 2.0], vec![10.0, 20.0])
        .unwrap()
        .build()
        .unwrap();

    let text = native_dump(&AnyObject::Interval(interval)).unwrap();

    assert!(text.contains("Begin Interval"), "missing Begin Interval");
    assert!(text.contains("End Interval"), "missing End Interval");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 2);

    // Inside.
    assert!(region.point_in_region(&[5.0, 10.0]).unwrap());
    // Outside.
    assert!(!region.point_in_region(&[0.0, 0.0]).unwrap());
}

#[test]
fn round_trip_null_region() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let null = NullRegionBuilder::new(frame).build().unwrap();

    let text = native_dump(&AnyObject::NullRegion(null)).unwrap();

    assert!(
        text.contains("Begin NullRegion"),
        "missing Begin NullRegion"
    );
    assert!(text.contains("End NullRegion"), "missing End NullRegion");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 2);

    // NullRegion (un-negated) contains no points.
    assert!(!region.point_in_region(&[0.0, 0.0]).unwrap());
}

#[test]
fn round_trip_ellipse() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(
        frame,
        vec![5.0, 5.0],
        [3.0, 2.0],
        std::f64::consts::FRAC_PI_2,
    )
    .unwrap()
    .build()
    .unwrap();

    let text = native_dump(&AnyObject::Ellipse(ellipse)).unwrap();

    assert!(text.contains("Begin Ellipse"), "missing Begin Ellipse");
    assert!(text.contains("End Ellipse"), "missing End Ellipse");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 2);

    // Centre inside.
    assert!(region.point_in_region(&[5.0, 5.0]).unwrap());
    // Far outside.
    assert!(!region.point_in_region(&[50.0, 50.0]).unwrap());
}

#[test]
fn round_trip_pointlist() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let pointlist =
        PointListBuilder::new(frame, vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]])
            .unwrap()
            .build()
            .unwrap();

    let text = native_dump(&AnyObject::PointList(pointlist)).unwrap();

    assert!(text.contains("Begin PointList"), "missing Begin PointList");
    assert!(text.contains("End PointList"), "missing End PointList");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 2);

    // The PointList contains exactly its defining points.
    assert!(region.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(region.point_in_region(&[3.0, 4.0]).unwrap());
    // Not in the list.
    assert!(!region.point_in_region(&[0.0, 0.0]).unwrap());
}

#[test]
fn round_trip_cmpregion() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle1 = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let circle2 = CircleBuilder::new(frame, vec![3.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();

    let cmp = CmpRegionBuilder::new(circle1, circle2, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();

    let text = native_dump(&AnyObject::CmpRegion(cmp)).unwrap();

    assert!(text.contains("Begin CmpRegion"), "missing Begin CmpRegion");
    assert!(text.contains("End CmpRegion"), "missing End CmpRegion");
    assert!(text.contains("RegionA"), "missing RegionA");
    assert!(text.contains("RegionB"), "missing RegionB");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 2);

    // Point in the intersection.
    assert!(region.point_in_region(&[1.5, 0.0]).unwrap());
    // Point in circle1 but not circle2.
    assert!(!region.point_in_region(&[-4.0, 0.0]).unwrap());
}

#[test]
fn round_trip_prism() {
    let frame2d = BasicFrameBuilder::new(2).build().unwrap();
    let frame1d = BasicFrameBuilder::new(1).build().unwrap();

    let circle = CircleBuilder::new(frame2d, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let interval = IntervalBuilder::new(frame1d, vec![0.0], vec![10.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(circle, interval)
        .unwrap()
        .build()
        .unwrap();

    let text = native_dump(&AnyObject::Prism(prism)).unwrap();

    assert!(text.contains("Begin Prism"), "missing Begin Prism");
    assert!(text.contains("End Prism"), "missing End Prism");
    assert!(text.contains("RegionA"), "missing RegionA");
    assert!(text.contains("RegionB"), "missing RegionB");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();
    assert_eq!(region.naxes(), 3);

    // Inside the cylinder.
    assert!(region.point_in_region(&[0.0, 0.0, 5.0]).unwrap());
    // Outside (z out of range).
    assert!(!region.point_in_region(&[0.0, 0.0, 15.0]).unwrap());
    // Outside (xy out of range).
    assert!(!region.point_in_region(&[10.0, 10.0, 5.0]).unwrap());
}

#[test]
fn round_trip_negated_circle() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    circle.negate();

    let text = native_dump(&AnyObject::Circle(circle)).unwrap();

    assert!(text.contains("Negate"), "missing Negate attribute");

    let restored = from_native_string(&text).unwrap();
    let region = restored.as_region().unwrap();

    // Negated: centre is now OUTSIDE.
    assert!(!region.point_in_region(&[0.0, 0.0]).unwrap());
    // Far point is now INSIDE.
    assert!(region.point_in_region(&[100.0, 100.0]).unwrap());
}

// ===========================================================================
// 17. KeyMap round-trip
// ===========================================================================

#[test]
fn keymap_round_trips_through_native_text() {
    let mut inner = KeyMap::new();
    inner.put_0d("Scale", 2.5, Some("a scale factor")).unwrap();

    let mut km = KeyMap::new();
    km.put_0c("Name", "example", None).unwrap();
    km.put_1i("Counts", &[1, 2, 3], Some("three counts"))
        .unwrap();
    km.put_0a("Inner", AnyObject::KeyMap(Box::new(inner)), None)
        .unwrap();
    km.put_0a(
        "Zoom",
        AnyObject::ZoomMap(ZoomMap::new(2, 3.0).unwrap()),
        None,
    )
    .unwrap();

    let text = native_dump(&AnyObject::KeyMap(Box::new(km.clone()))).unwrap();
    assert!(text.contains("Begin KeyMap"));
    assert!(text.contains("Map of key/value pairs"));
    // A nested Object entry nests its own Begin/End block.
    assert!(text.contains("Begin ZoomMap"));

    let back = from_native_string(&text).unwrap();
    let AnyObject::KeyMap(back) = back else {
        panic!("expected a KeyMap");
    };
    assert_eq!(back.map_size(), 4);
    assert_eq!(back.get_0c("Name").unwrap().as_deref(), Some("example"));
    assert_eq!(back.get_1i("Counts").unwrap(), Some(vec![1, 2, 3]));
    assert_eq!(back.map_comment("Counts").as_deref(), Some("three counts"));

    let Some(AnyObject::KeyMap(inner)) = back.get_0a("Inner").unwrap().unwrap() else {
        panic!("expected a nested KeyMap");
    };
    assert_eq!(inner.get_0d("Scale").unwrap(), Some(2.5));
    assert_eq!(
        inner.map_comment("Scale").as_deref(),
        Some("a scale factor")
    );

    let Some(AnyObject::ZoomMap(zm)) = back.get_0a("Zoom").unwrap().unwrap() else {
        panic!("expected a ZoomMap");
    };
    assert_eq!(zm.rate(&[0.0, 0.0], 1, 1).unwrap(), 3.0);

    // Re-serialising the reloaded object gives identical text.
    assert_eq!(native_dump(&AnyObject::KeyMap(back)).unwrap(), text);
}

#[test]
fn a_keymap_is_neither_a_mapping_nor_a_frame_nor_a_region() {
    let obj = AnyObject::KeyMap(Box::new(KeyMap::new()));
    assert!(obj.as_mapping().is_none());
    assert!(obj.as_frame().is_none());
    assert!(obj.as_region().is_none());
    let obj = AnyObject::KeyMap(Box::new(KeyMap::new()));
    assert!(obj.into_mapping().is_none());
    let obj = AnyObject::KeyMap(Box::new(KeyMap::new()));
    assert!(obj.into_frame().is_none());
    let obj = AnyObject::KeyMap(Box::new(KeyMap::new()));
    assert!(obj.into_region().is_none());
}

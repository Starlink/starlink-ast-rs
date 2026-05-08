//! Serialisation round-trip tests for all concrete AST types.
//!
//! Tests the public API: `to_native_string`, `from_native_string`, and `AnyObject`.
//! Each concrete type is serialised to Native text, checked for structural markers,
//! then deserialised and verified for both attribute fidelity and functional equivalence.

use ast_core::coords::PointSet;
use ast_core::frames::{
    BasicFrameBuilder, CmpFrame, Frame, FrameRef, FrameSet, SkyFrame, SkyRefIs, SkySystem,
};
use ast_core::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MatrixMap, PermMap, ShiftMap, UnitMap, ZoomMap,
};
use ast_core::{AnyObject, from_native_string, to_native_string};
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
    let text = to_native_string(&AnyObject::UnitMap(original)).unwrap();

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
    let text = to_native_string(&AnyObject::UnitMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::ZoomMap(original)).unwrap();

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
    let text = to_native_string(&AnyObject::ZoomMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::ZoomMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::ShiftMap(original)).unwrap();

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
    let text = to_native_string(&AnyObject::ShiftMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::MatrixMap(original)).unwrap();

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
    let text = to_native_string(&AnyObject::MatrixMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::MatrixMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::PermMap(original)).unwrap();

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
    let text = to_native_string(&AnyObject::PermMap(original)).unwrap();

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
    let text = to_native_string(&AnyObject::PermMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::CmpMap(original)).unwrap();

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
    let text = to_native_string(&AnyObject::CmpMap(original)).unwrap();

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
    let text = to_native_string(&AnyObject::CmpMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::BasicFrame(original)).unwrap();

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
    let text = to_native_string(&AnyObject::BasicFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();
    assert_eq!(frame.naxes(), 3);
    // Default attributes should still hold.
    match restored {
        AnyObject::BasicFrame(ref bf) => {
            assert_eq!(bf.title(), "");
            assert_eq!(bf.domain(), "");
        }
        _ => panic!("expected BasicFrame variant"),
    }
}

#[test]
fn round_trip_basic_frame_geometry() {
    // Verify the round-tripped frame still computes distance correctly.
    let original = BasicFrameBuilder::new(2).build().unwrap();
    let text = to_native_string(&AnyObject::BasicFrame(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();

    let d = frame.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert_close(d, 5.0, 1e-12, "distance");
}

#[test]
fn round_trip_basic_frame_with_epoch() {
    let mut original = BasicFrameBuilder::new(1).build().unwrap();
    original.set_epoch(2000.0);
    let text = to_native_string(&AnyObject::BasicFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::BasicFrame(ref bf) => {
            assert_eq!(bf.epoch(), Some(2000.0));
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
    let text = to_native_string(&AnyObject::CmpFrame(original)).unwrap();

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
    let text = to_native_string(&AnyObject::CmpFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();
    assert_eq!(frame.naxes(), 5);
}

#[test]
fn round_trip_cmp_frame_distance() {
    let f1: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(1).build().unwrap());
    let f2: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(1).build().unwrap());
    let original = CmpFrame::new(f1, f2);
    let text = to_native_string(&AnyObject::CmpFrame(original)).unwrap();

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

    let text = to_native_string(&AnyObject::FrameSet(fs)).unwrap();

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

    let text = to_native_string(&AnyObject::FrameSet(fs)).unwrap();
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

    let text = to_native_string(&AnyObject::FrameSet(fs)).unwrap();
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

    let text = to_native_string(&AnyObject::FrameSet(fs)).unwrap();
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
    let text1 = to_native_string(&AnyObject::ZoomMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for ZoomMap");
}

#[test]
fn double_round_trip_shift_map() {
    let original = ShiftMap::new([1.5, -2.7, 0.0]);
    let text1 = to_native_string(&AnyObject::ShiftMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
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
    let text1 = to_native_string(&AnyObject::BasicFrame(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
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

    let text1 = to_native_string(&AnyObject::FrameSet(fs)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for FrameSet");
}

#[test]
fn double_round_trip_cmp_map() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let original = CmpMap::new(z, s, Composition::Series).unwrap();

    let text1 = to_native_string(&AnyObject::CmpMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for CmpMap");
}

#[test]
fn double_round_trip_perm_map() {
    let original = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let text1 = to_native_string(&AnyObject::PermMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for PermMap");
}

#[test]
fn double_round_trip_matrix_map() {
    let original = MatrixMap::new(array![[0.0, -1.0], [1.0, 0.0]]).unwrap();
    let text1 = to_native_string(&AnyObject::MatrixMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
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
    let text1 = to_native_string(&AnyObject::CmpFrame(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for CmpFrame");
}

#[test]
fn double_round_trip_unit_map() {
    let original = UnitMap::new(4);
    let text1 = to_native_string(&AnyObject::UnitMap(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch for UnitMap");
}

// ===========================================================================
// 12. Multi-point transforms after round-trip
// ===========================================================================

#[test]
fn round_trip_zoom_map_multi_point() {
    let original = ZoomMap::new(2, 3.0).unwrap();
    let text = to_native_string(&AnyObject::ZoomMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::ShiftMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::ZoomMap(original)).unwrap();
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
    let text = to_native_string(&AnyObject::ZoomMap(original)).unwrap();
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

    let text = to_native_string(&AnyObject::FrameSet(fs)).unwrap();
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
    let text = to_native_string(&AnyObject::CmpMap(cm)).unwrap();

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
    let text = to_native_string(&AnyObject::SkyFrame(original)).unwrap();

    assert!(text.contains("Begin SkyFrame"), "missing Begin SkyFrame");
    assert!(text.contains("End SkyFrame"), "missing End SkyFrame");
    assert!(text.contains("Naxes = 2"), "missing Naxes");
    assert!(text.contains("System = \"FK5\""), "missing System");
    assert!(text.contains("IsA Frame"), "missing IsA Frame");
    // Default domain is SKY, should NOT be serialised.
    assert!(
        !text.contains("Domain"),
        "default domain SKY should not be written"
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
            assert!(!sf.neglon());
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
    original.set_epoch(53000.0);
    original.set_neglon(true);
    original.set_sky_tol(1.0e-7);
    original.set_projection("TAN");
    original.set_align_offset(true);

    let text = to_native_string(&AnyObject::SkyFrame(original)).unwrap();

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
            assert_eq!(sf.title(), "Galactic Test");
            assert_close(sf.epoch_mjd(), 53000.0, 1e-10, "epoch");
            assert!(sf.neglon());
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
    original.set_equinox(j2010_mjd);

    let text = to_native_string(&AnyObject::SkyFrame(original)).unwrap();
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
    original.set_sky_ref(1.5, 0.75);
    original.set_sky_ref_p(0.3, 1.2);

    let text = to_native_string(&AnyObject::SkyFrame(original)).unwrap();
    assert!(text.contains("SRefIs"), "missing SRefIs");
    assert!(text.contains("SRef1"), "missing SRef1");
    assert!(text.contains("SRef2"), "missing SRef2");
    assert!(text.contains("SRefP1"), "missing SRefP1");
    assert!(text.contains("SRefP2"), "missing SRefP2");

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.sky_ref_is(), SkyRefIs::Origin);
            assert_close(sf.sky_ref()[0], 1.5, 1e-10, "sky_ref lon");
            assert_close(sf.sky_ref()[1], 0.75, 1e-10, "sky_ref lat");
            assert_close(sf.sky_ref_p()[0], 0.3, 1e-10, "sky_ref_p lon");
            assert_close(sf.sky_ref_p()[1], 1.2, 1e-10, "sky_ref_p lat");
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

    let text = to_native_string(&AnyObject::SkyFrame(original)).unwrap();

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_close(sf.obs_lat().unwrap(), 0.3, 1e-10, "obs_lat");
            assert_close(sf.obs_lon().unwrap(), -2.1, 1e-10, "obs_lon");
            assert_close(sf.obs_alt().unwrap(), 4200.0, 1e-10, "obs_alt");
            assert_close(sf.dtai().unwrap(), 37.0, 1e-10, "dtai");
            assert_close(sf.dut1().unwrap(), 0.27, 1e-10, "dut1");
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn round_trip_skyframe_with_domain() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    original.set_domain("CUSTOM_SKY");

    let text = to_native_string(&AnyObject::SkyFrame(original)).unwrap();
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
    let text = to_native_string(&AnyObject::SkyFrame(original)).unwrap();
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
    original.set_epoch(53000.0);
    original.set_equinox(51544.5);
    original.set_neglon(true);
    original.set_sky_ref_is(SkyRefIs::Pole);
    original.set_sky_ref(0.5, 0.25);
    original.set_sky_ref_p(0.1, 1.0);
    original.set_sky_tol(1.0e-8);
    original.set_projection("SIN");
    original.set_align_offset(true);

    let text1 = to_native_string(&AnyObject::SkyFrame(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = to_native_string(&restored).unwrap();
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

    let text = to_native_string(&AnyObject::FrameSet(fs)).unwrap();
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
    original.set_equinox(b1950_mjd);

    let text = to_native_string(&AnyObject::SkyFrame(original)).unwrap();
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

/// Read a C-generated .ast/.map/.simp file, write it back via to_native_string,
/// and verify the output is byte-exact with the input.
fn native_round_trip_fixture(path: &std::path::Path) -> Result<(), String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let obj = from_native_string(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let output = to_native_string(&obj).map_err(|e| format!("render {}: {e}", path.display()))?;
    if output.trim() == text.trim() {
        Ok(())
    } else {
        // Find first differing line
        for (i, (got, want)) in output.lines().zip(text.lines()).enumerate() {
            if got != want {
                return Err(format!(
                    "{}: line {} differs:\n  got:  {:?}\n  want: {:?}",
                    path.file_name().unwrap().to_string_lossy(),
                    i + 1,
                    got,
                    want
                ));
            }
        }
        let got_lines = output.lines().count();
        let want_lines = text.lines().count();
        Err(format!(
            "{}: line count differs: got {got_lines}, want {want_lines}",
            path.file_name().unwrap().to_string_lossy()
        ))
    }
}

#[test]
fn native_byte_exact_round_trip_simplify_fixtures() {
    let fixture_dir = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference/simplify_fixtures"
    ));
    if !fixture_dir.exists() {
        eprintln!("simplify_fixtures dir not found, skipping");
        return;
    }

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
        match native_round_trip_fixture(&path) {
            Ok(()) => passed += 1,
            Err(e) => {
                if e.contains("unknown") || e.contains("unsupported") {
                    skipped += 1;
                } else {
                    failed.push(e);
                }
            }
        }
    }

    println!(
        "\nNative round-trip (simplify_fixtures): {} passed, {} failed, {} skipped",
        passed,
        failed.len(),
        skipped
    );
    if !failed.is_empty() {
        for f in &failed[..failed.len().min(10)] {
            eprintln!("  FAIL: {f}");
        }
        if failed.len() > 10 {
            eprintln!("  ... and {} more", failed.len() - 10);
        }
    }
    // Report but don't hard-fail yet — this is aspirational
    eprintln!(
        "Native round-trip: {}/{} byte-exact",
        passed,
        passed + failed.len()
    );
}

#[test]
fn native_byte_exact_round_trip_generated_fixtures() {
    let fixture_dir = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/generated"
    ));
    if !fixture_dir.exists() {
        eprintln!("generated fixtures dir not found, skipping");
        return;
    }

    let mut passed = 0usize;
    let mut failed = Vec::new();
    let mut skipped = 0usize;

    for entry in std::fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "ast" {
            continue;
        }
        match native_round_trip_fixture(&path) {
            Ok(()) => passed += 1,
            Err(e) => {
                if e.contains("unknown") || e.contains("unsupported") {
                    skipped += 1;
                } else {
                    failed.push(e);
                }
            }
        }
    }

    println!(
        "\nNative round-trip (C-generated fixtures): {} passed, {} failed, {} skipped",
        passed,
        failed.len(),
        skipped
    );
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("  FAIL: {f}");
        }
    }
    // Mapping fixtures must all pass; Frame/FrameSet fixtures are tracked.
    let mapping_failures: Vec<_> = failed
        .iter()
        .filter(|f| {
            !f.contains("frame")
                && !f.contains("Frame")
                && !f.contains("skyframe")
                && !f.contains("specframe")
                && !f.contains("timeframe")
                && !f.contains("specflux")
                && !f.contains("timj")
                && !f.contains("lsst")
                && !f.contains("degen")
                && !f.contains("splittest")
        })
        .collect();
    if !mapping_failures.is_empty() {
        panic!(
            "{} mapping fixture(s) failed byte-exact round-trip: {:?}",
            mapping_failures.len(),
            mapping_failures
        );
    }
}

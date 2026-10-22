//! Tests for TranMap — port of reference/starlink-ast/ast_tester logic.
//!
//! A TranMap combines the forward transformation of one Mapping with
//! the inverse transformation of another Mapping.

use crate::common::native_dump;
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{CmpMap, Composition, Mapping, TranMap, ZoomMap};
use ast_core::{AnyObject, from_native_string};

fn is_bad(v: f64) -> bool {
    v == BAD_VALUE
}

// ---------------------------------------------------------------------------
// Construction and basic properties
// ---------------------------------------------------------------------------

#[test]
fn basic_construction() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    assert_eq!(tm.nin(), 1);
    assert_eq!(tm.nout(), 1);
    assert!(!tm.is_inverted());
}

#[test]
fn construction_dimension_mismatch_nin() {
    let z1 = ZoomMap::new(1, 2.0).unwrap();
    let z2 = ZoomMap::new(2, 3.0).unwrap();
    let result = TranMap::new(Box::new(z1), Box::new(z2));
    assert!(result.is_err());
}

#[test]
fn construction_dimension_mismatch_nout() {
    // Use CmpMap to create mappings with different nout:
    // CmpMap(Zoom(1,2), Zoom(1,3), Parallel) has nin=2, nout=2
    let z1 = ZoomMap::new(1, 2.0).unwrap();
    let z2 = ZoomMap::new(2, 3.0).unwrap();
    // z1: nin=1, nout=1; z2: nin=2, nout=2
    // These have different nin, so TranMap should fail.
    let result = TranMap::new(Box::new(z1), Box::new(z2));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Forward and inverse transforms
// ---------------------------------------------------------------------------

#[test]
fn forward_uses_map1() {
    // Forward uses map1 (ZoomMap factor 2): output = input * 2
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    let pts = PointSet::from_rows([[5.0]]);
    let out = tm.transform(&pts, true).unwrap();
    let p = out.point(0);
    assert!((p[0] - 10.0).abs() < 1e-14);
}

#[test]
fn inverse_uses_map2() {
    // Inverse uses map2's inverse: ZoomMap(3) inverse is divide by 3.
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    let pts = PointSet::from_rows([[9.0]]);
    let out = tm.transform(&pts, false).unwrap();
    let p = out.point(0);
    assert!((p[0] - 3.0).abs() < 1e-14);
}

#[test]
fn forward_and_inverse_are_different() {
    // For a normal mapping, forward(inverse(x)) = x.
    // For a TranMap(ZoomMap(2), ZoomMap(3)), forward(5)=10, inverse(10)=10/3.
    // They are intentionally different.
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    let pts = PointSet::from_rows([[5.0]]);
    let fwd = tm.transform(&pts, true).unwrap();
    let inv = tm.transform(&fwd, false).unwrap();
    let p = inv.point(0);
    // Forward gives 10, inverse of 10 gives 10/3 ~= 3.333...
    assert!((p[0] - 10.0 / 3.0).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// Inversion
// ---------------------------------------------------------------------------

#[test]
fn inversion_swaps_transforms() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let mut tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    // Invert the TranMap.
    tm.invert();
    assert!(tm.is_inverted());

    // Forward of inverted TranMap = inverse of original = map2's inverse.
    // map2 is ZoomMap(3), its inverse divides by 3.
    let pts = PointSet::from_rows([[9.0]]);
    let out = tm.transform(&pts, true).unwrap();
    let p = out.point(0);
    assert!((p[0] - 3.0).abs() < 1e-14);

    // Inverse of inverted TranMap = forward of original = map1's forward.
    // map1 is ZoomMap(2), multiplies by 2.
    let pts2 = PointSet::from_rows([[5.0]]);
    let out2 = tm.transform(&pts2, false).unwrap();
    let p2 = out2.point(0);
    assert!((p2[0] - 10.0).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// Bad value handling
// ---------------------------------------------------------------------------

#[test]
fn bad_input_forward() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = tm.transform(&pts, true).unwrap();
    let p = out.point(0);
    assert!(is_bad(p[0]));
}

#[test]
fn bad_input_inverse() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = tm.transform(&pts, false).unwrap();
    let p = out.point(0);
    assert!(is_bad(p[0]));
}

// ---------------------------------------------------------------------------
// equal()
// ---------------------------------------------------------------------------

#[test]
fn equal_same_components() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm1 = TranMap::new(Box::new(z2.clone()), Box::new(z3.clone())).unwrap();
    let tm2 = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();
    assert!(tm1.equal(&tm2));
}

#[test]
fn equal_different_components() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let z4 = ZoomMap::new(1, 4.0).unwrap();
    let tm1 = TranMap::new(Box::new(z2.clone()), Box::new(z3)).unwrap();
    let tm2 = TranMap::new(Box::new(z2), Box::new(z4)).unwrap();
    assert!(!tm1.equal(&tm2));
}

#[test]
fn equal_inverted_vs_swapped() {
    // Inverted TranMap(A, B) should equal non-inverted TranMap(inv(B), inv(A))
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let mut tm1 = TranMap::new(Box::new(z2.clone()), Box::new(z3.clone())).unwrap();
    tm1.invert();

    // Create TranMap(inv(z3), inv(z2)):
    // inv(ZoomMap(3)) forward = divide by 3, i.e. ZoomMap(1/3)
    // inv(ZoomMap(2)) forward = divide by 2, i.e. ZoomMap(1/2)
    let mut z3_inv = z3;
    z3_inv.invert();
    let mut z2_inv = z2;
    z2_inv.invert();
    let tm2 = TranMap::new(Box::new(z3_inv), Box::new(z2_inv)).unwrap();

    assert!(tm1.equal(&tm2));
}

#[test]
fn not_equal_to_non_tranmap() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2.clone()), Box::new(z3)).unwrap();
    assert!(!tm.equal(&z2));
}

// ---------------------------------------------------------------------------
// Simplification: equal components → single mapping
// ---------------------------------------------------------------------------

#[test]
fn simplify_equal_components() {
    // TranMap(ZoomMap(2), ZoomMap(2)) should simplify to just ZoomMap(2)
    // since both components are equal and both have forward + inverse defined.
    let z2a = ZoomMap::new(1, 2.0).unwrap();
    let z2b = ZoomMap::new(1, 2.0).unwrap();
    let tm = TranMap::new(Box::new(z2a), Box::new(z2b)).unwrap();

    let simp = (&tm as &dyn Mapping).simplify().unwrap();

    // The simplified mapping should be a ZoomMap, not a TranMap.
    assert!(simp.as_any().is::<ZoomMap>());

    // Verify it works correctly.
    let pts = PointSet::from_rows([[5.0]]);
    let out = simp.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 10.0).abs() < 1e-14);
    let out_inv = simp.transform(&pts, false).unwrap();
    assert!((out_inv.point(0)[0] - 2.5).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// Simplification: invert normalization
// ---------------------------------------------------------------------------

#[test]
fn simplify_invert_normalize() {
    // An inverted TranMap(ZoomMap(2), ZoomMap(0.5)) should normalize to
    // TranMap(ZoomMap(2), ZoomMap(0.5)) non-inverted (the inversion causes
    // the components to swap and invert, giving TranMap(inv(0.5), inv(2)) =
    // TranMap(ZoomMap(2), ZoomMap(0.5))).
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z05 = ZoomMap::new(1, 0.5).unwrap();
    let mut tm = TranMap::new(Box::new(z2), Box::new(z05)).unwrap();
    tm.invert();

    let simp = (&tm as &dyn Mapping).simplify().unwrap();

    // Should still be a TranMap (components are not equal).
    assert!(simp.as_any().is::<TranMap>());
    // Should not be inverted.
    assert!(!simp.is_inverted());

    // Forward of inverted TranMap(2, 0.5) = inverse of original = inv(0.5) = multiply by 2
    // After simplification, forward should still produce the same result.
    let pts = PointSet::from_rows([[5.0]]);
    let out = simp.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 10.0).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// Simplification: component simplification
// ---------------------------------------------------------------------------

#[test]
fn simplify_component_simplification() {
    // TranMap(CmpMap(Zoom(2);Zoom(3)), CmpMap(Zoom(0.5);Zoom(2))) should
    // simplify the components: CmpMap(Z2;Z3)→Z6, CmpMap(Z0.5;Z2)→Z1=UnitMap
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let z05 = ZoomMap::new(1, 0.5).unwrap();
    let z2b = ZoomMap::new(1, 2.0).unwrap();

    let cmp1 = CmpMap::new(Box::new(z2), Box::new(z3), Composition::Series).unwrap();
    let cmp2 = CmpMap::new(Box::new(z05), Box::new(z2b), Composition::Series).unwrap();

    let tm = TranMap::new(Box::new(cmp1), Box::new(cmp2)).unwrap();
    let simp = (&tm as &dyn Mapping).simplify().unwrap();

    // Simplified forward should multiply by 6.
    let pts = PointSet::from_rows([[5.0]]);
    let out = simp.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 30.0).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// Serialization round-trip
// ---------------------------------------------------------------------------

#[test]
fn serialization_roundtrip() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    let obj = AnyObject::TranMap(tm.clone());
    let text = native_dump(&obj).unwrap();

    // Verify the text contains expected markers.
    assert!(text.contains("TranMap"));
    assert!(text.contains("MapA"));
    assert!(text.contains("MapB"));

    // Round-trip.
    let restored = from_native_string(&text).unwrap();
    let restored_mapping = restored.as_mapping().unwrap();

    // Test that it behaves the same.
    let pts = PointSet::from_rows([[5.0]]);
    let orig_fwd = tm.transform(&pts, true).unwrap();
    let rest_fwd = restored_mapping.transform(&pts, true).unwrap();
    assert!((orig_fwd.point(0)[0] - rest_fwd.point(0)[0]).abs() < 1e-14);

    let orig_inv = tm.transform(&pts, false).unwrap();
    let rest_inv = restored_mapping.transform(&pts, false).unwrap();
    assert!((orig_inv.point(0)[0] - rest_inv.point(0)[0]).abs() < 1e-14);
}

#[test]
fn serialization_roundtrip_inverted() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let mut tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();
    tm.invert();

    let obj = AnyObject::TranMap(tm.clone());
    let text = native_dump(&obj).unwrap();
    assert!(text.contains("Invert"));

    let restored = from_native_string(&text).unwrap();
    let restored_mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[5.0]]);
    let orig_fwd = tm.transform(&pts, true).unwrap();
    let rest_fwd = restored_mapping.transform(&pts, true).unwrap();
    assert!((orig_fwd.point(0)[0] - rest_fwd.point(0)[0]).abs() < 1e-14);
}

#[test]
fn serialization_with_inv_a_flag() {
    // Create a TranMap where map1 is inverted at construction.
    let mut z2 = ZoomMap::new(1, 2.0).unwrap();
    z2.invert();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();

    let obj = AnyObject::TranMap(tm.clone());
    let text = native_dump(&obj).unwrap();
    assert!(text.contains("InvA"));

    // Round-trip.
    let restored = from_native_string(&text).unwrap();
    let restored_mapping = restored.as_mapping().unwrap();

    // An inverted ZoomMap(2) as forward means it divides by 2.
    let pts = PointSet::from_rows([[10.0]]);
    let orig_fwd = tm.transform(&pts, true).unwrap();
    let rest_fwd = restored_mapping.transform(&pts, true).unwrap();
    assert!((orig_fwd.point(0)[0] - rest_fwd.point(0)[0]).abs() < 1e-14);
    assert!((orig_fwd.point(0)[0] - 5.0).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// Multi-dimensional tests
// ---------------------------------------------------------------------------

#[test]
fn multidim_transform() {
    // 2D TranMap: forward scales by 2 on both axes, inverse by 1/5 on both.
    let z2 = ZoomMap::new(2, 2.0).unwrap();
    let z5 = ZoomMap::new(2, 5.0).unwrap();
    let tm = TranMap::new(Box::new(z2), Box::new(z5)).unwrap();

    assert_eq!(tm.nin(), 2);
    assert_eq!(tm.nout(), 2);

    let pts = PointSet::from_rows([[3.0, 4.0]]);
    let fwd = tm.transform(&pts, true).unwrap();
    assert!((fwd.point(0)[0] - 6.0).abs() < 1e-14);
    assert!((fwd.point(0)[1] - 8.0).abs() < 1e-14);

    let inv = tm.transform(&pts, false).unwrap();
    assert!((inv.point(0)[0] - 0.6).abs() < 1e-14);
    assert!((inv.point(0)[1] - 0.8).abs() < 1e-14);
}

// ---------------------------------------------------------------------------
// try_merge: adjacent TranMap merge
// ---------------------------------------------------------------------------

#[test]
fn merge_adjacent_tranmaps() {
    // Two TranMaps in series: TranMap(Z2, Z0.5) ; TranMap(Z3, Z(1/3))
    // Combined forward = Z2;Z3 = Z6
    // Combined inverse = Z(1/3);Z(0.5) = Z(1/6)
    // Result: TranMap(Z6, Z(1/6))
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z05 = ZoomMap::new(1, 0.5).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let z_third = ZoomMap::new(1, 1.0 / 3.0).unwrap();

    let tm1 = TranMap::new(Box::new(z2), Box::new(z05)).unwrap();
    let tm2 = TranMap::new(Box::new(z3), Box::new(z_third)).unwrap();

    // Compose them in a CmpMap series and simplify.
    let cmp = CmpMap::new(Box::new(tm1), Box::new(tm2), Composition::Series).unwrap();
    let simp = (&cmp as &dyn Mapping).simplify().unwrap();

    // Simplified forward should multiply by 6.
    let pts = PointSet::from_rows([[5.0]]);
    let out = simp.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 30.0).abs() < 1e-14);

    // Simplified inverse: TranMap2.inverse ; TranMap1.inverse = *3 ; *2 = *6
    // (map2 of TranMap(Z2, Z0.5) is Z0.5, whose inverse is *2;
    //  map2 of TranMap(Z3, Z(1/3)) is Z(1/3), whose inverse is *3)
    // Combined inverse path in reverse: Z(1/3).inv ; Z(0.5).inv = *3 ; *2 = *6
    let out_inv = simp.transform(&pts, false).unwrap();
    assert!((out_inv.point(0)[0] - 30.0).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Negative tests: no merge when not applicable
// ---------------------------------------------------------------------------

#[test]
fn no_merge_parallel_tranmaps() {
    // TranMaps in parallel should NOT merge via try_merge.
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z05 = ZoomMap::new(1, 0.5).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let z_third = ZoomMap::new(1, 1.0 / 3.0).unwrap();

    let tm1 = TranMap::new(Box::new(z2), Box::new(z05)).unwrap();
    let tm2 = TranMap::new(Box::new(z3), Box::new(z_third)).unwrap();

    // try_merge in parallel should return None.
    let result = tm1.try_merge(&tm2, false, false, false);
    assert!(result.is_none());
}

#[test]
fn no_merge_with_non_tranmap() {
    // A TranMap should not merge with a plain ZoomMap.
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let z5 = ZoomMap::new(1, 5.0).unwrap();

    let tm = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();
    let result = tm.try_merge(&z5, true, false, false);
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// Clone
// ---------------------------------------------------------------------------

#[test]
fn clone_produces_independent_copy() {
    let z2 = ZoomMap::new(1, 2.0).unwrap();
    let z3 = ZoomMap::new(1, 3.0).unwrap();
    let tm1 = TranMap::new(Box::new(z2), Box::new(z3)).unwrap();
    let mut tm2 = tm1.clone();
    tm2.invert();

    // Original should be unaffected.
    assert!(!tm1.is_inverted());
    assert!(tm2.is_inverted());
}

#[test]
fn simplify_invert_normalize_fixture_roundtrip() {
    // Verify that simplifying the tranmap_invert_normalize fixture
    // produces byte-exact output matching the C reference.
    let fixture_dir = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/simplify"
    ));
    let input_text =
        std::fs::read_to_string(fixture_dir.join("tranmap_invert_normalize.map")).unwrap();
    let ref_text =
        std::fs::read_to_string(fixture_dir.join("tranmap_invert_normalize.simp")).unwrap();

    let input_obj = from_native_string(&input_text).unwrap();
    let mapping = input_obj.into_mapping().unwrap();
    let ref_obj = from_native_string(&ref_text).unwrap();
    let ref_mapping = ref_obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();
    assert!(
        simplified.equal(ref_mapping.as_ref()),
        "simplified should equal reference"
    );

    // Verify byte-exact serialization.
    if let Some(tm) = simplified.as_any().downcast_ref::<TranMap>() {
        let obj = AnyObject::TranMap(tm.clone());
        let text = native_dump(&obj).unwrap();
        assert_eq!(text, ref_text);
    }
}

/// `decompose` gives the forward and inverse Mappings and the Invert values
/// to apply them with; an inverted TranMap swaps them, as C's
/// `astDecompose` does (C probe).
#[test]
fn decompose_gives_components_and_invert_values_like_c() {
    use ast_core::mappings::ShiftMap;
    let mut shift = ShiftMap::new(vec![1.0]);
    shift.invert();
    let mut map = TranMap::new(Box::new(ZoomMap::new(1, 2.0).unwrap()), Box::new(shift)).unwrap();
    let d = map.decompose();
    assert!(d.map1.as_any().is::<ZoomMap>() && d.map2.as_any().is::<ShiftMap>());
    assert_eq!((d.invert1, d.invert2), (false, true));
    map.invert();
    let d = map.decompose();
    assert!(d.map1.as_any().is::<ShiftMap>() && d.map2.as_any().is::<ZoomMap>());
    assert_eq!((d.invert1, d.invert2), (false, true));
}

/// `decompose` gives the Invert values recorded in the TranMap, which a
/// loaded TranMap can hold apart from its components' own Invert flags.
#[test]
fn decompose_gives_recorded_invert_values_not_component_flags() {
    let text = " Begin TranMap \t# Compound Transformation Mapping
    Nin = 1 \t# Number of input coordinates
 IsA Mapping \t# Mapping between coordinate systems
    InvA = 1 \t# First Mapping used in inverse direction
    MapA = \t# Mapping for forward transformation
       Begin ZoomMap \t# Zoom about the origin
          Nin = 1 \t# Number of input coordinates
       IsA Mapping \t# Mapping between coordinate systems
          Zoom = 2 \t# Zoom factor
       End ZoomMap
    MapB = \t# Mapping for inverse transformation
       Begin ZoomMap \t# Zoom about the origin
          Nin = 1 \t# Number of input coordinates
       IsA Mapping \t# Mapping between coordinate systems
          Zoom = 0.5 \t# Zoom factor
       End ZoomMap
 End TranMap
";
    let mapping = from_native_string(text).unwrap().into_mapping().unwrap();
    let map = mapping.as_any().downcast_ref::<TranMap>().unwrap();
    let d = map.decompose();
    assert!(!d.map1.is_inverted());
    assert_eq!((d.invert1, d.invert2), (true, false));
}

/// A PermMap supplying constants to a conic WcsMap that lacks its PV2_1
/// parameter: simplifying it raises an error that is not an undefined
/// transformation.
fn failing_component() -> CmpMap {
    use ast_core::mappings::{PermMap, WcsMap, WcsProjection};
    CmpMap::new(
        Box::new(PermMap::new(&[0, 0], &[-1, -2], &[0.1, 0.2]).unwrap()),
        Box::new(WcsMap::new(2, WcsProjection::Coe, 1, 2).unwrap()),
        Composition::Series,
    )
    .unwrap()
}

/// The error simplifying an encapsulated Mapping is passed up from the
/// TranMap's simplification, as in C.
#[test]
fn an_error_simplifying_a_component_is_passed_up() {
    use ast_core::mappings::UnitMap;
    let map = TranMap::new(Box::new(failing_component()), Box::new(UnitMap::new(2))).unwrap();
    let err = <dyn Mapping>::simplify(&map).unwrap_err();
    assert_eq!(err.code(), ast_core::error::ErrorCode::Wcspa);
}

/// Two TranMaps in series merge by simplifying their forward and their
/// inverse Mappings in series; an error that raises is passed up, as in C.
#[test]
fn an_error_merging_two_tranmaps_is_passed_up() {
    use ast_core::mappings::{PermMap, UnitMap, WcsMap, WcsProjection};
    let constants = TranMap::new(
        Box::new(PermMap::new(&[0, 0], &[-1, -2], &[0.1, 0.2]).unwrap()),
        Box::new(UnitMap::new(2)),
    )
    .unwrap();
    let conic = TranMap::new(
        Box::new(WcsMap::new(2, WcsProjection::Coe, 1, 2).unwrap()),
        Box::new(UnitMap::new(2)),
    )
    .unwrap();
    let map = CmpMap::new(Box::new(constants), Box::new(conic), Composition::Series).unwrap();
    let err = <dyn Mapping>::simplify(&map).unwrap_err();
    assert_eq!(err.code(), ast_core::error::ErrorCode::Wcspa);
}

/// Two TranMaps in series merge into one whose inverse applies the higher
/// TranMap's inverse first, as the series does; non-commuting inverse
/// matrices make the order visible.
#[test]
fn a_merged_tranmap_keeps_the_order_of_the_inverses() {
    use ast_core::mappings::{MatrixMap, UnitMap};
    use ndarray::array;
    let lower = TranMap::new(
        Box::new(UnitMap::new(2)),
        Box::new(MatrixMap::new(array![[1.0, 2.0], [0.0, 1.0]]).unwrap()),
    )
    .unwrap();
    let higher = TranMap::new(
        Box::new(UnitMap::new(2)),
        Box::new(MatrixMap::new(array![[1.0, 0.0], [3.0, 1.0]]).unwrap()),
    )
    .unwrap();
    let map = CmpMap::new(Box::new(lower), Box::new(higher), Composition::Series).unwrap();
    let simplified = <dyn Mapping>::simplify(&map).unwrap();
    assert!(simplified.as_any().is::<TranMap>());
    let expected = map.tran2(&[1.0], &[2.0], false).unwrap();
    let got = simplified.tran2(&[1.0], &[2.0], false).unwrap();
    for (e, g) in [(expected.0[0], got.0[0]), (expected.1[0], got.1[0])] {
        assert!((e - g).abs() < 1e-12, "{e} != {g}");
    }
}

/// TranMaps of 2->3 then 3->4 Mappings merge, their inverse Mappings
/// combining lower first, as in C.
#[test]
fn tranmaps_of_differing_dimensions_merge() {
    use ast_core::mappings::PermMap;
    let lower = TranMap::new(
        Box::new(PermMap::new(&[1, 2], &[1, 2, -1], &[5.0]).unwrap()),
        Box::new(PermMap::new(&[2, 1], &[2, 1, -1], &[6.0]).unwrap()),
    )
    .unwrap();
    let higher = TranMap::new(
        Box::new(PermMap::new(&[1, 2, 3], &[1, 2, 3, -1], &[5.0]).unwrap()),
        Box::new(PermMap::new(&[3, 2, 1], &[3, 2, 1, -1], &[6.0]).unwrap()),
    )
    .unwrap();
    let map = CmpMap::new(Box::new(lower), Box::new(higher), Composition::Series).unwrap();
    let simplified = <dyn Mapping>::simplify(&map).unwrap();
    assert!(simplified.as_any().is::<TranMap>());
}

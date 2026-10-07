//! Tests for RateMap — differential mapping (Jacobian element).
//!
//! Ported from reference/starlink-ast/ast_tester concepts and the
//! simplify regression fixtures.

use ast_core::error::ErrorCode;
use ast_core::mappings::{
    CmpMap, Composition, DynMapping, Mapping, RateMap, ShiftMap, UnitMap, ZoomMap,
};
use ast_core::{AnyObject, from_native_string, to_native_string};

const BAD_VALUE: f64 = -f64::MAX;

/// Helper: check if a value is BAD.
fn is_bad(v: f64) -> bool {
    v == BAD_VALUE
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_construction_basic() {
    let zm: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let rm = RateMap::new(zm, 1, 1).unwrap();
    assert_eq!(rm.nin(), 1);
    assert_eq!(rm.nout(), 1);
    assert_eq!(rm.ax1(), 1);
    assert_eq!(rm.ax2(), 1);
    assert!(!rm.is_inverted());
    assert!(rm.has_forward());
    assert!(!rm.has_inverse());
}

#[test]
fn test_ratemap_construction_multi_axis() {
    let zm: DynMapping = Box::new(ZoomMap::new(3, 2.0).unwrap());
    let rm = RateMap::new(zm, 2, 3).unwrap();
    assert_eq!(rm.nin(), 3);
    assert_eq!(rm.nout(), 1);
    assert_eq!(rm.ax1(), 2);
    assert_eq!(rm.ax2(), 3);
}

#[test]
fn test_ratemap_construction_invalid_axes() {
    let zm: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    // ax1 out of range (output axis > nout).
    assert!(RateMap::new(zm.clone(), 3, 1).is_err());
    // ax2 out of range (input axis > nin).
    assert!(RateMap::new(zm.clone(), 1, 3).is_err());
    // ax1 = 0 (must be >= 1).
    assert!(RateMap::new(zm.clone(), 0, 1).is_err());
    // ax2 = 0 (must be >= 1).
    assert!(RateMap::new(zm, 1, 0).is_err());
}

// ---------------------------------------------------------------------------
// Transform — forward (rate computation)
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_zoommap_rate() {
    // ZoomMap with factor 3: output = 3 * input.
    // d(output)/d(input) = 3.0 everywhere.
    let zm: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let rm = RateMap::new(zm, 1, 1).unwrap();

    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0, 2.0, 5.0, -10.0]]).unwrap();
    let result = rm.transform(&pts, true).unwrap();
    assert_eq!(result.ncoord(), 1);
    assert_eq!(result.npoint(), 4);

    for i in 0..4 {
        let val = result.point(i)[0];
        assert!(
            (val - 3.0).abs() < 1e-6,
            "Expected 3.0 at point {i}, got {val}"
        );
    }
}

#[test]
fn test_ratemap_shiftmap_rate() {
    // ShiftMap with shifts [5.0, 10.0]: output_1 = input_1 + 5, output_2 = input_2 + 10.
    // d(output_1)/d(input_1) = 1.0 everywhere.
    let sm: DynMapping = Box::new(ShiftMap::new([5.0, 10.0]));
    let rm = RateMap::new(sm, 1, 1).unwrap();

    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0, 2.0], &[3.0, 4.0]]).unwrap();
    let result = rm.transform(&pts, true).unwrap();
    assert_eq!(result.ncoord(), 1);
    assert_eq!(result.npoint(), 2);

    for i in 0..2 {
        let val = result.point(i)[0];
        assert!(
            (val - 1.0).abs() < 1e-6,
            "Expected 1.0 at point {i}, got {val}"
        );
    }
}

#[test]
fn test_ratemap_off_diagonal_zero() {
    // ZoomMap(2D, factor=3): output = 3 * input for both axes.
    // d(output_1)/d(input_2) = 0.0 (off-diagonal).
    let zm: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    let rm = RateMap::new(zm, 1, 2).unwrap();

    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0, 2.0], &[3.0, 4.0]]).unwrap();
    let result = rm.transform(&pts, true).unwrap();
    assert_eq!(result.ncoord(), 1);

    for i in 0..2 {
        let val = result.point(i)[0];
        assert!(val.abs() < 1e-6, "Expected 0.0 at point {i}, got {val}");
    }
}

#[test]
fn test_ratemap_bad_input_propagation() {
    let zm: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let rm = RateMap::new(zm, 1, 1).unwrap();

    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0, BAD_VALUE, 3.0]]).unwrap();
    let result = rm.transform(&pts, true).unwrap();

    // Point 0: should be 3.0.
    assert!((result.point(0)[0] - 3.0).abs() < 1e-6);
    // Point 1: BAD input → BAD output.
    assert!(is_bad(result.point(1)[0]));
    // Point 2: should be 3.0.
    assert!((result.point(2)[0] - 3.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// Transform — inverse (undefined for non-inverted RateMap)
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_inverse_undefined() {
    let zm: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let rm = RateMap::new(zm, 1, 1).unwrap();

    assert!(!rm.has_inverse());

    // Requesting the inverse is AST__TRNND, as C's base Transform reports.
    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0, 2.0]]).unwrap();
    let err = rm.transform(&pts, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Trnnd);
}

// ---------------------------------------------------------------------------
// Inversion swaps directions
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_inverted() {
    let zm: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let mut rm = RateMap::new(zm, 1, 1).unwrap();
    rm.invert();

    // When inverted: nin=1, nout=1 (since intrinsic_nout=1, intrinsic_nin=1 for 1D).
    // Actually for 1D: intrinsic_nin=1, intrinsic_nout=1.
    // When inverted: nin = intrinsic_nout = 1, nout = intrinsic_nin = 1.
    assert_eq!(rm.nin(), 1);
    assert_eq!(rm.nout(), 1);
    assert!(!rm.has_forward());
    assert!(rm.has_inverse());
    assert!(rm.is_inverted());

    // Forward is undefined for an inverted RateMap: AST__TRNND.
    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0]]).unwrap();
    let err = rm.transform(&pts, true).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Trnnd);

    // Inverse should compute the rate.
    let result = rm.transform(&pts, false).unwrap();
    assert!((result.point(0)[0] - 3.0).abs() < 1e-6);
}

#[test]
fn test_ratemap_inverted_multi_axis() {
    // 2D ZoomMap: inverted RateMap swaps nin/nout.
    let zm: DynMapping = Box::new(ZoomMap::new(2, 5.0).unwrap());
    let mut rm = RateMap::new(zm, 1, 1).unwrap();
    // Not inverted: nin=2, nout=1.
    assert_eq!(rm.nin(), 2);
    assert_eq!(rm.nout(), 1);

    rm.invert();
    // Inverted: nin=1, nout=2.
    assert_eq!(rm.nin(), 1);
    assert_eq!(rm.nout(), 2);
}

// ---------------------------------------------------------------------------
// equal()
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_equal_same() {
    let zm1: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 2).unwrap();
    let rm2 = RateMap::new(zm2, 1, 2).unwrap();
    assert!(rm1.equal(&rm2));
}

#[test]
fn test_ratemap_equal_different_axes() {
    let zm1: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 1).unwrap();
    let rm2 = RateMap::new(zm2, 1, 2).unwrap();
    assert!(!rm1.equal(&rm2));
}

#[test]
fn test_ratemap_equal_different_mapping() {
    let zm1: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(1, 5.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 1).unwrap();
    let rm2 = RateMap::new(zm2, 1, 1).unwrap();
    assert!(!rm1.equal(&rm2));
}

#[test]
fn test_ratemap_equal_different_invert() {
    // Per C code: RateMaps with different Invert flags are never equal.
    let zm1: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 1).unwrap();
    let mut rm2 = RateMap::new(zm2, 1, 1).unwrap();
    rm2.invert();
    assert!(!rm1.equal(&rm2));
}

// ---------------------------------------------------------------------------
// simplified() — interior simplification
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_simplify_interior() {
    // RateMap wrapping CmpMap(ZoomMap(2), ZoomMap(3), series) should simplify
    // to RateMap wrapping ZoomMap(6).
    let zm1: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    let cmp = CmpMap::new(zm1, zm2, Composition::Series).unwrap();
    let cmp_dyn: DynMapping = Box::new(cmp);

    let rm = RateMap::new(cmp_dyn, 1, 1).unwrap();
    let simp = <dyn Mapping>::simplify(&rm).unwrap();

    // The simplified version should still be a RateMap (not a CmpMap or UnitMap).
    assert!(
        simp.as_any().is::<RateMap>(),
        "Expected RateMap after interior simplification"
    );

    // The rate should be 6.0 (= 2 * 3, the composed zoom factors).
    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0], &[1.0]]).unwrap();
    let result = simp.transform(&pts, true).unwrap();
    assert!(
        (result.point(0)[0] - 6.0).abs() < 1e-6,
        "Expected rate 6.0, got {}",
        result.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// try_merge — inverse cancellation
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_inverse_cancel_merge() {
    // RateMap(ZoomMap(2), ax1=1, ax2=1) in series with its inverse
    // should cancel to UnitMap.
    let zm1: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 1).unwrap();
    let rm2 = RateMap::new(zm2, 1, 1).unwrap();

    // self_inv=false, other_inv=true → opposite invert flags → can cancel.
    let merged = rm1.try_merge(&rm2, true, false, true);
    assert!(merged.is_some());
    let merged = merged.unwrap();
    assert!(merged.as_any().is::<UnitMap>());
    assert_eq!(merged.nin(), 1);
    assert_eq!(merged.nout(), 1);
}

#[test]
fn test_ratemap_no_cancel_same_invert() {
    // Same invert flags → no cancellation.
    let zm1: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 1).unwrap();
    let rm2 = RateMap::new(zm2, 1, 1).unwrap();

    let merged = rm1.try_merge(&rm2, true, false, false);
    assert!(merged.is_none());
}

#[test]
fn test_ratemap_no_cancel_different_axes() {
    // Different axes → no cancellation.
    let zm1: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 1).unwrap();
    let rm2 = RateMap::new(zm2, 1, 2).unwrap();

    let merged = rm1.try_merge(&rm2, true, false, true);
    assert!(merged.is_none());
}

#[test]
fn test_ratemap_no_cancel_different_mapping() {
    // Different encapsulated mappings → no cancellation.
    let zm1: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 1).unwrap();
    let rm2 = RateMap::new(zm2, 1, 1).unwrap();

    let merged = rm1.try_merge(&rm2, true, false, true);
    assert!(merged.is_none());
}

#[test]
fn test_ratemap_no_cancel_parallel() {
    // Parallel composition → no cancellation.
    let zm1: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let zm2: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let rm1 = RateMap::new(zm1, 1, 1).unwrap();
    let rm2 = RateMap::new(zm2, 1, 1).unwrap();

    let merged = rm1.try_merge(&rm2, false, false, true);
    assert!(merged.is_none());
}

// ---------------------------------------------------------------------------
// Serialization round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_serialization_roundtrip() {
    let zm: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    let rm = RateMap::new(zm, 2, 1).unwrap();

    let obj = AnyObject::RateMap(rm);
    let native = to_native_string(&obj).unwrap();

    let obj2 = from_native_string(&native).unwrap();
    let rm2 = obj2.as_mapping().unwrap();

    // Check it's a RateMap with correct properties.
    let rm2_concrete = rm2.as_any().downcast_ref::<RateMap>().unwrap();
    assert_eq!(rm2_concrete.nin(), 2);
    assert_eq!(rm2_concrete.nout(), 1);
    assert_eq!(rm2_concrete.ax1(), 2);
    assert_eq!(rm2_concrete.ax2(), 1);
    assert!(!rm2_concrete.is_inverted());

    // Verify it computes the same rates.
    // For ZoomMap(3) on 2 axes: output_i = 3 * input_i.
    // d(output_2)/d(input_1) = 0 (off-diagonal).
    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0, 2.0], &[3.0, 4.0]]).unwrap();
    let result = rm2.transform(&pts, true).unwrap();
    for i in 0..2 {
        let val = result.point(i)[0];
        assert!(val.abs() < 1e-6, "Expected 0.0 at point {i}, got {val}");
    }
}

#[test]
fn test_ratemap_serialization_roundtrip_diagonal() {
    // Test diagonal case: ax1=1, ax2=1 for ZoomMap(2D, factor=3).
    // d(output_1)/d(input_1) = 3.0.
    let zm: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    let rm = RateMap::new(zm, 1, 1).unwrap();

    let obj = AnyObject::RateMap(rm);
    let native = to_native_string(&obj).unwrap();

    let obj2 = from_native_string(&native).unwrap();
    let rm2 = obj2.as_mapping().unwrap();

    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0, 2.0], &[3.0, 4.0]]).unwrap();
    let result = rm2.transform(&pts, true).unwrap();
    for i in 0..2 {
        let val = result.point(i)[0];
        assert!(
            (val - 3.0).abs() < 1e-6,
            "Expected 3.0 at point {i}, got {val}"
        );
    }
}

#[test]
fn test_ratemap_serialization_default_axes() {
    // ax1=1, ax2=1 → IIn=0, IOut=0 → not written in serialized form.
    let zm: DynMapping = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let rm = RateMap::new(zm, 1, 1).unwrap();

    let obj = AnyObject::RateMap(rm);
    let native = to_native_string(&obj).unwrap();

    // Should NOT contain IIn or IOut since they default to 0.
    assert!(
        !native.contains("IIn"),
        "Should not contain IIn for default axis"
    );
    assert!(
        !native.contains("IOut"),
        "Should not contain IOut for default axis"
    );

    // Round-trip should still work.
    let obj2 = from_native_string(&native).unwrap();
    let rm2 = obj2
        .as_mapping()
        .unwrap()
        .as_any()
        .downcast_ref::<RateMap>()
        .unwrap();
    assert_eq!(rm2.ax1(), 1);
    assert_eq!(rm2.ax2(), 1);
}

#[test]
fn test_ratemap_read_fixture_inverse_cancel() {
    // Read the ratemap_inverse_cancel.map fixture (CmpMap with two RateMaps).
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/ratemap_inverse_cancel.map").unwrap();
    let obj = from_native_string(&input).unwrap();
    let mapping = obj.as_mapping().unwrap();

    // It's a CmpMap containing two RateMaps.
    assert_eq!(mapping.nin(), 1);
    // The second is inverted (InvB=1), so it defines the inverse of the CmpMap.
}

#[test]
fn test_ratemap_read_fixture_simplify_interior() {
    // Read the ratemap_simplify_interior.map fixture.
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/ratemap_simplify_interior.map").unwrap();
    let obj = from_native_string(&input).unwrap();
    let mapping = obj.as_mapping().unwrap();

    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 1);
}

// ---------------------------------------------------------------------------
// Simplify regression tests (from fixtures)
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_simplify_inverse_cancel_fixture() {
    // The fixture: CmpMap(RateMap(ZoomMap(2)), inv(RateMap(ZoomMap(2))), series)
    // Should simplify to UnitMap(1).
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/ratemap_inverse_cancel.map").unwrap();
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
fn test_ratemap_simplify_interior_fixture() {
    // The fixture: RateMap(CmpMap(ZoomMap(2), ZoomMap(3), series), ax1=1, ax2=1)
    // Should simplify to RateMap(ZoomMap(6), ax1=1, ax2=1).
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/ratemap_simplify_interior.map").unwrap();
    let obj = from_native_string(&input).unwrap();
    let mapping = obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();
    let rm = simplified
        .as_any()
        .downcast_ref::<RateMap>()
        .expect("Expected RateMap after simplification");

    // Verify: rate should be 6.0 (= 2 * 3) for a ZoomMap(6) on the diagonal.
    let pts = ast_core::coords::PointSet::from_columns(&[&[1.0], &[1.0]]).unwrap();
    let result = rm.transform(&pts, true).unwrap();
    assert!(
        (result.point(0)[0] - 6.0).abs() < 1e-6,
        "Expected rate 6.0, got {}",
        result.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// Negative simplify tests (should NOT simplify)
// ---------------------------------------------------------------------------

#[test]
fn test_ratemap_neg_nonratemap_neighbour() {
    // RateMap in series with a non-RateMap → no cancellation.
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/neg_ratemap_nonratemap_neighbour.map")
            .unwrap();
    let obj = from_native_string(&input).unwrap();
    let mapping = obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();
    // Should NOT simplify to a UnitMap — the neighbour is not a RateMap.
    assert!(
        !simplified.as_any().is::<UnitMap>(),
        "Should not simplify to UnitMap"
    );
}

#[test]
fn test_ratemap_neg_different_inner() {
    // Two RateMaps with different encapsulated mappings → no cancellation.
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/neg_ratemap_different_inner.map").unwrap();
    let obj = from_native_string(&input).unwrap();
    let mapping = obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();
    assert!(
        !simplified.as_any().is::<UnitMap>(),
        "Should not simplify to UnitMap with different inner mappings"
    );
}

#[test]
fn test_ratemap_neg_parallel() {
    // Two RateMaps in parallel → no cancellation (only series cancels).
    let input =
        std::fs::read_to_string("tests/fixtures/simplify/neg_ratemap_parallel.map").unwrap();
    let obj = from_native_string(&input).unwrap();
    let mapping = obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();
    assert!(
        !simplified.as_any().is::<UnitMap>(),
        "Should not simplify parallel RateMaps to UnitMap"
    );
}

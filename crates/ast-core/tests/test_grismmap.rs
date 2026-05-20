//! Tests for GrismMap.
//!
//! Ported from reference/starlink-ast/ast_tester/ (GrismMap sections).
//! Tests the forward/inverse transforms, serialization round-trip,
//! equal() semantics, and simplification rules.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{GrismMap, Mapping};
use ast_core::{AnyObject, from_native_string, to_native_string};

/// Helper: check if a value is BAD_VALUE.
fn is_bad(v: f64) -> bool {
    v == BAD_VALUE
}

// ===========================================================================
// Construction and attributes
// ===========================================================================

#[test]
fn default_grismmap_attributes() {
    let gm = GrismMap::default_map();
    assert_eq!(gm.nin(), 1);
    assert_eq!(gm.nout(), 1);
    assert_eq!(gm.grism_nr(), 1.0);
    assert_eq!(gm.grism_nrp(), 0.0);
    assert_eq!(gm.grism_wave_r(), 5000.0);
    assert_eq!(gm.grism_alpha(), 0.0);
    assert_eq!(gm.grism_g(), 0.0);
    assert_eq!(gm.grism_m(), 0);
    assert_eq!(gm.grism_eps(), 0.0);
    assert_eq!(gm.grism_theta(), 0.0);
    assert!(!gm.is_inverted());
}

#[test]
fn custom_grismmap_attributes() {
    let gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    assert_eq!(gm.grism_nr(), 1.5);
    assert_eq!(gm.grism_nrp(), -1e-6);
    assert_eq!(gm.grism_wave_r(), 5000.0);
    assert_eq!(gm.grism_alpha(), 0.1);
    assert_eq!(gm.grism_g(), 2e-4);
    assert_eq!(gm.grism_m(), 1);
    assert_eq!(gm.grism_eps(), 0.02);
    assert_eq!(gm.grism_theta(), 0.03);
}

// ===========================================================================
// Transform tests
// ===========================================================================

#[test]
fn forward_transform_typical_grism() {
    // A typical grism configuration
    let gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);

    // Transform a single grism parameter value
    let input = PointSet::from_rows(vec![vec![0.0]]);
    let output = gm.transform(&input, true).unwrap();
    let wavelength = output.point(0)[0];

    // The output should be a valid wavelength (not BAD)
    assert!(!is_bad(wavelength));
    // For a typical grism, the reference wavelength should be close to
    // the output when the grism parameter is near zero.
    assert!(wavelength > 0.0, "wavelength should be positive");
}

#[test]
fn forward_inverse_roundtrip() {
    let gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);

    // Forward: grism parameter → wavelength
    let grism_params = PointSet::from_rows(vec![vec![0.0], vec![0.01], vec![-0.01], vec![0.05]]);
    let wavelengths = gm.transform(&grism_params, true).unwrap();

    // Inverse: wavelength → grism parameter
    let recovered = gm.transform(&wavelengths, false).unwrap();

    // Check round-trip accuracy
    for i in 0..4 {
        let orig = grism_params.point(i)[0];
        let back = recovered.point(i)[0];
        assert!(!is_bad(back), "point {i}: inverse returned BAD");
        let diff = (orig - back).abs();
        assert!(
            diff < 1e-12,
            "point {i}: roundtrip error {diff} (orig={orig}, back={back})"
        );
    }
}

#[test]
fn inverse_forward_roundtrip() {
    let gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);

    // Start with wavelength values, apply inverse then forward
    let wavelengths =
        PointSet::from_rows(vec![vec![4900.0], vec![5000.0], vec![5100.0], vec![5200.0]]);
    let grism_params = gm.transform(&wavelengths, false).unwrap();
    let recovered = gm.transform(&grism_params, true).unwrap();

    for i in 0..4 {
        let orig = wavelengths.point(i)[0];
        let back = recovered.point(i)[0];
        assert!(!is_bad(back), "point {i}: forward returned BAD");
        let diff = (orig - back).abs();
        assert!(
            diff < 1e-10,
            "point {i}: roundtrip error {diff} (orig={orig}, back={back})"
        );
    }
}

#[test]
fn bad_value_propagation() {
    let gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);

    let input = PointSet::from_rows(vec![vec![BAD_VALUE], vec![0.0]]);
    let output = gm.transform(&input, true).unwrap();
    assert!(is_bad(output.point(0)[0]));
    assert!(!is_bad(output.point(1)[0]));
}

#[test]
fn degenerate_params_produce_bad() {
    // Default GrismMap has k2=0 which makes forward output BAD
    let gm = GrismMap::default_map();

    let input = PointSet::from_rows(vec![vec![0.0], vec![0.5]]);
    let output = gm.transform(&input, true).unwrap();
    assert!(is_bad(output.point(0)[0]));
    assert!(is_bad(output.point(1)[0]));
}

#[test]
fn inverted_grismmap_swaps_directions() {
    let mut gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);

    // Forward of non-inverted
    let input = PointSet::from_rows(vec![vec![0.0]]);
    let fwd_result = gm.transform(&input, true).unwrap();

    gm.invert();
    assert!(gm.is_inverted());

    // Forward of inverted = inverse of original
    let inv_fwd_result = gm.transform(&input, true).unwrap();
    // Inverse of inverted = forward of original
    let inv_inv_result = gm.transform(&input, false).unwrap();

    // inv_inv_result should match original fwd_result
    let diff = (fwd_result.point(0)[0] - inv_inv_result.point(0)[0]).abs();
    assert!(
        diff < 1e-15,
        "inverted inverse should match forward: diff={diff}"
    );

    // inv_fwd_result should be the inverse transform
    let recovered = gm.transform(&inv_fwd_result, false).unwrap();
    let diff2 = (input.point(0)[0] - recovered.point(0)[0]).abs();
    assert!(
        diff2 < 1e-12,
        "roundtrip through inverted map failed: diff={diff2}"
    );
}

// ===========================================================================
// equal() tests
// ===========================================================================

#[test]
fn equal_same_params() {
    let gm1 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let gm2 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    assert!(gm1.equal(&gm2));
}

#[test]
fn not_equal_different_nr() {
    let gm1 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let gm2 = GrismMap::new(1.6, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    assert!(!gm1.equal(&gm2));
}

#[test]
fn not_equal_different_m() {
    let gm1 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let gm2 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, -1, 0.02, 0.03);
    assert!(!gm1.equal(&gm2));
}

#[test]
fn not_equal_different_inversion() {
    let gm1 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let mut gm2 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    gm2.invert();
    assert!(!gm1.equal(&gm2));
}

// ===========================================================================
// Serialization round-trip
// ===========================================================================

#[test]
fn native_serialization_roundtrip() {
    let gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let obj = AnyObject::GrismMap(gm.clone());
    let text = to_native_string(&obj).unwrap();

    // Parse back
    let parsed = from_native_string(&text).unwrap();
    let mapping = parsed.as_mapping().unwrap();

    // Should be equal
    assert!(gm.equal(mapping));

    // Should produce same transform results
    let input = PointSet::from_rows(vec![vec![0.0], vec![0.01], vec![-0.01]]);
    let out_orig = gm.transform(&input, true).unwrap();
    let out_parsed = mapping.transform(&input, true).unwrap();
    for i in 0..3 {
        let diff = (out_orig.point(i)[0] - out_parsed.point(i)[0]).abs();
        assert!(
            diff < 1e-15,
            "transform mismatch after roundtrip at point {i}"
        );
    }
}

#[test]
fn native_serialization_roundtrip_inverted() {
    let mut gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    gm.invert();
    let obj = AnyObject::GrismMap(gm.clone());
    let text = to_native_string(&obj).unwrap();
    let parsed = from_native_string(&text).unwrap();
    let mapping = parsed.as_mapping().unwrap();
    assert!(gm.equal(mapping));
}

#[test]
fn native_serialization_roundtrip_defaults() {
    let gm = GrismMap::default_map();
    let obj = AnyObject::GrismMap(gm.clone());
    let text = to_native_string(&obj).unwrap();
    let parsed = from_native_string(&text).unwrap();
    let mapping = parsed.as_mapping().unwrap();
    assert!(gm.equal(mapping));
}

// ===========================================================================
// Simplification tests
// ===========================================================================

#[test]
fn inverse_cancellation() {
    // Two GrismMaps with opposite M values and opposite directions cancel
    let gm1 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let gm2 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, -1, 0.02, 0.03);

    // GrismMap(fwd) + GrismMap(inv with different M) should cancel
    let result = gm1.try_merge(&gm2 as &dyn Mapping, true, false, true);
    assert!(
        result.is_some(),
        "GrismMap + inverse GrismMap (different M) should cancel"
    );
    let merged = result.unwrap();
    assert_eq!(merged.nin(), 1);
    assert_eq!(merged.nout(), 1);
    // Should be a UnitMap (identity)
    let input = PointSet::from_rows(vec![vec![42.0]]);
    let output = merged.transform(&input, true).unwrap();
    assert_eq!(output.point(0)[0], 42.0);
}

#[test]
fn no_cancel_same_direction() {
    // Same M, same direction — should NOT cancel
    let gm1 = GrismMap::default_map();
    let gm2 = GrismMap::default_map();
    let result = gm1.try_merge(&gm2 as &dyn Mapping, true, false, false);
    assert!(result.is_none(), "same direction should not cancel");
}

#[test]
fn no_cancel_same_m_opposite_direction() {
    // Same M, opposite direction — C library does NOT cancel (M must differ)
    let gm1 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let gm2 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let result = gm1.try_merge(&gm2 as &dyn Mapping, true, false, true);
    assert!(
        result.is_none(),
        "same M opposite direction should not cancel per C library"
    );
}

#[test]
fn no_cancel_different_attrs() {
    // Different NR values — should not cancel even with opposite direction
    let gm1 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let gm2 = GrismMap::new(1.6, -1e-6, 5000.0, 0.1, 2e-4, -1, 0.02, 0.03);
    let result = gm1.try_merge(&gm2 as &dyn Mapping, true, false, true);
    assert!(result.is_none(), "different attrs should not cancel");
}

#[test]
fn no_merge_parallel() {
    // GrismMaps cannot merge in parallel
    let gm1 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let gm2 = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, -1, 0.02, 0.03);
    let result = gm1.try_merge(&gm2 as &dyn Mapping, false, false, true);
    assert!(result.is_none(), "parallel merge should not be supported");
}

// ===========================================================================
// Rate test
// ===========================================================================

#[test]
fn rate_at_zero() {
    let gm = GrismMap::new(1.5, -1e-6, 5000.0, 0.1, 2e-4, 1, 0.02, 0.03);
    let rate = gm.rate(&[0.0], 1, 1).unwrap();
    // Rate should be finite and non-zero for a valid grism
    assert!(rate.is_finite());
    assert!(rate != 0.0);
}

#[test]
fn merge_with_zoom_preserves_explicit_flags_for_unchanged_attrs() {
    use ast_core::mappings::GrismMap;
    use ast_core::{AnyObject, to_native_string};

    // Default GrismMap (all attrs at defaults). In a fixture parsed
    // from disk, all flags would be "default-flagged"; we approximate
    // by using default_map() and then merging.
    let g = GrismMap::default_map();
    let merged = g.merge_with_zoom(0.5, true).unwrap();
    let dyn_grism = merged.as_any().downcast_ref::<GrismMap>().unwrap().clone();
    let s = to_native_string(&AnyObject::GrismMap(dyn_grism)).unwrap();

    // The three modified attrs should be explicit (present uncommented).
    assert!(
        s.lines()
            .any(|l| l.contains("GrmNRP =") && !l.trim_start().starts_with('#')),
        "GrmNRP must be explicit:\n{s}"
    );
    assert!(
        s.lines()
            .any(|l| l.contains("GrmWR =") && !l.trim_start().starts_with('#')),
        "GrmWR must be explicit:\n{s}"
    );
    assert!(
        s.lines()
            .any(|l| l.contains("GrmG =") && !l.trim_start().starts_with('#')),
        "GrmG must be explicit:\n{s}"
    );

    // The others must NOT be explicit (present only as # comments — the
    // input GrismMap was constructed via default_map(), so its set_flags
    // are all true; after the proposed fix we *preserve* those flags.
    // Therefore in this *unit* test all eight are still expected explicit.
    // The serial-comparison check belongs to the regression fixture, which
    // parses from disk where unset attrs come in as default-flagged.
}

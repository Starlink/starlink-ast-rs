// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Tests for GrismMap.
//!
//! Ported from reference/starlink-ast/ast_tester/ (GrismMap sections).
//! Tests the forward/inverse transforms, serialization round-trip,
//! equal() semantics, and simplification rules.

use crate::common::native_dump;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{GrismMap, Mapping};
use starlink_ast::{AnyObject, from_native_string};

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
    let text = native_dump(&obj).unwrap();

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
    let text = native_dump(&obj).unwrap();
    let parsed = from_native_string(&text).unwrap();
    let mapping = parsed.as_mapping().unwrap();
    assert!(gm.equal(mapping));
}

#[test]
fn native_serialization_roundtrip_defaults() {
    let gm = GrismMap::default_map();
    let obj = AnyObject::GrismMap(gm.clone());
    let text = native_dump(&obj).unwrap();
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

/// A GrismMap made with no attribute values leaves every attribute unset, as
/// C's `astGrismMap("")` does, so its dump writes the defaults commented out.
/// The expected dump is C's.
#[test]
fn default_grismmap_dumps_as_c_does() {
    let want = [
        " Begin GrismMap \t# Map 1-d coordinates using a spectral disperser",
        "    Nin = 1 \t# Number of input coordinates",
        " IsA Mapping \t# Mapping between coordinate systems",
        "#   GrmNR = 1 \t# Refractive index at the ref. wavelength",
        "#   GrmNRP = 0 \t# Rate of change of refractive index",
        "#   GrmWR = 5000 \t# Ref. wavelength",
        "#   GrmAlp = 0 \t# Angle of incidence of incoming light",
        "#   GrmG = 0 \t# Grating ruling density",
        "#   GrmM = 0 \t# The interference order",
        "#   GrmEps = 0 \t# Angle between grating normal and dispersion plane",
        "#   GrmTh = 0 \t# Angle between detector normal and reference ray",
        " End GrismMap",
    ]
    .join("\n")
        + "\n";
    let got = native_dump(&starlink_ast::AnyObject::GrismMap(GrismMap::default_map())).unwrap();
    assert_eq!(got, want);
}

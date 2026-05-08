//! Tests for WcsMap — FITS-WCS sky projection mapping.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, WcsMap, WcsProjection};
use ast_core::{AnyObject, from_native_string, to_native_string};

// ===========================================================================
// Helpers
// ===========================================================================

fn assert_close(actual: f64, expected: f64, tol: f64, label: &str) {
    assert!(
        (actual - expected).abs() < tol,
        "{label}: expected {expected}, got {actual} (diff {})",
        (actual - expected).abs()
    );
}

// ===========================================================================
// Construction and accessors
// ===========================================================================

#[test]
fn new_basic() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    assert_eq!(wcs.nin(), 2);
    assert_eq!(wcs.nout(), 2);
    assert_eq!(wcs.lonax(), 1);
    assert_eq!(wcs.latax(), 2);
    assert_eq!(wcs.projection(), WcsProjection::Tan);
    assert!(!wcs.is_inverted());
}

#[test]
fn new_rejects_ncoord_less_than_2() {
    assert!(WcsMap::new(1, WcsProjection::Tan, 1, 1).is_err());
}

#[test]
fn new_rejects_equal_axes() {
    assert!(WcsMap::new(2, WcsProjection::Tan, 1, 1).is_err());
}

#[test]
fn new_rejects_axis_out_of_range() {
    assert!(WcsMap::new(2, WcsProjection::Tan, 0, 2).is_err());
    assert!(WcsMap::new(2, WcsProjection::Tan, 3, 2).is_err());
    assert!(WcsMap::new(2, WcsProjection::Tan, 1, 3).is_err());
}

// ===========================================================================
// TAN projection round-trip
// ===========================================================================

#[test]
fn tan_forward_inverse_round_trip() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    // A point near the pole in radians.
    let lon = 0.1_f64;
    let lat = 1.4_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = wcs.transform(&pts, true).unwrap();

    // Inverse should recover the original.
    let back = wcs.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "lon round-trip");
    assert_close(back.point(0)[1], lat, 1e-10, "lat round-trip");
}

// ===========================================================================
// CAR projection (plate carree)
// ===========================================================================

#[test]
fn car_round_trip() {
    let wcs = WcsMap::new(2, WcsProjection::Car, 1, 2).unwrap();
    let lon = 0.5_f64;
    let lat = 0.3_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = wcs.transform(&pts, true).unwrap();
    let back = wcs.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "CAR lon round-trip");
    assert_close(back.point(0)[1], lat, 1e-10, "CAR lat round-trip");
}

// ===========================================================================
// AIT projection (Hammer-Aitoff)
// ===========================================================================

#[test]
fn ait_round_trip() {
    let wcs = WcsMap::new(2, WcsProjection::Ait, 1, 2).unwrap();
    let lon = 0.4_f64;
    let lat = 0.6_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = wcs.transform(&pts, true).unwrap();
    let back = wcs.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "AIT lon round-trip");
    assert_close(back.point(0)[1], lat, 1e-10, "AIT lat round-trip");
}

// ===========================================================================
// Multi-axis passthrough
// ===========================================================================

#[test]
fn multi_axis_passthrough() {
    // 4 axes, lon on axis 2, lat on axis 3.
    let wcs = WcsMap::new(4, WcsProjection::Tan, 2, 3).unwrap();
    assert_eq!(wcs.nin(), 4);
    assert_eq!(wcs.nout(), 4);
    assert_eq!(wcs.lonax(), 2);
    assert_eq!(wcs.latax(), 3);

    let a1 = 100.0;
    let lon = 0.1_f64;
    let lat = 1.3_f64;
    let a4 = 200.0;
    let pts = PointSet::from_rows([[a1, lon, lat, a4]]);
    let out = wcs.transform(&pts, true).unwrap();

    // Axes 1 and 4 should pass through unchanged.
    assert_eq!(out.point(0)[0], a1, "axis 1 passthrough");
    assert_eq!(out.point(0)[3], a4, "axis 4 passthrough");

    // Axes 2 and 3 should be transformed (different from input).
    // For TAN near the pole, the projected values will differ from input.
    let x = out.point(0)[1];
    let y = out.point(0)[2];

    // Inverse should recover original.
    let back = wcs.transform(&out, false).unwrap();
    assert_eq!(back.point(0)[0], a1, "axis 1 inverse passthrough");
    assert_eq!(back.point(0)[3], a4, "axis 4 inverse passthrough");
    assert_close(back.point(0)[1], lon, 1e-10, "lon inverse");
    assert_close(back.point(0)[2], lat, 1e-10, "lat inverse");

    // Verify the projected values are actually different from the spherical ones
    // (i.e., the projection is actually doing something).
    assert!(
        (x - lon).abs() > 1e-6 || (y - lat).abs() > 1e-6,
        "TAN projection should change at least one of the coordinates"
    );
}

// ===========================================================================
// BAD_VALUE propagation
// ===========================================================================

#[test]
fn bad_value_on_lon() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 1.0]]);
    let out = wcs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn bad_value_on_lat() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let pts = PointSet::from_rows([[0.1, BAD_VALUE]]);
    let out = wcs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn nan_treated_as_bad() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let pts = PointSet::from_rows([[f64::NAN, 1.0]]);
    let out = wcs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

// ===========================================================================
// Inversion
// ===========================================================================

#[test]
fn inverted_flag() {
    let mut wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    assert!(!wcs.is_inverted());
    wcs.invert();
    assert!(wcs.is_inverted());

    // With invert flag set, forward should actually do the reverse.
    let lon = 0.1_f64;
    let lat = 1.4_f64;
    let pts = PointSet::from_rows([[lon, lat]]);

    // Get the forward result from the uninverted mapping.
    let wcs2 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let fwd = wcs2.transform(&pts, true).unwrap();

    // The inverted mapping's "forward" should give the reverse result.
    let inv_fwd = wcs.transform(&fwd, true).unwrap();
    assert_close(inv_fwd.point(0)[0], lon, 1e-10, "inverted forward -> lon");
    assert_close(inv_fwd.point(0)[1], lat, 1e-10, "inverted forward -> lat");
}

// ===========================================================================
// PV parameter setting
// ===========================================================================

#[test]
fn set_pv_parameter() {
    let mut wcs = WcsMap::new(2, WcsProjection::Azp, 1, 2).unwrap();
    // Set PV for the latitude axis (axis 2 = latax).
    wcs.set_pv(2, 1, 45.0).unwrap();

    // Should still produce valid results.
    let lon = 0.1_f64;
    let lat = 1.0_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = wcs.transform(&pts, true).unwrap();
    let back = wcs.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "AZP+PV lon round-trip");
    assert_close(back.point(0)[1], lat, 1e-10, "AZP+PV lat round-trip");
}

#[test]
fn set_pv_wrong_axis_rejected() {
    let mut wcs = WcsMap::new(4, WcsProjection::Tan, 2, 3).unwrap();
    // Axis 1 is neither lon nor lat.
    assert!(wcs.set_pv(1, 0, 1.0).is_err());
    // Axis 4 is neither lon nor lat.
    assert!(wcs.set_pv(4, 0, 1.0).is_err());
    // Axis 0 is out of range.
    assert!(wcs.set_pv(0, 0, 1.0).is_err());
    // Axis 5 is out of range.
    assert!(wcs.set_pv(5, 0, 1.0).is_err());
}

// ===========================================================================
// Clone
// ===========================================================================

#[test]
fn clone_independence() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let wcs2 = wcs.clone();

    // Both should produce the same result.
    let lon = 0.1_f64;
    let lat = 1.4_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out1 = wcs.transform(&pts, true).unwrap();
    let out2 = wcs2.transform(&pts, true).unwrap();
    assert_eq!(out1.point(0), out2.point(0));
}

// ===========================================================================
// Simplified
// ===========================================================================

#[test]
fn simplified_produces_working_clone() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let s = wcs.simplified().unwrap();

    let lon = 0.1_f64;
    let lat = 1.4_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = s.transform(&pts, true).unwrap();
    let back = s.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "simplified lon");
    assert_close(back.point(0)[1], lat, 1e-10, "simplified lat");
}

// ===========================================================================
// Rate
// ===========================================================================

#[test]
fn rate_numeric() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    // Near the pole, the rate should be finite.
    let r = wcs.rate(&[0.0, 1.4], 2, 2).unwrap();
    assert!(r.is_finite(), "rate should be finite, got {r}");
}

// ===========================================================================
// Serialisation round-trip
// ===========================================================================

#[test]
fn serialisation_round_trip_tan() {
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let text = to_native_string(&AnyObject::WcsMap(wcs)).unwrap();

    // Structural markers.
    assert!(text.contains("Begin WcsMap"), "missing Begin WcsMap");
    assert!(text.contains("End WcsMap"), "missing End WcsMap");
    assert!(text.contains("Type"), "missing Type attribute");
    assert!(text.contains("TAN"), "missing TAN projection code");

    // Deserialise and verify.
    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);

    // Functional test.
    let lon = 0.1_f64;
    let lat = 1.4_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = mapping.transform(&pts, true).unwrap();
    let back = mapping.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "restored lon");
    assert_close(back.point(0)[1], lat, 1e-10, "restored lat");
}

#[test]
fn serialisation_round_trip_non_default_axes() {
    let wcs = WcsMap::new(4, WcsProjection::Car, 3, 4).unwrap();
    let text = to_native_string(&AnyObject::WcsMap(wcs)).unwrap();

    // Should write WcsAx1 and WcsAx2 since they differ from defaults.
    assert!(text.contains("WcsAx1"), "missing WcsAx1");
    assert!(text.contains("WcsAx2"), "missing WcsAx2");
    assert!(text.contains("CAR"), "missing CAR projection code");

    let restored = from_native_string(&text).unwrap();
    let obj = match restored {
        AnyObject::WcsMap(w) => w,
        _ => panic!("expected WcsMap"),
    };
    assert_eq!(obj.lonax(), 3);
    assert_eq!(obj.latax(), 4);
    assert_eq!(obj.projection(), WcsProjection::Car);
}

#[test]
fn serialisation_round_trip_with_pv() {
    let mut wcs = WcsMap::new(2, WcsProjection::Azp, 1, 2).unwrap();
    wcs.set_pv(2, 1, 45.0).unwrap();
    let text = to_native_string(&AnyObject::WcsMap(wcs)).unwrap();

    // Should contain PV parameter.
    assert!(text.contains("PV2_1"), "missing PV2_1");

    let restored = from_native_string(&text).unwrap();
    let obj = match restored {
        AnyObject::WcsMap(w) => w,
        _ => panic!("expected WcsMap"),
    };

    // Verify functional equivalence with the PV param active.
    let lon = 0.1_f64;
    let lat = 1.0_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = obj.transform(&pts, true).unwrap();
    let back = obj.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "PV restored lon");
    assert_close(back.point(0)[1], lat, 1e-10, "PV restored lat");
}

#[test]
fn serialisation_inverted() {
    let mut wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    wcs.invert();
    let text = to_native_string(&AnyObject::WcsMap(wcs)).unwrap();
    assert!(text.contains("Invert"), "missing Invert attribute");

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();
    assert!(mapping.is_inverted(), "restored mapping should be inverted");
}

// ===========================================================================
// Multiple projection types round-trip
// ===========================================================================

#[test]
fn multiple_projections_round_trip() {
    let projs = [
        WcsProjection::Tan,
        WcsProjection::Car,
        WcsProjection::Ait,
        WcsProjection::Arc,
        WcsProjection::Stg,
        WcsProjection::Mer,
        WcsProjection::Zea,
        WcsProjection::Mol,
        WcsProjection::Sin,
        WcsProjection::Sfl,
    ];

    for proj in projs {
        let wcs = WcsMap::new(2, proj, 1, 2).unwrap();
        // Use a point safely within all projections' valid range.
        let lon = 0.05_f64;
        let lat = 0.8_f64;
        let pts = PointSet::from_rows([[lon, lat]]);
        let out = wcs.transform(&pts, true).unwrap();
        let back = wcs.transform(&out, false).unwrap();
        assert_close(
            back.point(0)[0],
            lon,
            1e-9,
            &format!("{} lon round-trip", proj.code()),
        );
        assert_close(
            back.point(0)[1],
            lat,
            1e-9,
            &format!("{} lat round-trip", proj.code()),
        );
    }
}

// ===========================================================================
// NCP (deprecated alias for SIN)
// ===========================================================================

#[test]
fn ncp_projection() {
    // NCP is a deprecated alias for SIN.
    let wcs = WcsMap::new(2, WcsProjection::Ncp, 1, 2).unwrap();
    assert_eq!(wcs.projection(), WcsProjection::Ncp);

    // It should still work as a projection.
    let lon = 0.05_f64;
    let lat = 1.2_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = wcs.transform(&pts, true).unwrap();
    let back = wcs.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "NCP lon round-trip");
    assert_close(back.point(0)[1], lat, 1e-10, "NCP lat round-trip");
}

// ===========================================================================
// GLS (deprecated alias for SFL)
// ===========================================================================

#[test]
fn gls_projection() {
    // GLS is a deprecated alias for SFL.
    let wcs = WcsMap::new(2, WcsProjection::Gls, 1, 2).unwrap();
    assert_eq!(wcs.projection(), WcsProjection::Gls);

    let lon = 0.3_f64;
    let lat = 0.5_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = wcs.transform(&pts, true).unwrap();
    let back = wcs.transform(&out, false).unwrap();
    assert_close(back.point(0)[0], lon, 1e-10, "GLS lon round-trip");
    assert_close(back.point(0)[1], lat, 1e-10, "GLS lat round-trip");
}

// ===========================================================================
// NatLat / NatLon accessors
// ===========================================================================

#[test]
fn wcsmap_natlat_natlon() {
    // TAN projection: native latitude of fiducial = 90 degrees = pi/2
    let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let nat_lat = wcs.nat_lat();
    assert!(
        (nat_lat - std::f64::consts::FRAC_PI_2).abs() < 1e-10,
        "TAN nat_lat should be pi/2, got {nat_lat}"
    );
    // Native longitude of fiducial = 0 for zenithal projections.
    let nat_lon = wcs.nat_lon();
    assert!(
        nat_lon.abs() < 1e-10,
        "TAN nat_lon should be 0, got {nat_lon}"
    );

    // CAR (cylindrical) projection: native latitude of fiducial = 0
    let wcs_car = WcsMap::new(2, WcsProjection::Car, 1, 2).unwrap();
    let nat_lat_car = wcs_car.nat_lat();
    assert!(
        nat_lat_car.abs() < 1e-10,
        "CAR nat_lat should be 0, got {nat_lat_car}"
    );
    let nat_lon_car = wcs_car.nat_lon();
    assert!(
        nat_lon_car.abs() < 1e-10,
        "CAR nat_lon should be 0, got {nat_lon_car}"
    );

    // ARC (zenithal equidistant): same as TAN - theta0=90 degrees
    let wcs_arc = WcsMap::new(2, WcsProjection::Arc, 1, 2).unwrap();
    let nat_lat_arc = wcs_arc.nat_lat();
    assert!(
        (nat_lat_arc - std::f64::consts::FRAC_PI_2).abs() < 1e-10,
        "ARC nat_lat should be pi/2, got {nat_lat_arc}"
    );
}

// ===========================================================================
// Equality and try_merge with PV parameters
// ===========================================================================

#[test]
fn wcsmap_equal_different_params() {
    let mut w1 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    // Same projection, same axes => equal
    assert!(w1.equal(&w2));

    // Set different PV parameters
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.7).unwrap();
    // Different PV => NOT equal
    assert!(!w1.equal(&w2));
}

#[test]
fn wcsmap_equal_same_params() {
    let mut w1 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.5).unwrap();
    // Same PV => equal
    assert!(w1.equal(&w2));
}

#[test]
fn wcsmap_no_cancel_different_params() {
    let mut w1 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.7).unwrap();
    // Should NOT cancel (different params) even though opposite invert directions
    let result = w1.try_merge(&w2, true, false, true);
    assert!(result.is_none());
}

#[test]
fn wcsmap_cancel_same_params() {
    let mut w1 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.5).unwrap();
    // Same params + opposite invert directions => should cancel to UnitMap
    let result = w1.try_merge(&w2, true, true, false);
    assert!(result.is_some());
}

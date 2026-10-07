//! Tests for WcsMap — FITS-WCS sky projection mapping.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::mappings::{CmpMap, Mapping, PermMap, WcsMap, WcsProjection};
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
// LonCheck: inverse-transform out-of-domain clipping (wcsmap.c Map:2878,
// LongRange:2597). Expected values captured from the C oracle
// (reference/starlink-ast) via a probe over these exact (x,y) inputs.
// ===========================================================================

// AIT at (x,y) = (3.0, 0.0) radians: with factor = 1.0 (TPNTan defaults to 1)
// the value handed to the wcslib reverse projection is 3.0 rad expressed in
// degrees, i.e. 171.887 deg. The AIT domain test computes
// u = 1 - x*x*w[2] = 0.4375, which is below 0.5 - tol, so astAITrev returns
// status 2 (outside the physical domain) -> BAD. This is independent of the
// LonCheck gate: the projection itself rejects the point before the longitude
// range test is even reached, so both LonCheck on and off yield BAD.
//
// (An older wcslib used a u < 0 domain test that accepted this point and
// returned lon = 3.392248 rad; the current AST source, which this port
// mirrors, uses the u < 0.5 - tol test and rejects it.)
#[test]
fn ait_inverse_out_of_range_clipped_when_loncheck_on() {
    let wcs = WcsMap::new(2, WcsProjection::Ait, 1, 2).unwrap();
    let pts = PointSet::from_rows([[3.0, 0.0]]);
    let out = wcs.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn ait_inverse_out_of_range_bad_regardless_of_loncheck_off() {
    let mut wcs = WcsMap::new(2, WcsProjection::Ait, 1, 2).unwrap();
    wcs.set_lon_check(false);
    let pts = PointSet::from_rows([[3.0, 0.0]]);
    let out = wcs.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

// CAR at (x,y) = (3.5, 0.0) radians: longitude is 200.5 deg, outside
// [-180,180]. Clipped to BAD with LonCheck on; passed through unchanged with
// LonCheck off (CAR longitude wraps and its inverse is identity in longitude).
#[test]
fn car_inverse_out_of_range_clipped_when_loncheck_on() {
    let wcs = WcsMap::new(2, WcsProjection::Car, 1, 2).unwrap();
    let pts = PointSet::from_rows([[3.5, 0.0]]);
    let out = wcs.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn car_inverse_out_of_range_kept_when_loncheck_off() {
    let mut wcs = WcsMap::new(2, WcsProjection::Car, 1, 2).unwrap();
    wcs.set_lon_check(false);
    let pts = PointSet::from_rows([[3.5, 0.0]]);
    let out = wcs.transform(&pts, false).unwrap();
    assert_close(out.point(0)[0], 3.5, 1e-12, "car lon (loncheck off)");
    assert_close(out.point(0)[1], 0.0, 1e-12, "car lat (loncheck off)");
}

// ZEA at (x,y) = (3.0, 0.0): the inverse is undefined (outside the projection
// disc), so wcslib returns bad regardless of LonCheck — this exercises the
// projection-Err branch rather than the longitude gate.
#[test]
fn zea_inverse_off_disc_bad_regardless_of_loncheck() {
    for loncheck in [true, false] {
        let mut wcs = WcsMap::new(2, WcsProjection::Zea, 1, 2).unwrap();
        wcs.set_lon_check(loncheck);
        let pts = PointSet::from_rows([[3.0, 0.0]]);
        let out = wcs.transform(&pts, false).unwrap();
        assert_eq!(out.point(0)[0], BAD_VALUE, "zea lon loncheck={loncheck}");
        assert_eq!(out.point(0)[1], BAD_VALUE, "zea lat loncheck={loncheck}");
    }
}

// In-domain inverse points are unaffected by the gate: AIT at the origin
// round-trips to (0,0) with LonCheck on.
#[test]
fn ait_inverse_in_domain_unaffected_by_loncheck() {
    let wcs = WcsMap::new(2, WcsProjection::Ait, 1, 2).unwrap();
    let pts = PointSet::from_rows([[0.0, 0.0]]);
    let out = wcs.transform(&pts, false).unwrap();
    assert_close(out.point(0)[0], 0.0, 1e-12, "ait origin lon");
    assert_close(out.point(0)[1], 0.0, 1e-12, "ait origin lat");
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
    let s = <dyn Mapping>::simplify(&wcs).unwrap();

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
// NCP (no projection functions)
// ===========================================================================

#[test]
fn ncp_projection() {
    // C has no projection functions for NCP, so a WcsMap made as NCP cannot
    // transform in either direction (wcsmap.c `Map` returns 1, which
    // `Transform` reports as AST__WCSTY). FITS readers turn an NCP header into
    // SIN with parameters instead.
    let wcs = WcsMap::new(2, WcsProjection::Ncp, 1, 2).unwrap();
    assert_eq!(wcs.projection(), WcsProjection::Ncp);

    let pts = PointSet::from_rows([[0.05, 1.2]]);
    for forward in [true, false] {
        let err = wcs.transform(&pts, forward).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Wcsty, "forward={forward}");
    }
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
    // -ZPN, not -TAN: these compare a `PV2_1` value, and only a projection
    // whose `mxpar` reaches 1 accepts that index (C `SetPV`, wcsmap.c:3884).
    let mut w1 = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
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
    // -ZPN, not -TAN: these compare a `PV2_1` value, and only a projection
    // whose `mxpar` reaches 1 accepts that index (C `SetPV`, wcsmap.c:3884).
    let mut w1 = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.5).unwrap();
    // Same PV => equal
    assert!(w1.equal(&w2));
}

#[test]
fn wcsmap_no_cancel_different_params() {
    // -ZPN, not -TAN: these compare a `PV2_1` value, and only a projection
    // whose `mxpar` reaches 1 accepts that index (C `SetPV`, wcsmap.c:3884).
    let mut w1 = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.7).unwrap();
    // Should NOT cancel (different params) even though opposite invert directions
    let result = w1.try_merge(&w2, true, false, true);
    assert!(result.is_none());
}

#[test]
fn wcsmap_cancel_same_params() {
    // -ZPN, not -TAN: these compare a `PV2_1` value, and only a projection
    // whose `mxpar` reaches 1 accepts that index (C `SetPV`, wcsmap.c:3884).
    let mut w1 = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    let mut w2 = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    w1.set_pv(2, 1, 0.5).unwrap();
    w2.set_pv(2, 1, 0.5).unwrap();
    // Same params + opposite invert directions => should cancel to UnitMap
    let result = w1.try_merge(&w2, true, true, false);
    assert!(result.is_some());
}

// ===========================================================================
// WcsMap + PermMap swap simplification
// ===========================================================================

#[test]
fn wcsmap_perm_swap_cancel_simplify() {
    // Fixture: WcsMap(TAN,1,2)_fwd ; PermMap(swap) ; WcsMap(TAN,2,1)_inv
    // Expected simplification: PermMap(swap)
    let fixture_dir = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/simplify"
    ));
    let input_text =
        std::fs::read_to_string(fixture_dir.join("wcsmap_perm_swap_cancel.map")).unwrap();
    let ref_text =
        std::fs::read_to_string(fixture_dir.join("wcsmap_perm_swap_cancel.simp")).unwrap();
    let input_obj = from_native_string(&input_text).unwrap();
    let mapping = input_obj.into_mapping().unwrap();
    let ref_obj = from_native_string(&ref_text).unwrap();
    let ref_mapping = ref_obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();

    // The result should be a PermMap.
    assert!(
        simplified.as_any().is::<PermMap>(),
        "Expected PermMap after simplification, got nin={} nout={} is_CmpMap={}",
        simplified.nin(),
        simplified.nout(),
        simplified.as_any().is::<CmpMap>()
    );

    // Check structural equality with C reference.
    assert!(
        simplified.equal(ref_mapping.as_ref()),
        "Simplified PermMap not structurally equal to C reference. inv_s={} inv_r={} nin_s={} nin_r={}",
        simplified.is_inverted(),
        ref_mapping.is_inverted(),
        simplified.nin(),
        ref_mapping.nin(),
    );
}

#[test]
fn wcsmap_perm_swap_simplify_constants() {
    // Fixture: WcsMap(TAN,1,2,3axes)_fwd ; PermMap(3→1, with constants)
    // Expected: PermMap with transformed constants (WcsMap → UnitMap).
    let fixture_dir = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/simplify"
    ));
    let input_text =
        std::fs::read_to_string(fixture_dir.join("wcsmap_perm_swap_simplify.map")).unwrap();
    let input_obj = from_native_string(&input_text).unwrap();
    let mapping = input_obj.into_mapping().unwrap();

    let simplified = mapping.simplify().unwrap();

    // The result should be a PermMap.
    assert!(
        simplified.as_any().is::<PermMap>(),
        "Expected PermMap after simplification, got nin={} nout={}",
        simplified.nin(),
        simplified.nout(),
    );
    assert_eq!(simplified.nin(), 3);
    assert_eq!(simplified.nout(), 1);
}

#[test]
fn wcsmap_absorbs_adjacent_permmap() {
    // WcsMap(TAN, lonax=1, latax=2) followed by PermMap that swaps axes [2,1].
    // After simplification via the swap mechanism, plus cancellation with an
    // inverse WcsMap, the result should simplify.
    let wcs_fwd = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let mut wcs_inv = WcsMap::new(2, WcsProjection::Tan, 2, 1).unwrap();
    wcs_inv.invert();

    // Build: WcsMap(1,2)_fwd ; PermMap(swap) ; WcsMap(2,1)_inv
    use ast_core::mappings::CmpMap;
    use ast_core::mappings::Composition;
    let inner = CmpMap::new(Box::new(pm), Box::new(wcs_inv), Composition::Series).unwrap();
    let full = CmpMap::new(Box::new(wcs_fwd), Box::new(inner), Composition::Series).unwrap();

    let simplified = <dyn Mapping>::simplify(&full).unwrap();

    // Should simplify to just a PermMap(swap).
    assert!(
        simplified.as_any().is::<PermMap>(),
        "Expected PermMap, got nin={} nout={} is_CmpMap={}",
        simplified.nin(),
        simplified.nout(),
        simplified.as_any().is::<CmpMap>()
    );

    // Verify the PermMap's functional equivalence: should swap axes.
    let pts = PointSet::from_rows([[0.1, 1.2]]);
    let out = simplified.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 1.2, 1e-10, "swap axis 0");
    assert_close(out.point(0)[1], 0.1, 1e-10, "swap axis 1");
}

// ===========================================================================
// Unknown (AST__WCSBAD) projection — behaves as an identity / UnitMap
// ===========================================================================

/// A WcsMap with an `Unknown` projection (C `AST__WCSBAD`) has no real
/// projection: C's Transform skips the projection entirely, leaving the
/// lon/lat axes as a straight copy (wcsmap.c:4247), and its MapMerge replaces
/// it with a UnitMap (wcsmap.c:3087). `NatLon`/`NatLat` are 0. The Rust port
/// previously rebuilt a CAR projection as a placeholder; this confirms the
/// observable behaviour is the C identity, not CAR's projection.
#[test]
fn unknown_projection_is_identity() {
    let wm = WcsMap::new(2, WcsProjection::Unknown, 1, 2).unwrap();

    // Transform is a pure pass-through on every axis (CAR would warp these).
    let pts = PointSet::from_rows([[0.3, 0.7], [-1.2, 0.45]]);
    let fwd = wm.transform(&pts, true).unwrap();
    assert_close(fwd.point(0)[0], 0.3, 1e-12, "unknown fwd lon");
    assert_close(fwd.point(0)[1], 0.7, 1e-12, "unknown fwd lat");
    assert_close(fwd.point(1)[0], -1.2, 1e-12, "unknown fwd lon 2");
    assert_close(fwd.point(1)[1], 0.45, 1e-12, "unknown fwd lat 2");

    // Inverse is identity too.
    let inv = wm.transform(&pts, false).unwrap();
    assert_close(inv.point(0)[0], 0.3, 1e-12, "unknown inv lon");
    assert_close(inv.point(0)[1], 0.7, 1e-12, "unknown inv lat");

    // Reference point is 0/0 (C GetNatLon = 0, GetNatLat = WCSBAD theta0 = 0).
    assert_eq!(wm.nat_lon(), 0.0);
    assert_eq!(wm.nat_lat(), 0.0);

    // Simplifies to a UnitMap (wcsmap.c:3087).
    let simp = <dyn Mapping>::simplify(&wm).unwrap();
    assert!(
        simp.as_any().is::<ast_core::mappings::UnitMap>(),
        "Unknown WcsMap should simplify to a UnitMap"
    );
}

// ===========================================================================
// Per-axis projection-parameter limits
// ===========================================================================

/// `astGetPVMax` reports the longitude axis's limit from the `PrjInfo`
/// table's `mxpar2` column and the latitude axis's from `mxpar`
/// (wcsmap.c:2205-2210), and -1 — here `None` — for any other axis.
#[test]
fn pv_max_is_per_axis() {
    // 2-axis TAN: lon = axis 1, lat = axis 2.
    let map = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    assert_eq!(map.pv_max(1), Some(4)); // mxpar2
    assert_eq!(map.pv_max(2), Some(0)); // mxpar
    // A third axis is neither the longitude nor the latitude axis.
    let map3 = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    assert_eq!(map3.pv_max(3), None);
    assert_eq!(map3.pv_max(0), None);
    // The limit follows the *declared* projection: NCP keeps its own even
    // though it transforms through SIN (C `FindPrjData( this->type )`).
    let ncp = WcsMap::new(2, WcsProjection::Ncp, 1, 2).unwrap();
    assert_eq!(ncp.pv_max(2), Some(2));
}

/// C `SetPV` raises `AST__AXIIN` for a parameter index the projection cannot
/// use (wcsmap.c:3884-3890). `-TAN` accepts only `PV<lat>_0`, and 4 on the
/// longitude axis.
#[test]
fn set_pv_rejects_an_index_the_projection_cannot_use() {
    use ast_core::error::ErrorCode;

    let mut map = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    assert!(map.set_pv(2, 0, 0.0).is_ok(), "PV2_0 is legal for TAN");
    let err = map
        .set_pv(2, 1, 0.0)
        .expect_err("PV2_1 is not legal for TAN");
    assert_eq!(err.code(), ErrorCode::Axiin);
    // The longitude axis accepts up to m = 4.
    assert!(map.set_pv(1, 4, 0.0).is_ok());
    assert!(map.set_pv(1, 5, 0.0).is_err());
    // A rejected parameter is not stored.
    assert_eq!(map.get_pv(2, 1), None);

    // ZPN's latitude axis accepts the full polynomial range.
    let mut zpn = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    assert!(zpn.set_pv(2, 99, 0.0).is_ok());
    assert!(zpn.set_pv(2, 100, 0.0).is_ok());
    assert!(zpn.set_pv(2, 101, 0.0).is_err());
}

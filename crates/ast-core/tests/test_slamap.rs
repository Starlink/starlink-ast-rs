//! Tests for SlaMap — celestial coordinate conversion chain.

use std::f64::consts::PI;

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, SlaCvtType, SlaMap};
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

fn deg2rad(d: f64) -> f64 {
    d * PI / 180.0
}

fn rad2deg(r: f64) -> f64 {
    r * 180.0 / PI
}

/// Normalize angle to [0, 2*pi).
fn normalize_angle(a: f64) -> f64 {
    let twopi = 2.0 * PI;
    let mut r = a % twopi;
    if r < 0.0 {
        r += twopi;
    }
    r
}

/// Assert two angles are close (modulo 2*pi).
fn assert_angle_close(actual: f64, expected: f64, tol: f64, label: &str) {
    let diff = normalize_angle(actual - expected);
    let diff = if diff > PI { 2.0 * PI - diff } else { diff };
    assert!(
        diff < tol,
        "{label}: expected {expected}, got {actual} (angular diff {diff})",
    );
}

// ===========================================================================
// Construction
// ===========================================================================

#[test]
fn new_empty() {
    let sla = SlaMap::new();
    assert_eq!(sla.nin(), 2);
    assert_eq!(sla.nout(), 2);
    assert!(sla.is_empty());
    assert_eq!(sla.len(), 0);
    assert!(!sla.is_inverted());
}

#[test]
fn add_steps() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]);
    sla.add(SlaCvtType::Eqgal, &[]);
    assert_eq!(sla.len(), 2);
    assert!(!sla.is_empty());
}

// ===========================================================================
// SlaCvtType methods
// ===========================================================================

#[test]
fn cvt_nargs() {
    assert_eq!(SlaCvtType::Galeq.nargs(), 0);
    assert_eq!(SlaCvtType::Eqgal.nargs(), 0);
    assert_eq!(SlaCvtType::Galsup.nargs(), 0);
    assert_eq!(SlaCvtType::Supgal.nargs(), 0);
    assert_eq!(SlaCvtType::Addet.nargs(), 1);
    assert_eq!(SlaCvtType::Fk45z.nargs(), 1);
    assert_eq!(SlaCvtType::Ecleq.nargs(), 1);
    assert_eq!(SlaCvtType::De2h.nargs(), 1);
    assert_eq!(SlaCvtType::Prec.nargs(), 2);
    assert_eq!(SlaCvtType::Map.nargs(), 2);
}

#[test]
fn cvt_code_round_trip() {
    let types = [
        SlaCvtType::Addet,
        SlaCvtType::Subet,
        SlaCvtType::Prebn,
        SlaCvtType::Prec,
        SlaCvtType::Fk45z,
        SlaCvtType::Fk54z,
        SlaCvtType::Amp,
        SlaCvtType::Map,
        SlaCvtType::Ecleq,
        SlaCvtType::Eqecl,
        SlaCvtType::Galeq,
        SlaCvtType::Eqgal,
        SlaCvtType::Galsup,
        SlaCvtType::Supgal,
        SlaCvtType::Hfk5z,
        SlaCvtType::Fk5hz,
        SlaCvtType::Dh2e,
        SlaCvtType::De2h,
    ];
    for t in &types {
        let code = t.code_string();
        let parsed = SlaCvtType::from_code(code).unwrap();
        assert_eq!(parsed, *t, "code round-trip failed for {code}");
    }
}

#[test]
fn cvt_from_code_case_insensitive() {
    assert_eq!(SlaCvtType::from_code("prec"), Some(SlaCvtType::Prec));
    assert_eq!(SlaCvtType::from_code("Eqgal"), Some(SlaCvtType::Eqgal));
    assert_eq!(SlaCvtType::from_code("GALEQ"), Some(SlaCvtType::Galeq));
}

#[test]
fn cvt_from_int() {
    assert_eq!(SlaCvtType::from_int(1), Some(SlaCvtType::Addet));
    assert_eq!(SlaCvtType::from_int(4), Some(SlaCvtType::Prec));
    assert_eq!(SlaCvtType::from_int(11), Some(SlaCvtType::Galeq));
    assert_eq!(SlaCvtType::from_int(26), Some(SlaCvtType::De2h));
    assert_eq!(SlaCvtType::from_int(99), None);
}

#[test]
fn cvt_inverse_pairs() {
    assert_eq!(SlaCvtType::Addet.inverse(), SlaCvtType::Subet);
    assert_eq!(SlaCvtType::Subet.inverse(), SlaCvtType::Addet);
    assert_eq!(SlaCvtType::Prec.inverse(), SlaCvtType::Prec);
    assert_eq!(SlaCvtType::Prebn.inverse(), SlaCvtType::Prebn);
    assert_eq!(SlaCvtType::Fk45z.inverse(), SlaCvtType::Fk54z);
    assert_eq!(SlaCvtType::Fk54z.inverse(), SlaCvtType::Fk45z);
    assert_eq!(SlaCvtType::Amp.inverse(), SlaCvtType::Map);
    assert_eq!(SlaCvtType::Map.inverse(), SlaCvtType::Amp);
    assert_eq!(SlaCvtType::Ecleq.inverse(), SlaCvtType::Eqecl);
    assert_eq!(SlaCvtType::Eqecl.inverse(), SlaCvtType::Ecleq);
    assert_eq!(SlaCvtType::Galeq.inverse(), SlaCvtType::Eqgal);
    assert_eq!(SlaCvtType::Eqgal.inverse(), SlaCvtType::Galeq);
    assert_eq!(SlaCvtType::Galsup.inverse(), SlaCvtType::Supgal);
    assert_eq!(SlaCvtType::Supgal.inverse(), SlaCvtType::Galsup);
    assert_eq!(SlaCvtType::Hfk5z.inverse(), SlaCvtType::Fk5hz);
    assert_eq!(SlaCvtType::Fk5hz.inverse(), SlaCvtType::Hfk5z);
    assert_eq!(SlaCvtType::Dh2e.inverse(), SlaCvtType::De2h);
    assert_eq!(SlaCvtType::De2h.inverse(), SlaCvtType::Dh2e);
}

// ===========================================================================
// 1. Galactic -> FK5: galactic centre known position
// ===========================================================================

#[test]
fn galactic_to_fk5() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Galeq, &[]);

    // Galactic centre: l=0, b=0
    let pts = PointSet::from_rows([[0.0_f64, 0.0_f64]]);
    let out = sla.transform(&pts, true).unwrap();
    // Normalize RA to [0, 360) since dcc2s can return [-pi, pi]
    let ra_deg = rad2deg(normalize_angle(out.point(0)[0]));
    let dec_deg = rad2deg(out.point(0)[1]);

    // Expected: RA ~ 266.4 deg, Dec ~ -28.9 deg
    assert_close(ra_deg, 266.405, 0.01, "galactic centre RA");
    assert_close(dec_deg, -28.936, 0.01, "galactic centre Dec");
}

// ===========================================================================
// 2. FK5 -> Galactic -> FK5 round-trip
// ===========================================================================

#[test]
fn fk5_gal_fk5_round_trip() {
    let mut fwd = SlaMap::new();
    fwd.add(SlaCvtType::Eqgal, &[]);

    let ra = 1.5_f64;
    let dec = 0.3_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    // Forward: FK5 -> Galactic
    let gal = fwd.transform(&pts, true).unwrap();

    // Inverse: Galactic -> FK5
    let back = fwd.transform(&gal, false).unwrap();

    assert_close(back.point(0)[0], ra, 1e-10, "ra round-trip");
    assert_close(back.point(0)[1], dec, 1e-10, "dec round-trip");
}

// ===========================================================================
// 3. Precession round-trip
// ===========================================================================

#[test]
fn precession_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]);

    let ra = 1.0_f64;
    let dec = 0.5_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    // Forward: precess from J2000 to J2020
    let precessed = sla.transform(&pts, true).unwrap();

    // Precession should change coordinates noticeably
    assert!(
        (precessed.point(0)[0] - ra).abs() > 1e-6,
        "precession should change RA"
    );

    // Inverse: precess back from J2020 to J2000
    let back = sla.transform(&precessed, false).unwrap();

    assert_close(back.point(0)[0], ra, 1e-10, "ra prec round-trip");
    assert_close(back.point(0)[1], dec, 1e-10, "dec prec round-trip");
}

// ===========================================================================
// 4. Multi-step chain: FK4 -> FK5 -> Galactic
// ===========================================================================

#[test]
fn multi_step_fk4_fk5_galactic() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Fk45z, &[1950.0]);
    sla.add(SlaCvtType::Eqgal, &[]);

    // A known FK4 coordinate
    let ra_fk4 = deg2rad(180.0);
    let dec_fk4 = deg2rad(45.0);
    let pts = PointSet::from_rows([[ra_fk4, dec_fk4]]);

    let out = sla.transform(&pts, true).unwrap();
    // Should produce valid galactic coordinates
    let gl = out.point(0)[0];
    let gb = out.point(0)[1];
    assert!(gl.is_finite(), "galactic longitude should be finite");
    assert!(gb.is_finite(), "galactic latitude should be finite");

    // Round-trip through inverse
    let back = sla.transform(&out, false).unwrap();
    assert_angle_close(back.point(0)[0], ra_fk4, 1e-6, "fk4 ra round-trip");
    assert_close(back.point(0)[1], dec_fk4, 1e-6, "fk4 dec round-trip");
}

// ===========================================================================
// 5. Inversion: forward then inverse = identity
// ===========================================================================

#[test]
fn inversion_identity() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]);
    sla.add(SlaCvtType::Eqgal, &[]);

    let ra = 2.0_f64;
    let dec = -0.3_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    let fwd = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&fwd, false).unwrap();

    assert_close(back.point(0)[0], ra, 1e-10, "identity ra");
    assert_close(back.point(0)[1], dec, 1e-10, "identity dec");
}

#[test]
fn invert_flag() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]);

    let ra = 1.0_f64;
    let dec = 0.5_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    // Forward transform
    let gal = sla.transform(&pts, true).unwrap();

    // Now invert the mapping
    sla.invert();
    assert!(sla.is_inverted());

    // Forward with inverted flag = inverse of original
    let back = sla.transform(&gal, true).unwrap();
    assert_close(back.point(0)[0], ra, 1e-10, "inverted ra");
    assert_close(back.point(0)[1], dec, 1e-10, "inverted dec");
}

// ===========================================================================
// 6. Empty SlaMap = identity
// ===========================================================================

#[test]
fn empty_slamap_is_identity() {
    let sla = SlaMap::new();

    let lon = 1.23_f64;
    let lat = 0.45_f64;
    let pts = PointSet::from_rows([[lon, lat]]);

    let out = sla.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], lon, 1e-15, "identity lon");
    assert_close(out.point(0)[1], lat, 1e-15, "identity lat");

    let out_inv = sla.transform(&pts, false).unwrap();
    assert_close(out_inv.point(0)[0], lon, 1e-15, "identity inv lon");
    assert_close(out_inv.point(0)[1], lat, 1e-15, "identity inv lat");
}

// ===========================================================================
// 7. BAD_VALUE propagation
// ===========================================================================

#[test]
fn bad_value_propagation() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Galeq, &[]);

    // BAD in first coord
    let pts = PointSet::from_rows([[BAD_VALUE, 0.5]]);
    let out = sla.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);

    // BAD in second coord
    let pts2 = PointSet::from_rows([[1.0, BAD_VALUE]]);
    let out2 = sla.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0)[0], BAD_VALUE);
    assert_eq!(out2.point(0)[1], BAD_VALUE);

    // NaN in input (also treated as BAD)
    let pts3 = PointSet::from_rows([[f64::NAN, 0.5]]);
    let out3 = sla.transform(&pts3, true).unwrap();
    assert_eq!(out3.point(0)[0], BAD_VALUE);
    assert_eq!(out3.point(0)[1], BAD_VALUE);
}

// ===========================================================================
// 8. Serialisation round-trip
// ===========================================================================

#[test]
fn serialisation_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]);
    sla.add(SlaCvtType::Eqgal, &[]);

    let obj = AnyObject::SlaMap(sla.clone());
    let text = to_native_string(&obj).unwrap();

    let restored = from_native_string(&text).unwrap();
    let sla2 = match restored {
        AnyObject::SlaMap(m) => m,
        other => panic!("expected SlaMap, got {other:?}"),
    };

    // Verify restored mapping produces the same results
    let ra = 1.5_f64;
    let dec = 0.3_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    let out1 = sla.transform(&pts, true).unwrap();
    let out2 = sla2.transform(&pts, true).unwrap();

    assert_close(out2.point(0)[0], out1.point(0)[0], 1e-15, "serialised lon");
    assert_close(out2.point(0)[1], out1.point(0)[1], 1e-15, "serialised lat");
}

// ===========================================================================
// 9. Text structure
// ===========================================================================

#[test]
fn text_structure() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]);
    sla.add(SlaCvtType::Eqgal, &[]);

    let obj = AnyObject::SlaMap(sla);
    let text = to_native_string(&obj).unwrap();

    assert!(text.contains("Begin SlaMap"), "should contain Begin SlaMap");
    assert!(text.contains("Nsla"), "should contain Nsla");
    assert!(text.contains("PREC"), "should contain PREC");
    assert!(text.contains("EQGAL"), "should contain EQGAL");
    assert!(text.contains("End SlaMap"), "should contain End SlaMap");
}

// ===========================================================================
// Additional conversion type tests
// ===========================================================================

#[test]
fn ecliptic_round_trip() {
    let mut sla = SlaMap::new();
    // MJD of J2000.0 = 51544.5
    sla.add(SlaCvtType::Eqecl, &[51544.5]);

    let ra = 1.0_f64;
    let dec = 0.5_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    let ecl = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&ecl, false).unwrap();

    assert_close(back.point(0)[0], ra, 1e-10, "ecliptic ra round-trip");
    assert_close(back.point(0)[1], dec, 1e-10, "ecliptic dec round-trip");
}

#[test]
fn supergalactic_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Galsup, &[]);

    let gl = 1.0_f64;
    let gb = 0.3_f64;
    let pts = PointSet::from_rows([[gl, gb]]);

    let sup = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&sup, false).unwrap();

    assert_close(back.point(0)[0], gl, 1e-10, "supergal gl round-trip");
    assert_close(back.point(0)[1], gb, 1e-10, "supergal gb round-trip");
}

#[test]
fn fk5_icrs_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Fk5hz, &[2000.0]);

    let ra = 2.0_f64;
    let dec = 0.4_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    let icrs = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&icrs, false).unwrap();

    assert_close(back.point(0)[0], ra, 1e-9, "fk5-icrs ra round-trip");
    assert_close(back.point(0)[1], dec, 1e-9, "fk5-icrs dec round-trip");
}

#[test]
fn horizon_round_trip() {
    let mut sla = SlaMap::new();
    // Observer latitude = 0.8 rad
    sla.add(SlaCvtType::De2h, &[0.8]);

    let ha = 0.5_f64;
    let dec = 0.4_f64;
    let pts = PointSet::from_rows([[ha, dec]]);

    let hor = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&hor, false).unwrap();

    assert_close(back.point(0)[0], ha, 1e-10, "horizon ha round-trip");
    assert_close(back.point(0)[1], dec, 1e-10, "horizon dec round-trip");
}

#[test]
fn aberration_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Addet, &[1984.0]);

    let rm = 1.0_f64;
    let dm = 0.5_f64;
    let pts = PointSet::from_rows([[rm, dm]]);

    let apparent = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&apparent, false).unwrap();

    assert_close(back.point(0)[0], rm, 1e-9, "aberration rm round-trip");
    assert_close(back.point(0)[1], dm, 1e-9, "aberration dm round-trip");
}

#[test]
fn mean_apparent_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Map, &[2000.0, 51544.5]);

    let rm = 2.0_f64;
    let dm = 0.3_f64;
    let pts = PointSet::from_rows([[rm, dm]]);

    let apparent = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&apparent, false).unwrap();

    assert_close(back.point(0)[0], rm, 1e-9, "map rm round-trip");
    assert_close(back.point(0)[1], dm, 1e-9, "map dm round-trip");
}

#[test]
fn bessel_newcomb_precession_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prebn, &[1950.0, 2000.0]);

    let ra = 1.0_f64;
    let dec = 0.5_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    let precessed = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&precessed, false).unwrap();

    assert_close(back.point(0)[0], ra, 1e-10, "prebn ra round-trip");
    assert_close(back.point(0)[1], dec, 1e-10, "prebn dec round-trip");
}

#[test]
fn fk4_fk5_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Fk45z, &[1984.0]);

    let ra = 1.0_f64;
    let dec = 0.5_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    let fk5 = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&fk5, false).unwrap();

    assert_close(back.point(0)[0], ra, 1e-6, "fk45z ra round-trip");
    assert_close(back.point(0)[1], dec, 1e-6, "fk45z dec round-trip");
}

// ===========================================================================
// Multiple points
// ===========================================================================

#[test]
fn multiple_points() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]);

    let pts = PointSet::from_rows([[0.0, 0.0], [1.0, 0.5], [BAD_VALUE, 0.3], [2.0, -0.4]]);

    let out = sla.transform(&pts, true).unwrap();
    assert_eq!(out.npoint(), 4);

    // Point 0 and 1 and 3 should be valid
    assert!(out.point(0)[0].is_finite());
    assert!(out.point(1)[0].is_finite());
    assert!(out.point(3)[0].is_finite());

    // Point 2 should be BAD
    assert_eq!(out.point(2)[0], BAD_VALUE);
    assert_eq!(out.point(2)[1], BAD_VALUE);
}

// ===========================================================================
// Serialisation of inverted mapping
// ===========================================================================

#[test]
fn serialise_inverted() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]);
    sla.invert();

    let obj = AnyObject::SlaMap(sla);
    let text = to_native_string(&obj).unwrap();

    let restored = from_native_string(&text).unwrap();
    let sla2 = match restored {
        AnyObject::SlaMap(m) => m,
        other => panic!("expected SlaMap, got {other:?}"),
    };
    assert!(sla2.is_inverted());
}

// ===========================================================================
// Simplified
// ===========================================================================

#[test]
fn simplified_returns_clone() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]);

    let simplified = sla.simplified().unwrap();
    assert_eq!(simplified.nin(), 2);
    assert_eq!(simplified.nout(), 2);

    let pts = PointSet::from_rows([[1.0_f64, 0.5]]);
    let out1 = sla.transform(&pts, true).unwrap();
    let out2 = simplified.transform(&pts, true).unwrap();
    assert_close(out2.point(0)[0], out1.point(0)[0], 1e-15, "simplified lon");
    assert_close(out2.point(0)[1], out1.point(0)[1], 1e-15, "simplified lat");
}

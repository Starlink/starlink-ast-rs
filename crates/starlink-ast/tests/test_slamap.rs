// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Tests for SlaMap — celestial coordinate conversion chain.

use crate::common::native_dump;
use std::f64::consts::PI;

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{Mapping, SlaCvtType, SlaMap};
use starlink_ast::{AnyObject, from_native_string};

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
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]).unwrap();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();
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
    assert_eq!(SlaCvtType::De2h.nargs(), 2);
    assert_eq!(SlaCvtType::Prec.nargs(), 2);
    assert_eq!(SlaCvtType::Map.nargs(), 2);
    assert_eq!(SlaCvtType::Hpceq.nargs(), 4);
    assert_eq!(SlaCvtType::J2000h.nargs(), 0);
    assert_eq!(SlaCvtType::Heeq.nargs(), 1);
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
// 1. Galactic -> FK5: galactic center known position
// ===========================================================================

#[test]
fn galactic_to_fk5() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Galeq, &[]).unwrap();

    // Galactic center: l=0, b=0
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
    fwd.add(SlaCvtType::Eqgal, &[]).unwrap();

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
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]).unwrap();

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
    sla.add(SlaCvtType::Fk45z, &[1950.0]).unwrap();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();

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

    // Round-trip through inverse. The FK45Z/FK54Z leg leaves a genuine ~5e-11
    // residual (see fk4_fk5_round_trip); 1e-9 sits above it with teeth.
    let back = sla.transform(&out, false).unwrap();
    assert_angle_close(back.point(0)[0], ra_fk4, 1e-9, "fk4 ra round-trip");
    assert_close(back.point(0)[1], dec_fk4, 1e-9, "fk4 dec round-trip");
}

// ===========================================================================
// 5. Inversion: forward then inverse = identity
// ===========================================================================

#[test]
fn inversion_identity() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]).unwrap();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();

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
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();

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
    sla.add(SlaCvtType::Galeq, &[]).unwrap();

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
// 8. Serialization round-trip
// ===========================================================================

#[test]
fn serialisation_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]).unwrap();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();

    let obj = AnyObject::SlaMap(sla.clone());
    let text = native_dump(&obj).unwrap();

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
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]).unwrap();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();

    let obj = AnyObject::SlaMap(sla);
    let text = native_dump(&obj).unwrap();

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
    sla.add(SlaCvtType::Eqecl, &[51544.5]).unwrap();

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
    sla.add(SlaCvtType::Galsup, &[]).unwrap();

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
    sla.add(SlaCvtType::Fk5hz, &[2000.0]).unwrap();

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
    // Observer latitude = 0.8 rad, diurnal aberration = 0.0
    sla.add(SlaCvtType::De2h, &[0.8, 0.0]).unwrap();

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
    sla.add(SlaCvtType::Addet, &[1984.0]).unwrap();

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
    sla.add(SlaCvtType::Map, &[2000.0, 51544.5]).unwrap();

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
    sla.add(SlaCvtType::Prebn, &[1950.0, 2000.0]).unwrap();

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
    sla.add(SlaCvtType::Fk45z, &[1984.0]).unwrap();

    let ra = 1.0_f64;
    let dec = 0.5_f64;
    let pts = PointSet::from_rows([[ra, dec]]);

    let fk5 = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&fk5, false).unwrap();

    // FK45Z then FK54Z is not a perfect inverse: the E-term and fictitious
    // proper-motion handling leaves a genuine ~6e-11 residual, present
    // identically in the C library (both call the same vendored palFk45z /
    // palFk54z). 1e-9 sits comfortably above that residual while still
    // catching a real regression.
    assert_close(back.point(0)[0], ra, 1e-9, "fk45z ra round-trip");
    assert_close(back.point(0)[1], dec, 1e-9, "fk45z dec round-trip");
}

// ===========================================================================
// Multiple points
// ===========================================================================

#[test]
fn multiple_points() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();

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
// Serialization of inverted mapping
// ===========================================================================

#[test]
fn serialise_inverted() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();
    sla.invert();

    let obj = AnyObject::SlaMap(sla);
    let text = native_dump(&obj).unwrap();

    let restored = from_native_string(&text).unwrap();
    let sla2 = match restored {
        AnyObject::SlaMap(m) => m,
        other => panic!("expected SlaMap, got {other:?}"),
    };
    assert!(sla2.is_inverted());
}

// ===========================================================================
// try_merge: step concatenation and simplification
// ===========================================================================

#[test]
fn slamap_merge_concatenates_steps() {
    use starlink_ast::mappings::{Mapping, SlaCvtType, SlaMap};

    // Two SlaMaps with non-cancelling steps: should merge into one combined SlaMap.
    let mut sla1 = SlaMap::new();
    sla1.add(SlaCvtType::Fk45z, &[1950.0]).unwrap();
    let mut sla2 = SlaMap::new();
    sla2.add(SlaCvtType::Prec, &[2000.0, 2010.0]).unwrap();

    // Merging in series should concatenate steps into a single SlaMap.
    let merged = sla1.try_merge(&sla2, true, false, false);
    assert!(merged.is_some());
    let m = merged.unwrap();
    assert!(m.as_any().is::<SlaMap>());
}

#[test]
fn slamap_merge_cancels_inverse_pairs() {
    use starlink_ast::mappings::{Mapping, SlaCvtType, SlaMap, UnitMap};

    // FK4->FK5 followed by inverted FK4->FK5 should cancel to UnitMap.
    let mut sla1 = SlaMap::new();
    sla1.add(SlaCvtType::Fk45z, &[1950.0]).unwrap();
    let mut sla2 = SlaMap::new();
    sla2.add(SlaCvtType::Fk45z, &[1950.0]).unwrap();

    let merged = sla1.try_merge(&sla2, true, false, true);
    assert!(merged.is_some());
    let m = merged.unwrap();
    assert!(m.as_any().is::<UnitMap>());
}

#[test]
fn slamap_merge_combines_precession() {
    use starlink_ast::mappings::{Mapping, SlaCvtType, SlaMap};

    // Prec(2000,2010) + Prec(2010,2020) should merge to Prec(2000,2020).
    let mut sla1 = SlaMap::new();
    sla1.add(SlaCvtType::Prec, &[2000.0, 2010.0]).unwrap();
    let mut sla2 = SlaMap::new();
    sla2.add(SlaCvtType::Prec, &[2010.0, 2020.0]).unwrap();

    let merged = sla1.try_merge(&sla2, true, false, false);
    assert!(merged.is_some());
    // Result should be a single SlaMap (not UnitMap since it's not identity).
    let m = merged.unwrap();
    assert!(m.as_any().is::<SlaMap>());
}

#[test]
fn slamap_merge_precession_to_identity() {
    use starlink_ast::mappings::{Mapping, SlaCvtType, SlaMap, UnitMap};

    // Prec(2000,2010) + Prec(2010,2000) = Prec(2000,2000) = identity.
    let mut sla1 = SlaMap::new();
    sla1.add(SlaCvtType::Prec, &[2000.0, 2010.0]).unwrap();
    let mut sla2 = SlaMap::new();
    sla2.add(SlaCvtType::Prec, &[2010.0, 2000.0]).unwrap();

    let merged = sla1.try_merge(&sla2, true, false, false);
    assert!(merged.is_some());
    assert!(merged.unwrap().as_any().is::<UnitMap>());
}

// ===========================================================================
// Simplified
// ===========================================================================

#[test]
fn simplified_returns_clone() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();

    let simplified = <dyn Mapping>::simplify(&sla).unwrap();
    assert_eq!(simplified.nin(), 2);
    assert_eq!(simplified.nout(), 2);

    let pts = PointSet::from_rows([[1.0_f64, 0.5]]);
    let out1 = sla.transform(&pts, true).unwrap();
    let out2 = simplified.transform(&pts, true).unwrap();
    assert_close(out2.point(0)[0], out1.point(0)[0], 1e-15, "simplified lon");
    assert_close(out2.point(0)[1], out1.point(0)[1], 1e-15, "simplified lat");
}

// ===========================================================================
// Scalar-carrier transform parity (mirrors slamap.c Transform)
// ===========================================================================

// R2H then H2R is the identity, and C carries the raw (un-normalized)
// longitude scalar through both steps, so a large out-of-range input
// longitude survives verbatim rather than being wrapped into a 2pi interval.
#[test]
fn r2h_h2r_preserves_raw_longitude() {
    let last = 3.5;
    let mut m = SlaMap::new();
    m.add(SlaCvtType::R2h, &[last]).unwrap(); // alpha' = last - alpha
    m.add(SlaCvtType::H2r, &[last]).unwrap(); // alpha'' = last - alpha' = alpha
    // A longitude far outside [0, 2pi): the net map is the identity, and C
    // returns it unchanged rather than wrapping it.
    let raw_lon = -1000.0_f64;
    let lat = 0.3;
    let pin = PointSet::from_rows([[raw_lon, lat]].iter().map(|p| p.as_slice()));
    let out = m.transform(&pin, true).unwrap();
    assert_close(out.point(0)[0], raw_lon, 1e-9, "raw longitude preserved");
    assert_close(out.point(0)[1], lat, 1e-9, "latitude preserved");
}

// A single matrix conversion (PREC) must return longitude in [0, 2pi):
// C applies palDranrm after the matrix multiply. Feed a point whose
// precessed longitude is small-negative before wrapping, and confirm the
// output is the positive wrapped value (i.e. >= 0), not a (-pi, 0) value.
#[test]
fn matrix_op_longitude_is_normalized_to_0_2pi() {
    // PREC from J2000.0 to J2000.0 is the identity matrix, so the output
    // longitude equals palDranrm(input longitude). A negative input
    // longitude must come back wrapped into [0, 2pi).
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Prec, &[2000.0, 2000.0]).unwrap();
    let neg_lon = -1.0_f64; // ~ -57 deg
    let pin = PointSet::from_rows([[neg_lon, 0.2]].iter().map(|p| p.as_slice()));
    let out = m.transform(&pin, true).unwrap();
    let got = out.point(0)[0];
    assert!((0.0..2.0 * PI).contains(&got), "lon in [0,2pi): {got}");
    // palPrec for equal epochs is not bit-exactly the identity: eraPmat06
    // evaluates the precession polynomials even at t=0, so PREC(2000,2000)
    // carries an ~1e-7 rotation. The exact C-oracle output for this point is
    // 5.2831853953259547 (palDranrm applied to the precessed longitude), which
    // Rust reproduces to the last bit because pal::prec calls the same
    // vendored palPrec. Assert against that golden, not the raw input.
    // Shortest literal that round-trips to the same f64 as the C printout.
    const GOLDEN_PREC_LON: f64 = 5.283_185_395_325_955;
    assert_close(got, GOLDEN_PREC_LON, 1e-9, "PREC lon matches C golden");
}

// Off-J2000 EQECL golden value from C. C builds
// convert = ecmat(date) . prec(2000, epj(date)); the identity-at-J2000
// ecmat-only matrix is wrong away from J2000.0.
#[test]
fn eqecl_matches_c_off_j2000() {
    // MJD 55197.0 = 2010-01-01, ~10 years from J2000.0. Golden values from
    // a C probe of palSlaMap EQECL at this date for (ra,dec) = (83deg,22deg)
    // linked against reference/starlink-ast/build-ast/libast.so.
    let date = 55197.0;
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Eqecl, &[date]).unwrap();
    let pin = PointSet::from_rows(
        [[deg2rad(83.0), deg2rad(22.0)]]
            .iter()
            .map(|p| p.as_slice()),
    );
    let out = m.transform(&pin, true).unwrap();
    const GOLDEN_LON: f64 = 1.459_968_862_726_898;
    const GOLDEN_LAT: f64 = -0.022_347_584_010_998_657;
    assert_angle_close(out.point(0)[0], GOLDEN_LON, 1e-9, "ecl lon");
    assert_close(out.point(0)[1], GOLDEN_LAT, 1e-9, "ecl lat");
}

// A conversion with a bad argument zeroes (bad-values) the whole step's
// output for every point, and the bad propagates to the chain output.
#[test]
fn bad_arg_propagates_to_bad_output() {
    // ECLEQ needs one arg (the date). Supply BAD_VALUE: the step outputs
    // BAD for all points regardless of input, matching slamap.c:3672-3687.
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Eqecl, &[BAD_VALUE]).unwrap();
    let pin = PointSet::from_rows([[0.5, 0.2], [1.0, -0.3]].iter().map(|p| p.as_slice()));
    let out = m.transform(&pin, true).unwrap();
    for i in 0..2 {
        assert_eq!(out.point(i)[0], BAD_VALUE, "point {i} lon bad");
        assert_eq!(out.point(i)[1], BAD_VALUE, "point {i} lat bad");
    }
}

// AMP then MAP at matching equinox/date must round-trip to the input.
// AMP args are (DATE, EQ) so palMappa receives (eq=args[1], ep=args[0])
// (slamap.c:3815); MAP args are (EQ, DATE) so palMappa receives
// (eq=args[0], ep=args[1]) (slamap.c:3851).  With mismatched argument
// order the two amprms arrays differ and the steps do not cancel.
#[test]
fn amp_map_round_trip() {
    // AMP args are (DATE, EQ); MAP args are (EQ, DATE). Chaining AMP then
    // MAP with matching equinox/date must return the input.
    let eq = 2000.0;
    let date = 55197.0; // MJD 2010-01-01
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Amp, &[date, eq]).unwrap(); // apparent -> mean  (args: DATE, EQ)
    m.add(SlaCvtType::Map, &[eq, date]).unwrap(); // mean -> apparent  (args: EQ, DATE)

    let ra = deg2rad(120.0);
    let dec = deg2rad(-15.0);
    let pin = PointSet::from_rows([[ra, dec]].iter().map(|p| p.as_slice()));
    let out = m.transform(&pin, true).unwrap();

    assert_angle_close(out.point(0)[0], ra, 1e-9, "ra AMP-then-MAP round-trip");
    assert_close(out.point(0)[1], dec, 1e-9, "dec AMP-then-MAP round-trip");
}

// A bad input coordinate produces bad output on both axes (TRAN_ARRAY
// couples the axes: either bad -> both bad), and a NaN input is treated
// identically (is_bad includes !is_finite).
#[test]
fn bad_and_nan_input_produce_bad_output() {
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Prec, &[2000.0, 2010.0]).unwrap();
    let pin = PointSet::from_rows(
        [[BAD_VALUE, 0.2], [f64::NAN, 0.3], [0.5, BAD_VALUE]]
            .iter()
            .map(|p| p.as_slice()),
    );
    let out = m.transform(&pin, true).unwrap();
    for i in 0..3 {
        assert_eq!(out.point(i)[0], BAD_VALUE, "point {i} lon bad");
        assert_eq!(out.point(i)[1], BAD_VALUE, "point {i} lat bad");
    }
}

// Inverting an AMP or MAP step exchanges its date and equinox arguments as
// well as its type code, because the two conversions take the pair in
// opposite orders (slamap.c:2830-2832).  Simplifying an inverted SlaMap must
// therefore produce a forward SlaMap with the arguments swapped, and that
// SlaMap must transform identically to the inverted original.
#[test]
fn simplify_inverted_map_step_swaps_date_and_equinox() {
    let eq = 2000.0;
    let date = 55197.0;
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Map, &[eq, date]).unwrap(); // mean -> apparent (args: EQ, DATE)
    m.invert();

    let simplified = <dyn Mapping>::simplify(&m).unwrap();
    let sla = simplified
        .as_any()
        .downcast_ref::<SlaMap>()
        .expect("simplified SlaMap");
    assert!(!sla.is_inverted(), "replacement is applied forward");

    let dump = native_dump(&AnyObject::SlaMap(sla.clone())).unwrap();
    assert!(dump.contains("\"AMP\""), "step type inverted:\n{dump}");
    let args: Vec<&str> = dump
        .lines()
        .filter(|l| l.contains("Sla1a") || l.contains("Sla1b"))
        .collect();
    assert_eq!(args.len(), 2, "two arguments dumped:\n{dump}");
    assert!(
        args[0].contains(&format!("{date}")),
        "AMP takes the date first:\n{dump}"
    );
    assert!(
        args[1].contains(&format!("{eq}")),
        "AMP takes the equinox second:\n{dump}"
    );

    let ra = deg2rad(120.0);
    let dec = deg2rad(-15.0);
    let pin = PointSet::from_rows([[ra, dec]].iter().map(|p| p.as_slice()));
    let before = m.transform(&pin, true).unwrap();
    let after = simplified.transform(&pin, true).unwrap();
    assert_close(after.point(0)[0], before.point(0)[0], 1e-13, "ra preserved");
    assert_close(
        after.point(0)[1],
        before.point(0)[1],
        1e-13,
        "dec preserved",
    );
}

/// slamap.c CvtCode recognizes only the documented names, matched as
/// astChrMatch matches: case-insensitive, ignoring trailing white space.
#[test]
fn cvt_codes_match_as_c_matches_them() {
    use starlink_ast::mappings::SlaCvtType;
    assert_eq!(SlaCvtType::from_code("h2e "), Some(SlaCvtType::Dh2e));
    assert_eq!(SlaCvtType::from_code("E2H\t"), Some(SlaCvtType::De2h));
    assert_eq!(SlaCvtType::from_code("DH2E"), None);
    assert_eq!(SlaCvtType::from_code("DE2H"), None);
    assert_eq!(SlaCvtType::from_code(" H2E"), None);
}

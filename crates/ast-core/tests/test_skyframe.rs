//! Comprehensive integration tests for SkyFrame.
//!
//! Covers: construction, attributes, coordinate system conversion, spherical
//! geometry (distance, angle, offset, norm), HMS/DMS formatting, FrameSet
//! integrity, offset coordinates (SkyRefIs), serialisation round-trip, and
//! BAD_VALUE handling.
//!
//! Rust equivalent of reference/starlink-ast/ast_tester/testskyframe.c
//! (subset — tests only the features that have landed in Step 7b).

use crate::common::native_dump;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{
    BasicFrameBuilder, Frame, FrameRef, FrameSet, SkyFrame, SkyRefIs, SkySystem,
};
use ast_core::mappings::{DynMapping, Mapping, ZoomMap};
use ast_core::{AnyObject, from_native_string};

// ===========================================================================
// Helpers
// ===========================================================================

const TOL: f64 = 1.0e-10;

/// Assert two floats are within tolerance.
fn assert_close(a: f64, b: f64, tol: f64, label: &str) {
    assert!(
        (a - b).abs() < tol,
        "{label}: expected {b}, got {a} (diff = {:.2e})",
        (a - b).abs()
    );
}

/// Transform a single (lon, lat) point through a FrameSet (forward).
fn transform_fwd(fs: &FrameSet, lon: f64, lat: f64) -> (f64, f64) {
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = fs.transform(&pts, true).unwrap();
    let p = out.point(0);
    (p[0], p[1])
}

/// Transform a single (lon, lat) point through a FrameSet (inverse).
fn transform_inv(fs: &FrameSet, lon: f64, lat: f64) -> (f64, f64) {
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = fs.transform(&pts, false).unwrap();
    let p = out.point(0);
    (p[0], p[1])
}

/// Compare two angles modulo 2*pi.
fn assert_angle_close(a: f64, b: f64, tol: f64, label: &str) {
    let diff = (a - b).rem_euclid(2.0 * PI);
    let err = diff.min(2.0 * PI - diff);
    assert!(
        err < tol,
        "{label}: expected {b}, got {a} (angular err = {err:.2e})"
    );
}

// ===========================================================================
// 1. Construction and Attributes
// ===========================================================================

#[test]
fn construction_fk5_defaults() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    assert_eq!(sky.naxes(), 2);
    assert_eq!(sky.system(), SkySystem::Fk5);
    assert_eq!(sky.domain(), "SKY");
    // The default C reports (skyframe.c `GetTitle`).
    assert!(!sky.test_title());
    assert_eq!(
        sky.title().unwrap(),
        "FK5 equatorial coordinates; mean equinox J2000.0"
    );
    assert!(!sky.neg_lon());
    assert_eq!(sky.sky_ref_is(), SkyRefIs::Ignored);
    assert!(sky.projection().is_none());
}

#[test]
fn construction_each_system() {
    let systems = [
        SkySystem::Fk4,
        SkySystem::Fk4NoE,
        SkySystem::Fk5,
        SkySystem::Gappt,
        SkySystem::Ecliptic,
        SkySystem::Galactic,
        SkySystem::Supergalactic,
        SkySystem::Icrs,
        SkySystem::J2000,
        SkySystem::Helioecliptic,
        SkySystem::Azel,
    ];
    for sys in systems {
        let sky = SkyFrame::new(sys);
        assert_eq!(sky.system(), sys, "system mismatch for {:?}", sys);
        assert_eq!(sky.naxes(), 2, "naxes should always be 2");
        assert_eq!(sky.domain(), "SKY", "domain should default to SKY");
    }
}

#[test]
fn default_equinox_fk5() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // FK5 default equinox is J2000.0 = MJD 51544.5
    assert_close(sky.equinox_mjd(), 51544.5, TOL, "FK5 default equinox");
}

#[test]
fn default_equinox_fk4() {
    let sky = SkyFrame::new(SkySystem::Fk4);
    // FK4 default equinox is B1950.0
    // B1950 MJD is approximately 33281.92346
    let b1950_mjd = SkySystem::Fk4.default_equinox_mjd();
    assert_close(sky.equinox_mjd(), b1950_mjd, 1.0e-6, "FK4 default equinox");
    // Verify it is NOT J2000
    assert!(
        (sky.equinox_mjd() - 51544.5).abs() > 1000.0,
        "FK4 equinox should differ from J2000"
    );
}

#[test]
fn set_get_epoch() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    sky.set_epoch_mjd(53000.0);
    assert_close(sky.epoch_mjd(), 53000.0, TOL, "epoch");
}

#[test]
fn default_epoch_depends_on_system() {
    // SkyFrame GetEpoch override (skyframe.c:3210-3225): B1950.0 for FK4 /
    // FK4-no-E, J2000.0 otherwise. Oracle-verified against the reference C
    // library: astGetD(Epoch) == 1950.0 (FK4/FK4NOE), 2000.0 (FK5/ICRS).
    // J2000.0 = MJD 51544.5 exactly (palEpj2d(2000.0)); B1950.0 is the same
    // Besselian fiducial the FK4 default equinox uses.
    let j2000 = 51544.5;
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let icrs = SkyFrame::new(SkySystem::Icrs);
    assert_close(fk5.epoch_mjd(), j2000, 1.0e-6, "FK5 default epoch = J2000");
    assert_close(
        icrs.epoch_mjd(),
        j2000,
        1.0e-6,
        "ICRS default epoch = J2000",
    );

    let b1950 = SkySystem::Fk4.default_equinox_mjd();
    let fk4 = SkyFrame::new(SkySystem::Fk4);
    let fk4ne = SkyFrame::new(SkySystem::Fk4NoE);
    assert_close(fk4.epoch_mjd(), b1950, 1.0e-6, "FK4 default epoch = B1950");
    assert_close(
        fk4ne.epoch_mjd(),
        b1950,
        1.0e-6,
        "FK4-no-E default epoch = B1950",
    );
    // B1950 is far from J2000.
    assert!((fk4.epoch_mjd() - j2000).abs() > 1000.0);
}

#[test]
fn default_bottom_top_clamp_latitude() {
    // SkyFrame GetBottom/GetTop overrides (skyframe.c:2907/3234): latitude
    // axis clamped to +/-pi/2, longitude axis unbounded (+/-f64::MAX).
    // Oracle-verified: external axis 1 (lon) = +/-DBL_MAX, axis 2 (lat) =
    // +/-1.5708.
    let sky = SkyFrame::new(SkySystem::Fk5);
    // Default external order: axis 1 = longitude, axis 2 = latitude.
    assert_eq!(sky.bottom(1).unwrap(), -f64::MAX);
    assert_eq!(sky.top(1).unwrap(), f64::MAX);
    assert_close(sky.bottom(2).unwrap(), -FRAC_PI_2, TOL, "lat bottom");
    assert_close(sky.top(2).unwrap(), FRAC_PI_2, TOL, "lat top");
}

#[test]
fn default_bottom_top_follow_axis_permutation() {
    // After permuting so latitude is external axis 1, the clamp must follow
    // the physical latitude axis (C resolves Bottom/Top on the permuted
    // axis_p, not the external index).
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    sky.perm_axes(&[2, 1]).unwrap();
    // Now external axis 1 is latitude, axis 2 is longitude.
    assert_close(sky.bottom(1).unwrap(), -FRAC_PI_2, TOL, "lat bottom (perm)");
    assert_close(sky.top(1).unwrap(), FRAC_PI_2, TOL, "lat top (perm)");
    assert_eq!(sky.bottom(2).unwrap(), -f64::MAX);
    assert_eq!(sky.top(2).unwrap(), f64::MAX);
}

#[test]
fn set_get_equinox() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    sky.set_equinox_mjd(55000.0);
    assert_close(sky.equinox_mjd(), 55000.0, TOL, "equinox");
}

#[test]
fn set_get_neglon() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    assert!(!sky.neg_lon());
    sky.set_neg_lon(true);
    assert!(sky.neg_lon());
    sky.set_neg_lon(false);
    assert!(!sky.neg_lon());
}

#[test]
fn set_get_sky_ref() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    sky.set_sky_ref_lon_lat(1.5, 0.75);
    assert_close(sky.sky_ref_lon_lat()[0], 1.5, TOL, "sky_ref lon");
    assert_close(sky.sky_ref_lon_lat()[1], 0.75, TOL, "sky_ref lat");
}

#[test]
fn set_get_sky_ref_is() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    assert_eq!(sky.sky_ref_is(), SkyRefIs::Ignored);
    sky.set_sky_ref_is(SkyRefIs::Origin);
    assert_eq!(sky.sky_ref_is(), SkyRefIs::Origin);
    sky.set_sky_ref_is(SkyRefIs::Pole);
    assert_eq!(sky.sky_ref_is(), SkyRefIs::Pole);
}

#[test]
fn set_get_projection() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    assert!(sky.projection().is_none());
    sky.set_projection("TAN");
    assert_eq!(sky.projection(), Some("TAN"));
}

#[test]
fn set_get_observer_attributes() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    assert!(!sky.test_obs_lat());
    assert!(!sky.test_obs_lon());
    assert!(!sky.test_obs_alt());
    assert!(!sky.test_dtai());
    assert!(!sky.test_dut1());

    sky.set_obs_lat(0.35);
    sky.set_obs_lon(-2.1);
    sky.set_obs_alt(4200.0);
    sky.set_dtai(37.0);
    sky.set_dut1(0.27);

    assert_close(
        {
            assert!(sky.test_obs_lat());
            sky.obs_lat()
        },
        0.35,
        TOL,
        "obs_lat",
    );
    assert_close(
        {
            assert!(sky.test_obs_lon());
            sky.obs_lon()
        },
        -2.1,
        TOL,
        "obs_lon",
    );
    assert_close(
        {
            assert!(sky.test_obs_alt());
            sky.obs_alt()
        },
        4200.0,
        TOL,
        "obs_alt",
    );
    assert_close(
        {
            assert!(sky.test_dtai());
            sky.dtai()
        },
        37.0,
        TOL,
        "dtai",
    );
    assert_close(
        {
            assert!(sky.test_dut1());
            sky.dut1()
        },
        0.27,
        TOL,
        "dut1",
    );
}

// ===========================================================================
// 2. Coordinate System Conversion (via convert)
// ===========================================================================

#[test]
fn fk5_to_galactic_centre() {
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let gal = SkyFrame::new(SkySystem::Galactic);
    let result = fk5.convert_to(&gal, &[]).unwrap();
    assert!(result.is_some(), "FK5->Galactic should succeed");
    let fs = result.unwrap();

    // Transform galactic centre (l=0, b=0) back to FK5 (inverse = gal->fk5).
    let (ra, dec) = transform_inv(&fs, 0.0, 0.0);
    let ra_deg = ra.to_degrees().rem_euclid(360.0);
    let dec_deg = dec.to_degrees();

    // Galactic centre in FK5 J2000: RA ~266.4 deg, Dec ~-28.9 deg
    assert!(
        (ra_deg - 266.4).abs() < 0.5,
        "RA of galactic centre: {ra_deg}"
    );
    assert!(
        (dec_deg - (-28.94)).abs() < 0.5,
        "Dec of galactic centre: {dec_deg}"
    );
}

#[test]
fn fk5_to_galactic_forward() {
    // Forward: FK5 -> Galactic
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let gal = SkyFrame::new(SkySystem::Galactic);
    let fs = fk5.convert_to(&gal, &[]).unwrap().unwrap();

    // FK5 RA=0, Dec=0 should give some finite galactic coordinates.
    let (l, b) = transform_fwd(&fs, 0.0, 0.0);
    assert!(l.is_finite(), "galactic l should be finite");
    assert!(b.is_finite(), "galactic b should be finite");
}

#[test]
fn fk5_to_icrs_near_identity() {
    // ICRS ~= FK5 J2000 to within ~20 mas (frame tie).
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let icrs = SkyFrame::new(SkySystem::Icrs);
    let fs = fk5.convert_to(&icrs, &[]).unwrap().unwrap();

    let test_lon = 2.0;
    let test_lat = 0.3;
    let (out_lon, out_lat) = transform_fwd(&fs, test_lon, test_lat);
    assert!(
        (out_lon - test_lon).abs() < 1.0e-4,
        "FK5->ICRS lon diff too large"
    );
    assert!(
        (out_lat - test_lat).abs() < 1.0e-4,
        "FK5->ICRS lat diff too large"
    );
}

#[test]
fn galactic_supergalactic_round_trip() {
    let gal = SkyFrame::new(SkySystem::Galactic);
    let sg = SkyFrame::new(SkySystem::Supergalactic);

    let fs_fwd = gal.convert_to(&sg, &[]).unwrap().unwrap();

    let test_lon = 1.5;
    let test_lat = 0.3;
    let (sg_lon, sg_lat) = transform_fwd(&fs_fwd, test_lon, test_lat);
    let (back_lon, back_lat) = transform_inv(&fs_fwd, sg_lon, sg_lat);

    assert_close(back_lon, test_lon, 1.0e-8, "galactic lon round-trip");
    assert_close(back_lat, test_lat, 1.0e-8, "galactic lat round-trip");
}

#[test]
fn fk4_to_fk5_round_trip() {
    let fk4 = SkyFrame::new(SkySystem::Fk4);
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let fs = fk4.convert_to(&fk5, &[]).unwrap().unwrap();

    let test_lon = 1.0;
    let test_lat = 0.5;
    let (fk5_lon, fk5_lat) = transform_fwd(&fs, test_lon, test_lat);
    let (back_lon, back_lat) = transform_inv(&fs, fk5_lon, fk5_lat);

    assert_close(back_lon, test_lon, 1.0e-8, "FK4->FK5 lon round-trip");
    assert_close(back_lat, test_lat, 1.0e-8, "FK4->FK5 lat round-trip");
}

#[test]
fn all_systems_round_trip_via_fk5() {
    // For each system, convert to FK5 and back; verify < 1e-8 rad.
    let systems = [
        SkySystem::Fk4,
        SkySystem::Fk4NoE,
        SkySystem::Fk5,
        SkySystem::Gappt,
        SkySystem::Ecliptic,
        SkySystem::Galactic,
        SkySystem::Supergalactic,
        SkySystem::Icrs,
        SkySystem::J2000,
        SkySystem::Helioecliptic,
    ];

    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let test_lon = 1.0;
    let test_lat = 0.4;

    for sys in systems {
        let frame = SkyFrame::new(sys);
        let fs = frame.convert_to(&fk5, &[]).unwrap().unwrap();

        let (fk5_lon, fk5_lat) = transform_fwd(&fs, test_lon, test_lat);
        let (back_lon, back_lat) = transform_inv(&fs, fk5_lon, fk5_lat);

        let name = sys.as_str();
        assert_close(back_lon, test_lon, 1.0e-8, &format!("{name} lon rt"));
        assert_close(back_lat, test_lat, 1.0e-8, &format!("{name} lat rt"));
    }
}

#[test]
fn convert_skyframe_to_blank_domain_frame_is_unitmap() {
    // `astConvert(SkyFrame, Frame[Domain=""])` succeeds with a UnitMap:
    // a blank-domain plain Frame matches any Frame, and the generic
    // ConvertX driver casts the SkyFrame down to a plain Frame for the
    // alignment. Matches the C oracle (ast_tester/testconvert.c).
    let sky = SkyFrame::new(SkySystem::Fk5);
    let basic = BasicFrameBuilder::new(2).build().unwrap();
    let fs = sky
        .convert_to(&basic, &[])
        .unwrap()
        .expect("SkyFrame -> blank-domain Frame converts");
    let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    assert!(
        map.as_any().is::<ast_core::mappings::UnitMap>(),
        "SkyFrame -> blank-domain Frame is a UnitMap"
    );
}

#[test]
fn convert_skyframe_and_sky_domain_frame_is_unitmap() {
    // Port of ast_tester/testconvert.c errors 11-18: a plain Frame with
    // Domain=SKY aligns with a SkyFrame in both directions, and the
    // resulting Mapping is a UnitMap. The generic ConvertX driver routes
    // through SkyFrame::match_template/sub_frame with the matchsub cast
    // retry that downcasts the SkyFrame to a plain Frame. Oracle-confirmed:
    // testconvert passes against libast.
    let mut bf = BasicFrameBuilder::new(2).build().unwrap();
    bf.set_domain("SKY");
    let sky = SkyFrame::new(SkySystem::Fk5);

    // Frame[SKY] -> SkyFrame.
    let fs1 = bf
        .convert_to(&sky, &[])
        .unwrap()
        .expect("Frame[SKY] -> SkyFrame converts");
    assert!(
        fs1.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_any()
            .is::<ast_core::mappings::UnitMap>(),
        "Frame[SKY] -> SkyFrame is a UnitMap"
    );

    // SkyFrame -> Frame[SKY].
    let fs2 = sky
        .convert_to(&bf, &[])
        .unwrap()
        .expect("SkyFrame -> Frame[SKY] converts");
    assert!(
        fs2.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_any()
            .is::<ast_core::mappings::UnitMap>(),
        "SkyFrame -> Frame[SKY] is a UnitMap"
    );

    // A non-SKY domain Frame must NOT align with a SkyFrame
    // (testconvert.c errors 9/10).
    let mut nf = BasicFrameBuilder::new(2).build().unwrap();
    nf.set_domain("NOTSKY");
    assert!(
        sky.convert_to(&nf, &[]).unwrap().is_none(),
        "SkyFrame -> Frame[NOTSKY] does not align"
    );
}

#[test]
fn convert_same_system_is_identity() {
    let f1 = SkyFrame::new(SkySystem::Fk5);
    let f2 = SkyFrame::new(SkySystem::Fk5);
    let fs = f1.convert_to(&f2, &[]).unwrap().unwrap();

    let test_lon = 2.5;
    let test_lat = 0.8;
    let (out_lon, out_lat) = transform_fwd(&fs, test_lon, test_lat);
    assert_close(out_lon, test_lon, TOL, "identity lon");
    assert_close(out_lat, test_lat, TOL, "identity lat");
}

// ===========================================================================
// 3. Spherical Geometry
// ===========================================================================

#[test]
fn distance_pole_to_pole() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let d = sky.distance(&[0.0, FRAC_PI_2], &[0.0, -FRAC_PI_2]).unwrap();
    assert_close(d, PI, TOL, "pole-to-pole distance");
}

#[test]
fn distance_equatorial_one_radian() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let d = sky.distance(&[0.0, 0.0], &[1.0, 0.0]).unwrap();
    assert_close(d, 1.0, 1.0e-8, "equatorial 1 radian");
}

#[test]
fn distance_same_point_is_zero() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let d = sky.distance(&[1.5, 0.3], &[1.5, 0.3]).unwrap();
    assert_close(d, 0.0, TOL, "same-point distance");
}

#[test]
fn distance_equatorial_quarter_circle() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let d = sky.distance(&[0.0, 0.0], &[FRAC_PI_2, 0.0]).unwrap();
    assert_close(d, FRAC_PI_2, 1.0e-8, "equatorial pi/2");
}

#[test]
fn angle_returns_finite() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // Verify angle returns a finite value for a general triangle.
    let a = [0.0, 0.0];
    let b = [0.5, 0.3];
    let c = [1.0, 0.0];
    let angle = sky.angle(&a, &b, &c).unwrap();
    assert!(angle.is_finite(), "angle should be finite");
    // The angle should be bounded by [-pi, pi].
    assert!(angle.abs() <= PI + 1.0e-10, "angle should be in [-pi, pi]");
}

#[test]
fn angle_collinear_equatorial_points() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // Three equatorial points in a straight line (great circle).
    // The angle at the middle point should be pi (straight through).
    let a = [0.0, 0.0];
    let b = [0.5, 0.0];
    let c = [1.0, 0.0];
    let angle = sky.angle(&a, &b, &c).unwrap();
    // For collinear points on the equator, bearings from A to B and C to B
    // are in opposite directions, so their difference should be near pi.
    assert_close(angle.abs(), PI, 1.0e-6, "collinear equatorial angle");
}

#[test]
fn offset_equator_toward_pole() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // Walk pi/4 radians from equator at lon=0 toward the north pole.
    let from = [0.0, 0.0];
    let to = [0.0, FRAC_PI_2];
    let result = sky.offset(&from, &to, FRAC_PI_4).unwrap();
    // Should end up at lat=pi/4.
    assert_angle_close(result[0], 0.0, 1.0e-8, "offset lon");
    assert_close(result[1], FRAC_PI_4, 1.0e-8, "offset lat");
}

/// C constrains the longitude `Offset` returns to `0 .. 2*pi`, because
/// `palDcc2s` does not do it itself (`skyframe.c:7720` and `:7746`). Walking
/// westward across longitude zero must therefore report a longitude just under
/// `2*pi`, not a small negative one — a Circle's mesh is built by repeated
/// `Offset`, so a raw negative longitude propagates into its bounds.
#[test]
fn offset_normalises_the_longitude_it_returns() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let from = [0.1, 0.0];
    let to = [0.0, 0.0];
    let result = sky.offset(&from, &to, 0.2).unwrap();
    assert!(
        result[0] >= 0.0 && result[0] < 2.0 * PI,
        "offset longitude must lie in 0..2*pi, got {}",
        result[0]
    );
    // Oracle-verified: C reports 6.183_185_307_179_587 for this offset.
    assert_close(
        result[0],
        6.183_185_307_179_587,
        1.0e-15,
        "offset lon wrapped",
    );

    // The coincident-points branch normalises too (skyframe.c:7718-7720). C
    // reaches it only when `sin(offset)` is exactly zero, which in double
    // precision means `offset == 0.0` — `sin(PI)` is 1.22e-16, so C returns
    // bad for a nominally antipodal offset.
    let result = sky.offset(&[-0.1, 0.0], &[-0.1, 0.0], 0.0).unwrap();
    assert_close(
        result[0],
        6.183_185_307_179_587,
        1.0e-15,
        "coincident offset lon wrapped",
    );

    let result = sky.offset(&[0.1, 0.0], &[0.1, 0.0], PI).unwrap();
    assert_eq!(
        result,
        vec![BAD_VALUE, BAD_VALUE],
        "a non-zero offset between coincident points defines no great circle"
    );
}

#[test]
fn offset_full_distance() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let from = [0.0, 0.0];
    let to = [FRAC_PI_2, 0.0];
    let sep = sky.distance(&from, &to).unwrap();
    let result = sky.offset(&from, &to, sep).unwrap();
    assert_angle_close(result[0], FRAC_PI_2, 1.0e-8, "offset full lon");
    assert_close(result[1], 0.0, 1.0e-8, "offset full lat");
}

#[test]
fn norm_longitude_wrapping() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // Longitude > 2*pi should wrap to [0, 2pi] (neglon=false).
    let mut coords = [3.0 * PI, 0.3];
    sky.norm(&mut coords).unwrap();
    // 3*pi mod 2*pi = pi
    assert_close(coords[0], PI, 1.0e-8, "norm lon wrapping");
    assert_close(coords[1], 0.3, TOL, "norm lat unchanged");
}

#[test]
fn norm_latitude_pole_folding() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // Latitude > pi/2 should fold through the north pole.
    let mut coords = [0.0, FRAC_PI_2 + 0.1];
    sky.norm(&mut coords).unwrap();
    // lat becomes pi/2 - 0.1
    assert_close(coords[1], FRAC_PI_2 - 0.1, 1.0e-8, "norm lat folding");
    // lon shifts by pi
    assert_close(coords[0], PI, 1.0e-8, "norm lon shifted by pi");
}

#[test]
fn norm_neglon_mode() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    sky.set_neg_lon(true);
    let mut coords = [4.0, 0.3];
    sky.norm(&mut coords).unwrap();
    // With neglon, longitude should be in [-pi, pi].
    assert!(
        coords[0] >= -PI && coords[0] <= PI,
        "neglon mode: lon={} should be in [-pi, pi]",
        coords[0]
    );
}

#[test]
fn norm_negative_latitude() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // Latitude < -pi/2 should fold through the south pole.
    let mut coords = [0.0, -FRAC_PI_2 - 0.1];
    sky.norm(&mut coords).unwrap();
    assert_close(
        coords[1],
        -FRAC_PI_2 + 0.1,
        1.0e-8,
        "norm south pole folding",
    );
}

// ===========================================================================
// 4. HMS/DMS Formatting
// ===========================================================================

#[test]
fn format_longitude_zero() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let s = sky.format(1, 0.0).unwrap();
    assert!(
        s.contains("0:00:00"),
        "0 rad should format as 0:00:00, got: {s}"
    );
}

#[test]
fn format_longitude_six_hours() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // pi/2 rad = 6 hours
    let s = sky.format(1, FRAC_PI_2).unwrap();
    assert!(
        s.contains("6:00:00"),
        "pi/2 rad should contain 6:00:00, got: {s}"
    );
}

#[test]
fn format_latitude_45_degrees() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // pi/4 rad = 45 degrees
    let s = sky.format(2, FRAC_PI_4).unwrap();
    // C `astFormat` emits no leading '+' for positive sexagesimal values.
    assert!(
        s.contains("45:00:00") && !s.contains('+'),
        "pi/4 should contain 45:00:00 with no leading +, got: {s}"
    );
}

#[test]
fn unformat_hms() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let (val, _nc) = sky.unformat(1, "12:30:00").unwrap();
    // 12h30m = 12.5 hours = 12.5 * pi/12 radians
    let expected = 12.5 * PI / 12.0;
    assert_close(val, expected, 1.0e-6, "unformat 12:30:00");
}

#[test]
fn format_unformat_round_trip_longitude() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    // Pick a value: 1.234 rad
    let original = 1.234;
    let formatted = sky.format(1, original).unwrap();
    let (restored, _nc) = sky.unformat(1, &formatted).unwrap();
    // Tolerance depends on formatting precision (1 decimal for HMS time).
    assert_close(restored, original, 1.0e-4, "format/unformat round-trip lon");
}

#[test]
fn format_unformat_round_trip_latitude() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let original = 0.5; // ~28.6 degrees
    let formatted = sky.format(2, original).unwrap();
    let (restored, _nc) = sky.unformat(2, &formatted).unwrap();
    // DMS with 0 decimal places: precision ~1 arcsec
    let one_arcsec = (1.0_f64).to_radians() / 3600.0;
    assert_close(
        restored,
        original,
        2.0 * one_arcsec,
        "format/unformat rt lat",
    );
}

#[test]
fn format_axis_out_of_range() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    assert!(sky.format(0, 0.0).is_err(), "axis 0 should be rejected");
    assert!(sky.format(3, 0.0).is_err(), "axis 3 should be rejected");
}

// ===========================================================================
// 5. FrameSet Integrity
// ===========================================================================

#[test]
fn frameset_with_skyframe_current() {
    // Build a FrameSet: base=BasicFrame(2), current=SkyFrame(FK5), mapping=ZoomMap
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let current: Box<dyn Frame> = Box::new(SkyFrame::new(SkySystem::Fk5));
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 1.0).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    // Frame delegation should go to the current SkyFrame.
    assert_eq!(fs.naxes(), 2);
    assert_eq!(fs.current().naxes(), 2);
    assert_eq!(fs.current().system_string().unwrap(), Some("FK5"));

    // Transform should work through the FrameSet.
    let pts = PointSet::from_rows([[1.0, 0.5]]);
    let out = fs.transform(&pts, true).unwrap();
    // ZoomMap(2,1.0) is identity.
    assert_close(out.point(0)[0], 1.0, TOL, "frameset fwd lon");
    assert_close(out.point(0)[1], 0.5, TOL, "frameset fwd lat");
}

#[test]
fn frameset_skyframe_frame_mut_escape_hatch() {
    // Use frame_mut to directly mutate the SkyFrame without integrity.
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let current: Box<dyn Frame> = Box::new(SkyFrame::new(SkySystem::Fk5));
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 1.0).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    // Transform before mutation.
    let pts = PointSet::from_rows([[1.0, 0.5]]);
    let out1 = fs.transform(&pts, true).unwrap();
    let r1 = out1.point(0).to_vec();

    // Use frame_mut (escape hatch) to change system. This bypasses integrity,
    // so the mapping edges are NOT updated.
    let frame = fs.frame_mut(FrameRef::Current).unwrap();
    assert!(frame.as_any().is::<SkyFrame>());

    // Transform after mutation -- since frame_mut bypasses integrity,
    // the mapping is unchanged, so the transform result is identical.
    let out2 = fs.transform(&pts, true).unwrap();
    let r2 = out2.point(0).to_vec();
    assert_close(r1[0], r2[0], TOL, "frame_mut no integrity change lon");
    assert_close(r1[1], r2[1], TOL, "frame_mut no integrity change lat");
}

#[test]
fn frameset_skyframe_distance_delegation() {
    // FrameSet with SkyFrame as current delegates distance to spherical geometry.
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let current: Box<dyn Frame> = Box::new(SkyFrame::new(SkySystem::Fk5));
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 1.0).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    // Spherical distance: poles are pi apart.
    let d = fs.distance(&[0.0, FRAC_PI_2], &[0.0, -FRAC_PI_2]).unwrap();
    assert_close(d, PI, TOL, "frameset skyframe distance pole-to-pole");
}

// ===========================================================================
// 6. Offset Coordinates
// ===========================================================================

#[test]
fn sky_ref_is_origin_offset() {
    // Source frame has offset coordinates; target is standard FK5.
    let mut src = SkyFrame::new(SkySystem::Fk5);
    src.set_sky_ref_lon_lat(1.0, 0.5);
    src.set_sky_ref_p_lon_lat(1.0, 0.5 + 0.1);
    src.set_sky_ref_is(SkyRefIs::Origin);

    let dst = SkyFrame::new(SkySystem::Fk5);
    let fs = src.convert_to(&dst, &[]).unwrap().unwrap();

    // (0,0) in offset should map to sky_ref in standard.
    let (lon, lat) = transform_fwd(&fs, 0.0, 0.0);
    assert_angle_close(lon, 1.0, 1.0e-10, "origin offset lon");
    assert_close(lat, 0.5, 1.0e-10, "origin offset lat");
}

#[test]
fn sky_ref_is_origin_round_trip() {
    let mut src = SkyFrame::new(SkySystem::Fk5);
    src.set_sky_ref_lon_lat(2.0, -0.3);
    src.set_sky_ref_p_lon_lat(2.0, -0.3 + 0.1);
    src.set_sky_ref_is(SkyRefIs::Origin);

    let dst = SkyFrame::new(SkySystem::Fk5);
    let fs = src.convert_to(&dst, &[]).unwrap().unwrap();

    // Round-trip: offset -> standard -> offset
    let test_lon = 0.1;
    let test_lat = 0.2;
    let (std_lon, std_lat) = transform_fwd(&fs, test_lon, test_lat);
    let (back_lon, back_lat) = transform_inv(&fs, std_lon, std_lat);
    assert_close(back_lon, test_lon, 1.0e-9, "origin rt lon");
    assert_close(back_lat, test_lat, 1.0e-9, "origin rt lat");
}

#[test]
fn sky_ref_is_pole_offset() {
    // With Pole mode, sky_ref becomes the north pole.
    let mut src = SkyFrame::new(SkySystem::Fk5);
    src.set_sky_ref_lon_lat(1.5, 0.8);
    src.set_sky_ref_p_lon_lat(0.0, 0.0);
    src.set_sky_ref_is(SkyRefIs::Pole);

    let dst = SkyFrame::new(SkySystem::Fk5);
    let fs = src.convert_to(&dst, &[]).unwrap().unwrap();

    // The offset north pole (0, pi/2) should map to sky_ref.
    let (_, lat) = transform_fwd(&fs, 0.0, FRAC_PI_2);
    assert_close(lat, 0.8, 1.0e-10, "pole offset lat at pole");
}

#[test]
fn sky_ref_is_pole_round_trip() {
    let mut src = SkyFrame::new(SkySystem::Fk5);
    src.set_sky_ref_lon_lat(0.5, 1.0);
    src.set_sky_ref_p_lon_lat(0.5, 0.0);
    src.set_sky_ref_is(SkyRefIs::Pole);

    let dst = SkyFrame::new(SkySystem::Fk5);
    let fs = src.convert_to(&dst, &[]).unwrap().unwrap();

    let test_lon = 0.3;
    let test_lat = 0.4;
    let (std_lon, std_lat) = transform_fwd(&fs, test_lon, test_lat);
    let (back_lon, back_lat) = transform_inv(&fs, std_lon, std_lat);
    assert_close(back_lon, test_lon, 1.0e-9, "pole rt lon");
    assert_close(back_lat, test_lat, 1.0e-9, "pole rt lat");
}

#[test]
fn sky_ref_is_ignored_no_effect() {
    // With Ignored mode, the conversion should be the same as standard.
    let mut src = SkyFrame::new(SkySystem::Fk5);
    src.set_sky_ref_lon_lat(1.0, 0.5);
    src.set_sky_ref_is(SkyRefIs::Ignored); // explicit

    let dst = SkyFrame::new(SkySystem::Fk5);
    let fs = src.convert_to(&dst, &[]).unwrap().unwrap();

    // With Ignored, the offset should have no effect. FK5->FK5 is identity.
    let test_lon = 2.0;
    let test_lat = 0.3;
    let (out_lon, out_lat) = transform_fwd(&fs, test_lon, test_lat);
    assert_close(out_lon, test_lon, TOL, "ignored lon");
    assert_close(out_lat, test_lat, TOL, "ignored lat");
}

#[test]
fn convert_between_offset_frames() {
    // Both source and target have offset coordinates.
    let mut src = SkyFrame::new(SkySystem::Fk5);
    src.set_sky_ref_lon_lat(0.5, 0.3);
    src.set_sky_ref_p_lon_lat(0.5, 0.3 + 0.1);
    src.set_sky_ref_is(SkyRefIs::Origin);

    let mut dst = SkyFrame::new(SkySystem::Fk5);
    dst.set_sky_ref_lon_lat(2.0, -0.4);
    dst.set_sky_ref_p_lon_lat(2.0, -0.4 + 0.1);
    dst.set_sky_ref_is(SkyRefIs::Origin);

    let fs = src.convert_to(&dst, &[]).unwrap().unwrap();

    // Round-trip test.
    let test_lon = 0.1;
    let test_lat = 0.2;
    let (mid_lon, mid_lat) = transform_fwd(&fs, test_lon, test_lat);
    let (back_lon, back_lat) = transform_inv(&fs, mid_lon, mid_lat);

    assert_close(back_lon, test_lon, 1.0e-9, "offset-offset rt lon");
    assert_close(back_lat, test_lat, 1.0e-9, "offset-offset rt lat");
}

// ===========================================================================
// 7. Serialisation Round-Trip
// ===========================================================================

#[test]
fn serialise_default_fk5() {
    let original = SkyFrame::new(SkySystem::Fk5);
    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();

    // Structural markers.
    assert!(text.contains("Begin SkyFrame"), "missing Begin SkyFrame");
    assert!(text.contains("IsA Frame"), "missing IsA Frame");
    assert!(text.contains("End SkyFrame"), "missing End SkyFrame");
    assert!(text.contains("FK5"), "missing system name");

    // Deserialise and verify.
    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.system(), SkySystem::Fk5);
            assert_eq!(sf.naxes(), 2);
            assert_eq!(sf.domain(), "SKY");
            assert!(!sf.neg_lon());
            assert_eq!(sf.sky_ref_is(), SkyRefIs::Ignored);
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn serialise_preserves_equinox() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    let j2010_mjd = 55197.0;
    original.set_equinox_mjd(j2010_mjd);

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    assert!(text.contains("Eqnox"), "missing Eqnox");

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_close(sf.equinox_mjd(), j2010_mjd, 1e-6, "equinox round-trip");
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn serialise_preserves_sky_ref() {
    let mut original = SkyFrame::new(SkySystem::Fk5);
    original.set_sky_ref_is(SkyRefIs::Origin);
    original.set_sky_ref_lon_lat(1.5, 0.75);
    original.set_sky_ref_p_lon_lat(0.3, 1.2);

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    assert!(text.contains("SRefIs"), "missing SRefIs");
    assert!(text.contains("SRef1"), "missing SRef1");
    assert!(text.contains("SRef2"), "missing SRef2");

    let restored = from_native_string(&text).unwrap();
    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.sky_ref_is(), SkyRefIs::Origin);
            assert_close(sf.sky_ref_lon_lat()[0], 1.5, 1e-10, "sky_ref lon rt");
            assert_close(sf.sky_ref_lon_lat()[1], 0.75, 1e-10, "sky_ref lat rt");
            assert_close(sf.sky_ref_p_lon_lat()[0], 0.3, 1e-10, "sky_ref_p lon rt");
            assert_close(sf.sky_ref_p_lon_lat()[1], 1.2, 1e-10, "sky_ref_p lat rt");
        }
        _ => panic!("expected SkyFrame variant"),
    }
}

#[test]
fn serialise_preserves_all_attributes() {
    let mut original = SkyFrame::new(SkySystem::Galactic);
    original.set_title("Test Sky");
    original.set_domain("CUSTOM");
    original.set_epoch_mjd(53000.0);
    original.set_neg_lon(true);
    original.set_sky_tol(1.0e-7);
    original.set_projection("SIN");
    original.set_align_offset(true);
    original.set_obs_lat(0.5);
    original.set_obs_lon(-1.5);
    original.set_obs_alt(2800.0);
    original.set_dtai(37.0);
    original.set_dut1(0.3);

    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    let restored = from_native_string(&text).unwrap();

    match restored {
        AnyObject::SkyFrame(ref sf) => {
            assert_eq!(sf.system(), SkySystem::Galactic);
            assert_eq!(sf.title().unwrap(), "Test Sky");
            assert_eq!(sf.domain(), "CUSTOM");
            assert_close(sf.epoch_mjd(), 53000.0, 1e-10, "epoch rt");
            assert!(sf.neg_lon());
            assert_close(sf.sky_tol(), 1.0e-7, 1e-15, "sky_tol rt");
            assert_eq!(sf.projection(), Some("SIN"));
            assert!(sf.align_offset());
            assert_close(
                {
                    assert!(sf.test_obs_lat());
                    sf.obs_lat()
                },
                0.5,
                1e-10,
                "obs_lat rt",
            );
            assert_close(
                {
                    assert!(sf.test_obs_lon());
                    sf.obs_lon()
                },
                -1.5,
                1e-10,
                "obs_lon rt",
            );
            assert_close(
                {
                    assert!(sf.test_obs_alt());
                    sf.obs_alt()
                },
                2800.0,
                1e-10,
                "obs_alt rt",
            );
            assert_close(
                {
                    assert!(sf.test_dtai());
                    sf.dtai()
                },
                37.0,
                1e-10,
                "dtai rt",
            );
            assert_close(
                {
                    assert!(sf.test_dut1());
                    sf.dut1()
                },
                0.3,
                1e-10,
                "dut1 rt",
            );
        }
        _ => panic!("expected SkyFrame variant"),
    }
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
    original.set_projection("TAN");

    let text1 = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    let restored = from_native_string(&text1).unwrap();
    let text2 = native_dump(&restored).unwrap();
    assert_eq!(text1, text2, "double round-trip text mismatch");
}

#[test]
fn serialise_skyframe_geometry_preserved() {
    // Verify the round-tripped frame still computes distance correctly.
    let original = SkyFrame::new(SkySystem::Fk5);
    let text = native_dump(&AnyObject::SkyFrame(original)).unwrap();
    let restored = from_native_string(&text).unwrap();
    let frame = restored.as_frame().unwrap();

    let d = frame
        .distance(&[0.0, FRAC_PI_2], &[0.0, -FRAC_PI_2])
        .unwrap();
    assert_close(d, PI, 1.0e-10, "serialised distance");
}

#[test]
fn serialise_skyframe_not_a_mapping() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let obj = AnyObject::SkyFrame(sky);
    assert!(obj.as_mapping().is_none());
    assert!(obj.as_frame().is_some());
}

#[test]
fn serialise_frameset_with_skyframe() {
    // SkyFrame inside a FrameSet survives serialisation.
    let base: Box<dyn Frame> = Box::new(BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap());
    let current: Box<dyn Frame> = Box::new(SkyFrame::new(SkySystem::Fk5));
    let mut fs = FrameSet::new(base);
    let z: DynMapping = Box::new(ZoomMap::new(2, 0.01).unwrap());
    fs.add_frame(FrameRef::Base, z, current).unwrap();

    let text = native_dump(&AnyObject::FrameSet(fs)).unwrap();
    assert!(
        text.contains("Begin SkyFrame"),
        "missing SkyFrame in FrameSet"
    );

    let restored = from_native_string(&text).unwrap();
    let mapping = restored.as_mapping().unwrap();

    let pts = PointSet::from_rows([[100.0, 200.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert_close(out.point(0)[0], 1.0, 1e-10, "fs skyframe x");
    assert_close(out.point(0)[1], 2.0, 1e-10, "fs skyframe y");
}

// ===========================================================================
// 8. BAD_VALUE Handling
// ===========================================================================

#[test]
fn distance_with_bad_input() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let d = sky.distance(&[BAD_VALUE, 0.0], &[1.0, 0.5]).unwrap();
    assert_eq!(
        d, BAD_VALUE,
        "distance with bad input should return BAD_VALUE"
    );
}

#[test]
fn distance_with_bad_second_point() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let d = sky.distance(&[1.0, 0.5], &[0.0, BAD_VALUE]).unwrap();
    assert_eq!(
        d, BAD_VALUE,
        "distance with bad input b should return BAD_VALUE"
    );
}

#[test]
fn format_bad_value() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let s1 = sky.format(1, BAD_VALUE).unwrap();
    assert_eq!(s1, "<bad>", "format axis 1 BAD_VALUE");
    let s2 = sky.format(2, BAD_VALUE).unwrap();
    assert_eq!(s2, "<bad>", "format axis 2 BAD_VALUE");
}

#[test]
fn angle_with_bad_input() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let ang = sky
        .angle(&[BAD_VALUE, 0.0], &[0.0, 0.0], &[1.0, 0.0])
        .unwrap();
    assert_eq!(
        ang, BAD_VALUE,
        "angle with bad input should return BAD_VALUE"
    );
}

#[test]
fn offset_with_bad_input() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let result = sky.offset(&[BAD_VALUE, 0.0], &[1.0, 0.0], 0.5).unwrap();
    assert_eq!(
        result[0], BAD_VALUE,
        "offset with bad from should return BAD_VALUE"
    );
    assert_eq!(result[1], BAD_VALUE);
}

#[test]
fn offset_with_bad_distance() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let result = sky.offset(&[0.0, 0.0], &[1.0, 0.0], BAD_VALUE).unwrap();
    assert_eq!(
        result[0], BAD_VALUE,
        "offset with bad distance should return BAD_VALUE"
    );
    assert_eq!(result[1], BAD_VALUE);
}

#[test]
fn norm_bad_value_no_op() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let mut coords = [BAD_VALUE, 0.5];
    sky.norm(&mut coords).unwrap();
    assert_eq!(coords[0], BAD_VALUE, "norm should not touch bad longitude");
    // Latitude should also be untouched when longitude is bad.
    assert_close(
        coords[1],
        0.5,
        TOL,
        "norm should not touch lat when lon is bad",
    );
}

// ===========================================================================
// 9. Additional edge cases
// ===========================================================================

#[test]
fn clone_is_independent() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    sky.set_title("original");
    sky.set_neg_lon(true);
    let mut clone = sky.clone();
    clone.set_title("clone");
    clone.set_neg_lon(false);
    assert_eq!(sky.title().unwrap(), "original");
    assert!(sky.neg_lon());
    assert_eq!(clone.title().unwrap(), "clone");
    assert!(!clone.neg_lon());
}

#[test]
fn skyframe_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<SkyFrame>();
}

#[test]
fn system_string() {
    let sky = SkyFrame::new(SkySystem::Galactic);
    assert_eq!(sky.system_string().unwrap(), Some("GALACTIC"));

    let sky2 = SkyFrame::new(SkySystem::Fk5);
    assert_eq!(sky2.system_string().unwrap(), Some("FK5"));
}

#[test]
fn debug_formatting() {
    let sky = SkyFrame::new(SkySystem::Fk5);
    let debug_str = format!("{:?}", sky);
    assert!(
        debug_str.contains("SkyFrame"),
        "debug should contain SkyFrame"
    );
    assert!(debug_str.contains("Fk5"), "debug should contain Fk5");
}

#[test]
fn convert_multi_point() {
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let gal = SkyFrame::new(SkySystem::Galactic);
    let fs = fk5.convert_to(&gal, &[]).unwrap().unwrap();

    // Multiple points in one transform call.
    let pts = PointSet::from_rows([[0.0, 0.0], [PI, 0.0], [0.0, FRAC_PI_4]]);
    let out = fs.transform(&pts, true).unwrap();
    // All outputs should be finite.
    for i in 0..3 {
        let p = out.point(i);
        assert!(p[0].is_finite(), "point {i} lon should be finite");
        assert!(p[1].is_finite(), "point {i} lat should be finite");
    }
}

#[test]
fn ecliptic_vernal_equinox() {
    // Ecliptic (lon=0, lat=0) = vernal equinox = FK5 (RA=0, Dec=0).
    let ecl = SkyFrame::new(SkySystem::Ecliptic);
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let fs = ecl.convert_to(&fk5, &[]).unwrap().unwrap();

    // ECLEQ normalizes longitude into [0, 2*pi) via palDranrm, so a value
    // fractionally below zero wraps to ~2*pi; compare RA as an angle
    // (RA 0 and RA 2*pi are the same point).
    let (ra, dec) = transform_fwd(&fs, 0.0, 0.0);
    let ra_wrapped = (ra + PI).rem_euclid(2.0 * PI) - PI;
    assert_close(ra_wrapped, 0.0, 1.0e-6, "vernal equinox RA");
    assert_close(dec, 0.0, 1.0e-6, "vernal equinox Dec");
}

#[test]
fn galactic_north_pole_in_fk5() {
    // The north galactic pole in FK5 J2000 is approximately
    // RA = 192.86 deg, Dec = 27.13 deg.
    let gal = SkyFrame::new(SkySystem::Galactic);
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let fs = gal.convert_to(&fk5, &[]).unwrap().unwrap();

    // (l=0, b=pi/2) = north galactic pole
    let (ra, dec) = transform_fwd(&fs, 0.0, FRAC_PI_2);
    let ra_deg = ra.to_degrees().rem_euclid(360.0);
    let dec_deg = dec.to_degrees();
    assert!(
        (ra_deg - 192.86).abs() < 0.5,
        "NGP RA should be ~192.86 deg, got {ra_deg}"
    );
    assert!(
        (dec_deg - 27.13).abs() < 0.5,
        "NGP Dec should be ~27.13 deg, got {dec_deg}"
    );
}

/// `SkyFrame::new(Icrs)` sets the System, as C's `astSkyFrame("System=ICRS")`
/// does, so as a template it converts a Galactic target to ICRS, where a
/// template with the System unset keeps the target's. The values are the C
/// library's for the same calls.
#[test]
fn an_icrs_template_converts_as_the_c_library_does() {
    use ast_core::frames::{Frame, FrameRef};
    let target = SkyFrame::new(SkySystem::Galactic);
    for (template, system, want) in [
        (
            SkyFrame::new(SkySystem::Icrs),
            "ICRS",
            [4.620_574_201_798_786, 0.571_401_888_770_203_5],
        ),
        (SkyFrame::unset_system(), "GALACTIC", [1.0, 0.5]),
    ] {
        let fs = target.find_frame_template(&template, "").unwrap().unwrap();
        let result = fs.frame(FrameRef::Current).unwrap();
        assert_eq!(result.system_string().unwrap(), Some(system));
        let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
        let (x, y) = map.tran2(&[1.0], &[0.5], true).unwrap();
        assert_eq!([x[0], y[0]], want, "template System {system}");
    }
}

/// testskyframe.c Errors 0-4: two FK5 SkyFrames whose epochs differ by less
/// than SkyTol (default 0.001 arcsec of resulting shift) convert with a
/// UnitMap; a smaller SkyTol, or a larger epoch difference, does not.
#[test]
fn an_epoch_change_within_skytol_converts_with_a_unitmap() {
    use ast_core::mappings::UnitMap;
    let fk5 = |year: f64| {
        let mut f = SkyFrame::new(SkySystem::Fk5);
        f.set_epoch_year(year);
        f
    };
    let is_unit = |a: &SkyFrame, b: &SkyFrame| {
        let fs = a
            .convert_to(b, &["SKY"])
            .unwrap()
            .expect("FK5 SkyFrames convert");
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_any()
            .is::<UnitMap>()
    };
    let sf1 = fk5(2015.0);
    let mut sf2 = fk5(2015.1);
    assert!(is_unit(&sf1, &sf2), "Error 1");
    assert_eq!(sf1.sky_tol(), 0.001, "Error 2");
    sf2.set_sky_tol(1.0e-6);
    assert!(!is_unit(&sf1, &sf2), "Error 3");
    assert!(!is_unit(&sf1, &fk5(2016.6)), "Error 4");
}

/// testskyframe.c Errors 5-6: AxNorm on the longitude axis, with oper 0
/// (each value into [0, 2pi)) and oper 1 (all values into one range, here
/// shifted down by 2pi together).
#[test]
fn ax_norm_on_the_longitude_axis_matches_c() {
    let sf1 = {
        let mut f = SkyFrame::new(SkySystem::Fk5);
        f.set_epoch_year(2015.0);
        f
    };
    let mut vals = [6.1, 6.15, 6.2, 6.25, 6.3];
    sf1.ax_norm(1, 0, &mut vals).unwrap();
    assert_eq!(vals, [6.1, 6.15, 6.2, 6.25, 6.3 - 2.0 * PI], "Error 5");
    sf1.ax_norm(1, 1, &mut vals).unwrap();
    for (v, x) in vals.iter().zip([6.1, 6.15, 6.2, 6.25, 6.3]) {
        assert!((v - (x - 2.0 * PI)).abs() <= 1.0e-10, "Error 6: {vals:?}");
    }
}

// ===========================================================================
// sky_offset_map
// ===========================================================================

/// `sky_offset_map` gives the Mapping C's `astSkyOffsetMap` gives: a UnitMap
/// without an offset coordinate system, and otherwise a CmpMap whose
/// transformations match the C library's.
/// Values captured by `tools/oracle_fixtures/sky_offset_map_probe.c`.
#[test]
fn sky_offset_map_follows_the_c_library() {
    use ast_core::error::ErrorCode;
    use ast_core::mappings::{CmpMap, UnitMap};

    type Case = (
        &'static str,
        &'static [(usize, f64)],
        &'static [(usize, f64)],
        Option<SkyRefIs>,
        bool,
        bool,
        [[f64; 2]; 3],
        [[f64; 2]; 3],
    );
    const INPUT: [[f64; 2]; 3] = [[1.0, 0.5], [1.2, -0.3], [4.0, 1.1]];
    const CASES: &[Case] = &[
        (
            "ignored",
            &[(1, 1.0), (2, 0.5)],
            &[],
            Some(SkyRefIs::Ignored),
            false,
            true,
            INPUT,
            INPUT,
        ),
        (
            "unset",
            &[],
            &[],
            Some(SkyRefIs::Origin),
            false,
            true,
            INPUT,
            INPUT,
        ),
        (
            "origin",
            &[(1, 1.0), (2, 0.5)],
            &[],
            Some(SkyRefIs::Origin),
            false,
            false,
            [
                [-1.1102230246251563e-16, 0.0],
                [0.27218691839587611, -0.78698270081011723],
                [1.0925418109024947, 1.4986324283930401],
            ],
            [
                [2.3237151508126428, 0.70503416433600519],
                [2.1069058376653653, -0.09351516272737756],
                [-1.6784670157476533, 0.69445018981625195],
            ],
        ),
        (
            "origin_refp",
            &[(1, 1.0), (2, 0.5)],
            &[(1, 0.2), (2, 1.2)],
            Some(SkyRefIs::Origin),
            false,
            false,
            [
                [5.5511151231257815e-17, 1.1102230246251563e-16],
                [-0.10210991864465951, -0.81817389036490573],
                [1.4897499824922824, 1.1484701853536217],
            ],
            [
                [2.4249301423352168, 1.0122189985077705],
                [2.2733863215807153, 0.19733240519296935],
                [-1.331861624286645, 0.50678292502143429],
            ],
        ),
        (
            "pole",
            &[(1, 1.0), (2, 0.5)],
            &[],
            Some(SkyRefIs::Pole),
            false,
            false,
            [
                [0.0, 1.5707963267948966],
                [-2.879757185700063, 0.74775553502988812],
                [-0.064090627261980476, 0.033189282448934675],
            ],
            [
                [-0.31463876459691453, 0.7022845055472241],
                [-1.0164073910259139, 0.16283461017645623],
                [1.3556251391076843, 0.16786011926334668],
            ],
        ),
        (
            "origin_lat_only",
            &[(2, 0.5)],
            &[],
            Some(SkyRefIs::Origin),
            false,
            false,
            [
                [0.85211163828032066, 0.19463774791986274],
                [1.3907003322727278, -0.4393019920225063],
                [-1.1178516922472144, 1.179074360848845],
            ],
            [
                [1.323715150812643, 0.70503416433600496],
                [1.1069058376653655, -0.093515162727377643],
                [-2.6784670157476533, 0.69445018981625206],
            ],
        ),
        // SkyRef is set before the axes are swapped, so it is the same
        // position as "origin", and the Mapping is unchanged by the swap:
        // it works in (longitude, latitude) order.
        (
            "permuted_origin",
            &[(1, 1.0), (2, 0.5)],
            &[],
            Some(SkyRefIs::Origin),
            true,
            false,
            [
                [-1.1102230246251563e-16, 0.0],
                [0.27218691839587611, -0.78698270081011723],
                [1.0925418109024947, 1.4986324283930401],
            ],
            [
                [2.3237151508126428, 0.70503416433600519],
                [2.1069058376653653, -0.09351516272737756],
                [-1.6784670157476533, 0.69445018981625195],
            ],
        ),
    ];
    let input = PointSet::from_rows(INPUT);
    for &(name, sky_ref, sky_ref_p, is, permute, unit, fwd, inv) in CASES {
        let mut sky = SkyFrame::new(SkySystem::Icrs);
        for &(axis, v) in sky_ref {
            sky.set_sky_ref(axis, v).unwrap();
        }
        for &(axis, v) in sky_ref_p {
            sky.set_sky_ref_p(axis, v).unwrap();
        }
        if let Some(is) = is {
            sky.set_sky_ref_is(is);
        }
        if permute {
            sky.perm_axes(&[2, 1]).unwrap();
        }
        let map = sky.sky_offset_map().unwrap();
        if unit {
            assert!(map.as_ref().as_any().is::<UnitMap>(), "{name}: a UnitMap");
        } else {
            assert!(map.as_ref().as_any().is::<CmpMap>(), "{name}: a CmpMap");
        }
        for (forward, want) in [(true, fwd), (false, inv)] {
            let out = map.transform(&input, forward).unwrap();
            for (i, w) in want.iter().enumerate() {
                crate::common::assert_c_values(
                    &out.point(i),
                    w,
                    &format!("{name} forward={forward} point {i}"),
                );
            }
        }
    }

    // A SkyRefP that coincides with SkyRef defines no orientation.
    let mut sky = SkyFrame::new(SkySystem::Icrs);
    for axis in [1, 2] {
        let v = [1.0, 0.5][axis - 1];
        sky.set_sky_ref(axis, v).unwrap();
        sky.set_sky_ref_p(axis, v).unwrap();
    }
    sky.set_sky_ref_is(SkyRefIs::Origin);
    let err = sky.sky_offset_map().unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badoc);
}

/// System and SkyRefIs names match as C's `astChrMatch` matches them: case
/// is ignored, and so is white space after the name, but not before it or
/// within it.
#[test]
fn sky_names_ignore_trailing_white_space() {
    assert_eq!(SkySystem::parse("FK5 "), Some(SkySystem::Fk5));
    assert_eq!(SkySystem::parse("galactic\t\n"), Some(SkySystem::Galactic));
    assert_eq!(
        SkySystem::parse("fk4-no-e\x0b\x0c\r"),
        Some(SkySystem::Fk4NoE)
    );
    assert_eq!(SkySystem::parse(" FK5"), None);
    assert_eq!(SkySystem::parse("FK5 x"), None);
    assert_eq!(SkyRefIs::parse("pole  "), Some(SkyRefIs::Pole));
    assert_eq!(SkyRefIs::parse(" Pole"), None);
}

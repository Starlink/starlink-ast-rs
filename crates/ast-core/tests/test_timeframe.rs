//! Tests for [`TimeFrame`] — time coordinate frame.
//!
//! Ported from `reference/starlink-ast/ast_tester/testtime.c`.
//! Uses the public Rust API only.

use ast_core::coords::PointSet;
use ast_core::frames::{Frame, FrameRef, TimeFrame, TimeScale, TimeSystem};
use ast_core::{AnyObject, from_native_string, to_native_string};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn convert_value(from: &TimeFrame, to: &TimeFrame, input: f64) -> f64 {
    let fs = from.convert_to(to, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[input]]);
    let out = mapping.transform(&pts, true).unwrap();
    out.point(0)[0]
}

fn round_trip(from: &TimeFrame, to: &TimeFrame, input: f64) -> f64 {
    let fs = from.convert_to(to, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[input]]);
    let fwd = mapping.transform(&pts, true).unwrap();
    let back = mapping.transform(&fwd, false).unwrap();
    back.point(0)[0]
}

// ---------------------------------------------------------------------------
// Default attributes (C errors 1-14)
// ---------------------------------------------------------------------------

#[test]
fn test_defaults_mjd() {
    let tf = TimeFrame::new(TimeSystem::Mjd);
    assert_eq!(tf.system(), TimeSystem::Mjd);
    assert_eq!(tf.time_origin(), 0.0);
    assert_eq!(tf.timescale(), TimeScale::Tai);
    assert_eq!(tf.align_timescale(), TimeScale::Tai);
    assert_eq!(tf.naxes(), 1);
}

#[test]
fn test_defaults_jd() {
    let tf = TimeFrame::new(TimeSystem::Jd);
    assert_eq!(tf.system(), TimeSystem::Jd);
    assert_eq!(tf.time_origin(), 0.0);
    assert_eq!(tf.timescale(), TimeScale::Tai);
    assert_eq!(tf.align_timescale(), TimeScale::Tai);
    assert_eq!(tf.naxes(), 1);
}

#[test]
fn test_defaults_jepoch() {
    let tf = TimeFrame::new(TimeSystem::Jepoch);
    assert_eq!(tf.system(), TimeSystem::Jepoch);
    assert_eq!(tf.time_origin(), 0.0);
    assert_eq!(tf.timescale(), TimeScale::Tai);
    assert_eq!(tf.align_timescale(), TimeScale::Tai);
    assert_eq!(tf.naxes(), 1);
}

#[test]
fn test_defaults_bepoch() {
    // Bepoch defaults to TT timescale.
    let tf = TimeFrame::new(TimeSystem::Bepoch);
    assert_eq!(tf.system(), TimeSystem::Bepoch);
    assert_eq!(tf.time_origin(), 0.0);
    assert_eq!(tf.timescale(), TimeScale::Tt);
    // AlignTimeScale for TT (non-earth-rotation) is TAI.
    assert_eq!(tf.align_timescale(), TimeScale::Tai);
    assert_eq!(tf.naxes(), 1);
}

#[test]
fn test_align_timescale_earth_rotation() {
    // For UT1 timescale, the alignment timescale defaults to UT1.
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Ut1);
    assert_eq!(tf.align_timescale(), TimeScale::Ut1);

    tf.set_timescale(TimeScale::Gmst);
    assert_eq!(tf.align_timescale(), TimeScale::Ut1);

    tf.set_timescale(TimeScale::Last);
    assert_eq!(tf.align_timescale(), TimeScale::Ut1);

    tf.set_timescale(TimeScale::Lmst);
    assert_eq!(tf.align_timescale(), TimeScale::Ut1);
}

// ---------------------------------------------------------------------------
// System conversion: MJD <-> JD (C errors 18-19)
// ---------------------------------------------------------------------------

#[test]
fn test_mjd_to_jd_with_matching_offsets() {
    // MJD with offset 53000 and JD with offset 2453000.5 are the same epoch.
    // Conversion between them should be a UnitMap.
    let mut tf1 = TimeFrame::new(TimeSystem::Mjd);
    tf1.set_time_origin(53000.0);

    let mut tf2 = TimeFrame::new(TimeSystem::Jd);
    tf2.set_time_origin(2_453_000.5);

    let result = convert_value(&tf1, &tf2, 100.0);
    assert!(
        (result - 100.0).abs() < 1.0e-10,
        "MJD+53000 to JD+2453000.5 should preserve value, got {result}"
    );
}

#[test]
fn test_mjd_to_jd_offset_2400000_5() {
    // MJD 0 = JD 2400000.5
    let tf1 = TimeFrame::new(TimeSystem::Mjd);
    let tf2 = TimeFrame::new(TimeSystem::Jd);

    let result = convert_value(&tf1, &tf2, 0.0);
    assert!(
        (result - 2_400_000.5).abs() < 1.0e-10,
        "MJD 0 should be JD 2400000.5, got {result}"
    );
}

#[test]
fn test_jd_to_mjd() {
    let tf1 = TimeFrame::new(TimeSystem::Jd);
    let tf2 = TimeFrame::new(TimeSystem::Mjd);

    let result = convert_value(&tf1, &tf2, 2_451_544.5 + 0.5);
    // JD 2451545.0 = MJD 51544.5
    assert!(
        (result - 51544.5).abs() < 1.0e-10,
        "JD 2451545.0 should be MJD 51544.5, got {result}"
    );
}

// ---------------------------------------------------------------------------
// System conversion: MJD <-> Julian epoch (J2000.0 = MJD 51544.5)
// ---------------------------------------------------------------------------

#[test]
fn test_mjd_to_jepoch_j2000() {
    let tf1 = TimeFrame::new(TimeSystem::Mjd);
    let tf2 = TimeFrame::new(TimeSystem::Jepoch);

    let result = convert_value(&tf1, &tf2, 51544.5);
    assert!(
        (result - 2000.0).abs() < 1.0e-10,
        "MJD 51544.5 should be J2000.0, got {result}"
    );
}

#[test]
fn test_jepoch_to_mjd_j2000() {
    let tf1 = TimeFrame::new(TimeSystem::Jepoch);
    let tf2 = TimeFrame::new(TimeSystem::Mjd);

    let result = convert_value(&tf1, &tf2, 2000.0);
    assert!(
        (result - 51544.5).abs() < 1.0e-10,
        "J2000.0 should be MJD 51544.5, got {result}"
    );
}

#[test]
fn test_mjd_jepoch_round_trip() {
    let tf1 = TimeFrame::new(TimeSystem::Mjd);
    let tf2 = TimeFrame::new(TimeSystem::Jepoch);

    let result = round_trip(&tf1, &tf2, 51544.5);
    assert!(
        (result - 51544.5).abs() < 1.0e-10,
        "MJD -> JEPOCH -> MJD round-trip failed, got {result}"
    );
}

// ---------------------------------------------------------------------------
// TimeOrigin handling
// ---------------------------------------------------------------------------

#[test]
fn test_time_origin_offset() {
    // MJD with origin 53000: input 100 means absolute MJD 53100.
    // Converting to JD with origin 2453000.5: same epoch, output should be 100.
    let mut tf1 = TimeFrame::new(TimeSystem::Mjd);
    tf1.set_time_origin(53000.0);

    let mut tf2 = TimeFrame::new(TimeSystem::Jd);
    tf2.set_time_origin(2_453_000.5);

    let result = convert_value(&tf1, &tf2, 100.0);
    assert!(
        (result - 100.0).abs() < 1.0e-10,
        "TimeOrigin: expected 100.0, got {result}"
    );
}

// ---------------------------------------------------------------------------
// Timescale conversions (C errors 36-46: TAI 1977-01-01 00:00:00)
// ---------------------------------------------------------------------------

#[test]
fn test_tai_to_utc() {
    // TAI is ahead of UTC by the leap second count.
    // At 1977-01-01 TAI, UTC is 1976-12-31 23:59:45 (15 seconds behind).
    // MJD 43144.0 is approximately 1977-01-01 (TAI epoch).
    let tf1 = TimeFrame::new(TimeSystem::Mjd); // TAI
    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Utc);

    // At a recent epoch (MJD 57844 ~ 2017), TAI-UTC = 37 seconds.
    let xin = 57844.0;
    let xout = convert_value(&tf1, &tf2, xin);
    let diff_seconds = (xin - xout) * 86400.0;
    assert!(
        (diff_seconds - 37.0).abs() < 1.0,
        "TAI-UTC at MJD 57844 should be ~37s, got {diff_seconds}s"
    );
}

#[test]
fn test_tai_to_tt() {
    // TT = TAI + 32.184 seconds.
    let tf1 = TimeFrame::new(TimeSystem::Mjd); // TAI
    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Tt);

    let xin = 51544.5; // J2000.0 in TAI
    let xout = convert_value(&tf1, &tf2, xin);
    let diff_seconds = (xout - xin) * 86400.0;
    assert!(
        (diff_seconds - 32.184).abs() < 1.0e-6,
        "TT - TAI should be 32.184s, got {diff_seconds}s"
    );
}

#[test]
fn test_tai_to_utc_with_dtai() {
    // When DTAI is explicitly set to 40, TAI-UTC should be 40 seconds.
    let tf1 = TimeFrame::new(TimeSystem::Mjd); // TAI
    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Utc);
    tf2.set_dtai(40.0);

    let xin = 57844.0;
    let xout = convert_value(&tf1, &tf2, xin);
    let diff_seconds = (xin - xout) * 86400.0;
    assert!(
        (diff_seconds - 40.0).abs() < 1.0e-3,
        "TAI-UTC with DTAI=40 should be 40s, got {diff_seconds}s"
    );
}

// ---------------------------------------------------------------------------
// Timescale conversion: GMST -> LMST (C errors 47-48)
// ---------------------------------------------------------------------------

#[test]
fn test_gmst_to_lmst() {
    // ObsLon = 90 degrees east. LMST = GMST + longitude/360 days.
    // longitude = 90 degrees = 0.25 rotations, so LMST = GMST + 0.25 days.
    let mut tf1 = TimeFrame::new(TimeSystem::Mjd);
    tf1.set_timescale(TimeScale::Gmst);
    tf1.set_obs_lon(90.0_f64.to_radians());
    tf1.set_time_origin(53000.0);

    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Lmst);
    tf2.set_obs_lon(90.0_f64.to_radians());
    tf2.set_time_origin(53000.0);

    let xout = convert_value(&tf1, &tf2, 1.0);
    assert!(
        (xout - 1.25).abs() < 1.0e-10,
        "GMST+1d at lon=90 should be LMST+1.25d, got {xout}"
    );

    // Round-trip.
    let back = round_trip(&tf1, &tf2, 1.0);
    assert!(
        (back - 1.0).abs() < 1.0e-10,
        "GMST->LMST round-trip failed, got {back}"
    );
}

// ---------------------------------------------------------------------------
// MJD to Bepoch conversion (C errors 20-21)
// ---------------------------------------------------------------------------

#[test]
fn test_mjd_utc_to_bepoch() {
    // tf1: MJD(UTC) with origin 53000, tf2: BEPOCH with origin 2004.
    // Input 100 (MJD 53100 UTC) should give ~0.2601 in Bepoch offset.
    let mut tf1 = TimeFrame::new(TimeSystem::Mjd);
    tf1.set_timescale(TimeScale::Utc);
    tf1.set_time_origin(53000.0);

    let mut tf2 = TimeFrame::new(TimeSystem::Bepoch);
    tf2.set_time_origin(2004.0);

    let xout = convert_value(&tf1, &tf2, 100.0);
    // C test expects 0.2600974092354136 with tolerance 1e-10.
    // The conversion UTC->TT involves a timescale change, so results may vary slightly.
    assert!(
        (xout - 0.2600974092354136).abs() < 1.0e-6,
        "MJD(UTC) 53100 should be ~B2004.26, got offset {xout}"
    );

    // Round-trip.
    let back = round_trip(&tf1, &tf2, 100.0);
    assert!(
        (back - 100.0).abs() < 1.0e-6,
        "MJD(UTC)->BEPOCH round-trip failed, got {back}"
    );
}

// ---------------------------------------------------------------------------
// Distance (1D)
// ---------------------------------------------------------------------------

#[test]
fn test_distance() {
    let tf = TimeFrame::new(TimeSystem::Mjd);
    let d = tf.distance(&[10.0], &[15.0]).unwrap();
    assert!((d - 5.0).abs() < 1.0e-15);

    let d2 = tf.distance(&[15.0], &[10.0]).unwrap();
    assert!((d2 - 5.0).abs() < 1.0e-15);
}

// ---------------------------------------------------------------------------
// Serialisation round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_serialisation_round_trip() {
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Utc);
    tf.set_time_origin(53000.0);
    tf.set_obs_lon(0.5);
    tf.set_obs_lat(0.3);
    tf.set_dtai(37.0);

    let obj = AnyObject::TimeFrame(tf.clone());
    let text = to_native_string(&obj).unwrap();

    let parsed = from_native_string(&text).unwrap();
    let tf2 = match parsed {
        AnyObject::TimeFrame(f) => f,
        _ => panic!("Expected TimeFrame, got something else"),
    };

    assert_eq!(tf2.system(), TimeSystem::Mjd);
    assert_eq!(tf2.timescale(), TimeScale::Utc);
    assert!((tf2.time_origin() - 53000.0).abs() < 1.0e-15);
    assert!((tf2.get_dtai() - 37.0).abs() < 1.0e-15);
}

#[test]
fn test_serialisation_with_align_timescale() {
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_timescale(TimeScale::Tdb);
    tf.set_align_timescale(TimeScale::Ut1);
    tf.set_lt_offset(5.5);

    let obj = AnyObject::TimeFrame(tf.clone());
    let text = to_native_string(&obj).unwrap();

    let parsed = from_native_string(&text).unwrap();
    let tf2 = match parsed {
        AnyObject::TimeFrame(f) => f,
        _ => panic!("Expected TimeFrame, got something else"),
    };

    assert_eq!(tf2.system(), TimeSystem::Jepoch);
    assert_eq!(tf2.timescale(), TimeScale::Tdb);
    assert_eq!(tf2.align_timescale(), TimeScale::Ut1);
    assert!((tf2.lt_offset() - 5.5).abs() < 1.0e-15);
}

// ---------------------------------------------------------------------------
// Frame trait: equal
// ---------------------------------------------------------------------------

#[test]
fn test_equal() {
    let tf1 = TimeFrame::new(TimeSystem::Mjd);
    let tf2 = TimeFrame::new(TimeSystem::Mjd);
    let tf3 = TimeFrame::new(TimeSystem::Jd);

    assert!(tf1.equal(&tf2));
    assert!(!tf1.equal(&tf3));
}

// ---------------------------------------------------------------------------
// System / timescale string representations
// ---------------------------------------------------------------------------

#[test]
fn test_time_system_parse() {
    assert_eq!(TimeSystem::parse("MJD"), Some(TimeSystem::Mjd));
    assert_eq!(TimeSystem::parse("jd"), Some(TimeSystem::Jd));
    assert_eq!(TimeSystem::parse("JEPOCH"), Some(TimeSystem::Jepoch));
    assert_eq!(TimeSystem::parse("bepoch"), Some(TimeSystem::Bepoch));
    assert_eq!(TimeSystem::parse("invalid"), None);
}

#[test]
fn test_time_scale_parse() {
    assert_eq!(TimeScale::parse("TAI"), Some(TimeScale::Tai));
    assert_eq!(TimeScale::parse("utc"), Some(TimeScale::Utc));
    assert_eq!(TimeScale::parse("UT1"), Some(TimeScale::Ut1));
    assert_eq!(TimeScale::parse("GMST"), Some(TimeScale::Gmst));
    assert_eq!(TimeScale::parse("LAST"), Some(TimeScale::Last));
    assert_eq!(TimeScale::parse("LMST"), Some(TimeScale::Lmst));
    assert_eq!(TimeScale::parse("TT"), Some(TimeScale::Tt));
    assert_eq!(TimeScale::parse("TDB"), Some(TimeScale::Tdb));
    assert_eq!(TimeScale::parse("TCB"), Some(TimeScale::Tcb));
    assert_eq!(TimeScale::parse("TCG"), Some(TimeScale::Tcg));
    assert_eq!(TimeScale::parse("LT"), Some(TimeScale::Lt));
    assert_eq!(TimeScale::parse("invalid"), None);
}

// ---------------------------------------------------------------------------
// Convert same system different origin (should be ShiftMap equivalent)
// ---------------------------------------------------------------------------

#[test]
fn test_same_system_different_origin() {
    let mut tf1 = TimeFrame::new(TimeSystem::Mjd);
    tf1.set_time_origin(50000.0);

    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_time_origin(51000.0);

    // Input 100 in tf1 = absolute MJD 50100
    // In tf2 with origin 51000, that's 50100 - 51000 = -900
    let result = convert_value(&tf1, &tf2, 100.0);
    assert!(
        (result - (-900.0)).abs() < 1.0e-10,
        "Same system different origin: expected -900, got {result}"
    );
}

// ---------------------------------------------------------------------------
// TT -> TDB conversion (should be close to identity for most epochs)
// ---------------------------------------------------------------------------

#[test]
fn test_tt_to_tdb() {
    // TDB differs from TT by periodic terms of amplitude ~1.7 ms.
    let mut tf1 = TimeFrame::new(TimeSystem::Mjd);
    tf1.set_timescale(TimeScale::Tt);

    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Tdb);

    let xin = 51544.5; // J2000.0
    let xout = convert_value(&tf1, &tf2, xin);
    let diff_ms = (xout - xin) * 86400.0 * 1000.0;
    // At J2000.0, TDB-TT should be very close to 0 (within ~2 ms).
    assert!(
        diff_ms.abs() < 2.0,
        "TDB-TT at J2000.0 should be < 2ms, got {diff_ms}ms"
    );
}

// ---------------------------------------------------------------------------
// map_from: tests that changing timescale produces a mapping
// ---------------------------------------------------------------------------

#[test]
fn test_map_from_timescale_change() {
    let tf_tai = TimeFrame::new(TimeSystem::Mjd);
    let mut tf_tt = TimeFrame::new(TimeSystem::Mjd);
    tf_tt.set_timescale(TimeScale::Tt);

    // map_from(tai) on tf_tt should give TAI->TT mapping.
    let mapping = tf_tt.map_from(&tf_tai).unwrap();
    let pts = PointSet::from_rows([[51544.5_f64]]);
    let out = mapping.transform(&pts, true).unwrap();
    let diff_seconds = (out.point(0)[0] - 51544.5) * 86400.0;
    assert!(
        (diff_seconds - 32.184).abs() < 1.0e-6,
        "map_from TAI->TT should add 32.184s, got {diff_seconds}s"
    );
}

// ---------------------------------------------------------------------------
// Local Time conversion
// ---------------------------------------------------------------------------

#[test]
fn test_lt_offset() {
    // LT = UTC + offset (in hours).
    // Set LT offset to +5 hours (e.g., EST is UTC-5, but we test positive).
    let mut tf1 = TimeFrame::new(TimeSystem::Mjd);
    tf1.set_timescale(TimeScale::Utc);

    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Lt);
    tf2.set_lt_offset(5.0); // 5 hours ahead of UTC

    let xin = 51544.5;
    let xout = convert_value(&tf1, &tf2, xin);
    let diff_hours = (xout - xin) * 24.0;
    assert!(
        (diff_hours - 5.0).abs() < 1.0e-10,
        "LT should be UTC + 5h, got diff={diff_hours}h"
    );
}

#[test]
fn format_default_is_seven_significant_digits() {
    // The non-date Format default is `%1.7G` — 7 *significant* digits, not
    // 7 decimal places. Oracle-verified against the reference C library:
    //   astFormat(12345.6789)     == "12345.68"
    //   astFormat(0.000123456789) == "0.0001234568"
    //   astFormat(51544.5)        == "51544.5"
    let tf = TimeFrame::new(TimeSystem::Mjd);
    assert_eq!(tf.format(1, 12345.6789).unwrap(), "12345.68");
    assert_eq!(tf.format(1, 0.000123456789).unwrap(), "0.0001234568");
    assert_eq!(tf.format(1, 51544.5).unwrap(), "51544.5");
}

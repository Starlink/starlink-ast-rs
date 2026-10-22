//! Tests for [`TimeFrame`] — time coordinate frame.
//!
//! Ported from `reference/starlink-ast/ast_tester/testtime.c`.
//! Uses the public Rust API only.

use crate::common::native_dump;
use ast_core::coords::PointSet;
use ast_core::frames::{Frame, FrameRef, TimeFrame, TimeScale, TimeSystem};
use ast_core::{AnyObject, from_native_string};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn convert_value(from: &TimeFrame, to: &TimeFrame, input: f64) -> f64 {
    let fs = from
        .clone()
        .convert_to(&mut to.clone(), &[])
        .unwrap()
        .unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[input]]);
    let out = mapping.transform(&pts, true).unwrap();
    out.point(0)[0]
}

fn round_trip(from: &TimeFrame, to: &TimeFrame, input: f64) -> f64 {
    let fs = from
        .clone()
        .convert_to(&mut to.clone(), &[])
        .unwrap()
        .unwrap();
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
    tf.set_timescale(TimeScale::Ut1).unwrap();
    assert_eq!(tf.align_timescale(), TimeScale::Ut1);

    tf.set_timescale(TimeScale::Gmst).unwrap();
    assert_eq!(tf.align_timescale(), TimeScale::Ut1);

    tf.set_timescale(TimeScale::Last).unwrap();
    assert_eq!(tf.align_timescale(), TimeScale::Ut1);

    tf.set_timescale(TimeScale::Lmst).unwrap();
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
    tf2.set_timescale(TimeScale::Utc).unwrap();

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
    tf2.set_timescale(TimeScale::Tt).unwrap();

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
    tf2.set_timescale(TimeScale::Utc).unwrap();
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
    tf1.set_timescale(TimeScale::Gmst).unwrap();
    tf1.set_obs_lon(90.0_f64.to_radians());
    tf1.set_time_origin(53000.0);

    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Lmst).unwrap();
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
    tf1.set_timescale(TimeScale::Utc).unwrap();
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
// Serialization round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_serialisation_round_trip() {
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Utc).unwrap();
    tf.set_time_origin(53000.0);
    tf.set_obs_lon(0.5);
    tf.set_obs_lat(0.3);
    tf.set_dtai(37.0);

    let obj = AnyObject::TimeFrame(tf.clone());
    let text = native_dump(&obj).unwrap();

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
    tf.set_timescale(TimeScale::Tdb).unwrap();
    tf.set_align_timescale(TimeScale::Ut1);
    tf.set_lt_offset(5.5);

    let obj = AnyObject::TimeFrame(tf.clone());
    let text = native_dump(&obj).unwrap();

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

#[test]
fn test_serialisation_with_unit_and_format() {
    // Unit and Format are stored in the Ax1 Axis node and must round-trip.
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Tai).unwrap();
    tf.set_unit(1, "s").unwrap();
    tf.set_format_str(1, "iso.2").unwrap();

    let obj = AnyObject::TimeFrame(tf);
    let text = native_dump(&obj).unwrap();
    // The Axis node carries the active Unit and Format lines.
    assert!(text.contains("Unit = \"s\""), "missing Unit: {text}");
    assert!(
        text.contains("Format = \"iso.2\""),
        "missing Format: {text}"
    );
    // A date Format relabels the axis "Date/Time".
    assert!(
        text.contains("Lbl1 = \"Date/Time\""),
        "missing label: {text}"
    );

    let parsed = from_native_string(&text).unwrap();
    let tf2 = match parsed {
        AnyObject::TimeFrame(f) => f,
        _ => panic!("Expected TimeFrame"),
    };
    assert_eq!(tf2.unit(1).unwrap(), "s");
    assert!(tf2.test_format_str(1).unwrap());
    assert_eq!(tf2.format_str(1).unwrap(), "iso.2");
}

// ---------------------------------------------------------------------------
// Default Title (timeframe.c GetTitle)
// ---------------------------------------------------------------------------

/// Extract the default (commented, un-set) Title from a TimeFrame's native
/// dump. The line looks like: `#   Title = "..." \t# Title of ...`.
fn default_title(tf: TimeFrame) -> String {
    let text = native_dump(&AnyObject::TimeFrame(tf)).unwrap();
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("#   Title = \"") {
            if let Some(end) = rest.find('"') {
                return rest[..end].to_string();
            }
        }
    }
    panic!("no default Title line in dump:\n{text}");
}

#[test]
fn test_default_title_system_only() {
    // No timescale set: just the system label (matches C astGetC(Title)).
    assert_eq!(
        default_title(TimeFrame::new(TimeSystem::Mjd)),
        "Modified Julian Date"
    );
    // Bepoch never shows a timescale (TT-only).
    assert_eq!(
        default_title(TimeFrame::new(TimeSystem::Bepoch)),
        "Besselian Epoch"
    );
}

#[test]
fn test_default_title_with_timescale() {
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Tai).unwrap();
    assert_eq!(default_title(tf), "Modified Julian Date [TAI]");
}

#[test]
fn test_default_title_offset_from_date() {
    // jepoch origin 2005.0 -> "offset from 2004-12-31 18:00:00" (J2005.0).
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_time_origin(2005.0);
    assert_eq!(
        default_title(tf),
        "Julian Epoch offset from 2004-12-31 18:00:00"
    );

    // mjd/utc origin 55000 -> "[UTC] offset from 2009-06-18 00:00:00".
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Utc).unwrap();
    tf.set_time_origin(55000.0);
    assert_eq!(
        default_title(tf),
        "Modified Julian Date [UTC] offset from 2009-06-18 00:00:00"
    );
}

#[test]
fn test_default_title_local_time_offset() {
    // Local Time shows the UTC offset in the timescale brackets.
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Lt).unwrap();
    tf.set_lt_offset(5.5);
    tf.set_time_origin(55000.0);
    assert_eq!(
        default_title(tf),
        "Modified Julian Date [LT (UTC+5.5)] offset from 2009-06-18 00:00:00"
    );
}

#[test]
fn test_default_title_date_format_suppresses_offset() {
    // When the Format is itself a date string, the "offset from" suffix is
    // omitted (the date Format is absolute).
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Tai).unwrap();
    tf.set_time_origin(55000.0);
    tf.set_format_str(1, "iso.0").unwrap();
    assert_eq!(default_title(tf), "Modified Julian Date [TAI]");
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

/// C's `SystemCode` (timeframe.c) also accepts each System's descriptive
/// name, ignoring case, as a whole name.
#[test]
fn time_system_parse_accepts_c_descriptive_names() {
    assert_eq!(
        TimeSystem::parse("Modified Julian Date"),
        Some(TimeSystem::Mjd)
    );
    assert_eq!(TimeSystem::parse("julian date"), Some(TimeSystem::Jd));
    assert_eq!(
        TimeSystem::parse("Besselian Epoch"),
        Some(TimeSystem::Bepoch)
    );
    assert_eq!(TimeSystem::parse("JULIAN EPOCH"), Some(TimeSystem::Jepoch));
    assert_eq!(TimeSystem::parse("Julian"), None);
}

/// C's `SystemCode` (timeframe.c) rejects the abbreviations "JEP" and
/// "BEP", which are only the default axis Symbols, and ignores trailing
/// white space, as `astChrMatch` does.
#[test]
fn time_system_parse_matches_c_names_only() {
    assert_eq!(TimeSystem::parse("JEP"), None);
    assert_eq!(TimeSystem::parse("bep"), None);
    assert_eq!(TimeSystem::parse("MJD  "), Some(TimeSystem::Mjd));
    assert_eq!(TimeSystem::parse(" MJD"), None);
}

/// C's `TimeScaleCode` (timeframe.c) matches with `astChrMatch` too.
#[test]
fn time_scale_parse_ignores_trailing_white_space() {
    assert_eq!(TimeScale::parse("tai "), Some(TimeScale::Tai));
    assert_eq!(TimeScale::parse(" TAI"), None);
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
// Per-instance Unit support (C MakeMap unit bracketing)
// ---------------------------------------------------------------------------

#[test]
fn test_unit_accessors() {
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    assert!(!tf.test_unit(1).unwrap());
    assert_eq!(tf.unit_or_default(), "d");
    tf.set_unit(1, "s").unwrap();
    assert!(tf.test_unit(1).unwrap());
    assert_eq!(tf.unit(1).unwrap(), "s");
    assert_eq!(tf.unit_or_default(), "s");
    tf.clear_unit(1).unwrap();
    assert!(!tf.test_unit(1).unwrap());
    assert_eq!(tf.unit_or_default(), "d");
}

#[test]
fn test_convert_seconds_to_days() {
    // A value in seconds converts to the same instant in days: the unit
    // bracketing in MakeMap scales by 86400 (same System/TimeScale/origin).
    let mut tf_s = TimeFrame::new(TimeSystem::Mjd);
    tf_s.set_unit(1, "s").unwrap();
    let tf_d = TimeFrame::new(TimeSystem::Mjd);

    // Both have origin 0, so 86400 s == 1 d (offset from the MJD origin).
    let out = convert_value(&tf_s, &tf_d, 86400.0);
    assert!(
        (out - 1.0).abs() < 1.0e-9,
        "86400 s should convert to 1 d, got {out}"
    );

    // Round-trip back to seconds.
    let back = round_trip(&tf_s, &tf_d, 86400.0);
    assert!((back - 86400.0).abs() < 1.0e-6, "round-trip failed: {back}");
}

#[test]
fn test_convert_seconds_default_units_unchanged() {
    // Regression guard: with default units (d) the convert path is the
    // identity for matching frames.
    let tf1 = TimeFrame::new(TimeSystem::Mjd);
    let tf2 = TimeFrame::new(TimeSystem::Mjd);
    let out = convert_value(&tf1, &tf2, 12345.0);
    assert!((out - 12345.0).abs() < 1.0e-12);
}

// ---------------------------------------------------------------------------
// iso-date Format / Unformat (testtime.c errors 26-34)
// ---------------------------------------------------------------------------

#[test]
fn test_iso_format_unformat_jepoch() {
    // C error 26: jepoch frame, timeorigin 2005.0, default numeric format.
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_time_origin(2005.0);

    assert_eq!(tf.format(1, 100.0).unwrap(), "100");
    let (v, nc) = tf.unformat(1, "100").unwrap();
    assert_eq!(nc, 3);
    assert!((v - 100.0).abs() < 1.0e-12);

    // error 27: format=iso -> date only.
    tf.set_format_str(1, "iso").unwrap();
    assert_eq!(tf.format(1, 1.0).unwrap(), "2006-01-01");
    let (v, nc) = tf.unformat(1, "2006-01-01").unwrap();
    assert_eq!(nc, "2006-01-01".len());
    assert!((v - 1.0).abs() < 1.0e-9, "got {v}");

    // error 28: format=iso.0 -> date + time, no fractional seconds.
    tf.set_format_str(1, "iso.0").unwrap();
    assert_eq!(tf.format(1, 1.0).unwrap(), "2006-01-01 00:00:00");
    let (v, nc) = tf.unformat(1, "2006-01-01 00:00:00").unwrap();
    assert_eq!(nc, "2006-01-01 00:00:00".len());
    assert!((v - 1.0).abs() < 1.0e-9, "got {v}");
}

#[test]
fn test_iso_format_unformat_seconds() {
    // C errors 29-30: jepoch frame, timeorigin 2005.0, unit=s, format=iso.2.
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_time_origin(2005.0);
    tf.set_unit(1, "s").unwrap();
    tf.set_format_str(1, "iso.2").unwrap();

    assert_eq!(tf.format(1, 10.0).unwrap(), "2004-12-31 18:00:10.00");
    let (v, nc) = tf.unformat(1, "2004-12-31 18:00:10.00").unwrap();
    assert_eq!(nc, "2004-12-31 18:00:10.00".len());
    assert!((v - 10.0).abs() < 1.0e-3, "got {v}");

    assert_eq!(tf.format(1, 10.12).unwrap(), "2004-12-31 18:00:10.12");
    let (v, nc) = tf.unformat(1, "2004-12-31 18:00:10.12").unwrap();
    assert_eq!(nc, "2004-12-31 18:00:10.12".len());
    assert!((v - 10.12).abs() < 1.0e-3, "got {v}");
}

#[test]
fn test_unformat_j_prefix_epoch() {
    // C errors 31-32: J-prefix epoch parsing on a jepoch/origin=2005 frame.
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_time_origin(2005.0);

    let (v, nc) = tf.unformat(1, "J2005.0").unwrap();
    assert_eq!(nc, 7);
    assert!((v - 0.0).abs() < 1.0e-9, "got {v}");

    let (v, nc) = tf.unformat(1, "J2010.0").unwrap();
    assert_eq!(nc, 7);
    assert!((v - 5.0).abs() < 1.0e-9, "got {v}");
}

#[test]
fn test_unformat_trailing_text() {
    // C error 33: a Gregorian date/time followed by trailing text. The
    // consumed count includes the trailing space after the time (17), and
    // the value is in Julian epoch years from origin 2005.0.
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_time_origin(2005.0);

    let (v, nc) = tf.unformat(1, "2005-jun-1 12:30 lunch time").unwrap();
    assert_eq!(nc, 17);
    assert!((v - 0.415525896).abs() < 1.0e-7, "got {v}");
}

#[test]
fn test_unformat_b_prefix_uses_tt() {
    // A Besselian-epoch string is interpreted in the TT timescale, then
    // converted to the frame's UTC timescale and Julian-epoch system. The
    // intended frame here is origin=2005 expressed in UTC, so the timescale
    // must be set *before* the origin: TimeOrigin records an absolute instant,
    // and changing the timescale afterwards would re-express it (see
    // test_unformat_b_prefix_origin_then_timescale).
    //
    // Oracle (reference C library, single-string construction
    // "system=jepoch, timescale=utc, timeorigin=2005") and astropy both give
    // the same value:
    //   astropy: Time(52091.8966962715, format="mjd", scale="tt").utc.jyear
    //            - 2005.0  ==  -3.5013115581077727
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_timescale(TimeScale::Utc).unwrap();
    tf.set_time_origin(2005.0);

    let (v, nc) = tf.unformat(1, "B2001.5 lunch time").unwrap();
    assert_eq!(nc, 8);
    assert!((v + 3.5013115581077727).abs() < 1.0e-10, "got {v}");
}

#[test]
fn test_unformat_b_prefix_origin_then_timescale() {
    // The same frame attributes as test_unformat_b_prefix_uses_tt but built in
    // the opposite order: origin first (in the JEPOCH default TAI timescale),
    // then timescale=utc. Setting the timescale re-expresses the stored origin
    // from TAI into UTC (C OriginScale), shifting it to 2004.99999899, so the
    // unformatted value differs from the origin=2005 case by the leap-second
    // offset. Rust matches the reference C library exactly:
    //   reference C: origin -> 2004.9999989859814, unformat -> -3.501310544089165
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_time_origin(2005.0);
    tf.set_timescale(TimeScale::Utc).unwrap();

    let (v, _nc) = tf.unformat(1, "B2001.5 lunch time").unwrap();
    assert!(
        (v + 3.501310544089165).abs() < 1.0e-10,
        "origin-then-timescale construction should match C, got {v}"
    );
}

#[test]
fn test_iso_format_mjd_tai_iso6() {
    // C errors 35-36 path: an absolute MJD/TAI time formatted with iso.6.
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Tai).unwrap();
    tf.set_format_str(1, "iso.6").unwrap();
    // 1977-01-01 00:00:00 in MJD is 43144.0.
    assert_eq!(tf.format(1, 43144.0).unwrap(), "1977-01-01 00:00:00.000000");
}

#[test]
fn test_iso_format_variants() {
    // Exercise the iso / iso.N / iso.NT format spellings.
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Tai).unwrap();
    tf.set_format_str(1, "iso").unwrap();
    assert_eq!(tf.format(1, 43144.0).unwrap(), "1977-01-01");
    tf.set_format_str(1, "iso.0T").unwrap();
    assert_eq!(tf.format(1, 43144.0).unwrap(), "1977-01-01T00:00:00");
}

// ---------------------------------------------------------------------------
// GetEpoch from TimeOrigin (timeframe.c:2458)
// ---------------------------------------------------------------------------

#[test]
fn test_epoch_default_is_j2000() {
    // No Epoch and no TimeOrigin: default J2000.0 (MJD 51544.5, year 2000).
    let tf = TimeFrame::new(TimeSystem::Mjd);
    assert!((tf.epoch_mjd() - 51544.5).abs() < 1.0e-9);
    assert!((tf.epoch_year() - 2000.0).abs() < 1.0e-9);
}

#[test]
fn test_epoch_from_time_origin_jepoch() {
    // Epoch unset, TimeOrigin set: Epoch is the TimeOrigin as an absolute TDB
    // MJD. Oracle: astGetD(Epoch) == 2005.0000010198 for jepoch/origin=2005.
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_time_origin(2005.0);
    assert!(
        (tf.epoch_year() - 2005.0000010198).abs() < 1.0e-7,
        "got {}",
        tf.epoch_year()
    );
}

#[test]
fn test_epoch_from_time_origin_ut1() {
    // Earth-rotation timescale exercises the DUT1==0 routing branch.
    // Oracle: astGetD(Epoch) == 2009.4606454922 for mjd/ut1/origin=55000.
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_timescale(TimeScale::Ut1).unwrap();
    tf.set_time_origin(55000.0);
    assert!(
        (tf.epoch_year() - 2009.4606454922).abs() < 1.0e-6,
        "got {}",
        tf.epoch_year()
    );
}

#[test]
fn test_epoch_explicit_overrides_time_origin() {
    // An explicitly-set Epoch takes priority over TimeOrigin.
    let mut tf = TimeFrame::new(TimeSystem::Mjd);
    tf.set_time_origin(55000.0);
    tf.set_epoch_mjd(50000.0);
    assert!((tf.epoch_mjd() - 50000.0).abs() < 1.0e-9);
}

// ---------------------------------------------------------------------------
// TT -> TDB conversion (should be close to identity for most epochs)
// ---------------------------------------------------------------------------

#[test]
fn test_tt_to_tdb() {
    // TDB differs from TT by periodic terms of amplitude ~1.7 ms.
    let mut tf1 = TimeFrame::new(TimeSystem::Mjd);
    tf1.set_timescale(TimeScale::Tt).unwrap();

    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Tdb).unwrap();

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
    tf_tt.set_timescale(TimeScale::Tt).unwrap();

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
    tf1.set_timescale(TimeScale::Utc).unwrap();

    let mut tf2 = TimeFrame::new(TimeSystem::Mjd);
    tf2.set_timescale(TimeScale::Lt).unwrap();
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

// ---------------------------------------------------------------------------
// TimeOrigin re-expression when System / TimeScale change
// (C OriginScale / OriginSystem; see docs/issues/timeframe-unformat-findings.md)
// ---------------------------------------------------------------------------

#[test]
fn test_set_timescale_reexpresses_time_origin() {
    // TimeOrigin records an absolute instant. A JEPOCH frame defaults to the
    // TAI timescale, so setting TimeOrigin=2005 stores the instant J2005.0(TAI).
    // Changing the timescale to UTC must re-express that same instant in UTC,
    // shifting the numeric origin down by the leap-second offset (~32 s).
    //
    // Oracle (reference C library, this machine):
    //   astTimeFrame("system=jepoch"); TimeOrigin=2005; set timescale=utc
    //   -> TimeOrigin == 2004.9999989859814
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_time_origin(2005.0);
    assert_eq!(tf.timescale(), TimeScale::Tai);

    tf.set_timescale(TimeScale::Utc).unwrap();
    let origin = tf.time_origin();
    assert!(
        (origin - 2004.9999989859814).abs() < 1.0e-9,
        "TimeOrigin should be re-expressed in UTC, got {origin}"
    );

    // Clearing the timescale re-expresses back to the default (TAI). The
    // round-trip through the leap-second conversion leaves a sub-ULP residual,
    // matching the C oracle value 2004.9999999999998 (not exactly 2005.0).
    tf.clear_timescale().unwrap();
    assert_eq!(tf.timescale(), TimeScale::Tai);
    let back = tf.time_origin();
    assert!(
        (back - 2004.9999999999998).abs() < 1.0e-9,
        "TimeOrigin should be re-expressed back to TAI, got {back}"
    );
}

#[test]
fn test_set_system_reexpresses_time_origin() {
    // Changing System re-expresses the stored TimeOrigin into the new system
    // (C OriginSystem). J2005.0(TAI) is MJD 53370.75(TAI).
    //
    // Oracle (reference C library):
    //   astTimeFrame("system=jepoch, timescale=tai"); TimeOrigin=2005;
    //   set system=mjd -> TimeOrigin == 53370.75
    let mut tf = TimeFrame::new(TimeSystem::Jepoch);
    tf.set_timescale(TimeScale::Tai).unwrap();
    tf.set_time_origin(2005.0);

    tf.set_system(TimeSystem::Mjd).unwrap();
    let origin = tf.time_origin();
    assert!(
        (origin - 53370.75).abs() < 1.0e-6,
        "TimeOrigin should be re-expressed in MJD, got {origin}"
    );
}

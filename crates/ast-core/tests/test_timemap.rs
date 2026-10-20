//! Tests for TimeMap — chains of time system conversions.
//!
//! Ported from: reference/starlink-ast/ast_tester/testtimemap.c
//! Tests verify the core time conversions: MJD↔JD, MJD↔Julian epoch,
//! MJD↔Besselian epoch, UTC↔TAI, TAI↔TT, and serialization round-trip.

use crate::common::native_dump;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, TimeCvtType, TimeMap};

const TOL: f64 = 1.0e-10;

/// Helper: transform a single f64 value through a TimeMap.
fn transform_one(tm: &TimeMap, val: f64, forward: bool) -> f64 {
    let pts = PointSet::from_rows([[val]]);
    let out = tm.transform(&pts, forward).unwrap();
    out.point(0)[0]
}

// ---------------------------------------------------------------------------
// MJD to JD conversion
// ---------------------------------------------------------------------------

#[test]
fn mjd_to_jd() {
    let mut tm = TimeMap::new();
    // Offsets of 0.0 mean: input is raw MJD, output is raw JD.
    tm.add(TimeCvtType::MjdToJd, &[0.0, 0.0]).unwrap();

    // MJD 51544.5 + 2400000.5 = JD 2451545.0 (J2000.0)
    let jd = transform_one(&tm, 51544.5, true);
    assert!(
        (jd - 2_451_545.0).abs() < TOL,
        "MJD 51544.5 -> JD: expected 2451545.0, got {jd}"
    );
}

#[test]
fn jd_to_mjd() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::JdToMjd, &[0.0, 0.0]).unwrap();

    // JD 2451545.0 - 2400000.5 = MJD 51544.5
    let mjd = transform_one(&tm, 2_451_545.0, true);
    assert!(
        (mjd - 51544.5).abs() < TOL,
        "JD 2451545.0 -> MJD: expected 51544.5, got {mjd}"
    );
}

#[test]
fn mjd_jd_round_trip() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToJd, &[0.0, 0.0]).unwrap();
    tm.add(TimeCvtType::JdToMjd, &[0.0, 0.0]).unwrap();

    let result = transform_one(&tm, 51544.5, true);
    assert!(
        (result - 51544.5).abs() < TOL,
        "MJD->JD->MJD round-trip: expected 51544.5, got {result}"
    );
}

// ---------------------------------------------------------------------------
// MJD to Julian epoch
// ---------------------------------------------------------------------------

#[test]
fn mjd_to_julian_epoch() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToJep, &[0.0, 0.0]).unwrap();

    // MJD 51544.5 = J2000.0
    let jep = transform_one(&tm, 51544.5, true);
    assert!(
        (jep - 2000.0).abs() < TOL,
        "MJD 51544.5 -> Julian epoch: expected 2000.0, got {jep}"
    );
}

#[test]
fn julian_epoch_to_mjd() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::JepToMjd, &[0.0, 0.0]).unwrap();

    // J2000.0 -> MJD 51544.5
    let mjd = transform_one(&tm, 2000.0, true);
    assert!(
        (mjd - 51544.5).abs() < TOL,
        "Julian epoch 2000.0 -> MJD: expected 51544.5, got {mjd}"
    );
}

#[test]
fn mjd_jep_round_trip() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToJep, &[0.0, 0.0]).unwrap();

    // Forward and inverse should round-trip.
    let jep = transform_one(&tm, 51544.5, true);
    let mjd = transform_one(&tm, jep, false);
    assert!(
        (mjd - 51544.5).abs() < TOL,
        "MJD->JEP->MJD round-trip: expected 51544.5, got {mjd}"
    );
}

// ---------------------------------------------------------------------------
// MJD to Besselian epoch
// ---------------------------------------------------------------------------

#[test]
fn mjd_to_besselian_epoch() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToBep, &[0.0, 0.0]).unwrap();

    // B1950.0 = MJD 33281.92346 (approximately)
    let bep = transform_one(&tm, 33281.92346, true);
    assert!(
        (bep - 1950.0).abs() < 1.0e-5,
        "MJD ~33281.9 -> Besselian epoch: expected ~1950.0, got {bep}"
    );
}

#[test]
fn mjd_bep_round_trip() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToBep, &[0.0, 0.0]).unwrap();

    let test_mjd = 40000.0;
    let bep = transform_one(&tm, test_mjd, true);
    let mjd = transform_one(&tm, bep, false);
    assert!(
        (mjd - test_mjd).abs() < TOL,
        "MJD->BEP->MJD round-trip: expected {test_mjd}, got {mjd}"
    );
}

// ---------------------------------------------------------------------------
// UTC <-> TAI
// ---------------------------------------------------------------------------

#[test]
fn utc_to_tai_at_j2000() {
    let mut tm = TimeMap::new();
    // Use BAD_VALUE for dtai to trigger automatic lookup.
    tm.add(
        TimeCvtType::UtcToTai,
        &[0.0, ast_core::constants::BAD_VALUE],
    )
    .unwrap();

    // At J2000.0 (MJD 51544.5), TAI-UTC = 32 seconds.
    let tai = transform_one(&tm, 51544.5, true);
    let expected = 51544.5 + 32.0 / 86400.0;
    assert!(
        (tai - expected).abs() < 1.0e-12,
        "UTC->TAI at J2000: expected {expected}, got {tai}"
    );
}

#[test]
fn tai_to_utc_at_j2000() {
    let mut tm = TimeMap::new();
    tm.add(
        TimeCvtType::TaiToUtc,
        &[0.0, ast_core::constants::BAD_VALUE],
    )
    .unwrap();

    // TAI = 51544.5 + 32/86400 should give UTC = 51544.5
    let tai = 51544.5 + 32.0 / 86400.0;
    let utc = transform_one(&tm, tai, true);
    assert!(
        (utc - 51544.5).abs() < 1.0e-12,
        "TAI->UTC at J2000: expected 51544.5, got {utc}"
    );
}

#[test]
fn utc_tai_round_trip() {
    let mut tm = TimeMap::new();
    tm.add(
        TimeCvtType::UtcToTai,
        &[0.0, ast_core::constants::BAD_VALUE],
    )
    .unwrap();

    let utc = 51544.5;
    let tai = transform_one(&tm, utc, true);
    let utc_back = transform_one(&tm, tai, false);
    assert!(
        (utc_back - utc).abs() < 1.0e-12,
        "UTC->TAI->UTC round-trip: expected {utc}, got {utc_back}"
    );
}

// ---------------------------------------------------------------------------
// TAI <-> TT
// ---------------------------------------------------------------------------

#[test]
fn tai_to_tt() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::TaiToTt, &[0.0]).unwrap();

    // TT-TAI = 32.184 seconds.
    let tai = 51544.5;
    let tt = transform_one(&tm, tai, true);
    let expected = tai + 32.184 / 86400.0;
    assert!(
        (tt - expected).abs() < 1.0e-15,
        "TAI->TT: expected {expected}, got {tt}"
    );
}

#[test]
fn tt_to_tai() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::TtToTai, &[0.0]).unwrap();

    let tt = 51544.5 + 32.184 / 86400.0;
    let tai = transform_one(&tm, tt, true);
    assert!(
        (tai - 51544.5).abs() < 1.0e-15,
        "TT->TAI: expected 51544.5, got {tai}"
    );
}

// ---------------------------------------------------------------------------
// MJD offset conversions
// ---------------------------------------------------------------------------

#[test]
fn mjd_to_mjd_offset() {
    let mut tm = TimeMap::new();
    // Input offset 50000, output offset 51000.
    // The conversion just shifts: output = input + (50000 - 51000) = input - 1000
    tm.add(TimeCvtType::MjdToMjd, &[50000.0, 51000.0]).unwrap();

    let result = transform_one(&tm, 1544.5, true);
    // Input MJD = 1544.5 (relative to 50000, so absolute = 51544.5)
    // Output = 1544.5 + (50000 - 51000) = 544.5 (relative to 51000, absolute = 51544.5)
    assert!(
        (result - 544.5).abs() < TOL,
        "MJD offset conversion: expected 544.5, got {result}"
    );
}

// ---------------------------------------------------------------------------
// TT <-> TCG
// ---------------------------------------------------------------------------

#[test]
fn tt_tcg_round_trip() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::TtToTcg, &[51544.0]).unwrap();

    let tt = 0.5; // offset from 51544.0
    let tcg = transform_one(&tm, tt, true);
    let tt_back = transform_one(&tm, tcg, false);
    assert!(
        (tt_back - tt).abs() < 1.0e-15,
        "TT->TCG->TT round-trip: expected {tt}, got {tt_back}"
    );
}

// ---------------------------------------------------------------------------
// TDB <-> TCB
// ---------------------------------------------------------------------------

#[test]
fn tdb_tcb_round_trip() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::TdbToTcb, &[51544.0]).unwrap();

    let tdb = 0.5;
    let tcb = transform_one(&tm, tdb, true);
    let tdb_back = transform_one(&tm, tcb, false);
    assert!(
        (tdb_back - tdb).abs() < 1.0e-15,
        "TDB->TCB->TDB round-trip: expected {tdb}, got {tdb_back}"
    );
}

// ---------------------------------------------------------------------------
// UT <-> GMST
// ---------------------------------------------------------------------------

#[test]
fn ut_gmst_round_trip() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::UtToGmst, &[51544.0]).unwrap();

    let ut = 0.5;
    let gmst = transform_one(&tm, ut, true);
    let ut_back = transform_one(&tm, gmst, false);
    assert!(
        (ut_back - ut).abs() < 1.0e-10,
        "UT->GMST->UT round-trip: expected {ut}, got {ut_back}"
    );
}

// ---------------------------------------------------------------------------
// UT1 <-> UTC (DUT1)
// ---------------------------------------------------------------------------

#[test]
fn ut_utc_dut1() {
    let mut tm = TimeMap::new();
    // DUT1 = 0.3 seconds
    tm.add(TimeCvtType::UtToUtc, &[0.3]).unwrap();

    let ut1 = 51544.5;
    let utc = transform_one(&tm, ut1, true);
    // UTC = UT1 - DUT1/86400
    let expected = ut1 - 0.3 / 86400.0;
    assert!(
        (utc - expected).abs() < 1.0e-15,
        "UT1->UTC: expected {expected}, got {utc}"
    );
}

// ---------------------------------------------------------------------------
// LT <-> UTC
// ---------------------------------------------------------------------------

#[test]
fn lt_utc_timezone() {
    let mut tm = TimeMap::new();
    // Timezone offset = 5 hours (e.g. EST)
    tm.add(TimeCvtType::LtToUtc, &[5.0]).unwrap();

    let lt = 51544.5;
    let utc = transform_one(&tm, lt, true);
    // UTC = LT - 5/24
    let expected = lt - 5.0 / 24.0;
    assert!(
        (utc - expected).abs() < 1.0e-15,
        "LT->UTC (5h): expected {expected}, got {utc}"
    );
}

// ---------------------------------------------------------------------------
// Inversion
// ---------------------------------------------------------------------------

#[test]
fn inversion() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToJep, &[0.0, 0.0]).unwrap();

    // Forward: MJD 51544.5 -> J2000.0
    let jep = transform_one(&tm, 51544.5, true);
    assert!((jep - 2000.0).abs() < TOL);

    // Invert and use forward (should act as inverse).
    tm.invert();
    let mjd = transform_one(&tm, 2000.0, true);
    assert!(
        (mjd - 51544.5).abs() < TOL,
        "Inverted TimeMap: expected 51544.5, got {mjd}"
    );
}

// ---------------------------------------------------------------------------
// BAD_VALUE handling
// ---------------------------------------------------------------------------

#[test]
fn bad_value_propagation() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToJd, &[0.0, 0.0]).unwrap();

    let result = transform_one(&tm, ast_core::constants::BAD_VALUE, true);
    assert_eq!(
        result,
        ast_core::constants::BAD_VALUE,
        "BAD_VALUE should propagate"
    );
}

// ---------------------------------------------------------------------------
// Multi-step chain
// ---------------------------------------------------------------------------

#[test]
fn chain_utc_to_tt() {
    let mut tm = TimeMap::new();
    // UTC -> TAI -> TT (the common conversion chain)
    tm.add(
        TimeCvtType::UtcToTai,
        &[0.0, ast_core::constants::BAD_VALUE],
    )
    .unwrap();
    tm.add(TimeCvtType::TaiToTt, &[0.0]).unwrap();

    // At J2000.0: TAI-UTC=32s, TT-TAI=32.184s, so TT-UTC = 64.184s
    let utc = 51544.5;
    let tt = transform_one(&tm, utc, true);
    let expected = utc + 64.184 / 86400.0;
    assert!(
        (tt - expected).abs() < 1.0e-12,
        "UTC->TAI->TT: expected {expected}, got {tt}"
    );
}

// ---------------------------------------------------------------------------
// Serialisation round-trip
// ---------------------------------------------------------------------------

#[test]
fn serialisation_round_trip() {
    use ast_core::{AnyObject, from_native_string};

    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToJd, &[100.0, 200.0]).unwrap();
    tm.add(
        TimeCvtType::UtcToTai,
        &[51544.0, ast_core::constants::BAD_VALUE],
    )
    .unwrap();
    tm.add(TimeCvtType::TaiToTt, &[51544.0]).unwrap();

    // Serialise.
    let text = native_dump(&AnyObject::TimeMap(tm.clone())).unwrap();

    // Deserialise.
    let restored = from_native_string(&text).unwrap();
    let tm2 = match restored {
        AnyObject::TimeMap(t) => t,
        _ => panic!("Expected TimeMap from deserialization"),
    };

    // Compare transforms.
    let test_val = 51544.5;
    let out1 = transform_one(&tm, test_val, true);
    let out2 = transform_one(&tm2, test_val, true);
    assert!(
        (out1 - out2).abs() < 1.0e-15,
        "Serialisation round-trip: {out1} != {out2}"
    );
}

// ---------------------------------------------------------------------------
// TT <-> TDB (with explicit DTAI)
// ---------------------------------------------------------------------------

#[test]
fn tt_tdb_round_trip() {
    let mut tm = TimeMap::new();
    // args: mjd_offset=51544.0, obslon=0.0, obslat=0.0, obsalt=0.0, dtai=32.0
    tm.add(TimeCvtType::TtToTdb, &[51544.0, 0.0, 0.0, 0.0, 32.0])
        .unwrap();

    let tt = 0.5; // offset
    let tdb = transform_one(&tm, tt, true);
    let tt_back = transform_one(&tm, tdb, false);
    assert!(
        (tt_back - tt).abs() < 1.0e-10,
        "TT->TDB->TT round-trip: expected {tt}, got {tt_back}"
    );

    // The TDB-TT correction should be ~1.6ms max, i.e. ~1.9e-8 days.
    let diff_days = (tdb - tt).abs();
    assert!(
        diff_days < 2.0e-8,
        "TDB-TT difference too large: {diff_days} days"
    );
}

// ---------------------------------------------------------------------------
// GMST <-> LMST
// ---------------------------------------------------------------------------

#[test]
fn gmst_lmst_round_trip() {
    let mut tm = TimeMap::new();
    // obslon=1.0 rad, obslat=0.5 rad
    tm.add(TimeCvtType::GmstToLmst, &[51544.0, 1.0, 0.5])
        .unwrap();

    let gmst = 0.5;
    let lmst = transform_one(&tm, gmst, true);
    let gmst_back = transform_one(&tm, lmst, false);
    assert!(
        (gmst_back - gmst).abs() < 1.0e-15,
        "GMST->LMST->GMST round-trip: expected {gmst}, got {gmst_back}"
    );
}

// ---------------------------------------------------------------------------
// LMST <-> LAST
// ---------------------------------------------------------------------------

#[test]
fn lmst_last_round_trip() {
    let mut tm = TimeMap::new();
    // obslon=0.5 rad, obslat=0.8 rad
    tm.add(TimeCvtType::LmstToLast, &[51544.0, 0.5, 0.8])
        .unwrap();

    let lmst = 0.5;
    let last = transform_one(&tm, lmst, true);
    let lmst_back = transform_one(&tm, last, false);
    // Equation of equinoxes is small (~tens of ms) but the
    // approximation means round-trip may not be exact to full precision
    // due to the TDB approximation. Should be good to ~1e-10.
    assert!(
        (lmst_back - lmst).abs() < 1.0e-10,
        "LMST->LAST->LMST round-trip: expected {lmst}, got {lmst_back}"
    );
}

// ---------------------------------------------------------------------------
// Equality
// ---------------------------------------------------------------------------

#[test]
fn equality() {
    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::MjdToJd, &[0.0, 0.0]).unwrap();
    tm1.add(TimeCvtType::UtcToTai, &[0.0, 32.0]).unwrap();

    let mut tm2 = TimeMap::new();
    tm2.add(TimeCvtType::MjdToJd, &[0.0, 0.0]).unwrap();
    tm2.add(TimeCvtType::UtcToTai, &[0.0, 32.0]).unwrap();

    assert!(tm1.equal(&tm2));

    let mut tm3 = TimeMap::new();
    tm3.add(TimeCvtType::MjdToJd, &[0.0, 0.0]).unwrap();
    tm3.add(TimeCvtType::UtcToTai, &[0.0, 33.0]).unwrap();

    assert!(!tm1.equal(&tm3));
}

// ---------------------------------------------------------------------------
// try_merge (inverse cancellation)
// ---------------------------------------------------------------------------

#[test]
fn try_merge_cancellation() {
    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::MjdToJep, &[0.0, 0.0]).unwrap();

    let mut tm2 = TimeMap::new();
    tm2.add(TimeCvtType::MjdToJep, &[0.0, 0.0]).unwrap();

    // Same TimeMap with opposite inversions should cancel.
    let merged = tm1.try_merge(&tm2, true, false, true);
    assert!(merged.is_some(), "Should merge to UnitMap");
    let merged = merged.unwrap();
    assert_eq!(merged.nin(), 1);
    assert_eq!(merged.nout(), 1);
}

// ---------------------------------------------------------------------------
// TimeCvtType utility methods
// ---------------------------------------------------------------------------

/// Every conversion's integer code is C's `AST__*` value (timemap.c), both as
/// the discriminant and through `from_int`.
#[test]
fn cvt_type_codes_match_c() {
    use TimeCvtType::*;
    let c_codes = [
        (MjdToMjd, 1),
        (MjdToJd, 2),
        (JdToMjd, 3),
        (MjdToBep, 4),
        (BepToMjd, 5),
        (MjdToJep, 6),
        (JepToMjd, 7),
        (TaiToUtc, 8),
        (UtcToTai, 9),
        (TtToTai, 10),
        (TaiToTt, 11),
        (TdbToTt, 12),
        (TtToTdb, 13),
        (TcgToTt, 14),
        (TtToTcg, 15),
        (TcbToTdb, 16),
        (TdbToTcb, 17),
        (UtToGmst, 18),
        (GmstToUt, 19),
        (GmstToLmst, 20),
        (LmstToGmst, 21),
        (LastToLmst, 22),
        (LmstToLast, 23),
        (UtToUtc, 24),
        (UtcToUt, 25),
        (LtToUtc, 26),
        (UtcToLt, 27),
    ];
    for (cvt, code) in c_codes {
        assert_eq!(cvt as i32, code, "{cvt:?} discriminant");
        assert_eq!(TimeCvtType::from_int(code), Some(cvt), "from_int({code})");
    }
}

#[test]
fn cvt_type_from_int() {
    assert_eq!(TimeCvtType::from_int(1), Some(TimeCvtType::MjdToMjd));
    assert_eq!(TimeCvtType::from_int(27), Some(TimeCvtType::UtcToLt));
    assert_eq!(TimeCvtType::from_int(0), None);
    assert_eq!(TimeCvtType::from_int(28), None);
}

#[test]
fn cvt_type_from_code() {
    assert_eq!(
        TimeCvtType::from_code("MJDTOJD"),
        Some(TimeCvtType::MjdToJd)
    );
    assert_eq!(
        TimeCvtType::from_code("mjdtojd"),
        Some(TimeCvtType::MjdToJd)
    );
    assert_eq!(TimeCvtType::from_code("BOGUS"), None);
}

#[test]
fn cvt_type_inverse() {
    assert_eq!(TimeCvtType::MjdToJd.inverse(), TimeCvtType::JdToMjd);
    assert_eq!(TimeCvtType::JdToMjd.inverse(), TimeCvtType::MjdToJd);
    assert_eq!(TimeCvtType::TaiToUtc.inverse(), TimeCvtType::UtcToTai);
    assert_eq!(TimeCvtType::MjdToMjd.inverse(), TimeCvtType::MjdToMjd);
}

// ---------------------------------------------------------------------------
// try_merge — step concatenation and simplification
// ---------------------------------------------------------------------------

#[test]
fn timemap_merge_concatenates() {
    use ast_core::mappings::UnitMap;

    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::MjdToMjd, &[0.0, 51544.5]).unwrap();
    let mut tm2 = TimeMap::new();
    tm2.add(TimeCvtType::TaiToUtc, &[0.0, 37.0]).unwrap();

    let merged = tm1.try_merge(&tm2, true, false, false);
    assert!(merged.is_some(), "Adjacent TimeMaps should always merge");
    let merged = merged.unwrap();
    // Should be a TimeMap (not UnitMap) since the steps don't cancel.
    assert!(
        !merged.as_any().is::<UnitMap>(),
        "Non-cancelling merge should not produce UnitMap"
    );
    assert!(
        merged.as_any().is::<TimeMap>(),
        "Merged result should be a TimeMap"
    );
}

#[test]
fn timemap_merge_cancels_inverse() {
    use ast_core::mappings::UnitMap;

    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::TaiToUtc, &[0.0, 37.0]).unwrap();
    let mut tm2 = TimeMap::new();
    tm2.add(TimeCvtType::TaiToUtc, &[0.0, 37.0]).unwrap();

    // Same steps, opposite direction → should cancel to UnitMap
    let merged = tm1.try_merge(&tm2, true, false, true);
    assert!(merged.is_some(), "Inverse pair should merge");
    assert!(
        merged.unwrap().as_any().is::<UnitMap>(),
        "Inverse pair should cancel to UnitMap"
    );
}

#[test]
fn timemap_merge_eliminates_no_effect() {
    use ast_core::mappings::UnitMap;

    // MjdToMjd with zero offset is a no-op
    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::MjdToMjd, &[0.0, 0.0]).unwrap();

    let s = <dyn Mapping>::simplify(&tm1).unwrap();
    assert!(
        s.as_any().is::<UnitMap>(),
        "MjdToMjd(0,0) should simplify to UnitMap"
    );
}

#[test]
fn timemap_merge_mjd_to_jd_cancels() {
    use ast_core::mappings::UnitMap;

    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::MjdToJd, &[100.0, 200.0]).unwrap();
    let mut tm2 = TimeMap::new();
    tm2.add(TimeCvtType::JdToMjd, &[200.0, 100.0]).unwrap();

    // MjdToJd(a,b) followed by JdToMjd(b,a) cancels
    let merged = tm1.try_merge(&tm2, true, false, false);
    assert!(merged.is_some());
    assert!(
        merged.unwrap().as_any().is::<UnitMap>(),
        "MjdToJd + JdToMjd with swapped args should cancel"
    );
}

#[test]
fn timemap_merge_mjd_to_mjd_equal_offsets() {
    use ast_core::mappings::UnitMap;

    // MjdToMjd where both offsets are equal (not zero) is still identity
    let mut tm1 = TimeMap::new();
    tm1.add(TimeCvtType::MjdToMjd, &[51544.5, 51544.5]).unwrap();

    let s = <dyn Mapping>::simplify(&tm1).unwrap();
    assert!(
        s.as_any().is::<UnitMap>(),
        "MjdToMjd with equal offsets should simplify to UnitMap"
    );
}

#[test]
fn timemap_simplified_chain() {
    use ast_core::mappings::UnitMap;

    // TaiToUtc followed by UtcToTai should cancel in simplified()
    let mut tm = TimeMap::new();
    tm.add(
        TimeCvtType::TaiToUtc,
        &[51544.0, ast_core::constants::BAD_VALUE],
    )
    .unwrap();
    tm.add(
        TimeCvtType::UtcToTai,
        &[51544.0, ast_core::constants::BAD_VALUE],
    )
    .unwrap();

    let s = <dyn Mapping>::simplify(&tm).unwrap();
    assert!(
        s.as_any().is::<UnitMap>(),
        "TaiToUtc + UtcToTai should simplify to UnitMap"
    );
}

#[test]
fn utc_tai_roundtrip_pre1972_eras() {
    use ast_core::constants::BAD_VALUE;
    // Ported from testtime.c (commit 8be504ac): one UTC MJD inside each of
    // the 14 pre-1972 "rubber second" eras. A UTC->TAI->UTC round trip must
    // return to the start to ~1e-9 d, and TAI-UTC at the test point must
    // match the published value for that era to ~1e-5 s. A units error in
    // the inverse divisor previously leaked ~1.9e-6 d; a wrong 1966
    // intercept leaked ~0.1 s at MJD 39500.
    let era_utc = [
        40000.0, 39500.0, 39060.0, 38970.0, 38880.0, 38790.0, 38700.0, 38560.0, 38440.0, 38360.0,
        38000.0, 37590.0, 37400.0, 37200.0,
    ];
    let era_dt = [
        6.478578, 5.282578, 4.227634, 4.010994, 3.794354, 3.577714, 3.361074, 3.079634, 2.824114,
        2.726482, 2.222130, 1.748658, 1.552418, 1.288218,
    ];

    let mut fwd = TimeMap::new();
    fwd.add(TimeCvtType::UtcToTai, &[0.0, BAD_VALUE]).unwrap();
    let mut inv = TimeMap::new();
    inv.add(TimeCvtType::TaiToUtc, &[0.0, BAD_VALUE]).unwrap();

    for (utc, dt) in era_utc.iter().zip(era_dt.iter()) {
        let tai = transform_one(&fwd, *utc, true); // UTC -> TAI (MJD)
        let tai_minus_utc = (tai - *utc) * 86400.0; // seconds
        assert!(
            (tai_minus_utc - dt).abs() < 1e-5,
            "TAI-UTC at MJD {utc}: got {tai_minus_utc}, expected {dt}"
        );
        let utc_back = transform_one(&inv, tai, true); // TAI -> UTC (MJD)
        assert!(
            (utc_back - *utc).abs() < 1e-9,
            "UTC->TAI->UTC round trip at MJD {utc}: got {utc_back}"
        );
    }
}

/// testtime.c errors 62 and 63: the rate is the slope of the conversions in
/// the direction the TimeMap applies, exactly as C's analytic `Rate` gives
/// it.
#[test]
fn rate_is_the_analytic_slope_in_either_direction() {
    let mut tm = TimeMap::new();
    tm.add(TimeCvtType::MjdToJep, &[0.0, 0.0]).unwrap();
    assert_eq!(tm.rate(&[51544.5], 1, 1).unwrap(), 1.0 / 365.25);
    tm.invert();
    assert_eq!(tm.rate(&[2000.0], 1, 1).unwrap(), 365.25);
}

//! Tests for LutMap — 1-D lookup table mapping.
//!
//! Ported from C's testlutmap.c and the Step 6 design spec.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::mappings::{CmpMap, Composition, LutInterp, LutMap, Mapping, UnitMap};
use ast_core::{AnyObject, from_native_string, to_native_string};

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn lutmap_dimensions() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    assert_eq!(m.nin(), 1);
    assert_eq!(m.nout(), 1);
}

#[test]
fn lutmap_empty_table_rejected() {
    let result = LutMap::new(&[], 0.0, 1.0);
    assert!(result.is_err());
}

#[test]
fn lutmap_zero_inc_rejected() {
    let result = LutMap::new(&[1.0, 2.0], 0.0, 0.0);
    assert!(result.is_err());
}

#[test]
fn lutmap_default_interp() {
    let m = LutMap::new(&[0.0, 10.0], 0.0, 1.0).unwrap();
    assert_eq!(m.interp(), LutInterp::Linear);
}

// ---------------------------------------------------------------------------
// Monotonicity detection
// ---------------------------------------------------------------------------

#[test]
fn lutmap_monotonic_increasing() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    assert_eq!(m.monotonic(), 1);
}

#[test]
fn lutmap_monotonic_decreasing() {
    let m = LutMap::new(&[30.0, 20.0, 10.0, 0.0], 0.0, 1.0).unwrap();
    assert_eq!(m.monotonic(), -1);
}

#[test]
fn lutmap_non_monotonic() {
    let m = LutMap::new(&[0.0, 10.0, 5.0, 30.0], 0.0, 1.0).unwrap();
    assert_eq!(m.monotonic(), 0);
}

#[test]
fn lutmap_flat_sections_monotonic() {
    // Flat sections are ignored: [0, 10, 10, 20] is still increasing
    let m = LutMap::new(&[0.0, 10.0, 10.0, 20.0], 0.0, 1.0).unwrap();
    assert_eq!(m.monotonic(), 1);
}

#[test]
fn lutmap_all_equal_undetermined() {
    // All equal -> no direction detected
    let m = LutMap::new(&[5.0, 5.0, 5.0], 0.0, 1.0).unwrap();
    assert_eq!(m.monotonic(), 0);
}

#[test]
fn lutmap_bad_entries_ignored_for_monotonicity() {
    // [0, BAD, 20, 30] — ignoring BAD, sequence is 0, 20, 30 -> increasing
    let m = LutMap::new(&[0.0, BAD_VALUE, 20.0, 30.0], 0.0, 1.0).unwrap();
    assert_eq!(m.monotonic(), 1);
}

// ---------------------------------------------------------------------------
// Linear forward transform
// ---------------------------------------------------------------------------

#[test]
fn lutmap_linear_interpolation() {
    // Table [0, 10, 20, 30], start=0, inc=1
    // Input 1.5 -> between lut[1]=10 and lut[2]=20 -> 15
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[1.5]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 15.0).abs() < 1e-10);
}

#[test]
fn lutmap_linear_at_exact_entry() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 20.0).abs() < 1e-10);
}

#[test]
fn lutmap_linear_at_first_entry() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[0.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 0.0).abs() < 1e-10);
}

#[test]
fn lutmap_linear_at_last_entry() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 30.0).abs() < 1e-10);
}

#[test]
fn lutmap_linear_extrapolation_below() {
    // Input -0.5: idx = -0.5, slope at start = lut[1]-lut[0] = 10
    // output = 0 + (-0.5) * 10 = -5
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[-0.5]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - (-5.0)).abs() < 1e-10);
}

#[test]
fn lutmap_linear_extrapolation_above() {
    // Input 3.5: idx = 3.5, slope at end = lut[3]-lut[2] = 10
    // output = 30 + 0.5 * 10 = 35
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[3.5]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 35.0).abs() < 1e-10);
}

#[test]
fn lutmap_linear_with_nonzero_start_and_inc() {
    // start=10, inc=5: table maps input 10->0, 15->10, 20->20, 25->30
    // Input 12.5: idx = (12.5 - 10) / 5 = 0.5
    // interpolate lut[0]=0 and lut[1]=10 -> 5
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 10.0, 5.0).unwrap();
    let pts = PointSet::from_rows([[12.5]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 5.0).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// Nearest-neighbour forward transform
// ---------------------------------------------------------------------------

#[test]
fn lutmap_nearest_in_bounds() {
    let mut m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    m.set_interp(LutInterp::Nearest);

    // Input 1.3 -> round to 1 -> lut[1] = 10
    let pts = PointSet::from_rows([[1.3]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 10.0).abs() < 1e-10);

    // Input 1.7 -> round to 2 -> lut[2] = 20
    let pts2 = PointSet::from_rows([[1.7]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert!((out2.point(0)[0] - 20.0).abs() < 1e-10);
}

#[test]
fn lutmap_nearest_outside_bounds() {
    let mut m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    m.set_interp(LutInterp::Nearest);

    // Input -1.0 -> round to -1 -> outside -> BAD
    let pts = PointSet::from_rows([[-1.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);

    // Input 4.0 -> round to 4 -> outside (nlut=4, max idx=3) -> BAD
    let pts2 = PointSet::from_rows([[4.0]]);
    let out2 = m.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0)[0], BAD_VALUE);
}

// ---------------------------------------------------------------------------
// BAD value handling
// ---------------------------------------------------------------------------

#[test]
fn lutmap_bad_input_propagation() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();

    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);

    let pts_nan = PointSet::from_rows([[f64::NAN]]);
    let out_nan = m.transform(&pts_nan, true).unwrap();
    assert_eq!(out_nan.point(0)[0], BAD_VALUE);
}

#[test]
fn lutmap_bad_in_table_brackets() {
    // Table [0, BAD, 20, 30]; interpolate between idx 0 and 1 -> BAD
    let m = LutMap::new(&[0.0, BAD_VALUE, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[0.5]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn lutmap_bad_at_boundary_extrapolation() {
    // Table [BAD, 10, 20, 30]; extrapolate below -> slope uses lut[0] which is BAD -> BAD
    let m = LutMap::new(&[BAD_VALUE, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[-0.5]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

// ---------------------------------------------------------------------------
// Inverse transform
// ---------------------------------------------------------------------------

#[test]
fn lutmap_inverse_monotonic_increasing() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    // Forward: input 1.5 -> 15
    // Inverse: input 15 -> should give 1.5
    let pts = PointSet::from_rows([[15.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 1.5).abs() < 1e-10);
}

#[test]
fn lutmap_inverse_monotonic_decreasing() {
    let m = LutMap::new(&[30.0, 20.0, 10.0, 0.0], 0.0, 1.0).unwrap();
    // Forward: input 1.5 -> between lut[1]=20 and lut[2]=10 -> 15
    let pts_fwd = PointSet::from_rows([[1.5]]);
    let out_fwd = m.transform(&pts_fwd, true).unwrap();
    assert!((out_fwd.point(0)[0] - 15.0).abs() < 1e-10);

    // Inverse: input 15 -> should give 1.5
    let pts_inv = PointSet::from_rows([[15.0]]);
    let out_inv = m.transform(&pts_inv, false).unwrap();
    assert!((out_inv.point(0)[0] - 1.5).abs() < 1e-10);
}

#[test]
fn lutmap_inverse_non_monotonic_is_an_error() {
    // A non-monotonic table has no inverse (lutmap.c GetMonotonic), so C's
    // base Transform raises AST__TRNND.
    let m = LutMap::new(&[0.0, 10.0, 5.0, 30.0], 0.0, 1.0).unwrap();
    assert!(!m.has_inverse());
    let pts = PointSet::from_rows([[15.0]]);
    let err = m.transform(&pts, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Trnnd);
}

#[test]
fn lutmap_flat_section_inverse_returns_bad() {
    // LUT with a flat section: [0.0, 1.0, 1.0, 1.0, 2.0]
    // At values corresponding to indices 1-3, output is constant 1.0.
    // Inverse at y=1.0 is undetermined => BAD.
    let lut = vec![0.0, 1.0, 1.0, 1.0, 2.0];
    let lm = LutMap::new(&lut, 0.0, 1.0).unwrap();

    let pts = PointSet::from_rows([[1.0]]); // y=1.0 hits the flat section
    let out = lm.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn lutmap_inverse_round_trip() {
    // Test that forward then inverse gives back the original
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let inputs = [0.5, 1.0, 1.5, 2.0, 2.5];
    for &x in &inputs {
        let pts = PointSet::from_rows([[x]]);
        let fwd = m.transform(&pts, true).unwrap();
        let inv = m.transform(&fwd, false).unwrap();
        assert!(
            (inv.point(0)[0] - x).abs() < 1e-10,
            "round-trip failed for x={x}: got {}",
            inv.point(0)[0]
        );
    }
}

#[test]
fn lutmap_inverse_extrapolation() {
    // Inverse should also extrapolate beyond the table range
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();

    // Forward maps -0.5 -> -5 (extrapolation below)
    // Inverse of -5 should give -0.5
    let pts = PointSet::from_rows([[-5.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert!(
        (out.point(0)[0] - (-0.5)).abs() < 1e-10,
        "inverse extrapolation below failed: got {}",
        out.point(0)[0]
    );

    // Forward maps 3.5 -> 35 (extrapolation above)
    // Inverse of 35 should give 3.5
    let pts2 = PointSet::from_rows([[35.0]]);
    let out2 = m.transform(&pts2, false).unwrap();
    assert!(
        (out2.point(0)[0] - 3.5).abs() < 1e-10,
        "inverse extrapolation above failed: got {}",
        out2.point(0)[0]
    );
}

#[test]
fn lutmap_inverse_bad_input() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

// ---------------------------------------------------------------------------
// Single-element table
// ---------------------------------------------------------------------------

#[test]
fn lutmap_single_element_is_an_error() {
    // C needs at least two table elements (lutmap.c astInitLutMap).
    let err = LutMap::new(&[42.0], 0.0, 1.0).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Lutin);
    let err = LutMap::new(&[1.0, 2.0], 0.0, 0.0).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Lutii);
}

#[test]
fn lutmap_invert_flag() {
    let mut m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    assert!(!m.is_inverted());
    m.invert();
    assert!(m.is_inverted());

    // After invert, "forward" applies the inverse formula
    // Input 15 (an output value) -> should give 1.5
    let pts = PointSet::from_rows([[15.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 1.5).abs() < 1e-10,
        "inverted forward failed: got {}",
        out.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// Multiple points
// ---------------------------------------------------------------------------

#[test]
fn lutmap_multiple_points() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    let pts = PointSet::from_rows([[0.0], [1.0], [1.5], [3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 0.0).abs() < 1e-10);
    assert!((out.point(1)[0] - 10.0).abs() < 1e-10);
    assert!((out.point(2)[0] - 15.0).abs() < 1e-10);
    assert!((out.point(3)[0] - 30.0).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// Rate
// ---------------------------------------------------------------------------

#[test]
fn lutmap_rate() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 0.0, 1.0).unwrap();
    // The rate (derivative) should be ~10 everywhere for this linear table
    let r = m.rate(&[1.5], 1, 1).unwrap();
    assert!(
        (r - 10.0).abs() < 1e-3,
        "rate at x=1.5 expected ~10, got {r}"
    );
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

#[test]
fn lutmap_serialisation_round_trip() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 5.0, 2.0).unwrap();
    let text = to_native_string(&AnyObject::LutMap(m.clone())).unwrap();
    let restored = from_native_string(&text).unwrap();
    let m2 = restored.into_mapping().unwrap();

    // Verify the restored mapping produces the same result
    let pts = PointSet::from_rows([[6.0]]);
    let out1 = m.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();
    assert!(
        (out1.point(0)[0] - out2.point(0)[0]).abs() < 1e-10,
        "round-trip mismatch: {} vs {}",
        out1.point(0)[0],
        out2.point(0)[0]
    );
}

#[test]
fn lutmap_serialisation_text_content() {
    let m = LutMap::new(&[0.0, 10.0, 20.0, 30.0], 5.0, 2.0).unwrap();
    let text = to_native_string(&AnyObject::LutMap(m)).unwrap();
    assert!(text.contains("Begin LutMap"), "missing Begin LutMap");
    assert!(text.contains("Nlut"), "missing Nlut");
    assert!(text.contains("Start"), "missing Start");
    assert!(text.contains("Incr"), "missing Incr");
    assert!(text.contains("L1"), "missing L1");
    assert!(text.contains("L4"), "missing L4");
    assert!(text.contains("End LutMap"), "missing End LutMap");
}

#[test]
fn lutmap_serialisation_default_commented() {
    // start=0 and inc=1 are defaults; both should appear commented to
    // match the C library's Dump output (`astWriteDouble` with helpful=1).
    let m = LutMap::new(&[0.0, 10.0, 20.0], 0.0, 1.0).unwrap();
    let text = to_native_string(&AnyObject::LutMap(m)).unwrap();
    assert!(
        text.contains("# ") && text.contains("Start = 0"),
        "Start should be commented for default 0"
    );
    assert!(
        text.contains("# ") && text.contains("Incr = 1"),
        "Incr should be commented for default 1"
    );
}

#[test]
fn lutmap_serialisation_nearest_interp() {
    let mut m = LutMap::new(&[0.0, 10.0, 20.0], 0.0, 1.0).unwrap();
    m.set_interp(LutInterp::Nearest);
    let text = to_native_string(&AnyObject::LutMap(m)).unwrap();
    assert!(
        text.contains("LutInt"),
        "LutInt should be present for Nearest"
    );

    // Round-trip should preserve interp
    let restored = from_native_string(&text).unwrap();
    if let AnyObject::LutMap(m2) = restored {
        assert_eq!(m2.interp(), LutInterp::Nearest);
    } else {
        panic!("Expected LutMap");
    }
}

#[test]
fn lutmap_serialisation_bad_entries() {
    // BAD entries should not be written to the node (missing = BAD default)
    let m = LutMap::new(&[0.0, BAD_VALUE, 20.0], 0.0, 1.0).unwrap();
    let text = to_native_string(&AnyObject::LutMap(m.clone())).unwrap();
    // L1 and L3 should be present, L2 should not
    assert!(text.contains("L1"), "L1 should be present");
    assert!(!text.contains("L2"), "L2 should be omitted for BAD_VALUE");
    assert!(text.contains("L3"), "L3 should be present");

    // Round-trip: the restored table should have BAD at position 1
    let restored = from_native_string(&text).unwrap();
    let m2 = restored.into_mapping().unwrap();
    let pts = PointSet::from_rows([[0.5]]);
    let out = m2.transform(&pts, true).unwrap();
    // Interpolating between lut[0]=0 and lut[1]=BAD should give BAD
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

#[test]
fn lutmap_serialisation_inverted() {
    let mut m = LutMap::new(&[0.0, 10.0, 20.0], 0.0, 1.0).unwrap();
    m.invert();
    let text = to_native_string(&AnyObject::LutMap(m)).unwrap();
    assert!(text.contains("Invert"), "Invert should be present");

    let restored = from_native_string(&text).unwrap();
    let m2 = restored.into_mapping().unwrap();
    assert!(m2.is_inverted());
}

// ---------------------------------------------------------------------------
// Cancellation against an adjacent inverse LutMap
// ---------------------------------------------------------------------------

/// A non-linear table, so the WinMap replacement cannot fire and the
/// cancellation branch is what is under test.
const CURVED: [f64; 5] = [0.0, 1.0, 4.0, 9.0, 16.0];

fn series_of(first: LutMap, second: LutMap) -> CmpMap {
    CmpMap::new(Box::new(first), Box::new(second), Composition::Series).unwrap()
}

#[test]
fn lutmap_cancels_with_inverse() {
    let lm = LutMap::new(&CURVED, 0.0, 1.0).unwrap();
    let mut inv = lm.clone();
    inv.invert();

    let s = <dyn Mapping>::simplify(&series_of(lm, inv)).unwrap();
    assert!(
        s.as_any().is::<UnitMap>(),
        "a LutMap in series with its own inverse collapses to a UnitMap"
    );
}

#[test]
fn lutmap_no_cancel_different_lut() {
    let lm1 = LutMap::new(&CURVED, 0.0, 1.0).unwrap();
    let mut lm2 = LutMap::new(&[0.0, 2.0, 3.0, 9.0, 8.0], 0.0, 1.0).unwrap();
    lm2.invert();

    let s = <dyn Mapping>::simplify(&series_of(lm1, lm2)).unwrap();
    assert!(
        !s.as_any().is::<UnitMap>(),
        "different tables do not cancel"
    );
}

#[test]
fn lutmap_no_cancel_same_direction() {
    let lm = LutMap::new(&CURVED, 0.0, 1.0).unwrap();

    let s = <dyn Mapping>::simplify(&series_of(lm.clone(), lm)).unwrap();
    assert!(
        !s.as_any().is::<UnitMap>(),
        "two LutMaps applied the same way are not an inverse pair"
    );
}

#[test]
fn lutmap_no_cancel_parallel() {
    let lm = LutMap::new(&CURVED, 0.0, 1.0).unwrap();
    let mut inv = lm.clone();
    inv.invert();

    let parallel = CmpMap::new(Box::new(lm), Box::new(inv), Composition::Parallel).unwrap();
    let s = <dyn Mapping>::simplify(&parallel).unwrap();
    assert!(
        !s.as_any().is::<UnitMap>(),
        "C only cancels the pair in series (lutmap.c:1178)"
    );
}

// ---------------------------------------------------------------------------
// Simplification: linear LUT -> WinMap
// ---------------------------------------------------------------------------

#[test]
fn lutmap_linear_simplifies_to_winmap() {
    use ast_core::mappings::WinMap;

    // A perfectly linear LUT: y = 2*x + 1 (for x = 0, 1, 2, 3, 4)
    // LUT values: [1, 3, 5, 7, 9]
    let lut = vec![1.0, 3.0, 5.0, 7.0, 9.0];
    let lm = LutMap::new(&lut, 0.0, 1.0).unwrap();

    let s = <dyn Mapping>::simplify(&lm).unwrap();
    assert!(
        s.as_any().is::<WinMap>(),
        "Linear LUT should simplify to WinMap"
    );
    assert_eq!(s.nin(), 1);
    assert_eq!(s.nout(), 1);

    // Verify the WinMap produces the same values.
    let pts = PointSet::from_rows([[2.5]]);
    let expected = lm.transform(&pts, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    assert!(
        (actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10,
        "WinMap output {} != LutMap output {}",
        actual.point(0)[0],
        expected.point(0)[0]
    );
}

#[test]
fn lutmap_nearest_linear_does_not_simplify() {
    use ast_core::mappings::WinMap;

    // The table is exactly linear, but nearest-neighbour interpolation puts
    // discontinuities at the first and last entries, so C's GetLinear refuses
    // to call it linear (lutmap.c:578).
    let mut lm = LutMap::new(&[1.0, 3.0, 5.0, 7.0, 9.0], 0.0, 1.0).unwrap();
    lm.set_interp(LutInterp::Nearest);

    let s = <dyn Mapping>::simplify(&lm).unwrap();
    assert!(
        !s.as_any().is::<WinMap>(),
        "a nearest-neighbour LutMap is not replaced by a WinMap"
    );
}

#[test]
fn lutmap_linear_with_start_and_inc_simplifies() {
    use ast_core::mappings::WinMap;

    // Linear LUT with start=10, inc=5: covers inputs [10, 15, 20]
    // LUT values: [100, 200, 300] — y = 100 + 100*(x-10)/5 = 100 + 20*(x-10)
    let lut = vec![100.0, 200.0, 300.0];
    let lm = LutMap::new(&lut, 10.0, 5.0).unwrap();

    let s = <dyn Mapping>::simplify(&lm).unwrap();
    assert!(
        s.as_any().is::<WinMap>(),
        "Linear LUT with non-trivial start/inc should simplify to WinMap"
    );

    // Verify at a few points
    for &x in &[10.0, 12.5, 15.0, 17.5, 20.0] {
        let pts = PointSet::from_rows([[x]]);
        let expected = lm.transform(&pts, true).unwrap();
        let actual = s.transform(&pts, true).unwrap();
        assert!(
            (actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10,
            "Mismatch at x={x}: WinMap={}, LutMap={}",
            actual.point(0)[0],
            expected.point(0)[0]
        );
    }
}

#[test]
fn lutmap_linear_inverted_simplifies() {
    use ast_core::mappings::WinMap;

    // Linear LUT, inverted
    let lut = vec![1.0, 3.0, 5.0, 7.0, 9.0];
    let mut lm = LutMap::new(&lut, 0.0, 1.0).unwrap();
    lm.invert();

    let s = <dyn Mapping>::simplify(&lm).unwrap();
    assert!(
        s.as_any().is::<WinMap>(),
        "Inverted linear LUT should simplify to WinMap"
    );
    // C `lutmap.c::MapMerge` builds the WinMap from swapped corner points and
    // clears the invert flag, so an inverted linear LutMap becomes a *forward*
    // WinMap that directly implements the inverse transform.
    assert!(!s.is_inverted());

    // Verify the forward WinMap produces the same values as the inverted LutMap
    let pts = PointSet::from_rows([[5.0]]);
    let expected = lm.transform(&pts, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    assert!(
        (actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10,
        "Inverted WinMap output {} != inverted LutMap output {}",
        actual.point(0)[0],
        expected.point(0)[0]
    );
}

#[test]
fn lutmap_nonlinear_stays_lutmap() {
    // Non-linear LUT: [0, 1, 4, 9, 16] (quadratic)
    let lut = vec![0.0, 1.0, 4.0, 9.0, 16.0];
    let lm = LutMap::new(&lut, 0.0, 1.0).unwrap();

    let s = <dyn Mapping>::simplify(&lm).unwrap();
    assert!(s.as_any().is::<LutMap>());
}

#[test]
fn lutmap_has_been_simplified_respected() {
    // Linear LUT with has_been_simplified flag should stay as LutMap
    let lut = vec![1.0, 3.0, 5.0, 7.0, 9.0];
    let mut lm = LutMap::new(&lut, 0.0, 1.0).unwrap();
    lm.set_has_been_simplified(true);

    let s = <dyn Mapping>::simplify(&lm).unwrap();
    assert!(s.as_any().is::<LutMap>());
}

// ---------------------------------------------------------------------------
// Inf handling
// ---------------------------------------------------------------------------

#[test]
fn lutmap_treats_inf_as_bad() {
    let lut = vec![0.0, 1.0, 2.0, 3.0];
    let lm = LutMap::new(&lut, 0.0, 1.0).unwrap();

    // Inf input should produce BAD output
    let pts = PointSet::from_rows([[f64::INFINITY]]);
    let out = lm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);

    // Negative Inf too
    let pts_neg = PointSet::from_rows([[f64::NEG_INFINITY]]);
    let out_neg = lm.transform(&pts_neg, true).unwrap();
    assert_eq!(out_neg.point(0)[0], BAD_VALUE);
}

/// Infinite and NaN entries are stored as bad values, as C's `astInitLutMap`
/// stores them, so a table holding one is not linear and simplifying it
/// leaves a LutMap; a linear table of the same length simplifies to a
/// UnitMap. The classes are those the C library gives for the same four
/// tables.
#[test]
fn lutmap_non_finite_entries_not_linear() {
    let tables: [(&str, [f64; 5], bool); 4] = [
        (
            "interior -inf",
            [1.0, 2.0, 3.0, f64::NEG_INFINITY, f64::NEG_INFINITY],
            true,
        ),
        ("last -inf", [1.0, 2.0, 3.0, 4.0, f64::NEG_INFINITY], true),
        ("interior nan", [1.0, 2.0, f64::NAN, 4.0, 5.0], true),
        ("linear", [1.0, 2.0, 3.0, 4.0, 5.0], false),
    ];
    for (label, lut, stays_lut) in tables {
        let s = <dyn Mapping>::simplify(&LutMap::new(&lut, 1.0, 1.0).unwrap()).unwrap();
        if stays_lut {
            assert!(s.as_any().is::<LutMap>(), "{label}: not a LutMap");
        } else {
            assert!(s.as_any().is::<UnitMap>(), "{label}: not a UnitMap");
        }
    }
}

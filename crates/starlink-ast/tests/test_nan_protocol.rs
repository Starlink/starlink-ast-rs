// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Acceptance tests for [`dyn Mapping::transform_nan_protocol`].
//!
//! Sketch validation: the public NaN-as-bad entry must produce results
//! identical (to BAD-output equivalence) to the existing `transform` method
//! across a CmpMap chain. Only one polynomial-arithmetic class (WinMap) has
//! its `transform_into_nan` override implemented in this commit — that's
//! enough to demonstrate the wiring works end-to-end. Remaining classes
//! still go through the default-impl fallback (which calls `transform_into`
//! with BAD-checking semantics), so the answer is correct regardless of
//! how many stages have opted in.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::from_native_string;
use starlink_ast::mappings::Mapping;

fn load_lsst() -> Box<dyn Mapping> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/lsst.ast"
    ))
    .expect("lsst.ast");
    from_native_string(&text)
        .unwrap()
        .into_mapping()
        .unwrap()
        .simplify()
        .unwrap()
}

fn make_points(n: usize) -> PointSet {
    PointSet::from_rows(
        (0..n)
            .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
            .collect::<Vec<_>>(),
    )
}

#[test]
fn nan_protocol_forward_matches_transform() {
    let map = load_lsst();
    let pts = make_points(1024);

    let regular = map.transform(&pts, true).unwrap();
    let via_nan = map.transform_nan_protocol(&pts, true).unwrap();

    for i in 0..pts.npoint() {
        let r = regular.point(i);
        let n = via_nan.point(i);
        for axis in 0..2 {
            let diff = (r[axis] - n[axis]).abs();
            assert!(
                diff < 1e-14,
                "pt {i} axis {axis}: regular={:.17e} nan_protocol={:.17e} diff={:.2e}",
                r[axis],
                n[axis],
                diff
            );
        }
    }
}

#[test]
fn nan_protocol_inverse_matches_transform() {
    let map = load_lsst();
    let pts = make_points(1024);
    let sky = map.transform(&pts, true).unwrap();

    let regular = map.transform(&sky, false).unwrap();
    let via_nan = map.transform_nan_protocol(&sky, false).unwrap();

    for i in 0..pts.npoint() {
        let r = regular.point(i);
        let n = via_nan.point(i);
        for axis in 0..2 {
            let diff = (r[axis] - n[axis]).abs();
            assert!(
                diff < 1e-10,
                "pt {i} axis {axis}: regular={:.17e} nan_protocol={:.17e} diff={:.2e}",
                r[axis],
                n[axis],
                diff
            );
        }
    }
}

#[test]
fn nan_protocol_slamap_matches_transform() {
    // Build a minimal SlaMap (Equatorial → Galactic) and exercise the
    // NaN-as-bad fast path directly, alongside a few BAD-injected points.
    // The pal-backed `transform_into_nan` should produce results identical
    // to the BAD-checked `transform_into`, including BAD propagation on
    // input.
    use starlink_ast::mappings::{SlaCvtType, SlaMap};

    let mut map = SlaMap::new();
    map.add(SlaCvtType::Eqgal, &[]).unwrap();

    let mut rows: Vec<Vec<f64>> = (0..256)
        .map(|i| {
            // RA in [0, 2π), Dec in [-π/2, π/2]
            let ra = (i as f64) * 0.01;
            let dec = -0.5 + (i as f64) * 0.001;
            vec![ra, dec]
        })
        .collect();
    rows[10][0] = BAD_VALUE;
    rows[42][1] = f64::NAN;
    rows[100][0] = f64::NAN;
    rows[100][1] = BAD_VALUE;
    let pts = PointSet::from_rows(rows);

    let regular = map.transform(&pts, true).expect("scalar");
    // Wrap as a trait object to access the NaN-protocol entry.
    let map_dyn: &dyn Mapping = &map;
    let via_nan = map_dyn
        .transform_nan_protocol(&pts, true)
        .expect("nan protocol");

    for i in 0..pts.npoint() {
        let r = regular.point(i);
        let n = via_nan.point(i);
        for axis in 0..2 {
            // Either both are finite and close, or both are BAD.
            if r[axis] == BAD_VALUE || n[axis] == BAD_VALUE {
                assert_eq!(
                    r[axis], n[axis],
                    "pt {i} ax {axis}: BAD mismatch r={} n={}",
                    r[axis], n[axis]
                );
            } else {
                let diff = (r[axis] - n[axis]).abs();
                assert!(
                    diff < 1e-14,
                    "pt {i} ax {axis}: regular={:.17e} nan={:.17e} diff={:.2e}",
                    r[axis],
                    n[axis],
                    diff
                );
            }
        }
    }
}

/// Helper: build a target/result pair via the NaN protocol entry,
/// and via the standard `transform`, and assert "invalid-equivalence"
/// per axis. A scalar path may legitimately leak NaN through
/// (CLAUDE.md says outputs should use `BAD_VALUE` but not every
/// mapping converts at exit) while the NaN-protocol path canonicalizes
/// to `BAD_VALUE` at the boundary. Both signal "invalid" — we treat
/// NaN-or-BAD as the same outcome for the parity check.
fn assert_nan_protocol_matches_transform(
    map: &dyn Mapping,
    pts: &PointSet,
    forward: bool,
    tol: f64,
) {
    let regular = map.transform(pts, forward).expect("scalar");
    let via_nan = map.transform_nan_protocol(pts, forward).expect("nan");
    let nout = if forward { map.nout() } else { map.nin() };
    let is_invalid = |v: f64| v == BAD_VALUE || v.is_nan();
    for i in 0..pts.npoint() {
        let r = regular.point(i);
        let n = via_nan.point(i);
        for axis in 0..nout {
            let r_bad = is_invalid(r[axis]);
            let n_bad = is_invalid(n[axis]);
            if r_bad || n_bad {
                assert!(
                    r_bad && n_bad,
                    "pt {i} ax {axis}: invalid-mismatch r={} n={}",
                    r[axis],
                    n[axis]
                );
            } else {
                let diff = (r[axis] - n[axis]).abs();
                assert!(
                    diff <= tol,
                    "pt {i} ax {axis}: regular={:.17e} nan={:.17e} diff={:.2e}",
                    r[axis],
                    n[axis],
                    diff
                );
            }
        }
    }
}

#[test]
fn nan_protocol_zoommap() {
    use starlink_ast::mappings::ZoomMap;
    let map = ZoomMap::new(2, 3.5).unwrap();
    let mut rows: Vec<Vec<f64>> = (0..64)
        .map(|i| vec![i as f64 * 0.1, -i as f64 * 0.2])
        .collect();
    rows[0][0] = BAD_VALUE;
    rows[10][1] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 0.0);
    assert_nan_protocol_matches_transform(m, &pts, false, 0.0);
}

#[test]
fn nan_protocol_shiftmap() {
    use starlink_ast::mappings::ShiftMap;
    let map = ShiftMap::new([1.0, -2.0]);
    let mut rows: Vec<Vec<f64>> = (0..64)
        .map(|i| vec![i as f64 * 0.1, -i as f64 * 0.2])
        .collect();
    rows[5][0] = f64::NAN;
    rows[7][1] = BAD_VALUE;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 0.0);
    assert_nan_protocol_matches_transform(m, &pts, false, 0.0);
}

#[test]
fn nan_protocol_matrixmap() {
    use ndarray::Array2;
    use starlink_ast::mappings::MatrixMap;
    // 2x2 full matrix; both rows have non-zero in both columns so the
    // "zero coefficient masks BAD" semantic isn't relevant for these
    // points — that's covered by a dedicated test below.
    let m_arr = Array2::from_shape_vec((2, 2), vec![1.5, 0.2, -0.3, 1.1]).unwrap();
    let map = MatrixMap::new(m_arr).unwrap();
    let mut rows: Vec<Vec<f64>> = (0..64)
        .map(|i| vec![i as f64 * 0.1, -i as f64 * 0.2])
        .collect();
    rows[3][0] = BAD_VALUE;
    rows[8][1] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 1e-14);
}

#[test]
fn nan_protocol_matrixmap_zero_coefficient_masks_bad() {
    use ndarray::Array2;
    use starlink_ast::mappings::MatrixMap;
    // Row 0 has zero coefficient in column 1 → output 0 must NOT see BAD
    // even when input axis 1 is BAD. Row 1 depends on both inputs.
    let m_arr = Array2::from_shape_vec((2, 2), vec![2.0, 0.0, 1.0, 3.0]).unwrap();
    let map = MatrixMap::new(m_arr).unwrap();
    let mut rows: Vec<Vec<f64>> = (0..16).map(|i| vec![i as f64, 10.0 + i as f64]).collect();
    rows[5][1] = BAD_VALUE; // BAD in column 1 only
    rows[7][1] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    let regular = m.transform(&pts, true).unwrap();
    let via_nan = m.transform_nan_protocol(&pts, true).unwrap();

    // Both BAD-in-column-1 rows: scalar path produces a valid value for axis 0
    // (since the zero coefficient masks BAD), BAD for axis 1. The NaN path
    // must match.
    for &bad_ix in &[5usize, 7] {
        let r = regular.point(bad_ix);
        let n = via_nan.point(bad_ix);
        assert_eq!(r[1], BAD_VALUE);
        assert_eq!(n[1], BAD_VALUE);
        // axis 0 is well-defined (depends only on column 0, which is finite).
        assert!(r[0].is_finite() && r[0] != BAD_VALUE);
        assert!(n[0].is_finite() && n[0] != BAD_VALUE);
        assert!((r[0] - n[0]).abs() < 1e-14);
    }
}

#[test]
fn nan_protocol_permmap() {
    use starlink_ast::mappings::PermMap;
    // 2-out permutation: inperm and outperm are 1-based axis selections.
    // outperm = [2, 1] swaps the two axes; inperm matches.
    let map = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let mut rows: Vec<Vec<f64>> = (0..32).map(|i| vec![i as f64, -(i as f64)]).collect();
    rows[2][0] = BAD_VALUE;
    rows[6][1] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 0.0);
    assert_nan_protocol_matches_transform(m, &pts, false, 0.0);
}

#[test]
fn nan_protocol_lutmap() {
    use starlink_ast::mappings::{LutInterp, LutMap};
    // 5-entry table covering x in [0, 4].
    let mut map = LutMap::new(&[10.0, 12.0, 14.0, 16.0, 18.0], 0.0, 1.0).unwrap();
    map.set_interp(LutInterp::Linear);

    let mut rows: Vec<Vec<f64>> = (0..32).map(|i| vec![i as f64 * 0.1]).collect();
    rows[3][0] = BAD_VALUE;
    rows[10][0] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 1e-14);
}

#[test]
fn nan_protocol_grismmap() {
    use starlink_ast::mappings::GrismMap;
    // Realistic-ish grism parameters.
    let map = GrismMap::new(1.5, 0.0, 5000.0e-10, 0.0, 1.0e7, 1, 1.0, 0.0);
    let mut rows: Vec<Vec<f64>> = (0..32).map(|i| vec![i as f64 * 0.001]).collect();
    rows[5][0] = BAD_VALUE;
    rows[12][0] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 1e-14);
}

#[test]
fn nan_protocol_timemap() {
    use starlink_ast::mappings::{TimeCvtType, TimeMap};
    let mut map = TimeMap::new();
    // TAI -> UTC conversion (needs an MJD-ish argument).
    map.add(
        TimeCvtType::TaiToUtc,
        &[50000.0, starlink_ast::constants::BAD_VALUE],
    )
    .unwrap();

    let mut rows: Vec<Vec<f64>> = (0..32).map(|i| vec![50000.0 + i as f64 * 0.1]).collect();
    rows[7][0] = BAD_VALUE;
    rows[11][0] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 1e-14);
}

#[test]
fn nan_protocol_specmap() {
    use starlink_ast::mappings::{SpecCvtType, SpecMap};
    let mut map = SpecMap::new();
    map.add(SpecCvtType::WnToFr, &[]).unwrap();

    let mut rows: Vec<Vec<f64>> = (1..33).map(|i| vec![i as f64]).collect();
    rows[7][0] = BAD_VALUE;
    rows[11][0] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 1e-14);
}

#[test]
fn nan_protocol_normmap() {
    use starlink_ast::frames::BasicFrameBuilder;
    use starlink_ast::mappings::NormMap;
    // BasicFrame's norm is identity, so this just tests the per-point
    // copy + (no-op) norm call path.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let map = NormMap::new(Box::new(frame));
    let mut rows: Vec<Vec<f64>> = (0..32).map(|i| vec![i as f64, -(i as f64)]).collect();
    rows[3][0] = BAD_VALUE;
    rows[9][1] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 0.0);
}

#[test]
fn nan_protocol_unitnormmap() {
    use starlink_ast::mappings::UnitNormMap;
    let map = UnitNormMap::new(vec![0.0, 0.0]).unwrap();
    let mut rows: Vec<Vec<f64>> = (1..33).map(|i| vec![i as f64, i as f64 * 0.5]).collect();
    rows[5][0] = BAD_VALUE;
    rows[7][1] = f64::NAN;
    let pts = PointSet::from_rows(rows);
    let m: &dyn Mapping = &map;
    assert_nan_protocol_matches_transform(m, &pts, true, 1e-14);
}

#[test]
fn nan_protocol_propagates_bad_and_nan_inputs() {
    let map = load_lsst();
    let mut rows: Vec<Vec<f64>> = (0..200)
        .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
        .collect();
    rows[0][0] = BAD_VALUE;
    rows[5][1] = f64::NAN;
    rows[100][0] = f64::NAN;
    rows[100][1] = BAD_VALUE;
    let pts = PointSet::from_rows(rows);

    let out = map.transform_nan_protocol(&pts, true).unwrap();

    for &i in &[0usize, 5, 100] {
        let p = out.point(i);
        assert_eq!(p[0], BAD_VALUE, "pt {i} axis 0: {}", p[0]);
        assert_eq!(p[1], BAD_VALUE, "pt {i} axis 1: {}", p[1]);
    }
    // Adjacent valid points must remain finite (NaN-or-BAD didn't leak).
    let p1 = out.point(1);
    assert!(p1[0].is_finite() && p1[0] != BAD_VALUE);
    assert!(p1[1].is_finite() && p1[1] != BAD_VALUE);
}

/// A Mapping with no NaN path of its own writes `BAD_VALUE` for a bad
/// output; inside the NaN protocol that must reach the next stage as NaN,
/// not as a number for it to scale.
#[test]
fn nan_protocol_carries_a_bad_output_of_a_mapping_without_its_own_nan_path() {
    use starlink_ast::mappings::{CmpMap, Composition, ZoomMap};
    let xph = from_native_string(
        " Begin XphMap\n    Nin = 2\n IsA Mapping\n    Order = 3\n    Type = \"HPX12\"\n End XphMap\n",
    )
    .unwrap()
    .into_mapping()
    .unwrap();
    let map: Box<dyn Mapping> = Box::new(
        CmpMap::new(
            xph,
            Box::new(ZoomMap::new(2, 0.5).unwrap()),
            Composition::Series,
        )
        .unwrap(),
    );
    // The first point is outside the projection, so XphMap makes it bad.
    let pts = PointSet::from_rows([[1000.0, 1000.0], [20.0, 20.0]]);
    let regular = map.transform(&pts, true).unwrap();
    let via_nan = map.transform_nan_protocol(&pts, true).unwrap();
    assert_eq!(regular.point(0).to_vec(), vec![BAD_VALUE, BAD_VALUE]);
    for i in 0..2 {
        assert_eq!(
            regular.point(i).to_vec(),
            via_nan.point(i).to_vec(),
            "point {i}"
        );
    }
}

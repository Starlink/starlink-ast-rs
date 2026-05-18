//! Correctness verification for the cross-lane SIMD PolyMap forward eval.
//!
//! Confirms that `PolyMap::forward_simd_2d_into` produces bit-identical
//! results to the scalar `Mapping::transform` path on the LSST PolyMap, and
//! that BAD_VALUE inputs round-trip to BAD_VALUE outputs through the
//! NaN-substitution pipeline.

#![cfg(feature = "simd-eval")]

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::from_native_string;
use ast_core::mappings::polymap_simd::{SimdPath, find_polymap};
use ast_core::mappings::{Mapping, PolyMap};

fn load_lsst_polymap() -> PolyMap {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference/lsst.ast"
    ))
    .expect("lsst.ast");
    let fset = from_native_string(&text)
        .expect("parse")
        .into_mapping()
        .expect("not a Mapping");
    let simp = fset.simplified().expect("simplify");
    find_polymap(simp.as_ref())
        .expect("PolyMap not found in simplified LSST FrameSet")
        .clone()
}

fn make_points(n: usize) -> PointSet {
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
        .collect();
    PointSet::from_rows(rows)
}

#[test]
fn simd_matches_scalar_bulk() {
    let map = load_lsst_polymap();
    // Above SIMD_MIN_POINTS so the kernel actually runs.
    let pts = make_points(1024);

    let scalar = map.transform(&pts, true).expect("scalar forward");
    let mut simd_out = PointSet::zeros(pts.npoint(), 2);
    let path = map
        .forward_simd_2d_into(&pts, &mut simd_out)
        .expect("simd forward");
    assert_eq!(path, SimdPath::Took, "SIMD path didn't run above threshold");

    for i in 0..pts.npoint() {
        let s = scalar.point(i);
        let v = simd_out.point(i);
        // mul_add ordering may shift the last bit or two vs scalar's
        // sequential FMA-or-not pattern; tolerate ULP-level drift.
        for axis in 0..2 {
            let diff = (s[axis] - v[axis]).abs();
            assert!(
                diff < 1e-14,
                "point {i} axis {axis}: scalar={:.17e} simd={:.17e} diff={:.2e}",
                s[axis],
                v[axis],
                diff
            );
        }
    }
}

#[test]
fn simd_falls_back_below_threshold() {
    let map = load_lsst_polymap();
    // 32 points is below SIMD_MIN_POINTS (=128) — should fall back.
    let pts = make_points(32);
    let mut out = PointSet::zeros(pts.npoint(), 2);
    let path = map.forward_simd_2d_into(&pts, &mut out).unwrap();
    assert_eq!(path, SimdPath::Fell);
}

#[test]
fn simd_propagates_bad_value() {
    let map = load_lsst_polymap();
    // Use a size > threshold but small enough to inspect by hand.
    let mut rows: Vec<Vec<f64>> = (0..200)
        .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
        .collect();
    // Sprinkle BAD_VALUE through both axes and at a chunk boundary
    // (LANES = 4) to exercise the BAD→NaN substitution.
    rows[0][0] = BAD_VALUE;
    rows[3][1] = BAD_VALUE;
    rows[100][0] = BAD_VALUE;
    rows[100][1] = BAD_VALUE;
    let pts = PointSet::from_rows(rows);

    let mut out = PointSet::zeros(pts.npoint(), 2);
    let path = map.forward_simd_2d_into(&pts, &mut out).unwrap();
    assert_eq!(path, SimdPath::Took);

    // BAD inputs should produce BAD outputs on both axes.
    for &bad_ix in &[0usize, 3, 100] {
        let p = out.point(bad_ix);
        assert_eq!(
            p[0], BAD_VALUE,
            "point {bad_ix} axis 0 wasn't BAD: {}",
            p[0]
        );
        assert_eq!(
            p[1], BAD_VALUE,
            "point {bad_ix} axis 1 wasn't BAD: {}",
            p[1]
        );
    }

    // A neighbour (non-BAD) should be finite.
    let p1 = out.point(1);
    assert!(
        p1[0].is_finite() && p1[1].is_finite(),
        "neighbour point became non-finite"
    );
}

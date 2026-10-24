//! Verify that scalar and SIMD PolyMap paths treat f64::NAN inputs
//! identically: both should produce BAD_VALUE outputs, per the
//! CLAUDE.md contract ("inputs accept f64::NAN; outputs always use
//! BAD_VALUE").

#![cfg(feature = "simd-eval")]

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::from_native_string;
use starlink_ast::mappings::polymap_simd::{SimdPath, find_polymap};
use starlink_ast::mappings::{Mapping, PolyMap};

fn load_lsst_polymap() -> PolyMap {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/lsst.ast"
    ))
    .expect("lsst.ast");
    let fset = from_native_string(&text).unwrap().into_mapping().unwrap();
    let simp = fset.simplify().unwrap();
    find_polymap(simp.as_ref()).unwrap().clone()
}

#[test]
fn scalar_and_simd_handle_nan_inputs_identically_forward() {
    let map = load_lsst_polymap();
    // 200 pts >= SIMD_MIN_POINTS so SIMD path will run.
    let mut rows: Vec<Vec<f64>> = (0..200)
        .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
        .collect();
    // Sprinkle in a mix of BAD_VALUE and f64::NAN inputs.
    rows[0][0] = BAD_VALUE;
    rows[5][1] = f64::NAN;
    rows[100][0] = f64::NAN;
    rows[100][1] = BAD_VALUE;
    rows[199][1] = f64::NAN;
    let pts = PointSet::from_rows(rows);

    let scalar = map.transform(&pts, true).expect("scalar forward");
    let mut simd_out = PointSet::zeros(pts.npoint(), 2);
    let path = map
        .forward_simd_2d_into(&pts.as_points(), &mut simd_out.as_points_mut())
        .expect("simd forward");
    assert_eq!(path, SimdPath::Took);

    // Every flagged input position must produce BAD on both axes in BOTH
    // paths; otherwise scalar/SIMD diverge on NaN handling.
    for &i in &[0usize, 5, 100, 199] {
        let s = scalar.point(i);
        let v = simd_out.point(i);
        assert_eq!(s[0], BAD_VALUE, "scalar pt {i} ax 0: {}", s[0]);
        assert_eq!(s[1], BAD_VALUE, "scalar pt {i} ax 1: {}", s[1]);
        assert_eq!(v[0], BAD_VALUE, "simd pt {i} ax 0: {}", v[0]);
        assert_eq!(v[1], BAD_VALUE, "simd pt {i} ax 1: {}", v[1]);
    }
}

#[test]
fn scalar_and_simd_handle_nan_inputs_identically_inverse() {
    let map = load_lsst_polymap();
    let pixel_pts = PointSet::from_rows(
        (0..200)
            .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
            .collect::<Vec<_>>(),
    );
    let sky = map.transform(&pixel_pts, true).expect("forward");
    let mut rows: Vec<Vec<f64>> = (0..sky.npoint()).map(|i| sky.point(i).to_vec()).collect();
    rows[0][0] = BAD_VALUE;
    rows[5][1] = f64::NAN;
    rows[100][0] = f64::NAN;
    rows[100][1] = BAD_VALUE;
    rows[199][1] = f64::NAN;
    let target = PointSet::from_rows(rows);

    let scalar = map.transform(&target, false).expect("scalar inverse");
    let mut simd_out = PointSet::zeros(target.npoint(), 2);
    let path = map
        .inverse_simd_2d_into(&target.as_points(), &mut simd_out.as_points_mut())
        .expect("simd inverse");
    assert_eq!(path, SimdPath::Took);

    for &i in &[0usize, 5, 100, 199] {
        let s = scalar.point(i);
        let v = simd_out.point(i);
        assert_eq!(s[0], BAD_VALUE, "scalar pt {i} ax 0: {}", s[0]);
        assert_eq!(s[1], BAD_VALUE, "scalar pt {i} ax 1: {}", s[1]);
        assert_eq!(v[0], BAD_VALUE, "simd pt {i} ax 0: {}", v[0]);
        assert_eq!(v[1], BAD_VALUE, "simd pt {i} ax 1: {}", v[1]);
    }
}

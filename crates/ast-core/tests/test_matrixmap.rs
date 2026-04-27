//! Tests for MatrixMap.
//!
//! Ancestor: (no direct C test — MatrixMap tests derived from the C AST
//! MatrixMap documentation and reference behaviour.)

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, MatrixMap};
use ndarray::array;

/// 2×3 matrix — nin = 3, nout = 2.
#[test]
fn matrix_map_non_square_dimensions() {
    let m = array![[1.0_f64, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let mm = MatrixMap::new(m).unwrap();
    assert_eq!(mm.nin(), 3);
    assert_eq!(mm.nout(), 2);
}

/// 2×2 identity matrix should pass points through unchanged.
#[test]
fn matrix_map_identity_passthrough() {
    let m = array![[1.0_f64, 0.0], [0.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[3.0, 7.0], [-1.0, 4.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 7.0]);
    assert_eq!(out.point(1), &[-1.0, 4.0]);
}

/// 90-degree rotation: [[0, -1], [1, 0]] maps (1,0) → (0,1).
#[test]
fn matrix_map_rotation_90() {
    let m = array![[0.0_f64, -1.0], [1.0, 0.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[1.0, 0.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 0.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 1.0).abs() < 1e-12);
}

/// Scale by 2, then inverse should divide by 2.
#[test]
fn matrix_map_inverse_scale() {
    let m = array![[2.0_f64, 0.0], [0.0, 2.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[4.0, 6.0]]);
    let out = mm.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 3.0).abs() < 1e-12);
}

/// Non-square matrix — inverse is not available.
#[test]
fn matrix_map_non_square_inverse_error() {
    let m = array![[1.0_f64, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    assert!(mm.transform(&pts, false).is_err());
}

/// BAD_VALUE in any input coord → whole output point is BAD_VALUE.
#[test]
fn matrix_map_bad_value_propagation() {
    let m = array![[1.0_f64, 0.0], [0.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 3.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

/// simplified() on an identity matrix should return a working UnitMap.
#[test]
fn matrix_map_simplified_identity() {
    let m = array![[1.0_f64, 0.0], [0.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();
    let simplified = mm.simplified().unwrap();
    // UnitMap pass-through
    let pts = PointSet::from_rows([[5.0, 9.0]]);
    let out = simplified.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[5.0, 9.0]);
}

/// simplified() on a non-identity matrix returns the original mapping.
#[test]
fn matrix_map_simplified_non_identity_preserved() {
    let m = array![[2.0_f64, 0.0], [0.0, 3.0]];
    let mm = MatrixMap::new(m).unwrap();
    let simplified = mm.simplified().unwrap();
    let pts = PointSet::from_rows([[1.0, 1.0]]);
    let out = simplified.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 3.0).abs() < 1e-12);
}

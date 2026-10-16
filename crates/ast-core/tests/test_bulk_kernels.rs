//! Whole-axis kernels: non-finite inputs and outputs longer than the input.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::{Points, PointsMut};
use ast_core::mappings::{Mapping, ShiftMap, UnitMap, ZoomMap};

/// Every non-finite input is bad: BAD, NaN, and both infinities.
const ODD: [f64; 5] = [BAD_VALUE, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.0];

/// Transform `ODD` on both axes into outputs one point longer, which must
/// keep their last value.
fn run(map: &dyn Mapping, forward: bool) -> [Vec<f64>; 2] {
    let mut out = [vec![7.0; ODD.len() + 1], vec![7.0; ODD.len() + 1]];
    let [a, b] = &mut out;
    map.transform_into(
        Points::new([&ODD[..], &ODD[..]]).unwrap(),
        forward,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    out
}

fn assert_bad_then(axis: &[f64], good: f64) {
    assert!(axis[..4].iter().all(|&v| v == BAD_VALUE), "{axis:?}");
    assert_eq!(axis[4], good);
    assert_eq!(axis[5], 7.0, "a point past the input was written");
}

#[test]
fn zoommap_kernel() {
    let m = ZoomMap::new(2, 4.0).unwrap();
    for axis in run(&m, true) {
        assert_bad_then(&axis, 8.0);
    }
    for axis in run(&m, false) {
        assert_bad_then(&axis, 0.5);
    }
}

#[test]
fn shiftmap_kernel() {
    let m = ShiftMap::new(vec![1.0, -1.0]);
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 3.0);
    assert_bad_then(&b, 1.0);
}

#[test]
fn unitmap_kernel() {
    let m = UnitMap::new(2);
    let [a, _] = run(&m, true);
    // UnitMap replaces only NaN; it passes infinities through.
    assert_eq!(
        a[..5],
        [BAD_VALUE, BAD_VALUE, f64::INFINITY, f64::NEG_INFINITY, 2.0]
    );
    assert_eq!(a[5], 7.0);
}

use ast_core::mappings::{PermMap, WinMap};

#[test]
fn winmap_kernel() {
    // Axis 1 is scaled by 2; axis 2 is shifted by 1.
    let m = WinMap::new(&[0.0, 1.0], &[2.0, 1.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 4.0);
    assert_bad_then(&b, 3.0);
    let [a, b] = run(&m, false);
    assert_bad_then(&a, 1.0);
    assert_bad_then(&b, 1.0);
}

#[test]
fn permmap_kernel() {
    // Output 1 is input 2, output 2 is the constant 5.
    let m = PermMap::new(&[2, 1], &[2, -1], &[5.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 2.0);
    assert!(b[..5].iter().all(|&v| v == 5.0), "{b:?}");
    assert_eq!(b[5], 7.0);
}

use ast_core::mappings::MatrixMap;

#[test]
fn diagonal_matrixmap_kernel() {
    let m = MatrixMap::diagonal(2, 2, &[2.0, -1.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 4.0);
    assert_bad_then(&b, -2.0);
}

#[test]
fn full_matrixmap_kernel_ignores_a_bad_input_with_a_zero_coefficient() {
    // Output 1 = 1*x + 0*y ; output 2 = 3*x + 2*y.
    let m = MatrixMap::new(ndarray::array![[1.0, 0.0], [3.0, 2.0]]).unwrap();
    let x = [1.0, 2.0, BAD_VALUE];
    let y = [BAD_VALUE, 1.0, 1.0];
    let mut out = [vec![0.0; 3], vec![0.0; 3]];
    let [a, b] = &mut out;
    m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        true,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    assert_eq!(out[0], [1.0, 2.0, BAD_VALUE]);
    assert_eq!(out[1], [BAD_VALUE, 8.0, BAD_VALUE]);
}

#[test]
fn full_matrixmap_kernel_crosses_its_block_boundary() {
    let m = MatrixMap::new(ndarray::array![[1.0, 2.0], [3.0, 4.0]]).unwrap();
    let n = 1000;
    let x: Vec<f64> = (0..n).map(f64::from).collect();
    let y: Vec<f64> = (0..n).map(|i| f64::from(i) * 0.5).collect();
    let mut out = [vec![0.0; x.len()], vec![0.0; x.len()]];
    let [a, b] = &mut out;
    m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        true,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    for i in 0..x.len() {
        assert_eq!(out[0][i], 0.0 + 1.0 * x[i] + 2.0 * y[i]);
        assert_eq!(out[1][i], 0.0 + 3.0 * x[i] + 4.0 * y[i]);
    }
}

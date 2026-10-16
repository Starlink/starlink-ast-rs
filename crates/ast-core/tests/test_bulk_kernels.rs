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

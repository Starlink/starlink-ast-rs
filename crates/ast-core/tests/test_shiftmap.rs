use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, ShiftMap};

#[test]
fn shift_map_dimensions() {
    let m = ShiftMap::new([1.0, 2.0, 3.0]);
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);
}

#[test]
fn shift_map_forward() {
    let m = ShiftMap::new([10.0, 20.0]);
    let pts = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[11.0, 22.0]);
    assert_eq!(out.point(1), &[13.0, 24.0]);
}

#[test]
fn shift_map_inverse() {
    let m = ShiftMap::new([10.0, 20.0]);
    let pts = PointSet::from_rows([[11.0, 22.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
}

#[test]
fn shift_map_bad_value() {
    let m = ShiftMap::new([1.0, 2.0]);
    let pts = PointSet::from_rows([[f64::NAN, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn shift_map_simplified_zero() {
    let m = ShiftMap::new([0.0, 0.0]);
    let s = m.simplified().unwrap();
    let pts = PointSet::from_rows([[5.0, 6.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[5.0, 6.0]);
}

#[test]
fn shift_map_rate() {
    let m = ShiftMap::new([10.0, 20.0]);
    let r = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r - 1.0).abs() < 1e-6);
}

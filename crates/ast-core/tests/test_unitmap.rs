use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, UnitMap};

#[test]
fn unit_map_dimensions() {
    let m = UnitMap::new(3);
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);
    assert!(!m.is_inverted());
}

#[test]
fn unit_map_forward_transform() {
    let m = UnitMap::new(2);
    let pts = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
    assert_eq!(out.point(1), &[3.0, 4.0]);
}

#[test]
fn unit_map_inverse_transform() {
    let m = UnitMap::new(2);
    let pts = PointSet::from_rows([[5.0, 6.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[5.0, 6.0]);
}

#[test]
fn unit_map_bad_value_passthrough() {
    let m = UnitMap::new(2);
    let pts = PointSet::from_rows([[BAD_VALUE, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[BAD_VALUE, 2.0]);
}

#[test]
fn unit_map_invert_is_noop() {
    let mut m = UnitMap::new(2);
    m.invert();
    assert!(m.is_inverted());
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
}

#[test]
fn unit_map_simplified_returns_self() {
    let m = UnitMap::new(2);
    let s = <dyn Mapping>::simplify(&m).unwrap();
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);
}

#[test]
fn unit_map_rate() {
    let m = UnitMap::new(2);
    let r = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r - 1.0).abs() < 1e-6);
    let r = m.rate(&[1.0, 1.0], 1, 2).unwrap();
    assert!(r.abs() < 1e-6);
}

#[test]
fn unit_rate_analytical() {
    let u = UnitMap::new(3);
    assert_eq!(u.rate(&[1.0, 2.0, 3.0], 1, 1).unwrap(), 1.0);
    assert_eq!(u.rate(&[1.0, 2.0, 3.0], 1, 2).unwrap(), 0.0);
    assert_eq!(u.rate(&[1.0, 2.0, 3.0], 2, 2).unwrap(), 1.0);
    assert_eq!(u.rate(&[1.0, 2.0, 3.0], 3, 1).unwrap(), 0.0);
    assert_eq!(u.rate(&[1.0, 2.0, 3.0], 3, 3).unwrap(), 1.0);
}

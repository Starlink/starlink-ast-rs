use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, ZoomMap};

#[test]
fn zoom_map_dimensions() {
    let m = ZoomMap::new(3, 2.5).unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);
}

#[test]
fn zoom_map_zero_factor_rejected() {
    assert!(ZoomMap::new(2, 0.0).is_err());
}

#[test]
fn zoom_map_forward() {
    let m = ZoomMap::new(2, 3.0).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0], [4.0, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 6.0]);
    assert_eq!(out.point(1), &[12.0, 15.0]);
}

#[test]
fn zoom_map_inverse() {
    let m = ZoomMap::new(2, 4.0).unwrap();
    let pts = PointSet::from_rows([[8.0, 12.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[2.0, 3.0]);
}

#[test]
fn zoom_map_bad_value() {
    let m = ZoomMap::new(2, 2.0).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

#[test]
fn zoom_map_inverted_flag() {
    let mut m = ZoomMap::new(2, 2.0).unwrap();
    m.invert();
    let pts = PointSet::from_rows([[4.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[2.0, 3.0]);
}

#[test]
fn zoom_map_simplified_identity() {
    let m = ZoomMap::new(2, 1.0).unwrap();
    let s = m.simplified().unwrap();
    let pts = PointSet::from_rows([[7.0, 8.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 8.0]);
}

#[test]
fn zoom_map_rate() {
    let m = ZoomMap::new(2, 3.0).unwrap();
    let r = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r - 3.0).abs() < 1e-4);
}

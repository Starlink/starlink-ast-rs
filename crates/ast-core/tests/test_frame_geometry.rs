//! Frame geometry/axis method family — Cartesian base implementations
//! ported from frame.c (Offset2:8228, Intersect:6243, AxAngle:1324,
//! AxDistance:1466, AxOffset:1744, Resolve:9429, ResolvePoints:9588).

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};

fn bf(n: usize) -> impl Frame {
    BasicFrameBuilder::new(n).build().unwrap()
}

#[test]
fn offset2_cartesian() {
    let f = bf(2);
    // angle 0 => +axis2 direction; offset 5 => point moves +5 on axis 2.
    let (p2, ret) = f.offset2(&[1.0, 2.0], 0.0, 5.0).unwrap();
    assert!((p2[0] - 1.0).abs() < 1e-12);
    assert!((p2[1] - 7.0).abs() < 1e-12);
    assert!((ret - 0.0).abs() < 1e-12);
}

#[test]
fn offset2_bad_inputs() {
    let f = bf(2);
    let (p2, ret) = f.offset2(&[BAD_VALUE, 2.0], 0.0, 5.0).unwrap();
    assert_eq!(p2[0], BAD_VALUE);
    assert_eq!(p2[1], BAD_VALUE);
    assert_eq!(ret, BAD_VALUE);
}

#[test]
fn intersect_crossing_lines() {
    let f = bf(2);
    // Line a: y=x through (0,0)-(1,1). Line b: y=2-x through (0,2)-(1,1).
    let cross = f
        .intersect(&[0.0, 0.0], &[1.0, 1.0], &[0.0, 2.0], &[2.0, 0.0])
        .unwrap();
    assert!((cross[0] - 1.0).abs() < 1e-12);
    assert!((cross[1] - 1.0).abs() < 1e-12);
}

#[test]
fn intersect_parallel_is_bad() {
    let f = bf(2);
    let cross = f
        .intersect(&[0.0, 0.0], &[1.0, 1.0], &[0.0, 1.0], &[1.0, 2.0])
        .unwrap();
    assert_eq!(cross[0], BAD_VALUE);
    assert_eq!(cross[1], BAD_VALUE);
}

#[test]
fn ax_distance_and_offset() {
    let f = bf(2);
    assert!((f.ax_distance(1, 3.0, 8.0).unwrap() - 5.0).abs() < 1e-12);
    assert_eq!(f.ax_distance(1, BAD_VALUE, 8.0).unwrap(), BAD_VALUE);
    assert!((f.ax_offset(1, 3.0, 5.0).unwrap() - 8.0).abs() < 1e-12);
    assert_eq!(f.ax_offset(1, BAD_VALUE, 5.0).unwrap(), BAD_VALUE);
}

#[test]
fn ax_angle_cartesian() {
    let f = bf(2);
    // Oracle-confirmed against the C library (astAxAngle, libast built
    // with cmake). For a=(0,0), b=(1,0), axis=1 the C nudge ladder hits
    // the `a[axis-1]==0 && b[axis-1]!=0` branch, which the C code offsets
    // by |b[naxes-1]| (a stale loop index); since b[1]==0 here the
    // nudged point coincides with `a`, giving a zero-length vector and a
    // bad angle. We mirror that bit-for-bit.
    assert_eq!(f.ax_angle(&[0.0, 0.0], &[1.0, 0.0], 1).unwrap(), BAD_VALUE);
    // axis=2 takes the mxab fallback: PI/2.
    let ang2 = f.ax_angle(&[0.0, 0.0], &[1.0, 0.0], 2).unwrap();
    assert!((ang2 - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
    // a=(0,5), b=(3,2), axis=1 hits the b[naxes-1] branch with b[1]=2,
    // giving PI/4 (oracle: 0.78539816339744828).
    let ang3 = f.ax_angle(&[0.0, 5.0], &[3.0, 2.0], 1).unwrap();
    assert!((ang3 - std::f64::consts::FRAC_PI_4).abs() < 1e-6);
}

#[test]
fn ax_norm_is_noop_for_cartesian() {
    let f = bf(2);
    let mut vals = vec![1.0, 2.0, 3.0];
    f.ax_norm(1, 0, &mut vals).unwrap();
    assert_eq!(vals, vec![1.0, 2.0, 3.0]);
}

#[test]
fn resolve_parallel_perp() {
    let f = bf(2);
    // basis (0,0)->(2,0); point3 = (1,3). parallel d1=1, perp d2=3.
    let (p4, d1, d2) = f.resolve(&[0.0, 0.0], &[2.0, 0.0], &[1.0, 3.0]).unwrap();
    assert!((d1 - 1.0).abs() < 1e-12);
    assert!((d2 - 3.0).abs() < 1e-12);
    assert!((p4[0] - 1.0).abs() < 1e-12);
    assert!((p4[1] - 0.0).abs() < 1e-12);
}

#[test]
fn resolve_zero_basis_is_bad() {
    let f = bf(2);
    let (_p4, d1, d2) = f.resolve(&[0.0, 0.0], &[0.0, 0.0], &[1.0, 3.0]).unwrap();
    assert_eq!(d1, BAD_VALUE);
    assert_eq!(d2, BAD_VALUE);
}

#[test]
fn resolve_points_signed_2d() {
    let f = bf(2);
    // basis (0,0)->(1,0). Two test points above and below the basis.
    // Matches the C library sign convention `x1*y2 - x2*y1 < 0 => negate`
    // (frame.c:9852); confirmed against the frame.c ResolvePoints source.
    let inset = PointSet::from_rows([[1.0_f64, 2.0], [1.0, -2.0]]);
    let out = f.resolve_points(&[0.0, 0.0], &[1.0, 0.0], &inset).unwrap();
    // d1 (axis 0) = 1.0 for both; d2 (axis 1) signed: +2 then -2.
    assert!((out.point(0)[0] - 1.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 2.0).abs() < 1e-12);
    assert!((out.point(1)[1] + 2.0).abs() < 1e-12);
}

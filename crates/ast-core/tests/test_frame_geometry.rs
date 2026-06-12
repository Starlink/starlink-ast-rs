//! Frame geometry/axis method family — Cartesian base implementations
//! ported from frame.c (Offset2:8228, Intersect:6243, AxAngle:1324,
//! AxDistance:1466, AxOffset:1744, Resolve:9429, ResolvePoints:9588).

use ast_core::constants::BAD_VALUE;
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

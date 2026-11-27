// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Frame geometry with an infinite coordinate, which the C library takes
//! to be a number, as it tests a coordinate only for being `AST__BAD`.
//!
//! The values are those of a probe of the C library at the pinned commit
//! (printed with `%g`, so compared to six significant figures), for a plain
//! Frame, a SkyFrame and a CmpFrame of two 1-D Frames. A result that C
//! computes as NaN from infinities is NaN here too.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::frames::{BasicFrameBuilder, CmpFrame, DynFrame, Frame, SkyFrame};

const I: f64 = f64::INFINITY;
const N: f64 = f64::NAN;

/// Whether `got` is C's `want`: both NaN, both bad, or equal to six
/// significant figures.
fn same(got: f64, want: f64) -> bool {
    if want.is_nan() {
        return got.is_nan();
    }
    if want == BAD_VALUE || want.is_infinite() || want == 0.0 {
        return got == want;
    }
    ((got - want) / want).abs() < 1.0e-5
}

fn check(what: &str, got: &[f64], want: &[f64]) {
    assert!(
        got.len() == want.len() && got.iter().zip(want).all(|(&g, &w)| same(g, w)),
        "{what}: got {got:?}, C gives {want:?}"
    );
}

/// The probe's cases for `frame`, with C's results in `want`, in the order
/// the probe prints them.
fn geometry(name: &str, frame: &dyn Frame, want: &[&[f64]]) {
    let (a, b, c, d) = ([0.0, 0.0], [I, 0.0], [0.0, 1.0], [1.0, 0.0]);
    let mut got: Vec<Vec<f64>> = vec![
        vec![frame.distance(&a, &b).unwrap()],
        vec![frame.distance(&b, &b).unwrap()],
        vec![frame.angle(&c, &a, &b).unwrap()],
        vec![frame.ax_angle(&a, &b, 1).unwrap()],
        vec![frame.ax_distance(1, 0.0, I).unwrap()],
        vec![frame.ax_offset(1, 0.0, I).unwrap()],
        vec![frame.ax_offset(1, I, 1.0).unwrap()],
        frame.offset(&a, &b, 1.0).unwrap(),
        frame.offset(&a, &d, I).unwrap(),
    ];
    let (p, ang) = frame.offset2(&a, 0.5, I).unwrap();
    got.push([p, vec![ang]].concat());
    let (p, ang) = frame.offset2(&b, 0.5, 1.0).unwrap();
    got.push([p, vec![ang]].concat());
    let (p, d1, d2) = frame.resolve(&a, &d, &[I, 1.0]).unwrap();
    got.push([p, vec![d1, d2]].concat());
    let mut n = [I, 0.5];
    frame.norm(&mut n).unwrap();
    got.push(n.to_vec());
    got.push(frame.intersect(&a, &b, &[0.5, -1.0], &[0.5, 1.0]).unwrap());
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        check(&format!("{name} case {i}"), g, w);
    }
}

#[test]
fn plain_frame() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    geometry(
        "frame",
        &frame,
        &[
            &[I],
            &[N],
            &[std::f64::consts::FRAC_PI_2],
            &[0.0],
            &[I],
            &[I],
            &[I],
            &[N, 0.0],
            &[I, N],
            &[I, I, 0.5],
            &[I, 0.877583, 0.5],
            &[I, N, I, N],
            &[I, 0.5],
            &[0.5, 0.0],
        ],
    );
}

#[test]
fn sky_frame() {
    let frame = SkyFrame::unset_system();
    geometry(
        "sky",
        &frame,
        &[
            &[N],
            &[N],
            &[N],
            &[N],
            &[N],
            &[N],
            &[N],
            &[N, N],
            &[N, N],
            &[N, N, N],
            &[N, 0.830784, N],
            &[BAD_VALUE, BAD_VALUE, BAD_VALUE, BAD_VALUE],
            &[N, 0.5],
            &[N, N],
        ],
    );
}

#[test]
fn cmp_frame() {
    let one = || -> DynFrame { Box::new(BasicFrameBuilder::new(1).build().unwrap()) };
    let frame = CmpFrame::new(one(), one()).unwrap();
    geometry(
        "cmp",
        &frame,
        &[
            &[I],
            &[N],
            &[std::f64::consts::FRAC_PI_2],
            &[0.0],
            &[I],
            &[I],
            &[I],
            &[N, 0.0],
            &[BAD_VALUE, BAD_VALUE],
            &[I, I, 0.5],
            &[I, 0.877583, 0.5],
            &[BAD_VALUE, BAD_VALUE, I, BAD_VALUE],
            &[I, 0.5],
            &[0.5, 0.0],
        ],
    );
}

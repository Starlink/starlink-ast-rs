// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A SplineMap at the edges of the area its spline covers.
//!
//! Ancestor: `ast_tester/testsplinemapedge.c`, whose six cases are
//! `c_*` here. Positions on the edges and corners of the area covered must
//! give the spline's value, and positions just outside it, or anywhere when
//! the knots on an axis are all equal, must give a bad value.
//!
//! Added here: knots that are not coincident at the ends ("open" knots),
//! where the spline covers less than the full range of the knots, and a NaN
//! input. Their expected values come from a probe of the C library at the
//! pinned commit.
//!
//! C's `astSplineMap` takes the coefficients with the Y index varying
//! fastest, which is [`SplineMap::from_scipy`]'s order.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::mappings::{Mapping, SplineMap};

fn uval(x: f64, y: f64) -> f64 {
    1.0 + 2.0 * x + 3.0 * y + 4.0 * x * y
}

fn vval(x: f64, y: f64) -> f64 {
    5.0 - x + 2.0 * y - 3.0 * x * y
}

/// What a case expects inside the area covered.
enum Inside {
    Bilinear,
    Constant(f64, f64),
    /// Every position is bad.
    Nothing,
}

/// C's `check_map`: transform a grid over the edges, corners and interior
/// of (0:xhi, 0:yhi), and positions just outside it.
fn check_map(map: &SplineMap, xhi: f64, yhi: f64, inside: Inside) {
    let frac = [-1.0e-6, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0 + 1.0e-6];
    let mut xin = Vec::new();
    let mut yin = Vec::new();
    for fy in frac {
        for fx in frac {
            xin.push(fx * xhi);
            yin.push(fy * yhi);
        }
    }
    let (u, v) = map.tran2(&xin, &yin, true).unwrap();
    let mut failures = Vec::new();
    for iy in 0..7 {
        for ix in 0..7 {
            let i = ix + 7 * iy;
            let interior = ix > 0 && ix < 6 && iy > 0 && iy < 6;
            let expected = match (&inside, interior) {
                (Inside::Nothing, _) | (_, false) => None,
                (Inside::Bilinear, true) => Some((uval(xin[i], yin[i]), vval(xin[i], yin[i]))),
                (Inside::Constant(a, b), true) => Some((*a, *b)),
            };
            let ok = match expected {
                None => u[i] == BAD_VALUE && v[i] == BAD_VALUE,
                Some((ue, ve)) => {
                    u[i] != BAD_VALUE
                        && v[i] != BAD_VALUE
                        && (u[i] - ue).abs() <= 1.0e-12
                        && (v[i] - ve).abs() <= 1.0e-12
                }
            };
            if !ok {
                failures.push(format!(
                    "at ({}, {}) gave ({}, {}), expected {expected:?}",
                    xin[i], yin[i], u[i], v[i]
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const T2: [f64; 4] = [0.0, 0.0, 1.0, 1.0];
const T1: [f64; 2] = [0.0, 1.0];
const T22: [f64; 5] = [0.0, 0.0, 1.0, 2.0, 2.0];
const TZERO: [f64; 4] = [1.0; 4];

/// Coefficients of a bilinear order-2 spline over `nx` by `ny` knots at the
/// integers: its values there, with the Y index varying fastest.
fn coeffs(nx: usize, ny: usize, f: fn(f64, f64) -> f64) -> Vec<f64> {
    let mut c = vec![0.0; nx * ny];
    for ix in 0..nx {
        for iy in 0..ny {
            c[iy + ny * ix] = f(ix as f64, iy as f64);
        }
    }
    c
}

fn bilinear(tx: &[f64], ty: &[f64], nx: usize, ny: usize) -> SplineMap {
    SplineMap::from_scipy(
        2,
        2,
        nx,
        ny,
        tx.to_vec(),
        ty.to_vec(),
        &coeffs(nx, ny, uval),
        &coeffs(nx, ny, vval),
    )
    .unwrap()
}

#[test]
fn c_bilinear_with_one_interval_per_axis() {
    check_map(&bilinear(&T2, &T2, 2, 2), 1.0, 1.0, Inside::Bilinear);
}

#[test]
fn c_constant_with_one_interval_per_axis() {
    let map = SplineMap::from_scipy(1, 1, 1, 1, T1.to_vec(), T1.to_vec(), &[7.0], &[-3.0]).unwrap();
    check_map(&map, 1.0, 1.0, Inside::Constant(7.0, -3.0));
}

#[test]
fn c_two_x_intervals_and_one_y_interval() {
    check_map(&bilinear(&T22, &T2, 3, 2), 2.0, 1.0, Inside::Bilinear);
}

#[test]
fn c_one_x_interval_and_two_y_intervals() {
    check_map(&bilinear(&T2, &T22, 2, 3), 1.0, 2.0, Inside::Bilinear);
}

#[test]
fn c_equal_x_knots_cover_nothing() {
    check_map(&bilinear(&TZERO, &T2, 2, 2), 1.0, 1.0, Inside::Nothing);
}

#[test]
fn c_equal_y_knots_cover_nothing() {
    check_map(&bilinear(&T2, &TZERO, 2, 2), 1.0, 1.0, Inside::Nothing);
}

/// The values C gives for (x, y) on the SplineMap of order 2 on each axis
/// with the given knots and the coefficients 1 to 6 for both outputs.
fn open_knots(tx: &[f64], ty: &[f64], nx: usize, ny: usize, cases: &[(f64, f64, f64)]) {
    let c: Vec<f64> = (1..=6).map(f64::from).collect();
    let map = SplineMap::from_scipy(2, 2, nx, ny, tx.to_vec(), ty.to_vec(), &c, &c).unwrap();
    for &(x, y, expected) in cases {
        let (u, v) = map.tran2(&[x], &[y], true).unwrap();
        assert_eq!((u[0], v[0]), (expected, expected), "at ({x}, {y})");
    }
}

#[test]
fn open_y_knots_cover_only_the_basic_interval() {
    // Along Y the spline covers knot 2 to knot 4 (1 to 3).
    open_knots(
        &T2,
        &[0.0, 1.0, 2.0, 3.0, 4.0],
        2,
        3,
        &[
            (0.5, 0.5, BAD_VALUE),
            (0.5, 3.0, 4.5),
            (0.5, 3.5, BAD_VALUE),
            (0.5, 2.0, 3.5),
        ],
    );
}

#[test]
fn open_x_knots_cover_only_the_basic_interval() {
    open_knots(
        &[0.0, 1.0, 2.0, 3.0, 4.0],
        &T2,
        3,
        2,
        &[
            (0.5, 0.3, BAD_VALUE),
            (0.5, 0.1, BAD_VALUE),
            (2.0, 0.5, 3.5),
            (3.5, 0.5, BAD_VALUE),
        ],
    );
}

#[test]
fn nan_is_outside_the_spline() {
    let map = bilinear(&T2, &T2, 2, 2);
    for (x, y) in [(f64::NAN, 0.5), (0.5, f64::NAN), (f64::NAN, f64::NAN)] {
        let (u, v) = map.tran2(&[x], &[y], true).unwrap();
        assert_eq!((u[0], v[0]), (BAD_VALUE, BAD_VALUE), "at ({x}, {y})");
    }
    assert!(map.has_forward());
}

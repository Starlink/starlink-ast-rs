// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! `Mapping::rate` for each basic Mapping class and for a compound Mapping.
//!
//! Ancestor: `reference/starlink-ast/ast_tester/testrate.c`. Each test is one
//! block of the C program, with its error numbers in the assertion messages.
//!
//! Divergences from the C original:
//!
//! * C builds the unit MatrixMap from its unit form; this API makes one from a
//!   diagonal of ones, which C's diagonal form also stores as a unit
//!   MatrixMap.
//! * C's CmpMap records each component's Invert flag when it is built, so the
//!   `astInvert` calls C makes on the components afterwards do not change the
//!   compound Mapping. Owned components are built here in the orientation they
//!   had when C composed them, and those later calls have no counterpart.

use ndarray::array;
use starlink_ast::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MatrixMap, PermMap, ShiftMap, TranMap, UnitMap,
    WinMap, ZoomMap,
};

/// The position C evaluates the 2-D rates at.
const AT: [f64; 2] = [10.0, 1.2e6];

fn rate(m: &dyn Mapping, at: &[f64], ax1: usize, ax2: usize) -> f64 {
    m.rate(at, ax1, ax2).unwrap()
}

/// The WinMap C uses twice: (0,0)-(1,1) onto (0.5,0.5)-(2.5,2.5).
fn win_map() -> WinMap {
    WinMap::from_corners(&[0.0, 0.0], &[1.0, 1.0], &[0.5, 0.5], &[2.5, 2.5]).unwrap()
}

/// The full MatrixMap C uses twice.
fn full_matrix() -> MatrixMap {
    MatrixMap::new(array![[1.2, 1.6], [-1.6, 2.2]]).unwrap()
}

/// Rates of the inverse of [`full_matrix`], Errors 15-18 and 51-54.
fn assert_inverse_full_matrix_rates(m: &dyn Mapping, errs: [u32; 4]) {
    let close = |r: f64, want: f64, err: u32| {
        assert!(
            (r - want).abs() <= 1.0e-6,
            "Error {err}: rate {r}, want {want}"
        );
    };
    close(rate(m, &AT, 1, 1), 0.423076923, errs[0]);
    close(rate(m, &AT, 2, 2), 0.230769231, errs[1]);
    close(rate(m, &AT, 1, 2), -0.307692308, errs[2]);
    close(rate(m, &AT, 2, 1), 0.307692308, errs[3]);
}

/// The compound Mapping of the C program's last block (and of
/// `testratemap.c`): a parallel ShiftMap and full MatrixMap, followed by the
/// inverse of a PermMap followed by an inverted ZoomMap.
pub(crate) fn compound_map() -> CmpMap {
    let c3 = CmpMap::new(
        Box::new(ShiftMap::new(vec![-1.0, 1.0])),
        Box::new(MatrixMap::new(array![[1.0, 2.0], [-2.0, 3.0]]).unwrap()),
        Composition::Parallel,
    )
    .unwrap();
    let c1 = PermMap::new(&[3, 4, 1, 2], &[3, 4, 1, 2], &[]).unwrap();
    let mut c2 = ZoomMap::new(4, 0.25).unwrap();
    c2.invert();
    let mut c4 = CmpMap::new(Box::new(c1), Box::new(c2), Composition::Series).unwrap();
    c4.invert();
    CmpMap::new(Box::new(c3), Box::new(c4), Composition::Series).unwrap()
}

/// The rates of [`compound_map`] at (1,2,3,4), as `(output axis, input
/// axis, value, exact)`, where `exact` is false for the rates C compares to
/// 1e-6 rather than exactly. C's error numbers run on from 55.
pub(crate) const COMPOUND_FORWARD: [(usize, usize, f64, bool); 8] = [
    (1, 1, 0.0, true),
    (1, 2, 0.0, true),
    (1, 3, 0.25, false),
    (1, 4, 0.5, true),
    (3, 1, 0.25, true),
    (3, 2, 0.0, true),
    (3, 3, 0.0, true),
    (3, 4, 0.0, true),
];

/// As [`COMPOUND_FORWARD`], for the inverted Mapping; error numbers from 63.
pub(crate) const COMPOUND_INVERSE: [(usize, usize, f64, bool); 8] = [
    (1, 1, 0.0, true),
    (1, 2, 0.0, true),
    (1, 3, 4.0, true),
    (1, 4, 0.0, true),
    (3, 1, 12.0 / 7.0, false),
    (3, 2, -8.0 / 7.0, false),
    (3, 3, 0.0, true),
    (3, 4, 0.0, true),
];

#[test]
fn unit_map() {
    let mut m = UnitMap::new(2);
    assert_eq!(rate(&m, &AT, 1, 1), 1.0, "Error 1");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 2");
    m.invert();
    assert_eq!(rate(&m, &AT, 1, 1), 1.0, "Error 202");
}

#[test]
fn zoom_map() {
    let mut m = ZoomMap::new(2, 2.0).unwrap();
    assert_eq!(rate(&m, &AT, 1, 1), 2.0, "Error 3");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 4");
    m.invert();
    assert_eq!(rate(&m, &AT, 1, 1), 0.5, "Error 402");
}

#[test]
fn unit_matrix_map() {
    let mut m = MatrixMap::diagonal(2, 2, &[1.0, 1.0]).unwrap();
    assert_eq!(rate(&m, &AT, 1, 1), 1.0, "Error 5");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 6");
    m.invert();
    assert_eq!(rate(&m, &AT, 1, 1), 1.0, "Error 602");
    assert_eq!(rate(&m, &AT, 1, 2), 0.0, "Error 603");
}

#[test]
fn diagonal_matrix_map() {
    let mut m = MatrixMap::diagonal(2, 2, &[-2.0, 1.5]).unwrap();
    assert_eq!(rate(&m, &AT, 1, 1), -2.0, "Error 7");
    assert_eq!(rate(&m, &AT, 2, 2), 1.5, "Error 8");
    assert_eq!(rate(&m, &AT, 1, 2), 0.0, "Error 9");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 10");
    m.invert();
    assert_eq!(rate(&m, &AT, 1, 1), -0.5, "Error 1002");
}

#[test]
fn full_matrix_map() {
    let mut m = full_matrix();
    assert_eq!(rate(&m, &AT, 1, 1), 1.2, "Error 11");
    assert_eq!(rate(&m, &AT, 2, 2), 2.2, "Error 12");
    assert_eq!(rate(&m, &AT, 1, 2), 1.6, "Error 13");
    assert_eq!(rate(&m, &AT, 2, 1), -1.6, "Error 14");
    m.invert();
    assert_inverse_full_matrix_rates(&m, [15, 16, 17, 18]);
}

#[test]
fn shift_map() {
    let mut m = ShiftMap::new(vec![-1.2, 1.2]);
    assert_eq!(rate(&m, &AT, 1, 1), 1.0, "Error 20");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 21");
    m.invert();
    assert_eq!(rate(&m, &AT, 1, 1), 1.0, "Error 23");
}

#[test]
fn win_map_rates() {
    let mut m = win_map();
    assert_eq!(rate(&m, &AT, 1, 1), 2.0, "Error 24");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 25");
    assert_eq!(rate(&m, &AT, 2, 2), 2.0, "Error 26");
    assert_eq!(rate(&m, &AT, 1, 2), 0.0, "Error 27");
    m.invert();
    assert_eq!(rate(&m, &AT, 1, 1), 0.5, "Error 29");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 30");
    assert_eq!(rate(&m, &AT, 2, 2), 0.5, "Error 31");
    assert_eq!(rate(&m, &AT, 1, 2), 0.0, "Error 32");
}

/// An asymmetric PermMap: the forward direction swaps the axes and the
/// inverse direction does not.
#[test]
fn perm_map() {
    let mut m = PermMap::new(&[1, 2], &[2, 1], &[]).unwrap();
    assert_eq!(rate(&m, &AT, 1, 1), 0.0, "Error 34");
    assert_eq!(rate(&m, &AT, 2, 1), 1.0, "Error 35");
    assert_eq!(rate(&m, &AT, 2, 2), 0.0, "Error 37");
    assert_eq!(rate(&m, &AT, 1, 2), 1.0, "Error 38");
    m.invert();
    assert_eq!(rate(&m, &AT, 1, 1), 1.0, "Error 40");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 41");
    assert_eq!(rate(&m, &AT, 2, 2), 1.0, "Error 43");
    assert_eq!(rate(&m, &AT, 1, 2), 0.0, "Error 44");
}

/// A TranMap takes its forward rates from the WinMap and its inverse rates
/// from the inverse of the MatrixMap.
#[test]
fn tran_map() {
    let mut m = TranMap::new(Box::new(win_map()), Box::new(full_matrix())).unwrap();
    assert_eq!(rate(&m, &AT, 1, 1), 2.0, "Error 46");
    assert_eq!(rate(&m, &AT, 2, 1), 0.0, "Error 47");
    assert_eq!(rate(&m, &AT, 2, 2), 2.0, "Error 48");
    assert_eq!(rate(&m, &AT, 1, 2), 0.0, "Error 49");
    m.invert();
    assert_inverse_full_matrix_rates(&m, [51, 52, 53, 54]);
}

#[test]
fn cmp_map() {
    let at = [1.0, 2.0, 3.0, 4.0];
    let mut m: DynMapping = Box::new(compound_map());
    for (table, first_err) in [(COMPOUND_FORWARD, 55), (COMPOUND_INVERSE, 63)] {
        for (k, &(ax1, ax2, want, exact)) in table.iter().enumerate() {
            let r = rate(m.as_ref(), &at, ax1, ax2);
            let err = first_err + k;
            if exact {
                assert_eq!(r, want, "Error {err}: rate({ax1},{ax2})");
            } else {
                assert!(
                    (r - want).abs() <= 1.0e-6,
                    "Error {err}: rate({ax1},{ax2}) = {r}, want {want}"
                );
            }
        }
        m.invert();
    }
}

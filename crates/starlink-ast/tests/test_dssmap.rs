// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A DssMap, the class only old dumps can reach: positions on and off its
//! plate, its copies, the values its dump must hold, and its merging with a
//! WinMap.
//!
//! Ported from `reference/starlink-ast/ast_tester/testdssmap.c`.
//! Divergences from the C:
//!
//! - The dumps the C test rejects are replayed, with the C library's
//!   messages, by `test_hardening_oracle.rs`.
//! - A copy is a clone.

use std::f64::consts::{FRAC_PI_2, PI};

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::mappings::{CmpMap, Composition, DssMap, Mapping, WinMap};
use starlink_ast::{AnyObject, from_native_string};

/// The plate centre of the DssMap in the fixture, in radians, as the C test
/// gives it.
#[allow(clippy::excessive_precision)]
const PLATE_RA: f64 = 2.8272374655684112;
#[allow(clippy::excessive_precision)]
const PLATE_DEC: f64 = -1.0518122540502668;

fn fixture() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/serialisation/c_dssmap.ast"
    ))
    .unwrap()
}

fn dssmap(dump: &str) -> DssMap {
    match from_native_string(dump).unwrap() {
        AnyObject::DssMap(map) => map,
        other => panic!("read a {}", other.class_name()),
    }
}

/// Transforming (xin, yin) gives (xwant, ywant) to within `tol`, a bad value
/// exactly.
fn check_tran(map: &dyn Mapping, forward: bool, input: [f64; 2], want: [f64; 2], tol: f64) {
    let (x, y) = map.tran2(&[input[0]], &[input[1]], forward).unwrap();
    let got = [x[0], y[0]];
    if want[0] == BAD_VALUE {
        assert_eq!(got, [BAD_VALUE, BAD_VALUE], "{input:?}");
    } else {
        assert!(
            got.iter().all(|v| *v != BAD_VALUE)
                && (got[0] - want[0]).abs() <= tol
                && (got[1] - want[1]).abs() <= tol,
            "{input:?} gave {got:?}, expected {want:?}"
        );
    }
}

/// The C test's expected positions, to its 17 digits.
#[test]
#[allow(clippy::excessive_precision)]
fn positions_on_and_off_the_plate() {
    let dss = dssmap(&fixture());
    assert_eq!((dss.nin(), dss.nout()), (2, 2));
    let first = [2.8243236575347148, -1.050832220924965];
    let second = [2.8080654084049983, -1.0425371444459393];
    check_tran(&dss, true, [1.0, 1.0], first, 1e-12);
    check_tran(&dss, true, [1000.0, 1000.0], second, 1e-12);
    check_tran(&dss, false, first, [1.0, 1.0], 1e-8);
    check_tran(&dss, false, second, [1000.0, 1000.0], 1e-8);

    // A plate shows only the half of the sky on its centre's side.
    let bad = [BAD_VALUE, BAD_VALUE];
    check_tran(&dss, false, [PLATE_RA + PI, -PLATE_DEC], bad, 0.0);
    check_tran(&dss, false, [0.0, FRAC_PI_2], bad, 0.0);
    check_tran(&dss, false, [0.0, 0.0], bad, 0.0);
    // A pixel position so large the plate solution overflows.
    check_tran(&dss, true, [1e300, 1e300], bad, 0.0);

    // A copy is equal to the original.
    assert!(dss.clone().equal(&dss));
}

/// A pixel size of zero cannot be divided into a distance on the plate.
#[test]
fn a_pixel_size_of_zero_gives_bad_pixel_positions() {
    let dump = fixture();
    let line = dump
        .lines()
        .find(|l| l.trim_start().starts_with("XPixSz ="))
        .unwrap();
    let dss = dssmap(&dump.replace(line, " XPixSz = 0"));
    check_tran(
        &dss,
        false,
        [PLATE_RA, PLATE_DEC],
        [BAD_VALUE, BAD_VALUE],
        0.0,
    );
}

/// A WinMap doubling both pixel axes and shifting them by `shift`, then the
/// DssMap, simplified, gives a `want` (true for a DssMap, false for a
/// CmpMap) that transforms as the pair does.
fn check_merge(dss: &DssMap, shift: f64, want_dssmap: bool) {
    let win = WinMap::from_corners(
        &[0.0, 0.0],
        &[1.0, 1.0],
        &[shift, shift],
        &[shift + 2.0, shift + 2.0],
    )
    .unwrap();
    let cmp = CmpMap::new(Box::new(win), Box::new(dss.clone()), Composition::Series).unwrap();
    let simple = <dyn Mapping>::simplify(&cmp).unwrap();
    let is_dss = simple.as_any().is::<DssMap>();
    let is_cmp = simple.as_any().is::<CmpMap>();
    assert!(
        if want_dssmap { is_dss } else { is_cmp },
        "shift {shift}: simplified to another class"
    );
    // Far off the plate the two need not agree to the same accuracy.
    if shift.abs() < 1e6 {
        let xin = [1.0, 250.0, -300.0, 1000.0];
        let yin = [1.0, -125.0, 750.0, 1000.0];
        let (x1, y1) = cmp.tran2(&xin, &yin, true).unwrap();
        let (x2, y2) = simple.tran2(&xin, &yin, true).unwrap();
        for i in 0..4 {
            assert!(
                x1[i] != BAD_VALUE
                    && y1[i] != BAD_VALUE
                    && (x1[i] - x2[i]).abs() <= 1e-9
                    && (y1[i] - y2[i]).abs() <= 1e-9,
                "shift {shift}: they differ at ({}, {})",
                xin[i],
                yin[i]
            );
        }
    }
}

/// A DssMap absorbs a preceding WinMap only if the new pixel offsets are
/// whole numbers that the DSS FITS keywords' integers can hold.
#[test]
fn merging_with_a_winmap() {
    let dss = dssmap(&fixture());
    check_merge(&dss, 0.5, true);
    check_merge(&dss, 0.75, false);
    check_merge(&dss, 2.0e10 + 0.5, false);
}

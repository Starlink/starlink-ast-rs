// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The logic of moved Intervals, Prism overlaps and CmpRegion bounds.
//!
//! Ported from `reference/starlink-ast/ast_tester/testregionlogic.c`, one test
//! per C section.
//! Each overlap is checked in both orders, as in the C.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::frames::{BasicFrame, BasicFrameBuilder};
use starlink_ast::regions::{
    BoxRegionBuilder, CmpOperator, CmpRegionBuilder, IntervalBuilder, Region,
};

use super::test_regionhardening::cylinder;

fn plain(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

/// The overlap codes of `a` with `b` and of `b` with `a`.
fn overlaps(a: &dyn Region, b: &dyn Region) -> (i32, i32) {
    (a.overlap(b).unwrap() as i32, b.overlap(a).unwrap() as i32)
}

#[test]
fn moves() {
    // The uncertainty given to a Box is copied and centred on the Box.
    let unc = IntervalBuilder::new(plain(2), vec![-0.1, -0.1], vec![0.1, 0.1])
        .unwrap()
        .build()
        .unwrap();
    let bx = BoxRegionBuilder::from_bounds(plain(2), &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .uncertainty(Box::new(unc))
        .build()
        .unwrap();
    let moved = bx.get_unc(false).unwrap().expect("an uncertainty");
    let (lbnd, ubnd) = moved.get_region_bounds().unwrap();
    for i in 0..2 {
        assert!((lbnd[i] - 4.9).abs() <= 1.0e-6, "lower bound {lbnd:?}");
        assert!((ubnd[i] - 5.1).abs() <= 1.0e-6, "upper bound {ubnd:?}");
    }
}

#[test]
fn prisms() {
    let bx = BoxRegionBuilder::from_bounds(plain(3), &[-10.0, -10.0, 0.0], &[10.0, 10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        overlaps(&cylinder(), &bx),
        (2, 3),
        "a Prism and a Box that holds it"
    );
    assert_eq!(
        overlaps(&cylinder(), &cylinder()),
        (5, 5),
        "two equal Prisms"
    );

    // The axes picked from a negated Region are used un-negated, as the
    // negation is allowed for afterwards.
    let mut negbox = bx.clone();
    negbox.negate();
    assert_eq!(
        overlaps(&cylinder(), &negbox),
        (1, 1),
        "a Prism and the negation of a Box that holds it"
    );
    let mut negprism = cylinder();
    negprism.negate();
    assert_eq!(
        overlaps(&cylinder(), &negprism),
        (6, 6),
        "a Prism and its negation"
    );
}

/// The intersection of the half-planes x >= 0 and x >= 1 is the half-plane
/// x >= 1, which is not bounded.
#[test]
fn cmpregions() {
    let half_plane = |x: f64| {
        IntervalBuilder::new(plain(2), vec![x, BAD_VALUE], vec![BAD_VALUE; 2])
            .unwrap()
            .build()
            .unwrap()
    };
    let and = CmpRegionBuilder::new(half_plane(0.0), half_plane(1.0), CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    assert!(!and.is_bounded().unwrap());
    let (lbnd, ubnd) = and.get_region_bounds().unwrap();
    assert_eq!(lbnd, [1.0, -f64::MAX]);
    assert_eq!(ubnd, [f64::MAX, f64::MAX]);
}

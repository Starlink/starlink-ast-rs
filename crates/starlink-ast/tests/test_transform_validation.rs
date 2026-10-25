// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Every Mapping's transformation makes the checks C's base-class
//! `Transform` makes before any class transforms a point (mapping.c
//! `Transform`): an undefined direction is `AST__TRNND`, an input with the
//! wrong number of coordinates is `AST__NCPIN`, and an output with too few
//! points or coordinates is `AST__NOPTS` or `AST__NOCTS`.

use starlink_ast::coords::PointSet;
use starlink_ast::error::ErrorCode;
use starlink_ast::frames::{BasicFrameBuilder, FrameSet};
use starlink_ast::mappings::{
    CmpMap, Composition, Mapping, PermMap, RateMap, UnitMap, WinMap, ZoomMap,
};
use starlink_ast::regions::CircleBuilder;

fn code<T>(r: starlink_ast::error::AstResult<T>) -> ErrorCode {
    match r {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.code(),
    }
}

fn mappings() -> Vec<Box<dyn Mapping>> {
    vec![
        Box::new(UnitMap::new(2)),
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        Box::new(WinMap::new(&[1.0, -2.0], &[2.0, 3.0]).unwrap()),
        Box::new(PermMap::new(&[2, 1], &[2, 1], &[]).unwrap()),
        Box::new(
            CmpMap::new(
                Box::new(ZoomMap::new(2, 2.0).unwrap()),
                Box::new(UnitMap::new(2)),
                Composition::Series,
            )
            .unwrap(),
        ),
    ]
}

#[test]
fn a_wrong_number_of_input_coordinates_is_ncpin() {
    for map in mappings() {
        let pts = PointSet::from_rows([[1.0]]);
        assert_eq!(code(map.transform(&pts, true)), ErrorCode::Ncpin, "{map:?}");
        let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
        assert_eq!(
            code(map.transform(&pts, false)),
            ErrorCode::Ncpin,
            "{map:?}"
        );
    }
}

#[test]
fn too_small_an_output_is_nopts_or_nocts() {
    for map in mappings() {
        let pts = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0]]);
        let mut few_points = PointSet::zeros(1, 2);
        assert_eq!(
            code(map.transform_into(pts.as_points(), true, few_points.as_points_mut())),
            ErrorCode::Nopts,
            "{map:?}"
        );
        let mut few_coords = PointSet::zeros(2, 1);
        assert_eq!(
            code(map.transform_into(pts.as_points(), true, few_coords.as_points_mut())),
            ErrorCode::Nocts,
            "{map:?}"
        );
    }
}

#[test]
fn an_undefined_direction_is_trnnd() {
    let rate = RateMap::new(Box::new(ZoomMap::new(1, 3.0).unwrap()), 1, 1).unwrap();
    let rate: &dyn Mapping = &rate;
    let pts = PointSet::from_rows([[1.0]]);
    assert_eq!(code(rate.transform(&pts, false)), ErrorCode::Trnnd);
    assert_eq!(
        code(rate.transform_nan_protocol(&pts, false)),
        ErrorCode::Trnnd
    );
}

#[test]
fn a_region_and_a_frameset_check_too() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let circle = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    let pts = PointSet::from_rows([[1.0]]);
    assert_eq!(code(circle.transform(&pts, true)), ErrorCode::Ncpin);

    let frameset = FrameSet::new(Box::new(BasicFrameBuilder::new(2).build().unwrap()));
    assert_eq!(code(frameset.transform(&pts, true)), ErrorCode::Ncpin);
}

/// `astRate` checks both axis indices before any class finds a rate
/// (mapping.c `astRate_`), and the base class's numeric rate needs the
/// forward transformation (mapping.c `Rate`).
#[test]
fn rate_checks_its_axes() {
    for map in mappings() {
        let at = [1.0, 2.0];
        for (ax1, ax2) in [(0, 1), (3, 1), (1, 0), (1, 3)] {
            assert_eq!(
                code(map.rate(&at, ax1, ax2)),
                ErrorCode::Axiin,
                "{map:?} {ax1} {ax2}"
            );
        }
        assert!(map.rate(&at, 1, 1).is_ok(), "{map:?}");
        assert_eq!(code(map.rate(&[1.0], 1, 1)), ErrorCode::Ncpin, "{map:?}");
    }

    let mut rate = RateMap::new(Box::new(ZoomMap::new(1, 3.0).unwrap()), 1, 1).unwrap();
    rate.invert();
    assert_eq!(code(rate.rate(&[1.0], 1, 1)), ErrorCode::Nodef);
}

/// `astMapBox` takes a 1-based output index and refuses one outside the
/// outputs (AST__BADCI).
#[test]
fn map_box_takes_a_one_based_output() {
    let map = WinMap::new(&[1.0, -2.0], &[2.0, 3.0]).unwrap();
    let (lbnd, ubnd) = ([0.0, 0.0], [1.0, 1.0]);
    let first = map.map_box(&lbnd, &ubnd, true, 1).unwrap();
    assert!((first.lbnd_out - 1.0).abs() < 1e-9 && (first.ubnd_out - 3.0).abs() < 1e-9);
    let second = map.map_box(&lbnd, &ubnd, true, 2).unwrap();
    assert!((second.lbnd_out + 2.0).abs() < 1e-9 && (second.ubnd_out - 1.0).abs() < 1e-9);
    for bad in [0, 3] {
        assert_eq!(code(map.map_box(&lbnd, &ubnd, true, bad)), ErrorCode::Badci);
    }
}

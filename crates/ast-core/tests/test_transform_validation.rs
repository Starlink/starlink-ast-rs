//! Every Mapping's transformation makes the checks C's base-class
//! `Transform` makes before any class transforms a point (mapping.c
//! `Transform`): an undefined direction is `AST__TRNND`, an input with the
//! wrong number of coordinates is `AST__NCPIN`, and an output with too few
//! points or coordinates is `AST__NOPTS` or `AST__NOCTS`.

use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::frames::{BasicFrameBuilder, FrameSet};
use ast_core::mappings::{
    CmpMap, Composition, Mapping, PermMap, RateMap, UnitMap, WinMap, ZoomMap,
};
use ast_core::regions::CircleBuilder;

fn code<T>(r: ast_core::error::AstResult<T>) -> ErrorCode {
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
            code(map.transform_into(&pts, true, &mut few_points)),
            ErrorCode::Nopts,
            "{map:?}"
        );
        let mut few_coords = PointSet::zeros(2, 1);
        assert_eq!(
            code(map.transform_into(&pts, true, &mut few_coords)),
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

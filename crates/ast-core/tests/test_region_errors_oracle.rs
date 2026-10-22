//! The errors C's Region constructors and setters report, and the values its
//! loader repairs: `astSetUnc` given a Region that cannot be re-centerd or
//! whose Frame cannot be converted (region.c `SetUnc`), `astSetFillFactor`
//! outside 0 to 1, a dump with an out-of-range `Fill` or a `MeshSz` below 5
//! (region.c `Load`), and BAD coordinates given to `astBox` (box.c
//! `astInitBox`) and `astCircle` (circle.c `astInitCircle`); `astPolygon`
//! given a 3-D Frame, no vertices, one or two, or a BAD vertex (polygon.c
//! `astInitPolygon`); and `astEllipse` given a 3-D Frame, BAD values, a
//! negative or zero axis, or points that do not determine an ellipse, which
//! C rejects only when the Ellipse is used (ellipse.c `Cache`); and a Box
//! dump whose points have three values for its 2-D Frame (region.c `Load`).
//! Also `astSetUnc` given an uncertainty whose current Frame is in another
//! Domain but whose base Frame is in the Region's, which region.c `Conv`
//! finds by converting FrameSet to FrameSet.
//!
//! Each expected code is the status C is left holding.
//!
//! The values come from `tools/oracle_fixtures/region_errors_probe.c`.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_errors_oracle/cases.rs"]
mod cases;

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::AstResult;
use ast_core::frames::{BasicFrame, BasicFrameBuilder};
use ast_core::from_native_string;
use ast_core::regions::{
    BoxRegionBuilder, Circle, CircleBuilder, EllipseBuilder, PolygonBuilder, Region,
};

fn frame(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

fn circle() -> Circle {
    CircleBuilder::new(frame(2), vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap()
}

fn code<T>(result: AstResult<T>) -> i32 {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.code().as_raw() as i32,
    }
}

#[test]
fn set_uncertainty_rejects_a_shape_that_cannot_be_recentred() {
    let poly = PolygonBuilder::new(
        frame(2),
        vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(
        code(circle().set_uncertainty(Box::new(poly))),
        cases::SETUNC_SHAPE
    );
}

#[test]
fn set_uncertainty_rejects_an_unconvertible_frame() {
    let mut f3 = frame(3);
    f3.set_domain("OTHER");
    let box3 = BoxRegionBuilder::from_bounds(f3, &[0.0; 3], &[1.0; 3])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        code(circle().set_uncertainty(Box::new(box3))),
        cases::SETUNC_FRAME
    );
}

#[test]
fn failed_set_uncertainty_drops_the_old_one() {
    let mut c = circle();
    let bx = BoxRegionBuilder::from_bounds(frame(2), &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    c.set_uncertainty(Box::new(bx)).unwrap();
    let poly = PolygonBuilder::new(
        frame(2),
        vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .build()
    .unwrap();
    assert!(c.set_uncertainty(Box::new(poly)).is_err());
    assert_eq!(c.test_unc(), cases::SETUNC_KEEPS_OLD);
}

#[test]
fn fill_factor_outside_zero_to_one_is_an_error() {
    let mut c = circle();
    c.set_fill_factor(0.5).unwrap();
    assert_eq!(code(c.set_fill_factor(1.5)), cases::FILL_HIGH);
    assert_eq!(c.fill_factor(), cases::FILL_AFTER);
    assert_eq!(code(c.set_fill_factor(-0.1)), cases::FILL_LOW);
}

#[test]
fn loader_rejects_an_out_of_range_fill() {
    assert_eq!(
        code(from_native_string(cases::LOAD_FILL_DUMP)),
        cases::LOAD_FILL
    );
}

#[test]
fn loader_raises_mesh_size_to_five() {
    let region = from_native_string(cases::LOAD_MESH_DUMP)
        .unwrap()
        .into_region()
        .unwrap();
    assert_eq!(region.mesh_size() as i32, cases::LOAD_MESH_SIZE);
}

#[test]
fn box_rejects_bad_coordinates() {
    let new = |c: Vec<f64>, p: Vec<f64>| BoxRegionBuilder::new(frame(2), c, p);
    let bounds = |l: &[f64], u: &[f64]| BoxRegionBuilder::from_bounds(frame(2), l, u);
    assert_eq!(
        code(new(vec![BAD_VALUE, 0.0], vec![1.0, 1.0])),
        cases::BOX_CENTRE
    );
    assert_eq!(
        code(new(vec![0.0, 0.0], vec![1.0, BAD_VALUE])),
        cases::BOX_CORNER
    );
    assert_eq!(
        code(bounds(&[BAD_VALUE, 0.0], &[1.0, 1.0])),
        cases::BOX_LOWER
    );
    assert_eq!(
        code(bounds(&[0.0, 0.0], &[1.0, BAD_VALUE])),
        cases::BOX_UPPER
    );
}

#[test]
fn circle_rejects_bad_coordinates() {
    let by_radius =
        |c: Vec<f64>, r: f64| CircleBuilder::new(frame(2), c).unwrap().radius(r).build();
    assert_eq!(
        code(by_radius(vec![BAD_VALUE, 0.0], 1.0)),
        cases::CIRCLE_CENTRE
    );
    assert_eq!(
        code(by_radius(vec![0.0, 0.0], BAD_VALUE)),
        cases::CIRCLE_RADIUS
    );
    let by_point = CircleBuilder::new(frame(2), vec![0.0, 0.0])
        .unwrap()
        .edge_point(vec![1.0, BAD_VALUE])
        .build();
    assert_eq!(code(by_point), cases::CIRCLE_CIRCUM);
}

fn polygon(naxes: usize, vertices: Vec<Vec<f64>>) -> AstResult<ast_core::regions::Polygon> {
    PolygonBuilder::new(frame(naxes), vertices)?.build()
}

#[test]
fn polygon_constructor_errors_match_c() {
    let tri = || vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 1.0]];
    assert_eq!(code(polygon(3, tri())), cases::POLY_NAXES);
    assert_eq!(code(polygon(2, vec![])), cases::POLY_EMPTY);
    let err = polygon(
        2,
        vec![vec![0.0, 0.0], vec![1.0, BAD_VALUE], vec![BAD_VALUE, 1.0]],
    )
    .unwrap_err();
    assert_eq!(err.code().as_raw() as i32, cases::POLY_BAD);
    assert_eq!(
        err.message(),
        "astInitPolygon(Polygon): One or more bad axis values supplied for the vertex number 3."
    );
}

#[test]
fn polygon_of_one_or_two_vertices_is_accepted() {
    assert_eq!(cases::POLY_FEW, 0);
    let one = polygon(2, vec![vec![0.0, 0.0]]).unwrap();
    assert_eq!(one.is_bounded().unwrap(), cases::POLY_ONE_BOUNDED);
    let two = polygon(2, vec![vec![0.0, 0.0], vec![1.0, 0.0]]).unwrap();
    assert_eq!(two.is_bounded().unwrap(), cases::POLY_TWO_BOUNDED);
}

fn ellipse(
    naxes: usize,
    centre: Vec<f64>,
    axes: [f64; 2],
    angle: f64,
) -> AstResult<ast_core::regions::Ellipse> {
    EllipseBuilder::new(frame(naxes), centre, axes, angle)?.build()
}

fn transform(region: &dyn Region) -> AstResult<Vec<f64>> {
    let out = region.transform(&PointSet::from_rows([[0.5, 0.5]]), true)?;
    Ok(out.point(0).to_vec())
}

#[test]
fn ellipse_constructor_errors_match_c() {
    assert_eq!(
        code(ellipse(3, vec![0.0, 0.0], [2.0, 1.0], 0.3)),
        cases::ELL_NAXES
    );
    assert_eq!(
        code(ellipse(2, vec![BAD_VALUE, 0.0], [2.0, 1.0], 0.3)),
        cases::ELL_CENTRE
    );
    assert_eq!(
        code(ellipse(2, vec![0.0, 0.0], [2.0, BAD_VALUE], 0.3)),
        cases::ELL_AXIS
    );
    assert_eq!(
        code(ellipse(2, vec![0.0, 0.0], [2.0, 1.0], BAD_VALUE)),
        cases::ELL_ANGLE
    );
}

#[test]
fn ellipse_with_a_negative_axis_is_an_ellipse() {
    assert_eq!(cases::ELL_NEGATIVE, 0);
    assert_eq!(cases::ELL_NEGATIVE_USE, 0);
    let e = ellipse(2, vec![0.0, 0.0], [-2.0, 1.0], 0.3).unwrap();
    assert_eq!(transform(&e).unwrap(), cases::ELL_NEGATIVE_OUT);
}

#[test]
fn ellipse_that_is_no_ellipse_fails_when_used() {
    assert_eq!(cases::ELL_ZERO, 0);
    let e = ellipse(2, vec![0.0, 0.0], [0.0, 1.0], 0.3).unwrap();
    assert_eq!(code(transform(&e)), cases::ELL_ZERO_USE);

    assert_eq!(cases::ELL_DEGENERATE_LOAD, 0);
    let loaded = from_native_string(cases::ELL_DEGENERATE_DUMP)
        .unwrap()
        .into_region()
        .unwrap();
    let err = transform(loaded.as_ref()).unwrap_err();
    assert_eq!(err.code().as_raw() as i32, cases::ELL_DEGENERATE_USE);
    assert_eq!(
        err.message(),
        "astInitEllipse(Ellipse): The supplied points do not determine an ellipse."
    );
}

#[test]
fn loader_rejects_points_of_the_wrong_length() {
    let err = from_native_string(cases::LOAD_NCOORD_DUMP).unwrap_err();
    assert_eq!(err.code().as_raw() as i32, cases::LOAD_NCOORD);
    assert_eq!(
        err.message(),
        "astLoadRegion(Box): Corrupt Box contains  incorrect number of coordinate values per point (3)."
    );
}

#[test]
fn set_uncertainty_matches_any_frame_of_the_uncertainty() {
    use ast_core::AnyObject;
    use ast_core::mappings::ZoomMap;

    assert_eq!(cases::SETUNC_OTHER_CURRENT, 0);
    let with_domain = |d: &str| {
        let mut f = frame(2);
        f.set_domain(d);
        f
    };
    let circle_in = |d: &str, r: f64| {
        CircleBuilder::new(with_domain(d), vec![1.0, 0.5])
            .unwrap()
            .radius(r)
            .build()
            .unwrap()
    };
    let mut target = circle_in("FOO", 0.1);
    let unc = circle_in("FOO", 0.01)
        .reg_map_region(&ZoomMap::new(2, 2.0).unwrap(), &with_domain("BAR"))
        .unwrap();
    target.set_uncertainty(unc).unwrap();
    let text = native_dump(&AnyObject::from_mapping(&target).unwrap()).unwrap();
    assert_eq!(text, cases::SETUNC_OTHER_CURRENT_DUMP);
}

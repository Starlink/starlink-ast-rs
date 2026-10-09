//! The errors C's Region constructors and setters report, and the values its
//! loader repairs: `astSetUnc` given a Region that cannot be re-centred or
//! whose Frame cannot be converted (region.c `SetUnc`), `astSetFillFactor`
//! outside 0 to 1, a dump with an out-of-range `Fill` or a `MeshSz` below 5
//! (region.c `Load`), and BAD coordinates given to `astBox` (box.c
//! `astInitBox`) and `astCircle` (circle.c `astInitCircle`).
//!
//! Each expected code is the status C is left holding.
//!
//! The values come from `tools/oracle_fixtures/region_errors_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_errors_oracle/cases.rs"]
mod cases;

use ast_core::constants::BAD_VALUE;
use ast_core::error::AstResult;
use ast_core::frames::{BasicFrame, BasicFrameBuilder};
use ast_core::from_native_string;
use ast_core::regions::{BoxRegionBuilder, Circle, CircleBuilder, PolygonBuilder, Region};

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

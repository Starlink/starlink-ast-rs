//! `Mapping::tran1` / `Mapping::tran2` — the scalar and 2-D convenience
//! entry points.
//!
//! C ancestors: `astTran1` (`reference/starlink-ast/src/mapping.c:89`) and
//! `astTran2`, together with the arms of `ValidateMapping` they invoke.
//!
//! Design commitment: `docs/superpowers/specs/2026-04-20-rust-port-design.md`,
//! "Coordinate data" — *"Single-axis / two-axis convenience `Mapping::tran1` /
//! `tran2` stay (avoid forcing users to allocate a `PointSet` for 1-D data)."*
//!
//! Idiomatic divergences from C: array lengths come from the slices rather
//! than a separate `npoint` argument, outputs are returned rather than written
//! into caller buffers, and errors are `AstResult` rather than global status.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::mappings::{Mapping, PermMap, RateMap, ShiftMap, UnitMap, ZoomMap};

// ===========================================================================
// tran1
// ===========================================================================

#[test]
fn tran1_applies_the_forward_transform() {
    let zoom = ZoomMap::new(1, 2.5).expect("1-D ZoomMap");
    let out = zoom.tran1(&[1.0, 2.0, -4.0], true).expect("tran1 succeeds");
    assert_eq!(out, vec![2.5, 5.0, -10.0]);
}

#[test]
fn tran1_applies_the_inverse_transform() {
    let zoom = ZoomMap::new(1, 2.5).expect("1-D ZoomMap");
    let out = zoom.tran1(&[2.5, 5.0], false).expect("tran1 succeeds");
    assert_eq!(out, vec![1.0, 2.0]);
}

#[test]
fn tran1_agrees_with_the_pointset_path() {
    // The convenience method must be a pure shorthand: same numbers as
    // building the one-column PointSet by hand.
    let shift = ShiftMap::new([3.25]);
    let xin = [0.0, 1.5, -2.75, 1e9];

    let via_tran1 = shift.tran1(&xin, true).expect("tran1 succeeds");

    let pts = PointSet::from_columns(&[&xin]).expect("one-column PointSet");
    let via_pointset: Vec<f64> = shift
        .transform(&pts, true)
        .expect("transform succeeds")
        .axis_iter(0)
        .collect();

    assert_eq!(via_tran1, via_pointset);
}

#[test]
fn tran1_propagates_bad_values() {
    let zoom = ZoomMap::new(1, 3.0).expect("1-D ZoomMap");
    let out = zoom
        .tran1(&[1.0, BAD_VALUE, 2.0], true)
        .expect("tran1 succeeds");
    assert_eq!(out[0], 3.0);
    assert_eq!(out[1], BAD_VALUE, "BAD in must stay BAD out");
    assert_eq!(out[2], 6.0);
}

#[test]
fn tran1_rejects_an_empty_input() {
    // C's astTran1 reports AST__NPTIN for npoint < 1 when astInitPointSet
    // builds its PointSet (pointset.c:3011), so an empty slice is an error
    // rather than a no-op. Keeping C's rule means
    // the Rust API and the eventual C-ABI shim agree; a caller that wants an
    // empty input to be a no-op guards with `if !xin.is_empty()`.
    let zoom = ZoomMap::new(1, 2.0).expect("1-D ZoomMap");
    let err = zoom
        .tran1(&[], true)
        .expect_err("zero points is AST__NPTIN, as in C");
    assert_eq!(err.code(), ErrorCode::Nptin);
}

#[test]
fn tran1_rejects_a_mapping_of_the_wrong_arity() {
    // C: ValidateMapping(this, forward, npoint, 1, 1, "astTran1") reports
    // AST__NCPIN when Nin or Nout is not 1.
    let zoom2d = ZoomMap::new(2, 2.0).expect("2-D ZoomMap");
    let err = zoom2d
        .tran1(&[1.0], true)
        .expect_err("a 2-D Mapping cannot serve tran1");
    assert_eq!(err.code(), ErrorCode::Ncpin);
    assert!(
        err.message().contains("Nin = Nout = 1"),
        "the message should state the required arity, got {:?}",
        err.message()
    );
}

// ===========================================================================
// tran2
// ===========================================================================

#[test]
fn tran2_applies_both_axes() {
    let shift = ShiftMap::new([1.0, -2.0]);
    let (x, y) = shift
        .tran2(&[0.0, 10.0], &[0.0, 10.0], true)
        .expect("tran2 succeeds");
    assert_eq!(x, vec![1.0, 11.0]);
    assert_eq!(y, vec![-2.0, 8.0]);
}

#[test]
fn tran2_agrees_with_the_pointset_path() {
    let shift = ShiftMap::new([0.5, 7.0]);
    let xin = [1.0, 2.0, 3.0];
    let yin = [-1.0, 0.0, 1.0];

    let (x, y) = shift.tran2(&xin, &yin, true).expect("tran2 succeeds");

    let pts = PointSet::from_columns(&[&xin, &yin]).expect("two-column PointSet");
    let out = shift.transform(&pts, true).expect("transform succeeds");
    let ex: Vec<f64> = out.axis_iter(0).collect();
    let ey: Vec<f64> = out.axis_iter(1).collect();

    assert_eq!(x, ex);
    assert_eq!(y, ey);
}

#[test]
fn tran2_respects_axis_order() {
    // A swap PermMap proves the two output vectors are not transposed: the
    // x output must come from the y input.
    let swap = PermMap::new(&[2, 1], &[2, 1], &[]).expect("swap PermMap");
    let (x, y) = swap
        .tran2(&[1.0, 2.0], &[30.0, 40.0], true)
        .expect("tran2 succeeds");
    assert_eq!(x, vec![30.0, 40.0]);
    assert_eq!(y, vec![1.0, 2.0]);
}

#[test]
fn tran2_rejects_mismatched_input_lengths() {
    // Rust-only condition: C takes one npoint for both arrays and so cannot
    // express a mismatch.
    let unit = UnitMap::new(2);
    let err = unit
        .tran2(&[1.0, 2.0], &[1.0], true)
        .expect_err("mismatched lengths must be rejected");
    assert_eq!(err.code(), ErrorCode::Nptin);
    assert!(
        err.message().contains("2 points but yin has 1"),
        "the message should name both lengths, got {:?}",
        err.message()
    );
}

#[test]
fn tran2_rejects_a_mapping_of_the_wrong_arity() {
    let unit1d = UnitMap::new(1);
    let err = unit1d
        .tran2(&[1.0], &[1.0], true)
        .expect_err("a 1-D Mapping cannot serve tran2");
    assert_eq!(err.code(), ErrorCode::Ncpin);
    assert!(err.message().contains("Nin = Nout = 2"));
}

// ===========================================================================
// Definedness
// ===========================================================================

#[test]
fn tran_rejects_an_undefined_direction() {
    // C's ValidateMapping reports AST__TRNND when the requested direction is
    // not defined. RateMap is the natural case: it computes a rate of change,
    // so in its un-inverted state the inverse does not exist (ratemap.c passes
    // (1, 0) for the transform-definedness flags).
    let rate = RateMap::new(Box::new(ZoomMap::new(1, 2.0).expect("1-D ZoomMap")), 1, 1)
        .expect("RateMap over a 1-D ZoomMap");
    assert!(rate.has_forward());
    assert!(!rate.has_inverse(), "a RateMap has no inverse transform");

    let err = rate
        .tran1(&[1.0], false)
        .expect_err("the inverse is not defined");
    assert_eq!(err.code(), ErrorCode::Trnnd);
    assert!(
        err.message().contains("inverse"),
        "the message should name the direction, got {:?}",
        err.message()
    );

    // The forward direction of the same Mapping still works, so the guard is
    // direction-specific rather than a blanket rejection.
    assert!(rate.tran1(&[1.0], true).is_ok());
}

#[test]
fn tran1_and_tran2_are_available_through_a_trait_object() {
    // The methods have default bodies on the trait, so they must be callable
    // on `&dyn Mapping` and not only on concrete types.
    let boxed: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 4.0).expect("1-D ZoomMap"));
    assert_eq!(boxed.tran1(&[2.0], true).expect("tran1"), vec![8.0]);

    let boxed2: Box<dyn Mapping> = Box::new(ShiftMap::new([1.0, 1.0]));
    let (x, y) = boxed2.tran2(&[0.0], &[0.0], true).expect("tran2");
    assert_eq!((x, y), (vec![1.0], vec![1.0]));
}

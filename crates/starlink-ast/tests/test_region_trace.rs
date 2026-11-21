// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Region boundary tracing and splitting against the C library (region.c
//! `RegTrace` and `RegSplit`; the `RegTrace` of box.c, circle.c, ellipse.c,
//! interval.c, polygon.c and moc.c; cmpregion.c `RegTrace`, `SetBreakInfo`
//! and `RegSplit`), the methods a Plot uses to draw a Region's outline.
//!
//! Each Region is built here and in `tools/oracle_fixtures/region_trace_probe.c`
//! from the same id; the values come from that probe.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_trace_oracle/cases.rs"]
mod cases;

use crate::common::{assert_c_text, c_value_eq, native_dump};
use starlink_ast::AnyObject;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::frames::{BasicFrame, BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use starlink_ast::mappings::Mapping;
use starlink_ast::regions::{
    BoxRegionBuilder, CircleBuilder, CmpOperator, CmpRegionBuilder, DynRegion, EllipseBuilder,
    IntervalBuilder, Moc, PointListBuilder, PolygonBuilder, Region,
};

fn frame(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

fn sky() -> SkyFrame {
    SkyFrame::new(SkySystem::Icrs)
}

fn galactic(f: &mut dyn Frame) -> starlink_ast::error::AstResult<()> {
    f.as_any_mut()
        .downcast_mut::<SkyFrame>()
        .expect("a sky Region's Frame is a SkyFrame")
        .set_system(SkySystem::Galactic);
    Ok(())
}

/// The Region the probe builds for `id`, with System changed to Galactic.
macro_rules! gal {
    ($id:expr, $ty:ty) => {{
        let mut reg = make_region($id);
        reg.as_any_mut()
            .downcast_mut::<$ty>()
            .unwrap()
            .with_integrity(galactic)
            .unwrap();
        reg
    }};
}

fn negated(id: &str) -> DynRegion {
    let mut reg = make_region(id);
    reg.negate();
    reg
}

fn moc(text: Option<&str>) -> DynRegion {
    let mut m = Moc::new();
    if let Some(text) = text {
        m.add_moc_string(CmpOperator::Or, false, -1, text).unwrap();
    }
    Box::new(m)
}

fn cmp(a: &str, b: &str, oper: CmpOperator) -> DynRegion {
    Box::new(
        CmpRegionBuilder::from_dyn(make_region(a), make_region(b), oper)
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn bx(f: impl Frame + 'static, p1: &[f64], p2: &[f64]) -> DynRegion {
    Box::new(
        BoxRegionBuilder::from_bounds(f, p1, p2)
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn circle(f: impl Frame + 'static, c: &[f64], r: f64) -> DynRegion {
    Box::new(
        CircleBuilder::new(f, c.to_vec())
            .unwrap()
            .radius(r)
            .build()
            .unwrap(),
    )
}

fn ellipse(f: impl Frame + 'static, c: &[f64], ab: [f64; 2], angle: f64) -> DynRegion {
    Box::new(
        EllipseBuilder::new(f, c.to_vec(), ab, angle)
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn interval(f: impl Frame + 'static, lo: &[f64], hi: &[f64]) -> DynRegion {
    Box::new(
        IntervalBuilder::new(f, lo.to_vec(), hi.to_vec())
            .unwrap()
            .build()
            .unwrap(),
    )
}

/// A Polygon from C's layout: all the first-axis values, then the second's.
fn polygon(f: impl Frame + 'static, xs: &[f64], ys: &[f64]) -> DynRegion {
    let vertices = xs.iter().zip(ys).map(|(&x, &y)| vec![x, y]).collect();
    Box::new(PolygonBuilder::new(f, vertices).unwrap().build().unwrap())
}

fn make_region(id: &str) -> DynRegion {
    use starlink_ast::regions::{BoxRegion, Circle, CmpRegion, Ellipse, Interval, Polygon};
    match id {
        "box2d" => bx(frame(2), &[0.0, 0.0], &[2.0, 1.0]),
        "box_sky" => bx(sky(), &[0.1, 0.2], &[0.4, 0.5]),
        "box_sky_gal" => gal!("box_sky", BoxRegion),
        "box3d" => bx(frame(3), &[0.0, 0.0, 0.0], &[1.0, 1.0, 1.0]),
        "box_neg" => negated("box2d"),
        "circle2d" => circle(frame(2), &[1.0, 2.0], 0.5),
        "circle_sky" => circle(sky(), &[1.0, 0.5], 0.1),
        "circle_sky_gal" => gal!("circle_sky", Circle),
        "circle1d" => circle(frame(1), &[1.0], 0.5),
        "circle_neg" => negated("circle2d"),
        "ellipse2d" => ellipse(frame(2), &[0.0, 0.0], [2.0, 1.0], 0.5),
        "ellipse_sky" => ellipse(sky(), &[1.0, 0.3], [0.1, 0.05], 0.3),
        "ellipse_sky_gal" => gal!("ellipse_sky", Ellipse),
        "interval2d" => interval(frame(2), &[0.0, 0.0], &[1.0, 2.0]),
        "interval_open" => interval(frame(2), &[0.0, BAD_VALUE], &[1.0, 2.0]),
        "interval_excl" => interval(frame(2), &[1.0, 2.0], &[0.0, 0.0]),
        "interval1d" => interval(frame(1), &[0.0], &[1.0]),
        "interval_sky" => interval(sky(), &[0.1, 0.2], &[0.4, 0.5]),
        "interval_sky_gal" => gal!("interval_sky", Interval),
        "polygon_quad" => polygon(frame(2), &[0.0, 2.0, 2.5, 0.0], &[0.0, 0.0, 1.0, 1.5]),
        "polygon_hept" => polygon(
            frame(2),
            &[0.0, 1.0, 2.0, 2.5, 2.0, 1.0, 0.2],
            &[0.0, -0.5, 0.0, 1.0, 2.0, 1.8, 1.0],
        ),
        "polygon_sky" => polygon(sky(), &[1.0, 1.1, 1.1, 1.0], &[0.5, 0.5, 0.6, 0.62]),
        "polygon_sky_gal" => gal!("polygon_sky", Polygon),
        "polygon_neg" => negated("polygon_quad"),
        "moc_cell" => moc(Some("3/25")),
        "moc_block" => moc(Some("4/96-111")),
        "moc_two" => moc(Some("3/25 3/600")),
        "moc_mixed" => moc(Some("2/6 3/28-29")),
        "moc_diag" => moc(Some("4/0 4/3")),
        "moc_empty" => moc(None),
        "moc_gal" => gal!("moc_block", Moc),
        "pointlist" => Box::new(
            PointListBuilder::new(frame(2), vec![vec![0.0, 0.0], vec![1.0, 1.0]])
                .unwrap()
                .build()
                .unwrap(),
        ),

        // Components of the CmpRegions.
        "cA" => circle(frame(2), &[1.0, 1.0], 1.0),
        "bA" => bx(frame(2), &[0.5, 0.5], &[2.5, 1.5]),
        "cB" => circle(frame(2), &[5.0, 5.0], 1.0),
        "cC" => circle(frame(2), &[1.8, 1.2], 0.6),
        "neg_bA" => negated("bA"),
        "sky_c1" => circle(sky(), &[1.0, 0.5], 0.1),
        "sky_c2" => circle(sky(), &[1.5, 0.5], 0.1),
        "sky_b1" => bx(sky(), &[1.05, 0.45], &[1.2, 0.55]),

        // CmpRegions.
        "and_cb" => cmp("cA", "bA", CmpOperator::And),
        "or_cb" => cmp("cA", "bA", CmpOperator::Or),
        "xor_cb" => cmp("cA", "bA", CmpOperator::Xor),
        "or_disjoint" => cmp("cA", "cB", CmpOperator::Or),
        "and_negbox" => cmp("cA", "neg_bA", CmpOperator::And),
        "or_negbox" => cmp("cA", "neg_bA", CmpOperator::Or),
        "nested" => cmp("or_cb", "cC", CmpOperator::And),
        "or_three" => cmp("or_disjoint", "cC", CmpOperator::Or),
        "or_sky" => cmp("sky_c1", "sky_b1", CmpOperator::Or),
        "or_sky_gal" => gal!("or_sky", CmpRegion),
        "or_sky_disjoint" => cmp("sky_c1", "sky_c2", CmpOperator::Or),
        "or_sky_disjoint_gal" => gal!("or_sky_disjoint", CmpRegion),
        "and_sky" => cmp("sky_c1", "sky_b1", CmpOperator::And),
        "and_sky_gal" => gal!("and_sky", CmpRegion),
        "or_unbounded" => cmp("cA", "interval_open", CmpOperator::Or),
        "or_pointlist2" => cmp("cA", "pointlist", CmpOperator::Or),
        "or_pointlist1" => cmp("pointlist", "cA", CmpOperator::Or),
        other => panic!("no region {other}"),
    }
}

/// Compare the traced positions of `reg` at `dist` with C's, one array per
/// current-Frame axis.
fn check_positions(id: &str, reg: &dyn Region, dist: &[f64], want: &[&[f64]]) {
    let got = reg
        .reg_trace(dist)
        .unwrap_or_else(|e| panic!("{id}: {e}"))
        .unwrap_or_else(|| panic!("{id}: traced with no points but not with some"));
    assert_eq!(got.ncoord(), want.len(), "{id}: axes");
    assert_eq!(got.npoint(), dist.len(), "{id}: points");
    for (j, axis) in want.iter().enumerate() {
        for (i, &w) in axis.iter().enumerate() {
            let g = got.axis(j)[i];
            assert!(
                c_value_eq(g, w),
                "{id}: axis {j} at dist {}: got {g:e}, C {w:e}",
                dist[i]
            );
        }
    }
}

fn check_trace_cases(cases: &[cases::TraceCase]) {
    for &(id, supported, up, mixed) in cases {
        let reg = make_region(id);
        let support = reg.reg_trace(&[]).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(support.is_some(), supported, "{id}: support");
        if supported {
            check_positions(id, reg.as_ref(), cases::DIST_UP, up);
            check_positions(id, reg.as_ref(), cases::DIST_MIXED, mixed);
        } else {
            assert!(
                reg.reg_trace(cases::DIST_UP).unwrap().is_none(),
                "{id}: positions without support"
            );
        }
    }
}

#[test]
fn reg_trace_matches_the_c_library() {
    check_trace_cases(cases::TRACE_CASES);
}

#[test]
fn an_empty_dist_only_answers_support() {
    let reg = make_region("circle2d");
    let traced = reg.reg_trace(&[]).unwrap().expect("a 2-D Circle traces");
    assert_eq!(traced.npoint(), 0);
}

#[test]
fn cmpregion_reg_trace_matches_the_c_library() {
    check_trace_cases(cases::CMP_TRACE_CASES);
}

/// C checks its first component twice where it means both, so a CmpRegion
/// whose second component cannot be traced claims to be traceable and then
/// reads positions from arrays nothing filled. The port keeps the claim and
/// gives bad positions.
#[test]
fn an_untraceable_second_component_gives_bad_positions() {
    for &(id, supported) in cases::CMP_UNDEFINED_CASES {
        let reg = make_region(id);
        assert_eq!(reg.reg_trace(&[]).unwrap().is_some(), supported, "{id}");
        let got = reg.reg_trace(cases::DIST_UP).unwrap().unwrap();
        assert_eq!(got.npoint(), cases::DIST_UP.len(), "{id}");
        for j in 0..got.ncoord() {
            assert!(
                got.axis(j).iter().all(|&v| v == BAD_VALUE),
                "{id}: axis {j}"
            );
        }
    }
}

#[test]
fn reg_split_matches_the_c_library() {
    for &(id, want) in cases::SPLIT_CASES {
        let parts = make_region(id).reg_split().unwrap();
        assert_eq!(parts.len(), want.len(), "{id}: parts");
        for (i, (part, want)) in parts.iter().zip(want).enumerate() {
            let dump =
                native_dump(&AnyObject::from_mapping(part.as_ref() as &dyn Mapping).unwrap())
                    .unwrap();
            assert_c_text(&dump, want, &format!("{id}: part {i}"));
        }
    }
}

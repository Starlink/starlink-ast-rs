//! Whether Regions are bounded, and their bounding boxes, against the C
//! library where a class's own `GetBounded` or `RegBaseBox` decides them: a
//! PointList, negated in a Cartesian Frame and in a SkyFrame (region.c
//! `GetBounded`); a Prism, negated (prism.c `GetBounded`); an Interval
//! excluding a range on each axis (interval.c `RegBaseBox`); and a sky
//! PointList either side of RA 0, with its default uncertainty
//! (pointlist.c `RegBaseBox`).
//!
//! The values come from `tools/oracle_fixtures/region_bounded_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_bounded_oracle/cases.rs"]
mod cases;

use ast_core::frames::{BasicFrame, BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use ast_core::regions::{
    BoxRegionBuilder, IntervalBuilder, PointList, PointListBuilder, PrismBuilder, Region,
};

fn frame(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

fn pointlist(frm: impl Frame + 'static, pts: &[[f64; 2]]) -> PointList {
    PointListBuilder::new(frm, pts.iter().map(|p| p.to_vec()).collect())
        .unwrap()
        .build()
        .unwrap()
}

fn bounds(reg: &dyn Region) -> [[f64; 2]; 2] {
    let (l, u) = reg.get_region_bounds().unwrap();
    [[l[0], l[1]], [u[0], u[1]]]
}

const POINTS: [[f64; 2]; 3] = [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0]];

#[test]
fn negated_pointlist_is_unbounded_unless_on_the_sky() {
    let mut pl = pointlist(frame(2), &POINTS);
    assert_eq!(pl.is_bounded().unwrap(), cases::POINTLIST);
    pl.negate();
    assert_eq!(pl.is_bounded().unwrap(), cases::POINTLIST_NEGATED);
    let mut sky = pointlist(SkyFrame::new(SkySystem::Icrs), &POINTS);
    sky.negate();
    assert_eq!(sky.is_bounded().unwrap(), cases::SKY_POINTLIST_NEGATED);
}

#[test]
fn negated_prism_tests_negated_components() {
    let bx = BoxRegionBuilder::from_bounds(frame(2), &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    let iv = IntervalBuilder::new(frame(1), vec![0.0], vec![1.0])
        .unwrap()
        .build()
        .unwrap();
    let mut prism = PrismBuilder::new(bx, iv).unwrap().build().unwrap();
    assert_eq!(prism.is_bounded().unwrap(), cases::PRISM);
    prism.negate();
    assert_eq!(prism.is_bounded().unwrap(), cases::PRISM_NEGATED);
}

#[test]
fn excluded_interval_keeps_its_limits() {
    let iv = IntervalBuilder::new(frame(2), vec![3.0, 4.0], vec![1.0, 2.0])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(bounds(&iv), cases::EXCLUDED_BOUNDS);
}

#[test]
fn sky_pointlist_box_wraps_through_ra_zero() {
    let pl = pointlist(
        SkyFrame::new(SkySystem::Icrs),
        &[[0.01, 0.1], [6.27, 0.2], [6.2, 0.15]],
    );
    assert_eq!(bounds(&pl), cases::SKY_BOUNDS);
    assert_eq!(
        bounds(pl.get_def_unc().unwrap().as_ref()),
        cases::SKY_UNC_BOUNDS
    );
}

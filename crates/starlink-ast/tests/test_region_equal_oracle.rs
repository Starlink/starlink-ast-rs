// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Region equality against the C library (region.c `Equal`): the same
//! class, defining points equal within pointset.c `Equal`'s default
//! accuracy, equal base and current Frames, an equal base->current Mapping,
//! and the same Negated and Closed values. A Region whose Unit was changed
//! adaptively has the same points and current Frame as a Box built in the
//! new unit, but a different base Frame and Mapping, so the two are neither
//! equal nor, for `astOverlap`, the same Region.
//!
//! The results come from `tools/oracle_fixtures/region_equal_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_equal_oracle/cases.rs"]
mod cases;

use starlink_ast::frames::{BasicFrame, BasicFrameBuilder, Frame};
use starlink_ast::mappings::Mapping;
use starlink_ast::regions::{BoxRegionBuilder, DynRegion, Overlap};

fn frame(unit1: Option<&str>) -> BasicFrame {
    let mut f = BasicFrameBuilder::new(2).build().unwrap();
    if let Some(unit1) = unit1 {
        f.set_unit(1, unit1).unwrap();
        f.set_unit(2, "m").unwrap();
        f.set_active_unit(true);
    }
    f
}

fn box_in(f: BasicFrame, x1: f64, y1: f64) -> DynRegion {
    Box::new(
        BoxRegionBuilder::from_bounds(f, &[0.0, 0.0], &[x1, y1])
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn pair(name: &str) -> (DynRegion, DynRegion) {
    match name {
        "remapped_against_plain" => {
            let mut a = box_in(frame(Some("m")), 1.0, 1.0);
            a.set_unit(1, "km").unwrap();
            (a, box_in(frame(Some("km")), 1.0, 1.0))
        }
        "points_within_accuracy" => (
            box_in(frame(None), 1.0, 1.0),
            box_in(frame(None), 1.0 + 1.0e-15, 1.0),
        ),
        "points_beyond_accuracy" => (
            box_in(frame(None), 1.0, 1.0),
            box_in(frame(None), 1.0 + 1.0e-9, 1.0),
        ),
        "same" => (box_in(frame(None), 1.0, 1.0), box_in(frame(None), 1.0, 1.0)),
        "negated" => {
            let mut a = box_in(frame(None), 1.0, 1.0);
            a.negate();
            (a, box_in(frame(None), 1.0, 1.0))
        }
        other => panic!("no Rust construction for case {other}"),
    }
}

fn code(overlap: Overlap) -> i32 {
    match overlap {
        Overlap::Incomparable => 0,
        Overlap::None => 1,
        Overlap::FirstInside => 2,
        Overlap::SecondInside => 3,
        Overlap::Partial => 4,
        Overlap::Same => 5,
        Overlap::Negation => 6,
    }
}

#[test]
fn equality_and_overlap_match_the_c_library() {
    for &(name, want_equal, want_overlap) in cases::CASES {
        let (a, b) = pair(name);
        assert_eq!(
            (a.as_ref() as &dyn Mapping).equal(b.as_ref() as &dyn Mapping),
            want_equal,
            "{name}: equal"
        );
        assert_eq!(
            code(a.overlap(b.as_ref()).unwrap()),
            want_overlap,
            "{name}: overlap"
        );
    }
}

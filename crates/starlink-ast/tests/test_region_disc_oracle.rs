//! `get_region_disc` against the C library (region.c `GetRegionDisc`, which
//! no Region class overrides): a disc in the current Frame enclosing the
//! un-negated Region's boundary mesh, found in a local system on the sky,
//! with a bad center and radius for an unbounded Region and AST__INVAR for
//! one that is not 2-dimensional.
//!
//! The values come from `tools/oracle_fixtures/region_disc_probe.c`. The
//! sky Ellipse is read from C's dump.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_disc_oracle/cases.rs"]
mod cases;

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::frames::{BasicFrameBuilder, SkyFrame, SkySystem};
use starlink_ast::from_native_string;
use starlink_ast::regions::{
    BoxRegionBuilder, CircleBuilder, DynRegion, IntervalBuilder, NullRegionBuilder,
    PointListBuilder, PolygonBuilder,
};

fn region(name: &str) -> DynRegion {
    let f2 = || BasicFrameBuilder::new(2).build().unwrap();
    match name {
        "box" | "negated_box" => {
            let mut b: DynRegion = Box::new(
                BoxRegionBuilder::from_bounds(f2(), &[0.0, 0.0], &[20.0, 10.0])
                    .unwrap()
                    .build()
                    .unwrap(),
            );
            if name == "negated_box" {
                b.negate();
            }
            b
        }
        "sky_circle" => Box::new(
            CircleBuilder::new(SkyFrame::new(SkySystem::Icrs), vec![1.0, 1.2])
                .unwrap()
                .radius(0.01)
                .build()
                .unwrap(),
        ),
        "box_6_8" => Box::new(
            BoxRegionBuilder::from_bounds(f2(), &[0.0, 0.0], &[6.0, 8.0])
                .unwrap()
                .build()
                .unwrap(),
        ),
        "circle_7" => Box::new(
            CircleBuilder::new(f2(), vec![3.0, 4.0])
                .unwrap()
                .radius(7.0)
                .build()
                .unwrap(),
        ),
        "pointlist_3" => Box::new(
            PointListBuilder::new(f2(), vec![vec![0.0, 0.0], vec![3.0, 0.0], vec![0.0, 4.0]])
                .unwrap()
                .build()
                .unwrap(),
        ),
        "sky_polygon" => Box::new(
            PolygonBuilder::new(
                SkyFrame::new(SkySystem::Icrs),
                vec![
                    vec![0.0, 0.2],
                    vec![1.0, 0.2],
                    vec![1.0, 0.8],
                    vec![0.0, 0.8],
                ],
            )
            .unwrap()
            .build()
            .unwrap(),
        ),
        "null_region" => Box::new(NullRegionBuilder::new(f2()).build().unwrap()),
        "sky_ellipse" => from_native_string(cases::SKY_ELLIPSE)
            .unwrap()
            .into_region()
            .unwrap(),
        "half_plane" => Box::new(
            IntervalBuilder::new(f2(), vec![0.0, BAD_VALUE], vec![BAD_VALUE, BAD_VALUE])
                .unwrap()
                .build()
                .unwrap(),
        ),
        "three_d_box" => Box::new(
            BoxRegionBuilder::from_bounds(
                BasicFrameBuilder::new(3).build().unwrap(),
                &[0.0; 3],
                &[1.0; 3],
            )
            .unwrap()
            .build()
            .unwrap(),
        ),
        other => panic!("no Rust construction for case {other}"),
    }
}

fn matches(name: &str, want: Result<[f64; 3], &str>) -> bool {
    match (region(name).get_region_disc(), want) {
        (Ok((centre, radius)), Ok(want)) => {
            crate::common::c_values_eq(&[centre[0], centre[1], radius], &want)
        }
        (Err(e), Err(code)) => format!("{:?}", e.code()) == code,
        _ => false,
    }
}

#[test]
fn discs_match_the_c_library() {
    for &(name, want) in cases::CASES {
        assert!(
            matches(name, want),
            "{name}: got {:?}, C gives {want:?}",
            region(name).get_region_disc()
        );
    }
}

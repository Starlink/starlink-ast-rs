//! NullRegion membership against the C library (nullregion.c `Transform`):
//! a negated NullRegion passes every point through as given, BAD axes and
//! all, before and after mapping through a ZoomMap, as it inherits Frame's
//! unit Transform; an un-negated one makes every value BAD.
//!
//! The values come from `tools/oracle_fixtures/region_nulltran_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_nulltran_oracle/cases.rs"]
mod cases;

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::BasicFrameBuilder;
use ast_core::mappings::ZoomMap;
use ast_core::regions::{DynRegion, NullRegionBuilder, Region};

fn null_region(negated: bool) -> DynRegion {
    let mut nr = NullRegionBuilder::new(BasicFrameBuilder::new(2).build().unwrap())
        .build()
        .unwrap();
    if negated {
        nr.negate();
    }
    Box::new(nr)
}

#[test]
fn transform_matches_the_c_library() {
    let frame = || BasicFrameBuilder::new(2).build().unwrap();
    for &(name, want) in cases::CASES {
        let region = match name {
            "negated" => null_region(true),
            "negated_zoomed" => null_region(true)
                .reg_map_region(&ZoomMap::new(2, 2.0).unwrap(), &frame())
                .unwrap(),
            "plain" => null_region(false),
            other => panic!("no NullRegion for case {other}"),
        };
        let out = region
            .transform(&PointSet::from_rows(cases::POINTS), true)
            .unwrap();
        let got: Vec<f64> = (0..3).flat_map(|i| out.point(i).to_vec()).collect();
        assert_eq!(got, want, "{name}");

        // Membership is a point left good.
        for (i, p) in cases::POINTS.iter().enumerate() {
            let inside = want[2 * i..2 * i + 2].iter().all(|&v| v != BAD_VALUE);
            assert_eq!(
                region.point_in_region(p).unwrap(),
                inside,
                "{name} point {i}"
            );
        }
    }
}

//! `mask` against the C library's `astMask8D` (region.c `Mask<X>`, and
//! pointlist.c `Mask<X>` for a PointList): which pixels of a grid are set,
//! and the count returned, masking inside and outside, with no Mapping and
//! through a Mapping from the Region's Frame to the grid, and the
//! AST__NGDIN and AST__GBDIN errors. The "_prefilled" cases start from an
//! array holding 0, 1, 2 in turn rather than zeros, and the "_ub" cases
//! (also prefilled) mask `u8` data with `astMask8UB`.
//!
//! The values come from `tools/oracle_fixtures/region_mask_probe.c`. C
//! wrote beyond the array for a PointList point outside the grid; fixed
//! upstream (ba5a6199), which the probe needs.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_mask_oracle/cases.rs"]
mod cases;

use starlink_ast::frames::BasicFrameBuilder;
use starlink_ast::mappings::{Mapping, UnitMap, ZoomMap};
use starlink_ast::regions::{
    BoxRegionBuilder, CircleBuilder, DynRegion, IntervalBuilder, PointListBuilder, mask,
};

fn circle(cx: f64, cy: f64, r: f64) -> DynRegion {
    Box::new(
        CircleBuilder::new(BasicFrameBuilder::new(2).build().unwrap(), vec![cx, cy])
            .unwrap()
            .radius(r)
            .build()
            .unwrap(),
    )
}

/// The case's Region, Mapping, whether the inside is masked, and grid.
type Case = (
    DynRegion,
    Option<Box<dyn Mapping>>,
    bool,
    Vec<i64>,
    Vec<i64>,
);

fn case(name: &str) -> Case {
    let name = name.trim_end_matches("_prefilled").trim_end_matches("_ub");
    let f2 = || BasicFrameBuilder::new(2).build().unwrap();
    let negated_box = || -> DynRegion {
        let mut b: DynRegion = Box::new(
            BoxRegionBuilder::from_bounds(f2(), &[2.0, 2.0], &[4.0, 4.0])
                .unwrap()
                .build()
                .unwrap(),
        );
        b.negate();
        b
    };
    let pointlist = || -> DynRegion {
        Box::new(
            PointListBuilder::new(f2(), vec![vec![2.0, 3.0], vec![4.0, 1.0], vec![10.0, 10.0]])
                .unwrap()
                .build()
                .unwrap(),
        )
    };
    let g5 = (vec![1, 1], vec![5, 5]);
    let g6 = (vec![1, 1], vec![6, 6]);
    match name {
        "circle_inside" => (circle(3.0, 3.0, 2.0), None, true, g5.0, g5.1),
        "circle_outside" => (circle(3.0, 3.0, 2.0), None, false, g5.0, g5.1),
        "small_circle_inside" => (circle(5.0, 5.0, 1.0), None, true, vec![1, 1], vec![10, 10]),
        "small_circle_outside" => (circle(5.0, 5.0, 1.0), None, false, vec![1, 1], vec![10, 10]),
        "circle_through_zoommap" => (
            circle(1.5, 1.5, 1.0),
            Some(Box::new(ZoomMap::new(2, 2.0).unwrap())),
            true,
            g6.0,
            g6.1,
        ),
        "negated_box_inside" => (negated_box(), None, true, g6.0, g6.1),
        "negated_box_outside" => (negated_box(), None, false, g6.0, g6.1),
        "pointlist_inside" => (pointlist(), None, true, g5.0, g5.1),
        "pointlist_outside" => (pointlist(), None, false, g5.0, g5.1),
        "interval_inside" => (
            Box::new(
                IntervalBuilder::new(
                    BasicFrameBuilder::new(1).build().unwrap(),
                    vec![3.0],
                    vec![7.0],
                )
                .unwrap()
                .build()
                .unwrap(),
            ),
            None,
            true,
            vec![1],
            vec![10],
        ),
        "map_with_wrong_inputs" => (
            circle(3.0, 3.0, 2.0),
            Some(Box::new(UnitMap::new(3))),
            true,
            g5.0,
            g5.1,
        ),
        "lower_bound_above_upper" => (circle(3.0, 3.0, 2.0), None, true, vec![6, 1], g5.1),
        other => panic!("no Rust construction for case {other}"),
    }
}

#[test]
fn masks_match_the_c_library() {
    for &(name, want) in cases::CASES {
        let (region, map, inside, lbnd, ubnd) = case(name);
        let npix: i64 = lbnd
            .iter()
            .zip(&ubnd)
            .map(|(l, u)| (u - l + 1).max(1))
            .product();
        let prefilled = name.ends_with("_prefilled") || name.ends_with("_ub");
        let mut data: Vec<u8> = (0..npix as usize)
            .map(|i| if prefilled { (i % 3) as u8 } else { 0 })
            .collect();
        let got = if name.ends_with("_ub") {
            mask(
                region.as_ref(),
                map.as_deref(),
                inside,
                &lbnd,
                &ubnd,
                &mut data,
                1,
            )
        } else {
            let mut doubles: Vec<f64> = data.iter().map(|&v| f64::from(v)).collect();
            let got = mask(
                region.as_ref(),
                map.as_deref(),
                inside,
                &lbnd,
                &ubnd,
                &mut doubles,
                1.0,
            );
            // The values are small whole numbers; anything else shows as '?'.
            for (d, &v) in data.iter_mut().zip(&doubles) {
                *d = if v == v.trunc() && (0.0..10.0).contains(&v) {
                    v as u8
                } else {
                    b'?' - b'0'
                };
            }
            got
        };
        match want {
            Ok((count, pattern)) => {
                let count_got = got.unwrap_or_else(|e| panic!("{name}: {e}"));
                let pattern_got: String = data.iter().map(|&v| char::from(b'0' + v)).collect();
                assert_eq!(
                    (count_got, pattern_got.as_str()),
                    (count, pattern),
                    "{name}"
                );
            }
            Err(code) => {
                let e = got.expect_err(name);
                assert_eq!(format!("{:?}", e.code()), code, "{name}");
            }
        }
    }
}

//! Box membership against the C library (box.c `Cache` and `Transform`): a
//! Box in an ICRS SkyFrame whose corner wraps through RA 0, which C sizes by
//! the axis distance from center to corner, and a Box of zero width on one
//! axis, which C widens to the width of its uncertainty when Negated and
//! Closed differ. Also the sky Box's bounding box, and the boundary meshes
//! (box.c `RegBaseMesh`) of a flat Box, one whose axes have different
//! units, a sky Box, a 3-D Box and a single-point Box, which C meshed wrongly
//! until upstream 4785102b.
//!
//! The values come from `tools/oracle_fixtures/region_box_probe.c`. C's
//! widening depended on whether the limits were cached first; fixed
//! upstream (d0853dcd), which the probe needs.

// Generated; formatted by its generator rather than rustfmt. 6.28 there is
// a right ascension near 0, not an approximation of 2*pi.
#[rustfmt::skip]
#[allow(clippy::approx_constant)]
#[path = "region_box_oracle/cases.rs"]
mod cases;

use ast_core::constants::BAD_VALUE;
use ast_core::frames::{BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use ast_core::regions::{BoxRegionBuilder, DynRegion, Region};

fn zero_width(negated: bool, closed: bool) -> DynRegion {
    let mut b: DynRegion = Box::new(
        BoxRegionBuilder::from_bounds(
            BasicFrameBuilder::new(2).build().unwrap(),
            &[1.0, 5.0],
            &[2.0, 5.0],
        )
        .unwrap()
        .build()
        .unwrap(),
    );
    if !closed {
        b.set_closed(false);
    }
    if negated {
        b.negate();
    }
    b
}

fn sky_box() -> DynRegion {
    Box::new(
        BoxRegionBuilder::new(
            SkyFrame::new(SkySystem::Icrs),
            vec![6.27, 0.5],
            vec![0.037, 0.52],
        )
        .unwrap()
        .build()
        .unwrap(),
    )
}

fn membership(reg: &dyn Region, x: &[f64], y: &[f64]) -> String {
    let (xo, _) = reg.tran2(x, y, true).unwrap();
    xo.iter()
        .map(|&v| if v == BAD_VALUE { '0' } else { '1' })
        .collect()
}

#[test]
fn membership_matches_the_c_library() {
    for &(name, want) in cases::CASES {
        let x15 = [1.5; 4];
        let got = match name {
            "sky_box" => membership(sky_box().as_ref(), &cases::SKY_X, &cases::SKY_Y),
            "zero_width" => membership(zero_width(false, true).as_ref(), &x15, &cases::ZERO_Y),
            "zero_width_negated_open" => {
                membership(zero_width(true, false).as_ref(), &x15, &cases::ZERO_Y)
            }
            "zero_width_negated_closed" => {
                membership(zero_width(true, true).as_ref(), &x15, &cases::ZERO_Y)
            }
            other => panic!("no Rust construction for case {other}"),
        };
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn sky_box_bounds_match_the_c_library() {
    let (lb, ub) = sky_box().get_region_bounds().unwrap();
    assert_eq!([lb[0], lb[1], ub[0], ub[1]], cases::SKY_BOUNDS);
}

fn boxed(f: impl Frame + 'static, lbnd: &[f64], ubnd: &[f64]) -> DynRegion {
    Box::new(
        BoxRegionBuilder::from_bounds(f, lbnd, ubnd)
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn mesh_box(name: &str) -> DynRegion {
    let f2 = || BasicFrameBuilder::new(2).build().unwrap();
    match name {
        "flat" => boxed(f2(), &[0.0, 0.0], &[3.0, 1.0]),
        "mixed_units" => {
            let mut f = f2();
            f.set_unit(1, "Hz").unwrap();
            f.set_unit(2, "m").unwrap();
            boxed(f, &[0.0, 0.0], &[3.0, 1.0])
        }
        "sky" => boxed(SkyFrame::new(SkySystem::Icrs), &[1.0, 0.2], &[1.3, 0.4]),
        "three_d" => boxed(
            BasicFrameBuilder::new(3).build().unwrap(),
            &[0.0; 3],
            &[1.0, 2.0, 3.0],
        ),
        "single_point" => boxed(f2(), &[1.0, 3.0], &[1.0, 3.0]),
        other => panic!("no Rust construction for case {other}"),
    }
}

#[test]
fn meshes_match_the_c_library() {
    for &(name, want) in cases::MESHES {
        let mesh = mesh_box(name).region_mesh().unwrap();
        let got: Vec<f64> = (0..mesh.npoint())
            .flat_map(|i| mesh.point(i).to_vec())
            .collect();
        assert_eq!(got.len(), want.len(), "{name}: number of values");
        assert_eq!(got, want, "{name}");
    }
}

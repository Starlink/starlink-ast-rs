//! `Ellipse::ellipse_pars` against the C library's `astEllipsePars`
//! (ellipse.c `EllipsePars` and `CalcPars`): a Cartesian and an ICRS Ellipse
//! given by form 1, a Cartesian Ellipse given by form 0 whose second point is
//! not the end of the secondary axis (read from C's dump, as the port has no
//! form-0 builder), and a Cartesian Ellipse mapped through a ZoomMap without
//! simplification, so its current Frame differs from its base Frame.
//!
//! The values come from `tools/oracle_fixtures/region_ellipsepars_probe.c`.

// Generated; formatted by its generator rather than rustfmt. Its angles are
// C's values, one of which is pi/2 to the last bit.
#[rustfmt::skip]
#[allow(clippy::approx_constant)]
#[path = "region_ellipsepars_oracle/cases.rs"]
mod cases;

// Generated; formatted by its generator rather than rustfmt. Only its form-0
// dump is used here.
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "region_ellipse_oracle/cases.rs"]
mod ellipse_cases;

use ast_core::frames::{BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use ast_core::from_native_string;
use ast_core::mappings::ZoomMap;
use ast_core::regions::{Ellipse, EllipseBuilder, Region};

fn form1(frame: impl Frame + 'static, centre: [f64; 2], axes: [f64; 2]) -> Ellipse {
    EllipseBuilder::new(frame, centre.to_vec(), axes, 0.3)
        .unwrap()
        .build()
        .unwrap()
}

fn form0() -> Ellipse {
    let (_, dump, _) = ellipse_cases::CASES
        .iter()
        .find(|(name, _, _)| *name == "form0")
        .unwrap();
    let reg = from_native_string(dump).unwrap().into_region().unwrap();
    reg.as_any().downcast_ref::<Ellipse>().unwrap().clone()
}

fn zoomed() -> Ellipse {
    let frame = || BasicFrameBuilder::new(2).build().unwrap();
    let e = form1(frame(), [1.0, 2.0], [2.0, 1.0]);
    let zoom = ZoomMap::new(2, 3.0).unwrap();
    let reg = e.reg_map_region(&zoom, &frame()).unwrap();
    reg.as_any().downcast_ref::<Ellipse>().unwrap().clone()
}

#[test]
fn ellipse_pars_match_the_c_library() {
    for &(name, want) in cases::CASES {
        let e = match name {
            "cartesian" => form1(
                BasicFrameBuilder::new(2).build().unwrap(),
                [1.0, 2.0],
                [2.0, 1.0],
            ),
            "sky" => form1(SkyFrame::new(SkySystem::Icrs), [1.0, 0.8], [0.2, 0.1]),
            "form0" => form0(),
            "zoomed" => zoomed(),
            other => panic!("no Ellipse for case {other}"),
        };
        let p = e.ellipse_pars().unwrap();
        let got = [
            p.centre[0],
            p.centre[1],
            p.a,
            p.b,
            p.angle,
            p.p1[0],
            p.p1[1],
            p.p2[0],
            p.p2[1],
        ];
        assert_eq!(got, want, "{name}");
    }
}

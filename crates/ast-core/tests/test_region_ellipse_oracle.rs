//! Ellipse membership against the C library (ellipse.c `Transform`, with its
//! axes from `CalcPars`): a three-point Ellipse whose second point is not the
//! end of its secondary axis, and an Ellipse in an ICRS SkyFrame over a grid,
//! where the components along and across the primary axis are measured on
//! the sky. Each Ellipse is read from C's dump, and `EllipseBuilder`, which
//! takes the arguments of C's form-1 constructor, reproduces those dumps.
//!
//! The values come from `tools/oracle_fixtures/region_ellipse_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_ellipse_oracle/cases.rs"]
mod cases;

use ast_core::constants::BAD_VALUE;
use ast_core::frames::{BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use ast_core::mappings::Mapping;
use ast_core::regions::EllipseBuilder;
use ast_core::{AnyObject, from_native_string, to_native_string};

#[test]
fn membership_matches_the_c_library() {
    let (nx, ny) = cases::GRID;
    let (gx, gy): (Vec<f64>, Vec<f64>) = (0..nx * ny)
        .map(|k| (0.7 + 0.01 * (k % nx) as f64, 0.65 + 0.006 * (k / nx) as f64))
        .unzip();
    for &(name, dump, want) in cases::CASES {
        let reg = from_native_string(dump).unwrap().into_region().unwrap();
        let (x, y): (&[f64], &[f64]) = match name {
            "form0" => (&cases::FORM0_X, &cases::FORM0_Y),
            "sky" => (&gx, &gy),
            "form1" => (&cases::FORM1_X, &cases::FORM1_Y),
            other => panic!("no test points for case {other}"),
        };
        let (xo, _) = reg.tran2(x, y, true).unwrap();
        let got: String = xo
            .iter()
            .map(|&v| if v == BAD_VALUE { '0' } else { '1' })
            .collect();
        let differ = got
            .chars()
            .zip(want.chars())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(differ, 0, "{name}: {differ} points differ from C");
    }
}

fn built(frame: impl Frame + 'static, centre: [f64; 2], axes: [f64; 2], angle: f64) -> String {
    let e = EllipseBuilder::new(frame, centre.to_vec(), axes, angle)
        .unwrap()
        .build()
        .unwrap();
    to_native_string(&AnyObject::from_mapping(&e as &dyn Mapping).unwrap()).unwrap()
}

/// `EllipseBuilder` takes the arguments of `astEllipse` form 1, the angle
/// from the second axis towards the first, and places the axis ends as C
/// does: its Ellipses dump exactly as C's.
#[test]
fn builder_matches_astellipse_form1() {
    let want = |name: &str| cases::CASES.iter().find(|c| c.0 == name).unwrap().1;
    assert_eq!(
        built(SkyFrame::new(SkySystem::Icrs), [1.0, 0.8], [0.2, 0.1], 0.3),
        want("sky")
    );
    assert_eq!(
        built(
            BasicFrameBuilder::new(2).build().unwrap(),
            [1.0, 2.0],
            [2.0, 1.0],
            0.3
        ),
        want("form1")
    );
}

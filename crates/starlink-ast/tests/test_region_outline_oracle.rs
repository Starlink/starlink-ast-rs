//! `Polygon::downsize` and `Polygon::outline` against the C library's
//! `astDownsize` (polygon.c `Downsize`, `DownsizePoly` and `FindMax`) and
//! `astOutline8<X>` (polygon.c `MAKE_OUTLINE`, `FindInsidePoint`, `TraceEdge`
//! and `SmoothPoly`).
//!
//! Downsize reduces a star-shaped Polygon in a Frame by a maximum error, a
//! maximum vertex count, both and neither, and one in an ICRS SkyFrame.
//! Outline traces a blob with a hole and two other components for each
//! comparison, with and without smoothing and downsizing, with the Starlink
//! pixel convention, from an inside pixel left of the hole, from one outside
//! the array, and for an int array.
//!
//! The values come from `tools/oracle_fixtures/region_outline_probe.c`.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_outline_oracle/cases.rs"]
mod cases;

use starlink_ast::AnyObject;
use starlink_ast::error::ErrorCode;
use starlink_ast::frames::{BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use starlink_ast::regions::{PixelOper, Polygon, PolygonBuilder};

fn dump(poly: Polygon) -> String {
    native_dump(&AnyObject::Polygon(poly)).unwrap()
}

fn polygon(frame: impl Frame + 'static, xy: &[[f64; 36]; 2]) -> Polygon {
    let vertices = (0..36).map(|i| vec![xy[0][i], xy[1][i]]).collect();
    PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn downsize_matches_the_c_library() {
    let plane = polygon(BasicFrameBuilder::new(2).build().unwrap(), &cases::STAR);
    let sky = polygon(SkyFrame::new(SkySystem::Icrs), &cases::SKY);
    for &(name, want) in cases::DOWNSIZE {
        let got = match name {
            "err" => plane.downsize(0.5, 0),
            "maxvert" => plane.downsize(0.0, 8),
            "both" => plane.downsize(0.1, 6),
            "copy" => plane.downsize(0.0, 0),
            "sky" => sky.downsize(1e-2, 0),
            other => panic!("no inputs for case {other}"),
        };
        assert_eq!(dump(got.unwrap()), want, "{name}");
    }
}

#[test]
fn outline_matches_the_c_library() {
    let (nx, ny) = cases::DIMS;
    let lbnd = [1, 1];
    let ubnd = [nx as i64, ny as i64];
    let lbnd2 = [-2, 5];
    let ubnd2 = [-2 + nx as i64 - 1, 5 + ny as i64 - 1];
    let f = &cases::ARRAY;
    let int: Vec<i32> = f.iter().map(|&v| v as i32).collect();
    let (blob, hole_row, two, blob2, outside) = ([3, 3], [2, 4], [10, 4], [0, 7], [100, 100]);
    let outline = |value: f32, oper, maxerr, maxvert, inside| {
        Polygon::outline(value, oper, f, lbnd, ubnd, maxerr, maxvert, inside, false)
    };

    for &(name, want) in cases::OUTLINE {
        let got = match name {
            "eq1" => outline(1.0, PixelOper::Eq, 0.0, 0, blob),
            "eq1_hole_row" => outline(1.0, PixelOper::Eq, 0.0, 0, hole_row),
            "eq1_err" => outline(1.0, PixelOper::Eq, 0.3, 0, blob),
            "eq1_maxvert" => outline(1.0, PixelOper::Eq, 0.0, 6, blob),
            "eq1_starpix" => {
                Polygon::outline(1.0, PixelOper::Eq, f, lbnd2, ubnd2, 0.0, 0, blob2, true)
            }
            "eq2" => outline(2.0, PixelOper::Eq, 0.0, 0, two),
            "eq2_search" => outline(2.0, PixelOper::Eq, 0.0, 0, outside),
            "ge1" => outline(1.0, PixelOper::Ge, 0.0, 0, blob),
            "gt1" => outline(1.0, PixelOper::Gt, 0.0, 0, outside),
            "le1" => outline(1.0, PixelOper::Le, 0.0, 0, blob),
            "lt1" => outline(1.0, PixelOper::Lt, 0.0, 0, outside),
            "ne0" => outline(0.0, PixelOper::Ne, 0.0, 0, blob),
            "int_ge2" => Polygon::outline(2, PixelOper::Ge, &int, lbnd, ubnd, 0.0, 0, two, false),
            other => panic!("no inputs for case {other}"),
        };
        assert_eq!(dump(got.unwrap()), want, "{name}");
    }
}

#[test]
fn outline_reports_no_selected_pixel_as_c_does() {
    let (nx, ny) = cases::DIMS;
    let err = Polygon::outline(
        9.0f32,
        PixelOper::Eq,
        &cases::ARRAY,
        [1, 1],
        [nx as i64, ny as i64],
        0.0,
        0,
        [3, 3],
        false,
    )
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Nonin);
    assert_eq!(
        err.message(),
        "astOutlineF: Could not find a pixel value equal to 9 in the supplied array."
    );
}

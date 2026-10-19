//! `Polygon::convex` against the C library's `astConvex8<X>` (polygon.c
//! `MAKE_CONVEX`, `ConvexHull`, `FindBoxEdge` and `PartHull`): every
//! comparison, pixel centres at integer and at half-integer (`starpix`) PIXEL
//! coordinates, a float and an int array, a single selected pixel, and none.
//!
//! The values come from `tools/oracle_fixtures/region_convex_probe.c`.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_convex_oracle/cases.rs"]
mod cases;

use ast_core::AnyObject;
use ast_core::regions::{PixelOper, Polygon};

fn dump(poly: Option<Polygon>) -> String {
    poly.map_or_else(String::new, |p| {
        native_dump(&AnyObject::Polygon(p)).unwrap()
    })
}

#[test]
fn convex_matches_the_c_library() {
    let (nx, ny) = cases::DIMS;
    let lbnd = [1, 1];
    let ubnd = [nx as i64, ny as i64];
    let lbnd2 = [-3, 10];
    let ubnd2 = [-3 + nx as i64 - 1, 10 + ny as i64 - 1];
    let f = &cases::ARRAY;
    let int: Vec<i32> = f.iter().map(|&v| v as i32).collect();
    let mut one = vec![0.0f32; nx * ny];
    one[2 * nx + 4] = 1.0;

    for &(name, want) in cases::CASES {
        let got = match name {
            "eq5" => Polygon::convex(5.0, PixelOper::Eq, f, lbnd, ubnd, false),
            "ge3" => Polygon::convex(3.0, PixelOper::Ge, f, lbnd, ubnd, false),
            "gt3" => Polygon::convex(3.0, PixelOper::Gt, f, lbnd, ubnd, false),
            "le0" => Polygon::convex(0.0, PixelOper::Le, f, lbnd, ubnd, false),
            "lt5" => Polygon::convex(5.0, PixelOper::Lt, f, lbnd, ubnd, false),
            "ne0" => Polygon::convex(0.0, PixelOper::Ne, f, lbnd, ubnd, false),
            "ne0_starpix" => Polygon::convex(0.0, PixelOper::Ne, f, lbnd2, ubnd2, true),
            "int_ge5" => Polygon::convex(5, PixelOper::Ge, &int, lbnd2, ubnd2, false),
            "single" => Polygon::convex(1.0, PixelOper::Eq, &one, lbnd, ubnd, false),
            "none" => Polygon::convex(9.0, PixelOper::Eq, f, lbnd, ubnd, false),
            other => panic!("no inputs for case {other}"),
        };
        assert_eq!(dump(got.unwrap()), want, "{name}");
    }
}

#[test]
fn convex_rejects_an_array_smaller_than_its_bounds() {
    let err = Polygon::convex(1.0f32, PixelOper::Eq, &[0.0; 5], [1, 1], [3, 2], false).unwrap_err();
    assert_eq!(err.code(), ast_core::error::ErrorCode::Dimin);
}

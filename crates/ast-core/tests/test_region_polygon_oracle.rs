//! Polygon boundary meshes against the C library (polygon.c `RegBaseMesh`):
//! a quadrilateral in an ICRS SkyFrame, whose mesh follows great circles,
//! and a Cartesian square and triangle.
//!
//! The values come from `tools/oracle_fixtures/region_polygon_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_polygon_oracle/cases.rs"]
mod cases;

use ast_core::frames::{BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use ast_core::regions::{Polygon, PolygonBuilder, Region};

fn polygon(frame: impl Frame + 'static, vertices: Vec<Vec<f64>>) -> Polygon {
    PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn meshes_match_the_c_library() {
    for &(name, xy, want) in cases::CASES {
        let nv = xy.len() / 2;
        let vertices: Vec<Vec<f64>> = (0..nv).map(|i| vec![xy[i], xy[nv + i]]).collect();
        let poly = match name {
            "sky" => polygon(SkyFrame::new(SkySystem::Icrs), vertices),
            _ => polygon(BasicFrameBuilder::new(2).build().unwrap(), vertices),
        };
        let mesh = poly.region_mesh().unwrap();
        let got: Vec<f64> = (0..mesh.npoint())
            .flat_map(|i| mesh.point(i).to_vec())
            .collect();
        assert_eq!(got.len(), want.len(), "{name}: number of values");
        assert_eq!(got, want, "{name}");
    }
}

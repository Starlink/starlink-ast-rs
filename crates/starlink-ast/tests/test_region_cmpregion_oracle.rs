// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! CmpRegion construction, membership, mesh and dump against the C library
//! (cmpregion.c `astInitCmpRegion`, `Transform`, `RegBaseMesh`, `Dump`,
//! `Load`): an FK5 Circle ANDed with a Galactic Box, which the constructor
//! maps into FK5, and the XOR of two Boxes, which C makes from AND and OR and
//! dumps as the two Boxes XORed.
//!
//! The values come from `tools/oracle_fixtures/region_cmpregion_probe.c`.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_cmpregion_oracle/cases.rs"]
mod cases;

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::frames::{BasicFrameBuilder, SkyFrame, SkySystem};
use starlink_ast::mappings::Mapping;
use starlink_ast::regions::{
    BoxRegionBuilder, CircleBuilder, CmpOperator, CmpRegion, CmpRegionBuilder, Region,
};
use starlink_ast::{AnyObject, from_native_string};

fn membership(reg: &dyn Region, x: &[f64], y: &[f64]) -> String {
    let (xo, _) = reg.tran2(x, y, true).unwrap();
    xo.iter()
        .map(|&v| if v == BAD_VALUE { '0' } else { '1' })
        .collect()
}

fn sky_and() -> CmpRegion {
    let circle = CircleBuilder::new(SkyFrame::new(SkySystem::Fk5), vec![1.0, 0.5])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();
    let gbox = BoxRegionBuilder::new(
        SkyFrame::new(SkySystem::Galactic),
        vec![2.857, -0.346],
        vec![3.057, -0.146],
    )
    .unwrap()
    .build()
    .unwrap();
    CmpRegionBuilder::new(circle, gbox, CmpOperator::And)
        .unwrap()
        .build()
        .unwrap()
}

fn xor() -> CmpRegion {
    let f = || BasicFrameBuilder::new(2).build().unwrap();
    let b = |l: [f64; 2], u: [f64; 2]| {
        BoxRegionBuilder::from_bounds(f(), &l, &u)
            .unwrap()
            .build()
            .unwrap()
    };
    CmpRegionBuilder::new(
        b([0.0, 0.0], [2.0, 2.0]),
        b([1.0, 1.0], [3.0, 3.0]),
        CmpOperator::Xor,
    )
    .unwrap()
    .build()
    .unwrap()
}

#[test]
fn second_region_is_mapped_into_the_first_frame() {
    let cmp = sky_and();
    assert_eq!(
        membership(&cmp, &[1.0, 1.05, 1.3], &[0.5, 0.5, 0.5]),
        cases::SKY_AND
    );
}

#[test]
fn mesh_uses_component_current_frames() {
    let mesh = sky_and().region_mesh().unwrap();
    let got: Vec<f64> = (0..mesh.npoint())
        .flat_map(|i| mesh.point(i).to_vec())
        .collect();
    assert_eq!(got.len(), cases::SKY_AND_MESH.len(), "number of values");
    crate::common::assert_c_values(&got, cases::SKY_AND_MESH, "mesh");
}

#[test]
fn xor_is_made_dumped_and_read_as_c_does() {
    let x = [0.5, 1.5, 2.5, 4.0];
    let cmp = xor();
    assert_eq!(cmp.operator(), CmpOperator::Xor);
    assert_eq!(membership(&cmp, &x, &x), cases::XOR);
    let dump = native_dump(&AnyObject::from_mapping(&cmp as &dyn Mapping).unwrap()).unwrap();
    assert_eq!(dump, cases::XOR_DUMP);

    let back = from_native_string(cases::XOR_DUMP)
        .unwrap()
        .into_region()
        .unwrap();
    assert_eq!(membership(back.as_ref(), &x, &x), cases::XOR);
}

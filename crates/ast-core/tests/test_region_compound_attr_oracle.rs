//! How a CmpRegion and a Prism share Closed, MeshSize and FillFactor with
//! their components, against the C library: setting or clearing Closed or
//! MeshSize sets or clears it on both components too (cmpregion.c and
//! prism.c `MAKE_SET`, `MAKE_CLEAR`); `astInitCmpRegion` copies the first
//! component's set MeshSize and Closed; and the default FillFactor is the
//! first component's for a CmpRegion and the product of the components' for
//! a Prism (`GetFillFactor`).
//!
//! The values come from `tools/oracle_fixtures/region_compound_attr_probe.c`.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_compound_attr_oracle/cases.rs"]
mod cases;

use ast_core::AnyObject;
use ast_core::frames::{BasicFrame, BasicFrameBuilder};
use ast_core::mappings::Mapping;
use ast_core::regions::{
    BoxRegion, BoxRegionBuilder, CmpOperator, CmpRegionBuilder, IntervalBuilder, PrismBuilder,
    Region,
};

fn frame(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

fn boxr(l: [f64; 2], u: [f64; 2]) -> BoxRegion {
    BoxRegionBuilder::from_bounds(frame(2), &l, &u)
        .unwrap()
        .build()
        .unwrap()
}

fn dump(m: &dyn Mapping) -> String {
    native_dump(&AnyObject::from_mapping(m).unwrap()).unwrap()
}

#[test]
fn cmpregion_shares_closed_and_mesh_size_with_its_components() {
    let mut b1 = boxr([0.0, 0.0], [2.0, 2.0]);
    b1.set_mesh_size(50);
    b1.set_closed(false);
    b1.set_fill_factor(0.5).unwrap();
    let b2 = boxr([1.0, 1.0], [3.0, 3.0]);
    let mut cmp = CmpRegionBuilder::new(b1, b2, CmpOperator::Or)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(dump(&cmp), cases::CMP_MADE);
    assert_eq!(cmp.fill_factor(), cases::CMP_FILL);

    cmp.set_mesh_size(30);
    cmp.set_closed(true);
    assert_eq!(dump(&cmp), cases::CMP_SET);

    cmp.clear_mesh_size();
    cmp.clear_closed();
    assert_eq!(dump(&cmp), cases::CMP_CLEARED);
}

#[test]
fn prism_shares_closed_and_mesh_size_with_its_components() {
    let mut b = boxr([0.0, 0.0], [1.0, 1.0]);
    b.set_fill_factor(0.5).unwrap();
    let mut iv = IntervalBuilder::new(frame(1), vec![0.0], vec![1.0])
        .unwrap()
        .build()
        .unwrap();
    iv.set_fill_factor(0.4).unwrap();
    let mut prism = PrismBuilder::new(b, iv).unwrap().build().unwrap();
    assert_eq!(prism.fill_factor(), cases::PRISM_FILL);

    prism.set_mesh_size(30);
    prism.set_closed(false);
    assert_eq!(dump(&prism), cases::PRISM_SET);

    prism.clear_mesh_size();
    prism.clear_closed();
    assert_eq!(dump(&prism), cases::PRISM_CLEARED);
}

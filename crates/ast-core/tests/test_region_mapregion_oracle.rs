//! `astMapRegion`, protected and public, against the C library: the
//! protected form (region.c `MapRegion`, the port's `reg_map_region`) given
//! an Interval with a BAD limit and a Mapping that drops an axis, which C
//! does not axis-reduce because it has no flags saying which axes give good
//! values; and the public form (region.c `astMapRegionId_`, the port's
//! `map_region`), which simplifies the protected form's result, given a Box
//! and a PermMap that swaps its axes. Also `simplify` of the first case's
//! PermMap, whose inperm C keeps as given (permmap.c `MapMerge`, `NullPerm`).
//! And the dump of a Circle mapped through a ZoomMap of 2 then one of 0.5,
//! whose base->current Mapping simplifies to a UnitMap, so its dump writes
//! only its Frame (region.c `Dump`, which tests the simplified
//! `astRegMapping`).
//!
//! The values come from `tools/oracle_fixtures/region_mapregion_probe.c`.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_mapregion_oracle/cases.rs"]
mod cases;

use ast_core::AnyObject;
use ast_core::constants::BAD_VALUE;
use ast_core::frames::{BasicFrame, BasicFrameBuilder};
use ast_core::mappings::{Mapping, PermMap};
use ast_core::regions::{BoxRegionBuilder, IntervalBuilder, Region};

fn frame(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

fn dump(m: &dyn Mapping) -> String {
    native_dump(&AnyObject::from_mapping(m).unwrap()).unwrap()
}

#[test]
fn bad_limit_keeps_every_axis() {
    let iv = IntervalBuilder::new(frame(3), vec![0.0, 0.0, 0.0], vec![1.0, 1.0, BAD_VALUE])
        .unwrap()
        .build()
        .unwrap();
    let pm = PermMap::new(&[1, 2, 0], &[1, 2], &[]).unwrap();
    let mapped = iv.reg_map_region(&pm, &frame(2)).unwrap();
    assert_eq!(dump(mapped.as_ref()), cases::DROP_UNBOUNDED);
}

#[test]
fn public_map_region_simplifies() {
    let bx = BoxRegionBuilder::from_bounds(frame(2), &[0.0, 1.0], &[2.0, 4.0])
        .unwrap()
        .build()
        .unwrap();
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let mapped = bx.map_region(&pm, &frame(2)).unwrap();
    assert_eq!(dump(mapped.as_ref()), cases::SWAPPED_PUBLIC);
}

#[test]
fn permmap_with_bad_input_simplifies_as_c_does() {
    let pm = PermMap::new(&[1, 2, 0], &[1, 2], &[]).unwrap();
    let simplified = (&pm as &dyn Mapping).simplify().unwrap();
    assert_eq!(dump(simplified.as_ref()), cases::PERMMAP_SIMPLIFIED);
}

#[test]
fn identity_chain_dumps_only_the_frame() {
    use ast_core::mappings::{CmpMap, Composition, ZoomMap};
    let with_domain = |d: &str| {
        let mut f = frame(2);
        f.set_domain(d);
        f
    };
    let circle = ast_core::regions::CircleBuilder::new(with_domain("FOO"), vec![1.0, 0.5])
        .unwrap()
        .radius(0.1)
        .build()
        .unwrap();
    let chain = CmpMap::new(
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        Box::new(ZoomMap::new(2, 0.5).unwrap()),
        Composition::Series,
    )
    .unwrap();
    let mapped = circle.reg_map_region(&chain, &with_domain("BAR")).unwrap();
    assert_eq!(dump(mapped.as_ref()), cases::IDENTITY_CHAIN);
}

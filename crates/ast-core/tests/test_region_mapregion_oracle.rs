//! `astMapRegion`, protected and public, against the C library: the
//! protected form (region.c `MapRegion`, the port's `reg_map_region`) given
//! an Interval with a BAD limit and a Mapping that drops an axis, which C
//! does not axis-reduce because it has no flags saying which axes give good
//! values; and the public form (region.c `astMapRegionId_`, the port's
//! `map_region`), which simplifies the protected form's result, given a Box
//! and a PermMap that swaps its axes. Also `simplify` of the first case's
//! PermMap, whose inperm C keeps as given (permmap.c `MapMerge`, `NullPerm`).
//!
//! The values come from `tools/oracle_fixtures/region_mapregion_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_mapregion_oracle/cases.rs"]
mod cases;

use ast_core::constants::BAD_VALUE;
use ast_core::frames::{BasicFrame, BasicFrameBuilder};
use ast_core::mappings::{Mapping, PermMap};
use ast_core::regions::{BoxRegionBuilder, IntervalBuilder, Region};
use ast_core::{AnyObject, to_native_string};

fn frame(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

fn dump(m: &dyn Mapping) -> String {
    to_native_string(&AnyObject::from_mapping(m).unwrap()).unwrap()
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

//! Whether the Region attributes Negated, Adaptive and RegionFS are set,
//! against the C library (region.c `TestNegated`, `TestAdaptive` and
//! `TestRegionFS`): after construction, negation, setting, clearing, reading
//! a dump (region.c `Load`, which sets RegionFS and reads Negate and Adapt as
//! set only when the dump has them uncommented) and copying (region.c
//! `Copy`, which clears RegionFS).
//!
//! The values come from `tools/oracle_fixtures/region_attr_test_probe.c`.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_attr_test_oracle/cases.rs"]
mod cases;

use ast_core::frames::BasicFrameBuilder;
use ast_core::regions::{BoxRegionBuilder, DynRegion, Region};
use ast_core::{AnyObject, from_native_string};

fn state(r: &dyn Region) -> [bool; 6] {
    [
        r.is_negated(),
        r.test_negated(),
        r.is_adaptive(),
        r.test_adaptive(),
        r.region_fs_flag(),
        r.test_region_fs(),
    ]
}

fn reread(r: &dyn Region) -> DynRegion {
    let text = native_dump(&AnyObject::from_mapping(r).unwrap()).unwrap();
    from_native_string(&text).unwrap().into_region().unwrap()
}

#[test]
fn set_states_match_the_c_library() {
    let mut bx = BoxRegionBuilder::from_bounds(
        BasicFrameBuilder::new(2).build().unwrap(),
        &[0.0, 0.0],
        &[1.0, 1.0],
    )
    .unwrap()
    .build()
    .unwrap();
    let mut got = Vec::new();
    got.push(("built", state(&bx)));
    bx.negate();
    got.push(("negated", state(&bx)));
    got.push(("negated_reread", state(reread(&bx).as_ref())));
    bx.negate();
    got.push(("negated_twice", state(&bx)));
    got.push(("negated_twice_reread", state(reread(&bx).as_ref())));
    bx.clear_negated();
    bx.set_adaptive(false);
    got.push(("not_adaptive", state(&bx)));
    got.push(("not_adaptive_reread", state(reread(&bx).as_ref())));
    bx.clear_adaptive();
    got.push(("cleared", state(&bx)));
    got.push(("cleared_reread", state(reread(&bx).as_ref())));
    bx.set_region_fs_flag(false);
    got.push(("no_region_fs", state(&bx)));
    let mut copy = bx.clone();
    copy.clear_region_fs_deep();
    got.push(("no_region_fs_copy", state(&copy)));

    let want: Vec<(&str, [bool; 6])> = cases::CASES.to_vec();
    assert_eq!(got, want);
}

/// A Box whose Adaptive attribute is set to 0 dumps `Adapt = 0` uncommented,
/// so it survives a dump and read (region.c `Dump`).
#[test]
fn not_adaptive_dump_matches_the_c_library() {
    let mut bx = BoxRegionBuilder::from_bounds(
        BasicFrameBuilder::new(2).build().unwrap(),
        &[0.0, 0.0],
        &[1.0, 1.0],
    )
    .unwrap()
    .build()
    .unwrap();
    bx.set_adaptive(false);
    let text = native_dump(&AnyObject::from_mapping(&bx).unwrap()).unwrap();
    assert_eq!(text, cases::NOT_ADAPTIVE_DUMP);
}

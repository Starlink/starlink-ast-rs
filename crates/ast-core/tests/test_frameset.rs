//! Rust equivalent of reference/starlink-ast/ast_tester/testframeset.c
//! Covers: construction, add_frame, mapping composition, Frame/Mapping delegation,
//! integrity, remove_frame, remap_frame, set_base/set_current, convert, invert.

#![allow(clippy::approx_constant)] // intentional opaque test values, not π

use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::frames::{BasicFrameBuilder, Frame, FrameRef, FrameSet};
use ast_core::mappings::{DynMapping, Mapping, ShiftMap, UnitMap, ZoomMap};

fn make_frame(naxes: usize) -> Box<dyn Frame> {
    Box::new(BasicFrameBuilder::new(naxes).build().unwrap())
}

fn make_named_frame(naxes: usize, domain: &str) -> Box<dyn Frame> {
    Box::new(
        BasicFrameBuilder::new(naxes)
            .domain(domain)
            .build()
            .unwrap(),
    )
}

// ---- Construction ----

#[test]
fn new_frameset_single_frame() {
    let fs = FrameSet::new(make_frame(2));
    assert_eq!(fs.nframe(), 1);
    assert_eq!(fs.base().naxes(), 2);
    assert_eq!(fs.current().naxes(), 2);
}

#[test]
fn base_and_current_are_same_initially() {
    let fs = FrameSet::new(make_named_frame(3, "PIXEL"));
    assert_eq!(fs.nin(), 3);
    assert_eq!(fs.nout(), 3);
}

// ---- add_frame ----

#[test]
fn add_frame_increases_count() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Current, z, make_frame(2)).unwrap();
    assert_eq!(fs.nframe(), 2);
}

#[test]
fn add_frame_sets_current() {
    let mut fs = FrameSet::new(make_named_frame(2, "PIXEL"));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_named_frame(3, "GRID"))
        .unwrap();
    assert_eq!(fs.current().naxes(), 3);
    assert_eq!(fs.base().naxes(), 2);
}

// ---- Mapping composition ----

#[test]
fn mapping_base_to_current_zoom() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 6.0]);
}

#[test]
fn mapping_current_to_base_inverts() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let m = fs.mapping(FrameRef::Current, FrameRef::Base).unwrap();
    let pts = PointSet::from_rows([[3.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 2.0).abs() < 1e-10);
}

#[test]
fn mapping_same_frame_is_unit() {
    let fs = FrameSet::new(make_frame(2));
    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[7.0, 8.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 8.0]);
}

#[test]
fn mapping_chain_composes() {
    // A --zoom(2)--> B --shift(10,20)--> C
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Current, z, make_frame(2)).unwrap();
    let s: DynMapping = Box::new(ShiftMap::new([10.0, 20.0]));
    fs.add_frame(FrameRef::Current, s, make_frame(2)).unwrap();

    assert_eq!(fs.nframe(), 3);
    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    // zoom: (2, 4), then shift: (12, 24)
    assert_eq!(out.point(0), &[12.0, 24.0]);
}

#[test]
fn mapping_by_index() {
    // A(1) --zoom(2)--> B(2) --shift(10,10)--> C(3)
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Current, z, make_frame(2)).unwrap();
    let s: DynMapping = Box::new(ShiftMap::new([10.0, 10.0]));
    fs.add_frame(FrameRef::Current, s, make_frame(2)).unwrap();

    // Index 1→2: zoom by 2
    let m = fs.mapping(FrameRef::Index(1), FrameRef::Index(2)).unwrap();
    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[10.0, 10.0]);

    // Index 2→3: shift by (10, 10)
    let m = fs.mapping(FrameRef::Index(2), FrameRef::Index(3)).unwrap();
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[15.0, 15.0]);
}

// ---- Tree topology ----

#[test]
fn tree_topology_path_composition() {
    //     B(2)
    //    /
    // A(1)
    //    \
    //     C(3)
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Index(1), z, make_frame(2)).unwrap();
    let s: DynMapping = Box::new(ShiftMap::new([10.0, 10.0]));
    fs.add_frame(FrameRef::Index(1), s, make_frame(2)).unwrap();

    assert_eq!(fs.nframe(), 3);

    // B(2) → A(1) → C(3): invert zoom, then shift
    let m = fs.mapping(FrameRef::Index(2), FrameRef::Index(3)).unwrap();
    let pts = PointSet::from_rows([[4.0, 6.0]]);
    let out = m.transform(&pts, true).unwrap();
    // B→A: /2 → (2, 3), A→C: +10 → (12, 13)
    assert!((out.point(0)[0] - 12.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 13.0).abs() < 1e-10);
}

// ---- FrameSet as Mapping ----

#[test]
fn frameset_transform_forward() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 5.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    assert_eq!(fs.nin(), 2);
    assert_eq!(fs.nout(), 2);
    let pts = PointSet::from_rows([[2.0, 3.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[10.0, 15.0]);
}

#[test]
fn frameset_transform_inverse() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 5.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let pts = PointSet::from_rows([[10.0, 15.0]]);
    let out = fs.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 3.0).abs() < 1e-10);
}

#[test]
fn frameset_transform_multi_point() {
    let mut fs = FrameSet::new(make_frame(2));
    let s: DynMapping = Box::new(ShiftMap::new([1.0, -1.0]));
    fs.add_frame(FrameRef::Base, s, make_frame(2)).unwrap();

    let pts = PointSet::from_rows([[0.0, 0.0], [10.0, 20.0], [100.0, 200.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, -1.0]);
    assert_eq!(out.point(1), &[11.0, 19.0]);
    assert_eq!(out.point(2), &[101.0, 199.0]);
}

// ---- Frame delegation ----

#[test]
fn frame_delegation_naxes() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(3)).unwrap();
    // Frame trait delegates to current (3-axis).
    assert_eq!(fs.naxes(), 3);
}

#[test]
fn frame_delegation_distance() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();
    let d = fs.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

#[test]
fn frame_delegation_format() {
    let mut fs = FrameSet::new(make_frame(1));
    let u: DynMapping = Box::new(UnitMap::new(1));
    fs.add_frame(FrameRef::Base, u, make_frame(1)).unwrap();
    let s = fs.format(1, 3.14).unwrap();
    assert!(s.contains("3.14"));
}

// ---- set_base / set_current ----

#[test]
fn set_current_changes_naxes() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(3)).unwrap();

    assert_eq!(fs.naxes(), 3);
    fs.set_current(FrameRef::Index(1)).unwrap();
    assert_eq!(fs.naxes(), 2);
}

#[test]
fn set_base_changes_nin() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(3)).unwrap();

    assert_eq!(fs.nin(), 2);
    fs.set_base(FrameRef::Current).unwrap();
    assert_eq!(fs.nin(), 3);
}

#[test]
fn set_current_invalid_index() {
    let fs = FrameSet::new(make_frame(2));
    let err = fs.frame(FrameRef::Index(0)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Frmin);
    let err = fs.frame(FrameRef::Index(2)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Frmin);
}

// ---- frame() / frame_mut() ----

#[test]
fn frame_access_by_ref() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(3)).unwrap();

    let f1 = fs.frame(FrameRef::Index(1)).unwrap();
    assert_eq!(f1.naxes(), 2);
    let f2 = fs.frame(FrameRef::Index(2)).unwrap();
    assert_eq!(f2.naxes(), 3);
    let fb = fs.frame(FrameRef::Base).unwrap();
    assert_eq!(fb.naxes(), 2);
    let fc = fs.frame(FrameRef::Current).unwrap();
    assert_eq!(fc.naxes(), 3);
}

#[test]
fn frame_mut_escape_hatch() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let frame = fs.frame_mut(FrameRef::Current).unwrap();
    assert_eq!(frame.naxes(), 2);
}

// ---- Integrity ----

#[test]
fn with_integrity_noop_for_basicframe() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    // BasicFrame's map_from always returns UnitMap, so integrity is a no-op.
    fs.with_integrity(|_frame| Ok(())).unwrap();

    // Mapping should still work.
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[2.0, 4.0]);
}

#[test]
fn with_integrity_error_propagates() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let err = fs.with_integrity(|_frame| {
        Err(ast_core::error::AstError::new(
            ErrorCode::Attin,
            "test error",
        ))
    });
    assert!(err.is_err());
}

// ---- remove_frame ----

#[test]
fn remove_frame_reduces_count() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Current, z, make_frame(2)).unwrap();
    let s: DynMapping = Box::new(ShiftMap::new([10.0, 20.0]));
    fs.add_frame(FrameRef::Current, s, make_frame(2)).unwrap();

    assert_eq!(fs.nframe(), 3);
    fs.remove_frame(FrameRef::Index(2)).unwrap();
    assert_eq!(fs.nframe(), 2);
}

#[test]
fn remove_last_frame_fails() {
    let mut fs = FrameSet::new(make_frame(2));
    let err = fs.remove_frame(FrameRef::Base).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Remin);
}

#[test]
fn remove_current_resets_to_first() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(3)).unwrap();

    assert_eq!(fs.naxes(), 3); // current is the 3-axis frame
    fs.remove_frame(FrameRef::Current).unwrap();
    assert_eq!(fs.nframe(), 1);
    assert_eq!(fs.naxes(), 2); // current reset to first remaining
}

// ---- remap_frame ----

#[test]
fn remap_frame_appends_delta() {
    // Mirrors C `astRemapFrame` semantics: the supplied map is the
    // forward mapping from the OLD coordinates of the named frame to
    // its NEW coordinates. The old base→current mapping survives;
    // the new mapping is then composed on top.
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    // Verify original: zoom by 2.
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[2.0, 4.0]);

    // Remap current with an additional shift. New base→current =
    // old_zoom ∘ shift, so (1, 2) → (2, 4) → (7, 9).
    let s: DynMapping = Box::new(ShiftMap::new([5.0, 5.0]));
    fs.remap_frame(FrameRef::Current, s).unwrap();
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 9.0]);
}

// ---- invert ----

#[test]
fn invert_swaps_direction() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    // Before invert: forward = base→current = *3.
    let pts = PointSet::from_rows([[2.0, 4.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[6.0, 12.0]);

    // After invert: forward = old-current→old-base = /3.
    fs.invert();
    let out = fs.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 2.0 / 3.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 4.0 / 3.0).abs() < 1e-10);
}

#[test]
fn double_invert_is_identity() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 7.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    fs.invert();
    fs.invert();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 14.0]);
}

// ---- simplified ----

#[test]
fn simplified_returns_base_to_current_mapping() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 4.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let m = <dyn Mapping>::simplify(&fs).unwrap();
    let pts = PointSet::from_rows([[2.0, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[8.0, 12.0]);
}

// ---- convert ----

#[test]
fn convert_same_naxes_succeeds() {
    let f1 = BasicFrameBuilder::new(2).build().unwrap();
    let f2 = BasicFrameBuilder::new(2).build().unwrap();
    let result = f1.convert_to(&f2, &[]).unwrap();
    assert!(result.is_some());
    let fs = result.unwrap();
    assert_eq!(fs.nframe(), 2);

    // The mapping should be a UnitMap (identity conversion).
    let pts = PointSet::from_rows([[42.0, 99.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[42.0, 99.0]);
}

#[test]
fn convert_different_naxes_returns_none() {
    let f1 = BasicFrameBuilder::new(2).build().unwrap();
    let f2 = BasicFrameBuilder::new(3).build().unwrap();
    let result = f1.convert_to(&f2, &[]).unwrap();
    assert!(result.is_none());
}

#[test]
fn frameset_convert_delegates_to_current() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let target = BasicFrameBuilder::new(2).build().unwrap();
    let result = fs.convert_to(&target, &[]).unwrap();
    assert!(result.is_some());
}

// ---- add_variant ----

#[test]
fn add_variant_succeeds() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_variant(Some(z), "ZOOMED").unwrap();
    assert!(fs.test_variant());
    assert_eq!(fs.variant(), "ZOOMED");
}

// ---- find_frame (stub) ----

#[test]
fn find_frame_stub_returns_none() {
    let fs = FrameSet::new(make_frame(2));
    let template = BasicFrameBuilder::new(2).build().unwrap();
    let result = fs.find_frame(&template, &["SKY"]).unwrap();
    assert!(result.is_none());
}

// ---- Debug ----

#[test]
fn debug_formatting() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let debug_str = format!("{:?}", fs);
    assert!(debug_str.contains("FrameSet"));
    assert!(debug_str.contains("nframe: 2"));
}

// ---- Edge cases ----

#[test]
fn single_frame_transform_is_identity() {
    let fs = FrameSet::new(make_frame(2));
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = fs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
}

#[test]
fn transform_into_preallocated() {
    let mut fs = FrameSet::new(make_frame(2));
    let s: DynMapping = Box::new(ShiftMap::new([1.0, 2.0]));
    fs.add_frame(FrameRef::Base, s, make_frame(2)).unwrap();

    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let mut out = PointSet::from_rows([[0.0, 0.0]]);
    fs.transform_into(&pts, true, &mut out).unwrap();
    assert_eq!(out.point(0), &[11.0, 22.0]);
}

#[test]
fn rate_on_frameset() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 3.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    // d(output_axis1)/d(input_axis1) of a zoom(3) = 3.0
    let r = fs.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r - 3.0).abs() < 1e-6);

    // Off-diagonal: d(output_axis1)/d(input_axis2) = 0
    let r = fs.rate(&[1.0, 1.0], 1, 2).unwrap();
    assert!(r.abs() < 1e-6);
}

/// Adding a FrameSet re-roots its tree at its current Frame, as C's
/// `astAddFrame` does, reversing the link on the way, which a native dump
/// records as `Inv<n>`; reading the dump back gives the same transformation.
#[test]
fn add_frameset_reverses_links_and_round_trips() {
    use ast_core::AnyObject;
    let grid = BasicFrameBuilder::new(1).domain("GRID").build().unwrap();
    let mut outer = FrameSet::new(Box::new(grid));

    // An inner FrameSet: base A, current B, joined by a shift of 10.
    let a = BasicFrameBuilder::new(1).domain("A").build().unwrap();
    let b = BasicFrameBuilder::new(1).domain("B").build().unwrap();
    let mut inner = FrameSet::new(Box::new(a));
    inner
        .add_frame(
            FrameRef::Base,
            Box::new(ShiftMap::new(vec![10.0])),
            Box::new(b),
        )
        .unwrap();

    // Join GRID to the inner current Frame B by a zoom of 2.
    outer
        .add_frameset(
            FrameRef::Base,
            Box::new(ZoomMap::new(1, 2.0).unwrap()),
            inner,
        )
        .unwrap();
    assert_eq!(outer.nframe(), 3);
    assert_eq!(outer.current().domain(), "B");

    let native = ast_core::to_native_string(&AnyObject::FrameSet(outer.clone())).unwrap();
    assert!(native.contains("Inv2 = 1"), "{native}");
    let AnyObject::FrameSet(back) = ast_core::from_native_string(&native).unwrap() else {
        panic!("expected a FrameSet");
    };

    // GRID 3 -> B 6 -> A -4.
    for fs in [&outer, &back] {
        let map = fs.mapping(FrameRef::Index(1), FrameRef::Index(2)).unwrap();
        let out = map.transform(&PointSet::from_rows([[3.0]]), true).unwrap();
        assert_eq!(out.point(0)[0], -4.0);
    }
}

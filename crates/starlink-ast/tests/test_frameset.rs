//! Rust equivalent of reference/starlink-ast/ast_tester/testframeset.c
//! Covers: construction, add_frame, mapping composition, Frame/Mapping delegation,
//! integrity, remove_frame, remap_frame, set_base/set_current, convert, invert.

#![allow(clippy::approx_constant)] // intentional opaque test values, not π

use crate::common::native_dump;

use starlink_ast::AnyObject;
use starlink_ast::coords::PointSet;
use starlink_ast::error::ErrorCode;
use starlink_ast::frames::FrameSearch;
use starlink_ast::frames::{BasicFrameBuilder, Frame, FrameRef, FrameSet};
use starlink_ast::mappings::{DynMapping, Mapping, MathMap, PermMap, ShiftMap, UnitMap, ZoomMap};

fn make_frame(naxes: usize) -> Box<dyn Frame> {
    Box::new(BasicFrameBuilder::new(naxes).build().unwrap())
}

/// A Mapping from 2 axes to 3, appending a constant third coordinate.
fn two_to_three() -> DynMapping {
    Box::new(PermMap::new(&[1, 2], &[1, 2, -1], &[0.0]).unwrap())
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

/// Frames X, Y and Z, with X to Y a ZoomMap of 2 and Y to Z a ShiftMap of
/// (1, 2).
fn xyz_unnamed() -> FrameSet {
    let mut fs = FrameSet::new(make_named_frame(2, "X"));
    fs.add_frame(
        FrameRef::Index(1),
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        make_named_frame(2, "Y"),
    )
    .unwrap();
    fs.add_frame(
        FrameRef::Index(2),
        Box::new(ShiftMap::new(vec![1.0, 2.0])),
        make_named_frame(2, "Z"),
    )
    .unwrap();
    fs
}

/// [`xyz_unnamed`] with Ident "orig".
fn xyz() -> FrameSet {
    let mut fs = xyz_unnamed();
    Frame::set_ident(&mut fs, "orig").unwrap();
    fs
}

/// A FrameSet made from a FrameSet takes its Frames, Mappings, Base, Current
/// and Invert, and none of its other attributes: the values of a C-oracle
/// probe of `astFrameSet` given a FrameSet.
#[test]
fn new_given_a_frameset_takes_its_frames_and_mappings() {
    let fs = FrameSet::new(Box::new(xyz()));
    assert_eq!(fs.nframe(), 3);
    assert_eq!((fs.base_index(), fs.current_index()), (1, 3));
    assert!(!fs.test_base());
    assert!(fs.test_current());
    assert!(!fs.test_invert());
    assert_eq!(fs.domain(), "Z");
    assert_eq!(Frame::ident(&fs), "");
    assert_eq!(
        native_dump(&AnyObject::FrameSet(fs)).unwrap(),
        native_dump(&AnyObject::FrameSet(xyz_unnamed())).unwrap()
    );

    let mut old = xyz();
    old.set_current(FrameRef::Index(2)).unwrap();
    old.set_base(FrameRef::Index(3)).unwrap();
    let fs = FrameSet::new(Box::new(old));
    assert_eq!((fs.base_index(), fs.current_index()), (3, 2));
    assert!(fs.test_base() && fs.test_current());
    assert_eq!(fs.domain(), "Y");

    let mut old = xyz();
    old.set_invert(false);
    assert!(FrameSet::new(Box::new(old)).test_invert());
}

/// A FrameSet made from a FrameSet takes its Frames' variant Mappings and
/// the Frames that mirror them, as C's clones of the Frames and copy of
/// the mirror indices do.
#[test]
fn new_given_a_frameset_takes_its_variants_and_mirrors() {
    let mut old = xyz_unnamed();
    old.add_variant(Some(Box::new(ShiftMap::new(vec![5.0, 5.0]))), "V2")
        .unwrap();
    old.set_current(FrameRef::Index(2)).unwrap();
    old.mirror_variants(FrameRef::Index(3)).unwrap();
    let expected = native_dump(&AnyObject::FrameSet(old.clone())).unwrap();
    let probe = [1.0, 1.0];
    let before = old
        .mapping(FrameRef::Index(1), FrameRef::Index(3))
        .unwrap()
        .tran2(&probe[..1], &probe[1..], true)
        .unwrap();

    let mut fs = FrameSet::new(Box::new(old));
    assert_eq!(
        native_dump(&AnyObject::FrameSet(fs.clone())).unwrap(),
        expected
    );
    assert_eq!(fs.variant(), "V2");
    assert_eq!(fs.all_variants(), "Z V2");
    assert_eq!(
        fs.mapping(FrameRef::Index(1), FrameRef::Index(3))
            .unwrap()
            .tran2(&probe[..1], &probe[1..], true)
            .unwrap(),
        before
    );

    // Selecting a variant through the mirror changes the mirrored Frame.
    fs.set_variant("Z").unwrap();
    fs.set_current(FrameRef::Index(3)).unwrap();
    assert_eq!(fs.variant(), "Z");
    assert_eq!(
        fs.mapping(FrameRef::Index(1), FrameRef::Index(3))
            .unwrap()
            .tran2(&probe[..1], &probe[1..], true)
            .unwrap(),
        (vec![3.0], vec![4.0])
    );
}

/// An inverted FrameSet given to `FrameSet::new` gives an inverted FrameSet
/// with the same effective Base and Current, dumped as C dumps it.
#[test]
fn new_given_an_inverted_frameset_stays_inverted() {
    let mut old = xyz();
    old.invert();
    let fs = FrameSet::new(Box::new(old));
    assert_eq!(fs.nframe(), 3);
    assert!(fs.is_inverted());
    assert_eq!((fs.base_index(), fs.current_index()), (3, 1));
    assert!(fs.test_base());
    assert!(!fs.test_current());
    assert_eq!(fs.domain(), "X");
    assert_eq!(
        fs.tran2(&[1.0], &[1.0], true).unwrap(),
        (vec![0.0], vec![-0.5])
    );
    assert_eq!(
        native_dump(&AnyObject::FrameSet(fs)).unwrap(),
        include_str!("fixtures/frameset_of_inverted_frameset.native")
    );
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
    let z: DynMapping = two_to_three();
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
    let z: DynMapping = two_to_three();
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
    let z: DynMapping = two_to_three();
    fs.add_frame(FrameRef::Base, z, make_frame(3)).unwrap();

    assert_eq!(fs.naxes(), 3);
    fs.set_current(FrameRef::Index(1)).unwrap();
    assert_eq!(fs.naxes(), 2);
}

#[test]
fn set_base_changes_nin() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = two_to_three();
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
    let z: DynMapping = two_to_three();
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
        Err(starlink_ast::error::AstError::new(
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
fn remove_current_resets_to_default() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = two_to_three();
    fs.add_frame(FrameRef::Base, z, make_frame(3)).unwrap();

    assert_eq!(fs.naxes(), 3); // current is the 3-axis frame
    fs.remove_frame(FrameRef::Current).unwrap();
    assert_eq!(fs.nframe(), 1);
    assert_eq!(fs.naxes(), 2); // current reset to the default, the last Frame
}

/// Removing the current Frame clears Current, so the last Frame becomes
/// current, not the first.
#[test]
fn remove_current_makes_the_last_frame_current() {
    let mut fs = FrameSet::new(make_named_frame(2, "A"));
    for d in ["B", "C"] {
        fs.add_frame(
            FrameRef::Base,
            Box::new(UnitMap::new(2)),
            make_named_frame(2, d),
        )
        .unwrap();
    }
    fs.set_current(FrameRef::Index(2)).unwrap();
    fs.remove_frame(FrameRef::Current).unwrap();
    assert!(!fs.test_current());
    assert_eq!(fs.current().domain(), "C");
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
    let mut f1 = BasicFrameBuilder::new(2).build().unwrap();
    let mut f2 = BasicFrameBuilder::new(2).build().unwrap();
    let result = f1.convert_to(&mut f2, &[]).unwrap();
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
    let mut f1 = BasicFrameBuilder::new(2).build().unwrap();
    let mut f2 = BasicFrameBuilder::new(3).build().unwrap();
    let result = f1.convert_to(&mut f2, &[]).unwrap();
    assert!(result.is_none());
}

#[test]
fn frameset_convert_delegates_to_current() {
    let mut fs = FrameSet::new(make_frame(2));
    let z: DynMapping = Box::new(ZoomMap::new(2, 2.0).unwrap());
    fs.add_frame(FrameRef::Base, z, make_frame(2)).unwrap();

    let mut target = BasicFrameBuilder::new(2).build().unwrap();
    let result = fs.convert_to(&mut target, &[]).unwrap();
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
    let mut fs = FrameSet::new(make_frame(2));
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
    fs.transform_into(pts.as_points(), true, out.as_points_mut())
        .unwrap();
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
    use starlink_ast::AnyObject;
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

    let native = native_dump(&AnyObject::FrameSet(outer.clone())).unwrap();
    assert!(native.contains("Inv2 = 1"), "{native}");
    let AnyObject::FrameSet(back) = starlink_ast::from_native_string(&native).unwrap() else {
        panic!("expected a FrameSet");
    };

    // GRID 3 -> B 6 -> A -4.
    for fs in [&outer, &back] {
        let map = fs.mapping(FrameRef::Index(1), FrameRef::Index(2)).unwrap();
        let out = map.transform(&PointSet::from_rows([[3.0]]), true).unwrap();
        assert_eq!(out.point(0)[0], -4.0);
    }
}

/// `add_frame` rejects a Mapping whose Nin is not the Naxes of the anchor
/// Frame, or whose Nout is not the Naxes of the new Frame, with AST__NCPIN
/// and leaves the FrameSet unchanged, as C's `astAddFrame` does.
#[test]
fn add_frame_rejects_a_mapping_of_the_wrong_size() {
    let mut fs = FrameSet::new(make_named_frame(2, "PIX"));
    let err = fs
        .add_frame(
            FrameRef::Base,
            Box::new(ZoomMap::new(3, 2.0).unwrap()),
            make_frame(3),
        )
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Ncpin);
    let err = fs
        .add_frame(
            FrameRef::Base,
            Box::new(ZoomMap::new(2, 2.0).unwrap()),
            make_frame(3),
        )
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Ncpin);
    assert_eq!(fs.nframe(), 1);
}

/// Adding a FrameSet as a Frame merges its Frames, linking the anchor to
/// its current Frame, and a FrameSet given as the Mapping is used as the
/// Mapping from its base Frame to its current Frame, as C's `astAddFrame`
/// does.
#[test]
fn add_frame_with_a_frameset_merges_it() {
    let mut a = FrameSet::new(make_named_frame(2, "PIX"));
    a.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 2.0).unwrap()),
        make_named_frame(2, "SKYX"),
    )
    .unwrap();
    let mut c = FrameSet::new(make_named_frame(2, "GRID"));
    c.add_frame(
        FrameRef::Base,
        Box::new(ShiftMap::new([1.0, 1.0])),
        Box::new(a),
    )
    .unwrap();
    assert_eq!(c.nframe(), 3);
    // The added FrameSet's current Frame, SKYX, is Frame 3 and current, and
    // the ShiftMap links GRID to it rather than to PIX.
    assert_eq!(c.frame(FrameRef::Index(3)).unwrap().domain(), "SKYX");
    assert_eq!(c.current().domain(), "SKYX");
    let out = c
        .transform(&PointSet::from_rows([[1.0, 1.0]]), true)
        .unwrap();
    assert_eq!(out.point(0), [2.0, 2.0]);
    assert_eq!(map11(&c, 1, 2), (1.0, 1.0));

    let mut m = FrameSet::new(make_frame(2));
    m.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 3.0).unwrap()),
        make_frame(2),
    )
    .unwrap();
    let mut d = FrameSet::new(make_named_frame(2, "GRID"));
    d.add_frame(FrameRef::Base, Box::new(m), make_named_frame(2, "Q"))
        .unwrap();
    let map = d.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    assert!(!map.as_ref().as_any().is::<FrameSet>());
    assert_eq!(
        map.tran2(&[1.0], &[1.0], true).unwrap(),
        (vec![3.0], vec![3.0])
    );
}

/// The point (1,1) mapped from Frame `a` to Frame `b`.
fn map11(fs: &FrameSet, a: usize, b: usize) -> (f64, f64) {
    let m = fs.mapping(FrameRef::Index(a), FrameRef::Index(b)).unwrap();
    let (x, y) = m.tran2(&[1.0], &[1.0], true).unwrap();
    (x[0], y[0])
}

/// C's dump of an inverted FrameSet (`r6c/inv.c`): A -ZoomMap 2-> B, then
/// `astInvert`. C keeps Base 1 and Current 2 and writes `Invert = 1`.
const C_INVERTED_FRAMESET: &str = concat!(
    " Begin FrameSet \t# Set of inter-related coordinate systems\n",
    "    Invert = 1 \t# Mapping inverted\n",
    " IsA Mapping \t# Mapping between coordinate systems\n",
    "#   Title = \"1-d coordinate system\" \t# Title of coordinate system\n",
    "#   Naxes = 1 \t# Number of coordinate axes\n",
    "#   Domain = \"A\" \t# Coordinate system domain\n",
    "#   Lbl1 = \"Axis 1\" \t# Label for axis 1\n",
    " IsA Frame \t# Coordinate system description\n",
    "    Nframe = 2 \t# Number of Frames in FrameSet\n",
    "#   Base = 1 \t# Index of base Frame\n",
    "    Currnt = 2 \t# Index of current Frame\n",
    "    Lnk2 = 1 \t# Node 2 is derived from node 1\n",
    "    Frm1 = \t# Frame number 1\n",
    "       Begin Frame \t# Coordinate system description\n",
    "#         Title = \"1-d coordinate system\" \t# Title of coordinate system\n",
    "          Naxes = 1 \t# Number of coordinate axes\n",
    "          Domain = \"A\" \t# Coordinate system domain\n",
    "#         Lbl1 = \"Axis 1\" \t# Label for axis 1\n",
    "          Ax1 = \t# Axis number 1\n",
    "             Begin Axis \t# Coordinate axis\n",
    "             End Axis\n",
    "       End Frame\n",
    "    Frm2 = \t# Frame number 2\n",
    "       Begin Frame \t# Coordinate system description\n",
    "#         Title = \"1-d coordinate system\" \t# Title of coordinate system\n",
    "          Naxes = 1 \t# Number of coordinate axes\n",
    "          Domain = \"B\" \t# Coordinate system domain\n",
    "#         Lbl1 = \"Axis 1\" \t# Label for axis 1\n",
    "          Ax1 = \t# Axis number 1\n",
    "             Begin Axis \t# Coordinate axis\n",
    "             End Axis\n",
    "       End Frame\n",
    "    Map2 = \t# Mapping between nodes 1 and 2\n",
    "       Begin ZoomMap \t# Zoom about the origin\n",
    "          Nin = 1 \t# Number of input coordinates\n",
    "       IsA Mapping \t# Mapping between coordinate systems\n",
    "          Zoom = 2 \t# Zoom factor\n",
    "       End ZoomMap\n",
    " End FrameSet\n",
);

/// A FrameSet read with `Invert = 1` is inverted, as C's, which maps 3 to
/// 1.5, and a FrameSet inverted here is dumped as C dumps it.
#[test]
fn an_inverted_frameset_reads_and_writes_as_c_does() {
    use starlink_ast::AnyObject;
    let AnyObject::FrameSet(back) = starlink_ast::from_native_string(C_INVERTED_FRAMESET).unwrap()
    else {
        panic!("expected a FrameSet");
    };
    assert!(back.is_inverted());
    assert_eq!(back.base().domain(), "B");
    assert_eq!(back.current().domain(), "A");
    assert_eq!(back.tran1(&[3.0], true).unwrap(), vec![1.5]);

    let mut fs = FrameSet::new(make_named_frame(1, "A"));
    fs.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(1, 2.0).unwrap()),
        make_named_frame(1, "B"),
    )
    .unwrap();
    fs.invert();
    assert!(fs.is_inverted());
    assert_eq!(
        native_dump(&AnyObject::FrameSet(fs)).unwrap(),
        C_INVERTED_FRAMESET
    );
}

// ---- Default Base and Current of an inverted FrameSet ----
//
// The expected values are those of a C-oracle probe: an inverted FrameSet's
// unset Current defaults to the first Frame and its unset Base to the last,
// and the defaults track the Frames as they are added or removed.

/// Frames A, B and C, with A to B a ZoomMap of 2 and B to C one of 3.
fn three_domains() -> FrameSet {
    let mut fs = FrameSet::new(make_named_frame(1, "A"));
    fs.add_frame(
        FrameRef::Current,
        Box::new(ZoomMap::new(1, 2.0).unwrap()),
        make_named_frame(1, "B"),
    )
    .unwrap();
    fs.add_frame(
        FrameRef::Current,
        Box::new(ZoomMap::new(1, 3.0).unwrap()),
        make_named_frame(1, "C"),
    )
    .unwrap();
    fs
}

/// Base and current Domains, whether Base and Current are set, and where the
/// FrameSet maps 1.
fn base_current(fs: &FrameSet) -> (String, String, bool, bool, f64) {
    (
        fs.base().domain().to_string(),
        fs.current().domain().to_string(),
        fs.test_base(),
        fs.test_current(),
        fs.tran1(&[1.0], true).unwrap()[0],
    )
}

/// Check `fs` against `want`, before and after a Native dump and reload.
fn assert_base_current(fs: &FrameSet, want: (&str, &str, bool, bool, f64)) {
    use starlink_ast::AnyObject;
    let want = (
        want.0.to_string(),
        want.1.to_string(),
        want.2,
        want.3,
        want.4,
    );
    let check = |fs: &FrameSet, when: &str| {
        let got = base_current(fs);
        assert_eq!(
            (&got.0, &got.1, got.2, got.3),
            (&want.0, &want.1, want.2, want.3),
            "{when}"
        );
        assert!(
            (got.4 - want.4).abs() < 1e-12,
            "{when}: tran1 {} != {}",
            got.4,
            want.4
        );
    };
    check(fs, "in memory");
    let text = native_dump(&AnyObject::FrameSet(fs.clone())).unwrap();
    let AnyObject::FrameSet(back) = starlink_ast::from_native_string(&text).unwrap() else {
        panic!("expected a FrameSet");
    };
    check(&back, "after a dump and reload");
}

#[test]
fn inverted_clear_current_defaults_to_the_first_frame() {
    let mut fs = three_domains();
    fs.invert();
    assert_base_current(&fs, ("C", "A", true, false, 1.0 / 6.0));
    fs.clear_current();
    assert_base_current(&fs, ("C", "A", true, false, 1.0 / 6.0));

    let mut fs = three_domains();
    fs.set_base(FrameRef::Index(2)).unwrap();
    fs.invert();
    assert_base_current(&fs, ("C", "B", true, true, 1.0 / 3.0));
    fs.clear_current();
    assert_base_current(&fs, ("C", "A", true, false, 1.0 / 6.0));
}

#[test]
fn inverted_clear_base_defaults_to_the_last_frame() {
    let mut fs = three_domains();
    fs.invert();
    fs.clear_base();
    assert_base_current(&fs, ("C", "A", false, false, 1.0 / 6.0));
}

#[test]
fn inverted_remove_frame_keeps_the_default_current_first() {
    let mut fs = three_domains();
    fs.invert();
    fs.remove_frame(FrameRef::Index(2)).unwrap();
    assert_base_current(&fs, ("C", "A", true, false, 1.0 / 6.0));

    let mut fs = three_domains();
    fs.invert();
    fs.remove_frame(FrameRef::Index(1)).unwrap();
    assert_base_current(&fs, ("C", "B", true, false, 1.0 / 3.0));
}

#[test]
fn inverted_remove_frame_moves_the_default_base_to_the_last_frame() {
    let mut fs = three_domains();
    fs.invert();
    fs.remove_frame(FrameRef::Index(3)).unwrap();
    assert_base_current(&fs, ("B", "A", false, false, 0.5));

    let mut fs = three_domains();
    fs.invert();
    fs.clear_base();
    fs.set_current(FrameRef::Index(2)).unwrap();
    fs.remove_frame(FrameRef::Index(1)).unwrap();
    assert_base_current(&fs, ("C", "B", false, true, 1.0 / 3.0));

    let mut fs = three_domains();
    fs.invert();
    fs.clear_base();
    fs.set_current(FrameRef::Index(2)).unwrap();
    fs.remove_frame(FrameRef::Index(3)).unwrap();
    assert_base_current(&fs, ("B", "B", false, true, 1.0));
}

#[test]
fn inverted_add_frame_moves_the_default_base_to_the_new_frame() {
    let mut fs = three_domains();
    fs.invert();
    fs.clear_base();
    fs.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(1, 5.0).unwrap()),
        make_named_frame(1, "D"),
    )
    .unwrap();
    assert_base_current(&fs, ("D", "D", false, true, 1.0));

    let mut fs = three_domains();
    fs.invert();
    fs.clear_base();
    fs.clear_current();
    fs.add_frame(
        FrameRef::Index(2),
        Box::new(ZoomMap::new(1, 5.0).unwrap()),
        make_named_frame(1, "D"),
    )
    .unwrap();
    assert_base_current(&fs, ("D", "D", false, true, 1.0));
}

// ---- Base and Current indices, and selection by Domain ----

/// Frames with Domains X, Y, Y and Z, each added from the first through a
/// ZoomMap of factor 2, 3 and 5.
fn four_domains() -> FrameSet {
    let mut fs = FrameSet::new(make_named_frame(1, "X"));
    for (zoom, domain) in [(2.0, "Y"), (3.0, "Y"), (5.0, "Z")] {
        fs.add_frame(
            FrameRef::Index(1),
            Box::new(ZoomMap::new(1, zoom).unwrap()),
            make_named_frame(1, domain),
        )
        .unwrap();
    }
    fs
}

// C probe (astGetI "Base"/"Current", astSet "Base= y  " etc.) on these
// Frames: unchanged FrameSet 1/4, then Base=2; inverted 4/1, then Base=2 and
// Current=4.
#[test]
fn base_and_current_indices() {
    let mut fs = four_domains();
    assert_eq!((fs.base_index(), fs.current_index()), (1, 4));
    fs.set_current(FrameRef::Index(2)).unwrap();
    assert_eq!(fs.current_index(), 2);
    fs.invert();
    assert_eq!((fs.base_index(), fs.current_index()), (2, 1));
}

#[test]
fn set_base_and_current_by_domain() {
    let mut fs = four_domains();
    // The first Frame with the Domain is taken, ignoring case, leading and
    // trailing white space.
    fs.set_base_by_domain(" y  ").unwrap();
    assert_eq!((fs.base_index(), fs.current_index()), (2, 4));
    fs.set_current_by_domain("x").unwrap();
    assert_eq!(fs.current_index(), 1);

    let mut inv = four_domains();
    inv.invert();
    assert_eq!((inv.base_index(), inv.current_index()), (4, 1));
    inv.set_base_by_domain("Y").unwrap();
    assert_eq!((inv.base_index(), inv.current_index()), (2, 1));
    inv.set_current_by_domain("z").unwrap();
    assert_eq!((inv.base_index(), inv.current_index()), (2, 4));
}

#[test]
fn set_by_unknown_domain_is_attin() {
    let mut fs = four_domains();
    let err = fs.set_base_by_domain("Q").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Attin);
    let err = fs.set_current_by_domain("w").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Attin);
    assert_eq!((fs.base_index(), fs.current_index()), (1, 4));
}

/// The value `cvt` maps 1.0 to.
fn tran_one(cvt: &FrameSet) -> f64 {
    let input = PointSet::from_rows([[1.0]]);
    cvt.transform(&input, true).unwrap().point(0)[0]
}

// C probe of astConvert with this FrameSet inverted: its Frames 2 and 3 both
// have Domain Y, and C takes an inverted FrameSet's other Frames in
// descending order, so Frame 3 is used each way round (3 and 1/3).
#[test]
fn convert_takes_an_inverted_framesets_other_frames_in_descending_order() {
    let mut y = BasicFrameBuilder::new(1).domain("Y").build().unwrap();
    let mut fs = four_domains();
    fs.invert();
    let cvt = fs.convert_to(&mut y, &[]).unwrap().unwrap();
    assert!((tran_one(&cvt) - 3.0).abs() < 1e-15);
    let mut fs = four_domains();
    fs.invert();
    let cvt = y.convert_to(&mut fs, &[]).unwrap().unwrap();
    assert!((tran_one(&cvt) - 1.0 / 3.0).abs() < 1e-15);

    // Uninverted, the ascending order reaches Frame 2 first (C: 0.4, 2.5).
    let mut fs = four_domains();
    let cvt = fs.convert_to(&mut y, &[]).unwrap().unwrap();
    assert!((tran_one(&cvt) - 0.4).abs() < 1e-15);
    let mut fs = four_domains();
    let cvt = y.convert_to(&mut fs, &[]).unwrap().unwrap();
    assert!((tran_one(&cvt) - 2.5).abs() < 1e-15);
}

// C probe: astConvert sets the Base of a FrameSet argument, either way
// round, to the Frame used (2 unchanged, 3 inverted), and leaves Current
// alone.
#[test]
fn convert_sets_the_base_of_a_frameset_argument_to_the_frame_used() {
    let mut y = BasicFrameBuilder::new(1).domain("Y").build().unwrap();
    for (inverted, base, current) in [(false, 2, 4), (true, 3, 1)] {
        let mut from = four_domains();
        let mut to = four_domains();
        if inverted {
            from.invert();
            to.invert();
        }
        from.convert_to(&mut y, &[]).unwrap().unwrap();
        assert_eq!((from.base_index(), from.current_index()), (base, current));
        y.convert_to(&mut to, &[]).unwrap().unwrap();
        assert_eq!((to.base_index(), to.current_index()), (base, current));
    }
}

#[test]
fn failed_convert_leaves_the_base_alone() {
    let mut q = BasicFrameBuilder::new(1).domain("Q").build().unwrap();
    let mut fs = four_domains();
    assert!(fs.convert_to(&mut q, &[]).unwrap().is_none());
    assert!(!fs.test_base());
    assert_eq!(fs.base_index(), 1);
}

// C probe: astFindFrame with domain list "Y" makes the Frame matched the
// current Frame (2 both ways; the search ascends whether or not the
// FrameSet is inverted), and the Mapping runs from the base Frame to it
// (2 unchanged; 2/5 inverted, from Frame 4).
#[test]
fn find_frame_makes_the_frame_matched_current() {
    let template = BasicFrameBuilder::new(1).build().unwrap();
    for (inverted, base, value) in [(false, 1, 2.0), (true, 4, 0.4)] {
        let mut fs = four_domains();
        if inverted {
            fs.invert();
        }
        let found = fs.find_frame(&template, &["Y"]).unwrap().unwrap();
        assert_eq!((fs.base_index(), fs.current_index()), (base, 2));
        assert!((tran_one(&found) - value).abs() < 1e-15);

        let mut fs = four_domains();
        if inverted {
            fs.invert();
        }
        fs.find_frame_template(&template, "Y").unwrap().unwrap();
        assert_eq!(fs.current_index(), 2);

        // No match leaves the Current alone.
        assert!(fs.find_frame(&template, &["Q"]).unwrap().is_none());
        assert_eq!(fs.current_index(), 2);
    }
}

/// `from`: Domain X (base and current), then A from X by a forward-only
/// y = 2x, then B from X by a ZoomMap of 3.
/// `to`: Domain A (base and current), then B from A by a UnitMap.
fn first_domain_pair() -> (FrameSet, FrameSet) {
    let mut from = FrameSet::new(make_named_frame(1, "X"));
    from.add_frame(
        FrameRef::Index(1),
        Box::new(MathMap::new(1, 1, &["y=2*x"], &["x"]).unwrap()),
        make_named_frame(1, "A"),
    )
    .unwrap();
    from.add_frame(
        FrameRef::Index(1),
        Box::new(ZoomMap::new(1, 3.0).unwrap()),
        make_named_frame(1, "B"),
    )
    .unwrap();
    from.set_current(FrameRef::Index(1)).unwrap();
    let mut to = FrameSet::new(make_named_frame(1, "A"));
    to.add_frame(
        FrameRef::Index(1),
        Box::new(UnitMap::new(1)),
        make_named_frame(1, "B"),
    )
    .unwrap();
    to.set_current(FrameRef::Index(1)).unwrap();
    (from, to)
}

// C probe of astConvert with domains "A,B": the first field gives a
// forward-only conversion through Domain A and the search stops there
// (from Base 2, to Base 1, maps 1 to 2), although "B" alone would give one
// with both transformations (from Base 3, to Base 2, maps 1 to 3).
#[test]
fn convert_stops_at_the_first_domain_that_gives_a_conversion() {
    let (mut from, mut to) = first_domain_pair();
    let cvt = from.convert_to(&mut to, &["A", "B"]).unwrap().unwrap();
    assert_eq!((from.base_index(), to.base_index()), (2, 1));
    assert!(cvt.has_forward() && !cvt.has_inverse());
    assert!((tran_one(&cvt) - 2.0).abs() < 1e-15);

    let (mut from, mut to) = first_domain_pair();
    let cvt = from.convert_to(&mut to, &["B", "A"]).unwrap().unwrap();
    assert_eq!((from.base_index(), to.base_index()), (3, 2));
    assert!((tran_one(&cvt) - 3.0).abs() < 1e-15);
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Tests for FrameSet behaviors ported from frameset.c: RemoveFrame
//! node preservation, Base/Current reset defaults, current-frame
//! delegation of Frame methods, Simplify returning a FrameSet, and
//! transform-definedness via the base→current path.

use starlink_ast::coords::PointSet;
use starlink_ast::frames::FrameSearch;
use starlink_ast::frames::{
    BasicFrameBuilder, DynFrame, Frame, FrameRef, FrameSet, SkyFrame, SkySystem,
};
use starlink_ast::mappings::{Mapping, PolyMap, PolyTerm, ShiftMap, UnitMap, ZoomMap};

fn frame_with_domain(naxes: usize, domain: &str) -> DynFrame {
    Box::new(
        BasicFrameBuilder::new(naxes)
            .domain(domain)
            .build()
            .unwrap(),
    )
}

/// F1 -(Zoom ×2)- F2 -(Shift +1)- F3, all 1-axis.
fn three_frame_chain() -> FrameSet {
    let mut fs = FrameSet::new(frame_with_domain(1, "FIRST"));
    fs.add_frame(
        FrameRef::Index(1),
        Box::new(ZoomMap::new(1, 2.0).unwrap()),
        frame_with_domain(1, "SECOND"),
    )
    .unwrap();
    fs.add_frame(
        FrameRef::Index(2),
        Box::new(ShiftMap::new(vec![1.0])),
        frame_with_domain(1, "THIRD"),
    )
    .unwrap();
    fs
}

// ===========================================================================
// RemoveFrame: graph connectivity and Base/Current reset
// ===========================================================================

#[test]
fn remove_middle_frame_preserves_connectivity() {
    // C RemoveFrame (frameset.c:8740) drops only the frame's node
    // association; TidyNodes preserves the conversion path between the
    // remaining frames by composing the bridged mappings.
    let mut fs = three_frame_chain();
    fs.remove_frame(FrameRef::Index(2)).unwrap();
    assert_eq!(fs.nframe(), 2);

    // FIRST → THIRD must still be x*2 + 1.
    let map = fs.mapping(FrameRef::Index(1), FrameRef::Index(2)).unwrap();
    let pts = PointSet::from_columns(&[&[3.0_f64]]).unwrap();
    let out = map.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 7.0).abs() < 1e-12,
        "got {}",
        out.point(0)[0]
    );
}

#[test]
fn remove_leaf_frame_keeps_remaining_chain() {
    let mut fs = three_frame_chain();
    fs.remove_frame(FrameRef::Index(3)).unwrap();
    assert_eq!(fs.nframe(), 2);
    let map = fs.mapping(FrameRef::Index(1), FrameRef::Index(2)).unwrap();
    let pts = PointSet::from_columns(&[&[3.0_f64]]).unwrap();
    let out = map.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
}

#[test]
fn remove_current_frame_resets_current_to_last() {
    // C clears the Current attribute when the current frame is removed,
    // and the Current default is the *last* frame (frameset.c:5091).
    let mut fs = three_frame_chain();
    fs.set_current(FrameRef::Index(2)).unwrap();
    fs.remove_frame(FrameRef::Index(2)).unwrap();
    assert_eq!(Frame::domain(&fs), "THIRD");
}

#[test]
fn remove_frame_above_current_keeps_current() {
    // Removing a frame with a higher index must not disturb which
    // frame is current.
    let mut fs = three_frame_chain();
    fs.set_current(FrameRef::Index(2)).unwrap();
    fs.remove_frame(FrameRef::Index(3)).unwrap();
    assert_eq!(Frame::domain(&fs), "SECOND");
}

// ===========================================================================
// Frame-method delegation to the current frame
// ===========================================================================

#[test]
fn frameset_delegates_domain_but_owns_ident() {
    let mut sky = SkyFrame::new(SkySystem::Icrs);
    sky.set_ident("Q");
    let mut fs = FrameSet::new(frame_with_domain(2, "PIXEL"));
    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(sky))
        .unwrap();

    // C forwards GetDomain to the current frame.
    assert_eq!(Frame::domain(&fs), "SKY");

    // Ident is the FrameSet's own, not the current frame's
    // (frameset.c:4841 passes `this`).
    assert_eq!(Frame::ident(&fs), "");
    Frame::set_ident(&mut fs, "FS").unwrap();
    assert_eq!(Frame::ident(&fs), "FS");
    assert_eq!(fs.frame(FrameRef::Current).unwrap().ident(), "Q");

    fs.set_current(FrameRef::Base).unwrap();
    assert_eq!(Frame::domain(&fs), "PIXEL");
    assert_eq!(Frame::ident(&fs), "FS");
}

#[test]
fn frameset_delegates_match_control_attributes() {
    let mut fs = FrameSet::new(frame_with_domain(3, "PIXEL"));
    assert_eq!(Frame::min_axes(&fs), 3);

    // Setting through the FrameSet must write through to the current
    // frame (C forwards SetMinAxes etc.).
    Frame::set_min_axes(&mut fs, 5).unwrap();
    assert_eq!(Frame::min_axes(&fs), 5);
    assert_eq!(fs.frame(FrameRef::Current).unwrap().min_axes(), 5);
}

#[test]
fn frameset_template_matches_like_current_frame() {
    // A FrameSet whose current frame is a SkyFrame must match as a
    // SkyFrame template (C forwards astMatch to the current frame):
    // it matches a SkyFrame target but NOT a basic Frame target.
    let mut fs = FrameSet::new(frame_with_domain(2, "PIXEL"));
    fs.add_frame(
        FrameRef::Base,
        Box::new(UnitMap::new(2)),
        Box::new(SkyFrame::new(SkySystem::Icrs)),
    )
    .unwrap();

    let sky_target = SkyFrame::new(SkySystem::Icrs);
    let m = Frame::match_template(&fs, &sky_target, false).unwrap();
    assert!(m.is_some(), "FrameSet(SkyFrame) should match a SkyFrame");

    let basic_target = BasicFrameBuilder::new(2).build().unwrap();
    let m = Frame::match_template(&fs, &basic_target, false).unwrap();
    assert!(
        m.is_none(),
        "FrameSet(SkyFrame) must not match a basic Frame target"
    );
}

// ===========================================================================
// Simplify returns a FrameSet
// ===========================================================================

#[test]
fn frameset_simplified_returns_frameset() {
    // C FrameSet::Simplify (frameset.c:10111) returns a *FrameSet*
    // copy with each internal mapping simplified.
    let mut z1: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let mut fs = FrameSet::new(frame_with_domain(1, "A"));
    // Edge: Zoom(2) then Zoom(2) inverted == UnitMap after simplify.
    let mut z2: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    z2.invert();
    let cmp = starlink_ast::mappings::CmpMap::new(
        std::mem::replace(&mut z1, Box::new(UnitMap::new(1))),
        z2,
        starlink_ast::mappings::Composition::Series,
    )
    .unwrap();
    fs.add_frame(FrameRef::Base, Box::new(cmp), frame_with_domain(1, "B"))
        .unwrap();

    let simp = <dyn Mapping>::simplify(&fs).unwrap();
    let sfs = simp
        .as_any()
        .downcast_ref::<FrameSet>()
        .expect("astSimplify of a FrameSet must return a FrameSet");
    assert_eq!(sfs.nframe(), 2);

    // The simplified FrameSet's internal edge must have collapsed.
    let map = sfs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    assert!(
        map.as_any().is::<UnitMap>(),
        "internal edge should simplify to UnitMap, got {map:?}"
    );
}

// ===========================================================================
// Transform definedness follows the base→current path
// ===========================================================================

#[test]
fn frameset_definedness_follows_path() {
    // A forward-only mapping in the path must make has_inverse false
    // (C GetTranInverse forwards to the base→current mapping).
    let fwd = vec![PolyTerm {
        coeff: 1.0,
        powers: vec![2],
        out_axis: 1,
    }];
    let mut pm = PolyMap::new(1, 1, &fwd, None).unwrap();
    pm.set_iter_inverse(false).unwrap();

    let mut fs = FrameSet::new(frame_with_domain(1, "A"));
    fs.add_frame(FrameRef::Base, Box::new(pm), frame_with_domain(1, "B"))
        .unwrap();
    assert!(Mapping::has_forward(&fs));
    assert!(!Mapping::has_inverse(&fs));
}

// ---------------------------------------------------------------------------
// Multi-member-frame convert (FrameSet::ConvertX, frameset.c:3201)
// ---------------------------------------------------------------------------

/// `astConvert` from a FrameSet must try every member frame, not just the
/// current one. Here the current frame (PIXEL) cannot align with a Galactic
/// SkyFrame, but the non-current base member (FK5 SkyFrame) can. The old
/// "forward to current frame" behavior returned None; the ConvertX port
/// aligns via the base member.
///
/// Oracle-verified against the reference C library: `astConvert` of the same
/// FrameSet (base=FK5 SkyFrame, current=PIXEL, linked by UnitMap) to a
/// Galactic SkyFrame yields a both-directions Mapping that maps (0.1, 0.2)
/// rad to (1.951902536797, -0.886577028810) rad — identical to Rust.
#[test]
fn convert_via_noncurrent_member_frame() {
    let fk5 = SkyFrame::new(SkySystem::Fk5);
    let mut fs = FrameSet::new(Box::new(fk5));
    let pixel = BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap();
    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(pixel))
        .unwrap();
    // current == PIXEL, base == FK5 SkyFrame.

    let mut gal = SkyFrame::new(SkySystem::Galactic);
    let out = fs
        .convert_to(&mut gal, &[])
        .unwrap()
        .expect("FrameSet should convert to Galactic via its FK5 base member");

    let m = out.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    assert!(Mapping::has_forward(m.as_ref()));
    assert!(Mapping::has_inverse(m.as_ref()));

    let pts = PointSet::from_rows([[0.1, 0.2]]);
    let res = m.transform(&pts, true).unwrap();
    assert!((res.point(0)[0] - 1.951_902_536_797).abs() < 1e-9);
    assert!((res.point(0)[1] - (-0.886_577_028_810)).abs() < 1e-9);
}

/// When the current frame *can* align directly, ConvertX still returns a
/// correct result (the current→current route scores a perfect 3 and wins).
#[test]
fn convert_via_current_frame_still_works() {
    // FrameSet whose current frame is an FK5 SkyFrame.
    let pixel = BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap();
    let mut fs = FrameSet::new(Box::new(pixel));
    fs.add_frame(
        FrameRef::Base,
        Box::new(UnitMap::new(2)),
        Box::new(SkyFrame::new(SkySystem::Fk5)),
    )
    .unwrap();
    // current == FK5 SkyFrame.

    let mut gal = SkyFrame::new(SkySystem::Galactic);
    let out = fs
        .convert_to(&mut gal, &[])
        .unwrap()
        .expect("current FK5 frame converts to Galactic");
    let m = out.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    assert!(Mapping::has_forward(m.as_ref()) && Mapping::has_inverse(m.as_ref()));
}

// ---------------------------------------------------------------------------
// add_frame(AST__ALLFRAMES) — append axes to every frame (frameset.c:1336)
// ---------------------------------------------------------------------------

/// `add_frame(FrameRef::AllFrames, _, frame)` appends the supplied frame's
/// axes to every frame in the set: each Frame becomes a CmpFrame(orig, frame)
/// and each Mapping a parallel CmpMap(orig, UnitMap). The Mapping argument is
/// ignored. Oracle-verified against the reference C library: a 2-axis
/// GRID→FPLANE WinMap FrameSet widened by a 1-axis SPEC frame becomes a 3-axis
/// set mapping (5,5,42) → (40,40,42) (axes 1-2 through the WinMap, axis 3
/// passthrough).
#[test]
fn add_frame_allframes_appends_axes() {
    use starlink_ast::mappings::WinMap;

    let wm = WinMap::from_corners(&[1.0, 1.0], &[10.0, 10.0], &[0.0, 0.0], &[90.0, 90.0]).unwrap();
    let mut fs = FrameSet::new(frame_with_domain(2, "GRID"));
    fs.add_frame(
        FrameRef::Current,
        Box::new(wm),
        frame_with_domain(2, "FPLANE"),
    )
    .unwrap();

    // The Mapping argument is ignored in AllFrames mode.
    fs.add_frame(
        FrameRef::AllFrames,
        Box::new(UnitMap::new(1)),
        frame_with_domain(1, "SPEC"),
    )
    .unwrap();

    assert_eq!(fs.nframe(), 2);
    assert_eq!(Mapping::nin(&fs), 3);
    assert_eq!(Mapping::nout(&fs), 3);
    assert_eq!(fs.frame(FrameRef::Base).unwrap().naxes(), 3);
    assert_eq!(fs.frame(FrameRef::Current).unwrap().naxes(), 3);

    let out = Mapping::transform(&fs, &PointSet::from_rows([[5.0, 5.0, 42.0]]), true).unwrap();
    assert!((out.point(0)[0] - 40.0).abs() < 1e-9);
    assert!((out.point(0)[1] - 40.0).abs() < 1e-9);
    assert!((out.point(0)[2] - 42.0).abs() < 1e-9);
}

/// `FrameRef::AllFrames` is rejected where a single frame is required.
#[test]
fn allframes_rejected_as_single_frame() {
    let fs = three_frame_chain();
    assert!(fs.frame(FrameRef::AllFrames).is_err());
}

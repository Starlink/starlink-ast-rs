//! Tests for FrameSet behaviours ported from frameset.c: RemoveFrame
//! node preservation, Base/Current reset defaults, current-frame
//! delegation of Frame methods, Simplify returning a FrameSet, and
//! transform-definedness via the base→current path.

use ast_core::coords::PointSet;
use ast_core::frames::{
    BasicFrameBuilder, DynFrame, Frame, FrameRef, FrameSet, SkyFrame, SkySystem,
};
use ast_core::mappings::{Mapping, PolyMap, PolyTerm, ShiftMap, UnitMap, ZoomMap};

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
fn frameset_delegates_domain_and_ident() {
    let mut sky = SkyFrame::new(SkySystem::Icrs);
    sky.set_ident("Q");
    let mut fs = FrameSet::new(frame_with_domain(2, "PIXEL"));
    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(sky))
        .unwrap();

    // C forwards GetDomain / GetIdent to the current frame.
    assert_eq!(Frame::domain(&fs), "SKY");
    assert_eq!(Frame::ident(&fs), "Q");

    fs.set_current(FrameRef::Base).unwrap();
    assert_eq!(Frame::domain(&fs), "PIXEL");
}

#[test]
fn frameset_delegates_match_control_attributes() {
    let mut fs = FrameSet::new(frame_with_domain(3, "PIXEL"));
    assert_eq!(Frame::min_axes(&fs), 3);

    // Setting through the FrameSet must write through to the current
    // frame (C forwards SetMinAxes etc.).
    Frame::set_min_axes(&mut fs, 5);
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
    let cmp = ast_core::mappings::CmpMap::new(
        std::mem::replace(&mut z1, Box::new(UnitMap::new(1))),
        z2,
        ast_core::mappings::Composition::Series,
    )
    .unwrap();
    fs.add_frame(FrameRef::Base, Box::new(cmp), frame_with_domain(1, "B"))
        .unwrap();

    let simp = Mapping::simplified(&fs).unwrap();
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
    pm.set_iter_inverse(false);

    let mut fs = FrameSet::new(frame_with_domain(1, "A"));
    fs.add_frame(FrameRef::Base, Box::new(pm), frame_with_domain(1, "B"))
        .unwrap();
    assert!(Mapping::has_forward(&fs));
    assert!(!Mapping::has_inverse(&fs));
}

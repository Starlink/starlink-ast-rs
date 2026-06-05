//! `astFindFrame` end-to-end tests for plain `BasicFrame` targets and
//! templates.
//!
//! These cases exercise the default `find_frame_template` path
//! (`Frame::find_frame_template` → `default_find_frame` →
//! `Frame::match_template` → `Frame::sub_frame`) without involving
//! FrameSet or CmpFrame. They focus on the domain-list filter and on
//! the `Domain` attribute interaction with template matching, both of
//! which are documented in C `astFindFrame` (frame.c:3735) and
//! `Frame::Match` (frame.c:7073).

use ast_core::frames::{BasicFrame, BasicFrameBuilder, Frame};

fn bf(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

#[test]
fn find_frame_basic_self() {
    // 2-axis BasicFrame target and template — should produce a 2-frame
    // FrameSet (base = target, current = result).
    let target = bf(2);
    let template = bf(2);
    let result = target
        .find_frame_template(&template, "")
        .unwrap()
        .expect("self-match should succeed");
    assert_eq!(result.nframe(), 2);
    assert_eq!(result.base().naxes(), 2);
    assert_eq!(result.current().naxes(), 2);
}

#[test]
fn find_frame_basic_domain_priority() {
    // The template has its Domain explicitly set; the target has no
    // Domain. C `astMatch` (frame.c:7256-7281) requires equality when
    // the template's Domain is set.
    let target = bf(2);
    let mut template = bf(2);
    template.set_domain("PIXEL");
    let result = target.find_frame_template(&template, "").unwrap();
    assert!(result.is_none());
}

#[test]
fn find_frame_basic_domain_list_blank() {
    // An empty `domainlist` permits any domain.
    let target = bf(2);
    let template = bf(2);
    let result = target.find_frame_template(&template, "").unwrap();
    assert!(result.is_some());
}

#[test]
fn find_frame_basic_domain_list_match() {
    // Target's domain (PIXEL) is in the priority list.
    let mut target = bf(2);
    target.set_domain("PIXEL");
    let template = bf(2);
    let result = target.find_frame_template(&template, "PIXEL,SKY").unwrap();
    assert!(result.is_some());
}

#[test]
fn find_frame_basic_domain_list_miss() {
    // Target's domain (PIXEL) is not in the priority list.
    let mut target = bf(2);
    target.set_domain("PIXEL");
    let template = bf(2);
    let result = target.find_frame_template(&template, "GRID,SKY").unwrap();
    assert!(result.is_none());
}

// ---------------------------------------------------------------------
// CmpFrame integration tests (Task 8 from
// docs/superpower/plans/2026-06-05-find-frame-resume.md).
// ---------------------------------------------------------------------

use ast_core::frames::{CmpFrame, SkyFrame, SkySystem, SpecFrame, SpecSystem};

/// `cmpframe_overlay_template_copies_domain` (resume doc).
///
/// CmpFrame Overlay's Domain handling — the dispatcher in
/// `apply_template_overlay` (find_frame.rs) recurses into each
/// template component as a sub-template against the corresponding
/// result component, and Frame-level Domain on the *template
/// CmpFrame itself* acts only as the Match-time gate (frame.c:7256-
/// 7281), not as an overlay source. So:
///
///   - template Domain matches target Domain: match succeeds.
///   - template Domain mismatches target Domain: match rejected.
///
/// This test exercises both directions of the gate, which together
/// verify Domain handling at the CmpFrame level.
#[test]
fn cmpframe_overlay_template_copies_domain() {
    let mut template = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(2).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    );
    template.set_domain("MYCMP");

    // Matching Domain on target → Match passes the Domain gate.
    let mut target = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(2).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    );
    target.set_domain("MYCMP");
    let result = target
        .find_frame_template(&template, "")
        .unwrap()
        .expect("matching Domain → match");
    assert_eq!(result.nframe(), 2);
    assert_eq!(result.current().naxes(), 3);

    // Different Domain → Match rejects.
    let mut other_target = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(2).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    );
    other_target.set_domain("OTHER");
    assert!(
        other_target
            .find_frame_template(&template, "")
            .unwrap()
            .is_none()
    );
}

/// `cmpframe_sub_frame_identity` (resume doc).
///
/// Identity axis pick on a CmpFrame target produces a UnitMap-shaped
/// (3 in, 3 out) Mapping to a 3-axis result Frame.
#[test]
fn cmpframe_sub_frame_identity() {
    let target = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(2).build().unwrap()),
        Box::new(BasicFrameBuilder::new(1).build().unwrap()),
    );
    let target_axes: Vec<i32> = (0..3).collect();
    let template_axes: Vec<i32> = (0..3).collect();
    let (map, result) = target
        .sub_frame(None, 3, &target_axes, &template_axes)
        .unwrap()
        .expect("identity sub_frame on CmpFrame target");
    assert_eq!(map.nin(), 3);
    assert_eq!(map.nout(), 3);
    assert_eq!(result.naxes(), 3);
}

/// `cmpframe_match_against_cmpframe_template` (resume doc).
///
/// Template = CmpFrame(SkyFrame(FK5), SpecFrame(Freq))   3 axes.
/// Target  = CmpFrame(SkyFrame(FK5), SpecFrame(Freq))   3 axes.
///
/// Same-shape CmpFrame match. Default MinAxes/MaxAxes both equal 3 so
/// the cardinality gate passes; no Domain set, so the domain gate
/// passes; PartMatch runs the standard partition loop and finds a
/// match with axes1=[0,1] / axes2=[2].
#[test]
fn cmpframe_match_against_cmpframe_template() {
    let template = CmpFrame::new(
        Box::new(SkyFrame::new(SkySystem::Fk5)),
        Box::new(SpecFrame::new(SpecSystem::Freq)),
    );
    let target = CmpFrame::new(
        Box::new(SkyFrame::new(SkySystem::Fk5)),
        Box::new(SpecFrame::new(SpecSystem::Freq)),
    );
    let m = template
        .match_template(&target, false)
        .unwrap()
        .expect("CmpFrame ↔ CmpFrame match");
    assert_eq!(m.result.naxes(), 3);
    assert_eq!(m.template_axes.len(), 3);
    assert_eq!(m.target_axes.len(), 3);
}

/// `cmpframe_match_with_default_max_axes_fails` (resume doc).
///
/// SkyFrame template (default MinAxes=MaxAxes=2) cannot match a
/// 3-axis CmpFrame target. Cardinality gate in `default_match`
/// rejects.
#[test]
fn cmpframe_match_with_default_max_axes_fails() {
    let template = SkyFrame::new(SkySystem::Fk5);
    let target = CmpFrame::new(
        Box::new(SkyFrame::new(SkySystem::Fk5)),
        Box::new(SpecFrame::new(SpecSystem::Freq)),
    );
    let result = template.match_template(&target, false).unwrap();
    assert!(result.is_none());
}

/// `cmpframe_match_against_skyframe_template_with_max_axes` (resume
/// doc).
///
/// SkyFrame template with MaxAxes=3, MinAxes=2 against a CmpFrame
/// target containing a SkyFrame component. The cardinality gate now
/// accepts (3 axes within [2..=3]); the SkyFrame::match_template
/// downcast then needs to walk into the CmpFrame to find the
/// SkyFrame component. That walk requires a Rust equivalent of C's
/// `astPrimaryFrame` and is not yet implemented (the SkyFrame::Match
/// only handles direct SkyFrame targets — see skyframe.rs:1049-1057).
///
/// Marked #[ignore] until the SkyFrame walk into CmpFrame components
/// lands. The `cmpframe_match_with_default_max_axes_fails` test
/// above confirms the cardinality side of the contract works
/// today.
// ---------------------------------------------------------------------
// FrameSet::find_frame_template integration tests (Task 10).
// ---------------------------------------------------------------------
use ast_core::frames::{FrameRef, FrameSet};
use ast_core::mappings::UnitMap;

/// `frameset_find_frame_self_returns_current` — a 2-frame FrameSet
/// containing two BasicFrames is searched with a 2-axis template.
/// The current frame is the first candidate in the search order, so
/// the returned FrameSet's current matches the original FrameSet's
/// current.
#[test]
fn frameset_find_frame_self_returns_current() {
    let mut fs = FrameSet::new(Box::new(bf(2)));
    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(bf(2)))
        .unwrap();
    let template = bf(2);
    let result = fs
        .find_frame(&template, &[])
        .unwrap()
        .expect("self-match should succeed");
    assert_eq!(result.nframe(), 2);
    assert_eq!(result.base().naxes(), 2);
    assert_eq!(result.current().naxes(), 2);
}

/// `frameset_find_frame_priority_order` — when several frames in
/// the FrameSet would match the template, the *current* frame wins.
/// Distinguish them with their Domain attribute and confirm the
/// returned FrameSet's current Domain matches the FrameSet's
/// current Domain.
#[test]
fn frameset_find_frame_priority_order() {
    let mut base = bf(2);
    base.set_domain("BASE");
    let mut current = bf(2);
    current.set_domain("CURRENT");

    let mut fs = FrameSet::new(Box::new(base));
    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(current))
        .unwrap();
    // After add_frame, the current is the newly-added frame
    // (Domain="CURRENT").

    let template = bf(2);
    let result = fs
        .find_frame(&template, &[])
        .unwrap()
        .expect("priority order match");
    // Current Frame in the search order wins → result.current's
    // Domain should be "CURRENT".
    assert_eq!(result.current().domain(), "CURRENT");
}

/// `frameset_find_frame_domain_filter` — domain-list filter restricts
/// which frames inside the FrameSet are considered. Setting the list
/// to "BASE" should prefer the base frame even though it is not the
/// current one.
#[test]
fn frameset_find_frame_domain_filter() {
    let mut base = bf(2);
    base.set_domain("BASE");
    let mut current = bf(2);
    current.set_domain("CURRENT");

    let mut fs = FrameSet::new(Box::new(base));
    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(current))
        .unwrap();

    let template = bf(2);
    let result = fs
        .find_frame(&template, &["BASE"])
        .unwrap()
        .expect("BASE domain filter");
    assert_eq!(result.current().domain(), "BASE");
}

/// `frameset_find_frame_no_match_returns_none` — domain-list lists
/// a domain that no frame has → no match.
#[test]
fn frameset_find_frame_no_match_returns_none() {
    let mut base = bf(2);
    base.set_domain("BASE");
    let mut current = bf(2);
    current.set_domain("CURRENT");
    let mut fs = FrameSet::new(Box::new(base));
    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(current))
        .unwrap();
    let template = bf(2);
    let result = fs.find_frame(&template, &["NOSUCH"]).unwrap();
    assert!(result.is_none());
}

#[test]
fn cmpframe_match_against_skyframe_template_with_max_axes() {
    let mut template = SkyFrame::new(SkySystem::Fk5);
    template.set_max_axes(3);
    template.set_min_axes(2);
    let target = CmpFrame::new(
        Box::new(SkyFrame::new(SkySystem::Fk5)),
        Box::new(SpecFrame::new(SpecSystem::Freq)),
    );
    let m = template
        .match_template(&target, false)
        .unwrap()
        .expect("SkyFrame template should walk into CmpFrame's SkyFrame component");
    // Result is a 2-axis SkyFrame in FK5.
    assert_eq!(m.result.naxes(), 2);
}

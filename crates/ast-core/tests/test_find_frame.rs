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

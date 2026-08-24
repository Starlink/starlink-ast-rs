//! Tests for Frame equality semantics.
//!
//! C ancestor: frame.c `Equal` (~3549) defines Frame equality as "same
//! class and the conversion between the two frames simplifies to a
//! UnitMap", so purely descriptive attributes (Title, Label, Symbol,
//! Format) never affect equality, and no Frame subclass overrides Equal.
//! The Rust port compares attributes instead; these tests pin the C
//! behavior for the cosmetic attributes, which gate merge cancellation
//! in the simplify engine (e.g. NormMap pairs).

use ast_core::frames::{BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use ast_core::mappings::{CmpMap, Composition, Mapping, NormMap, UnitMap};

#[test]
fn basic_frames_differing_only_in_cosmetics_are_equal() {
    let f1 = BasicFrameBuilder::new(2)
        .title("First frame")
        .label(1, "X")
        .symbol(1, "x")
        .format_str(1, "3")
        .build()
        .unwrap();
    let f2 = BasicFrameBuilder::new(2)
        .title("Second frame")
        .label(1, "Different label")
        .symbol(1, "q")
        .format_str(1, "7")
        .build()
        .unwrap();
    assert!(f1.equal(&f2));
}

#[test]
fn basic_frames_differing_in_domain_are_not_equal() {
    let f1 = BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap();
    let f2 = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
    assert!(!f1.equal(&f2));
}

#[test]
fn skyframes_differing_only_in_title_are_equal() {
    let sf1 = SkyFrame::new(SkySystem::Icrs);
    let mut sf2 = SkyFrame::new(SkySystem::Icrs);
    sf2.set_title("A different description");
    assert!(Frame::equal(&sf1, &sf2));
}

#[test]
fn skyframe_explicit_default_domain_equals_unset_domain() {
    // Explicitly setting the domain to its default value must compare
    // equal to leaving it unset (C compares effective Domain strings).
    let sf1 = SkyFrame::new(SkySystem::Icrs);
    let mut sf2 = SkyFrame::new(SkySystem::Icrs);
    sf2.set_domain("SKY");
    assert!(Frame::equal(&sf1, &sf2));
}

#[test]
fn skyframes_differing_in_system_are_not_equal() {
    let sf1 = SkyFrame::new(SkySystem::Icrs);
    let sf2 = SkyFrame::new(SkySystem::Galactic);
    assert!(!Frame::equal(&sf1, &sf2));
}

#[test]
fn normmap_pair_over_cosmetically_different_frames_cancels() {
    // NormMap(f) followed by NormMap(f')^-1 must simplify to a UnitMap
    // when f and f' differ only in Title (mirrors C, where the frames
    // compare equal and cmpmap merging cancels the pair).
    let sf1 = SkyFrame::new(SkySystem::Icrs);
    let mut sf2 = SkyFrame::new(SkySystem::Icrs);
    sf2.set_title("A different description");

    let nm1 = NormMap::new(Box::new(sf1));
    let mut nm2 = NormMap::new(Box::new(sf2));
    nm2.invert();

    let cmp = CmpMap::new(Box::new(nm1), Box::new(nm2), Composition::Series).unwrap();
    let simplified = <dyn Mapping>::simplify(&cmp).unwrap();
    assert!(
        simplified.as_any().is::<UnitMap>(),
        "expected UnitMap, got {simplified:?}"
    );
}

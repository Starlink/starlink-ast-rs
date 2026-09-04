//! A bare Frame where a Mapping is expected.
//!
//! C accepts a Frame in any Mapping slot, because `Frame::Transform`
//! (`frame.c:11739`) transforms through `astUnitMap( astGetNaxes( this ) )`.
//! This port makes Region the only Frame that is also a Mapping, so a `Map<n>`
//! slot holding `Begin Frame` or `Begin CmpFrame` is refused.
//!
//! The fixture here is a dump C wrote, and the first test pins the refusal so
//! that the day the decision changes, this test is what changes with it. The
//! remaining tests are the controls: the two Region nestings that are settled
//! must keep working, so a future fix cannot be a blanket "accept any Frame",
//! nor the refusal be widened to catch them.
//!
//! The question this exists to put to the maintainers is in
//! `docs/issues/frame-in-a-mapping-slot.md`.

use std::path::{Path, PathBuf};

use ast_core::AnyObject;
use ast_core::channel::{Channel, NativeChannel};
use ast_core::error::AstResult;

/// The one dump the reader refuses, with the reason it is refused.
///
/// Not a gap in the port's own terms and not a licensed relaxation of a
/// comparison: a construct whose support is an open design question. The reason
/// string is what a reader gets instead of a silent failure, and the count is
/// in the verification ledger so it cannot grow unnoticed.
const FRAME_IN_MAP_SLOT_UNDECIDED: &[(&str, &str)] = &[(
    "serialisation/frame_in_map_slot.ast",
    "a bare Frame in a Map<n> slot. C accepts it because every Frame is a \
     Mapping there; this port makes Region the only Frame that is a Mapping. \
     Whether C's acceptance is a side effect of supporting Region or answers a \
     real use case is a question for the maintainers — \
     docs/issues/frame-in-a-mapping-slot.md",
)];

/// Region classes as they appear at the head of a Native block.
const REGION_CLASSES: &[&str] = &[
    "Box",
    "Circle",
    "Ellipse",
    "Interval",
    "NullRegion",
    "PointList",
    "Polygon",
    "Prism",
    "CmpRegion",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_native(text: &str) -> AstResult<Option<AnyObject>> {
    NativeChannel::from_string(text).read()
}

/// Whether a Native dump nests a Region block inside a block of `outer`.
///
/// Containment is what the question is about, so this walks the block structure
/// rather than testing for both class names anywhere in the text: every
/// `CmpFrame` in the simplify corpus is a Region's *own* Frame, which is the
/// opposite nesting.
fn region_nested_in(text: &str, outer: &str) -> bool {
    let mut stack: Vec<&str> = Vec::new();
    for line in text.lines() {
        let s = line.trim();
        if let Some(class) = s.strip_prefix("Begin ") {
            let class = class.split_whitespace().next().unwrap_or("");
            if REGION_CLASSES.contains(&class) && stack.contains(&outer) {
                return true;
            }
            stack.push(class);
        } else if s.starts_with("End ") {
            stack.pop();
        }
    }
    false
}

/// The class filling a `Map<n>` slot, when that class is a Frame rather than a
/// Mapping.
///
/// The slot's value is the block that opens on the following line, so this
/// reads the pair rather than testing for the two names anywhere in the text.
fn frame_class_in_a_map_slot(text: &str) -> Option<String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let mut slot = false;
    for line in &mut lines {
        if slot {
            if let Some(rest) = line.strip_prefix("Begin ") {
                let class = rest.split_whitespace().next().unwrap_or("");
                if class == "Frame" || class == "CmpFrame" {
                    return Some(class.to_string());
                }
            }
            slot = false;
        }
        let is_slot = line
            .strip_prefix("Map")
            .map(|r| {
                let digits: String = r.chars().take_while(char::is_ascii_digit).collect();
                !digits.is_empty() && r[digits.len()..].trim_start().starts_with('=')
            })
            .unwrap_or(false);
        if is_slot {
            slot = true;
        }
    }
    None
}

#[test]
fn frame_in_a_map_slot_is_rejected() {
    for (name, reason) in FRAME_IN_MAP_SLOT_UNDECIDED {
        let path = fixtures().join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));

        // The construct is present in the fixture: if a future regeneration
        // lost it, this test would pass for the wrong reason.
        assert!(
            frame_class_in_a_map_slot(&text).is_some(),
            "{name} no longer puts a Frame in a Mapping slot, so it does not \
             exercise what it was captured for"
        );

        let err = read_native(&text).expect_err(&format!(
            "{name} was read; the decision recorded in \
             FRAME_IN_MAP_SLOT_UNDECIDED has changed, so update that entry and \
             this test rather than deleting them. Reason on record: {reason}"
        ));
        let message = err.to_string();
        assert!(
            message.contains("CmpFrame"),
            "{name} was refused, but not for the recorded reason: {message}"
        );
    }
}

/// The first control: a Region inside a `CmpMap`.
///
/// A Region is a Mapping here, so this nesting reads today, and the simplify
/// corpus is full of it. The floor keeps the refusal above narrow: it fails if
/// anyone closes the gap by refusing Frames in Mapping slots wholesale.
#[test]
fn a_region_inside_a_cmpmap_still_reads() {
    let corpus = fixtures().join("simplify");
    let mut read = 0usize;
    for entry in std::fs::read_dir(&corpus).expect("simplify corpus readable") {
        let path = entry.expect("readable entry").path();
        if path.extension().and_then(|x| x.to_str()) != Some("map") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("fixture readable");
        if !region_nested_in(&text, "CmpMap") {
            continue;
        }
        read_native(&text).unwrap_or_else(|e| panic!("{} no longer reads: {e}", path.display()));
        read += 1;
    }

    // The corpus held 33 such nestings when this control was written. A floor,
    // not an equality: fixtures are added.
    assert!(
        read >= 33,
        "only {read} fixtures nest a Region inside a CmpMap; the settled \
         nesting is no longer covered"
    );
}

/// The second control: a Region inside a `CmpFrame`.
///
/// A Region is a Frame, so a `CmpFrame` may hold one, and C's own dump of that
/// arrangement reads here. No simplify fixture exercises it — every `CmpFrame`
/// in that corpus is a Region's own Frame, the opposite nesting — so the
/// reference product beside this test is what covers it.
#[test]
fn a_region_inside_a_cmpframe_still_reads() {
    let path = fixtures().join("serialisation/region_in_cmpframe.ast");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));
    assert!(
        region_nested_in(&text, "CmpFrame"),
        "serialisation/region_in_cmpframe.ast no longer nests a Region inside a \
         CmpFrame, so \
         it does not cover the settled nesting"
    );
    read_native(&text)
        .unwrap_or_else(|e| panic!("{} no longer reads: {e}", path.display()))
        .expect("the dump holds one object");
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Attributes set, read, tested and cleared by name from strings, against
//! the C library.
//!
//! There is no C ancestor in `ast_tester/`: the cases are those of
//! `tools/oracle_fixtures/attr_strings_probe.c`, which applies each string
//! to an object built by the C library and records what `astSet`, `astSetC`,
//! `astGetC`, `astTest` and `astClear` do with it: the value, or the error
//! status and every message, and the object's Native dump afterwards.
//! Each case starts from the Native text C wrote for the object, so both
//! libraries work on the same one.

use crate::common::native_dump;

#[rustfmt::skip]
#[path = "attr_strings_oracle/cases.rs"]
mod cases;

use cases::{CASES, Case, Op, Out};
use starlink_ast::attributes::Attributes;
use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::frames::Frame;
use starlink_ast::{AnyObject, from_native_string};

/// Operations whose C outcome the Rust library cannot give, by case and
/// operation, each with its reason.
/// `cases_with_no_rust_value_still_differ` fails when one starts to match.
const NO_RUST_VALUE_EXCLUDED: &[(&str, Op, &str)] = &[
    (
        "frame2_no_rust_value",
        Op::Get("Nobject"),
        "the Rust library does not count the live objects of a class",
    ),
    (
        "frame2_no_rust_value",
        Op::Get("ObjSize"),
        "the Rust library has no in-memory size for an object",
    ),
    (
        "frame2_no_rust_value",
        Op::Get("RefCount"),
        "an owned Rust value has no reference count",
    ),
    (
        "frame2_no_rust_value",
        Op::Set("Invert=1"),
        "a Frame that is not a FrameSet stores no Mapping flags",
    ),
    (
        "frame2_no_rust_value",
        Op::Clear("Invert"),
        "a Frame that is not a FrameSet stores no Mapping flags",
    ),
    (
        "frame2_no_rust_value",
        Op::Set("Report=1"),
        "a Frame that is not a FrameSet stores no Mapping flags",
    ),
    (
        "frame2_no_rust_value",
        Op::Clear("Report"),
        "a Frame that is not a FrameSet stores no Mapping flags",
    ),
];

/// The case-name prefixes each test replays; `every_case_is_replayed`
/// checks that together they cover every case.
const OBJECT_AND_MAPPING: &[&str] = &["frame2_", "frame1_object"];
const FRAME: &[&str] = &["frame_"];
const SKY: &[&str] = &["sky_"];
const SPECTRAL: &[&str] = &["spec_", "dsb_"];
const TIME_AND_COMPOUND: &[&str] = &["time_", "cmp_"];

/// What the Rust library did with an operation.
#[derive(Debug, Clone, PartialEq)]
enum Got {
    Done,
    Value(String),
    Flag(bool),
    Err(ErrorCode, Vec<String>),
}

impl Got {
    fn of<T>(result: AstResult<T>, ok: impl FnOnce(T) -> Got) -> Got {
        match result {
            Ok(v) => ok(v),
            // C reports its first message and then any others; the port
            // keeps the others as the error's context.
            Err(e) => {
                let mut messages = vec![e.message().to_string()];
                messages.extend(e.context().iter().cloned());
                Got::Err(e.code(), messages)
            }
        }
    }

    fn matches(&self, expected: &Out) -> bool {
        match (self, expected) {
            (Got::Done, Out::Done) => true,
            (Got::Value(a), Out::Value(b)) => a == b,
            (Got::Flag(a), Out::Flag(b)) => a == b,
            (Got::Err(code, messages), Out::Err(c_code, c_messages)) => {
                code == c_code
                    && messages
                        .iter()
                        .map(String::as_str)
                        .eq(c_messages.iter().copied())
            }
            _ => false,
        }
    }
}

/// The Frame an object is, for the classes the cases use.
fn frame_mut(obj: &mut AnyObject) -> &mut dyn Frame {
    match obj {
        AnyObject::BasicFrame(f) => f,
        AnyObject::SkyFrame(f) => f,
        AnyObject::SpecFrame(f) => f,
        AnyObject::DSBSpecFrame(f) => f,
        AnyObject::TimeFrame(f) => f,
        AnyObject::CmpFrame(f) => f,
        AnyObject::FrameSet(f) => f,
        AnyObject::Plot(f) => f.as_mut(),
        other => panic!("no attribute strings for {other:?}"),
    }
}

fn apply(target: &mut dyn Frame, op: Op) -> Got {
    match op {
        Op::Set(settings) => Got::of(target.set_attributes(settings), |()| Got::Done),
        Op::SetC(name, value) => Got::of(target.set_attribute(name, value), |()| Got::Done),
        Op::Get(name) => Got::of(target.get_attribute(name), Got::Value),
        Op::Test(name) => Got::of(target.test_attribute(name), Got::Flag),
        Op::Clear(names) => Got::of(target.clear_attributes(names), |()| Got::Done),
    }
}

/// Run a case: what each operation gave, and the object's dump afterwards.
fn run(case: &Case) -> (Vec<Got>, String) {
    assert!(
        !case.fitschan,
        "{}: FitsChan cases are not replayed yet",
        case.name
    );
    let mut obj = from_native_string(case.object)
        .unwrap_or_else(|e| panic!("{}: cannot read the object: {e}", case.name));
    assert_eq!(
        native_dump(&obj).unwrap(),
        case.object,
        "{}: the object does not write as C wrote it",
        case.name
    );
    let got = case
        .ops
        .iter()
        .map(|(op, _)| apply(frame_mut(&mut obj), *op))
        .collect();
    (got, native_dump(&obj).unwrap())
}

fn excluded(case: &Case, op: Op) -> bool {
    NO_RUST_VALUE_EXCLUDED
        .iter()
        .any(|(name, x, _)| *name == case.name && *x == op)
}

/// Replay every case whose name starts with one of `prefixes`, and fail
/// with each difference from C.
fn replay(prefixes: &[&str]) {
    let mut failures = Vec::new();
    let mut ncase = 0;
    for case in CASES {
        if !prefixes.iter().any(|p| case.name.starts_with(p)) {
            continue;
        }
        ncase += 1;
        let (got, dump) = run(case);
        for (i, ((op, expected), got)) in case.ops.iter().zip(&got).enumerate() {
            if !excluded(case, *op) && !got.matches(expected) {
                failures.push(format!(
                    "{} op {i} {op:?}:\n      C: {expected:?}\n   Rust: {got:?}",
                    case.name
                ));
            }
        }
        if dump != case.dump {
            failures.push(format!(
                "{}: dump differs\n--- C\n{}--- Rust\n{dump}",
                case.name, case.dump
            ));
        }
    }
    assert!(ncase > 0, "no case starts with any of {prefixes:?}");
    assert!(
        failures.is_empty(),
        "{} differences from C:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn object_and_mapping_names_match_the_c_library() {
    replay(OBJECT_AND_MAPPING);
}

#[test]
fn frame_names_match_the_c_library() {
    replay(FRAME);
}

#[test]
fn skyframe_names_match_the_c_library() {
    replay(SKY);
}

#[test]
fn spectral_names_match_the_c_library() {
    replay(SPECTRAL);
}

#[test]
fn time_and_compound_names_match_the_c_library() {
    replay(TIME_AND_COMPOUND);
}

/// C's attribute functions call each other without end for a Top, Bottom
/// or Direction given without an index and with a value that is not a
/// number, on a Frame class of one axis that takes those names without an
/// index (`c-library-quirks.md`), so there is no C outcome to compare with.
/// The port reports the Axis object's error, as it does for a plain Frame.
#[test]
fn an_unreadable_axis_value_without_an_index_is_an_error() {
    use starlink_ast::frames::{SpecFrame, SpecSystem};
    let mut spec = SpecFrame::new(SpecSystem::Freq);
    for setting in ["Top=high", "Bottom=low", "Direction=x"] {
        let err = spec.set_attributes(setting).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Badat, "{setting}");
        assert_eq!(
            err.message(),
            format!(
                "astSet: The attribute setting \"{}\" is invalid for a Axis.",
                setting.to_lowercase()
            )
        );
    }
    // A value that can be read still goes through.
    spec.set_attributes("Top=5").unwrap();
    assert_eq!(spec.get_attribute("Top").unwrap(), "5");
    // Through a CmpFrame, which hands the setting to the SpecFrame, the
    // error is the CmpFrame's own.
    let mut cmp = starlink_ast::frames::CmpFrame::new(
        Box::new(spec),
        Box::new(starlink_ast::frames::SkyFrame::new(
            starlink_ast::frames::SkySystem::Icrs,
        )),
    );
    let err = cmp.set_attributes("Top(1)=high").unwrap_err();
    assert_eq!(
        err.message(),
        "astSet: The attribute setting \"top(1)=high\" is invalid for the given CmpFrame."
    );
}

#[test]
fn cases_with_no_rust_value_still_differ() {
    for &(name, op, reason) in NO_RUST_VALUE_EXCLUDED {
        let case = CASES
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no case {name}"));
        let (got, _) = run(case);
        let i = case
            .ops
            .iter()
            .position(|(x, _)| *x == op)
            .unwrap_or_else(|| panic!("{name} has no operation {op:?}"));
        assert!(
            !got[i].matches(&case.ops[i].1),
            "{name} {op:?} now matches C; remove it from NO_RUST_VALUE_EXCLUDED ({reason})"
        );
    }
}

#[test]
fn every_case_is_replayed() {
    let replayed = [OBJECT_AND_MAPPING, FRAME, SKY, SPECTRAL, TIME_AND_COMPOUND];
    for case in CASES {
        assert!(
            replayed
                .iter()
                .any(|set| set.iter().any(|p| case.name.starts_with(p))),
            "no test replays the case {}",
            case.name
        );
    }
}

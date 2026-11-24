// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Splitting formatted axis values into fields, abbreviating them, reading
//! dates, and finding a central value for tick marks.
//!
//! Ported from `reference/starlink-ast/ast_tester/testfields.c`, one test per
//! C section.
//! Divergences from the C:
//!
//! - C reaches `astFields`, `astAbbrev` and `astCentre` through the protected
//!   interface; here they are the public [`Frame::fields`], [`Frame::abbrev`]
//!   and [`Frame::centre`], with 1-based axes.
//! - An expected error is compared by its code and every message C reports,
//!   in order, without C's first "Error in routine" line.

use starlink_ast::attributes::Attributes;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::frames::{BasicFrameBuilder, Frame, SkyFrame, TimeFrame, TimeSystem};

/// Check that `result` is an error with `code` whose messages, one per
/// line, are `messages`, with no context that C would not report.
pub(crate) fn expect_err<T>(result: AstResult<T>, code: ErrorCode, messages: &[&str]) {
    let Err(err) = result else {
        panic!("an error was expected");
    };
    assert_eq!(err.code(), code, "error code of {err}");
    let got: Vec<&str> = err.message().split('\n').collect();
    assert_eq!(got, messages, "messages of {err}");
    assert!(err.context().is_empty(), "context C does not report: {err}");
}

/// Split `text` on axis 1 of `frame` with `fmt`, and check that there are
/// `nfld` fields, that the first starts `off` characters in and has `len`
/// characters, and that the value is `value` (unchecked if bad).
fn check_fields(
    frame: &dyn Frame,
    fmt: &str,
    text: &str,
    nfld: usize,
    off: usize,
    len: usize,
    value: f64,
) {
    let split = frame
        .fields(1, fmt, text, 3)
        .unwrap_or_else(|e| panic!("fields {fmt:?} {text:?}: {e}"));
    assert_eq!(split.fields.len(), nfld, "fields {fmt:?} {text:?}");
    if nfld > 0 {
        let first = &split.fields[0];
        assert_eq!(
            (first.start, first.len()),
            (off, len),
            "fields {fmt:?} {text:?}"
        );
        if value != BAD_VALUE {
            assert!(
                (split.value - value).abs() <= 1.0e-12,
                "fields {fmt:?} {text:?} gave {}",
                split.value
            );
        }
    }
}

/// A formatted sky axis value is split at the separators its Format calls
/// for.
#[test]
fn sky() {
    let deg = std::f64::consts::PI / 180.0;
    let sky = SkyFrame::unset_system();
    check_fields(&sky, "hms", "12:34", 0, 0, 0, BAD_VALUE);
    check_fields(&sky, "hms", "12", 0, 0, 0, BAD_VALUE);
    check_fields(&sky, "dms", "1:2", 0, 0, 0, BAD_VALUE);
    check_fields(&sky, "lhms", "12h34", 0, 0, 0, BAD_VALUE);
    check_fields(&sky, "hms", "12:34:56", 3, 0, 2, BAD_VALUE);

    // With no first value, the whole of a value that has no fields.
    assert_eq!(sky.abbrev(1, "hms", None, "a:b:c").unwrap(), 0);

    // A Format of one field: the field runs to the end of the text.
    check_fields(&sky, "d", "+12.5", 1, 0, 5, 12.5 * deg);
    check_fields(&sky, "d", "-12.5", 1, 0, 5, -12.5 * deg);
    check_fields(&sky, "d", "12.5 ", 1, 0, 4, 12.5 * deg);
    check_fields(&sky, "h", "+3", 1, 0, 2, 45.0 * deg);
}

/// A value on a simple axis is one field, without the spaces round it.
#[test]
fn simple() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    check_fields(&frame, "%g", "  -7 ", 1, 2, 2, -7.0);
    check_fields(&frame, "%10.3f", "     1.500", 1, 5, 5, 1.5);
    check_fields(&frame, "%g", "42", 1, 0, 2, 42.0);
}

/// A TimeFrame formatting its values as dates.
fn iso_frame() -> TimeFrame {
    let mut frame = TimeFrame::new(TimeSystem::Mjd);
    frame.set_format_str(1, "iso.0").unwrap();
    frame
}

/// The year, month and day of a date, read as a Modified Julian Date.
#[test]
fn dates() {
    let frame = iso_frame();
    assert_eq!(frame.unformat(1, "2012-03-05").unwrap(), (55991.0, 10));

    // A day of zero is the day before the first of the month.
    assert_eq!(frame.unformat(1, "2012-03-00").unwrap(), (55986.0, 10));

    // A month of zero or thirteen is not a date, so the whole of the text
    // is not read.
    for text in ["2012-00-01", "2012-13-01"] {
        let (_, nc) = frame.unformat(1, text).unwrap();
        assert!(nc < 10, "{text} read to {nc}");
    }

    // The same text is not accepted as an Epoch.
    let mut plain = BasicFrameBuilder::new(1).build().unwrap();
    assert!(plain.set_attribute("Epoch", "2012-13-01").is_err());
}

/// A central value for the tick marks of a date axis.
#[test]
fn centre() {
    let frame = iso_frame();
    assert_eq!(frame.centre(1, 56000.37, 0.01).unwrap(), 56000.0);

    // A gap too small to change the formatted date: the value itself.
    assert_eq!(frame.centre(1, 56000.37, 1.0e-9).unwrap(), 56000.37);
    assert_eq!(frame.centre(1, BAD_VALUE, 1.0).unwrap(), BAD_VALUE);
}

/// The message C gives for a Format it cannot use for an axis value.
fn unusable(fmt: &str) -> String {
    format!(
        "astFormat: The Format string \"{fmt}\" cannot be used to format an axis value. It \
         should have no more than one conversion specification, for a floating point or \
         integer value."
    )
}

/// A Format is given to `sprintf` with the axis value as its only
/// argument, so one that asks for anything else is refused, and so is the
/// same format given to `fields`.
#[test]
fn formats() {
    for fmt in [
        "%s", "%n", "%d%d", "%ld", "%hd", "%*d", "%", "abc%", "%Lf", "%p", "%10s",
    ] {
        let mut frame = BasicFrameBuilder::new(1).build().unwrap();
        frame.set_format_str(1, fmt).unwrap();
        expect_err(frame.format(1, 65.25), ErrorCode::Fmter, &[&unusable(fmt)]);
    }
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    expect_err(
        frame.fields(1, "%s", "65.25", 3),
        ErrorCode::Fmter,
        &[&unusable("%s")],
    );
    // A sky axis splits a `%` value as a plain axis does when abbreviating.
    expect_err(
        SkyFrame::unset_system().abbrev(1, "%s", Some("1"), "2"),
        ErrorCode::Fmter,
        &[&unusable("%s")],
    );

    // The C library formats these; the "'" flag groups no digits in the C
    // locale.
    for (fmt, want) in [
        ("%a", "0x1.05p+6"),
        ("%A", "0X1.05P+6"),
        ("%'d", "65"),
        ("%'.3f", "65.250"),
        ("%.1a", "0x1.0p+6"),
        ("%20a", "           0x1.05p+6"),
        ("%+a", "+0x1.05p+6"),
        ("%la", "0x1.05p+6"),
    ] {
        let mut frame = BasicFrameBuilder::new(1).build().unwrap();
        frame.set_format_str(1, fmt).unwrap();
        assert_eq!(frame.format(1, 65.25).unwrap(), want, "{fmt}");
    }

    // A result longer than C's buffer.
    let mut frame = BasicFrameBuilder::new(1).build().unwrap();
    frame.set_format_str(1, "%200f").unwrap();
    expect_err(
        frame.format(1, 65.25),
        ErrorCode::Fmter,
        &[
            "astAxisFormat(Axis): Internal buffer overflow while formatting a coordinate \
             value of 65.25 - result exceeds 127 characters.",
            "The format string was \"%200f\".",
        ],
    );
}

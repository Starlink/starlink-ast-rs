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

/// Check that `result` is an error with `code` whose messages are C's
/// `messages`, in order: the lines of the message, then each line of
/// context, which is where the port puts the messages C adds as an error
/// passes up.
pub(crate) fn expect_err<T>(result: AstResult<T>, code: ErrorCode, messages: &[&str]) {
    let Err(err) = result else {
        panic!("an error was expected");
    };
    assert_eq!(err.code(), code, "error code of {err}");
    let got: Vec<&str> = err
        .message()
        .split('\n')
        .chain(err.context().iter().map(String::as_str))
        .collect();
    assert_eq!(got, messages, "messages of {err}");
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

    // No number can be read from a blank value, which is a field of zero.
    check_fields(&sky, "d", "", 1, 0, 0, 0.0);
    check_fields(&sky, "d", "   ", 1, 3, 0, 0.0);
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

    // The same text is not accepted as an Epoch, and the year or month
    // that is not valid is named.
    let mut plain = BasicFrameBuilder::new(1).build().unwrap();
    expect_err(
        plain.set_attribute("Epoch", "2012-13-01"),
        ErrorCode::Attin,
        &[
            "Month value (13) is invalid.",
            "astSetAttrib(Frame): Invalid epoch value \"2012-13-01\" given for coordinate system.",
        ],
    );
    expect_err(
        plain.set_attribute("Epoch", "-4800-01-01"),
        ErrorCode::Attin,
        &[
            "Year value (-4800) is invalid.",
            "astSetAttrib(Frame): Invalid epoch value \"-4800-01-01\" given for coordinate system.",
        ],
    );
}

/// A date that has no calendar value is `<bad>`, as a bad value is, and
/// the seconds of a date have at most 9 decimal places.
#[test]
fn date_formats() {
    let mut frame = TimeFrame::new(TimeSystem::Mjd);
    for fmt in ["iso.9", "iso.10", "iso.12", "iso.80", "iso.200T"] {
        frame.set_format_str(1, fmt).unwrap();
        let sep = if fmt.ends_with('T') { 'T' } else { ' ' };
        assert_eq!(
            frame.format(1, 56000.12345).unwrap(),
            format!("2012-03-14{sep}02:57:46.079999926"),
            "{fmt}"
        );
    }
    frame.set_format_str(1, "iso.2").unwrap();
    for value in [1.0e30, 1.0e10, -1.0e10, -3.0e8] {
        assert_eq!(frame.format(1, value).unwrap(), "<bad>", "{value:e}");
    }
    assert_eq!(frame.format(1, 3.0e8).unwrap(), "823230-12-24 00:00:00.00");

    // A TimeOrigin with no date is shown as one in the default Label and
    // Title.
    let mut frame = TimeFrame::new(TimeSystem::Mjd);
    frame.set_time_origin(1.0e30);
    let want = "Modified Julian Date offset from <bad>";
    assert_eq!(Frame::label(&frame, 1).unwrap(), want);
    assert_eq!(Frame::title(&frame).unwrap(), want);
}

/// A central value for the tick marks of a date axis, from the dates a gap
/// either side of the value and the first field in which they differ.
/// When that is the day or a time field, the centre is the start of the
/// coarser unit, so that ticks a whole number of hours apart fall on round
/// hours of the day: midnight, or the first of the month.
/// When it is the month or the year, whose lengths vary so that no gap
/// divides them, the centre is the start of that unit inside the window.
///
/// The port differs from `testfields.c` here, by decision. C sets the
/// first differing field to zero, so a gap of a day gives the day before
/// the first of the month (55986), and a month or year of zero, which is
/// no date, falls back to rounding the value to a multiple of the gap
/// (56010 at a gap of 30 days, which `testfields.c` checks only to be
/// within a gap): see the quirk "TimeFrame `Centre` zeroes a calendar
/// field that has no zero".
#[test]
fn centre() {
    let frame = iso_frame();
    // 2012-03-14 08:52:48. The hours differ: midnight, as in C.
    assert_eq!(frame.centre(1, 56000.37, 0.01).unwrap(), 56000.0);
    assert_eq!(frame.centre(1, 56000.37, 0.25).unwrap(), 56000.0);
    // The day differs: the first of the month.
    assert_eq!(frame.centre(1, 56000.37, 1.0).unwrap(), 55987.0);
    assert_eq!(frame.centre(1, 56000.37, -1.0).unwrap(), 55987.0);
    // The month differs: the start of April, inside the window.
    assert_eq!(frame.centre(1, 56000.37, 30.0).unwrap(), 56018.0);
    // The year differs: the start of 2013, inside the window.
    assert_eq!(frame.centre(1, 56000.37, 400.0).unwrap(), 56293.0);

    // A window that crosses the end of March starts April, a gap or less
    // from the value.
    for gap in [0.1, 1.0, 7.0] {
        assert_eq!(frame.centre(1, 56017.98, gap).unwrap(), 56018.0, "{gap}");
    }
    // 2012-11-14, a month either side: the start of December.
    assert_eq!(frame.centre(1, 56245.37, 30.0).unwrap(), 56262.0);

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

    // `%c` writes the byte of the value, which outside ASCII is the
    // character of that code in ISO 8859-1, padded as one character.
    for (fmt, value, want) in [
        ("%c", 65.0, "A"),
        ("%c", 233.0, "\u{e9}"),
        ("%3c", 233.0, "  \u{e9}"),
        ("%-3c|", 255.0, "\u{ff}  |"),
        ("x%c", 0.0, "x"),
    ] {
        let mut frame = BasicFrameBuilder::new(1).build().unwrap();
        frame.set_format_str(1, fmt).unwrap();
        assert_eq!(frame.format(1, value).unwrap(), want, "{fmt} {value}");
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

/// A sky value whose text would pass the 70 characters of C's buffer is
/// `<bad>`; by then even the degrees are digits of no meaning. The values
/// are the C library's.
#[test]
fn huge_sky_values() {
    let cases = [
        ("d", 1.0e300, "<bad>"),
        ("d", -1.0e300, "<bad>"),
        ("dms.3", 1.0e59, "<bad>"),
        ("dms.3", -1.0e58, "<bad>"),
        (
            "dms.3",
            1.0e58,
            "572957795130823145056046885095140918830124262074803426426880:00:00.001",
        ),
        ("lhms.2", 1.0e60, "<bad>"),
        (
            "lhms.2",
            -1.0e58,
            "-38197186342054881912256091972331846639271185633743938256896h00m00.01s",
        ),
        (
            "hms",
            1.0e60,
            "3819718634205487990518902410457576390105654109539140271865856:00:01",
        ),
        ("dm", 1.0e300, "<bad>"),
        ("+d.5", 1.0, "+57.29578"),
    ];
    for (fmt, value, want) in cases {
        let mut sky = SkyFrame::unset_system();
        sky.set_format_str(1, fmt).unwrap();
        assert_eq!(sky.format(1, value).unwrap(), want, "{fmt} {value:e}");
    }
}

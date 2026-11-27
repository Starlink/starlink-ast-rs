// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A dump that cannot be read fails with the C library's ordered list of
//! messages: the first error, a `Failed to read the "<item>" Object value.`
//! line from the innermost Object holding it, and `Error while reading a
//! <class> from a Channel.`
//!
//! The expected lists are what the C library at the pinned commit reports for
//! the same dumps, captured with a probe built like
//! `tools/oracle_fixtures/hardening_probe.c`; C's first message, "AST: Error
//! in routine ...", is not compared.

use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::{AnyObject, from_native_string};

const FRAME2: &str =
    " Begin Frame\n Naxes = 2\n Ax1 =\n Begin Axis\n End Axis\n Ax2 =\n Begin Axis\n End Axis\n";

fn expect(result: AstResult<AnyObject>, code: ErrorCode, messages: &[&str]) {
    let Err(err) = result else {
        panic!("the dump was read");
    };
    let got: Vec<&str> = err
        .message()
        .split('\n')
        .chain(err.context().iter().map(String::as_str))
        .collect();
    assert_eq!((err.code(), got), (code, messages.to_vec()));
}

#[test]
fn a_failed_load_ends_with_the_class_and_channel() {
    expect(
        from_native_string(" Begin UnitMap\n Nin = 2147483648\n IsA Mapping\n End UnitMap\n"),
        ErrorCode::Badin,
        &[
            "astRead(Channel): The value \"nin = 2147483648\" cannot be read as an integer.",
            "Error while reading a UnitMap from a Channel.",
        ],
    );
}

#[test]
fn an_integer_c_cannot_read_is_refused() {
    for text in [
        "2147483648",
        "1e308",
        "2 3",
        "1.5",
        "0x10",
        "1e0",
        "<bad>",
        "x",
    ] {
        expect(
            from_native_string(&format!(
                " Begin Frame\n Naxes = 1\n MchEnd = {text}\n Ax1 =\n Begin Axis\n End Axis\n End Frame\n"
            )),
            ErrorCode::Badin,
            &[
                &format!(
                    "astRead(Channel): The value \"mchend = {text}\" cannot be read as an integer."
                ),
                "Error while reading a Frame from a Channel.",
            ],
        );
    }
}

#[test]
fn an_integer_c_can_read_is_read() {
    for text in [
        "+1",
        " 1 ",
        "-0",
        "01",
        "2147483647",
        "-2147483648",
        "\"3\"",
    ] {
        let dump = format!(
            " Begin Frame\n Naxes = 1\n MchEnd = {text}\n Ax1 =\n Begin Axis\n End Axis\n End Frame\n"
        );
        assert!(from_native_string(&dump).is_ok(), "{text}");
    }
}

#[test]
fn a_double_c_cannot_read_is_refused() {
    for text in ["abc", "2 3", "1e", "0x", "0x1p", "0x.p1"] {
        expect(
            from_native_string(&format!(
                " Begin Frame\n Naxes = 1\n Epoch = {text}\n Ax1 =\n Begin Axis\n End Axis\n End Frame\n"
            )),
            ErrorCode::Badin,
            &[
                &format!(
                    "astRead(Channel): The value \"epoch = {text}\" cannot be read as a double \
                     precision floating point number."
                ),
                "Error while reading a Frame from a Channel.",
            ],
        );
    }
    for text in ["1e999", "-1e999", "nan", "inf"] {
        expect(
            from_native_string(&format!(
                " Begin Frame\n Naxes = 1\n Epoch = {text}\n Ax1 =\n Begin Axis\n End Axis\n End Frame\n"
            )),
            ErrorCode::Badin,
            &[
                &format!(
                    "astRead(Channel): Illegal double precision floating point value \"{text}\" \
                     read for \"epoch\"."
                ),
                "Error while reading a Frame from a Channel.",
            ],
        );
    }
}

#[test]
fn a_double_c_can_read_is_read() {
    for text in ["<bad>", "\"<bad>\"", "1e308", "0x10", "1e-320", "\"3\""] {
        let dump = format!(
            " Begin Frame\n Naxes = 1\n Epoch = {text}\n Ax1 =\n Begin Axis\n End Axis\n End Frame\n"
        );
        assert!(from_native_string(&dump).is_ok(), "{text}");
    }
    for (text, zoom) in [
        ("0x10", 16.0),
        ("-0x10", -16.0),
        ("0x1.8p3", 12.0),
        ("0X1P-2", 0.25),
    ] {
        let dump =
            format!(" Begin ZoomMap\n Nin = 1\n IsA Mapping\n Zoom = {text}\n End ZoomMap\n");
        let AnyObject::ZoomMap(map) = from_native_string(&dump).unwrap() else {
            panic!("not a ZoomMap");
        };
        assert_eq!(map.zoom(), zoom, "{text}");
    }
}

#[test]
fn an_object_and_a_value_are_not_read_as_each_other() {
    expect(
        from_native_string(
            " Begin Frame\n Naxes = 1\n Title =\n Begin Axis\n End Axis\n Ax1 =\n Begin Axis\n End Axis\n End Frame\n",
        ),
        ErrorCode::Badin,
        &[
            "astRead(Channel): The Object \"title = <Axis>\" cannot be read as a string.",
            "Error while reading a Frame from a Channel.",
        ],
    );
    expect(
        from_native_string(" Begin Frame\n Naxes = 1\n Ax1 = 3\n End Frame\n"),
        ErrorCode::Badin,
        &[
            "astRead(Channel): The value \"ax1 = 3\" cannot be read as an Object.",
            "Error while reading a Frame from a Channel.",
        ],
    );
    let dump = " Begin Frame\n Naxes = 1\n Title = 3\n Ax1 =\n Begin Axis\n End Axis\n End Frame\n";
    let AnyObject::BasicFrame(frame) = from_native_string(dump).unwrap() else {
        panic!("not a Frame");
    };
    assert_eq!(starlink_ast::frames::Frame::title(&frame).unwrap(), "3");
}

#[test]
fn the_first_item_c_reads_gives_the_error() {
    expect(
        from_native_string(
            " Begin Frame\n Naxes = 1\n MchEnd = x\n MaxAx = y\n Ax1 =\n Begin Axis\n End Axis\n End Frame\n",
        ),
        ErrorCode::Badin,
        &[
            "astRead(Channel): The value \"maxax = y\" cannot be read as an integer.",
            "Error while reading a Frame from a Channel.",
        ],
    );
}

#[test]
fn a_nested_failure_names_the_item_once() {
    let points = " Points =\n Begin PointSet\n Npoint = 2\n Ncoord = 2\n X1 = 0\n X2 = 0\n X3 = 1\n X4 = 0\n End PointSet\n";
    let frame = FRAME2.replace(" Naxes = 2\n", " Naxes = 2\n MchEnd = 1.5\n");
    expect(
        from_native_string(&format!(
            " Begin Circle\n IsA Frame\n Frm =\n{frame} End Frame\n{points} IsA Region\n End Circle\n"
        )),
        ErrorCode::Badin,
        &[
            "astRead(Channel): The value \"mchend = 1.5\" cannot be read as an integer.",
            "Failed to read the \"frm\" Object value.",
            "Error while reading a Circle from a Channel.",
        ],
    );
    let unit = " Begin UnitMap\n Nin = 1\n IsA Mapping\n End UnitMap\n";
    let zoom = " Begin ZoomMap\n Nin = 1\n IsA Mapping\n Zoom = abc\n End ZoomMap\n";
    expect(
        from_native_string(&format!(
            " Begin CmpMap\n Nin = 1\n IsA Mapping\n MapA =\n Begin CmpMap\n Nin = 1\n IsA Mapping\n \
             MapA =\n{zoom} MapB =\n{unit} End CmpMap\n MapB =\n{unit} End CmpMap\n"
        )),
        ErrorCode::Badin,
        &[
            "astRead(Channel): The value \"zoom = abc\" cannot be read as a double precision \
             floating point number.",
            "Failed to read the \"mapa\" Object value.",
            "Error while reading a CmpMap from a Channel.",
        ],
    );
}

/// C cuts the message at 1023 characters to fit its buffer; the port keeps
/// it whole.
#[test]
fn a_long_value_is_quoted_in_full() {
    let digits = "7".repeat(5000);
    expect(
        from_native_string(&format!(" Begin Frame\n Naxes = {digits}\n End Frame\n")),
        ErrorCode::Badin,
        &[
            &format!(
                "astRead(Channel): The value \"naxes = {digits}\" cannot be read as an integer."
            ),
            "Error while reading a Frame from a Channel.",
        ],
    );
}

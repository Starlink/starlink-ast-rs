// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A FITS header may hold only printable ASCII characters (FITS Standard
//! 4.0, section 4.1.1), so a FitsChan refuses any other character in a card
//! it is given, and refuses to write an Object whose description would put
//! one in a card.
//!
//! The C library takes any bytes. As it does, a card ends at a NUL and the
//! control characters that end a line, such as a carriage return, are not
//! part of it.

use starlink_ast::error::{AstError, AstResult, ErrorCode};
use starlink_ast::frames::BasicFrameBuilder;
use starlink_ast::{AnyObject, Channel, Encoding, FitsChan, StringSource};

fn assert_refused<T: std::fmt::Debug>(result: AstResult<T>, shown: &str) {
    let err: AstError = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::Bdfts, "{err}");
    let text = err.to_string();
    assert!(text.contains("printable ASCII"), "{text}");
    assert!(text.contains(shown), "{text}");
}

#[test]
fn a_card_holding_a_character_that_is_not_printable_ascii_is_refused() {
    let mut fc = FitsChan::new().unwrap();
    assert_refused(fc.put_fits("OBJECT  = 'caf\u{e9}'", false), "U+00E9");
    assert_refused(fc.put_fits("OBJECT  = 'a\tb'", false), "U+0009");
    assert_refused(fc.put_fits("COMMENT caf\u{e9}", false), "U+00E9");
    assert_refused(fc.put_fits("CAF\u{c9}    = 1", false), "U+00C9");
    assert_eq!(fc.ncard().unwrap(), 0);
}

#[test]
fn a_card_ends_at_a_nul_or_at_the_control_characters_ending_a_line() {
    let mut fc = FitsChan::new().unwrap();
    fc.put_fits("OBJECT  = 'M31'\r\n", false).unwrap();
    fc.put_fits("EQUINOX =               2000.0\0caf\u{e9}", false)
        .unwrap();
    assert_eq!(fc.ncard().unwrap(), 2);
}

#[test]
fn a_value_comment_or_name_given_to_a_setter_must_be_printable_ascii() {
    let mut fc = FitsChan::new().unwrap();
    assert_refused(fc.set_fits_s("OBJECT", "caf\u{e9}", "", false), "U+00E9");
    assert_refused(fc.set_fits_cn("OBJECT", "caf\u{e9}", "", false), "U+00E9");
    assert_refused(fc.set_fits_s("OBJECT", "M31", "caf\u{e9}", false), "U+00E9");
    assert_refused(fc.set_fits_f("CAF\u{c9}", 1.0, "", false), "U+00C9");
    assert_eq!(fc.ncard().unwrap(), 0);
}

#[test]
fn a_header_or_source_holding_a_character_that_is_not_printable_ascii_is_refused() {
    assert_refused(
        FitsChan::from_header("OBJECT  = 'M31'\nOBSERVER= 'Andr\u{e9}'\n"),
        "U+00E9",
    );
    let mut fc = FitsChan::new().unwrap();
    fc.set_source(Box::new(StringSource::new("OBSERVER= 'Andr\u{e9}'\n")));
    assert_refused(fc.ncard(), "U+00E9");
}

#[test]
fn an_object_whose_description_is_not_printable_ascii_is_not_written() {
    let mut frame = BasicFrameBuilder::new(2).build().unwrap();
    frame.set_title("Flux in \u{b5}Jy");
    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::Native);
    assert_refused(fc.write(&AnyObject::BasicFrame(frame)), "U+00B5");
    assert_eq!(fc.ncard().unwrap(), 0);
}

/// A setter checks the text as the card would hold it: a commentary card
/// keeps the whole of its text, while the comment of a card with a value
/// loses the white space at each end.
#[test]
fn a_comment_is_checked_as_the_card_would_hold_it() {
    let mut fc = FitsChan::new().unwrap();
    assert_refused(
        fc.set_fits_cm(&format!("{}\u{a0}", "a".repeat(71)), false),
        "U+00A0",
    );
    assert_refused(fc.set_fits_cm("\u{2003}note", false), "U+2003");
    assert_refused(fc.set_fits_cm("note\t", false), "U+0009");
    assert_eq!(fc.ncard().unwrap(), 0);

    fc.set_fits_f("CRVAL1", 1.0, "\u{2003}deg\t\u{a0}", false)
        .unwrap();
    let header = fc.to_header().unwrap();
    assert!(header.is_ascii() && !header.contains('\t'), "{header:?}");
    assert!(header.contains("/ deg "), "{header:?}");
}

/// A FitsChan read from a dump holds only cards a FitsChan would accept.
#[test]
fn a_fitschan_dump_whose_cards_are_not_printable_ascii_is_refused() {
    let dump = |field: &str| {
        format!(
            " Begin FitsChan\n Card = 1\n Nm1 = \"CRVAL1\"\n {field}\n \
             Ty1 = \"floating point\"\n Dt1 = 1.5\n End FitsChan\n"
        )
    };
    assert_refused(
        starlink_ast::from_native_string(&dump(&format!("Cm1 = \"{}\"", "\u{e9}".repeat(60)))),
        "U+00E9",
    );
    assert_refused(
        starlink_ast::from_native_string(&dump("Cm1 = \"a\tb\"")),
        "U+0009",
    );
    assert!(starlink_ast::from_native_string(&dump("Cm1 = \"deg\"")).is_ok());
}

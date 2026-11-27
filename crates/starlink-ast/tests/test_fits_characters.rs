// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A FITS header may hold only printable ASCII characters (FITS Standard
//! 4.0, section 4.1.1). A FitsChan refuses to make a card holding any other
//! character, from a setter or from an Object whose description would put
//! one in a card, but stores any card it reads, as the C library does and as
//! `astropy.io.fits` does: headers in archives hold such characters in text
//! that has nothing to do with the WCS, and refusing the header would make
//! the WCS unreadable.
//!
//! As in C, a card ends at a NUL and the control characters that end a line,
//! such as a carriage return, are not part of it.

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

/// A card read is stored whatever it holds, and written back as it is.
/// As in C, whose `isprint` takes a byte outside ASCII to be non-printing,
/// such a character at the end of a card is cut with the trailing blanks.
#[test]
fn a_card_holding_a_character_that_is_not_printable_ascii_is_stored() {
    let mut fc = FitsChan::new().unwrap();
    fc.put_fits("OBJECT  = 'caf\u{e9}'", false).unwrap();
    fc.put_fits("COMMENT caf\u{e9} au lait", false).unwrap();
    fc.put_fits("HISTORY tab\there", false).unwrap();
    fc.put_fits("COMMENT caf\u{e9}", false).unwrap();
    assert_eq!(fc.ncard().unwrap(), 4);
    assert_eq!(
        fc.get_fits_s(Some("OBJECT")).unwrap().as_deref(),
        Some("caf\u{e9}")
    );
    assert!(fc.warning_texts().is_empty(), "{:?}", fc.warning_texts());
    let header = fc.to_header().unwrap();
    assert!(header.contains("'caf\u{e9}"), "{header:?}");
    assert!(header.contains("COMMENT caf\u{e9} au lait"), "{header:?}");
    assert!(header.contains("HISTORY tab\there"), "{header:?}");
    assert!(header.contains("COMMENT caf  "), "{header:?}");
}

/// A keyword name holding such a character is warned of, as C's
/// `CheckFitsName` warns of any illegal character, and the card is stored.
/// C warns twice, once from `PutFits` and once from the `SetFits<X>` it
/// hands the card to, and stores the ASTWARN block once.
#[test]
fn a_keyword_name_holding_such_a_character_is_warned_of_and_stored() {
    let mut fc = FitsChan::new().unwrap();
    fc.put_fits("CAF\u{c9}    = 1", false).unwrap();
    let warning = "The FITS keyword name ('CAF\u{c9}') contains an illegal character ('\u{c9}').";
    assert_eq!(fc.warning_texts(), [warning, warning]);
    let header = fc.to_header().unwrap();
    assert!(header.contains("CAF\u{c9}    = "), "{header:?}");
    assert_eq!(
        fc.ncard().unwrap(),
        5,
        "the ASTWARN block of four cards, then the card"
    );
}

#[test]
fn a_card_ends_at_a_nul_or_at_the_control_characters_ending_a_line() {
    let mut fc = FitsChan::new().unwrap();
    fc.put_fits("OBJECT  = 'M31'\r\n", false).unwrap();
    fc.put_fits("EQUINOX =               2000.0\0caf\u{e9}", false)
        .unwrap();
    assert_eq!(fc.ncard().unwrap(), 2);
    assert!(fc.to_header().unwrap().is_ascii());
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

/// A header or source holding such a character reads, with every card, and
/// the WCS it describes is read from it.
#[test]
fn a_header_or_source_holding_a_character_that_is_not_printable_ascii_is_read() {
    let header = "NAXIS   =                    2\n\
                  NAXIS1  =                  100\n\
                  NAXIS2  =                  100\n\
                  OBSERVER= 'Andr\u{e9}'\n\
                  CTYPE1  = 'RA---TAN'\n\
                  CTYPE2  = 'DEC--TAN'\n\
                  CRVAL1  =                 10.0\n\
                  CRVAL2  =                 20.0\n\
                  CRPIX1  =                 50.0\n\
                  CRPIX2  =                 50.0\n\
                  CDELT1  =               -0.001\n\
                  CDELT2  =                0.001\n\
                  COMMENT Observed from Cerro Tololo, caf\u{e9} in hand\n";
    let mut fc = FitsChan::from_header(header).unwrap();
    assert_eq!(fc.ncard().unwrap(), 13);
    assert!(matches!(fc.read().unwrap(), Some(AnyObject::FrameSet(_))));

    let mut fc = FitsChan::new().unwrap();
    fc.set_source(Box::new(StringSource::new(header)));
    assert_eq!(fc.ncard().unwrap(), 13);
    assert!(matches!(fc.read().unwrap(), Some(AnyObject::FrameSet(_))));
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

/// A FitsChan read from a dump holds the cards the dump describes, whatever
/// they hold, as in C.
#[test]
fn a_fitschan_dump_whose_cards_are_not_printable_ascii_is_read() {
    let dump = |field: &str| {
        format!(
            " Begin FitsChan\n Card = 1\n Nm1 = \"CRVAL1\"\n {field}\n \
             Ty1 = \"floating point\"\n Dt1 = 1.5\n End FitsChan\n"
        )
    };
    for field in ["Cm1 = \"caf\u{e9}\"", "Cm1 = \"a\tb\"", "Cm1 = \"deg\""] {
        let AnyObject::FitsChan(mut fc) = starlink_ast::from_native_string(&dump(field)).unwrap()
        else {
            panic!("{field}: not a FitsChan");
        };
        assert_eq!(fc.ncard().unwrap(), 1, "{field}");
    }
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Tests for the keyword-card variant of NATIVE encoding (BEGAST/ENDAST cards).
//!
//! The C library accepts NATIVE-encoded headers in two storage forms:
//!   1. COMMENT-card variant: native text inside `COMMENT  AST +++  AST` markers
//!   2. Keyword-card variant: BEGAST_xx / ENDAST_xx / data-keyword cards
//!
//! Variant 2 is what `polco.head`, `polco2.head`, `specflux.head`,
//! `timeplot.head` use. This test exercises that path end-to-end.

use std::path::PathBuf;

use starlink_ast::AnyObject;
use starlink_ast::channel::Channel;
use starlink_ast::fitschan::FitsChan;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv/inputs")
        .join(name)
}

fn read_fixture(name: &str) -> AnyObject {
    let text =
        std::fs::read_to_string(fixture_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    let mut fc = FitsChan::from_header(&text).unwrap_or_else(|e| panic!("from_header {name}: {e}"));
    fc.read()
        .unwrap_or_else(|e| panic!("read {name}: {e}"))
        .unwrap_or_else(|| panic!("read {name}: returned None"))
}

#[test]
fn polco_loads() {
    let _obj = read_fixture("polco.head");
    // Just verify it loads without error. Identity / value checks are out
    // of scope for the encoding-decoder test.
}

#[test]
fn polco2_loads() {
    let _obj = read_fixture("polco2.head");
}

#[test]
fn specflux_loads() {
    let _obj = read_fixture("specflux.head");
}

#[test]
fn timeplot_loads() {
    let _obj = read_fixture("timeplot.head");
}

/// A successful NATIVE read consumes the cards it read, as C `Read` does by
/// calling `FixUsed` with `remove` set for NATIVE encoding
/// (fitschan.c:25923-25929). The AST banner COMMENT blocks go with them
/// (C `ComBlock`, fitschan.c:7972). Each header has one blank line, a blank
/// card the read does not use; C leaves exactly that card (Ncard 1 after the
/// read, from a probe of the reference library).
#[test]
fn native_read_consumes_its_cards() {
    for name in ["specflux.head", "timeplot.head", "polco.head"] {
        let text = std::fs::read_to_string(fixture_path(name))
            .unwrap_or_else(|e| panic!("read {name}: {e}"));
        let mut fc =
            FitsChan::from_header(&text).unwrap_or_else(|e| panic!("from_header {name}: {e}"));
        assert!(fc.ncard().unwrap() > 0, "{name}: no cards to start with");
        let _obj = fc
            .read()
            .unwrap_or_else(|e| panic!("read {name}: {e}"))
            .unwrap_or_else(|| panic!("read {name}: returned None"));
        assert_eq!(
            fc.ncard().unwrap(),
            1,
            "{name}: cards left after a NATIVE read"
        );
    }
}

/// A header in the keyword-card NATIVE encoding holding `cards`, each the
/// text of one card, between the `BEGAST` and `ENDAST` cards of `class`.
fn native_header(class: &str, cards: &[&str]) -> String {
    let mut header = format!("BEGAST_A= '{class}'\n");
    for card in cards {
        header.push_str(card);
        header.push('\n');
    }
    header.push_str("ENDAST_A= '");
    header.push_str(class);
    header.push_str("'\n");
    header
}

/// The error reading the first Object of `header`, and the error reading the
/// same Object from a Channel, with each Channel-level context removed, so
/// that the two can be compared.
fn fits_and_channel_errors(class: &str, cards: &[&str], dump: &str) -> (String, String) {
    let mut fc = FitsChan::from_header(&native_header(class, cards)).unwrap();
    let fits_err = fc.read().unwrap_err().to_string();
    let mut ch = starlink_ast::NativeChannel::from_string(dump);
    let chan_err = ch.read().unwrap_err().to_string();
    (
        fits_err
            .replace("from a FitsChan", "from a Channel")
            .replace("astRead(FitsChan)", "astRead(Channel)"),
        chan_err,
    )
}

/// A value the loader cannot read as an integer fails a NATIVE read of a
/// FitsChan as it fails a Channel: the value is checked as text, not
/// truncated or defaulted, and the message is the C library's.
#[test]
fn a_fractional_integer_value_fails_the_read_as_in_a_channel() {
    let (fits_err, chan_err) = fits_and_channel_errors(
        "UnitMap",
        &["NIN_A   =                  2.5", "ISA_A   = 'Mapping'"],
        " Begin UnitMap\n    Nin = 2.5\n IsA Mapping\n End UnitMap\n",
    );
    assert!(
        fits_err.contains("cannot be read as an integer"),
        "{fits_err}"
    );
    assert_eq!(fits_err, chan_err);
}

#[test]
fn a_string_axis_count_fails_the_read_as_in_a_channel() {
    let (fits_err, chan_err) = fits_and_channel_errors(
        "Frame",
        &["NAXES_A = 'abc'"],
        " Begin Frame\n    Naxes = \"abc\"\n End Frame\n",
    );
    assert!(
        fits_err.contains("cannot be read as an integer"),
        "{fits_err}"
    );
    assert_eq!(fits_err, chan_err);
}

/// An item no loader reads is reported in the FitsChan's warnings, as the
/// Channel reports it, and the read goes on.
#[test]
fn an_unread_item_is_a_warning_as_in_a_channel() {
    use starlink_ast::channel::Channel as _;
    let mut fc = FitsChan::from_header(&native_header(
        "UnitMap",
        &[
            "NIN_A   =                    2",
            "FOO_A   =                    1",
            "ISA_A   = 'Mapping'",
        ],
    ))
    .unwrap();
    assert!(matches!(fc.read().unwrap(), Some(AnyObject::UnitMap(_))));
    let mut ch = starlink_ast::NativeChannel::from_string(
        " Begin UnitMap\n    Nin = 2\n    Foo = 1\n IsA Mapping\n End UnitMap\n",
    );
    assert!(matches!(ch.read().unwrap(), Some(AnyObject::UnitMap(_))));
    assert_eq!(fc.warning_texts(), ch.warning_texts());
    assert_eq!(fc.warning_texts().len(), 1, "{:?}", fc.warning_texts());
}

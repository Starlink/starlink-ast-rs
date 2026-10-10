//! `MocChan`: writing Mocs as text and reading them back.
//!
//! C ancestor: `reference/starlink-ast/ast_tester/testmocchan.c`. A Moc of
//! a few cells is written in string and then JSON form, the lines are
//! checked and read back to an equal Moc, and the `MocFormat` a read leaves
//! is checked; then a Moc made from a small Circle makes the same round
//! trips. The checks added upstream are `zero_line_len_is_too_small`
//! (`checkZeroLineLen`, `7ef81c01`), `dump_names_the_format`
//! (`checkDumpFormat`, `7ef81c01`) and `negative_line_len_loads_as_zero`
//! (`checkLoadNegativeLineLen`, `228a0310`).
//!
//! Divergence from C: the lines go to an in-memory sink and are read back
//! through `MocChan::from_string`, rather than through source and sink
//! functions over a static array. The C sink drops blank lines; none is
//! written.

use ast_core::error::ErrorCode;
use ast_core::frames::{SkyFrame, SkySystem};
use ast_core::mappings::Mapping;
use ast_core::regions::{CircleBuilder, CmpOperator, Moc};
use ast_core::{AnyObject, Channel, MocChan, MocFormat, from_native_string, to_native_string};

/// Write `moc` through a MocChan with the given format, returning the lines.
fn write(moc: &Moc, format: MocFormat) -> String {
    let mut ch = MocChan::with_string_sink();
    ch.set_moc_format(format);
    assert_eq!(ch.write(&AnyObject::Moc(moc.clone())).unwrap(), 1);
    ch.into_string()
}

/// Read a Moc from `text`, returning it and the MocFormat left behind.
fn read(text: &str) -> (Moc, MocFormat) {
    let mut ch = MocChan::from_string(text);
    let Some(AnyObject::Moc(moc)) = ch.read().unwrap() else {
        panic!("expected a Moc");
    };
    (moc, ch.moc_format())
}

#[test]
fn cells_round_trip() {
    let mut moc = Moc::new();
    moc.set_max_order(9).unwrap();
    for (order, npix) in [(7, 1000), (7, 1001), (7, 1002), (7, 997), (6, 500)] {
        moc.add_cell(CmpOperator::Or, order, npix).unwrap();
    }

    // Errors 1-3: the default format is strings, and is not set.
    let ch = MocChan::new();
    assert!(!ch.test_moc_format());
    let text = write(&moc, ch.moc_format());
    assert_eq!(text, "6/500 7/997,1000-1002 9/\n");

    // Errors 4-7.
    let (back, format) = read(&text);
    assert!(back.equal(&moc));
    assert_eq!(format, MocFormat::String);

    // Errors 8-9.
    let text = write(&moc, MocFormat::Json);
    assert_eq!(text, "{\"6\":[500],\"7\":[997,1000,1001,1002],\"9\":[]}\n");

    // Errors 10-12.
    let (back, format) = read(&text);
    assert!(back.equal(&moc));
    assert_eq!(format, MocFormat::Json);
}

#[test]
fn circle_round_trip() {
    let mut moc = Moc::new();
    moc.set_max_order(15).unwrap();
    moc.set_min_order(12);
    let circle = CircleBuilder::new(SkyFrame::new(SkySystem::Icrs), vec![1.0, 1.0])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();
    moc.add_region(CmpOperator::Or, &circle).unwrap();

    // Errors 13-18.
    for format in [MocFormat::Unknown, MocFormat::Json] {
        let (back, _) = read(&write(&moc, format));
        assert!(back.equal(&moc), "{format:?}");
    }
}

#[test]
fn zero_line_len_is_too_small() {
    let mut moc = Moc::new();
    moc.add_cell(CmpOperator::Or, 3, 5).unwrap();
    let mut ch = MocChan::with_string_sink();
    ch.set_moc_line_len(-5);
    assert_eq!(ch.moc_line_len(), 0);
    let err = ch.write(&AnyObject::Moc(moc)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Smbuf);
}

#[test]
fn dump_names_the_format() {
    for (format, name) in [(MocFormat::String, "STRING"), (MocFormat::Json, "JSON")] {
        let mut ch = MocChan::new();
        ch.set_moc_format(format);
        let text = to_native_string(&AnyObject::MocChan(Box::new(ch))).unwrap();
        assert!(text.contains(&format!("MocEnc = \"{name}\"")), "{text}");
        let AnyObject::MocChan(back) = from_native_string(&text).unwrap() else {
            panic!("expected a MocChan");
        };
        assert_eq!(back.moc_format(), format);
    }
}

#[test]
fn negative_line_len_loads_as_zero() {
    let AnyObject::MocChan(ch) =
        from_native_string(" Begin MocChan\n    MocLLn = -3\n End MocChan").unwrap()
    else {
        panic!("expected a MocChan");
    };
    assert_eq!(ch.moc_line_len(), 0);
}

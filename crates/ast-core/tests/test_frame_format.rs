//! Frame `Format` / `Unformat` tests — printf-style formatting with the
//! `Digits` attribute and the C `astUnformat` whitespace/error contract
//! (frame.c:4260, axis.c:626 / 1284).

use ast_core::frames::{BasicFrameBuilder, Frame};

#[test]
fn format_default_is_g_7() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    // Default format "%1.7G".
    assert_eq!(f.format(1, 1234.5678).unwrap(), "1234.568");
    assert_eq!(f.format(1, 123456789.0).unwrap(), "1.234568E+08");
}

#[test]
fn format_bad_is_bad_token() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    assert_eq!(
        f.format(1, ast_core::constants::BAD_VALUE).unwrap(),
        "<bad>"
    );
}

#[test]
fn format_honours_digits() {
    let mut f = BasicFrameBuilder::new(1).build().unwrap();
    f.set_digits(3);
    // "%1.3G" of 1234.5678 -> "1.23E+03".
    assert_eq!(f.format(1, 1234.5678).unwrap(), "1.23E+03");
}

#[test]
fn format_explicit_format_string() {
    let mut f = BasicFrameBuilder::new(1).build().unwrap();
    f.set_format_str(1, "%.2f").unwrap();
    assert_eq!(f.format(1, 9.87654).unwrap(), "9.88");
}

#[test]
fn format_precision_less_float_defaults_to_six() {
    // A user-set, precision-less float Format uses C printf's default of
    // 6 fraction digits, NOT Digits. C `ParseAxisFormat` passes such a
    // spec to `sprintf` unchanged. Verified against `printf("%E\n", 5.0)`
    // -> "5.000000E+00".
    let mut f = BasicFrameBuilder::new(1).build().unwrap();
    f.set_format_str(1, "%E").unwrap();
    assert_eq!(f.format(1, 5.0).unwrap(), "5.000000E+00");
    f.set_format_str(1, "%f").unwrap();
    assert_eq!(f.format(1, 5.0).unwrap(), "5.000000");
    // The unset/default path is unaffected: still 7-sig-fig %G.
    let g = BasicFrameBuilder::new(1).build().unwrap();
    assert_eq!(g.format(1, 1234.5678).unwrap(), "1234.568");
    assert_eq!(g.format(1, 123456789.0).unwrap(), "1.234568E+08");
}

#[test]
fn unformat_reads_number_and_trailing_ws() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    // C `sscanf("%lf %n")` counts trailing whitespace in nread.
    let (v, n) = f.unformat(1, "12.5  ").unwrap();
    assert_eq!(v, 12.5);
    assert_eq!(n, 6);
}

#[test]
fn unformat_failure_is_nread_zero_no_error() {
    let f = BasicFrameBuilder::new(1).build().unwrap();
    // C returns nread=0 with NO error on a non-numeric string.
    let (_v, n) = f.unformat(1, "xyz").unwrap();
    assert_eq!(n, 0);
}

#[test]
fn unformat_recognises_bad_token() {
    let f = BasicFrameBuilder::new(1).build().unwrap();

    // C `sscanf(" <bad> ", " < %*1[Bb] %*1[Aa] %*1[Dd] > %n", &nc)` sets
    // nc=7: the trailing space is matched and counted by the format's
    // space-before-`%n`.
    let (v, n) = f.unformat(1, " <bad> ").unwrap();
    assert_eq!(v, ast_core::constants::BAD_VALUE);
    assert_eq!(n, 7);

    // Whitespace is allowed before each token; "  < b a d >  " gives
    // nc=13 (verified against the same C scanf format).
    let (v, n) = f.unformat(1, "  < b a d >  ").unwrap();
    assert_eq!(v, ast_core::constants::BAD_VALUE);
    assert_eq!(n, 13);
}

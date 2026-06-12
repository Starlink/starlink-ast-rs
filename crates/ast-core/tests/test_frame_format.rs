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
    assert_eq!(f.format(1, 3.14159).unwrap(), "3.14");
}

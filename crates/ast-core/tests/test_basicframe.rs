//! Rust equivalent of reference/starlink-ast/ast_tester/testframe.c
//! Covers: construction, per-axis attributes, Euclidean geometry, format/unformat.

use std::f64::consts::PI;

use ast_core::constants::BAD_VALUE;
use ast_core::error::ErrorCode;
use ast_core::frames::{BasicFrameBuilder, Frame};

// ---- Construction ----

#[test]
fn builder_basic() {
    let frame = BasicFrameBuilder::new(3)
        .title("Test Frame")
        .domain("TEST")
        .build()
        .unwrap();
    assert_eq!(frame.naxes(), 3);
    assert_eq!(frame.title(), "Test Frame");
    assert_eq!(frame.domain(), "TEST");
}

#[test]
fn builder_zero_axes_rejected() {
    let result = BasicFrameBuilder::new(0).build();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::Naxin);
}

#[test]
fn builder_with_labels() {
    let frame = BasicFrameBuilder::new(2)
        .label(1, "X")
        .label(2, "Y")
        .unit(1, "m")
        .unit(2, "m")
        .build()
        .unwrap();
    assert_eq!(frame.label(1).unwrap(), "X");
    assert_eq!(frame.label(2).unwrap(), "Y");
    assert_eq!(frame.unit(1).unwrap(), "m");
    assert_eq!(frame.unit(2).unwrap(), "m");
}

// ---- Attribute defaults ----

#[test]
fn default_attributes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    assert_eq!(frame.title(), "");
    assert_eq!(frame.domain(), "");
    assert_eq!(frame.epoch(), None);
    assert_eq!(frame.label(1).unwrap(), "");
    assert_eq!(frame.unit(1).unwrap(), "");
    assert_eq!(frame.symbol(1).unwrap(), "");
    assert_eq!(frame.format_str(1).unwrap(), "");
}

// ---- Set / test / clear cycle ----

#[test]
fn set_clear_attributes() {
    let mut frame = BasicFrameBuilder::new(2).build().unwrap();

    // Initially unset.
    assert!(!frame.test_label(1).unwrap());
    assert_eq!(frame.label(1).unwrap(), "");

    // Set.
    frame.set_label(1, "RA").unwrap();
    assert!(frame.test_label(1).unwrap());
    assert_eq!(frame.label(1).unwrap(), "RA");

    // Clear.
    frame.clear_label(1).unwrap();
    assert!(!frame.test_label(1).unwrap());
    assert_eq!(frame.label(1).unwrap(), "");
}

// ---- Axis validation ----

#[test]
fn axis_validation() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();

    // Axis 0 is invalid (1-based).
    let err = frame.label(0).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Axiin);

    // Axis 3 is out of range for a 2-axis frame.
    let err = frame.label(3).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Axiin);
}

// ---- Epoch attribute ----

#[test]
fn epoch_attribute() {
    let mut frame = BasicFrameBuilder::new(1).build().unwrap();

    assert!(!frame.test_epoch());
    assert_eq!(frame.epoch(), None);

    frame.set_epoch(2000.0);
    assert!(frame.test_epoch());
    assert_eq!(frame.epoch(), Some(2000.0));

    frame.clear_epoch();
    assert!(!frame.test_epoch());
    assert_eq!(frame.epoch(), None);
}

// ---- Euclidean distance ----

#[test]
fn distance_2d() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let d = frame.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-12);
}

#[test]
fn distance_3d() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let d = frame.distance(&[1.0, 2.0, 3.0], &[4.0, 6.0, 3.0]).unwrap();
    // sqrt(9 + 16 + 0) = 5
    assert!((d - 5.0).abs() < 1e-12);
}

#[test]
fn distance_bad_value() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let d = frame.distance(&[BAD_VALUE, 0.0], &[3.0, 4.0]).unwrap();
    assert_eq!(d, BAD_VALUE);
}

// ---- Angle ----

#[test]
fn angle_right_angle() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let a = &[1.0, 0.0];
    let b = &[0.0, 0.0];
    let c = &[0.0, 1.0];
    let angle = frame.angle(a, b, c).unwrap();
    assert!((angle - PI / 2.0).abs() < 1e-12);
}

#[test]
fn angle_straight_line() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let a = &[-1.0, 0.0];
    let b = &[0.0, 0.0];
    let c = &[1.0, 0.0];
    let angle = frame.angle(a, b, c).unwrap();
    assert!((angle - PI).abs() < 1e-12);
}

// ---- Offset ----

#[test]
fn offset_2d() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = frame.offset(&[0.0, 0.0], &[3.0, 4.0], 5.0).unwrap();
    assert!((result[0] - 3.0).abs() < 1e-12);
    assert!((result[1] - 4.0).abs() < 1e-12);
}

#[test]
fn offset_half_distance() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = frame.offset(&[0.0, 0.0], &[4.0, 0.0], 2.0).unwrap();
    assert!((result[0] - 2.0).abs() < 1e-12);
    assert!((result[1]).abs() < 1e-12);
}

// ---- Norm ----

#[test]
fn norm_is_noop() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut coords = [3.14, 2.72];
    frame.norm(&mut coords).unwrap();
    assert_eq!(coords, [3.14, 2.72]);
}

// ---- Format ----

#[test]
fn format_default() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let s = frame.format(1, 3.14159).unwrap();
    assert!(s.contains("3.14159"));
}

#[test]
fn format_bad_value() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let s = frame.format(1, BAD_VALUE).unwrap();
    assert_eq!(s, "<bad>");
}

#[test]
fn format_with_precision() {
    let mut frame = BasicFrameBuilder::new(1).build().unwrap();
    frame.set_format_str(1, "3").unwrap();
    let s = frame.format(1, 3.14159).unwrap();
    assert_eq!(s, "3.142");
}

// ---- Unformat ----

#[test]
fn unformat_basic() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let (val, consumed) = frame.unformat(1, "3.14 rest").unwrap();
    assert!((val - 3.14).abs() < 1e-12);
    assert_eq!(consumed, 4);
}

#[test]
fn unformat_with_leading_whitespace() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let (val, consumed) = frame.unformat(1, "  42.0").unwrap();
    assert!((val - 42.0).abs() < 1e-12);
    assert_eq!(consumed, 6);
}

#[test]
fn unformat_invalid() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let result = frame.unformat(1, "abc");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), ErrorCode::Unfer);
}

// ---- Snapshot ----

#[test]
fn snapshot_is_independent_clone() {
    let frame = BasicFrameBuilder::new(3)
        .title("Original")
        .build()
        .unwrap();
    let snap = frame.snapshot();
    assert_eq!(snap.naxes(), 3);
}

// ---- map_from ----

#[test]
fn map_from_returns_unit() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let other = BasicFrameBuilder::new(2).build().unwrap();
    let mapping = frame.map_from(&other).unwrap();
    assert_eq!(mapping.nin(), 2);
    assert_eq!(mapping.nout(), 2);
}

// ---- Bottom / Top defaults ----

#[test]
fn bottom_top_defaults() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    assert_eq!(frame.bottom(1).unwrap(), -f64::MAX);
    assert_eq!(frame.top(1).unwrap(), f64::MAX);
    assert_eq!(frame.bottom(2).unwrap(), -f64::MAX);
    assert_eq!(frame.top(2).unwrap(), f64::MAX);
}

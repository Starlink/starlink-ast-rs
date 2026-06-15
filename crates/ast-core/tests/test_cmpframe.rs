//! Rust equivalent of reference/starlink-ast/ast_tester/testcmpframe.c
//! Covers: axis concatenation, routing, geometry, format/unformat delegation.

#![allow(clippy::approx_constant)] // 3.14 is intentional opaque test value, not π

use std::f64::consts::PI;

use ast_core::constants::BAD_VALUE;
use ast_core::error::ErrorCode;
use ast_core::frames::{BasicFrameBuilder, CmpFrame, Frame};

fn make_frame(naxes: usize) -> Box<dyn Frame> {
    Box::new(BasicFrameBuilder::new(naxes).build().unwrap())
}

// ---- Axis concatenation ----

#[test]
fn cmpframe_naxes() {
    let cmp = CmpFrame::new(make_frame(2), make_frame(3));
    assert_eq!(cmp.naxes(), 5);
    assert_eq!(cmp.naxes1(), 2);
}

// ---- Format delegation ----

#[test]
fn cmpframe_format_delegates() {
    let cmp = CmpFrame::new(make_frame(1), make_frame(1));
    // Both children are BasicFrames, so format uses default float formatting.
    let s1 = cmp.format(1, 3.14).unwrap();
    assert!(s1.contains("3.14"));
    let s2 = cmp.format(2, 2.72).unwrap();
    assert!(s2.contains("2.72"));
}

// ---- Direction routing ----

#[test]
fn cmpframe_direction_routes_to_component() {
    // C routes Direction per external axis to the owning component
    // (cmpframe.c MAKE_GET(Direction)). Reverse axis 1 of the first
    // component; the CmpFrame must report Direction(1)=false and the
    // remaining axes the default true. Oracle-verified: Dir(1)=0,
    // Dir(2)=1, Dir(3)=1.
    let mut f1 = BasicFrameBuilder::new(2).build().unwrap();
    f1.set_direction(1, false).unwrap();
    let cmp = CmpFrame::new(Box::new(f1), make_frame(1));
    assert!(!cmp.direction(1)); // reversed axis from frame1
    assert!(cmp.direction(2)); // default
    assert!(cmp.direction(3)); // frame2 default
    // Out-of-range axes fall back to the default.
    assert!(cmp.direction(0));
    assert!(cmp.direction(4));
}

// ---- Unformat delegation ----

#[test]
fn cmpframe_unformat_delegates() {
    // 2+1 compound: axis 3 delegates to frame2 axis 1.
    let cmp = CmpFrame::new(make_frame(2), make_frame(1));
    // C `sscanf("%lf %n")` counts the trailing whitespace after the
    // number, so "42.5 rest" reports 5 characters read.
    let (val, consumed) = cmp.unformat(3, "42.5 rest").unwrap();
    assert!((val - 42.5).abs() < 1e-12);
    assert_eq!(consumed, 5);
}

// ---- Axis validation ----

#[test]
fn cmpframe_axis_validation() {
    let cmp = CmpFrame::new(make_frame(2), make_frame(1));
    let total = cmp.naxes();
    assert_eq!(total, 3);

    // Axis 0 is invalid (1-based).
    let err = cmp.format(0, 1.0).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Axiin);

    // Axis 4 is out of range for a 3-axis compound frame.
    let err = cmp.format(4, 1.0).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Axiin);
}

// ---- Distance ----

#[test]
fn cmpframe_distance() {
    let cmp = CmpFrame::new(make_frame(2), make_frame(1));
    // (0,0,0) -> (1,2,2): sqrt(1+4+4) = 3
    let d = cmp.distance(&[0.0, 0.0, 0.0], &[1.0, 2.0, 2.0]).unwrap();
    assert!((d - 3.0).abs() < 1e-12);
}

#[test]
fn cmpframe_distance_bad_value() {
    let cmp = CmpFrame::new(make_frame(1), make_frame(1));
    let d = cmp.distance(&[BAD_VALUE, 0.0], &[1.0, 1.0]).unwrap();
    assert_eq!(d, BAD_VALUE);
}

// ---- Offset ----

#[test]
fn cmpframe_offset() {
    let cmp = CmpFrame::new(make_frame(1), make_frame(1));
    // (0,0) -> (3,4) by dist 5 = exactly (3,4).
    let result = cmp.offset(&[0.0, 0.0], &[3.0, 4.0], 5.0).unwrap();
    assert!((result[0] - 3.0).abs() < 1e-12);
    assert!((result[1] - 4.0).abs() < 1e-12);
}

// ---- Norm delegation ----

#[test]
fn cmpframe_norm_delegates() {
    let cmp = CmpFrame::new(make_frame(2), make_frame(1));
    let mut coords = [1.0, 2.0, 3.0];
    cmp.norm(&mut coords).unwrap();
    // BasicFrame norm is a no-op; coordinates should be unchanged.
    assert_eq!(coords, [1.0, 2.0, 3.0]);
}

// ---- Snapshot ----

#[test]
fn cmpframe_snapshot() {
    let cmp = CmpFrame::new(make_frame(2), make_frame(3));
    let snap = cmp.snapshot();
    assert_eq!(snap.naxes(), 5);
}

// ---- Axis-permutation native round-trip ----

#[test]
fn cmpframe_axis_permutation_round_trips() {
    // A permuted CmpFrame must serialise its `Axp{n}` tokens and read
    // them back, preserving the external axis order. Mirrors C
    // `CmpFrame::Dump`/`astLoad` (cmpframe.c:10453/11020). Distinct
    // per-axis labels let us confirm each external axis still maps to
    // the right child axis after round-trip.
    use ast_core::frames::Frame;
    use ast_core::{AnyObject, from_native_string, to_native_string};

    let f1 = BasicFrameBuilder::new(2)
        .label(1, "lon")
        .label(2, "lat")
        .build()
        .unwrap();
    let f2 = BasicFrameBuilder::new(1).label(1, "wave").build().unwrap();
    let mut cmp = CmpFrame::new(Box::new(f1), Box::new(f2));

    // Permute so external axes are (wave, lon, lat): external axis 1 ->
    // internal axis 3, 2 -> 1, 3 -> 2.
    cmp.perm_axes(&[3, 1, 2]).unwrap();
    let labels_before: Vec<String> = (1..=3).map(|a| cmp.effective_label(a)).collect();
    assert_eq!(labels_before, ["wave", "lon", "lat"]);

    let text = to_native_string(&AnyObject::CmpFrame(cmp)).unwrap();
    assert!(
        text.contains("Axp1"),
        "permuted CmpFrame must emit Axp tokens:\n{text}"
    );

    let back = match from_native_string(&text).unwrap() {
        AnyObject::CmpFrame(c) => c,
        other => panic!("expected CmpFrame, got {other:?}"),
    };
    let labels_after: Vec<String> = (1..=3).map(|a| back.effective_label(a)).collect();
    assert_eq!(
        labels_after, labels_before,
        "axis permutation must survive native round-trip"
    );
}

// ---- Angle ----

#[test]
fn cmpframe_angle() {
    // All-Cartesian CmpFrame uses the Frame-class angle, which is
    // *signed* in 2 dimensions (frame.c:1230-1320): A on +X to C on
    // +Y is -pi/2.
    let cmp = CmpFrame::new(make_frame(1), make_frame(1));
    let a = &[1.0, 0.0];
    let b = &[0.0, 0.0];
    let c = &[0.0, 1.0];
    let angle = cmp.angle(a, b, c).unwrap();
    assert!((angle + PI / 2.0).abs() < 1e-12, "got {angle}");
}

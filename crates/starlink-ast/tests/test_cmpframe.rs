// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Rust equivalent of reference/starlink-ast/ast_tester/testcmpframe.c
//! Covers: axis concatenation, routing, geometry, format/unformat delegation.

#![allow(clippy::approx_constant)] // 3.14 is intentional opaque test value, not π

use crate::common::native_dump;

use std::f64::consts::{FRAC_PI_2, PI};

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::error::ErrorCode;
use starlink_ast::frames::{
    BasicFrameBuilder, CmpFrame, Frame, SkyFrame, SkySystem, TimeFrame, TimeSystem,
};
use starlink_ast::regions::{CircleBuilder, Region};

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
    assert!(!cmp.direction(1).unwrap()); // reversed axis from frame1
    assert!(cmp.direction(2).unwrap()); // default
    assert!(cmp.direction(3).unwrap()); // frame2 default
    // An axis out of range is an error, as C's astValidateAxis makes it.
    assert!(cmp.direction(0).is_err());
    assert!(cmp.direction(4).is_err());
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
    // A permuted CmpFrame must serialize its `Axp{n}` tokens and read
    // them back, preserving the external axis order. Mirrors C
    // `CmpFrame::Dump`/`astLoad` (cmpframe.c:10453/11020). Distinct
    // per-axis labels let us confirm each external axis still maps to
    // the right child axis after round-trip.
    use starlink_ast::frames::Frame;
    use starlink_ast::{AnyObject, from_native_string};

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
    let labels_before: Vec<String> = (1..=3).map(|a| cmp.label(a).unwrap()).collect();
    assert_eq!(labels_before, ["wave", "lon", "lat"]);

    let text = native_dump(&AnyObject::CmpFrame(cmp)).unwrap();
    assert!(
        text.contains("Axp1"),
        "permuted CmpFrame must emit Axp tokens:\n{text}"
    );

    let back = match from_native_string(&text).unwrap() {
        AnyObject::CmpFrame(c) => c,
        other => panic!("expected CmpFrame, got {other:?}"),
    };
    let labels_after: Vec<String> = (1..=3).map(|a| back.label(a).unwrap()).collect();
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

// ---- NormBox delegation (cmpframe.c:5838) ----

/// The probe C hands each component Frame carries `AST__BAD` on the other
/// component's axes (cmpframe.c:5965-5991). Every Region that calls
/// `astNormBox` passes an un-negated copy of itself, and an un-negated Region
/// reports a point with a bad axis as outside, so neither pole is ever
/// detected and the box comes back untouched. Captured from C `astNormBox` on
/// a CmpFrame(SkyFrame, TimeFrame) with a Box spanning the north pole: the
/// bounds are returned exactly as supplied, where the same box on a plain
/// SkyFrame widens to `0 .. 2*pi` in longitude. A fill with anything but bad
/// values — the box's own coordinates, say — would widen the box here and fail
/// this test; the companion below is what proves the delegation runs at all.
#[test]
fn norm_box_probe_carries_bad_on_the_other_components_axes() {
    let cmp = CmpFrame::new(
        Box::new(SkyFrame::new(SkySystem::Icrs)),
        Box::new(TimeFrame::new(TimeSystem::Mjd)),
    );

    let mut lbnd = [-0.3, 1.3, 0.0];
    let mut ubnd = [0.3, FRAC_PI_2, 1.0];

    // What an un-negated Region's probe reports: a point carrying a bad axis
    // is outside.
    let inside = |point: &[f64]| -> Result<bool, starlink_ast::error::AstError> {
        Ok(!point.contains(&BAD_VALUE))
    };
    cmp.norm_box(&mut lbnd, &mut ubnd, &inside).unwrap();

    assert_eq!(lbnd, [-0.3, 1.3, 0.0]);
    assert_eq!(ubnd, [0.3, FRAC_PI_2, 1.0]);
}

/// A predicate that accepts the bad-carrying probe point — which is what a
/// negated Region does — reaches the SkyFrame's pole widening through the
/// delegation, so the split and the un-permutation are exercised. Captured
/// from C with `Negated` set on the same Box: the sky axes come back as
/// `0 .. 2*pi` and `-pi/2 .. pi/2`, and the time axis is untouched.
#[test]
fn norm_box_reaches_the_sky_component() {
    let cmp = CmpFrame::new(
        Box::new(SkyFrame::new(SkySystem::Icrs)),
        Box::new(TimeFrame::new(TimeSystem::Mjd)),
    );

    let mut lbnd = [-0.3, 1.3, 0.0];
    let mut ubnd = [0.3, FRAC_PI_2, 1.0];

    let inside = |_point: &[f64]| -> Result<bool, starlink_ast::error::AstError> { Ok(true) };
    cmp.norm_box(&mut lbnd, &mut ubnd, &inside).unwrap();

    assert_eq!(lbnd, [0.0, -FRAC_PI_2, 0.0]);
    assert_eq!(ubnd[0], 2.0 * PI);
    assert_eq!(ubnd[1], FRAC_PI_2);
    assert_eq!(ubnd[2], 1.0);
}

/// The second component's probe takes that component's own axis count, so a
/// CmpFrame whose components differ in size delegates to both
/// (cmpframe.c:5981-5991). Captured from C `astNormBox` on a
/// CmpFrame(TimeFrame, SkyFrame) with a box spanning the north pole: the bounds
/// come back exactly as supplied, because the SkyFrame's pole test sees the
/// `AST__BAD` the probe puts on the time axis.
#[test]
fn norm_box_delegates_to_a_second_component_of_a_different_size() {
    let cmp = CmpFrame::new(
        Box::new(TimeFrame::new(TimeSystem::Mjd)),
        Box::new(SkyFrame::new(SkySystem::Icrs)),
    );

    let mut lbnd = [0.0, -0.3, 1.3];
    let mut ubnd = [1.0, 0.3, FRAC_PI_2];

    let inside = |point: &[f64]| -> Result<bool, starlink_ast::error::AstError> {
        Ok(!point.contains(&BAD_VALUE))
    };
    cmp.norm_box(&mut lbnd, &mut ubnd, &inside).unwrap();

    assert_eq!(lbnd, [0.0, -0.3, 1.3]);
    assert_eq!(ubnd, [1.0, 0.3, FRAC_PI_2]);
}

/// A predicate that accepts the bad-carrying probe point reaches the pole
/// widening through the second component too, so the upper split and its
/// un-permutation are exercised.
#[test]
fn norm_box_reaches_a_sky_second_component() {
    let cmp = CmpFrame::new(
        Box::new(TimeFrame::new(TimeSystem::Mjd)),
        Box::new(SkyFrame::new(SkySystem::Icrs)),
    );

    let mut lbnd = [0.0, -0.3, 1.3];
    let mut ubnd = [1.0, 0.3, FRAC_PI_2];

    let inside = |_point: &[f64]| -> Result<bool, starlink_ast::error::AstError> { Ok(true) };
    cmp.norm_box(&mut lbnd, &mut ubnd, &inside).unwrap();

    assert_eq!(lbnd, [0.0, 0.0, -FRAC_PI_2]);
    assert_eq!(ubnd[0], 1.0);
    assert_eq!(ubnd[1], 2.0 * PI);
    assert_eq!(ubnd[2], FRAC_PI_2);
}

/// Region bounds on a CmpFrame that holds a SkyFrame must not depend on which
/// component the SkyFrame is: the same Circle near the north pole reports the
/// same limits on `(SkyFrame, 1-D)` and on `(1-D, SkyFrame)`, with the axes
/// permuted.
///
/// Ancestor: `reference/starlink-ast/ast_tester/testcmpframe.c`,
/// `check_normbox_component_order`.
#[test]
fn norm_box_bounds_do_not_depend_on_component_order() {
    let radius = 0.5;
    let sky_first = CmpFrame::new(
        Box::new(SkyFrame::new(SkySystem::Icrs)),
        Box::new(BasicFrameBuilder::new(1).domain("FPLANE").build().unwrap()),
    );
    let sky_last = CmpFrame::new(
        Box::new(BasicFrameBuilder::new(1).domain("FPLANE").build().unwrap()),
        Box::new(SkyFrame::new(SkySystem::Icrs)),
    );

    let c1 = CircleBuilder::new(sky_first, vec![0.0, 1.2707963267948966, 50.0])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();
    let c2 = CircleBuilder::new(sky_last, vec![50.0, 0.0, 1.2707963267948966])
        .unwrap()
        .radius(radius)
        .build()
        .unwrap();

    let (lb1, ub1) = c1.get_region_bounds().unwrap();
    let (lb2, ub2) = c2.get_region_bounds().unwrap();

    for axis in 0..3 {
        assert!(
            lb1[axis] != BAD_VALUE && ub1[axis] != BAD_VALUE,
            "axis {} of the sky-first bounds is bad",
            axis + 1
        );
    }

    // Axis 3 of `sky_first` is axis 1 of `sky_last`, and axes 1, 2 are 2, 3.
    assert!((lb1[2] - lb2[0]).abs() <= 1.0e-10 && (ub1[2] - ub2[0]).abs() <= 1.0e-10);
    for axis in 0..2 {
        assert!(
            (lb1[axis] - lb2[axis + 1]).abs() <= 1.0e-10
                && (ub1[axis] - ub2[axis + 1]).abs() <= 1.0e-10,
            "sky axis {} depends on component order",
            axis + 1
        );
    }
}

/// testcmpframe.c Errors 1-9: NormPoints on a CmpFrame whose axes are
/// permuted (3, 1, 2), holding a 1-D Frame and a SkyFrame whose own axes are
/// swapped, so the CmpFrame's axes are (RA, PX, Dec). With `contig` the
/// longitudes are brought into one range (the two near 2pi go below zero);
/// without it every value is returned as given.
#[test]
fn norm_points_on_a_permuted_cmpframe_holding_a_swapped_skyframe() {
    use starlink_ast::coords::PointSet;
    use starlink_ast::frames::{BasicFrameBuilder, CmpFrame, Frame, SkyFrame};
    let mut sky = SkyFrame::unset_system();
    sky.perm_axes(&[2, 1]).unwrap();
    let plane = BasicFrameBuilder::new(1).domain("FPLANE").build().unwrap();
    let mut cmp = CmpFrame::new(Box::new(plane), Box::new(sky));
    cmp.perm_axes(&[3, 1, 2]).unwrap();

    let ra = [6.1, 6.1, 0.04, 0.04];
    let dec = [0.2, -0.2, -0.2, 0.2];
    let px = [-100.0, -10.0, 10.0, 100.0];
    let pts = PointSet::from_rows((0..4).map(|i| vec![ra[i], px[i], dec[i]]));

    let normed = cmp.norm_points(&pts, true).unwrap();
    for i in 0..4 {
        let want_ra = if i < 2 { ra[i] - 2.0 * PI } else { ra[i] };
        assert_eq!(normed.point(i)[0], want_ra, "Errors 1-4, point {i}");
        assert_eq!(normed.point(i)[1], px[i], "Error 5, point {i}");
        assert_eq!(normed.point(i)[2], dec[i], "Error 6, point {i}");
    }

    let passed = cmp.norm_points(&pts, false).unwrap();
    for i in 0..4 {
        assert_eq!(
            passed.point(i).to_vec(),
            vec![ra[i], px[i], dec[i]],
            "Errors 7-9, point {i}"
        );
    }
}

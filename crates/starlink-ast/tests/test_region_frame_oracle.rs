// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The Frame methods a Region forwards to the current Frame of its FrameSet
//! (region.c `Offset2`, `Resolve`, `AxDistance`, `AxOffset`,
//! `SystemString`, `PickAxes`, and `SubFrame` through `astConvert`), against
//! the C library, for a Circle in an ICRS SkyFrame, where a Cartesian answer
//! differs from the spherical one.
//!
//! The values come from `tools/oracle_fixtures/region_frame_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_frame_oracle/cases.rs"]
mod cases;

use starlink_ast::frames::FrameSearch;
use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
use starlink_ast::regions::{Circle, CircleBuilder, Region};

fn circle() -> Circle {
    CircleBuilder::new(SkyFrame::new(SkySystem::Icrs), cases::CENTRE.to_vec())
        .unwrap()
        .radius(cases::RADIUS)
        .build()
        .unwrap()
}

#[test]
fn offset2_and_resolve_are_spherical() {
    let c = circle();
    let (point, angle) = c.offset2(&cases::CENTRE, 1.0, 0.5).unwrap();
    crate::common::assert_c_values(&[point[0], point[1], angle], &cases::OFFSET2, "offset2");
    let (point, d1, d2) = c.resolve(&cases::CENTRE, &[1.5, 1.2], &[1.2, 1.3]).unwrap();
    crate::common::assert_c_values(&[point[0], point[1], d1, d2], &cases::RESOLVE, "resolve");
}

#[test]
fn axis_distance_and_offset_wrap_longitude() {
    let c = circle();
    crate::common::assert_c_values(
        &[
            c.ax_distance(1, 0.1, 6.2).unwrap(),
            c.ax_offset(1, 6.2, 0.2).unwrap(),
        ],
        &cases::AX_DISTANCE_OFFSET,
        "ax_distance, ax_offset",
    );
}

#[test]
fn system_and_picked_axes_come_from_the_sky_frame() {
    let c = circle();
    assert_eq!(c.system_string().unwrap(), Some(cases::SYSTEM));
    let picked = c.pick_axes_snapshot(&[1, 0]).unwrap();
    let region = picked
        .as_any()
        .downcast_ref::<Circle>()
        .expect("PickAxes on a Circle gives a Circle");
    assert_eq!(cases::PICKED.0, "Circle");
    assert_eq!(region.system_string().unwrap(), Some(cases::PICKED.1));
}

#[test]
fn conversions_to_and_from_the_region() {
    let mut c = circle();
    let mut gal = SkyFrame::new(SkySystem::Galactic);
    let found = (
        gal.convert_to(&mut c, &[]).unwrap().is_some(),
        c.convert_to(&mut gal, &[]).unwrap().is_some(),
    );
    assert_eq!(found, cases::CONVERTS);
    assert!(!(&c as &dyn Region).is_unit_frame());
}

/// `region_points` gives the defining points in the current Frame, as
/// `astGetRegionPoints` does, here of a Box mapped by a ZoomMap.
#[test]
fn region_points_are_in_the_current_frame() {
    let frame = || {
        starlink_ast::frames::BasicFrameBuilder::new(2)
            .build()
            .unwrap()
    };
    let b = starlink_ast::regions::BoxRegionBuilder::from_bounds(frame(), &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    let zoom = starlink_ast::mappings::ZoomMap::new(2, 2.0).unwrap();
    let mapped = b.map_region(&zoom, &frame()).unwrap();
    let pts = mapped.region_points().unwrap();
    let got = [
        pts.point(0)[0],
        pts.point(1)[0],
        pts.point(0)[1],
        pts.point(1)[1],
    ];
    assert_eq!(got, cases::MAPPED_BOX_POINTS);
}

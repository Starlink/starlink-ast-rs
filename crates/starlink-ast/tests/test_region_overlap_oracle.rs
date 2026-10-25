// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! `astOverlap` codes against the C library for Region pairs that exercise
//! the per-class dispatch of region.c `Overlap` and `OverlapX`: an Interval
//! unbounded on three sides against Boxes beside, across and inside it,
//! where the generic `OverlapX` meshes the Interval's boundary through its
//! own `BndBaseMesh`; Regions whose Frames cannot be converted;
//! NullRegions, whose own `Overlap` and `OverlapX` decide; and Prisms, whose
//! own `Overlap` compares their components separately where the other
//! Region splits the same way, and otherwise uses the inherited `OverlapX`.
//! C passed the Regions to that `OverlapX` in the wrong order, which the
//! port does not copy (fixed upstream in 0516a13d; the probe needs it).
//!
//! The codes come from `tools/oracle_fixtures/region_overlap_probe.c`, which
//! builds the same Regions.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_overlap_oracle/cases.rs"]
mod cases;

use ndarray::array;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::frames::BasicFrameBuilder;
use starlink_ast::mappings::MatrixMap;
use starlink_ast::regions::{
    BoxRegionBuilder, CircleBuilder, DynRegion, IntervalBuilder, NullRegionBuilder, Overlap,
    PrismBuilder, Region,
};

/// The C code of an [`Overlap`].
fn code(overlap: Overlap) -> i32 {
    match overlap {
        Overlap::Incomparable => 0,
        Overlap::None => 1,
        Overlap::FirstInside => 2,
        Overlap::SecondInside => 3,
        Overlap::Partial => 4,
        Overlap::Same => 5,
        Overlap::Negation => 6,
    }
}

fn boxed(domain: &str, lbnd: [f64; 2], ubnd: [f64; 2]) -> DynRegion {
    let mut frame = BasicFrameBuilder::new(2);
    if !domain.is_empty() {
        frame = frame.domain(domain);
    }
    Box::new(
        BoxRegionBuilder::from_bounds(frame.build().unwrap(), &lbnd, &ubnd)
            .unwrap()
            .build()
            .unwrap(),
    )
}

/// The probe's Interval x >= 0.
fn half_plane() -> DynRegion {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    Box::new(
        IntervalBuilder::new(frame, vec![0.0, BAD_VALUE], vec![BAD_VALUE, BAD_VALUE])
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn null_region(negated: bool) -> DynRegion {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut nr = NullRegionBuilder::new(frame).build().unwrap();
    if negated {
        nr.negate();
    }
    Box::new(nr)
}

fn box3(lbnd: [f64; 3], ubnd: [f64; 3]) -> DynRegion {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    Box::new(
        BoxRegionBuilder::from_bounds(frame, &lbnd, &ubnd)
            .unwrap()
            .build()
            .unwrap(),
    )
}

/// The probe's Prism: the unit Circle extruded over z in [0, 1].
fn prism(negated: bool) -> DynRegion {
    let circle = CircleBuilder::new(BasicFrameBuilder::new(2).build().unwrap(), vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let interval = IntervalBuilder::new(
        BasicFrameBuilder::new(1).build().unwrap(),
        vec![0.0],
        vec![1.0],
    )
    .unwrap()
    .build()
    .unwrap();
    let mut prism = PrismBuilder::new(circle, interval)
        .unwrap()
        .build()
        .unwrap();
    if negated {
        prism.negate();
    }
    Box::new(prism)
}

/// The probe's Box [-5, 5]^3 rotated by 0.5 radians in the x-z plane.
fn rotated_box() -> DynRegion {
    let (s, c) = 0.5_f64.sin_cos();
    let rot = MatrixMap::new(array![[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]]).unwrap();
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    box3([-5.0; 3], [5.0; 3]).map_region(&rot, &frame).unwrap()
}

fn pair(name: &str) -> (DynRegion, DynRegion) {
    match name {
        "box_beside_half_plane" => (boxed("", [-10.0, -1.0], [-9.0, 1.0]), half_plane()),
        "box_across_half_plane_edge" => (boxed("", [-1.0, -1.0], [1.0, 1.0]), half_plane()),
        "box_inside_half_plane" => (boxed("", [5.0, -1.0], [6.0, 1.0]), half_plane()),
        "different_domains" => (
            boxed("AAA", [0.0, 0.0], [1.0, 1.0]),
            boxed("BBB", [0.0, 0.0], [1.0, 1.0]),
        ),
        "box_and_null_region" => (boxed("", [0.0, 0.0], [1.0, 1.0]), null_region(false)),
        "box_and_negated_null_region" => (boxed("", [0.0, 0.0], [1.0, 1.0]), null_region(true)),
        "prism_in_box" => (prism(false), box3([-5.0; 3], [5.0; 3])),
        "prism_in_rotated_box" => (prism(false), rotated_box()),
        "prism_across_box" => (prism(false), box3([0.0, -5.0, -5.0], [5.0; 3])),
        "negated_prism_and_box" => (prism(true), box3([-5.0; 3], [5.0; 3])),
        other => panic!("no Rust construction for case {other}"),
    }
}

#[test]
fn overlap_codes_match_the_c_library() {
    for &(name, want_ab, want_ba) in cases::CASES {
        let (a, b) = pair(name);
        assert_eq!(
            code(a.overlap(b.as_ref()).unwrap()),
            want_ab,
            "{name}: a, b"
        );
        assert_eq!(
            code(b.overlap(a.as_ref()).unwrap()),
            want_ba,
            "{name}: b, a"
        );
    }
}

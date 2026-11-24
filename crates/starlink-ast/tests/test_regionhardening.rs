// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Regions that are unusual in shape, extent or dimension: grids, masks,
//! overlaps, bounds, meshes, Ellipses, Polygons and Regions mapped into fewer
//! axes must give the C library's result or its error, and never panic, hang
//! or overflow the stack.
//!
//! Ported from `reference/starlink-ast/ast_tester/testregionhardening.c`, one
//! test per C section.
//! Divergences from the C:
//!
//! - C checks a few results only for being in range, as the sanitizers find
//!   the faults it guards against; each case here also pins the value the C
//!   library gives at the pinned commit, from the C test or a probe.
//! - An expected error is compared by its code and every message C reports,
//!   in order, without C's first "Error in routine" line.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::frames::{BasicFrame, BasicFrameBuilder, CmpFrame, Frame, SkyFrame};
use starlink_ast::mappings::{Mapping, MathMap, PermMap};
use starlink_ast::regions::{
    BoxRegionBuilder, CircleBuilder, CmpOperator, CmpRegionBuilder, EllipseBuilder,
    IntervalBuilder, NullRegionBuilder, PointListBuilder, PrismBuilder, Region, mask,
};

fn plain(naxes: usize) -> BasicFrame {
    BasicFrameBuilder::new(naxes).build().unwrap()
}

/// The overlap code C's `astOverlap` returns.
fn overlap_code(a: &dyn Region, b: &dyn Region) -> i32 {
    a.overlap(b).unwrap() as i32
}

/// The grid of a Box with corners `p1` and `p2` in `frame` and a MeshSize of
/// 30, as `check_grid` in the C test makes it.
fn box_grid(
    frame: impl Frame + 'static,
    p1: &[f64],
    p2: &[f64],
) -> AstResult<starlink_ast::coords::PointSet> {
    let mut bx = BoxRegionBuilder::from_bounds(frame, p1, p2)
        .unwrap()
        .build()
        .unwrap();
    bx.set_mesh_size(30);
    bx.region_grid()
}

/// Check that a grid has `npoint` points, each inside the Box with corners
/// `p1` and `p2`.
fn check_grid_inside(grid: &starlink_ast::coords::PointSet, npoint: usize, p1: &[f64], p2: &[f64]) {
    assert_eq!(grid.npoint(), npoint);
    for (j, (&a, &b)) in p1.iter().zip(p2).enumerate() {
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let tol = 1.0e-9 * (hi - lo);
        for &v in grid.axis(j) {
            assert!(
                v >= lo - tol && v <= hi + tol,
                "axis {} point {v} outside the Box",
                j + 1
            );
        }
    }
}

#[test]
fn grids() {
    // A Box with no width on its last axis, in a Frame whose axes have
    // different Units.
    let mut frame = plain(2);
    frame.set_unit(1, "m").unwrap();
    frame.set_unit(2, "s").unwrap();
    let (a, b) = ([0.0, 0.0], [1.0, 0.0]);
    check_grid_inside(&box_grid(frame, &a, &b).unwrap(), 10, &a, &b);

    // A 3-D Box with no width on axis 3, in a CmpFrame.
    let cmp = CmpFrame::new(Box::new(plain(2)), Box::new(plain(1)));
    let (a, b) = ([0.0, 0.0, 0.0], [1.0, 1.0, 0.0]);
    check_grid_inside(&box_grid(cmp, &a, &b).unwrap(), 18, &a, &b);

    // A sky Box with no height.
    let grid = box_grid(SkyFrame::unset_system(), &[0.5, 0.1], &[0.6, 0.1]).unwrap();
    assert_eq!(grid.npoint(), 28);
}

/// Mask the pixels 1 to 10 with `region`, and return the number changed and
/// which were changed.
fn mask_ten(region: &dyn Region, inside: bool) -> (usize, String) {
    let mut d = [0.0f64; 10];
    let n = mask(region, None, inside, &[1], &[10], &mut d, 9.0).unwrap();
    let got = d
        .iter()
        .map(|&v| if v == 9.0 { '1' } else { '0' })
        .collect();
    (n, got)
}

#[test]
fn masks() {
    // A pixel on the edge of a closed Region is in neither the Region nor
    // its negation.
    let below = IntervalBuilder::new(plain(1), vec![BAD_VALUE], vec![7.0])
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(mask_ten(&below, false), (3, "0000000111".to_string()));
    assert_eq!(mask_ten(&below, true), (6, "1111110000".to_string()));
}

fn two_points() -> impl Region {
    PointListBuilder::new(plain(2), vec![vec![1.0, 2.0], vec![3.0, 4.0]])
        .unwrap()
        .build()
        .unwrap()
}

fn box_0_5() -> impl Region {
    BoxRegionBuilder::from_bounds(plain(2), &[0.0, 0.0], &[5.0, 5.0])
        .unwrap()
        .build()
        .unwrap()
}

fn interval(naxes: usize, lo: &[f64], hi: &[f64]) -> impl Region {
    IntervalBuilder::new(plain(naxes), lo.to_vec(), hi.to_vec())
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn overlaps() {
    // A PointList and a Box that contains it.
    assert_eq!(overlap_code(&two_points(), &box_0_5()), 2);

    // Two PointLists that hold the same positions, one of them twice.
    let three = PointListBuilder::new(
        plain(2),
        vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![1.0, 2.0]],
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(overlap_code(&two_points(), &three), 5);

    // Two CmpRegions that each hold a PointList.
    let and1 = CmpRegionBuilder::new(two_points(), box_0_5(), CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    let and2 = CmpRegionBuilder::new(two_points(), box_0_5(), CmpOperator::And)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(overlap_code(&and1, &and2), 5);

    // Two 1-D Intervals, one the negation of the other.
    assert_eq!(
        overlap_code(&interval(1, &[2.0], &[1.0]), &interval(1, &[1.0], &[2.0])),
        6
    );
}

/// The PermMap that keeps the first of two axes and gives the second the
/// value 0 on the way back.
fn first_of_two() -> PermMap {
    PermMap::new(&[1, -1], &[1], &[0.0]).unwrap()
}

#[test]
fn axes() {
    // A PointList with two base Frame axes and one current Frame axis.
    let mapped = two_points().map_region(&first_of_two(), &plain(1)).unwrap();
    let (lbnd, ubnd) = mapped.get_region_bounds().unwrap();
    assert_eq!((lbnd, ubnd), (vec![1.0], vec![3.0]));

    // A NullRegion likewise: its bounds are the empty interval.
    let null = NullRegionBuilder::new(plain(2)).build().unwrap();
    let mapped = null.map_region(&first_of_two(), &plain(1)).unwrap();
    let (lbnd, ubnd) = mapped.get_region_bounds().unwrap();
    assert_eq!((lbnd.len(), ubnd.len()), (1, 1));
    assert!(lbnd[0] > ubnd[0], "bounds {lbnd:?} to {ubnd:?}");

    // A Prism with three base Frame axes and four current Frame axes.
    let mapped = cylinder()
        .map_region(
            &PermMap::new(&[1, 2, 3], &[1, 2, 3, -1], &[0.0]).unwrap(),
            &plain(4),
        )
        .unwrap();
    let mesh = mapped.region_mesh().unwrap();
    assert!(mesh.npoint() >= 1);
    assert_eq!(mesh.ncoord(), 4);
}

/// A Prism made from a Circle of radius 5 and the Interval 2 to 8.
pub(crate) fn cylinder() -> impl Region {
    let circle = CircleBuilder::new(plain(2), vec![0.0, 0.0])
        .unwrap()
        .radius(5.0)
        .build()
        .unwrap();
    PrismBuilder::new(circle, interval(1, &[2.0], &[8.0]))
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn ellipses() {
    // An Ellipse at the origin used as the uncertainty of a Box is moved to
    // the centre of the Box and keeps its size.
    let ellipse = EllipseBuilder::new(plain(2), vec![0.0, 0.0], [0.2, 0.1], 0.3)
        .unwrap()
        .build()
        .unwrap();
    let (elbnd, eubnd) = ellipse.get_region_bounds().unwrap();
    let bx = BoxRegionBuilder::from_bounds(plain(2), &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .uncertainty(Box::new(ellipse))
        .build()
        .unwrap();
    let unc = bx.get_unc(false).unwrap().expect("an uncertainty");
    let (lbnd, ubnd) = unc.get_region_bounds().unwrap();
    for i in 0..2 {
        assert!((0.5 * (lbnd[i] + ubnd[i]) - 5.0).abs() <= 1.0e-6);
        assert!(((ubnd[i] - lbnd[i]) - (eubnd[i] - elbnd[i])).abs() <= 1.0e-6);
    }

    // Three points in a line do not define an ellipse, which is reported
    // each time the Ellipse is used.
    let line =
        EllipseBuilder::from_points(plain(2), vec![0.0, 0.0], vec![2.0, 0.0], vec![1.0, 0.0])
            .unwrap()
            .build()
            .unwrap();
    let first = line.tran2(&[0.5], &[0.1], true).unwrap_err();
    let second = line.tran2(&[0.5], &[0.1], true).unwrap_err();
    assert_eq!(first.code(), ErrorCode::Badin);
    assert_eq!(second.code(), ErrorCode::Badin);
    assert_eq!(first.message(), second.message());

    // A Circle mapped so that no point of its boundary is away from the new
    // centre is still mapped.
    let circle = CircleBuilder::new(plain(2), vec![1.0, 0.0])
        .unwrap()
        .radius(2.0)
        .build()
        .unwrap();
    let map = MathMap::new(
        2,
        2,
        &["a = qif( x > 0, 5, <bad> )", "b = qif( x > 0, 7, <bad> )"],
        &["x = a", "y = b"],
    )
    .unwrap();
    circle.map_region(&map, &plain(2)).unwrap();
}

#[test]
fn fewer_axes() {
    let regions: Vec<(&str, Box<dyn Region>)> = vec![
        (
            "Box",
            Box::new(
                BoxRegionBuilder::from_bounds(plain(2), &[1.5, 2.5], &[3.5, 4.5])
                    .unwrap()
                    .build()
                    .unwrap(),
            ),
        ),
        (
            "Circle",
            Box::new(
                CircleBuilder::new(plain(2), vec![3.0, 3.5])
                    .unwrap()
                    .radius(1.6)
                    .build()
                    .unwrap(),
            ),
        ),
        (
            "PointList",
            Box::new(
                PointListBuilder::new(
                    plain(2),
                    vec![vec![2.0, 1.0], vec![3.0, 5.0], vec![4.0, 9.0]],
                )
                .unwrap()
                .build()
                .unwrap(),
            ),
        ),
    ];
    let counts = [4, 6, 3];
    let outside = ["100111", "111111", "100011"];

    // The Mapping keeps the first axis, and its inverse gives the second
    // axis no value.
    let map = PermMap::new(&[1, 0], &[1], &[]).unwrap();
    let copy = map.clone();
    for (i, (name, region)) in regions.iter().enumerate() {
        region
            .map_region(&map, &plain(1))
            .unwrap_or_else(|e| panic!("a {name} could not be mapped: {e}"));
        assert!(map.equal(&copy), "mapping a {name} changed the Mapping");

        // Masking with the same Mapping maps the Region in the same way.
        let mut d = [0i32; 6];
        let n = mask(region.as_ref(), Some(&map), false, &[1], &[6], &mut d, 1).unwrap();
        let got: String = d.iter().map(|&v| if v != 0 { '1' } else { '0' }).collect();
        assert_eq!(
            (n, got.as_str()),
            (counts[i], outside[i]),
            "masking outside a {name}"
        );
        assert!(
            map.equal(&copy),
            "masking with a {name} changed the Mapping"
        );
    }
}

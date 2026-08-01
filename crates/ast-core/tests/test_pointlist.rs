//! Tests for the PointList region implementation.
//!
//! Covers: construction, get_region_points, point matching (exact and
//! non-matching), negation, multiple defining points, bounding box,
//! is_bounded, batch transform.
//!
//! Ported from: reference/starlink-ast/ast_tester/testpointlist.c (partial)

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{PointListBuilder, Region};

// ---- Construction ----

#[test]
fn construct_single_point() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let pl = PointListBuilder::new(frame, vec![vec![1.0, 2.0]])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(pl.naxes(), 2);
    assert_eq!(pl.list_size(), 1);
}

#[test]
fn construct_multiple_points() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let points = vec![
        vec![1.0, 2.0, 3.0],
        vec![4.0, 5.0, 6.0],
        vec![7.0, 8.0, 9.0],
    ];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(pl.naxes(), 3);
    assert_eq!(pl.list_size(), 3);
}

#[test]
fn construct_requires_at_least_one_point() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = PointListBuilder::new(frame, vec![]);
    assert!(result.is_err());
}

#[test]
fn construct_requires_matching_axes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Point with wrong number of coordinates.
    let result = PointListBuilder::new(frame, vec![vec![1.0, 2.0, 3.0]]);
    assert!(result.is_err());
}

// ---- get_region_points ----

#[test]
fn get_region_points_returns_defining_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]];
    let pl = PointListBuilder::new(frame, points.clone())
        .unwrap()
        .build()
        .unwrap();

    let pts = pl.get_region_points().unwrap();
    assert_eq!(pts.npoint(), 3);
    assert_eq!(pts.ncoord(), 2);

    for (i, expected) in points.iter().enumerate() {
        let p = pts.point(i);
        assert!((p[0] - expected[0]).abs() < 1e-15);
        assert!((p[1] - expected[1]).abs() < 1e-15);
    }
}

// ---- Point matching (exact) ----

#[test]
fn point_matches_defining_point_exact() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    // Both defining points should be inside.
    assert!(pl.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(pl.point_in_region(&[3.0, 4.0]).unwrap());
}

#[test]
fn point_does_not_match_even_slightly_off() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    // A point that is clearly off should not match.
    assert!(!pl.point_in_region(&[1.1, 2.0]).unwrap());
    assert!(!pl.point_in_region(&[1.0, 2.1]).unwrap());
    assert!(!pl.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(!pl.point_in_region(&[1.001, 2.001]).unwrap());
}

#[test]
fn point_with_bad_value_not_inside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    assert!(!pl.point_in_region(&[BAD_VALUE, 2.0]).unwrap());
    assert!(!pl.point_in_region(&[1.0, BAD_VALUE]).unwrap());
}

// ---- Negation ----

#[test]
fn negation_flips_membership() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let mut pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    // Before negation.
    assert!(pl.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(!pl.point_in_region(&[0.0, 0.0]).unwrap());

    // Negate.
    pl.negate();
    assert!(pl.is_negated());

    // After negation: defining points are outside, other points are inside.
    assert!(!pl.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(pl.point_in_region(&[0.0, 0.0]).unwrap());
}

#[test]
fn negated_copy_independent() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    let neg = pl.negated_copy();

    assert!(!pl.is_negated());
    assert!(neg.is_negated());

    assert!(pl.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(!neg.point_in_region(&[1.0, 2.0]).unwrap());
}

// ---- Multiple defining points ----

#[test]
fn multiple_points_all_match() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![
        vec![0.0, 0.0],
        vec![1.0, 0.0],
        vec![0.0, 1.0],
        vec![1.0, 1.0],
        vec![0.5, 0.5],
    ];
    let pl = PointListBuilder::new(frame, points.clone())
        .unwrap()
        .build()
        .unwrap();

    // All defining points should match.
    for p in &points {
        assert!(
            pl.point_in_region(p).unwrap(),
            "Expected point {:?} to be inside",
            p
        );
    }

    // Non-defining points should not match.
    assert!(!pl.point_in_region(&[0.25, 0.25]).unwrap());
    assert!(!pl.point_in_region(&[2.0, 2.0]).unwrap());
}

// ---- Bounding box ----

#[test]
fn bounding_box_single_point() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![3.0, 7.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = pl.get_region_bounds().unwrap();
    assert_eq!(lower.len(), 2);
    assert_eq!(upper.len(), 2);
    assert!((lower[0] - 3.0).abs() < 1e-10);
    assert!((lower[1] - 7.0).abs() < 1e-10);
    assert!((upper[0] - 3.0).abs() < 1e-10);
    assert!((upper[1] - 7.0).abs() < 1e-10);
}

#[test]
fn bounding_box_multiple_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![
        vec![1.0, 5.0],
        vec![3.0, 2.0],
        vec![7.0, 8.0],
        vec![4.0, 1.0],
    ];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = pl.get_region_bounds().unwrap();
    assert!((lower[0] - 1.0).abs() < 1e-10);
    assert!((lower[1] - 1.0).abs() < 1e-10);
    assert!((upper[0] - 7.0).abs() < 1e-10);
    assert!((upper[1] - 8.0).abs() < 1e-10);
}

#[test]
fn bounding_box_3d() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let points = vec![
        vec![1.0, 2.0, 3.0],
        vec![4.0, 5.0, 6.0],
        vec![-1.0, 0.0, 10.0],
    ];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = pl.get_region_bounds().unwrap();
    assert!((lower[0] - (-1.0)).abs() < 1e-10);
    assert!((lower[1] - 0.0).abs() < 1e-10);
    assert!((lower[2] - 3.0).abs() < 1e-10);
    assert!((upper[0] - 4.0).abs() < 1e-10);
    assert!((upper[1] - 5.0).abs() < 1e-10);
    assert!((upper[2] - 10.0).abs() < 1e-10);
}

// ---- is_bounded ----

#[test]
fn is_bounded_always_true() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![100.0, 200.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    assert!(pl.is_bounded());
}

// ---- Batch transform ----

#[test]
fn transform_passes_matching_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    // Two defining points + one non-matching point.
    let pts = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]);
    let result = pl.transform(&pts, true).unwrap();

    assert_eq!(result.npoint(), 3);

    // Point 0: matches defining point (1,2) — passes through.
    assert!((result.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 2.0).abs() < 1e-10);

    // Point 1: matches defining point (3,4) — passes through.
    assert!((result.point(1)[0] - 3.0).abs() < 1e-10);
    assert!((result.point(1)[1] - 4.0).abs() < 1e-10);

    // Point 2: no match — BAD.
    assert_eq!(result.point(2)[0], BAD_VALUE);
    assert_eq!(result.point(2)[1], BAD_VALUE);
}

#[test]
fn transform_inverse_passes_all() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[99.0, 88.0], [1.0, 2.0]]);
    let result = pl.transform(&pts, false).unwrap();

    assert!((result.point(0)[0] - 99.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 88.0).abs() < 1e-10);
    assert!((result.point(1)[0] - 1.0).abs() < 1e-10);
    assert!((result.point(1)[1] - 2.0).abs() < 1e-10);
}

#[test]
fn transform_negated_inverts_mask() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0]];
    let mut pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    pl.negate();

    let pts = PointSet::from_rows([[1.0, 2.0], [5.0, 6.0]]);
    let result = pl.transform(&pts, true).unwrap();

    // Point 0: matches a defining point, but negated — so BAD.
    assert_eq!(result.point(0)[0], BAD_VALUE);
    assert_eq!(result.point(0)[1], BAD_VALUE);

    // Point 1: does NOT match — negated means it passes through.
    assert!((result.point(1)[0] - 5.0).abs() < 1e-10);
    assert!((result.point(1)[1] - 6.0).abs() < 1e-10);
}

// ---- Mapping properties ----

#[test]
fn mapping_nin_nout() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let points = vec![vec![1.0, 2.0, 3.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(pl.nin(), 3);
    assert_eq!(pl.nout(), 3);
}

// ---- Region attributes ----

#[test]
fn region_attributes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![0.0, 0.0]];
    let mut pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    // Defaults.
    assert!(pl.is_closed());
    assert!(!pl.is_negated());
    assert!(pl.is_bounded());
    assert!(pl.is_adaptive());
    assert_eq!(pl.mesh_size(), 200);
    assert!((pl.fill_factor() - 1.0).abs() < 1e-10);

    // Setters.
    pl.set_closed(false);
    assert!(!pl.is_closed());

    pl.set_mesh_size(500);
    assert_eq!(pl.mesh_size(), 500);

    pl.set_fill_factor(0.5);
    assert!((pl.fill_factor() - 0.5).abs() < 1e-10);

    pl.set_adaptive(false);
    assert!(!pl.is_adaptive());
}

// ---- Frame delegation ----

#[test]
fn frame_distance_delegation() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![0.0, 0.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    let d = pl.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

// ---- Clone ----

#[test]
fn clone_is_independent() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    let mut clone = pl.clone();
    clone.negate();

    assert!(!pl.is_negated());
    assert!(clone.is_negated());

    assert!(pl.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(!clone.point_in_region(&[1.0, 2.0]).unwrap());
}

// ---- Mesh ----

#[test]
fn mesh_is_defining_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]];
    let pl = PointListBuilder::new(frame, points.clone())
        .unwrap()
        .build()
        .unwrap();

    let mesh = pl.get_region_mesh().unwrap();
    assert_eq!(mesh.npoint(), 3);
    assert_eq!(mesh.ncoord(), 2);

    for (i, expected) in points.iter().enumerate() {
        let p = mesh.point(i);
        assert!((p[0] - expected[0]).abs() < 1e-15);
        assert!((p[1] - expected[1]).abs() < 1e-15);
    }
}

// ---- Disc ----

#[test]
fn get_region_disc() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![0.0, 0.0], vec![3.0, 0.0], vec![0.0, 4.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    let (centre, radius) = pl.get_region_disc().unwrap();
    // Centroid is (1.0, 4/3).
    assert!((centre[0] - 1.0).abs() < 1e-10);
    assert!((centre[1] - 4.0 / 3.0).abs() < 1e-10);
    // Radius should be max distance from centroid to any point.
    assert!(radius > 0.0);
}

// ---- Integrity ----

#[test]
fn with_integrity_preserves_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let mut pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    pl.with_integrity(|_frame| Ok(())).unwrap();

    // Points should still work.
    assert!(pl.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(pl.point_in_region(&[3.0, 4.0]).unwrap());
    assert!(!pl.point_in_region(&[0.0, 0.0]).unwrap());
}

// ---- reg_pins (on-boundary within uncertainty) ----

#[test]
fn reg_pins_defining_points_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let pl = PointListBuilder::new(frame, points.clone())
        .unwrap()
        .build()
        .unwrap();

    // The defining points themselves lie on the boundary: mutual coverage
    // holds in both directions.
    let pts = PointSet::from_rows(points.iter().map(|p| p.as_slice()));
    assert!(pl.reg_pins(&pts, None).unwrap());
}

#[test]
fn reg_pins_point_off_boundary_reports_false() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    // A supplied point far from every defining point fails direction 1.
    let supplied = [vec![1.0, 2.0], vec![10.0, 10.0]];
    let pts = PointSet::from_rows(supplied.iter().map(|p| p.as_slice()));
    assert!(!pl.reg_pins(&pts, None).unwrap());
}

#[test]
fn reg_pins_incomplete_cover_reports_false() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // This PointList has three defining points.
    let points = vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]];
    let pl = PointListBuilder::new(frame, points)
        .unwrap()
        .build()
        .unwrap();

    // The supplied points cover only two of the three defining points. Every
    // supplied point is on the boundary (direction 1 passes), but the third
    // defining point is not close to any supplied point (direction 2 fails).
    let supplied = [vec![1.0, 2.0], vec![3.0, 4.0]];
    let pts = PointSet::from_rows(supplied.iter().map(|p| p.as_slice()));
    assert!(!pl.reg_pins(&pts, None).unwrap());
}

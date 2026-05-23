//! Tests for the BoxRegion implementation.
//!
//! Covers: construction (centre+corner and from_bounds), point_in_region,
//! open boundary, negation, bounds, 3D box, batch transform, Frame delegation.
//!
//! Ancestor: reference/starlink-ast/ast_tester/testbox.c (partial port).

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{BoxRegionBuilder, Region};

// ---- Construction from centre + corner ----

#[test]
fn construct_centre_corner() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::new(frame, vec![5.0, 10.0], vec![8.0, 14.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(b.naxes(), 2);

    // Bounds should be [2, 6] to [8, 14].
    let (lbnd, ubnd) = b.get_region_bounds().unwrap();
    assert!((lbnd[0] - 2.0).abs() < 1e-10);
    assert!((lbnd[1] - 6.0).abs() < 1e-10);
    assert!((ubnd[0] - 8.0).abs() < 1e-10);
    assert!((ubnd[1] - 14.0).abs() < 1e-10);
}

#[test]
fn construct_centre_corner_symmetric() {
    // Corner on the other side: corner < centre.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::new(frame, vec![5.0, 10.0], vec![2.0, 6.0])
        .unwrap()
        .build()
        .unwrap();

    // Bounds should still be [2, 6] to [8, 14].
    let (lbnd, ubnd) = b.get_region_bounds().unwrap();
    assert!((lbnd[0] - 2.0).abs() < 1e-10);
    assert!((lbnd[1] - 6.0).abs() < 1e-10);
    assert!((ubnd[0] - 8.0).abs() < 1e-10);
    assert!((ubnd[1] - 14.0).abs() < 1e-10);
}

// ---- Construction from bounds ----

#[test]
fn construct_from_bounds() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[1.0, 2.0], &[5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(b.naxes(), 2);

    let (lbnd, ubnd) = b.get_region_bounds().unwrap();
    assert!((lbnd[0] - 1.0).abs() < 1e-10);
    assert!((lbnd[1] - 2.0).abs() < 1e-10);
    assert!((ubnd[0] - 5.0).abs() < 1e-10);
    assert!((ubnd[1] - 8.0).abs() < 1e-10);
}

#[test]
fn construct_requires_matching_axes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = BoxRegionBuilder::new(frame, vec![1.0, 2.0, 3.0], vec![4.0, 5.0]);
    assert!(result.is_err());
}

// ---- point_in_region ----

#[test]
fn point_inside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // Centre is inside.
    assert!(b.point_in_region(&[5.0, 5.0]).unwrap());

    // Points well inside.
    assert!(b.point_in_region(&[1.0, 1.0]).unwrap());
    assert!(b.point_in_region(&[9.0, 9.0]).unwrap());
    assert!(b.point_in_region(&[0.5, 5.0]).unwrap());
}

#[test]
fn point_outside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // Points outside.
    assert!(!b.point_in_region(&[-1.0, 5.0]).unwrap());
    assert!(!b.point_in_region(&[5.0, 11.0]).unwrap());
    assert!(!b.point_in_region(&[11.0, 5.0]).unwrap());
    assert!(!b.point_in_region(&[5.0, -0.1]).unwrap());
}

#[test]
fn point_on_boundary_closed() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // Default is closed: boundary points are included.
    assert!(b.is_closed());
    assert!(b.point_in_region(&[0.0, 5.0]).unwrap());
    assert!(b.point_in_region(&[10.0, 5.0]).unwrap());
    assert!(b.point_in_region(&[5.0, 0.0]).unwrap());
    assert!(b.point_in_region(&[5.0, 10.0]).unwrap());
    // Corners.
    assert!(b.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(b.point_in_region(&[10.0, 10.0]).unwrap());
}

#[test]
fn point_on_boundary_open() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    b.set_closed(false);
    assert!(!b.is_closed());

    // Boundary points are now excluded.
    assert!(!b.point_in_region(&[0.0, 5.0]).unwrap());
    assert!(!b.point_in_region(&[10.0, 5.0]).unwrap());
    assert!(!b.point_in_region(&[5.0, 0.0]).unwrap());
    assert!(!b.point_in_region(&[5.0, 10.0]).unwrap());

    // Interior points are still inside.
    assert!(b.point_in_region(&[5.0, 5.0]).unwrap());
    assert!(b.point_in_region(&[0.01, 0.01]).unwrap());
}

#[test]
fn point_with_bad_value() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    assert!(!b.point_in_region(&[BAD_VALUE, 5.0]).unwrap());
    assert!(!b.point_in_region(&[5.0, BAD_VALUE]).unwrap());
    assert!(!b.point_in_region(&[BAD_VALUE, BAD_VALUE]).unwrap());
}

// ---- Negation ----

#[test]
fn negation_flips_membership() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    assert!(b.point_in_region(&[5.0, 5.0]).unwrap());
    assert!(!b.point_in_region(&[15.0, 5.0]).unwrap());

    b.negate();
    assert!(b.is_negated());

    assert!(!b.point_in_region(&[5.0, 5.0]).unwrap());
    assert!(b.point_in_region(&[15.0, 5.0]).unwrap());
}

#[test]
fn negated_copy() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let neg = b.negated_copy();
    assert!(!b.is_negated());
    assert!(neg.is_negated());

    assert!(b.point_in_region(&[5.0, 5.0]).unwrap());
    assert!(!neg.point_in_region(&[5.0, 5.0]).unwrap());
}

// ---- Bounds ----

#[test]
fn bounds_from_offset_centre() {
    // Centre at (3, 7), corner at (5, 10).
    // Extent: |5-3| = 2, |10-7| = 3.
    // Bounds: [1, 4] to [5, 10].
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::new(frame, vec![3.0, 7.0], vec![5.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let (lbnd, ubnd) = b.get_region_bounds().unwrap();
    assert!((lbnd[0] - 1.0).abs() < 1e-10);
    assert!((lbnd[1] - 4.0).abs() < 1e-10);
    assert!((ubnd[0] - 5.0).abs() < 1e-10);
    assert!((ubnd[1] - 10.0).abs() < 1e-10);
}

// ---- 3D box ----

#[test]
fn box_3d() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[1.0, 2.0, 3.0], &[4.0, 6.0, 9.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(b.naxes(), 3);
    assert!(b.is_bounded());

    // Centre should be (2.5, 4, 6).
    let (centre, _) = b.get_region_disc().unwrap();
    assert!((centre[0] - 2.5).abs() < 1e-10);
    assert!((centre[1] - 4.0).abs() < 1e-10);
    assert!((centre[2] - 6.0).abs() < 1e-10);

    // Points inside.
    assert!(b.point_in_region(&[2.5, 4.0, 6.0]).unwrap());
    assert!(b.point_in_region(&[1.0, 2.0, 3.0]).unwrap()); // corner

    // Points outside.
    assert!(!b.point_in_region(&[0.0, 4.0, 6.0]).unwrap());
    assert!(!b.point_in_region(&[2.5, 7.0, 6.0]).unwrap());
    assert!(!b.point_in_region(&[2.5, 4.0, 10.0]).unwrap());
}

// ---- Mapping::transform ----

#[test]
fn transform_passes_interior() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[5.0, 5.0], [1.0, 1.0]]);
    let result = b.transform(&pts, true).unwrap();

    assert_eq!(result.npoint(), 2);
    assert!((result.point(0)[0] - 5.0).abs() < 1e-10);
    assert!((result.point(0)[1] - 5.0).abs() < 1e-10);
    assert!((result.point(1)[0] - 1.0).abs() < 1e-10);
    assert!((result.point(1)[1] - 1.0).abs() < 1e-10);
}

#[test]
fn transform_masks_exterior() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[-1.0, 5.0], [5.0, 11.0]]);
    let result = b.transform(&pts, true).unwrap();

    assert_eq!(result.point(0)[0], BAD_VALUE);
    assert_eq!(result.point(0)[1], BAD_VALUE);
    assert_eq!(result.point(1)[0], BAD_VALUE);
    assert_eq!(result.point(1)[1], BAD_VALUE);
}

#[test]
fn transform_inverse_passes_all() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[-100.0, 200.0], [5.0, 5.0]]);
    let result = b.transform(&pts, false).unwrap();

    assert!((result.point(0)[0] - (-100.0)).abs() < 1e-10);
    assert!((result.point(0)[1] - 200.0).abs() < 1e-10);
}

// ---- Batch transform (10k points) ----

#[test]
fn batch_transform_10k_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[-1.0, -1.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    // Create 10,000 points in a grid from -2 to 2.
    let n = 100;
    let total = n * n;
    let mut pts = PointSet::zeros(total, 2);
    for i in 0..n {
        for j in 0..n {
            let idx = i * n + j;
            let x = -2.0 + 4.0 * i as f64 / (n - 1) as f64;
            let y = -2.0 + 4.0 * j as f64 / (n - 1) as f64;
            pts.point_mut(idx)[0] = x;
            pts.point_mut(idx)[1] = y;
        }
    }

    let out = b.transform(&pts, true).unwrap();
    assert_eq!(out.npoint(), total);

    let mut inside_count = 0;
    let mut outside_count = 0;
    for i in 0..total {
        let p = pts.point(i);
        let o = out.point(i);
        let in_box = p[0] >= -1.0 && p[0] <= 1.0 && p[1] >= -1.0 && p[1] <= 1.0;
        if in_box {
            assert_eq!(o[0], p[0], "point {i} should pass through");
            assert_eq!(o[1], p[1]);
            inside_count += 1;
        } else {
            assert_eq!(o[0], BAD_VALUE, "point {i} should be BAD");
            assert_eq!(o[1], BAD_VALUE);
            outside_count += 1;
        }
    }

    // Sanity: box is [-1,1]x[-1,1] within [-2,2]x[-2,2] so ~25% inside.
    assert!(
        inside_count > 2000,
        "expected >2000 inside, got {inside_count}"
    );
    assert!(
        outside_count > 7000,
        "expected >7000 outside, got {outside_count}"
    );
}

// ---- Frame delegation ----

#[test]
fn frame_naxes() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0, 0.0], &[1.0, 1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(b.naxes(), 3);
    assert_eq!(b.nin(), 3);
    assert_eq!(b.nout(), 3);
}

#[test]
fn frame_distance_delegation() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let d = b.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

// ---- Region attributes ----

#[test]
fn region_attributes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    assert!(b.is_closed());
    assert!(!b.is_negated());
    assert!(b.is_bounded());
    assert!(b.is_adaptive());
    assert_eq!(b.mesh_size(), 200);
    assert!((b.fill_factor() - 1.0).abs() < 1e-10);

    b.set_closed(false);
    assert!(!b.is_closed());

    b.set_mesh_size(500);
    assert_eq!(b.mesh_size(), 500);

    b.set_fill_factor(0.5);
    assert!((b.fill_factor() - 0.5).abs() < 1e-10);

    b.set_adaptive(false);
    assert!(!b.is_adaptive());
}

// ---- Mesh ----

#[test]
fn mesh_2d_points_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[4.0, 3.0])
        .unwrap()
        .build()
        .unwrap();

    b.set_mesh_size(100);
    let mesh = b.get_region_mesh().unwrap();

    // All mesh points should be on the boundary of the box.
    for i in 0..mesh.npoint() {
        let p = mesh.point(i);
        let on_x_boundary = (p[0] - 0.0).abs() < 1e-10 || (p[0] - 4.0).abs() < 1e-10;
        let on_y_boundary = (p[1] - 0.0).abs() < 1e-10 || (p[1] - 3.0).abs() < 1e-10;
        let in_x_range = p[0] >= -1e-10 && p[0] <= 4.0 + 1e-10;
        let in_y_range = p[1] >= -1e-10 && p[1] <= 3.0 + 1e-10;

        assert!(
            (on_x_boundary && in_y_range) || (on_y_boundary && in_x_range),
            "mesh point {} at ({}, {}) is not on the box boundary",
            i,
            p[0],
            p[1]
        );
    }
}

#[test]
fn mesh_1d() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[3.0], &[7.0])
        .unwrap()
        .build()
        .unwrap();

    let mesh = b.get_region_mesh().unwrap();
    assert_eq!(mesh.npoint(), 2);
    assert_eq!(mesh.ncoord(), 1);

    let p0 = mesh.point(0)[0];
    let p1 = mesh.point(1)[0];
    let (lo, hi) = if p0 < p1 { (p0, p1) } else { (p1, p0) };
    assert!((lo - 3.0).abs() < 1e-10);
    assert!((hi - 7.0).abs() < 1e-10);
}

// ---- Clone ----

#[test]
fn clone_is_independent() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let mut clone = b.clone();
    clone.negate();

    assert!(!b.is_negated());
    assert!(clone.is_negated());
    assert!(b.point_in_region(&[5.0, 5.0]).unwrap());
    assert!(!clone.point_in_region(&[5.0, 5.0]).unwrap());
}

// ---- Region disc ----

#[test]
fn get_region_disc() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[6.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    let (centre, radius) = b.get_region_disc().unwrap();
    assert!((centre[0] - 3.0).abs() < 1e-10);
    assert!((centre[1] - 4.0).abs() < 1e-10);
    // Half-diagonal: sqrt(3^2 + 4^2) = 5.
    assert!((radius - 5.0).abs() < 1e-10);
}

// ---- Integrity ----

#[test]
fn with_integrity_preserves_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut b = BoxRegionBuilder::from_bounds(frame, &[1.0, 2.0], &[5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    let (orig_lbnd, orig_ubnd) = b.get_region_bounds().unwrap();

    b.with_integrity(|_frame| Ok(())).unwrap();

    let (lbnd, ubnd) = b.get_region_bounds().unwrap();
    assert!((lbnd[0] - orig_lbnd[0]).abs() < 1e-10);
    assert!((lbnd[1] - orig_lbnd[1]).abs() < 1e-10);
    assert!((ubnd[0] - orig_ubnd[0]).abs() < 1e-10);
    assert!((ubnd[1] - orig_ubnd[1]).abs() < 1e-10);
}

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
use ast_core::regions::{BoxRegion, BoxRegionBuilder, NullRegion, Region};

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

    // Bounds should be (1, 2, 3) to (4, 6, 9).
    let (lbnd, ubnd) = b.get_region_bounds().unwrap();
    assert_eq!(lbnd, [1.0, 2.0, 3.0]);
    assert_eq!(ubnd, [4.0, 6.0, 9.0]);

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

    b.set_fill_factor(0.5).unwrap();
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

// ---- reg_pins: on-boundary-within-uncertainty test (astRegPins port) ----

#[test]
fn reg_pins_boundary_point_is_on_boundary() {
    // Box spanning [0,0]-[10,10]; default uncertainty band ~5e-6 per axis.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // A point exactly on the left edge is on the boundary.
    let on_edge = PointSet::from_rows([[0.0_f64, 5.0]]);
    assert!(b.reg_pins(&on_edge, None).unwrap());

    // A corner is on the boundary.
    let corner = PointSet::from_rows([[10.0_f64, 10.0]]);
    assert!(b.reg_pins(&corner, None).unwrap());
}

#[test]
fn reg_pins_interior_point_is_not_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // The centre is well inside, not on the boundary.
    let centre = PointSet::from_rows([[5.0_f64, 5.0]]);
    assert!(!b.reg_pins(&centre, None).unwrap());
}

#[test]
fn reg_pins_exterior_point_is_not_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // A point far outside is not on the boundary.
    let far = PointSet::from_rows([[20.0_f64, 20.0]]);
    assert!(!b.reg_pins(&far, None).unwrap());
}

#[test]
fn reg_pins_full_boundary_mesh_all_on_boundary() {
    // Every point of the region's own boundary mesh must test as on-boundary.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let mesh = b.get_region_mesh().unwrap();
    assert!(b.reg_pins(&mesh, None).unwrap());
}

#[test]
fn reg_pins_mixed_set_reports_false() {
    // If any point is off the boundary, the whole set fails.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let mixed = PointSet::from_rows([[0.0_f64, 5.0], [5.0, 5.0]]);
    assert!(!b.reg_pins(&mixed, None).unwrap());
}

// ---- Simplify through a PermMap that feeds several base axes a constant ----
//
// Ancestor: checkBoxPermMapSlices in
// reference/starlink-ast/ast_tester/testregions.c.
//
// Each constant-fed base axis is a plane the slice has to lie in, so the
// Region is a NullRegion unless every constant falls inside the Box, and the
// order the constants appear in must not matter.

/// Map a 3-D Box onto two current-Frame axes through a PermMap whose base
/// axes 2 and 3 are each fed a constant, then simplify.
///
/// Axis 2 spans 0 to 20 and axis 3 spans 0 to 30, so 99 is outside the Box on
/// either and 5 and 15 are inside.
fn simplify_box_permmap_slice(constants: [f64; 2]) -> ast_core::mappings::DynMapping {
    use ast_core::mappings::PermMap;

    let base = BasicFrameBuilder::new(3).domain("PIXEL").build().unwrap();
    let curr = BasicFrameBuilder::new(2).domain("SLICE").build().unwrap();

    let pm = PermMap::new(&[1, -1, -2], &[1, 2], &constants).unwrap();
    let b = BoxRegionBuilder::from_bounds(base, &[0.0, 0.0, 0.0], &[10.0, 20.0, 30.0])
        .unwrap()
        .build()
        .unwrap();

    // astMapRegionId is astMapRegion followed by astSimplify (region.c:14033).
    let mapped = b.map_region(&pm, &curr).unwrap();
    <dyn Mapping>::simplify(mapped.as_ref()).unwrap()
}

#[test]
fn simplify_permmap_slice_outside_constant_first_is_null() {
    // Outside, then inside: the first constant's plane misses the Box, and
    // that verdict must survive the second constant.
    let r = simplify_box_permmap_slice([99.0, 5.0]);
    assert!(
        r.as_ref().as_any().is::<NullRegion>(),
        "constants 99,5 should give a NullRegion"
    );
}

#[test]
fn simplify_permmap_slice_outside_constant_last_is_null() {
    // Inside, then outside.
    let r = simplify_box_permmap_slice([5.0, 99.0]);
    assert!(
        r.as_ref().as_any().is::<NullRegion>(),
        "constants 5,99 should give a NullRegion"
    );
}

#[test]
fn simplify_permmap_slice_both_constants_outside_is_null() {
    let r = simplify_box_permmap_slice([99.0, 99.0]);
    assert!(
        r.as_ref().as_any().is::<NullRegion>(),
        "constants 99,99 should give a NullRegion"
    );
}

#[test]
fn simplify_permmap_slice_both_constants_inside_keeps_box() {
    // Both planes meet the Box, so the slice is a Box on the two current
    // Frame axes.
    let r = simplify_box_permmap_slice([5.0, 15.0]);
    let bx = r
        .as_ref()
        .as_any()
        .downcast_ref::<BoxRegion>()
        .expect("constants 5,15 should give a Box");
    assert_eq!(bx.naxes(), 2);
}

/// A Region's forward and inverse transformations are identical (box.c
/// `Transform`): an exterior point is bad in either direction. The values
/// are those C gives for astTran2 on astBox(frame, 1, [0,0], [1,1]).
#[test]
fn inverse_transform_masks_as_forward_does() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let b = BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    for forward in [true, false] {
        let (x, y) = b.tran2(&[0.5, 2.0], &[0.5, 2.0], forward).unwrap();
        assert_eq!(x, [0.5, BAD_VALUE], "forward={forward}");
        assert_eq!(y, [0.5, BAD_VALUE], "forward={forward}");
    }
}

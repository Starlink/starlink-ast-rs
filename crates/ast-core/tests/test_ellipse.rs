//! Tests for the Ellipse region implementation.
//!
//! Covers: construction with semi_axes and angle, point inside/outside/boundary
//! for axis-aligned and rotated ellipses, negation, bounding box, batch
//! transform.

use std::f64::consts::{FRAC_PI_4, PI};

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{EllipseBuilder, Region};

// ---- Construction ----

#[test]
fn construct_axis_aligned() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(ellipse.naxes(), 2);

    let (centre, radius) = ellipse.get_region_disc().unwrap();
    assert!((centre[0]).abs() < 1e-10);
    assert!((centre[1]).abs() < 1e-10);
    // Disc radius should be the semi-major axis.
    assert!((radius - 5.0).abs() < 1e-10);
}

#[test]
fn construct_rotated() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![1.0, 2.0], [4.0, 2.0], FRAC_PI_4)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(ellipse.naxes(), 2);
}

#[test]
fn construct_rejects_non_2d_frame() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let result = EllipseBuilder::new(frame, vec![0.0, 0.0, 0.0], [5.0, 3.0], 0.0);
    assert!(result.is_err());
}

#[test]
fn construct_rejects_wrong_centre_length() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = EllipseBuilder::new(frame, vec![0.0, 0.0, 0.0], [5.0, 3.0], 0.0);
    assert!(result.is_err());
}

#[test]
fn construct_rejects_non_positive_axes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = EllipseBuilder::new(frame, vec![0.0, 0.0], [0.0, 3.0], 0.0);
    assert!(result.is_err());

    let frame2 = BasicFrameBuilder::new(2).build().unwrap();
    let result2 = EllipseBuilder::new(frame2, vec![0.0, 0.0], [5.0, -1.0], 0.0);
    assert!(result2.is_err());
}

// ---- Point inside/outside/boundary: axis-aligned (angle=0) ----

#[test]
fn point_inside_axis_aligned() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Semi-major = 5 along axis 1, semi-minor = 3 along axis 2.
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    // Centre is inside.
    assert!(ellipse.point_in_region(&[0.0, 0.0]).unwrap());

    // Point along axis 1 within semi-major.
    assert!(ellipse.point_in_region(&[4.0, 0.0]).unwrap());

    // Point along axis 2 within semi-minor.
    assert!(ellipse.point_in_region(&[0.0, 2.0]).unwrap());

    // Interior point.
    assert!(ellipse.point_in_region(&[2.0, 1.0]).unwrap());
}

#[test]
fn point_outside_axis_aligned() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    // Beyond semi-major along axis 1.
    assert!(!ellipse.point_in_region(&[6.0, 0.0]).unwrap());

    // Beyond semi-minor along axis 2.
    assert!(!ellipse.point_in_region(&[0.0, 4.0]).unwrap());

    // Corner of bounding box is outside for an ellipse.
    assert!(!ellipse.point_in_region(&[5.0, 3.0]).unwrap());
}

#[test]
fn point_on_boundary_axis_aligned() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    // End of semi-major axis (closed: should be inside).
    assert!(ellipse.point_in_region(&[5.0, 0.0]).unwrap());
    assert!(ellipse.point_in_region(&[-5.0, 0.0]).unwrap());

    // End of semi-minor axis.
    assert!(ellipse.point_in_region(&[0.0, 3.0]).unwrap());
    assert!(ellipse.point_in_region(&[0.0, -3.0]).unwrap());
}

#[test]
fn point_on_boundary_open() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();
    ellipse.set_closed(false);

    // End of semi-major axis is on boundary — excluded in open region.
    assert!(!ellipse.point_in_region(&[5.0, 0.0]).unwrap());

    // Interior is still inside.
    assert!(ellipse.point_in_region(&[4.99, 0.0]).unwrap());
}

// ---- Point test for rotated ellipse (angle = pi/4) ----

#[test]
fn point_inside_rotated() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Semi-major = 4, semi-minor = 2, rotated 45 degrees.
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [4.0, 2.0], FRAC_PI_4)
        .unwrap()
        .build()
        .unwrap();

    // Centre is inside.
    assert!(ellipse.point_in_region(&[0.0, 0.0]).unwrap());

    // Point along the semi-major axis direction (45 degrees).
    let r = 3.0; // Less than semi-major (4).
    let p = [r * FRAC_PI_4.cos(), r * FRAC_PI_4.sin()];
    assert!(ellipse.point_in_region(&p).unwrap());

    // Point along the semi-minor axis direction (135 degrees).
    let r = 1.5; // Less than semi-minor (2).
    let p = [
        r * (FRAC_PI_4 + PI / 2.0).cos(),
        r * (FRAC_PI_4 + PI / 2.0).sin(),
    ];
    assert!(ellipse.point_in_region(&p).unwrap());
}

#[test]
fn point_outside_rotated() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [4.0, 2.0], FRAC_PI_4)
        .unwrap()
        .build()
        .unwrap();

    // Beyond semi-major along the 45-degree direction.
    let r = 5.0; // Greater than semi-major (4).
    let p = [r * FRAC_PI_4.cos(), r * FRAC_PI_4.sin()];
    assert!(!ellipse.point_in_region(&p).unwrap());

    // Beyond semi-minor along the perpendicular direction.
    let r = 3.0; // Greater than semi-minor (2).
    let p = [
        r * (FRAC_PI_4 + PI / 2.0).cos(),
        r * (FRAC_PI_4 + PI / 2.0).sin(),
    ];
    assert!(!ellipse.point_in_region(&p).unwrap());
}

#[test]
fn point_on_boundary_rotated() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [4.0, 2.0], FRAC_PI_4)
        .unwrap()
        .build()
        .unwrap();

    // End of semi-major axis at angle pi/4.
    let p = [4.0 * FRAC_PI_4.cos(), 4.0 * FRAC_PI_4.sin()];
    assert!(ellipse.point_in_region(&p).unwrap());

    // End of semi-minor axis at angle pi/4 + pi/2.
    let angle_minor = FRAC_PI_4 + PI / 2.0;
    let p = [2.0 * angle_minor.cos(), 2.0 * angle_minor.sin()];
    assert!(ellipse.point_in_region(&p).unwrap());
}

// ---- Negation ----

#[test]
fn negation() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    // Centre is inside.
    assert!(ellipse.point_in_region(&[0.0, 0.0]).unwrap());
    // Outside point.
    assert!(!ellipse.point_in_region(&[6.0, 0.0]).unwrap());

    // Negate.
    ellipse.negate();
    assert!(ellipse.is_negated());

    // Centre is now outside.
    assert!(!ellipse.point_in_region(&[0.0, 0.0]).unwrap());
    // Previously-outside point is now inside.
    assert!(ellipse.point_in_region(&[6.0, 0.0]).unwrap());
}

#[test]
fn negated_copy() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    let negated = ellipse.negated_copy();
    // Original is not negated.
    assert!(!ellipse.is_negated());
    // Copy is negated.
    assert!(negated.is_negated());
}

// ---- Bounding box ----

#[test]
fn bounding_box_axis_aligned() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![1.0, 2.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = ellipse.get_region_bounds().unwrap();
    assert!((lower[0] - (-4.0)).abs() < 1e-10); // 1 - 5
    assert!((lower[1] - (-1.0)).abs() < 1e-10); // 2 - 3
    assert!((upper[0] - 6.0).abs() < 1e-10); // 1 + 5
    assert!((upper[1] - 5.0).abs() < 1e-10); // 2 + 3
}

#[test]
fn bounding_box_rotated() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Semi-major = 4, semi-minor = 2, rotated 90 degrees (pi/2).
    // After rotation: x_extent = sqrt((4*0)^2 + (2*1)^2) = 2
    //                 y_extent = sqrt((4*1)^2 + (2*0)^2) = 4
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [4.0, 2.0], PI / 2.0)
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = ellipse.get_region_bounds().unwrap();
    assert!((lower[0] - (-2.0)).abs() < 1e-10);
    assert!((lower[1] - (-4.0)).abs() < 1e-10);
    assert!((upper[0] - 2.0).abs() < 1e-10);
    assert!((upper[1] - 4.0).abs() < 1e-10);
}

// ---- Batch transform (Mapping::transform) ----

#[test]
fn transform_forward_passes_interior() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    // Points: inside(2,1), outside(6,0), inside(0,0), outside(0,4)
    let pts = PointSet::from_rows(
        [
            [2.0, 1.0].as_slice(),
            [6.0, 0.0].as_slice(),
            [0.0, 0.0].as_slice(),
            [0.0, 4.0].as_slice(),
        ]
        .into_iter(),
    );

    let result = ellipse.transform(&pts, true).unwrap();

    // Point 0: inside — passes through.
    let p0 = result.point(0);
    assert!((p0[0] - 2.0).abs() < 1e-10);
    assert!((p0[1] - 1.0).abs() < 1e-10);

    // Point 1: outside — should be BAD.
    let p1 = result.point(1);
    assert_eq!(p1[0], BAD_VALUE);
    assert_eq!(p1[1], BAD_VALUE);

    // Point 2: inside (centre).
    let p2 = result.point(2);
    assert!((p2[0] - 0.0).abs() < 1e-10);
    assert!((p2[1] - 0.0).abs() < 1e-10);

    // Point 3: outside.
    let p3 = result.point(3);
    assert_eq!(p3[0], BAD_VALUE);
    assert_eq!(p3[1], BAD_VALUE);
}

#[test]
fn transform_inverse_passes_all() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[6.0, 0.0].as_slice(), [100.0, 200.0].as_slice()].into_iter());

    let result = ellipse.transform(&pts, false).unwrap();

    // Inverse always passes points through unchanged.
    let p0 = result.point(0);
    assert!((p0[0] - 6.0).abs() < 1e-10);
    assert!((p0[1] - 0.0).abs() < 1e-10);

    let p1 = result.point(1);
    assert!((p1[0] - 100.0).abs() < 1e-10);
    assert!((p1[1] - 200.0).abs() < 1e-10);
}

// ---- Mesh ----

#[test]
fn mesh_points_lie_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![1.0, 2.0], [5.0, 3.0], 0.3)
        .unwrap()
        .build()
        .unwrap();

    let mesh = ellipse.get_region_mesh().unwrap();
    let npoints = mesh.npoint();
    assert!(npoints >= 100); // Default mesh_size is 200.

    // Verify all mesh points satisfy the ellipse equation (normalised
    // distance ~ 1.0).
    let centre = &[1.0, 2.0];
    let a = 5.0_f64;
    let b = 3.0_f64;
    let angle = 0.3_f64;
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let inv_a2 = 1.0 / (a * a);
    let inv_b2 = 1.0 / (b * b);

    for i in 0..npoints {
        let p = mesh.point(i);
        let dx = p[0] - centre[0];
        let dy = p[1] - centre[1];
        let u = dx * cos_a + dy * sin_a;
        let v = -dx * sin_a + dy * cos_a;
        let d = u * u * inv_a2 + v * v * inv_b2;
        assert!(
            (d - 1.0).abs() < 1e-10,
            "Mesh point {i} has normalised distance {d}, expected 1.0"
        );
    }
}

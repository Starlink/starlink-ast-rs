//! Tests for the Ellipse region implementation.
//!
//! Covers: construction with semi_axes and angle, point inside/outside/boundary
//! for axis-aligned and rotated ellipses, negation, bounding box, batch
//! transform.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, CmpFrame, Frame, SkyFrame, SkySystem, pick_axes};
use ast_core::mappings::Mapping;
use ast_core::regions::{EllipseBuilder, Region};

// ---- Construction ----

#[test]
fn construct_axis_aligned() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(ellipse.naxes(), 2);

    // The semi-axes lie along the Frame's axes: 5 on the first, 3 on the
    // second.
    assert!(ellipse.point_in_region(&[4.9, 0.0]).unwrap());
    assert!(!ellipse.point_in_region(&[5.1, 0.0]).unwrap());
    assert!(ellipse.point_in_region(&[0.0, 2.9]).unwrap());
    assert!(!ellipse.point_in_region(&[0.0, 3.1]).unwrap());
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
    let result = EllipseBuilder::new(frame, vec![0.0, 0.0, 0.0], [5.0, 3.0], FRAC_PI_2);
    assert!(result.is_err());
}

#[test]
fn construct_rejects_wrong_centre_length() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = EllipseBuilder::new(frame, vec![0.0, 0.0, 0.0], [5.0, 3.0], FRAC_PI_2);
    assert!(result.is_err());
}

#[test]
fn construct_rejects_non_positive_axes() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = EllipseBuilder::new(frame, vec![0.0, 0.0], [0.0, 3.0], FRAC_PI_2);
    assert!(result.is_err());

    let frame2 = BasicFrameBuilder::new(2).build().unwrap();
    let result2 = EllipseBuilder::new(frame2, vec![0.0, 0.0], [5.0, -1.0], FRAC_PI_2);
    assert!(result2.is_err());
}

// ---- Point inside/outside/boundary: axis-aligned (angle=0) ----

#[test]
fn point_inside_axis_aligned() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Semi-major = 5 along axis 1, semi-minor = 3 along axis 2.
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
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
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
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
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
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
    let mut ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
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
    let mut ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
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
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
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

/// The bounds are the extremes of the boundary mesh, not the closed-form
/// extent of the ellipse, because C reads them off `RegBaseMesh`
/// (ellipse.c:988-998). Two consequences show here. A mesh sample only lands
/// on an axis tip when the tip's parametric angle is a multiple of the mesh
/// step, so an end that misses falls short by a factor `cos(2*pi/MeshSize)`.
/// And C accumulates with `if ... else if`, so the first sample — the primary
/// axis tip at parametric angle zero — claims the lower end of each axis and
/// cannot also claim the upper. Values captured from C `astEllipse` +
/// `astGetRegionBounds` at MeshSize 200.
#[test]
fn bounding_box_axis_aligned() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![1.0, 2.0], [5.0, 3.0], FRAC_PI_2)
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = ellipse.get_region_bounds().unwrap();
    assert!((lower[0] - (-4.0)).abs() < 1e-10); // 1 - 5
    assert!((lower[1] - (-1.0)).abs() < 1e-10); // 2 - 3
    // The primary axis tip took the lower end, so the upper end is the
    // neighbouring mesh point: 1 + 5*cos(2*pi/200).
    assert!((upper[0] - 5.997_532_801_828_654).abs() < 1e-10);
    assert!((upper[1] - 5.0).abs() < 1e-10); // 2 + 3
}

/// Semi-major 4, semi-minor 2, primary axis along axis 2. Axis 1 is spanned
/// exactly, because the secondary axis tips sit on mesh samples; axis 2 loses
/// its upper end to the `if ... else if`, as in
/// [`bounding_box_axis_aligned`]. Values captured from C `astEllipse` +
/// `astGetRegionBounds`.
#[test]
fn bounding_box_rotated() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [4.0, 2.0], 0.0)
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = ellipse.get_region_bounds().unwrap();
    assert!((lower[0] - (-2.0)).abs() < 1e-10);
    assert!((lower[1] - (-4.0)).abs() < 1e-10);
    assert!((upper[0] - 2.0).abs() < 1e-10);
    // 4*cos(2*pi/200).
    assert!((upper[1] - 3.998_026_241_462_923_3).abs() < 1e-10);
}

// ---- Batch transform (Mapping::transform) ----

#[test]
fn transform_forward_passes_interior() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
        .unwrap()
        .build()
        .unwrap();

    // Points: inside(2,1), outside(6,0), inside(0,0), outside(0,4)
    let pts = PointSet::from_rows([
        [2.0, 1.0].as_slice(),
        [6.0, 0.0].as_slice(),
        [0.0, 0.0].as_slice(),
        [0.0, 4.0].as_slice(),
    ]);

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
fn transform_inverse_masks_as_forward() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[6.0, 0.0].as_slice(), [100.0, 200.0].as_slice()]);

    // C's Region classes transform identically in both directions, so a
    // point outside the Region is bad on the inverse too.
    let inverse = ellipse.transform(&pts, false).unwrap();
    let forward = ellipse.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        assert_eq!(inverse.point(i), forward.point(i), "point {i}");
    }
    assert!(inverse.point(0).iter().all(|&v| v == BAD_VALUE));
}

// ---- Mesh ----

#[test]
fn mesh_points_lie_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let ellipse = EllipseBuilder::new(frame, vec![1.0, 2.0], [5.0, 3.0], FRAC_PI_2 - 0.3)
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

// ---- reg_pins: on-boundary-within-uncertainty test (astRegPins port) ----

#[test]
fn reg_pins_ellipse_edge_on_boundary() {
    // Axis-aligned ellipse: a=5 along axis 1, b=3 along axis 2.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let e = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
        .unwrap()
        .build()
        .unwrap();

    // The ends of the two semi-axes lie on the boundary.
    let major_end = PointSet::from_rows([[5.0_f64, 0.0]]);
    assert!(e.reg_pins(&major_end, None).unwrap());
    let minor_end = PointSet::from_rows([[0.0_f64, 3.0]]);
    assert!(e.reg_pins(&minor_end, None).unwrap());
}

#[test]
fn reg_pins_ellipse_interior_and_exterior_off_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let e = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
        .unwrap()
        .build()
        .unwrap();

    let interior = PointSet::from_rows([[0.0_f64, 0.0]]);
    assert!(!e.reg_pins(&interior, None).unwrap());
    let exterior = PointSet::from_rows([[20.0_f64, 20.0]]);
    assert!(!e.reg_pins(&exterior, None).unwrap());
}

#[test]
fn reg_pins_ellipse_full_mesh_on_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let e = EllipseBuilder::new(frame, vec![0.0, 0.0], [5.0, 3.0], FRAC_PI_2)
        .unwrap()
        .build()
        .unwrap();

    let mesh = e.get_region_mesh().unwrap();
    assert!(e.reg_pins(&mesh, None).unwrap());
}

// ---- Per-axis measurement rules ----

/// Each axis of an Ellipse's bounding box is measured by its own axis's rule.
///
/// Ancestor: `reference/starlink-ast/ast_tester/testregions.c`,
/// `checkEllipseAxisRules`.
///
/// `ax_distance` dispatches to the Axis on the named axis, so the index matters
/// whenever a Frame's two axes normalise differently. A sky axis folds its
/// distances into +/- pi; a plain axis does not. Neither a SkyFrame (both axes
/// fold) nor a plain Frame (neither does) can tell the two indices apart — it
/// takes a Frame whose axes are of different classes.
#[test]
fn ellipse_bounds_measure_each_axis_by_its_own_rule() {
    // The radii are deliberately larger than pi, so a folded axis and a raw one
    // cannot give the same answer.
    let radii = [4.0, 3.5];
    // A folded axis cannot report a limit beyond pi, while the plain axis here
    // reaches the 4.0 primary radius.
    let raw = PI + 1.0e-8;

    for sky_first in [true, false] {
        // A CmpFrame of a SkyFrame and a 1-D Frame, reduced to one sky axis
        // beside the plain one. With `sky_first` the sky axis is axis 1,
        // otherwise axis 2.
        let cf = CmpFrame::new(
            Box::new(SkyFrame::new(SkySystem::Fk5)),
            Box::new(BasicFrameBuilder::new(1).domain("T").build().unwrap()),
        );
        let pick = if sky_first { [1, 3] } else { [3, 1] };
        let (mixed, _map) = pick_axes(&cf, &pick).unwrap();

        let el = EllipseBuilder::from_dyn(mixed, vec![0.0, 0.0], radii, FRAC_PI_2)
            .unwrap()
            .build()
            .unwrap();
        let (lo, hi) = el.get_region_bounds().unwrap();

        let wrapped = hi[1] <= raw && lo[1] >= -raw;
        if sky_first {
            assert!(
                !wrapped,
                "axis 2 is a plain axis but its range {:.10}..{:.10} was folded",
                lo[1], hi[1]
            );
        } else {
            assert!(
                wrapped,
                "axis 2 is a sky axis but its range {:.10}..{:.10} was not folded",
                lo[1], hi[1]
            );
        }
    }
}

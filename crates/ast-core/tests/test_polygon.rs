//! Tests for the Polygon region implementation.
//!
//! Covers: construction validation, point inside/outside/boundary for
//! triangles, squares, and concave polygons, negation, bounding box,
//! and batch transform.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{PolygonBuilder, Region};

// ---- Construction validation ----

#[test]
fn construct_triangle() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let vertices = vec![vec![0.0, 0.0], vec![4.0, 0.0], vec![2.0, 3.0]];
    let poly = PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(poly.naxes(), 2);
}

/// astInitPolygon has no minimum vertex count; only an empty PointSet is
/// rejected, by astPointSet.
#[test]
fn construct_accepts_fewer_than_3_vertices() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let vertices = vec![vec![0.0, 0.0], vec![1.0, 1.0]];
    assert!(
        PolygonBuilder::new(frame, vertices)
            .unwrap()
            .build()
            .is_ok()
    );
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    assert!(PolygonBuilder::new(frame, vec![]).is_err());
}

#[test]
fn construct_rejects_non_2d_frame() {
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let vertices = vec![
        vec![0.0, 0.0, 0.0],
        vec![1.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0],
    ];
    let result = PolygonBuilder::new(frame, vertices);
    assert!(result.is_err());
}

#[test]
fn construct_rejects_1d_frame() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let vertices = vec![vec![0.0], vec![1.0], vec![2.0]];
    let result = PolygonBuilder::new(frame, vertices);
    assert!(result.is_err());
}

#[test]
fn construct_rejects_wrong_vertex_length() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let vertices = vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.5]]; // Third vertex has wrong length
    let result = PolygonBuilder::new(frame, vertices);
    assert!(result.is_err());
}

// ---- Triangle: point inside/outside/on vertex ----

fn make_triangle() -> ast_core::regions::Polygon {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Right triangle with vertices at (0,0), (4,0), (0,3)
    let vertices = vec![vec![0.0, 0.0], vec![4.0, 0.0], vec![0.0, 3.0]];
    PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn triangle_point_inside() {
    let poly = make_triangle();
    // Centroid of (0,0),(4,0),(0,3) is (4/3, 1)
    assert!(poly.point_in_region(&[1.0, 0.5]).unwrap());
    assert!(poly.point_in_region(&[0.5, 0.5]).unwrap());
}

#[test]
fn triangle_point_outside() {
    let poly = make_triangle();
    assert!(!poly.point_in_region(&[5.0, 5.0]).unwrap());
    assert!(!poly.point_in_region(&[-1.0, 0.0]).unwrap());
    assert!(!poly.point_in_region(&[3.0, 3.0]).unwrap());
}

#[test]
fn triangle_point_on_vertex() {
    let poly = make_triangle();
    // Vertices are on the boundary; closed polygon includes boundary.
    assert!(poly.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(poly.point_in_region(&[4.0, 0.0]).unwrap());
    assert!(poly.point_in_region(&[0.0, 3.0]).unwrap());
}

#[test]
fn triangle_point_on_edge() {
    let poly = make_triangle();
    // Midpoint of bottom edge (0,0)-(4,0) is (2,0).
    assert!(poly.point_in_region(&[2.0, 0.0]).unwrap());
    // Midpoint of left edge (0,0)-(0,3) is (0,1.5).
    assert!(poly.point_in_region(&[0.0, 1.5]).unwrap());
}

// ---- Square: all corners and interior ----

fn make_square() -> ast_core::regions::Polygon {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Unit square: (0,0), (1,0), (1,1), (0,1)
    let vertices = vec![
        vec![0.0, 0.0],
        vec![1.0, 0.0],
        vec![1.0, 1.0],
        vec![0.0, 1.0],
    ];
    PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn square_interior() {
    let poly = make_square();
    assert!(poly.point_in_region(&[0.5, 0.5]).unwrap());
    assert!(poly.point_in_region(&[0.1, 0.9]).unwrap());
    assert!(poly.point_in_region(&[0.99, 0.01]).unwrap());
}

#[test]
fn square_corners() {
    let poly = make_square();
    // All corners are on the boundary (closed).
    assert!(poly.point_in_region(&[0.0, 0.0]).unwrap());
    assert!(poly.point_in_region(&[1.0, 0.0]).unwrap());
    assert!(poly.point_in_region(&[1.0, 1.0]).unwrap());
    assert!(poly.point_in_region(&[0.0, 1.0]).unwrap());
}

#[test]
fn square_outside() {
    let poly = make_square();
    assert!(!poly.point_in_region(&[1.5, 0.5]).unwrap());
    assert!(!poly.point_in_region(&[-0.1, 0.5]).unwrap());
    assert!(!poly.point_in_region(&[0.5, -0.1]).unwrap());
    assert!(!poly.point_in_region(&[0.5, 1.1]).unwrap());
}

// ---- Concave polygon ----

fn make_l_shape() -> ast_core::regions::Polygon {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // L-shaped polygon:
    //   (0,0) -> (2,0) -> (2,1) -> (1,1) -> (1,2) -> (0,2)
    let vertices = vec![
        vec![0.0, 0.0],
        vec![2.0, 0.0],
        vec![2.0, 1.0],
        vec![1.0, 1.0],
        vec![1.0, 2.0],
        vec![0.0, 2.0],
    ];
    PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap()
}

// An arrowhead: a concave polygon with a triangular notch cut into its top
// edge. Traversed anti-clockwise so the inside is to the left of every edge.
//   (0,0) -> (4,0) -> (4,3) -> (2,1) -> (0,3)
// The membership results below are byte-for-byte those reported by the C
// Starlink AST Polygon Transform for the same vertices.
fn make_arrow() -> ast_core::regions::Polygon {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let vertices = vec![
        vec![0.0, 0.0],
        vec![4.0, 0.0],
        vec![4.0, 3.0],
        vec![2.0, 1.0],
        vec![0.0, 3.0],
    ];
    PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap()
}

#[test]
fn concave_inside_body() {
    let poly = make_arrow();
    // Point in the solid body below the notch.
    assert!(poly.point_in_region(&[2.0, 0.5]).unwrap());
}

#[test]
fn concave_inside_corner() {
    let poly = make_arrow();
    // Point in the lower-left corner.
    assert!(poly.point_in_region(&[0.5, 0.5]).unwrap());
}

#[test]
fn concave_in_notch() {
    let poly = make_arrow();
    // Point up inside the triangular notch, should be outside.
    assert!(!poly.point_in_region(&[2.0, 2.5]).unwrap());
}

#[test]
fn concave_outside() {
    let poly = make_arrow();
    assert!(!poly.point_in_region(&[5.0, 0.5]).unwrap());
    assert!(!poly.point_in_region(&[-0.5, 1.0]).unwrap());
}

// ---- Bounding box ----

#[test]
fn bounding_box_triangle() {
    let poly = make_triangle();
    let (lower, upper) = poly.get_region_bounds().unwrap();
    assert!((lower[0] - 0.0).abs() < 1e-10);
    assert!((lower[1] - 0.0).abs() < 1e-10);
    assert!((upper[0] - 4.0).abs() < 1e-10);
    assert!((upper[1] - 3.0).abs() < 1e-10);
}

#[test]
fn bounding_box_l_shape() {
    let poly = make_l_shape();
    let (lower, upper) = poly.get_region_bounds().unwrap();
    assert!((lower[0] - 0.0).abs() < 1e-10);
    assert!((lower[1] - 0.0).abs() < 1e-10);
    assert!((upper[0] - 2.0).abs() < 1e-10);
    assert!((upper[1] - 2.0).abs() < 1e-10);
}

// ---- Negation ----

#[test]
fn negation_inverts_membership() {
    let mut poly = make_triangle();
    // Interior point.
    assert!(poly.point_in_region(&[1.0, 0.5]).unwrap());
    // Exterior point.
    assert!(!poly.point_in_region(&[5.0, 5.0]).unwrap());

    // Negate.
    poly.negate();
    assert!(poly.is_negated());

    // Now the interior point is outside.
    assert!(!poly.point_in_region(&[1.0, 0.5]).unwrap());
    // And the exterior point is inside.
    assert!(poly.point_in_region(&[5.0, 5.0]).unwrap());
}

#[test]
fn negated_copy_preserves_original() {
    let poly = make_triangle();
    let neg = poly.negated_copy();

    assert!(!poly.is_negated());
    assert!(neg.is_negated());

    // Interior point in original.
    assert!(poly.point_in_region(&[1.0, 0.5]).unwrap());
    // Same point is exterior in negated copy.
    assert!(!neg.point_in_region(&[1.0, 0.5]).unwrap());
}

// ---- Batch transform ----

#[test]
fn batch_transform_inside_outside() {
    let poly = make_square();

    // Create a batch of points: some inside, some outside.
    let points = PointSet::from_rows(
        [
            [0.5, 0.5], // inside
            [2.0, 2.0], // outside
            [0.1, 0.1], // inside
            [5.0, 5.0], // outside
        ]
        .iter()
        .map(|p| p.as_slice()),
    );

    let result = poly.transform(&points, true).unwrap();

    // Inside points pass through.
    let p0 = result.point(0);
    assert!((p0[0] - 0.5).abs() < 1e-10);
    assert!((p0[1] - 0.5).abs() < 1e-10);

    // Outside points become BAD.
    let p1 = result.point(1);
    assert_eq!(p1[0], BAD_VALUE);
    assert_eq!(p1[1], BAD_VALUE);

    // Another inside.
    let p2 = result.point(2);
    assert!((p2[0] - 0.1).abs() < 1e-10);
    assert!((p2[1] - 0.1).abs() < 1e-10);

    // Another outside.
    let p3 = result.point(3);
    assert_eq!(p3[0], BAD_VALUE);
    assert_eq!(p3[1], BAD_VALUE);
}

#[test]
fn batch_transform_bad_input() {
    let poly = make_square();

    let points = PointSet::from_rows(
        [[BAD_VALUE, 0.5], [0.5, BAD_VALUE]]
            .iter()
            .map(|p| p.as_slice()),
    );

    let result = poly.transform(&points, true).unwrap();

    // BAD inputs always produce BAD outputs.
    assert_eq!(result.point(0)[0], BAD_VALUE);
    assert_eq!(result.point(1)[0], BAD_VALUE);
}

#[test]
fn inverse_transform_masks_as_forward() {
    let poly = make_square();

    let points = PointSet::from_rows([[5.0, 5.0], [0.5, 0.5]].iter().map(|p| p.as_slice()));

    // C's Region classes transform identically in both directions, so a
    // point outside the Region is bad on the inverse too.
    let inverse = poly.transform(&points, false).unwrap();
    let forward = poly.transform(&points, true).unwrap();
    for i in 0..points.npoint() {
        assert_eq!(inverse.point(i), forward.point(i), "point {i}");
    }
    assert!(inverse.point(0).iter().all(|&v| v == BAD_VALUE));
}

// ---- Mesh and centroid ----

#[test]
fn mesh_has_points_on_boundary() {
    let poly = make_square();
    let mesh = poly.base_mesh().unwrap();

    // All mesh points should be on the boundary of the square.
    let npoint = mesh.npoint();
    assert!(npoint > 0);

    for i in 0..npoint {
        let p = mesh.point(i);
        let x = p[0];
        let y = p[1];
        // Point is on boundary if x==0 or x==1 or y==0 or y==1
        // (and within the square's bounds).
        let on_boundary = (x.abs() < 1e-10 || (x - 1.0).abs() < 1e-10)
            || (y.abs() < 1e-10 || (y - 1.0).abs() < 1e-10);
        assert!(
            on_boundary,
            "Mesh point ({}, {}) is not on the boundary",
            x, y
        );
    }
}

#[test]
fn centroid_of_square() {
    let poly = make_square();
    let (centre, _radius) = poly.get_region_disc().unwrap();
    assert!((centre[0] - 0.5).abs() < 1e-10);
    assert!((centre[1] - 0.5).abs() < 1e-10);
}

// ---- Open polygon (boundary excluded) ----

#[test]
fn open_polygon_excludes_boundary() {
    let mut poly = make_square();
    poly.set_closed(false);

    // Interior point still inside.
    assert!(poly.point_in_region(&[0.5, 0.5]).unwrap());

    // Boundary point now excluded.
    assert!(!poly.point_in_region(&[0.0, 0.5]).unwrap());
    assert!(!poly.point_in_region(&[1.0, 0.5]).unwrap());
    assert!(!poly.point_in_region(&[0.5, 0.0]).unwrap());
    assert!(!poly.point_in_region(&[0.5, 1.0]).unwrap());

    // Vertex excluded.
    assert!(!poly.point_in_region(&[0.0, 0.0]).unwrap());
}

// ---- Region attributes ----

#[test]
fn is_bounded() {
    let poly = make_triangle();
    assert!(poly.is_bounded().unwrap());
}

#[test]
fn region_points_returns_vertices() {
    let poly = make_triangle();
    let pts = poly.base_points().unwrap();
    assert_eq!(pts.npoint(), 3);
    assert!((pts.point(0)[0] - 0.0).abs() < 1e-10);
    assert!((pts.point(0)[1] - 0.0).abs() < 1e-10);
    assert!((pts.point(1)[0] - 4.0).abs() < 1e-10);
    assert!((pts.point(1)[1] - 0.0).abs() < 1e-10);
    assert!((pts.point(2)[0] - 0.0).abs() < 1e-10);
    assert!((pts.point(2)[1] - 3.0).abs() < 1e-10);
}

// ---- reg_pins: on-boundary-within-uncertainty test (astRegPins port) ----

#[test]
fn reg_pins_polygon_edge_and_vertex_on_boundary() {
    // Square with vertices (0,0),(4,0),(4,4),(0,4).
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let vertices = vec![
        vec![0.0, 0.0],
        vec![4.0, 0.0],
        vec![4.0, 4.0],
        vec![0.0, 4.0],
    ];
    let poly = PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap();

    // A point on the bottom edge and a vertex are both on the boundary.
    let on_edge = PointSet::from_rows([[2.0_f64, 0.0]]);
    assert!(poly.reg_pins(&on_edge, None).unwrap());
    let vertex = PointSet::from_rows([[4.0_f64, 4.0]]);
    assert!(poly.reg_pins(&vertex, None).unwrap());
}

#[test]
fn reg_pins_polygon_interior_and_exterior_off_boundary() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let vertices = vec![
        vec![0.0, 0.0],
        vec![4.0, 0.0],
        vec![4.0, 4.0],
        vec![0.0, 4.0],
    ];
    let poly = PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap();

    let interior = PointSet::from_rows([[2.0_f64, 2.0]]);
    assert!(!poly.reg_pins(&interior, None).unwrap());
    let exterior = PointSet::from_rows([[10.0_f64, 10.0]]);
    assert!(!poly.reg_pins(&exterior, None).unwrap());
}

#[test]
fn reg_pins_polygon_mixed_set_reports_false() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let vertices = vec![
        vec![0.0, 0.0],
        vec![4.0, 0.0],
        vec![4.0, 4.0],
        vec![0.0, 4.0],
    ];
    let poly = PolygonBuilder::new(frame, vertices)
        .unwrap()
        .build()
        .unwrap();

    // One edge point, one interior point: the whole set fails.
    let mixed = PointSet::from_rows([[2.0_f64, 0.0], [2.0, 2.0]]);
    assert!(!poly.reg_pins(&mixed, None).unwrap());
}

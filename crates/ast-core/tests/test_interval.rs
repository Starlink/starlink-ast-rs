//! Tests for the Interval region implementation.
//!
//! Covers: bounded intervals (both axes), unbounded axes, BAD handling,
//! open/closed boundary, excluded intervals, batch transform, bounds query.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{IntervalBuilder, Region};

/// Check whether a value is the AST bad-value sentinel.
fn is_bad(v: f64) -> bool {
    v == BAD_VALUE
}

// ---- Construction ----

#[test]
fn construct_bounded_2d() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(interval.naxes(), 2);
    assert!(interval.is_bounded());
    assert!(!interval.is_negated());
    assert!(interval.is_closed());
}

#[test]
fn construct_unbounded_axis() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Lower bound on axis 1 is BAD (no lower limit).
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    assert!(!interval.is_bounded());
}

#[test]
fn construct_mismatched_axes_error() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = IntervalBuilder::new(frame, vec![1.0, 2.0, 3.0], vec![5.0, 8.0]);
    assert!(result.is_err());
}

#[test]
fn construct_mismatched_ubnd_error() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let result = IntervalBuilder::new(frame, vec![1.0, 2.0], vec![5.0]);
    assert!(result.is_err());
}

// ---- Point membership: bounded interval ----

#[test]
fn point_inside_bounded() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    // Centre point.
    assert!(interval.point_in_region(&[3.0, 5.0]).unwrap());

    // Corner point (closed boundary includes it).
    assert!(interval.point_in_region(&[1.0, 2.0]).unwrap());
    assert!(interval.point_in_region(&[5.0, 8.0]).unwrap());
    assert!(interval.point_in_region(&[1.0, 8.0]).unwrap());
    assert!(interval.point_in_region(&[5.0, 2.0]).unwrap());
}

#[test]
fn point_outside_bounded() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    // Outside on axis 1.
    assert!(!interval.point_in_region(&[0.5, 5.0]).unwrap());
    assert!(!interval.point_in_region(&[5.5, 5.0]).unwrap());

    // Outside on axis 2.
    assert!(!interval.point_in_region(&[3.0, 1.5]).unwrap());
    assert!(!interval.point_in_region(&[3.0, 8.5]).unwrap());

    // Outside on both.
    assert!(!interval.point_in_region(&[0.0, 0.0]).unwrap());
}

// ---- Unbounded axis ----

#[test]
fn unbounded_lower_accepts_any_below() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // No lower bound on axis 1 (BAD), upper bound is 10.
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // Very negative value on axis 1 should pass (no lower constraint).
    assert!(interval.point_in_region(&[-1e20, 5.0]).unwrap());
    // But above upper bound should fail.
    assert!(!interval.point_in_region(&[11.0, 5.0]).unwrap());
}

#[test]
fn unbounded_upper_accepts_any_above() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Lower bound is 0, no upper bound (BAD).
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![BAD_VALUE, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // Very large value on axis 1 should pass (no upper constraint).
    assert!(interval.point_in_region(&[1e20, 5.0]).unwrap());
    // Below lower bound should fail.
    assert!(!interval.point_in_region(&[-1.0, 5.0]).unwrap());
}

#[test]
fn fully_unbounded_axis() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    // Axis 1 is fully unconstrained, axis 2 is bounded [0, 10].
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 0.0], vec![BAD_VALUE, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // Any value on axis 1 passes, only axis 2 matters.
    assert!(interval.point_in_region(&[-1e30, 5.0]).unwrap());
    assert!(interval.point_in_region(&[1e30, 5.0]).unwrap());
    assert!(!interval.point_in_region(&[0.0, 11.0]).unwrap());
}

// ---- Open boundary ----

#[test]
fn open_boundary_excludes_edges() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut interval = IntervalBuilder::new(frame, vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();
    interval.set_closed(false);

    // Boundary points now excluded.
    assert!(!interval.point_in_region(&[1.0, 5.0]).unwrap());
    assert!(!interval.point_in_region(&[3.0, 8.0]).unwrap());
    assert!(!interval.point_in_region(&[5.0, 5.0]).unwrap());
    assert!(!interval.point_in_region(&[3.0, 2.0]).unwrap());

    // Interior point still inside.
    assert!(interval.point_in_region(&[3.0, 5.0]).unwrap());
}

// ---- Excluded interval (lb > ub) ----

#[test]
fn excluded_interval_wraps_around() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    // lb=8, ub=2 means the region is x<=2 OR x>=8.
    let interval = IntervalBuilder::new(frame, vec![8.0], vec![2.0])
        .unwrap()
        .build()
        .unwrap();

    // Inside the excluded region (outside [2, 8]).
    assert!(interval.point_in_region(&[1.0]).unwrap());
    assert!(interval.point_in_region(&[9.0]).unwrap());
    assert!(interval.point_in_region(&[2.0]).unwrap()); // closed, = ub
    assert!(interval.point_in_region(&[8.0]).unwrap()); // closed, = lb

    // Outside the excluded region (inside [2, 8]).
    assert!(!interval.point_in_region(&[5.0]).unwrap());
    assert!(!interval.point_in_region(&[3.0]).unwrap());
    assert!(!interval.point_in_region(&[7.0]).unwrap());
}

// ---- Negation ----

#[test]
fn negated_inverts_membership() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();
    interval.negate();

    // Interior of original → now outside.
    assert!(!interval.point_in_region(&[5.0, 5.0]).unwrap());

    // Exterior of original → now inside.
    assert!(interval.point_in_region(&[15.0, 5.0]).unwrap());
    assert!(interval.point_in_region(&[-1.0, 5.0]).unwrap());
}

// ---- Batch transform ----

#[test]
fn forward_transform_masks_outside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows(
        [
            [5.0, 5.0],  // inside
            [15.0, 5.0], // outside
            [5.0, -1.0], // outside
            [0.0, 10.0], // boundary (inside, closed)
        ]
        .iter()
        .map(|p| p.as_slice()),
    );

    let result = interval.transform(&pts, true).unwrap();
    assert_eq!(result.npoint(), 4);

    // Point 0: inside → pass through.
    let p = result.point(0);
    assert_eq!(p[0], 5.0);
    assert_eq!(p[1], 5.0);

    // Point 1: outside → BAD.
    let p = result.point(1);
    assert!(is_bad(p[0]));
    assert!(is_bad(p[1]));

    // Point 2: outside → BAD.
    let p = result.point(2);
    assert!(is_bad(p[0]));
    assert!(is_bad(p[1]));

    // Point 3: boundary, closed → pass through.
    let p = result.point(3);
    assert_eq!(p[0], 0.0);
    assert_eq!(p[1], 10.0);
}

#[test]
fn inverse_transform_passes_all() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[15.0, 5.0], [-1.0, -1.0]].iter().map(|p| p.as_slice()));

    let result = interval.transform(&pts, false).unwrap();
    // Inverse always passes through.
    let p = result.point(0);
    assert_eq!(p[0], 15.0);
    assert_eq!(p[1], 5.0);
    let p = result.point(1);
    assert_eq!(p[0], -1.0);
    assert_eq!(p[1], -1.0);
}

// ---- BAD input ----

#[test]
fn bad_input_gives_outside() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // BAD on one axis.
    assert!(!interval.point_in_region(&[BAD_VALUE, 5.0]).unwrap());
    assert!(!interval.point_in_region(&[5.0, BAD_VALUE]).unwrap());
}

#[test]
fn bad_input_in_batch_transform() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[5.0, 5.0], [BAD_VALUE, 5.0]].iter().map(|p| p.as_slice()));

    let result = interval.transform(&pts, true).unwrap();

    // First point: inside → pass.
    let p = result.point(0);
    assert_eq!(p[0], 5.0);

    // Second point: BAD input → BAD output.
    let p = result.point(1);
    assert!(is_bad(p[0]));
    assert!(is_bad(p[1]));
}

// ---- Bounds ----

#[test]
fn bounds_of_bounded_interval() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = interval.get_region_bounds().unwrap();
    assert_eq!(lower[0], 1.0);
    assert_eq!(lower[1], 2.0);
    assert_eq!(upper[0], 5.0);
    assert_eq!(upper[1], 8.0);
}

#[test]
fn bounds_of_unbounded_interval() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 2.0], vec![5.0, BAD_VALUE])
        .unwrap()
        .build()
        .unwrap();

    let (lower, upper) = interval.get_region_bounds().unwrap();
    // Axis 1: no lower bound → -MAX.
    assert_eq!(lower[0], -f64::MAX);
    assert_eq!(upper[0], 5.0);
    // Axis 2: no upper bound → +MAX.
    assert_eq!(lower[1], 2.0);
    assert_eq!(upper[1], f64::MAX);
}

// ---- Centre ----

#[test]
fn centre_of_bounded_interval() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 2.0], vec![10.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    let (centre, _radius) = interval.get_region_disc().unwrap();
    assert!((centre[0] - 5.0).abs() < 1e-10);
    assert!((centre[1] - 5.0).abs() < 1e-10);
}

#[test]
fn centre_of_unbounded_interval_fails() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    assert!(interval.get_region_disc().is_err());
}

// ---- Mesh ----

#[test]
fn mesh_of_bounded_2d_has_corners() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let mesh = interval.get_region_mesh().unwrap();
    // 2^2 = 4 corners.
    assert_eq!(mesh.npoint(), 4);
}

#[test]
fn mesh_of_unbounded_is_single_bad() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let mesh = interval.get_region_mesh().unwrap();
    // Unbounded: single BAD point.
    assert_eq!(mesh.npoint(), 1);
    assert!(is_bad(mesh.point(0)[0]));
}

// ---- map_region ----

#[test]
fn map_region_scales_bounds() {
    use ast_core::mappings::ZoomMap;

    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame.clone(), vec![1.0, 2.0], vec![5.0, 8.0])
        .unwrap()
        .build()
        .unwrap();

    let zoom = ZoomMap::new(2, 2.0).unwrap();
    let mapped = interval.map_region(&zoom, &frame).unwrap();

    // Bounds should be doubled.
    assert!(mapped.point_in_region(&[6.0, 10.0]).unwrap()); // 3*2, 5*2 → inside
    assert!(!mapped.point_in_region(&[1.0, 3.0]).unwrap()); // would need >= 2.0
}

// ---- Frame delegation ----

#[test]
fn frame_distance_delegation() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let d = interval.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1e-10);
}

// ---- 1D interval ----

#[test]
fn one_dimensional_interval() {
    let frame = BasicFrameBuilder::new(1).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![2.0], vec![8.0])
        .unwrap()
        .build()
        .unwrap();

    assert!(interval.point_in_region(&[5.0]).unwrap());
    assert!(interval.point_in_region(&[2.0]).unwrap()); // closed
    assert!(interval.point_in_region(&[8.0]).unwrap()); // closed
    assert!(!interval.point_in_region(&[1.0]).unwrap());
    assert!(!interval.point_in_region(&[9.0]).unwrap());
}

// ---- Large batch performance sanity check ----

#[test]
fn batch_10k_points() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![100.0, 100.0])
        .unwrap()
        .build()
        .unwrap();

    // Create 10000 points: half inside, half outside.
    let n = 10_000;
    let mut rows: Vec<[f64; 2]> = Vec::with_capacity(n);
    for i in 0..n {
        if i % 2 == 0 {
            rows.push([50.0, 50.0]); // inside
        } else {
            rows.push([150.0, 50.0]); // outside
        }
    }
    let pts = PointSet::from_rows(rows.iter().map(|p| p.as_slice()));

    let result = interval.transform(&pts, true).unwrap();

    // Check alternating pattern.
    for i in 0..n {
        let p = result.point(i);
        if i % 2 == 0 {
            assert_eq!(p[0], 50.0, "point {i} should pass through");
        } else {
            assert!(is_bad(p[0]), "point {i} should be BAD");
        }
    }
}

// ---- Native-PointSet layout: agreement with C AST encoding ----
//
// The C AST `pointset.c::Dump` emits `X1..X{npoint*ncoord}` keys with
// outer iteration over `point` and inner iteration over `coord`. So
// for a PointSet with `npoint=2` and `ncoord=2`, the encoding is:
//   X1 = (point 0, coord 0)
//   X2 = (point 0, coord 1)
//   X3 = (point 1, coord 0)
//   X4 = (point 1, coord 1)
//
// Our `serial::write_pointset_node` and `read_pointset_node` swap this
// iteration order (coord-outer, pt-inner). Reader and writer remain
// inverse-pairs of each other, so round-tripping data through our
// code preserves bytes; but builder-built data serialised by us
// disagrees with C-emitted bytes for any non-square (npoint != ncoord)
// or non-symmetric data, and C-emitted .map files parsed by us
// produce data with `[pt][coord]` indices effectively transposed.
//
// The two tests below pin the behaviour expected from a fix and are
// `#[ignore]`d. Once the loop order in `write_pointset_node` /
// `read_pointset_node` is swapped to match C, both tests pass — and
// the simplify-regression fixtures' downstream consumers
// (`Interval::simplified` Box reclassification, `BoxRegion::try_merge`,
// `Interval::try_merge`) will need to read points back as
// `[pt][coord]` rather than the transposed `[axis][bound]` layout
// used today. See commits 59437d0, b8da8cb, 17ede67 for those
// downstream sites.

/// Builder-built Interval with asymmetric 2-D bounds should serialise
/// to a native string whose X-key sequence matches C's iteration
/// order (point-outer, coord-inner). Currently fails: our writer
/// emits column-major bytes (coord-outer, pt-inner).
#[test]
#[ignore = "exposes pointset_layout bug (writer disagrees with C)"]
fn builder_serialised_pointset_uses_c_layout() {
    use ast_core::{AnyObject, to_native_string};

    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![3.0, 4.0], vec![7.0, 8.0])
        .unwrap()
        .build()
        .unwrap();
    let s = to_native_string(&AnyObject::Interval(interval)).unwrap();

    // C convention: lbnd's coords come first (X1=3, X2=4), then ubnd's
    // coords (X3=7, X4=8). Our writer's transposed iteration emits
    // X1=3, X2=7, X3=4, X4=8 instead.
    let expect_c_order = [
        ("X1 = 3", "lbnd[0]"),
        ("X2 = 4", "lbnd[1]"),
        ("X3 = 7", "ubnd[0]"),
        ("X4 = 8", "ubnd[1]"),
    ];
    for (needle, what) in expect_c_order {
        assert!(
            s.contains(needle),
            "expected `{needle}` ({what}) — but native output was:\n{s}"
        );
    }
}

/// Hand-crafted C-format native string for a 2-D Interval with
/// asymmetric bounds should parse such that `effective_bounds()`
/// returns the original lbnd / ubnd. Currently fails: our reader
/// transposes the X-keys, so `effective_bounds` reports degenerate
/// bounds for non-symmetric input.
#[test]
#[ignore = "exposes pointset_layout bug (reader disagrees with C)"]
fn parsed_c_format_pointset_preserves_bounds() {
    use ast_core::{AnyObject, from_native_string};

    // C-style PointSet layout (point-outer, coord-inner):
    //   X1..X2 = point 0 = lbnd = (3, 4)
    //   X3..X4 = point 1 = ubnd = (7, 8)
    let s = r#"
 Begin Interval
    Naxes = 2
 IsA Frame
    Frm =
       Begin Frame
          Naxes = 2
          Ax1 = Begin Axis End Axis
          Ax2 = Begin Axis End Axis
       End Frame
    Points =
       Begin PointSet
          Npoint = 2
          Ncoord = 2
          X1 = 3
          X2 = 4
          X3 = 7
          X4 = 8
       End PointSet
 IsA Region
 End Interval
"#;
    let obj = from_native_string(s).expect("parse Interval native string");
    let interval = match obj {
        AnyObject::Interval(i) => i,
        other => panic!("expected Interval, got {other:?}"),
    };
    // Round-trip through public API: contains((3, 4)) should be true
    // (lbnd is the lower corner, inclusive). contains((7, 8)) likewise
    // (ubnd is the upper corner). contains((3, 7)) should be true if
    // bounds are [3, 7] x [4, 8].
    let pts = PointSet::from_rows([[3.0, 4.0], [7.0, 8.0], [5.0, 6.0]]);
    let inside = interval.transform(&pts, true).unwrap();
    for k in 0..3 {
        let in_x = pts.point(k)[0];
        let in_y = pts.point(k)[1];
        let out_x = inside.point(k)[0];
        assert!(
            !is_bad(out_x),
            "({in_x}, {in_y}) should be inside Interval bounds [3,7]x[4,8] — got BAD; \
             native parsing is producing transposed/degenerate bounds"
        );
    }
}

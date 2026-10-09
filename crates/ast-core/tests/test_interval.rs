//! Tests for the Interval region implementation.
//!
//! Covers: bounded intervals (both axes), unbounded axes, BAD handling,
//! open/closed boundary, excluded intervals, batch transform, bounds query.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
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
    assert!(interval.is_bounded().unwrap());
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

    assert!(!interval.is_bounded().unwrap());
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
fn inverse_transform_masks_as_forward() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let pts = PointSet::from_rows([[15.0, 5.0], [-1.0, -1.0]].iter().map(|p| p.as_slice()));

    // C's Region classes transform identically in both directions, so a
    // point outside the Region is bad on the inverse too.
    let inverse = interval.transform(&pts, false).unwrap();
    let forward = interval.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        assert_eq!(inverse.point(i), forward.point(i), "point {i}");
    }
    assert!(inverse.point(0).iter().all(|&v| v == BAD_VALUE));
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

// ---- Mesh ----

/// The boundary mesh of a bounded Interval is its equivalent Box's
/// (interval.c `RegBaseMesh`); tests/test_region_interval_oracle.rs has C's.
#[test]
fn mesh_of_bounded_2d_is_the_box_mesh() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let mesh = interval.get_region_mesh().unwrap();
    assert_eq!(mesh.npoint(), 200);
}

/// An unbounded Interval has no finite boundary, so no mesh
/// (interval.c `RegBaseMesh` reports AST__INTER).
#[test]
fn mesh_of_unbounded_is_an_error() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    assert!(interval.get_region_mesh().is_err());
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
// outer iteration over `point` and inner iteration over `coord`. For
// `npoint=2 ncoord=2` the encoding is:
//   X1 = (point 0, coord 0)    X2 = (point 0, coord 1)
//   X3 = (point 1, coord 0)    X4 = (point 1, coord 1)
//
// `write_pointset_node` and `read_pointset_node` use this same order;
// the two tests below pin agreement with the C encoding (so a
// round-trip C → us → C is byte-equivalent).

/// Builder-built Interval with asymmetric 2-D bounds serialises to a
/// native string whose X-key sequence matches C's iteration order.
#[test]
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
/// asymmetric bounds parses such that the public-API `transform`
/// reports the source bounds correctly (i.e. points within the
/// claimed [3,7] x [4,8] region pass through, not BAD).
#[test]
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

// ---- reg_pins: on-boundary-within-uncertainty test (astRegPins port) ----

#[test]
fn reg_pins_bounded_interval_delegates_to_box() {
    // A fully-bounded Interval is Box-equivalent: RegPins must behave like
    // the Box on-boundary test. Band ~5e-6/axis from the default uncertainty.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![0.0, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // On an edge / corner → on boundary.
    let on_edge = PointSet::from_rows([[0.0_f64, 5.0]]);
    assert!(interval.reg_pins(&on_edge, None).unwrap());
    let corner = PointSet::from_rows([[10.0_f64, 10.0]]);
    assert!(interval.reg_pins(&corner, None).unwrap());

    // Interior / exterior → not on boundary.
    let centre = PointSet::from_rows([[5.0_f64, 5.0]]);
    assert!(!interval.reg_pins(&centre, None).unwrap());
    let far = PointSet::from_rows([[20.0_f64, 20.0]]);
    assert!(!interval.reg_pins(&far, None).unwrap());
}

#[test]
fn reg_pins_semi_infinite_interval_boundary() {
    // Interval unbounded below on axis 0 (lbnd BAD), bounded above at 10;
    // axis 1 bounded [0,10].
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    // The x==10 face is on the boundary even though axis 0 is unbounded below:
    // an upper limit is still a limit. C `astRegPins` returns 1 here.
    let on_x_face = PointSet::from_rows([[10.0_f64, 5.0]]);
    assert!(interval.reg_pins(&on_x_face, None).unwrap());

    // A point deep in the interior is likewise not on the boundary.
    let interior = PointSet::from_rows([[-100.0_f64, 5.0]]);
    assert!(!interval.reg_pins(&interior, None).unwrap());
}

#[test]
fn reg_pins_semi_infinite_boundary_mesh_when_bounded_axis() {
    // The finite y-face boundary of a semi-infinite interval: y==0 within the
    // x<=10 half-plane is on the boundary.
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let interval = IntervalBuilder::new(frame, vec![BAD_VALUE, 0.0], vec![10.0, 10.0])
        .unwrap()
        .build()
        .unwrap();

    let on_y_face = PointSet::from_rows([[5.0_f64, 0.0]]);
    assert!(interval.reg_pins(&on_y_face, None).unwrap());

    // `reg_pins` requires *every* point to be on the boundary, so a set mixing
    // the on-boundary x-face with an interior point fails on the latter.
    let mixed = PointSet::from_rows([[10.0_f64, 5.0], [-100.0, 5.0]]);
    assert!(!interval.reg_pins(&mixed, None).unwrap());
}

// ---- Merging with a PointList inside a Prism ----
//
// Ancestor: checkIntervalPointListMerge in
// reference/starlink-ast/ast_tester/testregions.c.
//
// Absorbing a PointList into an Interval attaches one constant per Interval
// axis to every point, which is only equivalent when the Interval is a point.

#[test]
fn wide_interval_does_not_merge_into_a_pointlist() {
    use ast_core::regions::{PointListBuilder, PrismBuilder};

    // An Interval spanning 0 to 10 on axis 1 and unbounded above on axis 2 is
    // nowhere near a point, so the Prism holds every (x, y, z) with x in
    // 0..10, y >= 0 and z in {3, 4} while a PointList would hold only its
    // points.
    let f1 = BasicFrameBuilder::new(1).build().unwrap();
    let f2 = BasicFrameBuilder::new(2).build().unwrap();
    let pl = PointListBuilder::new(f1, vec![vec![3.0], vec![4.0]])
        .unwrap()
        .build()
        .unwrap();
    let iv = IntervalBuilder::new(f2, vec![0.0, 0.0], vec![10.0, BAD_VALUE])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(iv, pl).unwrap().build().unwrap();
    let simp = <dyn Mapping>::simplify(&prism).unwrap();
    let simp: &dyn Region = simp
        .as_ref()
        .as_any()
        .downcast_ref::<ast_core::regions::Prism>()
        .map(|p| p as &dyn Region)
        .or_else(|| {
            simp.as_ref()
                .as_any()
                .downcast_ref::<ast_core::regions::PointList>()
                .map(|p| p as &dyn Region)
        })
        .expect("the simplified Prism should still be a Region");

    assert!(
        !simp.as_any().is::<ast_core::regions::PointList>(),
        "a wide Interval must not merge into a PointList"
    );
    assert!(
        simp.point_in_region(&[2.0, 7.0, 3.0]).unwrap(),
        "(2,7,3) is inside the Prism"
    );
    assert!(
        !simp.point_in_region(&[20.0, 1.0, 3.0]).unwrap(),
        "(20,1,3) is outside the Prism"
    );
}

#[test]
fn point_interval_merges_into_a_pointlist() {
    use ast_core::regions::{PointListBuilder, PrismBuilder};

    // An Interval with zero width on every axis is a point, so the Prism is
    // the PointList with the point's axis values attached to every member.
    let f1 = BasicFrameBuilder::new(1).build().unwrap();
    let f2 = BasicFrameBuilder::new(2).build().unwrap();
    let pl = PointListBuilder::new(f1, vec![vec![3.0], vec![4.0]])
        .unwrap()
        .build()
        .unwrap();
    let iv = IntervalBuilder::new(f2, vec![5.0, 0.0], vec![5.0, 0.0])
        .unwrap()
        .build()
        .unwrap();

    let prism = PrismBuilder::new(iv, pl).unwrap().build().unwrap();
    let simp = <dyn Mapping>::simplify(&prism).unwrap();
    let merged = simp
        .as_ref()
        .as_any()
        .downcast_ref::<ast_core::regions::PointList>()
        .expect("a point Interval should merge into a PointList");

    let pts = merged.get_region_points().unwrap();
    assert_eq!(pts.npoint(), 2);
    assert_eq!(pts.point(0), &[5.0, 0.0, 3.0]);
    assert_eq!(pts.point(1), &[5.0, 0.0, 4.0]);
}

/// Ported from testregions.c `checkIntervalCentreFrame`: an Interval mapped
/// through a ZoomMap of 2 without simplification keeps its base Frame limits
/// of 1 and 3, so its centre is 2 in the base Frame and 4 in the current
/// Frame (interval.c `RegCentre`).
#[test]
fn interval_centre_is_given_in_the_frame_asked_for() {
    use ast_core::frames::FrameRef;
    use ast_core::mappings::ZoomMap;

    let frame = || BasicFrameBuilder::new(1).build().unwrap();
    let iv = IntervalBuilder::new(frame(), vec![1.0], vec![3.0])
        .unwrap()
        .build()
        .unwrap();
    let mapped = iv
        .reg_map_region(&ZoomMap::new(1, 2.0).unwrap(), &frame())
        .unwrap();
    assert_eq!(
        mapped.reg_centre_query(FrameRef::Base).unwrap(),
        Some(vec![2.0])
    );
    assert_eq!(
        mapped.reg_centre_query(FrameRef::Current).unwrap(),
        Some(vec![4.0])
    );
}

#[test]
fn mesh_of_an_unbounded_interval_names_the_class() {
    // region.c `GetRegionMesh`, checked against the C library.
    let interval = IntervalBuilder::new(
        BasicFrameBuilder::new(1).build().unwrap(),
        vec![BAD_VALUE],
        vec![1.0],
    )
    .unwrap()
    .build()
    .unwrap();
    let err = interval.region_mesh().unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mbbnf);
    assert_eq!(
        err.message(),
        "astGetRegionMesh(Interval): The supplied Interval is unbounded so no mesh can be \
         created to cover it."
    );
}

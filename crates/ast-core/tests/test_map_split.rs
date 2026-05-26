//! Tests for the C-faithful `Mapping::map_split` (mirrors astMapSplit).
//!
//! The new signature takes `in_axes: &[usize]` (input indices to retain)
//! and returns `Option<(Vec<usize>, DynMapping)>` where the `Vec<usize>`
//! is the list of original-mapping output axes the kept inputs feed into,
//! and the `DynMapping` is the sub-mapping with `nin == in_axes.len()`
//! and `nout == returned_outs.len()`.
//!
//! C ancestor: reference/starlink-ast/src/mapping.c::MapSplit (~7798)
//! and the per-class overrides in unitmap.c / zoommap.c / shiftmap.c.

use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, ShiftMap, UnitMap, ZoomMap};

/// Helper: run forward through `m` on a single 1-row input.
fn fwd_point(m: &dyn Mapping, input: &[f64]) -> Vec<f64> {
    let pts = PointSet::from_rows(vec![input.to_vec()]);
    m.transform(&pts, true).unwrap().point(0).to_vec()
}

// ========================================================================
// UnitMap
// ========================================================================

#[test]
fn unit_split_picks_chosen_axes() {
    let u = UnitMap::new(3);
    let (outs, sub) = u.map_split(&[0, 2]).expect("split");
    assert_eq!(outs, vec![0, 2]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
    // Identity: in == out.
    assert_eq!(fwd_point(sub.as_ref(), &[7.0, 9.0]), vec![7.0, 9.0]);
}

#[test]
fn unit_split_full_inputs() {
    let u = UnitMap::new(3);
    let (outs, sub) = u.map_split(&[0, 1, 2]).expect("split");
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(sub.nin(), 3);
    assert_eq!(sub.nout(), 3);
}

#[test]
fn unit_split_reorders() {
    let u = UnitMap::new(3);
    let (outs, sub) = u.map_split(&[2, 0, 1]).expect("split");
    // For UnitMap the output axes are exactly in_axes.
    assert_eq!(outs, vec![2, 0, 1]);
    assert_eq!(sub.nin(), 3);
    assert_eq!(sub.nout(), 3);
}

#[test]
fn unit_split_rejects_duplicate_input() {
    let u = UnitMap::new(3);
    assert!(u.map_split(&[1, 1]).is_none());
}

#[test]
fn unit_split_rejects_out_of_range() {
    let u = UnitMap::new(3);
    assert!(u.map_split(&[0, 3]).is_none());
}

// ========================================================================
// ZoomMap
// ========================================================================

#[test]
fn zoom_split_carries_factor() {
    let z = ZoomMap::new(3, 5.0).unwrap();
    let (outs, sub) = z.map_split(&[0]).expect("split");
    assert_eq!(outs, vec![0]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    assert_eq!(fwd_point(sub.as_ref(), &[2.0]), vec![10.0]);
}

#[test]
fn zoom_split_subset_two_axes() {
    let z = ZoomMap::new(4, 3.0).unwrap();
    let (outs, sub) = z.map_split(&[1, 3]).expect("split");
    assert_eq!(outs, vec![1, 3]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
    assert_eq!(fwd_point(sub.as_ref(), &[2.0, 5.0]), vec![6.0, 15.0]);
}

#[test]
fn zoom_split_preserves_invert() {
    let mut z = ZoomMap::new(3, 2.0).unwrap();
    z.invert();
    let (outs, sub) = z.map_split(&[0, 2]).expect("split");
    assert_eq!(outs, vec![0, 2]);
    // Inverted ZoomMap(2) divides by 2.
    assert_eq!(fwd_point(sub.as_ref(), &[10.0, 6.0]), vec![5.0, 3.0]);
}

#[test]
fn zoom_split_rejects_invalid() {
    let z = ZoomMap::new(3, 5.0).unwrap();
    assert!(z.map_split(&[3]).is_none());
    assert!(z.map_split(&[1, 1]).is_none());
}

// ========================================================================
// ShiftMap
// ========================================================================

#[test]
fn shift_split_picks_axis_offset() {
    let s = ShiftMap::new(vec![10.0, 20.0, 30.0]);
    let (outs, sub) = s.map_split(&[2, 0]).expect("split");
    assert_eq!(outs, vec![2, 0]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
    // Sub input axis 0 is the original axis 2 → shift +30.
    // Sub input axis 1 is the original axis 0 → shift +10.
    let out = fwd_point(sub.as_ref(), &[5.0, 1.0]);
    assert_eq!(out[0], 35.0);
    assert_eq!(out[1], 11.0);
}

#[test]
fn shift_split_full_inputs_in_order() {
    let s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    let (outs, sub) = s.map_split(&[0, 1, 2]).expect("split");
    // Default impl path: full inputs in order, identity perm → no PermMap
    // wrap. Returned outs are the full output range of self.
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(
        fwd_point(sub.as_ref(), &[10.0, 20.0, 30.0]),
        vec![11.0, 22.0, 33.0]
    );
}

#[test]
fn shift_split_preserves_invert() {
    let mut s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    s.invert();
    let (outs, sub) = s.map_split(&[0, 1]).expect("split");
    assert_eq!(outs, vec![0, 1]);
    // Inverted ShiftMap subtracts the shift.
    assert_eq!(fwd_point(sub.as_ref(), &[10.0, 20.0]), vec![9.0, 18.0]);
}

#[test]
fn shift_split_rejects_invalid() {
    let s = ShiftMap::new(vec![1.0, 2.0]);
    assert!(s.map_split(&[2]).is_none());
    assert!(s.map_split(&[0, 0]).is_none());
}

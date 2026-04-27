//! Tests for PermMap.
//!
//! Ancestor: behaviour derived from the C AST PermMap documentation.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, PermMap};

/// Swap axes: outperm=[2,1] maps (x,y) → (y,x).
#[test]
fn perm_map_swap_axes_forward() {
    // nin=2 (inperm length), nout=2 (outperm length)
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    assert_eq!(pm.nin(), 2);
    assert_eq!(pm.nout(), 2);
    let pts = PointSet::from_rows([[3.0, 7.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[7.0, 3.0]);
}

/// Constant axis: outperm=[1, -1] maps x → (x, constant[0]).
#[test]
fn perm_map_constant_axis() {
    let pm = PermMap::new(&[1], &[1, -1], &[42.0]).unwrap();
    assert_eq!(pm.nin(), 1);
    assert_eq!(pm.nout(), 2);
    let pts = PointSet::from_rows([[5.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 5.0);
    assert_eq!(out.point(0)[1], 42.0);
}

/// Inverse transform uses inperm.
#[test]
fn perm_map_inverse_uses_inperm() {
    // outperm=[2,1] swaps, inperm=[2,1] also swaps (self-inverse).
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let pts = PointSet::from_rows([[10.0, 20.0]]);
    let out = pm.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[20.0, 10.0]);
}

/// Zero perm entry → BAD_VALUE in that output slot.
#[test]
fn perm_map_zero_entry_is_bad() {
    let pm = PermMap::new(&[1], &[1, 0], &[]).unwrap();
    let pts = PointSet::from_rows([[5.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 5.0);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

/// BAD_VALUE in any input coord → whole output point is BAD_VALUE.
#[test]
fn perm_map_bad_value_propagation() {
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 7.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

/// Identity permutation [1, 2] simplifies to a working UnitMap.
#[test]
fn perm_map_identity_simplifies() {
    let pm = PermMap::new(&[1, 2], &[1, 2], &[]).unwrap();
    let simplified = pm.simplified().unwrap();
    let pts = PointSet::from_rows([[4.0, 8.0]]);
    let out = simplified.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[4.0, 8.0]);
}

/// Non-identity permutation does not simplify to UnitMap.
#[test]
fn perm_map_non_identity_not_simplified() {
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let simplified = pm.simplified().unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = simplified.transform(&pts, true).unwrap();
    // Should still swap.
    assert_eq!(out.point(0), &[2.0, 1.0]);
}

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

/// BAD_VALUE in an input coord → only output axes that select it are BAD.
#[test]
fn perm_map_bad_value_propagation() {
    // outperm=[2,1]: out[0]=in[1], out[1]=in[0]
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 7.0]]);
    let out = pm.transform(&pts, true).unwrap();
    // out[0] = in[1] = 7.0 (unaffected by BAD in in[0])
    assert_eq!(out.point(0)[0], 7.0);
    // out[1] = in[0] = BAD
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

/// Per-axis BAD handling: only the selected source axis matters for each output.
#[test]
fn perm_bad_per_axis() {
    // outperm: [2, -1, 1] => out[0]=in[1], out[1]=const[0], out[2]=in[0]
    // inperm: [3, 1, 0]   => inv: out[0]=in[2], out[1]=in[0], out[2]=BAD
    let pm = PermMap::new(&[3, 1, 0], &[2, -1, 1], &[99.0]).unwrap();

    // Forward: input[0]=BAD. out[0]=in[1]=5.0 (not affected),
    //          out[1]=const=99.0 (never affected), out[2]=in[0]=BAD.
    let pts = PointSet::from_rows([[BAD_VALUE, 5.0, 7.0]]);
    let out = pm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 5.0);
    assert_eq!(out.point(0)[1], 99.0);
    assert_eq!(out.point(0)[2], BAD_VALUE);
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

/// Analytical rate for PermMap: 1.0 if output axis depends on input axis, else 0.0.
#[test]
fn perm_rate_analytical() {
    // outperm: [2, 1] => out[0]=in[1], out[1]=in[0]
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    // rate(output 1, input 1) = 0 (output 1 doesn't depend on input 1)
    assert_eq!(pm.rate(&[0.0, 0.0], 1, 1).unwrap(), 0.0);
    // rate(output 1, input 2) = 1 (output 1 = input 2)
    assert_eq!(pm.rate(&[0.0, 0.0], 1, 2).unwrap(), 1.0);
    // rate(output 2, input 1) = 1 (output 2 = input 1)
    assert_eq!(pm.rate(&[0.0, 0.0], 2, 1).unwrap(), 1.0);
    // rate(output 2, input 2) = 0
    assert_eq!(pm.rate(&[0.0, 0.0], 2, 2).unwrap(), 0.0);
}

/// Analytical rate for inverted PermMap uses inperm.
#[test]
fn perm_rate_analytical_inverted() {
    // outperm: [2, 1], inperm: [2, 1] (swap both ways)
    let mut pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    pm.invert();
    // Inverted: uses inperm [2, 1] => same swap pattern
    assert_eq!(pm.rate(&[0.0, 0.0], 1, 1).unwrap(), 0.0);
    assert_eq!(pm.rate(&[0.0, 0.0], 1, 2).unwrap(), 1.0);
    assert_eq!(pm.rate(&[0.0, 0.0], 2, 1).unwrap(), 1.0);
    assert_eq!(pm.rate(&[0.0, 0.0], 2, 2).unwrap(), 0.0);
}

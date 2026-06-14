//! Tests for PermMap.
//!
//! Ancestor: behaviour derived from the C AST PermMap documentation.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, PermMap, UnitMap};

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

/// Parallel merge: two adjacent parallel PermMaps combine into one larger PermMap.
#[test]
fn perm_parallel_merge() {
    // PermMap [2,1] parallel with PermMap [1] → combined 3-axis PermMap
    // First 2 outputs come from first PermMap (swaps inputs 1,2),
    // third output comes from second PermMap (passes input 3).
    let p1 = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let p2 = PermMap::new(&[1], &[1], &[]).unwrap();

    let merged = p1.try_merge(&p2, false, false, false);
    assert!(merged.is_some());
    let m = merged.unwrap();
    assert_eq!(m.nin(), 3);
    assert_eq!(m.nout(), 3);

    // Verify: input [a, b, c] → [b, a, c]
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 2.0);
    assert_eq!(out.point(0)[1], 1.0);
    assert_eq!(out.point(0)[2], 3.0);
}

/// simplified() must check both inperm and outperm for identity.
#[test]
fn perm_simplified_checks_both_perms() {
    // Identity outperm but non-identity inperm -> NOT a UnitMap.
    let pm = PermMap::new(&[2, 1], &[1, 2], &[]).unwrap();
    let s = pm.simplified().unwrap();
    assert!(!s.as_any().is::<UnitMap>());

    // Both identity -> IS a UnitMap.
    let pm2 = PermMap::new(&[1, 2], &[1, 2], &[]).unwrap();
    let s2 = pm2.simplified().unwrap();
    assert!(s2.as_any().is::<UnitMap>());
}

// ---------------------------------------------------------------------------
// Equality: C `PermMap::Equal` compares referenced constant *values*, not
// the constants pool positionally (permmap.c:224). All four cases below are
// oracle-verified against the reference C library.
// ---------------------------------------------------------------------------

#[test]
fn perm_equal_same_constant_index_ignores_value_quirk() {
    // Both inputs are fed by constants[0]. C compares the internal perm
    // values first and, finding them equal, never compares the referenced
    // constant values — so the PermMaps are deemed EQUAL even though the
    // stored constants (5.0 vs 7.0) differ. This is a C quirk; see
    // docs/issues/c-library-quirks.md. Oracle: astEqual == 1.
    let a = PermMap::new(&[-1], &[1], &[5.0]).unwrap();
    let b = PermMap::new(&[-1], &[1], &[7.0]).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn perm_equal_different_index_same_referenced_value() {
    // The inputs reference different constant slots (-1 vs -2), but both
    // referenced values are 9.0, so the PermMaps are equal. Oracle: == 1.
    let a = PermMap::new(&[-1], &[1], &[9.0, 0.0]).unwrap();
    let b = PermMap::new(&[-2], &[1], &[0.0, 9.0]).unwrap();
    assert!(a.equal(&b));
}

#[test]
fn perm_equal_different_index_different_referenced_value() {
    // Different slots whose referenced values differ (9.0 vs 4.0) -> the
    // PermMaps are unequal. Oracle: == 0.
    let a = PermMap::new(&[-1], &[1], &[9.0, 0.0]).unwrap();
    let b = PermMap::new(&[-2], &[1], &[0.0, 4.0]).unwrap();
    assert!(!a.equal(&b));
}

#[test]
fn perm_equal_ignores_unreferenced_surplus_constants() {
    // Neither PermMap references a constant (both axes pass through), so
    // the differing surplus constants (1.0 vs 2.0) are ignored and the
    // PermMaps are equal. Oracle: == 1.
    let a = PermMap::new(&[1], &[1], &[1.0]).unwrap();
    let b = PermMap::new(&[1], &[1], &[2.0]).unwrap();
    assert!(a.equal(&b));
}

// ---------------------------------------------------------------------------
// Out-of-range perm entries: C `astInitPermMap` (permmap.c:2849-2903) performs
// NO validation of the perm-array values, and `Transform` (permmap.c:1976-1997)
// substitutes AST__BAD for a negative entry whose constant slot is absent or a
// positive entry past the axis count. Construction must therefore SUCCEED and
// the value resolve to BAD at transform time. All cases oracle-verified against
// the reference C library (see /tmp probes in the DM-54793 work log).
// ---------------------------------------------------------------------------

#[test]
fn perm_negative_entry_without_constant_is_bad_not_rejected() {
    // outperm=[-1], inperm=[-1], NO constants supplied. C accepts this
    // (NULL constants array) and transforms to AST__BAD in both directions.
    // Oracle: construct astOK==1, forward==AST__BAD, inverse==AST__BAD.
    let pm = PermMap::new(&[-1], &[-1], &[]).unwrap();
    let pts = PointSet::from_rows([[99.0]]);
    let fwd = pm.transform(&pts, true).unwrap();
    assert_eq!(fwd.point(0)[0], BAD_VALUE);
    let inv = pm.transform(&pts, false).unwrap();
    assert_eq!(inv.point(0)[0], BAD_VALUE);
}

#[test]
fn perm_negative_entry_past_constants_pool_is_bad() {
    // outperm=[-2] references constants[1], but only constants[0] is given.
    // C reads past the (shorter) caller array at construction but resolves
    // missing slots to BAD; the safe Rust port substitutes BAD directly.
    let pm = PermMap::new(&[-2], &[-2], &[5.0]).unwrap();
    let pts = PointSet::from_rows([[99.0]]);
    let fwd = pm.transform(&pts, true).unwrap();
    assert_eq!(fwd.point(0)[0], BAD_VALUE);
}

#[test]
fn perm_positive_entry_past_axis_count_is_bad() {
    // nin=1, outperm=[2] selects input axis 2 which does not exist. C's
    // Transform yields AST__BAD. Oracle-verified (probe case B).
    let pm = PermMap::new(&[1], &[2], &[]).unwrap();
    let pts = PointSet::from_rows([[7.0]]);
    let fwd = pm.transform(&pts, true).unwrap();
    assert_eq!(fwd.point(0)[0], BAD_VALUE);
}

#[test]
fn perm_c_dump_with_negative_entry_no_nconst_roundtrips() {
    // A C PermMap built with NULL constants and negative perm entries dumps
    // as `Out1 = -1` / `In1 = -1` with NO `Nconst` line. Rust must be able
    // to read this native form and transform it to BAD, matching C's reread
    // behaviour (oracle probe: reread forward == AST__BAD).
    use ast_core::{AnyObject, from_native_string, to_native_string};
    let pm = PermMap::new(&[-1], &[-1], &[]).unwrap();
    let text = to_native_string(&AnyObject::PermMap(pm)).unwrap();
    let obj = from_native_string(&text).unwrap();
    let AnyObject::PermMap(reread) = obj else {
        panic!("expected PermMap");
    };
    let pts = PointSet::from_rows([[3.0]]);
    let fwd = reread.transform(&pts, true).unwrap();
    assert_eq!(fwd.point(0)[0], BAD_VALUE);
}

#[test]
fn perm_dimension_changing_merge_preserves_invertibility() {
    // A dimension-changing PermMap (2->3, copying input 2 to two outputs) is
    // invertible (it copies/drops axes). When such a PermMap is series-
    // composed with a diagonal scaling MatrixMap-equivalent (here a parallel
    // ZoomMap pair), the simplify engine must NOT fold the PermMap into a
    // rectangular (3x2) MatrixMap — that would be non-invertible and silently
    // drop the inverse direction. The simplified mapping must still round-trip.
    use ast_core::mappings::{CmpMap, Composition, ZoomMap};

    // 2->3 PermMap: out0=in0, out1=in1, out2=in1 (C outperm=[0,1,1] 1-based [1,2,2]).
    let perm = PermMap::new(&[1, 3], &[1, 2, 2], &[]).unwrap();
    // 3->3 diagonal scaling that drops nothing: scale each axis.
    let scale = {
        let z01 = CmpMap::new(
            Box::new(ZoomMap::new(1, 1.0e-26).unwrap()),
            Box::new(ZoomMap::new(1, 2.0).unwrap()),
            Composition::Parallel,
        )
        .unwrap();
        CmpMap::new(
            Box::new(z01),
            Box::new(ZoomMap::new(1, 3.0).unwrap()),
            Composition::Parallel,
        )
        .unwrap()
    };
    let chain = CmpMap::new(Box::new(perm), Box::new(scale), Composition::Series).unwrap();
    let simp = chain.simplified().unwrap();

    // Forward then inverse must round-trip (the inverse must exist).
    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let fwd = simp.transform(&pts, true).unwrap();
    let back = simp
        .transform(&fwd, false)
        .expect("simplified mapping must retain an inverse");
    assert!((back.point(0)[0] - 5.0).abs() < 1.0e-12);
    assert!((back.point(0)[1] - 7.0).abs() < 1.0e-12);
}

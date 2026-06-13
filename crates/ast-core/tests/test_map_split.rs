//! Tests for the C-faithful map-split algorithm.
//!
//! These exercise the protected, **0-based** `Mapping::map_split_zero_based`
//! (the direct analogue of C's protected `astMapSplit`, mapping.c:7798): it
//! takes `in_axes: &[usize]` (0-based input indices to retain) and returns
//! `Option<(Vec<usize>, DynMapping)>` where the `Vec<usize>` is the list of
//! 0-based original-mapping output axes the kept inputs feed into, and the
//! `DynMapping` is the sub-mapping with `nin == in_axes.len()` and
//! `nout == returned_outs.len()`. The per-class overrides implement this
//! 0-based form; the public 1-based `Mapping::map_split` is a thin ±1 wrapper
//! tested separately at the bottom of this file.
//!
//! C ancestor: reference/starlink-ast/src/mapping.c::MapSplit (~7798)
//! and the per-class overrides in unitmap.c / zoommap.c / shiftmap.c.

use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, PermMap, ShiftMap, UnitMap, WinMap, ZoomMap};

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
    let (outs, sub) = u.map_split_zero_based(&[0, 2]).expect("split");
    assert_eq!(outs, vec![0, 2]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
    // Identity: in == out.
    assert_eq!(fwd_point(sub.as_ref(), &[7.0, 9.0]), vec![7.0, 9.0]);
}

#[test]
fn unit_split_full_inputs() {
    let u = UnitMap::new(3);
    let (outs, sub) = u.map_split_zero_based(&[0, 1, 2]).expect("split");
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(sub.nin(), 3);
    assert_eq!(sub.nout(), 3);
}

#[test]
fn unit_split_reorders() {
    let u = UnitMap::new(3);
    let (outs, sub) = u.map_split_zero_based(&[2, 0, 1]).expect("split");
    // For UnitMap the output axes are exactly in_axes.
    assert_eq!(outs, vec![2, 0, 1]);
    assert_eq!(sub.nin(), 3);
    assert_eq!(sub.nout(), 3);
}

#[test]
fn unit_split_rejects_duplicate_input() {
    let u = UnitMap::new(3);
    assert!(u.map_split_zero_based(&[1, 1]).is_none());
}

#[test]
fn unit_split_rejects_out_of_range() {
    let u = UnitMap::new(3);
    assert!(u.map_split_zero_based(&[0, 3]).is_none());
}

// ========================================================================
// ZoomMap
// ========================================================================

#[test]
fn zoom_split_carries_factor() {
    let z = ZoomMap::new(3, 5.0).unwrap();
    let (outs, sub) = z.map_split_zero_based(&[0]).expect("split");
    assert_eq!(outs, vec![0]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    assert_eq!(fwd_point(sub.as_ref(), &[2.0]), vec![10.0]);
}

#[test]
fn zoom_split_subset_two_axes() {
    let z = ZoomMap::new(4, 3.0).unwrap();
    let (outs, sub) = z.map_split_zero_based(&[1, 3]).expect("split");
    assert_eq!(outs, vec![1, 3]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
    assert_eq!(fwd_point(sub.as_ref(), &[2.0, 5.0]), vec![6.0, 15.0]);
}

#[test]
fn zoom_split_preserves_invert() {
    let mut z = ZoomMap::new(3, 2.0).unwrap();
    z.invert();
    let (outs, sub) = z.map_split_zero_based(&[0, 2]).expect("split");
    assert_eq!(outs, vec![0, 2]);
    // Inverted ZoomMap(2) divides by 2.
    assert_eq!(fwd_point(sub.as_ref(), &[10.0, 6.0]), vec![5.0, 3.0]);
}

#[test]
fn zoom_split_rejects_invalid() {
    let z = ZoomMap::new(3, 5.0).unwrap();
    assert!(z.map_split_zero_based(&[3]).is_none());
    assert!(z.map_split_zero_based(&[1, 1]).is_none());
}

// ========================================================================
// ShiftMap
// ========================================================================

#[test]
fn shift_split_picks_axis_offset() {
    let s = ShiftMap::new(vec![10.0, 20.0, 30.0]);
    let (outs, sub) = s.map_split_zero_based(&[2, 0]).expect("split");
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
    let (outs, sub) = s.map_split_zero_based(&[0, 1, 2]).expect("split");
    // ShiftMap has its own map_split override (shift.rs); this test
    // confirms it succeeds for full inputs in order and applies each
    // shift to the matching axis.
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
    let (outs, sub) = s.map_split_zero_based(&[0, 1]).expect("split");
    assert_eq!(outs, vec![0, 1]);
    // Inverted ShiftMap subtracts the shift.
    assert_eq!(fwd_point(sub.as_ref(), &[10.0, 20.0]), vec![9.0, 18.0]);
}

#[test]
fn shift_split_rejects_invalid() {
    let s = ShiftMap::new(vec![1.0, 2.0]);
    assert!(s.map_split_zero_based(&[2]).is_none());
    assert!(s.map_split_zero_based(&[0, 0]).is_none());
}

// ========================================================================
// WinMap (per-class override added in Task 1.2)
// ========================================================================

/// Build a 3-axis `WinMap`: `y_i = shifts[i] + scales[i] * x_i`.
fn winmap_3axis() -> WinMap {
    WinMap::new(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]).expect("winmap")
}

#[test]
fn winmap_split_full_inputs_identity_perm() {
    // Full inputs in identity order: per-class impl returns a sub-WinMap
    // identical to `self` (same shifts, same scales) and outs=[0,1,2].
    let w = winmap_3axis();
    let (outs, sub) = w.map_split_zero_based(&[0, 1, 2]).expect("split");
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(sub.nin(), 3);
    assert_eq!(sub.nout(), 3);
    // Reproduces the original WinMap.
    let expected = fwd_point(&w, &[4.0, 5.0, 6.0]);
    assert_eq!(fwd_point(sub.as_ref(), &[4.0, 5.0, 6.0]), expected);
    // Spot-check the values explicitly: 1 + 10*4 = 41, 2 + 20*5 = 102,
    // 3 + 30*6 = 183.
    assert_eq!(expected, vec![41.0, 102.0, 183.0]);
}

#[test]
fn winmap_split_full_inputs_permuted() {
    // Full inputs but reordered. Per-class impl returns a sub-WinMap
    // whose axis i has the (shift, scale) of original axis in_axes[i].
    // outs == in_axes (same per-axis correspondence), and the sub-output
    // is computed directly from the picked coefficients — there is no
    // PermMap/CmpMap fallback because WinMap now has its own MapSplit.
    let w = winmap_3axis();
    let (outs, sub) = w.map_split_zero_based(&[2, 0, 1]).expect("split");
    assert_eq!(outs, vec![2, 0, 1]);
    assert_eq!(sub.nin(), 3);
    assert_eq!(sub.nout(), 3);
    // sub-input 0 -> axis 2 -> 3 + 30*5 = 153
    // sub-input 1 -> axis 0 -> 1 + 10*6 = 61
    // sub-input 2 -> axis 1 -> 2 + 20*7 = 142
    assert_eq!(
        fwd_point(sub.as_ref(), &[5.0, 6.0, 7.0]),
        vec![153.0, 61.0, 142.0]
    );
}

#[test]
fn winmap_split_rejects_duplicate_or_out_of_range() {
    // Per-class validation: duplicates and out-of-range axes both yield
    // None (project convention; see UnitMap/ZoomMap/ShiftMap impls).
    let w = winmap_3axis();
    assert!(w.map_split_zero_based(&[0, 1, 1]).is_none());
    assert!(w.map_split_zero_based(&[0, 1, 3]).is_none());
}

#[test]
fn winmap_split_rejects_empty() {
    // WinMap requires at least one axis, so `in_axes=[]` yields None
    // through WinMap::new returning `Badwm`.
    let w = winmap_3axis();
    assert!(w.map_split_zero_based(&[]).is_none());
}

#[test]
fn winmap_split_picks_per_axis_coeffs() {
    // 3-axis WinMap: y_i = shifts[i] + scales[i] * x_i
    let w = WinMap::new(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]).unwrap();
    let (outs, sub) = w.map_split_zero_based(&[2, 0]).expect("split");
    assert_eq!(outs, vec![2, 0]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
    let pts = PointSet::from_rows(vec![vec![5.0, 1.0]]);
    let out = sub.transform(&pts, true).unwrap();
    // input pos 0 -> axis 2 -> 3 + 30*5 = 153
    // input pos 1 -> axis 0 -> 1 + 10*1 = 11
    assert!((out.point(0)[0] - 153.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 11.0).abs() < 1e-12);
}

#[test]
fn winmap_split_preserves_invert() {
    // Inverted WinMap: forward becomes (input - shift) / scale. The split
    // sub-mapping must inherit the inverted flag.
    let mut w = WinMap::new(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]).unwrap();
    w.invert();
    let (outs, sub) = w.map_split_zero_based(&[2, 0]).expect("split");
    assert_eq!(outs, vec![2, 0]);
    // sub-input 0 -> axis 2 -> (153 - 3) / 30 = 5
    // sub-input 1 -> axis 0 -> (11  - 1) / 10 = 1
    let out = fwd_point(sub.as_ref(), &[153.0, 11.0]);
    assert!((out[0] - 5.0).abs() < 1e-12);
    assert!((out[1] - 1.0).abs() < 1e-12);
}

// ========================================================================
// MatrixMap
// ========================================================================

#[test]
fn matrix_split_diagonal_picks_axis() {
    use ast_core::mappings::MatrixMap;
    use ndarray::array;
    // Diagonal: every axis splits cleanly.
    let m = array![[2.0_f64, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 5.0]];
    let mm = MatrixMap::new(m).unwrap();
    let (outs, sub) = mm.map_split_zero_based(&[1]).expect("split");
    assert_eq!(outs, vec![1]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    let pts = PointSet::from_rows(vec![vec![4.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 12.0).abs() < 1e-12);
}

#[test]
fn matrix_split_block_diagonal() {
    use ast_core::mappings::MatrixMap;
    use ndarray::array;
    // Block-diagonal: first 2 axes coupled, third independent.
    let m = array![[1.0_f64, 2.0, 0.0], [3.0, 4.0, 0.0], [0.0, 0.0, 5.0],];
    let mm = MatrixMap::new(m).unwrap();
    let (outs, sub) = mm.map_split_zero_based(&[2]).expect("split");
    assert_eq!(outs, vec![2]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    // sub forward: y = 5 * x.
    let out = fwd_point(sub.as_ref(), &[7.0]);
    assert!((out[0] - 35.0).abs() < 1e-12);
    // Picking axes [0, 1] should also succeed (block 1).
    let (outs2, sub2) = mm.map_split_zero_based(&[0, 1]).expect("split block 1");
    assert_eq!(outs2, vec![0, 1]);
    assert_eq!(sub2.nin(), 2);
    assert_eq!(sub2.nout(), 2);
    // Verify forward action: y0 = 1*x0 + 2*x1, y1 = 3*x0 + 4*x1.
    let out2 = fwd_point(sub2.as_ref(), &[1.0, 1.0]);
    assert!((out2[0] - 3.0).abs() < 1e-12);
    assert!((out2[1] - 7.0).abs() < 1e-12);
}

#[test]
fn matrix_split_coupled_returns_none() {
    use ast_core::mappings::MatrixMap;
    use ndarray::array;
    // Fully coupled: every output depends on every input, so picking a
    // proper subset of inputs is impossible.
    let m = array![[1.0_f64, 1.0], [1.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();
    assert!(mm.map_split_zero_based(&[0]).is_none());
    assert!(mm.map_split_zero_based(&[1]).is_none());
    // Picking all inputs is fine (default fallback handles full case).
    let (outs, sub) = mm.map_split_zero_based(&[0, 1]).expect("full split");
    assert_eq!(outs, vec![0, 1]);
    assert_eq!(sub.nin(), 2);
}

#[test]
fn matrix_split_inverted_uses_effective_forward() {
    use ast_core::mappings::MatrixMap;
    use ndarray::array;
    // Square matrix whose inverse is itself diagonal. After invert(), the
    // effective forward is the inverse — which is also block-diagonal, so
    // the split must succeed against the inverse's row structure.
    let m = array![[2.0_f64, 0.0], [0.0, 4.0]];
    let mut mm = MatrixMap::new(m).unwrap();
    mm.invert();
    // Now effective forward: y0 = 0.5 * x0, y1 = 0.25 * x1.
    let (outs, sub) = mm.map_split_zero_based(&[0]).expect("split");
    assert_eq!(outs, vec![0]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    // Sub forward should be 0.5 * x (the inverse's row 0 entry).
    let out = fwd_point(sub.as_ref(), &[8.0]);
    assert!((out[0] - 4.0).abs() < 1e-12);
}

#[test]
fn matrix_split_empty_kept_outs_returns_none() {
    use ast_core::mappings::MatrixMap;
    use ndarray::array;
    // Rectangular matrix where the chosen input feeds NO output (all
    // entries in column 0 are zero). C: nout == 0 => return NULL.
    let m = array![[0.0_f64, 1.0], [0.0, 2.0], [0.0, 3.0]];
    let mm = MatrixMap::new(m).unwrap();
    // Picking input 0 alone yields no in-scope output rows.
    assert!(mm.map_split_zero_based(&[0]).is_none());
    // Picking input 1 yields all three rows.
    let (outs, sub) = mm.map_split_zero_based(&[1]).expect("split");
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 3);
}

// ========================================================================
// PermMap (per-class override added in Task 1.4)
// ========================================================================
//
// C ancestor: reference/starlink-ast/src/permmap.c::MapSplit (~1438).
// The per-class override walks the effective forward perm to identify
// outputs whose source is in the kept-input set; the effective inverse
// perm is then used to construct the sub-PermMap's inperm. Constants in
// outperm cause that output to be silently dropped (NOT a None result);
// constants in inperm are passed through verbatim into the sub-PermMap.

#[test]
fn perm_split_simple_swap() {
    // Swap axes 0 and 1: outperm = [2, 1] means output 0 takes input 1,
    // output 1 takes input 0. inperm = [2, 1] is the inverse swap.
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let (outs, sub) = pm.map_split_zero_based(&[1]).expect("split kept input 1");
    // Input 1 feeds output 0 (outperm[0] = 2 -> input idx 1 = kept).
    assert_eq!(outs, vec![0]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    // Sub forward: identity (the sub takes the kept input value through).
    let pts = PointSet::from_rows(vec![vec![5.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 5.0).abs() < 1e-12);
}

#[test]
fn perm_split_identity() {
    // Identity 3-axis PermMap: every input feeds the same-indexed output.
    let pm = PermMap::new(&[1, 2, 3], &[1, 2, 3], &[]).unwrap();
    let (outs, sub) = pm.map_split_zero_based(&[0, 1, 2]).expect("identity split");
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(sub.nin(), 3);
    assert_eq!(sub.nout(), 3);
    assert_eq!(
        fwd_point(sub.as_ref(), &[1.0, 2.0, 3.0]),
        vec![1.0, 2.0, 3.0]
    );
}

#[test]
fn perm_split_constant_outperm_silently_dropped() {
    // outperm = [1, -1]: output 0 takes input 0; output 1 is constant 42.
    // Per C MapSplit: constant outputs are silently dropped (the C loop
    // only adds outputs whose `iin >= 0 && iin < npin`, so iin < 0 simply
    // skips). The split succeeds with the constant output excluded.
    //
    // Plan said "return None"; the C source is authoritative and the
    // actual behaviour is "drop". The implementation mirrors C.
    let pm = PermMap::new(&[1], &[1, -1], &[42.0]).unwrap();
    let (outs, sub) = pm
        .map_split_zero_based(&[0])
        .expect("constant output is dropped");
    assert_eq!(outs, vec![0]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    let pts = PointSet::from_rows(vec![vec![7.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 7.0).abs() < 1e-12);
}

#[test]
fn perm_split_constant_inperm_passed_through() {
    // inperm = [-1] with constant 42: the inverse of input 0 yields 42.
    // outperm = [1] is forward identity. Splitting on [0] keeps output 0
    // and the inperm constant entry is preserved verbatim in the sub.
    let pm = PermMap::new(&[-1], &[1], &[42.0]).unwrap();
    let (outs, sub) = pm
        .map_split_zero_based(&[0])
        .expect("constant inperm survives");
    assert_eq!(outs, vec![0]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    // Forward: identity.
    let pts = PointSet::from_rows(vec![vec![7.0]]);
    assert!((sub.transform(&pts, true).unwrap().point(0)[0] - 7.0).abs() < 1e-12);
    // Inverse: yields the constant 42 regardless of input.
    let inv = sub.transform(&pts, false).unwrap();
    assert!((inv.point(0)[0] - 42.0).abs() < 1e-12);
}

#[test]
fn perm_split_unselected_output_feeds_kept_input_returns_none() {
    // outperm = [1, 2]: identity forward.
    // inperm = [2, 1]: inverse swap — input 0's source (in inverse) is
    // output 1.
    // Split on [0]: output 1's source is input 1 (not kept), so output 1
    // is dropped. But input 0's source (in inverse) is output 1 — which
    // is NOT in the kept output set. The split must return None.
    let pm = PermMap::new(&[2, 1], &[1, 2], &[]).unwrap();
    assert!(pm.map_split_zero_based(&[0]).is_none());
}

#[test]
fn perm_split_inverted_uses_effective_forward() {
    // Construct a PermMap where invert() flips both the dimensions and the
    // active perm direction. Raw shape: nin=2, nout=1.
    //   raw inperm  = [-1, 1]   (inverse: input 0 from constant 99,
    //                            input 1 from output 0)
    //   raw outperm = [2]       (forward: output 0 from input 1)
    // Inverted: nin=1, nout=2; effective forward = inperm = [-1, 1]
    // (output 0 = constant 99, output 1 = input 0); effective inverse =
    // outperm = [2] (the lone input is fed by output 1).
    //
    // Split on in_axes=[0]: output 0 is constant (silently dropped);
    // output 1 traces back to input 0 (kept). The inverse-direction lookup
    // for input 0 yields output 1, which is in the kept set, so the split
    // succeeds with outs=[1] (the surviving original-output index).
    let mut pm = PermMap::new(&[-1, 1], &[2], &[99.0]).unwrap();
    pm.invert();
    let (outs, sub) = pm.map_split_zero_based(&[0]).expect("split inverted");
    assert_eq!(outs, vec![1]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    // Sub forward: identity (single kept input feeds single kept output).
    let pts = PointSet::from_rows(vec![vec![42.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 42.0).abs() < 1e-12);
    // Sanity check: the sub's output corresponds to original output 1,
    // which the inverted parent computes as `input[0]` directly.
    let parent_in = PointSet::from_rows(vec![vec![42.0]]);
    let parent_out = pm.transform(&parent_in, true).unwrap();
    assert!((parent_out.point(0)[1] - out.point(0)[0]).abs() < 1e-12);
}

#[test]
fn perm_split_rejects_duplicate_or_out_of_range() {
    let pm = PermMap::new(&[1, 2, 3], &[1, 2, 3], &[]).unwrap();
    assert!(pm.map_split_zero_based(&[0, 0]).is_none());
    assert!(pm.map_split_zero_based(&[0, 3]).is_none());
}

#[test]
fn perm_split_unmatched_input_yields_none() {
    // 1-input → 1-output PermMap whose only output is fed by a constant
    // (so kept_outs is empty after the forward walk). The split must
    // return None because there are no surviving outputs.
    let pm = PermMap::new(&[1], &[-1], &[5.0]).unwrap();
    assert!(pm.map_split_zero_based(&[0]).is_none());
}

// ========================================================================
// WcsMap (per-class override added in Task 1.5)
// ========================================================================
//
// C ancestor: reference/starlink-ast/src/wcsmap.c::MapSplit (~3342). C first
// defers to the basic-Mapping parent (only succeeds when nin == mapnin) and
// then runs a WcsMap-specific override for partial input selections:
//
//   - both celestial axes kept       -> smaller WcsMap with permuted axes
//   - neither celestial axis kept    -> UnitMap (pass-through subset)
//   - only one celestial axis kept   -> PermMap dropping that axis
//                                       (only for nin > 1; otherwise None)

#[test]
fn wcs_split_full_inputs_succeeds() {
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let (outs, sub) = wm
        .map_split_zero_based(&[0, 1])
        .expect("full identity split");
    assert_eq!(outs, vec![0, 1]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
    // Sub forward should reproduce the original WcsMap forward exactly
    // (it's a clone of self via the parent identity-perm path).
    let pts = PointSet::from_rows(vec![vec![0.1_f64, 1.4]]);
    let parent = wm.transform(&pts, true).unwrap();
    let child = sub.transform(&pts, true).unwrap();
    assert!((parent.point(0)[0] - child.point(0)[0]).abs() < 1e-12);
    assert!((parent.point(0)[1] - child.point(0)[1]).abs() < 1e-12);
}

#[test]
fn wcs_split_drops_celestial_axis_returns_none_2axis() {
    // 2-axis WcsMap: dropping either celestial axis leaves nin == 1 and
    // the lone kept input is celestial. C falls into the `nin > 1` else
    // branch and frees the result — Rust returns None.
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    assert!(wm.map_split_zero_based(&[0]).is_none());
    assert!(wm.map_split_zero_based(&[1]).is_none());
}

#[test]
fn wcs_split_3axis_keeps_both_celestial_yields_wcsmap() {
    // 3-axis WcsMap, lonax=1, latax=2 (both celestial in the first two
    // input positions). Pick [0, 1] -> both celestial in kept set; the
    // result is a 2-axis WcsMap with the same projection.
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    let (outs, sub) = wm
        .map_split_zero_based(&[0, 1])
        .expect("split keeps celestial");
    assert_eq!(outs, vec![0, 1]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
    // Sub should behave like a 2-axis TAN projection — forward then inverse
    // recovers the input (TAN is invertible near the origin).
    let pts = PointSet::from_rows(vec![vec![0.1_f64, 1.4]]);
    let fwd = sub.transform(&pts, true).unwrap();
    let back = sub.transform(&fwd, false).unwrap();
    assert!((back.point(0)[0] - 0.1).abs() < 1e-10);
    assert!((back.point(0)[1] - 1.4).abs() < 1e-10);
}

#[test]
fn wcs_split_3axis_passthrough_only_yields_unitmap() {
    // 3-axis WcsMap, lonax=1, latax=2 (axes 0 and 1 are celestial).
    // Picking the pass-through axis [2] alone -> neither celestial in
    // kept set -> UnitMap with nin = nout = 1.
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    let (outs, sub) = wm.map_split_zero_based(&[2]).expect("pass-through subset");
    assert_eq!(outs, vec![2]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    // Identity behaviour.
    assert_eq!(fwd_point(sub.as_ref(), &[42.5]), vec![42.5]);
}

#[test]
fn wcs_split_3axis_only_lon_kept_drops_via_permmap() {
    // 3-axis WcsMap, lonax=1, latax=2. Picking [2, 0] keeps only the
    // longitude axis among celestial -> C constructs a PermMap that drops
    // the kept celestial position from the output (C: nin=2, nout=1).
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    let (outs, sub) = wm
        .map_split_zero_based(&[2, 0])
        .expect("split drops celestial");
    // The kept-input positions are [2, 0]; the longitude axis is at
    // position 1 (in_axes[1] == 0 == lonax). The C code shifts position
    // ilatlon=1 out of `result`, leaving result == [2].
    assert_eq!(outs, vec![2]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 1);
    // Forward: takes both kept inputs, produces a single output that is
    // the pass-through axis (sub-input 0, == original axis 2).
    let pts = PointSet::from_rows(vec![vec![123.0_f64, 0.5]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 123.0).abs() < 1e-12);
    // Inverse: the dropped celestial position should yield BAD.
    use ast_core::constants::BAD_VALUE;
    let inv_pts = PointSet::from_rows(vec![vec![7.0_f64]]);
    let inv = sub.transform(&inv_pts, false).unwrap();
    assert!((inv.point(0)[0] - 7.0).abs() < 1e-12); // sub-input 0 = forward input 0
    assert_eq!(inv.point(0)[1], BAD_VALUE); // sub-input 1 = dropped celestial
}

#[test]
fn wcs_split_preserves_invert_full_set() {
    // Inverted WcsMap, full identity split -> the returned sub clones the
    // original (including the invert flag).
    use ast_core::mappings::{WcsMap, WcsProjection};
    let mut wm = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    wm.invert();
    let (_outs, sub) = wm.map_split_zero_based(&[0, 1]).expect("full split");
    assert!(sub.is_inverted());
}

#[test]
fn wcs_split_preserves_invert_subset() {
    // Inverted 3-axis WcsMap, both-celestial subset -> the new WcsMap
    // inherits the invert flag (C: astSetInvert(newwcs, astGetInvert(this))).
    use ast_core::mappings::{WcsMap, WcsProjection};
    let mut wm = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    wm.invert();
    let (outs, sub) = wm
        .map_split_zero_based(&[0, 1])
        .expect("split keeps celestial");
    assert_eq!(outs, vec![0, 1]);
    assert!(sub.is_inverted());
}

#[test]
fn wcs_split_rejects_duplicate_or_out_of_range() {
    // Project convention: validation rejects duplicates and out-of-range
    // up front. C's WcsMap override only checks the in-range half but the
    // parent path catches out-of-range; the duplicate case is degenerate
    // in C either way.
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    assert!(wm.map_split_zero_based(&[0, 0, 1]).is_none());
    assert!(wm.map_split_zero_based(&[0, 1, 3]).is_none());
}

#[test]
fn wcs_split_full_set_permuted_simplifies_through() {
    // Full inputs but reordered: parent path builds PermMap+self and
    // simplifies. The semantic result is equivalent to running the full
    // original mapping with the input-axis permutation applied first.
    use ast_core::mappings::{WcsMap, WcsProjection};
    let wm = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    let (outs, sub) = wm
        .map_split_zero_based(&[2, 0, 1])
        .expect("permuted full split");
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(sub.nin(), 3);
    assert_eq!(sub.nout(), 3);
    // Verify forward action against the equivalent permuted call:
    // sub forward of [a, b, c] should equal original forward of
    // [in_axes[0]=2 -> sub-input 0=a, in_axes[1]=0 -> sub-input 1=b,
    //  in_axes[2]=1 -> sub-input 2=c] reordered into original layout.
    // I.e. parent_input = [b, c, a] (parent input k = sub-input where
    // in_axes equals k -> outperm[k] = position of k in in_axes).
    let sub_pts = PointSet::from_rows(vec![vec![10.0_f64, 0.1, 1.4]]);
    let sub_out = sub.transform(&sub_pts, true).unwrap();
    let parent_pts = PointSet::from_rows(vec![vec![0.1_f64, 1.4, 10.0]]);
    let parent_out = wm.transform(&parent_pts, true).unwrap();
    for i in 0..3 {
        assert!(
            (sub_out.point(0)[i] - parent_out.point(0)[i]).abs() < 1e-12,
            "axis {i} mismatch: sub={} parent={}",
            sub_out.point(0)[i],
            parent_out.point(0)[i]
        );
    }
}

// ========================================================================
// CmpMap (per-class override added in Task 1.6)
// ========================================================================
//
// C ancestor: reference/starlink-ast/src/cmpmap.c::MapSplit (~2788) which
// dispatches to MapSplit0 (~2405). The Rust port handles the recursive
// structure directly without the explicit MapList flattening: each child
// is split in its effective direction (folding self.inverted plus any
// inv_a/inv_b drift relative to the child's current is_inverted state).

#[test]
fn cmpmap_parallel_split_per_child() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap};
    // Parallel of two ShiftMaps: axes 0,1 from first; 2,3 from second.
    let s1 = ShiftMap::new(vec![1.0, 2.0]);
    let s2 = ShiftMap::new(vec![10.0, 20.0]);
    let cm = CmpMap::new(Box::new(s1), Box::new(s2), Composition::Parallel).unwrap();
    let (outs, _sub) = cm
        .map_split_zero_based(&[2])
        .expect("split second-child axis");
    assert_eq!(outs, vec![2]);
    let (outs2, _) = cm.map_split_zero_based(&[0, 3]).expect("split mixed");
    assert_eq!(outs2, vec![0, 3]);
}

#[test]
fn cmpmap_series_split_threads_through() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap, ZoomMap};
    // shift(3) -> zoom(3, 5): linear chain, every axis independent.
    let s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    let z = ZoomMap::new(3, 5.0).unwrap();
    let cm = CmpMap::new(Box::new(s), Box::new(z), Composition::Series).unwrap();
    let (outs, sub) = cm.map_split_zero_based(&[1]).expect("split");
    assert_eq!(outs, vec![1]);
    assert_eq!(sub.nin(), 1);
    let pts = PointSet::from_rows(vec![vec![3.0]]);
    let out = sub.transform(&pts, true).unwrap();
    // (3 + 2) * 5 = 25
    assert!((out.point(0)[0] - 25.0).abs() < 1e-12);
}

#[test]
fn cmpmap_parallel_one_child_rejects_returns_none() {
    use ast_core::mappings::{CmpMap, Composition, MatrixMap, ShiftMap};
    use ndarray::array;
    // Parallel of (fully-coupled MatrixMap, ShiftMap). Picking one input
    // from the matrix half is impossible (matrix.map_split returns None);
    // the whole CmpMap split must propagate that None.
    let coupled = MatrixMap::new(array![[1.0_f64, 1.0], [1.0, 1.0]]).unwrap();
    let s = ShiftMap::new(vec![10.0]);
    let cm = CmpMap::new(Box::new(coupled), Box::new(s), Composition::Parallel).unwrap();
    // axis 0 belongs to the coupled matrix; matrix rejects partial input.
    assert!(cm.map_split_zero_based(&[0]).is_none());
    // axis 1 also belongs to the matrix.
    assert!(cm.map_split_zero_based(&[1]).is_none());
    // axis 2 belongs to the ShiftMap and splits cleanly.
    let (outs, sub) = cm.map_split_zero_based(&[2]).expect("shift axis splits");
    assert_eq!(outs, vec![2]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
}

#[test]
fn cmpmap_nested_series_of_parallel() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap, ZoomMap};
    // Build (Shift(2) || Shift(2)) -> (Zoom(4)). The inner parallel has
    // 4 inputs / 4 outputs, then a 4-axis ZoomMap. Splitting on a single
    // axis must recurse through both layers.
    let inner1 = ShiftMap::new(vec![1.0, 2.0]);
    let inner2 = ShiftMap::new(vec![10.0, 20.0]);
    let parallel = CmpMap::new(Box::new(inner1), Box::new(inner2), Composition::Parallel).unwrap();
    let zoom = ZoomMap::new(4, 3.0).unwrap();
    let cm = CmpMap::new(Box::new(parallel), Box::new(zoom), Composition::Series).unwrap();
    // Pick original input axis 2 (= inner2's axis 0). After shift +10 then
    // zoom *3, x=4.0 -> (4+10)*3 = 42.
    let (outs, sub) = cm.map_split_zero_based(&[2]).expect("split nested");
    assert_eq!(outs, vec![2]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    let pts = PointSet::from_rows(vec![vec![4.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 42.0).abs() < 1e-12);
}

#[test]
fn cmpmap_inverted_series_split() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap, ZoomMap};
    // (Shift; Zoom) inverted: forward becomes Zoom^-1 then Shift^-1.
    // For axis i: y = (x / scale) - shift_i.
    let s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
    let z = ZoomMap::new(3, 5.0).unwrap();
    let mut cm = CmpMap::new(Box::new(s), Box::new(z), Composition::Series).unwrap();
    cm.invert();
    let (outs, sub) = cm
        .map_split_zero_based(&[1])
        .expect("split inverted series");
    assert_eq!(outs, vec![1]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    // Effective: axis 1 forward of the inverted CmpMap is x/5 - 2.
    // For x = 35: 35/5 - 2 = 5.
    let pts = PointSet::from_rows(vec![vec![35.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 5.0).abs() < 1e-12);
}

#[test]
fn cmpmap_inverted_parallel_split() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap};
    // (Shift([1,2]) || Shift([10,20])) inverted: forward subtracts the
    // shifts. After invert(), the CmpMap still has nin=4, nout=4 with the
    // same axis-to-child partition (axes 0,1 -> first; 2,3 -> second).
    let s1 = ShiftMap::new(vec![1.0, 2.0]);
    let s2 = ShiftMap::new(vec![10.0, 20.0]);
    let mut cm = CmpMap::new(Box::new(s1), Box::new(s2), Composition::Parallel).unwrap();
    cm.invert();
    // Axis 3 = second child's axis 1 = inverse shift -20.
    let (outs, sub) = cm
        .map_split_zero_based(&[3])
        .expect("split inverted parallel");
    assert_eq!(outs, vec![3]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    let pts = PointSet::from_rows(vec![vec![100.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 80.0).abs() < 1e-12);
}

#[test]
fn cmpmap_series_second_child_couples_returns_none() {
    use ast_core::mappings::{CmpMap, Composition, MatrixMap, ShiftMap};
    use ndarray::array;
    // Shift(2) -> coupled MatrixMap. Splitting on a single input asks the
    // matrix to split into one of its rows -- it can't, so the whole
    // series split returns None.
    let s = ShiftMap::new(vec![1.0, 2.0]);
    let coupled = MatrixMap::new(array![[1.0_f64, 1.0], [1.0, 1.0]]).unwrap();
    let cm = CmpMap::new(Box::new(s), Box::new(coupled), Composition::Series).unwrap();
    assert!(cm.map_split_zero_based(&[0]).is_none());
    assert!(cm.map_split_zero_based(&[1]).is_none());
    // Full inputs in identity order: each child splits, so the result is
    // a series CmpMap of clones (or a simplification thereof).
    let (outs, sub) = cm
        .map_split_zero_based(&[0, 1])
        .expect("full identity splits");
    assert_eq!(outs, vec![0, 1]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
}

#[test]
fn cmpmap_parallel_input_reorder_via_permmap() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap};
    // Parallel of two 2-axis Shifts; request the inputs in a non-ascending
    // order so the per-class impl prepends a PermMap. Verify the forward
    // action matches feeding the same values through the original CmpMap
    // at the corresponding axes.
    let s1 = ShiftMap::new(vec![1.0, 2.0]);
    let s2 = ShiftMap::new(vec![10.0, 20.0]);
    let cm = CmpMap::new(Box::new(s1), Box::new(s2), Composition::Parallel).unwrap();
    // Request axes in reversed order: [3, 1, 2, 0].
    let in_axes = [3usize, 1, 2, 0];
    let (outs, sub) = cm
        .map_split_zero_based(&in_axes)
        .expect("split with permutation");
    // Outputs come back in sorted-axis order (we partition sorted axes
    // ascending; combined_outs reflects that traversal order). For a
    // ShiftMap each axis maps to its same-indexed output; with the sort
    // [0, 1, 2, 3] we get outs = [0, 1, 2, 3].
    assert_eq!(outs, vec![0, 1, 2, 3]);
    assert_eq!(sub.nin(), 4);
    assert_eq!(sub.nout(), 4);
    // Forward check: feed [a, b, c, d] -> sub-input k corresponds to
    // original axis in_axes[k]. So sub forward of [u3, u1, u2, u0]
    // should output [u0+1, u1+2, u2+10, u3+20] (each output i in
    // sorted-axis order picks up its shift).
    let pts = PointSet::from_rows(vec![vec![100.0_f64, 50.0, 7.0, 3.0]]);
    let out = sub.transform(&pts, true).unwrap();
    // sub input -> original axis: 100 -> axis 3; 50 -> axis 1; 7 -> axis 2; 3 -> axis 0.
    // Output (in sorted-axis order [0,1,2,3]):
    //   axis 0 = 3 + 1 = 4
    //   axis 1 = 50 + 2 = 52
    //   axis 2 = 7 + 10 = 17
    //   axis 3 = 100 + 20 = 120
    let row = out.point(0);
    assert!((row[0] - 4.0).abs() < 1e-12);
    assert!((row[1] - 52.0).abs() < 1e-12);
    assert!((row[2] - 17.0).abs() < 1e-12);
    assert!((row[3] - 120.0).abs() < 1e-12);
}

#[test]
fn cmpmap_split_rejects_invalid() {
    use ast_core::mappings::{CmpMap, Composition, ShiftMap};
    let s1 = ShiftMap::new(vec![1.0, 2.0]);
    let s2 = ShiftMap::new(vec![10.0, 20.0]);
    let cm = CmpMap::new(Box::new(s1), Box::new(s2), Composition::Parallel).unwrap();
    assert!(cm.map_split_zero_based(&[4]).is_none());
    assert!(cm.map_split_zero_based(&[0, 0]).is_none());
}

// MapSplit2 (cmpmap.c ~line 2214) brute-force inverse-side fallback.

#[test]
fn cmpmap_mapsplit2_no_false_positive_when_genuinely_coupled() {
    // When the forward decomposition returns None AND no output subset
    // genuinely depends on only the requested inputs, MapSplit2 must
    // also return None (no false positives).
    //
    // Construction: ShiftMap(2) ; MatrixMap fully-coupled. Output i =
    // M[i,0]*(x[0]+a0) + M[i,1]*(x[1]+a1) — every output depends on
    // both inputs. No subset depends only on input 0.
    use ast_core::mappings::{CmpMap, Composition, MatrixMap, ShiftMap};
    use ndarray::array;
    let s = ShiftMap::new(vec![1.0, 2.0]);
    let m = MatrixMap::new(array![[1.0, 1.0], [1.0, -1.0]]).unwrap();
    let cm = CmpMap::new(Box::new(s), Box::new(m), Composition::Series).unwrap();
    // Forward path: MatrixMap.map_split_zero_based([0]) on a fully-coupled 2x2 fails.
    // MapSplit2 enumerates subsets {[0], [1]} of outputs; neither
    // depends only on input 0, so all recursive splits return None.
    assert!(cm.map_split_zero_based(&[0]).is_none());
    assert!(cm.map_split_zero_based(&[1]).is_none());
    // Full inputs work via the default full_set_split helper.
    assert!(cm.map_split_zero_based(&[0, 1]).is_some());
}

#[test]
fn cmpmap_mapsplit2_terminates_on_small_nout() {
    // MapSplit2 enumerates subset sizes mout in 1..nout. For nout < 2
    // the loop is empty and the function must return None promptly
    // without panicking.
    use ast_core::mappings::{CmpMap, Composition, MatrixMap};
    use ndarray::array;
    // 2-in 1-out fully-coupled MatrixMap. nout = 1 → MapSplit2 has
    // nothing to enumerate.
    let m1 = MatrixMap::new(array![[1.0, 1.0]]).unwrap();
    let m2 = MatrixMap::new(array![[2.0]]).unwrap();
    let cm = CmpMap::new(Box::new(m1), Box::new(m2), Composition::Series).unwrap();
    // Forward fails (m1 is coupled); MapSplit2 hits early-exit on nout < 2.
    assert!(cm.map_split_zero_based(&[0]).is_none());
    assert!(cm.map_split_zero_based(&[1]).is_none());
}

// PolyMap inherits the default impl (no per-class override per polymap.c).

#[test]
fn polymap_split_full_inputs_only() {
    use ast_core::mappings::{PolyMap, PolyTerm};
    // Identity-ish 2-in 2-out PolyMap: y0 = x0, y1 = x1.
    let fwd = vec![
        PolyTerm {
            coeff: 1.0,
            out_axis: 1,
            powers: vec![1, 0],
        },
        PolyTerm {
            coeff: 1.0,
            out_axis: 2,
            powers: vec![0, 1],
        },
    ];
    let pm = PolyMap::new(2, 2, &fwd, None).unwrap();
    // Default impl handles full-input case via PermMap+self.
    assert!(pm.map_split_zero_based(&[0, 1]).is_some());
    // Partial inputs return None (no per-class override).
    assert!(pm.map_split_zero_based(&[0]).is_none());
    assert!(pm.map_split_zero_based(&[1]).is_none());
}

// NormMap: mirrors normmap.c::MapSplit ~line 736.

#[test]
fn normmap_split_picks_axes() {
    use ast_core::frames::BasicFrameBuilder;
    use ast_core::mappings::{Mapping, NormMap};
    // 3-axis BasicFrame; pick axes [0, 2].
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let nm = NormMap::new(Box::new(frame));
    let (outs, sub) = nm.map_split_zero_based(&[0, 2]).expect("split");
    assert_eq!(outs, vec![0, 2]);
    assert_eq!(sub.nin(), 2);
    assert_eq!(sub.nout(), 2);
}

#[test]
fn normmap_split_full_inputs_via_default() {
    use ast_core::frames::BasicFrameBuilder;
    use ast_core::mappings::{Mapping, NormMap};
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let nm = NormMap::new(Box::new(frame));
    // Full-input case goes through full_set_split.
    let (outs, sub) = nm.map_split_zero_based(&[0, 1, 2]).expect("split");
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(sub.nin(), 3);
}

#[test]
fn normmap_split_rejects_invalid() {
    use ast_core::frames::BasicFrameBuilder;
    use ast_core::mappings::{Mapping, NormMap};
    let frame = BasicFrameBuilder::new(3).build().unwrap();
    let nm = NormMap::new(Box::new(frame));
    assert!(nm.map_split_zero_based(&[3]).is_none()); // out of range
    assert!(nm.map_split_zero_based(&[0, 0]).is_none()); // duplicate
}

// TranMap: mirrors tranmap.c::MapSplit ~line 1017.

#[test]
fn tranmap_split_picks_per_axis() {
    use ast_core::mappings::{Mapping, ShiftMap, TranMap};
    // TranMap of two ShiftMaps: forward = ShiftMap(+a), inverse =
    // ShiftMap(-a). Splitting on [1] should pick the shift on axis 1
    // for both directions.
    let fwd = ShiftMap::new(vec![10.0, 20.0, 30.0]);
    let mut inv = ShiftMap::new(vec![-10.0, -20.0, -30.0]);
    inv.invert();
    let tm = TranMap::new(Box::new(fwd), Box::new(inv)).unwrap();
    let (outs, sub) = tm.map_split_zero_based(&[1]).expect("split");
    assert_eq!(outs, vec![1]);
    assert_eq!(sub.nin(), 1);
    // Forward of the sub-mapping: 5 + 20 = 25 (uses shifts[1] = 20).
    let pts = ast_core::coords::PointSet::from_rows([[5.0]]);
    let out = sub.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 25.0).abs() < 1e-12);
}

// RateMap: mirrors ratemap.c::MapSplit ~line 819.

#[test]
fn ratemap_split_diff_output_dropped_returns_none() {
    use ast_core::mappings::{Mapping, RateMap, ShiftMap};
    // RateMap differentiates output 2 (1-based) wrt input 1. Splitting
    // on input 0 (= 1-based axis 1, the diff input): the encapsulated
    // ShiftMap.map_split_zero_based([0]) returns Some([0], one-axis), which lacks
    // output 1 (= 1-based axis 2, the differentiated output). So the
    // RateMap split must return None.
    let s = ShiftMap::new(vec![10.0, 20.0, 30.0]);
    let rm = RateMap::new(Box::new(s), 2, 1).unwrap();
    assert_eq!(rm.nin(), 3);
    assert_eq!(rm.nout(), 1);
    assert!(rm.map_split_zero_based(&[0]).is_none());
}

#[test]
fn ratemap_split_rejects_when_inverted() {
    use ast_core::mappings::{Mapping, RateMap, ShiftMap};
    // Per ratemap.c:906, the per-class fallback only succeeds when the
    // RateMap is not inverted. Use a 3-axis encapsulated mapping so the
    // inverted RateMap has nin = 1, nout = 3 — a partial-input split on
    // input axis 0 alone reaches the per-class fallback (it's the full
    // input set, but full_set_split's identity case kicks in only when
    // in_axes covers the entire input range, which it does here, so we
    // pick a 2-axis encapsulated map and a partial split target instead).
    //
    // Construction: 2-axis ShiftMap, RateMap differentiating output 1 wrt
    // input 1. Original nin = 2, nout = 1. Invert: nin = 1, nout = 2.
    // Now ask for input [0]: that's the full input set (nin = 1 = len),
    // full_set_split succeeds via identity. So the only way to test the
    // inverted-bail in isolation is when nin > 1 after invert, i.e. a
    // mapping with nout > 1 originally — which RateMap can't produce
    // (it always has nout = 1 in forward orientation). The C bail is
    // therefore unreachable from the Rust API in practice; we still
    // mirror it for correctness should the encapsulated mapping shape
    // change in the future.
    //
    // What we CAN test: a non-inverted partial split that succeeds via
    // the per-class fallback path.
    let s = ShiftMap::new(vec![10.0, 20.0, 30.0]);
    // RateMap differentiating output 1 wrt input 1 (1-based).
    let rm = RateMap::new(Box::new(s), 1, 1).unwrap();
    assert_eq!(rm.nin(), 3);
    assert_eq!(rm.nout(), 1);
    // Splitting on input 0 (= 1-based axis 1, the diff variable) should
    // succeed: ShiftMap.map_split_zero_based([0]) = Some([0], one-axis ShiftMap),
    // which contains output 0 (= 1-based axis 1, the differentiated
    // output). New RateMap is built.
    let (outs, sub) = rm
        .map_split_zero_based(&[0])
        .expect("split via per-class fallback");
    assert_eq!(outs, vec![0]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);

    // Splitting on input 1 alone (= 1-based axis 2, NOT the diff variable)
    // should return None — the diff input isn't in the kept set.
    assert!(rm.map_split_zero_based(&[1]).is_none());
}

// ========================================================================
// Public 1-based wrapper: `Mapping::map_split`
// ========================================================================
//
// C ancestor: the public astMapSplit interface (astMapSplitId_,
// mapping.c:25313), which decrements the supplied input indices and
// increments the returned output indices by one around the 0-based
// protected MapSplit. The design spec mandates 1-based axis indices on the
// public Rust API (matching `Mapping::rate`), so `map_split` is the 1-based
// face of the same algorithm exercised 0-based above.

#[test]
fn public_map_split_is_one_based_in_and_out() {
    // ZoomMap(4, 3): pick 1-based input axes [2, 4]. The 0-based call would
    // use [1, 3]; the public 1-based call must return 1-based outputs [2, 4].
    let z = ZoomMap::new(4, 3.0).unwrap();
    let (outs_1b, sub_1b) = z.map_split(&[2, 4]).expect("1-based split");
    assert_eq!(outs_1b, vec![2, 4]);
    assert_eq!(sub_1b.nin(), 2);
    assert_eq!(sub_1b.nout(), 2);
    assert_eq!(fwd_point(sub_1b.as_ref(), &[2.0, 5.0]), vec![6.0, 15.0]);

    // The 0-based protected call with the decremented indices is equivalent
    // apart from the ±1 offset on the returned output vector.
    let (outs_0b, _) = z.map_split_zero_based(&[1, 3]).expect("0-based split");
    assert_eq!(outs_0b, vec![1, 3]);
    assert_eq!(outs_1b, outs_0b.iter().map(|o| o + 1).collect::<Vec<_>>());
}

#[test]
fn public_map_split_rejects_zero_axis() {
    // A 0 entry is not a valid 1-based index; the wrapper rejects it rather
    // than underflowing to usize::MAX.
    let u = UnitMap::new(3);
    assert!(u.map_split(&[0]).is_none());
    assert!(u.map_split(&[1, 0]).is_none());
}

#[test]
fn public_map_split_rejects_out_of_range() {
    // 1-based: valid indices are 1..=nin. nin+1 is out of range.
    let u = UnitMap::new(3);
    assert!(u.map_split(&[4]).is_none());
    let (outs, sub) = u.map_split(&[1, 2, 3]).expect("full 1-based split");
    assert_eq!(outs, vec![1, 2, 3]);
    assert_eq!(sub.nin(), 3);
}

#[test]
fn public_map_split_permmap_constant_output_dropped() {
    // Same case as perm_split_constant_outperm_silently_dropped, but through
    // the 1-based public API. outperm=[1,-1]: 1-based output 1 takes input 1;
    // output 2 is constant (dropped). Pick 1-based input [1] -> outs == [1].
    let pm = PermMap::new(&[1], &[1, -1], &[42.0]).unwrap();
    let (outs, sub) = pm.map_split(&[1]).expect("constant output dropped");
    assert_eq!(outs, vec![1]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
}

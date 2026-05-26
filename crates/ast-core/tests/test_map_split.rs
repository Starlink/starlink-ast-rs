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
use ast_core::mappings::{Mapping, ShiftMap, UnitMap, WinMap, ZoomMap};

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
    let (outs, sub) = w.map_split(&[0, 1, 2]).expect("split");
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
    let (outs, sub) = w.map_split(&[2, 0, 1]).expect("split");
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
    assert!(w.map_split(&[0, 1, 1]).is_none());
    assert!(w.map_split(&[0, 1, 3]).is_none());
}

#[test]
fn winmap_split_rejects_empty() {
    // WinMap requires at least one axis, so `in_axes=[]` yields None
    // through WinMap::new returning `Badwm`.
    let w = winmap_3axis();
    assert!(w.map_split(&[]).is_none());
}

#[test]
fn winmap_split_picks_per_axis_coeffs() {
    // 3-axis WinMap: y_i = shifts[i] + scales[i] * x_i
    let w = WinMap::new(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]).unwrap();
    let (outs, sub) = w.map_split(&[2, 0]).expect("split");
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
    let (outs, sub) = w.map_split(&[2, 0]).expect("split");
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
    let (outs, sub) = mm.map_split(&[1]).expect("split");
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
    let (outs, sub) = mm.map_split(&[2]).expect("split");
    assert_eq!(outs, vec![2]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 1);
    // sub forward: y = 5 * x.
    let out = fwd_point(sub.as_ref(), &[7.0]);
    assert!((out[0] - 35.0).abs() < 1e-12);
    // Picking axes [0, 1] should also succeed (block 1).
    let (outs2, sub2) = mm.map_split(&[0, 1]).expect("split block 1");
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
    assert!(mm.map_split(&[0]).is_none());
    assert!(mm.map_split(&[1]).is_none());
    // Picking all inputs is fine (default fallback handles full case).
    let (outs, sub) = mm.map_split(&[0, 1]).expect("full split");
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
    let (outs, sub) = mm.map_split(&[0]).expect("split");
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
    assert!(mm.map_split(&[0]).is_none());
    // Picking input 1 yields all three rows.
    let (outs, sub) = mm.map_split(&[1]).expect("split");
    assert_eq!(outs, vec![0, 1, 2]);
    assert_eq!(sub.nin(), 1);
    assert_eq!(sub.nout(), 3);
}

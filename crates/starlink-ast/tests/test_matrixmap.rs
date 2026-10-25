// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Tests for MatrixMap.
//!
//! Ancestor: (no direct C test — MatrixMap tests derived from the C AST
//! MatrixMap documentation and reference behavior.)

use crate::common::native_dump;
use ndarray::array;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::error::ErrorCode;
use starlink_ast::mappings::{Mapping, MatrixMap};

/// 2×3 matrix — nin = 3, nout = 2.
#[test]
fn matrix_map_non_square_dimensions() {
    let m = array![[1.0_f64, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let mm = MatrixMap::new(m).unwrap();
    assert_eq!(mm.nin(), 3);
    assert_eq!(mm.nout(), 2);
}

/// 2×2 identity matrix should pass points through unchanged.
#[test]
fn matrix_map_identity_passthrough() {
    let m = array![[1.0_f64, 0.0], [0.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[3.0, 7.0], [-1.0, 4.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 7.0]);
    assert_eq!(out.point(1), &[-1.0, 4.0]);
}

/// 90-degree rotation: [[0, -1], [1, 0]] maps (1,0) → (0,1).
#[test]
fn matrix_map_rotation_90() {
    let m = array![[0.0_f64, -1.0], [1.0, 0.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[1.0, 0.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 0.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 1.0).abs() < 1e-12);
}

/// Scale by 2, then inverse should divide by 2.
#[test]
fn matrix_map_inverse_scale() {
    let m = array![[2.0_f64, 0.0], [0.0, 2.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[4.0, 6.0]]);
    let out = mm.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 3.0).abs() < 1e-12);
}

/// Non-square matrix — inverse is not available.
#[test]
fn matrix_map_non_square_inverse_error() {
    let m = array![[1.0_f64, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    assert!(mm.transform(&pts, false).is_err());
}

/// BAD_VALUE in an input coord with non-zero coefficient → that output is BAD.
/// But zero-coefficient columns mask BAD (per-axis handling).
#[test]
fn matrix_map_bad_value_propagation() {
    let m = array![[1.0_f64, 0.0], [0.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 3.0]]);
    let out = mm.transform(&pts, true).unwrap();
    // Identity: row 0 depends on input 0 (BAD) → BAD
    assert_eq!(out.point(0)[0], BAD_VALUE);
    // Identity: row 1 depends on input 1 (3.0) → 3.0
    assert_eq!(out.point(0)[1], 3.0);
}

/// Per-axis BAD handling: zero coefficients mask BAD inputs.
#[test]
fn matrix_bad_per_axis_zero_coeff() {
    // 2x3 matrix where row 0 has a zero coefficient on input 1.
    // Row 0: [1, 0, 2]  => output[0] = 1*x + 0*y + 2*z (y is masked)
    // Row 1: [0, 1, 1]  => output[1] = 0*x + 1*y + 1*z
    let m = array![[1.0_f64, 0.0, 2.0], [0.0, 1.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();

    // Input with y=BAD: output[0] should still compute (zero coeff masks BAD),
    // output[1] should be BAD (non-zero coeff on y).
    let pts = PointSet::from_rows([[3.0, BAD_VALUE, 5.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 1.0 * 3.0 + 0.0 + 2.0 * 5.0); // 13.0
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

/// All inputs BAD → all outputs BAD.
#[test]
fn matrix_bad_all_inputs_bad() {
    let m = array![[1.0_f64, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, BAD_VALUE]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}

/// simplified() on an identity matrix should return a working UnitMap.
#[test]
fn matrix_map_simplified_identity() {
    let m = array![[1.0_f64, 0.0], [0.0, 1.0]];
    let mm = MatrixMap::new(m).unwrap();
    let simplified = <dyn Mapping>::simplify(&mm).unwrap();
    // UnitMap pass-through
    let pts = PointSet::from_rows([[5.0, 9.0]]);
    let out = simplified.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[5.0, 9.0]);
}

/// simplified() on a non-identity matrix returns the original mapping.
#[test]
fn matrix_map_simplified_non_identity_preserved() {
    let m = array![[2.0_f64, 0.0], [0.0, 3.0]];
    let mm = MatrixMap::new(m).unwrap();
    let simplified = <dyn Mapping>::simplify(&mm).unwrap();
    let pts = PointSet::from_rows([[1.0, 1.0]]);
    let out = simplified.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 3.0).abs() < 1e-12);
}

/// Analytical rate for MatrixMap: rate(ax1, ax2) = matrix[ax1-1, ax2-1].
#[test]
fn matrix_rate_analytical() {
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    // rate(ax1=output, ax2=input) = matrix[ax1-1, ax2-1]
    assert_eq!(mm.rate(&[0.0, 0.0], 1, 1).unwrap(), 1.0);
    assert_eq!(mm.rate(&[0.0, 0.0], 1, 2).unwrap(), 2.0);
    assert_eq!(mm.rate(&[0.0, 0.0], 2, 1).unwrap(), 3.0);
    assert_eq!(mm.rate(&[0.0, 0.0], 2, 2).unwrap(), 4.0);
}

/// Analytical rate for inverted MatrixMap uses the inverse matrix.
#[test]
fn matrix_rate_analytical_inverted() {
    // [[2, 0], [0, 4]] => inverse is [[0.5, 0], [0, 0.25]]
    let m = array![[2.0, 0.0], [0.0, 4.0]];
    let mut mm = MatrixMap::new(m).unwrap();
    mm.invert();
    assert!((mm.rate(&[0.0, 0.0], 1, 1).unwrap() - 0.5).abs() < 1e-15);
    assert_eq!(mm.rate(&[0.0, 0.0], 1, 2).unwrap(), 0.0);
    assert_eq!(mm.rate(&[0.0, 0.0], 2, 1).unwrap(), 0.0);
    assert!((mm.rate(&[0.0, 0.0], 2, 2).unwrap() - 0.25).abs() < 1e-15);
}

/// MatrixMap followed by PermMap merges into a single MatrixMap.
/// PermMap permutes the outputs of MatrixMap.
#[test]
fn matrix_merge_with_permmap_after() {
    use starlink_ast::mappings::PermMap;

    // Matrix [[1,2],[3,4]] followed by PermMap that swaps outputs [2,1].
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();

    // MatrixMap then PermMap in series: self=mm first, other=pm second.
    let merged = mm.try_merge(&pm, true, false, false);
    assert!(merged.is_some(), "MatrixMap + PermMap should merge");

    // Verify the merged mapping produces the same result as sequential application.
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let mat_out = mm.transform(&pts, true).unwrap();
    let expected = pm.transform(&mat_out, true).unwrap();
    let actual = merged.unwrap().transform(&pts, true).unwrap();
    assert_eq!(actual.point(0)[0], expected.point(0)[0]);
    assert_eq!(actual.point(0)[1], expected.point(0)[1]);

    // Verify numeric values:
    // mm([1,2]) = [1*1+2*2, 3*1+4*2] = [5, 11]
    // pm([5,11]) = [11, 5] (swap)
    assert_eq!(actual.point(0)[0], 11.0);
    assert_eq!(actual.point(0)[1], 5.0);
}

/// PermMap followed by MatrixMap merges into a single MatrixMap.
/// PermMap permutes the inputs before MatrixMap applies.
#[test]
fn matrix_merge_with_permmap_before() {
    use starlink_ast::mappings::{CmpMap, Composition, PermMap};

    // PermMap [2,1] followed by MatrixMap [[1,2],[3,4]].
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();

    // Test via CmpMap simplification (the merge loop tries both directions).
    let cmp = CmpMap::new(
        Box::new(pm.clone()),
        Box::new(mm.clone()),
        Composition::Series,
    )
    .unwrap();
    let simplified = <dyn Mapping>::simplify(&cmp).unwrap();

    // Should simplify to a single MatrixMap.
    assert!(
        simplified.as_any().is::<MatrixMap>(),
        "PermMap + MatrixMap should simplify to MatrixMap"
    );

    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = simplified.transform(&pts, true).unwrap();

    // PermMap swaps inputs: [1,2] → [2,1], then Matrix [[1,2],[3,4]] * [2,1]:
    // [1*2+2*1, 3*2+4*1] = [4, 10]
    assert_eq!(out.point(0)[0], 4.0);
    assert_eq!(out.point(0)[1], 10.0);
}

/// PermMap followed by MatrixMap can also be tested directly via try_merge
/// on PermMap (pm.try_merge(mm)).
#[test]
fn permmap_try_merge_with_matrixmap() {
    use starlink_ast::mappings::PermMap;

    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();

    // PermMap.try_merge(MatrixMap): self=pm first, other=mm second.
    let merged = pm.try_merge(&mm, true, false, false);
    assert!(merged.is_some(), "PermMap + MatrixMap should merge");

    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = merged.unwrap().transform(&pts, true).unwrap();

    // pm swaps inputs: [1,2] → [2,1], then mm: [1*2+2*1, 3*2+4*1] = [4, 10]
    assert_eq!(out.point(0)[0], 4.0);
    assert_eq!(out.point(0)[1], 10.0);
}

/// MatrixMap + inverted PermMap should merge correctly.
#[test]
fn matrix_merge_with_inverted_permmap() {
    use starlink_ast::mappings::PermMap;

    // Matrix [[1,2],[3,4]] followed by inverted PermMap.
    // PermMap with outperm=[2,1], inperm=[2,1]: inverted uses inperm for forward.
    // inperm=[2,1] swaps the same way.
    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();

    // Treat PermMap as inverted: uses inperm for forward direction.
    let merged = mm.try_merge(&pm, true, false, true);
    assert!(
        merged.is_some(),
        "MatrixMap + inverted PermMap should merge"
    );

    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = merged.unwrap().transform(&pts, true).unwrap();

    // mm([1,2]) = [5, 11], then inverted pm uses inperm=[2,1] to swap: [11, 5]
    assert_eq!(out.point(0)[0], 11.0);
    assert_eq!(out.point(0)[1], 5.0);
}

/// Non-pure permutation (constants or BAD) should not merge.
#[test]
fn matrix_no_merge_with_constant_permmap() {
    use starlink_ast::mappings::PermMap;

    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    // PermMap with a constant reference (negative entry): outperm=[-1, 1]
    let pm = PermMap::new(&[2, 1], &[-1, 1], &[99.0]).unwrap();

    let merged = mm.try_merge(&pm, true, false, false);
    assert!(
        merged.is_none(),
        "PermMap with constants should not merge into MatrixMap"
    );
}

/// Diagonal MatrixMap followed by WinMap merges into a single WinMap.
#[test]
fn diagonal_matrix_merge_with_winmap() {
    use starlink_ast::mappings::WinMap;

    // Diagonal matrix [[2,0],[0,3]] followed by WinMap shift=[1,1] scale=[1,1]
    // Combined: out = 1 + 1*(diag*x) = 1 + [2x, 3y]
    let m = array![[2.0, 0.0], [0.0, 3.0]];
    let mm = MatrixMap::new(m).unwrap();
    let wm = WinMap::new(&[1.0, 1.0], &[1.0, 1.0]).unwrap();

    let merged = mm.try_merge(&wm, true, false, false);
    assert!(merged.is_some(), "diagonal MatrixMap + WinMap should merge");

    // Verify: MatrixMap then WinMap
    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let mat_out = mm.transform(&pts, true).unwrap(); // [10, 21]
    let expected = wm.transform(&mat_out, true).unwrap(); // [11, 22]
    let actual = merged.unwrap().transform(&pts, true).unwrap();
    assert!((actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10);
    assert!((actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10);
}

/// Diagonal matrix uses the O(N) fast path — verify correctness.
#[test]
fn matrix_diagonal_fast_path() {
    // Diagonal 3x3 matrix
    let m = array![[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();

    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 2.0);
    assert_eq!(out.point(0)[1], 6.0);
    assert_eq!(out.point(0)[2], 12.0);

    // Inverse of diagonal [[2,0,0],[0,3,0],[0,0,4]] is [[0.5,0,0],[0,1/3,0],[0,0,0.25]]
    let inv_out = mm.transform(&pts, false).unwrap();
    assert!((inv_out.point(0)[0] - 0.5).abs() < 1e-12);
    assert!((inv_out.point(0)[1] - 2.0 / 3.0).abs() < 1e-12);
    assert!((inv_out.point(0)[2] - 0.75).abs() < 1e-12);
}

/// Large diagonal matrix to verify the O(N) path scales correctly.
#[test]
fn matrix_diagonal_fast_path_large() {
    let n = 100;
    let mut big = ndarray::Array2::<f64>::zeros((n, n));
    for i in 0..n {
        big[[i, i]] = (i + 1) as f64;
    }
    let big_mm = MatrixMap::new(big).unwrap();
    let big_pts = PointSet::from_rows([vec![1.0; n].as_slice()]);
    let big_out = big_mm.transform(&big_pts, true).unwrap();
    for i in 0..n {
        assert_eq!(big_out.point(0)[i], (i + 1) as f64);
    }
}

/// Diagonal matrix BAD propagation: a BAD input gives BAD output whatever the
/// diagonal element, including a zero one.
#[test]
fn matrix_diagonal_bad_handling() {
    // Diagonal [[0, 0], [0, 5]] — first diag is 0, second is 5. The
    // off-diagonal zeros mean this compresses to the diagonal storage form
    // (matrixmap.c CompressMatrix, 686-800), so the diagonal branch of
    // Transform applies: a zero diagonal is multiplied through normally
    // rather than masking a bad input. Only a *BAD* diagonal short-circuits.
    let m = array![[0.0, 0.0], [0.0, 5.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 3.0]]);
    let out = mm.transform(&pts, true).unwrap();
    // Row 0: diag=0 is good but the input is BAD → BAD.
    assert_eq!(out.point(0)[0], BAD_VALUE);
    // Row 1: diag=5, input=3.0 → 15.0
    assert_eq!(out.point(0)[1], 15.0);

    // Now BAD in the second input where diag is non-zero
    let pts2 = PointSet::from_rows([[2.0, BAD_VALUE]]);
    let out2 = mm.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0)[0], 0.0); // diag=0, input=2.0 → 0.0
    assert_eq!(out2.point(0)[1], BAD_VALUE); // diag=5, input=BAD → BAD
}

/// Full matrix BAD handling: a BAD matrix element makes its output row BAD
/// even for good input, while a zero element masks a BAD input on that row.
#[test]
fn matrix_full_bad_coefficient_and_masking() {
    // Element [0][1] is BAD; row 1 is entirely good.
    let m = array![[2.0, BAD_VALUE], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[1.0_f64, 1.0]]);
    let out = mm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 7.0);

    // A zero coefficient keeps a BAD input off the rows independent of it.
    // Off-diagonal [1][0] is non-zero, so the matrix stays in full form.
    let m2 = array![[1.0, 0.0], [5.0, 1.0]];
    let mm2 = MatrixMap::new(m2).unwrap();
    let pts2 = PointSet::from_rows([[3.0_f64, BAD_VALUE]]);
    let out2 = mm2.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0)[0], 3.0); // BAD input has coefficient 0 → masked
    assert_eq!(out2.point(0)[1], BAD_VALUE); // coefficient 1 → propagates
}

/// Non-diagonal MatrixMap should NOT merge with WinMap.
#[test]
fn non_diagonal_matrix_no_merge_with_winmap() {
    use starlink_ast::mappings::WinMap;

    let m = array![[1.0, 2.0], [3.0, 4.0]];
    let mm = MatrixMap::new(m).unwrap();
    let wm = WinMap::new(&[1.0, 1.0], &[1.0, 1.0]).unwrap();

    let merged = mm.try_merge(&wm, true, false, false);
    assert!(
        merged.is_none(),
        "non-diagonal MatrixMap should not merge with WinMap"
    );
}

/// Diagonal MatrixMap + WinMap with non-trivial scales.
#[test]
fn diagonal_matrix_merge_with_winmap_scales() {
    use starlink_ast::mappings::WinMap;

    // Diagonal [[3,0],[0,2]], WinMap shifts=[10, 20], scales=[0.5, 4.0]
    // Combined: out = [10, 20] + [0.5, 4.0] * ([3, 2] * x)
    //         = [10 + 1.5*x, 20 + 8*y]
    let m = array![[3.0, 0.0], [0.0, 2.0]];
    let mm = MatrixMap::new(m).unwrap();
    let wm = WinMap::new(&[10.0, 20.0], &[0.5, 4.0]).unwrap();

    let merged = mm.try_merge(&wm, true, false, false);
    assert!(merged.is_some());

    let pts = PointSet::from_rows([[2.0, 3.0]]);
    let mat_out = mm.transform(&pts, true).unwrap(); // [6, 6]
    let expected = wm.transform(&mat_out, true).unwrap(); // [10+3, 20+24] = [13, 44]
    let actual = merged.unwrap().transform(&pts, true).unwrap();
    assert!((actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10);
    assert!((actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10);
}

#[test]
fn simplified_switches_full_to_diagonal_when_offdiagonals_are_zero() {
    use starlink_ast::{AnyObject, from_native_string};

    // Same fixture as crates/starlink-ast/tests/fixtures/simplify/
    // matrix_full_to_diagonal.map (Form="Full" with off-diagonals == 0).
    let src = r#" Begin MatrixMap
    Nin = 2
 IsA Mapping
    M0 = 2
    M1 = 0
    M2 = 0
    M3 = 3
    IM0 = 0.5
    IM1 = 0
    IM2 = 0
    IM3 = 0.33333333333333331
    Form = "Full"
 End MatrixMap
"#;
    let obj = from_native_string(src).unwrap();
    let map = obj.into_mapping().unwrap();
    let simp = map.simplify().unwrap();
    let mm = simp
        .as_any()
        .downcast_ref::<MatrixMap>()
        .expect("still a MatrixMap");

    let s = native_dump(&AnyObject::MatrixMap(mm.clone())).unwrap();
    assert!(
        s.contains("Form = \"Diagonal\""),
        "expected Diagonal form after simplify, got:\n{s}"
    );
    // Diagonal form emits only M0..Mk-1 — no M1/M2 (off-diagonal entries).
    assert!(!s.contains("M2 ="), "diagonal form should not emit M2");
}

// ===========================================================================
// astMtrRot / astMtrMult (matrixmap.c:3766 / :3526).
//
// These C entry points are protected (not in the public ast.h), so the values
// below are verified analytically against the documented rotation/product
// formulae rather than via an oracle binary. The forward 2-D rotation matrix
// is [[cos, sin], [-sin, cos]] (clockwise); the 3-D rotation is the standard
// axis-angle (Rodrigues) form C builds at matrixmap.c:3921-3929. astMtrMult
// returns a x this.
// ===========================================================================

fn fwd(m: &MatrixMap, input: &[f64]) -> Vec<f64> {
    let pts = PointSet::from_rows(vec![input.to_vec()]);
    m.transform(&pts, true).unwrap().point(0).to_vec()
}

#[test]
fn mtr_rot_2d_clockwise() {
    // Rotate the 2x2 identity by theta = 0.3 rad. Forward matrix is
    // [[cos, sin], [-sin, cos]]; applied to (1,0) -> (cos, -sin) and
    // (0,1) -> (sin, cos).
    let id = array![[1.0_f64, 0.0], [0.0, 1.0]];
    let m = MatrixMap::new(id).unwrap();
    let r = m.mtr_rot(0.3, &[]).unwrap();
    let (c, s) = (0.3_f64.cos(), 0.3_f64.sin());
    let o0 = fwd(&r, &[1.0, 0.0]);
    let o1 = fwd(&r, &[0.0, 1.0]);
    assert!((o0[0] - c).abs() < 1e-12 && (o0[1] - (-s)).abs() < 1e-12);
    assert!((o1[0] - s).abs() < 1e-12 && (o1[1] - c).abs() < 1e-12);
}

#[test]
fn mtr_rot_full_turn_is_unchanged() {
    // cos(theta) == 1 for a multiple of 2pi -> C returns an unchanged copy.
    let m = MatrixMap::new(array![[2.0_f64, 1.0], [0.0, 3.0]]).unwrap();
    let r = m.mtr_rot(0.0, &[]).unwrap();
    assert_eq!(fwd(&r, &[1.0, 0.0]), fwd(&m, &[1.0, 0.0]));
    assert_eq!(fwd(&r, &[0.0, 1.0]), fwd(&m, &[0.0, 1.0]));
}

#[test]
fn mtr_rot_3d_about_z_leaves_z_fixed() {
    // Rotating the 3x3 identity about the z axis by theta leaves z fixed.
    // C's 3-D Rodrigues form (matrixmap.c:3921-3929) for axis (0,0,1) is
    // rotmat = [[cos,-sin,0],[sin,cos,0],[0,0,1]]; the matrix-vector multiply
    // sends the x-basis (1,0,0) to the first *column* (cos, sin, 0). Note
    // this is the opposite off-diagonal sign from the 2-D convention — the
    // two are deliberately different in C and faithfully reproduced here.
    let id = array![[1.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let m = MatrixMap::new(id).unwrap();
    let r = m.mtr_rot(0.5, &[0.0, 0.0, 1.0]).unwrap();
    let (c, s) = (0.5_f64.cos(), 0.5_f64.sin());
    let ox = fwd(&r, &[1.0, 0.0, 0.0]);
    assert!((ox[0] - c).abs() < 1e-12);
    assert!((ox[1] - s).abs() < 1e-12);
    assert!(ox[2].abs() < 1e-12);
    let oz = fwd(&r, &[0.0, 0.0, 1.0]);
    assert!((oz[0]).abs() < 1e-12 && oz[1].abs() < 1e-12 && (oz[2] - 1.0).abs() < 1e-12);
}

#[test]
fn mtr_rot_3d_null_axis_errors() {
    let m = MatrixMap::new(array![
        [1.0_f64, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0]
    ])
    .unwrap();
    assert!(m.mtr_rot(0.5, &[0.0, 0.0, 0.0]).is_err());
    assert!(m.mtr_rot(0.5, &[]).is_err());
}

#[test]
fn mtr_rot_wrong_dimension_errors() {
    // 1x1 and 4x4 are neither 2 nor 3 rows.
    let m1 = MatrixMap::new(array![[2.0_f64]]).unwrap();
    assert!(m1.mtr_rot(0.5, &[]).is_err());
}

#[test]
fn mtr_mult_returns_a_times_this() {
    // this = [[1,1],[0,1]], a = diag(2,3). Product a x this =
    // [[2,2],[0,3]]: applied to (1,0) -> (2,0), (0,1) -> (2,3).
    let this = MatrixMap::new(array![[1.0_f64, 1.0], [0.0, 1.0]]).unwrap();
    let a = MatrixMap::new(array![[2.0_f64, 0.0], [0.0, 3.0]]).unwrap();
    let prod = this.mtr_mult(&a).unwrap();
    assert_eq!(fwd(&prod, &[1.0, 0.0]), vec![2.0, 0.0]);
    assert_eq!(fwd(&prod, &[0.0, 1.0]), vec![2.0, 3.0]);
}

#[test]
fn mtr_mult_shape_mismatch_errors() {
    // this is 2x3 (nout=2), a is 2x2 (ncol=2) -> ncol_a(2) != nrow_this(2)?
    // Build a genuine mismatch: this 3x2 (nout=3), a 2x2 (ncol=2);
    // ncol_a=2 != nrow_this=3 -> error.
    let this = MatrixMap::new(array![[1.0_f64, 0.0], [0.0, 1.0], [1.0, 1.0]]).unwrap();
    let a = MatrixMap::new(array![[2.0_f64, 0.0], [0.0, 3.0]]).unwrap();
    assert!(this.mtr_mult(&a).is_err());
}

/// A MatrixMap without a forward transformation, such as an inverted
/// singular one, cannot be multiplied, whichever factor it is.
#[test]
fn mtr_mult_without_a_forward_transformation_errors() {
    let mut singular = MatrixMap::new(array![[1.0_f64, 1.0], [1.0, 1.0]]).unwrap();
    singular.invert();
    let other = MatrixMap::new(array![[2.0_f64, 0.0], [0.0, 3.0]]).unwrap();
    for result in [singular.mtr_mult(&other), other.mtr_mult(&singular)] {
        assert_eq!(result.unwrap_err().code(), ErrorCode::Mtrml);
    }
}

#[test]
fn mtr_mult_non_square_product() {
    // this = 3x2 (maps 2 in -> 3 out), a = 2x3 (maps 3 in -> 2 out).
    // Product a x this is 2x2. ncol_a = 3 == nrow_this = 3. OK.
    let this = MatrixMap::new(array![[1.0_f64, 0.0], [0.0, 1.0], [1.0, 1.0]]).unwrap();
    let a = MatrixMap::new(array![[1.0_f64, 0.0, 1.0], [0.0, 1.0, 0.0]]).unwrap();
    let prod = this.mtr_mult(&a).unwrap();
    // a x this: row0 = [1,0,1]·cols, row1 = [0,1,0]·cols.
    // this cols: c0=(1,0,1)^T, c1=(0,1,1)^T.
    // prod[0][0]=1*1+0*0+1*1=2; prod[0][1]=1*0+0*1+1*1=1;
    // prod[1][0]=0; prod[1][1]=1.
    assert_eq!(fwd(&prod, &[1.0, 0.0]), vec![2.0, 0.0]);
    assert_eq!(fwd(&prod, &[0.0, 1.0]), vec![1.0, 1.0]);
}

/// An inverted non-square MatrixMap has no matrix in the direction it
/// applies, so C compares the stored forward matrices instead (matrixmap.c
/// Equal) and finds it equal to its own copy.
#[test]
fn an_inverted_non_square_matrixmap_equals_its_copy() {
    let mut m = MatrixMap::new(ndarray::array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]).unwrap();
    m.invert();
    assert!(!m.has_forward());
    assert!(m.equal(&m.clone()));
    let mut other = MatrixMap::new(ndarray::array![[1.0, 2.0, 3.0], [4.0, 5.0, 7.0]]).unwrap();
    other.invert();
    assert!(!m.equal(&other));
}

/// A diagonal MatrixMap over a long array: a bad, NaN or infinite input
/// gives a bad output, and a finite input whose product overflows gives the
/// infinite product, wherever in the array they fall.
#[test]
fn matrix_diagonal_long_array_with_non_finite_inputs() {
    let diag = [2.0, 0.0, 1e300];
    let mm = MatrixMap::diagonal(3, 3, &diag).unwrap();
    let n = 2051;
    let specials = [
        BAD_VALUE,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        1e10,
    ];
    let value = |axis: usize, i: usize| {
        if i % 97 == axis * 13 || i == n - 1 - axis {
            specials[(i / 97 + axis) % specials.len()]
        } else {
            (i as f64 - 1000.0) * 0.37
        }
    };
    let pts = PointSet::from_rows((0..n).map(|i| (0..3).map(|a| value(a, i)).collect::<Vec<_>>()));
    let out = mm.transform(&pts, true).unwrap();
    for i in 0..n {
        for (a, &d) in diag.iter().enumerate() {
            let v = value(a, i);
            let want = if v == BAD_VALUE || !v.is_finite() {
                BAD_VALUE
            } else {
                d * v
            };
            assert_eq!(
                out.point(i)[a].to_bits(),
                want.to_bits(),
                "point {i} axis {a}, input {v}"
            );
        }
    }
    // Every kind of special value, and an overflow, occurs.
    assert!((0..n).any(|i| value(2, i) == 1e10));
    assert!((0..n).any(|i| value(1, i).is_nan()));
}

/// An inverted singular MatrixMap, which has no forward transformation.
fn inverted_singular() -> MatrixMap {
    let mut m = MatrixMap::new(array![[1.0_f64, 1.0], [1.0, 1.0]]).unwrap();
    m.invert();
    m
}

/// Merging a MatrixMap with a neighbor multiplies the matrices, which an
/// inverted singular MatrixMap cannot take part in, and C passes the error
/// up from the simplification.
#[test]
fn merging_with_an_inverted_singular_matrix_is_an_error() {
    use starlink_ast::mappings::{CmpMap, Composition, ZoomMap};
    let full = MatrixMap::new(array![[1.0_f64, 2.0], [3.0, 4.0]]).unwrap();
    let pairs: [(Box<dyn Mapping>, Box<dyn Mapping>); 2] = [
        (Box::new(full), Box::new(inverted_singular())),
        (
            Box::new(inverted_singular()),
            Box::new(ZoomMap::new(2, 3.0).unwrap()),
        ),
    ];
    for (first, second) in pairs {
        let map = CmpMap::new(first, second, Composition::Series).unwrap();
        let err = <dyn Mapping>::simplify(&map).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Mtrml);
    }
}

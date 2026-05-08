//! Tests for MatrixMap.
//!
//! Ancestor: (no direct C test — MatrixMap tests derived from the C AST
//! MatrixMap documentation and reference behaviour.)

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, MatrixMap};
use ndarray::array;

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
    let simplified = mm.simplified().unwrap();
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
    let simplified = mm.simplified().unwrap();
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
    use ast_core::mappings::PermMap;

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
    use ast_core::mappings::{CmpMap, Composition, PermMap};

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
    let simplified = cmp.simplified().unwrap();

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
    use ast_core::mappings::PermMap;

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
    use ast_core::mappings::PermMap;

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
    use ast_core::mappings::PermMap;

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
    use ast_core::mappings::WinMap;

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

/// Diagonal matrix: BAD_VALUE with zero diagonal element should produce 0.0
/// (zero coeff masks BAD), non-zero diagonal with BAD should produce BAD.
#[test]
fn matrix_diagonal_bad_handling() {
    // Diagonal [[0, 0], [0, 5]] — first diag is 0, second is 5
    let m = array![[0.0, 0.0], [0.0, 5.0]];
    let mm = MatrixMap::new(m).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 3.0]]);
    let out = mm.transform(&pts, true).unwrap();
    // Row 0: diag=0 → output is 0.0 regardless of input
    assert_eq!(out.point(0)[0], 0.0);
    // Row 1: diag=5, input=3.0 → 15.0
    assert_eq!(out.point(0)[1], 15.0);

    // Now BAD in the second input where diag is non-zero
    let pts2 = PointSet::from_rows([[2.0, BAD_VALUE]]);
    let out2 = mm.transform(&pts2, true).unwrap();
    assert_eq!(out2.point(0)[0], 0.0); // diag=0 → 0
    assert_eq!(out2.point(0)[1], BAD_VALUE); // diag=5, input=BAD → BAD
}

/// Non-diagonal MatrixMap should NOT merge with WinMap.
#[test]
fn non_diagonal_matrix_no_merge_with_winmap() {
    use ast_core::mappings::WinMap;

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
    use ast_core::mappings::WinMap;

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

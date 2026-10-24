//! Patch a square CD matrix to be invertible when degenerate-axis
//! singularities allow it.
//!
//! Mirrors `fitschan.c::MakeInvertable` (~22314). Looks for matrix
//! inputs that contribute nothing to any output; when such an input
//! is associated with a degenerate grid axis (`dim == 1.0`), the
//! input is guaranteed to be zero, so the matrix's diagonal entry on
//! that input can be set to `1.0` without changing any output. This
//! turns an otherwise-singular matrix into one that the palDmat
//! solver in `make_int_world` can invert.

// Wired up in Task 4.4 (`make_int_world`); until then nothing in this
// module is reachable from the crate's non-test code.
#![allow(dead_code)]

use ndarray::Array2;

/// Patch the diagonal of `cd` for inputs that have no effect on any
/// output AND correspond to a degenerate (single-pixel) grid axis.
/// Sets such diagonal entries to `1.0`; leaves the matrix unchanged
/// otherwise.
///
/// `cd` is row-major shape `(nout, nin)` (i.e. `cd[[j, i]]` is the
/// contribution of input `i` to output `j`). The function only does
/// useful work for square `cd` (the call site in `make_int_world`
/// passes a square matrix after `orth_vector_set` has padded out
/// missing columns).
///
/// `dim` is `Some(d)` for known pixel-axis lengths, `None` for unknown
/// (mirrors `dim[i] == AST__BAD` in C). The patch only fires when an
/// input's dim is exactly `1.0`.
///
/// Mirrors `fitschan.c::MakeInvertable`.
pub(crate) fn make_invertable(cd: &mut Array2<f64>, dim: &[Option<f64>]) {
    let nout = cd.nrows();
    let nin = cd.ncols();
    let n = nout.min(nin);
    let upper = n.min(dim.len());
    for i in 0..upper {
        // "Unused input" = the column for this input is identically
        // zero across every output. Mirrors C's inner loop at lines
        // 22373-22382.
        let unused = (0..nout).all(|j| cd[[j, i]] == 0.0);
        if unused && dim[i] == Some(1.0) {
            cd[[i, i]] = 1.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(rows: &[&[f64]]) -> Array2<f64> {
        let nrows = rows.len();
        let ncols = rows[0].len();
        let flat: Vec<f64> = rows.iter().flat_map(|r| r.iter().copied()).collect();
        Array2::from_shape_vec((nrows, ncols), flat).expect("2d")
    }

    #[test]
    fn already_invertable_unchanged() {
        let mut cd = mat(&[&[1.0, 0.0], &[0.0, 1.0]]);
        let snapshot = cd.clone();
        let dim = vec![Some(100.0), Some(100.0)];
        make_invertable(&mut cd, &dim);
        assert_eq!(cd, snapshot);
    }

    #[test]
    fn degenerate_input_zeroed_column_patched() {
        // Input 1 (column 1) is all zeros; dim[1] = 1.0 → diagonal
        // patched to 1.0.
        let mut cd = mat(&[&[2.0, 0.0], &[3.0, 0.0]]);
        let dim = vec![Some(100.0), Some(1.0)];
        make_invertable(&mut cd, &dim);
        assert_eq!(cd[[0, 0]], 2.0);
        assert_eq!(cd[[1, 0]], 3.0);
        assert_eq!(cd[[0, 1]], 0.0);
        assert_eq!(cd[[1, 1]], 1.0); // patched
    }

    #[test]
    fn non_degenerate_zeroed_column_left_alone() {
        // Input 1 unused but dim[1] != 1.0 → no patch (the matrix
        // stays singular and the palDmat solve will fail downstream,
        // matching C semantics).
        let mut cd = mat(&[&[2.0, 0.0], &[3.0, 0.0]]);
        let dim = vec![Some(100.0), Some(50.0)];
        make_invertable(&mut cd, &dim);
        assert_eq!(cd[[1, 1]], 0.0);
    }

    #[test]
    fn unknown_dim_treated_as_non_degenerate() {
        // dim entry of None (i.e. AST__BAD) is not a degenerate axis.
        let mut cd = mat(&[&[2.0, 0.0], &[3.0, 0.0]]);
        let dim = vec![Some(100.0), None];
        make_invertable(&mut cd, &dim);
        assert_eq!(cd[[1, 1]], 0.0);
    }

    #[test]
    fn input_with_partial_zero_column_unchanged() {
        // Column 1 has a non-zero entry → "unused" is false.
        let mut cd = mat(&[&[2.0, 0.5], &[3.0, 0.0]]);
        let dim = vec![Some(100.0), Some(1.0)];
        make_invertable(&mut cd, &dim);
        assert_eq!(cd[[0, 1]], 0.5);
        assert_eq!(cd[[1, 1]], 0.0); // not patched — column wasn't all zeros
    }

    #[test]
    fn three_axis_one_degenerate() {
        // Input 1 is the unused-and-degenerate one; the others are
        // populated normally.
        let mut cd = mat(&[&[1.0, 0.0, 0.0], &[0.0, 0.0, 1.0], &[0.0, 0.0, 0.0]]);
        let dim = vec![Some(100.0), Some(1.0), Some(50.0)];
        make_invertable(&mut cd, &dim);
        assert_eq!(cd[[0, 0]], 1.0);
        assert_eq!(cd[[1, 1]], 1.0); // patched
        assert_eq!(cd[[2, 2]], 0.0); // not patched (column 2 is unused but dim != 1)
    }

    #[test]
    fn empty_dim_array_skips_patch() {
        // dim shorter than n → only iterate up to dim.len() (defensive
        // against caller mistakes).
        let mut cd = mat(&[&[0.0, 0.0], &[0.0, 0.0]]);
        let dim: Vec<Option<f64>> = vec![];
        make_invertable(&mut cd, &dim);
        assert_eq!(cd[[0, 0]], 0.0);
        assert_eq!(cd[[1, 1]], 0.0);
    }
}

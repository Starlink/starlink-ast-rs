//! Orthogonal-vector completion of a partial vector set.
//!
//! Mirrors `fitschan.c::OrthVector` (~23690) and `OrthVectorSet`
//! (~23938). Used by `make_int_world` to fill in CD columns when
//! the assembled pixel→IWC mapping has `nout > nin` (i.e. degenerate
//! grid axes that the writer needs to span out into FITS columns).

// Wired up in Task 4.4 (`make_int_world`); until then nothing in this
// module is reachable from the crate's non-test code.
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use crate::mappings::polymap::solve_linear_system;

/// Find a unit `n`-vector orthogonal to every vector in `in_vectors`,
/// where `m = in_vectors.len()` and each row has length `n`.
///
/// Returns `None` when:
///   - `m >= n` (the partial set already exhausts the space),
///   - any input row has the wrong length,
///   - the chosen `m × m` sub-system is singular (the partial set
///     doesn't span `m` independent axes — mirrors the `sing != 0`
///     branch in `OrthVector`).
///
/// Mirrors `fitschan.c::OrthVector`.
pub(crate) fn orth_vector(in_vectors: &[Vec<f64>], n: usize) -> Option<Vec<f64>> {
    let m = in_vectors.len();
    if m == 0 || m >= n {
        return None;
    }
    for v in in_vectors {
        if v.len() != n {
            return None;
        }
    }

    // Step 1: column magnitudes — colmag[i] = Σ_j (in[j][i])^2.
    let mut colmag = vec![0.0_f64; n];
    for i in 0..n {
        let mut acc = 0.0_f64;
        for j in 0..m {
            let x = in_vectors[j][i];
            acc += x * x;
        }
        colmag[i] = acc;
    }

    // Step 2: bubble-sort column indices by descending magnitude.
    // Mirrors C lines 23847-23861 (intentional bubble sort to keep the
    // permutation stable for ties — a stable sort_by would also work
    // but the C source uses bubble; we mirror it exactly).
    let mut colperm: Vec<usize> = (0..n).collect();
    let mut ii = n;
    let mut done = false;
    while !done {
        done = true;
        for i in 1..ii {
            let ih = colperm[i];
            let il = colperm[i - 1];
            if colmag[ih] > colmag[il] {
                colperm[i] = il;
                colperm[i - 1] = ih;
                done = false;
            }
        }
        ii = ii.saturating_sub(1);
    }

    // Step 3: build the m × m matrix and the m-vector RHS.
    //   mat[j*m + k] = in[j][colperm[k]]    for k in 0..m
    //   rhs[j]       = -in[j][colperm[m]]
    let mut mat = vec![0.0_f64; m * m];
    let mut rhs = vec![0.0_f64; m];
    for j in 0..m {
        for k in 0..m {
            mat[j * m + k] = in_vectors[j][colperm[k]];
        }
        rhs[j] = -in_vectors[j][colperm[m]];
    }

    // Step 4: palDmat-style solve — mat · x = rhs, in-place into rhs.
    if !solve_linear_system(&mut mat, &mut rhs, m) {
        return None;
    }

    // Step 5: build the n-vector. Components at colperm[0..m] take the
    // solved values; colperm[m] is set to 1.0; colperm[m+1..n] are
    // zeros. Then normalise to unit length, where the squared length
    // is (Σ x_j² for j in 0..m) + 1.
    let mut ret = vec![0.0_f64; n];
    let mut squared_len = 0.0_f64;
    for j in 0..m {
        let x = rhs[j];
        squared_len += x * x;
        ret[colperm[j]] = x;
    }
    squared_len += 1.0;
    ret[colperm[m]] = 1.0;
    // The remaining colperm[m+1..n] entries stay at 0.0 (already initialised).

    let inv_norm = 1.0 / squared_len.sqrt();
    for v in &mut ret {
        *v *= inv_norm;
        // Mirror C's "+0.0 instead of -0.0" cleanup.
        if *v == 0.0 {
            *v = 0.0;
        }
    }

    Some(ret)
}

/// Given `m` vectors of length `n` (where `m <= n`), return a set of
/// `n` mutually orthogonal vectors whose first `m` rows are the
/// supplied vectors verbatim and whose remaining `n - m` rows are
/// unit vectors orthogonal to every previous row. Returns `None`
/// when no such completion exists (e.g. the supplied vectors span
/// fewer than `m` independent axes).
///
/// Mirrors `fitschan.c::OrthVectorSet`.
pub(crate) fn orth_vector_set(in_vectors: &[Vec<f64>], n: usize) -> Option<Vec<Vec<f64>>> {
    let m = in_vectors.len();
    if m > n {
        return None;
    }
    for v in in_vectors {
        if v.len() != n {
            return None;
        }
    }

    let mut ret: Vec<Vec<f64>> = Vec::with_capacity(n);
    for v in in_vectors {
        ret.push(v.clone());
    }
    while ret.len() < n {
        let next = orth_vector(&ret, n)?;
        ret.push(next);
    }
    Some(ret)
}

#[cfg(test)]
#[allow(clippy::excessive_precision)]
mod tests {
    use super::*;

    // Captured from tools/oracle_fixtures/orth_vector_set.c via
    // `make orth_vector_set > captured.rs`. Regenerate when the
    // upstream reference C library is rebuilt.

    /// Fixture: ORTH_2_OF_3_BASIS_E0E1 — n=3, m=2
    pub const ORTH_2_OF_3_BASIS_E0E1: Option<&[&[f64]]> =
        Some(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);

    /// Fixture: ORTH_1_OF_2_BASIS_E0 — n=2, m=1
    pub const ORTH_1_OF_2_BASIS_E0: Option<&[&[f64]]> = Some(&[&[1.0, 0.0], &[0.0, 1.0]]);

    /// Fixture: ORTH_2_OF_3_SKEWED — n=3, m=2
    pub const ORTH_2_OF_3_SKEWED: Option<&[&[f64]]> =
        Some(&[&[1.0, 1.0, 0.0], &[1.0, -1.0, 0.0], &[0.0, 0.0, 1.0]]);

    /// Fixture: ORTH_2_OF_3_PARALLEL — n=3, m=2
    pub const ORTH_2_OF_3_PARALLEL: Option<&[&[f64]]> = None;

    /// Fixture: ORTH_1_OF_3_NONUNIT — n=3, m=1
    pub const ORTH_1_OF_3_NONUNIT: Option<&[&[f64]]> = Some(&[
        &[3.0, 0.0, 4.0],
        &[0.80000000000000004, 0.0, -0.59999999999999998],
        &[0.0, 1.0, 0.0],
    ]);

    /// Fixture: ORTH_3_OF_4_BASIS — n=4, m=3
    pub const ORTH_3_OF_4_BASIS: Option<&[&[f64]]> = Some(&[
        &[1.0, 0.0, 0.0, 0.0],
        &[0.0, 1.0, 0.0, 0.0],
        &[0.0, 0.0, 1.0, 0.0],
        &[0.0, 0.0, 0.0, 1.0],
    ]);

    fn assert_set_matches(
        actual: &Option<Vec<Vec<f64>>>,
        expected: Option<&[&[f64]]>,
        tol: f64,
        label: &str,
    ) {
        match (actual, expected) {
            (None, None) => {}
            (Some(a), Some(e)) => {
                assert_eq!(a.len(), e.len(), "{}: row count", label);
                for (i, (row_a, row_e)) in a.iter().zip(e.iter()).enumerate() {
                    assert_eq!(row_a.len(), row_e.len(), "{} row {}: len", label, i);
                    for (j, (va, ve)) in row_a.iter().zip(row_e.iter()).enumerate() {
                        assert!(
                            (va - ve).abs() < tol,
                            "{} [{}][{}]: {} vs {}",
                            label,
                            i,
                            j,
                            va,
                            ve
                        );
                    }
                }
            }
            (a, e) => panic!(
                "{}: variant mismatch — actual={:?}, expected={:?}",
                label, a, e
            ),
        }
    }

    fn vecs(rows: &[&[f64]]) -> Vec<Vec<f64>> {
        rows.iter().map(|r| r.to_vec()).collect()
    }

    #[test]
    fn two_of_three_basis_e0e1() {
        let r = orth_vector_set(&vecs(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0]]), 3);
        assert_set_matches(&r, ORTH_2_OF_3_BASIS_E0E1, 1e-12, "2_of_3_basis");
    }

    #[test]
    fn one_of_two_basis_e0() {
        let r = orth_vector_set(&vecs(&[&[1.0, 0.0]]), 2);
        assert_set_matches(&r, ORTH_1_OF_2_BASIS_E0, 1e-12, "1_of_2_basis");
    }

    #[test]
    fn two_of_three_skewed() {
        let r = orth_vector_set(&vecs(&[&[1.0, 1.0, 0.0], &[1.0, -1.0, 0.0]]), 3);
        assert_set_matches(&r, ORTH_2_OF_3_SKEWED, 1e-12, "2_of_3_skewed");
    }

    #[test]
    fn two_of_three_parallel_returns_none() {
        let r = orth_vector_set(&vecs(&[&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]]), 3);
        assert_set_matches(&r, ORTH_2_OF_3_PARALLEL, 1e-12, "parallel");
    }

    #[test]
    fn one_of_three_nonunit_preserves_input() {
        let r = orth_vector_set(&vecs(&[&[3.0, 0.0, 4.0]]), 3);
        assert_set_matches(&r, ORTH_1_OF_3_NONUNIT, 1e-12, "nonunit");
    }

    #[test]
    fn three_of_four_basis_completes_to_e3() {
        let r = orth_vector_set(
            &vecs(&[
                &[1.0, 0.0, 0.0, 0.0],
                &[0.0, 1.0, 0.0, 0.0],
                &[0.0, 0.0, 1.0, 0.0],
            ]),
            4,
        );
        assert_set_matches(&r, ORTH_3_OF_4_BASIS, 1e-12, "3_of_4_basis");
    }

    #[test]
    fn wrong_row_length_returns_none() {
        let r = orth_vector_set(&vecs(&[&[1.0, 0.0]]), 3);
        assert!(r.is_none());
    }

    #[test]
    fn empty_input_returns_full_basis() {
        // m=0 → n iterations of orth_vector against the running set.
        // The first call has m=0 inside orth_vector, which our
        // implementation rejects (returns None). So orth_vector_set
        // also returns None. This matches the C contract: callers
        // guarantee m >= 1 (see fitschan.c::MakeIntWorld:21912 etc.).
        let r = orth_vector_set(&vecs(&[]), 3);
        assert!(r.is_none());
    }

    #[test]
    fn orth_vector_orthogonal_to_input_set() {
        // Sanity-check for a non-axis-aligned input.
        let inputs = vec![vec![1.0_f64, 2.0, 3.0]];
        let v = orth_vector(&inputs, 3).expect("expected a result");
        let dot: f64 = v.iter().zip(&inputs[0]).map(|(a, b)| a * b).sum();
        assert!(dot.abs() < 1e-12, "dot product {}", dot);
        let len: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((len - 1.0).abs() < 1e-12, "not unit: len={}", len);
    }
}

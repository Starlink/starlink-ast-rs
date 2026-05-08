//! N-dimensional polynomial transformation mapping.

use levenberg_marquardt::{LeastSquaresProblem, LevenbergMarquardt};
use nalgebra::{DMatrix, DVector, Dyn, OMatrix, OVector, VecStorage};

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, point_has_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A single polynomial term: `coeff * x0^p0 * x1^p1 * ... * xN^pN`.
///
/// Each term contributes to exactly one output axis.
#[derive(Debug, Clone, PartialEq)]
pub struct PolyTerm {
    /// Coefficient value.
    pub coeff: f64,
    /// Power of each input axis (length = `nin` for forward, `nout` for inverse).
    pub powers: Vec<usize>,
    /// 1-based output axis this term contributes to.
    pub out_axis: usize,
}

/// An N-dimensional polynomial transformation mapping.
///
/// The forward transformation is defined by a set of polynomial terms,
/// each contributing to a specified output axis. An optional explicit
/// inverse can be provided, or an iterative Newton-Raphson inverse
/// can be enabled.
#[derive(Debug, Clone)]
pub struct PolyMap {
    nin: usize,
    nout: usize,
    forward: Vec<PolyTerm>,
    inverse: Option<Vec<PolyTerm>>,
    iter_inverse: bool,
    niter_inverse: usize,
    tol_inverse: f64,
    inverted: bool,
    /// Pre-computed Jacobian polynomial terms (one set of terms per input axis).
    /// Only populated when `iter_inverse` is true and `nin == nout`.
    jacobian_terms: Option<Vec<Vec<PolyTerm>>>,
    // -- Pre-computed evaluation indices for power-table evaluation --
    /// Indices into `forward` grouped by output axis (0-based).
    /// `forward_by_output[i]` contains indices of terms contributing to output axis `i+1`.
    forward_by_output: Vec<Vec<usize>>,
    /// Maximum power per input axis across all forward terms.
    forward_max_pows: Vec<usize>,
    /// Indices into `inverse` terms grouped by output axis, if inverse exists.
    inverse_by_output: Option<Vec<Vec<usize>>>,
    /// Maximum power per input axis across all inverse terms.
    inverse_max_pows: Vec<usize>,
}

// ---------------------------------------------------------------------------
// Pre-computation helpers for power-table evaluation
// ---------------------------------------------------------------------------

/// Compute the maximum power per input axis across a set of terms.
fn compute_max_powers(terms: &[PolyTerm], nin: usize) -> Vec<usize> {
    let mut max_pows = vec![0usize; nin];
    for term in terms {
        for (i, &p) in term.powers.iter().enumerate().take(nin) {
            if p > max_pows[i] {
                max_pows[i] = p;
            }
        }
    }
    max_pows
}

/// Group term indices by their output axis (0-based output index).
fn compute_terms_by_output(terms: &[PolyTerm], nout: usize) -> Vec<Vec<usize>> {
    let mut by_output = vec![Vec::new(); nout];
    for (idx, term) in terms.iter().enumerate() {
        by_output[term.out_axis - 1].push(idx);
    }
    by_output
}

impl PolyMap {
    /// Create a new `PolyMap` with the given polynomial terms.
    ///
    /// `forward` defines the forward transformation: each term's
    /// `powers` must have length `nin`, and `out_axis` must be in `1..=nout`.
    ///
    /// `inverse`, if provided, defines the explicit inverse: each term's
    /// `powers` must have length `nout`, and `out_axis` must be in `1..=nin`.
    ///
    /// Returns an error if any term is invalid.
    pub fn new(
        nin: usize,
        nout: usize,
        forward: &[PolyTerm],
        inverse: Option<&[PolyTerm]>,
    ) -> AstResult<Self> {
        if nin == 0 {
            return Err(AstError::new(ErrorCode::Badin, "PolyMap nin must be > 0"));
        }
        if nout == 0 {
            return Err(AstError::new(ErrorCode::Badin, "PolyMap nout must be > 0"));
        }

        // Validate forward terms.
        for (i, term) in forward.iter().enumerate() {
            if term.powers.len() != nin {
                return Err(AstError::new(
                    ErrorCode::Badin,
                    format!(
                        "forward term {} has {} powers but nin = {}",
                        i,
                        term.powers.len(),
                        nin
                    ),
                ));
            }
            if term.out_axis < 1 || term.out_axis > nout {
                return Err(AstError::new(
                    ErrorCode::Badin,
                    format!(
                        "forward term {} has out_axis {} but nout = {}",
                        i, term.out_axis, nout
                    ),
                ));
            }
            if term.coeff == 0.0 {
                return Err(AstError::new(
                    ErrorCode::Badin,
                    format!("forward term {} has zero coefficient", i),
                ));
            }
        }

        // Validate inverse terms, if provided.
        let inv = if let Some(inv_terms) = inverse {
            for (i, term) in inv_terms.iter().enumerate() {
                if term.powers.len() != nout {
                    return Err(AstError::new(
                        ErrorCode::Badin,
                        format!(
                            "inverse term {} has {} powers but nout = {}",
                            i,
                            term.powers.len(),
                            nout
                        ),
                    ));
                }
                if term.out_axis < 1 || term.out_axis > nin {
                    return Err(AstError::new(
                        ErrorCode::Badin,
                        format!(
                            "inverse term {} has out_axis {} but nin = {}",
                            i, term.out_axis, nin
                        ),
                    ));
                }
                if term.coeff == 0.0 {
                    return Err(AstError::new(
                        ErrorCode::Badin,
                        format!("inverse term {} has zero coefficient", i),
                    ));
                }
            }
            Some(inv_terms.to_vec())
        } else {
            None
        };

        let fwd_vec = forward.to_vec();
        let forward_by_output = compute_terms_by_output(&fwd_vec, nout);
        let forward_max_pows = compute_max_powers(&fwd_vec, nin);
        let (inverse_by_output, inverse_max_pows) = if let Some(ref inv_terms) = inv {
            (
                Some(compute_terms_by_output(inv_terms, nin)),
                compute_max_powers(inv_terms, nout),
            )
        } else {
            (None, Vec::new())
        };

        Ok(Self {
            nin,
            nout,
            forward: fwd_vec,
            inverse: inv,
            iter_inverse: false,
            niter_inverse: 4,
            tol_inverse: 1e-6,
            inverted: false,
            jacobian_terms: None,
            forward_by_output,
            forward_max_pows,
            inverse_by_output,
            inverse_max_pows,
        })
    }

    /// Whether iterative (Levenberg-Marquardt) inverse is enabled.
    pub fn iter_inverse(&self) -> bool {
        self.iter_inverse
    }

    /// Enable or disable the iterative inverse.
    pub fn set_iter_inverse(&mut self, v: bool) {
        self.iter_inverse = v;
        if v && self.nin == self.nout && self.jacobian_terms.is_none() {
            self.jacobian_terms = Some(self.compute_jacobian_terms());
        } else if !v {
            self.jacobian_terms = None;
        }
    }

    /// Maximum number of iterations for the iterative inverse.
    pub fn niter_inverse(&self) -> usize {
        self.niter_inverse
    }

    /// Set the maximum number of iterations for the iterative inverse.
    pub fn set_niter_inverse(&mut self, v: usize) {
        self.niter_inverse = v;
    }

    /// Target relative error for the iterative inverse.
    pub fn tol_inverse(&self) -> f64 {
        self.tol_inverse
    }

    /// Set the target relative error for the iterative inverse.
    pub fn set_tol_inverse(&mut self, v: f64) {
        self.tol_inverse = v;
    }

    /// Return a reference to the forward polynomial terms.
    pub fn forward_terms(&self) -> &[PolyTerm] {
        &self.forward
    }

    /// Return a reference to the inverse polynomial terms, if defined.
    pub fn inverse_terms(&self) -> Option<&[PolyTerm]> {
        self.inverse.as_deref()
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), next, Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), other, Composition::Parallel)
    }

    // ------------------------------------------------------------------
    // Newton-Raphson iterative inverse helpers
    // ------------------------------------------------------------------

    /// Compute the Jacobian of the forward polynomial as differentiated
    /// polynomial terms.
    ///
    /// Returns `Vec<Vec<PolyTerm>>` indexed by input axis `j` (0-based).
    /// Each inner `Vec<PolyTerm>` contains terms representing
    /// `d(forward_i)/d(x_j)` for all output axes `i`, suitable for
    /// evaluation via [`eval_poly`].
    fn compute_jacobian_terms(&self) -> Vec<Vec<PolyTerm>> {
        compute_jacobian_terms_from(&self.forward, self.nin)
    }

    /// Return cached Jacobian terms, computing them if not yet cached.
    fn get_jacobian_terms(&self) -> &[Vec<PolyTerm>] {
        // In the optimized path, jacobian_terms should always be Some
        // when newton_raphson_inverse is called (set during construction
        // or set_iter_inverse). But fall back to compute if somehow missing.
        // This cannot happen in normal usage since we set it eagerly.
        debug_assert!(
            self.jacobian_terms.is_some(),
            "jacobian_terms should be pre-computed"
        );
        // Safety: we always populate this before calling NR inverse.
        self.jacobian_terms.as_ref().unwrap()
    }

    /// Compute a linear approximation of the inverse and use it as the
    /// initial guess for Newton-Raphson iteration.
    ///
    /// Extracts the constant and linear terms from the forward polynomial,
    /// builds the linear system `y = offset + A*x`, inverts A, and applies
    /// `x0 = A^{-1} * (target - offset)`.
    ///
    /// Falls back to copying target values (identity guess) if the linear
    /// part is singular.
    fn compute_linear_guess(&self, target: &PointSet, result: &mut PointSet, converged: &[bool]) {
        let ncoord = self.nin;
        let npoint = target.npoint();

        // Extract constant terms (power sum = 0) and linear terms (exactly
        // one power = 1, rest = 0) from the forward polynomial.
        let mut offset = vec![0.0; ncoord];
        let mut matrix = vec![0.0; ncoord * ncoord]; // row-major: matrix[out*ncoord + in]

        for term in &self.forward {
            let power_sum: usize = term.powers.iter().sum();
            let out_idx = term.out_axis - 1;
            if power_sum == 0 {
                offset[out_idx] += term.coeff;
            } else if power_sum == 1 {
                for (j, &p) in term.powers.iter().enumerate() {
                    if p == 1 {
                        matrix[out_idx * ncoord + j] += term.coeff;
                        break;
                    }
                }
            }
        }

        // Try to invert the linear matrix.
        let inv_matrix = if ncoord == 1 {
            let a = matrix[0];
            if a.abs() > 1e-30 {
                Some(vec![1.0 / a])
            } else {
                None
            }
        } else if ncoord == 2 {
            let a = matrix[0];
            let b = matrix[1];
            let c = matrix[2];
            let d = matrix[3];
            let det = a * d - b * c;
            if det.abs() > 1e-30 {
                let inv_det = 1.0 / det;
                Some(vec![d * inv_det, -b * inv_det, -c * inv_det, a * inv_det])
            } else {
                None
            }
        } else {
            // General NxN: attempt inversion via Gaussian elimination.
            // Build augmented identity and solve.
            let mut aug = vec![0.0; ncoord * ncoord];
            aug.copy_from_slice(&matrix);
            let mut inv = vec![0.0; ncoord * ncoord];
            for i in 0..ncoord {
                inv[i * ncoord + i] = 1.0;
            }
            // Forward elimination with partial pivoting.
            let mut ok = true;
            for col in 0..ncoord {
                let mut max_val = aug[col * ncoord + col].abs();
                let mut max_row = col;
                for row in (col + 1)..ncoord {
                    let v = aug[row * ncoord + col].abs();
                    if v > max_val {
                        max_val = v;
                        max_row = row;
                    }
                }
                if max_val < 1e-30 {
                    ok = false;
                    break;
                }
                if max_row != col {
                    for k in 0..ncoord {
                        aug.swap(col * ncoord + k, max_row * ncoord + k);
                        inv.swap(col * ncoord + k, max_row * ncoord + k);
                    }
                }
                let pivot = aug[col * ncoord + col];
                for k in 0..ncoord {
                    aug[col * ncoord + k] /= pivot;
                    inv[col * ncoord + k] /= pivot;
                }
                for row in 0..ncoord {
                    if row == col {
                        continue;
                    }
                    let factor = aug[row * ncoord + col];
                    for k in 0..ncoord {
                        aug[row * ncoord + k] -= factor * aug[col * ncoord + k];
                        inv[row * ncoord + k] -= factor * inv[col * ncoord + k];
                    }
                }
            }
            if ok { Some(inv) } else { None }
        };

        // Apply the linear inverse guess.
        #[allow(clippy::needless_range_loop)]
        if let Some(ref inv_mat) = inv_matrix {
            for ip in 0..npoint {
                if converged[ip] {
                    continue;
                }
                let t = target.point(ip);
                let r = result.point_mut(ip);
                for i in 0..ncoord {
                    let mut sum = 0.0;
                    for j in 0..ncoord {
                        sum += inv_mat[i * ncoord + j] * (t[j] - offset[j]);
                    }
                    r[i] = sum;
                }
            }
        } else {
            for ip in 0..npoint {
                if converged[ip] {
                    continue;
                }
                result.point_mut(ip).copy_from_slice(target.point(ip));
            }
        }
    }

    /// Perform a bulk Newton-Raphson iterative inverse.
    ///
    /// Given target output positions in `target`, compute the input
    /// positions that produce them under the forward polynomial, writing
    /// results into `result`.
    ///
    /// Optimizations:
    /// - Pre-computed Jacobian polynomial terms (computed once, reused across calls)
    /// - Linear initial guess (faster convergence, fewer iterations needed)
    /// - Combined forward+Jacobian evaluation: a single power table is computed
    ///   once per point and reused to evaluate both the forward polynomial and
    ///   all Jacobian columns, avoiding redundant power computation.
    fn newton_raphson_inverse(&self, target: &PointSet, result: &mut PointSet) -> AstResult<()> {
        let npoint = target.npoint();
        let ncoord = self.nin; // nin == nout for iterative inverse
        debug_assert_eq!(self.nin, self.nout);

        // Use cached Jacobian terms.
        let jac_terms = self.get_jacobian_terms();

        let max_iter = self.niter_inverse;
        let tol_sq = self.tol_inverse * self.tol_inverse;
        let mut converged = vec![false; npoint];

        // Mark BAD points as already converged and fill their output with BAD.
        #[allow(clippy::needless_range_loop)]
        for i in 0..npoint {
            if point_has_bad(target.point(i)) {
                converged[i] = true;
                result.point_mut(i).fill(BAD_VALUE);
            }
        }

        // Better initial guess via linear truncation of forward polynomial.
        self.compute_linear_guess(target, result, &converged);

        // Pre-compute max powers across forward AND all Jacobian terms
        // so that the shared power table covers everything.
        let mut nr_max_pows = self.forward_max_pows.clone();
        for jcol in jac_terms {
            for term in jcol {
                for (i, &p) in term.powers.iter().enumerate().take(ncoord) {
                    if p > nr_max_pows[i] {
                        nr_max_pows[i] = p;
                    }
                }
            }
        }

        // Pre-group Jacobian terms by output axis for each column.
        let jac_by_output: Vec<Vec<Vec<usize>>> = jac_terms
            .iter()
            .map(|col_terms| compute_terms_by_output(col_terms, ncoord))
            .collect();

        // Allocate shared power table (reused for all points across all iterations).
        let mut power_table: Vec<Vec<f64>> =
            nr_max_pows.iter().map(|&m| vec![0.0; m + 1]).collect();

        // Scratch space for forward output, Jacobian matrix, and RHS.
        let mut fwd_out = vec![0.0; ncoord];
        let mut jac_matrix = vec![0.0; ncoord * ncoord];
        let mut dy = vec![0.0; ncoord];

        for _iter in 0..max_iter {
            if converged.iter().all(|&c| c) {
                break;
            }

            #[allow(clippy::needless_range_loop)]
            for ip in 0..npoint {
                if converged[ip] {
                    continue;
                }

                let inp = result.point(ip);

                // Fill power table once for this point.
                fill_power_table(inp, &nr_max_pows, &mut power_table);

                // Evaluate forward polynomial using the power table.
                for (out_idx, term_indices) in self.forward_by_output.iter().enumerate() {
                    let mut sum = 0.0;
                    for &idx in term_indices {
                        sum += eval_term_with_table(&self.forward[idx], &power_table);
                    }
                    fwd_out[out_idx] = sum;
                }

                // Evaluate Jacobian columns using the same power table.
                // J[row][col] = d(F_row)/d(x_col)
                for col in 0..ncoord {
                    let col_terms = &jac_terms[col];
                    for (row, term_indices) in jac_by_output[col].iter().enumerate() {
                        let mut sum = 0.0;
                        for &idx in term_indices {
                            sum += eval_term_with_table(&col_terms[idx], &power_table);
                        }
                        jac_matrix[row * ncoord + col] = sum;
                    }
                }

                // Compute residual: dy = target - forward(guess).
                let tgt_pt = target.point(ip);
                for ic in 0..ncoord {
                    dy[ic] = tgt_pt[ic] - fwd_out[ic];
                }

                // Solve J * dx = dy.
                if ncoord == 1 {
                    let j00 = jac_matrix[0];
                    if j00.abs() < 1e-30 {
                        result.point_mut(ip).fill(BAD_VALUE);
                        converged[ip] = true;
                        continue;
                    }
                    let dx = dy[0] / j00;
                    result.point_mut(ip)[0] += dx;
                    let xlensq = result.point(ip)[0].powi(2);
                    if dx * dx < tol_sq * xlensq || xlensq == 0.0 {
                        converged[ip] = true;
                    }
                } else if ncoord == 2 {
                    // Direct 2x2 solve via Cramer's rule.
                    let a = jac_matrix[0];
                    let b = jac_matrix[1];
                    let c = jac_matrix[2];
                    let d = jac_matrix[3];
                    let det = a * d - b * c;
                    if det.abs() < 1e-30 {
                        result.point_mut(ip).fill(BAD_VALUE);
                        converged[ip] = true;
                        continue;
                    }
                    let dx0 = (d * dy[0] - b * dy[1]) / det;
                    let dx1 = (a * dy[1] - c * dy[0]) / det;
                    result.point_mut(ip)[0] += dx0;
                    result.point_mut(ip)[1] += dx1;

                    let xlensq = result.point(ip)[0].powi(2) + result.point(ip)[1].powi(2);
                    let vlensq = dx0 * dx0 + dx1 * dx1;
                    if vlensq < tol_sq * xlensq || xlensq == 0.0 {
                        converged[ip] = true;
                    }
                } else {
                    // General NxN: Gaussian elimination with partial pivoting.
                    let mut mat = jac_matrix[..ncoord * ncoord].to_vec();
                    let mut rhs = dy.clone();
                    let ok = solve_linear_system(&mut mat, &mut rhs, ncoord);
                    if !ok {
                        result.point_mut(ip).fill(BAD_VALUE);
                        converged[ip] = true;
                        continue;
                    }
                    let mut vlensq = 0.0;
                    let pt = result.point_mut(ip);
                    for (ic, &dx_ic) in rhs.iter().enumerate().take(ncoord) {
                        pt[ic] += dx_ic;
                        vlensq += dx_ic * dx_ic;
                    }
                    let mut xlensq = 0.0;
                    for &x in result.point(ip).iter().take(ncoord) {
                        xlensq += x * x;
                    }
                    if vlensq < tol_sq * xlensq || xlensq == 0.0 {
                        converged[ip] = true;
                    }
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Polynomial evaluation (power-table approach)
// ---------------------------------------------------------------------------

/// Fill the power table for one point's input values.
///
/// `power_table[axis][p] = input[axis]^p`, computed incrementally:
/// `table[0] = 1.0, table[1] = x, table[p] = table[p-1] * x`.
///
/// The power table must be pre-allocated with `power_table[axis].len() >= max_pows[axis] + 1`.
#[inline]
fn fill_power_table(input: &[f64], max_pows: &[usize], power_table: &mut [Vec<f64>]) {
    for (axis, table) in power_table.iter_mut().enumerate() {
        let x = input[axis];
        table[0] = 1.0;
        let mp = max_pows[axis];
        if mp >= 1 {
            table[1] = x;
            for p in 2..=mp {
                table[p] = table[p - 1] * x;
            }
        }
    }
}

/// Evaluate a single term using the pre-filled power table.
#[inline]
fn eval_term_with_table(term: &PolyTerm, power_table: &[Vec<f64>]) -> f64 {
    let mut prod = term.coeff;
    for (axis, &pow) in term.powers.iter().enumerate() {
        if pow > 0 {
            prod *= power_table[axis][pow];
        }
    }
    prod
}

/// Evaluate polynomial terms for a single input point using power-table lookup.
///
/// `terms` are the polynomial terms, `n_in` is the number of input axes,
/// `n_out` is the number of output axes, and `input` is the input point
/// (length `n_in`).
///
/// Returns a vector of `n_out` output values.
fn eval_poly(terms: &[PolyTerm], n_in: usize, n_out: usize, input: &[f64]) -> Vec<f64> {
    debug_assert_eq!(input.len(), n_in);

    // Compute max powers on the fly for standalone calls.
    let max_pows = compute_max_powers(terms, n_in);
    let mut power_table: Vec<Vec<f64>> = max_pows.iter().map(|&m| vec![0.0; m + 1]).collect();
    fill_power_table(input, &max_pows, &mut power_table);

    let mut output = vec![0.0; n_out];
    for term in terms {
        output[term.out_axis - 1] += eval_term_with_table(term, &power_table);
    }
    output
}

/// Evaluate polynomial terms for ALL points in a PointSet using power-table
/// lookup.
///
/// The power table is allocated once and reused for every point. For each
/// point, powers are computed incrementally (`x^p = x^(p-1) * x`), then
/// all terms are evaluated via table lookup — avoiding redundant `powi()`
/// calls across terms that share the same input axis powers.
///
/// Points whose corresponding `skip` entry is true are left unchanged in
/// the output.
#[allow(dead_code)]
fn eval_poly_into(
    terms: &[PolyTerm],
    n_in: usize,
    n_out: usize,
    input: &PointSet,
    output: &mut PointSet,
    skip: &[bool],
) {
    let npoint = input.npoint();
    debug_assert_eq!(output.npoint(), npoint);
    debug_assert_eq!(input.ncoord(), n_in);
    debug_assert_eq!(output.ncoord(), n_out);
    debug_assert_eq!(skip.len(), npoint);

    // Pre-compute max powers and term grouping.
    let max_pows = compute_max_powers(terms, n_in);
    let terms_by_output = compute_terms_by_output(terms, n_out);

    // Allocate power table once, reuse for each point.
    let mut power_table: Vec<Vec<f64>> = max_pows.iter().map(|&m| vec![0.0; m + 1]).collect();

    #[allow(clippy::needless_range_loop)]
    for ip in 0..npoint {
        if skip[ip] {
            continue;
        }
        let inp = input.point(ip);
        fill_power_table(inp, &max_pows, &mut power_table);

        let outp = output.point_mut(ip);
        for (out_idx, term_indices) in terms_by_output.iter().enumerate() {
            let mut sum = 0.0;
            for &idx in term_indices {
                sum += eval_term_with_table(&terms[idx], &power_table);
            }
            outp[out_idx] = sum;
        }
    }
}

/// Compute Jacobian polynomial terms from a set of forward terms.
///
/// Returns `Vec<Vec<PolyTerm>>` indexed by input axis `j` (0-based).
/// Each inner `Vec<PolyTerm>` contains terms representing
/// `d(forward_i)/d(x_j)` for all output axes `i`.
fn compute_jacobian_terms_from(forward: &[PolyTerm], nin: usize) -> Vec<Vec<PolyTerm>> {
    let mut jac_terms = Vec::with_capacity(nin);
    for j in 0..nin {
        let mut col_terms = Vec::new();
        for term in forward {
            let pj = term.powers[j];
            if pj == 0 {
                continue;
            }
            let new_coeff = term.coeff * (pj as f64);
            let mut new_powers = term.powers.clone();
            new_powers[j] = pj - 1;
            col_terms.push(PolyTerm {
                coeff: new_coeff,
                powers: new_powers,
                out_axis: term.out_axis,
            });
        }
        jac_terms.push(col_terms);
    }
    jac_terms
}

// ---------------------------------------------------------------------------
// General NxN linear solve (Gaussian elimination, partial pivoting)
// ---------------------------------------------------------------------------

/// Solve `A * x = b` in-place using Gaussian elimination with partial
/// pivoting. `mat` is row-major `n x n`, `rhs` has length `n`.
/// On success the solution is written into `rhs` and returns `true`.
/// Returns `false` if the matrix is singular.
fn solve_linear_system(mat: &mut [f64], rhs: &mut [f64], n: usize) -> bool {
    // Forward elimination.
    for col in 0..n {
        // Partial pivoting: find the row with the largest absolute value
        // in the current column.
        let mut max_val = mat[col * n + col].abs();
        let mut max_row = col;
        for row in (col + 1)..n {
            let v = mat[row * n + col].abs();
            if v > max_val {
                max_val = v;
                max_row = row;
            }
        }
        if max_val < 1e-30 {
            return false; // Singular.
        }
        // Swap rows if needed.
        if max_row != col {
            for k in 0..n {
                mat.swap(col * n + k, max_row * n + k);
            }
            rhs.swap(col, max_row);
        }
        // Eliminate below.
        let pivot = mat[col * n + col];
        for row in (col + 1)..n {
            let factor = mat[row * n + col] / pivot;
            for k in (col + 1)..n {
                mat[row * n + k] -= factor * mat[col * n + k];
            }
            mat[row * n + col] = 0.0;
            rhs[row] -= factor * rhs[col];
        }
    }
    // Back substitution.
    for col in (0..n).rev() {
        let mut sum = rhs[col];
        for k in (col + 1)..n {
            sum -= mat[col * n + k] * rhs[k];
        }
        rhs[col] = sum / mat[col * n + col];
    }
    true
}

// ---------------------------------------------------------------------------
// Levenberg-Marquardt iterative inverse (legacy, kept for reference)
// ---------------------------------------------------------------------------

/// Problem struct for the LM solver. Solves: find `x` such that
/// `forward(x) = target`.
#[allow(dead_code)]
struct PolyInverseProblem<'a> {
    /// Current parameter estimate (the unknown input coordinates).
    params: DVector<f64>,
    /// The target output we want to invert.
    target: &'a [f64],
    /// Forward polynomial terms.
    terms: &'a [PolyTerm],
    /// Number of input axes.
    nin: usize,
    /// Number of output axes.
    nout: usize,
}

impl<'a> LeastSquaresProblem<f64, Dyn, Dyn> for PolyInverseProblem<'a> {
    type ParameterStorage = VecStorage<f64, Dyn, nalgebra::U1>;
    type ResidualStorage = VecStorage<f64, Dyn, nalgebra::U1>;
    type JacobianStorage = VecStorage<f64, Dyn, Dyn>;

    fn set_params(&mut self, p: &OVector<f64, Dyn>) {
        self.params.copy_from(p);
    }

    fn params(&self) -> OVector<f64, Dyn> {
        self.params.clone()
    }

    fn residuals(&self) -> Option<OVector<f64, Dyn>> {
        let x: Vec<f64> = self.params.iter().copied().collect();
        let fwd = eval_poly(self.terms, self.nin, self.nout, &x);
        let mut residuals = DVector::zeros(self.nout);
        for i in 0..self.nout {
            residuals[i] = fwd[i] - self.target[i];
        }
        Some(residuals)
    }

    fn jacobian(&self) -> Option<OMatrix<f64, Dyn, Dyn>> {
        let x: Vec<f64> = self.params.iter().copied().collect();
        let mut jac = DMatrix::zeros(self.nout, self.nin);

        // For each term, compute d(term)/d(x_j) for each input axis j,
        // and add to the appropriate row (output axis).
        for term in self.terms {
            let out_idx = term.out_axis - 1;
            for j in 0..self.nin {
                if term.powers[j] == 0 {
                    // Derivative with respect to x_j is zero if power is 0.
                    continue;
                }
                // d(term)/d(x_j) = coeff * powers[j] * x_j^(powers[j]-1)
                //                   * product(x_k^powers[k] for k != j)
                let mut deriv = term.coeff * (term.powers[j] as f64);
                for (k, (&xk, &pk)) in x.iter().zip(term.powers.iter()).enumerate() {
                    if k == j {
                        deriv *= xk.powi(pk as i32 - 1);
                    } else {
                        deriv *= xk.powi(pk as i32);
                    }
                }
                jac[(out_idx, j)] += deriv;
            }
        }
        Some(jac)
    }
}

/// Attempt to invert a single point using Levenberg-Marquardt.
#[allow(dead_code)]
fn lm_inverse(
    terms: &[PolyTerm],
    nin: usize,
    nout: usize,
    target: &[f64],
    niter: usize,
    tol: f64,
) -> Vec<f64> {
    // Initial guess: use target values, padded or truncated.
    let mut x0 = DVector::zeros(nin);
    for i in 0..nin {
        if i < nout {
            x0[i] = target[i];
        }
    }

    let problem = PolyInverseProblem {
        params: x0,
        target,
        terms,
        nin,
        nout,
    };

    let (result, report) = LevenbergMarquardt::new()
        .with_patience(niter)
        .minimize(problem);

    // Check convergence: if the residual norm is small enough, return.
    if report.termination.was_successful() || report.objective_function.sqrt() < tol {
        let solution: Vec<f64> = result.params.iter().copied().collect();
        // Final check: compute residual norm.
        let fwd = eval_poly(terms, nin, nout, &solution);
        let residual_sq: f64 = fwd
            .iter()
            .zip(target.iter())
            .map(|(f, t)| (f - t).powi(2))
            .sum();
        if residual_sq.sqrt() < tol {
            return solution;
        }
    }

    vec![BAD_VALUE; nin]
}

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for PolyMap {
    fn nin(&self) -> usize {
        if self.inverted { self.nout } else { self.nin }
    }

    fn nout(&self) -> usize {
        if self.inverted { self.nin } else { self.nout }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let n_out = if self.effective_forward(forward) {
            self.nout
        } else {
            self.nin
        };
        let mut out = PointSet::zeros(pts.npoint(), n_out);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        if fwd {
            // Forward transform using pre-computed power-table indices.
            let mut power_table: Vec<Vec<f64>> = self
                .forward_max_pows
                .iter()
                .map(|&m| vec![0.0; m + 1])
                .collect();
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if point_has_bad(inp) {
                    outp.fill(BAD_VALUE);
                } else {
                    fill_power_table(inp, &self.forward_max_pows, &mut power_table);
                    for (out_idx, term_indices) in self.forward_by_output.iter().enumerate() {
                        let mut sum = 0.0;
                        for &idx in term_indices {
                            sum += eval_term_with_table(&self.forward[idx], &power_table);
                        }
                        outp[out_idx] = sum;
                    }
                }
            }
        } else {
            // Inverse transform.
            if let Some(ref inv_terms) = self.inverse {
                // Explicit inverse terms: the inverse maps from nout-space
                // to nin-space. Input powers have length nout, output goes
                // to nin axes.
                let inv_by_output = self.inverse_by_output.as_ref().unwrap();
                let mut power_table: Vec<Vec<f64>> = self
                    .inverse_max_pows
                    .iter()
                    .map(|&m| vec![0.0; m + 1])
                    .collect();
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    if point_has_bad(inp) {
                        outp.fill(BAD_VALUE);
                    } else {
                        fill_power_table(inp, &self.inverse_max_pows, &mut power_table);
                        for (out_idx, term_indices) in inv_by_output.iter().enumerate() {
                            let mut sum = 0.0;
                            for &idx in term_indices {
                                sum += eval_term_with_table(&inv_terms[idx], &power_table);
                            }
                            outp[out_idx] = sum;
                        }
                    }
                }
            } else if self.iter_inverse {
                // Bulk Newton-Raphson iterative inverse.
                self.newton_raphson_inverse(pts, out)?;
            } else {
                // No inverse available: fill with BAD.
                for i in 0..pts.npoint() {
                    out.point_mut(i).fill(BAD_VALUE);
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        // Determine which terms are the "forward" set in the current orientation.
        let (fwd_terms, inv_terms) = if self.inverted {
            (
                self.inverse.as_deref().unwrap_or(&[]),
                Some(self.forward.as_slice()),
            )
        } else {
            (self.forward.as_slice(), self.inverse.as_deref())
        };

        // Consolidate forward terms: merge duplicates with same (powers, out_axis).
        let mut consolidated: Vec<PolyTerm> = Vec::new();
        for term in fwd_terms {
            if let Some(existing) = consolidated
                .iter_mut()
                .find(|t| t.out_axis == term.out_axis && t.powers == term.powers)
            {
                existing.coeff += term.coeff;
            } else {
                consolidated.push(term.clone());
            }
        }
        // Remove zero-coefficient terms.
        consolidated.retain(|t| t.coeff.abs() > 1e-30);

        // If nothing changed, return self unchanged.
        if consolidated.len() == fwd_terms.len() {
            return Ok(Box::new(self.clone()));
        }

        // Build a new PolyMap with the consolidated terms.
        // We need to map back to the non-inverted representation for the constructor.
        let (new_fwd, new_inv) = if self.inverted {
            (inv_terms.unwrap_or(&[]), Some(consolidated.as_slice()))
        } else {
            (consolidated.as_slice(), inv_terms)
        };

        let mut pm = PolyMap::new(self.nin, self.nout, new_fwd, new_inv)?;

        // Preserve attributes.
        if self.iter_inverse {
            pm.set_iter_inverse(true);
            pm.set_niter_inverse(self.niter_inverse);
            pm.set_tol_inverse(self.tol_inverse);
        }
        if self.inverted {
            pm.invert();
        }

        Ok(Box::new(pm))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }
        // Only handle inverse cancellation: same PolyMap in opposite directions.
        if self_inv == other_inv {
            return None;
        }
        let other_poly = other.as_any().downcast_ref::<PolyMap>()?;
        if self.nin == other_poly.nin
            && self.nout == other_poly.nout
            && self.forward.len() == other_poly.forward.len()
            && self.forward.iter().zip(&other_poly.forward).all(|(a, b)| {
                ast_equal(a.coeff, b.coeff) && a.powers == b.powers && a.out_axis == b.out_axis
            })
        {
            // Input dimension of the pair depends on which direction
            // self is being used in.
            let pair_nin = if self_inv { self.nout } else { self.nin };
            Some(Box::new(UnitMap::new(pair_nin)))
        } else {
            None
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<PolyMap>() else {
            return false;
        };
        if self.nin != other.nin || self.nout != other.nout {
            return false;
        }
        if self.inverted != other.inverted {
            return false;
        }
        // Compare forward terms.
        if self.forward.len() != other.forward.len() {
            return false;
        }
        for (a, b) in self.forward.iter().zip(&other.forward) {
            if !ast_equal(a.coeff, b.coeff) || a.powers != b.powers || a.out_axis != b.out_axis {
                return false;
            }
        }
        // Compare inverse terms.
        match (&self.inverse, &other.inverse) {
            (Some(a), Some(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                a.iter().zip(b).all(|(x, y)| {
                    ast_equal(x.coeff, y.coeff) && x.powers == y.powers && x.out_axis == y.out_axis
                })
            }
            (None, None) => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Serializable
// ---------------------------------------------------------------------------

impl Serializable for PolyMap {
    fn class_name(&self) -> &'static str {
        "PolyMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Polynomial transformation".to_string());
        write_mapping_base(&mut node, self);

        let nin = self.nin;
        let nout = self.nout;

        // Forward transformation.
        if !self.forward.is_empty() {
            // Compute max power of each input axis across all forward terms.
            let mut mxpow_f = vec![0usize; nin];
            for term in &self.forward {
                for (i, &p) in term.powers.iter().enumerate() {
                    if p > mxpow_f[i] {
                        mxpow_f[i] = p;
                    }
                }
            }
            for (i, &mp) in mxpow_f.iter().enumerate() {
                node.push_int(
                    &format!("MPF{}", i + 1),
                    mp as i64,
                    Some(format!(
                        "Max. power of input {} in any forward polynomial",
                        i + 1
                    )),
                );
            }

            // Group terms by output axis.
            let mut terms_by_out: Vec<Vec<&PolyTerm>> = vec![Vec::new(); nout];
            for term in &self.forward {
                terms_by_out[term.out_axis - 1].push(term);
            }

            // Write number of coefficients per output.
            for (i, terms) in terms_by_out.iter().enumerate() {
                node.push_int(
                    &format!("NCF{}", i + 1),
                    terms.len() as i64,
                    Some(format!("No. of coeff.s for forward polynomial {}", i + 1)),
                );
            }

            // Write coefficient values, sequentially across all outputs.
            let mut iv = 0usize;
            for (i, terms) in terms_by_out.iter().enumerate() {
                for (j, term) in terms.iter().enumerate() {
                    iv += 1;
                    if term.coeff != BAD_VALUE {
                        node.push_double(
                            &format!("CF{iv}"),
                            term.coeff,
                            Some(format!("Coeff {} of forward polynomial {}", j + 1, i + 1)),
                        );
                    }
                }
            }

            // Write powers, sequentially: for each output, for each term,
            // for each input axis.
            iv = 0;
            for terms in &terms_by_out {
                for term in terms {
                    for (k, &p) in term.powers.iter().enumerate() {
                        iv += 1;
                        if p > 0 {
                            node.push_double(
                                &format!("PF{iv}"),
                                p as f64,
                                Some(format!("Power of i/p {} for coeff of fwd poly", k + 1)),
                            );
                        }
                    }
                }
            }
        }

        // Inverse transformation.
        if let Some(ref inv_terms) = self.inverse {
            if !inv_terms.is_empty() {
                // Compute max power of each output axis (inverse input).
                let mut mxpow_i = vec![0usize; nout];
                for term in inv_terms {
                    for (i, &p) in term.powers.iter().enumerate() {
                        if p > mxpow_i[i] {
                            mxpow_i[i] = p;
                        }
                    }
                }
                for (i, &mp) in mxpow_i.iter().enumerate() {
                    node.push_int(
                        &format!("MPI{}", i + 1),
                        mp as i64,
                        Some(format!(
                            "Max. power of output {} in any inverse polynomial",
                            i + 1
                        )),
                    );
                }

                // Group inverse terms by their output axis (which maps
                // to input axes of the PolyMap).
                let mut terms_by_out: Vec<Vec<&PolyTerm>> = vec![Vec::new(); nin];
                for term in inv_terms {
                    terms_by_out[term.out_axis - 1].push(term);
                }

                // Write number of coefficients per output.
                for (i, terms) in terms_by_out.iter().enumerate() {
                    node.push_int(
                        &format!("NCI{}", i + 1),
                        terms.len() as i64,
                        Some(format!("No. of coeff.s for inverse polynomial {}", i + 1)),
                    );
                }

                // Write coefficient values.
                let mut iv = 0usize;
                for (i, terms) in terms_by_out.iter().enumerate() {
                    for (j, term) in terms.iter().enumerate() {
                        iv += 1;
                        if term.coeff != BAD_VALUE {
                            node.push_double(
                                &format!("CI{iv}"),
                                term.coeff,
                                Some(format!("Coeff {} of inverse polynomial {}", j + 1, i + 1)),
                            );
                        }
                    }
                }

                // Write powers.
                iv = 0;
                for terms in &terms_by_out {
                    for term in terms {
                        for (k, &p) in term.powers.iter().enumerate() {
                            iv += 1;
                            if p > 0 {
                                node.push_double(
                                    &format!("PI{iv}"),
                                    p as f64,
                                    Some(format!("Power of o/p {} for coeff of inv poly", k + 1)),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Iterative inverse attributes.
        if self.iter_inverse {
            node.push_int("IterInv", 1, Some("Use an iterative inverse".to_string()));
        }
        if self.niter_inverse != 4 {
            node.push_int(
                "NiterInv",
                self.niter_inverse as i64,
                Some("Max number of iterations for iterative inverse".to_string()),
            );
        }
        if self.tol_inverse != 1e-6 {
            node.push_double(
                "TolInv",
                self.tol_inverse,
                Some("Target relative error for iterative inverse".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (nin, nout, inverted) = read_mapping_base(node);

        // --- Forward transformation ---
        let mut forward = Vec::new();

        // Check if forward terms exist by looking for MPF1.
        let has_forward = node.get("MPF1").is_some();
        if has_forward {
            // Read max powers (we don't need them for reconstruction,
            // but they confirm the format).
            // Read number of coefficients per output.
            let mut ncoeff_f = Vec::with_capacity(nout);
            for i in 0..nout {
                let nc = node.get_int(&format!("NCF{}", i + 1), 0) as usize;
                ncoeff_f.push(nc);
            }

            // Read coefficient values (sequentially indexed).
            let total_coeffs: usize = ncoeff_f.iter().sum();
            let mut coeffs = Vec::with_capacity(total_coeffs);
            for iv in 1..=total_coeffs {
                coeffs.push(node.get_double(&format!("CF{iv}"), BAD_VALUE));
            }

            // Read powers (sequentially indexed: for each output, for each
            // term, for each input axis).
            let total_powers: usize = total_coeffs * nin;
            let mut powers = Vec::with_capacity(total_powers);
            for iv in 1..=total_powers {
                powers.push(node.get_double(&format!("PF{iv}"), 0.0) as usize);
            }

            // Reconstruct PolyTerm list.
            let mut coeff_idx = 0;
            let mut power_idx = 0;
            for (out_ax, &nc) in ncoeff_f.iter().enumerate() {
                for _ in 0..nc {
                    let coeff = coeffs[coeff_idx];
                    coeff_idx += 1;
                    let term_powers: Vec<usize> = powers[power_idx..power_idx + nin].to_vec();
                    power_idx += nin;
                    // Skip BAD coefficients (they represent absent terms).
                    if coeff != BAD_VALUE && coeff != 0.0 {
                        forward.push(PolyTerm {
                            coeff,
                            powers: term_powers,
                            out_axis: out_ax + 1,
                        });
                    }
                }
            }
        }

        // --- Inverse transformation ---
        let mut inverse_terms = Vec::new();
        let has_inverse = node.get("MPI1").is_some();
        if has_inverse {
            let mut ncoeff_i = Vec::with_capacity(nin);
            for i in 0..nin {
                let nc = node.get_int(&format!("NCI{}", i + 1), 0) as usize;
                ncoeff_i.push(nc);
            }

            let total_coeffs: usize = ncoeff_i.iter().sum();
            let mut coeffs = Vec::with_capacity(total_coeffs);
            for iv in 1..=total_coeffs {
                coeffs.push(node.get_double(&format!("CI{iv}"), BAD_VALUE));
            }

            let total_powers: usize = total_coeffs * nout;
            let mut powers = Vec::with_capacity(total_powers);
            for iv in 1..=total_powers {
                powers.push(node.get_double(&format!("PI{iv}"), 0.0) as usize);
            }

            let mut coeff_idx = 0;
            let mut power_idx = 0;
            for (out_ax, &nc) in ncoeff_i.iter().enumerate() {
                for _ in 0..nc {
                    let coeff = coeffs[coeff_idx];
                    coeff_idx += 1;
                    let term_powers: Vec<usize> = powers[power_idx..power_idx + nout].to_vec();
                    power_idx += nout;
                    if coeff != BAD_VALUE && coeff != 0.0 {
                        inverse_terms.push(PolyTerm {
                            coeff,
                            powers: term_powers,
                            out_axis: out_ax + 1,
                        });
                    }
                }
            }
        }

        let inverse = if has_inverse {
            Some(inverse_terms)
        } else {
            None
        };

        // Read iterative inverse attributes.
        let iter_inv = node.get_int("IterInv", 0) != 0;
        let niter_inv = {
            let v = node.get_int("NiterInv", 4);
            if v <= 0 { 4 } else { v as usize }
        };
        let tol_inv = {
            let v = node.get_double("TolInv", 1e-6);
            if v == BAD_VALUE { 1e-6 } else { v }
        };

        // Build directly rather than using `new()` to avoid re-validation
        // of terms that may include BAD_VALUE sentinel coefficients from the
        // serialised format.
        let jacobian_terms = if iter_inv && nin == nout {
            Some(compute_jacobian_terms_from(&forward, nin))
        } else {
            None
        };
        let forward_by_output = compute_terms_by_output(&forward, nout);
        let forward_max_pows = compute_max_powers(&forward, nin);
        let (inverse_by_output, inverse_max_pows) = if let Some(ref inv) = inverse {
            (
                Some(compute_terms_by_output(inv, nin)),
                compute_max_powers(inv, nout),
            )
        } else {
            (None, Vec::new())
        };
        let mut m = PolyMap {
            nin,
            nout,
            forward,
            inverse,
            iter_inverse: iter_inv,
            niter_inverse: niter_inv,
            tol_inverse: tol_inv,
            inverted: false,
            jacobian_terms,
            forward_by_output,
            forward_max_pows,
            inverse_by_output,
            inverse_max_pows,
        };
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}

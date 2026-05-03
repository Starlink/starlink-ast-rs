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
/// inverse can be provided, or an iterative Levenberg-Marquardt inverse
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

        Ok(Self {
            nin,
            nout,
            forward: forward.to_vec(),
            inverse: inv,
            iter_inverse: false,
            niter_inverse: 4,
            tol_inverse: 1e-6,
            inverted: false,
        })
    }

    /// Whether iterative (Levenberg-Marquardt) inverse is enabled.
    pub fn iter_inverse(&self) -> bool {
        self.iter_inverse
    }

    /// Enable or disable the iterative inverse.
    pub fn set_iter_inverse(&mut self, v: bool) {
        self.iter_inverse = v;
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
}

// ---------------------------------------------------------------------------
// Polynomial evaluation
// ---------------------------------------------------------------------------

/// Evaluate polynomial terms for a single input point.
///
/// `terms` are the polynomial terms, `n_in` is the number of input axes,
/// `n_out` is the number of output axes, and `input` is the input point
/// (length `n_in`).
///
/// Returns a vector of `n_out` output values.
fn eval_poly(terms: &[PolyTerm], n_in: usize, n_out: usize, input: &[f64]) -> Vec<f64> {
    debug_assert_eq!(input.len(), n_in);

    let mut output = vec![0.0; n_out];
    for term in terms {
        debug_assert_eq!(term.powers.len(), n_in);
        let mut val = term.coeff;
        for (&inp, &pow) in input.iter().zip(term.powers.iter()) {
            val *= inp.powi(pow as i32);
        }
        output[term.out_axis - 1] += val;
    }
    output
}

// ---------------------------------------------------------------------------
// Levenberg-Marquardt iterative inverse
// ---------------------------------------------------------------------------

/// Problem struct for the LM solver. Solves: find `x` such that
/// `forward(x) = target`.
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
            // Forward transform using forward terms.
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if point_has_bad(inp) {
                    outp.fill(BAD_VALUE);
                } else {
                    let result = eval_poly(&self.forward, self.nin, self.nout, inp);
                    outp.copy_from_slice(&result);
                }
            }
        } else {
            // Inverse transform.
            if let Some(ref inv_terms) = self.inverse {
                // Explicit inverse terms: the inverse maps from nout-space
                // to nin-space. Input powers have length nout, output goes
                // to nin axes.
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    if point_has_bad(inp) {
                        outp.fill(BAD_VALUE);
                    } else {
                        let result = eval_poly(inv_terms, self.nout, self.nin, inp);
                        outp.copy_from_slice(&result);
                    }
                }
            } else if self.iter_inverse {
                // LM iterative inverse.
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    if point_has_bad(inp) {
                        outp.fill(BAD_VALUE);
                    } else {
                        let result = lm_inverse(
                            &self.forward,
                            self.nin,
                            self.nout,
                            inp,
                            self.niter_inverse,
                            self.tol_inverse,
                        );
                        outp.copy_from_slice(&result);
                    }
                }
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
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
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
        let mut m = PolyMap {
            nin,
            nout,
            forward,
            inverse,
            iter_inverse: iter_inv,
            niter_inverse: niter_inv,
            tol_inverse: tol_inv,
            inverted: false,
        };
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}

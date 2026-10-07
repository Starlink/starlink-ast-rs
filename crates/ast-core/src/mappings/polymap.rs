//! N-dimensional polynomial transformation mapping.

use levenberg_marquardt::{LeastSquaresProblem, LevenbergMarquardt};
use nalgebra::{DMatrix, DVector, Dyn, OMatrix, OVector, VecStorage};

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, point_has_bad};
use super::matrix::MatrixMap;
use super::rate::numeric_rate;
use super::shift::ShiftMap;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

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
    /// IterInverse attribute. `None` = unset; the effective default is
    /// "no inverse coefficients supplied" (polymap.c:6311).
    iter_inverse: Option<bool>,
    niter_inverse: usize,
    tol_inverse: f64,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The `Report` attribute (mapping.c), `None` when unset.
    report: Option<bool>,
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

        let mut pm = Self {
            nin,
            nout,
            forward: fwd_vec,
            inverse: inv,
            iter_inverse: None,
            niter_inverse: 4,
            tol_inverse: 1e-6,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            report: None,
            jacobian_terms: None,
            forward_by_output,
            forward_max_pows,
            inverse_by_output,
            inverse_max_pows,
        };
        // The IterInverse default may enable the iterative inverse
        // immediately, so pre-build the Jacobian cache it relies on.
        if pm.nin == pm.nout && pm.iter_inverse() {
            pm.jacobian_terms = Some(pm.compute_jacobian_terms());
        }
        Ok(pm)
    }

    /// Whether the iterative (Newton-Raphson) inverse is enabled.
    ///
    /// When unset, defaults to "no inverse coefficients were supplied"
    /// — mirrors C `GetIterInverse` (polymap.c:6311), which makes a
    /// forward-only PolyMap invertible out of the box.
    pub fn iter_inverse(&self) -> bool {
        self.iter_inverse
            .unwrap_or_else(|| self.inverse.as_ref().is_none_or(|v| v.is_empty()))
    }

    /// Enable or disable the iterative inverse.
    pub fn set_iter_inverse(&mut self, v: bool) {
        // `IterInverse` decides whether the inverse transformation is defined
        // at all, so it changes what a simplification of this PolyMap could
        // conclude. PolyMap's own MapMerge does not read the attribute, but C
        // discards the records on every setter it covers, because doing so costs
        // only a redundant simplification (polymap.c:6340-6347, the convention at
        // mapping.h:449-472). The clear is guarded on the value differing, as C's
        // is -- see `LutMap::set_interp` for why that guard matters to the
        // loader.
        if Some(v) != self.iter_inverse {
            self.simplified.clear();
        }
        self.iter_inverse = Some(v);
        if v && self.nin == self.nout && self.jacobian_terms.is_none() {
            self.jacobian_terms = Some(self.compute_jacobian_terms());
        } else if !v {
            self.jacobian_terms = None;
        }
    }

    /// Whether IterInverse has been explicitly set. Mirrors C
    /// `astTestIterInverse`.
    pub fn test_iter_inverse(&self) -> bool {
        self.iter_inverse.is_some()
    }

    /// Clear IterInverse back to its default. Mirrors C
    /// `astClearIterInverse`.
    pub fn clear_iter_inverse(&mut self) {
        // As for `set_iter_inverse` (polymap.c:6340).
        self.simplified.clear();
        self.iter_inverse = None;
        if self.nin == self.nout && self.iter_inverse() {
            if self.jacobian_terms.is_none() {
                self.jacobian_terms = Some(self.compute_jacobian_terms());
            }
        } else {
            self.jacobian_terms = None;
        }
    }

    /// Maximum number of iterations for the iterative inverse.
    pub fn niter_inverse(&self) -> usize {
        self.niter_inverse
    }

    /// Set the maximum number of iterations for the iterative inverse.
    pub fn set_niter_inverse(&mut self, v: usize) {
        // `NiterInverse` changes what the iterative inverse computes
        // (polymap.c:6383-6385).
        if v != self.niter_inverse {
            self.simplified.clear();
        }
        self.niter_inverse = v;
    }

    /// Target relative error for the iterative inverse.
    pub fn tol_inverse(&self) -> f64 {
        self.tol_inverse
    }

    /// Set the target relative error for the iterative inverse.
    pub fn set_tol_inverse(&mut self, v: f64) {
        // `TolInverse` changes what the iterative inverse computes
        // (polymap.c:6420-6422).
        if v != self.tol_inverse {
            self.simplified.clear();
        }
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

    /// Whether the original (uninverted) forward transformation is
    /// defined, i.e. forward coefficients were supplied. Mirrors C
    /// `GetTranForward` (polymap.c:2222).
    fn forward_defined(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Whether the original (uninverted) inverse transformation is
    /// defined: inverse coefficients were supplied or the iterative
    /// inverse is enabled. Mirrors C `GetTranInverse` (polymap.c:2285).
    fn inverse_defined(&self) -> bool {
        self.inverse.as_ref().is_some_and(|v| !v.is_empty()) || self.iter_inverse()
    }

    /// Fit a replacement forward or inverse transformation by least
    /// squares, returning a new PolyMap that uses the fit.
    ///
    /// Mirrors `astPolyTran` (polymap.c:4746 / ReplaceTransformation):
    /// the *other* transformation is sampled on a regular grid of
    /// `2*order` points per axis over the rectangle `[lbnd, ubnd]`
    /// (in the replaced transformation's input space), and polynomials
    /// of increasing order are fitted until the RMS residual is below
    /// `acc` or `maxorder` is reached; the max-order fit is accepted
    /// if its RMS residual is below `maxacc`.
    ///
    /// `forward` selects the (effective-orientation) transformation to
    /// replace. `maxorder` is one more than the maximum power, so 3
    /// allows a quadratic; values below 2 are treated as 2.
    ///
    /// Returns `Ok(None)` — without an error — when no fit achieves
    /// `maxacc`. The returned PolyMap always has `IterInverse`
    /// cleared, so the new fit is used in preference to any iterative
    /// inverse. Only 1- and 2-dimensional PolyMaps with equal input
    /// and output counts are supported, and the transformation being
    /// sampled must have explicit coefficients (an iterative inverse
    /// cannot be sampled).
    pub fn poly_tran(
        &self,
        forward: bool,
        acc: f64,
        maxacc: f64,
        maxorder: usize,
        lbnd: &[f64],
        ubnd: &[f64],
    ) -> AstResult<Option<PolyMap>> {
        use crate::mappings::Mapping;

        // Dimensional checks (polymap.c:5132-5143).
        let ndim = self.nin();
        if self.nout() != ndim {
            return Err(AstError::new(
                ErrorCode::Badni,
                format!(
                    "astPolyTran(PolyMap): supplied PolyMap has different number of inputs \
                     ({ndim}) and outputs ({})",
                    self.nout()
                ),
            ));
        }
        if ndim > 2 {
            return Err(AstError::new(
                ErrorCode::Badni,
                format!(
                    "astPolyTran(PolyMap): supplied PolyMap has too many inputs and outputs \
                     ({ndim}) - must be 1 or 2"
                ),
            ));
        }

        // The transformation to be sampled must have explicit
        // coefficients (polymap.c:5145-5158): C tests ncoeff_f/ncoeff_i
        // directly, so an iterative inverse does not qualify.
        if forward == self.inverted {
            if self.forward.is_empty() {
                return Err(AstError::new(
                    ErrorCode::Nodef,
                    "astPolyTran(PolyMap): supplied PolyMap has no forward transformation",
                ));
            }
        } else if self.inverse.as_ref().is_none_or(|v| v.is_empty()) {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "astPolyTran(PolyMap): supplied PolyMap has no inverse transformation",
            ));
        }

        let Some(new_terms) = crate::mappings::helpers::poly_tran::replace_transformation(
            self, forward, acc, maxacc, maxorder, lbnd, ubnd,
        )?
        else {
            return Ok(None);
        };

        // Store the fitted terms in the original-orientation slot for
        // the replaced transformation (polymap.c StoreArrays swaps when
        // the PolyMap is inverted).
        let replace_original_forward = forward != self.inverted;
        let mut pm = if replace_original_forward {
            PolyMap::new(self.nin, self.nout, &new_terms, self.inverse.as_deref())?
        } else {
            PolyMap::new(self.nin, self.nout, &self.forward, Some(&new_terms))?
        };

        // Carry over attributes as astCopy would, except IterInverse,
        // which C clears so the new fit takes priority. The simplified record
        // is not carried over either: one of the transformations has been
        // replaced, so whatever was true of the original is not true of this
        // (C `astClearIsSimple( result )`, polymap.c:5352).
        pm.niter_inverse = self.niter_inverse;
        pm.tol_inverse = self.tol_inverse;
        if self.inverted {
            pm.invert();
        }
        Ok(Some(pm))
    }

    /// Gather forward terms per output axis for the 2D SIMD prototype.
    ///
    /// Returns `(out0_terms, out1_terms, [max_p0, max_p1])`. Only valid when
    /// `nin == nout == 2`; the caller guards on shape before invoking this.
    /// References point into `self.forward`; the returned `Vec<&PolyTerm>`s
    /// own only the indirection table, not the term bodies.
    #[cfg(feature = "simd-eval")]
    pub(crate) fn simd_forward_layout_2d(&self) -> (Vec<&PolyTerm>, Vec<&PolyTerm>, [usize; 2]) {
        debug_assert_eq!(self.nin, 2);
        debug_assert_eq!(self.nout, 2);
        let out0: Vec<&PolyTerm> = self.forward_by_output[0]
            .iter()
            .map(|&i| &self.forward[i])
            .collect();
        let out1: Vec<&PolyTerm> = self.forward_by_output[1]
            .iter()
            .map(|&i| &self.forward[i])
            .collect();
        (
            out0,
            out1,
            [self.forward_max_pows[0], self.forward_max_pows[1]],
        )
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
    ///
    /// `bad_sentinel` is the value written to outputs that fail (input
    /// flagged as invalid up-front, singular Jacobian mid-iteration).
    /// Callers from the BAD_VALUE world pass `BAD_VALUE`; callers in the
    /// NaN-as-bad protocol pass `f64::NAN` so downstream stages receive
    /// the protocol-canonical sentinel without an extra post-pass. Pre-
    /// marking uses [`point_has_bad`], which folds both `BAD_VALUE` and
    /// `!is_finite()` (NaN, ±∞) into the same "skip this point" decision,
    /// so the NaN-protocol caller gets the same iteration-skipping the
    /// BAD caller does.
    fn newton_raphson_inverse(
        &self,
        target: &PointSet,
        result: &mut PointSet,
        bad_sentinel: f64,
    ) -> AstResult<()> {
        let npoint = target.npoint();
        let ncoord = self.nin; // nin == nout for iterative inverse
        debug_assert_eq!(self.nin, self.nout);

        // Use cached Jacobian terms.
        let jac_terms = self.get_jacobian_terms();

        let max_iter = self.niter_inverse;
        let tol_sq = self.tol_inverse * self.tol_inverse;
        let mut converged = vec![false; npoint];

        // Mark invalid-input points as already converged and fill their
        // output with the sentinel. `point_has_bad` catches both BAD_VALUE
        // and NaN, so this works for both the BAD caller and the NaN-
        // protocol caller.
        #[allow(clippy::needless_range_loop)]
        for i in 0..npoint {
            if point_has_bad(target.point(i)) {
                converged[i] = true;
                result.point_mut(i).fill(bad_sentinel);
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

        // Bulk-flat slice access: walking the underlying contiguous buffer
        // skips per-point ndarray row-view + as_slice + expect machinery
        // that `point_mut(ip)` performs. With ~50k per-point-iter calls on
        // the LSST bench, removing that dispatch is measurable.
        let target_view = target.view();
        let target_flat: &[f64] = target_view.to_slice().expect("PointSet is C-contiguous");
        let mut result_view = result.view_mut();
        let result_flat: &mut [f64] = result_view
            .as_slice_mut()
            .expect("PointSet is C-contiguous");

        for _iter in 0..max_iter {
            if converged.iter().all(|&c| c) {
                break;
            }

            #[allow(clippy::needless_range_loop)]
            for ip in 0..npoint {
                if converged[ip] {
                    continue;
                }

                let base = ip * ncoord;
                let pt = &mut result_flat[base..base + ncoord];

                // Fill power table once for this point (pt reborrows as &[f64]).
                fill_power_table(pt, &nr_max_pows, &mut power_table);

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
                let tgt = &target_flat[base..base + ncoord];
                for ic in 0..ncoord {
                    dy[ic] = tgt[ic] - fwd_out[ic];
                }

                // Solve J * dx = dy.
                if ncoord == 1 {
                    let j00 = jac_matrix[0];
                    if j00.abs() < 1e-30 {
                        pt.fill(bad_sentinel);
                        converged[ip] = true;
                        continue;
                    }
                    let dx = dy[0] / j00;
                    pt[0] += dx;
                    let xlensq = pt[0] * pt[0];
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
                        pt.fill(bad_sentinel);
                        converged[ip] = true;
                        continue;
                    }
                    let dx0 = (d * dy[0] - b * dy[1]) / det;
                    let dx1 = (a * dy[1] - c * dy[0]) / det;
                    pt[0] += dx0;
                    pt[1] += dx1;

                    let xlensq = pt[0] * pt[0] + pt[1] * pt[1];
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
                        pt.fill(bad_sentinel);
                        converged[ip] = true;
                        continue;
                    }
                    let mut vlensq = 0.0;
                    let mut xlensq = 0.0;
                    for (ic, &dx_ic) in rhs.iter().enumerate().take(ncoord) {
                        pt[ic] += dx_ic;
                        vlensq += dx_ic * dx_ic;
                        xlensq += pt[ic] * pt[ic];
                    }
                    if vlensq < tol_sq * xlensq || xlensq == 0.0 {
                        converged[ip] = true;
                    }
                }
            }
        }

        Ok(())
    }

    /// Combine coefficients that refer to the same combination of input powers,
    /// forward then inverse (`polymap.c:3644-3765`). `None` when neither
    /// direction changed, which is C leaving `result` at -1 for this half.
    ///
    /// Basis-independent: two terms over the same powers add whether those
    /// powers index monomials or Chebyshev polynomials, which is why
    /// `ChebyMap::map_merge` calls this and not the linear half.
    pub(crate) fn consolidated_terms(&self) -> Option<(Vec<PolyTerm>, Option<Vec<PolyTerm>>)> {
        // Two constant terms for one output become one constant term. C does
        // the forward transformation and then the inverse (`polymap.c:3650`,
        // `for( check_fwd = 0; check_fwd < 2; check_fwd++ )`), so both
        // directions are consolidated here too. Doing only one leaves a SIP
        // inverse polynomial holding its identity term and its linear
        // correction as two separate coefficients, which the SIP extractor then
        // reads one of.
        let consolidate = |terms: &[PolyTerm]| -> Vec<PolyTerm> {
            let mut out: Vec<PolyTerm> = Vec::new();
            for term in terms {
                if let Some(existing) = out
                    .iter_mut()
                    .find(|t| t.out_axis == term.out_axis && t.powers == term.powers)
                {
                    existing.coeff += term.coeff;
                } else {
                    out.push(term.clone());
                }
            }
            // Remove zero-coefficient terms. C uses an exact `!= 0.0` test when
            // shuffling merged coefficients (`polymap.c:3724`), so mirror it
            // rather than a threshold — a legitimately tiny coefficient
            // survives in C.
            out.retain(|t| t.coeff != 0.0);
            out
        };

        let consolidated = consolidate(&self.forward);
        let consolidated_inv = self.inverse.as_deref().map(consolidate);

        // C's `mod` flag (`polymap.c:3649`, set at `:3746`, read at `:3754`). A
        // structural fingerprint of the forward terms is not enough, because
        // consolidating the *inverse* coefficients leaves the forward
        // transformation, and so the fingerprint, untouched.
        let inv_changed = match (self.inverse.as_deref(), consolidated_inv.as_ref()) {
            (Some(orig), Some(new)) => new.len() != orig.len(),
            _ => false,
        };
        if consolidated.len() == self.forward.len() && !inv_changed {
            return None;
        }

        Some((consolidated, consolidated_inv))
    }

    /// C's linear half (`polymap.c:3767-3918`): a polynomial whose every
    /// remaining term is constant or first-order is rebuilt out of a
    /// `MatrixMap` and a `ShiftMap`. Returns the replacement, or `None` when the
    /// polynomial is not linear.
    ///
    /// `terms` are the coefficients the reduction reads, which is C's `pmap0`
    /// after the consolidation half has had its say.
    ///
    /// `norm` is the per-input scale and offset applied to an input *before* the
    /// coefficients are used, or `None` where the coefficients apply to the
    /// input itself. A PolyMap has no normalisation; a ChebyMap's coefficients
    /// are those of a Chebyshev series in `z = scale*x + offset`, which maps its
    /// bounding box onto `[-1,1]` (`polymap.c:3778-3784`, `:3787-3793`).
    pub(crate) fn linear_replacement(
        &self,
        terms: &[PolyTerm],
        norm: Option<(&[f64], &[f64])>,
    ) -> Option<DynMapping> {
        // A term is constant if the sum of its powers is 0, linear if the sum is
        // 1 with exactly one power equal to 1. C tests only the constructed
        // forward transformation, and its own guard is that the forward
        // coefficients exist and the coordinate counts match (`polymap.c:3785`,
        // `if( pmap0->ncoeff_f && nout == nin )`). An empty consolidated set is
        // therefore still linear: a forward transformation whose coefficients all
        // cancel is the zero matrix, which is what C builds from its
        // zero-initialised workspace.
        let nin = self.nin;
        let nout = self.nout;
        let is_linear = self.forward_defined()
            && nin == nout
            && terms.iter().all(|t| {
                let power_sum: usize = t.powers.iter().sum();
                power_sum <= 1 && t.powers.iter().all(|&p| p <= 1)
            });
        if !is_linear {
            return None;
        }

        // C accumulates into a zero-initialised offset vector and coefficient
        // matrix (`polymap.c:3858-3877`): a term with no input powers adds to an
        // offset, and a term with a single unit power adds to a matrix element.
        // Under a normalisation a term `c*z_j` contributes `c*scale_j` to the
        // matrix and `c*offset_j` to the constant term, so an output can collect
        // a constant contribution from each of its linear terms as well as from
        // its own constant term — which is why both arrays accumulate.
        let mut offsets = vec![0.0; nout];
        let mut matrix = vec![0.0; nout * nin];

        for term in terms {
            let out_idx = term.out_axis - 1;
            let power_sum: usize = term.powers.iter().sum();
            if power_sum == 0 {
                offsets[out_idx] += term.coeff;
            } else {
                let in_idx = term.powers.iter().position(|&p| p == 1).unwrap();
                match norm {
                    Some((scale, offset)) => {
                        matrix[out_idx * nin + in_idx] += term.coeff * scale[in_idx];
                        offsets[out_idx] += term.coeff * offset[in_idx];
                    }
                    None => matrix[out_idx * nin + in_idx] += term.coeff,
                }
            }
        }

        // C builds the MatrixMap and the ShiftMap, composes them in series and
        // hands the pair to `astSimplify` (`polymap.c:3890-3894`), so whatever
        // shape the linear transformation reduces to is the simplifier's answer
        // rather than a shape decided here. The simplification is also what
        // leaves `IsSimp` on the replacement.
        let m = ndarray::Array2::from_shape_vec((nout, nin), matrix).ok()?;
        let mm = MatrixMap::new(m).ok()?;
        let sm = ShiftMap::new(offsets);
        let cm = CmpMap::new(Box::new(mm), Box::new(sm), Composition::Series).ok()?;
        let cm: &dyn Mapping = &cm;
        cm.simplify().ok()
    }

    /// The two "replace with simpler" steps C runs at the head of
    /// `polymap.c::MapMerge` (`polymap.c:3626-3918`), before it looks at any
    /// neighbour: coefficients over the same combination of input powers are
    /// combined, and a PolyMap whose remaining terms are all constant or
    /// first-order is rebuilt out of a `MatrixMap` and a `ShiftMap`.
    ///
    /// Returns the replacement, or `None` when neither transformation changed.
    ///
    /// Everything here reads the transformation as *constructed*: C uses
    /// `coeff_f` / `coeff_i` directly and takes the coordinate counts from
    /// before any inversion (`polymap.c:3630-3638`, `:3785`), so the stored
    /// `Invert` flag selects nothing and the caller keeps the list entry's
    /// direction flag rather than folding it in.
    fn replace_with_simpler(&self) -> Option<DynMapping> {
        // C's consolidation half. When it changes nothing the linear test below
        // runs on the terms as stored, which is C reading `pmap0` when its
        // `mod` flag is clear (`polymap.c:3754-3765`).
        let consolidation = self.consolidated_terms();
        let consolidated_changed = consolidation.is_some();
        let (consolidated, consolidated_inv) = match consolidation {
            Some((fwd, inv)) => (fwd, inv),
            None => (self.forward.clone(), self.inverse.clone()),
        };

        if let Some(linear) = self.linear_replacement(&consolidated, None) {
            return Some(linear);
        }

        // The linear test failed, so the only replacement left is the
        // consolidated PolyMap — and only if the consolidation changed
        // something.
        if !consolidated_changed {
            return None;
        }

        // Build a new PolyMap with the consolidated terms. C makes a deep copy
        // of the original and edits the copy's coefficient arrays in place
        // (`polymap.c:3640-3642`), so the replacement keeps every other
        // attribute, the stored `Invert` flag included.
        let mut pm = PolyMap::new(
            self.nin,
            self.nout,
            &consolidated,
            consolidated_inv.as_deref(),
        )
        .ok()?;

        if let Some(v) = self.iter_inverse {
            pm.set_iter_inverse(v);
        }
        pm.set_niter_inverse(self.niter_inverse);
        pm.set_tol_inverse(self.tol_inverse);
        if self.inverted {
            pm.invert();
        }

        Some(Box::new(pm))
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
pub(crate) fn solve_linear_system(mat: &mut [f64], rhs: &mut [f64], n: usize) -> bool {
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
    fn report_attr(&self) -> Option<bool> {
        self.report
    }

    fn set_report_attr(&mut self, value: Option<bool>) {
        self.report = value;
    }

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
            // Try the cross-lane SIMD forward path when the `simd-eval`
            // feature is built. It handles its own preconditions (shape,
            // npoint threshold) and returns `Fell` when ineligible, in which
            // case we drop through to the scalar path. The SIMD path
            // produces the same polynomial value with the same BAD-value
            // semantics; only timing differs.
            #[cfg(feature = "simd-eval")]
            {
                use super::polymap_simd::SimdPath;
                if self.forward_simd_2d_into(pts, out)? == SimdPath::Took {
                    return Ok(());
                }
            }

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
            // Inverse transform. The iterative inverse takes priority over
            // any supplied inverse coefficients (polymap.c:6092) — the
            // coefficients are typically only an approximation of the true
            // inverse (the FITS-SIP convention).
            if self.iter_inverse() {
                // Try the cross-lane SIMD NR path when the `simd-eval` feature
                // is built. It handles its own preconditions (shape, npoint
                // threshold) and returns `Fell` when ineligible, in which case
                // we drop through to the scalar NR. The SIMD path computes the
                // same NR fixed point with the same niter/tol, so behaviour is
                // identical when it runs — only timing differs.
                #[cfg(feature = "simd-eval")]
                {
                    use super::polymap_simd::SimdPath;
                    if self.inverse_simd_2d_into(pts, out)? == SimdPath::Took {
                        return Ok(());
                    }
                }
                self.newton_raphson_inverse(pts, out, BAD_VALUE)?;
            } else if let Some(ref inv_terms) = self.inverse {
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
            } else {
                // No inverse available: fill with BAD.
                for i in 0..pts.npoint() {
                    out.point_mut(i).fill(BAD_VALUE);
                }
            }
        }
        Ok(())
    }

    /// NaN-as-bad fast path for [`PolyMap`].
    ///
    /// Three sub-cases, mirroring `transform_into`:
    ///
    /// 1. **Forward polynomial eval.** Tries SIMD when the `simd-eval` feature
    ///    is built (already NaN-correct end-to-end); otherwise runs the scalar
    ///    power-table loop with the per-point `point_has_bad` check dropped —
    ///    NaN inputs produce NaN outputs through the arithmetic.
    /// 2. **Explicit inverse polynomial.** Same shape as forward — drop the
    ///    `point_has_bad` check, NaN propagates.
    /// 3. **Iterative NR inverse.** SIMD path is NaN-correct. The scalar
    ///    fallback (`newton_raphson_inverse`) emits `BAD_VALUE` for bad inputs;
    ///    to keep the protocol's "NaN inside the pipeline" invariant we do a
    ///    cheap one-pass `BAD_VALUE → NaN` translation on the output. This is
    ///    O(npoint × ncoord) — comparable to the per-point BAD check we saved
    ///    elsewhere — and only triggers when SIMD is off, so the cost is
    ///    bounded. Writing a NaN-native NR is follow-up work.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        if fwd {
            // Use the protocol-internal SIMD variant: NaN stays inside the
            // pipeline, public-API NaN → BAD post-pass is skipped (the
            // pipeline-level boundary does it once at the end).
            #[cfg(feature = "simd-eval")]
            {
                use super::polymap_simd::SimdPath;
                if self.forward_simd_2d_into_nan(pts, out)? == SimdPath::Took {
                    return Ok(());
                }
            }
            let mut power_table: Vec<Vec<f64>> = self
                .forward_max_pows
                .iter()
                .map(|&m| vec![0.0; m + 1])
                .collect();
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                // No `point_has_bad` gate: NaN propagates through `eval_term`
                // multiplications cleanly.
                fill_power_table(inp, &self.forward_max_pows, &mut power_table);
                for (out_idx, term_indices) in self.forward_by_output.iter().enumerate() {
                    let mut sum = 0.0;
                    for &idx in term_indices {
                        sum += eval_term_with_table(&self.forward[idx], &power_table);
                    }
                    outp[out_idx] = sum;
                }
            }
        } else if self.iter_inverse() {
            // The iterative inverse takes priority over any supplied inverse
            // coefficients (polymap.c:6092).
            // Protocol-internal SIMD variant — see forward branch comment.
            #[cfg(feature = "simd-eval")]
            {
                use super::polymap_simd::SimdPath;
                if self.inverse_simd_2d_into_nan(pts, out)? == SimdPath::Took {
                    return Ok(());
                }
            }
            // Fallback: scalar NR with the NaN sentinel — emits f64::NAN
            // directly for invalid inputs and singular Jacobian, so
            // downstream NaN-aware stages receive the protocol-canonical
            // sentinel with no post-pass needed.
            self.newton_raphson_inverse(pts, out, f64::NAN)?;
        } else if let Some(ref inv_terms) = self.inverse {
            let inv_by_output = self.inverse_by_output.as_ref().unwrap();
            let mut power_table: Vec<Vec<f64>> = self
                .inverse_max_pows
                .iter()
                .map(|&m| vec![0.0; m + 1])
                .collect();
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                fill_power_table(inp, &self.inverse_max_pows, &mut power_table);
                for (out_idx, term_indices) in inv_by_output.iter().enumerate() {
                    let mut sum = 0.0;
                    for &idx in term_indices {
                        sum += eval_term_with_table(&inv_terms[idx], &power_table);
                    }
                    outp[out_idx] = sum;
                }
            }
        } else {
            // No inverse available: emit NaN (was BAD).
            for i in 0..pts.npoint() {
                out.point_mut(i).fill(f64::NAN);
            }
        }
        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C's two replace-with-simpler steps, "without reference to the
        // neighbouring Mappings in the list" (`polymap.c:3626-3918`), taken
        // before any neighbour is considered. See
        // [`PolyMap::replace_with_simpler`].
        //
        // C works from the transformation as *constructed*, before any
        // inversion, and leaves `invert_list` untouched
        // (`polymap.c:3899-3902`) — so the replacement inherits the entry's
        // applied direction rather than absorbing it.
        if let Some(simp) = self.replace_with_simpler() {
            let eff_inv = ctx.entry(where_).1;
            ctx.set_pair_single(where_, (simp, eff_inv, false));
            return Some(where_);
        }

        super::simplify::default_map_merge(ctx, where_, series)
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

        // C forces the stored `Invert` flag of both PolyMaps to the direction
        // the list applies to the nominated one and then compares them with
        // `astEqual` (`polymap.c:3933-3967`). `PolyMap`'s own `Equal` rejects a
        // pair whose `Invert` flags differ (`polymap.c:824-829`), so the forcing
        // is what lets two list entries pointing in opposite directions compare
        // at all; what it then compares is each transformation as constructed.
        let mut a = self.clone();
        let mut b = other_poly.clone();
        if a.inverted != self_inv {
            a.invert();
        }
        if b.inverted != self_inv {
            b.invert();
        }
        if !a.equal(&b) {
            return None;
        }

        // Input dimension of the pair depends on which direction
        // self is being used in.
        let pair_nin = if self_inv { self.nout } else { self.nin };
        Some(Box::new(UnitMap::new(pair_nin)))
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
        // Inverse transformation: C compares its own per-axis coefficient
        // counts, per-axis maximum powers, coefficient values and power
        // patterns (`polymap.c:772-822`), each array indexed by the axis count
        // it was allocated with. A term list carries all four, so the inverse
        // terms compare exactly as the forward ones do.
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

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn has_forward(&self) -> bool {
        if self.inverted {
            self.inverse_defined()
        } else {
            self.forward_defined()
        }
    }

    fn has_inverse(&self) -> bool {
        if self.inverted {
            self.forward_defined()
        } else {
            self.inverse_defined()
        }
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
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
            for (out_idx, terms) in terms_by_out.iter().enumerate() {
                for (term_idx, term) in terms.iter().enumerate() {
                    for (k, &p) in term.powers.iter().enumerate() {
                        iv += 1;
                        if p > 0 {
                            node.push_double(
                                &format!("PF{iv}"),
                                p as f64,
                                Some(format!(
                                    "Power of i/p {} for coeff {} of fwd poly {}",
                                    k + 1,
                                    term_idx + 1,
                                    out_idx + 1
                                )),
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
                for (out_idx, terms) in terms_by_out.iter().enumerate() {
                    for (term_idx, term) in terms.iter().enumerate() {
                        for (k, &p) in term.powers.iter().enumerate() {
                            iv += 1;
                            if p > 0 {
                                node.push_double(
                                    &format!("PI{iv}"),
                                    p as f64,
                                    Some(format!(
                                        "Power of o/p {} for coeff {} of inv poly {}",
                                        k + 1,
                                        term_idx + 1,
                                        out_idx + 1
                                    )),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Iterative inverse attributes. C writes IterInv only when the
        // attribute is explicitly set (TestIterInverse).
        if let Some(v) = self.iter_inverse {
            let comment = if v {
                "Use an iterative inverse"
            } else {
                "Do not use an iterative inverse"
            };
            node.push_int("IterInv", v as i64, Some(comment.to_string()));
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
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("PolyMap");
        let base = read_mapping_base(whole);
        let nin = base.nin;
        let nout = base.nout;

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
                    if coeff != BAD_VALUE {
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
                    if coeff != BAD_VALUE {
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

        // Read iterative inverse attributes. An absent IterInv leaves
        // the attribute unset so the C default (no inverse coefficients
        // supplied) applies.
        let iter_inv = node.get_int_any("IterInv").map(|v| v != 0);
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
        // serialised format. The Jacobian cache is needed whenever the
        // effective IterInverse (explicit or default) is on.
        let iter_effective = iter_inv.unwrap_or_else(|| {
            inverse
                .as_ref()
                .is_none_or(|v: &Vec<PolyTerm>| v.is_empty())
        });
        let jacobian_terms = if iter_effective && nin == nout {
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
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
            report: None,
            jacobian_terms,
            forward_by_output,
            forward_max_pows,
            inverse_by_output,
            inverse_max_pows,
        };
        if base.inverted {
            m.invert();
        }
        // C assigns the structure component and only then re-applies it through
        // the setter, so the set changes nothing and the setter's guarded
        // `astClearIsSimple` does not fire (polymap.c:7461-7470): the struct literal
        // above already holds the values read from the cards. The setters still
        // run because they carry side effects -- `IterInverse` builds the
        // Jacobian terms.
        if let Some(v) = iter_inv {
            m.set_iter_inverse(v);
        }
        if node.get("NiterInv").is_some() {
            m.set_niter_inverse(niter_inv);
        }
        if node.get("TolInv").is_some() {
            m.set_tol_inverse(tol_inv);
        }
        Ok(m)
    }
}

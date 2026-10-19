//! N-dimensional polynomial transformation mapping.

use levenberg_marquardt::{LeastSquaresProblem, LevenbergMarquardt};
use nalgebra::{DMatrix, DVector, Dyn, OMatrix, OVector, VecStorage};

use crate::constants::BAD_VALUE;
use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::ast_equal;
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
    /// `None` while unset (C's `-INT_MAX`), reading as 4.
    niter_inverse: Option<usize>,
    /// `None` while unset (C's `AST__BAD`), reading as 1e-6.
    tol_inverse: Option<f64>,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
    /// Pre-computed Jacobian polynomial terms (one set of terms per input axis).
    /// Only populated when `iter_inverse` is true and `nin == nout`.
    jacobian_terms: Option<Vec<Vec<PolyTerm>>>,
    /// C's `lintrunc`: the linear truncation [`PolyMap::linear_guess`]
    /// builds for the iterative inverse's first guess, kept once built
    /// (polymap.c `LinearGuess`) and cleared when the coefficients change.
    /// A clone keeps it, as C's Copy copies it.
    lintrunc: std::cell::RefCell<Option<DynMapping>>,
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

/// The terms with non-zero coefficients, which are the ones C stores
/// (polymap.c `AddCoeff`).
fn nonzero(terms: &[PolyTerm]) -> Vec<PolyTerm> {
    terms.iter().filter(|t| t.coeff != 0.0).cloned().collect()
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
    /// A term with a zero coefficient contributes nothing and is dropped, as
    /// C's `AddCoeff` drops it (polymap.c). Returns an error if any term is
    /// invalid.
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
            // polymap.c StoreArrays: AST__BADCI.
            if term.out_axis < 1 || term.out_axis > nout {
                return Err(AstError::new(
                    ErrorCode::Badci,
                    format!(
                        "PolyMap: forward coefficient {} referred to an illegal output \
                         coordinate {}; this number should be in the range 1 to {nout}",
                        i + 1,
                        term.out_axis
                    ),
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
                        ErrorCode::Badci,
                        format!(
                            "PolyMap: inverse coefficient {} referred to an illegal input \
                             coordinate {}; this number should be in the range 1 to {nin}",
                            i + 1,
                            term.out_axis
                        ),
                    ));
                }
            }
            Some(nonzero(inv_terms))
        } else {
            None
        };

        let fwd_vec = nonzero(forward);
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
            niter_inverse: None,
            tol_inverse: None,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            jacobian_terms: None,
            lintrunc: Default::default(),
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
    /// When unset, it is set if no inverse coefficients were supplied and
    /// the PolyMap has as many inputs as outputs, as C's `GetIterInverse`
    /// default is (polymap.c:6384-6387), which makes such a forward-only
    /// PolyMap invertible out of the box.
    pub fn iter_inverse(&self) -> bool {
        self.iter_inverse.unwrap_or_else(|| {
            self.inverse.as_ref().is_none_or(|v| v.is_empty()) && self.nin == self.nout
        })
    }

    /// Enable or disable the iterative inverse.
    ///
    /// An iterative inverse needs as many inputs as outputs, so enabling it
    /// on any other PolyMap is an `AST__ATTIN` error that leaves the value
    /// unchanged, as C's `SetIterInverse` is (polymap.c:6388-6393).
    pub fn set_iter_inverse(&mut self, v: bool) -> AstResult<()> {
        if v && self.nin != self.nout {
            return Err(AstError::new(
                ErrorCode::Attin,
                "PolyMap: cannot use an iterative inverse because the PolyMap has \
                 unequal numbers of inputs and outputs",
            ));
        }
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
        Ok(())
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
        self.niter_inverse.unwrap_or(4)
    }

    /// Set the maximum number of iterations for the iterative inverse.
    pub fn set_niter_inverse(&mut self, v: usize) {
        // `NiterInverse` changes what the iterative inverse computes
        // (polymap.c:6383-6385).
        if Some(v) != self.niter_inverse {
            self.simplified.clear();
        }
        self.niter_inverse = Some(v);
    }

    /// Whether NiterInverse is set (polymap.c `TestNiterInverse`).
    pub fn test_niter_inverse(&self) -> bool {
        self.niter_inverse.is_some()
    }

    /// Unset NiterInverse, so that it reads as 4 (polymap.c
    /// `ClearNiterInverse`, which also discards both simplified records).
    pub fn clear_niter_inverse(&mut self) {
        self.simplified.clear();
        self.niter_inverse = None;
    }

    /// Target relative error for the iterative inverse.
    pub fn tol_inverse(&self) -> f64 {
        self.tol_inverse.unwrap_or(1e-6)
    }

    /// Set the target relative error for the iterative inverse.
    pub fn set_tol_inverse(&mut self, v: f64) {
        // `TolInverse` changes what the iterative inverse computes
        // (polymap.c:6420-6422).
        // C compares the raw value, AST__BAD while unset (polymap.c:6467).
        if v != self.tol_inverse.unwrap_or(BAD_VALUE) {
            self.simplified.clear();
        }
        // AST__BAD is C's unset value, so setting it unsets the attribute.
        self.tol_inverse = (v != BAD_VALUE).then_some(v);
    }

    /// Whether TolInverse is set (polymap.c `TestTolInverse`).
    pub fn test_tol_inverse(&self) -> bool {
        self.tol_inverse.is_some()
    }

    /// Unset TolInverse, so that it reads as 1e-6 (polymap.c
    /// `ClearTolInverse`, which also discards both simplified records).
    pub fn clear_tol_inverse(&mut self) {
        self.simplified.clear();
        self.tol_inverse = None;
    }

    /// Return a reference to the forward polynomial terms of the original,
    /// uninverted PolyMap, in the order they were given.
    /// [`PolyMap::coeffs`] is C's `astPolyCoeffs`, relative to the Invert
    /// flag.
    pub fn forward_terms(&self) -> &[PolyTerm] {
        &self.forward
    }

    /// Return a reference to the inverse polynomial terms of the original,
    /// uninverted PolyMap, if defined, in the order they were given.
    pub fn inverse_terms(&self) -> Option<&[PolyTerm]> {
        self.inverse.as_deref()
    }

    /// The coefficients of the forward (`forward`) or inverse transformation
    /// of this PolyMap as it stands, as C's `astPolyCoeffs` returns them: the
    /// transformation is relative to the Invert flag, so an inverted
    /// PolyMap's forward coefficients are the original inverse ones, and they
    /// are grouped by the output each defines, in the order each output's
    /// were given. Empty when the transformation has no coefficients.
    pub fn coeffs(&self, forward: bool) -> Vec<PolyTerm> {
        let (terms, by_output) = if forward != self.inverted {
            (Some(&self.forward), Some(&self.forward_by_output))
        } else {
            (self.inverse.as_ref(), self.inverse_by_output.as_ref())
        };
        match (terms, by_output) {
            (Some(terms), Some(by_output)) => by_output
                .iter()
                .flatten()
                .map(|&i| terms[i].clone())
                .collect(),
            _ => Vec::new(),
        }
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
    /// The coefficient tables of the original forward (`forward`) or inverse
    /// transformation, if it has coefficients.
    pub(crate) fn term_tables(&self, forward: bool) -> Option<TermTables<'_>> {
        if forward {
            Some(TermTables {
                terms: &self.forward,
                by_output: &self.forward_by_output,
                max_pows: &self.forward_max_pows,
            })
        } else {
            Some(TermTables {
                terms: self.inverse.as_deref()?,
                by_output: self.inverse_by_output.as_deref()?,
                max_pows: &self.inverse_max_pows,
            })
        }
    }

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
        // C copies the PolyMap and replaces a transformation of the copy.
        // The copy's simplified record no longer applies once that is done
        // (C `astClearIsSimple( result )`, polymap.c:5030).
        let mut result = self.clone();
        result.simplified = SimplifiedRecord::default();
        let Some(new_terms) = crate::mappings::helpers::poly_tran::replace_transformation(
            &mut result,
            forward,
            acc,
            maxacc,
            maxorder,
            lbnd,
            ubnd,
        )?
        else {
            return Ok(None);
        };
        result.store_terms(forward != self.inverted, &new_terms);

        // The new fit is used in preference to any iterative inverse.
        result.clear_iter_inverse();
        Ok(Some(result))
    }

    /// polymap.c `StoreArrays`: replace the coefficients of the original
    /// forward (`forward`) or inverse transformation.
    pub(crate) fn store_terms(&mut self, forward: bool, terms: &[PolyTerm]) {
        let terms = nonzero(terms);
        if forward {
            self.forward_by_output = compute_terms_by_output(&terms, self.nout);
            self.forward_max_pows = compute_max_powers(&terms, self.nin);
            self.forward = terms;
        } else {
            self.inverse_by_output = Some(compute_terms_by_output(&terms, self.nin));
            self.inverse_max_pows = compute_max_powers(&terms, self.nout);
            self.inverse = Some(terms);
        }
        self.jacobian_terms =
            (self.nin == self.nout && self.iter_inverse()).then(|| self.compute_jacobian_terms());
        // The linear truncation is no longer reliable (polymap.c:4110).
        *self.lintrunc.borrow_mut() = None;
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

    /// The linear truncation of this PolyMap's transformation, as polymap.c
    /// `LinearGuess` builds it: a MatrixMap of the linear coefficients
    /// followed by a ShiftMap of the constant ones, from the forward
    /// coefficients, or from the inverse ones (and then inverted) when there
    /// are no forward ones. A later coefficient for the same input replaces
    /// an earlier one rather than adding to it, as C assigns them. A row
    /// with no linear coefficient gets 1.0 on the diagonal so that the
    /// matrix can be inverted, and a matrix that still cannot be inverted
    /// is replaced by a UnitMap.
    fn linear_guess(&self) -> AstResult<DynMapping> {
        let (terms, by_output, nin, nout, inverse) = if !self.forward.is_empty() {
            (
                &self.forward,
                &self.forward_by_output,
                self.nin,
                self.nout,
                false,
            )
        } else {
            match (self.inverse.as_ref(), self.inverse_by_output.as_ref()) {
                (Some(terms), Some(by_output)) => (terms, by_output, self.nout, self.nin, true),
                _ => (
                    &self.forward,
                    &self.forward_by_output,
                    self.nin,
                    self.nout,
                    false,
                ),
            }
        };
        let mut matrix = ndarray::Array2::<f64>::zeros((nout, nin));
        let mut vector = vec![0.0; nout];
        for (iout, indices) in by_output.iter().enumerate() {
            for &ico in indices {
                let term = &terms[ico];
                // 0: constant, 1: linear in input `jin`, 2: anything else.
                let mut flag = 0;
                let mut jin = 0;
                for (iin, &pow) in term.powers.iter().enumerate() {
                    if pow != 0 {
                        if pow != 1 || flag == 1 {
                            flag = 2;
                            break;
                        }
                        flag = 1;
                        jin = iin;
                    }
                }
                match flag {
                    0 => vector[iout] = term.coeff,
                    1 => matrix[[iout, jin]] = term.coeff,
                    _ => {}
                }
            }
        }
        for iout in 0..nout {
            if (0..nin).all(|iin| matrix[[iout, iin]] == 0.0) {
                matrix[[iout, iout]] = 1.0;
            }
        }
        let mm = MatrixMap::new(matrix)?;
        let mm: DynMapping = if mm.has_inverse() {
            Box::new(mm)
        } else {
            Box::new(UnitMap::new(nin))
        };
        let mut result: DynMapping = Box::new(CmpMap::new(
            mm,
            Box::new(ShiftMap::new(vector)),
            Composition::Series,
        )?);
        if inverse {
            result.invert();
        }
        Ok(result)
    }

    /// The iterative inverse, transcribing polymap.c `Transform` with
    /// IterInverse set. The initial guess is the inverse of
    /// [`PolyMap::linear_guess`]. Each Newton-Raphson iteration transforms
    /// the current guesses with the forward transformation and with the
    /// Jacobian's PolyMaps, one per input, as C's `astTransform` calls do,
    /// and solves the Jacobian equation for each point not yet converged
    /// with `palDmat`. A singular Jacobian makes the point bad. A point has
    /// converged when `|dx|^2 <= tol^2 |x|^2`. Bad values are carried
    /// through the arithmetic as C carries them, so a point bad on one axis
    /// can keep a value on another. Each result C gets from `astTransform`
    /// has its non-finite values replaced by bad ones, as `astTransform`
    /// does, and so does the final one.
    fn newton_raphson_inverse(&self, target: &PointSet, result: &mut PointSet) -> AstResult<()> {
        let npoint = target.npoint();
        let ncoord = self.nin; // nin == nout for iterative inverse
        debug_assert_eq!(self.nin, self.nout);

        // The Jacobian, one PolyMap per input, each giving the derivatives of
        // every output with respect to that input (polymap.c GetJacobian).
        let jacob: Vec<PolyMap> = self
            .get_jacobian_terms()
            .iter()
            .map(|terms| PolyMap::new(ncoord, ncoord, terms, None))
            .collect::<AstResult<_>>()?;

        if self.lintrunc.borrow().is_none() {
            *self.lintrunc.borrow_mut() = Some(self.linear_guess()?);
        }
        self.lintrunc
            .borrow()
            .as_ref()
            .expect("built above")
            .transform_into(target.as_points(), false, result.as_points_mut())?;
        super::substitute_nan_to_bad_in_place(result);

        let maxiter = self.niter_inverse();
        let maxerr = self.tol_inverse() * self.tol_inverse();
        let tables = self
            .term_tables(true)
            .expect("an iterative inverse needs forward coefficients");
        let mut work = PointSet::zeros(npoint, ncoord);
        let mut ps_jac: Vec<PointSet> = (0..ncoord)
            .map(|_| PointSet::zeros(npoint, ncoord))
            .collect();
        let mut flags = vec![false; npoint];
        let mut nconv = 0;
        let mut mat = vec![0.0; ncoord * ncoord];
        let mut vec = vec![0.0; ncoord];

        let mut iter = 0;
        while iter < maxiter && nconv < npoint {
            iter += 1;
            poly_transform(
                self,
                true,
                &tables,
                &result.as_points(),
                &mut work.as_points_mut(),
                BAD_VALUE,
            );
            super::substitute_nan_to_bad_in_place(&mut work);
            for ic in 0..ncoord {
                let t = target.axis(ic);
                for (w, &t) in work.axis_mut(ic).iter_mut().zip(t) {
                    *w = if t != BAD_VALUE && *w != BAD_VALUE {
                        t - *w
                    } else {
                        BAD_VALUE
                    };
                }
            }
            for (map, ps) in jacob.iter().zip(ps_jac.iter_mut()) {
                map.transform_into(result.as_points(), true, ps.as_points_mut())?;
                super::substitute_nan_to_bad_in_place(ps);
            }
            for (ip, flag) in flags.iter_mut().enumerate() {
                if *flag {
                    continue;
                }
                for irow in 0..ncoord {
                    for icol in 0..ncoord {
                        mat[irow * ncoord + icol] = ps_jac[icol].axis(irow)[ip];
                    }
                    vec[irow] = work.axis(irow)[ip];
                }
                if !crate::pal::dmat(ncoord, &mut mat, &mut vec) {
                    for ic in 0..ncoord {
                        result.axis_mut(ic)[ip] = BAD_VALUE;
                    }
                    *flag = true;
                    nconv += 1;
                } else {
                    let mut vlensq = 0.0;
                    let mut xlensq = 0.0;
                    for (ic, &v) in vec.iter().enumerate().take(ncoord) {
                        let x = result.axis_mut(ic);
                        let xx = x[ip] + v;
                        x[ip] = xx;
                        xlensq += xx * xx;
                        vlensq += v * v;
                    }
                    if vlensq <= maxerr * xlensq {
                        *flag = true;
                        nconv += 1;
                    }
                }
            }
        }
        super::substitute_nan_to_bad_in_place(result);
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
            pm.set_iter_inverse(v).ok()?;
        }
        pm.niter_inverse = self.niter_inverse;
        pm.tol_inverse = self.tol_inverse;
        if self.inverted {
            pm.invert();
        }

        Some(Box::new(pm))
    }
}

// ---------------------------------------------------------------------------
// Polynomial evaluation (power-table approach)
// ---------------------------------------------------------------------------

/// The class-specific parts of `astPolyTran`, which polymap.c's fitting
/// driver (`ReplaceTransformation`, `FitPoly1D`, `FitPoly2D`) calls through
/// the vtab: `FitPoly1DInit` and `FitPoly2DInit` (polymap.c:1262, :1608),
/// which ChebyMap overrides (chebymap.c:523, :668), and the coefficient
/// counts and `StoreArrays` the driver reads and writes.
pub(crate) trait PolyFit: Mapping {
    /// C's `FitPoly1DInit`: the values each coefficient of a 1-D fit of
    /// `order` terms multiplies at each sample of `table[0]`, as
    /// `nsamp * order` values, sample by sample. `scales` holds the RMS that
    /// restores each table column, and a class that applies the input scaling
    /// itself resets `scales[0]` to 1. `forward` is the transformation being
    /// replaced.
    fn fit_poly_1d_init(
        &mut self,
        forward: bool,
        table: &[Vec<f64>],
        order: usize,
        scales: &mut [f64],
    ) -> AstResult<Vec<f64>>;

    /// C's `FitPoly2DInit`: as [`fit_poly_1d_init`](PolyFit::fit_poly_1d_init)
    /// for the two inputs `table[0]` and `table[1]`, giving the values for
    /// each.
    fn fit_poly_2d_init(
        &mut self,
        forward: bool,
        table: &[Vec<f64>],
        order: usize,
        scales: &mut [f64],
    ) -> AstResult<(Vec<f64>, Vec<f64>)>;

    /// Whether the original forward (`forward`) or inverse transformation has
    /// coefficients: C's `ncoeff_f` or `ncoeff_i` being non-zero.
    fn has_coeffs(&self, forward: bool) -> bool;
}

/// polymap.c `FitPoly1DInit`: the powers of each sample.
pub(crate) fn power_basis(x: &[f64], order: usize) -> Vec<f64> {
    let mut xp = Vec::with_capacity(x.len() * order);
    for &x1 in x {
        let mut tv = 1.0;
        for _ in 0..order {
            xp.push(tv);
            tv *= x1;
        }
    }
    xp
}

/// The values a polynomial term multiplies its coefficient by: C's
/// `PolyPowers`, the method PolyMap's `Transform` calls once per point
/// (polymap.c:4725) and ChebyMap overrides to give Chebyshev polynomials in
/// place of powers (chebymap.c:1062).
///
/// `work[axis][p]` receives the value standing for the input on `axis` raised
/// to power `p`, for `p` up to `max_pows[axis]`; `work[axis][0]` is always 1.
/// A value that cannot be formed, such as any power above zero of a bad input,
/// is `BAD_VALUE`. `forward` says which of the original transformations the
/// terms belong to. Returns whether every value is good, which lets the
/// caller skip the per-term checks.
pub(crate) trait PolyPowers {
    fn poly_powers(
        &self,
        work: &mut [Vec<f64>],
        max_pows: &[usize],
        input: &[f64],
        forward: bool,
    ) -> bool;
}

/// The coefficients of one of a PolyMap's original transformations, with the
/// indices of each output's terms and the highest power of each input.
pub(crate) struct TermTables<'a> {
    pub(crate) terms: &'a [PolyTerm],
    pub(crate) by_output: &'a [Vec<usize>],
    pub(crate) max_pows: &'a [usize],
}

/// Transform `pts` through polynomial terms, as PolyMap's `Transform` does
/// (polymap.c:6180-6283): the powers come from `basis`, a term with a bad
/// coefficient or a bad power makes its output bad, and every other term
/// adds to its output. An output that is bad, or NaN, is set to `bad`, which
/// is `BAD_VALUE` or, for the NaN protocol, NaN.
pub(crate) fn poly_transform<P: PolyPowers + ?Sized>(
    basis: &P,
    forward: bool,
    tables: &TermTables<'_>,
    pts: &Points<'_>,
    out: &mut PointsMut<'_>,
    bad: f64,
) {
    // C allocates at least two values per axis, since ChebyMap stores the
    // first-degree value whatever the highest power (polymap.c:6199).
    let mut work: Vec<Vec<f64>> = tables
        .max_pows
        .iter()
        .map(|&m| vec![0.0; m.max(1) + 1])
        .collect();
    let bad_coeff = tables.terms.iter().any(|t| t.coeff == BAD_VALUE);
    let pin = pts.axis_slices();
    let pout = out.axis_list_mut();
    let mut inp = vec![0.0; pin.len()];
    for i in 0..pts.npoint() {
        for (v, axis) in inp.iter_mut().zip(pin) {
            *v = axis[i];
        }
        let all_good = basis.poly_powers(&mut work, tables.max_pows, &inp, forward);
        if all_good && !bad_coeff {
            // No term can be bad, so this is the loop below without its checks.
            for (out_idx, term_indices) in tables.by_output.iter().enumerate() {
                let mut outval = 0.0;
                for &idx in term_indices {
                    outval += eval_term_with_table(&tables.terms[idx], &work);
                }
                pout[out_idx][i] = if outval.is_nan() { bad } else { outval };
            }
            continue;
        }
        for (out_idx, term_indices) in tables.by_output.iter().enumerate() {
            let mut outval = 0.0;
            'terms: for &idx in term_indices {
                let term = &tables.terms[idx];
                let mut value = term.coeff;
                if value == BAD_VALUE {
                    outval = BAD_VALUE;
                    break;
                }
                for (axis, &pow) in term.powers.iter().enumerate() {
                    if pow > 0 {
                        let xp = work[axis][pow];
                        if xp == BAD_VALUE {
                            outval = BAD_VALUE;
                            break 'terms;
                        }
                        value *= xp;
                    }
                }
                outval += value;
            }
            pout[out_idx][i] = if outval == BAD_VALUE || outval.is_nan() {
                bad
            } else {
                outval
            };
        }
    }
}

/// Fill `work` with the powers of each input value (polymap.c `PolyPowers`):
/// a bad input has bad powers above zero. Returns whether every power is
/// good.
pub(crate) fn monomial_powers(work: &mut [Vec<f64>], max_pows: &[usize], input: &[f64]) -> bool {
    let mut all_good = true;
    for (axis, pwork) in work.iter_mut().enumerate() {
        pwork[0] = 1.0;
        let x = input[axis];
        let mp = max_pows[axis];
        if x == BAD_VALUE {
            pwork[1..=mp].fill(BAD_VALUE);
            all_good &= mp == 0;
        } else {
            for p in 1..=mp {
                pwork[p] = pwork[p - 1] * x;
            }
        }
    }
    all_good
}

impl PolyFit for PolyMap {
    fn fit_poly_1d_init(
        &mut self,
        _forward: bool,
        table: &[Vec<f64>],
        order: usize,
        _scales: &mut [f64],
    ) -> AstResult<Vec<f64>> {
        Ok(power_basis(&table[0], order))
    }

    fn fit_poly_2d_init(
        &mut self,
        _forward: bool,
        table: &[Vec<f64>],
        order: usize,
        _scales: &mut [f64],
    ) -> AstResult<(Vec<f64>, Vec<f64>)> {
        Ok((power_basis(&table[0], order), power_basis(&table[1], order)))
    }

    fn has_coeffs(&self, forward: bool) -> bool {
        if forward {
            !self.forward.is_empty()
        } else {
            self.inverse.as_ref().is_some_and(|v| !v.is_empty())
        }
    }
}

impl PolyPowers for PolyMap {
    fn poly_powers(
        &self,
        work: &mut [Vec<f64>],
        max_pows: &[usize],
        input: &[f64],
        _forward: bool,
    ) -> bool {
        monomial_powers(work, max_pows, input)
    }
}

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
///
/// C skips an input whose power is zero (polymap.c:6279). This multiplies by
/// its table entry, which is always exactly 1, so the product is unchanged
/// and the loop has no branch; that makes PolyMap's forward transformation
/// about a quarter faster.
#[inline]
fn eval_term_with_table(term: &PolyTerm, power_table: &[Vec<f64>]) -> f64 {
    let mut prod = term.coeff;
    for (table, &pow) in power_table.iter().zip(&term.powers) {
        prod *= table[pow];
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
        fill_power_table(&inp, &max_pows, &mut power_table);

        let mut outp = output.point_mut(ip);
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
                        deriv *= crate::math::powi(xk, pk as i32 - 1);
                    } else {
                        deriv *= crate::math::powi(xk, pk as i32);
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
    if report.termination.was_successful() || crate::math::sqrt(report.objective_function) < tol {
        let solution: Vec<f64> = result.params.iter().copied().collect();
        // Final check: compute residual norm.
        let fwd = eval_poly(terms, nin, nout, &solution);
        let residual_sq: f64 = fwd
            .iter()
            .zip(target.iter())
            .map(|(f, t)| crate::math::powi(f - t, 2))
            .sum();
        if crate::math::sqrt(residual_sq) < tol {
            return solution;
        }
    }

    vec![BAD_VALUE; nin]
}

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for PolyMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
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

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let n_out = if self.effective_forward(forward) {
            self.nout
        } else {
            self.nin
        };
        let mut out = PointSet::zeros(pts.npoint(), n_out);
        self.transform_into(pts.as_points(), forward, out.as_points_mut())?;
        Ok(out)
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
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
                if self.forward_simd_2d_into(&pts, &mut out)? == SimdPath::Took {
                    return Ok(());
                }
            }

            if let Some(tables) = self.term_tables(true) {
                poly_transform(self, true, &tables, &pts, &mut out, BAD_VALUE);
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
                    if self.inverse_simd_2d_into(&pts, &mut out)? == SimdPath::Took {
                        return Ok(());
                    }
                }
                let target = PointSet::from_points(&pts);
                let mut result = PointSet::zeros(target.npoint(), self.nin);
                self.newton_raphson_inverse(&target, &mut result)?;
                out.copy_from(&result.as_points());
            } else if let Some(tables) = self.term_tables(false) {
                // Explicit inverse terms: the inverse maps from nout-space
                // to nin-space.
                poly_transform(self, false, &tables, &pts, &mut out, BAD_VALUE);
            } else {
                // No inverse available: fill with BAD.
                let n = pts.npoint();
                for axis in out.axes_iter_mut().take(self.nin) {
                    axis[..n].fill(BAD_VALUE);
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
    ///    fallback (`newton_raphson_inverse`) works in C's bad values, so NaN
    ///    inputs are translated to `BAD_VALUE` before it and bad outputs to
    ///    NaN after it.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        if fwd {
            // Use the protocol-internal SIMD variant: NaN stays inside the
            // pipeline, public-API NaN → BAD post-pass is skipped (the
            // pipeline-level boundary does it once at the end).
            #[cfg(feature = "simd-eval")]
            {
                use super::polymap_simd::SimdPath;
                if self.forward_simd_2d_into_nan(&pts, &mut out)? == SimdPath::Took {
                    return Ok(());
                }
            }
            if let Some(tables) = self.term_tables(true) {
                poly_transform(self, true, &tables, &pts, &mut out, f64::NAN);
            }
        } else if self.iter_inverse() {
            // The iterative inverse takes priority over any supplied inverse
            // coefficients (polymap.c:6092).
            // Protocol-internal SIMD variant — see forward branch comment.
            #[cfg(feature = "simd-eval")]
            {
                use super::polymap_simd::SimdPath;
                if self.inverse_simd_2d_into_nan(&pts, &mut out)? == SimdPath::Took {
                    return Ok(());
                }
            }
            // Fallback: the scalar iterative inverse works in C's bad values,
            // so NaN inputs become BAD_VALUE on the way in and bad outputs
            // NaN on the way out.
            let mut bad_in = PointSet::from_points(&pts);
            {
                let mut view = bad_in.view_mut();
                for v in view.as_slice_mut().expect("PointSet is C-contiguous") {
                    if v.is_nan() {
                        *v = BAD_VALUE;
                    }
                }
            }
            let mut result = PointSet::zeros(bad_in.npoint(), self.nin);
            self.newton_raphson_inverse(&bad_in, &mut result)?;
            out.copy_from(&result.as_points());
            crate::mappings::bad_to_nan(self, forward, result.npoint(), &mut out);
        } else if let Some(tables) = self.term_tables(false) {
            poly_transform(self, false, &tables, &pts, &mut out, f64::NAN);
        } else {
            // No inverse available: every output is bad, which is NaN here.
            let n = pts.npoint();
            for axis in out.axes_iter_mut().take(self.nin) {
                axis[..n].fill(f64::NAN);
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
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
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

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }
    fn clear_id(&mut self) {
        self.id.clear();
    }
    fn test_id(&self) -> bool {
        self.id.test()
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

        // The iterative inverse attributes, active when set and otherwise
        // the defaults their getters give (polymap.c `Dump`).
        let v = self.iter_inverse();
        node.write_int(
            "IterInv",
            self.iter_inverse.is_some(),
            false,
            i64::from(v),
            if v {
                "Use an iterative inverse"
            } else {
                "Do not use an iterative inverse"
            },
        );
        node.write_int(
            "NiterInv",
            self.niter_inverse.is_some(),
            false,
            self.niter_inverse() as i64,
            "Max number of iterations for iterative inverse",
        );
        node.write_double(
            "TolInv",
            self.tol_inverse.is_some(),
            false,
            self.tol_inverse(),
            "Target relative error for iterative inverse",
        );

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

        let forward = read_poly_terms(node, 'F', nin, nout).unwrap_or_default();
        let inverse = read_poly_terms(node, 'I', nout, nin);

        // Read iterative inverse attributes. An absent IterInv leaves
        // the attribute unset so the C default (no inverse coefficients
        // supplied) applies.
        let iter_inv = node.get_int_any("IterInv").map(|v| v != 0);
        // C reads each raw, with its unset value as the default (polymap.c
        // :7512-7517). A negative count runs no iterations, as 0 does.
        let niter_inv = node
            .get("NiterInv")
            .map(|_| node.get_int("NiterInv", 0).max(0) as usize);
        let tol_inv = node
            .get("TolInv")
            .map(|_| node.get_double("TolInv", BAD_VALUE))
            .filter(|&v| v != BAD_VALUE);

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
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            jacobian_terms,
            lintrunc: Default::default(),
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
            m.set_iter_inverse(v)?;
        }
        if let Some(v) = niter_inv {
            m.set_niter_inverse(v);
        }
        if let Some(v) = tol_inv {
            m.set_tol_inverse(v);
        }
        Ok(m)
    }
}

/// One direction's polynomial terms from a PolyMap's part of a dump, as
/// polymap.c `astLoadPolyMap` reads them: `None` when the direction is not
/// defined, which is when the maximum power of any of its `nvar` input
/// variables (`MPF<i>` or `MPI<i>`) or the term count of any of its `nout`
/// outputs (`NCF<i>` or `NCI<i>`) is missing. Each coefficient is
/// `CF<n>`, numbered through all the terms, or, if that is bad, the older
/// `CF<i>_<j>`; each power is `PF<n>`, or, if that is 0, `PF<i>_<j>_<k>`.
/// `dir` is `'F'` or `'I'`.
pub(crate) fn read_poly_terms(
    node: &Node,
    dir: char,
    nvar: usize,
    nout: usize,
) -> Option<Vec<PolyTerm>> {
    // The maximum powers, which the port does not keep, are read only to
    // find whether the direction is defined.
    for i in 1..=nvar {
        node.get_int_any_if_set(&format!("MP{dir}{i}"))?;
    }
    let mut ncoeff = Vec::with_capacity(nout);
    for i in 1..=nout {
        ncoeff.push(node.get_int_any_if_set(&format!("NC{dir}{i}"))?.max(0) as usize);
    }
    let mut coeffs = Vec::new();
    let mut iv = 0;
    for (i, &nc) in ncoeff.iter().enumerate() {
        for j in 1..=nc {
            iv += 1;
            let mut coeff = node.get_double(&format!("C{dir}{iv}"), BAD_VALUE);
            if coeff == BAD_VALUE {
                coeff = node.get_double(&format!("C{dir}{}_{j}", i + 1), BAD_VALUE);
            }
            coeffs.push(coeff);
        }
    }
    let mut terms = Vec::with_capacity(coeffs.len());
    let mut coeffs = coeffs.into_iter();
    let mut iv = 0;
    for (i, &nc) in ncoeff.iter().enumerate() {
        for j in 1..=nc {
            let mut powers = Vec::with_capacity(nvar);
            for k in 1..=nvar {
                iv += 1;
                let mut power = node.get_int(&format!("P{dir}{iv}"), 0);
                if power == 0 {
                    power = node.get_int(&format!("P{dir}{}_{j}_{k}", i + 1), 0);
                }
                powers.push(power.max(0) as usize);
            }
            // A coefficient missing from the dump is bad, as C reads it
            // (polymap.c:7388), and makes its output bad.
            terms.push(PolyTerm {
                coeff: coeffs.next().unwrap_or(BAD_VALUE),
                powers,
                out_axis: i + 1,
            });
        }
    }
    Some(terms)
}

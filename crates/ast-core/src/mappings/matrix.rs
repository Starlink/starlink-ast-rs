//! Linear matrix transformation mapping.
//!
//! `MatrixMap` applies a matrix multiplication to a set of input coordinates,
//! mapping `nin` inputs to `nout` outputs via an `nout × nin` matrix.
//! For square matrices, the inverse transformation is also available.

use ndarray::Array2;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::perm::PermMap;
use super::unit::UnitMap;
use super::winmap::WinMap;
use super::zoom::ZoomMap;
use super::{DynMapping, Mapping};

/// Tolerance used to identify identity matrices.
const IDENTITY_TOL: f64 = 1e-15;

/// Storage form of a matrix, used to select optimized transform paths.
///
/// This mirrors the C library's `form` field in `matrixmap.c`, which dispatches
/// between O(N) diagonal and O(N²) full matrix-vector products.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatrixForm {
    /// Full matrix: O(N²) matrix-vector product per point.
    Full,
    /// Diagonal matrix: O(N) per-axis multiply per point.
    Diagonal,
    /// Identity matrix: no elements stored in dump, mathematically a no-op.
    /// Preserved across round-trip so byte-exact serialisation matches C.
    Unit,
}

/// A linear mapping defined by a matrix multiplication.
///
/// Forward transform: `out = M × in` where `M` is an `nout × nin` matrix.
/// Inverse transform: `in = M⁻¹ × out` (only available for square, invertible
/// matrices).
#[derive(Debug, Clone)]
pub struct MatrixMap {
    /// The forward transformation matrix, shape (nout, nin).
    matrix: Array2<f64>,
    /// The inverse transformation matrix, shape (nin, nout), if available.
    inverse: Option<Array2<f64>>,
    /// Whether the mapping is currently inverted.
    inverted: bool,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    has_been_simplified: bool,
    /// Cached form of the forward matrix (Full or Diagonal).
    form: MatrixForm,
    /// Cached form of the inverse matrix (Full or Diagonal).
    inv_form: MatrixForm,
}

impl MatrixMap {
    /// Create a new `MatrixMap` from the given matrix.
    ///
    /// The matrix shape determines the number of inputs and outputs:
    /// - `nin = matrix.ncols()`
    /// - `nout = matrix.nrows()`
    ///
    /// For square matrices, the inverse is computed via Gauss-Jordan
    /// elimination.  For non-square matrices, the inverse is not available
    /// (calling `transform` with `forward=false` will return an error).
    pub fn new(matrix: Array2<f64>) -> AstResult<Self> {
        let nrows = matrix.nrows();
        let ncols = matrix.ncols();
        if nrows == 0 || ncols == 0 {
            return Err(AstError::new(ErrorCode::Mtrmt, "matrix must not be empty"));
        }

        let inverse = if nrows == ncols {
            // A singular square matrix — no inverse, but we allow creation.
            gauss_jordan_inverse(&matrix).ok()
        } else {
            None
        };

        let form = compute_form(&matrix);
        let inv_form = inverse.as_ref().map_or(MatrixForm::Full, compute_form);

        Ok(Self {
            matrix,
            inverse,
            inverted: false,
            has_been_simplified: false,
            form,
            inv_form,
        })
    }

    /// Construct a MatrixMap from an explicit forward matrix and inverse.
    ///
    /// Used when the caller has already computed a numerically clean inverse
    /// (e.g. the native→celestial rotation, whose inverse C accumulates via
    /// its own `astMtrRot` inverse sequence rather than by re-inverting the
    /// forward — see `wcs_celestial::build_rotation_from_pole`). Both matrices
    /// must be square and the same shape.
    pub(crate) fn new_with_inverse(matrix: Array2<f64>, inverse: Array2<f64>) -> AstResult<Self> {
        if matrix.nrows() == 0 || matrix.ncols() == 0 {
            return Err(AstError::new(ErrorCode::Mtrmt, "matrix must not be empty"));
        }
        if matrix.dim() != inverse.dim() {
            return Err(AstError::new(
                ErrorCode::Mtrmt,
                "matrix and inverse must have the same shape",
            ));
        }
        let form = compute_form(&matrix);
        let inv_form = compute_form(&inverse);
        Ok(Self {
            matrix,
            inverse: Some(inverse),
            inverted: false,
            has_been_simplified: false,
            form,
            inv_form,
        })
    }

    /// Returns a reference to the forward transformation matrix.
    pub(crate) fn forward_matrix(&self) -> &Array2<f64> {
        &self.matrix
    }

    /// Returns a reference to the inverse matrix, if available.
    pub(crate) fn inverse_matrix(&self) -> Option<&Array2<f64>> {
        self.inverse.as_ref()
    }

    /// Multiply this MatrixMap by another, returning the product `a × self`
    /// (i.e. `self` applied first, then `a`).
    ///
    /// Mirrors C `astMtrMult` (matrixmap.c:3526): the number of columns in
    /// `a` must equal the number of rows in `self`; the product has `a`'s
    /// row count and `self`'s column count. Both MatrixMaps' *effective*
    /// forward matrices (honouring their Invert flags) are used. `BAD_VALUE`
    /// in either factor propagates to `BAD_VALUE` in the corresponding
    /// product element.
    ///
    /// Errors with [`ErrorCode::Mtrml`] when either map lacks a forward
    /// transform or the shapes are incompatible.
    pub fn mtr_mult(&self, a: &MatrixMap) -> AstResult<MatrixMap> {
        // C requires both to have a defined forward transform. A MatrixMap
        // always has a forward transform in this crate (it is stored
        // directly), so the only failure mode is a shape mismatch.
        let this_matrix = self.effective_forward_matrix();
        let a_matrix = a.effective_forward_matrix();

        let nrow_this = this_matrix.nrows();
        let ncol_this = this_matrix.ncols();
        let nrow_a = a_matrix.nrows();
        let ncol_a = a_matrix.ncols();

        if ncol_a != nrow_this {
            return Err(AstError::new(
                ErrorCode::Mtrml,
                format!(
                    "astMtrMult: number of rows in the first MatrixMap ({nrow_this}) does not \
                     equal number of columns in the second MatrixMap ({ncol_a})"
                ),
            ));
        }

        // Product = a_matrix (nrow_a × ncol_a) · this_matrix (nrow_this ×
        // ncol_this), with ncol_a == nrow_this. Result is nrow_a × ncol_this.
        // Dot products short-circuit to BAD when either operand is BAD,
        // mirroring matrixmap.c:3726-3736.
        let mut product = Array2::<f64>::zeros((nrow_a, ncol_this));
        for row in 0..nrow_a {
            for col in 0..ncol_this {
                let mut sum = 0.0;
                let mut bad = false;
                for i in 0..ncol_a {
                    let av = a_matrix[[row, i]];
                    let tv = this_matrix[[i, col]];
                    if is_bad(av) || is_bad(tv) {
                        bad = true;
                        break;
                    }
                    sum += av * tv;
                }
                product[[row, col]] = if bad { BAD_VALUE } else { sum };
            }
        }

        MatrixMap::new(product)
    }

    /// Return a copy of this MatrixMap rotated by `theta` radians.
    ///
    /// Mirrors C `astMtrRot` (matrixmap.c:3766). Valid only for 2- or 3-row
    /// MatrixMaps. The current (effective) forward matrix is post-multiplied
    /// by the rotation matrix:
    /// - 2-row: clockwise rotation about the origin (from the +2 axis toward
    ///   the +1 axis); `axis` is ignored.
    /// - 3-row: clockwise rotation about `axis` (a 3-vector through the
    ///   origin) when looking along it.
    ///
    /// A rotation angle that is a multiple of 2π returns an unchanged copy.
    /// Errors with [`ErrorCode::Mtrax`] for a null/zero-length 3-D axis and
    /// [`ErrorCode::Mtr23`] for any row count other than 2 or 3.
    pub fn mtr_rot(&self, theta: f64, axis: &[f64]) -> AstResult<MatrixMap> {
        let costh = theta.cos();
        let sinth = theta.sin();

        // No change for a full-turn rotation (matrixmap.c:3845).
        if costh == 1.0 {
            return Ok(self.clone());
        }

        let cur = self.effective_forward_matrix();
        let nrow = cur.nrows();
        let ncol = cur.ncols();

        if nrow == 2 {
            // 2×2 forward rotation (clockwise). rotmat is row-major:
            // [[costh, sinth], [-sinth, costh]] (matrixmap.c:3860-3863).
            // Post-multiply: new = rotmat · cur (matrixmap.c SMtrMult post=1,
            // m=2, n=ncol gives rotmat[2×2] · cur[2×ncol]).
            let rotmat = [[costh, sinth], [-sinth, costh]];
            let mut new_mat = Array2::<f64>::zeros((2, ncol));
            // Post-multiply (SMtrMult post=1): new = rotmat · cur, i.e.
            // new[i][j] = Σ_k rotmat[i][k] · cur[k][j].
            for j in 0..ncol {
                for i in 0..2 {
                    let mut dot = 0.0;
                    let mut bad = false;
                    for k in 0..2 {
                        let rel = rotmat[i][k];
                        let cel = cur[[k, j]];
                        if is_bad(rel) || is_bad(cel) {
                            bad = true;
                            break;
                        }
                        dot += rel * cel;
                    }
                    new_mat[[i, j]] = if bad { BAD_VALUE } else { dot };
                }
            }
            return MatrixMap::new(new_mat);
        }

        if nrow == 3 {
            let axlen2 = if axis.len() >= 3 {
                axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]
            } else {
                0.0
            };
            if axlen2 <= 0.0 {
                return Err(AstError::new(
                    ErrorCode::Mtrax,
                    "astMtrRot: null or zero-length axis vector supplied",
                ));
            }
            let axlen = axlen2.sqrt();

            // Common terms (matrixmap.c:3910-3917).
            let as_ = sinth / axlen;
            let a = (1.0 - costh) / axlen2;
            let b = a * axis[0] * axis[1];
            let c = as_ * axis[2];
            let d = a * axis[0] * axis[2];
            let e = as_ * axis[1];
            let f = a * axis[1] * axis[2];
            let g = as_ * axis[0];

            // 3×3 forward rotation matrix (matrixmap.c:3921-3929), row-major.
            let rotmat = [
                [a * axis[0] * axis[0] + costh, b - c, d + e],
                [b + c, a * axis[1] * axis[1] + costh, f - g],
                [d - e, f + g, a * axis[2] * axis[2] + costh],
            ];
            let mut new_mat = Array2::<f64>::zeros((3, ncol));
            // Post-multiply (SMtrMult post=1): new = rotmat · cur.
            for j in 0..ncol {
                for i in 0..3 {
                    let mut dot = 0.0;
                    let mut bad = false;
                    for k in 0..3 {
                        let rel = rotmat[i][k];
                        let cel = cur[[k, j]];
                        if is_bad(rel) || is_bad(cel) {
                            bad = true;
                            break;
                        }
                        dot += rel * cel;
                    }
                    new_mat[[i, j]] = if bad { BAD_VALUE } else { dot };
                }
            }
            return MatrixMap::new(new_mat);
        }

        Err(AstError::new(
            ErrorCode::Mtr23,
            format!("astMtrRot: cannot rotate a {nrow}x{ncol} MatrixMap"),
        ))
    }

    /// The effective forward matrix, honouring the current Invert flag: the
    /// stored forward matrix when not inverted, else the inverse matrix
    /// (falling back to the forward matrix when no inverse exists).
    fn effective_forward_matrix(&self) -> Array2<f64> {
        if self.inverted {
            self.inverse.clone().unwrap_or_else(|| self.matrix.clone())
        } else {
            self.matrix.clone()
        }
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    /// Returns `true` if this matrix is the identity (within tolerance).
    fn is_identity(&self) -> bool {
        let n = self.matrix.nrows();
        if n != self.matrix.ncols() {
            return false;
        }
        for i in 0..n {
            for j in 0..n {
                let expected = if i == j { 1.0 } else { 0.0 };
                if (self.matrix[[i, j]] - expected).abs() > IDENTITY_TOL {
                    return false;
                }
            }
        }
        true
    }

    /// Apply the matrix `m` (shape `nout × nin`) to each point in `pts`
    /// and write results into `out`.
    ///
    /// Per-axis BAD handling: for each output row, only inputs with non-zero
    /// coefficients matter. If an input is BAD but its coefficient is zero,
    /// it is masked and does not propagate BAD.
    ///
    /// The `form` parameter selects the fast path:
    /// - `Diagonal`: O(N) per-axis multiply (each output depends only on the
    ///   corresponding input).
    /// - `Full`: O(N²) matrix-vector product (the general case).
    fn apply_matrix(m: &Array2<f64>, form: MatrixForm, pts: &PointSet, out: &mut PointSet) {
        // Math-wise Unit is just a special-case diagonal.
        let form = if matches!(form, MatrixForm::Unit) {
            MatrixForm::Diagonal
        } else {
            form
        };
        match form {
            MatrixForm::Unit => unreachable!(),
            MatrixForm::Diagonal => {
                let n = m.nrows();
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    for j in 0..n {
                        let diag = m[[j, j]];
                        if diag == 0.0 {
                            outp[j] = 0.0;
                        } else if is_bad(inp[j]) {
                            outp[j] = BAD_VALUE;
                        } else {
                            outp[j] = diag * inp[j];
                        }
                    }
                }
            }
            MatrixForm::Full => {
                let nout = m.nrows();
                let nin = m.ncols();
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    for row in 0..nout {
                        let mut sum = 0.0;
                        let mut bad = false;
                        for col in 0..nin {
                            let coeff = m[[row, col]];
                            if coeff != 0.0 {
                                if is_bad(inp[col]) {
                                    bad = true;
                                    break;
                                }
                                sum += coeff * inp[col];
                            }
                        }
                        outp[row] = if bad { BAD_VALUE } else { sum };
                    }
                }
            }
        }
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

impl Mapping for MatrixMap {
    fn nin(&self) -> usize {
        if self.inverted {
            self.matrix.nrows()
        } else {
            self.matrix.ncols()
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            self.matrix.ncols()
        } else {
            self.matrix.nrows()
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
        // Inversion invalidates the "already simplified" flag, since the
        // inverse may simplify further (C astSetInvert clears IsSimple).
        self.has_been_simplified = false;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        let (nout_coords, m, form) = if fwd {
            (self.matrix.nrows(), &self.matrix, self.form)
        } else {
            match &self.inverse {
                Some(inv) => (inv.nrows(), inv, self.inv_form),
                None => {
                    return Err(AstError::new(
                        ErrorCode::Nodef,
                        "MatrixMap inverse is not defined (non-square or singular matrix)",
                    ));
                }
            }
        };
        let mut out = PointSet::zeros(pts.npoint(), nout_coords);
        Self::apply_matrix(m, form, pts, &mut out);
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let (m, form) = if fwd {
            (&self.matrix, self.form)
        } else {
            match &self.inverse {
                Some(inv) => (inv, self.inv_form),
                None => {
                    return Err(AstError::new(
                        ErrorCode::Nodef,
                        "MatrixMap inverse is not defined (non-square or singular matrix)",
                    ));
                }
            }
        };
        Self::apply_matrix(m, form, pts, out);
        Ok(())
    }

    /// NaN-as-bad fast path. The "zero coefficient masks BAD" semantic of
    /// [`apply_matrix`] is preserved by keeping the `coeff != 0.0` guard:
    /// a NaN input in a column whose row has zero coefficient never
    /// participates in the dot product, so the row's output stays valid
    /// (matching the BAD-checked path). When the coefficient is non-zero,
    /// `coeff * NaN = NaN` and the row accumulator becomes NaN naturally.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let (m, form) = if fwd {
            (&self.matrix, self.form)
        } else {
            match &self.inverse {
                Some(inv) => (inv, self.inv_form),
                None => {
                    return Err(AstError::new(
                        ErrorCode::Nodef,
                        "MatrixMap inverse is not defined (non-square or singular matrix)",
                    ));
                }
            }
        };
        let form = if matches!(form, MatrixForm::Unit) {
            MatrixForm::Diagonal
        } else {
            form
        };
        match form {
            MatrixForm::Unit => unreachable!(),
            MatrixForm::Diagonal => {
                let n = m.nrows();
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    for j in 0..n {
                        let diag = m[[j, j]];
                        outp[j] = if diag == 0.0 { 0.0 } else { diag * inp[j] };
                    }
                }
            }
            MatrixForm::Full => {
                let nout = m.nrows();
                let nin = m.ncols();
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    for row in 0..nout {
                        let mut sum = 0.0;
                        for col in 0..nin {
                            let coeff = m[[row, col]];
                            if coeff != 0.0 {
                                sum += coeff * inp[col];
                            }
                        }
                        outp[row] = sum;
                    }
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.has_been_simplified {
            return Ok(Box::new(self.clone()));
        }

        let nrows = self.matrix.nrows();
        let ncols = self.matrix.ncols();

        // Only square matrices can reduce to simpler types.
        if nrows == ncols {
            if self.is_identity() {
                return Ok(Box::new(UnitMap::new(nrows)));
            }
            // Check for diagonal matrix with uniform scale → ZoomMap.
            if is_diagonal(&self.matrix) {
                let first = self.matrix[[0, 0]];
                if (1..nrows).all(|i| ast_equal(self.matrix[[i, i]], first)) {
                    if ast_equal(first, 1.0) {
                        return Ok(Box::new(UnitMap::new(nrows)));
                    }
                    if let Ok(mut zm) = ZoomMap::new(nrows, first) {
                        if self.inverted {
                            zm.invert();
                        }
                        return Ok(Box::new(zm));
                    }
                }
            }
        }

        // Full-form matrix whose off-diagonals are all zero re-stores as
        // Diagonal. The numeric matrix is unchanged; only `form` / `inv_form`
        // flip. Mirrors C matrixmap.c MapMerge "compress storage" pass.
        if matches!(self.form, MatrixForm::Full) && nrows == ncols && is_diagonal(&self.matrix) {
            let inv_is_diagonal = self.inverse.as_ref().is_none_or(is_diagonal);
            if inv_is_diagonal {
                let mut out = self.clone();
                out.form = MatrixForm::Diagonal;
                out.inv_form = MatrixForm::Diagonal;
                return Ok(Box::new(out));
            }
        }

        Ok(Box::new(self.clone()))
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

        // MatrixMap + MatrixMap → MatrixMap.
        if let Some(other_mat) = other.as_any().downcast_ref::<MatrixMap>() {
            // Get effective forward matrices, accounting for inversion.
            let a = if self_inv {
                self.inverse.as_ref()?.clone()
            } else {
                self.matrix.clone()
            };
            let b = if other_inv {
                other_mat.inverse.as_ref()?.clone()
            } else {
                other_mat.matrix.clone()
            };

            // Series: self first (matrix a), then other (matrix b).
            // Result = b * a. Requires b.ncols() == a.nrows().
            if b.ncols() != a.nrows() {
                return None;
            }

            let product = b.dot(&a);

            return if is_identity_matrix(&product) {
                Some(Box::new(UnitMap::new(product.nrows())))
            } else {
                MatrixMap::new(product)
                    .ok()
                    .map(|m| Box::new(m) as DynMapping)
            };
        }

        // MatrixMap + ZoomMap → MatrixMap (scaled).
        // ZoomMap commutes with MatrixMap: f*(M*x) = M*(f*x) = (f*M)*x.
        if let Some(zm) = other.as_any().downcast_ref::<ZoomMap>() {
            let f = if other_inv {
                1.0 / zm.factor()
            } else {
                zm.factor()
            };
            let a = if self_inv {
                self.inverse.as_ref()?.clone()
            } else {
                self.matrix.clone()
            };
            if a.ncols() != zm.nin() && a.nrows() != zm.nin() {
                return None;
            }
            let scaled = &a * f;
            if is_identity_matrix(&scaled) {
                return Some(Box::new(UnitMap::new(scaled.nrows())));
            }
            return MatrixMap::new(scaled)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }

        // MatrixMap + PermMap → MatrixMap (permuted).
        // Convention: self (MatrixMap) applied first, other (PermMap) applied second.
        // Result = P * A where P is the permutation matrix and A is self's matrix.
        // The PermMap permutes the rows (outputs) of the MatrixMap.
        if let Some(pm) = other.as_any().downcast_ref::<PermMap>() {
            let a = if self_inv {
                self.inverse.as_ref()?.clone()
            } else {
                self.matrix.clone()
            };

            // Get effective outperm: when other_inv is set, the PermMap's
            // forward direction uses inperm instead of outperm.
            let perm = if other_inv { pm.inperm() } else { pm.outperm() };
            let nout = perm.len();

            // Dimensions must match: MatrixMap output (a.nrows) == PermMap input (perm.len for fwd).
            let pm_nin = if other_inv {
                pm.outperm().len()
            } else {
                pm.inperm().len()
            };
            if a.nrows() != pm_nin {
                return None;
            }

            // Only handle pure permutations (all positive entries — no constants or BAD).
            if perm.iter().any(|&p| p <= 0) {
                return None;
            }

            // Build permutation matrix P: P[row, col] = 1 where perm[row] == col+1.
            let mut perm_mat = Array2::<f64>::zeros((nout, a.nrows()));
            for (row, &p) in perm.iter().enumerate() {
                let col = (p as usize) - 1;
                if col >= a.nrows() {
                    return None;
                }
                perm_mat[[row, col]] = 1.0;
            }

            // Result = P * A (PermMap applied after MatrixMap).
            let product = perm_mat.dot(&a);
            if is_identity_matrix(&product) {
                return Some(Box::new(UnitMap::new(product.nrows())));
            }
            return MatrixMap::new(product)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }

        // Diagonal MatrixMap + WinMap → WinMap (absorb diagonal scales).
        // Matches C matrixmap.c:3230 (MatWin2).
        if other.as_any().downcast_ref::<WinMap>().is_some() {
            let a = if self_inv {
                self.inverse.as_ref()?.clone()
            } else {
                self.matrix.clone()
            };
            if !is_diagonal(&a) {
                return None;
            }
            let n = a.nrows();
            // Extract effective (shifts, scales) from the WinMap.
            let (wshifts, wscales) = super::winmap::extract_linear_params(other, other_inv)?;
            if wshifts.len() != n {
                return None;
            }
            // Combined: out = wshift + wscale * (diag * x)
            //         = wshift + (wscale * diag) * x
            // Use compose_linear with s1=[0..], c1=diag, s2=wshifts, c2=wscales.
            let diag: Vec<f64> = (0..n).map(|i| a[[i, i]]).collect();
            let zeros = vec![0.0; n];
            return super::winmap::compose_linear(&zeros, &diag, &wshifts, &wscales);
        }

        None
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        let m = if self.inverted {
            match &self.inverse {
                Some(inv) => inv,
                None => return Ok(BAD_VALUE),
            }
        } else {
            &self.matrix
        };
        let row = ax1 - 1;
        let col = ax2 - 1;
        if row < m.nrows() && col < m.ncols() {
            Ok(m[[row, col]])
        } else {
            Ok(0.0)
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<MatrixMap>() else {
            return false;
        };
        // Compare effective forward matrices (resolving invert flags).
        // When inverted, the "effective forward" is the stored inverse matrix.
        let self_fwd = if self.inverted {
            match &self.inverse {
                Some(inv) => inv,
                None => return false,
            }
        } else {
            &self.matrix
        };
        let other_fwd = if other.inverted {
            match &other.inverse {
                Some(inv) => inv,
                None => return false,
            }
        } else {
            &other.matrix
        };
        if self_fwd.shape() != other_fwd.shape() {
            return false;
        }
        self_fwd
            .iter()
            .zip(other_fwd.iter())
            .all(|(a, b)| ast_equal(*a, *b))
    }

    fn map_split_zero_based(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors matrixmap.c::MapSplit (~2282). A MatrixMap split succeeds
        // only when the input/output dependency graph is block-diagonal w.r.t.
        // the chosen inputs: each output row's nonzero, non-BAD columns must
        // lie either entirely inside or entirely outside the kept-input set.
        //
        // C iterates rows of the *effective forward* matrix (i_matrix if the
        // MatrixMap is inverted, else f_matrix), classifying each row as
        // depending on selected and/or unselected inputs. Both 0.0 and
        // AST__BAD count as "no dependency". A row that depends on selected
        // inputs only is added to the result; a row that depends on both
        // returns NULL. The new MatrixMap is constructed with Invert=0 from
        // the effective-forward sub-matrix, so we do not propagate
        // is_inverted onto the sub-mapping.
        //
        // Project convention: reject duplicate or out-of-range axes (Task
        // 1.1 set this precedent for UnitMap / ZoomMap / ShiftMap; the C
        // function only checks the in-range half).
        let nin = self.nin();
        let nout = self.nout();
        crate::mappings::helpers::validate_in_axes(in_axes, nin)?;

        // Pick the effective forward matrix, matching C's `mat` selection.
        // When inverted but no inverse is available, the split is impossible
        // (C: `if( !mat || !astOK ) ok = 0`).
        let m: &Array2<f64> = if self.inverted {
            self.inverse.as_ref()?
        } else {
            &self.matrix
        };

        // Classify each output row.
        let kept: std::collections::HashSet<usize> = in_axes.iter().copied().collect();
        let mut kept_outs: Vec<usize> = Vec::new();
        let mut good = false;
        for j in 0..nout {
            let mut sel = false;
            let mut unsel = false;
            for i in 0..nin {
                let el = m[[j, i]];
                if el != 0.0 && !is_bad(el) {
                    if kept.contains(&i) {
                        sel = true;
                    } else {
                        unsel = true;
                    }
                }
            }
            if sel && unsel {
                // Cross-coupled: split impossible.
                return None;
            }
            if sel {
                kept_outs.push(j);
                // Track whether the row contributes any non-BAD value across
                // the kept columns (mirrors C's `good` flag).
                for &i in in_axes {
                    if !is_bad(m[[j, i]]) {
                        good = true;
                    }
                }
            }
        }

        // C requires nout > 0 && good (line 2499).
        if kept_outs.is_empty() || !good {
            return None;
        }

        // Build sub-matrix from kept_outs × in_axes.
        // Note: C's matrixmap.c:2478-2485 advances pmat only on astISGOOD,
        // leaving uninitialised heap in BAD slots. We write all kept entries
        // into a zero-initialised Array2 — see docs/issues/c-library-quirks.md.
        let mut sub_m = Array2::<f64>::zeros((kept_outs.len(), in_axes.len()));
        for (rj, &j) in kept_outs.iter().enumerate() {
            for (ri, &i) in in_axes.iter().enumerate() {
                sub_m[[rj, ri]] = m[[j, i]];
            }
        }
        let sub = MatrixMap::new(sub_m).ok()?;
        Some((kept_outs, Box::new(sub)))
    }

    fn has_been_simplified(&self) -> bool {
        self.has_been_simplified
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.has_been_simplified = flag;
    }

    // The inverse transformation exists only when the inverse matrix
    // could be computed (square and non-singular). Mirrors C
    // matrixmap.c GetTranForward / GetTranInverse.
    fn has_forward(&self) -> bool {
        if self.inverted {
            self.inverse.is_some()
        } else {
            true
        }
    }

    fn has_inverse(&self) -> bool {
        if self.inverted {
            true
        } else {
            self.inverse.is_some()
        }
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl Serializable for MatrixMap {
    fn class_name(&self) -> &'static str {
        "MatrixMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Matrix transformation".to_string());
        write_mapping_base(&mut node, self);

        let nout = self.matrix.nrows();
        let nin = self.matrix.ncols();

        match self.form {
            MatrixForm::Unit => {
                // Identity: no elements emitted.
            }
            MatrixForm::Diagonal => {
                let nel = nin.min(nout);
                for i in 0..nel {
                    let val = self.matrix[[i, i]];
                    if !is_bad(val) {
                        node.push_double(
                            &format!("M{i}"),
                            val,
                            Some("Forward matrix value".to_string()),
                        );
                    }
                }
            }
            MatrixForm::Full => {
                for i in 0..(nout * nin) {
                    let row = i / nin;
                    let col = i % nin;
                    let val = self.matrix[[row, col]];
                    if !is_bad(val) {
                        node.push_double(
                            &format!("M{i}"),
                            val,
                            Some("Forward matrix value".to_string()),
                        );
                    }
                }
            }
        }

        if let Some(ref inv) = self.inverse {
            match self.inv_form {
                MatrixForm::Unit => {
                    // Identity inverse: no elements emitted.
                }
                MatrixForm::Diagonal => {
                    let nel = nin.min(nout);
                    for i in 0..nel {
                        let val = inv[[i, i]];
                        if !is_bad(val) {
                            node.push_double(
                                &format!("IM{i}"),
                                val,
                                Some("Inverse matrix value".to_string()),
                            );
                        }
                    }
                }
                MatrixForm::Full => {
                    for i in 0..(nin * nout) {
                        let row = i / nout;
                        let col = i % nout;
                        let val = inv[[row, col]];
                        if !is_bad(val) {
                            node.push_double(
                                &format!("IM{i}"),
                                val,
                                Some("Inverse matrix value".to_string()),
                            );
                        }
                    }
                }
            }
        }

        let form_str = match self.form {
            MatrixForm::Unit => "Unit",
            MatrixForm::Diagonal => "Diagonal",
            MatrixForm::Full => "Full",
        };
        node.push_string("Form", form_str, Some("Matrix storage form".to_string()));
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);
        let nin = base.nin;
        let nout = base.nout;

        if nin == 0 || nout == 0 {
            return Err(AstError::new(
                ErrorCode::Lderr,
                "MatrixMap: zero-dimensional matrix",
            ));
        }

        let form = node.get_string("Form", "Full");

        // Read forward matrix based on storage form.
        let matrix = match form.as_str() {
            "Unit" => {
                // Identity matrix — no elements stored.
                Array2::eye(nin)
            }
            "Diagonal" => {
                // Only min(nout, nin) diagonal elements stored.
                let nel = nin.min(nout);
                let mut mat = Array2::zeros((nout, nin));
                for i in 0..nel {
                    mat[[i, i]] = node.get_double(&format!("M{i}"), 0.0);
                }
                mat
            }
            _ => {
                // "Full" — nout*nin elements in row-major order.
                let mut fwd_flat = Vec::with_capacity(nout * nin);
                for i in 0..(nout * nin) {
                    fwd_flat.push(node.get_double(&format!("M{i}"), BAD_VALUE));
                }
                Array2::from_shape_vec((nout, nin), fwd_flat).unwrap()
            }
        };

        // Read inverse matrix based on storage form.
        let inverse = match form.as_str() {
            "Unit" => {
                // Identity inverse — same as forward.
                Some(Array2::eye(nin))
            }
            "Diagonal" => {
                // Check if any IM elements exist.
                let nel = nin.min(nout);
                let has_inv = (0..nel).any(|i| node.get(&format!("IM{i}")).is_some());
                if has_inv {
                    let mut mat = Array2::zeros((nin, nout));
                    for i in 0..nel {
                        mat[[i, i]] = node.get_double(&format!("IM{i}"), 0.0);
                    }
                    Some(mat)
                } else {
                    None
                }
            }
            _ => {
                // "Full" — nin*nout elements.
                let mut has_inverse = false;
                let mut inv_flat = Vec::with_capacity(nin * nout);
                for i in 0..(nin * nout) {
                    let val = node.get_double(&format!("IM{i}"), BAD_VALUE);
                    inv_flat.push(val);
                    if !is_bad(val) {
                        has_inverse = true;
                    }
                }
                if has_inverse {
                    Some(Array2::from_shape_vec((nin, nout), inv_flat).unwrap())
                } else {
                    None
                }
            }
        };

        // If no inverse was stored in the file but the matrix is square,
        // compute it now (matching C library behaviour which always
        // recomputes the inverse on load).
        let inverse = match inverse {
            Some(inv) => Some(inv),
            None if matrix.nrows() == matrix.ncols() => gauss_jordan_inverse(&matrix).ok(),
            None => None,
        };

        // Preserve the storage form recorded in the source ("Unit" /
        // "Diagonal" / "Full") so the round-trip matches. For freshly-built
        // matrices we fall back to deriving the form from the matrix structure.
        let form_str = node.get_string("Form", "");
        let form = match form_str.as_str() {
            "Unit" => MatrixForm::Unit,
            "Diagonal" => MatrixForm::Diagonal,
            "Full" => MatrixForm::Full,
            _ => compute_form(&matrix),
        };
        // The inverse storage form follows the forward form when the source
        // pins it ("Unit"/"Diagonal"/"Full" all imply the inverse uses the
        // same layout); only fall back to deriving it from the inverse matrix
        // when no Form was recorded.
        let inv_form = match form_str.as_str() {
            "Unit" => MatrixForm::Unit,
            "Diagonal" => MatrixForm::Diagonal,
            "Full" => MatrixForm::Full,
            _ => inverse.as_ref().map_or(MatrixForm::Full, compute_form),
        };

        Ok(MatrixMap {
            matrix,
            inverse,
            inverted: base.inverted,
            has_been_simplified: base.has_been_simplified,
            form,
            inv_form,
        })
    }
}

/// Determine the storage form of a matrix (Diagonal or Full).
fn compute_form(m: &Array2<f64>) -> MatrixForm {
    if is_diagonal(m) {
        MatrixForm::Diagonal
    } else {
        MatrixForm::Full
    }
}

/// Check if a square matrix is diagonal (all off-diagonal elements are
/// zero within `ast_equal` tolerance).
pub(crate) fn is_diagonal(m: &Array2<f64>) -> bool {
    let n = m.nrows();
    if n != m.ncols() {
        return false;
    }
    for i in 0..n {
        for j in 0..n {
            if i != j && !ast_equal(m[[i, j]], 0.0) {
                return false;
            }
        }
    }
    true
}

/// Check if a matrix is the identity (within `ast_equal` tolerance).
pub(crate) fn is_identity_matrix_pub(m: &Array2<f64>) -> bool {
    is_identity_matrix(m)
}

/// Check if a matrix is the identity (within `ast_equal` tolerance).
fn is_identity_matrix(m: &Array2<f64>) -> bool {
    if m.nrows() != m.ncols() {
        return false;
    }
    let n = m.nrows();
    for i in 0..n {
        for j in 0..n {
            let expected = if i == j { 1.0 } else { 0.0 };
            if !ast_equal(m[[i, j]], expected) {
                return false;
            }
        }
    }
    true
}

/// Compute the inverse of a square matrix using Gauss-Jordan elimination
/// with partial pivoting.
///
/// Returns an error if the matrix is singular.
fn gauss_jordan_inverse(m: &Array2<f64>) -> AstResult<Array2<f64>> {
    let n = m.nrows();
    debug_assert_eq!(n, m.ncols(), "matrix must be square");

    // Build augmented matrix [M | I].
    let mut aug: Vec<f64> = Vec::with_capacity(n * 2 * n);
    for row in 0..n {
        for col in 0..n {
            aug.push(m[[row, col]]);
        }
        for col in 0..n {
            aug.push(if row == col { 1.0 } else { 0.0 });
        }
    }

    // Helper closures to index into the flat augmented matrix.
    let idx = |r: usize, c: usize| r * (2 * n) + c;

    for col in 0..n {
        // Find pivot row (partial pivoting).
        let pivot_row = (col..n).max_by(|&a, &b| {
            aug[idx(a, col)]
                .abs()
                .partial_cmp(&aug[idx(b, col)].abs())
                .unwrap()
        });
        let pivot_row = match pivot_row {
            Some(r) => r,
            None => {
                return Err(AstError::new(
                    ErrorCode::Nodef,
                    "matrix is singular and cannot be inverted",
                ));
            }
        };

        // Check that the pivot is non-zero.
        if aug[idx(pivot_row, col)].abs() < 1e-300 {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "matrix is singular and cannot be inverted",
            ));
        }

        // Swap rows if needed.
        if pivot_row != col {
            for c in 0..(2 * n) {
                aug.swap(idx(col, c), idx(pivot_row, c));
            }
        }

        // Scale pivot row so that the pivot element becomes 1.
        let pivot = aug[idx(col, col)];
        for c in 0..(2 * n) {
            aug[idx(col, c)] /= pivot;
        }

        // Eliminate column `col` from all other rows.
        for row in 0..n {
            if row != col {
                let factor = aug[idx(row, col)];
                if factor == 0.0 {
                    continue;
                }
                for c in 0..(2 * n) {
                    let sub = factor * aug[idx(col, c)];
                    aug[idx(row, c)] -= sub;
                }
            }
        }
    }

    // Extract the right half of the augmented matrix.
    let mut inv_flat = Vec::with_capacity(n * n);
    for row in 0..n {
        for col in n..(2 * n) {
            inv_flat.push(aug[idx(row, col)]);
        }
    }

    Ok(Array2::from_shape_vec((n, n), inv_flat).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn gauss_jordan_2x2() {
        // [[2, 0], [0, 4]] → [[0.5, 0], [0, 0.25]]
        let m = array![[2.0_f64, 0.0], [0.0, 4.0]];
        let inv = gauss_jordan_inverse(&m).unwrap();
        assert!((inv[[0, 0]] - 0.5).abs() < 1e-12);
        assert!((inv[[1, 1]] - 0.25).abs() < 1e-12);
        assert!(inv[[0, 1]].abs() < 1e-12);
        assert!(inv[[1, 0]].abs() < 1e-12);
    }

    #[test]
    fn gauss_jordan_singular() {
        let m = array![[1.0_f64, 2.0], [2.0, 4.0]];
        assert!(gauss_jordan_inverse(&m).is_err());
    }
}

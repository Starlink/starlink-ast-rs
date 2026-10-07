//! Linear matrix transformation mapping.
//!
//! `MatrixMap` applies a matrix multiplication to a set of input coordinates,
//! mapping `nin` inputs to `nout` outputs via an `nout × nin` matrix.
//! The inverse transformation is available for square non-singular matrices,
//! and for any matrix that reduces to diagonal or unit storage form.

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
use crate::mappings::SimplifiedRecord;

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
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
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
    /// The inverse is available whenever C would compute one: for a matrix that
    /// compresses to DIAGONAL or UNIT form always, and for one that stays FULL
    /// only when it is square and non-singular. Transforming in a direction
    /// with no inverse returns an error.
    ///
    /// The supplied matrix is always taken as a FULL matrix and then
    /// compressed, mirroring C `astInitMatrixMap` (matrixmap.c:6032-6073),
    /// which stores the caller's array, builds the inverse with
    /// `InvertMatrix`, and finally runs `CompressMatrix` on the result.
    pub fn new(matrix: Array2<f64>) -> AstResult<Self> {
        let nrows = matrix.nrows();
        let ncols = matrix.ncols();
        if nrows == 0 || ncols == 0 {
            return Err(AstError::new(ErrorCode::Mtrmt, "matrix must not be empty"));
        }

        // C reports AST__MTRMT when the whole supplied matrix is unusable
        // (matrixmap.c:6046-6061).
        if !matrix.iter().any(|&v| !is_bad(v)) {
            return Err(AstError::new(
                ErrorCode::Mtrmt,
                "supplied matrix contains only bad values",
            ));
        }

        // C builds the inverse from the *uncompressed* FULL matrix with
        // `InvertMatrix(FULL, ...)`, i.e. palDmat's SFA singularity gate; a
        // non-square matrix has no inverse (matrixmap.c:1554-1608).
        let mut matrix = matrix;
        let inverse = if nrows == ncols {
            gauss_jordan_inverse(&matrix).ok()
        } else {
            None
        };

        // ...and only then compresses (matrixmap.c:6072). Compression that
        // zeroes an off-diagonal element invalidates the inverse just built,
        // so C recomputes it from the compressed matrix — element-wise for a
        // now-DIAGONAL result, through palDmat again for a still-FULL one
        // (matrixmap.c:801-826). Only the FULL form needs a square matrix to be
        // invertible: DIAGONAL and UNIT store `min(nin, nout)` scale factors
        // whose reciprocals invert them whatever the shape, so a rectangular
        // matrix that compresses to one of those forms gains an inverse it did
        // not have as a FULL matrix (matrixmap.c:1523-1551).
        let mut form = MatrixForm::Full;
        let new_inv = compress_matrix(&mut matrix, &mut form);
        let inverse = match form {
            MatrixForm::Unit => Some(rect_eye(ncols, nrows)),
            MatrixForm::Diagonal => Some(diagonal_inverse(&matrix)),
            MatrixForm::Full if new_inv && nrows == ncols => gauss_jordan_inverse(&matrix).ok(),
            MatrixForm::Full => inverse,
        };
        // The stored inverse always uses the same layout as the forward
        // matrix; C keeps a single `form` field for both.
        let inv_form = form;

        Ok(Self {
            matrix,
            inverse,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
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
        // Compress the forward matrix as C `astInitMatrixMap` does, then follow
        // C in deriving the inverse for the compressed forms — a UNIT matrix
        // stores nothing and a DIAGONAL one is inverted element-wise. Only the
        // FULL case keeps the caller's inverse, which is the case this
        // constructor exists for: re-deriving it would perturb the last ULP of
        // the serialised `IMn` values.
        let mut matrix = matrix;
        let mut form = MatrixForm::Full;
        compress_matrix(&mut matrix, &mut form);
        let inverse = match form {
            MatrixForm::Unit => rect_eye(matrix.ncols(), matrix.nrows()),
            MatrixForm::Diagonal => diagonal_inverse(&matrix),
            MatrixForm::Full => inverse,
        };
        Ok(Self {
            matrix,
            inverse: Some(inverse),
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
            form,
            inv_form: form,
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

    /// Whether the matrix is stored in UNIT form (identity, no elements
    /// serialised). Mirrors C `mm->form == UNIT`.
    pub(crate) fn is_unit_form(&self) -> bool {
        matches!(self.form, MatrixForm::Unit)
    }

    /// Whether the matrix is stored in DIAGONAL form. Mirrors C
    /// `mm->form == DIAGONAL`.
    pub(crate) fn is_diagonal_form(&self) -> bool {
        matches!(self.form, MatrixForm::Diagonal)
    }

    /// Whether the matrix is stored in FULL form. Mirrors C
    /// `mm->form == FULL`.
    pub(crate) fn is_full_form(&self) -> bool {
        matches!(self.form, MatrixForm::Full)
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

    /// Return a copy of this MatrixMap scaled by `zoom`, applied in the
    /// effective direction `inv` (the direction the MatrixMap is used in
    /// within a merge). Faithful port of C `matrixmap.c::MtrZoom` (3988):
    /// copy the MatrixMap *preserving its orientation*, scale the current
    /// forward matrix by `zoom` and the current inverse by `1/zoom`. Keeping
    /// the orientation (rather than collapsing to a fresh forward matrix)
    /// matches C's serialised form and avoids recomputing an inverse.
    ///
    /// `zoom` is the effective forward zoom factor of the neighbouring
    /// ZoomMap in the composition.
    pub(crate) fn mtr_zoom(&self, zoom: f64, inv: bool) -> MatrixMap {
        // Current forward / inverse matrices for the effective orientation.
        let (cur_fwd, cur_inv) = if inv {
            (self.inverse.clone(), Some(self.matrix.clone()))
        } else {
            (Some(self.matrix.clone()), self.inverse.clone())
        };
        // Scale forward by zoom, inverse by 1/zoom.
        let new_fwd = cur_fwd.map(|m| &m * zoom);
        let new_inv = cur_inv.map(|m| &m * (1.0 / zoom));

        // Re-assemble into stored (f_matrix, i_matrix) honouring orientation:
        // when `inv`, the effective forward is the stored inverse.
        let (stored_fwd, stored_inv) = if inv {
            (new_inv, new_fwd)
        } else {
            (new_fwd, new_inv)
        };

        let mut out = match (stored_fwd, stored_inv) {
            (Some(f), Some(i)) => MatrixMap::new_with_inverse(f, i)
                .unwrap_or_else(|_| MatrixMap::new(self.matrix.clone()).unwrap()),
            (Some(f), None) => MatrixMap::new(f).unwrap(),
            _ => self.clone(),
        };
        if inv {
            out.inverted = true;
        }
        out
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

    /// Apply the matrix `m` (shape `nout × nin`) to each point in `pts`
    /// and write results into `out`.
    ///
    /// BAD handling follows `matrixmap.c::Transform` (5170-5283). A BAD matrix
    /// element always yields BAD output. A BAD input yields BAD output unless
    /// its coefficient is zero, in which case it is masked and does not
    /// propagate — this keeps a bad input axis out of output axes that do not
    /// depend on it. A zero coefficient is otherwise multiplied through
    /// normally; it is not a special case.
    ///
    /// The `form` parameter selects the fast path:
    /// - `Diagonal`: O(N) per-axis multiply (each output depends only on the
    ///   corresponding input).
    /// - `Full`: O(N²) matrix-vector product (the general case).
    ///
    /// For the diagonal and unit forms the matrix need not be square. C stores
    /// only `nax = min(nin, nout)` scale factors for those forms and writes
    /// zeros into any surplus output axes (matrixmap.c:5228-5283); the loop
    /// below does the same.
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
                let nax = m.nrows().min(m.ncols());
                let nout = m.nrows();
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    for j in 0..nax {
                        // A BAD diagonal element makes the whole output axis
                        // BAD; otherwise BAD input gives BAD output. C tests
                        // the diagonal once per axis, outside the point loop
                        // (matrixmap.c:5253); the loops are nested the other
                        // way here because a PointSet is row-major over points.
                        let diag = m[[j, j]];
                        outp[j] = if is_bad(diag) || is_bad(inp[j]) {
                            BAD_VALUE
                        } else {
                            diag * inp[j]
                        };
                    }
                    for o in outp.iter_mut().take(nout).skip(nax) {
                        *o = 0.0;
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
                            if is_bad(coeff) || (is_bad(inp[col]) && coeff != 0.0) {
                                bad = true;
                                break;
                            }
                            // Reaching here with a BAD input means the
                            // coefficient is zero, so the sum is unchanged.
                            if !is_bad(inp[col]) {
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
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

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
    /// `apply_matrix` is preserved by keeping the `coeff != 0.0` guard:
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
                let nax = m.nrows().min(m.ncols());
                let nout = m.nrows();
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let outp = out.point_mut(i);
                    for j in 0..nax {
                        let diag = m[[j, j]];
                        outp[j] = if diag == 0.0 { 0.0 } else { diag * inp[j] };
                    }
                    for o in outp.iter_mut().take(nout).skip(nax) {
                        *o = 0.0;
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
        //
        // Scale through [`MatrixMap::mtr_zoom`], which keeps an inverted
        // MatrixMap inverted with its stored values scaled, rather than
        // materialising the inverse into a fresh forward matrix. C does the
        // same, so the serialised orientation matches — and it avoids
        // recomputing an inverse, which would perturb the last ULP. This is the
        // generic-merge twin of the nominate-loop path in
        // `matrix_merge::try_direct_merge`; the two produced different
        // orientations until they were made to share this helper.
        if let Some(zm) = other.as_any().downcast_ref::<ZoomMap>() {
            let f = if other_inv {
                1.0 / zm.zoom()
            } else {
                zm.zoom()
            };
            let n = if self_inv { self.nin() } else { self.nout() };
            if zm.nin() != n {
                return None;
            }
            let scaled = self.mtr_zoom(f, self_inv);
            // Collapse to a UnitMap only when the whole Mapping is the
            // identity: an inverted result has to survive so its orientation
            // reaches serialisation.
            if !scaled.is_inverted() && is_identity_matrix(scaled.forward_matrix()) {
                return Some(Box::new(UnitMap::new(scaled.forward_matrix().nrows())));
            }
            return Some(Box::new(scaled) as DynMapping);
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
            // C merges a MatrixMap and a PermMap only via `MatPerm`, which is
            // gated by `PermOK` (matrixmap.c:1967, 4607): the PermMap must have
            // equal input and output counts. A dimension-changing PermMap
            // yields a rectangular product matrix; folding it into a matrix
            // would drop the always-defined PermMap inverse. Leave the pair as
            // a CmpMap in that case (mirrors the sibling guard in
            // `PermMap::try_merge`).
            if product.nrows() != product.ncols() {
                return None;
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
            // C merges a diagonal MatrixMap with a WinMap via MatWin2 /
            // WinDiagMat, which transform the terms without WinWin's
            // rounding snap.
            return super::winmap::compose_linear(&zeros, &diag, &wshifts, &wscales, false);
        }

        None
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        super::matrix_merge::matrix_map_merge(self, ctx, where_, series)
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

    // Mirrors C matrixmap.c `GetTranForward` (4892) / `GetTranInverse` (4973).
    // Both open by declaring every UNIT MatrixMap defined in both directions,
    // whatever its shape — a unit matrix stores no elements at all, so there is
    // no array whose absence could leave a direction undefined. Only otherwise
    // does the presence of the required element array decide it.
    fn has_forward(&self) -> bool {
        if matches!(self.form, MatrixForm::Unit) {
            return true;
        }
        if self.inverted {
            self.inverse.is_some()
        } else {
            true
        }
    }

    fn has_inverse(&self) -> bool {
        if matches!(self.form, MatrixForm::Unit) {
            return true;
        }
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
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("MatrixMap");
        let base = read_mapping_base(whole);
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
                rect_eye(nout, nin)
            }
            "Diagonal" => {
                // Only min(nout, nin) diagonal elements stored. A bad element
                // is not written, so a missing one reads as bad (matrixmap.c
                // Dump and Load).
                let nel = nin.min(nout);
                let mut mat = Array2::zeros((nout, nin));
                for i in 0..nel {
                    mat[[i, i]] = node.get_double(&format!("M{i}"), BAD_VALUE);
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
                Some(rect_eye(nin, nout))
            }
            "Diagonal" => {
                // Check if any IM elements exist.
                let nel = nin.min(nout);
                let has_inv = (0..nel).any(|i| node.get(&format!("IM{i}")).is_some());
                if has_inv {
                    // A missing diagonal IMi element defaults to AST__BAD on
                    // load (matrixmap.c: BAD inverse elements are not written
                    // and load back as AST__BAD), not to zero.
                    let mut mat = Array2::zeros((nin, nout));
                    for i in 0..nel {
                        mat[[i, i]] = node.get_double(&format!("IM{i}"), BAD_VALUE);
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

        // The storage form recorded in the source is authoritative, and the
        // inverse uses the same layout: C `astLoadMatrixMap` (matrixmap.c:6210)
        // reads `Form` into the single `form` field and never re-compresses, so
        // a dumped MatrixMap loads back in exactly the form it was written in.
        // An absent `Form` defaults to FULL (matrixmap.c:6219), not to a form
        // derived from the element values.
        let form = match form.as_str() {
            "Unit" => MatrixForm::Unit,
            "Diagonal" => MatrixForm::Diagonal,
            _ => MatrixForm::Full,
        };
        let inv_form = form;

        // When the dump carried no inverse elements, C rebuilds the inverse
        // with `InvertMatrix( new->form, ... )` (matrixmap.c:6279) — so the
        // form selects the arithmetic: element-wise reciprocals for DIAGONAL,
        // palDmat for FULL. (UNIT never reaches here; it stores no elements
        // and its inverse is the identity.)
        let inverse = match inverse {
            Some(inv) => Some(inv),
            None => match form {
                MatrixForm::Unit => Some(rect_eye(nin, nout)),
                MatrixForm::Diagonal => Some(diagonal_inverse(&matrix)),
                MatrixForm::Full if matrix.nrows() == matrix.ncols() => {
                    gauss_jordan_inverse(&matrix).ok()
                }
                MatrixForm::Full => None,
            },
        };

        Ok(MatrixMap {
            matrix,
            inverse,
            inverted: base.inverted,
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
            form,
            inv_form,
        })
    }
}

/// Reduce a matrix to its most compressed storage form, zeroing insignificant
/// off-diagonal elements in the process. Faithful port of C `CompressMatrix`
/// (matrixmap.c:587).
///
/// `form` is the form the matrix is currently stored in, and is updated to the
/// compressed form. The matrix itself is modified in place: off-diagonal
/// elements insignificant to *both* their own row and their own column are set
/// to exactly `0.0`, which is what turns the ~1e-16 trig residue of a
/// spherical-rotation product into the structural zeros C serialises.
///
/// Returns `true` when an off-diagonal element changed value, which is C's
/// `new_inv` flag: the caller must then recompute the inverse from the
/// compressed forward matrix, because the stored inverse no longer corresponds
/// to it.
///
/// The three branches mirror C exactly:
///
///  * `Unit` — already maximally compressed; nothing to do.
///  * `Diagonal` — collapses to `Unit` when every stored diagonal element is
///    `astEQUAL` to 1.0 (matrixmap.c:668-682). Off-diagonal elements are not
///    stored in this form, so none can be zeroed.
///  * `Full` — the row/column significance sweep (matrixmap.c:687-782). The
///    form starts at `Unit` and degrades: a `BAD_VALUE` element anywhere forces
///    `Full`; a diagonal element that is not exactly 1.0 drops `Unit` to
///    `Diagonal`; an off-diagonal element too large to zero forces `Full`.
///
/// Note the diagonal test in the `Full` branch is exact (`*a != 1.0`,
/// matrixmap.c:756) while the `Diagonal` branch's is tolerant (`astEQUAL`,
/// matrixmap.c:672) — a difference in the C that is preserved here.
fn compress_matrix(m: &mut Array2<f64>, form: &mut MatrixForm) -> bool {
    let nrow = m.nrows();
    let ncol = m.ncols();
    let ndiag = nrow.min(ncol);

    match *form {
        // Already maximally compressed (matrixmap.c:661).
        MatrixForm::Unit => false,

        // Diagonal storage: only the diagonal exists, so the sole question is
        // whether it is all ones (matrixmap.c:668-682).
        MatrixForm::Diagonal => {
            if (0..ndiag).all(|i| ast_equal(m[[i, i]], 1.0)) {
                *form = MatrixForm::Unit;
            }
            false
        }

        MatrixForm::Full => {
            // Largest magnitude in each row and column, scaled by
            // sqrt(DBL_EPSILON) to become the lower limit for a significant
            // value (matrixmap.c:691-724). BAD elements never participate.
            let tol = f64::EPSILON.sqrt();
            let mut rowmax = vec![0.0_f64; nrow];
            let mut colmax = vec![0.0_f64; ncol];
            for i in 0..nrow {
                for j in 0..ncol {
                    let v = m[[i, j]];
                    if !is_bad(v) {
                        let a = v.abs();
                        if a > rowmax[i] {
                            rowmax[i] = a;
                        }
                        if a > colmax[j] {
                            colmax[j] = a;
                        }
                    }
                }
            }
            for r in rowmax.iter_mut() {
                *r *= tol;
            }
            for c in colmax.iter_mut() {
                *c *= tol;
            }

            // Assume Unit and degrade (matrixmap.c:738-782).
            let mut new_form = MatrixForm::Unit;
            let mut new_inv = false;
            for k in 0..nrow {
                for j in 0..ncol {
                    let v = m[[k, j]];
                    if is_bad(v) {
                        new_form = MatrixForm::Full;
                    } else if j == k {
                        if v != 1.0 && new_form == MatrixForm::Unit {
                            new_form = MatrixForm::Diagonal;
                        }
                    } else {
                        let a = v.abs();
                        if a <= rowmax[k] && a <= colmax[j] {
                            if v != 0.0 {
                                new_inv = true;
                            }
                            m[[k, j]] = 0.0;
                        } else {
                            new_form = MatrixForm::Full;
                        }
                    }
                }
            }

            *form = new_form;
            new_inv
        }
    }
}

/// Run [`compress_matrix`] over a full-form matrix held outside a `MatrixMap`,
/// zeroing its insignificant off-diagonal elements.
///
/// C compresses such intermediate matrices in place inside `astMtrRot`
/// (matrixmap.c:3903, 3973), which records the resulting storage form on the
/// MatrixMap being rotated. A caller holding a bare array has nowhere to record
/// it, so both the updated form and the "inverse needs recomputing" flag are
/// discarded; whichever `MatrixMap` eventually receives the array derives its
/// own form from it.
pub(crate) fn compress_full_matrix(m: &mut Array2<f64>) {
    let mut form = MatrixForm::Full;
    compress_matrix(m, &mut form);
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

/// The `nrow × ncol` matrix with 1.0 on its leading diagonal and zeros
/// elsewhere — the explicit form of a UNIT MatrixMap.
///
/// C stores no elements at all for UNIT form and its `Transform` copies the
/// first `min(nin, nout)` input axes straight through (matrixmap.c:5239),
/// zero-filling any remaining outputs. Holding the equivalent matrix keeps
/// `nin`/`nout` in the array shape, as every other form here does.
fn rect_eye(nrow: usize, ncol: usize) -> Array2<f64> {
    let mut m = Array2::zeros((nrow, ncol));
    for i in 0..nrow.min(ncol) {
        m[[i, i]] = 1.0;
    }
    m
}

/// Invert a diagonal matrix element-wise, mirroring C's
/// `InvertMatrix(DIAGONAL, …)` (matrixmap.c:1525): each of the
/// `min(nrow, ncol)` diagonal elements is replaced by its reciprocal, with
/// zero or `BAD_VALUE` diagonals mapping to `BAD_VALUE`. Unlike the FULL path
/// this has no determinant threshold, so a near-singular-but-nonzero diagonal
/// still yields a stored inverse — and it applies to non-square matrices too,
/// since a diagonal matrix's inverse is well defined regardless of shape.
///
/// The result is transposed relative to the input (`ncol × nrow`), as the
/// inverse maps `nout` values back to `nin`.
fn diagonal_inverse(m: &Array2<f64>) -> Array2<f64> {
    let nrow = m.nrows();
    let ncol = m.ncols();
    let mut inv = Array2::zeros((ncol, nrow));
    for i in 0..nrow.min(ncol) {
        let d = m[[i, i]];
        inv[[i, i]] = if d != 0.0 && !is_bad(d) {
            1.0 / d
        } else {
            BAD_VALUE
        };
    }
    inv
}

/// Compute the inverse of a square matrix by delegating to the vendored
/// PAL routine `palDmat` (via [`crate::pal::dmat`]).
///
/// C `matrixmap.c::InvertMatrix` (matrixmap.c:1436) inverts a full matrix
/// with `palDmat`, and AST stores that result verbatim in a MatrixMap's
/// `IMn` dump fields. Byte-exact serialisation therefore requires the
/// *same* arithmetic, so we call the same C routine rather than a
/// Rust-side re-derivation (an independent Gauss-Jordan gives an inverse
/// equal to ~1e-16 but rounds near-zero elements differently, which is
/// visible in the native dump — e.g. `IM5` of a galactic rotation matrix).
///
/// Returns an error when `palDmat` reports the matrix singular (a pivot
/// magnitude below its internal `SFA = 1e-20`). `palDmat` also solves
/// `A·Y = X`; for a pure inverse the right-hand side is the zero vector,
/// so the solution is carried but unused.
fn gauss_jordan_inverse(m: &Array2<f64>) -> AstResult<Array2<f64>> {
    let n = m.nrows();
    debug_assert_eq!(n, m.ncols(), "matrix must be square");

    // Flatten to the row-major `n*n` block palDmat mutates in place.
    let mut a: Vec<f64> = Vec::with_capacity(n * n);
    for row in 0..n {
        for col in 0..n {
            a.push(m[[row, col]]);
        }
    }
    let mut y: Vec<f64> = vec![0.0; n];

    if !crate::pal::dmat(n, &mut a, &mut y) {
        return Err(AstError::new(
            ErrorCode::Nodef,
            "matrix is singular and cannot be inverted",
        ));
    }

    Ok(Array2::from_shape_vec((n, n), a).unwrap())
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

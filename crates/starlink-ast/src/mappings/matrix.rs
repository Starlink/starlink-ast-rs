// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Linear matrix transformation mapping.
//!
//! `MatrixMap` applies a matrix multiplication to a set of input coordinates,
//! mapping `nin` inputs to `nout` outputs via an `nout × nin` matrix.
//! The inverse transformation is available for square non-singular matrices
//! with no bad elements, and for any matrix that reduces to diagonal or unit
//! storage form.

use ndarray::Array2;

use crate::constants::BAD_VALUE;
use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{MXLOADEL, Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::perm::PermMap;
use super::unit::UnitMap;
use super::winmap::WinMap;

/// Points per pass of the full-matrix kernel for more than four input axes.
/// The size only tiles the loop; it cannot change any result.
const BLOCK: usize = 64;

/// One output axis of a full matrix with few input axes: `out[i]` is the
/// sum over the inputs, in order, of coefficient times value, or BAD when an
/// input with a non-zero coefficient is BAD (a BAD input through a zero
/// coefficient is ignored). With the input axes known, each point's sum
/// unrolls and the loop over points vectorizes. Adding 0.0 for a BAD input
/// leaves the sum unchanged, since it is never -0.0.
///
/// It is a macro so that the input axes are a tuple of slices, each cut to
/// the output length: the compiler can then drop every bounds check and
/// vectorize. A generic function over an array of slices keeps the checks
/// and runs about twice as slow. The BAD flag is built with `|` and `&`
/// rather than a branch for the same reason: a branch that sets a flag
/// stops the loop vectorizing.
macro_rules! full_row_fixed {
    ($coeffs:expr, $pin:expr, $out:expr, [$($c:tt),+]) => {{
        let n = $out.len();
        let coeffs = $coeffs;
        let x = ($( &$pin[$c][..n], )+);
        for (k, o) in $out.iter_mut().enumerate() {
            let mut sum = 0.0;
            let mut bad = false;
            $(
                let v = x.$c[k];
                let v_bad = is_bad(v);
                bad |= v_bad & (coeffs[$c] != 0.0);
                sum += if v_bad { 0.0 } else { coeffs[$c] * v };
            )+
            *o = if bad { BAD_VALUE } else { sum };
        }
    }};
}
use super::zoom::ZoomMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// Storage form of a MatrixMap's matrices, C's `form` field (matrixmap.c).
///
/// A MatrixMap stores only the elements its form needs, so a unit or diagonal
/// matrix of many axes costs no more than its diagonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatrixForm {
    /// Every element is stored, row by row.
    Full,
    /// Only the `min(nrow, ncol)` diagonal elements are stored; every other
    /// element is zero.
    Diagonal,
    /// No element is stored: the diagonal elements are 1 and the others zero.
    Unit,
}

/// One of a MatrixMap's two matrices as it is stored: the forward matrix,
/// with a row for each output and a column for each input, or the inverse,
/// with a row for each input and a column for each output.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StoredMatrix<'a> {
    form: MatrixForm,
    nrow: usize,
    ncol: usize,
    /// The stored elements: all of them, row by row, in the full form; the
    /// diagonal in the diagonal form; none in the unit form.
    elems: &'a [f64],
}

impl StoredMatrix<'_> {
    /// The number of rows.
    pub(crate) fn nrow(&self) -> usize {
        self.nrow
    }

    /// The number of diagonal elements, `min(nrow, ncol)`.
    fn ndiag(&self) -> usize {
        self.nrow.min(self.ncol)
    }

    /// The element in row `row` and column `col`, both 0-based.
    pub(crate) fn get(&self, row: usize, col: usize) -> f64 {
        match self.form {
            MatrixForm::Full => self.elems[row * self.ncol + col],
            MatrixForm::Diagonal if row == col => self.elems[row],
            MatrixForm::Unit if row == col => 1.0,
            _ => 0.0,
        }
    }

    /// Every element of the matrix, C's `ExpandMatrix` (matrixmap.c:1007).
    fn expand(&self) -> Array2<f64> {
        match self.form {
            MatrixForm::Full => Array2::from_shape_vec((self.nrow, self.ncol), self.elems.to_vec())
                .expect("a full matrix stores nrow * ncol elements"),
            _ => {
                let mut m = Array2::zeros((self.nrow, self.ncol));
                for i in 0..self.ndiag() {
                    m[[i, i]] = self.get(i, i);
                }
                m
            }
        }
    }

    /// The diagonal elements if the matrix is square and each element off
    /// the diagonal is zero within `ast_equal`, which a unit or diagonal
    /// matrix always is.
    pub(crate) fn square_diagonal(&self) -> Option<Vec<f64>> {
        if self.nrow != self.ncol {
            return None;
        }
        let n = self.nrow;
        if self.form == MatrixForm::Full {
            for i in 0..n {
                for j in 0..n {
                    if i != j && !ast_equal(self.get(i, j), 0.0) {
                        return None;
                    }
                }
            }
        }
        Some((0..n).map(|i| self.get(i, i)).collect())
    }

    /// Whether the matrix is square and equals the unit matrix within
    /// `ast_equal`.
    fn is_identity(&self) -> bool {
        self.square_diagonal()
            .is_some_and(|d| d.iter().all(|&v| ast_equal(v, 1.0)))
    }
}

/// Map coordinates by multiplying by a matrix.
///
/// A MatrixMap performs a general linear transformation.
/// Each set of input coordinates, regarded as a column vector, is
/// pre-multiplied by an `nout × nin` matrix, whose elements are given when the
/// MatrixMap is created, to give a new column vector holding the output
/// coordinates.
///
/// The forward transformation is always available.
/// The inverse transformation multiplies by the inverse matrix, and is
/// available when the matrix is diagonal or unit (see
/// [`diagonal`](MatrixMap::diagonal)), or is square, non-singular and free of
/// bad elements; [`has_inverse`](Mapping::has_inverse) reports which.
#[derive(Debug, Clone)]
pub struct MatrixMap {
    /// The number of columns of the forward matrix: the inputs when not
    /// inverted.
    ncol: usize,
    /// The number of rows of the forward matrix: the outputs when not
    /// inverted.
    nrow: usize,
    /// The storage form of both matrices.
    form: MatrixForm,
    /// The stored elements of the forward matrix (C's `f_matrix`): all
    /// `nrow * ncol`, row by row, in the full form, the diagonal in the
    /// diagonal form, none in the unit form.
    f_matrix: Vec<f64>,
    /// The stored elements of the inverse matrix (C's `i_matrix`), which has
    /// `ncol` rows and `nrow` columns, in the same form; `None` in the unit
    /// form, which needs none, and for a full matrix with no inverse.
    i_matrix: Option<Vec<f64>>,
    /// Whether the mapping is currently inverted.
    inverted: bool,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl MatrixMap {
    /// Create a MatrixMap from a full `nout × nin` matrix: the number of
    /// output coordinates is `matrix.nrows()` and the number of input
    /// coordinates `matrix.ncols()`.
    ///
    /// The inverse transformation is available when the matrix is square,
    /// non-singular and free of bad elements, or when it is diagonal: a matrix
    /// whose off-diagonal elements are zero, or negligible beside the other
    /// elements of their row and column, is stored and inverted as a diagonal
    /// one (and one whose diagonal elements are also exactly 1 as a unit one),
    /// whatever its shape.
    /// Transforming in a direction that is not available is an
    /// `ErrorCode::Trnnd` error.
    ///
    /// ```
    /// use starlink_ast::mappings::{Mapping, MatrixMap};
    /// use ndarray::array;
    ///
    /// // Swap and scale two axes.
    /// let map = MatrixMap::new(array![[0.0, 2.0], [3.0, 0.0]]).unwrap();
    /// let (x, y) = map.tran2(&[1.0], &[1.0], true).unwrap();
    /// assert_eq!((x, y), (vec![2.0], vec![3.0]));
    /// ```
    ///
    /// # Errors
    ///
    /// `ErrorCode::Mtrmt` if the matrix is empty or holds only bad values.
    #[doc(alias = "astMatrixMap")]
    pub fn new(matrix: Array2<f64>) -> AstResult<Self> {
        // The supplied matrix is always taken as a FULL matrix and then
        // compressed, mirroring C astInitMatrixMap (matrixmap.c:6032-6073),
        // which stores the caller's array, builds the inverse with
        // InvertMatrix, and finally runs CompressMatrix on the result.
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
        let inverse = if nrows == ncols {
            gauss_jordan_inverse_array(&matrix).ok()
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
        Ok(Self::compressed(matrix, inverse, true))
    }

    /// Create a MatrixMap with `nin` inputs and `nout` outputs whose matrix is
    /// diagonal, from its `min(nin, nout)` diagonal elements; every other
    /// element is zero.
    ///
    /// Passing 1.0 for every element gives a unit matrix.
    /// An infinite or NaN element is stored as [`BAD_VALUE`].
    ///
    /// The inverse transformation is always available: it multiplies by the
    /// reciprocal of each element, giving [`BAD_VALUE`] for a zero or bad one.
    /// When the matrix is not square, some input coordinates cannot be
    /// recovered from the output coordinates, and the inverse transformation
    /// sets them to zero.
    ///
    /// The result is a unit MatrixMap when every element equals 1.0 to within
    /// about 4e-11, a tolerance [`MatrixMap::new`] does not apply to a full
    /// matrix: diagonal values that differ from 1.0 in the last place give a
    /// unit MatrixMap here and a diagonal one there.
    ///
    /// ```
    /// use starlink_ast::coords::PointSet;
    /// use starlink_ast::mappings::{Mapping, MatrixMap};
    ///
    /// // Three inputs, two outputs: the third input is dropped.
    /// let map = MatrixMap::diagonal(3, 2, &[2.0, 4.0]).unwrap();
    /// assert_eq!((map.nin(), map.nout()), (3, 2));
    /// assert!(map.has_inverse());
    ///
    /// // The inverse zero-fills the dropped input.
    /// let out = map.transform(&PointSet::from_columns(&[&[2.0], &[8.0]]).unwrap(), false).unwrap();
    /// assert_eq!(out.axis(0), [1.0]);
    /// assert_eq!(out.axis(1), [2.0]);
    /// assert_eq!(out.axis(2), [0.0]);
    /// ```
    ///
    /// # Errors
    ///
    /// `ErrorCode::Mtrmt` if `diag` does not hold `min(nin, nout)` elements,
    /// that number is zero, or every element is bad.
    #[doc(alias = "astMatrixMap")]
    pub fn diagonal(nin: usize, nout: usize, diag: &[f64]) -> AstResult<Self> {
        // C's astMatrixMap given the diagonal form (matrixmap.c:6015-6076);
        // the unit test on the elements is astEQUAL (matrixmap.c:676).
        let ndiag = nin.min(nout);
        if ndiag == 0 || diag.len() != ndiag {
            return Err(AstError::new(
                ErrorCode::Mtrmt,
                format!(
                    "a diagonal MatrixMap with {nin} inputs and {nout} outputs needs {ndiag} \
                     diagonal elements, not {}",
                    diag.len()
                ),
            ));
        }
        let f_matrix: Vec<f64> = diag
            .iter()
            .map(|&v| if v.is_finite() { v } else { BAD_VALUE })
            .collect();
        if f_matrix.iter().all(|&v| is_bad(v)) {
            return Err(AstError::new(
                ErrorCode::Mtrmt,
                "supplied matrix contains only bad values",
            ));
        }
        // CompressMatrix of a diagonal matrix (matrixmap.c:695-712): it is a
        // unit matrix if every element equals 1 within astEQUAL, and is
        // otherwise inverted element by element.
        if f_matrix.iter().all(|&v| ast_equal(v, 1.0)) {
            return Ok(Self::from_parts(
                nin,
                nout,
                MatrixForm::Unit,
                Vec::new(),
                None,
            ));
        }
        let i_matrix = diagonal_inverse(&f_matrix);
        Ok(Self::from_parts(
            nin,
            nout,
            MatrixForm::Diagonal,
            f_matrix,
            Some(i_matrix),
        ))
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
        // FULL case keeps the caller's inverse, even where compression zeroes
        // an element, which is the case this constructor exists for:
        // re-deriving it would perturb the last ULP of the serialized `IMn`
        // values.
        Ok(Self::compressed(matrix, Some(inverse), false))
    }

    /// A MatrixMap holding the given stored matrices, not inverted.
    fn from_parts(
        ncol: usize,
        nrow: usize,
        form: MatrixForm,
        f_matrix: Vec<f64>,
        i_matrix: Option<Vec<f64>>,
    ) -> Self {
        Self {
            ncol,
            nrow,
            form,
            f_matrix,
            i_matrix,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }

    /// A MatrixMap holding the full forward matrix `matrix` and its inverse
    /// `inverse`, if any, reduced to its most compressed form by C's
    /// `CompressMatrix` (matrixmap.c:630) for a full matrix.
    ///
    /// An element off the diagonal that is insignificant to both its row and
    /// its column becomes zero. A matrix that is then unit keeps no elements,
    /// and one that is diagonal keeps its diagonal and is inverted element by
    /// element. A matrix that stays full keeps `inverse`, unless an element
    /// changed and `recompute` is set, when the inverse is found again from
    /// the changed matrix, as C does.
    fn compressed(mut matrix: Array2<f64>, inverse: Option<Array2<f64>>, recompute: bool) -> Self {
        let (nrow, ncol) = matrix.dim();
        let (form, new_inv) = compress_full(&mut matrix);
        match form {
            MatrixForm::Unit => Self::from_parts(ncol, nrow, form, Vec::new(), None),
            MatrixForm::Diagonal => {
                let f_matrix: Vec<f64> = (0..nrow.min(ncol)).map(|i| matrix[[i, i]]).collect();
                let i_matrix = diagonal_inverse(&f_matrix);
                Self::from_parts(ncol, nrow, form, f_matrix, Some(i_matrix))
            }
            MatrixForm::Full => {
                let f_matrix: Vec<f64> = matrix.iter().copied().collect();
                let i_matrix = if new_inv && recompute {
                    (nrow == ncol)
                        .then(|| gauss_jordan_inverse(&f_matrix, nrow).ok())
                        .flatten()
                } else {
                    inverse.map(|m| m.iter().copied().collect())
                };
                Self::from_parts(ncol, nrow, form, f_matrix, i_matrix)
            }
        }
    }

    /// The stored forward matrix, or with `inverse` set the stored inverse,
    /// in the MatrixMap's storage form; `None` for an inverse that is not
    /// defined. A unit MatrixMap has both.
    pub(crate) fn stored(&self, inverse: bool) -> Option<StoredMatrix<'_>> {
        let (nrow, ncol) = if inverse {
            (self.ncol, self.nrow)
        } else {
            (self.nrow, self.ncol)
        };
        let elems: &[f64] = match (self.form, inverse) {
            (MatrixForm::Unit, _) => &[],
            (_, false) => &self.f_matrix,
            (_, true) => self.i_matrix.as_deref()?,
        };
        Some(StoredMatrix {
            form: self.form,
            nrow,
            ncol,
            elems,
        })
    }

    /// Every element of the stored forward matrix, or with `inverse` set of
    /// the stored inverse; `None` for an inverse that is not defined.
    ///
    /// This is C's `ExpandMatrix` (matrixmap.c:1007), for the algorithms that
    /// work on a full matrix whatever the storage form.
    pub(crate) fn expanded(&self, inverse: bool) -> Option<Array2<f64>> {
        self.stored(inverse).map(|m| m.expand())
    }

    /// The diagonal of the stored forward matrix, or with `inverse` set of
    /// the stored inverse, if that matrix is defined and square and each of
    /// its elements off the diagonal is zero within `ast_equal`.
    pub(crate) fn square_diagonal(&self, inverse: bool) -> Option<Vec<f64>> {
        self.stored(inverse)?.square_diagonal()
    }

    /// Whether the stored forward matrix is square and equals the unit
    /// matrix within `ast_equal`.
    pub(crate) fn is_identity(&self) -> bool {
        self.stored(false).is_some_and(|m| m.is_identity())
    }

    /// Whether the matrix is stored in UNIT form (identity, no elements
    /// serialized). Mirrors C `mm->form == UNIT`.
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

    /// Whether the matrix this MatrixMap applies when used with Invert set
    /// to `invert` is defined and every element of it has a value.
    ///
    /// A unit matrix always does; the inverse of a singular matrix is not
    /// defined. Mirrors C's protected `astMtrUsable` (`FwdUsable`), which reads
    /// the stored forward or inverse matrix by `invert`, not by the Invert
    /// flag.
    pub(crate) fn mtr_usable(&self, invert: bool) -> bool {
        // Every stored element must have a value; a unit matrix stores none.
        self.stored(invert)
            .is_some_and(|m| m.elems.iter().all(|&v| v.is_finite() && !is_bad(v)))
    }

    /// The inputs on which the given outputs depend, in ascending order, if
    /// [`map_split`](Mapping::map_split) can split off exactly those outputs:
    /// each output depends on at least one input, and no other output
    /// depends on any of those inputs. `None` otherwise, or if an output index
    /// is out of range or repeated, or the matrix applied in the current
    /// direction is undefined. A zero or bad element is no dependence.
    ///
    /// Mirrors C's protected `astMtrSplitInputs`, which lets a CmpMap's
    /// split find the one subset of outputs a MatrixMap can give for a set of
    /// inputs without searching every subset.
    pub(crate) fn mtr_split_inputs(&self, outputs: &[usize]) -> Option<Vec<usize>> {
        // C expands the matrix (matrixmap.c astMtrSplitInputs); reading its
        // elements through the storage form gives the same values.
        let matrix = self.stored(self.inverted)?;
        let (nrow, ncol) = (matrix.nrow, matrix.ncol);
        if outputs.is_empty() {
            return None;
        }
        let mut wanted = vec![false; nrow];
        for &o in outputs {
            if o >= nrow || wanted[o] {
                return None;
            }
            wanted[o] = true;
        }
        let depends = |v: f64| v != 0.0 && !is_bad(v);
        let mut used = vec![false; ncol];
        for irow in (0..nrow).filter(|&r| wanted[r]) {
            let mut any = false;
            for (icol, used) in used.iter_mut().enumerate() {
                if depends(matrix.get(irow, icol)) {
                    *used = true;
                    any = true;
                }
            }
            if !any {
                return None;
            }
        }
        for irow in (0..nrow).filter(|&r| !wanted[r]) {
            if (0..ncol).any(|icol| used[icol] && depends(matrix.get(irow, icol))) {
                return None;
            }
        }
        Some((0..ncol).filter(|&c| used[c]).collect())
    }

    /// The product [`mtr_mult`](MatrixMap::mtr_mult) gives, or `None` if
    /// either matrix, in the direction its MatrixMap is used, is undefined
    /// or has an element with no value, or the product has one.
    ///
    /// Mirrors C's protected `astMtrProduct` (`Multiply` with `strict` set),
    /// which a merge uses so that a product it cannot use leaves the
    /// Mappings unmerged rather than reporting an error.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Mtrml`] when both matrices are usable but their shapes
    /// do not fit, which C reports whether or not `strict` is set.
    pub(crate) fn mtr_product(&self, a: &MatrixMap) -> AstResult<Option<MatrixMap>> {
        if !(self.mtr_usable(self.inverted) && a.mtr_usable(a.inverted)) {
            return Ok(None);
        }
        let product = self.mtr_mult(a)?;
        // An element of a unit or diagonal product that is not stored is
        // zero, so checking the stored elements checks them all.
        Ok(product
            .f_matrix
            .iter()
            .all(|&v| v.is_finite() && !is_bad(v))
            .then_some(product))
    }

    /// Multiply this MatrixMap by another, returning the product `a × self`
    /// (i.e. `self` applied first, then `a`).
    ///
    /// The number of columns in `a` must equal the number of rows in `self`;
    /// the product has `a`'s row count and `self`'s column count.
    /// Both MatrixMaps' *effective* forward matrices (honoring their Invert
    /// flags) are used.
    /// `BAD_VALUE` in either factor propagates to `BAD_VALUE` in the
    /// corresponding product element.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Mtrml`] when either MatrixMap has no forward
    /// transformation, as an inverted singular one has none, or when the
    /// shapes are incompatible.
    pub fn mtr_mult(&self, a: &MatrixMap) -> AstResult<MatrixMap> {
        // C astMtrMult (matrixmap.c:3526): both factors need a defined forward
        // transformation (matrixmap.c:3626-3639).
        for (factor, which) in [(self, "first"), (a, "second")] {
            if !factor.has_forward() {
                return Err(AstError::new(
                    ErrorCode::Mtrml,
                    format!(
                        "astMtrMult(MatrixMap): Cannot find the product of 2 MatrixMaps- the \
                         {which} MatrixMap has no forward transformation."
                    ),
                ));
            }
        }
        // C expands `this` to a full matrix (matrixmap.c Multiply) and reads
        // `a` in its storage form.
        let this_matrix = self.effective_forward_matrix();
        let a_matrix = a.applied_matrix();

        let nrow_this = this_matrix.nrows();
        let ncol_this = this_matrix.ncols();
        let nrow_a = a_matrix.nrow;
        let ncol_a = a_matrix.ncol;

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
        // ncol_this), with ncol_a == nrow_this. Result is nrow_a × ncol_this,
        // formed as C forms it for each storage form of `a`
        // (matrixmap.c:3670-3754): a unit `a` copies the first rows of
        // `this`, a diagonal one scales them, and a full one forms dot
        // products. Rows beyond those of `this` are zero, and a bad value
        // gives a bad element.
        let minrow = nrow_a.min(nrow_this);
        let mut product = Array2::<f64>::zeros((nrow_a, ncol_this));
        match a.form {
            MatrixForm::Unit => {
                for row in 0..minrow {
                    for col in 0..ncol_this {
                        product[[row, col]] = this_matrix[[row, col]];
                    }
                }
            }
            MatrixForm::Diagonal => {
                for row in 0..minrow {
                    let factor = a_matrix.get(row, row);
                    for col in 0..ncol_this {
                        let tv = this_matrix[[row, col]];
                        product[[row, col]] = if is_bad(factor) || is_bad(tv) {
                            BAD_VALUE
                        } else {
                            tv * factor
                        };
                    }
                }
            }
            MatrixForm::Full => product = mtr_product(&a_matrix.expand(), &this_matrix),
        }

        MatrixMap::new(product)
    }

    /// Return a copy of this MatrixMap scaled by `zoom`, applied in the
    /// effective direction `inv` (the direction the MatrixMap is used in
    /// within a merge). Faithful port of C `matrixmap.c::MtrZoom` (4217):
    /// copy the MatrixMap *preserving its orientation*, scale the current
    /// forward matrix by `zoom` and divide the current inverse by it. Keeping
    /// the orientation (rather than collapsing to a fresh forward matrix)
    /// matches C's serialized form and avoids recomputing an inverse.
    ///
    /// `zoom` is the effective forward zoom factor of the neighboring
    /// ZoomMap in the composition.
    pub(crate) fn mtr_zoom(&self, zoom: f64, inv: bool) -> MatrixMap {
        // C expands both matrices (matrixmap.c:4274), scales them, and
        // compresses the result.
        let mut f = self
            .expanded(false)
            .expect("the forward matrix is always stored");
        let mut i = self.expanded(true);
        // With `inv` set the matrix applied forward is the stored inverse.
        if inv {
            if let Some(i) = i.as_mut() {
                *i *= zoom;
            }
            f /= zoom;
        } else {
            f *= zoom;
            if let Some(i) = i.as_mut() {
                *i /= zoom;
            }
        }
        let mut out = Self::compressed(f, i, true);
        out.inverted = inv;
        out
    }

    /// Return a copy of this MatrixMap rotated by `theta` radians.
    ///
    /// Valid only for 2- or 3-row MatrixMaps.
    /// The current (effective) forward matrix is post-multiplied by the
    /// rotation matrix:
    /// - 2-row: clockwise rotation about the origin (from the +2 axis toward
    ///   the +1 axis); `axis` is ignored.
    /// - 3-row: clockwise rotation about `axis` (a 3-vector through the
    ///   origin) when looking along it.
    ///
    /// A rotation angle that is a multiple of 2π returns an unchanged copy.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Mtrax`] for a null or zero-length 3-D axis and
    /// [`ErrorCode::Mtr23`] for any row count other than 2 or 3.
    pub fn mtr_rot(&self, theta: f64, axis: &[f64]) -> AstResult<MatrixMap> {
        // C astMtrRot (matrixmap.c:3766).
        let costh = crate::math::cos(theta);
        let sinth = crate::math::sin(theta);

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
            let axlen = crate::math::sqrt(axlen2);

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

    /// The effective forward matrix, honoring the current Invert flag: the
    /// stored forward matrix when not inverted, else the inverse matrix
    /// (falling back to the forward matrix when no inverse exists).
    ///
    /// Every element is given, as C's `ExpandMatrix` gives them for
    /// `Multiply` and `MtrRot` (matrixmap.c:3822, 4101, 4152).
    fn effective_forward_matrix(&self) -> Array2<f64> {
        self.applied_matrix().expand()
    }

    /// The stored matrix the MatrixMap applies in its forward direction: the
    /// inverse when inverted, falling back to the forward matrix when no
    /// inverse exists.
    fn applied_matrix(&self) -> StoredMatrix<'_> {
        self.stored(self.inverted)
            .or_else(|| self.stored(false))
            .expect("the forward matrix is always stored")
    }

    /// The stored matrix that transforms in the direction `forward`, or a
    /// `Nodef` error if it is not defined.
    fn matrix_for(&self, forward: bool) -> AstResult<StoredMatrix<'_>> {
        self.stored(forward == self.inverted).ok_or_else(|| {
            AstError::new(
                ErrorCode::Nodef,
                "MatrixMap inverse is not defined (non-square or singular matrix)",
            )
        })
    }

    /// Apply the stored matrix `m` to each point in `pts` and write the
    /// results into `out`.
    ///
    /// BAD handling follows `matrixmap.c::Transform` (5170-5283). A BAD matrix
    /// element always yields BAD output. A BAD input yields BAD output unless
    /// its coefficient is zero, in which case it is masked and does not
    /// propagate — this keeps a bad input axis out of output axes that do not
    /// depend on it. A zero coefficient is otherwise multiplied through
    /// normally; it is not a special case.
    ///
    /// The storage form selects the path:
    /// - unit and diagonal: O(N) per-axis multiply (each output depends only
    ///   on the corresponding input), a unit matrix multiplying by 1.
    /// - full: O(N²) matrix-vector product (the general case).
    ///
    /// For the diagonal and unit forms the matrix need not be square. C stores
    /// only `nax = min(nin, nout)` scale factors for those forms and writes
    /// zeros into any surplus output axes (matrixmap.c:5228-5283); the loop
    /// below does the same.
    fn apply_matrix(m: StoredMatrix<'_>, pts: &Points<'_>, out: &mut PointsMut<'_>) {
        let n = pts.npoint();
        match m.form {
            MatrixForm::Unit | MatrixForm::Diagonal => {
                let nax = m.ndiag();
                for j in 0..nax {
                    // A BAD diagonal element makes the whole output axis
                    // BAD; otherwise BAD input gives BAD output. C tests the
                    // diagonal once per axis, outside the point loop
                    // (matrixmap.c:5253).
                    let diag = m.get(j, j);
                    let o = &mut out.axis_mut(j)[..n];
                    if is_bad(diag) {
                        o.fill(BAD_VALUE);
                        continue;
                    }
                    for (o, &v) in o.iter_mut().zip(pts.axis(j)) {
                        *o = if is_bad(v) { BAD_VALUE } else { diag * v };
                    }
                }
                for j in nax..m.nrow {
                    out.axis_mut(j)[..n].fill(0.0);
                }
            }
            MatrixForm::Full => {
                let pin = pts.axis_slices();
                // The rows are plain slices of the stored elements.
                for (coeffs, o) in m.elems.chunks_exact(m.ncol).zip(out.axes_iter_mut()) {
                    let o = &mut o[..n];
                    // A BAD element makes its whole row BAD.
                    if coeffs.iter().any(|&c| is_bad(c)) {
                        o.fill(BAD_VALUE);
                        continue;
                    }
                    // One to four inputs, which covers the matrices AST
                    // builds (direction cosines, linear WCS), get an unrolled
                    // loop. More inputs take a pass per input over a
                    // cache-sized block, which costs more per call but works
                    // for any count.
                    match coeffs.len() {
                        1 => full_row_fixed!(coeffs, pin, o, [0]),
                        2 => full_row_fixed!(coeffs, pin, o, [0, 1]),
                        3 => full_row_fixed!(coeffs, pin, o, [0, 1, 2]),
                        4 => full_row_fixed!(coeffs, pin, o, [0, 1, 2, 3]),
                        _ => full_row_blocked(coeffs, pin, o),
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

/// `full_row_fixed` for any number of input axes, a block of points at a time:
/// the output block holds the sums, then each input with a non-zero
/// coefficient marks its BAD points.
fn full_row_blocked(coeffs: &[f64], pin: &[&[f64]], out: &mut [f64]) {
    let n = out.len();
    for (start, sum) in (0..n).step_by(BLOCK).zip(out.chunks_mut(BLOCK)) {
        let end = start + sum.len();
        sum.fill(0.0);
        // Selects rather than branches, so the loops vectorize. Adding 0.0
        // leaves the sum unchanged, since it is never -0.0.
        for (&coeff, x) in coeffs.iter().zip(pin) {
            for (s, &v) in sum.iter_mut().zip(&x[start..end]) {
                *s += if is_bad(v) { 0.0 } else { coeff * v };
            }
        }
        for (&coeff, x) in coeffs.iter().zip(pin) {
            if coeff == 0.0 {
                continue;
            }
            for (s, &v) in sum.iter_mut().zip(&x[start..end]) {
                *s = if is_bad(v) { BAD_VALUE } else { *s };
            }
        }
    }
}

impl Mapping for MatrixMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    /// Always true: a MatrixMap is linear.
    // C's GetIsLinear for the class reports it linear.
    fn is_linear(&self) -> bool {
        true
    }

    fn nin(&self) -> usize {
        if self.inverted { self.nrow } else { self.ncol }
    }

    fn nout(&self) -> usize {
        if self.inverted { self.ncol } else { self.nrow }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let m = self.matrix_for(forward)?;
        let mut out = PointSet::zeros(pts.npoint(), m.nrow);
        Self::apply_matrix(m, &pts.as_points(), &mut out.as_points_mut());
        Ok(out)
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let m = self.matrix_for(forward)?;
        Self::apply_matrix(m, &pts, &mut out);
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
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let m = self.matrix_for(forward)?;
        let n = pts.npoint();
        match m.form {
            MatrixForm::Unit | MatrixForm::Diagonal => {
                let nax = m.ndiag();
                // A bad input stays bad even with a zero element, and a bad
                // element makes its axis bad (matrixmap.c Transform).
                for j in 0..nax {
                    let diag = m.get(j, j);
                    let o = &mut out.axis_mut(j)[..n];
                    if diag == BAD_VALUE {
                        o.fill(f64::NAN);
                        continue;
                    }
                    for (o, &v) in o.iter_mut().zip(pts.axis(j)) {
                        *o = diag * v;
                    }
                }
                for j in nax..m.nrow {
                    out.axis_mut(j)[..n].fill(0.0);
                }
            }
            MatrixForm::Full => {
                for (row, coeffs) in m.elems.chunks_exact(m.ncol).enumerate() {
                    // A bad element makes its row bad (matrixmap.c
                    // Transform).
                    if coeffs.contains(&BAD_VALUE) {
                        out.axis_mut(row)[..n].fill(f64::NAN);
                        continue;
                    }
                    // The output axis holds the sums, each over the input
                    // axes in order.
                    let sum = &mut out.axis_mut(row)[..n];
                    sum.fill(0.0);
                    for (col, &coeff) in coeffs.iter().enumerate() {
                        if coeff == 0.0 {
                            continue;
                        }
                        for (s, &v) in sum.iter_mut().zip(pts.axis(col)) {
                            *s += coeff * v;
                        }
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
            let a = self.stored(self_inv)?;
            let b = other_mat.stored(other_inv)?;

            // Series: self first (matrix a), then other (matrix b).
            // Result = b * a. Requires b.ncols() == a.nrows().
            if b.ncol != a.nrow {
                return None;
            }

            // C's MatMat: `astMtrMult` of the two in the direction each is
            // applied, self first.
            let mut first = self.clone();
            if self_inv {
                first.invert();
            }
            let mut second = other_mat.clone();
            if other_inv {
                second.invert();
            }
            let product = first.mtr_mult(&second).ok()?;

            return if product.is_identity() {
                Some(Box::new(UnitMap::within_limit(product.nrow)))
            } else {
                Some(Box::new(product))
            };
        }

        // MatrixMap + ZoomMap → MatrixMap (scaled).
        // ZoomMap commutes with MatrixMap: f*(M*x) = M*(f*x) = (f*M)*x.
        //
        // This is the merge a nominated ZoomMap makes with a neighboring
        // MatrixMap (zoommap.c MapMerge), so it scales through
        // [`MatrixMap::mtr_zoom`], C's astMtrZoom, which keeps an inverted
        // MatrixMap inverted with its stored values scaled rather than
        // recomputing an inverse. A nominated MatrixMap merges with a
        // ZoomMap through C's MatZoom instead
        // (`matrix_merge::merge_matrix_pair`), a product with a diagonal
        // MatrixMap whose inverse is computed afresh.
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
            // reaches serialization.
            if !scaled.is_inverted() && scaled.is_identity() {
                return Some(Box::new(UnitMap::within_limit(scaled.nrow)));
            }
            return Some(Box::new(scaled) as DynMapping);
        }

        // MatrixMap + PermMap → MatrixMap (permuted).
        // Convention: self (MatrixMap) applied first, other (PermMap) applied second.
        // Result = P * A where P is the permutation matrix and A is self's matrix.
        // The PermMap permutes the rows (outputs) of the MatrixMap.
        if let Some(pm) = other.as_any().downcast_ref::<PermMap>() {
            let a = self.expanded(self_inv)?;

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
            let product = mtr_product(&perm_mat, &a);
            if is_identity_matrix(&product) {
                return Some(Box::new(UnitMap::within_limit(product.nrows())));
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
            let diag = self.square_diagonal(self_inv)?;
            let n = diag.len();
            // Extract effective (shifts, scales) from the WinMap.
            let (wshifts, wscales) = super::winmap::extract_linear_params(other, other_inv)?;
            if wshifts.len() != n {
                return None;
            }
            // Combined: out = wshift + wscale * (diag * x)
            //         = wshift + (wscale * diag) * x
            // Use compose_linear with s1=[0..], c1=diag, s2=wshifts, c2=wscales.
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
        crate::mappings::validate_rate(self, _at, ax1, ax2)?;
        let Some(m) = self.stored(self.inverted) else {
            return Ok(BAD_VALUE);
        };
        let row = ax1 - 1;
        let col = ax2 - 1;
        if row < m.nrow && col < m.ncol {
            Ok(m.get(row, col))
        } else {
            Ok(0.0)
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<MatrixMap>() else {
            return false;
        };
        // matrixmap.c Equal: compare the matrices of the direction each
        // applies forward, or, when either lacks one, of the other direction;
        // MatrixMaps of the same shape with neither pair available are equal.
        if self.nin() != other.nin() || self.nout() != other.nout() {
            return false;
        }
        let pair = match (self.stored(self.inverted), other.stored(other.inverted)) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => self
                .stored(!self.inverted)
                .zip(other.stored(!other.inverted)),
        };
        // C compares every element of the two matrices expanded to full
        // form. An element off the diagonal of a unit or diagonal matrix is
        // zero, so when neither matrix is full only the diagonals can differ.
        match pair {
            Some((a, b)) if a.form != MatrixForm::Full && b.form != MatrixForm::Full => {
                (0..a.ndiag()).all(|i| ast_equal(a.get(i, i), b.get(i, i)))
            }
            Some((a, b)) => {
                (0..a.nrow).all(|r| (0..a.ncol).all(|c| ast_equal(a.get(r, c), b.get(r, c))))
            }
            None => true,
        }
    }

    fn map_split_zero_based(
        &self,
        in_axes: &[usize],
    ) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        // C allocates the returned output indices with astMalloc, which
        // gives NULL for none, so an empty selection gives no split.
        if in_axes.is_empty() {
            return Ok(None);
        }
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
        // The parent's MapSplit runs first: an input out of range is
        // AST__AXIIN, and a selection of every input, each once, returns this
        // MatrixMap whole, Invert and inverse matrix included. A repeated
        // input selects its column again, as in C.
        if let Some(r) = crate::mappings::helpers::base_map_split(self, in_axes)? {
            return Ok(Some(r));
        }
        let nin = self.nin();
        let nout = self.nout();

        // Pick the effective forward matrix, matching C's `mat` selection.
        // When inverted but no inverse is available, the split is impossible
        // (C: `if( !mat || !astOK ) ok = 0`).
        // C expands the matrix (matrixmap.c:2483); reading its elements
        // through the storage form gives the same values.
        let Some(m) = self.stored(self.inverted) else {
            return Ok(None);
        };

        // Classify each output row.
        let kept: std::collections::HashSet<usize> = in_axes.iter().copied().collect();
        let mut kept_outs: Vec<usize> = Vec::new();
        let mut good = false;
        for j in 0..nout {
            let mut sel = false;
            let mut unsel = false;
            for i in 0..nin {
                let el = m.get(j, i);
                if el != 0.0 && !is_bad(el) {
                    if kept.contains(&i) {
                        sel = true;
                    } else {
                        unsel = true;
                    }
                }
            }
            if sel && !unsel {
                kept_outs.push(j);
                // A BAD element does not count as a dependency above, so a
                // kept row can have one in a selected column; C declines the
                // split then, rather than build a matrix with a gap in it.
                for &i in in_axes {
                    if is_bad(m.get(j, i)) {
                        return Ok(None);
                    }
                    good = true;
                }
            }
            if sel && unsel {
                // Cross-coupled: split impossible.
                return Ok(None);
            }
        }

        // C requires nout > 0 && good.
        if kept_outs.is_empty() || !good {
            return Ok(None);
        }

        // Build sub-matrix from kept_outs × in_axes.
        let mut sub_m = Array2::<f64>::zeros((kept_outs.len(), in_axes.len()));
        for (rj, &j) in kept_outs.iter().enumerate() {
            for (ri, &i) in in_axes.iter().enumerate() {
                sub_m[[rj, ri]] = m.get(j, i);
            }
        }
        let sub = MatrixMap::new(sub_m)?;
        Ok(Some((kept_outs, Box::new(sub))))
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
            self.i_matrix.is_some()
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
            self.i_matrix.is_some()
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

        // The stored elements of each matrix, numbered from 0 as C's Dump
        // numbers them (matrixmap.c:5857): every element row by row for a
        // full matrix, the diagonal for a diagonal one, none for a unit one.
        // A bad element is not written, and reads back as bad.
        let stored = [
            ("M", "Forward matrix value", Some(&self.f_matrix)),
            ("IM", "Inverse matrix value", self.i_matrix.as_ref()),
        ];
        for (prefix, comment, elems) in stored {
            for (i, &val) in elems.into_iter().flatten().enumerate() {
                if !is_bad(val) {
                    node.push_double(&format!("{prefix}{i}"), val, Some(comment.to_string()));
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

        // The storage form recorded in the source is authoritative, and the
        // inverse uses the same layout: C `astLoadMatrixMap` (matrixmap.c:6401)
        // reads `Form` into the single `form` field and never re-compresses, so
        // a dumped MatrixMap loads back in exactly the form it was written in.
        // An absent `Form` defaults to FULL, not to a form derived from the
        // element values.
        let form = match node.get_string("Form", "Full").as_str() {
            "Unit" => MatrixForm::Unit,
            "Diagonal" => MatrixForm::Diagonal,
            _ => MatrixForm::Full,
        };

        // The number of elements stored for each matrix. A dump names a full
        // matrix's elements `M0` to `M9999998`, so one that claims more was
        // not written by AST, and nothing is read or sized for it
        // (matrixmap.c astLoadMatrixMap, `MXLOADEL`). A diagonal matrix
        // stores no more elements than a Mapping has axes.
        let nel = match form {
            MatrixForm::Full => {
                if nin.saturating_mul(nout) > MXLOADEL {
                    return Err(AstError::new(
                        ErrorCode::Lderr,
                        format!(
                            "astLoadMatrixMap({}): The full matrix read has {nout} rows and \
                             {nin} columns, but a dump cannot hold more than {MXLOADEL} \
                             elements.",
                            whole.class
                        ),
                    ));
                }
                nin * nout
            }
            MatrixForm::Diagonal => nin.min(nout),
            MatrixForm::Unit => 0,
        };

        // A bad element is not written, so a missing one reads as bad
        // (matrixmap.c Dump and Load).
        let read = |prefix: &str| -> Vec<f64> {
            (0..nel)
                .map(|i| node.get_double(&format!("{prefix}{i}"), BAD_VALUE))
                .collect()
        };
        let f_matrix = read("M");

        // The inverse is the one dumped if any of its elements were written.
        // C rebuilds it otherwise with `InvertMatrix( new->form, ... )`, so the
        // form selects the arithmetic: element-wise reciprocals for DIAGONAL,
        // palDmat for FULL. A unit matrix stores neither.
        let i_matrix = match form {
            MatrixForm::Unit => None,
            MatrixForm::Diagonal => {
                if (0..nel).any(|i| node.get(&format!("IM{i}")).is_some()) {
                    Some(read("IM"))
                } else {
                    Some(diagonal_inverse(&f_matrix))
                }
            }
            MatrixForm::Full => {
                let dumped = read("IM");
                if dumped.iter().any(|&v| !is_bad(v)) {
                    Some(dumped)
                } else if nin == nout {
                    gauss_jordan_inverse(&f_matrix, nin).ok()
                } else {
                    None
                }
            }
        };

        let mut map = Self::from_parts(nin, nout, form, f_matrix, i_matrix);
        map.inverted = base.inverted;
        map.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        Ok(map)
    }
}

/// The storage form a full matrix compresses to, zeroing insignificant
/// off-diagonal elements in the process: the full-matrix branch of C's
/// `CompressMatrix` (matrixmap.c:630).
///
/// The matrix is modified in place: off-diagonal elements insignificant to
/// *both* their own row and their own column are set to exactly `0.0`, which
/// is what turns the ~1e-16 trig residue of a spherical-rotation product into
/// the structural zeros C serializes.
///
/// Also returns `true` when an off-diagonal element changed value, which is
/// C's `new_inv` flag: the inverse built from the matrix as it was no longer
/// corresponds to it.
///
/// The row/column significance sweep starts at `Unit` and degrades: a
/// `BAD_VALUE` element anywhere forces `Full`; a diagonal element that is not
/// exactly 1.0 drops `Unit` to `Diagonal`; an off-diagonal element too large
/// to zero forces `Full`.
///
/// The diagonal test here is exact (`*a != 1.0`), while C's test of a matrix
/// already stored as diagonal, in [`MatrixMap::diagonal`], is tolerant
/// (`astEQUAL`) — a difference in the C that is preserved here.
fn compress_full(m: &mut Array2<f64>) -> (MatrixForm, bool) {
    let nrow = m.nrows();
    let ncol = m.ncols();
    // Largest magnitude in each row and column, scaled by
    // sqrt(DBL_EPSILON) to become the lower limit for a significant
    // value (matrixmap.c:691-724). BAD elements never participate.
    let tol = crate::math::sqrt(f64::EPSILON);
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

    (new_form, new_inv)
}

/// The product `a · this` of two full matrices, formed as C's `MtrMult`
/// forms it for a full `a` (matrixmap.c `MtrMult`): each element a sum
/// accumulated from zero in column order, and bad wherever a term has a bad
/// factor. The summation order is what keeps the last digits C's, so a
/// product C forms through `astMtrMult` is formed here rather than by a
/// library matrix product.
///
/// This is a choice of C compatibility over speed, not a requirement of the
/// mathematics. A library product such as ndarray's `dot`, which may block
/// or vectorize the sum, gives the same values to within rounding and could
/// be faster for large matrices. Switching to one would move merged
/// MatrixMaps by a few units in the last place, which the simplify corpus,
/// the FITS-WCS write oracles and anything else compared with C would
/// report, so it needs a deliberate decision to give up byte parity there,
/// backed by a benchmark. The matrices merged here are usually 2×2 or 3×3,
/// where a plain loop is unlikely to lose.
pub(crate) fn mtr_product(a: &Array2<f64>, this: &Array2<f64>) -> Array2<f64> {
    let mut product = Array2::<f64>::zeros((a.nrows(), this.ncols()));
    for row in 0..a.nrows() {
        for col in 0..this.ncols() {
            let mut sum = 0.0;
            let mut bad = false;
            for i in 0..a.ncols() {
                let av = a[[row, i]];
                let tv = this[[i, col]];
                if is_bad(av) || is_bad(tv) {
                    bad = true;
                    break;
                }
                sum += av * tv;
            }
            product[[row, col]] = if bad { BAD_VALUE } else { sum };
        }
    }
    product
}

/// Run [`compress_full`] over a full-form matrix held outside a `MatrixMap`,
/// zeroing its insignificant off-diagonal elements.
///
/// C compresses such intermediate matrices in place inside `astMtrRot`
/// (matrixmap.c:3903, 3973), which records the resulting storage form on the
/// MatrixMap being rotated. A caller holding a bare array has nowhere to record
/// it, so both the updated form and the "inverse needs recomputing" flag are
/// discarded; whichever `MatrixMap` eventually receives the array derives its
/// own form from it.
pub(crate) fn compress_full_matrix(m: &mut Array2<f64>) {
    compress_full(m);
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

/// Invert a diagonal matrix, given as its diagonal elements, element-wise,
/// mirroring C's `InvertMatrix(DIAGONAL, …)` (matrixmap.c:1486): each
/// element is replaced by its reciprocal, with zero or `BAD_VALUE` elements
/// mapping to `BAD_VALUE`. Unlike the FULL path this has no determinant
/// threshold, so a near-singular-but-nonzero diagonal still yields a stored
/// inverse — and it applies to non-square matrices too, since a diagonal
/// matrix's inverse is well defined regardless of shape.
fn diagonal_inverse(diag: &[f64]) -> Vec<f64> {
    diag.iter()
        .map(|&d| {
            if d != 0.0 && !is_bad(d) {
                1.0 / d
            } else {
                BAD_VALUE
            }
        })
        .collect()
}

/// Compute the inverse of a square matrix by delegating to the port of the
/// PAL routine `palDmat` ([`crate::pal::dmat`]).
///
/// C `matrixmap.c::InvertMatrix` (matrixmap.c:1436) inverts a full matrix
/// with `palDmat`, and AST stores that result verbatim in a MatrixMap's
/// `IMn` dump fields. Byte-exact serialization therefore requires the
/// *same* arithmetic, so we call the port of the same C routine rather than a
/// re-derivation (an independent Gauss-Jordan gives an inverse
/// equal to ~1e-16 but rounds near-zero elements differently, which is
/// visible in the native dump — e.g. `IM5` of a galactic rotation matrix).
///
/// Returns an error when the matrix holds a bad element, which C checks
/// before inverting (matrixmap.c:1568-1578), or when `palDmat` reports it
/// singular (a pivot magnitude below its internal `SFA = 1e-20`). `palDmat` also solves
/// `A·Y = X`; for a pure inverse the right-hand side is the zero vector,
/// so the solution is carried but unused.
///
/// `m` holds the `n * n` elements of the matrix row by row, and so does the
/// result.
fn gauss_jordan_inverse(m: &[f64], n: usize) -> AstResult<Vec<f64>> {
    debug_assert_eq!(m.len(), n * n, "matrix must be square");
    if m.contains(&BAD_VALUE) {
        return Err(AstError::new(
            ErrorCode::Nodef,
            "a matrix holding a bad element has no inverse",
        ));
    }

    // A copy of the row-major `n*n` block, which palDmat mutates in place.
    let mut a = m.to_vec();
    let mut y: Vec<f64> = vec![0.0; n];

    if !crate::pal::dmat(n, &mut a, &mut y) {
        return Err(AstError::new(
            ErrorCode::Nodef,
            "matrix is singular and cannot be inverted",
        ));
    }

    Ok(a)
}

/// [`gauss_jordan_inverse`] of a square matrix held as an array.
fn gauss_jordan_inverse_array(m: &Array2<f64>) -> AstResult<Array2<f64>> {
    let n = m.nrows();
    let elems: Vec<f64> = m.iter().copied().collect();
    let inv = gauss_jordan_inverse(&elems, n)?;
    Ok(Array2::from_shape_vec((n, n), inv).expect("an n by n inverse"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn gauss_jordan_2x2() {
        // [[2, 0], [0, 4]] → [[0.5, 0], [0, 0.25]]
        let m = array![[2.0_f64, 0.0], [0.0, 4.0]];
        let inv = gauss_jordan_inverse_array(&m).unwrap();
        assert!((inv[[0, 0]] - 0.5).abs() < 1e-12);
        assert!((inv[[1, 1]] - 0.25).abs() < 1e-12);
        assert!(inv[[0, 1]].abs() < 1e-12);
        assert!(inv[[1, 0]].abs() < 1e-12);
    }

    #[test]
    fn gauss_jordan_singular() {
        let m = array![[1.0_f64, 2.0], [2.0, 4.0]];
        assert!(gauss_jordan_inverse_array(&m).is_err());
    }

    /// A diagonal matrix that is not square is scaled like any other.
    #[test]
    fn mtr_zoom_scales_a_rectangular_diagonal_matrix() {
        let map = MatrixMap::diagonal(3, 2, &[2.0, 4.0]).unwrap();
        let zoomed = map.mtr_zoom(3.0, false);
        assert!(zoomed.is_diagonal_form());
        assert_eq!(zoomed.f_matrix, [6.0, 12.0]);
        assert_eq!(
            zoomed.i_matrix.as_deref(),
            Some(&[1.0 / 6.0, 1.0 / 12.0][..])
        );
        let zoomed = map.mtr_zoom(3.0, true);
        assert!(zoomed.is_inverted());
        assert_eq!(zoomed.f_matrix, [2.0 / 3.0, 4.0 / 3.0]);
    }

    /// The stored elements of each form, and the matrix they expand to.
    #[test]
    fn storage_forms() {
        let unit = MatrixMap::diagonal(2, 3, &[1.0, 1.0]).unwrap();
        assert!(unit.is_unit_form());
        assert!(unit.f_matrix.is_empty() && unit.i_matrix.is_none());
        assert_eq!(
            unit.expanded(true).unwrap(),
            array![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        let diag = MatrixMap::new(array![[2.0, 0.0], [0.0, 4.0], [0.0, 0.0]]).unwrap();
        assert!(diag.is_diagonal_form());
        assert_eq!(diag.f_matrix, [2.0, 4.0]);
        assert_eq!(diag.i_matrix.as_deref(), Some(&[0.5, 0.25][..]));
        assert_eq!(
            diag.expanded(false).unwrap(),
            array![[2.0, 0.0], [0.0, 4.0], [0.0, 0.0]]
        );
        let full = MatrixMap::new(array![[1.0, 2.0], [3.0, 4.0]]).unwrap();
        assert!(full.is_full_form());
        assert_eq!(full.f_matrix, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(full.i_matrix.as_ref().map(Vec::len), Some(4));
    }

    /// As C's strict `Multiply`: an unusable matrix or product gives no
    /// product, but matrices whose shapes do not fit are still an error.
    #[test]
    fn mtr_product_reports_a_shape_mismatch() {
        let two_by_three = MatrixMap::new(array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]).unwrap();
        let square = MatrixMap::new(array![[1.0, 2.0], [3.0, 4.0]]).unwrap();
        let err = square.mtr_product(&two_by_three).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::Mtrml);
        let mut singular = MatrixMap::new(array![[1.0, 1.0], [1.0, 1.0]]).unwrap();
        singular.invert();
        assert!(singular.mtr_product(&square).unwrap().is_none());
        assert!(square.mtr_product(&square).unwrap().is_some());
    }
}

#[cfg(test)]
mod split_inputs_tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn mtr_split_inputs_finds_the_inputs_of_a_separable_block() {
        let m = MatrixMap::new(array![[1.0, 2.0, 0.0], [3.0, 4.0, 0.0], [0.0, 0.0, 5.0]]).unwrap();
        assert_eq!(m.mtr_split_inputs(&[2]), Some(vec![2]));
        assert_eq!(m.mtr_split_inputs(&[1, 0]), Some(vec![0, 1]));
        // Output 0 shares input 1 with output 1, which is not asked for.
        assert_eq!(m.mtr_split_inputs(&[0]), None);
        assert_eq!(m.mtr_split_inputs(&[0, 0]), None);
        assert_eq!(m.mtr_split_inputs(&[3]), None);
        assert_eq!(m.mtr_split_inputs(&[]), None);
    }

    #[test]
    fn mtr_split_inputs_uses_the_matrix_of_the_current_direction() {
        // The forward matrix couples output 1 to input 0; its inverse does
        // not couple output 0 to input 1.
        let mut m = MatrixMap::new(array![[1.0, 0.0], [1.0, 1.0]]).unwrap();
        assert_eq!(m.mtr_split_inputs(&[0]), None);
        m.invert();
        assert_eq!(m.mtr_split_inputs(&[1]), None);
        assert_eq!(m.mtr_split_inputs(&[0, 1]), Some(vec![0, 1]));
        let mut singular = MatrixMap::new(array![[1.0, 1.0], [1.0, 1.0]]).unwrap();
        singular.invert();
        assert_eq!(singular.mtr_split_inputs(&[0, 1]), None);
    }

    #[test]
    fn mtr_split_inputs_treats_a_bad_element_as_no_dependence() {
        let m = MatrixMap::new(array![[1.0, BAD_VALUE], [0.0, 2.0]]).unwrap();
        assert_eq!(m.mtr_split_inputs(&[0]), Some(vec![0]));
        // An output whose only element is bad depends on nothing.
        let m = MatrixMap::new(array![[BAD_VALUE, 0.0], [0.0, 2.0]]).unwrap();
        assert_eq!(m.mtr_split_inputs(&[0]), None);
    }
}

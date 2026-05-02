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
use super::helpers::{is_bad, point_has_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// Tolerance used to identify identity matrices.
const IDENTITY_TOL: f64 = 1e-15;

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

        Ok(Self {
            matrix,
            inverse,
            inverted: false,
        })
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
    fn apply_matrix(m: &Array2<f64>, pts: &PointSet, out: &mut PointSet) {
        let nout = m.nrows();
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else {
                for row in 0..nout {
                    let mut sum = 0.0;
                    for (col, &x) in inp.iter().enumerate() {
                        sum += m[[row, col]] * x;
                    }
                    outp[row] = sum;
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
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        let (nout_coords, m) = if fwd {
            (self.matrix.nrows(), &self.matrix)
        } else {
            match &self.inverse {
                Some(inv) => (inv.nrows(), inv),
                None => {
                    return Err(AstError::new(
                        ErrorCode::Nodef,
                        "MatrixMap inverse is not defined (non-square or singular matrix)",
                    ));
                }
            }
        };
        let mut out = PointSet::zeros(pts.npoint(), nout_coords);
        Self::apply_matrix(m, pts, &mut out);
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let m = if fwd {
            &self.matrix
        } else {
            match &self.inverse {
                Some(inv) => inv,
                None => {
                    return Err(AstError::new(
                        ErrorCode::Nodef,
                        "MatrixMap inverse is not defined (non-square or singular matrix)",
                    ));
                }
            }
        };
        Self::apply_matrix(m, pts, out);
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.is_identity() {
            Ok(Box::new(UnitMap::new(self.matrix.nrows())))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
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

        // Forward matrix elements in row-major order.
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

        // Inverse matrix elements, if available.
        if let Some(ref inv) = self.inverse {
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

        node.push_string("Form", "Full", Some("Matrix storage form".to_string()));
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (nin, nout, inverted) = read_mapping_base(node);

        if nin == 0 || nout == 0 {
            return Err(AstError::new(
                ErrorCode::Lderr,
                "MatrixMap: zero-dimensional matrix",
            ));
        }

        // Read forward matrix (default BAD_VALUE for missing elements).
        let mut fwd_flat = Vec::with_capacity(nout * nin);
        for i in 0..(nout * nin) {
            fwd_flat.push(node.get_double(&format!("M{i}"), BAD_VALUE));
        }
        let matrix = Array2::from_shape_vec((nout, nin), fwd_flat).unwrap();

        // Read inverse matrix.
        let mut has_inverse = false;
        let mut inv_flat = Vec::with_capacity(nin * nout);
        for i in 0..(nin * nout) {
            let val = node.get_double(&format!("IM{i}"), BAD_VALUE);
            inv_flat.push(val);
            if !is_bad(val) {
                has_inverse = true;
            }
        }
        let inverse = if has_inverse {
            Some(Array2::from_shape_vec((nin, nout), inv_flat).unwrap())
        } else {
            None
        };

        Ok(MatrixMap {
            matrix,
            inverse,
            inverted,
        })
    }
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

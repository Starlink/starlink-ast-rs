//! Frame-unaware coordinate block.

use ndarray::{Array2, ArrayView2, ArrayViewMut2};

use crate::error::{AstError, AstResult, ErrorCode};

/// Frame-unaware coordinate block.
///
/// Row-major layout: shape = `(npoint, ncoord)`. Each row is one point;
/// each column is one coordinate axis.
#[derive(Debug, Clone)]
pub struct PointSet {
    data: Array2<f64>,
}

impl PointSet {
    /// Create a PointSet filled with zeros.
    pub fn zeros(npoint: usize, ncoord: usize) -> Self {
        Self {
            data: Array2::zeros((npoint, ncoord)),
        }
    }

    /// Create from an iterator of points (each point is a slice of coordinates).
    ///
    /// All points must have the same length. Panics if the iterator is empty.
    pub fn from_rows(rows: impl IntoIterator<Item = impl AsRef<[f64]>>) -> Self {
        let rows: Vec<Vec<f64>> = rows.into_iter().map(|r| r.as_ref().to_vec()).collect();
        assert!(!rows.is_empty(), "PointSet::from_rows requires at least one row");
        let ncoord = rows[0].len();
        let npoint = rows.len();
        let flat: Vec<f64> = rows.into_iter().flatten().collect();
        let data = Array2::from_shape_vec((npoint, ncoord), flat).unwrap();
        Self { data }
    }

    /// Create from column slices (one slice per coordinate axis).
    ///
    /// All columns must have the same length.
    pub fn from_columns(cols: &[&[f64]]) -> AstResult<Self> {
        if cols.is_empty() {
            return Err(AstError::new(
                ErrorCode::Ncoin,
                "from_columns requires at least one column",
            ));
        }
        let npoint = cols[0].len();
        for (i, col) in cols.iter().enumerate() {
            if col.len() != npoint {
                return Err(AstError::new(
                    ErrorCode::Ncpin,
                    format!(
                        "column {i} has length {} but expected {npoint}",
                        col.len()
                    ),
                ));
            }
        }
        let ncoord = cols.len();
        let mut data = Array2::zeros((npoint, ncoord));
        for (j, col) in cols.iter().enumerate() {
            for (i, &val) in col.iter().enumerate() {
                data[[i, j]] = val;
            }
        }
        Ok(Self { data })
    }

    /// Create from a pre-built ndarray.
    pub fn from_ndarray(a: Array2<f64>) -> Self {
        Self { data: a }
    }

    /// Number of points.
    pub fn npoint(&self) -> usize {
        self.data.nrows()
    }

    /// Number of coordinates per point.
    pub fn ncoord(&self) -> usize {
        self.data.ncols()
    }

    /// View the underlying array.
    pub fn view(&self) -> ArrayView2<'_, f64> {
        self.data.view()
    }

    /// Mutable view of the underlying array.
    pub fn view_mut(&mut self) -> ArrayViewMut2<'_, f64> {
        self.data.view_mut()
    }

    /// Consume and return the underlying ndarray.
    pub fn into_ndarray(self) -> Array2<f64> {
        self.data
    }

    /// Access a single point by index.
    ///
    /// Panics if `i >= npoint()`.
    pub fn point(&self, i: usize) -> &[f64] {
        self.data
            .row(i)
            .to_slice()
            .expect("row is contiguous in C-order array")
    }

    /// Mutable access to a single point by index.
    ///
    /// Panics if `i >= npoint()`.
    pub fn point_mut(&mut self, i: usize) -> &mut [f64] {
        self.data
            .row_mut(i)
            .into_slice()
            .expect("row is contiguous in C-order array")
    }

    /// Iterate over values in a single coordinate axis (column).
    ///
    /// `j` is 0-based (PointSet is frame-unaware and uses raw array indices).
    /// Panics if `j >= ncoord()`.
    pub fn axis_iter(&self, j: usize) -> impl Iterator<Item = f64> + '_ {
        // Use rows iterator to avoid borrow-of-temporary: we extract element j
        // from each row, keeping the borrow rooted in `self`.
        self.data.rows().into_iter().map(move |row| row[j])
    }
}

//! Frame-unaware coordinate block.

use ndarray::{Array2, ArrayView2, ArrayViewMut2};

use super::points::{PointBuf, PointMut, PointShape, Points, PointsMut, WritePoint};
use crate::error::{AstError, AstResult, ErrorCode};

/// Frame-unaware coordinate block.
///
/// One contiguous array per axis, as C's PointSet stores them: the data is
/// an `(ncoord, npoint)` array in standard layout, so axis `j` is row `j`.
#[derive(Debug, Clone)]
pub struct PointSet {
    data: Array2<f64>,
}

impl PointSet {
    /// Create a PointSet filled with zeros.
    pub fn zeros(npoint: usize, ncoord: usize) -> Self {
        Self {
            data: Array2::zeros((ncoord, npoint)),
        }
    }

    /// Create from an iterator of points (each point is a slice of coordinates).
    ///
    /// All points must have the same length. Panics if the iterator is empty.
    /// When the iterator knows its exact length the points are written
    /// straight into place; otherwise they are gathered first and
    /// transposed, which needs twice the memory.
    pub fn from_rows(rows: impl IntoIterator<Item = impl AsRef<[f64]>>) -> Self {
        let mut rows = rows.into_iter();
        let exact = match rows.size_hint() {
            (lo, Some(hi)) if lo == hi => Some(lo),
            _ => None,
        };
        let first = rows
            .next()
            .expect("PointSet::from_rows requires at least one row");
        let first = first.as_ref();
        let ncoord = first.len();
        let check = |row: &[f64]| {
            assert_eq!(
                row.len(),
                ncoord,
                "PointSet::from_rows needs every point to have {ncoord} coordinates"
            );
        };
        if let Some(npoint) = exact {
            let mut out = Self::zeros(npoint, ncoord);
            out.set_point(0, first);
            for (i, row) in rows.enumerate() {
                let row = row.as_ref();
                check(row);
                out.set_point(i + 1, row);
            }
            return out;
        }
        let mut flat = first.to_vec();
        let mut npoint = 1;
        for row in rows {
            let row = row.as_ref();
            check(row);
            flat.extend_from_slice(row);
            npoint += 1;
        }
        let by_point = Array2::from_shape_vec((npoint, ncoord), flat).unwrap();
        Self {
            data: by_point.reversed_axes().as_standard_layout().into_owned(),
        }
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
                    format!("column {i} has length {} but expected {npoint}", col.len()),
                ));
            }
        }
        let mut data = Array2::zeros((cols.len(), npoint));
        for (mut row, col) in data.rows_mut().into_iter().zip(cols) {
            row.as_slice_mut()
                .expect("standard layout")
                .copy_from_slice(col);
        }
        Ok(Self { data })
    }

    /// A copy of the points of a view.
    pub fn from_points(p: &Points<'_>) -> Self {
        let mut out = Self::zeros(p.npoint(), p.ncoord());
        out.as_points_mut().copy_from(p);
        out
    }

    /// Create from an `(ncoord, npoint)` array.
    pub fn from_ndarray(a: Array2<f64>) -> Self {
        Self {
            data: a.as_standard_layout().into_owned(),
        }
    }

    /// Number of points.
    pub fn npoint(&self) -> usize {
        self.data.ncols()
    }

    /// Number of coordinates per point.
    pub fn ncoord(&self) -> usize {
        self.data.nrows()
    }

    /// The `(ncoord, npoint)` array.
    pub fn view(&self) -> ArrayView2<'_, f64> {
        self.data.view()
    }

    /// The `(ncoord, npoint)` array, writable.
    pub fn view_mut(&mut self) -> ArrayViewMut2<'_, f64> {
        self.data.view_mut()
    }

    /// Consume and return the `(ncoord, npoint)` array.
    pub fn into_ndarray(self) -> Array2<f64> {
        self.data
    }

    /// A copy of point `i`.
    ///
    /// Panics if `i >= npoint()`.
    #[inline]
    pub fn point(&self, i: usize) -> PointBuf {
        let npoint = self.npoint();
        assert!(i < npoint, "point {i} of a PointSet of {npoint}");
        let data = self.data.as_slice().expect("standard layout");
        PointBuf::gather(self.ncoord(), |j| data[j * npoint + i])
    }

    /// Set point `i`; `coords` must have one value per axis.
    ///
    /// Panics if `i >= npoint()`.
    #[inline]
    pub fn set_point(&mut self, i: usize, coords: &[f64]) {
        assert_eq!(
            coords.len(),
            self.ncoord(),
            "set_point needs one value per axis"
        );
        let npoint = self.npoint();
        assert!(i < npoint, "point {i} of a PointSet of {npoint}");
        let data = self.data.as_slice_mut().expect("standard layout");
        for (j, &v) in coords.iter().enumerate() {
            data[j * npoint + i] = v;
        }
    }

    /// Writable access to point `i`: a copy of its coordinates, written
    /// back when the returned guard is dropped.
    ///
    /// Panics if `i >= npoint()`.
    #[inline]
    pub fn point_mut(&mut self, i: usize) -> PointMut<'_, Self> {
        let buf = self.point(i);
        PointMut::new(buf, i, self)
    }

    /// Axis `j` (0-based; PointSet is frame-unaware and uses raw array indices).
    #[inline]
    pub fn axis(&self, j: usize) -> &[f64] {
        self.data.row(j).to_slice().expect("standard layout")
    }

    /// Axis `j` (0-based), writable.
    #[inline]
    pub fn axis_mut(&mut self, j: usize) -> &mut [f64] {
        self.data.row_mut(j).into_slice().expect("standard layout")
    }

    /// Iterate over values in a single coordinate axis.
    ///
    /// `j` is 0-based. Panics if `j >= ncoord()`.
    pub fn axis_iter(&self, j: usize) -> impl Iterator<Item = f64> + '_ {
        self.axis(j).iter().copied()
    }

    /// A view of every axis.
    pub fn as_points(&self) -> Points<'_> {
        let npoint = self.npoint();
        Points::from_parts(
            self.data
                .rows()
                .into_iter()
                .map(|r| r.to_slice().expect("standard layout"))
                .collect(),
            npoint,
        )
    }

    /// A writable view of every axis.
    pub fn as_points_mut(&mut self) -> PointsMut<'_> {
        let npoint = self.npoint();
        PointsMut::from_parts(
            self.data
                .rows_mut()
                .into_iter()
                .map(|r| r.into_slice().expect("standard layout"))
                .collect(),
            npoint,
        )
    }
}

impl PointShape for PointSet {
    fn npoint(&self) -> usize {
        self.data.ncols()
    }
    fn ncoord(&self) -> usize {
        self.data.nrows()
    }
}

impl WritePoint for PointSet {
    #[inline]
    fn write_point(&mut self, i: usize, coords: &[f64]) {
        self.set_point(i, coords);
    }
}

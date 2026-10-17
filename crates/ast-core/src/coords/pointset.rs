//! Frame-unaware coordinate block.

use ndarray::{Array2, ArrayView2, ArrayViewMut2};

use super::points::{
    AxisList, INLINE_AXES, PointBuf, PointMut, PointShape, Points, PointsMut, WritePoint,
};
use crate::error::{AstError, AstResult, ErrorCode};

/// Frame-unaware coordinate block.
///
/// One contiguous array per axis, as C's PointSet stores them: axis `j` is
/// `data[j * npoint..(j + 1) * npoint]`, which [`PointSet::view`] presents as
/// an `(ncoord, npoint)` array.
#[derive(Debug, Clone)]
pub struct PointSet {
    data: Vec<f64>,
    ncoord: usize,
    npoint: usize,
}

impl PointSet {
    /// Create a PointSet filled with zeros.
    #[inline]
    pub fn zeros(npoint: usize, ncoord: usize) -> Self {
        Self {
            data: vec![0.0; ncoord * npoint],
            ncoord,
            npoint,
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
        let mut out = Self::zeros(npoint, ncoord);
        for (i, row) in flat.chunks_exact(ncoord.max(1)).enumerate().take(npoint) {
            out.set_point(i, &row[..ncoord]);
        }
        out
    }

    /// Create from column slices (one slice per coordinate axis).
    ///
    /// All columns must have the same length.
    #[inline]
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
        let mut data = Vec::with_capacity(cols.len() * npoint);
        for col in cols {
            data.extend_from_slice(col);
        }
        Ok(Self {
            data,
            ncoord: cols.len(),
            npoint,
        })
    }

    /// A copy of the points of a view.
    #[inline]
    pub fn from_points(p: &Points<'_>) -> Self {
        let mut out = Self::zeros(p.npoint(), p.ncoord());
        out.as_points_mut().copy_from(p);
        out
    }

    /// Create from an `(ncoord, npoint)` array.
    pub fn from_ndarray(a: Array2<f64>) -> Self {
        let (ncoord, npoint) = a.dim();
        Self {
            data: a.iter().copied().collect(),
            ncoord,
            npoint,
        }
    }

    /// Number of points.
    #[inline]
    pub fn npoint(&self) -> usize {
        self.npoint
    }

    /// Number of coordinates per point.
    #[inline]
    pub fn ncoord(&self) -> usize {
        self.ncoord
    }

    /// The `(ncoord, npoint)` array.
    #[inline]
    pub fn view(&self) -> ArrayView2<'_, f64> {
        ArrayView2::from_shape((self.ncoord, self.npoint), &self.data)
            .expect("data holds ncoord * npoint values")
    }

    /// The `(ncoord, npoint)` array, writable.
    #[inline]
    pub fn view_mut(&mut self) -> ArrayViewMut2<'_, f64> {
        ArrayViewMut2::from_shape((self.ncoord, self.npoint), &mut self.data)
            .expect("data holds ncoord * npoint values")
    }

    /// Consume and return the `(ncoord, npoint)` array.
    pub fn into_ndarray(self) -> Array2<f64> {
        Array2::from_shape_vec((self.ncoord, self.npoint), self.data)
            .expect("data holds ncoord * npoint values")
    }

    /// A copy of point `i`.
    ///
    /// Panics if `i >= npoint()`.
    #[inline]
    pub fn point(&self, i: usize) -> PointBuf {
        let npoint = self.npoint;
        assert!(i < npoint, "point {i} of a PointSet of {npoint}");
        PointBuf::gather(self.ncoord, |j| self.data[j * npoint + i])
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
        let npoint = self.npoint;
        assert!(i < npoint, "point {i} of a PointSet of {npoint}");
        for (j, &v) in coords.iter().enumerate() {
            self.data[j * npoint + i] = v;
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
        assert!(j < self.ncoord, "axis {j} of a PointSet of {}", self.ncoord);
        &self.data[j * self.npoint..(j + 1) * self.npoint]
    }

    /// Axis `j` (0-based), writable.
    #[inline]
    pub fn axis_mut(&mut self, j: usize) -> &mut [f64] {
        assert!(j < self.ncoord, "axis {j} of a PointSet of {}", self.ncoord);
        &mut self.data[j * self.npoint..(j + 1) * self.npoint]
    }

    /// Iterate over values in a single coordinate axis.
    ///
    /// `j` is 0-based. Panics if `j >= ncoord()`.
    #[inline]
    pub fn axis_iter(&self, j: usize) -> impl Iterator<Item = f64> + '_ {
        self.axis(j).iter().copied()
    }

    /// A view of every axis.
    #[inline]
    pub fn as_points(&self) -> Points<'_> {
        let n = self.npoint;
        let axes = if self.ncoord <= INLINE_AXES {
            let mut buf: [&[f64]; INLINE_AXES] = [&[]; INLINE_AXES];
            for (j, slot) in buf.iter_mut().enumerate().take(self.ncoord) {
                *slot = &self.data[j * n..(j + 1) * n];
            }
            AxisList::Inline(buf, self.ncoord)
        } else {
            AxisList::Heap(
                (0..self.ncoord)
                    .map(|j| &self.data[j * n..(j + 1) * n])
                    .collect(),
            )
        };
        Points::from_parts(axes, n)
    }

    /// A writable view of every axis.
    #[inline]
    pub fn as_points_mut(&mut self) -> PointsMut<'_> {
        let n = self.npoint;
        let ncoord = self.ncoord;
        let mut chunks = self.data.chunks_exact_mut(n.max(1));
        let axes = if ncoord <= INLINE_AXES {
            let mut buf: [&mut [f64]; INLINE_AXES] = Default::default();
            for slot in buf.iter_mut().take(ncoord) {
                *slot = if n == 0 {
                    &mut []
                } else {
                    chunks.next().expect("one chunk per axis")
                };
            }
            AxisList::Inline(buf, ncoord)
        } else {
            AxisList::Heap(
                (0..ncoord)
                    .map(|_| {
                        if n == 0 {
                            &mut [][..]
                        } else {
                            chunks.next().expect("one chunk per axis")
                        }
                    })
                    .collect(),
            )
        };
        PointsMut::from_parts(axes, n)
    }
}

impl PointShape for PointSet {
    fn npoint(&self) -> usize {
        self.npoint
    }
    fn ncoord(&self) -> usize {
        self.ncoord
    }
}

impl WritePoint for PointSet {
    #[inline]
    fn write_point(&mut self, i: usize, coords: &[f64]) {
        self.set_point(i, coords);
    }
}

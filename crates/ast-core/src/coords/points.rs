//! Borrowed per-axis views of point coordinates.
//!
//! A view holds one slice per axis, all of the same length, the layout C's
//! `astTranN` passes and C's PointSet stores. Transforms read a [`Points`]
//! and write a [`PointsMut`], so a caller's own arrays can be transformed
//! without copying.

use std::ops::{Deref, DerefMut, Range};

use smallvec::SmallVec;

use crate::error::{AstError, AstResult, ErrorCode};

/// Axis counts up to this are held without allocating; the transform oracle
/// corpora use at most 5 axes.
pub const INLINE_AXES: usize = 8;

/// One point's coordinates, copied out of a PointSet or a view.
#[derive(Clone, Debug, Default)]
pub struct PointBuf(SmallVec<[f64; INLINE_AXES]>);

impl PointBuf {
    /// A point of `ncoord` zeros.
    pub fn zeros(ncoord: usize) -> Self {
        Self(SmallVec::from_elem(0.0, ncoord))
    }

    /// The point whose coordinate `j` is `get(j)`, for `j` in `0..ncoord`.
    #[inline]
    pub(crate) fn gather(ncoord: usize, mut get: impl FnMut(usize) -> f64) -> Self {
        if ncoord <= INLINE_AXES {
            let mut buf = [0.0; INLINE_AXES];
            for (j, v) in buf.iter_mut().enumerate().take(ncoord) {
                *v = get(j);
            }
            Self(SmallVec::from_buf_and_len(buf, ncoord))
        } else {
            Self((0..ncoord).map(get).collect())
        }
    }
}

impl Deref for PointBuf {
    type Target = [f64];
    fn deref(&self) -> &[f64] {
        &self.0
    }
}

impl DerefMut for PointBuf {
    fn deref_mut(&mut self) -> &mut [f64] {
        &mut self.0
    }
}

impl AsRef<[f64]> for PointBuf {
    fn as_ref(&self) -> &[f64] {
        &self.0
    }
}

impl IntoIterator for PointBuf {
    type Item = f64;
    type IntoIter = smallvec::IntoIter<[f64; INLINE_AXES]>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a PointBuf {
    type Item = &'a f64;
    type IntoIter = std::slice::Iter<'a, f64>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl FromIterator<f64> for PointBuf {
    fn from_iter<I: IntoIterator<Item = f64>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl PartialEq for PointBuf {
    fn eq(&self, other: &Self) -> bool {
        self[..] == other[..]
    }
}

impl PartialEq<[f64]> for PointBuf {
    fn eq(&self, other: &[f64]) -> bool {
        self[..] == *other
    }
}

impl PartialEq<&[f64]> for PointBuf {
    fn eq(&self, other: &&[f64]) -> bool {
        self[..] == **other
    }
}

impl<const N: usize> PartialEq<[f64; N]> for PointBuf {
    fn eq(&self, other: &[f64; N]) -> bool {
        self[..] == other[..]
    }
}

impl<const N: usize> PartialEq<&[f64; N]> for PointBuf {
    fn eq(&self, other: &&[f64; N]) -> bool {
        self[..] == other[..]
    }
}

impl PartialEq<Vec<f64>> for PointBuf {
    fn eq(&self, other: &Vec<f64>) -> bool {
        self[..] == other[..]
    }
}

/// A set of points one of which can be overwritten: a
/// [`PointSet`](super::PointSet) or a [`PointsMut`] view.
pub trait WritePoint {
    /// Set point `i`; `coords` has one value per axis.
    fn write_point(&mut self, i: usize, coords: &[f64]);
}

/// Writable access to one point of `W`: a copy of its coordinates, written
/// back to the point when this guard is dropped, on every exit path.
pub struct PointMut<'a, W: WritePoint + ?Sized> {
    buf: PointBuf,
    i: usize,
    dst: &'a mut W,
}

impl<'a, W: WritePoint + ?Sized> PointMut<'a, W> {
    #[inline]
    pub(crate) fn new(buf: PointBuf, i: usize, dst: &'a mut W) -> Self {
        Self { buf, i, dst }
    }
}

impl<W: WritePoint + ?Sized> Deref for PointMut<'_, W> {
    type Target = [f64];
    #[inline]
    fn deref(&self) -> &[f64] {
        &self.buf
    }
}

impl<W: WritePoint + ?Sized> DerefMut for PointMut<'_, W> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [f64] {
        &mut self.buf
    }
}

impl<W: WritePoint + ?Sized> Drop for PointMut<'_, W> {
    #[inline]
    fn drop(&mut self) {
        self.dst.write_point(self.i, &self.buf);
    }
}

/// The shape of a set of points, for checking it against a Mapping.
pub(crate) trait PointShape {
    fn npoint(&self) -> usize;
    fn ncoord(&self) -> usize;
}

/// The common length of `lens`, or C's error for none or for a mismatch.
fn common_length(lens: impl IntoIterator<Item = usize>) -> AstResult<usize> {
    let mut lens = lens.into_iter();
    let Some(npoint) = lens.next() else {
        return Err(AstError::new(
            ErrorCode::Ncoin,
            "a set of points needs at least one axis",
        ));
    };
    for (j, len) in lens.enumerate() {
        if len != npoint {
            return Err(AstError::new(
                ErrorCode::Ncpin,
                format!("axis {} has {len} points but axis 1 has {npoint}", j + 2),
            ));
        }
    }
    Ok(npoint)
}

/// The checks `astTranN` makes on its array dimension (mapping.c `TranN`),
/// plus that `len` holds every axis.
fn check_stride(len: usize, ncoord: usize, npoint: usize, stride: usize) -> AstResult<usize> {
    if ncoord == 0 {
        return Err(AstError::new(
            ErrorCode::Ncoin,
            "a set of points needs at least one axis",
        ));
    }
    if stride < npoint {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!("the array dimension ({stride}) is less than the number of points ({npoint})"),
        ));
    }
    let need = (ncoord - 1) * stride + npoint;
    if len < need {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!(
                "{ncoord} axes at a dimension of {stride} need {need} values; the array has {len}"
            ),
        ));
    }
    Ok(need)
}

/// A read-only view of `npoint` points, one slice per axis.
#[derive(Clone, Debug)]
pub struct Points<'a> {
    axes: SmallVec<[&'a [f64]; INLINE_AXES]>,
    npoint: usize,
}

impl<'a> Points<'a> {
    /// A view of one slice per axis; every slice must have the same length.
    pub fn new(axes: impl IntoIterator<Item = &'a [f64]>) -> AstResult<Self> {
        let axes: SmallVec<[&'a [f64]; INLINE_AXES]> = axes.into_iter().collect();
        let npoint = common_length(axes.iter().map(|a| a.len()))?;
        Ok(Self { axes, npoint })
    }

    /// A view of `ncoord` axes of `npoint` points held one after another in
    /// `data`, axis `j` starting at `j * stride`: C's `astTranN` layout,
    /// whose `indim` is `stride`.
    pub fn from_strided(
        data: &'a [f64],
        ncoord: usize,
        npoint: usize,
        stride: usize,
    ) -> AstResult<Self> {
        check_stride(data.len(), ncoord, npoint, stride)?;
        let axes = (0..ncoord)
            .map(|j| &data[j * stride..j * stride + npoint])
            .collect();
        Ok(Self { axes, npoint })
    }

    /// A view whose axes the caller knows are all `npoint` long.
    pub(crate) fn from_parts(axes: SmallVec<[&'a [f64]; INLINE_AXES]>, npoint: usize) -> Self {
        debug_assert!(axes.iter().all(|a| a.len() == npoint));
        Self { axes, npoint }
    }

    /// Number of points.
    pub fn npoint(&self) -> usize {
        self.npoint
    }

    /// Number of axes.
    pub fn ncoord(&self) -> usize {
        self.axes.len()
    }

    /// Axis `j` (0-based).
    #[inline]
    pub fn axis(&self, j: usize) -> &'a [f64] {
        self.axes[j]
    }

    /// A copy of point `i`.
    #[inline]
    pub fn point(&self, i: usize) -> PointBuf {
        PointBuf::gather(self.axes.len(), |j| self.axes[j][i])
    }

    /// Points `range` of every axis.
    pub fn points(&self, range: Range<usize>) -> Points<'a> {
        let npoint = range.len();
        Points {
            axes: self.axes.iter().map(|a| &a[range.clone()]).collect(),
            npoint,
        }
    }

    /// Axes `range` (0-based).
    pub fn axes(&self, range: Range<usize>) -> Points<'a> {
        Points {
            axes: self.axes[range].iter().copied().collect(),
            npoint: self.npoint,
        }
    }
}

impl PointShape for Points<'_> {
    fn npoint(&self) -> usize {
        self.npoint
    }
    fn ncoord(&self) -> usize {
        self.axes.len()
    }
}

/// A writable view of `npoint` points, one slice per axis.
#[derive(Debug)]
pub struct PointsMut<'a> {
    axes: SmallVec<[&'a mut [f64]; INLINE_AXES]>,
    npoint: usize,
}

impl<'a> PointsMut<'a> {
    /// A view of one slice per axis; every slice must have the same length.
    pub fn new(axes: impl IntoIterator<Item = &'a mut [f64]>) -> AstResult<Self> {
        let axes: SmallVec<[&'a mut [f64]; INLINE_AXES]> = axes.into_iter().collect();
        let npoint = common_length(axes.iter().map(|a| a.len()))?;
        Ok(Self { axes, npoint })
    }

    /// The writable form of [`Points::from_strided`].
    pub fn from_strided(
        data: &'a mut [f64],
        ncoord: usize,
        npoint: usize,
        stride: usize,
    ) -> AstResult<Self> {
        let need = check_stride(data.len(), ncoord, npoint, stride)?;
        let mut axes = SmallVec::new();
        let mut rest = &mut data[..need];
        for j in 0..ncoord {
            let take = if j + 1 < ncoord { stride } else { npoint };
            let (chunk, tail) = std::mem::take(&mut rest).split_at_mut(take);
            axes.push(&mut chunk[..npoint]);
            rest = tail;
        }
        Ok(Self { axes, npoint })
    }

    /// A view whose axes the caller knows are all `npoint` long.
    pub(crate) fn from_parts(axes: SmallVec<[&'a mut [f64]; INLINE_AXES]>, npoint: usize) -> Self {
        debug_assert!(axes.iter().all(|a| a.len() == npoint));
        Self { axes, npoint }
    }

    /// Number of points.
    pub fn npoint(&self) -> usize {
        self.npoint
    }

    /// Number of axes.
    pub fn ncoord(&self) -> usize {
        self.axes.len()
    }

    /// Axis `j` (0-based).
    #[inline]
    pub fn axis(&self, j: usize) -> &[f64] {
        self.axes[j]
    }

    /// Axis `j` (0-based), writable.
    #[inline]
    pub fn axis_mut(&mut self, j: usize) -> &mut [f64] {
        self.axes[j]
    }

    /// A copy of point `i`.
    #[inline]
    pub fn point(&self, i: usize) -> PointBuf {
        PointBuf::gather(self.axes.len(), |j| self.axes[j][i])
    }

    /// Set point `i`; `coords` must have one value per axis.
    #[inline]
    pub fn set_point(&mut self, i: usize, coords: &[f64]) {
        assert_eq!(
            coords.len(),
            self.axes.len(),
            "set_point needs one value per axis"
        );
        for (a, &v) in self.axes.iter_mut().zip(coords) {
            a[i] = v;
        }
    }

    /// Writable access to point `i`, written back when the guard drops.
    #[inline]
    pub fn point_mut(&mut self, i: usize) -> PointMut<'_, Self> {
        let buf = self.point(i);
        PointMut::new(buf, i, self)
    }

    /// Copy `src` into the first `src.npoint()` points of each axis.
    pub fn copy_from(&mut self, src: &Points<'_>) {
        assert_eq!(
            src.ncoord(),
            self.ncoord(),
            "copy_from needs the same number of axes"
        );
        let n = src.npoint();
        for (j, a) in self.axes.iter_mut().enumerate() {
            a[..n].copy_from_slice(src.axis(j));
        }
    }

    /// A read-only view of the same points.
    pub fn as_points(&self) -> Points<'_> {
        Points {
            axes: self.axes.iter().map(|a| &**a).collect(),
            npoint: self.npoint,
        }
    }

    /// A shorter-lived writable view of the same points.
    pub fn reborrow(&mut self) -> PointsMut<'_> {
        PointsMut {
            axes: self.axes.iter_mut().map(|a| &mut **a).collect(),
            npoint: self.npoint,
        }
    }

    /// Points `range` of every axis.
    pub fn points_mut(&mut self, range: Range<usize>) -> PointsMut<'_> {
        let npoint = range.len();
        PointsMut {
            axes: self
                .axes
                .iter_mut()
                .map(|a| &mut a[range.clone()])
                .collect(),
            npoint,
        }
    }

    /// The first `k` axes and the rest, as two views.
    pub fn split_axes_at(self, k: usize) -> (PointsMut<'a>, PointsMut<'a>) {
        let npoint = self.npoint;
        let mut axes = self.axes.into_iter();
        let left: SmallVec<_> = axes.by_ref().take(k).collect();
        let right: SmallVec<_> = axes.collect();
        (
            PointsMut { axes: left, npoint },
            PointsMut {
                axes: right,
                npoint,
            },
        )
    }
}

impl PointShape for PointsMut<'_> {
    fn npoint(&self) -> usize {
        self.npoint
    }
    fn ncoord(&self) -> usize {
        self.axes.len()
    }
}

impl WritePoint for PointsMut<'_> {
    #[inline]
    fn write_point(&mut self, i: usize, coords: &[f64]) {
        self.set_point(i, coords);
    }
}

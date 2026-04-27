//! A compound frame combining two child frames.

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, UnitMap};

use super::{DynFrame, Frame, FrameRef, FrameSet};

/// A compound frame that concatenates the axes of two child frames.
///
/// If the first child has N1 axes and the second has N2 axes, the
/// `CmpFrame` has N1 + N2 axes. Axes 1..=N1 are served by the first
/// child and axes N1+1..=N1+N2 by the second.
///
/// Geometry methods (`distance`, `angle`, `offset`) use Euclidean
/// (Cartesian) formulae over the full compound axis space.
/// Per-axis operations (`format`, `unformat`, `norm`) are delegated
/// to the owning child frame.
pub struct CmpFrame {
    frame1: DynFrame,
    frame2: DynFrame,
}

impl CmpFrame {
    /// Create a compound frame from two child frames.
    ///
    /// The resulting frame has `frame1.naxes() + frame2.naxes()` axes.
    pub fn new(frame1: DynFrame, frame2: DynFrame) -> Self {
        Self { frame1, frame2 }
    }

    /// Number of axes contributed by the first child frame.
    pub fn naxes1(&self) -> usize {
        self.frame1.naxes()
    }

    /// Validate that `axis` is in 1..=naxes, returning an error if not.
    fn validate_axis(&self, axis: usize) -> AstResult<()> {
        let n = self.naxes();
        if axis < 1 || axis > n {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..={}", axis, n),
            ));
        }
        Ok(())
    }

    /// Route a 1-based compound axis to the owning child frame and its
    /// local 1-based axis index.
    ///
    /// Axes 1..=N1 map to `(frame1, axis)`.
    /// Axes N1+1..=N1+N2 map to `(frame2, axis - N1)`.
    ///
    /// # Panics
    ///
    /// Panics if `axis` is out of range. Call [`validate_axis`] first.
    fn route_axis(&self, axis: usize) -> (&dyn Frame, usize) {
        let n1 = self.frame1.naxes();
        if axis <= n1 {
            (self.frame1.as_ref(), axis)
        } else {
            (self.frame2.as_ref(), axis - n1)
        }
    }
}

impl fmt::Debug for CmpFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CmpFrame")
            .field("naxes1", &self.frame1.naxes())
            .field("naxes2", &self.frame2.naxes())
            .finish()
    }
}

impl Frame for CmpFrame {
    fn naxes(&self) -> usize {
        self.frame1.naxes() + self.frame2.naxes()
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.validate_axis(axis)?;
        let (child, local_axis) = self.route_axis(axis);
        child.format(local_axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.validate_axis(axis)?;
        let (child, local_axis) = self.route_axis(axis);
        child.unformat(local_axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) {
            return Ok(BAD_VALUE);
        }
        let sum: f64 = a
            .iter()
            .zip(b.iter())
            .map(|(&ai, &bi)| (ai - bi) * (ai - bi))
            .sum();
        Ok(sum.sqrt())
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) || point_has_bad(c) {
            return Ok(BAD_VALUE);
        }
        let n = self.naxes();
        let mut dot = 0.0;
        let mut mag_ba = 0.0;
        let mut mag_bc = 0.0;
        for i in 0..n {
            let ba = a[i] - b[i];
            let bc = c[i] - b[i];
            dot += ba * bc;
            mag_ba += ba * ba;
            mag_bc += bc * bc;
        }
        let denom = (mag_ba * mag_bc).sqrt();
        if denom == 0.0 {
            return Ok(0.0);
        }
        let cos_angle = (dot / denom).clamp(-1.0, 1.0);
        Ok(cos_angle.acos())
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        let n = self.naxes();
        if point_has_bad(from) || point_has_bad(to) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; n]);
        }
        let total = self.distance(from, to)?;
        if total == 0.0 || is_bad(total) {
            return Ok(from.to_vec());
        }
        let scale = dist / total;
        let result: Vec<f64> = from
            .iter()
            .zip(to.iter())
            .map(|(&f, &t)| f + scale * (t - f))
            .collect();
        Ok(result)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        let n1 = self.frame1.naxes();
        let (first, second) = coords.split_at_mut(n1);
        self.frame1.norm(first)?;
        self.frame2.norm(second)?;
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(CmpFrame::new(
            self.frame1.snapshot(),
            self.frame2.snapshot(),
        ))
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(self.naxes())))
    }

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        if self.naxes() == to.naxes() {
            let mut fs = FrameSet::new(self.snapshot());
            fs.add_frame(
                FrameRef::Base,
                Box::new(UnitMap::new(self.naxes())),
                to.snapshot(),
            )?;
            Ok(Some(fs))
        } else {
            Ok(None)
        }
    }
}

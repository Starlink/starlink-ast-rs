//! Constant offset mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;

use super::cmpmap::{CmpMap, Composition};
use super::helpers::point_has_bad;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A mapping that adds a constant shift vector to all input coordinates.
#[derive(Debug, Clone)]
pub struct ShiftMap {
    shift: Vec<f64>,
    inverted: bool,
}

impl ShiftMap {
    /// Create a new `ShiftMap` with the given shift offsets.
    ///
    /// The number of coordinates is determined by the length of `shift`.
    pub fn new(shift: impl Into<Vec<f64>>) -> Self {
        Self {
            shift: shift.into(),
            inverted: false,
        }
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
}

impl Mapping for ShiftMap {
    fn nin(&self) -> usize {
        self.shift.len()
    }
    fn nout(&self) -> usize {
        self.shift.len()
    }
    fn is_inverted(&self) -> bool {
        self.inverted
    }
    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), self.shift.len());
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let sign = if fwd { 1.0 } else { -1.0 };
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else {
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    *o = v + sign * self.shift[j];
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.shift.iter().all(|&s| s == 0.0) {
            Ok(Box::new(UnitMap::new(self.shift.len())))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}

//! Uniform scaling mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::point_has_bad;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A mapping that scales all input coordinates by a constant factor.
#[derive(Debug, Clone)]
pub struct ZoomMap {
    ncoord: usize,
    factor: f64,
    inverted: bool,
}

impl ZoomMap {
    /// Create a new `ZoomMap` that scales `ncoord` coordinates by `factor`.
    ///
    /// Returns an error if `factor` is zero.
    pub fn new(ncoord: usize, factor: f64) -> AstResult<Self> {
        if factor == 0.0 {
            return Err(AstError::new(
                ErrorCode::Zoomi,
                "zoom factor must not be zero",
            ));
        }
        Ok(Self {
            ncoord,
            factor,
            inverted: false,
        })
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

impl Mapping for ZoomMap {
    fn nin(&self) -> usize {
        self.ncoord
    }
    fn nout(&self) -> usize {
        self.ncoord
    }
    fn is_inverted(&self) -> bool {
        self.inverted
    }
    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), self.ncoord);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let scale = if fwd { self.factor } else { 1.0 / self.factor };
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else {
                for (o, &v) in outp.iter_mut().zip(inp.iter()) {
                    *o = v * scale;
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.factor == 1.0 {
            Ok(Box::new(UnitMap::new(self.ncoord)))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}

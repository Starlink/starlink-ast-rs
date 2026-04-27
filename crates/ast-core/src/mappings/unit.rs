//! The identity mapping.

use crate::coords::PointSet;
use crate::error::AstResult;

use super::cmpmap::{CmpMap, Composition};
use super::rate::numeric_rate;
use super::{DynMapping, Mapping};

/// The identity mapping: every output coordinate equals the corresponding input.
#[derive(Debug, Clone)]
pub struct UnitMap {
    ncoord: usize,
    inverted: bool,
}

impl UnitMap {
    /// Create a new `UnitMap` with `ncoord` coordinates.
    pub fn new(ncoord: usize) -> Self {
        Self {
            ncoord,
            inverted: false,
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

impl Mapping for UnitMap {
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

    fn transform(&self, pts: &PointSet, _forward: bool) -> AstResult<PointSet> {
        Ok(pts.clone())
    }

    fn transform_into(&self, pts: &PointSet, _forward: bool, out: &mut PointSet) -> AstResult<()> {
        out.view_mut().assign(&pts.view());
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}

//! 2-D pincushion/barrel distortion mapping.
//!
//! A `PcdMap` transforms 2-dimensional positions to correct for the
//! radial distortion introduced by some cameras and telescopes.  The
//! forward transformation applies the distortion:
//!
//!   RD = R * (1 + C * R^2)
//!
//! where R is the undistorted radial distance from the distortion
//! centre, RD is the distorted radial distance, and C is the
//! distortion coefficient.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::perm::PermMap;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// A 2-D pincushion/barrel distortion mapping.
///
/// The forward transformation applies radial distortion characterised
/// by a single coefficient `disc`:
///
/// ```text
/// dx = x - cen[0]
/// dy = y - cen[1]
/// f = 1 + disc * (dx^2 + dy^2)
/// x_out = dx * f + cen[0]
/// y_out = dy * f + cen[1]
/// ```
///
/// Positive `disc` gives pincushion distortion; negative gives barrel
/// distortion; zero gives no distortion (identity).
///
/// The inverse is an approximate iterative solution (from SLA_UNPCD)
/// that is accurate to negligible levels for astrometric purposes.
#[derive(Debug, Clone)]
pub struct PcdMap {
    disc: f64,
    pcdcen: [f64; 2],
    inverted: bool,
    do_not_simplify: bool,
}

impl PcdMap {
    /// Create a new `PcdMap` with the given distortion coefficient and centre.
    ///
    /// `disc` is the distortion coefficient (positive = pincushion,
    /// negative = barrel, zero = no distortion).
    /// `pcdcen` is the `[x, y]` centre of the distortion.
    pub fn new(disc: f64, pcdcen: [f64; 2]) -> Self {
        Self {
            disc,
            pcdcen,
            inverted: false,
            do_not_simplify: false,
        }
    }

    /// The distortion coefficient.
    pub fn disc(&self) -> f64 {
        self.disc
    }

    /// The distortion centre.
    pub fn pcdcen(&self) -> [f64; 2] {
        self.pcdcen
    }

    /// Get the effective distortion coefficient, accounting for inversion state.
    ///
    /// When the PcdMap is inverted, the disc value is still the same
    /// (the forward/inverse roles of the transform are swapped, not
    /// the coefficient itself).
    fn effective_disc(&self) -> f64 {
        self.disc
    }

    /// Get the effective centre, accounting for inversion state.
    fn effective_pcdcen(&self) -> [f64; 2] {
        self.pcdcen
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

    /// Check if a PermMap simply swaps axes 0 and 1 (both directions).
    pub(crate) fn perm_is_swap(pm: &PermMap) -> bool {
        let outperm = pm.outperm();
        let inperm = pm.inperm();
        if outperm.len() != 2 || inperm.len() != 2 {
            return false;
        }
        outperm[0] == 2 && outperm[1] == 1 && inperm[0] == 2 && inperm[1] == 1
    }
}

impl Mapping for PcdMap {
    fn nin(&self) -> usize {
        2
    }

    fn nout(&self) -> usize {
        2
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), 2);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let disco = self.effective_disc();
        let cen0 = self.effective_pcdcen()[0];
        let cen1 = self.effective_pcdcen()[1];

        if fwd {
            // Forward: apply distortion
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if is_bad(inp[0]) || is_bad(inp[1]) {
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                } else {
                    let dx = inp[0] - cen0;
                    let dy = inp[1] - cen1;
                    let f = 1.0 + disco * (dx * dx + dy * dy);
                    outp[0] = dx * f + cen0;
                    outp[1] = dy * f + cen1;
                }
            }
        } else {
            // Inverse: remove distortion (SLA_UNPCD approximation)
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if is_bad(inp[0]) || is_bad(inp[1]) {
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                } else {
                    let dxp = inp[0] - cen0;
                    let dyp = inp[1] - cen1;
                    let cr2 = disco * (dxp * dxp + dyp * dyp);
                    let a = (2.0 * cr2 + 1.0) / (3.0 * cr2 + 1.0);
                    let cr2a2 = cr2 * a * a;
                    let f = (2.0 * cr2a2 * a + 1.0) / (3.0 * cr2a2 + 1.0);
                    outp[0] = dxp * f + cen0;
                    outp[1] = dyp * f + cen1;
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }

        // If disc == 0, the PcdMap is identity.
        if ast_equal(self.disc, 0.0) {
            return Ok(Box::new(UnitMap::new(2)));
        }

        // Otherwise just return a clone (no further self-simplification).
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<PcdMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        ast_equal(self.disc, other.disc)
            && ast_equal(self.pcdcen[0], other.pcdcen[0])
            && ast_equal(self.pcdcen[1], other.pcdcen[1])
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

        // Case 1: PcdMap + PcdMap (inverse cancellation)
        if let Some(other_pcd) = other.as_any().downcast_ref::<PcdMap>() {
            // They cancel if they have the same disc and centre but opposite
            // effective inversion states.
            if ast_equal(self.disc, other_pcd.disc)
                && ast_equal(self.pcdcen[0], other_pcd.pcdcen[0])
                && ast_equal(self.pcdcen[1], other_pcd.pcdcen[1])
                && self_inv != other_inv
            {
                return Some(Box::new(UnitMap::new(2)));
            }
            return None;
        }

        // Case 2: PcdMap + UnitMap → PcdMap unchanged
        if other.as_any().downcast_ref::<UnitMap>().is_some() {
            let mut result = self.clone();
            // Normalize: apply the effective inversion state
            result.inverted = self_inv;
            return Some(Box::new(result));
        }

        None
    }

    fn do_not_simplify(&self) -> bool {
        self.do_not_simplify
    }

    fn set_do_not_simplify(&mut self, flag: bool) {
        self.do_not_simplify = flag;
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl Serializable for PcdMap {
    fn class_name(&self) -> &'static str {
        "PcdMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Apply pincushion distortion".to_string());
        write_mapping_base(&mut node, self);
        // PcdCn0 — always write (default 0.0)
        node.push_double(
            "PcdCn0",
            self.pcdcen[0],
            Some("Distortion centre on first axis".to_string()),
        );
        // PcdCn1 — always write (default 0.0)
        node.push_double(
            "PcdCn1",
            self.pcdcen[1],
            Some("Distortion centre on second axis".to_string()),
        );
        // Disco — always write (default 0.0)
        node.push_double(
            "Disco",
            self.disc,
            Some("Distortion coefficient".to_string()),
        );
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);
        let disc = node.get_double("Disco", 0.0);
        let pcdcen0 = node.get_double("PcdCn0", 0.0);
        let pcdcen1 = node.get_double("PcdCn1", 0.0);
        let mut m = PcdMap::new(disc, [pcdcen0, pcdcen1]);
        m.do_not_simplify = base.do_not_simplify;
        if base.inverted {
            m.invert();
        }
        Ok(m)
    }
}

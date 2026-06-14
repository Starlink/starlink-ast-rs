//! SelectorMap — region-membership classifier mapping.
//!
//! A `SelectorMap` encapsulates a list of `Region`s sharing the same
//! number of axes. Its forward transform takes a point in that axis space
//! and returns a single coordinate giving the 1-based index of the first
//! containing region (0 when no region matches; `badval` when the input is
//! itself bad). The inverse transform is undefined.

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};

use super::helpers::{ast_equal, is_bad};
use super::{DynMapping, Mapping, UnitMap};
use crate::regions::{
    BoxRegion, Circle, CmpRegion, DynRegion, Ellipse, Interval, NullRegion, PointList, Polygon,
    Prism,
};

/// A `Mapping` that classifies points by region membership.
///
/// Forward transform produces a 1-based region index per point (0 if no
/// region contains the point; [`SelectorMap::badval`] for bad input).
/// `nin` equals the regions' shared axis count; `nout` is always 1.
/// No inverse is defined.
pub struct SelectorMap {
    regions: Vec<DynRegion>,
    badval: f64,
    inverted: bool,
    has_been_simplified: bool,
}

/// Builder for [`SelectorMap`].
pub struct SelectorMapBuilder {
    regions: Vec<DynRegion>,
    badval: f64,
}

impl SelectorMapBuilder {
    /// Create a builder around a non-empty `Vec` of regions.
    ///
    /// All regions must share the same `naxes()`.
    pub fn new(regions: Vec<DynRegion>) -> AstResult<Self> {
        if regions.is_empty() {
            return Err(AstError::new(
                ErrorCode::Bdpar,
                "SelectorMap: at least one Region is required",
            ));
        }
        let naxes = regions[0].naxes();
        for (i, reg) in regions.iter().enumerate().skip(1) {
            if reg.naxes() != naxes {
                return Err(AstError::new(
                    ErrorCode::Bdpar,
                    format!(
                        "SelectorMap: Region {} has {} axes but Region 1 has {}",
                        i + 1,
                        reg.naxes(),
                        naxes
                    ),
                ));
            }
        }
        Ok(Self {
            regions,
            badval: BAD_VALUE,
        })
    }

    /// Set the value returned for bad input positions.
    pub fn badval(mut self, val: f64) -> Self {
        self.badval = val;
        self
    }

    /// Build the [`SelectorMap`].
    pub fn build(self) -> SelectorMap {
        SelectorMap {
            regions: self.regions,
            badval: self.badval,
            inverted: false,
            has_been_simplified: false,
        }
    }
}

impl SelectorMap {
    /// Encapsulated regions.
    pub fn regions(&self) -> &[DynRegion] {
        &self.regions
    }

    /// The value returned for inputs containing a bad coordinate.
    pub fn badval(&self) -> f64 {
        self.badval
    }

    /// The intrinsic number of axes (the regions' shared `naxes()`).
    fn intrinsic_nin(&self) -> usize {
        self.regions[0].naxes()
    }
}

impl fmt::Debug for SelectorMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectorMap")
            .field("nin", &self.intrinsic_nin())
            .field("nout", &1usize)
            .field("nreg", &self.regions.len())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for SelectorMap {
    fn clone(&self) -> Self {
        SelectorMap {
            regions: self.regions.clone(),
            badval: self.badval,
            inverted: self.inverted,
            has_been_simplified: self.has_been_simplified,
        }
    }
}

impl Mapping for SelectorMap {
    fn nin(&self) -> usize {
        if self.inverted {
            1
        } else {
            self.intrinsic_nin()
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            self.intrinsic_nin()
        } else {
            1
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
        // Inversion invalidates the "already simplified" flag, since the
        // inverse may simplify further (C astSetInvert clears IsSimple).
        self.has_been_simplified = false;
    }

    fn has_forward(&self) -> bool {
        // Intrinsic forward is defined; intrinsic inverse is not.
        // When inverted, the effective forward is the intrinsic inverse.
        !self.inverted
    }

    fn has_inverse(&self) -> bool {
        // Intrinsic inverse is undefined; only the inverted form has an
        // effective inverse direction (the intrinsic forward).
        self.inverted
    }

    fn dump_tran_flags(&self) -> (bool, bool) {
        // C passes (1, 0) to astInitMapping (selectormap.c).
        (true, false)
    }

    fn has_been_simplified(&self) -> bool {
        self.has_been_simplified
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.has_been_simplified = flag;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let effective_fwd = forward != self.inverted;
        let npoint = pts.npoint();
        let ncoord = self.intrinsic_nin();

        if !effective_fwd {
            // Inverse not defined — return all BAD.
            let mut out = PointSet::zeros(npoint, ncoord);
            for i in 0..npoint {
                let row = out.point_mut(i);
                for v in row.iter_mut() {
                    *v = BAD_VALUE;
                }
            }
            return Ok(out);
        }

        let mut out = PointSet::zeros(npoint, 1);

        // Initial classification: -1 sentinel = bad input, 0 = unassigned.
        for i in 0..npoint {
            let pt = pts.point(i);
            let has_bad = pt.iter().take(ncoord).any(|&v| is_bad(v));
            out.point_mut(i)[0] = if has_bad { -1.0 } else { 0.0 };
        }

        // Test points against each region in turn. Negate + open the region
        // so its forward transform marks interior points as BAD; that's the
        // standard AST trick for region membership in a Mapping context.
        let mut working = pts.clone();

        for ireg in 0..self.regions.len() {
            let reg = &self.regions[ireg];
            let mut neg_reg = reg.negated_copy();
            let was_closed = neg_reg.is_closed();
            neg_reg.set_closed(!was_closed);

            let transformed = (neg_reg.as_ref() as &dyn Mapping).transform(&working, true)?;

            for i in 0..npoint {
                if out.point(i)[0] == 0.0 && is_bad(transformed.point(i)[0]) {
                    out.point_mut(i)[0] = (ireg + 1) as f64;
                }
            }

            // Subsequent iterations test against the already-pruned point set
            // (interior points are now BAD and won't match further regions).
            working = transformed;
        }

        // Replace -1 sentinel with badval.
        for i in 0..npoint {
            if out.point(i)[0] == -1.0 {
                out.point_mut(i)[0] = self.badval;
            }
        }

        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.has_been_simplified {
            return Ok(Box::new(self.clone()));
        }

        // Try to simplify each child region. A simplified Mapping that is
        // still a known concrete Region type is retained; otherwise we fall
        // back to the original region for that slot.
        let mut changed = false;
        let mut simplified_regions: Vec<DynRegion> = Vec::with_capacity(self.regions.len());
        for reg in &self.regions {
            let simp: DynMapping = (reg.as_ref() as &dyn Mapping).simplified()?;
            let was_changed = !simp.equal(reg.as_ref() as &dyn Mapping)
                || simp.is_inverted() != reg.is_inverted();
            let new_reg: DynRegion = if let Some(r) = simp.as_any().downcast_ref::<Circle>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<BoxRegion>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<Ellipse>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<Polygon>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<Interval>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<NullRegion>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<CmpRegion>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<PointList>() {
                Box::new(r.clone())
            } else if let Some(r) = simp.as_any().downcast_ref::<Prism>() {
                Box::new(r.clone())
            } else {
                // Simplification produced a non-Region — keep original.
                simplified_regions.push(reg.clone());
                continue;
            };
            if was_changed {
                changed = true;
            }
            simplified_regions.push(new_reg);
        }

        if changed {
            Ok(Box::new(SelectorMap {
                regions: simplified_regions,
                badval: self.badval,
                inverted: self.inverted,
                has_been_simplified: false,
            }))
        } else {
            Ok(Box::new(self.clone()))
        }
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

        // Cancel adjacent SelectorMap + inverted SelectorMap → UnitMap.
        let other_sel = other.as_any().downcast_ref::<SelectorMap>()?;

        if self_inv == other_inv {
            return None;
        }
        if self.regions.len() != other_sel.regions.len() {
            return None;
        }
        if !ast_equal(self.badval, other_sel.badval) {
            return None;
        }
        for (a, b) in self.regions.iter().zip(other_sel.regions.iter()) {
            if !(a.as_ref() as &dyn Mapping).equal(b.as_ref() as &dyn Mapping) {
                return None;
            }
        }

        Some(Box::new(UnitMap::new(self.intrinsic_nin())))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other_sel) = other.as_any().downcast_ref::<SelectorMap>() else {
            return false;
        };
        if self.inverted != other_sel.inverted {
            return false;
        }
        if self.regions.len() != other_sel.regions.len() {
            return false;
        }
        if !ast_equal(self.badval, other_sel.badval) {
            return false;
        }
        for (a, b) in self.regions.iter().zip(other_sel.regions.iter()) {
            if !(a.as_ref() as &dyn Mapping).equal(b.as_ref() as &dyn Mapping) {
                return false;
            }
        }
        true
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl crate::serial::Serializable for SelectorMap {
    fn class_name(&self) -> &'static str {
        "SelectorMap"
    }

    fn to_node(&self) -> AstResult<crate::serial::Node> {
        use crate::serial::{Node, RegionToNode, write_mapping_base};

        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Region identification Mapping".to_string());
        write_mapping_base(&mut node, self);

        for (i, reg) in self.regions.iter().enumerate() {
            let key = format!("Reg{}", i + 1);
            let reg_node = RegionToNode::to_node(reg.as_ref())?;
            node.push_object(&key, reg_node, Some("Region of input space".to_string()));
        }

        if !is_bad(self.badval) {
            node.push_double(
                "BadVal",
                self.badval,
                Some("Output value for bad input positions".to_string()),
            );
        }

        Ok(node)
    }

    fn from_node(node: &crate::serial::Node) -> AstResult<Self> {
        use crate::serial::{read_mapping_base, region_from_node};

        let base = read_mapping_base(node);

        let mut regions: Vec<DynRegion> = Vec::new();
        let mut i = 1;
        loop {
            let key = format!("Reg{}", i);
            match node.get_object(&key) {
                Some(reg_node) => {
                    regions.push(region_from_node(reg_node)?);
                }
                None => break,
            }
            i += 1;
        }

        if regions.is_empty() {
            return Err(AstError::new(
                ErrorCode::Lderr,
                "SelectorMap: no regions found in serialized data",
            ));
        }

        let badval = node.get_double("BadVal", BAD_VALUE);

        let mut sm = SelectorMapBuilder::new(regions)
            .map_err(|e| AstError::new(ErrorCode::Lderr, format!("SelectorMap: {e}")))?
            .badval(badval)
            .build();

        if base.inverted {
            sm.invert();
        }

        sm.set_has_been_simplified(base.has_been_simplified);
        Ok(sm)
    }
}

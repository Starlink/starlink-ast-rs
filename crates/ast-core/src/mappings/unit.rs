//! The identity mapping.

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::{DynMapping, Mapping};

/// The identity mapping: every output coordinate equals the corresponding input.
#[derive(Debug, Clone)]
pub struct UnitMap {
    ncoord: usize,
    inverted: bool,
    do_not_simplify: bool,
}

impl UnitMap {
    /// Create a new `UnitMap` with `ncoord` coordinates.
    pub fn new(ncoord: usize) -> Self {
        Self {
            ncoord,
            inverted: false,
            do_not_simplify: false,
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

    // `transform_into_nan` is intentionally not overridden: the inherited
    // default delegates to `transform_into` above, which is a pure copy with
    // no per-point `is_bad` check or value-dependent branching. NaN inputs
    // pass through to NaN outputs naturally, so the trait default already
    // satisfies the NaN-as-bad protocol.

    fn simplified(&self) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap {
            ncoord: self.ncoord,
            inverted: false,
            do_not_simplify: false,
        }))
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        Ok(if ax1 == ax2 { 1.0 } else { 0.0 })
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<UnitMap>() else {
            return false;
        };
        self.ncoord == other.ncoord
    }

    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors unitmap.c::MapSplit (~629). UnitMap is identity per axis,
        // so any subset of inputs is splittable: the result is just a
        // smaller UnitMap, and the chosen output indices match the chosen
        // input indices.
        let nin = self.nin();
        crate::mappings::helpers::validate_in_axes(in_axes, nin)?;
        Some((in_axes.to_vec(), Box::new(UnitMap::new(in_axes.len()))))
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        _self_inv: bool,
        _other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            // Parallel: merge adjacent UnitMaps into one with combined ncoord.
            let other_unit = other.as_any().downcast_ref::<UnitMap>()?;
            Some(Box::new(UnitMap::new(self.ncoord + other_unit.ncoord)))
        } else {
            // Series: UnitMap removal handled by post-loop cleanup in merge_loop.
            None
        }
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

impl Serializable for UnitMap {
    fn class_name(&self) -> &'static str {
        "UnitMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Unit (null) Mapping".to_string());
        write_mapping_base(&mut node, self);
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);
        Ok(UnitMap {
            ncoord: base.nin,
            inverted: base.inverted,
            do_not_simplify: base.do_not_simplify,
        })
    }
}

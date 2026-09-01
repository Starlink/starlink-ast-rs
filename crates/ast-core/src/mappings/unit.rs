//! The identity mapping.

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// The identity mapping: every output coordinate equals the corresponding input.
#[derive(Debug, Clone)]
pub struct UnitMap {
    ncoord: usize,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
}

impl UnitMap {
    /// Create a new `UnitMap` with `ncoord` coordinates.
    pub fn new(ncoord: usize) -> Self {
        Self {
            ncoord,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
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

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        Ok(if ax1 == ax2 { 1.0 } else { 0.0 })
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<UnitMap>() else {
            return false;
        };
        self.ncoord == other.ncoord
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // Faithful port of C unitmap.c::MapMerge (unitmap.c:357), series case
        // only. In series with any other Mapping (nmap > 1) a UnitMap is
        // simply eliminated (unitmap.c:519): the following elements shuffle
        // down and the index of the first modified element is `where_`.
        //
        // Removing the UnitMap *in-loop* (rather than in a post-loop strip) is
        // what advances the nominate cursor exactly as C does — after this
        // returns `where_`, the map that shuffled into `where_` is nominated
        // next, which is how C reaches e.g. `MatrixMap::MapMerge` (MatWin2)
        // for a Win-Unit-Matrix run instead of the WinMap's own merge.
        //
        // The parallel case (adjacent-UnitMap coalescing and Invert clearing)
        // and the lone-UnitMap Invert clearing are left to the existing
        // post-loop handling, which the current fixtures already match.
        if ctx.len() == 1 {
            // A lone UnitMap: clear a set Invert flag, reporting the change
            // (unitmap.c:508). Matches C returning `result = 0` when the
            // single-map Invert flag was set.
            let entry = ctx.entry(0);
            if entry.1 || entry.2 {
                ctx.set_pair_single(0, (Box::new(UnitMap::new(self.ncoord)), false, false));
                return Some(0);
            }
            return None;
        }

        if series {
            ctx.remove_at(where_);
            return Some(where_);
        }

        super::simplify::default_map_merge(ctx, where_, series)
    }

    fn map_split_zero_based(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
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

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = if ident.is_empty() {
            None
        } else {
            Some(ident.to_string())
        };
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
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
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
        })
    }
}

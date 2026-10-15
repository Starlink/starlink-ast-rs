//! Constant offset mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::unit::UnitMap;
use super::unitnormmap::try_merge_into_unitnormmap;
use super::winmap::{compose_linear, extract_linear_params};
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// A mapping that adds a constant shift vector to all input coordinates.
#[derive(Debug, Clone)]
pub struct ShiftMap {
    shift: Vec<f64>,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl ShiftMap {
    /// Create a new `ShiftMap` with the given shift offsets.
    ///
    /// The number of coordinates is determined by the length of `shift`.
    pub fn new(shift: impl Into<Vec<f64>>) -> Self {
        Self {
            shift: shift.into(),
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }

    /// The shift offsets.
    pub(crate) fn shifts(&self) -> &[f64] {
        &self.shift
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
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    /// Linear, as C's `GetIsLinear` for the class reports.
    fn is_linear(&self) -> bool {
        true
    }

    fn nin(&self) -> usize {
        self.shift.len()
    }
    fn nout(&self) -> usize {
        self.shift.len()
    }
    fn is_inverted(&self) -> bool {
        self.inverted
    }
    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let mut out = PointSet::zeros(pts.npoint(), self.shift.len());
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        let fwd = self.effective_forward(forward);
        let sign = if fwd { 1.0 } else { -1.0 };
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                // A bad shift makes its axis bad (shiftmap.c Transform).
                *o = if is_bad(v) || self.shift[j] == BAD_VALUE {
                    BAD_VALUE
                } else {
                    v + sign * self.shift[j]
                };
            }
        }
        Ok(())
    }

    /// NaN-as-bad fast path: pure linear addition; NaN propagates cleanly,
    /// and an axis with a bad shift is NaN.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let sign = if fwd { 1.0 } else { -1.0 };
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                *o = if self.shift[j] == BAD_VALUE {
                    f64::NAN
                } else {
                    v + sign * self.shift[j]
                };
            }
        }
        Ok(())
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, _at, ax1, ax2)?;
        Ok(if ax1 == ax2 { 1.0 } else { 0.0 })
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<ShiftMap>() else {
            return false;
        };
        if self.shift.len() != other.shift.len() {
            return false;
        }
        if self.inverted == other.inverted {
            self.shift
                .iter()
                .zip(&other.shift)
                .all(|(a, b)| ast_equal(*a, *b))
        } else {
            self.shift
                .iter()
                .zip(&other.shift)
                .all(|(a, b)| ast_equal(*a, -b))
        }
    }

    fn map_split_zero_based(
        &self,
        in_axes: &[usize],
    ) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        // C allocates the returned output indices with astMalloc, which
        // gives NULL for none, so an empty selection gives no split.
        if in_axes.is_empty() {
            return Ok(None);
        }
        // shiftmap.c MapSplit: any selection of inputs in range splits off a
        // ShiftMap of the selected shifts, a repeated one included, inverted
        // when this one is; one out of range gives no split.
        if in_axes.iter().any(|&ix| ix >= self.nin()) {
            return Ok(None);
        }
        let shifts: Vec<f64> = in_axes.iter().map(|&ix| self.shift[ix]).collect();
        let mut sub = ShiftMap::new(shifts);
        if self.inverted {
            sub.invert();
        }
        Ok(Some((in_axes.to_vec(), Box::new(sub))))
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // A ShiftMap is equivalent to a WinMap with unit scaling. Mirror C
        // `shiftmap.c::MapMerge` (671-745): convert to an equivalent forward
        // WinMap, delegate to WinMap's MapMerge (which has the full
        // swap/merge logic), then revert to a ShiftMap if the WinMap merge
        // did nothing or merely turned the WinMap back into this ShiftMap.
        //
        // Every neighbour class a ShiftMap could pair with is reached through
        // that conversion, including another ShiftMap (C `WinShift`) and a
        // UnitNormMap — C notes at unitnormmap.c:359 that its own
        // ShiftMap+UnitNormMap branch never runs precisely because the
        // ShiftMap has become a WinMap by this point.
        let n = self.shift.len();
        let eff_inv = ctx.entry(where_).1;

        // Effective-forward shift terms (fold the applied inversion in), so
        // the WinMap is stored forward with invert flag cleared (C sets
        // invert_list[where] = 0).
        let fwd_shifts: Vec<f64> = if eff_inv {
            self.shift.iter().map(|&s| -s).collect()
        } else {
            self.shift.clone()
        };
        let scales = vec![1.0; n];
        let win = match super::winmap::WinMap::new(&fwd_shifts, &scales) {
            Ok(w) => w,
            Err(_) => return super::simplify::default_map_merge(ctx, where_, series),
        };

        let old_len = ctx.len();
        ctx.set_pair_single(where_, (Box::new(win.clone()), false, false));

        super::simplify::trace::map_merge(&win, false, where_, series, ctx.len());
        let result = super::winmap_merge::win_map_merge(&win, ctx, where_, series);
        super::simplify::trace::map_merge_result(&win, result, ctx.len());

        // Revert if: the WinMap merge made no change (None), or it produced
        // an equivalent lone ShiftMap at `where_` with the list length
        // unchanged (C's revert guard, shiftmap.c:716-745).
        let reverted_to_equiv = result == Some(where_)
            && ctx.len() == old_len
            && ctx
                .entry(where_)
                .0
                .as_any()
                .downcast_ref::<ShiftMap>()
                .is_some_and(|sm| sm.equal(self));

        if result.is_none() || reverted_to_equiv {
            // Restore a forward ShiftMap. If the original was applied
            // inverted, the returned forward ShiftMap holds the negated
            // shifts and we report a change (index); otherwise no change.
            // C reverts with `astClone( this )` when the ShiftMap was applied
            // forward, and with `astCopy( this )` plus negated shift terms when
            // it was applied inverted (shiftmap.c:721-741). Both carry the
            // object's `flags`, so a ShiftMap that reached the list already
            // recorded as simplified stays recorded across the WinMap round
            // trip; building a fresh ShiftMap here would drop the record and
            // suppress the `IsSimp` card C writes for it.
            let mut restored = self.clone();
            restored.shift = fwd_shifts;
            restored.inverted = false;
            ctx.set_pair_single(where_, (Box::new(restored), false, false));
            return if eff_inv { Some(where_) } else { None };
        }

        result
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

        // Fast path: ShiftMap + ShiftMap → ShiftMap (preserves type).
        if let Some(other_shift) = other.as_any().downcast_ref::<ShiftMap>() {
            if self.shift.len() != other_shift.shift.len() {
                return None;
            }
            let mut combined = Vec::with_capacity(self.shift.len());
            for i in 0..self.shift.len() {
                let s1 = if self_inv {
                    -self.shift[i]
                } else {
                    self.shift[i]
                };
                let s2 = if other_inv {
                    -other_shift.shift[i]
                } else {
                    other_shift.shift[i]
                };
                combined.push(s1 + s2);
            }
            if combined.iter().all(|&s| ast_equal(s, 0.0)) {
                return Some(Box::new(UnitMap::new(self.shift.len())));
            }
            return Some(Box::new(ShiftMap::new(combined)));
        }

        // ShiftMap + UnitNormMap(forward) → UnitNormMap(forward).
        if let Some(unm) = other
            .as_any()
            .downcast_ref::<super::unitnormmap::UnitNormMap>()
        {
            return try_merge_into_unitnormmap(self, self_inv, unm, other_inv);
        }

        // Cross-type: ShiftMap + {WinMap, ZoomMap} → WinMap or UnitMap.
        let n = self.shift.len();
        let s1: Vec<f64> = self
            .shift
            .iter()
            .map(|&s| if self_inv { -s } else { s })
            .collect();
        let c1 = vec![1.0; n];
        let (s2, c2) = extract_linear_params(other, other_inv)?;
        if n != s2.len() {
            return None;
        }
        // C converts a nominated ShiftMap to a WinMap and delegates to the
        // WinMap MapMerge, which reaches WinWin (with its rounding snaps)
        // only when the neighbour is itself a WinMap.
        compose_linear(
            &s1,
            &c1,
            &s2,
            &c2,
            other.as_any().is::<super::winmap::WinMap>(),
        )
    }

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }
    fn clear_id(&mut self) {
        self.id.clear();
    }
    fn test_id(&self) -> bool {
        self.id.test()
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

impl Serializable for ShiftMap {
    fn class_name(&self) -> &'static str {
        "ShiftMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Shift each coordinate axis".to_string());
        write_mapping_base(&mut node, self);
        // Each shift, set when not zero.
        for (i, &s) in self.shift.iter().enumerate() {
            node.write_double(
                &format!("Sft{}", i + 1),
                s != 0.0,
                false,
                s,
                &format!("Shift for axis {}", i + 1),
            );
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("ShiftMap");
        let base = read_mapping_base(whole);
        let mut shifts = Vec::with_capacity(base.nin);
        for i in 0..base.nin {
            shifts.push(node.get_double(&format!("Sft{}", i + 1), 0.0));
        }
        Ok(ShiftMap {
            shift: shifts,
            inverted: base.inverted,
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        })
    }
}

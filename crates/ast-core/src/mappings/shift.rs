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

/// A mapping that adds a constant shift vector to all input coordinates.
#[derive(Debug, Clone)]
pub struct ShiftMap {
    shift: Vec<f64>,
    inverted: bool,
    has_been_simplified: bool,
}

impl ShiftMap {
    /// Create a new `ShiftMap` with the given shift offsets.
    ///
    /// The number of coordinates is determined by the length of `shift`.
    pub fn new(shift: impl Into<Vec<f64>>) -> Self {
        Self {
            shift: shift.into(),
            inverted: false,
            has_been_simplified: false,
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
        // Inversion invalidates the "already simplified" flag, since the
        // inverse may simplify further (C astSetInvert clears IsSimple).
        self.has_been_simplified = false;
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
            for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                *o = if is_bad(v) {
                    BAD_VALUE
                } else {
                    v + sign * self.shift[j]
                };
            }
        }
        Ok(())
    }

    /// NaN-as-bad fast path: pure linear addition; NaN propagates cleanly.
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
                *o = v + sign * self.shift[j];
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.has_been_simplified {
            return Ok(Box::new(self.clone()));
        }
        if self.shift.iter().all(|&s| s == 0.0) {
            Ok(Box::new(UnitMap::new(self.shift.len())))
        } else if self.inverted {
            let normalized: Vec<f64> = self.shift.iter().map(|&s| -s).collect();
            Ok(Box::new(ShiftMap {
                shift: normalized,
                inverted: false,
                has_been_simplified: false,
            }))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
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

    fn map_split_zero_based(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors shiftmap.c::MapSplit (~750). ShiftMap is per-axis additive
        // offset, so a subset of inputs maps to the same-indexed outputs
        // via a smaller ShiftMap built from the chosen axes' shift values.
        let nin = self.nin();
        crate::mappings::helpers::validate_in_axes(in_axes, nin)?;
        let shifts: Vec<f64> = in_axes.iter().map(|&ix| self.shift[ix]).collect();
        let mut sub = ShiftMap::new(shifts);
        if self.inverted {
            sub.invert();
        }
        Some((in_axes.to_vec(), Box::new(sub)))
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
        // First try the direct pairwise merge (ShiftMap+ShiftMap, and the
        // UnitNormMap absorption via `try_merge`). These already match C and
        // must not be bypassed by the WinMap conversion below.
        if let Some(r) = super::simplify::default_map_merge(ctx, where_, series) {
            return Some(r);
        }

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

        let result = super::winmap_merge::win_map_merge(&win, ctx, where_, series);

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
            let restored = ShiftMap {
                shift: fwd_shifts,
                inverted: false,
                has_been_simplified: false,
            };
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
        compose_linear(&s1, &c1, &s2, &c2)
    }

    fn has_been_simplified(&self) -> bool {
        self.has_been_simplified
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.has_been_simplified = flag;
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
        for (i, &s) in self.shift.iter().enumerate() {
            if s != 0.0 {
                node.push_double(
                    &format!("Sft{}", i + 1),
                    s,
                    Some(format!("Shift for axis {}", i + 1)),
                );
            }
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);
        let mut shifts = Vec::with_capacity(base.nin);
        for i in 0..base.nin {
            shifts.push(node.get_double(&format!("Sft{}", i + 1), 0.0));
        }
        Ok(ShiftMap {
            shift: shifts,
            inverted: base.inverted,
            has_been_simplified: base.has_been_simplified,
        })
    }
}

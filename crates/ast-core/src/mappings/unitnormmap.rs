//! Mapping that decomposes a vector into a unit vector and its norm.
//!
//! A `UnitNormMap` converts a vector to a unit vector and its norm,
//! relative to a specified centre.
//!
//! The forward transformation subtracts the specified centre and then
//! transforms the resulting vector to a unit vector and the vector norm.
//! The output contains one more coordinate than the input: the initial
//! Nin outputs are in the same order as the input; the final output is
//! the norm.
//!
//! The inverse transformation multiplies each component of the provided
//! unit vector by the provided norm and adds the specified centre.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::rate::numeric_rate;
use super::shift::ShiftMap;
use super::unit::UnitMap;
use super::winmap::WinMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// A mapping that decomposes a vector into unit vector and norm.
///
/// The forward transformation subtracts the given centre from the input
/// vector, then computes the unit vector and norm (length) of the result.
/// Output has one more axis than input: the first Nin axes hold the unit
/// vector components, the last axis holds the norm.
///
/// If the norm is zero, the unit vector components are set to
/// [`BAD_VALUE`] and the norm output is zero.
///
/// Example: centre = `[1, -1]`, input = `[5, 2]`.
/// After subtracting centre: `[4, 3]`, norm = 5.
/// Output: `[0.8, 0.6, 5.0]`.
#[derive(Debug, Clone)]
pub struct UnitNormMap {
    centre: Vec<f64>,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl UnitNormMap {
    /// Create a new `UnitNormMap` with the given centre.
    ///
    /// `centre` defines the origin from which the unit vector and norm
    /// are computed. Its length determines the number of spatial axes; an
    /// empty centre is an `AST__BADSM` error (unitnormmap.c
    /// astInitUnitNormMap).
    pub fn new(centre: impl Into<Vec<f64>>) -> AstResult<Self> {
        let centre = centre.into();
        if centre.is_empty() {
            return Err(AstError::new(
                ErrorCode::Badsm,
                "UnitNormMap: the centre must have at least one axis",
            ));
        }
        Ok(Self {
            centre,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        })
    }

    /// The centre vector.
    pub fn centre(&self) -> &[f64] {
        &self.centre
    }

    /// The number of spatial axes (length of the centre array).
    fn ncoord(&self) -> usize {
        self.centre.len()
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

impl Mapping for UnitNormMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        if self.inverted {
            self.ncoord() + 1
        } else {
            self.ncoord()
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            self.ncoord()
        } else {
            self.ncoord() + 1
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let ncoord = self.ncoord();
        let fwd = self.effective_forward(forward);
        let nout = if fwd { ncoord + 1 } else { ncoord };
        let mut out = PointSet::zeros(pts.npoint(), nout);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    /// NaN-as-bad fast path. UnitNormMap has several BAD-emit sites
    /// (BAD centre, BAD per-axis input, zero-norm "indeterminate
    /// direction" output, BAD norm on inverse, etc.). All of them
    /// should appear as NaN inside the NaN-as-bad pipeline so
    /// downstream stages handle them uniformly. The norm output for the
    /// zero-vector case is 0.0, which is preserved (it's a genuine
    /// finite value, not a sentinel).
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        self.transform_into(pts, forward, out)?;
        let mut view = out.view_mut();
        let flat = view.as_slice_mut().expect("PointSet is C-contiguous");
        for v in flat.iter_mut() {
            if *v == BAD_VALUE {
                *v = f64::NAN;
            }
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        let fwd = self.effective_forward(forward);
        let ncoord = self.ncoord();

        // Check if any centre coordinate is bad — if so, all outputs are bad.
        for &c in &self.centre {
            if is_bad(c) {
                for i in 0..pts.npoint() {
                    let outp = out.point_mut(i);
                    for o in outp.iter_mut() {
                        *o = BAD_VALUE;
                    }
                }
                return Ok(());
            }
        }

        if fwd {
            // Forward: vector → unit vector + norm
            let ncoord_out = ncoord + 1;
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);

                // Check for any bad input — if so, all outputs are bad.
                let mut any_bad = false;
                let mut max_relin: f64 = 0.0;
                for (coord, (&v, &c)) in inp.iter().zip(self.centre.iter()).enumerate() {
                    if coord >= ncoord {
                        break;
                    }
                    if is_bad(v) {
                        any_bad = true;
                        break;
                    }
                    let abs_relin = (v - c).abs();
                    if abs_relin > max_relin {
                        max_relin = abs_relin;
                    }
                }

                if any_bad {
                    for o in outp[..ncoord_out].iter_mut() {
                        *o = BAD_VALUE;
                    }
                    continue;
                }

                if max_relin == 0.0 {
                    // Norm is zero: unit vector is unknown, norm is 0.
                    for o in outp[..ncoord].iter_mut() {
                        *o = BAD_VALUE;
                    }
                    outp[ncoord] = 0.0;
                    continue;
                }

                // Compute norm using scaled sum to avoid overflow.
                let mut scaled_sum: f64 = 0.0;
                for (&v, &c) in inp[..ncoord].iter().zip(self.centre.iter()) {
                    let scaled = (v - c) / max_relin;
                    scaled_sum += scaled * scaled;
                }
                let norm = max_relin * scaled_sum.sqrt();

                // Output unit vector and norm.
                for (coord, (&v, &c)) in inp[..ncoord].iter().zip(self.centre.iter()).enumerate() {
                    outp[coord] = (v - c) / norm;
                }
                outp[ncoord] = norm;
            }
        } else {
            // Inverse: unit vector + norm → vector
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);

                // The last input coordinate is the norm.
                let norm = inp[ncoord];

                if is_bad(norm) {
                    // Norm is bad: all outputs bad.
                    for o in outp[..ncoord].iter_mut() {
                        *o = BAD_VALUE;
                    }
                } else if norm == 0.0 {
                    // Norm is zero: output is the centre.
                    outp[..ncoord].copy_from_slice(&self.centre[..ncoord]);
                } else {
                    // Multiply unit vector by norm and add centre.
                    for (coord, (&v, &c)) in
                        inp[..ncoord].iter().zip(self.centre.iter()).enumerate()
                    {
                        if is_bad(v) {
                            outp[coord] = BAD_VALUE;
                        } else {
                            outp[coord] = v * norm + c;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<UnitNormMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        if self.ncoord() != other.ncoord() {
            return false;
        }
        for i in 0..self.ncoord() {
            if !ast_equal(self.centre[i], other.centre[i]) {
                return false;
            }
        }
        true
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

        // Determine effective forward/inverse state for self and other.
        // self_inv/other_inv indicate whether each mapping should be treated
        // as inverted in this composition.
        let self_is_inverted = self_inv != self.inverted;

        // Case: UnitNormMap + UnitNormMap (opposite directions)
        if let Some(other_unm) = other.as_any().downcast_ref::<UnitNormMap>() {
            let other_effective_inv = other_inv != other_unm.inverted;

            // Must be opposite directions to merge.
            if self_is_inverted == other_effective_inv {
                return None;
            }

            // Check if centres are the same.
            let centres_equal = self.ncoord() == other_unm.ncoord()
                && self
                    .centre
                    .iter()
                    .zip(other_unm.centre.iter())
                    .all(|(&a, &b)| ast_equal(a, b));

            if centres_equal {
                // Identical centres cancel to UnitMap.
                // The UnitMap has nin of the forward map.
                let n = if self_is_inverted {
                    self.ncoord() + 1
                } else {
                    self.ncoord()
                };
                return Some(Box::new(UnitMap::new(n)));
            }

            // Different centres: forward + inverse = ShiftMap
            if !self_is_inverted && other_effective_inv {
                // UnitNormMap(forward) + UnitNormMap(inverse) → ShiftMap
                let shift: Vec<f64> = self
                    .centre
                    .iter()
                    .zip(other_unm.centre.iter())
                    .map(|(&a, &b)| b - a)
                    .collect();
                return Some(Box::new(ShiftMap::new(shift)));
            }

            return None;
        }

        // Case: ShiftMap + UnitNormMap(forward)
        // This is: self = ShiftMap, other = UnitNormMap — but try_merge is called
        // ON this (the UnitNormMap). So we need to handle:
        //   - other is a ShiftMap and self is forward → not this case
        //   - other is a ShiftMap and self is inverted → UnitNormMap(inv) + ShiftMap

        if let Some(shift_map) = other.as_any().downcast_ref::<ShiftMap>() {
            // UnitNormMap(inverted) + ShiftMap = UnitNormMap(inverted) with adjusted centre
            if !self_is_inverted {
                return None; // UnitNormMap(forward) + ShiftMap not supported
            }

            let shift_effective_inv = other_inv != shift_map.is_inverted();
            let shift_mult = if shift_effective_inv { -1.0 } else { 1.0 };
            let shifts = shift_map.shifts();

            if shifts.len() != self.ncoord() {
                return None;
            }

            let new_centre: Vec<f64> = self
                .centre
                .iter()
                .zip(shifts.iter())
                .map(|(&c, &s)| c + shift_mult * s)
                .collect();

            let mut result = UnitNormMap::new(new_centre).ok()?;
            result.invert(); // Return in inverted state
            return Some(Box::new(result));
        }

        // Case: UnitNormMap(inverted) + WinMap with unit scale
        if let Some(win_map) = other.as_any().downcast_ref::<WinMap>() {
            if !self_is_inverted {
                return None; // UnitNormMap(forward) + WinMap not supported
            }

            let win_effective_inv = other_inv != win_map.is_inverted();
            let shift_mult = if win_effective_inv { -1.0 } else { 1.0 };
            let scales = win_map.scales();
            let shifts = win_map.shifts();

            if scales.len() != self.ncoord() {
                return None;
            }

            // Check unit scale on all axes.
            for &s in scales {
                if !ast_equal(s, 1.0) {
                    return None;
                }
            }

            let new_centre: Vec<f64> = self
                .centre
                .iter()
                .zip(shifts.iter())
                .map(|(&c, &s)| c + shift_mult * s)
                .collect();

            let mut result = UnitNormMap::new(new_centre).ok()?;
            result.invert(); // Return in inverted state
            return Some(Box::new(result));
        }

        // Case: UnitMap → keep self unchanged
        if other.as_any().downcast_ref::<UnitMap>().is_some() {
            let mut result = self.clone();
            result.inverted = self_inv;
            return Some(Box::new(result));
        }

        None
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C `unitnormmap.c::MapMerge` (820-880). A UnitNormMap cannot combine
        // in parallel with anything.
        if !series {
            return None;
        }

        // C walks every pair from `i1 = MAX( where - 1, 0 )` to the end of the
        // list, taking the first that merges, so a merge is found whether the
        // UnitNormMap is the second member of the pair (the
        // `<linear> ; UnitNormMap(forward)` absorptions) or the first (the
        // `UnitNormMap(inverted) ; <linear>` ones). Neither `default_map_merge`
        // nor the neighbours' own MapMerge covers the lower pair: WinMap's
        // routes every case through `merge_linear_pair`, which handles only
        // linear classes.
        let i1_start = where_.saturating_sub(1);
        for i1 in i1_start..ctx.len().saturating_sub(1) {
            let i2 = i1 + 1;
            let (m1, inv1, _) = ctx.entry(i1);
            let (m2, inv2, _) = ctx.entry(i2);
            if let Some(merged) = make_merged_map(m1.as_ref(), *inv1, m2.as_ref(), *inv2) {
                ctx.replace_pair_with(i1, merged, false);
                return Some(i1);
            }
        }

        None
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

/// Merge an adjacent pair of Mappings, one of which must be a UnitNormMap.
///
/// Port of C `unitnormmap.c::MakeMergedMap` (323). The supported series
/// merges, in C's own order, are:
///
/// | first | second | result |
/// |---|---|---|
/// | ShiftMap | UnitNormMap(forward) | UnitNormMap(forward) |
/// | WinMap (unit scale) | UnitNormMap(forward) | UnitNormMap(forward) |
/// | UnitNormMap(inverted) | ShiftMap | UnitNormMap(inverted) |
/// | UnitNormMap(inverted) | WinMap (unit scale) | UnitNormMap(inverted) |
/// | UnitNormMap | opposite-direction UnitNormMap | UnitMap or ShiftMap |
///
/// Returns `None` unless both members are one of UnitNormMap / ShiftMap /
/// WinMap and at least one is a UnitNormMap — C's `GetMappingType` gate.
fn make_merged_map(
    m1: &dyn Mapping,
    inv1: bool,
    m2: &dyn Mapping,
    inv2: bool,
) -> Option<DynMapping> {
    let unm1 = m1.as_any().downcast_ref::<UnitNormMap>();
    let unm2 = m2.as_any().downcast_ref::<UnitNormMap>();

    // One of the pair must be a UnitNormMap; the other must be a UnitNormMap,
    // a ShiftMap or a WinMap (C type 1/2/3, everything else type 0).
    let is_linear = |m: &dyn Mapping| {
        m.as_any().is::<ShiftMap>() || m.as_any().is::<WinMap>() || m.as_any().is::<UnitNormMap>()
    };
    if !is_linear(m1) || !is_linear(m2) {
        return None;
    }

    match (unm1, unm2) {
        // Two UnitNormMaps in opposite directions — C's final `else` branch,
        // reached through the nominated map's own pairwise merge.
        (Some(unm), Some(_)) => unm.try_merge(m2, true, inv1, inv2),
        // `<linear> ; UnitNormMap(forward)` — C's type1 == 2 / type1 == 3.
        (None, Some(unm)) => try_merge_into_unitnormmap(m1, inv1, unm, inv2),
        // `UnitNormMap(inverted) ; <linear>` — C's type2 == 2 / type2 == 3.
        (Some(unm), None) => unm.try_merge(m2, true, inv1, inv2),
        (None, None) => None,
    }
}

/// Try to merge another mapping followed by a UnitNormMap.
///
/// This is called from the ShiftMap and WinMap try_merge paths,
/// or from the simplification engine when iterating over map pairs.
/// We expose it as a free function so external merge logic can call it.
pub(crate) fn try_merge_into_unitnormmap(
    first: &dyn Mapping,
    first_inv: bool,
    unm: &UnitNormMap,
    unm_inv: bool,
) -> Option<DynMapping> {
    let unm_is_inverted = unm_inv != unm.inverted;

    // Only supported when UnitNormMap is in the forward direction.
    if unm_is_inverted {
        return None;
    }

    if let Some(shift_map) = first.as_any().downcast_ref::<ShiftMap>() {
        // ShiftMap + UnitNormMap(forward) = UnitNormMap(forward)
        let shift_effective_inv = first_inv != shift_map.is_inverted();
        let shift_mult = if shift_effective_inv { -1.0 } else { 1.0 };
        let shifts = shift_map.shifts();

        if shifts.len() != unm.ncoord() {
            return None;
        }

        let new_centre: Vec<f64> = unm
            .centre
            .iter()
            .zip(shifts.iter())
            .map(|(&c, &s)| c - shift_mult * s)
            .collect();

        return Some(Box::new(UnitNormMap::new(new_centre).ok()?));
    }

    if let Some(win_map) = first.as_any().downcast_ref::<WinMap>() {
        // WinMap(unit scale) + UnitNormMap(forward) = UnitNormMap(forward)
        let win_effective_inv = first_inv != win_map.is_inverted();
        let shift_mult = if win_effective_inv { -1.0 } else { 1.0 };
        let scales = win_map.scales();
        let shifts = win_map.shifts();

        if scales.len() != unm.ncoord() {
            return None;
        }

        // Check unit scale on all axes.
        for &s in scales {
            if !ast_equal(s, 1.0) {
                return None;
            }
        }

        let new_centre: Vec<f64> = unm
            .centre
            .iter()
            .zip(shifts.iter())
            .map(|(&c, &s)| c - shift_mult * s)
            .collect();

        return Some(Box::new(UnitNormMap::new(new_centre).ok()?));
    }

    None
}

impl Serializable for UnitNormMap {
    fn class_name(&self) -> &'static str {
        "UnitNormMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Compute unit vector and norm relative to a centre".to_string());
        write_mapping_base(&mut node, self);

        // Write centre values (only non-zero values, matching C's conditional write).
        for (axis, &c) in self.centre.iter().enumerate() {
            let key = format!("Ctr{}", axis + 1);
            if c != 0.0 {
                node.push_double(&key, c, Some(format!("Centre for axis {}", axis + 1)));
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("UnitNormMap");
        let base = read_mapping_base(whole);

        // The centre length equals the number of spatial axes, which is
        // always min(nin, nout) in the forward direction. The serialized
        // Nin/Nout are always the forward (non-inverted) dimensions
        // regardless of the Invert flag, so ncoord = base.nin (which is
        // always the smaller of the two for a UnitNormMap).
        let ncoord = base.nin;

        let mut centre = vec![0.0; ncoord];
        for (axis, c) in centre.iter_mut().enumerate() {
            let key = format!("Ctr{}", axis + 1);
            *c = node.get_double(&key, 0.0);
        }

        let mut m = UnitNormMap::new(centre)?;
        if base.inverted {
            m.invert();
        }
        m.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        Ok(m)
    }
}

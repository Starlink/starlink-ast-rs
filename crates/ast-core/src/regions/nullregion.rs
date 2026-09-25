//! NullRegion: a boundless region within a Frame.
//!
//! A NullRegion represents a region with no bounds. When not negated,
//! it contains no points. When negated, it represents the entire
//! coordinate system (all points are inside).

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::mappings::{Composition, DynMapping, Mapping, UnitMap, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Overlap, Region};

/// A region containing no points (or all points when negated).
///
/// The NullRegion is the simplest possible region. Its un-negated form
/// contains no points at all; its negated form contains every point in
/// the coordinate system.
#[derive(Debug, Clone)]
pub struct NullRegion {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing a [`NullRegion`].
pub struct NullRegionBuilder {
    frame: DynFrame,
    uncertainty: Option<DynRegion>,
}

impl NullRegionBuilder {
    /// Create a builder for a NullRegion in the given frame.
    pub fn new(frame: impl Frame + 'static) -> Self {
        NullRegionBuilder {
            frame: Box::new(frame),
            uncertainty: None,
        }
    }

    /// Create a builder from a boxed (dynamic) frame.
    pub fn from_dyn(frame: DynFrame) -> Self {
        NullRegionBuilder {
            frame,
            uncertainty: None,
        }
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the NullRegion.
    pub fn build(self) -> AstResult<NullRegion> {
        let frameset = FrameSet::new(self.frame);
        // NullRegion has no defining points.
        let points = Vec::new();
        let data = RegionData::new(frameset, points);
        let mut region = NullRegion { data };

        // astSetUnc, as C's constructor calls it (region.c:13372). A NullRegion
        // has no defining points, so C's re-centre step is skipped there too
        // (`if( this->points )`, region.c:10871-10876); the remap, the
        // negate-if-unbounded and the RegionFS tag all still apply.
        if let Some(unc) = self.uncertainty {
            region.set_uncertainty(unc);
        }

        Ok(region)
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl NullRegion {
    /// Mutate the current frame with region integrity enforcement.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
    {
        super::data::with_region_integrity(self, f)
    }

    /// Permute the axes of this region's current frame.
    pub fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.data.perm_axes(perm)
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for NullRegion {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        // NullRegion (un-negated) contains no points at all.
        Ok(vec![false; pts.npoint()])
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // An upper bound below the lower bound is how a Region reports that it
        // contains no points (nullregion.c:961).
        let naxes = self.data.frameset.base().naxes();
        Ok((vec![1.0; naxes], vec![-1.0; naxes]))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        // NullRegion has no boundary, so the mesh is empty.
        Ok(Vec::new())
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        Err(AstError::new(ErrorCode::Regcn, "NullRegion has no centre"))
    }

    fn is_bounded_inner(&self) -> bool {
        // NullRegion does not override the base Region GetBounded
        // (nullregion.c has no Bounded logic), so it inherits the base rule
        // (region.c:3358): bounded unless negated, and always bounded within a
        // SkyFrame. An un-negated NullRegion (the empty set) is therefore
        // bounded; its negated form (the whole coordinate system) is not.
        self.data.closed_curve_bounded()
    }

    fn remap_defining_points(&mut self, _mapping: &dyn Mapping, _forward: bool) -> AstResult<()> {
        // No defining points to remap.
        Ok(())
    }
}

// ============================================================================
// Region trait
// ============================================================================

impl Region for NullRegion {
    fn point_in_region(&self, _point: &[f64]) -> AstResult<bool> {
        // Un-negated NullRegion contains nothing; negated contains everything.
        Ok(self.data.negated)
    }

    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap> {
        super::overlap::compute_overlap(self, other)
    }

    fn negated_copy(&self) -> DynRegion {
        let mut copy = self.clone();
        copy.data.negated = !copy.data.negated;
        Box::new(copy)
    }

    fn negate(&mut self) {
        self.data.negated = !self.data.negated;
    }

    fn is_negated(&self) -> bool {
        self.data.negated
    }

    fn is_closed(&self) -> bool {
        self.data.closed
    }

    fn set_closed(&mut self, closed: bool) {
        self.data.closed = closed;
        self.data.closed_set = true;
    }

    fn test_closed(&self) -> bool {
        self.data.closed_set
    }

    fn set_closed_raw(&mut self, closed: bool, set: bool) {
        self.data.closed = closed;
        self.data.closed_set = set;
    }

    fn fill_factor(&self) -> f64 {
        self.data.effective_fill_factor()
    }

    fn set_fill_factor(&mut self, factor: f64) {
        self.data.set_fill_factor(factor);
    }

    fn test_fill_factor(&self) -> bool {
        self.data.fill_factor_set
    }

    fn clear_fill_factor(&mut self) {
        self.data.clear_fill_factor();
    }

    fn mesh_size(&self) -> usize {
        self.data.effective_mesh_size()
    }

    fn set_mesh_size(&mut self, size: usize) {
        self.data.set_mesh_size(size);
    }

    fn test_mesh_size(&self) -> bool {
        self.data.mesh_size_set
    }

    fn clear_mesh_size(&mut self) {
        self.data.clear_mesh_size();
    }

    fn is_bounded(&self) -> bool {
        self.is_bounded_inner()
    }

    fn is_adaptive(&self) -> bool {
        self.data.adaptive
    }

    fn set_adaptive(&mut self, adaptive: bool) {
        self.data.adaptive = adaptive;
    }

    fn has_dummy_fs(&self) -> bool {
        self.data.dummy_fs
    }

    fn set_region_fs(&mut self, frame: &dyn Frame) {
        self.data.set_region_fs(frame);
    }

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // C overrides the base-class method rather than inheriting it
        // (nullregion.c:1203): a NullRegion has no boundary to transform into
        // the current Frame, so the same empty-extent bounds are returned
        // whatever the base->current Mapping is.
        let naxes = self.data.frameset.base().naxes();
        Ok((vec![1.0; naxes], vec![-1.0; naxes]))
    }

    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        Err(AstError::new(
            ErrorCode::Regcn,
            "NullRegion has no bounding disc",
        ))
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        // NullRegion has no boundary mesh. Return a single-point PointSet
        // with BAD_VALUE on all axes (matching C behaviour).
        let naxes = self.data.frameset.base().naxes();
        let bad_point = vec![BAD_VALUE; naxes];
        Ok(PointSet::from_rows(std::iter::once(bad_point.as_slice())))
    }

    fn get_region_grid(&self, _npoints: usize) -> AstResult<Vec<Vec<f64>>> {
        // NullRegion has no interior points.
        Ok(Vec::new())
    }

    fn get_region_points(&self) -> AstResult<PointSet> {
        // No defining points.
        let naxes = self.data.frameset.base().naxes();
        Ok(PointSet::zeros(0, naxes))
    }

    fn get_region_frame(&self) -> DynFrame {
        self.data.frameset.current().snapshot()
    }

    fn get_region_frameset(&self) -> FrameSet {
        self.data.frameset.clone()
    }

    fn region_frameset_mut(&mut self) -> &mut FrameSet {
        &mut self.data.frameset
    }

    fn store_uncertainty(&mut self, unc: Option<DynRegion>) {
        self.data.uncertainty = unc;
        self.data.invalidate_cache();
    }

    fn set_region_fs_flag(&mut self, value: bool) {
        self.data.set_region_fs_flag(value);
    }

    fn region_fs_flag(&self) -> bool {
        self.data.region_fs
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn get_def_unc(&self) -> AstResult<DynRegion> {
        // Port of nullregion.c::GetDefUnc (nullregion.c:170): the default
        // uncertainty is a zero-radius Circle centred on the origin of the
        // base Frame, not the base-class bounding box.
        let bfrm = self.data.frameset.base().snapshot();
        let n = bfrm.naxes();
        let circle = super::CircleBuilder::from_dyn(bfrm, vec![0.0; n])?
            .radius(0.0)
            .build()?;
        Ok(Box::new(circle))
    }

    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        // Port of nullregion.c::RegBasePick (nullregion.c:1106): rebuild a
        // NullRegion on the picked base Frame. NullRegion has no defining
        // points; the negation flag is copied later by reg_overlay.
        let bfrm = self.data.frameset.base();
        let target: Vec<i32> = axes.iter().map(|&a| a as i32).collect();
        let frm = bfrm.pick_axes_snapshot(&target)?;

        // Select the same axes from the base-Frame uncertainty (if any); drop
        // it if picking does not yield a Region (nullregion.c:1174-1183).
        let unc = match &self.data.uncertainty {
            Some(u) => match super::pick_axes_region(u.as_ref(), axes)? {
                super::PickedAxes::Region(r) => Some(r),
                super::PickedAxes::Frame(_) => None,
            },
            None => None,
        };

        let frameset = FrameSet::new(frm);
        let mut data = RegionData::new(frameset, Vec::new());
        data.uncertainty = unc;
        Ok(Some(Box::new(NullRegion { data })))
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    fn region_merge(&self, other_reg: &dyn Region, intfirst: bool) -> Option<DynMapping> {
        // Faithful port of `nullregion.c::MergeNullRegion` (571). A NullRegion
        // only merges with another NullRegion (other classes handle merging a
        // NullRegion with themselves). `intfirst` selects the axis order.
        //
        // Guard (nullregion.c:645, 672): the same Closed, either both or
        // neither with a defined uncertainty, and equal Negated.
        let other = other_reg.as_any().downcast_ref::<NullRegion>()?;
        if self.is_closed() != other.is_closed()
            || self.test_unc() != other.test_unc()
            || self.data.negated != other.data.negated
        {
            return None;
        }

        // The merged Region is built on a CmpFrame of the two *base* Frames
        // and mapped into the current Frames at the end (nullregion.c:652-661).
        let bfrm_this = self.data.frameset.base().snapshot();
        let bfrm_reg = other_reg.get_region_frameset().base().snapshot();
        let bfrm = if intfirst {
            crate::frames::CmpFrame::new(bfrm_this, bfrm_reg)
        } else {
            crate::frames::CmpFrame::new(bfrm_reg, bfrm_this)
        };

        // NullRegion has no defining points.
        let frameset = FrameSet::new(Box::new(bfrm) as DynFrame);
        let mut new = NullRegion {
            data: RegionData::new(frameset, Vec::new()),
        };

        // Propagate the supplied Region's attributes (nullregion.c:675). C
        // overlays from `this` alone, not from a combination of the two, and
        // asks for the uncertainty to be cleared so the one installed below is
        // the only one.
        new.reg_overlay(self, true);

        // Ensure Negated is set correctly (nullregion.c:678-682). The guard
        // above already requires the two to agree.
        new.data.negated = self.data.negated;

        // When both supplied Regions have an uncertainty, the merged Region's
        // uncertainty is a Prism of the two base-Frame uncertainties,
        // simplified, installed through astSetUnc (nullregion.c:685-705).
        if self.test_unc() && other.test_unc() {
            let unc_this = self.get_unc_frm(FrameRef::Base).ok()?;
            let unc_reg = other_reg.get_unc_frm(FrameRef::Base).ok()?;
            let (first, second) = if intfirst {
                (unc_this, unc_reg)
            } else {
                (unc_reg, unc_this)
            };
            let bunc = super::PrismBuilder::from_dyn(first, second)
                .and_then(|b| b.build())
                .ok()?;
            let sbunc = <dyn Mapping>::simplify(&bunc).ok()?;
            let sbunc = super::merge_util::mapping_into_region(sbunc)?;
            new.set_uncertainty(sbunc);
        }

        // The current Frames, combined in the same order (nullregion.c:713-721).
        let cfrm_this = self.data.frameset.current().snapshot();
        let cfrm_reg = other_reg.get_region_frame();
        let cfrm = if intfirst {
            crate::frames::CmpFrame::new(cfrm_this, cfrm_reg)
        } else {
            crate::frames::CmpFrame::new(cfrm_reg, cfrm_this)
        };

        // The two base->current Mappings, in parallel (nullregion.c:724-735).
        let map_this = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)
            .ok()?;
        let map_reg = other_reg
            .get_region_frameset()
            .mapping(FrameRef::Base, FrameRef::Current)
            .ok()?;
        let bcmap = if intfirst {
            crate::mappings::CmpMap::new(map_this, map_reg, Composition::Parallel).ok()?
        } else {
            crate::mappings::CmpMap::new(map_reg, map_this, Composition::Parallel).ok()?
        };

        // Map the merged Region into the new current Frame (nullregion.c:738).
        let mut result = new.map_region(&bcmap, &cfrm).ok()?;

        // FillFactor product and MeshSize ratio scaling, applied to the mapped
        // result as C applies them (nullregion.c:740-776).
        let scaling = super::merge_util::MergeScaling {
            msz_this: (
                self.data.effective_mesh_size(),
                self.data.mesh_size_set,
                self.naxes(),
            ),
            msz_reg: (other.mesh_size(), other.test_mesh_size(), other.naxes()),
            fill_this: (self.data.effective_fill_factor(), self.data.fill_factor_set),
            fill_reg: (other.fill_factor(), other.test_fill_factor()),
        };
        super::merge_util::apply_merge_scaling_to(result.as_mut(), &scaling);

        // A Region is a Mapping; the sibling-trait port needs the upcast
        // written down.
        Some(result.as_dyn_mapping().clone_box())
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for NullRegion {
    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        self.data.frameset.current().primary_frame(axis)
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.data.frameset.current().format(axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.data.frameset.current().unformat(axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().distance(a, b)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().angle(a, b, c)
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        self.data.frameset.current().offset(from, to, dist)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.data.frameset.current().norm(coords)
    }

    /// A Region's axes are its encapsulated current Frame's axes. Mirrors
    /// C `Region::AxDistance` / `AxOffset` / `AxNorm` (region.c:1457, 1610,
    /// 1536), which all forward to that Frame.
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.data.frameset.current().get_axis(axis)
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    /// C has one `Simplify` slot, so a Region simplifies through its Mapping
    /// side and the caller keeps the result because it is still a Frame
    /// (`astIsAFrame`, `cmpframe.c:9024`).
    fn simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        simplify_frame_via_mapping(self)
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        self.data.frameset.current().map_from(previous)
    }

    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        self.data.frameset.current().convert_to(to, domains)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        if let Some(other_nr) = other.as_any().downcast_ref::<NullRegion>() {
            self.data
                .frameset
                .current()
                .equal(other_nr.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for NullRegion {
    // C's `Frame` overrides `astDoNotSimplify` to zero (frame.c:3511-3554): a
    // set `Ident` is a user label on a coordinate system, not a request to keep
    // a Mapping intact, so a named Frame is still simplified.
    fn do_not_simplify(&self) -> bool {
        false
    }

    fn nin(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn nout(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        false
    }

    fn invert(&mut self) {
        // A Region's forward and inverse transforms are identical (membership
        // is direction-independent), so inverting has no effect on behaviour.
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        super::data::region_transform(self, pts, forward)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        let npoint = result.npoint();
        let ncoord = result.ncoord();
        for i in 0..npoint {
            let src = result.point(i);
            let dst = out.point_mut(i);
            dst[..ncoord].copy_from_slice(src);
        }
        Ok(())
    }

    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        // Invoke the parent Region simplify first (nullregion.c:1376); it
        // simplifies the encapsulated FrameSet and any explicit uncertainty.
        // All further work is done on the returned copy.
        let parent = self.parent_region_simplify()?;
        let new_owned;
        let new: &NullRegion = match &parent {
            Some(p) => {
                new_owned = p
                    .as_ref()
                    .as_any()
                    .downcast_ref::<NullRegion>()
                    .expect("parent_region_simplify preserves the Region class")
                    .clone();
                &new_owned
            }
            None => self,
        };

        // Test the base->current Mapping of the parent-simplified Region
        // (nullregion.c:1379). A UnitMap means no geometric change is possible.
        let map = new
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let simpler = parent.is_some();

        let mut result: DynRegion = if map.as_any().is::<UnitMap>() {
            new.clone_region()
        } else {
            // Build a NullRegion on the parent-simplified current Frame,
            // carrying the parent's uncertainty (nullregion.c:1386-1389).
            let current_frame = new.data.frameset.current().snapshot();
            let frameset = FrameSet::new(current_frame);
            let mut data = RegionData::new(frameset, Vec::new());
            data.negated = new.data.negated;
            data.closed = new.data.closed;
            data.closed_set = new.data.closed_set;
            data.mesh_size = new.data.mesh_size;
            data.mesh_size_set = new.data.mesh_size_set;
            data.fill_factor = new.data.fill_factor;
            data.fill_factor_set = new.data.fill_factor_set;
            data.adaptive = new.data.adaptive;
            let mut region = NullRegion { data };
            // C reads the uncertainty with `astGetUnc( new, 0 )`, which yields
            // the *current*-Frame uncertainty and nothing at all when none was
            // ever assigned (nullregion.c:1388, region.c:3889-3893). The
            // constructor's SetUnc then maps it into the rebuilt NullRegion's
            // base Frame.
            if new.test_unc() {
                let unc = new.get_unc_frm(FrameRef::Current)?;
                region.set_uncertainty(unc);
            }
            Box::new(region)
        };

        // If any simplification occurred (FrameSet/uncertainty changed, or the
        // Mapping was not a UnitMap), overlay the original Region's attributes
        // and clear a fitted uncertainty if the original had none
        // (nullregion.c:1400). Otherwise keep the original.
        // C has no `simpler` flag here: it tests `result != this_mapping`
        // directly (nullregion.c:1400), which holds for the fresh NullRegion of
        // the non-UnitMap branch and for a parent simplify that changed
        // something. `None` is C's remaining case, where `result` is the
        // supplied pointer.
        let changed = simpler || !map.as_any().is::<UnitMap>();
        if changed {
            result.reg_overlay(self, true);
            Ok((result.clone_box(), true))
        } else {
            Ok((self.clone_box(), false))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<NullRegion>() else {
            return false;
        };
        self.data.negated == other.data.negated
            && self.data.closed == other.data.closed
            && Frame::equal(self.data.frameset.current(), other.data.frameset.current())
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        // Parallel merge only (series of two Regions is not a Region). The
        // direct pairwise path always has `self` as the lower-index (left)
        // component, so this NullRegion's axes go first (`intfirst = 1`). The
        // nominate loop reaches the lower-neighbour (`intfirst = 0`) case via
        // `Region::region_merge`, not through `try_merge`.
        if series || self_inv || other_inv {
            return None;
        }
        let other_reg: &dyn Region = super::as_region(other)?;
        Region::region_merge(self, other_reg, true)
    }

    fn map_merge(
        &self,
        ctx: &mut crate::mappings::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        crate::mappings::simplify::region_map_merge(self, ctx, where_, series)
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

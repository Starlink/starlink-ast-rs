//! NullRegion: a boundless region within a Frame.
//!
//! A NullRegion represents a region with no bounds. When not negated,
//! it contains no points. When negated, it represents the entire
//! coordinate system (all points are inside).

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::{AsAny, DynMapping, Mapping, UnitMap};

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
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(NullRegion { data })
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

    fn reg_pins(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        // NullRegion (un-negated) contains no points at all.
        Ok(vec![false; pts.npoint()])
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // No bounds — return BAD_VALUE for all axes.
        let naxes = self.data.frameset.base().naxes();
        Ok((vec![BAD_VALUE; naxes], vec![BAD_VALUE; naxes]))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        // NullRegion has no boundary, so the mesh is empty.
        Ok(Vec::new())
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        Err(AstError::new(ErrorCode::Regcn, "NullRegion has no centre"))
    }

    fn is_bounded_inner(&self) -> bool {
        false
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

    fn fill_factor(&self) -> f64 {
        self.data.fill_factor
    }

    fn set_fill_factor(&mut self, factor: f64) {
        self.data.fill_factor = factor;
    }

    fn mesh_size(&self) -> usize {
        self.data.mesh_size
    }

    fn set_mesh_size(&mut self, size: usize) {
        self.data.mesh_size = size;
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

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
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

    fn set_uncertainty(&mut self, unc: DynRegion) {
        self.data.uncertainty = Some(unc);
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn map_region(&self, _mapping: &dyn Mapping, frame: &dyn Frame) -> AstResult<DynRegion> {
        // NullRegion in the new frame — geometry is trivial (empty or universal).
        let frameset = FrameSet::new(frame.snapshot());
        let points = Vec::new();
        let mut data = RegionData::new(frameset, points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;

        Ok(Box::new(NullRegion { data }))
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

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
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
        // No-op for now — region inversion handled via negation.
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

    fn simplified(&self) -> AstResult<DynMapping> {
        let map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let simple_map = map.simplified()?;
        if simple_map.as_any().is::<UnitMap>() {
            return Ok(self.clone_box());
        }
        // NullRegion has no defining points; just construct in current frame.
        let current_frame = self.data.frameset.current().snapshot();
        let frameset = FrameSet::new(current_frame);
        let points = Vec::new();
        let mut data = RegionData::new(frameset, points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;
        Ok(Box::new(NullRegion { data }))
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
        // Parallel merge of two same-class NullRegions into a single
        // NullRegion over the CmpFrame of the two current frames. Mirrors C
        // `nullregion.c::MergeNullRegion` (571): both must share the same
        // Negated flag; the merged region carries the combined negation and
        // (when both have one) a Prism uncertainty.
        if series || self_inv || other_inv {
            return None;
        }
        let other = other.as_any().downcast_ref::<NullRegion>()?;
        if self.data.negated != other.data.negated {
            return None;
        }

        let f1 = self.data.frameset.current().snapshot();
        let f2 = other.data.frameset.current().snapshot();
        let cmp_frame = crate::frames::CmpFrame::new(f1, f2);
        let frameset = FrameSet::new(Box::new(cmp_frame) as DynFrame);

        // NullRegion has no defining points.
        let mut data = RegionData::new(frameset, Vec::new());
        data.negated = self.data.negated;
        data.closed = self.data.closed && other.data.closed;
        data.closed_set = self.data.closed_set || other.data.closed_set;
        data.adaptive = self.data.adaptive && other.data.adaptive;

        // Combine uncertainties into a Prism when both are present, mirroring
        // C `nullregion.c:686` (order: this then reg). C additionally
        // simplifies the Prism; storing it unsimplified is transform-
        // equivalent (a later simplify pass reduces it).
        if let (Some(u1), Some(u2)) = (&self.data.uncertainty, &other.data.uncertainty)
            && let Ok(prism) =
                super::PrismBuilder::from_dyn(u1.clone(), u2.clone()).and_then(|b| b.build())
        {
            data.uncertainty = Some(Box::new(prism) as DynRegion);
        }

        Some(Box::new(NullRegion { data }))
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

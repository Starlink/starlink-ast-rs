//! PointList region: a discrete collection of points in a Frame.
//!
//! A [`PointList`] represents a Region which is a collection of discrete
//! points within a coordinate frame. The region interior consists of
//! exactly those positions that coincide with one of the defining points
//! (within the positional uncertainty, if set).
//!
//! When used as a Mapping, a forward transform passes through points that
//! match any defining point, and sets exterior points to
//! [`BAD_VALUE`](crate::constants::BAD_VALUE). The inverse transform
//! always passes points unchanged.

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, UnitMap};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Overlap, Region};

/// A region consisting of a discrete set of points within a coordinate frame.
///
/// The region interior is defined by the set of points themselves: a test
/// point is inside if (and only if) it matches one of the defining points
/// within the positional uncertainty. Without an explicit uncertainty region,
/// matching uses the frame's `distance()` metric with a small relative
/// tolerance.
///
/// # Example
///
/// ```
/// use ast_core::frames::BasicFrameBuilder;
/// use ast_core::regions::{PointListBuilder, Region};
///
/// let frame = BasicFrameBuilder::new(2).build().unwrap();
/// let points = vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]];
/// let pl = PointListBuilder::new(frame, points).unwrap().build().unwrap();
///
/// assert!(pl.point_in_region(&[1.0, 2.0]).unwrap());
/// assert!(!pl.point_in_region(&[0.0, 0.0]).unwrap());
/// ```
#[derive(Debug, Clone)]
pub struct PointList {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing a [`PointList`].
pub struct PointListBuilder {
    frame: DynFrame,
    points: Vec<Vec<f64>>,
    uncertainty: Option<DynRegion>,
}

impl PointListBuilder {
    /// Create a builder for a PointList in the given frame.
    ///
    /// # Parameters
    ///
    /// - `frame`: The coordinate frame for the points.
    /// - `points`: A list of points, where each point is a `Vec<f64>` of
    ///   length equal to `frame.naxes()`. Must contain at least one point.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `points` is empty.
    /// - Any point has a length different from `frame.naxes()`.
    pub fn new(frame: impl Frame + 'static, points: Vec<Vec<f64>>) -> AstResult<Self> {
        let naxes = frame.naxes();

        if points.is_empty() {
            return Err(AstError::new(
                ErrorCode::Nptin,
                "PointList requires at least one point",
            ));
        }

        for (i, p) in points.iter().enumerate() {
            if p.len() != naxes {
                return Err(AstError::new(
                    ErrorCode::Naxin,
                    format!(
                        "PointList point {} has {} coordinates but frame has {} axes",
                        i,
                        p.len(),
                        naxes
                    ),
                ));
            }
        }

        Ok(PointListBuilder {
            frame: Box::new(frame),
            points,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the PointList.
    pub fn build(self) -> AstResult<PointList> {
        let frameset = FrameSet::new(self.frame);
        let mut data = RegionData::new(frameset, self.points);
        data.uncertainty = self.uncertainty;

        Ok(PointList { data })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl PointList {
    /// Mutate the current frame with region integrity enforcement.
    ///
    /// See [`Circle::with_integrity`](super::Circle) for the full
    /// description of the integrity mechanism.
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

    /// Return the number of points in this PointList.
    pub fn list_size(&self) -> usize {
        self.data.points.len()
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for PointList {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        // Faithful port of the uncertainty-driven PointList membership test
        // (pointlist.c:2504). A supplied point is inside the (un-negated)
        // PointList iff it coincides — to within the positional uncertainty —
        // with at least one of the defining points. C negates the base-Frame
        // uncertainty Region, recentres it at each defining point in turn, and
        // transforms the supplied points through it, accumulating a BAD mask
        // wherever a point falls inside a recentred uncertainty. The net raw
        // membership is therefore the OR, over all defining points, of "the
        // supplied point lies inside the uncertainty Region recentred at that
        // defining point". We compute that directly: negation is applied by
        // the `region_transform` caller, so here we return the un-negated
        // membership using the un-negated uncertainty.
        let npoint = pts.npoint();
        let naxes = self.data.frameset.base().naxes();

        // Base-Frame uncertainty Region (default: a tiny Box centred on the
        // PointList's bounding-box centre). Mirrors astGetUncFrm(this,
        // AST__BASE); its current Frame is the PointList's base Frame, so the
        // supplied base-Frame points can be transformed through it directly.
        let mut unc = self.get_unc_frm(FrameRef::Base)?;

        let mut result = vec![false; npoint];
        for def_point in &self.data.points {
            if def_point[..naxes].iter().any(|&v| is_bad(v)) {
                continue;
            }

            // Recentre the uncertainty Region at this defining point
            // (astRegCentre(unc, NULL, ptr_base, point, AST__BASE)).
            unc.reg_centre_set(&def_point[..naxes], FrameRef::Base)?;

            // Transform the supplied points through the (un-negated)
            // uncertainty: interior points pass through, exterior points
            // become BAD. A surviving (non-BAD) point coincides with this
            // defining point.
            let masked = unc.transform(pts, true)?;
            for (i, r) in result.iter_mut().enumerate() {
                if *r {
                    continue;
                }
                if point_has_bad(pts.point(i)) {
                    continue;
                }
                if !point_has_bad(masked.point(i)) {
                    *r = true;
                }
            }
        }
        Ok(result)
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        let base_frame = self.data.frameset.base();
        let naxes = base_frame.naxes();

        let mut lower = vec![f64::MAX; naxes];
        let mut upper = vec![f64::MIN; naxes];

        for point in &self.data.points {
            for (i, &val) in point.iter().enumerate().take(naxes) {
                if is_bad(val) {
                    continue;
                }
                if val < lower[i] {
                    lower[i] = val;
                }
                if val > upper[i] {
                    upper[i] = val;
                }
            }
        }

        Ok((lower, upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        // For a PointList, the "mesh" (boundary) is just the defining
        // points themselves — they ARE the region boundary.
        Ok(self.data.points.clone())
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        let naxes = self.data.frameset.base().naxes();
        let npoints = self.data.points.len();

        if npoints == 0 {
            return Ok(vec![0.0; naxes]);
        }

        // Compute the centroid (mean of all points).
        let mut centre = vec![0.0; naxes];
        let mut count = vec![0usize; naxes];

        for point in &self.data.points {
            for (i, &val) in point.iter().enumerate().take(naxes) {
                if !is_bad(val) {
                    centre[i] += val;
                    count[i] += 1;
                }
            }
        }

        for i in 0..naxes {
            if count[i] > 0 {
                centre[i] /= count[i] as f64;
            }
        }

        Ok(centre)
    }

    fn is_bounded_inner(&self) -> bool {
        // Discrete points are always bounded.
        true
    }

    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()> {
        let naxes = self.data.points[0].len();
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, forward)?;

        let mut new_points = Vec::with_capacity(self.data.points.len());
        for i in 0..self.data.points.len() {
            let p = transformed.point(i);
            new_points.push(p[..naxes].to_vec());
        }
        self.data.points = new_points;
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region trait
// ============================================================================

impl Region for PointList {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        if point_has_bad(point) {
            return Ok(false);
        }

        // Transform point from current frame to base frame.
        let naxes = self.data.frameset.base().naxes();
        let pts = PointSet::from_rows(std::iter::once(point));
        let base_pts = self.data.frameset.transform(&pts, false)?;
        let base_point = base_pts.point(0);

        if base_point[..naxes].iter().any(|&v| is_bad(v)) {
            return Ok(false);
        }

        // Test raw geometry.
        let inside = self.reg_base_inside_point(&base_point[..naxes])?;

        // Apply negation.
        if self.data.negated {
            Ok(!inside)
        } else {
            Ok(inside)
        }
    }

    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap> {
        super::overlap::compute_overlap(self, other)
    }

    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        // Faithful port of C PointList RegPins (pointlist.c:1868). The
        // supplied points (`pts`, in this PointList's base Frame) all lie on
        // the boundary iff mutual coverage holds in both directions, within
        // the joint positional uncertainty. C temporarily aliases the current
        // Frame to the base Frame so that its `astTransform` calls treat the
        // supplied base-Frame points as current-Frame points; here we test
        // base-Frame membership directly, which is the same computation.
        let base_frame = self.data.frameset.base();
        let naxes = base_frame.naxes();

        // Direction 1: every supplied point must lie within this (un-negated)
        // PointList — i.e. coincide with one of this PointList's defining
        // points, to within this PointList's own uncertainty. `reg_base_inside`
        // is the raw (un-negated) membership test, matching C's temporary
        // `astSetNegated(this, 0)`. This per-point flag is the boundary mask
        // (pointlist.c:1980-2007). The overall result additionally requires the
        // opposite-direction coverage check below.
        let inside = self.reg_base_inside(pts)?;
        if inside.iter().any(|&b| !b) {
            return Ok((false, inside));
        }

        // Direction 2: build a PointList from the supplied points, using the
        // supplied uncertainty `unc` both as the defining Frame and as the
        // positional uncertainty (C `astPointList(unc, pset, unc)`), then check
        // that every one of this PointList's defining points lies within it.
        // The supplied `unc` is stored verbatim as the new PointList's
        // uncertainty; its negation for the membership test happens inside
        // `reg_base_inside` (matching C's Transform, which negates the
        // uncertainty internally).
        let np = pts.npoint();
        let supplied: Vec<Vec<f64>> = (0..np).map(|i| pts.point(i)[..naxes].to_vec()).collect();

        let pl_frame: DynFrame = match unc {
            Some(u) => u.get_region_frame(),
            None => base_frame.snapshot(),
        };
        let frameset = FrameSet::new(pl_frame);
        let mut data = RegionData::new(frameset, supplied);
        data.uncertainty = unc.map(|u| u.clone_region());
        let pl = PointList { data };

        // Transform this PointList's own defining points through `pl`: any that
        // fall outside (BAD) mean this PointList is not fully covered by the
        // supplied points, so they do not all lie on the boundary.
        let def_pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let covered = pl.reg_base_inside(&def_pts)?;
        let result = covered.iter().all(|&b| b);
        // `inside` is all-true here (direction 1 passed); it is the mask.
        Ok((result, inside))
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

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        let centre = self.reg_centre()?;
        let base_frame = self.data.frameset.base();

        // Radius is the maximum distance from centroid to any defining point.
        let mut max_dist: f64 = 0.0;
        for point in &self.data.points {
            let dist = base_frame.distance(&centre, point)?;
            if !is_bad(dist) && dist > max_dist {
                max_dist = dist;
            }
        }

        Ok((centre, max_dist))
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        // Check for a cached mesh first.
        {
            let cached = self.data.cached_base_mesh.borrow();
            if let Some(mesh) = cached.as_ref() {
                return Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())));
            }
        }

        // Compute the mesh (just the defining points themselves).
        let mesh = self.reg_base_mesh()?;
        let result = PointSet::from_rows(mesh.iter().map(|p| p.as_slice()));

        // Cache it.
        *self.data.cached_base_mesh.borrow_mut() = Some(mesh);

        Ok(result)
    }

    fn get_region_grid(&self, npoints: usize) -> AstResult<Vec<Vec<f64>>> {
        self.reg_base_grid(npoints)
    }

    fn get_region_points(&self) -> AstResult<PointSet> {
        Ok(PointSet::from_rows(
            self.data.points.iter().map(|p| p.as_slice()),
        ))
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

    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        // Port of pointlist.c::RegBasePick (pointlist.c:1738): build a
        // PointList spanned by the selected base-Frame axes by slicing each
        // point's coordinates to those axes.
        let bfrm = self.data.frameset.base();
        let target: Vec<i32> = axes.iter().map(|&a| a as i32).collect();
        let frm = bfrm.pick_axes_snapshot(&target)?;

        let new_points: Vec<Vec<f64>> = self
            .data
            .points
            .iter()
            .map(|p| axes.iter().map(|&a| p[a]).collect())
            .collect();

        // Select the same axes from the base-Frame uncertainty (if any); drop
        // it if picking does not yield a Region (pointlist.c:1817-1826).
        let unc = match &self.data.uncertainty {
            Some(u) => match super::pick_axes_region(u.as_ref(), axes)? {
                super::PickedAxes::Region(r) => Some(r),
                super::PickedAxes::Frame(_) => None,
            },
            None => None,
        };

        let frameset = FrameSet::new(frm);
        let mut data = RegionData::new(frameset, new_points);
        data.uncertainty = unc;
        Ok(Some(Box::new(PointList { data })))
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    fn region_merge(&self, other_reg: &dyn Region, intfirst: bool) -> Option<DynMapping> {
        // Faithful port of `pointlist.c::MergePointList` (1205). A PointList
        // only merges with another PointList (other classes handle merging a
        // PointList with themselves). `intfirst` selects the axis order: when
        // true this PointList's axes go first, when false `other`'s go first.
        //
        // Guard (pointlist.c:1289): same Closed, and either both or neither
        // have a defined uncertainty; both must have equal Negated and contain
        // exactly one point.
        let other = other_reg.as_any().downcast_ref::<PointList>()?;
        if self.is_closed() != other.is_closed()
            || self.data.uncertainty.is_some() != other.data.uncertainty.is_some()
            || self.data.negated != other.data.negated
            || self.data.points.len() != 1
            || other.data.points.len() != 1
        {
            return None;
        }

        // Concatenate the two single points' axis values in the requested
        // order.
        let mut point = Vec::with_capacity(self.naxes() + other.naxes());
        if intfirst {
            point.extend_from_slice(&self.data.points[0]);
            point.extend_from_slice(&other.data.points[0]);
        } else {
            point.extend_from_slice(&other.data.points[0]);
            point.extend_from_slice(&self.data.points[0]);
        }

        // Combine the two current Frames into a CmpFrame in the requested
        // order.
        let f_this = self.data.frameset.current().snapshot();
        let f_reg = other_reg.get_region_frame();
        let cmp_frame = if intfirst {
            crate::frames::CmpFrame::new(f_this, f_reg)
        } else {
            crate::frames::CmpFrame::new(f_reg, f_this)
        };
        let frameset = FrameSet::new(Box::new(cmp_frame) as DynFrame);

        let unc = super::merge_util::combine_uncertainties(
            self.data.uncertainty.clone(),
            other_reg.get_uncertainty(),
            intfirst,
        );

        let mut data = RegionData::new(frameset, vec![point]);
        data.negated = self.data.negated;
        data.closed = self.data.closed && other.data.closed;
        data.closed_set = self.data.closed_set || other.data.closed_set;
        data.adaptive = self.data.adaptive && other.data.adaptive;
        data.uncertainty = unc;

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
        super::merge_util::apply_merge_scaling(&mut data, &scaling);

        Some(Box::new(PointList { data }))
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait
// ============================================================================

impl Frame for PointList {
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
        if let Some(other_pl) = other.as_any().downcast_ref::<PointList>() {
            self.data
                .frameset
                .current()
                .equal(other_pl.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait
// ============================================================================

impl Mapping for PointList {
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
        // Inversion for a region swaps forward/inverse semantics.
        // For now, this is a no-op stub.
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
        // Invoke the parent Region simplify first (pointlist.c:2228); it
        // simplifies the encapsulated FrameSet and any explicit uncertainty.
        // All further work is done on the returned copy.
        let parent = self.parent_region_simplify()?;
        let new_owned;
        let this: &PointList = match &parent {
            Some(p) => {
                new_owned = p
                    .as_ref()
                    .as_any()
                    .downcast_ref::<PointList>()
                    .expect("parent_region_simplify preserves the Region class")
                    .clone();
                &new_owned
            }
            None => self,
        };
        let simpler = parent.is_some();

        let map = this
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        if map.as_any().is::<UnitMap>() {
            return if simpler {
                let mut result = this.clone_region();
                result.reg_overlay(self, true);
                Ok(result.clone_box())
            } else {
                Ok(self.clone_box())
            };
        }
        // Transform defining points to current frame.
        let pts = PointSet::from_rows(this.data.points.iter().map(|p| p.as_slice()));
        let transformed = map.transform(&pts, true)?;
        let current_frame = this.data.frameset.current().snapshot();
        let naxes = current_frame.naxes();
        let new_points: Vec<Vec<f64>> = (0..this.data.points.len())
            .map(|i| transformed.point(i)[..naxes].to_vec())
            .collect();
        let frameset = FrameSet::new(current_frame);
        let mut data = RegionData::new(frameset, new_points);
        data.uncertainty = this.data.uncertainty.clone();
        let mut result: DynRegion = Box::new(PointList { data });
        result.reg_overlay(self, true);
        Ok(result.clone_box())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<PointList>() else {
            return false;
        };
        self.data.negated == other.data.negated
            && self.data.closed == other.data.closed
            && self.data.points == other.data.points
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
        // component, so this PointList's axes go first (`intfirst = 1`). The
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

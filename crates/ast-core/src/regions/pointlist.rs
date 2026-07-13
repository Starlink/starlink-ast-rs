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
use crate::mappings::{AsAny, DynMapping, Mapping, UnitMap};

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

/// Default tolerance for point matching when no uncertainty is set.
/// This is a small relative tolerance used with `Frame::distance()`.
const DEFAULT_TOLERANCE: f64 = 1.0e-10;

impl RegionGeometry for PointList {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_pins(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let base_frame = self.data.frameset.base();
        let naxes = base_frame.naxes();
        let closed = self.data.closed;

        // Compute a tolerance. Without an uncertainty region, use a small
        // value derived from the extent of the defining points.
        let tolerance = self.compute_tolerance()?;

        let mut result = vec![false; npoint];
        for (r, test_point) in result.iter_mut().zip((0..npoint).map(|i| pts.point(i))) {
            if point_has_bad(test_point) {
                continue;
            }

            // Check against each defining point.
            for def_point in &self.data.points {
                if def_point[..naxes].iter().any(|&v| is_bad(v)) {
                    continue;
                }

                let dist = base_frame.distance(&test_point[..naxes], &def_point[..naxes])?;
                if is_bad(dist) {
                    continue;
                }

                let matches = if closed {
                    dist <= tolerance
                } else {
                    dist < tolerance
                };

                if matches {
                    *r = true;
                    break;
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

impl PointList {
    /// Compute the matching tolerance for `reg_pins`.
    ///
    /// Without an uncertainty region, uses a small fraction of the
    /// characteristic scale of the defining points. If all points are
    /// identical or there is only one point, falls back to `DEFAULT_TOLERANCE`.
    fn compute_tolerance(&self) -> AstResult<f64> {
        let naxes = self.data.frameset.base().naxes();
        let npoints = self.data.points.len();

        if npoints <= 1 {
            return Ok(DEFAULT_TOLERANCE);
        }

        // Use the bounding box diagonal as characteristic scale.
        let mut max_extent: f64 = 0.0;
        for ax in 0..naxes {
            let mut lo = f64::MAX;
            let mut hi = f64::MIN;
            for pt in &self.data.points {
                let v = pt[ax];
                if !is_bad(v) {
                    if v < lo {
                        lo = v;
                    }
                    if v > hi {
                        hi = v;
                    }
                }
            }
            if hi > lo {
                let extent = hi - lo;
                if extent > max_extent {
                    max_extent = extent;
                }
            }
        }

        if max_extent > 0.0 {
            Ok(max_extent * DEFAULT_TOLERANCE)
        } else {
            Ok(DEFAULT_TOLERANCE)
        }
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
        let inside = self.reg_base_pick(&base_point[..naxes])?;

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

    fn set_uncertainty(&mut self, unc: DynRegion) {
        self.data.uncertainty = Some(unc);
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn map_region(&self, mapping: &dyn Mapping, frame: &dyn Frame) -> AstResult<DynRegion> {
        // Transform all defining points through the mapping.
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, true)?;

        let naxes = frame.naxes();
        let mut new_points = Vec::with_capacity(self.data.points.len());
        for i in 0..self.data.points.len() {
            new_points.push(transformed.point(i)[..naxes].to_vec());
        }

        let frameset = FrameSet::new(frame.snapshot());
        let mut data = RegionData::new(frameset, new_points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;

        Ok(Box::new(PointList { data }))
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
        let map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let simple_map = map.simplified()?;
        if simple_map.as_any().is::<UnitMap>() {
            return Ok(self.clone_box());
        }
        // Transform defining points to current frame.
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = simple_map.transform(&pts, true)?;
        let current_frame = self.data.frameset.current().snapshot();
        let naxes = current_frame.naxes();
        let new_points: Vec<Vec<f64>> = (0..self.data.points.len())
            .map(|i| transformed.point(i)[..naxes].to_vec())
            .collect();
        let frameset = FrameSet::new(current_frame);
        let mut data = RegionData::new(frameset, new_points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;
        Ok(Box::new(PointList { data }))
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

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

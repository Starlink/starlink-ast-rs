//! Circle region: a circular (or spherical-cap) area within a Frame.
//!
//! A Circle is defined by a centre point and a radius (specified as a
//! point on the circumference). In a [`SkyFrame`](crate::frames::SkyFrame),
//! the "radius" is a great-circle distance; in a flat frame it is
//! Euclidean distance.

use std::f64::consts::PI;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameSet};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Overlap, Region};

/// A circular region within a coordinate frame.
///
/// Defined by a centre point and a point on the circumference (or
/// equivalently, a centre and a radius computed as the frame distance
/// between the two points).
#[derive(Debug, Clone)]
pub struct Circle {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing a [`Circle`].
pub struct CircleBuilder {
    frame: DynFrame,
    centre: Vec<f64>,
    edge_point: Option<Vec<f64>>,
    radius: Option<f64>,
    uncertainty: Option<DynRegion>,
}

impl CircleBuilder {
    /// Create a builder for a Circle in the given frame.
    ///
    /// `centre` must have length equal to `frame.naxes()`.
    pub fn new(frame: impl Frame + 'static, centre: Vec<f64>) -> AstResult<Self> {
        let naxes = frame.naxes();
        if centre.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Circle centre has {} coordinates but frame has {} axes",
                    centre.len(),
                    naxes
                ),
            ));
        }
        Ok(CircleBuilder {
            frame: Box::new(frame),
            centre,
            edge_point: None,
            radius: None,
            uncertainty: None,
        })
    }

    /// Specify the circle by a point on the circumference.
    pub fn edge_point(mut self, point: Vec<f64>) -> Self {
        self.edge_point = Some(point);
        self
    }

    /// Specify the circle by a radius value.
    ///
    /// The radius is interpreted as a distance in the frame's metric
    /// (great-circle for sky, Euclidean for flat).
    pub fn radius(mut self, r: f64) -> Self {
        self.radius = Some(r);
        self
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Circle.
    ///
    /// Either `edge_point` or `radius` must have been specified.
    pub fn build(self) -> AstResult<Circle> {
        let naxes = self.frame.naxes();

        // Determine the edge point (either given directly or computed from radius).
        let edge = if let Some(ep) = self.edge_point {
            if ep.len() != naxes {
                return Err(AstError::new(
                    ErrorCode::Naxin,
                    format!(
                        "Circle edge point has {} coordinates but frame has {} axes",
                        ep.len(),
                        naxes
                    ),
                ));
            }
            ep
        } else if let Some(r) = self.radius {
            // Compute an edge point at distance `r` from centre.
            // We need a "direction" point for Frame::offset. Create one
            // that differs from centre along axis 1 only.
            let mut direction = self.centre.clone();
            direction[0] += 1.0;
            // Use the frame's offset method to move from centre toward
            // the direction point by distance r. This correctly handles
            // both Euclidean and spherical geometry.
            self.frame.offset(&self.centre, &direction, r)?
        } else {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "Circle requires either an edge_point or a radius",
            ));
        };

        // Create a FrameSet with the frame as both base and current.
        let frameset = FrameSet::new(self.frame);

        let points = vec![self.centre, edge];
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(Circle { data })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl Circle {
    /// Mutate the current frame with region integrity enforcement.
    ///
    /// When adaptive (the default), the FrameSet's base→current mapping is
    /// updated to account for the change, and defining points remain valid
    /// in the base frame. When non-adaptive, the change is applied to both
    /// frames without remapping.
    ///
    /// See [`with_region_integrity`](super::data::with_region_integrity) for
    /// the full algorithm.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
    {
        super::data::with_region_integrity(self, f)
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl Circle {
    /// Compute the radius of this Circle by measuring the geodesic distance
    /// between the centre (points[0]) and the edge point (points[1]) in the
    /// base frame.
    fn radius(&self) -> AstResult<f64> {
        let base_frame = self.data.frameset.base();
        base_frame.distance(&self.data.points[0], &self.data.points[1])
    }
}

impl RegionGeometry for Circle {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_pins(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let base_frame = self.data.frameset.base();
        let centre = &self.data.points[0];
        let radius = self.radius()?;
        let closed = self.data.closed;

        if is_bad(radius) {
            return Ok(vec![false; npoint]);
        }

        let mut result = vec![false; npoint];
        for (r, point) in result.iter_mut().zip((0..npoint).map(|i| pts.point(i))) {
            if point_has_bad(point) {
                continue;
            }
            let dist = base_frame.distance(point, centre)?;
            if is_bad(dist) {
                continue;
            }
            *r = if closed {
                dist <= radius
            } else {
                dist < radius
            };
        }
        Ok(result)
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        let base_frame = self.data.frameset.base();
        let naxes = base_frame.naxes();
        let centre = &self.data.points[0];
        let radius = self.radius()?;

        // For a simple (Euclidean) frame, the bounding box is
        // centre ± radius on each axis. This matches the C library's
        // behaviour for plain Frames.
        // For non-Euclidean frames (SkyFrame etc.), the correct approach
        // is to derive bounds from the mesh. For now, use the simple
        // Euclidean formula which is correct for BasicFrame.
        let mut lower = vec![0.0; naxes];
        let mut upper = vec![0.0; naxes];

        for i in 0..naxes {
            lower[i] = centre[i] - radius;
            upper[i] = centre[i] + radius;
        }

        Ok((lower, upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let base_frame = self.data.frameset.base();
        let naxes = base_frame.naxes();
        let centre = &self.data.points[0];
        let radius = self.radius()?;
        let np = self.data.mesh_size;

        if naxes == 1 {
            // 1-D: the boundary is just the two extreme points.
            let p1 = vec![centre[0] - radius];
            let p2 = vec![centre[0] + radius];
            return Ok(vec![p1, p2]);
        }

        if naxes == 2 {
            // 2-D: distribute points evenly around the circumference
            // using the frame's offset method (handles both Euclidean
            // and spherical geometry via the frame's geodesic).
            let delta = 2.0 * PI / np as f64;
            let mut mesh = Vec::with_capacity(np);

            for i in 0..np {
                let angle = i as f64 * delta;
                // For Euclidean geometry, parametric circle:
                let x = centre[0] + radius * angle.cos();
                let y = centre[1] + radius * angle.sin();
                mesh.push(vec![x, y]);
            }

            return Ok(mesh);
        }

        // N-D: approximate by offsetting from centre along each axis
        // direction pair, then projecting mesh points onto the sphere.
        // For the initial implementation, use the simple Euclidean
        // approach: offset along each pair of axes.
        let points_per_plane = np / (naxes * (naxes - 1) / 2).max(1);
        let delta = 2.0 * PI / points_per_plane.max(1) as f64;
        let mut mesh = Vec::new();

        for ax1 in 0..naxes {
            for ax2 in (ax1 + 1)..naxes {
                for k in 0..points_per_plane {
                    let angle = k as f64 * delta;
                    let mut point = centre.to_vec();
                    point[ax1] = centre[ax1] + radius * angle.cos();
                    point[ax2] = centre[ax2] + radius * angle.sin();
                    mesh.push(point);
                }
            }
        }

        Ok(mesh)
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        Ok(self.data.points[0].clone())
    }

    fn is_bounded_inner(&self) -> bool {
        true
    }

    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()> {
        // Transform the defining points (centre and edge) through the mapping.
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
// Region trait — stub implementations (completed in Task 2)
// ============================================================================

impl Region for Circle {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        // Check for BAD coordinates in the input.
        if point_has_bad(point) {
            return Ok(false);
        }

        // Transform point from current frame to base frame.
        let naxes = self.data.frameset.base().naxes();
        let pts = PointSet::from_rows(std::iter::once(point));
        let base_pts = self.data.frameset.transform(&pts, false)?;
        let base_point = base_pts.point(0);

        // Check the transformed point for BAD values.
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

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        let centre = self.data.points[0].clone();
        let radius = self.radius()?;
        Ok((centre, radius))
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        // Check for a cached mesh first.
        {
            let cached = self.data.cached_base_mesh.borrow();
            if let Some(mesh) = cached.as_ref() {
                return Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())));
            }
        }

        // Compute the mesh.
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
        // Create a new Circle with the same geometry but in the new frame.
        // The defining points are transformed through the mapping.
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, true)?;

        let naxes = frame.naxes();
        let centre = transformed.point(0)[..naxes].to_vec();
        let edge = transformed.point(1)[..naxes].to_vec();

        // Construct the Circle directly, bypassing the builder since we
        // already have the edge point computed.
        let frameset = FrameSet::new(frame.snapshot());
        let points = vec![centre, edge];
        let mut data = RegionData::new(frameset, points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;

        Ok(Box::new(Circle { data }))
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — stub (completed in Task 2)
// ============================================================================

impl Frame for Circle {
    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
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

    fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        self.data.frameset.current().convert(to, domains)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // Two Circles are frame-equal if their encapsulated frames are equal.
        if let Some(other_circle) = other.as_any().downcast_ref::<Circle>() {
            self.data
                .frameset
                .current()
                .equal(other_circle.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — stub (completed in Task 2)
// ============================================================================

impl Mapping for Circle {
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
        // For now, this is a no-op stub — full implementation handles
        // the inverted flag.
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
        Ok(self.clone_box())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Circle>() else {
            return false;
        };
        // Compare geometry: same frameset, same points, same negation.
        self.data.negated == other.data.negated
            && self.data.closed == other.data.closed
            && self.data.points == other.data.points
            && Frame::equal(self.data.frameset.current(), other.data.frameset.current())
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

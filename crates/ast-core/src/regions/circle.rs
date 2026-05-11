//! Circle region: a circular (or spherical-cap) area within a Frame.
//!
//! A Circle is defined by a centre point and a radius (specified as a
//! point on the circumference). In a [`SkyFrame`](crate::frames::SkyFrame),
//! the "radius" is a great-circle distance; in a flat frame it is
//! Euclidean distance.

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameSet};
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
            // Compute an edge point at distance `r` from centre along axis 1.
            // For now, use a simple offset along the first axis.
            // Full implementation in Task 2 will use frame.offset().
            let mut ep = self.centre.clone();
            ep[0] += r;
            ep
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
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for Circle {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_base_pick(&self, _point: &[f64]) -> AstResult<bool> {
        todo!("Circle::reg_base_pick — full implementation in Task 2")
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        todo!("Circle::reg_base_box — full implementation in Task 4")
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        todo!("Circle::reg_base_mesh — full implementation in Task 4")
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        Ok(self.data.points[0].clone())
    }

    fn is_bounded_inner(&self) -> bool {
        true
    }

    fn remap_defining_points(&mut self, _mapping: &dyn Mapping, _forward: bool) -> AstResult<()> {
        todo!("Circle::remap_defining_points — full implementation in Task 2")
    }
}

// ============================================================================
// Region trait — stub implementations (completed in Task 2)
// ============================================================================

impl Region for Circle {
    fn point_in_region(&self, _point: &[f64]) -> AstResult<bool> {
        todo!("Circle::point_in_region — full implementation in Task 2")
    }

    fn overlap(&self, _other: &dyn Region) -> AstResult<Overlap> {
        todo!("Circle::overlap — full implementation in Task 12")
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
        todo!("Circle::get_region_bounds — full implementation in Task 4")
    }

    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        todo!("Circle::get_region_disc — full implementation in Task 4")
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        todo!("Circle::get_region_mesh — full implementation in Task 4")
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

    fn map_region(&self, _mapping: &dyn Mapping, _frame: &dyn Frame) -> AstResult<DynRegion> {
        todo!("Circle::map_region — full implementation in Task 2")
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

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        todo!("Circle::map_from — full implementation in Task 3")
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

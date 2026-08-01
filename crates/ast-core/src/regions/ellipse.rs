//! Ellipse region: a rotated elliptical area within a 2-D Frame.
//!
//! An [`Ellipse`] is defined by a centre point, semi-major and semi-minor
//! axis lengths, and a position angle (measured from axis 1 toward axis 2).
//! The ellipse is restricted to 2-dimensional frames.

use std::f64::consts::PI;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, UnitMap};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Overlap, Region};

/// A rotated elliptical region within a 2-D coordinate frame.
///
/// Defined by a centre point, semi-major axis length `a`, semi-minor
/// axis length `b`, and a position angle (radians, measured from axis 1
/// toward axis 2). The frame must have exactly 2 axes.
///
/// The defining points stored internally are:
/// - `points[0]`: centre
/// - `points[1]`: end of semi-major axis
/// - `points[2]`: a point on the boundary (end of semi-minor axis)
#[derive(Debug, Clone)]
pub struct Ellipse {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing an [`Ellipse`].
pub struct EllipseBuilder {
    frame: DynFrame,
    centre: Vec<f64>,
    semi_axes: [f64; 2],
    angle: f64,
    uncertainty: Option<DynRegion>,
}

impl EllipseBuilder {
    /// Create a builder for an Ellipse in the given frame.
    ///
    /// # Parameters
    ///
    /// - `frame`: A 2-D frame (naxes must be 2).
    /// - `centre`: Coordinates of the ellipse centre (length 2).
    /// - `semi_axes`: `[a, b]` — semi-major and semi-minor axis lengths.
    /// - `angle`: Position angle in radians, measured from axis 1 toward
    ///   axis 2. An angle of 0 aligns the semi-major axis with axis 1.
    pub fn new(
        frame: impl Frame + 'static,
        centre: Vec<f64>,
        semi_axes: [f64; 2],
        angle: f64,
    ) -> AstResult<Self> {
        let naxes = frame.naxes();
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Ellipse requires a 2-dimensional frame, but got {} axes",
                    naxes
                ),
            ));
        }
        if centre.len() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Ellipse centre has {} coordinates but frame has 2 axes",
                    centre.len()
                ),
            ));
        }
        if semi_axes[0] <= 0.0 || semi_axes[1] <= 0.0 {
            return Err(AstError::new(
                ErrorCode::Badin,
                "Ellipse semi-axis lengths must be positive",
            ));
        }
        Ok(EllipseBuilder {
            frame: Box::new(frame),
            centre,
            semi_axes,
            angle,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Ellipse.
    pub fn build(self) -> AstResult<Ellipse> {
        let [a, b] = self.semi_axes;
        let angle = self.angle;
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        // Compute the end of the semi-major axis.
        let p1 = vec![self.centre[0] + a * cos_a, self.centre[1] + a * sin_a];

        // Compute a point on the boundary at the end of the semi-minor axis.
        // The semi-minor axis is perpendicular to the semi-major axis
        // (rotated by +90 degrees).
        let p2 = vec![self.centre[0] - b * sin_a, self.centre[1] + b * cos_a];

        let frameset = FrameSet::new(self.frame);
        let points = vec![self.centre, p1, p2];
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(Ellipse { data })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl Ellipse {
    /// Mutate the current frame with region integrity enforcement.
    ///
    /// When adaptive (the default), the FrameSet's base→current mapping is
    /// updated to account for the change, and defining points remain valid
    /// in the base frame. When non-adaptive, the change is applied to both
    /// frames without remapping.
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

    /// Compute the geometric parameters (semi-axes and angle) from the
    /// defining points.
    ///
    /// Returns `(a, b, angle)` where `a` is the semi-major length, `b`
    /// is the semi-minor length, and `angle` is the position angle in
    /// radians from axis 1 toward axis 2.
    fn ellipse_params(&self) -> AstResult<(f64, f64, f64)> {
        let centre = &self.data.points[0];
        let p1 = &self.data.points[1];

        // Semi-major axis: distance from centre to point1.
        let dx = p1[0] - centre[0];
        let dy = p1[1] - centre[1];
        let a = (dx * dx + dy * dy).sqrt();

        if a <= 0.0 {
            return Err(AstError::new(
                ErrorCode::Badin,
                "Ellipse semi-major axis length is zero",
            ));
        }

        // Position angle from axis 1 toward axis 2.
        let angle = dy.atan2(dx);

        // Semi-minor axis: distance from centre to point2.
        let p2 = &self.data.points[2];
        let dx2 = p2[0] - centre[0];
        let dy2 = p2[1] - centre[1];
        let b = (dx2 * dx2 + dy2 * dy2).sqrt();

        Ok((a, b, angle))
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for Ellipse {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let (a, b, angle) = self.ellipse_params()?;
        let centre = &self.data.points[0];
        let closed = self.data.closed;

        // Pre-compute trigonometric and reciprocal values for efficiency.
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let inv_a2 = 1.0 / (a * a);
        let inv_b2 = 1.0 / (b * b);

        let mut result = vec![false; npoint];

        for (r, point) in result.iter_mut().zip((0..npoint).map(|i| pts.point(i))) {
            if point_has_bad(&point[..2]) {
                continue;
            }

            // Translate relative to centre.
            let dx = point[0] - centre[0];
            let dy = point[1] - centre[1];

            // Rotate by negative angle to align with ellipse axes.
            let u = dx * cos_a + dy * sin_a;
            let v = -dx * sin_a + dy * cos_a;

            // Compute normalised distance.
            let d = u * u * inv_a2 + v * v * inv_b2;

            *r = if closed { d <= 1.0 } else { d < 1.0 };
        }

        Ok(result)
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        let (a, b, angle) = self.ellipse_params()?;
        let centre = &self.data.points[0];
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        // For a general rotated ellipse, the extent on each axis is:
        // x_extent = sqrt((a*cos(angle))^2 + (b*sin(angle))^2)
        // y_extent = sqrt((a*sin(angle))^2 + (b*cos(angle))^2)
        let x_extent = ((a * cos_a).powi(2) + (b * sin_a).powi(2)).sqrt();
        let y_extent = ((a * sin_a).powi(2) + (b * cos_a).powi(2)).sqrt();

        let lower = vec![centre[0] - x_extent, centre[1] - y_extent];
        let upper = vec![centre[0] + x_extent, centre[1] + y_extent];

        Ok((lower, upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let (a, b, angle) = self.ellipse_params()?;
        let centre = &self.data.points[0];
        let np = self.data.mesh_size;
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        let delta = 2.0 * PI / np as f64;
        let mut mesh = Vec::with_capacity(np);

        for i in 0..np {
            let t = i as f64 * delta;
            let x = centre[0] + a * t.cos() * cos_a - b * t.sin() * sin_a;
            let y = centre[1] + a * t.cos() * sin_a + b * t.sin() * cos_a;
            mesh.push(vec![x, y]);
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
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, forward)?;

        let mut new_points = Vec::with_capacity(self.data.points.len());
        for i in 0..self.data.points.len() {
            let p = transformed.point(i);
            new_points.push(p[..2].to_vec());
        }
        self.data.points = new_points;
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region trait
// ============================================================================

impl Region for Ellipse {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        if point_has_bad(point) {
            return Ok(false);
        }

        // Transform point from current frame to base frame.
        let pts = PointSet::from_rows(std::iter::once(point));
        let base_pts = self.data.frameset.transform(&pts, false)?;
        let base_point = base_pts.point(0);

        if base_point[..2].iter().any(|&v| is_bad(v)) {
            return Ok(false);
        }

        let inside = self.reg_base_inside_point(&base_point[..2])?;

        if self.data.negated {
            Ok(!inside)
        } else {
            Ok(inside)
        }
    }

    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap> {
        super::overlap::compute_overlap(self, other)
    }

    fn reg_pins(&self, pts: &PointSet, unc: Option<&dyn Region>) -> AstResult<bool> {
        // Faithful port of ellipse.c::RegPins (1381). The border half-width is
        // half the total geodesic length of the diagonals of the two
        // uncertainty bounding boxes: `drad = 0.5*(l1 + l2)`. A point is on
        // the boundary iff it lies inside an Ellipse with semi-axes enlarged
        // by `0.5*drad` and outside one with semi-axes shrunk by `0.5*drad`
        // (floored at `1e-6*drad`).
        let bfrm = self.data.frameset.base();
        if pts.ncoord() != 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "reg_pins(Ellipse): PointSet has {} coordinates per point, expected 2",
                    pts.ncoord()
                ),
            ));
        }

        // `tunc` is this Ellipse's own uncertainty in the base Frame; `l1` is
        // the geodesic length of its bounding-box diagonal. Re-centring is
        // omitted (bound width is centre-invariant; the reachable default
        // uncertainty is already centred — tracked as the RCENT / R-UNC gaps).
        let tunc = self.get_unc_frm(FrameRef::Base)?;
        let (lbnd_tunc, ubnd_tunc) = tunc.get_region_bounds()?;
        let l1 = bfrm.distance(&lbnd_tunc, &ubnd_tunc)?;

        let l2 = if let Some(unc) = unc {
            let (lbnd_unc, ubnd_unc) = unc.get_region_bounds()?;
            bfrm.distance(&lbnd_unc, &ubnd_unc)?
        } else {
            0.0
        };

        let drad = 0.5 * (l1 + l2);
        let (a, b, angle) = self.ellipse_params()?;
        let centre = &self.data.points[0];

        // Reconstruct the two defining boundary points from the centre, the
        // (adjusted) semi-axes, and the unchanged angle — as `build()` does.
        let make_ellipse = |sa: f64, sb: f64, negated: bool| -> Ellipse {
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            let p1 = vec![centre[0] + sa * cos_a, centre[1] + sa * sin_a];
            let p2 = vec![centre[0] - sb * sin_a, centre[1] + sb * cos_a];
            let frameset = FrameSet::new(bfrm.snapshot());
            let mut data = RegionData::new(frameset, vec![centre.clone(), p1, p2]);
            data.negated = negated;
            Ellipse { data }
        };

        let large_ellipse = make_ellipse(a + 0.5 * drad, b + 0.5 * drad, false);

        let lim = 1.0E-6 * drad;
        let mut small_a = a - 0.5 * drad;
        let mut small_b = b - 0.5 * drad;
        if small_a < lim {
            small_a = lim;
        }
        if small_b < lim {
            small_b = lim;
        }
        // Negate the smaller region (ellipse.c:1550).
        let small_ellipse = make_ellipse(small_a, small_b, true);

        // On the boundary iff inside both the large Ellipse and the negated
        // small Ellipse: chain the two forward transforms and require no BAD.
        let ps1 = super::data::region_transform(&large_ellipse, pts, true)?;
        let ps2 = super::data::region_transform(&small_ellipse, &ps1, true)?;
        for j in 0..ps2.npoint() {
            if ps2.point(j).iter().any(|&v| is_bad(v)) {
                return Ok(false);
            }
        }
        Ok(true)
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
        let centre = self.data.points[0].clone();
        let (a, b, _angle) = self.ellipse_params()?;
        // Bounding disc radius is the semi-major axis (the largest extent).
        let radius = a.max(b);
        Ok((centre, radius))
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        {
            let cached = self.data.cached_base_mesh.borrow();
            if let Some(mesh) = cached.as_ref() {
                return Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())));
            }
        }

        let mesh = self.reg_base_mesh()?;
        let result = PointSet::from_rows(mesh.iter().map(|p| p.as_slice()));

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

    fn set_uncertainty(&mut self, unc: DynRegion) {
        self.data.uncertainty = Some(unc);
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for Ellipse {
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
        if let Some(other_ellipse) = other.as_any().downcast_ref::<Ellipse>() {
            self.data
                .frameset
                .current()
                .equal(other_ellipse.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for Ellipse {
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
        // No-op for now.
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
        if simple_map.as_ref().as_any().is::<UnitMap>() {
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
        Ok(Box::new(Ellipse { data }))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Ellipse>() else {
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

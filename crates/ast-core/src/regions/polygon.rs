//! Polygon region: a closed area defined by ordered vertices.
//!
//! A [`Polygon`] represents a closed polygonal area within a 2-D
//! coordinate frame. It is defined by an ordered list of vertices.
//! Point membership is determined by the crossing-number (ray-casting)
//! algorithm: a ray cast from the test point in the +x direction, and
//! the number of polygon edge crossings determines inside/outside.
//! Odd crossings = inside, even = outside.
//!
//! The polygon is restricted to 2-dimensional frames (naxes must be 2).

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{AsAny, DynMapping, Mapping, UnitMap};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Overlap, Region};

/// Compute the 2D convex hull using Andrew's monotone chain algorithm.
///
/// Input points are sorted in-place. Returns the hull vertices in
/// counter-clockwise order. The input must have at least 3 points.
fn convex_hull_2d(points: &mut [[f64; 2]]) -> Vec<[f64; 2]> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }

    // Sort by x, then by y.
    points.sort_by(|a, b| {
        a[0].partial_cmp(&b[0])
            .unwrap()
            .then(a[1].partial_cmp(&b[1]).unwrap())
    });

    // Remove duplicates (points are sorted, so consecutive dupes are adjacent).
    let mut unique: Vec<[f64; 2]> = Vec::with_capacity(n);
    for &p in points.iter() {
        if unique
            .last()
            .is_none_or(|last| last[0] != p[0] || last[1] != p[1])
        {
            unique.push(p);
        }
    }
    let points = &unique;
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }

    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(2 * n);

    // Build lower hull.
    for p in points.iter() {
        while hull.len() >= 2 && cross(&hull[hull.len() - 2], &hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(*p);
    }

    // Build upper hull.
    let lower_len = hull.len() + 1;
    for p in points.iter().rev() {
        while hull.len() >= lower_len
            && cross(&hull[hull.len() - 2], &hull[hull.len() - 1], p) <= 0.0
        {
            hull.pop();
        }
        hull.push(*p);
    }

    // Remove the last point (same as first).
    hull.pop();
    hull
}

/// Cross product of vectors OA and OB (2D).
fn cross(o: &[f64; 2], a: &[f64; 2], b: &[f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// A polygonal region within a 2-D coordinate frame.
///
/// Defined by an ordered list of N vertices (N >= 3). The polygon is
/// implicitly closed (the last vertex connects back to the first).
/// Point membership uses the crossing-number (ray-casting) algorithm.
#[derive(Debug, Clone)]
pub struct Polygon {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing a [`Polygon`].
pub struct PolygonBuilder {
    frame: DynFrame,
    vertices: Vec<Vec<f64>>,
    uncertainty: Option<DynRegion>,
}

impl PolygonBuilder {
    /// Create a builder for a Polygon in the given frame.
    ///
    /// # Parameters
    ///
    /// - `frame`: A 2-D frame (naxes must be 2).
    /// - `vertices`: Ordered list of vertex positions. Each vertex is a
    ///   `Vec<f64>` of length 2. Must have at least 3 vertices.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The frame does not have exactly 2 axes.
    /// - Fewer than 3 vertices are supplied.
    /// - Any vertex does not have length equal to `frame.naxes()`.
    pub fn new(frame: impl Frame + 'static, vertices: Vec<Vec<f64>>) -> AstResult<Self> {
        let naxes = frame.naxes();
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!("Polygon requires a 2-D frame but got {} axes", naxes),
            ));
        }
        if vertices.len() < 3 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Polygon requires at least 3 vertices but got {}",
                    vertices.len()
                ),
            ));
        }
        for (i, v) in vertices.iter().enumerate() {
            if v.len() != naxes {
                return Err(AstError::new(
                    ErrorCode::Naxin,
                    format!(
                        "Polygon vertex {} has {} coordinates but frame has {} axes",
                        i,
                        v.len(),
                        naxes
                    ),
                ));
            }
        }
        Ok(PolygonBuilder {
            frame: Box::new(frame),
            vertices,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Polygon.
    pub fn build(self) -> AstResult<Polygon> {
        let frameset = FrameSet::new(self.frame);

        // Defining points are the vertices themselves.
        let points = self.vertices;
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(Polygon { data })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl Polygon {
    /// Mutate the current frame with region integrity enforcement.
    ///
    /// When adaptive (the default), the FrameSet's base-to-current mapping is
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

    /// Compute the convex hull of pixels matching a condition in a 2D array.
    ///
    /// Scans the 2D `data` array (stored row-major, dimensions `nx` × `ny`)
    /// for pixels whose value equals `value` (when `equal` is true) or does
    /// not equal `value` (when `equal` is false). The grid coordinates of
    /// matching pixels are computed using `lbnd` (the grid coordinates of
    /// pixel `[0, 0]`). Returns a Polygon representing the convex hull.
    ///
    /// This matches the C library's `astConvexF`.
    pub fn convex_hull_f32(
        value: f32,
        equal: bool,
        data: &[f32],
        nx: usize,
        ny: usize,
        lbnd: [i64; 2],
    ) -> AstResult<Self> {
        use crate::frames::BasicFrameBuilder;

        if data.len() < nx * ny {
            return Err(AstError::new(
                ErrorCode::Dimin,
                format!(
                    "data array has {} elements but nx*ny = {}",
                    data.len(),
                    nx * ny
                ),
            ));
        }

        // Collect grid coordinates of matching pixels.
        let mut points: Vec<[f64; 2]> = Vec::new();
        for row in 0..ny {
            for col in 0..nx {
                let pixel = data[col + row * nx];
                let matches = if equal {
                    pixel == value
                } else {
                    pixel != value
                };
                if matches {
                    let x = (lbnd[0] + col as i64) as f64;
                    let y = (lbnd[1] + row as i64) as f64;
                    points.push([x, y]);
                }
            }
        }

        if points.len() < 3 {
            return Err(AstError::new(
                ErrorCode::Ngdin,
                format!(
                    "convex hull requires at least 3 matching pixels, found {}",
                    points.len()
                ),
            ));
        }

        // Compute convex hull using Andrew's monotone chain algorithm.
        let hull = convex_hull_2d(&mut points);

        // Build a Polygon from the hull vertices.
        let frame = BasicFrameBuilder::new(2).build()?;
        let vertices: Vec<Vec<f64>> = hull.iter().map(|p| vec![p[0], p[1]]).collect();
        PolygonBuilder::new(frame, vertices)?.build()
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for Polygon {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_pins(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let nv = self.data.points.len();
        let closed = self.data.closed;

        // Pre-extract vertex x/y arrays for cache-friendly access.
        let vx: Vec<f64> = self.data.points.iter().map(|p| p[0]).collect();
        let vy: Vec<f64> = self.data.points.iter().map(|p| p[1]).collect();

        let mut result = vec![false; npoint];

        for (idx, inside) in result.iter_mut().enumerate() {
            let point = pts.point(idx);

            // BAD input coordinates mean the point is outside.
            if point_has_bad(&point[..2]) {
                continue;
            }

            let px = point[0];
            let py = point[1];

            // Crossing-number (ray-casting) algorithm.
            // Cast a ray from (px, py) in the +x direction.
            // Count how many polygon edges the ray crosses.
            let mut ncross = 0i32;
            let mut on_edge = false;

            for i in 0..nv {
                let j = (i + 1) % nv;

                let y0 = vy[i];
                let y1 = vy[j];
                let x0 = vx[i];
                let x1 = vx[j];

                // Check if test point's y is between the edge's y-endpoints.
                let (y_min, y_max) = if y0 < y1 { (y0, y1) } else { (y1, y0) };

                // Skip edge if point's y is completely outside the edge's y range.
                if py < y_min || py > y_max {
                    continue;
                }

                // Always check if point lies on this edge (needed for both
                // closed and open boundary handling).
                // Check if the point is at a vertex.
                if (px == x0 && py == y0) || (px == x1 && py == y1) {
                    on_edge = true;
                    break;
                }

                // Check if point lies on the edge (collinear and within bounds).
                if py >= y_min && py <= y_max {
                    if y0 == y1 {
                        // Horizontal edge: check if px is between x0 and x1.
                        let (xmin, xmax) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
                        if px >= xmin && px <= xmax {
                            on_edge = true;
                            break;
                        }
                        continue;
                    }

                    // Compute x-intersection at this y.
                    let x_intersect = x0 + (py - y0) * (x1 - x0) / (y1 - y0);
                    let tol = f64::EPSILON * (px.abs() + x_intersect.abs() + 1.0);
                    if (px - x_intersect).abs() <= tol {
                        on_edge = true;
                        break;
                    }
                }

                // Standard crossing-number test using half-open interval on y.
                // A crossing is counted if:
                // - The ray at y=py intersects the edge
                // - The intersection x is > px (ray goes in +x direction)
                // Use the convention: count if y is in [y_low, y_high) where
                // y_low is the lower y vertex. This avoids double-counting
                // at shared vertices.
                let upward = y1 > y0;
                let crosses = if upward {
                    // Edge goes upward: count if py in [y0, y1)
                    py >= y0 && py < y1
                } else if y0 > y1 {
                    // Edge goes downward: count if py in [y1, y0)
                    py >= y1 && py < y0
                } else {
                    // Horizontal edge: no crossing
                    false
                };

                if crosses {
                    // Compute x-intersection of edge with horizontal line y=py.
                    let x_intersect = x0 + (py - y0) * (x1 - x0) / (y1 - y0);
                    if x_intersect > px {
                        ncross += 1;
                    }
                }
            }

            // Determine if point is inside.
            if on_edge {
                // Point is on the boundary: inside if closed, outside if open.
                *inside = closed;
            } else {
                // Odd crossings = inside, even = outside.
                *inside = (ncross % 2) != 0;
            }
        }

        Ok(result)
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        let nv = self.data.points.len();

        let mut lower = vec![f64::MAX; 2];
        let mut upper = vec![f64::MIN; 2];

        for v in &self.data.points {
            for ax in 0..2 {
                if !is_bad(v[ax]) {
                    if v[ax] < lower[ax] {
                        lower[ax] = v[ax];
                    }
                    if v[ax] > upper[ax] {
                        upper[ax] = v[ax];
                    }
                }
            }
        }

        // If no valid vertices, return BAD bounds.
        if nv == 0 {
            lower = vec![crate::constants::BAD_VALUE; 2];
            upper = vec![crate::constants::BAD_VALUE; 2];
        }

        Ok((lower, upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let nv = self.data.points.len();
        let np = self.data.mesh_size;

        // Compute edge lengths.
        let mut edge_lengths = Vec::with_capacity(nv);
        let mut total_len = 0.0;

        for i in 0..nv {
            let j = (i + 1) % nv;
            let dx = self.data.points[j][0] - self.data.points[i][0];
            let dy = self.data.points[j][1] - self.data.points[i][1];
            let len = (dx * dx + dy * dy).sqrt();
            edge_lengths.push(len);
            total_len += len;
        }

        if total_len == 0.0 {
            // Degenerate polygon: all vertices coincide.
            return Ok(vec![self.data.points[0].clone()]);
        }

        // Distribute mesh_size points proportional to edge length.
        let mut np_edge = Vec::with_capacity(nv);
        let mut total_np = 0;
        let ppd = np as f64 / total_len;

        for elen in &edge_lengths {
            let n = ((elen * ppd).round() as usize).max(1);
            np_edge.push(n);
            total_np += n;
        }

        let mut mesh = Vec::with_capacity(total_np);

        for (i, &n) in np_edge.iter().enumerate() {
            let j = (i + 1) % nv;
            let dx = (self.data.points[j][0] - self.data.points[i][0]) / n as f64;
            let dy = (self.data.points[j][1] - self.data.points[i][1]) / n as f64;

            for k in 0..n {
                let x = self.data.points[i][0] + dx * k as f64;
                let y = self.data.points[i][1] + dy * k as f64;
                mesh.push(vec![x, y]);
            }
        }

        Ok(mesh)
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        // Centroid = average of vertices.
        let nv = self.data.points.len();
        let mut cx = 0.0;
        let mut cy = 0.0;
        for v in &self.data.points {
            cx += v[0];
            cy += v[1];
        }
        Ok(vec![cx / nv as f64, cy / nv as f64])
    }

    fn is_bounded_inner(&self) -> bool {
        // A polygon is always bounded.
        true
    }

    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()> {
        // Transform the defining points (vertices) through the mapping.
        let naxes = 2;
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

impl Region for Polygon {
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
        let centre = self.reg_centre()?;
        // Disc radius = maximum distance from centroid to any vertex.
        let mut max_dist = 0.0_f64;
        for v in &self.data.points {
            let dx = v[0] - centre[0];
            let dy = v[1] - centre[1];
            let d = (dx * dx + dy * dy).sqrt();
            if d > max_dist {
                max_dist = d;
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
        // Transform the defining points (vertices) through the mapping.
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, true)?;

        let naxes = frame.naxes();
        let mut new_vertices = Vec::with_capacity(self.data.points.len());
        for i in 0..self.data.points.len() {
            let p = transformed.point(i);
            new_vertices.push(p[..naxes].to_vec());
        }

        let frameset = FrameSet::new(frame.snapshot());
        let mut data = RegionData::new(frameset, new_vertices);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;

        Ok(Box::new(Polygon { data }))
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for Polygon {
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
        if let Some(other_poly) = other.as_any().downcast_ref::<Polygon>() {
            self.data
                .frameset
                .current()
                .equal(other_poly.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for Polygon {
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
        if simple_map.as_any().is::<UnitMap>() {
            return Ok(self.clone_box());
        }
        // Transform defining points (vertices) to current frame.
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
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;
        Ok(Box::new(Polygon { data }))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Polygon>() else {
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

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
use crate::mappings::{DynMapping, Mapping, UnitMap};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Overlap, Region};
use crate::constants::BAD_VALUE;
use crate::frames::{LineDef, SkyFrame};

/// The width of the polygon perpendicular to edge `i`, and the point half
/// way across.
///
/// Faithful port of C `Polywidth` (polygon.c:4047). Builds a line
/// perpendicular to edge `i` through its mid-point, extending towards the
/// polygon interior, and returns the distance to the nearest other edge
/// (`AST__BAD` if none is hit within its length) together with the position
/// half that distance along the perpendicular. When no width is found the
/// returned point is the far end of the perpendicular probe line (a
/// representative "inside" point for a clockwise/negated polygon).
fn polywidth(
    frm: &dyn Frame,
    edges: &[LineDef],
    i: usize,
    nv: usize,
) -> AstResult<(f64, [f64; 2])> {
    let mut result = BAD_VALUE;

    // Move to the mid point of edge `i`: the start of the perpendicular line.
    let l1 = 0.5 * edges[i].length;
    let start = frm.line_offset(&edges[i], l1, 0.0)?;

    // Move away at right angles. Start at 10*l1; for spherical Frames this may
    // wrap around, so shrink until the end is actually the requested distance
    // from the start.
    let mut l2 = 10.0 * l1;
    let mut end;
    loop {
        end = frm.line_offset(&edges[i], l1, l2)?;
        let d = frm.distance(&start, &end)?;
        if !is_bad(d) && (d - l2).abs() < 1.0e-6 * l2 {
            break;
        }
        l2 *= 0.5;
    }

    // Describe the perpendicular line, marked infinite so any crossing counts.
    let mut line = frm
        .line_def(&start, &end)?
        .ok_or_else(|| AstError::new(ErrorCode::Inter, "polywidth: bad perpendicular line"))?;
    line.infinite = true;

    // Nearest crossing with any other edge.
    for (j, edge) in edges.iter().enumerate().take(nv) {
        if j == i {
            continue;
        }
        let (crossed, _cross, d) = frm.line_crossing(&line, edge)?;
        if crossed && !is_bad(d) && (d < result || is_bad(result)) {
            result = d;
        }
    }

    let cen = if !is_bad(result) {
        let half = frm.offset(&start, &end, 0.5 * result)?;
        [half[0], half[1]]
    } else {
        end
    };

    Ok((result, cen))
}

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

    /// Create a builder for a Polygon from an owned [`DynFrame`].
    ///
    /// Equivalent to [`PolygonBuilder::new`] but accepts a boxed trait object,
    /// for use when the frame is only available as a `DynFrame` (e.g. a
    /// current-Frame snapshot during simplification).
    pub(crate) fn from_dyn(frame: DynFrame, vertices: Vec<Vec<f64>>) -> AstResult<Self> {
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
            frame,
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

        // Mirror C astInitPolygon (polygon.c:7160): reverse the vertex order if
        // necessary so that the unnegated Polygon represents the bounded
        // interior, matching the parent Region convention.
        let mut polygon = Polygon { data };
        polygon.ensure_inside()?;
        Ok(polygon)
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

    /// Calculate the cached intermediate values used by the membership and
    /// bounded tests.
    ///
    /// Faithful port of C `Cache` (polygon.c:575). Builds an edge description
    /// for each edge (edge `i` runs from vertex `i-1` to vertex `i`, edge 0
    /// wrapping from the last vertex). For each non-degenerate edge it measures
    /// the polygon width perpendicular to that edge via [`polywidth`] and keeps
    /// the interior representative point for the widest edge. If any positive
    /// width is found the vertices are stored in anti-clockwise order
    /// (`acw = true`) and the retained point lies inside the polygon; if none
    /// is found the vertices are in clockwise order (`acw = false`) and the
    /// retained point lies outside (the probes ran off to the infinite exterior
    /// of a polygonal hole). Returns `(edges, inside_point, acw)`.
    fn cache(&self) -> AstResult<(Vec<LineDef>, [f64; 2], bool)> {
        let bfrm = self.data.frameset.base();
        let ptr = &self.data.points;
        let nv = ptr.len();

        let mut edges: Vec<LineDef> = Vec::with_capacity(nv);
        let mut start = [ptr[nv - 1][0], ptr[nv - 1][1]];
        for vertex in ptr.iter() {
            let end = [vertex[0], vertex[1]];
            let edge = bfrm
                .line_def(&start, &end)?
                .ok_or_else(|| AstError::new(ErrorCode::Inter, "cache: bad polygon edge"))?;
            edges.push(edge);
            start = end;
        }

        // Find a point well inside the polygon: the mid-width point of the edge
        // with the greatest perpendicular width. `polcen` tracks the latest
        // probe centre so that, if no positive width is found (a clockwise
        // polygon), `inside` falls back to a point outside the polygonal hole.
        let mut maxwid = -1.0;
        let mut inside = [BAD_VALUE, BAD_VALUE];
        let mut polcen = [BAD_VALUE, BAD_VALUE];
        for i in 0..nv {
            if edges[i].length > 0.0 {
                let (polwid, cen) = polywidth(bfrm, &edges, i, nv)?;
                polcen = cen;
                if !is_bad(polwid) && polwid > maxwid {
                    maxwid = polwid;
                    inside = cen;
                }
            }
        }

        if maxwid < 0.0 {
            inside = polcen;
        }

        Ok((edges, inside, maxwid >= 0.0))
    }

    /// Whether the base-Frame vertices are stored in anti-clockwise order.
    fn cache_acw(&self) -> AstResult<bool> {
        Ok(self.cache()?.2)
    }

    /// The solid angle (steradians) subtended by a Polygon defined in a
    /// SkyFrame.
    ///
    /// Faithful port of C `SkyArea` (polygon.c:5469): sum the interior angle at
    /// each vertex (folded into `0..2*PI`) and subtract `(n-2)*PI`.
    fn sky_area(&self) -> AstResult<f64> {
        let bfrm = self.data.frameset.base();
        let ptr = &self.data.points;
        let nvert = ptr.len();
        let mut result = 0.0;
        for ivert in 0..nvert {
            let b = [ptr[ivert][0], ptr[ivert][1]];
            let a = if ivert < nvert - 1 {
                [ptr[ivert + 1][0], ptr[ivert + 1][1]]
            } else {
                [ptr[0][0], ptr[0][1]]
            };
            let c = if ivert > 0 {
                [ptr[ivert - 1][0], ptr[ivert - 1][1]]
            } else {
                [ptr[nvert - 1][0], ptr[nvert - 1][1]]
            };
            let mut angle = bfrm.angle(&a, &b, &c)?;
            if angle < 0.0 {
                angle += 2.0 * std::f64::consts::PI;
            }
            result += angle;
        }
        result -= (nvert as f64 - 2.0) * std::f64::consts::PI;
        Ok(result)
    }

    /// Whether the (possibly negated) Polygon is bounded.
    ///
    /// Faithful port of C `GetBounded` (polygon.c:2749): SkyFrame Polygons are
    /// always bounded (a finite region has a finite negation on the sphere).
    /// Otherwise the result depends on the vertex order (`acw`) and the
    /// negation flag: anti-clockwise ⇒ bounded iff not negated; clockwise ⇒
    /// bounded iff negated.
    fn bounded_impl(&self) -> AstResult<bool> {
        let is_sky = self.data.frameset.base().as_any().is::<SkyFrame>();
        if is_sky {
            return Ok(true);
        }
        let acw = self.cache_acw()?;
        let neg = self.data.negated;
        Ok(if acw { !neg } else { neg })
    }

    /// Ensure the unnegated Polygon represents the inside of the polygon.
    ///
    /// Faithful port of C `EnsureInside` (polygon.c:1768). Reversing the vertex
    /// order is equivalent to negating the Polygon, but the parent Region class
    /// assumes an unnegated closed region is bounded. So if the unnegated
    /// Polygon would be unbounded, reverse the vertex order and toggle the
    /// negation flag to compensate, leaving the represented region unchanged
    /// while restoring the bounded-when-unnegated convention.
    ///
    /// For a SkyFrame Polygon both lobes are bounded, so "bounded" is taken to
    /// mean the smaller lobe: area < 2π steradians.
    fn ensure_inside(&mut self) -> AstResult<()> {
        let is_sky = self.data.frameset.base().as_any().is::<SkyFrame>();
        let bounded = if is_sky {
            self.sky_area()? < 2.0 * std::f64::consts::PI
        } else {
            self.bounded_impl()?
        };

        let negated = self.data.negated;
        if (bounded && negated) || (!bounded && !negated) {
            self.data.points.reverse();
            self.data.negated = !negated;
            self.data.invalidate_cache();
        }
        Ok(())
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

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        // Faithful port of C Polygon Transform (polygon.c:6235). The membership
        // test is winding-relative: a line is drawn from a cached interior
        // point to the test point, and the number of edges crossed determines
        // whether the two lie on the same side of the boundary. This mirrors C
        // exactly (rather than a winding-absolute ray-cast to infinity) so that
        // reversing the vertex order in `ensure_inside` is a true no-op on the
        // represented region, as it is in C. The caller applies negation, so
        // this returns the un-negated membership (C's `neg == 0` branch).
        let npoint = pts.npoint();
        let closed = self.data.closed;
        let bfrm = self.data.frameset.base();

        let (edges, in_point, _acw) = self.cache()?;
        let nv = edges.len();

        // Boundary position of a test point relative to the polygon.
        #[derive(PartialEq)]
        enum Pos {
            Unknown,
            In,
            Out,
            On,
        }

        let mut result = vec![false; npoint];

        for (idx, inside) in result.iter_mut().enumerate() {
            let point = pts.point(idx);

            // BAD input coordinates mean the point is outside.
            if point_has_bad(&point[..2]) {
                continue;
            }

            let p = [point[0], point[1]];

            // Define the line from the interior point to the test point.
            let a = bfrm
                .line_def(&in_point, &p)?
                .ok_or_else(|| AstError::new(ErrorCode::Inter, "reg_base_inside: bad test line"))?;

            // Count crossings of this line with the polygon edges.
            let mut ncross = 0i32;
            let mut pos = Pos::Unknown;

            for edge in edges.iter().take(nv) {
                if bfrm.line_contains(edge, &p)? {
                    pos = Pos::On;
                    break;
                } else if bfrm.line_crossing(edge, &a)?.0 {
                    ncross += 1;
                }
            }

            if pos == Pos::Unknown {
                pos = if ncross % 2 == 0 { Pos::In } else { Pos::Out };
            }

            *inside = match pos {
                Pos::In => true,
                Pos::Out => false,
                _ => closed,
            };
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
        // Mirrors C GetBounded (polygon.c:2749): SkyFrame Polygons are always
        // bounded; otherwise it depends on vertex order and negation. If the
        // geometric probe fails (bad edges), fall back to treating the region
        // as bounded — the historical default for a well-formed Polygon.
        self.bounded_impl().unwrap_or(true)
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
        // Faithful port of polygon.c::RegPins (4562). The margin width `wid` is
        // half the total geodesic length of the diagonals of the two
        // uncertainty bounding boxes: `wid = 0.5*(l1 + l2)`. Each supplied
        // point is tested against every polygon edge by resolving it into
        // components parallel and perpendicular to the edge; a point counts as
        // on the boundary iff, for at least one edge, its parallel component is
        // in `(-wid, edge_len+wid)` and its perpendicular component is in
        // `(-wid, wid)`. A point on no edge fails the whole test.
        let bfrm = self.data.frameset.base();
        if pts.ncoord() != 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "reg_pins(Polygon): PointSet has {} coordinates per point, expected 2",
                    pts.ncoord()
                ),
            ));
        }

        // `tunc` is this Polygon's own uncertainty in the base Frame; `l1` is
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

        let wid = 0.5 * (l1 + l2);

        let vertices = &self.data.points;
        let nv = vertices.len();
        let np = pts.npoint();

        // `on_edge[ip]` becomes true once point `ip` is found to lie within the
        // margin of some edge (mirrors C setting the point BAD in pset1).
        let mut on_edge = vec![false; np];

        // Edge `i` runs from vertex `i-1` to vertex `i`; edge 0 runs from the
        // last vertex to vertex 0.
        let mut start = vertices[nv - 1].clone();
        for end in vertices.iter() {
            let edge_len = bfrm.distance(&start, end)?;
            let resolved = bfrm.resolve_points(&start, end, pts)?;
            let parmax = edge_len + wid;
            for (ip, on) in on_edge.iter_mut().enumerate() {
                let comp = resolved.point(ip);
                let par = comp[0];
                let prp = comp[1];
                if !is_bad(par)
                    && !is_bad(prp)
                    && par > -wid
                    && par < parmax
                    && prp > -wid
                    && prp < wid
                {
                    *on = true;
                }
            }
            start = end.clone();
        }

        // On the boundary iff every supplied point lies on some edge. The
        // per-point `on_edge` flags are the boundary mask.
        let result = on_edge.iter().all(|&b| b);
        Ok((result, on_edge))
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

impl Frame for Polygon {
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
        if simple_map.as_ref().as_any().is::<UnitMap>() {
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
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;
        // Remapping the vertices into the current Frame can change their
        // winding order, so restore the bounded-when-unnegated convention as
        // C astLoadPolygon (polygon.c:7325) does after loading vertices.
        let mut polygon = Polygon { data };
        polygon.ensure_inside()?;
        Ok(Box::new(polygon))
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

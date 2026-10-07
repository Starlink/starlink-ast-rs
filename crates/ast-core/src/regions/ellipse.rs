//! Ellipse region: a rotated elliptical area within a 2-D Frame.
//!
//! An [`Ellipse`] is defined by a centre point, semi-major and semi-minor
//! axis lengths, and a position angle (measured from axis 1 toward axis 2).
//! The ellipse is restricted to 2-dimensional frames.

use std::f64::consts::PI;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Overlap, Region};

/// Offset of pi/2 from the primary to the secondary ellipse axis
/// (C `AST__DPIBY2`).
const DPIBY2: f64 = PI / 2.0;

/// Find the best fitting Ellipse through a given mesh of points.
///
/// Faithful port of C `astBestEllipse` (ellipse.c:191). Ellipses are always
/// 2-dimensional. The `mesh` points and the supplied `centre` are in the Frame
/// represented by `unc` (the current Frame of the Region being simplified).
///
/// The method (as the C source notes, not very accurate and assuming the mesh
/// covers the whole ellipse uniformly): find the mesh point furthest from the
/// centre — the end of the primary axis — via the radial distance and position
/// angle of every point; fit a quadratic through the (radius, angle) samples at
/// that point and its two neighbours to locate the turning point; resolve every
/// mesh point into components parallel and perpendicular to the primary axis and
/// derive the mean secondary-axis length weighted by the perpendicular
/// component; then build the Ellipse from the centre, the primary-axis end, and
/// a point at the mean secondary-axis length.
///
/// Returns `None` if no suitable primary axis or secondary-axis mean can be
/// found. The returned Ellipse inherits `unc`'s Frame and carries `unc` as its
/// uncertainty.
pub(crate) fn best_ellipse(
    mesh: &PointSet,
    centre: &[f64],
    unc: &dyn Region,
) -> AstResult<Option<Ellipse>> {
    let np = mesh.npoint();
    let frm = unc.get_region_frame();

    let mut dist = vec![BAD_VALUE; np];
    let mut ang = vec![BAD_VALUE; np];

    // Find the first mesh point which is at a non-zero distance from the
    // centre.
    let mut ip = 0usize;
    while ip < np {
        let row = mesh.point(ip);
        let p = [row[0], row[1]];
        dist[ip] = frm.distance(centre, &p)?;
        if dist[ip] != BAD_VALUE && dist[ip] != 0.0 {
            break;
        } else {
            ang[ip] = BAD_VALUE;
            dist[ip] = BAD_VALUE;
        }
        ip += 1;
    }
    if ip >= np {
        return Ok(None);
    }

    // Find a point which is this distance away from the centre along the second
    // axis. This point defines zero angle when calling astAngle below.
    let first_row = mesh.point(ip);
    let mut p = [first_row[0], first_row[1]];
    let (aa, _) = frm.offset2(centre, 0.0, dist[ip])?;
    ang[ip] = frm.angle(&aa, centre, &p)?;

    // Get the distance from the centre to each of the remaining mesh points,
    // and the orientation of the radial line to each. Track the index of the
    // point with the largest radial distance.
    let mut maxat = ip;
    let mut r2 = dist[maxat];
    ip += 1;
    while ip < np {
        let row = mesh.point(ip);
        p = [row[0], row[1]];
        dist[ip] = frm.distance(centre, &p)?;
        ang[ip] = frm.angle(&aa, centre, &p)?;
        if dist[ip] != BAD_VALUE && dist[ip] > r2 {
            r2 = dist[ip];
            maxat = ip;
        }
        ip += 1;
    }

    // Find the higher-index neighbouring point, wrapping back to the start of
    // the list when the end is reached. Note the radius and position angle
    // (relative to the max-radius angle a0).
    let t2 = 0.0_f64;
    let a0 = ang[maxat];
    let mut r3 = BAD_VALUE;
    let mut t3 = BAD_VALUE;
    for j in (maxat + 1)..np {
        if dist[j] != BAD_VALUE {
            r3 = dist[j];
            t3 = crate::pal::drange(ang[j] - a0);
            break;
        }
    }
    if r3 == BAD_VALUE {
        for j in 0..maxat {
            if dist[j] != BAD_VALUE {
                r3 = dist[j];
                t3 = crate::pal::drange(ang[j] - a0);
                break;
            }
        }
    }

    // Find the lower-index neighbouring point, wrapping back to the end of the
    // list when the start is reached.
    let mut r1 = BAD_VALUE;
    let mut t1 = BAD_VALUE;
    for j in (0..maxat).rev() {
        if dist[j] != BAD_VALUE {
            r1 = dist[j];
            t1 = crate::pal::drange(ang[j] - a0);
            break;
        }
    }
    if r1 == BAD_VALUE {
        for j in ((maxat + 1)..np).rev() {
            if dist[j] != BAD_VALUE {
                r1 = dist[j];
                t1 = crate::pal::drange(ang[j] - a0);
                break;
            }
        }
    }

    // Fit a quadratic through the three pairs of (radius, angle) values. The
    // centre point (r2, t2) is the point which is furthest from the centre; the
    // other two are the neighbouring points found above.
    let a = r2 - r1;
    let b = t2 - t1;
    let c = t2 * t2 - t1 * t1;
    let d = r3 - r2;
    let e = t3 - t2;
    let f = t3 * t3 - t2 * t2;

    let den = c * e - b * f;
    if den == 0.0 {
        return Ok(None);
    }

    // The co-efficients of the interpolating polynomial.
    let c1 = (d * c - a * f) / den;
    let c2 = (a * e - d * b) / den;
    let c0 = r1 - c1 * t1 - c2 * t1 * t1;

    // Find the largest radius (the turning point of the quadratic), and the
    // angle at which it occurs.
    let (mx, at) = if c2 < 0.0 {
        ((4.0 * c0 * c2 - c1 * c1) / (4.0 * c2), a0 - c1 / (2.0 * c2))
    } else {
        (r2, a0 - t2)
    };

    // This point is the end of the ellipse primary axis. Find its (x,y) coords,
    // and store in "pa".
    let (pa, _) = frm.offset2(centre, at, mx)?;

    // Resolve all the supplied points into components parallel and
    // perpendicular to the line joining the centre and "pa".
    let ps2 = frm.resolve_points(centre, &pa, mesh)?;

    // For each mesh point, work out the length of the secondary axis which
    // would result if we used that point to define the ellipse. Find the mean
    // of these lengths, weighted by the length of the y component to reduce the
    // influence of poor conditioning at very low y.
    let mut smn = 0.0_f64;
    let mut sw = 0.0_f64;
    for j in 0..np {
        let row = ps2.point(j);
        let px = row[0];
        let py = row[1];
        if px != BAD_VALUE && py != BAD_VALUE {
            let den = mx * mx - px * px;
            if den > 0.0 {
                smn += (mx * py * py).abs() / den.sqrt();
                sw += py.abs();
            }
        }
    }

    if sw <= 0.0 {
        return Ok(None);
    }
    let mn = smn / sw;

    // Find the coords at the end of the mean secondary axis, and store in "pb".
    let (pb, _) = frm.offset2(centre, at + DPIBY2, mn)?;

    // Create the Ellipse to return, storing the three defining points directly
    // (astEllipse form 0).
    match Ellipse::from_defining_points(frm, centre.to_vec(), pa, pb, Some(unc.clone_region())) {
        Ok(ellipse) => Ok(Some(ellipse)),
        Err(_) => Ok(None),
    }
}

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

    /// Create a builder for an Ellipse from a boxed (dynamic) frame.
    ///
    /// The parameters are those of [`EllipseBuilder::new`].
    pub fn from_dyn(
        frame: DynFrame,
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
            frame,
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
        let data = RegionData::new(frameset, points);
        let mut region = Ellipse { data };

        // astSetUnc, as C's constructor calls it (region.c:13372).
        if let Some(unc) = self.uncertainty {
            region.set_uncertainty(unc);
        }

        Ok(region)
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl Ellipse {
    /// Construct an Ellipse from three defining points in the given frame.
    ///
    /// Mirrors C `astEllipse` with `form = 0` (ellipse.c:2381): the region is
    /// defined by the centre, the coordinates at the end of the primary axis
    /// (`point1`), and the coordinates of any other point on the boundary
    /// (`point2`). The three points are stored directly as the defining
    /// points, exactly as C stores them; the geometric parameters are derived
    /// on demand. Returns an error if the points do not determine an ellipse
    /// (zero-length primary axis or non-positive secondary axis).
    pub(crate) fn from_defining_points(
        frame: DynFrame,
        centre: Vec<f64>,
        point1: Vec<f64>,
        point2: Vec<f64>,
        uncertainty: Option<DynRegion>,
    ) -> AstResult<Self> {
        if frame.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "Ellipse requires a 2-dimensional frame",
            ));
        }
        let frameset = FrameSet::new(frame);
        let points = vec![centre, point1, point2];
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = uncertainty;
        let ellipse = Ellipse { data };
        // Validate that the supplied points determine an ellipse (a > 0 and a
        // finite, positive secondary axis), matching the CalcPars check in
        // astInitEllipse (ellipse.c:654).
        let (a, b, _) = ellipse.ellipse_params()?;
        if a <= 0.0 || is_bad(a) || b <= 0.0 || is_bad(b) {
            return Err(AstError::new(
                ErrorCode::Badin,
                "The supplied points do not determine an ellipse",
            ));
        }
        Ok(ellipse)
    }

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
        let closed = self.data.closed_for_base_inside();

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
        // C computes nothing here: it forces the base mesh to exist and reads
        // the bounds RegBaseMesh accumulated while generating it
        // (ellipse.c:988-998). `get_region_mesh` is the cached mesh, so the
        // points below are the same points in the same order.
        let mesh = self.get_region_mesh()?;
        let centre = &self.data.points[0];
        let frm = self.data.frameset.base();

        let mut lbx = f64::MAX;
        let mut ubx = -f64::MAX;
        let mut lby = f64::MAX;
        let mut uby = -f64::MAX;

        for ip in 0..mesh.npoint() {
            let p2 = mesh.point(ip);
            if is_bad(p2[0]) || is_bad(p2[1]) {
                continue;
            }

            // The box is accumulated as axis offsets from the centre, via
            // ax_distance, so that a cyclic axis wraps correctly
            // (ellipse.c:1143-1157). Each axis is measured by its own axis's
            // rule, which matters when the two axes normalise differently. C's
            // `else if` is mirrored: it stops one point extending both ends of
            // an axis.
            let dist = frm.ax_distance(1, centre[0], p2[0])?;
            if dist < lbx {
                lbx = dist;
            } else if dist > ubx {
                ubx = dist;
            }

            let dist = frm.ax_distance(2, centre[1], p2[1])?;
            if dist < lby {
                lby = dist;
            } else if dist > uby {
                uby = dist;
            }
        }

        let mut lower = vec![centre[0] + lbx, centre[1] + lby];
        let mut upper = vec![centre[0] + ubx, centre[1] + uby];

        // Extend the box to cover any coordinate-system singularity it
        // contains. C builds the membership probe by copying the Ellipse,
        // giving it the base Frame and clearing Negated (ellipse.c:1170-1173),
        // so the predicate is membership of the un-negated Ellipse and the raw
        // Closed value is the effective one.
        let (a, b, angle) = self.ellipse_params()?;
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let closed = self.data.closed;
        let inside = |point: &[f64]| -> AstResult<bool> {
            if point_has_bad(&point[..2]) {
                return Ok(false);
            }
            let dx = point[0] - centre[0];
            let dy = point[1] - centre[1];
            let u = dx * cos_a + dy * sin_a;
            let v = -dx * sin_a + dy * cos_a;
            let d = u * u / (a * a) + v * v / (b * b);
            Ok(if closed { d <= 1.0 } else { d < 1.0 })
        };
        frm.norm_box(&mut lower, &mut upper, &inside)?;

        Ok((lower, upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let centre = &self.data.points[0];
        let point1 = &self.data.points[1];
        let point2 = &self.data.points[2];
        let frm = self.data.frameset.base();
        let np = self.data.effective_mesh_size();

        // Ellipse parameters in the geodesic convention used by the mesh
        // (CalcPars, ellipse.c:697): the angle is measured from the positive
        // direction of axis 2 to the primary ellipse axis.
        let a = frm.distance(centre, point1)?;
        let (_, x, y) = frm.resolve(centre, point1, point2)?;
        let b = {
            let bb = a * a - x * x;
            if bb > 0.0 { a * y / bb.sqrt() } else { a }
        };
        let point3 = vec![centre[0], centre[1] + (0.1 * a).abs()];
        let angle = frm.angle(&point3, centre, point1)?;

        let delta = 2.0 * PI / np as f64;
        let mut mesh = Vec::with_capacity(np);

        // The parametric angle phi runs from axis 2 toward axis 1. For each
        // point, move a geodesic distance dx along the primary axis, then a
        // distance dy at right angles to it (ellipse.c:1115).
        let mut phi = 0.0_f64;
        for _ in 0..np {
            let dx = a * phi.cos();
            let dy = b * phi.sin();
            let (p, dir) = frm.offset2(centre, angle, dx)?;
            let (p2, _) = frm.offset2(&p, dir + DPIBY2, dy)?;
            mesh.push(vec![p2[0], p2[1]]);
            phi += delta;
        }

        Ok(mesh)
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        Ok(self.data.points[0].clone())
    }

    fn is_bounded_inner(&self) -> bool {
        self.data.closed_curve_bounded()
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

    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
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
        // small Ellipse: chain the two forward transforms; a point is on the
        // boundary iff the result has no BAD.
        let ps1 = super::data::region_transform(&large_ellipse, pts, true)?;
        let ps2 = super::data::region_transform(&small_ellipse, &ps1, true)?;
        let mask: Vec<bool> = (0..ps2.npoint())
            .map(|j| !ps2.point(j).iter().any(|&v| is_bad(v)))
            .collect();
        let result = mask.iter().all(|&b| b);
        Ok((result, mask))
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
        super::region_bounds_current(self)
    }

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
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

    fn get_region_grid(&self) -> AstResult<PointSet> {
        crate::regions::data::region_base_grid(self)
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

    fn reg_centre_query(&self, ifrm: FrameRef) -> AstResult<Option<Vec<f64>>> {
        // Ellipse always has a centre (ellipse.c:1305-1310). Return the
        // base-Frame centre, or transform it into the current Frame.
        let centre = self.data.points[0].clone();
        match ifrm {
            FrameRef::Current => Ok(Some(self.reg_tran_point(&centre, true)?)),
            _ => Ok(Some(centre)),
        }
    }

    fn reg_centre_set(&mut self, cen: &[f64], ifrm: FrameRef) -> AstResult<()> {
        let bfrm = self.data.frameset.base().snapshot();
        let old_centre = self.data.points[0].clone();

        // Resolve the supplied centre into a base-Frame position
        // (ellipse.c:1324-1350). An ellipse always has 2 base-Frame axes; a
        // current-Frame centre is transformed to the base Frame and BAD
        // components fall back to the existing centre value.
        let mut bc = old_centre.clone();
        match ifrm {
            FrameRef::Current => {
                let tc = self.reg_tran_point(cen, false)?;
                for ic in 0..2 {
                    if !is_bad(tc[ic]) {
                        bc[ic] = tc[ic];
                    }
                }
            }
            _ => {
                for ic in 0..2 {
                    if !is_bad(cen[ic]) {
                        bc[ic] = cen[ic];
                    }
                }
            }
        }

        // Find the direction and length of the offset between the old and new
        // centre, then shift all three defining points by that same offset
        // (ellipse.c:1352-1366).
        let angle = bfrm.ax_angle(&old_centre, &bc, 2)?;
        let dist = bfrm.distance(&old_centre, &bc)?;

        for ip in 0..self.data.points.len() {
            let (shifted, _) = bfrm.offset2(&self.data.points[ip], angle, dist)?;
            self.data.points[ip] = shifted;
        }

        self.data.invalidate_cache();
        Ok(())
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
    region_frame_attrs!();

    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    /// Mirrors C `Region::MatchAxesX` (region.c:6413): the axes of the
    /// current Frame of the Region's FrameSet are matched.
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        self.data.frameset.current().match_axes_x(frm1)
    }

    /// Mirrors C `Region::Match` (region.c:6214): the current Frame of the
    /// Region's FrameSet is matched, through `astMatch` and so with its
    /// cast retry.
    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
        crate::frames::find_frame::match_with_subclass(
            self.data.frameset.current(),
            target,
            match_sub,
        )
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

    /// region.c `RemoveRegions`: a Region becomes its Frame.
    fn frame_without_regions(&self) -> Option<DynFrame> {
        Some(self.get_region_frame())
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
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.data.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.data.mapping_flags
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame, which as a
    /// Mapping is a UnitMap with its number of axes.
    fn mapping_without_regions(&self) -> Option<crate::mappings::DynMapping> {
        Some(Box::new(crate::mappings::UnitMap::new(self.nin())))
    }

    /// region.c `ReportPoints`: the current Frame formats the values.
    fn report_points(&self, _forward: bool, inp: &PointSet, out: &PointSet) {
        let frame = self.data.frameset.current();
        crate::mappings::report::write_report(&crate::frames::frame_report_lines(
            frame, frame, inp, out,
        ));
    }

    region_mapping_ident!();

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
        crate::mappings::validate_transform(self, pts, forward, None)?;
        super::data::region_transform(self, pts, forward)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
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
        // Invoke the parent Region simplify first (ellipse.c:1950); it
        // simplifies the encapsulated FrameSet and any explicit uncertainty.
        // All further work is done on the returned copy.
        let parent = self.parent_region_simplify()?;
        let new_owned;
        let this: &Ellipse = match &parent {
            Some(p) => {
                new_owned = p
                    .as_ref()
                    .as_any()
                    .downcast_ref::<Ellipse>()
                    .expect("parent_region_simplify preserves the Region class")
                    .clone();
                &new_owned
            }
            None => self,
        };
        let mut simpler = parent.is_some();

        // The best-fitting region adopted below, if any. Defaults to the
        // parent-simplified Ellipse.
        let mut new: DynRegion = this.clone_region();

        // Attempt to re-define the Ellipse within its current Frame.
        // Transforming it may leave the region no longer elliptical, so test by
        // re-fitting a mesh of boundary points. Unlike Circle, the Ellipse path
        // always attempts the fit (no UnitMap short-circuit, ellipse.c:1957).
        let map = this
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;

        // A mesh of points covering the Ellipse in its current Frame.
        let base_mesh = this.get_region_mesh()?;
        let mesh = this.data.frameset.transform(&base_mesh, true)?;

        // The uncertainty Region within the Ellipse in its current Frame.
        let unc = this.get_unc_frm(FrameRef::Current)?;

        // Transform the ellipse centre (defining point 0) into the current
        // Frame.
        let pts = PointSet::from_rows(this.data.points.iter().map(|p| p.as_slice()));
        let ps2 = map.transform(&pts, true)?;
        let nc = ps2.ncoord();
        let cen: Vec<f64> = (0..nc).map(|ic| ps2.point(0)[ic]).collect();
        let ok = cen.iter().all(|&v| v != BAD_VALUE);

        // Find the best fitting Circle through the mesh, degrading the Ellipse
        // to a Circle if every mesh point falls on the Circle boundary.
        let mut adopted = false;
        if ok {
            if let Some(newreg) = super::circle::best_circle(&mesh, &cen, unc.as_ref())? {
                if newreg.reg_pins(&mesh, None)? {
                    new = Box::new(newreg);
                    simpler = true;
                    adopted = true;
                }
            }
        }

        // Otherwise, if the region is 2-D, re-fit an Ellipse to the mesh.
        if !adopted && ok && nc == 2 {
            if let Some(newreg) = best_ellipse(&mesh, &cen, unc.as_ref())? {
                if newreg.reg_pins(&mesh, None)? {
                    new = Box::new(newreg);
                    simpler = true;
                }
            }
        }

        if simpler {
            new.reg_overlay(self, true);
            Ok((new.clone_box(), true))
        } else {
            // C's `else`: `result = astClone( this )` (ellipse.c:2036).
            Ok((self.clone_box(), false))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
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

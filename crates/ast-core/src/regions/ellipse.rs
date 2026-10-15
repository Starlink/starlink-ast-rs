//! Ellipse region: a rotated elliptical area within a 2-D Frame.
//!
//! An [`Ellipse`] is defined by a centre point, the half-lengths of its
//! primary and secondary axes, and the angle of its primary axis, measured
//! from the second axis of the Frame towards the first, as C's `astEllipse`
//! measures it.
//! The ellipse is restricted to 2-dimensional frames.

use std::f64::consts::PI;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Region};

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
                smn += (mx * py * py).abs() / crate::math::sqrt(den);
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
    shape: EllipseShape,
    uncertainty: Option<DynRegion>,
}

/// How an [`EllipseBuilder`] was given the Ellipse: `astEllipse`'s two forms.
enum EllipseShape {
    /// Form 1: the semi-axis lengths and the orientation.
    Axes { semi_axes: [f64; 2], angle: f64 },
    /// Form 0: the end of the primary axis and another boundary point.
    Points { point1: Vec<f64>, point2: Vec<f64> },
}

impl EllipseBuilder {
    /// Create a builder for an Ellipse in the given frame.
    ///
    /// # Parameters
    ///
    /// - `frame`: A 2-D frame (naxes must be 2).
    /// - `centre`: Coordinates of the ellipse centre (length 2).
    /// - `semi_axes`: `[a, b]`, the half-lengths of the primary and
    ///   secondary axes, measured as the Frame measures distance.
    /// - `angle`: the angle in radians from the second axis of the Frame to
    ///   the primary axis, positive towards the first axis, as `astEllipse`
    ///   takes it: an angle of 0 lays the primary axis along the second axis.
    pub fn new(
        frame: impl Frame + 'static,
        centre: Vec<f64>,
        semi_axes: [f64; 2],
        angle: f64,
    ) -> AstResult<Self> {
        Self::from_dyn(Box::new(frame), centre, semi_axes, angle)
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
        check_naxes(frame.as_ref())?;
        if centre.len() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Ellipse centre has {} coordinates but frame has 2 axes",
                    centre.len()
                ),
            ));
        }
        Ok(EllipseBuilder {
            frame,
            centre,
            shape: EllipseShape::Axes { semi_axes, angle },
            uncertainty: None,
        })
    }

    /// Create a builder for an Ellipse given by three points, as
    /// `astEllipse` takes them with `form = 0`: the centre, the end of the
    /// primary axis (`point1`) and any other point on the boundary
    /// (`point2`). They are stored as given, and three points that do not
    /// determine an ellipse are an error when the Ellipse is first used, as
    /// in C.
    pub fn from_points(
        frame: impl Frame + 'static,
        centre: Vec<f64>,
        point1: Vec<f64>,
        point2: Vec<f64>,
    ) -> AstResult<Self> {
        Self::from_dyn_points(Box::new(frame), centre, point1, point2)
    }

    /// [`EllipseBuilder::from_points`] from a boxed (dynamic) frame.
    pub fn from_dyn_points(
        frame: DynFrame,
        centre: Vec<f64>,
        point1: Vec<f64>,
        point2: Vec<f64>,
    ) -> AstResult<Self> {
        check_naxes(frame.as_ref())?;
        for (p, what) in [
            (&centre, "centre"),
            (&point1, "point 1"),
            (&point2, "point 2"),
        ] {
            if p.len() != 2 {
                return Err(AstError::new(
                    ErrorCode::Naxin,
                    format!(
                        "Ellipse {what} has {} coordinates but frame has 2 axes",
                        p.len()
                    ),
                ));
            }
        }
        Ok(EllipseBuilder {
            frame,
            centre,
            shape: EllipseShape::Points { point1, point2 },
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Ellipse, as `astEllipse` with form 1 does: the end of the
    /// primary axis is `a` from the centre at `angle`, and the end of the
    /// secondary axis `b` from it a quarter turn further, each placed with
    /// the Frame's `offset2`, so on the sky they are along great circles.
    pub fn build(self) -> AstResult<Ellipse> {
        let mut region = match self.shape {
            EllipseShape::Axes { semi_axes, angle } => {
                Ellipse::from_axes(self.frame, self.centre, semi_axes, angle)?
            }
            EllipseShape::Points { point1, point2 } => {
                Ellipse::from_defining_points(self.frame, self.centre, point1, point2, None)?
            }
        };

        // astSetUnc, as C's constructor calls it (region.c:13372).
        if let Some(unc) = self.uncertainty {
            region.set_uncertainty(unc)?;
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
    /// points, exactly as C stores them, after C's checks for a 2-D Frame and
    /// BAD values. Points that do not determine an ellipse are accepted
    /// here, as C accepts them, and rejected when the Ellipse is first used
    /// (ellipse.c `Cache`).
    pub(crate) fn from_defining_points(
        frame: DynFrame,
        centre: Vec<f64>,
        point1: Vec<f64>,
        point2: Vec<f64>,
        uncertainty: Option<DynRegion>,
    ) -> AstResult<Self> {
        check_naxes(frame.as_ref())?;
        for i in 0..2 {
            for (v, place) in [
                (centre[i], "the ellipse centre"),
                (point1[i], "point 1 on the circumference of the ellipse"),
                (point2[i], "point 2 on the circumference of the ellipse"),
            ] {
                if is_bad(v) {
                    return Err(AstError::new(
                        ErrorCode::Badin,
                        format!(
                            "astInitEllipse(Ellipse): The value of axis {} is undefined at \
                             {place}.",
                            i + 1
                        ),
                    ));
                }
            }
        }
        let frameset = FrameSet::new(frame);
        let points = vec![centre, point1, point2];
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = uncertainty;
        Ok(Ellipse { data })
    }

    /// Mutate the current frame with region integrity enforcement.
    ///
    /// When adaptive (the default), the FrameSet's base→current mapping is
    /// updated to account for the change, and defining points remain valid
    /// in the base frame. When non-adaptive, the change is applied to both
    /// frames without remapping.
    ///
    /// `f` makes the change to each Frame it is made to, so it may be called
    /// more than once.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: Fn(&mut dyn Frame) -> AstResult<()>,
    {
        super::data::with_region_integrity(self, f)
    }

    /// Permute the axes of this region's current frame.
    pub fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.data.perm_axes(perm)
    }

    /// The half-lengths of the primary and secondary axes, as ellipse.c
    /// `CalcPars` finds them in the base Frame: the primary from the
    /// Frame's distance between the centre and point 1, the secondary from
    /// point 2 resolved along and across the primary axis.
    fn calc_axes(&self) -> AstResult<(f64, f64)> {
        let (a, b, _) = self.cache()?;
        Ok((a, b))
    }

    /// The Ellipse's centre, axis half-lengths, angle and axis ends in its
    /// current Frame, as `astEllipsePars` gives them (ellipse.c
    /// `EllipsePars`): the defining points transformed by the FrameSet, and
    /// the axes from ellipse.c `CalcPars` in the current Frame. `p2` is the
    /// end of the secondary axis, which `CalcPars` puts in place of the
    /// second defining point. When either half-length is not positive, `a`,
    /// `b` and `angle` are BAD.
    pub fn ellipse_pars(&self) -> AstResult<EllipsePars> {
        let pts = self
            .data
            .frameset
            .transform(&self.get_region_points()?, true)?;
        let centre = pts.point(0).to_vec();
        let p1 = pts.point(1).to_vec();
        let mut p2 = pts.point(2).to_vec();
        let frm = self.data.frameset.current();

        let a = frm.distance(&centre, &p1)?;
        let (mut b, mut angle) = (BAD_VALUE, BAD_VALUE);
        if a > 0.0 {
            let (_, x, y) = frm.resolve(&centre, &p1, &p2)?;
            b = a * a - x * x;
            if b > 0.0 {
                b = a * y / crate::math::sqrt(b);
            }
            let up = [centre[0], centre[1] + (0.1 * a).abs()];
            angle = frm.angle(&up, &centre, &p1)?;
            p2 = frm.offset2(&centre, angle + DPIBY2, b)?.0;
        }
        let (a, b, angle) = if a <= 0.0 || b <= 0.0 {
            (BAD_VALUE, BAD_VALUE, BAD_VALUE)
        } else {
            (a, b, angle)
        };
        Ok(EllipsePars {
            centre,
            a,
            b,
            angle,
            p1,
            p2,
        })
    }

    /// ellipse.c `CalcPars` in the base Frame: the half-lengths of the
    /// primary and secondary axes, and the angle from the second axis to
    /// the primary axis, positive towards the first axis. When the primary
    /// half-length is not positive, the others are BAD.
    fn calc_pars(&self) -> AstResult<(f64, f64, f64)> {
        let frm = self.data.frameset.base();
        let centre = &self.data.points[0];
        let point1 = &self.data.points[1];
        let point2 = &self.data.points[2];
        let a = frm.distance(centre, point1)?;
        let (mut b, mut angle) = (BAD_VALUE, BAD_VALUE);
        if a > 0.0 {
            let (_, x, y) = frm.resolve(centre, point1, point2)?;
            b = a * a - x * x;
            if b > 0.0 {
                b = a * y / crate::math::sqrt(b);
            }
            let up = [centre[0], centre[1] + (0.1 * a).abs()];
            angle = frm.angle(&up, centre, point1)?;
        }
        Ok((a, b, angle))
    }

    /// ellipse.c `Cache`: [`Ellipse::calc_pars`], or `AST__BADIN` when the
    /// defining points do not determine an ellipse. Kept until the Ellipse
    /// changes, as C keeps it under `stale`.
    fn cache(&self) -> AstResult<(f64, f64, f64)> {
        if let Some(super::data::ClassCache::Ellipse { a, b, angle }) =
            *self.data.class_cache.borrow()
        {
            return Ok((a, b, angle));
        }
        let (a, b, angle) = self.calc_pars()?;
        if a <= 0.0 || is_bad(a) || b <= 0.0 || is_bad(b) {
            return Err(AstError::new(
                ErrorCode::Badin,
                "astInitEllipse(Ellipse): The supplied points do not determine an ellipse.",
            ));
        }
        *self.data.class_cache.borrow_mut() =
            Some(super::data::ClassCache::Ellipse { a, b, angle });
        Ok((a, b, angle))
    }

    /// An Ellipse in `frame` as ellipse.c's form-1 constructor makes it: the
    /// end of the primary axis `axes[0]` from `centre` at `angle` (from the
    /// second axis, positive towards the first), and the end of the
    /// secondary axis `axes[1]` from it a quarter turn further, each placed
    /// with the Frame's `offset2`.
    fn from_axes(
        frame: DynFrame,
        centre: Vec<f64>,
        axes: [f64; 2],
        angle: f64,
    ) -> AstResult<Ellipse> {
        let (p1, _) = frame.offset2(&centre, angle, axes[0])?;
        let (p2, _) = frame.offset2(&centre, angle + DPIBY2, axes[1])?;
        Ellipse::from_defining_points(frame, centre, p1, p2, None)
    }
}

/// What [`Ellipse::ellipse_pars`] returns: the outputs of `astEllipsePars`.
#[derive(Debug, Clone, PartialEq)]
pub struct EllipsePars {
    /// The centre, in the current Frame.
    pub centre: Vec<f64>,
    /// The half-length of the primary axis.
    pub a: f64,
    /// The half-length of the secondary axis.
    pub b: f64,
    /// The angle from the second axis to the primary axis, positive towards
    /// the first axis, in radians.
    pub angle: f64,
    /// The end of the primary axis.
    pub p1: Vec<f64>,
    /// The end of the secondary axis.
    pub p2: Vec<f64>,
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for Ellipse {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        // Whatever the caller changes may change what `cache` derives.
        self.data.invalidate_cache();
        &mut self.data
    }

    /// ellipse.c `Transform`: each point resolved, in the base Frame,
    /// into components along and across the primary axis, and compared
    /// with the axes' half-lengths.
    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let (a, b) = self.calc_axes()?;
        let centre = &self.data.points[0];
        let point1 = &self.data.points[1];
        let closed = self.data.closed_for_base_inside();
        let res = self
            .data
            .frameset
            .base()
            .resolve_points(centre, point1, pts)?;
        let (c1, c2) = (1.0 / (a * a), 1.0 / (b * b));
        Ok((0..res.npoint())
            .map(|i| {
                let p = res.point(i);
                if is_bad(p[0]) || is_bad(p[1]) {
                    return false;
                }
                let d = c1 * p[0] * p[0] + c2 * p[1] * p[1];
                if closed { d <= 1.0 } else { d < 1.0 }
            })
            .collect())
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
        let (a, b) = self.calc_axes()?;
        let point1 = &self.data.points[1];
        let closed = self.data.closed;
        let inside = |point: &[f64]| -> AstResult<bool> {
            if point_has_bad(&point[..2]) {
                return Ok(false);
            }
            let (_, x, y) = frm.resolve(centre, point1, point)?;
            if is_bad(x) || is_bad(y) {
                return Ok(false);
            }
            let d = x * x / (a * a) + y * y / (b * b);
            Ok(if closed { d <= 1.0 } else { d < 1.0 })
        };
        frm.norm_box(&mut lower, &mut upper, &inside)?;

        Ok((lower, upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        self.data.cached_mesh(|| {
            let centre = &self.data.points[0];
            let frm = self.data.frameset.base();
            let np = self.data.effective_mesh_size();

            // Ellipse parameters in the geodesic convention used by the mesh
            // (CalcPars, ellipse.c:697): the angle is measured from the positive
            // direction of axis 2 to the primary ellipse axis.
            let (a, b, angle) = self.cache()?;

            let delta = 2.0 * PI / np as f64;
            let mut mesh = Vec::with_capacity(np);

            // The parametric angle phi runs from axis 2 toward axis 1. For each
            // point, move a geodesic distance dx along the primary axis, then a
            // distance dy at right angles to it (ellipse.c:1115).
            let mut phi = 0.0_f64;
            for _ in 0..np {
                let dx = a * crate::math::cos(phi);
                let dy = b * crate::math::sin(phi);
                let (p, dir) = frm.offset2(centre, angle, dx)?;
                let (p2, _) = frm.offset2(&p, dir + DPIBY2, dy)?;
                mesh.push(vec![p2[0], p2[1]]);
                phi += delta;
            }

            Ok(mesh)
        })
    }

    fn is_bounded_inner(&self) -> AstResult<bool> {
        Ok(self.data.closed_curve_bounded())
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
    fn def_unc_cache(&self) -> Option<&std::cell::RefCell<Option<DynRegion>>> {
        Some(&self.data.def_unc)
    }

    fn explicit_unc(&self) -> Option<&DynRegion> {
        self.data.uncertainty.as_ref()
    }

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
        let (a, b, angle) = self.cache()?;
        let centre = &self.data.points[0];

        // The Ellipses whose axes are wider and narrower by half the border,
        // made as C makes them, with its form-1 constructor (ellipse.c:1531).
        let large_ellipse = Ellipse::from_axes(
            bfrm.snapshot(),
            centre.clone(),
            [a + 0.5 * drad, b + 0.5 * drad],
            angle,
        )?;
        let lim = 1.0E-6 * drad;
        let small_axes = [(a - 0.5 * drad).max(lim), (b - 0.5 * drad).max(lim)];
        let mut small_ellipse =
            Ellipse::from_axes(bfrm.snapshot(), centre.clone(), small_axes, angle)?;
        // Negate the smaller region (ellipse.c:1550).
        small_ellipse.negate();

        // On the boundary iff inside both the large Ellipse and the negated
        // small Ellipse: chain the two forward transforms; a point is on the
        // boundary iff the result has no BAD.
        let ps1 = super::data::region_transform(&large_ellipse, pts)?;
        let ps2 = super::data::region_transform(&small_ellipse, &ps1)?;
        let mask: Vec<bool> = (0..ps2.npoint())
            .map(|j| !ps2.point(j).iter().any(|&v| is_bad(v)))
            .collect();
        let result = mask.iter().all(|&b| b);
        Ok((result, mask))
    }

    fn negated_copy(&self) -> DynRegion {
        let mut copy = self.clone();
        copy.data.negate();
        Box::new(copy)
    }

    fn negate(&mut self) {
        self.data.negate();
    }

    fn is_negated(&self) -> bool {
        self.data.negated
    }

    fn test_negated(&self) -> bool {
        self.data.negated_set
    }

    fn set_negated_raw(&mut self, negated: bool, set: bool) {
        self.data.negated = negated;
        self.data.negated_set = set;
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

    fn set_fill_factor(&mut self, factor: f64) -> AstResult<()> {
        self.data.set_fill_factor(factor)
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

    fn is_bounded(&self) -> AstResult<bool> {
        self.is_bounded_inner()
    }

    fn is_adaptive(&self) -> bool {
        self.data.adaptive
    }

    fn test_adaptive(&self) -> bool {
        self.data.adaptive_set
    }

    fn set_adaptive_raw(&mut self, adaptive: bool, set: bool) {
        self.data.adaptive = adaptive;
        self.data.adaptive_set = set;
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
        self.data.invalidate_cache();
        &mut self.data.frameset
    }

    fn store_uncertainty(&mut self, unc: Option<DynRegion>) {
        self.data.uncertainty = unc;
        self.data.invalidate_cache();
    }

    fn test_region_fs(&self) -> bool {
        self.data.region_fs_set
    }

    fn set_region_fs_raw(&mut self, value: bool, set: bool) {
        self.data.region_fs = value;
        self.data.region_fs_set = set;
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

    fn reg_frame_attrib<'a>(
        &mut self,
        setting: &super::FrameSetting<'a>,
    ) -> AstResult<super::FrameSetting<'a>> {
        super::data::region_frame_attrib(self, setting)
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

    /// A Region is a Frame, which C's frame.c `GetIsLinear` reports as
    /// linear.
    fn is_linear(&self) -> bool {
        true
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

    fn invert_orientation(&mut self) {
        // A Region's forward and inverse transforms are identical (membership
        // is direction-independent), so inverting has no effect on behaviour.
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        super::data::region_transform(self, pts)
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
            new.reg_overlay(self, true)?;
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

    /// region.c `Equal`, which this class does not extend.
    fn equal(&self, other: &dyn Mapping) -> bool {
        super::data::region_equal(self, other).is_some()
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

/// astInitEllipse's check that `frame` is 2-D.
fn check_naxes(frame: &dyn Frame) -> AstResult<()> {
    let nc = frame.naxes();
    if nc != 2 {
        return Err(AstError::new(
            ErrorCode::Badin,
            format!(
                "astInitEllipse(Ellipse): The supplied {} has {nc} axes - ellipses must have \
                 exactly 2 axes.",
                crate::frames::find_frame::frame_class_name(frame)
            ),
        ));
    }
    Ok(())
}

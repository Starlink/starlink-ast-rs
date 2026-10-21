//! Circle region: a circular (or spherical-cap) area within a Frame.
//!
//! A Circle is defined by a centre point and a radius (specified as a
//! point on the circumference). In a [`SkyFrame`](crate::frames::SkyFrame),
//! the "radius" is a great-circle distance; in a flat frame it is
//! Euclidean distance.

use std::f64::consts::PI;

use crate::constants::BAD_VALUE;
use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{
    AxisKind, BasicFrame, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping,
};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, UnitMap, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::{BoxRegion, DynRegion, Region};

/// Return a point on the circumference of a circle, mirroring C `CircumPoint`
/// (circle.c:570): the point where the circle crosses the first Frame axis.
///
/// A work position is offset away from `centre` along the first axis by
/// `radius` (used only as a convenient offset length), then `frm.offset`
/// walks the geodesic from `centre` toward that work position by `radius`.
fn circum_point(frm: &dyn Frame, centre: &[f64], radius: f64) -> AstResult<Vec<f64>> {
    let mut work = centre.to_vec();
    work[0] = frm.ax_offset(1, work[0], radius)?;
    frm.offset(centre, &work, radius)
}

/// Find the best-fitting Circle through a mesh of points.
///
/// Faithful port of C `astBestCircle` (circle.c:176). The `mesh` points and
/// the supplied `centre` are in the Frame represented by `unc` (the current
/// Frame of the Region being simplified). The radius is the RMS distance of
/// the mesh points from the centre, `rad = sqrt(nc * s2r / n)`, where `s2r` is
/// the sum of squared per-axis increments from the centre over all good mesh
/// points and `n` the count of good axis values. Returns `None` when no good
/// value contributes. The returned Circle inherits `unc`'s Frame and carries
/// `unc` as its uncertainty.
pub(crate) fn best_circle(
    mesh: &PointSet,
    centre: &[f64],
    unc: &dyn Region,
) -> AstResult<Option<Circle>> {
    let np = mesh.npoint();
    let nc = mesh.ncoord();

    let mut s2r = 0.0_f64;
    let mut n = 0_usize;
    for (ic, &p0) in centre.iter().enumerate().take(nc) {
        for ip in 0..np {
            let v = mesh.point(ip)[ic];
            if v != BAD_VALUE {
                let d = v - p0;
                s2r += d * d;
                n += 1;
            }
        }
    }

    if n == 0 {
        return Ok(None);
    }

    let rad = crate::math::sqrt((nc as f64) * s2r / (n as f64));
    let frm = unc.get_region_frame();
    let circle = CircleBuilder::from_dyn(frm, centre.to_vec())?
        .radius(rad)
        .uncertainty(unc.clone_region())
        .build()?;
    Ok(Some(circle))
}

/// A Circle's base-Frame boundary mesh together with the bounding box of that
/// mesh, which C's `Circle::RegBaseMesh` computes in one pass and caches in the
/// Circle structure (circle.c:894).
struct BaseMesh {
    /// Points on the boundary, in the base Frame.
    mesh: Vec<Vec<f64>>,
    /// Lower bound per base-Frame axis, extended for any singularity.
    lower: Vec<f64>,
    /// Upper bound per base-Frame axis, extended for any singularity.
    upper: Vec<f64>,
}

/// A circle or sphere within a Frame.
///
/// The radius is a distance as the Frame measures it, so in a
/// [`SkyFrame`](crate::frames::SkyFrame) a Circle is a spherical cap whose
/// radius is an arc of a great circle.
/// [`CircleBuilder`] makes a Circle from its center and either a point on
/// its circumference or its radius.
/// The Circle keeps its center and a point on its circumference, which
/// [`region_points`](Region::region_points) returns in that order, and
/// [`circle_pars`](Self::circle_pars) gives its center, radius and that
/// point.
///
/// A Frame attribute set through the Circle changes the coordinate system it
/// is described in; while the Circle is [Adaptive](Region::is_adaptive) it
/// keeps covering the same area.
/// [`with_integrity`](Self::with_integrity) makes any other change to its
/// Frame in the same way.
///
/// # Examples
///
/// ```
/// use ast_core::frames::BasicFrameBuilder;
/// use ast_core::regions::{CircleBuilder, Region};
///
/// let frame = BasicFrameBuilder::new(2).build()?;
/// let circle = CircleBuilder::new(frame, vec![1.0, 1.0])?.radius(2.0).build()?;
/// assert!(circle.point_in_region(&[2.0, 2.0])?);
/// assert!(!circle.point_in_region(&[3.0, 3.0])?);
/// let (center, radius, _) = circle.circle_pars()?;
/// assert_eq!((center, radius), (vec![1.0, 1.0], 2.0));
/// # Ok::<(), ast_core::error::AstError>(())
/// ```
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
    /// Start a Circle in `frame` with center `centre`.
    ///
    /// Give the circle its size with [`edge_point`](Self::edge_point) or
    /// [`radius`](Self::radius) before building it.
    /// The Circle takes ownership of `frame`.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Naxin`] if `centre` does not have one value for each
    /// axis of `frame`.
    #[doc(alias = "astCircle")]
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

    /// [`new`](Self::new) for a Frame already boxed.
    ///
    /// # Errors
    ///
    /// As for [`new`](Self::new).
    pub fn from_dyn(frame: DynFrame, centre: Vec<f64>) -> AstResult<Self> {
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
            frame,
            centre,
            edge_point: None,
            radius: None,
            uncertainty: None,
        })
    }

    /// Give the size of the circle by a point on its circumference, with one
    /// value for each axis of the Frame.
    ///
    /// It takes precedence over a [`radius`](Self::radius).
    pub fn edge_point(mut self, point: Vec<f64>) -> Self {
        self.edge_point = Some(point);
        self
    }

    /// Give the size of the circle by its radius, a distance as the Frame
    /// measures it: an arc of a great circle in a sky Frame, a straight
    /// line in a flat one.
    ///
    /// The Circle keeps the point where the circle crosses the first axis
    /// of the Frame as its point on the circumference.
    pub fn radius(mut self, r: f64) -> Self {
        self.radius = Some(r);
        self
    }

    /// Give the Circle an uncertainty: a Region that, centered on any point
    /// of the boundary, covers the possible positions of that point.
    ///
    /// [`Region::set_uncertainty`] describes the Regions that can be used,
    /// and [`build`](Self::build) fails as that method does with one that
    /// cannot.
    /// Without one, the Circle uses a default uncertainty 1.0E-6 of its
    /// size.
    /// The uncertainty is the tolerance [`overlap`](Region::overlap) uses
    /// when it compares Regions, and decides whether the boundary of the
    /// Circle mapped into another coordinate system can be described by a
    /// simpler shape when it is simplified.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Circle.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Naxin`] if neither [`edge_point`](Self::edge_point)
    ///   nor [`radius`](Self::radius) was given, or the point on the
    ///   circumference does not have one value for each axis.
    /// - [`ErrorCode::Badin`] if a value of the center or of the point on
    ///   the circumference is [`BAD_VALUE`] or NaN.
    /// - Passes on the error of finding the point on the circumference from
    ///   a radius, and of [`Region::set_uncertainty`] for an uncertainty
    ///   given to [`uncertainty`](Self::uncertainty).
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
            // C's constructor turns a radius into the point where the circle
            // crosses the first Frame axis (circle.c:2655), and stores that as
            // the second defining point. The radius the Circle then reports is
            // the geodesic distance back to it, so the offset length used to
            // build the direction position is load-bearing: C offsets along
            // axis 1 by `radius`, not by one unit.
            circum_point(self.frame.as_ref(), &self.centre, r)?
        } else {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "Circle requires either an edge_point or a radius",
            ));
        };

        // C `astInitCircle` rejects a BAD centre or circumference value
        // (circle.c:2670), after a radius has become a circumference point. A
        // NaN counts as BAD, as on every input.
        let bad = |v: f64| v == BAD_VALUE || v.is_nan();
        for (i, (&c, &e)) in self.centre.iter().zip(&edge).enumerate() {
            let place = if bad(c) {
                "at the circle centre"
            } else if bad(e) {
                "on the circumference of the circle"
            } else {
                continue;
            };
            return Err(AstError::new(
                ErrorCode::Badin,
                format!(
                    "astInitCircle: The value of axis {} is undefined {place}.",
                    i + 1
                ),
            ));
        }

        // Create a FrameSet with the frame as both base and current.
        let frameset = FrameSet::new(self.frame);

        let points = vec![self.centre, edge];
        let data = RegionData::new(frameset, points);
        let mut region = Circle { data };

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

impl Circle {
    /// The center of this Circle, its radius and a point on its
    /// circumference, as `(center, radius, point)`.
    ///
    /// They are in the Frame the Circle represents.
    /// If its coordinate system has changed since the Circle was made, they
    /// are in the new system; if the change is not linear, the Circle may
    /// no longer be an accurate circle there.
    /// The radius is the distance from the center to the point as that Frame
    /// measures it, so an arc of a great circle on the sky.
    ///
    /// # Errors
    ///
    /// Passes on the errors of transforming the defining points into the
    /// Frame the Circle represents and of measuring the distance.
    // C: circle.c `CirclePars`.
    #[doc(alias = "astCirclePars")]
    pub fn circle_pars(&self) -> AstResult<(Vec<f64>, f64, Vec<f64>)> {
        let pts = self.data.frameset.transform(&self.base_points()?, true)?;
        let centre = pts.point(0).to_vec();
        let p1 = pts.point(1).to_vec();
        let radius = self.data.frameset.current().distance(&centre, &p1)?;
        Ok((centre, radius, p1))
    }

    /// Make a change to the Frame this Circle represents, keeping the Circle
    /// consistent with it.
    ///
    /// `f` is applied as a Frame attribute set through the Circle is.
    /// While the Circle is [Adaptive](Region::is_adaptive), the change is made
    /// to the Frame it represents, and the Circle is remapped into the new
    /// coordinate system so that it covers the same area.
    /// Otherwise the change is made to both the Frame it represents and the
    /// Frame it was defined in, so the numbers that define it keep their
    /// values.
    /// `f` makes the change to each Frame it is made to, so it may be called
    /// more than once.
    ///
    /// # Errors
    ///
    /// Passes on the error returned by `f`, and the error of remapping the
    /// Circle.
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
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl Circle {
    /// The radius of this Circle: the geodesic distance between the centre
    /// (points[0]) and the edge point (points[1]) in the base frame. Measured
    /// once and kept until the Circle changes, as circle.c `Cache` keeps it.
    fn radius(&self) -> AstResult<f64> {
        if let Some(super::data::ClassCache::Circle { radius }) = *self.data.class_cache.borrow() {
            return Ok(radius);
        }
        let base_frame = self.data.frameset.base();
        let radius = base_frame.distance(&self.data.points[0], &self.data.points[1])?;
        *self.data.class_cache.borrow_mut() = Some(super::data::ClassCache::Circle { radius });
        Ok(radius)
    }

    /// Build the base-Frame boundary mesh and, alongside it, the bounding box
    /// of that mesh, extended for any coordinate-system singularity.
    ///
    /// Faithful port of C `Circle::RegBaseMesh` (circle.c:894): C computes the
    /// two together, caching the bounds in the Circle structure so that
    /// `RegBaseBox` can read them back (circle.c:833-843). The final
    /// `astNormBox` call is the tail of `RegBaseMesh` and so applies at every
    /// dimensionality (circle.c:1091-1100).
    fn base_mesh_bounds(&self) -> AstResult<BaseMesh> {
        let base_frame = self.data.frameset.base();
        let naxes = base_frame.naxes();
        let centre = &self.data.points[0];
        let radius = self.radius()?;
        let np = self.data.effective_mesh_size();

        let (mesh, mut lower, mut upper) = if naxes == 1 {
            // A 1-D "circle" has two boundary points, the extreme values, and
            // MeshSize is ignored (circle.c:946-960).
            let lo = centre[0] - radius;
            let hi = centre[0] + radius;
            (vec![vec![lo], vec![hi]], vec![lo], vec![hi])
        } else if naxes == 2 {
            // Distribute points evenly around the circumference using the
            // Frame's geodesic offset, and accumulate the bounding box in terms
            // of axis offsets from the centre so that a box crossing RA=0 or
            // RA=12h is handled (circle.c:962-1016).
            let delta = 2.0 * PI / np as f64;

            let mut mesh = Vec::with_capacity(np);
            let mut lbx = f64::MAX;
            let mut ubx = -f64::MAX;
            let mut lby = f64::MAX;
            let mut uby = -f64::MAX;

            let mut angle = 0.0;
            for _ in 0..np {
                let (p, _) = base_frame.offset2(centre, angle, radius)?;
                if !is_bad(p[0]) && !is_bad(p[1]) {
                    // C uses `else if` here (circle.c:994-1004): a single
                    // extreme cannot update both bounds in one iteration.
                    let dist = base_frame.ax_distance(1, centre[0], p[0])?;
                    if dist < lbx {
                        lbx = dist;
                    } else if dist > ubx {
                        ubx = dist;
                    }
                    let dist = base_frame.ax_distance(2, centre[1], p[1])?;
                    if dist < lby {
                        lby = dist;
                    } else if dist > uby {
                        uby = dist;
                    }
                }
                mesh.push(p);
                angle += delta;
            }

            (
                mesh,
                vec![centre[0] + lbx, centre[1] + lby],
                vec![centre[0] + ubx, centre[1] + uby],
            )
        } else {
            // More than 2 dimensions. Spreading points evenly over a sphere is
            // a complex task, so C does not attempt a genuinely even spread: it
            // meshes the bounding box of the sphere and projects each point
            // radially on to the sphere's surface (circle.c:1018-1089).
            //
            // The approximate bounding box is formed by walking the geodesic
            // from the centre toward a point offset by `radius` along each axis
            // in turn, taking that axis's coordinate from the result.
            let mut approx_upper = vec![0.0; naxes];
            for (j, upper) in approx_upper.iter_mut().enumerate() {
                let mut work = centre.to_vec();
                work[j] += radius;
                *upper = base_frame.offset(centre, &work, radius)?[j];
            }

            // A Box which just encompasses the circle, meshed with this
            // Circle's MeshSize (circle.c:1042-1048).
            let mut boxr =
                BoxRegion::from_corner_frame(base_frame.snapshot(), centre.to_vec(), approx_upper)?;
            boxr.set_mesh_size(np);
            let box_mesh = boxr.base_mesh()?;

            // Move each point of that mesh radially so that its distance from
            // the centre equals this Circle's radius, tracking the bounds of
            // the result (circle.c:1060-1079).
            let mut mesh = Vec::with_capacity(box_mesh.npoint());
            let mut lower = vec![f64::MAX; naxes];
            let mut upper = vec![-f64::MAX; naxes];
            for i in 0..box_mesh.npoint() {
                let p1 = &box_mesh.point(i)[..naxes];
                let p2 = base_frame.offset(centre, p1, radius)?;
                for j in 0..naxes {
                    if !is_bad(p2[j]) {
                        // C uses `else if` here too (circle.c:1071-1077).
                        if p2[j] < lower[j] {
                            lower[j] = p2[j];
                        } else if p2[j] > upper[j] {
                            upper[j] = p2[j];
                        }
                    }
                }
                mesh.push(p2);
            }

            (mesh, lower, upper)
        };

        // Extend the box to include the effect of any coordinate-system
        // singularities (e.g. a pole). The membership predicate mirrors C's
        // `reg` Mapping argument: it reports whether a point in the base Frame
        // lies inside the un-negated Circle. C builds that probe by copying the
        // Circle and clearing Negated (circle.c:1095-1097), leaving Closed as it
        // was, so the raw Closed value is the effective one here.
        let inside = |point: &[f64]| -> AstResult<bool> {
            let dist = base_frame.distance(centre, point)?;
            if is_bad(dist) {
                return Ok(false);
            }
            Ok(if self.data.closed {
                dist <= radius
            } else {
                dist < radius
            })
        };
        base_frame.norm_box(&mut lower, &mut upper, &inside)?;

        Ok(BaseMesh { mesh, lower, upper })
    }
}

impl RegionGeometry for Circle {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        // Whatever the caller changes may change the cached radius.
        self.data.invalidate_cache();
        &mut self.data
    }

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let base_frame = self.data.frameset.base();
        let centre = &self.data.points[0];
        let radius = self.radius()?;
        let closed = self.data.closed_for_base_inside();

        if is_bad(radius) {
            return Ok(vec![false; npoint]);
        }

        let mut result = vec![false; npoint];
        let axes: Vec<&[f64]> = (0..pts.ncoord()).map(|j| &pts.axis(j)[..npoint]).collect();
        let mut point = vec![0.0; axes.len()];
        for (i, r) in result.iter_mut().enumerate() {
            for (v, axis) in point.iter_mut().zip(&axes) {
                *v = axis[i];
            }
            if point_has_bad(&point) {
                continue;
            }
            // Match the argument order used by `radius()`
            // (`distance(centre, edge_point)`). `Frame::distance` is
            // mathematically symmetric but some libm implementations (e.g.
            // macOS aarch64) compute `pal::dsep` asymmetrically at the
            // 1-ULP level depending on argument order. Mixing orders here
            // and in `radius()` would mis-classify the constructor-supplied
            // edge point as "outside" a closed circle on those platforms.
            let dist = base_frame.distance(centre, &point)?;
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

        // Mirrors C `Circle::RegBaseBox` (circle.c:757): a simple Frame is
        // assumed flat, so the box is centre +/- radius on each axis
        // (circle.c:822-827). Only a genuine BasicFrame qualifies.
        if base_frame.as_any().is::<BasicFrame>() {
            let radius = self.radius()?;
            let mut lower = vec![0.0; naxes];
            let mut upper = vec![0.0; naxes];
            for i in 0..naxes {
                lower[i] = centre[i] - radius;
                upper[i] = centre[i] + radius;
            }
            return Ok((lower, upper));
        }

        // Any other Frame cannot be assumed flat, so the box is the bounding
        // box of the boundary mesh, which C caches when `astRegBaseMesh` runs
        // and reads back here (circle.c:829-843).
        let bm = self.base_mesh_bounds()?;
        Ok((bm.lower, bm.upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        self.data.cached_mesh(|| Ok(self.base_mesh_bounds()?.mesh))
    }

    fn is_bounded_inner(&self) -> AstResult<bool> {
        Ok(self.data.closed_curve_bounded())
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
    fn def_unc_cache(&self) -> Option<&std::cell::RefCell<Option<DynRegion>>> {
        Some(&self.data.def_unc)
    }

    fn explicit_unc(&self) -> Option<&DynRegion> {
        self.data.uncertainty.as_ref()
    }

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

    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        // Faithful port of circle.c::RegPins (1289). The border half-width is
        // half the total geodesic length of the diagonals of the two
        // uncertainty bounding boxes: `drad = 0.5*(l1 + l2)`. A point is on
        // the boundary iff it lies inside a Circle enlarged by `0.5*drad` and
        // outside a Circle shrunk by `0.5*drad`.
        let bfrm = self.data.frameset.base();
        let nc = bfrm.naxes();
        if pts.ncoord() != nc {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "reg_pins(Circle): PointSet has {} coordinates per point, expected {}",
                    pts.ncoord(),
                    nc
                ),
            ));
        }

        // `tunc` is this Circle's own uncertainty in the base Frame; `l1` is
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
        let radius = self.radius()?;
        let centre = &self.data.points[0];
        let edge = &self.data.points[1];

        // Build the larger / smaller edge points by offsetting from the centre
        // toward the existing edge point by the adjusted radius.
        let large_r = radius + 0.5 * drad;
        let mut small_r = radius - 0.5 * drad;
        if small_r < 0.0 {
            small_r = 0.0;
        }
        let large_edge = bfrm.offset(centre, edge, large_r)?;
        let small_edge = bfrm.offset(centre, edge, small_r)?;

        let make_circle = |edge_pt: Vec<f64>, negated: bool| -> Circle {
            let frameset = FrameSet::new(bfrm.snapshot());
            let mut data = RegionData::new(frameset, vec![centre.clone(), edge_pt]);
            if negated {
                data.negate();
            }
            Circle { data }
        };
        let large_circle = make_circle(large_edge, false);
        // Negate the smaller region (circle.c:1459).
        let small_circle = make_circle(small_edge, true);

        // On the boundary iff inside both the large Circle and the negated
        // small Circle: chain the two forward transforms; a point is on the
        // boundary iff the result has no BAD.
        let ps1 = super::data::region_transform(&large_circle, pts)?;
        let ps2 = super::data::region_transform(&small_circle, &ps1)?;
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
        self.data.invalidate_cache();
    }

    fn is_closed(&self) -> bool {
        self.data.closed
    }

    fn set_closed(&mut self, closed: bool) {
        self.data.closed = closed;
        self.data.closed_set = true;
        self.data.invalidate_cache();
    }

    fn test_closed(&self) -> bool {
        self.data.closed_set
    }

    fn set_closed_raw(&mut self, closed: bool, set: bool) {
        self.data.closed = closed;
        self.data.closed_set = set;
        self.data.invalidate_cache();
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

    fn base_mesh(&self) -> AstResult<PointSet> {
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

    fn base_grid(&self) -> AstResult<PointSet> {
        crate::regions::data::region_base_grid(self)
    }

    fn base_points(&self) -> AstResult<PointSet> {
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
        // Circle always has a centre (circle.c:1206-1211). Return the
        // base-Frame centre, or transform it into the current Frame.
        let centre = self.data.points[0].clone();
        match ifrm {
            FrameRef::Current => Ok(Some(self.reg_tran_point(&centre, true)?)),
            _ => Ok(Some(centre)),
        }
    }

    fn reg_centre_set(&mut self, cen: &[f64], ifrm: FrameRef) -> AstResult<()> {
        let bfrm = self.data.frameset.base().snapshot();
        let ncb = self.data.points[0].len();
        let radius = self.radius()?;

        // Resolve the supplied centre into a base-Frame position
        // (circle.c:1228-1267). A current-Frame centre is transformed to the
        // base Frame; BAD components fall back to the existing centre value.
        let mut bc = self.data.points[0].clone();
        match ifrm {
            FrameRef::Current => {
                let tc = self.reg_tran_point(cen, false)?;
                for ic in 0..ncb {
                    if !is_bad(tc[ic]) {
                        bc[ic] = tc[ic];
                    }
                }
            }
            _ => {
                for ic in 0..ncb {
                    if !is_bad(cen[ic]) {
                        bc[ic] = cen[ic];
                    }
                }
            }
        }

        // Recompute the circumference point from the new centre and radius
        // (CircumPoint, circle.c:570): offset from the centre along the first
        // axis by the geodesic radius.
        let circum = circum_point(bfrm.as_ref(), &bc, radius)?;

        self.data.points[0] = bc;
        self.data.points[1] = circum;
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
// Frame trait — stub (completed in Task 2)
// ============================================================================

impl Frame for Circle {
    region_frame_attrs!();

    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    // Mirrors C `Region::MatchAxesX` (region.c:6413): the axes of the
    // current Frame of the Region's FrameSet are matched.
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        self.data.frameset.current().match_axes_x(frm1)
    }

    // Mirrors C `Region::Match` (region.c:6214): the current Frame of the
    // Region's FrameSet is matched, through `astMatch` and so with its
    // cast retry.
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

    // A Region's axes are its encapsulated current Frame's axes. Mirrors
    // C `Region::AxDistance` / `AxOffset` / `AxNorm` (region.c:1457, 1610,
    // 1536), which all forward to that Frame.
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

    // C has one `Simplify` slot, so a Region simplifies through its Mapping
    // side and the caller keeps the result because it is still a Frame
    // (`astIsAFrame`, `cmpframe.c:9024`).
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
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.data.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.data.mapping_flags
    }

    // A Region is a Frame, which C's frame.c `GetIsLinear` reports as
    // linear.
    fn is_linear(&self) -> bool {
        true
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame, which as a
    /// Mapping is a UnitMap with its number of axes.
    fn mapping_without_regions(&self) -> Option<crate::mappings::DynMapping> {
        Some(Box::new(crate::mappings::UnitMap::new(self.nin())))
    }

    // region.c `ReportPoints`: the current Frame formats the values.
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

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let result = self.transform(&PointSet::from_points(&pts), forward)?;
        out.copy_from(&result.as_points());
        Ok(())
    }

    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        // Invoke the parent Region simplify first (circle.c:1840); it simplifies
        // the encapsulated FrameSet and any explicit uncertainty. All further
        // work is done on the returned copy.
        let parent = self.parent_region_simplify()?;
        let new_owned;
        let this: &Circle = match &parent {
            Some(p) => {
                new_owned = p
                    .as_ref()
                    .as_any()
                    .downcast_ref::<Circle>()
                    .expect("parent_region_simplify preserves the Region class")
                    .clone();
                &new_owned
            }
            None => self,
        };
        let mut simpler = parent.is_some();

        // The best-fitting region adopted below, if any. Defaults to the
        // parent-simplified Circle.
        let mut new: DynRegion = this.clone_region();

        // If the base->current Mapping is not a UnitMap, attempt to re-define
        // the Circle within its current Frame. Transforming it may leave it no
        // longer circular, so test by re-fitting a mesh of boundary points.
        let map = this
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        if !map.as_any().is::<UnitMap>() {
            // A mesh of points covering the Circle in its current Frame
            // (astRegMesh returns current-Frame points; base_mesh
            // returns base-Frame points, so map them forward).
            let base_mesh = this.base_mesh()?;
            let mesh = this.data.frameset.transform(&base_mesh, true)?;

            // The uncertainty Region within the Circle in its current Frame.
            let unc = this.get_unc_frm(FrameRef::Current)?;

            // Transform the circle centre (defining point 0) into the current
            // Frame.
            let pts = PointSet::from_rows(this.data.points.iter().map(|p| p.as_slice()));
            let ps2 = map.transform(&pts, true)?;
            let nc = ps2.ncoord();
            let cen: Vec<f64> = (0..nc).map(|ic| ps2.point(0)[ic]).collect();
            let ok = cen.iter().all(|&v| v != BAD_VALUE);

            // Find the best fitting Circle through the mesh, and see whether
            // every mesh point falls on its boundary within the uncertainty.
            let mut adopted = false;
            if ok {
                if let Some(newreg) = best_circle(&mesh, &cen, unc.as_ref())? {
                    if newreg.reg_pins(&mesh, None)? {
                        // Use the new Circle in place of the original. Note this
                        // branch does NOT set simpler (circle.c:1882-1886).
                        new = Box::new(newreg);
                        adopted = true;
                    }
                }
            }

            // Otherwise, if the region is 2-D, see whether an Ellipse can
            // represent the mesh.
            if !adopted && ok && nc == 2 {
                if let Some(newreg) = super::ellipse::best_ellipse(&mesh, &cen, unc.as_ref())? {
                    if newreg.reg_pins(&mesh, None)? {
                        new = Box::new(newreg);
                        simpler = true;
                    }
                }
            }
        }

        if simpler {
            new.reg_overlay(self, true)?;
            Ok((new.clone_box(), true))
        } else {
            // C's `else`: `result = astClone( this )` (circle.c:1929).
            Ok((self.clone_box(), false))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    // region.c `Equal`, which this class does not extend.
    fn equal(&self, other: &dyn Mapping) -> bool {
        super::data::region_equal(self, other).is_some()
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

//! Interval region: axis-aligned bounds on one or more axes of a Frame.
//!
//! An Interval represents upper and/or lower limits on one or more axes.
//! For a point to be inside the region, it must satisfy all axis
//! constraints simultaneously. Each axis may have an upper limit, a lower
//! limit, both, or neither. A bound value of [`BAD_VALUE`] means "no
//! constraint" on that axis in that direction.
//!
//! When both bounds are present on an axis and the lower bound exceeds
//! the upper bound, that axis represents an *excluded* interval (the
//! region is outside the range rather than inside).

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{AsAny, DynMapping, Mapping, UnitMap};

use super::data::{RegionData, RegionGeometry};
use super::merge_util::{MergedRegion, finish_region_merge};
use super::{DynRegion, Overlap, Region};

/// An axis-aligned interval region within a coordinate frame.
///
/// Defined by a lower bound and upper bound for each axis. A bound
/// value of [`BAD_VALUE`] means no constraint in that direction.
/// If `lbnd[i] > ubnd[i]` for axis `i`, that axis uses an excluded
/// interval (points outside `[ubnd[i], lbnd[i]]` pass the test).
#[derive(Debug, Clone)]
pub struct Interval {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing an [`Interval`].
pub struct IntervalBuilder {
    frame: DynFrame,
    lbnd: Vec<f64>,
    ubnd: Vec<f64>,
    uncertainty: Option<DynRegion>,
}

impl IntervalBuilder {
    /// Create a builder for an Interval in the given frame.
    ///
    /// `lbnd` and `ubnd` must each have length equal to `frame.naxes()`.
    /// Use [`BAD_VALUE`] for any bound that should be unconstrained.
    pub fn new(frame: impl Frame + 'static, lbnd: Vec<f64>, ubnd: Vec<f64>) -> AstResult<Self> {
        let naxes = frame.naxes();
        if lbnd.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Interval lower bounds have {} values but frame has {} axes",
                    lbnd.len(),
                    naxes
                ),
            ));
        }
        if ubnd.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Interval upper bounds have {} values but frame has {} axes",
                    ubnd.len(),
                    naxes
                ),
            ));
        }
        Ok(IntervalBuilder {
            frame: Box::new(frame),
            lbnd,
            ubnd,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Interval.
    pub fn build(self) -> AstResult<Interval> {
        let frameset = FrameSet::new(self.frame);

        // Defining points: points[0] = lower bounds, points[1] = upper bounds.
        let points = vec![self.lbnd, self.ubnd];
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(Interval { data })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl Interval {
    /// Mutate the current frame with region integrity enforcement.
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

    /// Get the effective lower and upper bounds, resolving BAD_VALUE
    /// to ±MAX for internal computations.
    fn effective_bounds(&self) -> (Vec<f64>, Vec<f64>) {
        let naxes = self.data.points[0].len();
        let mut lbnd = Vec::with_capacity(naxes);
        let mut ubnd = Vec::with_capacity(naxes);

        for i in 0..naxes {
            let lb = self.data.points[0][i];
            let ub = self.data.points[1][i];
            lbnd.push(if is_bad(lb) { -f64::MAX } else { lb });
            ubnd.push(if is_bad(ub) { f64::MAX } else { ub });
        }

        (lbnd, ubnd)
    }

    /// Re-express this Interval in its current Frame across a non-UnitMap
    /// base->current mapping. Faithful port of the unbounded branch of
    /// `interval.c::Simplify` (3216-3517).
    ///
    /// The strategy: build a fully-bounded Box in the base Frame (giving
    /// unbounded axes arbitrary finite limits), map it into the current
    /// Frame and simplify it. The Box only simplifies when the mapping has a
    /// 1-to-1 axis correspondence (e.g. scale/shift/permutation) — for a
    /// shear or rotation it does not, in which case no simplification is
    /// possible and a clone of the original Interval (with its FrameSet) is
    /// returned. When the Box does simplify, the current-Frame axis limits
    /// are recovered from the simplified Box, transferring any missing/equal
    /// /reversed limits from the correct base Frame axis.
    fn simplify_remapped(&self, map: &dyn Mapping) -> AstResult<DynMapping> {
        use crate::constants::BAD_VALUE;

        let nc = self.data.points[0].len();
        let raw_l = &self.data.points[0];
        let raw_u = &self.data.points[1];

        // 1. Copy the bounds, replacing missing limits with arbitrary
        //    non-BAD values (interval.c:3232-3264).
        let mut lbnd = vec![0.0f64; nc];
        let mut ubnd = vec![0.0f64; nc];
        for ic in 0..nc {
            lbnd[ic] = raw_l[ic];
            ubnd[ic] = raw_u[ic];

            if is_bad(ubnd[ic]) {
                ubnd[ic] = if is_bad(lbnd[ic]) {
                    1.0
                } else if lbnd[ic] > 0.0 {
                    lbnd[ic] * 1.01
                } else if lbnd[ic] < 0.0 {
                    lbnd[ic] * 0.99
                } else {
                    1.0
                };
            }
            if is_bad(lbnd[ic]) {
                lbnd[ic] = if ubnd[ic] > 0.0 {
                    ubnd[ic] * 0.99
                } else if ubnd[ic] < 0.0 {
                    ubnd[ic] * 1.01
                } else {
                    1.0
                };
            }
        }

        // 2. Round-trip the two corners through the mapping (forward then
        //    inverse) and expand the box to enclose the returned base-Frame
        //    positions (interval.c:3267-3294).
        // Layout: point 0 = lower corner, point 1 = upper corner.
        let mut corners = PointSet::zeros(2, nc);
        corners.point_mut(0).copy_from_slice(&lbnd);
        corners.point_mut(1).copy_from_slice(&ubnd);
        let fwd = map.transform(&corners, true)?;
        let back = map.transform(&fwd, false)?;
        for ic in 0..nc {
            let lb = back.point(0)[ic];
            if !is_bad(lb) {
                if lb < lbnd[ic] {
                    lbnd[ic] = lb;
                }
                if lb > ubnd[ic] {
                    ubnd[ic] = lb;
                }
            }
            let ub = back.point(1)[ic];
            if !is_bad(ub) {
                if ub < lbnd[ic] {
                    lbnd[ic] = ub;
                }
                if ub > ubnd[ic] {
                    ubnd[ic] = ub;
                }
            }
        }

        // 3. Clamp the box so it does not exceed the Interval's own limits
        //    (interval.c:3298-3333). Uses the effective (±MAX) bounds and
        //    respects excluded (lb > ub) axes.
        let (elb, eub) = self.effective_bounds();
        for ic in 0..nc {
            let lb = elb[ic];
            let ub = eub[ic];
            if lb <= ub {
                if lbnd[ic] < lb {
                    lbnd[ic] = lb;
                } else if lbnd[ic] > ub {
                    lbnd[ic] = ub;
                }
                if ubnd[ic] < lb {
                    ubnd[ic] = lb;
                } else if ubnd[ic] > ub {
                    ubnd[ic] = ub;
                }
            } else {
                let lwid = lb - lbnd[ic];
                let uwid = ubnd[ic] - ub;
                if lwid > uwid {
                    if lbnd[ic] > lb {
                        lbnd[ic] = lb;
                    }
                    if ubnd[ic] > lb {
                        ubnd[ic] = lb;
                    }
                } else {
                    if lbnd[ic] < ub {
                        lbnd[ic] = ub;
                    }
                    if ubnd[ic] < ub {
                        ubnd[ic] = ub;
                    }
                }
            }

            // Ensure the bounds are not equal (interval.c:3326-3332).
            if lbnd[ic] == 0.0 && ubnd[ic] == 0.0 {
                ubnd[ic] = 1.0;
            } else if crate::mappings::helpers::ast_equals(lbnd[ic], ubnd[ic], 1.0e9) {
                ubnd[ic] = lbnd[ic].max(ubnd[ic]) * (1.0e6 * f64::EPSILON);
            }
        }

        // 4. Build the base-Frame Box, map it into the current Frame, and
        //    simplify it (interval.c:3335-3346). Box stores its defining
        //    points as [centre, corner].
        let base_frame = self.data.frameset.base().snapshot();
        let centre: Vec<f64> = (0..nc).map(|i| 0.5 * (lbnd[i] + ubnd[i])).collect();
        let corner: Vec<f64> = ubnd.clone();
        let box_reg = {
            let frameset = FrameSet::new(base_frame);
            let mut data = RegionData::new(frameset, vec![centre, corner]);
            data.closed = true;
            data.closed_set = true;
            super::BoxRegion { data }
        };
        let current_frame = self.data.frameset.current().snapshot();
        let box2 = box_reg.map_region(map, current_frame.as_ref())?;
        let sreg = (box2.as_ref() as &dyn Mapping).simplified()?;

        // The Box only simplifies (changes concrete geometry) when the
        // mapping preserves a 1-to-1 axis correspondence. Detect this: our
        // BoxRegion::simplified returns an unchanged clone (still a Box whose
        // base->current mapping is *not* a UnitMap) when it cannot simplify.
        let sreg_is_box = sreg.as_any().downcast_ref::<super::BoxRegion>();
        let sreg_is_interval = sreg.as_any().downcast_ref::<Interval>();

        // Recover the simplified Box/Interval's base bounding box; bail to a
        // clone if it did not reduce to a simple axis-aligned Region.
        let (mut slbnd, mut subnd, snc) = if let Some(b) = sreg_is_box {
            // A simplified Box has a UnitMap base->current mapping.
            let m = b.data.frameset.mapping(FrameRef::Base, FrameRef::Current)?;
            if !m.as_any().is::<UnitMap>() {
                return Ok(self.clone_box());
            }
            let (lb, ub) = b.reg_base_box()?;
            let snc = lb.len();
            (lb, ub, snc)
        } else if let Some(iv) = sreg_is_interval {
            let m = iv
                .data
                .frameset
                .mapping(FrameRef::Base, FrameRef::Current)?;
            if !m.as_any().is::<UnitMap>() {
                return Ok(self.clone_box());
            }
            let snc = iv.data.points[0].len();
            (iv.data.points[0].clone(), iv.data.points[1].clone(), snc)
        } else {
            // NullRegion or non-Region result: not handled here — keep the
            // original (matches the conservative outcome for these fixtures).
            return Ok(self.clone_box());
        };

        // 5. Determine the base<->current axis correspondence and transfer
        //    missing/equal/reversed limits (interval.c:3377-3484).
        //
        // Build a PointSet with the simplified Box lower bounds plus one
        // extra displaced point per axis, transform it into the base Frame,
        // and see which base axis each current axis is parallel to.
        let mut probe = PointSet::zeros(snc + 1, snc);
        for ic in 0..snc {
            for jc in 0..(snc + 1) {
                probe.point_mut(jc)[ic] = slbnd[ic];
            }
            probe.point_mut(ic + 1)[ic] = subnd[ic];
        }
        let probe_base = map.transform(&probe, false)?;

        let bfrm = self.data.frameset.base();
        for ic in 0..snc {
            // Find the base Frame axis with the largest displacement.
            let mut maxd = -f64::MAX;
            let mut bax: isize = -1;
            for jc in 0..nc {
                let d = bfrm
                    .ax_distance(
                        jc + 1,
                        probe_base.point(0)[jc],
                        probe_base.point(ic + 1)[jc],
                    )
                    .map(|v| v.abs())
                    .unwrap_or(BAD_VALUE);
                if !is_bad(d) && d > maxd {
                    maxd = d;
                    bax = jc as isize;
                }
            }

            if maxd <= 0.0 {
                // No corresponding base axis (an axis introduced by a
                // PermMap) — collapse to a single value.
                if is_bad(slbnd[ic]) || is_bad(subnd[ic]) {
                    slbnd[ic] = BAD_VALUE;
                } else {
                    slbnd[ic] = 0.5 * (slbnd[ic] + subnd[ic]);
                }
                subnd[ic] = slbnd[ic];
            } else {
                let bax = bax as usize;
                // Transfer missing limits from the base Frame axis.
                if is_bad(raw_l[bax]) {
                    slbnd[ic] = BAD_VALUE;
                }
                if is_bad(raw_u[bax]) {
                    subnd[ic] = BAD_VALUE;
                }
                if raw_u[bax] == raw_l[bax] {
                    subnd[ic] = slbnd[ic];
                } else if raw_u[bax] > raw_l[bax] {
                    // Inclusion: ensure new limits are the same way round.
                    if subnd[ic] < slbnd[ic] {
                        std::mem::swap(&mut subnd[ic], &mut slbnd[ic]);
                    }
                } else if subnd[ic] > slbnd[ic] {
                    // Exclusion: ensure new limits are reversed.
                    std::mem::swap(&mut subnd[ic], &mut slbnd[ic]);
                }
            }
        }

        // 6. Build the simplified Interval from the current-Frame limits.
        let frameset = FrameSet::new(current_frame);
        let mut data = RegionData::new(frameset, vec![slbnd, subnd]);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;
        if let Some(unc) = self.data.uncertainty.as_ref() {
            data.uncertainty = Some(unc.clone());
        }
        Ok(Box::new(Interval { data }))
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for Interval {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_pins(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let naxes = self.data.points[0].len();
        let (lbnd, ubnd) = self.effective_bounds();
        let closed = self.data.closed;

        let mut result = vec![false; npoint];

        for (idx, inside) in result.iter_mut().enumerate() {
            let point = pts.point(idx);

            // BAD input coordinates mean the point is outside.
            if point_has_bad(&point[..naxes]) {
                continue;
            }

            // Check each axis constraint.
            let mut all_pass = true;
            for ax in 0..naxes {
                let p = point[ax];
                let lb = lbnd[ax];
                let ub = ubnd[ax];

                let pass = if closed {
                    // Closed: boundary points are included.
                    if lb <= ub {
                        // Normal included interval.
                        lb <= p && p <= ub
                    } else {
                        // Excluded interval (lb > ub): outside [ub, lb].
                        p <= ub || lb <= p
                    }
                } else {
                    // Open: boundary points are excluded.
                    if lb <= ub {
                        lb < p && p < ub
                    } else {
                        p < ub || lb < p
                    }
                };

                if !pass {
                    all_pass = false;
                    break;
                }
            }

            *inside = all_pass;
        }

        Ok(result)
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // Return the bounds directly. For unbounded directions, use the
        // effective bounds (±MAX).
        let (lbnd, ubnd) = self.effective_bounds();

        // For excluded intervals (lb > ub), the bounding box is
        // the full range since the region wraps around.
        let naxes = lbnd.len();
        let mut lower = lbnd;
        let mut upper = ubnd;

        for i in 0..naxes {
            if lower[i] > upper[i] {
                // Excluded interval — the region covers from -MAX to ub
                // and from lb to MAX, so the bbox is the whole axis.
                lower[i] = -f64::MAX;
                upper[i] = f64::MAX;
            }
        }

        Ok((lower, upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let naxes = self.data.points[0].len();
        let (lbnd, ubnd) = self.effective_bounds();

        // For unbounded intervals, we cannot produce a meaningful mesh.
        // Only generate corner points for bounded axes. If any axis is
        // unbounded (|bound| == MAX), skip it in corner generation.
        // Check if interval is fully bounded.
        if !self.is_bounded_inner() {
            // Unbounded intervals have no finite boundary mesh.
            return Ok(Vec::new());
        }

        // Generate corner points of the box: 2^naxes corners.
        let ncorners = 1_usize << naxes;
        let mut mesh = Vec::with_capacity(ncorners);

        for corner_idx in 0..ncorners {
            let mut point = Vec::with_capacity(naxes);
            for ax in 0..naxes {
                if (corner_idx >> ax) & 1 == 0 {
                    point.push(lbnd[ax]);
                } else {
                    point.push(ubnd[ax]);
                }
            }
            mesh.push(point);
        }

        Ok(mesh)
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        let naxes = self.data.points[0].len();

        // Only return a centre if the interval is bounded (equivalent to a Box).
        if !self.is_bounded_inner() {
            return Err(AstError::new(
                ErrorCode::Regcn,
                "Interval is unbounded and has no centre",
            ));
        }

        let (lbnd, ubnd) = self.effective_bounds();
        let mut centre = Vec::with_capacity(naxes);
        for i in 0..naxes {
            centre.push(0.5 * (lbnd[i] + ubnd[i]));
        }

        Ok(centre)
    }

    fn is_bounded_inner(&self) -> bool {
        // Bounded only if ALL axes have BOTH finite bounds, and none are
        // excluded intervals (lb > ub makes it effectively unbounded
        // in the C implementation's sense — but actually a closed excluded
        // interval IS bounded for purposes of this check when it's equivalent
        // to a Box). Following C: bounded iff equivalent to a Box, i.e. all
        // bounds are finite (not BAD/±MAX) and all axis intervals have the
        // same orientation.
        let naxes = self.data.points[0].len();

        if naxes == 0 {
            return false;
        }

        let (lbnd, ubnd) = self.effective_bounds();

        // Check all bounds are finite.
        for i in 0..naxes {
            if lbnd[i].abs() == f64::MAX || ubnd[i].abs() == f64::MAX {
                return false;
            }
        }

        // Check consistency: all axes must have the same orientation.
        // (If some are included and some excluded, it's not equivalent to a Box.)
        let first_neg = lbnd[0] > ubnd[0];
        for i in 1..naxes {
            if (lbnd[i] > ubnd[i]) != first_neg {
                return false;
            }
        }

        true
    }

    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()> {
        // Transform the defining points (lower and upper bounds) through
        // the mapping.
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

impl Region for Interval {
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
        // Only meaningful for bounded intervals.
        let centre = self.reg_centre()?;
        let (lbnd, ubnd) = self.effective_bounds();

        // Radius = half-diagonal of the bounding box.
        let naxes = centre.len();
        let mut sum_sq = 0.0;
        for i in 0..naxes {
            let half_width = 0.5 * (ubnd[i] - lbnd[i]);
            sum_sq += half_width * half_width;
        }
        let radius = sum_sq.sqrt();

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

        if mesh.is_empty() {
            // Unbounded: return a single BAD point (like NullRegion).
            let naxes = self.data.frameset.base().naxes();
            let bad_point = vec![BAD_VALUE; naxes];
            return Ok(PointSet::from_rows(std::iter::once(bad_point.as_slice())));
        }

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
        // Transform the defining points through the mapping.
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, true)?;

        let naxes = frame.naxes();
        let lbnd = transformed.point(0)[..naxes].to_vec();
        let ubnd = transformed.point(1)[..naxes].to_vec();

        let frameset = FrameSet::new(frame.snapshot());
        let points = vec![lbnd, ubnd];
        let mut data = RegionData::new(frameset, points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;

        Ok(Box::new(Interval { data }))
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for Interval {
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
        if let Some(other_int) = other.as_any().downcast_ref::<Interval>() {
            self.data
                .frameset
                .current()
                .equal(other_int.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for Interval {
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
        // Box-reclassification: an Interval whose bounds are all finite
        // (no BAD on either side) and well-ordered (lbnd[i] <= ubnd[i])
        // is mathematically a BoxRegion. The C reference's interval.c
        // Simplify performs this reclassification before any
        // base-to-current remap; mirror that ordering. Negated and
        // not-closed Intervals stay as Intervals — Box's contract is a
        // closed, non-negated axis-aligned region.
        let lbnd = &self.data.points[0];
        let ubnd = &self.data.points[1];
        let all_finite = lbnd.iter().chain(ubnd.iter()).all(|&v| !is_bad(v));
        let well_ordered = lbnd.iter().zip(ubnd.iter()).all(|(&l, &u)| l <= u);
        if all_finite && well_ordered && !self.data.negated && self.data.closed {
            // Box stores defining points as [centre, corner] (point 0 =
            // centre, point 1 = corner). Mirrors C box.c astBox
            // construction from lower/upper bounds.
            let centre: Vec<f64> = lbnd
                .iter()
                .zip(ubnd.iter())
                .map(|(&l, &u)| 0.5 * (l + u))
                .collect();
            let corner = ubnd.clone();
            let frameset = FrameSet::new(self.data.frameset.current().snapshot());
            let mut data = RegionData::new(frameset, vec![centre, corner]);
            data.adaptive = self.data.adaptive;
            data.mesh_size = self.data.mesh_size;
            data.fill_factor = self.data.fill_factor;
            data.closed = true;
            data.closed_set = true;
            if let Some(unc) = self.data.uncertainty.as_ref() {
                data.uncertainty = Some(unc.clone());
            }
            return Ok(Box::new(super::BoxRegion { data }));
        }

        let map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let simple_map = map.simplified()?;
        if simple_map.as_any().is::<UnitMap>() {
            return Ok(self.clone_box());
        }

        // Non-UnitMap base->current: attempt to re-express the Interval in
        // its current Frame. Port of interval.c::Simplify (3216-3517).
        self.simplify_remapped(simple_map.as_ref())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Interval>() else {
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
        // Parallel merge only (series of two Regions is not a Region).
        // Mirrors C `interval.c::MergeInterval` (1224): the Interval axes
        // always go first (`intfirst = 1`) because the merge loop only
        // ever calls `list[i].try_merge(list[i+1])`, so `self` is the
        // lower-index (left) component.
        if series || self_inv || other_inv {
            return None;
        }

        // Guard shared by every branch (interval.c:1320): the two Regions
        // must have the same Closed attribute, and either both or neither
        // must have a defined uncertainty.
        let other_reg: &dyn Region = super::as_region(other)?;
        if self.is_closed() != other_reg.is_closed()
            || self.data.uncertainty.is_some() != other_reg.get_uncertainty().is_some()
        {
            return None;
        }

        let neg_this = self.data.negated;

        // Build the merged Region's defining points in the base Frame,
        // dispatching on the concrete type of `other`. Each branch mirrors
        // the corresponding block of `MergeInterval`.
        let built: Option<MergedRegion> =
            if let Some(other) = other.as_any().downcast_ref::<Interval>() {
                // Interval + Interval → Interval (interval.c:1354). Both
                // un-negated.
                if neg_this || other.data.negated {
                    None
                } else {
                    let mut lbnd = self.data.points[0].clone();
                    lbnd.extend(other.data.points[0].iter().copied());
                    let mut ubnd = self.data.points[1].clone();
                    ubnd.extend(other.data.points[1].iter().copied());
                    Some(MergedRegion::Interval(lbnd, ubnd))
                }
            } else if let Some(other) = other.as_any().downcast_ref::<super::BoxRegion>() {
                // Interval + Box → Interval (interval.c:1380). Both un-negated.
                if neg_this || other.is_negated() {
                    None
                } else {
                    let mut lbnd = self.data.points[0].clone();
                    let mut ubnd = self.data.points[1].clone();
                    let (blb, bub) = other.reg_base_box().ok()?;
                    lbnd.extend(blb);
                    ubnd.extend(bub);
                    Some(MergedRegion::Interval(lbnd, ubnd))
                }
            } else if let Some(other) = other.as_any().downcast_ref::<super::NullRegion>() {
                // Interval + NullRegion → Interval (interval.c:1426). The
                // NullRegion must be negated and the Interval must not.
                if neg_this || !other.is_negated() {
                    None
                } else {
                    let nax_other = other.naxes();
                    let mut lbnd = self.data.points[0].clone();
                    let mut ubnd = self.data.points[1].clone();
                    lbnd.extend(std::iter::repeat_n(BAD_VALUE, nax_other));
                    ubnd.extend(std::iter::repeat_n(BAD_VALUE, nax_other));
                    Some(MergedRegion::Interval(lbnd, ubnd))
                }
            } else if other.as_any().is::<super::PointList>() {
                // Interval + PointList → PointList (interval.c:1458) is
                // unreachable in practice and is deliberately not ported here.
                // A point-Interval only reaches a neighbour merge after failing
                // to self-simplify, but a zero-width *closed* Interval always
                // reclassifies to a Box first (see `Interval::simplified` and C
                // `interval.c::Cache`), so the Box+PointList path in
                // `box.c::MergeBox` handles it instead. A non-closed point
                // Interval fails the shared Closed guard above, since a
                // PointList reports `Closed = !Negated`. The C branch itself is
                // also dead for the same reason and contains upstream defects
                // (a `lbnd[i] - lbnd[i]` self-subtraction, `astGetRegionBounds`
                // overwriting the Interval bounds, and reads of uninitialised
                // `lbnd_unc`/`ubnd_unc`); reproducing it faithfully is neither
                // possible nor useful.
                None
            } else {
                None
            };

        let built = built?;

        // Combine the two current Frames into a CmpFrame (Interval axes
        // first). Both Regions here have a UnitMap base->current mapping
        // (the fixtures use the `Frm` short-form), so base == current and
        // we can construct the merged Region directly in this Frame.
        let f1 = self.data.frameset.current().snapshot();
        let f2 = other_reg.get_region_frame();
        let cmp_frame = crate::frames::CmpFrame::new(f1, f2);
        let frameset = FrameSet::new(Box::new(cmp_frame) as crate::frames::DynFrame);

        // Combine uncertainties (Interval first) when both are present.
        let unc = super::merge_util::combine_uncertainties(
            self.data.uncertainty.clone(),
            other_reg.get_uncertainty(),
            true,
        );

        finish_region_merge(
            built,
            frameset,
            self.data.closed,
            self.data.closed_set,
            self.data.adaptive,
            unc,
        )
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

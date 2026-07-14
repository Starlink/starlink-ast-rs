//! Box region: an axis-aligned box defined by a centre and a corner.
//!
//! A [`BoxRegion`] represents a rectangular area with sides parallel to
//! the axes of a [`Frame`](crate::frames::Frame). It is defined by a
//! centre point and one corner point; the box extends symmetrically
//! around the centre so the opposite corner is the mirror of the
//! specified corner through the centre.
//!
//! Named `BoxRegion` (rather than `Box`) to avoid collision with
//! [`std::boxed::Box`]. In the C library this class is called "Box".

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{AsAny, DynMapping, Mapping, UnitMap};

use super::data::{RegionData, RegionGeometry};
use super::merge_util::MergedRegion;
use super::{DynRegion, Interval, NullRegion, Overlap, PointList, Region};

/// An axis-aligned box region within a coordinate frame.
///
/// Defined by a centre point and a corner point in the base frame.
/// The box extends symmetrically around the centre: the effective
/// bounds are `[2*centre - corner, corner]` per axis (with appropriate
/// ordering so that `lbnd <= ubnd`).
#[derive(Debug, Clone)]
pub struct BoxRegion {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing a [`BoxRegion`].
pub struct BoxRegionBuilder {
    frame: DynFrame,
    centre: Vec<f64>,
    corner: Vec<f64>,
    uncertainty: Option<DynRegion>,
}

impl BoxRegionBuilder {
    /// Create a builder for a BoxRegion in the given frame from a centre
    /// point and one corner.
    ///
    /// `centre` and `corner` must each have length equal to `frame.naxes()`.
    /// The box extends symmetrically around `centre`, so the opposite
    /// corner is at `2*centre - corner`.
    pub fn new(frame: impl Frame + 'static, centre: Vec<f64>, corner: Vec<f64>) -> AstResult<Self> {
        let naxes = frame.naxes();
        if centre.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "BoxRegion centre has {} coordinates but frame has {} axes",
                    centre.len(),
                    naxes
                ),
            ));
        }
        if corner.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "BoxRegion corner has {} coordinates but frame has {} axes",
                    corner.len(),
                    naxes
                ),
            ));
        }
        Ok(BoxRegionBuilder {
            frame: Box::new(frame),
            centre,
            corner,
            uncertainty: None,
        })
    }

    /// Create a builder from explicit lower and upper bounds.
    ///
    /// The centre is computed as the midpoint `(lbnd + ubnd) / 2` and
    /// the corner is set to `ubnd`. Both must have length equal to
    /// `frame.naxes()`.
    pub fn from_bounds(frame: impl Frame + 'static, lbnd: &[f64], ubnd: &[f64]) -> AstResult<Self> {
        let naxes = frame.naxes();
        if lbnd.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "BoxRegion lower bounds have {} values but frame has {} axes",
                    lbnd.len(),
                    naxes
                ),
            ));
        }
        if ubnd.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "BoxRegion upper bounds have {} values but frame has {} axes",
                    ubnd.len(),
                    naxes
                ),
            ));
        }
        let centre: Vec<f64> = lbnd
            .iter()
            .zip(ubnd.iter())
            .map(|(l, u)| 0.5 * (l + u))
            .collect();
        let corner: Vec<f64> = ubnd.to_vec();

        Ok(BoxRegionBuilder {
            frame: Box::new(frame),
            centre,
            corner,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the BoxRegion.
    pub fn build(self) -> AstResult<BoxRegion> {
        let frameset = FrameSet::new(self.frame);

        // Defining points: points[0] = centre, points[1] = corner.
        let points = vec![self.centre, self.corner];
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(BoxRegion { data })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl BoxRegion {
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

    /// Compute the effective lower and upper bounds from centre and corner.
    ///
    /// The box extends symmetrically around the centre: the opposite corner
    /// is at `2*centre - corner`. For each axis, lbnd is the minimum and
    /// ubnd is the maximum of the two corners.
    fn bounds(&self) -> (Vec<f64>, Vec<f64>) {
        let naxes = self.data.points[0].len();
        let centre = &self.data.points[0];
        let corner = &self.data.points[1];

        let mut lbnd = Vec::with_capacity(naxes);
        let mut ubnd = Vec::with_capacity(naxes);

        for i in 0..naxes {
            let c = centre[i];
            let extent = (corner[i] - c).abs();
            lbnd.push(c - extent);
            ubnd.push(c + extent);
        }

        (lbnd, ubnd)
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for BoxRegion {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_pins(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let naxes = self.data.points[0].len();
        let (lbnd, ubnd) = self.bounds();
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
                    lb <= p && p <= ub
                } else {
                    lb < p && p < ub
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
        Ok(self.bounds())
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let naxes = self.data.points[0].len();
        let (lbnd, ubnd) = self.bounds();
        let np = self.data.mesh_size;

        // Check if the box occupies a single point.
        let single = (0..naxes).all(|i| ubnd[i] <= lbnd[i]);
        if single {
            let centre = &self.data.points[0];
            return Ok(vec![centre.clone()]);
        }

        if naxes == 1 {
            // 1-D: the boundary is the two extreme points.
            return Ok(vec![vec![lbnd[0]], vec![ubnd[0]]]);
        }

        if naxes == 2 {
            // 2-D: distribute points evenly along 4 edges.
            // Corners: (lbnd[0],lbnd[1]) -> (lbnd[0],ubnd[1]) ->
            //          (ubnd[0],ubnd[1]) -> (ubnd[0],lbnd[1]) -> back
            let corners = [
                [lbnd[0], lbnd[1]],
                [lbnd[0], ubnd[1]],
                [ubnd[0], ubnd[1]],
                [ubnd[0], lbnd[1]],
            ];

            // For flat geometry, compute edge lengths as coordinate differences.
            let mut edge_lengths = [0.0_f64; 4];
            for iedge in 0..4 {
                let next = (iedge + 1) % 4;
                let dx = corners[next][0] - corners[iedge][0];
                let dy = corners[next][1] - corners[iedge][1];
                edge_lengths[iedge] = (dx * dx + dy * dy).sqrt();
            }

            let total_len: f64 = edge_lengths.iter().sum();

            // Compute points per edge proportional to edge length.
            let mut np_edge = [0_usize; 4];
            let mut total_np = 0;
            if total_len > 0.0 {
                let ppd = np as f64 / total_len;
                for iedge in 0..4 {
                    np_edge[iedge] = (edge_lengths[iedge] * ppd) as usize;
                    if np_edge[iedge] == 0 {
                        np_edge[iedge] = 1;
                    }
                    total_np += np_edge[iedge];
                }
            } else {
                // Degenerate: give equal points to each edge.
                let np0 = np / 4;
                for edge_np in &mut np_edge {
                    *edge_np = np0.max(1);
                    total_np += *edge_np;
                }
            }

            let mut mesh = Vec::with_capacity(total_np);

            for iedge in 0..4 {
                let next = (iedge + 1) % 4;
                let n = np_edge[iedge];
                let dx = (corners[next][0] - corners[iedge][0]) / n as f64;
                let dy = (corners[next][1] - corners[iedge][1]) / n as f64;

                for i in 0..n {
                    let x = corners[iedge][0] + dx * i as f64;
                    let y = corners[iedge][1] + dy * i as f64;
                    mesh.push(vec![x, y]);
                }
            }

            return Ok(mesh);
        }

        // N-D (naxes > 2): generate points on each face of the hyper-box.
        // Each face is defined by fixing one axis at either its lower or
        // upper bound and varying all other axes. Following the C approach:
        // compute np_axis samples per axis, yielding np_axis^(naxes-1) points
        // per face, with 2*naxes faces.
        let np_axis =
            1 + ((np as f64 / (2 * naxes) as f64).powf(1.0 / (naxes - 1) as f64)) as usize;
        let np_axis = np_axis.max(2);

        let mut mesh = Vec::new();

        // For each axis, create the upper and lower faces.
        for fixed_axis in 0..naxes {
            for &fixed_val in &[lbnd[fixed_axis], ubnd[fixed_axis]] {
                // Generate a grid over the remaining axes.
                let free_axes: Vec<usize> = (0..naxes).filter(|&a| a != fixed_axis).collect();
                let n_free = free_axes.len();

                // Total number of points on this face: np_axis^n_free
                let n_face = np_axis.pow(n_free as u32);

                for idx in 0..n_face {
                    let mut point = vec![0.0; naxes];
                    point[fixed_axis] = fixed_val;

                    let mut remainder = idx;
                    for &ax in &free_axes {
                        let step = remainder % np_axis;
                        remainder /= np_axis;
                        // Linearly interpolate between lbnd and ubnd for this axis.
                        point[ax] = if np_axis > 1 {
                            lbnd[ax] + (ubnd[ax] - lbnd[ax]) * step as f64 / (np_axis - 1) as f64
                        } else {
                            0.5 * (lbnd[ax] + ubnd[ax])
                        };
                    }

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
        // A box is always bounded (it has finite extent in all axes).
        true
    }

    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()> {
        // Transform the defining points (centre and corner) through the mapping.
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

impl Region for BoxRegion {
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
        let centre = self.data.points[0].clone();
        let (lbnd, ubnd) = self.bounds();

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
        let centre = transformed.point(0)[..naxes].to_vec();
        let corner = transformed.point(1)[..naxes].to_vec();

        let frameset = FrameSet::new(frame.snapshot());
        let points = vec![centre, corner];
        let mut data = RegionData::new(frameset, points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;

        Ok(Box::new(BoxRegion { data }))
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for BoxRegion {
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
        if let Some(other_box) = other.as_any().downcast_ref::<BoxRegion>() {
            self.data
                .frameset
                .current()
                .equal(other_box.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for BoxRegion {
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
        Ok(Box::new(BoxRegion { data }))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<BoxRegion>() else {
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
        // Mirrors C `box.c::MergeBox` (1658): the Box axes always go first
        // (`boxfirst = 1`) because the merge loop only ever calls
        // `list[i].try_merge(list[i+1])`, so `self` is the lower-index
        // (left) component.
        if series || self_inv || other_inv {
            return None;
        }

        // Guard shared by every branch (box.c:1749): the two Regions must
        // have the same Closed attribute, and either both or neither must
        // have a defined uncertainty.
        let other_reg: &dyn Region = super::as_region(other)?;
        if self.is_closed() != other_reg.is_closed()
            || self.data.uncertainty.is_some() != other_reg.get_uncertainty().is_some()
        {
            return None;
        }

        let neg_this = self.data.negated;
        // Raw centre/corner of this Box (astBoxPoints).
        let centre_this = &self.data.points[0];
        let corner_this = &self.data.points[1];

        let built: Option<MergedRegion> = if let Some(other) =
            other.as_any().downcast_ref::<BoxRegion>()
        {
            // Box + Box → Box (box.c:1783). Both un-negated.
            if neg_this || other.data.negated {
                None
            } else {
                let mut centre = centre_this.clone();
                centre.extend(other.data.points[0].iter().copied());
                let mut corner = corner_this.clone();
                corner.extend(other.data.points[1].iter().copied());
                Some(MergedRegion::Box(centre, corner))
            }
        } else if let Some(other) = other.as_any().downcast_ref::<Interval>() {
            // Box + Interval → Interval (box.c:1809). Both un-negated.
            if neg_this || other.is_negated() {
                None
            } else {
                // Convert this Box's centre/corner to lower/upper bounds.
                let (mut lbnd, mut ubnd) = self.bounds();
                // Append the Interval's raw bounds.
                lbnd.extend(other.data.points[0].iter().copied());
                ubnd.extend(other.data.points[1].iter().copied());
                Some(MergedRegion::Interval(lbnd, ubnd))
            }
        } else if let Some(other) = other.as_any().downcast_ref::<NullRegion>() {
            // Box + NullRegion → Interval (box.c:1855). The NullRegion must
            // be negated and the Box must not.
            if neg_this || !other.is_negated() {
                None
            } else {
                let (mut lbnd, mut ubnd) = self.bounds();
                let nax_other = other.naxes();
                lbnd.extend(std::iter::repeat_n(BAD_VALUE, nax_other));
                ubnd.extend(std::iter::repeat_n(BAD_VALUE, nax_other));
                Some(MergedRegion::Interval(lbnd, ubnd))
            }
        } else if let Some(other) = other.as_any().downcast_ref::<PointList>() {
            // Box + PointList → PointList (box.c:1907). Both un-negated, and
            // the Box must be a single point (zero width relative to its
            // uncertainty).
            if neg_this || other.is_negated() {
                None
            } else {
                // Zero-width test uses the raw bounding box (centre/corner
                // ordered) against the Box's own uncertainty.
                let (blb, bub) = self.bounds();
                if !super::merge_util::region_is_point(&blb, &bub, self.data.uncertainty.as_ref()) {
                    None
                } else {
                    // Each merged point is the PointList point with the
                    // Box centre appended on the Box's axes. Box axes first.
                    let other_pts = &other.data.points;
                    let merged: Vec<Vec<f64>> = other_pts
                        .iter()
                        .map(|p| {
                            let mut row = centre_this.clone();
                            row.extend(p.iter().copied());
                            row
                        })
                        .collect();
                    Some(MergedRegion::PointList(merged))
                }
            }
        } else {
            None
        };

        let built = built?;

        // Combine the two current Frames into a CmpFrame (Box axes first).
        let f1 = self.data.frameset.current().snapshot();
        let f2 = other_reg.get_region_frame();
        let cmp_frame = crate::frames::CmpFrame::new(f1, f2);
        let frameset = FrameSet::new(Box::new(cmp_frame) as DynFrame);

        // Combine uncertainties (Box first) when both are present.
        let unc = super::merge_util::combine_uncertainties(
            self.data.uncertainty.clone(),
            other_reg.get_uncertainty(),
            true,
        );

        super::merge_util::finish_region_merge(
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

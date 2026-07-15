//! Prism: an extrusion of a region into higher dimensions.
//!
//! A [`Prism`] represents an extrusion of an existing Region into one or
//! more orthogonal dimensions (specified by another Region). If the Region
//! to be extruded has N axes, and the Region defining the extrusion has M
//! axes, then the resulting Prism will have (N + M) axes.
//!
//! A point is inside the Prism if the first N axis values correspond to a
//! point inside the Region being extruded, and the remaining M axis values
//! correspond to a point inside the Region defining the extrusion.
//!
//! As an example, a cylinder can be represented by extruding an existing
//! Circle using an Interval to define the extrusion. In this case, the
//! Interval would have a single axis and would specify the upper and lower
//! limits of the cylinder along its length.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{CmpFrame, DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, UnitMap};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Overlap, Region};

/// An extrusion of a region into higher dimensions.
///
/// The Prism combines two sub-regions of potentially different
/// dimensionality. The first region defines the "base" shape and the
/// second defines the extrusion. The combined number of axes is the
/// sum of the two sub-regions' axes.
///
/// A point `[x1, ..., xN, y1, ..., yM]` is inside the Prism if
/// `[x1, ..., xN]` is inside region1 (N axes) AND `[y1, ..., yM]` is
/// inside region2 (M axes).
#[derive(Debug, Clone)]
pub struct Prism {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
    /// The region being extruded (contributes the first N axes).
    pub(crate) region1: DynRegion,
    /// The region defining the extrusion (contributes the last M axes).
    pub(crate) region2: DynRegion,
}

/// Builder for constructing a [`Prism`].
pub struct PrismBuilder {
    region1: DynRegion,
    region2: DynRegion,
    uncertainty: Option<DynRegion>,
}

impl PrismBuilder {
    /// Create a builder for a Prism that extrudes `region1` along `region2`.
    ///
    /// The resulting Prism will have `region1.naxes() + region2.naxes()` axes.
    /// `region1` defines the base shape (first N axes) and `region2` defines
    /// the extrusion (remaining M axes).
    pub fn new(region1: impl Region + 'static, region2: impl Region + 'static) -> AstResult<Self> {
        Ok(PrismBuilder {
            region1: Box::new(region1),
            region2: Box::new(region2),
            uncertainty: None,
        })
    }

    /// Create a builder from boxed (dynamic) regions.
    pub fn from_dyn(region1: DynRegion, region2: DynRegion) -> AstResult<Self> {
        Ok(PrismBuilder {
            region1,
            region2,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Prism.
    pub fn build(self) -> AstResult<Prism> {
        // Create a CmpFrame combining the frames of both sub-regions.
        let frame1 = self.region1.get_region_frame();
        let frame2 = self.region2.get_region_frame();
        let cmpframe = CmpFrame::new(frame1, frame2);
        let frameset = FrameSet::new(Box::new(cmpframe) as DynFrame);

        // Prism has no defining points of its own (geometry is in sub-regions).
        let points = Vec::new();
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(Prism {
            data,
            region1: self.region1,
            region2: self.region2,
        })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl Prism {
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

    /// Get a reference to the first component region (the extruded region).
    pub fn region1(&self) -> &dyn Region {
        self.region1.as_ref()
    }

    /// Get a reference to the second component region (the extrusion region).
    pub fn region2(&self) -> &dyn Region {
        self.region2.as_ref()
    }

    /// Number of axes contributed by region1.
    fn naxes1(&self) -> usize {
        self.region1.naxes()
    }

    /// Number of axes contributed by region2.
    fn naxes2(&self) -> usize {
        self.region2.naxes()
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for Prism {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_pins(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let naxes1 = self.naxes1();
        let naxes2 = self.naxes2();
        let naxes_total = naxes1 + naxes2;

        let mut result = vec![false; npoint];

        for (r, point) in result.iter_mut().zip((0..npoint).map(|i| pts.point(i))) {
            // Skip if any coordinate is BAD.
            if point_has_bad(&point[..naxes_total]) {
                continue;
            }

            // Split the point: first naxes1 coords for region1,
            // remaining naxes2 coords for region2.
            let part1 = &point[..naxes1];
            let part2 = &point[naxes1..naxes_total];

            // Test against both sub-regions.
            let in1 = self.region1.point_in_region(part1)?;
            if !in1 {
                continue;
            }
            let in2 = self.region2.point_in_region(part2)?;
            *r = in2;
        }

        Ok(result)
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // Concatenate bounding boxes of both sub-regions.
        let (lbnd1, ubnd1) = self.region1.get_region_bounds()?;
        let (lbnd2, ubnd2) = self.region2.get_region_bounds()?;

        let mut lbnd = lbnd1;
        lbnd.extend(lbnd2);

        let mut ubnd = ubnd1;
        ubnd.extend(ubnd2);

        Ok((lbnd, ubnd))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let naxes1 = self.naxes1();
        let naxes2 = self.naxes2();

        // The boundary of a Prism is:
        //   (interior of region1 × boundary of region2)  — "end caps"
        //   ∪ (boundary of region1 × interior of region2) — "side walls"
        //
        // Following the C library (prism.c RegBaseMesh): we split the Prism's
        // mesh_size budget between end caps and side walls, get boundary meshes
        // and interior grids for both sub-regions, then form the cross products.

        let mszp = self.data.mesh_size;

        // Compute mesh/grid sizes for each component region.
        // For 1-D regions the mesh is always 2 points (lower + upper bound).
        let msz1 = if naxes1 == 1 {
            2
        } else {
            (0.5 * mszp as f64).sqrt() as usize
        };
        let gsz2 = (0.5 * mszp as f64) as usize / msz1.max(1);

        let msz2 = if naxes2 == 1 {
            2
        } else {
            (0.5 * mszp as f64).sqrt() as usize
        };
        let gsz1 = (0.5 * mszp as f64) as usize / msz2.max(1);

        // Get boundary meshes from both sub-regions.
        // Use get_region_mesh which uses the sub-region's own mesh_size.
        let mesh1 = self.region1.get_region_mesh()?;
        let mesh2 = self.region2.get_region_mesh()?;

        // Get interior grids for both sub-regions using the computed sizes.
        let grid1 = self.region1.get_region_grid(gsz1)?;
        let grid2 = self.region2.get_region_grid(gsz2)?;

        let actual_msz1 = mesh1.npoint();
        let actual_gsz1 = grid1.len();
        let actual_msz2 = mesh2.npoint();
        let actual_gsz2 = grid2.len();

        let total = actual_gsz1 * actual_msz2 + actual_msz1 * actual_gsz2;
        let mut result = Vec::with_capacity(total);

        // End caps: grid1 × mesh2
        // Replicate the interior grid of region1 at every boundary point
        // of region2.
        for i in 0..actual_msz2 {
            let p2 = mesh2.point(i);
            if p2[..naxes2].iter().any(|&v| is_bad(v)) {
                continue;
            }
            for grid1_point in &grid1 {
                let mut point = Vec::with_capacity(naxes1 + naxes2);
                point.extend_from_slice(&grid1_point[..naxes1]);
                point.extend_from_slice(&p2[..naxes2]);
                result.push(point);
            }
        }

        // Side walls: mesh1 × grid2
        // Replicate the interior grid of region2 at every boundary point
        // of region1.
        for i in 0..actual_msz1 {
            let p1 = mesh1.point(i);
            if p1[..naxes1].iter().any(|&v| is_bad(v)) {
                continue;
            }
            for grid2_point in &grid2 {
                let mut point = Vec::with_capacity(naxes1 + naxes2);
                point.extend_from_slice(&p1[..naxes1]);
                point.extend_from_slice(&grid2_point[..naxes2]);
                result.push(point);
            }
        }

        Ok(result)
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        // Concatenate centres of both sub-regions.
        let (centre1, _) = self.region1.get_region_disc()?;
        let (centre2, _) = self.region2.get_region_disc()?;

        let mut centre = centre1;
        centre.extend(centre2);
        Ok(centre)
    }

    fn is_bounded_inner(&self) -> bool {
        // A Prism is bounded if both sub-regions are bounded.
        self.region1.is_bounded() && self.region2.is_bounded()
    }

    fn remap_defining_points(&mut self, _mapping: &dyn Mapping, _forward: bool) -> AstResult<()> {
        // Prism has no defining points of its own.
        // The sub-regions are immutable once constructed.
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region trait
// ============================================================================

impl Region for Prism {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        // Check for BAD coordinates.
        if point_has_bad(point) {
            return Ok(false);
        }

        let naxes_total = self.naxes1() + self.naxes2();

        // Transform from current frame to base frame.
        let pts = PointSet::from_rows(std::iter::once(point));
        let base_pts = self.data.frameset.transform(&pts, false)?;
        let base_point = base_pts.point(0);

        // Check for BAD values after transform.
        if base_point[..naxes_total].iter().any(|&v| is_bad(v)) {
            return Ok(false);
        }

        // Split and test against sub-regions.
        let naxes1 = self.naxes1();
        let part1 = &base_point[..naxes1];
        let part2 = &base_point[naxes1..naxes_total];

        let in1 = self.region1.point_in_region(part1)?;
        let in2 = if in1 {
            self.region2.point_in_region(part2)?
        } else {
            false
        };

        let inside = in1 && in2;

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
        // Store the FrameSet on the parent Region structure (and fix a
        // dummy-FS uncertainty). Mirrors C `prism.c` SetRegFS invoking the
        // parent method first (3170).
        self.data.set_region_fs(frame);

        // Recurse into component Regions that still carry a dummy FrameSet,
        // giving each the corresponding axes picked from `frame`:
        // region1 gets [0, nax1), region2 gets [nax1, nax1+nax2). Mirrors
        // C `prism.c` SetRegFS (3172-3200).
        let nax1 = self.region1.naxes();
        if self.region1.has_dummy_fs() {
            let axes: Vec<i32> = (0..nax1 as i32).collect();
            if let Ok(cfrm) = frame.pick_axes_snapshot(&axes) {
                self.region1.set_region_fs(cfrm.as_ref());
            }
        }
        if self.region2.has_dummy_fs() {
            let nax2 = self.region2.naxes();
            let axes: Vec<i32> = (0..nax2 as i32).map(|i| nax1 as i32 + i).collect();
            if let Ok(cfrm) = frame.pick_axes_snapshot(&axes) {
                self.region2.set_region_fs(cfrm.as_ref());
            }
        }
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        // Compute bounding disc from the bounding box.
        let (lbnd, ubnd) = self.reg_base_box()?;
        let naxes = lbnd.len();

        let mut centre = vec![0.0; naxes];
        let mut sum_sq = 0.0;
        for i in 0..naxes {
            if is_bad(lbnd[i]) || is_bad(ubnd[i]) {
                return Err(AstError::new(
                    ErrorCode::Regcn,
                    "Prism has unbounded axes; cannot compute bounding disc",
                ));
            }
            centre[i] = 0.5 * (lbnd[i] + ubnd[i]);
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
            // Return a single BAD point (matching C behaviour for empty meshes).
            let naxes = self.naxes1() + self.naxes2();
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
        // Prism has no defining points of its own.
        let naxes = self.naxes1() + self.naxes2();
        Ok(PointSet::zeros(0, naxes))
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

    fn map_region(&self, _mapping: &dyn Mapping, frame: &dyn Frame) -> AstResult<DynRegion> {
        // For Prism, mapping the whole region is complex because the mapping
        // might mix axes between the two sub-regions. For now, we create a
        // new Prism with the same sub-regions and a new frameset.
        let frameset = FrameSet::new(frame.snapshot());
        let points = Vec::new();
        let mut data = RegionData::new(frameset, points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;

        Ok(Box::new(Prism {
            data,
            region1: self.region1.clone(),
            region2: self.region2.clone(),
        }))
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for Prism {
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
        if let Some(other_prism) = other.as_any().downcast_ref::<Prism>() {
            self.data
                .frameset
                .current()
                .equal(other_prism.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for Prism {
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
        // No-op for now — region inversion handled via negation.
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
        // For Prism, rebuild with the current frame. Since the sub-regions
        // have their own geometry, clone them and update the outer FrameSet.
        let current_frame = self.data.frameset.current().snapshot();
        let frameset = FrameSet::new(current_frame);
        let points = Vec::new();
        let mut data = RegionData::new(frameset, points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;
        Ok(Box::new(Prism {
            data,
            region1: self.region1.clone(),
            region2: self.region2.clone(),
        }))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Prism>() else {
            return false;
        };
        self.data.negated == other.data.negated
            && self.data.closed == other.data.closed
            && Frame::equal(self.data.frameset.current(), other.data.frameset.current())
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

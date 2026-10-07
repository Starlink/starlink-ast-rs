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
use crate::frames::{
    AxisKind, CmpFrame, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping,
};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, ViaSimplify};

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
        let data = RegionData::new(frameset, points);
        let mut region = Prism {
            data,
            region1: self.region1,
            region2: self.region2,
        };

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

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
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

    /// prism.c `RegBaseMesh`: the Prism's boundary is the first component's
    /// interior at every point of the second's boundary, and the second's
    /// interior at every point of the first's. Each component gives its
    /// current-Frame boundary mesh and interior grid (`RegMesh`, `RegGrid`)
    /// at sizes that split the Prism's MeshSize between the two, set on the
    /// component for the purpose; a 1-D component's mesh has 2 points.
    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        // A component has a mesh if it is bounded when negated or not.
        let has_mesh = |r: &DynRegion| r.is_bounded() || r.negated_copy().is_bounded();
        if !has_mesh(&self.region1) || !has_mesh(&self.region2) {
            return Err(AstError::new(
                ErrorCode::Inter,
                "astRegBaseMesh(Prism): No mesh can be produced for the Prism bacause \
                 one of its component Regions is unbounded.",
            ));
        }
        // C's sizes are `int`s assigned from doubles, so each truncates.
        let mszp = self.data.effective_mesh_size() as f64;
        let nax1 = self.region1.naxes();
        let nax2 = self.region2.naxes();
        let msz1 = if nax1 == 1 {
            2
        } else {
            (0.5 * mszp).sqrt() as i32
        };
        let gsz2 = (0.5 * mszp / msz1 as f64) as i32;
        let msz2 = if nax2 == 1 {
            2
        } else {
            (0.5 * mszp).sqrt() as i32
        };
        let gsz1 = (0.5 * mszp / msz2 as f64) as i32;
        // The sizes are set on copies, which C does by setting them on the
        // components and putting their own MeshSize back afterwards.
        let sized = |r: &DynRegion, size: i32| {
            let mut copy = r.clone_region();
            copy.set_mesh_size(size.max(0) as usize);
            copy
        };
        let mesh2 = sized(&self.region2, msz2).reg_mesh()?;
        let grid2 = sized(&self.region2, gsz2).reg_grid()?;
        let mesh1 = sized(&self.region1, msz1).reg_mesh()?;
        let grid1 = sized(&self.region1, gsz1).reg_grid()?;

        let mut result =
            Vec::with_capacity(grid1.npoint() * mesh2.npoint() + mesh1.npoint() * grid2.npoint());
        for i in 0..mesh2.npoint() {
            for j in 0..grid1.npoint() {
                let mut point = grid1.point(j).to_vec();
                point.extend_from_slice(mesh2.point(i));
                result.push(point);
            }
        }
        for i in 0..mesh1.npoint() {
            for j in 0..grid2.npoint() {
                let mut point = mesh1.point(i).to_vec();
                point.extend_from_slice(grid2.point(j));
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

    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        _unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        // Faithful port of prism.c::RegPins (prism.c:2627). A point in the
        // Prism's base Frame (== the current Frames of the two component
        // Regions, concatenated) lies on the Prism boundary iff it lies on
        // the boundary of one component AND on or within the boundary of the
        // other. The supplied `unc` is unused: C uses each component's own
        // base-Frame uncertainty (astGetUncFrm(reg, AST__BASE)), not the
        // Prism-level uncertainty passed in.
        //
        // C temporarily forces each component's Closed attribute to its
        // current value, promoting the unset default to closed=true; the Rust
        // Region default is already closed=true and explicitly-set values are
        // preserved, so that toggle is a no-op here.
        let reg1 = self.region1.as_ref();
        let reg2 = self.region2.as_ref();
        let nax1 = self.naxes1();
        let nax2 = self.naxes2();
        let np = pts.npoint();

        // Sub-PointSet holding just the axes for each component (astSetSubPoints
        // prism.c:2742/2761): the first nax1 coords for reg1, the next nax2 for
        // reg2.
        let ps1 = PointSet::from_rows((0..np).map(|ip| &pts.point(ip)[..nax1]));
        let ps2 = PointSet::from_rows((0..np).map(|ip| &pts.point(ip)[nax1..nax1 + nax2]));

        // Boundary mask of each component. astRegPins expects base-Frame
        // points, so map the sub-points current->base first, and take each
        // component's own base-Frame uncertainty.
        let ps1b = reg1.get_region_frameset().transform(&ps1, false)?;
        let unc1 = reg1.get_unc_frm(FrameRef::Base)?;
        let mask1 = reg1.reg_pins_mask(&ps1b, Some(unc1.as_ref()))?.1;
        // Membership: reg1 as a Mapping maps interior points through and sets
        // exterior points to BAD (prism.c:2756).
        let ps1b2 = reg1.transform(&ps1b, true)?;

        let ps2b = reg2.get_region_frameset().transform(&ps2, false)?;
        let unc2 = reg2.get_unc_frm(FrameRef::Base)?;
        let mask2 = reg2.reg_pins_mask(&ps2b, Some(unc2.as_ref()))?.1;
        let ps2b2 = reg2.transform(&ps2b, true)?;

        // Combine per prism.c:2778-2821.
        let mut mask = Vec::with_capacity(np);
        let mut result = true;
        for i in 0..np {
            let on = if mask1[i] {
                if mask2[i] {
                    // On the boundary of both components.
                    true
                } else {
                    // On reg1's boundary: on the Prism boundary iff within
                    // reg2's volume (no BAD axis through reg2).
                    !ps2b2.point(i)[..nax2].iter().any(|&v| is_bad(v))
                }
            } else if mask2[i] {
                // On reg2's boundary: on the Prism boundary iff within reg1's
                // volume.
                !ps1b2.point(i)[..nax1].iter().any(|&v| is_bad(v))
            } else {
                // On the boundary of neither component.
                false
            };
            mask.push(on);
            if !on {
                result = false;
            }
        }

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

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // C overrides the base-class method (prism.c:941): where the Prism's
        // base->current Mapping splits on the two components' axes, each
        // component is bounded in its own part of the current Frame and the
        // results are un-permuted into place. That avoids meshing the whole
        // Prism boundary, and keeps an unbounded component's bounds exact.
        let nax1 = self.naxes1();
        let nax2 = self.naxes2();

        let map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let cfrm = self.data.frameset.current().snapshot();

        // Split the Mapping on the base-Frame axes of each component
        // (prism.c:1057-1075). `map_split` takes and returns 1-based axes.
        let axin1: Vec<usize> = (1..=nax1).collect();
        let split1 = map.map_split(&axin1);
        let axin2: Vec<usize> = (nax1 + 1..=nax1 + nax2).collect();
        let split2 = map.map_split(&axin2);

        // Where either component's axes do not split out, the parent method is
        // used (prism.c:1085-1087).
        let (Some((caxes1, map1)), Some((caxes2, map2))) = (split1, split2) else {
            return super::region_bounds_current(self);
        };

        let nax1_out = map1.nout();
        let nax2_out = map2.nout();
        let target1: Vec<i32> = caxes1.iter().map(|&a| (a - 1) as i32).collect();
        let target2: Vec<i32> = caxes2.iter().map(|&a| (a - 1) as i32).collect();
        let cfrm1 = cfrm.pick_axes_snapshot(&target1)?;
        let cfrm2 = cfrm.pick_axes_snapshot(&target2)?;

        // Bound each component within its own part of the current Frame
        // (prism.c:1099-1109).
        let reg1 = self.region1.map_region(map1.as_ref(), cfrm1.as_ref())?;
        let (tlbnd1, tubnd1) = reg1.get_region_bounds()?;
        let reg2 = self.region2.map_region(map2.as_ref(), cfrm2.as_ref())?;
        let (tlbnd2, tubnd2) = reg2.get_region_bounds()?;

        // Undo any axis permutation the Prism's FrameSet introduced
        // (prism.c:1111-1120). Any current-Frame axis neither split reaches is
        // left as C leaves it: untouched.
        let ncur = cfrm.naxes();
        let mut lbnd = vec![0.0; ncur];
        let mut ubnd = vec![0.0; ncur];
        for iax in 0..nax1_out {
            lbnd[caxes1[iax] - 1] = tlbnd1[iax];
            ubnd[caxes1[iax] - 1] = tubnd1[iax];
        }
        for iax in 0..nax2_out {
            lbnd[caxes2[iax] - 1] = tlbnd2[iax];
            ubnd[caxes2[iax] - 1] = tubnd2[iax];
        }

        Ok((lbnd, ubnd))
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

    fn get_region_grid(&self) -> AstResult<PointSet> {
        crate::regions::data::region_base_grid(self)
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

    fn clear_region_fs_deep(&mut self) {
        // C copies both component Regions (prism.c:3737-3738), and each copy clears its own
        // `RegionFS` (region.c:12936-12940).
        self.set_region_fs_flag(true);
        if let Some(mut unc) = self.get_uncertainty() {
            unc.clear_region_fs_deep();
            self.store_uncertainty(Some(unc));
        }
        self.region1.clear_region_fs_deep();
        self.region2.clear_region_fs_deep();
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn get_def_unc(&self) -> AstResult<DynRegion> {
        // Port of prism.c::GetDefUnc (prism.c:1207): a Prism formed from the
        // two component uncertainty Regions. The components' current Frames are
        // equivalent to the Prism's base Frame, so their current-Frame
        // uncertainties combine directly into the default uncertainty.
        let bunc1 = self.region1.get_unc_frm(FrameRef::Current)?;
        let bunc2 = self.region2.get_unc_frm(FrameRef::Current)?;
        let prism = super::PrismBuilder::from_dyn(bunc1, bunc2)?.build()?;
        Ok(Box::new(prism))
    }

    fn reg_centre_query(&self, ifrm: FrameRef) -> AstResult<Option<Vec<f64>>> {
        // Port of prism.c::RegCentre query path (prism.c:2551-2580). The two
        // component Regions' current Frames together form the parent's base
        // Frame. Both components must be re-centreable; if either returns None,
        // the Prism has no centre.
        let cen1 = self.region1.reg_centre_query(FrameRef::Current)?;
        let cen2 = self.region2.reg_centre_query(FrameRef::Current)?;
        let (Some(cen1), Some(cen2)) = (cen1, cen2) else {
            return Ok(None);
        };

        let mut total = cen1;
        total.extend(cen2);

        // `total` is in the parent base Frame; transform to the current Frame
        // when requested (prism.c:2569-2570).
        match ifrm {
            FrameRef::Current => Ok(Some(self.reg_tran_point(&total, true)?)),
            _ => Ok(Some(total)),
        }
    }

    fn reg_centre_set(&mut self, cen: &[f64], ifrm: FrameRef) -> AstResult<()> {
        // Port of prism.c::RegCentre set path (prism.c:2582-2617). Transform the
        // supplied centre into the parent base Frame (the component current
        // Frames) when supplied in the current Frame, then hand each component
        // its slice.
        let bc = match ifrm {
            FrameRef::Current => self.reg_tran_point(cen, false)?,
            _ => cen.to_vec(),
        };

        let nax1 = self.region1.naxes();
        self.region1
            .reg_centre_set(&bc[..nax1], FrameRef::Current)?;
        self.region2
            .reg_centre_set(&bc[nax1..], FrameRef::Current)?;
        self.data.invalidate_cache();
        Ok(())
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        // Port of prism.c::RegBasePick (prism.c:2287). Partition the requested
        // base-Frame axes between the two encapsulated Regions at nax1, pick
        // the corresponding axes from each, and return the single sub-region
        // (when the selection falls entirely in one component) or a Prism of
        // both.
        let nax1 = self.region1.naxes();

        let mut axes1: Vec<usize> = Vec::new();
        let mut axes2: Vec<usize> = Vec::new();
        for &j in axes {
            if j < nax1 {
                axes1.push(j);
            } else {
                axes2.push(j - nax1);
            }
        }

        // Pick any required axes from the first sub-region. Only a Region
        // result is usable (prism.c:2400-2404).
        let frm1: Option<DynRegion> = if axes1.is_empty() {
            None
        } else {
            match super::pick_axes_region(self.region1.as_ref(), &axes1)? {
                super::PickedAxes::Region(r) => Some(r),
                super::PickedAxes::Frame(_) => None,
            }
        };

        // Pick any required axes from the second sub-region.
        let frm2: Option<DynRegion> = if axes2.is_empty() {
            None
        } else {
            match super::pick_axes_region(self.region2.as_ref(), &axes2)? {
                super::PickedAxes::Region(r) => Some(r),
                super::PickedAxes::Frame(_) => None,
            }
        };

        // Assemble the result per the C decision table (prism.c:2419-2434).
        let result: Option<DynRegion> = match (axes1.is_empty(), axes2.is_empty(), frm1, frm2) {
            (false, true, Some(r1), _) => Some(r1),
            (true, false, _, Some(r2)) => Some(r2),
            (false, false, Some(r1), Some(r2)) => {
                Some(Box::new(PrismBuilder::from_dyn(r1, r2)?.build()?))
            }
            _ => None,
        };

        Ok(result)
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for Prism {
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
        // Faithful port of prism.c::Simplify (prism.c:3204). Attempts to
        // express the Prism's two component Regions within the parent
        // Region's current Frame, collapsing the Prism to a single Region
        // (or a simpler Prism) where possible.
        use crate::mappings::{CmpMap, Composition};

        // GetRegions (prism.c:1137): clone the two component Regions and read
        // the Prism's Negated flag. The component current Frames are
        // equivalent to components of the parent base Frame.
        let reg1 = self.region1.clone_region();
        let reg2 = self.region2.clone_region();
        let neg = self.data.negated;

        // Get the current Frame and the base->current Mapping of the parent.
        let cfrm = self.data.frameset.current().snapshot();
        let bcmap = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;

        // Combine the two Regions into a parallel CmpMap (a Region is a
        // Mapping) and simplify it. This drives the per-class astMapMerge
        // methods to merge adjacent Regions (prism.c:3305).
        let cmpmap = CmpMap::new(
            reg1.clone_region(),
            reg2.clone_region(),
            Composition::Parallel,
        )?;
        let smap = <dyn Mapping>::simplify(&cmpmap)?;

        // Initially assume we cannot form a new Region from the simplified
        // CmpMap.
        let mut new: Option<DynRegion> = None;

        // If the result is a Region, use it (prism.c:3318).
        if let Some(region) = super::merge_util::mapping_into_region(smap.clone_box()) {
            new = Some(region);
        // If the result is a parallel CmpMap whose two components are both
        // Regions differing from the originals, form a new Prism from them
        // (prism.c:3324).
        } else if let Some(cm) = smap.as_any().downcast_ref::<CmpMap>()
            && cm.composition() == Composition::Parallel
            && let (Some(m1), Some(m2)) =
                (super::as_region(cm.first()), super::as_region(cm.second()))
        {
            // The C `(AstRegion *) map1 != reg1` pointer test guards against
            // rebuilding an identical Prism. Rust has value semantics, so
            // compare by structural equality of the component Regions.
            let same = (m1 as &dyn Mapping).equal(reg1.as_ref() as &dyn Mapping)
                && (m2 as &dyn Mapping).equal(reg2.as_ref() as &dyn Mapping);
            if !same {
                let prism =
                    PrismBuilder::from_dyn(m1.clone_region(), m2.clone_region())?.build()?;
                new = Some(Box::new(prism) as DynRegion);
            }
        }

        // If the above produced a simplified Region, map it into the current
        // Frame of the parent and simplify it (prism.c:3342).
        if let Some(region) = new.take() {
            let mapped = region.map_region(bcmap.as_ref(), cfrm.as_ref())?;
            let simplified = <dyn Mapping>::simplify(mapped.as_ref())?;
            // astSimplify of a Region always yields a Region; if the concrete
            // simplified form is not a plain Region class, keep the mapped
            // Region unchanged.
            new = Some(super::merge_util::mapping_into_region(simplified).unwrap_or(mapped));
        } else {
            // A different approach (prism.c:3349): use astMapSplit to see
            // whether each component's axes correspond to a distinct set of
            // current-Frame axes.
            let nax1 = reg1.naxes();
            let nax2 = reg2.naxes();

            // Split bcmap on the first component's axes (0..nax1) and the
            // second component's axes (nax1..nax1+nax2). map_split takes
            // 1-based input axes and returns 1-based output-axis indices.
            let axin1: Vec<usize> = (0..nax1).map(|i| i + 1).collect();
            let split1 = bcmap.map_split(&axin1);
            let axin2: Vec<usize> = (0..nax2).map(|i| nax1 + i + 1).collect();
            let split2 = bcmap.map_split(&axin2);

            // Assume the component Regions cannot be simplified: use a clone
            // of the supplied Prism (prism.c:3382).
            new = Some(self.clone_region());

            let naxout1 = split1.as_ref().map(|(_, m)| m.nout()).unwrap_or(0);
            let naxout2 = split2.as_ref().map(|(_, m)| m.nout()).unwrap_or(0);
            let naxt = bcmap.nout();

            // axout* are the zero-based current-Frame axes each component maps
            // to (map_split returns 1-based; decrement).
            let axout1: Vec<usize> = split1
                .as_ref()
                .map(|(o, _)| o.iter().map(|&a| a - 1).collect())
                .unwrap_or_default();
            let axout2: Vec<usize> = split2
                .as_ref()
                .map(|(o, _)| o.iter().map(|&a| a - 1).collect())
                .unwrap_or_default();

            if naxout1 == naxt && naxout2 == 0 {
                // The second component contributes no axes; ignore it
                // (prism.c:3401).
                let (_, nmap1) = split1.as_ref().unwrap();
                let target: Vec<i32> = axout1.iter().map(|&a| a as i32).collect();
                let newfrm1 = cfrm.pick_axes_snapshot(&target)?;
                let newreg1 = reg1.map_region(nmap1.as_ref(), newfrm1.as_ref())?;
                let mut simplified = super::merge_util::mapping_into_region(
                    <dyn Mapping>::simplify(newreg1.as_ref())?,
                )
                .unwrap_or(newreg1);
                if neg {
                    simplified.negate();
                }
                // reg_perm_axes uses 1-based indices (Rust convention); the
                // zero-based axout list is C's astPermAxes argument.
                let perm: Vec<usize> = axout1.iter().map(|&a| a + 1).collect();
                simplified.reg_perm_axes(&perm)?;
                new = Some(simplified);
            } else if naxout1 == 0 && naxout2 == naxt {
                // The first component contributes no axes; ignore it
                // (prism.c:3418).
                let (_, nmap2) = split2.as_ref().unwrap();
                let target: Vec<i32> = axout2.iter().map(|&a| a as i32).collect();
                let newfrm2 = cfrm.pick_axes_snapshot(&target)?;
                let newreg2 = reg2.map_region(nmap2.as_ref(), newfrm2.as_ref())?;
                let mut simplified = super::merge_util::mapping_into_region(
                    <dyn Mapping>::simplify(newreg2.as_ref())?,
                )
                .unwrap_or(newreg2);
                if neg {
                    simplified.negate();
                }
                let perm: Vec<usize> = axout2.iter().map(|&a| a + 1).collect();
                simplified.reg_perm_axes(&perm)?;
                new = Some(simplified);
            } else if let (Some((_, nmap1)), Some((_, nmap2))) = (&split1, &split2) {
                // Both components correspond to a distinct subspace of the
                // current Frame: express each within the current Frame
                // (prism.c:3436).
                let target1: Vec<i32> = axout1.iter().map(|&a| a as i32).collect();
                let newfrm1 = cfrm.pick_axes_snapshot(&target1)?;
                let newreg1 = reg1.map_region(nmap1.as_ref(), newfrm1.as_ref())?;
                let snewreg1_map = <dyn Mapping>::simplify(newreg1.as_ref())?;

                let target2: Vec<i32> = axout2.iter().map(|&a| a as i32).collect();
                let newfrm2 = cfrm.pick_axes_snapshot(&target2)?;
                let newreg2 = reg2.map_region(nmap2.as_ref(), newfrm2.as_ref())?;
                let snewreg2_map = <dyn Mapping>::simplify(newreg2.as_ref())?;

                // Detect whether either component actually simplified by
                // structural comparison, mirroring C's pointer inequality
                // `snewreg1 != newreg1 || snewreg2 != newreg2` (prism.c:3457).
                let changed1 = !snewreg1_map.equal(newreg1.as_ref() as &dyn Mapping);
                let changed2 = !snewreg2_map.equal(newreg2.as_ref() as &dyn Mapping);

                if changed1 || changed2 {
                    let snewreg1 = super::merge_util::mapping_into_region(snewreg1_map)
                        .unwrap_or_else(|| newreg1.clone_region());
                    let snewreg2 = super::merge_util::mapping_into_region(snewreg2_map)
                        .unwrap_or_else(|| newreg2.clone_region());
                    let prism = PrismBuilder::from_dyn(snewreg1, snewreg2)?.build()?;
                    let mut prism: DynRegion = Box::new(prism);
                    if neg {
                        prism.negate();
                    }
                    // Restore the original current-Frame axis order
                    // (prism.c:3466). reg_perm_axes uses 1-based indices.
                    let mut perm = vec![0usize; naxout1 + naxout2];
                    for i in 0..naxout1 {
                        perm[i] = axout1[i] + 1;
                    }
                    for i in 0..naxout2 {
                        perm[naxout1 + i] = axout2[i] + 1;
                    }
                    prism.reg_perm_axes(&perm)?;
                    new = Some(prism);
                }
            }
        }

        // Carry any explicitly-supplied uncertainty forward to the new Region
        // (prism.c:3494), then invoke the parent Region Simplify on it
        // (prism.c:3502) to simplify the encapsulated FrameSet and uncertainty
        // Region. The C split path always leaves `new` non-NULL (defaulting to
        // a clone of the supplied Prism), so the parent simplify is always run;
        // it returns the input unchanged when nothing simplifies.
        let mut result = match new {
            Some(mut region) => {
                if let Some(unc) = self.data.uncertainty.clone() {
                    region.set_uncertainty(unc);
                }
                match region.parent_region_simplify()? {
                    Some(simplified) => simplified,
                    None => region,
                }
            }
            None => self.clone_region(),
        };

        // C's `result != this_mapping` (prism.c:3513), the one test that decides
        // both the attribute overlay and whether anything reduced. Prism keeps no
        // `simpler` flag, so value semantics make it a structural comparison.
        // Overlay passes unc=0: uncertainty is carried forward explicitly above,
        // so it is left untouched here.
        let changed = !(result.as_ref() as &dyn Mapping).equal(self as &dyn Mapping);
        if changed {
            result.reg_overlay(self, false);
        }

        Ok((result.clone_box(), changed))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Prism>() else {
            return false;
        };
        // Mirror C prism.c::Equal (516): the parent Region check covers
        // class, Negated, Closed, and the current Frame; equivalence then
        // requires both component Regions to be equal.
        self.data.negated == other.data.negated
            && self.data.closed == other.data.closed
            && Frame::equal(self.data.frameset.current(), other.data.frameset.current())
            && (self.region1.as_ref() as &dyn Mapping).equal(other.region1.as_ref() as &dyn Mapping)
            && (self.region2.as_ref() as &dyn Mapping).equal(other.region2.as_ref() as &dyn Mapping)
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }

    fn map_list_decompose(&self, _inv: bool, series: bool) -> Option<Vec<(DynMapping, bool)>> {
        // Port of C `Prism::MapList` (prism.c:1527): a Prism, considered as a
        // CmpMap, always combines its two component Regions in *parallel*. So
        // only decompose for a parallel request; a series request leaves the
        // Prism atomic (C falls back to the parent MapList, appending it whole).
        // Each component is a Region — unaffected by the Invert flag — so C
        // appends both with invert=0.
        if series {
            return None;
        }
        Some(vec![
            ((self.region1.as_ref() as &dyn Mapping).clone_box(), false),
            ((self.region2.as_ref() as &dyn Mapping).clone_box(), false),
        ])
    }
}

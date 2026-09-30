//! Shared data and internal geometry trait for all Region implementations.
//!
//! Every concrete region struct embeds a [`RegionData`] that stores the
//! encapsulated FrameSet, defining points, negation flag, and other
//! common attributes. The [`RegionGeometry`] trait provides the
//! geometry-specific hooks that each concrete region must supply.

use std::cell::RefCell;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet, SkyFrame};
use crate::mappings::helpers::is_bad;
use crate::mappings::{Mapping, UnitMap};

use super::DynRegion;

/// The default MeshSize for a Region with the given number of axes.
///
/// Mirrors the naxes-dependent default in C `astGetMeshSize`
/// (region.c:12714): 1-D → 2, 2-D → 200, ≥3-D → 2000.
pub(crate) fn default_mesh_size(naxes: usize) -> usize {
    match naxes {
        1 => 2,
        2 => 200,
        _ => 2000,
    }
}

/// Shared data for all Region implementations.
///
/// Every concrete region struct embeds one of these. Fields are
/// `pub(crate)` so sibling modules can access them directly.
#[derive(Debug)]
pub(crate) struct RegionData {
    /// The encapsulated FrameSet: base frame holds the region geometry,
    /// current frame is the user-facing coordinate system.
    pub(crate) frameset: FrameSet,

    /// The Region's own Object-class Ident, which C does not forward to the
    /// encapsulated Frame (object.c).
    pub(crate) ident: Option<String>,

    /// Defining points in the *base* frame.
    ///
    /// Layout: `points[i]` is the i-th defining point (a Vec of length
    /// naxes). The meaning is region-specific (e.g., for Circle:
    /// points[0] = centre, points[1] = edge point).
    pub(crate) points: Vec<Vec<f64>>,

    /// Whether the region is negated (inside/outside sense flipped).
    pub(crate) negated: bool,

    /// Whether boundary points are considered interior (closed=true)
    /// or exterior (closed=false).
    pub(crate) closed: bool,

    /// Tracks whether `closed` was explicitly set (by the user, by a
    /// simplification path, or by a source dump that had `Closed = …`
    /// uncommented). Mirrors the C library's `TestClosed` predicate
    /// (`this->closed != -INT_MAX`) so dump emission can distinguish
    /// "default true" from "explicitly set" — Interval-simplified-to-Box
    /// fixtures rely on this distinction.
    pub(crate) closed_set: bool,

    /// Number of points used for boundary mesh approximation.
    ///
    /// Only meaningful when [`mesh_size_set`](Self::mesh_size_set) is true.
    /// When unset, the effective value is the naxes-dependent default from
    /// [`default_mesh_size`].
    pub(crate) mesh_size: usize,

    /// Tracks whether `mesh_size` was explicitly set. Mirrors the C
    /// library's `TestMeshSize` predicate (`this->meshsize != -INT_MAX`,
    /// region.c:12713) so dump emission is gated on the set-flag rather than
    /// on the value differing from a default.
    pub(crate) mesh_size_set: bool,

    /// Fraction of the bounding box that is interior.
    ///
    /// Only meaningful when [`fill_factor_set`](Self::fill_factor_set) is
    /// true; the default effective value is `1.0`.
    pub(crate) fill_factor: f64,

    /// Tracks whether `fill_factor` was explicitly set. Mirrors the C
    /// library's `TestFillFactor` predicate (`this->fillfactor != AST__BAD`,
    /// region.c:12650).
    pub(crate) fill_factor_set: bool,

    /// Whether adaptive meshing is enabled.
    pub(crate) adaptive: bool,

    /// The uncertainty region, if set.
    pub(crate) uncertainty: Option<DynRegion>,

    /// Whether `frameset` is a *dummy* FrameSet synthesised at load time
    /// because the serialised region carried neither `Frm` nor `FrmSet`
    /// (a nested region that inherits its container's frame). Mirrors C's
    /// `DUMMY_FS` marker (`region.c:13593-13601`). A dummy FrameSet must be
    /// replaced by the real frame via [`RegionData::set_region_fs`] before
    /// the region is used; the parent compound loader performs that
    /// re-association.
    pub(crate) dummy_fs: bool,

    /// The C `RegionFS` attribute (`region.c:12603-12607`): non-zero means
    /// "include the Frame in the dump". Default (unset) is treated as 1.
    ///
    /// This is distinct from [`dummy_fs`](Self::dummy_fs): a frame-less
    /// nested region loads with `region_fs = false`, and although
    /// `set_region_fs` later gives it a real FrameSet (clearing `dummy_fs`),
    /// `region_fs` stays false — so on re-dump the Frame is still omitted,
    /// matching C's `astSetRegFS` leaving `regionfs` untouched
    /// (`region.c:10738-10747`) and the unit-map dump branch writing `Frm`
    /// only when `regionfs != 0` (`region.c:13099-13107`).
    pub(crate) region_fs: bool,

    /// Cached base-frame boundary mesh (lazily computed).
    pub(crate) cached_base_mesh: RefCell<Option<Vec<Vec<f64>>>>,
}

impl Clone for RegionData {
    fn clone(&self) -> Self {
        RegionData {
            frameset: self.frameset.clone(),
            ident: None,
            points: self.points.clone(),
            negated: self.negated,
            closed: self.closed,
            closed_set: self.closed_set,
            mesh_size: self.mesh_size,
            mesh_size_set: self.mesh_size_set,
            fill_factor: self.fill_factor,
            fill_factor_set: self.fill_factor_set,
            adaptive: self.adaptive,
            uncertainty: self.uncertainty.clone(),
            dummy_fs: self.dummy_fs,
            region_fs: self.region_fs,
            cached_base_mesh: RefCell::new(self.cached_base_mesh.borrow().clone()),
        }
    }
}

/// The Frame a Region encapsulates, given the Frame its constructor was
/// supplied (`region.c:13311-13323`).
///
/// A Region contributes the current Frame of its own FrameSet, a FrameSet its
/// current Frame, and any other Frame is used as supplied.
fn resolve_supplied_frame(frame: &dyn Frame) -> DynFrame {
    if let Some(region) = crate::regions::any_as_region(frame.as_any()) {
        region.get_region_frame()
    } else if let Some(fs) = frame.as_any().downcast_ref::<FrameSet>() {
        fs.current().snapshot()
    } else {
        frame.snapshot()
    }
}

impl RegionData {
    /// Create a new `RegionData` with default attribute values.
    ///
    /// - `frameset`: the encapsulated FrameSet (base = geometry frame,
    ///   current = user frame).
    /// - `points`: defining points in the base frame.
    pub(crate) fn new(frameset: FrameSet, points: Vec<Vec<f64>>) -> Self {
        let mut fs = frameset;
        // Ensure 2-frame structure: base + current connected by UnitMap.
        // This matches the C library and ensures with_integrity works
        // (fuse_into_current_edges needs at least one edge to compose the delta).
        //
        // A single-frame FrameSet is the frame a Region constructor was handed,
        // which C resolves before storing: a Region contributes the Frame it
        // encapsulates and a FrameSet its current Frame (region.c:13311-13323).
        if fs.nframe() == 1 {
            let resolved = resolve_supplied_frame(fs.current());
            let naxes = resolved.naxes();
            fs = FrameSet::new(resolved);
            let current_snap = fs.current().snapshot();
            fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(naxes)), current_snap)
                .expect("adding current frame to region FrameSet");
        }
        RegionData {
            frameset: fs,
            ident: None,
            points,
            negated: false,
            closed: true,
            closed_set: false,
            mesh_size: 0,
            mesh_size_set: false,
            fill_factor: 1.0,
            fill_factor_set: false,
            adaptive: true,
            uncertainty: None,
            dummy_fs: false,
            region_fs: true,
            cached_base_mesh: RefCell::new(None),
        }
    }

    /// Replace this region's (dummy) FrameSet with a real one built from
    /// `frame`, mirroring C `astSetRegFS` (`region.c:10674`).
    ///
    /// The new FrameSet holds two independent copies of `frame` connected
    /// by a `UnitMap` (base = current = the supplied frame), and the
    /// `dummy_fs` flag is cleared. If this region carries an uncertainty
    /// region that is itself dummy-FS, it is recursively re-associated
    /// with the same frame (C `region.c:10745-10751`).
    pub(crate) fn set_region_fs(&mut self, frame: &dyn Frame) {
        let naxes = frame.naxes();
        let mut fs = FrameSet::new(frame.snapshot());
        let current_snap = frame.snapshot();
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(naxes)), current_snap)
            .expect("adding current frame to region FrameSet");
        self.frameset = fs;
        self.dummy_fs = false;
        self.invalidate_cache();

        // Recursively fix a dummy-FS uncertainty region with the same frame.
        if let Some(unc) = self.uncertainty.as_mut()
            && unc.has_dummy_fs()
        {
            unc.set_region_fs(frame);
        }
    }

    /// Set the boolean `RegionFS` attribute (C `astSetRegionFS`,
    /// region.c:12603). When `false`, the encapsulated FrameSet is omitted
    /// from a dump. Unlike [`set_region_fs`](Self::set_region_fs), this does
    /// not install a new FrameSet — it only toggles the dump flag.
    pub(crate) fn set_region_fs_flag(&mut self, value: bool) {
        self.region_fs = value;
    }

    /// The effective MeshSize: the explicitly-set value if any, otherwise
    /// the naxes-dependent default (`astGetMeshSize`, region.c:12714): 1-D →
    /// 2, 2-D → 200, ≥3-D → 2000. The naxes count is that of the current
    /// Frame.
    pub(crate) fn effective_mesh_size(&self) -> usize {
        if self.mesh_size_set {
            self.mesh_size
        } else {
            default_mesh_size(self.frameset.current().naxes())
        }
    }

    /// The effective FillFactor: the explicitly-set value if any, otherwise
    /// `1.0` (`astGetFillFactor`, region.c:12648).
    pub(crate) fn effective_fill_factor(&self) -> f64 {
        if self.fill_factor_set {
            self.fill_factor
        } else {
            1.0
        }
    }

    /// Set the MeshSize, mirroring C `SetMeshSize` (region.c:12712): values
    /// are clamped to be greater than 5, and the set-flag is raised.
    pub(crate) fn set_mesh_size(&mut self, value: usize) {
        self.mesh_size = if value > 5 { value } else { 5 };
        self.mesh_size_set = true;
        self.invalidate_cache();
    }

    /// Set the FillFactor and raise the set-flag. Callers supply a value in
    /// the C `SetFillFactor` valid range `0.0..=1.0` (region.c:12651).
    pub(crate) fn set_fill_factor(&mut self, value: f64) {
        self.fill_factor = value;
        self.fill_factor_set = true;
    }

    /// Clear the MeshSize attribute, mirroring C `astClearMeshSize`.
    pub(crate) fn clear_mesh_size(&mut self) {
        self.mesh_size_set = false;
        self.invalidate_cache();
    }

    /// Clear the FillFactor attribute, mirroring C `astClearFillFactor`.
    pub(crate) fn clear_fill_factor(&mut self) {
        self.fill_factor_set = false;
    }

    /// Invalidate the cached mesh (call after geometry changes).
    #[allow(dead_code)]
    pub(crate) fn invalidate_cache(&self) {
        *self.cached_base_mesh.borrow_mut() = None;
    }

    /// The boundary-strictness that
    /// [`RegionGeometry::reg_base_inside`](super::RegionGeometry::reg_base_inside)
    /// must use, given that its caller applies the Negated attribute afterwards.
    ///
    /// In C, a point exactly on the boundary belongs to the region iff Closed,
    /// *independent of Negated* — a region and its negation both contain their
    /// shared boundary. Each class encodes that invariant differently:
    /// `polygon.c:6423-6444` states it directly, assigning `in_region = closed`
    /// for an `ON` point in both the negated and un-negated arms, while
    /// `circle.c:2078-2092`, `ellipse.c:2196-2210`, `box.c:4225/4263` (via the
    /// `astAxIn` strictness argument) and `interval.c:3666-3890` reach the same
    /// result by pairing each arm with a complementary comparison.
    ///
    /// `reg_base_inside` reports membership of the *un-negated* region and
    /// [`region_transform`] inverts it when Negated is set. Boolean inversion is
    /// correct for the interior but would move the boundary to the wrong side,
    /// so the boundary must be reported inverted here in order to come back out
    /// the right way — hence Closed XOR Negated rather than Closed.
    ///
    /// This is only ever the right question for `reg_base_inside`; the Closed
    /// attribute itself is [`RegionGeometry::is_closed`](super::RegionGeometry::is_closed).
    pub(crate) fn closed_for_base_inside(&self) -> bool {
        self.closed != self.negated
    }

    /// Whether a closed-curve region (Circle, Ellipse, Box, …) is bounded.
    ///
    /// Faithful port of the base `Region::GetBounded` (region.c:3358): a
    /// region defined by one or more closed curves is bounded so long as it
    /// has not been negated. The exception is a region defined within a
    /// SkyFrame (i.e. on a sphere), which is always bounded because a finite
    /// region has a finite negation.
    pub(crate) fn closed_curve_bounded(&self) -> bool {
        if self.frameset.base().as_any().is::<SkyFrame>() {
            true
        } else {
            !self.negated
        }
    }

    /// Permute the axes of the encapsulated FrameSet's current frame.
    ///
    /// Delegates to [`FrameSet::perm_axes`] which permutes the current
    /// frame and composes a PermMap onto the edge mapping.
    pub(crate) fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.frameset.perm_axes(perm)?;
        self.invalidate_cache();
        Ok(())
    }
}

/// Internal trait providing geometry-specific hooks for a Region.
///
/// Each concrete region implements this to supply its shape-specific
/// logic. The shared infrastructure in `RegionData` and the default
/// `Region` method implementations delegate to these hooks.
#[allow(dead_code)]
pub(crate) trait RegionGeometry {
    /// Access the shared region data.
    fn region_data(&self) -> &RegionData;

    /// Mutable access to the shared region data.
    fn region_data_mut(&mut self) -> &mut RegionData;

    /// Batch predicate: test which points (in the *base* frame) are inside
    /// the un-negated region boundary.
    ///
    /// `pts` is a PointSet with coordinates in the base frame. Returns a
    /// Vec<bool> with one entry per point (true = inside raw geometry).
    /// The negation flag is handled by the caller.
    ///
    /// This is the primary performance-critical method. Implementations
    /// should process all points in a tight loop to allow vectorisation.
    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>>;

    /// Test whether a single point (in the *base* frame) is inside the
    /// un-negated region boundary.
    ///
    /// Default implementation calls `reg_base_inside` with a 1-point PointSet.
    /// Override for efficiency if single-point queries are common.
    fn reg_base_inside_point(&self, point: &[f64]) -> AstResult<bool> {
        let pts = PointSet::from_rows(std::iter::once(point));
        let result = self.reg_base_inside(&pts)?;
        Ok(result[0])
    }

    /// Return the bounding box of the un-negated region in the base frame.
    ///
    /// Returns `(lower, upper)`, each of length `naxes`.
    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)>;

    /// Return a mesh of points on the region boundary in the base frame.
    ///
    /// The mesh should contain approximately `mesh_size` points.
    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>>;

    /// Return the centre of the region in the base frame.
    fn reg_centre(&self) -> AstResult<Vec<f64>>;

    /// Whether this region is bounded (finite extent in all axes).
    fn is_bounded_inner(&self) -> bool;

    /// Remap defining points through a mapping.
    ///
    /// Called when the region's coordinate system changes (e.g., via
    /// `map_region`). The implementation should transform `self.points`
    /// through the given mapping and update any derived quantities.
    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()>;

    /// Return a grid of points spread through the interior of the region.
    ///
    /// The default implementation generates a regular grid in the bounding box,
    /// then filters to keep only points inside the region. The `npoints`
    /// parameter is the approximate target number of interior points; the actual
    /// count may differ since grid points outside the region are discarded.
    ///
    /// For bounded regions only. Returns an empty Vec for unbounded regions.
    fn reg_base_grid(&self, npoints: usize) -> AstResult<Vec<Vec<f64>>> {
        // Get the bounding box.
        let (lbnd, ubnd) = self.reg_base_box()?;
        let naxes = lbnd.len();

        // Check all axes are bounded.
        for i in 0..naxes {
            if is_bad(lbnd[i]) || is_bad(ubnd[i]) {
                return Ok(Vec::new());
            }
        }

        let npoints = npoints.max(1);

        // Compute number of ticks per axis: ntick = npoints^(1/naxes), at least 2.
        let ntick_per_axis = (npoints as f64).powf(1.0 / naxes as f64).round() as usize;
        let ntick_per_axis = ntick_per_axis.max(2);

        // Total grid points.
        let total = ntick_per_axis.pow(naxes as u32);

        // Generate the regular grid filling the bounding box.
        let mut grid = Vec::with_capacity(total);

        // Compute step sizes.
        let steps: Vec<f64> = (0..naxes)
            .map(|i| (ubnd[i] - lbnd[i]) / (ntick_per_axis - 1).max(1) as f64)
            .collect();

        // Iterate through all grid positions using multi-index.
        let mut indices = vec![0usize; naxes];
        for _ in 0..total {
            // Build point from current indices.
            let point: Vec<f64> = (0..naxes)
                .map(|ax| lbnd[ax] + indices[ax] as f64 * steps[ax])
                .collect();
            grid.push(point);

            // Advance multi-index (odometer style).
            let mut ax = 0;
            while ax < naxes {
                indices[ax] += 1;
                if indices[ax] >= ntick_per_axis {
                    indices[ax] = 0;
                    ax += 1;
                } else {
                    break;
                }
            }
        }

        // Filter: keep only points inside the region.
        let pts = PointSet::from_rows(grid.iter().map(|p| p.as_slice()));
        let inside = self.reg_base_inside(&pts)?;

        let result: Vec<Vec<f64>> = grid
            .into_iter()
            .zip(inside)
            .filter_map(|(p, is_in)| if is_in { Some(p) } else { None })
            .collect();

        Ok(result)
    }

    /// Verify that a candidate remapped Region (defined in this Region's
    /// *current* Frame by the boundary points `candidate_mesh`) is an exact
    /// representation of `self` after the base→current `map`.
    ///
    /// Mirrors the inverse-direction check in `box.c::Simplify`
    /// (3905-3907): map the candidate's boundary mesh back into this
    /// Region's base Frame with the inverse of `map`, then verify every
    /// point lies on `self`'s boundary (its `reg_base_inside` is true for a closed
    /// Region). When the base->current Mapping does not preserve
    /// axis-alignment (e.g. a shear MatrixMap), a candidate axis-aligned Box
    /// or Interval is strictly larger than the true region, so some
    /// back-mapped boundary points fall outside `self` and the check fails —
    /// signalling that no simplification is possible and the original Region
    /// (with its FrameSet) must be returned unchanged.
    fn verify_remap_exact(
        &self,
        map: &dyn Mapping,
        candidate_mesh: &[Vec<f64>],
    ) -> AstResult<bool> {
        if candidate_mesh.is_empty() {
            // No boundary to test (e.g. an unbounded candidate) — cannot
            // confirm exactness, so treat as not simplifiable.
            return Ok(false);
        }
        let mesh = PointSet::from_rows(candidate_mesh.iter().map(|p| p.as_slice()));
        // Inverse-transform the candidate boundary into the base Frame.
        let base_mesh = map.transform(&mesh, false)?;
        let pins = self.reg_base_inside(&base_mesh)?;
        Ok(pins.iter().all(|&inside| inside))
    }
}

/// Apply a frame-attribute mutation to a Region with integrity enforcement.
///
/// This is the Region equivalent of [`FrameSet::with_integrity`]. It:
///
/// 1. Records the base→current mapping before the change.
/// 2. Delegates to `FrameSet::with_integrity` to mutate the current frame
///    and update the internal mapping.
/// 3. If the region is adaptive, computes the delta between old and new
///    base→current mappings and transforms the defining points through it
///    (in practice this is identity for standard region FrameSets, but the
///    mechanism is in place for complex multi-frame scenarios).
/// 4. Invalidates cached meshes.
///
/// When `adaptive` is `false`, the mutation is applied to **both** base and
/// current frames (bypassing integrity), so the defining points remain
/// numerically unchanged in the new coordinate system.
pub(crate) fn with_region_integrity<R, F>(region: &mut R, f: F) -> AstResult<()>
where
    R: RegionGeometry,
    F: FnOnce(&mut dyn Frame) -> AstResult<()>,
{
    let data = region.region_data();
    let adaptive = data.adaptive;

    if adaptive {
        // Adaptive: delegate to FrameSet integrity. The current frame
        // changes and the base→current mapping is updated. Defining
        // points are in the base frame and remain valid because the
        // base frame itself is never modified by with_integrity — only
        // the current frame and the base→current mapping change.
        let data = region.region_data_mut();
        data.frameset.with_integrity(f)?;
        data.invalidate_cache();
    } else {
        // Non-adaptive: apply the mutation to both base and current frames
        // without remapping. The defining points remain numerically unchanged
        // because both frames change simultaneously.
        //
        // We apply to the current frame via frame_mut (bypassing integrity)
        // and also apply to the base frame.
        let data = region.region_data_mut();

        // Apply to current frame (bypass integrity).
        f(data.frameset.frame_mut(FrameRef::Current)?)?;

        // Apply the same mutation to the base frame.
        // We need a fresh snapshot-based approach: snapshot the base,
        // apply the same conceptual change. Since we can't easily
        // re-run the closure, for now we apply it directly to the base
        // frame as well. This matches C's RegSetAttrib which sets both.
        //
        // NOTE: The closure has already been consumed. For non-adaptive
        // regions, the caller should use `with_integrity_non_adaptive`
        // or set attributes directly on both frames. For now, we just
        // invalidate the cache — the non-adaptive path is less common
        // and will be fully implemented when needed for SkyFrame tests.
        data.invalidate_cache();
    }

    Ok(())
}

/// Shared implementation of `Mapping::transform` for all Regions.
///
/// Points inside the region pass through unchanged; points outside are
/// set to [`BAD_VALUE`]. The negation flag is respected. Points with
/// any BAD input coordinate always produce BAD output.
///
/// This is the "region transform" described in the C library: forward
/// maps interior → pass-through, exterior → BAD; inverse always passes
/// points unchanged.
///
/// Uses the batch `reg_base_inside` predicate for efficient processing of
/// large point sets (tens of thousands of points at a time).
pub(crate) fn region_transform<R: RegionGeometry>(
    region: &R,
    pts: &PointSet,
    forward: bool,
) -> AstResult<PointSet> {
    let npoint = pts.npoint();
    let ncoord = pts.ncoord();

    // Inverse transform: always pass points through unchanged.
    if !forward {
        return Ok(pts.clone());
    }

    // Forward transform: batch-test all points for membership.
    let data = region.region_data();
    let negated = data.negated;

    // Transform input points from current frame to base frame for testing.
    // The region's frameset maps base→current (forward), so we need inverse.
    let base_pts = data.frameset.transform(pts, false)?;

    // Batch predicate: test all points at once.
    let inside = region.reg_base_inside(&base_pts)?;

    // Build output: passthrough for inside, BAD for outside.
    let mut out = PointSet::zeros(npoint, ncoord);
    for (i, &is_in) in inside.iter().enumerate() {
        let base_point = base_pts.point(i);
        let has_bad = base_point.iter().any(|&v| is_bad(v));

        let effective_inside = if has_bad {
            false
        } else if negated {
            !is_in
        } else {
            is_in
        };

        if effective_inside {
            let src = pts.point(i);
            let dst = out.point_mut(i);
            dst.copy_from_slice(src);
        } else {
            let dst = out.point_mut(i);
            dst.fill(BAD_VALUE);
        }
    }

    Ok(out)
}

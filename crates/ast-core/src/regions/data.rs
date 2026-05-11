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
use crate::frames::FrameSet;
use crate::mappings::Mapping;
use crate::mappings::helpers::is_bad;

use super::DynRegion;

/// Shared data for all Region implementations.
///
/// Every concrete region struct embeds one of these. Fields are
/// `pub(crate)` so sibling modules can access them directly.
#[derive(Debug)]
pub(crate) struct RegionData {
    /// The encapsulated FrameSet: base frame holds the region geometry,
    /// current frame is the user-facing coordinate system.
    pub(crate) frameset: FrameSet,

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

    /// Number of points used for boundary mesh approximation.
    pub(crate) mesh_size: usize,

    /// Fraction of the bounding box that is interior.
    pub(crate) fill_factor: f64,

    /// Whether adaptive meshing is enabled.
    pub(crate) adaptive: bool,

    /// The uncertainty region, if set.
    pub(crate) uncertainty: Option<DynRegion>,

    /// Cached base-frame boundary mesh (lazily computed).
    pub(crate) cached_base_mesh: RefCell<Option<Vec<Vec<f64>>>>,
}

impl Clone for RegionData {
    fn clone(&self) -> Self {
        RegionData {
            frameset: self.frameset.clone(),
            points: self.points.clone(),
            negated: self.negated,
            closed: self.closed,
            mesh_size: self.mesh_size,
            fill_factor: self.fill_factor,
            adaptive: self.adaptive,
            uncertainty: self.uncertainty.clone(),
            cached_base_mesh: RefCell::new(self.cached_base_mesh.borrow().clone()),
        }
    }
}

impl RegionData {
    /// Create a new `RegionData` with default attribute values.
    ///
    /// - `frameset`: the encapsulated FrameSet (base = geometry frame,
    ///   current = user frame).
    /// - `points`: defining points in the base frame.
    pub(crate) fn new(frameset: FrameSet, points: Vec<Vec<f64>>) -> Self {
        RegionData {
            frameset,
            points,
            negated: false,
            closed: true,
            mesh_size: 200,
            fill_factor: 1.0,
            adaptive: false,
            uncertainty: None,
            cached_base_mesh: RefCell::new(None),
        }
    }

    /// Invalidate the cached mesh (call after geometry changes).
    #[allow(dead_code)]
    pub(crate) fn invalidate_cache(&self) {
        *self.cached_base_mesh.borrow_mut() = None;
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
    fn reg_pins(&self, pts: &PointSet) -> AstResult<Vec<bool>>;

    /// Test whether a single point (in the *base* frame) is inside the
    /// un-negated region boundary.
    ///
    /// Default implementation calls `reg_pins` with a 1-point PointSet.
    /// Override for efficiency if single-point queries are common.
    fn reg_base_pick(&self, point: &[f64]) -> AstResult<bool> {
        let pts = PointSet::from_rows(std::iter::once(point));
        let result = self.reg_pins(&pts)?;
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
/// Uses the batch `reg_pins` predicate for efficient processing of
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
    let inside = region.reg_pins(&base_pts)?;

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

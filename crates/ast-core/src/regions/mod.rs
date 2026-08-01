//! The Region trait and associated types.
//!
//! A Region represents an area within a coordinate frame. It combines
//! the [`Frame`] trait (describing the coordinate system) with the
//! [`Mapping`] trait (transforming points via the inside/outside
//! predicate: interior points pass through, exterior points become
//! [`BAD_VALUE`](crate::constants::BAD_VALUE)).

pub(crate) mod data;
pub mod masking;
pub(crate) mod overlap;

mod box_region;
mod circle;
mod cmpregion;
mod ellipse;
mod interval;
mod merge_util;
mod nullregion;
mod pointlist;
mod polygon;
mod prism;
pub use box_region::{BoxRegion, BoxRegionBuilder};
pub use circle::{Circle, CircleBuilder};
pub use cmpregion::{CmpRegion, CmpRegionBuilder};
pub use ellipse::{Ellipse, EllipseBuilder};
pub use interval::{Interval, IntervalBuilder};
pub use nullregion::{NullRegion, NullRegionBuilder};
pub use pointlist::{PointList, PointListBuilder};
pub use polygon::{Polygon, PolygonBuilder};
pub use prism::{Prism, PrismBuilder};

pub use masking::mask;

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::frames::{DynFrame, Frame, FrameSet};
use crate::mappings::{CmpMap, DynMapping, Mapping, UnitMap};

/// An owned, dynamically-dispatched Region.
pub type DynRegion = Box<dyn Region>;

/// Result of comparing two regions for overlap.
///
/// These values mirror the C library's `AST__OVERLAP_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlap {
    /// No overlap between the regions (neither contains any points of the other).
    None,
    /// The first region is entirely inside the second.
    FirstInside,
    /// The second region is entirely inside the first.
    SecondInside,
    /// Partial overlap (some points of each lie inside the other).
    Partial,
    /// The regions are identical (or equivalent after negation).
    Same,
}

/// Comparison operator for combining regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOperator {
    /// Boolean AND: interior is the intersection.
    And,
    /// Boolean OR: interior is the union.
    Or,
}

/// A region of a coordinate frame.
///
/// Regions implement both [`Frame`] (they describe a coordinate system)
/// and [`Mapping`] (they transform points via the inside/outside
/// predicate). A forward transform passes interior points unchanged
/// and sets exterior points to [`BAD_VALUE`](crate::constants::BAD_VALUE);
/// the inverse always passes points unchanged.
///
/// # Negation
///
/// A negated region inverts the inside/outside sense: points that were
/// interior become exterior and vice-versa. Negation is a lightweight
/// flag — it does not recompute geometry.
///
/// # Closed vs Open
///
/// A closed region (the default) includes boundary points; an open
/// region excludes them. For most practical purposes with floating-point
/// coordinates, boundary membership is academic.
pub trait Region: Frame + Mapping {
    // ------------------------------------------------------------------
    // Point membership
    // ------------------------------------------------------------------

    /// Test whether a single point is inside this region.
    ///
    /// `point` must have length equal to `self.naxes()`. The coordinates
    /// are in the current frame of the region's encapsulated FrameSet.
    /// Returns `true` if the point is inside (respecting negation).
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool>;

    /// Determine the overlap between this region and another.
    ///
    /// Both regions must have the same number of axes. The method
    /// accounts for any difference in coordinate frame between the two
    /// regions by aligning them before comparison.
    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap>;

    // ------------------------------------------------------------------
    // Negation
    // ------------------------------------------------------------------

    /// Return a deep-copy of this region with the negation flag toggled.
    fn negated_copy(&self) -> DynRegion;

    /// Toggle the negation flag in place.
    fn negate(&mut self);

    /// Whether this region is currently negated.
    fn is_negated(&self) -> bool;

    // ------------------------------------------------------------------
    // Attributes: Closed, FillFactor, MeshSize, Bounded, Adaptive
    // ------------------------------------------------------------------

    /// Whether boundary points are included (closed) or excluded (open).
    fn is_closed(&self) -> bool;

    /// Set the closed/open flag.
    fn set_closed(&mut self, closed: bool);

    /// The fill factor (fraction of the bounding box that is interior).
    ///
    /// Used by the mesh and overlap algorithms to estimate point density.
    /// Defaults to 1.0 (the region fills its bounding box).
    fn fill_factor(&self) -> f64;

    /// Set the fill factor.
    fn set_fill_factor(&mut self, factor: f64);

    /// The number of points used to represent the boundary mesh.
    fn mesh_size(&self) -> usize;

    /// Set the boundary mesh size.
    fn set_mesh_size(&mut self, size: usize);

    /// Whether this region is bounded (has finite extent in all directions).
    fn is_bounded(&self) -> bool;

    /// Whether adaptive meshing is enabled.
    fn is_adaptive(&self) -> bool;

    /// Set the adaptive meshing flag.
    fn set_adaptive(&mut self, adaptive: bool);

    /// Whether this region currently carries a *dummy* FrameSet, i.e. it
    /// was deserialised without its own `Frm`/`FrmSet` and inherits the
    /// frame of its container. Mirrors C `astRegDummyFS`
    /// (`region.c:8874`). The parent compound loader must re-associate the
    /// real frame via [`Region::set_region_fs`] before use.
    fn has_dummy_fs(&self) -> bool;

    /// Replace this region's (dummy) FrameSet with a real one built from
    /// `frame`, clearing the dummy-FS flag. Mirrors C `astSetRegFS`
    /// (`region.c:10674`). Also re-associates a dummy-FS uncertainty
    /// region with the same frame.
    fn set_region_fs(&mut self, frame: &dyn Frame);

    // ------------------------------------------------------------------
    // Geometry queries
    // ------------------------------------------------------------------

    /// Get the bounding box of this region in the base frame.
    ///
    /// Returns `(lower, upper)` where each is a vector of length `naxes()`.
    /// Axes that are unbounded use [`BAD_VALUE`](crate::constants::BAD_VALUE).
    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)>;

    /// Get a bounding disc for this region.
    ///
    /// Returns `(centre, radius)` where `centre` has length `naxes()`
    /// and `radius` is a single scalar angular radius (for sky regions)
    /// or Cartesian distance.
    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)>;

    /// Get a mesh of points on the boundary of this region.
    ///
    /// The mesh is in the base frame of the encapsulated FrameSet.
    /// The number of points is approximately [`mesh_size()`](Region::mesh_size).
    fn get_region_mesh(&self) -> AstResult<PointSet>;

    /// Get a grid of points filling the interior of this region.
    ///
    /// Returns approximately `npoints` points spread through the volume of
    /// this region in the base frame. Used by compound regions (e.g. Prism)
    /// to build cross-product meshes.
    ///
    /// The default implementation generates a regular grid in the bounding box
    /// and filters to keep only interior points. Returns an empty Vec for
    /// unbounded regions.
    fn get_region_grid(&self, npoints: usize) -> AstResult<Vec<Vec<f64>>>;

    /// Get the defining points of this region in the base frame.
    ///
    /// The meaning depends on the concrete region type (e.g., centre +
    /// edge for Circle, corners for Box).
    fn get_region_points(&self) -> AstResult<PointSet>;

    // ------------------------------------------------------------------
    // Frame access
    // ------------------------------------------------------------------

    /// Get a copy of the current frame of this region's encapsulated FrameSet.
    fn get_region_frame(&self) -> DynFrame;

    /// Get a copy of the encapsulated FrameSet (base → current).
    fn get_region_frameset(&self) -> FrameSet;

    // ------------------------------------------------------------------
    // Uncertainty
    // ------------------------------------------------------------------

    /// Set the uncertainty region associated with boundary positions.
    ///
    /// The uncertainty region defines the positional uncertainty of
    /// points on the boundary. It must have the same number of axes
    /// as this region and be centred at the origin.
    fn set_uncertainty(&mut self, unc: DynRegion);

    /// Get a copy of the uncertainty region, if one has been set.
    fn get_uncertainty(&self) -> Option<DynRegion>;

    // ------------------------------------------------------------------
    // Remapping
    // ------------------------------------------------------------------

    /// Map this region into a new frame.
    ///
    /// The `mapping` transforms coordinates from this region's current
    /// frame to the new `frame`. Returns a new region representing the
    /// same area in the new coordinate system.
    fn map_region(&self, mapping: &dyn Mapping, frame: &dyn Frame) -> AstResult<DynRegion>;

    // ------------------------------------------------------------------
    // Axis selection (PickAxes / RegBasePick)
    // ------------------------------------------------------------------

    /// Return a Region formed by picking selected *base* Frame axes from
    /// this Region, or `None` if no same-class sub-region can be formed.
    ///
    /// `axes` holds zero-based base-Frame axis indices. Mirrors C
    /// `astRegBasePick` (region.c:13679 default → `None`); classes that can
    /// split their defining geometry per-axis (Box, Interval, PointList,
    /// NullRegion) override this. Classes whose geometry cannot be sliced
    /// axis-wise (Circle, Ellipse, Polygon, CmpRegion, Prism) use the
    /// default and return `None`.
    fn reg_base_pick_axes(&self, _axes: &[usize]) -> AstResult<Option<DynRegion>> {
        Ok(None)
    }

    /// Copy selected properties from `that` onto this Region.
    ///
    /// Mirrors C `astRegOverlay` (region.c:9861): copies Negated, Closed,
    /// and Adaptive; copies MeshSize/FillFactor only when the axis counts
    /// match. The `unc` flag controls uncertainty clearing (unused here as
    /// the picked regions carry their own uncertainty).
    fn reg_overlay(&mut self, that: &dyn Region) {
        if self.is_negated() != that.is_negated() {
            self.negate();
        }
        self.set_closed(that.is_closed());
        self.set_adaptive(that.is_adaptive());
        if self.naxes() == that.naxes() {
            self.set_mesh_size(that.mesh_size());
            self.set_fill_factor(that.fill_factor());
        }
    }

    /// Permute the axes of this region's current Frame, updating the
    /// encapsulated FrameSet and defining points to preserve the region
    /// integrity invariant. Mirrors C `astPermAxes` applied to a Region.
    ///
    /// `perm` holds zero-based target indices, one per current axis.
    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()>;

    // ------------------------------------------------------------------
    // Parallel merge (MergeXxx)
    // ------------------------------------------------------------------

    /// Attempt to combine this Region with `other` into a Region of higher
    /// dimensionality, for a parallel `CmpMap` of the two.
    ///
    /// `intfirst` selects the axis order in the combined Region: when true,
    /// this Region's axes go first and `other`'s axes second; when false,
    /// `other`'s axes go first. Returns the merged Region as a `DynMapping`,
    /// or `None` if the two Regions cannot be merged.
    ///
    /// Mirrors the per-class `MergeXxx` helpers in the C reference
    /// (`interval.c::MergeInterval`, `box.c::MergeBox`,
    /// `pointlist.c::MergePointList`, `nullregion.c::MergeNullRegion`), whose
    /// `intfirst`/`boxfirst`/`plsfirst` parameter this mirrors. The default
    /// returns `None` (classes that never merge in parallel — Circle,
    /// Ellipse, Polygon, CmpRegion, Prism).
    #[doc(hidden)]
    fn region_merge(&self, _other: &dyn Region, _intfirst: bool) -> Option<DynMapping> {
        None
    }

    // ------------------------------------------------------------------
    // Cloning (object-safe deep copy)
    // ------------------------------------------------------------------

    /// Clone this region into a boxed trait object.
    #[doc(hidden)]
    fn clone_region(&self) -> DynRegion;
}

/// Result of picking axes from a Region: either a same-class sub-Region
/// (when the current→base Mapping splits cleanly) or a plain Frame.
///
/// Mirrors the dual return of C `region.c::PickAxes` (region.c:7579), which
/// returns a Frame that `astIsARegion` may or may not accept.
pub enum PickedAxes {
    /// A Region spanning the selected axes.
    Region(DynRegion),
    /// A plain Frame (the Mapping did not split, or no sub-region formed).
    Frame(DynFrame),
}

/// Pick `axes` (zero-based current-Frame indices) from `region`, returning a
/// [`PickedAxes::Region`] when the current→base Mapping splits cleanly on the
/// selected axes and the region's class can form a base sub-region, else a
/// [`PickedAxes::Frame`].
///
/// Faithful port of C `region.c::PickAxes` (region.c:7579).
#[allow(dead_code)] // wired into Box::convert_to_prism in sub-project A2
pub(crate) fn pick_axes_region(region: &dyn Region, axes: &[usize]) -> AstResult<PickedAxes> {
    use crate::frames::FrameRef;

    // Pick the required axes from the current Frame. The C code passes the
    // axis list straight to astPickAxes (frame.c) which uses zero-based
    // indices; pick_axes_snapshot takes i32 with -1 meaning "default axis".
    let cfrm = region.get_region_frame();
    let target: Vec<i32> = axes.iter().map(|&a| a as i32).collect();
    let picked_frame = cfrm.pick_axes_snapshot(&target)?;

    // No default axes are supported for the Region path (C returns a basic
    // Frame if any default axes were requested). We only reach here with
    // concrete indices, so proceed to the split test.
    //
    // See if the requested current-Frame axes correspond to a unique set of
    // base-Frame axes by attempting to split the current->base Mapping.
    let cbmap = region
        .get_region_frameset()
        .mapping(FrameRef::Current, FrameRef::Base)?;

    // map_split expects 1-based axis indices and returns the (1-based)
    // output axes of `cbmap` — i.e. the base-Frame axes generated by the
    // selected current-Frame axes.
    let one_based: Vec<usize> = axes.iter().map(|&a| a + 1).collect();
    if let Some((out_axes, fsmap)) = cbmap.map_split(&one_based) {
        // Convert the returned base-Frame axis indices to zero-based for
        // reg_base_pick_axes, which slices the base defining geometry.
        let base_axes: Vec<usize> = out_axes.iter().map(|&a| a - 1).collect();

        if let Some(breg) = region.reg_base_pick_axes(&base_axes)? {
            // Invert fsmap so it maps selected base axes -> selected current
            // axes, then map the base region into the picked current Frame.
            let mut fsmap = fsmap;
            fsmap.invert();
            let mut creg = breg.map_region(fsmap.as_ref(), picked_frame.as_ref())?;
            // Copy properties from the old Region to the new Region.
            creg.reg_overlay(region);
            return Ok(PickedAxes::Region(creg));
        }
    }

    Ok(PickedAxes::Frame(picked_frame))
}

impl Clone for Box<dyn Region> {
    fn clone(&self) -> Self {
        self.clone_region()
    }
}

/// Borrow a `dyn Mapping` as a `dyn Region` if it is a concrete Region
/// type, else return `None`. The C reference uses `astIsARegion` for the
/// same purpose in the region merge code.
pub(crate) fn as_region(mapping: &dyn Mapping) -> Option<&dyn Region> {
    let a = mapping.as_any();
    if let Some(r) = a.downcast_ref::<Circle>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<BoxRegion>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<Ellipse>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<Polygon>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<Interval>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<NullRegion>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<CmpRegion>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<PointList>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<Prism>() {
        Some(r)
    } else {
        None
    }
}

/// Test whether a `dyn Mapping` is a Region.
fn is_region(mapping: &dyn Mapping) -> bool {
    let a = mapping.as_any();
    a.is::<Circle>()
        || a.is::<BoxRegion>()
        || a.is::<Ellipse>()
        || a.is::<Polygon>()
        || a.is::<Interval>()
        || a.is::<NullRegion>()
        || a.is::<CmpRegion>()
        || a.is::<PointList>()
        || a.is::<Prism>()
}

/// Remove all Region nodes from a mapping tree.
///
/// Walks the mapping tree recursively. Any node that is a Region is
/// replaced with a [`UnitMap`] of matching dimensionality, removing
/// the region's point-clipping behaviour while preserving the overall
/// dimensionality of the coordinate transformation.
///
/// For a [`CmpMap`], both children are processed recursively.
/// Other non-Region mappings are returned as clones.
pub fn remove_regions(mapping: &dyn Mapping) -> DynMapping {
    if is_region(mapping) {
        return Box::new(UnitMap::new(mapping.nin()));
    }

    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        let mut first = remove_regions(cmp.first());
        let mut second = remove_regions(cmp.second());
        // Preserve the effective inversion state of each child.
        if cmp.inv_a() != cmp.first().is_inverted() {
            first.invert();
        }
        if cmp.inv_b() != cmp.second().is_inverted() {
            second.invert();
        }
        let mut new_cmp = CmpMap::new(first, second, cmp.composition())
            .expect("remove_regions: CmpMap reconstruction failed");
        if mapping.is_inverted() {
            new_cmp.invert();
        }
        return Box::new(new_cmp);
    }

    if let Some(sw) = mapping
        .as_any()
        .downcast_ref::<crate::mappings::SwitchMap>()
    {
        let new_fs = sw.forward_selector().map(remove_regions);
        let new_is = sw.inverse_selector().map(remove_regions);
        let new_routes: Vec<DynMapping> = sw
            .route_maps()
            .iter()
            .map(|r| remove_regions(r.as_ref()))
            .collect();

        let fs_changed = match (sw.forward_selector(), &new_fs) {
            (Some(orig), Some(new)) => !new.equal(orig),
            _ => false,
        };
        let is_changed = match (sw.inverse_selector(), &new_is) {
            (Some(orig), Some(new)) => !new.equal(orig),
            _ => false,
        };
        let routes_changed = sw
            .route_maps()
            .iter()
            .zip(new_routes.iter())
            .any(|(orig, new)| !new.equal(orig.as_ref()));

        if fs_changed || is_changed || routes_changed {
            let mut new_sw = crate::mappings::SwitchMap::new(new_fs, new_is, new_routes)
                .expect("remove_regions: SwitchMap reconstruction failed");
            if mapping.is_inverted() {
                new_sw.invert();
            }
            return Box::new(new_sw);
        }

        return mapping.clone_box();
    }

    mapping.clone_box()
}

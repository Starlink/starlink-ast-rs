//! The Region trait and associated types.
//!
//! A Region represents an area within a coordinate frame. It combines
//! the [`Frame`] trait (describing the coordinate system) with the
//! [`Mapping`] trait (transforming points via the inside/outside
//! predicate: interior points pass through, exterior points become
//! [`BAD_VALUE`](crate::constants::BAD_VALUE)).

pub(crate) mod data;

mod box_region;
mod circle;
mod interval;
mod nullregion;
pub use box_region::{BoxRegion, BoxRegionBuilder};
pub use circle::{Circle, CircleBuilder};
pub use interval::{Interval, IntervalBuilder};
pub use nullregion::{NullRegion, NullRegionBuilder};

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::frames::{DynFrame, Frame, FrameSet};
use crate::mappings::Mapping;

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
    // Cloning (object-safe deep copy)
    // ------------------------------------------------------------------

    /// Clone this region into a boxed trait object.
    #[doc(hidden)]
    fn clone_region(&self) -> DynRegion;
}

impl Clone for Box<dyn Region> {
    fn clone(&self) -> Self {
        self.clone_region()
    }
}

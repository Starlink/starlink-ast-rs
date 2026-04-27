//! The Region trait and associated types.

use crate::error::AstResult;
use crate::frames::Frame;
use crate::mappings::Mapping;

/// An owned, dynamically-dispatched Region.
pub type DynRegion = Box<dyn Region>;

/// Result of comparing two regions for overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlap {
    /// No overlap between the regions.
    None,
    /// Partial overlap.
    Partial,
    /// The first region is entirely inside the second.
    FirstInside,
    /// The second region is entirely inside the first.
    SecondInside,
    /// The regions are identical.
    Same,
}

/// A region of a coordinate frame.
///
/// Regions implement both [`Frame`] (they describe a coordinate system)
/// and [`Mapping`] (they transform points via the inside/outside predicate).
pub trait Region: Frame + Mapping {
    /// Test whether a point is inside this region.
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool>;

    /// Determine the overlap between this region and another.
    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap>;

    /// Return a negated copy of this region.
    fn negated(&self) -> DynRegion;
}

//! The Mapping trait and associated types.

use crate::coords::PointSet;
use crate::error::AstResult;

/// An owned, dynamically-dispatched Mapping.
pub type DynMapping = Box<dyn Mapping>;

/// A coordinate transformation.
///
/// All concrete mapping classes implement this trait. Mappings are `Send`
/// (movable between threads) but not `Sync` (no shared references across
/// threads — mutation is lock-free).
pub trait Mapping: Send {
    /// Number of input coordinates.
    fn nin(&self) -> usize;

    /// Number of output coordinates.
    fn nout(&self) -> usize;

    /// Whether this mapping is currently inverted.
    fn is_inverted(&self) -> bool;

    /// Invert this mapping in place.
    fn invert(&mut self);

    /// Transform a set of points.
    ///
    /// If `forward` is true, applies the forward transformation; otherwise
    /// applies the inverse.
    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet>;

    /// Transform points into a pre-allocated output buffer.
    ///
    /// `out` must have shape `(pts.npoint(), self.nout())` for forward or
    /// `(pts.npoint(), self.nin())` for inverse transforms.
    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()>;

    /// Return a simplified version of this mapping.
    ///
    /// The simplified mapping is functionally equivalent but may be more
    /// efficient (e.g. cancelling inverse pairs, merging linear maps).
    fn simplified(&self) -> AstResult<DynMapping>;

    /// Rate of change of a specified output with respect to a specified input.
    ///
    /// `ax1` is the 1-based index of the output axis.
    /// `ax2` is the 1-based index of the input axis.
    /// `at` is the position at which to evaluate the rate.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64>;
}

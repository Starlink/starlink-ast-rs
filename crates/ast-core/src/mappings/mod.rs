//! The Mapping trait and associated types.

pub(crate) mod helpers;
pub(crate) mod rate;
pub(crate) mod simplify;
mod unit;
pub use unit::UnitMap;
mod zoom;
pub use zoom::ZoomMap;
mod shift;
pub use shift::ShiftMap;
mod matrix;
pub use matrix::MatrixMap;
mod perm;
pub use perm::PermMap;
mod winmap;
pub use winmap::WinMap;
mod lutmap;
pub use lutmap::{LutInterp, LutMap};
mod polymap;
pub use polymap::{PolyMap, PolyTerm};
mod chebymap;
pub use chebymap::ChebyMap;
mod wcsmap;
pub use crate::wcslib::WcsProjection;
pub use wcsmap::WcsMap;
mod slamap;
pub use slamap::{SlaCvtType, SlaMap};
mod specmap;
pub use specmap::{SpecCvtType, SpecMap};
mod timemap;
pub use timemap::{TimeCvtType, TimeMap};
mod sphmap;
pub use sphmap::SphMap;
mod cmpmap;
pub use cmpmap::{CmpMap, Composition};

use std::any::Any;
use std::fmt;

use crate::coords::PointSet;
use crate::error::AstResult;

/// An owned, dynamically-dispatched Mapping.
pub type DynMapping = Box<dyn Mapping>;

impl Clone for Box<dyn Mapping> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Provides downcasting from `&dyn Mapping` to a concrete type.
///
/// This is the Rust equivalent of the C library's `astIsAFoo` family of
/// class-membership checks. A blanket impl gives every `T: Any` an
/// `as_any` method automatically, so concrete mapping classes do not
/// need per-class boilerplate.
///
/// Use as `m.as_any().is::<UnitMap>()` or
/// `m.as_any().downcast_ref::<MatrixMap>()`.
pub trait AsAny: Any {
    /// Coerce to `&dyn Any` for runtime type queries.
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A coordinate transformation.
///
/// All concrete mapping classes implement this trait. Mappings are `Send`
/// (movable between threads) but not `Sync` (no shared references across
/// threads — mutation is lock-free).
pub trait Mapping: AsAny + Send + fmt::Debug {
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

    /// Test whether this mapping is equal to another.
    ///
    /// Uses AST's standard floating-point tolerance for numeric comparisons.
    /// Handles inverted mappings where appropriate (e.g., ZoomMap with
    /// factor 2 inverted equals ZoomMap with factor 0.5 non-inverted).
    fn equal(&self, other: &dyn Mapping) -> bool;

    /// Attempt to merge this mapping with an adjacent mapping.
    ///
    /// If `series` is true, the mappings are composed in series (self then
    /// other for forward, or other then self for inverse). If false, they
    /// are composed in parallel.
    ///
    /// `self_inv` and `other_inv` indicate whether each mapping should be
    /// treated as effectively inverted in the composition. The mappings
    /// themselves are stored in their forward (non-inverted) state; the
    /// inversion information is carried separately by the simplification
    /// engine.
    ///
    /// Returns `Some(merged)` if the two can be replaced by a single
    /// mapping, `None` otherwise. The returned mapping is in its forward
    /// (non-inverted) state.
    fn try_merge(
        &self,
        _other: &dyn Mapping,
        _series: bool,
        _self_inv: bool,
        _other_inv: bool,
    ) -> Option<DynMapping> {
        None
    }

    /// Split this mapping to extract a sub-mapping for a contiguous range of output axes.
    ///
    /// Given a 0-based output axis range `[first_out .. first_out + nout_wanted)`,
    /// returns the sub-mapping that produces exactly those outputs, consuming the
    /// corresponding input axes.
    ///
    /// Returns `None` if the mapping cannot be split at the requested boundary
    /// (e.g., a full MatrixMap where outputs depend on all inputs).
    fn map_split(&self, _first_out: usize, _nout_wanted: usize) -> Option<DynMapping> {
        None
    }

    /// Whether this mapping should be protected from simplification.
    ///
    /// When true, `simplified()` returns self unchanged and `try_merge()`
    /// returns `None` (enforced by the merge loop, not per-mapping).
    fn do_not_simplify(&self) -> bool {
        false
    }

    /// Set the do-not-simplify protection flag.
    ///
    /// When set to true, the simplification engine will not merge this
    /// mapping with adjacent mappings and will not simplify it individually.
    fn set_do_not_simplify(&mut self, _flag: bool) {}

    #[doc(hidden)]
    fn clone_box(&self) -> DynMapping;
}

impl dyn Mapping {
    /// Simplify this mapping and mark the result as simplified.
    ///
    /// This is the public entry point equivalent to C's `astSimplify`.
    /// It calls the type-specific `simplified()` method and then sets the
    /// `do_not_simplify` flag on the result, which causes `IsSimp = 1`
    /// to appear in the serialized output.
    ///
    /// Use this method for user-facing simplification. The trait method
    /// `simplified()` is the internal virtual dispatch used during the
    /// merge loop and does NOT set the flag.
    pub fn simplify(&self) -> AstResult<DynMapping> {
        let mut result = self.simplified()?;
        result.set_do_not_simplify(true);
        Ok(result)
    }

    /// Compose this mapping with `next` in series.
    ///
    /// The resulting `CmpMap` applies `self` first, then `next`.
    /// Requires `self.nout() == next.nin()`.
    pub fn then(self: Box<Self>, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(self, next, Composition::Series)
    }

    /// Compose this mapping with `other` in parallel.
    ///
    /// The resulting `CmpMap` applies `self` and `other` side-by-side
    /// to disjoint coordinate ranges.
    pub fn alongside(self: Box<Self>, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(self, other, Composition::Parallel)
    }
}

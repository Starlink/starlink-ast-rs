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
pub(crate) mod polymap;
pub use polymap::{PolyMap, PolyTerm};
mod chebymap;
#[cfg(feature = "simd-eval")]
pub mod polymap_simd;
pub use chebymap::ChebyMap;
mod wcsmap;
pub use crate::wcslib::WcsProjection;
pub use wcsmap::WcsMap;
mod slamap;
pub use slamap::{SlaCvtType, SlaMap};
pub(crate) mod specmap;
pub use specmap::{SpecCvtType, SpecMap};
mod timemap;
pub use timemap::{TimeCvtType, TimeMap};
mod pcdmap;
pub use pcdmap::PcdMap;
mod grismmap;
pub use grismmap::GrismMap;
mod splinemap;
pub use splinemap::SplineMap;
pub(crate) mod unitnormmap;
pub use unitnormmap::UnitNormMap;
mod sphmap;
pub use sphmap::SphMap;
mod cmpmap;
pub use cmpmap::{CmpMap, Composition};
mod tranmap;
pub use tranmap::TranMap;
mod ratemap;
pub use ratemap::RateMap;
mod normmap;
pub use normmap::NormMap;
mod dssmap;
pub use dssmap::DssMap;
mod mathmap;
pub use mathmap::MathMap;
mod selectormap;
pub use selectormap::{SelectorMap, SelectorMapBuilder};
mod switchmap;
pub use switchmap::SwitchMap;
mod linear_approx;
pub use linear_approx::LinearFit;
mod map_box;
pub use map_box::MapBoxResult;

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

    /// Coerce to `&mut dyn Any` for runtime type mutation.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
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

    /// Transform points using the "NaN-as-bad" internal protocol.
    ///
    /// This variant of [`transform_into`] is the low-level entry that
    /// compositional mappings (e.g. [`CmpMap`]) use to call sub-mappings
    /// without re-running the boundary BAD↔NaN substitution at each stage.
    /// Implementers should assume:
    ///
    /// - The caller has already converted `BAD_VALUE` inputs to `f64::NAN`
    ///   exactly once at the top of the pipeline (via
    ///   [`dyn Mapping::transform_nan_protocol`]).
    /// - The implementation may skip per-point BAD checks. NaN propagates
    ///   through arithmetic naturally, so polynomial / linear / trigonometric
    ///   stages just compute through.
    /// - The output is allowed to contain `f64::NAN` for bad points; the
    ///   top-level entry converts NaN → `BAD_VALUE` once at the bottom.
    ///
    /// The default implementation delegates to [`transform_into`], which
    /// preserves the original BAD-checking semantics. Classes that opt in to
    /// the protocol override this method to drop the per-point checks.
    ///
    /// Compositional mappings (`CmpMap`, `FrameSet`-as-Mapping, etc.) should
    /// override this to recursively call `transform_into_nan` on their
    /// sub-mappings — that's where the bulk of the saving is, since a chain
    /// of seven NaN-aware stages avoids seven separate per-point BAD scans.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        // Default: behave exactly like the existing path. Safe because
        // `is_bad` already includes `!is_finite()`, so a NaN input handed to
        // an unconverted Mapping still produces BAD output.
        self.transform_into(pts, forward, out)
    }

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

    /// Split this mapping to produce a sub-mapping covering a chosen set of inputs.
    ///
    /// Mirrors the C library's `astMapSplit` (mapping.c ~7798): given input
    /// indices `in_axes`, return the output axes those inputs feed into plus
    /// the sub-mapping that maps `in_axes` -> those outputs. Returns `None`
    /// when no such split exists - i.e. when some retained input feeds an
    /// output that *also* depends on a non-retained input, or when some
    /// non-retained input feeds an output that depends only on retained
    /// inputs.
    ///
    /// `in_axes` indices are 0-based and may be in any order. The returned
    /// `Vec<usize>` has length equal to the sub-mapping's nout. The default
    /// implementation handles the trivial case `in_axes.len() == self.nin()`
    /// by composing a `PermMap` with `self`; classes that can split for
    /// partial input selections override.
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        helpers::full_set_split(self, in_axes)
    }

    /// Best-fit affine approximation to the forward transform over a
    /// sample box.
    ///
    /// Returns `Some(fit)` when the transform is linear (within `tol`,
    /// Cartesian distance, output space) over the input box `[lbnd,
    /// ubnd]`; `None` otherwise. Subclasses with an exact linear form
    /// (e.g. `MatrixMap`, `ZoomMap`) may override for accuracy and
    /// speed; the default implementation samples and fits, mirroring
    /// `mapping.c::astLinearApprox`.
    ///
    /// `lbnd` / `ubnd` must each have length `self.nin()`.
    fn linear_approx(&self, lbnd: &[f64], ubnd: &[f64], tol: f64) -> Option<LinearFit> {
        linear_approx::linear_approx_default(self, lbnd, ubnd, tol)
    }

    /// Find the bounding box of a nominated output coordinate over a
    /// constrained input region.
    ///
    /// Mirrors C `astMapBox` (mapping.c:7121). Given input bounds
    /// `lbnd_in`/`ubnd_in` (length = effective `nin` for the chosen
    /// direction, with `lbnd > ubnd` accepted and silently swapped),
    /// returns the lowest and highest values of output coordinate
    /// `coord_out` (zero-based) reached within that input volume,
    /// together with positions where those bounds are attained.
    ///
    /// The default implementation simplifies the Mapping, runs
    /// SpecialBounds (when nin <= 12) for an initial estimate, then
    /// refines via the GlobalBounds simplex search. Concrete classes
    /// with closed-form bounds (e.g. linear maps) may override.
    fn map_box(
        &self,
        lbnd_in: &[f64],
        ubnd_in: &[f64],
        forward: bool,
        coord_out: usize,
    ) -> AstResult<MapBoxResult> {
        map_box::map_box_default(self, lbnd_in, ubnd_in, forward, coord_out)
    }

    /// Whether the forward transform is defined.
    fn has_forward(&self) -> bool {
        true
    }

    /// Whether the inverse transform is defined.
    fn has_inverse(&self) -> bool {
        true
    }

    /// Intrinsic (pre-invert) transform-definedness flags as written to
    /// native dumps.
    ///
    /// C dumps `Fwd`/`Inv` from the Mapping base-struct fields, which
    /// only CmpMap populates with computed values at construction —
    /// every other class passes `(1, 1)` to `astInitMapping` and
    /// reports availability dynamically via `GetTranForward` /
    /// `GetTranInverse`, which the Dump function never consults.
    /// Override only where the C class passes computed flags.
    fn dump_tran_flags(&self) -> (bool, bool) {
        (true, true)
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
        // Top-level cleanup: an inverted SwitchMap is canonicalised by
        // folding its inversion into the children (swapping fwd/inv
        // selectors and inverting each route). This matches the C
        // library's MapMerge inversion-fold path and is intentionally
        // not done by SwitchMap::simplified(), so that flatten() inside
        // a CmpMap can extract inversion via its bool tracker (which is
        // what enables the inverse-cancel pattern between two paired
        // SwitchMaps to merge).
        // Explicit `result.as_ref()` (rather than `result.as_any()`) is
        // needed: calling `as_any` directly on a `Box<dyn Mapping>`
        // would resolve via auto-deref but the resulting `&dyn Any`
        // points at the Box itself, not the inner mapping. `as_ref`
        // gives us `&dyn Mapping`, on which `as_any` returns the inner
        // type-erased reference that downcast_ref can match.
        let r: &dyn Mapping = result.as_ref();
        if let Some(sw) = r.as_any().downcast_ref::<SwitchMap>() {
            if let Some(normalized) = sw.normalize_inverted()? {
                result = Box::new(normalized);
                result = result.simplified()?;
            }
        }
        // Canonicalise nested CmpMaps with inverted=true so that no
        // CmpMap in the tree carries a redundant Invert/InvA/InvB pair
        // (the runtime XOR cancels them but the C reference emits a
        // single canonical form). The pass preserves runtime behaviour.
        // Same as_any caveat as the SwitchMap block above: must go
        // through `as_mut` on the Box first to reach the inner dyn.
        let rm: &mut dyn Mapping = result.as_mut();
        if let Some(cm) = rm.as_any_mut().downcast_mut::<CmpMap>() {
            cm.canonicalize_inverted_cmpmaps();
        }
        result.set_do_not_simplify(true);
        Ok(result)
    }

    /// Transform using the NaN-as-bad protocol: substitute `BAD_VALUE → NaN`
    /// at the input boundary once, delegate to [`Mapping::transform_into_nan`]
    /// (which compositional mappings thread through their sub-mappings
    /// without re-substituting), then `NaN → BAD_VALUE` at the output boundary
    /// once.
    ///
    /// This is the fast public entry for transforms that go through a
    /// multi-stage chain: each stage's `transform_into_nan` skips the
    /// per-point BAD check that `transform_into` performs, since the protocol
    /// guarantees there are no `BAD_VALUE` sentinels inside the pipeline.
    /// Naive Mappings (which only override `transform_into`) still work
    /// correctly — the default `transform_into_nan` falls through to
    /// `transform_into`, which re-imposes the BAD-check at that stage. The
    /// speedup scales with how many stages opt in.
    ///
    /// Result is identical to [`Mapping::transform`] modulo NaN-input
    /// handling: this variant converts any NaN output to `BAD_VALUE` (per
    /// the CLAUDE.md output contract), whereas an arbitrary `transform`
    /// implementation might pass NaN through.
    pub fn transform_nan_protocol(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        // Boundary substitution: BAD_VALUE → NaN. Allocates a new PointSet
        // rather than mutating the caller's input. For very large inputs we
        // could add an in-place variant if profiling motivates it.
        let pts_nan = substitute_bad_to_nan(pts);

        let nout = if forward { self.nout() } else { self.nin() };
        let mut out = PointSet::zeros(pts.npoint(), nout);
        self.transform_into_nan(&pts_nan, forward, &mut out)?;

        // Boundary substitution: NaN → BAD_VALUE (in place on the output).
        substitute_nan_to_bad_in_place(&mut out);
        Ok(out)
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

// ---------------------------------------------------------------------------
// NaN-as-bad protocol: boundary substitution helpers
// ---------------------------------------------------------------------------

/// Copy `pts`, substituting `BAD_VALUE` (and any other non-finite value) with
/// `f64::NAN`. Used by [`dyn Mapping::transform_nan_protocol`] to set up the
/// internal NaN-as-bad protocol once at the boundary of a transform pipeline.
///
/// We substitute *any* non-finite value (matches what [`helpers::is_bad`]
/// already considers BAD) so that pipelines accepting NaN or ±∞ inputs from
/// callers behave the same as those receiving the canonical `BAD_VALUE`.
fn substitute_bad_to_nan(pts: &PointSet) -> PointSet {
    use crate::constants::BAD_VALUE;
    let mut out = PointSet::zeros(pts.npoint(), pts.ncoord());
    let in_view = pts.view();
    let in_flat: &[f64] = in_view.to_slice().expect("PointSet is C-contiguous");
    let mut out_view = out.view_mut();
    let out_flat: &mut [f64] = out_view.as_slice_mut().expect("PointSet is C-contiguous");
    for (o, &v) in out_flat.iter_mut().zip(in_flat.iter()) {
        *o = if v == BAD_VALUE || !v.is_finite() {
            f64::NAN
        } else {
            v
        };
    }
    out
}

/// Replace every `f64::NAN` (and ±∞) value in `pts` with `BAD_VALUE` in
/// place. Used at the output boundary of [`dyn Mapping::transform_nan_protocol`]
/// to restore the canonical sentinel value before returning to the caller.
fn substitute_nan_to_bad_in_place(pts: &mut PointSet) {
    use crate::constants::BAD_VALUE;
    let mut view = pts.view_mut();
    let flat: &mut [f64] = view.as_slice_mut().expect("PointSet is C-contiguous");
    for v in flat.iter_mut() {
        if !v.is_finite() {
            *v = BAD_VALUE;
        }
    }
}

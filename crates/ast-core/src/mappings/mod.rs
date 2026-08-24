//! The Mapping trait and associated types.

pub(crate) mod helpers;
pub(crate) mod rate;
pub(crate) mod simplify;
mod simplify_entry;
pub use simplify_entry::ViaSimplify;
mod unit;
pub use unit::UnitMap;
mod zoom;
pub use zoom::ZoomMap;
mod shift;
pub use shift::ShiftMap;
mod matrix;
mod matrix_merge;
pub use matrix::MatrixMap;
pub(crate) use matrix::compress_full_matrix;
mod perm;
pub use perm::PermMap;
mod winmap;
pub use winmap::WinMap;
mod lutmap;
mod winmap_merge;
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
use crate::error::{AstError, AstResult, ErrorCode};

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

/// Coercion to `&dyn Mapping`.
///
/// `<dyn Mapping>::simplify` and the other entry points that take a trait object
/// are inherent methods on `dyn Mapping`, so reaching them needs a `&dyn
/// Mapping`. A `Mapping` provided method has `Self: ?Sized`, which cannot be
/// coerced directly, and this supplies the coercion. Being one of [`Mapping`]'s
/// supertraits, it is available on an already-erased `dyn Mapping` too, which
/// reaches its concrete class's implementation through the vtable.
///
/// Blanket-implemented like [`AsAny`], so concrete classes need no per-class
/// boilerplate.
pub trait AsDynMapping {
    /// Coerce to `&dyn Mapping`.
    fn as_dyn_mapping(&self) -> &dyn Mapping;
}

impl<T: Mapping> AsDynMapping for T {
    fn as_dyn_mapping(&self) -> &dyn Mapping {
        self
    }
}

/// Which orientations of a Mapping have already been simplified.
///
/// C keeps two bits — `AST__ISSIMPLE_FLAG` and `AST__ISSIMPLEINV_FLAG`
/// (mapping.h:414-419) — and picks between them on the current `Invert` value.
/// Simplifying a Mapping produces something equivalent to the orientation the
/// original had at the time, and the inverse may simplify differently, so the
/// two are recorded separately.
///
/// The consequence that matters is what does *not* happen: setting `Invert`
/// selects the other record instead of discarding one, so the many routines
/// that invert a Mapping only to line it up for inspection and then invert it
/// back leave it reporting exactly what it reported before. That is what makes
/// the placement of an `IsSimp` card in a dump a property of the Mapping tree
/// alone, rather than of which nodes some earlier probe happened to touch.
///
/// Neither record is cleared as a side effect of anything else. A class
/// discards both, via [`SimplifiedRecord::clear`], only when it lets something
/// change that feeds its own simplification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SimplifiedRecord {
    forward: bool,
    inverse: bool,
}

impl SimplifiedRecord {
    /// The record a Mapping loaded from a dump starts with: the `IsSimp` card
    /// describes the orientation the Mapping was written out in, which is the
    /// orientation its `Invert` card sets up (mapping.c:24135).
    pub(crate) fn for_orientation(inverted: bool, flag: bool) -> Self {
        let mut record = Self::default();
        record.set(inverted, flag);
        record
    }

    /// Whether the given orientation has been simplified.
    pub(crate) fn get(self, inverted: bool) -> bool {
        if inverted { self.inverse } else { self.forward }
    }

    /// Record (or un-record) the given orientation, leaving the other alone.
    pub(crate) fn set(&mut self, inverted: bool, flag: bool) {
        if inverted {
            self.inverse = flag;
        } else {
            self.forward = flag;
        }
    }

    /// Discard both records (C `astClearIsSimple`, mapping.h:440).
    ///
    /// A class needs this when it lets something change that feeds its own
    /// simplification — an input to the part of its merge that runs "without
    /// reference to the neighbouring Mappings in the list". Nothing else reads
    /// the records: a Mapping is offered to its neighbours' merge whatever
    /// they hold, so a change that only affects whether this Mapping merges
    /// with a neighbour need not discard them. Discarding anyway is safe, since
    /// the records can only ever suppress a simplification and never make one
    /// wrong, so err towards it when in doubt.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// A coordinate transformation.
///
/// All concrete mapping classes implement this trait. Mappings are `Send`
/// (movable between threads) but not `Sync` (no shared references across
/// threads — mutation is lock-free).
pub trait Mapping: AsAny + AsDynMapping + Send + fmt::Debug {
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

    /// Transform a set of 1-dimensional coordinates.
    ///
    /// Mirrors C `astTran1` (`mapping.c:89`): the Mapping must have `Nin` and
    /// `Nout` both equal to 1, and the requested direction must be defined.
    /// Saves the caller building a one-column [`PointSet`] for scalar data.
    ///
    /// The returned vector has the same length as `xin`.
    fn tran1(&self, xin: &[f64], forward: bool) -> AstResult<Vec<f64>> {
        validate_tran(self, forward, xin.len(), 1, "tran1")?;
        let pts = PointSet::from_columns(&[xin])?;
        let out = self.transform(&pts, forward)?;
        Ok(out.axis_iter(0).collect())
    }

    /// Transform a set of 2-dimensional coordinates.
    ///
    /// Mirrors C `astTran2` (`mapping.c:~150`): the Mapping must have `Nin` and
    /// `Nout` both equal to 2, and the requested direction must be defined.
    ///
    /// `xin` and `yin` must be the same length; C cannot express that mismatch
    /// because it takes a single `npoint` argument for both arrays.
    fn tran2(&self, xin: &[f64], yin: &[f64], forward: bool) -> AstResult<(Vec<f64>, Vec<f64>)> {
        if xin.len() != yin.len() {
            return Err(AstError::new(
                ErrorCode::Nptin,
                format!(
                    "tran2: xin has {} points but yin has {}",
                    xin.len(),
                    yin.len()
                ),
            ));
        }
        validate_tran(self, forward, xin.len(), 2, "tran2")?;
        let pts = PointSet::from_columns(&[xin, yin])?;
        let out = self.transform(&pts, forward)?;
        Ok((out.axis_iter(0).collect(), out.axis_iter(1).collect()))
    }

    /// Transform points using the "NaN-as-bad" internal protocol.
    ///
    /// This variant of [`Mapping::transform_into`] is the low-level entry that
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
    /// The default implementation delegates to [`Mapping::transform_into`], which
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

    /// Reduce this Mapping without the `IsSimple` gate — C's `Simplify` vtab
    /// slot.
    ///
    /// The reduced mapping is functionally equivalent but may be more
    /// efficient (e.g. cancelling inverse pairs, merging linear maps).
    ///
    /// Reachable only from `<dyn Mapping>::simplify`, which is what supplies the
    /// [`ViaSimplify`]. That mirrors C, where `mapping.c:24772` is the slot's
    /// only caller, so no reduction can happen without being short-circuited
    /// and recorded.
    ///
    /// Override this only if the matching C class overrides the slot: `CmpMap`,
    /// `FrameSet` and the nine Region classes do, and the other 25 keep every
    /// reduction in [`Mapping::map_merge`].
    fn simplified(&self, _gate: ViaSimplify) -> AstResult<DynMapping>;

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

    /// Decompose this mapping into a list of component mappings for the given
    /// composition direction, mirroring an overriding C `astMapList`.
    ///
    /// The simplification engine's `flatten` calls this on every non-CmpMap
    /// leaf before treating it as atomic. The default returns `None`, meaning
    /// "atomic leaf — append me as-is". A class that, when it appears in a
    /// compound of the matching kind, is equivalent to a sequence of simpler
    /// mappings (e.g. a `Prism`, which is a *parallel* combination of its two
    /// component Regions) returns `Some(components)` so the engine can
    /// continue flattening them and let the nominate loop merge across them.
    ///
    /// Each returned tuple is `(mapping, effective_inv)` — the effective
    /// inversion already combined with the caller-supplied `inv`. `series`
    /// selects the requested composition kind; a class only decomposes when
    /// `series` matches the kind it combines its components in (a Prism only
    /// for `series == false`).
    ///
    /// Port of C's overridable protected `astMapList` (mapping.c:7358);
    /// `Prism::MapList` (prism.c:1527) is the sole current override.
    #[doc(hidden)]
    fn map_list_decompose(&self, _inv: bool, _series: bool) -> Option<Vec<(DynMapping, bool)>> {
        None
    }

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

    /// Attempt to simplify a sequence of mappings by merging this
    /// (nominated) mapping with its neighbours.
    ///
    /// Port of C's protected `astMapMerge` (mapping.c:7660). When the
    /// simplification engine nominates this mapping at index `where_` in
    /// `ctx.list`, this method may remove, replace, insert, or reorder any
    /// entries in the list — updating the parallel invert flags — to
    /// shorten or speed up the sequence. `series` is true for a series
    /// composition, false for parallel. The invert flags carried in
    /// `ctx.list` (and not the mappings' own intrinsic Invert state) give
    /// the effective direction of each component, exactly as in C.
    ///
    /// Returns `Some(index)` of the first modified element on any change,
    /// or `None` for "no change" (C's `-1`).
    ///
    /// The default implementation performs a single direct pairwise merge
    /// of the nominated component with its right neighbour
    /// ([`simplify::default_map_merge`]) — the "direct merge" step (C's
    /// `MapMerge` step 2) shared by every class. Classes that additionally
    /// swap past, seek toward, or rewrite distant neighbours override this
    /// to add that logic (still calling the direct merge first).
    #[doc(hidden)]
    fn map_merge(
        &self,
        ctx: &mut simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        simplify::default_map_merge(ctx, where_, series)
    }

    /// Split this mapping to produce a sub-mapping covering a chosen set of
    /// inputs, using **1-based** axis indices throughout (public API).
    ///
    /// Mirrors the C library's public `astMapSplit` (the `astMapSplitId_`
    /// interface, mapping.c:25313): given 1-based input indices `in_axes`,
    /// return the **1-based** original-mapping output axes those inputs feed
    /// into, plus the sub-mapping that maps `in_axes` -> those outputs.
    /// Returns `None` when no such split exists — i.e. when some retained
    /// input feeds an output that *also* depends on a non-retained input, or
    /// when some non-retained input feeds an output that depends only on
    /// retained inputs.
    ///
    /// `in_axes` indices are 1-based (per the design-spec axis convention,
    /// matching [`Mapping::rate`]) and may be in any order. The returned
    /// `Vec<usize>` holds 1-based output indices and has length equal to the
    /// sub-mapping's nout.
    ///
    /// This is a thin ±1 wrapper over [`Mapping::map_split_zero_based`], which
    /// implements the algorithm in C's protected, 0-based encoding. Concrete
    /// classes override the zero-based method, not this one — exactly as C
    /// keeps a 0-based protected `MapSplit` (mapping.c:7798) behind the
    /// 1-based public shim.
    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Decrement to 0-based (astMapSplitId_, mapping.c:25429). A 0 entry
        // would underflow; treat it as out-of-range, matching the validation
        // that rejects axis indices outside 1..=nin.
        let mut in_zero = Vec::with_capacity(in_axes.len());
        for &ax in in_axes {
            if ax == 0 {
                return None;
            }
            in_zero.push(ax - 1);
        }
        let (out_zero, sub) = self.map_split_zero_based(&in_zero)?;
        // Increment the returned output axes back to 1-based (mapping.c:25437).
        let out_one = out_zero.into_iter().map(|o| o + 1).collect();
        Some((out_one, sub))
    }

    /// Protected, **0-based** implementation of [`Mapping::map_split`].
    ///
    /// Mirrors the C library's protected `astMapSplit` (mapping.c:7798): the
    /// algorithm operates entirely in 0-based axis indices, both for the
    /// supplied `in_axes` and for the returned output-axis vector. This is the
    /// method concrete classes override; the public 1-based [`Mapping::map_split`]
    /// wraps it with ±1 conversion at the boundary.
    ///
    /// The default implementation handles the trivial case
    /// `in_axes.len() == self.nin()` by composing a `PermMap` with `self`;
    /// classes that can split for partial input selections override.
    fn map_split_zero_based(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
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

    /// Whether this mapping has already been simplified **in its current
    /// orientation**.
    ///
    /// Mirrors C's `IsSimple` attribute (mapping.c): `Mapping::simplify`
    /// sets it on its result so a later `simplify` of the same object is a
    /// fast no-op (`simplified()` returns self unchanged and the merge loop
    /// skips it). It is an optimisation flag, not a permanent "never simplify"
    /// lock.
    ///
    /// Simplifying a Mapping produces something equivalent to the orientation
    /// it had at the time, and the inverse may simplify differently, so each
    /// orientation carries its own record, held in the crate-private
    /// `SimplifiedRecord`.
    /// `invert()` therefore changes which record applies rather than
    /// discarding one, and inverting and inverting back leaves the Mapping
    /// reporting exactly what it reported before. Serialised as the `IsSimp`
    /// token.
    fn has_been_simplified(&self) -> bool {
        false
    }

    /// Set (or clear) the "already simplified" record for the current
    /// orientation (C `astSetIsSimple`, mapping.h:428).
    ///
    /// `Mapping::simplify` sets it `true` on its result. Discarding the record
    /// is a class-level decision, made through `SimplifiedRecord::clear`.
    fn set_has_been_simplified(&mut self, _flag: bool) {}

    /// Whether the `IsSimp` token is written for this Mapping.
    ///
    /// C reads one record for both this and the `astSimplify` short-circuit.
    /// [`CmpMap`] splits them — see `CmpMap::dump_simplified` for why — so it
    /// overrides this; for every other class the two are the same thing.
    fn dump_is_simple(&self) -> bool {
        self.has_been_simplified()
    }

    /// Record `flag` for the named orientation, whatever orientation this
    /// Mapping is currently in.
    ///
    /// This is C's idiom for tagging a Mapping the caller is holding at the
    /// wrong orientation — set `Invert`, set the flag, set `Invert` back — and
    /// it works only because setting `Invert` no longer discards the record
    /// (mapping.c:23272). The engine needs it because it normalises a
    /// simplified component to the forward orientation while the tree still
    /// serialises that component inverted.
    fn set_simplified_in(&mut self, inverted: bool, flag: bool) {
        let restore = self.is_inverted();
        if restore != inverted {
            self.invert();
        }
        self.set_has_been_simplified(flag);
        if restore != inverted {
            self.invert();
        }
    }

    /// Whether this mapping carries the `AllowSimplify` flag (C `AlSimp`,
    /// mapping.h `AST__ALLOW_SIMPLIFY_FLAG`). Only consulted when the
    /// enclosing CmpMap has `RestrictedSimplify` set: under a restricted
    /// simplify, only components with this flag may be nominated for
    /// merging. Cleared once simplification completes (cmpmap.c:3683).
    /// Serialised as the `AlSimp` token.
    fn allow_simplify(&self) -> bool {
        false
    }

    /// Set (or clear) the `AllowSimplify` flag (C `AlSimp`).
    fn set_allow_simplify(&mut self, _flag: bool) {}

    #[doc(hidden)]
    fn clone_box(&self) -> DynMapping;
}

impl dyn Mapping {
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

/// Shared argument validation for [`Mapping::tran1`] and [`Mapping::tran2`].
///
/// Port of the relevant arms of C's `ValidateMapping` (`mapping.c`), which
/// `astTran1` / `astTran2` call before building their PointSets:
///
/// - `AST__TRNND` when the requested direction is not defined,
/// - `AST__NCPIN` when `Nin` or `Nout` is not the arity the entry point
///   requires,
/// - `AST__NPTIN` when fewer than one point is supplied.
///
/// Note the definedness check lives here because that is where C puts it.
/// The general [`Mapping::transform`] path does not yet raise `AST__TRNND`
/// for an undefined direction; that gap is tracked in `PLAN.md` §2.
fn validate_tran<M: Mapping + ?Sized>(
    map: &M,
    forward: bool,
    npoint: usize,
    arity: usize,
    method: &str,
) -> AstResult<()> {
    let defined = if forward {
        map.has_forward()
    } else {
        map.has_inverse()
    };
    if !defined {
        let dir = if forward { "forward" } else { "inverse" };
        return Err(AstError::new(
            ErrorCode::Trnnd,
            format!("{method}: the {dir} coordinate transformation is not defined"),
        ));
    }
    if map.nin() != arity || map.nout() != arity {
        return Err(AstError::new(
            ErrorCode::Ncpin,
            format!(
                "{method}: requires a Mapping with Nin = Nout = {arity}, but this \
                 Mapping has Nin = {} and Nout = {}",
                map.nin(),
                map.nout()
            ),
        ));
    }
    if npoint < 1 {
        return Err(AstError::new(
            ErrorCode::Nptin,
            format!("{method}: number of points to transform must be at least 1"),
        ));
    }
    Ok(())
}

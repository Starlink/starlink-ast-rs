//! The Mapping trait and associated types.

pub(crate) mod helpers;
pub(crate) mod rate;
pub mod report;
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
pub(crate) mod intramap;
pub use intramap::{IntraFlags, IntraMap, IntraRegistry, IntraTransform};
pub(crate) mod xphmap;
pub use xphmap::{XphMap, XphProjection};
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
mod quad_approx;
pub use quad_approx::QuadFit;
mod map_box;
pub mod rebin;
pub mod resample;
mod tran_grid;
pub use map_box::MapBoxResult;
pub use rebin::{
    RebinFlags, RebinInput, RebinOptions, RebinOutput, RebinSeqFlags, RebinSeqOptions, Spread,
    rebin, rebin_seq,
};
pub use resample::{
    Interp, InterpArgs, InterpFn, Kernel1Fn, ResampleFlags, ResampleInput, ResampleOptions,
    ResampleOutput, resample,
};

use std::any::Any;
use std::fmt;

use crate::coords::{PointSet, PointShape, Points, PointsMut};
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
/// Downcasting is how a class-membership test is made. A blanket impl gives
/// every `T: Any` an `as_any` method automatically, so concrete mapping
/// classes do not need per-class boilerplate.
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
    /// reference to the neighboring Mappings in the list". Nothing else reads
    /// the records: a Mapping is offered to its neighbors' merge whatever
    /// they hold, so a change that only affects whether this Mapping merges
    /// with a neighbor need not discard them. Discarding anyway is safe, since
    /// the records can only ever suppress a simplification and never make one
    /// wrong, so err towards it when in doubt.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The flags and attributes C keeps on its base Mapping class, which every
/// Mapping carries: the `Report` attribute and the `AllowSimplify` and
/// `RestrictedSimplify` flags of the Mapping `flags` word (mapping.c). A copy
/// keeps them, as `astCopy` does.
#[derive(Debug, Default)]
#[doc(hidden)]
pub struct MappingFlags {
    pub(crate) report: Option<bool>,
    pub(crate) allow_simplify: bool,
    pub(crate) restricted_simplify: bool,
    /// Which entry of a merge list the Mapping was, while a merge it may take
    /// part in runs (`simplify::dispatch_map_merge`); 0 otherwise. It stands
    /// in for C's comparison of Mapping pointers, so it stays with the
    /// Mapping as it moves and a copy does not take it.
    pub(crate) merge_tag: usize,
    /// Whether Invert is set to 0, which C tells from an unset Invert
    /// (mapping.c `TestInvert`); a set 1 is `is_inverted`.
    pub(crate) invert_zero: bool,
    /// C's `Frozen` flag (`astSetFrozen`): set on a Mapping whose merges a
    /// simplification found to be oscillating, so that it is not nominated
    /// again. It stays with the Mapping, as C's stays with the object, until
    /// a simplification clears it on the Mappings it starts with.
    pub(crate) frozen: bool,
}

impl Clone for MappingFlags {
    fn clone(&self) -> Self {
        MappingFlags {
            report: self.report,
            allow_simplify: self.allow_simplify,
            restricted_simplify: self.restricted_simplify,
            merge_tag: 0,
            invert_zero: self.invert_zero,
            frozen: self.frozen,
        }
    }
}

/// A relationship between two coordinate systems.
///
/// A Mapping transforms a set of coordinates representing "input" points
/// into a new set representing "output" points.
/// It describes how two coordinate systems are related and drives the
/// operations that use that relationship, such as transforming coordinates
/// and resampling grids of data.
/// The trait has no constructor of its own: each concrete class, such as
/// [`ZoomMap`] or [`CmpMap`], implements a particular kind of
/// transformation.
///
/// Every Mapping has these attributes:
///
/// - [`Invert`](Mapping::is_inverted): whether the Mapping is inverted.
/// - [`IsLinear`](Mapping::is_linear): whether the Mapping is linear.
/// - [`IsSimple`](Mapping::has_been_simplified): whether the Mapping has
///   been simplified.
/// - [`Nin`](Mapping::nin): the number of input coordinates.
/// - [`Nout`](Mapping::nout): the number of output coordinates.
/// - [`Report`](Mapping::report): whether transformed coordinates are
///   reported.
/// - [`TranForward`](Mapping::has_forward): whether the forward
///   transformation is defined.
/// - [`TranInverse`](Mapping::has_inverse): whether the inverse
///   transformation is defined.
///
/// Besides the methods here, grids of data are transformed by the free
/// functions [`resample()`], [`rebin()`] and [`rebin_seq`], and the Regions in a
/// Mapping are removed by [`remove_regions`](crate::regions::remove_regions).
///
/// Mappings are `Send` (movable between threads) but not `Sync` (no shared
/// references across threads — mutation is lock-free).
pub trait Mapping: AsAny + AsDynMapping + Send + fmt::Debug {
    /// The number of coordinates needed to specify an input point: the
    /// number of dimensions of the space the Mapping's input points lie in.
    ///
    /// Inverting the Mapping exchanges this with [`nout`](Mapping::nout).
    /// A [`CmpMap`] derives it from its components.
    /// A [`FrameSet`](crate::frames::FrameSet) describes its own.
    #[doc(alias = "Nin")]
    fn nin(&self) -> usize;

    /// The number of coordinates generated for each output point: the
    /// number of dimensions of the space the Mapping's output points lie in.
    ///
    /// Inverting the Mapping exchanges this with [`nin`](Mapping::nin).
    /// A [`CmpMap`] derives it from its components.
    /// A [`FrameSet`](crate::frames::FrameSet) describes its own.
    #[doc(alias = "Nout")]
    fn nout(&self) -> usize;

    /// Whether the Mapping is inverted.
    ///
    /// When false, the Mapping transforms coordinates as it did when it was
    /// created.
    /// When true, its forward and inverse transformations are exchanged, so
    /// that it behaves as the inverse of the original: [`nin`](Mapping::nin)
    /// and [`nout`](Mapping::nout) are exchanged, as are
    /// [`has_forward`](Mapping::has_forward) and
    /// [`has_inverse`](Mapping::has_inverse).
    ///
    /// Default: false.
    ///
    /// [`invert`](Mapping::invert) reverses it, and
    /// [`set_invert`](Mapping::set_invert),
    /// [`clear_invert`](Mapping::clear_invert) and
    /// [`test_invert`](Mapping::test_invert) set, clear and test it.
    /// A [`FrameSet`](crate::frames::FrameSet) describes its own.
    #[doc(alias = "Invert")]
    fn is_inverted(&self) -> bool;

    /// Invert the Mapping in place, reversing
    /// [`is_inverted`](Mapping::is_inverted).
    ///
    /// The attribute is left set when the result is inverted and unset when
    /// it is not, so [`test_invert`](Mapping::test_invert) reports whether
    /// the Mapping is now inverted.
    #[doc(alias = "astInvert")]
    fn invert(&mut self) {
        self.invert_orientation();
        self.mapping_flags_mut().invert_zero = false;
    }

    /// Flip this mapping's orientation: the class-specific part of
    /// [`Mapping::invert`], which callers use instead.
    #[doc(hidden)]
    fn invert_orientation(&mut self);

    /// Set [`is_inverted`](Mapping::is_inverted), inverting the Mapping if it
    /// is not already in the orientation asked for.
    ///
    /// Setting it to `false` still leaves it set, so
    /// [`test_invert`](Mapping::test_invert) is true.
    fn set_invert(&mut self, invert: bool) {
        if self.is_inverted() != invert {
            self.invert_orientation();
        }
        self.mapping_flags_mut().invert_zero = !invert;
    }

    /// Clear [`is_inverted`](Mapping::is_inverted), returning the Mapping to
    /// its original orientation.
    fn clear_invert(&mut self) {
        if self.is_inverted() {
            self.invert_orientation();
        }
        self.mapping_flags_mut().invert_zero = false;
    }

    /// Whether [`is_inverted`](Mapping::is_inverted) is set: true when the
    /// Mapping is inverted, or when it has been set to `false`.
    fn test_invert(&self) -> bool {
        self.is_inverted() || self.mapping_flags().invert_zero
    }

    /// Transform a set of points in any number of dimensions.
    ///
    /// If `forward` is true, applies the forward transformation; otherwise
    /// applies the inverse.
    /// `pts` holds one coordinate per input of the transformation used: the
    /// Mapping's [`nin`](Mapping::nin) for the forward transformation and
    /// its [`nout`](Mapping::nout) for the inverse.
    /// The result holds the same number of points, each with one coordinate
    /// per output of the transformation used.
    ///
    /// To transform coordinates held in separate per-axis arrays without
    /// copying them, use [`transform_into`](Mapping::transform_into).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Trnnd` when the requested transformation is not defined.
    /// - `ErrorCode::Ncpin` when `pts` has the wrong number of coordinates.
    ///
    /// # See also
    ///
    /// `astTranN`.
    #[doc(alias = "astTranN")]
    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        validate_transform(self, pts, forward, None)?;
        let nout = if forward { self.nout() } else { self.nin() };
        let mut out = PointSet::zeros(pts.npoint(), nout);
        self.transform_into(pts.as_points(), forward, out.as_points_mut())?;
        Ok(out)
    }

    /// Transform points into a caller's buffer.
    ///
    /// As [`transform`](Mapping::transform), but the input and output are
    /// views that may borrow a caller's own per-axis arrays, so coordinates
    /// held in one array per axis are transformed without a copy.
    /// `out` must have at least the Mapping's output axis count for the
    /// direction and at least `pts.npoint()` points; only the first
    /// `pts.npoint()` points of each output axis are written.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Trnnd` when the requested transformation is not defined.
    /// - `ErrorCode::Ncpin` when `pts` has the wrong number of coordinates.
    /// - `ErrorCode::Nopts` when `out` holds too few points.
    /// - `ErrorCode::Nocts` when `out` holds too few coordinates.
    ///
    /// # See also
    ///
    /// `astTranP`.
    #[doc(alias = "astTranP")]
    fn transform_into(&self, pts: Points<'_>, forward: bool, out: PointsMut<'_>) -> AstResult<()>;

    /// Transform a set of 1-dimensional coordinates.
    ///
    /// Applies the forward transformation when `forward` is true and the
    /// inverse otherwise, and returns the transformed coordinates, one per
    /// element of `xin`.
    /// This saves building a one-column [`PointSet`] for scalar data.
    /// When [`report`](Mapping::report) is true, each point is reported.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Trnnd` when the requested transformation is not defined.
    /// - `ErrorCode::Ncpin` when [`nin`](Mapping::nin) and
    ///   [`nout`](Mapping::nout) are not both 1.
    /// - `ErrorCode::Nptin` when `xin` is empty.
    #[doc(alias = "astTran1")]
    fn tran1(&self, xin: &[f64], forward: bool) -> AstResult<Vec<f64>> {
        validate_tran(self, forward, xin.len(), 1, "tran1")?;
        let mut xout = vec![0.0; xin.len()];
        self.transform_into(
            Points::new([xin])?,
            forward,
            PointsMut::new([&mut xout[..]])?,
        )?;
        // C reports from its array wrappers, not from astTransform
        // (mapping.c `Tran1`).
        if self.report() {
            let pts = PointSet::from_columns(&[xin])?;
            let out = PointSet::from_columns(&[&xout])?;
            self.report_points(forward, &pts, &out);
        }
        Ok(xout)
    }

    /// Transform a set of 2-dimensional coordinates.
    ///
    /// Applies the forward transformation when `forward` is true and the
    /// inverse otherwise to the points (`xin[i]`, `yin[i]`), and returns the
    /// transformed X and Y coordinates.
    /// When [`report`](Mapping::report) is true, each point is reported.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Nptin` when `xin` and `yin` differ in length, or are
    ///   empty.
    /// - `ErrorCode::Trnnd` when the requested transformation is not defined.
    /// - `ErrorCode::Ncpin` when [`nin`](Mapping::nin) and
    ///   [`nout`](Mapping::nout) are not both 2.
    #[doc(alias = "astTran2")]
    fn tran2(&self, xin: &[f64], yin: &[f64], forward: bool) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // C takes a single `npoint` for both arrays, so it cannot be given
        // arrays of different lengths.
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
        let mut xout = vec![0.0; xin.len()];
        let mut yout = vec![0.0; xin.len()];
        self.transform_into(
            Points::new([xin, yin])?,
            forward,
            PointsMut::new([&mut xout[..], &mut yout[..]])?,
        )?;
        if self.report() {
            let pts = PointSet::from_columns(&[xin, yin])?;
            let out = PointSet::from_columns(&[&xout, &yout])?;
            self.report_points(forward, &pts, &out);
        }
        Ok((xout, yout))
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
    /// The default implementation runs [`Mapping::transform_into`], whose
    /// `is_bad` checks treat a NaN input as bad, and turns the `BAD_VALUE` it
    /// writes into NaN, so that a later stage of the pipeline, which may not
    /// check, cannot take it for a number. Classes that opt in to the
    /// protocol override this method to drop the per-point checks.
    ///
    /// Compositional mappings (`CmpMap`, `FrameSet`-as-Mapping, etc.) should
    /// override this to recursively call `transform_into_nan` on their
    /// sub-mappings — that's where the bulk of the saving is, since a chain
    /// of seven NaN-aware stages avoids seven separate per-point BAD scans.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let npoint = pts.npoint();
        self.transform_into(pts, forward, out.reborrow())?;
        bad_to_nan(self, forward, npoint, &mut out);
        Ok(())
    }

    /// Reduce this Mapping without the `IsSimple` gate: the per-class step of
    /// `<dyn Mapping>::simplify`.
    ///
    /// The reduced mapping is functionally equivalent but may be more
    /// efficient (e.g. cancelling inverse pairs, merging linear maps).
    ///
    /// Reachable only from `<dyn Mapping>::simplify`, which is what supplies the
    /// [`ViaSimplify`], so no reduction can happen without being
    /// short-circuited and recorded.
    ///
    /// Override this only for a class that holds other Mappings and reduces
    /// them itself: [`CmpMap`], `FrameSet` and the Region classes do, and the
    /// other classes keep every reduction in their merge with neighboring
    /// Mappings.
    ///
    /// The default nominates this Mapping in a list of one and merges it with
    /// itself until nothing further changes.
    ///
    /// Returns the reduced Mapping and whether it is a different object from
    /// this one.
    /// The `false` case still carries an object rather than nothing, because
    /// a reduction that changes nothing at this level may still have recorded
    /// the components it simplified, and those records are on the returned
    /// clone.
    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        // C's `Simplify` vtab slot; the default is C's base `Simplify`
        // (mapping.c:16752). Its only caller is astSimplify_ (mapping.c:24772),
        // as `ViaSimplify` makes `<dyn Mapping>::simplify` the only caller
        // here. CmpMap, FrameSet and the nine Region classes override the
        // slot; the other 25 keep every reduction in `map_merge`.
        //
        // C's `Simplify` returns `astClone( this )` when nothing reduced, and
        // all three of its generic drivers test the returned pointer against
        // the input to decide whether to keep going (frameset.c:10208,
        // mapping.c:16891, cmpmap.c:1456). An owned-value port cannot make that
        // comparison after the fact, so the callee reports it. C's
        // `astClone( this )` is not a bare copy of the input: the nominate loop
        // writes `IsSimple` through aliases into `this`'s own children, and
        // those stamps are on the pointer C hands back. The port has only
        // `&self`, so it returns them on the clone.
        simplify::base_simplify(self.as_dyn_mapping())
    }

    /// The rate of change of output `ax1` with respect to input `ax2`, at the
    /// input position `at`.
    ///
    /// `ax1` and `ax2` are 1-based, and `at` holds one value per input.
    /// Classes whose rates are known exactly return them; otherwise the rate
    /// is estimated by fitting a polynomial to the Mapping in the
    /// neighborhood of `at`, choosing the size of the neighborhood that gives
    /// the most consistent gradient.
    /// This is accurate but may evaluate the Mapping 100 or more times.
    /// Returns `BAD_VALUE` when the rate cannot be found, for instance when
    /// `at` holds a bad value.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `ax1` is not an output or `ax2` not an
    ///   input.
    /// - `ErrorCode::Ncpin` when `at` does not hold one value per input.
    /// - `ErrorCode::Nodef` when an estimate is needed and the forward
    ///   transformation is not defined.
    #[doc(alias = "astRate")]
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
    /// (nominated) mapping with its neighbors.
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
    /// of the nominated component with its right neighbor
    /// ([`simplify::default_map_merge`]) — the "direct merge" step (C's
    /// `MapMerge` step 2) shared by every class. Classes that additionally
    /// swap past, seek toward, or rewrite distant neighbors override this
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

    /// Split off the part of this Mapping that connects a chosen set of
    /// inputs to the outputs they feed.
    ///
    /// Given the 1-based indices of the inputs to pick, in any order, returns
    /// the 1-based indices of the outputs of this Mapping that those inputs
    /// feed, and a new Mapping from the picked inputs to those outputs: its
    /// `i`th output is output `outputs[i]` of this Mapping, and it has one
    /// input per entry of `in_axes`, in that order.
    /// The new Mapping is independent of this one.
    ///
    /// A split exists only if some subset of the outputs depends on the
    /// picked inputs alone, and no other output depends on any of them.
    /// `Ok(None)` means there is no such subset.
    ///
    /// # Errors
    ///
    /// `ErrorCode::Axiin` for an index that is not an input, from the classes
    /// that check it; the other classes return `Ok(None)` for one.
    #[doc(alias = "astMapSplit")]
    fn map_split(&self, in_axes: &[usize]) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        // A thin wrapper over `map_split_zero_based`, which implements the
        // algorithm in C's protected, 0-based encoding: C keeps a 0-based
        // protected `MapSplit` (mapping.c:7798) behind the 1-based public
        // `astMapSplitId_` (mapping.c:25313). Whether an index out of range is
        // an error or no split depends on the class, as in C: the base
        // `MapSplit` and the classes that call it first report it, and the
        // others return no split. A repeated index is likewise handled as
        // each class's C handles it.
        //
        // Decrement to 0-based (astMapSplitId_, mapping.c:25429). C turns an
        // index of 0 into -1, which every class's range check refuses; the
        // wrapped `usize::MAX` is refused by the same checks.
        let in_zero: Vec<usize> = in_axes.iter().map(|&ax| ax.wrapping_sub(1)).collect();
        // Increment the returned output axes back to 1-based (mapping.c:25437).
        Ok(self
            .map_split_zero_based(&in_zero)?
            .map(|(out_zero, sub)| (out_zero.into_iter().map(|o| o + 1).collect(), sub)))
    }

    /// The **0-based** implementation of [`Mapping::map_split`], which
    /// concrete classes override.
    ///
    /// Both the supplied `in_axes` and the returned output indices are
    /// 0-based; [`Mapping::map_split`] converts at the boundary.
    ///
    /// The default reports an index out of range as `ErrorCode::Axiin`, and
    /// splits only a selection of every input, each once, by composing a
    /// [`PermMap`] with `self`. Classes that can split partial selections
    /// override it.
    fn map_split_zero_based(
        &self,
        in_axes: &[usize],
    ) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        // C's protected `MapSplit` (mapping.c:7798); the default is its base
        // implementation.
        helpers::base_map_split(self, in_axes)
    }

    /// A linear approximation to the forward transformation over a box of
    /// input coordinates, if the transformation is linear there.
    ///
    /// The box runs from `lbnd` to `ubnd` on each input.
    /// Returns `Some(fit)` when a linear fit deviates from the transformation
    /// by no more than `tol` at every point tested, `tol` being a Cartesian
    /// distance in the output space; `None` otherwise.
    /// To fit the inverse transformation, [`invert`](Mapping::invert) the
    /// Mapping first.
    ///
    /// An output that is bad at any of the points tested has its offset and
    /// gradients in the fit set to `BAD_VALUE`, but does not stop the other
    /// outputs being fitted, so check the coefficients before using them.
    /// When every output is bad, the result is `None`.
    ///
    /// The transformation is sampled at test points spread over the box and
    /// the fit made to them.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Ncpin` when `lbnd` or `ubnd` does not have one value per
    ///   input.
    /// - An error transforming the sample points is returned.
    #[doc(alias = "astLinearApprox")]
    fn linear_approx(&self, lbnd: &[f64], ubnd: &[f64], tol: f64) -> AstResult<Option<LinearFit>> {
        linear_approx::linear_approx_default(self, lbnd, ubnd, tol)
    }

    /// A quadratic least-squares fit to the forward transformation of a
    /// Mapping with 2 inputs.
    ///
    /// Each output is modeled as
    /// `a0 + a1*x + a2*y + a3*x*y + a4*x*x + a5*y*y` and fitted to a grid of
    /// `nx` × `ny` positions spanning `lbnd` to `ubnd`, with the first and
    /// last positions on the bounds; an `nx` or `ny` below 3 is taken as 3.
    /// Positions where an output is bad are left out of that output's fit.
    /// The result holds the coefficients, the RMS residual over every fitted
    /// output, and whether every output was fitted.
    /// To fit the inverse transformation, [`invert`](Mapping::invert) the
    /// Mapping first.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badni` when the Mapping does not have exactly 2 inputs.
    /// - An error transforming the grid is returned.
    #[doc(alias = "astQuadApprox")]
    fn quad_approx(
        &self,
        lbnd: [f64; 2],
        ubnd: [f64; 2],
        nx: usize,
        ny: usize,
    ) -> AstResult<QuadFit> {
        quad_approx::quad_approx(self, lbnd, ubnd, nx, ny)
    }

    /// Transform every position of a regular grid.
    ///
    /// The grid holds the integer positions from `lbnd` to `ubnd` inclusive
    /// on each input axis, each position being the center of a pixel of unit
    /// extent.
    /// The result has one point per position, the first axis varying
    /// fastest, with one coordinate per output of the transformation used:
    /// the forward one when `forward` is true and the inverse otherwise.
    ///
    /// When `tol` is positive, the Mapping is replaced by linear fits over
    /// sections of the grid wherever they are accurate to within `tol` (a
    /// distance in the output space), which is much faster for large grids.
    /// A fit is first tried over the whole grid; where it is not accurate
    /// enough, the section is divided in two along its longest axis and the
    /// fit tried again, until a fit is found or the section is too small,
    /// when the Mapping is used directly.
    /// A large `tol` makes the joins between adjacent sections more visible.
    /// A `tol` of zero transforms every position through the Mapping.
    ///
    /// Before any fit is tried, the grid is divided until no section is
    /// longer than `maxpix` positions on any axis.
    /// Normally `maxpix` is larger than the grid, so that the first fit is
    /// tried over all of it; a value of 50 to 100 guards against a Mapping
    /// that looks linear on large scales but has small-scale irregularities,
    /// at little cost, and a value too small to fit anything gives the
    /// result of a zero `tol`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Ncpin` when the bounds differ in length, or do not have
    ///   one value per input of the transformation used.
    /// - `ErrorCode::Gbdin` when a lower bound is above its upper bound.
    /// - `ErrorCode::Exspix` when the grid has too many positions.
    /// - `ErrorCode::Patin` when `tol` is negative.
    /// - `ErrorCode::Trnnd` when the transformation used is not defined.
    #[doc(alias = "astTranGrid")]
    fn tran_grid(
        &self,
        lbnd: &[i64],
        ubnd: &[i64],
        tol: f64,
        maxpix: usize,
        forward: bool,
    ) -> AstResult<PointSet> {
        tran_grid::tran_grid(self.as_dyn_mapping(), lbnd, ubnd, tol, maxpix, forward)
    }

    /// The bounds of one output coordinate over a box of input coordinates.
    ///
    /// Finds the lowest and highest values that output `coord_out` (1-based)
    /// takes as the input ranges over the box from `lbnd_in` to `ubnd_in`, and
    /// an input position (not necessarily unique) where each is reached.
    /// A lower bound above its upper bound is swapped with it.
    /// Calling this once per output gives the bounding box of the
    /// transformed input box, for instance the size of an image after it has
    /// been transformed.
    ///
    /// With `forward` false the inverse transformation is used, and inputs
    /// and outputs exchange roles: the bounds have one value per output of
    /// the Mapping, and `coord_out` names one of its inputs.
    ///
    /// Input positions that transform to a bad value on any output are
    /// ignored, even when the nominated output is good there.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badci` when `coord_out` is not an output, or a bound has
    ///   too few values.
    /// - `ErrorCode::Mbbnf` when no bounds are found, typically because every
    ///   position tested transformed to a bad value; many positions are
    ///   tested, so this is unlikely unless good positions are confined to a
    ///   very small part of the box.
    #[doc(alias = "astMapBox")]
    fn map_box(
        &self,
        lbnd_in: &[f64],
        ubnd_in: &[f64],
        forward: bool,
        coord_out: usize,
    ) -> AstResult<MapBoxResult> {
        // C's public interface takes a 1-based index and the method a 0-based
        // one (mapping.c astMapBoxId_); an index of 0 wraps past every output
        // and is refused there with the others. The default simplifies the
        // Mapping, runs C's SpecialBounds (when nin <= 12) for an initial
        // estimate, then refines with its GlobalBounds simplex search
        // (mapping.c:7121).
        map_box::map_box_default(self, lbnd_in, ubnd_in, forward, coord_out.wrapping_sub(1))
    }

    /// Whether the Mapping can transform coordinates in the forward
    /// direction, from input coordinates to output coordinates.
    ///
    /// Transforming in the forward direction is an `ErrorCode::Trnnd` error
    /// when this is false.
    /// Inverting the Mapping exchanges this with
    /// [`has_inverse`](Mapping::has_inverse).
    /// A [`CmpMap`] derives it from its components.
    /// A [`FrameSet`](crate::frames::FrameSet) describes its own.
    #[doc(alias = "TranForward")]
    fn has_forward(&self) -> bool {
        true
    }

    /// Whether the Mapping is linear: true for a class whose transformation
    /// is always linear.
    ///
    /// A class that can represent either linear or non-linear
    /// transformations, such as [`MathMap`], reports false; whether a
    /// particular instance is linear over a region can be tested with
    /// [`linear_approx`](Mapping::linear_approx).
    /// A [`CmpMap`] derives it from its components.
    /// A [`FrameSet`](crate::frames::FrameSet) describes its own.
    #[doc(alias = "IsLinear")]
    fn is_linear(&self) -> bool {
        // The base class does not claim it (mapping.c `GetIsLinear`); the
        // classes C marks linear override it.
        false
    }

    /// This Mapping with every Region it holds replaced by a UnitMap, or
    /// `None` when it holds none: C's `RemoveRegions` method, whose callers
    /// test for an unchanged Mapping by finding the same pointer returned.
    /// Regions and the Mappings that hold other Mappings or Frames override
    /// it, as in C; [`crate::regions::remove_regions`] is the public entry.
    #[doc(hidden)]
    fn mapping_without_regions(&self) -> Option<DynMapping> {
        None
    }

    /// Whether the Mapping can transform coordinates in the inverse
    /// direction, from output coordinates back to input coordinates.
    ///
    /// Transforming in the inverse direction is an `ErrorCode::Trnnd` error
    /// when this is false.
    /// Inverting the Mapping exchanges this with
    /// [`has_forward`](Mapping::has_forward).
    /// A [`CmpMap`] derives it from its components.
    /// A [`FrameSet`](crate::frames::FrameSet) describes its own.
    #[doc(alias = "TranInverse")]
    fn has_inverse(&self) -> bool {
        true
    }

    /// Whether the forward and inverse transformations are defined, before
    /// any inversion, as written to a Native dump.
    ///
    /// Every class but [`CmpMap`] writes both as defined, whatever
    /// [`has_forward`](Mapping::has_forward) and
    /// [`has_inverse`](Mapping::has_inverse) report.
    fn dump_tran_flags(&self) -> (bool, bool) {
        // C dumps `Fwd`/`Inv` from the Mapping base-struct fields, which only
        // CmpMap populates with computed values at construction — every other
        // class passes `(1, 1)` to `astInitMapping` and reports availability
        // dynamically via `GetTranForward` / `GetTranInverse`, which the Dump
        // function never consults. Override only where the C class passes
        // computed flags.
        (true, true)
    }

    /// Whether the Mapping has been simplified **in its current
    /// orientation**, so that simplifying it again would gain nothing.
    ///
    /// `<dyn Mapping>::simplify` records this on its result, and returns a
    /// Mapping that has it unchanged.
    /// The inverse of a Mapping may simplify differently, so each orientation
    /// has its own record: inverting a simplified Mapping makes this false,
    /// and inverting it back makes it true again.
    /// It is written to a Native dump as the `IsSimp` item.
    ///
    /// A FrameSet and every Region report false, since a Frame can be changed
    /// in ways that alter the Mapping it represents, so no record of having
    /// been simplified could be relied on.
    #[doc(alias = "IsSimple")]
    fn has_been_simplified(&self) -> bool {
        // C overrides the getter on the Frame class to return zero
        // unconditionally (frame.c:5590). Frame-kind classes hold no
        // `SimplifiedRecord` for the same reason, and
        // `test_simplify_regression.rs` gates the resulting absence of an
        // `IsSimp` card on every Frame-kind result in the corpus.
        false
    }

    /// Set (or clear) the record read by
    /// [`has_been_simplified`](Mapping::has_been_simplified) for the current
    /// orientation.
    ///
    /// `<dyn Mapping>::simplify` sets it `true` on its result. A class that
    /// keeps no record ignores this.
    fn set_has_been_simplified(&mut self, _flag: bool) {
        // C `astSetIsSimple` (mapping.h:428). Discarding the record is a
        // class-level decision, made through `SimplifiedRecord::clear`.
    }

    /// Whether the `IsSimp` item is written for this Mapping in a Native
    /// dump.
    ///
    /// For every class but [`CmpMap`] this is
    /// [`has_been_simplified`](Mapping::has_been_simplified).
    fn dump_is_simple(&self) -> bool {
        // C reads one record for both this and the astSimplify short-circuit.
        // CmpMap splits them — see `CmpMap::dump_simplified` for why.
        self.has_been_simplified()
    }

    /// Record `flag` for the named orientation, whatever orientation this
    /// Mapping is currently in.
    ///
    /// The Mapping is inverted, if need be, to reach that orientation, has
    /// the record set, and is inverted back; inverting does not discard the
    /// record.
    fn set_simplified_in(&mut self, inverted: bool, flag: bool) {
        // C's idiom for tagging a Mapping the caller is holding at the wrong
        // orientation — set `Invert`, set the flag, set `Invert` back — which
        // works only because setting `Invert` no longer discards the record
        // (mapping.c:23272). The engine needs it because it normalises a
        // simplified component to the forward orientation while the tree
        // still serialises that component inverted.
        let restore = self.is_inverted();
        if restore != inverted {
            self.invert();
        }
        self.set_has_been_simplified(flag);
        if restore != inverted {
            self.invert();
        }
    }

    /// Whether simplification of this mapping is restricted.
    ///
    /// When set, a simplification of this mapping may nominate only those
    /// components that carry [`allow_simplify`](Mapping::allow_simplify), and
    /// declines outright for a mapping that does not.
    /// A simplification clears it on its result.
    /// Written to a Native dump as the `ReSimp` item.
    fn restricted_simplify(&self) -> bool {
        // C's protected `RestrictedSimplify`:
        // `AST__RESTRICTED_SIMPLIFY_FLAG` (mapping.h), kept in the Mapping
        // `flags` word as the port keeps it in `MappingFlags`; the base driver
        // declines at mapping.c:16853-16855.
        self.mapping_flags().restricted_simplify
    }

    /// Set (or clear) the
    /// [`restricted_simplify`](Mapping::restricted_simplify) flag.
    fn set_restricted_simplify(&mut self, flag: bool) {
        self.mapping_flags_mut().restricted_simplify = flag;
    }

    /// The flags and attributes every Mapping carries on the C base class:
    /// `Report`, `AllowSimplify` and `RestrictedSimplify` (mapping.c).
    #[doc(hidden)]
    fn mapping_flags(&self) -> &MappingFlags;

    /// The [`MappingFlags`], to change.
    #[doc(hidden)]
    fn mapping_flags_mut(&mut self) -> &mut MappingFlags;

    /// The stored `Report` attribute, `None` when unset.
    #[doc(hidden)]
    fn report_attr(&self) -> Option<bool> {
        self.mapping_flags().report
    }

    /// Store the `Report` attribute; `None` clears it.
    #[doc(hidden)]
    fn set_report_attr(&mut self, value: Option<bool>) {
        self.mapping_flags_mut().report = value;
    }

    /// Whether [`tran1`](Mapping::tran1) and [`tran2`](Mapping::tran2) report
    /// the coordinates of each point before and after it is transformed, one
    /// line per point, to the thread's report writer
    /// ([`report::set_report_sink`], standard output by default).
    ///
    /// This is an aid to debugging, and saves reporting values explicitly in
    /// simple programs.
    /// [`transform`](Mapping::transform) and
    /// [`transform_into`](Mapping::transform_into) do not report.
    ///
    /// Default: false.
    ///
    /// Only the Report of the Mapping transformed is used, not that of any
    /// Mapping it holds, such as a [`CmpMap`]'s components.
    /// A [`FrameSet`](crate::frames::FrameSet) formats the input and output
    /// coordinates with its base and current Frames, and a Region with its
    /// Frame, as [`report_points`](Mapping::report_points) describes.
    /// A clone keeps the value, and a Native dump records it when set.
    ///
    /// [`set_report`](Mapping::set_report),
    /// [`test_report`](Mapping::test_report) and
    /// [`clear_report`](Mapping::clear_report) set, test and clear it.
    #[doc(alias = "Report")]
    fn report(&self) -> bool {
        self.report_attr().unwrap_or(false)
    }

    /// Set [`report`](Mapping::report).
    fn set_report(&mut self, value: bool) {
        self.set_report_attr(Some(value));
    }

    /// Whether [`report`](Mapping::report) is set.
    fn test_report(&self) -> bool {
        self.report_attr().is_some()
    }

    /// Clear [`report`](Mapping::report).
    fn clear_report(&mut self) {
        self.set_report_attr(None);
    }

    /// Write the effect of a transformation, `inp` mapped to `out`, to the
    /// thread's report writer.
    ///
    /// A Mapping writes each value to 17 significant digits or as `<bad>`; a
    /// FrameSet formats them with its base and current Frames, and a Region
    /// with its current Frame.
    fn report_points(&self, forward: bool, inp: &PointSet, out: &PointSet) {
        // C `astReportPoints`, a per-class method.
        let _ = forward;
        report::write_report(&report::report_lines(
            inp,
            out,
            report::plain_value,
            report::plain_value,
        ));
    }

    /// Whether this mapping may be merged by a restricted simplification.
    ///
    /// Only consulted when the enclosing CmpMap has
    /// [`restricted_simplify`](Mapping::restricted_simplify) set: under a
    /// restricted simplify, only components with this flag may be nominated
    /// for merging.
    /// It is cleared on each component as a simplified sequence is folded back
    /// into a CmpMap, and on the result of every simplification.
    /// Written to a Native dump as the `AlSimp` item.
    fn allow_simplify(&self) -> bool {
        // C's protected `AllowSimplify`:
        // `AST__ALLOW_SIMPLIFY_FLAG` (mapping.h); cleared at cmpmap.c:3692
        // and mapping.c:24788, so it never reaches a public dump.
        self.mapping_flags().allow_simplify
    }

    /// Set (or clear) the [`allow_simplify`](Mapping::allow_simplify) flag.
    fn set_allow_simplify(&mut self, flag: bool) {
        self.mapping_flags_mut().allow_simplify = flag;
    }

    /// The Ident attribute: a permanent string identifying this mapping.
    ///
    /// The library places no restriction on its contents.
    /// A clone keeps it, and it is written to and read back from a Native
    /// dump, so it can identify an object after either.
    /// A Mapping with Ident set is left intact by simplification (see
    /// [`do_not_simplify`](Mapping::do_not_simplify)).
    /// Default: an empty string.
    ///
    /// A class that is also a [`Frame`](crate::frames::Frame) holds one
    /// Ident, which [`Frame::ident`](crate::frames::Frame::ident) reads too.
    #[doc(alias = "Ident")]
    fn ident(&self) -> &str {
        // C keeps it on Object (object.c:5221), so it applies to every class.
        ""
    }

    /// Set [`ident`](Mapping::ident).
    /// An empty string counts as set.
    ///
    /// The default does nothing; every Mapping class in this crate overrides
    /// it.
    fn set_ident(&mut self, _ident: &str) {
        // C `SetIdent` (object.c:5225).
    }

    /// Clear [`ident`](Mapping::ident), restoring the unset state.
    fn clear_ident(&mut self) {
        // C `ClearIdent` (object.c:5217).
    }

    /// Whether [`ident`](Mapping::ident) has been set, even to an empty
    /// string.
    fn test_ident(&self) -> bool {
        // C `TestIdent` (object.c:5229) tests the stored pointer rather than
        // the string's contents.
        false
    }

    /// The ID attribute: a string identifying this mapping.
    ///
    /// The library places no restriction on its contents and does not use
    /// it.
    /// It is written to and read back from a Native dump, so consistent IDs
    /// identify objects read from a file without depending on the order they
    /// were written in.
    /// Unlike [`ident`](Mapping::ident), a clone does not keep it: the
    /// clone's ID is unset.
    /// Default: an empty string.
    ///
    /// A class that is also a [`Frame`](crate::frames::Frame) holds one ID,
    /// which [`Frame::id`](crate::frames::Frame::id) reads too.
    // C keeps it on Object (object.c:5175), and `astCopy` does not copy it.
    #[doc(alias = "ID")]
    fn id(&self) -> &str;

    /// Set [`id`](Mapping::id).
    fn set_id(&mut self, id: &str);

    /// Clear [`id`](Mapping::id).
    fn clear_id(&mut self);

    /// Whether [`id`](Mapping::id) has been set.
    fn test_id(&self) -> bool;

    /// Whether this mapping must be left intact by a simplification.
    ///
    /// True when [`ident`](Mapping::ident) is set, so that a Mapping a user
    /// has named keeps its identity.
    /// A FrameSet and every Region return false, so a named one is still
    /// simplified.
    fn do_not_simplify(&self) -> bool {
        // C `astDoNotSimplify` (mapping.c:1107), consulted by both of C's
        // generic drivers — the astSimplify_ gate (mapping.c:24764) and
        // astMapList's refusal to decompose a CmpMap (cmpmap.c:1154). The base
        // rule is at mapping.c:1150; `Frame` overrides it to `false`
        // (frame.c:3511-3554).
        self.test_ident()
    }

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
        validate_transform(self, pts, forward, None)?;
        // Boundary substitution: BAD_VALUE → NaN. Allocates a new PointSet
        // rather than mutating the caller's input. For very large inputs we
        // could add an in-place variant if profiling motivates it.
        let pts_nan = substitute_bad_to_nan(pts);

        let nout = if forward { self.nout() } else { self.nin() };
        let mut out = PointSet::zeros(pts.npoint(), nout);
        self.transform_into_nan(pts_nan.as_points(), forward, out.as_points_mut())?;

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
/// place, as C's `astReplaceNaN` does with AST_REPLACE_NAN set, which C's
/// `astTransform` applies to every result (mapping.c `astTransform_`). Used
/// at the output boundary of [`dyn Mapping::transform_nan_protocol`] to
/// restore the canonical sentinel value before returning to the caller, and
/// wherever a port follows C through an internal `astTransform`.
pub(crate) fn substitute_nan_to_bad_in_place(pts: &mut PointSet) {
    use crate::constants::BAD_VALUE;
    let mut view = pts.view_mut();
    let flat: &mut [f64] = view.as_slice_mut().expect("PointSet is C-contiguous");
    for v in flat.iter_mut() {
        if !v.is_finite() {
            *v = BAD_VALUE;
        }
    }
}

/// The checks C's public `astRate` makes before any class finds a rate
/// (mapping.c `astRate_`): `ax1` must name an output and `ax2` an input,
/// both 1-based, or it is `AST__AXIIN`. `at` must also hold a value for every
/// input, which a C array cannot be checked for (`AST__NCPIN`).
pub(crate) fn validate_rate<M: Mapping + ?Sized>(
    map: &M,
    at: &[f64],
    ax1: usize,
    ax2: usize,
) -> AstResult<()> {
    let (nin, nout) = (map.nin(), map.nout());
    if ax1 < 1 || ax1 > nout {
        return Err(AstError::new(
            ErrorCode::Axiin,
            format!("astRate: invalid output index ({ax1}); it should be in the range 1 to {nout}"),
        ));
    }
    if ax2 < 1 || ax2 > nin {
        return Err(AstError::new(
            ErrorCode::Axiin,
            format!("astRate: invalid input index ({ax2}); it should be in the range 1 to {nin}"),
        ));
    }
    if at.len() != nin {
        return Err(AstError::new(
            ErrorCode::Ncpin,
            format!(
                "astRate: {} input values supplied; the Mapping has {nin} inputs",
                at.len()
            ),
        ));
    }
    Ok(())
}

/// The checks C's base-class `Transform` makes before any class transforms a
/// point (mapping.c:22333-22380), which every class's `Transform` calls
/// first: the requested direction must be defined (`AST__TRNND`), the input
/// must have one coordinate per Mapping input (`AST__NCPIN`), and a supplied
/// output must hold every point (`AST__NOPTS`) and every output coordinate
/// (`AST__NOCTS`).
pub(crate) fn validate_transform<M: Mapping + ?Sized>(
    map: &M,
    pts: &impl PointShape,
    forward: bool,
    out: Option<&dyn PointShape>,
) -> AstResult<()> {
    match out {
        Some(out) => validate_transform_into(map, pts, forward, Some(out)),
        None => validate_transform_into(map, pts, forward, None::<&PointSet>),
    }
}

/// Set each `BAD_VALUE` that `map` wrote for `npoint` points in direction
/// `forward` to NaN, for a `transform_into_nan` built on `transform_into`.
/// Only the points and axes the transformation writes are touched.
pub(crate) fn bad_to_nan<M: Mapping + ?Sized>(
    map: &M,
    forward: bool,
    npoint: usize,
    out: &mut PointsMut<'_>,
) {
    let nout = if forward { map.nout() } else { map.nin() };
    for axis in out.axes_iter_mut().take(nout) {
        for v in &mut axis[..npoint] {
            if *v == crate::constants::BAD_VALUE {
                *v = f64::NAN;
            }
        }
    }
}

/// [`validate_transform`] with the output's type known, so its checks
/// inline into each class's `transform_into`.
#[inline]
pub(crate) fn validate_transform_into<M: Mapping + ?Sized, O: PointShape + ?Sized>(
    map: &M,
    pts: &impl PointShape,
    forward: bool,
    out: Option<&O>,
) -> AstResult<()> {
    require_direction(map, forward, "astTransform")?;
    let (nin, nout) = if forward {
        (map.nin(), map.nout())
    } else {
        (map.nout(), map.nin())
    };
    if pts.ncoord() != nin {
        return Err(AstError::new(
            ErrorCode::Ncpin,
            format!(
                "astTransform: bad number of coordinate values ({}) in the input; \
                 the Mapping requires {nin} for each point",
                pts.ncoord()
            ),
        ));
    }
    if let Some(out) = out {
        if out.npoint() < pts.npoint() {
            return Err(AstError::new(
                ErrorCode::Nopts,
                format!(
                    "astTransform: too few points ({}) in the output; {} are needed",
                    out.npoint(),
                    pts.npoint()
                ),
            ));
        }
        if out.ncoord() < nout {
            return Err(AstError::new(
                ErrorCode::Nocts,
                format!(
                    "astTransform: too few coordinate values per point ({}) in the \
                     output; {nout} are needed",
                    out.ncoord()
                ),
            ));
        }
    }
    Ok(())
}

/// An `AST__TRNND` error when `map` does not define the requested direction,
/// as C's base `Transform` reports (mapping.c).
pub(crate) fn require_direction<M: Mapping + ?Sized>(
    map: &M,
    forward: bool,
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
    Ok(())
}

/// Shared argument validation for [`Mapping::tran1`] and [`Mapping::tran2`].
///
/// Port of the relevant arms of C's `ValidateMapping` (`mapping.c`), which
/// `astTran1` / `astTran2` call before building their PointSets:
///
/// - `AST__TRNND` when the requested direction is not defined,
/// - `AST__NCPIN` when `Nin` or `Nout` is not the arity the entry point
///   requires,
/// - `AST__NPTIN` when fewer than one point is supplied. `ValidateMapping`
///   itself refuses only a negative count; C refuses zero when
///   `astInitPointSet` builds the PointSet (pointset.c:3011).
///
/// C raises `AST__TRNND` in its base `Transform` too (mapping.c:22333-22380),
/// which every class's `Transform` calls; the port's equivalent is
/// [`validate_transform`].
fn validate_tran<M: Mapping + ?Sized>(
    map: &M,
    forward: bool,
    npoint: usize,
    arity: usize,
    method: &str,
) -> AstResult<()> {
    require_direction(map, forward, method)?;
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

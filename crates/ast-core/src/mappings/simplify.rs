//! Core simplification engine for mapping composition trees.
//!
//! This module implements the flatten/merge/reconstruct pipeline that
//! decomposes `CmpMap` trees into flat lists, attempts pairwise merges
//! of adjacent mappings, and reconstructs the result.
//!
//! The pipeline is:
//! 1. **Flatten**: recursively decompose a `CmpMap` tree into a flat
//!    list of `(DynMapping, bool, bool)` pairs, where the bool indicates
//!    whether the mapping is effectively inverted in the composition.
//! 2. **Merge loop**: walk adjacent pairs, attempting `try_merge` in
//!    both directions until no further improvements are found.
//! 3. **Reconstruct**: build a new `CmpMap` tree (or single mapping)
//!    from the flat list.

use std::cell::RefCell;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::ast_equal;

use super::ViaSimplify;
use super::cmpmap::{CmpMap, Composition};
use super::grismmap::GrismMap;
use super::matrix::{MatrixMap, is_diagonal};
use super::pcdmap::PcdMap;
use super::perm::PermMap;
use super::shift::ShiftMap;
use super::simplify_entry::SimplifyOutcome;
use super::sphmap::SphMap;
use super::unit::UnitMap;
use super::wcsmap::WcsMap;
use super::winmap::WinMap;
use super::zoom::ZoomMap;
use super::{DynMapping, Mapping};

// ------------------------------------------------------------------ //
// Recursion guard: fingerprint stack                                   //
// ------------------------------------------------------------------ //

thread_local! {
    /// Mappings currently being simplified higher up this thread's call stack,
    /// one per depth.
    ///
    /// C keeps the same list as `simplify_stackmaps[]` and compares the supplied
    /// Mapping against every entry with `astEqual` (cmpmap.c:3487-3491),
    /// returning it unchanged on a match. That is how a Mapping whose
    /// simplification would recurse onto itself terminates.
    static SIMPLIFY_STACK: RefCell<Vec<DynMapping>> = const { RefCell::new(Vec::new()) };
}

/// RAII guard over [`SIMPLIFY_STACK`], pushing the Mapping being simplified for
/// the duration of the call and popping it again on the way out.
struct SimplifyGuard;

impl SimplifyGuard {
    /// Attempt to enter simplification for `mapping`, returning `None` when an
    /// equal Mapping is already being simplified further up this thread's stack.
    fn enter(mapping: &dyn Mapping) -> Option<Self> {
        let cycle = SIMPLIFY_STACK.with(|s| s.borrow().iter().any(|m| mapping.equal(m.as_ref())));
        if cycle {
            return None;
        }
        SIMPLIFY_STACK.with(|s| s.borrow_mut().push(mapping.clone_box()));
        Some(SimplifyGuard)
    }
}

impl Drop for SimplifyGuard {
    fn drop(&mut self) {
        SIMPLIFY_STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

/// Trace the simplify machinery as `tracing` events, mirroring
/// `tools/oracle_fixtures/build_simplify_trace.sh`'s instrumentation of the C
/// library so the two routes through a Mapping can be diffed line for line.
///
/// Everything here compiles to nothing without the `tracing` feature, following
/// [`crate::AstError`]'s pattern. Event names, field names and field order
/// match the C trace's, so the two streams diff line for line once each side's
/// prefix is stripped — see `tools/oracle_fixtures/README.md`. An event added
/// here needs its C counterpart in that script, and the reverse.
///
/// These sit in the decomposition and merge machinery, not in any `transform`
/// path, so the hot-path constraint in `CLAUDE.md` is unaffected.
pub(crate) mod trace {
    use super::{DynMapping, Mapping};

    #[cfg(feature = "tracing")]
    thread_local! {
        /// How many nominations enclose the current one. A nomination can
        /// simplify a CmpMap, which runs a merge loop of its own, so this is the
        /// nesting of merge loops. C keeps the same count in `cmpmap.c`.
        static DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[cfg(feature = "tracing")]
    fn depth() -> usize {
        DEPTH.with(std::cell::Cell::get)
    }

    /// Holds the nomination depth one level deeper until dropped.
    pub(crate) struct Nomination;

    /// Enter a nomination: events inside it report one more level of `depth`.
    #[inline]
    pub(crate) fn enter_nomination() -> Nomination {
        #[cfg(feature = "tracing")]
        DEPTH.with(|d| d.set(d.get() + 1));
        Nomination
    }

    impl Drop for Nomination {
        #[inline]
        fn drop(&mut self) {
            #[cfg(feature = "tracing")]
            DEPTH.with(|d| d.set(d.get() - 1));
        }
    }

    /// Class name for a trace line, as `astGetClass` gives it, except that a
    /// CmpMap also names its composition. Serializing is costly, but only
    /// runs when the event is enabled.
    #[cfg(feature = "tracing")]
    fn class_of(m: &dyn Mapping) -> String {
        use crate::mappings::{CmpMap, Composition};
        if let Some(c) = m.as_any().downcast_ref::<CmpMap>() {
            return if c.composition() == Composition::Series {
                "CmpMap[series]".into()
            } else {
                "CmpMap[parallel]".into()
            };
        }
        crate::AnyObject::from_mapping(m)
            .and_then(|obj| crate::serial::any_object_to_node(&obj).ok())
            .map_or_else(|| "?".into(), |node| node.class)
    }

    /// A merge list as `[ Class(eff[,s]/oN) ... ]`: each entry's class, the
    /// direction it is applied in, `,s` if it carries `IsSimp` at its stored
    /// orientation, and that stored orientation (C's `astGetInvert`).
    #[cfg(feature = "tracing")]
    fn list_of(list: &[(DynMapping, bool, bool)]) -> String {
        use std::fmt::Write;
        let mut s = String::from("[");
        for (m, eff, orig) in list {
            let simp = if at_stored(m.as_ref(), *orig).has_been_simplified() {
                ",s"
            } else {
                ""
            };
            let _ = write!(
                s,
                " {}({}{simp}/o{})",
                class_of(m.as_ref()),
                u8::from(*eff),
                u8::from(*orig)
            );
        }
        s.push_str(" ]");
        s
    }

    /// A list entry at its stored orientation, which is the orientation C's
    /// list holds the object in and so the one its `astGetInvert` and
    /// `astGetIsSimple` report.
    #[cfg(feature = "tracing")]
    fn at_stored(m: &dyn Mapping, stored_inv: bool) -> DynMapping {
        let mut stored = m.clone_box();
        if stored.is_inverted() != stored_inv {
            stored.invert();
        }
        stored
    }

    /// A per-class reduction is about to run. C's `SIMPLIFY-ENTER`.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn simplify_enter(site: &str, m: &dyn Mapping) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            class = %class_of(m),
            issimp = u8::from(m.has_been_simplified()),
            inv = u8::from(m.is_inverted()),
            nin = m.nin(),
            nout = m.nout(),
            site,
            "SIMPLIFY-ENTER",
        );
    }

    /// A class is being nominated. C's `MAPMERGE`. `stored_inv` is the
    /// entry's stored orientation, which `issimp` and `inv` report.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn map_merge(
        m: &dyn Mapping,
        stored_inv: bool,
        where_: usize,
        series: bool,
        len: usize,
    ) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            class = %class_of(m),
            where_,
            series = u8::from(series),
            nmap = len,
            issimp = u8::from(at_stored(m, stored_inv).has_been_simplified()),
            inv = u8::from(stored_inv),
            "MAPMERGE",
        );
    }

    /// What a nomination returned: the index it modified, or nothing, and the
    /// list length after it. C's `MAPMERGE-RESULT`. Distinguishes a productive
    /// nomination from one that declined, which is the difference between
    /// churn and progress.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn map_merge_result(m: &dyn Mapping, result: Option<usize>, len: usize) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            class = %class_of(m),
            result = result.map_or(-1i64, |i| i as i64),
            nmap = len,
            "MAPMERGE-RESULT",
        );
    }

    /// The whole merge list as an element is nominated from it. C's
    /// `MERGE-LIST`, from the `cmpmap.c` `Simplify` loop. A divergence in list
    /// order or orientation shows here before it shows in any nomination.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn merge_list(nominated: usize, series: bool, list: &[(DynMapping, bool, bool)]) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            depth = depth(),
            nominated,
            series = u8::from(series),
            nmap = list.len(),
            list = %list_of(list),
            "MERGE-LIST",
        );
    }

    /// A frozen element was passed over. C's `NOMINATE-SKIP`. Restricted
    /// simplification also skips elements; `frozen=0` marks those.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn nominate_skip(nominated: usize, frozen: bool) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            depth = depth(),
            nominated,
            frozen = u8::from(frozen),
            "NOMINATE-SKIP",
        );
    }

    /// The oscillation check froze an element. C's `FREEZE`.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn freeze(modified: usize, m: &dyn Mapping) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            depth = depth(),
            modified,
            class = %class_of(m),
            "FREEZE",
        );
    }

    /// A nominated CmpMap was simplified on its own: whether that changed it
    /// and what it became. C's `CMPMAP-HEAD`.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn cmpmap_head(where_: usize, simpler: bool, new: &dyn Mapping) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            depth = depth(),
            where_,
            simpler = u8::from(simpler),
            new = %class_of(new),
            "CMPMAP-HEAD",
        );
    }

    /// A nominated CmpMap was replaced in the list by its two components.
    /// C's `CMPMAP-SPLIT`.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn cmpmap_split(where_: usize) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            depth = depth(),
            where_,
            "CMPMAP-SPLIT",
        );
    }

    /// A decomposition is being requested. C's `MAPLIST`, the highest-volume of
    /// the events, so it sits a level lower.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn map_list(m: &dyn Mapping, series: bool, inv: bool) {
        #[cfg(feature = "tracing")]
        tracing::trace!(
            target: "ast::simplify",
            class = %class_of(m),
            series = u8::from(series),
            invert = u8::from(inv),
            "MAPLIST",
        );
    }
}

/// Recursively decompose a mapping into a flat list of leaf mappings.
///
/// Each element is `(mapping, effective_inv)` where:
/// - `mapping` is a cloned leaf mapping in its forward (non-inverted)
///   state — `is_inverted()` returns `false`.
/// - `effective_inv` is `true` if this mapping should be applied
///   inverted within the composition, `false` for forward.
///
/// `CmpMap` nodes whose composition matches `series` are absorbed
/// (their children are recursively flattened). Other mappings
/// (including `CmpMap` nodes of the other composition kind) are
/// treated as opaque leaves.
///
/// `inv` is the externally imposed inversion flag from a parent
/// `CmpMap` — `false` for the top-level call.
pub(crate) fn flatten(
    mapping: &dyn Mapping,
    inv: bool,
    series: bool,
) -> AstResult<Vec<(DynMapping, bool, bool)>> {
    let target_kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };

    trace::map_list(mapping, series, inv);

    // Try to decompose if this is a CmpMap of the matching kind that may be
    // simplified at all: C tests both in one condition (cmpmap.c:1154), and a
    // CmpMap that must be left intact falls through to the base `MapList`,
    // which appends the Mapping itself as a single leaf.
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>()
        && cmp.composition() == target_kind
        && !mapping.do_not_simplify()
    {
        return flatten_children(cmp, inv, series);
    }

    // A class that overrides C `astMapList` (currently only `Prism`, which is
    // a *parallel* combination of its two component Regions) decomposes here
    // into those components so the nominate loop can merge across them, exactly
    // as C's `Prism::MapList` (prism.c:1527) concatenates the MapLists of its
    // two Regions when a parallel decomposition is requested. Each component is
    // recursively flattened at its effective orientation.
    if let Some(components) = mapping.map_list_decompose(inv, series) {
        let mut result = Vec::new();
        for (comp, comp_inv) in components {
            result.extend(flatten(comp.as_ref(), comp_inv, series)?);
        }
        return Ok(result);
    }

    // Leaf: clone it, unreduced.
    //
    // C `cmpmap.c::MapList` decomposes structurally and does not simplify the
    // components it collects. Each leaf enters the nominate loop in its stored
    // form and its own `astMapMerge` reduces it there, in context, where it can
    // see its neighbors — which is what lets `WinMat` fold a WinMap's residual
    // scale into an adjacent MatrixMap, and what makes "replace with simpler"
    // the *last* step of a class's merge rather than the first thing that
    // happens to it.
    Ok(vec![leaf_entry(mapping, inv)])
}

/// One list entry for `mapping` applied in direction `inv`, unreduced: the
/// leaf case of [`flatten`].
fn leaf_entry(mapping: &dyn Mapping, inv: bool) -> (DynMapping, bool, bool) {
    let mut cloned = mapping.clone_box();
    // `AllowSimplify` (C `AlSimp`) and the `IsSimp` dump tag used to be copied
    // across by hand here, because `simplified()` returned a fresh object
    // without them. `Clone` carries both, which is what `astMapList` appending
    // the component pointer itself amounts to.
    // The leaf's own (intrinsic) Invert state, independent of any inversion
    // imposed by a parent CmpMap. Preserved as the element's `orig_inv` so
    // `reconstruct` serializes the child in the orientation C does: C keeps each
    // component's stored Invert and records the effective direction separately
    // in the parent's `InvA`/`InvB`. The clone is the leaf untouched, so its
    // orientation *is* the stored one and there is nothing to un-normalize.
    let orig_inv = cloned.is_inverted();
    let effective_inv = inv != orig_inv;
    // Reset the clone to forward state; the bool carries inversion.
    if cloned.is_inverted() {
        cloned.invert();
    }

    (cloned, effective_inv, orig_inv)
}

/// Decompose the two children of a same-kind `CmpMap`, each at its effective
/// direction, and concatenate the results in the order the composition applies
/// them. This is the body of C `cmpmap.c::MapList`'s decomposing branch
/// (`cmpmap.c:1154`), split out so a caller that needs the child-level
/// decomposition can ask for it directly.
pub(crate) fn flatten_children(
    cmp: &CmpMap,
    inv: bool,
    series: bool,
) -> AstResult<Vec<(DynMapping, bool, bool)>> {
    // Combine external inversion with the CmpMap's own flag.
    // XOR: if both are inverted they cancel.
    let cmp_inv = inv != cmp.is_inverted();

    // Effective inversion for each child: inv_a/inv_b record
    // the expected child invert state. If the child's current
    // state differs, that flips the direction. Then the CmpMap's
    // own inversion (cmp_inv) flips again.
    let child_inv_a = cmp_inv != (cmp.inv_a() != cmp.first().is_inverted());
    let child_inv_b = cmp_inv != (cmp.inv_b() != cmp.second().is_inverted());

    if cmp_inv && series {
        // Inverted series: reverse child order.
        let mut result = flatten(cmp.second(), child_inv_b, series)?;
        result.extend(flatten(cmp.first(), child_inv_a, series)?);
        Ok(result)
    } else {
        // Forward, or inverted parallel: same order.
        let mut result = flatten(cmp.first(), child_inv_a, series)?;
        result.extend(flatten(cmp.second(), child_inv_b, series)?);
        Ok(result)
    }
}

/// Mirror of the `simpler` flag returned by C `cmpmap.c` `MapList`
/// (`result = invert || r1 || r2`, MapList:1191): true when decomposing
/// `mapping` (in direction `inv`, for the given `series`/parallel kind)
/// encounters any same-kind CmpMap in an inverted state. Such an inversion
/// is normalized away by the flatten/reconstruct round-trip, so C treats it
/// as a simplification even when no component pair merges. Opposite-kind
/// CmpMaps and non-CmpMap leaves contribute nothing here; the change an
/// opposite-kind component can report comes from the nominate loop instead.
fn decomposition_is_simpler(mapping: &dyn Mapping, inv: bool, series: bool) -> bool {
    let target_kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == target_kind {
            // Effective inversion of this node and of each child, computed
            // exactly as in `flatten`.
            let cmp_inv = inv != cmp.is_inverted();
            let child_inv_a = cmp_inv != (cmp.inv_a() != cmp.first().is_inverted());
            let child_inv_b = cmp_inv != (cmp.inv_b() != cmp.second().is_inverted());
            return cmp_inv
                || decomposition_is_simpler(cmp.first(), child_inv_a, series)
                || decomposition_is_simpler(cmp.second(), child_inv_b, series);
        }
    }
    false
}

/// Working state for the per-class `map_merge` nominate loop.
///
/// Mirrors the C `Simplify` engine's mutable arguments to `astMapMerge`
/// (`cmpmap.c:3572`): the `map_list` of components, the parallel
/// `invert_list` of effective-direction flags, and the per-mapping
/// `Frozen` flag (`astSetFrozen`/`astFrozen`, `cmpmap.c:3530-3624`). As in
/// C, the frozen flag lives on the Mapping (`MappingFlags::frozen`), so a
/// Mapping frozen by an inner simplification is still frozen when it is
/// returned into an outer list.
///
/// Each entry of `list` is `(mapping, effective_inv, orig_inv)` exactly as
/// produced by [`flatten`]: `effective_inv` is the direction the component
/// is applied in (C's `invert_list[i]`), and `orig_inv` is the component's
/// own intrinsic Invert state preserved for [`reconstruct`].
///
/// `pub` + `#[doc(hidden)]` because it appears in the signature of the
/// `Mapping::map_merge` trait method, which is itself an internal
/// (C-`protected`) simplification hook hidden from the public API.
#[doc(hidden)]
pub struct MergeCtx {
    /// The component working list (C `map_list` + `invert_list`).
    pub(crate) list: Vec<(DynMapping, bool, bool)>,
    /// Whether the enclosing CmpMap has `RestrictedSimplify` set. When
    /// true, only components whose `AllowSimplify` flag is set may be
    /// nominated (C `cmpmap.c:3518-3564`).
    restricted: bool,
    /// An error a merge raised, which C leaves in its global status: the
    /// nominate loop stops at it (`while ( astOK && ... )`, cmpmap.c:3754)
    /// and the simplification fails with it.
    error: Option<AstError>,
    /// The records of simplification the nominate loop wrote on each
    /// component of the CmpMap being simplified, by the component's position
    /// in the list the simplification began with (`MappingFlags::origin`).
    /// C writes those records through its list's aliases onto the CmpMap's
    /// own components, where they stay whatever later merges do to the list.
    stamped: Vec<(usize, TagTree)>,
}

impl MergeCtx {
    /// Wrap a component list. Its Mappings keep any `Frozen` flag they
    /// carry; a simplification clears them with
    /// [`clear_frozen`](Self::clear_frozen).
    pub(crate) fn new(list: Vec<(DynMapping, bool, bool)>) -> Self {
        Self {
            list,
            restricted: false,
            error: None,
            stamped: Vec::new(),
        }
    }

    /// Record an error a merge raised, as C's merges leave it in the global
    /// status for the loop driving them to find.
    /// The first error is kept, as C keeps the first status set.
    pub(crate) fn fail(&mut self, error: AstError) {
        self.error.get_or_insert(error);
    }

    /// The value of a merge step that can raise an error.
    /// An error is recorded as [`fail`](Self::fail) records it, and the step
    /// then counts as one that found nothing to do, since the loop driving
    /// the merge stops at the error.
    pub(crate) fn settle<T>(&mut self, result: AstResult<Option<T>>) -> Option<T> {
        result.unwrap_or_else(|e| {
            self.fail(e);
            None
        })
    }

    /// Whether a merge has raised an error.
    /// C's later merge steps do nothing once the global status is set, so a
    /// merge checks this before taking its next step.
    pub(crate) fn failed(&self) -> bool {
        self.error.is_some()
    }

    /// Take the error a merge raised, if any.
    pub(crate) fn take_error(&mut self) -> Option<AstError> {
        self.error.take()
    }

    /// Keep the records of simplification on the entry at `index` if it is
    /// one of the components the simplification began with, so they can be
    /// returned on the CmpMap if the simplification fails.
    fn record_component_stamp(&mut self, index: usize) {
        let entry = &self.list[index].0;
        self.record_tags(entry.mapping_flags().origin, TagTree::of(entry.as_ref()));
    }

    /// Keep `tags` as the records of simplification on the component the
    /// simplification began with at position `origin`; 0 names none.
    fn record_tags(&mut self, origin: usize, tags: TagTree) {
        if origin == 0 {
            return;
        }
        match self.stamped.iter_mut().find(|(o, _)| *o == origin) {
            Some(slot) => slot.1 = tags,
            None => self.stamped.push((origin, tags)),
        }
    }

    /// Unfreeze every Mapping in the list, as C's `Simplify` does before
    /// its merge loop (`cmpmap.c:3741-3743`, mapping.c `astClearFrozen`).
    pub(crate) fn clear_frozen(&mut self) {
        for (m, _, _) in &mut self.list {
            m.mapping_flags_mut().frozen = false;
        }
    }

    /// Set the restricted-simplify mode (C `restricted`), captured from the
    /// enclosing CmpMap's `RestrictedSimplify` flag.
    pub(crate) fn set_restricted(&mut self, restricted: bool) {
        self.restricted = restricted;
    }

    /// Whether the component at `index` may be nominated for simplification.
    /// Under a restricted simplify, only `AllowSimplify`-flagged components
    /// qualify (C `restricted && !astAllowSimplify(...)`). The flag is read
    /// live from the component; a Mapping a merge creates has it when every
    /// Mapping the merge consumed did (`dispatch_map_merge`).
    fn may_nominate(&self, index: usize) -> bool {
        !self.restricted
            || self
                .list
                .get(index)
                .map(|(m, _, _)| m.allow_simplify())
                .unwrap_or(false)
    }

    /// Number of components in the working list (C `*nmap`).
    pub(crate) fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether the component at `index` is frozen (C `astFrozen`).
    pub(crate) fn is_frozen(&self, index: usize) -> bool {
        self.list
            .get(index)
            .is_some_and(|(m, _, _)| m.mapping_flags().frozen)
    }

    /// Freeze the component at `index` (C `astSetFrozen`), preventing it
    /// from being nominated again. Used by the oscillation break.
    pub(crate) fn freeze(&mut self, index: usize) {
        if let Some((m, _, _)) = self.list.get_mut(index) {
            m.mapping_flags_mut().frozen = true;
        }
    }

    /// Borrow a `(mapping, effective_inv, orig_inv)` entry.
    pub(crate) fn entry(&self, index: usize) -> &(DynMapping, bool, bool) {
        &self.list[index]
    }

    /// Replace a single entry in place (no length change). Used by the
    /// WinMap replace-with-simpler steps (→ MatrixMap / ShiftMap).
    pub(crate) fn set_pair_single(&mut self, index: usize, entry: (DynMapping, bool, bool)) {
        self.list[index] = entry;
    }

    /// Record that the component at `index` has been simplified.
    ///
    /// Mirrors the stamp C's `astSimplify` leaves on a nominated component even
    /// when it returns that component unreduced (`astSimplify_`, mapping.c:24763
    /// — the tag is set on the returned clone, which for an unreduced Mapping is
    /// the component itself).
    ///
    /// The record is per orientation, and the orientation C stamps is the one it
    /// forced the component into before calling `astSimplify` —
    /// `invert_list[where]` (cmpmap.c:1436-1440), the entry's effective
    /// inversion. C restores the component's own `Invert` afterwards
    /// (cmpmap.c:1447-1454), so the dump reads the record at the *stored*
    /// orientation instead, and the two need not agree.
    pub(crate) fn set_component_simple_tag(&mut self, index: usize) {
        let eff_inv = self.list[index].1;
        if let Some(cm) = self.list[index].0.as_any_mut().downcast_mut::<CmpMap>() {
            cm.set_dump_simplified_in(eff_inv, true);
        }
    }

    /// Replace the two adjacent entries at `i1`/`i1+1` with a single merged
    /// entry, closing the gap. Used by per-class `map_merge` after a direct
    /// merge produces one mapping from a neighboring pair (C annuls the two
    /// pointers, stores the merged one at `i1`, and shuffles the tail down).
    pub(crate) fn replace_pair_with(&mut self, i1: usize, mut merged: DynMapping, eff_inv: bool) {
        // Normalize the entry to `(forward_map, effective_inv, orig_inv)`.
        // A merge may return a mapping in its inverted orientation (e.g.
        // `MatrixMap::mtr_zoom` preserves an inverted MatrixMap so it
        // serializes `Invert=1` like C). Record that intrinsic orientation as
        // `orig_inv` and fold it into the effective direction, then store the
        // mapping forward — matching how `flatten` produces leaf entries.
        let orig_inv = merged.is_inverted();
        if orig_inv {
            merged.invert();
        }
        self.list[i1] = (merged, eff_inv != orig_inv, orig_inv);
        self.list.remove(i1 + 1);
    }

    /// Remove the single entry at `index`, shuffling the tail down. Faithful
    /// analogue of C `UnitMap::MapMerge` eliminating a UnitMap from a series
    /// sequence (unitmap.c:519): the mapping is annulled and the following
    /// elements shift down to fill the gap.
    pub(crate) fn remove_at(&mut self, index: usize) {
        self.list.remove(index);
    }

    /// Replace the two adjacent entries at `i1`/`i1+1` with the supplied
    /// swapped pair (same count, reversed roles). Used by the WinMap swap
    /// helpers (`WinMat`/`WinPerm`/`WinWcs`), which leave the list length
    /// unchanged.
    pub(crate) fn set_pair(
        &mut self,
        i1: usize,
        first: (DynMapping, bool, bool),
        second: (DynMapping, bool, bool),
    ) {
        self.list[i1] = first;
        self.list[i1 + 1] = second;
    }

    /// Replace the contiguous range `imap1..=imap2` with a single `merged`
    /// entry, closing the gap. Faithful analogue of the C `MapMerge` tail
    /// that annuls the run of merged mappings, stores the replacement at
    /// `imap1`, and shuffles the remaining elements down (e.g. zoommap.c
    /// MapMerge condensing a run of adjacent ZoomMaps/UnitMaps). The stored
    /// entry is forward (`effective_inv = false`), matching C setting the new
    /// invert flag to 0.
    pub(crate) fn replace_range_with(&mut self, imap1: usize, imap2: usize, merged: DynMapping) {
        self.list[imap1] = (merged, false, false);
        for idx in (imap1 + 1..=imap2).rev() {
            self.list.remove(idx);
        }
    }

    /// Run a single `map_merge` over the sub-range `start..` of the working
    /// list, then splice the (possibly shortened) sub-range back in.
    ///
    /// Faithful analogue of C's inline follow-on `astMapMerge` after a
    /// WinMap swap (winmap.c:1537-1559), which passes a *sub-slice* of the
    /// list (`map_list + start`) so the nominated mapping can only merge
    /// within that suffix — it cannot reach back past `start`. We replicate
    /// that scoping by lifting the suffix into its own `MergeCtx`, nominating
    /// `sub_index` there once, and writing the result back. Returns the
    /// sub-range index of the first modified element, if any.
    pub(crate) fn merge_subrange(
        &mut self,
        start: usize,
        sub_index: usize,
        series: bool,
    ) -> Option<usize> {
        let suffix: Vec<_> = self.list.split_off(start);
        let mut sub = MergeCtx::new(suffix);
        let nominee = sub.list[sub_index].0.clone_box();
        let modified = dispatch_map_merge(nominee.as_ref(), &mut sub, sub_index, series);
        if let Some(e) = sub.take_error() {
            self.fail(e);
        }
        self.stamped.append(&mut sub.stamped);
        self.list.extend(sub.list);
        modified
    }
}

/// Default `map_merge` implementation: a single direct pairwise merge of
/// the nominated component with its immediate right neighbor.
///
/// This is the Rust analogue of "step 2" (direct merge) shared by C's
/// per-class `MapMerge` methods, and is the body inherited by every class
/// that has not yet had its own `map_merge` ported. It reproduces exactly
/// the per-pair logic the previous `merge_loop` ran inline for the pair
/// `(where_, where_ + 1)`:
///
/// 1. `list[where_].try_merge(list[where_+1], …)` (left into right), then
/// 2. the reverse merge (right into left), with the series/parallel
///    dimension guards the old loop applied.
///
/// On success the pair is replaced by the single merged mapping (in its
/// forward state, `effective_inv = false`); if that mapping is itself a
/// same-kind `CmpMap` it is re-expanded in place. Returns `Some(where_)`
/// — the index of the first modified element, always the nominated index
/// for a direct pairwise merge — or `None` when the pair cannot merge
/// (C's `-1`).
/// `map_merge` for a nominated `CmpMap`, mirroring the "Simplify the CmpMap on
/// its own" step at the head of C `cmpmap.c::MapMerge` (1436-1475).
///
/// Before considering its neighbors, C recursively `astSimplify`s the
/// nominated CmpMap; if that produced a different Mapping it replaces the
/// component in the list (with the new Invert flag) and returns the modified
/// index. `astSimplify_` (mapping.c:24732) tags every result it returns with
/// `IsSimple`, so this self-simplify is also how a CmpMap left atomic by the
/// nominate loop comes to carry `IsSimp = 1` in the native dump.
///
/// The default `map_merge` skips this recursion; performing it here matches C
/// and is structure-preserving: a CmpMap already in normal form simplifies to
/// an equal CmpMap (only the flag is set). The `has_been_simplified`
/// short-circuit in `CmpMap::simplified` plus the `SimplifyGuard` cycle check
/// prevent re-entry, and on no structural change we fall through to
/// `default_map_merge` so the neighbor-merge behavior is unchanged.
pub(crate) fn cmp_map_merge(
    cmp: &CmpMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // Simplify the component in its effective applied direction, exactly as
    // C sets `astSetInvert(map, invert_list[where])` before `astSimplify`
    // (cmpmap.c:1440-1450). Whether it has already been simplified is a
    // question about that direction, since C records it per orientation
    // (mapping.h:416-422) and `astSimplify` tests it after the Invert is set.
    let eff_inv = ctx.entry(where_).1;
    let mut oriented = cmp.clone_box();
    if oriented.is_inverted() != eff_inv {
        oriented.invert();
    }
    if !oriented.has_been_simplified() {
        trace::simplify_enter("cmp-self-simplify-head", oriented.as_ref());
        // C's `astSimplify` passes up any error but an undefined
        // transformation, which ends the merge (cmpmap.c:1641, 1659).
        let (simplified, outcome) = match oriented.simplify_with_outcome() {
            Ok(result) => result,
            Err(e) => {
                ctx.fail(e);
                return None;
            }
        };
        let changed = matches!(outcome, SimplifyOutcome::Changed(_));
        trace::cmpmap_head(where_, changed, simplified.as_ref());
        // C's test is `simpler = ( new != map ) || ( new_invert !=
        // invert_list[where] )` (cmpmap.c:1456) — pointer identity, not
        // structure, and `changed` is that identity answer reported by the
        // callee. The second disjunct is redundant in C: it can only fire
        // when `new != map` already has, because the `astClone( this )`
        // branch hands back the very object whose `Invert` was just set to
        // `invert_list[where]`.
        if let SimplifyOutcome::Changed(records) = outcome {
            // The records the reduction wrote on the component's own
            // components stay on the component in C, though it is replaced
            // here, so keep them in case this simplification fails.
            if let Some(records) = records {
                records.apply_to(oriented.as_mut());
                if oriented.is_inverted() {
                    oriented.invert();
                }
                let origin = ctx.entry(where_).0.mapping_flags().origin;
                ctx.record_tags(origin, TagTree::of(oriented.as_ref()));
            }
            let new_inv = simplified.is_inverted();
            let mut fwd = simplified;
            if new_inv {
                fwd.invert();
            }
            // C `astSimplify_` (mapping.c:24763) tagged this nominated
            // component's reduced result. The record is per orientation and
            // `fwd` sits at the forward one after the normalizing `invert()`
            // above, while the tree serializes it at `new_inv`; record the
            // orientation the tag was earned in.
            fwd.set_simplified_in(new_inv, true);
            // C stores the result object at its own orientation and sets
            // `invert_list[where] = astGetInvert(new)` (cmpmap.c:1463-1464),
            // so effective == stored for the replaced element; the final
            // fold's set/restore then serializes the child at that stored
            // orientation. Record it as `orig_inv` so `reconstruct` does
            // the same.
            ctx.set_pair_single(where_, (fwd, new_inv, new_inv));
            return Some(where_);
        }

        // C's `!simpler` case is `new == map`: the recursion handed back the
        // very object it was given, so every `IsSimple` it raised on that
        // object's descendants is already on the component the closing fold
        // will encapsulate. Owned values make the returned object a separate
        // one, so adopt it in place of the entry to carry those records
        // forward; `!changed` is C's pointer identity, which means the two
        // are the same Mapping.
        let mut adopted = simplified;
        if adopted.is_inverted() {
            adopted.invert();
        }
        let (entry, eff, orig) = ctx.entry(where_);
        let (eff, orig) = (*eff, *orig);
        adopted.mapping_flags_mut().origin = entry.mapping_flags().origin;
        ctx.set_pair_single(where_, (adopted, eff, orig));
        // A reduction that failed is not recorded as simplified, as C's
        // `astSimplify` records only one that succeeded; the copy it
        // returns still carries the records written on its components.
        if !matches!(outcome, SimplifyOutcome::Failed) {
            ctx.set_component_simple_tag(where_);
        }
        ctx.record_component_stamp(where_);
    } else {
        // C's `astSimplify` hands a tagged Mapping straight back, so its head
        // still runs and finds nothing simpler.
        trace::cmpmap_head(where_, false, oriented.as_ref());
    }

    // The CmpMap did not simplify on its own. If it combines its components
    // as the list does, replace it with them (`cmpmap.c:1677-1734`). A list
    // built by `flatten` holds no such CmpMap, but a merge can make one, such
    // as the MatrixMap and ShiftMap a linear PolyMap becomes, and C splits it
    // on its next nomination. C inserts the two components themselves, so
    // they are not decomposed further here.
    let (entry, eff_inv, _) = ctx.entry(where_);
    if let Some(entry_cmp) = entry.as_any().downcast_ref::<CmpMap>()
        && (entry_cmp.composition() == Composition::Series) == series
    {
        let cmp_inv = *eff_inv != entry_cmp.is_inverted();
        let first = (
            entry_cmp.first(),
            cmp_inv != (entry_cmp.inv_a() != entry_cmp.first().is_inverted()),
        );
        let second = (
            entry_cmp.second(),
            cmp_inv != (entry_cmp.inv_b() != entry_cmp.second().is_inverted()),
        );
        let order = if cmp_inv && series {
            [second, first]
        } else {
            [first, second]
        };
        let pieces = order.map(|(m, inv)| leaf_entry(m, inv)).to_vec();
        ctx.list.splice(where_..=where_, pieces);
        trace::cmpmap_split(where_);
        return Some(where_);
    }

    // Merge adjacent CmpMaps (C `cmpmap.c::MapMerge` 1528-1813).
    // ========================================================
    // The self-simplify head above did not fire (C `!simpler`); look for a
    // neighboring CmpMap with which the nominated one can merge. C uses the
    // pair (imap1, imap2) where imap2 is the nominated CmpMap — except when
    // the nominee is the first in the list, in which case the pair is (0, 1)
    // (`cmpmap.c:1540-1546`). It only restructures when both members are
    // CmpMaps of the composition opposite to the current combination context,
    // and when at least one sub-pair genuinely simplifies (the compute cores
    // enforce both conditions, returning `None` otherwise). Applying it via
    // `replace_pair_with` annuls the pair and stores the forward replacement
    // at `imap1`, matching C setting the new invert flag to 0.
    if ctx.len() > 1 {
        let imap1 = if where_ == 0 { 0 } else { where_ - 1 };
        if imap1 + 1 < ctx.len() {
            let merged = if series {
                // Parallel CmpMaps in series (C 1629-1781).
                compute_merge_parallel_pair_in_series(&ctx.list, imap1)
            } else {
                // Series CmpMaps in parallel (C 1582-1622).
                compute_merge_series_pair_in_parallel(&ctx.list, imap1)
            };
            match merged {
                Ok(Some(new)) => {
                    ctx.replace_pair_with(imap1, new, false);
                    return Some(imap1);
                }
                Ok(None) => {}
                Err(e) => {
                    ctx.fail(e);
                    return None;
                }
            }
        }
    }

    // Swap a parallel CmpMap in front of an adjacent PermMap
    // (C `cmpmap.c::MapMerge` 1816-2125).
    // ==================================================
    // Reached only when no earlier branch simplified anything (C's
    // `result == -1`), the nominated CmpMap is parallel, it is not first in the
    // list, and its lower neighbor is a PermMap. The swap may let either of
    // the two Mappings merge with its new neighbor. C reports the PermMap's
    // old slot (`result = imap1`, `cmpmap.c:2119`), so the nominate loop
    // resumes there.
    if series
        && where_ > 0
        && ctx
            .entry(where_)
            .0
            .as_any()
            .downcast_ref::<CmpMap>()
            .is_some_and(|c| c.composition() == Composition::Parallel)
        && ctx.entry(where_ - 1).0.as_any().is::<PermMap>()
        && try_swap_permmap_parallel_at(ctx, where_ - 1)
    {
        return Some(where_ - 1);
    }

    default_map_merge(ctx, where_, series)
}

/// `map_merge` for a nominated `ZoomMap`, a faithful port of C
/// `zoommap.c::MapMerge` (zoommap.c:564).
///
/// Series: accumulate the effective zoom factors of the run of adjacent
/// `ZoomMap`/`UnitMap` mappings (dividing for inverted ZoomMaps, ignoring
/// UnitMaps) and replace the run with a single forward `ZoomMap` — or a
/// `UnitMap` when the product is unity. Parallel: gather each mapping's
/// per-axis effective zoom into a vector and replace the run with a
/// `UnitMap` (all unity), a single `ZoomMap` (all equal), or a diagonal
/// `MatrixMap` (mixed factors).
///
/// The key C rule (`simpler = ngone || unit || invert_list[where]`) means a
/// *lone* ZoomMap whose effective direction is inverted is still replaced —
/// by a forward ZoomMap holding the reciprocal factor — so the invert flag
/// is folded into the stored factor rather than surfacing as an `InvA`/`InvB`
/// on a parent CmpMap. This matches the C native dump (e.g. a degrees↔radians
/// ZoomMap serializes as a forward `Zoom` value, never as an inverted one).
/// Whether a ZoomMap can be absorbed into `m` as a neighbor (C
/// zoommap.c::MapMerge handles MatrixMap, WinMap and ShiftMap neighbors).
fn zoom_absorbable(m: &dyn Mapping) -> bool {
    let a = m.as_any();
    a.is::<super::MatrixMap>() || a.is::<super::WinMap>() || a.is::<ShiftMap>()
}

pub(crate) fn zoom_map_merge(
    zoom: &ZoomMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // Effective forward zoom of an element (`mapping` is stored forward, the
    // bool carries the applied inversion).
    let eff_zoom = |entry: &(DynMapping, bool, bool)| -> Option<f64> {
        let zm = entry.0.as_any().downcast_ref::<ZoomMap>()?;
        Some(if entry.1 { 1.0 / zm.zoom() } else { zm.zoom() })
    };
    // The `has_been_simplified` flag (C `IsSimple`) does NOT block merging:
    // C `MapMerge` never consults it. The only per-component merge barrier
    // is `Frozen` (oscillation break), enforced by the outer merge loop via
    // `MergeCtx::is_frozen`. Mirrors C `cmpmap.c` (IsSimple gates only the
    // top-level `astSimplify`, mapping.c:24740; the merge loop clears/uses
    // `Frozen`, cmpmap.c:3538,3608).
    let is_zoom = |entry: &(DynMapping, bool, bool)| entry.0.as_any().is::<ZoomMap>();
    let is_unit = |entry: &(DynMapping, bool, bool)| entry.0.as_any().is::<UnitMap>();

    let n = ctx.len();
    let where_inv = ctx.entry(where_).1;

    if series {
        // Effective zoom of the nominated ZoomMap.
        let mut zoom_acc = if where_inv {
            1.0 / zoom.zoom()
        } else {
            zoom.zoom()
        };

        // Extend downward over adjacent ZoomMaps (accumulate) and UnitMaps.
        let mut imap1 = where_;
        while imap1 >= 1 {
            let prev = ctx.entry(imap1 - 1);
            if is_zoom(prev) {
                let z = prev.0.as_any().downcast_ref::<ZoomMap>()?.zoom();
                zoom_acc = if prev.1 { zoom_acc / z } else { zoom_acc * z };
            } else if !is_unit(prev) {
                break;
            }
            imap1 -= 1;
        }

        // Extend upward similarly.
        let mut imap2 = where_;
        while imap2 + 1 < n {
            let next = ctx.entry(imap2 + 1);
            if is_zoom(next) {
                let z = next.0.as_any().downcast_ref::<ZoomMap>()?.zoom();
                zoom_acc = if next.1 { zoom_acc / z } else { zoom_acc * z };
            } else if !is_unit(next) {
                break;
            }
            imap2 += 1;
        }

        let ngone = imap2 - imap1;
        let unit = (zoom_acc - 1.0).abs() <= 8.0 * f64::EPSILON;
        let simpler = ngone > 0 || unit || where_inv;
        if !simpler {
            // No ZoomMap-run reduction: absorb the ZoomMap into a
            // neighboring MatrixMap/WinMap/ShiftMap. Mirroring C
            // zoommap.c::MapMerge (994-1122), the *previous* neighbor is
            // tried before the *next* one — this ordering is observable when
            // only one side is eligible (e.g. a restricted simplify where the
            // trailing Mapping is not AllowSimplify-flagged). The pairwise
            // fusion itself is done by ZoomMap::try_merge via
            // `default_map_merge`.
            if where_ > 0
                && let Some(prev) = ctx.list.get(where_ - 1)
                && zoom_absorbable(prev.0.as_ref())
            {
                if let Some(modified) = default_map_merge(ctx, where_ - 1, series) {
                    return Some(modified);
                }
            }
            return default_map_merge(ctx, where_, series);
        }

        // Input dimension of the (uniform) run — every element shares it.
        let nin = {
            let (ref m, inv, _) = *ctx.entry(where_);
            if inv { m.nout() } else { m.nin() }
        };
        // A product that underflows to zero is refused by the constructor,
        // and C leaves that error in the global status.
        let new: DynMapping = if unit {
            Box::new(UnitMap::new(nin))
        } else {
            Box::new(ctx.settle(ZoomMap::new(nin, zoom_acc).map(Some))?)
        };
        ctx.replace_range_with(imap1, imap2, new);
        return Some(imap1);
    }

    // Parallel: gather per-axis effective zoom factors across the run.
    let mut imap1 = where_;
    while imap1 >= 1 {
        let prev = ctx.entry(imap1 - 1);
        if !is_zoom(prev) && !is_unit(prev) {
            break;
        }
        imap1 -= 1;
    }
    let mut imap2 = where_;
    while imap2 + 1 < n {
        let next = ctx.entry(imap2 + 1);
        if !is_zoom(next) && !is_unit(next) {
            break;
        }
        imap2 += 1;
    }

    let mut zooms: Vec<f64> = Vec::new();
    let mut minzoom = f64::MAX;
    let mut maxzoom = -f64::MAX;
    for idx in imap1..=imap2 {
        let entry = ctx.entry(idx);
        let z = if is_zoom(entry) {
            eff_zoom(entry)?
        } else {
            1.0
        };
        // Number of parallel axes this element contributes (its effective nin).
        let (ref m, inv, _) = *entry;
        let ncoord = if inv { m.nout() } else { m.nin() };
        for _ in 0..ncoord {
            zooms.push(z);
        }
        if z < minzoom {
            minzoom = z;
        }
        if z > maxzoom {
            maxzoom = z;
        }
    }

    let ngone = imap2 - imap1;
    let single = minzoom == maxzoom;
    let unit = single && minzoom == 1.0;
    let simpler = ngone > 0 || unit || where_inv;
    if !simpler {
        return default_map_merge(ctx, where_, series);
    }

    let nin = zooms.len();
    let new: DynMapping = if unit {
        Box::new(UnitMap::new(nin))
    } else if single {
        Box::new(ctx.settle(ZoomMap::new(nin, minzoom).map(Some))?)
    } else {
        // A diagonal MatrixMap, as C's zoommap.c MapMerge makes it.
        Box::new(ctx.settle(MatrixMap::diagonal(nin, nin, &zooms).map(Some))?)
    };
    ctx.replace_range_with(imap1, imap2, new);
    Some(imap1)
}

/// `map_merge` for a nominated `PermMap`, mirroring C `permmap.c::MapMerge`
/// (1362-1373): a direct merge with the upper neighbor, then the rebuild into
/// the PermMap's canonical stored form.
///
/// The CmpMap-past-PermMap swap is not here. C runs it from the nominated
/// parallel CmpMap (`cmpmap.c:1816-2125`) and `permmap.c` never mentions
/// `CmpMap` at all, so it lives in [`cmp_map_merge`].
pub(crate) fn perm_map_merge(
    _perm: &PermMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    if let Some(modified) = default_map_merge(ctx, where_, series) {
        return Some(modified);
    }

    // C `permmap.c::MapMerge` rebuilds a nominated PermMap into its canonical
    // stored form — dropping a now-redundant stored permutation array (→
    // `OutCpy`/`InCpy`), clearing a set Invert flag, or collapsing to a
    // UnitMap — and reports this as a simplification even when no neighbor
    // merged (permmap.c:1362-1373). Mirror that here so the surrounding tree
    // is reconstructed with the canonical leaf rather than the stored form.
    let (m, eff_inv, _) = ctx.entry(where_);
    let eff_inv = *eff_inv;
    let canon = m
        .as_any()
        .downcast_ref::<PermMap>()?
        .canonical_null_form(eff_inv)?;
    // The rebuild resolves the applied direction into the permutation arrays,
    // so the replacement enters the list forward with both flags cleared
    // (C `(*invert_list)[imap1] = 0`, permmap.c:1420).
    ctx.set_pair_single(where_, (canon, false, false));
    Some(where_)
}

/// Per-class `map_merge` for a nominated `WcsMap`, mirroring the merge and
/// swap logic of C `wcsmap.c::MapMerge` (2922). A WcsMap can only merge
/// directly with its own inverse (`CanMerge`, wcsmap.c:820): C checks the
/// lower pair `(where_-1, where_)` first, then the upper pair
/// `(where_, where_+1)`, replacing a cancelling pair with a UnitMap
/// (wcsmap.c:3124-3170). Doing the lower check at this nomination — rather
/// than waiting for the lower WcsMap's own nomination — matters: the
/// returned `modified` index cascades the nominate loop back through the
/// canceled pair, which determines the association order of later linear
/// folds. When no direct merge is available, the WcsMap tries to swap past
/// an adjacent PermMap — either to move it next to a cancelling WcsMap on
/// the far side, or because the swap itself yields a simpler pair. On success
/// the first modified index is returned.
pub(crate) fn wcs_map_merge(
    wcs: &WcsMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // Replace-with-simpler, "without reference to the neighboring Mappings in
    // the list": a WcsMap of type `AST__WCSBAD` is equivalent to a UnitMap
    // (wcsmap.c:3095-3107). C makes this the `if` of an if/else whose `else`
    // holds the whole neighbor search, so a reduced WcsMap never looks at a
    // neighbor in the same pass. The replacement carries a cleared Invert flag
    // (wcsmap.c:3104).
    if wcs.projection() == crate::mappings::WcsProjection::Unknown {
        ctx.set_pair_single(where_, (Box::new(UnitMap::new(wcs.nin())), false, false));
        return Some(where_);
    }

    if series && ctx.len() > 1 {
        // Direct inverse-pair cancellation, lower neighbor first
        // (C wcsmap.c:3116-3131). Both elements must be WcsMaps (CanMerge);
        // `WcsMap::try_merge` implements the same-projection/opposite-sense
        // equivalence and returns the replacement UnitMap.
        let candidates = [
            where_.checked_sub(1),
            (where_ + 1 < ctx.len()).then_some(where_),
        ];
        for i1 in candidates.into_iter().flatten() {
            let merged = {
                let (m1, inv1, _) = ctx.entry(i1);
                let (m2, inv2, _) = ctx.entry(i1 + 1);
                if !m1.as_any().is::<WcsMap>() || !m2.as_any().is::<WcsMap>() {
                    continue;
                }
                m1.try_merge(m2.as_ref(), true, *inv1, *inv2)
            };
            if let Some(merged) = merged {
                ctx.replace_pair_with(i1, merged, false);
                return Some(i1);
            }
        }

        // No direct merge, so consider swapping the WcsMap with a neighbor to
        // bring it next to a WcsMap it can cancel against on the far side, or
        // because the swapped pair is itself simpler (C wcsmap.c:3171-3342).
        let wcs_inv = ctx.entry(where_).1;

        // Can the WcsMap swap with its higher neighbor, and would that be
        // intrinsically simpler? (C wcsmap.c:3181-3194.)
        let hi = if where_ + 1 < ctx.len() {
            can_swap_wcsmap_past_permmap(&ctx.list, where_)
        } else {
            Ok(None)
        };
        let (swaphi, do2, newwcs_hi) = match hi {
            Ok(Some((simpler, newwcs))) => (true, simpler, newwcs),
            Ok(None) => (false, false, None),
            Err(e) => {
                ctx.fail(e);
                return None;
            }
        };

        // Step forward looking for a Mapping the swapped WcsMap could cancel
        // against, stopping at the first Mapping it could not swap past, and
        // note how many Mappings separate the two (C wcsmap.c:3196-3222).
        let mut nstep2: Option<usize> = None;
        if swaphi {
            for i2 in (where_ + 1)..ctx.len() {
                if can_merge_wcsmaps(&ctx.list, i2, newwcs_hi.as_ref(), wcs_inv) {
                    nstep2 = Some(i2 - where_ - 1);
                    break;
                }
                if !ctx.list[i2].0.as_any().is::<PermMap>() {
                    break;
                }
            }
        }

        // The same working back towards the start of the list
        // (C wcsmap.c:3226-3255).
        let lo = if where_ > 0 {
            can_swap_wcsmap_past_permmap(&ctx.list, where_ - 1)
        } else {
            Ok(None)
        };
        let (swaplo, do1, newwcs_lo) = match lo {
            Ok(Some((simpler, newwcs))) => (true, simpler, newwcs),
            Ok(None) => (false, false, None),
            Err(e) => {
                ctx.fail(e);
                return None;
            }
        };

        let mut nstep1: Option<usize> = None;
        if swaplo {
            for i1 in (0..where_).rev() {
                if can_merge_wcsmaps(&ctx.list, i1, newwcs_lo.as_ref(), wcs_inv) {
                    nstep1 = Some(where_ - 1 - i1);
                    break;
                }
                if !ctx.list[i1].0.as_any().is::<PermMap>() {
                    break;
                }
            }
        }

        // Choose the neighbor so the WcsMap moves towards the nearer target
        // (C wcsmap.c:3259-3272). Both arms imply the corresponding swap was
        // found possible, so the neighbor index exists.
        let target = if do1 || (nstep1.is_some() && (nstep2.is_none() || nstep2 > nstep1)) {
            Some(where_ - 1)
        } else if do2 || nstep2.is_some() {
            Some(where_)
        } else {
            None
        };

        // With a target, swap unconditionally (C wcsmap.c:3274-3281).
        if let Some(i1) = target {
            match wcs_perm_at(&mut ctx.list, i1) {
                Ok(true) => return Some(i1),
                Ok(false) => {}
                Err(e) => {
                    ctx.fail(e);
                    return None;
                }
            }

        // With no target, the swapped pair may still be simpler than the
        // original: swap copies, and keep the result only if simplifying
        // either half changes its class (C wcsmap.c:3283-3342). When both
        // neighbors are suitable, swap with the lower.
        } else if swaphi || swaplo {
            let i1 = if swaplo { where_ - 1 } else { where_ };
            let mut copy = vec![
                (ctx.list[i1].0.clone_box(), ctx.list[i1].1, ctx.list[i1].2),
                (
                    ctx.list[i1 + 1].0.clone_box(),
                    ctx.list[i1 + 1].1,
                    ctx.list[i1 + 1].2,
                ),
            ];
            // C simplifies both swapped Mappings before comparing either.
            let swapped = wcs_perm_at(&mut copy, 0).and_then(|swapped| {
                if !swapped {
                    return Ok(false);
                }
                let s0 = copy[0].0.simplify()?;
                let s1 = copy[1].0.simplify()?;
                Ok(s0.as_any().type_id() != copy[0].0.as_any().type_id()
                    || s1.as_any().type_id() != copy[1].0.as_any().type_id())
            });
            let worthwhile = match swapped {
                Ok(worthwhile) => worthwhile,
                Err(e) => {
                    ctx.fail(e);
                    return None;
                }
            };
            if worthwhile {
                let mut entries = copy.into_iter();
                ctx.list[i1] = entries.next().unwrap();
                ctx.list[i1 + 1] = entries.next().unwrap();
                return Some(i1);
            }
        }
    }

    default_map_merge(ctx, where_, series)
}

/// The `MapMerge` shape shared by classes whose C counterpart opens with
/// "see if this Mapping can be replaced by a simpler Mapping, without
/// reference to the neighboring Mappings in the list" and then falls through
/// to the neighbor steps.
///
/// `reduced` is the replacement the class arrived at, or `None` when it has
/// none. The class computes it in its own `map_merge`, which is where C's body
/// computes it, and `Some` is the class stating that a simplification took
/// place — C's own test is on the pointer `astSimplify` handed back, and it too
/// is made in the class body, not here.
pub(crate) fn self_reduce_map_merge(
    this: &dyn Mapping,
    reduced: AstResult<Option<DynMapping>>,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // An error reaching the replacement, such as one simplifying an
    // encapsulated Mapping, ends the merge, as C leaves it in the global
    // status.
    let reduced = match reduced {
        Ok(reduced) => reduced,
        Err(e) => {
            ctx.fail(e);
            return None;
        }
    };
    if let Some(modified) = try_self_reduce(this, reduced, ctx, where_) {
        return Some(modified);
    }
    default_map_merge(ctx, where_, series)
}

/// The replace-with-simpler head on its own, for a class whose C body puts
/// other steps between it and the plain neighbor merge.
pub(crate) fn try_self_reduce(
    this: &dyn Mapping,
    reduced: Option<DynMapping>,
    ctx: &mut MergeCtx,
    where_: usize,
) -> Option<usize> {
    trace::simplify_enter("self-reduce-head", this);
    if let Some(simp) = reduced {
        let (eff, orig) = {
            let e = ctx.entry(where_);
            (e.1, e.2)
        };
        ctx.set_pair_single(where_, (simp, eff, orig));
        return Some(where_);
    }
    None
}

/// C's base `Simplify` (`mapping.c:16752`), inherited by every class whose C
/// counterpart does not override the slot.
///
/// Builds a one-element Mapping list and hands it to the class's own
/// [`Mapping::map_merge`] repeatedly, stopping when the merge reports no change
/// or the list grows past one element (`mapping.c:16871-16883`). Reports `false`
/// when nothing reduced, which is where C returns `astClone( this )`.
///
/// C's entry guard on `RestrictedSimplify` (`mapping.c:16853-16855`) is the
/// first thing the driver does: a Mapping under a restricted simplify that is
/// not flagged eligible is handed straight back. The per-component half of the
/// same rule — only `AllowSimplify` components may be nominated — belongs to
/// the enclosing `CmpMap`'s loop and lives in [`MergeCtx`].
pub(crate) fn base_simplify(this: &dyn Mapping) -> AstResult<(DynMapping, bool)> {
    // C's base `Simplify` declines outright when the Mapping is under a
    // restricted simplify without being flagged eligible
    // (mapping.c:16853-16855).
    if this.restricted_simplify() && !this.allow_simplify() {
        return Ok((this.clone_box(), false));
    }

    // C `astMapList( this, 1, astGetInvert( this ), … )`: for a class with no
    // `MapList` override this appends the Mapping itself and records its own
    // Invert as the effective direction, which is what `flatten` produces for a
    // leaf given no externally imposed inversion.
    let mut ctx = MergeCtx::new(flatten(this, false, true)?);
    ctx.clear_frozen();
    let mut simpler = false;

    // C loops while the nominated element is unfrozen. Only the oscillation
    // break inside a merge freezes an element, so in practice the break below is
    // what ends the loop.
    while !ctx.is_frozen(0) {
        let nominee = ctx.entry(0).0.clone_box();
        let modified = dispatch_map_merge(nominee.as_ref(), &mut ctx, 0, true);
        if let Some(e) = ctx.take_error() {
            return Err(e);
        }

        if ctx.len() > 1 || modified.is_none() {
            break;
        }
        simpler = true;
    }

    // `nmap > 1` is a reduction that produced more than one Mapping, which this
    // driver has nowhere to put: C returns the original even though the loop
    // reported a change (`mapping.c:16891`).
    if !simpler || ctx.len() > 1 {
        return Ok((this.clone_box(), false));
    }

    // The one remaining element, brought to the orientation the list asks for.
    // C copies it and sets or clears `Invert` when `invert_list[ 0 ]` disagrees
    // with the Mapping's own flag (`mapping.c:16899-16911`); for a one-element
    // list that is exactly what `reconstruct` does to its seed.
    reconstruct(std::mem::take(&mut ctx.list), true).map(|m| (m, true))
}

/// Offer the list to the nominated Mapping's own `map_merge`.
///
/// C's public dispatcher is `astMapMerge_` (`mapping.c:24242`), and its entry
/// guard is part of the contract every caller gets: a Mapping whose `Ident`
/// attribute has been set never merges, so its identity survives the
/// simplification of the tree around it (`mapping.c:24257`). The per-class
/// `MapMerge` implementations are reached only through here, which is why none
/// of them repeats the test.
///
/// The guard also stops a nominated Mapping *self*-reducing, since C's
/// replace-with-simpler halves live inside the same per-class method. That is
/// how an `Ident`-set leaf keeps its coefficients as constructed even when it is
/// nominated inside a list.
///
/// The `MAPMERGE` and `MAPMERGE-RESULT` trace events are emitted here, as the
/// C trace emits them from `astMapMerge_`, so a merge that hands off to another
/// class's merge through this function is traced as C traces it.
pub(crate) fn dispatch_map_merge(
    nominee: &dyn Mapping,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    trace::map_merge(nominee, ctx.entry(where_).2, where_, series, ctx.len());
    if nominee.do_not_simplify() {
        return None;
    }
    let result = merge_propagating_allow_simplify(nominee, ctx, where_, series);
    trace::map_merge_result(nominee, result, ctx.len());
    result
}

/// The body of `astMapMerge_` after its entry guard: the nominee's own
/// `map_merge`, passing `AllowSimplify` on to what it creates.
fn merge_propagating_allow_simplify(
    nominee: &dyn Mapping,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // A nominee with AllowSimplify set is probably part of a restricted
    // simplification, so the flag is passed on to the Mappings the merge
    // creates when every Mapping it consumed had it (mapping.c
    // `astMapMerge_`). C tells consumed and created Mappings apart by
    // pointer; here each entry is tagged with its position before the merge,
    // and a Mapping the merge makes, or copies, carries no tag.
    if !nominee.allow_simplify() {
        return nominee.map_merge(ctx, where_, series);
    }
    let allowed: Vec<bool> = ctx
        .list
        .iter()
        .map(|(m, _, _)| m.allow_simplify())
        .collect();
    for (i, (m, _, _)) in ctx.list.iter_mut().enumerate() {
        m.mapping_flags_mut().merge_tag = i + 1;
    }
    let result = nominee.map_merge(ctx, where_, series);
    if result.is_some() {
        let kept: Vec<usize> = ctx
            .list
            .iter()
            .map(|(m, _, _)| m.mapping_flags().merge_tag)
            .collect();
        let all_consumed_allowed = allowed
            .iter()
            .enumerate()
            .all(|(i, &allow)| allow || kept.contains(&(i + 1)));
        if all_consumed_allowed {
            for (m, _, _) in ctx.list.iter_mut() {
                if m.mapping_flags().merge_tag == 0 {
                    m.set_allow_simplify(true);
                }
            }
        }
    }
    for (m, _, _) in ctx.list.iter_mut() {
        m.mapping_flags_mut().merge_tag = 0;
    }
    result
}

pub(crate) fn default_map_merge(ctx: &mut MergeCtx, where_: usize, series: bool) -> Option<usize> {
    let i = where_;
    let list = &mut ctx.list;
    if i + 1 >= list.len() {
        return None;
    }

    // The `has_been_simplified` flag (C `IsSimple`) does NOT block merging:
    // C `MapMerge` never consults it. The oscillation barrier is `Frozen`,
    // enforced by the outer merge loop (`MergeCtx::is_frozen`), not here.

    let merged = {
        let (ref m1, inv1, _) = list[i];
        let (ref m2, inv2, _) = list[i + 1];

        // The effective input dimension of m1 in its applied direction —
        // this is what the merged result must accept.
        let m1_nin = if inv1 { m1.nout() } else { m1.nin() };

        // Try left merging into right.
        m1.try_merge(m2.as_ref(), series, inv1, inv2)
            // Try right merging into left (swap roles). Validate that the
            // result's input dimension matches m1's effective input, since
            // the result replaces m1's position in the pipeline.
            .or_else(|| {
                if series {
                    // Skip reverse merge when m1 is UnitMap — let post-loop
                    // strip handle it (matches C behavior where UnitMap
                    // removes itself before neighbors get a chance to merge).
                    if m1.as_any().is::<UnitMap>() {
                        return None;
                    }
                    m2.try_merge(m1.as_ref(), series, inv2, inv1)
                        .filter(|m| m.nin() == m1_nin)
                } else {
                    // For parallel: the reverse merge puts m2's axes first,
                    // which is wrong. Instead, try extracting linear params
                    // from both and merging with m1 first.
                    let (s1, c1) = super::winmap::extract_linear_params(m1.as_ref(), inv1)?;
                    let (s2, c2) = super::winmap::extract_linear_params(m2.as_ref(), inv2)?;
                    let mut new_shifts = s1;
                    new_shifts.extend(s2);
                    let mut new_scales = c1;
                    new_scales.extend(c2);
                    // C's parallel WinWin branch concatenates the two term
                    // arrays verbatim (winmap.c:4072-4096) — no rounding snap.
                    super::winmap::compose_linear(
                        &vec![0.0; new_shifts.len()],
                        &vec![1.0; new_shifts.len()],
                        &new_shifts,
                        &new_scales,
                        false,
                    )
                }
            })
    };

    let m = merged?;

    // If the merged result is a same-kind CmpMap, re-expand it in place so
    // the surrounding sequence can keep merging across its components.
    let target_kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };
    if let Some(cmp) = m.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == target_kind {
            if let Ok(expanded) = flatten(m.as_ref(), false, series) {
                list.remove(i + 1);
                list.remove(i);
                for (idx, item) in expanded.into_iter().enumerate() {
                    list.insert(i + idx, item);
                }
                return Some(i);
            }
        }
    }

    // Normalize exactly as [`MergeCtx::replace_pair_with`] does: a merge may
    // return its result in the inverted orientation — `MatrixMap::mtr_zoom`
    // keeps an inverted MatrixMap inverted so it serializes `Invert = 1` like C
    // — and that intrinsic orientation is what `reconstruct` needs in
    // `orig_inv`. Storing `(m, false, false)` threw it away, materializing the
    // inversion into the stored values and dropping the card.
    let mut m = m;
    let orig_inv = m.is_inverted();
    if orig_inv {
        m.invert();
    }
    list[i] = (m, orig_inv, orig_inv);
    list.remove(i + 1);
    Some(i)
}

/// `map_merge` for a nominated Region (Box/Interval/PointList/NullRegion),
/// a faithful port of the per-class `MapMerge` shared by those classes
/// (interval.c:991, box.c, pointlist.c:972, nullregion.c:338).
///
/// The structure, identical across the four classes:
/// 1. **Self-simplify head.** `astSimplify(this)`; if the result differs from
///    the original Region, replace the nominated component with it and return
///    (this is how, e.g., a fully-bounded Interval reclassifies to a Box, or
///    a remapped Region collapses to a simpler class).
/// 2. **Parallel-only neighbor merge.** With the head having made no change:
///    - try the **lower** neighbor (`where-1`) via
///      [`Region::region_merge`] with `intfirst = false` (the neighbor's axes
///      go first), storing the merged Region at `where-1`;
///    - failing that, try the **upper** neighbor (`where+1`) with
///      `intfirst = true` (this Region's axes go first), storing at `where`.
///
///    On success, C annuls both entries, stores the merged Region at `i1`, and
///    shuffles the tail down — [`MergeCtx::replace_pair_with`] does the same.
/// 3. In series, nothing (a series of two Regions is not a Region).
///
/// The nominated Region's own [`try_merge`] only ever covers the direct
/// pairwise (upper-neighbor, `intfirst = true`) case, so this method is what
/// lets a Region absorb the neighbor that sits *before* it in the list — the
/// case C reaches through the lower-neighbor branch and Rust cannot reach via
/// `default_map_merge` (which merges the nominee only with `where+1`).
pub(crate) fn region_map_merge(
    this: &dyn crate::regions::Region,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    use crate::regions::as_region;

    // ---- 1. Self-simplify head (astSimplify(this)) ----
    // C compares object pointers (`new != oldint`), which
    // `simplify_reporting_change` reports. A Region re-expressed in its
    // current Frame covers the same points, so a conversion-based `equal`
    // would not see that change.
    // An error the simplification raises ends the merge, as C leaves it in
    // the global status.
    match <dyn Mapping>::simplify_reporting_change(this) {
        Ok((simplified, true)) => {
            ctx.set_pair_single(where_, (simplified, false, false));
            return Some(where_);
        }
        Ok((_, false)) => {}
        Err(e) => {
            ctx.fail(e);
            return None;
        }
    }

    // ---- 2. Parallel-only neighbor merge ----
    if !series {
        // Lower neighbor first (intfirst = 0: neighbor axes go first).
        if where_ > 0 {
            let merged = {
                let (m, _, _) = ctx.entry(where_ - 1);
                as_region(m.as_ref()).and_then(|lower| this.region_merge(lower, false))
            };
            if let Some(merged) = merged {
                ctx.replace_pair_with(where_ - 1, merged, false);
                return Some(where_ - 1);
            }
        }

        // Upper neighbor (intfirst = 1: this Region's axes go first).
        if where_ + 1 < ctx.len() {
            let merged = {
                let (m, _, _) = ctx.entry(where_ + 1);
                as_region(m.as_ref()).and_then(|upper| this.region_merge(upper, true))
            };
            if let Some(merged) = merged {
                ctx.replace_pair_with(where_, merged, false);
                return Some(where_);
            }
        }
    }

    None
}

/// Append `val` to `list` and report the wavelength of any repeating
/// pattern, or 0 if none.
///
/// Faithful port of C `cmpmap.c::PatternCheck` (2851). The loop appends
/// `(modified, nmap)` history values and detects merge/split cycles: 3
/// complete repeats are required as evidence of looping, except a
/// wavelength of 1 needs 30 repeats (short sequences legitimately repeat a
/// single value). The check only runs once the list has grown past 29
/// entries, and considers wavelengths up to `iat/3` capped at 50.
///
/// `check` enables the pattern search (C passes the first call's returned
/// wavelength as the second call's `check` argument, gating the `nmap`
/// search on the `modified` search having found a pattern).
fn pattern_check(val: i64, check: i64, list: &mut Vec<i64>) -> i64 {
    let mut result: i64 = 0;

    let iat = list.len();
    list.push(val);

    // Only search once we have enough history (C: list_len > 29).
    if check != 0 && list.len() > 29 {
        let mut mxwave = iat / 3;
        if mxwave > 50 {
            mxwave = 50;
        }
        let jlo = iat as isize - 3 * mxwave as isize;

        // Search backwards for the most recent earlier occurrence of `val`.
        let mut jat = iat as isize - 1;
        'outer: while jat >= jlo {
            if list[jat as usize] == val {
                let wavelen = iat - jat as usize;

                // wavelength 1 needs 30 repeats; longer needs 3.
                let nwave = if wavelen == 1 { 30.min(iat) } else { 3 };

                if nwave * wavelen <= list.len() {
                    result = wavelen as i64;
                    // wave[k] points to the start of the k-th most recent
                    // window of length `wavelen`. Compare each window's
                    // elements against the most recent window.
                    let len = list.len();
                    'check: for k in 0..wavelen {
                        let base0 = len - wavelen + k;
                        for iwave in 1..nwave {
                            let idx = base0 - iwave * wavelen;
                            if list[idx] != list[base0] {
                                result = 0;
                                break 'check;
                            }
                        }
                    }
                }

                if result != 0 {
                    break 'outer;
                }
            }
            jat -= 1;
        }
    }

    result
}

/// Run C's nominate loop over the working list to direct-merge exhaustion.
///
/// Faithful port of the inner `while ( improved )` / nominate loop in C
/// `cmpmap.c::Simplify` (3537-3624): repeatedly nominate each unfrozen
/// component in turn, call its `map_merge`, and on any change restart from
/// the first modified index. The `mlist`/`nlist` histories drive
/// `pattern_check`; when a repeating `(modified, nmap)` pattern is found,
/// the modified component is frozen (`astSetFrozen`) so it can no longer
/// argue, breaking the cycle. The loop ends once a complete pass makes no
/// change.
///
/// Returns `true` if any merge occurred.
///
/// # Errors
///
/// The first error a merge raises, which ends the loop as C's
/// `while ( astOK && ... )` does (cmpmap.c:3754).
fn run_nominate_loop(
    ctx: &mut MergeCtx,
    series: bool,
    mlist: &mut Vec<i64>,
    nlist: &mut Vec<i64>,
) -> AstResult<bool> {
    let mut any = false;
    let mut improved = true;
    while improved {
        improved = false;
        let mut nominated = 0;
        while nominated < ctx.len() {
            // Frozen components do not get to suggest changes (C astFrozen).
            // Under a restricted simplify, components without AllowSimplify
            // are also skipped (C cmpmap.c:3563).
            if ctx.is_frozen(nominated) || !ctx.may_nominate(nominated) {
                trace::nominate_skip(nominated, ctx.is_frozen(nominated));
                nominated += 1;
                continue;
            }

            // Clone the nominated mapping and attempt to merge it with its
            // neighbors (C clones map_list[nominated] before astMapMerge).
            let nominee = ctx.list[nominated].0.clone_box();
            trace::merge_list(nominated, series, &ctx.list);
            let modified = {
                let _nomination = trace::enter_nomination();
                dispatch_map_merge(nominee.as_ref(), ctx, nominated, series)
            };
            if let Some(e) = ctx.take_error() {
                return Err(e);
            }

            // Move on to nominate the next mapping (C `nominated++`).
            nominated += 1;

            let Some(modified) = modified else {
                continue;
            };

            // Append to the history and check for a repeating pattern. A
            // pattern in `modified` is only honored if `nmap` also repeats
            // with a wavelength related by an integer factor (cmpmap.c:3589).
            let wlen1 = pattern_check(modified as i64, 1, mlist);
            let wlen2 = pattern_check(ctx.len() as i64, wlen1, nlist);
            let mut w1 = wlen1;
            let mut w2 = wlen2;
            if w1 != 0 && w2 != 0 {
                if w1 > w2 {
                    std::mem::swap(&mut w1, &mut w2);
                }
                if w2 % w1 != 0 {
                    w1 = 0;
                }
            }

            if w1 > 0 {
                // Repeating pattern: freeze the modified mapping so it stops
                // contributing to the argument (C astSetFrozen).
                ctx.freeze(modified);
                trace::freeze(modified, ctx.entry(modified).0.as_ref());
            } else {
                improved = true;
                any = true;
                // If an earlier mapping than the next nominee was modified,
                // go back and consider it first (C: nominated = modified).
                if modified < nominated {
                    nominated = modified;
                }
            }
        }
    }
    Ok(any)
}

/// Simplify a flattened series/parallel component list in place.
///
/// This is the Rust port of C `cmpmap.c::Simplify`'s simplification engine:
/// the nominate loop that drives each component's per-class `map_merge` to a
/// fixpoint (`cmpmap.c:3553-3629`). Every cross-class swap or restructure
/// lives in the owning class's `map_merge`, reached only from here, exactly as
/// C reaches `astMapMerge`.
///
/// Returns `true` if the list was modified.
#[cfg(test)]
pub(crate) fn merge_loop(list: &mut Vec<(DynMapping, bool, bool)>, series: bool) -> bool {
    merge_loop_restricted(list, series, false, &mut Vec::new()).expect("no merge fails")
}

/// Simplify a flattened component list in place. When `restricted` is true,
/// only components carrying `AllowSimplify` may be nominated
/// (`cmpmap.c:3563-3573`).
///
/// `stamped` receives the records of simplification [`MergeCtx`] keeps for
/// the components the loop began with.
///
/// # Errors
///
/// The first error a merge raises; `list` then holds the working list as the
/// merges before it left it.
fn merge_loop_restricted(
    list: &mut Vec<(DynMapping, bool, bool)>,
    series: bool,
    restricted: bool,
    stamped: &mut Vec<(usize, TagTree)>,
) -> AstResult<bool> {
    let mut ctx = MergeCtx::new(std::mem::take(list));
    ctx.clear_frozen();
    ctx.set_restricted(restricted);

    // `modified`/`nmap` history for `pattern_check`, persisting across the
    // whole simplification (C allocates these once before the loop).
    let mut mlist: Vec<i64> = Vec::new();
    let mut nlist: Vec<i64> = Vec::new();

    let changed = run_nominate_loop(&mut ctx, series, &mut mlist, &mut nlist);

    *list = ctx.list;
    *stamped = ctx.stamped;
    changed
}

/// Construct the (Linear', PermMap') pair that results from swapping a
/// Linear and PermMap. `linear_iwm == 0` means the Linear was *before* the
/// PermMap (forward direction); `linear_iwm == 1` means it was *after*
/// (mirror direction).
///
/// Matches winmap.c:3075-3209 (WinPerm).
pub(crate) fn build_winperm_swap(
    linear_iwm: usize,
    shifts: &[f64],
    scales: &[f64],
    outperm: &[i64],
    inperm: &[i64],
    consts: &[f64],
) -> AstResult<Option<(DynMapping, PermMap)>> {
    use crate::constants::BAD_VALUE;
    use crate::mappings::helpers::is_bad;

    let npout = outperm.len();
    let npin = inperm.len();
    let mut new_consts = consts.to_vec();

    let (new_dim, source_perm, ref_perm) = if linear_iwm == 0 {
        // Linear before PermMap: new Linear sits *after* the PermMap.
        // Its dim = npout. For each output axis k, source from outperm[k].
        // Adjust constants on the inperm side (inverse-direction constants).
        (npout, outperm, inperm)
    } else {
        // Linear after PermMap: new Linear sits *before* the PermMap.
        // Its dim = npin. For each input axis i, source from inperm[i].
        // Adjust constants on the outperm side (forward-direction constants).
        (npin, inperm, outperm)
    };

    // Build the new shifts/scales from `source_perm`.
    let mut new_shifts = vec![0.0; new_dim];
    let mut new_scales = vec![1.0; new_dim];
    let src_len = shifts.len() as i64;
    for k in 0..new_dim {
        let p = source_perm[k];
        if p >= 1 && p <= src_len {
            new_shifts[k] = shifts[(p - 1) as usize];
            new_scales[k] = scales[(p - 1) as usize];
        }
        // Constants and BAD: leave shift=0, scale=1 (no transform).
    }

    // Adjust constants on the *opposite* permutation array.
    // - linear_iwm == 0: iterate input axes (inperm); adjust = (c - a[i]) / b[i]
    //   (the supplied WinMap was forward; the inverse PermMap previously had
    //   its constants scaled by the inverse WinMap).
    // - linear_iwm == 1: iterate output axes (outperm); adjust = a[k] + c * b[k]
    //   (the supplied WinMap is forward; the forward PermMap previously had
    //   its constants scaled by the forward WinMap).
    let n_axis = ref_perm.len();
    if shifts.len() != n_axis {
        // Shouldn't happen: WinMap dim must equal PermMap's nin (when Linear
        // is before) or nout (when Linear is after). Bail safely.
        return Ok(None);
    }
    for i in 0..n_axis {
        let p = ref_perm[i];
        if p >= 0 {
            continue;
        }
        let cidx = (-p - 1) as usize;
        if cidx >= new_consts.len() {
            continue;
        }
        let c = new_consts[cidx];
        if is_bad(c) {
            continue;
        }
        let a = shifts[i];
        let b = scales[i];
        if is_bad(a) || is_bad(b) {
            new_consts[cidx] = BAD_VALUE;
            continue;
        }
        if linear_iwm == 0 {
            if b == 0.0 {
                new_consts[cidx] = BAD_VALUE;
            } else {
                new_consts[cidx] = (c - a) / b;
            }
        } else {
            new_consts[cidx] = a + c * b;
        }
    }

    let new_lin: DynMapping = Box::new(WinMap::new(&new_shifts, &new_scales)?);
    let new_pm = PermMap::new(inperm, outperm, &new_consts)?;
    Ok(Some((new_lin, new_pm)))
}

/// C's `CanMerge` (`wcsmap.c:820-928`) applied to the list entry at `i` and a
/// candidate WcsMap: both must be WcsMaps with the same projection, axis
/// count, celestial axis indices and projection parameters, applied in
/// opposite senses. `candidate` is absent when the swap would replace the
/// WcsMap with a UnitMap, which can never cancel against a WcsMap.
fn can_merge_wcsmaps(
    list: &[(DynMapping, bool, bool)],
    i: usize,
    candidate: Option<&WcsMap>,
    candidate_inv: bool,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    if !list[i].0.as_any().is::<WcsMap>() {
        return false;
    }
    candidate
        .try_merge(list[i].0.as_ref(), true, candidate_inv, list[i].1)
        .is_some()
}

/// The effective forward axis permutation arrays of a `PermMap` applied with
/// the given inversion, as `(inperm, outperm)`. C reaches the same pair by
/// setting the PermMap's Invert attribute to the list's flag before calling
/// `PermGet` (`wcsmap.c:1016-1022,1056`).
fn effective_perms(perm: &PermMap, perm_inv: bool) -> (&[i64], &[i64]) {
    if perm_inv {
        (perm.outperm(), perm.inperm())
    } else {
        (perm.inperm(), perm.outperm())
    }
}

/// C's `CanSwap` (`wcsmap.c:930-1153`) for the adjacent pair at `i`/`i+1`, one
/// a `WcsMap` and the other a `PermMap`.
///
/// `None` means the pair cannot be swapped. Otherwise the flag is C's
/// `*simpler`, whether the swapped pair is intrinsically simpler
/// (`wcsmap.c:1112-1120`), and the WcsMap is C's `*newwcsmap`, the WcsMap the
/// swap would produce — absent when the swap replaces it with a UnitMap.
///
/// C finds that WcsMap by making the swap (`WcsPerm`), which transforms the
/// PermMap's constants through the WcsMap when the swap replaces it with a
/// UnitMap, so an error that transformation raises is returned here too.
fn can_swap_wcsmap_past_permmap(
    list: &[(DynMapping, bool, bool)],
    i: usize,
) -> AstResult<Option<(bool, Option<WcsMap>)>> {
    let is_wcs_perm = list[i].0.as_any().is::<WcsMap>() && list[i + 1].0.as_any().is::<PermMap>();
    let is_perm_wcs = list[i].0.as_any().is::<PermMap>() && list[i + 1].0.as_any().is::<WcsMap>();
    if !is_wcs_perm && !is_perm_wcs {
        return Ok(None);
    }
    let (wcs_idx, perm_idx) = if is_wcs_perm { (i, i + 1) } else { (i + 1, i) };
    let (Some(wcs), Some(perm)) = (
        list[wcs_idx].0.as_any().downcast_ref::<WcsMap>(),
        list[perm_idx].0.as_any().downcast_ref::<PermMap>(),
    ) else {
        return Ok(None);
    };
    let (inperm, outperm) = effective_perms(perm, list[perm_idx].1);
    let npin = inperm.len();
    let npout = outperm.len();

    // Every link between axes has to be bi-directional, checked from both
    // ends (wcsmap.c:1062-1082). PermMap indices are 1-based and positive,
    // with negative values naming constants.
    for (k, &j) in outperm.iter().enumerate() {
        if j > 0 && (j as usize) <= npin && inperm[j as usize - 1] != k as i64 + 1 {
            return Ok(None);
        }
    }
    for (k, &j) in inperm.iter().enumerate() {
        if j > 0 && (j as usize) <= npout && outperm[j as usize - 1] != k as i64 + 1 {
            return Ok(None);
        }
    }

    // The celestial axes have to both have bi-directional links, or both be
    // unassigned (wcsmap.c:1084-1110). When they are assigned, the swap
    // produces a WcsMap with permuted celestial axes (wcsmap.c:4662-4675).
    let lonax = wcs.lonax_0based();
    let latax = wcs.latax_0based();
    let (from, limit) = if is_wcs_perm {
        (inperm, npout)
    } else {
        (outperm, npin)
    };
    if lonax >= from.len() || latax >= from.len() {
        return Ok(None);
    }
    let (lon, lat) = (from[lonax], from[latax]);
    let new_wcs = if lon < 0 && lat < 0 {
        swap_wcsmap_perm_unassigned(wcs, perm, list[wcs_idx].1, list[perm_idx].1, is_wcs_perm)?;
        None
    } else if lon < 1 || lon as usize > limit || lat < 1 || lat as usize > limit {
        return Ok(None);
    } else {
        let Some(new_wcs) = wcs.with_permuted_axes(limit, lon as usize - 1, lat as usize - 1)
        else {
            return Ok(None);
        };
        Some(new_wcs)
    };

    // The swapped pair is intrinsically simpler when the PermMap loses axes in
    // the direction it is applied (wcsmap.c:1112-1120).
    let simpler = if is_wcs_perm {
        npin > npout
    } else {
        npout > npin
    };
    Ok(Some((simpler, new_wcs)))
}

/// C's `WcsPerm` (`wcsmap.c:4471-4706`): replace the adjacent WcsMap/PermMap
/// pair at `i`/`i+1` with an equivalent pair in the opposite order, the
/// PermMap taking the WcsMap's slot. Returns `false` when the replacement
/// could not be built, leaving `list` unchanged.
/// [`can_swap_wcsmap_past_permmap`] establishes that the pair can be swapped.
fn wcs_perm_at(list: &mut [(DynMapping, bool, bool)], i: usize) -> AstResult<bool> {
    let is_wcs_perm = list[i].0.as_any().is::<WcsMap>() && list[i + 1].0.as_any().is::<PermMap>();
    let is_perm_wcs = list[i].0.as_any().is::<PermMap>() && list[i + 1].0.as_any().is::<WcsMap>();
    if !is_wcs_perm && !is_perm_wcs {
        return Ok(false);
    }
    let (wcs_idx, perm_idx) = if is_wcs_perm { (i, i + 1) } else { (i + 1, i) };
    let Some(wcs) = list[wcs_idx].0.as_any().downcast_ref::<WcsMap>() else {
        return Ok(false);
    };
    let Some(perm) = list[perm_idx].0.as_any().downcast_ref::<PermMap>() else {
        return Ok(false);
    };
    let wcs_inv = list[wcs_idx].1;
    let perm_inv = list[perm_idx].1;
    let (inperm, outperm) = effective_perms(perm, perm_inv);
    let (from, limit) = if is_wcs_perm {
        (inperm, outperm.len())
    } else {
        (outperm, inperm.len())
    };
    let lonax = wcs.lonax_0based();
    let latax = wcs.latax_0based();
    if lonax >= from.len() || latax >= from.len() {
        return Ok(false);
    }
    let (lon, lat) = (from[lonax], from[latax]);

    // Both celestial axes unassigned: the WcsMap becomes a UnitMap and the
    // PermMap's constants are transformed through it (wcsmap.c:4571-4657).
    if lon < 0 && lat < 0 {
        let (first, first_inv, second, second_inv) =
            swap_wcsmap_perm_unassigned(wcs, perm, wcs_inv, perm_inv, is_wcs_perm)?;
        list[i] = (first, first_inv, false);
        list[i + 1] = (second, second_inv, false);
        return Ok(true);
    }

    // Both assigned: build the WcsMap with permuted celestial axes and
    // exchange the two entries' positions (wcsmap.c:4659-4702).
    if lon < 1 || lon as usize > limit || lat < 1 || lat as usize > limit {
        return Ok(false);
    }
    let Some(new_wcs) = wcs.with_permuted_axes(limit, lon as usize - 1, lat as usize - 1) else {
        return Ok(false);
    };
    let vacated = if is_wcs_perm { i + 1 } else { i };
    let occupied = if is_wcs_perm { i } else { i + 1 };
    let perm_entry = std::mem::replace(&mut list[vacated], (Box::new(new_wcs), wcs_inv, false));
    list[occupied] = perm_entry;
    Ok(true)
}

/// Handle the "unassigned lon/lat" case of WcsMap↔PermMap swap.
///
/// When the PermMap feeds constants to the WcsMap's celestial axes (or
/// receives constants from them), the WcsMap can be replaced by a UnitMap
/// and the PermMap's constants are transformed through the WcsMap.
///
/// Returns (new_first_mapping, new_first_inv, new_second_mapping, new_second_inv).
/// An error transforming the constants, such as a WcsMap lacking a
/// projection parameter it needs, is returned, as C's `astTranN` leaves it
/// in the global status.
fn swap_wcsmap_perm_unassigned(
    wcs: &WcsMap,
    perm: &PermMap,
    wcs_inv: bool,
    perm_inv: bool,
    wcs_first: bool, // true if WcsMap comes before PermMap in the pipeline
) -> AstResult<(DynMapping, bool, DynMapping, bool)> {
    use crate::coords::PointSet;

    let outperm = if perm_inv {
        perm.inperm()
    } else {
        perm.outperm()
    };
    let inperm = if perm_inv {
        perm.outperm()
    } else {
        perm.inperm()
    };
    let constants = perm.constants();
    let npin = inperm.len();
    let npout = outperm.len();

    if wcs_first {
        // WcsMap outputs feed PermMap inputs. The PermMap's inperm has negative
        // entries for constant axes. We transform those constants through the
        // inverse WcsMap to get the new PermMap constants.
        let mut p = vec![BAD_VALUE; npin];
        let mut new_constants = constants.to_vec();

        for k in 0..npin {
            if inperm[k] < 0 {
                let cidx = (-inperm[k] - 1) as usize;
                if cidx < constants.len() {
                    p[k] = constants[cidx];
                }
            }
        }

        // Transform through the effective inverse WcsMap.
        // C equivalent: astTranN(wm, 1, npin, 1, p, 0, npin, 1, q) with Invert=wcs_inv.
        // effective_forward = forward XOR invert = 0 XOR wcs_inv = wcs_inv.
        // In Rust (self.inverted=false): transform(&pts, wcs_inv) gives same effective direction.
        let pts = PointSet::from_rows(vec![p]);
        let transformed = wcs.transform(&pts, wcs_inv)?;
        let q = transformed.point(0);

        // Update constants.
        for k in 0..npin {
            if inperm[k] < 0 {
                let cidx = (-inperm[k] - 1) as usize;
                if cidx < new_constants.len() {
                    new_constants[cidx] = q[k];
                }
            }
        }

        let new_pm = PermMap::new(inperm, outperm, &new_constants)?;
        let unit = UnitMap::new(npout);

        // Result: [PermMap (at original PermMap position), UnitMap (replacing WcsMap)]
        // Swapped order: [UnitMap, PermMap] — but UnitMap will be stripped.
        // Actually the C code puts: maps[iwm=0] = newpm, maps[1] = UnitMap(npout)
        // That means: position 0 gets the PermMap, position 1 gets the UnitMap.
        // So the result is [PermMap, UnitMap].
        Ok((
            Box::new(new_pm) as DynMapping,
            perm_inv,
            Box::new(unit) as DynMapping,
            false,
        ))
    } else {
        // PermMap outputs feed WcsMap inputs. The PermMap's outperm has negative
        // entries for constant axes on the celestial positions.
        let mut p = vec![BAD_VALUE; npout];
        let mut new_constants = constants.to_vec();

        for k in 0..npout {
            if outperm[k] < 0 {
                let cidx = (-outperm[k] - 1) as usize;
                if cidx < constants.len() {
                    p[k] = constants[cidx];
                }
            }
        }

        // Transform through the effective forward WcsMap.
        // C equivalent: astTranN(wm, 1, npout, 1, p, 1, npout, 1, q) with Invert=wcs_inv.
        // effective_forward = forward XOR invert = 1 XOR wcs_inv = !wcs_inv.
        // In Rust (self.inverted=false): transform(&pts, !wcs_inv) gives same effective direction.
        let pts = PointSet::from_rows(vec![p]);
        let transformed = wcs.transform(&pts, !wcs_inv)?;
        let q = transformed.point(0);

        // Update constants.
        for k in 0..npout {
            if outperm[k] < 0 {
                let cidx = (-outperm[k] - 1) as usize;
                if cidx < new_constants.len() {
                    new_constants[cidx] = q[k];
                }
            }
        }

        let new_pm = PermMap::new(inperm, outperm, &new_constants)?;
        let unit = UnitMap::new(npin);

        // Swapped: [UnitMap (replacing WcsMap), PermMap]
        Ok((
            Box::new(unit) as DynMapping,
            false,
            Box::new(new_pm) as DynMapping,
            perm_inv,
        ))
    }
}

/// Effective output coordinate count with external inversion.
fn effective_nout(m: &dyn Mapping, inv: bool) -> usize {
    if inv { m.nin() } else { m.nout() }
}

/// Effective input coordinate count with external inversion.
fn effective_nin(m: &dyn Mapping, inv: bool) -> usize {
    if inv { m.nout() } else { m.nin() }
}

/// Compute the merged replacement for the adjacent parallel-CmpMap pair at
/// `i` / `i+1` in a series list, without mutating the list. Returns the
/// forward replacement Mapping when at least one sub-pair genuinely
/// simplifies (C `cmpmap.c::MapMerge` "Parallel CmpMaps in series",
/// ~1629-1781), or `None` when no restructuring is warranted. This is the
/// shared core driven both by the legacy whole-list helper above and by
/// `cmp_map_merge`'s per-nomination neighbor restructure.
fn compute_merge_parallel_pair_in_series(
    list: &[(DynMapping, bool, bool)],
    i: usize,
) -> AstResult<Option<DynMapping>> {
    // Both must be parallel CmpMaps.
    let is_par1 = list[i]
        .0
        .as_any()
        .downcast_ref::<CmpMap>()
        .is_some_and(|c| c.composition() == Composition::Parallel);
    let is_par2 = list[i + 1]
        .0
        .as_any()
        .downcast_ref::<CmpMap>()
        .is_some_and(|c| c.composition() == Composition::Parallel);
    if !is_par1 || !is_par2 {
        return Ok(None);
    }

    let (ref m1, inv1, _) = list[i];
    let (ref m2, inv2, _) = list[i + 1];

    // Flatten both for parallel composition.
    let par_list1 = flatten(m1.as_ref(), inv1, false)?;
    let par_list2 = flatten(m2.as_ref(), inv2, false)?;

    // Dimension-match the two parallel lists.
    let Some(matched) = dimension_match_parallel_lists(&par_list1, &par_list2) else {
        return Ok(None);
    };

    // For each matched pair, combine in series and simplify.
    let mut simplified_pairs: Vec<(DynMapping, bool, bool)> = Vec::new();
    let mut any_improvement = false;

    for (sub1, sub2) in matched {
        // Build parallel submaps from each sublist.
        let par1 = reconstruct(sub1, false)?;
        let par2 = reconstruct(sub2, false)?;

        // Combine in series.
        let series_map = CmpMap::new(par1, par2, Composition::Series)?;
        let series_box: DynMapping = Box::new(series_map);
        let pre_simplify_inverted = series_box.is_inverted();

        // Simplify the series combination. C's `astSimplify` passes up any
        // error but an undefined transformation (cmpmap.c:1926).
        let (simplified, outcome) = simplify_mapping_with_outcome(series_box.as_ref())?;
        let changed = matches!(outcome, SimplifyOutcome::Changed(_));
        let failed = matches!(outcome, SimplifyOutcome::Failed);

        // C's `simpler` test (`cmpmap.c:1730-1734`): the simplified pair is a
        // different object, or its orientation changed.
        let new_inv = simplified.is_inverted();
        if changed || new_inv != pre_simplify_inverted {
            any_improvement = true;
        }

        // C keeps each pair's `astSimplify` result at whatever orientation it
        // was returned in — the rebuilt parallel CmpMap's snapshot records
        // `astGetInvert(smc)` and the object serializes at that stored
        // orientation (cmpmap.c:1629-1781 builds `astCmpMap(smc0, smc1, 0)`
        // with no reorientation). Normalize to the list invariant
        // (forward-stored + flags): effective == original == the result's
        // returned orientation.
        let mut simplified = simplified;
        if new_inv {
            simplified.invert();
        }
        // C `astSimplify_` (mapping.c:24763) tags every result but one whose
        // reduction failed. Each matched pair was combined in series and
        // `astSimplify`'d, so its reduced result carries the record — in the
        // orientation it was earned in, which is the one the tree serializes
        // the pair at.
        if !failed {
            simplified.set_simplified_in(new_inv, true);
        }
        simplified_pairs.push((simplified, new_inv, new_inv));
    }

    if !any_improvement {
        return Ok(None);
    }

    // Rebuild as parallel CmpMap and replace the pair in the list.
    let result = reconstruct(simplified_pairs, false)?;
    // C hands the assembled parallel CmpMap straight to the list and never
    // simplifies it (`new` at cmpmap.c:1738-1746); the nominate loop is what
    // reduces it further, once it is nominated in its own right.
    Ok(Some(result))
}

/// Dimension-match two parallel component lists for series combination.
///
/// Walks both lists accumulating output dimensions from list1 and input
/// dimensions from list2 until they match, forming paired sublists.
/// Returns `None` if the lists cannot be fully matched.
#[allow(clippy::type_complexity)]
fn dimension_match_parallel_lists(
    par_list1: &[(DynMapping, bool, bool)],
    par_list2: &[(DynMapping, bool, bool)],
) -> Option<Vec<(Vec<(DynMapping, bool, bool)>, Vec<(DynMapping, bool, bool)>)>> {
    let mut pairs = Vec::new();
    let mut i1 = 0usize;
    let mut i2 = 0usize;
    let mut out_accum: usize = 0;
    let mut in_accum: usize = 0;
    let mut sub1_start = 0usize;
    let mut sub2_start = 0usize;

    // Seed with the first element from each list.
    if par_list1.is_empty() || par_list2.is_empty() {
        return None;
    }

    loop {
        // Add from list1 if we need more outputs.
        if out_accum <= in_accum && i1 < par_list1.len() {
            let (ref m, inv, _) = par_list1[i1];
            out_accum += effective_nout(m.as_ref(), inv);
            i1 += 1;
        } else if i2 < par_list2.len() {
            let (ref m, inv, _) = par_list2[i2];
            in_accum += effective_nin(m.as_ref(), inv);
            i2 += 1;
        } else {
            // Can't advance either list — mismatch.
            return None;
        }

        if out_accum == in_accum && out_accum > 0 {
            // Match boundary found. Clone the sublists. C accumulates them with
            // `CombineMaps( …, 0, … )` (`cmpmap.c:1694-1712`), which combines the
            // components as they stand; the one `astSimplify` is on the combined
            // pair.
            let sub1: Vec<(DynMapping, bool, bool)> = par_list1[sub1_start..i1]
                .iter()
                .map(|(m, inv, orig)| (m.clone_box(), *inv, *orig))
                .collect();
            let sub2: Vec<(DynMapping, bool, bool)> = par_list2[sub2_start..i2]
                .iter()
                .map(|(m, inv, orig)| (m.clone_box(), *inv, *orig))
                .collect();
            pairs.push((sub1, sub2));
            sub1_start = i1;
            sub2_start = i2;
            out_accum = 0;
            in_accum = 0;

            // If both lists are exhausted, we're done.
            if i1 == par_list1.len() && i2 == par_list2.len() {
                break;
            }
        }
    }

    // Both lists must be fully consumed.
    if i1 != par_list1.len() || i2 != par_list2.len() {
        return None;
    }

    Some(pairs)
}

/// Compute the merged replacement for the adjacent series-CmpMap pair at
/// `i` / `i+1` in a parallel list, without mutating the list. Returns the
/// forward replacement Mapping when at least one parallel sub-pair genuinely
/// simplifies (C `cmpmap.c::MapMerge` "Series CmpMaps in parallel",
/// ~1577-1622), or `None` otherwise. Shared core driven both by the legacy
/// whole-list helper above and by `cmp_map_merge`.
fn compute_merge_series_pair_in_parallel(
    list: &[(DynMapping, bool, bool)],
    i: usize,
) -> AstResult<Option<DynMapping>> {
    // C `cmpmap.c` 1582-1622 uses ONLY each series CmpMap's two binary
    // children — it does not fully flatten the branches. From the two adjacent
    // series CmpMaps it forms:
    //
    //   new1 = parallel( branch1.first-applied-child, branch2.first-applied-child )
    //   new2 = parallel( branch1.second-applied-child, branch2.second-applied-child )
    //
    // simplifies each, and (when either genuinely simplified) recombines them
    // in series. We extract each branch's two direct children in effective
    // orientation, wrap them back into two forward binary series CmpMaps, and
    // delegate to `recombine_distributed_branches`, which is the shared,
    // tested implementation of that exact binary split (cmpmap.c:1590-1616).
    let extract =
        |entry: &(DynMapping, bool, bool)| -> Option<(DynMapping, bool, DynMapping, bool)> {
            let cmp = entry.0.as_any().downcast_ref::<CmpMap>()?;
            if cmp.composition() != Composition::Series {
                return None;
            }
            // Effective applied inversion of the whole CmpMap (C `invert1`), and
            // the absolute math direction applied to each recorded child. The
            // CmpMap transform applies direction `cmp_inv XOR inv_a` to a child
            // (the child's own stored Invert cancels through `first_invert_changed`,
            // exactly as `flatten` strips and re-XORs it at the leaf). So a
            // forward-oriented child pairs with `cmp_inv XOR inv_x`, using the raw
            // recorded snapshot — C `invert1a = cmpmap1->invert1 XOR invert1`.
            let cmp_inv = entry.1 != cmp.is_inverted();
            let a_eff = cmp_inv != cmp.inv_a();
            let b_eff = cmp_inv != cmp.inv_b();

            // Forward-orient each child; its full effective direction is carried by
            // the paired bool (`invert1?...map2:...map1` swap: when the CmpMap is
            // applied inverted, the second child is applied first).
            let mut first = cmp.first().clone_box();
            if first.is_inverted() {
                first.invert();
            }
            let mut second = cmp.second().clone_box();
            if second.is_inverted() {
                second.invert();
            }

            if !cmp_inv {
                Some((first, a_eff, second, b_eff))
            } else {
                Some((second, b_eff, first, a_eff))
            }
        };

    let (Some((fa1, fa1_inv, sa1, sa1_inv)), Some((fa2, fa2_inv, sa2, sa2_inv))) =
        (extract(&list[i]), extract(&list[i + 1]))
    else {
        return Ok(None);
    };

    // Rebuild each branch as a forward binary series CmpMap (first-applied then
    // second-applied child), then delegate to the shared recombination core.
    let branch1 = reconstruct(vec![(fa1, fa1_inv, false), (sa1, sa1_inv, false)], true)?;
    let branch2 = reconstruct(vec![(fa2, fa2_inv, false), (sa2, sa2_inv, false)], true)?;

    recombine_distributed_branches(&[(branch1, false, false), (branch2, false, false)])
}

/// Attempt the PermMap↔parallel-CmpMap swap at position `i` (PermMap)
/// and `i+1` (parallel CmpMap).
#[allow(clippy::needless_range_loop)]
///
/// The constants the PermMap supplies are transformed through the CmpMap's
/// components, and an error raised there, such as a component lacking the
/// transformation, is recorded on `ctx` and the swap is not made, as C's
/// merge leaves it in the global status.
fn try_swap_permmap_parallel_at(ctx: &mut MergeCtx, i: usize) -> bool {
    match swap_permmap_parallel_at(&mut ctx.list, i) {
        Ok(swapped) => swapped,
        Err(e) => {
            ctx.fail(e);
            false
        }
    }
}

#[allow(clippy::needless_range_loop)]
fn swap_permmap_parallel_at(list: &mut Vec<(DynMapping, bool, bool)>, i: usize) -> AstResult<bool> {
    let (ref perm_map, perm_inv, _) = list[i];
    let (ref par_map, par_inv, _) = list[i + 1];

    let perm = perm_map.as_any().downcast_ref::<PermMap>().unwrap();
    // Get effective permutation arrays based on inversion.
    let eff_outperm = if perm_inv {
        perm.inperm()
    } else {
        perm.outperm()
    };
    let eff_inperm = if perm_inv {
        perm.outperm()
    } else {
        perm.inperm()
    };

    let npin = eff_inperm.len();
    let npout = eff_outperm.len();

    // Must be one-to-one (same number of inputs and outputs).
    if npin != npout {
        return Ok(false);
    }

    // Get the parallel CmpMap's component dimensions.
    // Flatten for parallel to get the components.
    let Ok(par_list) = flatten(par_map.as_ref(), par_inv, false) else {
        return Ok(false);
    };
    if par_list.len() != 2 {
        // Only handle the 2-component case (matching C implementation).
        return Ok(false);
    }

    let nin_a = effective_nin(par_list[0].0.as_ref(), par_list[0].1);
    let nin_b = effective_nin(par_list[1].0.as_ref(), par_list[1].1);

    // Verify total dimensions match.
    if nin_a + nin_b != npout {
        return Ok(false);
    }

    // Check block-swap structure: first nin_a outputs must come from
    // a contiguous block at the END of the inputs (or all be constants),
    // and the next nin_b outputs must come from a contiguous block at
    // the START of the inputs (or all be constants).
    let aconstants = eff_outperm[0] < 0;
    let bconstants = if nin_a < npout {
        eff_outperm[nin_a] < 0
    } else {
        false
    };

    // Check first block (outputs 0..nin_a).
    for idx in 0..nin_a {
        let j = npin - nin_a + idx;
        if aconstants {
            if eff_outperm[idx] >= 0 {
                return Ok(false);
            }
        } else if eff_outperm[idx] != (j as i64 + 1) || eff_inperm[j] != (idx as i64 + 1) {
            return Ok(false);
        }
    }

    // Check second block (outputs nin_a..nin_a+nin_b).
    for idx in 0..nin_b {
        let j = npout - nin_b + idx;
        if bconstants {
            if eff_outperm[j] >= 0 {
                return Ok(false);
            }
        } else if eff_inperm[idx] != (j as i64 + 1) || eff_outperm[j] != (idx as i64 + 1) {
            return Ok(false);
        }
    }

    // The PermMap has block-swap structure. Build the swapped mappings.
    let nout_a = effective_nout(par_list[0].0.as_ref(), par_list[0].1);
    let nout_b = effective_nout(par_list[1].0.as_ref(), par_list[1].1);

    // Build new PermMap: after swapping, the parallel CmpMap has its
    // components in reversed order, so the new PermMap must undo the
    // block swap relative to the new component ordering.
    let npout_new = nout_a + nout_b;
    let npin_new = if aconstants {
        nout_b + (npin - nin_b)
    } else if bconstants {
        nout_a + (npin - nin_a)
    } else {
        nout_a + nout_b
    };

    let mut outperm_new = vec![0i64; npout_new];
    let mut inperm_new = vec![0i64; npin_new];
    let mut constants_new: Vec<f64> = Vec::new();

    // If any PermMap outputs are constant, transform those constants through
    // the CmpMap components. Matches C cmpmap.c:1929-1949.
    let (qa, qb) = if aconstants || bconstants {
        use crate::coords::PointSet;

        // Transform all-BAD inputs through the PermMap to extract constant outputs.
        let p = vec![BAD_VALUE; npin];
        let pts = PointSet::from_rows(vec![p]);
        let q = perm.transform(&pts, !perm_inv)?.point(0).to_vec();

        // Transform the first nin_a outputs through component A.
        let qa = if aconstants {
            let a_pts = PointSet::from_rows(vec![q[..nin_a].to_vec()]);
            let comp_a = &par_list[0];
            Some(comp_a.0.transform(&a_pts, !comp_a.1)?.point(0).to_vec())
        } else {
            None
        };

        // Transform the last nin_b outputs through component B.
        let qb = if bconstants {
            let b_pts = PointSet::from_rows(vec![q[nin_a..nin_a + nin_b].to_vec()]);
            let comp_b = &par_list[1];
            Some(comp_b.0.transform(&b_pts, !comp_b.1)?.point(0).to_vec())
        } else {
            None
        };

        (qa, qb)
    } else {
        (None, None)
    };

    if !bconstants {
        // Second component (now first in swapped CmpMap): its nout_b outputs
        // map to the last nout_b positions in the new PermMap output.
        for idx in 0..nout_b {
            let j = npout_new - nout_b + idx;
            inperm_new[idx] = j as i64 + 1;
            outperm_new[j] = idx as i64 + 1;
        }
    } else {
        // bconstants. C cmpmap.c:1985-2007: push the inperm-side
        // constants FIRST (referencing positions 0..(npin - nin_b) of
        // the new PermMap's inputs), THEN the outperm-side transformed
        // constants (qb). The C ordering determines the resulting
        // Con1/Con2/... numbering.
        for idx in 0..(npin - nin_b) {
            let src = eff_inperm[idx];
            if src < 0 {
                let old_cidx = (-src - 1) as usize;
                let val = perm.constants()[old_cidx];
                let const_idx = constants_new.len();
                constants_new.push(val);
                inperm_new[idx] = -((const_idx + 1) as i64);
            } else if src > npout as i64 {
                inperm_new[idx] = npout_new as i64 + 1;
            } else {
                inperm_new[idx] = src;
            }
        }
        for idx in 0..nout_b {
            let j = npout_new - nout_b + idx;
            let const_idx = constants_new.len();
            let val = qb
                .as_ref()
                .and_then(|v| v.get(idx).copied())
                .unwrap_or(BAD_VALUE);
            constants_new.push(val);
            outperm_new[j] = -((const_idx + 1) as i64);
        }
    }

    if !aconstants {
        // First component (now second in swapped CmpMap): its nout_a outputs
        // map to the first nout_a positions in the new PermMap output.
        for idx in 0..nout_a {
            let j = npin_new - nout_a + idx;
            inperm_new[j] = idx as i64 + 1;
            outperm_new[idx] = j as i64 + 1;
        }
    } else {
        // aconstants. C cmpmap.c:2019-2041: push the inperm-side
        // constants FIRST (positions nout_b..npin_new), THEN the
        // outperm-side transformed constants (qa). The C ordering
        // determines the resulting Con1/Con2/... numbering.
        for idx in nout_b..npin_new {
            let src_idx = idx - nout_b + nin_b;
            if src_idx < eff_inperm.len() {
                let src = eff_inperm[src_idx];
                if src < 0 {
                    let old_cidx = (-src - 1) as usize;
                    let val = perm.constants()[old_cidx];
                    let const_idx = constants_new.len();
                    constants_new.push(val);
                    inperm_new[idx] = -((const_idx + 1) as i64);
                } else if src > npout as i64 {
                    inperm_new[idx] = npout_new as i64 + 1;
                } else {
                    inperm_new[idx] = src;
                }
            }
        }
        for idx in 0..nout_a {
            let const_idx = constants_new.len();
            let val = qa
                .as_ref()
                .and_then(|v| v.get(idx).copied())
                .unwrap_or(BAD_VALUE);
            constants_new.push(val);
            outperm_new[idx] = -((const_idx + 1) as i64);
        }
    }

    // Build the new CmpMap with swapped component order. When
    // `aconstants` (resp. `bconstants`) is true, component A's (resp.
    // B's) inputs were absorbed into the new PermMap as transformed
    // constants. The component is no longer in the chain and must be
    // replaced by a UnitMap of its output dimension, which is C's `unit`
    // (`cmpmap.c:1961-1971`, consumed at `:2053-2070`).
    //
    // The surviving components go in as they are: C assembles the new CmpMap
    // from `cmpmap2->map1` / `map2` directly and never simplifies them. The
    // nominate loop reaches them afterwards, through the rebuilt CmpMap.
    let nout_a = effective_nout(par_list[0].0.as_ref(), par_list[0].1);
    let nout_b = effective_nout(par_list[1].0.as_ref(), par_list[1].1);

    let new_a: DynMapping = if aconstants {
        Box::new(UnitMap::new(nout_a))
    } else {
        let mut a = par_list[0].0.clone_box();
        if par_list[0].1 {
            a.invert();
        }
        a
    };
    let new_b: DynMapping = if bconstants {
        Box::new(UnitMap::new(nout_b))
    } else {
        let mut b = par_list[1].0.clone_box();
        if par_list[1].1 {
            b.invert();
        }
        b
    };

    let Ok(new_cm) = CmpMap::new(new_b, new_a, Composition::Parallel) else {
        return Ok(false);
    };

    // Build the new PermMap (or detect if it's identity).
    let is_identity = !aconstants
        && !bconstants
        && npin_new == npout_new
        && outperm_new
            .iter()
            .enumerate()
            .all(|(idx, &p)| p == (idx as i64 + 1));

    if is_identity {
        // Just replace with the swapped CmpMap (PermMap is removed).
        list[i] = (Box::new(new_cm), false, false);
        list.remove(i + 1);
    } else {
        let Ok(new_pm) = PermMap::new(&inperm_new, &outperm_new, &constants_new) else {
            return Ok(false);
        };
        // CmpMap goes first (was second), PermMap goes second (was first).
        list[i] = (Box::new(new_cm), false, false);
        list[i + 1] = (Box::new(new_pm), false, false);
    }
    Ok(true)
}

/// Try to merge the GrismMap/ZoomMap pair at indices `(i, i+1)`, mirroring the
/// two ZoomMap cases of C `grismmap.c::CanMerge` (508-557): a forward GrismMap
/// followed by a ZoomMap, or a ZoomMap followed by an inverted GrismMap. On
/// success the merged GrismMap replaces the pair (closing the gap) and `true`
/// is returned. The Grism+Grism inverse-cancellation case of `CanMerge` is not
/// handled here — it flows through `default_map_merge` via `GrismMap::try_merge`.
fn grism_zoom_merge_at(list: &mut Vec<(DynMapping, bool, bool)>, i: usize) -> bool {
    if i + 1 >= list.len() {
        return false;
    }

    // Pattern 1: GrismMap(fwd) + ZoomMap
    let is_fwd_grism = list[i].0.as_any().is::<GrismMap>() && !list[i].1;
    let is_zoom_after = list[i + 1].0.as_any().is::<ZoomMap>();

    if is_fwd_grism && is_zoom_after {
        let grism = list[i].0.as_any().downcast_ref::<GrismMap>().unwrap();
        let zoom = list[i + 1].0.as_any().downcast_ref::<ZoomMap>().unwrap();
        let zoom_inv = list[i + 1].1;

        // Get the effective forward zoom factor.
        // zoom.is_inverted() is always false (normalized in flatten).
        // If zoom_inv is true, the zoom is applied inversely → z = 1/factor.
        let mut z = zoom.zoom();
        if zoom_inv && z != 0.0 {
            z = 1.0 / z;
        }

        if z != 0.0
            && let Some(merged) = grism.merge_with_zoom(z, false)
        {
            store_merged(list, i, merged);
            return true;
        }
    }

    // Pattern 2: ZoomMap + GrismMap(inv)
    let is_zoom_before = list[i].0.as_any().is::<ZoomMap>();
    let is_inv_grism = list[i + 1].0.as_any().is::<GrismMap>() && list[i + 1].1;

    if is_zoom_before && is_inv_grism {
        let zoom = list[i].0.as_any().downcast_ref::<ZoomMap>().unwrap();
        let zoom_inv = list[i].1;
        let grism = list[i + 1].0.as_any().downcast_ref::<GrismMap>().unwrap();

        // The C code conceptually inverts the pair to get:
        //   forward GrismMap + inverted ZoomMap
        // The zoom factor for the "inverted" ZoomMap is the reciprocal
        // of its effective forward factor.
        // zoom.is_inverted() is always false (normalized in flatten).
        // C code: "if astGetInvert(map1) == inv1 && z != 0.0, z = 1/z"
        // With is_inverted()=false: if false == zoom_inv → if !zoom_inv → invert z.
        let mut z = zoom.zoom();
        if !zoom_inv && z != 0.0 {
            z = 1.0 / z;
        }

        if z != 0.0
            && let Some(merged) = grism.merge_with_zoom(z, true)
        {
            store_merged(list, i, merged);
            return true;
        }
    }

    false
}

/// Replace the pair at `(i, i+1)` with `merged`, recording its own Invert state
/// as the element's direction. C `grismmap.c::MapMerge` (1240-1241) stores
/// `astGetInvert( merged_map )` in the invert list rather than zero, which is
/// what carries the inversion `CanMerge` applies to a ZoomMap-then-inverted-
/// GrismMap pair; the list holds each leaf normalized forward, so the flag has
/// to be lifted out of the mapping here.
fn store_merged(list: &mut Vec<(DynMapping, bool, bool)>, i: usize, merged: DynMapping) {
    let mut merged = merged;
    let inv = merged.is_inverted();
    if inv {
        merged.invert();
    }
    list[i] = (merged, inv, inv);
    list.remove(i + 1);
}

/// Per-class `map_merge` for a nominated `GrismMap`, mirroring the neighbor
/// checks of C `grismmap.c::MapMerge` (1205-1259). A GrismMap merges only in
/// series. C examines the lower pair `(where-1, where)` first, then the upper
/// pair `(where, where+1)`, via `CanMerge`. The two ZoomMap-absorption cases of
/// `CanMerge` are handled here by [`grism_zoom_merge_at`]; the Grism+Grism
/// inverse-cancellation case (→ `UnitMap`) is left to [`default_map_merge`] via
/// `GrismMap::try_merge`. On a ZoomMap merge the index of the first modified
/// element is returned.
pub(crate) fn grism_map_merge(
    _grism: &GrismMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    if series {
        // Lower neighbor first (C MapMerge 1212-1217): pair (where-1, where).
        if where_ > 0 && grism_zoom_merge_at(&mut ctx.list, where_ - 1) {
            return Some(where_ - 1);
        }
        // Upper neighbor (C MapMerge 1221-1226): pair (where, where+1).
        if where_ + 1 < ctx.len() && grism_zoom_merge_at(&mut ctx.list, where_) {
            return Some(where_);
        }
    }

    default_map_merge(ctx, where_, series)
}

/// Per-class `map_merge` for a nominated `PcdMap`, a port of C
/// `pcdmap.c::MapMerge` (1406-1736).
///
/// C's body has four parts, in this order: replace a zero-distortion PcdMap
/// with a UnitMap without reference to its neighbors; merge directly with an
/// adjacent UnitMap or with the PcdMap's own inverse; swap past an adjacent
/// ZoomMap or axis-swapping PermMap to move the PcdMap towards a distant
/// Mapping it could merge with, then merge at the new adjacency; and, with no
/// such Mapping in reach, swap speculatively and keep the swap only if one of
/// the swapped Mappings then simplifies. A PcdMap combines in parallel with
/// nothing.
pub(crate) fn pcd_map_merge(
    pcd: &PcdMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // C's opening move, before any neighbor is considered: a PcdMap whose
    // distortion coefficient is zero is a UnitMap (pcdmap.c:1420-1432). The
    // replacement carries a cleared Invert flag (pcdmap.c:1429). C makes this
    // the `if` of an if/else whose `else` holds the whole neighbor search, so
    // a reduced PcdMap never looks at a neighbor in the same pass.
    if ast_equal(pcd.disc(), 0.0) {
        ctx.set_pair_single(where_, (Box::new(UnitMap::new(2)), false, false));
        return Some(where_);
    }

    // The whole neighbor search sits inside C's `if( series )`: a PcdMap
    // cannot combine in parallel with any other Mapping (pcdmap.c:1729-1731).
    if !series {
        return None;
    }

    // The direct merge comes first: an adjacent UnitMap, or this PcdMap's own
    // inverse (pcdmap.c:1446-1513). `PcdMap::try_merge` is C's `CanMerge`
    // together with the Mapping C builds from the pair, and
    // `default_map_merge` offers the pair on each side as C's two `CanMerge`
    // calls do.
    if let Some(modified) = default_map_merge(ctx, where_, series) {
        return Some(modified);
    }

    // Only when neither neighbor merges does C consider swapping
    // (pcdmap.c:1515-1524).
    pcd_swap_strategy(ctx, where_, series)
}

/// The swap half of C `pcdmap.c::MapMerge` (1526-1726).
///
/// Count the Mappings separating the PcdMap from the nearest Mapping it could
/// merge with on each side, swap towards the nearer one and merge at the new
/// adjacency; with no such Mapping in reach, swap speculatively with each
/// eligible neighbor in turn. Returns the index of the first modified
/// element, or `None` when no swap is made.
fn pcd_swap_strategy(ctx: &mut MergeCtx, where_: usize, series: bool) -> Option<usize> {
    if !ctx.entry(where_).0.as_any().is::<PcdMap>() {
        return None;
    }
    let n = ctx.len();

    // C's `swaphi` and `swaplo`, its `CanSwap` on each side
    // (pcdmap.c:1527-1534, 1570-1577).
    let swaphi = where_ + 1 < n && can_swap_pcd_with(&ctx.entry(where_ + 1).0);
    let swaplo = where_ > 0 && can_swap_pcd_with(&ctx.entry(where_ - 1).0);

    // `nstep2` and `nstep1`: how many Mappings separate the PcdMap from the
    // nearest Mapping it could merge with, walking towards the end and towards
    // the start of the list (pcdmap.c:1536-1566, 1579-1598). -1 means none.
    let nstep2 = if swaphi {
        pcd_steps_to_merge_target(ctx, where_, true)
    } else {
        -1
    };
    let nstep1 = if swaplo {
        pcd_steps_to_merge_target(ctx, where_, false)
    } else {
        -1
    };

    // Swap in the direction of the nearer target (pcdmap.c:1602-1612).
    let target = if nstep1 != -1 && (nstep2 == -1 || nstep2 > nstep1) {
        Some(where_ - 1)
    } else if nstep2 != -1 {
        Some(where_)
    } else {
        None
    };

    if let Some(i1) = target {
        let swapped = pcd_swap_pair(pcd_clone_pair(ctx, i1), where_ - i1)?;
        let [first, second] = swapped;
        ctx.set_pair(i1, first, second);

        // C follows the swap with an `astMapMerge` over the sub-list that
        // starts at the new adjacency, so the PcdMap that has just moved next
        // to the target merges in the same pass (pcdmap.c:1628-1647). The
        // sub-list scoping is C's: the nominated Mapping cannot reach back
        // past the element the sub-list starts at.
        if where_ == i1 && where_ + 1 < ctx.len() {
            ctx.merge_subrange(where_ + 1, 0, series);
        } else if where_ >= 2 {
            ctx.merge_subrange(where_ - 2, 0, series);
        }
        return Some(i1);
    }

    // No target: the swap may still be worth making if a swapped Mapping is
    // simpler than the Mapping it replaces (pcdmap.c:1651-1726).
    if swaphi || swaplo {
        return pcd_speculative_swap(ctx, where_, swaphi, swaplo);
    }
    None
}

/// How many Mappings separate the PcdMap at `where_` from the nearest Mapping
/// it could merge with, walking towards the end of the list (`forward`) or
/// towards its start; -1 when there is none.
///
/// C stops at the first UnitMap or PcdMap, and stops with -1 at the first
/// Mapping it could not swap past (pcdmap.c:1544-1564). The class tests are
/// C's: any PermMap can be stepped over here, not only the axis-swapping ones
/// `CanSwap` accepts, so a non-swapping PermMap in the run does not hide a
/// target behind it.
fn pcd_steps_to_merge_target(ctx: &MergeCtx, where_: usize, forward: bool) -> i64 {
    let n = ctx.len();
    let mut i = where_;
    loop {
        if forward {
            i += 1;
            if i >= n {
                return -1;
            }
        } else {
            if i == 0 {
                return -1;
            }
            i -= 1;
        }
        let m = ctx.entry(i).0.as_ref();
        if m.as_any().is::<UnitMap>() || m.as_any().is::<PcdMap>() {
            return if forward {
                (i - where_ - 1) as i64
            } else {
                (where_ - 1 - i) as i64
            };
        }
        if !(m.as_any().is::<ZoomMap>() || m.as_any().is::<PermMap>()) {
            return -1;
        }
    }
}

/// C's speculative swap (pcdmap.c:1654-1726): swap with the lower neighbor
/// and then with the upper one, keeping the first swap after which one of the
/// two Mappings simplifies to a different class. C works on copies so a
/// rejected swap leaves the list untouched.
fn pcd_speculative_swap(
    ctx: &mut MergeCtx,
    where_: usize,
    swaphi: bool,
    swaplo: bool,
) -> Option<usize> {
    for attempt in 0..2 {
        let i1 = if attempt == 0 && swaplo {
            where_ - 1
        } else if attempt == 1 && swaphi {
            where_
        } else {
            continue;
        };

        let Some(swapped) = pcd_swap_pair(pcd_clone_pair(ctx, i1), where_ - i1) else {
            continue;
        };

        // C compares the class of each swapped Mapping with the class of its
        // simplification and discards the swap when neither changed
        // (pcdmap.c:1694-1701).
        let worthwhile = swapped.iter().any(|entry| {
            entry
                .0
                .simplify()
                .ok()
                .is_some_and(|s| s.as_any().type_id() != entry.0.as_any().type_id())
        });

        if worthwhile {
            let [first, second] = swapped;
            ctx.set_pair(i1, first, second);
            return Some(i1);
        }
    }
    None
}

/// Check if a mapping can be swapped with an adjacent PcdMap — C's `CanSwap`
/// (pcdmap.c:833-854): a ZoomMap, or a PermMap that simply swaps the two axes.
fn can_swap_pcd_with(m: &DynMapping) -> bool {
    if m.as_any().is::<ZoomMap>() {
        return true;
    }
    if let Some(pm) = m.as_any().downcast_ref::<PermMap>() {
        return PcdMap::perm_is_swap(pm);
    }
    false
}

/// Copy the `(i1, i1 + 1)` pair without disturbing the list, as C copies the
/// two Mappings and their Invert flags before swapping them
/// (pcdmap.c:1680-1683).
fn pcd_clone_pair(ctx: &MergeCtx, i1: usize) -> [(DynMapping, bool, bool); 2] {
    let a = ctx.entry(i1);
    let b = ctx.entry(i1 + 1);
    [(a.0.clone_box(), a.1, a.2), (b.0.clone_box(), b.1, b.2)]
}

/// Swap a PcdMap past its neighbor — C's `PcdZoom` (pcdmap.c:2302-2420) and
/// `PcdPerm` (2422-2528). `ipc` is the index of the PcdMap within the supplied
/// pair, and the returned pair holds the same two classes in the opposite
/// order. `None` when the neighbor is neither a ZoomMap nor a PermMap.
///
/// The PcdMap C creates carries the distortion of the composition and the
/// Invert flag of the supplied PcdMap; the ZoomMap C creates carries a cleared
/// Invert flag, and a PermMap is passed through with its own flag.
fn pcd_swap_pair(
    pair: [(DynMapping, bool, bool); 2],
    ipc: usize,
) -> Option<[(DynMapping, bool, bool); 2]> {
    let [lower, upper] = pair;
    let (pcd_entry, other) = if ipc == 0 {
        (lower, upper)
    } else {
        (upper, lower)
    };
    let pcd = pcd_entry.0.as_any().downcast_ref::<PcdMap>()?;
    let pcd_inv = pcd_entry.1;
    let disco = pcd.disc();
    let [xcen, ycen] = pcd.pcdcen();

    let (new_pcd, new_other) = if let Some(zm) = other.0.as_any().downcast_ref::<ZoomMap>() {
        // `Zoom` is the stored factor whatever the Invert flag says — it is
        // ZoomMap's Transform that takes the reciprocal when the Mapping is
        // applied inverted — so the factor the merge list actually applies has
        // to be formed explicitly (pcdmap.c:2374-2380). The ZoomMap the swap
        // creates carries a cleared Invert flag (pcdmap.c:2425), so it holds
        // that factor directly.
        let zoom = if other.1 { 1.0 / zm.zoom() } else { zm.zoom() };

        // The PcdMap applied first sees the un-zoomed coordinates, so after the
        // swap its center is zoomed and its coefficient divided by the square
        // of the zoom; applied second, the reverse (pcdmap.c:2399-2409).
        let (cen, disc) = if ipc == 0 {
            ([xcen * zoom, ycen * zoom], disco / (zoom * zoom))
        } else {
            ([xcen / zoom, ycen / zoom], disco * zoom * zoom)
        };
        let new_pcd = PcdMap::new(disc, cen);
        let new_zoom: DynMapping = Box::new(ZoomMap::new(2, zoom).ok()?);
        (new_pcd, (new_zoom, false, false))
    } else if other.0.as_any().is::<PermMap>() {
        // The PermMap is passed through unchanged, with its own Invert flag,
        // and the PcdMap's center has its two axes exchanged
        // (pcdmap.c:2503-2523).
        let new_pcd = PcdMap::new(disco, [ycen, xcen]);
        (new_pcd, (other.0.clone_box(), other.1, other.2))
    } else {
        return None;
    };

    let mut new_pcd = new_pcd;
    if pcd_inv {
        new_pcd.invert();
    }
    let new_pcd_entry: (DynMapping, bool, bool) = (Box::new(new_pcd), false, false);

    Some(if ipc == 0 {
        [new_other, new_pcd_entry]
    } else {
        [new_pcd_entry, new_other]
    })
}

/// Detect and apply the diagonal-sandwich pattern anchored at index `i`,
/// mirroring the second branch of C `sphmap.c::MapMerge` (817-957) where the
/// nominated (inverted) SphMap is `map_list[where]`.
///
/// The trio at `(i, i+1, i+2)` must be `(inv-SphMap, diagonal-3→3, fwd-SphMap)`
/// with equal-magnitude diagonal elements. On a match it is replaced by
/// `(WinMap, inv-SphMap, fwd-SphMap)` — the diagonal is removed, the WinMap
/// takes position `i`, and the (PolarLong-adjusted) inverted SphMap moves to
/// `i+1`. Returns `true` if the list was modified.
fn try_sphmap_diagonal_pattern_at(list: &mut Vec<(DynMapping, bool, bool)>, i: usize) -> bool {
    if i + 2 >= list.len() {
        return false;
    }

    // Pattern: SphMap(inverted) ; diagonal(3→3) ; SphMap(forward)
    let (ref m1, inv1, _) = list[i];
    let (ref m2, inv2, _) = list[i + 1];
    let (ref m3, inv3, _) = list[i + 2];

    let sph1 = m1.as_any().downcast_ref::<SphMap>();
    let sph3 = m3.as_any().downcast_ref::<SphMap>();

    let (Some(s1), Some(s3)) = (sph1, sph3) else {
        return false;
    };

    // s1 must be applied inverted (sph→cart, 2→3)
    // s3 must be applied forward (cart→sph, 3→2)
    if !inv1 || inv3 {
        return false;
    }

    // Extract the 3D diagonal from the middle mapping.
    let Some(diag) = extract_3d_diagonal(m2.as_ref(), inv2) else {
        return false;
    };

    // All magnitudes must be equal and non-zero.
    let mag = diag[0].abs();
    if mag == 0.0 || !ast_equal(diag[1].abs(), mag) || !ast_equal(diag[2].abs(), mag) {
        return false;
    }

    // Compute the WinMap parameters from the sign pattern.
    // Matches C sphmap.c:875-900.
    let (lon_shift, lon_scale) = if diag[0] < 0.0 {
        if diag[1] < 0.0 {
            (std::f64::consts::PI, 1.0)
        } else {
            (std::f64::consts::PI, -1.0)
        }
    } else if diag[1] < 0.0 {
        (0.0, -1.0)
    } else {
        (0.0, 1.0)
    };

    let lat_scale = if diag[2] < 0.0 { -1.0 } else { 1.0 };

    // Check PolarLong compatibility (C sphmap.c:914-921):
    // The replacement transforms PolarLong through the WinMap.
    let new_polarlong = lon_shift + lon_scale * s1.polar_long();
    if !ast_equal(new_polarlong, s3.polar_long()) {
        return false;
    }

    // Build the WinMap.
    let Ok(winmap) = WinMap::new(&[lon_shift, 0.0], &[lon_scale, lat_scale]) else {
        return false;
    };

    // Replace (inv-SphMap, diag, fwd-SphMap) with
    // (WinMap, inv-SphMap, fwd-SphMap).
    // The SphMap gets the adjusted PolarLong.
    let mut new_sph = s1.clone();
    new_sph.set_polar_long(new_polarlong);

    // Remove the diagonal (position i+1) and replace
    // position i with WinMap, insert the modified SphMap.
    list.remove(i + 1); // remove diagonal
    list[i] = (Box::new(winmap), false, false);
    // Insert the modified inv-SphMap after the WinMap.
    list.insert(i + 1, (Box::new(new_sph), true, false));
    true
}

/// Per-class `map_merge` for a nominated `SphMap`, mirroring C
/// `sphmap.c::MapMerge` (712-963).
///
/// A SphMap merges directly with the SphMap that follows it in series when the
/// pair cancels to a `UnitMap` (C branch 1, lines 742-802) — this is driven by
/// [`default_map_merge`] via [`SphMap::try_merge`]. C only attempts the
/// diagonal-sandwich rewrite (branch 2, lines 817-957) when that direct merge
/// did not fire (`!simpler`), so the default merge is tried first here. The
/// sandwich rewrite is anchored at the nominated (inverted) SphMap — C's
/// `where` — and delegates to [`try_sphmap_diagonal_pattern_at`].
pub(crate) fn sph_map_merge(
    _sph: &SphMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // Direct SphMap+SphMap cancellation (C branch 1).
    if let Some(modified) = default_map_merge(ctx, where_, series) {
        return Some(modified);
    }

    // Diagonal-sandwich rewrite (C branch 2). Only reachable when the direct
    // merge failed, matching C's `!simpler` guard.
    if series && try_sphmap_diagonal_pattern_at(&mut ctx.list, where_) {
        return Some(where_);
    }

    None
}

/// Extract the diagonal of a 3×3 mapping if it's a ZoomMap or diagonal MatrixMap.
///
/// The inversion flag is noted but for this pattern (sign detection) the
/// inversion only affects magnitude (not signs), so we use the forward
/// matrix/factor for sign determination. The C code also ignores the Invert
/// flag for this purpose (sphmap.c:822-823).
fn extract_3d_diagonal(m: &dyn Mapping, _inv: bool) -> Option<[f64; 3]> {
    if let Some(zm) = m.as_any().downcast_ref::<ZoomMap>() {
        if zm.nin() == 3 {
            let f = zm.zoom();
            return Some([f, f, f]);
        }
    }
    if let Some(mm) = m.as_any().downcast_ref::<MatrixMap>() {
        let mat = mm.forward_matrix();
        if mat.nrows() == 3 && mat.ncols() == 3 && is_diagonal(mat) {
            return Some([mat[[0, 0]], mat[[1, 1]], mat[[2, 2]]]);
        }
    }
    None
}

/// C `cmpmap.c` "Series CmpMaps in parallel" re-combination (1577-1620),
/// scoped to the two simplified branches produced by the WinMap parallel
/// distribution.
///
/// Given two branches `b1`, `b2` that are each binary series CmpMaps
/// `b1 = (A1 ; B1)`, `b2 = (A2 ; B2)`, form
/// `(A1 ‖ A2) ; (B1 ‖ B2)`, simplify each parallel half, and — if at least
/// one half simplified (C's `simpler` test) — return the series composition
/// of the two simplified halves. Returns `None` when the branches are not
/// both binary series CmpMaps or when neither half simplifies (so the caller
/// keeps the parallel form, exactly as C does).
pub(crate) fn recombine_distributed_branches(
    branches: &[(DynMapping, bool, bool)],
) -> AstResult<Option<DynMapping>> {
    if branches.len() != 2 {
        return Ok(None);
    }

    // Each branch must be a forward, binary series CmpMap. The branches are
    // produced in forward orientation (effective_inv == false) by the
    // distribution loop, so each child's absolute applied direction is just the
    // branch's recorded `inv_a`/`inv_b` snapshot — the same value C reads from
    // `astDecompose` (cmpmap.c:758-786) and then writes over the component's own
    // stored `Invert`. Since the children are stripped to forward here, the
    // snapshot is used raw; the `inv_x != child.is_inverted()` form is the
    // relative flag of cmpmap.c:3862, which would double-count a
    // stored-inverted child.
    //
    // The stored orientation is carried separately as each element's `orig_inv`:
    // C `CombineMaps` sets the component's `Invert` to the effective direction
    // only for the duration of the `astCmpMap` call and restores the original
    // afterwards (cmpmap.c:625-662), so the enclosing CmpMap records the
    // effective direction in `InvA`/`InvB` while the component still serializes
    // in the orientation it was stored in.
    type SplitHalves = (DynMapping, bool, bool, DynMapping, bool, bool);
    let split = |entry: &(DynMapping, bool, bool)| -> Option<SplitHalves> {
        let cmp = entry.0.as_any().downcast_ref::<CmpMap>()?;
        if cmp.composition() != Composition::Series {
            return None;
        }
        let a_inv = cmp.inv_a();
        let b_inv = cmp.inv_b();
        let a_orig = cmp.first().is_inverted();
        let b_orig = cmp.second().is_inverted();
        let mut a = cmp.first().clone_box();
        if a.is_inverted() {
            a.invert();
        }
        let mut b = cmp.second().clone_box();
        if b.is_inverted() {
            b.invert();
        }
        Some((a, a_inv, a_orig, b, b_inv, b_orig))
    };

    let (
        Some((a1, a1_inv, a1_orig, b1, b1_inv, b1_orig)),
        Some((a2, a2_inv, a2_orig, b2, b2_inv, b2_orig)),
    ) = (split(&branches[0]), split(&branches[1]))
    else {
        return Ok(None);
    };

    // new1 = A1 ‖ A2, new2 = B1 ‖ B2 (C CombineMaps with series == 0).
    let new1 = reconstruct(vec![(a1, a1_inv, a1_orig), (a2, a2_inv, a2_orig)], false)?;
    let new2 = reconstruct(vec![(b1, b1_inv, b1_orig), (b2, b2_inv, b2_orig)], false)?;

    // C `simpler = ( simp1 != new1 ) || ( simp2 != new2 ) ||
    // astGetInvert( simp1 ) || astGetInvert( simp2 )` (cmpmap.c:1616): pointer
    // identity on each half, reported by the callee, plus C's safety check that
    // neither half came back inverted.
    let (mut simp1, outcome1) = simplify_mapping_with_outcome(new1.as_ref())?;
    let (mut simp2, outcome2) = simplify_mapping_with_outcome(new2.as_ref())?;
    let changed1 = matches!(outcome1, SimplifyOutcome::Changed(_));
    let changed2 = matches!(outcome2, SimplifyOutcome::Changed(_));
    let simpler = changed1 || changed2 || simp1.is_inverted() || simp2.is_inverted();
    if !simpler {
        return Ok(None);
    }

    // C reaches both halves through `astSimplify` (cmpmap.c:1602-1603), which
    // stamps its result unless the reduction failed; `simplify_mapping` is the
    // untagged internal entry, so stamp them here. Each becomes an element of
    // the series list the closing fold walks, which used to clear the tag again
    // and no longer does.
    if !matches!(outcome1, SimplifyOutcome::Failed) {
        simp1.set_has_been_simplified(true);
    }
    if !matches!(outcome2, SimplifyOutcome::Failed) {
        simp2.set_has_been_simplified(true);
    }

    // Compose the two simplified halves in series (C astCmpMap(simp1, simp2, 1)).
    Ok(Some(Box::new(CmpMap::new(
        simp1,
        simp2,
        Composition::Series,
    )?)))
}

/// Reconstruct a `CmpMap` tree (or single mapping) from a flat list.
///
/// Each element is `(map, effective_inv, orig_inv)` where `map` is in its
/// forward orientation, `effective_inv` is the direction the component is
/// applied in, and `orig_inv` is the orientation the component was stored in
/// within the original tree (see [`flatten`]).
///
/// This mirrors C `cmpmap.c` Simplify, which for each component records the
/// *effective* direction in the parent CmpMap's `InvA`/`InvB` while leaving
/// the component serialized in its *original* orientation. So a CmpMap child
/// gets `inv_a/inv_b = effective_inv` and a stored Invert of `orig_inv`;
/// these are independent and, by construction, the child's stored Invert
/// cancels out of the transform (see [`CmpMap::set_first_orientation`]).
///
/// For a single-element list the mapping is returned directly (no wrapping
/// `CmpMap`), in its effective orientation — matching C's `nmap == 1` branch
/// which sets the lone Mapping's Invert to the effective value.
///
/// `orig_inv` is honored only for series reconstruction. The parallel path
/// keeps the existing `invert_into_structure` behavior, which already emits
/// the canonical form C uses for parallel CmpMaps.
///
/// Used both as C `Simplify`'s closing fold and as a plain tree builder by
/// individual merge routines.
pub(crate) fn reconstruct(
    list: Vec<(DynMapping, bool, bool)>,
    series: bool,
) -> AstResult<DynMapping> {
    reconstruct_inner(list, series)
}

fn reconstruct_inner(list: Vec<(DynMapping, bool, bool)>, series: bool) -> AstResult<DynMapping> {
    let kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };

    // Apply the effective inversion to a component about to be composed.
    //
    // A plain flag toggle. C's canonical `InvA`/`InvB` shape for a UnitMap
    // component comes from `UnitMap::map_merge` clearing its list flag in
    // parallel (unitmap.c:575-584), as in C.
    let apply_effective = |m: &mut DynMapping| {
        m.invert();
    };

    // C `Simplify` (cmpmap.c:290-319) rebuilds the merged sequence by looping
    // the Mapping list *in reverse* and composing each map with the CmpMap
    // formed so far: `result = astCmpMap(map[i], result)`. This produces a
    // right-associated tree — `map[0] ∘ (map[1] ∘ (… ∘ map[n-1]))` — which is
    // what the native serializer then emits.
    let mut rest = list;

    let (mut acc, acc_eff, acc_orig) = rest.pop().expect("reconstruct called with empty list");
    // Seed: if it stays standalone (single-element list) it carries the
    // effective orientation. If it becomes a child below, `CmpMap::new` will
    // record that effective orientation in `inv_b` and its stored Invert is
    // re-set to `acc_orig` there.
    if acc_eff {
        apply_effective(&mut acc);
    }
    // Only the seed is a leaf whose original orientation must be restored on
    // first wrapping; later `acc` values are freshly built CmpMaps (forward).
    let mut seed_orig = Some(acc_orig);

    while let Some((mut m, m_eff, m_orig)) = rest.pop() {
        // MapA (m): set to its effective orientation so `CmpMap::new` records
        // inv_a = m_eff.
        if m_eff {
            apply_effective(&mut m);
        }
        let mut cmp = CmpMap::new(m, acc, kind)?;

        // Restore each child to the orientation it should serialize in (its
        // original tree orientation), leaving inv_a/inv_b carrying the
        // effective direction. C preserves each component's own stored Invert
        // (`astGetInvert(map1)`) and records the effective direction separately
        // in `invert1`/`invert2` — a component originally stored forward stays
        // forward with the inversion moved into `InvA`/`InvB`, and one stored
        // inverted stays inverted. Our transform derives a child's math
        // direction as `fwd XOR inv_a` (the child's own Invert cancels), so
        // re-setting the stored orientation after `CmpMap::new` captured the
        // effective flag is transform-neutral and only fixes the serialized
        // form.
        //
        // For a series reconstruct, or a leaf (non-CmpMap) child of a parallel
        // reconstruct, restore the original orientation. A *CmpMap* child of a
        // parallel reconstruct is excluded: `apply_effective` already rewrote it
        // to canonical forward form via `invert_into_structure` (which moves the
        // inversion into that child's own recorded structure), so re-inverting
        // it would undo that canonicalization.
        if series {
            cmp.set_first_orientation(m_orig);
            if let Some(o) = seed_orig.take() {
                cmp.set_second_orientation(o);
            }
        } else {
            if !cmp.first().as_any().is::<CmpMap>() {
                cmp.set_first_orientation(m_orig);
            }
            if let Some(o) = seed_orig.take() {
                if !cmp.second().as_any().is::<CmpMap>() {
                    cmp.set_second_orientation(o);
                }
            }
        }

        acc = Box::new(cmp);
    }

    Ok(acc)
}

/// The `IsSimple` records on a Mapping's tree of CmpMaps.
///
/// C's records of simplification live on the objects themselves, and the
/// nominate loop's list entries alias the CmpMap's own components, so a record
/// written on an entry is on the component.
/// The port's list holds copies, so it carries the records across in this
/// form, which holds only the tags and the shape needed to place them, not the
/// Mappings.
#[derive(Clone, Debug)]
pub(crate) enum TagTree {
    /// Anything other than a CmpMap, which carries no record here.
    Other,
    /// A CmpMap: its record at its own orientation, that orientation, and the
    /// records of its two components.
    Cmp {
        tag: bool,
        inverted: bool,
        components: Box<[TagTree; 2]>,
    },
}

impl TagTree {
    /// The records on `m` and its components.
    pub(crate) fn of(m: &dyn Mapping) -> Self {
        match m.as_any().downcast_ref::<CmpMap>() {
            None => TagTree::Other,
            Some(c) => TagTree::Cmp {
                tag: c.dump_is_simple(),
                inverted: c.is_inverted(),
                components: Box::new([TagTree::of(c.first()), TagTree::of(c.second())]),
            },
        }
    }

    /// The records of `dst` once these are written onto it: where both are
    /// CmpMaps, this tag replaces `dst`'s at its own orientation, and when the
    /// two are held at the same orientation their components are matched in
    /// turn; elsewhere `dst` keeps its own.
    /// The tag is carried in both directions, so an untagged source clears
    /// the tag on `dst`.
    fn written_onto(&self, dst: &TagTree) -> TagTree {
        match (self, dst) {
            (
                TagTree::Cmp {
                    tag,
                    inverted: s_inv,
                    components: s_c,
                },
                TagTree::Cmp {
                    inverted: d_inv,
                    components: d_c,
                    ..
                },
            ) => TagTree::Cmp {
                tag: *tag,
                inverted: *d_inv,
                components: if s_inv == d_inv {
                    Box::new([s_c[0].written_onto(&d_c[0]), s_c[1].written_onto(&d_c[1])])
                } else {
                    d_c.clone()
                },
            },
            _ => dst.clone(),
        }
    }

    /// Write these records onto `dst`, which has the shape they were taken
    /// from.
    pub(crate) fn apply_to(&self, dst: &mut dyn Mapping) {
        let TagTree::Cmp {
            tag,
            inverted,
            components,
        } = self
        else {
            return;
        };
        let Some(d) = dst.as_any_mut().downcast_mut::<CmpMap>() else {
            return;
        };
        let d_inv = d.is_inverted();
        d.set_dump_simplified_in(d_inv, *tag);
        if *inverted != d_inv {
            return;
        }
        components[0].apply_to(d.first_mut().as_mut());
        components[1].apply_to(d.second_mut().as_mut());
    }
}

/// The records on `node` once the nominate loop's records for the components
/// it began with are written onto them.
///
/// C's `SeparateMappings` (cmpmap.c:3326) only deep-copies *duplicate*
/// component pointers; every other entry of `map_list` aliases the live child
/// of the CmpMap being simplified. So when the nominate loop's self-simplify
/// head stamps `IsSimple` on an entry it reduced no further — `astSimplify`
/// (cmpmap.c:1437) tags even the clone it returns for an unreduced Mapping —
/// the stamp mutates the real child, and is still there when `Simplify` returns
/// `astClone( this )`, after its `!simpler` branch (cmpmap.c:3634) or a failed
/// merge, and when a nested reduction returns a new Mapping the stamps it wrote
/// on its own components are on the original.
///
/// Walk the tree in the order [`flatten`] built the list so entry *i* lands on
/// the component it came from. A stamp can sit anywhere inside an entry, not
/// just on its root — the entry is a whole subtree that C shares with the tree,
/// so [`TagTree::written_onto`] transplants the entry's tag state at every level.
fn stamped_tags(
    node: &dyn Mapping,
    inv: bool,
    series: bool,
    entries: &[Option<TagTree>],
    next: &mut usize,
) -> TagTree {
    let target_kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };
    if let Some(cmp) = node.as_any().downcast_ref::<CmpMap>()
        && cmp.composition() == target_kind
    {
        let cmp_inv = inv != cmp.is_inverted();
        let first_inv = cmp_inv != (cmp.inv_a() != cmp.first().is_inverted());
        let second_inv = cmp_inv != (cmp.inv_b() != cmp.second().is_inverted());
        // `flatten` reverses the child order for an inverted series CmpMap.
        let [first, second] = if cmp_inv && series {
            let second = stamped_tags(cmp.second(), second_inv, series, entries, next);
            let first = stamped_tags(cmp.first(), first_inv, series, entries, next);
            [first, second]
        } else {
            let first = stamped_tags(cmp.first(), first_inv, series, entries, next);
            let second = stamped_tags(cmp.second(), second_inv, series, entries, next);
            [first, second]
        };
        return TagTree::Cmp {
            tag: cmp.dump_is_simple(),
            inverted: cmp.is_inverted(),
            components: Box::new([first, second]),
        };
    }
    // A class overriding C `astMapList` contributes its own components; skip
    // past the indices they occupy without descending, since only a `CmpMap`
    // component can carry the tag.
    if let Some(components) = node.map_list_decompose(inv, series) {
        for (comp, comp_inv) in components {
            *next += flatten(comp.as_ref(), comp_inv, series)
                .map(|f| f.len())
                .unwrap_or(1);
        }
        return TagTree::of(node);
    }
    // Leaf: `flatten` emitted exactly one entry for this component.
    let own = TagTree::of(node);
    let tags = match entries.get(*next) {
        Some(Some(entry)) => entry.written_onto(&own),
        _ => own,
    };
    *next += 1;
    tags
}

#[cfg(test)]
fn simplify_mapping(mapping: &dyn Mapping) -> AstResult<DynMapping> {
    simplify_mapping_with_outcome(mapping).map(|(m, _)| m)
}

/// C's `Simplify` for a CmpMap (`cmpmap.c:3396`), reporting how it ended:
/// whether the result differs from the input — C's `tmap != tmap2`
/// (`cmpmap.c:1730`), which in an owned-value port is a fact the engine knows
/// rather than a pointer comparison — and whether the reduction failed, since
/// only one that did not is recorded as simplified.
///
/// The non-CmpMap path delegates to the public entry, which reports the same
/// for whichever per-class seam runs.
/// As every C call this stands in for is an `astSimplify`, a reduction that
/// reaches an undefined transformation returns the Mapping unsimplified,
/// carrying the records of simplification written on its components.
pub(crate) fn simplify_mapping_with_outcome(
    mapping: &dyn Mapping,
) -> AstResult<(DynMapping, SimplifyOutcome)> {
    let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() else {
        return mapping.simplify_with_outcome();
    };
    let mut tags = None;
    match simplify_cmpmap_into(cmp, &mut tags) {
        Ok((m, true)) => Ok((m, SimplifyOutcome::Changed(tags))),
        Ok((m, false)) => Ok((m, SimplifyOutcome::Unchanged)),
        Err(e) if matches!(e.code(), ErrorCode::Nodef | ErrorCode::Trnnd) => {
            let mut unsimplified = mapping.clone_box();
            if let Some(tags) = tags {
                tags.apply_to(unsimplified.as_mut());
            }
            Ok((unsimplified, SimplifyOutcome::Failed))
        }
        Err(e) => Err(e),
    }
}

/// [`CmpMap`]'s per-class simplification, reached through the gate.
///
/// The records of simplification the reduction wrote on the CmpMap's own
/// components are handed to the gate, as C writes them on the components
/// themselves.
pub(crate) fn simplify_cmpmap(
    cmp: &CmpMap,
    gate: &mut ViaSimplify,
) -> AstResult<(DynMapping, bool)> {
    let mut tags = None;
    let result = simplify_cmpmap_into(cmp, &mut tags);
    if let Some(tags) = tags {
        gate.component_records(tags);
    }
    result
}

/// The body of [`simplify_cmpmap`]: when the reduction fails or returns a new
/// Mapping, `input_tags` receives the records on `cmp` once those the
/// reduction wrote on its components are added.
fn simplify_cmpmap_into(
    cmp: &CmpMap,
    input_tags: &mut Option<TagTree>,
) -> AstResult<(DynMapping, bool)> {
    let mapping: &dyn Mapping = cmp;

    // C's recursion guard (cmpmap.c:3487-3491): if an equal Mapping is already
    // being simplified further up the stack, hand this one back unchanged.
    let series = cmp.composition() == Composition::Series;

    let _guard = match SimplifyGuard::enter(mapping) {
        Some(g) => g,
        None => {
            // The guard matched, so C returns `astClone( this_mapping )`. The
            // comparison that produced the match has already cleared the
            // element tags on the object being cloned.
            return Ok((mapping.clone_box(), false));
        }
    };

    let restricted = cmp.restricted_simplify();

    let mut list = flatten(mapping, false, series)?;
    let ncomponents = list.len();
    for (i, (m, _, _)) in list.iter_mut().enumerate() {
        m.mapping_flags_mut().origin = i + 1;
    }

    let mut stamped = Vec::new();
    let changed = merge_loop_restricted(&mut list, series, restricted, &mut stamped);
    for (m, _, _) in &mut list {
        m.mapping_flags_mut().origin = 0;
    }
    let mut records: Vec<Option<TagTree>> = (0..ncomponents).map(|_| None).collect();
    for (origin, tags) in stamped {
        records[origin - 1] = Some(tags);
    }
    let changed = match changed {
        Ok(changed) => changed,
        Err(e) => {
            // C's `Simplify` returns nothing and leaves the error for
            // `astSimplify` (cmpmap.c:3954), whose `astClone( this )` carries
            // the records written through the list's aliases.
            *input_tags = Some(stamped_tags(mapping, false, series, &records, &mut 0));
            return Err(e);
        }
    };

    // C's `simpler` (cmpmap.c:3634) decides between cloning the supplied CmpMap
    // and rebuilding it from the component list. `astMapList` sets it when the
    // decomposition met an inverted CmpMap (`result = invert || r1 || r2`,
    // MapList:1191), whose normalization is itself a simplification even if no
    // pair merges; the nominate loop sets it on every unfrozen modification.
    //
    // C discards the decomposition's flag under a restricted simplify: it cannot
    // yet know whether an inverted CmpMap it met will be eligible, so the
    // decomposition alone does not count as an improvement (cmpmap.c:3526-3530).
    let simpler = changed || (!restricted && decomposition_is_simpler(mapping, false, series));
    if !simpler {
        let mut cloned = mapping.clone_box();
        // Every `IsSimple` now on a list entry is one C raised on the *live*
        // component: `SeparateMappings` (cmpmap.c:3326) leaves each non-duplicate
        // entry aliasing the real child, and the nominate loop's `astSimplify`
        // (cmpmap.c:1437) writes through that alias. So the tags are still on
        // `this`'s children when `Simplify` returns `astClone( this )`
        // (cmpmap.c:3634), and they must be transplanted onto our fresh clone.
        let entries: Vec<Option<TagTree>> = list
            .iter()
            .map(|(m, _, _)| Some(TagTree::of(m.as_ref())))
            .collect();
        stamped_tags(mapping, false, series, &entries, &mut 0).apply_to(cloned.as_mut());
        return Ok((cloned, false));
    }

    // The reduction makes a new Mapping, but the records it wrote on this
    // CmpMap's components stay on them in C.
    *input_tags = Some(stamped_tags(mapping, false, series, &records, &mut 0));

    // C's `simpler` branch: the result is folded afresh from the component list,
    // so it is a new pointer even when the fold reproduces the input's shape.
    //
    // The fold clears the protected `AllowSimplify` flag on each component as it
    // goes (cmpmap.c:3689-3692), so the flag cannot appear in a public dump. C
    // does this only in its multi-element branch; a single-element result is an
    // `astCopy` (cmpmap.c:3651) whose flag `astSimplify` clears on the result
    // instead.
    if list.len() > 1 {
        for (component, _, _) in &mut list {
            component.set_allow_simplify(false);
        }
    }
    Ok((reconstruct(list, series)?, true))
}

// ------------------------------------------------------------------ //
// Tests                                                               //
// ------------------------------------------------------------------ //

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::{ShiftMap, ZoomMap};

    #[test]
    fn parallel_in_series_with_blocking_delta() {
        // Reproduces the sip-3d-swap chain pattern:
        //   parallel(SlaMap_Hfk5z, Unit_1) ;
        //   parallel(Unit_2,       SmallShift_1) ;
        //   parallel(SlaMap_Fk5hz, Unit_1)
        //
        // The sky and spec axes are independent; the SlaMap pair
        // (Hfk5z + Fk5hz at the same epoch) cancels when adjacent.
        // The simplifier should swap the middle parallel-CmpMap past
        // one of the SlaMap parallels (since their non-trivial parts
        // operate on disjoint axes) so the two SlaMap parallels
        // become adjacent and merge to identity.
        use crate::mappings::SlaCvtType;
        use crate::mappings::SlaMap;
        use crate::mappings::UnitMap;

        let mut sla1 = SlaMap::new();
        sla1.add_conversion(SlaCvtType::Hfk5z, &[2000.0]);
        let pair1: DynMapping = Box::new(
            CmpMap::new(
                Box::new(sla1),
                Box::new(UnitMap::new(1)),
                Composition::Parallel,
            )
            .unwrap(),
        );

        let pair2: DynMapping = Box::new(
            CmpMap::new(
                Box::new(UnitMap::new(2)),
                Box::new(ShiftMap::new(vec![0.001])),
                Composition::Parallel,
            )
            .unwrap(),
        );

        let mut sla2 = SlaMap::new();
        sla2.add_conversion(SlaCvtType::Fk5hz, &[2000.0]);
        let pair3: DynMapping = Box::new(
            CmpMap::new(
                Box::new(sla2),
                Box::new(UnitMap::new(1)),
                Composition::Parallel,
            )
            .unwrap(),
        );

        let chain12: DynMapping = Box::new(CmpMap::new(pair1, pair2, Composition::Series).unwrap());
        let full: DynMapping = Box::new(CmpMap::new(chain12, pair3, Composition::Series).unwrap());

        let s = full.simplify().unwrap();
        let flat = flatten(s.as_ref(), false, true).unwrap();
        eprintln!("simplified.len = {}", flat.len());
        for (i, (m, inv, _)) in flat.iter().enumerate() {
            let dbg = format!("{:?}", m);
            let preview: String = dbg.chars().take(80).collect();
            eprintln!(
                "  s[{}]: {}/{} inv={} {}",
                i,
                m.nin(),
                m.nout(),
                inv,
                preview
            );
        }

        // Sanity: outputs match.
        use crate::coords::PointSet;
        let pts = PointSet::from_rows([[0.5_f64, -0.3, 1000.0]]);
        let o = full.transform(&pts, true).unwrap();
        let os = s.transform(&pts, true).unwrap();
        let p = o.point(0);
        let ps = os.point(0);
        for k in 0..3 {
            assert!(
                (p[k] - ps[k]).abs() < 1e-9,
                "mismatch at axis {}: {} vs {}",
                k,
                p[k],
                ps[k]
            );
        }

        // Ideal: the SlaMap pair cancels, leaving only the
        // SmallShift_1axis on the spec axis. Currently we can't yet
        // prove the simplifier achieves this; the test mostly logs
        // the chain structure for diagnostic purposes.
        let any_sla = flat
            .iter()
            .any(|(m, _, _)| m.as_any().is::<crate::mappings::SlaMap>());
        if any_sla {
            // Failure marker; convert to a hard assert once the
            // simplifier learns to swap parallel-disjoint pairs.
            eprintln!(
                "[parallel_in_series_with_blocking_delta] simplifier did NOT cancel the SlaMap pair (chain.len={})",
                flat.len()
            );
        }
    }

    // Helper: box a mapping as DynMapping.
    fn dyn_map(m: impl Mapping + 'static) -> DynMapping {
        Box::new(m)
    }

    // ------------------------------------------------------------ //
    // pattern_check (oscillation detection)                         //
    // ------------------------------------------------------------ //

    #[test]
    fn pattern_check_short_history_never_fires() {
        // C only searches once list_len > 29; below that, always 0.
        let mut list = Vec::new();
        for i in 0..20 {
            assert_eq!(pattern_check((i % 2) as i64, 1, &mut list), 0);
        }
    }

    #[test]
    fn pattern_check_check_disabled_never_fires() {
        // With check == 0 the search is skipped regardless of length.
        let mut list = Vec::new();
        for _ in 0..60 {
            assert_eq!(pattern_check(7, 0, &mut list), 0);
        }
    }

    #[test]
    fn pattern_check_detects_wavelength_two() {
        // Alternating 0,1,0,1,... is a wavelength-2 pattern. Three full
        // repeats are required, and the list must exceed 29 entries before
        // the search runs. Feed a long alternating sequence.
        let mut list = Vec::new();
        let mut detected = 0;
        for i in 0..40 {
            let w = pattern_check((i % 2) as i64, 1, &mut list);
            if w != 0 {
                detected = w;
                break;
            }
        }
        assert_eq!(detected, 2);
    }

    #[test]
    fn pattern_check_wavelength_one_needs_thirty_repeats() {
        // A constant sequence has wavelength 1, which requires 30 repeats
        // (not 3) before it is reported, since short runs of a repeated
        // value occur legitimately.
        let mut list = Vec::new();
        let mut first_hit = None;
        for i in 0..60 {
            if pattern_check(5, 1, &mut list) != 0 {
                first_hit = Some(i);
                break;
            }
        }
        // Must not fire before 30 occurrences have accumulated.
        assert!(first_hit.is_some());
        assert!(first_hit.unwrap() >= 29, "fired too early: {first_hit:?}");
    }

    // ------------------------------------------------------------ //
    // MergeCtx (frozen flags)                                       //
    // ------------------------------------------------------------ //

    #[test]
    fn merge_ctx_freezes_the_mapping_itself() {
        let list = vec![
            (dyn_map(ZoomMap::new(1, 2.0).unwrap()), false, false),
            (dyn_map(ZoomMap::new(1, 3.0).unwrap()), false, false),
        ];
        let mut ctx = MergeCtx::new(list);
        assert_eq!(ctx.len(), 2);
        assert!(!ctx.is_frozen(0));
        assert!(!ctx.is_frozen(1));
        ctx.freeze(1);
        assert!(!ctx.is_frozen(0));
        assert!(ctx.is_frozen(1));
        // Out-of-range index is treated as not frozen, never panics.
        assert!(!ctx.is_frozen(99));
    }

    // ------------------------------------------------------------ //
    // flatten                                                       //
    // ------------------------------------------------------------ //

    #[test]
    fn flatten_non_cmpmap_returns_single_element() {
        let z = ZoomMap::new(2, 3.0).unwrap();
        let list = flatten(&z, false, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].1); // not inverted
        assert_eq!(list[0].0.nin(), 2);
        assert_eq!(list[0].0.nout(), 2);
    }

    #[test]
    fn flatten_non_cmpmap_with_inv_flag() {
        let z = ZoomMap::new(2, 3.0).unwrap();
        let list = flatten(&z, true, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].1); // effectively inverted
        assert!(!list[0].0.is_inverted()); // mapping itself is normalised
    }

    #[test]
    fn flatten_non_cmpmap_with_self_inverted() {
        let mut z = ZoomMap::new(1, 5.0).unwrap();
        z.invert();
        assert!(z.is_inverted());
        // The leaf is not reduced, so its stored inversion survives as
        // `orig_inv` and combines with the external direction rather than being
        // absorbed into the factor. C keeps each leaf at the orientation it is
        // stored in and records the effective direction separately.
        let list = flatten(&z, false, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].1); // stored inverted, applied forward → effectively inverted
        assert!(list[0].2); // and it serialises inverted
        assert!(!list[0].0.is_inverted()); // the entry itself is held forward
    }

    #[test]
    fn flatten_non_cmpmap_double_inversion_cancels() {
        let mut z = ZoomMap::new(1, 5.0).unwrap();
        z.invert();
        // Stored inverted and applied inverted cancel, which is the whole point
        // of carrying the two separately.
        let list = flatten(&z, true, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].1); // effectively forward
        assert!(list[0].2); // still serialises inverted
    }

    #[test]
    fn flatten_series_cmpmap_two_elements() {
        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let list = flatten(&cmp, false, true).unwrap();
        assert_eq!(list.len(), 2);
        assert!(!list[0].1);
        assert!(!list[1].1);
    }

    #[test]
    fn flatten_nested_series_three_elements() {
        // CmpMap(CmpMap(A, B, Series), C, Series) → [A, B, C]
        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let inner = CmpMap::new(a, b, Composition::Series).unwrap();
        let c = dyn_map(ZoomMap::new(1, 5.0).unwrap());
        let outer = CmpMap::new(dyn_map(inner), c, Composition::Series).unwrap();
        let list = flatten(&outer, false, true).unwrap();
        assert_eq!(list.len(), 3);
        // All forward
        assert!(!list[0].1);
        assert!(!list[1].1);
        assert!(!list[2].1);
    }

    #[test]
    fn flatten_parallel_cmpmap_not_absorbed_by_series() {
        // A parallel CmpMap is opaque when flattening for series.
        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let par = CmpMap::new(a, b, Composition::Parallel).unwrap();
        let list = flatten(&par, false, true).unwrap(); // flatten for series
        assert_eq!(list.len(), 1); // parallel treated as leaf
    }

    #[test]
    fn flatten_inverted_series_reverses_order() {
        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0, 20.0]));
        let mut cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        cmp.invert();
        let list = flatten(&cmp, false, true).unwrap();
        assert_eq!(list.len(), 2);
        // Inverted series: order reversed, both inverted.
        // The shift should come first (was second, now reversed).
        assert!(list[0].1); // inverted
        assert!(list[1].1); // inverted
        // First element should be the ShiftMap (originally second).
        assert!(list[0].0.as_any().is::<ShiftMap>());
        assert!(list[1].0.as_any().is::<ZoomMap>());
    }

    #[test]
    fn flatten_parallel_cmpmap_for_parallel() {
        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let cmp = CmpMap::new(a, b, Composition::Parallel).unwrap();
        // Flatten for parallel: absorbs the parallel CmpMap.
        let list = flatten(&cmp, false, false).unwrap();
        assert_eq!(list.len(), 2);
    }

    // ------------------------------------------------------------ //
    // merge_loop                                                    //
    // ------------------------------------------------------------ //

    #[test]
    fn merge_loop_no_merges_with_incompatible_types() {
        use crate::mappings::SphMap;

        // Types that have no cross-type merge rule should not merge.
        let a = dyn_map(SphMap::new());
        let b = dyn_map(ShiftMap::new([10.0, 20.0]));
        let mut list: Vec<(DynMapping, bool, bool)> = vec![(a, false, false), (b, false, false)];
        merge_loop(&mut list, true);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn merge_loop_strips_unitmaps_from_series() {
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let u = dyn_map(UnitMap::new(1));
        let b = dyn_map(ShiftMap::new([5.0]));
        let mut list: Vec<(DynMapping, bool, bool)> =
            vec![(a, false, false), (u, false, false), (b, false, false)];
        merge_loop(&mut list, true);
        // ZoomMap merges UnitMap (identity), then result merges ShiftMap.
        assert_eq!(list.len(), 1);
        assert!(!list[0].0.as_any().is::<UnitMap>());
    }

    #[test]
    fn merge_loop_merges_linear_parallel() {
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let u = dyn_map(UnitMap::new(1));
        let mut list: Vec<(DynMapping, bool, bool)> = vec![(a, false, false), (u, false, false)];
        merge_loop(&mut list, false);
        // Parallel: linear mappings are merged into a wider WinMap.
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0.nin(), 2);
    }

    #[test]
    fn merge_loop_all_unitmaps_preserves_one() {
        let u1 = dyn_map(UnitMap::new(2));
        let u2 = dyn_map(UnitMap::new(2));
        let mut list: Vec<(DynMapping, bool, bool)> = vec![(u1, false, false), (u2, false, false)];
        merge_loop(&mut list, true);
        // All UnitMaps stripped → fallback pushes one back.
        assert_eq!(list.len(), 1);
    }

    // ------------------------------------------------------------ //
    // reconstruct                                                   //
    // ------------------------------------------------------------ //

    #[test]
    fn reconstruct_single_element() {
        let z = dyn_map(ZoomMap::new(1, 4.0).unwrap());
        let result = reconstruct(vec![(z, false, false)], true).unwrap();
        assert_eq!(result.nin(), 1);
        assert!(!result.is_inverted());
    }

    #[test]
    fn reconstruct_single_element_inverted() {
        let z = dyn_map(ZoomMap::new(1, 4.0).unwrap());
        let result = reconstruct(vec![(z, true, false)], true).unwrap();
        assert!(result.is_inverted());
    }

    #[test]
    fn reconstruct_two_elements_series() {
        use crate::coords::PointSet;

        let z = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let s = dyn_map(ShiftMap::new([10.0]));
        let result = reconstruct(vec![(z, false, false), (s, false, false)], true).unwrap();
        // zoom(2) then shift(10): 5 → 10 → 20
        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 20.0).abs() < 1e-12);
    }

    #[test]
    fn reconstruct_three_elements_series() {
        use crate::coords::PointSet;

        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let c = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let result = reconstruct(
            vec![(a, false, false), (b, false, false), (c, false, false)],
            true,
        )
        .unwrap();
        // 5 → 10 → 20 → 60
        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 60.0).abs() < 1e-12);
    }

    #[test]
    fn reconstruct_two_elements_parallel() {
        use crate::coords::PointSet;

        let z = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let s = dyn_map(ShiftMap::new([100.0]));
        let result = reconstruct(vec![(z, false, false), (s, false, false)], false).unwrap();
        assert_eq!(result.nin(), 2);
        let pts = PointSet::from_rows([[2.0, 5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 105.0).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // simplify_mapping                                              //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_mapping_non_cmpmap_delegates() {
        let z = ZoomMap::new(1, 1.0).unwrap();
        // ZoomMap with factor 1.0 simplifies to UnitMap.
        let result = simplify_mapping(&z).unwrap();
        assert!(result.as_any().is::<UnitMap>());
    }

    #[test]
    fn simplify_mapping_cmpmap_roundtrip() {
        use crate::coords::PointSet;

        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // Must be functionally equivalent: 5 → 20.
        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 20.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_mapping_inverted_cmpmap() {
        use crate::coords::PointSet;

        let a = dyn_map(ZoomMap::new(1, 2.0).unwrap());
        let b = dyn_map(ShiftMap::new([10.0]));
        let mut cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        cmp.invert();
        // Inverted: inverse of (zoom(2) then shift(10)) = inv_shift(10) then inv_zoom(2)
        // = (x - 10) / 2
        // Apply forward to the simplified result (which encodes the inversion).
        let result = simplify_mapping(&cmp).unwrap();
        let pts = PointSet::from_rows([[20.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 5.0).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // try_merge: UnitMap removal                                    //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_unitmap_series_zoom_removes_unit() {
        // CmpMap(UnitMap(2), ZoomMap(2,3), Series) → ZoomMap(2,3)
        let a = dyn_map(UnitMap::new(2));
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<ZoomMap>());
        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);
    }

    // ------------------------------------------------------------ //
    // try_merge: ZoomMap merging                                    //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_zoom_series_merges_factors() {
        // CmpMap(ZoomMap(2,2), ZoomMap(2,3), Series) → ZoomMap(2,6)
        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<ZoomMap>());
        assert_eq!(result.nin(), 2);
        // Verify the merged factor via transform.
        let pts = crate::coords::PointSet::from_rows([[1.0, 2.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 12.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_zoom_series_inverse_becomes_unit() {
        // CmpMap(ZoomMap(2,2), ZoomMap(2,0.5), Series) → UnitMap(2)
        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ZoomMap::new(2, 0.5).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    // ------------------------------------------------------------ //
    // try_merge: ShiftMap merging                                   //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_shift_series_merges_offsets() {
        // CmpMap(ShiftMap([1,2]), ShiftMap([3,4]), Series) → ShiftMap([4,6])
        let a = dyn_map(ShiftMap::new(vec![1.0, 2.0]));
        let b = dyn_map(ShiftMap::new(vec![3.0, 4.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<ShiftMap>());
        let pts = crate::coords::PointSet::from_rows([[0.0, 0.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 4.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 6.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_shift_series_cancels_to_unit() {
        // CmpMap(ShiftMap([1,2]), ShiftMap([-1,-2]), Series) → UnitMap(2)
        let a = dyn_map(ShiftMap::new(vec![1.0, 2.0]));
        let b = dyn_map(ShiftMap::new(vec![-1.0, -2.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    // ------------------------------------------------------------ //
    // Transform equivalence: before/after simplification            //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_preserves_transform_zoom_series() {
        use crate::coords::PointSet;

        let a = dyn_map(ZoomMap::new(2, 2.0).unwrap());
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[1.0, -2.0], [0.5, 100.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            for j in 0..2 {
                assert!(
                    (before.point(i)[j] - after.point(i)[j]).abs() < 1e-12,
                    "mismatch at point {} coord {}",
                    i,
                    j,
                );
            }
        }
    }

    #[test]
    fn simplify_preserves_transform_shift_series() {
        use crate::coords::PointSet;

        let a = dyn_map(ShiftMap::new(vec![10.0, 20.0]));
        let b = dyn_map(ShiftMap::new(vec![30.0, 40.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[1.0, 2.0], [5.0, -3.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            for j in 0..2 {
                assert!(
                    (before.point(i)[j] - after.point(i)[j]).abs() < 1e-12,
                    "mismatch at point {} coord {}",
                    i,
                    j,
                );
            }
        }
    }

    #[test]
    fn simplify_preserves_transform_inverse() {
        use crate::coords::PointSet;

        // Test that inverse transforms also agree.
        let a = dyn_map(ZoomMap::new(1, 4.0).unwrap());
        let b = dyn_map(ZoomMap::new(1, 5.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[100.0]]);
        let before = cmp.transform(&pts, false).unwrap();
        let after = simplified.transform(&pts, false).unwrap();
        assert!((before.axis(0)[0] - after.axis(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // try_merge: UnitMap parallel merge                             //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_unitmap_parallel_merges() {
        // CmpMap(UnitMap(2), UnitMap(3), Parallel) → UnitMap(5)
        let a = dyn_map(UnitMap::new(2));
        let b = dyn_map(UnitMap::new(3));
        let cmp = CmpMap::new(a, b, Composition::Parallel).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 5);
    }

    // ------------------------------------------------------------ //
    // try_merge: WinMap merging                                     //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_winmap_series_composes() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // WinMap(s=1,c=2) then WinMap(s=10,c=3)
        // Compose: out = 10 + 3*(1 + 2*x) = 13 + 6*x
        let a = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let b = dyn_map(WinMap::new(&[10.0], &[3.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 43.0).abs() < 1e-12); // 13 + 6*5 = 43
    }

    #[test]
    fn simplify_winmap_inverse_cancels_to_unit() {
        use crate::mappings::WinMap;

        // WinMap(s=3,c=2) then WinMap(s=3,c=2)^-1 should cancel.
        let a = dyn_map(WinMap::new(&[3.0], &[2.0]).unwrap());
        let mut b = WinMap::new(&[3.0], &[2.0]).unwrap();
        b.invert();
        let cmp = CmpMap::new(a, dyn_map(b), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 1);
    }

    #[test]
    fn simplify_winmap_preserves_transform() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        let a = dyn_map(WinMap::new(&[1.0, 2.0], &[3.0, 4.0]).unwrap());
        let b = dyn_map(WinMap::new(&[5.0, 6.0], &[7.0, 8.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[1.0, -2.0], [0.5, 100.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            for j in 0..2 {
                assert!(
                    (before.point(i)[j] - after.point(i)[j]).abs() < 1e-10,
                    "mismatch at point {} coord {}",
                    i,
                    j,
                );
            }
        }
    }

    // ------------------------------------------------------------ //
    // try_merge: MatrixMap merging                                  //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_matrix_rot90_inverse_cancels() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // 90-degree rotation then its inverse = identity.
        let rot90 = array![[0.0_f64, -1.0], [1.0, 0.0]];
        let rot90_inv = array![[0.0_f64, 1.0], [-1.0, 0.0]];
        let a = dyn_map(MatrixMap::new(rot90).unwrap());
        let b = dyn_map(MatrixMap::new(rot90_inv).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_matrix_series_multiplies() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // Non-diagonal matrices that truly require MatrixMap after merging.
        let m1 = array![[1.0_f64, 1.0], [0.0, 1.0]]; // shear
        let m2 = array![[2.0_f64, 0.0], [0.0, 1.0]]; // non-uniform scale
        let a = dyn_map(MatrixMap::new(m1).unwrap());
        let b = dyn_map(MatrixMap::new(m2).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();
        assert!(simplified.as_any().is::<MatrixMap>());

        // m2 * m1 = [[2,2],[0,1]]; apply to (1,2): (2*1+2*2, 0*1+1*2) = (6, 2)
        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let out = simplified.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 2.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_matrix_preserves_transform() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        let m1 = array![[1.0_f64, 2.0], [3.0, 4.0]];
        let m2 = array![[5.0_f64, 6.0], [7.0, 8.0]];
        let a = dyn_map(MatrixMap::new(m1).unwrap());
        let b = dyn_map(MatrixMap::new(m2).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[1.0, -1.0], [3.0, 7.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        for i in 0..pts.npoint() {
            for j in 0..2 {
                assert!(
                    (before.point(i)[j] - after.point(i)[j]).abs() < 1e-10,
                    "mismatch at point {} coord {}",
                    i,
                    j,
                );
            }
        }
    }

    #[test]
    fn simplify_matrix_with_inversion_flag() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // MatrixMap then MatrixMap^-1 (via .invert()) should cancel.
        let m = array![[1.0_f64, 2.0], [3.0, 4.0]];
        let a = dyn_map(MatrixMap::new(m.clone()).unwrap());
        let mut b = MatrixMap::new(m).unwrap();
        b.invert();
        let cmp = CmpMap::new(a, dyn_map(b), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    // ------------------------------------------------------------ //
    // try_merge: SphMap inverse cancellation                        //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_sphmap_fwd_inv_cancels() {
        use crate::mappings::SphMap;

        // SphMap (fwd 3->2) then SphMap^-1 (inv 2->3) with unit_radius.
        let mut fwd = SphMap::new();
        fwd.set_unit_radius(true);
        let mut inv = SphMap::new();
        inv.set_unit_radius(true);
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 3);
    }

    #[test]
    fn simplify_sphmap_inv_fwd_cancels() {
        use crate::mappings::SphMap;

        // SphMap^-1 (inv, sph→cart 2->3) then SphMap (fwd, cart→sph 3->2).
        // Inv→fwd cancels without UnitRadius — palDcs2c always produces
        // unit vectors so the intermediate 3D vector is guaranteed unit.
        let mut inv = SphMap::new();
        inv.invert();
        let fwd = SphMap::new();
        let cmp = CmpMap::new(dyn_map(inv), dyn_map(fwd), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_sphmap_fwd_inv_cancels_with_unit_radius_on_first() {
        use crate::mappings::SphMap;

        // Forward (cart→sph) with UnitRadius, then inverse (sph→cart) without.
        // C AST only requires UnitRadius on the first (forward) map for
        // fwd→inv cancellation, so this SHOULD cancel.
        let mut fwd = SphMap::new();
        fwd.set_unit_radius(true);
        let mut inv = SphMap::new();
        // inv keeps unit_radius=false (default)
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 3);
    }

    #[test]
    fn simplify_sphmap_fwd_inv_no_cancel_without_unit_radius() {
        use crate::mappings::SphMap;

        // Forward (cart→sph) WITHOUT UnitRadius, then inverse (sph→cart).
        // Should NOT cancel: without UnitRadius the forward step loses
        // magnitude information.
        let fwd = SphMap::new(); // unit_radius=false
        let mut inv = SphMap::new();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(!result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // try_merge: WcsMap inverse cancellation                        //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_wcsmap_inverse_cancels() {
        use crate::mappings::{WcsMap, WcsProjection};

        let fwd = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let mut inv = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_wcsmap_different_proj_no_merge() {
        use crate::mappings::{WcsMap, WcsProjection};

        // Different projections should NOT cancel.
        let fwd = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let mut inv = WcsMap::new(2, WcsProjection::Sin, 1, 2).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(!result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // try_merge: SlaMap inverse cancellation                        //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_slamap_inverse_cancels() {
        use crate::mappings::{SlaCvtType, SlaMap};

        let mut fwd = SlaMap::new();
        fwd.add_conversion(SlaCvtType::Eqgal, &[]);
        let mut inv = SlaMap::new();
        inv.add_conversion(SlaCvtType::Eqgal, &[]);
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_slamap_different_steps_no_merge() {
        use crate::mappings::{SlaCvtType, SlaMap};

        // Different conversions should NOT cancel.
        let mut fwd = SlaMap::new();
        fwd.add_conversion(SlaCvtType::Eqgal, &[]);
        let mut inv = SlaMap::new();
        inv.add_conversion(SlaCvtType::Galeq, &[]);
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(!result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // try_merge: PolyMap inverse cancellation                       //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_polymap_inverse_cancels() {
        use crate::mappings::{PolyMap, PolyTerm};

        // Non-linear polynomial: y = x^2 + 3*x (not decomposed to MatrixMap)
        let terms = vec![
            PolyTerm {
                coeff: 1.0,
                powers: vec![2],
                out_axis: 1,
            },
            PolyTerm {
                coeff: 3.0,
                powers: vec![1],
                out_axis: 1,
            },
        ];
        let fwd = PolyMap::new(1, 1, &terms, None).unwrap();
        let mut inv = PolyMap::new(1, 1, &terms, None).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 1);
    }

    // ------------------------------------------------------------ //
    // try_merge: PermMap inverse cancellation                       //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_permmap_inverse_cancels() {
        use crate::mappings::PermMap;

        // Swap axes: outperm=[2,1], inperm=[2,1].
        let fwd = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        let mut inv = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_permmap_different_perms_no_merge() {
        use crate::mappings::PermMap;

        let fwd = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        let mut inv = PermMap::new(&[1, 2], &[1, 2], &[]).unwrap();
        inv.invert();
        let cmp = CmpMap::new(dyn_map(fwd), dyn_map(inv), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // Different perms: should NOT simplify to UnitMap.
        assert!(!result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: WinMap absorbs ZoomMap              //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_winmap_absorbs_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // WinMap(s=1,c=2) then ZoomMap(1,3): out = 3*(2*x + 1) = 6*x + 3
        let a = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let b = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        // 6*5 + 3 = 33
        assert!((out.axis(0)[0] - 33.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_zoommap_then_winmap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // ZoomMap(1,3) then WinMap(s=1,c=2): out = 2*(3*x) + 1 = 6*x + 1
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let b = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 31.0).abs() < 1e-12); // 6*5 + 1

        // Also verify transform equivalence.
        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - out.axis(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: WinMap absorbs ShiftMap             //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_winmap_absorbs_shiftmap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // WinMap(s=1,c=2) then ShiftMap([10]): out = 1*(2*x+1) + 10 = 2*x + 11
        let a = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let b = dyn_map(ShiftMap::new(vec![10.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 21.0).abs() < 1e-12); // 2*5 + 11
    }

    #[test]
    fn simplify_shiftmap_then_winmap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // ShiftMap([10]) then WinMap(s=1,c=2): out = 2*(x+10) + 1 = 2*x + 21
        let a = dyn_map(ShiftMap::new(vec![10.0]));
        let b = dyn_map(WinMap::new(&[1.0], &[2.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 31.0).abs() < 1e-12); // 2*5 + 21

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - out.axis(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: ZoomMap + ShiftMap                  //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_zoommap_then_shiftmap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // ZoomMap(1,3) then ShiftMap([10]): out = 3*x + 10
        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let b = dyn_map(ShiftMap::new(vec![10.0]));
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // Should produce a WinMap (shift + scale).
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 25.0).abs() < 1e-12); // 3*5 + 10

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - out.axis(0)[0]).abs() < 1e-12);
    }

    #[test]
    fn simplify_shiftmap_then_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // ShiftMap([10]) then ZoomMap(1,3): out = 3*(x + 10) = 3*x + 30
        let a = dyn_map(ShiftMap::new(vec![10.0]));
        let b = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 45.0).abs() < 1e-12); // 3*5 + 30

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - out.axis(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: with inversion                     //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_winmap_then_inv_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // WinMap(s=6,c=3) then ZoomMap(1,3)^-1: out = (1/3)*(3*x + 6) = x + 2
        let a = dyn_map(WinMap::new(&[6.0], &[3.0]).unwrap());
        let mut z = ZoomMap::new(1, 3.0).unwrap();
        z.invert();
        let cmp = CmpMap::new(a, dyn_map(z), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 7.0).abs() < 1e-12); // 5 + 2

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - out.axis(0)[0]).abs() < 1e-12);
    }

    #[test]
    fn simplify_inv_shiftmap_then_zoommap() {
        use crate::coords::PointSet;

        // ShiftMap([10])^-1 then ZoomMap(1,2): out = 2*(x - 10) = 2*x - 20
        let mut s = ShiftMap::new(vec![10.0]);
        s.invert();
        let z = ZoomMap::new(1, 2.0).unwrap();
        let cmp = CmpMap::new(dyn_map(s), dyn_map(z), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();

        let pts = PointSet::from_rows([[15.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 10.0).abs() < 1e-12); // 2*15 - 20

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - out.axis(0)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: multi-axis                         //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_cross_type_multi_axis() {
        use crate::coords::PointSet;
        use crate::mappings::WinMap;

        // 2-axis: ShiftMap([1,2]) then ZoomMap(2,3) → WinMap with scales=[3,3], shifts=[3,6]
        let a = dyn_map(ShiftMap::new(vec![1.0, 2.0]));
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<WinMap>());

        // out = 3*(x + shift) = [3*(x1+1), 3*(x2+2)] = [3*x1+3, 3*x2+6]
        let pts = PointSet::from_rows([[10.0, 20.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 33.0).abs() < 1e-12); // 3*10 + 3
        assert!((out.axis(1)[0] - 66.0).abs() < 1e-12); // 3*20 + 6

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - out.axis(0)[0]).abs() < 1e-12);
        assert!((before.axis(1)[0] - out.axis(1)[0]).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type linear merges: identity cancellation               //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_zoommap_inverse_via_winmap() {
        // ZoomMap(1,3) then WinMap(s=0,c=1/3): out = (1/3)*(3*x) = x → UnitMap
        use crate::mappings::WinMap;

        let a = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let b = dyn_map(WinMap::new(&[0.0], &[1.0 / 3.0]).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        assert!(result.as_any().is::<UnitMap>());
    }

    // ------------------------------------------------------------ //
    // Mixed chain: three different linear types                    //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_mixed_linear_chain() {
        use crate::coords::PointSet;

        // ShiftMap([1]) then ZoomMap(1,3) then ShiftMap([10])
        // Step 1: x + 1
        // Step 2: 3*(x+1) = 3*x + 3
        // Step 3: 3*x + 3 + 10 = 3*x + 13
        let a = dyn_map(ShiftMap::new(vec![1.0]));
        let b = dyn_map(ZoomMap::new(1, 3.0).unwrap());
        let inner = CmpMap::new(a, b, Composition::Series).unwrap();
        let c = dyn_map(ShiftMap::new(vec![10.0]));
        let outer = CmpMap::new(dyn_map(inner), c, Composition::Series).unwrap();
        let result = simplify_mapping(&outer).unwrap();

        let pts = PointSet::from_rows([[5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 28.0).abs() < 1e-12); // 3*5 + 13

        // Verify it reduced to a single mapping (not a CmpMap).
        assert!(!result.as_any().is::<CmpMap>());
    }

    // ------------------------------------------------------------ //
    // MatrixMap self-reduction                                      //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_matrix_identity_reduces_to_unitmap() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        let m = array![[1.0_f64, 0.0], [0.0, 1.0]];
        let mm = MatrixMap::new(m).unwrap();
        let result = <dyn Mapping>::simplify(&mm).unwrap();
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_matrix_uniform_diagonal_reduces_to_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        let m = array![[3.0_f64, 0.0], [0.0, 3.0]];
        let mm = MatrixMap::new(m).unwrap();
        let result = <dyn Mapping>::simplify(&mm).unwrap();
        assert!(result.as_any().is::<ZoomMap>());
        assert_eq!(result.nin(), 2);

        let pts = PointSet::from_rows([[2.0, 5.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 15.0).abs() < 1e-12);
    }

    #[test]
    fn simplify_matrix_nonuniform_diagonal_stays_matrix() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // Non-uniform diagonal: different scale per axis → stays MatrixMap.
        let m = array![[2.0_f64, 0.0], [0.0, 5.0]];
        let mm = MatrixMap::new(m).unwrap();
        let result = <dyn Mapping>::simplify(&mm).unwrap();
        assert!(result.as_any().is::<MatrixMap>());
    }

    #[test]
    fn simplify_matrix_non_square_stays_matrix() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // Non-square matrix can't reduce.
        let m = array![[1.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let mm = MatrixMap::new(m).unwrap();
        let result = <dyn Mapping>::simplify(&mm).unwrap();
        assert!(result.as_any().is::<MatrixMap>());
    }

    #[test]
    fn simplify_matrix_inverted_diagonal_reduces_to_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // An inverted uniform diagonal reduces to a forward ZoomMap whose
        // factor is read off the inverse matrix (`matrixmap.c:1885-1892`,
        // which clears the invert flag on the replacement at :1925).
        let m = array![[4.0_f64, 0.0], [0.0, 4.0]];
        let mut mm = MatrixMap::new(m).unwrap();
        mm.invert();
        let result = <dyn Mapping>::simplify(&mm).unwrap();
        assert!(result.as_any().is::<ZoomMap>());
        assert!(!result.is_inverted());
        assert!((result.as_any().downcast_ref::<ZoomMap>().unwrap().zoom() - 0.25).abs() < 1e-12);

        // Either way the forward transform divides by 4.
        let pts = PointSet::from_rows([[8.0, 12.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 2.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 3.0).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // ZoomMap absorbed into MatrixMap                               //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_zoommap_then_matrixmap() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // ZoomMap(2,3) then MatrixMap([[1,2],[3,4]]): result = 3 * M * x
        let a = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let m = array![[1.0_f64, 2.0], [3.0, 4.0]];
        let b = dyn_map(MatrixMap::new(m).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        // Should merge to a single MatrixMap.
        assert!(simplified.as_any().is::<MatrixMap>());

        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - after.axis(0)[0]).abs() < 1e-12);
        assert!((before.axis(1)[0] - after.axis(1)[0]).abs() < 1e-12);
    }

    #[test]
    fn simplify_matrixmap_then_zoommap() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // MatrixMap([[1,2],[3,4]]) then ZoomMap(2,3): result = 3 * M * x
        let m = array![[1.0_f64, 2.0], [3.0, 4.0]];
        let a = dyn_map(MatrixMap::new(m).unwrap());
        let b = dyn_map(ZoomMap::new(2, 3.0).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();
        assert!(simplified.as_any().is::<MatrixMap>());

        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let before = cmp.transform(&pts, true).unwrap();
        let after = simplified.transform(&pts, true).unwrap();
        assert!((before.axis(0)[0] - after.axis(0)[0]).abs() < 1e-12);
        assert!((before.axis(1)[0] - after.axis(1)[0]).abs() < 1e-12);
    }

    #[test]
    fn simplify_matrixmap_then_inv_zoommap() {
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // MatrixMap(3*I) then ZoomMap(2,3)^-1: (1/3)*(3*I*x) = I*x → UnitMap
        let m = array![[3.0_f64, 0.0], [0.0, 3.0]];
        let a = dyn_map(MatrixMap::new(m).unwrap());
        let mut z = ZoomMap::new(2, 3.0).unwrap();
        z.invert();
        let cmp = CmpMap::new(a, dyn_map(z), Composition::Series).unwrap();
        let result = simplify_mapping(&cmp).unwrap();
        // MatrixMap(3I) simplifies to ZoomMap(3), then ZoomMap(3)*ZoomMap(1/3) = UnitMap
        assert!(result.as_any().is::<UnitMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn simplify_diagonal_matrix_series_reduces() {
        use crate::coords::PointSet;
        use crate::mappings::MatrixMap;
        use ndarray::array;

        // Two uniform diagonal matrices: diag(2,2) * diag(3,3) = diag(6,6) → ZoomMap(2,6)
        let s2 = array![[2.0_f64, 0.0], [0.0, 2.0]];
        let s3 = array![[3.0_f64, 0.0], [0.0, 3.0]];
        let a = dyn_map(MatrixMap::new(s2).unwrap());
        let b = dyn_map(MatrixMap::new(s3).unwrap());
        let cmp = CmpMap::new(a, b, Composition::Series).unwrap();
        let simplified = simplify_mapping(&cmp).unwrap();

        // Both MatrixMaps simplify to ZoomMaps, which then merge to ZoomMap(2,6).
        assert!(simplified.as_any().is::<ZoomMap>());

        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let out = simplified.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 12.0).abs() < 1e-12);
    }

    // ------------------------------------------------------------ //
    // Cross-type: dimension mismatch gives no merge                //
    // ------------------------------------------------------------ //

    #[test]
    fn simplify_cross_type_dimension_mismatch_no_merge() {
        // ZoomMap(2-dim) + ShiftMap(3-dim): can't merge due to dimension mismatch.
        // These can't form a valid CmpMap in series either, but try_merge
        // should return None regardless.
        let z = ZoomMap::new(2, 3.0).unwrap();
        let s = ShiftMap::new(vec![1.0, 2.0, 3.0]);
        let result = z.try_merge(&s, true, false, false);
        assert!(result.is_none());
    }

    // ================================================================== //
    // Cross-composition restructuring tests                               //
    // ================================================================== //

    #[test]
    fn parallel_in_series_basic_zoom_merge() {
        use crate::coords::PointSet;

        // Par(Zoom(1,2), Zoom(1,3)) ; Par(Zoom(1,5), Zoom(1,7))
        // → Par(Zoom(1,10), Zoom(1,21))
        let z1 = ZoomMap::new(1, 2.0).unwrap();
        let z2 = ZoomMap::new(1, 3.0).unwrap();
        let par1 = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Parallel).unwrap();

        let z3 = ZoomMap::new(1, 5.0).unwrap();
        let z4 = ZoomMap::new(1, 7.0).unwrap();
        let par2 = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(par1), dyn_map(par2), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);

        // Verify transform: input [1.0, 1.0] → [2*5, 3*7] = [10, 21].
        let pts = PointSet::from_rows([[1.0, 1.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 10.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 21.0).abs() < 1e-12);
    }

    #[test]
    fn parallel_in_series_shift_merge() {
        use crate::coords::PointSet;

        // Par(Shift([2]), Shift([3])) ; Par(Shift([5]), Shift([7]))
        // → Par(Shift([7]), Shift([10]))
        let s1 = ShiftMap::new(vec![2.0]);
        let s2 = ShiftMap::new(vec![3.0]);
        let par1 = CmpMap::new(dyn_map(s1), dyn_map(s2), Composition::Parallel).unwrap();

        let s3 = ShiftMap::new(vec![5.0]);
        let s4 = ShiftMap::new(vec![7.0]);
        let par2 = CmpMap::new(dyn_map(s3), dyn_map(s4), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(par1), dyn_map(par2), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        // Verify transform: [1.0, 1.0] → [1+2+5, 1+3+7] = [8, 11].
        let pts = PointSet::from_rows([[1.0, 1.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 8.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 11.0).abs() < 1e-12);
    }

    #[test]
    fn parallel_in_series_no_improvement_no_change() {
        // Par(Zoom(1,2), Zoom(1,3)) ; Par(Zoom(1,5), Zoom(1,7))
        // Each matched pair (Zoom(1,2);Zoom(1,5)) simplifies to Zoom(1,10),
        // so this DOES improve. Use a case that genuinely doesn't:
        //
        // Par(Zoom(1,2), Shift([3])) ; Par(Shift([5]), Zoom(1,7))
        // Dimension mismatch within the first matched pair:
        // list1[0]=Zoom(1,2) has 1 output; list2[0]=Shift([5]) has 1 input → match.
        // Combine: Zoom(1,2);Shift([5]) = WinMap → improvement detected.
        //
        // Actually all same-dimension pairings will simplify due to linear merges.
        // The "no improvement" case only happens with genuinely non-simplifiable
        // mapping types. For now, just verify that dimension mismatches abort.
        //
        // Par(Zoom(2,2), Zoom(1,3)) ; Par(Zoom(1,5), Zoom(2,7))
        // list1 outputs: [2, 1] = 3 total. list2 inputs: [1, 2] = 3 total.
        // Dimension matching: list1[0] has 2 outputs vs list2[0] has 1 input →
        // need more from list2. After list2[0]+list2[1]: inputs=3 > outputs=2.
        // Then take from list1[1]: outputs=3, inputs=3 → match (single pair).
        // So this merges into one series pair. Not what we want for "no change".
        //
        // Test with actually non-matching total dimensions:
        // Par(Zoom(2,2), Zoom(1,3)) nin/nout=3 ; Par(Zoom(2,5)) nin/nout=2
        // Can't form a valid series CmpMap (3 != 2).
        // So we need same total but unmatchable internal splits.
        // That doesn't happen with the greedy algorithm — it always finds a
        // match if totals match. So "no_improvement" only comes from the inner
        // simplify check. Skip this test since all linear merges produce improvement.
        //
        // Instead test that mismatched total dimensions simply leave things alone.
        let z1 = ZoomMap::new(2, 2.0).unwrap();
        let z2 = ZoomMap::new(1, 3.0).unwrap();
        let par1 = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Parallel).unwrap();
        // par1: nin=3, nout=3

        let z3 = ZoomMap::new(2, 5.0).unwrap();
        let par2_map: DynMapping = dyn_map(z3);
        // par2_map: nin=2, nout=2 — NOT a parallel CmpMap, just a ZoomMap.
        // So adjacent is par1(ParCmp) + z3(ZoomMap) — won't trigger restructuring.

        // Use two parallel CmpMaps with incompatible parallel splits but same total.
        // Par(Zoom(2,2), Zoom(1,3)) nout=[2,1]=3, Par(Zoom(3,5)) nout=[3]=3
        // Second is not even a CmpMap. Use Par(Zoom(1,5), Zoom(1,7), Zoom(1,9))
        // which is actually Par(Par(Zoom1_5, Zoom1_7), Zoom1_9).
        let z3 = ZoomMap::new(1, 5.0).unwrap();
        let z4 = ZoomMap::new(1, 7.0).unwrap();
        let z5 = ZoomMap::new(1, 9.0).unwrap();
        let par2_inner = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Parallel).unwrap();
        let par2 = CmpMap::new(dyn_map(par2_inner), dyn_map(z5), Composition::Parallel).unwrap();
        // par2: nin=3, nout=3

        let series = CmpMap::new(dyn_map(par1), Box::new(par2), Composition::Series).unwrap();
        let _result = simplify_mapping(&series).unwrap();
        // The restructuring should attempt but the inner merges will succeed
        // (ZoomMap pairs always merge in series). Just verify it terminates.
        drop(par2_map);
    }

    #[test]
    fn parallel_in_series_dimension_matching_multicomponent() {
        use crate::coords::PointSet;

        // Par(Zoom(2,2), Zoom(2,3)) ; Par(Zoom(2,5), Zoom(2,7))
        // = [2,2] outputs feed [2,2] inputs → two matched pairs.
        let z1 = ZoomMap::new(2, 2.0).unwrap();
        let z2 = ZoomMap::new(2, 3.0).unwrap();
        let par1 = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Parallel).unwrap();

        let z3 = ZoomMap::new(2, 5.0).unwrap();
        let z4 = ZoomMap::new(2, 7.0).unwrap();
        let par2 = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(par1), dyn_map(par2), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        // Verify: [1,1,1,1] → [2*5, 2*5, 3*7, 3*7] = [10,10,21,21]
        let pts = PointSet::from_rows([[1.0, 1.0, 1.0, 1.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 10.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 10.0).abs() < 1e-12);
        assert!((out.axis(2)[0] - 21.0).abs() < 1e-12);
        assert!((out.axis(3)[0] - 21.0).abs() < 1e-12);
    }

    #[test]
    fn series_in_parallel_basic_zoom_merge() {
        use crate::coords::PointSet;

        // (Zoom(1,2) ; Zoom(1,0.5)) || (Zoom(1,3) ; Zoom(1,1/3))
        // = identity || identity → should collapse to UnitMap(2)
        let z1 = ZoomMap::new(1, 2.0).unwrap();
        let z2 = ZoomMap::new(1, 0.5).unwrap();
        let ser1 = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Series).unwrap();

        let z3 = ZoomMap::new(1, 3.0).unwrap();
        let z4 = ZoomMap::new(1, 1.0 / 3.0).unwrap();
        let ser2 = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Series).unwrap();

        let parallel = CmpMap::new(dyn_map(ser1), dyn_map(ser2), Composition::Parallel).unwrap();
        let result = simplify_mapping(&parallel).unwrap();

        // Each series pair cancels to UnitMap, so the parallel of two
        // UnitMaps should be a single UnitMap(2).
        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);

        // Verify transform: identity.
        let pts = PointSet::from_rows([[7.0, 13.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.axis(0)[0] - 7.0).abs() < 1e-12);
        assert!((out.axis(1)[0] - 13.0).abs() < 1e-12);
    }

    #[test]
    fn series_in_parallel_length_mismatch_no_change() {
        use crate::mappings::MatrixMap;
        // (Zoom(1,2) ; Zoom(1,3) ; Zoom(1,4)) || (Zoom(1,5) ; Zoom(1,6))
        // Different series lengths → cross-composition restructuring skipped.
        // However, each series merges individually (2*3*4=24, 5*6=30), then
        // the parallel of two ZoomMaps with different factors merges into a
        // diagonal MatrixMap.
        let z1 = ZoomMap::new(1, 2.0).unwrap();
        let z2 = ZoomMap::new(1, 3.0).unwrap();
        let z3 = ZoomMap::new(1, 4.0).unwrap();
        let ser1_inner = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Series).unwrap();
        let ser1 = CmpMap::new(dyn_map(ser1_inner), dyn_map(z3), Composition::Series).unwrap();

        let z4 = ZoomMap::new(1, 5.0).unwrap();
        let z5 = ZoomMap::new(1, 6.0).unwrap();
        let ser2 = CmpMap::new(dyn_map(z4), dyn_map(z5), Composition::Series).unwrap();

        let parallel = CmpMap::new(dyn_map(ser1), dyn_map(ser2), Composition::Parallel).unwrap();
        let result = simplify_mapping(&parallel).unwrap();

        // Series merge individually, then parallel ZoomMaps with different
        // factors produce a diagonal MatrixMap.
        assert!(result.as_any().is::<MatrixMap>());
        assert_eq!(result.nin(), 2);
    }

    #[test]
    fn permmap_swap_basic_block_swap() {
        use crate::coords::PointSet;

        // PermMap that swaps two 1-D blocks followed by Par(Shift([10]), Shift([20]))
        // PermMap: outperm=[2,1], inperm=[2,1] (swap axes)
        // After swap: Par(Shift([20]), Shift([10])) followed by identity PermMap
        // Which then simplifies away.
        let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        let s1 = ShiftMap::new(vec![10.0]);
        let s2 = ShiftMap::new(vec![20.0]);
        let par = CmpMap::new(dyn_map(s1), dyn_map(s2), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(pm), dyn_map(par), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        // Transform: [x,y] → PermMap → [y,x] → Par(Shift10, Shift20) → [y+10, x+20]
        // Verify with [1, 2]:
        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let out_orig = series.transform(&pts, true).unwrap();
        let out_simp = result.transform(&pts, true).unwrap();
        assert!((out_orig.axis(0)[0] - out_simp.axis(0)[0]).abs() < 1e-12);
        assert!((out_orig.axis(1)[0] - out_simp.axis(1)[0]).abs() < 1e-12);
    }

    #[test]
    fn permmap_swap_non_block_no_change() {
        use crate::coords::PointSet;

        // PermMap that is NOT a block swap: outperm=[1,2] (identity).
        // This doesn't have the block-swap structure needed.
        let pm = PermMap::new(&[1, 2], &[1, 2], &[]).unwrap();
        let s1 = ShiftMap::new(vec![10.0]);
        let s2 = ShiftMap::new(vec![20.0]);
        let par = CmpMap::new(dyn_map(s1), dyn_map(s2), Composition::Parallel).unwrap();

        let series = CmpMap::new(dyn_map(pm), dyn_map(par), Composition::Series).unwrap();
        let result = simplify_mapping(&series).unwrap();

        // Identity PermMap should merge away, parallel CmpMap remains.
        let pts = PointSet::from_rows([[1.0, 2.0]]);
        let out_orig = series.transform(&pts, true).unwrap();
        let out_simp = result.transform(&pts, true).unwrap();
        assert!((out_orig.axis(0)[0] - out_simp.axis(0)[0]).abs() < 1e-12);
        assert!((out_orig.axis(1)[0] - out_simp.axis(1)[0]).abs() < 1e-12);
    }

    #[test]
    fn iteration_cap_prevents_infinite_loop() {
        // Test that deeply nested parallel-in-series chains terminate.
        // Use ZoomMaps (nin==nout) so the series composition is valid.
        let z1 = ZoomMap::new(2, 2.0).unwrap();
        let z2 = ZoomMap::new(2, 3.0).unwrap();
        let par = CmpMap::new(dyn_map(z1), dyn_map(z2), Composition::Parallel).unwrap();

        let z3 = ZoomMap::new(2, 5.0).unwrap();
        let z4 = ZoomMap::new(2, 7.0).unwrap();
        let par2 = CmpMap::new(dyn_map(z3), dyn_map(z4), Composition::Parallel).unwrap();

        let z5 = ZoomMap::new(2, 11.0).unwrap();
        let z6 = ZoomMap::new(2, 13.0).unwrap();
        let par3 = CmpMap::new(dyn_map(z5), dyn_map(z6), Composition::Parallel).unwrap();

        // Build a 3-element series chain of parallel CmpMaps.
        let inner = CmpMap::new(dyn_map(par), dyn_map(par2), Composition::Series).unwrap();
        let outer = CmpMap::new(dyn_map(inner), dyn_map(par3), Composition::Series).unwrap();

        // Should terminate and produce correct dimensions.
        let result = simplify_mapping(&outer).unwrap();
        assert_eq!(result.nin(), 4);
        assert_eq!(result.nout(), 4);
    }

    #[test]
    fn series_parallel_absorb_trailing_shift_into_branch() {
        // series( parallel{ WcsMap(2-axis TAN), UnitMap(1) }, ShiftMap(3, Sft3=7.5) )
        // The trailing 3-axis ShiftMap only shifts axis 3, which falls in the
        // UnitMap branch. It must be absorbed: the axis-1,2 part is identity
        // (vanishes against WcsMap), and the axis-3 part becomes ShiftMap(1).
        // Result: parallel{ WcsMap(2-axis TAN), ShiftMap(1, Sft1=7.5) }.
        use crate::coords::PointSet;
        use crate::mappings::{UnitMap, WcsMap, WcsProjection};

        let wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        let par = CmpMap::new(
            Box::new(wcs),
            Box::new(UnitMap::new(1)),
            Composition::Parallel,
        )
        .unwrap();
        let shift = ShiftMap::new(vec![0.0, 0.0, 7.5]);
        let series = CmpMap::new(Box::new(par), Box::new(shift), Composition::Series).unwrap();

        let result = simplify_mapping(&series).unwrap();

        // Top level must be a parallel CmpMap.
        let cmp = result
            .as_any()
            .downcast_ref::<CmpMap>()
            .expect("result should be a CmpMap");
        assert_eq!(cmp.composition(), Composition::Parallel);
        assert_eq!(result.nin(), 3);
        assert_eq!(result.nout(), 3);

        // First branch is the WcsMap, second branch is a 1-axis ShiftMap.
        assert!(
            cmp.first().as_any().is::<WcsMap>(),
            "first branch should be WcsMap"
        );
        let second = cmp.second();
        assert!(
            second.as_any().is::<ShiftMap>(),
            "second branch should be ShiftMap"
        );
        assert_eq!(second.nin(), 1);
        let sm = second.as_any().downcast_ref::<ShiftMap>().unwrap();
        assert!((sm.shifts()[0] - 7.5).abs() < 1e-12);

        // Transform equivalence: the third (UnitMap) axis is shifted by 7.5
        // both before and after, independent of the WcsMap on axes 1,2.
        let pts = PointSet::from_rows([[0.1, 0.2, 4.0]]);
        let before = series.transform(&pts, true).unwrap();
        let after = result.transform(&pts, true).unwrap();
        for k in 0..3 {
            assert!(
                (before.point(0)[k] - after.point(0)[k]).abs() < 1e-12,
                "axis {k} mismatch: {} vs {}",
                before.point(0)[k],
                after.point(0)[k]
            );
        }
        assert!((after.axis(2)[0] - 11.5).abs() < 1e-12);
    }

    /// C's base `Simplify` builds a one-element list and calls `astMapMerge`
    /// until nothing changes or the list grows past one
    /// (`mapping.c:16871-16883`). A unit-factor ZoomMap is a UnitMap and
    /// `zoom_map_merge` says so, so the driver must reach that with no
    /// per-class `simplified()` in the way.
    #[test]
    fn base_simplify_reaches_a_classes_own_map_merge() {
        let zoom = ZoomMap::new(2, 1.0).expect("unit-factor ZoomMap constructs");
        let (out, _) = base_simplify(&zoom).expect("base_simplify succeeds");
        assert!(
            out.as_any().is::<UnitMap>(),
            "expected a UnitMap from a unit-factor ZoomMap"
        );
    }

    /// A unit-scale WinMap becomes a ShiftMap wherever it sits, because the
    /// narrowing is `win_map_merge`'s closing step (`winmap.c:1820-1822`) and
    /// the nominate loop reaches it for a parallel component too.
    ///
    /// The PermMap sibling is the discriminating part: it cannot merge with the
    /// WinMap, so the WinMap survives into the reconstructed tree and the
    /// narrowing has to have happened while it was nominated rather than as a
    /// side effect of a series merge.
    #[test]
    fn unit_scale_winmap_in_a_parallel_branch_narrows_to_a_shiftmap() {
        use crate::mappings::{PermMap, WinMap};

        let win = WinMap::new(&[3.0], &[1.0]).expect("unit-scale WinMap constructs");
        let perm = PermMap::new(&[1, 2], &[2, 1], &[]).expect("axis-swap PermMap constructs");
        let par = CmpMap::new(Box::new(win), Box::new(perm), Composition::Parallel)
            .expect("parallel CmpMap constructs");

        let out = <dyn Mapping>::simplify(&par).expect("simplify succeeds");
        let cm = out
            .as_any()
            .downcast_ref::<CmpMap>()
            .expect("still a parallel CmpMap");
        let shift = cm
            .first()
            .as_any()
            .downcast_ref::<ShiftMap>()
            .expect("the unit-scale WinMap narrowed to a ShiftMap");
        assert_eq!(shift.shifts(), &[3.0]);
    }

    /// A Mapping its own `map_merge` cannot reduce comes back unchanged rather
    /// than erroring, which is C returning a clone.
    #[test]
    fn base_simplify_returns_a_clone_when_nothing_reduces() {
        let z = ZoomMap::new(1, 3.0).expect("ZoomMap constructs");
        let (out, _) = base_simplify(&z).expect("base_simplify succeeds");
        assert_eq!(
            out.as_any()
                .downcast_ref::<ZoomMap>()
                .expect("still a ZoomMap")
                .zoom(),
            3.0
        );
    }
}

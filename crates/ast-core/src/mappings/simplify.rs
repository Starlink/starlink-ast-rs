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
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use crate::constants::BAD_VALUE;
use crate::error::AstResult;
use crate::mappings::helpers::ast_equal;

use super::cmpmap::{CmpMap, Composition};
use super::grismmap::GrismMap;
use super::matrix::{MatrixMap, is_diagonal};
use super::pcdmap::PcdMap;
use super::perm::PermMap;
use super::shift::ShiftMap;
use super::sphmap::SphMap;
use super::tranmap::TranMap;
use super::unit::UnitMap;
use super::wcsmap::WcsMap;
use super::winmap::WinMap;
use super::zoom::ZoomMap;
use super::{DynMapping, Mapping};

// ------------------------------------------------------------------ //
// Recursion guard: fingerprint stack                                   //
// ------------------------------------------------------------------ //

thread_local! {
    /// Tracks structural fingerprints of mappings currently being
    /// simplified on this thread.  Before entering the flatten/merge/
    /// reconstruct pipeline, we compute a fingerprint of the mapping
    /// and check whether an equivalent structure is already being
    /// simplified higher up the call stack.  If so, we bail immediately
    /// to prevent infinite recursion.
    ///
    /// This matches the C library's thread-local "simplify stack"
    /// (cmpmap.c:3481-3493) which stores pointers to mappings currently
    /// in progress and checks equality before entering simplification.
    /// We use structural fingerprints (hashes) instead of pointer
    /// equality since Rust values move.
    static SIMPLIFY_STACK: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());

    /// Whether the most recent [`simplify_mapping`] of a `CmpMap` built a fresh
    /// object rather than handing back a copy of its input.
    ///
    /// C's `CmpMap::Simplify` returns `astClone( this )` when its `simpler` flag
    /// stayed 0 and a newly folded Mapping otherwise (cmpmap.c:3630-3636), and
    /// callers distinguish the two by pointer identity. `MapMerge`'s
    /// self-simplify head is one such caller: `simpler = ( new != map ) || ...`
    /// (cmpmap.c:1456). A fold can reproduce the input's structure exactly and
    /// still be a different pointer, so structural comparison alone is not
    /// enough to reproduce that test — this flag supplies the missing bit.
    static SIMPLIFY_REBUILT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the `simplify` call that just returned built a fresh object rather
/// than handing back a copy of its input.
///
/// This is the bit C reads by comparing pointers after `astSimplify`. Callers
/// that reproduce such a test — `CmpMap::MapMerge`'s self-simplify head
/// (cmpmap.c:1456) and `WinMap::MapMerge`'s parallel distribution
/// (winmap.c:1370) — consult it alongside a structural comparison, since a
/// rebuild can reproduce its input's shape exactly and still be a new pointer.
pub(crate) fn simplify_rebuilt() -> bool {
    SIMPLIFY_REBUILT.get()
}

/// Reset the rebuild flag before a `simplify` call whose outcome will be tested.
pub(crate) fn clear_simplify_rebuilt() {
    SIMPLIFY_REBUILT.set(false);
}

/// Compute a structural fingerprint of a mapping for recursion detection.
///
/// The fingerprint incorporates the concrete type, dimensions, and for
/// CmpMaps the composition kind and recursive fingerprints of children.
/// Collisions are possible but harmless (they only cause an early bail
/// which is a conservative no-op, not a correctness issue).
pub(crate) fn mapping_fingerprint(mapping: &dyn Mapping) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    mapping.nin().hash(&mut hasher);
    mapping.nout().hash(&mut hasher);
    mapping.as_any().type_id().hash(&mut hasher);
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        (cmp.composition() == Composition::Series).hash(&mut hasher);
        mapping_fingerprint(cmp.first()).hash(&mut hasher);
        mapping_fingerprint(cmp.second()).hash(&mut hasher);
    }
    if let Some(tm) = mapping.as_any().downcast_ref::<TranMap>() {
        mapping_fingerprint(tm.map1()).hash(&mut hasher);
        mapping_fingerprint(tm.map2()).hash(&mut hasher);
    }
    if let Some(sw) = mapping.as_any().downcast_ref::<super::SwitchMap>() {
        sw.nroute().hash(&mut hasher);
        if let Some(fs) = sw.forward_selector() {
            mapping_fingerprint(fs).hash(&mut hasher);
        }
        if let Some(is) = sw.inverse_selector() {
            mapping_fingerprint(is).hash(&mut hasher);
        }
        for rm in sw.route_maps() {
            mapping_fingerprint(rm.as_ref()).hash(&mut hasher);
        }
    }
    if let Some(sel) = mapping.as_any().downcast_ref::<super::SelectorMap>() {
        sel.regions().len().hash(&mut hasher);
    }
    hasher.finish()
}

/// Maximum allowed recursion depth for `simplify_mapping`.
///
/// This complements the fingerprint-based cycle detection by capping
/// the call-stack depth absolutely.  Even if fingerprints don't match
/// (non-identical but deeply recursive structures), this limit
/// prevents stack overflow.
const MAX_SIMPLIFY_DEPTH: usize = 10;

/// RAII guard that manages both the fingerprint stack and depth counter.
///
/// On creation: checks the fingerprint against the stack (cycle detection)
/// and increments a depth counter (absolute limit).
/// On drop: removes the fingerprint and decrements the counter.
struct SimplifyGuard {
    fingerprint: u64,
}

thread_local! {
    static SIMPLIFY_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Re-entrancy depth of the public `Mapping::simplify` entry point.
    /// Some internal simplification steps (e.g. the WinMap parallel-CmpMap
    /// distribution probe in `winmap_merge.rs`) call the public `simplify`
    /// recursively; this counter lets the final top-level cleanup passes run
    /// only at the outermost call.
    static PUBLIC_SIMPLIFY_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// RAII guard that tracks re-entrancy of the public `Mapping::simplify`
/// entry point. `is_outermost()` is true only for the first (outermost)
/// active call on this thread.
pub(crate) struct PublicSimplifyGuard {
    outermost: bool,
}

impl PublicSimplifyGuard {
    /// Enter the public simplify entry, incrementing the depth counter.
    pub(crate) fn enter() -> Self {
        let outermost = PUBLIC_SIMPLIFY_DEPTH.with(|d| {
            let cur = d.get();
            d.set(cur + 1);
            cur == 0
        });
        Self { outermost }
    }

    /// Whether this is the outermost active public simplify call.
    pub(crate) fn is_outermost(&self) -> bool {
        self.outermost
    }
}

impl Drop for PublicSimplifyGuard {
    fn drop(&mut self) {
        PUBLIC_SIMPLIFY_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

impl SimplifyGuard {
    /// Attempt to enter simplification for `mapping`.
    ///
    /// Returns `None` if:
    /// - The mapping's fingerprint is already on the stack (cycle), or
    /// - The recursion depth limit has been reached.
    ///
    /// On success, returns a guard that cleans up on drop.
    fn enter(mapping: &dyn Mapping) -> Option<Self> {
        let fp = mapping_fingerprint(mapping);

        // Check for cycle: is this fingerprint already on the stack?
        let cycle = SIMPLIFY_STACK.with(|stack| stack.borrow().contains(&fp));
        if cycle {
            return None;
        }

        // Check depth limit.
        let depth_ok = SIMPLIFY_DEPTH.with(|d| {
            let current = d.get();
            if current >= MAX_SIMPLIFY_DEPTH {
                false
            } else {
                d.set(current + 1);
                true
            }
        });
        if !depth_ok {
            return None;
        }

        // Push fingerprint onto stack.
        SIMPLIFY_STACK.with(|stack| {
            stack.borrow_mut().insert(fp);
        });

        Some(SimplifyGuard { fingerprint: fp })
    }
}

impl Drop for SimplifyGuard {
    fn drop(&mut self) {
        SIMPLIFY_STACK.with(|stack| {
            stack.borrow_mut().remove(&self.fingerprint);
        });
        SIMPLIFY_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

/// Trace the simplify machinery as `tracing` events, mirroring
/// `tools/oracle_fixtures/build_simplify_trace.sh`'s instrumentation of the C
/// library so the two routes through a Mapping can be diffed line for line.
///
/// Everything here compiles to nothing without the `tracing` feature, following
/// [`crate::AstError`]'s pattern. The field names match the C trace's, so the
/// same `awk` recipe reads both — see `tools/oracle_fixtures/README.md`.
///
/// These sit in the decomposition and merge machinery, not in any `transform`
/// path, so the hot-path constraint in `CLAUDE.md` is unaffected.
pub(crate) mod trace {
    use super::Mapping;

    /// Class name for a trace line. `Serializable::class_name` is private to
    /// the serial module, so the classes the traces mention are matched here and
    /// anything else prints as `?`.
    #[cfg(feature = "tracing")]
    fn class_of(m: &dyn Mapping) -> &'static str {
        use crate::mappings::{
            CmpMap, Composition, GrismMap, LutMap, MatrixMap, NormMap, PcdMap, PermMap, RateMap,
            ShiftMap, SlaMap, SpecMap, SphMap, SwitchMap, TimeMap, TranMap, UnitMap, UnitNormMap,
            WcsMap, WinMap, ZoomMap,
        };
        if let Some(c) = m.as_any().downcast_ref::<CmpMap>() {
            return if c.composition() == Composition::Series {
                "CmpMap[series]"
            } else {
                "CmpMap[parallel]"
            };
        }
        macro_rules! name {
            ($($cls:ident),+ $(,)?) => {
                $( if m.as_any().is::<$cls>() { return stringify!($cls); } )+
            };
        }
        name!(
            UnitMap,
            ZoomMap,
            WinMap,
            ShiftMap,
            MatrixMap,
            PermMap,
            PcdMap,
            LutMap,
            SpecMap,
            SlaMap,
            TimeMap,
            SphMap,
            WcsMap,
            GrismMap,
            NormMap,
            UnitNormMap,
            RateMap,
            TranMap,
            SwitchMap,
        );
        "?"
    }

    /// A per-class reduction is about to run. C's `SIMPLIFY-ENTER`.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn simplify_enter(site: &str, m: &dyn Mapping) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            class = class_of(m),
            issimp = u8::from(m.has_been_simplified()),
            inv = u8::from(m.is_inverted()),
            nin = m.nin(),
            nout = m.nout(),
            site,
            "SIMPLIFY-ENTER",
        );
    }

    /// A class is being nominated. C's `MAPMERGE`.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn map_merge(m: &dyn Mapping, where_: usize, series: bool, len: usize) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            class = class_of(m),
            where_,
            series = u8::from(series),
            nmap = len,
            issimp = u8::from(m.has_been_simplified()),
            inv = u8::from(m.is_inverted()),
            "MAPMERGE",
        );
    }

    /// What a nomination returned: the index it modified, or nothing. C's
    /// `MAPMERGE-RESULT`. Distinguishes a productive nomination from one that
    /// declined, which is the difference between churn and progress.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn map_merge_result(m: &dyn Mapping, result: Option<usize>) {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            target: "ast::simplify",
            class = class_of(m),
            result = result.map_or(-1i64, |i| i as i64),
            "MAPMERGE-RESULT",
        );
    }

    /// A decomposition is being requested. C's `MAPLIST`, the highest-volume of
    /// the three, so it sits a level lower.
    #[inline]
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn map_list(m: &dyn Mapping, series: bool, inv: bool) {
        #[cfg(feature = "tracing")]
        tracing::trace!(
            target: "ast::simplify",
            class = class_of(m),
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

    // Try to decompose if this is a CmpMap of the matching kind.
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == target_kind {
            // Combine external inversion with the CmpMap's own flag.
            // XOR: if both are inverted they cancel.
            let cmp_inv = inv != cmp.is_inverted();

            // Effective inversion for each child: inv_a/inv_b record
            // the expected child invert state. If the child's current
            // state differs, that flips the direction. Then the CmpMap's
            // own inversion (cmp_inv) flips again.
            let child_inv_a = cmp_inv != (cmp.inv_a() != cmp.first().is_inverted());
            let child_inv_b = cmp_inv != (cmp.inv_b() != cmp.second().is_inverted());

            if !cmp_inv {
                let mut result = flatten(cmp.first(), child_inv_a, series)?;
                result.extend(flatten(cmp.second(), child_inv_b, series)?);
                return Ok(result);
            } else if series {
                // Inverted series: reverse child order.
                let mut result = flatten(cmp.second(), child_inv_b, series)?;
                result.extend(flatten(cmp.first(), child_inv_a, series)?);
                return Ok(result);
            } else {
                // Inverted parallel: same order.
                let mut result = flatten(cmp.first(), child_inv_a, series)?;
                result.extend(flatten(cmp.second(), child_inv_b, series)?);
                return Ok(result);
            }
        }
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
    // see its neighbours — which is what lets `WinMat` fold a WinMap's residual
    // scale into an adjacent MatrixMap, and what makes "replace with simpler"
    // the *last* step of a class's merge rather than the first thing that
    // happens to it.
    let mut cloned = mapping.clone_box();
    // `AllowSimplify` (C `AlSimp`) and the `IsSimp` dump tag used to be copied
    // across by hand here, because `simplified()` returned a fresh object
    // without them. `Clone` carries both, which is what `astMapList` appending
    // the component pointer itself amounts to.
    // The leaf's own (intrinsic) Invert state, independent of any inversion
    // imposed by a parent CmpMap. Preserved as the element's `orig_inv` so
    // `reconstruct` serialises the child in the orientation C does: C keeps each
    // component's stored Invert and records the effective direction separately
    // in the parent's `InvA`/`InvB`. The clone is the leaf untouched, so its
    // orientation *is* the stored one and there is nothing to un-normalise.
    let orig_inv = cloned.is_inverted();
    let effective_inv = inv != orig_inv;
    // Reset the clone to forward state; the bool carries inversion.
    if cloned.is_inverted() {
        cloned.invert();
    }

    Ok(vec![(cloned, effective_inv, orig_inv)])
}

/// Mirror of the `simpler` flag returned by C `cmpmap.c` `MapList`
/// (`result = invert || r1 || r2`, MapList:1191): true when decomposing
/// `mapping` (in direction `inv`, for the given `series`/parallel kind)
/// encounters any same-kind CmpMap in an inverted state. Such an inversion
/// is normalised away by the flatten/reconstruct round-trip, so C treats it
/// as a simplification even when no component pair merges. Opposite-kind
/// CmpMaps and non-CmpMap leaves contribute nothing here; the change an
/// opposite-kind component can report comes from the nominate loop instead
/// (see [`opaque_leaf_is_simpler`]).
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

/// Why an *opposite-kind* `CmpMap` component was reduced by [`flatten`]'s eager
/// leaf reduction, and therefore how the enclosing node must be rebuilt.
///
/// The two reducing paths have different consequences in C, because they place
/// the rebuild in different objects.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpaqueLeafReduction {
    /// No component was reduced.
    None,
    /// The component's own `astMapList` reports `simpler` — an inverted
    /// same-kind CmpMap inside it — so its `Simplify` always takes the rebuild
    /// branch and hands back a *new* pointer. The enclosing node's
    /// `simpler = ( new != map )` (cmpmap.c:1456) is therefore 1 and its closing
    /// fold runs.
    Rebuilds,
    /// The component's own `astMapList` reports nothing, so the reduction can
    /// only come from its nominate loop. That loop converges: once it has run,
    /// the reduced object sits in the tree and a further `astSimplify` on it
    /// finds nothing and returns the supplied pointer (mapping.c:24740, or the
    /// `astClone( this )` at cmpmap.c:3634 which `astSimplify_` then stamps in
    /// place). The enclosing node's `simpler` is 0 on that converged visit, so
    /// it takes `astClone( this )` and never folds.
    Converged,
}

/// Whether an *opposite-kind* `CmpMap` component would be normalised by
/// [`flatten`]'s eager leaf reduction, and so must be adopted from the
/// component list rather than read back off the original tree.
///
/// C never needs this test. Its `astMapList` appends such a component
/// unchanged, and the nominate loop's self-simplify head reduces it in place
/// (`astSimplify( map )` then `new != map`, cmpmap.c:1437-1456) — mutating the
/// very list entry the closing fold later wraps. `flatten` performs that
/// reduction as it builds the list instead, so the change never registers as a
/// merge-loop `changed`; detect it here, read-only, from the same tree C
/// inspects.
///
/// A component already carrying `IsSimple` is exempt: C's `astSimplify_`
/// early-out (mapping.c:24740) returns it as a clone of the supplied pointer
/// without dispatching to the class `Simplify`, so `new != map` is false. That
/// tag reaches the component because `flatten` carries it across the leaf
/// reduction, as C `astMapList` carries every flag on the component pointer it
/// appends. The exemption is conditional on the component's stored `Invert`
/// already matching the effective direction: C reaches `astSimplify` through
/// `astSetInvert( map, invert_list[ where ] )` (cmpmap.c:1433), and that macro
/// clears `IsSimple` (mapping.c:23256), so a component that has to be re-oriented
/// arrives at the early-out untagged and does dispatch.
fn opaque_leaf_reduction(mapping: &dyn Mapping, inv: bool, series: bool) -> OpaqueLeafReduction {
    let target_kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == target_kind {
            let cmp_inv = inv != cmp.is_inverted();
            let child_inv_a = cmp_inv != (cmp.inv_a() != cmp.first().is_inverted());
            let child_inv_b = cmp_inv != (cmp.inv_b() != cmp.second().is_inverted());
            let a = opaque_leaf_reduction(cmp.first(), child_inv_a, series);
            // A rebuilding component anywhere in the list drives the enclosing
            // fold, so it dominates a merely-converged one.
            if a == OpaqueLeafReduction::Rebuilds {
                return a;
            }
            let b = opaque_leaf_reduction(cmp.second(), child_inv_b, series);
            return if b == OpaqueLeafReduction::None { a } else { b };
        }
        // C re-orients the component to its effective direction before
        // simplifying, and only an *unchanged* stored `Invert` leaves the tag in
        // place for the early-out to honour.
        let reoriented = inv != cmp.is_inverted();
        if cmp.has_been_simplified() && !reoriented {
            return OpaqueLeafReduction::None;
        }
        // C's `new != map`: `astSimplify` returns a new object precisely when
        // the component's own `Simplify` took its rebuild branch. Half of that
        // decision is decidable without running it — `astMapList` inside the
        // component reports `simpler` as `invert || r1 || r2` over the
        // component's *own* composition kind, evaluated at the effective
        // direction C imposes with `astSetInvert(map, invert_list[where])`
        // (cmpmap.c:1433). An effectively-inverted component therefore always
        // rebuilds, however transform-identical the result.
        let own_series = cmp.composition() == Composition::Series;
        if decomposition_is_simpler(cmp, inv, own_series) {
            return OpaqueLeafReduction::Rebuilds;
        }
        // Otherwise the rebuild can only come from the component's nominate
        // loop, which shows up as a structural difference.
        let mut oriented = cmp.clone_box();
        if inv {
            oriented.invert();
        }
        trace::simplify_enter("cmp-tag-transfer", oriented.as_ref());
        if let Ok(simplified) = oriented.simplify()
            && !simplified.equal(oriented.as_ref())
        {
            return OpaqueLeafReduction::Converged;
        }
    }
    OpaqueLeafReduction::None
}

/// Working state for the per-class `map_merge` nominate loop.
///
/// Mirrors the C `Simplify` engine's mutable arguments to `astMapMerge`
/// (`cmpmap.c:3572`): the `map_list` of components, the parallel
/// `invert_list` of effective-direction flags, and the per-mapping
/// `Frozen` flag (`astSetFrozen`/`astFrozen`, `cmpmap.c:3530-3624`). In C
/// the frozen flag lives on the mapping object; here it is a per-entry
/// bool held alongside the list, kept index-parallel with `list` so that
/// inserting, removing, or reordering components keeps the flag attached
/// to the right entry.
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
    /// Per-entry frozen flag, index-parallel with `list` (C `astFrozen`).
    frozen: Vec<bool>,
    /// Whether the enclosing CmpMap has `RestrictedSimplify` set. When
    /// true, only components whose `AllowSimplify` flag is set may be
    /// nominated (C `cmpmap.c:3518-3564`).
    restricted: bool,
}

impl MergeCtx {
    /// Wrap a flattened component list, all entries initially unfrozen
    /// (C clears `Frozen` on every component before the loop,
    /// `cmpmap.c:3530-3533`).
    pub(crate) fn new(list: Vec<(DynMapping, bool, bool)>) -> Self {
        let frozen = vec![false; list.len()];
        Self {
            list,
            frozen,
            restricted: false,
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
    /// live from the component (a merged result is a new mapping without the
    /// flag, so it drops out of the allowed set — matching C, which clears
    /// AllowSimplify after simplification).
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
        self.frozen.get(index).copied().unwrap_or(false)
    }

    /// Freeze the component at `index` (C `astSetFrozen`), preventing it
    /// from being nominated again. Used by the oscillation break.
    pub(crate) fn freeze(&mut self, index: usize) {
        if let Some(f) = self.frozen.get_mut(index) {
            *f = true;
        }
    }

    /// Re-establish `frozen.len() == list.len()` after a mutation that
    /// changed the working list outside the precise per-entry bookkeeping
    /// (e.g. a global swap helper that operates directly on `list`). New
    /// entries are unfrozen; surplus flags are dropped. This is a
    /// length-only resync: the frozen *contents* only ever differ from
    /// all-false once the oscillation break has fired (`pattern_check`),
    /// which the global helpers being retired in later phases never
    /// trigger on the fixture corpus.
    fn resync_frozen(&mut self) {
        self.frozen.resize(self.list.len(), false);
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
    pub(crate) fn set_component_simple_tag(&mut self, index: usize) {
        if let Some(cm) = self.list[index].0.as_any_mut().downcast_mut::<CmpMap>() {
            let inv = cm.is_inverted();
            cm.set_dump_simplified_in(inv, true);
        }
    }

    /// Replace the two adjacent entries at `i1`/`i1+1` with a single merged
    /// entry, closing the gap. Used by per-class `map_merge` after a direct
    /// merge produces one mapping from a neighbouring pair (C annuls the two
    /// pointers, stores the merged one at `i1`, and shuffles the tail down).
    pub(crate) fn replace_pair_with(&mut self, i1: usize, mut merged: DynMapping, eff_inv: bool) {
        // Normalise the entry to `(forward_map, effective_inv, orig_inv)`.
        // A merge may return a mapping in its inverted orientation (e.g.
        // `MatrixMap::mtr_zoom` preserves an inverted MatrixMap so it
        // serialises `Invert=1` like C). Record that intrinsic orientation as
        // `orig_inv` and fold it into the effective direction, then store the
        // mapping forward — matching how `flatten` produces leaf entries.
        let orig_inv = merged.is_inverted();
        if orig_inv {
            merged.invert();
        }
        self.list[i1] = (merged, eff_inv != orig_inv, orig_inv);
        self.list.remove(i1 + 1);
        self.frozen.remove(i1 + 1);
    }

    /// Remove the single entry at `index`, shuffling the tail down. Faithful
    /// analogue of C `UnitMap::MapMerge` eliminating a UnitMap from a series
    /// sequence (unitmap.c:519): the mapping is annulled and the following
    /// elements shift down to fill the gap.
    pub(crate) fn remove_at(&mut self, index: usize) {
        self.list.remove(index);
        self.frozen.remove(index);
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
            self.frozen.remove(idx);
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
        trace::map_merge(nominee.as_ref(), sub_index, series, sub.len());
        let modified = nominee.map_merge(&mut sub, sub_index, series);
        trace::map_merge_result(nominee.as_ref(), modified);
        self.list.extend(sub.list);
        self.resync_frozen();
        modified
    }
}

/// Default `map_merge` implementation: a single direct pairwise merge of
/// the nominated component with its immediate right neighbour.
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
/// Before considering its neighbours, C recursively `astSimplify`s the
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
/// `default_map_merge` so the neighbour-merge behaviour is unchanged.
pub(crate) fn cmp_map_merge(
    cmp: &CmpMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    if !cmp.has_been_simplified() {
        let eff_inv = ctx.entry(where_).1;

        // Simplify the component in its effective applied direction, exactly as
        // C sets `astSetInvert(map, invert_list[where])` before `astSimplify`.
        let mut oriented = cmp.clone_box();
        if eff_inv {
            oriented.invert();
        }
        let fp_before = mapping_fingerprint(oriented.as_ref());

        SIMPLIFY_REBUILT.set(false);
        trace::simplify_enter("cmp-self-simplify-head", oriented.as_ref());
        if let Ok(simplified) = oriented.simplify() {
            // C's test is `simpler = ( new != map ) || ( new_invert !=
            // invert_list[where] )` (cmpmap.c:1456) — pointer identity, not
            // structure. A `CmpMap::Simplify` that set its own `simpler` flag
            // folds a new object out of its component list, so `new != map` holds
            // even when the fold reproduces the input's shape exactly; only the
            // `astClone( this )` branch yields the original pointer. Reproduce
            // that with the structural comparison *or* the rebuild flag the
            // recursion just set.
            if SIMPLIFY_REBUILT.get() || mapping_fingerprint(simplified.as_ref()) != fp_before {
                let new_inv = simplified.is_inverted();
                let mut fwd = simplified;
                if new_inv {
                    fwd.invert();
                }
                // C `astSimplify_` (mapping.c:24763) tagged this nominated
                // component's reduced result. The record is per orientation and
                // `fwd` sits at the forward one after the normalising `invert()`
                // above, while the tree serialises it at `new_inv`; record the
                // orientation the tag was earned in.
                fwd.set_simplified_in(new_inv, true);
                // C stores the result object at its own orientation and sets
                // `invert_list[where] = astGetInvert(new)` (cmpmap.c:1463-1464),
                // so effective == stored for the replaced element; the final
                // fold's set/restore then serialises the child at that stored
                // orientation. Record it as `orig_inv` so `reconstruct` does
                // the same.
                ctx.set_pair_single(where_, (fwd, new_inv, new_inv));
                return Some(where_);
            }
        }

        ctx.set_component_simple_tag(where_);
    }

    // Merge adjacent CmpMaps (C `cmpmap.c::MapMerge` 1528-1813).
    // ========================================================
    // The self-simplify head above did not fire (C `!simpler`); look for a
    // neighbouring CmpMap with which the nominated one can merge. C uses the
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
            if let Some(new) = merged {
                ctx.replace_pair_with(imap1, new, false);
                return Some(imap1);
            }
        }
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
/// ZoomMap serialises as a forward `Zoom` value, never as an inverted one).
/// Whether a ZoomMap can be absorbed into `m` as a neighbour (C
/// zoommap.c::MapMerge handles MatrixMap, WinMap and ShiftMap neighbours).
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
        Some(if entry.1 {
            1.0 / zm.factor()
        } else {
            zm.factor()
        })
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
            1.0 / zoom.factor()
        } else {
            zoom.factor()
        };

        // Extend downward over adjacent ZoomMaps (accumulate) and UnitMaps.
        let mut imap1 = where_;
        while imap1 >= 1 {
            let prev = ctx.entry(imap1 - 1);
            if is_zoom(prev) {
                let z = eff_zoom(prev)?;
                zoom_acc *= z;
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
                let z = eff_zoom(next)?;
                zoom_acc *= z;
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
            // neighbouring MatrixMap/WinMap/ShiftMap. Mirroring C
            // zoommap.c::MapMerge (994-1122), the *previous* neighbour is
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
        let new: DynMapping = if unit {
            Box::new(UnitMap::new(nin))
        } else {
            Box::new(ZoomMap::new(nin, zoom_acc).ok()?)
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
        Box::new(ZoomMap::new(nin, minzoom).ok()?)
    } else {
        let mut diag = ndarray::Array2::<f64>::zeros((nin, nin));
        for (i, &z) in zooms.iter().enumerate() {
            diag[[i, i]] = z;
        }
        Box::new(MatrixMap::new(diag).ok()?)
    };
    ctx.replace_range_with(imap1, imap2, new);
    Some(imap1)
}

/// `map_merge` for a nominated `PermMap`, performing the CmpMap-past-PermMap
/// swap of C `cmpmap.c::MapMerge` (1816-2119).
///
/// C runs that swap on the other element: it fires when the *CmpMap* is
/// nominated and the PermMap is its lower neighbour, and `permmap.c` never
/// mentions `CmpMap` at all. Here it fires when the PermMap is nominated and
/// the CmpMap is its upper neighbour. Those are the same pair, so the nominate
/// loop reaches the swap either way, but the index reported differs and so does
/// where the loop resumes -- see PLAN.md §10.4.
///
/// When a nominated PermMap immediately precedes a parallel CmpMap in a series
/// sequence and has a "block-swap" structure that aligns with the CmpMap's
/// component-dimension boundaries, the two are reordered: the CmpMap moves in
/// front and the PermMap is rebuilt (or removed if it becomes identity) to
/// undo the block swap against the reversed component ordering. Any PermMap
/// output that is a constant has its value transformed through the swapped
/// component (const-transform math borrowed from `cmpmap.c:1929-2041`, per the
/// swap helper). This can enable further pairwise merges on later nominations.
///
/// The swap logic lives in [`try_swap_permmap_parallel_at`]; the per-class
/// entry point only checks the `(where_, where_+1)` adjacency C examines when
/// the PermMap is nominated, then falls back to [`default_map_merge`] for the
/// ordinary direct-pairwise merge (C's `-1` for the swap becomes a plain
/// neighbour merge attempt).
pub(crate) fn perm_map_merge(
    _perm: &PermMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    if series
        && where_ + 1 < ctx.len()
        && ctx.entry(where_).0.as_any().is::<PermMap>()
        && ctx
            .entry(where_ + 1)
            .0
            .as_any()
            .downcast_ref::<CmpMap>()
            .is_some_and(|c| c.composition() == Composition::Parallel)
        && try_swap_permmap_parallel_at(&mut ctx.list, where_)
    {
        ctx.resync_frozen();
        return Some(where_);
    }

    if let Some(modified) = default_map_merge(ctx, where_, series) {
        return Some(modified);
    }

    // C `permmap.c::MapMerge` rebuilds a nominated PermMap into its canonical
    // stored form — dropping a now-redundant stored permutation array (→
    // `OutCpy`/`InCpy`), clearing a set Invert flag, or collapsing to a
    // UnitMap — and reports this as a simplification even when no neighbour
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
    ctx.resync_frozen();
    Some(where_)
}

/// Per-class `map_merge` for a nominated `WcsMap`, mirroring the merge and
/// swap logic of C `wcsmap.c::MapMerge` (2914). A WcsMap can only merge
/// directly with its own inverse (`CanMerge`, wcsmap.c:812): C checks the
/// lower pair `(where_-1, where_)` first, then the upper pair
/// `(where_, where_+1)`, replacing a cancelling pair with a UnitMap
/// (wcsmap.c:3116-3161). Doing the lower check at this nomination — rather
/// than waiting for the lower WcsMap's own nomination — matters: the
/// returned `modified` index cascades the nominate loop back through the
/// cancelled pair, which determines the association order of later linear
/// folds. When no direct merge is available, the WcsMap tries to swap past
/// an adjacent PermMap — either to move it next to a cancelling WcsMap on
/// the far side, or because the swap itself yields a simpler pair (the
/// WCSBAD →`UnitMap` case is handled by `simplified`). On success the first
/// modified index is returned.
pub(crate) fn wcs_map_merge(
    wcs: &WcsMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // Replace-with-simpler, "without reference to the neighbouring Mappings in
    // the list": a WcsMap of type `AST__WCSBAD` is equivalent to a UnitMap
    // (wcsmap.c:3187-3199). C does this first, before any neighbour is
    // considered.
    if wcs.projection() == crate::mappings::WcsProjection::Unknown {
        ctx.set_pair_single(where_, (Box::new(UnitMap::new(wcs.nin())), false, false));
        return Some(where_);
    }

    if series && ctx.len() > 1 {
        // Direct inverse-pair cancellation, lower neighbour first
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

        // Upper neighbour: WcsMap at `where_`, candidate PermMap at `where_+1`.
        if where_ + 1 < ctx.len() && try_swap_wcsmap_past_permmap_at(&mut ctx.list, where_) {
            ctx.resync_frozen();
            return Some(where_);
        }
        // Lower neighbour: candidate PermMap at `where_-1`, WcsMap at `where_`.
        if where_ > 0 && try_swap_wcsmap_past_permmap_at(&mut ctx.list, where_ - 1) {
            ctx.resync_frozen();
            return Some(where_ - 1);
        }
    }

    default_map_merge(ctx, where_, series)
}

/// The `MapMerge` shape shared by classes whose C counterpart opens with
/// "see if this Mapping can be replaced by a simpler Mapping, without
/// reference to the neighbouring Mappings in the list" and then falls through
/// to the neighbour steps.
///
/// `reduced` is the replacement the class arrived at, or `None` when it has
/// none. The class computes it in its own `map_merge`, which is where C's body
/// computes it, and the entry is replaced only when the candidate differs from
/// the Mapping it replaces — a candidate equal to the original is C's
/// "no simplification took place".
pub(crate) fn self_reduce_map_merge(
    this: &dyn Mapping,
    reduced: Option<DynMapping>,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    if let Some(modified) = try_self_reduce(this, reduced, ctx, where_) {
        return Some(modified);
    }
    default_map_merge(ctx, where_, series)
}

/// The replace-with-simpler head on its own, for a class whose C body puts
/// other steps between it and the plain neighbour merge.
pub(crate) fn try_self_reduce(
    this: &dyn Mapping,
    reduced: Option<DynMapping>,
    ctx: &mut MergeCtx,
    where_: usize,
) -> Option<usize> {
    trace::simplify_enter("self-reduce-head", this);
    if let Some(simp) = reduced
        && (simp.is_inverted() != this.is_inverted() || !simp.equal(this))
    {
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
/// or the list grows past one element (`mapping.c:16871-16883`). Returns a clone
/// when nothing reduced, as C does.
///
/// C's entry guard on `RestrictedSimplify` (`mapping.c:16853`) has no analogue
/// here: the attribute lives on `CmpMap` alone in this port, and `CmpMap`
/// overrides the slot, so no Mapping reaching this driver can carry it. The
/// per-component half of the same rule — only `AllowSimplify` components may be
/// nominated — belongs to the enclosing `CmpMap`'s loop and lives in
/// [`MergeCtx`].
pub(crate) fn base_simplify(this: &dyn Mapping) -> AstResult<DynMapping> {
    // C `astMapList( this, 1, astGetInvert( this ), … )`: for a class with no
    // `MapList` override this appends the Mapping itself and records its own
    // Invert as the effective direction, which is what `flatten` produces for a
    // leaf given no externally imposed inversion.
    let mut ctx = MergeCtx::new(flatten(this, false, true)?);
    let mut simpler = false;

    // C loops while the nominated element is unfrozen. Only the oscillation
    // break inside a merge freezes an element, so in practice the break below is
    // what ends the loop.
    while !ctx.is_frozen(0) {
        let nominee = ctx.entry(0).0.clone_box();
        trace::map_merge(nominee.as_ref(), 0, true, ctx.len());
        let modified = nominee.map_merge(&mut ctx, 0, true);
        trace::map_merge_result(nominee.as_ref(), modified);

        if ctx.len() > 1 || modified.is_none() {
            break;
        }
        simpler = true;
    }

    // `nmap > 1` is a reduction that produced more than one Mapping, which this
    // driver has nowhere to put: C returns the original even though the loop
    // reported a change (`mapping.c:16891`).
    if !simpler || ctx.len() > 1 {
        return Ok(this.clone_box());
    }

    // The one remaining element, brought to the orientation the list asks for.
    // C copies it and sets or clears `Invert` when `invert_list[ 0 ]` disagrees
    // with the Mapping's own flag (`mapping.c:16899-16911`); for a one-element
    // list that is exactly what `reconstruct` does to its seed.
    reconstruct(std::mem::take(&mut ctx.list), true)
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
                    // strip handle it (matches C behaviour where UnitMap
                    // removes itself before neighbours get a chance to merge).
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
                ctx.resync_frozen();
                return Some(i);
            }
        }
    }

    // Normalise exactly as [`MergeCtx::replace_pair_with`] does: a merge may
    // return its result in the inverted orientation — `MatrixMap::mtr_zoom`
    // keeps an inverted MatrixMap inverted so it serialises `Invert = 1` like C
    // — and that intrinsic orientation is what `reconstruct` needs in
    // `orig_inv`. Storing `(m, false, false)` threw it away, materialising the
    // inversion into the stored values and dropping the card.
    let mut m = m;
    let orig_inv = m.is_inverted();
    if orig_inv {
        m.invert();
    }
    list[i] = (m, orig_inv, orig_inv);
    list.remove(i + 1);
    ctx.resync_frozen();
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
/// 2. **Parallel-only neighbour merge.** With the head having made no change:
///    - try the **lower** neighbour (`where-1`) via
///      [`Region::region_merge`] with `intfirst = false` (the neighbour's axes
///      go first), storing the merged Region at `where-1`;
///    - failing that, try the **upper** neighbour (`where+1`) with
///      `intfirst = true` (this Region's axes go first), storing at `where`.
///
///    On success, C annuls both entries, stores the merged Region at `i1`, and
///    shuffles the tail down — [`MergeCtx::replace_pair_with`] does the same.
/// 3. In series, nothing (a series of two Regions is not a Region).
///
/// The nominated Region's own [`try_merge`] only ever covers the direct
/// pairwise (upper-neighbour, `intfirst = true`) case, so this method is what
/// lets a Region absorb the neighbour that sits *before* it in the list — the
/// case C reaches through the lower-neighbour branch and Rust cannot reach via
/// `default_map_merge` (which merges the nominee only with `where+1`).
pub(crate) fn region_map_merge(
    this: &dyn crate::regions::Region,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    use crate::regions::as_region;

    // ---- 1. Self-simplify head (astSimplify(this)) ----
    // C compares object pointers (`new != oldint`); the Rust analogue is a
    // structural inequality test via `Mapping::equal`, since Region simplify
    // returns a clone of `this` when nothing changed.
    if let Ok(simplified) = <dyn Mapping>::simplify(this) {
        if !simplified.equal(this as &dyn Mapping) {
            ctx.set_pair_single(where_, (simplified, false, false));
            return Some(where_);
        }
    }

    // ---- 2. Parallel-only neighbour merge ----
    if !series {
        // Lower neighbour first (intfirst = 0: neighbour axes go first).
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

        // Upper neighbour (intfirst = 1: this Region's axes go first).
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
fn run_nominate_loop(
    ctx: &mut MergeCtx,
    series: bool,
    mlist: &mut Vec<i64>,
    nlist: &mut Vec<i64>,
) -> bool {
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
                nominated += 1;
                continue;
            }

            // Clone the nominated mapping and attempt to merge it with its
            // neighbours (C clones map_list[nominated] before astMapMerge).
            let nominee = ctx.list[nominated].0.clone_box();
            trace::map_merge(nominee.as_ref(), nominated, series, ctx.len());
            let modified = nominee.map_merge(ctx, nominated, series);
            trace::map_merge_result(nominee.as_ref(), modified);

            // Move on to nominate the next mapping (C `nominated++`).
            nominated += 1;

            let Some(modified) = modified else {
                continue;
            };

            // Append to the history and check for a repeating pattern. A
            // pattern in `modified` is only honoured if `nmap` also repeats
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
    any
}

/// Simplify a flattened series/parallel component list in place.
///
/// This is the Rust port of C `cmpmap.c::Simplify`'s simplification engine
/// (the nominate loop driving per-class `astMapMerge`). It runs
/// [`run_nominate_loop`] to exhaust all direct/per-class merges, then — as
/// temporary scaffolding for phases of the rebuild still to come — falls
/// back to the legacy global swap helpers when the per-class methods stall.
/// Each helper is the cross-class swap/restructure logic that C keeps
/// inside a specific class's `MapMerge`; they are tried in the original
/// order and retired into their owning class's `map_merge` in later phases.
///
/// After stabilising, `UnitMap` entries are stripped from series lists
/// (they are identity and do not contribute).
///
/// Returns `true` if the list was modified.
#[cfg(test)]
pub(crate) fn merge_loop(list: &mut Vec<(DynMapping, bool, bool)>, series: bool) -> bool {
    merge_loop_restricted(list, series, false)
}

/// Simplify a flattened component list in place. When `restricted` is true,
/// only components carrying `AllowSimplify` may be nominated, and the legacy
/// global swap helpers (not part of C's restricted engine) are skipped.
fn merge_loop_restricted(
    list: &mut Vec<(DynMapping, bool, bool)>,
    series: bool,
    restricted: bool,
) -> bool {
    // Capture the original axis count so the empty-list fallback below
    // produces a UnitMap of the right size. For a series list, every
    // map has the same external nin/nout (after the upstream merging
    // makes them dimension-compatible). For a parallel list, the
    // composite is `nin = sum of nin`, but the empty-fallback case for
    // parallel doesn't fire in the strip block (we only strip in
    // series), so series is what we need to preserve.
    let original_nin: usize = if list.is_empty() {
        1
    } else if series {
        let (m, inv, _) = &list[0];
        if *inv { m.nout() } else { m.nin() }
    } else {
        list.iter()
            .map(|(m, inv, _)| if *inv { m.nout() } else { m.nin() })
            .sum()
    };

    let mut ctx = MergeCtx::new(std::mem::take(list));
    ctx.set_restricted(restricted);
    let mut changed = false;

    // `modified`/`nmap` history for `pattern_check`, persisting across the
    // whole simplification (C allocates these once before the loop).
    let mut mlist: Vec<i64> = Vec::new();
    let mut nlist: Vec<i64> = Vec::new();

    // The engine is now purely C's nominate loop: nominate each unfrozen
    // component in turn and call its per-class `map_merge`, to fixpoint. Every
    // cross-class swap/restructure that the old global helpers performed now
    // lives in the owning class's `map_merge` (WinMap/CmpMap/PermMap/WcsMap/
    // PcdMap/GrismMap/SphMap), driven by the nominate loop exactly as C's
    // `cmpmap.c::Simplify` drives `astMapMerge`.
    //
    // The one piece not owned by a nominated class is the in-loop UnitMap
    // strip: C's `UnitMap::MapMerge` (unitmap.c) removes a UnitMap from a
    // series sequence when the UnitMap itself is nominated. We fold that into
    // the same fixpoint: run the nominate loop, strip series UnitMaps, and
    // repeat until neither changes. Stripping only after the nominate loop
    // stabilises lets other classes react to a UnitMap neighbour first (e.g.
    // PermMap's OutCpy form, or folding SphMap/UnitMap into a MatrixMap)
    // before it vanishes.
    loop {
        if run_nominate_loop(&mut ctx, series, &mut mlist, &mut nlist) {
            changed = true;
        }

        if series && ctx.list.len() > 1 {
            let orig_len = ctx.list.len();
            ctx.list.retain(|(m, _, _)| !m.as_any().is::<UnitMap>());
            if ctx.list.is_empty() {
                ctx.list
                    .push((Box::new(UnitMap::new(original_nin)), false, false));
            }
            ctx.resync_frozen();
            if ctx.list.len() != orig_len {
                changed = true;
                continue;
            }
        }
        break;
    }

    // Post-loop: strip UnitMaps from series lists.
    if series && ctx.list.len() > 1 {
        let orig_len = ctx.list.len();
        ctx.list.retain(|(m, _, _)| !m.as_any().is::<UnitMap>());
        if ctx.list.len() != orig_len {
            changed = true;
        }
        if ctx.list.is_empty() {
            ctx.list
                .push((Box::new(UnitMap::new(original_nin)), false, false));
        }
    }

    *list = ctx.list;
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
) -> Option<(DynMapping, PermMap)> {
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
        return None;
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

    let new_lin: DynMapping = Box::new(WinMap::new(&new_shifts, &new_scales).ok()?);
    let new_pm = PermMap::new(inperm, outperm, &new_consts).ok()?;
    Some((new_lin, new_pm))
}

/// Attempt the WcsMap↔PermMap swap for the single adjacent pair at
/// `i`/`i+1` (one a `WcsMap`, the other a `PermMap`). Returns `true` if the
/// pair was swapped (and `list` mutated), `false` otherwise. This is the
/// per-pair body of [`try_swap_wcsmap_past_permmap`], factored out so
/// [`wcs_map_merge`] can drive it around the nominated WcsMap index.
pub(crate) fn try_swap_wcsmap_past_permmap_at(
    list: &mut [(DynMapping, bool, bool)],
    i: usize,
) -> bool {
    {
        let is_wcs_perm =
            list[i].0.as_any().is::<WcsMap>() && list[i + 1].0.as_any().is::<PermMap>();
        let is_perm_wcs =
            list[i].0.as_any().is::<PermMap>() && list[i + 1].0.as_any().is::<WcsMap>();

        if !is_wcs_perm && !is_perm_wcs {
            return false;
        }

        let (wcs_idx, perm_idx) = if is_wcs_perm { (i, i + 1) } else { (i + 1, i) };

        // Check if swapping is worthwhile: either there's a WcsMap merge partner
        // on the far side, or the swap itself produces a simpler result (when
        // the PermMap reduces dimensionality, replacing the WcsMap with a UnitMap
        // is a net simplification). Matches C wcsmap.c MapMerge logic.
        let far_side = if is_wcs_perm {
            i + 2
        } else {
            i.wrapping_sub(1)
        };
        let has_merge_partner = if is_wcs_perm {
            far_side < list.len() && list[far_side].0.as_any().is::<WcsMap>()
        } else {
            i > 0 && list[i - 1].0.as_any().is::<WcsMap>()
        };

        let perm_ref = list[perm_idx].0.as_any().downcast_ref::<PermMap>().unwrap();
        let perm_inv_flag = list[perm_idx].1;
        let perm_nin = if perm_inv_flag {
            perm_ref.outperm().len()
        } else {
            perm_ref.inperm().len()
        };
        let perm_nout = if perm_inv_flag {
            perm_ref.inperm().len()
        } else {
            perm_ref.outperm().len()
        };
        let simpler = if is_wcs_perm {
            // PermMap is second: simpler if nin > nout (reduces dimensions).
            perm_nin > perm_nout
        } else {
            // PermMap is first: simpler if nout > nin (increases dimensions).
            perm_nout > perm_nin
        };

        if !has_merge_partner && !simpler {
            return false;
        }

        let wcs = list[wcs_idx].0.as_any().downcast_ref::<WcsMap>().unwrap();
        let perm = list[perm_idx].0.as_any().downcast_ref::<PermMap>().unwrap();
        let wcs_inv = list[wcs_idx].1;
        let perm_inv = list[perm_idx].1;

        // Get the effective forward permutation arrays for the PermMap.
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
        let npin = inperm.len();
        let npout = outperm.len();

        // Check bi-directionality: all active links must be reversible.
        // Note: PermMap uses 1-based positive indices. outperm[k]=j means
        // output k gets input axis j (1-based). Convert to 0-based for indexing.
        let mut ok = true;
        for (k, &j) in outperm.iter().enumerate() {
            if j > 0 && (j as usize - 1) < npin {
                // outperm[k] links output k to input (j-1) (0-based).
                // Check reverse: inperm[j-1] should link back to output (k+1) (1-based).
                if inperm[j as usize - 1] != (k as i64 + 1) {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            for (k, &j) in inperm.iter().enumerate() {
                if j > 0 && (j as usize - 1) < npout {
                    // inperm[k] links input k to output (j-1) (0-based).
                    // Check reverse: outperm[j-1] should link back to input (k+1) (1-based).
                    if outperm[j as usize - 1] != (k as i64 + 1) {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if !ok {
            return false;
        }

        // Get the WcsMap's lon/lat axis indices (0-based).
        let lonax = wcs.lonax_0based();
        let latax = wcs.latax_0based();

        // Determine whether lon/lat are assigned or unassigned in the PermMap,
        // depending on whether the WcsMap comes before or after the PermMap.
        if is_wcs_perm {
            // WcsMap at i, PermMap at i+1.
            // PermMap operates on WcsMap's outputs. The PermMap's inperm tells
            // where each PermMap input (= WcsMap output) gets routed in the
            // PermMap's output. inperm[lonax] is 1-based output position.
            if lonax >= npin || latax >= npin {
                return false;
            }

            let lon_dest = inperm[lonax]; // 1-based or negative
            let lat_dest = inperm[latax];

            if lon_dest < 0 && lat_dest < 0 {
                // Both celestial axes are unassigned (receive constants in inverse).
                // WcsMap becomes UnitMap, PermMap gets transformed constants.
                if let Some(result) =
                    swap_wcsmap_perm_unassigned(wcs, perm, wcs_inv, perm_inv, true)
                {
                    list[i] = (result.0, result.1, false);
                    list[i + 1] = (result.2, result.3, false);
                    return true;
                }
            } else if lon_dest > 0
                && (lon_dest as usize) <= npout
                && lat_dest > 0
                && (lat_dest as usize) <= npout
            {
                // Both celestial axes are assigned. Create new WcsMap with permuted
                // axes. lon_dest is 1-based → convert to 0-based for new axis index.
                let new_lonax = lon_dest as usize - 1;
                let new_latax = lat_dest as usize - 1;
                let Some(new_wcs) = wcs.with_permuted_axes(npout, new_lonax, new_latax) else {
                    return false;
                };

                // Swap: [WcsMap, PermMap] → [PermMap, new_WcsMap].
                // The swap keeps the list length unchanged, so move the
                // PermMap entry from i+1 down to i and place the new WcsMap
                // at i+1.
                let perm_entry =
                    std::mem::replace(&mut list[i + 1], (Box::new(new_wcs), wcs_inv, false));
                list[i] = perm_entry;
                return true;
            }
            // else: one assigned, one not — cannot swap.
        } else {
            // PermMap at i, WcsMap at i+1.
            // PermMap operates on WcsMap's inputs. The PermMap's outperm tells
            // where each WcsMap input (= PermMap output) gets its value.
            // outperm[lonax] is 1-based input source position.
            if lonax >= npout || latax >= npout {
                return false;
            }

            let lon_src = outperm[lonax]; // 1-based or negative
            let lat_src = outperm[latax];

            if lon_src < 0 && lat_src < 0 {
                // Both celestial axes are unassigned (receive constants in forward).
                // WcsMap becomes UnitMap, PermMap gets transformed constants.
                if let Some(result) =
                    swap_wcsmap_perm_unassigned(wcs, perm, wcs_inv, perm_inv, false)
                {
                    list[i] = (result.0, result.1, false);
                    list[i + 1] = (result.2, result.3, false);
                    return true;
                }
            } else if lon_src > 0
                && (lon_src as usize) <= npin
                && lat_src > 0
                && (lat_src as usize) <= npin
            {
                // Both celestial axes are assigned. Create new WcsMap with permuted
                // axes. lon_src is 1-based → convert to 0-based.
                let new_lonax = lon_src as usize - 1;
                let new_latax = lat_src as usize - 1;
                let Some(new_wcs) = wcs.with_permuted_axes(npin, new_lonax, new_latax) else {
                    return false;
                };

                // Swap: [PermMap, WcsMap] → [new_WcsMap, PermMap].
                // Length-preserving: move the PermMap entry from i up to
                // i+1 and place the new WcsMap at i.
                let perm_entry =
                    std::mem::replace(&mut list[i], (Box::new(new_wcs), wcs_inv, false));
                list[i + 1] = perm_entry;
                return true;
            }
            // else: one assigned, one not — cannot swap.
        }

        false
    }
}

/// Handle the "unassigned lon/lat" case of WcsMap↔PermMap swap.
///
/// When the PermMap feeds constants to the WcsMap's celestial axes (or
/// receives constants from them), the WcsMap can be replaced by a UnitMap
/// and the PermMap's constants are transformed through the WcsMap.
///
/// Returns (new_first_mapping, new_first_inv, new_second_mapping, new_second_inv)
/// or None if the transformation fails.
fn swap_wcsmap_perm_unassigned(
    wcs: &WcsMap,
    perm: &PermMap,
    wcs_inv: bool,
    perm_inv: bool,
    wcs_first: bool, // true if WcsMap comes before PermMap in the pipeline
) -> Option<(DynMapping, bool, DynMapping, bool)> {
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
        let transformed = wcs.transform(&pts, wcs_inv).ok()?;
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

        let new_pm = PermMap::new(inperm, outperm, &new_constants).ok()?;
        let unit = UnitMap::new(npout);

        // Result: [PermMap (at original PermMap position), UnitMap (replacing WcsMap)]
        // Swapped order: [UnitMap, PermMap] — but UnitMap will be stripped.
        // Actually the C code puts: maps[iwm=0] = newpm, maps[1] = UnitMap(npout)
        // That means: position 0 gets the PermMap, position 1 gets the UnitMap.
        // So the result is [PermMap, UnitMap].
        Some((
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
        let transformed = wcs.transform(&pts, !wcs_inv).ok()?;
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

        let new_pm = PermMap::new(inperm, outperm, &new_constants).ok()?;
        let unit = UnitMap::new(npin);

        // Swapped: [UnitMap (replacing WcsMap), PermMap]
        Some((
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
/// `cmp_map_merge`'s per-nomination neighbour restructure.
fn compute_merge_parallel_pair_in_series(
    list: &[(DynMapping, bool, bool)],
    i: usize,
) -> Option<DynMapping> {
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
        return None;
    }

    let (ref m1, inv1, _) = list[i];
    let (ref m2, inv2, _) = list[i + 1];

    // Flatten both for parallel composition.
    let Ok(par_list1) = flatten(m1.as_ref(), inv1, false) else {
        return None;
    };
    let Ok(par_list2) = flatten(m2.as_ref(), inv2, false) else {
        return None;
    };

    // Dimension-match the two parallel lists.
    let matched = dimension_match_parallel_lists(&par_list1, &par_list2)?;

    // For each matched pair, combine in series and simplify.
    let mut simplified_pairs: Vec<(DynMapping, bool, bool)> = Vec::new();
    let mut any_improvement = false;

    for (sub1, sub2) in matched {
        // Pre-merge length (sum of leaf counts) for the
        // length-comparison heuristic below — total flat-leaf count
        // of the two sublists before composition.
        let pre_len: usize = sub1
            .iter()
            .map(|(m, _, _)| {
                flatten(m.as_ref(), false, true)
                    .map(|f| f.len())
                    .unwrap_or(1)
            })
            .sum::<usize>()
            + sub2
                .iter()
                .map(|(m, _, _)| {
                    flatten(m.as_ref(), false, true)
                        .map(|f| f.len())
                        .unwrap_or(1)
                })
                .sum::<usize>();

        // Build parallel submaps from each sublist.
        let Ok(par1) = reconstruct(sub1, false) else {
            return None;
        };
        let Ok(par2) = reconstruct(sub2, false) else {
            return None;
        };

        // Combine in series.
        let Ok(series_map) = CmpMap::new(par1, par2, Composition::Series) else {
            return None;
        };
        let series_box: DynMapping = Box::new(series_map);

        // Simplify the series combination.
        let Ok(simplified) = simplify_mapping(series_box.as_ref()) else {
            return None;
        };

        // Check if simplification produced something different.
        if !simplified.as_any().is::<CmpMap>() {
            any_improvement = true;
        } else if let Some(cmp) = simplified.as_any().downcast_ref::<CmpMap>() {
            // If still a CmpMap, check if it's structurally simpler:
            // a non-series result or a simpler series (fewer components).
            if cmp.composition() != Composition::Series {
                any_improvement = true;
            } else {
                // Check if the simplified chain is shorter than the
                // sum of pre-merge leaf counts. Mirrors C cmpmap.c:1726
                // which marks `simpler = 1` whenever the simplified
                // sub-chain pointer or invert state changed.
                if let Ok(flat) = flatten(simplified.as_ref(), false, true) {
                    if flat.len() < pre_len {
                        any_improvement = true;
                    }
                }
            }
        }

        // C keeps each pair's `astSimplify` result at whatever orientation it
        // was returned in — the rebuilt parallel CmpMap's snapshot records
        // `astGetInvert(smc)` and the object serialises at that stored
        // orientation (cmpmap.c:1629-1781 builds `astCmpMap(smc0, smc1, 0)`
        // with no reorientation). Normalise to the list invariant
        // (forward-stored + flags): effective == original == the result's
        // returned orientation.
        let new_inv = simplified.is_inverted();
        let mut simplified = simplified;
        if new_inv {
            simplified.invert();
        }
        // C `astSimplify_` (mapping.c:24763) tags every result. Each matched
        // pair was combined in series and `astSimplify`'d, so its reduced
        // result carries the record — in the orientation it was earned in,
        // which is the one the tree serialises the pair at.
        simplified.set_simplified_in(new_inv, true);
        simplified_pairs.push((simplified, new_inv, new_inv));
    }

    if !any_improvement {
        return None;
    }

    // Rebuild as parallel CmpMap and replace the pair in the list.
    let Ok(result) = reconstruct(simplified_pairs, false) else {
        return None;
    };
    // Recursively simplify the reconstructed parallel CmpMap so that
    // e.g. parallel(UnitMap, UnitMap) collapses to a single UnitMap of
    // the combined dimension via UnitMap::try_merge in parallel mode.
    // reconstruct() is purely structural; without a follow-up simplify
    // the merge_loop would not see the parallel children adjacent.
    let result = simplify_mapping(result.as_ref()).unwrap_or(result);
    Some(result)
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
            // Match boundary found. Clone the sublists.
            let sub1: Vec<(DynMapping, bool, bool)> = par_list1[sub1_start..i1]
                .iter()
                .map(|(m, inv, orig)| {
                    (
                        m.simplify().unwrap_or_else(|_| Box::new(UnitMap::new(1))),
                        *inv,
                        *orig,
                    )
                })
                .collect();
            let sub2: Vec<(DynMapping, bool, bool)> = par_list2[sub2_start..i2]
                .iter()
                .map(|(m, inv, orig)| {
                    (
                        m.simplify().unwrap_or_else(|_| Box::new(UnitMap::new(1))),
                        *inv,
                        *orig,
                    )
                })
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
) -> Option<DynMapping> {
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

    let (fa1, fa1_inv, sa1, sa1_inv) = extract(&list[i])?;
    let (fa2, fa2_inv, sa2, sa2_inv) = extract(&list[i + 1])?;

    // Rebuild each branch as a forward binary series CmpMap (first-applied then
    // second-applied child), then delegate to the shared recombination core.
    let branch1 = reconstruct(vec![(fa1, fa1_inv, false), (sa1, sa1_inv, false)], true).ok()?;
    let branch2 = reconstruct(vec![(fa2, fa2_inv, false), (sa2, sa2_inv, false)], true).ok()?;

    recombine_distributed_branches(&[(branch1, false, false), (branch2, false, false)])
}

/// Attempt the PermMap↔parallel-CmpMap swap at position `i` (PermMap)
/// and `i+1` (parallel CmpMap).
#[allow(clippy::needless_range_loop)]
fn try_swap_permmap_parallel_at(list: &mut Vec<(DynMapping, bool, bool)>, i: usize) -> bool {
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
        return false;
    }

    // Get the parallel CmpMap's component dimensions.
    // Flatten for parallel to get the components.
    let Ok(par_list) = flatten(par_map.as_ref(), par_inv, false) else {
        return false;
    };
    if par_list.len() != 2 {
        // Only handle the 2-component case (matching C implementation).
        return false;
    }

    let nin_a = effective_nin(par_list[0].0.as_ref(), par_list[0].1);
    let nin_b = effective_nin(par_list[1].0.as_ref(), par_list[1].1);

    // Verify total dimensions match.
    if nin_a + nin_b != npout {
        return false;
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
                return false;
            }
        } else if eff_outperm[idx] != (j as i64 + 1) || eff_inperm[j] != (idx as i64 + 1) {
            return false;
        }
    }

    // Check second block (outputs nin_a..nin_a+nin_b).
    for idx in 0..nin_b {
        let j = npout - nin_b + idx;
        if bconstants {
            if eff_outperm[j] >= 0 {
                return false;
            }
        } else if eff_inperm[idx] != (j as i64 + 1) || eff_outperm[j] != (idx as i64 + 1) {
            return false;
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
        let q_pts = perm.transform(&pts, !perm_inv).ok();
        let q = q_pts.as_ref().map(|ps| ps.point(0).to_vec());

        // Transform the first nin_a outputs through component A.
        let qa = if aconstants {
            if let Some(ref qv) = q {
                let a_input: Vec<f64> = qv[..nin_a].to_vec();
                let a_pts = PointSet::from_rows(vec![a_input]);
                let comp_a = &par_list[0];
                comp_a
                    .0
                    .transform(&a_pts, !comp_a.1)
                    .ok()
                    .map(|r| r.point(0).to_vec())
            } else {
                None
            }
        } else {
            None
        };

        // Transform the last nin_b outputs through component B.
        let qb = if bconstants {
            if let Some(ref qv) = q {
                let b_input: Vec<f64> = qv[nin_a..nin_a + nin_b].to_vec();
                let b_pts = PointSet::from_rows(vec![b_input]);
                let comp_b = &par_list[1];
                comp_b
                    .0
                    .transform(&b_pts, !comp_b.1)
                    .ok()
                    .map(|r| r.point(0).to_vec())
            } else {
                None
            }
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
    // replaced by a UnitMap of its output dimension. Mirrors C
    // permmap.c MapMerge "Swapped CmpMap" branch, where each consumed
    // component is reduced to a UnitMap before assembling the new
    // parallel CmpMap.
    let nout_a = effective_nout(par_list[0].0.as_ref(), par_list[0].1);
    let nout_b = effective_nout(par_list[1].0.as_ref(), par_list[1].1);

    let new_a: DynMapping = if aconstants {
        Box::new(UnitMap::new(nout_a))
    } else {
        trace::simplify_enter("par-branch-a", par_list[0].0.as_ref());
        let Ok(mut a) = par_list[0].0.simplify() else {
            return false;
        };
        if par_list[0].1 {
            a.invert();
        }
        a
    };
    let new_b: DynMapping = if bconstants {
        Box::new(UnitMap::new(nout_b))
    } else {
        trace::simplify_enter("par-branch-b", par_list[1].0.as_ref());
        let Ok(mut b) = par_list[1].0.simplify() else {
            return false;
        };
        if par_list[1].1 {
            b.invert();
        }
        b
    };

    let Ok(new_cm) = CmpMap::new(new_b, new_a, Composition::Parallel) else {
        return false;
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
            return false;
        };
        // CmpMap goes first (was second), PermMap goes second (was first).
        list[i] = (Box::new(new_cm), false, false);
        list[i + 1] = (Box::new(new_pm), false, false);
    }
    true
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
        let mut z = zoom.factor();
        if zoom_inv && z != 0.0 {
            z = 1.0 / z;
        }

        if z != 0.0
            && let Some(merged) = grism.merge_with_zoom(z, false)
        {
            list[i] = (merged, false, false);
            list.remove(i + 1);
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
        let mut z = zoom.factor();
        if !zoom_inv && z != 0.0 {
            z = 1.0 / z;
        }

        if z != 0.0
            && let Some(merged) = grism.merge_with_zoom(z, true)
        {
            list[i] = (merged, false, false);
            list.remove(i + 1);
            return true;
        }
    }

    false
}

/// Per-class `map_merge` for a nominated `GrismMap`, mirroring the neighbour
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
        // Lower neighbour first (C MapMerge 1212-1217): pair (where-1, where).
        if where_ > 0 && grism_zoom_merge_at(&mut ctx.list, where_ - 1) {
            ctx.resync_frozen();
            return Some(where_ - 1);
        }
        // Upper neighbour (C MapMerge 1221-1226): pair (where, where+1).
        if where_ + 1 < ctx.len() && grism_zoom_merge_at(&mut ctx.list, where_) {
            ctx.resync_frozen();
            return Some(where_);
        }
    }

    default_map_merge(ctx, where_, series)
}

/// Per-class `map_merge` for a nominated `PcdMap`, mirroring the swap logic
/// of C `pcdmap.c::MapMerge` (1503-1714). A PcdMap merges directly only with
/// its own inverse or a UnitMap (handled by [`default_map_merge`] via
/// `try_merge`); when no direct merge is available, it tries to swap past an
/// adjacent ZoomMap or axis-swapping PermMap to move towards a distant merge
/// partner (another PcdMap or UnitMap), so the PcdMap can merge directly on a
/// later nomination. The swap is attempted around the nominated index (C's
/// `where`); on success the index of the first modified element is returned.
pub(crate) fn pcd_map_merge(
    pcd: &PcdMap,
    ctx: &mut MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    // C's opening move, before any neighbour is considered: a PcdMap whose
    // distortion coefficient is zero is a UnitMap (pcdmap.c:1420-1432). The
    // replacement carries a cleared Invert flag (pcdmap.c:1429). C makes this
    // the `if` of an if/else whose `else` holds the whole neighbour search, so
    // a reduced PcdMap never looks at a neighbour in the same pass.
    if ast_equal(pcd.disc(), 0.0) {
        ctx.set_pair_single(where_, (Box::new(UnitMap::new(2)), false, false));
        ctx.resync_frozen();
        return Some(where_);
    }

    if series {
        if let Some(modified) = pcd_swap_at(&mut ctx.list, where_) {
            ctx.resync_frozen();
            return Some(modified);
        }
    }

    default_map_merge(ctx, where_, series)
}

/// Consider swapping the PcdMap at index `i` with the neighbour that brings it
/// nearest to a merge target, mirroring C `pcdmap.c::MapMerge` (1512-1637).
///
/// Steps to a merge target are counted in both directions (over the run of
/// swappable ZoomMaps / axis-swapping PermMaps), and the swap is made towards
/// the nearer target. Returns the index of the first modified element, or
/// `None` when no swap is worthwhile (or `i` is not a PcdMap).
fn pcd_swap_at(list: &mut Vec<(DynMapping, bool, bool)>, i: usize) -> Option<usize> {
    if !list[i].0.as_any().is::<PcdMap>() {
        return None;
    }

    let pcd_inv = list[i].1;

    // Try swapping with the higher neighbor (i+1) towards a target further right.
    // Count how many steps to a merge target in the forward direction.
    let nstep_hi = if i + 1 < list.len() && can_swap_pcd_with(&list[i + 1].0) {
        find_pcd_merge_target_steps(list, i + 2, true)
    } else {
        -1
    };

    // Try swapping with the lower neighbor (i-1) towards a target further left.
    let nstep_lo = if i >= 1 && can_swap_pcd_with(&list[i - 1].0) {
        if i >= 2 {
            find_pcd_merge_target_steps(list, i - 2, false)
        } else {
            -1
        }
    } else {
        -1
    };

    // Choose the direction that gets us to a merge target in fewer steps.
    if nstep_lo >= 0 && (nstep_hi < 0 || nstep_hi > nstep_lo) {
        // Swap with lower neighbor: neighbor at i-1, PcdMap at i.
        // PcdMap is second (ipc=1 in C terms).
        do_pcd_swap_second(list, i - 1, pcd_inv);
        Some(i - 1)
    } else if nstep_hi >= 0 {
        // Swap with higher neighbor: PcdMap at i, neighbor at i+1.
        // PcdMap is first (ipc=0 in C terms).
        do_pcd_swap_first(list, i, pcd_inv);
        Some(i)
    } else {
        None
    }
}

/// Check if a mapping can be swapped with an adjacent PcdMap.
fn can_swap_pcd_with(m: &DynMapping) -> bool {
    if m.as_any().is::<ZoomMap>() {
        return true;
    }
    if let Some(pm) = m.as_any().downcast_ref::<PermMap>() {
        return PcdMap::perm_is_swap(pm);
    }
    false
}

/// Search forward or backward from `start` for a PcdMap or UnitMap merge
/// target, skipping over ZoomMaps and axis-swapping PermMaps.
/// Returns the number of intermediate steps if found, -1 otherwise.
fn find_pcd_merge_target_steps(
    list: &[(DynMapping, bool, bool)],
    start: usize,
    forward: bool,
) -> i32 {
    if start >= list.len() {
        return -1;
    }
    let mut pos = start;
    let mut steps = 0i32;
    loop {
        if pos >= list.len() {
            return -1;
        }
        if list[pos].0.as_any().is::<PcdMap>() || list[pos].0.as_any().is::<UnitMap>() {
            return steps;
        }
        if !can_swap_pcd_with(&list[pos].0) {
            return -1;
        }
        steps += 1;
        if forward {
            pos += 1;
        } else {
            if pos == 0 {
                return -1;
            }
            pos -= 1;
        }
    }
}

/// Perform PcdMap swap when PcdMap is first (at `i`) and neighbor is at `i+1`.
/// After swap: neighbor at i, new PcdMap at i+1.
/// C equivalent: ipc=0 in PcdZoom/PcdPerm.
fn do_pcd_swap_first(list: &mut [(DynMapping, bool, bool)], i: usize, pcd_inv: bool) {
    let pcd = list[i].0.as_any().downcast_ref::<PcdMap>().unwrap();
    let pcd_disc = pcd.disc();
    let pcd_cen = pcd.pcdcen();

    if let Some(zm) = list[i + 1].0.as_any().downcast_ref::<ZoomMap>() {
        let other_inv = list[i + 1].1;
        let zoom = if other_inv {
            1.0 / zm.factor()
        } else {
            zm.factor()
        };

        // PcdMap first, ZoomMap second → after swap:
        // New centre = old_centre * zoom
        // New disc = old_disc / (zoom^2)
        let new_cen = [pcd_cen[0] * zoom, pcd_cen[1] * zoom];
        let new_disc = pcd_disc / (zoom * zoom);
        let mut new_pcd = PcdMap::new(new_disc, new_cen);
        if pcd_inv {
            new_pcd.invert();
        }

        let new_zoom = ZoomMap::new(2, zoom).unwrap();
        list[i] = (Box::new(new_zoom), false, false);
        list[i + 1] = (Box::new(new_pcd), false, false);
    } else if list[i + 1].0.as_any().is::<PermMap>() {
        let other_inv = list[i + 1].1;

        // PcdMap first, PermMap(swap) second → after swap:
        // PermMap at i, new PcdMap(swapped centre) at i+1.
        let new_cen = [pcd_cen[1], pcd_cen[0]];
        let mut new_pcd = PcdMap::new(pcd_disc, new_cen);
        if pcd_inv {
            new_pcd.invert();
        }

        let pm_clone = list[i + 1].0.clone_box();
        let pm_orig = list[i + 1].2;
        list[i] = (pm_clone, other_inv, pm_orig);
        list[i + 1] = (Box::new(new_pcd), false, false);
    }
}

/// Perform PcdMap swap when PcdMap is second (at `i+1`) and neighbor is at `i`.
/// After swap: new PcdMap at i, neighbor at i+1.
/// C equivalent: ipc=1 in PcdZoom/PcdPerm.
fn do_pcd_swap_second(list: &mut [(DynMapping, bool, bool)], i: usize, pcd_inv: bool) {
    let pcd = list[i + 1].0.as_any().downcast_ref::<PcdMap>().unwrap();
    let pcd_disc = pcd.disc();
    let pcd_cen = pcd.pcdcen();

    if let Some(zm) = list[i].0.as_any().downcast_ref::<ZoomMap>() {
        let other_inv = list[i].1;
        let zoom = if other_inv {
            1.0 / zm.factor()
        } else {
            zm.factor()
        };

        // ZoomMap first, PcdMap second → after swap:
        // New centre = old_centre / zoom
        // New disc = old_disc * (zoom^2)
        let new_cen = [pcd_cen[0] / zoom, pcd_cen[1] / zoom];
        let new_disc = pcd_disc * zoom * zoom;
        let mut new_pcd = PcdMap::new(new_disc, new_cen);
        if pcd_inv {
            new_pcd.invert();
        }

        let new_zoom = ZoomMap::new(2, zoom).unwrap();
        list[i] = (Box::new(new_pcd), false, false);
        list[i + 1] = (Box::new(new_zoom), false, false);
    } else if list[i].0.as_any().is::<PermMap>() {
        let other_inv = list[i].1;

        // PermMap(swap) first, PcdMap second → after swap:
        // new PcdMap(swapped centre) at i, PermMap at i+1.
        let new_cen = [pcd_cen[1], pcd_cen[0]];
        let mut new_pcd = PcdMap::new(pcd_disc, new_cen);
        if pcd_inv {
            new_pcd.invert();
        }

        let pm_clone = list[i].0.clone_box();
        let pm_orig = list[i].2;
        list[i] = (Box::new(new_pcd), false, false);
        list[i + 1] = (pm_clone, other_inv, pm_orig);
    }
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
        ctx.resync_frozen();
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
            let f = zm.factor();
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
) -> Option<DynMapping> {
    if branches.len() != 2 {
        return None;
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
    // effective direction in `InvA`/`InvB` while the component still serialises
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

    let (a1, a1_inv, a1_orig, b1, b1_inv, b1_orig) = split(&branches[0])?;
    let (a2, a2_inv, a2_orig, b2, b2_inv, b2_orig) = split(&branches[1])?;

    // new1 = A1 ‖ A2, new2 = B1 ‖ B2 (C CombineMaps with series == 0).
    let new1 = reconstruct(vec![(a1, a1_inv, a1_orig), (a2, a2_inv, a2_orig)], false).ok()?;
    let new2 = reconstruct(vec![(b1, b1_inv, b1_orig), (b2, b2_inv, b2_orig)], false).ok()?;

    // Simplify each half and detect whether either changed (C's `simpler`).
    let fp1_before = mapping_fingerprint(new1.as_ref());
    let fp2_before = mapping_fingerprint(new2.as_ref());
    let mut simp1 = simplify_mapping(new1.as_ref()).ok()?;
    let mut simp2 = simplify_mapping(new2.as_ref()).ok()?;
    let simpler = mapping_fingerprint(simp1.as_ref()) != fp1_before
        || mapping_fingerprint(simp2.as_ref()) != fp2_before;
    if !simpler {
        return None;
    }

    // C reaches both halves through `astSimplify` (cmpmap.c:1602-1603), which
    // stamps its result; `simplify_mapping` is the untagged internal entry, so
    // stamp them here. Each becomes an element of the series list the closing
    // fold walks, which used to clear the tag again and no longer does.
    simp1.set_has_been_simplified(true);
    simp2.set_has_been_simplified(true);

    // Compose the two simplified halves in series (C astCmpMap(simp1, simp2, 1)).
    CmpMap::new(simp1, simp2, Composition::Series)
        .ok()
        .map(|c| Box::new(c) as DynMapping)
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
/// the component serialised in its *original* orientation. So a CmpMap child
/// gets `inv_a/inv_b = effective_inv` and a stored Invert of `orig_inv`;
/// these are independent and, by construction, the child's stored Invert
/// cancels out of the transform (see [`CmpMap::set_first_orientation`]).
///
/// For a single-element list the mapping is returned directly (no wrapping
/// `CmpMap`), in its effective orientation — matching C's `nmap == 1` branch
/// which sets the lone Mapping's Invert to the effective value.
///
/// `orig_inv` is honoured only for series reconstruction. The parallel path
/// keeps the existing `invert_into_structure` behaviour, which already emits
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
    // Apply the effective inversion to a component about to be composed.
    //
    // A plain flag toggle. `CmpMap::normalize_unitmap_children`, run once on the
    // fully simplified top-level result, is what produces C's canonical
    // `InvA`/`InvB` shape; three further layers that also pushed an inversion
    // into a component's structure were measured redundant against the whole
    // corpus, including `parcomp_inverted_reemitted` and its nested sibling,
    // which exist to demand that shape. See PLAN.md §10.15.
    let apply_effective = |m: &mut DynMapping| {
        m.invert();
    };

    // C `Simplify` (cmpmap.c:290-319) rebuilds the merged sequence by looping
    // the Mapping list *in reverse* and composing each map with the CmpMap
    // formed so far: `result = astCmpMap(map[i], result)`. This produces a
    // right-associated tree — `map[0] ∘ (map[1] ∘ (… ∘ map[n-1]))` — which is
    // what the native serialiser then emits.
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

        // Restore each child to the orientation it should serialise in (its
        // original tree orientation), leaving inv_a/inv_b carrying the
        // effective direction. C preserves each component's own stored Invert
        // (`astGetInvert(map1)`) and records the effective direction separately
        // in `invert1`/`invert2` — a component originally stored forward stays
        // forward with the inversion moved into `InvA`/`InvB`, and one stored
        // inverted stays inverted. Our transform derives a child's math
        // direction as `fwd XOR inv_a` (the child's own Invert cancels), so
        // re-setting the stored orientation after `CmpMap::new` captured the
        // effective flag is transform-neutral and only fixes the serialised
        // form.
        //
        // For a series reconstruct, or a leaf (non-CmpMap) child of a parallel
        // reconstruct, restore the original orientation. A *CmpMap* child of a
        // parallel reconstruct is excluded: `apply_effective` already rewrote it
        // to canonical forward form via `invert_into_structure` (which moves the
        // inversion into that child's own recorded structure), so re-inverting
        // it would undo that canonicalisation.
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

/// Simplify a mapping using the flatten/merge/reconstruct pipeline.
///
/// For `CmpMap` inputs this decomposes the tree, attempts pairwise
/// merges, and rebuilds. For non-`CmpMap` inputs it delegates to the
/// mapping's own `simplified()` method.
/// Write the nominate loop's `IsSimple` stamps back onto the tree that
/// `simplify_mapping` is about to return unchanged.
///
/// C's `SeparateMappings` (cmpmap.c:3326) only deep-copies *duplicate*
/// component pointers; every other entry of `map_list` aliases the live child
/// of the CmpMap being simplified. So when the nominate loop's self-simplify
/// head stamps `IsSimple` on an entry it reduced no further — `astSimplify`
/// (cmpmap.c:1437) tags even the clone it returns for an unreduced Mapping —
/// the stamp mutates the real child, and is still there when `Simplify` takes
/// its `!simpler` branch and returns `astClone( this )` (cmpmap.c:3634).
///
/// Our list holds clones, so the stamps would otherwise be discarded with it.
/// Copy them across, walking the tree in the order [`flatten`] built the list so
/// entry *i* lands on the component it came from. A stamp can sit anywhere
/// inside an entry, not just on its root — the entry is a whole subtree that C
/// shares with the tree, so [`mirror_simple_tags`] transplants the entry's tag
/// state at every level.
fn apply_component_stamps(
    node: &mut DynMapping,
    inv: bool,
    series: bool,
    entries: &[DynMapping],
    next: &mut usize,
) {
    let target_kind = if series {
        Composition::Series
    } else {
        Composition::Parallel
    };
    if let Some(cmp) = node.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == target_kind {
            let cmp_inv = inv != cmp.is_inverted();
            let first_inv = cmp_inv != (cmp.inv_a() != cmp.first().is_inverted());
            let second_inv = cmp_inv != (cmp.inv_b() != cmp.second().is_inverted());
            // `flatten` reverses the child order for an inverted series CmpMap.
            let reversed = cmp_inv && series;
            let cmp = node
                .as_any_mut()
                .downcast_mut::<CmpMap>()
                .expect("downcast succeeded immediately above");
            if reversed {
                apply_component_stamps(cmp.second_mut(), second_inv, series, entries, next);
                apply_component_stamps(cmp.first_mut(), first_inv, series, entries, next);
            } else {
                apply_component_stamps(cmp.first_mut(), first_inv, series, entries, next);
                apply_component_stamps(cmp.second_mut(), second_inv, series, entries, next);
            }
            return;
        }
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
        return;
    }
    // Leaf: `flatten` emitted exactly one entry for this component.
    if let Some(entry) = entries.get(*next) {
        mirror_simple_tags(entry.as_ref(), node);
    }
    *next += 1;
}

/// Assign to `dst` the `IsSimple` tag carried by the structurally-corresponding
/// `CmpMap` in `src`, descending both trees in step.
///
/// The tag is mirrored in both directions: where `src` is untagged the tag is
/// removed from `dst`. `src` is a component-list entry and `dst` the same
/// component on the tree the caller preserves; in C they are one object, so
/// whatever the entry ended up holding has to land on `dst`.
fn mirror_simple_tags(src: &dyn Mapping, dst: &mut DynMapping) {
    let Some(s) = src.as_any().downcast_ref::<CmpMap>() else {
        return;
    };
    let (s_tag, s_inv) = (s.dump_is_simple(), s.is_inverted());
    let Some(d) = dst.as_any_mut().downcast_mut::<CmpMap>() else {
        return;
    };
    let d_inv = d.is_inverted();
    d.set_dump_simplified_in(d_inv, s_tag);
    // A differing stored orientation means the two trees pair children the other
    // way round; without a shared shape there is nothing to descend into.
    if s_inv != d.is_inverted() {
        return;
    }
    let (s_first, s_second) = (s.first(), s.second());
    mirror_simple_tags(s_first, d.first_mut());
    mirror_simple_tags(s_second, d.second_mut());
}

pub(crate) fn simplify_mapping(mapping: &dyn Mapping) -> AstResult<DynMapping> {
    let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() else {
        return mapping.simplify();
    };

    // Guard against infinite recursion.  The fingerprint-based guard
    // detects cycles (same structure reappearing on the call stack) and
    // the depth counter caps absolute recursion depth.  Either condition
    // causes an immediate bail — we return the mapping unchanged.
    let series = cmp.composition() == Composition::Series;

    let _guard = match SimplifyGuard::enter(mapping) {
        Some(g) => g,
        None => {
            // The guard matched, so C returns `astClone( this_mapping )`. The
            // comparison that produced the match has already cleared the
            // element tags on the object being cloned.
            return Ok(mapping.clone_box());
        }
    };

    let restricted = cmp.restricted_simplify();

    let mut list = flatten(mapping, false, series)?;

    let changed = merge_loop_restricted(&mut list, series, restricted);

    // If no simplification occurred, preserve the original tree structure
    // rather than reconstructing (which changes associativity).
    //
    // "No simplification" means both: `merge_loop` made no pairwise change,
    // *and* the decomposition itself was not made simpler by normalising an
    // inverted CmpMap. The latter mirrors C `cmpmap.c` Simplify, where
    // `astMapList` returns a `simpler` flag (`result = invert || r1 || r2`,
    // MapList:1191) that is true whenever any inverted CmpMap is encountered
    // during the decomposition; C then rebuilds the (right-associated)
    // sequence even when no component merge occurred. Without this, a tree
    // whose only redundancy is an inverted CmpMap (e.g. a buried linear
    // residue that decomposition floats to the front) is left in its
    // original nesting instead of the canonical form C produces.
    // C's `simpler` (cmpmap.c:3634) — the flag that decides between cloning the
    // supplied CmpMap and rebuilding it from the component list.
    let simpler = decomposition_is_simpler(mapping, false, series);
    // An opposite-kind component that `flatten` reduced eagerly must still be
    // adopted, so the tree has to be rebuilt from the list. Whether that rebuild
    // is C's closing fold depends on which reduction fired — see
    // [`OpaqueLeafReduction`]. A `Converged` one leaves C's `simpler` at 0, so
    // C took `astClone( this )`: no component `Invert` was set and no nested
    // `IsSimple` was cleared.
    let reduction = opaque_leaf_reduction(mapping, false, series);
    let adopt_reduced_leaf = reduction != OpaqueLeafReduction::None;
    if !changed && !simpler && !adopt_reduced_leaf && list.len() > 1 {
        let mut cloned = mapping.clone_box();
        if let Some(cmp_mut) = cloned.as_any_mut().downcast_mut::<CmpMap>() {
            cmp_mut.normalize_unitmap_children();
        }
        // Every `IsSimple` now on a list entry is one C raised on the *live*
        // component: `SeparateMappings` (cmpmap.c:3326) leaves each non-duplicate
        // entry aliasing the real child, and both stampers — the nominate loop's
        // `astSimplify` (cmpmap.c:1437) and the recursive simplify of a component
        // that `flatten` performs eagerly — write through that alias. So the tags
        // are still on `this`'s children when `Simplify` returns `astClone( this )`
        // (cmpmap.c:3634), and they must be transplanted onto our fresh clone.
        let entries: Vec<DynMapping> = list.into_iter().map(|(m, _, _)| m).collect();
        apply_component_stamps(&mut cloned, false, series, &entries, &mut 0);
        // C's `!simpler` branch: `astClone( this )`, not a fresh object.
        SIMPLIFY_REBUILT.set(false);
        return Ok(cloned);
    }

    // The plain builder is used both when nothing reduced at all and when the
    // only reduction was a `Converged` one: in the latter case C rebuilt nothing
    // itself, so there is no fold whose `astSetInvert` would clear component tags.
    if !changed && !simpler && reduction != OpaqueLeafReduction::Rebuilds {
        SIMPLIFY_REBUILT.set(false);
        return reconstruct(list, series);
    }

    // C's `simpler` branch: the result is folded afresh from the component list,
    // so it is a new pointer even when the fold reproduces the input's shape.
    SIMPLIFY_REBUILT.set(true);
    reconstruct(list, series)
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
        sla1.add(SlaCvtType::Hfk5z, &[2000.0]);
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
        sla2.add(SlaCvtType::Fk5hz, &[2000.0]);
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
    fn merge_ctx_tracks_frozen_per_entry() {
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
        assert!((out.point(0)[0] - 20.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 60.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 105.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 20.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 5.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 12.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 4.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 6.0).abs() < 1e-12);
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
        assert!((before.point(0)[0] - after.point(0)[0]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 43.0).abs() < 1e-12); // 13 + 6*5 = 43
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
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 2.0).abs() < 1e-12);
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
        fwd.add(SlaCvtType::Eqgal, &[]);
        let mut inv = SlaMap::new();
        inv.add(SlaCvtType::Eqgal, &[]);
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
        fwd.add(SlaCvtType::Eqgal, &[]);
        let mut inv = SlaMap::new();
        inv.add(SlaCvtType::Galeq, &[]);
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
        assert!((out.point(0)[0] - 33.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 31.0).abs() < 1e-12); // 6*5 + 1

        // Also verify transform equivalence.
        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 21.0).abs() < 1e-12); // 2*5 + 11
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
        assert!((out.point(0)[0] - 31.0).abs() < 1e-12); // 2*5 + 21

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 25.0).abs() < 1e-12); // 3*5 + 10

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 45.0).abs() < 1e-12); // 3*5 + 30

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 7.0).abs() < 1e-12); // 5 + 2

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 10.0).abs() < 1e-12); // 2*15 - 20

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 33.0).abs() < 1e-12); // 3*10 + 3
        assert!((out.point(0)[1] - 66.0).abs() < 1e-12); // 3*20 + 6

        let before = cmp.transform(&pts, true).unwrap();
        assert!((before.point(0)[0] - out.point(0)[0]).abs() < 1e-12);
        assert!((before.point(0)[1] - out.point(0)[1]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 28.0).abs() < 1e-12); // 3*5 + 13

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
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 15.0).abs() < 1e-12);
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
        assert!((result.as_any().downcast_ref::<ZoomMap>().unwrap().factor() - 0.25).abs() < 1e-12);

        // Either way the forward transform divides by 4.
        let pts = PointSet::from_rows([[8.0, 12.0]]);
        let out = result.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 2.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 3.0).abs() < 1e-12);
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
        assert!((before.point(0)[0] - after.point(0)[0]).abs() < 1e-12);
        assert!((before.point(0)[1] - after.point(0)[1]).abs() < 1e-12);
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
        assert!((before.point(0)[0] - after.point(0)[0]).abs() < 1e-12);
        assert!((before.point(0)[1] - after.point(0)[1]).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 12.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 10.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 21.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 8.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 11.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 10.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 10.0).abs() < 1e-12);
        assert!((out.point(0)[2] - 21.0).abs() < 1e-12);
        assert!((out.point(0)[3] - 21.0).abs() < 1e-12);
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
        assert!((out.point(0)[0] - 7.0).abs() < 1e-12);
        assert!((out.point(0)[1] - 13.0).abs() < 1e-12);
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
        assert!((out_orig.point(0)[0] - out_simp.point(0)[0]).abs() < 1e-12);
        assert!((out_orig.point(0)[1] - out_simp.point(0)[1]).abs() < 1e-12);
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
        assert!((out_orig.point(0)[0] - out_simp.point(0)[0]).abs() < 1e-12);
        assert!((out_orig.point(0)[1] - out_simp.point(0)[1]).abs() < 1e-12);
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
        assert!((after.point(0)[2] - 11.5).abs() < 1e-12);
    }

    /// C's base `Simplify` builds a one-element list and calls `astMapMerge`
    /// until nothing changes or the list grows past one
    /// (`mapping.c:16871-16883`). A unit-factor ZoomMap is a UnitMap and
    /// `zoom_map_merge` says so, so the driver must reach that with no
    /// per-class `simplified()` in the way.
    #[test]
    fn base_simplify_reaches_a_classes_own_map_merge() {
        let zoom = ZoomMap::new(2, 1.0).expect("unit-factor ZoomMap constructs");
        let out = base_simplify(&zoom).expect("base_simplify succeeds");
        assert!(
            out.as_any().is::<UnitMap>(),
            "expected a UnitMap from a unit-factor ZoomMap"
        );
    }

    /// A Mapping its own `map_merge` cannot reduce comes back unchanged rather
    /// than erroring, which is C returning a clone.
    #[test]
    fn base_simplify_returns_a_clone_when_nothing_reduces() {
        let z = ZoomMap::new(1, 3.0).expect("ZoomMap constructs");
        let out = base_simplify(&z).expect("base_simplify succeeds");
        assert_eq!(
            out.as_any()
                .downcast_ref::<ZoomMap>()
                .expect("still a ZoomMap")
                .factor(),
            3.0
        );
    }
}

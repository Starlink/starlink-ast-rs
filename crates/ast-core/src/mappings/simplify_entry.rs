//! The one way into a per-class simplification.
//!
//! C has a single public entry, `astSimplify_` (`mapping.c:24755`), which
//! short-circuits on the `IsSimple` bit, calls the per-class `Simplify` vtab
//! slot, and records the result. That slot has exactly one caller in the whole
//! library, `mapping.c:24772`, so every reduction anywhere in C passes the gate.
//!
//! [`ViaSimplify`] makes that arithmetic hold here too. It is nameable
//! everywhere and constructible only in this module, and this module contains
//! nothing but the entry point, so [`Mapping::simplified`] has exactly one
//! possible caller.
//!
//! **Do not add anything else to this file.** It holds the gate
//! (`simplify_reporting_change`), the caller that drops the gate's answer to
//! "did anything change?" (`simplify`) and the tail they share
//! (`record_simplified`); there is exactly one `ViaSimplify(())` in the file and
//! the guarantee is that nothing outside it can write a second one.

use super::{AstResult, CmpMap, DynMapping, Mapping, simplify};

/// Proof that a per-class reduction was reached through
/// `<dyn Mapping>::simplify`, and therefore through the `IsSimple` gate.
///
/// Constructible only in this module. Implementations of
/// [`Mapping::simplified`] receive one and never build one.
pub struct ViaSimplify(());

impl dyn Mapping {
    /// Simplify this mapping and mark the result as simplified.
    ///
    /// This is the public entry point equivalent to C's `astSimplify`
    /// (`mapping.c:24755`). It calls the type-specific
    /// [`Mapping::simplified`] and then sets the `has_been_simplified` flag on
    /// the result, which causes `IsSimp = 1` to appear in the serialized
    /// output.
    ///
    /// This is the only way to reach a per-class reduction: [`ViaSimplify`] is
    /// constructible only here, so anything that wants one calls this.
    pub fn simplify(&self) -> AstResult<DynMapping> {
        self.simplify_reporting_change().map(|(result, _)| result)
    }

    /// `simplify`, also reporting whether anything reduced.
    ///
    /// `false` is C's `astClone( this )` exit: the per-class slot handed back the
    /// object it was given, so the identity test C's generic drivers apply to
    /// the returned pointer (`frameset.c:10208`, `mapping.c:16891`,
    /// `cmpmap.c:1456`) says "no change". Callers that need that answer ask for
    /// it here; `simplify` is the same gate with the answer dropped.
    ///
    /// The Mapping comes back either way, because C's no-change pointer is the
    /// caller's own object and carries whatever `IsSimple` stamps the reduction
    /// wrote into it through aliases. The port has only `&self`, so those stamps
    /// arrive on the returned clone; discarding it would drop them.
    pub fn simplify_reporting_change(&self) -> AstResult<(DynMapping, bool)> {
        // The closest analogue of C's `astSimplify`, so the trace counts here
        // line up with the C library's `SIMPLIFY-ENTER`.
        simplify::trace::simplify_enter("public-simplify", self);

        // C short-circuits an already-simplified Mapping here, returning a
        // clone without reaching the vtab slot at all (mapping.c:24763). This
        // is the gate: a class whose reduction lives in `map_merge` never has
        // to repeat the test, because nothing can reach a reduction except
        // through this function.
        //
        // The same condition also declines for a Mapping that must be left
        // intact (`astDoNotSimplify`, mapping.c:24764) — C tests both in one
        // expression, so the two take the same `astClone( this )` exit.
        if self.has_been_simplified() || self.do_not_simplify() {
            return Ok((self.clone_box(), false));
        }

        let (mut result, changed) = self.simplified(ViaSimplify(()))?;
        record_simplified(self, &mut result);
        Ok((result, changed))
    }
}

/// C `astSimplify_`'s tail (`mapping.c:24755-24782`): record on the returned
/// object that it came out of a simplification.
///
/// A restricted simplify (C `RestrictedSimplify`) does NOT mark its result as
/// simplified — more simplification may still be possible — and the restricted
/// flag itself is cleared on the result (`mapping.c:24755-24760`).
fn record_simplified(this: &dyn Mapping, result: &mut DynMapping) {
    let restricted = this.restricted_simplify();

    // `as_any` cannot be called on the `Box<dyn Mapping>` directly: it would
    // resolve via auto-deref but the resulting `&dyn Any` points at the Box, not
    // the inner mapping. Going through `as_mut` first gives a `&mut dyn
    // Mapping`, on which `as_any_mut` returns the inner type-erased reference
    // that `downcast_mut` can match.
    let rm: &mut dyn Mapping = result.as_mut();
    if let Some(cm) = rm.as_any_mut().downcast_mut::<CmpMap>() {
        // Drop redundant InvA/InvB flags on a parallel CmpMap whose
        // corresponding child is a UnitMap: since UnitMap is its own inverse the
        // external flag is observationally irrelevant, and the C reference emits
        // it cleared.
        cm.normalize_unitmap_children();
    }
    if restricted {
        // Clear the restricted flag on the result; do not set IsSimple.
        result.set_restricted_simplify(false);
    } else {
        result.set_has_been_simplified(true);
    }
    // C clears this on the result whichever branch ran (`mapping.c:24788`), so
    // the protected flag cannot appear in a public dump.
    result.set_allow_simplify(false);
}

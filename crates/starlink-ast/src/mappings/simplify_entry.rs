// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

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
//! (`simplify_with_outcome`), the result it reports (`SimplifyOutcome`), the
//! callers that narrow its answer (`simplify_reporting_change` and `simplify`)
//! and the tail they share (`record_simplified`); there is exactly one
//! `ViaSimplify` construction in the file and the guarantee is that nothing
//! outside it can write a second one.

use super::simplify::TagTree;
use super::{AstResult, DynMapping, Mapping, simplify};
use crate::error::ErrorCode;

/// Proof that a per-class reduction was reached through
/// `<dyn Mapping>::simplify`, and therefore through the `IsSimple` gate.
///
/// Constructible only in this module. Implementations of
/// [`Mapping::simplified`] receive one and never build one.
pub struct ViaSimplify<'a>(&'a mut Option<TagTree>);

impl ViaSimplify<'_> {
    /// Supply the records of simplification the reduction wrote on this
    /// Mapping's own components.
    ///
    /// C writes them on the components themselves, through aliases, so they
    /// are on the Mapping C returns when the reduction fails because it
    /// reached an undefined transformation (mapping.c `astSimplify_`), and on
    /// the Mapping its caller holds when the reduction returns a new one.
    pub(crate) fn component_records(&mut self, records: TagTree) {
        *self.0 = Some(records);
    }
}

/// How a simplification ended.
pub(crate) enum SimplifyOutcome {
    /// The reduction returned a new Mapping. The records it wrote on this
    /// Mapping's components, if it wrote any, come with it.
    Changed(Option<TagTree>),
    /// Nothing reduced; the returned copy is recorded as simplified.
    Unchanged,
    /// The reduction reached an undefined transformation; the returned copy
    /// is not recorded as simplified.
    Failed,
}

impl dyn Mapping {
    /// Simplify this Mapping, eliminating redundant computational steps or
    /// merging separate steps that can be performed more efficiently as one.
    ///
    /// For example, a Mapping that multiplies coordinates by 5 and then by
    /// 10 simplifies to one that multiplies by 50, and one that multiplies by
    /// 5 and then divides by 5 reduces to a [`UnitMap`](super::UnitMap).
    /// Simplifying is worthwhile for a Mapping that has been through
    /// substantial processing or was built by combining other Mappings,
    /// before using it to transform a large number of points.
    ///
    /// Returns the simplified Mapping, which is a copy of this one when
    /// nothing could be simplified.
    /// The result records that it has been simplified
    /// ([`Mapping::has_been_simplified`]), so simplifying it again returns it
    /// unchanged.
    /// A Mapping whose [`ident`](Mapping::ident) is set is returned unchanged,
    /// so that it keeps its identity; this does not apply to a FrameSet or a
    /// Region.
    /// A [`FrameSet`](crate::frames::FrameSet) is returned as a copy in which
    /// every Mapping between its Frames has been simplified.
    ///
    /// A reduction that reaches an undefined transformation, such as one that
    /// needs to transform points through a component lacking it, ends without
    /// error and returns this Mapping unsimplified.
    ///
    /// # Errors
    ///
    /// Any other error raised while reducing the Mapping is returned.
    #[doc(alias = "astSimplify")]
    pub fn simplify(&self) -> AstResult<DynMapping> {
        // C's astSimplify (mapping.c:24755): this calls the type-specific
        // `Mapping::simplified` and then sets the `has_been_simplified` flag on
        // the result, which causes `IsSimp = 1` to appear in the serialized
        // output. This is the only way to reach a per-class reduction:
        // `ViaSimplify` is constructible only here, so anything that wants one
        // calls this.
        self.simplify_reporting_change().map(|(result, _)| result)
    }

    /// `simplify`, also reporting whether anything reduced.
    ///
    /// Returns the Mapping and `true` when it is a different object from this
    /// one, or a copy of this one and `false` when nothing reduced.
    /// The copy is returned either way because it carries the records of
    /// the components that were simplified on the way.
    ///
    /// # Errors
    ///
    /// As for `simplify`.
    pub fn simplify_reporting_change(&self) -> AstResult<(DynMapping, bool)> {
        self.simplify_with_outcome()
            .map(|(result, outcome)| (result, matches!(outcome, SimplifyOutcome::Changed(_))))
    }

    /// `simplify`, also reporting how it ended.
    pub(crate) fn simplify_with_outcome(&self) -> AstResult<(DynMapping, SimplifyOutcome)> {
        // `false` is C's `astClone( this )` exit: the per-class slot handed
        // back the object it was given, so the identity test C's generic
        // drivers apply to the returned pointer (frameset.c:10208,
        // mapping.c:16891, cmpmap.c:1456) says "no change". C's no-change
        // pointer is the caller's own object and carries whatever `IsSimple`
        // stamps the reduction wrote into it through aliases; the port has only
        // `&self`, so those stamps arrive on the returned clone.
        //
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
            return Ok((self.clone_box(), SimplifyOutcome::Unchanged));
        }

        let mut records = None;
        match self.simplified(ViaSimplify(&mut records)) {
            Ok((mut result, changed)) => {
                record_simplified(self, &mut result);
                let outcome = if changed {
                    SimplifyOutcome::Changed(records)
                } else {
                    SimplifyOutcome::Unchanged
                };
                Ok((result, outcome))
            }
            // C clears an AST__NODEF or AST__TRNND raised by the reduction and
            // returns `astClone( this )`, which is not recorded as simplified
            // (mapping.c `astSimplify_`).
            Err(e) if matches!(e.code(), ErrorCode::Nodef | ErrorCode::Trnnd) => {
                let mut unsimplified = self.clone_box();
                if let Some(records) = records {
                    records.apply_to(unsimplified.as_mut());
                }
                Ok((unsimplified, SimplifyOutcome::Failed))
            }
            Err(e) => Err(e),
        }
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

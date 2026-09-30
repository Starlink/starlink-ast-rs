//! Faithful Rust port of `reference/starlink-ast/src/unit.c`. Provides
//! `unit_mapper`, `unit_normaliser`, and `unit_label` — the public C
//! entry points — for converting FITS-WCS-style unit strings into
//! `Mapping`s.
//!
//! The phasing matches the C pipeline exactly:
//!
//! 1. [`parser::create_tree`] — string → `UnitNode` tree (with
//!    constants folded and inverted).
//! 2. [`simplify::remake_tree`] — substitute derived units with
//!    their basic-unit definitions.
//! 3. [`simplify::simplify_tree`] — algebraic standardisation.
//! 4. [`dim_anal::dim_anal`] (driven by [`mapper::unit_mapper`]) —
//!    confirm input and output are dimensionally compatible.
//! 5. [`mapper::unit_mapper`] — assemble a `Mapping` (UnitMap /
//!    ZoomMap shortcut, or MathMap general path).
//!
//! The non-linear (MathMap) branch lands in commit 2 of the port plan;
//! commit 1 returns `None` for non-linear conversions.

#![allow(dead_code)] // crate-internal; wired into SpecFrame in commit 3

mod cmp;
mod complicate;
mod concat;
mod dim_anal;
mod invert;
mod mapper;
mod parser;
mod simplify;
mod table;
mod to_expr;
mod tree;

#[cfg(test)]
mod tests;

#[allow(unused_imports)] // wired into SpecFrame in commit 3
pub(crate) use mapper::unit_mapper;

use crate::keymap::convert::scan_double;

/// Return a standard FITS-WCS form of a units string, or `None` if it
/// cannot be parsed as a units expression (where C reports `AST__BADUN`).
/// Mirrors C `astUnitNormaliser_` (`unit.c`:5869).
pub(crate) fn unit_normaliser(input: &str) -> Option<String> {
    let mut tree = parser::create_tree(input, false)?;

    // C passes `std = 1`, so the standardising rewrites run too.
    simplify::simplify_tree(&mut tree, true);

    // `create_tree` has already inverted the literal constants; C inverts
    // them a second time here, so that a constant of 1000 for "m" is
    // formatted as a unit of "1000 m" (`unit.c`:5933).
    simplify::invert_constants(&mut tree)?;

    let mut result = to_expr::make_units_exp(&tree);

    // A result that is wholly a constant is blanked; C tests this with
    // `astSscanf( result, " %lg %n", ... )` and a check that the whole
    // string was consumed (`unit.c`:5942).
    if scan_double(&result).is_some() {
        result.clear();
    }
    Some(result)
}

/// Normalise a units string, returning it unchanged if it is not a units
/// expression: a blank Unit, or a description of axis values such as
/// `"ddd:mm:ss"`. Mirrors C `astNormUnit_` (`unit.c`:5960), which recovers
/// from `AST__BADUN` only; no other failure is possible here.
pub(crate) fn norm_unit(input: &str) -> String {
    unit_normaliser(input).unwrap_or_else(|| input.to_string())
}

/// Public-API analogue of C `astUnitLabel_` (`unit.c`:5171-5236).
/// Returns the human-readable label for a known unit symbol (e.g.
/// `"Hertz"` for `"Hz"`), or `None` if the symbol is not known.
pub(crate) fn unit_label(sym: &str) -> Option<&'static str> {
    table::find_known_unit(sym).map(|u| u.label)
}

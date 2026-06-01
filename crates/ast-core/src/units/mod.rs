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

/// Public-API analogue of C `astUnitLabel_` (`unit.c`:5171-5236).
/// Returns the human-readable label for a known unit symbol (e.g.
/// `"Hertz"` for `"Hz"`), or `None` if the symbol is not known.
pub(crate) fn unit_label(sym: &str) -> Option<&'static str> {
    table::find_known_unit(sym).map(|u| u.label)
}

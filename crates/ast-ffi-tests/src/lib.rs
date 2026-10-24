//! Test tier 3: the C `ast_tester` programs against the Rust `libast`
//! — **not yet implemented**.
//!
//! When built, this crate compiles the upstream C test programs from
//! `reference/starlink-ast/ast_tester/` and links them against the
//! `starlink-ast-capi` cdylib, proving the shim preserves the C ABI. Skipped with a
//! notice when the submodule is absent.
//!
//! Blocked on `starlink-ast-capi`, which exports no symbols yet. See `PLAN.md` §7.

#![allow(missing_docs)]

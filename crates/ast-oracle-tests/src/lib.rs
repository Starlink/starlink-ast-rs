//! Test tier 4: live bit/ULP comparison against the C library
//! — **not yet implemented**.
//!
//! When built, this crate links `ast-core` and the reference `libast.a`
//! into one binary and drives matched inputs through both: bit-exact for
//! integer and classification outputs, bounded ULP for floating point,
//! property-based for random pipelines.
//!
//! This is the gap that matters most in the current verification posture:
//! **no test in this repository executes C code and compares the result
//! live.** All C comparison today is against frozen corpora committed under
//! `crates/ast-core/tests/fixtures/`, and a recording cannot notice when the
//! thing it recorded changes. See `PLAN.md` §7.

#![allow(missing_docs)]

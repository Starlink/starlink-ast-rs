//! C ABI shim — **not yet implemented**.
//!
//! When built, this crate becomes the `cdylib` exporting the symbols of
//! `ast.h` byte-for-byte, so existing applications (`pyast`, the Starlink
//! suite, DS9) link against the Rust build with no source or recompile
//! change. That is the design's stated purpose; nothing here is started.
//!
//! Exports nothing today: no `ast.h`, no handle registry, no
//! `astClone`/`astAnnul` reference counting, no `astBegin`/`astEnd` stack, no
//! thread-local status, no variadic `astSet` parsing, no `catch_unwind`
//! boundary. Test tiers 3 and 4 are blocked on it.
//!
//! See the design spec's "CFFI shim (`starlink-ast-capi`)" section for the intended
//! shape, and `PLAN.md` §7 for what is outstanding — including the
//! `AST__IMMUT` question, which this crate has to answer because it restores
//! object sharing that `starlink-ast` does not have.

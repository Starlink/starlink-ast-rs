//! Test tier 4: live comparison against the C library.
//!
//! The C library is loaded at runtime through `ast_bench::c_engine`, built by
//! `tools/perf/build_c_engine.sh`. Today the tier holds one test,
//! `tests/call_count_parity.rs`: for every transform-oracle section the port
//! must make the same calls into libm, PAL, ERFA and wcslib as C.
//!
//! The rest of the design (bit-exact and bounded-ULP comparison of live
//! outputs, property-based pipelines) is not implemented; until it is, output
//! comparison against C is against the frozen corpora under
//! `crates/starlink-ast/tests/fixtures/`. See `PLAN.md` §7.

#![allow(missing_docs)]

//! Test tier 5: Rust-versus-C performance benchmarks — **placeholder only**.
//!
//! `benches/oracle.rs` is a six-line criterion stub and CI runs no benchmark,
//! so the >10% hot-path regression gate described in the design spec does not
//! exist. Until it is wired up, per-class performance is measured by hand with
//! `cargo run --release --example bench_per_class -p ast-core` against the C
//! oracle, and compared with the snapshot in
//! `docs/issues/mapping-perf-vs-c-oracle.md`. See `PLAN.md` §2.

#![allow(missing_docs)]

//! Integration tests: Errors, PointSet, trait coverage, audit and ledger gates, and benchmarks.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to ast-core relinks a few programs rather than
//! one per file.

// Test code computes expected values directly; the counted `math` wrappers
// are for the library (`crates/ast-core/clippy.toml`).
#![allow(clippy::disallowed_methods)]

#[path = "../common/mod.rs"]
mod common;

#[path = "../bench_lsst.rs"]
mod bench_lsst;
#[path = "../error_codes.rs"]
mod error_codes;
#[path = "../error_model.rs"]
mod error_model;
#[path = "../points.rs"]
mod points;
#[path = "../pointset.rs"]
mod pointset;
#[path = "../test_area_membership.rs"]
mod test_area_membership;
#[path = "../test_audit_citations.rs"]
mod test_audit_citations;
#[path = "../test_c_compare.rs"]
mod test_c_compare;
#[path = "../test_c_docs_audit.rs"]
mod test_c_docs_audit;
#[path = "../test_constructor_checks.rs"]
mod test_constructor_checks;
#[path = "../test_dump_comparator.rs"]
mod test_dump_comparator;
#[path = "../test_guide.rs"]
mod test_guide;
#[path = "../test_mapmerge_audit.rs"]
mod test_mapmerge_audit;
#[path = "../test_nan_protocol.rs"]
mod test_nan_protocol;
#[path = "../test_simplify_regression.rs"]
mod test_simplify_regression;
#[path = "../test_tracing_feature.rs"]
mod test_tracing_feature;
#[path = "../test_transform_memory.rs"]
mod test_transform_memory;
#[path = "../test_verification_ledger.rs"]
mod test_verification_ledger;
#[path = "../traits_compile.rs"]
mod traits_compile;

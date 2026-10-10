//! Integration tests: Errors, PointSet, trait coverage, audit and ledger gates, and benchmarks.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to ast-core relinks a few programs rather than
//! one per file.

#[path = "../common/mod.rs"]
mod common;

#[path = "../bench_lsst.rs"]
mod bench_lsst;
#[path = "../error_codes.rs"]
mod error_codes;
#[path = "../error_model.rs"]
mod error_model;
#[path = "../pointset.rs"]
mod pointset;
#[path = "../test_area_membership.rs"]
mod test_area_membership;
#[path = "../test_audit_citations.rs"]
mod test_audit_citations;
#[path = "../test_constructor_checks.rs"]
mod test_constructor_checks;
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

//! Tests for the keyword-card variant of NATIVE encoding (BEGAST/ENDAST cards).
//!
//! The C library accepts NATIVE-encoded headers in two storage forms:
//!   1. COMMENT-card variant: native text inside `COMMENT  AST +++  AST` markers
//!   2. Keyword-card variant: BEGAST_xx / ENDAST_xx / data-keyword cards
//!
//! Variant 2 is what `polco.head`, `polco2.head`, `specflux.head`,
//! `timeplot.head` use. This test exercises that path end-to-end.

use std::path::PathBuf;

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv/inputs")
        .join(name)
}

fn read_fixture(name: &str) -> AnyObject {
    let text =
        std::fs::read_to_string(fixture_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    let mut fc = FitsChan::from_header(&text).unwrap_or_else(|e| panic!("from_header {name}: {e}"));
    fc.read()
        .unwrap_or_else(|e| panic!("read {name}: {e}"))
        .unwrap_or_else(|| panic!("read {name}: returned None"))
}

#[test]
fn polco_loads() {
    let _obj = read_fixture("polco.head");
    // Just verify it loads without error. Identity / value checks are out
    // of scope for the encoding-decoder test.
}

#[test]
fn polco2_loads() {
    let _obj = read_fixture("polco2.head");
}

#[test]
fn specflux_loads() {
    let _obj = read_fixture("specflux.head");
}

#[test]
fn timeplot_loads() {
    let _obj = read_fixture("timeplot.head");
}

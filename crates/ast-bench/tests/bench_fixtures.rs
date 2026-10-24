//! The bench fixtures are not in starlink-ast's transform-oracle gate (they are
//! not in `ast_tester` yet), so this test holds the port to C's recorded
//! outputs for them, with the golden tolerance of
//! `crates/starlink-ast/tests/test_transform_oracle.rs`.

use std::path::Path;

use ast_bench::load::{Loaded, load_fixture};
use ast_bench::oracle::{BENCH_FIXTURES, BENCH_ORACLE, parse_oracle};
use ast_bench::points::select_rows;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;

const GOLDEN_RTOL: f64 = 1e-7;
const GOLDEN_ATOL: f64 = 1e-7;

/// One per fixture and defined direction; `gen_bench_fixtures` reports it.
const BENCH_SECTIONS: usize = 52;

fn within_tol(got: f64, reference: f64) -> bool {
    if got == BAD_VALUE || reference == BAD_VALUE {
        return got == reference;
    }
    !got.is_nan() && (got - reference).abs() <= GOLDEN_ATOL + GOLDEN_RTOL * reference.abs()
}

#[test]
fn bench_fixtures_match_oracle() {
    let root = Path::new(BENCH_FIXTURES);
    let text = std::fs::read_to_string(root.join(BENCH_ORACLE)).unwrap();
    let sections = parse_oracle(&text).unwrap();
    assert_eq!(sections.len(), BENCH_SECTIONS);
    let mut failures = Vec::new();
    for s in sections {
        let Loaded::Mapping(obj) = load_fixture(root, &s.relpath).unwrap() else {
            panic!("{} is not a Mapping", s.relpath)
        };
        let n = s.npoint();
        let rows: Vec<Vec<f64>> = (0..n)
            .map(|r| s.inputs.iter().map(|c| c[r]).collect())
            .collect();
        let out = obj
            .as_mapping()
            .unwrap()
            .transform(&PointSet::from_rows(rows), s.forward)
            .unwrap();
        for r in 0..n {
            for a in 0..s.nout {
                if !within_tol(out.point(r)[a], s.outputs[a][r]) {
                    failures.push(format!(
                        "{} forward={} row {r} axis {a}: {} vs C {}",
                        s.relpath,
                        s.forward,
                        out.point(r)[a],
                        s.outputs[a][r]
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every section can be timed at every N: the small-N runs need a row that
/// is good in all its inputs and outputs.
#[test]
fn every_bench_section_has_a_good_row() {
    let text = std::fs::read_to_string(Path::new(BENCH_FIXTURES).join(BENCH_ORACLE)).unwrap();
    for s in parse_oracle(&text).unwrap() {
        select_rows(&s, 1).unwrap();
    }
}

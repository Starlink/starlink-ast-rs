//! Phase 0 of the benchmarking design
//! (`docs/superpowers/specs/2026-10-15-transform-benchmarking-design.md`):
//! for every transform-oracle section the port must make the same calls into
//! libm, PAL, ERFA and wcslib as C, counted at the boundary of AST's own code
//! on both sides.
//!
//! Needs the counting C engine: `tools/perf/build_c_engine.sh count`.

use std::collections::BTreeMap;
use std::path::Path;

use ast_bench::c_engine::{CEngine, Variant};
use ast_bench::load::{Loaded, load_fixture};
use ast_bench::oracle::{CORE_FIXTURES, CORE_ORACLES, parse_oracle};
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;

/// Sections whose counts differ for an understood reason, each with that
/// reason. Keys are `"<relpath> dir=forward|inverse"`.
const COUNT_PARITY_EXCLUDED: &[(&str, &str)] = &[];

/// Per section, each name whose count differs: (rust, c).
type Diff = BTreeMap<String, (u64, u64)>;

fn key(relpath: &str, forward: bool) -> String {
    format!(
        "{relpath} dir={}",
        if forward { "forward" } else { "inverse" }
    )
}

fn within_tol(got: f64, want: f64) -> bool {
    if got == BAD_VALUE || want == BAD_VALUE {
        return got == want;
    }
    (got - want).abs() <= 1e-7 + 1e-7 * want.abs()
}

/// Every section whose counts differ, and every section either engine could
/// not run.
fn survey() -> (Vec<(String, Diff)>, Vec<String>) {
    let c = CEngine::open(Variant::Count).unwrap();
    let root = Path::new(CORE_FIXTURES);
    let mut mismatched = Vec::new();
    let mut errors = Vec::new();
    for name in CORE_ORACLES {
        let text = std::fs::read_to_string(root.join("oracle").join(name)).unwrap();
        for s in parse_oracle(&text).unwrap() {
            let k = key(&s.relpath, s.forward);
            let obj = match load_fixture(root, &s.relpath) {
                Ok(Loaded::Mapping(obj)) => obj,
                Ok(Loaded::Untransformable(_)) => continue,
                Err(e) => {
                    errors.push(format!("{k}: rust load: {e}"));
                    continue;
                }
            };
            let map = obj.as_mapping().unwrap();
            let n = s.npoint();
            let rows: Vec<Vec<f64>> = (0..n)
                .map(|r| s.inputs.iter().map(|c| c[r]).collect())
                .collect();
            let pts = PointSet::from_rows(rows);
            let mut out = PointSet::zeros(n, s.nout);
            ast_core::call_counts::reset();
            if let Err(e) = map.transform_into(&pts, s.forward, &mut out) {
                errors.push(format!("{k}: rust transform: {e}"));
                continue;
            }
            let rust = ast_core::call_counts::snapshot();

            let cm = match c.load(root, &s.relpath) {
                Ok(m) => m,
                Err(e) => {
                    errors.push(format!("{k}: c load: {e}"));
                    continue;
                }
            };
            let mut cout = vec![0.0; s.nout * n];
            c.counts_reset().unwrap();
            if let Err(e) = cm.tran(s.forward, n, s.nin, &s.inputs.concat(), s.nout, &mut cout) {
                errors.push(format!("{k}: c transform: {e}"));
                continue;
            }
            let cc = c.counts().unwrap();
            if let Some((i, (got, want))) = cout
                .iter()
                .zip(s.outputs.concat())
                .enumerate()
                .find(|(_, (got, want))| !within_tol(**got, *want))
            {
                errors.push(format!("{k}: C value {i} is {got}, oracle {want}"));
            }

            let mut diff = Diff::new();
            for name in rust.keys().chain(cc.keys()) {
                let r = rust.get(name).copied().unwrap_or(0);
                let cv = cc.get(name).copied().unwrap_or(0);
                if r != cv {
                    diff.insert(name.clone(), (r, cv));
                }
            }
            if !diff.is_empty() {
                mismatched.push((k, diff));
            }
        }
    }
    (mismatched, errors)
}

#[test]
fn rust_and_c_make_the_same_library_calls() {
    let (mismatched, errors) = survey();
    let unexplained: Vec<_> = mismatched
        .iter()
        .filter(|(k, _)| !COUNT_PARITY_EXCLUDED.iter().any(|(e, _)| e == k))
        .collect();
    assert!(
        errors.is_empty(),
        "sections that could not be compared:\n{errors:#?}"
    );
    assert!(
        unexplained.is_empty(),
        "{} sections differ (name: (rust, c)):\n{unexplained:#?}",
        unexplained.len()
    );
}

#[test]
fn every_exclusion_still_differs() {
    let (mismatched, _) = survey();
    for (k, reason) in COUNT_PARITY_EXCLUDED {
        assert!(
            mismatched.iter().any(|(f, _)| f == k),
            "{k} now matches; remove it ({reason})"
        );
    }
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Phase 0 of the benchmarking design
//! (`docs/superpowers/specs/2026-10-15-transform-benchmarking-design.md`):
//! for every transform-oracle section the port must make the same calls into
//! libm, PAL, ERFA and wcslib as C, counted at the boundary of AST's own code
//! on both sides.
//!
//! Each engine transforms the section once before counting and the second,
//! counted call is compared: both libraries fill caches on first use (C's
//! Box `Cache`, a Region's default uncertainty, MathMap's constants, the
//! `palMappa` cache), and the steady-state cost per call is what matters.
//!
//! Needs the counting C engine: `tools/perf/build_c_engine.sh count`.

use std::collections::BTreeMap;
use std::path::Path;

use ast_bench::c_engine::{CEngine, Variant};
use ast_bench::load::{Loaded, load_fixture};
use ast_bench::oracle::{CORE_FIXTURES, CORE_ORACLES, parse_oracle};
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;

/// C moves the PointList's own uncertainty Region to each list point and
/// back again, and rebuilds its Box cache after each move; the port moves a
/// copy once per list point. `docs/issues/c-library-quirks.md`, "A
/// PointList's `Transform` moves its shared uncertainty Region twice per list
/// point".
const POINTLIST_RECENTRE: &str = "C re-centres the shared uncertainty twice per list point";

/// C negates and re-closes each selector Region in place around the
/// transform, which resets the Region's cache every call; the port uses a
/// negated copy. `docs/issues/c-library-quirks.md`, "SelectorMap's
/// `Transform` negates and re-closes each selector Region in place".
const SELECTOR_IN_PLACE: &str = "C negates its selector Regions in place, resetting their caches";

/// C's `CompressMatrix` computes `sqrt(DBL_EPSILON)` once per row and column;
/// the port once per matrix. `docs/issues/c-library-quirks.md`,
/// "`CompressMatrix` recomputes `sqrt( DBL_EPSILON )` for every row and
/// column".
const COMPRESS_SQRT_EPS: &str = "C recomputes sqrt(DBL_EPSILON) per row and column";

/// Sections whose counts differ for an understood reason, each with that
/// reason. Keys are `"<relpath> dir=forward|inverse"`. Every entry is C doing
/// work the port does not need; none is the port skipping work.
const COUNT_PARITY_EXCLUDED: &[(&str, &str)] = &[
    (
        "simplify/box_pointlist_merge.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/box_pointlist_merge.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/box_pointlist_merge.simp dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/box_pointlist_merge.simp dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_pointlist_01.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_pointlist_01.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_pointlist_01.simp dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_pointlist_01.simp dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_pointlist_02.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_pointlist_02.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_pointlist_02.simp dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_pointlist_02.simp dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_zoommap_04.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_zoommap_04.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_zoommap_04.simp dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/cap_zoommap_04.simp dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/interval_pointlist_merge.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/interval_pointlist_merge.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/interval_pointlist_merge.simp dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/interval_pointlist_merge.simp dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/neg_interval_unbounded_pointlist.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/neg_interval_unbounded_pointlist.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/neg_pointlist_parallel_nonregion.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/neg_pointlist_parallel_nonregion.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/neg_pointlist_series.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/neg_pointlist_series.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/neg_selectormap_not_equal.map dir=forward",
        SELECTOR_IN_PLACE,
    ),
    (
        "simplify/neg_selectormap_parallel.map dir=forward",
        SELECTOR_IN_PLACE,
    ),
    (
        "simplify/neg_selectormap_series_lower.map dir=forward",
        SELECTOR_IN_PLACE,
    ),
    (
        "simplify/neg_selectormap_series_upper.map dir=forward",
        SELECTOR_IN_PLACE,
    ),
    (
        "simplify/pointlist_parallel_merge_pair.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_parallel_merge_pair.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_parallel_merge_pair.simp dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_parallel_merge_pair.simp dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_self_simplify.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_self_simplify.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_self_simplify.simp dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_self_simplify.simp dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_unc_fillfactor_merge.map dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_unc_fillfactor_merge.map dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_unc_fillfactor_merge.simp dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/pointlist_unc_fillfactor_merge.simp dir=inverse",
        POINTLIST_RECENTRE,
    ),
    (
        "simplify/selectormap_region_simplify.map dir=forward",
        SELECTOR_IN_PLACE,
    ),
    (
        "simplify/selectormap_region_simplify.simp dir=forward",
        SELECTOR_IN_PLACE,
    ),
    (
        "simplify/selectormap_region_simplify_real.map dir=forward",
        SELECTOR_IN_PLACE,
    ),
    (
        "simplify/selectormap_region_simplify_real.simp dir=forward",
        SELECTOR_IN_PLACE,
    ),
    (
        "programs/teststcschan/stcschan-test1-doc3.ast dir=forward",
        POINTLIST_RECENTRE,
    ),
    (
        "programs/teststcschan/stcschan-test1-doc3.ast dir=inverse",
        POINTLIST_RECENTRE,
    ),
    ("serialisation/lsst.ast dir=inverse", COMPRESS_SQRT_EPS),
    (
        "wcsconv/framesets/lsst-ast.ast dir=inverse",
        COMPRESS_SQRT_EPS,
    ),
    (
        "wcsconv/framesets/sip2-head.ast dir=inverse",
        COMPRESS_SQRT_EPS,
    ),
    ("wcsconv/inputs/lsst.ast dir=inverse", COMPRESS_SQRT_EPS),
    ("wcsconv/inputs/lsst2.ast dir=inverse", COMPRESS_SQRT_EPS),
];

/// Per section, each name whose count differs: (rust, c).
type Diff = BTreeMap<String, (u64, u64)>;

/// The sections whose counts differ, and the sections that could not be
/// compared.
type Survey = (Vec<(String, Diff)>, Vec<String>);

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

/// The survey both tests read, run once: the C library's counters and state
/// are process-wide, so two surveys on parallel test threads would mix.
fn survey() -> &'static Survey {
    static SURVEY: std::sync::OnceLock<Survey> = std::sync::OnceLock::new();
    SURVEY.get_or_init(run_survey)
}

/// Every section whose counts differ, and every section either engine could
/// not run.
fn run_survey() -> Survey {
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
            if let Err(e) = map.transform_into(pts.as_points(), s.forward, out.as_points_mut()) {
                errors.push(format!("{k}: rust transform: {e}"));
                continue;
            }
            starlink_ast::call_counts::reset();
            if let Err(e) = map.transform_into(pts.as_points(), s.forward, out.as_points_mut()) {
                errors.push(format!("{k}: rust transform: {e}"));
                continue;
            }
            let rust = starlink_ast::call_counts::snapshot();

            let cm = match c.load(root, &s.relpath) {
                Ok(m) => m,
                Err(e) => {
                    errors.push(format!("{k}: c load: {e}"));
                    continue;
                }
            };
            let mut cout = vec![0.0; s.nout * n];
            if let Err(e) = cm.tran(s.forward, n, s.nin, &s.inputs.concat(), s.nout, &mut cout) {
                errors.push(format!("{k}: c transform: {e}"));
                continue;
            }
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

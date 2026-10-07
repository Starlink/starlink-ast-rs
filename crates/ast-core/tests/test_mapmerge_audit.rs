//! Enforce that every class in the audit universe has a `MapMerge` /
//! `Simplify` checklist, and that no row in one is left without a verdict.
//!
//! A prose audit cannot say whether it is finished. This one can: a class with
//! no checklist fails, and so does a row whose verdict is outside the closed
//! set. See `docs/audits/mapmerge/README.md` for the format.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Modules under `src/mappings/` that are helpers, not Mapping classes.
const MAPPING_HELPERS: &[&str] = &[
    "mod",
    "simplify",
    "simplify_entry",
    "helpers",
    "matrix_merge",
    "winmap_merge",
    "linear_approx",
    "map_box",
    "polymap_simd",
    "rate",
    "report",
];

/// The Region trait methods this audit covers. A region module that overrides
/// any of them takes part in `MapMerge` or `Simplify` and needs a checklist;
/// `RegPins` and `RegBaseMesh` are extension points too, but they answer a
/// different question and are audited elsewhere.
const REGION_SEAMS: &[&str] = &[
    "fn map_merge",
    "fn simplify_remapped",
    "fn get_def_unc",
    "fn simplified",
];

/// The marker for a class that overrides C's per-class `Simplify` vtab slot.
///
/// C's `astSimplify_` (`mapping.c:24772`) is the only caller of that slot, and a
/// class that overrides it owns a whole simplification algorithm of its own —
/// `cmpmap.c:3396`, `polygon.c:5291`, `circle.c:1769`, `ellipse.c:1879`,
/// `frameset.c:10111`. Those bodies are as much C's extension mechanism as
/// `MapMerge` is, so they belong in this audit whatever directory they live in.
const SIMPLIFY_SEAM: &str = "fn simplified";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/ast-core has two ancestors")
        .to_path_buf()
}

fn audit_dir() -> PathBuf {
    repo_root().join("docs/audits/mapmerge")
}

/// Module stems in a source directory, in sorted order.
///
/// A module may be held either as `<stem>.rs` or as `<stem>/mod.rs`; both name
/// the same class, so both count.
fn module_stems(dir: &Path) -> Vec<String> {
    let mut stems = Vec::new();
    for entry in std::fs::read_dir(dir).expect("source dir readable") {
        let path = entry.expect("readable entry").path();
        let stem = if path.is_dir() {
            if !path.join("mod.rs").is_file() {
                continue;
            }
            path.file_name()
        } else {
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            path.file_stem()
        };
        let stem = stem.and_then(|s| s.to_str()).expect("utf-8 stem");
        stems.push(stem.to_string());
    }
    stems.sort();
    stems
}

/// Every module that must carry a checklist, as checklist file stems.
///
/// Derived from the source tree rather than listed, so a class cannot be added
/// without its checklist being demanded.
fn audit_universe() -> BTreeSet<String> {
    let mut universe = BTreeSet::new();

    let mappings = repo_root().join("crates/ast-core/src/mappings");
    for stem in module_stems(&mappings) {
        if MAPPING_HELPERS.contains(&stem.as_str()) {
            continue;
        }
        universe.insert(stem);
    }

    // `regions/mod.rs` carries the generic `Region::Simplify` driver, so it is
    // in the universe whatever it overrides.
    let regions = repo_root().join("crates/ast-core/src/regions");
    for stem in module_stems(&regions) {
        let file = regions.join(format!("{stem}.rs"));
        let file = if file.is_file() {
            file
        } else {
            regions.join(&stem).join("mod.rs")
        };
        let source = std::fs::read_to_string(&file).expect("region module readable");
        if stem == "mod" || REGION_SEAMS.iter().any(|seam| source.contains(seam)) {
            universe.insert(format!("region_{stem}"));
        }
    }

    // A Frame class may also implement `Mapping` and override `Simplify`;
    // `FrameSet` does, and C gives it a `Simplify` body of its own. Scanning
    // `frames/` for the seam keeps such a class from staying outside the audit
    // simply because it is not filed under `mappings/`.
    let frames = repo_root().join("crates/ast-core/src/frames");
    for stem in module_stems(&frames) {
        let file = frames.join(format!("{stem}.rs"));
        let file = if file.is_file() {
            file
        } else {
            frames.join(&stem).join("mod.rs")
        };
        let source = std::fs::read_to_string(&file).expect("frame module readable");
        if source.contains(SIMPLIFY_SEAM) {
            universe.insert(format!("frame_{stem}"));
        }
    }

    universe
}

fn checklist_path(stem: &str) -> PathBuf {
    audit_dir().join(format!("{stem}.md"))
}

/// The verdict cell of every table row in a checklist.
fn verdicts(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .filter_map(|l| {
            let cells: Vec<&str> = l.trim().trim_matches('|').split('|').collect();
            if cells.len() < 5 {
                return None;
            }
            let first = cells[0].trim();
            // Skip the header row and the `|---|` separator.
            if first.parse::<u32>().is_err() {
                return None;
            }
            // The verdict is the last cell, so a checklist that also classifies
            // its steps as extension points is read the same way.
            Some(cells.last().expect("row has cells").trim().to_string())
        })
        .collect()
}

/// The verdicts a checklist row may carry.
///
/// Each one either asserts the step is present or names where the work or the
/// decision that excuses it is tracked. A verdict that names nothing would let
/// a missing step read as a finished one.
fn verdict_is_valid(verdict: &str) -> bool {
    verdict == "ported"
        || verdict.starts_with("n/a: ")
        // A divergence is a step the port refuses because performing it would
        // reproduce a defect in C's own. The citation is what makes it a
        // finding rather than an opinion, so it is part of the verdict.
        || (verdict.starts_with("divergence:")
            && verdict.contains("docs/issues/c-library-quirks.md"))
}

#[test]
fn every_audited_class_has_a_checklist() {
    let missing: Vec<String> = audit_universe()
        .into_iter()
        .filter(|stem| !checklist_path(stem).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "these classes have no MapMerge/Simplify checklist under \
         docs/audits/mapmerge/. Write one per docs/audits/mapmerge/README.md, \
         or add the module to the helper list if it is not a class: {missing:?}"
    );
}

#[test]
fn every_checklist_row_has_a_verdict() {
    let mut bad = Vec::new();
    for stem in audit_universe() {
        let path = checklist_path(&stem);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // reported by the other test
        };
        let rows = verdicts(&text);
        if rows.is_empty() {
            bad.push(format!("{stem}: checklist has no table rows"));
            continue;
        }
        for (i, v) in rows.iter().enumerate() {
            if !verdict_is_valid(v) {
                bad.push(format!("{stem} row {}: invalid verdict {v:?}", i + 1));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "a verdict must be `ported`, `n/a: <reason>`, or \
         `divergence: <reason citing docs/issues/c-library-quirks.md>`: \
         {bad:#?}"
    );
}

/// No checklist may sit in the directory without a class in the universe that
/// claims it. A stale file is a checklist nothing audits any more.
#[test]
fn no_checklist_is_orphaned() {
    let universe = audit_universe();
    let orphans: Vec<String> = module_stems(&audit_dir())
        .into_iter()
        .chain(
            std::fs::read_dir(audit_dir())
                .expect("audit dir readable")
                .filter_map(|e| {
                    let path = e.expect("readable entry").path();
                    if path.extension().and_then(|x| x.to_str()) != Some("md") {
                        return None;
                    }
                    let stem = path.file_stem()?.to_str()?.to_string();
                    (stem != "README").then_some(stem)
                }),
        )
        .filter(|stem| !universe.contains(stem))
        .collect();
    assert!(
        orphans.is_empty(),
        "these checklists name no module in the audit universe; the class was \
         renamed or removed and the checklist was left behind: {orphans:?}"
    );
}

/// Every C class that installs a `MapMerge` or `Simplify` vtab entry, paired
/// with what covers it on this side.
///
/// `audit_universe` is derived from the Rust module tree, so a C seam whose
/// class has no Rust module of its own produces no checklist to be missing and
/// sits outside the audit entirely. The C source is the authority on which
/// classes have a seam at all, so it is the authority here.
///
/// Coverage is one of:
///
/// - an audit checklist stem under `docs/audits/mapmerge/`;
/// - `driver: <repo-relative path>` for the base `Mapping` bodies, whose port
///   is one module the per-class checklists cite rather than a class of its own;
/// - `deferred: <class>` for a class this port has not started, counted in
///   `test_deferred_fixtures.rs::DEFERRED_CLASSES`;
/// - `undecided: <repo-relative path>` for a seam whose port depends on an open
///   design question, where the path states the question, names the fixture
///   that demonstrates it, and is counted in the verification ledger.
const C_SIMPLIFY_SEAMS: &[(&str, &str)] = &[
    ("box.c", "region_box_region"),
    ("circle.c", "region_circle"),
    (
        "cmpframe.c",
        "undecided: docs/issues/frame-in-a-mapping-slot.md",
    ),
    ("cmpmap.c", "cmpmap"),
    ("cmpregion.c", "region_cmpregion"),
    ("dssmap.c", "dssmap"),
    ("ellipse.c", "region_ellipse"),
    ("frameset.c", "frame_frameset"),
    ("grismmap.c", "grismmap"),
    ("interval.c", "region_interval"),
    ("intramap.c", "deferred: IntraMap"),
    ("lutmap.c", "lutmap"),
    (
        "mapping.c",
        "driver: crates/ast-core/src/mappings/simplify.rs",
    ),
    ("mathmap.c", "mathmap"),
    ("matrixmap.c", "matrix"),
    ("normmap.c", "normmap"),
    ("nullregion.c", "region_nullregion"),
    ("pcdmap.c", "pcdmap"),
    ("permmap.c", "perm"),
    ("pointlist.c", "region_pointlist"),
    ("polygon.c", "region_polygon"),
    ("polymap.c", "polymap"),
    ("prism.c", "region_prism"),
    ("ratemap.c", "ratemap"),
    ("region.c", "region_mod"),
    ("selectormap.c", "selectormap"),
    ("shiftmap.c", "shift"),
    ("slamap.c", "slamap"),
    ("specmap.c", "specmap"),
    ("sphmap.c", "sphmap"),
    ("splinemap.c", "splinemap"),
    ("stc.c", "deferred: StcResourceProfile"),
    ("switchmap.c", "switchmap"),
    ("timemap.c", "timemap"),
    ("tranmap.c", "tranmap"),
    ("unitmap.c", "unit"),
    ("unitnormmap.c", "unitnormmap"),
    ("wcsmap.c", "wcsmap"),
    ("winmap.c", "winmap"),
    ("xphmap.c", "deferred: XphMap"),
    ("zoommap.c", "zoom"),
];

/// Whether a line installs a `MapMerge` or `Simplify` vtab entry.
fn installs_simplify_seam(line: &str) -> bool {
    let line = line.trim_start();
    for prefix in ["vtab->", "mapping->"] {
        if let Some(rest) = line.strip_prefix(prefix) {
            for slot in ["MapMerge", "Simplify"] {
                if let Some(tail) = rest.strip_prefix(slot) {
                    if tail.trim_start().starts_with('=') {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[test]
fn every_c_simplify_seam_is_covered() {
    let universe = audit_universe();
    let mut bad = Vec::new();
    for (file, coverage) in C_SIMPLIFY_SEAMS {
        if let Some(path) = coverage.strip_prefix("driver: ") {
            if !repo_root().join(path.trim()).is_file() {
                bad.push(format!("{file}: driver {path:?} does not exist"));
            }
        } else if let Some(doc) = coverage.strip_prefix("undecided: ") {
            let path = repo_root().join(doc.trim());
            match std::fs::read_to_string(&path) {
                Err(e) => bad.push(format!("{file}: {doc} unreadable: {e}")),
                Ok(text) => {
                    // The document has to carry the question and the artifact,
                    // or "undecided" is an excuse rather than a record.
                    if !text.contains("## The question") {
                        bad.push(format!("{file}: {doc} states no question"));
                    }
                    if !text.contains("frame_in_map_slot.ast") {
                        bad.push(format!(
                            "{file}: {doc} names no fixture demonstrating the \
                             construct"
                        ));
                    }
                }
            }
        } else if let Some(class) = coverage.strip_prefix("deferred: ") {
            let deferred = std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test_deferred_fixtures.rs"),
            )
            .expect("deferred test readable");
            if !deferred.contains(&format!("\"{}\"", class.trim())) {
                bad.push(format!(
                    "{file}: {class:?} is not in DEFERRED_CLASSES, so nothing \
                     counts it as unported"
                ));
            }
        } else if !checklist_path(coverage).is_file() {
            bad.push(format!("{file}: no checklist {coverage}.md"));
        } else if !universe.contains(*coverage) {
            bad.push(format!(
                "{file}: checklist {coverage}.md exists but {coverage} is not in \
                 the Rust-side audit universe"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "every C class with a MapMerge or Simplify vtab entry needs a \
         checklist, a driver module, a DEFERRED_CLASSES row, or an issue \
         document stating what is undecided:\n  {}",
        bad.join("\n  ")
    );
}

/// Re-derive the C seam set from the submodule when it is present, so a pin
/// move that adds or removes a seam fails here instead of passing unnoticed.
/// Tier 1 runs without the submodule, where the committed list stands alone.
#[test]
fn the_c_seam_list_matches_the_c_source() {
    let src = repo_root().join("reference/starlink-ast/src");
    if !src.is_dir() {
        eprintln!(
            "reference/starlink-ast absent — C seam list checked against the \
             committed copy only. Run with the submodule to re-derive it."
        );
        return;
    }
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(&src).expect("C source dir readable") {
        let path = entry.expect("readable entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("c") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("C source readable");
        if text.lines().any(installs_simplify_seam) {
            found.insert(path.file_name().unwrap().to_str().unwrap().to_string());
        }
    }
    let listed: BTreeSet<String> = C_SIMPLIFY_SEAMS
        .iter()
        .map(|(f, _)| (*f).to_string())
        .collect();
    let added: Vec<&String> = found.difference(&listed).collect();
    let removed: Vec<&String> = listed.difference(&found).collect();
    assert!(
        added.is_empty() && removed.is_empty(),
        "C_SIMPLIFY_SEAMS no longer matches the pinned C source. New seams need \
         coverage; vanished ones need their row deleted.\n  new: {added:?}\n  \
         gone: {removed:?}"
    );
}

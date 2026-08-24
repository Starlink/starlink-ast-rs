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
            Some(cells[4].trim().to_string())
        })
        .collect()
}

fn verdict_is_valid(v: &str) -> bool {
    v == "ported"
        || (v.starts_with("n/a:") && v.len() > "n/a:".len() + 1)
        || (v.starts_with("gap: PLAN.md ") && v.len() > "gap: PLAN.md ".len() + 1)
        // A divergence is a step the port refuses because performing it would
        // reproduce a defect in C's own. The citation is what makes it a
        // finding rather than an opinion, so it is part of the verdict.
        || (v.starts_with("divergence:") && v.contains("docs/issues/c-library-quirks.md"))
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
        "a verdict must be `ported`, `n/a: <reason>`, `gap: PLAN.md <section>`, \
         or `divergence: <reason citing docs/issues/c-library-quirks.md>`: \
         {bad:#?}"
    );
}

/// Every `gap` verdict must name a `PLAN.md` section that exists. A gap citing
/// a section nobody wrote is a gap nobody has to answer for.
#[test]
fn every_gap_verdict_names_a_real_plan_section() {
    let plan = std::fs::read_to_string(repo_root().join("PLAN.md")).expect("PLAN.md readable");
    let mut dangling = Vec::new();
    for stem in audit_universe() {
        let Ok(text) = std::fs::read_to_string(checklist_path(&stem)) else {
            continue; // reported by every_audited_class_has_a_checklist
        };
        for (i, v) in verdicts(&text).iter().enumerate() {
            let Some(section) = v.strip_prefix("gap: PLAN.md ") else {
                continue;
            };
            let section = section.trim();
            // A section is cited as `§10.1` and written as `### 10.1 …`.
            let number = section.trim_start_matches('§');
            if !plan.contains(&format!("### {number} ")) {
                dangling.push(format!("{stem} row {}: PLAN.md has no {section:?}", i + 1));
            }
        }
    }
    assert!(
        dangling.is_empty(),
        "a `gap` verdict must cite a PLAN.md section that exists and says what \
         would expose the gap: {dangling:#?}"
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

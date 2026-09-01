//! The audit checklists' citations have to still designate what they claim.
//!
//! A row cites a source line and, once the file is repaired, the name of the
//! item at that line. The name is what makes a stale number detectable: the
//! two have to agree, and this test is where they are made to.
//!
//! A checklist opts in by carrying `Citations: verified against pin <sha>` in
//! its header. The number of files that have not opted in yet is counted in
//! `test_verification_ledger.rs`, so the remainder is a falling number rather
//! than a mode in which the check does not run.

use audit_tools::Resolution;
use std::path::{Path, PathBuf};

/// The marker a repaired checklist carries in its header.
const REPAIRED: &str = "Citations: verified against pin 3e60abaa";

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

/// Every checklist, excluding the directory's README.
fn checklists() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(audit_dir())
        .expect("audit dir readable")
        .map(|e| e.expect("readable entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter(|p| p.file_name().and_then(|f| f.to_str()) != Some("README.md"))
        .collect();
    out.sort();
    out
}

fn is_repaired(text: &str) -> bool {
    text.lines().take(12).any(|l| l.contains(REPAIRED))
}

/// Resolve a citation's file name to the one file it designates.
fn resolve(file: &str) -> Resolution {
    audit_tools::resolve_source(&repo_root(), file)
}

/// Whether the pinned C source is checked out, which decides whether a C
/// citation is allowed to resolve to nothing.
fn c_source_present() -> bool {
    repo_root().join("reference/starlink-ast/src").is_dir()
}

/// Every citation in a repaired checklist names the item that is there.
#[test]
fn every_repaired_citation_resolves_to_the_item_it_names() {
    let mut bad = Vec::new();
    for path in checklists() {
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        if !is_repaired(&text) {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        for (i, line) in text.lines().enumerate() {
            for (cit, _) in audit_tools::citations(line) {
                let Resolution::Found(src_path) = resolve(&cit.file) else {
                    // Reported by every_repaired_citation_resolves_to_exactly_one_file.
                    continue;
                };
                let src = std::fs::read_to_string(&src_path).expect("source readable");
                let Some(claimed) = cit.item.as_deref() else {
                    bad.push(format!(
                        "{name}:{}: `{}:{}` carries no (fn …) and the file is repaired",
                        i + 1,
                        cit.file,
                        cit.start
                    ));
                    continue;
                };
                let actual = if cit.file.ends_with(".rs") {
                    audit_tools::enclosing_rust_item(&src, cit.start)
                } else {
                    audit_tools::enclosing_c_function(&src, cit.start)
                };
                if actual.as_deref() != Some(claimed) {
                    bad.push(format!(
                        "{name}:{}: `{}:{}` claims `{claimed}`, line is in {}",
                        i + 1,
                        cit.file,
                        cit.start,
                        actual.as_deref().unwrap_or("no item")
                    ));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "a citation no longer designates what it claims. Re-anchor the row \
         against the code it describes; do not remap the number.\n  {}",
        bad.join("\n  ")
    );
}

/// A repaired checklist may not carry a citation that designates no single
/// file, because such a citation is unfalsifiable: the gate above skips it.
///
/// A bare `simplify.rs` names two files in this tree and a bare `mod.rs` names
/// thirteen, so a citation to one of those has to carry enough leading path
/// components to single one out.
#[test]
fn every_repaired_citation_resolves_to_exactly_one_file() {
    let mut bad = Vec::new();
    for path in checklists() {
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        if !is_repaired(&text) {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        for (i, line) in text.lines().enumerate() {
            for (cit, _) in audit_tools::citations(line) {
                match resolve(&cit.file) {
                    Resolution::Found(_) => {}
                    Resolution::Ambiguous(paths) => bad.push(format!(
                        "{name}:{}: `{}` names {} files — qualify it, as in `{}`",
                        i + 1,
                        cit.file,
                        paths.len(),
                        paths[0]
                            .iter()
                            .rev()
                            .take(2)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .map(|c| c.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("/"),
                    )),
                    Resolution::Missing => {
                        // With the submodule absent a C citation cannot be
                        // opened, and tier 1 has to run there. A Rust citation
                        // has no such excuse.
                        if cit.file.ends_with(".rs") || c_source_present() {
                            bad.push(format!("{name}:{}: no such file {}", i + 1, cit.file));
                        }
                    }
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "a citation designates no single file:\n  {}",
        bad.join("\n  ")
    );
}

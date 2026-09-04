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
///
/// The pin is part of the marker on purpose: moving the submodule invalidates
/// every line number in every repaired checklist, so the pin has to be
/// re-stated deliberately, in the same change that re-anchors the citations.
const REPAIRED: &str = "Citations: verified against pin 756d5ac0";

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

/// The marker a checklist carries once its rows classify their C steps.
const CLASSIFIED: &str = "Extension points: classified";

fn is_classified(text: &str) -> bool {
    text.lines().take(12).any(|l| l.contains(CLASSIFIED))
}

/// The cells of a checklist table row, trimmed, or `None` for a non-row.
fn row_cells(line: &str) -> Option<Vec<&str>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let cells: Vec<&str> = trimmed
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    cells.first()?.parse::<u32>().ok().map(|_| cells)
}

/// The Rust module a checklist audits, from its `Rust:` header line.
///
/// The class's own module is where its trait methods live, so it is what says
/// whether a cited helper hangs off one of them.
fn class_module(text: &str) -> Option<PathBuf> {
    let line = text
        .lines()
        .take(12)
        .find_map(|l| l.trim().strip_prefix("Rust:"))?;
    let path = repo_root().join(line.trim());
    path.is_file().then_some(path)
}

/// A classified checklist states, per row, whether the C step is an extension
/// point, and an architecture row's Rust site hangs off a trait method.
///
/// The classification is the mechanical test from `docs/porting-hazards.md` H1:
/// does a generic driver in another file dispatch to this through the vtab? A
/// step that answers yes and lands somewhere no per-class seam reaches is the
/// shape of the two most expensive mistakes this port has made. The site itself
/// need not be the trait method — a class's merge body lives in
/// `mappings/simplify.rs` and the trait method calls it — so a site the class's
/// trait methods reach counts.
#[test]
fn every_architecture_row_lands_on_a_trait_method() {
    let mut bad = Vec::new();
    for path in checklists() {
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        if !is_classified(&text) {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        for (i, line) in text.lines().enumerate() {
            let Some(cells) = row_cells(line) else {
                continue;
            };
            // | # | C step | C site | Rust site | extension point | verdict |
            if cells.len() != 6 {
                bad.push(format!(
                    "{name}:{}: a classified checklist needs six columns, this row has {}",
                    i + 1,
                    cells.len()
                ));
                continue;
            }
            let point = cells[4];
            if point != "architecture" && point != "ceremony" {
                bad.push(format!(
                    "{name}:{}: extension point is `{point}`, expected `architecture` or `ceremony`",
                    i + 1
                ));
                continue;
            }
            if point != "architecture" || cells[5] != "ported" {
                continue;
            }
            let sites = audit_tools::citations(cells[3]);
            if sites.is_empty() {
                bad.push(format!(
                    "{name}:{}: an architecture row must cite its Rust site, this one has `{}`",
                    i + 1,
                    cells[3]
                ));
                continue;
            }
            let class_src = class_module(&text)
                .map(|p| std::fs::read_to_string(p).expect("class module readable"))
                .unwrap_or_default();
            let mut verdicts = Vec::new();
            for (cit, _) in &sites {
                let Resolution::Found(src_path) = resolve(&cit.file) else {
                    // Reported by every_repaired_citation_resolves_to_exactly_one_file.
                    verdicts.push(Ok(()));
                    break;
                };
                let src = std::fs::read_to_string(&src_path).expect("source readable");
                let on_trait = audit_tools::enclosing_rust_block(&src, cit.start)
                    .is_some_and(|header| audit_tools::declares_trait_methods(&header));
                let owned = || {
                    audit_tools::enclosing_rust_item(&src, cit.start).is_some_and(|item| {
                        audit_tools::reached_from_trait_method(&class_src, &src, &item)
                    })
                };
                if on_trait || owned() {
                    verdicts.push(Ok(()));
                } else {
                    verdicts.push(Err(format!(
                        "{}:{} is in {}",
                        cit.file,
                        cit.start,
                        audit_tools::enclosing_rust_item(&src, cit.start)
                            .unwrap_or_else(|| "no item".to_string())
                    )));
                }
            }
            if !verdicts.iter().any(Result::is_ok) {
                bad.push(format!(
                    "{name}:{}: architecture step ported to {} — a step a generic \
                     driver dispatches to belongs on a trait method, or on code \
                     one of the class's trait methods calls",
                    i + 1,
                    verdicts
                        .into_iter()
                        .filter_map(Result::err)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "a classified checklist's rows have to say which C steps are extension \
         points, and an architecture step has to have landed on a trait \
         method:\n  {}",
        bad.join("\n  ")
    );
}

/// Every C step inside the seam body a repaired checklist audits is covered by
/// one of that checklist's rows.
///
/// The bodies considered are the ones the checklist's own table rows cite in the
/// C file its header names, so the gate needs no class-to-function map: putting
/// a row's C site inside a body commits the checklist to enumerating that body.
/// Prose elsewhere in the file may cite another class's code as evidence without
/// taking on its steps. A step the checklist deliberately does not enumerate is
/// listed under an `Uncited C steps:` heading with a reason, which is a named
/// exclusion rather than a silent one.
#[test]
fn every_c_step_in_a_cited_body_is_covered_by_a_row() {
    let mut bad = Vec::new();
    for path in checklists() {
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        if !is_repaired(&text) {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        // The C file this checklist audits, by base name.
        let Some(c_source) = text
            .lines()
            .take(12)
            .find_map(|l| l.trim().strip_prefix("C source:"))
            .and_then(|p| p.trim().rsplit('/').next())
        else {
            bad.push(format!("{name}: header names no `C source:`"));
            continue;
        };

        // The ranges the table rows cite in that file.
        let mut ranges: Vec<(String, usize, usize)> = Vec::new();
        for line in text.lines() {
            let Some(cells) = row_cells(line) else {
                continue;
            };
            for (cit, _) in audit_tools::citations(cells[2]) {
                if cit.file == c_source {
                    ranges.push((cit.file, cit.start, cit.end.unwrap_or(cit.start)));
                }
            }
        }
        let excused: Vec<&str> = text
            .lines()
            .skip_while(|l| !l.starts_with("Uncited C steps:"))
            .skip(1)
            .take_while(|l| l.starts_with("- "))
            .collect();

        let mut bodies: Vec<(String, String)> = Vec::new();
        for (file, start, _) in &ranges {
            let Resolution::Found(src_path) = resolve(file) else {
                continue;
            };
            let src = std::fs::read_to_string(&src_path).expect("source readable");
            if let Some(func) = audit_tools::enclosing_c_function(&src, *start) {
                if !bodies.iter().any(|(f, g)| f == file && *g == func) {
                    bodies.push((file.clone(), func));
                }
            }
        }

        for (file, func) in bodies {
            let Resolution::Found(src_path) = resolve(&file) else {
                continue; // reported by the resolution gate
            };
            let src = std::fs::read_to_string(&src_path).expect("source readable");
            for (line, comment) in audit_tools::c_step_comments(&src, &func) {
                let covered = ranges
                    .iter()
                    .any(|(f, s, e)| *f == file && *s <= line && line <= *e);
                let excused = excused
                    .iter()
                    .any(|l| l.contains(&format!("{file}:{line}")));
                if !covered && !excused {
                    bad.push(format!(
                        "{name}: {file}:{line} in {func} — \"{comment}\" is not covered \
                         by any row and is not listed under `Uncited C steps:`"
                    ));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "C steps present in a cited body and absent from the checklist:\n  {}",
        bad.join("\n  ")
    );
}

/// Every checklist declares both markers, so the four gates above have no
/// checklist they silently skip.
///
/// The markers were opt-in while the citations were being anchored, and a
/// ledger row counted the files that had not opted in. They are now the entry
/// condition: a checklist added later fails here until its citations are
/// anchored to the current pin and its rows classified, which is the point at
/// which its claims become checkable.
#[test]
fn every_checklist_declares_both_markers() {
    let mut missing = Vec::new();
    for path in checklists() {
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !is_repaired(&text) {
            missing.push(format!("{name}: no `{REPAIRED}` line in the header"));
        }
        if !is_classified(&text) {
            missing.push(format!("{name}: no `{CLASSIFIED}` line in the header"));
        }
    }
    assert!(
        missing.is_empty(),
        "checklists that opt out of the citation gates:\n  {}\n\
         Anchor the citations against the current pin and classify each row's \
         extension point, then declare both markers.",
        missing.join("\n  ")
    );
}

/// No checklist row may record a gap.
///
/// A `gap:` verdict says the C step has no Rust counterpart. The verdicts that
/// remain valid — `ported`, `n/a: <reason>` and
/// `divergence: <quirks citation>` — each say the behaviour is accounted for.
#[test]
fn no_row_is_left_at_gap() {
    let mut gaps = Vec::new();
    for path in checklists() {
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        for (i, line) in text.lines().enumerate() {
            let Some(cells) = row_cells(line) else {
                continue;
            };
            let verdict = cells.last().map(|c| c.trim()).unwrap_or("");
            if verdict.starts_with("gap:") {
                gaps.push(format!("{name}:{}: {verdict}", i + 1));
            }
        }
    }
    assert!(
        gaps.is_empty(),
        "checklist rows still recording a gap:\n  {}",
        gaps.join("\n  ")
    );
}

/// No checklist elides the file name from a citation.
///
/// A citation is `file.c:123`, and `audit_tools::citations` finds one by walking
/// back from each colon and requiring what precedes it to end in `.c`, `.h` or
/// `.rs`. A row that writes the second of two citations as a bare `:456`,
/// leaving the reader to carry the file name forward, is therefore invisible to
/// every other test in this file: it is neither resolved, nor checked against
/// the item it claims, nor counted as covering a C step.
///
/// Six of them existed, and had gone stale across two pin moves without any
/// gate noticing, because nothing looked for them. One was actively misleading
/// -- `slamap.md` wrote "C's note at `:2978`" directly after a `slamap.rs`
/// citation, so carrying the nearest file name forward gave the wrong file.
///
/// The rule is therefore that every citation names its file, however repetitive
/// that reads.
#[test]
fn no_citation_elides_its_file_name() {
    let mut bad = Vec::new();
    for path in checklists() {
        let text = std::fs::read_to_string(&path).expect("checklist readable");
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        for (i, line) in text.lines().enumerate() {
            let spans: Vec<_> = audit_tools::citations(line)
                .into_iter()
                .map(|(_, range)| range)
                .collect();
            for (colon, _) in line.match_indices(':') {
                let next_is_digit = line[colon + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit());
                if !next_is_digit {
                    continue;
                }
                if spans.iter().any(|r| r.contains(&colon)) {
                    continue;
                }
                let from = colon.saturating_sub(30);
                bad.push(format!(
                    "{name}:{}: `{}` is a line reference the citation parser \
                     cannot see — write the file name out",
                    i + 1,
                    &line[from..(colon + 8).min(line.len())]
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "citations with the file name left out, which no other test in this \
         file can check:\n  {}",
        bad.join("\n  ")
    );
}

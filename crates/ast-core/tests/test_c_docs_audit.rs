//! Gates for the C documentation audit
//! (`docs/superpowers/specs/2026-10-19-c-docs-audit-design.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use audit_tools::{LedgerRow, parse_headers_tsv, parse_ledger};

/// Rows not yet reviewed. It may only go down; it reaches zero when the
/// audit is complete.
const UNREVIEWED_ROWS: usize = 391;

fn audit_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/audits/c-docs")
}

fn headers() -> Vec<(String, String, String)> {
    let text = std::fs::read_to_string(audit_dir().join("c-headers.tsv")).unwrap();
    parse_headers_tsv(&text)
        .into_iter()
        .map(|(k, n, _, f)| (k.to_string(), n, f))
        .collect()
}

fn ledger() -> Vec<LedgerRow> {
    let text = std::fs::read_to_string(audit_dir().join("ledger.tsv")).unwrap();
    parse_ledger(&text).unwrap_or_else(|e| panic!("ledger.tsv: {e}"))
}

#[test]
fn every_header_has_exactly_one_row() {
    let want: BTreeSet<_> = headers().into_iter().collect();
    let mut got = BTreeMap::new();
    for r in ledger() {
        *got.entry((r.kind.clone(), r.name.clone(), r.file.clone()))
            .or_insert(0) += 1;
    }
    let missing: Vec<_> = want.iter().filter(|k| !got.contains_key(*k)).collect();
    let extra: Vec<_> = got.keys().filter(|k| !want.contains(*k)).collect();
    let dup: Vec<_> = got
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(k, _)| k)
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty() && dup.is_empty(),
        "missing {missing:?}\nextra {extra:?}\nduplicated {dup:?}"
    );
}

#[test]
fn reviewed_rows_are_well_formed() {
    let rows = ledger();
    let mapped: BTreeSet<_> = rows
        .iter()
        .filter(|r| !r.rust_item.is_empty())
        .map(|r| r.name.clone())
        .collect();
    for r in rows.iter().filter(|r| r.reviewed) {
        let what = format!("{} {} ({})", r.kind, r.name, r.file);
        assert!(
            r.rust_item.is_empty() != r.reason.is_none(),
            "{what}: needs exactly one of rust_item and reason"
        );
        if let Some(reason) = &r.reason {
            assert!(
                ["c-api-only", "not-ported", "merged"].contains(&reason.as_str()),
                "{what}: reason {reason}"
            );
            if reason == "merged" {
                assert!(
                    mapped.contains(r.note.trim()),
                    "{what}: merged into {:?}, which has no Rust item",
                    r.note
                );
            }
        }
    }
}

#[test]
fn the_unreviewed_count_only_goes_down() {
    let n = ledger().iter().filter(|r| !r.reviewed).count();
    assert_eq!(
        n, UNREVIEWED_ROWS,
        "lower UNREVIEWED_ROWS to {n} when rows are reviewed; it may never rise"
    );
}

/// The text of every Rust source file in ast-core.
fn source_files() -> Vec<String> {
    let mut out = Vec::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    out
}

/// Every alias in ast-core's source, as (alias, item, owner).
fn source_aliases() -> Vec<(String, String, String)> {
    source_files()
        .iter()
        .flat_map(|src| audit_tools::doc_aliases(src))
        .collect()
}

#[test]
fn every_mapped_row_has_its_alias_on_its_item() {
    let aliases_by_name: BTreeMap<String, String> =
        parse_headers_tsv(&std::fs::read_to_string(audit_dir().join("c-headers.tsv")).unwrap())
            .into_iter()
            .map(|(_, name, alias, _)| (name, alias))
            .collect();
    let found: BTreeSet<_> = source_aliases().into_iter().collect();
    // rustdoc rejects an alias equal to the item's own name, so a row whose
    // Rust item is a public top-level type or trait with the C name is checked
    // against the declarations instead.
    let types: BTreeSet<String> = source_files()
        .iter()
        .flat_map(|src| audit_tools::top_level_types(src))
        .collect();
    let mut wanted = BTreeSet::new();
    let mut missing = Vec::new();
    for r in ledger() {
        let Some(alias) = aliases_by_name.get(&r.name) else {
            // `every_header_has_exactly_one_row` reports a row with no header.
            if !r.rust_item.is_empty() {
                missing.push(format!("{}: no entry in c-headers.tsv", r.name));
            }
            continue;
        };
        for item in &r.rust_item {
            let (owner, name) = item.split_once("::").unwrap_or((item, item));
            if audit_tools::alias_satisfied_by_name(item, alias, &types) {
                continue;
            }
            let key = (alias.clone(), name.to_string(), owner.to_string());
            if !found.contains(&key) {
                missing.push(format!(
                    "{} -> {item}: no #[doc(alias = \"{alias}\")] on it",
                    r.name
                ));
            }
            wanted.insert(key);
        }
    }
    let unlisted: Vec<_> = found.iter().filter(|k| !wanted.contains(*k)).collect();
    assert!(
        missing.is_empty() && unlisted.is_empty(),
        "missing:\n{}\naliases with no ledger row: {unlisted:?}",
        missing.join("\n")
    );
}

/// Top-level modules whose rendered docs still carry C references, each
/// with why. The list only shrinks; `a_clean_module_cannot_stay_listed`
/// fails when a listed module is clean.
const C_CITATIONS_UNCLEANED: &[(&str, &str)] = &[
    (
        "(crate root)",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "call_counts",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "channel",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "fitschan",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "fitstable",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "frames",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "keymap",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "pixel",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "regions",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
    (
        "table",
        "C source citations in public docs; cleaned in its subsystem's review task",
    ),
];

/// The C references in each top-level module's rendered pages. Builds the
/// docs into its own target directory, so it never reads docs built by
/// someone else and does not wait on the outer `cargo test`'s lock. Only
/// `ast_core/` is scanned: the source-view pages under `src/`, which show
/// `//` comments, are not user-facing documentation.
fn rendered_c_references() -> (BTreeMap<String, Vec<String>>, BTreeMap<String, usize>) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = manifest.join("../../target/c-docs-gate");
    let status = std::process::Command::new(env!("CARGO"))
        .args(["doc", "-q", "-p", "ast-core", "--no-deps"])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(&manifest)
        .status()
        .unwrap();
    assert!(status.success(), "cargo doc failed");
    let public: BTreeSet<String> =
        parse_headers_tsv(&std::fs::read_to_string(audit_dir().join("c-headers.tsv")).unwrap())
            .into_iter()
            .map(|(_, _, alias, _)| alias)
            .collect();
    // `target/doc`, or `target/<triple>/doc` when a build target is set.
    let mut roots = vec![target.join("doc")];
    roots.extend(
        std::fs::read_dir(&target)
            .unwrap()
            .map(|e| e.unwrap().path().join("doc")),
    );
    let docs = roots
        .into_iter()
        .map(|d| d.join("ast_core"))
        .filter(|d| d.is_dir())
        .max_by_key(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok())
        .expect("rendered ast_core docs");
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pages: BTreeMap<String, usize> = BTreeMap::new();
    let mut stack = vec![docs.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "html") {
                let rel = p.strip_prefix(&docs).unwrap();
                let module = match rel.components().count() {
                    1 => "(crate root)".to_string(),
                    _ => rel
                        .components()
                        .next()
                        .unwrap()
                        .as_os_str()
                        .to_string_lossy()
                        .into_owned(),
                };
                let refs =
                    audit_tools::c_references(&std::fs::read_to_string(&p).unwrap(), &public);
                *pages.entry(module.clone()).or_default() += 1;
                out.entry(module)
                    .or_default()
                    .extend(refs.into_iter().map(|r| format!("{}: {r}", rel.display())));
            }
        }
    }
    out.retain(|_, v| !v.is_empty());
    (out, pages)
}

/// The top-level public modules `ast-core`'s `lib.rs` declares.
fn public_modules() -> Vec<String> {
    let lib = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .unwrap();
    lib.lines()
        .filter_map(|l| l.strip_prefix("pub mod "))
        .map(|m| m.trim_end_matches(';').trim().to_string())
        .collect()
}

/// Fails unless the scan read at least one page of every public module and
/// the crate root's `index.html`, so an empty or truncated documentation
/// directory cannot pass as clean.
fn assert_scan_covered_the_docs(pages: &BTreeMap<String, usize>) {
    let modules = public_modules();
    assert!(!modules.is_empty(), "no public modules found in lib.rs");
    let missing: Vec<_> = modules
        .iter()
        .filter(|m| pages.get(m.as_str()).copied().unwrap_or(0) == 0)
        .collect();
    assert!(missing.is_empty(), "no rendered pages read for {missing:?}");
    assert!(
        pages.get("(crate root)").copied().unwrap_or(0) > 0,
        "crate root pages (index.html) not read"
    );
}

fn listed_uncleaned() -> BTreeSet<&'static str> {
    C_CITATIONS_UNCLEANED.iter().map(|(m, _)| *m).collect()
}

#[test]
fn rendered_docs_carry_no_c_references() {
    let (refs, pages) = rendered_c_references();
    assert_scan_covered_the_docs(&pages);
    let listed = listed_uncleaned();
    let unlisted: Vec<_> = refs
        .iter()
        .filter(|(m, _)| !listed.contains(m.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "C references in rendered docs: {unlisted:#?}"
    );
}

#[test]
fn a_clean_module_cannot_stay_listed() {
    let (refs, pages) = rendered_c_references();
    assert_scan_covered_the_docs(&pages);
    let clean: Vec<_> = listed_uncleaned()
        .into_iter()
        .filter(|m| !refs.contains_key(*m))
        .collect();
    assert!(
        clean.is_empty(),
        "now clean; remove from C_CITATIONS_UNCLEANED: {clean:?}"
    );
}

//! Gates for the C documentation audit
//! (`docs/superpowers/specs/2026-10-19-c-docs-audit-design.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use audit_tools::{LedgerRow, parse_headers_tsv, parse_ledger};

/// Rows not yet reviewed. It may only go down; it reaches zero when the
/// audit is complete.
const UNREVIEWED_ROWS: usize = 517;

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

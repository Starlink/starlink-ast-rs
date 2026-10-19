//! The C documentation audit ledger (`docs/audits/c-docs/ledger.tsv`).
//!
//! One row per documentation header in the C library.
//! The columns are `kind name file rust_item reason reviewed note`, tab
//! separated, after a header line.
//! `rust_item` lists the Rust items that document the header, separated by
//! `;`, and is empty when the header has none.
//! A row whose `note` is empty may omit the final tab.

/// The reasons a header may have no Rust item.
const REASONS: [&str; 3] = ["c-api-only", "not-ported", "merged"];

/// One ledger row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRow {
    /// `class`, `function` or `attribute`.
    pub kind: String,
    /// The name in the C header.
    pub name: String,
    /// The C source file holding the header.
    pub file: String,
    /// `Owner::item` or `Owner` for each Rust item; empty when none.
    pub rust_item: Vec<String>,
    /// Why there is no Rust item: `c-api-only`, `not-ported` or `merged`.
    pub reason: Option<String>,
    /// Whether the row has been reviewed.
    pub reviewed: bool,
    /// Free-form note; for `merged` rows, the name merged into.
    pub note: String,
}

/// Parse the ledger text. An error names the offending line (1-based).
pub fn parse_ledger(text: &str) -> Result<Vec<LedgerRow>, String> {
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate().skip(1) {
        let n = i + 1;
        if line.trim().is_empty() {
            continue;
        }
        let mut cols: Vec<&str> = line.split('\t').collect();
        // Editors and the trailing-whitespace hook strip an empty final cell.
        if cols.len() == 6 {
            cols.push("");
        }
        let [kind, name, file, rust_item, reason, reviewed, note] = cols[..] else {
            return Err(format!(
                "line {n}: expected 7 columns, found {}",
                cols.len()
            ));
        };
        let reviewed = match reviewed {
            "yes" => true,
            "no" => false,
            other => {
                return Err(format!(
                    "line {n}: reviewed must be yes or no, not {other:?}"
                ));
            }
        };
        let reason = match reason {
            "" => None,
            r if REASONS.contains(&r) => Some(r.to_string()),
            other => return Err(format!("line {n}: unknown reason {other:?}")),
        };
        rows.push(LedgerRow {
            kind: kind.to_string(),
            name: name.to_string(),
            file: file.to_string(),
            rust_item: rust_item
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            reason,
            reviewed,
            note: note.to_string(),
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "kind\tname\tfile\trust_item\treason\treviewed\tnote\n";

    #[test]
    fn parses_a_well_formed_line() {
        let text = format!("{HEAD}function\tastFoo\tfoo.c\tFoo::foo;Foo\t\tyes\tn\n");
        let rows = parse_ledger(&text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rust_item, ["Foo::foo", "Foo"]);
        assert_eq!(rows[0].reason, None);
        assert!(rows[0].reviewed);
        assert_eq!(rows[0].note, "n");
    }

    #[test]
    fn parses_a_reason_and_empty_items() {
        let text = format!("{HEAD}class\tPlot\tplot.c\t\tnot-ported\tno\t\n");
        let rows = parse_ledger(&text).unwrap();
        assert!(rows[0].rust_item.is_empty());
        assert_eq!(rows[0].reason.as_deref(), Some("not-ported"));
        assert!(!rows[0].reviewed);
    }

    #[test]
    fn accepts_a_missing_empty_note() {
        let text = format!("{HEAD}class\tA\ta.c\t\t\tno\n");
        assert_eq!(parse_ledger(&text).unwrap()[0].note, "");
    }

    #[test]
    fn rejects_a_wrong_column_count() {
        let text = format!("{HEAD}class\tPlot\tplot.c\n");
        let e = parse_ledger(&text).unwrap_err();
        assert!(e.contains("line 2") && e.contains("7 columns"), "{e}");
    }

    #[test]
    fn rejects_an_unknown_reviewed_value() {
        let text = format!("{HEAD}class\tA\ta.c\t\t\tmaybe\t\n");
        let e = parse_ledger(&text).unwrap_err();
        assert!(e.contains("line 2") && e.contains("reviewed"), "{e}");
    }

    #[test]
    fn rejects_an_unknown_reason() {
        let text = format!("{HEAD}class\tA\ta.c\t\tbecause\tyes\t\n");
        let e = parse_ledger(&text).unwrap_err();
        assert!(e.contains("line 2") && e.contains("reason"), "{e}");
    }
}

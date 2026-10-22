//! The C documentation audit ledger (`docs/audits/c-docs/ledger.tsv`).
//!
//! One row per documentation header in the C library.
//! The columns are `kind name file rust_item reason reviewed note`, tab
//! separated, after a header line.
//! `rust_item` lists the Rust items that document the header, separated by
//! `;`, and is empty when the header has none.
//! A row whose `note` is empty may omit the final tab.
//! The header line must name those columns, in that order.

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

/// The header line every ledger starts with.
const HEADER: &str = "kind\tname\tfile\trust_item\treason\treviewed\tnote";

/// The owner and the item name in a ledger `rust_item`.
///
/// `Owner::item` names a member of `Owner`, and a longer path
/// (`module::Owner::item`) names it by its last two segments, which are what a
/// `#[doc(alias)]` sits on and inside. A bare `Owner` is its own item.
pub fn owner_and_name(item: &str) -> (&str, &str) {
    match item.rsplit_once("::") {
        Some((path, name)) => (path.rsplit("::").next().unwrap_or(path), name),
        None => (item, item),
    }
}

/// Parse the ledger text. An error names the offending line (1-based).
pub fn parse_ledger(text: &str) -> Result<Vec<LedgerRow>, String> {
    let first = text.lines().next().unwrap_or("");
    if first != HEADER {
        return Err(format!(
            "line 1: expected the header {HEADER:?}, found {first:?}"
        ));
    }
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
                "line {n}: expected 6 or 7 columns, found {}",
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
    fn rejects_a_row_with_too_many_columns() {
        let text = format!("{HEAD}class\tA\ta.c\t\t\tno\tn\textra\n");
        let e = parse_ledger(&text).unwrap_err();
        assert!(e.contains("line 2") && e.contains("6 or 7 columns"), "{e}");
        assert!(e.contains("found 8"), "{e}");
    }

    #[test]
    fn rejects_a_wrong_header_line() {
        let text = "kind\tname\tfile\treason\trust_item\treviewed\tnote\nclass\tA\ta.c\t\t\tno\t\n";
        let e = parse_ledger(text).unwrap_err();
        assert!(e.contains("line 1") && e.contains("header"), "{e}");
        // A first data row mistaken for the header is not skipped silently.
        let e = parse_ledger("class\tA\ta.c\t\t\tno\t\n").unwrap_err();
        assert!(e.contains("line 1"), "{e}");
        assert!(parse_ledger("").is_err());
    }

    #[test]
    fn an_item_path_is_split_at_its_last_two_segments() {
        assert_eq!(owner_and_name("Foo"), ("Foo", "Foo"));
        assert_eq!(owner_and_name("Foo::bar"), ("Foo", "bar"));
        assert_eq!(owner_and_name("a::Foo::bar"), ("Foo", "bar"));
        assert_eq!(owner_and_name("a::b::Foo::bar"), ("Foo", "bar"));
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

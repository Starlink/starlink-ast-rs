# C documentation audit: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Map every public C documentation header to the Rust item that documents it (or a reason it has none), move the content a Rust user needs into `starlink-ast`'s rustdoc with C-name search aliases, and move C source citations out of the rendered docs — all held by gates that cannot pass without checking.

**Architecture:** `audit-tools` gains a C-header extractor and a ledger parser. Its output, `docs/audits/c-docs/c-headers.tsv`, and the hand-maintained `docs/audits/c-docs/ledger.tsv` are committed. One meta test file in `starlink-ast` holds the five gates: coverage, row form, alias placement, rendered-docs cleanliness, and a review ratchet. The documentation itself is then reviewed subsystem by subsystem with one fixed procedure.

**Tech Stack:** Rust 2024 (`audit-tools`, `starlink-ast` meta tests), rustdoc HTML output, bash for the freshness script.

**Spec:** `docs/superpowers/specs/2026-10-19-c-docs-audit-design.md`

## Global Constraints

- Headers in scope: `*class++`, `*++`, `*att++` and `c++` blocks in `reference/starlink-ast/src/*.c`; `f++` blocks excluded; inside a block only `*` and `c ` lines are read.
- Ledger reasons are exactly `c-api-only`, `not-ported`, `merged`.
- Every row with a Rust item has `#[doc(alias = "<alias>")]` on that item; an alias with no row fails.
- No C file or line references and no internal C names (an `ast<Name>` not in `c-headers.tsv`) in rendered HTML under `starlink_ast/` (source-view pages under `src/` are not rendered docs and are not scanned).
- Content rules are the spec's "Content rules" section, verbatim; the review procedure below applies them.
- Docs describe the Rust code as it is; a C behavior the port lacks gets a ledger note and a `PLAN.md` row, never prose describing code that does not exist.
- No skip modes: every gate fails hard; exclusions are named lists with reasons and companion tests.
- One sentence per line in Markdown; American spelling; fmt and clippy clean with and without `c-oracle`.
- The Bash tool runs zsh: use arrays or script files, not word-split variables.
- Never stage `reference/starlink-ast`.

## Review Focus

1. A header whose name has a suffix or a generic marker (`Label(axis)`, `Digits/Digits(axis)`, `astMapGet0<X>`): the extractor must normalize it and the alias gate must accept the stem on every family member — Task 1 test `names_are_normalized`, Task 3 test `a_family_alias_on_each_member_passes`.
2. The same C name documented in two files: the ledger key includes the file, so both rows exist — Task 1 test `key_includes_the_file`.
3. A doc alias inside a `macro_rules!` expansion or on a re-exported type alias (`pub type StcResourceProfile = Stc<…>`): the alias gate must find it on `type` items — Task 3 test `an_alias_on_a_type_alias_passes`.
4. A rendered page that quotes a C file name legitimately, such as a `.c` inside a URL: the scanner must flag it so it is rewritten, not tolerated — Task 4 test `a_c_file_name_anywhere_in_text_is_flagged`.
5. A module in `C_CITATIONS_UNCLEANED` that becomes clean: the companion test must fail so the list shrinks — Task 4 test `a_clean_module_cannot_stay_listed`.

## Row review procedure

Tasks 5–9 each apply this procedure to a set of C files. For each C file in the set, in order:

1. **Print the headers.** `cargo run -q -p audit-tools --bin c-docs -- show <file.c>` prints every in-scope header of that file in its C edition.
2. **For each header, decide the row:**
   - a Rust item exists → fill `rust_item` (`Owner::item` or `Owner`, several separated by `;` for a `<X>` family);
   - otherwise one of `c-api-only`, `not-ported`, `merged` (a `merged` row names the C name whose item covers it in `note`).
3. **Write the docs** on the Rust item following the spec's content rules for that kind (class → struct doc; function → method doc with `# Errors`; attribute → getter doc with "Default: …", setter/test/clear linking to it).
4. **Add the alias** `#[doc(alias = "<alias>")]` directly above the item (after its doc comment, before other attributes is fine), using the alias column of `c-headers.tsv`.
5. **Add `# See also`** naming the public C function only when the correspondence is not obvious from the Rust name.
6. **Record differences.** If the C header documents a default or behavior the port lacks, write the docs for the port, put the difference in `note`, and add a `PLAN.md` row naming what would expose it.
7. **Move citations.** In every public item and public module doc in the Rust files touched, move C file/line references and internal C function names into `//` comments next to the code they describe.
8. **Mark the row** `reviewed = yes`, lower `UNREVIEWED_ROWS` in `test_c_docs_audit.rs` to the new count, and remove any module from `C_CITATIONS_UNCLEANED` that is now clean.
9. **Run the gates and the doc build:** `cargo test -p starlink-ast --test meta test_c_docs_audit::` and `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps`.

---

### Task 1: Extract the C headers

**Files:**
- Create: `crates/audit-tools/src/c_headers.rs`
- Modify: `crates/audit-tools/src/lib.rs` (declare and re-export the module)
- Create: `crates/audit-tools/src/bin/c-docs.rs`
- Create: `docs/audits/c-docs/c-headers.tsv` (generated)
- Create: `tools/oracle_fixtures/check_c_headers_fresh.sh`

**Interfaces:**
- Produces:
  ```rust
  pub enum HeaderKind { Class, Function, Attribute }   // c++ headers are Function
  pub struct CHeader {
      pub kind: HeaderKind,
      pub name: String,   // normalized: suffix "(…)" and "/…" removed; "<X>" kept
      pub alias: String,  // name with any "<…>" removed
      pub file: String,   // e.g. "permmap.c"
      pub text: String,   // the C edition of the header body
  }
  /// "class", "function" or "attribute", as the TSV and the ledger write it.
  impl std::fmt::Display for HeaderKind { /* … */ }
  pub fn c_headers(src: &str, file: &str) -> Vec<CHeader>;
  pub fn headers_tsv(headers: &[CHeader]) -> String; // "kind\tname\talias\tfile\n" rows, header line first
  pub fn parse_headers_tsv(text: &str) -> Vec<(HeaderKind, String, String, String)>;
  ```
- `c-docs` binary subcommands: `headers <src-dir>` (prints the TSV), `show <file.c> [--src <src-dir>]` (prints each header's C text).

- [ ] **Step 1: Write the failing unit tests** in `c_headers.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      const SRC: &str = "\
  /*
  *class++
  *  Name:
  *     PermMap

  *  Purpose:
  *     Coordinate permutation Mapping.
  *class--
  */
  /*
  *++
  *  Name:
  c     astPermMap
  f     AST_PERMMAP

  *  Purpose:
  *     Create a PermMap.
  f  Fortran only line.
  c  C only line.
  *--
  */
  /*
  *att++
  *  Name:
  *     Digits/Digits(axis)
  *att--
  */
  /*
  *att++
  *  Name:
  *     Label(axis)
  *att--
  */
  /*
  *++
  *  Name:
  c     astMapGet0<X>
  f     AST_MAPGET0<X>
  *--
  */
  /*
  c++
  *  Name:
  *     astToString
  c--
  */
  /*
  f++
  *  Name:
  *     AST_FORTRAN_ONLY
  f--
  */
  ";

      #[test]
      fn every_c_edition_header_is_found() {
          let h = c_headers(SRC, "x.c");
          let names: Vec<_> = h.iter().map(|h| h.name.as_str()).collect();
          assert_eq!(names, ["PermMap", "astPermMap", "Digits", "Label", "astMapGet0<X>", "astToString"]);
          assert_eq!(h[0].kind, HeaderKind::Class);
          assert_eq!(h[1].kind, HeaderKind::Function);
          assert_eq!(h[2].kind, HeaderKind::Attribute);
          assert_eq!(h[5].kind, HeaderKind::Function);
      }

      #[test]
      fn names_are_normalized() {
          let h = c_headers(SRC, "x.c");
          assert_eq!(h[4].alias, "astMapGet0");
          assert_eq!(h[2].alias, "Digits");
      }

      #[test]
      fn the_text_is_the_c_edition() {
          let h = c_headers(SRC, "x.c");
          assert!(h[1].text.contains("C only line."));
          assert!(!h[1].text.contains("Fortran only line."));
          assert!(!h[1].text.contains("AST_PERMMAP"));
      }

      #[test]
      fn key_includes_the_file() {
          let a = c_headers(SRC, "a.c");
          let b = c_headers(SRC, "b.c");
          let tsv = headers_tsv(&[a, b].concat());
          let rows = parse_headers_tsv(&tsv);
          assert_eq!(rows.len(), 12);
          assert_eq!(rows.iter().filter(|r| r.1 == "PermMap").count(), 2);
      }
  }
  ```

  Declare `pub mod c_headers;` in `lib.rs` and `pub use c_headers::{CHeader, HeaderKind, c_headers, headers_tsv, parse_headers_tsv};`.

- [ ] **Step 2: Run them to verify they fail.**
  Run: `cargo test -p audit-tools c_headers`
  Expected: compile errors, `c_headers` not found.

- [ ] **Step 3: Implement.**
  A header starts at a line equal to `*class++`, `*++`, `*att++` or `c++` and ends at the matching `--` line; `f++` blocks are skipped whole.
  Inside, keep lines starting `*` or `c ` (strip the first character and following spaces up to the column the text starts at, preserving relative indentation) and drop lines starting `f `.
  The name is the first non-empty kept line after the `Name:` line, trimmed; normalize by cutting at the first `/`, then at the first `(`; `alias` is the name with every `<…>` removed.
  `headers_tsv` writes a first line `kind\tname\talias\tfile` and one row per header, kinds as `class`, `function`, `attribute`, sorted by file then by position.

- [ ] **Step 4: Run the tests.**
  Run: `cargo test -p audit-tools c_headers`
  Expected: 4 passed.

- [ ] **Step 5: Write the binary** `crates/audit-tools/src/bin/c-docs.rs`:
  - `c-docs headers <src-dir>`: reads every `*.c` in `<src-dir>` in sorted order and prints `headers_tsv`.
  - `c-docs show <file.c> [--src <dir>]` (default `reference/starlink-ast/src`): prints each header of that file as `== <kind> <name>` followed by its `text`.

- [ ] **Step 6: Generate and check the list.**

  ```sh
  mkdir -p docs/audits/c-docs
  cargo run -q -p audit-tools --bin c-docs -- headers reference/starlink-ast/src > docs/audits/c-docs/c-headers.tsv
  ```

  Expected: 594 lines (header line plus 593 rows): `tail -n +2 docs/audits/c-docs/c-headers.tsv | cut -f1 | sort | uniq -c` shows 68 class, 298 function (283 + 15), 227 attribute.

- [ ] **Step 7: Write the freshness script** `tools/oracle_fixtures/check_c_headers_fresh.sh`:

  ```bash
  #!/bin/bash
  # Fail if docs/audits/c-docs/c-headers.tsv differs from the headers in the
  # pinned reference source. Run after every move of reference/starlink-ast.
  set -euo pipefail
  cd "$(dirname "$0")/../.."
  fresh=$(mktemp)
  trap 'rm -f "$fresh"' EXIT
  cargo run -q -p audit-tools --bin c-docs -- headers reference/starlink-ast/src > "$fresh"
  if ! diff -u docs/audits/c-docs/c-headers.tsv "$fresh"; then
      echo "c-headers.tsv is stale: regenerate it and add ledger rows for new headers" >&2
      exit 1
  fi
  ```

  Run it; expected exit 0.

- [ ] **Step 8: Commit.**

  ```sh
  git add crates/audit-tools docs/audits/c-docs/c-headers.tsv tools/oracle_fixtures/check_c_headers_fresh.sh
  git commit -m "Extract the public C documentation headers"
  ```

---

### Task 2: The ledger, and the coverage, form and ratchet gates

**Files:**
- Create: `crates/audit-tools/src/c_docs_ledger.rs`
- Modify: `crates/audit-tools/src/lib.rs`, `crates/audit-tools/src/bin/c-docs.rs` (add `init-ledger`)
- Create: `docs/audits/c-docs/ledger.tsv`
- Create: `docs/audits/c-docs/README.md`
- Create: `crates/starlink-ast/tests/test_c_docs_audit.rs`
- Modify: `crates/starlink-ast/tests/meta/main.rs`

**Interfaces:**
- Consumes: `parse_headers_tsv` (Task 1).
- Produces:
  ```rust
  pub struct LedgerRow {
      pub kind: String, pub name: String, pub file: String,
      pub rust_item: Vec<String>,   // "Owner::item" or "Owner"; empty when none
      pub reason: Option<String>,   // "c-api-only" | "not-ported" | "merged"
      pub reviewed: bool,
      pub note: String,
  }
  pub fn parse_ledger(text: &str) -> Result<Vec<LedgerRow>, String>; // Err names the line
  ```
  Ledger columns, tab-separated, with a header line: `kind name file rust_item reason reviewed note`; `rust_item` items separated by `;`.

- [ ] **Step 1: Generate the initial ledger** with a new `c-docs init-ledger` subcommand that writes one row per header with `rust_item` and `reason` empty and `reviewed` `no`, except rows from these files, which it fills as `not-ported`, `reviewed = yes`: `plot.c`, `plot3d.c`, `xmlchan.c`, `xml.c`, `yamlchan.c`, `stcschan.c`, `grf3d.c`, `grf_2.0.c`, `grf_5.6.c`, `grf_pgplot.c` (whichever exist).

  ```sh
  cargo run -q -p audit-tools --bin c-docs -- init-ledger docs/audits/c-docs/c-headers.tsv > docs/audits/c-docs/ledger.tsv
  ```

  Write `docs/audits/c-docs/README.md` stating what the two files are, the column meanings, the three reasons, and that the procedure is in `docs/superpowers/plans/2026-10-19-c-docs-audit.md`.

- [ ] **Step 2: Write the failing gates** in `crates/starlink-ast/tests/test_c_docs_audit.rs` and register it in `tests/meta/main.rs` (`#[path = "../test_c_docs_audit.rs"] mod test_c_docs_audit;`):

  ```rust
  //! Gates for the C documentation audit
  //! (`docs/superpowers/specs/2026-10-19-c-docs-audit-design.md`).

  use std::collections::{BTreeMap, BTreeSet};
  use std::path::PathBuf;

  use audit_tools::{LedgerRow, parse_headers_tsv, parse_ledger};

  /// Rows not yet reviewed. It may only go down; it reaches zero when the
  /// audit is complete.
  const UNREVIEWED_ROWS: usize = 0; // set to the generated count in Step 4

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
          *got.entry((r.kind.clone(), r.name.clone(), r.file.clone())).or_insert(0) += 1;
      }
      let missing: Vec<_> = want.iter().filter(|k| !got.contains_key(*k)).collect();
      let extra: Vec<_> = got.keys().filter(|k| !want.contains(*k)).collect();
      let dup: Vec<_> = got.iter().filter(|(_, n)| **n > 1).map(|(k, _)| k).collect();
      assert!(missing.is_empty() && extra.is_empty() && dup.is_empty(),
          "missing {missing:?}\nextra {extra:?}\nduplicated {dup:?}");
  }

  #[test]
  fn reviewed_rows_are_well_formed() {
      let rows = ledger();
      let mapped: BTreeSet<_> = rows.iter().filter(|r| !r.rust_item.is_empty()).map(|r| r.name.clone()).collect();
      for r in rows.iter().filter(|r| r.reviewed) {
          let what = format!("{} {} ({})", r.kind, r.name, r.file);
          assert!(r.rust_item.is_empty() != r.reason.is_none(), "{what}: needs exactly one of rust_item and reason");
          if let Some(reason) = &r.reason {
              assert!(["c-api-only", "not-ported", "merged"].contains(&reason.as_str()), "{what}: reason {reason}");
              if reason == "merged" {
                  assert!(mapped.contains(r.note.trim()), "{what}: merged into {:?}, which has no Rust item", r.note);
              }
          }
      }
  }

  #[test]
  fn the_unreviewed_count_only_goes_down() {
      let n = ledger().iter().filter(|r| !r.reviewed).count();
      assert_eq!(n, UNREVIEWED_ROWS, "lower UNREVIEWED_ROWS to {n} when rows are reviewed; it may never rise");
  }
  ```

- [ ] **Step 3: Run them to verify they fail.**
  Run: `cargo test -p starlink-ast --test meta test_c_docs_audit::`
  Expected: compile error, `parse_ledger` not found.

- [ ] **Step 4: Implement `parse_ledger`** (reject a line with the wrong number of columns, an unknown `reviewed` value, or a non-empty `reason` outside the three, naming the line number), then set `UNREVIEWED_ROWS` to the count the ratchet test reports.

- [ ] **Step 5: Run.**
  Run: `cargo test -p starlink-ast --test meta test_c_docs_audit:: && cargo test -p audit-tools`
  Expected: pass.

- [ ] **Step 6: Commit.**

  ```sh
  git add crates/audit-tools docs/audits/c-docs crates/starlink-ast/tests/test_c_docs_audit.rs crates/starlink-ast/tests/meta/main.rs
  git commit -m "Add the C documentation ledger and its coverage gates"
  ```

---

### Task 3: The alias gate

**Files:**
- Create: `crates/audit-tools/src/doc_aliases.rs`
- Modify: `crates/audit-tools/src/lib.rs`
- Modify: `crates/starlink-ast/tests/test_c_docs_audit.rs`

**Interfaces:**
- Consumes: `enclosing_rust_block` (existing, `audit-tools/src/lib.rs`), `LedgerRow`.
- Produces:
  ```rust
  /// Every `#[doc(alias = "…")]` in `src`: (alias, item name, owner).
  /// The item is the next `fn`/`struct`/`enum`/`trait`/`type`/`const` declared
  /// after the attribute; the owner is the subject of the enclosing `impl` or
  /// `trait` block (`"PermMap"` for `impl PermMap` and `impl Mapping for
  /// PermMap`, `"Mapping"` for `trait Mapping`), or the item's own name for a
  /// top-level struct, enum, trait or type.
  pub fn doc_aliases(src: &str) -> Vec<(String, String, String)>;
  ```

- [ ] **Step 1: Write failing unit tests** in `doc_aliases.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::doc_aliases;

      #[test]
      fn finds_aliases_on_methods_types_and_trait_items() {
          let src = r#"
  /// A map.
  #[doc(alias = "PermMap")]
  pub struct PermMap;
  impl PermMap {
      /// New.
      #[doc(alias = "astPermMap")]
      pub fn new() -> Self { PermMap }
  }
  pub trait Mapping {
      #[doc(alias = "Invert")]
      fn is_inverted(&self) -> bool;
  }
  impl Mapping for PermMap {
      fn is_inverted(&self) -> bool { false }
  }
  "#;
          let a = doc_aliases(src);
          assert!(a.contains(&("PermMap".into(), "PermMap".into(), "PermMap".into())));
          assert!(a.contains(&("astPermMap".into(), "new".into(), "PermMap".into())));
          assert!(a.contains(&("Invert".into(), "is_inverted".into(), "Mapping".into())));
      }

      #[test]
      fn an_alias_on_a_type_alias_passes() {
          let src = "#[doc(alias = \"StcResourceProfile\")]\npub type StcResourceProfile = Stc<ResourceProfile>;\n";
          assert_eq!(doc_aliases(src), [("StcResourceProfile".into(), "StcResourceProfile".into(), "StcResourceProfile".into())]);
      }

      #[test]
      fn other_attributes_between_alias_and_item_are_skipped() {
          let src = "impl K {\n    #[doc(alias = \"astMapGet0\")]\n    #[must_use]\n    pub fn map_get0_d(&self) {}\n}\n";
          assert_eq!(doc_aliases(src), [("astMapGet0".into(), "map_get0_d".into(), "K".into())]);
      }
  }
  ```

- [ ] **Step 2: Run them to verify they fail.**
  Run: `cargo test -p audit-tools doc_aliases`
  Expected: compile error.

- [ ] **Step 3: Implement `doc_aliases`.**
  For each line matching `#[doc(alias = "X")]` (whitespace-tolerant), scan forward past lines starting `#[`, `///` or `//`, and blank lines, to the declaration line; take the identifier after `fn `, `struct `, `enum `, `trait ` or `type ` (ignoring `pub`, `pub(crate)`, `const`, `unsafe`, `async`).
  Take the owner from `enclosing_rust_block` on that line: `"Trait for Type"` → `Type`, `"Type"` → `Type`, `"trait Name"` → `Name`; with no enclosing block the owner is the item's own name.

- [ ] **Step 4: Run.** `cargo test -p audit-tools doc_aliases` → 3 passed.

- [ ] **Step 5: Add the gate** to `test_c_docs_audit.rs`:

  ```rust
  /// Every alias in starlink-ast's source, as (alias, item, owner).
  fn source_aliases() -> Vec<(String, String, String)> {
      let mut out = Vec::new();
      let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
      let mut stack = vec![root];
      while let Some(dir) = stack.pop() {
          for e in std::fs::read_dir(dir).unwrap() {
              let p = e.unwrap().path();
              if p.is_dir() {
                  stack.push(p);
              } else if p.extension().is_some_and(|x| x == "rs") {
                  out.extend(audit_tools::doc_aliases(&std::fs::read_to_string(&p).unwrap()));
              }
          }
      }
      out
  }

  #[test]
  fn every_mapped_row_has_its_alias_on_its_item() {
      let aliases_by_name: BTreeMap<String, String> = parse_headers_tsv(
          &std::fs::read_to_string(audit_dir().join("c-headers.tsv")).unwrap(),
      )
      .into_iter()
      .map(|(_, name, alias, _)| (name, alias))
      .collect();
      let found: BTreeSet<_> = source_aliases().into_iter().collect();
      let mut wanted = BTreeSet::new();
      let mut missing = Vec::new();
      for r in ledger() {
          let alias = &aliases_by_name[&r.name];
          for item in &r.rust_item {
              let (owner, name) = item.split_once("::").unwrap_or((item, item));
              let key = (alias.clone(), name.to_string(), owner.to_string());
              if !found.contains(&key) {
                  missing.push(format!("{} → {item}: no #[doc(alias = \"{alias}\")] on it", r.name));
              }
              wanted.insert(key);
          }
      }
      let unlisted: Vec<_> = found.iter().filter(|k| !wanted.contains(*k)).collect();
      assert!(missing.is_empty() && unlisted.is_empty(),
          "missing:\n{}\naliases with no ledger row: {unlisted:?}", missing.join("\n"));
  }
  ```

  Add `a_family_alias_on_each_member_passes` to `doc_aliases.rs`'s tests: two methods `map_get0_d` and `map_get0_i` under `impl KeyMap`, both with alias `astMapGet0`, both returned.

- [ ] **Step 6: Run.** `cargo test -p starlink-ast --test meta test_c_docs_audit:: && cargo test -p audit-tools` → pass (no row has a Rust item yet, and the source has no aliases).

- [ ] **Step 7: Commit.**

  ```sh
  git add crates/audit-tools crates/starlink-ast/tests/test_c_docs_audit.rs
  git commit -m "Gate C-name aliases against the documentation ledger"
  ```

---

### Task 4: The rendered-docs gate

**Files:**
- Create: `crates/audit-tools/src/rendered_docs.rs`
- Modify: `crates/audit-tools/src/lib.rs`
- Modify: `crates/starlink-ast/tests/test_c_docs_audit.rs`
- Modify: `crates/starlink-ast/tests/test_verification_ledger.rs` (one `LEDGER` row)
- Modify: `PLAN.md` §6 (two rows)

**Interfaces:**
- Produces:
  ```rust
  /// The C references in one rendered page's visible text: C source file
  /// names (`permmap.c`, with or without `:line`) and `ast<Name>` words not
  /// in `public`.
  pub fn c_references(html: &str, public: &BTreeSet<String>) -> Vec<String>;
  ```

- [ ] **Step 1: Write failing unit tests** in `rendered_docs.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::c_references;
      use std::collections::BTreeSet;

      fn public() -> BTreeSet<String> { ["astPermMap".to_string()].into() }

      #[test]
      fn a_c_file_name_anywhere_in_text_is_flagged() {
          let html = "<p>As in permmap.c:2849 and <a href=\"x\">frame.c</a>.</p>";
          assert_eq!(c_references(html, &public()), ["permmap.c:2849", "frame.c"]);
      }

      #[test]
      fn internal_c_names_are_flagged_and_public_ones_are_not() {
          let html = "<p>Like astPermMap; built by astInitPermMap.</p>";
          assert_eq!(c_references(html, &public()), ["astInitPermMap"]);
      }

      #[test]
      fn markup_and_rust_names_are_not_text() {
          let html = "<a href=\"permmap.c.html\" class=\"astx\">starlink_ast</a><p>AstError</p>";
          assert!(c_references(html, &public()).is_empty());
      }
  }
  ```

- [ ] **Step 2: Run them to verify they fail.** `cargo test -p audit-tools rendered_docs` → compile error.

- [ ] **Step 3: Implement.** Remove tags (`<…>`) and decode `&lt;`, `&gt;`, `&amp;`, `&quot;`; in the remaining text find `\b[a-z][a-z0-9_]*\.c(:\d+(-\d+)?)?\b` and words matching `\bast[A-Z][A-Za-z0-9_]*`, the latter kept only when not in `public`. Return them in order of appearance.

- [ ] **Step 4: Run.** `cargo test -p audit-tools rendered_docs` → 3 passed.

- [ ] **Step 5: Add the gate** to `test_c_docs_audit.rs`. It builds the docs itself into its own target directory, so it never reads docs built by someone else and does not wait on the outer `cargo test`'s lock:

  ```rust
  /// Top-level modules whose rendered docs still carry C references, each
  /// with why. The list only shrinks; `a_clean_module_cannot_stay_listed`
  /// fails when a listed module is clean.
  const C_CITATIONS_UNCLEANED: &[(&str, &str)] = &[
      // filled in Step 6 from the first run
  ];

  /// The C references in each top-level module's rendered pages.
  fn rendered_c_references() -> BTreeMap<String, Vec<String>> {
      let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
      let target = manifest.join("../../target/c-docs-gate");
      let status = std::process::Command::new(env!("CARGO"))
          .args(["doc", "-q", "-p", "starlink-ast", "--no-deps"])
          .env("CARGO_TARGET_DIR", &target)
          .current_dir(&manifest)
          .status()
          .unwrap();
      assert!(status.success(), "cargo doc failed");
      let public: BTreeSet<String> = parse_headers_tsv(
          &std::fs::read_to_string(audit_dir().join("c-headers.tsv")).unwrap(),
      )
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
          .map(|d| d.join("starlink_ast"))
          .filter(|d| d.is_dir())
          .max_by_key(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok())
          .expect("rendered starlink_ast docs");
      let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
                      _ => rel.components().next().unwrap().as_os_str().to_string_lossy().into_owned(),
                  };
                  let refs = audit_tools::c_references(&std::fs::read_to_string(&p).unwrap(), &public);
                  out.entry(module).or_default().extend(refs.into_iter().map(|r| format!("{}: {r}", rel.display())));
              }
          }
      }
      out.retain(|_, v| !v.is_empty());
      out
  }

  fn listed_uncleaned() -> BTreeSet<&'static str> {
      C_CITATIONS_UNCLEANED.iter().map(|(m, _)| *m).collect()
  }

  #[test]
  fn rendered_docs_carry_no_c_references() {
      let refs = rendered_c_references();
      let listed = listed_uncleaned();
      let unlisted: Vec<_> = refs.iter().filter(|(m, _)| !listed.contains(m.as_str())).collect();
      assert!(unlisted.is_empty(), "C references in rendered docs: {unlisted:#?}");
  }

  #[test]
  fn a_clean_module_cannot_stay_listed() {
      let refs = rendered_c_references();
      let clean: Vec<_> = listed_uncleaned().into_iter().filter(|m| !refs.contains_key(*m)).collect();
      assert!(clean.is_empty(), "now clean; remove from C_CITATIONS_UNCLEANED: {clean:?}");
  }
  ```

  Each test runs `cargo doc`; the second run is incremental and quick.
  `rendered_c_references` scans only `starlink_ast/`, so the source-view pages under `src/`, which show `//` comments, are never read.

- [ ] **Step 6: Run it, then fill the list.**
  Run: `cargo test -p starlink-ast --test meta test_c_docs_audit::rendered`
  Expected: FAIL listing every module with C references.
  Set `C_CITATIONS_UNCLEANED` to one row per reported module, reason `"C source citations in public docs; cleaned in its subsystem's review task"`, and run again → pass.

- [ ] **Step 7: Register the list in the verification ledger.**
  Add to `LEDGER` in `test_verification_ledger.rs`:

  ```rust
  (
      "test_c_docs_audit.rs",
      "C_CITATIONS_UNCLEANED",
      <n>,
      "modules whose rendered docs still cite C source; each subsystem's review \
       task in the C documentation audit removes its modules, and a companion test \
       fails when a listed module is clean",
  ),
  ```

  with `<n>` the count from Step 6, and add the matching `PLAN.md` §6 row.
  Add a second §6 row for `UNREVIEWED_ROWS` (the ratchet), stating its current value and that it reaches zero at the end.
  (`count_entries` counts list consts; `UNREVIEWED_ROWS` is a number, so it is recorded in PLAN §6 and pinned by its own test rather than counted by the ledger.)

- [ ] **Step 8: Run the meta suite.** `cargo test -p starlink-ast --test meta` → pass.

- [ ] **Step 9: Commit.**

  ```sh
  git add crates/audit-tools crates/starlink-ast/tests/test_c_docs_audit.rs crates/starlink-ast/tests/test_verification_ledger.rs PLAN.md
  git commit -m "Gate C references in the rendered documentation"
  ```

---

### Task 5: Review the Mapping classes

**Files:** `crates/starlink-ast/src/mappings/*.rs`, `docs/audits/c-docs/ledger.tsv`, `crates/starlink-ast/tests/test_c_docs_audit.rs`, `PLAN.md` (gap rows only).

**C files:** `mapping.c`, `cmpmap.c`, `unitmap.c`, `zoommap.c`, `shiftmap.c`, `matrixmap.c`, `permmap.c`, `winmap.c`, `lutmap.c`, `polymap.c`, `chebymap.c`, `wcsmap.c`, `slamap.c`, `specmap.c`, `timemap.c`, `pcdmap.c`, `grismmap.c`, `sphmap.c`, `unitnormmap.c`, `xphmap.c`, `intramap.c`, `tranmap.c`, `ratemap.c`, `mathmap.c`, `splinemap.c`, `dssmap.c`, `normmap.c`, `selectormap.c`, `switchmap.c`.

- [ ] **Step 1:** Apply the row review procedure to each file, in the order listed, one commit per three to five files (`Document the <classes> from their C headers`).
- [ ] **Step 2:** After the last file: every row of these files is `reviewed = yes`; `mappings` is out of `C_CITATIONS_UNCLEANED`; the ledger's count and PLAN §6 are updated.
- [ ] **Step 3:** Run `cargo test -p starlink-ast && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps`; all pass.

---

### Task 6: Review the Frame classes

**C files:** `frame.c`, `axis.c`, `skyaxis.c`, `specframe.c`, `skyframe.c`, `timeframe.c`, `dsbspecframe.c`, `fluxframe.c`, `specfluxframe.c`, `cmpframe.c`, `frameset.c`.

- [ ] **Step 1:** Apply the row review procedure to each file, in the order listed, one commit per two or three files.
- [ ] **Step 2:** Every row of these files reviewed; `frames` out of `C_CITATIONS_UNCLEANED`; counts updated.
- [ ] **Step 3:** Run the same checks as Task 5 Step 3.

---

### Task 7: Review the Region classes

**C files:** `region.c`, `box.c`, `circle.c`, `ellipse.c`, `polygon.c`, `interval.c`, `nullregion.c`, `cmpregion.c`, `pointlist.c`, `prism.c`, `moc.c`, `stc.c`, `stcresourceprofile.c`, `stcsearchlocation.c`, `stccatalogentrylocation.c`, `stcobsdatalocation.c`.

- [ ] **Step 1:** Apply the row review procedure to each file, in the order listed, one commit per three to five files.
- [ ] **Step 2:** Every row of these files reviewed; `regions` out of `C_CITATIONS_UNCLEANED`; counts updated.
- [ ] **Step 3:** Run the same checks as Task 5 Step 3.

---

### Task 8: Review FitsChan and the Channels

**C files:** `fitschan.c`, `channel.c`, `mocchan.c`.

- [ ] **Step 1:** Apply the row review procedure to each file. `fitschan.c` alone carries the most attribute headers; commit it in two or three parts by attribute and function group.
- [ ] **Step 2:** Every row of these files reviewed; `fitschan` and `channel` out of `C_CITATIONS_UNCLEANED`; counts updated.
- [ ] **Step 3:** Run the same checks as Task 5 Step 3.

---

### Task 9: Review the rest

**C files:** every file with a row still `reviewed = no` — at least `object.c`, `keymap.c`, `table.c`, `fitstable.c`, `pointset.c`, `unit.c`, `memory.c`, `error.c`, `loader.c`, `globals.c` — listed by:

```sh
awk -F'\t' 'NR>1 && $6=="no" {print $3}' docs/audits/c-docs/ledger.tsv | sort -u
```

- [ ] **Step 1:** Apply the row review procedure to each listed file. Pointer, context, string-attribute, status and memory functions are `c-api-only`.
- [ ] **Step 2:** `UNREVIEWED_ROWS` is 0 and `C_CITATIONS_UNCLEANED` is empty; the ledger row for `C_CITATIONS_UNCLEANED` becomes 0 and PLAN §6's two rows say the audit is complete.
- [ ] **Step 3:** Run the same checks as Task 5 Step 3.

---

### Task 10: Record the process

**Files:** `CLAUDE.md`, `docs/superpowers/specs/2026-04-20-rust-port-design.md`.

- [ ] **Step 1:** In `CLAUDE.md`'s upstream sync protocol, after `check_corpus_fresh.sh`, add: "Also run `tools/oracle_fixtures/check_c_headers_fresh.sh`; new or changed C documentation headers need ledger rows in `docs/audits/c-docs/ledger.tsv`."
- [ ] **Step 2:** In `CLAUDE.md`'s "Adding a new Mapping / Frame / Region" list, add a step: "Give each public item mapped from a C header its `#[doc(alias)]` and ledger row; the C documentation gates in `test_c_docs_audit.rs` fail otherwise."
- [ ] **Step 3:** Add a design-spec revision entry: every public item mapped from a C header carries a `#[doc(alias)]` with its C name, rendered docs carry no C source citations, and `docs/audits/c-docs/ledger.tsv` maps every C header; see `2026-10-19-c-docs-audit-design.md`.
- [ ] **Step 4:** `cargo test -p starlink-ast --test meta` → pass; commit (`Record the C documentation audit in the working rules`).

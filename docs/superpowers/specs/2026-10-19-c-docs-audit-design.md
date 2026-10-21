# C documentation audit: design

**Status:** approved in conversation 2026-10-19; this document records it.
**Scope:** every public documentation header in the C source, mapped to the Rust item that documents it or to a reason it has none, with the content that matters to a Rust user moved into `ast-core`'s rustdoc; and C source citations moved out of the user-facing docs.

## Context

This is the second of three pieces of documentation work.
The first, `show` and `to_native_string` (`2026-10-19-show-and-native-string-design.md`), is done.
The third, a user guide adapted from SUN/211 as `ast_core::guide`, comes after this one and links to the item docs this work fills in.

The C library documents its public API in structured comment headers that `builddocs` turns into the reference sections of SUN/211.
Today the Rust docs have no cross-reference to C names (`ast-core` has no `#[doc(alias)]`), and many of them cite C source for porters: 1,982 doc-comment lines in 165 files name a C file or line, some on public items where users read them.

## Goals

1. **The Rust docs stand on their own.**
   A reader who has never seen the C library can use the API from rustdoc alone.
2. **A C user can find the Rust item.**
   Every mapped item carries `#[doc(alias = "<C name>")]`, so typing `astPermMap` or `PermSplit` into the rustdoc search box finds it.
3. **Nothing in the C headers is lost silently.**
   Every header has a ledger row, and the content a Rust user needs has been reviewed and moved.
4. **No C internals in user-facing text.**
   No C source or header file names, no line references, and no internal C function names appear in the rendered docs.

## The C headers

`reference/starlink-ast/src/*.c` holds four kinds of public header, delimited by marker lines:

| Kind | Start / end markers | Count |
|---|---|---:|
| Class | `*class++` / `*class--` | 68 |
| Function | `*++` / `*--` | 283 |
| Attribute | `*att++` / `*att--` | 227 |
| C-only function | `c++` / `c--` | 15 |

`f++` / `f--` headers (2) are Fortran-only and out of scope.
Inside a header, lines starting `c ` are the C edition's and lines starting `f ` the Fortran edition's; only `*` and `c ` lines are read, which is what `builddocs`'s `selectfc` does for SUN/211.
That gives 593 headers.

## Design

### The header list

A tool in `crates/audit-tools` reads `reference/starlink-ast/src` and writes `docs/audits/c-docs/c-headers.tsv`, one row per header: kind, C name, C file.
The C name is the header's `Name:` entry for its C edition (`astPermMap`, `PermSplit`, `PermMap`).
The file is committed so that the gates run without the submodule, as fixtures do.
`tools/oracle_fixtures/check_c_headers_fresh.sh` regenerates it from the pinned submodule and fails on any difference; it is run on every pin move, alongside `check_corpus_fresh.sh`, and the upstream sync protocol in `CLAUDE.md` names it.

### The ledger

`docs/audits/c-docs/ledger.tsv` has one row per header, with columns:

- **kind, C name** — matching `c-headers.tsv`.
- **Rust item** — the path of the item that documents it (`mappings::PermMap::new`, `mappings::PermMap::perm_split`, `frames::Frame::convert_to`), or empty.
- **reason** — empty when there is a Rust item; otherwise one of:
  - `c-api-only`: belongs to the C shim, not the Rust API — pointer and context management (`astAnnul`, `astBegin`, `astExempt`), string attribute access (`astGet<X>`, `astSet`, `astTest`, `astClear` by name), memory utilities, status handling.
  - `not-ported`: the class or function has no Rust port yet (Plot, Plot3D, XmlChan, YamlChan, StcsChan and their attributes and functions).
  - `merged`: documented by another row's item; the row names that C name in its note.
- **reviewed** — `yes` once the content has been moved as described below; `no` before.
- **note** — free text: the target of a `merged` row, a documented C behaviour the port lacks (with its `PLAN.md` row), or why a See also was or was not added.

### Content rules

These are what reviewing a row means.

**Class headers** become the struct's doc.
Purpose becomes the summary line and Description the body.
The attribute list becomes intra-doc links to the getters.
Inheritance, "no new functions" lists, copyright, licence and history are dropped; rustdoc shows trait impls itself.

**Function headers** become the method's or function's doc.
Purpose becomes the summary; Description and Notes are rewritten for Rust.
Parameters become a `# Arguments` section only where a name and type do not explain them.
The returned value goes into the prose.
Conditions that return `Err` become an `# Errors` section naming the `ErrorCode`, filled in from the code where the C header is silent.
Synopsis, Type and Class Membership are dropped, as is anything about `status`, pointers or Fortran.

**Attribute headers** go on the getter.
Description, the default (stated as "Default: …") and the allowed values are kept.
Where the attribute behaves differently per class (the header's Applicability section), each class's override says how.
The setter, `test_` and `clear_` methods link to the getter instead of repeating it.

**Everywhere:**

- C mechanics become Rust: "pointer to", `astSet("Attr=…")` strings, the global status and `AST__BAD` become owned values, typed setters, `AstResult` and `BAD_VALUE`.
- Axis indices stay 1-based, as the API is.
- C examples are not copied.
  Where an example earns its place it is written as a Rust doctest, which then runs in CI; encouraged on constructors and principal methods, not required.
- Every mapped item gets `#[doc(alias = "<C name>")]`, except a public top-level type or trait named exactly as the C name, which rustdoc does not allow to carry an alias of its own name.
- A `# See also` section naming the public C function is added only where the correspondence is not obvious from the Rust name: `PermMap::new` needs none, `Frame::convert_to` names `astConvert`.
- Documentation describes the Rust code as it is.
  When a C header documents a default or behaviour the port lacks or does differently, the docs describe the port, the ledger note records the difference, and a `PLAN.md` row records the gap.

### C citations leave the user-facing docs

On public items and public modules, doc comments that cite a C file or line, or name an internal C function (one not in `c-headers.tsv`, such as `astInitPermMap` or `astLoadPermMap`), have that text moved into ordinary `//` comments beside the code it explains, so maintainers keep the provenance and rustdoc never renders it.
Doc comments on private items are unchanged, since rustdoc does not render them.

### Gates

All in `crates/audit-tools/tests/` or `crates/ast-core/tests/meta/`, failing hard:

1. **Coverage.** Every row of `c-headers.tsv` has exactly one ledger row, and every ledger row has a header.
2. **Rows are well formed.** A row has a Rust item or a reason, not both; a reason is one of the three; a `merged` row names a C name that has a row with a Rust item.
3. **Aliases land on their items.** For every row with a Rust item, the source has `#[doc(alias = "<C name>")]` attached to an item of that name, inside that type's `impl`, `trait` or definition. The test parses the source textually: it finds each alias attribute, the item it is attached to (the next `fn`, `struct`, `enum`, `trait` or `type` line), and the enclosing `impl` or `trait` block.
   An alias with no ledger row also fails, so the two cannot drift apart.
   A row whose Rust item is a public top-level type or trait named exactly as the C name (`Mapping`, `PermMap`) is checked against that declaration instead, because rustdoc rejects an alias equal to the item's own name; a type named differently from the C name (`Box` → `BoxRegion`) still carries the alias.
4. **Rendered docs carry no C internals.** After `cargo doc -p ast-core --no-deps`, no HTML page under `ast_core/` contains a C source or header reference (a `<name>.c` or `<name>.h` file name, with or without a line number) or a name of the form `ast<Name>` that is not in `c-headers.tsv`.
   Modules not yet cleaned are listed in a named exclusion, `C_CITATIONS_UNCLEANED`, with a reason, and a companion test fails when a listed module becomes clean, so the list can only shrink.
5. **Review progress.** The number of rows with `reviewed = no` is pinned in the test and may only go down; it reaches zero at the end of the work.
   The pinned count and `C_CITATIONS_UNCLEANED` are added to the verification ledger (`test_verification_ledger.rs`, PLAN §6).

The rendered-docs gate builds the docs itself: its test runs `cargo doc -p ast-core --no-deps` into the workspace target directory (incremental, so a second run is quick) and then scans the HTML, so it never depends on docs someone else built and has no path that passes without checking.

## Phases

Each phase lands with every gate green.

1. **Tooling, gates and the Mapping classes.**
   The header tool, `c-headers.tsv`, the full ledger with every row present (unreviewed rows marked `no`), the five gates, and the review, aliases and citation clean-up for `mapping.c` and every Mapping subclass.
2. **Frames:** `frame.c`, `axis.c` and the axis and Frame subclasses, `frameset.c`, `cmpframe.c`.
3. **Regions:** `region.c` and every Region subclass, the Stc classes and `moc.c`.
4. **FitsChan and Channel:** `fitschan.c`, `channel.c`, `mocchan.c`.
5. **The rest:** `keymap.c`, `table.c`, `fitstable.c`, `object.c`, `unit.c`, `pointset.c`, and anything else with a row still `no`.

## Out of scope

- Writing docs for Rust items that have no C counterpart, beyond what the citation clean-up requires.
- The `ast-capi` shim's docs: `c-api-only` rows are recorded, not documented.
- The user guide (the third piece of work).

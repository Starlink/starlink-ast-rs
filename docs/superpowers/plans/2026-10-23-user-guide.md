# SUN/211 User Guide Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the narrative chapters of SUN/211 into `ast_core::guide` as rustdoc Markdown chapters with Rust doctests and intra-doc links.

**Architecture:** One item-less submodule per chapter under `crates/ast-core/src/guide/`, each pulling its text from a sibling `.md` file with `#![doc = include_str!(...)]`.
Examples are doctests run by `cargo test -p ast-core --doc`; links are checked by the denied rustdoc lint group; C internals are kept out by the existing rendered-docs gate in `test_c_docs_audit.rs`.
A small `audit-tools` binary computes each chapter's authorship from `git blame` so commit credit is mechanical.

**Tech Stack:** Rust 2024, rustdoc (Markdown, intra-doc links, doctests), `audit-tools` crate, `git blame`, Ghostscript / `rsvg-convert` for figures.

**Spec:** `docs/superpowers/specs/2026-10-23-user-guide-design.md`

## Global Constraints

- Source text: `reference/starlink-ast/sun_master.tex` at submodule commit `c6c60753`, C edition only: drop every `f+` … `f-` block and keep `c+` … `c-` blocks (what `reference/starlink-ast/selectfc` does without `-f`).
- Words stay the authors'; change only what makes no sense in Rust (spec "Rewriting rules").
- C is named in the Introduction; elsewhere only where the Rust library behaves differently from C. The one known difference: a Frame is not a Mapping.
- Handle-management passages (`astAnnul`, `astClone`, `astBegin`/`astEnd`, `astExempt`, `astExport`/`astImport`, pointer types) are removed, not described as differences. `astCopy` becomes `clone()`. Shared-object passages say the Rust holder keeps its own copy.
- No C source file names, line numbers, or internal `ast<Name>` names in rendered pages (`test_c_docs_audit::rendered_docs_carry_no_c_references`).
- Every named class, method, trait and accessor is an intra-doc link.
- Every example is a doctest that runs; `no_run`/`ignore` only with a one-line reason comment in the fence.
- Axis indices 1-based.
- One sentence per line; American English spelling.
- Dropped: XmlChan, StcsChan, YamlChan, Plot chapters and every mention of them; the class hierarchy; function/routine/attribute/class descriptions; UNIX commands; memory-management utilities; release notes; Fortran.
- Copyright line on the Introduction: `Copyright (C) 2026 East Asian Observatory and AURA`.
- Chapter commit authorship: share = fraction of the chapter's non-blank, non-comment C-edition lines `git blame -w -M -C` attributes to the person; credited if ≥ 10%; larger share is `--author`, the other a `Co-authored-by:` trailer. Identities: `David Berry <davidstuartberry@gmail.com>`, `Rodney Warren-Smith <rfws@star.rl.ac.uk>`. Trailers precede the AI attribution lines. Non-prose commits use the normal author.
- A real port bug found by an example: fix in the port, separate commit, normal author. A missing public API: **stop and ask the user** before adding anything.
- Every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo doc --workspace --no-deps`, `cargo test -p ast-core` green.

## Review Focus

1. **Subclass membership tests.** SUN/211's `astIsASpecFrame` is true for a DSBSpecFrame; `as_any().is::<SpecFrame>()` is not. Expected: the Rust example in "How To… Test if a WCS is a Spectral Coordinate System" accepts both — pinned by a doctest that builds a DSBSpecFrame and asserts the test reports spectral (Task 3).
2. **Shared-object passages.** A reader following "Adding New Frames to a FrameSet" expects to know whether changing the original Frame changes the FrameSet. Expected: the guide says it does not — pinned by a doctest that mutates the original after `add_frame` and asserts the FrameSet's Frame is unchanged (Task 6).
3. **Frame passed where a Mapping is expected.** Expected: the Frames chapter's "Using a Frame as a Mapping" section states the difference once and shows a `UnitMap` doctest; no other chapter repeats it (Task 5, check in its review step).
4. **Example output drift.** Printed output reproduced from SUN/211 (formatted coordinates, `show` dumps) that the Rust prints differently. Expected: the guide shows the Rust output, and the doctest asserts it with `assert_eq!` rather than printing it (every chapter task's doctests use asserts, not `println!`).
5. **Mentions of dropped classes leaking through.** Expected: no rendered guide page contains `XmlChan`, `StcsChan`, `YamlChan` or `Plot` as a word — pinned by `guide_mentions_no_unported_class` (Task 1).

---

## File Structure

```
crates/ast-core/src/guide/
├── mod.rs              # module doc = table of contents; declares chapter mods
├── README.md           # NOT rendered: source pin, section→chapter table, authorship table
├── intro.md            # Introduction + copyright
├── cargo.md            # Using AST from Cargo (replaces Compiling and Linking)
├── concepts.md         # Overview of AST Concepts
├── howto.md            # How To…
├── objects.md          # An AST Object Primer
├── mappings.md         # Inter-Relating Coordinate Systems (Mappings)
├── cmpmaps.md          # Compound Mappings (CmpMaps)
├── frames.md           # Representing Coordinate Systems (Frames)
├── skyframes.md        # SkyFrames
├── specframes.md       # SpecFrames
├── timeframes.md       # TimeFrames
├── cmpframes.md        # CmpFrames
├── conversions.md      # An Introduction to Coordinate System Conversions
├── framesets.md        # Coordinate System Networks (FrameSets)
├── frameset_ops.md     # Higher Level Operations on FrameSets
├── channels.md         # Saving and Restoring Objects (Channels)
├── fitschans.md        # Storing AST Objects in FITS Headers (FitsChans)
├── foreign_fits.md     # Using Foreign FITS Encodings
├── intramaps.md        # Creating Your Own Private Mappings (IntraMaps)
├── fits_wcs.md         # FITS-WCS Coverage
└── figures/*.svg       # converted SUN/211 figures
crates/audit-tools/src/guide_authors.rs       # blame-share computation (lib)
crates/audit-tools/src/bin/guide-authors.rs   # CLI over it
crates/ast-core/tests/test_guide.rs           # guide gates (in meta program)
```

`mod.rs` declares, in this order: `intro, cargo, concepts, howto, objects, mappings, cmpmaps, frames, skyframes, specframes, timeframes, cmpframes, conversions, framesets, frameset_ops, channels, fitschans, foreign_fits, intramaps, fits_wcs`.
Each is `pub mod <name> { #![doc = include_str!("<name>.md")] }` inline in `mod.rs` (no per-chapter `.rs`).
A chapter not yet ported is not declared (no placeholder pages).

## Per-chapter procedure (used by Tasks 2–9)

Every chapter task follows these steps; the task lists only what differs.

- [ ] **A. Extract.** `perl reference/starlink-ast/selectfc reference/starlink-ast/sun_master.tex | sed -n '<start>,<end>p'` is the C-edition text to port; the line ranges in each task are of the *unfiltered* file, so find the section by its `\section` heading in the filtered output.
- [ ] **B. Port the prose** into `crates/ast-core/src/guide/<name>.md` following Global Constraints. LaTeX → Markdown: `\section` → `#`, `\subsection` → `##`, `\subsubsection` → `###`; `\htmlref{text}{label}` / `\ref` → intra-doc link to the chapter module (`[text](crate::guide::frames)`) or heading anchor (`crate::guide::frames#the-domain-attribute`); `\textit`/`\emph` → `*…*`; `\verb`/`\texttt` → backticks; `\begin{terminalv}` → fenced code. Add each `\label` to the README section table.
- [ ] **C. Port each kept example** to a ` ```rust ` doctest using only public API (`use ast_core::…`), ending in `Ok::<(), ast_core::error::AstError>(())` when it uses `?`. Replace printed output with `assert_eq!` on the value SUN/211 shows. Where the Rust value differs: if C would produce SUN/211's value, it is a port bug → stop the chapter, fix in a separate normal-author commit (with a test in the area program), resume. If an example needs an API that does not exist → **stop and ask the user**.
- [ ] **D. Declare** the chapter in `guide/mod.rs` and add its line to the table of contents in the module doc.
  Set the fate of each of the chapter's rows in the README section table (a Markdown table `| line | title | fate |`), and lower `GUIDE_SECTIONS_PENDING` by the number resolved.
  If the chapter uses figures, convert them first by Task 2's step F1, in its own commit.
- [ ] **E. Verify:**
  - `cargo test -p ast-core --doc guide` → all pass.
  - `cargo doc -p ast-core --no-deps` → no warnings.
  - `cargo test -p ast-core --test meta test_guide:: test_c_docs_audit::` → pass.
  - `rg -n '\bast[A-Z]\w+|\.c\b|\bC\b' crates/ast-core/src/guide/<name>.md` → every hit is justified (a stated difference from C), else reword.
- [ ] **F. Authorship.** `cargo run -q -p audit-tools --bin guide-authors -- <start> <end>` prints `author` / `co-author` lines; record them in the README authorship table.
- [ ] **G. Commit** with fmt/clippy run first:

```bash
git add crates/ast-core/src/guide/
git commit --author="<author line from F>" -m "guide: <chapter title>

Ported from SUN/211.

Generated with AI

Co-authored-by: <co-author line from F, omit if none>
Co-Authored-By: SLAC AI"
```

---

### Task 1: Guide skeleton, gates, authorship tool, Introduction and Cargo chapter

**Files:**
- Create: `crates/audit-tools/src/guide_authors.rs`, `crates/audit-tools/src/bin/guide-authors.rs`
- Modify: `crates/audit-tools/src/lib.rs` (add `pub mod guide_authors;`)
- Create: `crates/ast-core/src/guide/{mod.rs,README.md,intro.md,cargo.md}`
- Modify: `crates/ast-core/src/lib.rs` (add `pub mod guide;` in sorted position; its doc comes from `guide/mod.rs`)
- Create: `crates/ast-core/tests/test_guide.rs`; Modify: `crates/ast-core/tests/meta/main.rs` (add `#[path = "../test_guide.rs"] mod test_guide;` in sorted position)

**Interfaces:**
- Produces: `audit_tools::guide_authors::shares(blame_porcelain: &str, start: usize, end: usize) -> Vec<(String, f64)>` (author name → share 0..1, sorted descending); `audit_tools::guide_authors::credit(shares: &[(String, f64)]) -> (Option<String>, Option<String>)` (author identity, co-author identity, using the two identities from Global Constraints, threshold 0.10). The CLI `guide-authors <start> <end>` runs `git -C reference/starlink-ast blame -w -M -C --line-porcelain sun_master.tex`, applies `selectfc` semantics (lines inside `f+`…`f-` excluded; marker lines excluded), skips blank lines and lines whose first non-space char is `%`, and prints `author: Name <email>` and, if any, `co-author: Name <email>`. Unknown blame names (not Berry/Warren-Smith) count toward the denominator but are never credited. Names match by surname: `Berry` → David, `Warren-Smith` → Rodney.
- Produces: `crate::guide` module and the chapter-declaration pattern above.

- [ ] **Step 1: Write failing unit tests in `guide_authors.rs`**

```rust
#[test]
fn fortran_blocks_and_comments_are_not_counted() {
    // porcelain with: 2 Berry lines, 1 Berry line inside f+/f-, 1 Warren-Smith comment line, 1 Warren-Smith text line
    let s = shares(SAMPLE, 1, 7);
    assert_eq!(s, vec![("David Berry".into(), 2.0 / 3.0), ("Rodney Warren-Smith".into(), 1.0 / 3.0)]);
}
#[test]
fn under_ten_percent_is_not_credited() {
    let (a, c) = credit(&[("Rodney Warren-Smith".into(), 0.91), ("David Berry".into(), 0.09)]);
    assert_eq!(a.as_deref(), Some("Rodney Warren-Smith <rfws@star.rl.ac.uk>"));
    assert_eq!(c, None);
}
#[test]
fn larger_share_is_author() {
    let (a, c) = credit(&[("David Berry".into(), 0.6), ("Rodney Warren-Smith".into(), 0.3)]);
    assert_eq!(a.as_deref(), Some("David Berry <davidstuartberry@gmail.com>"));
    assert_eq!(c.as_deref(), Some("Rodney Warren-Smith <rfws@star.rl.ac.uk>"));
}
#[test]
fn other_authors_dilute_but_are_not_credited() {
    let (a, c) = credit(&[("Tim Jenness".into(), 0.5), ("David Berry".into(), 0.45), ("Rodney Warren-Smith".into(), 0.05)]);
    assert_eq!(a.as_deref(), Some("David Berry <davidstuartberry@gmail.com>"));
    assert_eq!(c, None);
}
```

`SAMPLE` is a hand-written `--line-porcelain` snippet (header line `<sha> <orig> <final> 1`, `author …`, tab-prefixed content) in the test module.

- [ ] **Step 2:** `cargo test -p audit-tools guide_authors` → FAIL (functions undefined).
- [ ] **Step 3:** Implement `shares`, `credit`, and the CLI. `start`/`end` are 1-based inclusive final-line numbers in `sun_master.tex`.
- [ ] **Step 4:** `cargo test -p audit-tools guide_authors` → PASS. Sanity: `cargo run -q -p audit-tools --bin guide-authors -- 9246 9384` (CmpFrames) prints only `author: David Berry <davidstuartberry@gmail.com>`.
- [ ] **Step 5: Write failing gates in `test_guide.rs`**

Four `#[test]` functions in `test_guide.rs` (module doc: `//! Gates on the user guide in ast_core::guide.`), each a hard assert:

| Test | Asserts |
|---|---|
| `every_chapter_file_is_declared` | the set of `guide/*.md` files other than `README.md` equals the set of `include_str!("<name>.md")` targets in `guide/mod.rs` |
| `guide_mentions_no_unported_class` | no `guide/*.md` other than `README.md` matches `\b(XmlChan|StcsChan|YamlChan|Plot|Plot3D)\b` |
| `intro_carries_the_copyright` | `intro.md` contains `Copyright (C) 2026 East Asian Observatory and AURA` |
| `readme_records_every_section` | every row of `guide/sections.tsv` has a README section-table row with the same line number, and its fate is a guide link, `dropped`, `merged into <guide link>`, or `pending`; the number of `pending` rows equals `GUIDE_SECTIONS_PENDING` |

The submodule is absent in CI, so the section list is committed as `crates/ast-core/src/guide/sections.tsv` (`line<TAB>level<TAB>title`, the C-edition `\section` / `\subsection` headings between lines 97 and 18621, generated in Step 6).
`GUIDE_SECTIONS_PENDING` is a `const usize` in `test_guide.rs` that may only go down; register it in `test_verification_ledger.rs` and `PLAN.md` §6, as `C_CITATIONS_UNCLEANED` is. Every chapter task lowers it by the rows it resolves.

- [ ] **Step 6:** Generate `sections.tsv` (a one-off `perl selectfc | rg -n` pipeline; record the command in README) and write `README.md` with: source pin (`\stardocversion` resolved from `reference/starlink-ast/component.xml` or the latest `211.x` in the release notes — 211.34 — plus submodule commit `c6c60753`), the regeneration command, the section table (all rows `pending` except those this task resolves), and an empty authorship table.
- [ ] **Step 7:** Mark as `dropped` now: Release Notes and all reference sections (17868–18054), Plot (16797–17651), XmlChan (15289–15423), StcsChan (15424–15437), and in How To… the three Plot recipes (2138, 2409, 2466) and "Producing Graphical Output" (681). Set `GUIDE_SECTIONS_PENDING` to the remaining count.
- [ ] **Step 8:** `cargo test -p ast-core --test meta test_guide::` → FAIL only on `every_chapter_file_is_declared` / `intro_carries_the_copyright` (no files yet).
- [ ] **Step 9: Write `mod.rs`, `intro.md`, `cargo.md`.**
  - `mod.rs` module doc: title `# The AST user guide`, one paragraph saying the guide explains the library's concepts and the API docs describe each item, then the table of contents (links to each declared chapter).
  - `intro.md`: port lines 97–285 (What Problems…, Other Design Objectives, What Does "AST" Stand For?) per the procedure. Add, at the top after the title, a short paragraph: this crate is a Rust port of the Starlink AST library; this guide is adapted from SUN/211, the guide to the C library, by Rodney Warren-Smith and David Berry; and the copyright line. Remove references to Fortran and to the C/Fortran editions except in that paragraph.
  - `cargo.md`: title `# Using AST from Cargo`; a `[dependencies] ast-core = …` snippet (a `toml` fence, not a doctest), a sentence that the vendored C (PAL, ERFA, WCSLIB) is built by the crate's build script with a C compiler and needs nothing installed, and one doctest that builds a `SkyFrame` and formats a coordinate. Normal author (no SUN/211 prose).
  - Resolve README rows for 97–285 and 17652–17867 (`replaced by guide::cargo`).
- [ ] **Step 10:** Run procedure step E. Expected: all pass.
- [ ] **Step 11: Commit in two:** (a) tooling, gates, `mod.rs`, `README.md`, `sections.tsv`, `cargo.md`, ledger registration — normal author; (b) `intro.md` — authorship from `guide-authors 97 285`.

### Task 2: Overview of AST Concepts

Lines 286–749 → `concepts.md`. Drop "Producing Graphical Output" (681).
"Input/Output Facilities" lists Channel classes: keep Channel, FitsChan, MocChan; remove XmlChan/StcsChan/YamlChan mentions.
Figures `mapping`, `series`, `parallel`, `complex`, `frames`, `cmpframe`, `frameset` are first used here: convert them in this task (Step F1 below) and embed.

- [ ] **F1. Figures** (normal author, own commit). Conversion is a one-off; the converter is not a project dependency.
  - Get `pdftocairo` into a scratch env: `conda create -y -p <scratch>/poppler -c conda-forge poppler`.
  - For each figure the chapter uses: `<scratch>/poppler/bin/pdftocairo -svg reference/starlink-ast/sun211_figures/<n>.pdf crates/ast-core/src/guide/figures/<n>.svg`.
  - Embed by inlining: in the chapter Markdown, the figure is included as raw HTML via a `<figure>` wrapper containing the SVG text. rustdoc passes raw HTML through, and `include_str!` cannot splice into Markdown, so paste the SVG body (strip the XML prolog) between `<figure>` and a `<figcaption>` carrying the LaTeX caption. If one SVG exceeds 200 KiB, run it through `rsvg-convert -f svg` to flatten it and retry; if still over, drop the figure and record it in README.
  - Render each SVG (`rsvg-convert -f png`) and look at it: drop any figure that shows C-only content (C function names, pointer arrows labelled with C calls) and record it in README.
  - Record the conversion commands in README.
- [ ] Procedure A–G.

### Task 3: How To…

Lines 750–2586 → `howto.md`.
Drop "Obtain and Install AST" (point to `guide::cargo` instead — mark `merged into guide::cargo`), "Build an AST Program" (same), and the three Plot recipes.
"Structure an AST Program": rewrite around `fn main() -> AstResult<()>` and `?`; drop `astBegin`/`astEnd`.
"Read a WCS Calibration from a Dataset" and later recipes: inline a FITS header as a string read through `FitsChan` (use a header from `crates/ast-core/tests/fixtures/` that SUN/211's example matches, else write the cards SUN/211 shows).
"Test if a WCS is a Celestial/Spectral Coordinate System": use `fs.frame(FrameRef::Current)?.as_any().is::<SkyFrame>()`; the spectral recipe tests `SpecFrame` **or** `DSBSpecFrame` (Review Focus 1) with a doctest that asserts a DSBSpecFrame passes.
"Display AST Data": `show()`.

- [ ] Procedure A–G.

### Task 4: An AST Object Primer, Mappings, CmpMaps

Three chapters, three commits.

`objects.md` (2587–4386). Drop: "Object Creation and Pointers" is rewritten as "Object Creation" (constructors/builders, owned values); drop "Managing Object Pointers", both "Pointer Contexts", "Exporting, Importing and Exempting", "C Pointer Types", "Sharing the Error Status" (mark `dropped` in README). "Getting/Setting Attribute Values" and "Testing, Clearing and Defaulting" become typed accessors (`label(1)`, `set_label(1, …)`, `test_label`, `clear_label`), with no `astSet` strings. "Copying Objects" → `clone()` as a deep copy. "Error Detection" → `AstResult`, `AstError`, `ErrorCode`, `?`. "Multi-threaded Applications" → objects are `Send` and not `Sync`; move them between threads, do not share references. "The Object Hierarchy" → describe the `Mapping` / `Frame` / `Region` traits briefly and link to them; do not reproduce the C class tree.

`mappings.md` (4387–5054). "Transforming Coordinates" uses `tran1` / `tran2` / `transform`. Bad values → `BAD_VALUE`. "Inverting Mappings": `invert()` mutates in place; show it.

`cmpmaps.md` (5055–5564): series/parallel via `then` / `alongside`; simplification via `simplify()`. Figure `simpexamp`.

- [ ] Procedure A–G for each chapter.

### Task 5: Frames, SkyFrames, SpecFrames, TimeFrames, CmpFrames

Five chapters, five commits.

`frames.md` (5565–7588). "Using a Frame as a Mapping" (5642) is the **one** place the Frame/Mapping difference is stated: say that in the C library a Frame can be used as a Mapping (a unit transformation on its axes), that in Rust `Frame` and `Mapping` are separate traits so it cannot, and show a doctest using `UnitMap::new(frame.naxes())` instead. Note that FrameSets and Regions implement both. Check `rg -n "used as a Mapping|is not a Mapping" crates/ast-core/src/guide` returns only this file before committing (Review Focus 3).
`skyframes.md` (7589–8282), `specframes.md` (8283–9094, figures `dsbspec1–3`), `timeframes.md` (9095–9245), `cmpframes.md` (9246–9384).

- [ ] Procedure A–G for each chapter.

### Task 6: Conversions, FrameSets, Higher Level Operations on FrameSets

Three chapters, three commits.

`conversions.md` (9385–10181): `convert_to`.
`framesets.md` (10182–10913): figures `fsexample`. "Adding New Frames to a FrameSet": state that `add_frame` takes its own copy, so later changes to the original Frame do not reach the FrameSet; doctest: add a `SkyFrame`, call `set_epoch…` on the original, assert the FrameSet's current Frame still has the old value (Review Focus 2). "Using a FrameSet as a Mapping/Frame": FrameSet implements both traits.
`frameset_ops.md` (10914–12011): figures `fsconvert`, `fsalign`, `fsremap`, `fsmerge`. "Creating FrameSets with astConvert" → "Creating FrameSets by conversion" (`convert_to`). "Maintaining the Integrity of FrameSets": explain integrity and `frame_mut()` as the way around it.

- [ ] Procedure A–G for each chapter.

### Task 7: Channels, FitsChans, Foreign FITS Encodings

Three chapters, three commits.

`channels.md` (12012–13268): `Channel`, `NativeChannel`, `Source` / `Sink`, `StringSource` / `StringSink`, `FileSource` / `FileSink`. "Source and sink functions" → the `Source` / `Sink` traits. "Reading Objects from Files" uses a fixture under `crates/ast-core/tests/fixtures/` via `include_str!("../../tests/fixtures/…")` and a `StringSource`, so the doctest needs no files at run time.
`fitschans.md` (13269–14474) and `foreign_fits.md` (14475–15288): headers inline as strings.

- [ ] Procedure A–G for each chapter.

### Task 8: IntraMaps

`intramaps.md` (15438–16796). Rewrite "Writing a Transformation Function" around implementing `IntraTransform`; "Registering…" around `Channel::register_intra` and `IntraRegistry`; "Writing and Reading IntraMaps" around `from_native_string_with`. Drop "Managing Transformation Functions in Libraries" if it is only about C linking (mark in README). One commit.

- [ ] Procedure A–G.

### Task 9: FITS-WCS Coverage and close-out

`fits_wcs.md` (18055–18621). Commit by procedure.

Then, normal author:
- [ ] `GUIDE_SECTIONS_PENDING` is 0; set it so and update `test_verification_ledger.rs` and `PLAN.md` §6.
- [ ] Add a `PLAN.md` row recording that the guide is done, with the source pin, and a line in `CLAUDE.md`'s upstream sync protocol: "On a pin move, `git diff <guide pin>..<new pin> -- sun_master.tex` and port changed sections listed in `crates/ast-core/src/guide/README.md`."
- [ ] Update the spec's Status line to `implemented`.
- [ ] Full gate: fmt, clippy, `cargo doc --workspace --no-deps`, `cargo test -p ast-core` → green. Commit.

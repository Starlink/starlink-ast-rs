# User guide from SUN/211: design

**Status:** implemented, 2026-10-23.
**Scope:** the narrative chapters of SUN/211 (`reference/starlink-ast/sun_master.tex`), ported into `ast-core`'s rustdoc as the `ast_core::guide` module, with Rust examples as doctests and links to the Rust API.

## Context

This is the third of three pieces of documentation work.
The first, `show` and `to_native_string` (`2026-10-19-show-and-native-string-design.md`), and the second, the C documentation audit (`2026-10-19-c-docs-audit-design.md`), are done.
The audit moved the C reference headers into the item docs; this work moves the narrative that explains how the library fits together.

SUN/211 is the C edition of the AST guide; `sun_master.tex` also builds the Fortran edition (SUN/210).
Rodney Warren-Smith and David Berry wrote the text, and the guide keeps their words.

## Intent

Give a Rust user the explanation of AST that SUN/211 gives a C user: what Mappings, Frames and FrameSets are, how they combine, and how to read and write them, in the order SUN/211 teaches it.
This is a port of the text, not a rewrite.
The words change only where they make no sense for Rust.

## Goals

1. **The guide reads as SUN/211 does.**
   Chapters, sections and prose follow the LaTeX in order; wording is changed only where the C wording is wrong or meaningless for Rust.
2. **Examples are Rust, and they run.**
   Each C example that is kept becomes a Rust doctest, so it compiles and runs under `cargo test -p ast-core` in CI.
3. **Names link to the API.**
   Classes, methods and attributes the text names are intra-doc links to the Rust items, so the guide and the item docs filled in by the C audit form one set of documentation.
4. **C appears only where it matters.**
   The introduction says the library is a port of the C AST library and names SUN/211 as the source of the guide.
   After that, the text mentions C only where the Rust library behaves differently from C.
   The one known difference is that a Frame cannot be used as a Mapping (see below); any other found while porting is stated the same way.

## Non-goals

- Reference material that rustdoc already generates from the public API: the class hierarchy, the function, routine, attribute and class description sections, and the release notes.
- Fortran: the `f` edition text and its examples.
- An mdBook or other site; the guide may move to one later, and nothing here should make that harder than copying the Markdown chapters.
- New explanatory material beyond what porting a passage requires.

## Anti-goals

- **Rewriting the authors' text in Claude's voice.** Rewording beyond what Rust requires fails the work even if the result reads well.
- **C mechanics described as AST behavior.** A passage that leaves C handle management in place, or that says Rust "differs" where it only differs in memory management, misleads the reader.
- **Examples that do not compile.** Every example is a doctest that runs (`no_run` or `ignore` only with a stated reason, such as needing a file that is not in the crate).

## Chapters

In SUN/211's order:

| SUN/211 section | Guide |
|---|---|
| Introduction | ported, with the port note (below) |
| Overview of AST Concepts | ported |
| How To… | ported, omitting recipes for unported classes |
| An AST Object Primer | ported, rewritten where it describes C object handling (below) |
| Inter-Relating Coordinate Systems (Mappings) | ported |
| Compound Mappings (CmpMaps) | ported |
| Representing Coordinate Systems (Frames) | ported |
| Celestial Coordinate Systems (SkyFrames) | ported |
| Spectral Coordinate Systems (SpecFrames) | ported |
| Time Systems (TimeFrames) | ported |
| Compound Frames (CmpFrames) | ported |
| An Introduction to Coordinate System Conversions | ported |
| Coordinate System Networks (FrameSets) | ported |
| Higher Level Operations on FrameSets | ported |
| Saving and Restoring Objects (Channels) | ported |
| Storing AST Objects in FITS Headers (FitsChans) | ported |
| Using Foreign FITS Encodings | ported |
| Storing AST Objects as XML (XmlChan) | dropped |
| Reading and writing STC-S descriptions (StcsChans) | dropped |
| Creating Your Own Private Mappings (IntraMaps) | ported, around the Rust `IntraTransform` and `Channel::register_intra` |
| Producing Graphical Output (Plots) | dropped |
| Compiling and Linking Software that Uses AST | replaced by a short section on adding `ast-core` with Cargo |
| The AST Class Hierarchy | dropped (rustdoc) |
| AST Function / Routine / Attribute / Class Descriptions | dropped (rustdoc) |
| UNIX Command Descriptions | dropped |
| AST Memory Management and Utility Functions | dropped; see "Memory management" |
| FITS-WCS Coverage | ported; rustdoc has no equivalent |
| Release Notes | dropped |

Within the ported chapters, passages, examples and cross-references about XmlChan, StcsChan, YamlChan and Plot are removed, and the surrounding text is joined up so it still reads.
If a dropped class is only mentioned in passing (for example in a list of Channel classes), the mention is removed rather than flagged as unported.

## Rewriting rules

These say what "changed where it makes no sense in Rust" means.

**C mechanics become Rust.**
Constructor calls with option strings (`astSkyFrame("System=FK5")`) become builders or constructors plus typed setters.
`astSet` / `astGet` attribute strings become the typed accessors (`set_system`, `system`).
"A pointer to" an object becomes the object.
`status` and `astOK` become `AstResult` and `?`.
`AST__BAD` becomes `BAD_VALUE`.
`astTran2` and friends become the `Mapping` transform methods on `Points` / `PointsMut`.
None of these are stated as differences from C; the text simply describes the Rust.

**Memory management.**
The Rust library uses owned values: `Clone` is a deep copy, an object is freed when it is dropped, and there are no handles, reference counts, `astAnnul`, `astClone`, `astBegin` / `astEnd` or `astExempt`.
Passages that exist only to explain C handle management (object contexts, annulling, exempting) are removed.
Passages that use handle sharing to explain AST behavior are rewritten in owned-value terms.
Two need particular care:

- Where C shares one object between two holders (adding the same Frame to two FrameSets, or a Mapping into a CmpMap), the Rust holder has its own copy, so changing the original afterwards does not change the holder. The text says what the Rust code does.
- Where C's `astCopy` versus `astClone` distinction carries meaning, the Rust text uses `clone()` (always a deep copy) and drops the distinction.

These are not described as differences from C behavior, because they concern how objects are held, not what AST computes.

**Frames are not Mappings.**
In C a Frame is also a Mapping (a UnitMap on its own axes), so a Frame can be passed wherever a Mapping is expected.
The Rust `Frame` and `Mapping` are separate traits and a Frame cannot be used as a Mapping.
Where SUN/211 relies on this, the guide says so once, in the Frames chapter, and shows the Rust way (for example a `UnitMap` of the Frame's axis count).
FrameSets and Regions do implement both traits and are unaffected.

**Axis indices stay 1-based**, as in SUN/211 and the Rust API.

**Examples.**
Each C example in a kept passage is rewritten as Rust using only the public API, as a doctest.
Printed output shown in SUN/211 is checked against what the Rust prints.

**When an example exposes a problem in the port.**
A real bug (the Rust computes or prints something C would not) is fixed in the port, following the project's usual rules for porting from C, in its own commit with the normal author; the guide never records a port bug as a difference from C.
A missing public API (the example cannot be written without something the Rust library does not offer) stops the work on that example, and the options are discussed with the user before anything is added.
Class-membership tests are not missing: `Frame` has the public `AsAny` supertrait, so `frame.as_any().is::<SkyFrame>()` or `downcast_ref::<SkyFrame>()` replaces `astIsASkyFrame`, and `AnyObject` has `as_mapping` / `as_frame` / `as_region` and `TryFrom`.
`is::<T>()` matches the exact type only, where `astIsA<Class>` is also true for subclasses (a DSBSpecFrame is a SpecFrame); where SUN/211 relies on that, the Rust example tests each type it means.
Examples that read a file use a fixture already in `crates/ast-core/tests/fixtures/` or inline the FITS header as a string.

**Links.**
Every class, method, attribute accessor and trait the text names is an intra-doc link.
The existing `cargo doc` CI step denies broken intra-doc links, so a bad link fails CI.

**No C internals.**
The guide is under the existing rendered-docs gate in `test_c_docs_audit.rs`: no C source file names, line numbers, or internal `ast<Name>` names in the rendered pages.
C function names may appear only in the introduction and where a difference from C is stated.

## Shape

- `crates/ast-core/src/guide/mod.rs` declares `pub mod guide` (in `lib.rs`), with the guide's table of contents as its module doc.
- One submodule per chapter, each with no items and its text in `crates/ast-core/src/guide/<chapter>.md`, declared in `guide/mod.rs` with `chapter!(<chapter>)`.
  The build script copies each chapter to `$OUT_DIR/guide/` with its figure links inlined (below), and the module documentation is that copy.
  Plain Markdown files keep a later move to mdBook a copy.
- Cross-references between sections (`\label` / `\htmlref` in the LaTeX) become intra-doc links to the chapter module and, where needed, its heading anchor.
- One sentence per line in the Markdown, American English spelling (the user's documentation conventions).
  The LaTeX's British spellings (`serialise`, `colour`) are converted.

**Figures** (Claude's call).
The SUN/211 figures are PDFs in `reference/starlink-ast/sun211_figures/`.
Those for kept chapters (mapping, series, parallel, complex, frames, cmpframe, frameset, simpexamp, dsbspec1–3, fsexample, fsconvert, fsalign, fsremap, fsmerge) are converted to SVG and committed under `crates/ast-core/src/guide/figures/`, and referred to from the chapter Markdown as ordinary images, `![caption](figures/<name>.svg)`.
The build script replaces each such link with a `data:` URI holding the SVG, so the figures render without the submodule and without hosting, and the ids inside one SVG cannot collide with another's.
The front-page and Plot figures are dropped.
If a converted figure shows C-only content (C function names in a box, for example), it is redrawn or dropped rather than shipped wrong.

## Copyright

The guide is "Copyright (C) 2026 East Asian Observatory and AURA".
The notice appears in the introduction chapter, which is the guide's front page in rustdoc.

## Provenance and upstream drift

`crates/ast-core/src/guide/README.md` (not rendered; Claude's call) records the `sun_master.tex` version the guide was ported from (its `\stardocversion` and the submodule commit) and a table of every LaTeX `\section` and `\subsection` with its fate: the guide chapter and heading it became, or dropped, or merged into another.
On an upstream sync, `git diff <recorded commit>..<new pin> -- sun_master.tex` shows the changed sections, and the table says where each lives in the guide.
No automated freshness gate is added.

## Authorship

The words are substantially those of the two SUN/211 authors, Rodney Warren-Smith and David Berry, in proportions that differ by chapter.
Each chapter lands in its own commit, credited by the chapter's blame share in `sun_master.tex`:

- The share is the fraction of the chapter's non-blank, non-comment lines that `git blame -w -M -C` on `reference/starlink-ast/sun_master.tex`, at the submodule commit recorded in the guide's README, attributes to each person.
- An author with a share of 10% or more is credited; the one with the larger share is the commit author and the other is a `Co-authored-by:` trailer.
- An author with a share under 10% is not credited.
- Identities: `David Berry <davidstuartberry@gmail.com>` and `Rodney Warren-Smith <rfws@star.rl.ac.uk>`.
- The trailers end with the usual AI attribution lines; the committer is unchanged.

At the current pin this makes Warren-Smith the author, with Berry as co-author, for most chapters (Introduction, Overview, Frames, SkyFrames, Coordinate System Conversions, Channels, FitsChans, Foreign FITS encodings), Warren-Smith alone for the Object Primer, Mappings, CmpMaps, FrameSets, Higher Level Operations on FrameSets and IntraMaps, and Berry alone for SpecFrames, TimeFrames and CmpFrames.
How To… (Warren-Smith 79%, Berry 9%) is just under the threshold for Berry; the builder recomputes every share at port time rather than relying on these figures.
The FITS-WCS Coverage section's shares are computed the same way.

Commits with no SUN/211 prose (the Cargo section, the guide skeleton, figures, README, port fixes found by examples) keep the normal author.

## Phases

Each phase lands with `cargo test -p ast-core` (including the doctests), `cargo doc --workspace --no-deps`, clippy and fmt green.

1. **Skeleton:** `guide` module, introduction (with the port note), the Cargo section, and Overview of AST Concepts.
2. **Objects and Mappings:** How To…, Object Primer, Mappings, CmpMaps.
3. **Frames:** Frames, SkyFrames, SpecFrames, TimeFrames, CmpFrames.
4. **Conversions and FrameSets:** coordinate system conversions, FrameSets, higher-level FrameSet operations.
5. **I/O:** Channels, FitsChans, foreign FITS encodings, FITS-WCS coverage.
6. **IntraMaps.**

## Left to the builder

- How each chapter's sections map onto Markdown headings and anchors.
- Which C examples earn a doctest when a passage has several similar ones.
- The SVG conversion tool and any cleanup of the converted figures.
- Whether a How To… recipe that depends on an unported class can be kept by pointing at a ported equivalent, or is dropped.

# Plot Stage 2: `Grf`, `LogGrf`, `SvgGrf` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust-native `Grf` trait for Plot's drawing backend, and two backends in `starlink-ast`, `LogGrf` and `SvgGrf`, that reproduce C `grf_log.c`'s call log and SVG byte for byte.

**Architecture:** `grf_log.c` is one module doing two jobs over one model: a "virtual page" (200 mm square, world coordinates 0 to 1, 4 mm characters, a text-width rule) with an attribute store, and two optional outputs, a call log and SVG.
The port keeps the model once, as a private `VirtualPage` that answers `scales`, `char_height`, `text_extent` and `attr` exactly as C does, and builds two independent backends on it: `LogGrf` writes the log, `SvgGrf` writes the SVG.
The trait has one method per C function in `grf.h`, at C's granularity, with `f32` geometry and `f64` attribute values (spec, "Single precision" and "The trait follows C's granularity").

**Tech Stack:** Rust 2024, `starlink-ast`; a C probe compiled with `reference/starlink-ast/src/grf_log.c` against the reference `libast` (`-ffp-contract=off`).

**Spec:** [`docs/superpowers/specs/2026-10-30-plot-design.md`](../specs/2026-10-30-plot-design.md), stage 2 and gate 3.
Parent: [`2026-04-20-rust-port-design.md`](../specs/2026-04-20-rust-port-design.md), which this stage amends (backends move from `ast-plugins` into `starlink-ast`).

## Global Constraints

- Branch `u/timj/plot-2-grf`, stacked on `u/timj/plot-1-frame-methods`; one PR.
- No new dependencies; no C in `starlink-ast`. `ast-plugins` is left alone (stage 7 decides its fate).
- Every coordinate, scale and box is `f32` and every intermediate C computes in `float` is computed in `f32`, in C's operation order; values C computes in `double` are `f64` (`grf_log.c` mixes both).
- Formatted numbers in the log use the crate's C `printf` port, `frames::axis_format::sprintf_double`, never Rust `format!` precision, so `%.3f`/`%.4f`/`%.6f` round as C's do.
- A new source file opens with the AURA copyright and `SPDX-License-Identifier: LGPL-3.0-or-later` lines; generated `cases.rs` is exempt.
- `#![warn(missing_docs)]`: every public item documented, for Rust readers; C names only in `//` comments or a `# See also` section.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` (Rust 1.100), `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps` clean.
- `CHANGELOG.md` `Unreleased` → `Added` entry in the commit that makes each type public.
- Per-task test run: `cargo test -p starlink-ast --test frames test_grf::`; full `cargo test -p starlink-ast` once, in Task 2.
- Strip conda `CC`/`LDFLAGS`/`CFLAGS` for cargo and probe builds.

## Decisions

- **Module:** `crates/starlink-ast/src/grf/` — `mod.rs` (trait and types), `page.rs` (`VirtualPage`, private), `log.rs` (`LogGrf`), `svg.rs` (`SvgGrf`, `SvgOptions`). Public as `starlink_ast::grf`.
- **Test area:** the `frames` test program (`tests/frames/main.rs`), as `test_grf.rs`; Plot's own tests will join it.
- **Attribute "no change":** C passes `AST__BAD` to query without setting; the trait takes `Option<f64>`, `None` meaning query only. The future C adapter maps `AST__BAD` to `None`.
- **Justification:** an enum pair, not a string. `VJust` has C's four codes `Bottom` (`M`), `Baseline` (`B`), `Centre` (`C`), `Top` (`T`); `HJust` has `Left`, `Centre`, `Right`. `Justification::code()` gives C's two characters, which the log prints. `grf_log.c` accepts only `T`/`C`/`B` vertically, so `VirtualPage` treats `Bottom` as `Centre` for the extent, as C treats `M`.
- **Mismatched slices:** `line` and `mark` take `x` and `y` slices; unequal lengths are `ErrorCode::Grfer` (C has one count and cannot express the mismatch).
- **Writers:** both backends are generic over `W: std::io::Write`; an I/O failure is `ErrorCode::Grfer` carrying the I/O error's text. `SvgGrf::new` writes the SVG prologue; `SvgGrf::finish(self) -> AstResult<W>` writes `</svg>` and flushes (C's `astGrfLogClose`); `LogGrf::finish(self) -> AstResult<W>` flushes. Dropping without `finish` leaves an unterminated SVG, documented.
- **`SvgOptions`:** `#[non_exhaustive]`, `Default` = width 720, height 540, public fields `width: u32`, `height: u32`; later options are added as fields with C's value as default.
- **Text width:** one private function, `page::text_width_mm(text: &str, chr_ht: f64) -> f32`, C's `strlen( text ) * chr_ht * 0.6` with `strlen` as the byte length, so a later font-metrics option replaces one function.
- **Oracle shape:** the probe holds the call sequence as a C table and emits `cases.rs` with that sequence as a Rust `const` slice of a generated `Call` enum, the exact value each call returned (`%.9g` floats, error flags), and the whole log and SVG as raw strings. The Rust test replays the slice through each backend; there is one copy of the sequence.

## Review Focus

1. **A zero-length up-vector.** C logs `GScales`, reports `AST__GRFER` and writes no `GTxExt` line; `text_extent` returns `Err(Grfer)` after the same log line. Pinned in Task 1.
2. **Nested logging.** `GQch` and `GTxExt` call `GScales`, which logs its own line first; an empty `GTxExt` text skips that call. Pinned in Task 1.
3. **SVG text rotation.** `iangle = (int)( angle / 5.0 ) * 5`, then 5 less when the angle is negative and `iangle != (int)angle`, and no transform at all within 0.01 degrees of zero: -0.005 (none), -2.5 (-5), -7.5 (-10), -10 (-10), -10.5 (-10), -12 (-15), 92.3 (90), 180 (180). Pinned in Task 2.
4. **Out-of-range attribute values in SVG.** A colour index outside 0..15 draws as 1 (red); a line width below 1 draws as 1; a text size at or below 0 sizes as 1. Pinned in Task 2.
5. **Non-ASCII text.** The width rule counts bytes, so `"°"` is two characters wide, as in C; SVG text is written unescaped, as C writes it. Pinned in Tasks 1 and 2.

---

### Task 1: The trait, the virtual page, and `LogGrf`

**Files:**
- Create: `crates/starlink-ast/src/grf/mod.rs`, `crates/starlink-ast/src/grf/page.rs`, `crates/starlink-ast/src/grf/log.rs`
- Modify: `CHANGELOG.md`
- Modify: `crates/starlink-ast/src/lib.rs` (`pub mod grf;`)
- Create: `tools/oracle_fixtures/grf_log_probe.c`
- Create: `crates/starlink-ast/tests/grf_log_oracle/cases.rs` (generated)
- Create: `crates/starlink-ast/tests/test_grf.rs`; register in `crates/starlink-ast/tests/frames/main.rs`

**Interfaces:**
- Produces, in `starlink_ast::grf`:
  ```rust
  pub enum GrfAttr { Style, Width, Size, Font, Colour }          // GRF__STYLE.. GRF__COLOUR
  pub enum GrfPrimitive { Text, Line, Mark }                     // GRF__TEXT, LINE, MARK
  pub enum GrfCap { Escapes, MJust, Scales }                     // GRF__ESC, MJUST, SCALES
  pub enum VJust { Bottom, Baseline, Centre, Top }
  pub enum HJust { Left, Centre, Right }
  pub struct Justification { pub vertical: VJust, pub horizontal: HJust }
  impl Justification { pub fn code(&self) -> [u8; 2]; pub fn from_code(code: &str) -> AstResult<Self>; }
  pub struct TextBox { pub x: [f32; 4], pub y: [f32; 4] }        // C's xb/yb corners, in C's order

  pub trait Grf {
      fn line(&mut self, x: &[f32], y: &[f32]) -> AstResult<()>;                         // astGLine
      fn mark(&mut self, x: &[f32], y: &[f32], marker: i32) -> AstResult<()>;            // astGMark
      fn text(&mut self, text: &str, x: f32, y: f32, just: Justification, up: [f32; 2]) -> AstResult<()>;          // astGText
      fn text_extent(&mut self, text: &str, x: f32, y: f32, just: Justification, up: [f32; 2]) -> AstResult<TextBox>; // astGTxExt
      fn char_height(&mut self) -> AstResult<[f32; 2]>;                                 // astGQch: [chv, chh]
      fn scales(&mut self) -> AstResult<[f32; 2]>;                                      // astGScales: [alpha, beta]
      fn attr(&mut self, attr: GrfAttr, value: Option<f64>, prim: GrfPrimitive) -> AstResult<f64>; // astGAttr: previous value
      fn cap(&mut self, cap: GrfCap, value: i32) -> bool;                                // astGCap
      fn begin_buffer(&mut self) -> AstResult<()>;                                       // astGBBuf
      fn end_buffer(&mut self) -> AstResult<()>;                                         // astGEBuf
      fn flush(&mut self) -> AstResult<()>;                                              // astGFlush
  }
  ```
  The trait docs state C's contract for each method in Rust terms (what Plot relies on), including that `attr` both queries and sets, and that a backend holds its own attribute state.
- Produces, `pub(crate)` in `grf/page.rs`: `VirtualPage` with `new()`, `scales() -> [f32; 2]`, `char_height() -> [f32; 2]`, `text_extent(text, x, y, just, up) -> AstResult<Option<TextBox>>` (`None` for empty text, which C answers with an all-zero box and no `GScales` call), `attr(attr, value, prim) -> f64`, and read access to the stored attributes for the SVG writer. `text_width_mm` as in Decisions.
  Ports of `astGScales` (`grf_log.c:161`), `astGQch` (`:169`), `astGTxExt` (`:183`), `astGAttr` (`:364`), `ChrHtMM` (`:152`).

- [ ] **Step 1: Branch check.** `git status --short && git branch --show-current` → `u/timj/plot-2-grf`, clean.

- [ ] **Step 2: Write the probe.**
  `grf_log_probe.c` includes `grf.h` and `grf_log.h`, and is compiled together with `../src/grf_log.c`:
  `cd reference/starlink-ast/build && cc -I. -I../src ../../../tools/oracle_fixtures/grf_log_probe.c ../src/grf_log.c -L. -last -lm -Wl,-rpath,$PWD -o ../../../tools/oracle_fixtures/grf_log_probe`.
  A C table of calls, run once with both `astGrfLogInit( tmpfile() )` and `astGrfLogSetSvg( tmpfile(), 720, 540 )` active, then `astGrfLogClose()` (which must not close the tmpfiles before they are read back; write the footer, then rewind and read). After each call record its return and outputs; after an expected error, `astClearStatus`.
  The table covers: `GScales`; `GQch` before and after `GAttr SIZE` of 2.0 and of 0.0 and -1.0 on TEXT; `GTxExt` for all 12 justification pairs plus `M` (as `MC`), up-vectors (0,1), (1,0), (0.6,0.8), (-1,0), (0,0) (error), text `""`, `"12h"`, `"°"`; `GText` for each horizontal and vertical code, up-vectors giving angles 0, -0.005, -2.5, -7.5, -10, -10.5, -12, 92.3, 180, empty text; `GLine` with 0, 1, 2 points, closed polygon, open polyline of 5; `GMark` with 0 and 3 points; `GAttr` for each attribute on each primitive, set and query (`AST__BAD`), colour 3, 15, 16, -1, 2.6, width 0.5 and 3.4; `GCap` for each capability; `GFlush`, `GBBuf`, `GEBuf`.
  Emits, as `cases.rs`: a `Call` enum (one variant per C function with its arguments, justification as its two-character code), `CALLS: &[(Call, Expected)]` where `Expected` holds the exact returns (`f32` printed `%.9g`, `bool` for an error), `LOG: &str` and `SVG: &str` as raw strings (`r#"..."#`).

- [ ] **Step 3: Write the failing tests** in `test_grf.rs` (header naming the probe), through the public API only:
  `log_grf_matches_the_c_library` replays `CALLS` through `LogGrf::new(Vec::new())` with a shared `replay(&mut dyn Grf)` helper that asserts every returned `f32` equal bit for bit (`to_bits`), every returned `f64` equal, and every expected error an `Err` with `ErrorCode::Grfer`; then `String::from_utf8(grf.finish()?)` equals `cases::LOG` exactly.
  `justification_round_trips_its_code`: `from_code(c).code()` for all 13 codes, and `from_code("XX")` is `Err(Grfer)`.

- [ ] **Step 4: Run, see it fail to compile.** `cargo test -p starlink-ast --test frames test_grf::`

- [ ] **Step 5: Implement** the types, the trait, `VirtualPage` and `LogGrf` (below). Keep `grf_log.c`'s mixed precision exactly: `float` locals (`ux`, `uy`, `uplen`, `phys_hu`, `phys_hd`, `phys_w`, `padded_w`, `cx_mm`, `cy_mm`, `xc`, `yc`, the half sizes and deltas) as `f32`; `chr_ht` as `f64`; `uplen` as `(float)sqrt((double)(ux*ux+uy*uy))` — the sum in `f32`, the root in `f64`.
  `LogGrf<W: Write>`: `LogGrf::new(writer: W) -> Self` (C's `astGrfLogInit`, attributes reset to 1.0); `LogGrf::finish(self) -> AstResult<W>` flushes and returns the writer.
  Each method writes exactly C's line (`grf_log.c` formats), including the nested `GScales` line inside `char_height` and non-empty `text_extent`, the `GLine` line only when there are at least 2 points and the first and last differ, the `GMark` line with the first point only, and `GAttr` only when a value is set. `begin_buffer`/`end_buffer` write nothing; `flush` writes `GFlush` and flushes. `cap` returns true only for `Scales`. Text is written between double quotes unescaped, as C's `%s`.

- [ ] **Step 6: Run, it passes.** Same command.

- [ ] **Step 7: CHANGELOG** `Added`: `starlink-ast: the grf module: the Grf trait, through which a Plot draws, and LogGrf, a backend that logs each drawing call as the C library's grf_log does.` **Commit** `Add the Grf trait and LogGrf, which logs drawing calls as C's grf_log does`.

### Task 2: `SvgGrf`, and the spec amendment

**Files:**
- Create: `crates/starlink-ast/src/grf/svg.rs`
- Modify: `crates/starlink-ast/src/grf/mod.rs`, `crates/starlink-ast/tests/test_grf.rs`, `CHANGELOG.md`
- Modify: `docs/superpowers/specs/2026-04-20-rust-port-design.md` (rev 43: line 569 tree comment, delivery step 10 at line 1614, a revision-history entry), `docs/superpowers/specs/2026-10-30-plot-design.md` (mark stage 2 done and record the trait decisions above under "Calls made by Claude"), `PLAN.md` (a §7 row for Plot stage 2)

**Interfaces:**
- Consumes: Task 1.
- Produces: `pub struct SvgOptions { pub width: u32, pub height: u32 }` (`#[non_exhaustive]`, `Default` 720×540); `pub struct SvgGrf<W: Write>`; `SvgGrf::new(writer: W, options: SvgOptions) -> AstResult<Self>` writes the prologue of `astGrfLogSetSvg` (`grf_log.c:105`); `SvgGrf::finish(self) -> AstResult<W>` writes `</svg>\n`.
  Ports `SvgX`, `SvgY`, `SvgColour`, `SvgFontSize` and the SVG halves of `astGText`, `astGLine`, `astGMark`. `SvgX` is `(int) round( wx * (float)svg_w )`: the product in `f32`, `round` half away from zero (`f32::round`), then truncation to `i32`; the rotation angle is `f64`.
  `SvgGrf` writes no log; its `scales`, `char_height`, `text_extent` and `attr` answer from its own `VirtualPage`.

- [ ] **Step 1: Write the failing test** `svg_grf_matches_the_c_library`: replay `CALLS` through `SvgGrf::new(Vec::new(), SvgOptions::default())`, assert returns, and `finish()` equals `cases::SVG` exactly.
  Add `svg_options_are_additive`: `let mut o = SvgOptions::default(); o.width = 800;` compiles from the test crate (guards `#[non_exhaustive]` plus public fields).
- [ ] **Step 2: Run, see it fail.**
- [ ] **Step 3: Implement** `SvgGrf`.
- [ ] **Step 4: Run, it passes.**
- [ ] **Step 5: Docs and bookkeeping.** Parent spec rev 43: `LogGrf` and `SvgGrf` live in `starlink-ast` (no dependencies, the crate's tests need them, crates.io users get SVG without another crate); `ast-plugins` keeps only what stage 7 decides. Plot spec: stage 2 done. `PLAN.md` §7 row. CHANGELOG `Added`: `starlink-ast: SvgGrf, a Grf backend that draws to SVG as the C library's grf_log does, with SvgOptions for the page size.`
- [ ] **Step 6: Full gates.** `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps && cargo test -p starlink-ast` → all pass.
- [ ] **Step 7: Commit** `Add SvgGrf, which draws to SVG as C's grf_log does`.

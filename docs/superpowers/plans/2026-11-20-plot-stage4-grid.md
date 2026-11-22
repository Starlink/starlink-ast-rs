# Plot Stage 4: `grid` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `Plot::grid` draws the annotated coordinate grid C's `astGrid` draws, call for call: the labels match C's reference SVGs (gate 1) and the whole `grf_log` call log matches a capture from the local C build (gate 2).

**Architecture:** `astGrid` reaches 78 functions of `plot.c` (about 16k lines with comments): a curve tracer, a border tracer, a text layer that interprets escape sequences, tick placement, the choice between exterior and interior labelling, and the drawing of ticks, axes and labels.
The port builds them bottom up, each layer checked against C through the thinnest public method that reaches it: `grid_line` for the curve tracer, `border` for the border tracer, `text` for the text layer, the values an invisible-ink grid records for tick placement and labelling, and then `grid` itself, first without labels and then whole.
Each drawing call takes `&mut dyn Grf`; the state C keeps in thread-static globals while drawing lives in a per-call drawing context.

**Tech Stack:** Rust 2024, `starlink-ast`; C probes against the reference `libast` built with `-ffp-contract=off`, compiling `src/grf_log.c` in and linking `libast_err` whole, as `tools/oracle_fixtures/grf_log_probe.c`'s header shows; the reference build's own `build/ast_tester/testgrid` for gate 2.

**Spec:** [`docs/superpowers/specs/2026-10-30-plot-design.md`](../specs/2026-10-30-plot-design.md), stage 4, gates 1 and 2; "Plot's extension points", "Single precision", "The trait follows C's granularity".

## Global Constraints

- Branch `u/timj/plot-4-grid`, stacked on `u/timj/plot-3-object`; one PR.
- Port C properly: every function in the call graph of `Grid` (`plot.c:18258`) and of `GridLine`, `Border` and `Text` is ported as C has it, at C's granularity, with C's file and line in a `//` comment. No tolerance, iteration count or threshold that C does not have (CLAUDE.md "Stop signals").
- Single precision where C has it: every value passed to or returned by `Grf` is `f32`, rounded from `f64` where C rounds (`(float)` casts), and text boxes are used in `f32` as C uses them.
- The drawing seam (spec, "Plot's extension points") is a public trait `starlink_ast::plot::Draw`, implemented by `Plot`; this stage gives it `grid`, `grid_line`, `border` and `text`, and stage 6 adds the rest. Where C calls one of these through the vtab from inside another (`Grid` calls `astBorder`), the Rust calls the trait method.
- A Frame method C calls that the port lacks, or has with a different contract, is ported or fixed in the Frame module with its own C probe (gate 6), not worked around in `plot/`.
- A C quirk that the port mirrors gets a `docs/issues/c-library-quirks.md` entry; a C bug found gets a `docs/issues/c-library-quirks.md` entry and is reported to the user, not fixed in the submodule without asking.
- `#[doc(alias)]` and `docs/audits/c-docs/ledger.tsv` rows moved from `not-ported` for `astGrid`, `astGridLine`, `astBorder` and `astText`; CHANGELOG `Unreleased` → `Added` per public item.
- Oracle data lives in generated `crates/starlink-ast/tests/<name>_oracle/cases.rs`, never under `tests/fixtures/` (the fixture-mirror gate diffs that tree against C's). FITS headers are read at run time from `tests/fixtures/wcsconv/inputs/`, which `DIST_MANIFEST` ships.
- No skip modes; an excluded case is in a named list with a reason and a companion test that fails when it starts matching.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps` clean; strip conda (`env -u CC -u LDFLAGS -u CFLAGS`).
- Per-task test: `cargo test -p starlink-ast --test frames test_plot` (covers `test_plot`, `test_plot_draw` and `test_grid`); full suite in Task 7.

## Decisions

- **Scope moved from stage 6.** `grid_line`, `border` and `text` land here: `Grid` calls `astBorder`, and `grid_line` and `text` are the thinnest public entries into the curve tracer and the text layer that `grid` shares, so each layer is compared with C before `grid` depends on it. Task 1 amends the spec's stage 4 and stage 6 lists.
- **Module layout** under `crates/starlink-ast/src/plot/`: `draw.rs` (the `Draw` trait, the drawing context, C's `G*` wrappers around `Grf`), `curve.rs` (`Crv`, `CrvLine`, `Map1`/`Map2`/`Map5`, `AxPlot`, the polyline buffer `Opoly`/`Fpoly`/`Apoly`/`Bpoly`, `grid_line`), `sample.rs` (`MakeGrid`, `GraphGrid`, `GoodGrid`, shared by border and ticks), `border.rs` (`Border`, `TraceBorder`, `Boundary`, `DrawRegion`, `BoxCheck`, `LinePlot`), `text.rs` (`DrawText`, `InterpEscape`, `HasEscapes`, `PushGat`/`PopGat`, `JustMB`, `RightVector`, `ChrLen`, `UpdateConcat`, `text`), `ticks.rs` (`GridLines`, `TickMarks`, `GetTicks`, `FindMajTicks`, `FindMajTicks2`, `DefGap`, `Typical`, `CountGood`, `Norm1`, `TickInfo`), `labels.rs` (`EdgeLabels`, `EdgeCrossings`, `Labelat`, `swapEdges`, `CheckLabels`, `CheckLabels2`, `Overlap`, `PlotLabels`, `FindDPTZ`, `SplitValue`, `Labels`, `TextLabels`, `Inside`, `Cross`), `grid.rs` (`Grid`, `DrawGrid`, `DrawAxis`, `DrawTicks`, `Ticker`, `SaveTick`, the used values).
- **C's drawing globals → owned values.** Written as a paragraph in `draw.rs`'s module doc in Task 1 (CLAUDE.md H4): the values `RESET_GRF` resets (`Grf_chh`, `Grf_chv`, `Grf_alpha`, `Grf_beta`), the polyline buffer, `Crv`'s statics and `Map1`–`Map5`'s parameters, the escape-sequence attribute stack, and `Box_lbnd`/`Box_ubnd` live in a `Drawing` context created at the start of each public drawing call and dropped at its end; `Boxp_lbnd`/`Boxp_ubnd` and `Boxp_freeze`, which outlive a call for `BoundingBox`, are private `Plot` fields (not dumped, as C does not dump them).
- **Signatures** (on `Draw`, all `&mut self`):
  - `fn grid(&mut self, grf: &mut dyn Grf) -> AstResult<()>`
  - `fn grid_line(&mut self, grf: &mut dyn Grf, axis: usize, start: &[f64], length: f64) -> AstResult<()>` (1-based `axis`)
  - `fn border(&mut self, grf: &mut dyn Grf) -> AstResult<bool>` (C's int result: whether the border was drawn round invalid regions)
  - `fn text(&mut self, grf: &mut dyn Grf, text: &str, pos: &[f64], up: [f32; 2], just: Justification) -> AstResult<()>`
- **Used values.** C's string getter answers `Gap(1)` and the other 13 drawn attributes (`GetUsedGap` …, `plot.c:1225`) with the set value, or else by running `astGrid` with the protected `Ink` set to 0 and reading what it recorded. The port gives `Plot::used_values(&mut self, grf: &mut dyn Grf) -> AstResult<UsedValues>`, which runs one invisible grid and returns, per field, what C's getter returns. `UsedValues` (`#[non_exhaustive]`, public fields) holds per axis `[T; 2]`: `gap`, `log_gap`, `centre`, `edge`, `label_at`, `label_units`, `log_ticks`, `log_label`, `min_tick`, `text_lab`, `maj_tick_len`; and `grid`, `labelling`, `border`. `Ink` is a private `Plot` field, not public and not dumped, as in C.
- **Oracle probes.** `plot_curve_probe.c`, `plot_border_probe.c`, `plot_text_probe.c` and `plot_grid_values_probe.c` under `tools/oracle_fixtures/`, each writing `crates/starlink-ast/tests/<probe>_oracle/cases.rs` with each case's `grf_log` call log and SVG; `capture_grid_logs.sh` runs the reference `testgrid` over `cases.txt` for gate 2. Probes build each case's Plot as `testgrid.c` does; its viewport and aspect correction are one shared C function in the probes and one shared Rust test helper.
- **Test helpers.** The string-to-typed-setter helpers in `tests/test_plot.rs` (`set_attr`, `apply_settings` and the style ones) move to `tests/common/plot_attrs.rs` so `test_grid.rs` uses them; they gain whatever `cases.txt` needs (`labelling=int`, `edge(2)=r`, `format(1)=gd`, `width(axes)=3`, `style(grid1)=2`, `size(textlab2)=2`, `bottom(1)=0`, `system=freq`), Frame attributes going to the Plot's Frame setters.

## Open question for the user (before Task 4)

PLAN.md's plot-support row records that TimeFrame `centre` with a date `Format` reports `AST__INTER` whenever the dates a gap either side fall in different months or years, where C reads an uninitialized MJD (`c-library-quirks.md`, the ERFA bad-month entry), and that stage 4 must settle this before `grid` draws such an axis.
None of the 20 `cases.txt` cases meets it (`timeplot` spans a day in April 2015), so the gates do not force an answer.
Recommended: fix C upstream on a `u/timj` submodule branch so `Centre` treats a zeroed month or day as 1 (the first of its field), port the same, and add a probe case of a grid over a date axis spanning a month end.
The alternative is to leave the error, so `grid` on such an axis fails where C draws from an undefined value.
Task 4 Step 5 carries the recommended path; the user decides.

## Review Focus

1. **A backend that fails mid-drawing.** A `Grf` returning `Err` part-way through `grid` must give that error and leave the Plot's attributes as they were (`Edge`, `Clip`, `LogTicks` and `LogLabel` restored, as C's cleanup restores them). Pinned in Task 7 by `a_failing_backend_leaves_the_plot_unchanged`.
2. **Drawing twice.** A second `grid` on the same Plot, or a `grid` after `used_values`, must give the same log as the first: the drawing context and `RESET_GRF` state start afresh, and the bounding box is recomputed. Pinned in Task 7 by `drawing_twice_gives_the_same_log`.
3. **A Plot whose base or current Frame is not 2-D.** `grid` gives `AST__NAXIN` with C's message for a 3-D current Frame, and draws through C's `Fset2D` reduction where C does. Pinned in Task 5 by `grid_rejects_a_plot_that_is_not_two_dimensional`.
4. **A date axis that spans a month end.** See the open question; pinned in Task 4 Step 5.
5. **Platform drift in the call log.** The log prints coordinates to 3 decimals, so a last-bit libm difference off macOS can flip a printed digit. The gate is exact on macOS; on Linux CI a case that differs only so is relaxed per case under PLAN.md §6, never by a global tolerance. Checked in Task 7 Step 6.

---

### Task 1: The drawing seam, the Grf layer, the curve tracer and `grid_line`

**Files:**
- Create: `crates/starlink-ast/src/plot/draw.rs`, `crates/starlink-ast/src/plot/curve.rs`; modify `plot/mod.rs` (modules, `Draw` re-export, private `ink` and bounding-box fields)
- Create: `tools/oracle_fixtures/plot_curve_probe.c`, `crates/starlink-ast/tests/plot_curve_oracle/cases.rs` (generated)
- Create: `crates/starlink-ast/tests/test_plot_draw.rs`; register in `tests/frames/main.rs`
- Modify: `docs/superpowers/specs/2026-10-30-plot-design.md` (stages 4 and 6), `docs/audits/c-docs/ledger.tsv`, `CHANGELOG.md`

**Interfaces:**
- Produces: `pub trait Draw` with `grid_line` (and, from later tasks, `border`, `text`, `grid`); `impl Draw for Plot`.
- Produces (crate-private): `Drawing<'a>` holding `&'a mut dyn Grf` and the per-call state; its methods `g_line`, `g_mark`, `g_text`, `g_tx_ext`, `g_qch`, `g_scales`, `g_cap`, `g_attr`, `g_flush` (C's `GLine` … `GFlush`, including `Invisible` and `Ink` handling and the cached scales); the polyline buffer (`opoly`, `fpoly`, `apoly`, `bpoly`); `crv`, `crv_line`, `ax_plot`; `CurveData` (C's `AstPlotCurveData`) and the `Map1`/`Map2`/`Map5` curve parametrizations as a private trait or enum taking the parameters C passes through globals.

- [ ] **Step 1: Spec and module doc.** Amend the spec's stage 4 and 6 lists (Decisions, "Scope moved"). Write `draw.rs`'s module doc: the globals-to-owned-values paragraph, and the `Draw` seam's purpose.
- [ ] **Step 2: Probe.** `plot_curve_probe.c`: `astGridLine` on Plots of `astFrame(2)` with a plain plot box and with `LogPlot(1)=1`; of the `aitoff` and `polco` headers (a sky axis that wraps, a pole, invalid regions outside the projection); with `Clip` 0 and 1; `Tol` 0.01 and 0.001; lengths that leave the plot box and that cross a discontinuity; and with `Width(curves)`, `Colour(curves)` and `Style(curves)` set, so `GAttr` saves and restores. Each case records the call log and the SVG.
- [ ] **Step 3: Failing test** `grid_lines_match_the_c_library` (log and SVG byte-exact per case), plus `grid_line_rejects_an_invalid_axis` (axis 3 on a 2-D Plot gives C's `AST__AXIIN` error).
- [ ] **Step 4: Implement** the `G*` wrappers, the polyline buffer, `Crv`/`CrvLine`/`Map1`/`AxPlot` and `GridLine` (`plot.c:18636`).
- [ ] **Step 5: Pass; ledger row and `#[doc(alias = "astGridLine")]`; CHANGELOG; commit** `Add Plot::grid_line and the curve tracer it shares with grid`.

### Task 2: `border`

**Files:**
- Create: `plot/sample.rs`, `plot/border.rs`
- Create: `tools/oracle_fixtures/plot_border_probe.c`, `tests/plot_border_oracle/cases.rs`
- Test: `tests/test_plot_draw.rs`

**Interfaces:**
- Consumes: Task 1's `Drawing`, polyline buffer, `crv`.
- Produces: `Draw::border`; crate-private `make_grid`, `graph_grid`, `good_grid` (Task 4 uses them).

- [ ] **Step 1: Probe.** `astBorder` on each of the 20 `cases.txt` Plots, built as `testgrid.c` builds them and with their attributes applied, plus `astFrame(2)` and `LogPlot` Plots; each records the return value, the call log and the SVG.
- [ ] **Step 2: Failing test** `borders_match_the_c_library`.
- [ ] **Step 3: Implement** `Border` (`plot.c:5062`), `TraceBorder`, `Boundary`, `DrawRegion`, `BoxCheck`, `LinePlot`, `Map2`, `Map5`, and the sampling grids.
- [ ] **Step 4: Pass; ledger, alias `astBorder`; CHANGELOG; commit** `Add Plot::border`.

### Task 3: The text layer and `text`

**Files:**
- Create: `plot/text.rs`
- Create: `tools/oracle_fixtures/plot_text_probe.c`, `tests/plot_text_oracle/cases.rs`
- Test: `tests/test_plot_draw.rs`

**Interfaces:**
- Produces: `Draw::text`; crate-private `draw_text` (C's `DrawText`, with its `ink` flag and returned box), `has_escapes`, `interp_escape`, `just_mb`, `right_vector`, `chr_len`, used by Tasks 5–7.

- [ ] **Step 1: Probe.** `astText` on `astFrame(2)` and a sky Plot, and on a Plot with a reversed graphics box: every justification (`B`, `C`, `T`, `M` × `L`, `C`, `R`); up-vectors `(0,1)`, `(1,0)`, `(1,1)`, `(-1,0)`; strings with each escape sequence C defines (`%%`, `%^`, `%v`, `%>`, `%<`, `%s`, `%w`, `%f`, `%c`, `%t`, `%h`, `%g`, `%-`, `%+`, and an unknown one); `Escape` 0 and 1; `Size(strings)`, `Font(strings)`, `Colour(strings)` set. Each records the log and the SVG. (`grf_log` reports no escape or `M` capability, so Plot does both itself, which is what this probe covers.)
- [ ] **Step 2: Failing test** `text_matches_the_c_library`.
- [ ] **Step 3: Implement** `Text` (`plot.c:26313`) and the text layer.
- [ ] **Step 4: Pass; ledger, alias `astText`; CHANGELOG; commit** `Add Plot::text, with C's escape sequences`.

### Task 4: Tick placement

**Files:**
- Create: `plot/ticks.rs`
- Create: `tools/oracle_fixtures/plot_grid_values_probe.c`, `tests/plot_grid_values_oracle/cases.rs`
- Test: a `#[cfg(test)]` module in `plot/ticks.rs` that reads the generated cases through `#[path]`, since tick placement is not public until Task 5

**Interfaces:**
- Consumes: Task 2's sampling grids.
- Produces: crate-private `grid_lines(&mut Plot, drawing, cen: &mut [f64; 2], gap: &mut [f64; 2], inval: &mut bool) -> AstResult<[TickInfo; 2]>` (C's `GridLines`), `TickInfo` with C's fields (`ticks`, `minticks`, `nmajor`, `nminor`, `start`, `length`, `labels`, `fmt`, …).

- [ ] **Step 1: Probe.** For each `cases.txt` case and these variants: `LogPlot(1)=1` with and without `LogTicks`, `LogGap`, `Gap(1)` and `Centre(2)` set, `MinTick(1)=5`, `Labelling=interior`, and the `timeplot` header with its date format: every used value through the string getter (`astGetC(plot, "Gap(1)")` …, `%.*g` as C prints them), and the call log of one invisible grid, captured around `astGetC(plot, "LabelAt(1)")` (no case sets `LabelAt`, so that getter always runs the grid).
- [ ] **Step 2: Failing test** `tick_placement_matches_the_c_library`: `grid_lines` on each case gives C's used `Gap`, `LogGap`, `Centre`, `MinTick` and `LogTicks`.
- [ ] **Step 3: Implement** `GridLines` (`plot.c:18758`), `TickMarks`, `GetTicks`, `FindMajTicks`, `FindMajTicks2`, `DefGap`, `Typical`, `CountGood`, `Norm1`, `CleanGrid`.
- [ ] **Step 4: Pass; commit** `Port C's tick placement for Plot grids`.
- [ ] **Step 5: Date axis across a month end** (per the user's answer to the open question). With the recommended answer: the upstream C fix with a `History:` entry, the matching TimeFrame `centre` change with its `plot_frame_methods_probe.c` case updated, a probe case of an invisible grid over a date axis spanning 31 March to 1 April, the quirks entry rewritten, PLAN.md's row closed; commit `Centre a date gap that spans a month end as fixed C does`.

### Task 5: Labelling decisions and `used_values`

**Files:**
- Create: `plot/labels.rs` (decision half), `plot/grid.rs` (`Grid` with ink off, `DrawGrid`, `UsedValues`)
- Create: `tests/test_grid.rs`; register in `tests/frames/main.rs`; move the attribute helpers to `tests/common/plot_attrs.rs`

**Interfaces:**
- Consumes: Tasks 1–4.
- Produces: `Plot::used_values`, `UsedValues`; crate-private `grid_impl(&mut self, drawing, ink: bool) -> AstResult<Recorded>` holding C's `Grid` body, where `Recorded` is what C's `SetUsed*` calls store; `edge_labels`, `labelat`, `swap_edges`, `plot_labels` (both the measuring and the drawing path, `ink` as in C).

- [ ] **Step 1: Failing test** `used_values_match_the_c_library` over Task 4's cases: every `UsedValues` field equals C's getter, and the `LogGrf` log of the invisible run equals C's; plus `grid_rejects_a_plot_that_is_not_two_dimensional` (Review Focus 3).
- [ ] **Step 2: Implement** `EdgeLabels` (`plot.c:10897`), `EdgeCrossings`, `Labelat`, `swapEdges`, `CheckLabels`, `CheckLabels2`, `Overlap`, `PlotLabels`, `FindDPTZ`, `SplitValue`, `Inside`, `Cross`, `DrawGrid`, and `Grid` up to the `if( ink )` block, with the attribute set-and-restore and the `SetUsed*` recording after it.
- [ ] **Step 3: Pass; CHANGELOG; commit** `Add Plot::used_values, the values an invisible grid chooses`.

### Task 6: Ticks and axes drawn

**Files:**
- Modify: `plot/grid.rs`
- Create: `tools/oracle_fixtures/capture_grid_logs.sh`, `tests/grid_oracle/cases.rs` (generated)
- Test: `tests/test_grid.rs`

**Interfaces:**
- Consumes: Task 5's `grid_impl`.
- Produces: `Draw::grid` (labels still to come in Task 7); `draw_ticks`, `draw_axis`, `ticker`, `save_tick`.

- [ ] **Step 1: Capture script.** `capture_grid_logs.sh` runs `reference/starlink-ast/build/ast_tester/testgrid` over `cases.txt` twice per case: as listed, and with `,NumLab=0,TextLab=0,DrawTitle=0` appended to its attributes (lines only). It fails unless the reference `libast` has no `fmadd` (`otool -tv` on macOS, `objdump -d` elsewhere), and writes both logs per case into `cases.rs` as raw string constants, with the C commit it was built from.
- [ ] **Step 2: Failing test** `grid_lines_only_match_the_c_library`: the lines-only log of every case, byte-exact, with `GRID_LOG_EXCLUDED` (empty) and its companion test.
- [ ] **Step 3: Implement** `DrawTicks` (`plot.c:10148`), `Ticker`, `SaveTick`, `DrawAxis` (`plot.c:9231`), the `border` call, and the `if( ink )` block without `Labels` and `TextLabels`.
- [ ] **Step 4: Pass; commit** `Draw Plot grids' ticks, axes and border`.

### Task 7: Labels, and gates 1 and 2

**Files:**
- Modify: `plot/labels.rs` (`Labels`, `TextLabels`), `plot/grid.rs`
- Test: `tests/test_grid.rs`
- Modify: `PLAN.md` (§7 Plot row, the plot-support row, the `testgrid.c` row of the test tracker), the spec (stage 4 done), `CHANGELOG.md`, `ledger.tsv` (`astGrid`)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Failing tests.** `grid_logs_match_the_c_library` (gate 2: the full log of every case, byte-exact; `GRID_LOG_EXCLUDED` stays empty unless a case is shown to differ for a reason recorded beside it). `grid_labels_match_the_c_references` (gate 1, the port of `testgrid.c`: `SvgGrf` output's `<text ` lines, sorted, equal those of `tests/fixtures/plot/expected/<case>.svg`, as `cmake/run_grid_test.cmake` compares them; `cobe` in `GRID_LABELS_EXCLUDED` with C's reason, its companion test failing if `cobe` starts matching, and if it matches already the exclusion is dropped and a ruling recorded). `a_failing_backend_leaves_the_plot_unchanged` and `drawing_twice_gives_the_same_log` (Review Focus 1 and 2).
- [ ] **Step 2: Implement** `Labels` (`plot.c:20871`), `TextLabels` (`plot.c:26541`), `GVec`, and the bounding box `Grid` stores.
- [ ] **Step 3: Pass**; then the full suite, clippy, fmt and docs.
- [ ] **Step 4: Records.** Ledger row and alias `astGrid`; CHANGELOG; PLAN.md rows; the spec's stage 4 marked done with this plan's path; quirks entries for any C quirk mirrored.
- [ ] **Step 5: Commit** `Add Plot::grid, matching C's labels and call log`.
- [ ] **Step 6: Off-macOS check.** Ask the user to run `cargo test -p starlink-ast --test frames test_grid` on Linux (they run the Linux checks); a case that differs only in a printed last digit gets a per-case relaxation counted in PLAN.md §6.

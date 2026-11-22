# Plot Stage 3: Plot as an Object — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `Plot` that can be built, configured and saved exactly as C's: construction from a Frame or FrameSet with a graphics box and a plot box, every Plot attribute with C's defaults, and Native save and load byte-exact with C (gate 4). No drawing yet.

**Architecture:** C's Plot is a FrameSet subclass whose base Frame is a 2-D `GRAPHICS` Frame joined to the supplied FrameSet's base Frame by a WinMap from the plot box to the graphics box.
The Rust `Plot` holds a `FrameSet` by composition (spec, "Plot is a FrameSet") and implements `Mapping` and `Frame` by forwarding to it, with two exceptions that are C's own overrides: `transform` normalises forward results in the current Frame and then clips them to the stored clipping region (`plot.c` `Transform` → `Trans`, :28513, :28181), and removing a Frame keeps the clipping-Frame index in step (`RemoveFrame`, :24637).
Plot's own attributes are fields of the `Plot`, unset or set as in C, with getters giving C's defaults; setting `LogPlot` rewrites the graphics-to-base Mapping as C's does (`ToggleLogLin`, :29493).

**Tech Stack:** Rust 2024, `starlink-ast`; C probes against the reference `libast` (`-ffp-contract=off`), linking `libast_err` whole, as `tools/oracle_fixtures/grf_log_probe.c`'s header shows.

**Spec:** [`docs/superpowers/specs/2026-10-30-plot-design.md`](../specs/2026-10-30-plot-design.md), stage 3 and gate 4; "Plot is a FrameSet", "Plot's extension points".

## Global Constraints

- Branch `u/timj/plot-3-object`, stacked on `u/timj/plot-2-grf`; one PR (the spec allows splitting a stage; this plan keeps one).
- Before writing Rust for Plot, classify every `astInitPlotVtab` entry (CLAUDE.md "Before porting a class") in the `plot` module doc: Plot's own drawing methods (`Border`, `BoundingBox`, `Clip`, `Curve`, `GenCurve`, `Grid`, `GridLine`, `Mark`, `PolyCurve`, `RegionOutline`, `SetTickValues`, `Text`, `CopyPlotDefaults`, `Mirror`, `GetDrawnTicks`, `CvBrk`) are architecture for later stages; the `Grf*` registration methods are C-ABI only (spec non-goal); the per-attribute Get/Set/Test/Clear and the string `GetAttrib`/`SetAttrib`/`TestAttrib`/`ClearAttrib` are ceremony; `Transform` and `RemoveFrame` are architecture ported here.
- No drawing methods in this stage; their seam (the Plot3D trait of the spec) is introduced in stage 4 with `grid`.
- Native output byte-exact with C, including C's quirks in `Dump` (notably `Gap` and `LogGap` written with the `set` flag left over from the preceding `Cen` loop, `plot.c` Dump); every quirk mirrored gets a `c-library-quirks.md` entry.
- 1-based axes in the Rust API; C's protected attribute functions take 0-based axes.
- A C attribute whose value C derives while drawing (the "used" values Plot records during `astGrid`, and the tick values `SetTickValues` stores) is stored and dumped here only as loaded; computing it is stage 4. Say so in the getter's doc where C's getter returns a drawn value.
- `#[doc(alias)]` on each public item mapped from a documented C header, with its `docs/audits/c-docs/ledger.tsv` row moved from `not-ported` to the Rust item (the 36 Plot attribute rows and `astPlot`); `Grf` keeps `not-ported` with the reason "C-ABI graphics registration".
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps` clean; CHANGELOG `Unreleased` → `Added` entries; strip conda env vars.
- Per-task test: `cargo test -p starlink-ast --test frames test_plot::`; full suite once in Task 4.

## Decisions

- **Module:** `crates/starlink-ast/src/plot/` — `mod.rs` (`Plot`, construction, forwarding, vtab classification), `attrs.rs` (scalar and per-axis attributes), `elements.rs` (`PlotElement` and the style attributes), `serial.rs` (Native). Public as `starlink_ast::plot`.
- **Construction:** `Plot::new(frame: &dyn Frame, graphbox: [f32; 4], basebox: [f64; 4]) -> AstResult<Plot>` takes any Frame, FrameSet, Region or Plot, as `astPlot` does (C's `astIsAPlot`/`astIsAFrameSet` dispatch becomes a downcast through `as_any`), and `Plot::default_frame(graphbox, basebox)` for C's NULL frame. Port `Fset2D` (a non-2-D base gets a new 2-D base from its first two axes) as C does it.
- **Forwarding:** `Mapping` and `Frame` forward to the held `FrameSet` (which already forwards to its current Frame with integrity), except `transform`/`transform_into`, which run C's `Trans`. `Plot::frameset()` and `frameset_mut()` give the FrameSet; FrameSet operations C reaches through a Plot (`add_frame`, `remove_frame`, `set_base`, `set_current`, …) are inherent methods on `Plot` that call the FrameSet's and then do what Plot's override does (only `remove_frame` adds anything).
- **Attributes:** typed accessors per C attribute, `snake_case` (`set_tol`, `tol`, `test_tol`, `clear_tol`; per-axis take `axis: usize`). Enumerated values are Rust enums: `Labelling { Exterior, Interior }`, `TextGapType { Box, Plot }`, `Edge { Left, Top, Right, Bottom }`, `ClipOp { And, Or }` where C uses an int, with C's strings for Native.
- **Elements:** `PlotElement` enum of C's 20 element ids plus its 5 group ids (`Axes`, `NumLabs`, `TextLabs`, `GridLines`, `Ticks`); setting a group sets each member, getting a group gets C's representative member; style accessors `set_colour(element, i32)`, `colour(element)`, and likewise `style`, `font`, `width` (`f64`), `size` (`f64`), with test and clear.
- **Native:** `Plot` implements the private `Serializable` (`to_node`/`from_node`), writing the FrameSet part through the FrameSet's own and then Plot's cards in `Dump`'s order; an `AnyObject::Plot` variant and the Native read registry learn `Begin Plot`.
- **Oracle:** one probe per task area under `tools/oracle_fixtures/` (`plot_construct_probe.c`, `plot_attrs_probe.c`, `plot_native_probe.c`), each emitting `crates/starlink-ast/tests/plot_*_oracle/cases.rs`; Frames are built from shared ids as in stage 1's probe, and the FITS-header cases read `ast_tester/fixtures/plot/` headers already mirrored under `crates/starlink-ast/tests/fixtures/` (copy any that are not, per CLAUDE.md "Porting a test").

## Review Focus

1. **A reversed graphics box** (`graphbox` with x1 > x2 or y1 > y2, as screens with y down have): `Xrev`/`Yrev`, the graphics Frame's `Direction`, and the WinMap must all match C. Pinned in Task 1.
2. **A non-2-D or permuted base Frame**, a FrameSet whose base is a CmpFrame, and a Plot built from a Plot (existing `GRAPHICS` domains cleared). Pinned in Task 1.
3. **Unset attributes in a dump**: C writes many as commented `#` lines with their default, some only when the default is not bad, some with stale `set` flags; byte-exactness covers each. Pinned in Task 4.
4. **`LogPlot` on an axis whose plot-box range spans zero or is negative**, where a logarithmic mapping is impossible: the Mapping and the `LogPlot` value reported afterwards must be whatever `ToggleLogLin` gives. Pinned in Task 2.
5. **Clipping after a Frame is removed**: removing the clipping Frame turns clipping off; removing an earlier Frame shifts the index. Pinned in Tasks 1 and 4 (clip state comes from a loaded dump until `clip` lands in stage 6).

---

### Task 1: The object, its construction, and C's Mapping overrides

**Files:**
- Create: `crates/starlink-ast/src/plot/mod.rs`; modify `crates/starlink-ast/src/lib.rs` (`pub mod plot;`)
- Create: `tools/oracle_fixtures/plot_construct_probe.c`, `crates/starlink-ast/tests/plot_construct_oracle/cases.rs` (generated)
- Create: `crates/starlink-ast/tests/test_plot.rs`; register in `crates/starlink-ast/tests/frames/main.rs`

**Interfaces:**
- Produces `starlink_ast::plot::Plot` (`Clone`, `Debug`), with `Plot::new`, `Plot::default_frame`, `frameset()`, `frameset_mut()`, `graphics_box() -> [f64; 4]` (C's `xlo, ylo, xhi, yhi`), `reversed() -> [bool; 2]`, `plot_box() -> [f64; 4]` (C's `bbox`), `remove_frame(FrameRef)`, and `impl Mapping`, `impl Frame`.
- Private clip state (`clip_frame: Option<usize>`, `clip_lbnd`, `clip_ubnd`, `clip_axes`) used by `transform`, set only by Native load in this stage.

- [ ] **Step 1:** Read `astInitPlotVtab` (`plot.c:19663`) and write the classification into the `plot` module doc (`//` comments for C names).
- [ ] **Step 2: Probe.** `plot_construct_probe.c` builds Plots from: NULL, `astFrame(2)`, `astSkyFrame`, a 3-D `astFrame` (Fset2D), a FrameSet read from `ast_tester/fixtures/plot/` header `testgrid1` (or the first `cases.txt` header), a Plot of a Plot, and graphics boxes normal, x-reversed and y-reversed. For each: `Nframe`, `Base`, `Current`, each Frame's `Domain`, the four graphics bounds and `Xrev`/`Yrev`, and `astTran2` of 5 graphics points forward and back.
- [ ] **Step 3: Failing test** `plot_construction_matches_the_c_library` over those cases, plus `plot_rejects_a_bad_box`: a zero-size or bad graphics box and a bad plot box each give `ErrorCode::Badbx`, as `astInitPlot_` reports.
- [ ] **Step 4:** Implement construction (`astInitPlot_`, :30676; `Fset2D`) and forwarding; `transform` through a port of `Trans` (normalisation, then the clipping branch, which Task 4's loaded dumps exercise).
- [ ] **Step 5: Pass; commit** `Add Plot, built from a Frame or FrameSet as C's astPlot builds it`.

### Task 2: Plot's scalar and per-axis attributes

**Files:**
- Create: `crates/starlink-ast/src/plot/attrs.rs`
- Create: `tools/oracle_fixtures/plot_attrs_probe.c`, `crates/starlink-ast/tests/plot_attrs_oracle/cases.rs`
- Modify: `crates/starlink-ast/tests/test_plot.rs`, `docs/audits/c-docs/ledger.tsv`, `CHANGELOG.md`

**Interfaces:**
- The attributes in `Dump`'s order: `Tol`, `Grid`, `TickAll`, `ForceExterior`, `Invisible`, `Border`, `ClipOp`, `Clip`, `DrawTitle`, `DrawAxes(axis)`, `Abbrev(axis)`, `Escape`, `LabelAt(axis)`, `Centre(axis)`, `Gap(axis)`, `LogGap(axis)`, `NumLabGap(axis)`, `TextLabGap(axis)`, `LabelUp(axis)`, `LogPlot(axis)`, `LogTicks(axis)`, `LogLabel(axis)`, `NumLab(axis)`, `MinTick(axis)`, `TextLab(axis)`, `LabelUnits(axis)`, `TitleGap`, `MajTickLen(axis)`, `MinTickLen(axis)`, `Labelling`, `TextGapType`, `Edge(axis)`, and `Ink`; each with get (C's default when unset, from the attribute's `Get…` in `plot.c`), set (with C's validation and clamping), test, clear.
- `set_log_plot`/`clear_log_plot` rewrite the Mapping through a port of `ToggleLogLin`.

- [ ] **Step 1: Probe.** For each attribute, on Plots of `astFrame(2)`, a SkyFrame and the FITS header case: the getter unset, then after setting a valid value, an out-of-range value (C's clamping or error), and clearing. For `LogPlot`: plot boxes all positive, spanning zero, and negative; after setting, `astTran2` of graphics points and the getter.
- [ ] **Step 2: Failing tests**, one per attribute family, driven by the cases; `log_plot_cannot_map_a_box_spanning_zero` names Review Focus 4.
- [ ] **Step 3: Implement**; ledger rows and `#[doc(alias)]` for each documented attribute.
- [ ] **Step 4: Pass; CHANGELOG; commit** `Add Plot's attributes, with C's defaults and LogPlot's remapping`.

### Task 3: The per-element style attributes

**Files:**
- Create: `crates/starlink-ast/src/plot/elements.rs`
- Modify: the attrs probe (or a new section of it), `test_plot.rs`, ledger, CHANGELOG

**Interfaces:**
- `pub enum PlotElement` (C's ids 0..19 and group ids 20..24, with `name()` giving C's element name for messages) and `Style`, `Font`, `Colour` (`i32`), `Width`, `Size` (`f64`) per element, with get, set, test, clear; a group sets every member, and getting or testing a group does what C's getter does for it (read `GetStyle` etc. and the element-name logic near `plot.c:28829`; the probe settles it).

- [ ] **Step 1: Probe** each style attribute on each element and each group: default, set, the members a group set changes, clear.
- [ ] **Step 2: Failing test; Step 3: implement; Step 4: pass; commit** `Add Plot's per-element style attributes`.

### Task 4: Native save and load

**Files:**
- Create: `crates/starlink-ast/src/plot/serial.rs`
- Modify: the `AnyObject` enum and Native read registry (find where `FrameSet` is registered), `test_plot.rs`, `docs/issues/c-library-quirks.md`, `PLAN.md`, the plot spec (stage 3 done), CHANGELOG
- Create: `tools/oracle_fixtures/plot_native_probe.c`, `crates/starlink-ast/tests/plot_native_oracle/cases.rs`

**Interfaces:**
- `to_node`/`from_node` for `Plot`, ports of `Dump` (:30020) and `astLoadPlot_` (:31209); clip state, tick values and the used values stored as loaded.

- [ ] **Step 1: Probe.** Dumps (`astToString`) of: each Task 1 construction; Plots with many attributes set, including per-axis and per-element; a Plot with a clipping region set by `astClip` (so clip state appears), then with the clipping Frame removed and with an earlier Frame removed; a Plot after `astGrid` through `grf_log` (so used values and tick values appear). Also the probe's `astTran2` of points through each clipped Plot.
- [ ] **Step 2: Failing tests:** `plot_dumps_match_the_c_library` (Rust-built Plots with the same settings dump byte-exactly), `plot_dumps_round_trip` (each C dump read and re-dumped byte-exactly), `clipped_transform_matches_the_c_library`.
- [ ] **Step 3: Implement**; log each mirrored `Dump` quirk in `c-library-quirks.md`.
- [ ] **Step 4: Full gates** (fmt, clippy, doc, `cargo test -p starlink-ast`); PLAN.md §7 Plot row and the plot spec mark stage 3 done.
- [ ] **Step 5: Commit** `Save and load Plots in C's Native form`.

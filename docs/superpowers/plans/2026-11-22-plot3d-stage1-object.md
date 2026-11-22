# Plot3D Stage 1: Plot3D as an Object — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `Plot3D` that can be built, configured and saved exactly as C's: construction from a Frame or FrameSet with a 3D graphics box and base box, the three inner Plots C builds, C's FrameSet overrides, every attribute with C's defaults and forwarding, the attribute-string layer, and Native save and load byte for byte with C. No drawing.

**Architecture:** C's Plot3D is a Plot whose own Plot part is a dummy 2D Plot with its FrameSet rebuilt around a 3D `GRAPHICS` Frame (`astInitPlot3D_`, `plot3d.c:7662`), plus three ordinary 2D Plots, one per face of the graphics cube, built from FrameSets that `SplitFrameSet` (`plot3d.c:5886`) cuts out of the supplied one.
The Rust `Plot3D` holds its Plot part and the three inner Plots by value (`Plot` is `Clone`), and implements `Frame` and `Mapping` by forwarding to the Plot part's FrameSet, as a generated file like `plot/forward.rs`; its `transform` is the FrameSet's, not Plot's normalising and clipping one (`plot3d.c:6801`).
Plot attributes set on a Plot3D are stored on its Plot part and forwarded to the inner Plots by C's table (`plot3d.c:7218`–`7270`); an unset per-axis attribute reads from the inner Plot that labels the axis.

**Tech Stack:** Rust 2024, `starlink-ast`; C probes against the reference `libast` built with `-ffp-contract=off`, linking `libast_err`, `libast_grf_log` and the stub `libast_grf3d` whole, as `tools/oracle_fixtures/plot_native_probe.c`'s header shows. The stub `grf3d` is enough: nothing in this stage draws.

**Spec:** [`docs/superpowers/specs/2026-11-22-plot3d-design.md`](../specs/2026-11-22-plot3d-design.md), "Plot3D" and stage 1; [`2026-10-30-plot-design.md`](../specs/2026-10-30-plot-design.md) for the Plot conventions this follows.

## Global Constraints

- Branch `u/timj/plot3d-1-object`, stacked on `u/timj/plot3d-design`; one PR.
- Before writing Rust, classify every `astInitPlot3DVtab` entry (`plot3d.c:3651`; CLAUDE.md "Before porting a class") in the `plot3d` module doc, C names in `//` comments: `Grid`, `Text`, `SetTickValues`, `Border`, `Mark` are architecture for stage 3; `PolyCurve`, `BoundingBox`, `GridLine`, `Curve`, `GenCurve`, `Clip` are architecture that report C's error, stage 3; `GrfSet`, `GrfPush`, `GrfPop`, `GetGrfContext`, `ManageLock` and the `Plot3D*` grf callbacks and `Attr3D` are C-ABI or graphics-routing, replaced in stage 3 by the private per-face `Grf` adapter; `Transform`, `SetCurrent`, `ClearCurrent`, `RemoveFrame` are architecture ported here; `Equal`, `Cast`, `GetObjSize`, `Clear`, `VSet` and the string `Get/Set/Test/ClearAttrib` are ceremony.
- No drawing and no `Draw` impl in this stage; no change to the `Draw` trait (stage 3).
- No `frameset_mut()` on `Plot3D` (spec, "Plot3D").
- Native output byte for byte with C, including quirks; each quirk mirrored gets a `docs/issues/c-library-quirks.md` entry in the same commit, probed (CLAUDE.md memory "Track C quirks").
- 1-based axes in the Rust API (`norm(1..=3)`, `gap(3)`); C's protected functions take 0-based axes.
- Error messages name the class as C does (`astSetGap(Plot3D): …`): the Plot attribute helpers that hard-code `(Plot)` (`plot/attrs.rs` `attr_axis`) take the class name from the caller.
- `#[doc(alias)]` on each public item mapped from a C header, with its `docs/audits/c-docs/ledger.tsv` row moved from `not-ported` (rows 408–411: `Plot3D`, `RootCorner`, `Norm`, `astPlot3D`).
- `cargo fmt`; `cargo clippy --workspace --all-targets -- -D warnings`; `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps --all-features`; `CHANGELOG.md` `Unreleased` → `Added`; strip conda variables from the environment for cargo and probes.
- Per-task tests: `cargo test -p starlink-ast --test frames test_plot3d::` (and `--features attribute-strings` for Task 5); full suite once in Task 6.

## Decisions

- **Module:** `crates/starlink-ast/src/plot3d/`: `mod.rs` (`Plot3D`, construction, the FrameSet overrides, vtab classification), `split.rs` (`Fset3D`, `SplitFrameSet`, `StoreAxisInfo`, `SetPlotAttr`, `ChangeRootCorner`, `UpdatePlots`), `attrs.rs` (`Norm`, `RootCorner`, the forwarded Plot attributes), `serial.rs`, `forward.rs` (generated). Public as `starlink_ast::plot3d`.
- **Construction:** `Plot3D::new(frame: &dyn Frame, graphbox: [f32; 6], basebox: [f64; 6]) -> AstResult<Plot3D>` and `Plot3D::default_frame(graphbox, basebox)` for C's NULL frame, mirroring `Plot::new` and `Plot::default_frame` (`plot/mod.rs:233`, `:268`). Boxes are C's order: lower x, y, z then upper x, y, z.
- **Plot part:** built as C builds it: `Plot::default_frame([0.,0.,1.,1.], [0.,0.,1.,1.])`, then the Frame removals, the 2→3 `PermMap` to a 3D `GRAPHICS` Frame (`Title=Graphical Coordinates`), the 3D `WinMap` from `basebox` to `graphbox`, and the supplied FrameSet added, through `pub(crate)` access to the Plot's FrameSet.
- **State:** `plot: Plot` (the Plot part), `plot_xy`, `plot_xz`, `plot_yz: Plot`, `pix_frame: usize`, `base_plot: i32`, `axis_plot: [[i32; 3]; 2]` and `axis_index: [[i32; 3]; 2]` (C's `axis_plot1/2`, `axis_index1/2`, kept as C's ints because Native writes them), `root_corner: Option<RootCorner>`, `norm: [Option<f64>; 3]`, `gbox: [f64; 6]`.
- **Types:** `RootCorner { x: Limit, y: Limit, z: Limit }` with `Limit::{Lower, Upper}`, parsed from and shown as C's three letters (`"LLL"`, `"ULU"`; `RootCornerInt`, `RootCornerString`); `Plane::{Xy, Xz, Yz}`.
- **FrameSet overrides:** inherent `set_current`, `clear_current`, `set_base`, `add_frame`, `remove_frame` on `Plot3D`, each calling the Plot part's and then doing what C's override does (`UpdatePlots` after `set_current`/`clear_current`; `remove_frame` refuses `pix_frame` with `ErrorCode::Pxfrrm` and shifts it).
- **Forwarding:** extend `tools/gen_plot_forward.py` to also write `crates/starlink-ast/src/plot3d/forward.rs`, forwarding to `self.plot.frameset()`, with `transform`/`transform_into` forwarding too (the FrameSet's, `plot3d.c:6801`) and `snapshot`, `equal`, `as_frameset` and the others in `OWN_FRAME`/`OWN_MAPPING` hand-written as Plot's are; the existing freshness test gains a Plot3D twin.
- **Attributes:** typed accessors with Plot's names, generated by a macro over C's table, each entry `(attribute, which plots)`, `whichplots` as C's `MAKE_ALL` gives it: −1 all three Plots, 0 both Plots touching the axis, 1 only the labelling Plot (`AxisPlot`, `plot3d.c:1508`); per-element styles go through `Element2D` (`plot3d.c:2649`); attributes C does not forward (`Edge`, `DrawTitle`, `TitleGap`, `LabelAt`, `Centre`, `TextGapType`) are set on the Plot part only. Per-axis getters: the Plot part's value if set, else the labelling Plot's (`plot3d.c:1014`). Each inner Plot is readable and writable through `plane(Plane) -> &Plot` and `plane_mut(Plane) -> &mut Plot`, the typed form of C's `_XY` suffix.
- **Native:** `Plot3D` implements the private `Serializable`; the Plot part writes through Plot's own, then Plot3D's cards in `Dump`'s order (`plot3d.c:7383`). Add `AnyObject::Plot3D`, the read registries (`serial/mod.rs:401`, `:723`, `:2649`), the `to_node` dispatch (`:523`, `:615`, `:2742`), the Frame and Mapping views (`:2202`, `:2293`, `:2346`, `:2432`, `:2478`), `object_text.rs`, and FitsChan's FrameSet-like match (`fitschan/mod.rs:3693`). `("Plot3D", "Plot")` already sits in `native_stream.rs:78`.
- **Oracle:** `tools/oracle_fixtures/plot3d_construct_probe.c`, `plot3d_attrs_probe.c`, `plot3d_native_probe.c`, each writing `crates/starlink-ast/tests/plot3d_*_oracle/cases.rs` in the style of `plot_native_oracle`; inputs are `tests/fixtures/programs/testplot3d/plot3d-test1.ast`, Frames built in the probe, and one spectral-cube FITS header (RA, Dec, frequency) copied into `crates/starlink-ast/tests/fixtures/plot3d/` if no mirrored one fits.
- **Tests:** `crates/starlink-ast/tests/test_plot3d.rs`, registered in `crates/starlink-ast/tests/frames/main.rs` beside `test_plot`, its header naming `ast_tester/testplot3d.c` as the ancestor of the checkdump test.

## Review Focus

1. **A FrameSet that cannot be split** into an independent pair of axes plus one, such as a 3D `MatrixMap` that mixes all three: C reports `AST__3DFSET`; the port must report `ErrorCode::ThreeDfset`, not build Plots from a wrong split. Pinned in Task 1.
2. **A cube whose independent pair is not axes 1 and 2** (spectral axis first, then RA and Dec): `BasePl`, the axis bookkeeping and which face labels which axis all change. Pinned in Task 1.
3. **Changing the current Frame**, to another sky system, to a Frame whose axes pair differently, and to the `GRAPHICS` Frame itself: `UpdatePlots` rebuilds each inner Plot's current Frame and re-applies the root corner. Pinned in Task 2.
4. **Removing Frames**: the original base Frame (`ErrorCode::Pxfrrm`), a Frame before it (`pix_frame` shifts), and the current Frame. Pinned in Task 2.
5. **`Norm` in a dump**: C writes it with its `set` flag always 0, so a set `Norm` is shown but never read back (`plot3d.c:7452`); the port mirrors this and round-trips byte for byte. Pinned in Task 4.

---

### Task 1: The object and its construction

**Files:**
- Create: `crates/starlink-ast/src/plot3d/{mod,split,forward}.rs`; modify `crates/starlink-ast/src/lib.rs` (`pub mod plot3d;`), `crates/starlink-ast/src/plot/mod.rs` (`pub(crate)` FrameSet access for construction), `tools/gen_plot_forward.py`
- Create: `tools/oracle_fixtures/plot3d_construct_probe.c`, `crates/starlink-ast/tests/plot3d_construct_oracle/cases.rs` (generated)
- Create: `crates/starlink-ast/tests/test_plot3d.rs`; modify `crates/starlink-ast/tests/frames/main.rs`

**Interfaces:**
- Produces `starlink_ast::plot3d::{Plot3D, Plane, RootCorner, Limit}`; `Plot3D` is `Clone + Debug` with `new`, `default_frame`, `plane(Plane) -> &Plot`, `plane_mut(Plane) -> &mut Plot`, `graphics_box() -> [f64; 6]`, `pix_frame() -> usize`, `impl Frame`, `impl Mapping`.
- `pub(crate)` in `split.rs`: `fset_3d(fs: FrameSet) -> AstResult<FrameSet>`; `split_frameset(fs: &FrameSet) -> AstResult<Split>`, `Split` holding the three 2D FrameSets, their `label` and `wcs` pairs and `base_plane`; `Plot3D::store_axis_info`, `set_plot_attr(plot: &mut Plot, plane: Plane, label: [i32; 2])`, `change_root_corner(old, new)`.

- [ ] **Step 1:** Write the vtab classification (Global Constraints) into the `plot3d` module doc.
- [ ] **Step 2: Probe.** `plot3d_construct_probe.c` builds Plot3Ds from: NULL; `astFrame(3)`; a 4-D `astFrame` (`Fset3D`); a 2-D Frame (expect `AST__NAXIN`); `plot3d-test1.ast` with `testplot3d.c`'s boxes (`gbox` −1..1, `bbox` 0.5..155.5, 0.5..107.5, 0.5..1640.5); the spectral cube; the cube with axes permuted to (frequency, RA, Dec); a 3D `MatrixMap` mixing all axes (expect `AST__3DFSET`); a Plot3D of a Plot3D; bad and zero-size boxes (expect `AST__BADBX`). For each it prints `Nframe`, `Base`, `Current`, each Frame's `Domain`, `PixFrm`, `BasePl`, the twelve axis bookkeeping ints, and for each inner Plot its graphics box, `Xrev`/`Yrev`, `Nframe`, current Frame `Domain` and `Naxes`, and the attributes `SetPlotAttr` and `ChangeRootCorner` set (`Edge(1)`, `Edge(2)`, `NumLab(1)`, `NumLab(2)`, `TextLab(1)`, `TextLab(2)`, `DrawAxes`, `Border`, and whatever else those two functions touch), plus `astTranN` of 5 graphics points forward and back through the Plot3D.
- [ ] **Step 3: Failing tests** `plot3d_construction_matches_the_c_library` over the cases; `plot3d_rejects_a_frame_that_is_not_3d` (`ErrorCode::Naxin`); `plot3d_rejects_a_frameset_with_no_independent_axes` (`ErrorCode::ThreeDfset`, Review Focus 1); `plot3d_rejects_a_bad_box` (`ErrorCode::Badbx`, each of the three C messages); `plot3d_transform_does_not_normalise` (a graphics point mapping to RA beyond 2π comes back as the FrameSet gives it, where a `Plot` would normalise it).
- [ ] **Step 4:** Implement `astInitPlot3D_`, `Fset3D`, `CreatePlots`, `SplitFrameSet`, `SetPlotAttr`, `StoreAxisInfo`, `ChangeRootCorner` (at `LLL`), each at C's granularity with C's call order (CLAUDE.md "Match C's construction shape"); regenerate `forward.rs`.
- [ ] **Step 5: Pass; commit** `Add Plot3D, built from a Frame or FrameSet as C's astPlot3D builds it`.

### Task 2: The FrameSet overrides

**Files:**
- Modify: `crates/starlink-ast/src/plot3d/{mod,split}.rs`, `tools/oracle_fixtures/plot3d_construct_probe.c`, its `cases.rs`, `crates/starlink-ast/tests/test_plot3d.rs`

**Interfaces:**
- Produces inherent `set_current(FrameRef)`, `clear_current()`, `set_base(FrameRef)`, `add_frame(FrameRef, Box<dyn Mapping>, DynFrame)`, `remove_frame(FrameRef)` on `Plot3D`, each `-> AstResult<()>`, with `Plot`'s signatures (`plot/mod.rs:395`–`433`).
- `update_plots()` in `split.rs`, a port of `UpdatePlots` (`plot3d.c:6885`), relying as C does on Frame 2 of each split FrameSet matching Frame 2 of the inner Plot.

- [ ] **Step 1: Probe cases** on the `plot3d-test1.ast` and spectral-cube Plot3Ds: after `astSetCurrent` to each Frame index, after `astClear(Current)`, after adding a Frame (a `SkyFrame` in Galactic through a `UnitMap`) and making it current, after removing a Frame before `PixFrm`, after removing the current Frame, and the error on removing `PixFrm`. Each prints the same state as Task 1.
- [ ] **Step 2: Failing tests** `plot3d_frameset_changes_match_the_c_library` (Review Focus 3, including `set_current(FrameRef::Index(1))`, the `GRAPHICS` Frame); `plot3d_refuses_to_remove_its_pixel_frame` (`ErrorCode::Pxfrrm`); `plot3d_pixel_frame_follows_a_removal` (Review Focus 4).
- [ ] **Step 3: Implement**, `update_plots` reading `root_corner` as C's `astGetRootCorner` does and re-applying it with `change_root_corner(rc, rc)`.
- [ ] **Step 4: Pass; commit** `Keep a Plot3D's inner Plots in step with its current Frame`.

### Task 3: Attributes

**Files:**
- Create: `crates/starlink-ast/src/plot3d/attrs.rs`; modify `crates/starlink-ast/src/plot/attrs.rs`, `crates/starlink-ast/src/plot/elements.rs` (class name in errors)
- Create: `tools/oracle_fixtures/plot3d_attrs_probe.c`, `crates/starlink-ast/tests/plot3d_attrs_oracle/cases.rs`
- Modify: `crates/starlink-ast/tests/test_plot3d.rs`, `docs/audits/c-docs/ledger.tsv`, `CHANGELOG.md`

**Interfaces:**
- `norm(axis) -> AstResult<f64>` (unset: 1.0, `plot3d.c:7204`), `set_norm`, `test_norm`, `clear_norm`; `root_corner() -> RootCorner` (unset: `LLL`), `set_root_corner(RootCorner)` (calls `change_root_corner(old, new)`, `plot3d.c:5777`), `test_root_corner`, `clear_root_corner` (`plot3d.c:2431`).
- Every Plot attribute accessor Plot has, by the same name and signature, on `Plot3D`, with per-axis ones accepting axis 3.

- [ ] **Step 1: Probe.** On the `plot3d-test1.ast` Plot3D and a default one: `Norm` and `RootCorner` unset, set (every one of the eight corners), cleared; each forwarded Plot attribute set on axes 1, 2 and 3 (or on its elements), then read back from the Plot3D and from each inner Plot through `astGet` on the inner Plots obtained with C's internal pointers (`astMakePointer`, `docs/issues/c-oracle-probe-recipe.md`), and cleared; each non-forwarded attribute set and shown absent from the inner Plots; an axis 4 (`AST__AXIIN`). After each `RootCorner`, print every inner Plot's `Edge(1)`, `Edge(2)`, `NumLab`, `TextLab` and the other attributes `ChangeRootCorner` sets.
- [ ] **Step 2: Failing tests**, one per family: `plot3d_norm_and_root_corner_match_the_c_library`, `plot3d_forwards_axis_attributes_as_c_does`, `plot3d_forwards_scalar_attributes_as_c_does`, `plot3d_forwards_element_styles_as_c_does`, `plot3d_keeps_unforwarded_attributes_to_itself`, `plot3d_rejects_axis_4`.
- [ ] **Step 3: Implement**; ledger rows for `RootCorner` and `Norm`; `#[doc(alias)]`.
- [ ] **Step 4: Pass; CHANGELOG; commit** `Add Plot3D's attributes, forwarded to its inner Plots as C forwards them`.

### Task 4: Native save and load

**Files:**
- Create: `crates/starlink-ast/src/plot3d/serial.rs`; modify `crates/starlink-ast/src/serial/{mod,object_text}.rs`, `crates/starlink-ast/src/fitschan/mod.rs`
- Create: `tools/oracle_fixtures/plot3d_native_probe.c`, `crates/starlink-ast/tests/plot3d_native_oracle/cases.rs`
- Modify: `crates/starlink-ast/tests/test_plot3d.rs`, `docs/issues/c-library-quirks.md`, `PLAN.md`

**Interfaces:**
- `Plot3D` implements the private `Serializable`; `AnyObject::Plot3D(Box<Plot3D>)`; Channel and `from_str`/`to_string` read and write `Begin Plot3D`.

- [ ] **Step 1: Probe.** The dump (default Channel, `Comment` true, `Full` 0) of each Task 1 construction, of each Task 2 change, of Plot3Ds with attributes set by an `astSet` string (including `Norm(1)=0.5`, `RootCorner=ULU`, `Gap(3)=100`, `Colour(Axes)=3`), and `testplot3d.c` test 1's `astPlot3D(NULL, gbox, bbox, "minticklen=0")`; and the dump after reading each back.
- [ ] **Step 2: Failing tests** `plot3d_dumps_match_the_c_library`; `plot3d_reads_back_what_c_reads_back` (the second dump equals C's second dump, so `Norm` drops out as C's does; Review Focus 5); `plot3d_checkdump` (the port of `testplot3d.c` test 1: write, read, write again, equal).
- [ ] **Step 3: Implement**; mirror the `Norm` dump quirk with a `c-library-quirks.md` entry; any other mismatch C's dump forces gets its own entry.
- [ ] **Step 4:** Mark `testplot3d.c` test 1 ported in `PLAN.md`'s migration tracker (test 2 draws; it waits for stage 3).
- [ ] **Step 5: Pass; commit** `Save and load a Plot3D byte for byte with C`.

### Task 5: The attribute-string layer

**Files:**
- Create: `crates/starlink-ast/src/attributes/plot3d.rs`; modify `crates/starlink-ast/src/attributes/{mod,public}.rs`
- Modify: `tools/oracle_fixtures/plot3d_attrs_probe.c`, its `cases.rs`, `crates/starlink-ast/tests/test_plot3d.rs`, `docs/issues/c-library-quirks.md`

**Interfaces:**
- `Plot3D` implements `crate::attributes::Attributes` under the `attribute-strings` feature, as `Plot` does (`attributes/plot.rs`), parsing as C's `SetAttrib`, `GetAttrib`, `TestAttrib`, `ClearAttrib` (`plot3d.c:5552`, `:3156`, `:6470`, `:2291`): `Norm(axis)`, `RootCorner`, a plane suffix (`Grid_XY`, `Colour_YZ(Axes)`) passed to that inner Plot with the suffix removed, anything else to the Plot part.

- [ ] **Step 1: Probe cases** for `astSet`, `astGet`, `astTest` and `astClear` with each form above, every suffix spelling C accepts (`_XY`, `_YX`, `_XZ`, `_ZX`, `_YZ`, `_ZY`, any case), a suffix on a per-axis attribute (`Gap_XY(1)`), and an unusable `RootCorner` (`AST__ATTIN`). Record which inner Plot each suffix reaches.
- [ ] **Step 2: Failing tests** `plot3d_attribute_strings_match_the_c_library` and `plot3d_plane_suffixes_reach_the_plots_c_reaches`.
- [ ] **Step 3: Implement.** C sends `_XZ` to `plotyz` and `_YZ` to `plotxz` in all four string methods (`plot3d.c:2372`, `:3270`, `:5652`, `:6565`), against its own documentation; if the probe confirms it, mirror it, add a `c-library-quirks.md` entry, and tell the user before the PR so they can decide whether to fix it upstream.
- [ ] **Step 4:** A get of an unset attribute whose Plot default depends on a drawn grid (`Gap`, `Grid`, `Labelling`, `Edge` and the rest `attributes/plot.rs` computes through a grid) returns an error naming Plot3D drawing as not yet ported, never a 2D grid's value, until stage 3.
- [ ] **Step 5: Pass; commit** `Set and get a Plot3D's attributes as strings, with C's plane suffixes`.

### Task 6: Finish

**Files:**
- Modify: `PLAN.md` (the Plot3D row: stage 1 done), `CHANGELOG.md`, `docs/superpowers/specs/2026-11-22-plot3d-design.md` (stage 1 done, with any divergence found)

- [ ] **Step 1:** `cargo test -p starlink-ast --all-features` and `cargo test -p starlink-ast`: all pass.
- [ ] **Step 2:** `cargo fmt --check`, clippy and doc commands from Global Constraints: clean.
- [ ] **Step 3:** `./tools/oracle_fixtures/check_c_headers_fresh.sh` and the C documentation gates in `test_c_docs_audit.rs`: pass.
- [ ] **Step 4: Commit** `Record Plot3D stage 1 as done`.

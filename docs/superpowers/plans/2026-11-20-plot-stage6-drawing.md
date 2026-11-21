# Plot Stage 6: The Other Drawing Methods — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Plot draws geodesic curves, polycurves, curves given by a Mapping, markers and Region outlines, reports the bounding box of what it last drew, and clips to a box in any of its Frames, each checked call for call against C through `grf_log` (gate 5).

**Architecture:** `Curve`, `GenCurve`, `PolyCurve`, `Mark`, `BoundingBox` and `Clip` are the remaining members of Plot's extension seam (spec, "Plot's extension points"), so they become methods of the `Draw` trait beside `draw_grid`; `RegionOutline` is not overridden by Plot3D and becomes an inherent `Plot` method.
The curves reuse the stage 4 tracer: two new `CurveMap` variants port C's `Map3` (a geodesic between two positions) and `Map4` (a user Mapping from distance to position), and the tracer's entry point is split so `PolyCurve` can trace several segments into one polyline, as C does.
One new probe drives every method through C's `grf_log` from a list of operations that the Rust test replays.

**Tech Stack:** Rust 2024, `starlink-ast`; a C probe against the reference `libast` (`-ffp-contract=off`) and `src/grf_log.c`, built as `tools/oracle_fixtures/plot_curve_probe.c`'s header describes.

**Spec:** [`docs/superpowers/specs/2026-10-30-plot-design.md`](../specs/2026-10-30-plot-design.md), stage 6 and gate 5.

## Global Constraints

- Branch `u/timj/plot-6-drawing`, stacked on `u/timj/plot-5-regtrace`; one PR.
- Port C at C's granularity, in C's order of backend calls: the call log is compared byte for byte, so the order of `GAttr`, `GScales`, `GLine` and `GMark` calls is part of the contract (see Decisions, "Call order").
- Logs and SVGs compared byte for byte, as `test_plot_draw.rs` compares them; no skip modes, no tolerance.
- Oracle data in a generated `crates/starlink-ast/tests/plot_methods_oracle/cases.rs`; no files under `tests/fixtures/`.
- Every new public item gets its `#[doc(alias)]`, and its `docs/audits/c-docs/ledger.tsv` row changes from `not-ported` to the Rust item; docs written for the Rust, with no C file or line citations in rendered docs (CLAUDE.md, "Adding a new Mapping / Frame / Region", step 9).
- A user-visible change gets a `CHANGELOG.md` `Unreleased` entry in the same commit.
- Commit messages end with the `Co-Authored-By` line only (no session line).
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps` clean; strip conda (`env -u CC -u LDFLAGS -u CFLAGS`).
- Per-task test: `cargo test -p starlink-ast --test frames test_plot_draw::`; full suite (`cargo test -p starlink-ast`) at the end of Task 4.

## Decisions

- **Signatures**, on `Draw` (`crates/starlink-ast/src/plot/draw.rs`) with their `Plot` implementations; positions are a `PointSet` with one axis per current-Frame axis, as `Mapping::transform` takes them:
  - `fn draw_curve(&mut self, grf: &mut dyn Grf, start: &[f64], finish: &[f64]) -> AstResult<()>` — `astCurve`.
  - `fn draw_gen_curve(&mut self, grf: &mut dyn Grf, map: &dyn Mapping) -> AstResult<()>` — `astGenCurve`.
  - `fn draw_poly_curve(&mut self, grf: &mut dyn Grf, positions: &PointSet) -> AstResult<()>` — `astPolyCurve`.
  - `fn draw_mark(&mut self, grf: &mut dyn Grf, positions: &PointSet, marker: i32) -> AstResult<()>` — `astMark`.
  - `fn bounding_box(&self) -> AstResult<([f32; 2], [f32; 2])>` — `astBoundingBox`, `(lbnd, ubnd)`.
  - `fn set_clip_region(&mut self, frame: FrameRef, lbnd: &[f64], ubnd: &[f64]) -> AstResult<()>` and `fn clear_clip_region(&mut self) -> AstResult<()>` — `astClip` with a Frame and with `AST__NOFRAME`. `clip` is taken by the Clip attribute's getter, as `border` and `grid` were in stage 4.
  - Inherent on `Plot`: `pub fn draw_region_outline(&mut self, grf: &mut dyn Grf, region: &dyn Region) -> AstResult<()>` — `astRegionOutline`.
  - The `Draw` trait's module doc and trait doc say it holds the methods Plot3D overrides, which include `bounding_box` and the clip-region pair as well as the drawing methods.
- **Errors**, each with C's message text from `plot.c`:
  - Every method: `ErrorCode::Naxin` when the base Frame is not 2-D (`"<method>(Plot): Number of axes (N) in the base Frame of the supplied Plot is invalid - this number should be 2."`).
  - `draw_curve`: `ErrorCode::Ncpin` when `start` or `finish` has fewer values than the current Frame has axes, worded as `draw_grid_line`'s.
  - `draw_poly_curve`: `ErrorCode::Naxin` when the PointSet's axis count is not the current Frame's (`plot.c:24126`'s message). C's `indim < npoint` check (`AST__DIMIN`) has no counterpart, since a PointSet holds its own count.
  - `draw_mark`: C gives no check of its own for a wrong axis count; the port's transform error stands. C's `AST__DIMIN` check has no counterpart.
  - `draw_gen_curve`: a Mapping that does not have 1 input and as many outputs as the current Frame has axes fails in the transform, as C's does.
  - `set_clip_region`: the frame index error FrameSet gives for an invalid index; `ErrorCode::Clpax` when `lbnd` or `ubnd` does not have one value per axis of the clipping Frame. This is a ruling: C's `AST__CLPAX` is for a NULL bounds pointer, and a slice of the wrong length is the Rust form of that mistake.
  - `draw_region_outline`: `ErrorCode::Nocnv` with C's message (`plot.c:24605`) when no conversion from the Plot's current Frame to the Region is found.
- **Call order.** C does not put these calls in one order across methods, and the port keeps each method's own:
  - `Curve`: reset box, reset Grf; then `CurvePlot` (`plot.c:8782`): if any `start` or `finish` value is bad, nothing at all (no attribute calls); else set the Curves attributes, `GScales`, trace with `Map3`, `Opoly`, restore attributes; then `Fpoly`.
  - `GenCurve` (`plot.c:14432`): set attributes, `GScales`, trace with `Map4`, `Opoly`, `Fpoly`, then restore attributes.
  - `PolyCurve` (`plot.c:23988`): `GScales`, then set attributes, then each segment whose two ends are good is traced with `Map3` into the same polyline (no `Opoly` between segments), then `Opoly`, `Fpoly` and restore. Fewer than two positions still sets and restores the attributes.
  - `Mark` (`plot.c:22711`): set the Markers attributes, transform with `norm` off, drop bad positions and (when `Clip & 2`) positions outside the plotting area, one `GMark` call, restore.
- **Tracer split** in `crates/starlink-ast/src/plot/curve.rs`, so `PolyCurve` can trace segments without ending the polyline:
  - `fn crv_pass(&mut self, map: &CurveMap, crv: &mut Crv) -> AstResult<()>` — the map over `CRV_NPNT` points and `crv`, without `opoly`; `trace_curve` becomes `crv_pass` then `opoly`.
  - `fn end_curve(&mut self, crv: &mut Crv) -> AstResult<CurveData>` — the bookkeeping now at the end of `trace` (the zero or final break, the `CRV_MXBRK` error, `purge`); `trace` becomes `trace_curve` then `end_curve`.
  - `Crv::start_segment(&mut self)` — what `PolyCurve` resets per segment: `ux0` to bad, `out` to true, and the breaks emptied. C resets nothing else; `nbrk` and `len` restart in `CrvLine` when `out` is set, as the port already does.
- **New `CurveMap` variants**, beside `Axis`, `Line` and `Region`:
  - `Geodesic { frame: DynFrame, map: DynMapping, origin: Vec<f64>, end: Vec<f64>, scale: f64 }` — `Map3` (`plot.c:22177`): `frame.offset(origin, end, scale * d)` per distance, then the Plot's transform from current to base with `norm` on, through `map`. `frame` is the current Frame, `map` the unsimplified base-to-current Mapping, and `scale` the `frame.distance(origin, end)`.
  - `User { umap: DynMapping, map: DynMapping }` — `Map4` (`plot.c:22368`): `umap` forward from distance to current-Frame position, then the same transform as `Geodesic`.
- **Curve data:** `draw_curve`, `draw_gen_curve` and `draw_poly_curve` store their `CurveData` in `self.draw.curve_data`, as C stores `Curve_data`; for `PolyCurve` that is the last segment's, as in C.
- **Marks:** a `Plot::g_mark` beside `g_line` (`plot.c:15060`): nothing is sent to the backend under Invisible, and the drawn box grows to every marker position whether or not it was drawn, as `g_line` does.
- **`RegionOutline`** (`plot.c:24538`): note the base index; `convert_to` from the Plot to a copy of the Region (which sets the Plot's base to the Frame it used); restore the base; add the Region as a new Frame joined to the current Frame by the conversion's unsimplified base-to-current Mapping; `self.draw_border(grf)` through the trait; remove that Frame and reselect the old current Frame. C joins the Region at the current Frame whichever Frame `astConvert` aligned through, since its Mapping starts at the current Frame; the port does the same.
- **Probe:** `tools/oracle_fixtures/plot_methods_probe.c` writes `tests/plot_methods_oracle/cases.rs`. Each case is a Plot (a header under `HEADS` or a 2-D Frame, a graphics box, a plot box, an attribute string, as `plot_curve_probe.c` builds them), a list of operations run in order under one capture, the log and SVG, and the bounding box after the last operation.
  The Rust side of the generated file:

  ```rust
  pub enum ClipFrame { None, Base, Current, Index(usize) }
  pub enum Op {
      Curve(&'static [f64], &'static [f64]),
      PolyCurve(&'static [&'static [f64]]),   // one slice per axis
      GenCurve(&'static str),                 // Mapping id
      Mark(&'static [&'static [f64]], i32),   // one slice per axis, marker
      Clip(ClipFrame, &'static [f64], &'static [f64]),
      Outline(&'static str),                  // Region id
  }
  pub struct Case {
      pub name: &'static str,
      pub head: Option<&'static str>,
      pub gbox: [f32; 4],
      pub pbox: [f64; 4],
      pub attr: &'static str,
      pub ops: &'static [Op],
      pub log: &'static str,
      pub svg: Svg,
      pub bbox: [f32; 4],                     // lbnd x, lbnd y, ubnd x, ubnd y
  }
  pub const CASES: &[Case] = &[ /* ... */ ];
  pub const CLIP_DUMPS: &[(&str, &str)] = &[ /* (case name, Native dump of the Plot after its ops) */ ];
  ```

  Mapping and Region ids are built in the probe and in the test from the same name, as `region_plot` is.
  Every case must draw something in C's SVG (a polyline or marker) unless its point is that nothing is drawn; a case that draws nothing by accident is moved until it does.
- **Upstream:** gate 5 says to offer the new fixtures upstream as port-only fixtures; as in stages 4 and 5, this plan does not, and the PR description says so.
- **Not ported:** `ast_tester/test_bbox_values.c` needs PLplot and checks only `astGTxExt`, not `astBoundingBox`; PLAN.md's tracker records it as deferred with Plot3D and PLplot.

## Review Focus

1. **Bad positions.** A `start` or `finish` with a bad value draws nothing and makes no attribute calls; a polycurve skips the segments on either side of a bad vertex and still joins the rest; bad marker positions are dropped. Pinned by `Op` lists in Tasks 1 and 2.
2. **Clip attribute bits against the clip region.** Curves are clipped only with bit 1 of Clip and markers only with bit 2; ClipOp switches between clipping a point outside any axis's bounds and outside all of them; a bad bound means no limit on that side. Pinned in Task 3.
3. **`draw_region_outline` leaves the Plot as it was.** Same Frame count, base and current indices and clip region afterwards, including when the Plot has a clip region on a Frame after the current one. Pinned in Task 4.
4. **A bounding box with nothing drawn.** After a drawing that drew nothing, `bounding_box` gives C's initial box, lower bounds `f32::MAX` and upper bounds `f32::MIN_POSITIVE` (C's `FLT_MIN`); under Invisible it gives the box of what would have been drawn. Pinned in Tasks 1 and 2.
5. **Curves across a discontinuity of a sky Plot.** A geodesic or polycurve segment that leaves the projection's valid area breaks there and resumes, as C's tracer does. Pinned by the sky cases in Task 1.

---

### Task 1: `draw_curve`, `draw_poly_curve` and `bounding_box`

**Files:**
- Modify: `crates/starlink-ast/src/plot/curve.rs` (tracer split, `CurveMap::Geodesic`, curve and polycurve drivers), `crates/starlink-ast/src/plot/draw.rs` (trait methods, docs), `docs/audits/c-docs/ledger.tsv`, `CHANGELOG.md`
- Create: `tools/oracle_fixtures/plot_methods_probe.c`, `crates/starlink-ast/tests/plot_methods_oracle/cases.rs`
- Test: `crates/starlink-ast/tests/test_plot_draw.rs`

**Interfaces:**
- Produces: `Draw::draw_curve`, `Draw::draw_poly_curve`, `Draw::bounding_box` as in Decisions; `crv_pass`, `end_curve`, `Crv::start_segment`, `CurveMap::Geodesic`; the probe and its `Op`, `ClipFrame`, `Case` and `CLIP_DUMPS` types, with the `Op` variants of later tasks already declared (the generator emits only those its cases use until then).

- [ ] **Step 1: Probe.** Cases on the 2-D Frame Plot of `plot_curve_probe.c` (box 0 to 10) and on `aitoff.head` (plot box 10, -10, 300, 280), at least:
  - `frame_curve`: three curves, one partly outside the plotting area; `frame_curve_styles` with `Colour(curves)=2,Width(curves)=3,Style(curves)=2`; `frame_curve_log` with `LogPlot(1)=1`; `frame_curve_bad`: a start with a bad value only (bbox is C's initial box); `frame_curve_invisible` with `Invisible=1`.
  - `aitoff_curve`: a short geodesic, and one that runs off the edge of the projection; `aitoff_curve_tol` with `Tol=0.001`.
  - `frame_poly`: five vertices, one bad in the middle; `frame_poly_one`: a single vertex; `aitoff_poly`: four vertices, one segment crossing the projection's edge.
- [ ] **Step 2: Failing test** `methods_match_the_c_library`: for each case build the Plot with `plot_of` and `apply_settings`, replay `ops` through `log_and_svg` and `compare`, and assert `plot.bounding_box()` after the ops equals `bbox` bit for bit. Add `From<&methods::Svg> for WantSvg`. Add `draw_curve_rejects_a_short_start` (`Ncpin`) and `draw_poly_curve_rejects_the_wrong_axis_count` (`Naxin`, C's message).
- [ ] **Step 3: Implement** the tracer split, `CurveMap::Geodesic` in `curve_map`, `CurvePlot` (`plot.c:8782`), `Curve` (`plot.c:8647`), `PolyCurve` (`plot.c:23988`) and `BoundingBox` (`plot.c:5228`), in the call order of Decisions.
- [ ] **Step 4: Pass**, with the stage 4 and 5 tests in `test_plot_draw.rs` still passing (the tracer split must not change them); ledger rows `astCurve`, `astPolyCurve`, `astBoundingBox`; CHANGELOG entry. Commit `Add Plot::draw_curve, draw_poly_curve and bounding_box`.

### Task 2: `draw_gen_curve` and `draw_mark`

**Files:**
- Modify: `crates/starlink-ast/src/plot/curve.rs` (`CurveMap::User`, gen-curve driver), `crates/starlink-ast/src/plot/draw.rs` (trait methods, `g_mark`), `tools/oracle_fixtures/plot_methods_probe.c`, `crates/starlink-ast/tests/plot_methods_oracle/cases.rs`, `docs/audits/c-docs/ledger.tsv`, `CHANGELOG.md`
- Test: `crates/starlink-ast/tests/test_plot_draw.rs`

**Interfaces:**
- Consumes: `crv_pass`, `end_curve` (Task 1).
- Produces: `Draw::draw_gen_curve`, `Draw::draw_mark`, `Plot::g_mark`, `CurveMap::User`.

- [ ] **Step 1: Probe cases.**
  - Mapping ids: `parabola`, a 1-to-2 PolyMap with x = 1 + 8t and y = 2 + 6t² (frame Plot); `sky_arc`, a 1-to-2 PolyMap linear in t whose positions cross the edge of the aitoff projection.
  - `frame_gen_curve`, `frame_gen_curve_styles` (Curves styles set), `aitoff_gen_curve`.
  - `frame_mark`: markers 1 and 3, with one bad position and one outside the plotting area, under the default Clip; `frame_mark_clip` with `Clip=2` (the outside marker dropped); `frame_mark_styles` with `Colour(markers)=4,Size(markers)=2`; `frame_mark_invisible`; `aitoff_mark`.
- [ ] **Step 2: Failing tests:** the new cases fail in `methods_match_the_c_library`; `draw_gen_curve_rejects_a_two_input_mapping` (the transform's error, the code C's gives).
- [ ] **Step 3: Implement** `CurveMap::User` (`Map4`), `GenCurve` (`plot.c:14432`), `GMark` (`plot.c:15060`) and `Mark` (`plot.c:22711`).
- [ ] **Step 4: Pass**; ledger rows `astGenCurve`, `astMark`; CHANGELOG. Commit `Add Plot::draw_gen_curve and draw_mark`.

### Task 3: `set_clip_region` and `clear_clip_region`

**Files:**
- Modify: `crates/starlink-ast/src/plot/draw.rs` (trait methods), `crates/starlink-ast/src/plot/mod.rs` (the `ClipRegion` it sets), `tools/oracle_fixtures/plot_methods_probe.c`, `crates/starlink-ast/tests/plot_methods_oracle/cases.rs`, `docs/audits/c-docs/ledger.tsv`, `CHANGELOG.md`
- Test: `crates/starlink-ast/tests/test_plot_draw.rs`

**Interfaces:**
- Consumes: the curve, polycurve and mark ops (Tasks 1 and 2).
- Produces: `Draw::set_clip_region`, `Draw::clear_clip_region`.

- [ ] **Step 1: Probe cases**, each a Clip op followed by a curve, a polycurve and marks that cross the clip box:
  - `frame_clip_base`: clip box in the base (graphics) Frame, default Clip and ClipOp; `frame_clip_current` in the current Frame with a bad lower bound on axis 2; `frame_clip_op` with `ClipOp=1`; `frame_clip_curves` with `Clip=1` and `frame_clip_marks` with `Clip=2`, so each kind is clipped only by its own bit.
  - `frame_clip_cleared`: a clip set and then cleared, so nothing is clipped.
  - `aitoff_clip`: a clip box in the sky Frame (by index), with a curve and marks.
  - `CLIP_DUMPS` for `frame_clip_current` and `aitoff_clip`: the Plot's Native dump after its ops.
- [ ] **Step 2: Failing tests:** the new cases; `clip_dumps_match_the_c_library` (the Native dump of the Plot after replaying the ops, through `common::native_dump`, equal to C's); `set_clip_region_rejects_bounds_of_the_wrong_length` (`Clpax`) and `set_clip_region_rejects_an_invalid_frame`.
- [ ] **Step 3: Implement** `Clip` (`plot.c:7030`): the base-Frame check, frame validation, bounds stored with bad replaced by `-f64::MAX` and `f64::MAX`, and clearing for `clear_clip_region`.
- [ ] **Step 4: Pass**; ledger row `astClip` (both methods); CHANGELOG. Commit `Add Plot::set_clip_region and clear_clip_region`.

### Task 4: `draw_region_outline`, and closing the stage

**Files:**
- Modify: `crates/starlink-ast/src/plot/draw.rs` or `border.rs` (`draw_region_outline`), `tools/oracle_fixtures/plot_methods_probe.c`, `crates/starlink-ast/tests/plot_methods_oracle/cases.rs`, `docs/audits/c-docs/ledger.tsv`, `CHANGELOG.md`, `PLAN.md` (Plot row; tracker row for `test_bbox_values.c`), `docs/superpowers/specs/2026-10-30-plot-design.md` (stage 6 done note)
- Test: `crates/starlink-ast/tests/test_plot_draw.rs`

**Interfaces:**
- Consumes: `Draw::draw_border` (stage 4, with stage 5's Region tracing).
- Produces: `Plot::draw_region_outline`.

- [ ] **Step 1: Probe cases**, one Outline op each (several per case where it keeps the file short), the Regions built by id:
  - On the frame Plot: `box`, `circle`, `ellipse`, `interval`, `polygon` (seven vertices), `and_overlap` (an AND CmpRegion of two overlapping Circles), `or_disjoint` (an OR of two disjoint Circles, split by `RegSplit`), and `interval_open` (an Interval unbounded on one side, which cannot trace and is drawn through `Boundary`'s Region branch).
  - On the aitoff Plot: `sky_circle` and `sky_polygon` in the header's sky system, `gal_circle` (a Circle in Galactic, converted), and `moc` (a Moc whose cells are inside the plotted area).
  - `frame_outline_clipped`: an Outline on a Plot with a clip region on its base Frame.
- [ ] **Step 2: Failing tests:** the new cases; `draw_region_outline_leaves_the_plot_as_it_was` (Frame count, base, current and the Native dump unchanged, on a Plot with three Frames whose current Frame is not the last, and with a clip region on Frame 3); `draw_region_outline_rejects_a_region_it_cannot_convert` (a 3-D Box on the 2-D Frame Plot, `Nocnv`, C's message).
- [ ] **Step 3: Implement** `RegionOutline` as in Decisions.
- [ ] **Step 4: Pass**; ledger row `astRegionOutline`; CHANGELOG. Spec stage 6 marked done with this plan's path and the Rust names; PLAN.md's Plot row records stage 6 done and the tracker records `test_bbox_values.c` as deferred (PLplot). Full suite, clippy and docs. Commit `Add Plot::draw_region_outline`.

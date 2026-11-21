# Plot Stage 1: Frame Methods Plot Needs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port C's `astGap`, `astCentre`, `astAbbrev` and `astFields` to every Frame class with C's per-class behavior, and close SK-18 for the line family, each checked against the C library through a probe.

**Architecture:** C reaches these through two layers: a Frame method (`Gap`, overridden by SkyFrame, TimeFrame, CmpFrame, FrameSet, Region) and, in the base Frame, the Axis method of the Axis object `astGetAxis` returns (`AxisGap`, overridden by SkyAxis).
The port keeps both layers as `Frame` trait methods: `gap`/`centre`/`abbrev`/`fields` for the Frame layer and `axis_gap`/`axis_centre`/`axis_abbrev`/`axis_fields` for the Axis layer, routed to the axis's owner the way `get_axis` already is.
The two layers diverge in C (a Region holding a TimeFrame splits fields with the plain Axis, not TimeFrame's override), so collapsing them would be hazard H1.

**Tech Stack:** Rust 2024, `starlink-ast` crate; C oracle probes built against `reference/starlink-ast/build` (`-ffp-contract=off`).

**Spec:** [`docs/superpowers/specs/2026-10-30-plot-design.md`](../specs/2026-10-30-plot-design.md), stage 1 and gate 6.

## Scope found while planning

The spec's stage-1 list predates work already landed.
`frame_grid`, `normal_points`/`norm_points` and `line_def`/`line_offset`/`line_crossing`/`line_contains` exist for Frame, SkyFrame, CmpFrame, FrameSet and Region (`crates/starlink-ast/src/frames/mod.rs:1883-1990`, `skyframe.rs:2487-2990`).
`docs/issues/frame-mapping-c-port-gaps.md:235` is stale on this; Task 5 corrects it.
What is missing is `Gap`, `Centre`, `Abbrev`, `Fields` at both layers, C-oracle coverage of the line family, and SK-18's line ownership check.

`plot.c` and `plot3d.c` call no line method (`rg -c astLine plot.c` is 0); the callers are `polygon.c` and the FrameSet/Region forwards.
SkyFrame's Cartesian side-channel is reachable but untriggered.
`LineCrossing` always writes the Cartesian crossing vector into `cross[2..5]` (it is its working storage), and `LineContains` reads it back from `point + 2` only when `def` is non-zero (skyframe.c:5423).
`astLineContains` is protected (`#if defined(astCLASS)` in `frame.h`, absent from `ast.h`), so only code inside the library could pass `def = 1`, and no C caller does: `polygon.c:6407` passes 0, and the FrameSet and Region forwards pass their caller's value through.
It is an optimization hook, saving the recomputation of a point's Cartesian vector, that C built and never used.
This plan ports the ownership check and does **not** port the side-channel.
Task 5 records the hook where the performance phase of the port will find it, beside PG-5 (the Polygon edge cache, deferred to that phase in `docs/audits/region-c-parity.md`), since the Polygon edge loops (`polygon.c:4152`, `:6407`) are where reusing the vector would pay.

## Global Constraints

- Stage 1 of the plot design is one PR on its own branch, stacked on `u/timj/plotting` (the branch carrying the design). Branch: `u/timj/plot-1-frame-methods`.
- 1-based axis indices in the Rust API; C's protected `astGap_` etc. take **0-based** axes, so the probe passes `axis - 1`.
- No C in `starlink-ast`; no new dependencies.
- Every new source file opens with the two-line AURA copyright / `SPDX-License-Identifier: LGPL-3.0-or-later` header; generated `cases.rs` is exempt.
- Every new public trait method carries `#[doc(alias = "astGap")]` (etc.), docs written for Rust with C provenance in `//` comments only.
- `cargo fmt`, `cargo clippy --workspace -- -D warnings` and `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps` clean.
- A user-visible change gets a `CHANGELOG.md` `Unreleased` entry in the same commit, prefixed `starlink-ast:`.
- No tolerance in the oracle comparison beyond the existing `crate::common::c_values_eq`; any per-case relaxation is named in source and counted in `PLAN.md` §6.
- Do not run the full suite per task; run the one area test with `cargo test -p starlink-ast --test frames test_plot_frame_methods::`. Run `cargo test -p starlink-ast` once at the end (Task 5).
- Strip conda's `CC`/`LDFLAGS` from the environment for cargo and for probe builds.

## Review Focus

1. **A zero or bad gap.** `AxisGap` returns 0 and leaves `*ntick` untouched when `gap == 0` (axis.c:915). The Rust return is `(0.0, None)`; Plot keeps its previous minor-tick count on `None`. Pinned in Task 1.
2. **Frame Digits set, Axis Digits unset.** `frame.c` `Gap`/`Centre`/`Abbrev`/`Fields` do not lend the Frame's Digits to the Axis (contrast `Format`, frame.c:4353), but `CmpFrame::Gap` lends the CmpFrame's Digits to the sub-Frame (cmpframe.c:2967). A sky axis's gap depends on which Digits it sees. Pinned in Tasks 2 and 4.
3. **A Region or CmpFrame holding a TimeFrame with a date Format.** `fields` through a Region reaches the plain Axis (region.c has no `Fields` override, so `frame.c` `Fields` → `astGetAxis`), not TimeFrame's date splitting. Pinned in Task 4.
4. **Abbrev on identical, empty, or differently-fielded strings.** The returned offset must be C's (start of the last field, or 0), never a past-the-end index. Pinned in Tasks 1 and 2.
5. **A permuted SkyFrame.** Axis 1 is then latitude, so gap and fields use degrees, not hours. Pinned in Task 2.

---

### Task 1: The two-layer seam and the plain Axis algorithms

**Files:**
- Modify: `crates/starlink-ast/src/frames/mod.rs` (trait `Frame`: eight new methods; `FieldSplit` type)
- Modify: `crates/starlink-ast/src/frames/axis.rs` (plain-axis algorithms)
- Modify: `crates/starlink-ast/src/frames/basic.rs` (`axis_*` dispatch on `FrameAxis`, plain arm only for now)
- Create: `tools/oracle_fixtures/plot_frame_methods_probe.c`
- Create: `crates/starlink-ast/tests/plot_frame_methods_oracle/cases.rs` (generated)
- Create: `crates/starlink-ast/tests/test_plot_frame_methods.rs`
- Modify: `crates/starlink-ast/tests/frames/main.rs` (add the module)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces, in `crates/starlink-ast/src/frames/mod.rs`, re-exported from `starlink_ast::frames`:
  ```rust
  /// Fields found in a formatted axis value.
  pub struct FieldSplit {
      /// Byte range of each field within the input, most significant first.
      pub fields: Vec<std::ops::Range<usize>>,
      /// The axis value the fields represent, or BAD_VALUE.
      pub value: f64,
  }
  ```
  Trait `Frame` methods (Frame layer, `#[doc(alias)]` in brackets):
  ```rust
  fn gap(&self, axis: usize, gap: f64) -> AstResult<(f64, Option<i32>)>;          // astGap
  fn centre(&self, axis: usize, value: f64, gap: f64) -> AstResult<f64>;          // astCentre
  fn abbrev(&self, axis: usize, fmt: &str, str1: &str, str2: &str) -> AstResult<usize>; // astAbbrev
  fn fields(&self, axis: usize, fmt: &str, text: &str, maxfld: usize) -> AstResult<FieldSplit>; // astFields
  ```
  Axis layer (C `astGetAxis` then the Axis method; routed to the axis's owner like `get_axis`):
  ```rust
  fn axis_gap(&self, axis: usize, gap: f64) -> AstResult<(f64, Option<i32>)>;
  fn axis_centre(&self, axis: usize, value: f64, gap: f64) -> AstResult<f64>;
  fn axis_abbrev(&self, axis: usize, fmt: &str, str1: &str, str2: &str) -> AstResult<usize>;
  fn axis_fields(&self, axis: usize, fmt: &str, text: &str, maxfld: usize) -> AstResult<FieldSplit>;
  ```
  `gap` returns the nice gap and the minor-tick count; `None` means C left `*ntick` unchanged.
  `abbrev` returns a byte offset into `str2` (C returns a pointer into `str2`).
  Defaults: each Frame-layer method validates the axis (`ErrorCode::Axiin`, as `format` does) and calls its Axis-layer twin (frame.c:4595, 1849, 1039, 3648). Each Axis-layer default applies the plain Axis algorithm.
- Produces, in `axis.rs`: `pub(crate) fn plain_axis_gap(gap: f64) -> (f64, Option<i32>)`, `plain_axis_centre(value: f64, gap: f64) -> f64`, `plain_axis_abbrev(fmt: &str, str1: &str, str2: &str) -> usize`, `plain_axis_fields(fmt: &str, text: &str, maxfld: usize) -> FieldSplit`, ports of axis.c `AxisGap` (915), `AxisCentre` (853), `AxisAbbrev` (273), `AxisFields` (414).
- Produces the probe and test scaffolding later tasks extend: the probe prints one Rust `const` slice per method; the test builds each case's Frame from a string id with a `fn frame(id: &str) -> Box<dyn Frame>` match.

- [ ] **Step 1: Branch.**
  Run `git status --short && git branch --show-current`; then `git switch -c u/timj/plot-1-frame-methods u/timj/plotting`.

- [ ] **Step 2: Write the probe's plain cases.**
  `plot_frame_methods_probe.c`, header modelled on `sky_geometry_probe.c` (purpose, regeneration command). Declare the protected entry points from `frame.h:951-963` (`astGap_`, `astCentre_`, `astAbbrev_`, `astFields_`) and call them through `astMakePointer` (`docs/issues/c-oracle-probe-recipe.md`). Set `ntick = -999` before each `astGap_` so "unchanged" prints as `None`; set `val = AST__BAD` before each `astFields_`. Print `fields` as `(start, len)` byte offsets from the input pointer. Emit:
  ```rust
  pub const GAP_CASES: &[(&str, usize, f64, f64, Option<i32>)];             // (frame id, axis, gap, result, ntick)
  pub const CENTRE_CASES: &[(&str, usize, f64, f64, f64)];                  // (id, axis, value, gap, result)
  pub const ABBREV_CASES: &[(&str, usize, &str, &str, &str, usize)];        // (id, axis, fmt, str1, str2, offset)
  pub const FIELDS_CASES: &[(&str, usize, &str, &str, usize, &[(usize, usize)], f64)]; // (id, axis, fmt, text, maxfld, spans, value)
  ```
  Frame id `"basic2"` = `astFrame(2, "")`. Gap inputs: 0, BAD, ±1e-300, ±0.0037, ±1, ±7.4, ±9.6, ±123.0, 1e300 (one per `table1` slot at least). Centre: value BAD, gap 0, gap BAD, value 17.3 gap 5, value -2.5 gap 5 (rounds half up). Abbrev with `%g`: identical strings, `""` vs `"12"`, `"1.5"` vs `"2.5"`. Fields with `%g`, `%&g` and maxfld 1 and 20: `"1.5e3"`, `"  -7 "`, `""`, `"10**4"` with `%&g`.
  Build: `cd reference/starlink-ast/build && cc ../../../tools/oracle_fixtures/plot_frame_methods_probe.c -I. -L. -last -lm -Wl,-rpath,$PWD -o ../../../tools/oracle_fixtures/plot_frame_methods_probe` (confirm `otool -tv libast.dylib | grep -c fmadd` is 0 first; rebuild per `reference-libast-build` if not).
  Run it into `crates/starlink-ast/tests/plot_frame_methods_oracle/cases.rs`.

- [ ] **Step 3: Write the failing test.**
  `test_plot_frame_methods.rs` (header comment names the probe, as `test_sky_geometry.rs` does), registered in `tests/frames/main.rs`. One `#[test]` per method — `gap_matches_the_c_library`, `centre_matches_the_c_library`, `abbrev_matches_the_c_library`, `fields_matches_the_c_library` — each looping its slice, collecting failures with the case printed, and asserting the list is empty. Compare floats with `crate::common::c_values_eq`, offsets and spans exactly.
  Add `gap_of_zero_leaves_ntick_unset`: `basic.gap(1, 0.0).unwrap() == (0.0, None)`.

- [ ] **Step 4: Run it and see it fail to compile** (`gap` not a method).
  `cargo test -p starlink-ast --test frames test_plot_frame_methods::`

- [ ] **Step 5: Implement** the `FieldSplit` type, the eight trait methods with the defaults above, the four `plain_axis_*` functions, and `BasicFrame`'s `axis_*` overrides dispatching on `self.axis_object(axis)?` (plain arm calls `plain_axis_*`; leave the `FrameAxis::Sky` arm as `todo!("Task 2")`).
  `AxisGap`'s table index is `round(absgap / b) - 1`; keep C's `round` and `floor(log10(..))` order of operations so bits match.

- [ ] **Step 6: Run the test; it passes.** Same command.

- [ ] **Step 7: CHANGELOG and commit.**
  Add under `Unreleased` → `Added`: `starlink-ast: Frame::gap, centre, abbrev and fields, which choose and split axis values for annotating plots, as C's astGap, astCentre, astAbbrev and astFields do.`
  `git add` the files above; commit `Add Frame gap, centre, abbrev and fields for plain axes`.

### Task 2: Sky axes and SkyFrame

**Files:**
- Modify: `crates/starlink-ast/src/frames/skyaxis.rs`
- Modify: `crates/starlink-ast/src/frames/skyframe.rs`
- Modify: `crates/starlink-ast/src/frames/basic.rs` (the `FrameAxis::Sky` arm)
- Modify: probe, `cases.rs`, `test_plot_frame_methods.rs`

**Interfaces:**
- Consumes: Task 1's trait methods, `FieldSplit`, `plain_axis_centre`.
- Produces, in `skyaxis.rs`: `pub(crate) fn sky_axis_gap(fmt: &str, digits: i32, gap: f64) -> AstResult<(f64, Option<i32>)>` (skyaxis.c `AxisGap` 890 and its `DHmsGap`), `sky_axis_abbrev(fmt: &str, digits: i32, str1: &str, str2: &str) -> AstResult<usize>` (307), `sky_axis_fields(fmt: &str, digits: i32, text: &str, maxfld: usize) -> AstResult<FieldSplit>` (479). SkyAxis has no `AxisCentre`; it uses the plain one.
- `SkyFrame::gap` override: skyframe.c:2314 lends the SkyFrame's Format (set, or SkyFrame's default `GetFormat`) to the axis, then runs the parent `Gap`. In Rust: `sky_axis_gap(&self.phys_format(phys), digits, gap)`, with `digits` whatever `DHmsGap` would read through `astGetAxisDigits` on that axis — determine it from the C, then confirm with the Review Focus 2 probe cases.
- `SkyFrame::axis_*` and `BasicFrame::axis_*` for a SkyAxis use the **Axis object's own** Format and Digits (set, or SkyAxis's default), because `frame.c` lends nothing to the Axis in these methods. Abbrev and Fields take `fmt` from the caller and only Digits from the axis.

- [ ] **Step 1: Extend the probe.**
  Frame ids: `"sky"` (`astSkyFrame("")`), `"sky_perm"` (axes permuted 2,1), `"sky_fmt"` (`Format(1)=hms.2, Format(2)=dms.1`), `"sky_digits"` (`Digits=10` on the SkyFrame only), `"sky_axdigits"` (`Digits(1)=3`), `"basic_skyaxis"` (an `astPickAxes` of a SkyFrame into a plain Frame, which keeps a SkyAxis). Gap inputs in radians from 1e-9 to 6.0 across the `DHmsGap` table boundaries (read skyaxis.c for them), each axis. Abbrev with `hms`/`dms` strings sharing 0, 1 and 2 leading fields, and differing field counts. Fields with `hms.2`, `dms`, `+zhms` on `"12:34:56.78"`, `"-01:02:03"`, `"12h34m"`, maxfld 1 and 3.
  Regenerate `cases.rs`; add the ids to the test's `frame()` match.

- [ ] **Step 2: Run the test; the new cases fail** (`todo!` panics or mismatches).

- [ ] **Step 3: Implement** the three `sky_axis_*` functions, the BasicFrame Sky arm, `SkyFrame::axis_*`, and `SkyFrame::gap`.

- [ ] **Step 4: Run the test; it passes.**

- [ ] **Step 5: Commit** `Add gap, abbrev and fields for sky axes`.

### Task 3: TimeFrame

**Files:**
- Modify: `crates/starlink-ast/src/frames/timeframe.rs`
- Modify: probe, `cases.rs`, `test_plot_frame_methods.rs`

**Interfaces:**
- Consumes: Task 1 defaults (the parent methods C calls as `parent_gap` etc.), `TimeFrame::current_time` (timeframe.rs:762), `make_map` (1308), `date_format` (1676).
- Produces: `TimeFrame` overrides of `gap` (timeframe.c:1838), `centre` (603), `abbrev` (439), `fields` (1306). Each uses the parent Frame method unless `date_format(&self.format_str(1)?)` reports a date Format, as C does with `DateFormat(astGetFormat(this, 0))`. TimeFrame's Axis-layer methods stay the plain defaults: its axis is a plain Axis.
- Port the `Gap` day/year ladder (365.25, 270, 150, 90, 45, 22, 12, 7.5, 4.5, 3, 1.5 … days with their `ntick`) and the sub-day branches as written; these thresholds are C's, not tolerances.

- [ ] **Step 1: Extend the probe.**
  Frame ids: `"time"` (default MJD, no date Format), `"time_iso"` (`Format=iso.0`), `"time_iso2"` (`Format=iso.2`), `"time_jd_iso"` (`System=JD, Unit=d, Format=iso.0`), `"time_origin_iso"` (`TimeOrigin=56000, Format=iso.0`). Gap inputs that land in every branch of the ladder in timeframe.c:1838-2000 and its sub-day branches (read them for the boundaries), plus 0. Centre, Abbrev and Fields with dates from `astFormat` of the same Frame, so the strings are C's own.
  `astCurrentTime` makes `Gap`'s date branch depend on the clock only through the difference `xout[1]-xout[0]`; if a case's result still differs run to run, pick a TimeScale whose MJD mapping is linear and say so in the probe.

- [ ] **Step 2: Run the test; the new cases fail.**

- [ ] **Step 3: Implement** the four overrides.

- [ ] **Step 4: Run the test; it passes.**

- [ ] **Step 5: Commit** `Add TimeFrame gap, centre, abbrev and fields for date formats`.

### Task 4: Routing through CmpFrame, SpecFluxFrame, FrameSet and Region

**Files:**
- Modify: `crates/starlink-ast/src/frames/cmpframe.rs`
- Modify: `crates/starlink-ast/src/frames/specfluxframe.rs`
- Modify: `crates/starlink-ast/src/frames/frameset.rs`
- Modify: `crates/starlink-ast/src/frames/frame_attrs.rs` (the Region forwarding macro, near line 1099)
- Modify: probe, `cases.rs`, `test_plot_frame_methods.rs`

**Interfaces:**
- Consumes: Tasks 1-3.
- CmpFrame: Frame layer forwards to the sub-Frame through `route_axis` (cmpframe.c `Abbrev` 825, `Centre` 1370, `Fields` 2560, `Gap` 2967). `Gap` lends the CmpFrame's Digits to the sub-Frame when the sub-Frame's is unset (cmpframe.c:2967); read the other three and lend exactly where C does. The Rust lending must not mutate: pass the lent Digits down, following however `CmpFrame::format` already handles the same lending. Axis layer routes `axis_*` to `child.axis_*(local_axis, …)`, as C's `GetAxis` reaches the sub-Frame's Axis.
- SpecFluxFrame: route both layers the way its `format` does (specfluxframe.rs:328).
- FrameSet: both layers forward to `current()` (frameset.c `Abbrev` 1091, `Centre` 2461, `Fields` 3697, `Gap` 4470; `GetAxis` also forwards).
- Region: `gap`, `centre`, `abbrev` forward to the FrameSet's current Frame (region.c 2991, 2053, 1208). `fields` is **not** overridden: it keeps the trait default, which calls `self.axis_fields`, and Region's `axis_*` forward to `current().axis_*`. This is what makes Review Focus 3 hold.

- [ ] **Step 1: Extend the probe.**
  Frame ids: `"cmp_sky_time"` (`astCmpFrame(SkyFrame, TimeFrame Format=iso.0)`, `Digits=10` on the CmpFrame only), `"cmp_perm"` (same, axes permuted 3,1,2), `"specflux"`, `"frameset_sky"` (FrameSet whose current Frame is `"sky_fmt"`), `"box_time_iso"` (an `astBox` on `"time_iso"`, 1-D), `"box_sky"` (an `astBox` on `"sky"`). For every id, at least one case per method; for `"box_time_iso"` and `"cmp_sky_time"` axis 3, a `fields` case on an iso date string.

- [ ] **Step 2: Write the Review Focus 3 test.**
  `region_fields_bypass_timeframe_date_splitting`: on `"box_time_iso"`, `fields(1, "iso.0", <date>, 20)` equals the plain-axis split of the same text (via `BasicFrame`), and differs from the TimeFrame's own `fields`. The C cases from Step 1 back this; the named test states the rule.

- [ ] **Step 3: Run the test; the new cases fail.**

- [ ] **Step 4: Implement** the overrides.

- [ ] **Step 5: Run the test; it passes.**

- [ ] **Step 6: Commit** `Route gap, centre, abbrev and fields through compound Frames`.

### Task 5: Line family against C, SK-18 ownership, and bookkeeping

**Files:**
- Modify: `crates/starlink-ast/src/frames/mod.rs` (`line_offset`, `line_crossing`, `line_contains`: ownership check)
- Modify: `crates/starlink-ast/src/frames/skyframe.rs` (same, the other way round)
- Create: `tools/oracle_fixtures/line_geometry_probe.c`
- Create: `crates/starlink-ast/tests/line_geometry_oracle/cases.rs` (generated)
- Create: `crates/starlink-ast/tests/test_line_geometry.rs`; register in `tests/frames/main.rs`
- Modify: `docs/issues/frame-mapping-c-port-gaps.md` (lines 25, 37, 40, 217, 235, 366, 369, 408)
- Modify: `docs/audits/frame-family-c-parity.md` (SK-18 rows, lines 79 and 137)
- Modify: `docs/audits/region-c-parity.md` (PG-5 entry, line 604)
- Modify: `PLAN.md` (the Plot-support row, and a new §2 performance-candidate row)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Ownership check: C errors `AST__INTER` when a LineDef was made by another Frame (frame.c:6544, 6666, 6671; skyframe.c:5414, 5551, 5556). Rust has no object identity across `Clone`, so the port checks the kind instead: the planar methods reject a `LineDef` with `sky: Some(_)`, SkyFrame's reject one with `sky: None`, each with `ErrorCode::Inter` and C's message text (`"astLineContains(SkyFrame): The supplied line does not relate to the supplied SkyFrame (AST internal programming error)."`, and the `First`/`Second supplied line` forms for crossing). Say in a `//` comment that this is the reachable part of C's pointer check.
- No change to `LineDef`, `line_crossing`'s return or `line_contains`'s arguments (no side-channel; see "Scope found while planning").

- [ ] **Step 1: Write the probe.**
  `line_geometry_probe.c`, using `astLineDef_`, `astLineOffset_`, `astLineCrossing_`, `astLineContains_` (frame.h:949-1027) with `def = 0`, freeing each LineDef with `astFree`. Frames: `astFrame(2,"")`, `astSkyFrame("")`, the same permuted. Cases: ordinary segments; zero-length; a bad start point (`astLineDef` returns NULL → Rust `None`); parallel and anti-parallel lines; crossing exactly at a start point and at an end point (start inclusive, end exclusive); a point on the line within and just outside `1e-7 * length`; sky segments crossing RA 0 and passing near a pole. Also `astFrameGrid` on each frame and on a CmpFrame of the two, size 9 and 100, so `frame_grid` is compared directly. Emit one `const` slice per method.

- [ ] **Step 2: Write the failing tests.**
  `test_line_geometry.rs`: one oracle test per method, as in Task 1; plus `planar_frame_rejects_a_sky_line` and `sky_frame_rejects_a_planar_line`, each asserting `ErrorCode::Inter` and the message.

- [ ] **Step 3: Run them.** `cargo test -p starlink-ast --test frames test_line_geometry::`
  The ownership tests fail. If an oracle case fails, it is a real divergence: debug it against the C before changing anything (CLAUDE.md "Stop signals").

- [ ] **Step 4: Implement** the ownership check; fix any oracle divergence found.

- [ ] **Step 5: Run the tests; they pass.**

- [ ] **Step 6: Bookkeeping.**
  Mark `Gap`/`Centre`/`Abbrev`/`Fields` fixed in `frame-mapping-c-port-gaps.md`, and correct its stale claim that `FrameGrid`, `NormalPoints` and the line family are missing. Set both SK-18 rows to closed: ownership ported as a kind check; the Cartesian side-channel is reachable only through the protected interface with `def = 1`, which no C caller passes, and `plot.c`/`plot3d.c` call no line method. Update `PLAN.md`.
  Record the side-channel as a performance candidate in two places, so the profiling phase finds it: a sentence in the PG-5 entry of `docs/audits/region-c-parity.md` (line 604), and a new `PLAN.md` §2 row, status `DEFERRED to the performance phase`, saying: SkyFrame `LineCrossing` computes the crossing's Cartesian vector and C's `LineContains(def = 1)` can reuse it rather than recompute it from the axis values; C never uses the hook; the port drops it (`LineDef` and the line methods carry no Cartesian state); measure the Polygon edge loops with `ast-bench` on a sky Polygon before adding it, and add it with PG-5's edge cache if it pays. Add `starlink-ast: the line methods report ErrorCode::Inter when given a line made by a different kind of Frame, as C does.` to `CHANGELOG.md`.

- [ ] **Step 7: Full gates.**
  `cargo fmt --check && cargo clippy --workspace -- -D warnings && RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps && cargo test -p starlink-ast`
  Expected: all pass, including `test_c_docs_audit` (if it asks for ledger rows for the four new aliases, add them to `docs/audits/c-docs/ledger.tsv` as `function`/`frame.c` rows naming the `Frame::` method).

- [ ] **Step 8: Commit** `Check line ownership and compare the line methods with C`.

# Plot Stage 5: `RegTrace` and `RegSplit` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Regions trace their own boundaries and split into disjoint parts as C's `astRegTrace` and `astRegSplit` do, checked against C (gate 6), and a Plot whose current Frame is a Region draws its border and grid as C does.

**Architecture:** `astRegTrace` is a protected Region method with seven class overrides (Box, Circle, Ellipse, Interval, Polygon, Moc, CmpRegion) and a base that reports "not implemented"; `astRegSplit` has a base that returns the Region itself and a CmpRegion override that splits an OR tree into disjoint parts.
Both become `#[doc(hidden)]` methods on the `Region` trait, as the crate's other protected Region methods are (`reg_base_mesh`, `reg_pins`), at C's granularity, since Plot's `DrawRegion` dispatches to them per class (CLAUDE.md, "Before porting a class").
With them in place, Plot's `DrawRegion` and `Map5`, and the Region branch of `Boundary`, replace the error stage 4 left for a Region current Frame.

**Tech Stack:** Rust 2024, `starlink-ast`; C probes against the reference `libast` (`-ffp-contract=off`), calling the protected functions through `astMakePointer` as `docs/issues/c-oracle-probe-recipe.md` describes.

**Spec:** [`docs/superpowers/specs/2026-10-30-plot-design.md`](../specs/2026-10-30-plot-design.md), stage 5 and gate 6; `docs/audits/region-class-coverage-audit.md` RT-TRACE and RS-SPLIT.

## Global Constraints

- Branch `u/timj/plot-5-regtrace`, stacked on `u/timj/plot-4-grid`; one PR.
- Port C properly, at C's granularity; every C quirk mirrored gets a `c-library-quirks.md` entry; an over-read or undefined value in C is ported as the nearest defined behavior, with a ruling and a quirks entry.
- Values from probes compared bit for bit (macOS); off macOS through the existing `common::c_value_eq` tolerance, as the other Region oracles are.
- Oracle data in generated `crates/starlink-ast/tests/<name>_oracle/cases.rs`; no files added under `tests/fixtures/`.
- Commit messages end with the `Co-Authored-By` line only (no session line).
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc -p starlink-ast --no-deps` clean; strip conda.
- Per-task test: `cargo test -p starlink-ast --test regions test_region_trace` (Tasks 1–2) and `--test frames test_plot_draw` (Task 3); full suite in Task 3.

## Decisions

- **Signatures** (on `Region`, `#[doc(hidden)]`):
  - `fn reg_trace(&self, dist: &[f64]) -> AstResult<Option<PointSet>>` — `None` where C returns 0 (class without the method, or a base Frame not 2-D); otherwise the current-Frame positions at each `dist`, one point per value. An empty `dist` answers C's support query (`n == 0`).
  - `fn reg_split(&self) -> AstResult<Vec<DynRegion>>` — the base returns a copy of the Region; CmpRegion overrides.
- **CmpRegion's break information** (C's `SetBreakInfo`, cached on the object and cleared when it changes) is computed when `reg_trace` needs it, not stored: the values are the same, and the port has no invalidation to get wrong.
- **Probes:** `region_trace_probe.c` writes `tests/region_trace_oracle/cases.rs` with `TRACE_CASES` (simple classes), `CMP_TRACE_CASES` and `SPLIT_CASES` (CmpRegion); each Region is built in the probe and in the Rust test from the same id, as `region_*_probe.c` and their tests do. Task 3 adds Region cases to `plot_border_probe.c`.
- **Test file:** `crates/starlink-ast/tests/test_region_trace.rs`, registered in `tests/regions/main.rs`.

## Review Focus

1. **A Region whose base Frame is not 2-D** (a 3-D Box, a 1-D Interval): `reg_trace` is `None`, as C's 0. Pinned in Task 1.
2. **A negated Region**: its boundary is traced the same way, and a negated CmpRegion component changes what CmpRegion keeps. Pinned in Tasks 1 and 2.
3. **A Region with a non-unit base-to-current Mapping** (a Circle mapped through a ZoomMap with `map_region`, a Polygon on a SkyFrame): positions go through the transform path. Pinned in Task 1.
4. **`dist` at 0 and 1, repeated values, and an empty list**: the ends coincide for a closed boundary; an empty list only answers support. Pinned in Task 1.
5. **A CmpRegion whose second component has no `RegTrace`**: C tests the first component twice (`cmpregion.c:2806`), so it claims support and then traces from arrays the second component never fills. Rule on the port's answer in Task 2 and pin it.

---

### Task 1: `reg_trace` for the base and the simple classes

**Files:**
- Modify: `crates/starlink-ast/src/regions/mod.rs` (trait method, base), `box.rs` (`BoxRegion`), `circle.rs`, `ellipse.rs`, `interval.rs`, `polygon.rs`, `moc.rs`
- Create: `tools/oracle_fixtures/region_trace_probe.c`, `crates/starlink-ast/tests/region_trace_oracle/cases.rs`, `crates/starlink-ast/tests/test_region_trace.rs`

**Interfaces:**
- Produces: `Region::reg_trace` as in Decisions.

- [ ] **Step 1: Probe.** For each Region (Box, Circle, Ellipse, Interval, Polygon and Moc on a 2-D Frame and on a SkyFrame; a Circle mapped through a ZoomMap; negated forms; a 3-D Box and a 1-D Interval; a PointList, as a class without the method): C's return value with `n = 0`, and the positions at `dist` = 0, 0.125, 0.25, 0.5, 0.5, 0.75, 0.9999, 1. The CmpRegion cases go in `CMP_TRACE_CASES` and `SPLIT_CASES` from the start (Task 2 tests them).
- [ ] **Step 2: Failing test** `reg_trace_matches_the_c_library` over `TRACE_CASES` (support flag and every position), plus `an_empty_dist_only_answers_support`.
- [ ] **Step 3: Implement** the base (`region.c:11601`) and the overrides: `box.c:3186`, `circle.c:1544`, `ellipse.c:1631`, `interval.c:2907`, `polygon.c:4820`, `moc.c:8651`.
- [ ] **Step 4: Pass; commit** `Add Region::reg_trace, tracing a Region's boundary as C does`.

### Task 2: CmpRegion's `reg_trace` and `reg_split`

**Files:**
- Modify: `crates/starlink-ast/src/regions/cmpregion.rs`, `regions/mod.rs` (`reg_split` with its base)
- Test: `tests/test_region_trace.rs`

**Interfaces:**
- Consumes: Task 1's `reg_trace`.
- Produces: `Region::reg_split`; CmpRegion's `reg_trace`.

- [ ] **Step 1: Failing tests** `cmpregion_reg_trace_matches_the_c_library` over `CMP_TRACE_CASES` (AND, OR and XOR of a Circle and a Box; nested; a negated component; disjoint and overlapping parts; a component without `RegTrace`, Review Focus 5) and `reg_split_matches_the_c_library` over `SPLIT_CASES` (number of parts and each part's Native dump, byte for byte).
- [ ] **Step 2: Implement** `RegSplit` (`cmpregion.c:2472`, and the base `region.c:8154`) and `RegTrace` (`cmpregion.c:2695`) with `SetBreakInfo` (`cmpregion.c:3185`) computed on demand; quirks entry for the doubled `region1` test, and a ruling for what the port returns where C reads unfilled arrays.
- [ ] **Step 3: Pass; commit** `Add CmpRegion's reg_trace and reg_split`.

### Task 3: The border of a Plot whose current Frame is a Region

**Files:**
- Modify: `crates/starlink-ast/src/plot/border.rs`, `plot/curve.rs` (a `CurveMap::Region` variant for `Map5`)
- Modify: `tools/oracle_fixtures/plot_border_probe.c` (regenerated `tests/plot_border_oracle/cases.rs`)
- Test: `tests/test_plot_draw.rs`
- Modify: `PLAN.md` (RT-TRACE and RS-SPLIT rows, the stage-4 Region note, §7 Plot row), `docs/audits/region-class-coverage-audit.md`, the spec (stage 5 done), `CHANGELOG.md`

**Interfaces:**
- Consumes: `reg_trace`, `reg_split`.

- [ ] **Step 1: Probe cases** in `plot_border_probe.c`: border and grid logs and SVGs for Plots whose current Frame is a Circle on a SkyFrame, a Box on a 2-D Frame, an OR of two disjoint Circles, a negated Circle, and a Region without `RegTrace` (a Prism of two Intervals, which takes `Boundary`'s Region branch: `astMapRegion` and `astGetRegionBounds`).
- [ ] **Step 2: Failing test** `region_borders_match_the_c_library` over those cases (the existing `borders_match_the_c_library` picks up the border ones), and remove the stage-4 test expectation of `ErrorCode::Inter`.
- [ ] **Step 3: Implement** `DrawRegion` (`plot.c:9583`), `Map5` (`plot.c:22536`) and `Boundary`'s Region branch (`plot.c:4870`), dropping the `Inter` error from `border.rs` and the `draw_border`/`draw_grid` docs.
- [ ] **Step 4: Pass; full suite, clippy, fmt, docs; records** (PLAN rows closed with the tests that pin them, audit entries marked done, spec stage 5 done with this plan's path, CHANGELOG `Changed`/`Fixed` entry: a Plot whose current Frame is a Region draws its border and grid).
- [ ] **Step 5: Commit** `Draw the border of a Region current Frame as C does`.

# `astFindFrame` Port — Resume Document

> **Purpose:** Pick up the `astFindFrame` / `astMatch` / `astSubFrame` port mid-stream in a fresh context. The full plan is at `docs/superpower/plans/2026-06-05-find-frame-and-match.md`. This document summarises what's done, what's next, and concrete starting points.

## Snapshot (state at end of session)

- Branch: `tickets/DM-54793`
- Last commit landing find_frame work: `3d93d28 specmap: mirror C float-cast in LsrkVel/LsrdVel/LgVel/GalVel`
- Test counts: **907 lib tests passing**, **14/14 test_find_frame**, **176/192 test_wcsconv** (class-ait, class-arc, class-sin all pass; remaining 16 fail on unrelated issues — CTYPE selection, keyword ordering).
- `cargo fmt`, `cargo clippy --workspace -- -D warnings` clean.

Tasks 8 (CmpFrame), 9 (FluxFrame), 9b (SpecFluxFrame), 9c (TimeFrame), 10 (FrameSet), 11 (CLASS encoder — AddEncodingFrame + spectral CRVAL conversion + AzEl emission infrastructure), 12 (SkyFrame LAST), 12b (diurnal aberration), and 12c (SpecMap LSRK float-cast) all complete. Remaining: Task 13 (Region overrides), Task 14 (audit existing find_frame consumers), and the unrelated wcsconv CLASS residuals.

## Tasks done

| # | Task | Commit | C ref |
|---|------|--------|-------|
| 1 | Frame match-control attrs (MinAxes/MaxAxes/MatchEnd/PreserveAxes/Permute) | `dd9b084` | frame.c attrs |
| 2 | Scaffold match_template / sub_frame / find_frame_template + MatchResult | `c6d818e` | frame.c:7073/10838/3735 trampoline |
| 3 (default SubFrame) + 4 (default Match) + 5 (BasicFrame integration tests) | `6dceb3c` | frame.c:10838 + 7073 |
| 6 | SkyFrame::Match + SubFrame + Overlay | `359ea82` | skyframe.c:6604 + 9869 + Overlay |
| 7 | SpecFrame::Match + SubFrame + Overlay | `4edf007` | specframe.c:2998 + 4705 + Overlay |
| 7b | DSBSpecFrame::Match + SubFrame + Overlay | `faddba8` | dsbspecframe.c:976 + 1470 + 1154 |
| 8a | CmpFrame::Overlay + apply_template_overlay dispatcher | `55560b2` | cmpframe.c:169-432 |
| 8b | GenAxisSelection helper | `f4977ec` | cmpframe.c:204 |
| 8c | AddExtraAxes / RenumberAxes / GoodPerm / PartMatch | `b27562e` | cmpframe.c:929 / 7651 / 4562 / 6591 |
| 8d | ComponentMatch | `94cf2e7` | cmpframe.c:1972-2321 |
| 8e | CmpFrame::Match orchestration | `ff7edb8` | cmpframe.c:5051 |
| 8f | CmpFrame::SubFrame + Domain/Title accessors | `f5c3d58` | cmpframe.c:9184 |
| 8g | CmpFrame integration tests | `ca2c0ef` | tests |
| 10 | FrameSet::find_frame_template override | `29d5973` | frameset.c:3790 |
| 9 | FluxFrame::Match + SubFrame + Overlay | `838f1f6` | fluxframe.c |
| 9c | TimeFrame::Match + SubFrame + Overlay | `0dba1b5` | timeframe.c |
| 9b | SpecFluxFrame::Match + SubFrame + Overlay | `db2ab1a` | specfluxframe.c:1216 / 1428 |
| (extra) | primary_frame_walk + Sky/Spec/Flux/Time CmpFrame walking | `b94e6f6` | astPrimaryFrame |
| 11a | AddEncodingFrame for FITS-CLASS | `a735559` | fitschan.c:2435 |
| 11b | CLASS spectral CRVAL conversion via SpecFrame find_frame | `a9b1053` | fitschan.c:6843-6898 |
| 11c | CLASS AZ/EL emission via SkyFrame find_frame | `dbf0fa6` | fitschan.c:6901-6944 |
| 12  | SkyFrame LAST via TimeFrame TDB→LAST | `848731e` | skyframe.c:1218 (CalcLAST) |
| 12b | Diurnal aberration in AzEl SLA chain | `600c39f` | skyframe.c:3735 (GetDiurab) + 6181/6292 |
| 12c | Mirror C float-cast in SpecMap LSRK helpers | `3d93d28` | specmap.c:1764, 1768, 1980, 2030, 2080 |

Each commit is a focused, self-contained port. Tests added per class.

## Tasks remaining (in priority order)

### Task 8 — CmpFrame::Match + SubFrame (Overlay + helpers landed)

**Landed already (2026-06-05):**
- `apply_template_overlay()` dispatcher in `crates/ast-core/src/frames/find_frame.rs` — single template-side polymorphism point. CmpFrame templates route through C's `CmpFrame::Overlay` algorithm; everything else falls through to `result.overlay_template()`. `default_sub_frame()` now uses this dispatcher, so SkyFrame/SpecFrame/BasicFrame results inherit attributes from CmpFrame templates correctly.
- `gen_axis_selection()`, `add_extra_axes()`, `renumber_axes()`, `good_perm()`, `part_match()` — all in `cmpframe.rs` as `pub(crate)` helpers, gated `#[allow(dead_code)]` until `CmpFrame::Match` lands.
- `CmpFrame::perm()` public accessor (mirrors `astGetPerm`).
- 16 cmpframe tests (overlay + helpers + 2 part_match smoke tests).

**Important caveat:** `part_match` calls `target.sub_frame(None, axes, ...)` to extract sub-frames from the target. For a non-CmpFrame target this works via the trait default. **For a CmpFrame target it returns None today** (CmpFrame inherits the trait default `Ok(None)`), which is why the part_match tests currently use BasicFrame targets only. CmpFrame::sub_frame is the missing piece.

**C source still to port:**
- `Match` lines 5051-5572 (~520 lines).
- `SubFrame` lines 9184-9706 (~520 lines).
- `ComponentMatch` lines 1972-2526 (~555 lines).
- C dumps still at `/tmp/cmpframe_*.c`.

### Task 8 (legacy task notes for reference) — CmpFrame::Match + SubFrame + Overlay (largest)

**C source:** `reference/starlink-ast/src/cmpframe.c` (extracted to `/tmp/cmpframe_*.c`):
- `Match` lines 5051-5572 (~520 lines).
- `SubFrame` lines 9184-9706 (~520 lines).
- `Overlay` lines 169-432 (~265 lines).
- Helpers: `PartMatch` (lines 6591-7316, ~720 lines), `GenAxisSelection` (lines 3377-3667, ~290 lines), `ComponentMatch` (lines 1972-2526, ~555 lines).

**Total C surface to port: ~2870 lines.** This is the single biggest task in the plan. **Realistic estimate: a full session on its own.**

**Structure of the C `Match`:**
1. Compute `maxax`/`minax` (template-level) and `maxax1`/`maxax2`, `minax1`/`minax2` (per-component bounds). Mirrors cmpframe.c:5215-5239.
2. Validate axis-count and Domain (cmpframe.c:5244-5275).
3. Loop over every possible `(naxes1, naxes2)` partitioning of target axes between the two component frames (cmpframe.c:5309-5340). Outer loop is `naxes1` from `naxes_max` down to `naxes_min`. Inner loop iterates over every choice of which `naxes1` target axes go to component 1 — `GenAxisSelection` (lines 3377-3667) generates these in lex order.
4. For each partitioning, call `PartMatch` (lines 6591-7316) which delegates to each component's `Match` and combines the results.
5. If the template's `Permute` attribute is false, reject any match that involves an axis permutation (cmpframe.c:5343-5388).
6. If no partitioning works, try `ComponentMatch` (lines 1972-2526) — match the *whole* target against just one component of the template (used when the target Frame is itself a non-CmpFrame).

**Structure of the C `SubFrame`:**
- Similar branching: target is CmpFrame (parallel decompose) vs target is non-CmpFrame.
- Builds the result Frame as a CmpFrame snapshot or single-component snapshot.
- Builds the Mapping via parallel CmpMap composition.

**Structure of `Overlay` (cmpframe.c:169):**
- Two paths: result is CmpFrame → per-component overlay (preserving component boundary); else fall-back per-axis split.

**Approach for Task 8 in a fresh session:**

1. **Read the C in 200-line chunks**, taking notes. Key files saved at `/tmp/cmpframe_match.c`, `/tmp/cmpframe_partmatch.c`, `/tmp/cmpframe_genax.c`, `/tmp/cmpframe_subframe.c`, `/tmp/cmpframe_overlay.c`.
2. **Port helpers first** — `GenAxisSelection` is a pure utility (lex-order subset enumeration); port it standalone and unit-test. Then `PartMatch` (depends on per-component Match dispatching). Then `ComponentMatch`.
3. **Then port `Match`** — its body is mostly orchestration over the helpers, plus the Permute-axis-monotonicity check.
4. **Then `SubFrame`** — similar shape to Match's case branches but builds a Mapping + result Frame.
5. **Then `Overlay`** — straightforward two-path implementation.
6. **Tests** at the end of `crates/ast-core/src/frames/cmpframe.rs::mod tests`:
   - `cmpframe_overlay_template_copies_domain`
   - `cmpframe_sub_frame_identity` (target_axes = [0..n-1] → UnitMap)
   - `cmpframe_match_against_cmpframe_template` (CmpFrame ↔ CmpFrame parallel match)
   - `cmpframe_match_against_skyframe_template_with_max_axes` (SkyFrame template with `set_max_axes(3); set_min_axes(2)` matches a `CmpFrame(SkyFrame, SpecFrame)` target — exercises ComponentMatch).
   - `cmpframe_match_with_default_max_axes_fails` (same target, default MaxAxes=2 → None).
7. **Important note:** the parent agent killed two CmpFrame subagents earlier in this thread because they stalled in research. **Do this inline, not via subagent.** Subagents on tasks this large stall before writing code.

**File scope for Task 8:** `crates/ast-core/src/frames/cmpframe.rs` only. Helpers stay private to that file.

**An important Rust-vs-C detail:** the existing Rust `CmpFrame` has a `perm: Vec<usize>` field that tracks an external→internal axis permutation, set non-trivially by `perm_axes`. The C port stores this via `astGetPerm`. The Match/SubFrame logic must respect this — see how `route_axis(axis)` (at line 119 in cmpframe.rs) maps external 1-based axis to internal child + local axis.

**Helpers needed from FrameSet:** `mapping_base_to_current()` does **not** exist on FrameSet today; it has `mapping(FrameRef::Base, FrameRef::Current)` or similar. Check `crates/ast-core/src/frames/frameset.rs` for the available mapping-extraction API before depending on a method name. The `find_frame_template` override below assumed `mapping_base_to_current()` — that was a my-mistake; use whatever the actual API is.

**Override `CmpFrame::find_frame_template`** (probably the simplest route to making `template = SkyFrame, target = CmpFrame` work even before full PartMatch lands): walk components and call `template.find_frame_template(component, domainlist)` for each, prefixing the component's base→full PermMap. This is **not what C does** structurally (C has the full PartMatch path inside SkyFrame::Match via astPrimaryFrame), but it gives the same answer for typical fixtures and unblocks Task 11. Document it as an interim shortcut with a TODO referencing skyframe.c:6759-6803 / specframe.c:3143-3155 for the proper fix.

### Task 9 — FluxFrame::Match + SubFrame + Overlay

**C source:** `reference/starlink-ast/src/fluxframe.c` — search for `static int Match`, `static int SubFrame`, `static void Overlay`. FluxFrame is 1-axis; structure mirrors SpecFrame's.

**Approach:** Look at `crates/ast-core/src/frames/fluxframe.rs` for existing storage. Add `*_set` flags + `test_*` accessors for any Flux-specific attributes (System, AlignSystem, RefValue, etc.) following the SpecFrame template (commit `4edf007`). Wire `match_template`, `sub_frame`, `overlay_template`, `pick_axes_snapshot` through the existing `convert()` infrastructure.

### Task 9b — SpecFluxFrame::Match + SubFrame + Overlay

**C source:** `reference/starlink-ast/src/specfluxframe.c`. SpecFluxFrame is a 2-axis specialised CmpFrame (1 spectral + 1 flux). Defers most work to the underlying SpecFrame and FluxFrame.

### Task 9c — TimeFrame::Match + SubFrame + Overlay

**C source:** `reference/starlink-ast/src/timeframe.c`. TimeFrame is 1-axis. Same shape as SpecFrame.

### Task 10 — FrameSet::find_frame_template override

**C source:** `reference/starlink-ast/src/frameset.c:3790-4030`.

The C override iterates the FrameSet's frames in priority order (current → base → others), calls `astFindFrame(frame, template, ...)` on each, and prefixes the FrameSet's base→that-frame mapping when a match is found.

**Algorithm:**
```rust
fn find_frame_template(&self, template: &dyn Frame, domainlist: &str)
    -> AstResult<Option<FrameSet>>
{
    let target_nframe = self.nframe();
    let target_current = self.current_idx() + 1; // 1-based
    let target_base = self.base_idx() + 1;

    // Search order: current first, then base, then others.
    let mut order: Vec<usize> = vec![target_current];
    if target_base != target_current { order.push(target_base); }
    for i in 1..=target_nframe {
        if i != target_current && i != target_base { order.push(i); }
    }

    for domain_field in canonicalise_domainlist(domainlist) {
        for &target_idx in &order {
            let selected = self.frame(FrameRef::Index(target_idx))?;
            let found = selected.find_frame_template(template, &domain_field)?;
            if let Some(found) = found {
                let map_base_to_current = /* extract from `found` */;
                let result_frame = found.current().snapshot();
                let prefix = self.mapping(FrameRef::Base, FrameRef::Index(target_idx))?;
                let combined: DynMapping = Box::new(CmpMap::new(prefix, map_base_to_current,
                                                                Composition::Series)?);
                let combined_simplified = combined.simplified()?;
                let mut result_fs = FrameSet::new(self.base().snapshot());
                result_fs.add_frame(FrameRef::Base, combined_simplified, result_frame)?;
                return Ok(Some(result_fs));
            }
        }
    }
    Ok(None)
}
```

`canonicalise_domainlist` is a small helper that splits on commas, strips whitespace, uppercases, and yields each field.

### Task 5b — Port `ast_tester/testconvert.c` (51 assertions)

**C source:** `reference/starlink-ast/ast_tester/testconvert.c` (237 lines, 51 numbered error assertions).

**Output:** `crates/ast-core/tests/test_convert_findframe.rs`. Mirror each `astError(AST__INTER, "Error N\n")` site with a Rust `assert!(...)` that names the same error number in its message.

The test will fail many assertions until Tasks 8 and 10 land — mark `#[ignore]` initially, remove the ignore once enough of the surface works.

### Task 11 — Wire find_frame into FITS-CLASS encoder

**Files:** `crates/ast-core/src/fitschan/wcs_write/mod.rs` (add `encode_class` entry), `crates/ast-core/src/fitschan/enc_class.rs` (port `CLASSFromStore`), `crates/ast-core/src/fitschan/mod.rs` (switch FitsClass dispatch).

**C source:** `reference/starlink-ast/src/fitschan.c:6324-7300` (~975 lines; full `CLASSFromStore`). Three sub-tasks per the original plan: spatial-axis adjust, spectral CRVAL conversion via SpecFrame, AZIMUTH/ELEVATIO via SkyFrame Az/El.

This is what unblocks the 10 CLASS-bucket fixtures in the wcsconv suite.

### Task 12 — CLASS edge-case fixtures

`skyspec3d_class_fits_class`, `jan_fits_class_ast`, `jan_fits_class_rt`, plus any leftover after Task 11.

### Task 13 — Region overrides

`reference/starlink-ast/src/region.c` Match/SubFrame overrides. Region is treated as a Frame for matching purposes (e.g. aligning two NDFs whose WCS frames are Regions).

### Task 14 — Audit existing find_frame consumers

`grep -rn "find_frame" crates/` — list every current consumer; verify the new behaviour matches C; add regression tests covering the wcsconv fixtures that exercise virtual-system matching (e.g. AzEl).

## Concrete starting commands for Task 8

```bash
# Source dumps already saved at:
ls -l /tmp/cmpframe_*.c
# Read the algorithm:
less /tmp/cmpframe_match.c
less /tmp/cmpframe_partmatch.c
less /tmp/cmpframe_genax.c
less /tmp/cmpframe_subframe.c
less /tmp/cmpframe_overlay.c

# Existing Rust:
less crates/ast-core/src/frames/cmpframe.rs

# Sanity check baselines:
cargo test -p ast-core --lib 2>&1 | tail -3
cargo test -p ast-core --test test_find_frame 2>&1 | tail -3
cargo test -p ast-core --test test_wcsconv 2>&1 | tail -3
```

After each substantive code change, run the full test triple plus `cargo clippy --workspace -- -D warnings` and `cargo fmt`. Commit per discrete fix.

## Lessons learned this session

- **Subagents stall on tasks > ~500 lines of C.** Three subagent dispatches stalled during research mode and never produced code (CLASS encoder twice, CmpFrame once, DSBSpecFrame once). Doing the port inline succeeded for SkyFrame/SpecFrame (each ~270-350 lines of C). For tasks bigger than that, port inline.
- **Per-class `*_set` flags** for every attribute Overlay copies — both SkyFrame (commit `359ea82`) and SpecFrame (commit `4edf007`) needed these added (system_set, neglon_set, sky_ref_set, etc). DSBSpecFrame inherited from SpecFrame and didn't need its own flags. CmpFrame and the rest will likely need similar additions.
- **`find_frame_template` default body** uses `self.snapshot()` to avoid the `&Self` → `&dyn Frame` coercion problem. Per-class overrides only need to call `find_frame::default_find_frame(self.snapshot(), template, domainlist)` when defaulting; concrete overrides build the FrameSet themselves.
- **Dispatch model:** in C, `astMatch(template, target)` is virtual on the *template*. So `SkyFrame::Match` is the entry when template is SkyFrame. Walking into a CmpFrame *target* is done inside SkyFrame::Match via `astPrimaryFrame`. For Rust we deferred that walk to "Task 8 will provide the infrastructure" — but in practice, the cleanest way to handle it without modifying SkyFrame/SpecFrame is to override `CmpFrame::find_frame_template` to walk components when the template's match returned None against the whole CmpFrame. Document this as an interim shortcut and revisit with the proper SkyFrame/SpecFrame extension later.

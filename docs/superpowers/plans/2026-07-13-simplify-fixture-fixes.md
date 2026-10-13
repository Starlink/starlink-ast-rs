# Simplify Fixture Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or subagent-driven-development) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make all 64 newly-imported `astSimplify` fixtures (from the 50c2b21b sync) pass in `crates/ast-core/tests/test_simplify_regression.rs` — the core goal of the port is that these fixtures pass.

**Architecture:** Fixes grouped by root cause discovered in a four-bucket parallel investigation (region, selectormap/switchmap, general-mapping-merge, spec/sla/time). Ordered so shared prerequisites and foundational simplifier bugs land first; downstream normal-form fixtures are re-measured after the foundations. Each root cause is verified against the C oracle at `reference/starlink-ast/build-ast/` and ported by mirroring C control flow (no heuristics, no shortcuts).

**Tech Stack:** Rust 2024, `cargo test -p ast-core`, reference C `libast` (cmake) for oracle reproducers.

**Prior context:** This is the Task 13 continuation of `docs/superpowers/plans/2026-07-13-upstream-quirk-sync.md`. Group A (quirk fixes) and Group B (fixture import + manifest + harness) are already committed. All 64 failures are newly-imported fixtures — zero regressions of pre-existing tests.

## Global Constraints

- Rust 2024 edition; MSRV in `rust-toolchain.toml`.
- `cargo fmt` + `cargo clippy --workspace -- -D warnings` clean before every commit.
- `#![warn(missing_docs)]`; CI uses `RUSTFLAGS="-D warnings"`.
- `BAD_VALUE` = `-f64::MAX`; never `NaN` as an output sentinel. 1-based axis indices.
- Port C control flow directly; no heuristics/shortcuts. Re-derive each fix from the actual C source, verified with an oracle reproducer.
- Oracle reproducer recipe: link `reference/starlink-ast/build-ast/libast.so` + `libast_grf_5.6.a libast_grf_3.2.a libast_grf_2.0.a libast_grf3d.a libast_err.a`; includes `-I reference/starlink-ast -I reference/starlink-ast/build-ast`; run with `LD_LIBRARY_PATH=<abs>/reference/starlink-ast/build-ast`.
- Commit message footer:
  ```
  Generated with AI

  Co-Authored-By: SLAC AI
  ```
- Never push; never post to remote.

## Two test consumers (both must pass)

The `simplify_fixtures/` corpus feeds **two** independent tests — the fix
tasks are "done" only when both are green:

1. `test_simplify_regression.rs::simplify_regression` — manifest-driven;
   checks equal()/transform/serialization of the *simplified* result.
2. `test_serialisation.rs::native_byte_exact_round_trip_simplify_fixtures`
   (and `test_native_roundtrip_all.rs`) — **auto-discovers** every
   `.map`/`.simp` and does a pure read→write byte-exact round-trip (no
   simplify). Importing the corpus exposed ~48 read→write failures here
   that the original investigation (test #1 only) missed. Same root
   causes (SpecMap arg comments, region dumps, CmpMap structure).

Unported-class fixtures (IntraMap/XphMap/StcResourceProfile) are staged in
`simplify_fixtures_unsupported/<class>/`, excluded from both tests. After
every fix task, run BOTH tests and record the delta for each.

## Failure inventory (post P1+S1)

- **Parse-skips:** 0 (P1 cleared them; unported classes staged out).
- **test #1:** 39 equal / 9 transform / 36 serialization (overlapping).
- **test #2:** ~48 byte-exact round-trip failures across ~34 fixtures.
- Historical (pre-P1): 17 parse-skips, 38 equal, 6 transform, 38 serial.

## Root-cause map (from investigation)

| ID | Root cause | Fixtures | Kind | Size |
|----|-----------|----------|------|------|
| **P1** | Dummy-FrameSet loader fallback + compound re-association | 15 parse-skips across region/selectormap/cap_zoommap_04 | parser/serial | M |
| **S1** | `IsSimple` used as merge barrier (should be `Frozen`) | timemap_05/07, slamap_05, +contrib to slamap_02/03, specmap_01 | simplifier foundation | M |
| **S2** | Lone inverted Spec/Sla/Time component not normalized to forward | specmap_05/07/08/10/13, slamap_03 | simplifier | M |
| **S3** | SpecMap `arg_descriptions()` comment text | specmap_invert_normalize, specmap_06, specmap_03 | serialization | S |
| **M1** | WinMap→ShiftMap final narrowing for residual leaves | winmap_01, unitmap_01, unitnormmap_winmap_inv_refused, unitnormmap_fwd_winmap_refused, neg_winmap_perm_nonbidir | serialization normal-form | S-M |
| **M2** | LutMap constant-table guard + PermMap bad-vs-constant encoding | neg_lut_constant, cap_permmap_02 | missing guard / normal-form | S-M |
| **R1** | Cross-type parallel region merge (Interval/Box + NullRegion/PointList) | interval_nullregion_merge, interval_pointlist_merge, box_nullregion_merge | simplifier | M-L |
| **R2** | NullRegion/PointList have no `try_merge` (same-class parallel) | nullregion_parallel_merge_pair, pointlist_parallel_merge_pair, nullregion_merge_tail | simplifier | S-M |
| **R3** | Remapped Interval simplify uses a point-transform shortcut (breaks under shear) | interval_remapped_simplify, interval_halfinf_simplify | simplifier (de-heuristic) | M |
| **W1** | SwitchMap `equal` ignores invert-resolution; inverted forward transform → all-BAD | switchmap_inv_invonly, switchmap_inv_fwdonly | class-method | M |
| **B1** | WinMap↔Matrix swap normal-form ordering | winmap_swap_to_zoom, cap_zoommap_01 | normal-form | M |
| **E1** | CmpMap invert-flag placement + shift-through-matrix fusion; `ReSimp`/`AlSimp` restricted-simplify unimplemented | cmpmap_01-04, matrixmap_02, zoommap_01 | normal-form / missing feature | M-L |
| **C1** | WcsMap has no `map_merge`: self-inverse cancel + WcsMap↔PermMap const-swap | matrixmap_01, winmap_02, wcsmap_01 | missing merge | L |
| **D1** | Residual structural grouping (re-assess after S1/S2/E1) | specmap_01/02/12, cap_matrixmap_02 residue | normal-form | reassess |

## Sequencing rationale

1. **P1 first** — unblocks 15 parse-skipped fixtures across three buckets; several R/selectormap merge fixtures can't even be reached until frame-less child regions load.
2. **S1 next** — foundational simplifier bug (IsSimple-as-barrier) that changes the merge sequence for many fixtures; fixing it first prevents re-deriving downstream normal forms twice.
3. **S2, S3, M1, M2, W1** — independent, well-scoped, high-yield.
4. **R1, R2, R3** — region-merge logic (after P1).
5. **B1, E1** — normal-form ordering (after S1).
6. **C1** — largest missing piece (WcsMap::map_merge).
7. **D1 last** — re-measure; several likely resolve once S1/S2/E1 land.

After every task: run the full simplify regression and record the pass-count delta; run the full `cargo test -p ast-core` to guard against regressions.

---

## Task P1: Dummy-FrameSet loader fallback + compound re-association

**Root cause:** C `astLoadRegion` (region.c:13593-13603) builds a default dummy FrameSet (`astFrame(naxpt||1)`, ident `ASTREGION-DUMMY`, `regionfs=0`) when a nested region is dumped without `Frm`/`FrmSet`; the parent compound loader (SelectorMap:1788-1795, and Prism/CmpRegion/Box-uncertainty) re-associates the real frame via `astSetRegFS`/`astRegDummyFS`. Rust's `read_region_base` errors instead.

**Files:**
- Modify: `crates/ast-core/src/serial/mod.rs` (`read_region_base` / `read_region_data`, ~lines 716-753)
- Modify: `crates/ast-core/src/regions/mod.rs` (Region model — add a dummy-FS marker + `set_region_fs`-equivalent)
- Modify: `crates/ast-core/src/mappings/selectormap.rs` (`from_node`, ~393-431)
- Modify: region/compound loaders that embed child regions: `crates/ast-core/src/regions/{prism,cmpregion}.rs`, and Box/Interval/etc. uncertainty (`Unc =`) deserialization
- Test: `crates/ast-core/tests/test_simplify_regression.rs` (driver)

**Interfaces:**
- Produces: a way to load a `Region` whose node has neither `Frm` nor `FrmSet` (synthesize a dummy `BasicFrame` of `naxpt` axes, flagged dummy), and a `set_region_frame`-style internal method the compound loaders call to overlay the real frame.

- [ ] **Step 1: Reproduce the exact parse errors**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | sed -n '/parse-skipped/,/equal() failures/p'
```
Expected: lists the 17 parse-skips. Confirm (via a scratch test, deleted after) that region-containing ones fail with `Region: missing Frm and FrmSet`.

- [ ] **Step 2: Read the C dummy-FS mechanism**

Read `reference/starlink-ast/src/region.c:13562-13603` (Load), `:8814-8876` (`RegDummyFS`), `:10674+` (`SetRegFS`), and `reference/starlink-ast/src/selectormap.c:1775-1804` (re-association loop). Note: `nax = naxpt ? naxpt : 1`; dummy FS is tagged with ident `DUMMY_FS` (`ASTREGION-DUMMY`), `regionfs=0`.

- [ ] **Step 3: Add a dummy-FS representation to the Region model**

In `crates/ast-core/src/regions/mod.rs` (or the shared region base struct), add an internal flag marking a region as carrying a dummy FrameSet, plus an internal `set_region_fs(&mut self, fs)` that replaces the dummy frame with the real one and clears the flag. Mirror C's `astRegDummyFS`/`astSetRegFS` semantics. Keep it `pub(crate)` — not public API.

- [ ] **Step 4: Make `read_region_base` synthesize the dummy FS instead of erroring**

In `crates/ast-core/src/serial/mod.rs`, in the branch where neither `FrmSet` nor `Frm` is present, build a default `BasicFrame` with `naxpt` axes (from the loaded PointSet's ncoord, falling back to 1), wrap it as the region's FrameSet, and set the dummy flag — rather than returning `Err(Lderr)`.

- [ ] **Step 5: Re-associate in compound loaders**

In `selectormap.rs::from_node` (and Prism/CmpRegion loaders that embed multiple regions), after loading child regions: capture the first fully-framed region's FrameSet and, for each later region reporting dummy-FS, call `set_region_fs` with it. Mirror selectormap.c:1788-1795.

- [ ] **Step 6: Verify the parse-skips clear**

Run:
```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep -E "Simplify regression:|parse"
```
Expected: the parse-skip count drops (target 0 for region/selectormap fixtures; the 16 IntraMap/XphMap remain BLOCKED in the manifest and don't count). Some of these fixtures may now surface as equal/transform failures handled by R1/R2/W1 — that's expected.

- [ ] **Step 7: Full suite + fmt + clippy + commit**

```bash
cargo test -p ast-core 2>&1 | grep -cE "test result: FAILED"   # expect no NEW non-simplify failures
cargo fmt && cargo clippy --workspace -- -D warnings
git add -A && git commit  # message: "serial: load frame-less child regions via dummy FrameSet (match C)"
```

---

## Task S1: Separate `IsSimple` from the `Frozen` merge barrier

**Root cause:** Rust uses `has_been_simplified()` (C `IsSimple`) as a hard merge barrier in `default_map_merge` and per-class merges. In C these are distinct bits: `IsSimple` only short-circuits top-level `astSimplify_` (mapping.c:24740); the merge barrier is `Frozen`, cleared on every component at merge-loop entry (cmpmap.c:3538) and set only when `PatternCheck` detects oscillation (cmpmap.c:3608).

**Files:**
- Modify: `crates/ast-core/src/mappings/simplify.rs` (merge guards at ~483, 546-557, 695; the merge loop / `ctx` bookkeeping)
- Modify: `crates/ast-core/src/mappings/mod.rs` or wherever the mapping carries the `has_been_simplified`/frozen state
- Test: rewrite `has_been_simplified_prevents_merge` and the second-mapping variant in `test_simplify_regression.rs:380-427` (they encode the wrong assumption; re-derive from the oracle)

**Interfaces:**
- Produces: a `frozen` concept in the simplify merge context, distinct from `has_been_simplified()`. `frozen` starts clear each merge-loop pass and is set only by the existing oscillation/pattern-detection path.

- [ ] **Step 1: Read the C Frozen mechanism**

Read `reference/starlink-ast/src/mapping.h:409-428` (IsSimple vs Frozen macros), `mapping.c:16851-16862` (`astClearFrozen` + merge loop), `mapping.c:24740` (IsSimple top-level short-circuit only), `cmpmap.c:3527-3620` (clear-at-entry, set-on-oscillation).

- [ ] **Step 2: Oracle-confirm a component's IsSimp does not block merge**

Build a reproducer: `CmpMap(ZoomMap[IsSimp=1,z=3] ; ZoomMap[z=2])` → expect single `ZoomMap`; `CmpMap(SlaMap[IsSimp=1,FK5HZ] ; SlaMap[HFK5Z])` → expect `UnitMap`. Print `astShow` of the simplified result.

- [ ] **Step 3: Introduce `frozen` state, cleared at merge-loop entry**

In `simplify.rs`, add a per-entry `frozen` flag in the merge context, cleared when the merge loop starts iterating over the component list (mirror cmpmap.c:3538). Set it only where the existing oscillation/pattern-check detects a loop (mirror cmpmap.c:3608).

- [ ] **Step 4: Replace the `has_been_simplified()` merge guards with `frozen`**

At `simplify.rs:483, 546, 550, 554, 557, 695` (and any analogous per-class guard), replace `has_been_simplified()` with the new `frozen` check. Leave the *top-level* `simplify()` short-circuit (the one that returns early when the whole object `has_been_simplified()`) intact — that mirrors C IsSimple correctly.

- [ ] **Step 5: Re-derive the two unit tests from the oracle**

Rewrite `has_been_simplified_prevents_merge` and `..second_mapping..` (test_simplify_regression.rs:380-427) to assert the *correct* oracle behaviour (a component's IsSimp does not block merging; only oscillation-driven Frozen does). Use the Step-2 oracle results.

- [ ] **Step 6: Run affected fixtures + full suite**

```bash
cargo test -p ast-core --test test_simplify_regression -- --nocapture 2>&1 | grep "Simplify regression:"
cargo test -p ast-core 2>&1 | grep -cE "test result: FAILED"
```
Expected: timemap_05/07, slamap_05 clear; overall simplify pass-count rises. No non-simplify regressions.

- [ ] **Step 7: fmt + clippy + commit** ("simplify: use Frozen (not IsSimple) as the component merge barrier (match C)")

---

## Task S2: Normalize a lone inverted Spec/Sla/Time component to forward

**Root cause:** C `SpecMap::MapMerge` (specmap.c:2089, condition d at :2570) replaces an inverted SpecMap component with a forward one (steps reversed, codes swapped via `SWAP_CODES`), clearing the invert flag — even without a same-class neighbour. Rust's SpecMap has only `try_merge` (needs a SpecMap neighbour) + `simplified` (self), so a lone inverted component survives as `InvA=1` with un-flipped codes. Same for SlaMap/TimeMap.

**Files:**
- Modify: `crates/ast-core/src/mappings/specmap.rs`, `slamap.rs`, `timemap.rs` — add a per-class `map_merge`
- Modify: `crates/ast-core/src/mappings/simplify.rs` — dispatch to the new per-class `map_merge`

**Interfaces:**
- Consumes: existing `normalized_steps` / `sla_normalized_steps` / `time_normalized_steps` helpers.
- Produces: `map_merge` for each of the three classes that (a) gathers an adjacent same-class run and (b) normalizes a lone inverted instance to forward.

- [ ] **Step 1: Read C MapMerge for the three classes**

`specmap.c:2089` (MapMerge), `:2335-2417` (`SWAP_CODES`), `:2570` (condition d); the analogous `slamap.c` / `timemap.c` MapMerge.

- [ ] **Step 2: Oracle-confirm the forward-normalized output**

For `cap_specmap_08`: expect simplified steps `Spec1="WNTOFR" Spec2="FRTOWV"` (forward), not `InvA=1 Spec1="WVTOFR" Spec2="FRTOWN"`. Confirm via reproducer.

- [ ] **Step 3: Implement `SpecMap::map_merge`**

Mirror specmap.c MapMerge: gather adjacent same-class run; when the (possibly lone) component is inverted, rebuild it forward using `normalized_steps(spec, true)` and clear the invert flag. Return the modified list per the merge protocol.

- [ ] **Step 4: Implement `SlaMap::map_merge` and `TimeMap::map_merge`** analogously with their normalized-steps helpers.

- [ ] **Step 5: Dispatch from `simplify.rs`** to the new per-class `map_merge` (alongside the existing zoom/winmap/matrix/cmpmap dispatch).

- [ ] **Step 6: Run affected fixtures + full suite**

Expected: specmap_05/07/08/10/13, slamap_03 clear (equal + serialization). No regressions.

- [ ] **Step 7: fmt + clippy + commit** ("specmap/slamap/timemap: normalize lone inverted component to forward in map_merge (match C)")

---

## Task S3: Fix SpecMap `arg_descriptions()` comment text

**Root cause:** `SpecCvtType::arg_descriptions()` (specmap.rs:204-260) returns wrong description slices for the `UsF2Hl`/`HlF2Us` group (should be 4 entries: Velocity, RA, DEC, correction — specmap.c:1167-1190) and the `ByF2Hl`/`HlF2By` group (should be with the Geocentric UT1-epoch group, not the 2-arg LSRK set — specmap.c:1252-1275). `nargs`/`szargs` are already correct.

**Files:**
- Modify: `crates/ast-core/src/mappings/specmap.rs:204-260` (`arg_descriptions`)

- [ ] **Step 1: Read C `CvtString` cases** `USF2HL`, `GEF2HL`, `BYF2HL` at specmap.c:1167-1275; record the exact comment strings.

- [ ] **Step 2: Fix the US/HLF2US group** to return the 4-entry list `["Velocity offset (m/s)", "RA of source", "DEC of source", "Frequency correction factor"]` (exact C text).

- [ ] **Step 3: Move BY/HLF2BY into the Geocentric group** (`["UT1 epoch of observaton", "RA of source", "DEC of source", "Frequency correction factor"]` — match C's exact spelling incl. "observaton").

- [ ] **Step 4: Run affected fixtures**

Expected: specmap_invert_normalize, specmap_06 clear (serialization); specmap_03 partially.

- [ ] **Step 5: fmt + clippy + commit** ("specmap: correct arg descriptions for US/BY velocity conversions (match C)")

---

## Task M1: WinMap→ShiftMap final narrowing for residual leaves

**Root cause:** C freezes a pure-shift WinMap into a `ShiftMap` as the final deferred step (winmap.c:1817-1857). Rust's `winmap_merge.rs:394-402` does this only when the WinMap is the nominated map; residual pure-shift WinMap leaves inside a settled CmpMap stay `WinMap`. `equal()` tolerates it but serialization emits the wrong class.

**Files:**
- Modify: `crates/ast-core/src/mappings/winmap_merge.rs` (~394-402), possibly `winmap.rs::simplified` (257-303) and `simplify.rs:245-256` (`defer_winmap`)

- [ ] **Step 1: Read C winmap.c:1817-1857** (`isshift` → `astShiftMap`, `result==-1` guard).

- [ ] **Step 2: Reproduce** cap_unitmap_01: expected leaf `Begin ShiftMap` (Sft1=-128 Sft2=-128), actual `Begin WinMap`.

- [ ] **Step 3: Ensure residual pure-shift WinMaps narrow to ShiftMap** at the same point C freezes them — extend the deferred-narrowing path so a pure-shift (all scales==1) WinMap surviving as a leaf becomes a ShiftMap.

- [ ] **Step 4: Run affected fixtures**

Expected: winmap_01, unitmap_01, unitnormmap_winmap_inv_refused, unitnormmap_fwd_winmap_refused, neg_winmap_perm_nonbidir clear. (cap_winmap_01 may also need the IsSimp-flag propagation noted under M1 sub-case — verify; if it's an `IsSimp=1` missing on an inner CmpMap, handle here.)

- [ ] **Step 5: fmt + clippy + commit** ("winmap: narrow residual pure-shift WinMap leaves to ShiftMap (match C)")

---

## Task M2: LutMap constant-table guard + PermMap bad-vs-constant encoding

**Root cause (a):** C refuses to convert a constant LutMap to a WinMap — guard `if(b1 != b2)` at lutmap.c:1150 (first ≠ last LUT value). Rust (lutmap.rs:563-594) computes slope 0 and emits `WinMap(scale=0)`. **(b):** PermMap unpaired output — C encodes as *bad* (0), Rust as a *constant* ref (-1) (perm.rs merge path).

**Files:**
- Modify: `crates/ast-core/src/mappings/lutmap.rs:563-594`
- Modify: `crates/ast-core/src/mappings/perm.rs` (merge/simplify path)

- [ ] **Step 1: LutMap — add the `lut[0] != lut[n-1]` guard** before the WinMap conversion (mirror lutmap.c:1150). Oracle: neg_lut_constant reference == unchanged LutMap.

- [ ] **Step 2: Run neg_lut_constant** — expect it to clear.

- [ ] **Step 3: PermMap — encode an unpaired output as bad (0), not constant (-1)**, mirroring C permmap.c MapMerge. Oracle: cap_permmap_02 expects `Out1 = 0` ("output is bad"), not `Out1 = -1`.

- [ ] **Step 4: Run cap_permmap_02** — expect it to clear.

- [ ] **Step 5: fmt + clippy + commit** ("lutmap/permmap: constant-LUT guard + bad-output encoding (match C)")

---

## Task W1: SwitchMap `equal` invert-resolution + inverted transform

**Root cause:** (a) `SwitchMap::equal` (switchmap.rs:525-559) returns false when invert flags differ, comparing raw stored selectors; C `Equal` (switchmap.c:300-354) compares *effective* (invert-resolved) selectors/routes via `GetSelector`/`GetRoute`. (b) `SwitchMap::transform` inverted-direction logic (switchmap.rs:404-522) yields all-BAD for an inverted map; C applies `astInvert` to the resolved selector/route (switchmap.c:508-512, 597).

**Files:**
- Modify: `crates/ast-core/src/mappings/switchmap.rs` (`equal` 525-559; `transform` 404-522; `route_effective_forward` 296-306; `selector_effective_forward` 308-323; `get_selector`/`get_selector_inv` 279-294)

- [ ] **Step 1: Read C** switchmap.c `Equal` (300-354), `GetRoute` (429-520), `GetSelector` (525-601), `Transform` (1592-1817).

- [ ] **Step 2: Oracle table** for switchmap_inv_invonly (Invert=1, fwd selector ZoomMap z=2, route z=3), forward-transform inputs 1..5: C orig → `0.3333, 0.6667, BAD, BAD, BAD`; C simp → same; astEqual(orig,simp)=1. (Rust currently: orig all-BAD, equal=false.)

- [ ] **Step 3: Rewrite `equal` to compare invert-resolved selectors/routes** (mirror GetSelector/GetRoute), so an inverted map equals its normalized non-inverted twin.

- [ ] **Step 4: Fix the inverted forward-transform** — port C's `GetSelector(map, forward)` / `GetRoute(map, sel)` helper structure (returning a resolved mapping with effective direction, applying `astInvert` when the SwitchMap is inverted) rather than the ad-hoc XOR expressions.

- [ ] **Step 5: Run switchmap_inv_invonly, switchmap_inv_fwdonly** — expect both clear (equal + transform).

- [ ] **Step 6: fmt + clippy + commit** ("switchmap: invert-resolved equal + inverted transform (match C)")

---

## Task R1: Cross-type parallel region merge

**Root cause:** C `MergeInterval` (interval.c:1224-1660) / `MergeBox` (box.c:1658+) merge parallel regions of different classes (Interval+NullRegion negated, Interval+PointList, Box+NullRegion, …) into one higher-D region over a CmpFrame. Rust's `Interval::try_merge`/`BoxRegion::try_merge` only downcast to their own type. (Requires P1 first — inputs carry frame-less `Unc` sub-regions.)

**Files:**
- Modify: `crates/ast-core/src/regions/interval.rs:690-727` (`try_merge`), `box_region.rs:713-749`

- [ ] **Step 1: Read C** interval.c `MergeInterval` (1224-1660, esp. bound construction 1354-1522, uncertainty→Prism 1547-1573), box.c `MergeBox` (1658+).

- [ ] **Step 2: Oracle** the expected merged class for each: interval_nullregion_merge → 2-D Interval; interval_pointlist_merge → PointList; box_nullregion_merge → 2-D Interval.

- [ ] **Step 3: Extend `Interval::try_merge`** to handle Interval+{Box,NullRegion(negated),PointList} per the C type-matrix incl. negation gating, FillFactor product, uncertainty combination, and `astMapRegion` into the CmpFrame.

- [ ] **Step 4: Extend `BoxRegion::try_merge`** for Box+{NullRegion,…} analogously.

- [ ] **Step 5: Run the three fixtures** — expect equal + transform clear.

- [ ] **Step 6: fmt + clippy + commit** ("regions: cross-type parallel Interval/Box merges (match C MergeInterval/MergeBox)")

---

## Task R2: NullRegion / PointList same-class `try_merge`

**Root cause:** NullRegion, PointList, CmpRegion define no `try_merge` → trait default returns None; parallel same-class merges (into a CmpFrame) don't happen. C: nullregion.c `MergeNullRegion` (571+), pointlist.c `MapMerge` (972+).

**Files:**
- Modify: `crates/ast-core/src/regions/nullregion.rs`, `pointlist.rs` (add `try_merge`)

- [ ] **Step 1: Read C** nullregion.c:571+, pointlist.c:972+.
- [ ] **Step 2: Add `NullRegion::try_merge`** (parallel same-class → single higher-D NullRegion over CmpFrame; nullregion_merge_tail is a 3-way nest).
- [ ] **Step 3: Add `PointList::try_merge`** (concatenate point axes into the CmpFrame).
- [ ] **Step 4: Run** nullregion_parallel_merge_pair, pointlist_parallel_merge_pair, nullregion_merge_tail — expect clear.
- [ ] **Step 5: fmt + clippy + commit** ("regions: NullRegion/PointList parallel same-class merge (match C)")

---

## Task R3: Remapped Interval simplify — port C's box-round-trip

**Root cause:** Rust `Interval::simplified` (interval.rs:649-673) transforms the two bound-points through base→current and rebuilds an axis-aligned Interval — wrong under a shear (non-axis-aligned). C `Interval::Simplify` (interval.c:3101-3400) builds a base-frame box (with fill-ins for half-infinite limits), round-trips corners to enclose both, clamps to the original limits, `astMapRegion`s into the current frame, `astSimplify`s that, and substitutes only if it reduced to Box/Interval/NullRegion. This is a "no corner cuts" case — replace the heuristic with the C algorithm.

**Files:**
- Modify: `crates/ast-core/src/regions/interval.rs:649-673` (`simplified`)

- [ ] **Step 1: Read C** interval.c:3101-3400 (esp. 3216-3360: box build, corner round-trip, limit clamp, MapRegion + resimplify + substitution guard).
- [ ] **Step 2: Oracle** interval_remapped_simplify (shear M=[[1,1],[0,1]]): reference == input `.map` (external region unchanged); Rust currently produces all-BAD. interval_halfinf_simplify exercises missing-limit fill-in.
- [ ] **Step 3: Replace the point-transform shortcut** with the base-box round-trip + clamp + MapRegion-and-resimplify path.
- [ ] **Step 4: Run** both fixtures — expect clear.
- [ ] **Step 5: fmt + clippy + commit** ("regions: port Interval::Simplify box-round-trip (replace shear-breaking shortcut)")

---

## Task B1: WinMap↔MatrixMap swap normal-form

**Root cause:** WinMap↔MatrixMap swap ordering differs (winmap.c:1220-1231, WinMat ~2793 + deferred narrowing). winmap_swap_to_zoom, cap_zoommap_01. (After S1.)

**Files:**
- Modify: `crates/ast-core/src/mappings/winmap_merge.rs:176-260` (`try_swap`/`WinMat`), `:421` (`try_direct_merge`)

- [ ] **Step 1: Read C** winmap.c WinMat/WinDiagMat swap (1220-1231, ~2793).
- [ ] **Step 2: Oracle** winmap_swap_to_zoom → `MatrixMap[1,1;0,1] ; WinMap(shift=2,1 scale=2,2)`.
- [ ] **Step 3: Align the swap-to-normal-form ordering** with C (shift to output side of matrix; fold trailing zoom into a WinMap).
- [ ] **Step 4: Run** winmap_swap_to_zoom, cap_zoommap_01 — expect clear (cap_zoommap_01 may also need E1's ReSimp; re-check after E1).
- [ ] **Step 5: fmt + clippy + commit** ("winmap: align WinMap/MatrixMap swap normal form (match C)")

---

## Task E1: CmpMap invert normal-form + shift-through-matrix fusion + ReSimp/AlSimp

**Root cause:** (a) invert-flag placement in the CmpMap tree differs (cmpmap.c:1191 `simpler`); (b) two ShiftMaps flanking a MatrixMap should fuse (matrixmap.c); (c) `ReSimp`/`AlSimp` restricted-simplify attributes are parsed but unimplemented in Rust. cap_cmpmap_01-04, matrixmap_02, zoommap_01.

**Files:**
- Modify: `crates/ast-core/src/mappings/cmpmap.rs:818-841` (`equal`/flatten), `simplify.rs:279-298` (`decomposition_is_simpler`), `matrix_merge.rs` (shift-through-matrix)
- Modify: wherever `ReSimp`/`AlSimp` should gate simplification (currently ignored)

- [ ] **Step 1: Read C** cmpmap.c invert normalization (1191, MapList), matrixmap.c shift-through-matrix, and the `ReSimp`/`AlSimp` (restricted-simplify) semantics in mapping.c/cmpmap.c.
- [ ] **Step 2: Oracle** cap_cmpmap_02 (identical class counts; only InvA/InvB placement + one ShiftMap sign differ); cap_matrixmap_02 (fuse flanking shifts → `MatrixMap ; ShiftMap(-1.1,-0.2)`); cap_zoommap_01 (ReSimp/AlSimp restricted).
- [ ] **Step 3: Align CmpMap invert-flag placement** with C's normal form.
- [ ] **Step 4: Implement shift-through-matrix fusion** (two ShiftMaps flanking a MatrixMap → one trailing ShiftMap).
- [ ] **Step 5: Implement `ReSimp`/`AlSimp` restricted-simplify** (only simplify mappings flagged AlSimp when restricted). This is the medium-large sub-piece — verify against the oracle for cap_zoommap_01.
- [ ] **Step 6: Run** cap_cmpmap_01-04, matrixmap_02, zoommap_01 — expect clear.
- [ ] **Step 7: fmt + clippy + commit** ("cmpmap/matrixmap: invert normal form, shift fusion, ReSimp/AlSimp (match C)")

---

## Task C1: WcsMap::map_merge (self-inverse cancel + PermMap const-swap)

**Root cause:** WcsMap has no `map_merge` → `default_map_merge` can't cancel a WcsMap against its inverse across parallel structure, nor swap WcsMap/diagonal-Matrix through a PermMap (rewriting injected constants). matrixmap_01, winmap_02, wcsmap_01. Largest piece.

**Files:**
- Create/Modify: `crates/ast-core/src/mappings/wcsmap.rs` (add `map_merge`)
- Modify: `crates/ast-core/src/mappings/simplify.rs` (dispatch; PermMap-const-swap at ~1130-1400 for the WcsMap+diagonal-Matrix case)

- [ ] **Step 1: Read C** wcsmap.c MapMerge (~900-1010): self-inverse → UnitMap cancellation, WcsMap↔PermMap swap; constant transform mirrors cmpmap.c:1929-1949.
- [ ] **Step 2: Oracle** matrixmap_01 → single 3-axis WinMap (WcsMap TAN/TAN⁻¹ pair annihilates, flanking linear maps fuse); wcsmap_01 → `PermMap ; MatrixMap(full) ; CmpMap(...)` with rotated PermMap constants.
- [ ] **Step 3: Implement `WcsMap::map_merge`** — forward/inverse self-cancellation to UnitMap (including across an intervening parallel spectral branch), and the WcsMap↔PermMap swap that transforms injected constants.
- [ ] **Step 4: Dispatch from simplify.rs** and extend the PermMap-constant-swap path for the WcsMap+diagonal-Matrix case.
- [ ] **Step 5: Run** matrixmap_01, winmap_02, wcsmap_01 — expect clear.
- [ ] **Step 6: fmt + clippy + commit** ("wcsmap: add MapMerge for self-inverse cancel and PermMap swap (match C)")

---

## Task D1: Re-measure residual structural grouping

**Root cause:** specmap_01/02/12 (and any cap_matrixmap_02 residue) are downstream normal-form divergences that the investigation predicts will largely resolve once S1/S2/E1 land. Re-measure before doing bespoke work.

**Files:** TBD after re-measurement — likely `simplify.rs` (parallel/series restructuring, WinMap/MatrixMap absorption).

- [ ] **Step 1: Re-run the full simplify regression** and list which of specmap_01/02/12 still fail.
- [ ] **Step 2: For each still-failing fixture,** build an oracle reproducer, diff actual vs expected structure, and identify the specific restructuring path (cmpmap.c MapMerge 1205+, parallel/series 1577-1620; winmap distribution).
- [ ] **Step 3: Port the specific missing restructuring** (mirror C; no heuristics).
- [ ] **Step 4: Run** the remaining fixtures — expect clear.
- [ ] **Step 5: fmt + clippy + commit.**

---

## Task FINAL: Full green + verification

- [ ] **Step 1:** `cargo test -p ast-core --test test_simplify_regression -- --nocapture` → `0 equal(), 0 transform, 0 serialization, 0 other failures`; only skips are the 16 BLOCKED IntraMap/XphMap (parse-skip) and 0 file-absent.
- [ ] **Step 2:** `cargo test -p ast-core` → all green. `cargo test -p ast-core --test '*-regression'` and `test_wcsconv` → green.
- [ ] **Step 3:** `cargo build --workspace`, `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` → clean.
- [ ] **Step 4:** Update `docs/issues/c-library-quirks.md` / PLAN.md if any fix revealed a new quirk or completed a tracked item.

## Self-Review Notes

- **Coverage:** all 64 fixtures map to a task (P1: 15 parse-skips; S1-3: spec/sla/time; M1-2, W1, B1, E1, C1: general mappings; R1-3: regions; D1: residual). The 16 IntraMap/XphMap stay BLOCKED (out of scope until those classes are ported — a separate future effort).
- **Ordering:** P1 (unblock parses) and S1 (foundational merge-barrier bug) first; D1 last (re-measure). Dependencies noted (R1/R2 need P1; B1/E1 after S1; D1 after S1/S2/E1).
- **Convention:** every task cites the C source and requires an oracle reproducer; no heuristics (R3 explicitly de-heuristics existing Rust).
- **Risk:** C1 (WcsMap::map_merge) and E1 (ReSimp/AlSimp) are the largest; if either balloons, stop and re-scope with the user rather than expanding silently.

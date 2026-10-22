# Porting plan: rebuild the simplifier around C's `MapMerge` architecture

> **HISTORICAL DOCUMENT (completed).** The entire phased rebuild landed: the
> simplifier now mirrors C's nominate-loop + per-class `MapMerge` architecture
> (`run_nominate_loop`, per-class `map_merge` on every Mapping class), the
> legacy global-helper scaffold was retired, and the canonical rigby case is
> byte-exact. Retained as a record of the rebuild; no outstanding work.

## Status

In progress.

- **Phase 0 — done** (commit "simplify: add MapMerge scaffolding"). Added
  the `Mapping::map_merge` trait method (default = inherited null, C's
  `-1`), the `MergeCtx` working-list wrapper with a per-entry `frozen`
  flag, and a faithful port of C's `PatternCheck` oscillation detector
  (`pattern_check`). **Deviation:** the plan called for porting `CanSwap`
  "once as a free function", but C has no shared `CanSwap` — there are four
  distinct per-class versions (`matrixmap.c`, `pcdmap.c`, `winmap.c`,
  `wcsmap.c`) with different logic and signatures. Each is deferred to its
  owning class's phase (2-4) rather than unified, to keep the port
  faithful.
- **Phase 1 — done** (commit "simplify: rewrite merge_loop as C's nominate
  loop"). `merge_loop` now drives C's nominate loop (`run_nominate_loop`)
  over `MergeCtx`, with `default_map_merge` providing the shared direct
  pairwise merge (C MapMerge step 2) as the inherited `map_merge` body.
  The legacy global swap helpers remain as a temporary stall-handler
  fallback (retired into per-class `map_merge` in Phases 2-5), guarded by a
  `MAX_STALL_ROUNDS` bound that is removed in Phase 5. Behavior-neutral by
  construction and verified byte-for-byte: the full ast-core suite stays
  green and instrumentation confirms neither the `pattern_check` freeze nor
  the stall cap ever fires on the corpus.
- **Phases 2-5 — not started.** Resume from Phase 2 (WinMap `map_merge`,
  the one that unblocks `rigby`).

## Why this exists

The Rust simplifier (`crates/ast-core/src/mappings/simplify.rs`) was grown
opportunistically: individual C simplification behaviors were reimplemented as
standalone helper functions (`try_swap_winmap_past_matrixmap`,
`try_swap_wcsmap_past_permmap`, `try_distribute_winmap_into_parallel`, …) as
fixtures demanded them, without reproducing C's overall control structure.

The result passes most fixtures but diverges on cases that depend on the *order*
and *interaction* of simplification steps — most visibly `rigby` (see
"Worked example" below), where the exact byte output differs from C even though
the mapping is transform-equivalent.

Each attempt to fix such a case by adding or loosening another standalone helper
either misses the case (the structure C relies on never arises) or breaks a
negative-test invariant (e.g. `neg_win_swap_no_simplify`). This is the
signature of an architectural mismatch, not a missing rule: Rust is pattern
matching on flattened adjacencies, while C runs a per-class, nominate-driven
rewrite loop. We should stop patching and rebuild the engine to mirror C.

## C architecture (the big picture)

The authoritative source is `reference/starlink-ast/src/`. The control flow for
simplifying a compound mapping is:

### 1. `astSimplify` → `cmpmap.c::Simplify` (line 3390)

`Simplify` is the CmpMap override of the generic `astSimplify`. It:

1. Guards against infinite recursion via a thread-local stack of in-progress
   mappings (`simplify_stackmaps`); returns a clone if the same mapping is
   already being simplified higher up the stack.
2. Decomposes the tree with `astMapList` into a flat `map_list` plus a parallel
   `invert_list`, and captures the returned `simpler` flag.
3. Honors `RESTRICTED_SIMPLIFY` / `ALLOW_SIMPLIFY` (we can ignore this — we
   never set those flags).
4. `SeparateMappings` — deep-copies so components are independent (a class's
   `MapMerge` may temporarily mutate a component).
5. Clears the per-mapping `Frozen` flag on every component.
6. **Runs the nominate loop** (see 2 below) until a full pass makes no change.
7. Detects oscillation via a pattern-wavelength check on the `(modified, nmap)`
   history, to break merge/split cycles.
8. **Reconstructs** the final `map_list` into a right-associated CmpMap, setting
   each component's `Invert` to `invert_list[i]` and restoring the original
   afterwards (this orientation handling is already ported — see
   `reconstruct` and `CmpMap::set_first/second_orientation`).

### 2. The nominate loop + `astMapMerge` (the part we are missing)

```
improved = 1
while improved:
    improved = 0
    nominated = 0
    while nominated < nmap:
        if Frozen(map_list[nominated]): nominated++; continue
        result = astMapMerge(map_list[nominated], nominated, series,
                             &nmap, &map_list, &invert_list)
        if result == -1: nominated++          # this map declined
        else: improved = 1; nominated = result # restart from first change
```

`astMapMerge` dispatches to the **nominated mapping's class** `MapMerge`. That
method owns *all* the logic for how that class merges with, swaps past, or
rewrites its neighbors. Its contract (`cmpmap.c` MapMerge prologue, and the
WinMap prologue at `winmap.c:884`):

- `this` is the nominated mapping; `where` is its index in `map_list`.
- It may remove, replace, insert, reorder any entries in `map_list` /
  `invert_list`, updating `*nmap`.
- It returns the index of the first modified element, or `-1` for "no change".
- `invert_list` carries the effective direction; the *actual* `Invert`
  attributes of the supplied mappings are ignored.

This is fundamentally different from Rust's current model (a fixed sequence of
global swap helpers tried after pairwise `try_merge`). In C, *each class decides*
what to do when nominated, including multi-step swaps toward a distant target.

### 3. Per-class `MapMerge` (C overrides present)

`box cmpmap dssmap grismmap interval intramap lutmap mapping matrixmap mathmap
normmap nullregion pcdmap permmap pointlist polymap ratemap selectormap shiftmap
slamap specmap sphmap splinemap switchmap timemap tranmap unitmap unitnormmap
wcsmap winmap xphmap zoommap`.

The ones that carry real series/parallel rewrite logic for our fixtures:

- **`winmap.c::MapMerge` (884)** — the richest. Steps:
  1. *Replace-with-simpler*: if all shifts are zero, become a diagonal
     `MatrixMap` (faster). The WinMap→ShiftMap conversion is deferred to the
     end and `Frozen` to avoid an infinite ShiftMap↔WinMap loop.
  2. *Direct merge* with a mergeable neighbor (another WinMap / ZoomMap /
     ShiftMap / UnitMap / **diagonal** MatrixMap): `WinWin`, `WinZoom`, …
  3. *Swap toward the nearest mergeable target*: compute `nstep1`/`nstep2`
     (distance to the nearest mergeable map on each side, passing only over
     MatrixMap / WcsMap / PermMap); swap that direction via `WinMat` /
     `WinPerm` / `WinWcs`; then immediately `astMapMerge` at the new adjacency.
  4. *Speculative swap*: even with no target, swap with a neighbor if the
     result is "simpler" — kept iff the two neighbors can now merge **or**
     either swapped map `astSimplify`s to something different
     (`winmap.c:1660-1670`). A unit-scale WinMap simplifies to a ShiftMap, so
     absorbing a non-unit scale into an adjacent matrix is "simpler" and kept.
  - Helpers: `WinWin` (3888, with the equal-and-opposite cancellation and
    1e-15 snaps — already ported), `WinZoom` (4117), `WinMat` (2793, absorbs a
    WinMap's diagonal scale into a MatrixMap leaving scale **exactly 1.0**),
    `WinPerm` (2973), `WinWcs` (3833), `CanSwap`.
- **`matrixmap.c::MapMerge`** — `MatMat`, `MatZoom`, `MatPerm`, `MatWin2`
  (diagonal MatrixMap × WinMap only), `MatPermSwap`, `MatWin`, `CanSwap`,
  `MtrMult`/`SMtrMult`.
- **`permmap.c`, `zoommap.c`, `shiftmap.c` (→WinMap), `unitmap.c` (removes
  self), `wcsmap.c`, `sphmap.c`, `cmpmap.c` (parallel/series interchange),
  `specmap.c`, `slamap.c`, `pcdmap.c`, `grismmap.c`** — each has its own.
- **`cmpmap.c::MapMerge` (1205)** — handles a CmpMap nominated inside another:
  parallel-of-series ↔ series-of-parallel interchange, swapping a PermMap past
  a parallel CmpMap, etc.

### Shared helpers worth porting once

- `CanSwap(map1, map2, inv1, inv2)` — can these two adjacent mappings change
  order, and does it help. Currently scattered/implicit in Rust.
- `astMapList` decomposition + the `simpler` flag — Rust's `flatten` +
  `decomposition_is_simpler` already cover this.
- `astIsDiagonal` — present as `MatrixForm::Diagonal` / `is_diagonal`.
- `MtrMult` (matrix × matrix) — present via `ndarray`.

## Current Rust architecture (what we have)

`crates/ast-core/src/mappings/simplify.rs` (~4300 lines):

- `flatten` (181) ≈ `astMapList`; returns `(map, effective_inv, orig_inv)`.
- `decomposition_is_simpler` (248) ≈ the `simpler` flag.
- `merge_loop` (281): a pass loop that does pairwise `try_merge`, then, when no
  pair merged, tries a **fixed sequence** of global helpers:
  `try_swap_winmap_wcsmap`, `try_swap_winmap_past_matrixmap`,
  `try_swap_winmap_past_permmap`, `try_swap_wcsmap_past_permmap`,
  `try_swap_pcdmap_past_neighbor`, `try_merge_zoom_before_inv_grism`,
  `try_sphmap_diagonal_pattern`, `try_merge_parallel_in_series`,
  `try_merge_series_in_parallel`, `try_swap_permmap_parallel`,
  `try_distribute_winmap_into_parallel`, then a UnitMap strip.
- `reconstruct` (2583): reverse fold with orientation handling (faithful to C).
- Per-class `try_merge` (Mapping trait) in each `mappings/*.rs` — only does
  *direct pairwise* merges (C's step 2), not swaps or seeks.

### Why this diverges from C

1. **No nomination / ownership.** C lets the nominated class run its full
   strategy. Rust spreads each class's strategy across global helpers that only
   see flattened adjacencies, so multi-step "swap toward a distant target"
   (C step 3) and "speculative swap kept if simpler" (C step 4) are absent or
   approximated per-helper.
2. **Fixed helper order vs. data-driven seek.** Rust's helper order is a
   hand-tuned heuristic; C chooses direction by `nstep` distance to the nearest
   mergeable map. Different choices → different (but equivalent) structures.
3. **No `CanSwap` keep-if-simpler gate.** Rust gates each swap with bespoke,
   local conditions; C gates uniformly with "do the neighbors now merge, or
   does a swapped map simplify". This is exactly the gate `rigby` needs and the
   one that, when crudely approximated, breaks `neg_win_swap_no_simplify`.

## Worked example: `rigby` (the canonical failure)

Fixture: `crates/ast-core/tests/fixtures/native/reference/rigby.{map,simp}`,
listed in `simplify_tests.txt` (currently `skip_string_compare = yes`).

Symptom: Rust's simplified celestial WinMap keeps `Scl1 = 0.99999999997788802`
/ `Scl2 = 0.99999999998530598`; C has them at exactly `1.0` (omitted in the
dump). The scales are real values inherited from the FITS, not 1.0.

Mechanism in C (confirmed by instrumenting `winmap.c` `WinMat`/`WinWin`/`Dump`
and `matrixmap.c` `MatWin2`, then reverting to pristine):

1. The 3-axis WinMap `(0.9999, 0.9998, 551361)` is distributed into the
   parallel celestial/spectral CmpMap.
2. The celestial round-trip (`WcsMap`/`SphMap`/`Matrix` sandwich) cancels,
   leaving the celestial 2-axis WinMap adjacent to a **residual matrix**.
3. `WinMat` (iwm=0, `[WinMap, Matrix]`) absorbs the scale into that matrix and
   sets the WinMap scale to literally `1.0` (`winmap.c:2936`). The swap is kept
   because the unit-scale WinMap simplifies to a ShiftMap (the step-4 gate).

Why Rust fails: the distribution fires, but in Rust the celestial WinMap is
blocked behind a `WcsMap` and never reaches a residual matrix — Rust's celestial
round-trip cancellation produces a different residual structure than C, so step 2
does not set up step 3. Fixing this needs both the orchestration (steps 3-4) and
the upstream cancellation to match C, which only the architectural rebuild gives.

Two targeted patches were tried and reverted (recorded so they are not retried
blindly):

- A standalone `[Matrix, WinMap]` absorption: transform-correct but
  structurally divergent (broke `equal()`, not the transform) — it is not C's
  path.
- Relaxing `try_swap_winmap_past_matrixmap` to fire on any non-unit scale: broke
  `neg_win_swap_no_simplify` (over-fired) and still missed `rigby`.

## Rebuild plan (phased)

The goal is to replace the `merge_loop` + global-helper grab-bag with C's
nominate loop + per-class `MapMerge`, while keeping `flatten`, `reconstruct`,
and the per-class `try_merge` direct-merge logic (which is correct and reusable
as the "direct merge" step).

### Phase 0 — Scaffolding and safety net

- Freeze the current behavior as the baseline: the 437 `.map`/`.simp` +
  139 `.ast` + 24 generated round-trip fixtures, the `simplify_tests.txt`
  regression (correctness via `equal()` + transform), and the full `ast-core`
  suite must stay green at every step.
- Add a `MapMerge` trait method to `Mapping`:
  `fn map_merge(&self, ctx: &mut MergeCtx, where_: usize, series: bool) -> Option<usize>`
  where `MergeCtx` wraps the `Vec<(DynMapping, bool, bool)>` working list plus
  the `Frozen` set. Default impl returns `None` (no change), so classes can be
  migrated incrementally.
- Port `CanSwap` once as a free function over two list entries.

### Phase 1 — The nominate loop

- Rewrite `merge_loop` as C's loop: iterate `nominated` over the list, call
  `list[nominated].map_merge(ctx, nominated, series)`, restart from the returned
  index on change, stop when a full pass makes none.
- Port the oscillation/wavelength break and the `Frozen` mechanism (a per-entry
  flag in `MergeCtx`, not on the mapping object).
- Keep the existing direct-merge `try_merge` as the first thing each class's
  `map_merge` tries (so nothing regresses while swaps are still TODO).
- At this point behavior should equal today's (helpers still invoked from a
  generic fallback `map_merge`). Verify byte-for-byte against the baseline.

### Phase 2 — WinMap `MapMerge` (the big one; unblocks `rigby`)

Port `winmap.c::MapMerge` faithfully into `WinMap::map_merge`:

- Step 1 replace-with-simpler (shifts zero → diagonal MatrixMap; deferred,
  frozen WinMap→ShiftMap at end).
- Step 2 direct merge with mergeable neighbor (reuse `try_merge`; add
  `WinZoom`).
- Step 3 `nstep1`/`nstep2` seek + `WinMat`/`WinPerm`/`WinWcs` swap + follow-on
  merge. `WinMat` must set the result WinMap scale to exactly `1.0` (the
  absorption), matching `winmap.c:2936`.
- Step 4 speculative swap kept iff neighbors merge or a swapped map simplifies
  (`winmap.c:1660-1670`).
- Retire `try_swap_winmap_past_matrixmap`, `try_swap_winmap_past_permmap`,
  `try_swap_winmap_wcsmap`, and the `try_distribute_winmap_into_parallel`
  WinMap-specific bits in favor of this method.
- Re-enable `rigby` (drop its `skip_string_compare`) and confirm byte-exact.

### Phase 3 — MatrixMap, PermMap, ZoomMap, ShiftMap, UnitMap `MapMerge`

- `MatrixMap::map_merge`: `MatMat`, `MatZoom`, `MatPerm`, `MatWin2` (diagonal),
  `MatPermSwap`, `CanSwap`. Retire `try_swap_*` PCD/matrix helpers as covered.
- `ShiftMap::map_merge` → convert to WinMap and defer (as C does).
- `ZoomMap`, `UnitMap` (self-removal), `PermMap`.

### Phase 4 — CmpMap, WcsMap, SphMap, and the remaining classes

- `CmpMap::map_merge`: series/parallel interchange and PermMap-past-parallel —
  replaces `try_merge_parallel_in_series`, `try_merge_series_in_parallel`,
  `try_swap_permmap_parallel`.
- `WcsMap`, `SphMap`, `PcdMap`, `GrismMap`, `SpecMap`, `SlaMap` as their
  fixtures require; retire the matching `try_*` helpers.

### Phase 5 — Cleanup

- Delete the now-unused global helpers and the fallback generic `map_merge`.
- `simplify.rs` should be left as: `flatten`, the nominate loop, `reconstruct`,
  `CanSwap`, oscillation/Frozen support, and shared math. Per-class strategy
  lives in each `mappings/*.rs` `map_merge`, next to that class's `try_merge`.

## Validation strategy

- **Every phase** must keep the three native round-trip corpora byte-exact
  (`test_serialisation`) and the `simplify_tests.txt` regression green
  (`equal()` correctness + transform-equivalence).
- Treat any new `skip_string_compare`/`astequal` relaxation as a regression to
  justify, not a convenience (see `wcsconv-compare-mode-policy`).
- Because this touches the most fragile code, do each phase as its own commit
  with the full corpus run, and consider a multi-agent review pass (port →
  adversarially verify against all fixtures) for Phases 2-4.
- Keep the C reference reproducer pattern handy: build a tiny C program that
  `astFromString` → `astSimplify` → `astShow`, instrument the relevant C
  function, **revert and rebuild the oracle pristine afterwards** (the
  submodule must stay clean).

## Invariants to preserve (do not regress)

- Native serialization stays byte-exact with C (CLAUDE.md contract).
- `reconstruct`'s orientation handling (component stored in its original
  orientation, effective direction in `InvA`/`InvB`) — already correct.
- The `WinWin` equal-and-opposite cancellation + 1e-15 snaps — already ported
  (`winmap.rs`).
- Negative fixtures (`neg_*`) must keep *not* simplifying; they are the guard
  against the over-firing that crude gating caused.
- No heuristics that paper over numeric drift: match C's exact arithmetic and
  control flow (e.g. `WinMat` sets scale to literal `1.0`; cancellation uses raw
  terms). The drift "fix" is structural (absorb scale into a matrix), never a
  tolerance snap beyond the ones C itself applies.

## Pointers

- C: `reference/starlink-ast/src/cmpmap.c` (`Simplify` 3390, `MapMerge` 1205,
  `MapList` 1004), `winmap.c` (`MapMerge` 884, `WinMat` 2793, `WinWin` 3888,
  `WinPerm` 2973, `WinWcs` 3833, `WinZoom` 4117), `matrixmap.c` (`MapMerge`,
  `MatMat`/`MatZoom`/`MatPerm`/`MatWin2`/`MatWin`/`CanSwap` ~294-331).
- Rust: `crates/ast-core/src/mappings/simplify.rs`, per-class `try_merge` in
  `mappings/*.rs`.
- Related: `c-library-quirks.md`, `native-roundtrip-simplify-gaps.md`,
  `simplify-serialization-gaps.md`, `simplifier-winmap-inverted-merge-bug.md`.

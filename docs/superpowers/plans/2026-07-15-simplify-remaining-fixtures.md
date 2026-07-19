# Remaining simplify-fixture parity work (match C exactly)

> Resume plan after the C-faithful simplify-engine rewrite. HEAD at write time:
> `0dfdaa3` on branch `tickets/DM-54793-ci-clippy-sync`.
> Requirement (restated by user): **the port must match C.** Any divergence —
> including a >4-ULP floating value or a differently-placed flag — means we are
> doing something structurally different from C and MUST be corrected to match,
> not tolerated. "Byte-cosmetic" is NOT acceptable as an end state; every diff
> below is a real C-divergence to close.

## Current state (baseline for this plan)

Run these to reproduce:
```
cargo test -p ast-core --test test_simplify_regression simplify_regression -- --nocapture 2>&1 | grep "Simplify regression:"
# => 407 passed, 4 equal() failures, 0 transform failures, 6 serialization failures
cargo test -p ast-core --lib 2>&1 | grep "test result"                 # 999 passed
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"   # 192 passed
cargo test -p ast-core --test test_serialisation native_byte_exact_round_trip_simplify_fixtures -- --nocapture 2>&1 | grep byte-exact  # 649/677
```
The `simplify_regression` test now PASSES (it hard-panics only on transform/other
failures; equal + serialization mismatches are reported but non-fatal). Do NOT
regress: 407 passing fixtures, 999 lib tests, 192 wcsconv, 649/677 byte-exact.

## What the engine now is (context)

The engine was rewritten this cycle to be C-faithful: `flatten` (=`astMapList`)
→ nominate loop `run_nominate_loop` with per-class `map_merge` + `pattern_check`
/freeze (=`cmpmap.c::Simplify`) → `reconstruct` (=reverse `astCmpMap` rebuild).
All 10 legacy whole-list swap helpers were folded into per-class `map_merge`
(WinMap/CmpMap/PermMap/WcsMap/PcdMap/GrismMap/SphMap) and the stall-loop scaffold
deleted (commit `0b31272`). Key files: `crates/ast-core/src/mappings/simplify.rs`
(engine, MergeCtx, flatten, reconstruct, per-class *_map_merge free fns),
`winmap_merge.rs`, `matrix_merge.rs`, and per-class `map_merge` in each class file.

## Tooling (reuse every task)

**Dump got-vs-want** — append to `crates/ast-core/tests/test_simplify_regression.rs`
(REMOVE before finishing; it uses the in-file helpers `from_native_string`,
`mapping_to_native_string`):
```rust
#[test] #[ignore] fn dbg_dump() {
    let name = std::env::var("F").unwrap();
    let dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/native/reference/simplify_fixtures"));
    let m = from_native_string(&std::fs::read_to_string(dir.join(format!("{name}.map"))).unwrap()).unwrap().into_mapping().unwrap();
    let got = mapping_to_native_string(m.simplify().unwrap().as_ref()).unwrap();
    std::fs::write(format!("/tmp/{name}.got"), &got).unwrap();
    std::fs::write(format!("/tmp/{name}.want"), std::fs::read_to_string(dir.join(format!("{name}.simp"))).unwrap()).unwrap();
}
```
`F=cap_cmpmap_01 cargo test -p ast-core --test test_simplify_regression dbg_dump -- --ignored --nocapture` then `diff /tmp/cap_cmpmap_01.got /tmp/cap_cmpmap_01.want`.
For region fixtures (parse as objects, compare via equal()): build a ref via
`from_native_string(.simp).into_mapping()` and print `s.equal(refm.as_ref())`.

**C oracle** — authoritative trace of the exact C merge/arith sequence.
The reference build lives at `reference/starlink-ast/build` (created per the
`reference-libast-build` memory recipe; `libast.dylib` on macOS). Build the
probe fresh (its old /tmp copy does not survive reboots): a ~25-line C program
that `astRead`s argv[1] through an `astChannel` source callback, calls
`astSimplify`, and `astShow`s the result — compile from inside `build/` with
`cc -I. -I../src probe.c -L. -last -lm -Wl,-rpath,$PWD -o repro_dbg` (rpath
makes LD_PRELOAD unnecessary). Sanity check: its output must byte-match the
fixture's `.simp`.
Add env-gated `fprintf` (guard `if(getenv("AST_DBG"))`; cmpmap.c needs
`#include <stdlib.h>` added) to `reference/starlink-ast/src/*.c` — useful
probe points: `astSimplify_` after `astSetIsSimple`, the cmpmap.c MapMerge
head after `new = astSimplify(map)`, the nominate loop around `astMapMerge`,
and the `astClearIsSimple` macro in mapping.h (print only on set→clear).
Rebuild `cmake --build build --target ast`, run with `AST_DBG=1`. **MUST**
`cd reference/starlink-ast && git checkout src/ && cmake --build build
--target ast` (revert + rebuild clean) before committing.

**Per-task discipline**: measure the exact gap first (disable/inspect), port the
C logic faithfully, run all 4 gates, commit atomically with a message ending
`Generated with AI\n\nCo-Authored-By: SLAC AI` (write via a file — backticks/
parens in `-m` break the shell). Never leave a transform regression. If a fix
can't land without regressing a passing fixture, the C guard is being modeled
wrong — trace deeper, don't force.

---

## GROUP 1 — CmpMap child ORIENTATION REPRESENTATION (3 fixtures, one root cause)

**Fixtures:** cap_cmpmap_01 (serial: 5 missing `IsSimp` + 1 `Invert`),
cap_cmpmap_04 (serial: 4 `IsSimp` + 1 `Invert`), cap_winmap_01 (serial: 1
missing `Invert=1` on a WcsMap child of a parallel CmpMap with `InvA=1`).

**Root cause (fully diagnosed):** C's CmpMap stores each component at its OWN
`Invert` flag (`this->invert1`/`invert2`, captured at construction, cmpmap.c:4656)
and records the effective transform direction separately. A component originally
stored inverted stays serialized `Invert=1` with `InvA`/`InvB` carrying the
effective direction; C's `astSimplify_` tags each astSimplify RESULT with
`IsSimple` (mapping.c:24759) and `astSetInvert` CLEARS it (mapping.c:23256), so
the surviving `IsSimp` tokens are exactly the components astSimplify'd and not
subsequently reoriented. Our engine normalizes child orientation differently in
`flatten`/`reconstruct`, and the missing `IsSimp` tags on cap_cmpmap_01/04 are
COUPLED to this: they sit inside a TranMap inverse subtree that C keeps inverted
(C's `tranmap.c` MapMerge, lines 826-861, sets each component to its snapshot
`invert1`/`invert2`, astSimplify's at that orientation, and builds
`astTranMap(smap_f, smap_i)` keeping the result orientation). Our
`TranMap::simplified` (`crates/ast-core/src/mappings/tranmap.rs:258`) instead
calls `self.map1.simplified()`/`self.map2.simplified()` on the RAW stored maps
(ignoring `inv_a`/`inv_b`), which normalizes the inverted branch to forward.

**Infra already present** (from earlier committed work, still in the tree):
`CmpMap::is_simple_serial` (serialization-only tag, separate from the
`has_been_simplified` merge-guard), `dump_is_simple()` = OR of the two,
`write_mapping_base` emits `IsSimp` from `dump_is_simple()`, and both `invert()`
and `invert_into_structure()` clear `is_simple_serial` (mirroring astSetInvert →
astClearIsSimple). It is raised in `flatten` (changed opaque CmpMap leaf) and in
`compute_merge_parallel_pair_in_series`. It is NOT yet raised at every C
astSimplify-tag point.

**What was tried and REVERTED (do not repeat naively):**
- Snapshot-orientation fix in `TranMap::simplified`: added a `snapshot_view`
  helper simplifying each component at `inv_a`/`inv_b` orientation. Net-neutral
  on the corpus (no fixture moved), reverted as inert-without-the-fuller-rework.
  It is CORRECT in principle (simplifying the raw stored map when `inv_b` is set
  simplifies the wrong direction — a real latent bug) but insufficient alone
  because `.simplified()` on the inverted view still normalizes the result to
  forward and `TranMap::new` recomputes definedness.
- Tagging unchanged nominated CmpMaps in `cmp_map_merge` self-simplify head:
  regressed cap_cmpmap_03 (serial 14→12 at the time) — over-tags.
- Public `simplify()` in the TranMap component step: regressed (top-level
  cleanup perturbs unrelated fixtures).
- Re-inverting the simplified inverted component to preserve orientation:
  regressed equal() (flips `TranMap::new`'s `has_inverse`/`has_forward`).

**Correct fix (invasive, do carefully):** Rework `TranMap::simplified` to mirror
C `tranmap.c` MapMerge invert bookkeeping: keep `map2` in its stored orientation
tracked by `inv_b` (don't fold the inversion into forward structure), simplify
each component at its snapshot orientation, build the result preserving each
component's orientation and re-derive `tran_forward`/`tran_inverse` from the
snapshot-oriented components (C's `astGetTranForward`/`Inverse` on the components
as set to `invert1`/`invert2`). This will make the TranMap inverse branch
serialize `Invert=1` (fixing cap_cmpmap_01/04's `Invert` diff) and, combined
with tagging the astSimplify'd surviving components' `is_simple_serial`, emit the
missing `IsSimp` tokens. Then the same child-stays-inverted convention fixes
cap_winmap_01's WcsMap `Invert=1`.
- **HIGH RISK**: 12 TranMap fixtures exist (grep `Begin TranMap` in the fixtures
  dir): cap_cm_01, cap_cm_04, neg_tra_nontranmap_neighbour, neg_tra_oneway,
  neg_tra_parallel, neg_tra_unequal_components, neg_tra_adjacent_no_merge,
  tra_component_simplify, tra_equal_components, tra_merge_tail, tra_adjacent_merge,
  tra_invert_normalize — 10 currently PASS. Also the cap_cmpmap_02/03 byte-exact
  pair is sensitive to CmpMap orientation. Verify ALL of these after each change.
- **Approach**: first trace C's exact per-node `Invert`+`IsSimple` for
  cap_cmpmap_01 (env-gated fprintf in cmpmap.c `Dump` printing
  `astGetIsSimple(this)`, `this->invert1/2`, `astGetClass(map1/map2)`,
  `astGetInvert(map1/map2)`, nin/nout). That gives the exact target flag set per
  node. Then make `reconstruct`/`TranMap::simplified` reproduce it.

**Verified C IsSimple lifecycle (2026-07-15, C-oracle traced on cap_wcsmap_01,
brad, cmpmap_parallel_series_dim_mismatch — instrument `astClearIsSimple` in
mapping.h to fprintf on set→clear transitions, plus mapping.c `astSimplify_`
and the cmpmap.c MapMerge head/nominate loop):**

1. `astSimplify_` (mapping.c) tags EVERY result with `IsSimple` — even when
   the class Simplify returned `astClone(this)` unchanged: the clone shares
   the object, so the *original tree node* gains the tag. All the failed
   pass's nested head-tags likewise persist through shared objects.
2. `astSetInvert` clears `IsSimple` even when setting the current value
   (astMAKE_SET expansion, mapping.c:23256). `astClearInvert` does NOT clear
   it (plain astMAKE_CLEAR).
3. Probe side effects: matrixmap.c `CanSwap` (418-421 set, 580-581 restore)
   `astSetInvert`s BOTH real list objects on every probe — any neighbour
   class, even when no swap results — wiping their tags. Equivalent patterns
   exist in winmap.c/wcsmap.c CanSwap and matrixmap.c PermGet. A wiped CmpMap
   element re-runs the full recursive Simplify at its next nomination (the
   `astGetIsSimple` shortcut no longer fires), re-tagging its nested
   components; a tagged element short-circuits, freezing its subtree's flags.
4. The MapMerge head brackets its `astSimplify` with an adjust/restore
   `astSetInvert` pair when stored Invert ≠ applied invert_list value
   (cmpmap.c:1432-1448); the restore wipes the fresh root tag immediately
   (nested tags survive — the restore touches only the element object).
5. The final "Construct the output Mapping" fold `astSetInvert`s every
   element it wraps → a multi-element rebuilt result NEVER carries `IsSimp`
   on a spine leaf. The `nmap == 1` path (`astCopy` + `astClearInvert`)
   preserves tags. Nothing after the fold re-tags anything except the root
   (tagged by the caller's `astSimplify_`).
6. Consequently the final `IsSimp` placement is an EMERGENT property of the
   full clear/re-tag sequence. Piecemeal ports all regress: dump-only
   fold-clear broke cap_specmap_01 (C re-tags after an intermediate fold via
   pass-2 nominations); fold-clear + matrix probe-clear + head-retag combos
   broke brad / cmpmap_parallel_series_dim_mismatch / cap_specmap_01 (our
   engine lacks C's shortcut state, so heads recurse and over-tag) and a
   store-back variant broke cap_cmpmap_02's InvA/InvB placement (fingerprint
   ignores orientation, so "unchanged" store-back re-oriented the element —
   C never replaces the object in the unchanged case). Model the lifecycle
   whole — tag-on-simplify, clear-on-set-invert (probes included),
   shortcut-on-tagged, fold-clear — together with the orientation rework,
   not as isolated patches.

## GROUP 2 — cap_winmap_02: WinMap shift-fold ASSOCIATIVITY (>4 ULP => real divergence)

**STATUS (2026-07-19): DONE — landed as `simplify: cancel WcsMap inverse
pairs at either neighbour during WcsMap nomination`. The root cause was NOT
WinMap arithmetic (`compose_linear` matches C's WinWin/WinShift/WinDiagMat
per-pair formulas exactly); it was nominate-loop merge ORDER. C's
`wcsmap.c::MapMerge` (3116-3161, via `CanMerge` 812) cancels a WcsMap
inverse pair when EITHER member is nominated, checking the lower pair
`(where-1, where)` first; our `wcs_map_merge` only reached the upper pair
through `default_map_merge`, so the TAN pair cancelled one nomination later
than in C. That let the `(shift_a∘diag)` WinMap fold `shift_b` before the
front WinMap arrived, associating `w + (a + b)` instead of C's
`(w + a) + b`. Porting the lower/upper pair checks makes the nominate
cascade (`modified` jump-back, cmpmap.c:3619) match C event-for-event and
cap_winmap_02 is byte-exact. Serialization failures 6→5; all other gates
unchanged (407/999/192, 649/677).**

**Diff:** `Sft1 = -9.9999999999967808e-11` (ours) vs `-1.0000000000000751e-10`
(C). This is >4 ULP → we compute the shift by a different operation order than C.

**Root cause:** C folds three shift terms as `((winshift + shift1) + shift2)`;
our merge combines `shift1 + shift2` first, then the winshift. Different
association → different rounding. Trace which merge in the nominate sequence
produces `Sft1`: env-gated fprintf in `reference/starlink-ast/src/winmap.c` at
the shift computation printing the operands and intermediate. The Rust side is
`compose_linear` (`winmap.rs:534`, `new_shifts[i] = c2[i]*s1[i] + s2[i]`) and/or
the WinMap→parallel distribution path or a WinMap absorbing a preceding shift.
Identify the 3-term fold site and reorder the additions to match C EXACTLY
(don't algebraically simplify — mirror the parenthesization). Verify no other
WinMap/ShiftMap fixture's value shifts and wcsconv stays 192.

## GROUP 3 — cap_specmap_02: Fwd/Inv transform-definedness orientation

**Diff:** 14 lines, dominated by `Fwd = 0` vs `Inv = 0` swaps across ~6 nodes on
a numerically-correct inverted one-way LutMap result, plus a couple misplaced
`IsSimp`. equal() and transform PASS. The mapping is a one-way (inverse-only or
forward-only) LutMap-bearing branch whose definedness flags serialize in the
opposite orientation to C. Likely the same orientation-representation family as
Group 1 (a component kept inverted by C vs normalized by us), but on a LutMap
rather than a CmpMap child. Trace C's node orientation via the oracle; the fix is
to preserve the component's stored orientation (and thus its Fwd/Inv definedness)
as C does. May be fixed as a side effect of the Group 1 TranMap/orientation
rework — re-check cap_specmap_02 after Group 1.

## GROUP 4 — cap_wcsmap_01: orphan PermMap constants not pruned

**STATUS (2026-07-15): constants DONE, landed as `permmap: compact the
constants pool when rebuilding a PermMap`. The remaining 1-line diff (an
extra `IsSimp = 1` on the parallel `[2×2-series ‖ UnitMap]` CmpMap child) is
an IsSimple-lifecycle diff — moved into Group 1's scope (see the findings
appended there).**

**Diff:** 5 lines. The front PermMap serializes `Nconst = 4` with orphan
`Con3 = <bad>` / `Con4 = <bad>` (two unreferenced constants) plus a cosmetic
`IsSimp`; C emits `Nconst = 2`. C's `astSimplify(pmnew)` inside the PermMap
MapMerge prunes constants no longer referenced by any `inperm`/`outperm` entry.
Our PermMap rebuild (the `perm_get`/`mat_perm` path or `PermMap::simplified` /
`canonical_null_form`) keeps the full constants array. **Fix (landed):**
`rebuild_perms` in `perm.rs` mirrors C's accumulation (permmap.c:1065-1094):
walk inperm first then outperm (C's `back = 0/1` order), route missing/BAD
constants to the bad marker `0`, dedupe by exact `==` into a fresh pool,
renumber refs. `canonical_null_form` also gained C's final `simpler` tests
(permmap.c:1375-1395, element-wise rebuilt-vs-stored comparison). All other
PermMap fixtures verified passing.

## GROUP 5 — Region class-conversion (4 equal() failures) — SEPARATE SUBSYSTEM

**Fixtures:** cap_slamap_02 (Box → Prism), cap_interval_01 (Prism regrouping,
Box+Interval+NullRegion), cap_pointlist_02 (Prism, Interval+PointList),
cap_slamap_07 (CmpRegion, Box+Circle). These are Region objects with their own
`simplified()` in `crates/ast-core/src/regions/`; they do NOT route through the
CmpMap engine.

**Root cause:** all need `astConvertToPrism` (reference/starlink-ast/src/prism.c:232):
loop over all `2^nout` current-Frame axis subsets; `astPickAxes(region, nax, ax)`
each subset into a sub-Region (and the complementary subset into another); if
either picked+`astSimplify`'d half simplifies, form `astPrism(sp1, sp2)`, permute
its axes back to the original order (perm array logic at prism.c:364-378), wrap
via `astMapRegion(prism, UnitMap, current_frame)`, and `astSimplify` the result.

**Blocking dependency (why this is a separate project):** `astConvertToPrism`
needs Region-level `PickAxes` returning a Region, which C implements
(region.c:7685) via: `astMapSplit` on the current→base Mapping → `astRegBasePick`
→ `astMapRegion` → `RegOverlay`. Our regions only have frame-level
`pick_axes_snapshot` (`prism.rs:423`); there is no region-returning `PickAxes`.
So this task = port Region `PickAxes` (region.c:7579/7685) + `RegBasePick` (per
region class) + wire `astConvertToPrism` into Box/Prism/CmpRegion `simplified()`.
Substantial; do as its own plan after Groups 1-4.
- **Guard care:** a prior attempt at a Prism port over-collapsed cap_pointlist_01
  (a passing fixture where C keeps the outer Prism). C forms the Prism ONLY when
  a picked half actually simplifies (`sp1 != pickfrm1 || sp2 != pickfrm2`) — model
  that exact condition; don't convert unconditionally.

## Suggested order

1. Group 4 (cap_wcsmap_01 constant pruning) — smallest, self-contained, low risk.
2. Group 2 (cap_winmap_02 shift-fold order) — isolated arithmetic, low risk.
3. Group 1 (TranMap/CmpMap orientation rework) — fixes 3 fixtures + likely
   Group 3; highest value but invasive, do with full TranMap-fixture regression
   watch. Re-check cap_specmap_02 (Group 3) after.
4. Group 5 (region PickAxes + ConvertToPrism) — separate subsystem, own effort.

## Verification after every commit
```
cargo test -p ast-core --test test_simplify_regression simplify_regression -- --nocapture 2>&1 | grep "Simplify regression:"
cargo test -p ast-core --test test_serialisation native_byte_exact_round_trip_simplify_fixtures -- --nocapture 2>&1 | grep byte-exact
cargo test -p ast-core --lib 2>&1 | grep "test result"
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
cargo clippy -p ast-core --lib 2>&1 | grep -E "warning|error"   # must be empty
cargo fmt -p ast-core
```
Success = 0 equal, 0 transform, 0 serialization failures on simplify fixtures;
byte-exact round-trip 677/677 for non-region fixtures; 407→ all passing; 999 lib;
192 wcsconv — all with zero regression. The memory note
`project_simplify_fixture_resume` has the full commit history and per-fixture
diagnoses; keep it updated as fixtures close.

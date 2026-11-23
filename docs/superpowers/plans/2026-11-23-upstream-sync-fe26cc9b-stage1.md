# Upstream sync to `fe26cc9b`, stage 1 (Mappings and the simplifier) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every Mapping and simplifier result match the C library at `fe26cc9b`, port `testmapmerge.c`, `testpolymapinv.c` and `testsplinemapedge.c`, and leave `check_c_headers_fresh.sh` and the MapMerge audit gates green with no stage-1 notes left in the checklists.

**Architecture:** One commit (or a small group) per upstream fix, each C-faithful at C's granularity.
`testmapmerge.c` is ported first with a named exclusion list, and each fix task removes the cases it closes, so the list proves progress and cannot hide a regression.
`astMtrSplitInputs` and CmpMap's per-component `MapSplit1` are ported as seams (MatrixMap method, CmpMap split step), not special cases.

**Tech Stack:** Rust 2024 (`starlink-ast`), C probes against `reference/starlink-ast/build`, the simplify trace (`tools/oracle_fixtures/build_simplify_trace.sh`), `audit-tools` (`reanchor-citations`, `c-docs`), `ast-bench`.

**Spec:** `docs/superpowers/specs/2026-11-22-upstream-sync-fe26cc9b-design.md` (Stage 1, "Per-stage method", "Rules settled during review").

## Global Constraints

- Branch `u/timj/sync-c-mappings`, branched from the tip of `u/timj/sync-c` (the user's choice, overriding the spec's "branched from `main` after the previous one merges"). Never push. Fixups with `git commit --fixup`. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, no session line.
- Shell is zsh; strip conda's toolchain (`unset CC CFLAGS CPPFLAGS LDFLAGS CXX`) for cargo and probe builds. Probes link `cc p.c -I$B -L$B -last -Wl,-force_load,$B/libast_err.a -lm -Wl,-rpath,$B` with `B=reference/starlink-ast/build`; set `AST_REPLACE_NAN=1` where NaN outputs matter.
- Never modify the submodule's tracked files.
- New source files open with the two-line AURA copyright / `SPDX-License-Identifier: LGPL-3.0-or-later` header; a ported C test names its C ancestor and its divergences in its header comment and joins `crates/starlink-ast/tests/mappings/main.rs`.
- Docs: one sentence per line, American spelling, no C file/line citations in rendered docs (provenance in `//` comments).
- A user-visible change gets a `CHANGELOG.md` `Unreleased` entry in the same commit; a mirrored quirk that upstream fixed gets a `docs/issues/c-library-quirks.md` entry under "Resolved upstream" (`RESOLVED (upstream <hash>)`, with a Contents line) in the same commit.
- Every commit that edits Rust cited by `docs/audits/mapmerge/*.md` re-runs `cargo run -q -p audit-tools --bin reanchor-citations -- --rust-only --warn-stuck --write docs/audits/mapmerge/*.md`, and every commit that ports C new at `fe26cc9b` turns that checklist's `Uncited C steps` line for it into a row.
- Error messages raised for C errors use C's text with C's `method(class): ` prefix, as the port's other C errors do.
- Stage end: `cargo test -p starlink-ast --no-fail-fast`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS=-D warnings cargo doc -p starlink-ast --no-deps`, `tools/fixtures/check_mirror.sh`, `tools/oracle_fixtures/check_c_headers_fresh.sh` and `tools/oracle_fixtures/check_probes_fresh.sh` all pass.

## Triage

Read from each commit's diff and the Rust site, with C probes and a throwaway Rust harness over every `testmapmerge.c` case (2026-11-23).
Verdicts: **mirror** (port reproduces old C; invert), **missing** (port new C), **already** (port already matches new C), **n/a memory**.

| commit | verdict | what changes | Rust site | task |
|---|---|---|---|---|
| `0b427150`, `9a598e55`, `fd90f3b1` | mirror | SplineMap: NaN is outside the spline; upper-limit step-back as CMLIB; evaluate only on knots K..N+1; `Dintrv(ty, ny+1)` | `splinemap.rs` `dbvalu_k`, `db2val` | 2 |
| `857f4a99` | mirror | PolyMap IterInverse needs a forward transformation (default, setter error) | `polymap.rs` `iter_inverse`, `set_iter_inverse`, `clear_iter_inverse` | 3 |
| `2d9d4449` | mirror (latent) | `astEQUAL`: identical values, or both NaN, are equal | `helpers/mod.rs` `ast_equals` | 4 |
| `7de5007a` | mirror | `astRate` floors its first step at `DBL_MIN`; Rust hangs at x = 1e-320 | `rate.rs` `numeric_rate` | 4 |
| `f584a552` | missing | MathMap `Equal` compares compiled opcodes and constants (with `astEQUAL`) in effective directions | `mathmap/mod.rs` `equal` | 5 |
| `b9cf680b` | mirror (most parts) | MatrixMap/WinMap merges and swaps only when equivalent and usable; replacement keeps its direction | `matrix_merge.rs`, `winmap_merge.rs` | 6 |
| `f8c7723e` | mirror | PermMap/parallel-CmpMap swap refused when both blocks constant or a constant block's input is fed by an output | `simplify.rs` `swap_permmap_parallel_at` | 7 |
| `f0230815` | mirror (CanMerge) / already (WcsPerm) | WcsMaps merge only with equal parameter counts | `wcsmap.rs` `params_equal` | 8 |
| `e7cb7d95` | mirror (PermSplit) / already (rest) | PermSplit `MapSplit` rejects an input fed by a bad value or an out-of-range output | `perm.rs` `map_split_zero_based` | 8 |
| `e6141f23` (+`1bb7a75d`, `ce8a892c`, `45e598ba`) | missing | `astMapMerge` checks the list fits before a merge (`AST__INNCO`) and after (`AST__INTER`) | `simplify.rs` `merge_propagating_allow_simplify` | 9 |
| `3438acbc` | missing | CmpMap `MapSplit2` goes straight to a MatrixMap's or WcsMap's inputs (`astMtrSplitInputs`); the port also lacks the per-component `MapSplit1` | `cmpmap.rs` `split_child`, `map_split_forward`, `map_split2`; `matrix.rs` | 10 |
| `78c0f559` | already | Restores a shared PolyMap's Invert after cancelling; owned values cannot share one | — | 11 (note) |
| `8cad1a4f` | already | TDB↔TCB cancel compares one argument; `time_steps_cancel` uses `nargs()` = 1 | — | 11 (note) |
| `2a84cade` | already | `MergeShift` uses nout; `merge_shift_2d` is 2-in/2-out only | — | 11 (note) |
| `0f11fe09` | already | RateMap stops if the parent transform failed; the port refuses the direction first (message text is stage 5) | — | 11 (note) |
| `1df4a93a` | n/a memory | Caller's PointSet kept on failure; Rust writes into caller-owned buffers | — | 11 (note) |
| `73d3cc88` | n/a memory | FrameSet `AllVariants` wrote before its buffer; exposes C's 200-character Domain-list error, which is stage 4 under the buffer rule | — | 11 (PLAN row, quirk) |

## Review Focus

1. A SplineMap evaluated at a NaN, at exactly the last knot, or on knots that are not coincident at the ends (open knots) must give `AST__BAD` or C's value and never panic. Pinned in Task 2 (open-knot cases at (0.5,0.5), (0.5,3), (0.5,3.5), (0.5,0.3)).
2. `astRate`, `astSimplify` and `astEqual` on values of 1e-320, infinity and NaN must terminate and agree with C. Pinned in Tasks 4 and 6 (rate at 1e-320; inverted WinMap of scale 1e-320 then ZoomMap 3).
3. A merge that C now refuses must leave the CmpMap's class and structure as C does, not only an equivalent transform. Pinned in Tasks 6 and 7 by structural assertions on the four structural-only cases.
4. A Native dump of a PolyMap with `IterInv = 1` and no forward coefficients must be rejected on read with C's code. Pinned in Task 3.
5. A WcsMap whose PV was set and then cleared keeps its parameter count through a Native round trip, so a merge decision does not change after save and load. Pinned in Task 8.

---

### Task 1: Port `testmapmerge.c` with a named exclusion list

**Files:**
- Create: `crates/starlink-ast/tests/test_mapmerge.rs`
- Modify: `crates/starlink-ast/tests/mappings/main.rs`, `PLAN.md` (tracker row for `testmapmerge.c`)

**Interfaces:**
- Produces: `const MAPMERGE_GAP_EXCLUDED: &[(&str, &str)]` (case description as C prints it, reason naming the commit and task that closes it); test `excluded_cases_still_differ` that fails, naming the case, once an excluded case passes.

- [ ] **Step 1: Port every case** of `test_permmap`, `test_permmap_parallel`, `test_wcsmap`, `test_polymap`, `test_timemap`, `test_matrixmap`, `test_winmap` and `test_mathmap`, keeping C's `what` strings as case names and C's `compare` / `check` / `check_equal` logic (`agree` tolerance as in C). The MatrixMap built from a dump uses `from_native_string`; null permutations use `PermMap::with_null_perms`.
- [ ] **Step 2: Add structural assertions** where C's result is a specific class or keeps a CmpMap: "inverted MatrixMap of zeros, then a MatrixMap" (CmpMap kept), the WcsMap+ZoomMap swap cases in both orders and the two-WcsMaps case (CmpMap kept, not one PermMap). Get each expected class from a C probe printing `astGetC(simp, "Class")`.
- [ ] **Step 3: Run** `cargo test -p starlink-ast --test mappings test_mapmerge::` and record each failing case in `MAPMERGE_GAP_EXCLUDED` with its closing task. Expected from triage: 6 `test_matrixmap`/`test_winmap` cases and 1 structural (Task 6), 2 behavioral and 3 structural `test_permmap_parallel` cases (Task 7), 3 `check_equal` cases (Task 5). Any other failure is a finding: trace it (CLAUDE.md "Debugging the simplifier") and assign it before continuing.
- [ ] **Step 4: Run again.** Expected: PASS, with `excluded_cases_still_differ` passing.
- [ ] **Step 5: Commit** ("Port testmapmerge.c").

### Task 2: SplineMap evaluates only where the spline is defined

**Files:**
- Create: `crates/starlink-ast/tests/test_splinemapedge.rs`
- Modify: `crates/starlink-ast/src/mappings/splinemap.rs` (`dbvalu_k`, `db2val`, struct doc), `crates/starlink-ast/tests/mappings/main.rs`, `docs/audits/c-docs/c-headers.tsv`, `docs/audits/c-docs/ledger.tsv` (row `class SplineMap`), `docs/issues/c-library-quirks.md`, `CHANGELOG.md`, `PLAN.md`

- [ ] **Step 1: Port `testsplinemapedge.c`**, all six cases, via `SplineMap::from_scipy`. Run it: expected FAIL in "bilinear 1x1", "constant 1x1", "2x 1y" (BAD along y = 1) and "equal X knots" (NaN at x = 1).
- [ ] **Step 2: Add unit tests in `splinemap.rs`** with C's values (probe them again before committing): open knots along Y (0,1,2,3,4) at (0.5,0.5) → BAD, (0.5,3) → 4.5, (0.5,3.5) → BAD; open knots along X at (0.5,0.3) → BAD; a NaN input → BAD. Expected: the Y cases panic today (`splinemap.rs` index out of range).
- [ ] **Step 3: Port the fixed C** (`splinemap.c` at the pin): the definedness test `!( xval >= tx[kx-1] && xval <= tx[nx] && yval >= ty[ky-1] && yval <= ty[ny] )` → BAD; `Dintrv( ty, ny+1, … )`; the do-while step-back `do { if( lefty == ky-1 ) return AST__BAD; lefty--; } while( yval == ty[lefty] );` and its `i` twin in `Dbvalu`; `!( x >= t[k-1] )` in `Dbvalu`; BAD if a column's `Dbvalu` is BAD.
- [ ] **Step 4: Run** both test sets. Expected: PASS.
- [ ] **Step 5: Docs.** Add to the `SplineMap` struct doc, after the extrapolation paragraph: "On each axis, a spline of order K with N coefficients covers the range from knot K to knot N+1 (counting from one). This is the full range of the knots if the first K knots are coincident and the last K knots are coincident, as is usual." Regenerate `c-headers.tsv` with the command `check_c_headers_fresh.sh` uses and re-review the `class SplineMap` ledger row. Fix the quirk "SplineMap's Dbvalu loops…" citation (`Dbvalu` is now at `splinemap.c:645`) and add a RESOLVED entry for the three commits. CHANGELOG "Fixed": SplineMap gives a bad value, not a panic or a wrong value, outside the knots it covers; the value on the upper limit is the spline's.
- [ ] **Step 6: Commit** ("Evaluate a SplineMap only where its spline is defined").

### Task 3: PolyMap's iterative inverse needs a forward transformation

**Files:**
- Create: `crates/starlink-ast/tests/test_polymapinv.rs`
- Modify: `crates/starlink-ast/src/mappings/polymap.rs` (`iter_inverse`, `set_iter_inverse`, `clear_iter_inverse`, docs at the struct and constructor), `crates/starlink-ast/tests/mappings/main.rs`, `docs/audits/c-docs/c-headers.tsv`, `docs/audits/c-docs/ledger.tsv` (row `attribute IterInverse`), `CHANGELOG.md`, `PLAN.md`

- [ ] **Step 1: Port `testpolymapinv.c`.** The "constructing … with IterInverse=1" case uses `set_iter_inverse(true)` after construction (no options string in Rust); say so in the header comment. Add the Review Focus 4 case: a Native PolyMap dump with `IterInv = 1` and no forward coefficients read through `Channel`, expecting `ErrorCode::Attin`. Run: expected FAIL (today: empty PolyMap has `iter_inverse()` true, the setter accepts, the load accepts).
- [ ] **Step 2: Implement.** Default: true only if `forward_defined()` and no inverse coefficients and nin == nout. Setter: `value && !(nin == nout && forward_defined())` → `ErrorCode::Attin`, message `astSetIterInverse(PolyMap): Cannot use an iterative inverse because the PolyMap has unequal numbers of inputs and outputs.` or `… because the PolyMap was created without a forward transformation.` (check the exact class-name substitution against a probe). The loader already calls the setter.
- [ ] **Step 3: Run.** Expected: PASS.
- [ ] **Step 4: Docs.** `iter_inverse` "Default:" line becomes "true if forward coefficients but no inverse coefficients were supplied, and the numbers of inputs and outputs are equal"; `set_iter_inverse` `# Errors` adds "or has no forward transformation"; the struct and constructor sentences that say an absent inverse defaults to an iterative inverse gain the forward condition. Regenerate `c-headers.tsv`, re-review the ledger row. CHANGELOG "Changed". `check_c_headers_fresh.sh` must now pass.
- [ ] **Step 5: Commit** ("Require a forward transformation for a PolyMap's iterative inverse").

### Task 4: `astEQUAL` of infinity and NaN, and `astRate` near zero

**Files:**
- Modify: `crates/starlink-ast/src/helpers/mod.rs` (`ast_equals`), `crates/starlink-ast/src/mappings/rate.rs` (`numeric_rate`), `docs/issues/c-library-quirks.md`, `CHANGELOG.md`

- [ ] **Step 1: Tests.** In `helpers/mod.rs`: `ast_equals(INF, INF)`, `ast_equals(NAN, NAN)` true; `ast_equals(INF, -INF)` false. Public-API tests in the existing ShiftMap/WinMap test files: `ShiftMap::new(&[INF])` equals a copy of itself; a WinMap of scale INF equals a copy. Rate: a MathMap `y = 2*x`, `rate` at x = 1e-320 → 2.0, run under a 10-second guard thread so a hang fails rather than stalls. Expected: FAIL (false; hang).
- [ ] **Step 2: Implement.** `ast_equals`: `a == b || (a.is_nan() && b.is_nan())` returns true before the tolerance test (C `object.h.in:1487`). `numeric_rate`: `if h0 < f64::MIN_POSITIVE { h0 = f64::MIN_POSITIVE }` at C's place (`mapping.c:9175`).
- [ ] **Step 3: Run** the new tests and `cargo test -p starlink-ast --test mappings`. Expected: PASS.
- [ ] **Step 4:** RESOLVED quirk entries for both, CHANGELOG "Fixed" for both, commit ("Treat an infinite or NaN value as equal to itself, and floor astRate's first step").

### Task 5: MathMap `Equal` compares compiled operations

**Files:**
- Modify: `crates/starlink-ast/src/mappings/mathmap/mod.rs` (`equal`), test file for MathMap, `test_mapmerge.rs` (remove the 3 `check_equal` exclusions), `CHANGELOG.md`

- [ ] **Step 1: Tests** (with C probe values): `b=a+1` vs `y=x+1` equal; constants differing within `astEQUAL` tolerance equal; "a MathMap and the inverse of its opposite" equal. Expected: FAIL.
- [ ] **Step 2: Port C's `Equal`** (`mathmap.c` at the pin, loop `icode = 1; icode <= ncode`): compare the effective-direction `fwd_compiled`/`inv_compiled` opcode streams and constants (`ast_equals`), with `LdCon`, `LdVar`, `Max`, `Min` consuming constants as C does.
- [ ] **Step 3: Run** MathMap tests and `test_mapmerge::`. Expected: PASS; `excluded_cases_still_differ` forces the exclusions out.
- [ ] **Step 4:** CHANGELOG "Changed"; commit ("Compare every compiled operation in MathMap equality").

### Task 6: MatrixMap and WinMap merge only to an equivalent, usable result

**Files:**
- Modify: `crates/starlink-ast/src/mappings/matrix_merge.rs`, `crates/starlink-ast/src/mappings/winmap_merge.rs`, `crates/starlink-ast/src/mappings/matrix.rs`, `test_mapmerge.rs`, `docs/audits/mapmerge/matrix.md`, `docs/audits/mapmerge/winmap.md`, `docs/issues/c-library-quirks.md`, `CHANGELOG.md`

- [ ] **Step 1: Add the Review Focus 2 case**: CmpMap(inverted WinMap of scale 1e-320, ZoomMap 3) simplifies without error to what C gives (probe C's class and transform). Expected: FAIL (`Mtrmt` today).
- [ ] **Step 2: Port `b9cf680b` piece by piece**, removing each `test_mapmerge` exclusion as it closes, and tracing any case that does not close: `replace_with_simpler` stores the original direction (`inv2 = invert_list[where]`, `matrixmap.c:1963`) and needs `ngood > 0`; `factor != AST__BAD` (`:1934`); the square test for the diagonal WinMap merge (`:2025`); `FwdUsable` gating every merger but the UnitMap one (`:2045-2053`, `FwdUsable` at `:5121`); a strict `Multiply` that returns no matrix for an undefined matrix, a bad element or a non-finite product; `MatWin`/`WinMat` returning "swapped", with callers skipping the merge and the speculative swap when it is 0 (`matrixmap.c:2247`, `:2315`, `winmap.c:1606`, `:1678`); `IsSquareDiagonal` requiring diagonal, square and usable (`winmap.c:2648`); `WinDiagMat` treating a unit matrix as a diagonal of ones; the discard check (`matrixmap.c:3288`, `winmap.c:3099`); the all-undefined-scales guard (`winmap.c:1158-1167`).
- [ ] **Step 3: Run** `test_mapmerge::`, the simplify regression (`cargo test -p starlink-ast --test meta test_simplify_regression::`) and `tools/oracle_fixtures/check_simplify_fresh.sh`. Expected: PASS, every Task 6 exclusion gone.
- [ ] **Step 4: Audit.** Re-port `matrix.md` rows 7, 8, 9, 13, 14, 21, 22, 24 and the rows 6–8 paragraph, and `winmap.md` rows 6, 8, 10, 21–25, against the new C; turn the uncited steps `matrixmap.c:2039` and `winmap.c:1158` into rows; delete both checklists' stage-1 notes. `cargo test -p starlink-ast --test meta test_audit_citations::` → PASS.
- [ ] **Step 5:** RESOLVED quirk entry for the mirrored parts; CHANGELOG "Fixed": simplify no longer fails on or changes such Mappings; commit ("Merge a MatrixMap or WinMap only to an equivalent, usable Mapping").

### Task 7: CmpMap swaps a PermMap with parallel Mappings only when it can

**Files:**
- Modify: `crates/starlink-ast/src/mappings/simplify.rs` (`swap_permmap_parallel_at`), `test_mapmerge.rs`, `docs/audits/mapmerge/cmpmap.md`, `docs/issues/c-library-quirks.md`, `CHANGELOG.md`

- [ ] **Step 1:** Confirm the five `test_permmap_parallel` exclusions fail.
- [ ] **Step 2: Port `f8c7723e`**: `if aconstants && bconstants { canswap = false }` (`cmpmap.c:2170`); an input of a constant block fed by an output index below `npout` refuses the swap (`:2142`, `:2158`), replacing the copy into `inperm_new`.
- [ ] **Step 3: Run** `test_mapmerge::` and the simplify regression. Expected: PASS, exclusions gone.
- [ ] **Step 4:** Re-port `cmpmap.md` rows 33, 34, 36, 37 and delete its stage-1 note; reanchor; RESOLVED quirk entry; CHANGELOG "Fixed"; commit ("Swap a PermMap with parallel Mappings only when the swap is equivalent").

### Task 8: WcsMap parameter counts and PermSplit's MapSplit

**Files:**
- Modify: `crates/starlink-ast/src/mappings/wcsmap.rs` (`params_equal`, `from_node` if needed), `crates/starlink-ast/src/mappings/perm.rs` (`map_split_zero_based`, stale `-1` comments), `docs/audits/mapmerge/wcsmap.md`, `docs/audits/mapmerge/perm.md`, `docs/issues/c-library-quirks.md`, `CHANGELOG.md`

- [ ] **Step 1: Tests.** An AZP WcsMap with PV2_1 set then cleared, in series with the inverse of a plain AZP, stays a CmpMap in both orders (C probe `wcsnp.c` in the triage scratch); the same after a Native round trip of the first WcsMap. PermSplit set: `PermMap(inperm=[1,0], outperm=[1,2])` split on input 2, and `inperm=[1,5]`, give no split. Expected: FAIL (UnitMap; outputs `[4]` and `[5]`).
- [ ] **Step 2: Implement.** WcsMaps with different parameter-list lengths do not merge (C's `np`, which grows on set and does not shrink on clear; make `from_node` reproduce it). PermSplit rejects `result[i] >= npout` (`permmap.c:1703`) and an input fed by a bad value; correct the comments at `perm.rs` that describe `-1`.
- [ ] **Step 3: Run** the tests, `test_mapmerge::`, `test_wcsmap`, `test_permmap`. Expected: PASS.
- [ ] **Step 4:** `wcsmap.md` row 14, `perm.md` rows 13, 33, 34 re-checked; reanchor; RESOLVED quirk entries; CHANGELOG "Fixed" (two entries); one commit per fix ("Merge WcsMaps only with equal parameter counts", "Refuse a PermSplit split on an output a PermMap does not have").

### Task 9: `astMapMerge` checks the axis counts of a merger's result

**Files:**
- Modify: `crates/starlink-ast/src/mappings/simplify.rs` (`merge_propagating_allow_simplify`, new `list_axes`), `docs/audits/mapmerge/` (the checklist that covers `astMapMerge`), `CHANGELOG.md`

**Interfaces:**
- Produces: `fn list_axes(list: &[(DynMapping, bool, bool)], series: bool) -> Option<(usize, usize)>` — None when a series list does not fit; nin/nout swapped per entry when `m.is_inverted() != inv` (C's `ListAxes`, `mapping.c:7560`).

- [ ] **Step 1: Tests** (in-crate, beside `base_simplify_reaches_a_classes_own_map_merge`): a hand-built series list `[ZoomMap(1 axis), ZoomMap(2 axes)]` through `dispatch_map_merge` → `ErrorCode::Innco`, `astMapMerge(ZoomMap): The Mappings to be merged in series do not fit together; the number of inputs of one is not the number of outputs of the one before it.`; a test-only Mapping whose `map_merge` replaces itself with one of different axis counts → `ErrorCode::Inter` with C's message (`mapping.c:24623`: nominee class, "series"/"parallel", class, new nin and nout, `""` or `" and do not feed one another"`, old nin and nout, "(internal AST programming error)"). Expected: FAIL.
- [ ] **Step 2: Implement** in `merge_propagating_allow_simplify` after the `do_not_simplify` guard: misfit before the merge → `ctx.fail(Innco)` and `None`; after a `Some` result with `!ctx.failed()`, recompute and `ctx.fail(Inter)` on a change.
- [ ] **Step 3: Run** the tests and the whole simplify regression. Expected: PASS (no corpus case reaches either error).
- [ ] **Step 4:** Audit row for the new C; CHANGELOG "Added" (a new error `simplify` can return); commit ("Check the axis counts of every Mapping merge").

### Task 10: `astMtrSplitInputs` and CmpMap's per-component split

**Files:**
- Modify: `crates/starlink-ast/src/mappings/matrix.rs` (new method), `crates/starlink-ast/src/mappings/cmpmap.rs` (`split_child`, `map_split_forward`, `map_split2`), `docs/audits/` rows if a checklist covers MapSplit, `CHANGELOG.md` (only if a result changes)

**Interfaces:**
- Produces: `impl MatrixMap { pub(crate) fn mtr_split_inputs(&self, outputs: &[usize]) -> Option<Vec<usize>> }` — from `matrixmap.c:6739`: reads the effective matrix, treats 0 and BAD as no dependency, returns the inputs in ascending order, None when the outputs depend on every input or on none.

- [ ] **Step 1: Find a case.** Write a C probe calling `astMapSplit` on CmpMaps that reach `MapSplit1` (a series CmpMap whose components split individually but whose whole does not, and one ending in a MatrixMap or WcsMap) and compare with the port. Record each difference as a test with C's outputs; if none differs, say so in the commit message and test the seam directly: `mtr_split_inputs` on a block-diagonal 3×3 matrix, a full one, an inverted one, and one with BAD elements.
- [ ] **Step 2: Implement** `mtr_split_inputs`, the per-component fallback (C's `MapSplit1`, `cmpmap.c:2453`, called from `MapSplit0` at `:2879` and `:3002`), and `map_split2` taking `&dyn Mapping` with the MatrixMap and WcsMap fast paths matching C's class tests.
- [ ] **Step 3: Run** the tests, `cargo test -p starlink-ast --test mappings`, and time the 9-axis rotation plus ZoomMap split on axis 1 before and after (`ast-bench` or a release-mode test). Expected: PASS, not slower.
- [ ] **Step 4:** Commit ("Split a CmpMap through its components, and a MatrixMap straight to its inputs").

### Task 11: Close the stage

**Files:**
- Modify: `PLAN.md` (§8 stage 1 row and notes, tracker rows), `docs/issues/c-library-quirks.md`, `docs/audits/mapmerge/*.md` (any stage-1 note left)

- [ ] **Step 1: Notes.** In `PLAN.md` §8, mark stage 1 done and record the verdicts for `78c0f559` (owned values cannot share a PolyMap), `8cad1a4f`, `2a84cade`, `0f11fe09` (message text left to stage 5) and `1df4a93a`. Add a stage 4 row for C's 200-character `AllVariants` Domain-list error (`frameset.c:322`, `:7018`) and an n/a-buffer quirks entry saying the port has no such limit.
  List the `~/work/astjava/docs/rust-c-differences.md` entries these commits resolve or change (TimeMap TDB/TCB cancel, PolyMap cancel, PermMap null permutations, MathMap comparison, MatrixMap/WinMap merges, WcsMap parameters, PermMap/parallel swap, SplineMap coverage, PolyMap all-zero coefficients) in the stage notes; that repository is not edited.
- [ ] **Step 2: Sweep.** `rg -n "until stage 1" docs/audits/mapmerge` returns nothing; `MAPMERGE_GAP_EXCLUDED` is empty and removed with its companion test.
- [ ] **Step 3: Gates.** The Global Constraints stage-end list, plus `cargo run --release -p ast-bench -- bits --base main` (expect differences only in SplineMap and rate corpus transforms, each explained in the commit message) and `cargo run --release -p ast-bench -- compare --set bench --base main`.
- [ ] **Step 4: Commit** ("Record stage 1 of the fe26cc9b sync as done").

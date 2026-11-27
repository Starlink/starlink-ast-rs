# Upstream sync to `fe26cc9b`, stage 3 (Regions) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Region grids, masks, overlaps, bounds and the Ellipse, Polygon, Prism and CmpRegion classes match the C library at `fe26cc9b`, port `testregionhardening.c` and `testregionlogic.c`, and remove the panics, the hang and the stack overflow the triage found.

**Architecture:** Each fix task adds its own cases to the two ported test files, red first, so no case that hangs or overflows the stack is ever run before its fix.
Task 1 ports the cases that already match C.
Each upstream fix is ported into the Rust function that ports the C function it changes: `FrameGrid` per Frame class, `Mask<X>`, `OverlapX` and `BndBaseMesh`, Interval and Prism `Overlap`, CmpRegion `GetBounded`, Ellipse `RegCentre`, and Polygon `Cache` and `Polywidth`.

**Tech Stack:** Rust 2024 (`starlink-ast`), C probes against `reference/starlink-ast/build`, `audit-tools`.

**Spec:** `docs/superpowers/specs/2026-11-22-upstream-sync-fe26cc9b-design.md` (Stage 3, "Per-stage method", "Rules settled during review").

## Global Constraints

- Branch `u/timj/sync-c-regions`, branched from the tip of `u/timj/sync-c-fitschan` (the user's choice). Never push. Fixups with `git commit --fixup`. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, no session line.
- Shell is zsh; `unset CC CFLAGS CPPFLAGS LDFLAGS CXX` for cargo and probe builds. Probes link `cc p.c -I$B -L$B -last -Wl,-force_load,$B/libast_err.a -lm -Wl,-rpath,$B` with `B=reference/starlink-ast/build`; to capture messages, define `astPutErr_` and drop `libast_err.a`.
- Never modify the submodule's tracked files.
- Limits that exist only because C had a fixed buffer are not mirrored; each gets a one-line `c-library-quirks.md` entry. Documented limits are mirrored.
- Error parity for a `testregionhardening.c` case is the status code and every message C reports, in order, except C's first `AST: Error in routine <caller> at line N in file <file>.` line.
- A user-visible change gets a `CHANGELOG.md` `Unreleased` entry in the same commit; a mirrored quirk upstream fixed gets a `RESOLVED (upstream <hash>)` entry with a Contents line.
- New source files carry the AURA copyright and SPDX header; a ported C test names its ancestor and divergences in its header and joins `crates/starlink-ast/tests/regions/main.rs`.
- An exclusion list is a relaxation: add it to `test_verification_ledger.rs` `LEDGER` and `PLAN.md` §6 with its size, and update both whenever entries leave.
- Run `cargo fmt` before `reanchor-citations --rust-only --write`, and the citation gate after both, right before each commit.
- Stage end: `cargo test -p starlink-ast --no-fail-fast`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS=-D warnings cargo doc -p starlink-ast --no-deps`, `tools/fixtures/check_mirror.sh`, `tools/oracle_fixtures/check_c_headers_fresh.sh`, `tools/oracle_fixtures/check_probes_fresh.sh`, and `cargo run --release -p ast-bench -- bits --base main`.

## Triage

From each commit's diff, the Rust site, C probes and a throwaway port of every case of the two C tests (2026-11-23).
The C tests both pass at the pin.

| commit | verdict | what changes | Rust site | task |
|---|---|---|---|---|
| `3bf19b0e` | mirror (panic, hang), missing (CmpFrame product) | `FrameGrid` leaves zero widths out of the mean, counts in doubles and refuses a product over `INT_MAX` (AST__ATTIN); SkyFrame caps parallels at `2*size+2` and gives a zero-length one no samples; CmpFrame checks its product | `frames/mod.rs:3311` `frame_grid_default`; `frames/skyframe.rs:2604-2636`; `frames/cmpframe.rs:923` | 2 |
| `ba6a0a62`, `27beb5a0` | already (typed setters), blocked (attribute strings) | a setting a Region passes to its FrameSet is not a printf format, and keeps its commas and quotes | `regions/data.rs:745` `set_region_frame_attr`; the attribute-strings layer has no Region classes | 8 |
| `64c4401a` | mirror (panic in debug, old C in release) | `Mask<X>` compares the grid bounds as doubles before converting | `regions/masking.rs:298` | 3 |
| `8b86f10a` | already (behavior), message differs | PointList `Mask<X>` does not read a missing Region | `regions/masking.rs:388` | 3 |
| `07517ca8` | already | `MapRegion` into fewer axes | `regions/mod.rs:1771` | 1 |
| `3b775b35` | mirror (INTER where C overran) | `OverlapX` bounds the other Region's mesh in reg1's current Frame | `regions/overlap.rs:146`, `:193-206`, `:322` | 4 |
| `93432265` | missing (stack overflow) | `BndBaseMesh` reports AST__INTER when neither the Region nor its negation is bounded | `regions/overlap.rs:390` `region_bnd_base_mesh` | 4 |
| `59aaa1be` | already | PointList `RegPins` | `regions/pointlist.rs:369` | 1 |
| `9b5a63a6` | already (PointList, Prism axes), mirror latent (NullRegion), missing latent (Prism `INT_MAX` mesh) | arrays sized by the right Frame | `nullregion.rs:340`; `prism.rs:437` | 8 |
| `bc374b82` | mirror (panic) | Interval `Overlap` gives 6 for an axis of mutual negation only if no other axis has a limit, else 1 | `regions/interval.rs:519` | 4 |
| `3046b37e` | already (indexing), mirror (same centre) | Ellipse `RegCentre` leaves the points alone when the centre does not move | `regions/ellipse.rs:1160` | 6 |
| `fbac9f50`, `3e7c4051` | already | Ellipse and Circle caches after an error; no best Ellipse without a point away from the centre | `ellipse.rs:72`, `:602`; `circle.rs:383` | 1 |
| `832be0e3` | missing | Polygon `Cache` starts the inside point at the first vertex | `regions/polygon.rs:1548` | 7 |
| `aa29054e` | missing (Cache error), stage 5 (loader) | `Cache` reports AST__BADIN for an edge with no description and keeps no cache; `astLoadPolygon` refuses an undefined vertex (AST__LDERR) | `regions/polygon.rs` `compute_cache`; `from_node` | final review |
| `ec7d8586` | mirror (hang) | `Polywidth` caps its start length at `DBL_MAX` and ends at zero length | `regions/polygon.rs:49-58` | 7 |
| `f19880b3` | already | Box `Simplify` with no Polygon | `regions/box_region.rs:514` | — |
| `b222d947` | already | moved Interval, Circle and Prism caches | `interval.rs:1542`, `circle.rs:985`, `prism.rs:974` | 1 |
| `cd1ca2b7` | mirror | Prism `Overlap` uses the Regions picked from a negated Region un-negated | `regions/prism.rs:287-313` | 5 |
| `3cce24ce` | mirror | CmpRegion `GetBounded`: an AND of two unbounded Regions is unbounded | `regions/cmpregion.rs:497-508` | 5 |
| `b73010f9` | already | CmpFrame `NormBox` order (the port's predicate is order-free) | `frames/cmpframe.rs:1004` | — |
| `ad2b515b` | mirror | "bacause" → "because" | `cmpregion.rs:727`, `:735`; `prism.rs:405` | 5 |
| `1df4a93a` (Region halves) | n/a, memory | a failed Transform no longer annuls the caller's PointSet; owned PointSets cannot be freed under the caller | — | — |

Side finding for Task 7: `frames/mod.rs:2060` `line_def` treats an infinite coordinate as bad, where C's `LineDef` tests only `AST__BAD`, so a Polygon with an infinite vertex fails with AST__INTER where C makes it.
Open quirk "`CmpFrame::NormBox` can never widen a box" stays open: `b73010f9` fixes the order, not the widening.
Recorded, not ported: "The order in which a Box's simplification tries the corners of a Polygon" (astjava; latent unless both orders qualify).

## Review Focus

1. A Region of any shape (no width, no height, a billion to one, an axis with no limit) used for a grid, a mask, a mesh or an overlap returns C's result or C's error, and never panics, hangs or overflows the stack. Pinned case by case in Tasks 2–7.
2. An overlap of two Regions of different dimensions, or where one has no mesh, gives C's code or C's AST__INTER message, in both orders. Pinned in Task 4.
3. A Polygon of coincident vertices, of vertices near `1e308`, or with an infinite vertex is made and transforms as C's. Pinned in Task 7.
4. A Region used as another's uncertainty, moved to the centre it already has, keeps its shape. Pinned in Task 6.
5. A CmpRegion and a Prism combined with negated or unbounded Regions report C's Bounded, bounds and overlap. Pinned in Task 5.

---

### Task 1: Port the two C tests' cases that already match

**Files:** Create `crates/starlink-ast/tests/test_regionhardening.rs`, `crates/starlink-ast/tests/test_regionlogic.rs`; modify `crates/starlink-ast/tests/regions/main.rs`, `PLAN.md` tracker rows.

**Interfaces:** Produces one `#[test]` per C section (`grids`, `masks`, `overlaps`, `axes`, `ellipses`, `polygons`, `fewer_axes`, `attributes`; `moves`, `prisms`, `cmpregions`), each holding the cases ported so far, which later tasks extend; a shared `fn expect_err(r: AstResult<T>, code: ErrorCode, messages: &[&str])` comparing the code and the message list.

- [ ] **Step 1:** Port the matching cases, each with C's value from the pinned C test or a probe: grids 2-D no width with other Units (10), 3-D CmpFrame (18), sky box with no height (28); masks with no lower limit (3, 6); overlaps PointList×Box (2), PointList×PointList (5), CmpRegion×CmpRegion (5), 1-D Intervals (6); axes PointList bounds `[1],[3]`, NullRegion bounds, Prism mesh in 4-D; fewer axes Box, Circle, PointList (Mapping unchanged, masks 4, 6, 3); ellipses k=0 move, collinear points twice (BADIN, same message), mapped Circle giving a Circle; logic `moves` (4.9, 5.1), Prism×Box (2,3), two equal Prisms (5,5).
- [ ] **Step 2:** Run `cargo test -p starlink-ast --test regions test_regionhardening:: test_regionlogic::`. Expected: PASS. A failure is a finding to triage before going on.
- [ ] **Step 3:** Tracker rows "partly ported (stage 3 in progress)". Commit ("Port the cases of testregionhardening.c and testregionlogic.c that already match").

### Task 2: Frame grids (`3bf19b0e`)

**Files:** `frames/mod.rs` (`frame_grid_default`), `frames/skyframe.rs`, `frames/cmpframe.rs`, `test_regionhardening.rs`, CHANGELOG.

- [ ] **Step 1: Tests** (values from C): 2-D and 3-D Box with no width give C's point counts; the 1e12×1e-12 Box gives AST__ATTIN "astFrameGrid(Frame): The supplied bounds and grid size (30) need a grid of more points than can be created."; the narrow sky box and the sky box with no width give AST__NPTIN "astSetNpoint(PointSet): Number of points (0) is not valid." then "Should be in the range 1 to 60."; a CmpFrame grid over `INT_MAX` points gives C's ATTIN. Run each under a thread with a 10 s timeout and `catch_unwind`. Expected: FAIL (panics, one hang).
- [ ] **Step 2:** Port frame.c `FrameGrid` (zero widths left out of `nsame` and `gmean`, `gmean` 1 when none; ticks and product as `f64`, `!(dtick >= 2.0)` gives 2; product over `i32::MAX` is ATTIN), skyframe.c `FrameGrid` (`maxpar = 2*size+2`, guarded `ipmax`, `nmer = 0` unless `dmer >= 1`; an empty grid is C's NPTIN pair, not a panic in `PointSet::from_rows`) and cmpframe.c's product check.
- [ ] **Step 3:** Run; CHANGELOG "Fixed" (panics and a hang). Commit ("Limit the points of a Frame grid as the C library does").

### Task 3: Masks (`64c4401a`, `8b86f10a`)

**Files:** `regions/masking.rs`, `test_regionhardening.rs`, CHANGELOG.

- [ ] **Step 1: Tests:** masking outside an Interval with no upper limit gives 2 and `1100000000`, inside gives 7 and `0001111111`; a 2-D PointList on a 1-D grid gives AST__NGDIN with "astMaskI(PointList): Bad number of input grid dimensions (1)." then "The PointList given requires 2 coordinate values to specify an input position.". Expected: FAIL (panic; one message).
- [ ] **Step 2:** Port region.c `Mask<X>`'s bounds loop (`dlo = round(lbndgd) - 2`, `dhi = round(ubndgd) + 2` as `f64`; disjoint gives no pixels; convert only inside the array) and split the NGDIN message into C's two, naming `astMask<X>(<class>)`.
- [ ] **Step 3:** Run; CHANGELOG "Fixed". Commit ("Mask with an unbounded Region as the C library does").

### Task 4: Overlaps (`3b775b35`, `93432265`, `bc374b82`)

**Files:** `regions/overlap.rs`, `regions/interval.rs`, `test_regionhardening.rs`, CHANGELOG.

- [ ] **Step 1: Tests:** a half-line mapped into 2-D against a Box gives 3, the reverse 2; Circle×Prism (the Prism with an unbounded Interval) and Prism×Circle give AST__INTER "astOverlap(Prism): The current implementation of astOverlap cannot determine the overlap between the supplied Regions (a Prism and a Circle)." (each order with C's class names); two 2-D Intervals with an axis of mutual negation give 6 with no other limit, 1 with one. Expected: FAIL (INTER, a stack overflow, panics). Add the stack-overflow case last and run it only after Step 2's `BndBaseMesh` check, so the program does not abort.
- [ ] **Step 2:** In `region_bnd_base_mesh`, port region.c `BndBaseMesh`'s AST__INTER when neither the Region nor its negation is bounded; in `overlap_x`, drop the bounds pass in the first loop and bound reg2's mesh, transformed into reg1's current Frame, there (region.c `OverlapX`, `ps6`); give the `overlap.rs:326` message C's class names; port interval.c `Overlap`'s `nolim`/`prevnolim` handling before indexing the result table.
- [ ] **Step 3:** Run; CHANGELOG "Fixed" (a stack overflow, a panic, wrong overlaps). Commit ("Find Region overlaps as the C library does").

### Task 5: Prism and CmpRegion (`cd1ca2b7`, `3cce24ce`, `ad2b515b`)

**Files:** `regions/prism.rs`, `regions/cmpregion.rs`, `test_regionlogic.rs`, `tests/test_cmpregion.rs`, quirks, CHANGELOG.

- [ ] **Step 1: Tests:** Prism×negated Box gives (1,1), Prism×its negation (6,6); the AND of x≥0 and x≥1 is not bounded and its bounds are lbnd (1, -1.797e308), ubnd (1.797e308, 1.797e308); the CmpRegion "neither of its component Regions has a mesh" message spells "because"; invert `and_of_nested_unbounded_regions_reports_bounded_as_c_does` (`test_cmpregion.rs:809`) and rename it. Expected: FAIL.
- [ ] **Step 2:** In `prism_overlap`, un-negate the Regions picked from a negated `that` before mapping them (prism.c `Overlap`); the AND branch of `get_bounded` with neither component bounded returns false without an overlap (cmpregion.c `GetBounded`); spell "because" at the three sites.
- [ ] **Step 3:** Run; quirk "A CmpRegion that ANDs two nested unbounded Regions reports itself as bounded" to RESOLVED (upstream 3cce24ce); CHANGELOG "Fixed". Commit ("Combine Prisms and CmpRegions with unbounded or negated Regions as C does").

### Task 6: An Ellipse moved to its own centre (`3046b37e`)

**Files:** `regions/ellipse.rs`, `test_regionhardening.rs`, CHANGELOG.

- [ ] **Step 1: Test:** the Ellipse (0.2, 0.1, 0.3) at (5,5) as the uncertainty of the Box (0,0)–(10,10) gives bounds 4.8877…5.1123 and 4.8067…5.1933 (C's digits from a probe). Expected: FAIL (BADIN "astInitEllipse(Ellipse): The supplied points do not determine an ellipse.").
- [ ] **Step 2:** In `reg_centre_set`, shift the points and invalidate the cache only when the distance is good and non-zero (ellipse.c `RegCentre`).
- [ ] **Step 3:** Run; CHANGELOG "Fixed". Commit ("Leave an Ellipse alone when it is moved to its own centre").

### Task 7: Polygons (`832be0e3`, `ec7d8586`, the infinite vertex)

**Files:** `regions/polygon.rs`, `frames/mod.rs` (`line_def`), `test_regionhardening.rs`, CHANGELOG.

- [ ] **Step 1: Tests** (C probe values): three vertices at (1,2) make a Polygon that is bounded, has bounds (1,2,1,2), overlaps itself with 5, and transforms (0,0) and (-1,-1) to bad with no error; vertices (0,0), (1e308,0), (0,1e308) make a Polygon whose Tran2 keeps (0,0), makes (-1,-1) bad, and is bounded (10 s timeout); the same with one vertex at infinity gives C's result. Expected: FAIL (INTER, a hang, INTER).
- [ ] **Step 2:** In `compute_cache`, start the inside point at the first vertex (polygon.c `Cache`); in `polywidth`, cap the start length at `f64::MAX`, loop while it is positive, and return (BAD, the edge's midpoint) when no length is usable (polygon.c `Polywidth`); in `line_def`, test a coordinate for `BAD_VALUE` or NaN only, as C's `LineDef` tests `AST__BAD` and the port reads NaN as bad on input.
- [ ] **Step 3:** Run; CHANGELOG "Fixed" (a hang). Commit ("Make Polygons of coincident, distant or infinite vertices as C does").

### Task 8: What stays open (`ba6a0a62`, `27beb5a0`, `9b5a63a6`)

**Files:** `regions/nullregion.rs`, `test_regionhardening.rs`, `test_verification_ledger.rs`, `PLAN.md`, quirks.

- [ ] **Step 1:** Port `test_attributes` through the typed setters (`set_title`, `set_format`; six Titles × Adaptive 0/1 keep each value exactly and Domain stays OLD). The attribute-string form goes in `REGION_HARDENING_EXCLUDED` with the reason "the attribute-strings layer has no Region classes", and `excluded_cases_still_differ` fails when one matches; ledger and §6 row (size 1); a `PLAN.md` row for Region attribute strings naming `ba6a0a62` and `27beb5a0` (C's `\r` escaping and quoting in region.c `SetAttrib`) as what the port must reproduce when they land.
- [ ] **Step 2:** NullRegion `get_region_bounds` sizes its bounds by the current Frame's axes (nullregion.c, `astGetNout`), with a unit test, since no public input reaches it; a `PLAN.md` stage 5 row for Prism's `INT_MAX` mesh check (`9b5a63a6`), which needs a MeshSize near `INT_MAX`.
- [ ] **Step 3:** Commit ("Record what stage 3 leaves open").

### Task 9: Close the stage

- [ ] **Step 1:** Tracker rows "ported" (testregionhardening.c with its one exclusion); `PLAN.md` §8 stage 3 done, with the verdicts (`07517ca8`, `59aaa1be`, `fbac9f50`, `3e7c4051`, `f19880b3`, `b222d947`, `b73010f9` already; `1df4a93a` n/a) and the astjava entries resolved, for that port to follow.
- [ ] **Step 2:** Stage-end gates from Global Constraints; `bits` should differ from `main` only by stages 0–2's known cases and any Region transform Tasks 6–7 change, each explained. Commit ("Record stage 3 of the fe26cc9b sync as done").

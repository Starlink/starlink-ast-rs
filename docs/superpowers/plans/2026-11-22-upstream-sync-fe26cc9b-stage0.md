# Upstream sync to `fe26cc9b`, stage 0 (pin move) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the `reference/starlink-ast` pin from `3d3f255f` to `fe26cc9b` with every tier-1 gate, the fixture mirror and the wcslib golden green, port the two behavior changes the mirrored fixtures need, and record every stale corpus for the stage that owns it.

**Architecture:** Mechanical sync work (pin, manifest, citation re-anchor, fixture copy, golden recapture, freshness sweeps) plus two small C-faithful ports: FitsChan `SpecTrans`'s zero-CDELT replacement (`45ae1bd8`, `41e2f2c3`) and WcsMap `LongRange`'s cyclic test (`a87d1700`). A new script makes the ~130 generated C-probe oracles checkable after any pin move.

**Tech Stack:** Rust 2024 (`starlink-ast`), C probes in `tools/oracle_fixtures/` linked against a cmake build of the submodule, bash, `audit-tools`.

**Spec:** `docs/superpowers/specs/2026-11-22-upstream-sync-fe26cc9b-design.md` (stage 0 section, "Rules settled during review").

## Global Constraints

- Branch `u/timj/sync-c`. Never push. Fixups with `git commit --fixup`. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, no session line.
- Shell is zsh: quote globs passed to programs, `$VAR` is not word-split. Strip conda's `CC`/`CFLAGS`/`LDFLAGS` for cargo and probe builds (`env -u CC -u CFLAGS -u LDFLAGS -u CXX …`).
- Reference build: `cd reference/starlink-ast && cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DAST_BUILD_FORTRAN=OFF -DCMAKE_C_FLAGS=-ffp-contract=off && cmake --build build -j 18`; `otool -tv build/libast.dylib | grep -c fmadd` must print 0.
- Never modify the submodule's tracked files.
- New source files open with the two-line AURA copyright / `SPDX-License-Identifier: LGPL-3.0-or-later` header.
- Docs: one sentence per line, American spelling, no C file/line citations in rendered docs (keep provenance in `//` comments).
- A user-visible change gets a `CHANGELOG.md` `Unreleased` entry in the same commit; every C oddity found gets a `docs/issues/c-library-quirks.md` entry in the same commit.
- Stage end: `cargo test -p starlink-ast --no-fail-fast`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, the docs build, and `tools/fixtures/check_mirror.sh` all pass.

## Review Focus

1. A zero CDELT on an alternate axis description (`CDELT2A = 0`) must take its replacement from `CRVAL2A`, not the primary `CRVAL2`. Pinned in Task 2.
2. A `CRVAL` that is not a number, or is absent, gives a replacement of 1.0, not an error. Pinned in Task 2.
3. A `CRVAL` small enough that a millionth of it underflows to zero (`1e-320`) gives 1.0. Pinned in Task 2.
4. A negative `CRVAL` gives a negative CDELT (C multiplies; no `abs`). Pinned in Task 2.
5. A cyclic projection with usable parameters (CAR) must stay cyclic after the `long_range` change, so LonCheck behavior for good WcsMaps is unchanged. Pinned in Task 3.

---

### Task 1: Move the pin and re-anchor the citation audit

At the new pin the only tier-1 failures are the two `test_audit_citations` gates (verified 2026-11-22): 121 citations that no longer name their item and 430 cited-body C steps not covered by a row, almost all from line drift.

**Files:**
- Modify: `reference/starlink-ast` (gitlink only), `crates/starlink-ast/tests/fixtures/DIST_MANIFEST`
- Modify: `docs/audits/mapmerge/*.md` (rows re-anchored, `Uncited C steps:` additions)

**Interfaces:**
- Produces: a built `reference/starlink-ast/build/` at `fe26cc9b`, used by every later task.

- [ ] **Step 1: Build the reference library at the pin** with the Global Constraints command; check the `fmadd` count is 0.

- [ ] **Step 2: Copy the manifest line.** Copy upstream's `ast_tester/fixtures/DIST_MANIFEST` over the crate's (it adds `serialisation/c_dssmap.ast` with its comment). Run `cargo test -p starlink-ast --test meta test_dist_manifest::` — expected PASS.

- [ ] **Step 3: Confirm the gates fail.** Run `cargo test -p starlink-ast --test meta test_audit_citations::` — expected FAIL in `every_repaired_citation_resolves_to_the_item_it_names` and `every_c_step_in_a_cited_body_is_covered_by_a_row`.

- [ ] **Step 4: Dry-run the C re-anchor.** `cargo run -q -p audit-tools --bin reanchor-citations -- --warn-stuck docs/audits/mapmerge/*.md` (no `--rust-only`: the rows' recorded pin, `3d3f255f`, is in the local submodule, so C citations can be followed through the diff). Read the proposed rewrites; every relocation must land in the function the row names.

- [ ] **Step 5: Write it**, same command with `--write`. If any C citation comes out wrong, `git checkout -- docs/audits/mapmerge/` and re-anchor that checklist by hand instead.

- [ ] **Step 6: Re-anchor the stuck rows by hand.** For each row the tool reports, find the function the row describes at the new pin and cite it there. The gate's rule: "Re-anchor the row against the code it describes; do not remap the number."

- [ ] **Step 7: Classify the remaining uncovered C steps.** Re-run the gate. For each step still reported: if an existing row's Rust covers it, extend that row's citation range; if it is C code new in this range (the merge fixes of `b9cf680b`, `f8c7723e`, `f0230815`, `e7cb7d95`, `78c0f559`, `8cad1a4f`, `e6141f23`, `2a84cade`, `3438acbc`), list it under that checklist's `Uncited C steps:` as `<file>:<line> (<fn>) — new at fe26cc9b (<commit>); stage 1 ports it`.

- [ ] **Step 8: Run the gates.** `cargo test -p starlink-ast --test meta` — expected PASS.

- [ ] **Step 9: Commit** the gitlink, `DIST_MANIFEST` and the checklists: `Move the reference/starlink-ast pin to fe26cc9b`. The message body names the two upstream PRs (#103, #104) and the design doc.

### Task 2: Zero CDELT takes its replacement from CRVAL

Upstream `45ae1bd8`: a zero CDELT becomes `1.0e-6 * CRVAL` of the same axis and version, or 1.0 if that is zero, CRVAL being 0.0 when absent, bad or non-finite. Upstream `41e2f2c3`: only a CDELT that was read and is zero is replaced; the port's `let Some(cdeltj) = cards.get_float(&key) else { continue }` already matches, so this task only pins it with a test.

**Files:**
- Modify: `crates/starlink-ast/src/fitschan/spec_trans.rs` (`translate_zero_cdelt`, ~line 280-310; the comment at ~135; unit tests ~2230)
- Copy from upstream: `tests/fixtures/wcsconv/expected/zero-cdelt.native`, `tests/fixtures/wcsconv/framesets/zero-cdelt-head.ast`, `tests/fixtures/oracle/{framesets,headers,simplify_fixtures}.oracle`
- Regenerate: `crates/starlink-ast/tests/fixture_full_dump_oracle/cases.rs`, `crates/starlink-ast/tests/fixture_read_warnings_oracle/cases.rs` (commands in each probe's header comment)
- Modify: `docs/issues/c-library-quirks.md`, `CHANGELOG.md`

- [ ] **Step 1: Replace the unit test** `zero_cdelt_is_replaced_with_one` with `zero_cdelt_uses_a_millionth_of_crval`:

```rust
// CDELT1 non-zero, untouched; CDELT2 = 0 with CRVAL2 = 180 -> 1.8e-4;
// CDELT3 = 0 with CRVAL3 = -45 -> -4.5e-5; CDELT4 = 0, no CRVAL4 -> 1.0;
// CDELT5 = 0 with CRVAL5 = 'abc' -> 1.0; CDELT6 = 0 with CRVAL6 = 1e-320 -> 1.0.
assert_eq!(cards.get_float("CDELT1"), Some(1.0e-10));
assert_eq!(cards.get_float("CDELT2"), Some(1.0e-6 * 180.0));
assert_eq!(cards.get_float("CDELT3"), Some(1.0e-6 * -45.0));
assert_eq!(cards.get_float("CDELT4"), Some(1.0));
assert_eq!(cards.get_float("CDELT5"), Some(1.0));
assert_eq!(cards.get_float("CDELT6"), Some(1.0));
```

and extend `zero_cdelt_handles_alternate_versions` with `CRVAL1A = 2.0E6` and `CRVAL1 = 7.0`, asserting `CDELT1A == Some(1.0e-6 * 2.0e6)`. Add `missing_or_non_numeric_cdelt_is_not_replaced`: a header with `CDELT2 = 0.0`, `CRVAL2 = 10.0`, no `CDELT1`, and `CDELT3 = 'x'` gives `find("CDELT1")` none, `CDELT2 == Some(1.0e-5)`, and `CDELT3` still the string card.

- [ ] **Step 2: Run them.** `cargo test -p starlink-ast --lib fitschan::spec_trans::` — expected FAIL on the CRVAL assertions; `missing_or_non_numeric_cdelt_is_not_replaced` should already pass. If it does not, `get_float` treats a string card differently from C's `GetValue2` (no error, not read) and that is part of this task.

- [ ] **Step 3: Implement** in `translate_zero_cdelt`: on a zero `cdeltj`, read `CRVAL{j}{suffix}` with `get_float`, treat `None`, `BAD_VALUE` and non-finite as 0.0, set `1.0e-6 * crval`, and 1.0 if that is 0.0. Update the comment block at ~135 and the inline comment to cite `SpecTrans` at the new pin.

- [ ] **Step 4: Run the unit tests** — expected PASS.

- [ ] **Step 5: Copy the five fixture files** from `reference/starlink-ast/ast_tester/fixtures/` and run `tools/fixtures/check_mirror.sh` — expected PASS (no differences).

- [ ] **Step 6: Regenerate the two fixture oracles** by building and running `fixture_full_dump_probe` and `fixture_read_warnings_probe` exactly as their header comments say. `git diff --stat` must show only the `zero-cdelt` rows changed; anything else is a stale reference for Task 4, so revert it here and note it.

- [ ] **Step 7: Run the whole suite.** `cargo test -p starlink-ast --no-fail-fast` — expected PASS, including `test_wcsconv`, `test_transform_oracle`, `test_fixture_full_dump_oracle`, `test_fixture_read_warnings_oracle`.

- [ ] **Step 8: Record it.** Add a `### RESOLVED (upstream 45ae1bd8): SpecTrans replaces a zero CDELT with 1.0 whatever CRVAL is` entry to `c-library-quirks.md` saying the port mirrored the 1.0 until this commit, and a `CHANGELOG.md` `Unreleased` "Changed" entry: a FITS header with a CDELT of zero now gets a pixel size of a millionth of the axis's CRVAL (1.0 if CRVAL is zero or absent), as the C library now does.

- [ ] **Step 9: Commit**: `Replace a zero CDELT using CRVAL, as upstream 45ae1bd8 does`.

### Task 3: WcsMap's cyclic test needs both test positions

Upstream `a87d1700`: wcslib's set-up functions now record success only after validating the parameters, so a WcsMap with unusable parameters fails every time, which the port already did (quirk "A WcsMap reports unusable parameters only on its first transformation", never mirrored). `LongRange` changed with it: the cyclic test compares the two positions only if both project, and the second is not projected if the first fails.

**Files:**
- Modify: `crates/starlink-ast/src/mappings/wcsmap.rs` (`long_range`, ~line 700; unit tests)
- Modify: `crates/starlink-ast/src/wcslib/prj.rs` (module doc, lines ~24-27), the `at 3d3f255f` provenance comments in `src/wcslib/*.rs`
- Modify: `crates/starlink-ast/tests/test_wcsmap.rs` (comments of `unusable_parameters_are_an_error_on_every_transform`, ~1147)
- Modify: `crates/starlink-ast/tests/wcslib_golden/projections.tsv` (if the recapture changes it), `docs/issues/c-library-quirks.md`

- [ ] **Step 1: Write the failing unit tests** in `wcsmap.rs`'s `#[cfg(test)]` module: `long_range_is_not_cyclic_when_projection_fails` (AZP with `PV2_1 = -1`: `long_range().0 == false`) and `long_range_of_car_is_cyclic` (CAR, default parameters: `long_range().0 == true`). Under `call-counts`, the first also asserts that one `long_range` call adds 1 to `wcs_fwd`, not 2.

- [ ] **Step 2: Run them.** `cargo test -p starlink-ast --lib mappings::wcsmap::` and again with `--features call-counts` — expected FAIL on the AZP test (today both statuses are ignored and two zero positions compare equal).

- [ ] **Step 3: Implement** in `long_range`: count and run the second `fwd_raw` only when the first returns `Status::Ok`; `cyclic` is false unless both are `Ok` and the positions agree within `1.0e-10`. Leave the `bb` update as it is unless a probe shows C's `WcsRev` leaves `bb` unchanged on a failing point where `rev_raw` returns a value; if it does, that is a quirk entry and a fix in this task.

- [ ] **Step 4: Run the WcsMap tests.** Unit tests, then `cargo test -p starlink-ast --test mappings test_wcsmap::` — expected PASS.

- [ ] **Step 5: Confirm C now agrees** on the cases `unusable_parameters_are_an_error_on_every_transform` covers (forward first, inverse first, repeated), with a probe against the Task 1 build: each must raise `AST__WCSPA`. Rewrite that test's comments, which say the port and C differ there, to say they agree.

- [ ] **Step 6: Recapture the golden.** `./tools/oracle_fixtures/recapture_wcslib_golden.sh --check`; if it fails, run it without flags and inspect the diff (expected: the header commit, plus any `E` line set whose shape changed). Then `cargo test -p starlink-ast --lib wcslib::golden_tests::` — expected PASS.

- [ ] **Step 7: Update the docs.** In `prj.rs` replace the paragraph saying C runs the bodies on half-initialized constants after a failed set-up: C and the port now both report the set-up status on every call. Move the provenance comments to `fe26cc9b`. Retitle the quirk `### RESOLVED (upstream a87d1700): A WcsMap reports unusable parameters only on its first transformation` and move it with the other resolved entries, adding one line on the `LongRange` change.

- [ ] **Step 8: Commit**: `Treat a projection as cyclic only if both test positions project`.

### Task 4: Check every C-probe oracle against the new pin

About 130 `crates/starlink-ast/tests/*_oracle/cases.rs` files are generated by `tools/oracle_fixtures/*_probe.c`, each documenting its own command in its header comment. No tool checks them after a pin move, so a behavior change the existing tests exercise is invisible until someone regenerates by hand.

**Files:**
- Create: `tools/oracle_fixtures/probes.tsv` (columns: `probe`, `run_dir` relative to the repo root, `args`, `output` relative to the repo root; `#` comments allowed)
- Create: `tools/oracle_fixtures/check_probes_fresh.sh`
- Modify: `tools/oracle_fixtures/README.md` (new section), `CLAUDE.md` (sync protocol: run it after a pin move)

**Interfaces:**
- Consumes: the Task 1 reference build.
- Produces: `check_probes_fresh.sh [--write] [probe...]`. Without `--write` it builds the listed probes (default: all in `probes.tsv`) with `make`, runs each from its `run_dir` with stderr discarded, and diffs stdout against `output`. It prints one line per probe, `<probe> SAME` or `<probe> DIFFERS <n> lines`, and exits 1 if any differ or any probe fails to run. With `--write` it overwrites `output` instead of diffing.

- [ ] **Step 1: Build `probes.tsv`** from every `cases.rs` whose first line is `// Generated by tools/oracle_fixtures/<probe>.c`, taking the command from that probe's header comment. A probe whose header gives no usable command is listed as a `#` comment with the reason.

- [ ] **Step 2: Write the script.** Self-test it at the old pin: check out `3d3f255f` in a scratch worktree of the submodule (`git -C reference/starlink-ast worktree add <scratchpad>/ast-old 3d3f255f`), build it, and run the script with `AST_REFERENCE` set to that worktree; the script passes it to `make` as `REF=$AST_REFERENCE` (a command-line variable overrides the Makefile's `REF`) and defaults to `reference/starlink-ast`. Every probe must report `SAME`; a `DIFFERS` there is a stale or platform-captured `cases.rs` and is listed separately, not hidden.

- [ ] **Step 3: Run it at the new pin.** Save the output. For each `DIFFERS`, read the diff and assign the stage that owns the behavior change, using the commit lists in the design. Do not regenerate here; the owning stage regenerates when it ports the fix (provenance rule: regenerate the whole module then).

- [ ] **Step 4: Document** the script in the README beside `check_corpus_fresh.sh` and add it to the `CLAUDE.md` upstream-sync paragraph.

- [ ] **Step 5: Commit**: `Add a freshness check for the generated C-probe oracles`.

### Task 5: Sweep the remaining corpora and record the sync in PLAN.md

**Files:**
- Modify: `PLAN.md` (§8 and the C → Rust test migration tracker)

- [ ] **Step 1: Run the sweeps** and save each output: `./tools/oracle_fixtures/check_corpus_fresh.sh`, `./tools/oracle_fixtures/check_simplify_fresh.sh`, `./tools/oracle_fixtures/check_c_headers_fresh.sh` (expected clean), `tools/fixtures/check_mirror.sh` (expected clean). The range does not touch `pal/`, `erfa/`, `palwrap.c` or `sun_master.tex`; confirm with `git -C reference/starlink-ast diff --stat 3d3f255f..fe26cc9b -- pal erfa src/palwrap.c sun_master.tex` (expected empty).

- [ ] **Step 2: Triage.** Per the README classifications: a `CARD-ONLY` or `MIXED` corpus case, or a simplify `card` row, whose cause is a stage 1–5 commit is assigned to that stage; one with no cause in the range is investigated now (it predates the range). `NUMERIC` and `numeric` rows are tolerated as the README says. Any case that makes tier 1 fail goes in the matching named exclusion list with its reason and companion test, per the design.

- [ ] **Step 3: Write `PLAN.md` §8** "In progress: sync to `fe26cc9b`": the design doc link; stage 0 done with its commits; a table of stages 1–5 (area, branch suggestion `u/timj/sync-c-<area>`, status pending); a table of every stale reference found in Tasks 4 and 5, with corpus, case, classification and owning stage. Leave the PR #107 "Pending" subsection as it is.

- [ ] **Step 4: Add tracker rows** for the 15 new C programs (`testcast`, `testcorruptinput`, `testdssmap`, `testfields`, `testfitswrite`, `testframeperm`, `testframesetload`, `testloaderhardening`, `testmapmerge`, `testplotclip`, `testplotcopy`, `testpolymapinv`, `testregionhardening`, `testregionlogic`, `testsplinemapedge`), each `not ported — stage <n>` with the stage the design assigns, and one for `compare_dumps_selftest.c` (not ported, as `compare_dumps.c` is not). Amend the existing `testcmpframe.c`, `testmoc.c` and `testxphmap.c` rows (add any that are missing): their checks new at `fe26cc9b` are unported (stage 4 for `testcmpframe`, stage 5 for `testmoc` and `testxphmap`, except the PR #107 checks).

- [ ] **Step 5: Run the stage-end gates** from Global Constraints. Expected: all pass.

- [ ] **Step 6: Commit**: `Record the fe26cc9b sync's stages and stale corpora in PLAN.md`.

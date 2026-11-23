# Upstream sync to `fe26cc9b` (C hardening) — design

Pin move `3d3f255f` → `fe26cc9b`: 168 commits, Starlink/ast PRs #103 (Frame permutation validation) and #104 (loader hardening sweep), 2026-11-05 to 2026-11-19.

## Intent

Upstream C AST was fuzzed with corrupt object dumps and FITS headers, and about sixty faults were fixed.
The same campaign, plus review of each class's merge and Region code, fixed a long list of behavioral defects: wrong `astSimplify` results, FitsChan write and read faults, Region bounds and overlap faults, date and Format validation, and casting between Frame classes.
`ast.news` in the pinned C describes them all, under "Bug fixes", between the AST__MXAXES entry and the MOC string-range entry.

The port should stop reproducing every C behavior that upstream fixed, and gain every check upstream added, so that:

1. results match the fixed C wherever the port follows C, and
2. no file handed to the port can crash it, make it allocate memory or run for a time out of proportion to the file's size, or recurse without bound.

The second goal is stronger than C's own.
C has no bound on the nesting depth of a dump, and the port has failure modes C's fuzzing could not find: index panics, `Vec::with_capacity` on an unchecked count, integer overflow (a panic in debug builds), and stack overflow, which aborts the process with no chance to report an error.

## Decisions already made

- **Staged, one PR per area** (the user chose this over a single sync branch and over hardening-first).
  Stage 0 is on the branch `u/timj/sync-c`.
- **Limits that exist only because C had a fixed buffer are not mirrored.**
  The port keeps Rust's growable strings: no default Label or Symbol cut at 200 characters, no error message cut at 1024, no Channel warning cut at 201.
  Limits that are documented behavior *are* mirrored: `AST__MXAXES` (65536), Format strings validated before use, `iso.n` capped at nine decimal places, `<bad>` for an undateable time or an unformattable sky value, and rejection of an invalid month or year.
  Each pure-buffer fix gets a one-line `c-library-quirks.md` entry saying the port has no such limit and why, so the decision is visible.
- **Hostile input must fail the way C fails.**
  For every case in the new C hardening tests, the port reports the same error code as C and the same message text.
  The C tests assert only the code; the message text is captured from C.
- **Rust is not limited to C's string handling.**
  Where C truncates or rejects only to protect a buffer, the port accepts the full text.

## Goals

1. Every behavioral fix in the range is either ported, or recorded as not applicable with the reason (C memory safety only; pure buffer truncation per the decision above).
2. Every C quirk the port mirrors and upstream fixed is inverted, and its `c-library-quirks.md` entry is marked `RESOLVED (upstream <hash>)`.
3. Each of the 16 new C test programs, and the new checks in `testcmpframe.c`, `testmoc.c` and `testxphmap.c`, has a Rust counterpart or a `PLAN.md` row naming what blocks it.
4. Error-code and message parity for every case of `testcorruptinput.c`, `testloaderhardening.c`, `testframeperm.c`, `testframesetload.c` and `testregionhardening.c`.
5. A Rust fuzzing harness over the readers, with every input it has found to crash, hang or over-allocate committed as a regression case that tier 1 replays.
6. Each stage leaves `cargo test -p starlink-ast`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, the docs gates and `tools/fixtures/check_mirror.sh` green.

## Non-goals

- Message-text parity for errors outside the hostile-input cases.
  Goal 4 covers the corpus that triggered the hardening, not every `AstError` in the crate.
- Starlink/ast PR #107 (Moc, XphMap, sky `LineIncludes`).
  It is not merged upstream; its `PLAN.md` §8 rows stay pending.
- Plot3D fixes (`de6e47a3`, and the Plot3D halves of `1df4a93a` and `32b62f90`).
  Plot3D is paused waiting for the C oracle; record the commits in its pending notes.
- XmlChan and YamlChan fixes (`71c02ba7`): the port has no XML or YAML reader.
- The ~300 entries of the Java port's `~/work/astjava/docs/rust-c-differences.md`, except where a stage's commits touch them (see Inputs).

## Anti-goals

- Copying a C buffer size into the Rust API.
- A hardening limit with no name, no error code and no documentation.
  Every port-only limit is a named constant, raises an `AstError`, and has a `c-library-quirks.md` entry stating that C has no such limit.
- A test that passes without comparing (CLAUDE.md "Comparators and skip modes").
  Cases not yet portable go in a named exclusion list with a per-case reason and a companion test that fails when one starts passing.
- A panic, abort or stack overflow on any input read from a file.

## Constraints

- Sync protocol from `CLAUDE.md`: check `c-library-quirks.md` for mirrors; run `check_corpus_fresh.sh` and `check_simplify_fresh.sh`; run `recapture_wcslib_golden.sh` because `wcslib/proj.c` changed; run `check_c_headers_fresh.sh` (clean at this pin); `sun_master.tex` is unchanged, so the guide needs no port.
- CI runs `fixture-mirror` on every PR, so the fixture tree must match the pin from stage 0 on.
- Architecture seams (H1): `3438acbc` adds a protected `astMtrSplitInputs` to MatrixMap, called from CmpMap's `MapSplit2`.
  Port it as a MatrixMap method at C's granularity, not as a special case inside CmpMap.
- `CHANGELOG.md` `Unreleased` entry for each user-visible change, in the same commit.
- C changes found necessary during the sync follow the `u/timj` submodule-branch precedent and get a `History:` entry.

## Starting state (verified 2026-11-22)

- `tools/fixtures/check_mirror.sh` differs in six files only: `wcsconv/expected/zero-cdelt.native`, `wcsconv/framesets/zero-cdelt-head.ast`, the `zero-cdelt` sections of the three `oracle/*.oracle` files (from `45ae1bd8`, CDELT of zero now becomes CRVAL × 1e-6), and one new `DIST_MANIFEST` line naming `serialisation/c_dssmap.ast`.
- `check_c_headers_fresh.sh`: clean.
- `wcslib/proj.c`: the AZP, SZP, ZPN, AIR, CYP, CEA, COP, COE, COD and COO set-up functions now set `prj->flag` only after the parameters pass, so a failed set-up fails every time (`a87d1700` and its `wcsmap.c` half).
  This retires the open quirk "A WcsMap reports unusable parameters only on its first transformation".
- The C fuzz harness and corpus lived in session scratch directories that have since been purged.
  The harness source and the campaign notes survive only in the C session transcript, `~/.claude/projects/-Users-timj-work-ast/f0576fb3-….jsonl` and its `subagents/` files, and in `~/.claude/projects/-Users-timj-work-ast/memory/`.
  Every input that crashed C is reproduced as a case in `ast_tester/testcorruptinput.c` or `testloaderhardening.c`.

## Approaches considered

1. **Staged, one PR per area (chosen).**
   Six PRs, each reviewable and each leaving `main` green.
2. **One sync branch, as at 9.5.0.**
   Rejected: the behavioral set is about three times 9.5.0's, too large to review well.
3. **Hardening and fuzzing first.**
   Rejected: much of what a Rust fuzzer finds would overlap fixes in stages 1–4 not yet landed, so findings would be triaged twice.

## Per-stage method

Each stage's plan opens with a triage table built by reading every assigned commit's diff and the Rust site it corresponds to, with one row per commit and one of four verdicts:

- **mirror** — the port reproduces the old C behavior; invert it.
- **missing** — the port never had the old or the new behavior; port the new.
- **n/a, memory** — a C memory-safety fix with no observable behavior; the port has no equivalent fault.
  Verify this rather than assume it: an observable side (an error raised, a value rejected) means the verdict is *missing*.
- **n/a, buffer** — a pure buffer truncation; not mirrored, per the decision above, with a quirks entry.

This is how the 9.5.0 sync found three fixes that were not mirrors at all.
`~/work/astjava/docs/rust-c-differences.md` is a cross-check: it lists differences between this port and C at `3d3f255f`, found while porting to Java, and its entries in a stage's area should be re-read against the fixed C during that stage's triage (for example, "Cancelling TDBTOTCB with TCBTOTDB in a TimeMap", "Cancelling a PolyMap with its inverse", and "PermMap with a null forward permutation and more outputs than inputs" are all touched by stage 1).

## Stages

Commit assignments below come from commit subjects and file lists; each stage's triage confirms or moves them.

### Stage 0 — pin move (`u/timj/sync-c`)

Mechanical, plus the minimum behavior needed to make the fixture mirror and golden gates pass.

- Commit the submodule pin.
- Port the CDELT fixes `45ae1bd8` (zero CDELT replaced by CRVAL × 1e-6, or 1.0 if CRVAL is zero or absent) and `41e2f2c3` (a missing or non-numeric CDELT is 1.0), then copy the six drifted fixtures.
- Port the `proj.c` set-up ordering and `a87d1700`; run `recapture_wcslib_golden.sh` and commit `projections.tsv` if it changes.
- Fix `test_audit_citations` for C line numbers that moved.
- Run `check_corpus_fresh.sh` and `check_simplify_fresh.sh`; any stale reference is recorded and handed to the stage that owns its cause rather than fixed here.
- Update `PLAN.md` §8: this sync's status, the stage list, and the five stages as pending rows.

### Stage 1 — Mappings and the simplifier

Commits: `b9cf680b` (MatrixMap/WinMap merge and swap), `f8c7723e` (CmpMap PermMap swap), `f0230815` (WcsMap merge), `e7cb7d95` (PermMap bad value for an absent `outperm`), `78c0f559` (PolyMap Invert after cancelling), `8cad1a4f` (TimeMap TDB/TCB cancel), `e6141f23` with its fixups `1bb7a75d`, `ce8a892c`, `45e598ba` (merger axis-count check, new `astSimplify` error), `2a84cade` (PolyMap `MergeShift`), `3438acbc` (MapSplit search, new `astMtrSplitInputs`), `7de5007a` (`astRate` near zero), `2d9d4449` (`astEQUAL` of infinity and NaN, which ended the simplify hang on a 1e-320 WinMap), `f584a552` (MathMap `Equal` compares every operation), `0f11fe09` (RateMap in its undefined direction), `0b427150`, `9a598e55`, `fd90f3b1` (SplineMap NaN, upper limit, domain), `857f4a99` (PolyMap iterative inverse requires a forward), `1df4a93a` (caller's PointSet on failure; expected *n/a, memory*, confirm).

Tests: `testmapmerge.c`, `testpolymapinv.c`, `testsplinemapedge.c`.
`78c0f559` concerns a shared PolyMap whose Invert changed under another user; under owned values this probably cannot happen, so expect *n/a* with a sentence in the stage notes saying why.

### Stage 2 — FitsChan

Commits: `6a00ff8f`, `afc179b0`, `68708cb5`, `01fc69d6`, `4e42485f`, `75d87b05`, `e809b988`, `8690ee52` (`Similar` compares the last character; WCSNAME and comment replacement change), `a6e42163` (lone celestial axis of a translated projection), `4ae0505f`, `31541c4a` (FITS-CLASS), `cbd04fa7`, `4175c735` (cards of a failed write: removed on error, overwritten cards put back), `07011b5f` (FitsDigits within a card), `ef779057`, `bd9ffbfc` (no DATE-OBS without a calendar date), `df9c0243` (WCSAXES 1–999, short CTYPE, WAT without a string), `5a3e23e3`, the FitsChan halves of `f38b4a9e` and `cfdbbe8d` (`astGetFitsI`/`K` refuse an out-of-range float).

Test: `testfitswrite.c`.

### Stage 3 — Regions

Commits: `3bf19b0e` (grid point limit), `ba6a0a62`, `27beb5a0` (Region attribute settings with `%`, commas and quotes, in the attribute-strings layer), `64c4401a`, `8b86f10a` (`astMask<X>` with an unbounded Region, PointList mask), `07517ca8` (`astMapRegion` into fewer axes), `3b775b35`, `93432265`, `59aaa1be`, `9b5a63a6`, `bc374b82` (overlap and bounds faults), `3046b37e`, `fbac9f50`, `3e7c4051` (Ellipse), `832be0e3`, `ec7d8586` (Polygon), `f19880b3` (Box `Simplify`), `b222d947` (moved Interval, Circle, Prism caches), `cd1ca2b7` (Prism overlap with a negation), `3cce24ce` (CmpRegion Bounded; retires the open quirk "A CmpRegion that ANDs two nested unbounded Regions reports itself as bounded"), `b73010f9` (CmpFrame `NormBox` order; re-read the open quirk "`CmpFrame::NormBox` can never widen a box"), `ad2b515b` (message spelling, relevant to goal 4), and the Region halves of `1df4a93a`.

Tests: `testregionhardening.c`, `testregionlogic.c`.

### Stage 4 — Frames, Axis, formatting, time, units, casting, Plot

Commits: `b6d4e160` (Format validation), `50b6e3af` (`<bad>` for a huge sky value), `56fdcbdd` (`iso.n` at most 9; no date gives `<bad>`), `86601275` (invalid year or month), `b009d17c` (TimeFrame `Centre`), `3a32fec8`, `f3e0de73`, `49e2c58c` (`AxisFields` and `AxisAbbrev`; retires two open quirks), `acbda4d7`, `c6a8f61d` (unit mapper), `58f5de2f`, `55cd143d` (cast only into a class the object belongs to: SpecFluxFrame/CmpFrame conversions change), `107f7513` (CmpFrame `Match` with no axes), the SkyFrame half of `f38b4a9e`, `865e65bd`, `bdb3fb86`, `591f3f2e`, `32b62f90`, `a782e537`, `84b6b007` (expected *n/a, buffer*, confirm each), and Plot `fac9b4ca`, `481795f5` and the calendar-tick fixes.

Tests: `testfields.c`, `testcast.c`, `testplotcopy.c`, `testplotclip.c`, and the new `testcmpframe.c` checks.

### Stage 5 — hostile input

Loader checks: `80e779ba`, `afd8c283`, `0a1c7b19`, `82cf1a52`, `7e48793d` (Frame and CmpFrame permutations), `6716f499`, `5ef98333`, `14ffe4a9` (FrameSet graph), `2ae11f85`, `cda35a7b`, `639232d1`, `a48c4a04`, `ca7c350e`, `d3f5b711`, `4abd31f2`, `6d87f732`, `d5fee31d`, `cba0743a`, `7f917006`, `aa915bed`, `e384383c`, `c6932471`, `8c542c65`, `65df6bf8`, `2572667b`, `8078ddd8`, `1e1ae0fe`, `a247426e` (`astCheckLoaded`), `63c9f42f`, `744ddab4`, `da7d3330`, `57a16d1a` (integer out of range), `b3a1143b` (`AST__MXAXES` for constructors and loaders), `50c3995a`, `2d00a457` (*n/a, buffer* for the truncation; message text still compared), `dbd9bc94` (StcsChan digits), `1be1c664` (a string MOC range is not expanded), and the MOC and StcsChan halves of `f38b4a9e`.

Tests: `testcorruptinput.c`, `testloaderhardening.c`, `testframeperm.c`, `testframesetload.c`, `testdssmap.c`, the new `testxphmap.c` and `testmoc.c` checks that are not PR #107's.

Stage 5 also carries the port-only hardening below.

## Error parity (stage 5, and stages 1–4 where a case is in their area)

- A committed C probe under `tools/oracle_fixtures/` runs every case of the five hardening tests against the pinned C and records the status code and each message passed to `astPutErr`, in order.
- **The parity unit is the whole ordered list** (decided by the user): the status code, then every message C reports, context lines included, in C's order.
  If `AstError` cannot produce that list as it stands, changing how it builds or orders its context is in scope, with a `CHANGELOG.md` entry; the CFFI `astPutErr` replay must emit the same list.
- The Rust tests embed the expected code and messages for each case, with the probe and pin named in a comment, rather than adding a port-only file to the mirrored fixture tree.
- A message that C truncates to fit its buffer is compared as C would have written it in full, per the buffer decision; each such case is listed in the test with the reason.
- The known difference "A failed load at the top level loses C's context message" (Java port, `rust-c-differences.md`) is in scope and must be closed by this work.

## Port-only hardening (stage 5)

- **Nesting depth.** The Native reader, and any other recursive reader the triage finds (MathMap expressions, unit strings, STC-S), rejects input nested deeper than a named constant with `AST__LDERR` (or the reader's own C error code) and a message, instead of overflowing the stack.
  The builder picks the value; it must comfortably exceed the deepest nesting in the fixture corpus.
- **Allocation.** No count read from input sizes an allocation before the data it counts has been seen, which is C's own fix (`14ffe4a9`, `744ddab4`, `63c9f42f`); the triage should search for `with_capacity` and `vec![_; n]` fed by parsed counts.
- **Amplification.** Text the port keeps in full, where C truncated, is bounded only by the allocator, and in Rust a failed allocation aborts the process rather than returning an error.
  That is acceptable because such text is never larger than the input it came from.
  What is not acceptable is a small input that asks for a large output: a Format field width or precision, `Digits`, `FitsDigits`, `iso.n`, a grid or mesh size, a MOC range, a repeat count.
  Each such value is bounded explicitly before it sizes anything, by C's documented limit where C has one (`iso.n` at 9, `AST__MXAXES`, the grid-point limit of `3bf19b0e`) and otherwise by a named port-only constant with an error and a quirks entry.
- **Time.** No reader or `astSimplify`/`astMapSplit` call on a fuzzed input runs for longer than a per-input bound set in the harness; an input that exceeds it is a finding.
- **Panics.** A panic on input read from a file is a bug and becomes an `AstError`; the CFFI `catch_unwind` is not a substitute.

## Fuzzing harness (stage 5)

- `cargo-fuzz` targets in a `fuzz/` directory outside the workspace's default members, needing nightly only to run the fuzzer: at least Native read (`Channel`), FitsChan read and write-back, StcsChan read and MocChan read.
- Seeds from `crates/starlink-ast/tests/fixtures/`, plus a dictionary of Native and FITS tokens.
- Mutation in the spirit of the C campaign (change one value in a valid input); the builder recovers the C harness's strategy and lessons from the transcripts named above.
- Run with debug assertions and overflow checks on, as the Rust counterpart of the C campaign's optimized-versus-unoptimized differential.
- **Differential replay**: a tool that reads a corpus directory through both the port and the reference C library (linked as `ast-oracle-tests` does) and reports any input where they disagree on accepting it or on the error code.
  This is how the sync checks that error parity holds beyond the hand-written cases.
- Every crash, hang or over-allocation found is minimized and committed as a regression case that a tier-1 test replays on stable Rust.
  CI does not run the fuzzer.

## Rules settled during review

- **Stage order and green main.**
  Stages merge in order, each branched from `main` after the previous one merges.
  Stage 0 ports whatever behavior the copied fixtures need for tier 1 to pass (the CDELT and `proj.c` fixes are the known ones).
  Any stale reference that `check_corpus_fresh.sh` or `check_simplify_fresh.sh` reports, and that tier 1 does not already fail on, is listed in `PLAN.md` §8 with the stage that owns it.
  A case that does fail and belongs to a later stage goes in a named exclusion list with its reason and a companion test that fails when it starts passing; the owning stage removes it.
- **Buffer cases in the hardening tests.**
  Where C rejects input for a reason a caller can see (an unknown attribute name, which a name of a hundred characters always is, or an invalid Format), the port rejects it with C's code.
  Where C only truncates (a Label, a Symbol, an error message), the port keeps the full text, and the test expects C's text as C would have written it without the buffer, naming the case and the reason.
  `2d00a457` is such a case: the message is compared, in full.
- **Floating-point results in the new tests.**
  The project's existing rules apply: check new fixtures against a default-flags C build (FMA contraction on), and any relaxation is per case, named in source and counted in `PLAN.md` §6.
- **Where port-only test data lives.**
  Fuzz regression inputs go in `crates/starlink-ast/tests/regressions/`, outside the mirrored fixture tree.
  They are not distributed in the crate archive; their test carries the `ast_dist_tier = "cutdown"` ignore attribute.
- **Differential disagreements that are expected.**
  An input rejected by a port-only limit (nesting depth, for example) is an allowed disagreement, and the replay tool reports it under a separate heading naming the limit.
  The tool runs the C library in a child process for each input, so that a C crash or hang is reported rather than ending the run.
- **C faults the Rust fuzzer finds.**
  Each is recorded in `c-library-quirks.md`, with a probe, and listed by the replay tool as a known C fault; the stage is not blocked on a C fix.
  Fixing C upstream is a separate decision for the C maintainer.
- **`fuzz/`** is not a workspace member, its sources carry the standard two-line header, and it needs no `tracing` feature.

## Left to the builder

- Branch names for stages 1–5 (suggested: `u/timj/sync-c-<area>`).
- The order of commits within a stage.
- The nesting-depth constant and the fuzz time bound.
- Which Rust test file each ported C test lands in, following "Porting a test" in `CLAUDE.md`.

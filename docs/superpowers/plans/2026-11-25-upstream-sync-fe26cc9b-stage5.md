# Upstream sync to `fe26cc9b`, stage 5 (hostile input) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every corrupt dump in the C hardening tests fails in the port with C's code and C's whole ordered message list, no input read from a file can crash, hang or over-allocate the port, and a fuzzing harness with a differential replay against C keeps it that way.

**Architecture:** Task 1 commits a C probe that records C's code and messages for every `astFromString` call of the five hardening tests, and a Rust test that replays them, with every case that differs in a named exclusion list. Task 2 makes the error list C's shape (context lines, typed-read errors). Task 3 ports C's shared loader checks (`astCheckLoaded`, `astCheckLoadedCoords`, `astCheckLoadedMap`, `astCheckPermArray`, `AST__MXAXES`) at C's granularity. Tasks 4–9 port each class's loader checks, each removing its cases from the exclusion list. Tasks 10–12 port the non-loader sections and the port-only limits. Tasks 13–14 add the fuzzer, its regression replay and the differential tool.

**Tech Stack:** Rust 2024 (`starlink-ast`), C probes against `reference/starlink-ast/build`, `cargo-fuzz` (nightly, outside the workspace), `ast-oracle-tests`.

**Spec:** `docs/superpowers/specs/2026-11-22-upstream-sync-fe26cc9b-design.md` (Stage 5, "Error parity", "Port-only hardening", "Fuzzing harness", "Rules settled during review").

## Global Constraints

- Branch `u/timj/sync-c-hardening`, branched from the tip of `u/timj/sync-c-frames` (the user's choice, 2026-11-25). Never push. Fixups with `git commit --fixup`. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, no session line.
- Shell is zsh; `unset CC CFLAGS CPPFLAGS LDFLAGS CXX` for cargo and probe builds. Probes link `cc p.c -I$B -L$B -last -lm -Wl,-rpath,$B` with `B=reference/starlink-ast/build`; the C tests also need `-I reference/starlink-ast/ast_tester -I reference/starlink-ast/ast_tester/star_compat`.
- Never modify the submodule's tracked files.
- Error parity is the status code and every message C reports, in order, except C's first `AST: Error in routine …` line: the port's list is `err.message().split('\n')` followed by `err.context()`.
- Limits that exist only because C had a fixed buffer are not mirrored; a message C cuts at 1023 characters is compared as C would have written it in full, and the case is named with the reason. Documented limits (`AST__MXAXES` = 65536, `MXLOADCON` 99999, `MXLOADEL` 9999999, `AST__MXORDHPX` 27, `AST__MXCOLNAMLEN` 100) are mirrored.
- Every port-only limit is a named constant, raises an `AstError`, has a doc comment and a `c-library-quirks.md` entry saying C has no such limit. No count read from input sizes an allocation or a loop before it is checked against a limit or the data it counts has been read.
- A panic on input read from a file is a bug; `cargo test` runs with overflow checks, so integer overflow on input counts.
- A user-visible change gets a `CHANGELOG.md` `Unreleased` entry in the same commit; every C oddity found gets a quirks entry, probed where possible.
- New test files carry the AURA header, name their C ancestor and divergences, and join `crates/starlink-ast/tests/serial/main.rs` unless named otherwise. Generated `cases.rs` files are exempt from the header.
- An exclusion list is a relaxation: `test_verification_ledger.rs` `LEDGER` and `PLAN.md` §6, updated whenever entries leave.
- Run `cargo clippy -p starlink-ast --all-targets -- -D warnings`, `cargo fmt`, then `cargo run -q -p audit-tools --bin reanchor-citations -- --rust-only --write docs/audits/mapmerge/*.md`, right before each commit.
- Stage end: `cargo test -p starlink-ast --no-fail-fast`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS=-D warnings cargo doc -p starlink-ast --no-deps`, `tools/fixtures/check_mirror.sh`, `tools/oracle_fixtures/check_c_headers_fresh.sh`, `tools/oracle_fixtures/check_probes_fresh.sh`, `cargo run --release -p ast-bench -- bits --base main`.

## Triage

From each commit's diff, the Rust site, and a replay of all 261 `astFromString` cases of the five tests through C and the port (2026-11-25; evidence, with the capture probe prototype `cap.c` and the per-case `.cap`, `.port` and `.diff` files, in `.superpowers/sdd/2026-11-25-upstream-sync-fe26cc9b-stage5/evidence/`).
Replay result: 70 the same, 54 differ only in messages, 104 accepted by the port and rejected by C, 14 another code, 14 crash or hang the port, 5 rejected by the port and accepted by C.
Case ids are `<test>#<ordinal>` (0-based `astFromString` call): `tci` testcorruptinput, `tlh` testloaderhardening, `tfp` testframeperm, `tfl` testframesetload, `tds` testdssmap.

| commit | verdict | what changes | Rust site | task |
|---|---|---|---|---|
| (all loaders) | missing | C ends every failed top-level load with `Error while reading a <class> from a Channel.` and a nested one with `Failed to read the "<item>" Object value.` once | `channel/base.rs:275` drops it; nested loads have no item name | 2 |
| `57a16d1a` | missing | an integer item that is not an integer or is outside `int` is AST__BADIN `astRead(Channel): The value "<name> = <text>" cannot be read as an integer.` (likewise double, string, Object) | `serial/node.rs:366-394`, `serial/native_read.rs:121` | 2 |
| `2d00a457` | n/a buffer, text compared in full | C cuts a message at 1023 characters | — | 2 (tci#117, #118 named) |
| `a247426e`, `1e1ae0fe` | missing | `astCheckLoaded`: a component missing or of the wrong class is AST__LDERR `astLoad(<class>): The <item> is missing from the <class> read.` / `… is a <found>, which is not a <expected>.`; the dispatchers read any class, then check | `serial/mod.rs:419`, `:741`, `:1321` ("unknown Mapping class"); Frame, CmpFrame, FluxFrame, Table, FitsTable, Object `keymap` | 3 |
| `4abd31f2` | missing (the seam) | `astCheckLoadedCoords`, `astCheckLoadedMap` | beside `read_mapping_base`, `serial/mod.rs:302` | 3 |
| `7e48793d` | missing | one `astCheckPermArray(this, perm, naxes, method)`, AST__PRMIN, two messages | `frames/mod.rs:3573` `validate_permutation` (port texts, no method) | 3 |
| `b3a1143b` | missing | `AST__MXAXES` 65536 for Mapping and Frame constructors and loaders, CmpFrame sums | no constant; `read_mapping_base`, `basic.rs:1286`, `:158`; `UnitMap::new`, `ShiftMap::new` return `Self` | 3 |
| `80e779ba`, `afd8c283` | missing | Frame `Prm`, CmpFrame `Axp` checked with `astLoadFrame`/`astLoadCmpFrame` | `basic.rs:1315` (filters a bad value out), `cmpframe.rs:1327` (`as usize` wraps) | 4 |
| `2572667b` | missing | fixed-axis Frames check Naxes; SpecFluxFrame its components; a SkyFrame's plain Axis becomes a SkyAxis | `skyframe.rs:3945`, `specframe.rs:2962`, `dsbspecframe.rs:1382`, `timeframe.rs:2643`, `fluxframe.rs:1310`, `specfluxframe.rs:865`; stale `frame_load_oracle`, `sky_plain_axis_oracle` | 4 |
| `6716f499`, `14ffe4a9` | missing | FrameSet indices, links form a tree, axis counts agree, Frames read one at a time; `astGetVarFrm` stops a cycle | `frameset.rs:3029-3160`, `:1306` (loops for ever on `Vfr1 = 2, Vfr2 = 1`) | 5 |
| `a48c4a04` | missing | Plot base Frame ≤ 3 axes (AST__NAXIN), tick counts, clip Frame and bounds (LDERR) | `plot/serial.rs:572`, `:692-727` | 5 |
| `6d87f732`, `65df6bf8`, `744ddab4` | missing; `744ddab4` messages only | Nin/Nout of 14 simple Mappings; UnitNormMap 0 axes; a missing Spec/Sla step is AST__BADIN C text, a negative count is 0 | 14 `from_node`s (triage5a), `unitnormmap.rs:772`, `specmap.rs:1831`, `slamap.rs:1673`, `timemap.rs:1581` | 6 |
| `d5fee31d` | missing | CmpMap, NormMap, RateMap, SelectorMap, SwitchMap, TranMap check their components and arity | `cmpmap.rs:1284`, `normmap.rs:398`, `ratemap.rs:622` (`-1 as usize + 1` overflows), `selectormap.rs:544`, `switchmap.rs:943`, `tranmap.rs:841` | 6 |
| `2ae11f85`, `cda35a7b`, `639232d1`, `cfdbbe8d` (WcsMap), `ca7c350e`, `aa915bed`, `c6932471`, `da7d3330` | missing | PolyMap powers, LutMap size and inverse, WcsMap `CheckAxes` (skipped for a blank Type), SplineMap `CheckSizes`/`CheckKnots`, MathMap functions, PermMap constants, MatrixMap 9999999 elements; constructors share each check with a method name | per class (triage5a) | 7 |
| `cba0743a`, `7f917006` | missing (load, merge guard, order); transforms already | DssMap offsets ≥ `INT_MAX-1` (load and merge); XphMap Order 0..=27; XphMap far positions overflow in debug | `dssmap.rs:631`, `:836`; `xphmap.rs:198`, `:229`, `:483` | 7 |
| `8078ddd8`, `aa29054e` (loader), `63c9f42f` | missing | Region loaders check Unc/Frm/FrmSet, points per class, components, Operator; Polygon undefined vertex; PointSet ≤ 9999999 values | `serial/mod.rs:1060-1862`, `polygon.rs:1558` (panics), `serial/mod.rs:829` | 8 |
| `9b5a63a6` (Prism mesh) | missing | AST__INTER when the mesh would pass `INT_MAX` | `prism.rs:451` | 8 |
| `d3f5b711`, `1be1c664`, `f38b4a9e` (Moc) | missing | Moc NRange and MaxOrder; a string range is kept as a range; huge RA reduced with `fmod` | `serial/mod.rs:1765`, `moc.rs:2420`, `:2477`, `:1186`, `:1332`, `:3573`, `:3977` | 9 |
| `e384383c` | missing | Table column name > 100 characters | `table/serial.rs:46` | 9 |
| `8c542c65` | n/a memory; port-only clamp | `SzGss` reserves without bound | `keymap/mod.rs:235` | 9 |
| `dbd9bc94`, StcsChan half of `f38b4a9e` | n/a: StcsChan not ported | — | — | 15 (PLAN row) |
| `50c3995a`, `0f11fe09` | n/a memory (verified) | — | — | — |

Found in triage, not from a commit:
- A top-level PointSet dump loads in C (tci#71); the port has no PointSet object. Task 8 rules.
- A Region's Frame part is never read, so `System = "FK5"` in a Circle's Frame part loads (tci#40, C AST__ATTIN). Task 8.
- The port reads the whole text before loading, so a later syntax error hides an earlier semantic one; no case depends on it. Task 15 (PLAN row).
- `Table` refuses a dump with no `Columns` KeyMap, which C allows. Task 9.

## Review Focus

1. A dump with a huge or negative count (axes, points, constants, functions, matrix elements, ranges, Frames) is refused with C's error in bounded time and memory, in a debug build. Pinned by the Task 1 crash cases and Task 13's regression replay.
2. A dump whose component is missing or of another class gives C's `astCheckLoaded` text, not "unknown X class". Pinned in Task 3.
3. A valid dump of every class still loads and round-trips byte for byte after the checks. Pinned in Task 6 (`tlh` `check_coords`, the valid half) and the existing serialisation corpus.
4. A nested failure carries exactly one `Failed to read the "<item>" Object value.` line and one `Error while reading` line. Pinned in Task 2.
5. A deeply nested dump is refused with an error, not a stack overflow, at the limit on a 2 MiB test thread in a debug build. Pinned in Task 12.

---

### Task 1: The parity probe and replay test

**Files:** Create `tools/oracle_fixtures/hardening_probe.c`, `tools/oracle_fixtures/build_hardening_probe.sh`, `crates/starlink-ast/tests/hardening_oracle/cases.rs` (generated), `crates/starlink-ast/tests/test_hardening_oracle.rs`; modify `tests/serial/main.rs`, `tests/test_verification_ledger.rs`, `PLAN.md` §6 and tracker, `tools/oracle_fixtures/README.md`.

**Interfaces:** Produces `CASES: &[Case]` with `Case { test: &str, ordinal: usize, dump: &str, code: i32, messages: &[&str] }` (`code` 0 when C returned an object), `HARDENING_EXCLUDED: &[(&str, usize, Why)]` with `enum Why { Differs(&str), Crashes(&str) }`, and `pub(crate) fn messages(err: &AstError) -> Vec<&str>` (message lines then context). Every later task removes its cases from `HARDENING_EXCLUDED`.

- [ ] **Step 1:** Write `hardening_probe.c`: it `#include`s one C test chosen by `-DTEST_SOURCE`, renames its `main`, wraps `astFromString` (`#undef` then `#define`) to record the dump, and defines `astPutErr_` to record messages until the call returns; prints Rust source, one `Case` per call. Model: the scratch `proto5/cap.c`. `build_hardening_probe.sh` builds it once per test (testcorruptinput, testloaderhardening, testframeperm, testframesetload, testdssmap) and writes `cases.rs`; testdssmap reads its fixture, so run it from `ast_tester/`.
- [ ] **Step 2:** Run the script. Expected: 261 cases (tci 125, tlh 109, tfp 9, tfl 7, tds 11).
- [ ] **Step 3:** Write `test_hardening_oracle.rs`: `cases_match_the_c_library` loads every case not excluded with `from_native_string` inside `within_10s` (import it from `test_regionhardening.rs`) and asserts the code (`as_raw`) and `messages`, or success when C's code is 0. `excluded_cases_still_differ`: a `Differs` case runs in process and must differ; a `Crashes` case runs in a child process (`std::env::current_exe()` with the test name, `--exact`, and an env var `AST_HARDENING_CASE=<test>#<ordinal>` that a `#[test] fn run_one_case` reads and otherwise returns at once), killed after 20 s, and must not exit 0. `excluded_cases_are_in_the_corpus`.
- [ ] **Step 4:** Fill `HARDENING_EXCLUDED` from the run, every case with a reason naming the task and commit that fixes it (the triage table); the 14 crash cases (tlh#33; tci#11, 13, 31, 49, 50, 52, 60, 77, 102, 107, 108, 110, 117) are `Crashes`.
- [ ] **Step 5:** Run `cargo test -p starlink-ast --test serial test_hardening_oracle::`. Expected: PASS, about 191 exclusions. Add the ledger row and §6 row (count), the tracker rows ("partly ported (stage 5 in progress)"), and the probe's README entry. Commit ("Replay the C hardening tests' corrupt dumps against the C library's messages").

### Task 2: C's ordered message list for a failed load

**Files:** `crates/starlink-ast/src/channel/base.rs:261-300`, `serial/node.rs`, `serial/native_read.rs`, `serial/mod.rs` (dispatchers), `error.rs`; tests in `test_hardening_oracle.rs`, `tests/test_native_errors.rs` (create).

**Interfaces:** Produces `AstError::messages(&self) -> Vec<&str>`; `Node::key()` (the lower-cased item name a nested object was read under); `pub(crate) fn nested<T>(node: &Node, r: AstResult<T>) -> AstResult<T>` in `serial/mod.rs`, which every loader uses to load a child; typed-read errors recorded on the `Node` and returned by `any_object_from_node` ahead of the loader's own.

- [ ] **Step 1:** Write failing tests in `test_native_errors.rs`, with C's lists from the probe: a top-level Frame with `Prm1 = 6` ends with `Error while reading a Frame from a Channel.`; a Circle whose PointSet has `Npoint = 2147483647` gives the PointSet message, `Failed to read the "points" Object value.`, `Error while reading a Circle from a Channel.` (the `Failed` line once, from the innermost parent); `Nin = 2147483648`, `1e308`, `2 3` give AST__BADIN `astRead(Channel): The value "nin = 2147483648" cannot be read as an integer.` and the `Error while reading` line; tci#117's 5000-digit Naxes gives the same message in full (2d00a457).
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3:** Wrap the error at `base.rs:275` as `:288` does; record the item name in `Node::push_object`; add `nested` with a private once-only flag on `AstError` (C's `astreadclassdata_msg`); make `get_int` (and `get_double`, `get_string`, `get_object`) record C's typed-read error for text C's `astRead<Type>` rejects, using `strtoll`'s rule (whole item, within `i32`) and return C's default, and make the loader paths return a recorded error first (C's loaders run under `if (astOK)`, so the first error wins).
- [ ] **Step 4:** Run the new tests and `cargo test -p starlink-ast --test serial`. Expected: PASS. Remove the cases this fixes from `HARDENING_EXCLUDED` (tds#1-3, tlh#56, tci#2-7, #105 and others the run shows); `excluded_cases_still_differ` names any left behind. CHANGELOG entry. Commit ("Report a failed load with the C library's ordered messages, and refuse an integer an int cannot hold").

### Task 3: C's shared loader checks

**Files:** `serial/mod.rs` (beside `read_mapping_base`, the three dispatchers), `frames/mod.rs:3573`, `constants.rs`, `mappings/unit.rs`, `mappings/shift.rs`, `frames/basic.rs`, `frames/cmpframe.rs`, every public Mapping and Frame constructor that takes a number of axes; callers across the workspace.

**Interfaces:** Produces `pub const MXAXES: usize = 65536` (`constants.rs`, `#[doc(alias = "AST__MXAXES")]`); `pub(crate) fn check_loaded<'a>(node: Option<&'a Node>, class: &str, needed: bool, name: &str, item: &str) -> AstResult<Option<&'a Node>>` (is-a through `native_stream::class_chain`); `check_loaded_coords(name: &str, nin: usize, nout: usize, want_in: usize, want_out: usize) -> AstResult<()>`; `check_loaded_map(node: Option<&Node>, invert: bool, name: &str, item: &str) -> AstResult<(DynMapping, usize, usize)>`; `pub(crate) fn check_perm_array(method: &str, class: &str, perm: &[i64], naxes: usize) -> AstResult<()>`; `UnitMap::new` and `ShiftMap::new` return `AstResult<Self>`.

- [ ] **Step 1:** Failing tests: tci#9-14 and #60 (Nin, Nout, Naxes, RegAxes 65537 and 2147483647: C's BADNI, BADNO, NAXIN texts); tci#22 (`The associated KeyMap read for a UnitMap is a Frame, which is not a KeyMap.`); tlh#89, #95, #106 (a component of the wrong class); `UnitMap::new(65537)`, `ShiftMap::new(vec![0.0; 65537])`, `BasicFrameBuilder::new(65537).build()`, `CmpFrame` of 65536 + 1 axes: C's `astInitMapping`/`astInitFrame` texts (testcorruptinput `test_axis_limit`); a public `perm_axes` with a repeated axis gives C's `astPermAxes(Frame)` text.
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3:** Port the helpers from mapping.c, object.c and frame.c at the fe26cc9b lines; check axis counts in `read_mapping_base` and the Frame loader before anything is sized; the dispatchers load any class through `any_object_from_node` and `check_loaded` checks it. Change `UnitMap::new` and `ShiftMap::new` (the user's "Rust API open; return AstResult wherever C can raise" rule) and fix every caller. Replace `validate_permutation` with `check_perm_array`; each public `perm_axes` passes `astPermAxes`.
- [ ] **Step 4:** Run `cargo test -p starlink-ast --no-fail-fast` and `cargo build --workspace`. Expected: PASS. Remove the fixed cases from `HARDENING_EXCLUDED`. CHANGELOG (the two constructors, `MXAXES`). Commit ("Check what a dump supplies with the C library's shared loader checks, and limit axes to AST__MXAXES").

### Task 4: Frame loaders

**Files:** `frames/basic.rs:1280-1320`, `cmpframe.rs:1310-1341`, `skyframe.rs`, `specframe.rs`, `dsbspecframe.rs`, `timeframe.rs`, `fluxframe.rs`, `specfluxframe.rs` (`from_node`); `tools/oracle_fixtures/frame_load_probe.c`, `sky_plain_axis_probe.c` outputs.

- [ ] **Step 1:** Failing cases come from `HARDENING_EXCLUDED`: tfp#0-3, #7; tci#16-21, 87-93; delete their entries and run. Expected: FAIL with the port's texts.
- [ ] **Step 2:** Port `80e779ba`, `afd8c283`, `2572667b` and the Frame halves of `1e1ae0fe` through Task 3's helpers: a bad `Prm` or `Axp` (including `INT_MIN`, read as -1) is refused, not filtered; each fixed-axis class calls `check_loaded_naxes`; a SkyFrame replaces a plain Axis with a SkyAxis carrying its attributes (skyframe.c `astLoadSkyFrame`).
- [ ] **Step 3:** Regenerate `frame_load_oracle` and `sky_plain_axis_oracle` from their probes; the diff must be only the `2572667b` change. Remove the PLAN §8 stale-corpus row.
- [ ] **Step 4:** Run `cargo test -p starlink-ast --test serial --test frames --test domain_frames`. Expected: PASS. CHANGELOG. Commit ("Check the axes and permutation of a Frame read from a dump as the C library does").

### Task 5: FrameSet and Plot loaders

**Files:** `frames/frameset.rs:1306`, `:3029-3160`; `plot/serial.rs:572-727`.

- [ ] **Step 1:** Delete tfl#1-5, tci#76-86 (#77 crashes) and tlh#38-42 from `HARDENING_EXCLUDED`, and add a test that a FrameSet with `Vfr1 = 2, Vfr2 = 1` loads and its `variant()` gives AST__FRSIN `astGetVarFrm(FrameSet): Invalid or corrupt FrameSet - the chain of mirrored Frames is not valid.` within 10 s (tfl#6's later call). Run. Expected: FAIL (tci#77 by timeout).
- [ ] **Step 2:** Port `6716f499` and `14ffe4a9`: read Frames and Mappings one at a time, stopping at the first missing one; check every index, that links form a tree, and C's two axis-count messages; `get_var_frm` returns `AstResult<usize>` and stops after `nframe` steps (its callers propagate). Port `a48c4a04`.
- [ ] **Step 3:** Run `cargo test -p starlink-ast --test serial --test frames`. Expected: PASS. CHANGELOG. Commit ("Check the graph of a FrameSet and the axes of a Plot read from a dump").

### Task 6: Mapping arity and components

**Files:** the 14 simple loaders of `6d87f732` (triage5a lists file:line), `unitnormmap.rs:772`, `specmap.rs:1824`, `slamap.rs:1666`, `timemap.rs:1574`, and the six compound loaders of `d5fee31d`.

- [ ] **Step 1:** Delete tlh#59-108, tci#97, #103-105 and tds#4-5 from `HARDENING_EXCLUDED`; add a test that a valid dump of each of the 20 classes reloads and writes the same text (testloaderhardening `check_coords`/`check_valid`). Run. Expected: FAIL for the deleted cases, PASS for the round trips.
- [ ] **Step 2:** Each loader calls `check_loaded_coords` with C's numbers (triage5a row `6d87f732`), each compound loader `check_loaded_map`/`check_loaded`, with C's arity messages; RateMap's `IIn`/`IOut` are range-checked before use; UnitNormMap, SpecMap and SlaMap give C's codes and texts; a negative step count is 0.
- [ ] **Step 3:** Run `cargo test -p starlink-ast --test serial --test mappings`. Expected: PASS. CHANGELOG. Commit ("Check the inputs, outputs and components of a Mapping read from a dump").

### Task 7: Mapping contents

**Files:** `polymap.rs:2265-2410`, `lutmap.rs:133`, `:963`, `wcsmap.rs:229-253`, `:1512`, `splinemap.rs:174`, `:1348`, `mathmap/mod.rs:1047`, `perm.rs:1516`, `matrix.rs:1495`, `dssmap.rs:631`, `:836`, `xphmap.rs:114`, `:198`, `:229`, `:483`; `tests/test_xphmap.rs`, `tests/test_dssmap.rs` (create, from testdssmap.c).

- [ ] **Step 1:** Delete tlh#1-35, tci#100-102, #107, #108, #110-112, tds#6-9 from `HARDENING_EXCLUDED`. Port testxphmap.c's new checks (`check_bad_order` -1, 28, 31, 32 → LDERR; `check_far_positions` at orders 0, 3, 27, run in the debug test build) and testdssmap.c whole (its merge with offsets of 2e10 + 0.5 stays a CmpMap). Run. Expected: FAIL (the crash cases by timeout, XPHN/XPHS by overflow panic).
- [ ] **Step 2:** Port each commit's check with C's text and order; where C shares a check between constructor and loader (`CheckAxes`, `CheckSizes`, `CheckKnots`, LutMap's, MathMap's), the Rust constructor and `from_node` call one function with the method name. A WcsMap of blank Type is accepted (tlh#27-29). Counts are checked before anything is sized (`MXLOADCON`, `MXLOADEL`, SplineMap's `INT_MAX` products). MatrixMap Diagonal and Unit forms do not build a dense matrix from a loaded count. XphMap indices follow C's doubles (`7f917006`) or checked arithmetic, giving C's values.
- [ ] **Step 3:** Run `cargo test -p starlink-ast --test serial --test mappings`, then `cargo run --release -p ast-bench -- bits --base main`. Expected: PASS, no bits changed. CHANGELOG. Commit ("Check the contents of a Mapping read from a dump as the C library does").

### Task 8: Region and PointSet loaders

**Files:** `serial/mod.rs:829-846` (`read_pointset_node`), `:1060-1200` (`read_region_base`), the Region loaders `:1374-1862`, `regions/polygon.rs:1553`, `regions/prism.rs:451`, `regions/data.rs` (`region_base_grid`).

- [ ] **Step 1:** Delete tci#28-74 from `HARDENING_EXCLUDED` (#31, 49, 50, 52 crash). Add a Prism whose MeshSize makes `gsz1*msz2 + msz1*gsz2` pass `INT_MAX`: AST__INTER `astRegBaseMesh(Prism): The mesh for the Prism would have more points than can be created.`; and a Region grid with no points: both NPTIN messages (PLAN §1 rows). Run. Expected: FAIL.
- [ ] **Step 2:** Port `8078ddd8`, the loader half of `aa29054e`, `63c9f42f` and the Prism half of `9b5a63a6`; read a Region's Frame part through the Frame loader so its `System` is checked (tci#40). Ruling to record: tci#71-74 (a top-level PointSet) stay excluded with the reason "the port has no PointSet object" and a PLAN §1 row, unless C's text can be matched without a new public class.
- [ ] **Step 3:** Run `cargo test -p starlink-ast --test serial --test regions`. Expected: PASS. Remove the three stage 5 PLAN §1 rows. CHANGELOG. Commit ("Check what a dump supplies to define a Region").

### Task 9: Moc, Table and KeyMap

**Files:** `serial/mod.rs:1765` (Moc), `regions/moc.rs:1186`, `:1332`, `:2180-2540`, `:3573`, `:3977`; `table/serial.rs:46-78`; `keymap/mod.rs:235`; `tests/test_moc.rs`.

- [ ] **Step 1:** Delete tlh#53-55, tci#114-116, #119-121 from `HARDENING_EXCLUDED`; port testmoc.c's `checkLargeRange` and `checkCells`, and a Moc of one range with no MaxOrder (C: `Missing or invalid MaxOrder`), and a sky Moc given RA 1e300 and infinity (each within 10 s). Run. Expected: FAIL (`checkLargeRange` by timeout).
- [ ] **Step 2:** Port `d3f5b711`; store a string-MOC range as a range (`1be1c664`), so `27/0-216172782113783807` costs two values; port the `f38b4a9e` `fmod` at the four Moc sites; port `e384383c`; a Table with no `Columns` loads as C's does; clamp a loaded `SzGss` to 131072 (`8c542c65`, named constant, quirks entry).
- [ ] **Step 3:** Run `cargo test -p starlink-ast --test serial --test regions`. Expected: PASS. CHANGELOG. Commit ("Check a Moc, Table or KeyMap read from a dump, and keep a string Moc's ranges as ranges").

### Task 10: The C tests' other sections

**Files:** Create `tests/test_corruptinput.rs`, `tests/test_loaderhardening.rs`, `tests/test_framesetload.rs`, `tests/test_frameperm.rs`; `PLAN.md` tracker.

- [ ] **Step 1:** Port testcorruptinput.c `test_long_text` (5000-character System: AST__ATTIN in full; long default Labels, Symbols, Domains compared in full, each named as a buffer case; Format refusals), `test_attributes` (300-character attribute names: AST__BADAT for CmpFrame and FrameSet through `attributes::Attributes`; Prism and Plot3D in an exclusion list naming PLAN §1 and Plot3D), `test_documents` (FitsDigits -300 and 300 with C's card and C's BDFTS text from a probe; STC-S, XML and YAML named as not portable), `test_split` (12 axes, both splits), `test_fits` (every header read, written and transformed within 10 s with C's outcome from a probe); testloaderhardening.c `check_spline_values`; the valid round trips of testframeperm.c and testframesetload.c.
- [ ] **Step 2:** Run each file. Expected: PASS but for real differences; fix each in this task, or, if it belongs to a later task, exclude it with that reason.
- [ ] **Step 3:** Tracker rows for the five tests: ported, with what is not portable. Commit ("Port the sections of the C hardening tests that do not read a dump").

### Task 11: The unit parser's errors

**Files:** `crates/starlink-ast/src/units/` (`fix_constants`, `simplify_tree`, `create_tree`, `unit_mapper` and its callers); `tests/test_flux_badun.rs`.

- [ ] **Step 1:** Remove `UNIT_MESSAGE_EXCLUDED` from `test_flux_badun.rs` so its cases run; add tci#122's later call (Unit with six nested logs) with C's BADUN messages. Run. Expected: FAIL.
- [ ] **Step 2:** Make the unit functions return `AstResult` as unit.c raises (AST__BADUN `Illegal negative or zero constant value '<v>' encountered.`), and every caller of `unit_mapper` propagate it.
- [ ] **Step 3:** Run `cargo test -p starlink-ast --no-fail-fast`. Expected: PASS. Remove the PLAN §2 row and the §6 count. CHANGELOG. Commit ("Pass on the unit parser's errors as the C library does").

### Task 12: Port-only limits

**Files:** `serial/native_stream.rs` (reader recursion), `mappings/mathmap/` (expression parser), `units/` (unit parser), and every site found in Step 1; `docs/issues/c-library-quirks.md`.

- [ ] **Step 1:** List every recursive reader and every value read from input that sizes output (Format width and precision, `Digits`, `FitsDigits`, grid and mesh sizes, MOC orders and ranges, repeat counts) and whether a limit already bounds it; record the list in the commit message.
- [ ] **Step 2:** Failing tests: a Native dump nested 100 000 deep, a MathMap expression of 100 000 nested parentheses, a unit string of 100 000 nested parentheses, each an error (not a crash) on a thread with a 2 MiB stack in the debug build; a test per unbounded size value found in Step 1.
- [ ] **Step 3:** Add `MAX_NESTING` (one constant per reader, each at least ten times the deepest nesting in `tests/fixtures/`, measured, and low enough that the limit itself loads on a 2 MiB debug stack), raising AST__LDERR for the Native reader and the reader's own C code elsewhere, with a message naming the limit; bound each value from Step 1 by C's documented limit or a named constant. A quirks entry per port-only limit.
- [ ] **Step 4:** Run `cargo test -p starlink-ast --no-fail-fast`. Expected: PASS. CHANGELOG. Commit ("Refuse input nested or sized beyond named limits instead of overflowing the stack").

### Task 13: The fuzzer and its regression replay

**Files:** Create `fuzz/Cargo.toml` (not a workspace member), `fuzz/fuzz_targets/{native_read,fitschan_read_write,mocchan_read}.rs`, `fuzz/dict/{native,fits}.dict`, `fuzz/README.md`, `crates/starlink-ast/tests/regressions/`, `crates/starlink-ast/tests/test_fuzz_regressions.rs`.

- [ ] **Step 1:** Stop and ask the user before installing anything: running the fuzzer needs a nightly toolchain and `cargo install cargo-fuzz`.
- [ ] **Step 2:** The C campaign ran from the Java port's session (`~/.claude/projects/-Users-timj-work-astjava/9938f0b2-b70b-4e25-b697-23f14501d134.jsonl` and its `subagents/`); its files were rebuilt from that transcript into the plan workspace, `.superpowers/sdd/2026-11-25-upstream-sync-fe26cc9b-stage5/cfuzz-recovered/`: `cfuzz/fuzzlib.py` (seeds and line-level mutations: a value replaced from `NATIVE_VALUES` such as `2147483647`, `1e999`, `nan`, `<bad>`; lines dropped, duplicated or swapped; a class name replaced from `CLASSES`; every input rebuilt from its seed and mutation list), `cfuzz/harness.c`, `cfuzz/minimise.py`, and `cfuzz/REPORT*.md` (the failure buckets, the hangs and what PR #104 fixed). Its mutations, value list, class list and lessons go in `fuzz/README.md`, credited, and the mutator of each target follows them. Write the targets (Native read; FitsChan read then write back; MocChan read; StcsChan when it exists), each with a per-input time bound and `-rss_limit_mb`, seeds copied from `tests/fixtures/` by a script, and the dictionaries. Build with debug assertions and overflow checks.
- [ ] **Step 3:** `test_fuzz_regressions.rs` replays every file in `tests/regressions/<target>/` through the same entry point within 10 s, expecting no panic (stable Rust; `#[cfg_attr(ast_dist_tier = "cutdown", ignore = "fixture not distributed")]`).
- [ ] **Step 4:** Run each target for one hour. Each crash, timeout or out-of-memory is minimized (`cargo fuzz tmin`), committed under `tests/regressions/`, and fixed test-first in this task; a C fault the input also shows gets a quirks entry and a probe.
- [ ] **Step 5:** Run `cargo test -p starlink-ast --test serial test_fuzz_regressions::`. Expected: PASS. Commit per finding; final commit ("Add cargo-fuzz targets for the readers and replay their findings").

### Task 14: Differential replay against C

**Files:** Create `tools/oracle_fixtures/load_one.c` (reads one file with `astFromString` or a FitsChan, prints the status and messages), `crates/ast-oracle-tests/src/bin/hardening_replay.rs`, README entries.

- [ ] **Step 1:** Read the Java port's differential fuzzers first, `cfuzz-recovered/rv3fuzz/` (FITS headers: a C writer `cw.c`, a mutator `mut.py`, a comparator `cmp.py`) and `rv5fuzz/` (C and Java drivers `cdrv.c`, `JDrv.java`, generators and `cmp.py`), and reuse their output format and comparison rules where they fit. `hardening_replay <corpus dir> [--fits]` runs, per input, `load_one` and the port (the binary re-run with `--one <file>`) each in a child process with a 20 s limit, and reports under four headings: agreement count; disagreements on acceptance or code; refusals by a port-only limit (matched by the limit's message, Task 12); C crashes or hangs, each a known C fault to record.
- [ ] **Step 2:** Run it over `tests/regressions/` and the fuzz corpora. Every disagreement is fixed test-first, or recorded as a port-only limit or C fault with a quirks entry. Expected: no unexplained disagreement.
- [ ] **Step 3:** Commit ("Add a tool that replays a corpus through the port and the C library").

### Task 15: Close the stage

**Files:** `PLAN.md` (§1, §2, §6, §8 stage 5 paragraph and table row, tracker), `docs/issues/c-library-quirks.md`, `CHANGELOG.md`.

- [ ] **Step 1:** `HARDENING_EXCLUDED` holds only cases with a permanent reason (Plot3D, a top-level PointSet if ruled so); §6 count matches. PLAN rows: StcsChan's `dbd9bc94` and `f38b4a9e` halves; the read-whole-text-then-load ordering; the CFFI `astPutErr` replay of `messages()` when the shim lands.
- [ ] **Step 2:** Write the §8 stage 5 paragraph in the stage 1–4 shape, with the Java port's `rust-c-differences.md` entries these commits resolve ("A failed load at the top level loses C's context message" among them).
- [ ] **Step 3:** Run the stage-end gates. Expected: all pass. Commit ("Record stage 5 of the fe26cc9b sync as done").

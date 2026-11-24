# Upstream sync to `fe26cc9b`, stage 4 (Frames, Axis, formatting, time, units, casting, Plot) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Axis and TimeFrame formatting, date reading, `Centre`, `Fields`, unit errors, `Frame::cast`, SpecFluxFrame and CmpFrame conversions, and Frame geometry match the C library at `fe26cc9b`, and port `testfields.c`, `testcast.c`, `testplotclip.c` and the new `testcmpframe.c` checks.

**Architecture:** Task 1 ports the cases that already match into four test files; each later task adds its own failing cases first and then ports the C function it names, at the granularity of the C (`ParseAxisFormat`, `TimeFrame` `Format` and `Centre`, `astReadDateTime`, SkyAxis `AxisFields`, `astUnitMapper`, each class's `Cast`, SpecFluxFrame `MakeMap3`, the geometry functions of frame.c, skyframe.c and cmpframe.c).

**Tech Stack:** Rust 2024 (`starlink-ast`), C probes against `reference/starlink-ast/build`, `audit-tools`, `ast-bench`.

**Spec:** `docs/superpowers/specs/2026-11-22-upstream-sync-fe26cc9b-design.md` (Stage 4, "Per-stage method", "Rules settled during review").

## Global Constraints

- Branch `u/timj/sync-c-frames`, branched from the tip of `u/timj/sync-c-regions` (the user's choice). Never push. Fixups with `git commit --fixup`. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, no session line.
- Shell is zsh; `unset CC CFLAGS CPPFLAGS LDFLAGS CXX` for cargo and probe builds. Probes link `cc p.c -I$B -L$B -last -lm -Wl,-rpath,$B` with `B=reference/starlink-ast/build`, defining `astPutErr_` to capture messages.
- Never modify the submodule's tracked files.
- Limits that exist only because C had a fixed buffer are not mirrored; each gets a one-line `c-library-quirks.md` entry. Documented limits are mirrored. Where C refuses a value for a reason a caller can see (an invalid Format), the port refuses it with C's code.
- Error parity for a case is the status code and every message C reports, in order, except C's first `AST: Error in routine …` line; tests compare with the `expect_err` pattern of `test_regionhardening.rs` (message lines and no context).
- A user-visible change gets a `CHANGELOG.md` `Unreleased` entry in the same commit; a mirrored quirk upstream fixed gets a `RESOLVED (upstream <hash>)` entry with a Contents line; every C oddity found gets a quirks entry.
- New test files carry the AURA header, name their C ancestor and divergences, and join `crates/starlink-ast/tests/frames/main.rs`.
- An exclusion list is a relaxation: `test_verification_ledger.rs` `LEDGER` and `PLAN.md` §6, updated whenever entries leave.
- Run `cargo clippy -p starlink-ast --all-targets -- -D warnings`, `cargo fmt`, then `reanchor-citations --rust-only --write docs/audits/mapmerge/*.md`, right before each commit.
- Stage end: `cargo test -p starlink-ast --no-fail-fast`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS=-D warnings cargo doc -p starlink-ast --no-deps`, `tools/fixtures/check_mirror.sh`, `tools/oracle_fixtures/check_c_headers_fresh.sh`, `tools/oracle_fixtures/check_probes_fresh.sh`, `cargo run --release -p ast-bench -- bits --base main`.

## Triage

From each commit's diff, the Rust site, C probes and scratch ports of the C tests (2026-11-23; evidence in the session scratchpad `triage4a/`, `triage4b/`).

| commit | verdict | what changes | Rust site | task |
|---|---|---|---|---|
| `b6d4e160` | partly already, rest missing | a Format (and the format given to `astFields`) may have at most one conversion from `eEfFgGaAdiuoxXc`, `l` allowed before a float code, digit widths and precisions only; else AST__FMTER; an over-long result is two messages | `frames/axis_format.rs:53`, `:138`; `axis.rs:616` `plain_axis_fields` | 2 |
| `50b6e3af` | n/a, buffer (user decision, see Task 3) | DHms text over 70 characters is `<bad>` | `skyaxis.rs:513` `dhms_format` | 3 |
| `56fdcbdd` | mirror | iso.n capped at 9 places; a time with no calendar date is `<bad>`, in a value and in the TimeOrigin of the default Label and Title | `timeframe.rs:1673` `date_format`, `:2205`, `:1138` | 4 |
| `86601275` | behavior already, message missing | `astReadDateTime` reports `Month value (13) is invalid.` / `Year value (-4800) is invalid.` | `date_time.rs:229` | 4 |
| `b009d17c` | mirror (the port follows the unmerged C branch `u/timj/timeframe-centre-date`, port commit `1dbd342e`) | `Centre` stops comparing at the end of either date; an unreadable zeroed date falls back to the parent `Centre`; a day of 0 stays 0 | `timeframe.rs:1974` | 5 |
| `3a32fec8` | already | value with leading spaces | `axis.rs:616` | 1 (`FIELDS_EXCLUDED` leaves) |
| `f3e0de73` | already | `astAxisAbbrev` with no fields | `skyaxis.rs:1142` | 1 |
| `49e2c58c` | already, except a blank value | a blank sky value is one field at its start, value 0 | `skyaxis.rs:1083-1087` | 6 |
| `acbda4d7` | n/a (C crash), message missing | an unusable unit's error is C's BADUN text | `units/simplify.rs:202-216` | 7 |
| `c6a8f61d` | already | blank label unchanged by `astUnitMapper` | `units/mapper.rs:167` | 1 |
| `f38b4a9e` (SkyFrame) | already | no hang on a huge angle | `sky_last.rs:99`, `:167` | 1; the AZEL Epoch 1e11 difference is Task 11 |
| `58f5de2f` | n/a, memory | vtab set up before Cast | — | — |
| `55cd143d` | already (refusals), missing (casts that succeed) | a Frame is cast only into a class it belongs to; Region and FrameSet cast their current Frame; SpecFluxFrame casts into CmpFrame; each leaf class into its own class or Frame | `frames/mod.rs:1813` default `Frame::cast`, `cmpframe.rs:1179`, `plot/forward.rs:442`, no SpecFluxFrame `cast` | 8 |
| `107f7513` | missing | CmpFrame `Match` with a selection of no axes; needs Frames of no axes, which C allows | `basic.rs:245` refuses 0 axes; `basic.rs:1122`, `mod.rs:1860` | 10 |
| `865e65bd`, `bdb3fb86` | n/a, buffer | C cuts default Labels, Symbols, Domain and Title at 200 (100 for DSBSpecFrame Label and CmpFrame Domain) | — | 12 |
| `591f3f2e`, `a782e537` | already | — | — | — |
| `32b62f90` | already (Frame, CmpFrame); Region blocked on attribute strings (PLAN §1) | — | — | — |
| `84b6b007` | n/a, buffer, nothing observable | — | — | 12 |
| `73d3cc88` | n/a, memory (the AllVariants error predates it) | — | `frameset.rs:1605` | 12 (PLAN §8 row corrected) |
| `fac9b4ca`, `481795f5` | n/a, memory (verified; `481795f5`'s values all match) | — | `plot/mod.rs:135`, `plot/forward.rs:634` | 1 (testplotclip) |
| "calendar-tick fixes" | none in plot.c: the news item is `b009d17c` and `86601275` | — | — | 4, 5 |

Found in triage, not from a commit:
- SpecFluxFrame `MakeMap3` converts its SpecFrames without `astConvert`, so two whose Domains differ convert (testcast) — `specfluxframe.rs:664`, Task 9.
- `IntraMap` puts the user's message after C's on one line, where C reports C's message alone (testplotclip) — `intramap.rs:482`, Task 7.
- Frame geometry tests `is_bad` where C tests `AST__BAD` (PLAN §2 row from stage 3): every geometry function of frame.c, skyframe.c, cmpframe.c, axis.c and skyaxis.c tests `AST__BAD` alone — Task 11.
- AZEL→ICRS at Epoch MJD 1e11 is a UnitMap in C and a SlaMap giving BAD in the port, cause untraced — Task 11.
- C formats inf as `INF`/`NAN`, and a SkyFrame inf as `inf:00:00.1` — quirk and PLAN row only (Task 12).

## Review Focus

1. A Format a caller sets (or passes to `fields`) that C refuses is refused with C's FMTER text, and one C accepts (`%a`, `%A`, `%'d`) is accepted and formats as C's. Pinned in Task 2.
2. A TimeFrame value, Label or Title that has no calendar date gives `<bad>`, never a made-up date, at any iso.n. Pinned in Task 4.
3. `Centre` of a calendar-date axis gives C's value at every gap, including gaps of a day, a month and a year. Pinned in Task 5.
4. `Frame::cast` gives the class C gives, or `None` where C refuses, for every class pair of testcast. Pinned in Task 8.
5. A conversion that needs a selection of no axes gives no conversion and no error, both ways. Pinned in Task 10.

---

### Task 1: Port the cases that already match

**Files:** Create `tests/test_fields.rs`, `tests/test_cast.rs`, `tests/test_plotclip.rs`; `tests/frames/main.rs`, `tests/test_plot_frame_methods.rs` (`FIELDS_EXCLUDED` loses the `3a32fec8` case and the ledger count drops), `PLAN.md` tracker.

**Interfaces:** Produces one `#[test]` per C section in each file, extended by later tasks, and `expect_err` in `test_fields.rs` (`pub(crate)`, as in `test_regionhardening.rs`).

- [ ] **Step 1:** Port the matching cases with C's values: testfields' five missing-separator and hms cases, abbrev `a:b:c` → 0, the `+12.5`/`-12.5`/`12.5 `/`+3` cases, the three simple-axis cases (`"  -7 "` → (2,2)), the four unformat cases (bad month: nc=4, v=55927.5), Epoch refused, `centre` gap 0.01 → 56000, gap 1e-9 → the value, BAD → BAD; testcast's CmpFrame-holding-a-Region conversions (2+2), CmpFrame↔SpecFluxFrame conversions (4) and the 8 refused casts (`None`); testplotclip whole (an `IntraMap` whose transform fails on a flag, `Plot::new`, `set_clip_region(FrameRef::Index(3), …)`, `tran2` keeps (0.5,0.5) and gives BAD for (0.95,0.5), border and grid through `LogGrf::new(Vec::new())`, each failure AST__ITFER, the message compared in Task 7).
- [ ] **Step 2:** Run `cargo test -p starlink-ast --test frames test_fields:: test_cast:: test_plotclip:: test_plot_frame_methods::` (one filter at a time). Expected: PASS.
- [ ] **Step 3:** Tracker: testfields, testcast, testplotclip "partly ported (stage 4 in progress)"; testplotcopy "not portable: needs the graphics context (`astGetGrfContext`) and Plot3D, neither in the Rust API; port with the C API". Commit ("Port the cases of testfields.c, testcast.c and testplotclip.c that already match").

### Task 2: Format validation (`b6d4e160`)

**Files:** `frames/axis_format.rs`, `frames/axis.rs`, `test_fields.rs`, quirks, CHANGELOG.

- [ ] **Step 1: Tests:** each of `%s %n %d%d %ld %hd %*d % abc% %Lf %p %10s` set as Format(1) gives AST__FMTER `astFormat: The Format string "<fmt>" cannot be used to format an axis value. It should have no more than one conversion specification, for a floating point or integer value.` when 65.25 is formatted; `fields` with `%s` gives the same; `%a` → `0x1.05p+6`, `%A` → `0X1.05P+6`, `%'d` → `65`, `%'.3f` → `65.250`; `%200f` gives the two messages `…result exceeds 127 characters.` and `The format string was "%200f".` (probe each for the exact text first). Expected: FAIL.
- [ ] **Step 2:** Port axis.c `ParseAxisFormat`'s check and call it from `fields` as C does; format `%a`/`%A` (C99 hex float) and accept the `'` flag (C locale: no grouping); split the overflow error into C's two messages.
- [ ] **Step 3:** Run; CHANGELOG "Fixed"; commit ("Check a Format as the C library does").

### Task 3: A huge sky value (`50b6e3af`)

**Files:** `frames/skyaxis.rs`, quirks, `test_fields.rs`.

- [ ] **Step 1:** Per the user's decision (handoff question 1). If not mirrored: a test that ±1e300 with Format `dms.3` gives the full text, and a quirks entry "SkyAxis formats a value of more than 70 characters as `<bad>`" (buffer, not mirrored; `3e0868e9`'s testcorruptinput case named for stage 5). If mirrored: a test that ±1e300, and 1e60 with `dms.3` and `lhms.2`, give `<bad>`, and the port does it in `dhms_format` where the text passes 70 characters.
- [ ] **Step 2:** Commit ("Record how the C library formats a huge sky value" or "Format a huge sky value as `<bad>`, as the C library does").

### Task 4: Dates (`56fdcbdd`, `86601275`)

**Files:** `frames/timeframe.rs`, `frames/date_time.rs`, `test_fields.rs` (or `test_timeframe.rs` for formatting), CHANGELOG.

- [ ] **Step 1: Tests:** iso.10, iso.12, iso.80, iso.200T of MJD 56000.12345 give `2012-03-14 02:57:46.666570748` (probe the exact input); 1e30, ±1e10, -3e8 give `<bad>`; 3e8 gives `823230-12-24 00:00:00.00`; with TimeOrigin 1e30 the Label and Title are `Modified Julian Date offset from <bad>`; Epoch `2012-13-01` gives `Month value (13) is invalid.` then `astSetAttrib(Frame): Invalid epoch value "2012-13-01" given for coordinate system.` with AST__ATTIN; `-4800-01-01` gives `Year value (-4800) is invalid.` first. Expected: FAIL.
- [ ] **Step 2:** Port timeframe.c `Format`'s cap and its `<bad>` for a date ERFA cannot give (no `unwrap_or(0)` on `djcl`), the TimeOrigin text in `GetLabel`/`GetTitle`, and `astReadDateTime`'s year and month messages.
- [ ] **Step 3:** Run; quirk "`astReadDateTime` tests ERFA's status…" rewritten or RESOLVED; CHANGELOG "Fixed"; commit ("Format and read dates as the C library does").

### Task 5: TimeFrame `Centre` (`b009d17c`)

**Files:** `frames/timeframe.rs:1974`, `test_fields.rs`, `test_plot_frame_methods.rs:291`, `tests/plot_frame_methods_oracle/cases.rs` (regenerated), quirks, CHANGELOG.

- [ ] **Step 1: Tests:** `centre(56000.37, 1)` = 55986, gap 30 → 56010, gap 400 → 56000; flip `test_plot_frame_methods.rs:291` from 55927. Expected: FAIL.
- [ ] **Step 2:** Replace the port's `Centre` with timeframe.c `Centre` at the pin, removing what came from the unmerged branch; regenerate `plot_frame_methods_oracle` with `check_probes_fresh.sh --write plot_frame_methods_oracle` (12 CENTRE rows change; `time_unit_h_iso` stays a known drift).
- [ ] **Step 3:** Run; the second paragraph of quirk "TimeFrame `Fields` never counts…" to RESOLVED (upstream b009d17c); CHANGELOG "Fixed"; commit ("Centre a calendar date axis as the C library does").

### Task 6: A blank sky value (`49e2c58c`)

**Files:** `frames/skyaxis.rs:1083-1087`, `test_fields.rs`, quirks.

- [ ] **Step 1: Test:** `fields` of `""` on a `d` axis gives 1 field at (0,0), value 0; of `"   "` gives 1 field at (3,0), value 0. Expected: FAIL (0 fields, BAD).
- [ ] **Step 2:** Port skyaxis.c `AxisFields`' handling of a blank value.
- [ ] **Step 3:** Run; quirks "SkyAxis `AxisFields` dereferences a null pointer…" and "`AxisFields` reads past the end of a value with leading spaces" to RESOLVED (49e2c58c, 3a32fec8); CHANGELOG; commit.

### Task 7: Unit and IntraMap messages (`acbda4d7`, testplotclip)

**Files:** `units/simplify.rs:202-216`, `mappings/intramap.rs:482`, `test_fields.rs` (or `test_fluxframe.rs`), `test_plotclip.rs`, CHANGELOG.

- [ ] **Step 1: Tests:** Unit `log(log(log(log(log(Jy)))))` on a FluxFrame gives AST__BADUN `Illegal negative or zero constant value '-0.872603' encountered.` on set and on each get (probe the full list); the testplotclip failures give `astTransform(IntraMap): Error signalled by "CopyOrFail" transformation function.` with the user's message, if C reports none, absent. Expected: FAIL.
- [ ] **Step 2:** Propagate the unit-tree error as unit.c does instead of replacing it; build the IntraMap error as C's message, keeping the user's text only where C would (probe `astSetStatus` with and without `astError` in the function).
- [ ] **Step 3:** Run; CHANGELOG; commit ("Report unusable units and failed IntraMaps with the C library's messages").

### Task 8: `Frame::cast` (`55cd143d`)

**Files:** `frames/mod.rs:1813` and each class's `cast` (`basic.rs`, `skyframe.rs`, `specframe.rs`, `dsbspecframe.rs`, `timeframe.rs`, `fluxframe.rs`, `specfluxframe.rs`, `cmpframe.rs`, `frameset.rs`, `plot/forward.rs`, `regions/mod.rs`), `test_cast.rs`, CHANGELOG.

**Interfaces:** `fn cast(&self, template: &dyn Frame) -> Option<DynFrame>` (existing signature) gives what C's `astCast` gives.

- [ ] **Step 1: Tests:** the seven casts testcast expects to succeed give, by `downcast_ref`, StcObsDataLocation, Circle, CmpFrame (SpecFluxFrame → CmpFrame), FrameSet (Plot → FrameSet), Frame, SkyFrame, Frame (FrameSet → Frame); the eight refusals stay `None`. Expected: FAIL (7).
- [ ] **Step 2:** List the `Cast` vtab entries of frame.c, skyframe.c, specfluxframe.c, cmpframe.c, frameset.c, region.c and plot.c (architecture, per CLAUDE.md "Before porting a class") and port each as that class's `cast`: a Region or FrameSet casts its current Frame, SpecFluxFrame casts into CmpFrame, each class casts into itself or a class it derives from.
- [ ] **Step 3:** Run; CHANGELOG "Fixed"; commit ("Cast a Frame into each class it belongs to, as the C library does").

### Task 9: SpecFluxFrames whose SpecFrames do not convert

**Files:** `frames/specfluxframe.rs:664` `make_map3`, `test_cast.rs`, CHANGELOG.

- [ ] **Step 1: Test:** two SpecFluxFrames whose SpecFrames have Domains `FOO` and the default give no conversion either way, and `match_template` with the FOO template gives no match. Expected: FAIL.
- [ ] **Step 2:** Port specfluxframe.c `MakeMap3`'s `astConvert` of the two SpecFrames (Domain included) in place of `make_spec_mapping`.
- [ ] **Step 3:** Run; CHANGELOG; astjava entry "SpecFluxFrames whose SpecFrames do not convert" noted for Task 12; commit.

### Task 10: Frames of no axes (`107f7513`)

**Files:** `frames/basic.rs:245`, `:1122`, `frames/mod.rs:1860`, `frames/cmpframe.rs:1423`, `test_cmpframe.rs`, CHANGELOG.

- [ ] **Step 1: Tests:** `convert` between a 1-axis Frame and CmpFrame(CmpFrame(A,B),C) gives none and no error, both ways; with the 1-axis Frame's Domain A it gives a 1→3 conversion, 5 → (5, BAD, BAD); `BasicFrameBuilder::new(0).build()` and `pick_axes(&[])` succeed, as `astFrame(0, "")` and `astPickAxes` of no axes do (probe what a 0-axis Frame's attributes and dump are). Expected: FAIL (AST__NAXIN).
- [ ] **Step 2:** Allow zero axes wherever C does (frame.c `astInitFrame` checks `naxes < 0`); audit the code that assumes at least one axis for the paths the tests reach, and port cmpframe.c `GenAxisSelection`'s no-axes loop end.
- [ ] **Step 3:** Run the whole crate (a 0-axis Frame reaches far); CHANGELOG; commit ("Allow a Frame of no axes, and match a CmpFrame selection of none, as the C library does").

### Task 11: Frame geometry with infinite values, and the AZEL Epoch

**Files:** `frames/mod.rs` (`AxisKind::distance`/`offset`/`norm`, `offset2`, `intersect`, `line_contains`, `ax_angle`, `resolve`, `resolve_points`, `cartesian_*`), `skyframe.rs`, `cmpframe.rs`, `specfluxframe.rs`, the 1-D `angle`s of `specframe.rs`, `fluxframe.rs`, `timeframe.rs`, `test_regionhardening.rs`, `PLAN.md`, quirks.

- [ ] **Step 1:** Per the user's decision (handoff question 2). If followed: tests that distance((0,0),(inf,0)) = inf, `ax_angle` = π/2, `intersect` = (0.5,0) on a plain Frame and C's values on a SkyFrame and CmpFrame (triage4b probes); the `infinite_vertex` exclusion leaves and `polygons` asserts C's `00100`, bounds (0,0)–(inf,1), `Same`. Expected: FAIL.
- [ ] **Step 2:** Each listed site tests `BAD_VALUE` alone, as its C function does; geometry results that C gives as NaN stay NaN (they are not transform outputs). `val_ok` stays.
- [ ] **Step 3:** Run the crate and `ast-bench bits --base main`; ledger and PLAN §6 counts; the §2 row closed; quirk "A Polygon with a vertex at infinity…" says the port mirrors it.
- [ ] **Step 4:** Trace AZEL→ICRS at Epoch MJD 1e11 with the simplify trace (CLAUDE.md "Debugging the simplifier"): C simplifies to a UnitMap. Port the cause if it is in this stage's code; otherwise a PLAN row naming it. Commit per change.

### Task 12: Record what stays open, and close the stage

- [ ] **Step 1:** Quirks (buffer, not mirrored): default Labels, Symbols, Domain and Title cut at 200/100; the 99-character SkyFrame dump comment; C formatting inf as `INF`/`NAN` and a sky inf as `inf:00:00.1` (with a PLAN row for the port's formatting of inf). PLAN §8: the `AllVariants` row corrected (the error predates `73d3cc88`) and closed; stage 5 note that testcorruptinput's `2bd4ab23` Label/Symbol length checks are buffer cases.
- [ ] **Step 2:** Tracker rows "ported"; PLAN §8 stage 4 paragraph and table row done, with the verdicts and the astjava entries resolved or changed: "Date and time strings", "A SpecFrame Symbol changed by its Unit", "The comment of a FluxFrame's SFCBRW units", "The default Title of a SpecFluxFrame", the unparsable-units entry, "Casting between a CmpFrame and a SpecFluxFrame", "Casting a Region into the class of another Region", "SpecFluxFrames whose SpecFrames do not convert", "A Frame with no axes", and, with Task 11, "Infinite input coordinates" and "SkyFrame `Offset` with a bad offset".
- [ ] **Step 3:** Stage-end gates from Global Constraints; `bits` may differ from `main` only by stages 0–3's known cases and what Tasks 5 and 11 change, each explained. Commit ("Record stage 4 of the fe26cc9b sync as done").

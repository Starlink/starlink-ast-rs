# Upstream sync to `fe26cc9b`, stage 2 (FitsChan) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make FitsChan reads and writes match the C library at `fe26cc9b`, port `testfitswrite.c`, remove the two panics and the hang the triage found, and give the FitsChan cases of `testcorruptinput.c` C's error codes and messages.

**Architecture:** `testfitswrite.c` is ported first with a named exclusion list that each fix task empties.
Each upstream fix is ported at C's granularity: a replaced-card stack in `CardList` for C's `SaveReplaced`/`FixReplaced`, C's `MjdToDateObs` for every DATE-OBS, C's `GetWcsAxes` check for WCSAXES, and C's `CnvType` range checks.

**Tech Stack:** Rust 2024 (`starlink-ast`), C probes against `reference/starlink-ast/build`, `tools/oracle_fixtures/check_probes_fresh.sh`, `audit-tools`.

**Spec:** `docs/superpowers/specs/2026-11-22-upstream-sync-fe26cc9b-design.md` (Stage 2, "Per-stage method", "Error parity", "Rules settled during review").

## Global Constraints

- Branch `u/timj/sync-c-fitschan`, branched from the tip of `u/timj/sync-c-mappings` (the user's choice). Never push. Fixups with `git commit --fixup`. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, no session line.
- Shell is zsh; `unset CC CFLAGS CPPFLAGS LDFLAGS CXX` for cargo and probe builds. Probes link `cc p.c -I$B -L$B -last -Wl,-force_load,$B/libast_err.a -lm -Wl,-rpath,$B` with `B=reference/starlink-ast/build`; to capture messages, define `astPutErr_` and drop `libast_err.a`.
- Never modify the submodule's tracked files.
- Limits that exist only because C had a fixed buffer are not mirrored; each gets a one-line `c-library-quirks.md` entry. Documented limits are mirrored.
- Error parity for a hostile-input case is the status code and every message C reports, in order, except C's first `AST: Error in routine <caller> at line N in file <file>.` line, which names the calling program's source location.
- A user-visible change gets a `CHANGELOG.md` `Unreleased` entry in the same commit; a mirrored quirk upstream fixed gets a `RESOLVED (upstream <hash>)` entry with a Contents line.
- New source files carry the AURA copyright and SPDX header; a ported C test names its ancestor and divergences in its header and joins `crates/starlink-ast/tests/fitschan/main.rs`.
- An exclusion list is a relaxation: add it to `test_verification_ledger.rs` `LEDGER` and `PLAN.md` §6 with its size, and update both whenever entries leave.
- Run `cargo fmt` before `reanchor-citations --rust-only --write`, and the citation gate after both, right before each commit.
- Stage end: `cargo test -p starlink-ast --no-fail-fast`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS=-D warnings cargo doc -p starlink-ast --no-deps`, `tools/fixtures/check_mirror.sh`, `tools/oracle_fixtures/check_c_headers_fresh.sh`, `tools/oracle_fixtures/check_probes_fresh.sh`.

## Triage

From each commit's diff, the Rust site, C probes, and a throwaway port of every `testfitswrite.c` case (2026-11-23).

| commit | verdict | what changes | Rust site | task |
|---|---|---|---|---|
| `4175c735` | mirror | a failed write puts back the cards it replaced (`SaveReplaced`, `FixReplaced`) | `card_list.rs` `put_or_append`, `fix_new`; `mod.rs` `write` | 2 |
| `cbd04fa7` | already | a failed write's new cards are removed and Card restored | `mod.rs` `write` | — |
| `6a00ff8f` | missing | a CmpMap that may not be simplified is not searched for a WcsMap | `fset_to_store/celestial_axes.rs` `split_map2`, `push_or_recurse` | 3 |
| `68708cb5` | missing (SIP), panic (WorldAxes) | SIP needs the other axes to use exactly the remaining grid axes; an out-of-range permutation index is bad, not a panic | `sip_int_world.rs:368`; `map_tab.rs` `fits_order_perm` | 3 |
| `5a3e23e3` | missing (null FrameSet), n/a memory (LogWcs) | nothing is written when `MakeFitsFrameSet` gives none (two SpecFrames, two SkyFrames, no fiducial point) | `fset_to_store/mod.rs` 468-472, 582-591 | 3 |
| `01fc69d6` | mirror | the absolute-coordinates description takes a free letter and marks it used | `wcs_write/plane_stores.rs` 131, 146 | 4 |
| `4e42485f` | mirror | FITS-PC compares secondary PVs up to the larger index and rejects a different `naxis` | `enc_pc.rs` 164, 216 | 4 |
| `07011b5f` | mirror, panic | FitsDigits clamped to the card width | `format.rs` `encode_float_in`; panic at `frames/axis_format.rs:514` | 5 |
| `bd9ffbfc` | missing | DATE-OBS only for a finite MJD with a calendar date, by C's `MjdToDateObs` | `wcs_write/mod.rs` `mjd_to_date_str`, `mjd_to_gregorian`; callers | 6 |
| `df9c0243` | missing (WCSAXES), already (SIP, short CTYPE, WAT) | WCSAXES must be 1 to 999 (`GetWcsAxes`); the port hangs on huge values | `wcs_keywords.rs` `extract_wcs_keywords` | 7 |
| `cfdbbe8d` | mirror (CnvType), missing (grism) | a float out of `int`/`int64` range is refused (AST__FTCNV); a grism order out of range is AST__BDFTS | `card_value.rs` `from_float`; `wcs_spectral.rs:431` | 8 |
| `f38b4a9e` | mirror (latent) | `DSSFromStore` reduces a huge angle with `fmod` before the ±360 loop | `enc_dss.rs` `write_to_cards` | 9 |
| `e809b988` | n/a buffer, plus a general divergence | C cuts the offset-axis comment at 79 characters; `NewCard` strips a comment's leading blanks, the port does not | `fset_to_store/sky_sys.rs:353`; card layer | 9 |
| `ef779057` | n/a buffer | strings copied into fixed buffers (warnings, CTYPE, Symbol, Label, TPN over 80 characters) | — | 9 |
| `31541c4a` | already, n/a memory | CLASS `crval` initialised; fewer than three CRPIX axes not written | `spec_trans.rs` `translate_class`; `enc_class.rs` | — |
| `afc179b0`, `75d87b05`, `8690ee52`, `a6e42163`, `4ae0505f` | already | — | — | — |

Side findings for Task 9: `sip_int_world.rs:97` (`let nout = nin;`) still mirrors the bug the quirk "RESOLVED (upstream 232643bc)" says the port fixed; `fset_to_store/mod.rs:1107` tests `is_finite()` where C tests `!= AST__BAD`; `docs/audits/mapmerge/dssmap.md` blames `cfdbbe8d` for the CNPIX change that is `cba0743a` (stage 5).
The `fitschan_encodings_oracle` drift is `4175c735` (Task 2).

## Review Focus

1. A FitsChan write that fails part way leaves the FitsChan with exactly the cards and Card position it had, whichever cards it replaced or added. Pinned in Task 2 (a replaced card in the middle, one at the end, and one whose keyword the write removed).
2. A header with WCSAXES of 2147483647, 1e300, 1000, 999.6, 0, -1 or 0.4 is rejected promptly with C's code and message, never hangs. Pinned in Task 7.
3. FitsDigits of any value, including -100000 and 1000, writes a card that fits in 80 characters and never panics. Pinned in Task 5.
4. An MJD-OBS of NaN, 1e10, 1e300 or BAD writes no DATE-OBS; one before 1999 uses C's `DD/MM/YY` form. Pinned in Task 6.
5. A FrameSet the port cannot write as FITS returns "not written" (false) with no error and leaves the FitsChan unchanged, never panics. Pinned in Tasks 3 and 2.

---

### Task 1: Port `testfitswrite.c` with a named exclusion list

**Files:** Create `crates/starlink-ast/tests/test_fitswrite.rs`; modify `crates/starlink-ast/tests/fitschan/main.rs`, `test_verification_ledger.rs`, `PLAN.md` (§6 row, tracker row).

**Interfaces:** Produces `const FITSWRITE_GAP_EXCLUDED: &[(&str, &str)]` and `excluded_cases_still_differ`, which reports every excluded case that now matches.

- [ ] **Step 1:** Port every case by C's description, pinning each "any return" case to C's actual return (Ident 0, the `-TAB` cases 0, SIP with shared pixel axis 0, CLASS 4-D 0, the rest 1) and its cards. Run each case under `std::panic::catch_unwind`, so a panic is a failure of that case.
- [ ] **Step 2:** Run `cargo test -p starlink-ast --test fitschan test_fitswrite::`. Expected failures, each excluded with its closing task: two axes on one pixel axis (panic, Task 3), distorted sky with two axes sharing a pixel axis (Task 3), offset sky with Ident (Task 4), PC secondary PV with no primary (Task 4), failed TPN write (Task 2), abandoned secondary keeps C1VAL1 (Task 2). Any other failure is a finding to triage before going on.
- [ ] **Step 3:** Add the list to the ledger and §6; tracker row "ported, gaps excluded". Commit ("Port testfitswrite.c").

### Task 2: A failed write puts back the cards it replaced

**Files:** `crates/starlink-ast/src/fitschan/card_list.rs`, `crates/starlink-ast/src/fitschan/mod.rs`, `test_fitswrite.rs`, `tests/fitschan_encodings_oracle/cases.rs`, `tools/oracle_fixtures/probes.tsv`, quirks, CHANGELOG.

- [ ] **Step 1: Tests** (Review Focus 1, values from a C probe): a FITS-PC write of a TPN FrameSet over four cards it replaces and fails on keeps all four with their values and Card; a replaced card that the write then deleted comes back at the end, as `FixReplaced` appends it. Expected: FAIL (two cards of four left).
- [ ] **Step 2:** Port `SaveReplaced` (`fitschan.c:28047`, called where a card is overwritten with the new flag, skipping one already flagged `NEW2`) as a replaced-card stack in `CardList`, and `FixReplaced` (`fitschan.c:17072`: most recent first, appending when the keyword is gone), called first by `fix_new` and at the end of `write` with the `NEW1|NEW2` flags.
- [ ] **Step 3:** Run the tests, remove the two Task 1 exclusions, regenerate the oracle (`check_probes_fresh.sh --write fitschan_encodings_probe`, `cargo fmt`), drop its `known` entry, check it reports SAME. RESOLVED quirk; close astjava "A failed write loses a card it replaced" in the stage notes; CHANGELOG "Fixed". Commit ("Put back the cards a failed FitsChan write replaced").

### Task 3: FrameSets that cannot be written

**Files:** `fset_to_store/celestial_axes.rs`, `sip_int_world.rs`, `map_tab.rs`, `fset_to_store/mod.rs`, `test_fitswrite.rs`, CHANGELOG.

- [ ] **Step 1: Tests** (C probe values): a FrameSet whose Mapping holds a CmpMap with Ident set, alone and nested in a series CmpMap, writes nothing and returns false; the panic case and the SIP-shared case return false with no error; two SpecFrames, two SkyFrames, and a sky reference with no fiducial point return false for every encoding (testcorruptinput `spec2`). Expected: FAIL, including a panic.
- [ ] **Step 2:** `push_or_recurse` and the flatten helpers do not descend into a CmpMap whose `do_not_simplify()` is true (`fitschan.c:34254`); after the SIP split, require `nin - 2` other inputs none of which is a celestial one, else `Ok(None)` (`fitschan.c:30016`); `fits_order_perm` treats an index at or past `n` as bad instead of indexing; return "not written" wherever C's `MakeFitsFrameSet` returns NULL (`fitschan.c:22388`).
- [ ] **Step 3:** Run; remove the two Task 1 exclusions; CHANGELOG "Fixed" (a panic, and FrameSets C does not write); astjava "Two SkyFrames or two SpecFrames in MakeFitsFrameSet" in the notes. Commit ("Write no FITS for a FrameSet the C library cannot write").

### Task 4: Version letters and FITS-PC secondary descriptions

**Files:** `wcs_write/plane_stores.rs`, `enc_pc.rs`, `test_fitswrite.rs`, CHANGELOG.

- [ ] **Step 1: Tests:** an offset SkyFrame beside a Frame with Ident `A` writes `CTYPE1A='FOO1'` with no RADESYSA and the offset sky in B with SREFISB and RADESYSB; a secondary FITS-PC description with PVs and no primary ones writes no `C1YPE1`; one with an extra axis is rejected for its axis count, not by coincidence. Expected: FAIL for the first two.
- [ ] **Step 2:** Port `FreeVersion` (`fitschan.c:12517`) at both `s += 1` sites, marking the letter used; in `enc_pc.rs` compare PVs up to `max(primary, secondary)` index and reject `naxis != primnaxis` (`fitschan.c:25573`, `:25819`).
- [ ] **Step 3:** Run; remove the two exclusions (the list is now empty); CHANGELOG "Fixed". Commit ("Give a FitsChan version letter once, and compare FITS-PC descriptions fully").

### Task 5: FitsDigits within a card

**Files:** `fitschan/format.rs`, `frames/axis_format.rs` (if the panic is there), tests in the FitsChan area, CHANGELOG.

- [ ] **Step 1: Tests** (C values): FitsDigits 1000 writes `1.0E+300` for 1e300 and `-5.1E-4 1.0E+300` for the complex pair; FitsDigits -100000 writes without panicking, as C does. Expected: FAIL (AST__BDFTS; panic).
- [ ] **Step 2:** Clamp the digits as C does (`fitschan.c:10288`, `ldigits = min(ldigits, maxwidth)`) before any formatting sizes a buffer.
- [ ] **Step 3:** Run; CHANGELOG "Fixed". Commit ("Format a FITS value within its card whatever FitsDigits is").

### Task 6: DATE-OBS by C's `MjdToDateObs`

**Files:** `wcs_write/mod.rs` (`mjd_to_date_str`, `mjd_to_gregorian`), the six callers, `erfa` (a `jdcalf` port if `palDjcal` needs it), tests, quirks, CHANGELOG.

**Interfaces:** Produces `pub(crate) fn mjd_to_date_obs(mjd: f64) -> Option<String>` replacing the Meeus code.

- [ ] **Step 1: Tests** (C probe values): 55000.5 → `'2009-06-18T12:00:00.000'`; 1e10, 1e300, NaN, BAD → none; -2468570 → `'01/03/-6800'`; -50000.6 → `'24/12/-179'`; 51179.9999999999 → `'1999-01-01T24:00:00.000'`; testcorruptinput `check_dateobs`. Expected: FAIL.
- [ ] **Step 2:** Port `MjdToDateObs` (`fitschan.c:8907`): none unless finite and not BAD; before `palCaldj(99,1,1)` use `palDjcal(0)` with `%2.2d/%2.2d/%2.2d` and the year less 1900 or 2000; otherwise `palDjcl` and `palDd2tf(3)`. Port ERFA's `jdcalf` natively if the crate lacks it, bit for bit, with a golden row. Use it at every caller C uses it (3962, 4483, 7170, 19943, 25787, 38454).
- [ ] **Step 3:** Run, plus `check_corpus_fresh.sh` (a DATE-OBS change can move references); quirk entry for the port's former calendar; CHANGELOG "Fixed". Commit ("Write DATE-OBS as the C library does, and none without a calendar date").

### Task 7: WCSAXES is 1 to 999

**Files:** `wcs_keywords.rs`, the dead checks in `enc_pc.rs:40` and `enc_iraf.rs:35`, tests, CHANGELOG.

- [ ] **Step 1: Tests** (Review Focus 2) under a 10-second guard thread: WCSAXES 2147483647, 1000, 1e300, 0, -3 → AST__BDFTS with `astRead(FitsChan): The FITS keyword WCSAXES  has an invalid value (<v>) - it should be an integer between 1 and 999.` where `<v>` is C's `%g` (`2.14748e+09`, `1000`, `1e+300`, `0`, `-3`); 999.6 and 0.4 as C gives them (probe); 100 → C's AST__INTER `GetItemC(fitschan): AST internal error; intermediate axis index 99 is invalid.`. Expected: FAIL or hang.
- [ ] **Step 2:** Port `GetWcsAxes` (`fitschan.c:13711`, error at `:13777`) and use it wherever C does (FindLonLatSpecAxes, TabMapping, WcsMapFrm); mirror the 99-axis limit C reports, as it is documented behavior of the keyword scheme, not a buffer.
- [ ] **Step 3:** Run; CHANGELOG "Fixed" (a hang); astjava "More than 99 WCS axes" in the notes. Commit ("Check WCSAXES as the C library does").

### Task 8: Float to integer conversions

**Files:** `card_value.rs` `from_float`, `wcs_spectral.rs:431`, tests, quirks, CHANGELOG.

- [ ] **Step 1: Tests:** `get_fits_i` on NAXIS1 = -1e300 → AST__FTCNV with `astGetFitsI(FitsChan): Cannot convert FITS keyword 'NAXIS1' to integer.` then `astGetFitsI(FitsChan): Cannot get value for FITS keyword 'NAXIS1'.`; I of 1e10 and K of 1e19 refused, K of 9.2e18 accepted and 9.3e18 refused (probe the exact bounds); a grism PV order of 1e10 and -2147483647 → AST__BDFTS `astRead(FitsChan): The FITS keyword PV1_1  has an invalid value (1e+10) for the diffraction order of a grism.`. Expected: FAIL (saturating casts).
- [ ] **Step 2:** Port C's range tests (`fitschan.c:7971`, `:7977`, `:7988`; grism `!(fabs(pv) < INT_MAX)` at `:14931`).
- [ ] **Step 3:** Run; RESOLVED quirk; CHANGELOG "Fixed". Commit ("Refuse a FITS value an integer cannot hold").

### Task 9: Small fixes and records

**Files:** `enc_dss.rs`, `sip_int_world.rs:97`, `fset_to_store/mod.rs:1107`, the card layer's comment handling, `docs/audits/mapmerge/dssmap.md`, quirks, CHANGELOG.

- [ ] **Step 1:** `f38b4a9e`: in `write_to_cards`, `fmod` a value larger than 1e8 in magnitude before the ±360 loop (test: CRVAL2 = 1e10 gives C's cards).
- [ ] **Step 2:** `NewCard` strips a comment's leading blanks (`fitschan.c:24391`); make the port's card creation do so (test: an empty-Symbol offset SkyFrame writes C's `/ offset from 0:40:00.0, 20:00:00`). Run `check_corpus_fresh.sh`; CHANGELOG "Fixed".
- [ ] **Step 3:** `sip_int_world.rs:97`: read `nout` as C now does and correct the quirk "RESOLVED (upstream 232643bc)"; `fset_to_store/mod.rs:1107`: test `!= BAD_VALUE` as C does, with a test of a CRVAL that the transform makes infinite (probe C first).
- [ ] **Step 4:** n/a-buffer quirk entries for `e809b988` (offset-axis comment at 79 characters) and `ef779057` (warning texts, CTYPE, Symbol and Label copies, TPN over 80 characters); `dssmap.md`'s note names `cba0743a` and stage 5. One commit per fix.

### Task 10: Close the stage

- [ ] **Step 1:** Remove `FITSWRITE_GAP_EXCLUDED`, its companion test, ledger entry and §6 row; tracker row "ported".
- [ ] **Step 2:** `PLAN.md` §8: stage 2 done, the verdicts (`cbd04fa7`, `31541c4a`, `afc179b0`, `75d87b05`, `8690ee52`, `a6e42163`, `4ae0505f` already), the astjava entries resolved, and the stale table without `fitschan_encodings_oracle`.
- [ ] **Step 3:** Stage-end gates from Global Constraints, and `ast-bench bits --base main` (expect only the DATE-OBS and comment changes, explained). Commit ("Record stage 2 of the fe26cc9b sync as done").

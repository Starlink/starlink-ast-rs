# Vendored wcslib, ERFA and PAL update in C AST — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the wcslib, ERFA and PAL code inside C AST (`reference/starlink-ast`) up to date — wcslib projection fixes from upstream 8.10, ERFA 2.0.1, and PAL routines that ERFA now duplicates calling ERFA — as one upstream pull request, then re-pin this repository's submodule to it and re-copy `crates/starlink-ast/vendor/` so the C oracle and the port agree before any more C is ported.

**Architecture:** Two repositories, in order. In the submodule, on branch `u/timj/DM-56839-vendored` from `origin/master`, each change is its own commit with a test in `ast_tester` when it changes behaviour, and the C oracle fixtures are regenerated last. In this repository, on branch `tickets/DM-56839-vendored` (from `main`, and the base the SphMap stage `tickets/DM-56839-sphmap` is stacked on, so that the vendored C is the reference every Rust replacement is measured against), the pin moves, `vendor/` is re-copied, and the mirrored fixtures and Rust-side expectations follow.

**Tech Stack:** C99, CMake and autotools (C AST builds with both), CTest; Rust 2024 for the port side.

**Spec:** `docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md`, section "Step 0: update the vendored libraries in C AST".

## Global Constraints

- Every change of behaviour lands in C AST first; this repository only re-pins and re-copies.
- One commit per change in the submodule, message in the style of `7b67e1b4` (what changed, the upstream version it came from, why it matters).
- A `History:` entry `YYYY-MM-DD (TIMJ):` is added to any C file that carries a History block (all `pal/*.c`, AST's `src/*.c`); `wcslib/` and `erfa/` files carry none and get none.
- `erfa/` is refreshed by copying whole files from `/Users/timj/work/lsst/erfa/src` (v2.0.1-12-g1a8044c, SOFA 20231011); its sources stay unmodified, as `palwrap.c` documents.
- Every new ERFA symbol gets an `astEra` rename in `erfa2ast.h`, so nothing is exported unprefixed.
- C AST is built for tests with `cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DAST_BUILD_FORTRAN=OFF -DCMAKE_C_FLAGS=-ffp-contract=off && cmake --build build -j 18`, tested with `ctest --test-dir build -j 8`.
- New sources are listed in both `CMakeLists.txt`/`palwrap.c` and `Makefile.am` (`ERFA_FILES` at line 393, `PAL_FILES` at 585).
- Nothing is pushed by the implementer: the user pushes the submodule branch and opens the upstream pull request, and pushes this repository's branch.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01DidLSReaHyVFnUCCMn54Xw`.
- Run cargo and C builds with conda's `CC`, `CXX`, `CFLAGS`, `CPPFLAGS`, `LDFLAGS` unset; the shell is zsh.

## Decided out of scope

From the research of 2026-10-27, recorded so no task reopens them:

- `palDrange` stays: `eraAnpm(±π)` is `∓π` where `palDrange(±π)` is `±π`, and `wcsmap.c:2841-2851`, `skyaxis.c`, `skyframe.c`, `fitschan.c` and `ellipse.c` depend on `+π`.
- `palEqgal`/`palGaleq` stay (FK4-derived galactic matrix, not ERFA's ICRS one); `palPrebn`, `palMappa`, `palAmpqk`, `palMapqkz`, `palEtrms`, `palDmat` have no ERFA equivalent.
- wcslib: CSC's `float` intermediates (AST V3.2-6, not an upstream bug), the HPX +180 meridian remap (unreachable after `palDrange`), the 8.10 SZP `prjbchk` tolerance, stride/vector/`wcserr`/thread-safety refactors, and H/K rounding and `pv <= 0` validation in HPX setup.

AST's own native-coordinate bounds check in `wcsmap.c` (~line 2893) is in scope as Task 7b, at the user's direction (2026-10-27): matching wcslib's tolerance is less confusing for anyone comparing AST with other FITS-WCS software.

## Review Focus

1. **A WcsMap projection evaluated exactly at a pole, a face centre or a ±180 edge** (QSC face 4 centre, HPX and COP poles): a round trip returns the point, or both directions agree it is bad — never forward-valid and reverse-bad. Pinned in Tasks 2, 3 and 6.
2. **Non-default projection parameters** (HPX with H, K other than 4, 3): the same results as wcslib 8.10. Pinned in Task 3.
3. **FK4 at an epoch other than B1950**: the FK4/FK5 conversion matches ERFA's, and the change from PAL's is stated and bounded. Pinned in Task 10.
4. **A single-translation-unit build** (`palwrap.c`): new ERFA files compile together without symbol clashes and every new symbol is renamed. Pinned in Task 8.
5. **Formatting of a zero angle**: `ERFA_DNINT` no longer yields `-0.0`, so a formatted angle that rounds to zero must not print with a minus sign where it did not before. Pinned in Task 8.

---

### Task 1: Submodule branch and a baseline build

**Files:** none changed.

- [ ] **Step 1:** In `reference/starlink-ast`: `git fetch origin && git switch -c u/timj/DM-56839-vendored origin/master`.
- [ ] **Step 2:** Build (Global Constraints command) and run `ctest --test-dir build -j 8 > /tmp/... 2>&1`; record the pass/fail list. Expected: all pass. A failure here is pre-existing and goes in the final report, not fixed in this plan.
- [ ] **Step 3:** Record `git rev-parse HEAD` as SUB_BASE.

---

### Task 2: `testwcsmap.c` and the QSC face-centre fix

**Files:**
- Create: `ast_tester/testwcsmap.c`
- Modify: `ast_tester/CMakeLists.txt` (`ast_add_test(testwcsmap)`), `ast_tester/Makefile.am` (add to the test programs as the others are)
- Modify: `wcslib/proj.c:3925`

**Interfaces:**
- Produces: `ast_tester/testwcsmap.c` with `static void fail(const char *)` (as `testxphmap.c`), `static void check_fwd(const char *label, int type, const char *pv, double phi_deg, double theta_deg, double x_want, double y_want, double tol)` and `static void check_rev(...)` with the same shape (x, y in, phi, theta out), which build a 2-axis `astWcsMap(2, type, 1, 2, pv)`, transform one point with `astTran2` (WcsMap works in radians on both sides: convert degrees with `AST__DD2R`), and `fail` with the label when an output differs from the wanted value by more than `tol` degrees or is `AST__BAD` when a value is wanted (pass `AST__BAD` as a wanted value to require a bad output). Later tasks add cases to `main`.

- [ ] **Step 1: Write the failing test**

Cases (wanted values from wcslib 8.10's `qscs2x`, computed with the research harness at `/private/tmp/claude-501/-Users-timj-work-lsst-starlink-astrs/9bf4ca3a-77a3-43d1-86d6-0a94a4231eee/scratchpad/h`, or rebuilt from `../wcsLib-8.10/C/prj.c`, printed with `%.17g`):

```c
/* QSC, face 4 centre: the small-angle branch. */
check_fwd( "qsc face 4 centre", AST__QSC, "", -90.005, 0.0, X1, Y1, 1e-10 );
check_fwd( "qsc face 4 near", AST__QSC, "", -89.995, 0.004, X2, Y2, 1e-10 );
```

with X1 ≈ 269.9949, X2 ≈ 270.0057, Y2 ≈ 0.00476 replaced by 8.10's exact output. Also a round trip at each point.

- [ ] **Step 2:** Build and run `ctest --test-dir build -R testwcsmap --output-on-failure`. Expected: FAIL (fork gives x = 269.5382 and (270.403, 0.3344)).
- [ ] **Step 3:** `wcslib/proj.c:3925`: `p *= (90.0 + p)*D2R;` becomes `p = (90.0 + p)*D2R;` (upstream 3.0 rewrote `qscs2x`; no CHANGES entry).
- [ ] **Step 4:** Run the test. Expected: PASS.
- [ ] **Step 5:** Commit: "wcslib: QSC forward was wrong within 0.008 deg of face 4's centre".

---

### Task 3: HPX fixes

**Files:** Modify `wcslib/proj.c:4252`, `:4295`, `:4304`, `:4311`, `:4316`; add cases to `ast_tester/testwcsmap.c`.

- [ ] **Step 1: Write the failing cases** (wanted values from 8.10's `hpxs2x`/`hpxx2s`):
  - (a) `"PV2_1=3,PV2_2=3"`, fwd(-180, -90): x = -120 (fork -240). `"PV2_1=4,PV2_2=4"`, fwd(-131.9, -90): x = -90 (fork -180).
  - (b) `"PV2_1=3,PV2_2=3"`, rev(x=-0.4, y=-119.5): valid, 8.10's (phi, theta) (fork rejects).
  - (c) default HPX, fwd(90, -89.26) gives (134.288, -89.288); rev of that is valid and returns (90, -89.26) to 1e-9 (fork rejects).
  - (d) default HPX, fwd(phi, -90) for phi in {-180, -45, 0, 90}; rev of each is valid with theta = -90 (fork rejects the pole).
- [ ] **Step 2:** Run. Expected: FAIL on each.
- [ ] **Step 3:** Fix, each as 8.10 does: `+ 1/2` becomes `+ 0.5` at `:4252` and `:4304` (integer division); `absy <= 90` becomes `absy <= w[4]*w[6]` at `:4295` (upstream `ylim`, 4.7); `fabs(t) <= w[6]` becomes `<= w[6] + 1.0e-12` at `:4316` (upstream `slim`, 4.7/4.20); at `:4311` the pole (`sigma == 0`) is accepted with `s = 1e9, t = 90` as upstream 4.3 does, rather than returning status 2.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit: "wcslib: HPX forward and reverse fixes from upstream (4.3, 4.7, 4.20)", the body listing (a)-(d).

---

### Task 4: ZPN far-side test for degree 2

**Files:** Modify `wcslib/proj.c:1356`; add a case to `ast_tester/testwcsmap.c`.

- [ ] **Step 1:** Case: `"PV2_1=1.27118,PV2_2=-0.210643"`, fwd(5.4085552, -89.2063216) must be bad (`AST__BAD` wanted).
- [ ] **Step 2:** Run. Expected: FAIL (fork returns a value).
- [ ] **Step 3:** Remove `&& prj->n > 2` from the guard at `:1356`: it worked around the uninitialised `w[0]` that `7b67e1b4` fixed, and upstream applies the far-side test at every degree.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit: "wcslib: ZPN rejects far-side points for a degree-2 polynomial, as upstream does".

---

### Task 5: PCO forward near theta = 0

**Files:** Modify `wcslib/proj.c` `astPCOset` and `astPCOfwd` (~`:3162`); add a case.

- [ ] **Step 1:** Case: fwd(90, 1e-9) has y = 8.10's value (≈ 2.23e-9; fork 1.0e-9), tolerance 1e-15.
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3:** Port 8.10's small-theta branch of `pcos2x` only: for `|theta| < 1e-4`, `x = w[0]*phi*cosd(theta); y = (w[0] + w[3]*x*x)*theta` with `w[3] = D2R/w[2]` set in `astPCOset` (upstream 4.22). Leave `astPCOrev` as it is: 8.10's `pcox2s` returns NaN for x = 0 with |y| ≥ 1e-4.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit: "wcslib: PCO forward near the equator without cancellation (upstream 4.22)".

---

### Task 6: COP at the poles

**Files:** Modify `wcslib/proj.c:2570-2592`; add cases.

- [ ] **Step 1:** Cases: `"PV1_1=60,PV1_2=0"` fwd(-180, -90) is bad; `"PV1_1=-45,PV1_2=5"` fwd(0, 90) is bad; and the pole in the hemisphere of theta_a returns r = 0 exactly (x and y exactly 8.10's).
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3:** Port 8.10's pole handling in `cops2x`: r = 0 exactly at the pole on theta_a's side; the opposite pole is bad.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit: "wcslib: COP at the poles as upstream (4.22)".

---

### Task 7: `wcsmath.h` macro parentheses

**Files:** Modify `wcslib/wcsmath.h:62-65`; add a case.

- [ ] **Step 1:** Case: MER fwd(0, 89.999999) has y bit for bit 8.10's (1063.2311737966…, fork 1063.2311752544), tolerance 0.
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3:** Parenthesise `D2R`, `R2D` and `SQRT2INV` as upstream 8.7 does, so `angle*D2R` is `angle*(PI/180)`.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit: "wcslib: parenthesise D2R, R2D and SQRT2INV (upstream 8.7)". The body notes it moves last-bit results of most projections near the poles.

---

### Task 7b: WcsMap accepts native coordinates within wcslib's tolerance

**Files:** Modify `src/wcsmap.c` (the clamp at ~`:2893-2925` and the projection table, or a switch, giving each projection its tolerance); add cases to `ast_tester/testwcsmap.c`.

AST absorbs exactly one ULP past a limit (`90*DBL_EPSILON` at ±90 latitude, `(longhi-longlo)*DBL_EPSILON` below the lower longitude limit), added so a header's validity would not depend on FMA contraction. wcslib 8.10's `prjbchk` clamps anything within a per-projection tolerance in degrees and rejects beyond it, so AST returns BAD for 3-15% of near-pole edge points that wcslib accepts (SFL, PAR, MOL, BON and PCO reverse of a forward at -180 lands up to 8e-12 below -180; ARC and COD at the antipode land 2.8e-14 past -90).

- [ ] **Step 1: Write the failing cases.** For ARC, a reverse whose latitude is 2 to 7 ULP past -90 (from the antipode; the research harness at `/private/tmp/claude-501/-Users-timj-work-lsst-starlink-astrs/9bf4ca3a-77a3-43d1-86d6-0a94a4231eee/scratchpad/h` has examples, or search x, y near r = 180 with wcslib's `arcx2s`) is valid with latitude exactly -90. For SFL, a reverse of fwd(-180, theta) whose longitude lands between 1 ULP and 1e-12 below -180 is valid with longitude exactly -180. For TAN, a latitude 2e-13 past 90 is still bad (beyond its 1e-13). The existing 1-ULP cases keep passing.
- [ ] **Step 2:** Run. Expected: FAIL on the first two.
- [ ] **Step 3:** Replace the two 1-ULP clamps with wcslib's rule and tolerances: a latitude or longitude past a limit by at most `tol` degrees is set to the limit, beyond it is bad; `tol` is 1e-13 for AZP, TAN, STG, SIN, ARC, ZPN, ZEA, AIR, CYP, CEA, CAR, MER, AIT, COP, COE, COD, COO, TSC, CSC, QSC and AST's TPN; 2e-13 for SZP; 1e-12 for SFL, PAR, PCO, HPX and XPH; 1e-11 for BON and MOL. Keep AST's half-open upper longitude limit (a value at or past `longhi` is the wrap of `longlo` and stays clipped, to stop a whole-sky grid mapping two pixels to one meridian); the tolerance applies at the lower limit and at both latitude limits. Rewrite the comment to say this is wcslib's `prjbchk` rule and that it also covers the FMA case the old clamp was written for. History entry in `wcsmap.c`.
- [ ] **Step 4:** Run. Expected: PASS, and full `ctest`, moved fixtures listed for Task 11.
- [ ] **Step 5:** Commit: "WcsMap: accept native coordinates within wcslib's tolerance at the limits".

---

### Task 8: ERFA 2.0.1

**Files:**
- Replace: every `erfa/*.c` and `erfa/erfa.h`, `erfa/erfam.h`, `erfa/LICENSE` present in AST with `/Users/timj/work/lsst/erfa/src`'s
- Create: `erfa/erfadatextra.c`, `erfa/erfadatextra.h`, `erfa/erfaextra.h` (copied)
- Modify: `palwrap.c` (include `erfa/erfadatextra.c`), `erfa2ast.h` (`eraDatini`, `eraGetLeapSeconds`, `eraSetLeapSeconds`), `Makefile.am` `ERFA_FILES` (add `erfadatextra.c`, and drop the stale `erfa/00READ.ME`), header lists if CMake installs them
- Test: `ast_tester/testskyframe.c` (or a new `ast_tester/testerfa.c` if testskyframe's structure does not fit)

AST's `erfa/` is ERFA v1.3.0. Of the 81 files AST reaches, five change code: `dat.c` (IYV 2016 → 2023; the table moves behind `eraDatini` and thread-local storage in `erfadatextra.c`; ΔAT unchanged, last entry 2017-01-01 = 37 s), `jd2cal.c` (compensated summation; no change for AST's `2400000.5 + mjd` pattern over 1M MJDs), `p06e.c` (a sign in an output AST never uses), `eors.c` and `pnm06a.c` (no effect). `erfam.h` changes `ERFA_DAU` to 149597870.7e3 (IAU 2012), which moves `palPvobs` by 4.7e-9 relative, and `ERFA_DNINT` no longer returns -0.0.

- [ ] **Step 1: Write the failing tests**
  - `palPvobs` through its public consumer: an AzEl SkyFrame with ObsLon/ObsLat/ObsAlt set, converted from ICRS at a fixed epoch, gives the value computed by calling `eraPvobs`-based arithmetic with `ERFA_DAU = 149597870.7e3` (equivalently: capture C's output after the change and assert it, with the before value in a comment and the difference stated); the test fails on the old ERFA.
  - Formatting: an angle that rounds to zero seconds (e.g. `-1e-12` radians with `Format(1)=dms`) formats without a leading `-` — or, if it formatted as `-0` before and C AST wants that, the test pins the old text and the task records the change; decide by running both builds and say which in the commit.
- [ ] **Step 2:** Run. Expected: FAIL (old constant).
- [ ] **Step 3:** Copy the files, add the renames and the include, update `Makefile.am`. Check the single-TU build for the `changes`/`NDAT` shadowing between `dat.c` and `erfadatextra.c` (legal; leave it) and that `__thread` is selected under `-std=c99`.
- [ ] **Step 4:** `nm build/libast_pal.a | rg ' T era'` prints nothing (every symbol renamed). Full `ctest`. Expected: the new tests pass; any other failure is a fixture whose numbers moved, listed for Task 11, not edited here.
- [ ] **Step 5:** Commit: "Update ERFA to 2.0.1 (SOFA 20231011)", the body listing the five code changes and the `erfam.h` constants.

---

### Task 9: PAL routines that ERFA now provides, bit-identical

**Files:** Modify `pal/palDe2h.c`, `pal/palDh2e.c`, `pal/palDs2tp.c`, `pal/palDtp2s.c` (bodies become calls, History entries added); copy `erfa/hd2ae.c`, `erfa/ae2hd.c`, `erfa/tpxes.c`, `erfa/tpsts.c` and add them to `palwrap.c`, `erfa2ast.h`, `Makefile.am`.

The research harness found 0 differences in 200k random inputs for each pair, so there is no failing test to write: the gate is the existing suite plus a bit-equality check in this task.

- [ ] **Step 1:** Add to `ast_tester/testskyframe.c` (or `testerfa.c` from Task 8) a check that an AzEl↔HADec round trip through a SkyFrame gives the same bits as before the change, by comparing against values captured from the build at the end of Task 8 (`%a` hex-float literals).
- [ ] **Step 2:** Run. Expected: PASS before the change (it pins current behaviour).
- [ ] **Step 3:** Replace each body: `palDe2h` → `eraHd2ae(ha, dec, phi, az, el)`; `palDh2e` → `eraAe2hd(az, el, phi, ha, dec)`; `palDs2tp` → `*j = eraTpxes(ra, dec, raz, decz, xi, eta)`; `palDtp2s` → `eraTpsts(xi, eta, raz, decz, ra, dec)`. History: "Call ERFA's eraX, which ERFA 2.0 added, instead of PAL's own copy of the same algorithm."
- [ ] **Step 4:** Run. Expected: PASS, and full `ctest` unchanged from Task 8.
- [ ] **Step 5:** Commit: "PAL: call ERFA's horizon and tangent-plane routines".

---

### Task 10: PAL FK4/FK5 and `palDtps2c` through ERFA

**Files:** Modify `pal/palFk45z.c`, `pal/palFk54z.c`, `pal/palFk524.c`, `pal/palDtps2c.c` (History entries); copy `erfa/fk45z.c`, `erfa/fk524.c`, `erfa/fk54z.c`, `erfa/tpors.c` and add them as in Task 9.

Research results: `palFk45z`, `palFk524` and `palDtps2c` differ from ERFA by rounding only (≤ 8.9e-16 rad; `palDtps2c` ≤ 5e-15 rad, and its 0/1/2-solution decision can flip only at the degenerate boundary). `palFk54z` differs materially away from B1950: PAL applies the fictitious proper motion linearly in (α, δ), ERFA in Cartesian with normalisation — 2.4e-11 rad at B1960, up to 4.7e-10 rad (0.1 mas) over B1900-2050, rounding-level at B1950; the returned proper motions are identical.

- [ ] **Step 1: Write the failing test** in `ast_tester/testslamap.c`: a SlaMap `FK54Z` at Besselian epoch 1980 transforms (α, δ) = (1.0, 0.5) rad to the result of `eraFk54z(1.0, 0.5, 1980.0, ...)` called directly (the test includes `erfa.h`, which renames to the `astEra` symbol libast exports), to the bit; likewise `FK45Z`. At B1950 the two agree to rounding, so the B1980 case is the one that fails first.
- [ ] **Step 2:** Run. Expected: FAIL for FK54Z (PAL's linear proper motion), rounding-level or PASS for FK45Z.
- [ ] **Step 3:** Replace each body with the ERFA call: `palFk45z` → `eraFk45z(r1950, d1950, bepoch, r2000, d2000)`; `palFk54z` → `eraFk54z(r2000, d2000, bepoch, r1950, d1950, dr1950, dd1950)`; `palFk524` → `eraFk524(...)` with the same argument order; `palDtps2c` → `*n = eraTpors(xi, eta, ra, dec, raz1, decz1, raz2, decz2)`. History entries name the change in results for `palFk54z`.
- [ ] **Step 4:** Run. Expected: PASS; full `ctest`, with moved fixtures listed for Task 11.
- [ ] **Step 5:** Commit: "PAL: FK4/FK5 conversions through ERFA", the body stating the FK54Z change and its size.

---

### Task 11: Regenerate C AST's fixtures and record the changes

**Files:** `ast_tester/fixtures/**` (regenerated, only those that move), `ast.news`.

- [ ] **Step 1:** Regenerate the oracles and wcsconv expectations with C AST's own generators (`gen_transform_oracle`, `wcsconverter` over `fixtures/wcsconv/inputs` with `cases.txt`, and the others whose outputs are committed under `ast_tester/fixtures`), as the fixture README in `ast_tester/fixtures/oracle/README.md` describes.
- [ ] **Step 2:** For every changed fixture, attribute the change to one of Tasks 2-10 (a fixture that moves for no listed reason is a bug: stop and find it). Expected: projection fixtures through Task 7's last-bit change and Tasks 2-6's cases; FK4 fixtures through Task 10; AzEl-with-observatory fixtures through Task 8.
- [ ] **Step 3:** `ast.news`, "Main Changes in this Version": one entry each for the wcslib projection fixes, ERFA 2.0.1, and the FK4 conversion change (with its size).
- [ ] **Step 4:** Full `ctest`. Expected: all pass.
- [ ] **Step 5:** Commit: "Regenerate fixtures for the wcslib, ERFA and PAL updates".
- [ ] **Step 6: Hand-off.** Stop and give the user the branch name `u/timj/DM-56839-vendored` and SUB_BASE..HEAD to push and open the upstream pull request. Task 12 waits until the user has pushed (CI's `fixture-mirror` job and fresh clones need the commit upstream).

---

### Task 12: Re-pin and re-vendor in this repository

**Files:** `reference/starlink-ast` (pin), `crates/starlink-ast/vendor/{wcslib,erfa,pal}/**`, `vendor/palwrap.c`, `vendor/erfa2ast.h`, `vendor/pal2ast.h`, `vendor/README.md`, mirrored fixtures under `crates/starlink-ast/tests/fixtures/`, `CLAUDE.md` ("What NOT to do" row on `vendor/wcslib`), `PLAN.md` §7, `CHANGELOG.md`.

- [ ] **Step 1:** `git switch -c tickets/DM-56839-vendored main`, carrying the design spec and the `ast-bench` and C-engine commits Steps 5 and 6 use; in the submodule `git checkout <pushed HEAD>`; `git add reference/starlink-ast`.
- [ ] **Step 2:** Re-copy the vendored trees from the submodule, keeping `vendor/wcslib/proj.c`'s one local change (`#include "memory.h"`) and `ast_memory_stub.c`; record the new origin commit in `vendor/README.md`.
- [ ] **Step 3:** Copy every changed fixture the `fixture-mirror` job compares into `crates/starlink-ast/tests/fixtures/`; run `tools/oracle_fixtures/check_corpus_fresh.sh` and `check_c_headers_fresh.sh`. Expected: no card-only drift.
- [ ] **Step 3b:** Port Task 7b's tolerance rule to `crates/starlink-ast/src/mappings/wcsmap.rs` (both clamp sites, ~`:1113` and `:1242`, and the per-projection tolerance), with a Rust test mirroring Task 7b's cases, written first.
- [ ] **Step 4:** `cargo test -p starlink-ast`. Expected failures, each handled and none loosened: Rust tests holding C values that moved (regenerate them with their probe, or update the value citing the submodule commit); a `*_LICENSED` case that now matches C exactly fails its stale check (remove it from the list and `PLAN.md` §6).
- [ ] **Step 5:** `ast-bench bits --base main` and `ast-bench ulps --base main`: every changed case attributable to Tasks 2-10.
- [ ] **Step 6:** Rebuild the C engine (`tools/perf/build_c_engine.sh bench`) and record the step-1 baseline the spec asks for: `ast-bench snapshot --set native` as `docs/perf/<date>-macos-aarch64-native-vendored.tsv`, with a `docs/perf/README.md` row.
- [ ] **Step 7:** `CLAUDE.md`: the row forbidding C edits in `vendor/wcslib` becomes "Edit `vendor/` only by re-copying from the submodule"; `PLAN.md` §7 row: step 0 done; `CHANGELOG.md` `Changed`: the projection fixes, ERFA 2.0.1 and the FK4 change, as user-visible.
- [ ] **Step 8:** Commit: "Re-pin C AST and re-vendor wcslib, ERFA and PAL"; the user pushes.

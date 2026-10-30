# Native PAL and ERFA: bit-identical port, vendored C deleted, hot spots vectorized on measurement — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port every PAL and ERFA function the crate calls to Rust, bit for bit, delete `vendor/erfa`, `vendor/pal`, `palwrap.c`, `pal2ast.h`, `erfa2ast.h` and the crate's C build, then vectorize only the functions that measurement shows are real per-point hot spots.

**Architecture:** `pal.rs` becomes a directory module whose wrappers keep today's signatures, so no caller changes; each wrapper's FFI call becomes a call to a native body in `pal/` or `erfa/`.
ERFA functions are one file each in the existing `erfa/` module; PAL functions with their own arithmetic are one file each in `pal/`, and PAL's one-line `palOne2One.c` wrappers become the wrapper bodies in `pal/mod.rs`.
Every function is checked bit for bit against the still-linked C until the C is deleted, after which golden values captured from the C keep the check, as for wcslib.

**Tech Stack:** Rust 2024 (`starlink-ast`, `ast-bench`); the vendored C stays linked only until Task 12, for differential tests; the C engine built by `tools/perf/build_c_engine.sh bench` for fixtures and timing.

**Spec:** `docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md`, section "PAL and ERFA stage (2026-10-29)" and everything it refers to.
Models: `docs/superpowers/plans/2026-10-27-native-erfa-sphmap.md` and `docs/superpowers/plans/2026-10-28-native-wcslib-wcsmap.md` (both done); `src/erfa/c2s.rs` is the file pattern, `src/wcslib/golden_tests.rs` the golden-value pattern.

**Scope, re-measured 2026-10-29:** the closure of the 52 roots in `pal.rs`, with C comments stripped before scanning for calls, is 130 functions: 79 ERFA functions in 81 files (less `c2s` and `s2c`, already Rust) and 51 PAL functions in 26 files.
`eraDatini`, `eraSetLeapSeconds` and `eraGetLeapSeconds` (`erfadatextra.c`) are in C's closure but are not ported (spec, "Leap seconds").

## Global Constraints

- Branch `tickets/DM-56839-erfapal`, stacked on `tickets/DM-56839-wcsmap`; one pull request; nothing is pushed.
- Port first, vectorize only measured hot spots (user direction, 2026-10-29): Tasks 2–12 add no kernel, no slice entry and no `vmath` call.
- Everything in `erfa` and `pal` is `pub(crate)` or narrower; both depend only on their own types, `f64` math, `vmath` and `BAD_VALUE`, never on Mapping, Frame or `Points` types; `pal` depends on `erfa`, never the reverse.
- The `pal` wrappers' signatures (the `pub(crate) fn` list in today's `pal.rs`) do not change, and no caller outside `pal/` changes until Task 14, except where a wrapper's bare status integer becomes a typed status (Task 4: `djcl`, `cldj`, `dtf2d`; Task 6: `gd2gc`, `gc2gd` keep their `Option`).
- Every new file opens with the two AURA/SPDX lines (`test_source_headers`).
- Every ERFA-derived file carries ERFA's notice, copied verbatim from `src/erfa/c2s.rs` lines 3–61, then `// Ported from ERFA eraXxx (erfa/xxx.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.`
- Every PAL-derived file carries `// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.` then `// Ported from PAL palXxx (pal/palXxx.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.` (this plan's call, from `COPYRIGHT`'s PAL line).
- No C file or line citations in `///` docs; provenance stays in `//` comments.
- Bit identity: C's operations in C's order, with these translations, each confirmed by the function's differential test rather than assumed:
  - `fmod` → `%`; `ERFA_DINT`, `ERFA_DNINT`, `ERFA_DSIGN` → `erfa::dint`, `erfa::dnint`, `erfa::dsign`, written as the `erfam.h` macros are.
  - A `sin(x)` and `cos(x)` of the same `x` in one function → `erfa::sincos(x)`, as clang fuses them (`palwrap.c` is one translation unit, so a pair split across an inlined call can fuse too); if the differential test shows C did not fuse a pair, use `f64::sin` and `f64::cos` and say so in a comment.
  - C `long` arithmetic → `i64`, `int` → `i32`; a `double`→integer cast that C performs on a value that can be out of range or NaN mirrors what clang's build gives, with a comment (repository rule).
  - Series sums keep C's loop direction (`nut00a`, `s06`, `dtdb`, `epv00` sum from the last term).
  - Coefficient tables are `const` arrays whose literals are copied character for character from the C; integer multipliers keep an integer type and convert to `f64` where C does.
  - `ERFA_*` and `PAL__*` constants come from `erfam.h` and `palmac.h` as `const`s in `erfa/mod.rs` and `pal/mod.rs`, each the C expression transcribed (so `ERFA_DAS2R` is `4.848136811095359935899141e-6`, not recomputed).
- Bodies use `f64` methods directly (`#![allow(clippy::disallowed_methods)]` is already on `erfa`; add it to `pal`), not the counted wrappers in `crate::math`.
- Call counts: each `pal` wrapper calls `crate::call_counts::hit("astPalXxx")` (or `"astEraXxx"`) exactly where it called `counted!` before, once per call; nothing inside `erfa` or `pal` bodies counts.
- C status codes become Rust types, never bare integers (user decision, 2026-10-29). A status that means no result is an error: the function returns `Result<T, E>` with a per-function `enum E` whose variants name C's codes (`Cal2jdError::{BadYear, BadMonth}`). A status that comes with a result is a warning: the function returns the result with a per-function warning `enum` in an `Option` (`cal2jd`'s bad day, `tf2d`'s out-of-range field, `dat`'s pre-UTC date, `epv00`'s date range). Each enum's doc says which C code each variant is, in a `//` comment. Callers that tested C's integer match on the variant instead.
- Relaxations: none are expected in this plan, since every gate is bit identity; if one is ever needed it is per case, named in source and counted in `PLAN.md` §6.
- `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `#![warn(missing_docs)]` stay clean.
- Run cargo with conda's variables unset: `env -u CC -u CXX -u CFLAGS -u CPPFLAGS -u LDFLAGS cargo …`; the shell is zsh.
- Each port task ends with `cargo run --release -p ast-bench -- bits --base HEAD~1` after its commit, expected all match; a mismatch is fixed and the commit amended.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **NaN, `BAD_VALUE` and huge dates into the calendar path** (`pal::dat`, `pal::djcl`, `pal::cldj`, `tdb_conv`): `eraJd2cal` rejects out-of-range dates but lets NaN through to `(long)` casts, and `palDat` ignores both statuses and reads outputs `jd2cal` did not write. The port does not panic, and gives what the C gives on macOS aarch64 for NaN, ±∞, `BAD_VALUE`, ±1e300; where C's result is undefined, `dat` returns 0.0 (`eraDat`'s initialised `deltat`) and a comment says so. Pinned in Task 4.
2. **Leap-second instants**: `dat` at the MJD of every table entry, one ULP either side, and a fraction of the day within one ULP of 1.0 (where `jd2cal`'s day rounding decides the month) gives C's bits; so does `ast_dat` at the same instants, forward and inverse. Pinned in Task 4.
3. **`deuler` with an unusual order string** (empty, longer than three characters, upper case, characters other than x/y/z): same matrix as C, no panic. Pinned in Task 3.
4. **A singular or near-singular matrix into `dmat` / `dmat_det`**: same singular flag, same partially reduced `a` and `y`, same determinant bits as C. Pinned in Task 3.
5. **Epochs far from J2000 and at the poles** (epochs 1000, 1900, 2000, 2100, 3000 and `BAD_VALUE`; declinations ±π/2 and ±(π/2 − 1e-15)) into the FK, precession, galactic and apparent-place functions: C's bits, and no panic. Pinned in the input generators of Tasks 7–11 (each test's inputs include them).

---

### Task 1: Bench cases for the PAL per-point callers, and the stage baseline

**Files:**
- Modify: `tools/perf/gen_bench_fixtures.c` (four constructors, four table rows)
- Create: `crates/ast-bench/fixtures/{slamap_amp,slamap_fk45z,slamap_prec,timemap_mjdtobep}.map`; Modify: `crates/ast-bench/fixtures/bench.oracle` (regenerated)
- Modify: `crates/ast-bench/sets/bench.txt`, `crates/ast-bench/sets/native.txt`
- Create: `docs/perf/<date>-macos-aarch64-native-erfapal-baseline.tsv`; Modify: `docs/perf/README.md`
- Modify: `docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md` (the stage's closure counts: "about 130 functions: 79 ERFA functions in 81 files and 51 PAL functions in 26 files", replacing "about 150 … 90 of ERFA's 236 files and 26 of PAL's 29")

**Interfaces:**
- Produces: bench cases `slamap_amp` (`astSlaAdd(m, "AMP", 2, {55000.0, 2000.0})`, inputs 0 .. 1.5), `slamap_fk45z` (`"FK45Z"`, `{1950.0}`, inputs 0 .. 1.5), `slamap_prec` (`"PREC"`, `{1950.0, 2000.0}`, inputs 0 .. 1.5), `timemap_mjdtobep` (`astTimeAdd(m, "MJDTOBEP", 2, {0.0, 0.0})`, inputs 50000 .. 50100); and this task's commit, referred to below as PBASE.

- [ ] **Step 1: Add the four constructors and rows** in the style of `slamap` and `timemap` (`gen_bench_fixtures.c:148-158`).
- [ ] **Step 2: Regenerate** — `tools/perf/build_c_engine.sh bench && target/perf/c/bench/gen_bench_fixtures crates/ast-bench/fixtures`. Expected: `git diff --stat crates/ast-bench/fixtures` shows four new files and `bench.oracle` gaining four sections; no existing section changes (if one does, stop: the C engine does not match the pin).
- [ ] **Step 3: Add the four lines to `bench.txt` and `native.txt`**, and correct `native.txt`'s two comments: `specmap_wn_to_fr` is `# SpecMap: arithmetic only, no PAL` and `timemap_taiutc` is `# TimeMap: AST's own leap-second table, no PAL`. New lines: `# SlaMap AMP: palAmpqk`, `# SlaMap FK45Z: eraFk45z`, `# SlaMap PREC: palDcs2c, palDmxv, palDcc2s`, `# TimeMap MJDTOBEP: palEpb`.
- [ ] **Step 4: Check every case on both engines** — `cargo run --release -p ast-bench --features c-oracle -- run --set native --engine both`; expected exit 0 and rows on both engines for every case.
- [ ] **Step 5: Snapshot on a quiet machine** — `cargo run --release -p ast-bench --features c-oracle -- snapshot --set native`, saved as `docs/perf/<date>-macos-aarch64-native-erfapal-baseline.tsv`, with a README row ("PAL/ERFA stage baseline: native set plus the four PAL per-point cases; PAL and ERFA still C").
- [ ] **Step 6: Commit** — "Add bench cases for SlaMap and TimeMap's PAL calls and record the PAL/ERFA baseline". This commit is PBASE. Hand the user the x86-64 commands (`snapshot --set native` and `compare --set native --base tickets/DM-56839-wcsmap`) per the spec's Platforms section.

---

### Task 2: `pal` as a directory, the differential-test harness, and ERFA's vector, matrix and angle functions

**Files:**
- Create: `crates/starlink-ast/src/pal/mod.rs` (today's `pal.rs` less its `extern` block), `crates/starlink-ast/src/pal/ffi.rs` (the `extern` block, moved unchanged); Delete: `crates/starlink-ast/src/pal.rs`
- Create: `crates/starlink-ast/src/erfa/difftest.rs` (`#[cfg(test)]`: `extern "C"` declarations of every `astEra*` symbol the port's differential tests call, added to as each task needs them)
- Modify: `crates/starlink-ast/src/erfa/mod.rs` (constants, `dint`, `dnint`, `dsign`, module list)
- Create, one file each: `crates/starlink-ast/src/erfa/{anp,anpm,pas,pdp,pm,pmp,pn,ppp,ppsp,pxp,sxp,sepp,seps,pv2s,s2pv,pvu,rx,ry,rz,rxp,rxpv,rxr,tr,trxp,rv2m,rm2v}.rs`

**Interfaces:**
- Produces in `erfa` (all `pub(crate)`, `#[inline]`; vectors `[f64; 3]`, matrices `[[f64; 3]; 3]`, pv-vectors `[[f64; 3]; 2]`, all by value or `&`):
  - `anp(a) -> f64`, `anpm(a) -> f64`, `pas(al, ap, bl, bp) -> f64`, `seps(al, ap, bl, bp) -> f64`, `sepp(a: &V3, b: &V3) -> f64`
  - `pdp(a, b) -> f64`, `pm(p) -> f64`, `pmp(a, b) -> V3`, `pn(p) -> (f64, V3)` (modulus, unit vector), `ppp(a, b) -> V3`, `ppsp(a, s, b) -> V3`, `pxp(a, b) -> V3`, `sxp(s, p) -> V3`
  - `pv2s(pv) -> (f64, f64, f64, f64, f64, f64)` (θ, φ, r, θ̇, φ̇, ṙ), `s2pv(theta, phi, r, td, pd, rd) -> PV`, `pvu(dt, pv) -> PV`
  - `rx(phi, r: M3) -> M3`, `ry`, `rz` likewise; `rxp(r, p) -> V3`, `rxpv(r, pv) -> PV`, `rxr(a, b) -> M3`, `tr(r) -> M3`, `trxp(r, p) -> V3`, `rv2m(w) -> M3`, `rm2v(r) -> V3`
  - `type V3 = [f64; 3]; type M3 = [[f64; 3]; 3]; type PV = [[f64; 3]; 2];` in `erfa/mod.rs`
  - `dint(a) -> f64`, `dnint(a) -> f64`, `dsign(a, b) -> f64`; the `erfam.h` constants used anywhere in the closure
- Hand-written on plain arrays, not a linear-algebra crate (user decision 2026-10-29; a crate is a `PLAN.md` §7 item for later).
- `eraCp`, `eraCr`, `eraZp`, `eraIr` are not ported as functions: Rust arrays are `Copy`, and `[0.0; 3]` and the identity matrix are literals (this plan's call). A C function that writes its result into one of its inputs (`eraRxp(r, p, p)`) is ported to return the value, and the differential test covers the aliased call.
- Test inputs (in `crate::test_inputs`, so they outlive `difftest.rs`): `pub(crate) fn vectors(n) -> Vec<[f64; 3]>` (unit vectors, vectors scaled by 1e-300 … 1e300, the zero vector, and every triple of `specials()`), `pub(crate) fn matrices(n) -> Vec<[[f64; 3]; 3]>` (random rotations, random general matrices, the identity, a zero matrix, matrices carrying specials).

- [ ] **Step 1: Move `pal.rs`** to `pal/mod.rs` + `pal/ffi.rs` (`git mv`, then split); `cargo test -p starlink-ast --lib pal::` passes unchanged.
- [ ] **Step 2: Write the differential tests**, one per function, named `<fn>_matches_vendored_c_bit_for_bit`, in each file's `#[cfg(test)] mod tests`, each comparing every output's `to_bits()` with the C function's (via `erfa::difftest`) over `vectors(20_000)`, `matrices(2_000)` and `test_inputs::angle_pairs`/`specials()` as the arguments call for. Two NaN outputs compare equal whatever their sign and payload (as `wcslib/golden_tests.rs`).
- [ ] **Step 3: Run them to see them fail** — `cargo test -p starlink-ast --lib erfa::`; expected: functions not found.
- [ ] **Step 4: Implement** the functions and `mod.rs` additions.
- [ ] **Step 5: Run them** — expected PASS; then `cargo clippy -p starlink-ast --all-targets -- -D warnings`.
- [ ] **Step 6: Commit** — "Port ERFA's vector, matrix and angle functions to Rust, checked bit for bit against the vendored C", then the bits check.

---

### Task 3: PAL's vector, matrix and angle roots on the native functions

**Files:**
- Modify: `crates/starlink-ast/src/pal/mod.rs` (wrappers `dmxv`, `dimxv`, `dmxm`, `dvxv`, `dvdv`, `dvn`, `dranrm`, `drange`, `dsep`, `dbear`, `dav2m`, `deuler`, `dmat`, `dmat_det`)
- Create: `crates/starlink-ast/src/pal/{drange,deuler,dmat}.rs`
- Modify: `crates/starlink-ast/src/pal/ffi.rs` (nothing removed: the tests still call the C)

**Interfaces:**
- Consumes: Task 2's `erfa` functions.
- Produces: `pal::drange::drange(a) -> f64`, `pal::deuler::deuler(order: &str, phi, theta, psi) -> M3`, `pal::dmat::dmat(n: usize, a: &mut [f64], y: &mut [f64]) -> (bool, f64)` (singular, determinant; the two wrappers keep their signatures). The `palOne2One.c` wrappers become wrapper bodies: for example `dmxv(dm, va)` is `hit("astPalDmxv"); erfa::rxp(dm, va)`.

- [ ] **Step 1: Write the tests** in `pal/mod.rs`'s test module, named `<wrapper>_matches_vendored_c_bit_for_bit`, each calling the wrapper and the `ffi.rs` function over Task 2's generators; `deuler` also over the order strings `""`, `"x"`, `"zyx"`, `"ZXZ"`, `"xyzx"`, `"abc"`, `"z1y"` (Review Focus 3); `dmat` over n = 1, 2, 3, 6 with random, singular (two equal rows), near-singular (rows differing by 1e-15) and special-carrying matrices, comparing the flag, every element of `a` and `y` and the determinant (Review Focus 4).
- [ ] **Step 2: Run them** before changing the wrappers — expected PASS (wrapper and C are the same code); this proves the harness.
- [ ] **Step 3: Switch the wrappers** to the native bodies, keeping each `hit` where `counted!` was.
- [ ] **Step 4: Run them** — expected PASS; `cargo test -p starlink-ast` (whole crate) passes.
- [ ] **Step 5: Commit** — "Run PAL's vector, matrix and angle routines in Rust", then the bits check.

---

### Task 4: Calendar, epochs and leap seconds

**Files:**
- Create: `crates/starlink-ast/src/erfa/{cal2jd,jd2cal,d2tf,tf2d,epb,epb2jd,epj,epj2jd,dat}.rs`, `crates/starlink-ast/src/pal/dat.rs`
- Modify: `crates/starlink-ast/src/pal/mod.rs` (wrappers `epj`, `epj2d`, `epb`, `epb2d`, `djcl`, `cldj`, `dd2tf`, `dtf2d`, `dat`), `crates/starlink-ast/src/mappings/timemap.rs` (test only, unless Step 6 shares the table)

**Interfaces:**
- Produces in `erfa`: `cal2jd(iy, im, id) -> Result<((f64, f64), Option<BadDay>), Cal2jdError>`, `jd2cal(dj1, dj2) -> Result<(i32, i32, i32, f64), Jd2calError>`, `d2tf(ndp, days) -> (char, [i32; 4])`, `tf2d(s: char, ihour, imin, sec) -> (f64, Option<Tf2dWarning>)` (`Hour`, `Minute`, `Second`), `epb(dj1, dj2) -> f64`, `epb2jd(epb) -> (f64, f64)`, `epj(dj1, dj2) -> f64`, `epj2jd(epj) -> (f64, f64)`, `dat(iy, im, id, fd) -> Result<(f64, Option<PreUtc>), DatError>` (`DatError` covers C's −1 … −4).
- The wrappers `cldj` and `dtf2d` return `(f64, Option<…>)` with the ERFA warning or error as an enum, and `djcl` returns `Result<(i32, i32, i32, f64), Jd2calError>`; their callers (`frames/date_time.rs`, `frames/timeframe.rs`, `fitschan/wcs_write/mod.rs`) match on the variants where they tested the integer.
- `dat`'s table is `const CHANGES: [(i32, i32, f64); N]` and `const DRIFT: [[f64; 2]; NERA1]`, literal for literal from `dat.c`; no `IYV`, no dubious-year status, no runtime override (spec). A comment says why the table is final.
- `pal::dat::dat(dju: f64) -> f64`: `jd2cal(PAL__MJD0, dju)` then `dat`, returning 0.0 on any error, as C's `deltat` (Review Focus 1).

- [ ] **Step 1: Write the differential tests** for the nine ERFA functions and nine wrappers. Inputs: MJD 0 … 100,000 every 0.37 days plus every leap-second table date ±1 ULP and with fractions within 1 ULP of 0 and 1 (Review Focus 2); years −4800 … 3000 including invalid months and days for `cal2jd`; `tf2d` and `d2tf` over field values in and out of range and `ndp` −2 … 10; NaN, ±∞, `BAD_VALUE`, ±1e300 for every `f64` argument (Review Focus 1). For a NaN date where C reads unwritten outputs, the test asserts only that the port returns 0.0 and does not panic, with a comment.
- [ ] **Step 2: Run them to see them fail**, then **implement**, then **run them** — expected PASS.
- [ ] **Step 3: Switch the nine wrappers**; `cargo test -p starlink-ast` passes.
- [ ] **Step 4: Write `ast_dat_and_dat_agree_on_every_date_both_accept`** in `timemap.rs`'s test module: `ast_dat(mjd, true)` against `pal::dat(mjd)` over MJD 36934 (1960-01-01) … 80000 every 0.25 days plus every table instant ±1 ULP, comparing bits.
- [ ] **Step 5: Run it.** If it passes: `ast_dat`'s forward branch reads `erfa::dat`'s `const` table through a shared `pub(crate) const LEAP_SECONDS` (UTC MJD of each change, TAI−UTC) built at compile time from `CHANGES`, and the inverse branch uses the same table only if `test_timemap` and the time oracles stay bit-identical; otherwise the inverse keeps its own thresholds and a comment says why. If it fails: rename it `ast_dat_and_dat_differ_only_at_known_dates`, assert the exact set of differing dates found, and keep two tables with a comment naming the difference (spec: "if they differ, they stay two tables, as C has two").
- [ ] **Step 6: Commit** — "Port ERFA's calendar, epoch and leap-second functions to Rust", then the bits check.

---

### Task 5: `dtdb` and the TDB conversions

**Files:**
- Create: `crates/starlink-ast/src/erfa/dtdb.rs`
- Modify: `crates/starlink-ast/src/pal/mod.rs` (`tdb_conv`, `dtdb_geocentre`)

**Interfaces:**
- Produces: `erfa::dtdb(date1, date2, ut, elong, u, v) -> f64`, its `FAIRHD` table a `const [[f64; 3]; 787]` (count as in `dtdb.c`).

- [ ] **Step 1: Write `dtdb_matches_vendored_c_bit_for_bit`** over dates JD 2,400,000.5 + (0 … 100,000) at random fractions, `ut` in [0, 1], `elong` in ±π, `u` 0 … 6,400 km, `v` ±6,400 km, plus specials in each argument.
- [ ] **Step 2: Fail, implement, pass**, keeping `dtdb.c`'s term order and its five separate partial sums.
- [ ] **Step 3: Switch the two wrappers**; `cargo test -p starlink-ast` passes.
- [ ] **Step 4: Commit** — "Port ERFA's TDB−TT series to Rust", then the bits check.

---

### Task 6: Geodetic and observatory position

**Files:**
- Create: `crates/starlink-ast/src/erfa/{eform,gd2gc,gd2gce,gc2gd,gc2gde}.rs`, `crates/starlink-ast/src/pal/{geoc,pvobs}.rs`
- Modify: `crates/starlink-ast/src/pal/mod.rs` (`gd2gc`, `gc2gd`, `geoc`, `pvobs`)

**Interfaces:**
- Produces: `erfa::Ellipsoid` (`Wgs84`, `Grs80`, `Wgs72`; C's `n` = 1, 2, 3, so an invalid `n` cannot be expressed), `eform(e: Ellipsoid) -> (f64, f64)`, `gd2gce(a, f, elong, phi, height) -> Result<V3, Gd2gceError>`, `gd2gc(e: Ellipsoid, elong, phi, height) -> Result<V3, Gd2gceError>`, `gc2gde(a, f, xyz) -> Result<(f64, f64, f64), Gc2gdeError>`, `gc2gd(e: Ellipsoid, xyz) -> Result<(f64, f64, f64), Gc2gdeError>` (this plan's call; the differential tests cover only C's valid `n`); `pal::geoc::geoc(p, h) -> (f64, f64)`, `pal::pvobs::pvobs(p, h, stl) -> [f64; 6]`. The wrappers keep returning `Option`.

- [ ] **Step 1: Write the differential tests**: latitudes over ±π/2 including the poles and ±(π/2 − 1e-15), longitudes over ±2π, heights −1e4 … 1e7 m, geocentric vectors over `vectors()` scaled by 1e3 … 1e8 including points on the polar axis and at the centre, each `Ellipsoid`, plus specials.
- [ ] **Step 2: Fail, implement, pass; switch the wrappers**; `cargo test -p starlink-ast` passes.
- [ ] **Step 3: Commit** — "Port ERFA's geodetic conversions and PAL's observatory position to Rust", then the bits check.

---

### Task 7: FK4, FK5 and Hipparcos conversions

**Files:**
- Create: `crates/starlink-ast/src/erfa/{fk45z,fk524,fk54z,fk5hip,fk5hz,hfk5z}.rs`
- Modify: `crates/starlink-ast/src/pal/mod.rs` (`fk45z`, `fk54z`, `fk5hz`, `hfk5z`)

**Interfaces:**
- Produces: `erfa::fk45z(r1950, d1950, bepoch) -> (f64, f64)`, `fk524(r2000, d2000, dr2000, dd2000, p2000, v2000) -> (f64, f64, f64, f64, f64, f64)`, `fk54z(r2000, d2000, bepoch) -> (f64, f64, f64, f64)`, `fk5hip() -> (M3, V3)`, `fk5hz(r5, d5, date1, date2) -> (f64, f64)`, `hfk5z(rh, dh, date1, date2) -> (f64, f64, f64, f64)`. `palFk5hz` / `palHfk5z` (`palOne2One.c`) convert the epoch with `epj2jd` in the wrapper, as C does.

- [ ] **Step 1: Write the differential tests** over `angle_pairs` (RA 0 … 2π, Dec ±π/2) plus the poles and ±(π/2 − 1e-15), epochs 1000, 1900, 1950, 2000, 2100, 3000 and random in 1800 … 2200, and `BAD_VALUE` and NaN in each argument (Review Focus 5).
- [ ] **Step 2: Fail, implement, pass; switch the wrappers**; `cargo test -p starlink-ast` passes.
- [ ] **Step 3: Commit** — "Port ERFA's FK4, FK5 and Hipparcos conversions to Rust", then the bits check.

---

### Task 8: Galactic and supergalactic rotations, E-terms, radial-velocity components

**Files:**
- Create: `crates/starlink-ast/src/pal/{galeq,eqgal,galsup,supgal,etrms,addet,subet,rvgalc,rvlg,rvlsrd,rvlsrk}.rs`
- Modify: `crates/starlink-ast/src/pal/mod.rs` (the ten corresponding wrappers)

**Interfaces:**
- Produces: `pal::<name>::<name>` with the wrappers' signatures; `etrms(ep) -> V3`. Each rotation matrix is a `const` copied from its C file.

- [ ] **Step 1: Write the differential tests** over the Task 7 angle and epoch inputs (Review Focus 5).
- [ ] **Step 2: Fail, implement, pass; switch the wrappers**; `cargo test -p starlink-ast` passes.
- [ ] **Step 3: Commit** — "Port PAL's galactic, E-term and radial-velocity routines to Rust", then the bits check.

---

### Task 9: Precession and the ecliptic

**Files:**
- Create: `crates/starlink-ast/src/erfa/{obl06,pfw06,fw2m,pmat06,p06e}.rs`, `crates/starlink-ast/src/pal/{prec,prebn,ecmat}.rs`
- Modify: `crates/starlink-ast/src/pal/mod.rs` (`prec`, `prebn`, `ecmat`)

**Interfaces:**
- Produces: `erfa::obl06(date1, date2) -> f64`, `pfw06(date1, date2) -> (f64, f64, f64, f64)`, `fw2m(gamb, phib, psi, eps) -> M3`, `pmat06(date1, date2) -> M3`, `p06e(date1, date2) -> [f64; 16]` (its sixteen outputs in C's argument order; this plan's call), and the three PAL bodies with the wrappers' signatures.

- [ ] **Step 1: Write the differential tests** over Julian dates for epochs 1000 … 3000 (random, plus the Task 7 list) split as C's callers split them, and `BAD_VALUE`/NaN.
- [ ] **Step 2: Fail, implement, pass; switch the wrappers**; `cargo test -p starlink-ast` passes.
- [ ] **Step 3: Commit** — "Port ERFA's IAU 2006 precession and PAL's precession and ecliptic matrices to Rust", then the bits check.

---

### Task 10: Nutation and sidereal time

**Files:**
- Create: `crates/starlink-ast/src/erfa/{fad03,fae03,faf03,faju03,fal03,falp03,fama03,fame03,faom03,fapa03,fasa03,faur03,fave03,nut00a,nut06a,pnm06a,bpn2xy,s06,eors,era00,gmst06,gst06,gst06a,ee06a}.rs`, `crates/starlink-ast/src/pal/prenut.rs`
- Modify: `crates/starlink-ast/src/pal/mod.rs` (`gmst`, `eqeqx`)

**Interfaces:**
- Produces: the thirteen `faXX03(t) -> f64`; `nut00a(date1, date2) -> (f64, f64)` (its luni-solar table `const [Ls; 678]` and planetary table `const [Pl; 687]`, `Ls`/`Pl` structs of `i8` multipliers and `f64` amplitudes as in `nut00a.c`, counts as in C); `nut06a`, `pnm06a(date1, date2) -> M3`, `bpn2xy(rbpn) -> (f64, f64)`, `s06(date1, date2, x, y) -> f64`, `eors(rnpb, s) -> f64`, `era00(dj1, dj2) -> f64`, `gmst06(uta, utb, tta, ttb) -> f64`, `gst06(uta, utb, tta, ttb, rnpb) -> f64`, `gst06a(uta, utb, tta, ttb) -> f64`, `ee06a(date1, date2) -> f64`; `pal::prenut::prenut(epoch, date) -> M3`.
- LAST↔LMST and the three-input topocentric SpecMap reach `nut00a` per point; they stay scalar (spec, user decision 2026-10-29).

- [ ] **Step 1: Write the differential tests**: `t` in ±10 Julian centuries for the `fa*03`; dates for epochs 1000 … 3000 for the rest; `eqeqx` and `gmst` over MJD 0 … 100,000; `BAD_VALUE`/NaN.
- [ ] **Step 2: Fail, implement, pass; switch the wrappers**; `cargo test -p starlink-ast` passes, including `test_timemap`, the time oracles and the topocentric SpecMap tests.
- [ ] **Step 3: Commit** — "Port ERFA's IAU 2000A/2006 nutation and sidereal time to Rust", then the bits check.

---

### Task 11: Earth ephemeris and apparent place

**Files:**
- Create: `crates/starlink-ast/src/erfa/epv00.rs`, `crates/starlink-ast/src/pal/{evp,mappa,mapqkz,ampqk}.rs`
- Modify: `crates/starlink-ast/src/pal/mod.rs` (`evp`, `mappa`, `mapqkz`, `ampqk`)

**Interfaces:**
- Produces: `erfa::epv00(date1, date2) -> ((PV, PV), Option<OutsideRange>)` (heliocentric, barycentric; C's +1 date-range warning, which comes with a result); its tables `const` arrays named and sized as in `epv00.c`. The four PAL bodies keep the wrappers' signatures.

- [ ] **Step 1: Write the differential tests**: `epv00` over dates for epochs 1000 … 3000 (its warning range included); `evp` and `mappa` over the same, with equinoxes 1950, 2000 and random; `mapqkz` and `ampqk` over `angle_pairs` with `amprms` from C's `mappa` at five epochs; poles and specials (Review Focus 5).
- [ ] **Step 2: Fail, implement, pass; switch the wrappers**. Then confirm the closure is done: `rg -n 'counted!' crates/starlink-ast/src/pal/mod.rs` prints nothing, and `cargo test -p starlink-ast` passes.
- [ ] **Step 3: Run tier 4 by hand** — `git submodule update --init reference/starlink-ast && cargo test -p ast-oracle-tests`; expected PASS (call-count parity unchanged).
- [ ] **Step 4: Commit** — "Port ERFA's Earth ephemeris and PAL's apparent place to Rust", then the bits check.

---

### Task 12: Golden values, delete the vendored C, and record the bit-identical milestone

**Files:**
- Create: `crates/starlink-ast/tests/erfa_pal_golden/roots.tsv` (not distributed, like the wcslib golden file), `crates/starlink-ast/src/pal/golden_tests.rs`
- Delete: `crates/starlink-ast/vendor/{erfa,pal,palwrap.c,pal2ast.h,erfa2ast.h}`, `crates/starlink-ast/src/pal/ffi.rs`, `crates/starlink-ast/src/erfa/difftest.rs`, and every `*_matches_vendored_c_bit_for_bit` test (spec step 2: the C halves go with the C), including those in `erfa/c2s.rs` and `erfa/s2c.rs`
- Modify: `crates/starlink-ast/build.rs` (the PAL/ERFA compilation and its `rerun-if-changed` lines), `crates/starlink-ast/Cargo.toml` (`cc` build-dependency, and the `include` list if it names the deleted paths), `crates/starlink-ast/vendor/README.md` (PAL and ERFA sections become one line each: ported to `src/pal/` / `src/erfa/`, with the commit; `palwrap.c` section removed), `COPYRIGHT` (PAL and ERFA lines name `crates/starlink-ast/src/pal` and `src/erfa` as derived code; the ERFA licence is in each file's header), `CLAUDE.md` (layout tree; the C99 vendored-language bullet and the "Editing the C in vendor/" row, which no longer apply), `README.md` and `crates/starlink-ast/README.md` (no C compiler needed), `CHANGELOG.md` (`Unreleased`: "Building no longer needs a C compiler: PAL and ERFA are now Rust."), `PLAN.md` §7 step-6a row
- Create: `docs/perf/<date>-macos-aarch64-native-erfapal-ported.tsv`; Modify: `docs/perf/README.md`

**Interfaces:**
- `roots.tsv` holds, for each of the 52 roots, the inputs and output bits of the Tasks 3–11 differential inputs thinned to at most 2,000 calls per root, written while the C is still linked by `#[test] #[ignore] fn capture_golden_values()` in `pal/golden_tests.rs` (deleted in this task after its one run, as the wcslib capture was not kept); its header names the C (`reference/starlink-ast` 3d3f255f, `-O3 -ffp-contract=off`, macOS aarch64) and the line format.
- `pal/golden_tests.rs::roots_match_the_captured_c_values` follows `wcslib/golden_tests.rs`: compares every result before failing, names the first few differences and counts the rest with each root's largest ULP difference; ignored off macOS aarch64 and in the cutdown archive with the same `cfg_attr` lines.

- [ ] **Step 1: Capture** — write the capture test, run it with `--ignored`, commit `roots.tsv` and `roots_match_the_captured_c_values` (which passes). Then delete the capture test.
- [ ] **Step 2: Delete and edit.** Then `rg -n 'palwrap|pal2ast|erfa2ast|vendor/(pal|erfa)|astPal\w+\(|astEra\w+\(' --glob '!reference/**' --glob '!docs/**' --glob '!*.tsv'` lists only `hit("astPal…")`/`hit("astEra…")` call-count names, and `rg -n 'cc' crates/starlink-ast/Cargo.toml` shows no `cc` dependency.
- [ ] **Step 3: Run** `cargo build && cargo test -p starlink-ast && cargo clippy --workspace -- -D warnings && cargo doc -p starlink-ast --no-deps`; expected PASS. `cargo package -p starlink-ast --list | rg -c '\.c$|\.h$'` prints 0, and building the package needs no C compiler: `env -u CC CC=/usr/bin/false cargo build -p starlink-ast` succeeds.
- [ ] **Step 4: Check bits** — commit "Delete the vendored PAL and ERFA", then `cargo run --release -p ast-bench -- bits --base HEAD~1`; expected all match. Run tier 4 again; expected PASS.
- [ ] **Step 5: Snapshot** the native set (Task 1 Step 5) into `…-native-erfapal-ported.tsv`, README row comparing the four PAL cases and the six FrameSets with PBASE; commit "Record PAL and ERFA after the bit-identical port". Hand the user the x86-64 commands.

---

### Task 13: Find the real hot spots, and stop for the user's decision

**Files:**
- Create: `docs/perf/<date>-erfapal-hot-spots.md`

This task decides nothing on its own: it measures, writes the table, and stops.

- [ ] **Step 1: Count PAL and ERFA calls per point** on each of the six FrameSets and the four PAL cases, forward and inverse: `cargo run -p starlink-ast --features call-counts --example count_one -- <relpath> forward|inverse` (FrameSets from the `framesets` oracle; for the bench cases, give `count_one` an optional third argument, an oracle file path, and pass `crates/ast-bench/fixtures/bench.oracle`). Record each `astPal*`/`astEra*` count divided by the points transformed.
- [ ] **Step 2: Time** the four PAL cases and the six FrameSets, Rust against C, at N = 1, 100 and 10,000, from the Task 12 snapshot.
- [ ] **Step 3: Profile** each case or FrameSet whose PAL calls per point are at least 1: run it under `ast-bench run` for a few seconds and sample it (`sample <pid> 5 1 -file target/perf/sample-<case>.txt`, as `crates/ast-bench/README.md` describes); record the share of self time in each ported PAL/ERFA body and in the libm calls (`sin`, `cos`, `atan2`, `sqrt`) they make.
- [ ] **Step 4: Write the table** in `docs/perf/<date>-erfapal-hot-spots.md`: per function, the per-point callers that reach it, calls per point on each realistic input, its share of time, its libm share, and a proposal (vectorize or leave scalar) with one line of reason. A function is proposed only if a realistic input (the six FrameSets, or a Mapping class used per point in practice) reaches it per point and its libm share is a large part of that input's time; everything else is proposed scalar.
- [ ] **Step 5: Commit** — "Measure where PAL and ERFA time goes after the port". **Stop and give the user the table.** Task 14 covers only the functions the user agrees to.

---

### Task 14: Vectorize the agreed hot spots (one sub-task per function)

Run once per function the user agreed to in Task 13, in the order the user gives, each as its own commit group. Nothing here runs for a function the user did not agree to.

**Files (per function `f`):** Modify: `crates/starlink-ast/src/erfa/<f>.rs` or `crates/starlink-ast/src/pal/<f>.rs` (kernel and slice entry), its caller (`mappings/slamap.rs` or `mappings/timemap.rs`); Modify: `crates/starlink-ast/src/pal/mod.rs` (slice wrapper).

**Interfaces (per function, following `pal::dcc2s_slice` and `erfa::c2s`):**
- `f_kernel::<const FMA: bool>(…)` (the per-value body with libm replaced by `vmath`), `f_needs_special(…) -> bool`, and in `pal/mod.rs` `f_slice(inputs: &[f64]…, outputs: &mut [f64]…)` that runs the kernel over every element then the per-value body over flagged elements, with the AVX2+FMA runtime dispatch on x86-64, counting with `call_counts::hit_n`.
- The per-value entry is unchanged (spec, "Interim").
- The caller sends every point of a transform through the slice, a single point included.

- [ ] **Step 1: Write the tests**: `f_kernel_is_within_bound_of_f` (each elementary function a kernel replaces within 1 ULP of the exact value, per the spec's per-function rule, over the Task 3–11 inputs; at least one input differs from the per-value body, proving the kernel is in use) and `f_slice_gives_each_element_the_same_bits_whatever_its_length` (lengths 1 … 17 and 1,000, every element equal to its own one-element slice).
- [ ] **Step 2: Fail, implement, pass.**
- [ ] **Step 3: Move the caller** to the slice; `cargo test -p starlink-ast` passes. Any C-oracle case that moves gets a per-case bound named in source with its measured drift, counted in `PLAN.md` §6.
- [ ] **Step 4: Measure** — `cargo run --release -p ast-bench -- ulps --base HEAD~1` (per-case drift, every changed case inspected) and `cargo run --release -p ast-bench --features c-oracle -- compare --set native --base HEAD~1`. Keep the kernel only if the case it was chosen for is faster at N = 100 and 10,000 and not slower at N = 1; otherwise revert the kernel, keep its tests deleted, and record the rejection with timings in the hot-spots file. On 2026-10-29 the user overrode the "not slower at N = 1" rule for `fk45z` and `fk54z`: FK4/FK5 is not used by large surveys, and the spec's anti-goal, that N = 1 and N = 100 stay faster than C, governs.
- [ ] **Step 5: Commit** — "Vectorize <f> for <caller>" (or "Record that <f> stays scalar").

---

### Task 15: Close the stage

**Files:**
- Create: `docs/perf/<date>-macos-aarch64-native-erfapal.tsv`, `docs/reports/<date>-native-erfapal.md`; Modify: `docs/perf/README.md`, `PLAN.md` (§7 step-6a row; §6 relaxation count if Task 14 added any), the spec's "PAL and ERFA stage" section (a "Done <date>" line naming this plan, and any decision made in Task 13), `CHANGELOG.md` (an entry only if Task 14 changed results or speed users can see)
- Modify: `/Users/timj/.claude/projects/-Users-timj-work-lsst-starlink-astrs/memory/erfa-pal-stage-scope.md` (stage done; what was vectorized)

- [ ] **Step 1: Snapshot** the native set and run `ast-bench roman` (spec, Measurement), saved as the end-of-stage record with its README row.
- [ ] **Step 2: Write the report**: the four PAL cases and six FrameSets at PBASE, after the port (Task 12), and at the end, Rust and C, at N = 1, 100, 10,000; the Task 13 table and the user's decisions; every kept and rejected kernel with its timing.
- [ ] **Step 3: Run the full gates** — `cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test -p starlink-ast && cargo doc -p starlink-ast --no-deps`, then tier 4; expected PASS.
- [ ] **Step 4: Commit** — "Record the PAL and ERFA stage". Hand the user the x86-64 commands for the final `compare --set native --base tickets/DM-56839-wcsmap`.

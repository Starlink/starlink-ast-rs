# Native wcslib for WcsMap: bit-identical port, vendored C deleted, vectorized projections — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `crates/starlink-ast/vendor/wcslib` with a native Rust `wcslib` module, port all 28 projections in the fork's `proj.c` plus TPN bit for bit, delete the C, then vectorize WcsMap's slice paths: TAN, TPN, SIN and ZPN first (phase 1), every other projection family after (phase 2).

**Architecture:** `wcslib.rs` becomes a directory module.
Each projection's forward and reverse is written once, as a function generic over a degree-trigonometry trait `Trig`; `Libm` reproduces `wcstrig.c` bit for bit and `Kernel<FMA>` uses `vmath`.
A `Prj` value (projection, parameters, set-up results) replaces the FFI `PrjParams`; its per-value entries run the `Libm` body, its in-place slice entries run the `Kernel` body over every point and then the `Libm` body for points the kernels do not cover.
WcsMap calls the slice entries once per transform, so every point a WcsMap transforms gets the same bits whatever N.

**Tech Stack:** Rust 2024 (`starlink-ast`, `ast-bench`); the vendored wcslib C stays linked only until Task 9, for differential tests; the C engine built by `tools/perf/build_c_engine.sh bench` for fixtures and timing.

**Spec:** `docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md`, section "WcsMap stage (2026-10-28)" and everything it refers to.
Model: `docs/superpowers/plans/2026-10-27-native-erfa-sphmap.md` (the SphMap stage, done); its patterns (`pal::dcc2s_slice`, `vmath`, `SPHMAP_DRIFT_LICENSED`) are the ones reused here.

## Global Constraints

- Branch `tickets/DM-56839-wcsmap`, stacked on `tickets/DM-56839-sphmap`; one pull request for both phases; nothing is pushed.
- The `wcslib` module and everything in it is `pub(crate)` except `WcsProjection`, which stays public (re-exported from `mappings`) with its current API unchanged.
- The `wcslib` module depends only on its own types, `f64` math, `vmath` and `BAD_VALUE`; never on Mapping, Frame or `Points` types.
- Every new file opens with the two AURA/SPDX lines (`test_source_headers`).
- Every file with wcslib-derived code carries, after those lines, `// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.` (this plan's call; it keeps the notice `COPYRIGHT` carries today).
- Every ported function has a `//` lineage comment: `vendor/wcslib/<file>.c`, C function, and `reference/starlink-ast` at the pinned commit (`3d3f255f`); name the wcslib 8.10 fix where step 0 adopted one.
- No C file or line citations in `///` docs.
- One body per projection direction, generic over `Trig`: no second implementation of any projection's arithmetic.
- Per-value entries (`Prj::fwd`, `Prj::rev`) keep C's operations bit for bit for as long as the spec's interim rule holds; only slice entries run kernels.
- A slice gives each element the same bits whatever its length and whichever build of its loop runs (SSE2, or AVX2+FMA chosen at run time on x86-64, as `pal::dcc2s_slice`).
- Each elementary function a kernel replaces is within 1 ULP of libm (2026-10-27 per-function rule); drift through an iterative projection is bounded by that solver's own convergence tolerance (spec, "Iterative projections").
- Relaxations are per case, named in source with the measured drift or the tolerance derivation, and counted in `PLAN.md` §6; no fixture is re-baselined in bulk.
- Call counts keep the names `wcs_fwd` / `wcs_rev`, counting one per point that reaches the projection (C counts calls of `WcsFwd`/`WcsRev`, which `Map` makes only for good points).
- `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `#![warn(missing_docs)]` stay clean.
- Run cargo with conda's variables unset: `env -u CC -u CXX -u CFLAGS -u CPPFLAGS -u LDFLAGS cargo …`; the shell is zsh.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A WcsMap whose projection parameters are unusable** (SZP with `PV2_1 = -1/sin(PV2_3)`, CYP with `PV2_2 = 0`, ZPN with every coefficient zero, so `astZPNrev`'s `k < 1` path): transforming a good point is an `AST__WCSPA` error, as today; transforming only BAD points is not an error (C never calls the projection for them). Pinned in Task 8.
2. **Arguments at exact multiples of 90°, and zero arguments to `atan2d`**, inside a bulk call: TAN's (0, 90) maps to exactly (0, 0) and its reverse of (0, 0) to exactly (0, 90), as `wcstrig.c`'s exact branches give. Pinned in Task 10 (kernel level) and Task 11 (WcsMap level).
3. **One BAD and one NaN coordinate in a bulk call**, on both `transform` and the NaN protocol: those points are BAD (or NaN) and their neighbours have their single-point bits. Pinned in Task 8.
4. **Huge and non-finite angles in the degree trigonometry** (±`f64::MAX`, ±∞, NaN, 1e300): `Libm`'s `cosd`/`sind` give C's bits; where C's `(int)floor(angle/90 ± 0.5)` overflows `int` (undefined in C), the port gives what C built by clang gives (repository rule: mirror the wrap when it is the right answer), recorded in a comment. Pinned in Task 2.
5. **A projection whose iteration hits its cap** (TPN's 50 Newton steps, AIR's and PCO's solvers): a point that is bad on one path is bad on the other; vectorization never turns a point between bad and good. Pinned in Tasks 12 and 15 (each a per-projection test over points chosen near the cap, found by recording iteration counts in the differential test).

---

### Task 1: Per-family bench cases and the WcsMap-stage baseline

**Files:**
- Modify: `tools/perf/gen_bench_fixtures.c` (seven constructors and table rows)
- Create: `crates/ast-bench/fixtures/wcsmap_{arc,cea,ait,cod,bon,qsc,hpx}.map`; Modify: `crates/ast-bench/fixtures/bench.oracle` (regenerated)
- Modify: `crates/ast-bench/sets/bench.txt`, `crates/ast-bench/sets/native.txt`
- Create: `docs/perf/<date>-macos-aarch64-native-wcsmap-baseline.tsv`; Modify: `docs/perf/README.md`

**Interfaces:**
- Produces: bench case names `wcsmap_arc` (zenithal), `wcsmap_cea` (cylindrical, `PV2_1=1`), `wcsmap_ait` (pseudo-cylindrical), `wcsmap_cod` (conic, `PV2_1=45, PV2_2=10`), `wcsmap_bon` (polyconic, `PV2_1=45`), `wcsmap_qsc` (quad-cube), `wcsmap_hpx` (HEALPix), each over inputs `-0.05 .. 0.05` like the existing three; and the commit this task makes, referred to below as WBASE.

- [ ] **Step 1: Add the seven constructors and rows** in the style of `wcsmap_zpn` (`gen_bench_fixtures.c:95`).

- [ ] **Step 2: Regenerate**

Run: `tools/perf/build_c_engine.sh bench && target/perf/c/bench/gen_bench_fixtures crates/ast-bench/fixtures`
Expected: `git diff --stat crates/ast-bench/fixtures` shows the seven new files and `bench.oracle` gaining seven sections; no existing section's values change. If any existing section changes, stop: the C engine does not match the pin.

- [ ] **Step 3: Add the seven lines to `bench.txt` and `native.txt`** (`# WcsMap <CODE>: wcslib, <family>`).

- [ ] **Step 4: Check every case on both engines**

Run: `cargo run --release -p ast-bench --features c-oracle -- run --set native --engine both`
Expected: exit 0, every case has rows on both engines.

- [ ] **Step 5: Snapshot on a quiet machine**

Run: `cargo run --release -p ast-bench --features c-oracle -- snapshot --set native`, rename the output to `docs/perf/<date>-macos-aarch64-native-wcsmap-baseline.tsv`, add a README row ("WcsMap stage baseline: the native set with one case per projection family, wcslib still C").

- [ ] **Step 6: Commit** — "Add one WcsMap bench case per projection family and record the WcsMap baseline". This commit is WBASE.

---

### Task 2: `wcslib` as a directory, and bit-identical degree trigonometry

**Files:**
- Create: `crates/starlink-ast/src/wcslib/mod.rs` (the `WcsProjection` enum and its tables, moved unchanged from `wcslib.rs`), `crates/starlink-ast/src/wcslib/ffi.rs` (the `AstPrjPrm`/`PrjParams` FFI layer, moved unchanged), `crates/starlink-ast/src/wcslib/trig.rs`
- Delete: `crates/starlink-ast/src/wcslib.rs`
- Create: `crates/starlink-ast/src/wcslib/difftest.rs` (`#[cfg(test)]`)
- Modify: `crates/starlink-ast/src/test_inputs.rs` (two generators)

**Interfaces:**
- Produces, in `trig.rs`:
  - `pub(crate) trait Trig { fn sind(a: f64) -> f64; fn cosd(a: f64) -> f64; fn tand(a: f64) -> f64; fn asind(v: f64) -> f64; fn acosd(v: f64) -> f64; fn atand(v: f64) -> f64; fn atan2d(y: f64, x: f64) -> f64; }`, every method `#[inline(always)]` in each impl.
  - `pub(crate) struct Libm;` implementing `Trig` as `wcstrig.c`'s `astCosd` … `astATan2d`, C's operations in C's order (`%` for `fmod`, `f64::floor`, `f64::cos` etc.), with `D2R`/`R2D`/`WCSTRIG_TOL` from `wcsmath.h`/`wcstrig.h` as `const`s.
  - In `difftest.rs`: the extern declarations of `astCosd`, `astSind`, `astTand`, `astACosd`, `astASind`, `astATand`, `astATan2d`.
  - In `crate::test_inputs` (so they outlive `difftest.rs`): `pub(crate) fn degrees(n: usize) -> Vec<f64>` (random in ±720, every multiple of 45 in ±720, each ±1 ULP, and `test_inputs::specials()`), `pub(crate) fn unit_range(n: usize) -> Vec<f64>` (random in [−1.1, 1.1], ±1, 0, ±(1 ± `WCSTRIG_TOL`/2), ±(1 ± 2·`WCSTRIG_TOL`), specials).
- `ffi.rs` stays until Task 9; `wcsmap.rs`'s imports change to `crate::wcslib::ffi::PrjParams`.

- [ ] **Step 1: Move the files** (`git mv` then split), fix imports. Run `cargo test -p starlink-ast --lib wcslib::`; expected PASS (no behaviour change).

- [ ] **Step 2: Write `libm_trig_matches_wcstrig_bit_for_bit`** in `trig.rs`: each `Libm` method against its C function over `degrees(200_000)` (or `unit_range(200_000)` for the inverses; every pair of `degrees` values thinned to 1000 × 1000 for `atan2d`), comparing `to_bits()`.

- [ ] **Step 3: Run it to see it fail** — `cargo test -p starlink-ast --lib wcslib::trig::`; expected: `Libm` not found.

- [ ] **Step 4: Implement `Trig` and `Libm`.** For `(int)floor(...)` use the conversion that reproduces C's bits on the specials; if `as i32` (saturating) differs from C for some input, match C and comment why (Review Focus 4).

- [ ] **Step 5: Run it** — expected PASS.

- [ ] **Step 6: Commit** — "Port wcslib's degree trigonometry to Rust, checked bit for bit against the vendored C".

---

### Task 3: The native `Prj` and the zenithal projections

**Files:**
- Create: `crates/starlink-ast/src/wcslib/prj.rs`, `crates/starlink-ast/src/wcslib/zenithal.rs`
- Modify: `crates/starlink-ast/src/wcslib/difftest.rs`

**Interfaces:**
- Consumes: `Trig`, `Libm` (Task 2).
- Produces, in `prj.rs`:
  - `#[repr(u8)] #[derive(Clone, Copy, PartialEq, Eq, Debug)] pub(crate) enum Status { Ok = 0, BadParams = 1, BadPoint = 2 }` — wcslib's return codes.
  - `#[derive(Clone, Debug, PartialEq)] pub(crate) struct PrjPrm { pub(crate) phi0: f64, pub(crate) theta0: f64, pub(crate) r0: f64, pub(crate) p: [f64; PRJ_PARAM_SLOTS], pub(crate) p2: [f64; PRJ_PARAM_SLOTS], pub(crate) w: [f64; 20], pub(crate) n: i32, pub(crate) bounds: bool }` — C's `AstPrjPrm` without `code`, `flag`, `fwd`, `rev`; `bounds` is C's `flag > 0` test.
  - `#[derive(Clone, Debug, PartialEq)] pub(crate) struct Prj { proj: WcsProjection, prm: PrjPrm, setup: Status }`.
  - `pub(crate) fn Prj::new(proj: WcsProjection, params: &[Vec<Option<f64>>], tpn_tan: bool) -> AstResult<Prj>`: `ffi::PrjParams::new` + `init_prj_prm`'s defaults, then the projection's set-up run eagerly, its failure stored in `setup` (not returned), so the error still surfaces where C raises it: at the first point that reaches the projection. `Err` only for NCP, GLS and Unknown, as `PrjParams::new` today.
  - `pub(crate) fn Prj::proj(&self) -> WcsProjection`, `pub(crate) fn Prj::get_param(&self, axis: usize, m: usize) -> Option<f64>` (as `PrjParams::get_param`).
  - `pub(crate) fn Prj::fwd_raw(&self, phi: f64, theta: f64) -> (Status, f64, f64)` and `rev_raw(&self, x: f64, y: f64) -> (Status, f64, f64)`: `setup` if it failed, else the projection's body with `Libm`. These are what the differential tests compare.
  - In each family file, per projection: `pub(super) fn <code>_set(prm: &mut PrjPrm) -> Status`, `pub(super) fn <code>_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64)`, `pub(super) fn <code>_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64)`, all `#[inline(always)]`; set-up functions use `Libm` directly.
  - `prj.rs` dispatches with one `match self.proj` per direction.
- Produces, in `difftest.rs`: `pub(crate) fn check_projection(proj: WcsProjection, pv_sets: &[Vec<(usize, usize, f64)>])` — for each parameter set (axis, m, value), builds both `ffi::PrjParams` (with `init_prj_prm`) and `Prj`, and reuses that one C struct for every call with forward and reverse calls interleaved, as WcsMap uses it (C's lazy set-up keeps state between calls), and compares status and output bits of `fwd_raw`/`forward_raw` over random (phi, theta) in [−180, 180] × [−90, 90] plus every multiple of 45 and the specials, and of `rev_raw`/`reverse_raw` over random (x, y) in [−400, 400]², the forward outputs of the first set (round trips) and the specials; outputs compared only where status is `Ok`; panics naming projection, parameter set and input. Also compares `setup` status with C's first-call status.

Zenithal family: AZP, SZP, TAN, STG, SIN, ARC, ZPN, ZEA, AIR.

- [ ] **Step 1: Write `zenithal_projections_match_vendored_c`**: `check_projection` for each of the nine with the default parameters, every parameter set used for that projection in `tests/test_wcsmap_edges.rs` and `tests/test_wcsmap_pv.rs`, and these unusable sets: SZP `PV2_1 = -2, PV2_3 = 30` (`w[3] == 0`), AIR `PV2_1 = -90` (`astAIRset` fails for `PV2_1 <= -90`; add `PV2_1 = 90`, its special case), ZPN all coefficients zero (`astZPNrev`'s `k < 1`).

- [ ] **Step 2: Run it to see it fail** — `cargo test -p starlink-ast --lib wcslib::`; expected: compile failure.

- [ ] **Step 3: Implement `Status`, `PrjPrm`, `Prj`, and the nine projections.** Port each from `proj.c`, early returns and all; the generic body may compute outputs unconditionally and return the status at the end, provided every `Ok` output keeps C's bits (later vectorization needs straight-line bodies; do this now where it is free).

- [ ] **Step 4: Run it** — expected PASS.

- [ ] **Step 5: Commit** — "Port the zenithal projections to Rust, checked bit for bit against the vendored C".

---

### Task 4: TPN

**Files:** Create: `crates/starlink-ast/src/wcslib/tpn.rs`; Modify: `prj.rs` (dispatch), `difftest.rs`

**Interfaces:**
- Produces: `tpn_set`, `tpn_fwd<T: Trig>`, `tpn_rev<T: Trig>` as in Task 3, ported from `vendor/wcslib/tpn.c`, with `prm.n` as the `TPNTan` switch.
- `tpn_fwd` keeps the Newton loop's tolerance (`1.0E-5`, relative) and cap (50) as named `const`s with a comment giving `tpn.c` as their source.

- [ ] **Step 1: Write `tpn_matches_vendored_c`**: `check_projection(Tpn, …)` with defaults, `dss.ast`'s TPN parameters (read them from `tests/fixtures/wcsconv/inputs/dss.ast` and write them as literals), random sets of up to 40 coefficients on each axis of magnitude ≤ 1e-3, and each with `tpn_tan` true and false. Record the iteration count per point in the C-side harness only if needed for Review Focus 5; otherwise leave that to Task 12.
- [ ] **Step 2: Run to fail; Step 3: implement; Step 4: run to pass.**
- [ ] **Step 5: Commit** — "Port TPN to Rust, checked bit for bit against the vendored C".

---

### Task 5: Cylindrical and pseudo-cylindrical projections

**Files:** Create: `crates/starlink-ast/src/wcslib/cylindrical.rs` (CYP, CEA, CAR, MER), `crates/starlink-ast/src/wcslib/pseudocyl.rs` (SFL, PAR, MOL, AIT); Modify: `prj.rs`, `difftest.rs`

**Interfaces:** as Task 3, one triple per projection. GLS stays SFL with parameters, as today.

- [ ] **Step 1: Write `cylindrical_projections_match_vendored_c` and `pseudocylindrical_projections_match_vendored_c`** with defaults, the sets from the two WcsMap test files, and CYP `PV2_2 = 0` (set-up failure), CEA `PV2_1 = 0` and `1.5` (set-up failures).
- [ ] **Steps 2–4: fail, implement, pass.**
- [ ] **Step 5: Commit** — "Port the cylindrical and pseudo-cylindrical projections to Rust, checked bit for bit".

---

### Task 6: Conic and polyconic projections

**Files:** Create: `crates/starlink-ast/src/wcslib/conic.rs` (COP, COE, COD, COO), `crates/starlink-ast/src/wcslib/polyconic.rs` (BON, PCO); Modify: `prj.rs`, `difftest.rs`

- [ ] **Step 1: Write the two tests** with the sets from the WcsMap test files plus `PV2_1` in {−60, −10, 10, 45, 89} × `PV2_2` in {0, 5, 20}, `cop_at_the_poles`'s set, COE with `PV2_1 = 0, PV2_2 = 30` (set-up failure, `astCOEset`'s `w[0] == 0`), and BON with `PV2_1 = 0`.
BON with `PV2_1 = 0` calls `astSFLfwd`/`astSFLrev` on BON's own struct, whose flag check re-runs SFL's set-up and overwrites `r0` and `w` (and flips the flag, so the next BON call sets BON up again): the native `Prj` must give the bits of that sequence, and the test must cover a BON(0) forward followed by a reverse and vice versa, against one C struct reused across the calls, as WcsMap uses it.
- [ ] **Steps 2–4: fail, implement, pass.**
- [ ] **Step 5: Commit** — "Port the conic and polyconic projections to Rust, checked bit for bit".

---

### Task 7: Quad-cube and HEALPix projections

**Files:** Create: `crates/starlink-ast/src/wcslib/quadcube.rs` (TSC, CSC, QSC), `crates/starlink-ast/src/wcslib/healpix.rs` (HPX, XPH); Modify: `prj.rs`, `difftest.rs`

- [ ] **Step 1: Write the two tests** with defaults, the sets from the WcsMap test files (HPX non-default H and K included), and forward inputs at every face centre, face edge and facet apex the edge tests name (`qsc_near_face_4_centre`, `hpx_polar_zone_and_facet_edges`, `hpx_pole_at_facet_apex`, `hpx_phi_plus_180`), each ±1 ULP.
- [ ] **Steps 2–4: fail, implement, pass.**
- [ ] **Step 5: Commit** — "Port the quad-cube and HEALPix projections to Rust, checked bit for bit".

After this task `prj.rs`'s `match` has an arm for every `WcsProjection` that `ffi::WcsProjection::prj_fns` lists.

---

### Task 8: WcsMap on the native `Prj`, through in-place slice entries

**Files:**
- Modify: `crates/starlink-ast/src/wcslib/prj.rs` (per-value and slice entries)
- Modify: `crates/starlink-ast/src/mappings/wcsmap.rs` (`prj` field, `rebuild_prj`, `long_range`, `transform_into`, `transform_into_nan`, `wcslib_result` removed)
- Test: `crates/starlink-ast/tests/test_wcsmap.rs`

**Interfaces:**
- Consumes: `Prj`, `Status` (Tasks 3–7).
- Produces, on `Prj`:
  - `pub(crate) fn fwd(&self, phi: f64, theta: f64) -> AstResult<Option<(f64, f64)>>` and `rev(&self, x: f64, y: f64) -> AstResult<Option<(f64, f64)>>`: `Ok(Some)` for `Status::Ok`, `Ok(None)` for `BadPoint`, `Err(AST__WCSPA, "WcsMap: the projection parameter values are unusable")` for `BadParams`; each counts `hit("wcs_fwd")` / `hit("wcs_rev")`.
  - `pub(crate) fn fwd_slice(&self, a: &mut [f64], b: &mut [f64]) -> AstResult<()>`: on entry `a[i], b[i]` are (phi, theta) in degrees, on return (x, y); an input pair containing `BAD_VALUE` or NaN, or a point whose status is `BadPoint`, gives `BAD_VALUE` in both; `Err` as `fwd` if any good input pair gets `BadParams`. Counts `hit_n("wcs_fwd", <good input pairs>)`.
  - `pub(crate) fn rev_slice(&self, a: &mut [f64], b: &mut [f64]) -> AstResult<()>`: the same for (x, y) → (phi, theta).
  - In this task both slices run the `Libm` body per element; Task 11 onwards gives them kernels.
- WcsMap: `prj: Prj` (no `RefCell`; `Prj: Clone`); `long_range` calls the per-value `fwd`/`rev`.
  Forward: `out.copy_from(&pts)`, then one loop writing `forward_prep`'s degrees into the output lon/lat axes (BAD in, BAD out), one `fwd_slice` call, one loop scaling by `DD2R / factor`.
  Reverse: one loop scaling into degrees, one `rev_slice`, one loop applying the tolerance and longitude checks exactly as the per-point code does now.
  `transform_into_nan` is the same with NaN for bad: convert `BAD_VALUE` outputs to NaN in its last loop.

- [ ] **Step 1: Write the WcsMap tests (public API) in `test_wcsmap.rs`**

```rust
#[test]
fn bulk_and_single_point_give_identical_bits() {
    // TAN, SIN, ZPN(PV2_1=1, PV2_3=0.05), TPN(dss.ast's PVs), HPX, COD(45,10):
    // forward and inverse at lengths 1, 2, 3, 7, 1001 of random points in
    // range; point i alone == point i in the batch, by bits.
}

#[test]
fn one_bad_and_one_nan_point_leave_neighbours_alone() {
    // 7-point TAN forward and inverse batch, point 2 lat = BAD_VALUE, point 5
    // lon = NaN: transform gives BAD_VALUE for both on both outputs;
    // transform_nan_protocol gives NaN; every other point equals its
    // single-point result.
}

#[test]
fn unusable_parameters_are_an_error_only_for_good_points() {
    // CYP with PV2_2 = 0 and ZPN with all coefficients zero (inverse):
    // transforming one good point is Err with ErrorCode::Wcspa; transforming
    // [[BAD_VALUE, BAD_VALUE]] is Ok and BAD.
}
```

- [ ] **Step 2: Run them on today's code** — `cargo test -p starlink-ast --test mappings test_wcsmap::`; expected PASS (they pin current behaviour).

- [ ] **Step 3: Implement the entries and reorganize WcsMap.**

- [ ] **Step 4: Run the gates**

```
cargo test -p starlink-ast --test mappings test_wcsmap test_wcsmap_edges test_wcsmap_pv
cargo test -p starlink-ast --lib wcslib::
cargo test -p starlink-ast --features call-counts --lib
cargo test -p starlink-ast
```
Expected: PASS.

- [ ] **Step 5: Commit, then check bits and time**

Commit "Run WcsMap on the native projections through slice entry points", then:
```
cargo run --release -p ast-bench -- bits --base HEAD~1
cargo run --release -p ast-bench -- compare --set native --base HEAD~1
```
Expected: every output bit matches; no slowdown flagged. If either fails, fix and amend.
Then run tier 4 by hand (`cargo test -p ast-oracle-tests`), which compares call counts with C's; expected PASS.

---

### Task 9: Delete the vendored wcslib, and record the bit-identical milestone

**Files:**
- Delete: `crates/starlink-ast/vendor/wcslib/`, `crates/starlink-ast/src/wcslib/ffi.rs`, `difftest.rs` and the tests that call it (the C halves of the differential tests, per spec step 2)
- Modify: `crates/starlink-ast/build.rs` (`compile_wcslib` and its call), `crates/starlink-ast/vendor/README.md` (the wcslib section becomes one line: ported to `src/wcslib/`, with the commit), `COPYRIGHT` (WCSLIB line names `crates/starlink-ast/src/wcslib` as derived code), `CLAUDE.md` (layout tree line), `PLAN.md` §7 step-6a row, `crates/starlink-ast/Cargo.toml` `include` list if it names the directory
- Create: `docs/perf/<date>-macos-aarch64-native-wcsmap-ported.tsv`; Modify: `docs/perf/README.md`

- [ ] **Step 1: Delete and edit.** Then `rg -n 'vendor/wcslib|ast_wcslib|astPRJ|astTANfwd' --glob '!reference/**' --glob '!docs/**'` lists nothing but `crates/audit-tools/src/rendered_docs.rs`'s test string.
- [ ] **Step 2: Run** `cargo build && cargo test -p starlink-ast && cargo clippy --workspace -- -D warnings && cargo doc -p starlink-ast --no-deps`; expected PASS, and `cargo package -p starlink-ast --list | rg wcslib` shows only `src/wcslib/*.rs`.
- [ ] **Step 3: Check bits** — commit "Delete the vendored wcslib", then `cargo run --release -p ast-bench -- bits --base HEAD~1`; expected all match.
- [ ] **Step 4: Snapshot** the native set (as Task 1 Step 5) into `…-native-wcsmap-ported.tsv`, README row stating TAN/SIN/ZPN and the six FrameSets against WBASE; commit "Record WcsMap after the bit-identical port".

---

### Task 10: Vectorizable degree trigonometry, `Kernel<FMA>`

**Files:** Modify: `crates/starlink-ast/src/wcslib/trig.rs`

**Interfaces:**
- Consumes: `vmath::sin_cos`, `sin_cos_covers`, `atan2::<FMA>`, `atan2_covers`.
- Produces:
  - `pub(crate) struct Kernel<const FMA: bool>;` implementing `Trig`: `sind`/`cosd` from `vmath::sin_cos(a * D2R)` (the same rounded argument C passes to libm), `atan2d` as `vmath::atan2::<FMA>(y, x) * R2D`, `atand(v)` as `vmath::atan2::<FMA>(v, 1.0) * R2D`; `tand`, `asind`, `acosd` delegate to `Libm` unless a kernel for them is added and kept (Task 13).
  - `pub(crate) fn sincosd_covers(a: f64) -> bool`: false when `a % 90.0 == 0.0` (wcstrig's exact branch) or `!sin_cos_covers(a * D2R)`.
  - `pub(crate) fn atan2d_covers(y: f64, x: f64) -> bool`: false when `y == 0.0 || x == 0.0` (wcstrig's exact branches) or `!atan2_covers(y, x)`.
  - `pub(crate) fn atand_covers(v: f64) -> bool`: false for v in {−1, 0, 1} or non-finite.

- [ ] **Step 1: Write the tests**

```rust
#[test]
fn kernel_trig_is_within_one_ulp_of_libm_where_it_covers() {
    // degrees(2_000_000) filtered by sincosd_covers: ulps(Kernel::<HAS_FMA>::sind(a),
    // Libm::sind(a)) <= 1, same for cosd; angle pairs filtered by atan2d_covers
    // for atan2d; unit_range filtered by atand_covers for atand. At least one
    // value differs for each (proves the kernel is in use).
}

#[test]
fn covers_rejects_wcstrig_exact_arguments() {
    for a in [0.0, -0.0, 90.0, -90.0, 180.0, 270.0, 360.0, -720.0] {
        assert!(!sincosd_covers(a));
    }
    assert!(!atan2d_covers(0.0, -1.0) && !atan2d_covers(1.0, 0.0));
    assert!(!atand_covers(1.0) && !atand_covers(-1.0) && !atand_covers(0.0));
    assert!(sincosd_covers(45.0) && atan2d_covers(1.0, 1.0));
}
```

`ulps` is `test_inputs::ulps`. Note that ×R2D adds its own rounding: if `atan2d` misses 1 ULP only through it, record the measured bound as the per-function bound for `atan2d` (2026-10-27 rule: the bound is per elementary function, the product is one more rounding) and say so in the test.

- [ ] **Steps 2–4: fail, implement, pass.**
- [ ] **Step 5: Commit** — "Add vectorizable degree trigonometry for the projections, within 1 ULP of libm".

---

### Task 11: Vectorize TAN

**Files:**
- Modify: `crates/starlink-ast/src/wcslib/prj.rs` (slice entries: kernel loop, fix-up loop, x86 dispatch), `zenithal.rs` (TAN's `needs_special`)
- Modify: whatever the drift inspection finds (Step 5), `PLAN.md` §6, `CHANGELOG.md`

**Interfaces:**
- Consumes: `Kernel<FMA>`, the covers predicates (Task 10).
- Produces:
  - Per vectorized projection: `pub(super) fn <code>_fwd_needs_special(prm: &PrjPrm, phi: f64, theta: f64) -> bool` and `<code>_rev_needs_special(prm: &PrjPrm, x: f64, y: f64) -> bool`: true when any trig argument the body would compute is outside its kernel's cover (for TAN forward: `!sincosd_covers(theta) || !sincosd_covers(phi)`; reverse: `r == 0.0 || !atan2d_covers(x, -y) || !atan2d_covers(r0, r)`).
  - `fwd_slice`/`rev_slice` for a vectorized projection: pass 1 runs `<code>_fwd::<Kernel<FMA>>` over every element; pass 2 overwrites BAD inputs with BAD, and runs `<code>_fwd::<Libm>` for each element where `needs_special`; pass 3 maps `BadPoint` to BAD and `BadParams` to the error. Non-vectorized projections keep Task 8's `Libm` loop.
  - x86-64: the pass-1 loop has an `#[target_feature(enable = "avx2,fma")]` copy chosen by `vmath::x86_has_avx2_fma()`, generic over `const FMA: bool`, exactly as `pal::dcc2s_slice`.

- [ ] **Step 1: Write the tests in `prj.rs`**

```rust
#[test]
fn tan_slices_are_within_kernel_drift_of_the_per_value_body() {
    // random (phi, theta) and (x, y) as difftest, 1_000_000 each: every
    // fwd_slice/rev_slice output within 2 ULP of fwd/rev (TAN forward is a
    // product of a sine and a ratio: one ULP each); bad/good identical.
    // At least one differs.
}

#[test]
fn tan_slices_give_each_point_the_same_bits_at_any_length() {
    // lengths 1, 2, 3, 7, 1001, specials and multiples of 90 interleaved.
}

#[test]
fn tan_exact_points_stay_exact_in_a_slice() {
    // fwd_slice of (0, 90) inside a 7-point slice gives exactly (0, 0);
    // rev_slice of (0, 0) gives exactly (0, 90).
}
```

The 2-ULP figure is this plan's starting point; replace it with the measured maximum, justified by the formula, if larger, before relaxing anything downstream.

- [ ] **Steps 2–4: fail, implement, pass**, plus `cargo test -p starlink-ast --test mappings test_wcsmap`.

- [ ] **Step 5: Find and inspect every moved result**

Commit, then:
```
cargo run --release -p ast-bench -- ulps --base HEAD~1
cargo test -p starlink-ast
cargo test -p ast-oracle-tests   # tier 4, by hand
```
For each case `ulps` lists and each failing test, confirm the chain contains a TAN WcsMap and that the drift is the kernel's, grown through later stages.
A value changing between BAD and good, or a failure with any other cause, is a bug: stop and fix it.
For a justified failure, add the case to a `WCSMAP_DRIFT_LICENSED` list in that test file with a `MEASURED_BOUND`, following `SPHMAP_DRIFT_LICENSED` (a licensed case that then matches exactly fails, so no list goes stale); count it in `PLAN.md` §6.
`test_wcsmap_edges.rs` cases: a zero-tolerance case that moves gets its own named tolerance set from the measured drift, with a comment.

- [ ] **Step 6: Time it** — `cargo run --release -p ast-bench -- compare --set native --base HEAD~1`; expect `wcsmap_tan` and the five TAN FrameSets faster at large N and N=1 no slower than C. If `wcsmap_tan` at 1 000 000 points is not faster than at HEAD~1, check the pass-1 loop vectorized (`cargo rustc -p starlink-ast --release -- --emit asm`, look for vector `fdiv` in `fwd_slice`) before going on. If it cannot be made faster, revert to the `Libm` loop for TAN and record why in `prj.rs`'s module doc (keep-or-reject rule).

- [ ] **Step 7: CHANGELOG and commit** — `Unreleased` / `Changed`: "WcsMap's TAN projection uses vectorizable trigonometry in bulk transforms; results may differ from earlier releases by a few ULP." Commit "Vectorize WcsMap's TAN projection".

---

### Task 12: Vectorize TPN

As Task 11, for TPN, with these differences:

- Pass 1 runs only TPN's trigonometric front end (`xi`, `eta` for the forward; the final `atan2d` pair for the reverse) with `Kernel<FMA>`; the Newton iteration runs per point on the result, in the same generic body (split the body at that stage boundary into two `#[inline(always)]` functions; no arithmetic is written twice).
- The drift test bounds a forward output by the solver's own tolerance: `|Δx| ≤ TPN_TOL² · |x|` per spec ("Iterative projections"), with a comment deriving the bound from `TPN_TOL` (quadratic convergence: the step after one within `TPN_TOL` relative is within `TPN_TOL²`); reverse outputs (closed form) by the measured ULP.
- Add `tpn_cap_points_agree_on_bad_or_good`: points whose per-value forward takes 49, 50 or 51 iterations (found by searching random inputs with dss.ast's parameters for high iteration counts; record them as literals) are bad in `fwd_slice` exactly when bad in `fwd` (Review Focus 5).
- Downstream licences for `dss.ast` and anything else through TPN use the tolerance-derived bound, named as such.
- Timing headline: `dss.ast`.
- CHANGELOG line and commit "Vectorize WcsMap's TPN projection's trigonometry".

---

### Task 13: Vectorize SIN and ZPN

As Task 11, once per projection, two commits.

- SIN's reverse calls `asind` and `acosd`. Try `asind(v) = atan2d(v, sqrt((1 - v)(1 + v)))` and `acosd(v) = atan2d(sqrt((1 - v)(1 + v)), v)` as `Kernel` methods with covers predicates excluding wcstrig's exact branches and |v| ≥ 1; keep them only if within 1 ULP of `Libm` over `unit_range(2_000_000)` and SIN's reverse is faster; otherwise SIN's reverse uses `Libm` for those two and the module doc records the measured ULP and timing.
- ZPN's reverse has a polynomial root search per point: vectorize only its final `atan2d` (and the forward's `sind`/`cosd`), as TPN's split.
- Timing headlines: `wcsmap_sin`, `wcsmap_zpn`.

---

### Task 14: Phase 1 milestone record and x86 hand-off

**Files:** Create: `docs/perf/<date>-macos-aarch64-native-wcsmap-phase1.tsv`, `docs/perf/<date>-macos-aarch64-native-wcsmap-phase1-roman.txt`; Modify: `docs/perf/README.md`, `PLAN.md` §7 (step-6a row: WcsMap phase 1)

- [ ] **Step 1: Snapshot and roman** as SphMap Task 9 Step 1.
- [ ] **Step 2: README rows**, each giving TAN, TPN, SIN, ZPN and the six FrameSets at 1 000 000 points against WBASE, Task 9's record, and C.
- [ ] **Step 3: Commit** — "Record WcsMap after vectorizing TAN, TPN, SIN and ZPN".
- [ ] **Step 4: Give the user the x86-64 commands**, with WBASE and HEAD as shas:

```
git checkout tickets/DM-56839-wcsmap && git submodule update --init reference/starlink-ast
tools/perf/build_c_engine.sh bench
cargo run --release -p ast-bench -- compare --set native --base <WBASE sha>
cargo run --release -p ast-bench --features c-oracle -- snapshot --set native
```

and ask for the output; it becomes `docs/perf/<date>-linux-x86_64-native-wcsmap-phase1.tsv` with a README row.

---

### Task 15: Phase 2 — vectorize each remaining family

Repeat Task 11 once per row, one commit per family ("Vectorize WcsMap's <family> projections"), each with its own drift inspection, licences and CHANGELOG line:

| Family | Projections | Timed by | Notes |
|---|---|---|---|
| zenithal | AZP, SZP, STG, ARC, ZEA, AIR | `wcsmap_arc` | AIR's reverse iterates: bound as TPN, cap test as Task 12 |
| cylindrical | CYP, CEA, CAR, MER | `wcsmap_cea` | CAR has no trigonometry: leave it on `Libm` |
| pseudo-cylindrical | SFL, PAR, MOL, AIT | `wcsmap_ait` | MOL's forward iterates: as AIR |
| conic | COP, COE, COD, COO | `wcsmap_cod` | |
| polyconic | BON, PCO | `wcsmap_bon` | PCO's reverse iterates: as AIR |
| quad-cube | TSC, CSC, QSC | `wcsmap_qsc` | face selection is branchy: vectorize only the trig front end |
| HEALPix | HPX, XPH | `wcsmap_hpx` | |

The keep-or-reject rule applies per family from its bench case: a family whose case is not faster at 1 000 000 points stays on `Libm`, recorded in `prj.rs`'s module doc with its timing.
Projections in a kept family that share no kernel (CAR) are listed there too.

---

### Task 16: Stage close: records, tracker, hand-off

**Files:** Create: `docs/perf/<date>-macos-aarch64-native-wcsmap.tsv`, `…-native-wcsmap-roman.txt`; Modify: `docs/perf/README.md`, `PLAN.md` §6 (licence counts) and §7 (step-6a row: WcsMap done, vendor/wcslib deleted), the spec's "WcsMap stage" section (one line: done, with the plan's path)

- [ ] **Step 1: Snapshot, roman, README rows** against WBASE, Task 9 and Task 14.
- [ ] **Step 2: Full gates** — `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `cargo doc -p starlink-ast --no-deps`, tier 3 and tier 4 by hand; expected PASS.
- [ ] **Step 3: Commit** — "Record WcsMap after the native wcslib port".
- [ ] **Step 4: x86-64 hand-off** as Task 14 Step 4, file name `…-native-wcsmap.tsv`.

# Native ERFA for SphMap: baseline, tooling, port and vectorization — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record the performance baseline for every transform that reaches the vendored C, build the ULP and vForce tooling the rest of the ticket needs, and take SphMap off the vendored C: first a bit-identical Rust port of `eraC2s`/`eraS2c` with slice entry points, then a vectorized `atan2` within 1 ULP.

**Architecture:** A new private `erfa` module holds Rust ports of ERFA functions, one file per C function, each with its lineage and ERFA's licence notice.
A new private `vmath` module holds branchless math kernels the compiler vectorizes.
`pal::dcc2s`/`dcs2c` stop calling C and gain slice entries; SphMap's `transform_into` and `transform_into_nan` call the slice entries once per axis set and fix up BAD and pole points afterwards.

**Tech Stack:** Rust 2024 (`starlink-ast`, `ast-bench`), the vendored C still linked for differential tests, Apple Accelerate (vForce) in `ast-bench` only, on macOS only.

**Spec:** `docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md`.
This is the first of several plans for that spec; it covers the shared groundwork and SphMap.
Later plans: step 0 (the vendored-library update in C AST), WcsMap, then the remaining PAL and ERFA callers.

## Global Constraints

- The `erfa` and `vmath` modules are `pub(crate)`; no item is `pub` beyond `pub(crate)` or reachable from outside `starlink-ast`.
- Every new source file opens with `// Copyright 2026 Association of Universities for Research in Astronomy (AURA)` and `// SPDX-License-Identifier: LGPL-3.0-or-later` (enforced by `test_source_headers`).
- Every file with ERFA-derived code carries, after those two lines, ERFA's copyright and BSD-3-Clause licence text, copied verbatim from the end of the C file it ports.
- Every file with fdlibm/musl-derived code carries Sun's fdlibm notice verbatim ("Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved. … provided that this notice is preserved.") and names the musl files it came through.
- Every ported function has a `//` lineage comment: library, C file, C function, and version or commit (`reference/starlink-ast` at the pinned commit; note when current ERFA v2.0.1 is unchanged in substance).
- No C file or line citations in `///` docs; provenance stays in `//` comments.
- One body per function: the per-value entry and the slice entry give identical bits for every input, whatever the slice length.
- Vectorized results stay within 1 ULP of the libm-based result of the same C operations.
- No `unsafe` outside FFI declarations and calls; no platform intrinsics.
- Call counts keep C's symbol names (`astPalDcc2s`, `astPalDcs2c`) and count once per point.
- `cargo fmt`, `cargo clippy --workspace -- -D warnings`, and `#![warn(missing_docs)]` stay clean.
- Run cargo with conda's `CC`, `CXX`, `CFLAGS`, `CPPFLAGS`, `LDFLAGS` unset (`env -u CC -u CXX -u CFLAGS -u CPPFLAGS -u LDFLAGS cargo …`); the shell is zsh.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; nothing is pushed.

## Review Focus

1. **A point on the −x meridian whose y is −0.0**, in a bulk call: longitude must be +π, as the scalar `pal::dcc2s` gives today (its `+ 0.0` canonicalization). Pinned in Task 5.
2. **One BAD coordinate in one point of a bulk call**: that point's outputs are all `BAD_VALUE`, its neighbours are unaffected. Pinned in Task 5.
3. **Poles and near-poles in a bulk call** (`|x|, |y| < |1000 z|·ε`): longitude is `polar_long`, latitude ±π/2, exactly as the per-point code gives. Pinned in Task 5.
4. **The same point alone and inside a long batch, at lengths that are not a multiple of the vector width** (1, 2, 3, 7, 1001): identical bits. Pinned in Tasks 5 and 7.
5. **NaN inputs through the NaN protocol, and the zero vector**: NaN in gives NaN out (not `BAD_VALUE`) on `transform_into_nan`; (0, 0, 0) gives (0, 0) as `eraC2s` does. Pinned in Tasks 5 and 7.

---

### Task 1: The `native` bench set and the baseline record

**Files:**
- Create: `crates/ast-bench/sets/native.txt`
- Create: `docs/perf/2026-10-27-macos-aarch64-native.tsv` (written by `snapshot`)
- Create: `docs/perf/2026-10-27-macos-aarch64-native-roman.txt`
- Modify: `docs/perf/README.md` (two table rows)
- Modify: `crates/ast-bench/README.md` ("Sets" section: one sentence on `native`)

**Interfaces:**
- Produces: the set name `native`, and the commit this task creates, referred to below as BASELINE.

- [ ] **Step 1: Write the set file**

```
# The native set: every bench transform that reaches the vendored ERFA, PAL
# or wcslib C, and the real-world FrameSets that are the headline measure of
# docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md.
# Each line: <oracle> <relpath> # reason.
bench sphmap.map # SphMap: eraC2s, eraS2c
bench wcsmap_sin.map # WcsMap SIN: wcslib
bench wcsmap_tan.map # WcsMap TAN: wcslib
bench wcsmap_zpn.map # WcsMap ZPN: wcslib
bench slamap_eqgal.map # SlaMap: PAL galactic conversion
bench specmap_wn_to_fr.map # SpecMap
bench timemap_taiutc.map # TimeMap: palDat
framesets wcsconv/inputs/lsst.ast # Rubin LSST FrameSet
framesets wcsconv/inputs/lsst2.ast # Rubin LSST FrameSet with a SplineMap distortion
framesets wcsconv/inputs/a20070718_00010_02_cube.ast # JCMT ACSIS spectral cube
framesets wcsconv/inputs/dss.ast # DSS plate solution
framesets wcsconv/inputs/timj.ast # real-world celestial FrameSet
framesets wcsconv/inputs/degen1.ast # degenerate-axis FrameSet
```

- [ ] **Step 2: Check every case loads and times on both engines**

Run: `tools/perf/build_c_engine.sh bench && cargo run --release -p ast-bench --features c-oracle -- run --set native --engine both`
Expected: a table with every case, exit status 0. A case with no row good on both engines is an error; if `degen1.ast` is one, drop its line and say why in the set file's header.

- [ ] **Step 3: Record the baseline**

Run: `cargo run --release -p ast-bench --features c-oracle -- snapshot --set native` on a quiet machine, then `cargo run --release -p ast-bench --features c-oracle -- roman > docs/perf/2026-10-27-macos-aarch64-native-roman.txt`.
Expected: `docs/perf/<date>-macos-aarch64-native.tsv` written; rename to the date above if the UTC date differs.

- [ ] **Step 4: Add the README rows**

Two rows in `docs/perf/README.md`'s table: the `native` record ("The baseline for the native ERFA/PAL/wcslib ticket: every bench transform reaching the vendored C, and the wcsconv/inputs FrameSets, before any of that C is replaced") and the roman record.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-bench/sets/native.txt crates/ast-bench/README.md docs/perf/
git commit -m "Record the baseline for transforms that reach the vendored C"
```

This commit is BASELINE; Task 9 looks its sha up with `git log -1 --format=%H -- crates/ast-bench/sets/native.txt`.

---

### Task 2: `ast-bench ulps`, a ULP comparison against a base commit

**Files:**
- Create: `crates/ast-bench/src/ulps.rs`
- Modify: `crates/ast-bench/src/lib.rs` (`pub mod ulps;`)
- Modify: `crates/ast-bench/src/bits.rs` (factor out the keyed case list)
- Modify: `crates/ast-bench/src/base.rs` (generalize `bits_at`)
- Modify: `crates/ast-bench/src/main.rs` (`values` and `ulps` subcommands, USAGE)
- Modify: `crates/ast-bench/README.md` (a "Measuring how far results moved" section)

**Interfaces:**
- Consumes: `roman::ulps(a: f64, b: f64) -> u64` (existing).
- Produces:
  - `pub fn keyed_cases() -> Result<Vec<(String, Case)>, String>` in `bits.rs`, the key scheme `all_bits` already uses; `all_bits` is rewritten on top of it with unchanged output.
  - `pub fn output_at(git_ref: &str, args: &[&str]) -> Result<String, String>` in `base.rs`; `bits_at(r)` becomes `output_at(r, &["bits"])`.
  - In `ulps.rs`: `pub fn all_values() -> Result<String, String>`, one line per case: `key\tncoord\t` then every output value of every oracle row through `transform`, point by point, as 16-hex-digit bit patterns separated by spaces; or `key\t<failure text>` when the case cannot load or transform.
  - `pub struct UlpDiff { pub key: String, pub axis: usize, pub differ: usize, pub values: usize, pub max_ulps: u64 }`.
  - `pub fn diff_ulps(base: &str, cur: &str) -> Result<Vec<UlpDiff>, String>`; a key present on one side only, or a failure text differing, is an `Err` naming the key.
  - CLI: `ast-bench values` prints `all_values()`; `ast-bench ulps --base <git-ref> [--max N]` prints one line per differing (case, axis) sorted by `max_ulps` descending, prints `u64::MAX` as `sign/NaN/BAD change`, and with `--max N` fails for each line above N.

- [ ] **Step 1: Write the failing tests in `ulps.rs`**

```rust
#[test]
fn identical_text_has_no_diffs() {
    let t = "a\t3ff0000000000000 4000000000000000\n";
    assert!(diff_ulps(t, t).unwrap().is_empty());
}

#[test]
fn one_ulp_on_axis_two_is_reported() {
    // Two outputs per point, two points: axis is (value index % ncoord) + 1,
    // so the line carries ncoord; format: key\tncoord\tvalues.
    let base = "a\t2\t3ff0000000000000 4000000000000000 3ff0000000000000 4000000000000000\n";
    let cur = "a\t2\t3ff0000000000000 4000000000000001 3ff0000000000000 4000000000000000\n";
    let d = diff_ulps(base, cur).unwrap();
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].axis, d[0].differ, d[0].values, d[0].max_ulps), (2, 1, 2, 1));
}

#[test]
fn a_key_on_one_side_only_is_an_error() {
    assert!(diff_ulps("a\t1\t3ff0000000000000\n", "b\t1\t3ff0000000000000\n").is_err());
}

#[test]
fn a_changed_failure_text_is_an_error() {
    assert!(diff_ulps("a\terror AST__BADIN\n", "a\t1\t3ff0000000000000\n").is_err());
}
```

The line format is `key\tncoord\tvalues…` for a transformed case and `key\t<failure text>` otherwise; the tests fix it.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ast-bench ulps::`
Expected: compile failure, `diff_ulps` not found.

- [ ] **Step 3: Implement `keyed_cases`, `output_at`, `all_values`, `diff_ulps`, and the two subcommands**

`all_values` uses the oracle rows once each (no repetition: `Section::npoint()` rows), through `map.transform(&pts, s.forward)`, so the output stays small (about 24 rows per case).

- [ ] **Step 4: Run the tests** — `cargo test -p ast-bench ulps::`; expected PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-bench
git commit -m "Add ast-bench ulps, comparing every corpus output with a base commit in ULP"
```

- [ ] **Step 6: Compare the tree with itself**

Run: `cargo run --release -p ast-bench -- ulps --base HEAD`
Expected: no lines printed, exit 0. (A base older than this commit has no `values` subcommand and cannot be used.)

---

### Task 3: `ast-bench vforce`, Apple's vector math as a yardstick (macOS only)

**Files:**
- Create: `crates/ast-bench/src/vforce.rs` (`#![cfg(target_os = "macos")]`)
- Modify: `crates/ast-bench/src/lib.rs`, `crates/ast-bench/src/main.rs` (subcommand, USAGE), `crates/ast-bench/README.md`

**Interfaces:**
- Consumes: `roman::inputs(Inputs::Cartesian | Inputs::Spherical, n) -> Vec<Vec<f64>>`, `roman::time_batches`, `roman::median`, `roman::compare_bits`.
- Produces: `ast-bench vforce [--max-n N] [--reps R]`, printing per N, for SphMap forward and inverse: the port's ns/point through the public `SphMap` transform, the same math through vForce (`vvatan2` twice and `vvsqrt` forward; `vvsincos` twice inverse, computing the products as `eraS2c` does) in ns/point, and the vForce result's differing values and max ULP against the port.
  Declarations: `#[link(name = "Accelerate", kind = "framework")] unsafe extern "C" { fn vvatan2(z: *mut f64, y: *const f64, x: *const f64, n: *const i32); fn vvsqrt(y: *mut f64, x: *const f64, n: *const i32); fn vvsincos(s: *mut f64, c: *mut f64, x: *const f64, n: *const i32); }`.
  This is measurement only; nothing in `starlink-ast` links Accelerate.

- [ ] **Step 1: Write a test that the vForce forward matches the port within a few ULP on 1000 Roman points**

```rust
#[test]
fn vforce_forward_is_close_to_the_port() {
    let r = sphmap_forward_check(1000);
    assert!(r.max_ulps <= 4, "{r:?}");
}
```

`sphmap_forward_check(n) -> roman::BitsCheck` runs both paths on `roman::inputs(Inputs::Cartesian, n)`.

- [ ] **Step 2: Run it to see it fail** — `cargo test -p ast-bench vforce::`; expected: `sphmap_forward_check` not found.

- [ ] **Step 3: Implement `vforce.rs` and the subcommand.**

- [ ] **Step 4: Run the test and the subcommand**

Run: `cargo test -p ast-bench vforce:: && cargo run --release -p ast-bench -- vforce --max-n 1048576 > docs/perf/2026-10-27-macos-aarch64-vforce.txt`
Expected: test passes; the file shows vForce well below the port's ns/point at large N (the ceiling later tasks aim at).

- [ ] **Step 5: Add a `docs/perf/README.md` row and commit**

```bash
git add crates/ast-bench docs/perf
git commit -m "Add ast-bench vforce, timing Apple's vector math as a yardstick for SphMap"
```

---

### Task 4: The `erfa` module with bit-identical `c2s` and `s2c`

**Files:**
- Create: `crates/starlink-ast/src/erfa/mod.rs`, `crates/starlink-ast/src/erfa/c2s.rs`, `crates/starlink-ast/src/erfa/s2c.rs`
- Create: `crates/starlink-ast/src/test_inputs.rs` (`#[cfg(test)]`)
- Modify: `crates/starlink-ast/src/lib.rs` (`pub(crate) mod erfa;`, `#[cfg(test)] pub(crate) mod test_inputs;`)

**Interfaces:**
- Produces:
  - `pub(crate) fn erfa::c2s(p: [f64; 3]) -> (f64, f64)`, returning (theta, phi).
  - `pub(crate) fn erfa::s2c(theta: f64, phi: f64) -> [f64; 3]`.
  - Both `#[inline(always)]`, C's operations in C's order, `f64::atan2`, `f64::sqrt`, `f64::sin`, `f64::cos`.
  - In `test_inputs`: `pub(crate) struct SplitMix64` (`new(seed: u64)`, `next_f64(&mut self) -> f64` in [0, 1)); `pub(crate) fn specials() -> Vec<f64>` (±0.0, ±1.0, ±f64::MIN_POSITIVE, ±subnormal, ±f64::MAX, `BAD_VALUE`, ±∞, NaN, ±π, ±π/2); `pub(crate) fn unit_vectors(n: usize) -> Vec<[f64; 3]>` (random directions plus the six axis directions, signed zeros included); `pub(crate) fn angle_pairs(n: usize) -> Vec<(f64, f64)>` (random lon/lat, plus pairs whose magnitudes span 2^−40 to 2^40 with all sign combinations); `pub(crate) fn ulps(a: f64, b: f64) -> u64` (as `ast_bench::roman::ulps`).
  - `erfa/mod.rs`'s module doc states the lineage and licence rule and the one-body rule from the spec.

- [ ] **Step 1: Write the differential tests in each file's `#[cfg(test)] mod tests`**

```rust
unsafe extern "C" {
    fn astEraC2s(p: *const f64, theta: *mut f64, phi: *mut f64);
}

#[test]
fn c2s_matches_vendored_c_bit_for_bit() {
    let mut inputs = unit_vectors(100_000);
    // every triple drawn from specials(), for the special-value paths
    let s = specials();
    for &x in &s { for &y in &s { for &z in &s { inputs.push([x, y, z]); } } }
    for p in inputs {
        let (mut t, mut f) = (0.0, 0.0);
        // SAFETY: p is a 3-element array; t and f are valid outputs.
        unsafe { astEraC2s(p.as_ptr(), &mut t, &mut f) };
        let (rt, rf) = c2s(p);
        assert_eq!((rt.to_bits(), rf.to_bits()), (t.to_bits(), f.to_bits()), "{p:?}");
    }
}
```

`s2c_matches_vendored_c_bit_for_bit` is the same over `angle_pairs(100_000)` plus every pair from `specials()`, against `astEraS2c(theta, phi, c: *mut f64)`. Compare NaN results by bits too.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p starlink-ast --lib erfa::`
Expected: compile failure, `c2s` / `s2c` not found.

- [ ] **Step 3: Implement `c2s`, `s2c`, `test_inputs`, the module doc, and each file's header**

Header of each `erfa/*.rs`: the two required lines; then ERFA's copyright and full BSD-3-Clause licence text from the end of `vendor/erfa/c2s.c` (or `s2c.c`) verbatim; then `// Ported from ERFA eraC2s (erfa/c2s.c) as vendored at reference/starlink-ast 7c12422f; unchanged in substance in ERFA v2.0.1.`

- [ ] **Step 4: Run them to see them pass**

Run: `cargo test -p starlink-ast --lib erfa::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/erfa crates/starlink-ast/src/test_inputs.rs crates/starlink-ast/src/lib.rs
git commit -m "Port eraC2s and eraS2c to Rust, checked bit for bit against the vendored C"
```

---

### Task 5: `pal::dcc2s`/`dcs2c` on the Rust ERFA, with slice entries, and SphMap on them

**Files:**
- Modify: `crates/starlink-ast/src/pal.rs` (drop the `astPalDcs2c`/`astPalDcc2s` extern declarations; rewrite `dcs2c`, `dcc2s`; add two slice functions)
- Modify: `crates/starlink-ast/src/call_counts.rs` (add `pub(crate) fn hit_n(name: &'static str, n: u64)`, no-op without the feature)
- Modify: `crates/starlink-ast/src/mappings/sphmap.rs` (`transform_into`, `transform_into_nan`)
- Test: `crates/starlink-ast/tests/test_sphmap.rs`

**Interfaces:**
- Consumes: `erfa::c2s`, `erfa::s2c` (Task 4).
- Produces:
  - `pub(crate) fn pal::dcc2s(v: &[f64; 3]) -> (f64, f64)` and `pub(crate) fn pal::dcs2c(a: f64, b: f64) -> [f64; 3]`, same signatures as today, now Rust; each calls `call_counts::hit("astPalDcc2s")` / `hit("astPalDcs2c")`.
  - `pub(crate) fn pal::dcc2s_slice(x: &[f64], y: &[f64], z: &[f64], a: &mut [f64], b: &mut [f64])`: for every i, exactly `dcc2s(&[x[i], y[i], z[i]])`, including the `+ 0.0` canonicalization; counts `hit_n("astPalDcc2s", n)`.
  - `pub(crate) fn pal::dcs2c_slice(a: &[f64], b: &[f64], x: &mut [f64], y: &mut [f64], z: &mut [f64])`: for every i, exactly `dcs2c(a[i], b[i])`; counts `hit_n("astPalDcs2c", n)`.
  - Both slice functions `assert_eq!` all lengths equal.

- [ ] **Step 1: Write the failing SphMap tests (public API)**

In `crates/starlink-ast/tests/test_sphmap.rs`:

```rust
#[test]
fn bulk_and_single_point_give_identical_bits() {
    // Forward and inverse, at lengths 1, 2, 3, 7 and 1001: transforming
    // point i alone gives the same bits as point i inside the batch.
}

#[test]
fn minus_x_meridian_with_negative_zero_y_gives_plus_pi() {
    // forward of (-1.0, -0.0, 0.0) inside a 7-point batch: lon == PI exactly
}

#[test]
fn one_bad_coordinate_makes_only_that_point_bad() {
    // 7-point forward batch, point 3 has y = BAD_VALUE: point 3 outputs are
    // BAD_VALUE, every other point equals its single-point result.
}

#[test]
fn poles_in_a_batch_use_polar_long() {
    // PolarLong set to 0.5; points (0,0,1), (0,0,-1), (1e-20,1e-20,1) in a
    // 5-point batch: lon == 0.5, lat == +-FRAC_PI_2 exactly.
}

#[test]
fn nan_protocol_passes_nan_and_zero_vector_gives_zeros() {
    // transform_nan_protocol: (NaN, 0, 1) -> (NaN, NaN); transform:
    // (0, 0, 0) -> (0.0, 0.0).
}
```

The first test is the one that fails before the change only if the reorganization is wrong; it guards Step 3.
Write the other four from the comments with exact assertions.

- [ ] **Step 2: Run them**

Run: `cargo test -p starlink-ast --test mappings test_sphmap::`
Expected: all five pass on today's code (they pin current behaviour; the reorganization must keep them passing).

- [ ] **Step 3: Rewrite `pal::dcc2s`/`dcs2c` on `erfa`, add the slice functions and `hit_n`, and reorganize SphMap**

SphMap forward: one `pal::dcc2s_slice` call over the three input axes into the two output axes, then one loop that overwrites BAD points (`transform_into` only) and pole points with the values today's per-point code gives.
Inverse: one `pal::dcs2c_slice` call, then one loop overwriting BAD points (`transform_into` only).
Computing `dcc2s` on a BAD input is harmless (no traps; the result is overwritten).

- [ ] **Step 4: Run the gates**

Run:
```
cargo test -p starlink-ast --test mappings test_sphmap::
cargo test -p starlink-ast --lib pal:: erfa::
cargo test -p starlink-ast --features call-counts --lib call_counts::
```
Expected: PASS.

- [ ] **Step 5: Commit, then check bits against the parent**

```bash
git add crates/starlink-ast/src crates/starlink-ast/tests/test_sphmap.rs
git commit -m "Run SphMap on the Rust eraC2s and eraS2c through slice entry points"
cargo run --release -p ast-bench -- bits --base HEAD~1
cargo run --release -p ast-bench -- compare --set native --base HEAD~1
```
Expected: "every transform's output bits match HEAD~1"; `compare` reports no slowdown.
If either fails, fix and amend before moving on.

---

### Task 6: `vmath::atan2`, a branchless `atan2` within 1 ULP

**Files:**
- Create: `crates/starlink-ast/src/vmath/mod.rs`, `crates/starlink-ast/src/vmath/atan2.rs`
- Modify: `crates/starlink-ast/src/lib.rs` (`pub(crate) mod vmath;`)

**Interfaces:**
- Produces:
  - `pub(crate) fn vmath::atan2(y: f64, x: f64) -> f64`, `#[inline(always)]`, branchless (selects only), valid when `y` and `x` are finite and non-zero; any other input gives an unspecified value.
  - `pub(crate) fn vmath::atan2_covers(y: f64, x: f64) -> bool`: true exactly when `atan2` is valid for the pair.
  - `vmath/mod.rs`'s doc: what a kernel is (branchless, valid on a stated domain, paired with a `*_covers` predicate), the 1 ULP rule, and that kernels do not call libm.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn atan2_is_within_one_ulp_of_libm() {
    // angle_pairs(2_000_000) filtered by atan2_covers, plus pairs straddling
    // each reduction boundary (|y/x| at 7/16, 11/16, 19/16, 39/16, +-1 ULP),
    // in all four quadrants.
    // assert ulps(atan2(y, x), y.atan2(x)) <= 1 for every pair
}

#[test]
fn atan2_covers_rejects_zeros_non_finite_and_nan() {
    for &(y, x) in &[(0.0, 1.0), (-0.0, 1.0), (1.0, 0.0), (1.0, f64::INFINITY),
                    (f64::NAN, 1.0), (1.0, f64::NEG_INFINITY)] {
        assert!(!atan2_covers(y, x));
    }
    assert!(atan2_covers(1e-300, -1e300));
}
```

- [ ] **Step 2: Run them to see them fail** — `cargo test -p starlink-ast --lib vmath::`; expected: not found.

- [ ] **Step 3: Implement `atan2` and `atan2_covers`**

The algorithm is the appendix of `docs/issues/vectorized-erfa-pal-wcslib.md` (fdlibm's reduction and polynomial, each branch a select); port it from there and check its constants against musl's `src/math/atan.c` and `atan2.c`.
The file carries Sun's fdlibm notice and `// Algorithm and constants from fdlibm's s_atan.c and e_atan2.c via musl src/math/atan.c and atan2.c; branches replaced by selects.`

- [ ] **Step 4: Run the tests** — expected PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/starlink-ast/src/vmath crates/starlink-ast/src/lib.rs
git commit -m "Add a branchless atan2 within 1 ULP of libm"
```

---

### Task 7: Vectorized `c2s` and SphMap forward

**Files:**
- Modify: `crates/starlink-ast/src/erfa/c2s.rs`
- Modify: `crates/starlink-ast/src/pal.rs` (`dcc2s_slice`)
- Modify: tests or fixtures that the 1 ULP change moves (found in Step 5), `PLAN.md` §6 if any relaxation is added
- Modify: `CHANGELOG.md` (`Unreleased` → `Changed`)

**Interfaces:**
- Consumes: `vmath::atan2`, `vmath::atan2_covers` (Task 6).
- Produces (in `erfa/c2s.rs`):
  - `pub(crate) fn c2s_kernel(p: [f64; 3]) -> (f64, f64)`: `theta = vmath::atan2(y, x)`, `phi = vmath::atan2(z, sqrt(x*x + y*y))`, no branches.
  - `pub(crate) fn c2s_needs_special(p: [f64; 3]) -> bool`: true unless `atan2_covers(y, x)` and `atan2_covers(z, sqrt(x*x + y*y))`.
  - `pub(crate) fn c2s_special(p: [f64; 3]) -> (f64, f64)`: the Task 4 libm body, unchanged.
  - `c2s(p)` becomes `if c2s_needs_special(p) { c2s_special(p) } else { c2s_kernel(p) }`.
  - `pal::dcc2s_slice` becomes two passes: `c2s_kernel` over every canonicalized point, then `c2s_special` for each point where `c2s_needs_special`; its contract (equal to `dcc2s` per point) is unchanged.

- [ ] **Step 1: Write the failing tests in `erfa/c2s.rs`**

```rust
#[test]
fn c2s_is_within_one_ulp_of_the_libm_body() {
    // unit_vectors(1_000_000) plus every specials() triple:
    // ulps(c2s(p).0, c2s_special(p).0) <= 1 and the same for .1;
    // and at least one input differs (proves the kernel is in use).
}

#[test]
fn per_value_and_slice_agree_bit_for_bit() {
    // for lengths 1, 2, 3, 7, 1001 of unit_vectors plus specials triples
    // interleaved: pal::dcc2s_slice output == pal::dcc2s per point, by bits.
}
```

The second test lives in `pal.rs`'s tests since `dcc2s_slice` is there.

- [ ] **Step 2: Run them to see them fail** — the first fails on "at least one input differs".

- [ ] **Step 3: Implement the kernel split and the two-pass slice.**

- [ ] **Step 4: Run the unit tests and the SphMap tests**

Run: `cargo test -p starlink-ast --lib erfa:: pal:: vmath:: && cargo test -p starlink-ast --test mappings test_sphmap::`
Expected: PASS.

- [ ] **Step 5: Find and inspect every moved result**

Commit, then run:
```
cargo run --release -p ast-bench -- ulps --base HEAD~1
cargo test -p starlink-ast
cargo test -p ast-oracle-tests   # tier 4, needs the submodule; run by hand
```
For each corpus case `ulps` lists, and each failing test: confirm the chain contains `pal::dcc2s` (SphMap, SkyFrame, SlaMap, SpecMap all call it per value and now get the kernel too) and that the drift is explained by ≤1 ULP at `c2s` growing through later stages.
A test failing for any other reason is a bug: stop and fix it.
For a justified failure, relax that case only, named in source with a comment giving the measured ULP and the reason, and count it in `PLAN.md` §6.
Never re-baseline a fixture file in bulk; a regenerated fixture's commit message lists each case and its measured ULP.

- [ ] **Step 6: Time it**

Run: `cargo run --release -p ast-bench -- compare --set native --base HEAD~1` (expect only speed-ups; SphMap forward and the FrameSets containing SphMap should be faster at large N, and N=1 not slower beyond the threshold) and `cargo run --release -p ast-bench -- run --set native` for the numbers.
If SphMap forward at N=1 000 000 is not faster than at HEAD~1, stop: the kernel did not vectorize; check `cargo rustc -p starlink-ast --release -- --emit asm` for vector `fdiv` in `dcc2s_slice` before going further.

- [ ] **Step 7: Add the CHANGELOG entry and commit**

`CHANGELOG.md`, `Unreleased`, `### Changed`: "SphMap's forward transform, and other conversions from Cartesian to spherical coordinates, use a vectorizable arctangent; results may differ from earlier releases by up to 1 ULP."

```bash
git add -A crates/starlink-ast CHANGELOG.md PLAN.md
git commit -m "Vectorize eraC2s with a branchless atan2, within 1 ULP"
```

---

### Task 8: Try a branchless `sin`/`cos` for `s2c`

**Files:**
- Create (if kept): `crates/starlink-ast/src/vmath/sincos.rs`
- Modify (if kept): `crates/starlink-ast/src/erfa/s2c.rs`, `crates/starlink-ast/src/pal.rs` (`dcs2c_slice`), `CHANGELOG.md`
- Modify (either way): `crates/starlink-ast/src/vmath/mod.rs` doc (records the result)

**Interfaces:**
- Produces (if kept): `pub(crate) fn vmath::sin_cos(x: f64) -> (f64, f64)` and `pub(crate) fn vmath::sin_cos_covers(x: f64) -> bool`, with `s2c` split into `s2c_kernel` / `s2c_needs_special` / `s2c_special` exactly as `c2s` in Task 7.

- [ ] **Step 1: Write the tests first** — `sin_cos_is_within_one_ulp_of_libm` over `angle_pairs` angles and `specials()`, and `per_value_and_slice_agree_bit_for_bit` for `dcs2c_slice`, as in Tasks 6 and 7.

- [ ] **Step 2: Implement** fdlibm's `__rem_pio2` (the medium-size path only; `sin_cos_covers` is false beyond it) and the `__sin`/`__cos` kernels from musl, selecting the quadrant with floating-point arithmetic rather than an integer, so the loop vectorizes; Sun's notice and lineage as in Task 6.

- [ ] **Step 3: Decide**

Keep it only if every test passes and `ast-bench compare --set native --base HEAD` shows SphMap inverse faster at N=1 000 000 beyond the 10% threshold.
Otherwise delete the code, keep `s2c` on libm, and record in `vmath/mod.rs`'s doc what was tried, the measured ULP and timing, and why it was not kept (the user accepts this outcome).

- [ ] **Step 4: Run Task 7 Step 5's inspection if kept, then commit** — message "Vectorize eraS2c with a branchless sin and cos, within 1 ULP" or "Record why eraS2c stays on libm".

---

### Task 9: SphMap milestone records and the x86 hand-off

**Files:**
- Create: `docs/perf/<date>-macos-aarch64-native.tsv` (snapshot), `docs/perf/<date>-macos-aarch64-native-roman.txt`
- Modify: `docs/perf/README.md`, `PLAN.md` §7 (step 6a row: SphMap done), `docs/issues/vectorized-erfa-pal-wcslib.md` (status line points to the spec and this plan)

- [ ] **Step 1: Snapshot and roman on a quiet machine**

Run: `cargo run --release -p ast-bench --features c-oracle -- snapshot --set native` and `cargo run --release -p ast-bench --features c-oracle -- roman > docs/perf/<date>-macos-aarch64-native-roman.txt`.

- [ ] **Step 2: README rows** — one per file, each stating SphMap forward and inverse ns/point at N=1 000 000 against the Task 1 baseline and C, and the vForce yardstick from Task 3.

- [ ] **Step 3: Commit**

```bash
git add docs/perf PLAN.md docs/issues/vectorized-erfa-pal-wcslib.md
git commit -m "Record SphMap after the native ERFA port and vectorization"
```

- [ ] **Step 4: Hand the user the x86-64 commands**

With BASELINE the Task 1 commit and HEAD the Task 9 commit, give the user exactly:

```
git checkout tickets/DM-56839-sphmap && git submodule update --init reference/starlink-ast
tools/perf/build_c_engine.sh bench
cargo run --release -p ast-bench -- compare --set native --base <BASELINE sha>
cargo run --release -p ast-bench --features c-oracle -- snapshot --set native
cargo run --release -p ast-bench --features c-oracle -- roman
```

and ask for the outputs; they become `docs/perf/<date>-linux-x86_64-native*.{tsv,txt}` records with README rows when returned.

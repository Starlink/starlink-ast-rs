# Transform-Oracle Parity — Phase 1 (SlaMap) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `SlaMap`'s transform to carry scalar `(alpha, delta)` coordinates through each conversion step exactly as C's `slamap.c` `Transform` does, closing every SlaMap output-mismatch in the transform oracle, and add a per-class oracle gate so the fixed sections cannot silently regress.

**Architecture:** C's `SlaMap` `Transform` carries **scalar longitude/latitude arrays** (`alpha`, `delta`) through an ordered chain of SLALIB/PAL conversions, mutating them in place per step.
The Rust port instead carries a **Cartesian 3-vector** between steps and converts to spherical only once at the chain end.
That carrier difference is the root cause of the SlaMap oracle failures: it discards longitude winding (so `R2H`∘`H2R` cannot return a raw out-of-range longitude), it never applies C's per-step `[0, 2π)` normalization on the matrix conversions, and it has no per-step bad-argument guard.
This plan replaces the vector carrier with a faithful scalar carrier that mirrors C's `Transform` switch arm-for-arm, folding in the two known math bugs (the `ECLEQ`/`EQECL` precession factor and the `AMP` `palMappa` argument order) as arms of that switch.
Each behavior is locked by a public-API unit test, and the existing `test_transform_oracle` harness is extended with a non-`#[ignore]` SlaMap gate carrying a shrinking exclusion list (mirroring `NATIVE_READ_EXCLUDED`).

**Tech Stack:** Rust 2024, `ast-core` crate. PAL conversions via the crate-internal `pal` FFI module (`crates/ast-core/src/pal.rs`). C ground truth: `reference/starlink-ast/src/slamap.c`. Oracle fixtures: `crates/ast-core/tests/fixtures/oracle/*.oracle`.

## Global Constraints

- **C-fidelity is the rule.** Every arm mirrors C control flow directly (`reference/starlink-ast/src/slamap.c` `Transform`, ~line 3540). No compute-then-check tolerance heuristics without asking the human first. Where C uses a heuristic, replicate it and comment that it mirrors C.
- **Port ALL C logic.** Lack of test coverage is never a reason to skip a C branch. C logic beats existing diverged Rust — the vector carrier is discarded, not patched.
- **`AST__BAD` = `BAD_VALUE` = `-f64::MAX`** (bit-identical to C). Inputs may be `f64::NAN`; outputs always use `BAD_VALUE`. `is_bad(v)` (`= v == BAD_VALUE || !v.is_finite()`) treats NaN as bad, matching C's `== AST__BAD` guard once NaN is excluded upstream.
- **Public Rust API only in tests** (`ast_core::mappings::{SlaMap, SlaCvtType, Mapping}`, `ast_core::coords::PointSet`, `ast_core::constants::BAD_VALUE`). No private modules. Step-level helpers cannot be unit-tested directly; test through `SlaMap::transform`.
- **Fixtures must work with the submodule absent** — the oracle corpus is already vendored under `crates/ast-core/tests/fixtures/oracle/`. Never symlink from `reference/`.
- **Comments describe the code as it is today** — no references to prior bugs, plans, the vector carrier, or "was previously". State what C does and why.
- **C-oracle golden values come from a C probe** linked against the repo's own `libast.so` at `reference/starlink-ast/build-ast/libast.so` (recipe: `docs/issues/c-oracle-probe-recipe.md`; mind the conda-RPATH trap). Never invent a golden value.
- **Formatting/lint gate before every commit:** `cargo fmt` and clippy clean. Clippy under the conda/rustup mismatch uses the documented workaround (isolated `CARGO_TARGET_DIR` + `RUSTC` override); commit with `--no-verify` only if the pre-commit clippy hook hits the E0514 quirk, after verifying clippy passes under the workaround.
- **One sentence per line** in prose/Markdown. American English.

## Background: what C's `Transform` does (the port target)

C's `Transform` (`slamap.c:3540-4230`) after the parent call:

1. Aliases `alpha = ptr_out[0]`, `delta = ptr_out[1]` and `memcpy`s the inputs into them (`3626-3630`).
2. Sets loop direction from `forward` XOR `astGetInvert` (`3618`, then `start`/`end`/`inc` at `3636-3638`).
3. For each conversion in order:
   - Reads `args = cvtargs[cvt]` (`3646`).
   - **Bad-args guard** (`3672-3687`): calls `CvtString(ct, ..., &nargs, ...)`, then if any of the first `nargs` args `== AST__BAD`, sets `alpha[point] = delta[point] = AST__BAD` for every point and `continue`s to the next conversion (bad values then propagate).
   - Dispatches on `ct` through the `TRAN_ARRAY` macro (`3651-3661`), which for each point sets both axes bad if **either** `alpha[point]` or `delta[point]` is `AST__BAD`, else runs the arm body.
4. The matrix arms (`PREBN` `3726`, `PREC` `3751`, `ECLEQ` `3878`, `EQECL` `3924`) each do, per point: `palDcs2c(alpha,delta,v1) → palDmxv(matrix,v1,v2) → palDcc2s(v2,&alpha,&delta) → alpha = palDranrm(alpha)`. `palDranrm` wraps longitude into `[0, 2π)` (C comment: "palDcc2s doesn't do this itself"). These four are the **only** arms that call `palDranrm`; the pal-function arms (`GALEQ`, `FK45Z`, `DE2H`, …) normalize internally.
5. `R2H`/`H2R` (`4204-4207`): `TRAN_ARRAY( alpha[point] = args[0] - alpha[point]; )` — **scalar arithmetic, not normalized**; the forward and inverse are identical.
6. `J2000H`/`HJ2000` (`4191`, `4198`): call the file-static `J2000H(forward, npoint, alpha, delta)` helper, which does its **own** per-point loop (`palDcs2c → palDmxv`/`palDimxv` → `palDcc2s`, **no** `palDranrm`, **no** `TRAN_ARRAY` bad-check — it is bypassed for this arm).
7. `HPCEQ`/`HPREQ`/`HEEQ` and their `EQ*` inverses (`4195-4188`): call `STPConv(...)` on the `alpha`/`delta` arrays directly (no `TRAN_ARRAY`, no `palDranrm`).

The precession-bearing arms build the combined matrix (`ECLEQ` `3888-3894`, `EQECL` `3933-3935`):

```
palPrec( 2000.0, palEpj( args[0] ), precess );   /* J2000 -> date */
palEcmat( args[0], rotate );                      /* equatorial -> ecliptic at date */
palDmxm( rotate, precess, convert );              /* convert = rotate . precess */
```

`ECLEQ` forward applies `palDimxv(convert, ...)` (ecliptic→equatorial = inverse); `EQECL` forward applies `palDmxv(convert, ...)` (equatorial→ecliptic).

The `MAP`/`AMP` arms consume a star-independent `amprms[21]` array from `palMappa`. C orders its arguments per conversion: `MAP` args are `(EQ, DATE)` → `palMappa(args[0], args[1])` (`3851-3854`); `AMP` args are `(DATE, EQ)` → `palMappa(args[1], args[0])` (`3815-3818`). The history note at `slamap.c:79` records the AMP argument-order fix.

## File Structure

- `crates/ast-core/src/mappings/slamap.rs` — the SlaMap implementation. Rewrite sites:
  - `apply_step_forward` (~line 936) and `apply_step_inverse` (~line 1089): the `[f64; 3]`-vector step functions. Replaced by scalar functions carrying `(alpha, delta)` — signature `fn apply_step_forward(cvt: &SlaConversion, alpha: f64, delta: f64, forward: bool, amprms: Option<&[f64; 21]>) -> (f64, f64)` (see Task 1 for the exact shape). The matrix arms do the `dcs2c`/matmul/`dcc2s`/`dranrm` round-trip internally.
  - `transform_into` (~line 1176): the per-point loop. Rewritten to carry scalar `(alpha, delta)`, apply the per-step bad-args guard and the `TRAN_ARRAY` axis-coupling, and drop the single end-of-chain `dcc2s`.
  - `transform_into_nan` (~line 1239): the NaN fast-path override. Removed — SlaMap's per-step work (matrix + `atan2` in `dranrm`, `palMappa`, trig) is not the branchless-polynomial case the fast path targets, and C's per-step axis-coupling must run. Deleting the override delegates to the trait default, which calls `transform_into` (whose `is_bad` gate already treats NaN as bad).
- `crates/ast-core/tests/test_slamap.rs` — public-API unit tests. Gets new tests locking each behavior (matrix-op `[0, 2π)` longitude, off-J2000 `EQECL` golden, `R2H`/`H2R` raw-longitude preservation, `AMP`∘`MAP` round-trip, bad-arg propagation).
- `crates/ast-core/tests/test_transform_oracle.rs` — the oracle harness. Gets a new `#[test]` (not `#[ignore]`) `slamap_sections_match_oracle` gate with a `SLAMAP_ORACLE_EXCLUDED` list, plus a helper to run only SlaMap sections.

The `transform_oracle_report` `#[ignore]` investigative test is left untouched — it still measures the whole corpus. The new gate asserts only on SlaMap sections.

## Task sequence rationale

The carrier cannot be half-converted — one loop cannot carry scalars for some arms and a vector for others — so **Task 1 is a single coherent rewrite** of the whole step-dispatch and per-point loop, folding in the `ECLEQ`/`EQECL` precession fix (it lives in the matrix arms being rewritten anyway).
Task 1 deliberately **preserves the existing (buggy) `AMP` `palMappa` argument order** so that Task 2 is a clean, independently reviewable two-line fix with its own locking test.
Task 3 converts the oracle harness into a gate once the math is correct.

---

### Task 1: Scalar-carrier rewrite of `SlaMap` transform (folds in ECLEQ/EQECL precession)

Replace the 3-vector carrier with C's scalar `(alpha, delta)` carrier.
This is the core faithful port.
It mirrors `slamap.c` `Transform` arm-for-arm: the bad-args guard, the `TRAN_ARRAY` axis-coupling, per-step `palDranrm` on the four matrix arms only, `R2H`/`H2R` as scalar `args[0] - alpha`, and the corrected `ECLEQ`/`EQECL` combined precession matrix.
It keeps the existing `MAP | AMP` `mappa_cache` argument order (Task 2 fixes `AMP`).

**Files:**
- Modify: `crates/ast-core/src/mappings/slamap.rs` — replace `apply_step_forward` (~936), `apply_step_inverse` (~1089), rewrite the `transform_into` loop (~1176), delete `transform_into_nan` (~1239).
- Test: `crates/ast-core/tests/test_slamap.rs`.

**Interfaces:**
- Consumes (all `pub(crate)` in `crates/ast-core/src/pal.rs`, already exposed): `pal::dcs2c(a, b) -> [f64; 3]`, `pal::dcc2s(v: &[f64; 3]) -> (f64, f64)`, `pal::dmxv(dm, va) -> [f64; 3]`, `pal::dimxv(dm, va) -> [f64; 3]`, `pal::dmxm(a, b) -> [[f64; 3]; 3]`, `pal::prec(ep0, ep1) -> [[f64; 3]; 3]`, `pal::prebn(ep0, ep1) -> [[f64; 3]; 3]`, `pal::ecmat(date) -> [[f64; 3]; 3]`, `pal::epj(date) -> f64`, `pal::dranrm(angle) -> f64`, `pal::deuler(order, a, b, c) -> [[f64; 3]; 3]`, `pal::mappa(eq, date) -> [f64; 21]`, `pal::mapqkz`, `pal::ampqk`, `pal::galeq`/`eqgal`/`galsup`/`supgal`/`fk45z`/`fk54z`/`fk5hz`/`hfk5z`/`addet`/`subet` (all already used by the current arms). The `SlaCvtType::nargs()` method (`slamap.rs:86`) gives the bad-args guard its `nargs` count. `SlaConversion { cvt_type, args }` (`slamap.rs:330`). `de2h_with_aberration` / `dh2e_with_aberration` / `stp_convert` helpers (already in the file).
- Produces: a scalar step-dispatch and a `transform_into` that reproduces C's `Transform` byte-for-byte in control flow. `apply_step_forward` / `apply_step_inverse` change signature from `([f64;3]) -> [f64;3]` to `(alpha, delta, forward, amprms) -> (f64, f64)` (or two functions keyed on `forward` — implementer's choice, mirroring the existing forward/inverse split). No other file consumes these functions (verified: only `transform_into` / `transform_into_nan` call them).

- [ ] **Step 1: Write the failing tests**

Add these tests to `test_slamap.rs`.
The `GOLDEN_*` constants in `eqecl_matches_c_off_j2000` are placeholders — Step 2 obtains them from a C probe before the fix is written.

```rust
// R2H then H2R is the identity, and C carries the raw (un-normalized)
// longitude scalar through both steps, so a large out-of-range input
// longitude must survive verbatim. The vector carrier cannot reproduce
// this (a unit vector only recovers (-pi, pi]); the scalar carrier can.
#[test]
fn r2h_h2r_preserves_raw_longitude() {
    let last = 3.5;
    let mut m = SlaMap::new();
    m.add(SlaCvtType::R2h, &[last]); // alpha' = last - alpha
    m.add(SlaCvtType::H2r, &[last]); // alpha'' = last - alpha' = alpha
    // A longitude far outside [0, 2pi): the net map is the identity, and C
    // returns it unchanged rather than wrapping it.
    let raw_lon = -1000.0_f64;
    let lat = 0.3;
    let pin = PointSet::from_rows([[raw_lon, lat]].iter().map(|p| p.as_slice()));
    let out = m.transform(&pin, true).unwrap();
    assert_close(out.point(0)[0], raw_lon, 1e-9, "raw longitude preserved");
    assert_close(out.point(0)[1], lat, 1e-9, "latitude preserved");
}

// A single matrix conversion (PREC) must return longitude in [0, 2pi):
// C applies palDranrm after the matrix multiply. Feed a point whose
// precessed longitude is small-negative before wrapping, and confirm the
// output is the positive wrapped value (i.e. >= 0), not a (-pi, 0) value.
#[test]
fn matrix_op_longitude_is_normalized_to_0_2pi() {
    // PREC from J2000.0 to J2000.0 is the identity matrix, so the output
    // longitude equals palDranrm(input longitude). A negative input
    // longitude must come back wrapped into [0, 2pi).
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Prec, &[2000.0, 2000.0]);
    let neg_lon = -1.0_f64; // ~ -57 deg
    let pin = PointSet::from_rows([[neg_lon, 0.2]].iter().map(|p| p.as_slice()));
    let out = m.transform(&pin, true).unwrap();
    let got = out.point(0)[0];
    assert!(got >= 0.0 && got < 2.0 * std::f64::consts::PI, "lon in [0,2pi): {got}");
    assert_angle_close(got, neg_lon, 1e-9, "wrapped value equals input mod 2pi");
}

// Off-J2000 EQECL golden value from C. C builds
// convert = ecmat(date) . prec(2000, epj(date)); the identity-at-J2000
// ecmat-only matrix is wrong away from J2000.0.
#[test]
fn eqecl_matches_c_off_j2000() {
    // MJD 55197.0 = 2010-01-01, ~10 years from J2000.0. Golden values from
    // a C probe of palSlaMap EQECL at this date for (ra,dec) = (83deg,22deg)
    // linked against reference/starlink-ast/build-ast/libast.so. FILL BEFORE
    // WRITING THE FIX (see Step 2).
    let date = 55197.0;
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Eqecl, &[date]);
    let pin = PointSet::from_rows(
        [[deg2rad(83.0), deg2rad(22.0)]].iter().map(|p| p.as_slice()),
    );
    let out = m.transform(&pin, true).unwrap();
    const GOLDEN_LON: f64 = /* from C probe */ 0.0;
    const GOLDEN_LAT: f64 = /* from C probe */ 0.0;
    assert_angle_close(out.point(0)[0], GOLDEN_LON, 1e-9, "ecl lon");
    assert_close(out.point(0)[1], GOLDEN_LAT, 1e-9, "ecl lat");
}

// A conversion with a bad argument zeroes (bad-values) the whole step's
// output for every point, and the bad propagates to the chain output.
#[test]
fn bad_arg_propagates_to_bad_output() {
    // ECLEQ needs one arg (the date). Supply BAD_VALUE: the step outputs
    // BAD for all points regardless of input, matching slamap.c:3672-3687.
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Eqecl, &[BAD_VALUE]);
    let pin = PointSet::from_rows(
        [[0.5, 0.2], [1.0, -0.3]].iter().map(|p| p.as_slice()),
    );
    let out = m.transform(&pin, true).unwrap();
    for i in 0..2 {
        assert_eq!(out.point(i)[0], BAD_VALUE, "point {i} lon bad");
        assert_eq!(out.point(i)[1], BAD_VALUE, "point {i} lat bad");
    }
}

// A bad input coordinate produces bad output on both axes (TRAN_ARRAY
// couples the axes: either bad -> both bad), and a NaN input is treated
// identically (is_bad includes !is_finite).
#[test]
fn bad_and_nan_input_produce_bad_output() {
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Prec, &[2000.0, 2010.0]);
    let pin = PointSet::from_rows(
        [[BAD_VALUE, 0.2], [f64::NAN, 0.3], [0.5, BAD_VALUE]]
            .iter()
            .map(|p| p.as_slice()),
    );
    let out = m.transform(&pin, true).unwrap();
    for i in 0..3 {
        assert_eq!(out.point(i)[0], BAD_VALUE, "point {i} lon bad");
        assert_eq!(out.point(i)[1], BAD_VALUE, "point {i} lat bad");
    }
}
```

- [ ] **Step 2: Obtain the EQECL golden value from a C probe, then run the tests to verify they fail**

Follow `docs/issues/c-oracle-probe-recipe.md`.
Build a tiny C program linked against `reference/starlink-ast/build-ast/libast.so` (force the repo build's RPATH per the recipe — the conda `libast.so` is stale and wins by default).
Construct an `AstSlaMap` with a single `EQECL` step at `args = {55197.0}`, transform the single point `(ra, dec) = (83 deg, 22 deg)` in radians forward, and print both output coordinates to full `%.17g` precision.
Substitute the two printed numbers for `GOLDEN_LON` / `GOLDEN_LAT` in `eqecl_matches_c_off_j2000`.

Run: `cargo test -p ast-core --test test_slamap r2h_h2r_preserves_raw_longitude matrix_op_longitude_is_normalized_to_0_2pi eqecl_matches_c_off_j2000 bad_arg_propagates_to_bad_output bad_and_nan_input_produce_bad_output`

Expected: FAIL. The vector carrier normalizes `R2H`∘`H2R` output into `(-pi, pi]` (so `-1000` is lost); it never applies `palDranrm` after `PREC` (so a negative longitude stays negative); it uses `ecmat` alone for `EQECL` (so the off-J2000 value is wrong); and it has no bad-args guard (so a bad `EQECL` date yields NaN/garbage, not `BAD_VALUE`). At least one assertion in each of the first four tests fails; `bad_and_nan_input_produce_bad_output` may already pass (the vector path's `is_bad` gate handles bad inputs) — that is fine, it guards the rewrite.

- [ ] **Step 3: Write the fix**

Rewrite the three functions in `slamap.rs`.
Mirror `slamap.c` `Transform` (see "Background" above).
Carry scalar `(alpha, delta)`.

**3a. Scalar step functions.**
Replace `apply_step_forward` and `apply_step_inverse` with scalar versions.
Keep the existing forward/inverse split (inverse routes most arms through the inverse `SlaCvtType` plus `Prec`/`Prebn` epoch-swap, `J2000h`/`Hj2000` transpose, and STP `forward=false`, exactly as today).
Convert each arm from `[f64; 3]` in/out to `(f64, f64)` in/out.
For the arms that currently do `let (lon, lat) = pal::dcc2s(&v); ...; pal::dcs2c(a, b)`, drop the `dcc2s`/`dcs2c` bookends and operate on the scalar `(alpha, delta)` directly (they already computed spherical→spherical internally).
For the matrix arms, do the round-trip internally and apply `palDranrm`:

```rust
// Prec forward: precession matrix, then re-normalize longitude into
// [0, 2pi) (palDcc2s does not; slamap.c:3762).
SlaCvtType::Prec => {
    let m = pal::prec(cvt.args[0], cvt.args[1]);
    let v = pal::dcs2c(alpha, delta);
    let (a, d) = pal::dcc2s(&pal::dmxv(&m, &v));
    (pal::dranrm(a), d)
}
```

Apply the same `dcs2c → matmul → dcc2s → (dranrm(a), d)` shape to `Prebn` (using `pal::prebn`), and to `Ecleq`/`Eqecl` with the corrected combined precession matrix:

```rust
// Ecliptic (of date) to equatorial J2000.0. C builds the combined matrix
// precessing J2000 equatorial to the date then rotating to ecliptic
// (slamap.c:3888); ECLEQ applies its inverse (palDimxv). Longitude is
// wrapped into [0, 2pi) afterwards (slamap.c:3906/3915).
SlaCvtType::Ecleq => {
    let precess = pal::prec(2000.0, pal::epj(cvt.args[0]));
    let rotate = pal::ecmat(cvt.args[0]);
    let convert = pal::dmxm(&rotate, &precess);
    let v = pal::dcs2c(alpha, delta);
    let (a, d) = pal::dcc2s(&pal::dimxv(&convert, &v));
    (pal::dranrm(a), d)
}
// Equatorial J2000.0 to ecliptic (of date). Forward applies the matrix
// directly (palDmxv) (slamap.c:3924).
SlaCvtType::Eqecl => {
    let precess = pal::prec(2000.0, pal::epj(cvt.args[0]));
    let rotate = pal::ecmat(cvt.args[0]);
    let convert = pal::dmxm(&rotate, &precess);
    let v = pal::dcs2c(alpha, delta);
    let (a, d) = pal::dcc2s(&pal::dmxv(&convert, &v));
    (pal::dranrm(a), d)
}
```

`R2H`/`H2R` become scalar arithmetic with **no** normalization (slamap.c:4204):

```rust
// R2H and H2R are identical scalar operations (self-inverse): the
// longitude is reflected about the local sidereal time, un-normalized.
SlaCvtType::R2h | SlaCvtType::H2r => (cvt.args[0] - alpha, delta),
```

`J2000h`/`Hj2000` do the matrix round-trip with **no** `palDranrm` (the C `J2000H` helper does not normalize):

```rust
SlaCvtType::J2000h => {
    let rmat = pal::deuler("XYZ", -0.0068192 * AS2R, 0.0166172 * AS2R, 0.0146000 * AS2R);
    let v = pal::dcs2c(alpha, delta);
    pal::dcc2s(&pal::dmxv(&rmat, &v))
}
SlaCvtType::Hj2000 => {
    let rmat = pal::deuler("XYZ", -0.0068192 * AS2R, 0.0166172 * AS2R, 0.0146000 * AS2R);
    let v = pal::dcs2c(alpha, delta);
    pal::dcc2s(&pal::dimxv(&rmat, &v))
}
```

The `Map`/`Amp`, `Galeq`/`Eqgal`/`Galsup`/`Supgal`, `Fk45z`/`Fk54z`/`Fk5hz`/`Hfk5z`, `Addet`/`Subet`, `De2h`/`Dh2e`, and STP arms keep their current internal math but take/return scalar `(alpha, delta)` instead of `dcc2s(&v)` / `dcs2c(...)`.
**Keep the existing `Map | Amp` `mappa_cache` argument order for now** — Task 2 fixes `AMP`.

**3b. `transform_into` loop.**
Rewrite the per-point loop to carry scalar `(alpha, delta)` and enforce C's per-step semantics.
Compute the direction once (`let fwd = self.effective_forward(forward);`).
Keep the `mappa_cache` as-is.
Structure the loop over conversions on the outside and points on the inside, mirroring C (the bad-args guard is per-conversion, not per-point):

```rust
// Initialise output = input (C memcpy at slamap.c:3626).
for i in 0..pts.npoint() {
    out.point_mut(i)[0] = pts.point(i)[0];
    out.point_mut(i)[1] = pts.point(i)[1];
}

// Apply each conversion in order (reverse for inverse), mutating the
// output arrays in place — the scalar (alpha, delta) carrier.
let order: Vec<usize> = if fwd {
    (0..self.conversions.len()).collect()
} else {
    (0..self.conversions.len()).rev().collect()
};

for &ci in &order {
    let cvt = &self.conversions[ci];
    let cached = mappa_cache[ci].as_ref();

    // Bad-args guard (slamap.c:3672-3687): if any of this conversion's
    // nargs arguments is bad, the whole step outputs BAD for every point.
    let nargs = cvt.cvt_type.nargs();
    if cvt.args.iter().take(nargs).any(|&a| is_bad(a)) {
        for i in 0..out.npoint() {
            out.point_mut(i)[0] = BAD_VALUE;
            out.point_mut(i)[1] = BAD_VALUE;
        }
        continue;
    }

    // TRAN_ARRAY (slamap.c:3651): per point, either axis bad -> both bad,
    // else apply the conversion.
    for i in 0..out.npoint() {
        let p = out.point_mut(i);
        if is_bad(p[0]) || is_bad(p[1]) {
            p[0] = BAD_VALUE;
            p[1] = BAD_VALUE;
        } else {
            let (a, d) = if fwd {
                apply_step_forward(cvt, p[0], p[1], cached)
            } else {
                apply_step_inverse(cvt, p[0], p[1], cached)
            };
            p[0] = a;
            p[1] = d;
        }
    }
}
```

(The implementer may hoist `p` reads/writes to locals to satisfy the borrow checker; the control flow above is the contract.)
Note: the `mappa_cache` is indexed by conversion position `ci` and consumed in the same order regardless of direction, matching C reading `cvtargs[cvt]` for the current `cvt` — do **not** reverse the cache separately.

**3c. Delete `transform_into_nan`.**
Remove the whole override.
SlaMap then inherits the trait default (delegates to `transform_into`).
This is faithful — C runs the same per-step bad-checks in both directions — and perf-neutral for the real hot path (SkyFrame, not SlaMap; SlaMap steps are matrix/`atan2`/`palMappa`, not the branchless polynomials the fast path targets).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ast-core --test test_slamap`
Expected: PASS (all) — the five new tests plus every pre-existing SlaMap test.
The pre-existing J2000.0-dated tests still pass because `prec(2000, 2000)` is the identity and `dranrm` is a no-op on already-in-range longitudes.

- [ ] **Step 5: Commit**

Run `cargo fmt` and clippy (workaround per Global Constraints) first.

```bash
git add crates/ast-core/src/mappings/slamap.rs crates/ast-core/tests/test_slamap.rs
git commit -m "fix(slamap): carry scalar (alpha,delta) through transform like C

Port slamap.c Transform faithfully: carry scalar longitude/latitude
through each step, apply the per-step bad-args guard and TRAN_ARRAY
axis-coupling, normalize longitude with palDranrm on the four matrix
conversions only, keep R2H/H2R as un-normalized scalar arithmetic, and
build the ECLEQ/EQECL combined precession matrix ecmat(date).prec(2000,
epj(date)). Removes the diverged 3-vector carrier and its NaN fast path."
```

---

### Task 2: Fix AMP `palMappa` argument order

C orders `palMappa` arguments per conversion: `MAP` args are `(EQ, DATE)` → `palMappa(eq=args[0], ep=args[1])` (slamap.c:3851-3854); `AMP` args are `(DATE, EQ)` → `palMappa(eq=args[1], ep=args[0])` (slamap.c:3815-3818).
The history note at slamap.c:79 records this exact fix.
The `mappa_cache` closure calls `pal::mappa(cvt.args[0], cvt.args[1])` for **both** `Map` and `Amp`, so `Amp` swaps its epoch and equinox.

**Files:**
- Modify: `crates/ast-core/src/mappings/slamap.rs` — the `mappa_cache` closure in `transform_into` (only one closure now, after Task 1 deleted `transform_into_nan`).
- Test: `crates/ast-core/tests/test_slamap.rs`.

**Interfaces:**
- Consumes: `pal::mappa(eq: f64, date: f64) -> [f64; 21]`.
- Produces: `Amp` steps cache with the correct `(eq, date)` order. The step arms are unchanged — they consume the cached `amprms`.

- [ ] **Step 1: Write the failing test**

`AMP` (geocentric apparent → mean place) followed by `MAP` (mean → apparent) at the same equinox and date must round-trip to the input.
With `AMP`'s arguments swapped, the two steps use different `amprms` arrays and do not cancel.

```rust
#[test]
fn amp_map_round_trip() {
    // AMP args are (DATE, EQ); MAP args are (EQ, DATE). Chaining AMP then
    // MAP with matching equinox/date must return the input.
    let eq = 2000.0;
    let date = 55197.0; // MJD 2010-01-01
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Amp, &[date, eq]); // apparent -> mean  (args: DATE, EQ)
    m.add(SlaCvtType::Map, &[eq, date]); // mean -> apparent  (args: EQ, DATE)

    let ra = deg2rad(120.0);
    let dec = deg2rad(-15.0);
    let pin = PointSet::from_rows([[ra, dec]].iter().map(|p| p.as_slice()));
    let out = m.transform(&pin, true).unwrap();

    assert_angle_close(out.point(0)[0], ra, 1e-9, "ra AMP-then-MAP round-trip");
    assert_close(out.point(0)[1], dec, 1e-9, "dec AMP-then-MAP round-trip");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --test test_slamap amp_map_round_trip`
Expected: FAIL — `AMP` caches `mappa(date, eq)` (args swapped) while `MAP` caches `mappa(eq, date)`, so the two `amprms` arrays differ and the steps do not cancel; the output diverges from the input by the equinox/epoch mismatch.

- [ ] **Step 3: Write the fix**

In the `mappa_cache` closure in `transform_into`, split the `Map | Amp` arm into per-type argument order:

```rust
.map(|cvt| match cvt.cvt_type {
    // MAP args are (EQ, DATE): palMappa(eq=args[0], ep=args[1]).
    SlaCvtType::Map => Some(pal::mappa(cvt.args[0], cvt.args[1])),
    // AMP args are (DATE, EQ): palMappa(eq=args[1], ep=args[0])
    // (slamap.c:3815). MAP and AMP order their args oppositely.
    SlaCvtType::Amp => Some(pal::mappa(cvt.args[1], cvt.args[0])),
    _ => None,
})
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ast-core --test test_slamap amp_map_round_trip`
Expected: PASS.

Run: `cargo test -p ast-core --test test_slamap`
Expected: PASS (all) — confirms no regression.

- [ ] **Step 5: Commit**

Run `cargo fmt` and clippy first.

```bash
git add crates/ast-core/src/mappings/slamap.rs crates/ast-core/tests/test_slamap.rs
git commit -m "fix(slamap): correct palMappa argument order for AMP

AMP args are (DATE, EQ) so palMappa takes (eq=args[1], ep=args[0]);
MAP args are (EQ, DATE). The shared closure used args[0],args[1] for
both, swapping AMP's epoch and equinox (slamap.c:79, :3815)."
```

---

### Task 3: SlaMap oracle gate with a shrinking exclusion list

Convert the SlaMap scope of the transform-oracle harness from investigation to a regression gate.
Add a non-`#[ignore]` test that runs only the SlaMap-family oracle sections and asserts they match C, except for an explicit `SLAMAP_ORACLE_EXCLUDED` list of sections not yet fixed — each carrying a one-line reason, mirroring `NATIVE_READ_EXCLUDED` (`test_wcsconv.rs:72`).
After Tasks 1-2 the list should be empty or hold only a small, documented residue; the list only ever shrinks.

**Files:**
- Modify: `crates/ast-core/tests/test_transform_oracle.rs` — add the gate test and a SlaMap-section helper. Leave `transform_oracle_report` (the `#[ignore]` investigative test) untouched.

**Interfaces:**
- Consumes: `run_oracle(oracle_text: &str, overrides: &Overrides) -> Vec<SectionReport>`, `parse_overrides(text: &str) -> Overrides`, `SectionReport { relpath: String, forward: bool, outcome: Outcome }`, `enum Outcome { Pass, LoadFail(String), Skipped(String), SkippedByOverride(String), Arity(String), Mismatch(Vec<AxisMiss>) }`. The oracle dir is `concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle")`; the SlaMap sections all live in `simplify_fixtures.oracle`.
- Produces: a hard gate on SlaMap sections. No new production code.

- [ ] **Step 1: Determine which SlaMap sections are SlaMap-owned, and which (if any) still fail after Tasks 1-2**

Regenerate the report and inspect the SlaMap-family sections:

```bash
cargo test -p ast-core --test test_transform_oracle -- --ignored --nocapture transform_oracle_report
python3 - <<'PY'
import json
d = json.load(open('target/transform_oracle_report.json'))
for s in d['sections']:
    rp = s['relpath'].lower()
    if ('sla' in rp) and s['status'] != 'pass':
        print(s['status'], s['relpath'], '::', s['detail'][:70])
PY
```

The SlaMap-owned sections are the `simplify_fixtures/*sla*` `.map` and `.simp` entries (the current failing set is: `cap_slamap_05`, `neg_sla_2arg_mismatch`, `sla_geocentric_cancel`, `sla_ha_cancel`, `sla_prec_merge`, `sla_run_partial`, `slamap_invert_normalize` — 19 section-directions).
Any still-failing section after Tasks 1-2 goes into `SLAMAP_ORACLE_EXCLUDED` with a specific reason; the rest are asserted `Pass` (or `SkippedByOverride`, which the committed `transform_oracle_overrides.txt` may set for some).
A `.simp` equivalence mismatch that is really a **simplifier** divergence (not a transform-math bug) is out of Phase 1 scope — exclude it with reason `"simplifier: <detail>"` and note it for Phase 2; do not force it green by changing transform math.

- [ ] **Step 2: Write the gate test**

Add to `test_transform_oracle.rs` (outside the existing `#[cfg(test)] mod` blocks, at file scope, alongside `transform_oracle_report`):

```rust
/// SlaMap-family oracle sections not yet matching C, each with a one-line
/// reason. The goal is to empty this list; it only ever shrinks. A section
/// is identified by (relpath, forward). Reasons name the divergence class
/// (e.g. "simplifier: normal-form" for a .simp equivalence gap that belongs
/// to Phase 2, not SlaMap transform math).
const SLAMAP_ORACLE_EXCLUDED: &[(&str, bool, &str)] = &[
    // Populate from Step 1 with whatever still fails after the fixes. Ideally
    // empty. Example row shape:
    // ("simplify_fixtures/sla_prec_merge.simp", true, "simplifier: normal-form (Phase 2)"),
];

/// Is this oracle section owned by SlaMap transform math (a *sla* fixture in
/// the simplify corpus)?
fn is_slamap_section(relpath: &str) -> bool {
    let base = relpath.rsplit('/').next().unwrap_or(relpath);
    base.contains("sla") // cap_slamap_*, sla_*, slamap_*, neg_sla_*
}

/// Regression gate: every SlaMap-family oracle section matches C, except the
/// explicitly-excluded ones. Mirrors the NATIVE_READ_EXCLUDED discipline in
/// test_wcsconv.rs — the list only shrinks; empty list == parity proven for
/// this class.
#[test]
fn slamap_sections_match_oracle() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oracle");
    let ov = parse_overrides(
        &std::fs::read_to_string(format!("{dir}/transform_oracle_overrides.txt"))
            .expect("overrides file present"),
    );
    let text = std::fs::read_to_string(format!("{dir}/simplify_fixtures.oracle"))
        .expect("oracle present");
    let reports = run_oracle(&text, &ov);

    let excluded: std::collections::HashSet<(&str, bool)> = SLAMAP_ORACLE_EXCLUDED
        .iter()
        .map(|(rp, fwd, _)| (*rp, *fwd))
        .collect();

    let mut unexpected_failures: Vec<String> = Vec::new();
    let mut stale_exclusions: Vec<(&str, bool)> = excluded.iter().copied().collect();

    for r in &reports {
        if !is_slamap_section(&r.relpath) {
            continue;
        }
        let key = (r.relpath.as_str(), r.forward);
        let is_fail = matches!(r.outcome, Outcome::Mismatch(_) | Outcome::LoadFail(_) | Outcome::Arity(_));
        if excluded.contains(&key) {
            // An excluded section that now passes means the list is stale.
            if !is_fail {
                // Passing: keep it in stale list to report; the gate should
                // then have the entry removed.
            } else {
                stale_exclusions.retain(|k| k != &key);
            }
        } else if is_fail {
            unexpected_failures.push(format!("{} (forward={}): {:?}", r.relpath, r.forward, r.outcome));
        }
    }

    assert!(
        unexpected_failures.is_empty(),
        "SlaMap oracle sections diverged from C (not excluded):\n{}",
        unexpected_failures.join("\n"),
    );
    assert!(
        stale_exclusions.is_empty(),
        "SLAMAP_ORACLE_EXCLUDED entries now pass — remove them:\n{:?}",
        stale_exclusions,
    );
}
```

(The implementer refines the stale-exclusion bookkeeping if a cleaner form is clearer; the two assertions — no un-excluded failure, no stale exclusion — are the contract.)

- [ ] **Step 3: Run the gate to verify it passes**

Run: `cargo test -p ast-core --test test_transform_oracle slamap_sections_match_oracle`
Expected: PASS — every SlaMap section either matches C or is explicitly excluded with a reason, and no excluded section secretly passes.

Run the whole oracle test file to confirm no collateral breakage:
Run: `cargo test -p ast-core --test test_transform_oracle`
Expected: PASS (all non-`#[ignore]` tests).

- [ ] **Step 4: Commit**

Run `cargo fmt` and clippy first.

```bash
git add crates/ast-core/tests/test_transform_oracle.rs
git commit -m "test(slamap): gate SlaMap oracle sections against C

Add slamap_sections_match_oracle: asserts every *sla* simplify-corpus
section matches the C oracle, except an explicit SLAMAP_ORACLE_EXCLUDED
list (each with a reason). Mirrors NATIVE_READ_EXCLUDED; the list only
shrinks. transform_oracle_report is left untouched."
```

---

## Self-Review

**Spec coverage** (against `docs/superpowers/specs/2026-08-02-transform-oracle-parity-design.md`, Phase 1):

- Fix A (ECLEQ/EQECL precession) — Task 1, matrix arms with `ecmat(date) . prec(2000, epj(date))`. ✓
- Fix B (AMP `palMappa` arg order) — Task 2. ✓
- Fix C (per-step longitude normalization) — Task 1, `palDranrm` on the four matrix arms only, verified against C (`PREBN`/`PREC`/`ECLEQ`/`EQECL` are the only `palDranrm` call sites in `Transform`). The spec framed this as "minor"; diagnosis showed it is inseparable from the carrier and is realized by the scalar-carrier rewrite. ✓
- Fix D (bad-args guard) — Task 1, the per-conversion `nargs` guard mirroring slamap.c:3672-3687. ✓
- Off-J2000 regression test — Task 1, `eqecl_matches_c_off_j2000` (golden from C probe). ✓
- Gate with shrinking exclusion list — Task 3. ✓
- **Carrier divergence (not in the original spec, adjudicated with the human 2026-08-02):** the vector-vs-scalar carrier is the actual root cause of `sla_ha_cancel`, `slamap_invert_normalize`, and the bulk of `cap_slamap_05`. Resolved by the faithful scalar-carrier port (Task 1), per the "C logic beats existing diverged Rust" rule. ✓

**Placeholder scan:** the only intentional placeholders are `GOLDEN_LON`/`GOLDEN_LAT` in `eqecl_matches_c_off_j2000` and the `SLAMAP_ORACLE_EXCLUDED` rows — both are explicitly filled from a C probe / the post-fix report during execution (Task 1 Step 2, Task 3 Step 1), with instructions in place. No "TBD"/"handle appropriately"/"similar to" placeholders remain.

**Type consistency:** step-function signature is stated once (Task 1 Interfaces) as `(cvt, alpha, delta, [forward,] amprms) -> (f64, f64)` and used consistently in the loop contract. `SectionReport`/`Outcome` field and variant names in Task 3 match the current `test_transform_oracle.rs` definitions (verified: `outcome`, `Outcome::Mismatch(Vec<AxisMiss>)`, `LoadFail`, `Arity`). `pal` function names match `pal.rs`. `nargs()` and `SlaConversion { cvt_type, args }` match `slamap.rs`.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-08-02-transform-oracle-parity-phase1-slamap.md`.
Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?

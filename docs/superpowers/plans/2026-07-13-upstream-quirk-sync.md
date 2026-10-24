# Upstream Quirk-Fix Sync + Simplify Fixture Import — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Realign the Rust port with the fixed C oracle at `50c2b21b` — remove the workarounds that mirror now-fixed C bugs, update regenerated goldens, mark resolved quirks, and import the 164 new `astSimplify` fixtures.

**Architecture:** Two logical commit groups on the current branch `tickets/DM-54793`. Group A un-mirrors each fixed quirk (behavior + golden + doc + test, each quirk self-contained). Group B imports the new simplify corpus, tags unported-class fixtures, and fixes all supported-class failures.

**Tech Stack:** Rust 2024, `cargo test -p starlink-ast`, reference C `libast` built with cmake for oracle verification.

**Design spec:** `docs/superpowers/specs/2026-07-13-upstream-quirk-sync-design.md`

## Global Constraints

- Rust 2024 edition; MSRV pinned in `rust-toolchain.toml`.
- `cargo fmt` and `cargo clippy --workspace -- -D warnings` must be clean before every commit (memory `feedback_format_clippy`).
- `#![warn(missing_docs)]` on public items; CI uses `RUSTFLAGS="-D warnings"`.
- `AST__BAD` / `BAD_VALUE` = `-f64::MAX`; never use `NaN` as an output sentinel.
- All axis indices are 1-based in the public Rust API.
- Fixtures are committed into `crates/starlink-ast/tests/fixtures/`; never symlink to the submodule.
- `cargo`/`rustc` live in the `lsst-scipipe` conda bin — ensure it is on `PATH` (memory `reference_cargo_conda_env`); pre-commit hooks need it too.
- Reference oracle `libast` is built with **cmake**, never autotools (memory `reference_oracle_build_cmake`).
- Port C control flow directly; no heuristics, no corner-cutting. Re-derive each change from the actual upstream `src/*.c` diff, not the quirks-doc's proposed patch text (memories `feedback_port_all_c_logic`, `feedback_no_corner_cuts_on_c`, `feedback_c_first_then_idiomatic`).
- End every git commit message with the two-line footer:
  ```
  Generated with AI

  Co-Authored-By: SLAC AI
  ```
- Never push; never post to remote (memory / global instructions).

## Commit-ordering rule (Group A)

Each Group-A behavior change and its regenerated golden(s) land in the **same commit**, so tier-2 regression never breaks mid-sequence. Where a behavior change flips an existing Rust unit test that locked the old (quirk) value, that test edit is in the same commit too.

---

## Task 0: Rebuild the reference oracle at 50c2b21b

**Files:**
- No repo files change. Produces build artifacts under `reference/starlink-ast/build-ast/`.

**Interfaces:**
- Produces: a cmake-built reference `libast` and the `wcsconverter` + test binaries used for oracle verification in later tasks.

- [ ] **Step 1: Ensure the submodule is checked out at the synced commit**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git submodule update --init reference/starlink-ast
git -C reference/starlink-ast rev-parse HEAD
```
Expected: prints `50c2b21b...` (matches the committed submodule pointer).

- [ ] **Step 2: Configure and build the oracle with cmake**

Run:
```bash
cmake -B reference/starlink-ast/build-ast -S reference/starlink-ast
cmake --build reference/starlink-ast/build-ast --target ast -j
```
Expected: builds `libast` without error. (If a specific target name differs, list targets with `cmake --build reference/starlink-ast/build-ast --target help`.)

- [ ] **Step 3: Build the test binaries used later**

Run:
```bash
cmake --build reference/starlink-ast/build-ast --target wcsconverter -j
```
Expected: `wcsconverter` binary built. This is the oracle for the regenerated FITS goldens.

- [ ] **Step 4: No commit** — build artifacts are git-ignored; nothing to commit here.

---

## Task 1: SpecMap — remove f32 casts on RA/Dec

**Upstream fix:** `409f1311`. **Quirk doc:** "SpecMap velocity routines cast RefRA/RefDec to float".

**Files:**
- Modify: `crates/starlink-ast/src/mappings/specmap.rs:600-644` (helpers `lsrk_vel`, `lsrd_vel`, `lg_vel`, `gal_vel` + the block comment above them)
- Modify (goldens): `crates/starlink-ast/tests/fixtures/wcsconv/expected/class-arc.fits-class`, `class-olddate.fits-class`, `class-sin.fits-class`, `skyspec3d-class.fits-class`, `vrad-v2f.native`, `jan.ast`
- Test: `crates/starlink-ast/tests/test_wcsconv.rs` (existing regression driver — no new test file)

**Interfaces:**
- Produces: `lsrk_vel(ra: f64, dec: f64) -> f64`, `lsrd_vel(ra, dec) -> f64`, `lg_vel(ra, dec) -> f64`, `gal_vel(ra, dec) -> f64` — signatures unchanged; only the body drops the `as f32 as f64` truncation.

- [ ] **Step 1: Confirm the failing goldens before the change**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
diff crates/starlink-ast/tests/fixtures/wcsconv/expected/class-arc.fits-class reference/starlink-ast/ast_tester/class-arc.fits-class
```
Expected: shows `VELO-LSR`/`VLSR` differ (`6876.2765` ours vs `6876.2752` upstream) — confirming our golden carries the old buggy value.

- [ ] **Step 2: Edit the four helpers to pass f64 straight through**

In `crates/starlink-ast/src/mappings/specmap.rs`, replace the four helper bodies:

```rust
/// Velocity of the kinematic LSR away from the source (m/s, heliocentric).
///
/// Mirrors `specmap.c` `LsrkVel`.
pub(crate) fn lsrk_vel(ra: f64, dec: f64) -> f64 {
    -1000.0 * pal::rvlsrk(ra, dec)
}

/// Velocity of the dynamical LSR away from the source (m/s, heliocentric).
///
/// Mirrors `specmap.c` `LsrdVel`.
pub(crate) fn lsrd_vel(ra: f64, dec: f64) -> f64 {
    -1000.0 * pal::rvlsrd(ra, dec)
}

/// Velocity of the Local Group away from the source (m/s, heliocentric).
///
/// Mirrors `specmap.c` `LgVel`.
pub(crate) fn lg_vel(ra: f64, dec: f64) -> f64 {
    -1000.0 * pal::rvlg(ra, dec)
}

/// Velocity of the galactic centre away from the source (m/s, heliocentric).
///
/// Mirrors `specmap.c` `GalVel`.
pub(crate) fn gal_vel(ra: f64, dec: f64) -> f64 {
    let s1 = pal::rvlsrd(ra, dec);
    let s2 = pal::rvgalc(ra, dec);
    -1000.0 * (s1 + s2)
}
```

(Keep whatever exact line-number references the existing doc comments carry; only the bodies and the truncation comment need to change.)

- [ ] **Step 3: Replace the stale block comment above the helpers**

Replace the multi-line comment (currently starting "`LsrdVel`, `LgVel`, `GalVel`. C casts the f64 RA/Dec…") with:

```rust
// LSRK/LSRD/LG/Galactic velocity helpers. These pass the f64 RA/Dec
// straight to the double-precision `palRv*` routines. Upstream C AST
// (specmap.c, commit 409f1311) removed the historical `(float)` casts
// that truncated ~20 mas of precision; the Rust port matches that.
```

- [ ] **Step 4: Copy the six regenerated goldens from the oracle tree**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
for f in class-arc.fits-class class-olddate.fits-class class-sin.fits-class skyspec3d-class.fits-class vrad-v2f.native jan.ast; do
  cp "reference/starlink-ast/ast_tester/$f" "crates/starlink-ast/tests/fixtures/wcsconv/expected/$f"
done
```
Then verify each changed line is only the expected VELO-LSR/VLSR/velocity drift (not unrelated content):
```bash
git diff --stat crates/starlink-ast/tests/fixtures/wcsconv/expected/
git diff crates/starlink-ast/tests/fixtures/wcsconv/expected/class-arc.fits-class
```
Expected: only spectral-velocity numeric fields changed.

- [ ] **Step 5: Run the wcsconv regression to verify Rust now matches**

Run:
```bash
cargo test -p starlink-ast --test test_wcsconv 2>&1 | tail -20
```
Expected: PASS — the class/jan/vrad fixtures now agree with the regenerated goldens.

- [ ] **Step 6: Format, clippy, commit**

Run:
```bash
cargo fmt
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
git add crates/starlink-ast/src/mappings/specmap.rs crates/starlink-ast/tests/fixtures/wcsconv/expected/
git commit -m "$(cat <<'EOF'
specmap: drop f32 casts on RA/Dec in velocity helpers (match fixed C)

Upstream 409f1311 removed the historical (float) casts on the RA/Dec
arguments to palRvlsrk/palRvlsrd/palRvlg/palRvgalc. Match that: pass f64
straight through. Regenerates the FITS-CLASS/native goldens (class-arc,
class-olddate, class-sin, skyspec3d-class, vrad-v2f, jan) toward the
full-precision VELO-LSR/VLSR values.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 2: VELOSYS — full-precision RefRA/RefDec (drop lossy round-trip)

**Upstream fix:** `ad04291c`. **Quirk doc:** "VELOSYS drift via lossy RefRA/RefDec round-trip".

**Background:** C now bumps the SkyFrame `Digits` to 20 before the `astFormat`/`astSetC` round-trip, making the string conversion effectively lossless. The Rust mirror deliberately reproduced the *lossy* default-precision round-trip. Match fixed C by using the full-precision `reflon`/`reflat` values directly (still normalized to AST's canonical ranges), which is what the Digits=20 round-trip yields.

**Files:**
- Modify: `crates/starlink-ast/src/fitschan/fset_to_store/mod.rs:482-538`
- Modify (golden): `crates/starlink-ast/tests/fixtures/wcsconv/expected/longslit.fits-wcs`
- Test: `crates/starlink-ast/tests/test_wcsconv.rs` (existing driver)

**Interfaces:**
- Consumes: `reflon: f64`, `reflat: f64` (from the earlier `fid` binding at line 477); `normalise_ra`, `normalise_dec` from `crate::frames::skyaxis`.
- Produces: no signature change; `spec.set_ref_ra` / `spec.set_ref_dec` now receive full-precision normalized values.

- [ ] **Step 1: Confirm the failing golden before the change**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
diff crates/starlink-ast/tests/fixtures/wcsconv/expected/longslit.fits-wcs reference/starlink-ast/ast_tester/longslit.fits-wcs
```
Expected: `VELOSYS` differs (`-20038.807` ours vs `-20038.808` upstream).

- [ ] **Step 2: Replace the lossy format/unformat block with full-precision normalize**

In `crates/starlink-ast/src/fitschan/fset_to_store/mod.rs`, replace the block from the `// C uses \`astSetC(tfs, "RefRA"...` comment through the `let (reflon_quant, reflat_quant) = { ... };` binding (lines ~488-530) with:

```rust
    // C uses `astSetC(tfs, "RefRA", astFormat(tfs, ilon, reflon))`, but
    // upstream (fitschan.c, commit ad04291c) temporarily raises the
    // SkyFrame's Digits to 20 before the format/unformat round-trip so no
    // precision is lost. The net effect is that the SpecFrame receives the
    // full-precision fiducial position. We therefore set the values
    // directly, normalised to AST's canonical ranges: RA in `[0, 2π)` (via
    // `eraAnp`), Dec in `(-π, π]` (via `eraAnpm`). Without the normalise
    // the SlaMap output's `(-π, π]` RA gives a different f64 than C's
    // `[0, 2π)`, perturbing the downstream VELOSYS.
    use crate::frames::skyaxis::{normalise_dec, normalise_ra};
    let (reflon_quant, reflat_quant) = (normalise_ra(reflon), normalise_dec(reflat));
```

Leave the `owned.with_integrity(|cur| { ... set_ref_ra/set_ref_dec ... })?;` block that follows unchanged.

- [ ] **Step 3: Copy the regenerated longslit golden**

Run:
```bash
cp reference/starlink-ast/ast_tester/longslit.fits-wcs crates/starlink-ast/tests/fixtures/wcsconv/expected/longslit.fits-wcs
git diff crates/starlink-ast/tests/fixtures/wcsconv/expected/longslit.fits-wcs
```
Expected: only the `VELOSYS` line changed (`-20038.807` → `-20038.808`).

- [ ] **Step 4: Run the wcsconv regression**

Run:
```bash
cargo test -p starlink-ast --test test_wcsconv 2>&1 | tail -20
```
Expected: PASS — `longslit` now agrees; no other fixture regressed.

- [ ] **Step 5: Format, clippy, commit**

Run:
```bash
cargo fmt
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
git add crates/starlink-ast/src/fitschan/fset_to_store/mod.rs crates/starlink-ast/tests/fixtures/wcsconv/expected/longslit.fits-wcs
git commit -m "$(cat <<'EOF'
fitschan: set RefRA/RefDec at full precision (match fixed C)

Upstream ad04291c raises SkyFrame Digits to 20 around the RefRA/RefDec
format round-trip so no precision is lost. Drop the Rust mirror of the
lossy default-precision round-trip and set the normalised full-precision
fiducial directly. Regenerates the longslit VELOSYS golden.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 3: Frame::ax_angle — fix stale loop index

**Upstream fix:** `da9ddb39`. **Quirk doc:** "astAxAngle nudge reads a stale loop index".

**Files:**
- Modify: `crates/starlink-ast/src/frames/mod.rs:670-676` (the `else if b[k] != 0.0` branch of `ax_angle`)
- Modify (test): `crates/starlink-ast/tests/test_frame_geometry.rs:63-79` (`ax_angle_cartesian`)

**Interfaces:**
- Produces: `ax_angle(&self, a: &[f64], b: &[f64], axis: usize) -> AstResult<f64>` — signature unchanged; behavior corrected for the `a[axis-1]==0 && b[axis-1]!=0` case.

- [ ] **Step 1: Update the existing test to the corrected (fixed-C) expectations**

In `crates/starlink-ast/tests/test_frame_geometry.rs`, replace the `ax_angle_cartesian` body's first assertion and comment:

```rust
fn ax_angle_cartesian() {
    let f = bf(2);
    // Oracle-confirmed against fixed C (astAxAngle, libast at 50c2b21b,
    // commit da9ddb39). For a=(0,0), b=(1,0), axis=1 the nudge is scaled
    // by |b[axis-1]| = |b[0]| = 1, so the offset point is well separated
    // from `a` and the angle is defined (0 radians: the offset lies along
    // +axis1 from a, and the direction a->b is also +axis1).
    let ang1 = f.ax_angle(&[0.0, 0.0], &[1.0, 0.0], 1).unwrap();
    assert!(ang1 != BAD_VALUE, "fixed C returns a valid angle, not BAD");
    assert!(ang1.abs() < 1e-6, "expected 0 rad, got {ang1}");
    // axis=2 takes the mxab fallback: PI/2.
    let ang2 = f.ax_angle(&[0.0, 0.0], &[1.0, 0.0], 2).unwrap();
    assert!((ang2 - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
    // a=(0,5), b=(3,2), axis=1: nudge by |b[0]|=3, angle is PI/4
    // (oracle: 0.78539816339744828).
    let ang3 = f.ax_angle(&[0.0, 5.0], &[3.0, 2.0], 1).unwrap();
    assert!((ang3 - std::f64::consts::FRAC_PI_4).abs() < 1e-6);
}
```

Note on `ang1`: verify the exact oracle value in Step 4; the design expects a valid (non-BAD) angle. If the oracle returns a value other than 0, set the assertion to the oracle value (do not keep BAD).

- [ ] **Step 2: Run the test to confirm it fails against current (quirk) code**

Run:
```bash
cargo test -p starlink-ast --test test_frame_geometry ax_angle_cartesian 2>&1 | tail -15
```
Expected: FAIL — current code returns `BAD_VALUE` for `ang1`.

- [ ] **Step 3: Fix the branch in `ax_angle`**

In `crates/starlink-ast/src/frames/mod.rs`, replace the `else if b[k] != 0.0` branch:

```rust
        } else if b[k] != 0.0 {
            // Upstream da9ddb39 fixed the stale-index bug: the nudge is
            // scaled by |b[axis-1]|, not |b[naxes-1]|.
            aa[k] = eps * b[k].abs();
```

- [ ] **Step 4: Verify against the rebuilt oracle (optional confirm of ang1)**

Build a tiny probe or reuse `wcsconverter`? `ax_angle` has no wcsconverter path, so verify by reading the ported C test values: the upstream `testframe.c` asserts `ang1 != AST__BAD` and `ang2 == PI/4` for `(0,5)->(3,2)`. Our `ang3` matches that. For `ang1` value confirmation, compile a 20-line C probe against `reference/starlink-ast/build-ast` libast calling `astAxAngle(astFrame(2," "),{0,0},{1,0},1)` and print the result (pattern: memory `reference_c_oracle_reproducer`). Set the Step-1 `ang1` assertion to that printed value.

- [ ] **Step 5: Run the test to confirm it passes**

Run:
```bash
cargo test -p starlink-ast --test test_frame_geometry ax_angle_cartesian 2>&1 | tail -10
```
Expected: PASS.

- [ ] **Step 6: Format, clippy, commit**

Run:
```bash
cargo fmt
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
git add crates/starlink-ast/src/frames/mod.rs crates/starlink-ast/tests/test_frame_geometry.rs
git commit -m "$(cat <<'EOF'
frame: fix ax_angle stale loop index (match fixed C)

Upstream da9ddb39: the near-axis-aligned nudge is scaled by |b[axis-1]|,
not the stale |b[naxes-1]|. Corrects ax_angle for a[axis-1]==0 &&
b[axis-1]!=0; updates the ported test to the fixed-C expectations.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 4: astRate — fix x0=0 seed for narrow-domain Mappings

**Upstream fix:** `e782c220`. **Quirk doc:** "astRate returns BAD at x0=0 for domain-restricted Mappings".

**Files:**
- Modify: `crates/starlink-ast/src/mappings/rate.rs:42-47` (the `h0` seed)
- Modify (tests): `crates/starlink-ast/tests/test_chebymap.rs:770-796` (`chebymap_rate`), `crates/starlink-ast/tests/test_numeric_rate.rs:43-48` (`numeric_rate_at_zero`) and `:51-73` (`numeric_rate_tan_at_zero_matches_c_artifact`)

**Interfaces:**
- Produces: `numeric_rate(mapping, at, ax1, ax2) -> AstResult<f64>` — unchanged signature; the zero-`x0` seed changes from `1.0` to `f64::EPSILON * 1e9`.

- [ ] **Step 1: Update `chebymap_rate` to expect the true gradient at x0=0**

In `crates/starlink-ast/tests/test_chebymap.rs`, replace the x=0 assertion block:

```rust
    // At x=0 the derivative of f(x)=x is 1.0. Upstream e782c220 seeds the
    // search interval with a small step (not 1.0) so the sample points
    // stay inside the ChebyMap domain [-1,1]; astRate now returns the true
    // gradient at x0=0 instead of BAD.
    let rate = m.rate(&[0.0], 1, 1).unwrap();
    assert!(
        (rate - 1.0).abs() < 1e-6,
        "expected rate ~1.0 at x=0, got {rate}"
    );
```

- [ ] **Step 2: Update `numeric_rate_tan_at_zero_matches_c_artifact`**

This test locks the OLD C artifact (`-1.9950849488779807e-4`) that arose from the `h0=1.0` seed. With the fixed seed, `d/dx tan(x)` at 0 converges to `1.0`. Replace the test:

```rust
#[test]
fn numeric_rate_tan_at_zero() {
    // d/dx tan(x) at x=0 is 1.0. Upstream e782c220 fixed the x0=0 seed so
    // the numeric rate converges to the true derivative (previously the
    // h0=1.0 seed produced a spurious small negative value).
    let m = MathMap::new(1, 1, &["y = tan(x)"], &["x = atan(y)"]).unwrap();
    let rate = m.rate(&[0.0], 1, 1).unwrap();
    assert!(
        (rate - 1.0).abs() < 1e-6,
        "expected ~1.0 at x=0, got {rate}"
    );

    // Away from zero the same Mapping converges to 1/cos^2(1) = 3.4255188224.
    let rate1 = m.rate(&[1.0], 1, 1).unwrap();
    assert!(
        (rate1 - 3.4255188224).abs() < 1e-6,
        "expected ~3.4255188224 at x=1, got {rate1}"
    );
}
```

(`numeric_rate_at_zero` for the ZoomMap already expects `5.0` and stays correct — the linear map's gradient is domain-independent — so it needs no change. Confirm in Step 5.)

- [ ] **Step 3: Run the tests to confirm they fail against current code**

Run:
```bash
cargo test -p starlink-ast --test test_chebymap chebymap_rate 2>&1 | tail -10
cargo test -p starlink-ast --test test_numeric_rate numeric_rate_tan_at_zero 2>&1 | tail -10
```
Expected: FAIL — current code returns `BAD_VALUE` (chebymap) / the negative artifact (tan).

- [ ] **Step 4: Fix the seed in `rate.rs`**

In `crates/starlink-ast/src/mappings/rate.rs`, replace the `h0` seed:

```rust
    // Initial interval size (mapping.c Rate, commit e782c220): a small
    // fraction of |x0|, or the same small absolute step when x0 is zero.
    // Seeding with 1.0 for x0==0 pushed the first sampled interval outside
    // the domain of narrow-domain Mappings (e.g. a ChebyMap on [-1,1]),
    // making astRate return BAD at x0=0. The loop below grows the interval
    // as needed for both restricted and unrestricted Mappings.
    let mut h0 = if x0 != 0.0 {
        f64::EPSILON * 1e9 * x0.abs()
    } else {
        f64::EPSILON * 1e9
    };
```

- [ ] **Step 5: Run the affected tests + the full rate/chebymap suites**

Run:
```bash
cargo test -p starlink-ast --test test_numeric_rate 2>&1 | tail -15
cargo test -p starlink-ast --test test_chebymap 2>&1 | tail -15
```
Expected: PASS. If any other test in these files asserted an x0=0 BAD/artifact value, update it to the true-gradient value in this same task (search: `grep -n "x0\|at_zero\|BAD" crates/starlink-ast/tests/test_numeric_rate.rs`).

- [ ] **Step 6: Run the full starlink-ast test suite to catch downstream effects**

Run:
```bash
cargo test -p starlink-ast 2>&1 | tail -25
```
Expected: PASS. The design notes FELO/VOPT spectral CDELT could shift by ~1e-7 in CmpMap rate paths — but that path uses whole-map numeric rate (Bucket 3, unchanged) so wcsconv should stay green. If a wcsconv fixture regresses here, STOP and raise it (it would mean a fixed-C golden also changed — check `reference/starlink-ast/ast_tester/` for that fixture).

- [ ] **Step 7: Format, clippy, commit**

Run:
```bash
cargo fmt
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
git add crates/starlink-ast/src/mappings/rate.rs crates/starlink-ast/tests/test_chebymap.rs crates/starlink-ast/tests/test_numeric_rate.rs
git commit -m "$(cat <<'EOF'
mapping: fix astRate x0=0 seed for narrow-domain Mappings (match fixed C)

Upstream e782c220 seeds the interval search with a small absolute step
when x0 is zero, instead of 1.0, so the sample points stay inside a
narrow domain (e.g. ChebyMap on [-1,1]). astRate now returns the true
gradient at x0=0. Updates the ChebyMap and tan rate tests accordingly.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 5: astRate — fix zero-range stale-slot index

**Upstream fix:** `c1a88658`. **Quirk doc:** "astRate zero-range confirmation reads a stale array slot".

**Files:**
- Modify: `crates/starlink-ast/src/mappings/rate.rs:99` and `:141` (the two zero-range confirmation branches)

**Interfaces:**
- Produces: `numeric_rate(...)` — unchanged signature; the two `ranges[iin ± 1]` reads become `ranges[itop - 1]` / `ranges[ibot + 1]` (store indices).

- [ ] **Step 1: Fix the upward-search branch (line ~99)**

In `crates/starlink-ast/src/mappings/rate.rs`, replace:

```rust
        } else if range == 0.0 && ranges[iin - 1] == 0.0 {
```
with:
```rust
        } else if range == 0.0 && ranges[itop - 1] == 0.0 {
            // Upstream c1a88658: index the range array by the store index
            // `itop`, not the loop counter `iin`. They diverge when an
            // interval was skipped (FindGradient failed), so `iin` could
            // read a slot never written for this search.
```
Keep the existing `iret = itop; break;` body; remove the old stale-slot comment lines.

- [ ] **Step 2: Fix the downward-search branch (line ~141)**

Replace:
```rust
        } else if range == 0.0 && ranges[iin + 1] == 0.0 {
```
with:
```rust
        } else if range == 0.0 && ranges[ibot + 1] == 0.0 {
            // Upstream c1a88658: index by the store index `ibot`, not the
            // loop counter `iin` (see upward-search branch).
```
Keep the existing `iret = ibot; break;` body; remove the old stale-slot comment.

- [ ] **Step 3: Verify the loop-variable `iin` is still used elsewhere**

Run:
```bash
grep -n "iin" crates/starlink-ast/src/mappings/rate.rs
```
Expected: `iin` still drives the `for` ranges. If the two edits leave `iin` unused inside a loop body, clippy would warn — the `for iin in ...` header still binds it for `h *= 4.0` iteration count, so it remains used. If clippy flags an unused binding, rename the loop header to `for _ in ...` only where truly unused (verify first).

- [ ] **Step 4: Run the rate suites**

Run:
```bash
cargo test -p starlink-ast --test test_numeric_rate 2>&1 | tail -10
cargo test -p starlink-ast --test test_chebymap 2>&1 | tail -10
cargo test -p starlink-ast 2>&1 | tail -15
```
Expected: PASS. This branch is only reached for smooth Mappings where `iin`/store index coincide, so no test value should change; this is a latent-correctness alignment.

- [ ] **Step 5: Format, clippy, commit**

Run:
```bash
cargo fmt
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
git add crates/starlink-ast/src/mappings/rate.rs
git commit -m "$(cat <<'EOF'
mapping: fix astRate zero-range stale-slot index (match fixed C)

Upstream c1a88658: the zero-range confirmation now indexes the range
array by the store index (itop/ibot), not the loop counter iin, which
diverge when an interval is skipped after a FindGradient failure.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 6: PermMap::equal — compare constant values on matching negative indices

**Upstream fix:** `0fbd9cc3`. **Quirk doc:** "PermMap::Equal skips constant-value comparison for matching indices".

**Files:**
- Modify: `crates/starlink-ast/src/mappings/perm.rs:958-973` (the `entry_eq` closure)
- Modify (test): `crates/starlink-ast/tests/test_permmap.rs:175-183` (`perm_equal_same_constant_index_ignores_value_quirk`)

**Interfaces:**
- Produces: `PermMap::equal(&self, other: &dyn Mapping) -> bool` — unchanged signature; two PermMaps feeding an axis from the same constant *index* now compare unequal if the stored *values* differ.

- [ ] **Step 1: Rewrite the quirk test to the fixed-C expectation**

In `crates/starlink-ast/tests/test_permmap.rs`, replace `perm_equal_same_constant_index_ignores_value_quirk` (name and body):

```rust
#[test]
fn perm_equal_same_constant_index_differing_values() {
    // Both inputs are fed by constants[0]. Upstream 0fbd9cc3 compares the
    // referenced constant *values* whenever both entries are negative, so
    // PermMaps storing different values (5.0 vs 7.0) compare UNEQUAL even
    // though the constant index matches. Oracle: astEqual == 0.
    let a = PermMap::new(&[-1], &[1], &[5.0]).unwrap();
    let b = PermMap::new(&[-1], &[1], &[7.0]).unwrap();
    assert!(!a.equal(&b));
}
```

(The other three permmap-equal tests — different index/same value, different index/different value, unreferenced surplus — already match fixed C; leave them.)

- [ ] **Step 2: Add a same-index/same-value positive case**

Immediately after the test above, add:

```rust
#[test]
fn perm_equal_same_constant_index_same_value() {
    // Same index (-1), identical stored value -> equal. Oracle: == 1.
    let a = PermMap::new(&[-1], &[1], &[5.0]).unwrap();
    let b = PermMap::new(&[-1], &[1], &[5.0]).unwrap();
    assert!(a.equal(&b));
}
```

- [ ] **Step 3: Run the tests to confirm the differing-values one fails against current code**

Run:
```bash
cargo test -p starlink-ast --test test_permmap perm_equal 2>&1 | tail -15
```
Expected: FAIL on `perm_equal_same_constant_index_differing_values` (current code returns `true` via the `p1 == p2` short-circuit).

- [ ] **Step 4: Fix the `entry_eq` closure**

In `crates/starlink-ast/src/mappings/perm.rs`, replace the `entry_eq` closure so that whenever both entries are negative (constants) the referenced values are compared — even when `p1 == p2`. Mirror the fixed-C structure ("if either >= 0: unequal unless equal; else compare constant values"):

```rust
        // Compare one input/output position. Mirrors fixed C
        // permmap.c:361-386 (commit 0fbd9cc3): if either entry is an axis
        // reference (>= 0) the entries must be identical; otherwise both
        // feed a constant and the *referenced values* must be exactly
        // equal — even when both reference the same constant index. A
        // missing constant slot is treated as BAD (matching the forward
        // transformation's NULL-constants behaviour).
        let entry_eq = |p1: i64, p2: i64| -> bool {
            if p1 >= 0 || p2 >= 0 {
                return p1 == p2;
            }
            // Both constants: compare the referenced values exactly. An
            // out-of-range slot reads as BAD_VALUE.
            let c1 = self
                .constants
                .get((-p1 - 1) as usize)
                .copied()
                .unwrap_or(BAD_VALUE);
            let c2 = other
                .constants
                .get((-p2 - 1) as usize)
                .copied()
                .unwrap_or(BAD_VALUE);
            c1 == c2
        };
```

Ensure `BAD_VALUE` is in scope (it is imported at the top of `perm.rs` per the earlier grep: `use crate::constants::BAD_VALUE;`).

- [ ] **Step 5: Run the permmap tests**

Run:
```bash
cargo test -p starlink-ast --test test_permmap 2>&1 | tail -15
```
Expected: PASS (all four+new cases).

- [ ] **Step 6: Run the full suite (this affects simplify over-merge)**

Run:
```bash
cargo test -p starlink-ast 2>&1 | tail -20
```
Expected: PASS. The fix makes `equal` stricter (fewer merges); if a simplify regression fixture relied on the over-merge it would now fail — if so, that fixture's `.simp` will have changed upstream too; STOP and reconcile against `reference/starlink-ast/ast_tester/simplify_fixtures/`.

- [ ] **Step 7: Format, clippy, commit**

Run:
```bash
cargo fmt
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
git add crates/starlink-ast/src/mappings/perm.rs crates/starlink-ast/tests/test_permmap.rs
git commit -m "$(cat <<'EOF'
permmap: compare constant values on matching negative indices (fixed C)

Upstream 0fbd9cc3: PermMap::Equal now compares the referenced constant
values whenever both permutation entries are negative, so two PermMaps
feeding an axis from the same constant index but storing different values
compare unequal (preventing an astSimplify over-merge).

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 7: timemap ast_dat — fix pre-1972 inverse divisors and 1966 intercept

**Upstream fix:** `8be504ac`. **Quirk doc:** "astDat inverse-direction divisor wrong in pre-1972 rubber-second era".

**Files:**
- Modify: `crates/starlink-ast/src/mappings/timemap.rs:701-731` (the pre-1972 inverse branches)
- Test: `crates/starlink-ast/tests/test_timemap.rs` (add a round-trip test ported from `testtime.c`)

**Interfaces:**
- Produces: `ast_dat` inverse branch — same signature; divisors change from the literal `1.0xxxx` forms to `(1.0 + b/SPD)`, and the 1966 branch intercept fixes `4.2131700` → `4.3131700`.

- [ ] **Step 1: Add the ported round-trip test (expected to fail first)**

`ast_dat` is private (`fn ast_dat` in `timemap.rs`, not `pub(crate)`), so the integration test cannot call it directly. Port the `testtime.c` cases at the public `TimeMap` level, reusing the existing `transform_one` helper and the `UtcToTai`/`TaiToUtc` conversions already used by `utc_tai_round_trip` in this file. `transform_one(&tm, val, forward)` transforms one MJD; construct the map with `TimeCvtType::UtcToTai` / `TimeCvtType::TaiToUtc` and args `[0.0, BAD_VALUE]` (BAD triggers the automatic DTAI lookup, i.e. the `ast_dat` path).

Add to `crates/starlink-ast/tests/test_timemap.rs`:

```rust
#[test]
fn utc_tai_roundtrip_pre1972_eras() {
    use starlink_ast::constants::BAD_VALUE;
    // Ported from testtime.c (commit 8be504ac): one UTC MJD inside each of
    // the 14 pre-1972 "rubber second" eras. A UTC->TAI->UTC round trip must
    // return to the start to ~1e-9 d, and TAI-UTC at the test point must
    // match the published value for that era to ~1e-5 s. A units error in
    // the inverse divisor previously leaked ~1.9e-6 d; a wrong 1966
    // intercept leaked ~0.1 s at MJD 39500.
    let era_utc = [
        40000.0, 39500.0, 39060.0, 38970.0, 38880.0, 38790.0, 38700.0,
        38560.0, 38440.0, 38360.0, 38000.0, 37590.0, 37400.0, 37200.0,
    ];
    let era_dt = [
        6.478578, 5.282578, 4.227634, 4.010994, 3.794354, 3.577714, 3.361074,
        3.079634, 2.824114, 2.726482, 2.222130, 1.748658, 1.552418, 1.288218,
    ];

    let mut fwd = TimeMap::new();
    fwd.add(TimeCvtType::UtcToTai, &[0.0, BAD_VALUE]);
    let mut inv = TimeMap::new();
    inv.add(TimeCvtType::TaiToUtc, &[0.0, BAD_VALUE]);

    for (utc, dt) in era_utc.iter().zip(era_dt.iter()) {
        let tai = transform_one(&fwd, *utc, true); // UTC -> TAI (MJD)
        // TAI-UTC in seconds at this UTC.
        let tai_minus_utc = (tai - *utc) * 86400.0;
        assert!(
            (tai_minus_utc - dt).abs() < 1e-5,
            "TAI-UTC at MJD {utc}: got {tai_minus_utc}, expected {dt}"
        );
        let utc_back = transform_one(&inv, tai, true); // TAI -> UTC (MJD)
        assert!(
            (utc_back - *utc).abs() < 1e-9,
            "UTC->TAI->UTC round trip at MJD {utc}: got {utc_back}"
        );
    }
}
```

Verify the `transform_one` forward/`true` convention against the existing `tai_to_utc_at_j2000` test (it calls `transform_one(&tm, tai, true)` on a `TaiToUtc` map to get UTC) — that confirms `true` runs the map's forward direction, so a `TaiToUtc` map with `forward=true` yields UTC. Adjust if the local helper differs.

- [ ] **Step 2: Run the test to confirm it fails against current code**

Run:
```bash
cargo test -p starlink-ast --test test_timemap utc_tai_roundtrip_pre1972_eras 2>&1 | tail -15
```
Expected: FAIL — the round trip leaks ~1.9e-6 d (>1e-9) and the 1966-era offset is off by ~0.1 s.

- [ ] **Step 3: Fix the inverse divisors and the 1966 intercept**

In `crates/starlink-ast/src/mappings/timemap.rs`, in the inverse branch (lines ~701-731), change every pre-1972 divisor from the literal form to `(1.0 + b/SPD)`, and fix the 1966 branch intercept. Concretely:

```rust
        } else if mjd >= 39887.0 + (4.2131700 + (39887.0 - 39126.0) * 0.002592) / SPD {
            -(4.2131700 + (mjd - 39126.0) * 0.002592) / (1.0 + 0.002592 / SPD)
        } else if mjd >= 39126.0 + (4.3131700 + (39126.0 - 39126.0) * 0.002592) / SPD {
            -(4.3131700 + (mjd - 39126.0) * 0.002592) / (1.0 + 0.002592 / SPD)
        } else if mjd >= 39004.0 + (3.8401300 + (39004.0 - 38761.0) * 0.001296) / SPD {
            -(3.8401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38942.0 + (3.7401300 + (38942.0 - 38761.0) * 0.001296) / SPD {
            -(3.7401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38820.0 + (3.6401300 + (38820.0 - 38761.0) * 0.001296) / SPD {
            -(3.6401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38761.0 + (3.5401300 + (38761.0 - 38761.0) * 0.001296) / SPD {
            -(3.5401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38639.0 + (3.4401300 + (38639.0 - 38761.0) * 0.001296) / SPD {
            -(3.4401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38486.0 + (3.3401300 + (38486.0 - 38761.0) * 0.001296) / SPD {
            -(3.3401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38395.0 + (3.2401300 + (38395.0 - 38761.0) * 0.001296) / SPD {
            -(3.2401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38334.0 + (1.9458580 + (38334.0 - 37665.0) * 0.0011232) / SPD {
            -(1.9458580 + (mjd - 37665.0) * 0.0011232) / (1.0 + 0.0011232 / SPD)
        } else if mjd >= 37665.0 + (1.8458580 + (37665.0 - 37665.0) * 0.0011232) / SPD {
            -(1.8458580 + (mjd - 37665.0) * 0.0011232) / (1.0 + 0.0011232 / SPD)
        } else if mjd >= 37512.0 + (1.3728180 + (37512.0 - 37300.0) * 0.001296) / SPD {
            -(1.3728180 + (mjd - 37300.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 37300.0 + (1.4228180 + (37300.0 - 37300.0) * 0.001296) / SPD {
            -(1.4228180 + (mjd - 37300.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else {
            -(1.4178180 + (mjd - 37300.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        }
```

Two substantive corrections vs the current code: (1) the 1966 branch second constant `4.2131700` → `4.3131700`; (2) the 1965-July branch divisor `1.01296` → `(1.0 + 0.001296/SPD)` (the current code's anomalous `1.01296` differs from the sibling `1.001296` even). Verify each `else if` guard expression is unchanged (only the `result` line's divisor/intercept change), matching the upstream diff exactly.

- [ ] **Step 4: Run the ported test + full timemap suite**

Run:
```bash
cargo test -p starlink-ast --test test_timemap 2>&1 | tail -20
```
Expected: PASS.

- [ ] **Step 5: Run the full suite (timemap feeds fset_to_store::sky_sys)**

Run:
```bash
cargo test -p starlink-ast 2>&1 | tail -20
```
Expected: PASS. wcsconv `fk4_aips_*` fixtures use MJD-OBS 40000 but the AIPS/IRAF writers do not re-emit MJD-OBS (mirrored in enc_aips/enc_iraf), so no golden should change. If one does, STOP and reconcile.

- [ ] **Step 6: Format, clippy, commit**

Run:
```bash
cargo fmt
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
git add crates/starlink-ast/src/mappings/timemap.rs crates/starlink-ast/tests/test_timemap.rs
git commit -m "$(cat <<'EOF'
timemap: fix astDat pre-1972 inverse divisors and 1966 intercept (fixed C)

Upstream 8be504ac: the rubber-second-era inverse (TAI->UTC) divisor is
(1 + b/SPD), not the (1 + b)/(1 + 10b) literals; the 1966 inverse branch
intercept is 4.3131700, matching its forward branch. Adds the ported
testtime round-trip across all 14 eras.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 8: Port discrete new C-test assertions (PermMap already done; add remaining)

Most Bucket-1 C-test assertions were ported inline with their fixes (Tasks 3, 4, 6, 7). This task adds the two that were verification-only and confirms coverage.

**Files:**
- Modify: `crates/starlink-ast/tests/test_permmap.rs` — already covered in Task 6. No-op here; verify.
- Verify only: `test_frame_geometry.rs` (Task 3), `test_chebymap.rs`/`test_numeric_rate.rs` (Task 4), `test_timemap.rs` (Task 7).

- [ ] **Step 1: Confirm each ported C assertion has a Rust counterpart**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
grep -l "ax_angle" crates/starlink-ast/tests/test_frame_geometry.rs
grep -l "chebymap_rate\|numeric_rate_tan_at_zero" crates/starlink-ast/tests/test_chebymap.rs crates/starlink-ast/tests/test_numeric_rate.rs
grep -l "perm_equal_same_constant_index" crates/starlink-ast/tests/test_permmap.rs
grep -l "utc_tai_roundtrip_pre1972_eras" crates/starlink-ast/tests/test_timemap.rs
```
Expected: each grep prints its file — all four discrete C tests have Rust counterparts.

- [ ] **Step 2: Run the four suites together**

Run:
```bash
cargo test -p starlink-ast --test test_frame_geometry --test test_chebymap --test test_numeric_rate --test test_permmap --test test_timemap 2>&1 | tail -20
```
Expected: PASS.

- [ ] **Step 3: No commit** — this task only verifies; if a gap is found, add the missing test in the relevant task's file and commit there.

---

## Task 9: Update the quirks doc — mark Bucket 1 & 2 resolved

**Files:**
- Modify: `docs/issues/c-library-quirks.md`

**Interfaces:** documentation only.

- [ ] **Step 1: Mark each resolved Bucket-1 entry**

For each of these entries, prepend the heading with `RESOLVED upstream` and add a one-line note under the heading giving the fixing commit and the Rust task. Do this for:
- "VELOSYS drift via lossy RefRA/RefDec round-trip" → `ad04291c`
- "SpecMap velocity routines cast RefRA/RefDec to float" → `409f1311`
- "astAxAngle nudge reads a stale loop index" → `da9ddb39`
- "astRate returns BAD at x0=0 for domain-restricted Mappings" → `e782c220`
- "astRate zero-range confirmation reads a stale array slot" → `c1a88658`
- "PermMap::Equal skips constant-value comparison for matching indices" → `0fbd9cc3`
- "astDat inverse-direction divisor wrong in pre-1972 rubber-second era" → `8be504ac`

Use this exact pattern (example for one):

```markdown
## RESOLVED (upstream 409f1311): SpecMap velocity routines cast RefRA/RefDec to float

**Status:** Fixed upstream at commit 409f1311 and matched in the Rust port
(the `as f32 as f64` truncations were removed from `specmap.rs`; goldens
regenerated). The historical discussion below is retained for context.

<existing body unchanged>
```

- [ ] **Step 2: Mark each Bucket-2 entry resolved (behavior already correct in Rust)**

For these, add a `RESOLVED` note stating the Rust port already had the correct behavior and the C side is now fixed too:
- "MatrixMap MapSplit: uninitialized pmat advance on BAD entries" → `c9ce955a`
- "ScalePolyInputs: stale pointer in inverse-coefficient branch" → `f4c019d4`
- "SIPIntWorld reads nout from astGetNin (typo bug)" → `232643bc`
- "CLASSFromStore: spectral CDELT multiplied by specfactor twice" → `17416d33`
- "SpecFluxFrame astEqual reports identical frames as unequal" → `a3b9468c`
- "DATE-OBS fractional-seconds parser drops leading zeros" → `254d4c99`

- [ ] **Step 3: Leave Bucket-3 entries unchanged**

Confirm these two entries are NOT marked resolved (upstream did not fix them):
- "CmpMap Rate chain-rule override is defined but never installed"
- "Frame::unformat numeric token: Rust f64::parse vs C strtod"

Run:
```bash
grep -n "^## " docs/issues/c-library-quirks.md | grep -iv resolved
```
Expected: the two Bucket-3 headings (plus the already-withdrawn TimeFrame one and the FrameSet-variants divergence) remain un-"RESOLVED".

- [ ] **Step 4: Commit**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
git add docs/issues/c-library-quirks.md
git commit -m "$(cat <<'EOF'
docs: mark quirks resolved by the 50c2b21b upstream sync

Bucket 1 (workaround removed to match fixed C) and Bucket 2 (Rust was
already correct) entries are marked RESOLVED with their fixing commits.
CmpMap Rate and Frame::unformat strtod remain open (not fixed upstream).

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 10: Import the new simplify fixtures (files only)

**Files:**
- Create: ~164 new `.map` + partner `.simp` files under `crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures/`

**Interfaces:**
- Produces: the fixture files on disk, ready for the manifest wiring in Task 11.

- [ ] **Step 1: Copy every new fixture not already present**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
SRC=reference/starlink-ast/ast_tester/simplify_fixtures
DST=crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures
for f in "$SRC"/*.map "$SRC"/*.simp; do
  base=$(basename "$f")
  if [ ! -e "$DST/$base" ]; then cp "$f" "$DST/$base"; fi
done
```

- [ ] **Step 2: Verify the counts now match upstream**

Run:
```bash
echo "ours .map:  $(ls crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures/*.map | wc -l)"
echo "up   .map:  $(ls reference/starlink-ast/ast_tester/simplify_fixtures/*.map | wc -l)"
echo "ours .simp: $(ls crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures/*.simp | wc -l)"
echo "up   .simp: $(ls reference/starlink-ast/ast_tester/simplify_fixtures/*.simp | wc -l)"
```
Expected: ours ≥ upstream for both (ours = upstream + our 6 local-only `neg_*` .map). `.map`: ours 425 vs up 419; `.simp`: ours 276 vs up 276 (adjust if local-only fixtures also have `.simp`).

- [ ] **Step 3: Confirm the 6 local-only fixtures are still present**

Run:
```bash
comm -23 <(ls crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures/*.map | xargs -n1 basename | sort) <(ls reference/starlink-ast/ast_tester/simplify_fixtures/*.map | xargs -n1 basename | sort)
```
Expected: lists exactly the 6 `neg_*` fixtures (`neg_cmpmap_mode_mismatch`, `neg_cmpmap_neighbour_not_cmpmap`, `neg_poly_different_coeffs`, `neg_time_2arg_mismatch`, `neg_unitnormmap_nonunit_scale`, `neg_win_series_cmpmap_neighbour`).

- [ ] **Step 4: Commit the fixture files (before wiring, so the wiring diff is readable)**

Run:
```bash
git add crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures/
git commit -m "$(cat <<'EOF'
test: import new astSimplify fixtures from upstream 50c2b21b

Copies the ~164 new .map/.simp fixtures added upstream (simplify coverage
sweep). Manifest wiring and failure triage follow in subsequent commits.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 11: Wire the manifest + tag unported-class fixtures

**Files:**
- Modify: `crates/starlink-ast/tests/fixtures/native/reference/simplify_tests.txt`

**Interfaces:**
- Consumes: the fixture files from Task 10.
- Produces: manifest entries `name | input | reference | [skip]`; unported-class fixtures preceded by a `#`-comment block.

- [ ] **Step 1: Identify the 15 unported-class (IntraMap/XphMap) fixtures**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
grep -lE "IntraMap|XphMap" crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures/*.map | xargs -n1 basename | sort
```
Expected: 16 fixtures (15 new + any already present). Record this list — these get commented out in the manifest.

- [ ] **Step 2: Build the merged manifest**

Take upstream's `simplify_tests.txt` as the base (it has the correct entries + skip flags for the new fixtures), then append our 6 local-only entries. Preserve the existing header comments. For each fixture whose `.map` contains `IntraMap` or `XphMap`, prefix its manifest line with `# BLOCKED(IntraMap/XphMap not ported): `.

Generate it:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
DST=crates/starlink-ast/tests/fixtures/native/reference
# Fixtures to block (class not ported):
grep -lE "IntraMap|XphMap" "$DST"/simplify_fixtures/*.map | xargs -n1 basename | sed 's/\.map$//' | sort > /tmp/blocked_names.txt
wc -l /tmp/blocked_names.txt
```
Then hand-edit `simplify_tests.txt`: for each name in `/tmp/blocked_names.txt`, find its manifest line and prefix with the BLOCKED comment. Add a section header comment above the first blocked entry:
```
# --- Blocked: IntraMap / XphMap not yet ported (see PLAN.md). Re-enable
# --- when those Mapping classes land. ---
```

- [ ] **Step 3: Verify the manifest parses and references existing files**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
DST=crates/starlink-ast/tests/fixtures/native/reference
awk -F'|' '!/^#/ && NF>=3 { gsub(/ /,"",$2); print $2 }' "$DST/simplify_tests.txt" | while read p; do
  [ -e "$DST/$p" ] || echo "MISSING: $p"
done
```
Expected: no `MISSING` lines (every non-commented entry points to a real file).

- [ ] **Step 4: Confirm blocked fixtures are commented**

Run:
```bash
grep -c "BLOCKED" crates/starlink-ast/tests/fixtures/native/reference/simplify_tests.txt
```
Expected: matches the count in `/tmp/blocked_names.txt`.

- [ ] **Step 5: Commit the manifest**

Run:
```bash
git add crates/starlink-ast/tests/fixtures/native/reference/simplify_tests.txt
git commit -m "$(cat <<'EOF'
test: wire new simplify fixtures into the regression manifest

Merges upstream's simplify_tests.txt entries (with skip flags) plus our 6
local-only neg_* fixtures. IntraMap/XphMap fixtures are commented out with
a BLOCKED marker until those Mapping classes are ported.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 12: Extend the harness to separate skip reasons

**Files:**
- Modify: `crates/starlink-ast/tests/test_simplify_regression.rs` (the loop's skip accounting + the summary print)

**Interfaces:**
- Produces: the summary line distinguishes `skipped: unported class` from `skipped: unexpected`.

- [ ] **Step 1: Add a parse-failure vs missing-file distinction**

In `crates/starlink-ast/tests/test_simplify_regression.rs`, replace the single `skipped` counter with two: `skipped_missing` (file absent — expected for any not-yet-imported entry) and `skipped_parse` (present but `from_native_string` failed — an unported class or a genuine parser gap). At each `skipped += 1;` site, use the appropriate counter based on whether the failure was "file not found" vs "parse error". The existing structure already separates the `!input_path.exists()` check (→ `skipped_missing`) from the `from_native_string(...)` `Err` arm (→ `skipped_parse`).

- [ ] **Step 2: Collect the names of parse-skips for reporting**

Add a `let mut parse_skipped: Vec<String> = Vec::new();` and push `name` in the parse-error arms (input and reference), so the summary can list which fixtures were skipped for parse reasons (these should all be the BLOCKED IntraMap/XphMap set once the manifest is correct).

- [ ] **Step 3: Update the summary print**

Replace the summary `println!` to report both counters and list parse-skips:

```rust
    println!(
        "\nSimplify regression: {passed} passed, {skipped_missing} skipped (file absent), \
         {} skipped (parse — unported class or gap), {} equal() failures, \
         {} transform failures, {} serialization failures, {} other failures",
        parse_skipped.len(),
        equal_failures.len(),
        transform_failures.len(),
        serial_failures.len(),
        other_failures.len(),
    );
    if !parse_skipped.is_empty() {
        eprintln!("  parse-skipped (expected: IntraMap/XphMap not ported):");
        for n in &parse_skipped {
            eprintln!("    {n}");
        }
    }
```

- [ ] **Step 4: Run the regression to see the new summary**

Run:
```bash
cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | tail -40
```
Expected: compiles and runs; the summary shows the two skip categories. There will very likely be **failures** now (newly-wired supported-class fixtures) — that is expected and handled in Task 13. This step only validates the harness change compiles and the counters populate.

- [ ] **Step 5: Format, clippy, commit**

Run:
```bash
cargo fmt
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
git add crates/starlink-ast/tests/test_simplify_regression.rs
git commit -m "$(cat <<'EOF'
test: separate simplify skip reasons (file-absent vs parse/unported)

The regression summary now distinguishes fixtures skipped because the
file is absent from those skipped because parsing failed (unported class
or a genuine parser gap), and lists the parse-skips so a silent
regression cannot hide in the skip counter.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 13: Triage and fix all supported-class fixture failures

This is the open-ended task. Per the design decision, **all** supported-class failures must pass before landing; unported-class (IntraMap/XphMap) fixtures stay BLOCKED in the manifest. If a supported-class failure reveals a genuinely large simplifier gap, STOP and raise it rather than silently skipping.

**Files:**
- Modify: whichever `crates/starlink-ast/src/mappings/*.rs` / `regions/*.rs` the failures implicate (unknown until Step 1).
- Test: `crates/starlink-ast/tests/test_simplify_regression.rs` (driver; no per-fixture code).

- [ ] **Step 1: Capture the full failure list**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | tee /tmp/simplify_run.txt | tail -60
```
Read `/tmp/simplify_run.txt` and enumerate: equal() failures, transform failures, serialization mismatches, and parse-skips. Confirm every parse-skip is an IntraMap/XphMap BLOCKED fixture (if not, a supported fixture is failing to parse — a real parser gap to fix).

- [ ] **Step 2: Classify each failure**

For each failing fixture, determine the class and failure kind. Group by root cause (multiple fixtures often share one). For each group, build a standalone C reproducer against the rebuilt oracle to get the authoritative simplified form (pattern: memory `reference_c_oracle_reproducer`):
```bash
# Example skeleton — adapt per fixture:
cat > /tmp/repro.c <<'CEOF'
#include "ast.h"
#include <stdio.h>
int main(void){
  astBegin;
  AstChannel *ch = astChannel(NULL,NULL,"");
  /* read the .map, astSimplify, astShow the result */
  astEnd; return 0;
}
CEOF
# compile against reference/starlink-ast/build-ast libast + includes
```

- [ ] **Step 3: Fix one root-cause group at a time (TDD-style)**

For each group:
1. Confirm the fixture fails: `cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | grep <fixture-name>`.
2. Root-cause by comparing the Rust simplified output to the oracle's, mirroring C control flow in the implicated `map_merge` / `simplify` path (memories `feedback_c_logic_beats_existing_rust`, `feedback_no_corner_cuts_on_c`).
3. Fix the Rust source.
4. Re-run the specific fixture and the full regression to confirm no new breakage.
5. `cargo fmt && cargo clippy --workspace -- -D warnings`.
6. Commit with a message naming the class and the fixtures fixed:
   ```
   simplify: <class> — <what was fixed> (fixtures: a, b, c)

   Generated with AI

   Co-Authored-By: SLAC AI
   ```

- [ ] **Step 4: Repeat until zero supported-class failures**

Run after each group:
```bash
cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | tail -30
```
Expected end state: `0 equal() failures, 0 transform failures, 0 serialization failures, 0 other failures`; the only skips are file-absent (none, since all imported) and the BLOCKED IntraMap/XphMap parse-skips.

- [ ] **Step 5: Escalation checkpoint**

If any group turns out to be a large simplifier gap (e.g. a whole merge pathway not yet ported), STOP: write a short note in `docs/issues/` describing the gap and raise it with the user before either implementing a large new subsystem or marking the fixture BLOCKED. Do not silently skip a supported-class fixture.

---

## Task 14: Final full-workspace verification

**Files:** none (verification only).

- [ ] **Step 1: Full starlink-ast test suite**

Run:
```bash
cd /sdf/home/t/timj/WORK/starlink-astrs
cargo test -p starlink-ast 2>&1 | tail -30
```
Expected: all green.

- [ ] **Step 2: Regression tier + wcsconv**

Run:
```bash
cargo test -p starlink-ast --test '*-regression' 2>&1 | tail -20
cargo test -p starlink-ast --test test_wcsconv 2>&1 | tail -20
```
Expected: all green (goldens updated in Tasks 1-2).

- [ ] **Step 3: Workspace build, fmt, clippy**

Run:
```bash
cargo build --workspace 2>&1 | tail -5
cargo fmt --check
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
```
Expected: clean.

- [ ] **Step 4: Optional — tier-4 oracle tests if the submodule build is available**

Run:
```bash
cargo test -p ast-oracle-tests 2>&1 | tail -20
```
Expected: green (or a clean skip-with-notice if the harness gates on the oracle build). This is the strongest confirmation that the un-mirrored quirks match the fixed C.

- [ ] **Step 5: No commit** — verification only. Confirm `git status` is clean (all work already committed in Tasks 1-13).

---

## Self-Review Notes

- **Spec coverage:** Bucket 1 (Tasks 1-7), Bucket 2 doc (Task 9), Bucket 3 left untouched (Task 9 Step 3), regenerated goldens (Tasks 1-2), fixture import (Task 10), manifest + tagging (Task 11), harness skip-reason split (Task 12), fix-all failures (Task 13), verification (Tasks 0, 14). All spec sections map to a task.
- **Ported C tests:** AxAngle (Task 3), astRate/ChebyMap (Task 4), PermMap (Task 6), timemap round-trip (Task 7), SpecFluxFrame already matches (no test needed; Bucket 2). SpecMap/VELOSYS covered by regenerated goldens.
- **Type consistency:** helper signatures (`lsrk_vel` etc.), `ax_angle`, `numeric_rate`, `PermMap::equal`, `ast_dat` all referenced with their existing signatures; only bodies change.
- **Escalation points** are explicit in Tasks 4, 6, 7, 13 (STOP-and-raise if a golden or supported-class fixture behaves unexpectedly).

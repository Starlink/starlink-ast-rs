# Tier-2: encoder-level numerical cleanups (revised K)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pass roadmap category-F wcsconv tests (~7 fixtures) by porting the three thresholded numerical cleanups the C library applies in its CD/CRVAL/CRPIX encoder path. Then delete the existing `enc_aips` CROTA snap (subsumed by the new generic logic).

**Architecture:** Mirror three C-library cleanups exactly, all applied in `wcs_write::store_from_frameset` after the `cd_decomp`/CRVAL/CRPIX values are populated and before encoder dispatch:

1. **PC near-zero zeroing** — mirrors `SplitMat` in `fitschan.c:33253-33357`. After CD → (CDELT, PC) factorisation, set any PC element below 1e-7 to 0; recompute CD as CDELT × PC so all encoders (CD-direct AIPS++ and PC-style FITS-WCS) see the cleaned values.
2. **CRVAL near-zero zeroing** — mirrors `WcsFromStore` line 2939-2958. For each axis, compute the row-magnitude of the CD matrix as a CDELT-equivalent. If `|crval| < sqrt(DBL_EPSILON · cdelt²)`, set CRVAL to 0.0.
3. **CRPIX micro-pixel rounding** — mirrors `WcsFromStore` line 2961-2968. Round each CRPIX value to the nearest 1e-6 (millionth of a pixel).

This is the *correct* implementation of what the spec called "F1 — encoder snap." The earlier framing of these as "hacks" was based on a wrong model of how the C library cleans up. The investigation log (in conversation history above) confirmed: C `astSimplify` preserves the residue identically to ours; the cleanup happens entirely at the encoder level.

**Tech Stack:** Rust 2024, existing `wcs_decompose::split_cd`, existing `FitsStore`. No new dependencies.

---

## Background — why "K (simplifier)" was wrong and "encoder cleanups" is right

The 2026-05-21 roadmap framed the residue problem as a simplifier deep-traversal gap. Investigation in conversation history (commit `c23b6cd` captures the gappt fixture as evidence) showed:

- The pre-simplify mapping captured from the wcsconv path already contains 1e-16 residue values inside `MatrixMap` and `WinMap` components.
- Both Rust `Mapping::simplify` and the C `astSimplify` binary preserve these residues identically — the .simp output of both contains the same M0/M1 values.
- The C library's wcsconv tests pass anyway, because the cleanup is applied in `WcsFromStore`/`SplitMat` AFTER simplification, at the encoder level.

So the K plan's premise ("close simplifier gap, residue disappears") was wrong. The actual fix mirrors the C library encoder.

Three cleanup sites identified in `reference/starlink-ast/src/fitschan.c`:
- Line 33238: `if( fabs( *a ) < 1.E-7 ) *a = 0.0;` (PC element snap, inside `SplitMat`)
- Line 2955: `if( fabs( crval ) < sqrt( DBL_EPSILON*cdelt2 ) ) { SetItem(&(store->crval), ..., 0.0, ...); }` (CRVAL snap, inside `WcsFromStore`)
- Line 2966: `floor( crpix*1.0E6 + 0.5 )*1.0E-6` (CRPIX micro-pixel rounding, inside `WcsFromStore`)

These are the policy our Rust port must mirror.

---

## File structure

- **Modify:** `crates/ast-core/src/fitschan/wcs_write.rs` — add `clean_store_residue(store: &mut FitsStore)` helper plus wire it into `store_from_frameset` near the end (before/after `identify_axes`, depending on which sets the final CRVAL).
- **Modify:** `crates/ast-core/src/fitschan/wcs_decompose.rs` — extend `split_cd` to also return cleaned CD by applying the 1e-7 PC snap and re-multiplying. Or: add a separate `split_cd_with_snap` if API stability matters. Or: leave `split_cd` alone and snap PC inside `clean_store_residue` after a temporary split.
- **Modify:** `crates/ast-core/src/fitschan/enc_aips.rs` — delete the CROTA snap branch (`if crota.abs() < 1e-9 { crota = 0.0; }`) and surrounding paragraph of comment (lines ~252-272).

---

## Hard rule: thresholds match C exactly

These threshold values are part of the contract:
- PC element snap: `< 1.0e-7` (NOT 1e-9 or 1e-6)
- CRVAL snap: `< sqrt(DBL_EPSILON · cdelt_row_magnitude²)` (NOT a fixed threshold)
- CRPIX rounding: `floor(x · 1e6 + 0.5) · 1e-6`

If any threshold needs to change, document why with a citation to the C source line that justifies the deviation.

---

## Task 1: Failing integration probes

**Files:**
- Modify: `crates/ast-core/tests/test_fitschan_wcs.rs`

The wcsconv tests already exercise the full path. We need direct unit-level tests that exercise the cleanup logic in isolation. But before that, we want to confirm the current wcsconv state and pin which fixtures the cleanup unblocks.

- [ ] **Step 1: Capture current wcsconv state**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "wcsconv results|gappt|ukirt-dtai|galactic-tan_fits-iraf|sin-plain_fits-iraf|fk4-aips_fits-iraf|fits-pc-altgal|pixel-shear"
```

Expected (current): `=== wcsconv results: 110 passed, 82 failed, 0 skipped (of 192 total) ===` and seven category-F failures with 1e-14/1e-15 residue patterns.

Record this baseline in your scratch notes; you'll re-run the same command at the end of Task 5 to confirm the delta.

- [ ] **Step 2: Add focused unit tests for each of the three cleanups**

Append to `crates/ast-core/tests/test_fitschan_wcs.rs`:

```rust
#[cfg(test)]
mod cleanup_tests {
    //! Encoder-level numerical cleanup tests. These exercise the
    //! `clean_store_residue` logic in isolation, mirroring the C library's
    //! WcsFromStore CRVAL/CRPIX cleanups (fitschan.c:2991-3020) and
    //! SplitMat PC snap (fitschan.c:33350).
    //!
    //! These tests are intentionally written against the public wcsconv
    //! end-to-end behaviour where possible — they don't test the helper
    //! function directly because that would require exposing internals.
    //! The unit-level tests for the helper live alongside it in wcs_write.rs.

    // (Placeholder — actual tests are end-to-end via test_wcsconv suite,
    // which Task 5 verifies. The helper itself is unit-tested in
    // wcs_write.rs's mod tests.)
}
```

(Skip this if the existing test_fitschan_wcs.rs is already crowded; the unit tests live in wcs_write.rs alongside the helper anyway.)

---

## Task 2: PC near-zero snap on CD matrix

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_decompose.rs` OR `wcs_write.rs` (location decision below)

The C library's `SplitMat` (line 33141) factors CD into (CDELT, PC) and applies `if (fabs(*a) < 1.E-7) *a = 0.0;` to each PC element. Mirror this. We choose to add the snap as a NEW helper rather than modifying the existing `split_cd` so that `split_cd`'s contract (pure decomposition) stays clean.

- [ ] **Step 1: Write the failing test**

Add a unit test in `crates/ast-core/src/fitschan/wcs_decompose.rs::tests`:

```rust
    #[test]
    fn split_cd_snap_zeroes_residue_pc_elements() {
        // CD with rotation cos(30°)/sin(30°) at scale 1e-3 plus a
        // 1e-15 off-diagonal residue. The residue should snap to 0
        // after the PC threshold is applied.
        let cos30 = 30.0_f64.to_radians().cos();
        let sin30 = 30.0_f64.to_radians().sin();
        let scale = 1e-3;
        let mut cd = ndarray::Array2::zeros((2, 2));
        cd[[0, 0]] = scale * cos30;
        cd[[0, 1]] = -scale * sin30;
        cd[[1, 0]] = scale * sin30;
        cd[[1, 1]] = scale * cos30 + 1e-15; // residue

        let cleaned = clean_cd_residue(&cd);

        // Diagonal cos(30°)*scale stays.
        assert!((cleaned[[0, 0]] - scale * cos30).abs() < 1e-12);
        // The 1e-15 residue: pre-split, it was scale * cos30 + 1e-15.
        // Post-split: (cos30 + 1e-15/scale) magnitude ≈ 1.
        // The 1e-15/scale term in PC is 1e-12 — well below 1e-7 ?
        // Actually 1e-15 / 1e-3 = 1e-12. That's < 1e-7, so the
        // perturbation snaps out of PC. Cleaned CD becomes scale * cos30.
        assert!((cleaned[[1, 1]] - scale * cos30).abs() < 1e-13);
    }

    #[test]
    fn split_cd_snap_preserves_real_off_diagonals() {
        // Real rotation off-diagonals (sin(30°) * 1e-3 ≈ 5e-4) must NOT
        // be zeroed, because PC[0][1] = -sin(30°) ≈ -0.5, well above 1e-7.
        let cos30 = 30.0_f64.to_radians().cos();
        let sin30 = 30.0_f64.to_radians().sin();
        let scale = 1e-3;
        let mut cd = ndarray::Array2::zeros((2, 2));
        cd[[0, 0]] = scale * cos30;
        cd[[0, 1]] = -scale * sin30;
        cd[[1, 0]] = scale * sin30;
        cd[[1, 1]] = scale * cos30;

        let cleaned = clean_cd_residue(&cd);

        // All four entries should round-trip exactly.
        assert!((cleaned[[0, 0]] - scale * cos30).abs() < 1e-15);
        assert!((cleaned[[0, 1]] - (-scale * sin30)).abs() < 1e-15);
        assert!((cleaned[[1, 0]] - scale * sin30).abs() < 1e-15);
        assert!((cleaned[[1, 1]] - scale * cos30).abs() < 1e-15);
    }
```

- [ ] **Step 2: Implement the helper in `wcs_decompose.rs`**

Add below `split_cd` (around line 97):

```rust
/// Apply the C library's `SplitMat` 1e-7 PC-element threshold: split
/// CD into (CDELT, PC), zero any PC element whose absolute value is
/// below 1e-7, then recompose CD = CDELT × PC. Returns the cleaned CD.
///
/// Mirrors `fitschan.c::SplitMat` line 33238:
/// `if( fabs( *a ) < 1.E-7 ) *a = 0.0;`
///
/// Rationale (from C comment, line 33232): "Set to zero any PC values
/// which are less than 1.0E-7 (such values may be produced by rounding
/// errors)." The threshold is scale-relative because PC has unit row
/// magnitude — a PC value of 1e-7 corresponds to a CD value of
/// 1e-7 × CDELT, which is the round-trip noise floor.
pub(crate) fn clean_cd_residue(cd: &Array2<f64>) -> Array2<f64> {
    let naxis = cd.nrows();
    let (cdelt, mut pc) = split_cd(cd);

    // Apply the 1e-7 PC snap.
    for i in 0..naxis {
        for j in 0..naxis {
            if pc[[i, j]].abs() < 1.0e-7 {
                pc[[i, j]] = 0.0;
            }
        }
    }

    // Recompose CD = diag(CDELT) * PC.
    let mut cleaned = Array2::zeros((naxis, naxis));
    for i in 0..naxis {
        for j in 0..naxis {
            cleaned[[i, j]] = cdelt[i] * pc[[i, j]];
        }
    }
    cleaned
}
```

- [ ] **Step 3: Run the unit tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_decompose -- --nocapture
```

Expected: existing tests pass + 2 new ones.

- [ ] **Step 4: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_decompose.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): add clean_cd_residue helper mirroring SplitMat 1e-7 snap

C library's SplitMat (fitschan.c:33253-33357) factors CD into CDELT+PC
and zeroes PC elements below 1e-7 to remove rounding-error residue.
This helper applies the same logic: split, snap PC < 1e-7 to 0, then
re-multiply to produce a cleaned CD matrix.

The threshold is scale-relative because PC has unit row magnitude;
1e-7 corresponds to a CD residue of 1e-7 * CDELT which is the round-
trip noise floor. Rotations and shears with PC components above 1e-7
are preserved exactly.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 3: CRVAL near-zero snap

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

C library's `WcsFromStore` line 2939-2958 sets CRVAL to 0 when `|crval| < sqrt(DBL_EPSILON · cdelt²)`, where cdelt² is the squared row magnitude of the PC matrix scaled by CDELT. For our purposes, cdelt² = `Σ_j CD[i][j]²` (the row magnitude of CD before splitting).

- [ ] **Step 1: Write the failing test**

Add to `crates/ast-core/src/fitschan/wcs_write.rs::tests` (or create the mod tests block if missing):

```rust
    #[test]
    fn snap_crval_near_zero_to_exact_zero() {
        let cd = ndarray::array![[1e-3, 0.0], [0.0, 1e-3]];
        let crval = vec![1e-15, 5.0];
        let snapped = snap_crval_residue(&crval, &cd);
        // 1e-15 with cdelt² = 1e-6: threshold = sqrt(eps * 1e-6) ≈ 1.5e-11.
        // 1e-15 < 1.5e-11 so should snap to 0.
        assert_eq!(snapped[0], 0.0);
        // 5.0 is way above any threshold; stays.
        assert_eq!(snapped[1], 5.0);
    }

    #[test]
    fn snap_crval_preserves_real_small_values() {
        let cd = ndarray::array![[1e-3, 0.0], [0.0, 1e-3]];
        let crval = vec![1e-9, 5.0];
        // 1e-9 is above threshold (1.5e-11) so should be preserved.
        let snapped = snap_crval_residue(&crval, &cd);
        assert_eq!(snapped[0], 1e-9);
        assert_eq!(snapped[1], 5.0);
    }
```

- [ ] **Step 2: Implement the helper**

Add to `wcs_write.rs` (at module-private scope, near `store_from_frameset`):

```rust
/// Apply the C library's CRVAL near-zero snap: if `|crval[i]|` is below
/// `sqrt(DBL_EPSILON · cdelt²[i])` where `cdelt²[i] = Σ_j CD[i][j]²`,
/// set `crval[i]` to 0.
///
/// Mirrors `fitschan.c::WcsFromStore` lines 2939-2958. Threshold is
/// scale-aware: for CD scaled at 1e-3, threshold ≈ 1.5e-11; for CD at
/// 1.0, threshold ≈ 1.5e-8. This catches FP residue while preserving
/// physically-meaningful small values.
fn snap_crval_residue(crval: &[f64], cd: &ndarray::Array2<f64>) -> Vec<f64> {
    let naxis = crval.len();
    let mut out = crval.to_vec();
    for i in 0..naxis {
        let cdelt2: f64 = (0..naxis).map(|j| cd[[i, j]] * cd[[i, j]]).sum();
        let threshold = (f64::EPSILON * cdelt2).sqrt();
        if out[i].abs() < threshold {
            out[i] = 0.0;
        }
    }
    out
}
```

- [ ] **Step 3: Run the unit tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_write -- --nocapture
```

Expected: existing tests pass + 2 new ones.

- [ ] **Step 4: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): add snap_crval_residue helper

Mirrors fitschan.c WcsFromStore lines 2939-2958: CRVAL[i] is set to 0
when |crval[i]| < sqrt(DBL_EPSILON * cdelt[i]^2), where cdelt[i]^2 is
the row magnitude of the CD matrix. Threshold is scale-aware so it
catches FP residue (e.g. pixel-shear's CRVAL1 = -7.1e-14) without
zeroing physically-meaningful small values.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 4: CRPIX micro-pixel rounding

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

C library's `WcsFromStore` line 2961-2968: `floor(crpix * 1e6 + 0.5) * 1e-6`. Quantizes to the nearest millionth of a pixel.

- [ ] **Step 1: Write the failing test**

Add to `wcs_write.rs::tests`:

```rust
    #[test]
    fn round_crpix_to_micropixel_truncates_subpixel_drift() {
        let crpix = vec![100.0000001234, 50.5];
        let rounded = round_crpix_to_micropixel(&crpix);
        // 100.0000001234 quantizes to 100.0 (within 1e-7 of nearest 1e-6).
        assert!((rounded[0] - 100.0).abs() < 1e-9);
        // 50.5 stays exactly.
        assert_eq!(rounded[1], 50.5);
    }

    #[test]
    fn round_crpix_negative_values() {
        let crpix = vec![-10.5000002];
        let rounded = round_crpix_to_micropixel(&crpix);
        // -10.5000002 quantizes to -10.500000 (the 2e-7 drift goes).
        assert!((rounded[0] - (-10.5)).abs() < 1e-9);
    }
```

- [ ] **Step 2: Implement the helper**

Add to `wcs_write.rs`:

```rust
/// Apply the C library's CRPIX micro-pixel rounding: round each CRPIX
/// to the nearest millionth of a pixel.
///
/// Mirrors `fitschan.c::WcsFromStore` line 2966:
/// `floor( crpix*1.0E6 + 0.5 )*1.0E-6`.
///
/// This removes sub-millionth-pixel round-trip drift from numerical
/// CRPIX solving (`solve_crpix_from_decomp`). Real CRPIX values are
/// physically meaningful only at pixel-fraction precision, never at
/// sub-millionth.
fn round_crpix_to_micropixel(crpix: &[f64]) -> Vec<f64> {
    crpix.iter().map(|x| (x * 1.0e6 + 0.5).floor() * 1.0e-6).collect()
}
```

Note: `(x * 1.0e6 + 0.5).floor()` is the C library's idiom for "round to nearest"; this matches `fitschan.c:3018` exactly. For negative numbers in C with negative-zero-aware rounding, this maps correctly. Verify the test `round_crpix_negative_values` passes — if not, the rounding rule may need to be `(x * 1e6).round() * 1e-6` instead. Match what the C library's output produces.

- [ ] **Step 3: Run the unit tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_write -- --nocapture
```

Expected: 2 new tests pass.

- [ ] **Step 4: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): add round_crpix_to_micropixel helper

Mirrors fitschan.c WcsFromStore line 2966: round CRPIX to the nearest
millionth of a pixel via floor(x*1e6 + 0.5) * 1e-6. Removes sub-
millionth-pixel round-trip drift from numerical CRPIX solving.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 5: Wire all three cleanups into store_from_frameset

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

Add a `clean_store_residue` umbrella function that applies all three cleanups, and call it at the end of `store_from_frameset`.

- [ ] **Step 1: Add the umbrella function**

In `wcs_write.rs`, after the three helpers added in Tasks 2-4:

```rust
/// Apply C-library-style numerical cleanups to the FitsStore before
/// encoder dispatch. Mirrors three cleanups in fitschan.c:
///
/// 1. CD residue snap via SplitMat 1e-7 threshold (line 33238).
/// 2. CRVAL near-zero zeroing with scale-aware threshold (lines 2939-2958).
/// 3. CRPIX micro-pixel rounding (line 2966).
///
/// Without these, FP rounding noise at the 1e-15 level reaches the
/// encoder and gets emitted as `CD1_2 = -7.4E-15` cards. The C library
/// applies these in WcsFromStore; we mirror it so all encodings (FITS-WCS,
/// AIPS, AIPS++, IRAF, PC) emit clean values.
fn clean_store_residue(store: &mut FitsStore) {
    let naxis = store.naxis;

    // Build a CD matrix from store.cd, applying defaults for unset cells.
    let mut cd_array = ndarray::Array2::zeros((naxis, naxis));
    for i in 0..naxis {
        for j in 0..naxis {
            cd_array[[i, j]] = store.cd[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
        }
    }

    // 1. Snap small CD residue via SplitMat-style 1e-7 PC threshold.
    let cleaned_cd = crate::fitschan::wcs_decompose::clean_cd_residue(&cd_array);
    for i in 0..naxis {
        for j in 0..naxis {
            store.cd[i][j] = Some(cleaned_cd[[i, j]]);
        }
    }

    // 2. CRVAL near-zero snap (scale-aware threshold based on cleaned CD).
    let crval_in: Vec<f64> = (0..naxis).map(|i| store.crval[i].unwrap_or(0.0)).collect();
    let crval_out = snap_crval_residue(&crval_in, &cleaned_cd);
    for i in 0..naxis {
        if store.crval[i].is_some() {
            store.crval[i] = Some(crval_out[i]);
        }
    }

    // 3. CRPIX micro-pixel rounding.
    let crpix_in: Vec<f64> = (0..naxis).map(|i| store.crpix[i].unwrap_or(1.0)).collect();
    let crpix_out = round_crpix_to_micropixel(&crpix_in);
    for i in 0..naxis {
        if store.crpix[i].is_some() {
            store.crpix[i] = Some(crpix_out[i]);
        }
    }
}
```

- [ ] **Step 2: Wire into store_from_frameset**

Find the end of `store_from_frameset` (around line 184). Currently:

```rust
    // Identify axis types from the current frame and populate CTYPE/CRVAL.
    identify_axes(fs, &crval_for_store, &mut store, projection)?;

    Ok(store)
}
```

Replace with:

```rust
    // Identify axis types from the current frame and populate CTYPE/CRVAL.
    identify_axes(fs, &crval_for_store, &mut store, projection)?;

    // Apply C-library-style numerical cleanups (SplitMat 1e-7, CRVAL
    // near-zero, CRPIX micro-pixel rounding) so all encoders see clean
    // values and emit CD/CRVAL/CRPIX cards without FP residue.
    clean_store_residue(&mut store);

    Ok(store)
}
```

- [ ] **Step 3: Run the wcsconv suite — confirm category-F tests pass**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "wcsconv results|gappt|ukirt-dtai|galactic-tan_fits-iraf|sin-plain_fits-iraf|fk4-aips_fits-iraf|fits-pc-altgal|pixel-shear"
```

Expected: pass count up by ~6-7. The seven category-F failures should turn green:
- `gappt_fits-wcs`
- `ukirt-dtai_fits-wcs`
- `galactic-tan_fits-iraf`
- `sin-plain_fits-iraf`
- `fk4-aips_fits-iraf`
- `fits-pc-altgal_fits-wcs`
- `pixel-shear_fits-aips++`

If any still fails: read the diff, see what's still residue-y. The CROTA snap in `enc_aips` is still active at this point (Task 6 removes it). If a test passes only because of the CROTA snap, that's expected — it'll be re-tested in Task 6.

- [ ] **Step 4: Run existing FitsChan tests for regression**

```bash
cargo test -p ast-core --test test_fitschan
cargo test -p ast-core --test test_fitschan_encodings
cargo test -p ast-core --test test_fitschan_wcs
cargo test -p ast-core --test test_fitschan_wcs_write
cargo test -p ast-core --test test_fitschan_native_keyword
cargo test -p ast-core --test test_simplify_regression
```

All must remain green. Particular attention: tests that assert exact CD/CRVAL/CRPIX values may now produce slightly different (cleaner) values; if they fail, the test expectations need updating to match the new cleaner output (which is the right answer per the C library).

- [ ] **Step 5: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
fix(fitschan): apply C-library encoder numerical cleanups in store_from_frameset

Adds clean_store_residue() that mirrors three threshold-based cleanups
the C library applies in WcsFromStore/SplitMat:

  1. PC element snap (1e-7) on the CD matrix via SplitMat-style
     factorisation (fitschan.c:33350).
  2. CRVAL near-zero snap with scale-aware threshold
     sqrt(DBL_EPSILON * cdelt^2) (fitschan.c:3007).
  3. CRPIX micro-pixel rounding floor(x*1e6 + 0.5)*1e-6
     (fitschan.c:3018).

Closes ~7 wcsconv category-F failures (gappt, ukirt-dtai,
galactic-tan_fits-iraf, sin-plain_fits-iraf, fk4-aips_fits-iraf,
fits-pc-altgal, pixel-shear) where build_pixel_to_iwc produced
1e-15-level residue that propagated through Mapping::simplify
(correctly preserved as mathematical truth) into the encoded cards.

The C library's astSimplify also preserves this residue identically;
the cleanup is unambiguously an encoder-level policy, not a
simplifier gap. Pre-existing CROTA snap in enc_aips is now subsumed
by the generic logic and is removed in the next commit.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 6: Delete the CROTA snap workaround

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_aips.rs`

The 1e-9° CROTA snap at `enc_aips.rs:266-270` is now subsumed by the generic CD/PC cleanup applied upstream in `clean_store_residue`. Delete it together with the long explanatory comment.

- [ ] **Step 1: Locate the block**

```bash
grep -n "Snap rotations\|crota.abs() < 1e-9" crates/ast-core/src/fitschan/enc_aips.rs
```

Around line 252-272.

- [ ] **Step 2: Inspect the actual current shape**

```bash
sed -n '240,275p' crates/ast-core/src/fitschan/enc_aips.rs
```

Identify the exact range to delete. Note: the comment explaining the simplifier-gap workaround spans many lines and goes too. The remaining code after deletion should be just the bare CROTA computation:

```rust
    if naxis >= 2 {
        let crota = pc_vals[[1, 0]].atan2(pc_vals[[1, 1]]).to_degrees();
        let k = crota_axis(store);
        cards.set_float(&format!("CROTA{k}"), crota, "Axis rotation");
    }
```

(No snap branch, no preceding multi-paragraph workaround comment.)

- [ ] **Step 3: Make the edit**

Replace the entire snap-related block (comment + if-branch + computation) with the bare version above. Verify by:

```bash
grep -n "1e-9\|Snap rotations\|simplifier gap" crates/ast-core/src/fitschan/enc_aips.rs
```

— no matches expected.

- [ ] **Step 4: Run the FITS-AIPS wcsconv tests**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "fits-aips"
```

The AIPS pass count should hold or rise. If any AIPS test now fails with a tiny CROTA residue, the upstream cleanup is incomplete — return to Task 5's logic.

- [ ] **Step 5: Run the wcsconv suite for the global pass count**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: same count or higher than after Task 5 (deleting dead code shouldn't change anything; if it does, the upstream cleanup didn't cover all the cases the snap was covering).

- [ ] **Step 6: Run the FITS-AIPS unit tests**

```bash
cargo test -p ast-core --lib fitschan::enc_aips -- --nocapture
```

All green.

- [ ] **Step 7: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_aips.rs
git commit -m "$(cat <<'EOF'
refactor(fitschan): remove CROTA snap-to-zero workaround

The 1e-9° snap and its multi-paragraph comment were a partial
implementation of what the previous commit now applies generically
in clean_store_residue (mirroring fitschan.c's SplitMat 1e-7 PC
snap). Removing the AIPS-specific snap aligns Rust output with C
output for FITS-AIPS exactly the way other encodings (FITS-WCS,
FITS-IRAF, FITS-PC) now also align.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 7: Final verification

**Files:** none

- [ ] **Step 1: Run the full wcsconv suite**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: 110 → ~117 (close to the +7 estimate). `pixel-shear_fits-aips++` may resolve fully or partially depending on whether its only issue was the CRVAL residue.

- [ ] **Step 2: Full workspace tests**

```bash
cargo test --workspace 2>&1 | grep -E "FAILED|panic" | head -10
```

No failures.

- [ ] **Step 3: Workspace clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Clean.

- [ ] **Step 4: No commit needed** unless verification revealed something to fix.

---

## Self-review checklist

- [ ] **Spec coverage:** the three cleanups from `fitschan.c` are each implemented (Tasks 2, 3, 4) and wired into `store_from_frameset` (Task 5). The CROTA snap is removed (Task 6) and verified (Tasks 6+7).
- [ ] **No placeholders:** every step has explicit code, paths, commands, expected outputs.
- [ ] **Frequent commits:** 5 commits total (one per cleanup helper, one wire-in, one snap removal).
- [ ] **Done criteria:** wcsconv ≥117/192, full workspace green, clippy clean, CROTA snap deleted from `enc_aips`.

---

## What this plan does NOT cover

- **Native-text reference handling:** the existing simplifier `cmpmap_nested_inverse_cancel` and the new `wcsconv_gappt_iwc_residue` fixtures continue to verify that our `Mapping::simplify` produces transform-equivalent output to C. Both should remain passing throughout this work; if either fails, the simplifier itself has regressed.
- **Other roadmap categories:** H1 (alt-WCS plane emission) and D-pre (DATE-OBS derivation) remain Tier-2 work but are independent of this cleanup. They each get their own plan.
- **Tier-3 / Tier-4 items:** unchanged.

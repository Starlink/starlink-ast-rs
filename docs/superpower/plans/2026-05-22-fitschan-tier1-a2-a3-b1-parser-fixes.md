# FitsChan Tier-1 A2 + A3 + B1 — misc parser fixes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pass `car3_native`, `axes-swap_native`, `jan_fits-class_ast`, and `jan_fits-class-rt` wcsconv tests by closing three independent reader gaps.

**Architecture:** Three orthogonal fixes, one commit each.
- **A2** — extend `wcs_keywords::extract_wcs_keywords` axis-count fallback so that, when `WCSAXES` and `NAXIS` and `NAXISn` are all absent, the highest CTYPEi/CRVALi/CRPIXi index decides the axis count. (`car3_native` has only `CTYPE1`, `CTYPE2`, `CRVAL1`, `CRVAL2`, `CDELT1`, `CDELT2`, `CRPIX1`, `CRPIX2`.)
- **A3** — fix the `wcs_read::build_general_iwc_to_world` hardcoded "celestial axes are consecutive" assumption (`crates/ast-core/src/fitschan/wcs_read.rs:141`). When the celestial pair is non-adjacent, wrap the parallel-composed mapping in a `PermMap` to route axes back to FITS order. (`axes-swap_native` has CTYPE1=DEC, CTYPE2=WAVE, CTYPE3=RA, CTYPE4=STOKES — celestial pair at indices 0 and 2, separated by spectral axis at 1.)
- **B1** — extend `enc_class::read_from_cards` so that when NAXIS is missing it falls back to the same CTYPE-index inference as A2. (`jan.fits-class` has CTYPE1/2/3 but no NAXIS.)

**Tech Stack:** Rust 2024, existing FitsChan infrastructure (`CardList`, `FitsStore`, `wcs_keywords`, `wcs_read`, `enc_class`, `mappings::PermMap`). No new dependencies.

---

## File structure

- **Modify:** `crates/ast-core/src/fitschan/wcs_keywords.rs` — extend axis-count fallback (A2). Extract the fallback into a private helper so B1 can reuse it.
- **Modify:** `crates/ast-core/src/fitschan/wcs_read.rs::build_general_iwc_to_world` — replace the hardcoded `i = first_cel + 2` with correct second-axis tracking; insert a `PermMap` wrapper when the natural assembly order doesn't match FITS axis order (A3).
- **Modify:** `crates/ast-core/src/fitschan/enc_class.rs::read_from_cards` — call the shared axis-count helper when NAXIS is absent (B1).
- **Test:** Three `#[test]` functions added to `crates/ast-core/tests/test_fitschan_wcs.rs` — one per fixture, each load-only (no value check beyond "doesn't panic and decodes to a FrameSet").

---

## Background facts (read before starting)

- Existing axis-count fallback at `crates/ast-core/src/fitschan/wcs_keywords.rs:23-43` already handles `NAXIS1..NAXIS9`. It does NOT scan `CTYPE1..` or `CRVAL1..`.
- `wcs_read::build_general_iwc_to_world` at `crates/ast-core/src/fitschan/wcs_read.rs:106-184` walks axes in FITS order and emits `parts: Vec<DynMapping>` in encounter order. When celestial axes are non-adjacent (e.g. lon=0, lat=2 for `axes-swap`), at index 0 it pushes the celestial mapping (which has nin=2, nout=2) and then jumps to `i = first_cel + 2 = 2` — skipping index 1 (the spectral axis). The spectral mapping is never pushed. The result has nout=4 (2 celestial + 1 stokes-shift + … wait, let me re-derive: see line 141 comment "Assuming consecutive for now"), but the FITS axis ordering in the chain needs to be undone with a `PermMap`.
- `mappings::PermMap` (`crates/ast-core/src/mappings/perm.rs`) takes `inperm: &[i64]`, `outperm: &[i64]`, `constants: &[f64]`. 1-based axis indices; positive entries reference an input axis; negative entries reference a constant. For our purposes only positive entries are used.
- `FitsStore::celestial_axes() -> Option<(usize, usize)>` returns 0-based indices already.
- The CLASS reader at `enc_class.rs:32-36` errors hard on missing NAXIS. `jan.fits-class` has CTYPE1=RA, CTYPE2=DEC, CTYPE3=FREQ — three axes — but no NAXIS card.

---

## Task 1: Failing integration tests (red)

**Files:**
- Modify: `crates/ast-core/tests/test_fitschan_wcs.rs`

- [ ] **Step 1: Confirm the current failures**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "car3|axes-swap|jan_fits"
```

Expected output (current):
```
  jan_fits-class_ast : read: invalid FITS keyword value found: NAXIS keyword not found in CLASS header
  car3_native : read: invalid FITS keyword value found: cannot determine number of WCS axes: neither WCSAXES nor NAXIS found
  axes-swap_native : read: incompatible numbers of coordinates: series CmpMap: first.nout()=4 but second.nin()=3
  jan_fits-class-rt : read: invalid FITS keyword value found: NAXIS keyword not found in CLASS header
```

- [ ] **Step 2: Add focused tests**

Append to `crates/ast-core/tests/test_fitschan_wcs.rs`:

```rust
//! Tier-1 A2/A3/B1 regression tests — three reader gaps that block
//! wcsconv fixtures with unusual headers (no NAXIS, non-adjacent
//! celestial axes, NAXIS-less CLASS encoding).

use std::path::PathBuf;

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv/inputs")
        .join(name)
}

fn read_fixture(name: &str) -> AnyObject {
    let text = std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|e| panic!("read {name}: {e}"));
    let mut fc = FitsChan::from_header(&text)
        .unwrap_or_else(|e| panic!("from_header {name}: {e}"));
    fc.guess_encoding();
    fc.read()
        .unwrap_or_else(|e| panic!("read {name}: {e}"))
        .unwrap_or_else(|| panic!("read {name}: returned None"))
}

#[test]
fn car3_naxis_inferred_from_ctype_indices() {
    // car3.head has CTYPE1/CTYPE2/CRVAL1/CRVAL2/CDELT1/CDELT2/CRPIX1/CRPIX2
    // and no NAXIS, no NAXISn, no WCSAXES. Reader must infer naxis = 2.
    let _obj = read_fixture("car3.head");
}

#[test]
fn axes_swap_non_adjacent_celestial_axes() {
    // axes-swap.head has CTYPE1=DEC--TAN, CTYPE2=WAVE, CTYPE3=RA---TAN,
    // CTYPE4=STOKES — celestial pair at indices 0 and 2 (separated by
    // spectral). Reader must produce a working pixel-to-world mapping.
    let _obj = read_fixture("axes-swap.head");
}

#[test]
fn jan_class_no_naxis_keyword() {
    // jan.fits-class has CTYPE1=RA, CTYPE2=DEC, CTYPE3=FREQ but no NAXIS.
    // CLASS reader must infer naxis = 3 from CTYPE indices.
    let _obj = read_fixture("jan.fits-class");
}
```

- [ ] **Step 3: Run them, confirm they fail**

```bash
cargo test -p ast-core --test test_fitschan_wcs car3_naxis_inferred_from_ctype_indices -- --nocapture
cargo test -p ast-core --test test_fitschan_wcs axes_swap_non_adjacent_celestial_axes -- --nocapture
cargo test -p ast-core --test test_fitschan_wcs jan_class_no_naxis_keyword -- --nocapture
```

Each should panic with the expected error from Step 1.

- [ ] **Step 4: Commit the failing tests**

```bash
git add crates/ast-core/tests/test_fitschan_wcs.rs
git commit -m "$(cat <<'EOF'
test(fitschan): expected-failure tests for A2/A3/B1 parser gaps

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 2: A2 — CTYPE-index axis-count fallback

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs`

The existing fallback (lines 27-42) only scans `NAXIS1..NAXIS9`. Extend it to also scan `CTYPE1..CTYPE9`, `CRVAL1..CRVAL9`, `CRPIX1..CRPIX9`, `CDELT1..CDELT9` and take the maximum index found. Extract the fallback into a private helper so B1 can reuse it.

- [ ] **Step 1: Add the helper and use it**

In `wcs_keywords.rs`, replace the body of `extract_wcs_keywords` axis-count logic. Find:

```rust
    let naxis = match (wcsaxes, naxis_val) {
        (Some(w), _) => w,
        (None, Some(n)) => n,
        (None, None) => {
            // Fallback: infer from highest-numbered NAXISn keyword.
            let mut max_n = 0;
            for i in 1..=9 {
                let key = format!("NAXIS{i}");
                if cards.find(&key).is_some() {
                    max_n = i;
                }
            }
            if max_n == 0 {
                return Err(AstError::new(
                    ErrorCode::Bdfts,
                    "cannot determine number of WCS axes: neither WCSAXES nor NAXIS found",
                ));
            }
            max_n
        }
    };
```

Replace with:

```rust
    let naxis = match (wcsaxes, naxis_val) {
        (Some(w), _) => w,
        (None, Some(n)) => n,
        (None, None) => {
            infer_axis_count_from_indexed_keywords(cards).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Bdfts,
                    "cannot determine number of WCS axes: neither WCSAXES, NAXIS, NAXISn, CTYPEn, CRVALn, CRPIXn, nor CDELTn found",
                )
            })?
        }
    };
```

Add the helper function above `extract_wcs_keywords`:

```rust
/// Infer the number of WCS axes from the highest-indexed FITS keyword
/// found in the header.
///
/// Scans `NAXIS{n}`, `CTYPE{n}`, `CRVAL{n}`, `CRPIX{n}`, `CDELT{n}` for
/// `n` in 1..=9 and returns the maximum index seen, or `None` if none
/// are present. Mirrors the C library's last-resort axis-count guess in
/// `fitschan.c::AstroSysCelOrder` and related call sites: when no
/// explicit count is available, the presence of axis-indexed keywords
/// up to `n` implies at least `n` axes exist.
pub(crate) fn infer_axis_count_from_indexed_keywords(cards: &CardList) -> Option<usize> {
    let prefixes = ["NAXIS", "CTYPE", "CRVAL", "CRPIX", "CDELT"];
    let mut max_n = 0;
    for n in 1..=9 {
        for prefix in &prefixes {
            let key = format!("{prefix}{n}");
            if cards.find(&key).is_some() {
                max_n = n;
                break;
            }
        }
    }
    if max_n == 0 { None } else { Some(max_n) }
}
```

- [ ] **Step 2: Add a unit test**

In the existing `#[cfg(test)] mod tests` block in `wcs_keywords.rs`, add:

```rust
    #[test]
    fn axis_count_inferred_from_ctype_when_naxis_missing() {
        let header = "\
CTYPE1  = 'GLON-CAR'
CRVAL1  = 1.85e2
CDELT1  = -1.25e-1
CRPIX1  = 200.0
CTYPE2  = 'GLAT-CAR'
CRVAL2  = -3.5e1
CDELT2  = 1.25e-1
CRPIX2  = 200.0
END
";
        let cards = cards_from(header);
        assert_eq!(infer_axis_count_from_indexed_keywords(&cards), Some(2));
    }

    #[test]
    fn axis_count_returns_none_when_no_indexed_keywords() {
        let header = "\
SIMPLE  =                    T
EXTEND  =                    T
END
";
        let cards = cards_from(header);
        assert_eq!(infer_axis_count_from_indexed_keywords(&cards), None);
    }
```

(`cards_from` is the existing test helper at `wcs_keywords.rs:222`.)

- [ ] **Step 3: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_keywords::tests -- --nocapture
```

All existing tests must pass + the 2 new ones.

- [ ] **Step 4: Run the integration test added in Task 1**

```bash
cargo test -p ast-core --test test_fitschan_wcs car3_naxis_inferred_from_ctype_indices -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_keywords.rs
git commit -m "$(cat <<'EOF'
fix(fitschan): infer axis count from CTYPE/CRVAL/CRPIX/CDELT indices

When WCSAXES, NAXIS, and NAXIS{n} are all absent, scan CTYPE{n} /
CRVAL{n} / CRPIX{n} / CDELT{n} (n in 1..=9) and use the highest index
found as the axis count. car3.head exercises this path.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 3: B1 — CLASS reader NAXIS fallback

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_class.rs`

`enc_class::read_from_cards` errors hard at line 34 when NAXIS is absent. Reuse the helper from Task 2.

- [ ] **Step 1: Update the CLASS reader**

In `enc_class.rs`, find:

```rust
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    // Determine the number of axes from NAXIS.
    let naxis = cards.get_int("NAXIS").map(|v| v as usize).ok_or_else(|| {
        AstError::new(ErrorCode::Bdfts, "NAXIS keyword not found in CLASS header")
    })?;
```

Replace with:

```rust
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    // Determine the number of axes. CLASS headers normally carry NAXIS,
    // but some real-world headers (e.g. ACSIS spectra written by JCMT)
    // omit it. Fall back to scanning indexed CTYPE/CRVAL/CRPIX/CDELT
    // keywords like the C library does in `WcsFcRead`'s NAXIS-absent
    // branch.
    let naxis = cards
        .get_int("NAXIS")
        .map(|v| v as usize)
        .or_else(|| super::wcs_keywords::infer_axis_count_from_indexed_keywords(cards))
        .ok_or_else(|| {
            AstError::new(
                ErrorCode::Bdfts,
                "CLASS header has no NAXIS keyword and no axis-indexed keywords (CTYPEn, CRVALn, ...) to infer axis count from",
            )
        })?;
```

(The `super::wcs_keywords::` path assumes `enc_class.rs` is at `fitschan/enc_class.rs` and `wcs_keywords.rs` is at `fitschan/wcs_keywords.rs` — both children of the same `fitschan` module. Verify the path resolves; if not, adjust.)

- [ ] **Step 2: Run the integration tests**

```bash
cargo test -p ast-core --test test_fitschan_wcs jan_class_no_naxis_keyword -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run the wcsconv suite spot-check**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "jan_fits-class"
```

Expected: no output (both `jan_fits-class_ast` and `jan_fits-class-rt` either pass or fail with a *different* error than "NAXIS keyword not found in CLASS header"). The string-compare half of `jan_fits-class-rt` may still fail because of CLASS-write content gaps tracked under category B (Tier 3); only the *read* error is in scope here.

- [ ] **Step 4: Run existing CLASS tests for regression**

```bash
cargo test -p ast-core --lib fitschan::enc_class
cargo test -p ast-core --test test_fitschan_encodings
```

All must remain green.

- [ ] **Step 5: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_class.rs
git commit -m "$(cat <<'EOF'
fix(fitschan): CLASS reader infers axis count when NAXIS is absent

Reuses wcs_keywords::infer_axis_count_from_indexed_keywords for the
fallback so CLASS headers missing NAXIS (e.g. JCMT/ACSIS jan.fits-class)
read instead of erroring. The wcsconv jan_fits-class_ast and
jan_fits-class-rt fixtures exercise this path.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 4: A3 — non-adjacent celestial axes

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs`

The existing `build_general_iwc_to_world` (at `wcs_read.rs:106-184`) walks axes in FITS index order and pushes per-axis mappings into `parts: Vec<DynMapping>`. When the celestial pair is consecutive (e.g. axes 0 and 1), this works because the celestial mapping is pushed at index 0 and `i` advances to 2 to skip the second axis. When the pair is non-adjacent (e.g. axes 0 and 2 in `axes-swap.head`), the current code's `i = first_cel + 2` skips the WRONG axis: it skips the axis that physically lives at position 1 (the spectral axis WAVE), not the second celestial axis at position 2. This produces a parallel composition with the wrong axis count and ordering.

The C library handles this by walking *output* axes (the CTYPE order), pushing each mapping in CTYPE order, then applying a `PermMap` to remap the parallel-composition output back to FITS-axis-order if needed. Equivalent here: we walk axes in CTYPE/FITS order (already do), but we MUST advance `i` by exactly 1 after pushing the celestial mapping (not by 2), and then handle the "second celestial axis" specially when we encounter it (skip without pushing). After all parts are pushed, if any output reordering is needed, wrap the result in a `PermMap`.

For `axes-swap.head` specifically:
- naxis = 4
- celestial pair: lon=2 (RA), lat=0 (DEC)
- spectral axis: 1 (WAVE)
- linear axis: 3 (STOKES)

The natural assembly order (walk i=0..4, processing each axis) is:
- i=0 (DEC, second celestial): we need to push the celestial mapping HERE on the first encounter (whichever of lon/lat comes first), and skip the second encounter.
- i=1 (WAVE, spectral): push spectral mapping.
- i=2 (RA, first celestial): skip — already handled.
- i=3 (STOKES, linear): push shift.

After parallel composition: parts emit (lon_world, lat_world, spec_world, stokes_world) — but that's already in FITS axis order if we push the celestial mapping such that its outputs are (lon, lat) at the correct positions. **Problem:** the celestial mapping is a fixed 2-in/2-out unit; its outputs are (lon, lat), but for `axes-swap.head` axis 0 is *lat* (DEC) and axis 2 is *lon* (RA), so the composition order is wrong.

**Approach** (mirrors C library):
1. After computing `parts`, the parallel composition has nout axes laid out in the order parts were pushed. Track that order in a `Vec<usize>` of "which FITS axis index is at each output position of the parallel chain".
2. Compare to identity (0..naxis). If different, append a `PermMap` that permutes parallel-output to FITS-axis-order.

The celestial mapping itself outputs `(lon, lat)`. When we push it, we record the two output positions as (lon_axis, lat_axis) — these are the destination FITS-axis indices.

- [ ] **Step 1: Refactor build_general_iwc_to_world**

Replace the body (`wcs_read.rs:111-184`) with:

```rust
fn build_general_iwc_to_world(
    store: &FitsStore,
    celestial: Option<CelestialResult>,
    spectral: Option<SpectralResult>,
    claimed: &[bool],
) -> AstResult<DynMapping> {
    use crate::mappings::PermMap;

    let naxis = store.naxis;

    // Walk axes in FITS index order. For each axis, push the appropriate
    // mapping into `parts`; record which FITS-axis-index that mapping's
    // outputs map to in `output_to_fits` (length = total output axes).
    let mut parts: Vec<DynMapping> = Vec::new();
    let mut output_to_fits: Vec<usize> = Vec::new();

    let mut celestial = celestial;
    let mut spectral = spectral;

    for i in 0..naxis {
        // Celestial axis: emit the celestial mapping when we hit the
        // FIRST of the celestial pair (in FITS order); skip on the
        // second encounter.
        if let Some(cel) = celestial.as_ref() {
            let lon = cel.lon_axis;
            let lat = cel.lat_axis;
            let cel_first = lon.min(lat);
            let cel_second = lon.max(lat);
            if i == cel_first {
                let cel = celestial.take().unwrap();
                parts.push(cel.mapping);
                // The celestial mapping outputs (lon, lat) in that order.
                output_to_fits.push(cel.lon_axis);
                output_to_fits.push(cel.lat_axis);
                continue;
            }
            if i == cel_second {
                continue; // second celestial axis: skipped (already handled)
            }
        }

        // Spectral axis.
        if let Some(spec) = spectral.as_ref() {
            if i == spec.axis {
                let spec = spectral.take().unwrap();
                parts.push(spec.mapping);
                output_to_fits.push(spec.axis);
                continue;
            }
        }

        // Linear/unclaimed axis: emit a CRVAL shift.
        if !claimed[i] {
            let crval = store.crval[i].unwrap_or(0.0);
            let shift = ShiftMap::new(vec![crval]);
            parts.push(Box::new(shift));
            output_to_fits.push(i);
        }
    }

    if parts.is_empty() {
        return Ok(Box::new(UnitMap::new(naxis)));
    }

    // Compose all parts in parallel.
    let mut result: DynMapping = parts.remove(0);
    for part in parts {
        result = Box::new(CmpMap::new(result, part, Composition::Parallel)?);
    }

    // If the parallel-output order differs from FITS axis order, append
    // a PermMap to permute parallel-output back to FITS order.
    let identity_order: Vec<usize> = (0..naxis).collect();
    if output_to_fits != identity_order {
        // PermMap takes 1-based FITS-style indices.
        // outperm[k] = i+1 where output position k of the PermMap should
        // get the value from input position (i+1) of the PermMap. The
        // PermMap is appended after the parallel-composition, so its
        // input is `output_to_fits` order and its output should be FITS
        // axis order.
        // For output position dest_fits_axis, we want the value from the
        // input position whose output_to_fits entry equals dest_fits_axis.
        let mut outperm = vec![0i64; naxis];
        for dest in 0..naxis {
            // Find the parallel-output position whose value should land
            // at FITS axis `dest`.
            let src = output_to_fits
                .iter()
                .position(|&fa| fa == dest)
                .ok_or_else(|| {
                    crate::error::AstError::new(
                        crate::error::ErrorCode::Bdfts,
                        format!(
                            "internal: FITS axis {dest} not produced by any decoder; \
                             output_to_fits = {output_to_fits:?}"
                        ),
                    )
                })?;
            outperm[dest] = (src + 1) as i64;
        }
        // Inverse permutation.
        let mut inperm = vec![0i64; naxis];
        for src in 0..naxis {
            let dest_fits = output_to_fits[src];
            inperm[src] = (dest_fits + 1) as i64;
        }
        let perm = PermMap::new(&inperm, &outperm, &[])?;
        result = Box::new(CmpMap::new(result, Box::new(perm), Composition::Series)?);
    }

    Ok(result)
}
```

You will need to confirm `PermMap` is in scope. Add `use crate::mappings::PermMap;` if not already present (it's currently not). The existing imports at the top of `wcs_read.rs` already pull in `CmpMap`, `Composition`, `ShiftMap`, `UnitMap`, `DynMapping`.

- [ ] **Step 2: Run the integration test for axes-swap**

```bash
cargo test -p ast-core --test test_fitschan_wcs axes_swap_non_adjacent_celestial_axes -- --nocapture
```

Expected: PASS.

If it still fails: the error message tells you what's wrong. Common modes:
- `series CmpMap: first.nout()=N but second.nin()=M` — the PermMap dimensions disagree with the parallel-composition output count. Add `eprintln!("output_to_fits = {output_to_fits:?}, naxis = {naxis}")` before the PermMap construction and inspect.
- `outperm entry refers to input axis X but nin = Y` — a permutation entry is out of range. Likely a typo in the index translation between 0-based and 1-based.

- [ ] **Step 3: Run the existing wcs_read tests for regression**

The existing tests cover the consecutive-celestial case and 1D/2D shortcuts (which take an earlier return path). Confirm:

```bash
cargo test -p ast-core --lib fitschan::wcs_read::tests
cargo test -p ast-core --test test_fitschan_wcs
cargo test -p ast-core --test test_fitschan_wcs_write
```

All must remain green.

- [ ] **Step 4: Run the wcsconv suite spot-check**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "axes-swap"
```

Expected: no failure for `axes-swap_native`. Other 2D celestial+spectral or 3D fixtures that previously worked must still work.

- [ ] **Step 5: Add a unit test for the PermMap path**

In `wcs_read.rs::tests`, add:

```rust
    #[test]
    fn build_pixel_to_world_handles_non_adjacent_celestial_axes() {
        // FITS axes: 0=DEC, 1=WAVE, 2=RA. Celestial pair is non-adjacent
        // (separated by spectral axis). Reader must produce a working
        // 3D pixel-to-world mapping.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("DEC--TAN".to_string());
        store.ctype[1] = Some("WAVE    ".to_string());
        store.ctype[2] = Some("RA---TAN".to_string());
        store.crval[0] = Some(22.0);
        store.crval[1] = Some(5.0e-7);
        store.crval[2] = Some(83.6);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(25.5);
        store.crpix[2] = Some(50.5);
        store.cdelt[0] = Some(2.78e-4);
        store.cdelt[1] = Some(1.0e-10);
        store.cdelt[2] = Some(-2.78e-4);
        store.cunit[1] = Some("m".to_string());

        let obj = decode_wcs(&store).expect("decode succeeds");
        let AnyObject::FrameSet(ref fs) = obj else {
            panic!("expected FrameSet")
        };
        assert_eq!(fs.base().naxes(), 3);
        assert_eq!(fs.current().naxes(), 3);

        // At the reference pixel, output axis 0 is DEC (latitude in
        // radians, since SkyFrame stores radians internally) ~ 22°,
        // and output axis 2 is RA (longitude in radians) ~ 83.6°.
        let pts = PointSet::from_columns(&[&[50.5], &[25.5], &[50.5]]).unwrap();
        let out = fs.transform(&pts, true).unwrap();
        let point = out.point(0);
        let dec_deg = point[0].to_degrees();
        let ra_deg = point[2].to_degrees();
        assert!(
            (dec_deg - 22.0).abs() < 1.0,
            "axis 0 (DEC) at refpix expected ~22°, got {dec_deg}°"
        );
        assert!(
            (ra_deg - 83.6).abs() < 1.0,
            "axis 2 (RA) at refpix expected ~83.6°, got {ra_deg}°"
        );
    }
```

- [ ] **Step 6: Run the new unit test**

```bash
cargo test -p ast-core --lib fitschan::wcs_read::tests::build_pixel_to_world_handles_non_adjacent_celestial_axes -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Clippy / fmt clean**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_read.rs
git commit -m "$(cat <<'EOF'
fix(fitschan): handle non-adjacent celestial axes in pixel-to-world build

The previous build_general_iwc_to_world advanced i by 2 after pushing
the celestial mapping, assuming the celestial pair was consecutive.
For headers like axes-swap.head where the pair is separated by a
non-celestial axis (DEC at index 0, WAVE at 1, RA at 2), this skipped
the wrong axis and produced a malformed parallel composition.

Replaced with a single-step walk that pushes the celestial mapping at
the first encountered celestial axis, skips the second, and tracks
each pushed mapping's output FITS-axis indices. After parallel
composition, append a PermMap when the natural parallel-output order
doesn't match FITS axis order. Mirrors the C library's `WcsMapFrm`
output-axis-permutation step.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 5: Final verification

**Files:** none

- [ ] **Step 1: Run the wcsconv suite**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: `=== wcsconv results: 111 passed, 81 failed, 0 skipped (of 192 total) ===` (108 → 111: +1 from `car3_native`, +1 from `axes-swap_native`, +1 from `jan_fits-class_ast`. `jan_fits-class-rt` decodes successfully but its write-side string-compare may still fail under category B Tier-3 work — that's expected.)

If only some pass, capture which still fail:

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "car3|axes-swap|jan_fits"
```

A still-failing case needs investigation — check the error message against the fix's intended behaviour.

- [ ] **Step 2: Full workspace tests**

```bash
cargo test --workspace 2>&1 | grep -E "test result|FAILED" | head -20
```

All test suites must still pass. The A3 PermMap insertion changes the shape of the produced mapping for all multi-axis non-pure-celestial cases — verify this doesn't perturb existing tests.

- [ ] **Step 3: Workspace clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Clean.

- [ ] **Step 4: No commit needed if no further code changes**

If verification revealed nothing to fix, stop. The plan is complete.

---

## Self-review checklist (run before declaring done)

- [ ] **Spec coverage:** A2 + A3 + B1 from the roadmap each have a dedicated task (Tasks 2, 4, 3 respectively); Task 1 is the red-test commit and Task 5 is the verification step.
- [ ] **No placeholders:** every step has actual code, file paths, commands, expected outputs.
- [ ] **Type consistency:** `infer_axis_count_from_indexed_keywords` (introduced Task 2) is referenced by name in Task 3. The function name is consistent.
- [ ] **Frequent commits:** 5 commits (1 red test + 3 fixes + final verification only adds work if needed).
- [ ] **Done criteria:** wcsconv goes 108 → 111 (`jan_fits-class-rt` may stay red for write-side reasons unrelated to this plan).

---

## What this plan does NOT cover

- **CLASS write side** (`jan_fits-class-rt` string compare, the seven `class-*_fits-class` tests with wrong CTYPE order): tracked under category B (Tier 3) of the roadmap; separate plan.
- **Permutation handling for celestial-pair internal swap** (e.g. CTYPE1=DEC, CTYPE2=RA at *consecutive* indices but reverse order): not exercised by `axes-swap.head` (which has DEC at axis 0 and RA at axis 2). If a future fixture has a swapped consecutive pair, it'll need additional logic in the celestial-mapping output ordering. Out of scope here.
- **More than 9 axes** (`infer_axis_count_from_indexed_keywords` scans 1..=9): real WCS headers don't exceed 9. Defer until/unless a fixture needs it.

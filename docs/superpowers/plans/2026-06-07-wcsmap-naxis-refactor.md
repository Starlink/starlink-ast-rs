# N-axis WcsMap construction refactor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the Rust read-side mapping construction in `fitschan/wcs_*` into line with C `WcsMapFrm`'s shape — each axis-class contribution returns an N-axis mapping, then `build_iwc_to_world` composes them in series — so `astSimplify` can collapse the integrity-injected SlaMap chain that currently leaves `wcsconv_cases::sip_3d_swap_fits_wcs` failing with a 19-mapping pre-simplify chain that reduces to 9 instead of 3.

**Architecture:** Today, each decoder returns its mapping at the smallest natural rank (celestial = 2-axis, spectral = 1-axis), and `build_general_iwc_to_world` glues them together with `Composition::Parallel` plus an output-`PermMap`. After the refactor, each decoder produces an `N`-axis mapping by embedding the small mapping inside a `PermMap`-sandwich-around-`Parallel`-`CmpMap` (the C-equivalent shape — see `fitschan.c:38315-38358`), and `build_iwc_to_world` composes them with `Composition::Series`. The output-permutation step disappears.

**Tech Stack:** Rust 2024 edition, ast-core crate. Existing types: `WcsMap`, `MatrixMap`, `ZoomMap`, `UnitMap`, `PermMap`, `CmpMap`, `Composition::{Series, Parallel}`, `DynMapping`. Test surface: `cargo test -p ast-core --lib`, `cargo test -p ast-core --test test_wcsconv`, `cargo test --workspace`.

**Source spec:** `docs/issues/wcsmap-naxis-refactor.md` (referred to below as "the spec"). Read it before starting.

**Authority rule:** the C library is authoritative. If anywhere in
this plan a step asks you to preserve the existing Rust shape because
it "already works" or "the rest of the code expects it", and the C
source disagrees with that shape — the C source wins. Discard the
Rust shape and restructure the callers to match C. See memory note
[[c-logic-beats-existing-rust]]. The relevant C entry points are
`fitschan.c::WcsMapFrm` (line 37632), `WcsCelestial` (35458),
`WcsNative` (37978), `WcsSpectral` (39364), `SplitMap2` (32850-33140).

**Phase structure (from brainstorming):**
- Phase A — N-axis WcsMap construction (spec Step 1)
- Phase B — N-axis embedding for celestial + spectral (spec Steps 2-3)
- Phase C — Series composition + writer-side audit (spec Steps 4-5)
- Phase D — Idiomatic-Rust cleanup (spec Step 7)

**Hard invariant (regression policy):** the wcsconv pass count must stay ≥ **181/192** at every commit. Establish this baseline before Phase A:

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
# Expected: test result: FAILED. 181 passed; 11 failed; 0 ignored; 0 measured; 0 filtered out
```

If a step drops the count below 181, **stop**, diagnose, and either fix in place or revert the offending step's commit before proceeding. The headline success criterion is `wcsconv_cases::sip_3d_swap_fits_wcs` flipping green by the end of Phase C; pass count rising to 182/192.

**Out of scope** (do NOT touch — these have separate root causes per `docs/issues/wcsconv-residual-classification.md`): DSS-encoded fixtures (`dss_ast_*`, `dss_fits_*`); `offset_sky_*` fixtures; `jan_fits_class_*` and `class_write_fits_class`; `degen1_fits_wcs`. Their failure modes are encoder/CTYPE issues unaffected by this refactor.

---

## File Structure

| File | Responsibility | Phases that touch it |
|---|---|---|
| `crates/ast-core/src/fitschan/wcs_celestial.rs` | Celestial decoder: `build_celestial_with_def_b1950` returns `CelestialResult { mapping, frame, lon_axis, lat_axis }`. `build_iwc_to_sky_mapping` builds the celestial 2-axis chain (Zoom → WcsMap_inv → rotation). | A, B, D |
| `crates/ast-core/src/fitschan/wcs_spectral.rs` | Spectral decoder: `build_spectral` returns `SpectralResult { mapping, frame, axis }` with a 1-axis mapping. | B, D |
| `crates/ast-core/src/fitschan/wcs_read.rs` | Composer: `build_iwc_to_world` and the helper `build_general_iwc_to_world` assemble the per-class mappings into the full IWC→world mapping. | C |
| `crates/ast-core/src/fitschan/fset_to_store/celestial_axes.rs` | Writer-side: `find_wcsmap`, `split_around_wcsmap`, `split_map2*` walk the FrameSet's pixel→WCS mapping looking for a WcsMap. **No code change expected** — the helpers already handle WcsMap at any depth — but Phase C verifies behaviour. | C (audit only, in-task verification) |

The refactor stays inside `fitschan/`. No new public API surface, no new modules, no changes to `mappings/` (`WcsMap::new` already supports any `ncoord >= 2`; this is the lever the refactor exploits).

---

## Conventions used by every task

- **Compose-in-series:** `CmpMap::new(a, b, Composition::Series).ok()?` chains `a` then `b`.
- **Compose-in-parallel:** `CmpMap::new(a, b, Composition::Parallel).ok()?` runs `a` on the first `nin(a)` axes and `b` on the next `nin(b)` axes.
- **Axis indices:** `lon_axis` and `lat_axis` in `CelestialResult` and `axis` in `SpectralResult` are **0-based**. `WcsMap::new(ncoord, proj, lonax, latax)` takes **1-based** lon/lat. `PermMap::new(inperm, outperm, constants)` takes **1-based** signed indices in the perm arrays (positive → axis index, negative → constant index).
- **Bad-value semantics:** outputs use `BAD_VALUE = -f64::MAX`; inputs may be `NaN`. The N-axis embedding uses `UnitMap` for the pass-through axes, which is bad-value-safe.
- **Commit cadence:** every numbered task ends with a commit so each task is independently revertable. Commit message style: imperative, lower-case, no Conventional-Commit type prefix (matches recent repo history — `mappings/simplify: relax …`, `frameset: port C astRemapFrame …`).

Before every commit:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

(Per the project's `feedback_format_clippy` memory.)

---

## Phase A — N-axis WcsMap construction (spec Step 1)

**Goal:** Make `build_celestial_with_def_b1950` build the WcsMap with the full pipeline rank (`store.naxis`), pointing its 1-based `lonax` / `latax` at the FITS celestial axes. The remaining celestial-side scaffolding (Zoom, rotation, output PermMap-sandwich) is unchanged in this phase — it stays 2-axis and continues to be wrapped by `build_general_iwc_to_world`'s parallel composer. The phase's only job is: prove that going from a 2-axis WcsMap to an N-axis WcsMap with two non-passthrough axes is a no-op for the test surface.

**Why this works as an isolated phase:** The downstream pipeline still expects the celestial mapping to be 2-input/2-output (because `build_general_iwc_to_world` wraps it in `Parallel` with `nin = 2`). So the WcsMap's "extra" pass-through axes have to be peeled off again *inside `build_iwc_to_sky_mapping`* before the result leaves the function. That peel-off is implemented as a `PermMap` sandwich — the same kind of sandwich Phase B will move outwards to enclose the whole chain. We're temporarily building extra structure that Phase B will simplify away; the win is that Phase A is testable in isolation against the full wcsconv suite.

### Task A1: Capture baseline & confirm starting point

**Files:** none modified.

- [ ] **Step 1: Capture wcsconv baseline**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```
Expected: `test result: FAILED. 181 passed; 11 failed; 0 ignored; 0 measured; 0 filtered out`

- [ ] **Step 2: Capture lib-test baseline**

```bash
cargo test -p ast-core --lib 2>&1 | grep "test result" | tail -1
```
Expected: `test result: ok. 908 passed; 0 failed; 0 ignored; …` (record the exact number for later comparison; the spec quotes 908 but exact value can drift with unrelated commits).

- [ ] **Step 3: Capture the headline failure's pre-simplify chain length**

```bash
cargo test -p ast-core --test test_wcsconv -- --exact wcsconv_cases::sip_3d_swap_fits_wcs --nocapture 2>&1 | tail -40
```
Expected: failure with a chain-length / mapping-count mismatch. Save the output for comparison with the post-Phase-C run.

No commit (this is just snapshotting state).

---

### Task A2: Pass `naxis` into `build_iwc_to_sky_mapping` and `WcsMap::new`

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs`

**Why:** Spec Step 1 — `WcsMap::new(2, proj, 1, 2)` becomes `WcsMap::new(naxis, proj, lon_axis + 1, lat_axis + 1)`. The `+ 1` converts 0-based `lon_axis` / `lat_axis` to the WcsMap's 1-based convention. We also need `build_iwc_to_sky_mapping` to know `naxis` so it can build the temporary peel-back PermMap sandwich.

- [ ] **Step 1: Add a regression test for the N-axis construction**

Append to the `tests` module of `crates/ast-core/src/fitschan/wcs_celestial.rs` (just before the closing `}` of `mod tests`):

```rust
    #[test]
    fn naxis_wcsmap_built_with_full_rank() {
        // 3-axis FITS header with celestial pair at axes 0, 1 and a
        // spectral axis at 2. The decoder should ultimately still
        // return a 2-input/2-output mapping (Phase A invariant), but
        // the inner WcsMap's `ncoord` must reflect the full naxis.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crval[2] = Some(1.4e9);

        let result = build_celestial(&store).expect("celestial axes recognised");
        assert_eq!(result.lon_axis, 0);
        assert_eq!(result.lat_axis, 1);
        // Phase A invariant: the outer mapping is still 2D (the
        // peel-back sandwich removes the extra pass-through axis).
        assert_eq!(result.mapping.nin(), 2);
        assert_eq!(result.mapping.nout(), 2);
    }
```

- [ ] **Step 2: Run the new test and confirm it passes (the existing 2-axis path satisfies the Phase A invariant trivially)**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial::tests::naxis_wcsmap_built_with_full_rank
```
Expected: PASS. (We're testing the invariant *first* so Phase A's refactor demonstrably preserves it.)

- [ ] **Step 3: Edit `build_iwc_to_sky_mapping` signature and `build_celestial_with_def_b1950` callsite to thread `naxis`**

In `crates/ast-core/src/fitschan/wcs_celestial.rs`:

Find the existing definition (line 474):
```rust
fn build_iwc_to_sky_mapping(
    store: &FitsStore,
    wcsmap: WcsMap,
    lon_axis: usize,
    lat_axis: usize,
) -> Option<DynMapping> {
```

Change to:
```rust
fn build_iwc_to_sky_mapping(
    store: &FitsStore,
    wcsmap: WcsMap,
    naxis: usize,
    lon_axis: usize,
    lat_axis: usize,
) -> Option<DynMapping> {
```

Find the call at line 315:
```rust
    let inner = build_iwc_to_sky_mapping(store, wcsmap, lon_axis, lat_axis)?;
```

Change to:
```rust
    let inner = build_iwc_to_sky_mapping(store, wcsmap, store.naxis, lon_axis, lat_axis)?;
```

- [ ] **Step 4: Build N-axis WcsMap and add a 2-axis peel-back inside `build_iwc_to_sky_mapping`**

In `build_celestial_with_def_b1950`, find the WcsMap construction (line 118):
```rust
    let mut wcsmap = WcsMap::new(2, proj, 1, 2).ok()?;
```

Change to:
```rust
    // N-axis WcsMap: ncoord = full naxis, with this pair's lon/lat
    // axes pointed at FITS positions (1-based). The pass-through
    // axes here are conceptually for the rest of the pipeline; for
    // Phase A they're peeled back inside build_iwc_to_sky_mapping
    // so the outer mapping stays 2D. Phase B/C remove the peel-back.
    let mut wcsmap = WcsMap::new(store.naxis, proj, lon_axis + 1, lat_axis + 1).ok()?;
```

The PV-parameter loops directly below currently use 1-based axis = 1 or 2 to index into `wcsmap.set_pv`. With the N-axis WcsMap they must use the 1-based FITS axis indices. Find lines 122-135:
```rust
    for m in 0..store.pv[lat_axis].len() {
        if let Some(val) = store.pv[lat_axis][m] {
            // WcsMap set_pv uses 1-based axis: latitude is axis 2 in our 2D WcsMap.
            let _ = wcsmap.set_pv(2, m, val);
        }
    }
    // PV parameters on the longitude axis.
    for m in 0..store.pv[lon_axis].len() {
        if let Some(val) = store.pv[lon_axis][m] {
            // longitude is axis 1 in our 2D WcsMap.
            let _ = wcsmap.set_pv(1, m, val);
        }
    }
```

Change to:
```rust
    for m in 0..store.pv[lat_axis].len() {
        if let Some(val) = store.pv[lat_axis][m] {
            // WcsMap set_pv uses 1-based FITS axis indices.
            let _ = wcsmap.set_pv(lat_axis + 1, m, val);
        }
    }
    // PV parameters on the longitude axis.
    for m in 0..store.pv[lon_axis].len() {
        if let Some(val) = store.pv[lon_axis][m] {
            let _ = wcsmap.set_pv(lon_axis + 1, m, val);
        }
    }
```

The LONPOLE/LATPOLE → PVi_3 / PVi_4 propagation block (lines 144-155) uses literal axis `1`. It must use the 1-based FITS longitude axis. Find:
```rust
        if wcsmap.get_pv(1, 3).is_none()
            && let Some(lon_deg) = store.lonpole
        {
            let _ = wcsmap.set_pv(1, 3, lon_deg * deg2rad_local);
        }
        if wcsmap.get_pv(1, 4).is_none()
            && let Some(lat_deg) = store.latpole
        {
            let _ = wcsmap.set_pv(1, 4, lat_deg * deg2rad_local);
        }
```

Change to:
```rust
        if wcsmap.get_pv(lon_axis + 1, 3).is_none()
            && let Some(lon_deg) = store.lonpole
        {
            let _ = wcsmap.set_pv(lon_axis + 1, 3, lon_deg * deg2rad_local);
        }
        if wcsmap.get_pv(lon_axis + 1, 4).is_none()
            && let Some(lat_deg) = store.latpole
        {
            let _ = wcsmap.set_pv(lon_axis + 1, 4, lat_deg * deg2rad_local);
        }
```

- [ ] **Step 5: Add the peel-back PermMap sandwich inside `build_iwc_to_sky_mapping`**

The interior of `build_iwc_to_sky_mapping` currently does (paraphrased, around lines 491-595):

```rust
    let zoom = ZoomMap::new(2, deg2rad).ok()?;
    let wcsmap_fwd = wcsmap.clone();
    let mut wcsmap_inv = wcsmap;
    wcsmap_inv.invert();
    // ... rotation = build_native_to_celestial_rotation_for_wcsmap(...)
    let step1 = CmpMap::new(Box::new(zoom), Box::new(wcsmap_inv), Composition::Series).ok()?;
    let full = CmpMap::new(Box::new(step1), rotation, Composition::Series).ok()?;
    Some(Box::new(full))
```

`zoom` is 2D and `wcsmap_inv` is N-D after the change in Step 4. We must lift `zoom` to N-D *or* add a peel-back PermMap that restricts the WcsMap to act on just 2 axes.

For Phase A, choose the **PermMap peel-back** form because it keeps the rest of `build_iwc_to_sky_mapping` byte-identical (same ZoomMap, same rotation, same composition):

Replace the composition tail (the `step1 = ...` and `full = ...` lines, ~590-594) with:

```rust
    // Phase A invariant: the outer mapping is 2-input/2-output
    // (Phase B converts to fully N-axis). The WcsMap is now N-axis,
    // so sandwich it between a "lift 2D into the N-D slot" PermMap
    // and a "drop back to 2D" PermMap, keeping the wider WcsMap.
    //
    // Lift PermMap: 2 inputs, naxis outputs. Output `lon_axis` (0-
    // based) takes input 0 (the lon component); output `lat_axis`
    // takes input 1; every other output takes constant 0.0 (the
    // pass-through axes never reach the WcsMap's lon/lat math).
    //
    // Drop PermMap: naxis inputs, 2 outputs. Output 0 (lon) takes
    // input `lon_axis`; output 1 (lat) takes input `lat_axis`. The
    // other inputs are discarded.
    // lift: nin = 2 (zoom outputs), nout = naxis (WcsMap inputs).
    // Forward: outperm[lon_axis] = 1 (lon-rad), outperm[lat_axis] = 2
    // (lat-rad), other slots get constant 0.0.
    // Inverse: inperm[0] = lon_axis + 1 (recover lon from forward
    // output slot lon_axis), inperm[1] = lat_axis + 1.
    let lift_inperm: Vec<i64> = vec![(lon_axis as i64) + 1, (lat_axis as i64) + 1];
    let mut lift_outperm: Vec<i64> = vec![-1; naxis]; // -1 = constant 0.0
    lift_outperm[lon_axis] = 1;
    lift_outperm[lat_axis] = 2;
    let lift = PermMap::new(&lift_inperm, &lift_outperm, &[0.0]).ok()?;

    let mut drop_inperm: Vec<i64> = vec![-1; naxis];
    drop_inperm[lon_axis] = 1;
    drop_inperm[lat_axis] = 2;
    let drop_outperm: Vec<i64> = vec![(lon_axis as i64) + 1, (lat_axis as i64) + 1];
    let drop = PermMap::new(&drop_inperm, &drop_outperm, &[0.0]).ok()?;

    // Pipeline: zoom (2D) → lift (2→N) → wcsmap_inv (N→N) → drop (N→2) → rotation (2→2)
    let zoom_then_lift =
        CmpMap::new(Box::new(zoom), Box::new(lift), Composition::Series).ok()?;
    let with_wm = CmpMap::new(
        Box::new(zoom_then_lift),
        Box::new(wcsmap_inv),
        Composition::Series,
    )
    .ok()?;
    let with_drop =
        CmpMap::new(Box::new(with_wm), Box::new(drop), Composition::Series).ok()?;
    let full = CmpMap::new(Box::new(with_drop), rotation, Composition::Series).ok()?;
    Some(Box::new(full))
```

PermMap semantics for context: `PermMap::new(inperm, outperm, constants)` — `inperm[i]` says the inverse-direction's `i`-th *output* takes which forward-input (1-based) or constant (negative); `outperm[j]` says the forward-direction's `j`-th *output* takes which forward-input (1-based) or constant (negative). For lift, the forward direction has 2 inputs (zoom outputs) and `naxis` outputs (WcsMap inputs); for drop, the forward direction has `naxis` inputs and 2 outputs.

- [ ] **Step 6: Run the targeted test, the existing celestial tests, and the new naxis test**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial::tests
```
Expected: PASS for all 9 (or however many) existing tests in the module + PASS for `naxis_wcsmap_built_with_full_rank`.

- [ ] **Step 7: Run the full lib test suite**

```bash
cargo test -p ast-core --lib 2>&1 | grep "test result" | tail -1
```
Expected: same pass count as the baseline captured in Task A1 Step 2 (≥ 908). If lower, **stop** — diagnose before continuing.

- [ ] **Step 8: Run the full wcsconv suite**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```
Expected: `181 passed; 11 failed`. If pass count drops below 181, **stop** per the regression policy and revert.

- [ ] **Step 9: Format and lint**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```
Both must succeed.

- [ ] **Step 10: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_celestial.rs
git commit -m "fitschan/wcs_celestial: build N-axis WcsMap with FITS-axis lon/lat indices"
```

---

## Phase B — N-axis embedding for celestial + spectral (spec Steps 2-3)

**Goal:** Lift the celestial chain (Zoom + WcsMap_inv + rotation) to N-axis end-to-end, and lift the spectral chain to N-axis end-to-end. After this phase, both `CelestialResult.mapping` and `SpectralResult.mapping` are full-rank — `nin == nout == naxis` — with non-celestial / non-spectral axes passing through untouched. The rest of `build_general_iwc_to_world` is unchanged in this phase; it sees N-axis inputs but still parallel-composes them, which is wrong but transient (Phase C fixes it).

**Why this works as an isolated phase:** A parallel composition of two N-axis identity-on-the-other-axes mappings is *equivalent* to a series composition: each parallel branch only touches its own axes, so series and parallel produce the same outputs at every point. The wcsconv suite stays at ≥ 181/192 because nothing about the *math* changes — the only change is the *shape* of the mapping graph. The simplifier may briefly produce a worse result for some intermediate stages because the parallel-of-N-axis structure is novel; that's why Phase C immediately follows.

**During this phase, every test must still pass.** If a test fails it's a Phase-B implementation bug (wrong axis index on the lift sandwich), not a "wait for Phase C" situation.

### Task B1: Replace celestial 2-axis Zoom with N-axis diagonal MatrixMap

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs`

**Why:** Spec Step 2 — instead of `ZoomMap(2, deg2rad)`, build a `MatrixMap` whose diagonal has `deg2rad` on `lon_axis` and `lat_axis` and `1.0` elsewhere. This mirrors C `fitschan.c:35831-35839`. `MatrixMap` recognises this as `MatrixForm::Diagonal` (see `mappings/matrix.rs:33-34, 143`), so it's still O(N) per point.

- [ ] **Step 1: Write failing test for the N-axis Zoom (round-trip identity on pass-through axes)**

Append to the `tests` module of `crates/ast-core/src/fitschan/wcs_celestial.rs`:

```rust
    #[test]
    fn naxis_celestial_passthrough_axes_are_identity() {
        // 3-axis: celestial pair at 0, 1; spectral at 2. After Phase B,
        // the celestial mapping operates as identity on axis 2.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crval[2] = Some(1.4e9);

        let result = build_celestial(&store).expect("celestial axes recognised");
        assert_eq!(result.mapping.nin(), 3);
        assert_eq!(result.mapping.nout(), 3);

        // Send IWC (0, 0, 12345.0) through. axes 0, 1 should land on
        // CRVAL in radians; axis 2 should pass through unchanged.
        let pts = PointSet::from_columns(&[&[0.0], &[0.0], &[12345.0]]).unwrap();
        let out = result.mapping.transform(&pts, true).unwrap();
        let p = out.point(0);
        assert!((p[0] - 180.0_f64.to_radians()).abs() < 1e-6);
        assert!((p[1] - 45.0_f64.to_radians()).abs() < 1e-6);
        assert!((p[2] - 12345.0).abs() < 1e-12, "axis 2 was {}", p[2]);
    }
```

- [ ] **Step 2: Run the test and confirm it FAILS**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial::tests::naxis_celestial_passthrough_axes_are_identity
```
Expected: FAIL — currently `result.mapping.nin() == 2`. (Update: also update `naxis_wcsmap_built_with_full_rank` to assert `nin == 3, nout == 3` instead of 2 — the Phase A invariant has been superseded.)

Edit the existing `naxis_wcsmap_built_with_full_rank` assertions:
```rust
        // Phase A invariant: the outer mapping is still 2D (the
        // peel-back sandwich removes the extra pass-through axis).
        assert_eq!(result.mapping.nin(), 2);
        assert_eq!(result.mapping.nout(), 2);
```
to:
```rust
        // Phase B invariant: the outer mapping is now full-rank.
        assert_eq!(result.mapping.nin(), 3);
        assert_eq!(result.mapping.nout(), 3);
```

- [ ] **Step 3: Replace 2-axis ZoomMap with N-axis diagonal MatrixMap**

In `build_iwc_to_sky_mapping`, find:
```rust
    // Step 1: Degrees to radians (2D zoom).
    let zoom = ZoomMap::new(2, deg2rad).ok()?;
```

Replace with:
```rust
    // Step 1: Degrees to radians on the celestial axes only; identity
    // elsewhere. Diagonal MatrixMap, recognised by MatrixMap::form()
    // as MatrixForm::Diagonal — O(N) per point. Mirrors C
    // fitschan.c:35831-35839.
    use ndarray::Array2;
    let mut zoom_mat: Array2<f64> = Array2::eye(naxis);
    zoom_mat[[lon_axis, lon_axis]] = deg2rad;
    zoom_mat[[lat_axis, lat_axis]] = deg2rad;
    let zoom = crate::mappings::MatrixMap::new(zoom_mat).ok()?;
```

Note: `ZoomMap` is no longer used here. Remove the `ZoomMap` import from `wcs_celestial.rs:18` if it has no other use *in this file* — but check: it's currently in the brace-list `use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, PermMap, WcsMap, ZoomMap};`. Run `grep -n ZoomMap crates/ast-core/src/fitschan/wcs_celestial.rs` first; if no other reference, delete `ZoomMap` from the import list. Otherwise leave it.

- [ ] **Step 4: Update the rotation to be N-axis via PermMap-around-Parallel-CmpMap sandwich**

The intrinsic rotation is 2-axis (`SphMap_inv ; MatrixMap_3x3 ; SphMap_fwd` produced by `build_native_to_celestial_rotation_for_wcsmap`). Embed it. Find the existing `rotation = ...` call and the composition tail (after the Step 5 changes from Phase A — i.e. the `lift` / `drop` / `with_drop` / `full` block).

Replace the entire composition tail (everything from the `Phase A invariant` comment block through the `Some(Box::new(full))`) with:

```rust
    // The rotation is intrinsically 2-axis. Embed it in N-axis form
    // via PermMap + parallel CmpMap (with UnitMap_(N-2) on the other
    // axes) + inverse PermMap. Mirrors C fitschan.c:38315-38358
    // (the nax_rem > 0 branch and the surrounding PermMap sandwich
    // at 38325-38346).
    use crate::mappings::UnitMap;
    let rotation_naxis: DynMapping = if naxis == 2 {
        rotation
    } else {
        // perm_to_2d: naxis-input → naxis-output PermMap that puts
        // (lon_axis, lat_axis) into the first two output slots and
        // the remaining (naxis-2) axes into slots 2..naxis,
        // preserving their original FITS order.
        // perm_back: the inverse permutation.
        let mut others: Vec<usize> = (0..naxis)
            .filter(|&i| i != lon_axis && i != lat_axis)
            .collect();
        // Output slots 0, 1 = lon_axis, lat_axis (FITS-relative);
        // slots 2..naxis come from `others` in order.
        // Build perm_to_2d.outperm[j] = (input axis at slot j) + 1.
        let mut outperm_to_2d: Vec<i64> = Vec::with_capacity(naxis);
        outperm_to_2d.push((lon_axis as i64) + 1);
        outperm_to_2d.push((lat_axis as i64) + 1);
        for &k in &others {
            outperm_to_2d.push((k as i64) + 1);
        }
        // Inverse: build inperm[i] for forward = the slot that
        // produced original input i. We construct the back-perm
        // independently and let PermMap derive its own inverse.
        let mut inperm_to_2d: Vec<i64> = vec![0; naxis];
        for (slot_idx, &input_idx) in [lon_axis, lat_axis]
            .iter()
            .chain(others.iter())
            .enumerate()
        {
            inperm_to_2d[input_idx] = (slot_idx as i64) + 1;
        }
        let perm_to_2d = PermMap::new(&inperm_to_2d, &outperm_to_2d, &[]).ok()?;

        // Inverse permutation: outperm_back[i] = "which slot j of the
        // permuted layout holds original axis i".
        let mut outperm_back: Vec<i64> = vec![0; naxis];
        outperm_back[lon_axis] = 1;
        outperm_back[lat_axis] = 2;
        for (k, &orig) in others.iter().enumerate() {
            outperm_back[orig] = (k as i64) + 3;
        }
        let mut inperm_back: Vec<i64> = Vec::with_capacity(naxis);
        inperm_back.push((lon_axis as i64) + 1);
        inperm_back.push((lat_axis as i64) + 1);
        for &k in &others {
            inperm_back.push((k as i64) + 1);
        }
        let perm_back = PermMap::new(&inperm_back, &outperm_back, &[]).ok()?;

        // Parallel: rotation (2-axis) || UnitMap(naxis - 2)
        let unit_rest = UnitMap::new(naxis - 2);
        let parallel = CmpMap::new(rotation, Box::new(unit_rest), Composition::Parallel).ok()?;

        // PermMap sandwich: perm_to_2d ; parallel ; perm_back
        let s1 =
            CmpMap::new(Box::new(perm_to_2d), Box::new(parallel), Composition::Series).ok()?;
        let s2 = CmpMap::new(Box::new(s1), Box::new(perm_back), Composition::Series).ok()?;
        Box::new(s2)
    };

    // Pipeline: zoom (N→N diagonal) → wcsmap_inv (N→N) → rotation_naxis (N→N)
    let step1 =
        CmpMap::new(Box::new(zoom), Box::new(wcsmap_inv), Composition::Series).ok()?;
    let full = CmpMap::new(Box::new(step1), rotation_naxis, Composition::Series).ok()?;
    Some(Box::new(full))
```

Delete the `wcsmap_fwd` clone if the rotation builder doesn't need it after this change. (Re-check: it does — `build_native_to_celestial_rotation_for_wcsmap(&wcsmap_fwd, …)` uses it for `wcs_nat_pole`. Keep the clone.)

- [ ] **Step 5: Run the targeted celestial tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial::tests
```
Expected: all PASS, including the new `naxis_celestial_passthrough_axes_are_identity`.

- [ ] **Step 6: Update existing 2-axis `reference_pixel_maps_to_crval` if needed**

Re-read `reference_pixel_maps_to_crval` (~line 835): it uses a 2-axis FitsStore. With Phase B's N-axis change, a 2-axis FitsStore still produces a 2D mapping (because `naxis == 2` and the `if naxis == 2 { rotation }` branch is taken). The test should still PASS unchanged. If it FAILS, the construction has a 2-axis-special-case bug — fix before proceeding.

- [ ] **Step 7: Run lib + wcsconv**

```bash
cargo test -p ast-core --lib 2>&1 | grep "test result" | tail -1
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```
Expected: lib tests at baseline; wcsconv at `181 passed; 11 failed`. If wcsconv drops, **stop**.

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_celestial.rs
git commit -m "fitschan/wcs_celestial: lift celestial chain to N-axis end-to-end"
```

---

### Task B2: Lift spectral mapping to N-axis

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_spectral.rs`

**Why:** Spec Step 3. Currently `SpectralResult.mapping` is 1-axis. After this task, it's N-axis with the spectral component on `axis` and identity on the rest.

- [ ] **Step 1: Write failing test for N-axis spectral pass-through**

Append to the `tests` module of `crates/ast-core/src/fitschan/wcs_spectral.rs`:

```rust
    #[test]
    fn naxis_spectral_passthrough_other_axes() {
        use crate::coords::PointSet;
        use crate::mappings::Mapping;

        // 3-axis: celestial pair at 0, 1; spectral at 2.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[2] = Some(1.4e9);

        let spec = build_spectral(&store).expect("spectral axis recognised");
        assert_eq!(spec.axis, 2);
        assert_eq!(spec.mapping.nin(), 3);
        assert_eq!(spec.mapping.nout(), 3);

        // Pass IWC (a, b, 0) — axes 0, 1 must come out unchanged;
        // axis 2 picks up CRVAL.
        let pts = PointSet::from_columns(&[&[42.0], &[7.0], &[0.0]]).unwrap();
        let out = spec.mapping.transform(&pts, true).unwrap();
        let p = out.point(0);
        assert!((p[0] - 42.0).abs() < 1e-12, "axis 0 was {}", p[0]);
        assert!((p[1] - 7.0).abs() < 1e-12, "axis 1 was {}", p[1]);
        assert!((p[2] - 1.4e9).abs() < 1.0);
    }
```

- [ ] **Step 2: Run the test and confirm it FAILS**

```bash
cargo test -p ast-core --lib fitschan::wcs_spectral::tests::naxis_spectral_passthrough_other_axes
```
Expected: FAIL with `nin == 1`.

- [ ] **Step 3: Add an `embed_1axis_in_naxis` helper and use it in `build_spectral`**

In `crates/ast-core/src/fitschan/wcs_spectral.rs`, just below the imports, add a private helper:

```rust
/// Embed a 1-axis mapping into an N-axis mapping with `UnitMap`s on
/// the other axes. Identity-fast for `naxis == 1`. Mirrors C
/// `fitschan.c:38315-38358` — PermMap + parallel CmpMap + inverse
/// PermMap sandwich, but specialised to a single non-passthrough
/// axis.
fn embed_1axis_in_naxis(
    inner: DynMapping,
    naxis: usize,
    axis_0based: usize,
) -> AstResult<DynMapping> {
    use crate::mappings::{PermMap, UnitMap};

    debug_assert_eq!(inner.nin(), 1);
    debug_assert_eq!(inner.nout(), 1);

    if naxis == 1 {
        return Ok(inner);
    }

    // perm_to_first: route axis `axis_0based` to slot 0; route the
    // remaining axes into slots 1..naxis (in original FITS order).
    let others: Vec<usize> = (0..naxis).filter(|&i| i != axis_0based).collect();

    let mut outperm_to_first: Vec<i64> = Vec::with_capacity(naxis);
    outperm_to_first.push((axis_0based as i64) + 1);
    for &k in &others {
        outperm_to_first.push((k as i64) + 1);
    }
    let mut inperm_to_first: Vec<i64> = vec![0; naxis];
    inperm_to_first[axis_0based] = 1;
    for (k, &orig) in others.iter().enumerate() {
        inperm_to_first[orig] = (k as i64) + 2;
    }
    let perm_to_first = PermMap::new(&inperm_to_first, &outperm_to_first, &[])?;

    // Inverse permutation.
    let mut outperm_back: Vec<i64> = vec![0; naxis];
    outperm_back[axis_0based] = 1;
    for (k, &orig) in others.iter().enumerate() {
        outperm_back[orig] = (k as i64) + 2;
    }
    let mut inperm_back: Vec<i64> = Vec::with_capacity(naxis);
    inperm_back.push((axis_0based as i64) + 1);
    for &k in &others {
        inperm_back.push((k as i64) + 1);
    }
    let perm_back = PermMap::new(&inperm_back, &outperm_back, &[])?;

    // Parallel: inner (1-axis) || UnitMap(naxis - 1)
    let unit_rest = UnitMap::new(naxis - 1);
    let parallel = CmpMap::new(inner, Box::new(unit_rest), Composition::Parallel)?;

    // Sandwich.
    let s1 = CmpMap::new(Box::new(perm_to_first), Box::new(parallel), Composition::Series)?;
    let s2 = CmpMap::new(Box::new(s1), Box::new(perm_back), Composition::Series)?;
    Ok(Box::new(s2))
}
```

Add `use crate::mappings::UnitMap;` and `use crate::mappings::PermMap;` to the file's imports if not already present (the existing imports are at line 18: `use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, ZoomMap};`). Update to:

```rust
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, UnitMap, ZoomMap};
```

(`PermMap` is referenced from inside the helper; since the helper has its own `use` it's fine.)

Now find the result construction at the bottom of `build_spectral` (line 137-141):

```rust
    Some(SpectralResult {
        mapping,
        frame,
        axis,
    })
```

Replace with:

```rust
    let naxis = store.naxis;
    let mapping = match embed_1axis_in_naxis(mapping, naxis, axis) {
        Ok(m) => m,
        Err(_) => return None,
    };

    Some(SpectralResult {
        mapping,
        frame,
        axis,
    })
```

- [ ] **Step 4: Run the spectral tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_spectral::tests
```
Expected: all PASS, including the new `naxis_spectral_passthrough_other_axes`.

Note: `detects_freq_axis` (line 264) currently asserts `nin == 1, nout == 1`. With Phase B that test passes a 3-axis store, so `nin == nout == 3` post-change. Update the assertions in `detects_freq_axis`:

```rust
        assert_eq!(spec.mapping.nin(), 1);
        assert_eq!(spec.mapping.nout(), 1);
```
to:
```rust
        assert_eq!(spec.mapping.nin(), 3);
        assert_eq!(spec.mapping.nout(), 3);
```

`detects_wave_axis`, `detects_vrad_axis`, `sets_std_of_rest`, `sets_rest_freq`, `crval_shift_mapping` all use 1-axis stores, so `naxis == 1` short-circuits in the helper and they stay 1×1. They should keep passing without changes.

- [ ] **Step 5: Run lib + wcsconv**

```bash
cargo test -p ast-core --lib 2>&1 | grep "test result" | tail -1
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```
Expected: lib at baseline; wcsconv at `181 passed; 11 failed`. **Stop on regression.**

The wcsconv suite may report a numerical-differences-style failure here even though the mappings are mathematically equivalent — that's because `build_general_iwc_to_world` is now seeing N-axis inputs and trying to parallel-compose them, which produces a wrong shape for axes ≥ 3. If pass count drops, it's the boundary-mismatch problem Phase C resolves; before continuing, **verify via diff-of-outputs** that the celestial+spectral results are correct in isolation (not via the composer):

```bash
cargo test -p ast-core --lib fitschan::wcs_spectral::tests::naxis_spectral_passthrough_other_axes
cargo test -p ast-core --lib fitschan::wcs_celestial::tests::naxis_celestial_passthrough_axes_are_identity
```
Both must PASS. If they do but wcsconv still drops, the parallel composer in `build_general_iwc_to_world` is (mis)wrapping N-axis inputs. Skip ahead to Phase C immediately rather than reverting.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_spectral.rs
git commit -m "fitschan/wcs_spectral: lift spectral mapping to N-axis with identity passthrough"
```

---

## Phase C — Series composition + writer-side audit (spec Steps 4-5)

**Goal:** Replace the parallel composer in `build_iwc_to_world` / `build_general_iwc_to_world` with a series composer. Each per-class N-axis mapping (now full-rank from Phase B) gets composed in series — the others-pass-through invariant means series is correct. Drop the trailing output PermMap (no longer needed; per-class mappings are already in FITS axis order on both ends). Also audit the writer-side helpers (`find_wcsmap`, `split_around_wcsmap`, `split_map2*`) — should not need code changes, but verify behaviour with the new shape.

### Task C1: Series composition in `build_general_iwc_to_world`

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_read.rs`

- [ ] **Step 1: Run the headline target and capture pre-Phase-C state**

```bash
cargo test -p ast-core --test test_wcsconv -- --exact wcsconv_cases::sip_3d_swap_fits_wcs --nocapture 2>&1 | tail -30
```
Expected: still FAIL at this point; capture the chain length / mismatch for comparison.

- [ ] **Step 2: Replace the parallel-compose block with a series compose**

In `crates/ast-core/src/fitschan/wcs_read.rs`, find the body of `build_general_iwc_to_world` (the `claimed`-walk loop at line 533+, the parallel compose at 590-593, and the trailing PermMap block at 597-625).

Replace the entire body — **but keep the function signature and the early `parts.is_empty() → UnitMap::new(naxis)` short-circuit** — with the following. Read the original carefully first; here's the full replacement starting from `let mut parts: Vec<DynMapping> = Vec::new();`:

```rust
    let mut parts: Vec<DynMapping> = Vec::new();

    let mut celestial = celestial;
    let mut spectral = spectral;

    for (i, &is_claimed) in claimed.iter().enumerate() {
        // Celestial axis: emit the celestial mapping when we hit the
        // FIRST of the celestial pair (in FITS order); skip the second
        // encounter. Post-Phase-B the celestial mapping is full N-axis
        // with identity on the non-celestial axes.
        if let Some(cel) = celestial.as_ref() {
            let cel_first = cel.lon_axis.min(cel.lat_axis);
            let cel_second = cel.lon_axis.max(cel.lat_axis);
            if i == cel_first {
                let cel = celestial.take().unwrap();
                parts.push(cel.mapping);
                continue;
            }
            if i == cel_second {
                continue;
            }
        }

        // Spectral axis: emit the spectral mapping when we hit it.
        // Post-Phase-B, it's also full N-axis.
        if let Some(spec) = spectral.as_ref() {
            if i == spec.axis {
                let spec = spectral.take().unwrap();
                parts.push(spec.mapping);
                continue;
            }
        }

        // Linear/unclaimed axis: emit a CRVAL shift, but lifted to
        // N-axis form. ShiftMap on its own is naxis-axis when given a
        // length-naxis vector; here we want a shift that affects only
        // axis `i`. Use a ShiftMap wrapped in embed_1axis_in_naxis-
        // style sandwich, OR equivalently a length-`naxis` shift
        // vector with zeros on the unclaimed axes and CRVAL on `i`.
        // The latter is simpler.
        if !is_claimed {
            let mut shifts = vec![0.0; naxis];
            shifts[i] = store.crval[i].unwrap_or(0.0);
            let shift = ShiftMap::new(shifts);
            parts.push(Box::new(shift));
        }
    }

    if parts.is_empty() {
        return Ok(Box::new(UnitMap::new(naxis)));
    }

    // Compose all parts in SERIES (not parallel). Each part is full
    // N-axis with identity on the axes it doesn't own, so series
    // composition is correct and produces a flat top-level chain
    // that astSimplify can flatten and merge across. Mirrors C
    // `WcsMapFrm` (fitschan.c:37643) which calls astSimplify on the
    // series composition of N-axis WcsCelestial / WcsSpectral /
    // WcsOthers contributions.
    let mut result: DynMapping = parts.remove(0);
    for part in parts {
        result = Box::new(CmpMap::new(result, part, Composition::Series)?);
    }

    // No trailing output PermMap: every per-class mapping is already
    // in FITS axis order on both ends, so the parallel-output-permute
    // step that the old parallel-composer needed is unnecessary.

    Ok(result)
```

- [ ] **Step 3: Note that `build_iwc_to_world`'s "simple cases" early-out may need updating**

Look at lines 487-499 in `wcs_read.rs`:

```rust
    if naxis == 2 && spectral.is_none() && !claimed.contains(&false) {
        if let Some(cel) = celestial {
            // Pure 2D celestial case.
            return Ok(cel.mapping);
        }
    }

    if naxis == 1 && celestial.is_none() {
        if let Some(spec) = spectral {
            // Pure 1D spectral case.
            return Ok(spec.mapping);
        }
    }
```

After Phase B these short-circuits still work correctly: at `naxis == 2`, `cel.mapping` is 2-input/2-output and complete (the embedding helper is identity for `naxis == 2`); at `naxis == 1`, `spec.mapping` is 1-input/1-output. **Leave them as-is.**

- [ ] **Step 4: Run the lib tests inside `wcs_read`**

```bash
cargo test -p ast-core --lib fitschan::wcs_read::tests
```
Expected: 5 tests, all PASS (`decode_2d_celestial`, `decode_crpix_maps_to_crval`, `decode_1d_spectral`, `decode_3d_mixed`, `build_pixel_to_world_handles_non_adjacent_celestial_axes`). The non-adjacent test is the strict gate — its assertions on DEC-at-axis-0 / RA-at-axis-2 *must* pass under series composition with the new N-axis mappings.

- [ ] **Step 5: Run the headline target**

```bash
cargo test -p ast-core --test test_wcsconv -- --exact wcsconv_cases::sip_3d_swap_fits_wcs --nocapture 2>&1 | tail -30
```
Expected: PASS. If FAIL, examine the chain length: it should now be ≤ 3 mappings post-simplify (`Shift ; PolyMap ; MatrixMap`-like). If the chain is shorter than C's 3 elements, that's progress; if longer, look at where the FK5↔ICRS SlaMap pair is — it should now be cancellable because everything is in a flat top-level series.

- [ ] **Step 6: Run the full wcsconv suite**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```
Expected: `182 passed; 10 failed` (sip_3d_swap_fits_wcs flips green). **If the pass count drops below 181, stop and revert.**

- [ ] **Step 7: Run the full lib test suite**

```bash
cargo test -p ast-core --lib 2>&1 | grep "test result" | tail -1
```
Expected: at or above the baseline captured in Task A1 Step 2. If lower, **stop**.

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_read.rs
git commit -m "fitschan/wcs_read: compose N-axis IWC->world contributions in series"
```

---

### Task C2: Audit writer-side helpers against the new shape

**Files:**
- Read-only: `crates/ast-core/src/fitschan/fset_to_store/celestial_axes.rs`
- Read-only: `crates/ast-core/src/fitschan/fset_to_store/fiducial.rs`
- Read-only: `crates/ast-core/src/fitschan/fset_to_store/sky_pole.rs`
- Read-only: `crates/ast-core/src/fitschan/fset_to_store/make_int_world.rs`

**Why:** Spec Step 5 mentions four helpers/methods to audit:
- `find_wcsmap` — recursive into CmpMap children; should still work.
- `split_around_wcsmap` (mirrors C `SplitMap2`) — handles arbitrary series/parallel; re-test against new shape.
- `axlon_0based()` / `axlat_0based()` on WcsMap — read the same fields, no change. (Confirmed: `wcsmap.lonax()` is 1-based, `lonax_0based()` is 0-based; both already exist.)
- `make_int_world::primary_frame` — routes via `specframe::primary_frame_walk`; no change.

The audit is execution-driven: run the writer-exercising tests and confirm they still pass under the new mapping shape.

- [ ] **Step 1: Run all `fset_to_store` lib tests**

```bash
cargo test -p ast-core --lib fitschan::fset_to_store
```
Expected: all PASS. If anything fails, the writer is reading a field that's moved due to the new N-axis WcsMap location (specifically: `wcs_offset` in `split_map2_parallel` at `celestial_axes.rs:563` — when WcsMap is now top-level, `wcs_offset` is 0 and `wm.lonax() + 0` is already the FITS-1-based lon index, so this should just work).

- [ ] **Step 2: Run the FrameSet round-trip lib tests**

```bash
cargo test -p ast-core --lib frameset
cargo test -p ast-core --test test_frameset 2>&1 | grep "test result"
```
Expected: at-baseline pass counts. If round-trips fail, the writer is mis-reading the new shape and Step 3 below applies.

- [ ] **Step 3: Run the encoder fixture tests**

```bash
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```
Expected: stays at `182 passed; 10 failed` (or better — the spec notes the `class_write_fits_class` and `jan_fits_class_*` failures are FITS-CLASS encoder issues *unaffected* by this refactor). If pass count drops vs the end of Task C1, **stop, diagnose, fix** before commit.

- [ ] **Step 4: If any test failed in Steps 1-3, port from C — don't patch the Rust**

If a writer-side helper has a problem under the new shape, the failure mode tells you what to do:

- **Existing Rust diverges from C `SplitMap2` (fitschan.c:32861-33151) and the divergence shows up under N-axis input.** Re-port the affected helper from the C source. The Rust shape has no authority — it was a 2-axis-WcsMap-shaped attempt that didn't need to handle this case. (Authority rule above; memory: [[c-logic-beats-existing-rust]].)
- **Hard-coded 2-axis assumption surfaces (e.g. `wm.lonax()` returning literal `1` because the old WcsMap was always built 2-axis).** Re-read the corresponding C source for that helper, mirror its handling line-for-line, and update Rust callers to match.

In either case: do NOT add a "if naxis == 2 use old path else use new path" branch to preserve the existing Rust shape. The old Rust shape is being retired by this refactor; cleaning it up now is part of the work, not future debt.

If no test failed in Steps 1-3 (most likely outcome — the Rust helpers already mirror `SplitMap2`'s recursive series/parallel walk), this step is "nothing to commit" — proceed.

- [ ] **Step 5: Format, lint, commit (only if Step 4 produced changes)**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -p  # interactively pick the writer-side hunks
git commit -m "fitschan/fset_to_store: handle top-level N-axis WcsMap in writer split"
```

If no changes were needed, skip the commit. Either way: **before proceeding to Phase D, run the full workspace test once:**

```bash
cargo test --workspace 2>&1 | tail -5
```
Expected: all crates green; wcsconv at `182 passed; 10 failed`; lib at baseline.

---

## Phase D — Idiomatic Rust cleanup (spec Step 7)

**Goal:** Once the refactor is green and the headline test is flipped, factor out the inline N-axis embedding code into a private helper *only if it doesn't drift from C*. The C library builds the embedding inline at each call site (fitschan.c:38315-38358 for `WcsNative`, similar pattern for `WcsSpectral`); the Rust shape should mirror that. So Phase D is conservative: keep the spectral helper from B2 because it deduplicates a real bit of inline code, but **only** extract a celestial helper if doing so doesn't push Rust further from the C shape.

Authority rule reminder: if at any point during Phase D the cleanup makes the Rust diverge more from C than it already does, abandon the cleanup and keep the inline form. We're not chasing DRY at the cost of fidelity.

### Task D1: Extract `embed_celestial_in_naxis` helper

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs`

**Why:** The N-axis embedding code added in Phase B Task B1 is inline in `build_iwc_to_sky_mapping`. Extract it into a private function.

- [ ] **Step 1: Add the helper just below `build_iwc_to_sky_mapping`**

```rust
/// Embed a 2-axis mapping that operates on (lon, lat) into an
/// `naxis`-rank mapping that passes the other axes through unchanged.
/// Identity-fast for `naxis == 2` (returns `inner` directly).
///
/// Internally, the embedding is implemented as
/// `PermMap_to_2d ; (inner || UnitMap_(N-2)) ; PermMap_back` —
/// mirrors C `fitschan.c:38315-38358` (the nax_rem > 0 branch and
/// the surrounding PermMap sandwich at 38325-38346).
fn embed_celestial_in_naxis(
    inner: DynMapping,
    naxis: usize,
    lon_axis_0based: usize,
    lat_axis_0based: usize,
) -> Option<DynMapping> {
    use crate::mappings::UnitMap;
    debug_assert_eq!(inner.nin(), 2);
    debug_assert_eq!(inner.nout(), 2);

    if naxis == 2 {
        return Some(inner);
    }

    let others: Vec<usize> = (0..naxis)
        .filter(|&i| i != lon_axis_0based && i != lat_axis_0based)
        .collect();

    let mut outperm_to_2d: Vec<i64> = Vec::with_capacity(naxis);
    outperm_to_2d.push((lon_axis_0based as i64) + 1);
    outperm_to_2d.push((lat_axis_0based as i64) + 1);
    for &k in &others {
        outperm_to_2d.push((k as i64) + 1);
    }
    let mut inperm_to_2d: Vec<i64> = vec![0; naxis];
    for (slot_idx, &input_idx) in [lon_axis_0based, lat_axis_0based]
        .iter()
        .chain(others.iter())
        .enumerate()
    {
        inperm_to_2d[input_idx] = (slot_idx as i64) + 1;
    }
    let perm_to_2d = PermMap::new(&inperm_to_2d, &outperm_to_2d, &[]).ok()?;

    let mut outperm_back: Vec<i64> = vec![0; naxis];
    outperm_back[lon_axis_0based] = 1;
    outperm_back[lat_axis_0based] = 2;
    for (k, &orig) in others.iter().enumerate() {
        outperm_back[orig] = (k as i64) + 3;
    }
    let mut inperm_back: Vec<i64> = Vec::with_capacity(naxis);
    inperm_back.push((lon_axis_0based as i64) + 1);
    inperm_back.push((lat_axis_0based as i64) + 1);
    for &k in &others {
        inperm_back.push((k as i64) + 1);
    }
    let perm_back = PermMap::new(&inperm_back, &outperm_back, &[]).ok()?;

    let unit_rest = UnitMap::new(naxis - 2);
    let parallel = CmpMap::new(inner, Box::new(unit_rest), Composition::Parallel).ok()?;
    let s1 = CmpMap::new(Box::new(perm_to_2d), Box::new(parallel), Composition::Series).ok()?;
    let s2 = CmpMap::new(Box::new(s1), Box::new(perm_back), Composition::Series).ok()?;
    Some(Box::new(s2))
}
```

- [ ] **Step 2: Replace the inline `if naxis == 2 { rotation } else { … }` block in `build_iwc_to_sky_mapping` with a call to the helper**

Find the block from Phase B Task B1 Step 4 (the `let rotation_naxis: DynMapping = if naxis == 2 { rotation } else { ... };`).

Replace the whole block with:

```rust
    let rotation_naxis = embed_celestial_in_naxis(rotation, naxis, lon_axis, lat_axis)?;
```

- [ ] **Step 3: Run the celestial tests, lib, and wcsconv**

```bash
cargo test -p ast-core --lib fitschan::wcs_celestial::tests
cargo test -p ast-core --lib 2>&1 | grep "test result" | tail -1
cargo test -p ast-core --test test_wcsconv 2>&1 | grep "test result"
```
Expected: all PASS; wcsconv at `182 passed; 10 failed` or better.

- [ ] **Step 4: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/ast-core/src/fitschan/wcs_celestial.rs
git commit -m "fitschan/wcs_celestial: factor N-axis embedding into embed_celestial_in_naxis"
```

---

### Task D2: Final workspace verification

**Files:** none modified.

- [ ] **Step 1: Run the full workspace tests**

```bash
cargo test --workspace 2>&1 | tail -10
```
Expected: all crates green. Record the wcsconv pass count: must be ≥ 182. Record the lib pass count: must be at-or-above baseline.

- [ ] **Step 2: Run the targeted-failure check one final time**

```bash
cargo test -p ast-core --test test_wcsconv -- --exact wcsconv_cases::sip_3d_swap_fits_wcs
```
Expected: `test result: ok. 1 passed; 0 failed; …`.

- [ ] **Step 3: Confirm format and clippy are clean across the workspace**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```
Both must succeed with no diff / no warnings.

- [ ] **Step 4: Inspect the commit log for the phase-grouped story**

```bash
git log --oneline -n 8
```
Expected (top-down):
- `fitschan/wcs_celestial: factor N-axis embedding into embed_celestial_in_naxis` (D1)
- (optional) `fitschan/fset_to_store: handle top-level N-axis WcsMap in writer split` (C2)
- `fitschan/wcs_read: compose N-axis IWC->world contributions in series` (C1)
- `fitschan/wcs_spectral: lift spectral mapping to N-axis with identity passthrough` (B2)
- `fitschan/wcs_celestial: lift celestial chain to N-axis end-to-end` (B1)
- `fitschan/wcs_celestial: build N-axis WcsMap with FITS-axis lon/lat indices` (A2)

No final commit needed — Task D2 is verification-only.

---

## Self-review summary (executed by the plan author)

**Spec coverage:**
- Spec Step 1 (N-axis WcsMap) → Phase A Task A2.
- Spec Step 2 (N-axis Zoom + N-axis rotation) → Phase B Task B1 (Steps 3-4).
- Spec Step 3 (Spectral mirror) → Phase B Task B2.
- Spec Step 4 (Series composition + drop output PermMap) → Phase C Task C1.
- Spec Step 5 (Update fset_to_store consumers) → Phase C Task C2 (audit; expected no code change).
- Spec Step 6 (Test surface, expected outcomes) → integrated as the test cadence in every task; headline `wcsconv_cases::sip_3d_swap_fits_wcs` flips green at end of Phase C Task C1.
- Spec Step 7 (Idiomatic-Rust cleanup) → Phase D Task D1 (celestial embed helper). Phase B Task B2 already produced `embed_1axis_in_naxis` for the spectral side, so no additional spectral cleanup task is needed. The spec's optional `Mapping::embed_in_naxis` trait-method idea is explicitly *not* pursued — the spec says "only if the C reference suggests this — the C source builds the embedding inline at each call-site". Holding to the C-first rule.

**Risk-assessment coverage (spec section "Risk assessment"):**
- "After Step 1, re-run all wcsconv tests; the count must remain at 181/192 before proceeding." → Phase A Task A2 Step 8.
- "After Step 2, the `build_iwc_to_sky_mapping` test should still pass with the same numerical outputs." → Phase B Task B1 Step 6 (`reference_pixel_maps_to_crval`).
- "After Step 4, run `cargo test --workspace` — the FrameSet round-trip and encoder fixture tests should all stay green." → Phase C Task C2 Step 2 + Phase D Task D2 Step 1.

**Out-of-scope items respected:** DSS, offset_sky, jan_fits_class_*, class_write_fits_class, degen1_fits_wcs are never touched. Verified by file scope: this plan only modifies `wcs_celestial.rs`, `wcs_spectral.rs`, `wcs_read.rs`, and (conditionally) `fset_to_store/celestial_axes.rs`. None of the out-of-scope fixtures are exercised by these files in a way that would change their behaviour.

**Type / signature consistency:**
- `WcsMap::new(ncoord, proj, lonax, latax)` — 1-based lon/lat, used consistently as `lon_axis + 1` / `lat_axis + 1` everywhere.
- `PermMap::new(inperm, outperm, constants)` — 1-based positive entries for axes, negative for constants. Used identically in lift/drop/perm_to_2d/perm_back/embed_1axis_in_naxis/embed_celestial_in_naxis. Constants array empty `&[]` when no constants are referenced.
- `CelestialResult.lon_axis` / `lat_axis` and `SpectralResult.axis` are 0-based throughout. Conversion to 1-based for Wcs/Perm calls is always at the call site, never in the result struct.
- `embed_1axis_in_naxis` (B2) and `embed_celestial_in_naxis` (D1) are private; their signatures use 0-based axis indices to match the surrounding decoder code.

**Placeholder scan:** no TBD/TODO/etc. Every step contains literal code or literal commands.

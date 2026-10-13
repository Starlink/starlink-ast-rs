# MakeIntWorld port — design

**Status:** draft, awaiting user review.
**Author:** brainstorming session 2026-05-25.
**Branch:** single feature branch (no parallel paths, no flag-gated rollout).
**Predecessors:** prior CRPIX-related work in commits 4008be9, 45a9696, fdc0b65 (heuristic, reverted in 5d9aaa6), 1f9f3b7 (helper port). Memory note `project_makeintworld_port.md`.

## Problem

The FITS-WCS writer's CRPIX recovery is off by ~µpixel for inputs whose CRPIX uses the FITS half-pixel convention (`vopt-geo`, `freq-ghz-bary`, etc.) and miscomputes other linear keywords (CD, CDELT, PC) for several other fixtures. Today 40 wcsconv tests fail; ~30 of them are numeric-value mismatches that share a root cause: our writer extracts the IWC mapping from the existing FrameSet's `pixel→world` chain, which inherits unit-conversion FP noise from the FrameSet's history.

The C oracle (`libast`) does not have this problem because it *constructs* the IWC mapping from chosen header values (CRVAL/CDELT/projection) before extracting linear keywords. Earlier attempts to fix this with heuristics (Newton refinement + 1e-9 threshold; floor on g0) papered over symptoms while violating the project's "mirror C control flow directly" rule.

## Goal

Replace the value-extraction half of `store_from_frameset_to` with a port of C's `FsetToStore` (fitschan.c:12100) and its callees, so every wcsconv test that today fails on a numeric-value mismatch passes bytewise.

Tests whose failures are purely structural (encoding selection, card ordering, missing degenerate-axis support) or formatting-only are explicitly out of scope and stay red.

## Non-goals

- Reader-side changes. The reader (`wcs_read.rs`) keeps its current shape.
- Card-emission code (`write_pc_cdelt`, `write_cd_matrix`, comment text). The new code stops at populating the `FitsStore`; the existing card-emission walks the store unchanged.
- Encoding selection (DSS-vs-native auto-pick).
- Performance work. Correctness first; performance can come later.

## Constraint: no heuristics

This port exists because earlier attempts to patch the symptom with tolerance constants and Newton iterations cost more time than they saved. The rule for this branch:

> When the C oracle has an exact algorithm, port it. Do not substitute a heuristic. Do not introduce tolerance constants that don't appear in the C source. If you discover the C is itself a heuristic (a tolerance, a magic number), port the heuristic — but document its C provenance.

This rule overrides every Rust-idiom temptation. If you catch yourself writing `if (max_step < some_threshold)` without a corresponding `fitschan.c:NNNN` reference, stop.

## Scope

### In scope (top-level ports)

| Rust function       | Port of                  | Approx. C exec lines |
|---------------------|--------------------------|----------------------|
| `fset_to_store`     | `FsetToStore` @ 12006    | 250                  |
| `celestial_axes`    | `CelestialAxes` @ 4890   | 340                  |
| `spectral_axes`     | `SpectralAxes` @ 30050   | 370                  |
| `other_axes`        | `OtherAxes` @ 24030      | 180                  |
| `make_int_world`    | `MakeIntWorld` @ 21689   | 300                  |
| `sip_int_world`     | `SIPIntWorld` @ 28495    | 300                  |

### In scope (helpers)

| Rust item                            | Port of                              |
|--------------------------------------|--------------------------------------|
| `Mapping::map_split` (replaced)      | `astMapSplit`                        |
| `mappings::helpers::linear_approx`   | `astLinearApprox`                    |
| `mappings::helpers::analyse_poly`    | `AnalysePoly` (fitschan.c)           |
| `mappings::helpers::scale_poly_inputs` | `ScalePolyInputs` (fitschan.c)     |
| `wcs_keywords::orth::orth_vector_set` + `orth_vector` | `OrthVectorSet`/`OrthVector` |
| `wcs_keywords::invertable::make_invertable` | `MakeInvertable`              |
| `wcs_keywords::sky_sys::sky_sys`     | `SkySys` (used by `celestial_axes`)  |
| `wcs_keywords::celestial_axes::get_fiducial_wcs` | `GetFiducialWCS` (used by `celestial_axes`) |
| `wcs_keywords::is_sky_off`           | `IsSkyOff`                           |
| (small helpers as encountered)       | as needed                            |

Already ported and used: `find_basis_vectors`, `fit_line`, `fit_line_cd`, `fit_ok` (from commit 1f9f3b7).

### Out of scope

- Wcsconv tests that fail on encoding selection, card ordering, missing features (`degen1`, `dss_*`, `jan_fits-class_ast`, `multi-wcs-dup_fits-wcs`, `azel_fits-wcs`, `tan-pv0_fits-wcs`).
- The `WcsValues` and `SpecTrans` C functions only insofar as `celestial_axes` / `spectral_axes` reach into them — port the slices that those callers exercise, not the rest of those functions.
- Alternative axis descriptions (`CTYPEjA`, etc.). The existing alternative-axis code in `wcs_write.rs` keeps working; it calls `store_from_frameset_to` per alt-axis letter just like today.

## Architecture

### Module layout

```
crates/ast-core/src/fitschan/
├── wcs_keywords/                    # NEW
│   ├── mod.rs                       # re-exports + fset_to_store entry
│   ├── celestial_axes.rs            # ~700 LoC
│   ├── spectral_axes.rs             # ~700 LoC
│   ├── other_axes.rs                # ~400 LoC
│   ├── make_int_world.rs            # ~600 LoC
│   ├── sip_int_world.rs             # ~600 LoC
│   ├── orth.rs                      # OrthVectorSet + OrthVector
│   ├── invertable.rs                # MakeInvertable
│   ├── sky_sys.rs                   # SkySys, IsSkyOff, GetFiducialWCS
│   └── (small helpers)
├── wcs_decompose.rs                 # FindBasisVectors / FitLine / FitOK / FitLineCd
│                                    #   (already ported; drop #[allow(dead_code)])
└── wcs_write.rs                     # ~1000 LoC removed; thin shim into wcs_keywords

crates/ast-core/src/mappings/
└── helpers/                         # cross-class Mapping helpers
    ├── linear_approx.rs             # NEW — astLinearApprox port
    ├── analyse_poly.rs              # NEW — AnalysePoly port
    └── scale_poly_inputs.rs         # NEW — ScalePolyInputs port
```

The `Mapping` trait gets one signature change: `map_split` is reshaped to mirror `astMapSplit`. The contiguous-output-range version is retired because it has no production callers.

### Replaced pipeline

Today's flow inside `store_from_frameset_to`:
```
identify_axes → populate_celestial / populate_spectral / populate_other_axis
              → decompose_linear (finite-diff at floored g0)
              → solve_crpix_from_decomp
              → snap_crval_residue, write_cd_matrix, write_pc_cdelt
```

New flow:
```
fset_to_store:
  ├── celestial_axes  → wcs→iwc segment + store CRVAL/CTYPE/etc.
  ├── spectral_axes   → wcs→iwc segment + store
  ├── other_axes      → wcs→iwc segment + store
  ├── pixel→iwc = pixel→wcs ∘ wcs→iwc
  └── make_int_world(pixel_iwc, …) → CRPIX/CD/PC/CDELT in store
        ├── (if SIP) sip_int_world → strips PolyMap, supplies celestial CRPIX/CD
        ├── find_basis_vectors → g0 / w0 / tol
        ├── fit_line_cd → CD columns from 101-point LSQ per axis
        ├── orth_vector_set → fills extra columns when nout > nin
        ├── make_invertable → patches degenerate-axis singularities
        └── solve_linear_system (existing) → CRPIX = g0 - y
```

### Why this fixes the µpixel issue

`fit_line_cd` over 101 samples returns clean CD columns *only when its input mapping has clean CDELT values to begin with*. The existing path builds the input mapping by extracting from the FrameSet (noisy); the new path builds it from `chosen_crval` + projection structure (bit-exact). Once the input is clean, the LU solve gives sub-ULP CRPIX.

### Mapping primitives

#### `Mapping::map_split` (reshape)

```rust
// Replaces existing map_split(first_out, nout_wanted) -> Option<DynMapping>.
fn map_split(&self, in_axes: &[usize])
    -> Option<(Vec<usize>, DynMapping)>;
```

Default returns `None`. Per-class implementations: `UnitMap`, `ZoomMap`, `ShiftMap`, `WinMap`, `MatrixMap`, `PermMap`, `CmpMap` (series + parallel), `WcsMap`, `PolyMap`. Faithful ports of the C `MapSplit_<class>` methods.

#### `linear_approx`

```rust
pub struct LinearFit {
    pub shift: Vec<f64>,    // length nout
    pub matrix: Vec<f64>,   // nout × nin, row-major
}

pub fn linear_approx(
    map: &dyn Mapping,
    lbnd: &[f64],
    ubnd: &[f64],
    tol: f64,
) -> Option<LinearFit>;
```

#### `analyse_poly`

```rust
pub fn analyse_poly(pm: &PolyMap)
    -> Option<(ShiftMap, PolyMap, MatrixMap)>;
```

Decomposes a 2-in/2-out PolyMap into (leading shift, canonical SIP-shape PolyMap, trailing matrix).

#### `scale_poly_inputs`

```rust
pub fn scale_poly_inputs(pm: &PolyMap, scales: &[f64]) -> PolyMap;
```

### Top-level ported functions

#### `fset_to_store`

```rust
pub(super) fn fset_to_store(
    fs: &FrameSet,
    naxis: usize,
    dim: &[Option<f64>],
    encoding: Encoding,
) -> AstResult<FitsStore>;
```

Replaces `store_from_frameset_to`. About 250 exec C lines. Walks the FrameSet, picks target frame and `wperm`, calls `celestial_axes` / `spectral_axes` / `other_axes`, composes `pixel→iwc`, calls `make_int_world`.

#### `celestial_axes`

Returns the `wcs→iwc` segment for celestial axes; passes through non-celestial. Stores CTYPE / CUNIT / CRVAL / EQUINOX / RADESYS / MJD-OBS for those axes.

The CRVAL is computed via `get_fiducial_wcs` (port of `GetFiducialWCS`) — traces the WcsMap's fiducial native point through the post-Wcs chain to get the exact celestial coordinate at `IWC = (0, 0)`. **Not** via `world(g0)`; that's the heuristic the current code uses.

Largest top-level port (~340 exec C lines). Calls `sky_sys` (~250 lines).

#### `spectral_axes`

Returns `wcs→iwc` for the spectral axis. Stores spectral CTYPE (algorithm code chosen by inspecting the SpecMap chain), CUNIT, CRVAL, RESTFRQ, SPECSYS, VELOSYS.

About 370 exec C lines. Where most of `vopt-geo` / `freq-ghz-bary` / `vrad-lsrd` get their CRVAL3 right.

#### `other_axes`

Catch-all for non-celestial-non-spectral axes. Returns a `ShiftMap`-per-axis `wcs→iwc` segment, stores simple CTYPE / CUNIT / CRVAL.

Simplest top-level port (~180 exec C lines). **Land first** as the smoke test for the `wcs_keywords/` architecture.

#### `make_int_world`

```rust
fn make_int_world(
    pixel_iwc: &dyn Mapping,
    wcs_frame: &dyn Frame,
    wperm: &[usize],
    store: &mut FitsStore,
    dim: &[Option<f64>],
    fitstol: f64,
    sip_ok: bool,
) -> AstResult<bool>;
```

Receives the assembled `pixel→iwc` mapping. Sets CRPIX / CD / PC / CDELT in the store. Returns `Ok(true)` on success, `Ok(false)` when the chain isn't linear (matches C's int-status return).

#### `sip_int_world`

```rust
struct SipResult {
    pub stripped: DynMapping,
    pub in_axes: [usize; 2],
    pub crpix: [f64; 2],
    pub cd: [f64; 4],   // [lon_in0, lon_in1, lat_in0, lat_in1]
}

fn sip_int_world(
    pixel_iwc: &dyn Mapping,
    fitstol: f64,
    lonax: usize,
    latax: usize,
    store: &mut FitsStore,
    dim: &[Option<f64>],
) -> Option<SipResult>;
```

Splits `pixel_iwc` around the SIP PolyMap, verifies pre-Poly is a shift and post-Poly is a linear matrix with no shift, computes CRPIX from pre-Poly applied to (0, 0) and CD from `linear_approx` of post-Poly. Stores SIP A/B/AP/BP coefficients. Returns the stripped mapping for `make_int_world` to continue with.

### Data flow

State carried through `fset_to_store`:

```rust
let mut store     = FitsStore::new(naxis);
let mut wperm     = (0..naxis).collect::<Vec<_>>();
let mut axis_done = vec![false; naxis];
let mut crvals    = vec![None::<f64>; naxis];
let mut wcs_iwc: DynMapping = Box::new(UnitMap::new(naxis));
```

`celestial_axes` → `spectral_axes` → `other_axes` each compose into `wcs_iwc` and mark axes done. Final compose `pixel_iwc = pixel→wcs ∘ wcs_iwc.` Then `make_int_world(pixel_iwc, …)`.

### Invariants

- After `celestial_axes + spectral_axes + other_axes`, every `axis_done[i] == true`. Otherwise the FrameSet has an unhandled frame kind — error.
- `wperm` is a permutation of `0..naxis`.
- `pixel_iwc.nin() == pixel_iwc.nout() == naxis`.
- `make_int_world` is the only function that writes `store.crpix`, `store.cd`, `store.pc`, `store.cdelt`. Every other ported function writes axis-attribute fields only.

## Testing strategy

### Three layers

1. **Per-function unit tests** in the same module, each with hand-rolled inputs and expected outputs.
2. **C-oracle-derived fixtures.** For the algorithmic functions (`linear_approx`, `analyse_poly`, `scale_poly_inputs`, `make_int_world`, `sip_int_world`, `find_basis_vectors`, `fit_line`), capture reference output values once from the C library and embed as constants in the Rust test. Generation lives in a small `tools/oracle_fixtures/` directory (a few `.c` programs that print formatted Rust constant arrays); the captured values stay checked-in. Tests run without the oracle.
3. **wcsconv suite** (`tests/test_wcsconv.rs`) as the integration gate.

### C-oracle-derived fixture pattern

For each function whose output depends on subtle numerical behaviour, follow this recipe:

```
1. Write a small C program in tools/oracle_fixtures/<fn>.c that
   constructs a representative input (PolyMap, WcsMap, FrameSet,
   …), calls the C function, and prints the result as a Rust
   constant declaration.
2. Build it against reference/starlink-ast (tier-3 build target).
3. Run it once; copy stdout into the test file as a literal const.
4. Commit both the .c source and the generated .rs constants
   together so the provenance is recoverable.
```

This pattern was used successfully for VELOSYS earlier in this codebase (per the C-library-quirks memory). Re-use for every function whose unit test would otherwise require us to *compute* the expected value by hand.

Specific oracle fixtures to capture:

| Function           | Fixture sets to capture                                          |
|--------------------|------------------------------------------------------------------|
| `linear_approx`    | Existing `testmapping.c` cases (3 PolyMaps) + 6-8 new shapes covering: |
|                    |   - 2x2 PolyMap with constant terms (nontrivial shift)           |
|                    |   - 3x3 PolyMap with mixed linear and quadratic terms            |
|                    |   - PolyMap that fails the linearity tolerance (expect None)     |
|                    |   - WcsMap (TAN, ARC, ZEA) over various box sizes                |
|                    |   - CmpMap series of MatrixMap+ShiftMap                          |
|                    |   - SIP-shape PolyMap from sip-3d header                         |
| `analyse_poly`     | 4-6 PolyMaps of differing analyseability:                        |
|                    |   - SIP-shape (A/B coefficients, no shift, identity linear)      |
|                    |   - PolyMap with constant terms → expect non-trivial shift       |
|                    |   - PolyMap with non-unit linear → expect non-trivial matrix     |
|                    |   - Non-analyseable polymap → expect None                        |
| `scale_poly_inputs`| 3-4 PolyMaps × 2-3 scale vectors → captured coefficient outputs  |
| `find_basis_vectors`| Dim choices that exercise centre / (1,…,1) / diagonal-sweep paths |
| `fit_line` / `fit_line_cd` | Several linear and non-linear chains; capture slope arrays |
| `make_int_world`   | Half a dozen synthetic pixel→IWC mappings (linear-only, SIP, nout > nin) |
| `sip_int_world`    | sip-3d, sip-3d-swap, plus a non-SIP mapping that returns None    |

For `celestial_axes` / `spectral_axes` / `other_axes`, the wcsconv suite is itself the oracle-derived fixture set (the expected `.fits-wcs` files were generated by C). No additional capture needed.

### Per-function unit tests (summary table)

| Port                 | Test focus                                                                                          |
|----------------------|-----------------------------------------------------------------------------------------------------|
| `Mapping::map_split` | One test per implementing class. CmpMap series + parallel + nested. PermMap with non-trivial perm.   |
| `linear_approx`      | Oracle fixtures (above) + boundary cases.                                                            |
| `analyse_poly`       | Oracle fixtures (above) + boundary cases.                                                            |
| `scale_poly_inputs`  | Oracle fixtures + round-trip identity (`scale_by(1.0) == identity`).                                 |
| `find_basis_vectors` | Oracle fixtures across the centre / origin / diagonal paths.                                         |
| `fit_line` / `fit_line_cd` | Oracle fixtures + linearity-refusal case.                                                      |
| `orth_vector_set`    | 2-of-3 known columns → third is unit-perpendicular. Degenerate input → `None`.                       |
| `make_invertable`    | Already-invertable unchanged. Singular column on degenerate axis → patched. Non-degenerate → unchanged. |
| `other_axes`         | 1D BasicFrame, 2D linear frame.                                                                       |
| `celestial_axes`     | 2D pure TAN, 3D galactic+pixel, SkyOffset (origin + pole).                                            |
| `spectral_axes`      | One per algorithm code we currently emit (FREQ, FREQ-OHEL, FREQ-LSR, VOPT-F2W, VRAD, WAVE, …).        |
| `make_int_world`     | Linear, half-pixel CRPIX, SIP, nout > nin, non-linear refusal.                                        |
| `sip_int_world`      | sip-3d, sip-3d-swap, non-SIP refusal.                                                                 |

### wcsconv gate

Final check at end of branch: `cargo test -p ast-core --test test_wcsconv` shows every numeric-value-failing test now passes. Out-of-scope failures (encoding, ordering, formatting) are still red and explicitly classified.

### Mid-port debugging

When a numeric value is unexpectedly off during the work, build a tiny C oracle program (`/tmp/oracle_X.c`) that calls the same C function with the same input, dump intermediate values, diff against the Rust pipeline. Same playbook as the earlier VELOSYS investigation. Note any discovered C library quirks in `docs/issues/c-library-quirks.md`.

## Migration plan within the branch

Eight commit groups. Each independently buildable; tests are allowed to fail until group 7. CI build gate stays green throughout.

| # | Group                                              | New LoC | Removed LoC | Tests turning green   |
|---|----------------------------------------------------|---------|-------------|----------------------|
| 1 | `Mapping::map_split` reshape                       | ~600    | ~200        | none — invariant     |
| 2 | `linear_approx`, `analyse_poly`, `scale_poly_inputs` | ~600  | none        | none — invariant     |
| 3 | `wcs_keywords/` skeleton + `other_axes`            | ~400    | none        | none — not wired     |
| 4 | `make_int_world` + `orth_vector_set` + `make_invertable` | ~700 | none     | none — not wired     |
| 5 | `celestial_axes` + `spectral_axes`                 | ~1500   | none        | none — not wired     |
| 6 | `sip_int_world`                                    | ~400    | none        | none — not wired     |
| 7 | Wire `fset_to_store` into the writer; delete old   | ~50     | ~1000       | most numeric         |
| 8 | Landing fixes from the wcsconv walkthrough         | ~200    | trim        | rest                 |
|   | **Total**                                          | ~4500   | ~1200       | ~30+ wcsconv         |

Per-group rules:

- `cargo fmt && cargo clippy --workspace -- -D warnings` passes per group.
- Build stays green per group.
- Tests are allowed to fail in groups 1–6 only when an *intentionally-not-yet-wired* path exists; a genuine compilation or pre-existing-test break must be fixed.
- Group 7 swings the wcsconv numbers; group 8 cleans up.

## Risks

| Risk                                            | Mitigation                                                       |
|-------------------------------------------------|------------------------------------------------------------------|
| Oracle-fixture capture diverges from C source over time | Both `.c` source and captured `.rs` constants are checked in; provenance is recoverable. |
| `analyse_poly` returns a wrong split for an unfamiliar PolyMap shape | Unit tests cover all shapes that appear in our reader-emitted PolyMaps; non-analysable inputs return None. |
| `Mapping::map_split` reshape breaks something we don't see  | Only 4 internal callers + a test file; full test suite gate per commit. |
| C oracle has a quirk that the port replicates incorrectly  | Document quirks in `docs/issues/c-library-quirks.md` as discovered (existing pattern). |
| Branch grows too large to review               | Eight commit groups each with a self-contained scope; commit messages are detailed. |
| New unit tests pass but wcsconv still fails    | Group 8 is reserved for exactly this; allow time for it. |

## Out-of-scope follow-ups

After this port lands, separate (smaller) projects:

- Encoding selection fixes (`dss_*`, `jan_fits-class_ast`).
- Card ordering fixes (`multi-wcs-dup`, `azel`).
- Degenerate-axis support (`degen1`).
- Formatting precision (`tan-pv0`).
- Performance benchmarking once correctness is locked.

## References

- C source: `reference/starlink-ast/src/fitschan.c` (line numbers cited above).
- Earlier work: commits 4008be9, 45a9696, fdc0b65, 5d9aaa6, 1f9f3b7.
- Memory notes: `feedback_no_corner_cuts_on_c.md`, `project_makeintworld_port.md` (retired by this port), `reference_c_library_quirks_doc.md`.
- C oracle quirks log: `docs/issues/c-library-quirks.md`.
- Existing C reference tests: `reference/starlink-ast/ast_tester/testmapping.c` (sparse `astLinearApprox` coverage).

# N-axis WcsMap construction refactor

> **HISTORICAL DOCUMENT (completed).** The N-axis WcsMap refactor landed
> (commits `d9cbfd8`→`95eac6a`); `sip_3d_swap_fits_wcs` is green and wcsconv
> passes 192/192. Retained as a record of the refactor; no outstanding work.

## Background

The C library's `WcsCelestial` (`fitschan.c:35584`) and `WcsNative`
(`fitschan.c:38104`) build the celestial axes' contribution to the
pixel→WCS mapping as an **N-axis mapping** that operates only on the
two celestial axes and passes the remaining axes through unchanged.
The mapping pipeline `WcsMapFrm` (`fitschan.c:37758`) composes the
N-axis results from `WcsCelestial` / `WcsSpectral` / `WcsOthers` in
**series**, and finally calls `astSimplify` on the composition.

In our Rust port, the celestial decoder returns a *2-axis* mapping
that the read-side composer wraps in a parallel `CmpMap` with the
spectral and other-axis 2- or 1-axis sub-mappings.  The shape is
mathematically equivalent, but the chain it produces is harder for
the simplifier to collapse — particularly when integrity remappings
inject SlaMap deltas inside the parallel CmpMap structure that the
parallel-in-series merger cannot move past intermediate
parallel CmpMap deltas operating on disjoint axes.

The concrete failure is `wcsconv_cases::sip_3d_swap_fits_wcs`: a
SIP-3D fixture with the SkyFrame axes swapped (`CTYPE1='DEC--TAN-SIP'`,
`CTYPE2='RA---TAN-SIP'`).  C produces a 12-mapping `tmap1` chain that
`astSimplify` reduces to 3 elements (`Shift` ; `PolyMap` ; `MatrixMap`)
inside `SIPIntWorld`.  We produce a 19-mapping pre-simplify chain
that simplifies to only 9 elements; the residual 9 still contain a
redundant FK5↔ICRS SlaMap pair (Hfk5z + Fk5hz at the same epoch) that
should mathematically cancel but cannot because they are buried
inside two separate parallel `CmpMap`s in the chain.

## Investigation summary

Verified facts:

1. **The simplifier is correct** for the basic
   `parallel(SlaMap_Hfk5z, Unit) ; parallel(Unit_2, ShiftMap_1) ;
   parallel(SlaMap_Fk5hz, Unit)` pattern — see the
   `parallel_in_series_with_blocking_delta` test in
   `crates/ast-core/src/mappings/simplify.rs`.  It cancels the
   SlaMap pair and merges the entire chain to a single 3-axis
   ShiftMap.
2. **The FrameSet integrity port is correct.**  Commit `8576be7`
   ports `astRemapFrame` + `TidyNodes`, which mirror C's
   add-node-then-collapse behaviour for `astSetC` on a FrameSet
   attribute.  After that change, `with_integrity` adds a new node
   carrying the delta and immediately runs `tidy_nodes`, which
   simplifies the combined edge mapping (matching C
   `frameset.c:11191`).
3. **The `make_sky_mapping` simplification matches C
   `MakeSkyMapping`.**  Commit `b267de5` calls `simplified()` on
   the assembled SlaMap before returning, mirroring C's
   `astSimplify(tmap2)` at `skyframe.c:6583`.
4. **The SphMap merge rules now match C.**  Commit `ad16938`
   removed a spurious PolarLong-equality check on the fwd→inv
   cancellation path that C does not perform.
5. **The structural difference between C's chain and ours is the
   shape of the WcsCelestial result.**  C builds the celestial
   contribution as an N-axis mapping with a **top-level N-axis
   `WcsMap`** (e.g. `WcsMap nin=3 nout=3`).  Our code constructs a
   2-axis chain (Zoom ; WcsMap_inv ; rotation) and the read-side
   composer (`build_iwc_to_world` in `wcs_read.rs`) wraps it in a
   parallel `CmpMap` with the spectral / other contributions — so
   the WcsMap ends up nested several levels deep inside parallel
   `CmpMap`s.

The C-style structure interacts much better with the simplifier
because `astSimplify` can flatten the top-level series of N-axis
mappings into a single list and apply class-specific MapMerges
across it; our shape forces the simplifier to descend into parallel
CmpMaps via `try_merge_parallel_in_series`, which can only commute
mappings whose dimensions match across the parallel boundary.

## Refactor plan

The goal is to bring the Rust read-side mapping construction into
line with C's `WcsMapFrm` shape: each axis-class contribution
returns an N-axis mapping, and `build_iwc_to_world` composes them in
series.

### Step 1: N-axis WcsMap construction

`WcsMap::new` already supports any `ncoord >= 2` (with `lonax` /
`latax` as 1-based axis indices into the N axes).  No `WcsMap` API
change is required.

In `wcs_celestial.rs::build_celestial_with_def_b1950`, replace
`WcsMap::new(2, proj, 1, 2)` with
`WcsMap::new(naxis, proj, lon_axis + 1, lat_axis + 1)`.  Pass
`naxis = store.naxis` through from the entry point.

### Step 2: N-axis Zoom / native→celestial rotation

`build_iwc_to_sky_mapping` currently creates a 2-axis `ZoomMap` and
a 2-axis rotation.  Replace with:

- **Zoom**: a diagonal `MatrixMap` of size N×N with `DD2R` on the
  celestial axes only and `1.0` elsewhere — mirrors C
  `fitschan.c:35946-35954`.
- **Rotation**: keep the inner 2-axis rotation
  (`SphMap_inv ; MatrixMap_3x3 ; SphMap_fwd`) as it is — it's
  intrinsically a 2-axis operation.  Embed it in N-axis form via
  `PermMap` + parallel `CmpMap` (with `UnitMap_(N-2)` on the other
  axes) + inverse `PermMap`.  Mirrors C
  `fitschan.c:38430-38473` (the `nax_rem > 0` branch and the
  surrounding PermMap sandwich at `38325-38346`).

### Step 3: Spectral mirror

Apply the same shape change to `wcs_spectral::build_spectral`.  The
`SpecMap` is already 1-axis; embed it in an N-axis mapping with
`UnitMap`s on the other axes.

### Step 4: Series composition

Refactor `wcs_read::build_iwc_to_world` (and the helper
`build_general_iwc_to_world`) to compose the celestial / spectral /
other contributions in **series**, not parallel.  Each contribution
is now N-axis with the relevant axes carrying the transform and the
others passing through, so simply chaining them in series is
correct.

The output-permutation `PermMap` that `build_general_iwc_to_world`
currently appends (to handle non-FITS-axis-order parallel layouts)
goes away — the per-class N-axis mappings are already in FITS axis
order on both ends.

### Step 5: Update fset_to_store consumers

The writer-side code in `fset_to_store` walks the FrameSet's
pixel→WCS mapping and looks for a `WcsMap` somewhere inside via
`split_around_wcsmap` and `find_wcsmap` (in
`fset_to_store/celestial_axes.rs`).  Both helpers already cope
with the WcsMap being nested inside a `CmpMap` tree, but the
N-axis structure means the WcsMap is at a shallower depth and may
already be N-axis.  Audit and adjust:

- `find_wcsmap` searches recursively into `CmpMap` children — should
  continue to work but now finds a top-level N-axis WcsMap.
- `split_around_wcsmap` (mirrors C `SplitMap2`) — already handles
  arbitrary series/parallel combinations, but should be re-tested
  against the new shape.
- `axlon_0based()` / `axlat_0based()` on the WcsMap — read the same
  fields, no change needed.
- `make_int_world::primary_frame` — already routes via
  `specframe::primary_frame_walk`, no change needed.

### Step 6: Test surface

Run the full test suite after each step:

```bash
cargo test -p ast-core --lib                  # 908 tests
cargo test -p ast-core --test test_wcsconv    # 192 tests (currently 181 passing)
cargo test --workspace
```

Expected outcomes:

- `wcsconv_cases::sip_3d_swap_fits_wcs` should pass after Step 4.
- The SIP, LSST, and degen1 fixtures should be unaffected (their
  primary chain shape is already simple).
- `class_write_fits_class` and `jan_fits_class_*` are likely
  unaffected (they use FITS-CLASS encoding, primary-only).
- The remaining DSS / offset_sky fixture failures have unrelated
  root causes per `wcsconv-residual-classification.md`.

### Step 7: Idiomatic-Rust cleanup pass

Once the refactor is green, follow the project's "C-first,
idiomatic-Rust second" rule and clean up the new helpers:

- The `embed_celestial_in_naxis` helper can be a private function
  in `wcs_celestial.rs` taking
  `(inner_2axis: DynMapping, naxis: usize, lon_axis: usize,
  lat_axis: usize) -> AstResult<DynMapping>` and returning the
  PermMap-parallel-PermMap sandwich.
- A similar `embed_1axis_in_naxis` helper in `wcs_spectral.rs`.
- Consider whether the embedding helpers can share a trait-method
  on `Mapping` (`embed_in_naxis`) for symmetry, but only if the
  C reference suggests this — the C source builds the embedding
  inline at each call-site.

## Risk assessment

The refactor touches the construction of the pixel→WCS mapping
that **every** wcsconv fixture exercises.  A naïve implementation
that gets axis indexing wrong will silently produce wrong CRVAL /
CRPIX values on round-trip.  Mitigations:

- After Step 1, re-run all wcsconv tests; the count must remain at
  181/192 before proceeding.
- After Step 2, the `build_iwc_to_sky_mapping` test (in
  `wcs_celestial.rs::tests`) should still pass with the same
  numerical outputs.
- After Step 4, run `cargo test --workspace` — the FrameSet round-
  trip tests in `tests/test_frameset.rs` and the encoder
  fixture tests should all stay green.

## Out of scope for this refactor

- DSS-encoded fixtures (`dss_ast_*`, `dss_fits_*`).  These have a
  separate `DssMap` construction path that the refactor does not
  touch.
- The `offset_sky_*` fixtures.  Their failure mode is a CTYPE
  encoding mismatch (`OFLN-TAN` vs bare `OFLN`) tracked separately.
- The `jan_fits_class_*` and `class_write_fits_class` failures.
  These are FITS-CLASS-specific encoder issues unaffected by the
  read-side refactor.
- The `degen1_fits_wcs` `CmpMap` nin/nout shape mismatch.  This is
  a degenerate-axis bug in the writer, not the read pipeline.

The refactor's scope is exactly the chain that `SIPIntWorld`
inspects on `sip-3d-swap` — which is also the cleanest C reference
shape, so the work has good ROI for downstream simplification
quality even if the only headline test that flips green is
`sip_3d_swap_fits_wcs`.

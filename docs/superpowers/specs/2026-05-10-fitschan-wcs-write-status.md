# FITS-WCS Write Status (2026-05-10)

Snapshot of the FITS-WCS encoder work after the
`tickets/DM-54793` session. Use this to pick up where the
session left off without re-deriving the architecture.

## Headline numbers

`cargo test -p ast-core --test test_wcsconv` reports
**104 passed / 88 failed / 192 total**.

Started the session at 101/192 — the visible failure pattern
(many `CRPIX = 50.5 / Reference pixel`-shaped diffs) read like a
"0.5 pixel offset" bug but turned out to be card-ordering plus
a small number of unrelated content gaps. The +3 net moves
came from genuine fixes (END card emission, CDELT/CROTA in
fits-aips, two pure-pixel cases that snapped clean once those
landed). Several other cases now match the canonical for
**most** lines but still trip on later content gaps tracked
below.

## Architecture facts you need before touching this code

These are non-obvious; the C library encodes a lot of policy in
the order it does things.

1. **The encoder must preserve input card positions.** When a
   FITS header is loaded then re-written, the C library's
   `WcsFromStore` updates existing keywords in place rather
   than purging-and-reappending. Card order ≈ identical to
   input. We implement this in `card_list.rs` via
   `mark_all_unused`/`sweep_unused_wcs` plus a `used` flag on
   each card; `put_or_append` flips the flag when the encoder
   touches a card and the post-encode sweep deletes any WCS
   card the encoder didn't re-write.

2. **CRPIX is solved, not extracted.** The C library's
   `MakeIntWorld` (`fitschan.c:21800`) picks an arbitrary root
   grid position `g₀ = (1, 1, …)`, evaluates pixel→IWC there
   to get `w₀`, evaluates the Jacobian to get `CD`, then
   solves `w₀ = CD · (g₀ − CRPIX)` for CRPIX via `palDmat`.
   We do the same in `wcs_write.rs` (`solve_crpix_from_decomp`),
   reusing `mappings::polymap::solve_linear_system`. This
   replaced an earlier tree-walking extractor that fell back
   to `(1, 1, …)` whenever the FrameSet's mapping started
   with a UnitMap (which it does for native AST inputs).

3. **CRVAL is fiducial-evaluated, not Jacobian-evaluated.**
   `GetFiducialWCS` (`fitschan.c:12919`) transforms the
   WcsMap's exact native fiducial point (e.g. `(0, π/2)` for
   zenithal projections, derived from the WcsMap's
   `nat_lon`/`nat_lat`) through the **post-WcsMap** sub-chain
   to get CRVAL bit-exact. Doing it via a numerical Jacobian
   on the full pixel→world introduces ~1e-11 drift that
   downstream simplification can't cancel. We implement
   `fiducial_crval` and use it for the celestial axes only;
   non-celestial axes still use the Jacobian-derived CRVAL.

4. **Celestial CD wants pixel→IWC, not pixel→world.**
   `CelestialAxes` (`fitschan.c:5293`) builds the WCS→IWC
   mapping as the inverse of
   `(WcsMap_inverted ∘ WcsNative ∘ deg2rad ∘ ShiftMap(ppcfid))`,
   composes pixel→world with WCS→IWC to get pixel→IWC, and
   extracts CD from that. Our `build_pixel_to_iwc` does the
   shape (omitting the ShiftMap because we don't yet handle
   non-zero `ppcfid` — relevant only for TPN and zenithal
   projections with `PVi_0a ≠ 0`).

5. **`build_native_to_celestial_rotation` must use the C
   library's elementary-rotation recipe.** The previous
   PAL `deuler("ZYZ", …)` based version mapped the native
   pole to CRVAL (the only point the read-path test
   exercises), but disagreed with FITS-WCS convention at all
   other points. The IWC composition lit this up immediately
   because it samples points off the pole. The matrix is now
   `R_z(π+α₀) · R_y(δ₀-π/2) · R_z(-LONPOLE)`, built directly
   in `wcs_celestial.rs:280`. Don't switch back to PAL
   without re-running the off-pole tests.

6. **Encoder card comments come from the input when possible.**
   `put_or_append` preserves an existing card's comment when
   overwriting — only newly-created cards use the encoder's
   default comment. Matches C-library behaviour where an
   input header's hand-written comment ("Linear pixel axis",
   "SIN projection", …) survives a round-trip. Encoder
   defaults are now the canonical AST comments
   ("Reference pixel on axis N", "Type of co-ordinate on
   axis N", "Transformation matrix element", etc.) so cases
   where the input had no comment also produce canonical
   output.

7. **`Mapping::simplify` is shallower than C's `astSimplify`.**
   The C library deep-traverses nested `CmpMap` chains,
   flattens them into a flat list, and recognises distant
   inverse-pairs as cancellable. Ours only merges adjacent
   siblings. This shows up in the IWC composition: the
   inverse-WcsNative pair sits inside a deeply-nested CmpMap
   and ours leaves a ~1e-11° rotation residue that the C
   library doesn't have. The `cmpmap_nested_inverse_cancel`
   regression fixture (added 2026-05-10, commit 768b6e9)
   demonstrates the gap with a hand-written .map and a
   C-library-generated .simp. The CROTA snap-to-zero in
   `enc_aips::write_to_cards` is an explicit workaround.
   When the simplifier closes that gap, remove the snap.

## What's been done (this session)

In commit order — every commit is small and focused so future
bisecting is easy.

| SHA | What it does | Pass-count delta |
|-----|--------------|------------------|
| `4063753` | Replace upfront `purge_wcs_keywords` with mark/sweep using the `used` flag. Card positions now survive a round-trip. Big invisible improvement; failure shape changes from "wrong keyword" to "wrong content". | 0 |
| `e6defcb` | CTYPE projection extracted from WcsMap (was hardcoded TAN); `BasicFrame` axes derive CTYPE from the per-axis label that the reader copies from input CTYPE; comment-on-overwrite preservation; encoder defaults rebased onto canonical AST strings. | 0 |
| `7953025` | `enc_aips::write_to_cards` now factors `store.cd` into (CDELT, PC) when CDELT/PC aren't already populated, and always emits CDELT and CROTAk. `to_header()` appends a trailing `END`. | +2 |
| `1323846` | Pixel→IWC via `split_pre_wcsmap` for celestial CD extraction. CD now extracted from the linear part of the chain only. | 0 |
| `6728e33` | Algebraic CRPIX solve (`palDmat` analogue) and `SkyFrame::norm` on the celestial CRVAL pair before degree conversion. timj's CRPIX moves from (1, 1) to (129, 128). | 0 |
| `d920bdf` | Replace PAL-deuler-based `build_native_to_celestial_rotation` with C-library elementary rotation; plumb full `WcsNative⁻¹ ∘ WcsMap_forward` IWC composition into `store_from_frameset`. timj off-diagonal CD now correct. | 0 |
| `5e7fee2` | Use post-WcsMap chain at native fiducial point for CRVAL (`fiducial_crval`); call `Mapping::simplify` on both pixel→world and the composed pixel→IWC; restore CROTA `< 1e-9°` snap to mask the simplifier-gap residue. | 0 |
| `7dfbfa0` | Drop the now-unused `LinearDecomposition.crpix` field. CI fix. | 0 |
| `768b6e9` | New regression fixture `cmpmap_nested_inverse_cancel.{map,simp}` documenting the simplifier deep-traversal gap. Fails `equal()` and serialization checks; transform-equivalence still passes. | 0 |
| `48c2438` | `[workspace.lints.rust] warnings = "deny"`. Local build now matches CI. | 0 |
| `9fdd61a` | `[workspace.lints.clippy] all = "deny"` plus cargo aliases (`lint`, `lint-all`, `fmt-check`) and `rustfmt.toml` locking edition. | 0 |
| `3411a56` | `.pre-commit-config.yaml` with file hygiene (excluding `vendor/`, `crates/ast-core/tests/fixtures/`, `reference/`), `cargo fmt --all`, `cargo clippy --workspace`. | 0 |

## What's left (in rough priority order)

Each entry names the failure cluster and the specific work
needed. Pass-count gain estimates are best-guess based on
the failures the cluster currently dominates.

### 1. Alt-WCS plane emission (timj cluster, ~5 tests)

`timj.fits-wcs` expects `WCSAXESA`/`WCSNAMEA` for the PIXEL
frame and `WCSAXESB`/`WCSNAMEB` for the AXIS frame, plus
their per-frame CRPIX/CRVAL/CTYPE/CD. The C library walks all
non-base/non-current frames in the FrameSet (`FsetToStore`
loop at `fitschan.c:12263–12259`) and assigns version letters
A–Z, calling `AddVersion` for each. We currently only emit
the primary plane.

**Next step:** extend `store_from_frameset` to iterate over
non-base/non-current frames, allocating version letters,
calling a new `AddVersion`-equivalent for each. Tests:
`timj_fits-wcs`, `multi-wcs_fits-wcs`, `multi-wcs-dup_fits-wcs`,
`cname_fits-wcs`, `gappt_fits-wcs`.

### 2. DATE-OBS derivation from MJD-OBS (~10 tests)

Several expected files have both MJD-OBS and DATE-OBS. We
emit MJD-OBS when `sky.epoch_mjd()` differs from J2000.0 but
never emit DATE-OBS. The C library converts MJD to ISO-8601
date-time via `palCtcd` / similar. Tests fail at line 12-15
with "got END / exp DATE-OBS=…".

**Next step:** in `wcs_write.rs::populate_celestial`, when
`mjdobs` is set, also compute the DATE-OBS string and store
it (need a new field on `FitsStore` and a write-step in each
encoder's `write_to_cards`). Tests: `sin-plain_fits-aips`,
`ncp_fits-aips`, `fk4-aips_fits-aips`, `timj_fits-iraf`,
`timj_fits-aips`, …

### 3. timj's MJD-OBS format bug

For `timj_fits-wcs`, our output has `MJD-OBS = 2002.3822`
where the canonical has `MJD-OBS = 52414.59`. We're emitting
the Julian epoch, not MJD. `populate_celestial` calls
`pal::epj(eq_mjd)` for EQUINOX (correct: Julian year for
EQUINOX) but `sky.epoch_mjd()` for MJD-OBS, which should be
correct. Suspicion: the `.ast` value `Epoch = 2002.3821786502`
may be loaded as a Julian epoch but stored as MJD without
conversion. Investigate `SkyFrame::set_epoch` and the
serial reader.

### 4. fits-pc card ordering for native input

`timj_fits-pc` expects PC matrix → CDELT → CRPIX → CRVAL →
CTYPE order for AST-native inputs. Our encoder emits in the
order CRPIX → CRVAL → CDELT → CTYPE → PC. The card-position
preservation logic only kicks in for keywords that already
exist in the input header; for native input, all WCS cards
are new, so the order falls back to whatever the encoder
emits. **Next step:** reorder the `write_to_cards` function
in `enc_pc.rs` to emit PC matrix first.

### 5. Genuine value bugs (~5 tests)

These all need root-cause investigation; the IWC composition
is correct architecturally but produces wrong values for
specific inputs.

- **lsst_fits-wcs**: CRPIX got `5235.83`, expected `5247.71`
  — close but ~12 pixels off. Also has `RA---TAN-SIP`
  expected, we emit `RA---TAN`. SIP distortion probably not
  being recognised in the input; the chain probably has a
  PolyMap that should drive a SIP read.
- **longslit_fits-wcs**: CRPIX2 got `1.0`, expected `1024.5`.
  CRPIX extraction failure — probably the same pattern as
  the timj UnitMap issue but on a non-celestial axis.
- **dss_fits-dss_ast**: Output is FITS not native — wrong
  encoding selected somewhere?
- **dss_ast_fits-wcs**: CRVAL1 got `-18.011`, expected
  `161.989` — exactly 180° offset. Look for missing
  longitude wrap.
- **freq-pixel3d_fits-aips**: CRPIX1 got `-907.4` with
  `CRVAL1=0`; expected `513.0` with `CRVAL1=1.4204e9`.
  Spectral axis CRPIX/CRVAL extraction picks the wrong
  reference point.

### 6. SIP / TPV distortion handling (lsst, sip-3d cluster)

CTYPE for SIP is `RA---TAN-SIP` etc. Our encoder doesn't
recognise the PolyMap that follows the WcsMap as a SIP
distortion, so we emit `RA---TAN`. The C library's
`SIPIntWorld` (`fitschan.c:31130-ish`) detects and serializes
SIP coefficients as `A_p_q`, `B_p_q`, `AP_p_q`, `BP_p_q`
keywords. Big chunk of work; defer.

### 7. CLASS encoder content gaps

`class-write_fits-class`, `class-gal_fits-class`, and several
`class-*_fits-class` tests fail at `NAXIS = 3 / Number of dimensions`
vs canonical `NAXIS = 3 / Number of dimensions` — wait, those
match now. Re-check whether these tests still fail and
where; the CLASS encoder is mostly working but a few
specifics may need attention.

### 8. Float exponent format (`E8` vs `E+8`)

`sip_fits-wcs` and `sip2_fits-wcs` line 27 differs:
`UTCS_OBS = 2.3635893E8` (us) vs `2.3635893E+8` (canonical).
Cosmetic; `format_fits_double` doesn't emit the explicit
plus sign. The C library's `palDfltf`/`fits.c` formatter
does. Adjust `format::format_fits_double` to match.

### 9. Reader errors (separate from write path)

These are read-path failures that block the write tests
because the test reads then writes:

- `a20070718_*`: `unknown or unsupported Frame class: DSBSpecFrame`.
  Need a DSBSpecFrame implementation; substantial work.
- `polco_native`, `polco2_native`, `specflux_native`,
  `timeplot_native`: `read error: expected 'Begin', got: AST            Beginning of AST data for FrameSet object`.
  The native reader doesn't handle the `AST -- AST` separator
  comment that AST-native dump uses between objects.
- `axes-swap_native`: `incompatible numbers of coordinates: series CmpMap: first.nout()=4 but second.nin()=3`.
  The reader is constructing a malformed mapping for axis-permuted inputs.
- `jan_fits-class_ast`: `NAXIS keyword not found in CLASS header`.
  CLASS reader requires NAXIS; should fall back gracefully.
- `car3_native`: `cannot determine number of WCS axes: neither WCSAXES nor NAXIS found`.
  Same.

### 10. Simplifier deep-traversal (tracked via fixture)

`cmpmap_nested_inverse_cancel` documents the gap. Closing it
would let us remove the CROTA snap-to-zero workaround in
`enc_aips`. Out of scope for the FITS-WCS write effort but
correlated.

## Where to look in the code

| Purpose | File |
|---------|------|
| Top-level encoder dispatch | `crates/ast-core/src/fitschan/mod.rs::write` (`write` impl on `Channel for FitsChan`) |
| FrameSet → FitsStore (the meat) | `crates/ast-core/src/fitschan/wcs_write.rs::store_from_frameset` |
| pre-WcsMap / post-WcsMap split | `wcs_write.rs::split_pre_wcsmap`, `split_around_wcsmap` |
| Algebraic CRPIX solve | `wcs_write.rs::solve_crpix_from_decomp` |
| Fiducial CRVAL extraction | `wcs_write.rs::fiducial_crval` |
| IWC composition | `wcs_write.rs::build_pixel_to_iwc` |
| WcsNative rotation | `wcs_celestial.rs::build_native_to_celestial_rotation` |
| Celestial CTYPE/CRVAL/RADESYS/EQUINOX | `wcs_write.rs::populate_celestial` |
| Spectral CTYPE/CRVAL/SPECSYS/RESTFRQ | `wcs_write.rs::populate_spectral` |
| Per-encoding `write_to_cards` | `enc_aips.rs`, `enc_iraf.rs`, `enc_pc.rs`, `enc_class.rs`, `enc_dss.rs` |
| Card position preservation | `card_list.rs::put_or_append` (used flag, comment preserve) |
| Card list mark/sweep | `card_list.rs::mark_all_unused`, `sweep_unused_wcs` |
| END card emission | `mod.rs::FitsChan::to_header` |
| Test harness | `crates/ast-core/tests/test_wcsconv.rs` |

## How to resume

```sh
git checkout tickets/DM-54793
cargo test -p ast-core --test test_wcsconv -- --nocapture | grep "wcsconv results"
# expect: 104 passed, 88 failed, 0 skipped (of 192 total)
```

To see the first three failures:

```sh
cargo test -p ast-core --test test_wcsconv -- --nocapture | tail -30
```

To dump full got vs expected for one named case, drop a
small diagnostic test in `crates/ast-core/tests/diag_*.rs`:

```rust
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};

#[test]
fn diag_one_case() {
    let text = std::fs::read_to_string(
        "crates/ast-core/tests/fixtures/wcsconv/inputs/timj.ast",
    ).unwrap();
    let obj = ast_core::from_native_string(&text).unwrap();
    let mut fc = FitsChan::new();
    fc.set_encoding(Encoding::FitsWcs);
    fc.set_fits_digits(8);
    fc.set_cd_matrix(true);
    fc.write(&obj).unwrap();
    eprintln!("{}", fc.to_header());
}
```

Run with `cargo test -p ast-core --test diag_one_case -- --nocapture`.
Don't commit the diagnostic; use it for quick visibility.

## Reference (C library)

Built locally for fixture generation:

```sh
cmake -B reference/starlink-ast/build-ast \
      -S reference/starlink-ast \
      -DCMAKE_BUILD_TYPE=Release
cmake --build reference/starlink-ast/build-ast --target simplify
# binary at reference/starlink-ast/build-ast/ast_tester/simplify
```

Use it to generate `.simp` references for new simplification
fixtures:

```sh
DYLD_LIBRARY_PATH=reference/starlink-ast/build-ast \
  reference/starlink-ast/build-ast/ast_tester/simplify \
  input.map output.simp
```

The submodule is gated by the project's policy (everyday
`cargo build` should not need it); use it for fixture
generation, design reference, and oracle benches.

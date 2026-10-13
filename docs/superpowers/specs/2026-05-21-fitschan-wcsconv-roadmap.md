# FitsChan WCS-conversion roadmap (2026-05-21)

A campaign-level design that catalogues every failing case in
`cargo test -p ast-core --test test_wcsconv`, groups them by
root cause, and orders the fixes so each tier unblocks the next.

This roadmap supersedes the 2026-05-10 status snapshot
(`docs/superpower/specs/2026-05-10-fitschan-wcs-write-status.md`),
incorporating its findings and adding categories it didn't cover.

Each tier becomes its own implementation plan in a follow-up
session via `writing-plans`. This document is the campaign
contract; per-tier plans own the diff-level details.

## Status as of writing

```
cargo test -p ast-core --test test_wcsconv -- --nocapture | grep wcsconv
=== wcsconv results: 104 passed, 88 failed, 0 skipped (of 192 total) ===
```

## Goal

**192 / 192 passing.** No deferred edge cases.

The goal is bit/string parity with the C library's
`wcsconverter` reference output for every fixture in
`crates/ast-core/tests/fixtures/wcsconv/`. The path to that
parity is "follow the C implementation" — when the existing
Rust shape can't express what `fitschan.c` needs to do, we
change the Rust shape rather than work around it. We do not
re-architect speculatively, but no part of the FitsChan stack
(including `FrameSet → FitsStore`, `populate_*`, encoder
dispatch, or the simplifier interface) is off-limits if the
C implementation calls for it.

## Guiding principles

1. **Follow the C library.** When in doubt, read
   `reference/starlink-ast/src/fitschan.c` and mirror its
   structure. The C code encodes 30 years of policy that the
   Rust port has to honour exactly to keep canonical output
   bit-stable.
2. **Card-position preservation is sacred.** The mark/sweep
   logic in `card_list.rs` (`mark_all_unused` / `sweep_unused_wcs`
   / `put_or_append`) is what keeps round-trips stable. Don't
   regress it.
3. **CRPIX is solved, CRVAL is fiducial-evaluated, CD is from
   pixel→IWC.** These three contracts (documented in the
   2026-05-10 snapshot) survive this campaign unchanged. Fixes
   plug into them, not around them.
4. **Tier 1 first.** Read failures block downstream write tests
   on the same fixture. Resolve the parser before chasing
   write-side residue.

## Failure-mode catalogue (10 categories, ~88 tests)

Each row: identifier letter, name, test count, side (R = read,
W = write), and one-line root cause.

| #  | Category                                          | Tests | Side  | Root cause |
|----|---------------------------------------------------|------:|-------|-----------|
| A  | Native-encoding & parser bugs                     | 6     | R     | Keyword-card NATIVE encoding (`BEGAST_A=`, `ENDAST_A=`, etc.) not implemented (4 tests, `polco`/`polco2`/`specflux`/`timeplot`); WCSAXES/NAXIS fallback missing (1 test, `car3_native`); axes-swap CmpMap dimension mismatch (1 test, `axes-swap_native`) |
| B  | CLASS encoder — content & axis convention         | 9     | R+W   | CLASS expects spectral axis as axis 1 (`FREQ` first); read errors when NAXIS absent |
| C  | Spectral axis collapsed into spatial during write | 12    | W     | `populate_spectral` feeds CRVAL1 from celestial path verbatim; spectral CRVAL ends up as a sky longitude |
| D  | AIPS / AIPS++ encoder logic                       | ~20   | W     | (i) AIPS++ should emit CDELT+PC not CD when no shear; (ii) CROTA sign wrong; (iii) NCP→SIN missing PROJP; (iv) EPOCH-MJD vs Besselian-year format; (v) SFL→GLS at-origin shift; (vi) missing DATE-OBS / MJD-OBS in several |
| E  | CRPIX shift propagation in write                  | 5     | W     | `MakeIntWorld` algebraic solve fails when chain has upstream pixel ShiftMap or unresolved SIP polynomial |
| F  | Simplifier deep-traversal gap (CD/CRVAL residue)  | 6     | W     | Simplifier deep-traversal gap leaves 1e-14 residue on inverse-WcsNative chains; encoder emits residue as cards, shifting line positions and burying real CD2_2 / CDELT1. **Fixed by closing K, not by snapping at the encoder.** |
| G  | SIP / TPV distortion writeback                    | 4     | W     | PolyMap downstream of WcsMap-TAN not detected as SIP; CTYPE missing `-SIP` suffix; A_p_q / B_p_q cards not emitted |
| H  | Multi-WCS / alt-plane emission                    | 5     | W     | C library walks all non-base/non-current frames, allocates version letters A–Z, emits a per-frame WCSAXES{X}/CTYPE{X}/etc plane; we only emit primary |
| I  | DSS encoder                                       | 3     | R+W   | Reader: TPN→TAN simplification missing, longitude wrap missing. Writer: card emission order wrong (PLTRAH before CNPIX1; SIMPLE before native FrameSet) |
| J  | Misc edge cases                                   | ~9    | mixed | degen1 (degenerate axis squeezed); azel (Az/El frame CRVAL ≈ 0); offset-sky (OFLN-TAN suffix instead of OFLN); tan-pv0 (PV1_0 not preserved); pixel-shear CRVAL near-zero; float `E+8` vs `E8` cosmetic |

Cross-cutting: the simplifier deep-traversal gap (tracked by
`cmpmap_nested_inverse_cancel` fixture) underlies category F's
residue and the existing CROTA snap workaround in `enc_aips`.
**It is closed properly as item K, not papered over with an
encoder-side snap.** The existing CROTA snap in `enc_aips`
gets deleted as part of K.

## Reconciliation with 2026-05-10 snapshot

Items in the prior doc map to this catalogue as follows:

| Prior item                                  | This roadmap |
|---------------------------------------------|--------------|
| 1. Alt-WCS plane emission (timj cluster)    | H            |
| 2. DATE-OBS derivation from MJD-OBS         | D-pre (tier 2 prereq for D) |
| 3. timj's MJD-OBS format bug                | D            |
| 4. fits-pc card ordering for native input   | within H + D |
| 5. Genuine value bugs (lsst, longslit, dss, freq-pixel3d) | E + I + C |
| 6. SIP / TPV distortion handling            | G            |
| 7. CLASS encoder content gaps               | B            |
| 8. Float exponent format E8 vs E+8          | J1           |
| 9. Reader errors                            | A            |
| 10. Simplifier deep-traversal               | K (in scope) |

New categories not in the prior doc: **C** (spectral
collapse, ~12 tests), **F** (cleanly attributed to the
simplifier deep-traversal gap rather than per-encoder
content, ~6 tests), several specific D-tier items (AIPS++
CDELT+PC, CROTA sign, NCP→SIN, EPOCH-MJD, SFL→GLS), the
OFLN suffix and AzEl items in J.

## Dependency-ordered tiers

Each tier is internally parallelisable; tiers run sequentially
because earlier ones unblock later ones (or remove workarounds).

### Tier 1 — read-side blockers (~5 tests + unblocks others)

- **A1.** Implement keyword-card NATIVE encoding (`enc_native.rs`). The C library detects this variant via "any keyword starting with BEGAST" (`fitschan.c::Read` line 25699) and routes through the inherited Channel reader using a keyword-card source function that reassembles `BEGAST_xx`/`ENDAST_xx`/data-keyword cards back into a `Begin Class … End Class` text stream. The Rust port currently only supports the COMMENT-card variant. Add a new `crates/ast-core/src/fitschan/enc_native.rs` mirroring the per-encoding pattern of `enc_aips.rs`/`enc_class.rs`/etc. that converts keyword-card NATIVE input to a native-text string; the existing `serial::native_read::parse_native` then takes over. Tests: `polco_native`, `polco2_native`, `specflux_native`, `timeplot_native`.
- **A2.** Native parser — when WCSAXES and NAXIS both absent, infer axis count from highest CTYPEi index. Test: `car3_native`. Files: `crates/ast-core/src/fitschan/wcs_read.rs`.
- **A3.** axes-swap CmpMap dimension mismatch — investigate `wcs_read.rs::build_pixel_to_world` PermMap insertion when CTYPE order is permuted. Test: `axes-swap_native`.
- **B1.** CLASS reader fallback when NAXIS absent. Tests: `jan_fits-class_ast`, `jan_fits-class-rt`. Files: `crates/ast-core/src/fitschan/enc_class.rs`.

Estimated impact: 104 → ~112.

### Tier 2 — cross-cutting writes

- **K.** Close the simplifier deep-traversal gap (mirror C
  library `astSimplify`'s deep CmpMap walk + flat-list
  inverse-pair recognition). Documented by the
  `cmpmap_nested_inverse_cancel` fixture. Closing K removes
  the 1e-14 residue at the source, which (a) fixes the
  category-F tests cleanly without an encoder-side snap, and
  (b) lets us delete the existing CROTA snap in
  `enc_aips::write_to_cards`. Tests: `gappt_fits-wcs`,
  `ukirt-dtai_fits-wcs`, `galactic-tan_fits-iraf`,
  `sin-plain_fits-iraf`, `fk4-aips_fits-iraf`,
  `fits-pc-altgal_fits-wcs`, `pixel-shear_fits-aips++`. Files:
  `crates/ast-core/src/mappings/cmpmap.rs`,
  `crates/ast-core/src/mappings/simplify.rs`. Removal: delete the CROTA
  snap branch in `enc_aips::write_to_cards` once K passes.
- **H1.** Alt-WCS plane emission. Mirror C library `FsetToStore` (`fitschan.c:12251-12341`): walk non-base/non-current frames, allocate version letters A–Z, call a new `add_version_to_store(store, frame, letter)` helper that mirrors the primary path. Plumb version letter through `populate_celestial` / `populate_spectral` / each encoder's `write_to_cards`. Cards emit after primary plane. Tests: `timj_*` (4), `cname_fits-wcs`, `multi-wcs_fits-wcs`, `multi-wcs-dup_fits-wcs`, `tan-pv0_fits-wcs`, `date-z_fits-wcs`. Files: `wcs_write.rs`, every `enc_*.rs`.
- **D-pre.** DATE-OBS derivation from MJD-OBS. Add `dateobs: Option<String>` to `FitsStore`; in `populate_celestial` when `mjdobs` is set, also format ISO-8601 from MJD via PAL or pure-Rust calendar math; each encoder's `write_to_cards` emits DATE-OBS card after MJD-OBS. Prerequisite for parts of D. Tests: `sin-plain_fits-aips`, `ncp_fits-aips`, `fk4-aips_fits-aips`, `timj_fits-iraf`, `timj_fits-aips`, `date-z_fits-wcs`, plus partial credit for several others.

Estimated impact: 112 → ~133. K is sequenced first within
tier 2 because clean CD/CRVAL values upstream make tier 3's
per-encoder diagnoses easier to read.

### Tier 3 — per-encoder content (parallel across encoder files)

- **D1.** AIPS++ — emit CDELT+PC instead of CD when no shear. Reuse the AIPS-path factorisation already in `enc_aips`. Tests: `timj_fits-aips++`, `tan-cdmatrix_fits-aips++`, `sin-plain_fits-aips++`, `fk4-aips_fits-aips++`, `pixel-1d_fits-aips++`, `pixel-2d_fits-aips++`, `pixel-rotated_fits-aips++`, `galactic-tan_fits-aips++`. File: `enc_aips.rs::write_to_cards` (AIPS++ branch).
- **D2.** AIPS — CROTA sign convention. Re-derive from C library `MakeBasicWcs`. Tests: `tan-rotated_fits-aips`, `pixel-rotated_fits-aips`, `fk4-aips_fits-aips`. File: `enc_aips.rs::factor_cd_to_cdelt_crota`.
- **D3.** AIPS — NCP→SIN with PROJP terms. When `ctype.projection() == Ncp`, rewrite to `RA---SIN` and emit `PROJP2 = cot(δ₀)`. Tests: `ncp_fits-aips`, `ncp_fits-aips++`, `ncp_fits-iraf`. File: `enc_aips.rs::populate_celestial_aips` (and IRAF equivalent).
- **D4.** AIPS — EPOCH format MJD vs Besselian. Emit MJD value (51544.5 for J2000) for FK5/ICRS rather than Julian year. Test: `ecliptic-tan_fits-aips`. File: `enc_aips.rs`.
- **D5.** AIPS / IRAF — SFL→GLS CRVAL2 at-origin shift. When projection is SFL/GLS and CRVAL2 ≠ 0, transform to equator (mirror C library `SflGlsAtOrigin`). Tests: `sfl-origin_fits-aips`, `sfl-origin_fits-aips++`, `sfl-origin_fits-iraf`, `sfl-pc_fits-pc`. Files: `enc_aips.rs`, `enc_iraf.rs`, `enc_pc.rs`.
- **B2.** CLASS encoder — emit spectral axis as axis 1. Permute CD rows/columns accordingly. Mirror C library `Class` writer. Tests: `class-write_fits-class`, `class-gal_fits-class`, `class-arc_fits-class`, `class-ait_fits-class`, `class-sin_fits-class`, `class-sfl_fits-class`, `class-olddate_fits-class`, plus the spectral CLASS roundtrip cases. File: `enc_class.rs::write_to_cards`.
- **I1.** DSS reader — TPN→TAN simplification when polynomial coefficients zero; longitude wrap into [0, 360). Test: `dss_ast_fits-wcs`. File: `enc_dss.rs::read_from_cards`.
- **I2.** DSS writer — re-order card emission to match C library DSS output. Tests: `dss_fits-dss_ast`, `dss_ast_dss`. File: `enc_dss.rs::write_to_cards`.
- **J1.** Float exponent format `E+N` for positive exponents. File: `format::format_fits_double`. Cosmetic; affects `sip*` tests partially.

Estimated impact: 133 → ~165.

### Tier 4 — distortion & tail (heaviest design work)

- **C1.** Spectral-axis decomposition for writeback. Restructure `populate_spectral` parallel to `populate_celestial`: derive CRVAL/CDELT/CTYPE from the spectral-frame portion of the chain, not from the celestial pixel→IWC composition. Likely needs a `split_pre_specmap` analogue and an `IwsComposition` (intermediate world for spectral) parallel to the existing IWC. **This is the most likely place to need FrameSet → FitsStore restructuring** — design first, code after. Tests: `skyspec3d_*` (5), `vrad-lsrd_*` (2), `vopt-geo_*` (2), `freq-ghz-bary_*` (2), `freq-pixel3d_*` (2), `wave-1d_fits-iraf`, `vopt-f2w_fits-wcs`, `wave-f2w_fits-wcs`. Files: `wcs_write.rs::populate_spectral`, possibly new `wcs_spectral_decompose.rs`.
- **E1.** CRPIX algebraic-solve robustness. Detect upstream pixel ShiftMap and fold its offset into CRPIX before the algebraic solve. Defer the SIP-aware path until G1 lands. Partial fix only without G1 (gets a20070718 + longslit; lsst still fails). Tests: `a20070718_fits-wcs/aips/aips++/iraf` (3), `longslit_fits-wcs`. File: `wcs_write.rs::solve_crpix_from_decomp`.
- **G1.** SIP / TPV detection on write. Mirror C library `SIPIntWorld` (`fitschan.c:31116-ish`): in `populate_celestial`, after IWC composition, detect a PolyMap with degree ≥ 2 in the post-WcsMap chain; emit SIP coefficients via a new `populate_sip` helper; suffix CTYPE with `-SIP`. Tests: `sip_fits-wcs`, `sip2_fits-wcs`, `sip-3d_fits-wcs`, `sip-3d-swap_fits-wcs`, plus residual on `lsst_fits-wcs`. Files: new `wcs_sip.rs`, hooks in `wcs_write.rs`.
- **J2.** `degen1_fits-wcs` — degenerate axis preservation. WCSAXES output 2 vs expected 3.
- **J3.** `azel_fits-wcs` — AzEl frame CRVAL extraction. CRVAL1 ≈ 0 vs expected 185.
- **J4.** `offset-sky_*` (2 tests) — OFLN frame shouldn't carry projection suffix; emit `OFLN` not `OFLN-TAN`.
- **J5.** `tan-pv0_fits-wcs` — preserve PV1_0 fiducial-offset card through write.

Estimated impact: 165 → 192.

### On K specifically

K (simplifier deep-traversal closure) is sequenced first within
tier 2 and is **mandatory**. We do not introduce an encoder-side
snap as a workaround; we fix the simplifier the way C does so
the residue never reaches the encoder.

The C library's `astSimplify` deep-traverses nested CmpMap chains,
flattens them into a flat list, and recognises distant inverse-pairs
as cancellable. Ours only merges adjacent siblings. The gap is
documented by the `cmpmap_nested_inverse_cancel` fixture (commit
`768b6e9`).

Closing K is a `Mapping::simplify` change, not a FitsChan change.
Once it lands:
- Category F's six tests pass with exact CD/CROTA values
  (`pixel-shear_fits-aips++`, currently dual-counted under J,
  also closes here).
- The existing CROTA snap branch in `enc_aips::write_to_cards`
  gets deleted in the same PR (or the immediately following one).
- The `cmpmap_nested_inverse_cancel` fixture's expected-failure
  marker gets removed.

#### K — fixture-driven workflow

The simplifier work must be testable in isolation, with no
FitsChan code in the loop. For each failing wcsconv case in
category F, capture the exact mapping that the simplifier is
mis-handling and turn it into a tier-2 simplify fixture:

1. **Capture the input mapping.** From a diagnostic test, run
   the wcsconv path up to (but not including) the simplifier
   call, then `to_native_string` the mapping that's about to
   be simplified. Write it to
   `crates/ast-core/tests/fixtures/simplify/<name>.map`.
2. **Generate the C oracle reference.** Run the C library's
   `simplify` binary on the captured `.map`:
   ```sh
   DYLD_LIBRARY_PATH=reference/starlink-ast/build-ast \
     reference/starlink-ast/build-ast/ast_tester/simplify \
     fixtures/simplify/<name>.map fixtures/simplify/<name>.simp
   ```
   The `.simp` file is the canonical post-simplification
   serialisation. Commit both `.map` and `.simp` together with
   a commit message naming the wcsconv case they came from.
3. **Wire it into the manifest.** Add an entry to
   `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`:
   ```
   <name> | simplify_fixtures/<name>.map | simplify_fixtures/<name>.simp |
   ```
   The existing manifest-driven harness picks it up
   automatically — no new Rust test code needed. Place the
   `.map` and `.simp` files under
   `crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/`
   alongside the existing focused-rule fixtures
   (`unit_series_elision`, `zoom_series_merge`, etc).
4. **Develop K against the fixtures.** The tier-2 plan for K
   iterates the simplifier until every captured fixture
   passes; this stays out of the FitsChan code path entirely
   and is independently bisectable.

This is the same pattern as the existing focused-rule
fixtures under `simplify_fixtures/` (and the integration
fixture `cmpmap_nested_inverse_cancel` from commit `768b6e9`);
K extends the fixture set rather than introducing a new
mechanism. Expected new fixtures, one per category-F wcsconv
case (names indicative): `gappt_iwc_residue.{map,simp}`,
`ukirt_dtai_iwc_residue.{map,simp}`,
`galactic_tan_iraf_iwc_residue.{map,simp}`,
`sin_plain_iraf_iwc_residue.{map,simp}`,
`fk4_aips_iraf_iwc_residue.{map,simp}`,
`fits_pc_altgal_iwc_residue.{map,simp}`,
`pixel_shear_aipspp_iwc_residue.{map,simp}`. If multiple
cases reduce to the same minimal mapping, dedupe — one
fixture per distinct simplifier failure mode is enough.

## Estimated cumulative pass-count

| After tier | Passing |
|------------|--------:|
| current    | 104     |
| Tier 1     | ~112    |
| Tier 2     | ~133    |
| Tier 3     | ~165    |
| Tier 4     | 192     |

These are best-guess based on the test-cluster overlap; actual
deltas will be measured per implementation plan.

## Why this ordering

- **Tier 1 first** — read failures block any future write test
  on the same fixture (e.g. `polco_native` failing means a
  later `polco_fits-*` test never runs).
- **K before D-tier** — the simplifier residue currently
  breaks line positioning across many AIPS/IRAF tests. Closing
  K produces exact CD/CRVAL values upstream, so tier 3's
  per-encoder diagnoses are read against clean data and several
  D-tier symptoms will vanish or simplify.
- **H1 early in tier 2** — many "WCSAXES at line N vs CTYPE1
  expected" failures are alt-plane symptoms, not real
  card-content bugs. Fixing this clears noise.
- **D-pre as tier-2 prerequisite** — DATE-OBS plumbing is
  reused by every encoder; doing it once in tier 2 simplifies
  every D-item in tier 3.
- **C1 in tier 4** — heaviest design item; deserves its own
  implementation plan and may require FrameSet → FitsStore
  restructuring. Don't let it gate the tier-3 quick wins.
- **E1 partial without G1** — lsst's CRPIX gap is a SIP
  symptom; fixing CRPIX alone gets a20070718 + longslit. lsst
  closes when G1 lands.

## Files that will see most of the work

Indicative; per-tier plans will own specifics.

| Layer                  | File                                              |
|------------------------|---------------------------------------------------|
| FrameSet → FitsStore   | `crates/ast-core/src/fitschan/wcs_write.rs`       |
| Celestial decomposition| `wcs_write.rs::populate_celestial`, `wcs_celestial.rs` |
| Spectral decomposition | `wcs_write.rs::populate_spectral`, `wcs_spectral.rs`, possibly new `wcs_spectral_decompose.rs` |
| Distortion (SIP/TPV)   | new `wcs_sip.rs`                                  |
| Per-encoder            | `enc_aips.rs`, `enc_iraf.rs`, `enc_pc.rs`, `enc_class.rs`, `enc_dss.rs` |
| Card list / ordering   | `card_list.rs::put_or_append`, `mark_all_unused`, `sweep_unused_wcs` |
| Format helpers         | `format.rs::format_fits_double`                   |
| Native parser          | `crates/ast-core/src/serial/native_read.rs`       |
| Simplifier (tier K)    | `crates/ast-core/src/mappings/cmpmap.rs`, `crates/ast-core/src/mappings/simplify.rs` |

## Done criteria

- `cargo test -p ast-core --test test_wcsconv` reports
  **192/192 passing**.
- All five tier-1 read failures resolved.
- `cargo test -p ast-core --test test_fitschan_wcs` and
  `test_fitschan_wcs_write` continue to pass.
- `cargo test --workspace` is green.
- No regression in `test_native_roundtrip_all` or any
  `test_simplify_*` suite.
- K closed: `cmpmap_nested_inverse_cancel` no longer
  expected-failure; CROTA snap branch removed from
  `enc_aips::write_to_cards`.

## How tiers become implementation plans

Each tier is owned by a separate `writing-plans` session that
produces a focused implementation plan. Per-plan scope:

- **Tier 1 plan:** A1 + A2 + A3 + B1, all read-side, mostly
  parser fixes. Should land in 1–2 commits.
- **Tier 2 plan:** K (simplifier), H1 (alt-plane), D-pre
  (DATE-OBS). Three sub-plans, one PR each. K lands first
  (separate from FitsChan code, in `mappings/`); the CROTA-snap
  deletion piggybacks on K's PR or the next one. H1 is the
  largest piece because it touches every encoder. D-pre is
  small and lands alongside H1.
- **Tier 3 plans:** parallelisable per encoder. Suggest one
  plan covering D1–D5 (AIPS / AIPS++ family), one for B2
  (CLASS), one for I1 + I2 (DSS), one for J1 (cosmetic).
- **Tier 4 plans:** C1 (spectral) and G1 (SIP) each get their
  own design-heavy plan. E1 piggybacks on G1's plan or runs
  alongside. J2–J5 can be a single tail-cleanup plan.

K's outcome is unconditional across the whole campaign:
F is closed by K, no fallback encoder-side snap is introduced.

## Out of scope

- ABI / CFFI surface changes (no public API additions needed)
- Performance / hot-path concerns (FitsChan is not on the
  SCUBA-2 hot path)
- Tier 5 oracle benchmark drift (separate suite)
- Speculative re-architecture of FrameSet → FitsStore beyond
  what the C library calls for (re-architecture *is* on the
  table where the C path requires it; we just don't refactor
  for its own sake)

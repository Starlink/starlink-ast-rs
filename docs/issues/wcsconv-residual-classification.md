# wcsconv residual classification (26 failures, 2026-06-04)

> **HISTORICAL DOCUMENT (resolved).** wcsconv now passes 192/192 (0 ignored);
> every bucket classified here has been cleared. Retained as a record; no
> outstanding work.

Snapshot of remaining `cargo test -p starlink-ast --test test_wcsconv -- --include-ignored`
failures on `tickets/DM-54793` after the SpecTrans port (166/192 passing).

Cleared since the original 2026-06-02 snapshot:
- **Bucket A (FITS-PC alt-axis Cnxxx reader)** — 8 fixtures cleared by the
  SpecTrans port (translation 6 rewrites Cnxxx → *A..*Z).
- **Bucket D (F2W CTYPE)** — 2 fixtures cleared by the SpecTrans translation
  18 + the wcs_spectral NonLinSpecWcs port.
- **Bucket J (SFL-at-origin AIPS)** — 4 fixtures cleared by the AIPS
  encoder fix.
- Several singletons (azel, fits-iraf-cd-old, vopt-geo on aips++, …).

Remaining 26 fixtures cluster around 7 buckets (write-side issues
mostly; the SpecTrans port covered the read side comprehensively):

| Bucket | Count | Description | Path forward |
|--------|------:|-------------|--------------|
| B (CLASS encoder) | 8+2 | class_*_fits_class + jan_*_class | Port C `ClassFromStore` (write side) |
| H (SIP write) | 4 | sip*_fits_wcs | SIP encoder + comment leak |
| I (offset-sky) | 2 | offset-sky*_fits_wcs | OFLN-OFFSET branch |
| K (CRPIX integer-snap, 1D/3D) | 4 | vopt-geo, zpn, longslit, a20070718_fits_iraf | Port AIPS spectral integer-snap |
| Singletons | 4 | degen1, lsst, dss×3 | Per-fixture |
| FITS-PC last-plane-wins | 2 | fits-pc-altgal, fits-pc-gal | Galactic alt-plane B1950 epoch fallback (writer-side propagation) |
| Singletons (CLASS) | 1 | skyspec3d-class | CLASS spectral CRPIX |

The previous classification (kept below for context) was written
when 55 fixtures failed.

---

## Original snapshot (2026-06-02)

Each bucket lists the fixtures and the root cause that makes them fail.
**No bucket should be addressed with a heuristic** — each one corresponds to a
specific branch of `reference/starlink-ast/src/fitschan.c` that needs to be
ported line-for-line.

## Bucket A — FITS-PC alt-axis (`Cnxxx` packed form) reader missing (8)

The FITS-PC encoder stores alt-axis records in the `CnXXX` packed form
(distinct from the `*A..*Z` letter suffix used by FITS-WCS). Our reader
(`wcs_keywords::detect_alt_letters`) only scans the suffixed
`CRPIXnX` form, so alt-A planes round-trip-disappear.

- `timj_fits-aips`
- `timj_fits-aips++`
- `timj_fits-iraf`
- `timj_fits-pc`
- `fits-pc-altgal_fits-wcs`
- `fits-pc-gal_fits-pc`
- `multi-wcs-dup_fits-wcs` (adjacent secondary-plane handling)
- `lsst_fits-wcs` (alt-A `NORMEDPIXELS` and alt-B `INTERMEDIATE0` planes)

**Fix:** dedicated FITS-PC alt-axis reader pass, mirroring C `Cnxxx` parsing.

## Bucket B — CLASS encoder structurally wrong (8)

The `fits-class` (ALMA/CLASS dialect) write path is incomplete: wrong axis
ordering, wrong velocity-axis CRVAL/CDELT, missing rest-frequency,
missing `DELTAV`, missing `VELO-LSR`, missing `RADESYS=FK5` propagation.

- `class-ait_fits-class`
- `class-arc_fits-class`
- `class-gal_fits-class`
- `class-olddate_fits-class`
- `class-sfl_fits-class`
- `class-sin_fits-class`
- `class-write_fits-class`
- `skyspec3d-class_fits-class`
- `jan_fits-class_ast` and `jan_fits-class-rt` (CLASS-family residuals)

**Fix:** port C `astWriteFits` CLASS branch from `fitschan.c`.

## Bucket C — 3D spectral CRPIX integer-snap missing (6+)

Expected output shows `CRPIX3=513.0` and `CRVAL3=0.0` (or `+500`); we emit
`CRPIX3=512.0` and `CRVAL3=-1000` (or `-500`). Pattern: input has
`CRPIX3=512.5`; C resolves the half-pixel by snapping to 513 *and* shifting
the reference value by one channel via the spectral mapping.

- `vrad-lsrd_fits-aips`, `vrad-lsrd_fits-aips++`
- `vopt-geo_fits-aips`, `vopt-geo_fits-aips++`
- `freq-ghz-bary_fits-aips`, `freq-ghz-bary_fits-aips++`
- `freq-pixel3d_fits-aips`, `freq-pixel3d_fits-aips++`

**Fix:** port AIPS-flavour spectral pass from C `SpecTrans` / `SpectralAxes`
that bumps CRPIX to the nearest integer and re-derives CRVAL.

## Bucket D — F2W (frequency-to-wavelength) CTYPE suffix not emitted (2)

Expected `CTYPE1='WAVE-F2W'` / `'VOPT-F2W'`; we emit unsuffixed `'WAVE'` /
`'VOPT'`. The frequency-derived CRVAL also differs because the algorithm
applies a non-linear transform.

- `wave-f2w_fits-wcs`
- `vopt-f2w_fits-wcs`

**Fix:** detect SpecMap chain in `enc_wcs.rs` and emit the WCS3
"alternate spectral algorithm" suffix (`-F2W`, `-W2F`, `-V2F`, etc.) per
Greisen-Calabretta paper III §3.

## Bucket E — 1D spectral wavelength precision drift (3)

Same off-by-1 pixel + linear residue as bucket C, but on 1D fixtures.

- `wave-1d_fits-iraf`
- `a20070718_fits-iraf` (partial; also see bucket F)

**Fix:** same as bucket C.

## Bucket F — `a20070718` reader produces Pixel-Frame headers (4)

We emit `AXIS1`/`AXIS2` headers (no celestial). Expected has full
`RA---TAN`/`DEC--TAN` celestial. Reader fails to detect the celestial
component in this fixture, producing a naked Pixel frame instead of
CmpFrame(Sky+Spec).

- `a20070718_fits-wcs`
- `a20070718_fits-aips`
- `a20070718_fits-aips++`
- `a20070718_fits-iraf`

**Fix:** targeted reader debug — likely `wcs_celestial::wcs_skyframe`
rejects an unusual CTYPE/RADESYS/EPOCH combination.

## Bucket G — DSS dialect (3)

Structural; DSS encoder/decoder still incomplete.

- `dss_ast_dss`
- `dss_ast_fits-wcs`
- `dss_fits-dss_ast`

**Fix:** dedicated DSS work, separate ticket.

## Bucket H — SIP encoding/decoding (4)

- `sip_fits-wcs` — extra blank/section comment cards passing through
  (input has section markers we should drop); `1.797693135E+308` BAD
  coefficients leaking through.
- `sip2_fits-wcs` — same blank/section cards.
- `sip-3d_fits-wcs` — emits `RA---TAN`/`DEC--TAN` rather than
  `RA---TAN-SIP`/`DEC--TAN-SIP`; missing `CRPIX1/2/3` and `CD3_3`
  (3D spectral axis dropped); SIP suffix not added when SIP is present.
- `sip-3d-swap_fits-wcs` — same as sip-3d but with axis-swap; the swap
  is wrong (DEC value emitted under CRVAL1 still labeled RA).

**Fix:** SIP write path — detect a residual PolyMap and emit `-SIP` CTYPE
suffix plus `Ai_j`/`Bi_j` keys. Currently we strip the SIP and write
plain TAN. Header comment passthrough leak in `sip_fits-wcs` is a separate
fixture-prep bug.

## Bucket I — Offset-sky / `SkyFrame.SkyRefIs` = "Origin"|"Pole" (2)

Expected emits `OFLN-TAN`/`OFLT-TAN` CTYPE plus `WCSNAME=SKY_ORIGIN` /
`SKY_POLE` plus an alt-A plane carrying the pre-offset RA/DEC frame.
We emit `OFLN`/`OFLT` (no projection suffix) and miss WCSNAME and the
alt-A plane.

- `offset-sky_fits-wcs`
- `offset-sky-pole_fits-wcs`

**Fix:** port C `SkySys` `OFLN-OFFSET` branch (around the `OFLN` token
table in `fitschan.c`); auto-add un-offset SkyFrame as a secondary
description.

## Bucket J — SFL-at-origin AIPS GLS quirk (4)

Input has `GLS` projection at origin (CRVAL2=0). Expected emits `GLS` with
CRVAL=(0,0) and proper CDELT. We emit `GLS` but with `CRVAL2=-90, CDELT=1.0`
— the SkyFrame got reoriented to celestial pole instead of the input origin.

- `sfl-origin_fits-aips`
- `sfl-origin_fits-aips++`
- `sfl-origin_fits-iraf`
- `sfl-pc_fits-pc`

**Fix:** port AIPS GLS "if origin on pole, emit (0,0) CRVAL" shortcut from
C `SpecialAips` / `AIPSFromStore`.

## Bucket K — Singletons / numeric residue (~7 unique)

Each is its own small bug:

- `azel_fits-wcs` — missing `OBSGEO-X/Y/Z`, `MJD-OBS`, `DATE-OBS` in primary;
  CRVAL slightly off (185.005 → 185.0). AzEl SkyFrame's observatory
  location not propagating to FITS store.
- `tan-pv0_fits-wcs` — CRVAL2 sign flip (-45 vs +45), missing
  `CD1_1`/`CD2_2`, missing `CRPIX1/2`. PV1_0 native-shift handling: our
  writer emits the unrotated frame. Port C `WcsNoWcs` PV-aware decompose.
- `ncp_fits-aips`, `ncp_fits-aips++`, `ncp_fits-iraf` —
  `CD2_2 = 0.00027780013` vs `0.0002778`; final-digit residue from CD
  recovery, ~3.6e-7 relative. SplitMat threshold revisit.
- `degen1_fits-wcs` — wrong axis order (WAVE first; expected third).
  Axis permutation lost when input had a degenerate axis.
- `longslit_fits-wcs` — pending detailed read.
- `zpn_fits-pc` — wrong `CDELT` (1.0 vs ±0.0667); missing `CRPIX`/`LATPOLE`.
  Same family as tan-pv0 (PV-distorted projection write path).
- `skyspec3d_fits-wcs` — only missing `VELOSYS = -19195.766`
  (topo→barycentric apparent velocity not computed/emitted). Small
  targeted port of C `SpecCelestialAxes` velocity computation.

## Summary

| Bucket | Count | Difficulty | C-side reference |
|---|---|---|---|
| A. FITS-PC `Cnxxx` reader | 8 | Medium | `fitschan.c` `Cnxxx` parser |
| B. CLASS encoder | 8+ | Medium-High | `fitschan.c` CLASS branch |
| C. 3D spectral CRPIX integer-snap | 6+ | Medium | `SpecTrans` / `SpectralAxes` |
| D. F2W algorithm | 2 | Medium | WCS3 §3 suffix detection |
| E. 1D spectral residue | 3 | Medium | Same as C |
| F. a20070718 reader | 4 | Low-Medium | `wcs_skyframe` debug |
| G. DSS | 3 | High | DSS dialect; separate ticket |
| H. SIP encode | 4 | Medium | SIP write branch + fixture cleanup |
| I. Offset-sky | 2 | Medium | `SkySys` OFLN-OFFSET branch |
| J. SFL-at-origin | 4 | Low | `SpecialAips` / `AIPSFromStore` |
| K. Singletons | ~7 | Mixed | Per-fixture |
| **Total** | **55** |  |  |

## Recommended attack order

Easy wins first, ramping toward harder numeric work. None require heuristics;
each maps to a specific C branch.

1. **VELOSYS emit** (skyspec3d) — single missing keyword
2. **AzEl OBSGEO emit** (azel) — propagate observatory from AzEl SkyFrame
3. **SFL-at-origin shortcut** (4 fixtures) — small AIPS branch
4. **a20070718 reader debug** (4 fixtures) — find why celestial detection fails
5. **Offset-sky CTYPE + WCSNAME** (2 fixtures) — OFLN-TAN suffix + alt-plane
6. **F2W CTYPE suffix** (2 fixtures) — straightforward branch
7. **SIP encoder + sip fixture comment leak** (4 fixtures)
8. **CLASS encoder** (8 fixtures) — biggest unit, full port of CLASS branch
9. **3D + 1D spectral CRPIX integer-snap** (9 fixtures) — port `IntegerCRPIX`
10. **FITS-PC `Cnxxx` reader** (8 fixtures)
11. **DSS, tan-pv0, zpn, degen1, longslit, ncp residue** — last

## Reproduction

The 55 `.diff` files were generated by running:

```
rm -rf /tmp/wcsconv_out/* && /tmp/classify_failures.sh
```

against the `tickets/DM-54793` HEAD at commit `5522f6a`.

To regenerate:

```
cargo test -p starlink-ast --test wcsconv -- --include-ignored 2>&1 \
  | grep -E '^(test |FAIL)' \
  | tee /tmp/wcsconv_status.txt
```

then the per-fixture diff dump in `/tmp/wcsconv_out/<fixture>.diff`.

# Frame and Mapping classes: C-vs-Rust port gaps

Status: partial audit (2026-06-09).
This report records verified gaps from a class-level inventory sweep, a stub/workaround grep, and cross-referencing the existing audit docs.
The deep per-class logic comparisons (section 5) have **not** been done yet and are listed as a step-by-step work queue for a dedicated agent.
Every item below was verified by reading the code at the cited location; items needing re-verification are marked.

## 1. Verified missing functionality (fix these)

Ordered roughly by impact.

- **[HIGH] PolyMap — `astPolyTran` not ported.**
  C `polymap.c` fits an inverse (or forward) polynomial by least squares (`PolyTran`/`FitPoly1D`/`FitPoly2D`, iterative order increase, `acc`/`maxacc`/`maxorder`).
  Nothing in Rust (`grep poly_tran` → 0 implementations).
  `crates/ast-core/src/fitschan/wcs_matrix.rs:205-208` explicitly works around the gap by enabling only the iterative inverse for SIP headers — exactly the C-divergence this report exists to eliminate.
  Fix: port `PolyTran` and its fitting machinery, then remove the wcs_matrix workaround.

- **[HIGH] FluxFrame — `MakeFluxMapping` not ported.**
  C `fluxframe.c` converts between flux systems (FLXDN/FLXDNW/SFLUX/SFLUXW) using `SpecVal`.
  Rust returns `None` for any cross-system conversion (`crates/ast-core/src/frames/fluxframe.rs:369`, `:634`; test note at `:805`).

- **[HIGH] SpecFluxFrame — cross-system conversion missing.**
  `crates/ast-core/src/frames/specfluxframe.rs:197` only converts to another SpecFluxFrame with identical spec and flux systems; C `specfluxframe.c` `MakeMap` composes the SpecFrame and FluxFrame conversions (including the spectral-position-dependent flux scaling).
  Depends on the FluxFrame item above.

- **[MED] Frame — `astReadDateTime` only partially ported.**
  `crates/ast-core/src/frames/date_time.rs:4` ports only the decimal-year branch of C `frame.c:8892`; the Besselian/Julian-epoch prefix (`B1950.0`, `J2000.0`), MJD/JD, and Gregorian date/time string forms are missing.
  Any Epoch string-setting path through ast-capi will mis-handle those forms.

- **[MED] PermMap — `PermSplit` attribute not ported.**
  C `permmap.c` `astGetPermSplit` selects an alternative `MapSplit` strategy; `crates/ast-core/src/mappings/perm.rs:401` notes it is not a settable attribute in the crate.

- **[MED] Mapping — `astRebin`/`astRebinSeq`/`astResample`/`astTranGrid`/`astQuadApprox` absent.**
  No `fn rebin|resample|tran_grid|quad_approx` anywhere in `crates/ast-core/src`.
  These are large self-contained subsystems of `mapping.c` (spreading/interpolation kernels, adaptive linear-approximation subdivision).
  They are required for the frozen C ABI (step 10/11) and used by SMURF; decide a delivery step and track in PLAN.md rather than leaving unscheduled.

- **[MED] DssMap — cannot be constructed from a DSS-encoded FitsChan.**
  `crates/ast-core/src/mappings/dssmap.rs:41`: only native-dump deserialisation works; the C `fitschan.c` DSS encoding path is unported.

- **[LOW] WcsMap — `Unknown` projection rebuilt as CAR placeholder.**
  `crates/ast-core/src/mappings/wcsmap.rs:278` substitutes a CAR projection when rebuilding `PrjParams` for `WcsProjection::Unknown`; C `wcsmap.c` carries a real "no projection" state (`AST__WCSBAD` type with no prj dispatch).
  Verify what transform/attribute behavior C gives an Unknown-type WcsMap and mirror it instead of aliasing CAR.

- **[LOW] Serialisation — JSON renderer from design step 5 absent.**
  Native read/write exists (`serial/native_read.rs`, `native_write.rs`) but no JSON/XML/YAML renderer despite the spec listing JSON in step 5.

- **[PROCESS] PLAN.md is empty.**
  It contains only a title line, so the C→Rust test-migration tracker that CLAUDE.md mandates does not exist.
  Reconstruct it from `docs/superpower/plans/` and the design spec's delivery order; without it, "missing vs not-yet-scheduled" cannot be policed.

## 2. Absent classes (confirm scheduling, not bugs)

- **IntraMap**: no Rust code; the spec sketches an `Arc<dyn IntraTransform>` plugin trait but no delivery step covers it.
- **XphMap** and **Moc**/**MocChan**: absent; XphMap's only C consumer is `moc.c`.
- **Stc\*** classes and **YamlChan/StcsChan/XmlChan**: absent (step 8/9 remainder).
- **wcslib**: still vendored C behind FFI (`src/wcslib.rs`); step 6a (Rust port, delete `vendor/wcslib/`) not started — by design, do not rewrite piecemeal.

## 3. Deliberate divergences (do NOT "fix")

`docs/issues/c-library-quirks.md` records where Rust mirrors or intentionally diverges from C bugs.
Key entries a fix agent must not regress: MatrixMap MapSplit zero-init (C has UB), SpecMap float-cast of RefRA/RefDec (mirrored for parity), `ast_dat` pre-1972 divisor bug (mirrored), VELOSYS string-quantisation drift (mirrored), DATE-OBS fractional-seconds parsing (Rust is correct, fixture adjusted).

## 4. Prior mapping audit — re-verify before fixing

`docs/mapping-gaps-audit.md` (2026-05-08) is a detailed Rust-vs-C audit of the mapping classes: per-axis BAD-value handling, equality bugs (CmpMap/MatrixMap/PermMap/WinMap/WcsMap), ~28 missing simplification rules (SlaMap/TimeMap/SphMap/MatrixMap/WinMap/WcsMap/LutMap/PermMap/PolyMap/SpecMap/ZoomMap/ChebyMap), CmpMap recursion-guard/PatternCheck gaps, and missing analytic `rate()` overrides.
A fix plan (`docs/superpower/plans/2026-05-08-mapping-gaps-fix.md`) followed it, so an unknown subset is already fixed.
First task for the fix agent: re-test each numbered item in that audit against current code and strike the resolved ones, then fold the survivors into section 1 ordering.

## 5. Audit work queue (not yet compared against C)

These class groups still need a function-by-function comparison; none has been done.
For each: enumerate the C vtab overrides and attribute branches, locate Rust counterparts (logic may be centralized in `simplify.rs`, `find_frame.rs`, `map_box.rs`, `rate.rs`, `linear_approx.rs`), and deep-compare the algorithmic methods listed.

1. **Frame base** (`frame.c` ~16.5k + `axis.c` ~3.5k vs `frames/mod.rs`, `basic.rs`, `find_frame.rs`): Match/Subframe/Overlay/Convert, Angle/Offset/Offset2/Distance/Resolve family, Norm/NormBox, Gap/Format/Unformat/Abbrev/Fields, LineDef family, MatchAxesX, ActiveUnit, attribute dynamic defaults (Digits→Format, Label/Symbol/Unit), AST__BAD handling.
2. **SkyFrame + SkyAxis** (`skyframe.c` + `skyaxis.c` vs `skyframe.rs`, `skyaxis.rs`, `sky_convert.rs`): MakeSkyMapping system paths, SkyRef/SkyRefIs/AlignOffset offset-rotation, great-circle Line* geometry, sexagesimal Format/Unformat specifier coverage, Gap, AsTime/NegLon defaults, cache invalidation on ObsLocation/Epoch.
3. **SpecFrame + DSBSpecFrame** (`specframe.c`, `dsbspecframe.c`): MakeSpecMapping rest-frame table, source-velocity corrections, SpecOrigin, unit adaptation; DSB sideband mapping, ImagFreq, AlignSideBand.
4. **TimeFrame + FluxFrame + SpecFluxFrame** (`timeframe.c`, `fluxframe.c`, `specfluxframe.c`): MakeTimeMapping timescale chains, TimeOrigin, iso-date Format/Unformat/Gap; plus section 1 items.
   Cross-check `docs/issues/timeframe-and-epoch-api-port.md` first.
5. **CmpFrame** (`cmpframe.c` ~11k vs `cmpframe.rs` ~2.4k): PartMatch/SubFrame axis partitioning, per-axis attribute routing through the perm array, nested CmpFrames, Domain/System defaults, component-wise Transform/Norm/Gap.
6. **FrameSet** (`frameset.c` ~13.6k vs `frameset.rs` ~1.2k): RemoveFrame renumbering + invert-flag bookkeeping, GetMapping path finding, integrity remapping, variant mappings (AddVariant/MirrorVariants/Variant), Frame/Mapping delegation completeness.
7. **Mapping base** (`mapping.c` vs `mod.rs`, `rate.rs`, `map_box.rs`, `linear_approx.rs`): Rate algorithm fidelity, MapBox global-bounds search constants/convergence, LinearApprox fit + tolerance test, MapSplit default, Invert/Report semantics.
8. **CmpMap + TranMap + simplify engine** (`cmpmap.c`, `tranmap.c` vs `cmpmap.rs`, `tranmap.rs`, `simplify.rs`): invert-flag bookkeeping through MapList flattening, nominate/swap protocol, TranMap cancellation rules, TranForward/TranInverse AND/OR logic.
   Cross-check section 4 and `docs/issues/simplify-serialization-gaps.md` first.
9. **PolyMap + ChebyMap** (`polymap.c`, `chebymap.c`): scalar Transform parity (SIMD mode exempt), IterInverse Newton-Raphson details, ChebyDomain/out-of-bounds behavior, coefficient validation.
10. **Linear family** (`matrixmap.c`, `permmap.c`, `shiftmap.c`, `winmap.c`, `zoommap.c`, `unitmap.c`, `pcdmap.c`): transform edge cases (singular/non-square MatrixMap, PermMap constants, PcdMap inverse series), MtrRot/MtrMult presence, MapSplit overrides.
11. **Sphere family** (`wcsmap.c`, `sphmap.c`, `dssmap.c`, `normmap.c`, `unitnormmap.c`): PV defaults per projection, NatLat/NatLon, longitude wrapping, SphMap PolarLong/UnitRadius, NormMap/UnitNormMap merge math.
    Cross-check `docs/issues/sph_fwd_inv_unitradius_cancel_analysis.md` first.
12. **Spectral/time maps** (`specmap.c`, `grismmap.c`, `slamap.c`, `timemap.c`): full conversion-code coverage and math parity, SlaMap cached precession matrices, leap-second table currency, GrismMap inverse.

## 6. Working rules for the fix agent

- Port the C logic; never paper over a gap with a heuristic the C does not have (repo convention).
- Before claiming something missing, grep the whole crate — much C per-class logic is centralized in shared Rust modules.
- Respect the SkyFrame setter hot path (O(1), allocation-free, lock-free) and the documented MatrixMap no-BLAS decision.
- Mirror C bugs only where `c-library-quirks.md` says parity requires it, and add to that doc when a new quirk is found.

# PLAN.md — Rust AST port: outstanding-work clearing house

Ticket DM-54793. This file is the single index of active, audited, outstanding
work in the Rust port. When a gap is found and written up, link it here so the
write-up is discoverable and does not go stale in isolation. When work lands,
update the status column (and, when a whole area closes, strike it through with
the closing commit).

Convention: detailed analysis lives in `docs/audits/` (structured gap sweeps) or
`docs/issues/` (per-subsystem port-gap catalogues). This file holds only the
one-line pointer and current status for each. A finding closes to one sentence
here, and its write-up moves to
[`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md) under the
same section number, so this file stays cheap to read in full.

---

## 1. Region classes

| Area | Doc | Status |
|---|---|---|
| Phase 5 cross-cutting parity (simplify / uncertainty machinery) | `docs/audits/region-phase5-parity-spec.md` | **CLOSED** — all 8 gaps landed (RB-PICK/MERGE-FF/SETUNC/DEFUNC/SIMP-STAB/CE-FIT + R-UNC/PR-MAP by refactor); final commit `a96bc80` |
| 4 critical `equal()` failures + shared root-cause primitives (R-PINS, R-OVL) | `docs/audits/region-synthesis-plan.md` | **CLOSED** — all 4 fixtures pass; `d834b81` closed the last (cap_slamap_07) |
| Post-Phase-5 coverage sweep (broader vtab override audit) | `docs/audits/region-class-coverage-audit.md` | CLOSED except the two Plot-gated items — re-checked against the code on 2026-10-13: MESH-GEO, PLY-DOWN, GRID-FRM, BND-INT, ACC-PARS and CIR-NB3 are done (table below); RT-TRACE and RS-SPLIT wait on a Plot / Grf backend |

Per-area source audits feeding the two closed docs above (kept for reference):
`region-base.md`, `region-box-interval-pointlist.md`,
`region-circle-ellipse-polygon-null.md`, `region-cmpregion-prism.md`.

### Outstanding region findings (from the coverage sweep)

Detail and C↔Rust locations in `docs/audits/region-class-coverage-audit.md`.

| ID | Gap | Gated behind | Priority |
|---|---|---|---|
| MESH-GEO | Polygon `reg_base_mesh` geodesic, by the Frame's `distance` and `offset` (polygon.c:4329) | — | DONE (cde69449) |
| PLY-DOWN | Polygon `Downsize` / `DownsizePoly` | — | DONE (281a38b5) |
| GRID-FRM | Frame-aware `astFrameGrid` for `reg_base_grid` | — | DONE (3ebd56c6) |
| BND-INT | Interval `BndBaseMesh` override (interval.c:215) | — | DONE (d49e0082) |
| RT-TRACE | `RegTrace` boundary tracing unported (base + 6 classes) | Plot/Grf backend | deferred |
| RS-SPLIT | CmpRegion `RegSplit` disjoint split unported (cmpregion.c:2472) | Plot/Grf backend | deferred |
| ACC-PARS | Geometry accessors: `circle_pars` and `ellipse_pars`, C's `astCirclePars` and `astEllipsePars` | — | DONE |
| CIR-NB3 | Circle `reg_base_box` calls `norm_box` after both branches, as C calls `astNormBox` (circle.c:1100) | — | DONE (3acfc833) |

## 2. Frames and Mappings

| Area | Doc | Status |
|---|---|---|
| Frame + Mapping class C-port gaps (function-by-function) | `docs/issues/frame-mapping-c-port-gaps.md` | OPEN — audit COMPLETE; remaining items are deferred delivery-step work only (JSON/XML/YAML renderers, absent classes, wcslib port 6a, Plot-support Frame methods + NormalPoints, SkyAxis Format precision) |
| Frame family C-parity audit (2026-10-05) | `docs/audits/frame-family-c-parity.md` | CLOSED 2026-10-10 — every finding across Frame, SkyFrame, SpecFrame, DSBSpecFrame, TimeFrame, FluxFrame, SpecFluxFrame, CmpFrame and FrameSet-as-Frame is fixed or recorded as not a defect, except two LOW items: SK-18 (SkyFrame line methods, needed by Plot) and CF-2 (CmpFrame `RemoveRegions`, which follows from CmpFrame not being a Mapping, §9.6) |
| Mapping classes C-parity audit (2026-10-06) | `docs/audits/mapping-c-parity.md` | CLOSED 2026-10-07 — every finding is fixed, recorded as not a defect, or recorded as a deliberate divergence (PY-10, the PolyTran fitter); CH-4, the ChebyMap PolyTran, was ported on 2026-10-14 against the pinned C, ahead of upstream Starlink/ast pull request 81, whose changes to it are listed in the CH-4 row; CH-8 (a forward box kept with no forward coefficients) is open. Twelve C defects found on the way are fixed on `u/timj/norm-unit`, past the submodule pin. XphMap (2026-10-07) and IntraMap (2026-10-08) are ported |
| FitsChan C-parity audit (2026-10-10) | `docs/audits/fitschan-c-parity.md` | CLOSED 2026-10-13 except FA-6 in part — of the 66 findings, every one is fixed or recorded as not a defect (the Progress table, and FA-22 and FF-11 in their rows). Open: with Clean set, a NATIVE read that fails does not remove the cards it had read (FA-6, NOT PORTED); a read that fails partway reports the items of the whole description rather than those after the failure, since the port loads a description whole; and C's habit of saving and restoring the card cursor in about a dozen functions, recorded for upstream review. `RefCnt` and `Nobj` are not written, by design |
| Region classes C-parity audit (2026-10-08) | `docs/audits/region-c-parity.md` | CLOSED 2026-10-09 except PG-5, a performance matter deferred — 59 findings (15 HIGH, all probed against C; 28 MED; 16 LOW). The missing per-class seams (OverlapX, BndBaseMesh, RegSetAttrib/RegClearAttrib, Mask, the forwarded Frame methods) come first: an unbounded Interval overflows the stack in `overlap`, Adaptive has no effect, and `StcResourceProfile` waits on them. Supersedes the older `region-*.md` audits. `StcResourceProfile` landed on 2026-10-09 |
| Per-class Mapping performance vs C oracle | `docs/issues/mapping-perf-vs-c-oracle.md` | OPEN — measured with `ast-bench` (`crates/ast-bench/README.md`); records in `docs/perf/` (README lists them). `ast-bench compare --set bench --base main` is the by-hand gate; CI runs only the harness's own tests, no timing. Bench set on an M1 Ultra (2026-10-16 record), Rust/C geometric mean: 0.05 at N=1, 0.15 at N=10, 0.58 at N=100, 2.05 at N=100k, 1.92 at N=1e6; worst `cmpmap_parallel` ~14×. Older hand-run snapshot (2026-05-19) records cheap mappings 5–42× slower than C at N=100k (permmap-fwd 42×, winmap 6.6–7.6×, shiftmap 5.7–7.6×, diag matrixmap 5.5×, slamap 3.9×, wcsmap-tan-fwd 4.2×); PolyMap/MatrixMap — the SCUBA-2 / LSST path — remain at or ahead of parity. Untested hypothesis worth measuring before per-class work: `PointSet` is row-major `(npoint, ncoord)` where C keeps one contiguous array per axis, which would explain the whole cheap-mapping cluster at once. Transform memory (2026-10-10): nested series CmpMaps held a full copy of the points per level, fixed by porting cmpmap.c's 8192-point batches; PolyMap's iterative inverse (4× its output) and RateMap (2×) still hold more than their output, and parallel CmpMaps copy where C uses views; see the doc's 2026-10-10 addendum |
| `astTran*` definedness validation | `docs/audits/mapping-c-parity.md` | DONE 2026-10-07 — every `transform` / `transform_into` makes C's base-`Transform` checks (`mappings::validate_transform`: AST__TRNND, AST__NCPIN, AST__NOPTS, AST__NOCTS), as C's `astTransform` does; `tran1` / `tran2` keep their own arity checks. MB-1 in the audit |
| CmpMap `inv_a`/`inv_b`: split the load contract from the dump-shaping emulation | `docs/issues/cmpmap-invert-snapshots.md` | DEFERRED to post-oracle-compliance — measured, with a falsifiable go/no-go test: after the §8 pin move, re-run the load-drift probe and collapse the mechanism only if it reports zero |
| TimeFrame port + Epoch/Equinox API | `docs/issues/timeframe-and-epoch-api-port.md` | CLOSED 2026-10-13 — Issue A (DATE-OBS/TDB drift) resolved; Issue B: Epoch is documented as TDB, the Equinox as an MJD with no timescale of its own that C's FITS-WCS writer takes as UTC (`SkyFrame::equinox_mjd`), and the `*_year` setters stay public as the typed form of C's bare-year string path |
| `NormUnit` | `crates/starlink-ast/tests/test_frame.rs` | CLOSED — `Frame::norm_unit`, with the SkyFrame override C has; write-up in [`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md) §2 |
| SkyFrame `Format`, `Digits`, `AsTime` and `Unit` | `crates/starlink-ast/tests/test_skyframe_format.rs` | CLOSED 2026-09-30 — the attributes, `DHmsFormat` / `DHmsUnit` / `ParseDHmsFormat`, the default-Format rules, dump/load and Overlay, pinned by 1800 formatted values, 120 Format/Unit/NormUnit triples and 14 byte-exact dumps captured from C by `tools/oracle_fixtures/skyframe_format_probe.c`. The Overlay port also copies Label and Symbol, which it did not before |
| A SkyAxis picked into a plain Frame is not a SkyAxis | `crates/starlink-ast/tests/test_picked_skyaxis.rs` | CLOSED 2026-09-30 — a plain Frame holds each axis as a plain `Axis` or a `SkyAxis` (`FrameAxis`), and SkyFrame's SubFrame attaches the picked SkyAxis with AsTime, Format, Label, Symbol, Unit, IsLatitude and CentreZero set as skyframe.c:10230 sets them, so the picked axis formats, unformats, normalises, reports its NormUnit and dumps as `Begin SkyAxis` as C's does. Pinned by 1820 values and 42 byte-exact dumps from `tools/oracle_fixtures/picked_skyaxis_probe.c`, including the `Format(1)=bhms` case (`3 49 11`, NormUnit `hh mm ss`) |
| SkyAxis `AxisUnformat` ignores the Format | `crates/starlink-ast/src/frames/skyaxis.rs` (`unformat_sky_axis`) | CLOSED 2026-09-30 — a port of skyaxis.c:4068: the separators, suffix field identifiers, angle-or-time inference and expected fields from `ParseDHmsFormat`, the 60-limit check and C's left-to-right accumulation; a SkyFrame reads with its own Format, set or default, as skyframe.c:10892 does. Pinned by the `Unformat` rows of `test_picked_skyaxis.rs`, which cover letter separators (`3h49m11s`, `45d30m`) |
| Plain Frame attributes | `crates/starlink-ast/tests/test_frame_attrs.rs` | CLOSED 2026-09-30 — every Frame attribute on BasicFrame (System, AlignSystem, ObsLat, ObsLon, ObsAlt, Dtai and Dut1 added; test/clear for all), C's defaults (Title, Label and Symbol from the internal axis, Format `%1.<Digits>G`), CleanDomain, the Epoch as an MJD, the axis permutation kept as C keeps it, the full Frame dump and the Axis card order, and Overlay, pinned by 1147 values and 30 byte-exact dumps from `tools/oracle_fixtures/frame_attrs_probe.c`. Phase 1 of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md`; phase 2 puts these attributes on the `Frame` trait |
| Frame-class attributes on the `Frame` trait | `crates/starlink-ast/tests/test_frame_class_attrs.rs` | CLOSED 2026-09-30 — Title, Domain, Epoch, Digits, ObsLat, ObsLon, ObsAlt, Dtai, Dut1, Ident, MinAxes, MaxAxes, MatchEnd, PreserveAxes, Permute and ActiveUnit, with set/test/clear, on every Frame class, a FrameSet and every Region, stored in the shared `FrameAttrs`; CmpFrame and SpecFluxFrame share Epoch and the observatory attributes with their components as C does. Pinned by 3584 values from `tools/oracle_fixtures/frame_class_attrs_probe.c`. Phase 2a of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md`; phase 2b puts the per-axis attributes on the trait |
| Per-axis attributes on the `Frame` trait | `crates/starlink-ast/tests/test_frame_axis_attrs.rs` | CLOSED 2026-09-30 — Label, Symbol, Unit, Format, Digits(axis), Direction, Top and Bottom, with set/test/clear, on every Frame class, a FrameSet and every Region, each taking a 1-based axis and returning `AstResult`. SpecFrame, TimeFrame and FluxFrame now hold an `Axis`, and `SkyAxis` wraps one. A CmpFrame routes to its components and lends them its Digits as cmpframe.c does, and reads Top and Bottom through the component's Axis object. Pinned by 4370 values from `tools/oracle_fixtures/frame_axis_attrs_probe.c`, which exposed wrong FluxFrame Symbols, a SpecFrame Symbol that fell back to the Label, a CmpFrame plain-component Symbol of `"Axis N"` rather than `"x<n>"`, a TimeFrame Format that ignored Digits, a FluxFrame dump without `Ax1`, and SpecFrame, TimeFrame and FluxFrame Overlays that copied only the Unit. Phase 2b of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md` |
| SkyFrame attributes | `crates/starlink-ast/tests/test_skyframe_attrs.rs` | CLOSED 2026-09-30 — System, AlignSystem, Equinox, Epoch, NegLon, Projection, SkyRef(axis), SkyRefIs, SkyRefP(axis), SkyTol, AlignOffset, AsTime(axis), IsLatAxis(axis), IsLonAxis(axis), LatAxis and LonAxis, with set/test/clear where C has them, and the SkyFrame defaults of Title, Label, Symbol, Unit, Format, Direction, Top and Bottom, pinned by 1548 values and 43 byte-exact dumps from `tools/oracle_fixtures/skyframe_attrs_probe.c`. It exposed a NegLon that ignored its offset-origin default, SkyRefP with no computed default, SkyRef taken as a (lon, lat) pair rather than per external axis (so `is_offset` missed a longitude-only SkyRef), NegLon and AlignOffset dumped when true rather than when set, SRef dumped for an unset axis, and a FITS-WCS reader that skipped C's CRVAL SkyRef for an offset SkyFrame behind a domain test C does not have. A SkyFrame's own SkyAxes have IsLatitude unset, as C's do; see `docs/issues/c-library-quirks.md`. Phase 3 of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md` |
| TimeFrame, SpecFrame, DSBSpecFrame and FluxFrame attributes | `crates/starlink-ast/tests/test_class_attrs.rs` | CLOSED 2026-09-30 — every class attribute (TimeScale, AlignTimeScale, TimeOrigin, LTOffset; StdOfRest, AlignStdOfRest, RestFreq, RefRA, RefDec, SourceVel, SourceVRF, SourceSys, SpecOrigin, AlignSpecOffset; DSBCentre, IF, SideBand, AlignSideBand, ImagFreq; SpecVal) and each class's System and AlignSystem, with get, set, test and clear, the getters giving C's defaults, and current-unit accessors where C's attribute is in the Frame's units. Each class writes the Frame part of its dump through the shared `write_frame_cards`, read through the `Frame` trait, as C's subclass `Dump` calls the Frame class's. Pinned by 811 values and 61 byte-exact dumps from `tools/oracle_fixtures/class_attrs_probe.c`, which exposed: SourceVRF and SourceSys that did not re-express SourceVel (`ConvertSourceVel` was ported only to heliocentric velocity), RestFreq, RefRA, RefDec, SourceVel and SpecOrigin with no default, a TimeFrame date label that lost its Local Time suffix, a FluxFrame label that ignored its unit-derived System, missing Ident, AlSys and Lbl1 cards, and a FluxFrame dump without most of the Frame part. Phase 4 of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md` |
| SkyFrame and CmpFrame `Resolve`, `ResolvePoints` and `Intersect` | `crates/starlink-ast/tests/test_sky_geometry.rs` | CLOSED 2026-09-30 — SkyFrame resolves and intersects on great circles (skyframe.c:8321, `Intersect`) rather than inheriting the planar Frame versions, and a CmpFrame resolves each component's part of the vector and combines them (cmpframe.c:7753, :7962). Pinned bit for bit by `tools/oracle_fixtures/sky_geometry_probe.c`, permuted and not, with the bad and degenerate cases; C's `ResolvePoints` axis-order mix-up is fixed upstream on `u/timj/norm-unit` (5a2cac2a, past the submodule pin) and the port follows the fix, so the permuted `ResolvePoints` cases need that commit to agree (`docs/issues/c-library-quirks.md`). Still open from the audit: CmpFrame `IsUnitFrame`, UseDefs, and `astSkyOffsetMap`, `astGetRefPos` and `astCurrentTime` |
| `MatchAxes` on every Frame class | `crates/starlink-ast/tests/test_match_axes.rs` | CLOSED 2026-09-30 — the `MatchAxesX` overrides of SkyFrame (the whole SkyFrame is matched, skyframe.c:6977), CmpFrame (each component, then permuted, cmpframe.c:5444), FrameSet and every Region (the current Frame, frameset.c:7398, region.c:6413), and Region `Match` (region.c:6214). The base `MatchAxesX`, FrameSet `Match` and Region `Match` go through `astMatch`'s cast retry, and a leaf class cast into a plain Frame keeps every Frame-class attribute, as `astCastCopy` does. Pinned against C for all 169 pairs of 13 Frames by `tools/oracle_fixtures/match_axes_probe.c` |
| A non-linear Unit does not rewrite the default Label or Symbol | `crates/starlink-ast/src/units/label.rs` | CLOSED 2026-09-30 — `unit_mapper_label` returns the label `astUnitMapper` produces (unit.c:5349): the label parsed by a port of `MakeLabelTree`, joined to the units tree and printed in `MakeExp`'s label style, which drops scale factors, so `"Frequency"` with a Unit of `log(Hz)` becomes `"log( Frequency )"`. SpecFrame, TimeFrame and FluxFrame apply it to their default Label and Symbol when a Unit is set, as C's `GetLabel` and `GetSymbol` do. Pinned by the class attribute oracle, where it removes the space C's label rewrite trims from a TimeFrame label |
| FluxFrame has no UsedUnits and SetUnit does not clear an inconsistent System | `crates/starlink-ast/tests/test_class_attrs.rs` | CLOSED 2026-09-30 — FluxFrame records the Unit used with each System, clears the System for a Unit that does not suit it, re-instates the recorded Unit when the System changes (clearing Label, Symbol and Title, as fluxframe.c:2733 does), and dumps and reads the `U<system>` cards |
| An axis-qualified attribute string on a CmpFrame goes to the primary Frame | — | OPEN — `astGetC( cmp, "Label(2)" )` and `astSet` hand the attribute to the primary Frame (cmpframe.c `GetAttrib`, `SetAttrib`), skipping the CmpFrame's own `GetLabel` and its lent Digits, so the string and function paths differ (a plain Frame component's Label is `"Axis 1"` by string and `"Axis 2"` by function). The `Frame` trait mirrors the functions; the C-ABI attribute dispatch, which does not exist yet, must take the string route |
| A Region's own Ident is not dumped or loaded | `crates/starlink-ast/src/regions/data.rs` | CLOSED — with the Object part of every dump (CH-2), a Region's Ident is written and read back; checked 2026-10-13 by a Native round trip of a Circle with Ident set |
| Unit scale factors and ZoomMap merging | `crates/starlink-ast/tests/test_unit_mapper.rs` | CLOSED 2026-10-05 — `astUnitMapper` factors for 70 unit pairs, SpecFrame unit changes in six Systems, and the simplified series CmpMap of two or three ZoomMaps with every combination of inversions, pinned bit for bit by `tools/oracle_fixtures/unit_mapper_probe.c`. Two divergences fixed: ZoomMap `MapMerge` multiplied by the reciprocal of an inverted neighbour's zoom where C divides by it (zoommap.c:762, :786), and SpecFrame `MakeSpecMapping` returned its chain unsimplified where C simplifies it (specframe.c:2972), so SpecFrame Hz→GHz gave 9.9999999999999986e-10 and VOPT m/s→km/h 3.5999999999999996 where C gives the nearest doubles to 1e-9 and 3.6 |
| C's diagonal MatrixMaps built as full matrices | `crates/starlink-ast/src/mappings/` | CLOSED 2026-10-13 — a MatrixMap C builds in diagonal form (`astMatrixMap(nin, nout, 1, ...)`) becomes a unit one when every element equals 1 within `astEQUAL`, where a full one needs each to be exactly 1. Every Rust counterpart now uses `MatrixMap::diagonal`: the zoom merges (`zoommap.c:944`, `simplify.rs` and `zoom.rs`), the zero-shift WinMap (`winmap.c:1119`), `WinMat` / `MatWin` (`winmap.c:2910`, `matrixmap.c:3153`, which also now uses the supplied MatrixMap as it is rather than rebuilding it), the zero-off-diagonal branch of `matrixmap.c` MapMerge (`:1913`), and `MatZoom` (`:3526`). `MatZoom` had been ported as `astMtrZoom`, the merge a nominated ZoomMap makes; a nominated MatrixMap now forms C's product with a diagonal MatrixMap, and `MtrMult` gained C's unit and diagonal branches. `MtrZoom` divides the inverse by the zoom as C does rather than multiplying by its reciprocal. Eleven `diag_unit_*` and `matzoom_*` rows in the simplify corpus, from values a few parts in 1e11 from 1; five differed from C before. Faithful but not observable in the corpus: `MtrMult`'s branches (equal to the dot product for finite values), `MtrZoom`'s division (within the harness's 4 ULP), the `zoom.rs` and `finalize_linear` diagonals, and `WinMat`'s scale matrix, whose near-unit scales a WinMap loses before the swap |
| `transform` and `transform_into` do not report | `crates/starlink-ast/src/mappings/mod.rs` (`Mapping::report`) | OPEN — found by the C documentation audit (mapping.c): only `tran1` / `tran2` report, where C's `astTranN` and `astTranP` (`transform` and `transform_into` here) report too. The port also uses `transform` and `transform_into` internally where C uses the non-reporting `astTransform`, so reporting from them needs a separate internal entry first (a change to the `Mapping` trait's required method). Exposed by setting Report on a 3-input Mapping and calling `transform`. Its design is to be settled together with the Rust-native tracing. |

Closed (marked historical in-doc, removed as active work): `mapping-gaps-audit.md`
(superseded by `frame-mapping-c-port-gaps.md`), `wcsmap-naxis-refactor.md`
(landed), `timeframe-unformat-findings.md` (resolved 2026-06-25).

## 3. Containers

| Area | Doc | Status |
|---|---|---|
| KeyMap port | `docs/superpowers/specs/2026-08-06-keymap-port-design.md` | DONE — `crates/starlink-ast/src/keymap/`; entries in a `HashMap` with `MapSz` reconstructed at dump time; `testkeymap.c` green; C-ABI shim deferred with the rest of `starlink-ast-capi` |
| Table / FitsTable | `crates/starlink-ast/src/table/` | Table DONE 2026-10-12 — the KeyMap is embedded; the KeyMap methods C overrides are Table methods, the rest are reached through `as_keymap` or forwarded; `tests/test_table_oracle.rs` compares 24 scripts with C, dumps byte for byte; `testtable.c` green. FitsTable DONE 2026-10-13 (`crates/starlink-ast/src/fitstable/`) — the Table is embedded and its `AddColumn` override cannot be bypassed; `tests/test_fitstable_oracle.rs` compares 27 scripts with C, dumps and headers byte for byte, against the library with the `fitstable.c` fixes `369c0e91` and `ca8d3a82`; `testfitstable.c` green. The FitsChan table store DONE 2026-10-13 (`get_tables`, `put_table`, `put_tables`, `remove_tables`, dumped and copied as C does; audit FF-11). `-TAB` read DONE 2026-10-13 (`fitschan/wcs_tab.rs`, C's `TabMapping`, `MakeColumnMap` and `GetNamedTable`, with `set_table_source`); `tests/test_fitschan_tab_read_oracle.rs` compares 26 scripts with C, 23 byte for byte and three named in §6. `-TAB` write DONE 2026-10-13 (`fitschan/fset_to_store/map_tab.rs`, C's `IsMapTab1D`, `IsMapTab2D` and `AdaptLut`, with a positive TabOK as the EXTVER and ForceTab honoured); `tests/test_fitschan_tab_write_oracle.rs` compares 12 FrameSets with C, all byte for byte, against the library with the `fitschan.c` fixes `9e6e4415` and `9efa99f3`. `testtabspectrum`, `checktab` and `checktab2` from `testfitschan.c` are in `tests/test_fitschan_tab.rs` |

## 4. Simplifier

All four simplifier docs are now **historical** (rebuild complete, bugs fixed —
each carries an in-doc note): `simplifier-mapmerge-rebuild-plan.md` (nominate
loop + per-class `map_merge` landed, rigby byte-exact),
`simplifier-winmap-inverted-merge-bug.md` (fixed `a4bff3c`, `spec_cel_invert`
active regression), `simplify-serialization-gaps.md` (0 failures, 719/719),
`sph_fwd_inv_unitradius_cancel_analysis.md` (fixture byte-exact). No active
simplifier work remains here; ongoing simplify parity is tracked by the
regression corpus.

## 5. Serialization / FITS / round-trip

| Area | Doc | Status |
|---|---|---|
| FitsChan C-port gaps | `docs/issues/fitschan-c-port-gaps.md` | OPEN, one gap — C's `WcsToStore` fills the FitsStore card by card (`WcsFcRead`, from the header and then from `SpecTrans`'s translations), so the last of two cards with the same keyword wins and both are consumed; the port fills it keyword by keyword with first-match lookups (`wcs_keywords.rs`), so it takes the first. Exposed by `error-bad-radesys` in `test_fitschan_errors.rs` (found 2026-10-14). Otherwise done — the read path, the attribute surface (every FitsChan attribute, `Card` included) and the public functions (`purge_wcs`, `retain_fits`, `test_fits`, `show_fits`, `read_fits`, `write_fits`, the typed and complex getters and setters, the table functions) are ported, and all 194 wcsconv cases are byte-compared with `NATIVE_READ_EXCLUDED` empty. `-TAB` reading and writing are ported (the §3 Table / FitsTable row). The reader makes a SpecFrame for every spectral axis of an axis description, as C's `WcsSpectral` loop does (2026-10-13; `shared_table` in `tests/test_fitschan_tab_read_oracle.rs`, a header with WAVE and FREQ axes) |
| A FITS-WCS write of a spectral FrameSet with no celestial axes does not add them from RefRA and RefDec | `crates/starlink-ast/src/fitschan/fset_to_store/mod.rs` (`make_fits_frameset`) | OPEN — found in the Task 8 review: C's `MakeFitsFrameSet` (fitschan.c, the `!skyfrm` branch after the RefRA/RefDec test) gives a FrameSet whose current Frame has a SpecFrame with RefRA and RefDec set but no SkyFrame two extra celestial axes, read from a small TAN header through an internal `astFitsChan`; the port returns without a SkyFrame, so the write describes no celestial axes. Its port must make that FitsChan with `FitsChan::new()`, so that `FITSCHAN_OPTIONS` reaches it as in C (with `Encoding=NATIVE` C's internal read returns NULL and nothing is added). Exposed by a FITS-WCS write of a 1-D or 2-D spectral FrameSet with RefRA and RefDec set, compared with C |
| `fset_to_store` port gaps | `docs/issues/fset_to_store-port-gaps.md` | MOSTLY DONE — SIPIntWorld + VELOSYS + Group 3/5 branches ported. `FitsTol` and `SipOK` were plumbed-but-unreachable (fixed at `FITS_TOL_DEFAULT` and hardcoded `true`); both became reachable in the 9.5.0 sync — `FitsTol` has `set_fits_tol`/`fits_tol` accessors and `SipOK` is derived from the encoding at `fset_to_store/mod.rs:260`. `-TAB` (IsMapTab1D/2D, AdaptLut, ForceTab, and the forward-only branch of `WorldAxes`) is ported, recorded in the §3 Table / FitsTable row |
| `sparse.ast` written as `-TAB` differed from C | `crates/starlink-ast/src/fitschan/`, `crates/starlink-ast/src/frames/frameset.rs` | CLOSED 2026-10-13 — the case now matches C byte for byte. Four causes, each fixed with its own gate: `LutMap::new` kept infinite entries, which C stores as bad (`tests/test_lutmap.rs`); the reader did not label unclaimed axes with their CTYPE comments (`tests/test_fitschan_labels_oracle.rs`); the reader built the pixel to WCS chain in another shape than C's `WcsMapFrm`, with a full rather than a diagonal CDELT matrix (`tests/test_fitschan_read_chain_oracle.rs`, `fitschan_alt_diagonal_cdelt` in the simplify corpus); and `FrameSet::with_integrity` remapped the current Frame even when the change was a UnitMap, because it tested the box holding the Mapping rather than the Mapping, so every integrity change re-simplified the FrameSet's Mappings and a write's CRVAL3 came from a Mapping C never forms (`unit_change_leaves_mappings_alone` in `tests/test_frameset_integrity.rs`) |
| The FITS-WCS writer put `SREFIS`, `SREF<i>` and `SREFP<i>` after `OBSGEO`, where C puts them right after `AXREF<i>` | `crates/starlink-ast/src/fitschan/wcs_write/mod.rs`, `fset_to_store/` | CLOSED 2026-10-13 — the cards now follow `AXREF`, as `WcsFcWrite` writes them. The case written to expose the order showed the cards were not written at all: the absolute description of an offset SkyFrame took its `isoff` after SkyRefIs had been cleared, so it was 0 where C's `AddVersion` passes -1, -2 or -3 for POLE, ORIGIN or neither. `SkySys` now also stores SkyRef and SkyRefP only when set, by external axis, and writes C's CTYPE comment for offset coordinates, with SkyRef formatted as `astGetC( skyfrm, "SkyRef" )` gives it. `tan_offset_origin` and `tan_offset_pole` in `tests/test_fitschan_encodings_oracle.rs` |
| The rotation FrameSet integrity makes for an offset SkyFrame differed from C's in its last digits | `crates/starlink-ast/src/mappings/` | CLOSED 2026-10-13 — traced: the FrameSet before the change and the conversion Mapping were C's to the last digit, and both simplifiers took the same path through the composed Mapping, merging the same two MatrixMaps. The port formed their product with ndarray's matrix product, which sums in another order than C's `MtrMult` loop. Every product C forms through `astMtrMult` (MatMat, MatPerm, MatZoom and the PermMap merges) now goes through `matrix::mtr_product`, C's loop, or `MatrixMap::mtr_mult`. `offset_skyref_integrity` in the simplify corpus; `tan_offset_origin` and `tan_offset_pole` in `tests/test_fitschan_encodings_oracle.rs` now match C byte for byte |
| Round-trip harness misses trailing-whitespace divergences and the corpus has no comment-less nested-object card | `crates/starlink-ast/tests/test_serialisation.rs` | CLOSED 2026-09-04 — `native_round_trip_fixture` compared whole files with `trim()` and lines with `split_whitespace()`, so `"Frm1 = "` and `"Frm1 ="` compared equal. The whole-file path is now exact and the line walk splits on whitespace *keeping the separators*, so a difference in them is a difference; the 4-ULP tolerance covers numeric values and nothing else. The whole shared corpus passes under it, so the port's dumps are byte-exact including whitespace. The second half of the row is answered by the first: the comparator now normalises nothing, so there is nothing for a missing fixture to hide (the two files in the tree that do carry a `" ="` line, `programs/testyamlchan/lsst_wcs.txt` and `programs/joye_car_headers/total_hi.head`, are not native dumps and are read by their own programs) |
| `WcsFromStore` bounds its per-axis loops by a per-version axis count the port does not compute | `crates/starlink-ast/src/fitschan/wcs_write/mod.rs` | CLOSED 2026-10-12, no divergence — C bounds each version's loops by its WCSAXES, or else by the highest CRPIX index stored for it (`fitschan.c:37523-37528`); the port builds a store per version, sized as the larger of the pixel and WCS axis counts. These agree because `MakeIntWorld` stores a CRPIX for every pixel axis and a WCSAXES whenever there are more WCS axes than pixel axes. `tests/test_fitschan_alt_naxes_oracle.rs` writes primaries and alternates with fewer, the same and more axes than the pixel Frame against C: C leaves out an alternate with fewer, writes `WCSAXESA` for one with more, and refuses a primary with fewer, and the port matches |
| Channels as objects: what remains after `NativeChannel` and `MocChan` | `crates/starlink-ast/src/channel/` | CLOSED 2026-10-12 — CH-1 to CH-4 in `docs/audits/fitschan-c-parity.md`: FitsChan is an `AnyObject` and dumps and loads as C's does, every class has the Object ID, a NativeChannel reads one Object per read and honours Skip and Strict, and with Full above zero every class writes C's commented unset attributes |
| The FITS-AIPS, PC and IRAF readers need `WCSAXES`, `NAXIS` or an `NAXISn` card to count the axes | `crates/starlink-ast/src/fitschan/wcs_mark.rs` | DONE 2026-10-12 — every foreign encoding but DSS is now read as C reads it: `SpecTrans`, then one extraction (`wcs_keywords::extract_wcs_keywords_with_alts`) and one decoder. The pixel axes are counted from the highest CRPIX index of the primary or any alternate axis description (`wcs_mark::wcs_versions`, C `FitsToStore`), each description has WCSAXES WCS axes or as many as there are pixel axes, and a PermMap joins a description with a different number to the pixel Frame (C `AddFrame`). The FITS-AIPS and FITS-CLASS readers of their own are removed, and `test_trangrid.rs` reads its header without the NAXIS card it had added. `tests/test_fitschan_axis_count_oracle.rs` compares twelve headers with C |
| Comment-less nested-object card wrote a trailing space C never writes | — | FIXED (`1d2aa8f`, `86810ce`) — C's `WriteObject` (`channel.c:5106-5112`) appends `" ="` and adds the separator only when the comment is non-empty; the Rust Object arm pushed `" = "` unconditionally |

### Deferred C-oracle simplify fixtures

None remain.
The last fixtures that held an unported class, `cap_timemap_02.map` and `.simp`
(`StcResourceProfile`), load since 2026-10-09 and are gated like any other.
`crates/starlink-ast/tests/test_deferred_fixtures.rs` keeps `UNPORTED_CLASSES`, now
empty, and fails if a fixture names a class the reader cannot load.

Closed (marked historical in-doc, removed as active work):
`native-roundtrip-gaps.md` (24/24 + 139/139 byte-exact),
`native-roundtrip-simplify-gaps.md` (719/719 byte-exact),
`wcsconv-residual-classification.md` (wcsconv 192/192, all buckets cleared).

## 6. Cross-cutting

| Area | Doc | Status |
|---|---|---|
| C-library quirks mirrored in the port | `docs/issues/c-library-quirks.md` | running list — discuss with upstream |
| Whether `XmlChan` / `YamlChan` / `StcsChan` carry a `KeyMap` or a `Node` | `docs/superpowers/specs/2026-08-06-keymap-port-design.md` (Deferred decisions) | OPEN — decide per subclass when the first one is ported; `yamlchan.c` relies on `SortBy` ordering as algorithm state (`:9814`, `:11391`, `:11531`, `:12191`), which `Node` cannot express |
| Simplify builds a `TagTree` on every reduction that changes something | `crates/starlink-ast/src/mappings/simplify.rs` (`stamped_tags`) | OPEN — performance only, results are unaffected. The record of the `IsSimple` stamps written on a CmpMap's components is built whenever a reduction returns a new Mapping, in case an outer reduction then fails and C's stamps must be transplanted onto the Mapping returned unsimplified. It is cheaper than the copies it replaced, but most reductions never need it; building it only on the failure path would need the stamps to be recoverable after the fact. Exposed if `ast-bench compare` shows simplify-heavy sets (FitsChan reads, `convert_to`) slower than before it landed. |
| C line numbers in `//` comments are not re-anchored | `crates/audit-tools` (`reanchor-citations`) | OPEN — about 2,300 `file.c:NNN` citations in `crates/starlink-ast/src` comments point at the C source as it was when each was written, and many have drifted since. The re-anchor hook handles only the `docs/audits/mapmerge/*.md` table rows. It needs a mode that reads `// C: file.c:N` citations from Rust sources, takes each baseline from the blamed commit's submodule pin (fetched into the submodule), relocates the line through the C diff, and validates against the C function the comment names, reporting what it cannot validate. Exposed by any comment whose cited line no longer holds the code it describes. |
| Tiers 3, 4 and 5 are not run by CI | `.github/workflows/ci.yml` | OPEN — the design (step 0) calls for a nightly job running the submodule-dependent tiers. It does not exist: the workflow has a `fixture-mirror` job and a `test` job running tier-1 / tier-2 plus clippy, fmt and docs, and nothing on a schedule. So `ast-ffi-tests`, `ast-oracle-tests` and `ast-bench` are run by hand, which is also why the hot-path constraint has no gate (CLAUDE.md says so directly). Until a scheduled job exists, a tier-3/4 regression is caught only when someone runs it |

### Verification-posture ledger

Every way a gate in `starlink-ast` can be relaxed, counted so the total cannot grow
silently. **This table is mirrored by `crates/starlink-ast/tests/test_verification_ledger.rs`,
which is the executable copy**: it asserts each count and fails on any
`const` whose name contains `EXCLUDED`, `LICENSED`, `SKIPPED`, `DEFERRED`,
`UNGATED`, `DISAGREEMENTS`, `UNTRANSFORMABLE` or another bypass word, in this
crate's tests or the transform tests of `ast-oracle-tests` and `ast-bench`,
that is not in its table. Adding a relaxation therefore means
writing its reason down in two places, and finding one by accident is no longer
possible.

The last two suffixes were added at the simplify parity closeout, and with them
three lists that had been documented in §10 but never counted here: the six
byte-comparison divergences in `SERIAL_UNGATED` and the seven-fixture
`ISSIMP_DISAGREEMENTS` set (both since reduced).
Each was already asserted as an exact set in its own harness, so none could
grow unnoticed; what was missing was their appearance in the total. The counter
gained the ability to count them at the same time: it read a flat `&[&str]`
list as empty, so a ledger row for one would have passed at any size.

| Gate | Cases | Relaxed | What the relaxation is |
|---|---:|---:|---|
| wcsconv (`test_wcsconv.rs`) | 194 | 12 | `astequal` compare mode — FMA contraction / libm differences vs x86-64 Linux captures that C AST on arm64 does not reproduce either |
| wcsconv | 194 | 2 | `ISSIMP_PLACEMENT_LICENSED` — drops the `IsSimp` tag lines both sides *and pins the card count on each side*, so it covers the tag placement and nothing else. A deliberate divergence, not a gap: C's placement here records the order its reader simplified in (§8). `vopt-f2w`, `wave-f2w`; C emits more cards than the port in each. `axes-swap` retired on 2026-10-13, when the reader began building the pixel to WCS chain in the shape C's `WcsMapFrm` builds it (`build_pixel_to_world`): its four cards now sit where C's do. 24 rows preceded these; 20 were pinned against references the pinned C no longer reproduced and were retired with the corpus refresh (§9.3), and `cd-cross` retired when the nominated-component branch stopped discarding the stamps its own simplify raised (§10.3) — its four cards now sit exactly where C's do, and `axes-swap`'s pin rose from two to three the same way |
| wcsconv | 194 | 0 | **No skip mode exists.** `NATIVE_READ_EXCLUDED` is empty and gated against refilling, and a case whose input fixture is missing fails |
| wcsconv, simplify regression | 194, 517 | 0 | **What "byte-exact" means** (`tests/common/mod.rs`, pinned by `test_dump_comparator.rs`): every line byte for byte, blank FITS cards included (wcsconv drops only a final `END`), nothing trimmed; a number may be written differently only when its value differs within 4 ULP (or `astEQUAL` in the `astequal` rows), and whitespace may differ only to absorb that number's change of width in a fixed-width card. The same value written another way (`1`, `1.0`) fails |
| simplify regression | 517 | 156 | `skip_string_compare` — upstream's own manifest column, now shared with its harness. 154 of these rows are negative fixtures, whose reference is the unsimplified input and so can never carry the `IsSimp` card `astSimplify` stamps; upstream gives up the whole dump for them and gates them in a separate `_noop` ctest family, while this harness compares them inline with `IsSimp` stripped (the `ignore_is_simp_line` row below). The two rows with a distinct reference, `cap_interval_01` and `cap_pointlist_01`, are platform-numeric cases upstream cannot license per card; this harness keeps comparing them because `NUMERIC_PLATFORM_DRIFT` licenses the one drifting value each. The port's own column was empty and gated before the manifests merged, and no row here is a port opt-out
| simplify regression | 517 | 4 | `NUMERIC_PLATFORM_DRIFT` — `cap_interval_01`'s `X5` / `X6`, the two sky half-widths of its uncertainty Box, `2.4240684055495722e-06` and `2.4240684055430873e-06` in the reference against `2.4240684055476816e-06` from both the port and the pinned C here; and `cap_pointlist_01`'s `X5` / `X6`, the sky half of its merged uncertainty Box's corner, `3.1241551949160478` and `-0.20069734795308758` in the reference against `3.1241551949160575` and `-0.20069734795284888` here. Numeric drift, not a port gap: the references were captured on arm64, and the corpus-freshness rule keeps such a reference rather than replacing it with an x86-64 recapture. `cap_interval_01`'s two agree with the reference to twelve significant figures and are nominally equal to each other, which this machine's C makes them and that platform's did not; `cap_pointlist_01`'s two are what `astSetUnc` regenerates by re-centring the Box and re-offsetting its corner through a `CmpFrame(TimeFrame, SkyFrame)`, so they carry that round trip's `atan2` / `asin` pair. Each licence names the fixture, the card and the reference's value exactly, and bounds how far the port's own value may sit from it: the reference side is fixed bytes and is matched as written, while the port's side is matched by distance, so the licence fires on an arm64 build — where the port reproduces the reference exactly — as well as here. Exactly one line must drop from each side, which is what stops `cap_interval_01`'s exact-zero `X5` going with it. `rigby`'s `IM5` was the fifth entry and is retired: the `astEQUAL`-zero snap upstream added to `astMapMerge` (§9.4) removed the cancellation residue, and the regenerated reference now matches the port exactly |
| LSST oracle (`test_lsst_oracle.rs`) | 4 | 2 | `OFF_PLATFORM_DRIFT_LICENSED` — the inverse transforms, as read and simplified, off the capture platform only, within 4.5e-10 pixel, the most measured on Linux x86-64 (CI, 2026-10-28). PolyMap's iterative inverse stops once a step is below the Mapping's TolInv of 1e-7 pixel, so another libm's last-bit differences in the sines, cosines and arctangents before it move where it stops. On macOS every comparison is exact |
| simplify regression | 517 | 5 | `ABANDONED_PROBE_STAMP_LICENSED` — one reference-only `IsSimp = 1` line each in `rigby`, `cap_specmap_02`, `cap_cmpmap_01`, `cap_cmpmap_04` and `fitschan_tab_spectral`, the last a FitsChan read of a `-TAB` spectral axis whose stamp was traced in C to a discarded series-in-parallel probe. `cap_cmpmap_nested_tag` was the fifth and is retired: the port reproduces its dump byte for byte at the current pin. C stamped a sub-Mapping inside a merge probe it then abandoned: `CombineMaps` assembles the trial CmpMaps by cloning (`cmpmap.c:626-627`), so a trial holds the very sub-Mappings the live tree holds, the recursion stamps whatever `astSimplify` returns (`cmpmap.c:1443`), and discarding the probe (`cmpmap.c:1621-1628`) leaves the mark behind. The neighbouring parallel-in-series probe guards against exactly this with `SeparateMappings` (`cmpmap.c:1650-1651`); this one does not. Owned values give each tree its own children, so no probe in the port can leave a mark that outlives it (`docs/issues/c-library-quirks.md`, "An `IsSimp` card can record a merge probe C threw away"). Spent positionally — a line counts only where the port's line is the reference's *next* line — one per fixture, required to be found, with every other line compared |
| simplify regression | 517 | 2 | `INVAB_ALIASING_LICENSED` — one node in each of `cap_cmpmap_01` and `cap_cmpmap_04` whose dump carries `Invert = 1` in C and `IsSimp = 1` in the port. C's `CombineMaps` records a shared child's `Invert` on the CmpMap it builds, so the pair the dump carries is reference-count history rather than a property of the Mapping (`docs/issues/c-library-quirks.md`, "CmpMap dumps InvA/InvB disagreeing with the child's own Invert"); the port holds components by value and has no such history. The two cards are consumed as a pair at one position, so neither can go missing elsewhere unnoticed. This is what retired both fixtures' `skip_string_compare`, which had been hiding the whole 350-line file. Preceded by `ISSIMP_PLACEMENT_LICENSED`, which read `cap_specmap_02`'s missing card as a CmpMap the port never forms (§10.3): the port forms it byte for byte, and the card was the abandoned probe's |
| simplify regression | 517 | 2 | `SHARED_INVERT_HISTORY_LICENSED` — two MathMaps in `cap_cmpmap_02` whose dump carries `Invert = 1` in C and no card in the port. C's `CombineMaps` sets a shared component's `Invert` while it builds a CmpMap and restores it afterwards (`cmpmap.c:625-662`), so which value the result dumps depends on which objects the surrounding simplification kept and which it copied; the port records the applied direction in `InvA`/`InvB` alone. Each card is consumed at one position and must be spent. Introduced 2026-10-14 with the CmpMap split and the Frozen flag on the Mapping, which brought the merge order to C's for this fixture |
| simplify regression | 517 | 3 | `ISSIMP_DISAGREEMENTS` — fixtures whose `IsSimp` card count still differs from C. Each under-tags by exactly one card, and that card is one C wrote during a merge probe it then abandoned: the probe's CmpMaps are assembled by cloning, so they share their sub-Mappings with the live tree and the stamp outlives the probe (`docs/issues/c-library-quirks.md`, "An `IsSimp` card can record a merge probe C threw away"). Owned values give each tree its own children, so the port cannot reproduce it — `rigby`, `cap_specmap_02` and `fitschan_tab_spectral`. Retired at the current pin: `cap_cmpmap_nested_tag`, shrunk from `cap_specmap_02` to isolate the signature, whose dump the port now reproduces exactly. Retired with this cluster: `cap_zoommap_04`, which over-tagged by three because its uncertainty simplification was being discarded (§10.23); `lsst`, when `PolyMap`'s linear branch began composing its MatrixMap and ShiftMap and handing the pair to `astSimplify` as C does, and `cap_cmpmap_01` / `cap_cmpmap_04`, when `CmpMap::map_merge` began adopting the object its own self-simplify returns unchanged — C's `new == map`, which carries the recursion's descendant stamps forward (§10.3). Retired earlier: `cefit_circle_conformal` and `cefit_circle_nonconformal_keep` when `from_node` began routing through the setters that discard the record, as C's loaders do (§9.2); the three `cmpmap_perm_swap*` fixtures when the swap branch stopped simplifying the components C hands on as they stand; `cap_cmpmap_03` when the same branch stopped simplifying the parallel CmpMap it reassembles; and `parpair_one_side_reduces` when it stopped simplifying a dimension-matched sublist's members (all §10.3). The byte comparison licenses all three through `ABANDONED_PROBE_STAMP_LICENSED`, so the count is their only gate. Listed because otherwise the number is invisible, and asserted as a set in `test_simplify_regression.rs` so it cannot grow. A stale reference is ruled out per fixture by `tools/oracle_fixtures/check_simplify_fresh.sh`, which recaptures the corpus from the pinned C |
| simplify regression | 517 | 158 | `ignore_is_simp_line` for the negative fixtures, whose reference *is* their input and so cannot carry the tag `simplify` always stamps. The companion assertion requires a top-level card exactly when the result is not Frame-kind — C's `IsSimple` getter is overridden on the Frame class to return zero (`frame.c:5590`) — so the strip can hide neither an untagged Mapping result nor a wrongly tagged Region. 134 with `neg_time_mjdtojd_real` (§10.9), `neg_unit_parallel_forward` (§10.6) `neg_parpair_aligned_irreducible` (§10.14), `neg_lut_near_linear` (§10.8), and five upstream repaired at pin `8d4f526f`. Was 141 before `cap_box_permmap_nonbidi`, the negative control for a PermMap whose permutation is not bi-directional (§10.13), and 142 before `neg_wcsmap_perm_allconst` and `neg_wcsmap_perm_both_sides`, the two controls on C's WcsMap swap decision (§10.31) |
| simplify regression | 517 | 0 | `SERIAL_UNGATED` — empty. Every fixture's simplified dump is byte-compared against C's `.simp` in full, and any difference fails. The table survives so a newly diagnosed C-side artefact has a home, and the harness asserts the set exactly, so a row cannot appear without a diagnosis. All seven causes enumerated in the table below are closed: A (SwitchMap `IsSimp` on FSMap / ISMap), B (a card on a Mapping inside a Region's FrameSet, §9.2), C (`nullregion_unc_fillfactor_merge`'s `Prism` uncertainty), D (`cap_zoommap_04`'s `FrmSet` block, §10.23), E (`cap_interval_01`'s corner point, §10.25), F (`cap_pointlist_01`'s uncertainty corner, §10.26) and G (`parpair_one_side_reduces`'s extra `IsSimp` cards, §10.3) |
| simplify round-trip (`test_serialisation.rs`) | 823 | 0 | **No relaxation exists.** Every `.map` / `.simp` in the corpus is read, written and byte-compared against itself, and any difference fails. The 43 `.rt` references are gone with the defect that needed them: C's subclass loaders re-applied a validated attribute through a clear-on-set setter, discarding the `IsSimp` card `astLoadMapping` had just installed, so reading a dump was not the identity. Upstream `07d0cd2d` guards each clear on the value actually differing and the port mirrors it (§9.4), so a fixture is now its own round-trip reference. In place of `.rt` the comparison drops the leading run of comment lines from both sides, as upstream's driver does: a hand-written header saying what a fixture is for is documentation, not part of the object. Was 4 under `ROUNDTRIP_DISCARD_LICENSED`, two GrismMap and two PcdMap fixtures whose reference dropped a card the port kept
| transform oracle | 2261 sections | 0 | all three `*_ORACLE_EXCLUDED` lists are empty |
| transform oracle | 2261 sections | 3 | `transform_oracle_overrides.txt` — two round-trip skips and one golden row list, asserted as an allowlist by `parses_committed_overrides_file` rather than by an emptiness check, so a fourth cannot appear unnoticed. Both round-trip entries are `tnx-cheb`, reached once as the header and once as its dump under `wcsconv/framesets/`: its TNX Chebyshev distortion builds a ChebyMap, which cannot represent its own Jacobian, so sky->pixel is BAD throughout and there is no round trip to assert. The golden entry names rows 2 and 3 of one `neg_unitnormmap_inv_fwd_diffcentre` inverse section — both the all-zero sample, where a zero norm leaves the forward direction unconstrained and the reconstruction's value depends on whether the compiler contracts its multiply-add. Upstream narrowed this from `golden-inv off` to the two rows at the 9.5.0 sync, so the rest of that section is now pinned; the row-list form is preferred over a whole-direction `off` for exactly that reason. A loosened round-trip *tolerance* remains impossible: a test requires every round-trip entry to be a disabled one |
| transform oracle | 51 rows | 5 | `RTRIP_REFERENCE_GAPS_LICENSED` — rows where C's own recorded inverse does not recover the pixel either. Rekeyed to full fixture paths when the corpus moved to the shared tree, which is what raised the count from two entries to five: the same cases, addressed by the path the oracle records rather than by basename. Asserted as an exact set against the corpus with no fixture loaded, and reached only after the live result has failed both the direct and the aliased comparison |
| transform oracle | 2 rows | 0 | `FMA_SUBSTITUTIONS_LICENSED` — empty. It held two rows of one UnitNormMap inverse section where the generator host's compiler fused the multiply-add at `unitnormmap.c:1079` and stepped past C's own zero-norm branch. The regenerated oracle records the unfused values, and the port reproduces them because it builds without contraction; the table survives because that is a build setting, and a contracting Rust build would need it back (§9.4). Upstream reached the opposite conclusion about which value is right — the fused one is the correctly-rounded evaluation, and the true displacement is 2.58e-16 on both axes rather than zero — so it skips those two rows outright rather than pinning either. The port keeps them skipped via the overrides row above; substituting the value C's source prescribes would pin the answer upstream declined to pin
| frameset round-trip | 139 | 0 | `FRAMESET_FIXTURE_EXCLUDED` is empty — every wcsconv object round-trips byte-exact |
| deferred fixtures | — | 0 | `UNPORTED_CLASSES` — empty: every class a fixture names has a loader. Its one row was `StcResourceProfile`, which landed on 2026-10-09. The fixtures used to sit in per-class subdirectories both consumers skipped by not recursing; the tree is now shared with upstream, whose harness reads one flat directory, so each consumer names them instead |
| transform oracle | 2261 sections | 1 | `DEFERRED_LOAD` — a fixture the reader cannot build, `serialisation/frame_in_map_slot.ast`, a Frame in a Mapping slot (§9.6), so the oracle's sections for it cannot be checked. Not a tolerance. `deferred_fixtures_still_fail_to_load` fails when one starts loading, so a class landing cannot leave its coverage unclaimed |
| transform oracle | 22 sections | 11 | `UNTRANSFORMABLE` — fixtures that load as a bare Frame, which has no transform in the port. Each records whether C's outputs equal its inputs per direction, and the gates check that against the oracle; all are identities except `region_in_cmpframe.ast` forward, C masking points through its Box (§9.6). The gates compare exact section counts and assert how many round-trip and equivalence checks run |
| FitsChan cursor (`test_fitschan_cursor_oracle.rs`) | — | 5 | `EXCLUDED` — four NATIVE reads the port parses whole where C reads card by card, and `native_full_1`'s RefCnt and Nobj comment cards (the rest compared) |
| object IDs (`test_object_id_oracle.rs`) | — | 6 | `EXCLUDED` — C copies or shares an Object where the port moves or clones it |
| Channel reads (`test_channel_read_oracle.rs`) | — | 2 | `EXCLUDED` — C reports AST__BADIN for a string read as a number; the port reads it |
| fixture read warnings / Full = 1 dumps | — | 1 + 1 | `EXCLUDED` — `frame_in_map_slot.ast`, refused by design |
| SipOK and FITS encodings writes | — | 0 | `EXCLUDED` — both empty |
| simplify regression | 517 | 0 | `DEFERRED` — empty: every manifest fixture loads |
| call-count parity (`ast-oracle-tests`, tier 4) | — | 53 | `COUNT_PARITY_EXCLUDED` — sections whose libm/PAL/ERFA call counts differ because C does work the port does not need; never the port skipping work |
| simplify regression | 517 | 0 | `C_DEFECT_EXCLUDED` — empty. It held `cheby_linear_lone_reduce`, whose reference recorded a C defect rather than C's intent: PolyMap's linear `MapMerge` reduction read a ChebyMap's `coeff_f` / `power_f` without the scale and offset that map its input onto [-1,1], so `astSimplify` returned a `ZoomMap` of 2 where the ChebyMap computes `0.4x-2`, and reproducing the reference would have meant porting the defect. Upstream fixed it in `abe5baae`, the port ported the fix (§10.2), and the row is compared byte for byte like any other. `c_defect_rows_still_diverge` is what reported the fix: it fails when an excluded row starts matching, so the row could not stay excluded unnoticed. The table survives empty so a newly diagnosed C-side artefact has a home |
| simplify regression | 517 | 2 | `astequal` compare mode — upstream's own manifest column: two rows. `cap_timemap_02`, whose uncertainty coordinates move by about 7e-12 relative under FMA contraction, and `offset_skyref_integrity` (2026-10-14), whose MatrixMap is `MtrMult`'s product of two rotations with off-diagonal elements near 1e-3 that nearly cancel, so a fused dot product moves them by some 50 ULP; it failed upstream's macOS arm64 CI and passed on a non-contracting build. Each dump is compared within `astEQUAL` rather than byte for byte. The comparator honours the column so the two harnesses cannot diverge on what a row asks for |
| round-trip, serialisation + programs (`test_serialisation.rs`) | 34 | 3 | `ROUND_TRIP_EXCLUDED` — one refusal by decision: `serialisation/frame_in_map_slot.ast`, a Frame where a Mapping belongs, which the Mapping loader declines (§9.6); and two dumps that are not the identity by design, `programs/testmocchan/backcompat_string.ast` and `backcompat_json.ast`, written by C master at 3dc9d49a with the `# Format` comment the current writer replaces with the explanation of the reversed MocEnc name (added 2026-10-22). Upstream keeps those two out of its byte-for-byte roundtrip family for the same reason, and `test_mocchan.rs` reads both with their original MocFormat. `programs/testfitschan/sparse.ast` left the list on 2026-09-30, once SkyAxis Format specifiers were parsed (phase 3 of the Frame attribute plan) and CmpFrame wrote the Frame part of its dump through its own getters. `round_trip_exclusions_still_fail` fails when an entry starts passing. This sweep replaced three skip modes: a missing fixture directory returned early with a notice, a fixture that would not load was passed over on a substring match against the error text, and the failure list itself was filtered by substring, so every Frame, FrameSet, `lsst`, `degen` and `splittest` fixture could fail silently |
| sky conversion oracle (`test_sky_convert.rs`) | 1584 | 1 | `SINCOS_MERGE_LICENSED` — one AZEL case allowed a measured 1 unit in the last place (the two AZEL-to-AZEL cases aligned in FK4 left the list on 2026-10-28, when ERFA's FK5-to-FK4 conversion moved their input to values both merged and separate `sin`/`cos` round alike): Apple clang merges the `sin`/`cos` pairs of slamap.c `De2h` into `__sincos_stret`, which an unoptimised Rust build does not. A release build matches C exactly, and the companion test asserts that in both build modes |
| map split (`test_map_split_oracle.rs`) | 66 | 2 | `ISSIMP_COMPONENT_EXCLUDED` — `ratemap_full_permuted` and `tranmap_full_permuted`: a full permuted selection splits off the simplified CmpMap of a PermMap and the Mapping, and C dumps it with one `IsSimp = 1` card more, on a component (the RateMap's ZoomMap, one of the TranMap's ZoomMaps). Compared with `IsSimp` lines set aside, every other line byte for byte. The signature matches `ISSIMP_DISAGREEMENTS`; why C stamps the component here is not established. `issimp_component_exclusions_still_differ` fails when an entry starts matching |
| `-TAB` read (`test_fitschan_tab_read_oracle.rs`) | 26 | 0 | `TAB_READ_EXCLUDED` — empty: `shared_table`, two spectral axes in one axis description, matches C since the reader makes a SpecFrame for each, as C's `WcsSpectral` does. `excluded_cases_still_differ` fails when an entry matches |
| testregions.c functions (`test_regions_c.rs`) | — | 1 | `REGIONS_C_GAP_EXCLUDED` — `checkEllipse` Errors 15-17: an Ellipse with a Box uncertainty mapped through a full MatrixMap and back. C's mapped Ellipse carries an unrotated Box uncertainty and maps back to an equal Ellipse; the port's carries the rotation, and mapping back fails when that uncertainty simplifies to a Polygon, which cannot be re-centred. The lead is that C's Ellipse `Simplify` makes the new Ellipse with `astEllipse`, whose `SetUnc` maps the uncertainty into the new base Frame, where the port's `best_ellipse` stores it directly. `region_gaps_still_differ` fails when it closes |
| testfitschanerrors.c bad headers (`test_fitschan_errors.rs`) | 34 | 4 | `FITSCHAN_ERRORS_EXCLUDED` (3) — cases whose check is C's string attribute interface (an unknown Encoding or AltAxes name, setting Ncard), which typed accessors cannot express. `FITSCHAN_ERRORS_GAP_EXCLUDED` (1) — `error-bad-radesys`, the duplicate-keyword gap in the FitsChan row of §5; `port_gap_cases_still_differ` fails when it closes |
| `-TAB` read (`test_fitschan_tab_read_oracle.rs`) | 26 | 1 | `PROBE_STAMP_LICENSED` — `mixed`, one `IsSimp = 1` card C writes on the celestial rotation's CmpMap during a merge probe it discards, the same Mapping as the simplify corpus's `fitschan_tab_spectral` (`ABANDONED_PROBE_STAMP_LICENSED`). Removed from C's transcript at its line, which must hold that card, and the raw transcripts must still differ |
| C-oracle tests off macOS (26 files: `crates/starlink-ast/tests/common/mod.rs`, `c_value_eq`, `c_text_eq`) | 26 files | 0 on macOS | The C values in these tests were captured on macOS, where the port matches them bit for bit, and they stay exact there. On any other platform libm rounds some last bits differently, so values compare within `LIBM_RTOL` = 1e-13 relative plus `LIBM_ATOL` = 1e-14 (the Linux CI run of 2026-10-15 needed at most 3.6e-14 and 3.8e-15), text dumps token by token the same way, and the four tests that hash every output value (`tran_grid`, `resample`, `rebin`, `rebinseq`) compare their recorded samples only, as a hash cannot be compared within a tolerance. `AST_ORACLE_TOLERANT=1` runs the off-macOS comparisons on macOS |
| STC dumps off macOS (`test_region_stc_oracle.rs`) | 3 | 3 | `ERROR_ELLIPSE_LICENSED` — `sky`, `catalog_entry` and `sky_coord` hold the sky AstroCoords Error Region, a 1e-4 rad Galactic Circle the Stc re-expresses in ICRS as the Ellipse best fitting its mesh. The shape is all but circular, so the fitted axis direction is ill-conditioned and last-bit libm differences move the defining points by about 1e-5 relative (Linux CI, 2026-10-15); C would move them as much. Off macOS only, those PointSet values are left out and the rest of each dump is compared; on macOS everything is compared exactly. `error_ellipse_licence_is_still_needed` fails off macOS if the points come to match |
| ast-bench `compare` (`crates/ast-bench/src/compare.rs`) | all | 1 | `SLOWDOWN_LICENSED` — `specmap_wn_to_fr.map` at 25%. A timing is a slowdown when its fastest sample is more than `SLOWDOWN_THRESHOLD` (10%) slower than the saved baseline's; a case allowed more is named there with its reason. SpecMap's wave-number conversion is a loop of about 1 ns per point whose speed follows the code layout: on Linux x86-64 the same vendored commit measured 1.04 or 1.23 ns per point forward at N=1 000 000 depending only on the directory it was built in, and builds of commits that do not touch SpecMap sat at one speed or the other; the most a `compare` reported was 22% (2026-10-28) |
| `-TAB` read (`test_fitschan_tab_read_oracle.rs`) | 26 | 1 | `ULP_LICENSED` — `celestial_iwc`, one WinMap shift 1 ULP from C's (`Sft2 = 59.999999999999993` against `60`) in the WCS to IWC Mapping, which the port's simplifier folds in another order; the same Mapping is `fitschan_tab_celestial_iwc` in the simplify corpus, which passes within that harness's 4 ULP token bound. Only the named line may differ, by at most 1 ULP |
| `-TAB` write (`test_fitschan_tab_write_oracle.rs`) | 12 | 0 | `TAB_WRITE_EXCLUDED` — empty: `sparse`, its one entry, matches C since the fixes recorded in the §5 `sparse.ast` row. The table survives so a newly diagnosed difference has a home, and `excluded_cases_still_differ` fails when an entry matches |
| FITS encodings (`test_fitschan_encodings_oracle.rs`) | 2 | 0 | `VALUES_LICENSED` — empty: `tan_offset_origin` and `tan_offset_pole` match C since MatrixMap products are formed in C's order (the §5 row). The table survives so a newly diagnosed difference has a home, and `licensed_values_still_differ` fails when an entry matches |
| hand-written Frame descriptions (`test_frame_load.rs`) | 17 | 0 | `FRAME_LOAD_EXCLUDED` — empty: a SkyFrame with no `Ax` objects now holds the plain Axis objects C gives it. `excluded_descriptions_still_differ` fails if an entry is added that matches |
| Frame in a Mapping slot (`test_frame_in_map_slot.rs`) | 1 | 1 | `FRAME_IN_MAP_SLOT_DECLINED` — one C-written dump the reader refuses, a bare Frame in a `Map<n>` slot, declined by decision on 2026-10-13; question, decision and artifact in `docs/issues/frame-in-a-mapping-slot.md` (§9.6). The exclusion carries the reason string, and two companion tests keep the refusal narrow: the settled Region-in-`CmpMap` and Region-in-`CmpFrame` nestings must keep reading |
| C documentation audit (`test_c_docs_audit.rs`) | 0 modules | 0 | `C_CITATIONS_UNCLEANED` — empty: no top-level module's rendered rustdoc cites C source files or internal C names, and `rendered_docs_carry_no_c_references` fails for any module that gains one. The table survives so a module whose clean-up has to wait has a home, and `a_clean_module_cannot_stay_listed` fails when a listed module is clean |
| C documentation audit | 0 rows | 0 | `UNREVIEWED_ROWS` — ledger rows not yet reviewed, pinned by its own test rather than counted by the ledger. The review is complete: every row is reviewed, and the count may never rise |
| user guide (`test_guide.rs`) | 220 sections | 0 | `GUIDE_SECTIONS_PENDING` — SUN/211 sections not yet ported into `starlink_ast::guide`, pinned by its own test rather than counted by the ledger. The guide is complete: every section of SUN/211 211.34 (submodule `c6c60753`) is ported, merged or dropped with a reason in `crates/starlink-ast/src/guide/README.md`, and the count may never rise |
| SphMap vectorized drift (eight C-oracle tests) | — | 15 | `SPHMAP_DRIFT_LICENSED` in `test_frameset_integrity.rs` (1), `test_fitschan_encodings_oracle.rs` (2), `test_fitschan_cursor_oracle.rs` (1), `test_tran_grid_oracle.rs` (3 grids) and `test_lsst_oracle.rs` (4 comparisons), and empty in `test_fitschan_alt_epoch_oracle.rs` and `test_fitschan_axis_count_oracle.rs`; `SKY_OFFSET_SPHMAP_DRIFT_LICENSED` in `test_skyframe.rs` (4). Each case's values go through SphMap, whose vectorized sines, cosines and arctangents are each within 1 ULP of C's (`docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md`), and are compared by `sphmap_drift_eq` within the most drift measured in that test (a `MEASURED_BOUND` beside each list: 1 to 16 ULP, with an absolute floor where a value is the small difference of larger terms, and 4.5e-10 pixel for the LSST inverse, which PolyMap's iterative inverse carries to its TolInv); a licensed case that matches C exactly fails, so no list can go stale, and the TAN grids are pinned bit for bit to the port's own hashes. The count fell from 33 on 2026-10-28, when `vmath::atan2` began carrying the rounding error of `y / x` as a correction and rounding its final additions once: it then differs from macOS's libm for 1.6% of longitudes and 4.2% of latitudes rather than 12% and 18%. The sines and cosines are as close to libm as accuracy can take them: macOS's `__sincos_stret` is not correctly rounded for about 4% of arguments, the kernel for about 3%. No corpus value changes to or from BAD or NaN (`ast-bench ulps` against the vendored C, 2026-10-28). Interim: reassessed when the vendored C is gone, when C AST may adopt the same kernel |
| wcslib degree arctangents (`crates/starlink-ast/src/wcslib/trig.rs`, unit test) | 2 functions | 2 | `R2D_PRODUCT_ULPS` — `Kernel`'s `atan2d` and `atand` are compared with `Libm`'s within 2 ULP rather than 1. Each is the vectorized arctangent, asserted within 1 ULP of libm's in radians in the same test, times `R2D`; that product rounds once more, and since `R2D` is about 57.3 two radians 1 ULP apart can land 2 ULP apart in degrees. The 1-ULP rule is per elementary function, so the product's own rounding is this one extra unit, 2 being the most measured (2026-10-28, macOS arm64: 31797 of 2 000 000 `atan2d` values and 88341 of 2 000 024 `atand` values differ). `sind` and `cosd` are held to 1 ULP, their argument being the same rounded `a * D2R` libm receives |
| WcsMap TAN slice bound (`crates/starlink-ast/src/wcslib/prj.rs`, unit test) | 1 direction | 1 | `TAN_FWD_SLICE_ULPS` (18) — TAN's forward slice entry, which runs the vectorized `Kernel` trigonometry, against its per-value entry, which keeps wcslib's bits, over 1 000 000 random and special pairs. Each output is three operations on three kernel values (`r0 * cosd(theta) / sind(theta)` times `sind(phi)` or `cosd(phi)`), each within 2 ULP (2 * 2^-52 relative) of libm's, the premise `Kernel`'s documentation states for every slice bound: 6 * 2^-52 from the kernels, plus each path's rounding of its three operations, 3 * 2^-52 between the paths since their operands differ, so 9 * 2^-52 relative, and an ULP of the output is at least 2^-53 of it, so the bound is 18 ULP. Measured 2026-10-28: 5 against macOS libm on arm64 (4 in x, 5 in y), and 6 in both against macOS's x86-64 libm (under Rosetta, SSE2 build). Bad and good points are identical in every pair tested. No C-oracle test moved, so no case carries a WcsMap licence; TAN's reverse slice entry keeps the `Libm` loop and its bits |
| WcsMap SIN slice bound (`crates/starlink-ast/src/wcslib/prj.rs`, unit test) | 1 direction | 1 | `sin_fwd_drift_over_bound` — SIN's forward slice entry, which runs the vectorized `Kernel` trigonometry, against its per-value entry, which keeps wcslib's bits, over 2 000 000 random, near-pole and special pairs, for the orthographic projection, NCP's parameters at declination 30 and a general synthesis projection. The bound is per output, derived rather than measured: each output is `r0 (a + p z)`, `a` a kernel sine or cosine of `phi` times `cosd(theta)` and `z = 1 - sind(theta)`; with each kernel sine and cosine within 2 ULP (2 * 2^-52 relative) of libm's, the premise `Kernel`'s documentation states for every slice bound, and each operation rounded once on each path, the bound is `2^-52 (r0 (5 |a| + |p| (2 + 2 |z|)) + 2 |out|)` to first order (the `|p|` term only away from the pole, where `z` comes from a sine), a few ULP for the orthographic projection and an absolute bound where the synthesis terms cancel. Measured 2026-10-29: at most 0.33, 0.50 and 0.44 of the bound against macOS libm on arm64 (4 ULP in each output for the orthographic projection), and 0.43, 0.54 and 0.53 against macOS's x86-64 libm (under Rosetta, SSE2 build). Bad and good points are identical in every pair tested, among them a million pairs at and near the synthesis projection's lowest valid latitude (`sin_synthesis_limit_points_agree_on_bad_or_good`); the slice redoes with `Libm` a point within twice the derived bound on the kernels' limit, `R2D * 2^-52 * (3 + 4 (|p1| + |p2|)) + 2^-46` degrees, and with no margin that test finds a disagreement on both platforms. SIN's reverse slice entry keeps the `Libm` loop and its bits |
| WcsMap ZPN slice bound (`crates/starlink-ast/src/wcslib/prj.rs`, unit test) | 1 direction | 1 | `ZPN_FWD_SLICE_ULPS` (5) — ZPN's forward slice entry, which runs the sine and cosine of the native longitude with the vectorized `Kernel` trigonometry and the polynomial per point, against its per-value entry, which keeps wcslib's bits, over 2 000 000 random and special pairs, for a linear, a quadratic, a cubic and a quintic polynomial. Each output is the polynomial, computed alike on both paths, times one kernel sine or cosine, which is within 2 ULP of libm's, the premise `Kernel`'s documentation states for every slice bound: a relative difference of up to 2 * 2^-52, at most 4 ULP of the output, plus 1 ULP for the two paths' roundings, so 5 ULP. Measured 2026-10-29: 2 against macOS libm on arm64, and 4 against its x86-64 libm (under Rosetta, SSE2 build). The status depends on the latitude alone, so bad and good points are identical by construction, and in every pair tested. ZPN's reverse slice entry keeps the `Libm` loop and its bits |
| WcsMap TPN slice bound (`crates/starlink-ast/src/wcslib/prj.rs`, unit test) | 1 direction | 1 | `tpn_drift_over_bound` — TPN's forward slice entry, which runs its gnomonic front end with the vectorized `Kernel` trigonometry and its Newton iteration per point on the result, against its per-value entry, over 260 000 random, on-plate, near-equator, on-axis and special pairs with the DSS plate's parameters (`wcsconv/inputs/dss.ast`). The bound is per output, `NEWTON_RELATIVE_TOLERANCE^2 |x| + 2 nu`, derived rather than measured: nu (`tpn_fwd_step_noise`) bounds the two paths' difference in a computed Newton step from the rounding of the polynomial residuals (113 units of 2^-53 of the polynomials' term magnitudes, counted from the operations, with TAN's forward derivation, 9 * 2^-52 relative from 2 ULP per kernel sine and cosine, for the front end), and the tolerance-squared term is the one extra step a convergence test split at its threshold can cost, by quadratic convergence. A point with a coordinate below the floor where nu reaches `NEWTON_RELATIVE_TOLERANCE |x|`, such as the image of an axis of the projection plane, is redone with `Libm` and is bit for bit the per-value entry. Measured 2026-10-28, against the earlier 107-unit nu: at most 4.5e-5 of the bound (9.7e-15 relative in x, 77 ULP; 6.0e-16 in y, 5 ULP) against macOS libm on arm64, and 2.6e-4 of it (5.6e-13 in x, 4096 ULP; 1.1e-14 in y, 49 ULP) against macOS's x86-64 libm (under Rosetta, SSE2 build). Bad and good points are identical in every pair tested, among them points converging at the 49th, 50th and 51st iteration and on-axis points that are bad in the per-value entry (`tpn_cap_points_agree_on_bad_or_good`, `tpn_axis_points_match_the_per_value_entry_in_a_slice`). TPN's reverse slice entry keeps the `Libm` loop and its bits |
| WcsMap zenithal slice bounds (`crates/starlink-ast/src/wcslib/prj.rs`, unit tests) | 7 directions | 7 | The forward slice entries of AZP, SZP, STG, ARC, ZEA and AIR and the reverse slice entry of SZP, which run the vectorized `Kernel` trigonometry, against their per-value entries, which keep wcslib's bits, over 1 500 000 random and special pairs (300 000 for SZP's reverse) and every pair a test adds near a status threshold. Every bound is derived from the premise `Kernel`'s documentation states for every slice bound, each kernel sine and cosine within 2 ULP (2 * 2^-52 relative) of libm's and each arctangent 2 ULP after its product with R2D, with each operation rounded once on each path, plus 2^-1074 per operation where a result is subnormal (`SUBNORMAL_ROUNDING`): `RADIUS_TIMES_KERNEL_SLICE_ULPS` (5) for ARC and AIR, whose outputs are a radius computed alike on both paths times one kernel sine or cosine; `ZEA_FWD_SLICE_ULPS` (12), two kernel values and two operations; `stg_fwd_bound`, `azp_fwd_bound` and `szp_fwd_bound`, per output, relative 7 or 8 * 2^-52 plus the drift of the projection's denominator relative to the kernels' denominator, which grows where the denominator cancels (STG near the latitude -90, SZP with its default parameters near the latitude 0, AZP and SZP near their divergence limits), and for SZP's numerators, which can cancel too, absolute; and `SZP_REV_PHI_ULPS` (2) for SZP's reverse longitude, its latitude bit for bit. Measured 2026-10-29 against macOS libm on arm64: ARC 2 ULP, ZEA 4, AIR 2, STG 0.35 of its bound, AZP 0.27 to 0.46 over four parameter sets, SZP 0.37 to 0.64 over three, SZP's reverse longitude 2 ULP; against macOS's x86-64 libm (under Rosetta, SSE2 build): ARC 3, ZEA 4, AIR 4, STG 0.42, AZP 0.33 to 0.48, SZP 0.40 to 0.90, SZP's reverse 2. Bad and good points are identical in every pair tested, among them a million pairs at and near AZP's and SZP's divergence limits, where their denominators are zero. The slice redoes with `Libm` a point whose denominator is within twice its derived drift of zero (`azp_fwd_t_near_zero`, `szp_fwd_t_near_zero`), or, for STG, whose one plus the kernels' sine of the latitude is at most 2^-52 (`stg_fwd_s_near_zero`); a test per margin shows a margin of zero admitting kernel values within 2 ULP of Libm's whose status would differ. Two corpus cases moved, `wcsconv` `origin.head` (8 ULP) and `class-arc.head` (1 ULP) with their FrameSets, each through an ARC WcsMap's forward projection, within their tests' tolerances, so no case carries a licence; the other zenithal reverse slice entries keep the `Libm` loop and its bits |
| WcsMap CYP slice bound (`crates/starlink-ast/src/wcslib/prj.rs`, unit tests) | 1 direction | 1 | `cyp_fwd_bound` — CYP's forward slice entry, which runs the sine and cosine of the latitude with the vectorized `Kernel` trigonometry, against its per-value entry, which keeps wcslib's bits, over 1 500 000 random and special pairs and 1 000 000 pairs at and near the latitudes where its denominator is zero, for the default parameters, Gall's and (0.5, 0.8). The longitude output has no trigonometry and is asserted bit for bit. The bound on the latitude output is derived from the premise `Kernel`'s documentation states for every slice bound, each kernel sine and cosine within 2 ULP (2 * 2^-52 relative) of libm's, with each operation rounded once on each path: the denominator `s = p1 + cosd(theta)` drifts by at most `D = 2^-52 (2 |cosd(theta)| + |s|)`, so the output by `(4 * 2^-52 + D / (|s| - D)) |y|` to first order, plus 2^-1074 per operation where a result is subnormal. Measured 2026-10-29: 0.35, 0.40 and 0.50 of the bound against macOS libm on arm64, and 0.37, 0.41 and 0.97 against macOS's x86-64 libm (under Rosetta, SSE2 build); the largest is where the cosine is near -0.5, the edge of a binade, where 2 ULP is exactly the 2 * 2^-52 relative the premise allows. Bad and good points are identical in every pair tested; the slice redoes with `Libm` a point whose denominator is within twice its derived drift of zero (`cyp_fwd_s_near_zero`), and a test shows a margin of zero admitting kernel cosines within 2 ULP of Libm's whose status would differ. No corpus case has a CYP WcsMap, so none moved; CYP's reverse slice entry keeps the `Libm` loop and its bits |
| WcsMap AIT slice bound (`crates/starlink-ast/src/wcslib/prj.rs`, unit tests) | 1 direction | 1 | `ait_fwd_bound` — AIT's forward slice entry, which runs its whole body with the vectorized `Kernel` trigonometry, against its per-value entry, which keeps wcslib's bits, over 1 500 000 random and special pairs and 40 000 near (0, 180) and (360, 0), where the divisor of its square root's argument is zero. The bound is derived from the premise `Kernel`'s documentation states for every slice bound, each kernel sine and cosine within 2 ULP (2 * 2^-52 relative) of libm's, with each operation rounded once on each path: the divisor `1 + cthe chalf` drifts by at most `D = 2^-52 (5 |cthe chalf| + |divisor|)`, the square root by half that relative plus a rounding, and the outputs by 6 and 3 * 2^-52 more, to first order, plus 2^-1074 per operation where a result is subnormal. Measured 2026-10-29: 0.41 of the bound against macOS libm on arm64 and 0.50 against macOS's x86-64 libm (under Rosetta, SSE2 build). The slice redoes with `Libm` a point whose divisor is within twice its derived drift of zero (`ait_fwd_divisor_near_zero`), so NaN and infinite outputs are identical, and a test shows a margin of zero admitting kernel cosines within 2 ULP of Libm's whose divisor's sign would differ. Seven corpus lines moved, each through an AIT WcsMap's forward projection and within its test's tolerance, so no case carries a licence: `wcsconv` `ait-highlatpole.head` (256 ULP in a pixel coordinate that cancels to near zero), `atf.head` (8 ULP) and `aitoff.head` (2 ULP), each with its FrameSet, and `simplify` `spec_cel_invert.simp` (2 ULP). MOL's forward slice entry was rejected and keeps the `Libm` loop and its bits, as do the pseudo-cylindrical reverse slice entries |
| WcsMap conic slice bounds (`crates/starlink-ast/src/wcslib/prj.rs`, unit tests) | 5 directions | 5 | `conic_fwd_bound` with `conic_fwd_radius`, and `coo_rev_phi_bound` — the forward slice entries of COP, COE, COD and COO and the reverse slice entry of COO, which run the vectorized `Kernel` trigonometry, against their per-value entries, which keep wcslib's bits, over 1 500 000 random and special pairs and 80 000 at and near the poles, with the benchmarks' parameters (45, 10) and (-30, 15) for each, and (80, 10) for COE, whose square root's argument is zero at the north pole; 300 000 pairs for COO's reverse. Every bound is derived from the premise `Kernel`'s documentation states for every slice bound, each kernel sine and cosine within 2 ULP (2 * 2^-52 relative) of libm's and each arctangent 2 ULP after its product with R2D, with each operation rounded once on each path, plus 2^-1074 per operation where a result is subnormal: the outputs `x = r S` and `y = w2 - r C` differ by `|S| dr + 3 * 2^-52 |x|` and `|C| dr + 3 * 2^-52 |r C| + 2^-52 |y|`, with `dr` zero for COD and COO, whose radius is computed alike on both paths, `(D / (|R| - D) / 2 + 2 * 2^-52) |r|` for COE, its radicand `R` drifting by `D = 2^-52 (3 |w5 sind(theta)| + |R|)`, and `6 * 2^-52 |q| + 2^-52 |r|` for COP, `q` the cone's quotient; COO's reverse longitude by `3 * 2^-52` of itself, its latitude bit for bit. Measured 2026-10-29 against macOS libm on arm64: 0.62 to 0.65 of the bound for COP, 0.62 to 0.71 for COE, 0.66 to 0.75 for COD, 0.68 to 0.73 for COO and 0.38 to 0.53 for COO's reverse longitude; against macOS's x86-64 libm (under Rosetta, SSE2 build): 0.66, 0.63 to 0.75, 0.75 to 0.79, 0.69 to 0.77 and 0.38 to 0.53. Bad and good points and NaN outputs are identical in every pair tested. The slice redoes with `Libm` a point whose COE radicand or COP `r w0` is within twice its derived drift of zero (`coe_fwd_radicand_near_zero`, `cop_fwd_r_w0_near_zero`), or whose COP cosine is within two of the least subnormals of zero (`sincosd_near_zero`), and a test per margin shows a margin of zero admitting kernel values within 2 ULP of Libm's whose status or sign would differ. A WcsMap's inverse redoes with `Libm` a COO, BON or SZP reverse point whose longitude from the kernels is within twice that longitude's derived drift (`lon_drift`: 3, 6 and 2 * 2^-52 of itself) of a limit of the range it accepts (`LonRange::near_limit` in `mappings/wcsmap.rs`), which `inverse_longitudes_near_a_limit_are_decided_as_per_value` shows is needed: without it a longitude at -180 less the tolerance is accepted in one and rejected in the other; SZP is cyclic, so its longitude is never tested (`szp_is_cyclic_so_its_longitude_is_not_tested`). No corpus value moved: the one case with a conic projection, `wcsmap_cod.map`, is unchanged at its 24 oracle rows, so no case carries a licence; the reverse slice entries of COP, COE and COD keep the `Libm` loop and its bits |
| WcsMap polyconic slice bounds (`crates/starlink-ast/src/wcslib/prj.rs`, unit tests) | 2 directions | 2 | `bon_fwd_bound` and `bon_rev_phi_bound` — the forward and reverse slice entries of BON, which run the vectorized `Kernel` trigonometry, against its per-value entries, which keep wcslib's bits, over 1 500 000 random and special pairs forward and 300 000 reverse, with BON's standard parallel at 45 and at -30 degrees. Every bound is derived from the premise `Kernel`'s documentation states for every slice bound, each kernel sine and cosine within 2 ULP (2 * 2^-52 relative) of libm's and each arctangent 2 ULP after its product with R2D, with each operation rounded once on each path, plus 2^-1074 per operation where a result is subnormal; the forward projection's second sine and cosine are of an argument `a` from the kernels' cosine, drifting by 4 * 2^-52 of itself, and move by that argument's drift in radians, its rounding there, and 4 * 2^-52 relative (`sincosd_moved`: the kernel within 1 ULP of the exact value at each argument, as `Kernel`'s documentation states, and within 2 ULP of libm's at the same one); the reverse longitude drifts by 6 * 2^-52 of itself, the latitude bit for bit. Measured 2026-10-29 against macOS libm on arm64: 0.54 and 0.69 of the bound forward and 0.48 and 0.43 for the reverse longitude; against macOS's x86-64 libm (under Rosetta, SSE2 build): 0.51 and 0.80, and 0.48 and 0.47. Bad and good points are identical in every pair tested. The slice redoes with `Libm` a reverse point whose cosine of the latitude is within two of the least subnormals of zero (`sincosd_near_zero`), and a WcsMap's inverse one whose longitude is within twice its drift of a limit of the range it accepts, as for COO. No corpus value moved: the one case with a polyconic projection, `wcsmap_bon.map`, is unchanged at its 24 oracle rows, so no case carries a licence; BON with a zero standard parallel, which is SFL, and PCO in both directions keep the `Libm` loop and its bits, PCO's forward because near the equator its drift, about 7e-9 degrees (bounded by 3e-8) from a cancellation in one less a kernel cosine, is about 1e6 ULP of its coordinate scale, the 57.3-degree radius, where every kept direction stays within a few ULP of its own |
| WcsMap quad-cube slice bounds (`crates/starlink-ast/src/wcslib/prj.rs`, unit tests) | 2 directions | 2 | `tsc_fwd_bound`, and bit for bit for CSC — the forward slice entries of TSC and CSC, which run their direction cosines with the vectorized `Kernel` trigonometry and the rest per point with `Libm`, against their per-value entries, which keep wcslib's bits, over 1 500 000 random and special pairs and 200 000 at and near the edges and corners of the faces. The bound is derived from the premise `Kernel`'s documentation states for every slice bound, each kernel sine and cosine within 2 ULP (2 * 2^-52 relative) of libm's, with each operation rounded once on each path, plus 2^-1074 per operation where a result is subnormal: each direction cosine drifts by at most 5 * 2^-52 of itself, and TSC's position, a quotient of two of them, by 11. Measured 2026-10-29: 0.29 of the bound for TSC (5 ULP) against macOS libm on arm64 and 0.34 (7 ULP) against macOS's x86-64 libm (under Rosetta, SSE2 build); CSC bit for bit on both. Bad and good points are identical in every pair tested. The slice redoes with `Libm` a TSC point whose face is decided by two direction cosines within twice their drift of each other (`cube_face_near_tie`), and a CSC point whose cosine of the latitude or direction cosine is within twice its drift, in ULP, of a midpoint between single-precision values (`f32_rounding_near`); a test per margin shows a margin of zero admitting values within 2 ULP of Libm's whose face or rounding would differ. No corpus value moved: `wcsconv` `tsc.head` and `cobe.head` with their FrameSets are unchanged at their oracle rows, so no case carries a licence; QSC in both directions and the TSC and CSC reverse slice entries keep the `Libm` loop and its bits, QSC's forward because near a face's centre its drift, about 2e-10 degrees (bounded by 5e-10) from the cancellation in one less the largest direction cosine, is 1e4 to 1e5 ULP of its face coordinates of 45 to 90 degrees, where every kept direction stays within a few ULP of its own |

What the upstream `IsSimp` fix did and did not do, since the two get conflated:
`e40e71b4` made **C's** placement deterministic — a property of the Mapping tree
rather than of which nodes some earlier probe happened to invert. Before it, no
structural rule could reproduce C's answer, so the differences were unfixable in
principle and were licensed as such. After it, the answer is a well-defined
target the port can be held to, and the six fixtures above are ordinary port
bugs against that target rather than C artifacts. The fix changed them from
"cannot be matched" to "not yet matched"; it did not match them.

Movement at the `f6762005` sync: the wcsconv tag licence went from 5 cases to
20, but narrowed from "drop every `IsSimp` and `Invert` line, unpaired" to "drop
the `IsSimp` lines with both counts pinned" — `Invert` is byte-compared again,
and the residue is one diagnosed card per plane rather than an undiagnosed
count mismatch. Refreshing the corpus against that same pin then took the
licence from 24 rows to 4: 20 of the 24 were pinned against references the
pinned C no longer reproduced, and the port already matched C on them. Only the
placement residue itself remains. The table row read 20 while the
test-enforced ledger read 24; the ledger was right. `skip_string_compare` went from 7 cases to 5:
`cap_matrixmap_03` became byte-exact outright, and `cap_cmpmap_03` /
`cap_specmap_02` moved to the counted licence (`cap_cmpmap_03` has since left it
altogether — §10.3). The negative-fixture row went from 132 to
131 when `neg_unitnormmap_nonunit_scale` was found to assert something C does not do —
C replaces its zero-shift WinMap with a diagonal MatrixMap — and was given a real
oracle reference instead of naming its own input. The `ignore_is_simp_line` row and
the empty rows are newly *listed* rather than newly added — they predate the
sync and were simply not written down, which is what the executable ledger now
prevents.

Movement at the simplify parity closeout: the corpus grew and the relaxations
did not. The simplify manifest went from 460 cases to 468 and the transform
oracle from 1614 sections to 1752, with every `*_ORACLE_EXCLUDED` list still
empty; the `ignore_is_simp_line` count moved with the negative fixtures added
alongside them. The ChebyMap linear-reduction divergence recorded in that pass
is not a row here: the port declines a C step that loses a ChebyMap's domain
scaling, and the two fixtures that would pin C's answer are kept out of the
corpus rather than compared under a loosened gate — `chebymap.md` carries it as
a `divergence:` verdict citing `docs/issues/c-library-quirks.md`, which is where
work refused rather than work owed belongs. The Polygon axis-1 guard asymmetry
from the same pass is a faithful mirror, logged in the same document, and
relaxes nothing.

Movement at the 9.5.0 sync: the corpus grew and no relaxation was added. The
simplify manifest went from 490 rows to 498, the wcsconv manifest from 193 to
194, the round-trip sweep from 808 files to 823 and the transform oracle from
2229 sections to 2261. Two counts moved with the corpus and not with the posture:
`skip_string_compare` and `ignore_is_simp_line` each rose by three, because three
of the eight new simplify fixtures are negative controls whose reference is their
own input.

Two rows here were stale rather than merely out of date, and the sync is what
surfaced them. The overrides row claimed the file declares nothing and is
asserted empty; the file has declared three overrides since `4f746fb`, and its
test asserts them as an allowlist, not as an emptiness check — so the row was
describing a gate that does not exist while the real one held. And
`FMA_SUBSTITUTIONS_LICENSED`'s row recorded the port's conclusion that the
unfused value is the right one; upstream's narrowing of that section to two rows
records the opposite, which is now stated beside it.

Known hole, not a relaxation: `native_round_trip_fixture`'s `trim()` /
`split_whitespace()` comparison — see the open row in §5.

---

## 7. Delivery-step work not yet started

Tracked here because these are the largest outstanding items in the project and
were previously indexed nowhere. The first row is the design's stated purpose.

| Area | State | Blocks |
|---|---|---|
| `starlink-ast-capi` C-ABI shim | UNSTARTED — `crates/starlink-ast-capi/src/lib.rs` is a one-line comment; 0 exported symbols, no `ast.h`, no `astClone`/`astAnnul` refcounting, no `astBegin`/`astEnd` handle stack, no thread-local status, no variadic `astSet` parsing, no `catch_unwind` boundary, and neither of the two CI header-drift checks exists | tiers 3 and 4 below; the drop-in-replacement goal (pyast / Starlink / DS9 linking unchanged) |
| `starlink-ast-capi` Object attributes with no `starlink-ast` value | OPEN, latent — starlink-ast has no counterpart for ObjSize (an object's in-memory size), Nobject (live objects per class), RefCount (handles per object) or the integer `astVersion` (only `constants::PACKAGE_STRING`, as text). Exposed by the first C caller of `astGetI( obj, "ObjSize" )`, `"Nobject"`, `"RefCount"` or `astVersion()` through the shim; RefCount and Nobject follow from the handle table, ObjSize and the integer version need defining there | `starlink-ast-capi` |
| `starlink-ast-capi` immutability enforcement (`AST__IMMUT`) | OPEN, design decision — see the note below | `starlink-ast-capi`; drop-in compatibility for callers that mutate a shared Mapping |
| Tier 3 — `ast-ffi-tests` | UNSTARTED — crate body is `#![allow(missing_docs)]`. Would run the real C `ast_tester` programs against the Rust `libast` | blocked on `starlink-ast-capi` |
| Tier 4 — `ast-oracle-tests` | UNSTARTED — same. Would link `starlink-ast` + reference `libast.a` for live bit/ULP comparison. **No test in the repo currently executes C code and compares live**; all C comparison is against frozen committed corpora | blocked on `starlink-ast-capi` |
| Tier 5 — `ast-bench` | LANDED — `run`, `compare` (saved baseline or `--base <ref>`), `snapshot`, `sweep`, `bits`, and `roman` (Starlink/ast pull request 51's Roman WCS benchmark); the `bench` set holds the real-world FrameSets and Mappings; Phase 0 call-count parity in `ast-oracle-tests`. Not in CI as a timing gate (machine-specific); see the perf row in §2 | — |
| SIMD transform comparison | DEFERRED until the C side's SIMD kernels (https://github.com/Starlink/ast/pull/51) land; then build the C engine from that branch and compare with `ast-bench roman` (which repeats that pull request's benchmark) and `ast-bench run --engine both`. `crates/starlink-ast/examples/bench_polymap_simd.rs` and the `simd-eval` feature are kept for it | — |
| `ast-plugins` (Grf / Grf3d backends) | UNSTARTED — one-line stub | `Plot` / `Plot3D`, and the `RegTrace` / `RegSplit` region deferrals in §1 |
| wcslib Rust port (step 6a), with ERFA and PAL | IN PROGRESS on DM-56839 (design `docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md`). Step 0 DONE 2026-10-28 (`docs/superpowers/plans/2026-10-27-vendored-library-update.md`): C AST's wcslib carries the upstream 8.10 projection fixes, ERFA is 2.0.1 and SlaMap's FK4/FK5 conversions call ERFA, merged upstream as Starlink/ast#100; the submodule is pinned at `3d3f255f`, `vendor/` re-copied from it, and WcsMap's limit tolerance and SpecMap's au ported to Rust. SphMap DONE 2026-10-27 (`docs/superpowers/plans/2026-10-27-native-erfa-sphmap.md`): `eraC2s` and `eraS2c` are Rust (`crates/starlink-ast/src/erfa/`), checked bit for bit against the vendored C, and SphMap runs every point through vectorized slice entries (`vmath::atan2`, `vmath::sin_cos`): on 1M Roman points forward 53 to 13.4 ns per point and inverse 19.5 to 11.8 (C 53 and 22); forward 16.0 once `vmath::atan2` carried its quotient's rounding error (2026-10-28, 262144 points, the same build 13.4 before), which took it from 2 ULP of libm to 1. On Linux x86-64 the slice loops run in an AVX2 and FMA build chosen at run time: forward 21.1 and inverse 13.8 ns per point on 1M Roman points (C 50.5 and 49.0), where the baseline SSE2 build ran forward at 0.79 of C's speed. One-off callers keep ERFA's exact arithmetic (interim, reassessed when the vendored C is gone). WcsMap DONE 2026-10-29: the port landed 2026-10-28, when every wcslib projection became Rust (`crates/starlink-ast/src/wcslib/`), checked bit for bit against the vendored C before that C was deleted, with `WcsMap` on slice entry points; `vendor/wcslib` is gone and its row in `docs/perf/README.md` records the timings. The stage, vectorization included, closed 2026-10-29, when every projection family had been through keep-or-reject, with the result recorded in `docs/perf/2026-10-29-macos-aarch64-native-wcsmap.tsv` (TAN forward 64.5 to 25.9 ns per point, SIN 50.7 to 26.4, ZPN 39.4 to 19.4, ARC 35.9 to 17.6, AIT 47.2 to 27.5, COD 36.1 to 18.2, BON 42.9 to 29.3; x86-64 record pending). Next: the rest of PAL and ERFA; ERFA (16,970 code lines) and PAL (983 + a 230-line `palwrap.c`) are still compiled and linked | — |

### `AST__IMMUT` at the shim boundary

C enforces, in every class as of the pending `u/timj/misc-bugs` merge, that a
Mapping which has been cloned or included in another Object may not have its
attributes changed — `Invert` excepted. `ast.news` files this under "may break
existing code": callers that previously changed `PolyMap.IterInverse`,
`MathMap.Seed` or `SplineMap.InvTol` through a shared pointer succeeded
silently and now get `AST__IMMUT`.

**None of this can arise in `starlink-ast`**, for three independent reasons: `Clone`
on `Box<dyn Mapping>` is a deep copy that recurses through compound classes;
`DynMapping` is single-owner and compound constructors consume their arguments;
and component accessors are `pub(crate)`, so a component is unreachable from
outside the crate. `ErrorCode::Immut` is generated from `ast_err.msg` and is
correctly unused. The rationale is recorded in `crates/starlink-ast/src/error.rs`'s
module docs so the unused variant is not mistaken for an omission.

**`starlink-ast-capi` restores the hazard by design** and has to decide how to police it.
The spec commits to `astClone` = `Arc::clone` sharing one `RefCell<AnyObject>`
across handles (design spec, "Handle registry in the owned-value world"), and
says nothing about immutability — so this is a gap in the shim design, not
deferred work.

The two halves are not equally hard:

- **"has been cloned"** is tractable: `Arc::strong_count() > 1` is a natural
  test at the point of mutation.
- **"included in another Object"** has no Rust equivalent, because inclusion
  *consumes* the object. When a C caller runs `astCmpMap(a, b)`, none of the
  three obvious options is right: deep-copying into the CmpMap leaves the
  caller's handles valid and independently mutable, which is what C forbids;
  moving makes the handles dangle, breaking the ABI; and doing nothing loses
  the error C now raises. The shim therefore needs to keep the `Arc`s alive and
  shared *and* carry an immutability flag per slot — state `starlink-ast` has no
  concept of, which means it belongs in the registry rather than in the object.

Deciding this before the shim is written is much cheaper than retrofitting it,
because it constrains the registry's slot representation.

Absent classes, for completeness (38,873 code lines of C at pin c6c60753, 21% of
`src/*.c` by code volume, with no Rust counterpart): `Plot` / `Plot3D` + `grf*`,
`YamlChan`, `XmlChan` with its `xml.c` parser, and `StcsChan`. `Table` /
`FitsTable`, `Moc` / `MocChan`, `IntraMap`, `XphMap`, the `Stc*` classes and the
`Mapping`-level algorithms `astResample`, `astRebin`, `astRebinSeq`,
`astTranGrid` and `astQuadApprox` are ported. None of the absent classes gates
active work; `Plot` gates only the `RegTrace` and `RegSplit` deferrals above.

## 8. Upstream syncs — done

Landed in `2efde3a`, `e0cb246`, `e029927` and `3a0a9d9`: nineteen upstream defect
fixes reconciled against `docs/issues/c-library-quirks.md`, five mirrored bugs
removed, 97 fixtures copied across byte-identical, and one divergence recorded
rather than closed (`IsSimp` card placement in a FITS-WCS read). The full account
is in [`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md).

Two rules came out of it and hold for the next pin move:

- Before moving the pin, diff the incoming range for serialization changes and
  check `docs/issues/c-library-quirks.md` for entries the port *mirrors*. Four of
  the five mirrors found this way were invisible to every existing test, because
  the fixtures had been generated by the buggy C.
- Freshness must be checked on every corpus the pin feeds, not just the ones a
  test names. `tools/oracle_fixtures/check_simplify_fresh.sh` covers the simplify
  corpus and must be run alongside `check_corpus_fresh.sh` after any pin move;
  before it existed, two stale references were read as port defects.

### Pin move to the reorganised fixture tree — done

Planned in
[`docs/superpowers/specs/2026-09-03-upstream-fixture-reorg-design.md`](docs/superpowers/specs/2026-09-03-upstream-fixture-reorg-design.md)
and executed in two parts: 23 commits upstream on `u/timj/more-fixtures-2`
(Starlink/ast PR #77), then the Rust side here.

Upstream and the port now share one fixture tree and one manifest per corpus.
`tools/fixtures/check_mirror.sh` diffs `crates/starlink-ast/tests/fixtures/` against
`reference/starlink-ast/ast_tester/fixtures/` and a CI job runs it on every pull
request, so a fixture can no longer exist on one side only. The transform-oracle
sections are keyed by the same relative paths both harnesses read, which is what
makes a fixture and its oracle section impossible to separate.

What the sync closed:

- **The `IsSimp` card did not survive being read back, for ten classes.** A
  subclass loader re-applies a validated attribute through a clear-on-set setter,
  which discarded the record `astLoadMapping` had just installed from the card.
  Upstream `07d0cd2d` guards each clear on the value actually differing; the port
  mirrors it across 27 setters. The 43 `.rt` round-trip references that existed
  only to record the defect are gone, and upstream gained `roundtrip.c` plus a
  `roundtrip_*` ctest family, which is the only family that can see a
  serialisation defect at all — every other one calls `astSimplify` after reading,
  and that re-stamps `IsSimp` on each node it visits.
- **A WinMap shift that is zero within `astEQUAL` is zero.** `cap_winmap_03`'s
  residual shift on axis 2 cancelled to exactly zero without a fused multiply-add
  and to about -1e-16 with one, and a ShiftMap writes a `Sft` card only for a
  non-zero shift, so the dump gained a line on a contracting build. Upstream
  `1e00a02c` snaps it in `astMapMerge`, beside the tolerance the scale terms were
  already tested against. That retired the fixture's opt-out and `rigby`'s
  `NUMERIC_PLATFORM_DRIFT` entry with it.
- **`Interval` overrides `astOverlap` and the port had collapsed it** into the
  generic implementation, which needs a boundary mesh an unbounded Interval cannot
  supply. Ported at the same granularity as C (§1).

Freshness after the move: `check_simplify_fresh.sh` reports 485 fresh, 5
numerically drifted, 0 card-drifted, 0 structurally drifted — and now covers the
whole corpus, because `gen_simp` links upstream's `oracle_register_intramaps` and
so can load the IntraMap fixtures it previously reported as structural drift.
`check_corpus_fresh.sh` reports 108 reproduced, 1 numeric.

That `check_corpus_fresh.sh` figure was the last one it produced. This move
renamed `wcsconv/wcsconv_tests.txt` to `wcsconv/cases.txt` and made its path
fields relative to the fixture root, and the script was not updated, so from
here until the 9.5.0 sync it died on a missing manifest rather than comparing
anything. A gate nobody ran is the failure mode §6's ledger exists to catch;
this one escaped it because it lives in `tools/` rather than in a test. Repaired
in the 9.5.0 sync, where it reports 109 reproduced and no drift of any kind.

A third rule comes out of this move:

- **A corpus generated on both sides can disagree with itself.** The `.ast`
  FrameSet round-trip corpus had a generator here and gained one upstream;
  keeping both would make "which is authoritative" a question nobody asks until
  the two diverge. The local generator and its two probes were deleted and the
  tree is mirrored, with the mirror asserted in CI.

Still open from this move: the pin points at a branch tip rather than a merge
commit, so it needs moving again once PR #77 lands. Of the two port gaps the new
round-trip sweep exposed, one remains, named in §6's ledger; the SkyAxis Format
gap closed on 2026-09-30.

Since 2026-09-30 that tip is `d9e43a6b` on `u/timj/norm-unit`, three `unit.c`
fixes and one `skyaxis.c` fix on top of `208d0998` found while porting
`NormUnit` (§2); it needs moving again when that branch merges too.
After the move `check_corpus_fresh.sh` reports 95 reproduced, 14 numeric and no
card-only or structural drift on arm64 macOS, where the numeric residue is the
Linux-capture drift the protocol leaves alone.
`check_simplify_fresh.sh` reports 496 fresh, 2 numerically drifted and no
card-only or structural drift. Upstream's ctest passes 2496 of 2496 at the new
pin.

## C → Rust test migration tracker

| C test (`ast_tester/`) | Rust counterpart | Status |
|---|---|---|
| testconvert.c | crates/starlink-ast/tests/test_convert.rs | ported 2026-06-12 |
| testframe.c | crates/starlink-ast/tests/test_frame.rs | ported 2026-09-30, the SkyFrame `Format` / `Unit` block included; the `astAxAngle` block is in `test_frame_geometry.rs`. `testframe.c`'s newer picked-axis block is not ported: it needs the picked-SkyAxis row in §2. The file also pins 46 Unit / NormUnit pairs captured from the C library; the three `unit.c` defects they exposed are fixed upstream on `u/timj/norm-unit` (`965b7587`, `25bd4924`, `8c734154`) and in the port, and are under "Resolved upstream" in `docs/issues/c-library-quirks.md`. `test_convert.rs` gained the matching `testconvert.c` Errors 52-53 |
| check_transform_oracle | crates/starlink-ast/tests/test_transform_oracle.rs | ported 2026-08-05 (submodule `3a3c3e51`); consumes `tests/fixtures/oracle/*.oracle`; all 2261 sections gated by three asserting tests (`head_`, `slamap_`, `simplify_sections_match_oracle`), with `every_oracle_section_is_gated` proving the partition covers the corpus; `transform_oracle_report` remains `#[ignore]` as the investigative view; `gen_transform_oracle` intentionally not ported |
| testsplinemap_c.c | crates/starlink-ast/tests/test_splinemap_c_api.rs | ported 2026-10-07, against the mirrored `2dspline_c.dat`; needed `SplineMap::from_scipy` and `SplineMap::coeffs` (audit SN-2). `testsplinemap.f`, the Fortran API in CMLIB order with `2dspline.dat`, is not ported |
| testcmpframe.c | crates/starlink-ast/tests/test_cmpframe.rs | ported 2026-04-27 |
| testflux.c | crates/starlink-ast/tests/test_flux.rs | ported 2026-09-30, including the `SFCBRW` read-back check added upstream on `u/timj/norm-unit` (`b4457da9`); porting it needed the FluxFrame default SpecVal (`DfSpc`) and the full FluxFrame Overlay |
| testframeset.c | crates/starlink-ast/tests/test_frameset.rs | ported 2026-05-01 |
| testskyframe.c | crates/starlink-ast/tests/test_skyframe.rs | ported 2026-05-02 |
| testspecflux.c | crates/starlink-ast/tests/test_specflux.rs | ported 2026-09-30 |
| testspecframe.c | crates/starlink-ast/tests/test_specframe.rs | ported 2026-05-06 |
| testtime.c | crates/starlink-ast/tests/test_timeframe.rs | ported 2026-05-06 |
| testkeymap.c | crates/starlink-ast/tests/test_keymap.rs | ported 2026-08-08; the nine functions added upstream at `f6762005` ported 2026-08-20 |
| testtable.c | crates/starlink-ast/tests/test_table.rs | ported 2026-10-12 with Table, including the purge with a parameter value added upstream at `f32573b1`. C stores a Table in its own cell to check `AST__KYCIR`, which a Rust value cannot do |
| testfitstable.c | crates/starlink-ast/tests/test_fitstable.rs | ported 2026-10-13 with FitsTable, including the checks added upstream with the `fitstable.c` fixes `369c0e91` and `ca8d3a82` (errors 31 to 42). Column data is a typed `ColumnData` rather than untyped memory |
| testmapping.c | crates/starlink-ast/tests/test_mapping.rs | ported — the four `IsSimple` functions added upstream at `f6762005`. `testselfsimplifyafterset` ported 2026-10-14 as `a_simplified_mapping_set_to_unit_simplifies_again`. The LinearApprox and PermMap-equality checks are `linear_approx_of_a_polymap` and `perm_map_equality_compares_referenced_constant_values`, and `testwcsmapequal`'s reverse comparisons are in `test_equal_oracle.rs` (2026-10-14). Only the no-op `Invert` set has no Rust equivalent, since `invert()` toggles |
| testfitschan.c | crates/starlink-ast/tests/test_fitschan*.rs | ported apart from 994-998 — `testgrismorder`, added upstream at `f6762005`, is in `test_fitschan_wcs_write.rs` (2026-08-20); the `905`-`912` SIP-validation cases, added upstream at `c384289f` and `121d12d2`, are in the same file (2026-09-24); the rest is spread across the seven `test_fitschan_*` files. `testtabspectrum` (`testfitschan.c:256`, added upstream at `39def937`), `checktab` and `checktab2` are in `test_fitschan_tab.rs` (2026-10-13), using `purge_wcs` as C uses `astPurgeWCS`; `test_fitsrounding` is `test_fitschan.rs::fits_rounding_formats_found_cards` (2026-10-13), so every helper function is ported. The inline checks in `main()` that no other test made are in `test_fitschan_c_inline.rs` (2026-10-14), apart from 994-998, which go through C's string attribute path and belong to `starlink-ast-capi`; see [`docs/issues/testfitschan-coverage.md`](docs/issues/testfitschan-coverage.md). Porting them fixed the FITS-WCS writer's `-X2P` choice, which tested an unsimplified pixel-to-X Mapping |
| testimmutable.c | (deferred) | tests `AST__IMMUT`, which guards a change made to a Mapping whose reference count exceeds one. Owned values, deep `Clone` and single-owner `Box` make that condition unconstructible in `starlink-ast`; the guard belongs to `starlink-ast-capi`, which restores sharing at the handle boundary and is unstarted (§7) |
| gen_simplify_fixtures / check_simplify | crates/starlink-ast/tests/test_simplify_regression.rs | ported; 481 of the 498 rows in the shared manifest against the pinned oracle's `.map` / `.simp` dumps, plus `test_serialisation`'s 823-file round trip over the same corpus. The 17 rows not run are named, all of them in `DEFERRED` (unported class), each with a companion test that fails when it starts working; `C_DEFECT_EXCLUDED` is empty. `gen_simplify_fixtures` itself stays in C: it writes the references |
| (no C ancestor) | crates/starlink-ast/tests/test_frame_in_map_slot.rs | added 2026-09-03; pins the reader's refusal of a C-written bare Frame in a `Map<n>` slot, with the settled Region nestings as controls (§9.6) |
| roundtrip.c | crates/starlink-ast/tests/test_serialisation.rs | ported 2026-09-04 as three sweeps covering the same file set upstream's `roundtrip_*` ctests glob: `simplify/*.{map,simp}`, `serialisation/*.ast`, `programs/**/*.ast` and `wcsconv/framesets/*.ast`. `wcsconv/` itself is excluded on both sides: its references are FITS-encoded, so reading one back needs a FitsChan and the encoding it was written with, which the wcsconv harness already does |
| testissimp.c | crates/starlink-ast/tests/test_clear_on_set.rs | ported 2026-09-04. Upstream built `testissimp.c` to cover the clear-on-set rule across all ten affected classes rather than the four the fixture corpus happens to reach; the Rust file already had a case per setter, and `load_preserves_the_record_for_every_clear_on_set_class` is the read-back half. Upstream needs `astCLASS` for `PermMap`'s `PermSplit` and `WcsMap`'s `FITSProj` / `LonCheck`, which are absent from those classes' `SetAttrib`; the port exposes all three as typed setters, so no equivalent is needed |
| compare_dumps.c | (not ported) | the comparator upstream's fixture-driven ctests use: token-wise with a 4-ULP budget and an `--astequal` mode. `tests/common/mod.rs` is the Rust equivalent and predates it (`native_strings_equivalent`, `native_strings_astequal`, `ast_equal_close`); the two were reconciled during the sync, which is where C's missing `1e-12` absolute floor and the port's invented `astEQUAL` tolerance were both found |
| testxphmap.c | crates/starlink-ast/tests/test_xphmap.rs | ported 2026-10-07 with XphMap: loading each projection type, the round trip, the HPX12 inverse and the refused Type; plus C's forward and inverse transformations of every type, bit for bit, from `tools/oracle_fixtures/xphmap_probe.c`. The C test's leak check has no counterpart, since nothing leaks the projection name in Rust |
| testtrangrid.c | crates/starlink-ast/tests/test_trangrid.rs | ported 2026-10-09 with `Mapping::tran_grid`; the FITS-WCS header gains an `NAXIS` card (see the §5 FITS-AIPS reader row). `test_tran_grid_oracle.rs` compares the grids bit for bit with `tools/oracle_fixtures/tran_grid_probe.c` |
| testrebin.c | crates/starlink-ast/tests/test_rebin.rs | ported 2026-10-10 with `mappings::rebin`: tests 1-9 for the `D`, `F` and `I` types and C's six spreading schemes. `test_rebin_oracle.rs` compares 135 cases over every pixel type and scheme, and 10 error results, with `tools/oracle_fixtures/rebin_probe.c` |
| testrebinseq.c | crates/starlink-ast/tests/test_rebinseq.rs | ported 2026-10-10 with `mappings::rebin_seq`: tests 1-12 for C's nine cases (1-3 dimensions with Gauss, nearest and linear spreading); the noise comes from a seeded `MathMap` `Gauss`, as in C, and matches C bit for bit. `test_rebinseq_oracle.rs` compares 90 call sequences over every pixel type and flag, and 12 error results, with `tools/oracle_fixtures/rebinseq_probe.c` |
| testhuge.c | crates/starlink-ast/tests/test_huge.rs | ported 2026-10-10, `#[ignore]`d as C's runs only under `TEST_HUGE=1`; run with `cargo test --release -p starlink-ast --test regions test_huge:: -- --ignored`. Passes in about 6 minutes with a 38 GB peak memory footprint. It first ran out of memory (killed at 81 GB) because nested series CmpMaps held a full copy of the points per level, which cmpmap.c avoids by transforming 8192 points at a time; the port now does the same, and `test_transform_memory.rs` holds the bound |
| testresample.c | crates/starlink-ast/tests/test_resample.rs, test_tran_grid_oracle.rs (Test 10) | ported 2026-10-10: tests 1-9 in `test_resample.rs`; test 10, `astQuadApprox`, is the `poly` case of `QUADS`. `test_resample_oracle.rs` compares 370 cases over every pixel type and interpolation scheme, flux conservation included, and 15 error statuses with `tools/oracle_fixtures/resample_probe.c` |
| testmocchan.c | crates/starlink-ast/tests/test_mocchan.rs | ported 2026-10-09 with `MocChan`, including `checkZeroLineLen` and `checkDumpFormat` added upstream at `7ef81c01`, `checkLoadNegativeLineLen` at `228a0310`, and the reversed MocEnc names with `checkLoadMasterDump` and its two `programs/testmocchan` fixtures at `f5fdca61`. `test_mocchan_oracle.rs` compares writes, reads and attributes with `tools/oracle_fixtures/mocchan_probe.c`, and `test_channel_dump_oracle.rs` Channel and MocChan dumps with `tools/oracle_fixtures/channel_dump_probe.c` |
| testmoc.c | crates/starlink-ast/tests/test_moc.rs | ported 2026-10-09, with the ten `check*` functions added upstream on `u/timj/norm-unit`. Its pixel mask needed FrameSet `MapSplit` (frameset.c), ported with it. `checkPinsMask` uses the port's public `Region::reg_pins_mask` for C's protected `astRegPins` |
| testobjectkeymap.c | (deferred) | tests the Object-associated retained KeyMap (`object.c:1736`, `:2480`), `astGetObjSize` and `astSame`; those calls (`astMapPut0I`, `astMapGet0I`, `astMapSize`) are only reachable through the protected `astGetKeyMap`/`astHasKeyMap` machinery, an explicit spec Non-goal of the owned-value port |
| testaxis.c | crates/starlink-ast/tests/test_frame.rs | ported (`unit_is_simplified`, through a one-axis Frame) |
| testslamap.c | crates/starlink-ast/tests/test_slamap.rs | ported; the FK45Z and FK54Z cases added upstream at `38e0bfcf` are `fk4_fk5_match_erfa` (2026-10-28) |
| testspecmap.c | crates/starlink-ast/tests/test_specmap.rs | ported 2026-10-28 as `topocentric_to_heliocentric_frequency`; the C test's other purpose, catching `astSpecAdd` reading past the caller's array under AddressSanitizer, has no Rust equivalent, since `SpecMap::add` takes a slice |
| testwcsmap.c | crates/starlink-ast/tests/test_wcsmap_edges.rs | ported 2026-10-28, every case with the C test's wanted values |
| testunitnorm.c | crates/starlink-ast/tests/test_frame.rs | ported (`norm_unit_matches_the_c_library`) |
| testerror.c | — | no Rust equivalent: the `astSetPutErr` reporter, and a ShiftMap with -1 axes cannot be built |
| testpointset.c | crates/starlink-ast/tests/pointset.rs | ported, apart from the options-string constructor and `ID`, which a plain-data PointSet does not have |
| testobject.c | crates/starlink-ast/tests/test_channel.rs | `TestCheckToString` ported 2026-10-14 as `converted_frameset_string_round_trip`; the rest tests the handle registry and creation-site tracking, which belong to `starlink-ast-capi` |
| testchannel.c | crates/starlink-ast/tests/test_channel.rs | ported; Errors 1-6 added 2026-10-14 as `default_skyframe_file_round_trip_is_equal`. Error 7 (a read while `SinkFile` is set) and the `astTuneC` checks have no Rust equivalent |
| testcmpmap.c | crates/starlink-ast/tests/test_map_split*.rs | ported; Errors 1-13 added 2026-10-14 as `testcmpmap_split_*` |
| testlutmap.c | crates/starlink-ast/tests/test_lutmap*.rs | ported; Errors 3-12 added 2026-10-14 as `flat_start_and_bad_value_tables` |
| testnormmap.c | crates/starlink-ast/tests/test_normmap.rs | ported; Errors 1-9 added 2026-10-14 as `normmap_on_a_permuted_cmpframe` |
| testrate.c | crates/starlink-ast/tests/test_rate.rs | ported 2026-10-14, one test per C block |
| testratemap.c | crates/starlink-ast/tests/test_ratemap.rs | ported 2026-10-14 (`rate_maps_of_a_compound_mapping`) |
| testresimp.c | crates/starlink-ast/tests/test_allow_simplify.rs, test_simplify_regression.rs | ported; Errors 4-10 added 2026-10-14 as `testresimp_*`, the restricted result compared with the dump the C library writes; the `astDecompose` checks (Errors 6-8, 11-13) added 2026-10-21 through `CmpMap::decompose` |
| testunitnormmap.c | crates/starlink-ast/tests/test_unitnormmap.rs | ported 2026-10-14; `testsimplify` is `c_testsimplify` |
| testzoommap.c | crates/starlink-ast/tests/test_zoommap.rs | Errors 1-8 ported 2026-10-14 as `zoom_attribute_set_clear_and_get`; Errors 9 and 11, `AST__IMMUT` on a ZoomMap shared with a CmpMap, belong to `starlink-ast-capi` |
| testregions.c | crates/starlink-ast/tests/test_regions_c.rs, test_regions.rs and the Region test files | all 30 functions ported (2026-10-14, `test_regions_c.rs` holding the ones the class files did not); `checkEllipse` Errors 15-17 are a recorded gap (§6). `checkBox` transforms its CmpFrame through the Mapping C's CmpFrame `Transform` builds, since a Frame is not a Mapping here (§9.6). Porting them fixed three library gaps: Regions on a FrameSet path did not clip, a conversion to a Region did not use its Frame, and a CmpFrame reported a SkyFrame axis's display Unit as its InternalUnit. `checkBox` and `checkConvex` never ran in C until `f111c05c` on `u/timj/norm-unit`, which fixed their `status` guards and three Fortran-conversion slips |
| testregionmasking.c | crates/starlink-ast/tests/test_region_masking.rs | ported 2026-10-14, pinning the pixel counts the C library reports as well as its assertions |
| testfitschanerrors.c | crates/starlink-ast/tests/test_fitschan_errors.rs, test_fitschan_cursor_oracle.rs, test_fitschan_tab_read_oracle.rs | ported 2026-10-14: the bad-header table is generated from the C source and run by C's own procedure; 30 of its 34 cases match C, 3 have no Rust equivalent and 1 exposes the duplicate-keyword gap (§5, §6). The direct checks outside the table are in the cursor and `-TAB` read oracles |
| testswitchmap.c | crates/starlink-ast/tests/test_switchmap.rs | ported 2026-10-14 as `testswitchmap_c`, one test per section. It exposed a SwitchMap simplification gap: the neighbour cancellation went through the generic pairwise merge, which compared one SwitchMap's forward selector with the other's inverse selector, so an inverted SwitchMap and its forward copy with different selectors did not cancel; `merge_with_neighbour` now ports switchmap.c's comparison, with a corpus fixture (`switchmap_inverse_cancel_distinct_selectors`) |
| testchebymap.c | crates/starlink-ast/tests/test_chebymap.rs | ported 2026-10-14 (`c_*`, in the C program's order) apart from Error 4, the IterInverse attribute, which ChebyMap does not expose; porting the PolyTran half added `ChebyMap::poly_tran` and fixed an inverse box kept with no inverse coefficients (CH-4, CH-7 in `docs/audits/mapping-c-parity.md`) |
| testpolymap.c | crates/starlink-ast/tests/test_polymap.rs, test_polytran.rs | ported 2026-10-14 (`c_*` and the 8019-8028 tests). Errors 3002-3003 needed the CmpMap split of `cmpmap.c:1677-1734` and C's Frozen flag kept on the Mapping |
| teststc.c | — | deferred: every function reads STC XML through XmlChan, which is not ported |
| testxmlchan.c / teststcschan.c / testyamlchan.c | — | deferred: XmlChan, StcsChan and YamlChan are not ported |
| testgrid.c / testplot3d.c / testplotter.c | — | deferred: Plot is not ported |
| testthreads.c | — | not applicable: tests C's thread-local handle and status machinery |

## 9. Findings from the fixture regeneration

The `.simp` corpus was stale relative to the submodule pin. Regenerating it
against the pinned C closed two disagreements and opened two questions.
Only §9.6 remains open, and it is not a simplifier gap: it waits on a maintainer
decision. §9.4 closed at the 9.5.0 sync, which brought the Rust-only fixtures'
oracle sections into the shared tree.
What the gates hold today is stated with its evidence in
[`docs/reports/2026-09-03-simplify-parity-final.md`](docs/reports/2026-09-03-simplify-parity-final.md).

Closed entries keep their numbers, and each one's write-up is in
[`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md).

| § | Finding | Resolution |
|---|---|---|
| 9.0 | The simplify and Region harnesses each hand-rolled a `&dyn Mapping` -> `AnyObject` dispatch that returned `None` for whole classes, from inside the `if let` that guarded the byte comparison | CLOSED — one shared dispatch; the eleven failures it had been hiding were exposed and fixed |
| 9.1 | `cefit_circle_*` emitted an `IsSimp` card on an inner Mapping that the pinned C does not | CLOSED — C's loader discards the card, so the two dumps describe the same Mapping |
| 9.2 | C's clear-on-set convention: a setter for an attribute feeding the first part of its own `astMapMerge` discards both simplified records | PORTED for every attribute the port can mutate, and gated; the five attributes named in §10's open list have no setter to attach it to yet |
| 9.3 | 24 of the 109 NATIVE-family wcsconv references looked stale against the new pin | DONE — replayed against the pinned C with each row's `attrs`; no keyword sequence differed in any of the 109 and the residue is cross-platform capture drift |
| 9.5 | Audit checklist citations were not anchored to a pin, so a line number could rot unnoticed | CLOSED — all 37 checklists in `docs/audits/mapmerge/` declare pin `3e60abaa`, held by six gates in `test_audit_citations.rs` |

### 9.4 Nine simplify fixture files have no transform-oracle section — CLOSED except for the nine that cannot have one

The 9.5.0 sync closed the Rust-only half of this. Of the 842 `.map` / `.simp`
files in `crates/starlink-ast/tests/fixtures/simplify/`, nine carry no oracle
section, and all nine are the kind that cannot.

**Nine cannot have one.** `cap_matrixmap_04.map`, `cap_specmap_02.map` and
`.simp`, `neg_ratemap_different_inner.map`, `neg_ratemap_diff_indices.map`,
`neg_ratemap_diff_inner.map`, `ratemap_inverse_cancel.map`,
`selectormap_inverse_cancel.map` and `selectormap_inverse_cancel_tail.map` are
CmpMaps whose dump carries `Fwd = 0` and `Inv = 0`: neither transformation is
defined, so `astTran` has nothing to compute and `gen_transform_oracle` emits
nothing (`gen_transform_oracle.c:210-211` reads `TranForward` / `TranInverse` and
skips each direction that is absent). These fixtures exist to pin structure —
what the simplifier does or declines to do to the tree — and that is what
`test_simplify_regression.rs` compares.

**The forty-two Rust-only fixtures are covered.** Twenty of the twenty-two
Rust-only stems were the Region `Simplify` fixtures upstream had never seen;
they now live in the shared tree, and the 9.5.0 corpus carries their sections.
The two that do not are `neg_cmpmap_neighbour_not_cmpmap` and
`neg_win_series_cmpmap_neighbour`, which are no longer in the tree at all: the
fixture reorganisation dropped them rather than upstreaming them, so they have
neither a manifest row nor an oracle section and nothing reads them.

This was never a hole in the transform-oracle gates:
`resolves_every_oracle_relpath` asserts every oracle *section* resolves to a
fixture, and all 2261 that exist are gated with empty exclusion lists. Each
fixture is also compared for serialisation byte-exactness and `IsSimp` placement
by `test_simplify_regression.rs`; what the sections add is the independent check
that the simplified Mapping *transforms* points the way C's does.

Sections are captured from the oracle, never by regenerating the file: a full
regeneration rewrites every value in `headers.oracle` and `framesets.oracle` in
the last digit or two — platform drift, which the corpus-freshness protocol says
to leave alone.

The two oracle trees are now byte-identical, which is what
`tools/fixtures/check_mirror.sh` asserts: the submodule copy lives at
`reference/starlink-ast/ast_tester/fixtures/oracle/` beside the other shared
fixtures, and holds the same 1631 simplify sections, 353 frameset sections and
277 header sections this tree does.

### 9.6 A bare Frame where a Mapping is expected — DECIDED 2026-10-13, not supported

C accepts a Frame in any Mapping slot, because `Frame::Transform`
(`frame.c:11739`) transforms through a UnitMap (`frame.c:11806`).
This port makes Region the only Frame that is also a Mapping, so a `Map<n>` slot
holding `Begin Frame` or `Begin CmpFrame` is refused.

The maintainers decided that a Frame does not act as a Mapping in this port:
nobody could name a real-world use, so C's acceptance is taken as a side effect
of making Frame a Mapping to support Region.
The question, its C citations, the cost of each answer and the decision are in
[`docs/issues/frame-in-a-mapping-slot.md`](docs/issues/frame-in-a-mapping-slot.md).

The artifact is `crates/starlink-ast/tests/fixtures/frame_in_map_slot.ast`, a dump
the C library wrote, reproducible with
`tools/oracle_fixtures/frame_in_map_slot_probe.c`.
`crates/starlink-ast/tests/test_frame_in_map_slot.rs` pins the refusal and keeps it
narrow: the 33 Region-in-`CmpMap` nestings in the fixture corpus and the
Region-in-`CmpFrame` dump the same probe wrote must keep reading, so the gap
cannot be closed by refusing Frames in Mapping slots wholesale.

`C_SIMPLIFY_SEAMS` records `cmpframe.c` as
`declined: docs/issues/frame-in-a-mapping-slot.md`, and requires that document
to state the question, record the decision and name the fixture;
`FRAME_IN_MAP_SLOT_DECLINED` counts the one fixture in §6.

`CmpFrame::Simplify` (`cmpframe.c:8963-9050`, installed at `:4745`) is the seam
the decision governs. It calls `astSimplify` on each component Frame and, if
either returned a different object and both are still Frames, returns a copy of
the CmpFrame holding the simplified components.
`crates/starlink-ast/src/frames/cmpframe.rs` has no counterpart and cannot have one
while `CmpFrame` is not a `Mapping`, because the operation C spells
`astSimplify( cmpframe )` is not expressible through the Rust API.
Reachability is narrow but real: only `region.c`, `frameset.c`, `stc.c` and
`cmpframe.c` override the Frame-level `Simplify` slot, so simplifying a component
Frame changes something only when that component is a Region, a FrameSet, an Stc
or a nested CmpFrame containing one.
Nothing inside AST reaches the body except itself — `astSimplify` is applied to a
Frame-typed argument only at `cmpframe.c:9019-9020`, and `FrameSet::Simplify`
reduces the inter-node Mappings without touching the Frames
(`frameset.c:10195`) — so the entry point is a caller's own `astSimplify` on a
CmpFrame, which is why no fixture in the corpus exercises it and no result in the
corpus is wrong because of it.

What made this invisible is worth more than the gap itself: `audit_universe` in
`test_mapmerge_audit.rs` was derived from the Rust module tree, so a C seam with
no Rust module produced no checklist to be missing, and
`every_audited_class_has_a_checklist` passed. The gate now takes the C source as
the authority on which classes have a seam: `C_SIMPLIFY_SEAMS` lists all 41 C
files that install a `MapMerge` or `Simplify` vtab entry with what covers each,
and when the submodule is present the list is re-derived from
`reference/starlink-ast/src/*.c` and required to match, so a pin move that adds
a seam fails there rather than passing unnoticed.

## 10. Gaps named by the `MapMerge` / `Simplify` audit

Each entry was cited by a `gap` verdict in a checklist under
`docs/audits/mapmerge/`. Every one is closed, so no row cites this section any
more and the `gap:` verdict form has been withdrawn; `no_row_is_left_at_gap` and
`every_checklist_row_has_a_verdict` together keep it that way.

Closed entries keep their numbers, and each one's write-up is in
[`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md).

What is left open anywhere in this section:

- **§9.2** — closed 2026-10-08: `ZoomMap::Zoom`, `LutMap::LutEpsilon`,
  `PcdMap::Disco`, `IntraMap::IntraFlag` and GrismMap's attributes all have
  setters now, and each discards the simplified record as C's does.

| § | Gap | Resolution |
|---|---|---|
| 10.1 | RateMap's self-reduction sat in `simplified()`, unreachable from the nominate loop | FIXED — `map_merge` uses `self_reduce_map_merge`, which is C's order: reduce first, then cancel against an inverse neighbour |
| 10.2 | ChebyMap reached neither half of the `MapMerge` it inherits from PolyMap | FIXED — both halves run; upstream's `abe5baae` fixed the linear reduction's lost normalisation and the port ported it, retiring its one `C_DEFECT_EXCLUDED` row |
| 10.3 | Three divergences in the port's handling of C's parallel-in-series branch | FIXED — rows 22, 23 and 26 of `cmpmap.md` are `ported`, and two fixtures came byte-exact with the change |
| 10.4 | The CmpMap/PermMap swap was hung off the PermMap, where C hangs it off the CmpMap | FIXED — it runs when the CmpMap is nominated, as the last block of its `map_merge` |
| 10.6 | A parallel UnitMap's Invert flag looked unported | FIXED 2026-10-13 — the retraction was wrong: the dump came from §10.22's tree rewrite, not the rebuild; `UnitMap::map_merge` now clears the flag as C does |
| 10.7 | PcdMap's zero-distortion reduction was unreachable from the nominate loop | FIXED — same shape as §10.1, moved into `pcd_map_merge` ahead of the neighbour tests |
| 10.8 | LutMap's linearity test used a hardcoded tolerance with no analogue in C | FIXED — scaled by `LutEpsilon` and taken as the larger of C's two estimates |
| 10.9 | TimeMap dropped one no-effect change-of-system step where C drops three | FIXED — the derived third argument is tested for `MJDTOMJD`, `MJDTOJD` and `JDTOMJD` alike |
| 10.10 | SwitchMap left its simplified children untagged | FIXED — the tag is raised where C raises it, confirmed against a C trace of the three nominations |
| 10.11 | `TranMap::map_merge` reached one of C's four self-simplification outcomes | FIXED — all four live in `map_merge`, in C's order |
| 10.12 | `GetDefUnc`'s unbounded-axis guard caught an unbounded lower bound but not an upper one | FIXED — both bounds tested against `±DBL_MAX` as C does |
| 10.13 | Box's PermMap simplification branch was unported | FIXED — the permutation arrays and constants are read directly and the current-Frame Region built axis by axis |
| 10.14 | Three claims that the port over-reaches C's nominate loop | RETRACTED — a trace of the C library's three dispatchers disproved all of them |
| 10.15 | Nine `C-ARTIFACT` sites said to exist only to reproduce C's memory model | RESOLVED — four mechanisms push an inverted CmpMap's inversion into its structure, three were redundant and are gone, one is load-bearing and measured |
| 10.16 | The port had a per-class `simplified()` seam C does not have | FIXED — collapsed onto C's two seams, `Simplify` and the repeated `astMapMerge` of the base implementation |
| 10.17 | NormMap did not simplify its encapsulated Frame | FIXED — in C's order: simplify the Frame first, then the basic-Frame test, then the neighbours |
| 10.18 | Polygon's vertex-transform simplification skipped both of C's guards | FIXED — both guards ported, `SimpVertices` added, and the uncertainty passed in the Frame the new vertices live in |
| 10.19 | A recursion-depth cap of ten and a structural hash standing in for `astEqual`, neither with a counterpart in C | CLOSED — both removed, and what C does instead is gated |
| 10.20 | The restricted-simplify path dropped three of C's steps | FIXED — all three ported, each with a fixture pair that fails without it |
| 10.21 | `astDoNotSimplify` and the Mapping-level `Ident` it reads were unported | FIXED — all three generic-driver call sites honour it |
| 10.22 | The public entry performed two tree rewrites C's `astSimplify_` does not | FIXED — `narrow_unit_scale_winmap_children` was dead and deleted; `normalize_unitmap_children` stood in for C's unported parallel branch of `UnitMap::MapMerge`, which is now ported, and is deleted |
| 10.23 | The uncertainty simplification asked `Mapping::equal` where C asks pointer identity | FIXED — any reduction counts, including one that rewrites only the structure |
| 10.24 | A merged Region's combined uncertainty went straight to the parallel CmpMap `Prism::Simplify` starts from | FIXED — the Prism's tail contributes, so the uncertainty is built as C builds it |
| 10.25 | A bounded Interval was reclassified to a Box without simplifying it | FIXED — C's `Cache` builds the base-Frame Box and the result is simplified |
| 10.26 | `MergePointList` installed its combined uncertainty by assignment | FIXED — installed through `set_unc`, as C's shared merge tail does |
| 10.27 | PcdMap's swap half was a sketch of C's | FIXED — five divergences closed, including the speculative swap with no merge target in reach |
| 10.28 | LutMap's linearity test and merge tail diverged from C's in four places | FIXED — nearest-neighbour tables refused up front, and the tail matches C |
| 10.29 | PolyMap's merge half, and a third `astDoNotSimplify` site in the shared dispatcher | FIXED — three divergences closed |
| 10.30 | ChebyMap's cancellation test was hand-rolled and never looked at the inverse terms | FIXED — it goes through the inherited equality, bounding boxes and both coefficient sets |
| 10.31 | WcsMap's swap driver was two attempts where C runs a search | FIXED — C's bidirectional PermMap walk, nearer-target choice and speculative swap are ported |

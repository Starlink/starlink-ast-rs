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
| Post-Phase-5 coverage sweep (broader vtab override audit) | `docs/audits/region-class-coverage-audit.md` | OPEN — 7 findings, none a current red gate; see below |

Per-area source audits feeding the two closed docs above (kept for reference):
`region-base.md`, `region-box-interval-pointlist.md`,
`region-circle-ellipse-polygon-null.md`, `region-cmpregion-prism.md`.

### Outstanding region findings (from the coverage sweep)

Detail and C↔Rust locations in `docs/audits/region-class-coverage-audit.md`.

| ID | Gap | Gated behind | Priority |
|---|---|---|---|
| MESH-GEO | Polygon `reg_base_mesh` planar not geodesic (polygon.rs:623 vs polygon.c:4329) | — (latent; sky-polygon fixture) | highest — affects overlap/MOC when reached |
| PLY-DOWN | Polygon `Downsize`/`DownsizePoly` unported; verify Simplify vertex reduction | — (latent; serialization parity) | high — confirm before scheduling |
| GRID-FRM | `reg_base_grid` generic odometer, no Frame-aware `astFrameGrid` (data.rs:354) | Frame work | medium |
| BND-INT | Interval `BndBaseMesh` override missing (overlap.rs:376 vs interval.c:215) | overlap pass | medium |
| RT-TRACE | `RegTrace` boundary tracing unported (base + 6 classes) | Plot/Grf backend | deferred |
| RS-SPLIT | CmpRegion `RegSplit` disjoint split unported (cmpregion.c:2472) | Plot/Grf backend | deferred |
| ACC-PARS | Geometry accessors (`CirclePars` etc.) unexposed | STC-S | deferred |
| CIR-NB3 | Circle `reg_base_box` calls `norm_box` only when `naxes == 2` (circle.rs:329), where C calls `astNormBox` for both the 2-D and the >2-D branch (circle.c:1100, outside the if/else); above two axes the port also takes an axis-offset path C does not have | — (latent; needs a >2-D Circle on a Frame with an override, i.e. a Circle on a CmpFrame(SkyFrame, ...) enclosing a pole, whose bounds C reports unwidened via the quirk in `docs/issues/c-library-quirks.md` and whose Frame-order variant C errors on) | medium |

## 2. Frames and Mappings

| Area | Doc | Status |
|---|---|---|
| Frame + Mapping class C-port gaps (function-by-function) | `docs/issues/frame-mapping-c-port-gaps.md` | OPEN — audit COMPLETE; remaining items are deferred delivery-step work only (Rebin/Resample/TranGrid/QuadApprox, JSON/XML/YAML renderers, absent classes, wcslib port 6a, Plot-support Frame methods + NormalPoints, SkyAxis Format precision) |
| Frame family C-parity audit (2026-10-05) | `docs/audits/frame-family-c-parity.md` | OPEN — 61 behavioural gaps (19 HIGH) across Frame, SkyFrame, SpecFrame, DSBSpecFrame, TimeFrame, FluxFrame, SpecFluxFrame, CmpFrame and FrameSet-as-Frame; most HIGH items oracle-probed; per-finding status tracked in the audit |
| Per-class Mapping performance vs C oracle | `docs/issues/mapping-perf-vs-c-oracle.md` | OPEN — **no automated perf gate exists**; `ast-oracle-bench` is a placeholder and CI runs no benchmark, so the >10% hot-path gate CLAUDE.md describes is not enforced. Snapshot (2026-05-19) records cheap mappings 5–42× slower than C at N=100k (permmap-fwd 42×, winmap 6.6–7.6×, shiftmap 5.7–7.6×, diag matrixmap 5.5×, slamap 3.9×, wcsmap-tan-fwd 4.2×); PolyMap/MatrixMap — the SCUBA-2 / LSST path — remain at or ahead of parity. Untested hypothesis worth measuring before per-class work: `PointSet` is row-major `(npoint, ncoord)` where C keeps one contiguous array per axis, which would explain the whole cheap-mapping cluster at once |
| `astTran*` definedness validation | — | OPEN — `Mapping::tran1`/`tran2` raise `AST__TRNND` for an undefined direction (C `ValidateMapping`, mapping.c:179), but the general `transform` / `transform_into` path raises it nowhere: `Trnnd` appeared in no source file before `156cf68`. Deciding which classes should report it needs a C-oracle probe per class, since C validates in the public `astTran*` entry points rather than in `astTransform` |
| CmpMap `inv_a`/`inv_b`: split the load contract from the dump-shaping emulation | `docs/issues/cmpmap-invert-snapshots.md` | DEFERRED to post-oracle-compliance — measured, with a falsifiable go/no-go test: after the §8 pin move, re-run the load-drift probe and collapse the mechanism only if it reports zero |
| TimeFrame port + Epoch/Equinox API | `docs/issues/timeframe-and-epoch-api-port.md` | OPEN (residual) — Issue A (DATE-OBS/TDB drift) RESOLVED; only Issue-B doc/hygiene items remain (Equinox=UTC note; `_year` setter public/private decision) |
| `NormUnit` | `crates/ast-core/tests/test_frame.rs` | CLOSED — `Frame::norm_unit`, with the SkyFrame override C has; write-up in [`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md) §2 |
| SkyFrame `Format`, `Digits`, `AsTime` and `Unit` | `crates/ast-core/tests/test_skyframe_format.rs` | CLOSED 2026-09-30 — the attributes, `DHmsFormat` / `DHmsUnit` / `ParseDHmsFormat`, the default-Format rules, dump/load and Overlay, pinned by 1800 formatted values, 120 Format/Unit/NormUnit triples and 14 byte-exact dumps captured from C by `tools/oracle_fixtures/skyframe_format_probe.c`. The Overlay port also copies Label and Symbol, which it did not before |
| A SkyAxis picked into a plain Frame is not a SkyAxis | `crates/ast-core/tests/test_picked_skyaxis.rs` | CLOSED 2026-09-30 — a plain Frame holds each axis as a plain `Axis` or a `SkyAxis` (`FrameAxis`), and SkyFrame's SubFrame attaches the picked SkyAxis with AsTime, Format, Label, Symbol, Unit, IsLatitude and CentreZero set as skyframe.c:10230 sets them, so the picked axis formats, unformats, normalises, reports its NormUnit and dumps as `Begin SkyAxis` as C's does. Pinned by 1820 values and 42 byte-exact dumps from `tools/oracle_fixtures/picked_skyaxis_probe.c`, including the `Format(1)=bhms` case (`3 49 11`, NormUnit `hh mm ss`) |
| SkyAxis `AxisUnformat` ignores the Format | `crates/ast-core/src/frames/skyaxis.rs` (`unformat_sky_axis`) | CLOSED 2026-09-30 — a port of skyaxis.c:4068: the separators, suffix field identifiers, angle-or-time inference and expected fields from `ParseDHmsFormat`, the 60-limit check and C's left-to-right accumulation; a SkyFrame reads with its own Format, set or default, as skyframe.c:10892 does. Pinned by the `Unformat` rows of `test_picked_skyaxis.rs`, which cover letter separators (`3h49m11s`, `45d30m`) |
| Plain Frame attributes | `crates/ast-core/tests/test_frame_attrs.rs` | CLOSED 2026-09-30 — every Frame attribute on BasicFrame (System, AlignSystem, ObsLat, ObsLon, ObsAlt, Dtai and Dut1 added; test/clear for all), C's defaults (Title, Label and Symbol from the internal axis, Format `%1.<Digits>G`), CleanDomain, the Epoch as an MJD, the axis permutation kept as C keeps it, the full Frame dump and the Axis card order, and Overlay, pinned by 1147 values and 30 byte-exact dumps from `tools/oracle_fixtures/frame_attrs_probe.c`. Phase 1 of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md`; phase 2 puts these attributes on the `Frame` trait |
| Frame-class attributes on the `Frame` trait | `crates/ast-core/tests/test_frame_class_attrs.rs` | CLOSED 2026-09-30 — Title, Domain, Epoch, Digits, ObsLat, ObsLon, ObsAlt, Dtai, Dut1, Ident, MinAxes, MaxAxes, MatchEnd, PreserveAxes, Permute and ActiveUnit, with set/test/clear, on every Frame class, a FrameSet and every Region, stored in the shared `FrameAttrs`; CmpFrame and SpecFluxFrame share Epoch and the observatory attributes with their components as C does. Pinned by 3584 values from `tools/oracle_fixtures/frame_class_attrs_probe.c`. Phase 2a of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md`; phase 2b puts the per-axis attributes on the trait |
| Per-axis attributes on the `Frame` trait | `crates/ast-core/tests/test_frame_axis_attrs.rs` | CLOSED 2026-09-30 — Label, Symbol, Unit, Format, Digits(axis), Direction, Top and Bottom, with set/test/clear, on every Frame class, a FrameSet and every Region, each taking a 1-based axis and returning `AstResult`. SpecFrame, TimeFrame and FluxFrame now hold an `Axis`, and `SkyAxis` wraps one. A CmpFrame routes to its components and lends them its Digits as cmpframe.c does, and reads Top and Bottom through the component's Axis object. Pinned by 4370 values from `tools/oracle_fixtures/frame_axis_attrs_probe.c`, which exposed wrong FluxFrame Symbols, a SpecFrame Symbol that fell back to the Label, a CmpFrame plain-component Symbol of `"Axis N"` rather than `"x<n>"`, a TimeFrame Format that ignored Digits, a FluxFrame dump without `Ax1`, and SpecFrame, TimeFrame and FluxFrame Overlays that copied only the Unit. Phase 2b of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md` |
| SkyFrame attributes | `crates/ast-core/tests/test_skyframe_attrs.rs` | CLOSED 2026-09-30 — System, AlignSystem, Equinox, Epoch, NegLon, Projection, SkyRef(axis), SkyRefIs, SkyRefP(axis), SkyTol, AlignOffset, AsTime(axis), IsLatAxis(axis), IsLonAxis(axis), LatAxis and LonAxis, with set/test/clear where C has them, and the SkyFrame defaults of Title, Label, Symbol, Unit, Format, Direction, Top and Bottom, pinned by 1548 values and 43 byte-exact dumps from `tools/oracle_fixtures/skyframe_attrs_probe.c`. It exposed a NegLon that ignored its offset-origin default, SkyRefP with no computed default, SkyRef taken as a (lon, lat) pair rather than per external axis (so `is_offset` missed a longitude-only SkyRef), NegLon and AlignOffset dumped when true rather than when set, SRef dumped for an unset axis, and a FITS-WCS reader that skipped C's CRVAL SkyRef for an offset SkyFrame behind a domain test C does not have. A SkyFrame's own SkyAxes have IsLatitude unset, as C's do; see `docs/issues/c-library-quirks.md`. Phase 3 of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md` |
| TimeFrame, SpecFrame, DSBSpecFrame and FluxFrame attributes | `crates/ast-core/tests/test_class_attrs.rs` | CLOSED 2026-09-30 — every class attribute (TimeScale, AlignTimeScale, TimeOrigin, LTOffset; StdOfRest, AlignStdOfRest, RestFreq, RefRA, RefDec, SourceVel, SourceVRF, SourceSys, SpecOrigin, AlignSpecOffset; DSBCentre, IF, SideBand, AlignSideBand, ImagFreq; SpecVal) and each class's System and AlignSystem, with get, set, test and clear, the getters giving C's defaults, and current-unit accessors where C's attribute is in the Frame's units. Each class writes the Frame part of its dump through the shared `write_frame_cards`, read through the `Frame` trait, as C's subclass `Dump` calls the Frame class's. Pinned by 811 values and 61 byte-exact dumps from `tools/oracle_fixtures/class_attrs_probe.c`, which exposed: SourceVRF and SourceSys that did not re-express SourceVel (`ConvertSourceVel` was ported only to heliocentric velocity), RestFreq, RefRA, RefDec, SourceVel and SpecOrigin with no default, a TimeFrame date label that lost its Local Time suffix, a FluxFrame label that ignored its unit-derived System, missing Ident, AlSys and Lbl1 cards, and a FluxFrame dump without most of the Frame part. Phase 4 of `docs/superpowers/plans/2026-09-30-frame-attribute-completeness.md` |
| SkyFrame and CmpFrame `Resolve`, `ResolvePoints` and `Intersect` | `crates/ast-core/tests/test_sky_geometry.rs` | CLOSED 2026-09-30 — SkyFrame resolves and intersects on great circles (skyframe.c:8321, `Intersect`) rather than inheriting the planar Frame versions, and a CmpFrame resolves each component's part of the vector and combines them (cmpframe.c:7753, :7962). Pinned bit for bit by `tools/oracle_fixtures/sky_geometry_probe.c`, permuted and not, with the bad and degenerate cases; C's `ResolvePoints` axis-order mix-up is fixed upstream on `u/timj/norm-unit` (5a2cac2a, past the submodule pin) and the port follows the fix, so the permuted `ResolvePoints` cases need that commit to agree (`docs/issues/c-library-quirks.md`). Still open from the audit: CmpFrame `IsUnitFrame`, UseDefs, and `astSkyOffsetMap`, `astGetRefPos` and `astCurrentTime` |
| `MatchAxes` on every Frame class | `crates/ast-core/tests/test_match_axes.rs` | CLOSED 2026-09-30 — the `MatchAxesX` overrides of SkyFrame (the whole SkyFrame is matched, skyframe.c:6977), CmpFrame (each component, then permuted, cmpframe.c:5444), FrameSet and every Region (the current Frame, frameset.c:7398, region.c:6413), and Region `Match` (region.c:6214). The base `MatchAxesX`, FrameSet `Match` and Region `Match` go through `astMatch`'s cast retry, and a leaf class cast into a plain Frame keeps every Frame-class attribute, as `astCastCopy` does. Pinned against C for all 169 pairs of 13 Frames by `tools/oracle_fixtures/match_axes_probe.c` |
| A non-linear Unit does not rewrite the default Label or Symbol | `crates/ast-core/src/units/label.rs` | CLOSED 2026-09-30 — `unit_mapper_label` returns the label `astUnitMapper` produces (unit.c:5349): the label parsed by a port of `MakeLabelTree`, joined to the units tree and printed in `MakeExp`'s label style, which drops scale factors, so `"Frequency"` with a Unit of `log(Hz)` becomes `"log( Frequency )"`. SpecFrame, TimeFrame and FluxFrame apply it to their default Label and Symbol when a Unit is set, as C's `GetLabel` and `GetSymbol` do. Pinned by the class attribute oracle, where it removes the space C's label rewrite trims from a TimeFrame label |
| FluxFrame has no UsedUnits and SetUnit does not clear an inconsistent System | `crates/ast-core/tests/test_class_attrs.rs` | CLOSED 2026-09-30 — FluxFrame records the Unit used with each System, clears the System for a Unit that does not suit it, re-instates the recorded Unit when the System changes (clearing Label, Symbol and Title, as fluxframe.c:2733 does), and dumps and reads the `U<system>` cards |
| An axis-qualified attribute string on a CmpFrame goes to the primary Frame | — | OPEN — `astGetC( cmp, "Label(2)" )` and `astSet` hand the attribute to the primary Frame (cmpframe.c `GetAttrib`, `SetAttrib`), skipping the CmpFrame's own `GetLabel` and its lent Digits, so the string and function paths differ (a plain Frame component's Label is `"Axis 1"` by string and `"Axis 2"` by function). The `Frame` trait mirrors the functions; the C-ABI attribute dispatch, which does not exist yet, must take the string route |
| A Region's own Ident is not dumped or loaded | `crates/ast-core/src/regions/data.rs` | OPEN — `RegionData::ident` is held and read through the trait, but the Region dump does not write an `Ident` card and the loader does not read one (C writes it through `astDump` of the Object class). Exposed by a Native round-trip of a Region with Ident set; no fixture has one |
| Unit scale factors and ZoomMap merging | `crates/ast-core/tests/test_unit_mapper.rs` | CLOSED 2026-10-05 — `astUnitMapper` factors for 70 unit pairs, SpecFrame unit changes in six Systems, and the simplified series CmpMap of two or three ZoomMaps with every combination of inversions, pinned bit for bit by `tools/oracle_fixtures/unit_mapper_probe.c`. Two divergences fixed: ZoomMap `MapMerge` multiplied by the reciprocal of an inverted neighbour's zoom where C divides by it (zoommap.c:762, :786), and SpecFrame `MakeSpecMapping` returned its chain unsimplified where C simplifies it (specframe.c:2972), so SpecFrame Hz→GHz gave 9.9999999999999986e-10 and VOPT m/s→km/h 3.5999999999999996 where C gives the nearest doubles to 1e-9 and 3.6 |

Closed (marked historical in-doc, removed as active work): `mapping-gaps-audit.md`
(superseded by `frame-mapping-c-port-gaps.md`), `wcsmap-naxis-refactor.md`
(landed), `timeframe-unformat-findings.md` (resolved 2026-06-25).

## 3. Containers

| Area | Doc | Status |
|---|---|---|
| KeyMap port | `docs/superpowers/specs/2026-08-06-keymap-port-design.md` | DONE — `crates/ast-core/src/keymap/`; entries in a `HashMap` with `MapSz` reconstructed at dump time; `testkeymap.c` green; C-ABI shim deferred with the rest of `ast-capi` |
| Table / FitsTable | — | UNPORTED — gates the `-TAB` branches in the two section-5 rows; `KeyMap` is shaped to be embedded by both |

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
| FitsChan C-port gaps | `docs/issues/fitschan-c-port-gaps.md` | PARTIALLY DONE — the read path is substantially complete: DSS (`enc_dss.rs`), SAO, DSBSetUp, SipReplace, `-ZPX`, DTAI, AXREF, Warn-card emission and `CarLin` (a real field with `set_car_lin`, honoured at `test_wcsconv.rs:246`) are all ported, and **all 194 wcsconv cases are byte-compared with `NATIVE_READ_EXCLUDED` empty**. What remains is the *attribute surface*, not the algorithms: eight attributes have no accessor at all (`Card`, `Clean`, `Iwc`, `PolyTan`, `FitsAxisOrder`, `IgnoreBadAlt`, `AltAxes`, `Warnings`) — `Card` is the substantive one, with 540 internal references to card position and no `set_card`/`get_card`, so a client cannot drive the cursor; two are blocked on FitsTable (`TabOK`, `ForceTab`); and about thirteen public functions are unported (`astPurgeWCS`, `astSetCard`/`astGetCard`, `astTestFits`, `astShowFits`, and the typed card accessors `astSetFitsCF`/`CI`/`CN`/`T`/`K`). `astRetainFits` has its foundations in the consumed-card work but is not ported. `-TAB` defers to the §3 FitsTable row |
| `fset_to_store` port gaps | `docs/issues/fset_to_store-port-gaps.md` | MOSTLY DONE — SIPIntWorld + VELOSYS + Group 3/5 branches ported. `FitsTol` and `SipOK` were plumbed-but-unreachable (fixed at `FITS_TOL_DEFAULT` and hardcoded `true`); both became reachable in the 9.5.0 sync — `FitsTol` has `set_fits_tol`/`fits_tol` accessors and `SipOK` is derived from the encoding at `fset_to_store/mod.rs:260`. `-TAB` (IsMapTab1D/2D) defers to the §3 FitsTable row rather than being the last step here |
| Round-trip harness misses trailing-whitespace divergences and the corpus has no comment-less nested-object card | `crates/ast-core/tests/test_serialisation.rs` | CLOSED 2026-09-04 — `native_round_trip_fixture` compared whole files with `trim()` and lines with `split_whitespace()`, so `"Frm1 = "` and `"Frm1 ="` compared equal. The whole-file path is now exact and the line walk splits on whitespace *keeping the separators*, so a difference in them is a difference; the 4-ULP tolerance covers numeric values and nothing else. The whole shared corpus passes under it, so the port's dumps are byte-exact including whitespace. The second half of the row is answered by the first: the comparator now normalises nothing, so there is nothing for a missing fixture to hide (the two files in the tree that do carry a `" ="` line, `programs/testyamlchan/lsst_wcs.txt` and `programs/joye_car_headers/total_hi.head`, are not native dumps and are read by their own programs) |
| `WcsFromStore` bounds its per-axis loops by a per-version axis count the port does not compute | `crates/ast-core/src/fitschan/wcs_write/mod.rs` | OPEN — C derives `nwcs = GetMaxJM( &(store->crpix), s, status ) + 1;` for each version letter when `WCSAXES` is unset (`fitschan.c:37313-37323`), and every CRPIX / CRVAL / CTYPE / CNAME / CD / CDELT / PC / CUNIT / AXREF / SREF / SREFP loop in that function runs `for( i = 0; i < nwcs; i++ )`. `GetMaxJM` (`fitschan.c:13237-13341`) walks `(*item)[si][i]` per intermediate axis, takes `astSizeOf(…)/sizeof(double) - 1`, then decrements past trailing `AST__BAD`, returning the max over axes and `-1` when nothing is stored. `PCFromStore` does the same at `fitschan.c:24755`. The port has no `nwcs` / `GetMaxJM` concept: it loops over `store.naxis`, which `FitsStore::new` fixes once as `max(base naxes, nwcs)` (`fset_to_store/mod.rs:161-162`). **What would expose it**: a FrameSet whose alternate description (a non-blank version letter) covers fewer axes than the primary one, so C's per-version count is smaller than `store.naxis` and C stops where the port keeps going. No fixture in the corpus has an alternate version with a different axis count, which is why the divergence is latent; adding one is the test. Investigated and ruled out as the cause of the grism write failure closed by the `is_bad` and `GrismMap` merge fixes |
| Comment-less nested-object card wrote a trailing space C never writes | — | FIXED (`1d2aa8f`, `86810ce`) — C's `WriteObject` (`channel.c:5106-5112`) appends `" ="` and adds the separator only when the comment is non-empty; the Rust Object arm pushed `" = "` unconditionally |

Cross-cutting blocker for the first two rows above: `-TAB` is not one algorithm
but three unported things — `Table`, `FitsTable`, and the algorithm itself
(`AdaptLut`, `PutTable`, `IsMapTab1D`/`2D`).
`crates/ast-core/src/` has no `table` module.
The §3 Table / FitsTable row is where that blocker is recorded; the two rows
above defer to it.

### Deferred C-oracle simplify fixtures

Twenty-four fixtures from upstream `ast_tester/simplify_fixtures/` are staged in
`crates/ast-core/tests/fixtures/native/reference/simplify_fixtures_unsupported/`
because they exercise classes the port has not implemented.
They are counted and enforced by
`crates/ast-core/tests/test_deferred_fixtures.rs`, which fails if a
subdirectory is added without a matching `DEFERRED_CLASSES` row.

| Subdirectory | Unported class | Files | Blocks |
|---|---|---|---|
| `intramap/` | `IntraMap` | 13 | 10 simplify cases (3 positive, 7 negative-control) |
| `xphmap/` | `XphMap` | 9 | 6 simplify cases (3 positive, 3 negative-control) |
| `stcresourceprofile/` | `StcResourceProfile` | 2 | 1 simplify case (`cap_timemap_02`) |

No wcsconv case is blocked on any of these classes.
The re-enabling procedure is in the directory's `README.md`.

Closed (marked historical in-doc, removed as active work):
`native-roundtrip-gaps.md` (24/24 + 139/139 byte-exact),
`native-roundtrip-simplify-gaps.md` (719/719 byte-exact),
`wcsconv-residual-classification.md` (wcsconv 192/192, all buckets cleared).

## 6. Cross-cutting

| Area | Doc | Status |
|---|---|---|
| C-library quirks mirrored in the port | `docs/issues/c-library-quirks.md` | running list — discuss with upstream |
| Whether `XmlChan` / `YamlChan` / `StcsChan` carry a `KeyMap` or a `Node` | `docs/superpowers/specs/2026-08-06-keymap-port-design.md` (Deferred decisions) | OPEN — decide per subclass when the first one is ported; `yamlchan.c` relies on `SortBy` ordering as algorithm state (`:9814`, `:11391`, `:11531`, `:12191`), which `Node` cannot express |
| Tiers 3, 4 and 5 are not run by CI | `.github/workflows/ci.yml` | OPEN — the design (step 0) calls for a nightly job running the submodule-dependent tiers. It does not exist: the workflow has a `fixture-mirror` job and a `test` job running tier-1 / tier-2 plus clippy, fmt and docs, and nothing on a schedule. So `ast-ffi-tests`, `ast-oracle-tests` and `ast-oracle-bench` are run by hand, which is also why the hot-path constraint has no gate (CLAUDE.md says so directly). Until a scheduled job exists, a tier-3/4 regression is caught only when someone runs it |

### Verification-posture ledger

Every way a gate in `ast-core` can be relaxed, counted so the total cannot grow
silently. **This table is mirrored by `crates/ast-core/tests/test_verification_ledger.rs`,
which is the executable copy**: it asserts each count and fails on any
`const *_EXCLUDED` / `_LICENSED` / `_SKIPPED` / `_UNGATED` / `_DISAGREEMENTS`
in the test tree that is not in its table. Adding a relaxation therefore means
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
| wcsconv (`test_wcsconv.rs`) | 195 | 12 | `astequal` compare mode — FMA contraction / libm differences vs x86-64 Linux captures that C AST on arm64 does not reproduce either |
| wcsconv | 195 | 3 | `ISSIMP_PLACEMENT_LICENSED` — drops the `IsSimp` tag lines both sides *and pins the card count on each side*, so it covers the tag placement and nothing else. A deliberate divergence, not a gap: C's placement here records the order its reader simplified in (§8). `axes-swap`, `vopt-f2w`, `wave-f2w`; C emits more cards than the port in each. 24 rows preceded these; 20 were pinned against references the pinned C no longer reproduced and were retired with the corpus refresh (§9.3), and `cd-cross` retired when the nominated-component branch stopped discarding the stamps its own simplify raised (§10.3) — its four cards now sit exactly where C's do, and `axes-swap`'s pin rose from two to three the same way |
| wcsconv | 195 | 0 | **No skip mode exists.** `NATIVE_READ_EXCLUDED` is empty and gated against refilling |
| simplify regression | 481 | 156 | `skip_string_compare` — upstream's own manifest column, now shared with its harness. 154 of these rows are negative fixtures, whose reference is the unsimplified input and so can never carry the `IsSimp` card `astSimplify` stamps; upstream gives up the whole dump for them and gates them in a separate `_noop` ctest family, while this harness compares them inline with `IsSimp` stripped (the `ignore_is_simp_line` row below). The two rows with a distinct reference, `cap_interval_01` and `cap_pointlist_01`, are platform-numeric cases upstream cannot license per card; this harness keeps comparing them because `NUMERIC_PLATFORM_DRIFT` licenses the one drifting value each. The port's own column was empty and gated before the manifests merged, and no row here is a port opt-out
| simplify regression | 481 | 4 | `NUMERIC_PLATFORM_DRIFT` — `cap_interval_01`'s `X5` / `X6`, the two sky half-widths of its uncertainty Box, `2.4240684055495722e-06` and `2.4240684055430873e-06` in the reference against `2.4240684055476816e-06` from both the port and the pinned C here; and `cap_pointlist_01`'s `X5` / `X6`, the sky half of its merged uncertainty Box's corner, `3.1241551949160478` and `-0.20069734795308758` in the reference against `3.1241551949160575` and `-0.20069734795284888` here. Numeric drift, not a port gap: the references were captured on arm64, and the corpus-freshness rule keeps such a reference rather than replacing it with an x86-64 recapture. `cap_interval_01`'s two agree with the reference to twelve significant figures and are nominally equal to each other, which this machine's C makes them and that platform's did not; `cap_pointlist_01`'s two are what `astSetUnc` regenerates by re-centring the Box and re-offsetting its corner through a `CmpFrame(TimeFrame, SkyFrame)`, so they carry that round trip's `atan2` / `asin` pair. Each licence names the fixture, the card and the reference's value exactly, and bounds how far the port's own value may sit from it: the reference side is fixed bytes and is matched as written, while the port's side is matched by distance, so the licence fires on an arm64 build — where the port reproduces the reference exactly — as well as here. Exactly one line must drop from each side, which is what stops `cap_interval_01`'s exact-zero `X5` going with it. `rigby`'s `IM5` was the fifth entry and is retired: the `astEQUAL`-zero snap upstream added to `astMapMerge` (§9.4) removed the cancellation residue, and the regenerated reference now matches the port exactly |
| simplify regression | 481 | 4 | `ABANDONED_PROBE_STAMP_LICENSED` — one reference-only `IsSimp = 1` line each in `rigby`, `cap_specmap_02`, `cap_cmpmap_01` and `cap_cmpmap_04`. `cap_cmpmap_nested_tag` was the fifth and is retired: the port reproduces its dump byte for byte at the current pin. C stamped a sub-Mapping inside a merge probe it then abandoned: `CombineMaps` assembles the trial CmpMaps by cloning (`cmpmap.c:626-627`), so a trial holds the very sub-Mappings the live tree holds, the recursion stamps whatever `astSimplify` returns (`cmpmap.c:1443`), and discarding the probe (`cmpmap.c:1621-1628`) leaves the mark behind. The neighbouring parallel-in-series probe guards against exactly this with `SeparateMappings` (`cmpmap.c:1650-1651`); this one does not. Owned values give each tree its own children, so no probe in the port can leave a mark that outlives it (`docs/issues/c-library-quirks.md`, "An `IsSimp` card can record a merge probe C threw away"). Spent positionally — a line counts only where the port's line is the reference's *next* line — one per fixture, required to be found, with every other line compared |
| simplify regression | 481 | 2 | `INVAB_ALIASING_LICENSED` — one node in each of `cap_cmpmap_01` and `cap_cmpmap_04` whose dump carries `Invert = 1` in C and `IsSimp = 1` in the port. C's `CombineMaps` records a shared child's `Invert` on the CmpMap it builds, so the pair the dump carries is reference-count history rather than a property of the Mapping (`docs/issues/c-library-quirks.md`, "CmpMap dumps InvA/InvB disagreeing with the child's own Invert"); the port holds components by value and has no such history. The two cards are consumed as a pair at one position, so neither can go missing elsewhere unnoticed. This is what retired both fixtures' `skip_string_compare`, which had been hiding the whole 350-line file. Preceded by `ISSIMP_PLACEMENT_LICENSED`, which read `cap_specmap_02`'s missing card as a CmpMap the port never forms (§10.3): the port forms it byte for byte, and the card was the abandoned probe's |
| simplify regression | 481 | 2 | `ISSIMP_DISAGREEMENTS` — fixtures whose `IsSimp` card count still differs from C. Both under-tag by exactly one card, and that card is one C wrote during a merge probe it then abandoned: the probe's CmpMaps are assembled by cloning, so they share their sub-Mappings with the live tree and the stamp outlives the probe (`docs/issues/c-library-quirks.md`, "An `IsSimp` card can record a merge probe C threw away"). Owned values give each tree its own children, so the port cannot reproduce it — `rigby` and `cap_specmap_02`. Retired at the current pin: `cap_cmpmap_nested_tag`, shrunk from `cap_specmap_02` to isolate the signature, whose dump the port now reproduces exactly. Retired with this cluster: `cap_zoommap_04`, which over-tagged by three because its uncertainty simplification was being discarded (§10.23); `lsst`, when `PolyMap`'s linear branch began composing its MatrixMap and ShiftMap and handing the pair to `astSimplify` as C does, and `cap_cmpmap_01` / `cap_cmpmap_04`, when `CmpMap::map_merge` began adopting the object its own self-simplify returns unchanged — C's `new == map`, which carries the recursion's descendant stamps forward (§10.3). Retired earlier: `cefit_circle_conformal` and `cefit_circle_nonconformal_keep` when `from_node` began routing through the setters that discard the record, as C's loaders do (§9.2); the three `cmpmap_perm_swap*` fixtures when the swap branch stopped simplifying the components C hands on as they stand; `cap_cmpmap_03` when the same branch stopped simplifying the parallel CmpMap it reassembles; and `parpair_one_side_reduces` when it stopped simplifying a dimension-matched sublist's members (all §10.3). The byte comparison licenses both through `ABANDONED_PROBE_STAMP_LICENSED`, so the count is their only gate. Listed because otherwise the number is invisible, and asserted as a set in `test_simplify_regression.rs` so it cannot grow. A stale reference is ruled out per fixture by `tools/oracle_fixtures/check_simplify_fresh.sh`, which recaptures the corpus from the pinned C |
| simplify regression | 481 | 158 | `ignore_is_simp_line` for the negative fixtures, whose reference *is* their input and so cannot carry the tag `simplify` always stamps. The companion assertion requires a top-level card exactly when the result is not Frame-kind — C's `IsSimple` getter is overridden on the Frame class to return zero (`frame.c:5590`) — so the strip can hide neither an untagged Mapping result nor a wrongly tagged Region. 134 with `neg_time_mjdtojd_real` (§10.9), `neg_unit_parallel_forward` (§10.6) `neg_parpair_aligned_irreducible` (§10.14), `neg_lut_near_linear` (§10.8), and five upstream repaired at pin `8d4f526f`. Was 141 before `cap_box_permmap_nonbidi`, the negative control for a PermMap whose permutation is not bi-directional (§10.13), and 142 before `neg_wcsmap_perm_allconst` and `neg_wcsmap_perm_both_sides`, the two controls on C's WcsMap swap decision (§10.31) |
| simplify regression | 481 | 0 | `SERIAL_UNGATED` — empty. Every fixture's simplified dump is byte-compared against C's `.simp` in full, and any difference fails. The table survives so a newly diagnosed C-side artefact has a home, and the harness asserts the set exactly, so a row cannot appear without a diagnosis. All seven causes enumerated in the table below are closed: A (SwitchMap `IsSimp` on FSMap / ISMap), B (a card on a Mapping inside a Region's FrameSet, §9.2), C (`nullregion_unc_fillfactor_merge`'s `Prism` uncertainty), D (`cap_zoommap_04`'s `FrmSet` block, §10.23), E (`cap_interval_01`'s corner point, §10.25), F (`cap_pointlist_01`'s uncertainty corner, §10.26) and G (`parpair_one_side_reduces`'s extra `IsSimp` cards, §10.3) |
| simplify round-trip (`test_serialisation.rs`) | 823 | 0 | **No relaxation exists.** Every `.map` / `.simp` in the corpus is read, written and byte-compared against itself, and any difference fails. The 43 `.rt` references are gone with the defect that needed them: C's subclass loaders re-applied a validated attribute through a clear-on-set setter, discarding the `IsSimp` card `astLoadMapping` had just installed, so reading a dump was not the identity. Upstream `07d0cd2d` guards each clear on the value actually differing and the port mirrors it (§9.4), so a fixture is now its own round-trip reference. In place of `.rt` the comparison drops the leading run of comment lines from both sides, as upstream's driver does: a hand-written header saying what a fixture is for is documentation, not part of the object. Was 4 under `ROUNDTRIP_DISCARD_LICENSED`, two GrismMap and two PcdMap fixtures whose reference dropped a card the port kept
| transform oracle | 2261 sections | 0 | all three `*_ORACLE_EXCLUDED` lists are empty |
| transform oracle | 2261 sections | 3 | `transform_oracle_overrides.txt` — two round-trip skips and one golden row list, asserted as an allowlist by `parses_committed_overrides_file` rather than by an emptiness check, so a fourth cannot appear unnoticed. Both round-trip entries are `tnx-cheb`, reached once as the header and once as its dump under `wcsconv/framesets/`: its TNX Chebyshev distortion builds a ChebyMap, which cannot represent its own Jacobian, so sky->pixel is BAD throughout and there is no round trip to assert. The golden entry names rows 2 and 3 of one `neg_unitnormmap_inv_fwd_diffcentre` inverse section — both the all-zero sample, where a zero norm leaves the forward direction unconstrained and the reconstruction's value depends on whether the compiler contracts its multiply-add. Upstream narrowed this from `golden-inv off` to the two rows at the 9.5.0 sync, so the rest of that section is now pinned; the row-list form is preferred over a whole-direction `off` for exactly that reason. A loosened round-trip *tolerance* remains impossible: a test requires every round-trip entry to be a disabled one |
| transform oracle | 25 rows | 5 | `RTRIP_REFERENCE_GAPS_LICENSED` — rows where C's own recorded inverse does not recover the pixel either. Rekeyed to full fixture paths when the corpus moved to the shared tree, which is what raised the count from two entries to five: the same cases, addressed by the path the oracle records rather than by basename. Asserted as an exact set against the corpus with no fixture loaded, and reached only after the live result has failed both the direct and the aliased comparison |
| transform oracle | 2 rows | 0 | `FMA_SUBSTITUTIONS_LICENSED` — empty. It held two rows of one UnitNormMap inverse section where the generator host's compiler fused the multiply-add at `unitnormmap.c:1079` and stepped past C's own zero-norm branch. The regenerated oracle records the unfused values, and the port reproduces them because it builds without contraction; the table survives because that is a build setting, and a contracting Rust build would need it back (§9.4). Upstream reached the opposite conclusion about which value is right — the fused one is the correctly-rounded evaluation, and the true displacement is 2.58e-16 on both axes rather than zero — so it skips those two rows outright rather than pinning either. The port keeps them skipped via the overrides row above; substituting the value C's source prescribes would pin the answer upstream declined to pin
| frameset round-trip | 139 | 0 | `FRAMESET_FIXTURE_EXCLUDED` is empty — every wcsconv object round-trips byte-exact |
| deferred fixtures | — | 3 | `UNPORTED_CLASSES` — `IntraMap`, `XphMap`, `StcResourceProfile` are unported classes, not gates that could otherwise pass (§5). The fixtures used to sit in per-class subdirectories both consumers skipped by not recursing; the tree is now shared with upstream, whose harness reads one flat directory, so each consumer names them instead |
| transform oracle | 2261 sections | 20 | `DEFERRED_LOAD` — manifest rows whose fixture holds an unported class, so the oracle's sections for them cannot be checked. Not a tolerance: the reader cannot build the Mapping at all. `deferred_fixtures_still_fail_to_load` fails when one starts loading, so a class landing cannot leave its coverage unclaimed |
| simplify regression | 481 | 0 | `C_DEFECT_EXCLUDED` — empty. It held `cheby_linear_lone_reduce`, whose reference recorded a C defect rather than C's intent: PolyMap's linear `MapMerge` reduction read a ChebyMap's `coeff_f` / `power_f` without the scale and offset that map its input onto [-1,1], so `astSimplify` returned a `ZoomMap` of 2 where the ChebyMap computes `0.4x-2`, and reproducing the reference would have meant porting the defect. Upstream fixed it in `abe5baae`, the port ported the fix (§10.2), and the row is compared byte for byte like any other. `c_defect_rows_still_diverge` is what reported the fix: it fails when an excluded row starts matching, so the row could not stay excluded unnoticed. The table survives empty so a newly diagnosed C-side artefact has a home |
| simplify regression | 481 | 1 | `astequal` compare mode — upstream's own manifest column: one row, `cap_timemap_02`, whose uncertainty coordinates move by about 7e-12 relative under FMA contraction. The port never reaches it (`StcResourceProfile` is unported, so it is a `DEFERRED` row), but the comparator honours the column so the two harnesses cannot diverge on what a row asks for |
| round-trip, serialisation + programs (`test_serialisation.rs`) | 33 | 20 | `ROUND_TRIP_EXCLUDED` — nineteen fixtures holding an unported class, the same set as `DEFERRED_LOAD` counted per file rather than per manifest row, plus one port gap, named with what closes it: `serialisation/frame_in_map_slot.ast`, a Frame where a Mapping belongs, which the Mapping loader has no case for (§9.6). `programs/testfitschan/sparse.ast` left the list on 2026-09-30, once SkyAxis Format specifiers were parsed (phase 3 of the Frame attribute plan) and CmpFrame wrote the Frame part of its dump through its own getters. `round_trip_exclusions_still_fail` fails when an entry starts passing. This sweep replaced three skip modes: a missing fixture directory returned early with a notice, a fixture that would not load was passed over on a substring match against the error text, and the failure list itself was filtered by substring, so every Frame, FrameSet, `lsst`, `degen` and `splittest` fixture could fail silently |
| sky conversion oracle (`test_sky_convert.rs`) | 1584 | 3 | `SINCOS_MERGE_LICENSED` — three AZEL cases allowed a measured 1-5 units in the last place: Apple clang merges the `sin`/`cos` pairs of slamap.c `De2h` into `__sincos_stret`, which an unoptimised Rust build does not. A release build matches C exactly, and the companion test asserts that in both build modes |
| Frame in a Mapping slot (`test_frame_in_map_slot.rs`) | 1 | 1 | `FRAME_IN_MAP_SLOT_UNDECIDED` — one C-written dump the reader refuses, a bare Frame in a `Map<n>` slot. Whether to support the construct is an open design question, stated with its artifact in `docs/issues/frame-in-a-mapping-slot.md` (§9.6). The exclusion carries the reason string, and two companion tests keep the refusal narrow: the settled Region-in-`CmpMap` and Region-in-`CmpFrame` nestings must keep reading |

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
| `ast-capi` C-ABI shim | UNSTARTED — `crates/ast-capi/src/lib.rs` is a one-line comment; 0 exported symbols, no `ast.h`, no `astClone`/`astAnnul` refcounting, no `astBegin`/`astEnd` handle stack, no thread-local status, no variadic `astSet` parsing, no `catch_unwind` boundary, and neither of the two CI header-drift checks exists | tiers 3 and 4 below; the drop-in-replacement goal (pyast / Starlink / DS9 linking unchanged) |
| `ast-capi` immutability enforcement (`AST__IMMUT`) | OPEN, design decision — see the note below | `ast-capi`; drop-in compatibility for callers that mutate a shared Mapping |
| Tier 3 — `ast-ffi-tests` | UNSTARTED — crate body is `#![allow(missing_docs)]`. Would run the real C `ast_tester` programs against the Rust `libast` | blocked on `ast-capi` |
| Tier 4 — `ast-oracle-tests` | UNSTARTED — same. Would link `ast-core` + reference `libast.a` for live bit/ULP comparison. **No test in the repo currently executes C code and compares live**; all C comparison is against frozen committed corpora | blocked on `ast-capi` |
| Tier 5 — `ast-oracle-bench` | UNSTARTED — 6-line criterion placeholder; see the perf row in §2 | — |
| `ast-plugins` (Grf / Grf3d backends) | UNSTARTED — one-line stub | `Plot` / `Plot3D`, and the `RegTrace` / `RegSplit` region deferrals in §1 |
| wcslib Rust port (step 6a) | UNSTARTED — `vendor/wcslib` (3,327 code lines) still compiled and linked, alongside ERFA (16,626) and PAL (1,386 + a 234-line `palwrap.c`) | — |

### `AST__IMMUT` at the shim boundary

C enforces, in every class as of the pending `u/timj/misc-bugs` merge, that a
Mapping which has been cloned or included in another Object may not have its
attributes changed — `Invert` excepted. `ast.news` files this under "may break
existing code": callers that previously changed `PolyMap.IterInverse`,
`MathMap.Seed` or `SplineMap.InvTol` through a shared pointer succeeded
silently and now get `AST__IMMUT`.

**None of this can arise in `ast-core`**, for three independent reasons: `Clone`
on `Box<dyn Mapping>` is a deep copy that recurses through compound classes;
`DynMapping` is single-owner and compound constructors consume their arguments;
and component accessors are `pub(crate)`, so a component is unreachable from
outside the crate. `ErrorCode::Immut` is generated from `ast_err.msg` and is
correctly unused. The rationale is recorded in `crates/ast-core/src/error.rs`'s
module docs so the unused variant is not mistaken for an omission.

**`ast-capi` restores the hazard by design** and has to decide how to police it.
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
  shared *and* carry an immutability flag per slot — state `ast-core` has no
  concept of, which means it belongs in the registry rather than in the object.

Deciding this before the shim is written is much cheaper than retrofitting it,
because it constrains the registry's slot representation.

Absent classes, for completeness (52,168 code lines of C, 28% of the library by
code volume, with no Rust counterpart): `Plot` / `Plot3D` + `grf*`, `YamlChan`,
`XmlChan`, `Moc` / `MocChan`, `StcsChan`, all `Stc*` regions, `Table` /
`FitsTable`, `IntraMap`, `XphMap`. Absent `Mapping`-level algorithms: `astResample`,
`astRebin`, `astRebinSeq`, `astTranGrid`, `astQuadApprox`. Of these only
`Table` / `FitsTable` (§3, §5) and `IntraMap` / `XphMap` (§5 deferred fixtures)
currently gate active work.

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
`tools/fixtures/check_mirror.sh` diffs `crates/ast-core/tests/fixtures/` against
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
| testconvert.c | crates/ast-core/tests/test_convert.rs | ported 2026-06-12 |
| testframe.c | crates/ast-core/tests/test_frame.rs | ported 2026-09-30, the SkyFrame `Format` / `Unit` block included; the `astAxAngle` block is in `test_frame_geometry.rs`. `testframe.c`'s newer picked-axis block is not ported: it needs the picked-SkyAxis row in §2. The file also pins 46 Unit / NormUnit pairs captured from the C library; the three `unit.c` defects they exposed are fixed upstream on `u/timj/norm-unit` (`965b7587`, `25bd4924`, `8c734154`) and in the port, and are under "Resolved upstream" in `docs/issues/c-library-quirks.md`. `test_convert.rs` gained the matching `testconvert.c` Errors 52-53 |
| check_transform_oracle | crates/ast-core/tests/test_transform_oracle.rs | ported 2026-08-05 (submodule `3a3c3e51`); consumes `tests/fixtures/oracle/*.oracle`; all 2261 sections gated by three asserting tests (`head_`, `slamap_`, `simplify_sections_match_oracle`), with `every_oracle_section_is_gated` proving the partition covers the corpus; `transform_oracle_report` remains `#[ignore]` as the investigative view; `gen_transform_oracle` intentionally not ported |
| testcmpframe.c | crates/ast-core/tests/test_cmpframe.rs | ported 2026-04-27 |
| testflux.c | crates/ast-core/tests/test_flux.rs | ported 2026-09-30, including the `SFCBRW` read-back check added upstream on `u/timj/norm-unit` (`b4457da9`); porting it needed the FluxFrame default SpecVal (`DfSpc`) and the full FluxFrame Overlay |
| testframeset.c | crates/ast-core/tests/test_frameset.rs | ported 2026-05-01 |
| testskyframe.c | crates/ast-core/tests/test_skyframe.rs | ported 2026-05-02 |
| testspecflux.c | crates/ast-core/tests/test_specflux.rs | ported 2026-09-30 |
| testspecframe.c | crates/ast-core/tests/test_specframe.rs | ported 2026-05-06 |
| testtime.c | crates/ast-core/tests/test_timeframe.rs | ported 2026-05-06 |
| testkeymap.c | crates/ast-core/tests/test_keymap.rs | ported 2026-08-08; the nine functions added upstream at `f6762005` ported 2026-08-20 |
| testmapping.c | crates/ast-core/tests/test_mapping.rs | partially ported 2026-08-20 — the four `IsSimple` functions added upstream at `f6762005`. `testselfsimplifyafterset` is unconstructible here (ZoomMap's `Zoom` and PcdMap's `Disco` have no `&mut self` setter). The file's original PolyMap / LinearApprox body, converted from `testmapping.f`, is covered by `test_polymap.rs` and `test_polytran.rs` |
| testfitschan.c | crates/ast-core/tests/test_fitschan*.rs | partially ported — `testgrismorder`, added upstream at `f6762005`, is in `test_fitschan_wcs_write.rs` (2026-08-20); the `905`-`912` SIP-validation cases, added upstream at `c384289f` and `121d12d2`, are in the same file (2026-09-24); the rest is spread across the seven `test_fitschan_*` files. **`testtabspectrum` (`testfitschan.c:256`, called at `:1444`, added upstream at `39def937`) is not ported**: it writes a non-linear spectral axis as `-TAB`, which needs the `Table` / `FitsTable` / algorithm trio in the §3 row. It is the case that first exposed the `astEQUALS` overflow upstream; that fix is landed and gated by the other `39def937` cases, so what this row owes is the end-to-end `-TAB` path, not the arithmetic |
| testimmutable.c | (deferred) | tests `AST__IMMUT`, which guards a change made to a Mapping whose reference count exceeds one. Owned values, deep `Clone` and single-owner `Box` make that condition unconstructible in `ast-core`; the guard belongs to `ast-capi`, which restores sharing at the handle boundary and is unstarted (§7) |
| gen_simplify_fixtures / check_simplify | crates/ast-core/tests/test_simplify_regression.rs | ported; 481 of the 498 rows in the shared manifest against the pinned oracle's `.map` / `.simp` dumps, plus `test_serialisation`'s 823-file round trip over the same corpus. The 17 rows not run are named, all of them in `DEFERRED` (unported class), each with a companion test that fails when it starts working; `C_DEFECT_EXCLUDED` is empty. `gen_simplify_fixtures` itself stays in C: it writes the references |
| (no C ancestor) | crates/ast-core/tests/test_frame_in_map_slot.rs | added 2026-09-03; pins the reader's refusal of a C-written bare Frame in a `Map<n>` slot, with the settled Region nestings as controls (§9.6) |
| roundtrip.c | crates/ast-core/tests/test_serialisation.rs | ported 2026-09-04 as three sweeps covering the same file set upstream's `roundtrip_*` ctests glob: `simplify/*.{map,simp}`, `serialisation/*.ast`, `programs/**/*.ast` and `wcsconv/framesets/*.ast`. `wcsconv/` itself is excluded on both sides: its references are FITS-encoded, so reading one back needs a FitsChan and the encoding it was written with, which the wcsconv harness already does |
| testissimp.c | crates/ast-core/tests/test_clear_on_set.rs | ported 2026-09-04. Upstream built `testissimp.c` to cover the clear-on-set rule across all ten affected classes rather than the four the fixture corpus happens to reach; the Rust file already had a case per setter, and `load_preserves_the_record_for_every_clear_on_set_class` is the read-back half. Upstream needs `astCLASS` for `PermMap`'s `PermSplit` and `WcsMap`'s `FITSProj` / `LonCheck`, which are absent from those classes' `SetAttrib`; the port exposes all three as typed setters, so no equivalent is needed |
| compare_dumps.c | (not ported) | the comparator upstream's fixture-driven ctests use: token-wise with a 4-ULP budget and an `--astequal` mode. `tests/common/mod.rs` is the Rust equivalent and predates it (`native_strings_equivalent`, `native_strings_astequal`, `ast_equal_close`); the two were reconciled during the sync, which is where C's missing `1e-12` absolute floor and the port's invented `astEQUAL` tolerance were both found |
| testxphmap.c | (not ported) | a 219-line LeakSanitizer gate added upstream at the 9.5.0 pin. `XphMap` does not exist here (§5 deferred fixtures), and upstream itself withdrew the leak-detecting registration at `d8e0414e` because LeakSanitizer is unavailable on Darwin |
| testobjectkeymap.c | (deferred) | tests the Object-associated retained KeyMap (`object.c:1736`, `:2480`), `astGetObjSize` and `astSame`; those calls (`astMapPut0I`, `astMapGet0I`, `astMapSize`) are only reachable through the protected `astGetKeyMap`/`astHasKeyMap` machinery, an explicit spec Non-goal of the owned-value port |

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
files in `crates/ast-core/tests/fixtures/simplify/`, nine carry no oracle
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

### 9.6 A bare Frame where a Mapping is expected — OPEN, awaiting a maintainer decision

C accepts a Frame in any Mapping slot, because `Frame::Transform`
(`frame.c:11739`) transforms through a UnitMap (`frame.c:11806`).
This port makes Region the only Frame that is also a Mapping, so a `Map<n>` slot
holding `Begin Frame` or `Begin CmpFrame` is refused.

The narrow question — whether C's acceptance is a side effect of having made
Frame a Mapping in order to support Region, or answers a real use case — is
written up with its C citations, the cost of each answer, and the evidence
available in
[`docs/issues/frame-in-a-mapping-slot.md`](docs/issues/frame-in-a-mapping-slot.md).

The artifact is `crates/ast-core/tests/fixtures/frame_in_map_slot.ast`, a dump
the C library wrote, reproducible with
`tools/oracle_fixtures/frame_in_map_slot_probe.c`.
`crates/ast-core/tests/test_frame_in_map_slot.rs` pins the refusal and keeps it
narrow: the 33 Region-in-`CmpMap` nestings in the fixture corpus and the
Region-in-`CmpFrame` dump the same probe wrote must keep reading, so the gap
cannot be closed by refusing Frames in Mapping slots wholesale.

`C_SIMPLIFY_SEAMS` records `cmpframe.c` as
`undecided: docs/issues/frame-in-a-mapping-slot.md`, and requires that document
to state the question and name the fixture; `FRAME_IN_MAP_SLOT_UNDECIDED` counts
the one fixture in §6.

`CmpFrame::Simplify` (`cmpframe.c:8963-9050`, installed at `:4745`) is the seam
the decision governs. It calls `astSimplify` on each component Frame and, if
either returned a different object and both are still Frames, returns a copy of
the CmpFrame holding the simplified components.
`crates/ast-core/src/frames/cmpframe.rs` has no counterpart and cannot have one
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
One carries an open remainder and stays here in full: §10.22's
`normalize_unitmap_children`.

What is left open anywhere in this section:

- **§9.2, for five attributes with no setter** — `ZoomMap::Zoom`,
  `LutMap::LutEpsilon`, `PcdMap::Disco`, `IntraMap::IntraFlag` and GrismMap's
  macro-form attributes. The convention is ported for every attribute the port
  can mutate; these have nothing to attach it to yet.

| § | Gap | Resolution |
|---|---|---|
| 10.1 | RateMap's self-reduction sat in `simplified()`, unreachable from the nominate loop | FIXED — `map_merge` uses `self_reduce_map_merge`, which is C's order: reduce first, then cancel against an inverse neighbour |
| 10.2 | ChebyMap reached neither half of the `MapMerge` it inherits from PolyMap | FIXED — both halves run; upstream's `abe5baae` fixed the linear reduction's lost normalisation and the port ported it, retiring its one `C_DEFECT_EXCLUDED` row |
| 10.3 | Three divergences in the port's handling of C's parallel-in-series branch | FIXED — rows 22, 23 and 26 of `cmpmap.md` are `ported`, and two fixtures came byte-exact with the change |
| 10.4 | The CmpMap/PermMap swap was hung off the PermMap, where C hangs it off the CmpMap | FIXED — it runs when the CmpMap is nominated, as the last block of its `map_merge` |
| 10.6 | A parallel UnitMap's Invert flag looked unported | RETRACTED — C clears the list's applied direction, not the object's attribute, which the port does elsewhere |
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
| 10.23 | The uncertainty simplification asked `Mapping::equal` where C asks pointer identity | FIXED — any reduction counts, including one that rewrites only the structure |
| 10.24 | A merged Region's combined uncertainty went straight to the parallel CmpMap `Prism::Simplify` starts from | FIXED — the Prism's tail contributes, so the uncertainty is built as C builds it |
| 10.25 | A bounded Interval was reclassified to a Box without simplifying it | FIXED — C's `Cache` builds the base-Frame Box and the result is simplified |
| 10.26 | `MergePointList` installed its combined uncertainty by assignment | FIXED — installed through `set_unc`, as C's shared merge tail does |
| 10.27 | PcdMap's swap half was a sketch of C's | FIXED — five divergences closed, including the speculative swap with no merge target in reach |
| 10.28 | LutMap's linearity test and merge tail diverged from C's in four places | FIXED — nearest-neighbour tables refused up front, and the tail matches C |
| 10.29 | PolyMap's merge half, and a third `astDoNotSimplify` site in the shared dispatcher | FIXED — three divergences closed |
| 10.30 | ChebyMap's cancellation test was hand-rolled and never looked at the inverse terms | FIXED — it goes through the inherited equality, bounding boxes and both coefficient sets |
| 10.31 | WcsMap's swap driver was two attempts where C runs a search | FIXED — C's bidirectional PermMap walk, nearer-target choice and speculative swap are ported |

### 10.22 The public entry performs one tree rewrite C's `astSimplify_` does not

`mapping.c:24755-24808` adds no algorithm: an `IsSimple` short-circuit, the
`Simplify` call, the stamp, and the two flag clears of §10.20.
`<dyn Mapping>::simplify` (`simplify_entry.rs:36-82`) also rewrites the result
tree once before returning it.

`normalize_unitmap_children` (`cmpmap.rs:223`, called at `simplify_entry.rs:70`)
drops redundant `InvA`/`InvB` flags on a parallel CmpMap whose corresponding
child is a UnitMap. It is load-bearing and measured: §10.15 records four
`equal()` failures and two serialisation failures without it, and it is one of
the two layers that reproduce C's inversion push-down. The section stays open for
it.

A second rewrite, `narrow_unit_scale_winmap_children`, was measured dead and
deleted; the account is in
[`docs/issues/closed-plan-findings.md`](docs/issues/closed-plan-findings.md).

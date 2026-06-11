# Frame and Mapping classes: C-vs-Rust port gaps

Status: audit COMPLETE (2026-06-10).
All twelve class groups in the section 5 work queue have been compared function-by-function against the C reference; detailed findings are in sections 6-6k.
Every item was verified by reading the code at the cited locations.

## 0. Prioritized fix order

Work top to bottom; each entry cites the detailed section.

1. **Transform-definedness flags** (6g, 6h, 6i) — **FIXED 2026-06-10**: CmpMap/TranMap now derive `tran_forward`/`tran_inverse` from their components, PolyMap/ChebyMap/MatrixMap override `has_forward`/`has_inverse`, and native dumps mirror C by writing `Fwd`/`Inv` from constructor-time flags via `Mapping::dump_tran_flags` (only CmpMap, RateMap, SelectorMap, SwitchMap, LutMap, MathMap pass computed values in C).
   Tests: `tests/test_tran_definedness.rs`.
2. **PolyMap inverse priority + `astPolyTran`** (6h, section 1) — **FIXED 2026-06-10**, including fitschan SIP adoption: the fit is verified bit-compatible with cminpack against a built reference libast, the previously-unknown C `IterInverse` default (`ncoeff_i == 0`, polymap.c:6311) is ported, and the FitsChan `SipReplace` attribute (default true) now drives the C PolyTran call in `wcs_matrix.rs`.
   One deliberate deferral: ChebyMap keeps the iterative-inverse default disabled because the Rust Newton-Raphson machinery is power-basis-specific (C iterates through the virtual astTransform); noted in `chebymap.rs`.
3. **FrameSet repair** (6b) — **FIXED 2026-06-11**: `remove_frame` keeps the node as an anonymous junction and tidies (dead-end pruning added to `tidy_nodes`); the Frame impl now forwards domain/ident/match-control attributes, `match_template`, `sub_frame`, `pick_axes_snapshot`, and `overlay_template` to the current frame; `simplified()` returns a FrameSet copy with per-edge simplification; Current resets to the last frame on removal; `has_forward`/`has_inverse`/`rate` delegate to the base→current mapping.
   Tests: `tests/test_frameset_repair.rs`. Still open from 6b: variants stub, `convert` member-frame iteration (blocked on ConvertX), `map_split`/`RemoveRegions` overrides, AST__ALLFRAMES.
4. **SkyFrame `angle` + SkyTol** (6d) — **FIXED 2026-06-11**: `angle` now uses vertex bearings with the signed difference, the permuted-axes π/2 correction, and AST__BAD for coincident points (oracle-matched against the built C library); SkyTol defaults to 0.001 arcsec and the TestSlaUnit gate (14-probe-point sphere grid) now replaces sub-tolerance conversion chains with a UnitMap at all three mapping-construction sites.
   Correction to the original finding: the ICRS↔FK5 frame bias (~23 mas) *exceeds* the default 1 mas tolerance, so that chain correctly survives at defaults (C-verified); the gate fires for genuinely sub-tolerance alignments or raised SkyTol.
5. **CmpFrame geometry + defaults** (6c) — **FIXED 2026-06-11** (oracle-matched): distance combines component distances in quadrature, offset splits proportionally with per-component application, angle uses the Frame-class algorithm when all primary frames are Cartesian and otherwise the exactly-one-component-defined rule; MinAxes/MaxAxes default to 0/1000000; Domain defaults to `"<DOM1>-<DOM2>"` (oracle: `"SKY-"` with a blank second domain) or `"CMP"`; Epoch/ObsLat/ObsLon/ObsAlt/Dtai setters forward to both components with the C Test-chain on the getters.
   This work also fixed the section-6 BasicFrame `angle` (signed 2-D, degenerate→BAD via a shared `cartesian_angle` helper) and `offset` (coincident+nonzero→BAD), and the 6d SkyFrame `offset` degenerate case, since the CmpFrame combination rules depend on them.
   Platform note: the coincident/degenerate branches require an *exactly* zero cross product or distance; FP contraction in the vendored PAL keeps general-position "coincident" points off those branches in both libraries (oracle-verified), and the SkyFrame cross-product argument order now mirrors C so even the noise-regime outputs agree.
6. **SpecFrame SpecOrigin** (6e): MakeSpecMapping steps 0/9 and system-change rescaling.
7. **Frame base** (6): `astConvert`/ConvertX, matchsub cast-retry, 2-D signed `angle`, `offset` BAD semantics, printf Format + Digits, then the missing method family (Offset2/Ax\*/Resolve/Intersect/NormBox/MatchAxes/ActiveUnit/PrimaryFrame — unwinding the two fitschan `primary_frame` copies).
8. **Mapping base** (6a): `rate` interval-search continue-vs-break and h0 seed; `linear_approx` all-BAD success case.
9. **Frame equality** (6l) — **interim fix applied 2026-06-10**: Title/Label/Symbol/Format no longer affect frame equality and Domain is compared as effective strings (BasicFrame/SkyFrame/SpecFrame/TimeFrame/FluxFrame; compound frames delegate), so cosmetic differences no longer block NormMap-style cancellation (tests: `tests/test_frame_equality.rs`).
   Still pending: full conversion-based `Equal` once ConvertX lands, and PermMap referenced-constant equality.
10. **Medium tail**: CmpFrame epoch/obs forwarding and per-axis accessors (6c), SkyFrame offset/epoch/Bottom/Top (6d), TimeFrame iso Format + Epoch-from-TimeOrigin (6f), FluxFrame `MakeFluxMapping` chain (6f), PermMap constant-index laxity + `astMtrRot`/`astMtrMult` (6i), CmpMap chain-rule `rate` (6g), DSB capi unit conversions (6e), 1-based/0-based `map_split` convention (6l).
11. **Re-verify the 2026-05-08 mapping audit** (section 4) — several items are confirmed fixed (2.1, 2.2, 2.4, 2.5, 3.11, 4.1-4.3, 4.8-4.9, 5.1 partially; 2.3 half-fixed, see 6l); strike the stale ones and fold survivors into the simplify backlog (6g pairwise-protocol note).
12. **Deferred with their delivery steps**: Rebin/Resample/TranGrid/QuadApprox, Plot-support Frame methods (Gap/Abbrev/Fields/FrameGrid/LineDef family), IntraMap/XphMap/Moc, wcslib 6a, JSON renderer, PLAN.md reconstruction (section 1/2).

## 1. Verified missing functionality (fix these)

Ordered roughly by impact.

- **[HIGH→PORTED 2026-06-10, adoption complete] PolyMap — `astPolyTran`.**
  `PolyMap::poly_tran` ports polymap.c `ReplaceTransformation`/`SamplePoly1D`/`SamplePoly2D`/`FitPoly1D`/`FitPoly2D` (grid of `2*order` points per axis, unit-RMS column scaling, linear least squares via the `levenberg_marquardt` crate, insignificant-coefficient zeroing, increasing order with the `acc`/`maxacc`/`maxorder` acceptance rule, IterInverse cleared in the result); tests in `tests/test_polytran.rs`.
  The fit was verified **bit-compatible with the reference C library** by building libast and probing `astPolyTran` on the sip2 header: per-order racc agrees to 7 significant figures (order 7: C 6.463631e-3 vs Rust 6.4636e-3) and the fitted inverse at the origin matches to 7 digits — neither minimiser is "more robust"; both fully converge on this linear problem.
  The earlier fixture mismatch had a different cause entirely: the wcsconv cases pass `sipreplace=0` (so the C oracle never ran PolyTran there), and C's `IterInverse` *default* is `ncoeff_i == 0` (polymap.c:6311) — both now mirrored: the IterInverse default is ported, the FitsChan `SipReplace` attribute exists (default true, honoured pre-read by the test harness as in `wcsconverter.c:87`), and `wcs_matrix.rs` makes the real C PolyTran call with the IterInverse=1/Niter=6/Tol=1e-8 fallback.
  The old workaround is gone; behaviour tests in `tests/test_fitschan_wcs.rs`.

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

1. **Frame base** — DONE 2026-06-10, findings in section 7.
2. **SkyFrame + SkyAxis** — DONE 2026-06-10, findings in section 6d.
3. **SpecFrame + DSBSpecFrame** — DONE 2026-06-10, findings in section 6e.
4. **TimeFrame + FluxFrame + SpecFluxFrame** — DONE 2026-06-10, findings in section 6f.
5. **CmpFrame** — DONE 2026-06-10, findings in section 6c.
6. **FrameSet** — DONE 2026-06-10, findings in section 6b.
7. **Mapping base** — DONE 2026-06-10, findings in section 6a.
8. **CmpMap + TranMap + simplify engine** — DONE 2026-06-10, findings in section 6g.
9. **PolyMap + ChebyMap** — DONE 2026-06-10, findings in section 6h.
10. **Linear family** — DONE 2026-06-10, findings in section 6i.
11. **Sphere family** — DONE 2026-06-10, findings in section 6j.
12. **Spectral/time maps** — DONE 2026-06-10, findings in section 6k.

All twelve groups are now audited; see section 0 for the prioritized fix order.

## 6. Detailed findings: Frame base (audited 2026-06-10)

Compared `frame.c`/`axis.c` against `frames/mod.rs`, `frames/basic.rs`, `frames/find_frame.rs`.
`find_frame.rs` (default Match/SubFrame/FindFrame/Overlay) is a faithful port; the gaps are elsewhere.

- **[HIGH] Frame — `astConvert` is a stub.**
  `BasicFrame::convert` (`basic.rs:678`) returns a bare UnitMap whenever axis counts match and `None` otherwise, ignoring the `domains` argument entirely.
  C `ConvertX` (`frame.c:3028-3258`) loops over the domain list in priority order, builds a "common coordinate system" by a double `astMatch` (template→frame1 giving common1, then `PreserveAxes=0` and common1→frame2), composes `from_map` then inverted `to_map`, simplifies, and keeps the candidate with the best `2*TranForward + TranInverse` score (early exit on 3).
  Port the full algorithm; it is what makes cross-domain and attribute-overlaid alignment work for plain and compound frames.

- **[HIGH] Frame — `astMatch` matchsub cast-retry missing.**
  `default_match` (`find_frame.rs:393`) ignores its `_match_sub` parameter.
  C `astMatch_` (`frame.c:15497`) retries a failed match by casting the template down to the target's class (`astCast`), re-fixing `Domain`, and re-invoking Match — this is what lets a specialised template (e.g. SkyFrame, or a Frame vs a CmpFrame) align with a plainer target sharing its Domain.
  Without it `astConvert(basicframe_FRED, cmpframe_FRED)` style alignments fail.

- **[HIGH] Frame — `angle` wrong in 2-D and for degenerate vectors.** *(FIXED 2026-06-11, shared `cartesian_angle` helper.)*
  C `Angle` (`frame.c:1230-1320`) returns a *signed* angle (`palDrange(angc - anga)`, range ±π) for 2-axis frames and AST__BAD when either vector has zero length or any input is bad.
  Rust (`basic.rs:534`) always returns the unsigned `acos` form and returns `0.0` for zero-length vectors.

- **[HIGH] Frame — `offset` coincident-point behavior.** *(FIXED 2026-06-11.)*
  C `Offset` (`frame.c:8470-8490`) returns all-AST__BAD output when the two points are coincident and the requested offset is non-zero (and `point1` when offset is zero).
  Rust (`basic.rs:558`) returns `from` unchanged in both cases.

- **[HIGH] Frame — Format attribute is not printf-style and Digits is absent.**
  C formats through the Axis Format attribute, a full printf specifier with default `"%1.<Digits>G"` (`axis.c GetDefaultFormat`, Digits default 7) including integer-cast handling for `%d`.
  Rust (`basic.rs:471`) accepts only a bare precision integer in `format_strs` and otherwise uses Rust's default `Display` (shortest-repr), so most formatted output differs from C.
  Port printf-format interpretation (or a faithful subset) plus the Digits-derived default.

- **[MED] Frame — Digits and Direction attributes do not exist.**
  No `digits` or `direction` symbol anywhere under `frames/` (verified by grep); C has frame-level `Digits`, per-axis `Digits(axis)`, and per-axis `Direction(axis)` (`axis.c:1421-1765`) with Format interplay and Plot consumers.

- **[MED] Frame — `unformat` error/whitespace contract differs.**
  C `astUnformat` (`frame.c:11819` → `axis.c:1354`, `sscanf("%lf %n")`) returns nread=0 *without raising an error* on parse failure and counts trailing whitespace in nread.
  Rust (`basic.rs:487`) raises `Unfer` on failure and stops nread at the last numeric character; the C contract matters for ast-capi parity and multi-field parsing.

- **[MED] Frame — geometry/axis method family missing.**
  Zero implementations exist for `astOffset2`, `astAxAngle`, `astAxDistance`, `astAxOffset`, `astAxNorm`, `astAxIn`, `astIntersect`, `astResolve`/`ResolvePoints`, `astNormBox`, `NormalPoints`/`NormPoints`, `astMatchAxes`/`MatchAxesX` (symbol grep over `crates/ast-core/src`).
  SkyFrame is supposed to override several of these (`skyframe.c`), so the base implementations are prerequisites for the SkyFrame audit items.

- **[MED] Frame — ActiveUnit / unit adaptation absent.**
  No `active_unit` symbol exists; C `astSetActiveUnit`/`GetActiveUnit` make Match/Overlay insert `astUnitMapper` conversions when units differ, and SpecFrame/FluxFrame enable it by default.
  Without it, unit differences between matched frames are silently ignored.

- **[MED] Frame — `astPrimaryFrame` missing and worked around.**
  No Frame-level `primary_frame`; `fitschan/fset_to_store/make_int_world.rs:53` and `celestial_axes.rs:341` each carry a private re-implementation.
  Port the Frame method (plus the CmpFrame override) and delete the two local copies.

- **[LOW] Frame — `format` of AST__BAD returns `"<bad>"`.**
  C `AxisFormat` has no bad-value special case and formats the raw double; the `"<bad>"` token (`basic.rs:473`) is an invention — verify no Rust caller depends on it, then match C.

- **[LOW] Frame — `IsUnitFrame` missing.**
  C uses it (`frame.c:6417`) to let simplification drop no-op Frame conversions; relevant once Frame-as-Mapping/Region simplification paths exercise it.

- **[DEFERRED] Frame — Plot-support methods missing: `astGap`, `Centre`, `Abbrev`, `Fields`, `FrameGrid`, `LineDef`/`LineContains`/`LineCrossing`/`LineOffset`.**
  All have zero Rust implementations; they are exercised mainly by Plot (delivery step 10) and Polygon/Region edge logic, so schedule them with that step (note `regions/polygon.rs` currently carries its own geometry).

## 6a. Detailed findings: Mapping base (audited 2026-06-10)

Compared `mapping.c` against `mappings/mod.rs`, `rate.rs`, `map_box.rs`, `linear_approx.rs`.
`map_box.rs` mirrors the C helper decomposition (Random/MapFunction/NewVertex/UphillSimplex/LocalMaximum/SpecialBounds/GlobalBounds) with identical constants (default_acc 3.0e-5, maxiter 10000, minsame 5, nbatch 32, simplex factor 3.0, maxcall 1500) — no findings beyond the spot check.
`linear_approx.rs` mirrors the C sampling phases (face centres, 0.49/0.51 nudges, vertex + half-way test points) and the per-point tolerance test including C's early-`break` quirk, with oracle fixtures — one finding below.

- **[HIGH] Mapping — `rate` abandons the interval search on a failed interval.**
  C `Rate` (`mapping.c:9040-9100`) keeps scanning larger/smaller interval sizes when `FindGradient` returns AST__BAD for one interval (the `if (mean != AST__BAD)` guard skips storing but the loop continues).
  Rust (`rate.rs:83-85` and `:122-124`) `break`s out of both search loops instead, so mappings with restricted domains (e.g. sky projections near the boundary) return BAD or a poorer derivative where C succeeds.

- **[MED] Mapping — `rate` initial interval for x0 = 0 is wrong.**
  C seeds `h0 = 1.0` when the evaluation position is zero (`mapping.c:9008`); Rust uses `f64::EPSILON * 1e9` ≈ 2.2e-7 (`rate.rs:43-47`), starting the adaptive search ~7 orders of magnitude too small.

- **[LOW] Mapping — `rate` zero-range confirmation indexing.**
  C reads `y[iin-1]`/`y[iin+1]` indexed by the loop counter even though stores go to `itop`/`ibot`, so after skipped (bad) intervals it can read stale slots — a C quirk.
  Rust currently avoids this only because of the incorrect `break` above; when fixing that, mirror C's indexing and record the quirk in `c-library-quirks.md`.

- **[MED] Mapping — `linear_approx` all-outputs-BAD case inverted.**
  When every output is bad at the face-centre sample points, C (`mapping.c:6952-6975`) fills the fit with identity gradients and AST__BAD zero terms and returns *success* (`result = 1`); Rust (`linear_approx.rs` step-3 all-BAD branch) returns `None` and its comment mis-states the C as returning false.
  Affects any caller probing near-degenerate regions (e.g. `is_map_linear`-style FitsChan checks).

- **[LOW] Mapping — `IsSimple` attribute and `astDecompose` have no trait-level counterpart.**
  No `is_simple`/`decompose`/`report` symbols in `mappings/mod.rs` or `simplify.rs`; C uses IsSimple to short-circuit repeated `astSimplify` calls and Report for debug tracing.
  `do_not_simplify` exists; add IsSimple when simplify-performance parity matters, and Decompose/Report at the ast-capi layer.

## 6b. Detailed findings: FrameSet (audited 2026-06-10)

Compared `frameset.c` against `frames/frameset.rs`.
The node-graph core (BFS path finding with per-edge inversion, `remap_frame` + `tidy_nodes`, `with_integrity`, `invert()` as base/current swap) is sound; the gaps are in removal, delegation, and the Mapping-side overrides.

- **[HIGH] FrameSet — `remove_frame` disconnects the graph.** *(FIXED 2026-06-11.)*
  Rust (`frameset.rs:233-281`) deletes every edge touching the removed frame's node, so removing a middle frame in a chain leaves the remaining frames with no conversion path (`mapping()` then fails with `Frsin`).
  C `RemoveFrame` (`frameset.c:8740-8800`) only drops the frame's *association* with its node and calls `TidyNodes`, which preserves connectivity by composing mappings through now-frameless nodes.
  Fix: stop deleting edges; drop the frame/node-map entry and call the existing `tidy_nodes` (already used by `remap_frame`).

- **[HIGH] FrameSet — Frame-method delegation is ~17 of ~130.** *(FIXED 2026-06-11 for the Rust trait surface — every trait method with a constant default now forwards to the current frame; the C per-axis attribute forwards become relevant as those accessors land.)*
  C forwards essentially every Frame virtual to the current frame (`frameset.c:6260-6350`), including Get/Set/Test/Clear of Domain, Label, Symbol, Unit, Format, Digits, Direction, Min/MaxAxes, MatchEnd, Permute, PreserveAxes, System, AlignSystem, Top/Bottom, ActiveUnit, plus Match, SubFrame, ConvertX, Overlay, PickAxes, Norm, Gap, Intersect, etc.
  Rust `impl Frame for FrameSet` forwards only naxes/format/unformat/distance/angle/offset/norm/snapshot/map_from/equal/convert/find_frame_template/obs\*/dtai/dut1/system_string/perm_axes; everything else silently falls back to constant trait defaults.
  Concretely: `fs.domain()` returns `""` instead of the current frame's domain, `test_domain()` false, `ident()` `""`, `min_axes`/`max_axes` return `fs.naxes()` rather than the current frame's values, and `match_template`/`sub_frame`/`overlay_template`/`pick_axes_snapshot` use the generic defaults — a FrameSet whose current frame is a SkyFrame does not match like a SkyFrame.
  Fix: forward every trait method with a default to `self.current()` (or `current_mut` via integrity for setters).

- **[HIGH] FrameSet — `simplified()` type-degrades to a bare mapping.** *(FIXED 2026-06-11.)*
  C `FrameSet::Simplify` (`frameset.c:10111`) returns a *copy of the FrameSet* with each internal edge mapping individually simplified (honoring the stored per-edge invert flags); Rust returns the composed base→current mapping, discarding the FrameSet.
  ast-capi `astSimplify` on a FrameSet must hand back a FrameSet (pyast/DS9 rely on it).

- **[MED] FrameSet — `remove_frame` base/current reset diverges.** *(FIXED 2026-06-11: Current resets to the last frame; set-state is not tracked persistently, so the default is applied at removal time rather than dynamically.)*
  C clears the attribute when the removed frame *was* base/current so the default applies (Base default = 1, Current default = **nframe, the last frame** — `frameset.c:5091-5150`), and decrements set values above the removed index.
  Rust resets both to frame 0 (the first frame), so the post-removal current frame differs from C whenever more than one frame remains.

- **[MED] FrameSet — variants are a stub.**
  `add_variant` (`frameset.rs:541`) only pushes `(name, map)`; there is no Variant attribute (select/get by name with remapping), no `astMirrorVariants`, no AllVariants, and `remove_frame` does no `varfrm` bookkeeping (C decrements mirror indices and calls `RemoveMirrors`, `frameset.c:8755-8765`).

- **[MED] FrameSet — Mapping-side overrides missing.** *(PARTIALLY FIXED 2026-06-11: `has_forward`/`has_inverse`/`rate` now delegate to the base→current mapping; `MapSplit`/`GetIsLinear`/`RemoveRegions` remain.)*
  C overrides `GetTranForward`/`GetTranInverse` (forward to the base→current path), `GetIsLinear`, `MapSplit`, `Rate` (delegates to the composed mapping so analytic rates apply), and `RemoveRegions` (`frameset.c:877-906`).
  Rust `impl Mapping for FrameSet` relies on trait defaults: `has_forward`/`has_inverse` always return `true` (wrong when the path contains a one-way mapping), and `rate` always uses generic `numeric_rate`.

- **[MED] FrameSet — `convert` ignores the other member frames.**
  Rust forwards to `self.current().convert(...)` (`frameset.rs:788`); C `FrameSet::ConvertX` considers *every* member frame (current, base, then index order) as a candidate alignment endpoint and prefixes the intra-set mapping.
  Blocked on the Frame-base ConvertX port (section 6, first item) — implement together.

- **[LOW] FrameSet — `astAddFrame(AST__ALLFRAMES, map, frame)` mode missing.**
  `FrameRef` has only Base/Current/Index variants (`frameset.rs:17`); the C AllFrames mode remaps every frame through the supplied PermMap-style mapping (used for axis re-ordering of whole sets).

## 6c. Detailed findings: CmpFrame (audited 2026-06-10)

Compared `cmpframe.c` against `frames/cmpframe.rs`.
The Match/SubFrame machinery (PartMatch, GenAxisSelection, GoodPerm analogues in `cmpframe_match`/`partition_selection`/`cmpframe_sub_frame`) is a genuine port with good test coverage, and `norm`/`perm_axes` handle the perm array correctly.
The gaps are in the geometry methods, attribute defaults, and attribute forwarding.

- **[HIGH] CmpFrame — `distance`/`angle`/`offset` are Cartesian-only.** *(FIXED 2026-06-11.)*
  C projects points into the component frames through the perm array and delegates: `Distance` (`cmpframe.c:2475-2560`) combines the two component distances in quadrature, `Offset` (`cmpframe.c:6163-6164`) splits the offset per component, and `Angle` (`cmpframe.c:1170-1240`) uses the Cartesian parent implementation *only* when every axis's primary frame is a plain `Frame`, otherwise the component path.
  Rust (`cmpframe.rs:214-265`) computes raw Euclidean math over the external coordinates for all three, giving wrong results whenever a component is a SkyFrame (the standard sky+spectral cube case).
  `Resolve` (`cmpframe.c:7868`) is likewise component-aware and has no Rust counterpart at all (cross-ref section 6 missing-method family).

- **[HIGH] CmpFrame — MinAxes/MaxAxes defaults are wrong.** *(FIXED 2026-06-11.)*
  C defaults are `0` and `1000000` when unset (`cmpframe.c:3680-3760`), so a CmpFrame template matches targets of any axis count; Rust defaults both to `naxes()` (`cmpframe.rs:319-336`, with a comment acknowledging the divergence).
  This silently disables the C behavior where `astFindFrame` with a CmpFrame template can match larger or smaller targets via PartMatch's extra-axis handling.

- **[HIGH] CmpFrame — Domain default is empty.** *(FIXED 2026-06-11.)*
  C `GetDomain` (`cmpframe.c:3595-3634`) returns `"<DOM1>-<DOM2>"` (component domains joined with a hyphen, recursing through nested CmpFrames) or `"CMP"` when both are blank; Rust returns `""` (`cmpframe.rs:194`).
  Domain strings drive `find_frame` domainlist gating and FITS-WCS bookkeeping, so e.g. `"SKY-SPECTRUM"` lookups fail.

- **[MED] CmpFrame — Epoch/ObsLat/ObsLon/ObsAlt/Dtai not forwarded to components.** *(FIXED 2026-06-11 via per-class downcast helpers — the Frame trait lacks epoch/observer setters, so BasicFrame (year-only setter) and FluxFrame components are skipped; revisit when the trait grows setters.)*
  C `SetEpoch` (`cmpframe.c:8800-8812`) sets the parent value *and* both components (same pattern for the Obs\*/Dtai setters), and `GetEpoch` (`cmpframe.c:3917`) falls back to frame1's value (then frame2) when unset on the CmpFrame itself.
  Rust CmpFrame has no epoch accessor and inherits the `None`-returning trait defaults for `obs_lat`/`obs_lon`/`obs_alt`/`dtai`, so observer/epoch context set at the compound level never reaches a SkyFrame/SpecFrame component.

- **[MED] CmpFrame — no per-axis attribute routing.**
  C routes Get/Set/Test/Clear of Label, Symbol, Unit, Format, Direction, Bottom, Top per external axis through the perm array to the owning component (vtab list `cmpframe.c`); Rust routes only `format`/`unformat` via `route_axis`.
  There are no `label(axis)`/`symbol(axis)`/`unit(axis)`-style accessors on CmpFrame at all, so callers must downcast components and re-derive the permutation themselves.
  Fix: add the standard per-axis accessors using the existing `route_axis` helper.

- **[LOW] CmpFrame — Title default is empty.**
  C `GetTitle` (`cmpframe.c:4424+`) defaults to `"<N>-d compound coordinate system"`; Rust returns `""` (`cmpframe.rs:73`).

## 6d. Detailed findings: SkyFrame + SkyAxis (audited 2026-06-10)

Compared `skyframe.c`/`skyaxis.c` against `frames/skyframe.rs`, `frames/skyaxis.rs`, `frames/sky_convert.rs`.
The attribute surface (System/Equinox/Epoch/NegLon/SkyRef/SkyRefIs/SkyRefP/AlignOffset/Projection/axis labels), `norm`, `distance`, `perm_axes`, the LAST computation, and the Match/SubFrame/Overlay machinery (including offset-coordinate frames, permutation-swap matching gated on Permute, and FK4 default-equinox handling) are ported with test coverage.
The confirmed divergences:

- **[HIGH] SkyFrame — `angle` math is wrong.** *(FIXED 2026-06-11, oracle-matched.)*
  C `Angle` (`skyframe.c:1190-1212`) computes `palDbear(b→a)` and `palDbear(b→c)` (bearings *from the vertex b*), returns `palDrange(angc − anga)`, applies a `π/2 − result` correction when the axes are permuted (lon on axis 2), and returns AST__BAD when a==b or c==b.
  Rust (`skyframe.rs:826-835`) calls `dbear(a→b)` and `dbear(c→b)` (bearings *of* b seen from a and c — not the same angle on a sphere), has no permutation correction, and no coincidence check.

- **[HIGH] SkyFrame — SkyTol is stored but never used, and its default is wrong.** *(FIXED 2026-06-11; note the original example was wrong — FK5↔ICRS does NOT collapse at the default tolerance, its 23 mas bias exceeds 1 mas.)*
  C defaults SkyTol to 0.001 arcsec (`skyframe.c:11549`) and `MakeSkyMapping` transforms NTEST probe points through the constructed SlaMap, replacing it with a UnitMap when the maximum shift is below the smaller of the two frames' SkyTol (`skyframe.c:10740-10772`).
  Rust defaulted `sky_tol` to 0.0 and no conversion path consulted it, so sub-tolerance chains never collapsed to UnitMap.

- **[MED] SkyFrame — `offset` degenerate-case handling.** *(FIXED 2026-06-11, incl. mirroring C's cross-product argument order for noise-regime bit parity.)*
  For coincident or antipodal points C (`skyframe.c:7592-7615`) returns output only when `sin(offset) == 0` (rotating v1 by the multiple of π via `cos(offset)` scaling) and AST__BAD otherwise; Rust (`skyframe.rs:851-853`) returns `from` unchanged whenever `|v1×v2| < 1e-20` regardless of the requested offset.

- **[MED] SkyFrame — Epoch default ignores the FK4 rule.**
  C `GetEpoch` (`skyframe.c:3156+`) defaults to B1950.0 for FK4/FK4-NO-E systems and J2000.0 otherwise; Rust always returns MJD 51544.5 = J2000.0 (`skyframe.rs:389`), so default-epoch FK4 frames convert with the wrong observation epoch.

- **[MED] SkyFrame — Bottom/Top defaults missing.**
  C `GetBottom`/`GetTop` (`skyframe.c:2907`/`3234`) clamp the latitude axis to ±π/2 by default; `skyframe.rs` has no bottom/top handling at all (zero grep hits), which feeds NormBox/Region bounding and the serialised Bot/Top tokens.

- **[MED] SkyAxis — Format attribute and Digits-driven precision unsupported.**
  Rust `format_sky` (`skyaxis.rs:159`) hardcodes precision 1 (time) / 0 (degrees), which happens to equal the C defaults at Digits=7 (`skyaxis.c GetAxisFormat`: "h"/"hm"/"hms"/"hms.N" by Digits), but there is no per-axis Format string (C accepts specifiers like `+zhms.2`, `g`, `d`, `lbi`, separator control), no Digits attribute, and no `AxisGap`/`AxisAbbrev`/`AxisFields`.
  Affects astFormat/astUnformat C-API parity and Plot tick labelling.

- **[DEFERRED] SkyFrame — spherical overrides of the missing Frame-method family.**
  `Gap`, `GetDirection`, `Offset2`, `Resolve`/`ResolvePoints`, `Intersect`, `NormBox`, `NormalPoints`, `FrameGrid`, `LineDef`/`LineContains`/`LineCrossing`/`LineOffset`, `MatchAxesX` all have SkyFrame-specific great-circle implementations in C and no Rust counterpart; schedule with the Frame-base family (section 6) since the base methods are prerequisites.

## 6e. Detailed findings: SpecFrame + DSBSpecFrame (audited 2026-06-10)

Compared `specframe.c`/`dsbspecframe.c` against `frames/specframe.rs`, `frames/dsbspecframe.rs`.
`make_spec_mapping_full` (`specframe.rs:1255`) is a faithful line-referenced port of C MakeSpecMapping steps 2-7 (system→freq, SOR chains through heliocentric, alignment-system relativistic steps, unit wrapping at both boundaries), the SOR conversion argument lists match C's TRANSFORM_n macros exactly (TPF2HL lon/lat/alt/epoch/ra/dec etc.), Match/SubFrame/Overlay have tests, and the DSB attribute set (DSBCentre 3.0e10 / IF 4.0e9 defaults, sideband mappings, ImagFreq, AlignSideBand) is present.
Confirmed divergences:

- **[HIGH] SpecFrame — SpecOrigin/AlignSpecOffset never applied in conversions.**
  C MakeSpecMapping wraps the step-2-7 chain with step 0 — prepend `ShiftMap(+target_origin)` when `GetSpecOriginCur` is non-zero (`specframe.c:2937-2945`) — and step 9 — append `ShiftMap(−result_origin)`, where the result origin is the *transformed target origin* when both frames have AlignSpecOffset set (offset-system alignment, `specframe.c:2947-2965`).
  Additionally `GetSpecOriginCur` (`specframe.c:2129`) converts the stored origin to the frame's current system/units, and `SetSystem`/`ClearSystem` rescale the stored SpecOrigin to the new system (`specframe.c:643,682`).
  Rust stores `spec_origin` (used only by overlay and serialisation — `specframe.rs:1971,2316`) and `align_spec_offset`, but `make_spec_mapping_full` has no step 0/9 and `set_system` does not rescale, so offset spectral frames (a SMURF/SCUBA-2 staple) convert wrongly.

- **[MED] DSBSpecFrame — DSBCentre/IF attribute-interface conversions live nowhere.**
  C's attribute layer converts DSBCentre to/from the frame's *current spectral system* via TopoMap on every string get/set (`dsbspecframe.c:459-480, 1322-1340`) and expresses IF in GHz externally (÷1e9 on get, ×1e9 on set); the internal values are topocentric Hz.
  Rust typed accessors use raw internal units, which is fine for ast-core, but the conversion must be implemented in the ast-capi string layer (and the Rust docs should state the units explicitly) or `astGetC(\"DSBCentre\")` parity breaks.

- **[LOW] DSBSpecFrame — Domain is explicitly set instead of defaulted.**
  `DSBSpecFrame::new` calls `set_domain(\"DSBSPECTRUM\")` (`dsbspecframe.rs:111`), so `test_domain()` is true; C provides DSBSPECTRUM via a `GetDomain` override with TestDomain false.
  An explicitly-set Domain is copied by Overlay and gates Match differently from a defaulted one.

## 6f. Detailed findings: TimeFrame + FluxFrame + SpecFluxFrame (audited 2026-06-10)

Compared `timeframe.c`/`fluxframe.c`/`specfluxframe.c` against the corresponding `frames/*.rs`.
TimeFrame's `make_time_mapping` covers all 11 timescales (TAI/UTC/UT1/GMST/LAST/LMST/TT/TDB/TCB/TCG/LT), applies TimeOrigin on both sides, passes LTOffset in hours, negates ObsLon per the TimeMap convention, and threads Dut1/Dtai; Match/SubFrame/Overlay have tests.
The numerical TDB↔UTC drift questions are already tracked in `docs/issues/timeframe-and-epoch-api-port.md` (leap-second constants, Fairhead-Bretagnon precision, Epoch/Equinox timescale documentation) and are not repeated here.

- **[MED] TimeFrame — no iso-date Format/Unformat.**
  C overrides `Format`/`Unformat` (plus `Gap`/`Centre`/`Abbrev`/`Fields`) so that `Format=\"iso.N[T]\"` renders and parses calendar date strings (`timeframe.c` Format/Unformat).
  Rust `format` (`timeframe.rs:724`) prints `{value:.7}` — which is also 7 *decimal places* rather than C's `%1.7G` 7 significant digits — and `unformat` parses bare numbers only.

- **[MED] TimeFrame — Epoch default ignores TimeOrigin.**
  When Epoch is unset but TimeOrigin is set, C `GetEpoch` (`timeframe.c:2458+`) returns the TimeOrigin converted to an absolute TDB MJD (UT1≈UTC approximation for rotation timescales); Rust always returns J2000 (`timeframe.rs:380`).
  Relative time axes read from FITS get a meaningful default observation epoch in C and a fixed J2000 in Rust.

- **[LOW] TimeFrame — `astCurrentTime` missing.**
  C provides the current time expressed in the TimeFrame's own system/timescale/origin; no Rust counterpart.

- **[HIGH, already in section 1] FluxFrame/SpecFluxFrame — cross-system conversion unported; C entry points for the fix.**
  FluxFrame: `MakeFluxMapping` (`fluxframe.c`) plus the `GetDensitySystem`/`GetDensityUnit` helpers (`fluxframe.c:1785-1790`).
  SpecFluxFrame: `MakeSFMapping` built from `MakeMap2`/`MakeMap3`/`MakeMapF`/`MakeMapI` (`specfluxframe.c:516-1060`), which make the flux conversion depend on the per-point spectral position; porting requires MakeFluxMapping first.
  Rust currently returns `None` for any cross-system case (`fluxframe.rs:369,634`; `specfluxframe.rs:197`) — correct refusal, no silent wrong answers.

- **[LOW] FluxFrame/SpecFluxFrame — positives.**
  System validation (FLXDN/FLXDNW/SFLUX/SFLUXW), SpecVal storage, per-system label/symbol/unit defaults, the SpecFluxFrame axis-swap matching, and `GetTitle` composition are present with tests.

## 6g. Detailed findings: CmpMap + TranMap + simplify engine (audited 2026-06-10)

Compared `cmpmap.c`/`tranmap.c` against `mappings/cmpmap.rs`, `mappings/tranmap.rs`, `mappings/simplify.rs`.
The engine is in much better shape than the 2026-05-08 audit snapshot: `flatten` mirrors `astMapList` with correct invert bookkeeping (inverted-series order reversal, double-inversion cancellation — tested), a fingerprint-based oscillation guard plus pass caps cover C's PatternCheck/recursion-guard role (old audit items 4.2/4.3), merged CmpMaps are re-expanded into the list (old 4.1), `CmpMap::equal` compares canonically flattened lists (old 2.1), `CmpMap::map_split` includes the MapSplit2 brute-force inverse fallback, and `TranMap::simplified`/`try_merge` reproduce C's MapMerge rules (invert normalisation, per-component simplification, replace-by-map1 when `map1 ∘ inv(map2)` simplifies to UnitMap, neighbour-TranMap merging).
Several old-audit simplification items are therefore FIXED — section 4's re-verification instruction stands.

- **[HIGH] CmpMap — `tran_forward`/`tran_inverse` are hardcoded `true`.** *(FIXED 2026-06-10, incl. TranMap and the dump-flag separation; see section 0 item 1.)*
  `CmpMap::new` (`cmpmap.rs:99-100`) sets both flags unconditionally; C `GetTranForward`/`GetTranInverse` (`cmpmap.c`) derive them from the components (AND over both children in the applied direction, respecting their invert flags).
  Consequently `has_forward()`/`has_inverse()` lie for any composition containing a one-way mapping (e.g. forward-only PolyMaps in SIP chains), and the C guarantee that simplification preserves transform-definedness cannot be enforced.
  Fix in `new()` (and verify `from_node` restores stored values).

- **[MED] CmpMap — `rate` ignores C's chain rule.**
  C `Rate` (`cmpmap.c:2997-3142`) chain-rules series compositions (sum over intermediate axes of `r1*r2` using each component's own `astRate`, so analytic component rates are used) and dispatches parallel compositions to the owning component, returning exactly `0.0` cross-component.
  Rust uses generic `numeric_rate` on the whole composition (`cmpmap.rs:710`), losing analytic precision and returning numerical noise instead of exact zero across parallel blocks.

- **[MED] Simplify — pairwise-adjacent protocol vs C's list-based MapMerge.**
  C nominates each mapping and hands its class MapMerge the *entire* map list plus position, allowing one call to consume an arbitrary run of neighbours (SlaMap/SpecMap/TimeMap merge whole same-class runs) or restructure 1→N.
  Rust `merge_loop` (`simplify.rs:246`) tries adjacent pairs in both orders, compensating with engine-level swap/restructure strategies (WinMap↔WcsMap/MatrixMap/PermMap swaps, parallel-in-series, series-in-parallel, PcdMap swaps, SphMap diagonal, WinMap distribution).
  This structural difference is the root cause of the residual categories in `docs/issues/simplify-serialization-gaps.md` (parallel WinMap merges, PermMap series folding, SlaMap run-merge); fixing them piecemeal is viable, but consider whether a list-based `try_merge_list` hook is cheaper than more bespoke strategies.

## 6h. Detailed findings: PolyMap + ChebyMap (audited 2026-06-10)

Compared `polymap.c`/`chebymap.c` against `mappings/polymap.rs`, `mappings/chebymap.rs` (the SIMD evaluation path was excluded as a documented deliberate divergence).
Strong parity in most areas: IterInverse/NiterInverse/TolInverse defaults match C (off, 4, 1e-6), the Newton-Raphson inverse uses C's linear-truncation initial guess, term evaluation uses C's power-table scheme, coefficient consolidation and linear decomposition (MatrixMap/WinMap/ShiftMap replacement) are implemented in `simplified()` — old-audit items 4.8/4.9 are FIXED — and `rate` matches C (which also uses generic numeric differentiation for PolyMap).
Findings:

- **[HIGH] PolyMap/ChebyMap — `has_forward`/`has_inverse` not overridden.** *(FIXED 2026-06-10.)*
  C `GetTranForward` returns whether forward coefficients exist and `GetTranInverse` returns `ncoeff_i || IterInverse` (`polymap.c:2222,2285`); neither Rust class overrides the trait defaults, so a forward-only PolyMap claims to have an inverse.
  Same bug class as the CmpMap finding in section 6g — fix together.

- **[HIGH] PolyMap — explicit inverse coefficients take priority over IterInverse.** *(FIXED 2026-06-10.)*
  C uses the iterative inverse *in preference to* any supplied inverse coefficients whenever IterInverse is set (`polymap.c:6092`); Rust uses explicit inverse terms first and iterates only when none exist (`polymap.rs:1036-1081`).
  The common FITS-SIP configuration supplies *approximate* inverse coefficients AND sets IterInverse, expecting iteration; Rust silently uses the approximation.

- **[LOW] PolyMap — `astMergeShift` not ported.**
  C's protected `MergeShift`/`MergeShifts` (`polymap.c:261,290`) absorb a neighbouring ShiftMap into the polynomial coefficients during merging; no Rust counterpart.

- **[LOW] PolyMap — zero-coefficient test uses a 1e-30 threshold.**
  C drops only exactly-zero coefficients during consolidation; Rust `consolidated.retain(|t| t.coeff.abs() > 1e-30)` (`polymap.rs:1217`) could drop legitimate tiny coefficients.
  Mirror C's exact comparison.

- **[LOW] ChebyMap — bounding-box test differs from C.**
  C marks a point bad when the *scaled* coordinate satisfies `fabs(x*scale + offset) > 1.0` (`chebymap.c:1165-1170`); Rust compares the raw input against lbnd/ubnd directly and its comment mentions \"a small tolerance\" not present in C (`chebymap.rs:300`).
  The two disagree by FP rounding exactly at the box boundary; mirror the C scaled test.

- **[Cross-ref] `astPolyTran` (and ChebyMap's override of it) is the section 1 headline gap**; the fitschan workaround at `wcs_matrix.rs:205` is the first consumer to unwind after porting.

## 6i. Detailed findings: linear family (audited 2026-06-10)

Compared `matrixmap.c`/`permmap.c`/`shiftmap.c`/`winmap.c`/`zoommap.c`/`unitmap.c`/`pcdmap.c` against the corresponding `mappings/*.rs`.
Healthy overall: every class has an analytic `rate` override and a `map_split` (closing most of old-audit item 5.1 for this family), MatrixMap tracks full/diagonal/identity forms and correctly refuses inverse transforms of singular or non-square matrices (`Nodef`), PermMap's positive/negative/zero permutation-entry semantics match C, PcdMap mirrors the SLA_UNPCD series (noted in `c-library-quirks.md`), and ZoomMap rejects zero factors.
Merge-rule completeness for this family is tracked separately (old audit sections 3/4 + `simplify-serialization-gaps.md`).
Findings:

- **[HIGH, joins the 6g/6h flag bucket] MatrixMap — `has_forward`/`has_inverse` not overridden.** *(FIXED 2026-06-10.)*
  A singular or non-square MatrixMap stores `inverse: None` and errors on inverse transform, but still reports `has_inverse() == true` via the trait default; C `GetTranInverse` reports whether the inverse matrix exists.
  Fix together with CmpMap/PolyMap/ChebyMap.

- **[MED] MatrixMap — `astMtrRot`/`astMtrMult` missing.**
  The public C methods for post-multiplying and rotating a MatrixMap have no Rust counterpart, and `fitschan/wcs_celestial.rs:765-818` carries a private re-implementation of the astMtrRot rotation sequence — another instance of the workaround pattern; port the methods and unwind the local copy.

- **[MED] PermMap — out-of-range constant indices rejected at construction.**
  C substitutes AST__BAD at transform time when a negative permutation entry refers past the supplied constants (`permmap.c:1049-1054`; behavior explicitly added per the changelog); Rust `PermMap::new` returns `Prmin` instead.
  C-built objects (deserialisation, C-ABI construction) can legitimately carry such entries; move the check to transform time.

## 6j. Detailed findings: sphere family (audited 2026-06-10)

Compared `wcsmap.c`/`sphmap.c`/`dssmap.c`/`normmap.c`/`unitnormmap.c` against the corresponding `mappings/*.rs` and `wcslib.rs`.
This family gets a near-clean bill of health:

- **WcsMap**: the projection enum covers all 31 C projections plus Unknown; PV parameters have typed get/set with axis validation and are included in `equal` via `params_equal` (old-audit item 2.5 is FIXED); the engine has `try_swap_wcsmap_past_permmap` (old 3.11 FIXED); neither C nor Rust overrides the transform-definedness flags, so no parity issue there.
- **SphMap**: UnitRadius and PolarLong both present (PolarLong with explicit set-tracking), pole/bad-value/round-trip behavior covered by tests; the one known cancellation gap is already analysed in `docs/issues/sph_fwd_inv_unitradius_cancel_analysis.md`.
- **DssMap**: `platepos`/`platepix` are a genuine Rust port of the plate code (not FFI), with WinMap-absorption merge terms.
- **NormMap**: properly overrides `has_forward`/`has_inverse` (the only mapping class audited so far that does), with merge cancellation present.
- **UnitNormMap**: all four C MapMerge rules implemented (forward+inverse → UnitMap/ShiftMap, UnitNormMap(inv)+ShiftMap absorb, ShiftMap+UnitNormMap(fwd) absorb in `shift.rs:203` — a rule C's own comments note never fires upstream because ShiftMaps become WinMaps first).

Remaining items, all already recorded elsewhere: the WcsMap `Unknown`→CAR placeholder and DSS-FitsChan construction gap (section 1), and the SphMap fixture analysis doc.

- **[LOW] WcsMap — NatLat/NatLon/PVMax accessors absent.**
  C exposes them as read-only attributes (`wcsmap.c:765-769`); the underlying values are reachable via `wcslib.rs::reference_point`, so this is an ast-capi surface gap rather than missing functionality.

## 6k. Detailed findings: spectral/time maps (audited 2026-06-10)

Compared `specmap.c`/`grismmap.c`/`slamap.c`/`timemap.c` against the corresponding `mappings/*.rs`.
Clean bill of health on completeness:

- **SpecMap**: all 34 C conversion codes present 1:1 (FRTOVL…HLF2US); the RefRA/RefDec float-cast quirk is deliberately mirrored per `c-library-quirks.md`.
- **SlaMap**: all 28 C conversion codes present (E2H/H2E mapped to De2h/Dh2e); argument validation in place.
- **TimeMap**: all 28 C conversion codes present including the four long sidereal codes (GMSTTOLMST etc.); the leap-second table matches C through 2017-01-01 (MJD 57754 + TAI−UTC = 37 s) and the pre-1972 `astDat` divisor bug is deliberately mirrored.
- **GrismMap**: all 8 attributes (NR, NRP, WaveR, Alpha, G, M, Eps, Theta) with the GrismMap+ZoomMap merge updating the constants (`merge_with_zoom`/`update_constants`), analytic `rate`, and dispersion fwd/inv.
- All four classes have tier-1 integration tests ported from `ast_tester` (`tests/test_{specmap,slamap,timemap,grismmap}.rs`).

No new findings.
The known residual items are already tracked elsewhere: SlaMap adjacent-run concatenation (old audit 3.1 / `simplify-serialization-gaps.md` §6), TimeMap step-level simplification (old 3.2), SpecMap unconditional adjacent merge (old 4.10), and SlaMap per-point `palMappa` performance (old 5.3 / perf doc).

## 6l. Supplemental pass: astEqual / astSimplify / astMapSplit (audited 2026-06-10)

Focused follow-up on the three historically problematic entry points, covering what sections 6-6k left unverified: Frame-side equality, old-audit equality items 2.2-2.4, and the MapSplit algorithm bodies.

- **[HIGH] Frame equality is attribute-based; C's is conversion-based.** *(Interim fix 2026-06-10: cosmetic attributes excluded, effective-Domain comparison; conversion-based Equal still pending on ConvertX.)*
  C `Frame::Equal` (`frame.c:3549-3573`) defines equality as: same class AND `astConvert(that, this, \"\")` succeeds AND the resulting mapping simplifies to a UnitMap — and **no Frame subclass overrides Equal** (verified: no `static int Equal(` in skyframe/specframe/timeframe/cmpframe/fluxframe.c).
  Rust frames compare attributes instead, including cosmetic ones — `BasicFrame::equal` (`basic.rs:654`) compares title, labels, units, symbols, and format strings.
  Consequence: NormMap pair cancellation (`normmap.rs:221,230`) and every other frame-equality-gated merge under-cancels relative to C whenever two frames differ only cosmetically; this presents as \"astSimplify didn't simplify\" bugs.
  The proper fix is conversion-based equality, which is blocked on the ConvertX port (section 6, first item); as an interim step, exclude Title/Label/Symbol/Format from the attribute comparison.

- **[MED] PermMap equality compares constants positionally; C compares referenced values.**
  C `Equal` (`permmap.c:376-390`) treats two differing negative entries as equal when the *referenced* constants are equal (`constant[-p1-1] == constant[-p2-1]`, exact comparison) and never looks at unreferenced constants; out-of-range entries go through `SAFE_PERMVAL`.
  Rust (`perm.rs:864-875`) requires identical perm arrays plus whole-array positional constant equality, so behaviorally identical PermMaps with reordered or surplus constants compare unequal (under-merging).
  Old-audit item 2.3 is thus half-fixed: invert-flag resolution is done, referenced-value comparison is not.

- **Old-audit equality items 2.2 and 2.4 are FIXED.**
  MatrixMap now compares effective forward matrices resolving invert flags (`matrix.rs:543-572`); WinMap compares effective scale/shift parameters resolving inversion (`winmap.rs:302-314`).
  Strike both from the section 4 re-verification list.

- **MapSplit algorithm bodies verified — no new math findings.**
  The trait default (`helpers::full_set_split`) matches C's full-selection path including the permuted-selection PermMap composition (`mapping.c:7798+`).
  `PermMap::map_split` (`perm.rs:378-470`) is a faithful port of `permmap.c:1438` with documented encoding translation (1-based/0-as-BAD vs C 0-based/negative), the constants pool reused, and deliberate stricter duplicate-axis validation; the missing `PermSplit` alternative mode remains a section 1 item.
  `CmpMap::map_split` (incl. the MapSplit2 fallback) was verified in 6g; the MatrixMap zero-init divergence is the documented quirk; FrameSet's missing override is in 6b.
  `TranMap::map_split` exists but its body was not deep-compared (low risk: it splits both components and recombines).

- **[LOW] Axis-index convention is inconsistent within the Mapping trait.**
  `rate` takes 1-based `ax1`/`ax2` (`rate.rs:32-33` subtracts 1) while `map_split` takes 0-based `in_axes` (`mod.rs:229` doc states it).
  The design spec mandates 1-based axis indices throughout the public API; this mixed convention is exactly the kind of trap that produces off-by-one workarounds downstream — standardise on 1-based.

## 7. Working rules for the fix agent

- Port the C logic; never paper over a gap with a heuristic the C does not have (repo convention).
- Before claiming something missing, grep the whole crate — much C per-class logic is centralized in shared Rust modules.
- Respect the SkyFrame setter hot path (O(1), allocation-free, lock-free) and the documented MatrixMap no-BLAS decision.
- Mirror C bugs only where `c-library-quirks.md` says parity requires it, and add to that doc when a new quirk is found.

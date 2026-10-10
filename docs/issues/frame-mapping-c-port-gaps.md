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
6. **SpecFrame SpecOrigin** (6e) — **FIXED 2026-06-11** (oracle-matched): MakeSpecMapping now applies step 0 (prepend ShiftMap by the target origin in current units, via the ported `GetSpecOriginCur`) and step 9 (append ShiftMap by the negated result origin, or by the transformed target origin when both frames set AlignSpecOffset); System and StdOfRest changes rescale a set SpecOrigin through a cleared-attribute conversion (`OriginSystem`/`OriginStdOfRest`).
   Probe note: the C `RestFreq` *attribute* is in GHz — a units trap when comparing against `set_rest_freq` (Hz).
7. **Frame base** (6) — **FIXED 2026-06-12**: `astConvert`/ConvertX (`find_frame::convert_frames`), the matchsub cast-retry (`match_with_subclass` + `Frame::cast`, wired through CmpFrame `part_match`/`component_match`), 2-D signed `angle`, `offset` BAD semantics, printf Format + Digits (`frames/axis_format.rs`), and the method family (Offset2/Ax\*/Resolve/Intersect/NormBox/MatchAxes/ActiveUnit/PrimaryFrame as `Frame` trait methods) are all ported; the two fitschan `primary_frame` copies were unwound to the `Frame::primary_frame` trait method. All per-class `convert_to` overrides retired except `FrameSet`/`Region` (by design). The SpecFrame/TimeFrame System/TimeScale set-ness fix and the Permute-default + SpecFrame RefRA fixes landed alongside. Acceptance gate: `tests/test_convert.rs` (ported `testconvert.c`).
   Still deferred (see section 6 sub-items): `NormalPoints`/`NormPoints`, the `reg` Mapping argument of SkyFrame `NormBox` (the great-circle `Offset2`, `Resolve` and `Intersect` and the `NormBox` itself are ported; `Resolve` and `Intersect` closed 2026-09-30, `tests/test_sky_geometry.rs`), Plot log-escape rendering of the Format `&` flag, the `astGet/SetActiveUnit` and geometry/MatchAxes capi shims, the `NewUnit` Label-rewrite/Format-clear, and FluxFrame/SpecFluxFrame cross-system conversion (section 1, `MakeFluxMapping`/`MakeSFMapping`).
8. **Mapping base** (6a) — **FIXED 2026-06-12** (oracle-matched): `rate` now mirrors C's `h0=1.0` seed at `x0=0` (`rate.rs:43`), `continue`s past BAD intervals instead of `break`ing both search loops, drops a spurious Rust-only `iin>NN+1` guard, indexes the zero-range neighbour by the C loop counter (stale-slot quirk), and returns `BAD_VALUE` rather than a fluke mean when no positive-range interval exists. Two confirmed C bugs documented in `c-library-quirks.md`: the `h0=1.0` seed makes `astRate` return BAD at `x0=0` for domain-restricted Mappings (ChebyMap) and a small wrong value for `tan` at 0; and the zero-range confirmation reads a stale array slot after skipped intervals. The `linear_approx` "all-BAD returns success" finding was a **false positive**: the oracle confirms C returns `result=0` with an all-BAD fit (line 6972 overwrites the unit-transform fill), which the existing Rust `None` already matches — only the misleading comment was corrected.
   Tests: `tests/test_numeric_rate.rs`, `tests/test_chebymap.rs` (ChebyMap-at-zero now asserts BAD).
9. **Frame equality** (6l) — **FIXED 2026-06-13** (oracle-matched): every leaf Frame class (`BasicFrame`/`SkyFrame`/`SpecFrame`/`DSBSpecFrame`/`TimeFrame`/`FluxFrame`/`SpecFluxFrame`/`CmpFrame`) now routes `equal` through the shared `find_frame::frames_equal` — same class name, `astConvert` succeeds, and the base→current mapping simplifies to a `UnitMap` (frame.c:3549). No leaf class overrides `Equal` in C, so the old per-class attribute checks were replaced wholesale; `FrameSet`/`Region` keep their structural overrides. This corrected several oracle divergences the attribute model got wrong: same-Unit-different-string frames are equal with ActiveUnit off, DSBSpecFrame ignores its DSB-only attributes, same-System FluxFrames are equal regardless of SpecVal, and plain-Frame Epoch/SkyFrame Equinox-at-default/NegLon differences are equal. Recursion is safe: frame conversion never emits a `NormMap` (the only mapping that re-enters `frame.equal`). Exposed and fixed a latent SkyFrame gap: `make_offset_mapping` now mirrors C `SkyOffsetMap` (skyframe.c:7722) by gating the offset rotation on `SkyRef` being *set*, not on `SkyRefIs` alone, so an Origin frame with default SkyRef converts via the identity. PermMap equality now ports C `PermMap::Equal` (permmap.c:224): per-entry comparison in C's internal encoding, exact referenced-constant comparison, ignored surplus constants, and the matching-index constant-skip quirk. Tests: `tests/test_equal.rs`, `tests/test_permmap.rs`, `tests/test_frame_equality.rs`, `tests/test_fluxframe.rs`. Quirk logged in `c-library-quirks.md`.
10. **Medium tail**: CmpFrame epoch/obs forwarding and per-axis accessors (6c), SkyFrame offset/epoch/Bottom/Top (6d), TimeFrame iso Format + Epoch-from-TimeOrigin (6f), FluxFrame `MakeFluxMapping` chain (6f), PermMap constant-index laxity + `astMtrRot`/`astMtrMult` (6i), CmpMap chain-rule `rate` (6g), DSB capi unit conversions (6e), 1-based/0-based `map_split` convention (6l).
    Status (2026-06-13) — all eight worked:
    - **`map_split` 1-based convention — FIXED** (public 1-based `map_split` wrapping a 0-based protected `map_split_zero_based`, mirroring C's `astMapSplitId_`/`astMapSplit`; `tests/test_map_split.rs`).
    - **PermMap constant-index laxity — FIXED** (construction no longer validates perm values; out-of-range axes / absent constants resolve to BAD at transform time, matching C `astInitPermMap`/`Transform`; `tests/test_permmap.rs`).
    - **`astMtrRot`/`astMtrMult` — FIXED** (`MatrixMap::mtr_rot`/`mtr_mult`; `tests/test_matrixmap.rs`). The fitschan rotation-construction unwind is deferred — it uses a CCW convention that already matches C and the literal CW `astMtrRot` sequence flips a pole longitude sign (6i).
    - **SkyFrame Epoch-FK4 default + Bottom/Top — FIXED** (B1950 for FK4/FK4-no-E; latitude axis clamps ±π/2; `tests/test_skyframe.rs`).
    - **CmpFrame per-axis Direction routing + Title/epoch revisit — FIXED** (Direction routes to the owning component; Title default & epoch/obs forwarding confirmed already-correct; `tests/test_cmpframe.rs`). Settable per-axis Label/Symbol/Unit/Format accessors remain a follow-up (no failing behaviour).
    - **DSBSpecFrame capi unit conversions — typed methods added** (`dsb_centre_cur`/`set_dsb_centre_cur`, `if_ghz`/`set_if_ghz`, `imag_freq_ghz`; the ast-capi DSBSpecFrame string dispatch that calls them does not exist yet).
    - **TimeFrame — FIXED 2026-06-13**: bare-number Format `%1.7G`, the full `iso`/`iso.N`/`iso.NT` calendar Format and the 7-format date/time Unformat (via the now-exposed vendored pal calendar wrappers `pal::djcl`/`cldj`/`dd2tf`/`dtf2d` + a `read_date_time` port of `astReadDateTime`), per-instance Unit (with `make_map` unit bracketing) and Format storage, the `Date/Time`/`Date` axis-label override, and Epoch-from-TimeOrigin (TimeOrigin→TDB MJD) all landed and oracle-matched (`testtime.c` errors 26-34; native dump byte-identical for `unit=s,format=iso.2`). A suspected Besselian-epoch `astUnformat` "C bug" was later withdrawn (FIXED 2026-06-25): it was a missing `TimeOrigin` re-expression in the Rust port (C `OriginScale`/`OriginSystem`), now added on `set_system`/`set_timescale`/`clear_*`; see `c-library-quirks.md` (RESOLVED) and `timeframe-unformat-findings.md`. The GetTitle "offset from <date>" Title default is also ported and oracle byte-matched. Remaining 6f work is Plot-only (`Gap`/`Centre`/`Abbrev`/`Fields`). Tests: `tests/test_timeframe.rs`, `date_time` unit tests.
    - **FluxFrame `MakeFluxMapping` — FIXED** (oracle-verified). **CmpMap chain-rule `rate` — RESOLVED (no change needed) 2026-06-13**: instrumented the C oracle and found C's chain-rule `Rate` override is *defined but never installed* (`cmpmap.c:892` comments out `mapping->Rate = Rate`), so C uses whole-map numeric differentiation — exactly what Rust's `numeric_rate` already does (oracle-confirmed bit-identical: dsbydx `-2.11061140544971257e8` in both). The earlier "AIPS card mark/sweep bug" was a misdiagnosis: there is no card-sweep bug; both libraries recompute CDELT3 ≈ 999.99999888, both fail `astEQUAL` vs the input 1000.0, both correctly rewrite the comment to "Pixel size". Porting the override would *diverge* from C. Logged in `c-library-quirks.md`; `cmpmap.rs::rate` keeps `numeric_rate` with a note. **SpecFluxFrame `MakeSFMapping`** — FIXED 2026-06-14 (section 1): the full cross-system (spec, flux) conversion is ported and oracle-verified against `testspecflux.c`.
11. **Re-verify the 2026-05-08 mapping audit** (section 4) — several items are confirmed fixed (2.1, 2.2, 2.4, 2.5, 3.11, 4.1-4.3, 4.8-4.9, 5.1 partially; 2.3 half-fixed, see 6l); strike the stale ones and fold survivors into the simplify backlog (6g pairwise-protocol note).
12. **Deferred with their delivery steps**: Rebin/Resample, Plot-support Frame methods (Gap/Abbrev/Fields/FrameGrid/LineDef family), Moc, wcslib 6a, JSON renderer, PLAN.md reconstruction (section 1/2).

## 1. Verified missing functionality (fix these)

Ordered roughly by impact.

- **[HIGH→PORTED 2026-06-10, adoption complete] PolyMap — `astPolyTran`.**
  `PolyMap::poly_tran` ports polymap.c `ReplaceTransformation`/`SamplePoly1D`/`SamplePoly2D`/`FitPoly1D`/`FitPoly2D` (grid of `2*order` points per axis, unit-RMS column scaling, linear least squares via the `levenberg_marquardt` crate, insignificant-coefficient zeroing, increasing order with the `acc`/`maxacc`/`maxorder` acceptance rule, IterInverse cleared in the result); tests in `tests/test_polytran.rs`.
  The fit was verified **bit-compatible with the reference C library** by building libast and probing `astPolyTran` on the sip2 header: per-order racc agrees to 7 significant figures (order 7: C 6.463631e-3 vs Rust 6.4636e-3) and the fitted inverse at the origin matches to 7 digits — neither minimiser is "more robust"; both fully converge on this linear problem.
  The earlier fixture mismatch had a different cause entirely: the wcsconv cases pass `sipreplace=0` (so the C oracle never ran PolyTran there), and C's `IterInverse` *default* is `ncoeff_i == 0` (polymap.c:6311) — both now mirrored: the IterInverse default is ported, the FitsChan `SipReplace` attribute exists (default true, honoured pre-read by the test harness as in `wcsconverter.c:87`), and `wcs_matrix.rs` makes the real C PolyTran call with the IterInverse=1/Niter=6/Tol=1e-8 fallback.
  The old workaround is gone; behaviour tests in `tests/test_fitschan_wcs.rs`.

- **[HIGH] FluxFrame — `MakeFluxMapping`.** *(PORTED 2026-06-13.)*
  C `fluxframe.c::MakeFluxMapping` (1971) converts between flux systems (FLXDN/FLXDNW/SFCBR/SFCBRW) using `SpecVal`. Ported as `FluxFrame::make_flux_mapping` / `convert_flux_to` and wired into `fluxframe_sub_frame`: collapse surface-brightness onto flux-density, reject sb-mismatched pairs, standardise both SpecVals into the density system and require them equal, then read the zoom from the simplified spectral conversion (UnitMap→1 / ZoomMap→factor / else `astRate` at the mean SpecVal) → `ZoomMap(|zoom|)`. An optional encapsulated SpecFrame was added to FluxFrame (the `SpcFr` token round-trips) so SpecVal can be interpreted consistently across density systems. Oracle-verified: FLXDN→FLXDNW at 1.4e9 Hz = 0.653785630413752; same-system identity; sb-mismatch / absent-or-mismatched SpecVal → None. Tests: `tests/test_fluxframe.rs`.
  Per-instance Unit support landed 2026-06-13: `FluxFrame` now carries a Unit string (`unit`/`set_unit`/`test_unit`/`clear_unit`/`unit_or_default`), `system()` derives the default System from the Unit when System is unset (C `GetSystem`, fluxframe.c:1514), and `make_flux_mapping` brackets the density zoom with `map1`/`map3` unit conversions exactly as C does. Oracle-verified: 1 Jy FLXDN → 1e-26 default; Jy FLXDN → W/m²/Angstrom FLXDNW at 1.4 GHz → 6.5378563041e-27; incompatible unit → no conversion. The Unit round-trips via the Ax1 Axis node. This removes the blocker for SpecFluxFrame below.

- **[HIGH] SpecFluxFrame — cross-system conversion.** *(FIXED 2026-06-14.)*
  C `specfluxframe.c::MakeSFMapping` (1060) and its helpers `MakeMapF`/`MakeMapI` (754/895), `MakeMap2`/`MakeMap3` (516/631), and the standardised `GetFluxFrame`/`GetSpecFrame` (186/255) are ported into `specfluxframe.rs`. The 2-axis (spec, flux) conversion scales the flux value by the rate of change of the output spectral system w.r.t. the input — evaluated at the actual spectral coordinate via a `RateMap` — through the PermMap + parallel CmpMaps + `TranMap`(`MathMap` divide/multiply) graph, wired into `specfluxframe_sub_frame` with axis-swap bracketing. Oracle-verified against `testspecflux.c`: Jy FLXDN → W/m²/Angstrom FLXDNW = 1.33425638e-26; log(W/m2/nm) → -24.8747607; surface brightness → -21.3585154 (spectral axis passed through). Supporting fixes: `MathMap` accepts a bare-identifier inverse expression as an input-name declaration (matching C `ExtractVariables`); FluxFrame Overlay clears the Unit on a default-unit-changing System overlay (fluxframe.c:105); `convert_frames`/`FrameSet::edge_mapping` avoid re-simplifying into the simplify-engine gap that drops the TranMap inverse; `make_sf_mapping` short-circuits identity conversions to a UnitMap. A residual simplify-engine limitation (it cannot fully collapse the TranMap-carrying graph) is worked around, not yet fixed — noted in `docs/issues/simplify-serialization-gaps.md`. Tests: `tests/test_fluxframe.rs`.

- **[MED] Frame — `astReadDateTime` only partially ported.** — **FIXED.**
  `frames/date_time.rs::read_date_time` now ports the full C `frame.c:8892`:
  B/J epochs, decimal years (IAU-1984), MJD, JD, and Gregorian dates with named
  months and `-`/`/`/`:`/space/`T` separators (see the item at the end of
  section 6 recording the complete port). Earlier snapshot's "decimal-year only"
  claim is superseded.

- **[MED→FIXED 2026-06-14] PermMap — `PermSplit` attribute.**
  `PermMap` now carries a settable `perm_split` attribute (`perm_split`/`set_perm_split`/`test_perm_split`/`clear_perm_split`), and `map_split_zero_based` ports the alternative `MapSplit` branch (permmap.c:1633): when set, the forward `outperm` is ignored, `inperm` is treated as the exact inverse, and the split returns a `UnitMap(nin)` with output indices read from the selected inputs' `inperm` values — failing when a selected input is fed by a constant or when two selected inputs share an output. The `pmsplt` token round-trips through the native dump. Oracle-verified against the built reference libast (protected `astSetPermSplit`/`astMapSplit` probe): pick {0,1}→UnitMap outs [1,0]; constant-fed→NONE; duplicate-output→NONE. Tests: `tests/test_permmap.rs`.

- **[wcsconv] FrameSet `Base` set-ness + SkyFrame-axis `ActiveUnit` + CLASS azel CRVAL — FIXED 2026-06-14.**
  Three distinct C-vs-Rust gaps surfaced while reopening the wcsconv tail (183/9 → 184/8):
  1. **FrameSet `Base` serialisation.** C writes the `Base` token as an active value only when the attribute is explicitly set (`set = this->base != -INT_MAX`, frameset.c:12554); a FrameSet assembled from a FITS header has it set (`astSetBase(ret, pixel)`, fitschan.c:11990), one built by `astAddFrame` leaves it defaulted (commented). Rust conflated "base_idx == 0" with "unset". Added a `base_set` flag (set by `set_base`, read from whether `Base` is an active item in `from_node`, cleared on base-frame removal per `astClearBase`, frameset.c:8769), and the FITS reader now calls `set_base(Index(1))` like C. The pixel/GRID frame also gains per-axis "Pixel axis N" labels (fitschan.c:11935). Fixed the `dss_fits_dss_ast` Base/label mismatch (case still blocked deeper on read-side mapping-tree simplification).
  2. **Broken Frame matching: SpecFrame template vs CmpFrame{Sky,Spec}.** `astFindFrame` with a 1-axis SpecFrame template (MinAxes=1,MaxAxes=3) against a CmpFrame holding a 2-axis SkyFrame + 1-axis SpecFrame failed in Rust but succeeds in C (oracle-confirmed). Root cause: the parent `Frame::Match` probe picks a single axis that lands on the SkyFrame, producing a `BasicFrame` snapshot; Rust's snapshot reported `ActiveUnit=true` (overlaid from the SpecFrame template), so the unit-compatibility step tried to map `hh:mm:ss.s` → `m/s` and aborted the whole match. C's `Frame::GetActiveUnit` (frame.c:4626) **always returns 0 for a Frame containing a SkyAxis**, skipping that step. Rust has no per-axis `SkyAxis`, so `BasicFrame` gained a transient `has_sky_axis` flag (set by `SkyFrame::pick_axes_snapshot` on a partial pick) that forces `get_active_unit()` to 0. Non-FitsChan regression tests added (`tests/test_find_frame.rs`, `frames::basic` unit test).
  3. **CLASS AZIMUTH/ELEVATIO + VELO-LSR wrong CRVAL axis.** The CLASS writer hard-coded `CRVAL2`/`CRVAL3` for the spatial reference point; C uses `crval[axlon]`/`crval[axlat]` (fitschan.c:6922) where the lon/lat axes are located by CTYPE prefix. Added `find_lon_lat_axes` (RA--/DEC-, xLON/xLAT) so the right CRVALs feed the radec→azel chain. Fix (2) also restored the VELO-LSR/VLSR/DELTAV emission, which had been silently skipped because the same find_frame call failed. `skyspec3d_class_fits_class` now passes byte-exact.

  Follow-ups since: DSBSpecFrame-on-read (IMAGFREQ→DSBSpecFrame + DSBSetUp port) landed `class_write` (185/7); the ASTWARN-on-illegal-keyword port landed `jan_fits_class_rt` (186/6); the simplifier series→parallel absorption fix + read-side `WcsMapFrm` simplify are groundwork; the offset-SkyFrame FITS-WCS writer landed `offset_sky` + `offset_sky_pole` (188/4); the TPN→`RA---TAN`+`QVi_m` writer landed `dss_ast` (189/3); the degenerate FITS-WCS writer + CmpFrame `Axp` serialisation landed `degen1` (190/2); the `reconstruct` right-association fix collapsed the mapping-tree divergence in the last two cases. Still failing (2): `jan_fits_class_ast` and `dss_fits_dss_ast` — both now blocked ONLY on SkyFrame/SpecFrame/DSBSpecFrame **serialisation details** (their mapping trees match C exactly): missing `Ident=" "`+`IsA Object`, `SRefIs`/`SRef1`/`SRef2` (RefPos→SkyRef), DSBSpecFrame `Frequency (USB)` axis label + `(Hertz)` unit-name comments, child `IsSimp=1` flags, a MatrixMap near-zero element (Rust `5.57e-17` vs C `0`) + ~1-ULP drift, 1-ULP `Epoch`/`RefRA`, and `UFreq` attribute ordering.

- **[wcsconv/simplify] `reconstruct` right-association — FIXED 2026-06-14.**
  C `CmpMap::Simplify` (cmpmap.c:290-319) rebuilds a merged Mapping sequence by looping the list in reverse and composing `result = astCmpMap(map[i], result)`, giving a right-associated tree `map[0] ∘ (map[1] ∘ …)`. Rust's `reconstruct` folded forward (left-associated) — transform-equivalent but a different serialised nesting, so a FITS-read FrameSet's native dump diverged structurally from C. Switched to C's reverse fold. Fixture `reconstruct_right_assoc.{map,simp}` (a `WcsMap ∘ ZoomMap ∘ UnitMap ∘ WcsMap(inv)` series where the UnitMap merges away, leaving 3 survivors C nests right). No regressions (431-fixture byte-exact round-trip and the simplify regression suite both clean). Collapsed the entire mapping-tree divergence in `jan_fits_class_ast`/`dss_fits_dss_ast` (0 mapping-structure lines now differ from C).

- **[wcsconv] Degenerate FITS-WCS writer + CmpFrame `Axp` serialisation — FIXED 2026-06-14.**
  `degen1` is a degenerate WCS (GRID→current Mapping has Nin=2, Nout=3) whose current Frame is a permuted `CmpFrame{SkyFrame(RA,DEC), Frame(WAVE)}`. Three coupled bugs: (1) **CmpFrame axis permutation was dropped on native round-trip** — `to_node`/`from_node` never read/wrote the `Axp{n}` tokens (cmpframe.c:10453-10475 Dump / 11020-11031 Load), so the read FrameSet lost the WAVE-first axis order. Ported `Axp{n}` round-trip (1-based internal axis, emitted only when permuted, identity suppressed via C's `helpful=0`), routing the Lbl/Uni/Dir forwarding through the permutation. (2) `other_axes::resolve_subframe` ignored the CmpFrame perm, leaking a stale RA CTYPE onto the WAVE axis (cmpframe.c:8290+). (3) The pixel→IWC WcsMap deproject/reproject round-trip didn't cancel because Rust built a fresh 2-axis WcsMap where C reuses the full N-axis lifted WcsMap (`map2b`, fitschan.c:5199-5230) + an N-axis `WcsNative` (fitschan.c:38334-38377); the residual ~3e-15 FP noise broke the degenerate DEC axis's `FitLine` linearity test (tolerance floor at fitschan.c:21970), so `make_int_world` returned false and no CRPIX/CD were emitted. Fixed by reusing the full lifted WcsMap + an `embed_native_rotation` helper, and extending the simplifier's `try_swap_winmap_wcsmap` to also swap a `ShiftMap` past a WcsMap when identity on the celestial axes, so the two opposite-direction WcsMaps become adjacent and cancel. Tests: `cmpframe_axis_permutation_round_trips` (Axp round-trip), `degen1` wcsconv fixture. One sub-gap: if a celestial WcsMap carried `PVi_3`/`PVi_4` (LONPOLE/LATPOLE-as-params), C clears them on `map2b`; Rust keeps them (no current fixture diverges) — a `WcsMap::clear_pv` would be the faithful addition if one arises.

- **[wcsconv] FITS-WCS writer: TPN projection → `RA---TAN` + `QVi_m` — FIXED 2026-06-14.**
  The AST-specific `TPN` ("TAN with polynomial correction") projection is written to FITS-WCS as a standard `RA---TAN`/`DEC--TAN` CTYPE with the polynomial coefficients carried in `QVi_m` keywords instead of `PVi_m`. Non-AST software then reads a plain TAN and ignores the QV terms; AST reverses the conversion on read. Ported the per-axis transform in the keyword writer (`WcsFromStore`, fitschan.c:37066-37077): when a CTYPE ends in `-TPN`, rewrite the suffix to `-TAN` and emit that axis's projection parameters with the `QV` prefix (the `parprefix` mechanism, fitschan.c:37074/37278). Fixes `dss_ast_fits_wcs` (DSS plate solution → TPN → `RA---TAN`+`QVi_m`). Covered by the existing `dss.ast`→`dss.fits-wcs` wcsconv fixture.

- **[wcsconv] Offset-SkyFrame FITS-WCS representation — FIXED 2026-06-14.**
  An offset SkyFrame (`SkyRefIs == Origin`/`Pole`) writes to FITS-WCS as plain linear `OFLN`/`OFLT` axes (no projection), with `WCSNAME = SKY_ORIGIN`/`SKY_POLE`, plus an extra alternate version describing the absolute coordinates. Load-bearing C fact: the `IsASkyFrame` macro (fitschan.c:1365) is `astIsASkyFrame(f) && Domain=="SKY"`; an offset frame carries Domain `SKY_ORIGIN`/`SKY_POLE`, so it deliberately fails the test and is routed through `OtherAxes` (linear CTYPEs from axis Symbols) rather than `CelestialAxes`/`SkySys` (which would emit a `-TAN` projection). Ported: `WcsSkyFrame` OF branch (read: `System=Unknown`, OFLN/OFLT Symbols, no EQUINOX/EPOCH, Domain marker retained — fitschan.c:39259-39354); `TidyOffsets` setting SkyRefIs without clobbering the Domain marker (fitschan.c:34464-34468); `MakeFitsFrameSet` internal-vs-external unit remap (radian→degree ZoomMap, which is why CD comes out -0.0099998477 not the raw -0.01 — fitschan.c:21234-21320); `FsetToStore` dual-version emission (extra absolute alternate per offset frame — fitschan.c:12230-12257); `AddVersion` negative-isoff SkyRefIs-clear (fitschan.c:2787-2820); `OtherAxes` generic Symbol/Label/Unit CTYPE/CNAME/CUNIT emission (fitschan.c:24358-24390); `WcsFcRead` mark-for-removal of input SREFIS/SREF/SREFP cards (fitschan.c:36650-36695, keeping SREFRA/SREFDEC). New `Frame` trait methods `internal_unit`/`test_axis_label`/`test_axis_unit` (astGetInternalUnit/astTestLabel/astTestUnit), overridden on SkyFrame (internal unit = rad). One documented divergence: Rust sets the read-side SkyRefIs directly on the offset Frame (not through FrameSet integrity as C does) because the OFLN/OFLT read already yields the correct linear offset Mapping and re-fusing through integrity would produce a Mapping the native serializer can't emit; the write-side absolute alternate still clears SkyRefIs through integrity, the only place the remap is needed.

- **[wcsconv] FITS keyword-name validation → ASTWARN cards — FIXED 2026-06-14.**
  C `CheckFitsName` (fitschan.c:5852) runs inside `astPutFits`/`SplitCard` (fitschan.c:32259) on every card read; when a non-blank keyword name contains an illegal character (anything outside `isFits` = letter/digit/`-`/`_`, fitschan.c:1361), `Warn(this,"badkeyname",...)` (fitschan.c:34946) inserts a block of `ASTWARN` comment cards immediately before the offending card. The `Warnings` attribute defaults to a list including `BadKeyName` and `Clean` defaults off, so the warning fires by default. Ported as `card::check_fits_name` (preserves the original-case name in the message, as C does — `SplitCard` copies the name verbatim before the check, never upper-casing it) + `card::wrap_warning_text` (the ~60-char word-boundary line wrap, fitschan.c:35000) + `CardList::push_astwarn` (leading blank `ASTWARN`, one card per wrapped line, trailing blank, with first-line dedup), wired into `from_header`/`put_cards`/`put_fits`. Fixes `jan_fits_class_rt`, whose input has a malformed `sys.` continuation line parsed as a keyword `SYS.` with an illegal `.`. Tests: `fitschan::card` unit tests.

- **[MED] Mapping — `astRebin`/`astRebinSeq`/`astResample` absent.**
  `mappings::resample` ports `astResample<X>` with nearest-neighbour and linear interpolation (2026-10-09); the kernel schemes, Rebin and RebinSeq follow the stages of `docs/superpowers/plans/2026-10-09-resample-rebin.md`.
  `astTranGrid` and `astQuadApprox` are ported as `Mapping::tran_grid` and `Mapping::quad_approx` (2026-10-09).
  These are large self-contained subsystems of `mapping.c` (spreading/interpolation kernels, adaptive linear-approximation subdivision).
  They are required for the frozen C ABI (step 10/11) and used by SMURF; decide a delivery step and track in PLAN.md rather than leaving unscheduled.

- **[MED] DSS FitsChan encoding — lossy TAN approximation.** — **FIXED 2026-06-14 (FitsChan path + DssMap class follow-up).**
  Reframed after reading the C reference: C's FitsChan DSS path does **not** construct a `DssMap` — `astDssMap()` is only ever called inside `dssmap.c`, never from `fitschan.c`. DSS is a pure FitsStore↔keyword conversion (`DSSToStore`, fitschan.c:9364 / `DSSFromStore`, fitschan.c:9066) that maps the AMDX1-20 / AMDY1-20 plate-solution polynomial to/from a **TPN** ("TAN with polynomial corrections") projection via the FitsStore PV parameters (`pv = amd/3600`, with C's exact index map), `RA---TPN`/`DEC--TPN` CTYPE, a PC matrix carrying the pixel sizes, CDELT=1, FK5/J2000/LONPOLE=180. The Rust `enc_dss.rs` previously collapsed the polynomial into a single plate-scale CDELT with a plain `RA---TAN`, losing the distortion. `enc_dss.rs` `read_from_cards`/`write_to_cards` are now a faithful port of `DSSToStore`/`DSSFromStore`. Also added `WcsProjection::description` (porting `astWcsPrjDesc`, wcsmap.c:4419) and wired `wcs_celestial.rs` to set the SkyFrame `Projection` from the WcsMap type (mirroring `WcsSkyFrame`, fitschan.c:39300), so the Title gains its `"; gnomonic polynomial projection"` suffix. Net wcsconv: 182/10 → 183/9 (`dss_ast_dss` now byte-exact; `dss_fits_dss_ast` is correct through the WCS content and now blocked only by the shared FrameSet `Base`-serialisation issue, same as `jan_fits_class_ast`; `dss_ast_fits_wcs` is blocked by the FITS-WCS *writer's* TPN→`RA---TAN`+`QVi_m` representation — a separate writer gap).

  **DssMap class follow-up — DONE 2026-06-14.** Mirroring C, where the `astDssMap` constructor is *protected* (only in `dssmap.h`, not public `ast.h`), `DssMap::from_plate_params` is now `pub(crate)` — a `DssMap` is produced only by native deserialisation (or the simplifier's `WinMap`-absorption rebuild), never via a public constructor. The `DssMap` type stays public (matching C's public `AstDssMap` type / `astIsADssMap`). Construction-based tests moved in-crate to `mappings/dssmap.rs` (`#[cfg(test)] mod tests`). This also enables the DssMap simplification pathways C's own harness marks "infeasible: protected constructor" (`simplify_pathways.md` dssmap-01..08): oracle-generated fixtures `dssmap_winmap_absorb` (07), `dssmap_inv_winmap_absorb` (08), `dssmap_zoom_no_merge` (03) were added to `simplify_tests.txt` — the `.map` inputs written by the Rust serializer, the `.simp` references by the reference C library's `astSimplify` reading those native dumps (oracle-confirmed: C reads a DssMap dump as a DssMap without converting it, and its `MapMerge` matches Rust's `try_merge` — CNPIX1=2100/XPixSz=50.526 forward, CNPIX1=8400/Invert=1 inverted). A C-written native dump is committed as `tests/fixtures/dssmap/c_dssmap.ast` and read back by an in-crate test against the C `astTran2` value.

- **[LOW] WcsMap — `Unknown` projection rebuilt as CAR placeholder.** — **FIXED 2026-06-14 (already-correct behaviour, made explicit).**
  Verified against C: an Unknown (`AST__WCSBAD`) WcsMap is an **identity** — C's Transform skips the projection, leaving lon/lat as a straight copy (wcsmap.c:4247), MapMerge replaces it with a UnitMap (wcsmap.c:3087), and `NatLon`/`NatLat` are 0 (the `PrjInfo` `theta0`/`phi0` for WCSBAD). The Rust transform/simplify paths already short-circuit Unknown to identity/UnitMap, so the CAR `PrjParams` was never actually exercised for the transform — but `nat_lon`/`nat_lat` read it (CAR's reference point is also 0, so the result was correct only by coincidence). Made the intent explicit: `nat_lon`/`nat_lat` now return 0 directly for Unknown, and the CAR placeholder is documented as inert storage that is never consulted. Oracle-verified: (0.3,0.7)→(0.3,0.7) identity, simplifies to UnitMap, nat_lon=nat_lat=0. Tests: `tests/test_wcsmap.rs::unknown_projection_is_identity`.

- **[LOW] Serialisation — JSON renderer from design step 5 absent.**
  Native read/write exists (`serial/native_read.rs`, `native_write.rs`) but no JSON/XML/YAML renderer despite the spec listing JSON in step 5.

- **[PROCESS] PLAN.md is empty.** — **FIXED.**
  PLAN.md is now the clearing-house index of active audited work (this doc is
  linked from it) and retains the C→Rust test-migration tracker.

## 2. Absent classes (confirm scheduling, not bugs)

- **IntraMap**: ported 2026-10-08 as `mappings/intramap.rs`, with the spec's `Arc<dyn IntraTransform>` and a per-Channel `IntraRegistry` (`Channel::register_intra`).
- **Moc**/**MocChan**: absent. XphMap, whose only C creator is `moc.c`, is ported (2026-10-07).
- **Stc\*** classes and **YamlChan/StcsChan/XmlChan**: absent (step 8/9 remainder).
- **wcslib**: still vendored C behind FFI (`src/wcslib.rs`); step 6a (Rust port, delete `vendor/wcslib/`) not started — by design, do not rewrite piecemeal.

## 3. Deliberate divergences (do NOT "fix")

`docs/issues/c-library-quirks.md` records where Rust mirrors or intentionally diverges from C bugs.
Key entries a fix agent must not regress: MatrixMap MapSplit zero-init (C has UB), SpecMap float-cast of RefRA/RefDec (mirrored for parity), `ast_dat` pre-1972 divisor bug (mirrored), VELOSYS string-quantisation drift (mirrored), DATE-OBS fractional-seconds parsing (Rust is correct, fixture adjusted).

## 4. Prior mapping audit — re-verify before fixing

`docs/mapping-gaps-audit.md` (2026-05-08) is a detailed Rust-vs-C audit of the mapping classes: per-axis BAD-value handling, equality bugs (CmpMap/MatrixMap/PermMap/WinMap/WcsMap), ~28 missing simplification rules (SlaMap/TimeMap/SphMap/MatrixMap/WinMap/WcsMap/LutMap/PermMap/PolyMap/SpecMap/ZoomMap/ChebyMap), CmpMap recursion-guard/PatternCheck gaps, and missing analytic `rate()` overrides.
A fix plan (`docs/superpowers/plans/2026-05-08-mapping-gaps-fix.md`) followed it, so an unknown subset is already fixed.
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

- **[FIXED] Frame — generic `astConvertX` driver.** *(2026-06-12)*
  The generic driver is ported as `find_frame::convert_frames`
  (`find_frame.rs`), a faithful port of C `ConvertX` (`frame.c:3028-3258`):
  the domain-field loop, the `icom` 0/1 common-coordinate-system
  negotiation via `match_with_subclass(..., match_sub=true)`, the
  `from_map`/`to_map` bookkeeping, `from_map ∘ inverted to_map` series
  composition, simplify, and the `2*TranForward + TranInverse` score with
  early exit at 3. `Frame::convert_to` has a default body that calls it,
  and **all per-class Frame overrides have been retired**: `BasicFrame`,
  `CmpFrame`, `SkyFrame`, `SpecFrame`, `TimeFrame`, `DSBSpecFrame`,
  `FluxFrame`, `SpecFluxFrame` now route through the generic driver via
  `Match`/`SubFrame`. The two earlier blockers are resolved:
  1. The class System/TimeScale set-ness now tracks "explicitly set"
     independently of value (`SpecFrame`/`TimeFrame`), with the C defaults
     (WAVELEN / MJD); the generic driver's `Match`/`Overlay` path overlays
     System correctly without the spectral-fixture regression.
  2. The SkyFrame `convert_to` override was retired earlier (the SkyFrame
     AlignSystem / diurnal-aberration path now matches C), so the fitschan
     CLASS encoder's `convert_to` calls flow through the generic driver
     with byte-exact WCS output (the wcsconv regression suite is unchanged).
  Acceptance gate: `crates/ast-core/tests/test_convert.rs` (ported from
  `testconvert.c`).
  **Only `FrameSet` and the `Region` classes still override `convert_to`,
  by design** — they legitimately delegate to the current/encapsulated
  frame rather than negotiating a common system.

- **[FIXED] Frame — `astMatch` matchsub cast-retry.** *(2026-06-12)*
  `match_with_subclass` (`find_frame.rs`) ports C `astMatch_`
  (`frame.c:15473`): it retries a failed match by casting the template down
  to the target's class (`Frame::cast`, modelling `astCast`), re-fixing
  `Domain`, and re-invoking Match. `Frame::cast` is implemented on the leaf
  specialised frames (down to `BasicFrame`), `DSBSpecFrame` (down to
  `SpecFrame`/`BasicFrame`), and `CmpFrame`. The CmpFrame partition and
  component matches (`part_match`/`component_match`) route each child match
  through `match_with_subclass` too, so a specialised component (e.g. the
  SkyFrame inside a Spec+Sky CmpFrame) aligns with a plainer target sharing
  its Domain. This is what makes `astConvert(Frame[SKY], CmpFrame)` and the
  `Frame[Domain=FRED]` vs `CmpFrame[Domain=FRED]` style alignments work.

- **[HIGH] Frame — `angle` wrong in 2-D and for degenerate vectors.** *(FIXED 2026-06-11, shared `cartesian_angle` helper.)*
  C `Angle` (`frame.c:1230-1320`) returns a *signed* angle (`palDrange(angc - anga)`, range ±π) for 2-axis frames and AST__BAD when either vector has zero length or any input is bad.
  Rust (`basic.rs:534`) always returns the unsigned `acos` form and returns `0.0` for zero-length vectors.

- **[HIGH] Frame — `offset` coincident-point behavior.** *(FIXED 2026-06-11.)*
  C `Offset` (`frame.c:8470-8490`) returns all-AST__BAD output when the two points are coincident and the requested offset is non-zero (and `point1` when offset is zero).
  Rust (`basic.rs:558`) returns `from` unchanged in both cases.

- **[HIGH] Frame — Format attribute is not printf-style and Digits is absent.** — **FIXED.**
  C formats through the Axis Format attribute, a full printf specifier with default `"%1.<Digits>G"` (`axis.c GetDefaultFormat`, Digits default 7) including integer-cast handling for `%d`. The printf-format interpreter is ported as `frames/axis_format.rs::format_value` (the `%G`/`%E`/`%f`/`%d` subset) with the Digits-derived default, and `BasicFrame` carries per-axis Format/Digits state (section 0 item 7). The leaf spectral/flux divergence was closed 2026-06-14: `SpecFrame`/`FluxFrame` had hand-rolled `format` overrides (fixed-decimal precision, and SpecFrame *appended units* — neither matches C, which uses `%1.7G` with no units) even though **C does not override Format for SpecFrame, FluxFrame or DSBSpecFrame** (no `static int Format` — they inherit the base). Fixed by mirroring C's inheritance: the `Frame` trait now carries a default `format` (validate axis → `<bad>` token → `effective_format`/`axis_digits` → `format_value`), `BasicFrame::format` and the `SpecFrame`/`FluxFrame`/`DSBSpecFrame` overrides were deleted so all inherit it (`SpecFluxFrame` keeps CmpFrame-inherited per-axis routing; `CmpFrame`/`TimeFrame`/`SkyFrame` keep their genuine overrides). Oracle-verified for all five classes (`1.5e9`→`1.5E+09`, `1/3`→`0.3333333`, `1e-26`→`1E-26`, BAD→`<bad>`). The pre-existing `test_format_freq` had asserted unit-appended output (a Rust mistake, not a C-bug workaround) and was corrected to the C result. Tests: `tests/test_specframe.rs`, `tests/test_fluxframe.rs`.

- **[MED] Frame — Digits and Direction attributes do not exist.** — **FIXED.**
  `BasicFrame` carries per-axis Format/Digits state and `frames/axis_format.rs`
  consumes the Digits-derived default (section 0 item 7); CmpFrame per-axis
  Direction routes to the owning component (section 6 CmpFrame item). Any
  remaining `Direction` consumers are Plot-only and tracked with the Plot work.

- **[MED] Frame — `unformat` error/whitespace contract differs.** — **FIXED 2026-06-14.**
  C `astUnformat` (`frame.c:11819` → `axis.c:1284`, `sscanf("%lf %n")`) returns nread=0 *without raising an error* on parse failure, counts leading and trailing whitespace in nread, and recognises the `<bad>` token. `BasicFrame::unformat` already matched this; the gap was that `SpecFrame`/`FluxFrame` carried hand-rolled `unformat` overrides (dropping trailing whitespace from nread, not handling `<bad>`) even though **C does not override Unformat for either class** (no `static int Unformat` in specframe.c/fluxframe.c — they inherit the base). Fixed by mirroring C's inheritance: the shared base contract is now a free fn `basic::axis_unformat`, the `Frame` trait carries a default `unformat` (validate axis → `axis_unformat`), and the spurious overrides in `specframe.rs`/`fluxframe.rs`/`dsbspecframe.rs` were deleted so all four inherit it (`SpecFluxFrame` keeps its CmpFrame-inherited per-axis routing override, cmpframe.c:10041). `CmpFrame`/`TimeFrame`/`SkyFrame` keep their genuine C overrides. Oracle-verified against the built reference libast for all five classes: `"  12.5  "`→(12.5, 8), `"<bad>"`/`" < b a d > "`→(BAD, 5/11), `"12.5xyz"`→(12.5, 4), unparseable→(BAD, 0). Tests: `tests/test_specframe.rs`, `tests/test_fluxframe.rs`, `tests/test_dsbspecframe.rs`.

- **[MED] Frame — geometry/axis method family** — **FIXED 2026-06-11.**
  The Cartesian base implementations now exist as `Frame` trait methods in `crates/ast-core/src/frames/mod.rs`: `offset2`, `intersect`, `ax_distance`, `ax_offset`, `ax_angle`, `ax_norm`, `resolve`, `resolve_points` (with the signed 2-D perpendicular component), `match_axes`/`match_axes_x`, `is_unit_frame`, and `norm_box` (base no-op). `ax_angle` mirrors the C stale-loop-index quirk bit-for-bit (logged in `docs/issues/c-library-quirks.md`); `match_axes` was oracle-confirmed against `astMatchAxes` (CmpFrame(Spec,Sky) → `[2,3]`).
  Still deferred: `NormalPoints`/`NormPoints` (PointSet-batch Norm; no current core consumer), the SkyFrame `norm_box` `reg` argument (great-circle `offset2`, `resolve` and `intersect` are ported; the last two closed 2026-09-30) with the C `reg` Mapping argument — `norm_box` will gain the `reg` parameter when that override lands), and the capi shims for all of these.
  SkyFrame is supposed to override several of these (`skyframe.c`), so the base implementations are prerequisites for the SkyFrame audit items.

- **[MED] Frame — ActiveUnit / unit adaptation** — **FIXED 2026-06-12.**
  `get_active_unit`/`set_active_unit`/`test_active_unit` are now `Frame` trait methods reconciled with C: the base default is false (`frame.c:4651`); SkyFrame forces false (SkyAxis present); SpecFrame/FluxFrame force true (`frame.c:9882`). Serialization writes `ActUnt` only when explicitly set, with its actual value (`frame.c:14614`).
  `default_sub_frame` now ports the SubFrame units tail (`frame.c:11077-11154`): when both template and result use active units, it overlays the template's per-axis Unit onto the result (`axis.c:1270`) and composes a parallel `astUnitMapper`-derived CmpMap so unit differences become real conversions; the units map is added in series after the permutation map when it is not a pure UnitMap.
  `overlay_template` transfers the ActiveUnit flag (`frame.c:8578`).
  Deferred: the `NewUnit` Label-rewrite / Format-clear (`frame.c:7564`), which fires when ActiveUnit is set and an axis Unit actually changes during Overlay. The current `overlay_template` route does not change per-axis Units outside the active-units SubFrame tail, so `NewUnit` is not reachable here yet; it lands when general per-axis Unit overlay is wired through the active-units path. The `astGet/SetActiveUnit` capi shims are also still deferred.

- **[MED] Frame — `astPrimaryFrame`** — **FIXED 2026-06-11.**
  `primary_frame` is now a `Frame` trait method (leaf base returns `(self, axis)`; CmpFrame descends through its perm, SkyFrame applies its axis permutation). The two private fitschan copies in `make_int_world.rs` and `celestial_axes.rs` were deleted and now call the trait method directly; `specframe::primary_frame_walk` delegates to it. Behaviour verified unchanged via `test_fitschan_wcs` and the simplify regression suite.

- **[LOW] Frame — `format` of AST__BAD returns `"<bad>"`.** — **FALSE POSITIVE (closed 2026-06-14).**
  The finding claimed C `AxisFormat` has no bad-value special case; in fact `axis.c:706` returns `"<bad>"` for `value == AST__BAD` before any Format processing. The Rust `"<bad>"` token matches C exactly (oracle-verified). The trait-default `format` (see the Format item above) preserves it.

- **[LOW] Frame — `IsUnitFrame`** — **FIXED 2026-06-11** (base only).
  `is_unit_frame` is now a `Frame` trait method returning `true` for the base Frame (`frame.c:6417`). The specialised-frame overrides that return `false` are deferred until the Frame-as-Mapping/Region simplification paths consume the result.

- **[DEFERRED] Frame — Plot-support methods missing: `astGap`, `Centre`, `Abbrev`, `Fields`, `FrameGrid`, `LineDef`/`LineContains`/`LineCrossing`/`LineOffset`.**
  All have zero Rust implementations; they are exercised mainly by Plot (delivery step 10) and Polygon/Region edge logic, so schedule them with that step (note `regions/polygon.rs` currently carries its own geometry).

## 6a. Detailed findings: Mapping base (audited 2026-06-10)

Compared `mapping.c` against `mappings/mod.rs`, `rate.rs`, `map_box.rs`, `linear_approx.rs`.
`map_box.rs` mirrors the C helper decomposition (Random/MapFunction/NewVertex/UphillSimplex/LocalMaximum/SpecialBounds/GlobalBounds) with identical constants (default_acc 3.0e-5, maxiter 10000, minsame 5, nbatch 32, simplex factor 3.0, maxcall 1500) — no findings beyond the spot check.
`linear_approx.rs` mirrors the C sampling phases (face centres, 0.49/0.51 nudges, vertex + half-way test points) and the per-point tolerance test including C's early-`break` quirk, with oracle fixtures — one finding below.

- **[HIGH] Mapping — `rate` abandons the interval search on a failed interval.** *(FIXED 2026-06-12.)*
  C `Rate` (`mapping.c:9040-9100`) keeps scanning larger/smaller interval sizes when `FindGradient` returns AST__BAD for one interval (the `if (mean != AST__BAD)` guard skips storing but the loop continues).
  Rust (`rate.rs`) used to `break` out of both search loops; both now `continue`, and a spurious Rust-only `iin>NN+1`/`iin<NN-1` guard on the zero-range branch was removed. Latent (smooth Mappings grow range monotonically with h, so natural sweeps were already bit-identical to the oracle), but the control flow now matches C exactly.

- **[MED] Mapping — `rate` initial interval for x0 = 0 is wrong.** *(FIXED 2026-06-12.)*
  C seeds `h0 = 1.0` when the evaluation position is zero (`mapping.c:9009`); Rust used `f64::EPSILON * 1e9` ≈ 2.2e-7, starting the adaptive search ~7 orders of magnitude too small. Now seeds `1.0` to match C — which means `astRate` at `x0=0` for a domain-restricted Mapping (ChebyMap on `[-1,1]`) returns BAD as the first interval overflows the domain, and `tan` at 0 returns a small wrong value; both are confirmed C bugs now in `c-library-quirks.md`.

- **[LOW] Mapping — `rate` zero-range confirmation indexing.** *(FIXED 2026-06-12.)*
  C reads `y[iin-1]`/`y[iin+1]` indexed by the loop counter even though stores go to `itop`/`ibot`, so after skipped (bad) intervals it can read stale slots — a C quirk. Rust now indexes `ranges[iin-1]`/`ranges[iin+1]` exactly as C does (documented in `c-library-quirks.md`); Rust's zero-initialised array can diverge from C's uninitialised stack only in the pathological skipped-interval case, which is unreachable for well-behaved Mappings.

- **[MED] Mapping — `linear_approx` all-outputs-BAD case inverted.** *(FALSE POSITIVE — closed 2026-06-12.)*
  The original finding claimed C returns *success* (`result=1`) with a unit transform when every output is BAD at the face centres. Oracle-verified against cmake-built `libast`: C actually returns `result=0` with an **all-BAD** fit — the unit-transform fill at `mapping.c:6952-6966` is immediately overwritten by the trailing `if(!astOK || !linear)` at `mapping.c:6972` because `linear` is still 0. The existing Rust `None` already matched the oracle; only the misleading comment in `linear_approx.rs` was corrected. (The SUN/211 doc note about returning the unit transform is itself inaccurate — a candidate for the quirks discussion.)

- **[FIXED] Mapping — `IsSimple` attribute.**
  The flag previously named `do_not_simplify` *was* C's `IsSimple` (set only by `simplify()`, serialised as the `IsSimp` token, round-tripped through `from_node`). Two corrections landed: it is renamed `has_been_simplified` so the field reads as "this mapping is already the simplified form" rather than "may not be simplified"; and `invert()` now clears it, mirroring C's `astSetInvert`/`astClearInvert` clearing `IsSimple` (oracle-confirmed 1→0 on invert) so the inverse re-simplifies. Deserialisation restores the raw `Invert` and `IsSimp` fields independently — every mapping `from_node` applies `invert()` *before* restoring `has_been_simplified`, so loading a dump preserves the stored flag instead of having the live `invert()` reset it.
  `astDecompose` (splitting a compound mapping into its two components) remains deferred — it is an ast-capi-layer public surface with no current Rust-core consumer; add it there when one appears. `Report` (debug tracing) likewise stays unported.

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

- **[MED] FrameSet — variants are a stub.** — **FIXED 2026-06-14 (oracle-matched, both directions).**
  The full Variants subsystem is ported (frameset.c): `add_variant` (with-Mapping append and `None` rename, including the mirror-frame `Mirro` and duplicate-name `Bdvnm` rejections), `variant`/`set_variant`/`clear_variant`/`test_variant` (the `Variant` attribute, with the node-Mapping recompose on selection done via the existing `remap_frame` edge machinery), `all_variants`, `mirror_variants`, the `varfrm` mirror-chain resolver (`get_var_frm`), and `remove_frame`/RemoveMirrors `varfrm` bookkeeping. **Storage divergence (documented)**: C keeps the Variants FrameSet on the Frame itself (`frame.h:636`); since the 18 concrete Rust frame structs share no base struct and the variant API only ever acts on a FrameSet's current frame, the port holds variants FrameSet-side keyed by frame index (`frame_variants: Vec<Option<FrameSet>>` + `varfrm: Vec<usize>`). A bare `Frame` carrying variants outside a FrameSet is therefore unsupported — not in the C public test surface; logged in `c-library-quirks.md`. **Serialisation matches C byte-exact**: the `Vrnts` object is written into / read from each member Frame's node and `VarFrm<i>` ints on the FrameSet, exactly as C's per-Frame storage dumps. Oracle-verified bidirectionally: C reads the Rust dump and Rust reads a C-generated dump with identical AllVariants / Variant / transforms (testframeset.c errors 1-39: FP2→(150,150), FP1→(0,0), FP3→(-150,-150)). Tests: `tests/test_frameset_variants.rs` + the C fixture `tests/fixtures/frameset/c_variants.ast`.

- **[MED] FrameSet — Mapping-side overrides missing.** *(FIXED: `has_forward`/`has_inverse`/`rate` delegate to the base→current mapping (2026-06-11); `GetIsLinear` and `RemoveRegions` are `is_linear` and `mapping_without_regions`; `MapSplit` is `map_split_zero_based` (2026-10-09), which `testmoc.c`'s pixel mask needs to split the sky axes from a 3-D FITS FrameSet; `test_map_split.rs::frameset_splits_its_base_to_current_mapping`.)*
  C overrides `GetTranForward`/`GetTranInverse` (forward to the base→current path), `GetIsLinear`, `MapSplit`, `Rate` (delegates to the composed mapping so analytic rates apply), and `RemoveRegions` (`frameset.c:877-906`).
  Rust `impl Mapping for FrameSet` relies on trait defaults: `has_forward`/`has_inverse` always return `true` (wrong when the path contains a one-way mapping), and `rate` always uses generic `numeric_rate`.

- **[LOW] Region — `MapSplit` is not frame.c's.** A Region inherits frame.c `MapSplit` (frame.c:7002), which picks the selected axes with `astPickAxes` and returns them as the split Mapping, with the same axes as outputs.
  The port's Regions use the base `map_split_zero_based`, which splits only a selection of every input.
  Latent: exposed by splitting part of a Region used as a Mapping, which no test or caller does yet. `astPickAxes` can return a plain Frame, which the port cannot hold as a Mapping (PLAN.md §9.6), so the port needs that decision first.

- **[MED] FrameSet — `convert_to` ignores the other member frames.** — **FIXED 2026-06-14 (oracle-matched).**
  Rust previously forwarded to the current frame only; C `FrameSet::ConvertX` (frameset.c:3201) considers *every* member frame of both the `from` and `to` FrameSets (base first, then index order, current last) as a candidate alignment endpoint, prefixing the `from`-current→member mapping and appending the `to`-member→current mapping, simplifying, and scoring by transform availability (forward+inverse=3, forward=2, inverse=1). Ported as `frameset_convert_to` (modelled on the existing one-sided `frameset_find_frame`), reusing the `find_frame::convert_frames` per-pair driver. So a conversion only possible (or better-scoring) via a non-current member now succeeds. Oracle-verified against the reference C library: a FrameSet (base=FK5 SkyFrame, current=PIXEL, linked by UnitMap) converts to a Galactic SkyFrame via its FK5 base member, mapping (0.1, 0.2) rad → (1.951902536797, -0.886577028810) rad — identical to C, both directions defined. Tests: `tests/test_frameset_repair.rs`. Deliberate divergence: C also sets the input FrameSets' Base attribute to record the route used (frameset.c:3502-3503); the Frame trait takes `&self`/`&dyn Frame` and cannot mutate the inputs, so that side-effect (an optimisation hint, not part of the result) is omitted.

- **[LOW] FrameSet — `astAddFrame(AST__ALLFRAMES, map, frame)` mode missing.** — **FIXED 2026-06-14 (oracle-matched).**
  Correction to the original finding: AllFrames mode does **not** remap frames through a PermMap or reorder axes. Per C (`AppendAxes`, frameset.c:1943, invoked at frameset.c:1336), the supplied `map` is **ignored** and the supplied frame's axes are *appended* to every frame in the set — each Frame becomes a `CmpFrame(original, frame)` and each internal Mapping a parallel `CmpMap(original, UnitMap(naxes))`, simplified. Ported as a new `FrameRef::AllFrames` variant routed through `FrameSet::add_frame` to a private `append_axes` (frameset.rs); `FrameRef::AllFrames` is rejected (`Frmin`) where a single frame is required. Oracle-verified: a 2-axis GRID→FPLANE WinMap FrameSet widened by a 1-axis SPEC frame becomes a 3-axis set mapping (5,5,42) → (40,40,42), identical to the reference C library. Tests: `tests/test_frameset_repair.rs`.

## 6c. Detailed findings: CmpFrame (audited 2026-06-10)

Compared `cmpframe.c` against `frames/cmpframe.rs`.
The Match/SubFrame machinery (PartMatch, GenAxisSelection, GoodPerm analogues in `cmpframe_match`/`partition_selection`/`cmpframe_sub_frame`) is a genuine port with good test coverage, and `norm`/`perm_axes` handle the perm array correctly.
The gaps are in the geometry methods, attribute defaults, and attribute forwarding.

- **[HIGH] CmpFrame — `distance`/`angle`/`offset` are Cartesian-only.** *(FIXED 2026-06-11.)*
  C projects points into the component frames through the perm array and delegates: `Distance` (`cmpframe.c:2475-2560`) combines the two component distances in quadrature, `Offset` (`cmpframe.c:6163-6164`) splits the offset per component, and `Angle` (`cmpframe.c:1170-1240`) uses the Cartesian parent implementation *only* when every axis's primary frame is a plain `Frame`, otherwise the component path.
  Rust (`cmpframe.rs:214-265`) computes raw Euclidean math over the external coordinates for all three, giving wrong results whenever a component is a SkyFrame (the standard sky+spectral cube case).
  `Resolve` (`cmpframe.c:7753`) and `ResolvePoints` (`cmpframe.c:7962`) are component-aware; both ported 2026-09-30 and pinned against C by `tests/test_sky_geometry.rs`.

- **[HIGH] CmpFrame — MinAxes/MaxAxes defaults are wrong.** *(FIXED 2026-06-11.)*
  C defaults are `0` and `1000000` when unset (`cmpframe.c:3680-3760`), so a CmpFrame template matches targets of any axis count; Rust defaults both to `naxes()` (`cmpframe.rs:319-336`, with a comment acknowledging the divergence).
  This silently disables the C behavior where `astFindFrame` with a CmpFrame template can match larger or smaller targets via PartMatch's extra-axis handling.

- **[HIGH] CmpFrame — Domain default is empty.** *(FIXED 2026-06-11.)*
  C `GetDomain` (`cmpframe.c:3595-3634`) returns `"<DOM1>-<DOM2>"` (component domains joined with a hyphen, recursing through nested CmpFrames) or `"CMP"` when both are blank; Rust returns `""` (`cmpframe.rs:194`).
  Domain strings drive `find_frame` domainlist gating and FITS-WCS bookkeeping, so e.g. `"SKY-SPECTRUM"` lookups fail.

- **[MED] CmpFrame — Epoch/ObsLat/ObsLon/ObsAlt/Dtai not forwarded to components.** *(FIXED 2026-06-11 via per-class downcast helpers — the Frame trait lacks epoch/observer setters, so BasicFrame (year-only setter) and FluxFrame components are skipped; revisit when the trait grows setters.)*
  C `SetEpoch` (`cmpframe.c:8800-8812`) sets the parent value *and* both components (same pattern for the Obs\*/Dtai setters), and `GetEpoch` (`cmpframe.c:3917`) falls back to frame1's value (then frame2) when unset on the CmpFrame itself.
  Rust CmpFrame has no epoch accessor and inherits the `None`-returning trait defaults for `obs_lat`/`obs_lon`/`obs_alt`/`dtai`, so observer/epoch context set at the compound level never reaches a SkyFrame/SpecFrame component.

- **[MED] CmpFrame — per-axis attribute routing.** *(PARTIALLY FIXED 2026-06-13.)*
  C routes Get/Set/Test/Clear of Label, Symbol, Unit, Format, Direction per external axis through the perm array to the owning component (vtab `MAKE_GET/SET/CLEAR/TEST` for Direction/Format/Label/Symbol/Unit, cmpframe.c:10243-10340). **C does NOT override Bottom/Top** for CmpFrame — those use the parent Frame defaults (±DBL_MAX), so the original finding listing Bottom/Top here was wrong.
  Now routed via `route_axis`: the read getters `effective_label`/`effective_symbol`/`effective_unit`/`effective_format` (pre-existing) and **`direction(axis)`** (added 2026-06-13, oracle-verified — `tests/test_cmpframe.rs::cmpframe_direction_routes_to_component`).
  Still missing: strongly-typed *settable* per-axis accessors (`set_label(axis,…)`, `set_symbol`, `set_unit`, `set_format`, `set_direction`) plus their per-axis Test/Clear, routed through `route_axis`. These are a pure API-surface addition (no failing behaviour — conversion-based equality and internal logic use the `effective_*` getters), tracked for a follow-up.

- **[LOW] CmpFrame — Title default.** *(Already correct — finding was stale.)*
  C `GetTitle` (`cmpframe.c:4424+`) defaults to `"<N>-d compound coordinate system"`. Rust's `title()` returns `""` when unset (matching the codebase convention shared with `BasicFrame::title`), but `Serializable::to_node` already emits the computed `"{naxes}-d compound coordinate system"` default (cmpframe.rs to_node) — oracle-verified (`3-d`/`5-d compound coordinate system`).

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

- **[MED] SkyAxis — Format attribute and Digits-driven precision unsupported.** *(FORMATTING PORTED 2026-09-30; unformatting and Plot methods remain.)*
  Rust `format_sky` hardcoded precision 1 (time) / 0 (degrees), which happens to equal the C defaults at Digits=7 (`skyaxis.c GetAxisFormat`: "h"/"hm"/"hms"/"hms.N" by Digits), but there was no per-axis Format string (C accepts specifiers like `+zhms.2`, `g`, `d`, `lbi`, separator control), no Digits attribute, and no `AxisGap`/`AxisAbbrev`/`AxisFields`.
  Now ported: Format, Digits (Frame and per axis), AsTime and Unit on SkyFrame, with `ParseDHmsFormat`, `DHmsFormat`, `DHmsUnit` and the default-Format rules of `GetAxisFormat` and `SkyFrame::GetFormat`, their Native dump/load and Overlay, all checked against C in `tests/test_skyframe_format.rs`.
  Closed 2026-09-30: `AxisUnformat` reads the Format and a SkyAxis picked into a plain Frame stays a SkyAxis (`PLAN.md` §2). Still open: `AxisGap`/`AxisAbbrev`/`AxisFields` wait for Plot.

- **[DEFERRED] SkyFrame — spherical overrides of the missing Frame-method family.**
  `Gap`, `GetDirection`, `Offset2`, `Resolve`/`ResolvePoints`, `Intersect`, `NormBox`, `NormalPoints`, `FrameGrid`, `LineDef`/`LineContains`/`LineCrossing`/`LineOffset`, `MatchAxesX` all have SkyFrame-specific great-circle implementations in C and no Rust counterpart; schedule with the Frame-base family (section 6) since the base methods are prerequisites.
  Closed 2026-09-30: `Resolve`/`ResolvePoints`, `Intersect` and `MatchAxesX` (`PLAN.md` §2).

## 6e. Detailed findings: SpecFrame + DSBSpecFrame (audited 2026-06-10)

Compared `specframe.c`/`dsbspecframe.c` against `frames/specframe.rs`, `frames/dsbspecframe.rs`.
`make_spec_mapping_full` (`specframe.rs:1255`) is a faithful line-referenced port of C MakeSpecMapping steps 2-7 (system→freq, SOR chains through heliocentric, alignment-system relativistic steps, unit wrapping at both boundaries), the SOR conversion argument lists match C's TRANSFORM_n macros exactly (TPF2HL lon/lat/alt/epoch/ra/dec etc.), Match/SubFrame/Overlay have tests, and the DSB attribute set (DSBCentre 3.0e10 / IF 4.0e9 defaults, sideband mappings, ImagFreq, AlignSideBand) is present.
Confirmed divergences:

- **[HIGH] SpecFrame — SpecOrigin/AlignSpecOffset never applied in conversions.** *(FIXED 2026-06-11, oracle-matched.)*
  C MakeSpecMapping wraps the step-2-7 chain with step 0 — prepend `ShiftMap(+target_origin)` when `GetSpecOriginCur` is non-zero (`specframe.c:2937-2945`) — and step 9 — append `ShiftMap(−result_origin)`, where the result origin is the *transformed target origin* when both frames have AlignSpecOffset set (offset-system alignment, `specframe.c:2947-2965`).
  Additionally `GetSpecOriginCur` (`specframe.c:2129`) converts the stored origin to the frame's current system/units, and `SetSystem`/`ClearSystem` rescale the stored SpecOrigin to the new system (`specframe.c:643,682`).
  Rust stores `spec_origin` (used only by overlay and serialisation — `specframe.rs:1971,2316`) and `align_spec_offset`, but `make_spec_mapping_full` has no step 0/9 and `set_system` does not rescale, so offset spectral frames (a SMURF/SCUBA-2 staple) convert wrongly.

- **[MED] DSBSpecFrame — DSBCentre/IF attribute-interface conversions.** *(TYPED METHODS ADDED 2026-06-13; capi wiring still TODO.)*
  C's attribute layer converts DSBCentre to/from the frame's *current spectral system* via TopoMap on every string get/set (`dsbspecframe.c:459-483, 1355-1370`) and expresses IF in GHz externally (÷1e9 on get, ×1e9 on set); the internal values are topocentric Hz.
  Rather than push the conversion onto callers (cf. [[feedback_typed_setters_no_string_layer]]), typed convenience methods now expose both representations on `DSBSpecFrame`:
  - `dsb_centre_cur()` / `set_dsb_centre_cur()` — DSBCentre in the current spectral System, via the inverse / forward of `to_topo_mapping()` (the Rust `TopoMap` analogue; mirrors C `astGetAttrib`/`astSetAttrib` for "DSBCentre").
  - `if_ghz()` / `set_if_ghz()` and `imag_freq_ghz()` — the GHz external representation of IF / ImagFreq.
  The raw `dsb_centre()` / `ifr()` getters now document that their unit is **topocentric Hz**. Tests: `tests/test_dsbspecframe.rs` (round-trip on a topocentric frame; non-topocentric frame shows the expected velocity shift, matching the C oracle's `30 GHz → 29.997 GHz`).
  **Still TODO:** the ast-capi string-attribute dispatch for DSBSpecFrame does not exist yet (no DSBSpecFrame surface in `ast-capi`); when it lands, `astGetC("DSBCentre")` / `astGetC("IF")` / `astGetC("ImagFreq")` should call these typed methods for byte-exact parity.

- **[LOW] DSBSpecFrame — Domain is explicitly set instead of defaulted.** — **ALREADY CORRECT (verified 2026-06-14).**
  The finding was stale: `DSBSpecFrame::new` does *not* set Domain; `domain()` returns "DSBSPECTRUM" only as a default (mirroring C `GetDomain`, dsbspecframe.c:531) and `test_domain()` stays false until Domain is explicitly set. Oracle-verified against the built reference libast (default Domain="DSBSPECTRUM", TestDomain=0; after set, TestDomain=1). Regression test added: `tests/test_dsbspecframe.rs::domain_default_is_unset`.

## 6f. Detailed findings: TimeFrame + FluxFrame + SpecFluxFrame (audited 2026-06-10)

Compared `timeframe.c`/`fluxframe.c`/`specfluxframe.c` against the corresponding `frames/*.rs`.
TimeFrame's `make_time_mapping` covers all 11 timescales (TAI/UTC/UT1/GMST/LAST/LMST/TT/TDB/TCB/TCG/LT), applies TimeOrigin on both sides, passes LTOffset in hours, negates ObsLon per the TimeMap convention, and threads Dut1/Dtai; Match/SubFrame/Overlay have tests.
The numerical TDB↔UTC drift questions are already tracked in `docs/issues/timeframe-and-epoch-api-port.md` (leap-second constants, Fairhead-Bretagnon precision, Epoch/Equinox timescale documentation) and are not repeated here.

- **[MED] TimeFrame — iso-date Format/Unformat.** *(FIXED 2026-06-13.)*
  C overrides `Format`/`Unformat` so that `Format="iso.N[T]"` renders and parses calendar date strings (`timeframe.c` Format:1597 / Unformat:6062 / DateFormat:813). Fully ported:
  - The bare-number default routes through `axis_format::format_value("%1.7G", …)` (7 significant digits).
  - The vendored pal calendar helpers `palDjcl`/`palCldj`/`palDd2tf`/`palDtf2d` are exposed in `crates/ast-core/src/pal.rs` as `djcl`/`cldj`/`dd2tf`/`dtf2d` (C retained and wrapped, not reimplemented).
  - `frames/date_time.rs::read_date_time` ports `astReadDateTime` (frame.c:8892): B/J epochs, decimal years (IAU-1984), MJD, JD, and Gregorian dates with named months / `-`/`/`/`:`/space / `T` separators / decimal-day or h:m:s fractions.
  - `TimeFrame` gained per-instance Unit and Format storage; `make_map` mirrors C `MakeMap` (timeframe.c:3505) with `unit_mapper` bracketing so `unit=s` etc. convert correctly; `date_format`/`value_to_mjd`/`mjd_to_value` drive the iso Format/Unformat; the axis label becomes `Date/Time`/`Date` under a date Format.
  - Oracle-matched against `testtime.c` errors 26-34 and a byte-identical native dump for `unit=s,format=iso.2`.
  - **Withdrawn "C bug" (not a bug)**: the Besselian-epoch `astUnformat` discrepancy was traced to a missing `TimeOrigin` re-expression in the Rust port (C `OriginScale`/`OriginSystem`), not a C defect — the C value is correct for the construction order used. Fixed by `origin_scale`/`origin_system` on `set_system`/`set_timescale`/`clear_*`; see `c-library-quirks.md` (RESOLVED entry) and `timeframe-unformat-findings.md`.
  The GetTitle default (timeframe.c:3107) is also ported: system label + ` [<TimeScale>]` (set-gated, BEPOCH-excluded, with ` (UTC±<g>)` for Local Time) + ` offset from <iso.0 date>` when a non-zero TimeOrigin is set and the Format is not a date string — oracle byte-matched. Remaining (Plot-only): `Gap`/`Centre`/`Abbrev`/`Fields` for tick labelling.

- **[MED] TimeFrame — Epoch default ignores TimeOrigin.** *(FIXED 2026-06-13.)*
  When Epoch is unset but TimeOrigin is set, `epoch_mjd` now returns the TimeOrigin converted to an absolute TDB MJD (earth-rotation timescales routed via UT1 with DUT1==0), mirroring C `GetEpoch` (timeframe.c:2458); the native dump's unset-Epoch default reflects it. Tests in `tests/test_timeframe.rs`.

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

- **[MED] CmpMap — `rate` and C's chain rule.** *(RESOLVED — no change needed, 2026-06-13.)*
  C defines a chain-rule `Rate` override at `cmpmap.c:2997-3157` (series: sum over intermediate axes of `r1*r2` via each component's `astRate`; parallel: dispatch to the owning component, exact `0.0` cross-component) — **but it is never installed**. `astInitCmpMapVtab` comments out `mapping->Rate = Rate` (`cmpmap.c:887-893`) with the note that the override "can be immensely slow for complex Mapping". So every `astRate` on a CmpMap in C runs the inherited base `Mapping::Rate`, i.e. numerical differentiation of the whole composition — which is exactly what Rust's `numeric_rate` already does.
  - **Oracle-confirmed by instrumentation:** `astRate_` dispatch reaches `class=CmpMap` but the CmpMap `Rate` body is never entered; the rate of the `vopt-geo` X→S CmpMap is `-2.11061140544971257e8` in C and **bit-identical** in Rust's `numeric_rate`.
  - **The earlier "AIPS card mark/sweep bug" was a misdiagnosis.** There is no card-sweep ordering bug. C recomputes CDELT3 as `999.99999888049786` (the rate residue of the not-exactly-linear FELO↔VOPT round-trip), which fails `astEQUAL` against the input `1000.0` (diff ~1.1e-6 ≫ tol ~4.4e-8), so C rewrites the comment to "Pixel size". Rust's `numeric_rate` gives `999.9999988804965` — same outcome, same comment. `card_list.rs::put_or_append` already matches C's `SetFits` comment-retention exactly.
  - **The deferred chain-rule port was the problem, not the fix.** A faithful port of the *disabled* override produces dsbydx `-2.11061140781252474e8` (diverging from C at the 9th figure), yielding CDELT3 `999.9999999999882`, which *passes* `astEQUAL` and wrongly retains the stale "Channel width (m/s)" comment. Porting the override therefore diverges from the oracle.
  - **Resolution:** `cmpmap.rs::rate` keeps `numeric_rate` (mirrors C's installed behaviour) with a TODO noting the override could be revisited for speed/accuracy only if we deliberately diverge from C. Logged in `docs/issues/c-library-quirks.md`.

- **[MED] Simplify — pairwise-adjacent protocol vs C's list-based MapMerge.** — **VERIFIED BEHAVIOURALLY IDENTICAL 2026-06-14 (one latent SlaMap bug found and fixed).**
  C nominates each mapping and hands its class MapMerge the *entire* map list plus position, accumulating an arbitrary run of adjacent same-class neighbours (SlaMap/SpecMap/TimeMap) into one combined step list before cancelling. Rust `merge_loop` (`simplify.rs`) instead merges adjacent pairs and relies on repeated passes to collapse longer runs. To check the two algorithms agree (rather than just trusting that no fixture failed — the C public harness *can't* easily build these run cases), oracle fixtures were generated for runs of 3-4 adjacent SpecMap/TimeMap/SlaMaps including an inverted middle map and partial cancellations: `spec_run4_identity`, `spec_run_inverted_mid`, `time_run_identity`, `sla_run_identity`, `sla_run_partial` (`.map` inputs C-canonicalised, `.simp` from the reference `astSimplify`). All match — the pairwise engine reaches the same result as C's list accumulation.
  This exposed two genuine latent divergences in **SlaMap native serialisation** (no prior fixture exercised a BAD SlaMap argument): (1) `from_node` defaulted a missing conversion argument to `0.0` instead of `AST__BAD` (C `astLoadSlaMap`, slamap.c:4986 reads with `AST__BAD`; SpecMap/TimeMap already used `BAD_VALUE`) — fixed in `slamap.rs`; (2) the native writer rendered a BAD double as the raw `-DBL_MAX` literal instead of the `<bad>` token C `astWriteDouble` emits (channel.c:146) — fixed centrally in `serial/native_write.rs`/`native_read.rs` so any BAD double round-trips as `<bad>` (SpecMap still *skips* BAD args per its C Dump; SlaMap writes them, per its C Dump). The list-based `try_merge_list` refactor is therefore unnecessary for correctness.

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

- **[LOW] PolyMap — zero-coefficient test uses a 1e-30 threshold.** — **FIXED 2026-06-14.**
  C drops only exactly-zero coefficients when shuffling merged terms (`polymap.c:3681`, `outcof[ic] != 0.0`); Rust's `simplified()` consolidation now mirrors that with `consolidated.retain(|t| t.coeff != 0.0)` instead of `> 1e-30`, so a legitimately tiny coefficient survives as it does in C.

- **[LOW] ChebyMap — bounding-box test differs from C.** — **FIXED 2026-06-14.**
  C marks a point bad when the *scaled* coordinate satisfies `fabs(x*scale + offset) > 1.0` (`chebymap.c:1170-1174`); Rust compared the raw input against lbnd/ubnd directly (with a misleading "small tolerance" comment). `eval_cheby` now applies the scaling first and tests `x_norm.abs() > 1.0` (equivalent to C's `!(fabs(x) <= 1.0)` since non-finite inputs are already rejected upstream), so the two agree to the last bit at the box boundary. `lbnd`/`ubnd` are retained in the signature but no longer consulted for the bounds test.

- **[Cross-ref] `astPolyTran` (and ChebyMap's override of it) is the section 1 headline gap**; the fitschan workaround at `wcs_matrix.rs:205` is the first consumer to unwind after porting.

## 6i. Detailed findings: linear family (audited 2026-06-10)

Compared `matrixmap.c`/`permmap.c`/`shiftmap.c`/`winmap.c`/`zoommap.c`/`unitmap.c`/`pcdmap.c` against the corresponding `mappings/*.rs`.
Healthy overall: every class has an analytic `rate` override and a `map_split` (closing most of old-audit item 5.1 for this family), MatrixMap tracks full/diagonal/identity forms and correctly refuses inverse transforms of singular or non-square matrices (`Nodef`), PermMap's positive/negative/zero permutation-entry semantics match C, PcdMap mirrors the SLA_UNPCD series (noted in `c-library-quirks.md`), and ZoomMap rejects zero factors.
Merge-rule completeness for this family is tracked separately (old audit sections 3/4 + `simplify-serialization-gaps.md`).
Findings:

- **[HIGH, joins the 6g/6h flag bucket] MatrixMap — `has_forward`/`has_inverse` not overridden.** *(FIXED 2026-06-10.)*
  A singular or non-square MatrixMap stores `inverse: None` and errors on inverse transform, but still reports `has_inverse() == true` via the trait default; C `GetTranInverse` reports whether the inverse matrix exists.
  Fix together with CmpMap/PolyMap/ChebyMap.

- **[MED] MatrixMap — `astMtrRot`/`astMtrMult` missing.** *(METHODS PORTED 2026-06-13; fitschan unwind deferred.)*
  The protected C methods for multiplying (`MtrMult`, matrixmap.c:3526 — returns `a × this`) and rotating (`MtrRot`, matrixmap.c:3766 — post-multiplies the effective forward matrix by a 2-D clockwise or 3-D axis-angle rotation) a MatrixMap are now ported as `MatrixMap::mtr_mult` / `MatrixMap::mtr_rot`, including the bad-value short-circuit, the full-turn no-op (`cos θ == 1`), and the `Mtrml`/`Mtrax`/`Mtr23` error cases. Tests: `tests/test_matrixmap.rs` (verified analytically against the documented rotation/product formulae — the C entry points are protected, not in the public `ast.h`, so an oracle binary can't call them directly; the 2-D `[[cos,sin],[-sin,cos]]` and 3-D Rodrigues sign conventions are both reproduced and cross-checked, including that they deliberately differ).
  **Deferred:** unwinding the `fitschan/wcs_celestial.rs::build_rotation_from_pole` private rotation construction. That code's `rot_z`/`rot_y` helpers use the standard *CCW* convention (its comment claiming to "mirror astMtrRot" is misleading) and the resulting matrix matches C's `WcsNative` output (verified: sky longitude at CRVAL=180° is `+π`, oracle-confirmed). Replacing it with the literal `astMtrRot` *clockwise* sequence flips the longitude to `-π` at the pole wraparound (3 lib unit tests), so the two conventions are not interchangeable in this composition. The local copy is correct as-is; reconciling it with `mtr_rot` needs the SphMap-wrapping convention worked through and is tracked separately — do not unwind without resolving the CW/CCW reconciliation.

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

- **[HIGH] Frame equality is attribute-based; C's is conversion-based.** *(FIXED 2026-06-13, oracle-matched.)*
  C `Frame::Equal` (`frame.c:3549-3573`) defines equality as: same class AND `astConvert(that, this, "")` succeeds AND the resulting mapping simplifies to a UnitMap — and **no Frame subclass overrides Equal** (verified: no `static int Equal(` in skyframe/specframe/timeframe/cmpframe/fluxframe.c).
  All leaf Frame classes now route `equal` through the shared `find_frame::frames_equal` helper (same class name + convert + simplify-to-UnitMap); `FrameSet`/`Region` keep their structural overrides. Recursion terminates because frame conversion never produces a `NormMap`. The oracle confirmed the old attribute model was wrong in several cases now corrected: Unit-string differences are equal under ActiveUnit off, DSBSpecFrame ignores DSB-only attributes, same-System FluxFrames are equal regardless of SpecVal, and plain-Frame Epoch / SkyFrame Equinox-at-default / NegLon differences are equal. A latent SkyFrame conversion gap surfaced and was fixed: `make_offset_mapping` now gates on `SkyRef` being set (mirroring C `SkyOffsetMap`, skyframe.c:7722) so an Origin frame with default SkyRef converts via the identity. Tests: `tests/test_equal.rs`, `tests/test_frame_equality.rs`, `tests/test_fluxframe.rs`.

- **[MED] PermMap equality compares constants positionally; C compares referenced values.** *(FIXED 2026-06-13, oracle-matched.)*
  C `Equal` (`permmap.c:224-392`) treats two differing negative entries as equal when the *referenced* constants are equal (`constant[-p1-1] == constant[-p2-1]`, exact comparison) and never looks at unreferenced constants; out-of-range entries go through `SAFE_PERMVAL`. It also skips the constant comparison entirely when the perm indices match (a quirk: two PermMaps referencing the same constant slot compare equal even if the stored values differ).
  `PermMap::equal` (`perm.rs`) now ports this per-entry against C's internal encoding, with exact referenced-constant comparison, ignored surplus constants, and the matching-index skip quirk preserved (logged in `c-library-quirks.md`). Old-audit item 2.3 is now fully fixed. Tests: `tests/test_permmap.rs`, `tests/test_equal.rs`.

- **Old-audit equality items 2.2 and 2.4 are FIXED.**
  MatrixMap now compares effective forward matrices resolving invert flags (`matrix.rs:543-572`); WinMap compares effective scale/shift parameters resolving inversion (`winmap.rs:302-314`).
  Strike both from the section 4 re-verification list.

- **MapSplit algorithm bodies verified — no new math findings.**
  The trait default (`helpers::full_set_split`) matches C's full-selection path including the permuted-selection PermMap composition (`mapping.c:7798+`).
  `PermMap::map_split` (`perm.rs:378-470`) is a faithful port of `permmap.c:1438` with documented encoding translation (1-based/0-as-BAD vs C 0-based/negative), the constants pool reused, and deliberate stricter duplicate-axis validation. The `PermSplit` alternative mode is now ported too (section 1, FIXED 2026-06-14).
  `CmpMap::map_split` (incl. the MapSplit2 fallback) was verified in 6g; the MatrixMap zero-init divergence is the documented quirk; FrameSet's missing override is in 6b.
  `TranMap::map_split` exists but its body was not deep-compared (low risk: it splits both components and recombines).

- **[LOW] Axis-index convention is inconsistent within the Mapping trait.**
  `rate` takes 1-based `ax1`/`ax2` (`rate.rs:32-33` subtracts 1) while `map_split` takes 0-based `in_axes` (`mod.rs:229` doc states it).
  The design spec mandates 1-based axis indices throughout the public API; this mixed convention is exactly the kind of trap that produces off-by-one workarounds downstream — standardise on 1-based.

## 6m. Serialisation-default audit (2026-06-14)

Prompted by the SlaMap `from_node` bug (a missing conversion argument
defaulted to `0.0` instead of `AST__BAD`, see section 6g), every mapping and
Frame class's `from_node` numeric defaults were compared against the C
`astLoad*` `astReadDouble`/`astReadInt` defaults — the exact pattern that hid
that divergence. **SlaMap was the only genuine bug**; all other classes are
behaviourally identical to C:

- **Raw-default match** (the deserialised value feeds the transform directly
  and the default equals C's): MatrixMap (`AST__BAD`), ShiftMap (`0.0`),
  WinMap (`0.0`/`1.0`), UnitNormMap centre (`0.0`), SpecMap/TimeMap args
  (`AST__BAD`), DssMap (`AST__BAD`). These match C exactly.
- **Store-BAD-present-as-default idiom** (C stores its own "unset" sentinel —
  `AST__BAD`/`-INT_MAX`/`-1` — but every consumer maps it to a physical
  default via `astGet*`; Rust stores the physical default directly, which is
  transform-equivalent): PcdMap Disco/PcdCen (BAD→0.0), GrismMap NR/WaveR/M/…
  (BAD→1.0/5000/0/…), PolyMap NiterInv/TolInv (−INT_MAX→4, BAD→1e-6 — Rust
  also guards the raw sentinels), SphMap UnitRadius/PolarLong (−1→0, BAD→0.0).
  For these the public constructor always sets the field, so the read-default
  is only reachable via a hand-crafted dump, and even then the transform
  agrees; the only theoretical difference is the `Test*`/`set`-flag on an
  artificially-unset field. Confirmed by the byte-exact round-trip corpus
  (431/431) and the targeted round-trip tests.
- **Conditionally-read** (Rust stores `Some(..)` only when the key is present,
  matching C's unset semantics): DSBSpecFrame DSBCen/IF, the Frame ActiveUnit
  flag, Digits/Direction. Correct.

Conclusion: no further serialisation-default divergences remain; the run-merge
fixtures and this audit close the section 6g concern.

## 6n. Consumed-card tracking (C "USED card" mechanism, ported 2026-06-14)

C `astRead` flags the FITS cards it consumes (the WCS-description keywords —
`MarkCard`/`FixUsed`/`CARDUSED`, fitschan.c) so they are excluded from later
output (a re-write, or a NATIVE dump). The Rust port mirrors this:
`FitsCard.flags` carries a `Consumption` tri-state (`Free`/`Provisional`/`Used`)
plus a `protected` bool; the reader records consumed keyword names via the
`*_marked` `CardList` getters (mark=1) — observation-detail keywords (the MJD/DATE
epoch chain, `TIMESYS`, `DUT1`, `DTAI`, `OBSGEO-*`) use the plain getters (mark=0)
and survive; SpecTrans records each translated original via `delete_consumed`.
On read success the provisional flags are promoted (`fix_used`) and `to_header`/
`ncard`/`find_fits` skip `Used` cards. A subsequent write re-emits any card it
rewrites (the full-card replace in `put`/`put_or_append` resets the flag). The
`protected` flag is the foundation for a future `astRetainFits` (ABI shim TBD).

**Documented divergence:** the Rust NATIVE *read* arm does not consume cards (it
parses COMMENT/keyword-card text, with no `GetValue`-style mark sites), so a
repeated NATIVE read of the same chan still re-returns the object — whereas C
removes the consumed COMMENT block on the first NATIVE read. This is faithful for
the read-FitsWcs/write-NATIVE wcsconverter flow (the consumption happens at the
FitsWcs read); it differs from C only for back-to-back NATIVE reads of one chan,
which the public test surface does not exercise.

## 7. Working rules for the fix agent

- Port the C logic; never paper over a gap with a heuristic the C does not have (repo convention).
- Before claiming something missing, grep the whole crate — much C per-class logic is centralized in shared Rust modules.
- Respect the SkyFrame setter hot path (O(1), allocation-free, lock-free) and the documented MatrixMap no-BLAS decision.
- Mirror C bugs only where `c-library-quirks.md` says parity requires it, and add to that doc when a new quirk is found.

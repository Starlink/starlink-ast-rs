# Mapping classes: C-parity audit

Date: 2026-10-06.
Scope: the `Mapping` base class and every Mapping subclass, their attributes and their methods.
`MapMerge` and `Simplify` are out of scope: each class's are audited in `docs/audits/mapmerge/`.
Frame and Region classes are out of scope here except where they act as Mappings (see `frame-family-c-parity.md` and `region-*.md`).
`ast-capi` is out of scope: it is unstarted, so no string-attribute dispatch (`astGet`/`astSet` by name) exists for any class, and C's `GetAttrib`, `SetAttrib`, `TestAttrib` and `ClearAttrib` overrides have no counterpart by design.
`Report`, `AllowSimplify` and `RestrictedSimplify` were ported just before this audit and are not re-audited.

C line numbers refer to the `reference/starlink-ast` checkout on branch `u/timj/norm-unit` at `7e1f74f4`, relative to `src/`.
Rust line numbers refer to branch `tickets/DM-54793` at `39ed9e6a`, relative to `crates/ast-core/src/`.

## Method

1. Every `vtab->X = X` assignment in each class's C source was listed and mapped onto a Rust trait method, inherent method, or "no counterpart".
2. Every C attribute was checked for typed get/set/test/clear accessors in Rust.
3. Each C method with a Rust counterpart was compared by reading both implementations, in two class groups: the base class and the thirteen linear or structural classes, then the thirteen polynomial, celestial, spectral and expression classes.
4. Every HIGH finding but one (WC-1, which needs a FitsChan round trip) was then run through paired probes: a C program linked against the built reference `libast`, and a scratch Rust crate depending on `ast-core`, given the same inputs.
   Twelve were confirmed and one (PY-4) was not, and is downgraded.

Findings marked UNVERIFIED were established by reading only, and say what would settle them.
Ratings: HIGH means wrong coordinates, a hang or panic, or silent data loss. MED means a wrong default, a missing error, or a wrong dump. LOW means cosmetic, latent, or internal-only.
"Probed" means confirmed by paired C/Rust probes.

## Summary

99 findings: 13 HIGH, 35 MED, 51 LOW.
The transform arithmetic of most classes matches C; the gaps are at the edges, where a value is BAD, an input is malformed, or a dump is read.
Six root causes account for most findings:

| Theme | What is wrong | Findings |
|---|---|---|
| A. Per-axis BAD | C makes an output BAD only when a term that uses a BAD input or BAD coefficient contributes to it. Several Rust classes blank the whole point, or carry a BAD coefficient through as `-DBL_MAX` arithmetic. | PY-1, CH-1, NM-1, MX-1, SM-1, PY-7, RM-1, LM-4, MX-4 |
| B. BAD in loaded data | C writes a BAD value as an unset (commented) card and reads a missing card back as BAD. Rust loaders default a missing card to 0.0. | MX-2, PM-1, DS-1, PM-2 |
| C. Lost or ignored dump content | Cards that C reads are dropped: legacy keys, Nin for a 3-D SpecMap, set-to-default attributes. | WC-5, SP-2, PY-8, SH-3, SN-5, PM-3, GR-1, SL-2 |
| D. Definedness | `has_forward`/`has_inverse` disagree with C's `TranForward`/`TranInverse`, so Rust attempts a transform C refuses and returns numbers, panics, or loops. | PY-2, PY-3, MX-1, SW-1, WC-4, LM-1 |
| E. Missing validation | C raises an error on malformed constructor arguments or PointSets; Rust accepts them, panics, or pads them. | MB-1, MB-3, TR-1, SW-2, LM-3, SA-1, SP-3, TM-1, SN-1, UN-1, CH-2, WC-3, SL-1 |
| F. Missing public API | Resample (nearest and linear only so far), Rebin, Decompose, IsLinear, test/clear accessors, the RemoveRegions overrides, and the IntraMap and XphMap classes. | MB-4, MB-6, TR-3, RM-3, NM-2, and the tables below |

**Why the tests missed these.** The Mapping tests build their objects with Rust constructors from good values and transform good points.
BAD reaches a coefficient or table entry only through a C dump or an explicit argument, and neither is exercised (themes A and B).
Inverted and dimension-changing cases of `has_inverse` are not asserted against C (theme D).
A transform-oracle case with one BAD input axis per class, and a load of each class's C dump with one BAD parameter, would have caught every HIGH finding except WC-5 and SP-2.

## Progress

**Closed 2026-10-07.** Every finding below is fixed, recorded as not a defect, or recorded as a deliberate divergence (PY-10); CH-4 and CH-5 wait on upstream Starlink/ast pull request 81.
The C defects found along the way are fixed on `u/timj/norm-unit`, past the submodule pin, and listed under "Resolved upstream" in `docs/issues/c-library-quirks.md`.

| IDs | Status | Evidence |
|---|---|---|
| LM-1, LM-2 | FIXED 2026-10-06: the inverse is a port of the inverse branch of lutmap.c `Transform`, searching the table with its bad entries removed and flagged as C's `GetMonotonic` builds it, with the nearest-neighbour branch. C reads one element before the table for a value beyond its first entry; the port reads nothing there and gives C's result wherever C's is determined (fixed upstream in ee96dc91, past the submodule pin; `docs/issues/c-library-quirks.md`) | `tests/test_lutmap_inverse.rs`: 20 C-oracle cases from `tools/oracle_fixtures/lutmap_inverse_probe.c`, bit-exact |
| PM-1, PM-2, MX-2, WC-5 | FIXED 2026-10-06: a PermMap constant or diagonal MatrixMap element missing from a dump reads as bad, as C's loaders default it; a PermMap dump writes only the constants its permutations refer to, a bad one unset; the WcsMap loader reads the `ProjP<m>` keys of older dumps into the latitude axis before the `PVi_m` keys, as wcsmap.c `Load` does | `tests/test_mapping_load.rs`: 8 C-oracle load cases, each re-dumped and transformed bit-exact, and 3 PermMaps built in Rust dumped as C dumps them, from `tools/oracle_fixtures/mapping_load_probe.c` |
| SP-1, SP-2 | FIXED 2026-10-06: `SpecMap::with_nin(3)` makes the 3-dimensional SpecMap, whose rest-frame conversions use each point's RA and Dec and negate the velocity for the inverse, as specmap.c `FrameChange` does; RA and Dec pass through. The loader takes the SpecMap's axis count from Nin, merges stop at a SpecMap with a different count, and Equal and Rate follow C | `tests/test_specmap_3d.rs`: 4 C-oracle SpecMaps (every rest-frame conversion, a chain and an inverted one) loaded, re-dumped and transformed both ways bit-exact, and 2 simplifications, from `tools/oracle_fixtures/specmap_3d_probe.c` |
| PY-2, PY-3, MX-1, SW-1, WC-4 | FIXED 2026-10-07: a PolyMap's IterInverse defaults on only when it has no inverse coefficients and as many inputs as outputs, and setting it on for any other PolyMap is AST__ATTIN (`set_iter_inverse` now returns `AstResult`); a PolyMap transform in an undefined direction is AST__TRNND, through the `require_direction` check C's base `Transform` makes; a full MatrixMap holding a bad element has no inverse (matrixmap.c `InvertMatrix`); an inverted SwitchMap swaps its definedness; transforming through an NCP WcsMap is AST__WCSTY | `tests/test_tran_definedness_oracle.rs`: 9 C-oracle cases with controls (TranForward, TranInverse, and whether each transform and setting IterInverse fails), and MatrixMaps built in Rust, from `tools/oracle_fixtures/tran_definedness_probe.c` |
| PY-1, PY-7, CH-1, CH-5 | FIXED 2026-10-07: PolyMap and ChebyMap transform through one port of PolyMap's `Transform` and its `PolyPowers` seam (`polymap.rs` `poly_transform`, the `PolyPowers` trait): a bad input, or a ChebyMap input outside its box, makes bad only its powers above zero, so only the terms using them, and a bad coefficient makes its output bad; a ChebyMap direction with no box is an ordinary polynomial. A coefficient missing from a dump loads as bad, as C reads it. When every power and coefficient is good the loop runs without its checks, which keeps PolyMap forward at C's speed (56.6 ns/pt against C's 57.0 at 100k points; ChebyMap 37.7 against 38.4) | `tests/test_poly_bad.rs`: 6 C-oracle PolyMaps and ChebyMaps transformed both ways, through the bad-value path and the NaN protocol, bit-exact, from `tools/oracle_fixtures/poly_bad_probe.c` |
| NM-1, SM-1, MX-4 | FIXED 2026-10-07: a NormMap normalises every point, bad values included, as normmap.c `Transform` does, and gives bad for NaN; a ShiftMap axis with a bad shift is bad; MatrixMap's NaN protocol keeps a bad input bad through a zero diagonal element and makes a bad element's output bad, as its bad-value path does | `tests/test_mapping_bad.rs`: 7 C-oracle NormMaps, ShiftMaps and MatrixMaps transformed both ways through both paths, bit-exact, from `tools/oracle_fixtures/mapping_bad_probe.c`; a NaN input is compared with C given `AST__BAD` (`docs/issues/c-library-quirks.md`, "C treats a NaN input as a number") |
| MB-1, RM-2 | FIXED 2026-10-07: every Mapping, Region and FrameSet `transform` and `transform_into`, and the NaN protocol's entry, make the checks C's base `Transform` makes first (`mappings::validate_transform`): an undefined direction is AST__TRNND, an input with the wrong number of coordinates AST__NCPIN, an output with too few points or coordinates AST__NOPTS or AST__NOCTS. A RateMap's inverse, a SelectorMap's inverse and a non-monotonic LutMap's inverse are therefore errors, not bad values. The simplify regression no longer skips a fixture's dump and `IsSimp` checks when its C reference cannot be transformed; it requires the simplified Mapping to refuse too | `tests/test_transform_validation.rs`, `tests/test_tran_definedness_oracle.rs`, `tests/test_simplify_regression.rs` |
| MB-2, MB-3, TR-1, SW-2, LM-3, SA-1, SP-3, TM-1, SN-1, UN-1, CH-2 | FIXED 2026-10-07: every `rate` checks its axis indices as `astRate` does (AST__AXIIN) and the numeric rate needs a forward transformation (AST__NODEF); UnitMap and MathMap no longer pass a NaN through, so no Mapping gives a NaN output. TranMap and SwitchMap refuse components without the direction they need (AST__INTRD); LutMap refuses a one-element table (AST__LUTIN) and a zero increment (AST__LUTII); `SlaMap::add`, `SpecMap::add` and `TimeMap::add` return `AstResult` and refuse the wrong number of arguments (AST__TIMIN), with `add_conversion` for the port's own rebuilds; `SplineMap::new` and `UnitNormMap::new` return `AstResult` with C's AST__ATTIN and AST__BADSM; a ChebyMap box is refused only at zero width (AST__BADBX) | `tests/test_constructor_checks.rs`, `tests/test_nan_outputs.rs`, `tests/test_transform_validation.rs` |
| WC-3, SL-1 | FIXED 2026-10-07: a WcsMap transformation reads wcslib's status as C's `Map` does, so status 1 (unusable parameters) and a missing PV2_1 for a conic or Bonne projection are AST__WCSPA and only status 2 gives bad points, and the projection's set-up runs on its first call, as C's `InitPrjPrm` leaves it (`docs/issues/c-library-quirks.md`, "A WcsMap reports unusable parameters only on its first transformation"); a SelectorMap refuses Regions whose Frames differ from the first's (AST__BADNI) | `tests/test_wcsmap.rs` (`unusable_projection_parameters_are_errors`, from C probed in the same order), `tests/test_constructor_checks.rs` |
| TR-3, RM-3, NM-2, CM-3, MB-5 | FIXED 2026-10-07: removing Regions is a per-class method, as C's `RemoveRegions` is (`Mapping::mapping_without_regions`, `Frame::frame_without_regions`, with `regions::remove_regions` the public entry): CmpMap, TranMap, RateMap, NormMap, SwitchMap, FrameSet, CmpFrame and every Region override it, a series CmpMap drops a component that became a UnitMap, and a parallel CmpMap or TranMap of two UnitMaps becomes one. C's TranMap gave a UnitMap of twice its dimensionality; fixed upstream (80d66b0a). `map_box` takes a 1-based output index, as `astMapBox` does (AST__BADCI) | `tests/test_remove_regions_oracle.rs`: 11 C-oracle Mappings, dumps byte-exact, from `tools/oracle_fixtures/remove_regions_probe.c`; `tests/test_transform_validation.rs` (`map_box_takes_a_one_based_output`) |
| WC-2, CM-1, CM-2 | FIXED 2026-10-07: `WcsMap::get_pv` returns C's `astGetPV` value, with the defaulted latitude parameters, the longitude defaults from the native reference point except for TPN, bad where there is no value, and AST__AXIIN for an invalid axis or parameter index; `test_pv` is C's `astTestPV`, and the port's own readers use the set value (`stored_pv`) as before. CmpMap equality sets each component's Invert flag to the one the CmpMap applies and asks the component's own `equal`, as cmpmap.c `Equal` does. A FrameSet component of a CmpMap is replaced by its base-to-current Mapping, as `astInitCmpMap` does | `tests/test_wcsmap_pv.rs`: 20 C-oracle `astGetPV` values from `tools/oracle_fixtures/wcsmap_pv_probe.c`; `tests/test_cmpmap.rs` (C's results for both) |
| MX-3, PY-6, GR-1, DS-1 | FIXED 2026-10-07: MatrixMap equality falls back to the other direction's matrices when either lacks the one it applies, and is true when neither pair exists, as matrixmap.c `Equal` is; a PolyMap or ChebyMap term with a zero coefficient is dropped as C's `AddCoeff` drops it, not refused; `GrismMap::default_map` leaves every attribute unset as `astGrismMap("")` does, so its dump writes them commented; a DssMap dump missing a plate parameter, PPO3, PPO6 or one of the first 13 plate solution coefficients on an axis is AST__RDERR, the rest defaulting to 0 | `tests/test_matrixmap.rs`, `tests/test_polymap.rs`, `tests/test_grismmap.rs` (C's dump), `tests/test_mapping_load.rs` |
| MB-4 | FIXED 2026-10-07: `Mapping::is_linear` is the read-only `IsLinear` attribute, with C's `GetIsLinear` overrides: UnitMap, ZoomMap, ShiftMap, WinMap, MatrixMap and every Region are linear, a CmpMap when both components are, a FrameSet when its base-to-current Mapping is, and every other class, PermMap and PolyMap included, is not | `tests/test_is_linear.rs`: 12 Mappings against C's `astGetI(map, "IsLinear")` |
| SL-2 | FIXED 2026-10-07: a SelectorMap writes its second and later Regions without their Frame, as selectormap.c `Dump` does by clearing RegionFS; the loader already gives them the first Region's | `tests/test_selectormap.rs` (`selectormap_dump_matches_the_c_library`): byte-exact against C's dump from `tools/oracle_fixtures/selectormap_dump_probe.c`, and a round trip |
| WC-1 | FIXED 2026-10-07: the FITS-WCS writer decides whether a projection is zenithal with a port of wcsmap.c `IsZenithal` (`WcsMap::is_zenithal`): TAN, STG, ARC, ZPN, ZEA, AIR and TPN always, AZP, SIN and SZP by their parameters, and not HPX or XPH, so a non-zenithal projection has its pole worked out from its Mappings. C's SZP test reads the wrong parameter, which the port mirrors (`docs/issues/c-library-quirks.md`, "`IsZenithal` tests the wrong SZP parameter"). WC-10 records a read gap the probe found | `tests/test_sky_pole_oracle.rs`: 14 headers read and written, LONPOLE and LATPOLE against C's from `tools/oracle_fixtures/sky_pole_probe.c`; the two C refuses are refused since WC-10 |
| CH-3 | FIXED 2026-10-07: `ChebyMap::domain` returns both bounds as C's `astChebyDomain` does, from the stored box or, when the requested transformation has none, by MapBox from the other transformation's box, with bad bounds where neither gives one. C mapped that box in the wrong direction for an inverted ChebyMap and failed; fixed upstream (58786d9c) and ported with the fix (`docs/issues/c-library-quirks.md`, "`ChebyDomain` fails for an inverted ChebyMap with only one transformation") | `tests/test_chebymap.rs` (`chebymap_domain_inverted_forward_only`): a forward-only 2-D ChebyMap and its inverse, against the fixed C |
| SN-2 | FIXED 2026-10-07: `SplineMap::from_scipy` takes the coefficients in the SciPy order C's `astSplineMap` takes and transposes them into the CMLIB order `SplineMap::new` keeps, as splinemap.c `astSplineMapId_` does; `SplineMap::coeffs` returns them in SciPy order as `astSplineCoeffs` does, ignoring Invert, with AST__AXIIN for an axis other than 1 or 2 where C silently reads the v coefficients. `SplineMap::new` keeps the CMLIB order of C's Fortran API, so it disagrees with C's C API; the `splinemap.rs` module doc records this | `tests/test_splinemap_c_api.rs`: `testsplinemap_c.c` ported against its 150 by 100 SciPy fixture, coefficients exact and positions within C's 1e-11 |
| CH-4, CH-5 | DEFERRED 2026-10-07: the ChebyMap `PolyTran` override and its `FitPoly1DInit` / `FitPoly2DInit` seams are not ported until upstream Starlink/ast pull request 81 (https://github.com/Starlink/ast/pull/81), which changes ChebyMap inverse fitting, is merged; porting the current C first would mean porting it twice. CH-5 becomes reachable only with CH-4 | none yet |
| WC-10 | FIXED 2026-10-07: a FITS-WCS read whose LONPOLE cannot hold for its reference point is AST__BDFTS, as C's `WcsNative` reports it: the native-to-celestial rotation returns the error where it returned no rotation, and the celestial decoder returns `AstResult<Option<_>>`, so `None` means only that the header has no celestial axes. The decoder's Mapping and SkyFrame constructor errors, which it dropped, now propagate too | `tests/test_sky_pole_oracle.rs`: all 14 headers against C, and `refused_cases_are_ill_conditioned` for the error code |
| PY-11, WC-7, MM-2 | FIXED 2026-10-07: C's error codes. A PolyMap term for an output or input that does not exist is AST__BADCI (polymap.c `StoreArrays`); `WcsMap::set_pv` on an axis out of range, or on one that is neither the longitude nor the latitude axis, is AST__AXIIN (wcsmap.c `SetPV`, where `astGetPVMax` gives -1 for such an axis); a MathMap with too few forward or inverse functions is AST__INNTF (mathmap.c `astInitMathMap`) | `tests/test_constructor_checks.rs` |
| UM-1, CH-6 | FIXED 2026-10-07, by earlier rows: UnitMap's `transform` validates its PointSet and converts NaN to BAD through MB-1 and MB-2, and a ChebyMap term with a zero coefficient is dropped through PY-6 | as those rows |
| MB-7 | NOT A DEFECT: C refuses zero points too. `ValidateMapping` refuses only a negative count, but `astTran1` and `astTran2` then build a PointSet, and `astInitPointSet` reports AST__NPTIN for fewer than one point (pointset.c:3011); probed, all of `astTran1`, `astTran2` and `astTranN` give AST__NPTIN for `npoint = 0`. The port's AST__NPTIN stands, and its doc and test now cite the PointSet check | `tests/test_tran1_tran2.rs` (`tran1_rejects_an_empty_input`) |
| ZM-1, PM-3, LM-7, PY-8, WC-6, PC-1, GR-2, SN-5, SH-1, SH-2, SH-3, SH-4, MM-1 | FIXED 2026-10-07: every attribute these findings name keeps C's unset state and has `test_*` and `clear_*` methods: Zoom (unset as C's 0.0, reading 1.0), PermSplit, LutInterp, LutEpsilon, NiterInverse, TolInverse, the PV parameters (`clear_pv`) and LonCheck, FITSProj and TPNTan, Disco and PcdCen, the eight GrismMap parameters, InvNiter, InvTol and OutUnit, UnitRadius and PolarLong, SimpFI and SimpIF. A clear discards both simplified records where C's does. Setting a double to `BAD_VALUE`, C's unset value, unsets it (TolInverse, InvTol, the PV parameters, PolarLong). Dumps write a set attribute whenever it is set, its default included, and a cleared one commented only when C marks it helpful. SphMap equality compares the stored values, so an unset attribute differs from one set to its default (SH-4). The PolyMap attribute copies in `analyse_poly` and `scale_poly_inputs` copy only what is set, as fitschan.c does | `tests/test_attr_test_clear_oracle.rs`: 16 cases against C's `astTest`, dump and read-back dump, from `tools/oracle_fixtures/attr_test_clear_probe.c` |
| MB-6 | FIXED 2026-10-07: Invert has C's three states. `Mapping::set_invert`, `clear_invert` and `test_invert` are C's `astSetInvert`, `astClearInvert` and `astTestInvert`, and an Invert set to 0 is recorded in `MappingFlags` and read from an `Invert = 0` card. `Mapping::invert` is a provided method, as C's `Invert` is one base-class method: it flips the orientation through the per-class `invert_orientation` and leaves Invert set only when the result is inverted, so a second inversion unsets it | `tests/test_attr_test_clear_oracle.rs`: four Invert cases against C's `astTest` and dump |
| TR-2, SW-3, RM-1, RM-4 | FIXED 2026-10-07: a TranMap's rate is its forward component's `rate` in the orientation the TranMap records, as tranmap.c `Rate` asks it; a SwitchMap's is the chosen route's `rate` in whatever orientation, and bad with no forward selector, as switchmap.c `Rate` gives it; a RateMap asks its Mapping's `rate` at every point, bad inputs included, with the Mapping in its recorded orientation, as ratemap.c `Transform` does | `tests/test_rate_oracle.rs`: 8 rates against C's from `tools/oracle_fixtures/rate_probe.c`, a RateMap read with its recorded orientation differing from its Mapping's among them; `tests/test_ratemap.rs` (`test_ratemap_bad_input_asks_the_mapping`) |
| SN-4, TM-2 | FIXED 2026-10-07: C's `Rate` for both classes ignored Invert, giving the forward transformation's rate for an inverted Mapping; fixed upstream (fb43dfb3, 4b313e0d) and ported with the fixes. A SplineMap's forward rate is the spline derivative with no numeric fallback, and an inverted one's is numeric; a TimeMap's is C's analytic product of slopes, its reciprocal when inverted, numeric with GMST (`docs/issues/c-library-quirks.md`) | `tests/test_splinemap_c_api.rs`, `tests/test_timemap.rs` |
| SP-4 | NOT A DEFECT: C's generic Mapping transform passes every result through `astReplaceNaN` (mapping.c:24815), which with AST_REPLACE_NAN set replaces every non-finite value, infinities included, by AST__BAD (pointset.c `ReplaceNaN`). The port's outputs are always C's with AST_REPLACE_NAN set, the design's BAD-output rule, so its final non-finite check is that pass; within the chain both leave a non-finite value to the next conversion | none needed |
| LM-4, LM-5, LM-6 | FIXED 2026-10-07: LutMap's forward transformation is lutmap.c `Transform` transcribed: the scale `1/inc` applied as a product, nearest-neighbour taking `floor(x + 0.5)`, linear interpolation written `lut[i]*(1-f) + lut[i+1]*f`, and an input on or above the last node extrapolated from the last two entries, so a bad neighbour makes it bad | `tests/test_lutmap_forward_oracle.rs`: 31 outputs compared bit for bit with C's from `tools/oracle_fixtures/lutmap_forward_probe.c`; the previous code fails 3 of them, one per finding |
| TR-4, SL-3, WM-1 | FIXED 2026-10-07: equality follows each class's C `Equal`. A TranMap compares its components in the orientations it recorded, so one read from a dump whose InvA differs from the component's Invert still equals the original; a SelectorMap compares its number of inputs, Regions and bad value and not the Invert flag, so only a one-dimensional SelectorMap equals its inverted copy, as the swapped numbers of inputs and outputs decide it in C; a WinMap with the same stored terms as another equals it only with the same Invert flag, the applied terms being compared only when the flags differ. C's SelectorMap `Equal` accepted either a matching Region count or a matching bad value; fixed upstream (11c2367b) | `tests/test_equal_oracle.rs`: 6 comparisons against C's `astEqual` from `tools/oracle_fixtures/equal_probe.c` |
| MB-8, NM-3, MX-5 | FIXED 2026-10-07: `map_split` returns `AstResult<Option<_>>` and each class splits as its C `MapSplit` does. The base `MapSplit`, and NormMap, MatrixMap, RateMap, TranMap and WcsMap, which call it first, report an index out of range as AST__AXIIN; UnitMap, ZoomMap, ShiftMap, WinMap, PermMap and CmpMap give no split; an index of 0 behaves as C's -1. A repeated index is accepted where C accepts it, and NormMap's is AST__SELIN from picking the Frame's axes; an empty selection gives no split. A NormMap split is not inverted (NM-3); a MatrixMap returns itself whole for a full selection and declines a kept row with a bad element (MX-5); a WinMap split holds the applied terms. C's PermMap read beyond its arrays for an index out of range; fixed upstream (010b5e85) | `tests/test_map_split_oracle.rs`: 66 splits against C's from `tools/oracle_fixtures/map_split_probe.c`, outputs and dumps, two with `IsSimp` set aside (`ISSIMP_COMPONENT_EXCLUDED`) |
| WC-8 | FIXED 2026-10-07: WcsMap equality compares each projection parameter, unset in both or set in both to equal values, an unset one differing from one set to zero. C's `Equal` compared none when only one WcsMap had any, and compared the lengths of the stored arrays; fixed upstream (47826767), with the simplify corpus unchanged | `tests/test_equal_oracle.rs`: four WcsMap comparisons against the fixed C |
| PY-4, PY-5, SN-3 | FIXED 2026-10-07: the PolyMap iterative inverse transcribes polymap.c `Transform`: its initial guess is the inverse of `LinearGuess`'s MatrixMap-and-ShiftMap, coefficients assigned rather than summed and a row without a linear term given 1.0 on the diagonal; each iteration transforms the guesses with the PolyMap and with the Jacobian's PolyMaps, replaces non-finite results by bad ones as `astTransform` does, and solves each point with `palDmat`; a point converges when `|dx|^2 <= tol^2 |x|^2`, and bad values run through the arithmetic as in C. The probe showed the different guess converging to a different root, which the audit's probe had not found. The SplineMap iterative inverse solves with `palDmat` and clamps every guess to the knots each iteration, as splinemap.c does. The `simd-eval` feature's PolyMap paths, off by default, keep their own arithmetic | `tests/test_poly_iter_inverse_oracle.rs`: 7 PolyMaps inverted bit for bit against C from `tools/oracle_fixtures/poly_iter_inverse_probe.c`; `tests/test_splinemap_c_api.rs` (`forward_and_inverse_match_the_c_library_bit_for_bit`, from `tools/oracle_fixtures/spline_inverse_probe.c`) |
| PY-9 | FIXED 2026-10-07: `PolyMap::coeffs` is `astPolyCoeffs`, relative to the Invert flag and grouped by output; `forward_terms` and `inverse_terms` stay the original orientation. C's `PolyCoeffs` took its counts from the inverted PolyMap and read beyond its arrays for one with different numbers of inputs and outputs; fixed upstream (436f069f) | `tests/test_polymap.rs` (`coeffs_are_relative_to_invert_and_grouped_by_output`) |
| PY-10 | DELIBERATE DIVERGENCE 2026-10-07: PolyTran is minimised with the `levenberg_marquardt` Rust crate, not cminpack's `lmder1`, so fitted coefficients can differ from C's at the solver's tolerance. The port is to hold no C code (wcslib, pal and erfa are themselves to be ported), so cminpack is not vendored to close this | none |
| MB-9 | FIXED 2026-10-07: `linear_approx` returns `AstResult<Option<_>>`: an error transforming the sample points is returned, as it propagates from C's `astLinearApprox`, and bounds of the wrong length are AST__NCPIN | `mappings/linear_approx.rs` tests (`transform_error_is_reported`, `dimension_mismatch_is_an_error`) |
| WC-9 | FIXED 2026-10-07: a conic WcsMap's native latitude is its PV2_1 through `get_pv`, bad while unset, as wcsmap.c `GetNatLat` reads it where the projection table holds AST__BAD | `tests/test_wcsmap.rs` (`conic_native_latitude_is_its_pv`, C's values) |
| SA-2 | FIXED 2026-10-07: `SlaCvtType::from_code` recognises only C's names, so not `DH2E` or `DE2H`, and the SlaMap, SpecMap and TimeMap codes are matched as C's `astChrMatch` matches them, ignoring case and trailing white space | `tests/test_slamap.rs` (`cvt_codes_match_as_c_matches_them`) |
| DS-2 | FIXED 2026-10-07: `DssFits` is a protected function whose one caller is DssMap's own `MapMerge`, which rebuilds the merged DssMap from the cards it writes; the finding's "no caller" was wrong. The port amends the plate parameters directly and now reproduces the round trip's one numeric effect, the plate centre passing through sexagesimal cards: a PltRA of 0.1 merges to 0.09999999999999999, as in C | `tests/test_dss_merge_oracle.rs`: four plate centres against C's from `tools/oracle_fixtures/dss_merge_probe.c` |

## Recommended order

1. **LM-1.** FIXED; see Progress.
2. **Theme B and C loaders.** PM-1, MX-2, WC-5 and SP-2 (with SP-1) are FIXED; see Progress.
3. **Theme D: PY-2 with PY-3, MX-1, SW-1, WC-4.** FIXED; see Progress. Only PolyMap calls `require_direction` from its transform so far; the other classes are MB-1.
4. **Theme A.** PY-1, CH-1, PY-7, CH-5, NM-1, MX-4 and SM-1 are FIXED; see Progress.
5. **MB-1** is FIXED; see Progress. **MB-3** (`rate` validation) remains.
6. LM-2, then the MED tail, then the missing test/clear accessors in one sweep.

## The `astTran*` interfaces

C transforms points through `astTransform` and five array wrappers.
They differ in the layout of the caller's arrays, not in what they compute:

| C | Arrays | Rust |
|---|---|---|
| `astTransform` | a PointSet | `Mapping::transform`, and `transform_into` with an output supplied |
| `astTran1` | one array of values | `Mapping::tran1` |
| `astTran2` | an x array and a y array | `Mapping::tran2` |
| `astTranN` | one array, a coordinate's values a stride apart | none |
| `astTranP` | one pointer per coordinate | none |
| `astTranGrid` | a regular grid of input positions, optionally by piecewise linear approximation | `Mapping::tran_grid` |

`astTranN` and `astTranP` exist in C because a C array does not carry its shape.
A Rust `PointSet` is a two-dimensional array that does, so `transform` covers both layouts and the Rust API needs neither.
The C ABI does need them: when `ast-capi` exports `astTranN` and `astTranP`, each must build a `PointSet` from its layout.

Two consequences follow:

- C reports a transformation only from the four array wrappers, `astTran1`, `astTran2`, `astTranN` and `astTranP`, when the Mapping's `Report` attribute is set (mapping.c), never from `astTransform`.
  The port's `transform` does not report either, which is faithful, so a Rust caller who wants the report uses `tran1` or `tran2`.
  The `ast-capi` exports of all four must call `report_points` when `Report` is set.
- `astTranGrid` is a separate algorithm, not a layout: it transforms every position of a regular grid, approximating the Mapping by linear fits over sub-regions where that is accurate enough (`TranGridAdaptively`, `TranGridSection`, `TranGridWithBlocking`).
  The port has it as `Mapping::tran_grid` (`mappings/tran_grid.rs`), which returns a `PointSet` with a row per grid position.

The validation the wrappers share is not where PLAN.md §2 places it.
C's base `Transform` (mapping.c:22333-22380), which every class's `Transform` calls first, raises `AST__TRNND`, `AST__NCPIN`, `AST__NOPTS` and `AST__NOCTS`, so `astTransform` validates as fully as the wrappers do (MB-1).

## Findings by class

### Mapping (base)

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| MB-1 | MED | mapping.c:22333-22380 (base `Transform`, reached by every class through `parent_transform`) | mappings/mod.rs:295,255; e.g. mappings/zoom.rs:126-138, mappings/cmpmap.rs:671 | `transform`/`transform_into` raise none of `AST__TRNND`, `AST__NCPIN`, `AST__NOPTS`, `AST__NOCTS`. A PointSet with the wrong `ncoord` is silently truncated or zero-filled (ZoomMap zips), or panics (CmpMap parallel slice). | `ZoomMap::new(2,2.0)` transform of a 1-coord PointSet: C NCPIN, Rust returns 2 coords with axis 2 = 0.0. `RateMap.transform(..., false)`: C TRNND, Rust all-BAD. |
| MB-2 | MED | mapping.c:24815 (`astReplaceNaN(result)` in `astTransform_`); pointset.c:1614-1630 (`AST_REPLACE_NAN`) | mappings/unit.rs:67-69; mappings/normmap.rs:150-158 | C converts NaN outputs to AST__BAD only when the `AST_REPLACE_NAN` environment variable is `1`; by default it leaves them NaN (probed). The port's design makes every output use BAD (CLAUDE.md), which is C's `AST_REPLACE_NAN=1` behaviour, but classes that copy values through leak NaN. Oracle probes for this should set `AST_REPLACE_NAN=1`. | `UnitMap::new(1).transform([[NaN]])`: C with `AST_REPLACE_NAN=1` BAD, Rust NaN. The same holds for NormMap with a NaN input. |
| MB-3 | MED | mapping.c:8998-9015, 24733-24742 | mappings/rate.rs:32-33; mappings/winmap.rs:427; mappings/matrix.rs:851; mappings/unit.rs:82; mappings/zoom.rs:187 | `rate` does no `AST__AXIIN` validation of `ax1`/`ax2` and no `AST__NODEF` check that a forward transform exists. Axis 0 underflows `usize`, which panics in debug builds and indexes out of bounds in release. Out-of-range axes return 0.0 or the zoom factor silently. | `WinMap.rate(&at,0,0)`: Rust panics, C reports AXIIN. `ZoomMap(2).rate(&at,5,5)`: C AXIIN, Rust returns the factor. |
| MB-4 | MED | mapping.c:1628 (base GetIsLinear = 0); overrides cmpmap.c:631, unitmap.c:342, zoommap.c:555, shiftmap.c:504, matrixmap.c:1424, winmap.c:880 | none | The public read-only `IsLinear` attribute has no counterpart. | `astGetI(map,"IsLinear")` cannot be served by ast-core. |
| MB-5 | MED | mapping.c:25064-25075 (`astMapBoxId_` decrements `coord_out`) | mappings/mod.rs:596,551-559; mappings/map_box.rs:913 | `map_box` takes a 0-based `coord_out`, while `rate` and `map_split` take 1-based indices. That breaks the 1-based rule in CLAUDE.md, and a caller using public C numbering silently gets the next axis. | `map.map_box(.., true, 1)` returns the bounds of output 2. |
| MB-6 | LOW | mapping.c:23269-23273, 6342-6352 | mappings/mod.rs:248-251 | `Invert` has only a toggle (`invert`) and a getter (`is_inverted`). There is no `set_invert`, `clear_invert` or `test_invert`. C `astInvert` clears and then sets only when needed, so `TestInvert` is 0 after a double inversion. | `astSetI(m,"Invert",1)`, `astTest(m,"Invert")`. |
| MB-7 | LOW | mapping.c:23208 (`npoint < 0` only) | mappings/mod.rs:974 | `tran1`/`tran2` reject 0 points with `AST__NPTIN`, but C accepts `npoint == 0`. | `m.tran1(&[], true)`: C OK, Rust Err. |
| MB-8 | LOW | mapping.c:7848-7856 (AXIIN) | mappings/mod.rs:536-551; mappings/helpers/mod.rs:59-101 | `map_split` returns `None` for out-of-range or 0 indices instead of raising `AST__AXIIN`. It also rejects duplicate indices, which C per-class splits accept (this is documented in the helper). | `m.map_split(&[0])`, `m.map_split(&[nin+1])`. |
| MB-9 | LOW | mapping.c:6556,6830 (errors propagate) | mappings/linear_approx.rs:110, ~300 (`transform(..).ok()?`) | `linear_approx` turns a transform error into `None` ("not linear") instead of reporting the error. | A Mapping with an undefined forward transform. |

### CmpMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| CM-1 | MED | cmpmap.c:458-…, compare loop sets each element's Invert to the list value, then calls `astEqual` | mappings/cmpmap.rs:884 | `equal` requires identical per-component effective Invert flags. C sets them and lets the class `Equal` decide, so pairs such as ZoomMap(2)⁻¹ against ZoomMap(0.5), or UnitMap⁻¹ against UnitMap, compare equal in C and unequal in Rust. | `CmpMap(UnitMap(1) inverted ; ZoomMap(1,2))` vs `CmpMap(UnitMap(1) ; ZoomMap(1,2))`: C 1, Rust false. |
| CM-2 | MED | cmpmap.c:4844-4856 | mappings/cmpmap.rs:125-160 | C replaces a FrameSet component with its base→current Mapping. Rust stores the FrameSet itself, so the dump contains a whole FrameSet where C writes a Mapping. | `CmpMap::new(Box::new(frameset), Box::new(UnitMap::new(2)), Series)` then dump. |
| CM-3 | LOW | cmpmap.c:3365-3470 | regions/mod.rs:1457-1477 | `remove_regions` always rebuilds through `CmpMap::new`, which drops Ident and the Mapping flags even when nothing changed. It also never collapses as C does: a series pair with a UnitMap becomes the other component, and parallel UnitMaps become one UnitMap. Only the shape differs; the transform is the same. | `remove_regions(CmpMap(Circle ; ZoomMap))`: C ZoomMap, Rust CmpMap(UnitMap ; ZoomMap). |

### TranMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| TR-1 | MED | tranmap.c:2090-2101 (`AST__INTRD`) | mappings/tranmap.rs:63-93 | `TranMap::new` accepts a `map1` with no forward transform or a `map2` with no inverse. C rejects both, and so always has `TranForward = TranInverse = 1`. | `TranMap::new(selector_map, x)` where the first argument lacks a forward transform. |
| TR-2 | LOW | tranmap.c:1191-1230 (delegates to component `astRate`) | mappings/tranmap.rs:421-423 | `rate` differentiates the whole TranMap numerically instead of calling the chosen component's `rate`, so it loses analytic rates and BAD semantics. | `TranMap(WinMap, WinMap⁻¹).rate`: C gives exactly `b`, Rust a numeric estimate. |
| TR-3 | MED | tranmap.c:1280 (RemoveRegions override) | regions/mod.rs:1457-1505 (no TranMap arm) | Regions inside a TranMap are not removed. | `remove_regions(TranMap(Circle, Circle))`: C returns UnitMap components, Rust returns the clone. |
| TR-4 | LOW | tranmap.c:184-260 (restores `invert1`/`invert2` before comparing) | mappings/tranmap.rs:425-447 | `equal` uses the children's stored orientation, not the `inv_a`/`inv_b` snapshot. This matters only for a loaded dump where InvA differs from MapA's Invert. | A dump with `InvA=1` and MapA not inverted. |

### RateMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| RM-1 | LOW | ratemap.c:1195-1201 (calls `astRate` on every point) | mappings/ratemap.rs:262-277 | Rust outputs BAD when any input axis is BAD, before consulting the Mapping. C defers to `astRate`, whose analytic overrides (UnitMap, ZoomMap, ShiftMap, WinMap, MatrixMap, PermMap) ignore other axes. | `RateMap(UnitMap(2),1,1)` at (1, BAD): C 1.0, Rust BAD. |
| RM-2 | LOW | ratemap.c:1159-1163 (TRNND from parent, then AST__INTER) | mappings/ratemap.rs:247-253 | The inverse direction returns an all-BAD PointSet instead of an error. | `rm.transform(&pts, false)`. |
| RM-3 | MED | ratemap.c:971-1067 | regions/mod.rs:1457-1505 | The RemoveRegions override is missing, so a Region encapsulated in a RateMap survives. | `remove_regions(RateMap(CmpMap(Circle;…)))`. |
| RM-4 | LOW | ratemap.c:1168-1171 (resets `emap` Invert to `map->invert`) | mappings/ratemap.rs:277 | `transform` calls `self.map.rate` in the child's current orientation, ignoring `invert_snapshot`. This matters only for a loaded dump where Inv differs from the child's Invert. | A dump with RateMap `Inv=1` and a child that is not inverted. |

### NormMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| NM-1 | HIGH, probed | normmap.c:1079-1086 (calls `astNorm` on every point); skyframe.c:7210 (normalises longitude when latitude is BAD) | mappings/normmap.rs:153-156, 172-175 (skips `norm` if any axis is bad); mappings/normmap.rs:199 (NaN path calls `norm` unconditionally) | A point with any BAD axis is returned unnormalised. C normalises the good axes, as do CmpFrame per component and SkyFrame longitude-only. The two Rust paths also disagree with each other. | `NormMap(SkyFrame)` with input (7.0, BAD): C (0.71681469282041377, BAD), which is 7−2π; Rust (7.0, BAD). |
| NM-2 | MED | normmap.c:896-984 | regions/mod.rs:1457-1505 | The RemoveRegions override is missing, so a NormMap over a Region keeps the Region. | `remove_regions(NormMap(circle_as_frame))`. |
| NM-3 | LOW | normmap.c:821-824 (new NormMap, never inverted) | mappings/normmap.rs:351 | `map_split` copies Invert onto the sub-NormMap. The transform is the same, but the dump gains an `Invert` card. | Split an inverted NormMap and dump the result. |

### SelectorMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SL-1 | MED | selectormap.c:1592-1606 (`AST__BADNI` unless every RegFrame equals the first) | mappings/selectormap.rs:49-66 | Only `naxes` equality is checked, so Regions in different Frames are accepted. | A SelectorMap built from a SkyFrame Circle and a 2-D Frame Box. |
| SL-2 | MED (UNVERIFIED) | selectormap.c:1156-1162 (RegionFS=0 for regions 2..n) | mappings/selectormap.rs:424-428 | Every Region is dumped with its own Frame. C omits the Frame from the second and later Regions, so the dumps are not byte-equal. | Native dump of a 2-region SelectorMap, compared with C's. |
| SL-3 | LOW | selectormap.c:249-269 (ignores Invert; tests `nreg == ‖ badval`, a C defect) | mappings/selectormap.rs:382-402 | `equal` requires matching Invert, nreg and badval. C reports equal for SelectorMaps that differ only in Invert. | `sm.equal(&inverted_clone)`: C 1, Rust false. |

### SwitchMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SW-1 | MED | switchmap.c:2646-2650 sets the flags; mapping.c:1786-1900 swaps them under Invert | mappings/switchmap.rs:458-464 | `has_forward`/`has_inverse` return `fsmap.is_some()`/`ismap.is_some()` whatever the Invert state, so an inverted SwitchMap reports the wrong definedness. CmpMap tran flags and the `tran1` TRNND check inherit the error. | A SwitchMap with only `fsmap`, inverted: C TranForward=0, Rust `has_forward()==true`. |
| SW-2 | LOW | switchmap.c:2597-2621 (`AST__INTRD`) | mappings/switchmap.rs:89-160 | `new` does not check that `fsmap` has a forward transform or that `ismap` has an inverse. | `fsmap` = a RateMap that has been inverted. |
| SW-3 | LOW | switchmap.c:1196-1231 | mappings/switchmap.rs:591-617 | For an inverted route, `rate` differentiates the whole SwitchMap numerically instead of calling `astRate(route)`. With no forward selector it returns Err (via `transform`) where C returns BAD. | An inverted SwitchMap with a ZoomMap route. |

### UnitMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| UM-1 | LOW | unitmap.c Transform (parent validation) and mapping.c:24815 | mappings/unit.rs:67-69 | `transform` returns `pts.clone()`, so the input passes through regardless of `ncoord`, and NaN is not converted to BAD. These are instances of MB-1 and MB-2. | A 3-coord PointSet into `UnitMap(2)`. |

### ZoomMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| ZM-1 | LOW | zoommap.c:1698-1712 | mappings/zoom.rs:53-81 | There is no `test_zoom` or `clear_zoom`. In C, clearing gives Zoom 1.0 and dumps `Zoom` as unset. | `astClear(zm,"Zoom")`. |

### ShiftMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SM-1 | MED | shiftmap.c:1049-1050 (a BAD shift gives a BAD axis) | mappings/shift.rs:99,119 | A BAD shift is added arithmetically. The inverse gives `+DBL_MAX`, not BAD. The forward gives `-DBL_MAX` only because it happens to equal BAD. C's `astShiftMap` with NULL shifts produces BAD shifts. | `ShiftMap::new(vec![BAD_VALUE])`, inverse of `[[1.0]]`: Rust +1.797e308, C BAD. |

### MatrixMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| MX-1 | HIGH, probed | matrixmap.c:1569-1575 (no inverse if any element is BAD) | mappings/matrix.rs:100, 119, 1229 | A full square matrix containing BAD is still inverted by `gauss_jordan_inverse`, so Rust reports an inverse and returns finite coordinates where C has none. | `MatrixMap::new([[1,BAD],[0,1]])`: C TranInverse=0 and the inverse raises `AST__TRNND`; Rust `has_inverse()` is true and the inverse of (1,1) is (+1.797e308, 1.0). |
| MX-2 | HIGH, probed | matrixmap.c:6252 (missing `M%d` defaults to BAD); 5617-5623 (BAD elements never written) | mappings/matrix.rs:1162 | Loading a `Form="Diagonal"` MatrixMap reads a missing diagonal `M{i}` as 0.0, so a BAD diagonal element becomes 0 and that axis outputs 0 instead of BAD. | C `astMatrixMap(2,2,1,{2,AST__BAD})` dumped and loaded in Rust; forward (1,1): C (2,BAD), Rust (2,0). |
| MX-3 | MED | matrixmap.c:930-942 (falls back to the other-direction matrices) | mappings/matrix.rs:860-885 | `equal` returns false when the effective matrix is absent. C compares the stored matrices instead. As a result an inverted non-square MatrixMap is not equal to its own clone. | `let mut m = MatrixMap::new(2×3)?; m.invert(); m.equal(&m.clone())`: Rust false, C 1. |
| MX-4 | MED | matrixmap.c:5205-5210, 5258-5266 | mappings/matrix.rs:639, 656 | `transform_into_nan`, reachable through the public `transform_nan_protocol`, differs from `transform`. A zero diagonal with a NaN input gives 0.0 (C: BAD), and a BAD full-matrix coefficient is multiplied in instead of forcing BAD. | Diagonal `diag(0,2)` on (NaN,1) through `transform_nan_protocol`: 0.0 vs BAD. |
| MX-5 | LOW | matrixmap.c:2387 (parent full-set path first), 2490-2505 (declines a kept row with a BAD selected element) | mappings/matrix.rs:891-974 | `map_split` never tries the parent full-set split (C returns the original, inverse and Invert included), so it re-inverts the effective matrix. It also copies BAD selected elements where C declines the split. | Full-set split of an inverted 2×2 MatrixMap: last-bit differences in the inverse. |

### PermMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| PM-1 | HIGH, probed | permmap.c:2412 (a BAD constant is written unset, `#Con1 = "<bad>"`); 3180-3181 (default AST__BAD) | mappings/perm.rs:1348-1354 | On load, a missing or commented `ConN` becomes 0.0 instead of AST__BAD. A C dump of a PermMap with a BAD constant therefore loads with that constant at 0.0, and outputs 0.0 where C outputs BAD. | C `astPermMap(1,[1],2,[1,-1],{AST__BAD})`, dumped and read by Rust; forward (5): C (5, BAD), Rust (5, 0). |
| PM-2 | MED | permmap.c:2328, 2377, 2389-2407 | mappings/perm.rs:1288-1302 | `Nconst` and `ConN` are written for every supplied constant. C writes only up to the highest index referenced by In/Out. Rust also writes a BAD constant uncommented, where C writes it commented. | `PermMap::new(&[1], &[1,-1], &[5.0,6.0])`: Rust writes `Nconst=2`, C writes 1. |
| PM-3 | LOW | permmap.c:2073-2079, 2269-2271 | mappings/perm.rs:254-290, 1221-1228 | `test_perm_split` returns the value, not whether it is set, and the dump writes `pmsplt` only when true. C writes it whenever it is set, including `pmsplt = 0`. | `set_perm_split(false)`: C TestPermSplit=1. |

### WinMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| WM-1 | LOW | winmap.c:584-586 (identical terms with different Invert give 0) | mappings/winmap.rs:297-307 | `equal` compares effective terms, so a self-inverse WinMap equals its inverted copy. C returns 0 in that case. | `WinMap::new(&[0.0],&[-1.0])` vs the same inverted: C 0, Rust true. |

### LutMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| LM-1 | HIGH, probed | lutmap.c:1620-1630, 1655-1688 (bisects the compressed `luti` table; BAD if both bracketing entries adjoin a bad entry) | mappings/lutmap.rs:386-405, 552-571 | The inverse never terminates on tables with BAD entries. When the midpoint is BAD, `find_non_bad_near` returns `low` itself, so `low` never advances. | `LutMap::new(&[1.0, BAD, 3.0], 1.0, 1.0)`, inverse of 2.0: C BAD; Rust was still running when killed after 10 s. |
| LM-2 | HIGH, probed | lutmap.c:1693-1709 (NEAR inverse returns `start + inc*index` of the nearest entry) | mappings/lutmap.rs:383-489 (`interp` never consulted) | The inverse transform ignores `LutInterp::Nearest` and always interpolates linearly. | `lut=[1,2,4]`, start 1, inc 1, Nearest, inverse of 2.4: C 2.0, Rust 2.2. |
| LM-3 | MED | lutmap.c:2375-2384 (AST__LUTIN for nlut<2, AST__LUTII for inc==0) | mappings/lutmap.rs:76-89 | A 1-element table is accepted. Both errors use `Badin` instead of LUTIN/LUTII. | `LutMap::new(&[5.0], 0.0, 1.0)`: C LUTIN, Rust Ok. |
| LM-4 | LOW | lutmap.c:1579-1587 (`ix >= nlut-1` takes the extrapolation branch and needs `lut[n-2]` good) | mappings/lutmap.rs:334-336 | Input exactly on the last node returns `lut[n-1]` even when `lut[n-2]` is BAD. | `[1,BAD,3]`, start 1, x=3: C BAD, Rust 3. |
| LM-5 | LOW | lutmap.c:1594 | mappings/lutmap.rs:344-345 | The forward interpolation is written `v_lo+f*(v_hi-v_lo)` rather than C's `lut[i]*(1-f)+lut[i+1]*f`, which gives ULP-level differences. | A bit-exact comparison against the C oracle. |
| LM-6 | LOW | lutmap.c:1553 (`floor(x+0.5)`) | mappings/lutmap.rs:371 (`round`, half away from zero) | The Nearest forward differs at exactly −0.5 increments below `start`. | Input `start-0.5*inc`: C `lut[0]`, Rust BAD. |
| LM-7 | LOW | lutmap.c:1793-1848 | mappings/lutmap.rs:107-185 | `LutInterp` and `LutEpsilon` have no test or clear. `set_lut_epsilon(BAD_VALUE)` stands in for clear. | `astTest(lm,"LutInterp")`. |

### PolyMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| PY-1 | HIGH, probed | polymap.c:4813-4828 (PolyPowers), 6240-6270 (Transform term loop) | mappings/polymap.rs:1302, 1446 | C propagates BAD per term: a BAD input sets only powers >=1 of that axis to BAD (power 0 stays 1.0), so outputs whose terms do not use that axis stay good. The Rust scalar path blanks the whole output point when any input is BAD (`point_has_bad`). The NaN fast path (polymap.rs:1407-1462) has no gate and propagates per term, so the two Rust paths also disagree with each other. | 2-in/2-out PolyMap y1=x1, y2=x2; forward of (BAD, 3): C (BAD, 3), Rust (BAD, BAD). |
| PY-2 | HIGH, probed | polymap.c:6402-6405 (GetIterInverse default `ncoeff_i==0 && nin==nout`), 2270-2300 (GetTranInverse) | mappings/polymap.rs:234-237, 324-326, 485-495, 1413-1427 | The Rust IterInverse default omits the `nin == nout` condition. A forward-only PolyMap with nin != nout therefore reports `has_inverse() == true`, where C reports no inverse. An inverse transform takes the iterative branch, and `get_jacobian_terms()` unwraps a `None` (the Jacobian is only built when nin == nout), so it panics. | `PolyMap::new(2,1,&[1*x1 -> out1],None)`: C TranInverse=0; Rust `has_inverse()` is true and the inverse transform panics. |
| PY-3 | MED | polymap.c:6406-6411 | mappings/polymap.rs:240-259 | `astSetIterInverse(1)` on a PolyMap with nin != nout raises AST__ATTIN and leaves the value unchanged. Rust accepts it, which leads to the PY-2 panic. | `set_iter_inverse(true)` on a 2->1 PolyMap. |
| PY-4 | LOW, probe did not confirm | polymap.c:2864-2906 (LinearGuess) | mappings/polymap.rs:540-673 | The initial guess for the iterative inverse is built differently. (1) C puts 1.0 on the diagonal of any output row that has no linear term; Rust leaves the row zero, finds the matrix singular, and falls back to an identity guess for all axes. (2) C assigns constant and linear coefficients (`=`, :2876/2880); Rust sums them (`+=`). The probe gave identical outputs for the case below, so a case where the different start changes the converged result has not been found. | Forward y1=10*x1+x1^3, y2=x2^2, no inverse; inverse of (30, 9) and (2, 9): C and Rust agree. |
| PY-5 | LOW | polymap.c:2699 (palDmat, pivot SFA=1e-20 in vendor/pal/palDmat.c:93), 2716-2720 (`vlensq <= maxerr*xlensq`) | mappings/polymap.rs:567, 787, 799, 819, 1166-1210 | Several constants have no C counterpart (CLAUDE.md stop signal 1). Singularity uses `|det| < 1e-30` (Cramer's rule for 2x2, own Gaussian elimination for NxN) where C uses palDmat's LU with a 1e-20 pivot test. Convergence uses `<` plus an extra `\|\| xlensq == 0.0`. Results differ in the last bits, and near-singular points can switch between BAD and good. PY-4's divergent guess belongs to the same port of `LinearGuess`. | Iterative inverse at a point where the Jacobian determinant is about 1e-25. |
| PY-6 | MED | polymap.c:423 (AddCoeff silently skips `c == 0.0`) | mappings/polymap.rs:143-151, 171-177 | A zero coefficient is a hard error (Badin) in Rust; C drops the term silently. Coefficient arrays from external sources (SIP, YAML, fitted) can contain exact zeros. ChebyMap inherits this through `PolyMap::new` (chebymap.rs:176). | `PolyMap::new(1,1,&[coeff 0.0, coeff 1.0*x],None)`: C succeeds, Rust errors. |
| PY-7 | LOW | polymap.c:6269 (`term == AST__BAD` -> output BAD) | mappings/polymap.rs:949-957 | A BAD coefficient makes the output BAD in C. In Rust it is multiplied through as -DBL_MAX, giving a garbage finite value. | Coefficient AST__BAD supplied to the constructor or loaded from a dump. |
| PY-8 | LOW | polymap.c:6444-6488, Dump 6774-6786 | mappings/polymap.rs:282-322, 1900-1913 | NiterInverse and TolInverse have no test/clear. The dump writes them only when they differ from 4 or 1e-6, while C writes them whenever TestX is true, so an explicitly set default is lost on round trip. | `set_niter_inverse(4)`, then dump: C writes `NiterInv = 4`, Rust writes nothing. |
| PY-9 | LOW | polymap.c:4624-4636 (PolyCoeffs picks the coefficient set by `forward != Invert`) | mappings/polymap.rs:324-345 | `forward_terms()` and `inverse_terms()` always return the original orientation. C astPolyCoeffs is relative to the current Invert state and groups the output by output axis. | Inverted PolyMap: C `astPolyCoeffs(map,1,..)` returns the original inverse coefficients. |
| PY-10 | LOW (UNVERIFIED) | polymap.c:5051-5133 (ReplaceTransformation via FitPoly1D/2D, cminpack lmder1) | mappings/helpers/poly_tran.rs:14-18, 262 | PolyTran is minimised with the `levenberg_marquardt` crate instead of lmder1, so fitted coefficients can differ at solver-tolerance level. | `poly_tran` on a quadratic 2-D map; compare coefficients with C astPolyTran. |
| PY-11 | LOW | polymap.c StoreArrays (AST__BADCI, AST__BADPW) | mappings/polymap.rs:120-184 | Wrong error codes: Badin for an illegal output index, where C uses AST__BADCI. | Term with out_axis 0. |

### ChebyMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| CH-1 | HIGH, probed | chebymap.c:1171-1192 (PolyPowers: BAD or out-of-box input sets T_n, n>=1, BAD for that axis only) | mappings/chebymap.rs:345-362 | Rust returns all outputs BAD when any input is BAD or outside the box. C goes BAD only for terms with order >= 1 on the offending axis, so outputs that do not depend on that axis survive. | 2->2 ChebyMap, y1=T1(x1), y2=T1(x2), box [0,1]²; forward of (5, 0.5) and of (BAD, 0.5): C (BAD, 0), Rust (BAD, BAD). |
| CH-2 | MED | chebymap.c:2126-2156 (only zero-width boxes are errors, AST__BADBX) | mappings/chebymap.rs:112-124, 149-160 | Rust rejects `lbnd > ubnd` (and uses Badin). C accepts reversed bounds, which give a negative scale. | `ChebyMap::new(.., lbnd_f=[1], ubnd_f=[-1], ..)`. |
| CH-3 | MED | chebymap.c:364-401 (ChebyDomain uses astMapBox of the other transformation when the requested domain is undefined) | mappings/chebymap.rs:236-246 | `domain()` returns `None` when the requested transformation has no stored box. C derives the box by MapBox. | ChebyMap with forward coefficients only; `domain(false)`. |
| CH-4 | MED | chebymap.c:1199-1266 (PolyTran override; bounds default to the Chebyshev box) and FitPoly1DInit/FitPoly2DInit (:1039-1040) | none | No PolyTran on ChebyMap: the inner PolyMap is private and is not Chebyshev-aware. C's ChebyMap fits a Chebyshev inverse through the FitPoly*Init seams, an architecture method in the sense of CLAUDE.md (H1). | `astPolyTran(chebymap,0,...)`. |
| CH-5 | LOW | chebymap.c:1154-1158 (a transformation with NULL scale is evaluated as an ordinary polynomial) | mappings/chebymap.rs:484-490 | An inverse with coefficients but no inverse box returns all-BAD in Rust. In C that is the standard-polynomial case. Latent until CH-4 lands. | A ChebyMap whose inverse is fitted by PolyTran. |
| CH-6 | LOW | as PY-6 | mappings/chebymap.rs:176 | A zero coefficient is rejected. | As PY-6. |

### WcsMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| WC-1 | HIGH (UNVERIFIED) | wcsmap.c:2522-2559 (IsZenithal), used by fitschan.c:29913 | fitschan/fset_to_store/sky_pole.rs:46-62, 109 | The Rust stand-in for IsZenithal also counts HPX and XPH as zenithal, and treats AZP, SIN and SZP as zenithal unconditionally. C has AZP zenithal only when PV2_2 is 0 or unset, SIN only when PV2_1 and PV2_2 are 0, SZP only when PV2_3 is ±90. For those cases C computes LONPOLE/LATPOLE from the pole; Rust writes only explicit PV1_3/PV1_4, so a header with a non-default pole loses LONPOLE. | Write a FrameSet with a HPX (or SIN with PV2_1=0.1) projection and a non-default native pole through FitsChan; compare the LONPOLE card. |
| WC-2 | MED | wcsmap.c:2055-2159 (GetPV) | mappings/wcsmap.rs:332-344 | `get_pv` returns `None` for any parameter not explicitly set. C GetPV returns the defaulted latitude-axis values (from `params.p` after InitPrjPrm, e.g. SZP PV2_3=90, AIR PV2_1=90, HPX 4/3) and, for non-TPN longitude axes, PVi_0=0, PVi_1=NatLon(deg), PVi_2=NatLat(deg). It also errors on out-of-range indices instead of returning None. | TAN WcsMap: `get_pv(lonax,2)`: C 90.0, Rust None. |
| WC-3 | MED | wcsmap.c:2804-2807 (missing required PV gives 400+i), 4310-4340 (AST__WCSPA / AST__WCSTY raised) | mappings/wcsmap.rs:911-921, 879-882; mappings/wcslib.rs:696-770 | A missing un-defaultable parameter (BON/COP/COE/COD/COO PV2_1) or a wcslib status 1 ("unusable parameters") is an error in C. Rust gives no error for any non-zero status. | COP WcsMap without PV2_1, transform (0.1, 0.2): C AST__WCSPA, Rust (NaN, NaN), which is also MB-2. |
| WC-4 | HIGH, probed | wcsmap.c:730 (NCP has NULL WcsFwd/WcsRev, so Map returns 1 and AST__WCSTY is raised) | mappings/wcsmap.rs:170-174, 516-520 | Rust silently implements NCP as SIN with default (zero) parameters, which is plain orthographic and not NCP. C refuses to transform. These are wrong coordinates instead of an error. | `WcsMap::new(2, Ncp, 1, 2)?.transform((0.1, 0.2))`: C raises "illegal projection type", Rust returns numbers. |
| WC-5 | HIGH, probed | wcsmap.c:6125-6130 (load reads `ProjP<m>` into the latitude axis) | mappings/wcsmap.rs:1373-1386 | The loader reads only `PV<i>_<m>`. The legacy `ProjP<m>` keys are silently dropped, so projection parameters from old dumps are lost. | Native COP dump with `ProjP1 = 45`: C reads PV2_1 = 45, Rust `get_pv(2,1)` is None. |
| WC-6 | LOW | wcsmap.c:4769-4799 vtab (ClearPV/ClearFITSProj/ClearTPNTan/ClearLonCheck), SetPV(AST__BAD) unsets (TestPV false) | mappings/wcsmap.rs:256-422 | No `clear_*` methods. `set_pv(axis,m,BAD_VALUE)` stores `Some(BAD)`, so `get_pv` returns `Some(-DBL_MAX)` where C TestPV is false. | `set_pv(2,1,BAD_VALUE)`, then `get_pv(2,1)`. |
| WC-7 | LOW | wcsmap.c:3894-3904 (AST__AXIIN) | mappings/wcsmap.rs:259-280 | Wrong error code for an invalid axis in set_pv: Wcsax where C uses AST__AXIIN. | `set_pv(3,1,…)` on a 2-axis WcsMap. |
| WC-8 | LOW | wcsmap.c:1496-1525 (Equal compares np/p arrays) | mappings/wcsmap.rs:645-658 | Equal treats an unset PV as 0.0. C distinguishes "unset" from "explicitly 0", but WC-2 makes C's GetPV values equal in most cases. | Two WcsMaps differing only in an explicit PV of 0. |
| WC-9 | LOW (UNVERIFIED) | wcsmap.c:1913-1929 (GetNatLat returns BAD if the conic PV2_1 is unset) | mappings/wcsmap.rs:540-558 | `nat_lat()` reads wcslib `theta0`, which conics set from `p[1]` (vendor/wcslib/proj.c:2541). With p[1] = BAD that gives -DBL_MAX*DD2R instead of BAD. | COP WcsMap without PV, `nat_lat()`. |
| WC-10 | MED | fitschan.c:38669 (`WcsNative` reports AST__BDFTS when `WcsNatPole` fails) | fitschan/wcs_celestial.rs:1082-1117 (`build_native_to_celestial_rotation_for_wcsmap` returns `None`) | A header whose LONPOLE cannot hold for its reference point is refused by C as ill-conditioned; the port's reader gets no rotation from `wcs_nat_pole` and reads the header anyway. Found by the WC-1 probe; a FitsChan read gap, outside this audit's Mappings. | HPX or CAR header with CRVAL2 = 40 and LONPOLE = 170: C AST__BDFTS, the port reads it (`NOT_REFUSED` in `tests/test_sky_pole_oracle.rs`). |

### SlaMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SA-1 | MED | slamap.c:774 (AddSlaCvt: wrong arg count raises AST__TIMIN) | mappings/slamap.rs:390-396, e.g. 970 | `add()` does not check the argument count. Too few arguments panics later with an index out of bounds in transform (`cvt.args[0]`, `args[1]`); extras are ignored. | `sla.add(SlaCvtType::Prec, &[1950.0])`, then transform. |
| SA-2 | LOW | slamap.c:860-945 (CvtCode) | mappings/slamap.rs:148-182 | `from_code` also accepts "DH2E"/"DE2H", which C rejects. The conversion list (28 types) and argument counts match C, and so do the transforms. | `SlaCvtType::from_code("DH2E")`. |

### SpecMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SP-1 | MED | specmap.c:4417-4421 (nin 1 or 3), 3706-3720, 1647-1700 (FrameChange per-point RA/Dec branch) | mappings/specmap.rs:1251-1257 | The 3-D SpecMap (nin=3, RA/Dec carried per point and used for rest-frame corrections) is not supported; Rust is fixed at nin=1. | `astSpecMap(3, 0, "")` with LKF2HL. |
| SP-2 | HIGH, probed | specmap.c load (reads Nin through Mapping) | mappings/specmap.rs:1535 (from_node never consults Nin) | Loading a Native dump of a 3-D SpecMap silently yields a 1-D SpecMap, which loses data and changes the mapping's dimensionality. | C dump of a 3-D SpecMap: C Nin=3, Rust `nin()`=1. |
| SP-3 | MED | specmap.c:677 (AST__TIMIN on wrong arg count) | mappings/specmap.rs:536-557 | `add()` pads missing arguments with 0.0 and ignores extras, so a missing rest frequency becomes 0 and gives wrong spectral values with no error. | `spec.add(FrToVl, &[])`. |
| SP-4 | LOW | specmap.c SystemChange (inf left as is for ENTOFR etc.) | mappings/specmap.rs:1339-1343 | Rust converts every non-finite final value to BAD; C does so only per conversion. | ENTOFR of 1e308. |

The conversion list (34 types), argument counts, SystemChange arithmetic, 1-D FrameChange caching and the Rate override all match C.

### TimeMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| TM-1 | MED | timemap.c:742 (AST__TIMIN) | mappings/timemap.rs:444-458 | `add()` pads missing arguments with BAD. For TAITOUTC/UTCTOTAI and TTTOTDB a BAD DTAI means "use astDat", so too few arguments silently switch semantics; for the others BAD enters the arithmetic. | `tm.add(MjdToMjd, &[0.0])`. |
| TM-2 | LOW | timemap.c:2473-2534 (Rate: analytic product of slopes; unity for time scales; numeric only for GMST; ignores Invert) | mappings/timemap.rs:1379-1381 | Rust always uses the numeric rate, so values differ slightly from C's analytic 1.0, 365.25 etc. | `rate` on MJDTOJEP: C exactly 1/365.25. |

The conversion list (28 types), argument counts, derived-argument setup (AddArgs) and every per-step formula match C.

### PcdMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| PC-1 | LOW | pcdmap.c:1213-1220 | mappings/pcdmap.rs:93-166 | No `test_disc`/`clear_disc`/`test_pcd_cen`/`clear_pcd_cen` (a setter given BAD_VALUE acts as clear). Transform, dump and load match. | `astTest(pcd,"Disco")`. |

### GrismMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| GR-1 | MED | grismmap.c:2123, 2404-2412 (astGrismMap leaves all 8 attributes unset) | mappings/grismmap.rs:108-157 | `GrismMap::new` and `default_map` mark every attribute as set. A user-built map therefore dumps all 8 cards uncommented, where C writes commented defaults for unset ones. There is no way to build one with unset attributes. FitsChan is unaffected because C sets all 8 there (fitschan.c:14456-14474). | `GrismMap::default_map()` dump vs C `astGrismMap("")` dump. |
| GR-2 | LOW | grismmap.c vtab Test*/Clear* | mappings/grismmap.rs:159-440 | No test/clear (a setter given BAD_VALUE or i32::MAX acts as clear). UpdateConstants and transform match. | `astTest(gm,"GrismNR")`. |

### SplineMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SN-1 | MED | splinemap.c:3433-3468 (AST__ATTIN for bad kx/ky/nx/ny and non-monotonic knots) | mappings/splinemap.rs:112-129 | `new` validates with `assert!` (a panic, not an error) and does not check that knots are monotonic. | `SplineMap::new` with decreasing tx. |
| SN-2 | MED | splinemap.c:3278-3298 (public astSplineMap takes SciPy order and transposes), 2160-2180 (astSplineCoeffs transposes back) | mappings/splinemap.rs:93-106, 164-171 | The Rust constructor takes, and `cu()`/`cv()` return, the internal CMLIB/Fortran order (documented). Code ported from the C/pyast API that passes SciPy-ordered coefficients gets a transposed spline and wrong coordinates. No accessor matches astSplineCoeffs. | Spline built by `SplineMap::new` from SciPy coefficients, of any shape: both orders have nx*ny elements, so nothing is refused. |
| SN-3 | LOW | splinemap.c:1569 (palDmat) | mappings/splinemap.rs:799 | The iterative inverse uses Cramer's rule with `\|det\| < 1e-300` instead of palDmat (1e-20 pivot); a constant with no C analogue. | Near-singular Jacobian. |
| SN-4 | LOW | splinemap.c:1858-1973 (Rate ignores Invert: always the forward spline derivative) | mappings/splinemap.rs:933-951 | Rust uses the analytic rate only when not inverted and falls back to numeric otherwise. C returns the forward derivative even when inverted (a C quirk). | `rate` on an inverted SplineMap. |
| SN-5 | LOW | splinemap.c:2560-2652, Dump 2962-2975 | mappings/splinemap.rs:252-305, 1018-1046 | No test/clear for InvNiter, InvTol or OutUnit; OutUnit explicitly set to 0 is not dumped. | `set_out_unit(false)`, then dump. |

### UnitNormMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| UN-1 | MED | unitnormmap.c:1580 (AST__BADSM when the centre has < 1 axis) | mappings/unitnormmap.rs:57-65 | `new(vec![])` is accepted, giving a 0->1 mapping. | `UnitNormMap::new(Vec::<f64>::new())`. |

Transform, Equal and Dump match.
C's bad-centre branch indexes `ptr_out[point][coord]`, a C defect; the Rust code is correct there.

### SphMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| SH-1 | LOW | sphmap.c:535-543 | mappings/sphmap.rs:69-146 | No test/clear for UnitRadius or PolarLong. | `astTest(sph,"PolarLong")`. |
| SH-2 | LOW | sphmap.c:1436-1441 | mappings/sphmap.rs:119-146 | `set_polar_long(BAD_VALUE)` stores it as set and the getter returns -DBL_MAX. In C AST__BAD means unset and the getter returns 0.0. | `set_polar_long(BAD_VALUE); polar_long()`. |
| SH-3 | LOW | sphmap.c:1577-1585 | mappings/sphmap.rs:419-428, 452 | UnitRadius explicitly set to 0 is dumped by C as `UntRd = 0`; Rust writes nothing and cannot tell set from unset. | `set_unit_radius(false)`, then dump. |
| SH-4 | LOW | sphmap.c Equal (raw fields: unset differs from explicit default) | mappings/sphmap.rs:413-420 | Equal compares effective values. | A SphMap with PolarLong set to 0 against one with it unset. |

Transform matches C, including the pole test.

### DssMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| DS-1 | MED | dssmap.c:1860-1930 (load raises AST__RDERR for missing PltRA..YPixSz, PPO3, PPO6, AMDX1-13, AMDY1-13) | mappings/dssmap.rs:807-834 | The Rust loader silently substitutes BAD (plate parameters) or 0.0 (coefficients), so a truncated dump yields garbage coordinates instead of an error. | Native DssMap dump with `AMDX1` removed. |
| DS-2 | LOW | dssmap.c DssFits (vtab) | none | No DssFits. It has no caller inside AST (astDssMap itself is only reached via Load). | `astDssFits` from user code. |

platepos, platepix and Transform match C exactly.

### MathMap

| ID | Rating | C | Rust | Finding | Exposed by |
|---|---|---|---|---|---|
| MM-1 | LOW | mathmap.c:630-648 (Clear/Test SimpFI/SimpIF) | mappings/mathmap/mod.rs:182-235 | SimpFI and SimpIF have get/set only, no test/clear. Seed is complete. | `astTest(mm,"SimpFI")`. |
| MM-2 | LOW | mathmap.c astInitMathMap (AST__INNTF for too few functions) | mappings/mathmap/mod.rs:131-151 | Wrong error code: Badni instead of AST__INNTF. | `MathMap::new(1,2,&["y=x"],..)`. |

The function and operator table (all 55 names plus the operators), constant parsing (`d` exponent), per-operation BAD and overflow semantics, tristate AND/OR, QIF with an unchecked condition, bit operations, max/min, and the ran2 generator with its Seed handling all match C.

### IntraMap and XphMap (both ported)

**IntraMap** (intramap.c) wraps a user-registered C or Fortran transformation function (`astIntraReg`/`astIntraRegFor`) under a name, with the flags AST__NOFWD, AST__NOINV, AST__SIMPFI and AST__SIMPIF and the attribute IntraFlag.
Its vtab provides Transform (calling the registered function), MapMerge (cancelling forward/inverse pairs when SIMPFI/SIMPIF are set and the IntraFlags match), Equal, the GetNin/GetNout overrides, and IntraFlag get/set/test/clear.
Nothing inside AST creates one; the only callers are user code and fintramap.c.
Without it, applications that register private transforms lose the feature, and any dump containing an IntraMap fails to load (C also fails unless the function is registered in-process).

**Ported 2026-10-08** as `mappings/intramap.rs`: the transformation is an `Arc<dyn IntraTransform>`, as the design spec has it, and the registry a dump's function name is looked up in belongs to the Channel that reads it (`Channel::register_intra`, holding an `IntraRegistry`) rather than to the process.
Transform, Equal (both IntraFlags set and equal), MapMerge (the class's own trait method, `docs/audits/mapmerge/intramap.md`), the NOFWD/NOINV definedness, IntraFlag get/set/test/clear and the dump follow C.
The 10 fixtures it blocked are back in the simplify, serialisation and transform-oracle gates, which read with the C test programs' four identity transformations registered (`tests/common/mod.rs`).

**XphMap** (xphmap.c) maps grid coordinates in one of four HEALPix layouts (HPX0, HPX12, XPHN, XPHS) at a given order onto HPX12 grid coordinates.
Its vtab provides Transform, Equal, GetIsLinear (linear only for HPX12) and MapMerge (an equal and opposite pair cancels).
Its constructor is protected: the public `ast.h` declares the type and `astIsAXphMap` but not `astXphMap`, so its only creator is moc.c (lines 2084, 2208, 2576, 2687), and otherwise an XphMap is read from a dump.
FitsChan does not use it; the HPX and XPH FITS projections go through WcsMap and wcslib.

**Ported 2026-10-07** as `mappings/xphmap.rs`, with a crate-private constructor as C's is protected: Transform transcribed per projection and direction, Equal, IsLinear, and MapMerge as the class's own trait method (`docs/audits/mapmerge/xphmap.md`).
`tests/test_xphmap.rs` ports `testxphmap.c` and compares each projection's forward and inverse transformations with C's bit for bit (`tools/oracle_fixtures/xphmap_probe.c`); the 7 fixtures it blocked are back in the simplify, serialisation and transform-oracle gates.

## C methods with no Rust counterpart

| C method | Classes | Impact |
|---|---|---|
| `astResample<X>` (4- and 8-byte, all data types), with `astUinterp`/`astUkern1` | Mapping | Image regridding, used by KAPPA, SMURF and pyast. `mappings::resample` has nearest and linear interpolation; the kernels follow (`docs/superpowers/plans/2026-10-09-resample-rebin.md`). |
| `astRebin<X>`, `astRebinSeq<X>` | Mapping | Flux-conserving rebinning and SMURF map-making. Deferred in PLAN.md §7. |
| `astTranN`, `astTranP` | Mapping | Not needed by the Rust API; `ast-capi` must provide them (see "The `astTran*` interfaces"). |
| `astDecompose` | Mapping (base), CmpMap (cmpmap.c:881), TranMap (tranmap.c:368) | The component accessors are `pub(crate)` (mappings/cmpmap.rs:166-177, TranMap `map1`/`map2`), so `ast-capi` cannot implement it on the public API either; `docs/issues/frame-mapping-c-port-gaps.md` defers it to `ast-capi`. |
| `GetIsLinear` / `IsLinear` | Mapping, CmpMap, UnitMap, ZoomMap, ShiftMap, MatrixMap, WinMap, XphMap | MB-4. |
| `SetInvert` / `ClearInvert` / `TestInvert` | Mapping | Only a toggle exists (MB-6). |
| `RemoveRegions` overrides | TranMap, RateMap, NormMap | `regions::remove_regions` has arms only for Region, CmpMap and SwitchMap (TR-3, RM-3, NM-2). |
| `PolyPowers` (vtab seam) | PolyMap, overridden by ChebyMap | Rust has two separate evaluators, each with the per-axis BAD error (PY-1, CH-1). C has one Transform with a per-class power hook. |
| `FitPoly1DInit` / `FitPoly2DInit` (vtab seam), PolyTran override | PolyMap, overridden by ChebyMap | No Chebyshev inverse fitting (CH-4). |
| `ChebyDomain` MapBox fallback | ChebyMap | Ported (CH-3, fixed). |
| `IsZenithal` | WcsMap | Re-implemented in FitsChan with different rules (WC-1). |
| GetPV defaults, `ClearPV` / `TestPV`, `ClearFITSProj` / `ClearTPNTan` / `ClearLonCheck` | WcsMap | WC-2, WC-6; `TestPV` only as `get_pv().is_some()`. |
| `SplineCoeffs` (SciPy order) | SplineMap | Ported as `SplineMap::coeffs` (SN-2, fixed). |
| `DssFits` | DssMap | No internal callers (DS-2). |
| Analytic Rate | TimeMap | Numeric only (TM-2). |
| 3-D Transform branch, nin=3 constructor | SpecMap | SP-1, SP-2. |
| String-coded `SlaAdd` / `SpecAdd` / `TimeAdd` argument-count validation | SlaMap, SpecMap, TimeMap | The typed `add()` exists but does not validate the count (SA-1, SP-3, TM-1). |

## Attribute status

Attributes whose accessors are complete and correct are omitted: IsSimple, Nin, Nout, Report, PVMax, WcsType, WcsAxis, NatLat/NatLon (WC-9 aside), SplineKx/Ky/Nx/Ny, and Seed.
SlaMap, SpecMap, TimeMap, UnitNormMap, DssMap, CmpMap, TranMap, RateMap, NormMap, SelectorMap, SwitchMap, UnitMap, ShiftMap, MatrixMap and WinMap have no class attributes.

| Class | Attribute | C access | Rust status |
|---|---|---|---|
| Mapping | Invert | get/set/test/clear | `is_inverted` and the toggle `invert` only (MB-6) |
| Mapping | IsLinear | get (read-only) | missing (MB-4) |
| Mapping | TranForward / TranInverse | get (read-only) | `has_forward` / `has_inverse`, wrong for SwitchMap under Invert (SW-1), for a dimension-changing PolyMap (PY-2) and for a MatrixMap with a BAD element (MX-1) |
| ZoomMap | Zoom | get/set/test/clear | get/set only (ZM-1) |
| PermMap | PermSplit | get/set/test/clear | all four, but test returns the value (PM-3) |
| LutMap | LutInterp, LutEpsilon | get/set/test/clear | get/set only (LM-7) |
| PolyMap | IterInverse | get/set/test/clear | complete, but the default and setter ignore nin != nout (PY-2, PY-3) |
| PolyMap | NiterInverse, TolInverse | get/set/test/clear | get/set only; set defaults not dumped (PY-8) |
| WcsMap | PV(i,m) | get/set/test/clear | set; get without defaults (WC-2); test via `Option`; no clear (WC-6) |
| WcsMap | FITSProj, TPNTan, LonCheck (protected) | get/set/test/clear | no clear (WC-6) |
| PcdMap | Disco, PcdCen(axis) | get/set/test/clear | get/set; BAD acts as clear; no test (PC-1) |
| GrismMap | GrismNR, GrismNRP, GrismWaveR, GrismAlpha, GrismG, GrismM, GrismEps, GrismTheta | get/set/test/clear | get/set; a sentinel acts as clear; no test; the constructor marks all set (GR-1, GR-2) |
| SplineMap | InvNiter, InvTol, OutUnit | get/set/test/clear | get/set only (SN-5) |
| SphMap | UnitRadius, PolarLong | get/set/test/clear | get/set only (SH-1 to SH-3) |
| MathMap | SimpFI, SimpIF | get/set/test/clear | get/set only (MM-1) |
| IntraMap | IntraFlag | get/set/test/clear | ported with the class (2026-10-08) |

## Corrections to other documents

- PLAN.md §2, the "`astTran*` definedness validation" row, says C validates in the public `astTran*` entry points rather than in `astTransform`.
  C's base `Transform` (mapping.c:22333-22380), which every class's `Transform` calls, raises `AST__TRNND` and the three shape errors, so `astTransform` validates too (MB-1).
  The row now says so and points here.

## Probe sources

The paired C and Rust probes are in the session scratchpad and are not committed.
They covered PY-1, PY-2, PY-4, CH-1, WC-4, WC-5, SP-2, NM-1, MX-1, MX-2, PM-1, LM-1 and LM-2; for the load findings, the C probe wrote the dump and the Rust probe read it.
To make any finding a regression test, port the probe's inputs and C output into a `crates/ast-core/tests/` oracle case, following `docs/issues/c-oracle-probe-recipe.md`.

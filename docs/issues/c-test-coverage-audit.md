# C test coverage audit

An audit, on 2026-10-14, of every test program in `reference/starlink-ast/ast_tester/` against the Rust tests in `crates/ast-core/tests/`.
A C check counts as covered when a Rust test, or an oracle case replaying C, exercises the same behavior; where the values differ, the entry says so.
`testfitschan.c` was audited separately and its gaps are in [`testfitschan-coverage.md`](testfitschan-coverage.md).

## Out of scope

- **Plot not ported:** `testgrid.c`, `testplot3d.c`, `testplotter.c`.
- **Channel classes not ported:** `testyamlchan.c` (YamlChan), `testxmlchan.c` (XmlChan), `teststcschan.c` (StcsChan), and `teststc.c`, whose seven functions each read STC XML through `astXmlChan` although the Stc classes themselves are ported.
- **C threading:** `testthreads.c`.
- **C tooling, not tests of the library:** `ast_astequal.c`, `ast_test.c`, `check_transform_oracle.c`, `compare_dumps.c`, `draw3d-subs.c`, `gen_*.c`, `normalize_fixture.c`, `plplotutil.c`, `simplify.c`, `test_bbox_values.c`, `test_oracle_util.c`, `transform_oracle_util.c`, `wcsconverter.c`.
  The ctests these drive (`wcsconv_*`, `simplify_*`, `roundtrip_*`, `keymap_mapsz`, `transform_oracle_*`) have Rust harnesses.

## Summary

| C program | Verdict |
|---|---|
| `testaxis.c` | covered (`test_frame.rs::unit_is_simplified`) |
| `testslamap.c` | covered (`test_slamap.rs`) |
| `testunitnorm.c` | covered (`test_frame.rs::norm_unit_matches_the_c_library`) |
| `testerror.c` | no Rust equivalent: the `astSetPutErr` reporter and a ShiftMap with -1 axes, which cannot be built |
| `testpointset.c` | covered, apart from the options-string constructor and the `ID` attribute, which a plain-data PointSet does not have |
| `testobject.c` | partial; the rest is the handle registry and creation-site tracking |
| `testchannel.c` | partial |
| `testcmpmap.c` | ported |
| `testlutmap.c` | ported |
| `testnormmap.c` | ported |
| `testrate.c` | ported (`test_rate.rs`) |
| `testratemap.c` | ported (`test_ratemap.rs`) |
| `testresimp.c` | partial |
| `testunitnormmap.c` | mostly covered |
| `testzoommap.c` | ported, apart from the `AST__IMMUT` checks, which belong to `ast-capi` |
| `testmapping.c` | partial |
| `testregions.c` | 18 of 30 functions covered, 11 partial, 1 missing; two functions never run in C |
| `testregionmasking.c` | ported (`test_region_masking.rs`) |
| `testfitschanerrors.c` | 22 of 41 cases covered, 3 partial, 11 missing, 5 with no Rust equivalent |
| `testswitchmap.c` | partial |
| `testchebymap.c` | partial; its PolyTran half needs `ChebyMap::poly_tran`, which is not ported |
| `testpolymap.c` | mostly covered |

## Findings that are more than a missing test

- **Two `testregions.c` functions never ran in C; fixed upstream-side in `f111c05c` on `u/timj/norm-unit`.**
  `checkBox` and `checkConvex` tested the pointer `status` rather than `*status`, so both returned at once.
  Once they ran, `checkBox` failed on three slips in its conversion from `testregions.f`, not on the library: single-precision literals made double, `AST__BAD` used for a float mask where Fortran used `VAL__BADR`, and the Fortran arrays indexed transposed.
  With those fixed the C test passes, so `checkBox`'s values are now verified against the library.
- **ChebyMap's PolyTran is not ported.**
  `chebymap.c` overrides `PolyTran` (installed at `chebymap.c:1039`), fitting in Chebyshev form over the Mapping's own bounding box; the port has `PolyMap::poly_tran` only.
- **`testunitnormmap.c` Error 6b was a missing test, not a behavior gap.**
  Transforming the centre gives BAD unit components and a zero norm, and C requires the inverse to give the centre back (`testunitnormmap.c:186-187`); the port's zero-norm branch takes precedence as C's does.
  `test_unitnormmap.rs::not_linear_and_round_trips_including_the_centre` now pins it with Errors 5 and 6.
- **`testselfsimplifyafterset` was recorded as unconstructible after its setters existed.**
  It is now `test_mapping.rs::a_simplified_mapping_set_to_unit_simplifies_again`.

## Gaps by program

### `testchannel.c`

- Err 1-6 (`:54-93`): a default SkyFrame written through `SinkFile`, read back, and compared with `astEqual`, twice; `test_channel.rs::round_trip_skyframe_via_file` checks only `naxes` and System.
- Err 0/-1 (`astTuneC("hrdel")`) and Err 7 (one Channel with both `SinkFile` and `SourceFile`) have no Rust equivalent.

### `testobject.c`

- `TestCheckToString` (`:20-57`): the FrameSet from `astConvert(Frame[Domain=SKY], SkyFrame)` written to a string, read back, written again identically, and equal to the original.

### `testresimp.c`

- Err 4-8 (`:52-76`): a CmpMap whose ZoomMap has `AllowSimplify`, followed by its inverse, simplifies to a UnitMap unrestricted, and under a restricted simplify to a ShiftMap followed by an inverted ShiftMap.
- Err 9-10 (`:81-95`): an unrestricted simplify of Shift(1), Zoom(2), Shift(-2) gives a ZoomMap of 2.0; only the restricted variant is in the corpus (`cap_zoommap_01`).

### `testunitnormmap.c`

- `testsimplify` (`:89-155`): seven combinations with centres (2,-1,0) and (-1,6,4) and a shift of (3,7,-9) give ShiftMap, UnitNormMap, UnitNormMap, CmpMap, UnitNormMap, UnitNormMap and CmpMap; only the fifth is an exact fixture (`cap_unitnormmap_01`).

### `testmapping.c`

- Err 0-1 (`:583-597`): `linear_approx` of the PolyMap out1 = 1+2x, out2 = 1+3y over [-1,1]² with tolerance 0.001 gives [1, 1, 2, 0, 0, 3].
- Err 2-3 (`:599-610`): with coefficient 12 BAD, the fit succeeds with elements 1, 4 and 5 BAD.
- Err 4 (`:612-615`): adding a 3y² term makes `linear_approx` refuse.
- Err 8 (`:655-659`): PermMap equality with a referenced constant (5.0 in both) and a differing unreferenced one (7 against 9).
- `testwcsmapequal` (`:494-514`): `test_equal_oracle.rs` compares one direction; C also checks the reverse for cases 1 and 3.

### `testregions.c`

- `checkPolygonMaskLargeLobe` (`:3210-3302`), missing: an ICRS Polygon wound so its interior is the large lobe, masked over a 21×21 grid, sets all 441 pixels.
- `checkInterval` (`:168-666`): InternalUnit on a SkyFrame + SpecFrame(Angstrom) CmpFrame; 3-D CmpFrame Intervals with BAD, reversed and one-sided limits, closed and negated; overlap codes 1-10 for half-unbounded Intervals; MapRegion through PermMaps with constants giving an Interval or NullRegion.
- `checkPolygon` (`:667-800`): a sky Polygon closed, open and negated; whether a FrameSet is held before and after System=FK5 and after simplify; a polar Polygon's bounds, disc, and a Prism with an Interval keeping them after simplify.
- `checkCircle` (`:2049-2345`): disc AA5-7, mesh counts 200 and 201 and mesh distances, Galactic simplify with and without a FrameSet, overlaps 10-16 on C's circles, pole bounds 16a-d, a different-Domain overlap, and a 4-D CmpFrame self-overlap.
- `checkEllipse` (`:2346-2603`): an Ellipse in a permuted FK4 + wavelength CmpFrame, FITS TAN pole bounds, membership on both axes, overlaps 5-14, a MatrixMap round trip, and RA-wrap overlaps 20-22.
- `checkPrism` (`:2869-2957`): an FK5 Box × SpecFrame Interval simplifying to a Box, overlap with a negated copy, Closed=0 overlaps, half-open wavelength Intervals in Galactic and frequency, and pole Box bounds.
- `checkCmpRegion` (`:2682-2834`): an FK5 Box with a Galactic Circle under AND and OR, its disc, overlap through `astConvert` and MapRegion, and A OR ¬A and A AND ¬A simplifying to a negated and a plain NullRegion.
- `checkNullRegion` (`:2604-2681`): FK4 to ICRS simplify without a FrameSet; `astMask8D` on a negated NullRegion (no change) and a plain one (25 pixels set BAD).
- `checkPointList` (`:1940-2048`): a SpecFrame PointList overlapping itself (5) and its negation (6); MapRegion through y=x² and simplify; `astMaskI` through a WinMap.
- `checkRemoveRegions` (`:2958-3015`): tests 4-9, `astConvert` to a Region giving a clipping FrameSet that `remove_regions` then unclips.
- `checkBoxTransformAfterBounds` (`:3174-3192`): the bounds of a zero-width Box stay at 5, and are taken before the transform.
- `checkBox` (`:801-1505`): never run in C (see above); generic analogues exist, but none of its sky, MapRegion, mask or mesh-count checks.

### `testfitschanerrors.c`

Missing:

- `tnx-unsupported` (`:196`) and `zpx-unsupported` (`:217`): the "unsupported IRAF" warnings from WAT cards.
- `aips-reject-ait-nonorigin` (`:240`), `aips-reject-wave` (`:263`), `aips++-reject-wave` (`:284`): FITS-AIPS or FITS-AIPS++ writes nothing where FITS-WCS writes.
- `error-cdelt-zero` (`:373`): CDELT1=0 on RA---TAN still reads.
- `error-mixed-celestial` (`:417`), `error-duplicate-lat` (`:460`), `error-unknown-lat-projection` (`:496`), `error-unknown-spectral-algorithm` (`:539`): each gives AST__BDFTS.
- `tab-missing-axis` (`:879-929`): a 3-D COORDS column with only PV1_3 gives AST__BADTAB.

Partial:

- `error-duplicate-lon` (`:403`): the Rust case pairs RA with GLON, C pairs RA with RA.
- `badkeyname-nonprint` (`:489`): the non-printing-character branch.
- `aips-reject-pixel-shear` (`:353`): a non-celestial CD shear, which takes a different path from the celestial one tested.

No Rust equivalent: bad Encoding and AltAxes strings (AST__BADAT), and setting or clearing the read-only Ncard and AllWarnings (AST__NOWRT).

### `testswitchmap.c`

- Inverse transforms through MathMap `qif` inverse selectors: section 1 Err 9-24, section 2 Err 109-123, section 3 Err 140-147.
- Section 2 forward, Err 101-108, with a MathMap `qif` forward selector.
- Section 3 rates, Err 151-154.
- Partial: section 1 forward 1-8 and section 3 forward 132-139 on C's routes, rates 28-31 and 128-131, and the no-change simplify of "ast_equal 1-3".

### `testchebymap.c`

- Needing `ChebyMap::poly_tran`: Err 5-8, 102-109, 11-20, 29-36 and 501-506.
- Partial: the exact sum 1.5·T0 − T2 + 2·T3 + 1.3·T4 (Err 0-3), C's coefficient sets evaluated (Err 10, 101), and the inverse domain through MapBox for a forward-only map (Err 21-28).

### `testpolymap.c`

- PolyTran round trips 1-7 (`:77-171`) and the IterInverse round trips on the fitted maps, with C's maps and ranges.
- Err 8007-8015: IterInverse and TranForward after `astInvert`.

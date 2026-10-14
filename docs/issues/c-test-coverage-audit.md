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
| `testobject.c` | ported, apart from the handle registry and creation-site tracking |
| `testchannel.c` | ported, apart from two checks with no Rust equivalent |
| `testcmpmap.c` | ported |
| `testlutmap.c` | ported |
| `testnormmap.c` | ported |
| `testrate.c` | ported (`test_rate.rs`) |
| `testratemap.c` | ported (`test_ratemap.rs`) |
| `testresimp.c` | ported |
| `testunitnormmap.c` | mostly covered |
| `testzoommap.c` | ported, apart from the `AST__IMMUT` checks, which belong to `ast-capi` |
| `testmapping.c` | ported, apart from a no-op `Invert` set, which a toggling `invert()` cannot express |
| `testregions.c` | all 30 functions covered; `checkEllipse` Errors 15-17 are a recorded gap |
| `testregionmasking.c` | ported (`test_region_masking.rs`) |
| `testfitschanerrors.c` | ported (`test_fitschan_errors.rs`); one case exposes a duplicate-keyword gap in the reader |
| `testswitchmap.c` | ported; exposed and fixed a SwitchMap cancellation gap |
| `testchebymap.c` | ported, apart from Error 4 (IterInverse, not exposed on ChebyMap) |
| `testpolymap.c` | ported; Errors 3002-3003 are a recorded gap |

## Findings that are more than a missing test

- **Two `testregions.c` functions never ran in C; fixed upstream-side in `f111c05c` on `u/timj/norm-unit`.**
  `checkBox` and `checkConvex` tested the pointer `status` rather than `*status`, so both returned at once.
  Once they ran, `checkBox` failed on three slips in its conversion from `testregions.f`, not on the library: single-precision literals made double, `AST__BAD` used for a float mask where Fortran used `VAL__BADR`, and the Fortran arrays indexed transposed.
  With those fixed the C test passes, so `checkBox`'s values are now verified against the library.
  The port (`test_regions_c.rs::check_box_*`) matched them with no library change.
- **ChebyMap's PolyTran was not ported, and a ChebyMap kept an inverse box C drops.**
  `ChebyMap::poly_tran` now ports `chebymap.c` `PolyTran` through C's `FitPoly1DInit` / `FitPoly2DInit` seams, and a box is kept only for a transformation with coefficients, as `chebymap.c:2124-2158` does, which Errors 25-28 exposed (CH-4 and CH-7 in `docs/audits/mapping-c-parity.md`).
- **`testunitnormmap.c` Error 6b was a missing test, not a behavior gap.**
  Transforming the centre gives BAD unit components and a zero norm, and C requires the inverse to give the centre back (`testunitnormmap.c:186-187`); the port's zero-norm branch takes precedence as C's does.
  `test_unitnormmap.rs::not_linear_and_round_trips_including_the_centre` now pins it with Errors 5 and 6.
- **`testselfsimplifyafterset` was recorded as unconstructible after its setters existed.**
  It is now `test_mapping.rs::a_simplified_mapping_set_to_unit_simplifies_again`.

## Gaps by program

### `testunitnormmap.c`

- `testsimplify` (`:89-155`): seven combinations with centres (2,-1,0) and (-1,6,4) and a shift of (3,7,-9) give ShiftMap, UnitNormMap, UnitNormMap, CmpMap, UnitNormMap, UnitNormMap and CmpMap; only the fifth is an exact fixture (`cap_unitnormmap_01`).

### `testregions.c`

- `checkEllipse` Errors 15-17: a recorded gap, `REGIONS_C_GAP_EXCLUDED` in `test_regions_c.rs`.

### `testpolymap.c`

- Errors 3002-3003: a recorded gap, `POLYMAP_C_GAP` in `test_polymap.rs`; C splits a CmpMap a merge creates (`cmpmap.c:1677-1734`) and the port does not.

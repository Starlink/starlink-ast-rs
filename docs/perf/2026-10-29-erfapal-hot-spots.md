# Where PAL and ERFA time goes after the port

This is a measurement and a proposal; it changes no library code.
Nothing here is decided: the table ends with what each function would be, and the user chooses which of them Task 14 vectorizes.

## Method

- Calls per point: `count_one` (`--features call-counts`) on each input, forward and inverse, over the 24 oracle points, steady-state second call; counts below are per point.
  The six FrameSets are `wcsconv/inputs/*.ast`; the four PAL cases are `crates/ast-bench/fixtures/{slamap_amp,slamap_fk45z,slamap_prec,timemap_mjdtobep}.map` against `bench.oracle`.
- Timing: the Task 12 snapshot (`2026-10-29-macos-aarch64-native-erfapal-ported.tsv`, commit `168a4da0`, load average 7.1 falling to 4.8), median ns per point, Rust / C.
  The snapshot's point counts are 1, 10, 100, 100 000 and 1 000 000; there is no 10 000 row, so 100 000 stands in for the large-N column.
- Profile: `sample ast-bench 5 1` on `ast-bench run --engine rust --set-file <one case>` repeated in a loop (release build, load average 3.5 to 4.7), so each profile is the case's forward and inverse together at all point counts.
  Raw output is `target/perf/sample-<case>.txt` (not committed).
  Self time is grouped into the ported PAL/ERFA body, libm (`atan2`, `__sincos_stret`, `fmod`, `sqrt` and their stubs), and everything else; libm is attributed to a function by its immediate caller in the call graph.

## Calls per point

`Dcs2c` and `Dcc2s` from `SphMap` are the slice kernels (`dcs2c_slice`, `dcc2s_slice`), which already go through `vmath`; they are counted under the scalar names.
`Dmat` is `pal::dmat`/`dmat_det` and is reached from `PolyMap` and `SplineMap` inverse iterations, which solve a small system per point per iteration.
`Drange` comes from `WcsMap`'s forward preparation.

| Input | Forward, per point | Inverse, per point |
|---|---|---|
| `a20070718_00010_02_cube.ast` | Dcc2s 1, Dcs2c 1 | Dcc2s 1, Dcs2c 1, Drange 2 |
| `degen1.ast` | Dcc2s 1, Dcs2c 1 | Dcc2s 1, Dcs2c 1, Drange 2 |
| `dss.ast` | Dcc2s 1, Dcs2c 1 | Dcc2s 1, Dcs2c 1, Drange 2 |
| `lsst.ast` | Dcc2s 1, Dcs2c 1 | Dcc2s 1, Dcs2c 1, Dmat 3.1, Drange 2 |
| `lsst2.ast` | Dcc2s 1, Dcs2c 1 | Dcc2s 1, Dcs2c 1, Dmat 4.1, Drange 2 |
| `timj.ast` | Fk45z 1, Dcc2s 1, Dcs2c 1 | Fk54z 1, Dcc2s 1, Dcs2c 1, Drange 2 |
| `slamap_amp.map` | Ampqk 1 | Mapqkz 1 |
| `slamap_fk45z.map` | Fk45z 1 | Fk54z 1 |
| `slamap_prec.map` | Dcs2c 1, Dmxv 1, Dcc2s 1, Dranrm 1 (Prec once) | the same |
| `timemap_mjdtobep.map` | Epb 1 | Epb2d 1 |

No FrameSet reaches `Ampqk`, `Mapqkz`, `Epb`, `Epb2d`, `Prec`, `Dranrm` or any ERFA series function per point.
Five of the six FrameSets reach PAL per point only through the already-vectorized `SphMap` slice kernels, `Drange`, and (the two LSST sets) `Dmat`.
Only `timj.ast` reaches a PAL/ERFA function with trigonometry per point, and that is `Fk45z` forward and `Fk54z` inverse.

## Time

Median ns per point, Rust / C, forward then inverse.

| Input | N = 1 | N = 100 | N = 100 000 |
|---|---|---|---|
| `a20070718_00010_02_cube.ast` | 3467 / 15826, 3436 / 15225 | 92 / 216, 97 / 277 | 59 / 60, 62 / 121 |
| `degen1.ast` | 3871 / 17426, 4046 / 17323 | 96 / 231, 103 / 292 | 57 / 60, 61 / 122 |
| `dss.ast` | 2069 / 8221, 1821 / 8048 | 115 / 176, 223 / 309 | 92 / 91, 206 / 231 |
| `lsst.ast` | 8119 / 19425, 15593 / 38952 | 175 / 419, 544 / 1304 | 87 / 219, 362 / 907 |
| `lsst2.ast` | 8853 / 21954, 16773 / 41159 | 358 / 649, 1347 / 2213 | 259 / 425, 1141 / 1771 |
| `timj.ast` | 2641 / 12390, 2740 / 12349 | 132 / 234, 226 / 373 | 106 / 113, 199 / 253 |
| `slamap_amp.map` | 241 / 753, 105 / 610 | 199 / 203, 67 / 69 | 196 / 196, 67 / 64 |
| `slamap_fk45z.map` | 98 / 605, 169 / 666 | 61 / 62, 143 / 138 | 60 / 57, 141 / 132 |
| `slamap_prec.map` | 132 / 630, 125 / 645 | 52 / 58, 51 / 58 | 51 / 52, 50 / 52 |
| `timemap_mjdtobep.map` | 14.1 / 554, 13.9 / 555 | 2.9 / 7.2, 2.6 / 7.0 | 2.7 / 1.6, 2.4 / 1.5 |

At large N the Rust engine is within noise of C on every PAL case and no slower than C on any FrameSet.
Rust is ahead of C on the FrameSets because of the earlier native stages, not because of this one.

## Where the time goes (profile, forward and inverse together)

| Input | PAL/ERFA bodies | of which slice kernels | libm | everything else |
|---|---|---|---|---|
| `slamap_amp.map` | 27% (`ampqk` 22%, `mapqkz` 5%) | 0 | 67% (`atan2` 44%, `__sincos_stret` 10%, `fmod` 6%) | 6% |
| `slamap_fk45z.map` | 21% (`fk54z` 18.5%, `fk45z` 2%) | 0 | 74% (`atan2` 34%, `__sincos_stret` 17%, `fmod` 9%) | 5% |
| `slamap_prec.map` | 1% | 0 | 83% (`atan2` 38%, `__sincos_stret` 19%, `fmod` 13%), called from `SlaMap`'s inlined scalar path | 16% |
| `timemap_mjdtobep.map` | 0 | 0 | 0 | 100% (`TimeMap`'s own loop 89%) |
| `timj.ast` | 29% (`fk54z` 10%, `fk45z` 2.5%) | 17% | 52% (`atan2` 25%, `__sincos_stret` 10.5%); essentially all of it under `fk54z` and `fk45z` | 19% |
| `lsst.ast` | 28% (`dmat` 18%) | 10% | 3% | 69% (`PolyMap` 30%, 9% Newton-Raphson) |
| `lsst2.ast` | 10% (`dmat` 1.7%) | 8.5% | 4%, from `WcsMap` | 86% (`SplineMap` `dbvalu`/`dintrv` 56%) |
| `dss.ast` | 14% | 14% | 4%, from `WcsMap` | 81% (`TpnFwd` kernel 67%) |
| `degen1.ast` | 39% | 39% | 6%, from `WcsMap` | 56% |
| `a20070718_00010_02_cube.ast` | 36% | 36% | 5%, from `WcsMap` | 60% |

In every FrameSet except `timj.ast` the libm time is `WcsMap`'s projection (`fmod`, `atan2`), not PAL or ERFA.

## Table and proposals

A function is proposed only if a realistic input reaches it per point and its libm share is a large part of that input's time.
"Realistic" means one of the six FrameSets, or a Mapping class used per point in practice; the second route is marked where it is the only one.

| Function | Per-point callers | Calls per point | Share of time | libm share | Proposal |
|---|---|---|---|---|---|
| `pal::fk54z` and `pal::fk45z` | `SlaMap` FK45Z/FK54Z; `timj.ast` | 1 on `timj.ast` and on `slamap_fk45z.map` | `timj.ast` about 64% (body 12.5%, libm 52%); `slamap_fk45z.map` about 95% | about 50% of `timj.ast`, 74% of the case | Vectorize: reached by a real FrameSet and libm is half its time. |
| `pal::ampqk` and `pal::mapqkz` | `SlaMap` AMP/MAP only; no FrameSet | 1 | 27% body, 67% libm of `slamap_amp.map` | 67% of the case | Vectorize, class route only: no FrameSet reaches it, but `SlaMap` apparent-place conversions are per point in pipelines; the user decides whether the class counts. |
| `pal::dcs2c`, `pal::dmxv`, `pal::dcc2s`, `pal::dranrm` as `SlaMap`'s rotation step (PREC, EQGAL, GALEQ, GALSUP, ...) | `SlaMap` conversions; `timj.ast` does not use them | 1 each | about 100% of `slamap_prec.map`; inlined into `SlaMap`'s step loop | 83% of the case | Vectorize, class route only: `dcs2c_slice` and `dcc2s_slice` already exist for `SphMap`, so the work is a `SlaMap` step kernel on slices, not new math. No FrameSet reaches it. |
| `pal::dcs2c_slice`, `pal::dcc2s_slice` | `SphMap`; all six FrameSets | 1 each | 5% to 39% by input | already vectorized | Already done; nothing to propose. |
| `pal::dmat`, `pal::dmat_det` | `PolyMap` and `SplineMap` inverse iterations; `lsst.ast`, `lsst2.ast` | 3.1 and 4.1 inverse | 18% of `lsst.ast`, 1.7% of `lsst2.ast` | 0 (arithmetic only) | Scalar: no libm; the cost is a per-point allocation and a small solve inside `PolyMap`, which a `PolyMap`-level change would address, not a vector `dmat`. |
| `pal::drange` | `WcsMap` forward preparation; five FrameSets | 2 inverse | not visible as self time | not visible (a `%`/`fmod` at most) | Scalar: below the profile's resolution. |
| `pal::epb`, `pal::epb2d` | `TimeMap` MJD to Besselian epoch | 1 | 0: the case is `TimeMap`'s own loop, 2.7 ns per point | 0 | Scalar: no libm and already at memory-bound speed. |
| `pal::prec` | `SlaMap` PREC set-up | once per transform | 0 | 0 | Scalar: set-up, not per point. |
| `pal::dat`, `erfa::dtdb`, `erfa::nut00a`, `pal::nut`, `pal::evp`, `erfa::epv00`, `pal::mappa` | `TimeMap` LAST/LMST, three-input topocentric `SpecMap`, `SkyFrame` set-up | nutation per point for LAST/LMST and topocentric `SpecMap`; the rest once | not reached by any realistic input | not measured | Scalar: the user decided TimeMap LAST/LMST and topocentric `SpecMap` stay scalar; set-up runs once. |
| The remaining ERFA and PAL functions (calendar, `tdb_conv`, matrices, `galeq` family, `addet`/`subet`, `fk5hz`/`hfk5z`) | `SlaMap`, `TimeMap`, `SpecMap` conversions | 0 on every input here | 0 | not measured | Scalar: no realistic input reaches them per point. |

## Honest summary

- Of the six FrameSets, only `timj.ast` reaches PAL/ERFA trigonometry per point (`fk45z` forward, `fk54z` inverse), and it is the only FrameSet where libm inside PAL/ERFA is a large share of time (52%).
- The other five spend PAL time in the `SphMap` slice kernels (already vectorized), `drange` and, for `lsst.ast`, `dmat` (18%, no libm).
  Their libm time is `WcsMap`'s projection, which is outside this stage.
- The `SlaMap` cases are libm-bound (67% to 83%) and are where a vector kernel would pay, but they are reached only by the Mapping class, not by any of the six FrameSets.
- A decision for the user: vectorize `fk54z`/`fk45z` only (the one FrameSet-backed case), or also the `SlaMap` AMP/MAP and rotation-step kernels on the class argument.

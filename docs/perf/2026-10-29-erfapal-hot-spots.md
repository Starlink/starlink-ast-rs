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
`Drange` comes from `WcsMap`'s `forward_prep`, which runs when the FrameSet is transformed in its inverse direction (sky to pixel), so it appears only in the inverse column.

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

At large N the Rust engine is within noise of C on the three `SlaMap` cases and no slower than C on any FrameSet.
`timemap_mjdtobep.map` is the exception: at N = 100 000 Rust takes 2.7 ns per point against C's 1.6 forward and 2.4 against 1.5 inverse, about 1.6 to 1.7 times slower.
That case is `TimeMap`'s own loop plus `epb`, at 2 to 3 ns per point, and the profile shows no libm in it; this stage did not change it, and the cause was not investigated.

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
| `pal::ampqk` and `pal::mapqkz` | `SlaMap` AMP/MAP only; no FrameSet | 1 | 27% body, 67% libm of `slamap_amp.map` | 67% of the case | Vectorize, class route only: no FrameSet reaches it, so the case rests on the argument that `SlaMap` AMP/MAP conversions are per-point work; that is for the user to weigh, not something measured here. |
| `pal::dcs2c`, `pal::dmxv`, `pal::dcc2s`, `pal::dranrm` as `SlaMap`'s rotation step (PREC, EQGAL, GALEQ, GALSUP, ...) | `SlaMap` conversions; `timj.ast` does not use them | 1 each | `slamap_prec.map`: bodies 1%, the rest inlined into `SlaMap`'s step loop (libm 83%, other 16%) | 83% of the case | Vectorize, class route only: `dcs2c_slice` and `dcc2s_slice` already exist for `SphMap`, so the work is a `SlaMap` step kernel on slices, not new math. No FrameSet reaches it. |
| `pal::dcs2c_slice`, `pal::dcc2s_slice` | `SphMap`; all six FrameSets | 1 each | 8.5% to 39% by input | already vectorized | Already done; nothing to propose. |
| `pal::dmat`, `pal::dmat_det` | `PolyMap` and `SplineMap` inverse iterations; `lsst.ast`, `lsst2.ast` | 3.1 and 4.1 inverse | 18% of `lsst.ast`, 1.7% of `lsst2.ast` | 0 (arithmetic only) | Scalar: no libm; the profile shows `dmat` at 18% of `lsst.ast` and nothing more, and a remedy would be at `PolyMap` level, not a vector `dmat`. |
| `pal::drange` | `WcsMap` forward preparation; five FrameSets | 2 inverse | not visible as self time | not visible (a `%`/`fmod` at most) | Scalar: below the profile's resolution. |
| `pal::epb`, `pal::epb2d` | `TimeMap` MJD to Besselian epoch | 1 | 0: the case is `TimeMap`'s own loop, 2.7 ns per point | 0 | Scalar: no libm and already at memory-bound speed. |
| `pal::prec` | `SlaMap` PREC set-up | once per transform | 0 | 0 | Scalar: set-up, not per point. |
| `pal::dat`, `erfa::dtdb`, `erfa::nut00a`, `pal::nut`, `pal::evp`, `erfa::epv00`, `pal::mappa` | `TimeMap` LAST/LMST and three-input topocentric `SpecMap` (nutation per point); `SkyFrame` set-up (once) | per point on those two paths, once for set-up | none of the six FrameSets reaches the per-point paths | not profiled: the user ruled these scalar on 2026-10-29 | Scalar: the user's ruling; set-up runs once. |
| The remaining ERFA and PAL functions (calendar, `tdb_conv`, matrices, `galeq` family, `addet`/`subet`, `fk5hz`/`hfk5z`) | `SlaMap`, `TimeMap`, `SpecMap` conversions | 0 on every input here | 0 | not measured | Scalar: no realistic input reaches them per point. |

## Honest summary

- Of the six FrameSets, only `timj.ast` reaches PAL/ERFA trigonometry per point (`fk45z` forward, `fk54z` inverse), and it is the only FrameSet where libm inside PAL/ERFA is a large share of time (52%).
- The other five spend PAL time in the `SphMap` slice kernels (already vectorized), `drange` and, for `lsst.ast`, `dmat` (18%, no libm).
  Their libm time is `WcsMap`'s projection, which is outside this stage.
- The `SlaMap` cases are libm-bound (67% to 83%) and are where a vector kernel would pay, but they are reached only by the Mapping class, not by any of the six FrameSets.
- A decision for the user: vectorize `fk54z`/`fk45z` only (the one FrameSet-backed case), or also the `SlaMap` AMP/MAP and rotation-step kernels on the class argument.

## Outcome: the kernels, and the user's decision to keep them

The user chose `fk45z` and `fk54z` from the table above.
Each was given a vectorized slice entry in the SphMap pattern and measured against the rule this stage set for keeping a kernel: faster on `slamap_fk45z.map` (and on `timj.ast`) at N = 100 and at large N, and not slower at N = 1.
Neither kernel meets the N = 1 condition, and both were first reverted and recorded as staying scalar.
The user then decided to keep them (2026-10-29): FK4/FK5 conversions are not used by large-scale surveys, which use ICRS, so the single-point slowdown does not matter; the design's anti-goal governs, that N = 1 and N = 100 must stay faster than C, which both kernels do at three to four times C's speed, not this plan's stricter "not slower at N = 1" rule.
The per-value ports, bit for bit ERFA's, remain for one-off callers and for the positions outside a kernel's domain.

Timings are median ns per point, the Rust engine, base `53db8ff8` against the tree with the kernels, from three alternating rounds of the two binaries (the fastest round's median), load average 6.4 falling to 4.8, macOS aarch64.
Two runs of `ast-bench compare --set native --base 53db8ff8` (load average about 5) reported the same N = 1 slowdowns, `slamap_fk45z.map` forward 85 to 162 and 80 to 154, inverse 158 to 192.
Both also reported `slamap_prec.map`, which the change does not reach, 10 to 22% slower at N = 100 in both runs, which reads as a code-layout effect of the larger `SlaMap::transform_into`.

### `fk45z` (FK45Z forward, FK54Z inverse): kept by the user's decision

The kernel runs eraFk45z's body with eraS2c's two sines and cosines and eraC2s's two arctangents from `vmath`, the epoch terms (the adjusted A vector and the fictitious-motion interval) computed once per slice, which gives the same bits as per point, and eraAnp's `fmod` dropped, since an arctangent is already within 2 pi and `fmod` returns it exactly.
Positions outside the kernel's domain (an input angle of zero or beyond 1e6, or a final vector with a zero or extreme component) go to the per-value body.
`SlaMap` gathers the good points of each block of 32 (4 for transforms of at most 4 points) into the slice.

Every sine, cosine and arctangent it uses is within 1 ULP of the exact value, rounded, at every call over the FK test positions and epochs (3,016 random and polar positions at 12 epochs); against the per-value body the largest difference was 8 * 2^-53 radians in R.A. times cos Dec and 3 * 2^-53 in Dec (2 and 4 ULP of the outputs), against a derived bound of 40 * 2^-53.

| Case | N = 1 | N = 10 | N = 100 | N = 100 000 | N = 1 000 000 |
|---|---|---|---|---|---|
| `slamap_fk45z.map` forward | 96.0 → 163.0 (1.70) | 62.2 → 56.8 (0.91) | 58.9 → 47.8 (0.81) | 59.3 → 46.6 (0.79) | 59.3 → 46.6 (0.79) |
| `timj.ast` forward | 2638 → 2613 (0.99) | 357 → 342 (0.96) | 132 → 121 (0.92) | 105.5 → 95.2 (0.90) | 105.6 → 95.7 (0.91) |

The bench case's one-point row is (0, 0), which the sine kernel does not cover, so that point pays for the kernel and then for the per-value body.
A covered point is slower too: one point through the slice took 95 to 98 ns against 67 ns for the per-value body, measured as a dependent chain, since each `vmath` sine-cosine pair and arctangent has a latency of about 36 ns against libm's 29 ns, and two of each lie on the path.
No change inside this function can remove that; a shorter-latency `vmath::sin_cos` and `vmath::atan2`, the remedy the native design names for SphMap's N = 1 cost, could.
C takes 605 ns per point at N = 1 on this case, so the kernel stays about four times faster than C.

With this kernel alone, 146 of the 1,584 cases of the C-oracle `test_sky_convert` moved off C's bits, every one converting through FK4, in the last one or two digits of a value; each is named in `FK_SLICE_DRIFT_LICENSED` with its measured drift.

### `fk54z` (FK54Z forward, FK45Z inverse): kept by the user's decision

The kernel runs eraFk54z's body over a kernel of eraFk524 for a star with zero proper motion, parallax and radial velocity: eraS2pv's two sines and cosines and eraPv2s's two arctangents from `vmath`, eraPv2s's null-vector and polar-axis branches left to the domain test, and the parallax update, which a zero parallax never makes, left out; then eraS2c's sines and cosines of the FK4 position, which eraFk54z takes a second time for the fictitious proper motion and the kernel takes once, and eraC2s's two arctangents, from `vmath`; eraAnp's `fmod` dropped as for `fk45z`.
`SlaMap` uses it as for `fk45z`.

Every sine, cosine and arctangent it uses is within 1 ULP of the exact value, rounded, at every call over the same test positions and epochs; against the per-value body the largest difference was 16 * 2^-53 radians in R.A. times cos Dec and 4 * 2^-53 in Dec (3 and 4 ULP of the outputs), against a derived bound of 80 * 2^-53.

| Case | N = 1 | N = 10 | N = 100 | N = 100 000 | N = 1 000 000 |
|---|---|---|---|---|---|
| `slamap_fk45z.map` inverse | 166.5 → 203.3 (1.22) | 139.8 → 101.9 (0.73) | 138.4 → 97.2 (0.70) | 138.5 → 96.9 (0.70) | 138.3 → 96.9 (0.70) |
| `timj.ast` inverse | 2690 → 2714 (1.01) | 452 → 404 (0.89) | 225 → 181 (0.80) | 198.9 → 154.2 (0.78) | 198.9 → 154.0 (0.77) |

The one-point row here, (0.0112, 0.0049), is covered, so the slowdown is the latency of the kernels themselves: four sine-cosine pairs and four arctangents, two of each on the path, each about 8 ns slower than libm's.
C takes 666 ns per point at N = 1 on this case, so the kernel stays about three times faster than C.

With both kernels in place, `ast-bench ulps` against `53db8ff8` found no value changing between bad and good.
The largest movements were `timj.ast` inverse (sky to pixel), 2,952 ULP on the first pixel axis and 478 on the second, on pixel values between 1 and 1,000, so at most about 3e-10 pixel; the simplify corpus's SlaMap cases up to 28 ULP (`cap_slamap_05`); and 1 ULP on `slamap_fk45z.map` and `timj.ast` forward.
306 of the 1,584 `test_sky_convert` cases moved off C's bits, by at most 1.33e-15 radian, each named in `FK_SLICE_DRIFT_LICENSED` with its drift (two of the 146 the FK45Z kernel had moved match C again), and no other test of the crate changed.

# Native PAL and ERFA stage: close-out report (2026-10-29)

Branch `tickets/DM-56839-erfapal`, stacked on `tickets/DM-56839-wcsmap`, ticket DM-56839, one pull request.
Plan: `docs/superpowers/plans/2026-10-29-native-erfa-pal.md`; design: "PAL and ERFA stage" in `docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md`.

## What the stage did

1. Ported every PAL routine the crate calls, and the ERFA functions under them, to Rust (`crates/starlink-ast/src/pal/`, `src/erfa/`), bit for bit the vendored C's results.
   Each function has a differential test against the still-linked C, and `crates/starlink-ast/tests/erfa_pal_golden/roots.tsv` records every root's results so the check survives the C's deletion.
2. Deleted `vendor/pal`, `vendor/erfa`, `palwrap.c` and the crate's C build (commit `168a4da0`).
   `starlink-ast` now compiles no C and needs no C compiler.
3. Measured where PAL and ERFA time goes per point (`docs/perf/2026-10-29-erfapal-hot-spots.md`), and vectorized only `fk45z` (forward) and `fk54z` (inverse) for SlaMap's FK4/FK5 steps (`e5607c45`, `7858f26e`).

## Records

| Record | Commit | Notes |
|---|---|---|
| `docs/perf/2026-10-29-macos-aarch64-native-erfapal-baseline.tsv` | `2b9edd18` (PBASE) | PAL and ERFA still C; load 5 to 6.3 |
| `docs/perf/2026-10-29-macos-aarch64-native-erfapal-ported.tsv` | `168a4da0` | all Rust, C deleted, no kernels; load 7.1 falling to 4.8 |
| `docs/perf/2026-10-29-macos-aarch64-native-erfapal-final.tsv` | `7858f26e` | end of stage; load 3.5 |
| `docs/perf/2026-10-29-macos-aarch64-native-erfapal-roman.txt` | `7858f26e` | `ast-bench roman`; load 3.4 |

The snapshot has no N = 10 000 row (its points are 1, 10, 100, 100 000 and 1 000 000), so N = 100 000 stands in for the large-N column, as in the hot-spots record.
Times are median ns per point.
The port and end columns come from separate runs on a machine that was not guaranteed quiet; the C columns show the run-to-run noise (C is the same code in all three, and its geometric mean moved 1.00 between baseline and end).

## The six FrameSets and four PAL cases

Rust and C, at PBASE, after the port, and at the end.
The last column is the Rust engine's end time over its PBASE time.

| Case | Dir | N | Rust PBASE | Rust port | Rust end | C PBASE | C port | C end | Rust end / PBASE |
|---|---|---|---|---|---|---|---|---|---|
| `slamap_amp.map` | forward | 1 | 239.6 | 240.5 | 235.0 | 731.0 | 753.1 | 750.0 | 0.98 |
| `slamap_amp.map` | forward | 100 | 196.3 | 198.5 | 197.7 | 199.6 | 202.7 | 203.2 | 1.01 |
| `slamap_amp.map` | forward | 100000 | 196.6 | 195.9 | 198.1 | 194.9 | 195.5 | 198.3 | 1.01 |
| `slamap_amp.map` | inverse | 1 | 111.9 | 104.7 | 107.3 | 606.0 | 609.9 | 607.0 | 0.96 |
| `slamap_amp.map` | inverse | 100 | 66.5 | 67.1 | 65.5 | 66.9 | 68.7 | 66.8 | 0.99 |
| `slamap_amp.map` | inverse | 100000 | 66.5 | 67.4 | 65.6 | 61.4 | 63.8 | 62.0 | 0.99 |
| `slamap_fk45z.map` | forward | 1 | 93.9 | 97.5 | 163.7 | 592.4 | 605.0 | 593.9 | 1.74 |
| `slamap_fk45z.map` | forward | 100 | 59.5 | 60.5 | 48.0 | 61.3 | 62.4 | 62.3 | 0.81 |
| `slamap_fk45z.map` | forward | 100000 | 59.5 | 60.0 | 46.7 | 56.2 | 57.0 | 56.3 | 0.78 |
| `slamap_fk45z.map` | inverse | 1 | 163.4 | 169.2 | 203.8 | 657.2 | 666.3 | 659.7 | 1.25 |
| `slamap_fk45z.map` | inverse | 100 | 133.0 | 142.5 | 97.5 | 134.9 | 138.1 | 135.2 | 0.73 |
| `slamap_fk45z.map` | inverse | 100000 | 135.4 | 140.8 | 96.9 | 132.4 | 132.2 | 129.8 | 0.72 |
| `slamap_prec.map` | forward | 1 | 133.6 | 131.9 | 131.6 | 618.0 | 630.3 | 620.3 | 0.99 |
| `slamap_prec.map` | forward | 100 | 53.5 | 52.3 | 52.7 | 56.2 | 58.1 | 58.3 | 0.98 |
| `slamap_prec.map` | forward | 100000 | 53.1 | 51.3 | 50.4 | 51.0 | 52.0 | 51.0 | 0.95 |
| `slamap_prec.map` | inverse | 1 | 131.2 | 125.3 | 123.9 | 624.5 | 644.7 | 626.2 | 0.94 |
| `slamap_prec.map` | inverse | 100 | 52.2 | 50.5 | 50.0 | 56.4 | 58.1 | 56.5 | 0.96 |
| `slamap_prec.map` | inverse | 100000 | 51.6 | 49.5 | 49.8 | 51.3 | 52.0 | 51.4 | 0.96 |
| `timemap_mjdtobep.map` | forward | 1 | 14.9 | 14.1 | 13.7 | 537.9 | 554.0 | 537.7 | 0.92 |
| `timemap_mjdtobep.map` | forward | 100 | 2.89 | 2.88 | 2.77 | 6.89 | 7.16 | 6.92 | 0.96 |
| `timemap_mjdtobep.map` | forward | 100000 | 2.73 | 2.72 | 2.59 | 1.54 | 1.62 | 1.54 | 0.95 |
| `timemap_mjdtobep.map` | inverse | 1 | 14.5 | 13.9 | 13.8 | 538.5 | 555.0 | 550.8 | 0.95 |
| `timemap_mjdtobep.map` | inverse | 100 | 2.80 | 2.62 | 2.55 | 6.83 | 7.01 | 6.84 | 0.91 |
| `timemap_mjdtobep.map` | inverse | 100000 | 2.64 | 2.44 | 2.39 | 1.46 | 1.49 | 1.46 | 0.91 |
| `dss.ast` | forward | 1 | 2067.6 | 2069.1 | 2023.3 | 8276.0 | 8220.7 | 8254.2 | 0.98 |
| `dss.ast` | forward | 100 | 114.7 | 114.6 | 114.7 | 174.8 | 175.9 | 175.1 | 1.00 |
| `dss.ast` | forward | 100000 | 93.7 | 91.9 | 96.1 | 93.3 | 91.4 | 95.6 | 1.03 |
| `dss.ast` | inverse | 1 | 1808.2 | 1820.5 | 1798.8 | 8030.9 | 8048.2 | 7991.5 | 0.99 |
| `dss.ast` | inverse | 100 | 224.8 | 223.3 | 225.3 | 308.6 | 308.9 | 312.0 | 1.00 |
| `dss.ast` | inverse | 100000 | 207.0 | 205.5 | 204.5 | 229.5 | 231.3 | 230.7 | 0.99 |
| `lsst.ast` | forward | 1 | 7784.2 | 8119.1 | 7705.4 | 19053.4 | 19424.5 | 19039.1 | 0.99 |
| `lsst.ast` | forward | 100 | 165.2 | 174.8 | 164.9 | 399.4 | 418.7 | 392.2 | 1.00 |
| `lsst.ast` | forward | 100000 | 83.3 | 86.8 | 83.0 | 212.5 | 218.8 | 213.4 | 1.00 |
| `lsst.ast` | inverse | 1 | 15066.1 | 15592.5 | 15069.7 | 37951.8 | 38951.8 | 37457.0 | 1.00 |
| `lsst.ast` | inverse | 100 | 535.1 | 544.4 | 531.2 | 1257.4 | 1303.6 | 1269.4 | 0.99 |
| `lsst.ast` | inverse | 100000 | 355.7 | 362.2 | 349.4 | 882.2 | 906.9 | 898.5 | 0.98 |
| `lsst2.ast` | forward | 1 | 8510.1 | 8852.5 | 8491.2 | 20886.7 | 21953.8 | 21419.9 | 1.00 |
| `lsst2.ast` | forward | 100 | 351.5 | 358.2 | 341.6 | 626.7 | 649.2 | 629.6 | 0.97 |
| `lsst2.ast` | forward | 100000 | 260.1 | 259.2 | 252.0 | 418.7 | 425.0 | 422.1 | 0.97 |
| `lsst2.ast` | inverse | 1 | 16339.2 | 16772.8 | 16212.9 | 40841.2 | 41158.9 | 40166.7 | 0.99 |
| `lsst2.ast` | inverse | 100 | 1343.9 | 1346.7 | 1316.8 | 2162.1 | 2212.5 | 2158.4 | 0.98 |
| `lsst2.ast` | inverse | 100000 | 1159.8 | 1140.5 | 1130.4 | 1760.3 | 1770.8 | 1780.8 | 0.97 |
| `timj.ast` | forward | 1 | 2624.8 | 2640.9 | 2577.1 | 12368.8 | 12390.3 | 12299.2 | 0.98 |
| `timj.ast` | forward | 100 | 136.1 | 132.1 | 123.4 | 236.0 | 233.6 | 238.4 | 0.91 |
| `timj.ast` | forward | 100000 | 112.0 | 106.3 | 96.5 | 114.2 | 112.8 | 115.9 | 0.86 |
| `timj.ast` | inverse | 1 | 2764.5 | 2740.2 | 2731.2 | 12420.6 | 12349.3 | 12443.0 | 0.99 |
| `timj.ast` | inverse | 100 | 223.6 | 226.2 | 180.4 | 373.3 | 372.6 | 371.8 | 0.81 |
| `timj.ast` | inverse | 100000 | 196.7 | 198.8 | 157.4 | 252.4 | 252.5 | 257.3 | 0.80 |
| `degen1.ast` | forward | 1 | 3862.8 | 3871.3 | 3826.1 | 17400.4 | 17425.8 | 17415.4 | 0.99 |
| `degen1.ast` | forward | 100 | 96.0 | 96.1 | 95.9 | 230.5 | 231.2 | 231.1 | 1.00 |
| `degen1.ast` | forward | 100000 | 56.0 | 57.0 | 57.2 | 58.2 | 59.6 | 59.7 | 1.02 |
| `degen1.ast` | inverse | 1 | 4024.9 | 4046.4 | 3980.0 | 17283.8 | 17322.3 | 17300.1 | 0.99 |
| `degen1.ast` | inverse | 100 | 104.4 | 102.8 | 102.0 | 291.8 | 292.3 | 292.1 | 0.98 |
| `degen1.ast` | inverse | 100000 | 62.3 | 60.7 | 60.3 | 121.4 | 121.8 | 123.7 | 0.97 |
| `a20070718_00010_02_cube.ast` | forward | 1 | 3422.8 | 3467.0 | 3413.2 | 15745.5 | 15825.5 | 15696.6 | 1.00 |
| `a20070718_00010_02_cube.ast` | forward | 100 | 92.4 | 92.1 | 91.9 | 216.9 | 215.7 | 216.3 | 1.00 |
| `a20070718_00010_02_cube.ast` | forward | 100000 | 57.0 | 58.6 | 57.0 | 59.5 | 59.7 | 59.5 | 1.00 |
| `a20070718_00010_02_cube.ast` | inverse | 1 | 3502.7 | 3435.7 | 3524.1 | 15767.6 | 15224.6 | 16005.2 | 1.01 |
| `a20070718_00010_02_cube.ast` | inverse | 100 | 99.1 | 96.9 | 96.6 | 278.5 | 277.1 | 275.9 | 0.97 |
| `a20070718_00010_02_cube.ast` | inverse | 100000 | 63.8 | 62.0 | 62.9 | 121.9 | 121.1 | 122.4 | 0.99 |

Reading the table:

- The port alone moved nothing beyond noise: Rust-engine geometric mean over the 240 rows is 1.000 of the baseline (C engine 1.016).
  The largest port-only movement is the FK45Z inverse at large N, 135.4 to 140.8 (1.04; C 132.4 to 132.2), and the six FrameSets are within 1.06.
- A TimeMap TAI to UTC slowdown (+22% to +41%, 6.6 to 8.1 ns per point) appeared in `68e71934`, where the calendar and leap-second functions were ported and TimeMap's own table lookup changed.
  The ported record shows it; `cc7bc1bc` fixed it with no change in any bit, and the end-of-stage record no longer flags it.
- At the end, Rust's geometric mean over the 240 rows is 0.96 of the baseline (C 1.00); the movement is the FK rows.
- `timemap_mjdtobep.map` at large N is slower than C throughout (2.6 against 1.5 ns per point forward) and was before this stage; it is TimeMap's own loop plus `epb`, with no libm in it, and was not investigated.

## Hot spots (Task 13) and decisions

`docs/perf/2026-10-29-erfapal-hot-spots.md` counts calls per point and profiles each case.
Only `timj.ast` reaches PAL trigonometry per point (`fk45z` forward, `fk54z` inverse: about 64% of its time, libm about 50% of it); `slamap_fk45z.map` is about 95% in those two.
The other five FrameSets spend their PAL time in the SphMap slice kernels (already vectorized), `drange` and, for `lsst.ast`, `dmat` (18%, no libm, PolyMap-level cost).

User decisions, 2026-10-29:

- Task 14 covers `fk45z` and `fk54z` only.
- The plan's rule required a kernel to be faster at N = 100 and large N and not slower at N = 1.
  The `fk45z` and `fk54z` kernels failed it at N = 1, so they were first recorded as staying scalar.
  The user then kept them: FK4/FK5 is not used by large surveys (they use ICRS), and the spec's anti-goal, that N = 1 and N = 100 stay faster than C, governs; both kernels stay 3 to 4 times faster than C at N = 1.
- SlaMap AMP/MAP and rotation-step kernels: not done (user's choice).
- TimeMap LAST/LMST and topocentric SpecMap nutation: scalar by decision; no realistic input reaches them per point.
- A linear-algebra crate for the hand-written 3-vector code: deferred (`PLAN.md` §7 row).
- `dmat`'s cost is PolyMap's, not PAL's, so it is not a kernel candidate.

## Kernels, kept and rejected

| Function | Direction | Verdict | N = 1 | N = 100 | Large N | FrameSet |
|---|---|---|---|---|---|---|
| `fk45z` | FK45Z forward | kept (user) | +70% (96 to 163 ns) | 0.81 | 0.79 | `timj.ast` forward 0.86 to 0.90 |
| `fk54z` | FK54Z inverse | kept (user) | +22% (163 to 204 ns) | 0.73 | 0.70 to 0.72 | `timj.ast` inverse 0.78 to 0.80 |
| SlaMap AMP/MAP (`ampqk`, `mapqkz`) | both | not done | | | | none reaches it |
| SlaMap rotation steps | both | not done | | | | none reaches it |
| TimeMap LAST/LMST, SpecMap nutation | | scalar by decision | | | | |

Each kernel is the ERFA body with the sines, cosines and arctangents from `vmath`, the epoch terms once per slice, and positions outside its domain sent to the per-value body.
Largest differences from the per-value body are 8 and 16 units of 2^-53 radian in R.A. times cos Dec and 3 and 4 in Dec; each function's trigonometry is within 1 ULP of exact.
306 `test_sky_convert` cases carry per-case licences (`FK_SLICE_DRIFT_LICENSED`, at most 1.33e-15 rad), counted in `PLAN.md` §6, and every licence is checked to name an existing case and to still be needed.
In the hot-spots record, the other SlaMap cases (AMP, EQGAL, PREC) were flagged by `compare` at N = 1 and N = 100 under load; direct alternation gave at most +8% (PREC forward N = 100), which reads as code layout or load.

## Gates

- `ast-bench roman` (`7858f26e`, load 3.4): SphMap forward 60.0 and inverse 76.4 million points per second at 1 048 576 points, 3.3 and 1.5 times C; degree-5 and degree-1 PolyMaps 5.1 and 5.2 times C; outputs differ from C by at most 1 ULP (SphMap forward), 3 ULP (inverse), and are identical for PolyMap and MatrixMap.
- `ast-bench compare` across the Task 14 change exits non-zero on the accepted `slamap_fk45z.map` N = 1 rows.
  The controller ruled not to add the case to `SLOWDOWN_LICENSED`, since a licence for the case would mask a regression at every N.
- Full gates: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test -p starlink-ast` (4,528 tests) and `cargo doc -p starlink-ast --no-deps` pass.
- Tier 4 (`cargo test -p ast-oracle-tests --no-fail-fast`): everything passes except `call_count_parity`, whose 28 differing sections all differed on `astPalDeuler` alone (Rust 1, C 0).
  This stage caused it: the Rust `pal::prebn` and `pal::ecmat` called the counting `pal::deuler` wrapper, where C's counting build counts no call PAL makes internally.
  They now call the uncounted body and `call_count_parity` passes (`PLAN.md` §6 row "`call_count_parity` fails on `astPalDeuler`").

## Open items

- x86-64 record for this stage (commands below).
- `timemap_mjdtobep.map` large-N gap to C.
- Shorter-latency `vmath::sin_cos` and `vmath::atan2` would remove the FK kernels' N = 1 cost.
- The vendored-C rewrite ticket is superseded for PAL and ERFA by this port.
- Interim reassessment: the spec keeps per-value entries on ERFA's exact arithmetic while the C is the oracle.
  Whether to move them onto kernels now that the C is gone is a separate decision, to be made after this pull request.

## x86-64 commands

```shell
git checkout tickets/DM-56839-erfapal
cargo run --release -p ast-bench -- compare --set native --base tickets/DM-56839-wcsmap
cargo run --release -p ast-bench --features c-oracle -- snapshot --set native
cargo run --release -p ast-bench --features c-oracle -- roman -o roman.csv
cargo test -p starlink-ast
env -u CC -u CXX -u CFLAGS -u CPPFLAGS -u LDFLAGS cargo test -p starlink-ast --lib roots_match -- --ignored
```

The last command prints the per-root ULP summary, which records how far glibc moves the ports from the captured C values.

`compare` is expected to report `slamap_fk45z.map` forward and inverse N = 1 slower and `slamap_fk45z.map` and `timj.ast` faster at N >= 100.
By the user's decision, `ast-bench compare --base tickets/DM-56839-wcsmap` (or `--base main`) is expected to flag `slamap_fk45z.map` at N = 1.

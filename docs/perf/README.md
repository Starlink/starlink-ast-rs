# Transform performance records

Timings of the port against the reference C library, recorded with `ast-bench` (`crates/ast-bench/README.md`) so that the effect of each round of optimization can be shown against where it started.

Each file is one run on one machine: a header of `#` lines names the commit, the date, the machine, the Rust compiler and the C build, and the body is `ast-bench run --format tsv` output with both engines.
Absolute times belong to that machine; the Rust/C ratios travel better, but libm, vector width and caches still differ between platforms, so a ratio from another machine is a guide, not a gate.

| File | Commit | What it records |
|---|---|---|
| `2026-10-15-macos-aarch64-pre-phase0.tsv` | `048e9317` | The `bench` set before the call-count parity fixes: the starting point. |
| `2026-10-15-macos-aarch64-post-phase0.tsv` | `74348ecd` | The `bench` set after them (WcsMap set-up, SlaMap, SpecMap, MathMap and PolyMap constants, Region caches). |
| `2026-10-16-macos-aarch64-bench.tsv` | `7f62db96` | After SpecMap's conversion loop was restructured to match C's (`f015997c`); the first record written by `ast-bench snapshot`. |
| `2026-10-16-macos-aarch64-sweep.tsv` | `54d21ee6` | The first full sweep: every section of the three core oracles and `bench.oracle` at 100 and 100 000 points. |
| `2026-10-16-macos-aarch64-sweep-intramap-retime.tsv` | `33db9f51` | The sweep's IntraMap sections again, after the harness's IntraMap transform stopped allocating per point. |
| `2026-10-16-macos-aarch64-sweep-untimed.txt` | `54d21ee6` | The 256 sweep sections that could not be timed, with the reason for each. |
| `2026-10-17-macos-aarch64-bench.tsv` | `35da6e94` | The `bench` set after transforms moved to per-axis `Points` views. |
| `2026-10-17-macos-aarch64-sweep.tsv` | `35da6e94` | The full sweep again after the same change, with a `rust-cols` engine; the same 256 sections are untimed. |
| `2026-10-18-macos-aarch64-roman.txt` | `be015c2d` | `ast-bench roman`, Starlink/ast pull request 51's Roman WCS benchmark, on both engines: SphMap at parity with C at large N, PolyMap 1.4x and the full 2x2 MatrixMap 3.5x faster, the diagonal 2x2 MatrixMap 0.54x C while its data fits in cache; outputs identical on all 200M values. |
| `2026-10-19-macos-aarch64-bench.tsv` | `69c40996` | The `bench` set after the SplineMap, LutMap and PolyMap block kernels and the default NaN path: Rust/C geometric mean 0.09 at N=1, 0.33 at N=100 and 0.84 at N=1 000 000; `lsst` 0.39 forward and 0.44 inverse. Load average 6.5 to 7.6. |
| `2026-10-19-macos-aarch64-framesets.tsv` | `69c40996` | The first record of the `framesets` set, at the same commit and load. |
| `2026-10-26-macos-aarch64-c-fixed-cost.txt` | `9b9c58b4` | `tools/perf/prof_tran` on six `bench` fixtures, thread-safe and without pthreads, with a sampled profile of the UnitMap call: a one-point `astTranN` costs about 550 ns thread-safe and 210 ns without, most of it the two PointSet objects it creates; the arithmetic is under 40 ns for a simple Mapping. |
| `2026-10-27-macos-aarch64-native.tsv` | `9dbd6c88` | The `native` set (`crates/ast-bench/sets/native.txt`): every bench transform that reaches the vendored ERFA, PAL or wcslib C, and the `wcsconv/inputs` FrameSets, before any of that C is replaced by Rust. The baseline for `docs/superpowers/specs/2026-10-27-native-erfa-pal-wcslib-design.md`. Load average 4.2 to 5.1. |
| `2026-10-27-macos-aarch64-native-roman.txt` | `9dbd6c88` | `ast-bench roman` at the same commit: SphMap at parity with C at large N, 18.6 million points per second forward and about 54 inverse on both engines, because both call the same ERFA C per point. |
| `2026-10-28-macos-aarch64-native-vendored.tsv` | `8a5ad681` | The `native` set again after the vendored wcslib, ERFA and PAL update in C AST (submodule `3d3f255f`), on both engines, with SphMap's bench fixture at 4096 points: the baseline every replacement of vendored C after this one is measured against. Rows other than SphMap are within 4% of the record made before the update (geometric means 0.99 to 1.01), so the update changed no timing. SphMap's per-point forward transform takes 42.1 ns per point in the port and 39.8 in C at N=1 000 000, where the 24-point fixture gave 13.5 and 12.9: the short cycle let the CPU learn libm's branches. Load average 6 to 9. |
| `2026-10-28-linux-x86_64-native-vendored.tsv` | `d62cdfea` | The same `native` set on Linux x86-64 (AMD EPYC 7713P, conda-forge gcc 14.4 for the C engine): the Linux counterpart of the record above. On this fixture SphMap's forward transform takes 39.7 ns per point in the port, which calls the vendored C per point, and 36.4 in C at N=1 000 000. Load not recorded. Its SphMap rows are from the 24-point fixture, before `8a5ad681`. |

The pre-Phase-0 run uses that commit's `starlink-ast` with the current `ast-bench` harness, built in a git worktree, since the harness did not exist yet; it uses no `starlink-ast` API newer than that commit.

Both runs alternated pre, post, pre, post on a quiet machine, and each row is the run with the lower median; the same build agreed with itself across the two rounds to within about 5% (5th to 95th percentile).
The post-Phase-0 record shows one regression the fixes introduced, SpecMap's forward transform at large N (2.9 to 4.2 ns per point), which the next record addresses: SpecMap's forward transform at large N drops to 2.5 ns per point, while its single-point call rises from 9.6 to 13.5 ns (C takes 537 ns).
The dates in the file names are UTC.

To add a record, run `ast-bench snapshot --set bench` (built with `--features c-oracle`), which writes `<date>-<os>-<arch>-bench.tsv` here, and add a row to the table.
`compare --engine both` warns when a ratio is worse than in the newest such record for its platform.

## The first full sweep

The sweep timed 2289 of 2545 sections in 28 minutes, on a machine shared with other work (load average 6 to 9), so its absolute times are noisier than the `bench` records'.
Rust/C geometric mean: 0.57 at N=100, 2.5 at N=100 000; 839 timings are over 3x, only 3 of them at N=100, so the gap is per point, not per call.
- Parallel CmpMaps of cheap Mappings (WinMap, ZoomMap, PermMap, NullRegion): 15 to 25x at N=100 000; the port copies each parallel slice where C transforms in place.
- Spectral FITS headers, inverse: about 11x (`vopt-wav`, `class-spec`, `freq-f2w`).
- IntraMap: the sweep's 65 to 128x was the harness's own test transform allocating per point; re-timed with that fixed it is about 13x at N=100 000 and 0.5x at N=100.
- Real-world FrameSets are at parity, 0.98 to 1.14 (LSST, TNX-Chebyshev, SIP, DSS, SAO TAN).
- Of the untimed sections, 232 have no row good on both engines (simplifier fixtures whose transforms are all BAD), and 24 are bare Frames or a refused CmpFrame slot, which the port does not treat as Mappings.

The `framesets` set (`crates/ast-bench/sets/framesets.txt`) was chosen from this sweep.

## After per-axis views

A `PointSet` now holds one array per axis, and each `transform_into` loops down whole axes of a borrowed `Points` view into a `PointsMut` view.
The `rust-cols` engine times the same transform called on caller-owned per-axis arrays, the shape a C caller of `astTranN` passes; it is within 10% of `rust` at large N, either side.
Every corpus transform's output bits are unchanged (`ast-bench bits`).

The `bench` set at N=1 000 000, forward, in ns per point:

| Case | Rust 2026-10-16 | Rust 2026-10-17 | `rust-cols` | C |
|---|---|---|---|---|
| `cmpmap_parallel` | 7.56 | 0.54 | 0.54 | 0.61 |
| `cmpmap_series` | 5.61 | 1.06 | 1.05 | 1.26 |
| `matrixmap_3x3_full` | 17.65 | 6.62 | 6.62 | 12.05 |
| `matrixmap_3x3_diag` | 3.73 | 0.85 | 0.85 | 0.85 |
| `winmap_2d` | 3.11 | 0.59 | 0.59 | 0.47 |
| `shiftmap_2d` | 2.65 | 0.52 | 0.52 | 0.48 |
| `permmap_2d_swap` | 2.32 | 0.52 | 0.55 | 0.35 |
| `tranmap_zoom_shift` | 1.64 | 0.52 | 0.52 | 0.68 |
| `normmap_2d` | 6.55 | 4.92 | 4.86 | 34.12 |
| `mathmap_2d_linear` | 12.02 | 8.03 | 7.91 | 4.10 |
| `wcsmap_sin` | 36.15 | 32.84 | 32.84 | 31.66 |
| `ratemap_polymap` | 2770 | 2373 | 2363 | 2627 |

Classes whose cost is the per-point math (WcsMap, SlaMap, SphMap, PolyMap, ChebyMap, GrismMap, PcdMap) moved by at most 11% and stay within 7% of C.
The worst remaining `bench` ratios at large N are SpecMap wavenumber to frequency (4.2 to 5.1x), MathMap (1.8 to 2.1x), SplineMap (1.6x), PermMap and UnitMap (1.5x, at under 0.6 ns per point) and LutMap (1.3x).
A single-point call costs about 6 ns more than before, for building the two views; C's single-point calls take 540 ns or more on the same cases.

The sweep took 33 minutes at load average 5.6 to 7.4.
Rust's time per point fell to a geometric mean of 0.59 of the first sweep's at N=100 and 0.45 at N=100 000, while C's stayed within 3%.
Rust/C geometric mean is now 0.34 at N=100 and 1.17 at N=100 000; timings over 3x at N=100 000 fell from 842 to 123:
- IntraMap (20 to 33x): the harness's test transform, not the port's.
- SpecMap chains and spectral FITS headers (3.6 to 10x), the same cause as the `bench` SpecMap row.
- SwitchMap (up to 4x), and Region and PointList simplifier fixtures (3.1 to 4.5x).

### Branchless kernels

Three kernels were then rewritten so that the loop over points has no branch, timed with `ast-bench run` on the same machine, in ns per point (not in the recorded TSVs):

| Case | N | Before | After | C |
|---|---|---|---|---|
| `matrixmap_3x3_full` forward | 1 000 000 | 6.8 | 3.3 | 11.5 |
| `unitnormmap_2d` forward | 10 | 8.2 | 5.3 | |
| `polymap_2d_deg5` forward | 1 000 000 | 53 | 38 | 58 |

A branch that sets a flag stops a loop vectorizing, so MatrixMap builds its BAD flag with a select and unrolls each row's sum for one to four inputs, and UnitNormMap does the same for up to three coordinates.
PolyMap multiplies each term by the zeroth power of an input (exactly 1) where C skips it, which removes the branch from the term loop; ChebyMap shares that loop.
With these, the checked path is faster than the NaN protocol (`ast-bench run --nan`) for MatrixMap at every N, the one class where the NaN path had won.

# Transform performance records

Timings of the port against the reference C library, recorded with `ast-bench` (`crates/ast-bench/README.md`) so that the effect of each round of optimisation can be shown against where it started.

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

The pre-Phase-0 run uses that commit's `ast-core` with the current `ast-bench` harness, built in a git worktree, since the harness did not exist yet; it uses no `ast-core` API newer than that commit.

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

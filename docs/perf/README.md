# Transform performance records

Timings of the port against the reference C library, recorded with `ast-bench` (`crates/ast-bench/README.md`) so that the effect of each round of optimisation can be shown against where it started.

Each file is one run on one machine: a header of `#` lines names the commit, the date, the machine, the Rust compiler and the C build, and the body is `ast-bench run --format tsv` output with both engines.
Absolute times belong to that machine; the Rust/C ratios travel better, but libm, vector width and caches still differ between platforms, so a ratio from another machine is a guide, not a gate.

| File | Commit | What it records |
|---|---|---|
| `2026-10-15-macos-aarch64-pre-phase0.tsv` | `048e9317` | The `bench` set before the call-count parity fixes: the starting point. |
| `2026-10-15-macos-aarch64-post-phase0.tsv` | `74348ecd` | The `bench` set after them (WcsMap set-up, SlaMap, SpecMap, MathMap and PolyMap constants, Region caches). |
| `2026-10-16-macos-aarch64-bench.tsv` | `7f62db96` | After SpecMap's conversion loop was restructured to match C's (`f015997c`); the first record written by `ast-bench snapshot`. |

The pre-Phase-0 run uses that commit's `ast-core` with the current `ast-bench` harness, built in a git worktree, since the harness did not exist yet; it uses no `ast-core` API newer than that commit.

Both runs alternated pre, post, pre, post on a quiet machine, and each row is the run with the lower median; the same build agreed with itself across the two rounds to within about 5% (5th to 95th percentile).
The post-Phase-0 record shows one regression the fixes introduced, SpecMap's forward transform at large N (2.9 to 4.2 ns per point), which the next record addresses: SpecMap's forward transform at large N drops to 2.5 ns per point, while its single-point call rises from 9.6 to 13.5 ns (C takes 537 ns).
The dates in the file names are UTC.

To add a record, run `ast-bench snapshot --set bench` (built with `--features c-oracle`), which writes `<date>-<os>-<arch>-bench.tsv` here, and add a row to the table.
`compare --engine both` warns when a ratio is worse than in the newest such record for its platform.

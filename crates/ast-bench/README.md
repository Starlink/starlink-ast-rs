# ast-bench

Transform benchmarks for the AST port.

Performance work has two eras, and this crate serves both.
While the port is being brought to parity, it times Rust and the reference C library on the same Mappings and the same points.
Once Rust is known to be performant, C is left behind and the gate becomes "this change is not slower than its base", Rust against Rust.
The C engine is therefore optional (the `c-oracle` feature); everything else builds and runs without the `reference/starlink-ast` submodule.

The design is `docs/superpowers/specs/2026-10-15-transform-benchmarking-design.md`.

Whenever the Rust engine runs, it also times `rust-cols`: the same transform called the way a C caller of `astTranN` calls it, with one input array per axis and one output array per axis, and any conversion the port needs inside the timing.
`rust-cols` against `c` is what a C program would see on switching libraries; tables show it in the `cols ns/pt` column.
`run --nan` and `sweep --nan` also time `rust-nan`: the port through the NaN protocol (`transform_nan_protocol`) with its BAD↔NaN boundary passes inside the timing and its buffers allocated once, which is what an entry point using the protocol would cost; tables show it in the `nan ns/pt` column.
It is opt-in because `compare` reports a timing missing from the base as unmatched, and a baseline cannot be saved with it.

## Sets

A set is a file in `sets/` naming oracle sections, one per line with the reason it is there.
`bench` holds one plain Mapping of each class, then the real-world FrameSets and Mappings in the core fixtures (the LSST FrameSets, `brad`, `rigby` and others), so the performance gate covers the objects users transform.
`framesets` holds real-world FrameSets that must stay at parity with C (LSST, TNX, SIP, DSS, TAN) and the worst ratios the first full sweep found (spectral headers, parallel CmpMaps, IntraMaps).
Every case in a set must have a row that is good on both engines, since the small point counts time only such rows; a test checks this.

## Fairness rules

- Both engines at `-O3` (Rust: the release profile).
- C built with `-ffp-contract=off`, because Rust never fuses into FMA.
- No explicit SIMD; auto-vectorization is allowed on both sides.
- Every timed case is covered by an asserting oracle gate.

## Known bias

Runs of 100 000 and 1 000 000 points repeat the 24 oracle rows in order.
Real data is less regular, so branch prediction does better here than it would in an application.
The bias applies equally to every engine and every build, so comparisons stay fair; absolute times are slightly optimistic.

## Checking for slowdowns

    ast-bench run --set bench --save      # this machine's baseline
    ast-bench compare --set bench         # after a change

Baselines live under `target/perf/baselines/`, one TSV per set beside the fingerprint (CPU, OS, rustc, C build, profile) of the run that saved it; `compare` refuses a baseline from another fingerprint and says which field differs.
A timing in the baseline or the new run but not both is an error, as is a re-timing that fails.
A timing counts as slower when its fastest sample is more than 10% slower than the baseline's (`SLOWDOWN_THRESHOLD`); a suspect is timed twice more and reported only if every re-timing is slow too, which clears the noise that sub-nanosecond, memory-bound timings show between runs.

To compare with another commit instead of a saved baseline:

    ast-bench compare --set bench --base main

This builds `ast-bench` at that commit in a git worktree under `target/perf/base/<sha>`, times the set with that build and the current one in three alternating rounds, and compares the combined timings, so both builds see the same machine state.
The base must contain `ast-bench`.
A base older than the `rust-cols` timing has no `rust-cols` rows, which `compare --base` reports as unmatched, so choose a base that has them.
Either build failing a case, or timing a case the other does not, is an error.
The worktree is kept for the next comparison; `git worktree remove target/perf/base/<sha>` removes it.

A report names what got slower, not why.
Changing one Mapping's code can move another's hot loop in the binary: making ZoomMap slower on purpose also made PolyMap's forward transform 20% slower in the same build, although PolyMap does not use ZoomMap.
That slowdown is real for that binary, so `compare` is right to report it, but read a report that lists unrelated classes with this in mind.

## Checking that results did not change

    ast-bench bits [--base <git-ref>]

prints a hash of the output bits of every transform in the sweep corpus, through `transform` and through the NaN protocol, at 10 000 points per case.
It fails on any case where the two paths give different bits, since the NaN protocol must not change a result.
With `--base`, it builds `ast-bench` at that commit, runs the same command there, and fails on any case whose hashes differ, including a case that fails to load or transform on one side only.
Use it for changes that must not alter any result, such as a change of data layout.
The hashes depend on the platform's libm, so they are only ever compared on one machine, never committed.

## Records and the full sweep

    ast-bench snapshot --set bench        # needs --features c-oracle

times the set on both engines and writes `docs/perf/<date>-<os>-<arch>-<set>.tsv` with a header naming the commit, machine, compilers and C build.
The records are kept so that each round of optimization can be shown against the ones before it; `docs/perf/README.md` lists them.
`compare --set S --engine both` also prints a warning for each Rust/C ratio more than the slowdown threshold worse than in the newest record of `S` for this platform.
The warning never sets the exit status, since the ratio moves with the machine as well as the code.

    ast-bench sweep                       # needs --features c-oracle

times every section of the three core transform oracles and the bench oracle at 100 and 100 000 points on both engines, writes the rows to `target/perf/sweep-<date>.tsv`, and prints the 20 worst Rust/C ratios and the sections that could not be timed, with the reason for each.
It is meant to be run overnight.

## The Roman benchmark

    ast-bench roman [--engine rust|c|both] [--max-n N] [--reps R] [--markdown] [-o file.csv]

repeats `perf/bench_simd.c` from [Starlink/ast pull request 51](https://github.com/Starlink/ast/pull/51), which times the transforms that dominate the Roman Space Telescope's WCS: SphMap both ways, a degree-5 and a degree-1 PolyMap, and a diagonal and a full 2x2 MatrixMap.
The Mappings are that program's, written by `tools/perf/gen_roman_fixtures.c` into `fixtures/roman`; the inputs are its `drand48` values from seed 42, reproduced bit for bit; the point counts run from 1 to 4088x4088, the full Roman WFI science area.
The timing is also the pull request's rather than `measure`'s: each point count runs a batch of calls that doubles until it takes at least 20 ms, times that batch `R` times (default 5) and keeps the median time per call, so the numbers are comparable with the tables in the pull request.
Both engines get the same buffer, one axis after another, as `astTranP` callers hold them.
With both engines it prints the summary in millions of points per second, the Rust/C speed ratio, and how many output values differ between the engines at the largest point count, in units in the last place.
`-o` writes the pull request's CSV (`transform,n_points,rep,time_s`), with the engine appended to each transform's name.
The engine defaults to `both` with the `c-oracle` feature and `rust` without; the largest point count needs more than 1 GB of memory, and `--max-n` limits the sweep.

Regenerate the fixtures from the reference library:

    tools/perf/build_c_engine.sh bench
    target/perf/c/bench/gen_roman_fixtures crates/ast-bench/fixtures/roman

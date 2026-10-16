# ast-bench

Transform benchmarks for the AST port.

Performance work has two eras, and this crate serves both.
While the port is being brought to parity, it times Rust and the reference C library on the same Mappings and the same points.
Once Rust is known to be performant, C is left behind and the gate becomes "this change is not slower than its base", Rust against Rust.
The C engine is therefore optional (the `c-oracle` feature); everything else builds and runs without the `reference/starlink-ast` submodule.

The design is `docs/superpowers/specs/2026-10-15-transform-benchmarking-design.md`.

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

Baselines live under `target/perf/baselines/`, one directory per fingerprint (CPU, OS, rustc, C build, profile); `compare` refuses a baseline from another fingerprint and says which field differs.
A timing counts as slower when its fastest sample is more than 10% slower than the baseline's (`SLOWDOWN_THRESHOLD`); a suspect is timed twice more and reported only if every re-timing is slow too, which clears the noise that sub-nanosecond, memory-bound timings show between runs.

To compare with another commit instead of a saved baseline:

    ast-bench compare --set bench --base main

This builds `ast-bench` at that commit in a git worktree under `target/perf/base/<sha>`, times the set with that build and the current one in three alternating rounds, and compares the combined timings, so both builds see the same machine state.
The base must contain `ast-bench`.
The worktree is kept for the next comparison; `git worktree remove target/perf/base/<sha>` removes it.

A report names what got slower, not why.
Changing one Mapping's code can move another's hot loop in the binary: making ZoomMap slower on purpose also made PolyMap's forward transform 20% slower in the same build, although PolyMap does not use ZoomMap.
That slowdown is real for that binary, so `compare` is right to report it, but read a report that lists unrelated classes with this in mind.

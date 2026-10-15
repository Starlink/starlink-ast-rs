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

# Transform benchmarking infrastructure — design

**Date:** 2026-10-15
**Status:** draft for review
**Supersedes:** the hand-run sweep in `crates/ast-core/examples/bench_per_class.rs` and `crates/ast-core/tests/bench_per_class_c.c`, and the placeholder `ast-oracle-bench` crate.

## Purpose

The port needs repeatable measurement of transform performance that keeps working as the library evolves.
Performance work has two eras, and the infrastructure serves both:

1. **Parity.** Compare Rust against the C library on the same Mappings and the same points, to find and close the gaps.
2. **Regression.** Once Rust is known to be performant, the C library is left behind, and the gate becomes "this pull request is not slower than its base", Rust against Rust, with no C involved.

The C engine is therefore an optional plug-in to a Rust-only harness, not its core.

This spec covers the measuring tools only.
Optimisations are separate work, each judged with these tools (see "Follow-ups").

## Scope

- Transforms only: `astTran`-style forward and inverse evaluation.
- Objects are timed **as loaded**: a FrameSet or Mapping read from a fixture is transformed exactly as read, never simplified first.
  The simplifier has its own corpus; benchmarking must time the same structure on both sides, and applications that never call `astSimplify` are a real workload.
- Point counts N ∈ {1, 10, 100, 100 000, 1 000 000}.
  Small N measures per-call overhead; large N measures the per-point loop.
- No explicit SIMD on either side, so the comparison with C is direct.
  Compiler auto-vectorisation is allowed on both sides.

## Phase 0 — call-count parity

A validation step, run before any timing: for every transform-oracle case, Rust and C must make the same calls into PAL, ERFA, wcslib and libm trigonometry.
A sky transform limited by trigonometry cannot be compared fairly until the call counts are known to match.

### What is counted

Calls made **from AST code** into:

- PAL and ERFA functions;
- wcslib functions;
- libm `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `exp`, `log`, `pow`, `sqrt`.

Calls made *inside* the vendored PAL, ERFA and wcslib sources are not counted on either side.
Those libraries are the same code in both engines, so their internal calls add no information.

Counts are of calls written in the source, on both sides.
A compiler fusing `sin` and `cos` into `sincos` is a timing matter, visible in Phase 1, not a count difference.

### Rust side

- A `call-counts` cargo feature on `ast-core`.
- PAL and ERFA calls are counted at the single boundary in `crates/ast-core/src/pal.rs`, and wcslib calls in `crates/ast-core/src/wcslib.rs`: one counting wrapper per function.
- A new internal `math` module provides `sin`, `cos`, and the rest of the list above.
  Each is an `#[inline(always)]` pass-through, incrementing a thread-local counter only under `call-counts`.
- `clippy.toml` gains `disallowed-methods` entries for the corresponding `f64` methods, so new code in `ast-core` cannot bypass the counters.
  The existing ~190 call sites are converted once, mechanically.
- Without the feature the wrappers compile to the bare call, so normal builds pay nothing.

### C side

- `tools/perf/build_c_count_lib.sh` builds the submodule's libast into a scratch directory with a forced-include header.
- The header includes `<math.h>` and the PAL, ERFA and wcslib headers first, then defines a function-like macro for each counted function that calls a counting wrapper.
- The header is applied to AST's own `src/*.c` files only, never to the PAL, ERFA or wcslib sources, matching the Rust boundary.
- The library exports a reset and a read function for the counters.
- The pattern follows `tools/oracle_fixtures/build_simplify_trace.sh`: a permanent build variant, not a patch.

### The check

- A test in `ast-oracle-tests`, which is a C-comparison crate by nature, runs every section of `simplify_fixtures.oracle`, `headers.oracle` and `framesets.oracle` through both engines.
- For each section it checks that both outputs match the oracle, and that the per-function counts are equal.
- A case whose counts differ for a reason that is understood goes in a named exclusion list with a per-case reason string, plus a companion test that fails when an excluded case starts matching.
  There is no skip mode.
- A count mismatch that is not understood is a porting defect and is investigated as one.

## Phase 1 — the harness

### Crate

`ast-oracle-bench` is renamed **`ast-bench`**: C is one optional engine, not the point of the crate.
It is a plain binary, not a criterion bench.
Criterion's saved baselines are not aware of machine fingerprints, it cannot interleave two builds, and its statistics are slow at N = 1e6.

### Engines

- **Rust** is always present.
  The harness core depends only on fixtures and oracles committed in this repository (`crates/ast-core/tests/fixtures/` and `crates/ast-bench/fixtures/`), so it builds and runs without the submodule.
- **C** sits behind a `c-oracle` cargo feature and is selected with `--engine rust|c|both`.
  `tools/perf/build_c_bench_lib.sh` builds libast from the submodule as a shared library with pinned flags (`-O3 -ffp-contract=off`) into `target/perf/c/`.
  The harness loads it at runtime with `libloading`, in its own namespace, so its PAL and ERFA symbols cannot clash with the copies compiled into `ast-core`.
  The FFI surface is the handful of calls the harness needs: `astFromString` or an equivalent `astChannel` read, `astTranN`, `astGetI`, `astAnnul`, status handling.
  The loader is a library module of `ast-bench` behind `c-oracle`; Phase 0's test in `ast-oracle-tests` uses the same module to load the counting build, so there is one way of reaching C.

### Cases and points

A case is one fixture plus a direction.
Its points come from the fixture's transform-oracle section, so C chose them and the oracle records the expected outputs.

- **N = 1, 10, 100:** only rows whose input and output are non-BAD in the oracle are used, repeated in order if fewer than N exist.
  A small-N measurement made only of BAD points would short-circuit the code under test.
  A case with no such row is a hard error.
- **N = 100 000 and 1 000 000:** all oracle rows, BAD included, repeated in order.
  Repetition makes branch patterns more regular than real data; this is recorded in the README as a known bias, and it applies equally to every engine and build.

### Correctness is gated elsewhere

The harness does not re-check outputs before timing.
Every section of the three committed oracles is already asserted against C's recorded outputs by `test_transform_oracle.rs`, and `every_oracle_section_is_gated` fails if any of the 2261 sections escapes those gates.
Each exception is named in `transform_oracle_overrides.txt`.
The `bench` fixtures, which are not yet in those oracles, get their own gate (see below).

### Timing

- The timed call is the public transform entry point: `Mapping::transform` (or the FrameSet's) in Rust, `astTranN` in C.
- Input and output buffers are allocated once per case and N, outside the timed region.
- Each (case, direction, N, engine) is repeated until a time budget is filled, with a minimum repeat count.
  Median and minimum ns per point are recorded.
- Runs alternate engines (and, under `--base`, builds) so that drift affects each side equally.

### Curated sets

A curated set is a text file under `crates/ast-bench/sets/`: one case per line, with a one-line reason.

- **`bench`** (today's set): one plain Mapping per class, from the new `crates/ast-bench/fixtures/` (see below).
- Later additions: the large series and parallel FrameSets already in the corpus (LSST, SCUBA-2 and DSS-style headers under `wcsconv/` and `framesets/`), and any case the full sweep flags as unusually slow.

### The `bench/` fixtures

The existing single-Mapping fixtures were chosen to exercise the simplifier, and several are negative cases with unusual parameters, so they make poor baselines and could change meaning when a simplify test is edited.

- About 25 `.map` files, one per Mapping class, in `crates/ast-bench/fixtures/`.
  Each is the simplest *realistic* instance: a 2-D ZoomMap, a TAN WcsMap, a degree-3 PolyMap with an inverse, a 3×3 MatrixMap, and so on.
- They are not added to `ast_tester` now; that is a later, separate C pull request.
  They cannot live under `crates/ast-core/tests/fixtures/`, because `tools/fixtures/check_mirror.sh` requires that tree to equal `ast_tester/fixtures` exactly.
- `tools/perf/gen_bench_fixtures.c`, built against the submodule's libast, writes the fixtures, so they are C's own dumps.
  The existing `gen_transform_oracle` is run with `crates/ast-bench/fixtures/` as its root to write `crates/ast-bench/fixtures/bench.oracle`, so C chooses the points and records the outputs.
- A test in `ast-bench`, `bench_fixtures_match_oracle`, asserts every section of `bench.oracle` against the Rust transforms with the same tolerances as `test_transform_oracle.rs`, and checks it examined every section.
  When the fixtures move upstream this test is deleted and the main gate covers them.
- Classes with no realistic transform (for example a Region used as a Mapping) are listed in the README with the reason.

### Commands

| Command | Does |
|---|---|
| `run --set <name> [--engine …]` | Time the set; print case, direction, N, ns/point per engine, and the Rust/C ratio when both ran. |
| `run … --save` | Also store absolute times under `target/perf/<fingerprint>/`, never committed. |
| `compare` | Re-run and compare with the saved local baseline for this fingerprint; list cases slower than the threshold and exit nonzero. Warn, without failing, when a Rust/C ratio is worse than the committed snapshot. |
| `compare --base <git-ref>` | Build `<git-ref>` in a git worktree and run base and current builds alternately in one session; report and gate on the difference. Needs no saved baseline. |
| `sweep` | Run every oracle case at N = 100 and 100 000. Overnight, on demand. |
| `snapshot` | Write the committed ratio file `docs/perf/<platform>.tsv`. |

The slowdown threshold is a named constant with a comment saying why it has that value.
A per-case relaxation is named in source with a reason, and counted in `PLAN.md` §6 like every other relaxation.

### Machine-specific data

- **Absolute times are machine-specific.**
  They are keyed by a fingerprint: CPU model, OS version, rustc version, C compiler and flags, and build profile.
  `compare` refuses to compare across fingerprints and says to re-save.
- **Rust/C ratios are partly portable.**
  Clock speed and load cancel because both engines run in one process, but libm, FMA and vector width, and cache sizes still differ between platforms.
  Ratios are committed per platform tag (`macos-arm64`, `linux-x86_64`), each row recording the git commit, rustc, C compiler and flags, and date.
  A ratio from another platform is information, never a gate.
- `compare --base` sidesteps the problem: both builds run on the same machine in the same session.

### Fairness rules

Recorded in the crate README and enforced by the build scripts:

- Both engines at `-O3` (Rust: release profile).
- C built with `-ffp-contract=off`, because Rust never fuses into FMA.
- No explicit SIMD; auto-vectorisation allowed on both sides.
- Every timed case is covered by an asserting oracle gate.

## Phase 2 — the first full sweep

`sweep` runs every oracle case at N = 100 and 100 000 on both engines, overnight.
It runs once at the start, to confirm the `bench` set misses no slow combination and to commit the first `macos-arm64` snapshot, and thereafter on demand.
A case it flags joins a curated set with its reason.

## Testing

- Unit tests for point selection (including the no-non-BAD-row error), fingerprinting, threshold comparison and ratio computation.
- The Phase 0 count parity is a test in `ast-oracle-tests`, so a mismatch fails loudly.
- The `math` lint is enforced by clippy, which CI already runs.
- `ast-bench` builds and its unit tests run in CI without the submodule (Rust engine only).
- Each check gets a negative control when it lands: a deliberately broken count, a deliberately wrong `bench` transform, and a deliberately slowed case must each be caught.

## Housekeeping in the same change range

- Delete `examples/bench_per_class.rs`, `tests/bench_per_class_c.c` and `examples/bench_polymap_simd.rs` once the `bench` set covers their classes.
- Point `docs/issues/mapping-perf-vs-c-oracle.md` at the first snapshot.
- Update CLAUDE.md's "Hot-path constraint" paragraph, which says no performance gate exists, and the "Bulk-transform optimisation" section, which names `ast-oracle-bench`.
- Update the `PLAN.md` rows for per-class performance and tier 5.
- Amend the port design spec (`2026-04-20-rust-port-design.md`) for the crate rename, with a revision-history entry.

## Order of work today

1. Phase 0: the counting builds on both sides and the parity test over all three oracles. No timing is involved, so this is quick.
2. The `bench/` fixtures and Phase 1 on the `bench` set.
3. Phase 2's sweep, started overnight.

## Follow-ups (out of scope)

- Moving the `bench` fixtures into `ast_tester/fixtures/bench/` with a C pull request, mirroring them, and folding `bench.oracle` into the main oracle gate.
- Optimisations, each its own change: the prototype's faster PolyMap (`../starlink-astrs-proto`), the `PointSet` row-major layout hypothesis in `PLAN.md`, moving BAD checks out of inner loops by substituting NaN internally.
- Counting trig called directly in non-vendored code at a finer grain (per call site, via `tracing`), if Phase 0 shows a mismatch the per-function totals cannot place.
- Modern wcslib's vectorised projection routines, compared with the 20-year-old vendored copy.
- A `linux-x86_64` snapshot.
- `compare --base` as a pull-request CI job, once Rust has reached parity; running both builds in one job makes it usable on noisy shared runners.
- Backporting `compare --base` to the C library, so C gets the same regression gate.

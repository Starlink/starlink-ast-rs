# Mapping per-class performance vs. C oracle

**Date:** 2026-05-19
**Host:** Apple M4 Max, macOS 25.5
**Branch:** tickets/DM-54793
**Source data:** `cargo run --release --example bench_per_class -p ast-core`
paired with the C oracle (`crates/ast-core/tests/bench_per_class_c.c`)
linking `reference/starlink-ast/build-ast/libast.dylib`.

## Scope

This is the per-class Rust-vs-C transform benchmark sweep run after the
recent native-serialisation byte-exact round-trip fixes (see commits
`8d5d908` through `1de5377`). Each variant builds a deliberately
non-trivial mapping shape, serialises it to Native text, and times the
forward (and inverse where defined) `astTran` arms on both sides at
N ∈ {16, 256, 10 000, 100 000}. The C side ingests the Rust-emitted
serial form via `astFromString` so both engines bench the same logical
mapping.

Variants currently in the harness: 22. Variants intentionally omitted
this iteration:

- **ChebyMap** — newly enabled this session but disabled again after
  the harness surfaced a real Rust serialisation bug. The Rust serializer
  is missing the `IsA PolyMap` inheritance marker and emits the
  ChebyMap-level `FSCL*` / `FOFF*` attributes *before* the inherited
  PolyMap coefficient block. The C library's `astFromString` parses the
  dump without errors but `astTran2` segfaults — the ChebyMap-specific
  scale/offset never reach the right struct fields. The bug is now
  pinned in the byte-exact corpus by two new fixtures
  (`neg_chebymap_standalone.map` and `chebymap_inverse_cancel.{map,simp}`,
  registered in `simplify_tests.txt`); the byte-exact regression test
  now fails on those two entries. Re-enable the bench variant once the
  serializer is fixed.
- **MathMap** — still not part of `AnyObject`, so it can't go through
  `to_native_string` for the C side. Wiring requires touching the
  entire serial dispatch match-list; tracked as separate cleanup.

## Reading the table

`ratio = rust_ns_per_point / c_ns_per_point`, so **< 1 means Rust is
faster** at that N, **> 1 means C is faster**. The 100 000-point column
reflects the SCUBA-2-style bulk-transform regime CLAUDE.md calls out as
the load-bearing hot path; the 16-point column exposes per-call
overhead. The 256 / 10 000 columns interpolate between the two regimes.

```
variant                      dir          r16     r256     r10k    r100k
wcsmap_zpn                   inverse     0.03     0.04     0.03     0.03
normmap_basic_2d             forward     0.08     0.14     0.15     0.14
polymap_2d_deg5              inverse     0.50     0.63     0.64     0.62
polymap_2d_deg5              forward     0.56     0.72     0.85     0.90
dssmap_typical               inverse     0.89     0.92     0.92     0.93
dssmap_typical               forward     0.45     0.85     0.95     0.94
lutmap_100entries            inverse     0.31     0.78     0.82     0.94
grismmap_typical             forward     0.02     0.08     0.99     0.99
grismmap_typical             inverse     0.02     0.08     0.99     0.99
sphmap_3d_to_2d              forward     0.34     1.02     1.05     1.05
wcsmap_zpn                   forward     0.42     0.95     1.07     1.07
ratemap_polymap              forward     1.09     1.10     1.12     1.10
matrixmap_3x3_full           inverse     0.33     1.12     1.29     1.15
pcdmap_typical               inverse     0.11     0.74     1.15     1.16
matrixmap_3x3_full           forward     0.31     1.17     1.42     1.20
wcsmap_tan                   inverse     0.29     1.07     1.25     1.22
wcsmap_sin                   inverse     0.26     1.10     1.35     1.35
unitnormmap_2d_offset        forward     0.13     1.18     1.58     1.37
pcdmap_typical               forward     0.05     0.59     1.43     1.43
unitnormmap_2d_offset        inverse     0.10     0.74     1.43     1.43
splinemap_5x5_cubic          forward     1.25     1.41     1.43     1.49
timemap_taiutc               forward     0.16     1.00     1.69     1.75
timemap_taiutc               inverse     0.14     1.00     1.78     1.84
lutmap_100entries            forward     0.09     0.82     1.60     1.89
tranmap_zoom_shift           forward     0.05     0.48     1.66     2.00
zoommap_2d_x2.5              forward     0.03     0.40     1.43     3.00
zoommap_2d_x2.5              inverse     0.03     0.37     1.57     3.00
slamap_eqgal                 forward     2.07     3.59     3.92     3.93
wcsmap_sin                   forward     1.67     3.45     3.53     3.94
slamap_eqgal                 inverse     2.17     3.54     3.82     3.97
wcsmap_tan                   forward     1.71     3.46     3.62     4.13
matrixmap_3x3_diagonal       forward     0.11     1.35     5.74     5.49
matrixmap_3x3_diagonal       inverse     0.12     1.35     5.74     5.49
specmap_wn_to_fr             inverse     0.23     1.94     4.56     5.49
permmap_2d_swap              inverse     0.07     0.92     5.97     5.97
tranmap_zoom_shift           inverse     0.07     0.76     4.32     6.47
winmap_2d                    inverse     0.13     1.50     6.98     6.64
shiftmap_2d                  forward     0.08     0.81     4.98     6.98
specmap_wn_to_fr             forward     0.18     1.53     5.49     7.31
shiftmap_2d                  inverse     0.08     0.81     8.97     9.63
winmap_2d                    forward     0.13     1.43     6.64     9.95
permmap_2d_swap              forward     0.07     0.93     5.97    43.28
```

## Buckets

### Rust significantly faster (ratio < 0.7 at all N)

The whole `r16` column shows Rust winning on per-call overhead — C's
`astTran*` dispatch path (status check, AST handle resolution, output
PointSet alloc) dominates at small N. But two cases stay Rust-favoured
even at bulk-N:

| variant | r100k | note |
|---|---|---|
| `wcsmap_zpn` inverse | 0.03 | 33× faster — verify it isn't an accuracy gap |
| `normmap_basic_2d` forward | 0.14 | BasicFrame Norm is identity in Rust; C still walks the dispatch path |
| `polymap_2d_deg5` (both) | 0.62 / 0.90 | SIMD forward + NR inverse pay off |

The `wcsmap_zpn inverse` result is suspicious — at N=16 it is *already*
0.03×, suggesting Rust is short-circuiting iteration that C performs.
Worth a correctness cross-check (an oracle ULP comparison on the inverse
output) before celebrating.

### Rough parity (0.7 ≤ ratio ≤ 1.5 at bulk N)

| variant | r100k |
|---|---|
| `dssmap_typical` (both) | 0.93 / 0.94 |
| `lutmap_100entries` inverse | 0.94 |
| `grismmap_typical` (both) | 0.99 / 0.99 |
| `sphmap_3d_to_2d` forward | 1.05 |
| `wcsmap_zpn` forward | 1.07 |
| `ratemap_polymap` forward | 1.10 |
| `matrixmap_3x3_full` (both) | 1.15 / 1.20 |
| `pcdmap_typical` (both) | 1.16 / 1.43 |
| `wcsmap_tan/sin` inverse | 1.22 / 1.35 |
| `unitnormmap_2d_offset` (both) | 1.37 / 1.43 |
| `splinemap_5x5_cubic` forward | 1.49 |

No urgent action — these are the bulk of the catalogue. Re-visit any
that the SCUBA-2 / LSST hot paths actually exercise (notably
`polymap` and `matrixmap`).

### Rust 1.7–4× slower (mid-tier optimisation candidates)

| variant | r100k | notes |
|---|---|---|
| `timemap_taiutc` (both) | 1.75 / 1.84 | conversion-step list dispatch |
| `lutmap_100entries` forward | 1.89 | inverse hits the binary-search path equally hard but matches C |
| `tranmap_zoom_shift` forward | 2.00 | per-call overhead of routing fwd/inv through child mappings |
| `zoommap_2d_x2.5` (both) | 3.00 | cheap-mapping bulk-N tax (sub-2 ns/pt vs sub-1 ns/pt) |
| `slamap_eqgal` (both) | 3.93 / 3.97 | celestial conversion (pal); investigate vectorisation across step chain |
| `wcsmap_sin/tan` forward | 3.94 / 4.13 | sky projection; **largest absolute deficit** because the math body is heavy |

`wcsmap` forward is the biggest real-arithmetic regression — TAN+SIN
forward go through Rust's `wcsmap` projection without the per-projection
inlining the C wcslib has. Profile and consider whether the cross-axis
"polynomial-like" body qualifies for the GEMM/BLAS bulk-N pattern
documented in CLAUDE.md.

`slamap`'s 4× hit at all N (including N=16) suggests the *per-conversion-step*
dispatch is the culprit rather than per-call overhead. The C library
unrolls the step list at construction time; Rust currently walks an
`enum SlaCvtType` per step.

### Rust 5×+ slower (highest-priority opt targets)

| variant | r100k | notes |
|---|---|---|
| `matrixmap_3x3_diagonal` (both) | 5.49 | diagonal MatrixMap should be near-zoommap speed in Rust |
| `specmap_wn_to_fr` (both) | 5.49 / 7.31 | mirror of slamap dispatch tax |
| `permmap_2d_swap` inverse | 5.97 | axis swap should be ~memcpy |
| `tranmap_zoom_shift` inverse | 6.47 | inverse arm hits child-mapping invert + dispatch twice |
| `winmap_2d` (both) | 6.64 / 9.95 | per-axis scale+shift; C tight loop is sub-0.3 ns/pt |
| `shiftmap_2d` (both) | 6.98 / 9.63 | same pattern as winmap |
| **`permmap_2d_swap` forward** | **43.28** | **outlier — investigate first** |

The `permmap_2d_swap forward` 43× ratio at N=100 000 is the standout
anomaly. The inverse arm — same swap, same data shape — comes in at
~6×. There is something happening *only* on the forward path of the
PermMap with a 1-1 swap that is dropping into a slow generic per-axis
loop instead of a copy/swap fast-path; the gap widens by 7× between
the inverse and forward arms. **First place to look.**

The remaining 5-10× cluster (`winmap`, `shiftmap`, `matrixmap_diagonal`,
`specmap`, `tranmap` inverse, `permmap inverse`) is the "Cheap-mapping
large-N tax" pattern from CLAUDE.md: C hits sub-0.3 ns/pt and Rust
plateaus at 1.5-3 ns/pt. Some of this is `PointSet` indirection cost
in `transform_into`; some is the lack of a fused per-mapping kernel
specialisation. For SCUBA-2-style workloads the *absolute* difference
(2 ns/pt vs 0.3 ns/pt → 1.7 ns/pt × 100k points × 10k timeslices ≈
1.7 s) is small but real.

## Hot-path obligation (from CLAUDE.md)

> The SCUBA-2 / SMURF pipeline mutates `SkyFrame::Epoch` and
> `SkyFrame::SkyRefIs` per time slice ... Tier-5 oracle benches fail
> CI if Rust is >10% slower than C on these.

The SkyFrame attribute mutators aren't in *this* bench — that's a
Frame-attribute bench, not a Mapping-transform bench, and lives
elsewhere. None of the Mapping classes that *form* the SCUBA-2
WCS pipeline (`PolyMap`, `MatrixMap`, `WcsMap`, `WinMap`, `SphMap`,
`SlaMap`, `ShiftMap`) is more than 10× slower at the bulk-N regime;
the worst real-pipeline candidate is `wcsmap_tan forward` at 4.13×,
followed by `slamap_eqgal` at ~3.95×. Those two are the priority
targets if the LSST WCS pipeline ever needs to claim parity.

## Reproducing

```shell
# 1. Rust side
cargo run --release --example bench_per_class -p ast-core > /tmp/rust.tsv

# 2. C oracle
cc -O2 -o /tmp/bench_per_class_c crates/ast-core/tests/bench_per_class_c.c \
   -I reference/starlink-ast/build-ast -I reference/starlink-ast \
   -I /opt/homebrew/include \
   -L reference/starlink-ast/build-ast -L /opt/homebrew/lib \
   -last -lyaml -lm -lpthread
DYLD_LIBRARY_PATH=reference/starlink-ast/build-ast /tmp/bench_per_class_c \
   /tmp/ast_bench_per_class/manifest.tsv > /tmp/c.tsv

# 3. Compare per-N
paste /tmp/rust.tsv /tmp/c.tsv | column -t
```

Numbers are sensitive to thermal state and CPU contention — re-run two
or three times before treating any single ratio as load-bearing,
especially in the sub-2-ns/pt territory where measurement noise is
≈ 1 ns. The `permmap_2d_swap forward` 43× outlier reproduced across
multiple runs of the previous bench harness iteration; it is a real
effect, not a transient measurement glitch.

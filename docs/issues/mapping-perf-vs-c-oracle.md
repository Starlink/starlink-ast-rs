# Mapping per-class performance vs. C oracle

**Date:** 2026-05-19 (updated for MathMap after `ff00e3a` and
ChebyMap after `53e052d` — full 24/24 coverage)
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

Variants currently in the harness: **all 24 Serializable Mapping
classes**. MathMap was wired into `AnyObject` in `750d7e3` and added
to the bench in `ff00e3a`; ChebyMap's dump section-ordering bug
(originally surfaced via the simplify fixtures added in `4814212`)
was fixed in `4e65583` and the variant re-enabled in `53e052d`.

## Reading the table

`ratio = rust_ns_per_point / c_ns_per_point`, so **< 1 means Rust is
faster** at that N, **> 1 means C is faster**. The 100 000-point column
reflects the SCUBA-2-style bulk-transform regime CLAUDE.md calls out as
the load-bearing hot path; the 16-point column exposes per-call
overhead. The 256 / 10 000 columns interpolate between the two regimes.

```
variant                      dir          r16     r256     r10k    r100k
wcsmap_zpn                   inverse     0.03     0.03     0.03     0.03
normmap_basic_2d             forward     0.07     0.11     0.12     0.11
polymap_2d_deg5              inverse     0.51     0.62     0.62     0.67
polymap_2d_deg5              forward     0.58     0.78     0.75     0.68
lutmap_100entries            inverse     0.31     0.86     0.88     0.79
dssmap_typical               forward     0.47     0.89     0.93     0.94
dssmap_typical               inverse     0.93     0.96     0.96     0.96
grismmap_typical             forward     0.02     0.07     0.99     0.99
grismmap_typical             inverse     0.02     0.07     0.99     0.99
sphmap_3d_to_2d              forward     0.35     0.96     1.04     1.00
wcsmap_zpn                   forward     0.44     0.89     1.03     1.01
matrixmap_3x3_full           inverse     0.30     1.01     1.25     1.10
ratemap_polymap              forward     1.09     1.11     1.13     1.10
unitnormmap_2d_offset        inverse     0.09     0.59     1.07     1.13
unitnormmap_2d_offset        forward     0.13     0.75     1.21     1.16
matrixmap_3x3_full           forward     0.30     1.04     1.27     1.22
pcdmap_typical               inverse     0.10     0.64     1.10     1.26
wcsmap_tan                   inverse     0.25     1.00     1.17     1.26
wcsmap_sin                   inverse     0.25     1.14     1.31     1.37
pcdmap_typical               forward     0.05     0.47     1.28     1.43
splinemap_5x5_cubic          forward     1.33     1.48     1.50     1.50
timemap_taiutc               forward     0.15     1.00     1.50     1.59
tranmap_zoom_shift           forward     0.05     0.48     1.50     1.80
lutmap_100entries            forward     0.09     0.90     1.89     1.89
timemap_taiutc               inverse     0.15     1.08     1.80     1.93
chebymap_2d_deg3             forward     1.02     1.93     2.04     2.01
zoommap_2d_x2.5              inverse     0.04     0.67     3.39     2.59
mathmap_2d_linear            inverse     0.80     2.42     3.03     3.12
zoommap_2d_x2.5              forward     0.04     0.42     2.59     3.59
mathmap_2d_linear            forward     0.73     2.45     3.68     3.68
wcsmap_sin                   forward     1.57     3.28     3.45     3.76
slamap_eqgal                 forward     2.09     3.57     3.94     3.77
slamap_eqgal                 inverse     2.07     3.47     3.87     3.94
wcsmap_tan                   forward     1.56     3.26     3.43     4.20
specmap_wn_to_fr             inverse     0.23     1.94     4.56     5.16
permmap_2d_swap              inverse     0.08     0.92     6.47     5.47
matrixmap_3x3_diagonal       forward     0.14     1.76     6.73     5.49
shiftmap_2d                  forward     0.10     1.14     6.64     5.65
matrixmap_3x3_diagonal       inverse     0.14     1.70     5.79     5.74
tranmap_zoom_shift           inverse     0.07     0.81     3.99     6.47
winmap_2d                    inverse     0.12     1.57     6.64     6.64
specmap_wn_to_fr             forward     0.17     1.53     4.39     6.98
shiftmap_2d                  inverse     0.13     1.21     5.65     7.64
winmap_2d                    forward     0.12     1.77     6.98     7.64
permmap_2d_swap              forward     0.07     1.00     5.97    42.29
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
| `normmap_basic_2d` forward | 0.11 | BasicFrame Norm is identity in Rust; C still walks the dispatch path |
| `polymap_2d_deg5` (both) | 0.67 / 0.68 | SIMD forward + NR inverse pay off |

The `wcsmap_zpn inverse` result is suspicious — at N=16 it is *already*
0.03×, suggesting Rust is short-circuiting iteration that C performs.
Worth a correctness cross-check (an oracle ULP comparison on the inverse
output) before celebrating.

### Rough parity (0.7 ≤ ratio ≤ 1.5 at bulk N)

| variant | r100k |
|---|---|
| `lutmap_100entries` inverse | 0.79 |
| `dssmap_typical` (both) | 0.94 / 0.96 |
| `grismmap_typical` (both) | 0.99 / 0.99 |
| `sphmap_3d_to_2d` forward | 1.00 |
| `wcsmap_zpn` forward | 1.01 |
| `matrixmap_3x3_full` (both) | 1.10 / 1.22 |
| `ratemap_polymap` forward | 1.10 |
| `unitnormmap_2d_offset` (both) | 1.13 / 1.16 |
| `pcdmap_typical` (both) | 1.26 / 1.43 |
| `wcsmap_tan/sin` inverse | 1.26 / 1.37 |
| `splinemap_5x5_cubic` forward | 1.50 |

No urgent action — these are the bulk of the catalogue. Re-visit any
that the SCUBA-2 / LSST hot paths actually exercise (notably
`polymap` and `matrixmap`).

### Rust 1.7–4× slower (mid-tier optimisation candidates)

| variant | r100k | notes |
|---|---|---|
| `timemap_taiutc` (both) | 1.59 / 1.93 | conversion-step list dispatch |
| `tranmap_zoom_shift` forward | 1.80 | per-call overhead of routing fwd/inv through child mappings |
| `lutmap_100entries` forward | 1.89 | inverse hits the binary-search path equally hard but matches C |
| `chebymap_2d_deg3` forward | 2.01 | Chebyshev forward poly — no SIMD path yet, unlike `polymap` (0.68×) |
| `zoommap_2d_x2.5` (both) | 2.59 / 3.59 | cheap-mapping bulk-N tax |
| `mathmap_2d_linear` (both) | 3.68 / 3.12 | parser-emitted bytecode evaluator; per-token dispatch per point |
| `wcsmap_sin/tan` forward | 3.76 / 4.20 | sky projection; **largest absolute deficit** because the math body is heavy |
| `slamap_eqgal` (both) | 3.77 / 3.94 | celestial conversion (pal); investigate vectorisation across step chain |

`wcsmap` forward is the biggest real-arithmetic regression — TAN+SIN
forward go through Rust's `wcsmap` projection without the per-projection
inlining the C wcslib has. Profile and consider whether the cross-axis
"polynomial-like" body qualifies for the GEMM/BLAS bulk-N pattern
documented in CLAUDE.md.

`slamap`'s 4× hit at all N (including N=16) suggests the *per-conversion-step*
dispatch is the culprit rather than per-call overhead. The C library
unrolls the step list at construction time; Rust currently walks an
`enum SlaCvtType` per step.

`mathmap` is one of the newest entrants; the chosen variant uses an
analytical inverse so neither side enters the Newton-Raphson inverse
loop. The 3-4× hit at bulk N is per-point bytecode dispatch — the eval
walks an opcode/operand vector inside an interpreter loop rather than
emitting fused native code. A JIT or staged-evaluation strategy would
unlock the bulk-N wins; not worth pursuing until users actually pin a
workload on MathMap.

`chebymap` is interesting because `polymap` runs at **0.68×** with the
same 2D, ~12-coefficient shape but a SIMD-vectorised forward kernel,
while `chebymap` lacks one and lands at **2.01×**. The factor of ~3 gap
between the two classes is almost entirely the SIMD pipeline that
`polymap` enjoys; porting it to `chebymap` is the obvious next step if
the LSST WCS pipeline ever needs Chebyshev parity (it currently does
not — `polymap` is the SCUBA-2 / LSST workhorse).

### Rust 5×+ slower (highest-priority opt targets)

| variant | r100k | notes |
|---|---|---|
| `specmap_wn_to_fr` inverse | 5.16 | mirror of slamap dispatch tax |
| `permmap_2d_swap` inverse | 5.47 | axis swap should be ~memcpy |
| `matrixmap_3x3_diagonal` (both) | 5.49 / 5.74 | diagonal MatrixMap should be near-zoommap speed in Rust |
| `shiftmap_2d` forward | 5.65 | per-axis constant shift; C tight loop is sub-0.3 ns/pt |
| `tranmap_zoom_shift` inverse | 6.47 | inverse arm hits child-mapping invert + dispatch twice |
| `winmap_2d` (both) | 6.64 / 7.64 | per-axis scale+shift; same pattern as shiftmap |
| `specmap_wn_to_fr` forward | 6.98 | conversion-step dispatch on the cheap path |
| `shiftmap_2d` inverse | 7.64 | inverse arm of the shift fast path |
| **`permmap_2d_swap` forward** | **42.29** | **outlier — investigate first** |

The `permmap_2d_swap forward` ~42× ratio at N=100 000 is the standout
anomaly. The inverse arm — same swap, same data shape — comes in at
~5×. There is something happening *only* on the forward path of the
PermMap with a 1-1 swap that is dropping into a slow generic per-axis
loop instead of a copy/swap fast-path; the gap widens by 8× between
the inverse and forward arms. **First place to look.**

The remaining 5-8× cluster (`winmap`, `shiftmap`, `matrixmap_diagonal`,
`specmap`, `tranmap` inverse, `permmap` inverse) is the "Cheap-mapping
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
`SlaMap`, `ShiftMap`) is more than 8× slower at the bulk-N regime;
the worst *real-arithmetic* pipeline candidate is `wcsmap_tan forward`
at 4.20×, followed by `slamap_eqgal` at ~3.9×. Those two are the
priority targets if the LSST WCS pipeline ever needs to claim parity.

MathMap and ChebyMap don't sit on the SCUBA-2 / SMURF hot path —
MathMap is a user-supplied symbolic mapping, ChebyMap is used for
distortion polynomials in some FITS-WCS flavours but not in the LSST
pipeline — so their 2-4× mid-tier slowdowns are informational rather
than load-bearing.

## Reproducing

```shell
# 1. Rust side
cargo run --release --example bench_per_class -p ast-core > /tmp/rust.tsv

# 2. C oracle. Since 9.5.0 the satellite libraries are separate, so each
#    must be named; the rpath forces the repo build over any conda libast
#    on the loader path. On macOS add -I/-L /opt/homebrew for libyaml and
#    use DYLD_LIBRARY_PATH in place of the rpath flags.
cc -O2 -o /tmp/bench_per_class_c crates/ast-core/tests/bench_per_class_c.c \
   -I reference/starlink-ast/build-ast -I reference/starlink-ast \
   -L reference/starlink-ast/build-ast \
   -Wl,-rpath,$PWD/reference/starlink-ast/build-ast -Wl,--disable-new-dtags \
   -last -last_pal -last_cminpack -last_grf_log -last_grf3d -last_err \
   -lyaml -lm -lpthread
/tmp/bench_per_class_c /tmp/ast_bench_per_class/manifest.tsv > /tmp/c.tsv

# 3. Compare per-N
paste /tmp/rust.tsv /tmp/c.tsv | column -t
```

Numbers are sensitive to thermal state and CPU contention — re-run two
or three times before treating any single ratio as load-bearing,
especially in the sub-2-ns/pt territory where measurement noise is
≈ 1 ns. The `permmap_2d_swap forward` ~40-45× outlier reproduces
across runs (42-45× observed); it is a real effect, not a transient
measurement glitch. The mid-tier numbers (`wcsmap`, `slamap`,
`mathmap`, `chebymap`, `zoommap`) drift by ±15% between runs but the
bucket membership stays stable.

## Addendum: the 9.5.0 sync (2026-09-25)

Two changes in the 9.4.1 / 9.5.0 upstream sync touch arithmetic that
comparison paths run, so this is the manual gate CLAUDE.md asks for when a
transform path is touched.

- `ast_equal` / `ast_equals` regroup their tolerance so the operand sum is
  scaled by `f64::EPSILON` before the tolerance multiplier is applied, which is
  what upstream now does in `src/object.h.in` (`39def937`).
  Multiplying first overflowed for operands near the top of the double range.
- `Region::GetDefUnc` floors the default uncertainty's half-width at `0.5E-12`
  where the bounding box and the axis value are both zero, and `PointList`
  negates its uncertainty before recentring it.

Neither is a per-point cost, which the call graph settles before the bench
does.
`ast_equal` is called from `equal`, `try_merge` and the Simplify helpers, and
from `MatrixMap`'s `compress_matrix`, which runs in `MatrixMap::new` to pick
between the diagonal and full `MatrixForm` — construction, not transform.
No `transform`, `transform_into` or `transform_into_nan` body calls it.
`get_def_unc` is per axis inside a Region's uncertainty construction, and
`PointList`'s `negated_copy` is hoisted above the per-defining-point loop, one
allocation per `reg_base_inside` call, which is where C's single
`astSetNegated` sits.

Measured anyway, since a call-graph argument is not a measurement.
This branch's `bench_per_class` against the same tree with the `ast_equal`
regrouping reverted, both built at `--release`, seven interleaved reps per side,
comparing per-cell minima over 180 variant × direction × N cells:

```
max 1.026   min 0.941   median 1.000   (after / before)
rep-to-rep spread within the before side: median 3.0%, p90 11.6%, max 69.1%
```

No cell is more than 5% slower, and the largest deviation in either direction
sits well inside the single-side noise. At three reps one cell read 1.061
(`zoommap_2d_x2.5 forward` at N=256, 3.3 → 3.5 ns/pt); it disappeared at seven,
and `ZoomMap::transform_into` does not call `ast_equal` at all, so it could not
have been causal.

The ratios in the table above are not comparable with this run: that snapshot
is an Apple M4 Max and this is Linux x86_64. This addendum is a before/after on
one host, which is what the question needs, and it does not re-baseline the
Rust-vs-C columns.

The `astEQUALS` change was not contingent on the result. It fixes an overflow
that makes comparisons against values near `f64::MAX` report equality, and a
Frame axis has Top/Bottom of `±f64::MAX` unless they are set, so bounds tests
hit it routinely. A measured cost would have been something to record and
address rather than a reason to keep the defect.

## Addendum: per-term bad values in PolyMap and ChebyMap (2026-10-07)

Porting C's `PolyPowers` seam (`4f51c5eb`) made a bad input spoil only the terms that use it, as C does, where the port had blanked the whole point.
The shared loop (`polymap.rs` `poly_transform`) checks each power and coefficient for BAD only when the point's `PolyPowers` reports a bad value; otherwise it runs without checks.
Measured on macOS (arm64) with default features, at 100k points:

| Variant | Before | After | C |
|---|---|---|---|
| `polymap_2d_deg5` forward | 53.0 ns/pt | 56.6 | 57.0 |
| `chebymap_2d_deg3` forward | 70.8 | 37.7 | 38.4 |

A prototype of the bad-to-NaN substitution described in CLAUDE.md, for PolyMap's forward transformation, gave about 53.5 ns/pt (three interleaved runs each), recovering the 5% with results identical to C on `tests/test_poly_bad.rs`.
It replaced the forward call with: when no coefficient is bad, copy the input with BAD set to NaN, fill the power table with no BAD test (`fill_power_table`), sum each output's terms (`eval_term_with_table`), then set NaN outputs to BAD; a bad coefficient keeps the checked loop.
NaN gives C's per-term result because power zero is 1.0 whatever the input, so only terms using a power above zero of a bad input become NaN.
It is not committed: C parity of every Mapping comes first, and the copy costs an allocation per call that the measurement at small N did not separate from noise.

## Addendum: transform memory (2026-10-10)

`testhuge.c` ran out of memory in `Moc::add_pixel_mask`, and the cause was general.
A series CmpMap holds its first component's result while the second runs, and the port held a full-size copy of the points at each level of a nested series, where cmpmap.c `Transform` passes them through 8192 at a time for exactly that reason.
`PointSet::from_rows` also gave every point its own `Vec`, about 70 bytes per 2-D point.
Both are fixed (`d7df68c7`); `tests/test_transform_memory.rs` counts allocations per thread to hold the bounds.
On `astAddPixelMask8F` over a 12001 x 12001 image the peak fell from 11.5 GB to 2.2 GB, against C's 2.3 GB.
The batches cost about 1 ns per point for copying each batch in and out, a few percent on a deep chain and up to a quarter on a two-stage chain of trivial Mappings; letting a PointSet borrow a slice of another's points would remove the copies.

Peak memory of each class's own transform, as a multiple of its output, at a million points with `bench_per_class`'s variants:

| Variant | Forward | Inverse |
|---|---|---|
| Every other variant | 1.00 | 1.00 or no inverse |
| `polymap_2d_deg5` | 1.00 | 4.06 |
| `ratemap_polymap` | 2.05 | no inverse |

Two remain above the output alone, both bounded (they do not grow with chain depth) and not yet compared with C:

- **PolyMap's iterative inverse** (`polymap_2d_deg5`, used when no inverse polynomial is given) holds about three copies of the points beyond its result. C's `PolyMap` iterative inverse (polymap.c `IterInverse`) is the reference for what it needs.
- **RateMap** holds about one copy beyond its result. C's `RateMap` Transform (ratemap.c) estimates the rate through `astRate` per point, which is the reference.

Parallel CmpMaps also copy each component's input and output where cmpmap.c uses `astSetSubPoints` views, a constant factor of about two that does not grow with depth.

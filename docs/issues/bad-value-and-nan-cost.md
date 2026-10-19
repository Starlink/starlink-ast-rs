# Discussion: what AST__BAD, NaN and infinity cost in transforms

Status: open discussion, nothing decided.
It records what C and the port do with `AST__BAD`, NaN and infinity in transforms, what that costs, and the options for changing it, here and upstream.
Measurements are from macOS aarch64 (Apple M1 Ultra), `ast-bench` at commit `97c2d73e`, release builds, C at `-O3 -ffp-contract=off`.

## What C does

- Every `Transform` kernel tests each input for `AST__BAD` and writes `AST__BAD` for it; nothing else is treated as bad.
  A NaN or infinite input goes through the arithmetic and comes out as IEEE makes it (`matrixmap.c` `Transform`, diagonal branch: one compare, one multiply, one select per value).
- `astTransform` (`mapping.c:24815`) calls `astReplaceNaN` on the result.
  That does nothing unless the environment variable `AST_REPLACE_NAN` is set (`pointset.c:1614`); with it set to 1 it replaces NaN outputs with `AST__BAD`, and with another value it reports an error.
- A CmpMap transforms each component through `astTransform` (`cmpmap.c:4110`), so with `AST_REPLACE_NAN` set the replacement runs after every stage, not once at the edge.
  By default it never runs, and C's outputs can hold NaN and infinity.

## What the port does

- The output contract (`CLAUDE.md`) is that outputs always use `BAD_VALUE`, never NaN; NaN is accepted on input as bad.
- Kernels test each input with `is_bad`, which is true for `BAD_VALUE`, NaN and both infinities (`mappings/helpers/mod.rs`).
  So in a CmpMap every stage re-tests every value, where C tests only for `AST__BAD`.
- The handling of infinity is not uniform: UnitMap and NormMap turn only NaN into `BAD_VALUE` and pass infinity through (`unit.rs:88`, `normmap.rs:164`), unlike the kernels that use `is_bad`.
- There is a second path, `transform_nan_protocol` (`mappings/mod.rs:978`): it copies the input with `BAD_VALUE` turned into NaN, runs each stage's `transform_into_nan`, which lets NaN propagate without per-value tests, and turns NaN back into `BAD_VALUE` in one pass over the output.
  It is the "convert once at the edge" design, but only `ast-bench` and tests call it; `Mapping::transform` and the C API use the per-stage path.

## What it costs

### The per-value test in a cheap kernel

The Roman benchmark's 2x2 diagonal MatrixMap (`ast-bench roman`), while the data fits in cache:

| Bad-value test | Million points per second |
|---|---:|
| C: `v == AST__BAD` | 3 700 |
| Port with C's test (experiment, not kept) | 3 600 to 3 800 |
| Port with `is_bad` (current) | 2 000 |
| Port, four exact rewrites (blocked checks, two comparisons) | 1 750 to 2 300 |

C's compiled loop and the port's with C's test are the same instructions (`fcmeq`, `fmul`, `bsl` per pair of values); `is_bad` adds three vector operations per pair to a loop that is otherwise a multiply.
At the full Roman array (4088 x 4088 points) memory bandwidth dominates and the port is at 0.9 of C.
`matrix_diagonal_long_array_with_non_finite_inputs` in `tests/test_matrixmap.rs` pins the port's stricter behaviour.

### The per-value test in real chains

Nanoseconds per point at 1 000 000 points, per-stage path against the edge-only NaN path:

| Case | Per-stage | NaN path | C |
|---|---:|---:|---:|
| `lsst.ast` forward | 149.7 | 150.8 | 210.9 |
| `lsst2.ast` inverse | 1443 | 1446 | 1724 |
| ACSIS cube forward | 57.9 | 59.7 | 61.1 |
| `rigby.map` forward | 122.3 | 116.1 | 133.9 |
| `brad.map` inverse | 16.2 | 18.5 | 14.7 |
| UnitMap 2-D | 0.51 | 1.74 | 0.32 |
| WinMap 2-D | 0.58 | 1.80 | 0.50 |
| ZoomMap 2-D | 0.55 | 1.85 | 0.70 |

In every chain but `rigby` the edge-only path is no faster, and for cheap Mappings it is two to three times slower.
The kernels' per-value tests are branchless selects, which cost little next to real arithmetic, so skipping them saves little; the two boundary passes cost about 1.2 ns per point for two axes, because the caller's input cannot be changed in place and has to be copied.
Converting once at the edge only pays when the stages are many and cheap.

### The edge-only path is not yet correct everywhere

Of 2 313 corpus transforms, four give different bits on the two paths (`ast-bench bits`, which records both): `cap_xphmap_01.map` forward, `cap_xphmap_02.map` and its `.simp` inverse, and `neg_switchmap_series_lower.map` inverse.
The per-stage path is the one checked against C, so these are faults in the NaN path's XphMap and SwitchMap handling.
They show the general risk: wherever `AST__BAD` means more than "a value that poisons arithmetic", NaN propagation is not the same rule.
Known places:

- MatrixMap: a zero element masks a bad input (`0 * NaN` is NaN);
- PolyMap: C skips a zero power, so a bad input with power zero does not poison the term;
- SwitchMap and SelectorMap: a bad selector selects no route;
- LutMap: a bad table entry, and the flags marking entries next to one;
- Regions: `AST__BAD` marks a point outside;
- every kernel that writes `AST__BAD` for a point outside its domain (projections, iterative inverses that fail to converge).

## Options

### A. Keep the port as it is

Outputs never hold NaN; every kernel tests NaN and infinity as bad.
Cost: the cheapest kernels run at about half C's speed while their data is in cache (about 0.5 ns per point against C's 0.27, so about 0.2 ns per point); real chains are unaffected or faster than C.
No work.

### B. Make the edge-only path the default

Keeps the output contract, and tests each value once instead of once per stage.
The measurements above say it gains nothing on real chains and loses on cheap ones, and it needs the four faults fixed and the list above audited first.
Not worth doing as it stands.

### C. Follow C: test only `BAD_VALUE`, let NaN and infinity through

Kernels test only for `BAD_VALUE`, as C's do, and outputs can hold NaN and infinity, as C's do by default.
`BAD_VALUE` handling stays exactly C's, so bit parity with C extends to NaN and infinite inputs, which today it does not cover.
Provide helpers for callers who want one sentinel: NaN to `BAD_VALUE` over an output, and `BAD_VALUE` to NaN, which some callers do anyway since tools outside AST and Starlink have no double-precision bad constant.
Cost: an API change in the contract (`CLAUDE.md`, the design spec), the UnitMap and NormMap special cases removed, and every test that expects NaN in to give `BAD_VALUE` out revisited.
Gain: the cheap kernels' 2x while in cache, and a contract that matches the library being replaced.

### D. Propose an IEEE transform mode to upstream AST

A caller declares, for a transform call or a Mapping, that it uses no `AST__BAD`: bad values are NaN on input, NaN and infinity follow IEEE rules through every Mapping, and outputs are left as they are.
Kernels in that mode have no per-value tests at all, which is what lets compilers vectorize them (the SIMD kernels in [Starlink/ast pull request 51](https://github.com/Starlink/ast/pull/51) already need a scalar fix-up pass for bad values), and the input copy and output sweep of option B disappear because nothing is converted.
What it needs, in C and here alike: every place listed under "not yet correct everywhere" given a NaN rule (a zero matrix element masking NaN, a NaN selector selecting no route, out-of-domain points written as NaN rather than `AST__BAD`), and a test corpus run in both modes.
This is a design discussion for upstream first; the port would follow the API C adopts.

## Open questions

- Should the port's contract follow C's default (option C) before anything else, so that results for NaN and infinite inputs are comparable with C at all?
- Is there appetite upstream for option D, and would it be per call (a flag on `astTranP` and friends) or per Mapping (an attribute)?
- Do any users depend on the port turning NaN outputs into `BAD_VALUE`, or on C's `AST_REPLACE_NAN`?

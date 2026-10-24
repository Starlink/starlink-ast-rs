# Per-axis PointSet and borrowed point views — design

**Date:** 2026-10-16
**Status:** implemented; every Mapping's transform kernel and the Region transform read and write whole axes (see "As built")
**Amends:** [`2026-04-20-rust-port-design.md`](2026-04-20-rust-port-design.md), the `PointSet` layout and the claim that its buffers cross the C ABI without a copy (rev 34, see "Spec amendment").

## Purpose

Every cheap Mapping is about 2.5 times slower than C at large N, and a C caller would see far worse.
This change makes the C API as fast as C for transforms, and makes the same entry point serve Rust callers at that speed.
A C user has no reason to replace the C library with a version that is slower at its most-used operation.

## Measurements behind the design

All timings are ZoomMap, 2 axes, 10⁶ points, macOS aarch64, release build, best of 20.

| Path | ns per point |
|---|---|
| C `astTranN` | 0.68 |
| Port `transform_into` on a `PointSet` | 1.48 |
| The same arithmetic over the flat buffer in one loop | 0.51 |
| Port `tran2` (per-axis arrays in and out, as a C caller passes them) | 3.93 |

The `tran2` figure splits into 0.93 to build the row-major `PointSet` from columns, 1.48 for the kernel, and 1.03 to read the columns back out.

Two causes follow.

1. **The kernels.**
   Each Mapping loops over points, fetching each point's row through `PointSet::point(i)` and `point_mut(i)` (ndarray `row(i).to_slice()`), then loops over 2 or 3 coordinates.
   The compiler cannot vectorize that, and the accessor is not free.
   The BAD↔NaN substitution is not the cause: dropping the BAD check altogether only reaches 1.14.
2. **The layout.**
   C's `TranN` (`mapping.c`, `TranN`) points its PointSet at the caller's per-axis arrays with `astSetPoints` and copies nothing.
   `astTranN`, `astTran2` and `astTranP` all take per-axis arrays.
   The port's row-major `PointSet` forces a transpose in and out of every C call.
   A well-written transpose still costs about 0.66 ns per point in each direction, a floor of about 2 times C before any arithmetic.

Fixing the kernels alone would help Rust callers but leave every C caller at least 2 times slower than C.

## Decision

`PointSet` stores one contiguous array per axis, as C's PointSet does.
Transforms read and write through borrowed views of per-axis slices, so the C shim can wrap the caller's arrays without copying, exactly as C's `TranN` does.

A separate Python package over the Rust library is planned; it is out of scope here and places no requirement on this design.

## Types

### `PointSet`

- Storage stays an ndarray `Array2<f64>`, with its shape flipped to `(ncoord, npoint)` in standard (C) order, so axis `j` is the contiguous row `j`.
- New: `axis(j) -> &[f64]`, `axis_mut(j) -> &mut [f64]`, `as_points() -> Points<'_>`, `as_points_mut() -> PointsMut<'_>`, and `set_point(i, &[f64])`.
- `point(i)` returns a `PointBuf`, an owned copy of the point's coordinates held inline (no allocation) up to the inline capacity below.
  `PointBuf` dereferences to `[f64]` and compares equal to slices, arrays and vectors of `f64`.
  Most existing reads, such as `out.point(0)[1]` and `assert_eq!(out.point(0), &[..])`, compile unchanged.
- `point_mut(i)` is removed; its callers move to axis loops or `set_point`.
- `from_columns` becomes a plain copy of each column; `from_rows` stays, and transposes.
  `from_rows` is for tests and convenience, not hot paths.
- `view()`, `view_mut()`, `from_ndarray()` and `into_ndarray()` use the new `(ncoord, npoint)` shape.
  This breaks the public Rust API, which is acceptable while it is unreleased.

### `Points<'a>` and `PointsMut<'a>`

- `Points<'a>` is a read view: `ncoord` slices `&'a [f64]`, each `npoint` long.
- `PointsMut<'a>` is the writable counterpart, with `&'a mut [f64]` slices.
- The list of axis slices is a `smallvec::SmallVec`, held inline up to a fixed capacity and on the heap beyond it.
  Only the list of pointers spills, never the data, and results are identical either way.
  C pays the same cost on every call: `TranN` mallocs its `in_ptr` and `out_ptr` arrays each time.
- The inline capacity is 8.
  The oracle corpora (`simplify_fixtures`, `headers`, `framesets`, `bench`) use at most 5 axes, so every corpus case stays inline; a view costs about 128 bytes of stack.
- Constructors check that every axis has the same length and fail with C's codes: `Ncoin` for no axes, `Ncpin` for a mismatched axis length.
- Views support taking points `start..end` of every axis, and taking a sub-range of axes.
  Both are zero-copy.
- Views have `point(i) -> PointBuf`, and `PointsMut` has `set_point(i, &[f64])`.
  These are permanent API for code that is not a bulk kernel (Regions, FrameSet, the per-point fallbacks of classes not yet rewritten) and for tests.
- `smallvec` is a new dependency of `starlink-ast`.

### The `Mapping` trait

- `transform_into(&self, input: Points<'_>, forward: bool, out: PointsMut<'_>) -> AstResult<()>` is the one required transform method.
- `transform_into_nan` takes the same arguments.
  Its behavior and the boundary BAD↔NaN copy in `transform_nan_protocol` are unchanged.
- `transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet>` becomes a provided method that allocates the output and calls `transform_into`.
  It replaces 41 near-identical copies.
- `tran1` and `tran2` build views over their argument slices and write into the returned vectors directly, with no `PointSet` in between.
- `validate_transform` checks view shapes against `nin` and `nout` as it checks `PointSet` shapes today.

### CmpMap

- A parallel CmpMap gives each child a sub-range of the view's axes, instead of copying columns.
- A series CmpMap batches by taking points `start..end` of every axis, instead of copying each batch out and back.
- The intermediate buffer between series stages remains an owned `PointSet`, reused across batches.

### The C shim

`starlink-ast-capi` is still a stub, so nothing is wired in this change.
When `astTranN`, `astTran2` and `astTranP` are written, they build `Points` over `in + coord * indim` (or the per-axis pointers) and `PointsMut` over the output arrays, then call `transform_into`.

C does not document in-place transforms, and no C source passes the same PointSet as input and output.
A C caller may still pass overlapping arrays, which Rust views cannot express.
The shim detects overlap between the input and output ranges and copies the input first; only callers that overlap pay that cost.

## Bit-exactness

Converting a kernel to an axis loop changes the order in which points are visited, never the arithmetic applied to one point.

- Sums across axes, as in MatrixMap's `apply_matrix`, accumulate over the input axes in the same order as the C kernel, with no ndarray `dot` or BLAS call standing in for them.
  Each output axis becomes an array of accumulators updated one input axis at a time, which keeps every point's sum in C's order.
- No fused multiply-add contraction is introduced.
- Vectorizing an elementwise loop does not change any element's result.

The oracle corpus and the simplify corpus therefore stay byte-exact, and any change in them is a defect.

## Stages

Each stage leaves the tree green.

### Stage 1 — a C-caller timing in `ast-bench`

Add a timing that has the shape of a C caller: per-axis input arrays and per-axis output arrays, with any conversion the port needs inside the timed region.
Today it runs through `tran`-style column conversion and shows the 3.93 figure above.
Record it in the `bench` and `framesets` sets so later stages show and protect the C-API gain.

### Stage 2 — flip the layout and change the signature, behavior unchanged

Change `PointSet` storage, add the views, change the trait, and convert every Mapping and every other `point_mut` caller (121 sites; `point` has 455 source and about 1,330 test reads).
Each Mapping keeps its current per-point logic, reading each point with the view's `point(i)` and writing it with `set_point(i, ..)`.
The 51 `view()` and `view_mut()` uses, mostly matrix arithmetic, are each checked for the flipped orientation, not only recompiled.

This stage cannot be split by class, because the trait signature changes everywhere at once.
Its gate is correctness: the full `starlink-ast` suite, `ast-oracle-tests` (bit or ULP against C) and the simplify corpus.
CI runs neither of the last two, so they run by hand on this stage.
Speed is recorded but not expected to change much.

### Stage 3 — axis-loop kernels, one class per commit

Rewrite each Mapping's kernel as loops down whole axes.
This change rewrites ZoomMap, ShiftMap, WinMap, UnitMap, PermMap, MatrixMap, and CmpMap's axis splitting and batching.
Each commit passes `ast-bench compare --set bench --base <previous>` and states its gain in the message.

The other classes slower than 1.4 times C at N=100 000 in `docs/perf/2026-10-16-macos-aarch64-bench.tsv` are SpecMap, MathMap, SplineMap and LutMap.
They are a follow-up plan, ordered by the sweep re-recorded in stage 4, since stage 2 changes their numbers.
Classes within 1.3 times of C keep per-point code, because their arithmetic, not the loop, dominates.

The rule in `CLAUDE.md`'s bulk-transform section still applies to anything beyond the loop shape: a BLAS or GEMM path is committed only when `ast-bench` shows it winning on the shapes that occur.

### Stage 4 — close out

- Re-record the `docs/perf` sweep for the tech report, beside the first one.
- Amend the design spec (rev 34), see below.
- Update `CLAUDE.md`'s bulk-transform section to describe axis-loop kernels over views.

## Testing

- Unit tests for `Points` and `PointsMut`: construction, both error codes, point ranges, axis ranges.
- A test with C's `TranN` layout: all axes in one buffer at a stride `indim` greater than `npoint`, giving the same output as an owned `PointSet`.
- Negative control: a view with one axis shorter than the rest is rejected with `Ncpin`.
- A test that `tran2` returns the same values as `transform` on an equivalent `PointSet`.
- Existing per-class tests, `ast-oracle-tests` and the simplify corpus cover behavior.
- `ast-bench compare --base` gates every stage-3 commit.

## Spec amendment

Rev 34 of `2026-04-20-rust-port-design.md`:

- `PointSet` is one contiguous array per axis, shape `(ncoord, npoint)`.
- The statement that a row-major `Array2` lets `starlink-ast-capi` hand buffers to C without a copy is withdrawn; it was false for every C transform entry, all of which take per-axis arrays.
- `transform_into` takes `Points` and `PointsMut`, and the C shim builds them over caller memory without copying.

## As built

Where the code differs from the design above:

- `point_mut(i)` stays, returning `PointMut`, a guard that holds a copy of the point and writes it back when dropped.
  Hand-placed `set_point` calls at 122 sites missed write-backs on `continue`, `?` and early-return paths.
- The list of axis slices is `AxisList`, an enum of an inline array of 8 and a `Vec`, rather than a `SmallVec`, so that a `PointSet` can build its views without a loop.
  The inline capacity stays 8.
- Stage 3 covers every Mapping, not only the seven classes named there: the other kernels were converted to axis loops or to direct indexing of bound axis slices, so that no class is slower than before the change at large N.
  Non-kernel code (Region geometry, `map_box`, `linear_approx`, the simplifier) still reads points with `point(i)`.
- The views avoid a copy only where a Mapping's kernel reads and writes them directly.
  Every Region, SelectorMap, SwitchMap and PolyMap's iterative inverse still implement `transform_into` by copying the input view into a `PointSet`, transforming that, and copying the result into the output view, so a C-shaped caller pays two copies for them.
- A view costs about 6 ns per transform call at N=1 over the old `&PointSet` arguments: building two views and passing them by value.
  Rust callers at N=1 accept that cost; a C-shaped caller's single-point transform fell from about 61 ns to 13 ns, against C's 540 ns.
- Storage is a plain `Vec<f64>` holding the axes one after another, not an ndarray `Array2`; `view()` and `view_mut()` still give `(ncoord, npoint)` ndarray views of it.
- MatrixMap's full kernel and UnitNormMap's forward kernel unroll their per-point arithmetic for small axis counts, with selects in place of branches, so that the loop over points vectorizes; larger axis counts use blocked passes.

## Out of scope

- Writing `starlink-ast-capi`'s transform entry points.
- The planned Python package.
- Explicit SIMD; the open C pull request adding SIMD is the comparison point for that later.
- Redesigning the NaN protocol's boundary copy.
- Region, Frame and FitsChan code that reads points but is not a transform kernel, beyond what stage 2 needs to compile.

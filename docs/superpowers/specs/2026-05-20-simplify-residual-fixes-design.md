# Simplify Regression — Residual Failure Fixes

Date: 2026-05-20

## Goal

Drive `cargo test -p ast-core --test test_simplify_regression` to **0 failures
and 0 skipped** entries in `simplify_tests.txt`, modulo IntraMap (out of scope —
see §4).

Current state:

```
Simplify regression: 246 passed, 1 skipped,
  9 equal() failures, 2 transform failures, 10 serialization failures,
  0 other failures
```

The skipped test is `intramap_inverse_cancel` (IntraMap class not yet ported).
The two transform failures are also in the equal-failure list, leaving
**11 unique failing fixtures**.

## Failure groups

The 11 failures cluster into three independent groups with different root
causes. Each group is addressed by a separate, mostly self-contained patch
series; PRs land in the order C → A → B (smallest, most isolated first).

### Group C — Per-class normalisations (3 fixtures, serial-only)

| Fixture | Why it fails | Fix |
|---|---|---|
| `matrix_full_to_diagonal` | `MatrixMap::simplified` collapses to `UnitMap` / `ZoomMap` but does not switch a `Form="Full"` matrix whose off-diagonals are zero to `Form="Diagonal"`. | Extend `MatrixMap::simplified` (`crates/ast-core/src/mappings/matrix.rs:333`) so that, when the stored form is `Full` and `is_diagonal(&self.matrix)` holds (and the inverse, if present, is also diagonal), it returns a clone of `self` with `form` and `inv_form` set to `MatrixForm::Diagonal`. The numeric content is unchanged; only the storage form switches. |
| `grism_zoom_inv_merge` | The merge math is correct (`GrmWR = 5000 × 0.5 = 2500`), but `GrismMap::merge_with_zoom` constructs the result via `GrismMap::new`, which marks **all eight** attributes as explicit-set. The C reference only marks the **three attributes that the merge math touched** (`GrmNRP`, `GrmWR`, `GrmG`) as explicit; the others retain the default-flagged status from the source GrismMap. | Modify `merge_with_zoom` (`crates/ast-core/src/mappings/grismmap.rs:233`) to construct the result by `clone()`-ing `self` (which copies the existing `set_flags`) and then writing the three modified scalars while flipping only `set_flags.{nrp, wave_r, g}` to `true`. `GrmNR`, `GrmAlp`, `GrmM`, `GrmEps`, `GrmTh` keep their original explicit/default state. |
| `unit_parallel_invert_clear` | A parallel CmpMap with `InvB=1` and a `UnitMap` with `Invert=1` in `MapB` is structurally a no-op double-inversion of a UnitMap. Our `flatten` strips the inner `Invert=1` and sets `effective_inv=true` instead; reconstruct then emits the parallel CmpMap with `InvB=1` flag, while the C reference output has no `InvB` and a UnitMap with `Invert=1`. Transform output is identical; only serialisation differs. | In `simplify::reconstruct` (parallel branch), when an entry's `effective_inv` is true *and* the entry is `is::<UnitMap>()` (UnitMap is self-inverse), restore the inversion onto the leaf by calling `entry.invert()` and emit it with no external inv. This produces a UnitMap with internal `Invert=1` and no parent `InvB=1` — byte-equal to the C reference. The same idea applies symmetrically for the `MapA` side. |

### Group A — CmpMap structural simplifications (5 fixtures)

All five are equal-failure + serial-failure pairs. The pattern is the same:
**inversion flags on the outer CmpMap need to be distributed into nested
sub-CmpMap children before `merge_loop` can see the cancellation pair**, and
**`PermMap`-with-constants must propagate constants when swapped past a
parallel CmpMap**.

The C reference algorithm lives in `reference/starlink-ast/src/cmpmap.c`,
function `MapMerge` (line 1205). The Rust port (`simplify::flatten`,
`simplify::merge_loop`, `try_swap_permmap_parallel`) implements the
non-inverted, non-constant cases. The five failures are the inverted /
constant edge cases.

| Fixture | Pattern | Required rule |
|---|---|---|
| `cmpmap_nested_inverse_cancel` | Series CmpMap with `InvB=1` over a series-CmpMap whose children are `(MatrixMap, UnitMap)` and `(UnitMap, MatrixMap)`. | When `flatten` encounters an inverted *child* CmpMap of the same composition kind as the parent, it already handles the case (XOR of `cmp.is_inverted()` and `inv`). What it doesn't handle: the resulting flat list contains `MatrixMap, UnitMap, UnitMap⁻¹, MatrixMap⁻¹` — but `MatrixMap⁻¹` has `effective_inv = true`, and `merge_loop` does not currently recognise "M then M⁻¹" as a cancellation pair when one carries the external inv. Fix in `merge_loop`: for adjacent `(a, inv_a)` and `(b, inv_b)` where `a.equal(b)` and `inv_a != inv_b`, replace the pair with `UnitMap(nin)`. |
| `cmpmap_parallel_in_series_merge` | Series of two parallel-CmpMaps; second has `InvA=1, InvB=1`. After flatten, both halves of the second parallel CmpMap come out inverted; the resulting series list is `(MathMap_2x, MathMap_3x) ‖ (MathMap_2x⁻¹, MathMap_3x⁻¹)`. | Same cancellation rule as above, applied parallel-wise inside the merge loop: detect the case "parallel-list adjacent to its inverse" and collapse to UnitMap. The flattened structure already exposes the cancellation; merge_loop needs to look one step ahead. |
| `cmpmap_series_in_parallel_inverted` | Top-level parallel CmpMap with outer `InvA=1`; MapA is a series-CmpMap of `(MathMap_2x, MathMap_2x)`. Output pushes outer InvA inside, producing inner CmpMap with `InvA=InvB=1` and clearing the top-level `InvA`. | This is `flatten`'s job: when `cmp_inv != cmp.is_inverted()` and the child is a *different-kind* CmpMap (here parallel-over-series), flatten cannot recurse and instead returns the single child with the accumulated inv. The reconstructed parallel CmpMap then has the inv flag set on MapA externally — but the C reference *interns* the inversion: it inverts each grandchild explicitly (so the inner series CmpMap stores `InvA=InvB=1`) rather than carrying an external InvA on the outer CmpMap. Fix in `reconstruct`: when emitting a parallel list whose entry carries `effective_inv = true`, push the inversion **into** that entry by calling `entry.invert()` (and, if the entry is itself a CmpMap, distribute the invert into the child flags via `cmpmap.invert()`'s existing semantics) rather than emitting an outer `InvA` / `InvB` flag. |
| `cmpmap_perm_swap_aconstants` | Series of `PermMap[Out=(-1, 1), Con=99]` then parallel `(MathMap_2x, MathMap_3x)`. C swaps the PermMap to the right of the parallel: the new constant set becomes `{99, 198}` because `99 → MathMap_2x → 198`. | `try_swap_permmap_parallel` (`simplify.rs:1563`) currently rebuilds the swapped PermMap but does not apply the parallel branches' transform to the constants vector. Add: for each PermMap output that references a constant, **transform the constant through the corresponding parallel branch** before placing it in the swapped PermMap's constants array. Use the branch's `transform()` method on a 1-row PointSet; on success, replace the constant with the result; on failure (branch isn't axiom-evaluable), refuse the swap. |
| `cmpmap_perm_swap_bconstants` | Mirror of the above (PermMap on the right, parallel on the left). | Same fix in the symmetric branch of `try_swap_permmap_parallel`. The constant should be transformed through the **inverse** of the parallel branch since the swap moves the PermMap to the left. |

The C reference for the constant tracking is in `cmpmap.c:MapMerge` around
the `SwapPermMap` invocations (lines ~1900-2030); the function
`SwapPermMap` itself (in `cmpmap.c`) computes the new constants array by
calling the parallel branch's `astTran1` / `astTran2`.

### Group B — Region merges (3 fixtures, 2 numerically wrong)

Region simplification currently lives only in per-class `simplified()`
methods, which handle "remap defining points to current frame" but **no
inter-region merging**. Two fixtures hard-fail correctness because the Rust
simplifier returns the original parallel CmpMap of two 1-D regions while the
C reference produces a single 2-D Box on a `CmpFrame`.

| Fixture | Required rule |
|---|---|
| `interval_self_simplify` | A 2-D `Interval` with all bounds finite and `lbnd[i] <= ubnd[i]` is exactly a `Box`. Extend `Interval::simplified` (`crates/ast-core/src/regions/interval.rs:610`): when `effective_bounds` returns no `BAD_VALUE` entries, build a `BoxRegion` from the same bounds on the same frame, copy region attributes (`negated`, `closed=true`, `adaptive`, `mesh_size`, `fill_factor`, uncertainty, `IsSimp=true`), and return that. Otherwise return self unchanged. |
| `box_parallel_merge` (correctness) | Implement `BoxRegion::try_merge` for the parallel case. Given two `BoxRegion`s `A` (1-D) and `B` (1-D) under a parallel CmpMap: build a `CmpFrame` from the two frames, concatenate the defining points (`A` has `npoint × naxes_A`, `B` has `npoint × naxes_B` — the C library uses `npoint=2` by convention; concatenate the columns), and return a new `BoxRegion` on that CmpFrame with the combined points. Domain becomes `"CMP"` (from `CmpFrame` defaults). The series case returns `None`. |
| `interval_parallel_merge` (correctness) | Implement `Interval::try_merge` for the parallel case. The C reference's behaviour: when both intervals have all bounds finite, the merged region is reclassified to `BoxRegion` (matching the `interval_self_simplify` rule applied to the merged region). When any bound is infinite, the merged region must remain an `Interval`; if both are intervals, build a 2-D Interval on a CmpFrame. |

The defining-points layout for a `BoxRegion`/`Interval` is `npoint=2,
ncoord=naxes`: row 1 = corner-min (or centre, depending on subclass), row 2 =
corner-max. Concatenation is column-wise: `(a_row1, b_row1)`, `(a_row2,
b_row2)`. The output Closed/Negated/Adapt flags are the per-component AND/OR
that C uses; check `box.c:MapMerge` (`reference/starlink-ast/src/box.c`) and
`interval.c:MapMerge` for the exact semantics.

`CmpFrame` exists; constructor signature can be confirmed via `grep -nE 'pub
fn new' crates/ast-core/src/frames/cmpframe.rs`.

## Architecture & rationale

### Where each fix lives

| Group | Files touched |
|---|---|
| C | `crates/ast-core/src/mappings/matrix.rs`, `grismmap.rs`, `simplify.rs` |
| A | `crates/ast-core/src/mappings/simplify.rs` (merge_loop, reconstruct, try_swap_permmap_parallel) |
| B | `crates/ast-core/src/regions/box_region.rs`, `interval.rs` (new `try_merge` impls) |

No new modules, no public API changes, no ABI change in `ast-capi`.

### Why per-class `try_merge` for regions

Mirrors the existing pattern (MatrixMap, ZoomMap, ShiftMap, GrismMap, etc.
all live in their own files and implement `try_merge` for class-specific
fusions). Centralising region merges in `simplify.rs` would force `simplify`
to import region-specific types, breaking the layering of regions on top of
mappings (regions implement `Mapping` but `simplify` is mapping-focused).

### Why "port C MapMerge inverse-distribution verbatim"

The CLAUDE.md project rule is explicit: "Port should study C library
algorithms, not reinvent from first principles." The five Group A fixtures
are exactly the cases where the C library's `MapMerge` is non-trivial. Adding
ad-hoc Rust rules per fixture risks reinventing partial logic; future
fixtures (or pyast users hitting the same shapes in production) may regress.
The C function is ~600 lines of dense pointer-arithmetic but the algorithmic
shape is small: (a) inverse-distribution into nested CmpMaps, (b)
constant-propagation through parallel-branch transforms when swapping
PermMaps. We port those two shapes faithfully and let the existing Rust
flatten/merge-loop infrastructure absorb the rest.

### Hot-path impact

None. All changes are in `simplified()` / `try_merge` / merge-loop paths,
which run during canonicalisation, not during the per-point transform path
that SCUBA-2 hits.

## Out of scope

1. **IntraMap port.** The 1 skipped test (`intramap_inverse_cancel`)
   requires implementing IntraMap (`reference/starlink-ast/src/intramap.c`),
   which involves a Fortran-callable transform registry and FFI for legacy
   user code. This is a multi-week task on its own. We will **remove the
   `intramap_inverse_cancel` entry from `simplify_tests.txt`** with a
   `# TODO(intramap): re-enable when IntraMap is ported` comment, so the
   skipped count drops to 0 without coupling this work to a much larger
   feature. When IntraMap lands, the entry is one line to restore.
2. **Aggregate (3+) region merges.** Some C fixtures construct three or more
   regions; this spec only covers the 2-region parallel-merge case the
   failing fixtures exercise. Extension is straightforward but unneeded
   here.
3. **Polygon / Ellipse / Circle parallel merges.** None of the failing
   fixtures exercise these, and their merge semantics differ from
   Box/Interval.

## Test strategy

- Each Group lands as its own commit (`fix(simplify): ...` / `fix(matrix):
  ...` / `fix(regions): ...`) so bisect stays useful.
- After each commit, the simplify-regression suite is re-run; the
  per-failure list in this spec serves as the acceptance checklist.
- Existing passing tests must not regress (`cargo test -p ast-core` —
  full tier-1 suite).
- `cargo fmt` + `cargo clippy --workspace -- -D warnings` before each
  commit (per project memory `feedback_format_clippy.md`).

## Acceptance criteria

```
cargo test -p ast-core --test test_simplify_regression
# →
# Simplify regression: 247 passed, 0 skipped,
#   0 equal() failures, 0 transform failures, 0 serialization failures, 0 other failures
```

(247 = 246 + the previously-skipped intramap entry, removed from the
manifest per §4.)

## References

- C source of truth: `reference/starlink-ast/src/cmpmap.c` (`MapMerge`,
  `Simplify`, `SwapPermMap`), `reference/starlink-ast/src/box.c`
  (`MapMerge`), `reference/starlink-ast/src/interval.c` (`MapMerge`,
  `Simplify`), `reference/starlink-ast/src/matrixmap.c` (form switching),
  `reference/starlink-ast/src/grismmap.c` (`MapMerge` with ZoomMap).
- Rust starting point: `crates/ast-core/src/mappings/simplify.rs:181` (`flatten`),
  `:246` (`merge_loop`), `:1563` (`try_swap_permmap_parallel`),
  `:1882` (`try_merge_zoom_before_inv_grism`).
- Test harness: `crates/ast-core/tests/test_simplify_regression.rs`.
- Fixture manifest: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`.
- Prior simplify-fix design: `docs/superpower/specs/2026-05-07-simplification-fixes-design.md`
  (this spec is its successor; the 2026-05-07 work landed the bulk of
  simplification, and we now close the residual edge cases).

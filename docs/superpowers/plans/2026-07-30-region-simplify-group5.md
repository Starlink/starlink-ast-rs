# Group 5 — Region-simplification port (close the final 4 `equal()` failures)

## Context

DM-54793 drives the pure-Rust AST port to byte-exact serialization parity with
the C library on the simplify-fixture corpus.
After the IsSimp tag-placement quirk was accepted as `astequal`-only (commit
`36f0863`), the simplify regression test reports **0 serialization failures**
and **4 remaining `equal()` failures**:

| Fixture | Rust produces today | C reference | Mechanism |
|---|---|---|---|
| `cap_slamap_02`   | `Box`       | `Prism`      | Box splits into a Prism of two sub-regions |
| `cap_interval_01` | `Prism`     | `Interval`   | nested Prism collapses to one Interval |
| `cap_pointlist_02`| `Prism`     | `PointList`  | Prism collapses via Interval+PointList merge |
| `cap_slamap_07`   | `CmpRegion` | `NullRegion` | AND of disjoint regions is empty |

Transform math is already correct in all four (0 transform failures); only the
simplified *shape* differs. These are the previously-deferred **Group 5** in
`docs/superpowers/plans/2026-07-15-simplify-remaining-fixtures.md` (§ "GROUP 5
— Region class-conversion"). The Region-level `try_merge`, `overlap`,
`is_bounded`, `negate`, and `map_region` primitives are all already ported and
exercised by the region fixtures; the gap is that the Region `simplified()`
methods are **stubs** that never invoke that machinery, plus two missing C
primitives (`PickAxes`-returning-a-Region and per-class `RegBasePick`).

This effort splits into two genuinely independent sub-projects, landed as
separate commits. The design constraint throughout: **mirror the C control flow
directly** (per project CLAUDE.md and the recorded feedback memories); do not
substitute heuristics or compute-then-check shortcuts for C branches.

---

## Sub-project A — Prism / ConvertToPrism path (3 fixtures)

Closes `cap_slamap_02`, `cap_interval_01`, `cap_pointlist_02`.

### A0. Region-returning `PickAxes` + per-class `RegBasePick` (blocking dependency)

C `region.c::PickAxes` (region.c:7579) overrides the Frame-level pick: it picks
the axes from the current Frame, then — if the current→base Mapping splits
cleanly on those axes (`astMapSplit`) — calls `astRegBasePick` to build a
same-class Region over the corresponding **base** axes, remaps it into the
picked current Frame via the inverted split Mapping (`astMapRegion`), and
overlays region properties (`astRegOverlay`). Otherwise it returns a plain
Frame.

> **Name-collision warning:** Rust already has `RegionData::reg_base_pick(point)
> -> bool` (`regions/data.rs:198`) — a *point-in-region test*, unrelated to C's
> `astRegBasePick`. Do **not** overload it. Introduce a distinct trait method,
> e.g. `Region::reg_base_pick_axes(&self, axes: &[usize]) -> Option<DynRegion>`.

Port per-class `RegBasePick` (returns `None` when a valid sub-region cannot be
formed):
- `Box` (box.c:2655): pick the selected axes from base Frame, slice the
  center/corner-defining points to those axes, build a new `BoxRegion`.
- `Interval` (interval.c:2328): slice `lbnd`/`ubnd` to the selected axes.
- `PointList` (pointlist.c:1738): slice each point's coordinates.
- `NullRegion` (nullregion.c:1106): rebuild a `NullRegion` on the picked Frame.
- Base `Region::RegBasePick` (region.c:8277 / :13679) default → `None` (used by
  classes that cannot split, e.g. `Circle`, `Ellipse`, `Polygon`, `CmpRegion`).

Add the current-Frame `PickAxes` override on the `Region` trait (a shared
helper in `regions/mod.rs` or `regions/data.rs`), reusing the existing
`map_split` (`mappings/mod.rs:274`), frame `pick_axes_snapshot`
(`frames/basic.rs:898`), `map_region` (per class), and a `reg_overlay`
copy of region attributes.

### A1. Real `Prism::simplified` (prism.c:3204)

Replace the stub at `regions/prism.rs:638`. Mirror C exactly:

1. `GetRegions` → clone `region1`, `region2`, read `negated` (prism.c:1137 —
   trivial in Rust; the fields are stored directly).
2. Build a **parallel** `CmpMap(reg1, reg2)` (a Region is a Mapping), call
   `.simplify()`. This drives the already-ported Region `try_merge` methods via
   the parallel merge loop (`mappings/simplify.rs:1078`).
3. If the simplified map **is a Region**, use it. Else if it is a **parallel
   CmpMap** whose two components are both Regions and differ from the originals,
   form a new `Prism` from them.
4. If a new Region was produced: `map_region` it into the Prism's current Frame
   and `.simplify()` again.
5. Else fall back to the `astMapSplit`-based path (prism.c:3355-3489): split the
   base→current Mapping on each component's axes; if one component covers all
   current axes and the other none, drop the empty one (`pick_axes` +
   `map_region` + `simplify` + `perm_axes`); if both split cleanly, remap and
   re-Prism only when a component actually simplified; else clone the input.
6. Carry uncertainty forward; invoke the parent Region simplify to simplify the
   encapsulated FrameSet; `reg_overlay` original attributes onto the result.

### A2. Wire `ConvertToPrism` into `Box::simplified` (box.c:4036, prism.c:232)

After the existing Box remap logic (`regions/box_region.rs:681`) fails to
produce a simpler region, call a new `convert_to_prism(&self) -> AstResult<
Option<DynRegion>>` mirroring `astConvertToPrism_` (prism.c:232):

- Loop `bitmask` over all `1..2^nout` current-axis subsets.
- `pick_axes(subset)` and `pick_axes(complement)`; proceed only when **both**
  yield Regions (via A0).
- `simplify` each half; form a `Prism(sp1, sp2)` **only when at least one half
  actually simplified** (`sp1 != pickfrm1 || sp2 != pickfrm2`) — this exact
  guard is what stops over-collapsing the passing `cap_pointlist_01` fixture.
- Permute the Prism's axes back to original order (prism.c:364-378), wrap via
  `map_region(prism, UnitMap, current_frame)`, `simplify`, and return it.

Return `None` when no subset produces a Prism; `Box::simplified` then keeps its
current result.

### A verification
```
cargo test -p starlink-ast --test test_simplify_regression simplify_regression -- --nocapture 2>&1 | grep "Simplify regression:"
```
Expect `cap_slamap_02`, `cap_interval_01`, `cap_pointlist_02` to drop off the
`equal()` failure list, `cap_pointlist_01` (and all other region fixtures) still
passing, 0 transform / 0 serialization failures.

---

## Sub-project B — CmpRegion overlap / NullRegion path (1 fixture)

Closes `cap_slamap_07`. Independent of A.

Replace the `CmpRegion::simplified` stub (`regions/cmpregion.rs:732`) with a
faithful port of `cmpregion.c::Simplify` (cmpregion.c:3517):

1. Parent Region simplify of the FrameSet (`newc`).
2. `GetRegions` → `reg1`, `reg2`, `oper`, `neg1`, `neg2`; apply negation to a
   component whose stored `Negated` differs from the required value (via
   `negated_copy`).
3. `simplify` each component.
4. **NullRegion exclusion** (cmpregion.c:3644): if either simplified component
   is a `NullRegion`, replace the CmpRegion per the null/infinite × AND/OR table
   (`astNullRegion(other)` / `copy(other)` / negated variants).
5. **Overlap reduction** (cmpregion.c:3695): `overlap(sreg1, sreg2)` — already
   ported per class — then apply the C decision table (no-overlap+AND → Null;
   negation+OR → infinite Null; identical → one component; containment → the
   containing/contained component per operator; else rebuild the CmpRegion only
   if a component simplified).
6. If `newb` was produced in the base Frame, `map_region` it into the current
   Frame; else clone `newc`. `reg_overlay` and return.

All required primitives exist: `overlap` (per class), `is_bounded`
(`cmpregion.rs:497` mirrors C `GetBounded`), `negated_copy`, `NullRegionBuilder`
(`nullregion.rs:35`), `map_region`.

### B verification
```
cargo test -p starlink-ast --test test_simplify_regression simplify_regression -- --nocapture 2>&1 | grep "Simplify regression:"
```
Expect `cap_slamap_07` to drop off; 0 `equal()` failures overall.

---

## Files to modify

- `crates/starlink-ast/src/regions/mod.rs` — add `reg_base_pick_axes` + region
  `pick_axes` to the `Region` trait (default `None`); `reg_overlay` helper.
- `crates/starlink-ast/src/regions/box_region.rs` — `reg_base_pick_axes`;
  `convert_to_prism`; wire into `simplified`.
- `crates/starlink-ast/src/regions/interval.rs`,
  `crates/starlink-ast/src/regions/pointlist.rs`,
  `crates/starlink-ast/src/regions/nullregion.rs` — `reg_base_pick_axes` overrides.
- `crates/starlink-ast/src/regions/prism.rs` — real `simplified` (A1).
- `crates/starlink-ast/src/regions/cmpregion.rs` — real `simplified` (B).
- `crates/starlink-ast/src/regions/data.rs` — shared helpers if warranted.
- Possibly `crates/starlink-ast/src/regions/circle.rs` / `ellipse.rs` / `polygon.rs`
  only if the default trait method needs an explicit `None` override.

No `.rs` file carries a C-style prologue history block; the vendored C in
`reference/` is read-only reference and is not edited.

## C reference map

- `astConvertToPrism_` — reference/starlink-ast/src/prism.c:232
- Prism `Simplify` — prism.c:3204; `GetRegions` — prism.c:1137
- Region `PickAxes` (override) — region.c:7579; base `RegBasePick` —
  region.c:8277 / :13679
- Per-class `RegBasePick` — box.c:2655, interval.c:2328, pointlist.c:1738,
  nullregion.c:1106
- Box `Simplify` (ConvertToPrism call site) — box.c:4036
- CmpRegion `Simplify` — cmpregion.c:3517; `GetBounded` — cmpregion.c:703

## Full regression gate (run after each sub-project commit)

```
cargo test -p starlink-ast --test test_simplify_regression simplify_regression -- --nocapture 2>&1 | grep "Simplify regression:"
cargo test -p starlink-ast --test test_serialisation 2>&1 | grep -iE "byte-exact|test result"
cargo test -p starlink-ast --lib 2>&1 | grep "test result"
cargo test -p starlink-ast --test test_wcsconv 2>&1 | grep "test result"
cargo test -p starlink-ast --test test_frameset 2>&1 | grep "test result"
cargo test -p starlink-ast --test test_regions 2>&1 | grep "test result"
CARGO_TARGET_DIR=/lscratch/timj/starlink-astrs-clippy cargo clippy -p starlink-ast --lib 2>&1 | grep -E "warning|error"   # must be empty
cargo fmt -p starlink-ast
```

Success criteria:
- **0** `equal()` failures, **0** transform failures, **0** serialization
  failures on the simplify fixtures.
- No regression in: byte-exact round-trip corpus (still 649/677, the pre-existing
  red gate untouched — verify the 4 targets are not newly in its failure set),
  lib 999, wcsconv 192, frameset 39, `test_regions`.
- `cargo fmt` / `clippy` clean.

## Risks / watch-items

- **Over-collapse guard (A2):** `cap_pointlist_01` currently passes with C
  keeping the outer Prism. The `sp1 != pickfrm1 || sp2 != pickfrm2` condition is
  load-bearing; model it exactly, do not convert unconditionally.
- **M1 WinMap-narrow interaction:** the memory note
  `project_m1_winmap_narrow_blocker` records a WinMap→ShiftMap final-narrow that
  corrupted `cap_cmpmap_02`; the Prism path re-simplifies inner mappings, so
  re-run the full simplify corpus, not just the 4 targets.
- **Recursion:** `Prism::simplified` builds a CmpMap of Regions and re-simplifies;
  ensure the existing simplify recursion guard covers the region path and add a
  focused inline test if the region merge is otherwise untested end-to-end.
- **IsSimp / owned-value quirk:** unrelated to this path, but if any new region
  serialization diff appears that is tag-placement-only, treat it under the
  established `skip_string_compare` policy (see `project_issimp_flag_collapse_rejected`).

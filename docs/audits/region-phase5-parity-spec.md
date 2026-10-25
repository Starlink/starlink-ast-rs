# Phase 5 — Region cross-cutting parity: port spec + required C fixtures

Ticket DM-54793.
Status as of 2026-08-02: **all 8 gaps CLOSED.** Verified against HEAD
(commit `a96bc80`).

| Gap | Verdict | Landed as |
|---|---|---|
| R-UNC | CLOSED | shared `map_region` refactor (deep-copies uncertainty) |
| PR-MAP | CLOSED | shared `map_region` refactor (C Prism has no override either) |
| RB-PICK | CLOSED | `f62a41f` |
| MERGE-FF | CLOSED | `e4898b1` |
| SETUNC | CLOSED | `d5e78b6` |
| DEFUNC | CLOSED | `c608570` |
| SIMP-STAB | CLOSED | `24ce72e` |
| CE-FIT | CLOSED | `a96bc80` |

## Why this document exists

The 4 critical Group 5 `equal()` failures were closed first.
The remaining Phase 5 items were faithful-port gaps in region machinery that
**no fixture exercised** — which is exactly why they went unnoticed.

Per the project principle "lack of test coverage is not a reason to skip a C
branch" (`feedback_test_coverage_is_separate_from_porting`), the response to
missing coverage is *more fixtures*, not less code.
But we cannot validate a port against fixtures that do not yet exist.

So this document does two things for each gap:

1. Records the exact faithful-port work (Rust target, C reference, control flow).
2. Specifies the C-side fixture(s) that must be generated to drive and validate
   the port, in the form of concrete additions to
   `reference/starlink-ast/ast_tester/gen_simplify_fixtures.c`.

## Fixture pipeline (how these get created)

Fixtures are generated on the C side and replayed on the Rust side:

1. Add a block to `gen_region_fixtures()` (or a new `gen_region_unc_fixtures()`)
   in `reference/starlink-ast/ast_tester/gen_simplify_fixtures.c`.
   Each block constructs an object via the **public C API**, optionally applies
   `astMapRegion` / attribute setters, then calls `write_fixture(dir, name, map)`
   — which serializes the object to `<name>.map`, runs `astSimplify`, and
   serializes the result to `<name>.simp`.
2. Build and run from the submodule:
   `cmake --build build-dev --target gen_simplify_fixtures`
   then run it from `ast_tester/` so output lands in
   `ast_tester/simplify_fixtures/`.
3. Copy the new `.map` / `.simp` pair into
   `crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures/`
   (fixtures must be committed — tier-1 runs with the submodule absent).
4. `test_simplify_regression.rs` auto-discovers the pair and replays it:
   read `.map`, `astSimplify`, compare structure (`equal()`) and serialization
   (byte-exact) against `.simp`.

Note: `write_fixture` only captures the `astSimplify` transform. For gaps that
are **not** simplify-driven (SETUNC, DEFUNC, GetUncFrm), a plain round-trip
fixture (serialize an object that *carries* an explicit uncertainty, so the
`Unc` node exercises the read/write path) is the right vehicle; those go through
the native round-trip corpus (`test_serialisation.rs`) rather than the simplify
harness. Both mechanisms are noted per gap below.

---

## Gap RB-PICK (small) — base `reg_base_pick_axes` + CmpRegion/Prism overrides

### Verdict
CLOSED (`f62a41f`). Base copy-on-permutation path plus CmpRegion/Prism overrides
landed; the mod.rs doc comment was corrected. Two distinct sub-gaps shared one
root.

### Rust target
- `regions/mod.rs:614` — base default returns `Ok(None)` unconditionally.
- `regions/cmpregion.rs`, `regions/prism.rs` — no `reg_base_pick_axes` override.
- `regions/mod.rs:611-613` doc comment wrongly lists CmpRegion/Prism as using
  the default.

### C reference
- Base `RegBasePick` — `region.c:8346-8392`: get base-Frame naxes; proceed only
  when `nax == naxes` (ALL base axes selected); verify `axes` is a genuine
  permutation (each index once); if so `result = astCopy(this)`,
  `astSetRegFS(result, fr)`, and `astPermAxes(result, axes)` when not identity.
  Returns NULL only for a strict subset.
- CmpRegion `RegBasePick` — `cmpregion.c:2096`: pick the same base axes from each
  sub-region; if both yield Regions, build a `CmpRegion` with the same `oper`.
- Prism `RegBasePick` — `prism.c:2293`: partition `axes` at `nax1` between the
  two sub-regions, pick from each, return the single sub-region clone or an
  `astPrism` of both.

### Why it is a genuine divergence (not just uncovered)
`pick_axes_region` (mod.rs:693) turns a `None` from `reg_base_pick_axes` into a
plain `Frame`, losing the Region type. C `PickAxes` (region.c:7717-7731) uses the
copy from `astRegBasePick` to build a Region. So `astPickAxes` on a
Circle/Ellipse/Polygon selecting all axes in permuted order yields a Region in C
but only a Frame in Rust — and subset-axis picks on a CmpRegion/Prism yield a
Region in C but `None` in Rust.

### Faithful-port sketch
1. Base mod.rs:614: port region.c:8346-8392 — if `axes.len() == base_naxes` and
   `axes` is a permutation of `0..base_naxes`, return `Some(self.clone_region())`
   with FrameSet reset to the base Frame (`astSetRegFS` analogue) and
   `reg_perm_axes(axes)` when not the identity; else `Ok(None)`.
2. CmpRegion override (port cmpregion.c:2096): `pick_axes_region` each component
   on the same base axes; if both are Regions build a `CmpRegion` with the same
   operator; else `None`.
3. Prism override (port prism.c:2293): split `axes` at `nax1`; pick from each
   component; return the single clone or an `astPrism` of both.
4. Fix the mod.rs:611-613 doc comment.

### Required C fixtures
Add to `gen_region_fixtures()` in `gen_simplify_fixtures.c` — these drive
`astPickAxes` indirectly through simplify only if a simplify path calls it, so
the more direct validation is a dedicated pick-axes round-trip. Two fixtures:

- `regbasepick_circle_permaxes` — a 2-D Circle whose current→base Mapping is a
  clean axis swap (PermMap), `astMapRegion`'d into a swapped Frame, so
  `astSimplify` triggers the all-axes-permuted `RegBasePick` copy path. Expect
  `.simp` to remain a Circle (Region type preserved), not degrade to a Frame.
- `regbasepick_cmpregion_subset` — an AND CmpRegion of a 2-D Box (axes 0,1) and
  a 1-D Interval (axis 2) in a 3-D Frame, with a base→current Mapping that
  splits cleanly on {0,1} vs {2}; `astSimplify` should exercise the CmpRegion
  `RegBasePick` subset path and keep a CmpRegion of the picked sub-regions.

Because these validate a type-preservation property that `astSimplify` may or
may not surface for a given construction, ALSO add a direct Rust unit test in
`crates/starlink-ast/tests/test_regions.rs` calling `pick_axes` on a permuted-axis
Circle and asserting the result downcasts to a Region — the fixture confirms C's
answer, the unit test asserts the Rust API contract.

---

## Gap DEFUNC (medium) — per-class `get_def_unc` overrides

### Verdict
CLOSED (`c608570`). The four class overrides (Interval, NullRegion, Prism,
CmpRegion) were ported on top of the faithful base `get_def_unc` port, along with
the RegBaseBox2 distinction. Previously PARTIAL: the base arithmetic
(mod.rs:303-322) mirrored C base `GetDefUnc` (region.c:3492-3592) but every
concrete class incorrectly fell through to the base.

### C reference (all four bodies read in full)
- **Interval** — `interval.c:583-703`: if the Interval caches to an equivalent
  Box, `result = astGetDefUnc(box)`. Otherwise build the 1e-6 box from the
  interval's **own** `this->lbnd/ubnd` (per-axis: if both limits finite use
  `hw = |0.5e-6*(ub-lb)|`, `c = 0.5*(ub+lb)`, `hw==0 → hw=c*0.5e-6`; if either
  limit is `±DBL_MAX` use `0.0/0.0`). Note: uses the interval limits, NOT the
  region bounding box the base method reads.
- **NullRegion** — `nullregion.c:170-235`: returns a **zero-radius Circle**
  centered at the origin (`astCircle(this, 1, cen={0..}, &rad=0, ...)`), not a Box.
- **Prism** — `prism.c:1212-1280`: `astPrism(astGetUncFrm(region1, CURRENT),
  astGetUncFrm(region2, CURRENT))` — a Prism of the two component current-Frame
  uncertainties (the components' current Frames == the Prism's base Frame).
- **CmpRegion** — `cmpregion.c:1090-1160`: if `astTestUnc(region1)` →
  `astGetUncFrm(region1, CURRENT)`; else if `astTestUnc(region2)` →
  `astGetUncFrm(region2, CURRENT)`; else defer to the parent (base) method.

### Base-method subtlety (region.c:3554-3562)
C base uses `astRegBaseBox2`, NOT `astRegBaseBox`, so compound regions propagate
a "not inverted" assumption into components. Rust base calls
`get_region_bounds()` (the `RegBaseBox` analogue). This only bites classes that
actually reach the base method; with the four overrides added, the base is
reached only by Box/Circle/Ellipse/Polygon (leaf, non-compound), where
`RegBaseBox` and `RegBaseBox2` coincide — so no separate `RegBaseBox2` port is
needed once the overrides land.

### Missing Rust primitives (add first)
- **`test_unc` predicate** — C `astTestUnc` = "an explicit (non-default)
  uncertainty is set". Rust equivalent is `self.data.uncertainty.is_some()`
  (data.rs:61); expose it as a trait method `fn test_unc(&self) -> bool` so the
  CmpRegion override reads cleanly.
- **Zero-radius base-Frame Circle** — NullRegion needs `CircleBuilder::new(bfrm,
  vec![0.0; n]).radius(0.0).build()`. Confirm the builder accepts radius 0
  (circle.rs:74 `radius`); the base-Frame snapshot is `get_region_frameset()
  .base().snapshot()`.
- **Interval Box-equivalence** — the C `Cache(this)` "is this Interval a Box?"
  test. Rust has the reclassification logic inside `Interval::simplified`
  (interval.rs:137-289) but no standalone predicate; factor out an
  `equivalent_box(&self) -> Option<BoxRegion>` helper for the Interval override
  to call.

### Faithful-port sketch
Add `fn get_def_unc` overrides to Interval, NullRegion, Prism, CmpRegion mirroring
the four C bodies above. Prism/CmpRegion reuse `get_unc_frm(Current)` (mod.rs:331)
which already exists.

### Required C fixtures
DEFUNC is not surfaced by `astSimplify` alone — it is exercised when an
uncertainty is *needed* but none was set (e.g. overlap/mask on a region with a
default uncertainty). The validating fixtures are round-trip objects that force a
default-uncertainty computation, checked via `test_serialisation.rs`:

- `defunc_interval_unbounded` — a 2-D Interval with one bounded axis and one
  half-open axis (`ubnd[1] = DBL_MAX`); serialize with no explicit `Unc`. The
  Rust-vs-C check is on the default-uncertainty Box dimensions (bounded axis →
  1e-6 half-width, open axis → 0.0). Requires a test hook that calls
  `astGetUncFrm(reg, AST__BASE)` and serializes the result (add a small C probe
  `gen_defunc_fixtures` that writes `astGetUncFrm` output, since `write_fixture`
  only captures `astSimplify`).
- `defunc_nullregion_2d` — a 2-D NullRegion; probe `astGetUncFrm` and confirm the
  default uncertainty is a zero-radius Circle, not a Box.
- `defunc_prism_box_interval` — a Prism(Box 2-D, Interval 1-D); probe
  `astGetUncFrm` and confirm the default is a Prism of the two component default
  uncertainties.
- `defunc_cmpregion_component_unc` — an AND CmpRegion where `region1` carries an
  explicit uncertainty; probe `astGetUncFrm` and confirm it equals region1's
  uncertainty (not the base bounding-box box).

These four require a new C probe target (`gen_defunc_fixtures.c` or a block in the
existing generator that serializes `astGetUncFrm` results) because the default
uncertainty is otherwise internal. Document the probe alongside the fixture.

---

## Gap SETUNC (medium) — `set_uncertainty` full remap/validate/recenter body

### Verdict
CLOSED (`d5e78b6`). The full C `SetUnc` body (validate/remap/negate-to-bound/
RegionFS/recenter/cache) landed as a shared default trait method, replacing the
9 bare `self.data.uncertainty = Some(unc)` stores. Previously OPEN.

### C reference — `region.c:10761-10918`, read in full
In order:
1. Annul existing uncertainty; set `changed` if one was present (10826-10832).
2. Validate class: `cen0 = astRegCentre(unc, NULL, NULL, 0, CURRENT)`; NULL
   means the class is not centro-symmetric → error `AST__BADIN`/`AST__NCPIN`
   "must be a centro-symetric subclass" (10836, 10899-10904).
3. Remap `unc` into this Region's **base** Frame: invert `this->frameset`,
   `fs2 = Conv(unc->frameset, fs)`, re-invert; `map = astGetMapping(fs2, BASE,
   CURRENT)`, `frm = astGetFrame(fs2, CURRENT)`, `this->unc = astMapRegion(unc,
   map, frm)` (10840-10850). This is the deep-copy-and-transform that satisfies
   the documented "a deep copy is taken" contract.
4. Error `AST__BADIN`/`AST__NCPIN` if the Frames are not convertible (10889-96).
5. Ensure bounded: `if(!astGetBounded(this->unc)) astNegate(this->unc)` (10856).
6. RegionFS optimization: `smap = astSimplify(astGetMapping(unc->frameset, BASE,
   CURRENT))`; if `astIsAUnitMap(smap)` → `astSetRegionFS(this->unc, 0)` so the
   FrameSet is omitted from dumps (10862-10869).
7. Re-center `unc` at the Region's first defining point:
   `if(this->points) astRegCentre(this->unc, NULL, ptr_reg, 0, CURRENT)`
   (10871-10876).
8. `if(changed) astResetCache(this)` (10907-10909).

### Missing Rust primitives (add first)
- **`reg_centre` query form** — C `astRegCentre(unc, NULL, NULL, 0, CURRENT)`
  returns the center (or NULL for a non-centro-symmetric class). Rust only has
  `reg_centre_set` (mod.rs:411). Add `fn reg_centre(&self) -> Option<Vec<f64>>`
  (returns `None` for classes that are not centro-symmetric — the same set that
  can't be an uncertainty: PointList, Polygon, CmpRegion, Prism, NullRegion
  return `None`; Box/Circle/Ellipse/Interval return their center).
- Everything else exists: `convert_to` (Conv analogue, frames/mod.rs:190),
  `map_region` (mod.rs:440), `is_bounded`/`negate` (mod.rs:197/120),
  `set_region_fs`/`dummy_fs` (data.rs:134/70 — the RegionFS-omission flag),
  `invalidate_cache` (data.rs:154 — the `astResetCache` analogue).

### Faithful-port sketch
Port region.c:10761-10918 into a **shared** default trait method
`set_uncertainty` on `Region` (the C body is fully generic — it touches only
`frameset`, `unc`, `points`, cache). Each class currently has an identical bare
store; replace them with one default and delete the per-class copies. Steps map
1:1 to the C list above; use the existing primitives plus the new `reg_centre`
query.

### Required C fixtures
SETUNC is a read/write-path gap, validated by round-trip through
`test_serialisation.rs`, not `astSimplify`. Add to `gen_simplify_fixtures.c` (or
a round-trip generator) objects that carry an explicit uncertainty supplied in a
DIFFERENT Frame than the region's base, so the remap/recenter path fires:

- `setunc_box_remapped_unc` — a 2-D Box in a base Frame, given an uncertainty Box
  defined in a zoomed/shifted Frame via `astSetUnc`; serialize. The Rust-vs-C
  check confirms the stored uncertainty is remapped into the region's base Frame
  and re-centered at the box center (the `Unc` node geometry must match C
  byte-for-byte).
- `setunc_unbounded_negated` — supply an uncertainty that is unbounded as given
  (e.g. a negated Box); confirm C's "negate to make bounded" step (10856) is
  reproduced — the serialized `Unc` is the bounded (negated) form.
- `setunc_unitmap_regionfs_omitted` — supply an uncertainty whose base→current
  Mapping simplifies to a UnitMap; confirm the `RegionFS` flag is cleared so the
  uncertainty's FrameSet is omitted from the dump (serialized `Unc` node has no
  embedded FrameSet). This directly exercises step 6.

All three are constructed with `astSetUnc(region, unc)` then `write_fixture`
(the `.map` serialization alone validates SETUNC; the `.simp` is a bonus). The
existing corpus has NO uncertainty-carrying fixtures, so these are the first to
exercise the `Unc` read/write path under a non-trivial Frame relationship.

---

## Gap SIMP-STAB (medium) — parent-Region Simplify uncertainty stability check

### Verdict
CLOSED (`24ce72e`). The shared parent-Region simplify helper was factored out and
the 0.1%-tolerance uncertainty-stability check ported into it. Previously OPEN:
the check was absent from every Rust `simplified()` and each class re-implemented
the base→current-map simplify inline (e.g. box_region.rs:1219-1227) without
touching the uncertainty Region.

### C reference — `region.c:11049-11335`, stability block read in full (11145-11312)
The base `Region::Simplify` runs BEFORE each class's own simplify work (invoked
as `parent_simplify`). It:
1. Deep-copies the Region; simplifies the encapsulated FrameSet; notes if it
   changed (11128-11142).
2. If the Region has **default** uncertainty, deletes it (regenerated on demand)
   (11148).
3. If `astTestUnc(new)` (explicit uncertainty):
   a. `unc = astGetUncFrm(new, BASE)`; get `map = base→current` of `this`.
   b. If `nin != nout` (PermMap slice): re-center `unc` by transforming its
      current center forward-then-back through `map` (11165-11188).
   c. `sunc = astSimplify(unc)`; only if `sunc != unc` proceed to stability test.
   d. Stability test: pick a test box (bounding box of `this` if bounded, else
      100× the uncertainty size centered on `orig_cen`); re-center `sunc` at the
      test box's lower corner, get its bounding-box widths `w1`; re-center at the
      upper corner, get widths `w2`; per axis, if `|w1-w2| > 0.005*(|w1|+|w2|)`
      set `ok=0` and break (11277-11284).
   e. Reinstate `sunc`'s original center; if `ok`, `astSetUnc(new, sunc)` and
      mark `simpler=1` (11303-11306).

### Structural prerequisite
Factor the inline base→current-map simplify (currently duplicated in all 9
`simplified()` methods) into ONE shared helper —
`regions::data::parent_region_simplify(&dyn Region) -> AstResult<Option<
region-with-simplified-FrameSet-and-uncertainty>>` — mirroring C's
`parent_simplify`. Each class's `simplified()` calls it first, then does its own
class-specific geometry work on the result. This also benefits CE-FIT (below),
which C structures the same way.

### Primitives
- `reg_centre` get + set (`reg_centre_set` exists mod.rs:411; the query form is
  the same one SETUNC needs — build SETUNC first or land them together).
- `get_region_bounds` / `reg_base_box` (present), `is_bounded` (present),
  `test_unc` (add per DEFUNC), `get_unc_frm(Base)` (present, mod.rs:331).
- `set_uncertainty` (the real SETUNC port — SIMP-STAB calls `astSetUnc(new,
  sunc)`, so SETUNC must land first or concurrently).

### Dependency note
SIMP-STAB **depends on** SETUNC (step 3e calls `astSetUnc`) and on the
`reg_centre` query + `test_unc` primitives from DEFUNC/SETUNC. Land order:
DEFUNC/SETUNC primitives → SETUNC → SIMP-STAB.

### Required C fixtures
Add to `gen_region_fixtures()`:
- `simpstab_box_linear_unc` — a Box carrying an explicit uncertainty Box, mapped
  through a linear (Zoom+Shift) Mapping so the uncertainty simplifies AND is
  stable across the test box. Expect `.simp` to carry the simplified uncertainty
  (stability test passes, `ok=1`).
- `simpstab_box_nonlinear_unc` — the same Box+uncertainty mapped through a
  Mapping whose linearization varies across `this` (e.g. a mildly non-linear
  PolyMap or a SphMap-like transform) so the uncertainty width changes >0.1%
  corner-to-corner. Expect `.simp` to KEEP the un-simplified uncertainty
  (stability test fails, `ok=0`) — this is the branch that distinguishes a
  faithful port from a naive "always simplify the uncertainty".
- `simpstab_permmap_slice_unc` — a Region whose base→current Mapping is a PermMap
  with `nin != nout` (a slice), carrying an explicit uncertainty; exercises the
  re-center-on-slice branch (11165-11188).

The non-linear fixture is the load-bearing one: without it a port that skips the
stability test would still pass the linear and slice fixtures.

---

## Gap MERGE-FF (medium) — FillFactor / MeshSize scaling in parallel merges

### Verdict
CLOSED (`e4898b1`). The FillFactor (product) and MeshSize (product-of-ratios ×
default) scaling landed in the merge helpers, backed by new `mesh_size_set` /
`fill_factor_set` tracking flags that also drive the serialization emit guards.
Previously OPEN: the merge helpers copied Negated/Closed/Adaptive/uncertainty
but omitted the scaling.

### Rust target
- `merge_util.rs::finish_region_merge` (merge_util.rs:43-81), `apply_attrs`
  (51-59): sets closed/closed_set/adaptive/negated/uncertainty only. The merged
  `RegionData` keeps constructor defaults (`mesh_size: 200`, `fill_factor: 1.0`,
  data.rs:117-118). Used by box_region.rs (merge) and interval.rs (merge).
- `pointlist.rs::region_merge` and `nullregion.rs::region_merge`: same omission.

### C reference — `box.c:2016-2052` (MergeBox), identical in interval.c
After `astMapRegion(new, bcmap, cfrm)` produces `result`:
- FillFactor: `if(astTestFillFactor(reg) || astTestFillFactor(this))
  astSetFillFactor(result, astGetFillFactor(reg)*astGetFillFactor(this))`.
- MeshSize: if either is set, compute `fac_this = msz_this/default_msz(this)`
  and `fac_reg = msz_reg/default_msz(reg)` (default obtained by temporarily
  clearing), then `astSetMeshSize(result, fac_this*fac_reg*
  astGetMeshSize(result))`; reinstate the source MeshSize values.

This is deliberately NOT `astRegOverlay`'s plain copy (region.c:9914-9920 fires
only when axis counts match; after a merge `nax = nax_this + nax_reg`, so it
never fires — hence the explicit scaling).

### Prerequisite — set-tracking flags
C's scaling is gated on `astTestFillFactor` / `astTestMeshSize` ("explicitly
set?"). Rust `RegionData` has only `closed_set` (data.rs:49); there is NO
`mesh_size_set` / `fill_factor_set`. Serialization currently detects
"non-default" by VALUE (`mesh_size != 200`, `fill_factor != 1.0`, serial/mod.rs:
554,562), which is NOT equivalent to a set-flag (a caller may explicitly set 200,
or the product may land on a default). Add `mesh_size_set: bool` and
`fill_factor_set: bool`:
1. Add fields to `RegionData` (mirror `closed_set`: Clone data.rs:83-85, `new`
   defaults false data.rs:116-118).
2. Set them in `set_mesh_size` / `set_fill_factor`.
3. Switch serial/mod.rs:573,562 emit-guards from value-comparison to the
   set-flag; read them back in from-node (serial/mod.rs:782-783) — a present
   `Fill`/`MeshSz` node means set=true. **This changes serialization emit
   conditions, so the byte-exact corpus must be re-run and the failure set
   compared before/after (baseline 649/677).**

### Faithful-port sketch
Thread the two source regions' `(mesh_size, mesh_size_set, fill_factor,
fill_factor_set)` into `finish_region_merge` (and the pointlist/nullregion
merges); after building the merged region apply the two C formulas above. The
"default MeshSize" is the constant 200 (data.rs:117), so `fac = msz/200.0`.

### Required C fixtures
Add to `gen_region_fixtures()` — the existing `box_parallel_merge` /
`interval_parallel_merge` fixtures use default MeshSize/FillFactor, so they do
NOT exercise the scaling. Add variants that SET the attributes:
- `merge_box_meshsize_scaled` — two 1-D Boxes, each with an explicit non-default
  `MeshSize` (e.g. `astSetI(b1,"MeshSize",30)`, `astSetI(b2,"MeshSize",50)`),
  merged in parallel. Expect the `.simp` merged 2-D Box to carry
  `MeshSize = (30/200)*(50/200)*200` (the product-of-ratios × default).
- `merge_box_fillfactor_product` — two Boxes with explicit `FillFactor`
  (e.g. 0.5 and 0.8); expect the merged FillFactor = 0.4.
- `merge_interval_meshsize_scaled` — same as the first but with Intervals, to
  cover MergeInterval's identical block.

These are the first fixtures to set MeshSize/FillFactor on a merge input; they
also validate the new `*_set` serialization path.

---

## Gap CE-FIT (large) — Circle/Ellipse Simplify re-fit / validate / degrade

### Verdict
CLOSED. `best_circle` (circle.c:176) and `best_ellipse` (ellipse.c:191) are
ported as free helpers; both `Circle::simplified` and `Ellipse::simplified` now
mesh the current-Frame boundary, re-fit, validate via `reg_pins`, and degrade
class per the C control flow (Circle keeps its `!UnitMap` guard and adopts a
best-circle fit without setting `simpler`; Ellipse always attempts and degrades
to Circle first). Porting also revealed that `Ellipse::reg_base_mesh` used a
single-shot rotation formula with a from-+X angle convention; C generates the
mesh with two sequential `astOffset2` calls (move `dx` along the primary axis,
then `dy` perpendicular) using the from-+Y `CalcPars` angle, so the mesh was
rewritten to mirror C exactly, restoring byte-exact fitted-ellipse points.

### C reference — `circle.c:1840-1930` (Circle), `ellipse.c:1957+` (Ellipse)
Circle (after `parent_simplify`):
1. `map = base→current`; only if NOT a UnitMap do the re-fit.
2. `mesh = astRegMesh(new)` (current-Frame boundary mesh); `unc =
   astGetUncFrm(new, CURRENT)`; transform the center into the current Frame.
3. `newreg = astBestCircle(mesh, cen, unc)`; if `astRegPins(newreg, mesh)` (all
   mesh points on the fitted circle within uncertainty) → adopt it.
4. Else if 2-D: `newreg = astBestEllipse(mesh, cen, unc)`; if `astRegPins` →
   adopt the Ellipse (Circle **degrades to Ellipse**).
5. `if simpler { astRegOverlay(new, this, 1); }` else clone.

Ellipse `Simplify` has the same structure but always attempts the fit (no
UnitMap guard around it) and tries `astBestCircle` first, degrading
**Ellipse→Circle** (ellipse.c) when the circle fit validates.

### The two fit helpers (unported — `grep best_circle|best_ellipse` is empty)
- `astBestCircle` — **small** (circle.c:176-289): radius = RMS distance of mesh
  points from the supplied center, `rad = sqrt(nc*s2r/n)`; build a Circle at
  `cen` with that radius in the uncertainty's Frame.
- `astBestEllipse` — **medium** (ellipse.c:191-474): find the max-radius mesh
  point (primary axis end) via a quadratic fit through three neighboring
  (radius, angle) samples; resolve all points into parallel/secondary
  components; derive the mean secondary-axis length; build the Ellipse. Pure
  geometry (atan2/sqrt), no matrix solve — tractable but non-trivial.

### Faithful-port sketch
1. Port `astBestCircle` and `astBestEllipse` as free helpers in circle.rs /
   ellipse.rs producing a current-Frame `Circle`/`Ellipse` from (mesh, center,
   uncertainty).
2. Rewrite both `simplified()` to: call the shared parent-simplify (the
   SIMP-STAB helper); compute the current-Frame boundary mesh (`base_mesh`
   → transform to current, infra exists); transform the center; try
   `best_circle` + `reg_pins_mask` (Circle::reg_pins_mask exists circle.rs:358),
   adopt on all-on-boundary; else if `nc==2` try `best_ellipse` +
   `reg_pins_mask` (Ellipse ellipse.rs:323); finish with `reg_overlay`
   (mod.rs:624). Circle keeps its `!UnitMap` guard; Ellipse always attempts and
   can degrade to Circle.

### Dependency note
CE-FIT benefits from the shared parent-simplify helper (SIMP-STAB) — C invokes
`(*parent_simplify)` first in both. Land SIMP-STAB's refactor first, then CE-FIT
reuses it. `reg_pins_mask` is already ported for both classes.

### Required C fixtures
Add to `gen_region_fixtures()`:
- `cefit_circle_conformal` — a Circle mapped through a conformal (isotropic
  Zoom + Shift) Mapping into a new Frame. Expect `.simp` to remain a Circle with
  the re-fitted radius (BestCircle validates via RegPins). Distinguishes the
  re-fit port from the naive transform only if the naive transform would produce
  a *different* radius representation — so pick a mapping where the naive
  center+radius-point transform is subtly wrong (e.g. radius point transforms to
  a slightly off distance) and BestCircle's RMS fit is the correct one.
- `cefit_circle_to_ellipse` — a Circle mapped through an ANISOTROPIC scaling
  (different zoom per axis) so it becomes an ellipse. Expect `.simp` to be an
  **Ellipse** (Circle degrades). The naive stub would wrongly keep a Circle —
  this is the load-bearing fixture.
- `cefit_ellipse_to_circle` — an Ellipse mapped through a Mapping that
  equalizes the axes into a circle. Expect `.simp` to be a **Circle** (Ellipse
  degrades).
- `cefit_circle_nonconformal_keep` — a Circle mapped through a NON-LINEAR
  MathMap so neither BestCircle nor BestEllipse validates via RegPins. A linear
  shear or rotation of a circle is still an ellipse (BestEllipse would validate
  it), so a non-linear map is required to exercise the keep branch. Expect
  `.simp` to KEEP the original Circle (with its live FrameSet) — the "no
  simplification" branch.
- `regbasepick_circle_permaxes` — a Circle mapped through an axis-swapping
  PermMap. The Circle class cannot be axis-sliced, so `.simp` keeps the Circle
  with its live PermMap FrameSet.

The `cefit_circle_to_ellipse` and `cefit_ellipse_to_circle` fixtures are the
ones the current naive stubs would fail; the conformal and non-conformal-keep
fixtures pin the boundaries. As landed, `cefit_circle_conformal` keeps a Circle
carrying its live ZoomMap FrameSet (the best-circle branch adopts the fit but
does NOT set `simpler`, so the returned Region is the original with its
FrameSet, not a collapsed Circle).

---

## Landing order (as executed)

Independent/safe first; uncertainty cluster (interdependent) next; largest last.
All seven steps landed in this order, each its own commit with the full
regression gate (simplify regression + byte-exact corpus failure-set diff +
lib/wcsconv/frameset/test_regions + fmt + clippy) and its C fixtures generated
and committed into
`crates/starlink-ast/tests/fixtures/native/reference/simplify_fixtures/`.

1. **RB-PICK** (`f62a41f`) — base copy-on-permutation + CmpRegion/Prism overrides
   + doc fix. No uncertainty interaction.
2. **MERGE-FF** (`e4898b1`) — added `mesh_size_set`/`fill_factor_set`, changed the
   serialization emit guards (byte-exact corpus re-run, failure set compared).
3. **`reg_centre` query + `test_unc` primitives** — shared prerequisite for the
   next three (landed alongside SETUNC in `d5e78b6`).
4. **SETUNC** (`d5e78b6`) — full remap/validate/recenter/cache body as a shared
   default; deleted the 9 bare stores.
5. **DEFUNC** (`c608570`) — 4 per-class `get_def_unc` overrides (reuses
   `test_unc`, `get_unc_frm`, zero-radius Circle) + RegBaseBox2.
6. **SIMP-STAB** (`24ce72e`) — factored the shared parent-simplify helper; added
   the 0.1% stability check (depends on SETUNC + `reg_centre` query).
7. **CE-FIT** (`a96bc80`) — ported `astBestCircle`/`astBestEllipse`; rewrote
   Circle/Ellipse `simplified()` on the SIMP-STAB parent-simplify helper, and
   fixed the `Ellipse::reg_base_mesh` two-`astOffset2` / from-+Y angle convention.

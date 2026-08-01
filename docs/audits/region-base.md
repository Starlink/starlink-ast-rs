# Region base-class & shared-infrastructure fidelity audit

Scope: the base `Region` class and the shared region infrastructure in the
Rust port. C reference is `reference/starlink-ast/src/region.c`; Rust is
`crates/ast-core/src/regions/{mod.rs,data.rs,overlap.rs,merge_util.rs,masking.rs}`
plus the per-class hooks those files drive.

Status legend:
- **PORTED** — faithful port of the C control flow.
- **STUB** — returns a trivial / empty / default result where C does real work.
- **HEURISTIC** — an approximation that does not mirror C's control flow.
- **MISSING-BRANCH** — the main path is ported but named sub-branches are dropped.
- **ABSENT** — no Rust equivalent exists.

All C line numbers are from `reference/starlink-ast/src/region.c` unless noted.

## 1. Gap matrix

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `RegPins` (region.c:8879) — boundary-membership test with joint uncertainty | `RegionGeometry::reg_pins` (data.rs:191) + `verify_remap_exact` (data.rs:313) | **HEURISTIC** | C `astRegPins` answers "do these points lie *on the boundary* of `this`, to within the joint uncertainty of `this` and `unc`?" The base C method is abstract (errors); each class ports a genuine geometric on-boundary + uncertainty-width test. Rust `reg_pins` is instead an **inside/interior** predicate (Box: `lb<=p<=ub`, box_region.rs:309). It takes no `unc` argument and never tests boundary proximity. `verify_remap_exact` (data.rs:313) reuses that interior predicate to approximate the box.c:3905 boundary round-trip. There is no uncertainty tolerance anywhere. |
| `Overlap` (region.c:6912) → `OverlapX` (region.c:6996) | `compute_overlap` (overlap.rs:40) | **HEURISTIC / MISSING-BRANCH** | C first tries `astEqual` (→5) and negated-equal (→6); does `astConvert` frame alignment; builds a mesh, transforms into the first region's base frame, calls `astRegPins` with the mapped uncertainty (`unc1`, region.c:7202) to detect equivalent boundaries; then uses a submesh + edge test-point logic and a `touch`/`Closed` rule (region.c:7421). Rust instead: NullRegion special-cases, bounding-box quick reject, then counts mesh points inside with a hard-coded 90 % `THRESHOLD` (overlap.rs:24) — an explicit floating-point fudge substituting for C's uncertainty machinery. It never returns `Same-by-negation` (value 6), does no `astConvert` alignment (comment admits "assumes both regions are already in compatible coordinate systems", overlap.rs:37-39), and drops the touch/Closed edge rule. |
| `MapRegion` (region.c:5375) | per-class `map_region` (box_region.rs:664; circle.rs:472; ellipse.rs:436; polygon.rs:664; interval.rs:757; pointlist.rs:489; cmpregion.rs:606; nullregion.rs:241; prism.rs:514) | **MISSING-BRANCH** | Two documented losses. (1) **Uncertainty is dropped**: C takes `astCopy(usethis)` (region.c:5704) which deep-copies the uncertainty Region, then adds the new frame/mapping. Every Rust `map_region` builds a fresh `RegionData` copying negated/closed/mesh_size/fill_factor/adaptive but **never** `data.uncertainty` (e.g. box_region.rs:675-682, circle.rs:486-492). (2) **Axis-reduction path absent**: C's `nnew < nold` block (region.c:5593-5701) uses `astMapSplit` + `axflags` + `astPickAxes` to discard base axes the new frame does not need. Rust `map_region` blindly transforms the defining points and slices `[..naxes]`, with no split/axflags logic. |
| `PickAxes` override (region.c:7579) | `pick_axes_region` (mod.rs:334) | **PORTED** | Faithful: picks current-frame axes, `map_split`s the current→base mapping, calls `reg_base_pick_axes`, inverts the split map, `map_region`s the base sub-region, `reg_overlay`s properties (mod.rs:358-372). Matches region.c:7684-7734. Minor: the C `def` (default-axis) short-circuit (region.c:7688-7695) is only handled implicitly (comment mod.rs:344-347); a `-1` axis request is not explicitly rejected before the split test, though callers never pass one. |
| `RegBasePick` base default (region.c:8277 / :13679) | `Region::reg_base_pick_axes` default (mod.rs:255) | **MISSING-BRANCH** | C's base default is *not* "always None": it returns a permuted copy of the whole region **when all base axes are selected** (a genuine permutation), else NULL (region.c:8345-8388). Rust's default trait method returns `Ok(None)` unconditionally. Concrete overrides (Box box_region.rs:686, Interval, PointList, NullRegion) cover the split cases, but a class that relies on the base default (Circle/Ellipse/Polygon/CmpRegion/Prism) will fail to produce the "select-all-axes permutation" region the C base default would. |
| `RegOverlay` (region.c:9861) | `Region::reg_overlay` (mod.rs:265) + `merge_util::finish_region_merge` (merge_util.rs:51) | **MISSING-BRANCH** | Rust copies Negated, Closed, Adaptive, and (same-naxes only) MeshSize/FillFactor. Missing vs C: (a) `this->regionfs = that->regionfs` (region.c:9906) — the RegionFS dump flag is not propagated; (b) the `unc` parameter behaviour — C clears `this`'s uncertainty when `unc!=0 && !astTestUnc(that)` (region.c:9919); Rust ignores the flag entirely (doc admits "unused here", mod.rs:263-264); (c) C's else-branch `astClearMeshSize/FillFactor` on axis-count mismatch is not done. |
| `Simplify` (parent Region, region.c:11042) | per-class `simplified` bodies (box_region.rs:940; interval.rs:1031; cmpregion.rs:736; etc.) | **MISSING-BRANCH** | The parent Region `Simplify` (a) simplifies the encapsulated FrameSet, (b) **drops default uncertainty** (region.c:11148), and (c) for explicit uncertainty performs the whole stability check: re-centre the simplified uncertainty at opposite corners of a test box and reject the simplification if the uncertainty bounding-box width changes by >0.1 % (region.c:11196-11306). No Rust class implements this uncertainty-stability check; there is no shared "parent simplify" helper. Each class's `simplified` open-codes only the FrameSet/geometry part and forwards uncertainty verbatim (e.g. interval.rs:1060) with no re-centre-and-compare. |
| `GetUncFrm` / `astGetUncFrm` (region.c:3912) | `get_uncertainty` (box_region.rs:660) | **MISSING-BRANCH** | C returns the explicit uncertainty *or a default* (`astGetDefUnc`), and maps it into the requested (BASE/CURRENT) frame via `astMapRegion` when the base→current mapping is not a UnitMap (region.c:3982-4013). Rust `get_uncertainty` returns `self.data.uncertainty.clone()` — `None` when none was set (no default synthesised), only ever the base-frame region, and never frame-mapped. There is no `ifrm` parameter. |
| `GetDefUnc` (region.c:3488) | `merge_util::region_is_point` inline formula only (merge_util.rs:143-155) | **ABSENT (as a constructible Region)** | C builds an actual default-uncertainty `Box` spanning 1.0E-6 of the base bounding box (half-width `|c|*0.5E-6` on zero-extent axes), via `astRegBaseBox2` (region.c:3556-3575). Rust reproduces only the *width scalar* inline inside the point-merge test; no code constructs a default uncertainty Region, so `GetUncFrm`, `Overlap`'s `unc1`, and `SetUnc`'s bounded-check have nothing to draw a default from. |
| `TestUnc` (region.c:4089) | `data.uncertainty.is_some()` checks (e.g. box_region.rs:729) | **PORTED (thin)** | C `astTestUnc` = "has an *explicit* (non-default) uncertainty". Rust models this as `Option::is_some`, which is adequate because Rust never stores a default uncertainty in the same field. Semantically equivalent for present callers. |
| `SetUnc` (region.c:10754) | `set_uncertainty` (box_region.rs:656) | **STUB** | C does substantial work: validates the uncertainty is re-centrable (`astRegCentre`), `Conv`-maps it into `this`'s base Frame (region.c:10842-10850), forces it bounded by negating if unbounded (region.c:10856), sets its RegionFS flag from the simplified base→current mapping (region.c:10867-10869), and re-centres it at the region's first defining point. Rust `set_uncertainty` is one line: `self.data.uncertainty = Some(unc)` — no frame conversion, no bounded-enforcement, no re-centring. |
| `GetUnc` (public, region.c:3784) | (folded into `get_uncertainty`) | **MISSING-BRANCH** | The public `astGetUnc(this, def)` returns a copy of the uncertainty, optionally the default when `def` is set, in the current Frame. Rust has no `def` distinction and no current-frame mapping (see GetUncFrm). |
| `GetBounded` (region.c:3358) | per-class `is_bounded` / `is_bounded_inner` (box_region.rs:579,481; cmpregion.rs:497) | **MISSING-BRANCH** | C base default: **always bounded if the base Frame is a SkyFrame** (a finite region on a sphere has a finite negation), else `!Negated` (region.c:3406-3411). Rust `is_bounded` for closed-curve classes returns `!negated` (box_region.rs:579-590) with no SkyFrame special case, so a negated Box/Circle/Ellipse on a SkyFrame is wrongly reported unbounded — this feeds the Overlap NullRegion detection (overlap.rs:163) and the `Simplify` test-box selection. |
| `RegMesh` (region.c:8731) | per-class `get_region_mesh` (box_region.rs:619; cmpregion.rs:555) | **PORTED** | C gets the base mesh and transforms into the current frame if the mapping is not a UnitMap. Rust equivalents build/transform the base mesh into current coords. Faithful for the classes checked. |
| `RegGrid` (region.c:8645) | `Region::get_region_grid` (mod.rs:199) → `RegionGeometry::reg_base_grid` (data.rs:235) | **MISSING-BRANCH** | C caches `this->basegrid` and transforms it into the current frame (region.c:8705-8718). Rust `reg_base_grid` (data.rs:235) builds a regular grid in the bounding box and filters by `reg_pins`; it does not cache, and (unlike C's base `RegBaseGrid`, region.c:7960) makes no 3-attempt density-refinement loop to hit the target point count. |
| `RegBaseMesh` (region.c:8221, abstract) | `RegionGeometry::reg_base_mesh` (data.rs:212, required) | **PORTED** | Both abstract/required; each class supplies it. OK by design. |
| `RegBaseGrid` base algorithm (region.c:7960) | `RegionGeometry::reg_base_grid` default (data.rs:235) | **HEURISTIC** | C's base implementation wraps the region in a Box, calls the Box's `RegGrid`, masks with `BTransform`, and retries up to 3× adjusting `np` to land within ~5 % of the target size (region.c:8047-8060+). Rust uses a fixed `npoints^(1/naxes)` odometer grid with no retry/target-size convergence. Density differs from C. |
| `RegBaseBox` / `RegBaseBox2` (region.c:7855 / :7909) | `RegionGeometry::reg_base_box` (data.rs:207) | **MISSING-BRANCH** | C has *two* methods: `RegBaseBox` assumes un-negated; `RegBaseBox2` propagates the negation assumption into component regions (used by `GetDefUnc`, region.c:3554). Rust has only `reg_base_box`; there is no `reg_base_box2`, so compound regions (Prism/CmpRegion) cannot compute the negation-aware box that C's default-uncertainty path relies on. |
| `GetRegionBounds` (region.c:8948) | `get_region_bounds` (box_region.rs:599) | **MISSING-BRANCH** | C simplifies the base→current mapping; if UnitMap returns the base box, else meshes the boundary, transforms to current, and takes min/max — i.e. **bounds are in the current Frame**. Rust `get_region_bounds` for Box just returns `reg_base_box()` (box_region.rs:600) — the **base**-frame box, with no current-frame mesh transform. Correct only when base==current. |
| `RegCentre` (region.c:8397) | `RegionGeometry::reg_centre` (data.rs:215) | **MISSING-BRANCH** | C `astRegCentre` is dual-purpose: **get** the centre (when `cen`/`ptr` NULL) **or re-centre** the region at a supplied position, in BASE or CURRENT frame (`index`,`ifrm` args). Rust `reg_centre` is get-only, base-frame only (data.rs:215). The re-centre capability — required by the parent `Simplify` uncertainty-stability loop (region.c:11240-11289) and by `SetUnc` — is entirely absent. |
| `Negate` (region.c:6472) | `Region::negate` (box_region.rs:546) + `negated_copy` | **PORTED** | Toggles the negated flag. Faithful (C also just toggles the attribute). |
| `SetRegFS` (region.c:10674) | `RegionData::set_region_fs` (data.rs:134) + `Region::set_region_fs` | **PORTED** | Builds a two-frame FrameSet (base=current=frame via UnitMap), clears the dummy-FS flag, recursively fixes a dummy-FS uncertainty. Matches region.c:10745-10751. |
| `RegDummyFS` (region.c:8814) | `has_dummy_fs` + `data.dummy_fs` (data.rs:70) | **PORTED** | Flag-based; equivalent to the C `DUMMY_FS` Ident marker. |
| `MatchRegion` | (cmpregion.c, out of base scope) | **N/A** | Not defined in region.c; lives in cmpregion.c. Out of base-class scope. |
| `astConvert` / conversion-based equality | frame `frames_equal` / `astConvert` in frames module | **ABSENT here** | `OverlapX` (region.c:7088) and `SetUnc` (region.c:10844) rely on `astConvert`/`Conv` to align two region frames before comparison. Rust `compute_overlap` explicitly skips this (overlap.rs:37-39). No region-frame alignment step exists in the shared overlap/merge code. |
| `RegSetAttrib` (region.c:9923) | `with_region_integrity` (data.rs:347) non-adaptive branch | **STUB (non-adaptive path)** | C `RegSetAttrib` sets the attribute on *both* base and current frames without remapping, handling axis-permutation in the setting string. The Rust non-adaptive branch of `with_region_integrity` (data.rs:364-388) admits in its own comment that it "just invalidates the cache — the non-adaptive path is less common and will be fully implemented when needed"; it does **not** re-apply the closure to the base frame. |
| `Mask<X>` (region.c:5969 macro) | `masking::mask` (masking.rs:42) | **MISSING-BRANCH** | Rust `mask` is a straightforward per-pixel transform-and-test. C's `MAKE_MASK` special-cases zero-volume regions (PointList) so a pixel is masked if any part of the region passes through it (region.c:5828-5834 doc), and computes the region↔grid overlap box to skip whole rows outside the region. Rust tests every pixel and has no zero-volume inclusion rule. |

## 2. Direct answers to the posed questions

**Does Rust `verify_remap_exact` + `reg_pins` faithfully replace C's `astRegPins`?**
No. C `astRegPins(this, pset, unc, &mask)` is a *boundary*-membership test with a
joint-uncertainty tolerance and returns a per-point `mask` of which points lie on
the boundary. Rust `reg_pins` (data.rs:191) is an *interior* predicate with no
uncertainty argument and no mask; `verify_remap_exact` (data.rs:313) layers a
box.c:3905-style inverse round-trip on top of that interior predicate. It works for
the narrow simplify use (is a candidate axis-aligned box exact under the mapping?)
but is not a general `astRegPins`: it cannot answer OverlapX's "are these two
boundaries equivalent to within uncertainty?" question, which is why
`compute_overlap` fell back to a 90 % threshold heuristic. This is the same class of
gap flagged in the memory note `project_overlap_is_heuristic_not_port`.

**Is uncertainty carried through `map_region`?**
No. Every per-class `map_region` copies the scalar attributes but omits
`data.uncertainty` (box_region.rs:675-682 and the other eight impls). C preserves it
via `astCopy(usethis)` at region.c:5704. This is the cross-cutting gap recorded in
`project_map_region_drops_uncertainty` and it is confirmed present in all nine
classes. `reg_base_pick_axes` (box_region.rs:701-707) *does* thread uncertainty
through, so the loss is specifically in `map_region`.

**Does the base `PickAxes` override exist and match region.c:7579?**
Yes — `pick_axes_region` (mod.rs:334) is a faithful port of the region.c:7579 control
flow (pick current axes → `map_split` → `reg_base_pick_axes` → invert split map →
`map_region` → `reg_overlay`). The only shortfall is that the region-returning
`RegBasePick` *base default* it calls (mod.rs:255) returns `None` unconditionally
rather than C's "permuted whole-region when all base axes selected" (region.c:8345).

## 3. Blocking dependencies for the 4 remaining simplify failures

| Fixture | Target shape | Blocking gaps |
|---|---|---|
| `cap_slamap_02` (Box → Prism) | Prism | `Box::simplified` already calls `convert_to_prism` (box_region.rs:981); `convert_to_prism` (box_region.rs:180) depends on `pick_axes_region` (PORTED) and per-class `reg_base_pick_axes` (PORTED for Box). The residual risk is `verify_remap_exact`/`reg_pins` (HEURISTIC): the "candidate box exact?" decision (box_region.rs:976) uses the interior predicate, not C `astRegPins` with uncertainty, so the box-vs-prism branch can be taken on the wrong side of the boundary. Also `map_region` dropping uncertainty (§2) can make the resulting Prism serialise differently from C. |
| `cap_interval_01` (Prism → Interval) | Interval | `Prism::simplified` (prism.rs:642) is ported and drives `region_merge`; the merge tail (`finish_region_merge`, merge_util.rs:51) re-simplifies a fully-bounded Interval to a Box. Blocking risks: (a) `RegOverlay` not propagating RegionFS (§1) and (b) parent-Simplify uncertainty handling absent, both affecting the final serialised form; (c) `combine_uncertainties` (merge_util.rs:183) builds a Prism/CmpMap of uncertainties but the merged region's uncertainty still won't survive a later `map_region`. |
| `cap_pointlist_02` (Prism → PointList) | PointList | Same Prism/merge path as above. `region_is_point` (merge_util.rs:128) mirrors box.c:1921 using the inline `GetDefUnc` width; since no real default-uncertainty Region is constructed (§1 GetDefUnc ABSENT), the zero-width test relies solely on that inline formula and cannot use an explicit mapped uncertainty. |
| `cap_slamap_07` (CmpRegion → NullRegion) | NullRegion | **`CmpRegion::simplified` (cmpregion.rs:736) is still the STUB** — it only remaps the two sub-regions and rebuilds a CmpRegion. It performs neither the NullRegion-exclusion table (cmpregion.c:3644) nor the overlap-reduction decision table (cmpregion.c:3695). This is sub-project B in the Group-5 plan and is not yet landed. It further depends on `compute_overlap` returning C-accurate `None`/`Same`/containment results, which is compromised by the HEURISTIC overlap (threshold, no `astConvert`, no negated-equal value 6) and by `is_bounded` lacking the SkyFrame rule (§1 GetBounded) — both feed the NullRegion detection. |

## 4. Cross-cutting gaps (shared across all Region classes)

1. **Uncertainty dropped in `map_region`** (all 9 classes) — C preserves via
   `astCopy`; Rust never copies `data.uncertainty`. Blocks byte-exact uncertainty
   round-trip after any remap and after `pick_axes`/`ConvertToPrism`. (memory:
   `project_map_region_drops_uncertainty`.)
2. **No uncertainty-stability check in Simplify** — the parent Region
   `Simplify` re-centre-at-corners / 0.1 %-width test (region.c:11196-11306) is
   implemented nowhere; explicit uncertainties are forwarded verbatim or dropped.
3. **`reg_pins` is an interior test, not a boundary+uncertainty test** — the base
   `astRegPins` semantics are absent library-wide; `verify_remap_exact` and
   `compute_overlap` both paper over this (threshold heuristic / round-trip). (memory:
   `project_overlap_is_heuristic_not_port`.)
4. **Overlap has no `astConvert` frame alignment and no value-6 (negation) case**;
   uses a hard-coded 90 % `THRESHOLD` (overlap.rs:24) instead of joint uncertainty.
5. **No default-uncertainty Region** (`GetDefUnc`/`GetUncFrm` default path) — only an
   inline width scalar exists; `get_uncertainty` returns `None` where C returns a
   synthesised default Box.
6. **`SetUnc` is a bare field store** — no frame conversion, boundedness enforcement,
   RegionFS flag, or re-centre.
7. **`GetBounded` lacks the SkyFrame-always-bounded rule** — affects negated regions
   on the sky, feeding overlap and simplify test-box logic.
8. **`RegCentre` is get-only** — no re-centre; and there is no `RegBaseBox2`
   (negation-aware) or current-frame `GetRegionBounds` mesh transform.
9. **Non-adaptive `with_region_integrity` is a stub** (data.rs:364-388) — does not
   re-apply attribute mutations to the base frame as C `RegSetAttrib` does.

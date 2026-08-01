# Fidelity audit: Box / Interval / PointList regions

Scope: the "sliceable geometry" region classes that support `RegBasePick`.

- C reference: `reference/starlink-ast/src/{box,interval,pointlist}.c`
- Rust: `crates/ast-core/src/regions/{box_region,interval,pointlist}.rs`
- Shared Rust infrastructure: `crates/ast-core/src/regions/{data,overlap,merge_util}.rs`

Status legend: **PORTED** (faithful) | **STUB** | **HEURISTIC** | **MISSING-BRANCH**
(main path ported, sub-branches omitted) | **ABSENT**.

Every claim cites C `file:line` and Rust `file:line`.

---

## 1. Box (`box.c` → `box_region.rs`)

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `Simplify` (box.c:3606) | `Mapping::simplified` (box_region.rs:941) | **MISSING-BRANCH** | Three of the four C branches are absent. (a) **PermMap fast path** (box.c:3722-3874): C reads InPerm/OutPerm/Constants to build a new Box, or `NullRegion`/`Interval` when an output is fed a constant/`AST__BAD` or a slice misses the box. Rust has no PermMap branch — a PermMap base→current mapping falls into the generic `verify_remap_exact` path. (b) **2-D→Polygon branch** (box.c:3917-4027): the KNOWN gap — when `BestBox`+`RegPins` fails and `nin==nout==2`, C builds an `astPolygon` from the 4 transformed corners (with clockwise/anti-clockwise + SkyFrame `right_way_round` selection). Rust jumps straight from a failed `verify_remap_exact` to `convert_to_prism` (box_region.rs:976-986), never attempting a Polygon. (c) The `BestBox` fitting (box.c:3895) is replaced by a direct point-transform + `verify_remap_exact` heuristic (box_region.rs:952-977) rather than the least-squares axis fit. |
| `Simplify` → `astConvertToPrism` (box.c:4036) | `convert_to_prism` (box_region.rs:180-271) | **PORTED** | Subset scan + pick/simplify/Prism/permute/remap is a faithful port. Correctly reached only after the earlier paths fail. |
| `BestBox` (box.c:407) | ABSENT | **ABSENT** | The least-squares best-fitting-box-through-a-mesh routine is not ported; Rust uses direct corner transforms instead (box_region.rs:952). Consequence: shear/near-degenerate mappings that C would still fit as a Box are not simplified. |
| `RegPins` (box.c:2952) | `reg_pins` (box_region.rs:310) | **HEURISTIC** | C builds a *larger* and a *negated smaller* Box (widened/narrowed by half the combined uncertainty widths, box.c:3115-3129) and tests membership of both, i.e. a true "on the boundary within uncertainty" band test. Rust's `reg_pins` is a plain point-in-box interior test with no boundary-band and no uncertainty widening. This is the region-`RegPins` semantic (used by Simplify/Overlap), fundamentally different from C's boundary test. |
| `RegBasePick` (box.c:2655) | `reg_base_pick_axes` (box_region.rs:687) | **PORTED** | Slices centre/corner to selected axes, picks base Frame, propagates+drops uncertainty per box.c:2727-2736. Faithful. |
| `RegCentre` (box.c:2778) | `reg_centre` getter only (box_region.rs:478) | **MISSING-BRANCH** | Rust implements only the *read* path (return centre). The *set/re-centre* path (box.c:2885-2944 — recompute corner via `GeoCorner`, update points, reset cache/basemesh/basegrid) is ABSENT. No public re-centre entry point exists for Box. |
| `RegBaseBox` (box.c:2106) | `reg_base_box` (box_region.rs:351) | **PORTED** | Returns `bounds()` (centre±extent). Equivalent. |
| `RegBaseMesh` (box.c:2277) | `reg_base_mesh` (box_region.rs:355) | **MISSING-BRANCH** | 1-D/2-D/single-point paths present. But C computes per-edge point counts using **geodesic** edge lengths via the Frame metric (box.c NP_EDGE sampling, `astDistance`); Rust uses flat Euclidean `sqrt(dx²+dy²)` (box_region.rs:389). For a curved/SkyFrame base this distributes mesh points differently. The N-D face-grid path (box_region.rs:433-475) is a plausible reconstruction, not a line-for-line port. |
| `RegBaseGrid` (box.c:2179) | `reg_base_grid` default (data.rs:235) | **PORTED (generic)** | Box does not use a class-specific grid in Rust; the generic bounded-box grid+filter is equivalent for an axis-aligned box. |
| `RegTrace` (box.c:3216) | ABSENT | **ABSENT** | 2-D boundary tracing not ported. Out of stated scope but noted. |
| `MergeBox` (box.c:1658) | `region_merge` (box_region.rs:720) | **MISSING-BRANCH** | Box+Box/Interval/NullRegion/PointList branches ported faithfully (incl. the point-width guard for PointList). BUT the post-merge attribute propagation is incomplete: C scales the result **FillFactor** by the product of the two inputs' fill factors (box.c:2051-2054) and scales **MeshSize** by the product of the used/default ratios (box.c:2056-2085). Rust `finish_region_merge` (merge_util.rs:43-81) copies only Closed/Adaptive/Negated/uncertainty — FillFactor product and MeshSize scaling are ABSENT. |
| `Equal` (inherited, region.c:2631) | `Mapping::equal` (box_region.rs:994) | **PORTED (approx)** | Compares negated/closed/points/current-frame. C also requires equal *base* Frames and a UnitMap base→current mapping (region.c:2666-2670); Rust compares only the current frame and does not verify the base-frame/UnitMap condition. Adequate for UnitMap-based regions; diverges for remapped regions. |
| `MapRegion` (inherited, region.c:5375) | `map_region` (box_region.rs:665) | **MISSING-BRANCH** | C `astMapRegion` takes a deep copy (`astCopy`, region.c:5704) which **preserves the uncertainty Region**, then re-frames it. Rust builds a fresh `RegionData` copying negated/closed/mesh/fill/adaptive but **drops `data.uncertainty`** (box_region.rs:674-684). See Cross-cutting §3. Also omits the bad-value validation and axis-removal (`astMapSplit`) logic. |

---

## 2. Interval (`interval.c` → `interval.rs`)

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `Simplify` (interval.c:3101) | `Mapping::simplified` (interval.rs:1031) + `simplify_remapped` (interval.rs:146) | **PORTED (approx)** | The Box-reclassification (Cache) path and the unbounded-remap path (interval.c:3216-3517) are both ported and closely follow the C bound-transfer logic. One divergence in the reclassification guard: C `Cache` (interval.c:489) makes the equivalent Box whenever `isBox && !neg`, handling Negated (line 509) and Closed (line 512) *separately afterwards*. Rust `simplified` (interval.rs:1043) only reclassifies when `!negated && closed`, so a **closed negated** or **non-closed** well-ordered Interval that C would still route through the equivalent Box does not reclassify. |
| `RegBasePick` (interval.c:2328) | `reg_base_pick_axes` (interval.rs:779) | **PORTED** | Slices lbnd/ubnd, picks base Frame, uncertainty propagate/drop. Faithful. |
| `Overlap` (interval.c:1765) | none — falls to `compute_overlap` (interval.rs:620 → overlap.rs:40) | **HEURISTIC / MISSING** | C provides a *specialised* Interval-vs-Interval overlap (interval.c:1887-...) that transfers axis limits across the base-frame Mapping when it is one-to-one (`OneToOne`), because an unbounded Interval has no finite boundary mesh. Rust routes every Interval overlap through the generic **mesh-based** `compute_overlap`, which explicitly cannot represent an unbounded Interval's boundary (its `reg_base_mesh` returns empty for unbounded intervals, interval.rs:490). Unbounded-Interval overlaps are therefore unreliable. |
| `RegPins` (interval.c:2580) | `reg_pins` (interval.rs:406) | **HEURISTIC** | For the Box-equivalent case C delegates to `astRegPins` on the cached Box (interval.c:2683) — the large/small boundary-band test. For the genuinely unbounded case C builds larger/smaller Intervals and tests the band (interval.c:2640-...). Rust `reg_pins` is a plain interior membership test with no boundary band or uncertainty widening. Same class of gap as Box. |
| `RegBaseBox` (interval.c:2191) | `reg_base_box` (interval.rs:459) | **PORTED (approx)** | Uses effective (±MAX) bounds and widens excluded axes to the full range. Matches C intent for the bounded case; unbounded axes return ±`f64::MAX` rather than the C behaviour of reporting via the cached Box (C errors for unbounded mesh). |
| `RegBaseMesh` (interval.c:2257) | `reg_base_mesh` (interval.rs:482) | **MISSING-BRANCH** | C delegates to the equivalent Box's mesh and **errors (`AST__INTER`) if unbounded** (interval.c:2317). Rust returns an empty Vec for unbounded (interval.rs:490-493) and generates 2^naxes corners for the bounded case rather than the Box's edge-distributed mesh — so even the bounded mesh differs from C's Box-derived mesh. |
| `RegCentre` (interval.c:2451) | `reg_centre` getter (interval.rs:514) | **MISSING-BRANCH** | Read path only (bounded intervals). The set/re-centre path is ABSENT. |
| `MergeInterval` (interval.c:1224) | `region_merge` (interval.rs:810) | **MISSING-BRANCH** | Interval+Interval/Box/NullRegion branches ported. The **Interval+PointList** branch (interval.c:1458) is deliberately not ported (interval.rs:876-891) — argued unreachable and defective upstream; acceptable but note it is a real C branch. Same FillFactor/MeshSize post-merge scaling gap as MergeBox (merge_util.rs:43-81). |
| `Equal` (inherited region.c:2631) | `Mapping::equal` (interval.rs:1084) | **PORTED (approx)** | Same base-frame/UnitMap omission as Box. |
| `MapRegion` (inherited region.c:5375) | `map_region` (interval.rs:757) | **MISSING-BRANCH** | Drops uncertainty (interval.rs:766-776); see §3. |

---

## 3. PointList (`pointlist.c` → `pointlist.rs`)

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `Simplify` (pointlist.c:2161) | `Mapping::simplified` (pointlist.rs:708) | **PORTED** | UnitMap short-circuit + transform-points-into-current-Frame + rebuild in current Frame with UnitMap. Faithful to pointlist.c:2240-2260. |
| `RegBasePick` (pointlist.c:1738) | `reg_base_pick_axes` (pointlist.rs:512) | **PORTED** | Slices each point to selected axes, uncertainty propagate/drop. Faithful. |
| `RegPins` (pointlist.c:1868) | `reg_pins` (pointlist.rs:161) | **HEURISTIC** | C does a bidirectional membership test using the actual **uncertainty Region** (transform points through the PointList/uncertainty, check no BAD both ways — pointlist.c:1970-2047). Rust invents a relative tolerance from the bounding-box diagonal (`DEFAULT_TOLERANCE = 1e-10 × extent`, pointlist.rs:150,290-327) and does a `Frame::distance` threshold. When an explicit uncertainty is set, C uses it; Rust ignores it in `reg_pins`. This is a fabricated tolerance, not a port. |
| `Transform` (pointlist.c:2359) | `region_transform` (data.rs, shared) | **HEURISTIC (consequence)** | C membership uses the negated uncertainty Region re-centred at each point (pointlist.c:2494-2570). The Rust `reg_pins` tolerance path feeds `point_in_region`; the uncertainty-based masking algorithm is not reproduced. |
| `RegBaseMesh` (pointlist.c:1667) | `reg_base_mesh` (pointlist.rs:227) | **PORTED** | Mesh = copy of the defining points. Faithful. |
| `RegBaseBox` (pointlist.c:1532) | `reg_base_box` (pointlist.rs:203) | **PORTED (approx)** | Min/max over points. C also expands by the uncertainty bounds; Rust returns the raw point extent only. Minor. |
| `RegCentre` | `reg_centre` (pointlist.rs:233) | **HEURISTIC** | Rust returns the centroid (mean of points). PointList in C does not define a meaningful settable centre the same way; the getter as centroid is a reasonable but non-canonical choice. |
| `MergePointList` (pointlist.c:1205) | `region_merge` (pointlist.rs:547) | **PORTED (approx)** | PointList+PointList (single point each, equal Negated) ported faithfully. Same FillFactor/MeshSize post-merge scaling gap (pointlist.c:1427 region + merge_util.rs:43-81). |
| `Equal` (inherited) | `Mapping::equal` (pointlist.rs:740) | **PORTED (approx)** | Compares points/negated/closed/current-frame; same base-frame/UnitMap omission. |
| `MapRegion` (inherited) | `map_region` (pointlist.rs:489) | **MISSING-BRANCH** | Drops uncertainty (pointlist.rs:500-509); see §3. |

---

## Blocking dependencies

- **cap_slamap_02 (Box→Prism, needs Box→Polygon branch)** — BLOCKED by the
  **missing 2-D→Polygon branch** in `Box::simplified` (C box.c:3917-4027; Rust
  box_region.rs:976 jumps from failed `verify_remap_exact` directly to
  `convert_to_prism`). When a 2-D Box under a rotation/shear cannot be re-fit as
  an axis-aligned Box, C produces a Polygon of the 4 transformed corners; Rust
  instead attempts a Prism split, which does not yield the expected Polygon form.
  The `Polygon`/`PolygonBuilder` type exists (polygon.rs:91) so the branch is
  portable — this is the primary fix. Secondary: the **missing `BestBox` fit**
  (box.c:407) and **PermMap fast path** (box.c:3722) mean some cases never reach
  even the Box-refit decision the way C does.

- **cap_interval_01 (nested Prism→Interval)** — Depends on
  `Interval::simplified`'s remap path (interval.rs:146), which is ported, AND on
  the `MergeInterval` Interval+Interval/Box/Null branches (ported). The likely
  residual blocker is the **reclassification guard divergence** (Rust requires
  `!negated && closed` at interval.rs:1043 where C `Cache` reclassifies on
  `isBox && !neg` and handles Closed/Negated separately, interval.c:489-512), so
  a nested Interval that is well-ordered but not `closed` fails to reduce to a
  Box the way C does, blocking the downstream Prism collapse.

- **cap_pointlist_02 (Prism→PointList via Interval+PointList merge)** — BLOCKED.
  The C route runs through `Box::MergeBox`'s PointList branch (box.c:1907) or the
  Interval+PointList path; the **Interval+PointList branch is intentionally not
  ported** (interval.rs:876-891). The Box+PointList branch *is* ported
  (box_region.rs:784), so the outcome depends on whether the Prism's sub-Interval
  reclassifies to a Box first (see the guard gap above). If it does not
  reclassify, no merge helper handles Interval+PointList and the Prism cannot
  collapse to a PointList.

---

## Cross-cutting

### Uncertainty dropped in `map_region` (all three classes)

C `astMapRegion` (region.c:5704) deep-copies the source Region with `astCopy`,
which **retains the uncertainty Region**, then re-frames it. All three Rust
`map_region` impls build a fresh `RegionData` and copy the scalar attributes but
**omit `data.uncertainty`**:

- Box: box_region.rs:674-684
- Interval: interval.rs:766-776
- PointList: pointlist.rs:500-509

This matches the known cross-cutting gap "map_region drops uncertainty" in
project memory. Impact: any pipeline that maps an uncertain Region into a new
frame loses its positional uncertainty, which in turn degrades `RegPins`/overlap
band widths downstream.

### `reg_pins` is a heuristic interior test, not the C boundary-band port (all three)

C's `RegPins` for each class is a *boundary* test: build a slightly larger and a
negated slightly smaller region (widened by half the combined `this`+supplied
uncertainty widths) and require the point to be inside both — i.e. within an
uncertainty-scaled band around the boundary (box.c:3115-3186;
interval.c:2640-...; pointlist.c bidirectional uncertainty test 1970-2047). The
Rust `reg_pins` methods (box_region.rs:310, interval.rs:406, pointlist.rs:161)
are plain point-membership tests:

- No larger/smaller boundary band.
- No use of the region's own or the supplied uncertainty widths.
- PointList fabricates a `1e-10 × bbox-diagonal` tolerance (pointlist.rs:290)
  instead of using the uncertainty Region.

Because `Box::Simplify`'s exactness check relies on `astRegPins` (box.c:3899,
3907), the Rust substitute `verify_remap_exact` (data.rs:313) — an all-points-
inside interior test — is a *different* predicate and can accept/reject
simplifications differently from C, compounding the Simplify gaps above.

### MergeBox/MergeInterval/MergePointList: FillFactor + MeshSize propagation absent

All three C merge helpers scale the result's FillFactor by the product of the
inputs' fill factors and scale MeshSize by the product of used/default ratios
(box.c:2049-2085; interval.c ~1601 region; pointlist.c ~1427). The shared Rust
`finish_region_merge` (merge_util.rs:43-81) and PointList's inline merge
(pointlist.rs:594-601) copy only Closed/Adaptive/Negated/uncertainty. FillFactor
product and MeshSize scaling are not applied.

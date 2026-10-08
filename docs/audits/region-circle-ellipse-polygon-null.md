# Region fidelity audit: Circle, Ellipse, Polygon, NullRegion

Read-only audit of the pure-Rust port against the C reference. Scope:
`circle`, `ellipse`, `polygon`, `nullregion`. Every claim cites C
`reference/starlink-ast/src/<f>.c` and Rust
`crates/ast-core/src/regions/<f>.rs`.

Status legend:
- **PORTED** — faithful control-flow-equivalent port.
- **STUB** — present but trivial/no-op where C does real work.
- **HEURISTIC** — replaced C's exact geometry with an approximation/tolerance.
- **MISSING-BRANCH** — ported partially; specific C branches absent.
- **ABSENT** — no Rust counterpart at all.

A cross-cutting semantic mismatch runs through all four classes: **C
`RegPins` is an *on-boundary* test** (is each supplied point within the
uncertainty band of the boundary curve), whereas the Rust trait method
`reg_pins` is an *inside* test (is each point interior). The two are used
for different purposes in C (`Transform` uses inside; `Simplify`/`Overlap`
use on-boundary `astRegPins`). The Rust port has no on-boundary predicate
at all, which is the root cause of several gaps below.

---

## Circle (`circle.c` ↔ `circle.rs`)

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `Simplify` (circle.c:1769) | `Mapping::simplified` (circle.rs:603) | STUB / MISSING-BRANCH | C fits a *new* Circle to the current-Frame boundary mesh via `astBestCircle`, validates with `astRegPins`, and if 2-D falls back to `astBestEllipse` (circle.c:1876-1906). Rust only transforms the two defining points through the simplified base→current map and rebuilds a Circle; it never re-fits, never tries the Circle→Ellipse degeneration, and never validates that the transformed shape is still a circle. Non-conformal maps silently produce a wrong Circle. |
| `RegPins` (circle.c:1289) | `reg_pins` (circle.rs:182) | HEURISTIC (semantic mismatch) | C builds large/small concentric circles sized by the uncertainty-box diagonal and tests membership in the annulus (on-boundary test, circle.c:1450-1466). Rust `reg_pins` is a plain `dist <= radius` interior test with no uncertainty band. Uncertainty Region is entirely ignored. |
| `RegBaseBox` (circle.c:757) | `reg_base_box` (circle.rs:218) | HEURISTIC | Rust uses Euclidean `centre ± radius` on every axis. C derives the box from the offset mesh via `astOffset2`/`astAxDistance` and `astNormBox` (circle.c RegBaseMesh 985-1015). Wrong for SkyFrame circles and for any non-Euclidean base Frame (comment at circle.rs:226-229 admits this). |
| `RegBaseMesh` (circle.c:850) | `reg_base_mesh` (circle.rs:241) | HEURISTIC | Rust 2-D path uses parametric `centre + r*(cos,sin)` in raw coordinates (circle.rs:264-267); C uses `astOffset2` along the frame geodesic (circle.c:985). Differs on the sphere. N-D path is an ad-hoc per-axis-pair scatter (circle.rs:281-291) vs C's bounding-box-mesh-projected-to-sphere (circle.c:1018+). |
| `RegCentre` (circle.c:1114) | `reg_centre` (circle.rs:296) get-only | MISSING-BRANCH | C supports both *get* and *set* (re-centre) of the circle centre, shifting the edge point accordingly (circle.c:1206+). Rust only returns `points[0]`; no re-centre path. `astRegCentre(...,cen,...)` is used by C `RegPins` to re-centre uncertainty — absent here. |
| `Overlap`/`OverlapX` (inherited from region.c) | `overlap` → `compute_overlap` (circle.rs:354, overlap.rs:40) | HEURISTIC | Mesh-count-with-90%-threshold (overlap.rs:21, THRESHOLD=0.90) rather than C's `astRegPins`-based classification. No `Same`/negation (value 6) detection except NullRegion special cases. |
| `RegBasePick` (base default → NULL) | `reg_base_pick_axes` default `None` (mod.rs:301) | PORTED | Circle correctly uses the default (returns None). Matches C. |
| `map_region` (region.c:5375 `MapRegion`) | `map_region` (circle.rs:472) | MISSING-BRANCH | **Uncertainty dropped.** C `astMapRegion` takes `astCopy(usethis)` (region.c:5704), preserving the uncertainty Region. Rust builds a fresh `RegionData` and never copies `self.data.uncertainty` (circle.rs:486-492). Also omits the bad-value validation (region.c:5507-5551) and the fewer-axes MapSplit reduction (region.c:5593). |
| `Equal` (inherited) | `Mapping::equal` (circle.rs:636) | PORTED (approx) | Compares points+flags+frame. Reasonable; C `Region` Equal is also geometry+frame based. |
| `GetBounded` (base → always bounded) | `is_bounded_inner` → true (circle.rs:300) | PORTED | Circle always bounded; matches base Region default. |

---

## Ellipse (`ellipse.c` ↔ `ellipse.rs`)

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `Simplify` (ellipse.c:1879) | `Mapping::simplified` (ellipse.rs:565) | STUB / MISSING-BRANCH | C re-fits via `astBestEllipse` on the current-Frame mesh and validates with `astRegPins`; can also degrade to a Circle (ellipse.c mirrors circle). Rust only transforms the 3 defining points and rebuilds — no re-fit, no validation, no Circle degeneration. |
| `RegPins` (ellipse.c:1381) | `reg_pins` (ellipse.rs:194) | HEURISTIC (semantic mismatch) | C sandwiches between large/small ellipses sized by uncertainty diagonal (ellipse.c:1538-1557, on-boundary test). Rust computes normalised radius `u²/a² + v²/b² <= 1` — an interior test that ignores uncertainty entirely. |
| `RegBaseBox` (ellipse.c:933) | `reg_base_box` (ellipse.rs:230) | HEURISTIC | Rust uses the closed-form rotated-ellipse extent `sqrt((a cos)²+(b sin)²)` in raw coords. C derives from mesh + `astNormBox`. Diverges on non-Euclidean base Frames (Ellipse is 2-D only, so lower risk, but not sky-correct). |
| `RegBaseMesh` (ellipse.c:1000) | `reg_base_mesh` (ellipse.rs:248) | HEURISTIC | Rust parametric rotated ellipse in raw coordinates; C walks the geodesic. Fine on a flat frame, wrong on the sphere. |
| `RegCentre` (ellipse.c:1206) | `reg_centre` (ellipse.rs:268) get-only | MISSING-BRANCH | Get-only; no re-centre path (same gap as Circle). |
| `map_region` | `map_region` (ellipse.rs:436) | MISSING-BRANCH | **Uncertainty dropped** (ellipse.rs:447-453, no `uncertainty` copy). Same axis-validation/MapSplit omissions as Circle. |
| `RegBasePick` (default → NULL) | default `None` (mod.rs:301) | PORTED | Ellipse cannot slice per-axis; correct. |
| `Overlap` | `compute_overlap` (overlap.rs) | HEURISTIC | Shared mesh-threshold engine. |
| `Equal` | `Mapping::equal` (ellipse.rs:597) | PORTED (approx) | points+flags+frame. |

---

## Polygon (`polygon.c` ↔ `polygon.rs`) — priority for cap_slamap_02

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| Constructor + `EnsureInside` (polygon.c:1768; called 7160/7325) | `PolygonBuilder::build` (polygon.rs:162) | **ABSENT** | C runs `EnsureInside` at construction and on load: it computes `astGetBounded`/`SkyArea`, and if the un-negated polygon would be *unbounded* it **reverses vertex order and toggles Negated** (polygon.c:1845-1882) so the un-negated polygon is always the bounded lobe. Rust stores vertices verbatim with no ordering normalisation and no `acw` flag. A clockwise-ordered vertex list (as produced by transforming Box corners) is therefore mis-oriented. |
| `Cache`/`acw` orientation + interior point (polygon.c:575, 668-703) | — | **ABSENT** | C finds a well-interior "safe" point via `Polywidth` per edge and sets the `acw` (anti-clockwise) flag; both feed `GetBounded`, `Transform`, and `RegPins`. Rust has no `acw`, no cached interior point (`reg_centre` just averages vertices, polygon.rs:470, which is not guaranteed interior for concave polygons). |
| `GetBounded` (polygon.c:2749) | `is_bounded_inner` → always true (polygon.rs:482) | **MISSING-BRANCH** | C returns `!negated` for ACW order and `negated` for CW order (SkyFrame always bounded) (polygon.c:2811-2822). Rust hardcodes `true`, so a negated or clockwise polygon is wrongly reported bounded. Breaks the bounding-box quick-reject in `compute_overlap` (overlap.rs:56) and any unbounded-polygon logic. |
| `Simplify` (polygon.c:5291) | `Mapping::simplified` (polygon.rs:795) | **MISSING-BRANCH** | C requires `nout == 2` (polygon.c:5380), rebuilds a Polygon from the transformed vertices, and — unless `SimpVertices` is set — validates that the original mesh still lies on the new polygon boundary via `astRegPins`, annulling the result if the edges bent (polygon.c:5412-5429). Rust only transforms vertices and rebuilds; no `nout==2` guard, no `SimpVertices` attribute, no bend validation. |
| `RegPins` (polygon.c:4562) | `reg_pins` (polygon.rs:278) | **HEURISTIC (semantic mismatch)** | C on-boundary test: resolves each point into parallel/perpendicular components per edge via `astResolvePoints`, marks points within `wid` (uncertainty-diagonal) of any edge as on-boundary (polygon.c:4737-4800). Rust `reg_pins` is a crossing-number *inside* test with an ad-hoc `f64::EPSILON` on-edge tolerance (polygon.rs:343-345). This is the exact predicate `Box::Simplify` and `Polygon::Simplify` need to validate a fitted polygon, and it is not implemented — **the single biggest blocker for cap_slamap_02.** |
| `RegBaseBox` (polygon.c:4180) | `reg_base_box` (polygon.rs:392) | HEURISTIC | Rust takes raw min/max of vertex coords. C works in `astAxDistance` offsets from the first vertex and calls `astNormBox` to handle RA wrap (polygon.c:4266-4314). Wrong for SkyFrame polygons crossing RA=0. |
| `RegBaseMesh` (polygon.c:4329) | `reg_base_mesh` (polygon.rs:420) | HEURISTIC | Rust distributes points by straight-line Euclidean edge length in raw coords (polygon.rs:432). C uses `astDistance` geodesic edge lengths (polygon.c:4430). Diverges on the sphere; also Rust omits the final closing-edge vertex handling nuance. |
| `Transform` (polygon.c:6235) | shared `region_transform` + `reg_pins` (data.rs:405) | MISSING-BRANCH | C `Transform` uses the cached interior point + `astLineDef`/`astLineCrossing`/`astLineContains` ray from the safe interior point (polygon.c:6386-6411), honouring `acw` and geodesic edges. Rust routes through the crossing-number `reg_pins`, so it inherits the raw-coordinate and orientation gaps. |
| `map_region` | `map_region` (polygon.rs:664) | MISSING-BRANCH | **Uncertainty dropped** (polygon.rs:677-683). Also does not re-run `EnsureInside`, so a map that flips handedness (e.g. a reflection) leaves the polygon mis-oriented. |
| `astDownsize` (polygon.c:1323) | — | ABSENT | The public vertex-reduction method (and its `Segment`/`DownsizePoly` machinery) has no Rust counterpart. Only `convex_hull_f32` (polygon.rs:206, an `astConvexF` analogue via Andrew monotone chain) exists — itself unvalidated against C's `ConvexHull`/`TraceEdge` scanline algorithm. |
| `Order`/`SimpVertices` attributes (polygon.c GetAttrib 2657, Dump 6633, load 7305) | — | ABSENT | No `Order` (vertex-order serialisation flag) or `SimpVertices` attribute in Rust. Affects Native round-trip fidelity and the Simplify bend-check. |
| `SkyArea` (polygon.c:5469) | — | ABSENT | Needed by `EnsureInside` for SkyFrame polygons; absent. |
| `RegBasePick` (default → NULL) | default `None` (mod.rs:301) | PORTED | Correct. |
| `Equal` | `Mapping::equal` (polygon.rs:827) | MISSING-BRANCH | Compares `points` for exact vector equality; does not account for cyclic rotation of the vertex list or reversed order (which C treats via Negated). Two geometrically identical polygons with rotated vertex lists compare unequal. |

---

## NullRegion (`nullregion.c` ↔ `nullregion.rs`)

| C method (file:line) | Rust (file:line) | Status | Gap description |
|---|---|---|---|
| `Overlap`/`OverlapX` (nullregion.c:798/874) | `overlap` → `compute_overlap` (nullregion.rs:132; overlap.rs:40-68) | PORTED (partial) | The exact enumerated cases (both null → 5/6 by Negated equality; empty → 1; universal-that → 2; universal-this → 3; else 4) are reproduced by `compute_overlap`'s NullRegion special-casing (overlap.rs:44-68), but the "negation-of-each-other" result (C value 6, `Overlap::Same`-vs-distinct) is collapsed to `Same` — the port has no distinct enum for value 6. Also relies on the `is_null_empty`/`is_null_universal` mesh sniffing (overlap.rs:162) rather than a class check, so a bounded region with an all-BAD mesh could be misclassified. |
| `Simplify` (nullregion.c:1314) | `Mapping::simplified` (nullregion.rs:446) | PORTED | Faithful: UnitMap → clone; else rebuild NullRegion in current Frame. Rust preserves flags. C additionally re-copies uncertainty via `astGetUnc` (nullregion.c:1388); Rust drops uncertainty here (nullregion.rs:459-465). MISSING-BRANCH on the uncertainty copy. |
| `MergeNullRegion` (nullregion.c:571) | `region_merge` (nullregion.rs:291) | PORTED | Faithful port including the guard (same Closed, matching uncertainty presence, equal Negated) and the Prism-combination of uncertainties. Documented divergence: Rust stores the Prism unsimplified. |
| `MapMerge` (nullregion.c:338) | `map_merge`/`try_merge` (nullregion.rs:522/475) | PORTED | Routed through the shared `region_map_merge`; parallel-only merge documented. |
| `RegBasePick` (nullregion.c:1106) | `reg_base_pick_axes` (nullregion.rs:263) | PORTED | Faithful: picks base axes, selects uncertainty axes and drops if not a Region, rebuilds a NullRegion. Matches nullregion.c:1167-1189. |
| `GetDefUnc` (nullregion.c:170) | — | ABSENT | C returns a default uncertainty Region (a small Box) for a NullRegion. Rust has no `get_def_unc`; `get_uncertainty` returns only an explicitly-set uncertainty (nullregion.rs:244). Downstream `RegPins`/`Overlap` uncertainty bands therefore cannot be defaulted. |
| `RegBaseBox` (nullregion.c:961) | `reg_base_box` (nullregion.rs:97) | PORTED | Returns BAD bounds on all axes. Matches C (unbounded). |
| `RegBaseMesh` (nullregion.c:1024) / `RegMesh` (1266) | `reg_base_mesh` (nullregion.rs:103) / `get_region_mesh` (206) | PORTED (approx) | Rust returns empty base mesh and a single BAD point for the region mesh. C returns a single BAD point too; acceptable. |
| `map_region` | `map_region` (nullregion.rs:248) | MISSING-BRANCH | **Uncertainty dropped** (nullregion.rs:252-258). For NullRegion the geometry is trivial so only the uncertainty loss matters, but it is still a divergence from `astMapRegion`'s `astCopy`. |
| `is_bounded` | `is_bounded_inner` → false (nullregion.rs:112) | MISSING-BRANCH | Always `false`. C `astGetBounded` for a NullRegion returns `Negated` (a negated NullRegion is the whole space — still "unbounded" but the point-membership is universal). The bounded flag itself is fine as false, but `point_in_region` correctly returns `negated` (nullregion.rs:129). |

---

## Blocking dependencies

### cap_slamap_02 — Polygon from Box corners (`box.c:3917`)
`Box::Simplify` transforms 4 corners into 2-D and constructs a Polygon,
then validates the fit with `astRegPins` on the boundary mesh. The Rust
`BoxRegion::simplified` (box_region.rs:940) has **no** 2-D-Box→Polygon
branch at all — it either returns a re-fitted Box, a Prism, or the
original (box_region.rs:976-986). Even if that branch were added, it
would fail because:

1. **`Polygon::reg_pins` is an inside test, not the on-boundary test**
   C `astRegPins` performs (polygon.c:4737). The validation step
   (`verify_remap_exact`, data.rs:313, which calls `reg_pins`) would
   accept/reject on wrong grounds.
2. **`EnsureInside` is absent** (polygon.rs constructor at 162): Box
   corners come out in a fixed winding that may be clockwise; without
   vertex-order normalisation + Negated toggle the resulting Polygon is
   mis-oriented and `GetBounded` (hardcoded `true`, polygon.rs:482) can't
   detect it.
3. **`Polygon::Simplify` lacks the `nout==2` guard and the bend
   validation** (polygon.rs:795), so it cannot be relied on to confirm
   the corner-polygon is exact.

**Minimum work to close cap_slamap_02:** add the on-boundary `RegPins`
(parallel/perp resolve per edge, polygon.c:4562), port `EnsureInside` +
`acw`/`Cache` interior-point (polygon.c:575, 1768), fix `GetBounded`
(polygon.c:2749), and add the 2-D-Box→Polygon branch to
`BoxRegion::simplified`.

### cap_slamap_07 — CmpRegion AND of disjoint → NullRegion
Depends on `NullRegion` construction/exclusion, which is largely PORTED:
`region_merge` (nullregion.rs:291), `Simplify` (439), `RegBasePick`
(256), and the enumerated `Overlap` cases (overlap.rs:44-68) are faithful.
The residual risks are (a) `GetDefUnc` ABSENT (no default uncertainty), and
(b) the `Overlap` value-6 (mutual negation) collapse to `Same`. Neither
blocks a plain "AND of disjoint circles → empty NullRegion" reduction, but
the disjoint decision itself flows through the **heuristic mesh-threshold
`compute_overlap`** (overlap.rs), whose `THRESHOLD=0.90` boundary-noise
tolerance (overlap.rs:21) is not a faithful `astRegPins` port and may
mis-call near-tangent regions. That is the real cap_slamap_07 exposure
(consistent with the existing `overlap_is_heuristic_not_port` memory note).

---

## Cross-cutting findings

### Uncertainty dropped in `map_region` (per class)
C `astMapRegion` deep-copies the whole Region including its uncertainty
(`result = astCopy(usethis)`, region.c:5704). Every Rust `map_region`
constructs a fresh `RegionData` and never copies `self.data.uncertainty`:
- Circle: circle.rs:486-492 (no `data.uncertainty = ...`).
- Ellipse: ellipse.rs:447-453.
- Polygon: polygon.rs:677-683.
- NullRegion: nullregion.rs:252-258.

This is the same cross-cutting gap already recorded for other region
classes in the `map_region drops uncertainty` memory note; it applies to
all four classes here. Additionally all four omit the C forward/inverse
bad-value validation (region.c:5507-5551) and the `nnew < nold` MapSplit
axis-reduction (region.c:5593).

### `reg_pins` heuristic status (per class)
None of the four classes implement C's on-boundary `astRegPins`. Instead
`reg_pins` is an **inside** predicate in every case:
- Circle (circle.rs:182): `dist <= radius`, uncertainty ignored.
- Ellipse (ellipse.rs:194): normalised radius `<= 1`, uncertainty ignored.
- Polygon (polygon.rs:278): crossing-number inside test with `EPSILON`
  on-edge fudge, uncertainty ignored.
- NullRegion (nullregion.rs:92): always `false` (correct for the inside
  test; C's on-boundary test is trivially "no boundary").

Because `Simplify` validation (`verify_remap_exact`, data.rs:313) and the
overlap engine both need the *on-boundary* semantics C's `astRegPins`
provides, the current inside-only `reg_pins` cannot faithfully drive
Region simplification. The uncertainty-band width (`0.5*(l1+l2)` from the
two uncertainty-box diagonals, circle.c:1448 / polygon.c:4735) is nowhere
computed in the Rust port.

### Overlap engine
`compute_overlap` (overlap.rs:40) is a shared mesh-sampling heuristic with
a fixed 90% agreement threshold (overlap.rs:21) plus a bounding-box
quick-reject (overlap.rs:56). It is **not** a port of region.c `OverlapX`,
which uses `astRegPins`/mesh containment with uncertainty. NullRegion's
exact enumerated cases are layered on top (overlap.rs:44-68). Consequence:
`Partial` vs `FirstInside`/`SecondInside` calls near tangency are
tolerance-driven, not exact.

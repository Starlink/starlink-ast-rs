# Region-class coverage audit — post-Phase-5 gap sweep

Ticket DM-54793.
Date: 2026-08-02, against HEAD `51984b4`.

## Why this document exists

The Phase 5 parity work (`region-phase5-parity-spec.md`) closed the cross-cutting
simplify/uncertainty machinery gaps surfaced by the simplify-fixture corpus.
This audit is the broader follow-up: a function-by-function sweep of every C
region class's `vtab->Method` overrides against the Rust trait and impls, to find
what the fixture-driven work did *not* reach.

None of the findings below is a current red gate — each is either latent (no
fixture exercises it yet) or gated behind a subsystem that is not yet ported
(Plot/Grf, STC-S). They are recorded here so they are not rediscovered from
scratch when those subsystems land.

The 8 already-closed Phase-5 gaps (RB-PICK, MERGE-FF, SETUNC, DEFUNC, SIMP-STAB,
CE-FIT, R-UNC, PR-MAP) are out of scope — see `region-phase5-parity-spec.md`.

## Findings

| ID | Class(es) | Kind | Gated behind | Size | Verified |
|---|---|---|---|---|---|
| MESH-GEO | Polygon | divergent control flow | — (latent) | medium | yes |
| PLY-DOWN | Polygon | missing method + Simplify branch | — (latent) | medium | partial |
| GRID-FRM | Box + base default | missing override | — (latent) | medium | reported |
| BND-INT | Interval | missing override | — (latent) | medium | reported |
| RT-TRACE | base + 6 classes | unported method | Plot/Grf | large | reported |
| RS-SPLIT | CmpRegion | unported override | Plot/Grf | medium | reported |
| ACC-PARS | 7 classes | unported accessors | STC-S | small each | reported |

"Verified" = whether the C↔Rust divergence was confirmed by reading both source
trees during this audit ("yes"), spot-checked ("partial"), or taken from the
sweep and still to be confirmed at implementation time ("reported").

---

### MESH-GEO — Polygon `reg_base_mesh` is planar, not geodesic *(verified)*

- **Rust**: `regions/polygon.rs:623` — edge lengths as Euclidean
  `sqrt(dx²+dy²)`, mesh points by linear interpolation `dx/n, dy/n` between
  vertices, per-edge count `((elen*ppd).round()).max(1)`.
- **C**: `polygon.c:4329` `RegBaseMesh` — edge length via `astDistance`
  (geodesic), mesh points via `astOffset` (great-circle interpolation), each
  vertex `astNorm`'d, per-edge count `n = 1 + (int)(len*mp)`.

For a Polygon on a SkyFrame the boundary should follow great circles; Rust
places mesh points on straight lines in axis space. Circle (`circle.rs:388`) and
Ellipse already use `offset2` correctly — Polygon is the outlier. The per-edge
point-count formula also differs (`round().max(1)` vs `1 + (int)(len*mp)`), so
the mesh can differ even for a Cartesian frame.

**Impact**: wrong mesh geometry for sky polygons feeds `overlap()` / `reg_pins`
/ mask accuracy and any mesh-based comparison; relevant to MOC and sky-region
correctness. No current fixture uses a SkyFrame polygon mesh, so it is latent
today. A faithful port replaces the Euclidean length with `frame.distance` and
the linear step with `frame.offset`, and adopts the C count formula.

### PLY-DOWN — Polygon `Downsize` / `DownsizePoly` unported *(partial)*

- **Rust**: absent — no `downsize` in `polygon.rs` (only `convex_hull_f32`).
- **C**: `polygon.c:1323` `Downsize` (public `astDownsize`) + `DownsizePoly`
  helper (`polygon.c:1425`), the latter used inside polygon `Simplify`
  (`polygon.c:3665`).

The public `astDownsize` has only Fortran callers, so it is not needed until a
Fortran layer exists. The parity-relevant part is the *internal* `DownsizePoly`
use inside C polygon `Simplify`: if the Rust polygon `Simplify` omits the
vertex-reduction step, simplified polygons could keep more vertices than C — a
serialization-parity risk on polygon round-trips.

**Action before this is scheduled**: confirm whether the current Rust polygon
`Simplify` reaches a vertex-reduction step. If it does not, this becomes a real
(if currently unexercised) parity gap and should be promoted.

### GRID-FRM — `reg_base_grid` uses a generic odometer grid *(reported)*

- **Rust**: `regions/data.rs:354` `reg_base_grid` — uniform rectangular odometer
  grid in the bounding box, filtered by `reg_base_inside`. No `basegrid` cache.
- **C**: base `region.c:7964`; Box overrides at `box.c:2179` calling
  `astFrameGrid(frm, meshsize, lbnd, ubnd)` (Frame-aware, spherical-uniform) and
  caching the result in `this->basegrid`.

Grid-point *distribution* differs from C on non-Cartesian frames. Fewer callers
than the mesh path; no current test compares grid layout. Requires porting
`astFrameGrid` (a Frame method), so it is coupled to Frame work.

### BND-INT — Interval `BndBaseMesh` override missing *(reported)*

- **Rust**: generic `bnd_base_mesh` in `regions/overlap.rs:365` (builds a
  CmpRegion AND of region ∩ box and meshes it); Interval has no own override.
- **C**: `interval.c:215` `BndBaseMesh` — class-specific construction from the
  finite interval bounds, handling unbounded/semi-infinite axes directly.

The generic path may diverge for partially-unbounded Intervals; used inside
overlap (`overlap.rs:308`). Worth checking whether `test_interval.rs` covers
semi-infinite overlap before deciding this needs the bespoke override.

### RT-TRACE — `RegTrace` boundary tracing entirely unported *(reported)*

- **Rust**: absent — no `reg_trace` anywhere; the `Region` trait has no such
  method.
- **C**: base `region.c:11601` (returns 0 = "not implemented"); real overrides
  in `box.c:3216`, `circle.c:1544`, `ellipse.c:1631`, `polygon.c:4820`,
  `interval.c:2887`, `cmpregion.c:2685`.

`astRegTrace` returns parametrized boundary positions (dist 0..1). Sole caller is
`plot.c` (drawing region outlines). **Hard blocker once the Plot backend lands**;
no serialization or transform impact today. Large (6 class-specific ports plus
trait plumbing).

### RS-SPLIT — CmpRegion `RegSplit` unported *(reported)*

- **Rust**: absent — no `reg_split`. The trivial base clone-self behaviour is
  effectively available via `clone_region`.
- **C**: base `region.c:8154` (clones self); CmpRegion override `cmpregion.c:2472`
  (walks the OR-tree into a disjoint-component list).

Callers are `plot.c:9700` and internally `cmpregion.c:2561`. Same
"blocks plotting later" status as RT-TRACE.

### ACC-PARS — class geometry accessors unexposed *(reported)*

Missing: `CirclePars`, `EllipsePars`, `BoxPoints`, `IntervalPoints`,
`PointListPoints`, `CmpRegionList`, `astPolygonPoints` (C sources: `circle.c`,
`ellipse.c`, `box.c`, `interval.c`, `pointlist.c`, `cmpregion.c`, `polygon.c`).
These return a region's defining parameters. External callers are almost entirely
`stcschan.c` (STC-S serialization) plus Fortran shims — STC-S is not yet ported,
so these are gated behind that. Small each.

---

## Clean bills of health

Confirmed present and matching C during the sweep: RegBasePick (Circle/Ellipse/
Polygon correctly inherit the default; Box/PointList/NullRegion/Interval/
CmpRegion/Prism override), RegCentre, RegBaseBox, RegPins, Overlap/OverlapX,
GetBounded, GetDefUnc, GetRegionBounds, MapRegion, RegSetAttrib/RegClearAttrib,
Simplify, MapMerge, and Prism Decompose/MapList.

`Mapping::invert` is a no-op on every region class, which is **correct** — a
Region's forward and inverse transforms are identical (membership is
direction-independent) and C's `region.c` does not override `vtab->Invert`. The
stale "no-op for now" comments implying incompleteness were reworded to state the
invariant (commit landing with this doc).

### Lower-confidence note — PointList `Mask`

C `PointList` overrides `Mask` (`pointlist.c:800`) to stamp only the discrete
listed points rather than an area; Rust `masking.rs` is generic/region-agnostic.
This is a *possible* divergence but PointList masking is an edge case — flagged
for confirmation only if PointList mask parity is required.

## Recommended handling

- **MESH-GEO** and **PLY-DOWN** are the only two that can bite current transform
  or serialization parity if a fixture reaches them. Confirm PLY-DOWN's Simplify
  usage; port MESH-GEO when a spherical-polygon fixture is added (per the
  project rule, missing coverage means *add the fixture*, not skip the branch).
- **RT-TRACE**, **RS-SPLIT**, **ACC-PARS** are deferred until their gating
  subsystems (Plot/Grf, STC-S) are scheduled; they are listed in `PLAN.md` so
  they surface at that point.
- **GRID-FRM**, **BND-INT** are latent parity items to fold into the next Frame
  and overlap passes respectively.

# Regions Design Spec

## Summary

Implements step 9 of the delivery plan: all geometric Region classes with
full shared infrastructure (integrity, bounds, mesh, overlap, masking).
Excludes Moc/MocChan/StcsChan (separate spec).

**Scope:** 9 concrete classes — Circle, BoxRegion, Ellipse, Polygon,
Interval, CmpRegion, NullRegion, PointList, Prism — plus the shared
Region infrastructure (~14k lines of `region.c` logic).

## Architecture

### Approach: Shared RegionData + Internal Geometry Trait

Each concrete Region owns a `RegionData` struct containing shared state
(internal FrameSet, defining points, flags, caches). A private
`RegionGeometry` trait provides per-class virtual functions (point-in
predicate, bounding box, mesh generation). The public `Region` trait
delegates to both.

This mirrors C's `region.c` pattern: a base struct with shared state plus
per-class virtual function pointers.

### Public Region Trait

```rust
pub trait Region: Frame + Mapping {
    // --- Core predicate ---
    /// Test whether a point (in current frame) is inside this region.
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool>;

    // --- Overlap ---
    /// Determine the overlap between this region and another.
    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap>;

    // --- Negation ---
    /// Return a negated copy of this region.
    fn negated_copy(&self) -> DynRegion;
    /// Negate this region in place.
    fn negate(&mut self);
    /// Whether this region is negated.
    fn is_negated(&self) -> bool;

    // --- Attributes ---
    fn is_closed(&self) -> bool;
    fn set_closed(&mut self, closed: bool);
    fn fill_factor(&self) -> f64;
    fn set_fill_factor(&mut self, f: f64);
    fn mesh_size(&self) -> usize;
    fn set_mesh_size(&mut self, n: usize);
    fn is_bounded(&self) -> bool;

    // --- Bounds ---
    /// Bounding box in current frame.
    fn get_region_bounds(&self, lbnd: &mut [f64], ubnd: &mut [f64]) -> AstResult<()>;
    /// Enclosing disc (centre + radius) for quick overlap pre-filtering.
    fn get_region_disc(&self, centre: &mut [f64], radius: &mut f64) -> AstResult<()>;

    // --- Mesh ---
    /// Mesh of points on boundary (surface=false) or interior grid (surface=true).
    fn get_region_mesh(&self, surface: bool) -> AstResult<Vec<Vec<f64>>>;
    /// The defining points of this region (class-specific meaning).
    fn get_region_points(&self) -> AstResult<Vec<Vec<f64>>>;

    // --- Frame access ---
    /// The current frame of this region.
    fn get_region_frame(&self) -> DynFrame;
    /// The full internal FrameSet (base → current).
    fn get_region_frameset(&self) -> FrameSet;

    // --- Uncertainty ---
    fn set_uncertainty(&mut self, unc: Option<DynRegion>);
    fn get_uncertainty(&self, current: bool) -> Option<DynRegion>;

    // --- Remapping ---
    /// Map this region into a new coordinate frame.
    fn map_region(&self, map: &dyn Mapping, frame: &dyn Frame) -> AstResult<DynRegion>;
}
```

### RegionData (shared internal state)

```rust
pub(crate) struct RegionData {
    /// Internal FrameSet: frame 1 = base frame (defining points live here),
    /// frame 2 = current frame (public interface).
    frameset: FrameSet,
    /// Defining points in base-frame coordinates.
    /// Layout is class-specific (e.g. [centre, edge] for Circle).
    points: Vec<Vec<f64>>,
    /// Whether the region is negated (inside ↔ outside swapped).
    negated: bool,
    /// Whether the boundary is part of the region.
    closed: bool,
    /// Target point count for mesh generation.
    mesh_size: usize,
    /// Fill factor (0.0..=1.0), used by overlap weighting.
    fill_factor: f64,
    /// Does the region adapt defining points when frame attributes change?
    adaptive: bool,
    /// Optional uncertainty region (defines tolerance for boundary tests).
    uncertainty: Option<Box<dyn Region>>,
    /// Cached base-frame mesh (lazily computed, invalidated on mutation).
    cached_base_mesh: RefCell<Option<Vec<Vec<f64>>>>,
}
```

### RegionGeometry (per-class virtual functions)

```rust
pub(crate) trait RegionGeometry {
    fn region_data(&self) -> &RegionData;
    fn region_data_mut(&mut self) -> &mut RegionData;

    /// Batch predicate: which points (in base frame) are inside?
    /// `unc` is the uncertainty region (for boundary tolerance).
    fn reg_pins(&self, points: &[&[f64]], unc: Option<&dyn Region>) -> AstResult<Vec<bool>>;

    /// Tight bounding box in base frame.
    fn reg_base_box(&self, lbnd: &mut [f64], ubnd: &mut [f64]) -> AstResult<()>;

    /// Generate mesh in base frame.
    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>>;

    /// Centre of region in base frame (if meaningful).
    fn reg_centre(&self) -> Option<Vec<f64>>;

    /// Whether the region is bounded (finite extent).
    fn is_bounded_inner(&self) -> bool;

    /// Remap defining points after integrity delta (class-specific).
    fn remap_defining_points(&mut self, delta: &dyn Mapping) -> AstResult<()>;
}
```

## Region Integrity

Mirrors FrameSet integrity. When a frame attribute is mutated through a
Region, the defining points are auto-remapped to preserve the geometric
relationship:

1. Snapshot the current frame.
2. Mutate the current frame via FrameSet integrity (updates base→current mapping).
3. Compute the old-current→new-current mapping via `map_from`.
4. Transform defining points: map base→old-current (old mapping), apply
   delta (old→new), map new-current→base (inverse of new mapping).
   This is `remap_defining_points` — per-class because some classes
   (Circle) remap centre and recompute radius, while others (Polygon)
   remap all vertices.
5. Invalidate cached meshes.

The `adaptive` flag controls whether step 4 happens. When `false`, the
defining points stay fixed in base frame and only the current-frame
representation changes. Default is `true`.

Escape hatch: `frame_mut()` exposes the raw current frame without
integrity enforcement, matching the design spec.

## Mapping::transform for Regions

All regions implement `Mapping::transform` as the inside/outside predicate
mapper. Shared implementation:

1. Transform input points into base frame (via internal FrameSet mapping).
2. Call `reg_pins` (batch predicate — per-class geometry).
3. Apply negation flag.
4. Build output: passthrough for "inside" points, `BAD_VALUE` for "outside".

The forward and inverse transforms are identical (the region predicate
is frame-direction-independent for the masking use case). `nin` = `nout`
= `naxes`.

Performance: `reg_pins` operates on batches to allow vectorisation.
No allocation in the inner loop beyond the output buffer. For simple
frames, the coordinate test is direct arithmetic.

## Concrete Classes

### Circle

- **Fields:** `RegionData` + `cached_radius: Cell<Option<f64>>`
- **Defining points:** `[centre, edge_point]` in base frame
- **Constructor:** `CircleBuilder::new(frame, centre, radius)` or `from_points(frame, centre, edge_point)`
- **reg_pins:** `Frame::distance(centre, point) ≤ radius` (geodesic-aware on SkyFrame)
- **reg_base_box:** centre ± radius per axis (flat); great-circle-aware for sky
- **reg_base_mesh:** evenly-spaced points around circumference

### BoxRegion

- **Fields:** `RegionData` only
- **Defining points:** `[centre, corner]` in base frame
- **Constructor:** `BoxRegionBuilder::new(frame, centre, corner)` or `from_bounds(frame, lbnd, ubnd)`
- **reg_pins:** per-axis `lbnd ≤ x ≤ ubnd` test
- **reg_base_box:** directly from defining corner + mirror around centre
- **reg_base_mesh:** points along each edge
- Note: on sky, edges are great circles of constant longitude/latitude

### Ellipse

- **Fields:** `RegionData` only
- **Defining points:** `[centre, point_on_semimajor, point_on_boundary]`
- **Constructor:** `EllipseBuilder::new(frame, centre, semi_axes, angle)` or from 3 points
- **reg_pins:** normalised-ellipse distance ≤ 1 (rotation + scaling to unit circle)
- **reg_base_box:** from semi-axes + rotation angle
- **reg_base_mesh:** parametric ellipse sweep

### Polygon

- **Fields:** `RegionData` only
- **Defining points:** N vertices (ordered)
- **Constructor:** `PolygonBuilder::new(frame, vertices)`
- **reg_pins:** winding-number test (ray-casting for flat, geodesic-aware for sky)
- **reg_base_box:** min/max of vertices per axis
- **reg_base_mesh:** interpolation along edges
- Additional: `convex_hull()` constructor from point cloud

### Interval

- **Fields:** `RegionData` only
- **Defining points:** `[lower_bounds, upper_bounds]` — `BAD_VALUE` = unbounded
- **Constructor:** `IntervalBuilder::new(frame, lbnd, ubnd)`
- **reg_pins:** per-axis range test, `BAD_VALUE` bounds treated as ±∞
- **reg_base_box:** the bounds themselves (with BAD → ±MAX)
- **reg_base_mesh:** corner points + edge samples

### NullRegion

- **Fields:** `RegionData` only
- **Defining points:** none
- **Constructor:** `NullRegionBuilder::new(frame)`
- **reg_pins:** always `false` (empty region; negated = universal)
- **is_bounded:** `false`
- **reg_base_mesh:** empty

### CmpRegion

- **Fields:** `RegionData` + `region1: DynRegion` + `region2: DynRegion` + `operator: CmpOperator`
- **Operator:** `CmpOperator::And` (intersection) or `CmpOperator::Or` (union)
- **Constructor:** `CmpRegionBuilder::new(region1, region2, operator)`
- **reg_pins:** apply operator to sub-region results. Short-circuit where possible.
- **reg_base_box:** intersection (AND) or union (OR) of sub-region boxes
- **reg_base_mesh:** merge sub-region meshes, filter by combined predicate

### PointList

- **Fields:** `RegionData` only
- **Defining points:** N discrete points
- **Constructor:** `PointListBuilder::new(frame, points)`
- **reg_pins:** exact match within uncertainty tolerance
- **is_bounded:** `true`
- **reg_base_mesh:** the points themselves

### Prism

- **Fields:** `RegionData` + `region1: DynRegion` + `region2: DynRegion`
- **Constructor:** `PrismBuilder::new(region1, region2)` — region2 extends region1 along extra axes
- **Combined naxes:** `region1.naxes() + region2.naxes()`
- **reg_pins:** split input axes, test each sub-region on its own axes, AND results
- **reg_base_box:** concatenate sub-region boxes
- **reg_base_mesh:** Cartesian product of sub-region meshes (filtered)

## Shared Infrastructure

### Bounding Box

Two levels:
- `reg_base_box()` — per-class, tight bounds in base frame.
- `get_region_bounds()` — public, transforms base-box corners to current
  frame, takes enclosing box. Sky-frame-aware (handles 0/2π wrapping).

### Mesh Generation

- `reg_base_mesh()` — per-class, generates `mesh_size` points in base frame.
- `get_region_mesh()` — public, transforms base mesh to current frame.
- Cached in `RegionData`; invalidated on any mutation that changes geometry.
- `mesh_size` default: 200.

### Overlap Algorithm

Mesh-based approximate overlap (matches C):
1. Quick reject: if bounding boxes don't intersect → `Overlap::None`.
2. Map both regions to a common frame (via `convert`).
3. Generate meshes for both regions.
4. Test mesh-A points against region-B predicate.
5. Test mesh-B points against region-A predicate.
6. Classify:
   - All A inside B AND all B inside A → `Same`
   - All A inside B → `FirstInside`
   - All B inside A → `SecondInside`
   - Some inside → `Partial`
   - None inside → `None`

Uncertainty region used as tolerance for boundary-adjacent points.

### Masking

Generic over element type:
```rust
pub fn mask<T: Copy>(
    region: &dyn Region,
    mapping: &dyn Mapping,   // grid coords → region frame
    inside: bool,            // mask inside or outside?
    lbnd: &[i64],           // array lower bounds
    ubnd: &[i64],           // array upper bounds
    data: &mut [T],         // flattened data array
    val: T,                 // replacement value
) -> AstResult<usize>      // count of masked pixels
```

Processes in row-batches for efficiency. Transforms grid coordinates
through the mapping, tests against region predicate, replaces values.

### Simplification

`Region::simplify()` returns a simplified equivalent:
- CmpRegion(A AND B) where B fully contains A → A
- CmpRegion(A OR B) where B fully contains A → B
- Prism of Intervals → single Interval
- Region with identity base→current mapping → drop the mapping
- Reuses existing `Mapping::simplify()` on the internal FrameSet mapping.

## Serialization

Each class implements `Serializable` (`to_node` / `from_node`). Native
format serializes:
1. Class name token (e.g., `"Circle"`, `"Box"`, `"CmpRegion"`)
2. Region attributes: Negated, Closed, MeshSize, FillFactor, Adaptive
3. Internal FrameSet (base → current mapping)
4. Defining points
5. Uncertainty region (if set)
6. Class-specific data (e.g., CmpRegion operator)

`AnyObject` extended with 9 new variants. New accessor:
`as_region() -> Option<&dyn Region>`.

Native round-trip must be byte-exact with C library output.

## Performance Requirements

- `reg_pins` batch predicate: within 10% of C for Circle and Box on
  large point sets (measured in `ast-oracle-bench`).
- `Mapping::transform` (masking path): same constraint.
- No allocations in inner predicate loop (pre-allocate output buffer).
- Batch processing enables auto-vectorisation of simple arithmetic tests.
- `CmpRegion` short-circuits per point in the batch: for AND, if
  region1 says "out" for a point, skip region2 test for that point.
  For OR, if region1 says "in", skip region2. Implemented by testing
  region1 first, then only testing region2 for points that need it.

## Testing

### Tier 1 (Rust unit tests per class)

Per-class: construction, point_in_region, bounds, negation, closed/open,
serialization round-trip. Integrity tests for each class. Overlap matrix.

### Tier 2 (native format regression)

Byte-exact round-trip for region fixtures from `ast_tester/`.

### Port of testregions.c

`crates/ast-core/tests/testregions.rs` — full port covering:
- `checkBox`, `checkCircle`, `checkEllipse`, `checkPolygon`
- `checkInterval`, `checkNullRegion`, `checkCmpRegion`
- `checkPointList`, `checkPrism`
- `generalChecks` (bounds, overlap, MapRegion, RemoveRegions)
- `checkConvex` (convex hull)

### Tier 5 (oracle bench)

`ast-oracle-bench`: Circle and Box `reg_pins` with N=10000 points,
Rust vs C. Target: ≤10% slower.

## File Layout

```
crates/ast-core/src/regions/
├── mod.rs           # Region trait, Overlap enum, DynRegion, re-exports
├── data.rs          # RegionData struct, RegionGeometry trait
├── bounds.rs        # Bounding box shared logic
├── mesh.rs          # Mesh generation shared logic
├── overlap.rs       # Overlap algorithm
├── masking.rs       # Generic mask() implementation
├── circle.rs
├── box_region.rs
├── ellipse.rs
├── polygon.rs
├── interval.rs
├── cmpregion.rs
├── nullregion.rs
├── pointlist.rs
└── prism.rs
```

## Out of Scope

- **Moc / MocChan / StcsChan** — separate design, different architecture
  (HEALPix hierarchical coverage vs geometric regions).
- **Plot/Grid integration** — step 10, depends on regions but designed separately.
- **RemoveRegions mapping simplification** — included as part of the
  simplify infrastructure but full implementation deferred if complex.

# SelectorMap & SwitchMap Implementation Design

**Date:** 2026-05-11
**Status:** Approved
**Scope:** Complete the public mapping classes by implementing SelectorMap and SwitchMap

## Overview

SelectorMap and SwitchMap are companion mappings that together enable
region-based coordinate routing. A SelectorMap identifies which region
contains a given point; a SwitchMap uses a selector to dispatch points
to different route mappings based on their location in coordinate space.

These are the final two public mapping classes needed to complete the
mapping layer of the Rust port.

## SelectorMap

### Data Model

```rust
pub struct SelectorMap {
    regions: Vec<DynRegion>,
    badval: f64,
    inverted: bool,
}
```

### Constructor

```rust
pub struct SelectorMapBuilder { ... }

impl SelectorMapBuilder {
    pub fn new(regions: Vec<DynRegion>) -> AstResult<Self>;
    pub fn badval(mut self, val: f64) -> Self;
    pub fn build(self) -> AstResult<SelectorMap>;
}
```

**Validation:**
- At least one region required
- All regions must have the same `naxes()`

### Mapping Trait

- `nin()` = region dimensionality (number of axes)
- `nout()` = 1
- `is_inverted()` / `invert()`: standard flag
- `tran_forward` = true, `tran_inverse` = false

### Transform (forward only)

For each input point:
1. Check for any BAD input coordinate → output `badval`
2. Iterate regions in order (1..=nreg)
3. Use the Region's transform (negate + check BAD output) to determine containment
4. Return the 1-based index of the first containing region
5. Return 0.0 if no region contains the point

The algorithm follows the C implementation's pattern of using negated
region transforms to identify interior points efficiently across batches:

1. Copy input points to working buffer
2. For each region: negate it, transform working points, identify newly-BAD
   outputs as interior points, assign their index, swap buffers
3. Replace sentinel -1 (bad input) with `badval`

The inverse transform produces all-BAD output (not defined).

### Simplification

1. Simplify each child region; if any changed, rebuild the SelectorMap
2. In series: if adjacent to an inverted SelectorMap that is equal when
   un-inverted, replace the pair with a UnitMap(nin)

### Serialization

Native tokens (matching C library byte-exact):
- `Reg1`, `Reg2`, ... — child Region objects (Reg2+ omit their FrameSet via RegionFS=0)
- `BadVal` — f64, omitted when equal to `BAD_VALUE` (-f64::MAX)

### Accessors

- `regions(&self) -> &[DynRegion]`
- `badval(&self) -> f64`

## SwitchMap

### Data Model

```rust
pub struct SwitchMap {
    fsmap: Option<DynMapping>,
    ismap: Option<DynMapping>,
    route_maps: Vec<DynMapping>,
    fsinv: bool,
    isinv: bool,
    route_inv: Vec<bool>,
    inverted: bool,
}
```

### Constructor

```rust
impl SwitchMap {
    pub fn new(
        fsmap: Option<DynMapping>,
        ismap: Option<DynMapping>,
        route_maps: Vec<DynMapping>,
    ) -> AstResult<Self>;
}
```

**Validation:**
- At least one route mapping required
- All route mappings must have the same `nin()` and `nout()`
- If `fsmap` provided: `fsmap.nin()` == route `nin()`, `fsmap.nout()` == 1
- If `ismap` provided: `ismap.nout()` == route `nout()`, `ismap.nin()` == 1
- At least one of `fsmap` or `ismap` must be provided

### Mapping Trait

- `nin()` = route mappings' common `nin()` value
- `nout()` = route mappings' common `nout()` value
- (These may differ — route mappings can be N→M transforms)
- When inverted, nin/nout swap.
- `tran_forward` = `fsmap.is_some()`
- `tran_inverse` = `ismap.is_some()`

### Transform (forward)

1. Get the appropriate selector mapping (forward selector for forward
   transform, inverse selector for inverse transform — swapped when
   the SwitchMap is itself inverted)
2. Transform all input points through the selector → get selector values
3. Round each selector value to nearest integer (using `f64::round()`)
4. Group points by route index
5. For each route that has points:
   - Collect the input points for that route into a batch
   - Apply the route mapping's transform
   - Scatter results back to correct positions in output
6. Points with out-of-range or BAD selector values get all-BAD output

This batched approach matches the C implementation and avoids per-point
dispatch overhead.

### Transform (inverse)

Same algorithm but uses the inverse selector mapping and the inverse
direction of the route mappings.

### Rate Override

1. Transform the input position through the forward selector to get
   the route index
2. Round to nearest integer
3. If valid route index: delegate to that route mapping's `rate()`
4. If invalid: return `BAD_VALUE`

### RemoveRegions

Recurse into `fsmap`, `ismap`, and all `route_maps`. If any child
changes, rebuild the SwitchMap with the new children. This enables
`remove_regions()` in `regions/mod.rs` to handle SwitchMaps.

### Simplification

1. **Cancel adjacent inverse pairs:** In series, if neighbor is a
   SwitchMap that is equal-but-inverted, replace both with UnitMap(nin)
2. **Normalize inversion:** If the SwitchMap's own Invert flag is set,
   create an equivalent non-inverted SwitchMap by swapping selectors
   and inverting all route flags
3. **Simplify children:** Simplify each selector and route mapping;
   if any changed, rebuild

### Serialization

Native tokens (matching C library byte-exact):
- `FSMap` — forward selector Mapping object (omitted if None)
- `FSInv` — int (0/1), forward selector's recorded invert flag
- `ISMap` — inverse selector Mapping object (omitted if None)
- `ISInv` — int (0/1), inverse selector's recorded invert flag
- `RMap1`, `RMap2`, ... — route Mapping objects
- `RInv1`, `RInv2`, ... — int (0/1), route recorded invert flags

### Accessors

- `forward_selector(&self) -> Option<&dyn Mapping>`
- `inverse_selector(&self) -> Option<&dyn Mapping>`
- `route_maps(&self) -> &[DynMapping]`
- `nroute(&self) -> usize`

## Integration Points

### Serialization Dispatch

Add to `mapping_from_node()` match arms:
```rust
"SelectorMap" => Ok(Box::new(SelectorMap::from_node(node)?)),
"SwitchMap" => Ok(Box::new(SwitchMap::from_node(node)?)),
```

Add to `MappingToNode` impl dispatch chain.

### remove_regions()

Extend `remove_regions()` in `regions/mod.rs` to handle SwitchMap:
downcast, recurse into selectors and route mappings, rebuild if changed.

SelectorMap does not need `remove_regions` handling — its child Regions
are the functional core (removing them would destroy semantics, not
simplify the transform chain). The C library does not override
`RemoveRegions` for SelectorMap either.

### Simplify Infrastructure

Both mappings implement the `simplified()` method via their MapMerge
logic. The simplify dispatcher in `simplify.rs` needs to recognize
these types for the adjacent-inverse-cancel pattern.

## Test Plan

### Unit Tests (in respective module files)

- Construction validation (dimension mismatches rejected)
- SelectorMap forward transform: point in region 1, region 2, no region, bad input
- SwitchMap forward/inverse: route dispatching, out-of-range selector, BAD handling
- Rate delegation for SwitchMap
- Invert flag handling for both

### Integration Test: `tests/switchmap.rs`

Port from `reference/starlink-ast/ast_tester/testswitchmap.c`:
- Section 1: overlapping spectral regions with WinMap routes
- Section 2: FITS WCS with SwitchMap (FitsChan round-trip)
- Serialization round-trip (checkdump pattern)
- Simplification (adjacent inverse cancellation)

### Simplify Fixtures

Copy from `reference/starlink-ast/ast_tester/simplify_fixtures/`:
- `selectormap_inverse_cancel.map` / `.simp`
- `switchmap_internal_simplify.map` / `.simp`
- `switchmap_inverse_cancel.map` / `.simp`
- `switchmap_invert_normalize.map` / `.simp`

## File Layout

```
crates/starlink-ast/src/mappings/
├── selectormap.rs          # SelectorMap struct, builder, Mapping impl
├── switchmap.rs            # SwitchMap struct, Mapping impl
└── mod.rs                  # Add pub mod + re-exports

crates/starlink-ast/tests/
├── switchmap.rs            # Integration test (ported from C)
└── fixtures/
    ├── selectormap_inverse_cancel.map
    ├── switchmap_*.map
    └── switchmap_*.simp
```

## Dependencies

These mappings depend on:
- The Region trait and concrete region types (complete)
- DynMapping / Mapping trait (complete)
- Serialization infrastructure (complete)
- Simplify infrastructure (complete)

No new external crate dependencies required.

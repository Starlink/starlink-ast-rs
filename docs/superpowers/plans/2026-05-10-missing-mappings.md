# Missing Mappings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement 9 remaining Mapping classes to unblock simplification and WCS conversion regression tests.

**Architecture:** Each mapping follows the established pattern: struct with fields + `impl Mapping` trait + `impl Serializable` trait + serialization dispatch wiring + simplify regression fixtures enabled. Transform logic is ported from the C reference (`reference/starlink-ast/src/<name>.c`).

**Tech Stack:** Rust 2024, `logos` crate (MathMap only), `ndarray` (existing dep), `ast-core` crate.

---

## File Structure

Each mapping creates/modifies these files:

| Per-mapping | Path |
|---|---|
| Implementation | `crates/ast-core/src/mappings/<name>.rs` |
| Module export | `crates/ast-core/src/mappings/mod.rs` (add `mod` + `pub use`) |
| Serialization dispatch | `crates/ast-core/src/serial/mod.rs` (two dispatch arms) |
| Unit test | `crates/ast-core/tests/test_<name>.rs` |
| Simplify manifest | `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt` (enable entries) |

Cross-cutting (once):
- `crates/ast-core/Cargo.toml` — add `logos` dependency (Phase 5 only)

---

## Reference Material

For every mapping, the implementor MUST study:
1. **C source:** `reference/starlink-ast/src/<name>.c` — the `Transform`, `Equal`, `MapMerge`, and `Dump`/`Load` functions define the behavior.
2. **C header:** `reference/starlink-ast/src/<name>.h` — struct definition and attribute list.
3. **Existing Rust patterns:** `crates/ast-core/src/mappings/zoom.rs` (simple), `sphmap.rs` (with try_merge), `cmpmap.rs` (wrapper with child mappings).
4. **Simplify fixtures:** The `.map` and `.simp` files in `tests/fixtures/native/reference/simplify_fixtures/` — these define the expected input→output for simplification.

---

## Task 1: PcdMap

**Files:**
- Create: `crates/ast-core/src/mappings/pcdmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Create: `crates/ast-core/tests/test_pcdmap.rs`

**C reference:** `reference/starlink-ast/src/pcdmap.c`

- [ ] **Step 1: Create PcdMap struct and constructor**

Create `crates/ast-core/src/mappings/pcdmap.rs`. Study the C struct in `pcdmap.h` (lines 217-236) for fields. The struct needs:

```rust
#[derive(Debug, Clone)]
pub struct PcdMap {
    disc: f64,
    pcdcen: [f64; 2],
    inverted: bool,
    do_not_simplify: bool,
}
```

Constructor: `PcdMap::new(disc: f64, pcdcen: [f64; 2]) -> AstResult<Self>`. No validation needed (disc can be zero — simplify handles that case).

- [ ] **Step 2: Implement Mapping trait — transform**

Port the `Transform` function from `pcdmap.c` lines 2216-2282:

Forward: `f = 1.0 + disc * (dx*dx + dy*dy); output = (dx*f + cen0, dy*f + cen1)`

Inverse (from SLA_UNPCD approximation):
```
cr2 = disc * (dxp*dxp + dyp*dyp)
a = (2*cr2 + 1) / (3*cr2 + 1)
cr2a2 = cr2 * a * a
f = (2*cr2a2*a + 1) / (3*cr2a2 + 1)
output = (dxp*f + cen0, dyp*f + cen1)
```

`nin() = 2`, `nout() = 2`.

- [ ] **Step 3: Implement equal()**

Study `Equal` in `pcdmap.c` (search for `static int Equal`). Compare `disc` and `pcdcen` values with `ast_equal()` tolerance. Handle inverted state: if inversion flags differ, equality is generally false for PcdMap (the inverse is not parameterically equivalent).

- [ ] **Step 4: Implement simplified()**

If `disc == 0.0` → return `UnitMap::new(2)`. Otherwise normalize inversion (store effective direction in a non-inverted PcdMap). Study C `MapMerge` lines 1405-1421.

- [ ] **Step 5: Implement try_merge()**

Port the full `MapMerge` logic from `pcdmap.c` lines 1238-2095. Key merge rules:

1. **Series with inverse PcdMap:** If neighbour is PcdMap with same disc/centre and opposite inversion → cancel to UnitMap.
2. **Series with PermMap (swap):** Call `PcdPerm` logic — PcdMap can commute past a 2D axis-swap PermMap by swapping its own centre coordinates. Study `CanSwap` and `PcdPerm` functions.
3. **Series with ZoomMap:** Call `PcdZoom` logic — absorb zoom factor into disc (disc' = disc/zoom^2) and adjust centre. Study `PcdZoom` function at line 2290.

For parallel composition: return `None` (PcdMap is always 2D, parallel merge not meaningful).

- [ ] **Step 6: Implement Serializable (to_node / from_node)**

Study the `Dump`/`Load` functions in `pcdmap.c`. Tokens:
- `Disco` — distortion coefficient (default 0.0)
- `PcdCn1` — centre axis 1 (default 0.0)
- `PcdCn2` — centre axis 2 (default 0.0)

Use `node.push_double()` / `node.get_double()` matching exact token names from C.

- [ ] **Step 7: Wire into serialization dispatch**

In `crates/ast-core/src/serial/mod.rs`:
- Add `"PcdMap" => Ok(Box::new(PcdMap::from_node(node)?))` to `mapping_from_node`
- Add downcast arm `if let Some(m) = self.as_any().downcast_ref::<PcdMap>() { return m.to_node(); }` to `MappingToNode`

In `crates/ast-core/src/mappings/mod.rs`:
- Add `mod pcdmap; pub use pcdmap::PcdMap;`

- [ ] **Step 8: Add use statements to serial/mod.rs**

Add `use crate::mappings::PcdMap;` to the imports in `serial/mod.rs`.

- [ ] **Step 9: Write unit tests**

Create `crates/ast-core/tests/test_pcdmap.rs`. Port tests from `reference/starlink-ast/ast_tester/testpcdmap.c` if it exists, or write:
- Construction with various disc values
- Forward transform on known points (compare with hand-computed values)
- Inverse transform roundtrip (forward then inverse ≈ identity)
- disc=0 simplifies to UnitMap
- Serialization roundtrip (to_native_string → from_native_string → equal)
- equal() with matching and non-matching PcdMaps

- [ ] **Step 10: Enable simplify regression fixtures**

In `simplify_tests.txt`, the following entries reference PcdMap and should now pass:
- `pcd_zero_to_unit`
- `pcd_inverse_cancel`
- `pcd_unit_series_merge`
- `pcd_zoom_swap_cancel`
- `pcd_perm_swap_cancel`
- `pcd_swap_zoom_simplifies`
- `neg_pcd_parallel`
- `neg_pcd_nonpcd_neighbour`
- `neg_pcd_nonswappable_between`

Verify: `cargo test -p ast-core --test test_simplify_regression`

- [ ] **Step 11: Run cargo fmt and clippy, commit**

```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo test -p ast-core
git add -A && git commit -m "feat: implement PcdMap (2D pincushion/barrel distortion)"
```

---

## Task 2: GrismMap

**Files:**
- Create: `crates/ast-core/src/mappings/grismmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Create: `crates/ast-core/tests/test_grismmap.rs`

**C reference:** `reference/starlink-ast/src/grismmap.c`

- [ ] **Step 1: Create GrismMap struct**

Study `grismmap.h` for the struct. Fields:
```rust
#[derive(Debug, Clone)]
pub struct GrismMap {
    grism_nr: f64,      // refractive index at ref wavelength
    grism_nrp: f64,     // dn/dlambda
    grism_wave_r: f64,  // reference wavelength
    grism_alpha: f64,   // angle of incidence (radians)
    grism_g: f64,       // grating ruling spacing
    grism_m: i32,       // diffraction order
    grism_eps: f64,     // angle between detector normal and grating (radians)
    grism_theta: f64,   // angle of incidence on detector (radians)
    inverted: bool,
    do_not_simplify: bool,
}
```

Constructor sets all 8 parameters. `nin() = 1`, `nout() = 1`.

- [ ] **Step 2: Implement transform**

Port the `Transform` function from `grismmap.c`. The forward transform is the grism dispersion equation from FITS-WCS Paper III. The inverse uses Newton-Raphson iteration. Study the C carefully — the math is ~50 lines per direction.

- [ ] **Step 3: Implement equal()**

Compare all 8 attributes with `ast_equal()`. When inversion differs → not equal (GrismMap inverse is not parametrically equivalent).

- [ ] **Step 4: Implement simplified() and try_merge()**

Port `MapMerge` from `grismmap.c`:
1. **Inverse cancel:** GrismMap + inverse GrismMap with same attributes → UnitMap
2. **GrismMap(fwd) + ZoomMap in series:** Absorb zoom into grism — study `GrismZoom` function in C. The zoom factor scales the output, which can be absorbed by adjusting grism parameters.
3. **ZoomMap + GrismMap(inv) in series:** Similar but from the other side.

For the zoom merge, study the C `GrismZoom` helper carefully — it modifies `grism_g` and/or creates a new GrismMap with adjusted parameters.

- [ ] **Step 5: Implement Serializable**

Tokens from `Dump`/`Load` in `grismmap.c`:
- `GrsmNR` (default 1.0)
- `GrsmNRP` (default 0.0)
- `GrsmWR` (default 0.0)
- `GrsmA` (default 0.0)
- `GrsmG` (default 0.0)
- `GrsmM` (default 0, stored as int)
- `GrsmEps` (default 0.0)
- `GrsmTh` (default 0.0)

- [ ] **Step 6: Wire serialization dispatch + module export**

Same pattern as PcdMap: add to `mapping_from_node`, `MappingToNode`, and `mod.rs`.

- [ ] **Step 7: Write unit tests**

- Known-value forward/inverse transforms (use C library to generate reference values)
- Roundtrip: forward then inverse ≈ identity for typical wavelength ranges
- Serialization roundtrip
- equal() tests
- Newton-Raphson convergence for edge cases (near-zero grating spacing, etc.)

- [ ] **Step 8: Enable simplify fixtures**

Enable in `simplify_tests.txt`:
- `grism_inverse_cancel`
- `grism_zoom_merge`
- `grism_zoom_inv_merge`
- `neg_grism_same_direction`
- `neg_grism_different_attrs`
- `neg_grism_parallel`
- `neg_grism_inv_then_zoom`
- `neg_grism_fwd_then_nonzoom`
- `neg_grism_zoom_before_fwd`
- `neg_grism_nonzoom_before_inv`

- [ ] **Step 9: cargo fmt, clippy, test, commit**

---

## Task 3: UnitNormMap

**Files:**
- Create: `crates/ast-core/src/mappings/unitnormmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Create: `crates/ast-core/tests/test_unitnormmap.rs`

**C reference:** `reference/starlink-ast/src/unitnormmap.c`

- [ ] **Step 1: Create UnitNormMap struct**

```rust
#[derive(Debug, Clone)]
pub struct UnitNormMap {
    centre: Vec<f64>,  // length = nin
    inverted: bool,
    do_not_simplify: bool,
}
```

`nin = centre.len()`, `nout = nin + 1` (forward adds the norm as last output).
When inverted: `nin = centre.len() + 1`, `nout = centre.len()`.

- [ ] **Step 2: Implement transform**

Forward (nin → nin+1): subtract centre, compute norm = sqrt(sum of squares), output unit vector components (divided by norm) then norm itself. If norm is 0, output BAD for unit components and 0 for norm.

Inverse (nin+1 → nin): last input is norm, first nin inputs are unit vector components. Multiply each component by norm, add centre.

Study `Transform` in `unitnormmap.c` carefully for BAD_VALUE handling.

- [ ] **Step 3: Implement equal()**

Compare centres with `ast_equal()`. Handle inversion flags.

- [ ] **Step 4: Implement try_merge()**

Port `MapMerge` from `unitnormmap.c`. Key rules:
1. **Inverse cancel:** Same centres → UnitMap
2. **UnitNormMap + ShiftMap series:** Absorb shift into centre
3. **UnitNormMap + WinMap series:** Absorb linear transform into centre
4. **Two UnitNormMaps with different centres (fwd+inv):** Difference → ShiftMap

Study the C `MapMerge` carefully — it has complex logic for merging with ShiftMap and WinMap that modifies the centre appropriately depending on forward/inverse direction.

- [ ] **Step 5: Implement Serializable**

Tokens: `Centre(i)` for i=1..nin. Study `Dump`/`Load` in `unitnormmap.c`.

- [ ] **Step 6: Wire serialization dispatch + module export**

- [ ] **Step 7: Write unit tests**

- Forward: [3, 4] with centre [0,0] → unit vector [0.6, 0.8] + norm 5.0
- Inverse roundtrip
- Zero-vector → BAD components, 0 norm
- Serialization roundtrip
- equal() tests

- [ ] **Step 8: Enable simplify fixtures**

Enable:
- `unitnormmap_inverse_cancel`
- `unitnormmap_diff_centre_to_shift`
- `unitnormmap_inv_fwd_cancel`
- `unitnormmap_shift_fwd_merge`
- `unitnormmap_winmap_fwd_merge`
- `unitnormmap_inv_shift_merge`
- `unitnormmap_inv_winmap_merge`
- `neg_unitnormmap_nonunit_scale`
- `neg_unitnormmap_nonmergeable`

- [ ] **Step 9: cargo fmt, clippy, test, commit**

---

## Task 4: TranMap

**Files:**
- Create: `crates/ast-core/src/mappings/tranmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Create: `crates/ast-core/tests/test_tranmap.rs`

**C reference:** `reference/starlink-ast/src/tranmap.c`

- [ ] **Step 1: Create TranMap struct**

```rust
#[derive(Debug, Clone)]
pub struct TranMap {
    map1: DynMapping,  // supplies forward transform
    map2: DynMapping,  // supplies inverse transform
    inverted: bool,
    do_not_simplify: bool,
}
```

Constructor validates `map1.nin() == map2.nin()` and `map1.nout() == map2.nout()`.
`nin = map1.nin()`, `nout = map1.nout()`.

- [ ] **Step 2: Implement transform**

Forward: apply map1 forward. Inverse: apply map2 inverse.
When TranMap itself is inverted, swap: forward uses map2's inverse, inverse uses map1's forward.

`has_forward`: check that the effective forward mapping has a forward transform.
`has_inverse`: check that the effective inverse mapping has an inverse transform.

- [ ] **Step 3: Implement equal()**

Both stored mappings must be recursively `equal()`. Handle inversion (inverted TranMap(A,B) should equal non-inverted TranMap(B,A)).

- [ ] **Step 4: Implement simplified() and try_merge()**

From `tranmap.c` `MapMerge`:
1. **Invert normalize:** If inverted, swap map1/map2 and clear inversion flag
2. **Equal components:** If map1.equal(map2) → simplify to just map1
3. **Component simplification:** Simplify each stored mapping independently
4. **Adjacent TranMap merge:** TranMap(A,B) in series with TranMap(C,D) → TranMap(CmpMap(A,C), CmpMap(D,B)) only when the composition dimensions match
5. **TranMap + identical inverse TranMap → UnitMap**

- [ ] **Step 5: Implement Serializable**

TranMap serializes two child nodes (like CmpMap). Study `Dump`/`Load` in `tranmap.c`. The two child mappings are stored as nested objects. Pattern follows `CmpMap::from_node` — read two child nodes from the node's children list.

- [ ] **Step 6: Wire serialization dispatch + module export**

- [ ] **Step 7: Write unit tests**

- Construct TranMap from ZoomMap(2) forward + ZoomMap(3) inverse
- Forward transform uses factor 2, inverse uses factor 1/3
- equal() for matching/non-matching TranMaps
- Serialization roundtrip
- Simplification: equal components → single mapping

- [ ] **Step 8: Enable simplify fixtures**

Enable:
- `tranmap_equal_components`
- `tranmap_invert_normalize`
- `tranmap_component_simplify`
- `tranmap_adjacent_merge`
- `neg_tranmap_unequal_components`
- `neg_tranmap_parallel`
- `neg_tranmap_nontranmap_neighbour`

- [ ] **Step 9: cargo fmt, clippy, test, commit**

---

## Task 5: RateMap

**Files:**
- Create: `crates/ast-core/src/mappings/ratemap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Create: `crates/ast-core/tests/test_ratemap.rs`

**C reference:** `reference/starlink-ast/src/ratemap.c`

- [ ] **Step 1: Create RateMap struct**

```rust
#[derive(Debug, Clone)]
pub struct RateMap {
    map: DynMapping,
    ax1: usize,  // 1-based output axis
    ax2: usize,  // 1-based input axis
    inverted: bool,
    do_not_simplify: bool,
}
```

`nin = map.nin()`, `nout = 1`.
`has_forward = true`, `has_inverse = false` (unless inverted, then swap).

- [ ] **Step 2: Implement transform**

Forward: for each input point, call `map.rate(point, ax1, ax2)`. The output is a single value per point.

When inverted, forward is undefined and inverse performs the rate computation.

- [ ] **Step 3: Implement equal()**

Same encapsulated mapping (recursively) AND same ax1, ax2.

- [ ] **Step 4: Implement simplified() and try_merge()**

From `ratemap.c` `MapMerge`:
1. **Inverse cancel:** RateMap + inverse RateMap with same map/axes → UnitMap(nin)
2. **Interior simplification:** Simplify the encapsulated mapping, build new RateMap around simplified version

- [ ] **Step 5: Implement Serializable**

Tokens from `ratemap.c` Dump/Load:
- `RatAx1` — 1-based output axis (int)
- `RatAx2` — 1-based input axis (int)
- Plus one child node: the encapsulated mapping

- [ ] **Step 6: Wire serialization dispatch + module export**

- [ ] **Step 7: Write unit tests**

- RateMap wrapping ZoomMap(3) with ax1=1, ax2=1 → rate should be 3.0 everywhere
- RateMap wrapping ShiftMap → rate should be 1.0 (shift is linear)
- Serialization roundtrip
- equal() tests

- [ ] **Step 8: Enable simplify fixtures**

Enable:
- `ratemap_inverse_cancel`
- `ratemap_simplify_interior`
- `neg_ratemap_nonratemap_neighbour`
- `neg_ratemap_different_inner`
- `neg_ratemap_parallel`

- [ ] **Step 9: cargo fmt, clippy, test, commit**

---

## Task 6: SplineMap

**Files:**
- Create: `crates/ast-core/src/mappings/splinemap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Create: `crates/ast-core/tests/test_splinemap.rs`

**C reference:** `reference/starlink-ast/src/splinemap.c`

- [ ] **Step 1: Create SplineMap struct**

Study `splinemap.h`. Key fields:
```rust
#[derive(Debug, Clone)]
pub struct SplineMap {
    order: usize,
    nknot: [usize; 2],
    knots: [Vec<f64>; 2],
    coeff: [Vec<f64>; 2],  // one coefficient array per output axis
    out_unit: f64,          // value outside domain (BAD_VALUE if not set)
    inverted: bool,
    do_not_simplify: bool,
}
```

`nin = 2`, `nout = 2`. Always 2D→2D.

- [ ] **Step 2: Implement B-spline evaluation (forward transform)**

Port the B-spline evaluation from `splinemap.c`. The forward transform evaluates two bivariate B-splines (one per output coordinate) at the input (x,y) position. Study the `EvalPoly` or equivalent function. If outside the knot domain, output `out_unit` (or BAD if not set).

- [ ] **Step 3: Implement inverse transform (Newton-Raphson)**

Port the iterative inverse from `splinemap.c`. It uses Newton-Raphson on the 2D system, computing the Jacobian via spline derivatives. Match C iteration limits and convergence criteria exactly.

- [ ] **Step 4: Implement equal()**

Compare order, all knots, all coefficients within tolerance. Handle inverted state.

- [ ] **Step 5: Implement try_merge()**

SplineMap only supports inverse-cancel: two equal SplineMaps with opposite inversion → UnitMap.

- [ ] **Step 6: Implement Serializable**

Study `Dump`/`Load` in `splinemap.c`. Tokens:
- `SpOrd` — order
- `NKnot1`, `NKnot2` — knot counts
- `Knot1(i)`, `Knot2(i)` — knot values
- `CoA(i)`, `CoB(i)` — coefficients for output 1 and 2
- `OutUnt` — out_unit value

- [ ] **Step 7: Wire serialization dispatch + module export**

- [ ] **Step 8: Write unit tests**

- Construct from known knots/coefficients, verify forward transform
- Inverse roundtrip (forward then inverse ≈ identity within iteration tolerance)
- Outside-domain → out_unit or BAD
- Serialization roundtrip
- equal() tests

- [ ] **Step 9: Enable simplify fixtures**

Enable:
- `spline_inverse_cancel`
- `neg_spline_same_direction`
- `neg_spline_parallel`
- `neg_spline_nonspline_neighbour`
- `neg_spline_different_coeffs`

- [ ] **Step 10: cargo fmt, clippy, test, commit**

---

## Task 7: DssMap

**Files:**
- Create: `crates/ast-core/src/mappings/dssmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Create: `crates/ast-core/tests/test_dssmap.rs`

**C reference:** `reference/starlink-ast/src/dssmap.c`

- [ ] **Step 1: Create DssMap struct**

Study `dssmap.h` and the `WorldCoor` struct used internally in `dssmap.c`. Port the plate solution parameters:

```rust
#[derive(Debug, Clone)]
pub struct DssMap {
    plate_ra: f64,
    plate_dec: f64,
    plate_scale: f64,
    x_pixel_size: f64,
    y_pixel_size: f64,
    ppo: [f64; 6],
    amd_x: [f64; 20],
    amd_y: [f64; 20],
    // Derived quantities cached at construction:
    x_coeff: f64,
    y_coeff: f64,
    inverted: bool,
    do_not_simplify: bool,
}
```

No public constructor — DssMap is created from FITS keywords via `DssMap::from_fits_keywords()`. Study the `DssFits` function in `dssmap.c` for how keywords map to struct fields.

- [ ] **Step 2: Implement platepos (forward: pixel → RA,Dec)**

Port the `platepos` function from `dssmap.c`. This is a polynomial plate solution converting (x,y) pixel offsets from plate centre to (RA, Dec) in radians. The algorithm involves plate constants PPO1-6 to compute standard coordinates, then AMDx/AMDy polynomial correction.

- [ ] **Step 3: Implement platepix (inverse: RA,Dec → pixel)**

Port the `platepix` function. This is iterative — starts with approximate inversion then refines. Study convergence criteria and iteration limits in C.

- [ ] **Step 4: Implement equal()**

Compare all plate constants within tolerance.

- [ ] **Step 5: Implement simplified() and try_merge()**

DssMap simplification: basic inverse-cancel only. The C also has DssMap + WinMap merge — study `MapMerge` in `dssmap.c` to see if this applies.

- [ ] **Step 6: Implement Serializable**

Study `Dump`/`Load` in `dssmap.c`. DssMap stores its parameters as a set of FITS-keyword-like tokens in the native format. The token names match the DSS FITS keywords (PLTRAH, PLTRAM, PLTRAS, etc.).

- [ ] **Step 7: Wire serialization dispatch + module export**

- [ ] **Step 8: Write unit tests**

- Load from the existing `dss.ast` fixture in `tests/fixtures/wcsconv/inputs/`
- Forward/inverse roundtrip on known pixel positions
- Serialization roundtrip
- equal() tests
- Compare with C output on the DSS test data

- [ ] **Step 9: cargo fmt, clippy, test, commit**

---

## Task 8: NormMap

**Files:**
- Create: `crates/ast-core/src/mappings/normmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Create: `crates/ast-core/tests/test_normmap.rs`

**C reference:** `reference/starlink-ast/src/normmap.c`

- [ ] **Step 1: Create NormMap struct**

```rust
#[derive(Debug, Clone)]
pub struct NormMap {
    frame: DynFrame,
    inverted: bool,
    do_not_simplify: bool,
}
```

Constructor: `NormMap::new(frame: DynFrame) -> Self`.
`nin = nout = frame.naxes()`.

- [ ] **Step 2: Implement transform**

Both forward and inverse call `frame.norm(point)`. The normalization is idempotent so forward and inverse are the same operation.

For each input point: if any coordinate is BAD, output BAD for all. Otherwise call `frame.norm()` to normalize (e.g., wrap longitude into [0, 2pi] for SkyFrame).

- [ ] **Step 3: Implement equal()**

Stored frames must be equal (using `Frame::equal` — study existing frame equality). Handle inverted state (irrelevant for NormMap since forward = inverse).

- [ ] **Step 4: Implement try_merge()**

From `normmap.c` `MapMerge`:
1. **BasicFrame → UnitMap:** If stored frame is a BasicFrame (whose `norm()` is identity), simplify to UnitMap.
2. **Inverse cancel:** NormMap + inverse NormMap with equal frames → UnitMap. (Note: since forward=inverse, this is really just "adjacent equal NormMaps cancel".)
3. **Duplicate elimination:** Two adjacent NormMaps with equal frames → single NormMap.
4. **NormMap in parallel:** Not mergeable (return None).

- [ ] **Step 5: Implement Serializable**

NormMap stores one child node: the frame. Study `Dump`/`Load` in `normmap.c`. The frame is serialized as a nested object (like CmpMap stores child mappings).

This requires `frame_from_node` / frame `to_node` dispatch — verify this exists in `serial/mod.rs`.

- [ ] **Step 6: Wire serialization dispatch + module export**

- [ ] **Step 7: Write unit tests**

- NormMap with SkyFrame: normalize longitude outside [0, 2pi]
- NormMap with BasicFrame → simplifies to UnitMap
- Two adjacent NormMaps → single NormMap
- Serialization roundtrip
- equal() tests

- [ ] **Step 8: Enable simplify fixtures**

Enable:
- `normmap_basic_frame_to_unit`
- `normmap_inverse_cancel`
- `normmap_inverse_cancel_upper`
- `normmap_duplicate_elim`
- `neg_normmap_parallel`
- `neg_normmap_different_frames`
- `neg_normmap_nonnorm_neighbour`

- [ ] **Step 9: cargo fmt, clippy, test, commit**

---

## Task 9: MathMap

**Files:**
- Create: `crates/ast-core/src/mappings/mathmap/mod.rs`
- Create: `crates/ast-core/src/mappings/mathmap/token.rs`
- Create: `crates/ast-core/src/mappings/mathmap/parser.rs`
- Create: `crates/ast-core/src/mappings/mathmap/compiler.rs`
- Create: `crates/ast-core/src/mappings/mathmap/evaluator.rs`
- Create: `crates/ast-core/src/mappings/mathmap/rng.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Modify: `crates/ast-core/Cargo.toml` (add `logos`)
- Modify: `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Create: `crates/ast-core/tests/test_mathmap.rs`

**C reference:** `reference/starlink-ast/src/mathmap.c` (7,424 lines)

This is decomposed into sub-steps because of its size.

- [ ] **Step 1: Add logos dependency**

In `crates/ast-core/Cargo.toml`, add:
```toml
logos = "0.16"
```

- [ ] **Step 2: Create module structure**

Create `crates/ast-core/src/mappings/mathmap/` directory with `mod.rs` and sub-modules. In parent `mod.rs` add:
```rust
mod mathmap;
pub use mathmap::MathMap;
```

The mathmap module splits into:
- `token.rs` — logos-based tokenizer
- `parser.rs` — Pratt parser producing AST or direct opcodes
- `compiler.rs` — compiles parsed expressions to opcodes + constants
- `evaluator.rs` — stack VM that executes compiled expressions
- `rng.rs` — random number generator (exact port from C)
- `mod.rs` — public MathMap struct and Mapping trait impl

- [ ] **Step 3: Implement tokenizer (logos)**

Define token enum in `token.rs` using logos derive:
- Numbers: integer and float literals
- Identifiers: `[a-zA-Z_][a-zA-Z0-9_]*`
- System constants: `<bad>`, `<pi>`, `<e>`, `<dig>`, `<epsilon>`, etc.
- Operators: all arithmetic, relational, bitwise, assignment
- Logical operators: `.and.`, `.or.`, `.not.`, `.eqv.`, `.xor.`
- Punctuation: `(`, `)`, `,`, `?`, `:`

Write unit tests for the tokenizer in isolation.

- [ ] **Step 4: Implement Pratt parser**

In `parser.rs`, implement a precedence-climbing parser. Study the precedence table in `mathmap.c` symbol table (lines ~400-560). The parser produces a postfix opcode stream.

Key: the parser must handle:
- Assignment: `y = expr` or `y1 = expr`
- Variable-arity: `max(a, b, c)` (count args at parse time, emit OP_MAX with arg count)
- Ternary: `a ? b : c` (special precedence handling)
- Unary prefix: `-x`, `.not. x`

Write parser unit tests: parse known expressions, verify opcode output.

- [ ] **Step 5: Define Opcode enum and CompiledExpr**

In `compiler.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Opcode {
    LdCon(usize),   // load constant at index
    LdVar(usize),   // load variable x(i) (0-based internally)
    LdBad, LdPi, LdE, LdDig, LdEpsilon, // ... system constants
    // Unary functions
    Abs, Acos, Acosd, Acosh, /* ... all from spec ... */ Tanh,
    // Binary functions
    Atan2, Atan2d, Dim, Mod, Pow, Sign,
    // Random (these need RNG context)
    Gauss, Poiss, Rand,
    // Ternary
    Qif,
    // Variable-arity
    Max(usize), Min(usize),  // arg count
    // Arithmetic
    Neg, Add, Sub, Mul, Div,
    // Bitwise
    ShiftL, ShiftR, BitAnd, BitOr, BitXor,
    // Relational
    Eq, Ne, Lt, Gt, Le, Ge,
    // Logical
    And, Or, Not, Eqv, Xor,
    // Special
    IsBad, Nint, Ceil, Floor, Sinc, Sqr,
}

pub(crate) struct CompiledExpr {
    pub opcodes: Vec<Opcode>,
    pub constants: Vec<f64>,
    pub stack_size: usize,
}
```

- [ ] **Step 6: Implement stack-based evaluator**

In `evaluator.rs`, implement `evaluate(expr: &CompiledExpr, inputs: &[f64], rng: &mut MathMapRng) -> f64`.

Key semantics:
- BAD_VALUE propagation: most ops check inputs for BAD and produce BAD
- `IsBad`: does NOT propagate — returns 0.0 or 1.0
- `Qif` (ternary): only evaluate selected branch
- `And`/`Or`: three-valued logic (BAD .and. false = false)
- Bitwise ops: reinterpret f64 bits as i64, operate, reinterpret back
- Math errors (div by zero, sqrt(-1), etc.) → BAD

Write exhaustive evaluator unit tests covering each opcode.

- [ ] **Step 7: Implement random number generator**

In `rng.rs`, port the exact RNG from `mathmap.c`:
- `Rand()` function (combined generator with shuffle table)
- `Gauss()` (Box-Muller or rejection method — match C exactly)
- `Poisson()` (rejection method)
- `DefaultSeed()` function

Study `mathmap.c` lines ~600-800 for the RNG implementation. The algorithms use specific constants that must be reproduced exactly.

Write RNG unit tests: seed with known value, verify first N outputs match C.

- [ ] **Step 8: Implement MathMap struct and Mapping trait**

In `mod.rs`:
```rust
pub struct MathMap {
    fwd_exprs: Vec<String>,
    inv_exprs: Vec<String>,
    fwd_compiled: Vec<CompiledExpr>,
    inv_compiled: Vec<CompiledExpr>,
    rng: MathMapRng,
    simp_fi: Option<bool>,
    simp_if: Option<bool>,
    inverted: bool,
    do_not_simplify: bool,
}
```

Constructor: `MathMap::new(nin: usize, nout: usize, fwd: &[&str], inv: &[&str]) -> AstResult<Self>`.
Compiles all expressions at construction time.

Transform: evaluate compiled expressions for each point.

- [ ] **Step 9: Implement equal() and try_merge()**

`equal()`: Compare expression strings (exact string match after normalization — study C `Equal`). Compare simp_fi/simp_if attributes.

`try_merge()`:
- If `simp_fi` is true: forward MathMap + inverse MathMap (same exprs) → UnitMap
- If `simp_if` is true: inverse MathMap + forward MathMap (same exprs) → UnitMap
- No other merge rules.

- [ ] **Step 10: Implement Serializable**

Tokens:
- `Nfwd` — int
- `Ninv` — int
- `Fwd1`, `Fwd2`, ... — expression strings (quoted)
- `Inv1`, `Inv2`, ... — expression strings (quoted)
- `Seed` — int (only if set)
- `SimpFI` — int (0/1)
- `SimpIF` — int (0/1)

Study `Dump`/`Load` in `mathmap.c` for exact token format.

- [ ] **Step 11: Wire serialization dispatch + module export**

- [ ] **Step 12: Write comprehensive unit tests**

Test cases covering:
- Simple expressions: `y = x1 * 2`, `y = sin(x1)`
- Multi-variable: `y1 = x1 + x2`, `y2 = x1 - x2`
- System constants: `y = <pi>`, `y = <bad>`
- BAD propagation: BAD input → BAD output
- `isbad()` does NOT propagate
- Three-valued logic: `<bad> .and. 0` = 0, `<bad> .and. 1` = BAD
- Ternary: `x1 > 0 ? x1 : -x1` (abs equivalent)
- Bitwise: `x1 >> 1`
- Variable-arity: `max(x1, x2, x3)`
- Random functions (with fixed seed, compare against C output)
- Serialization roundtrip
- Simplification with SimpFI/SimpIF

- [ ] **Step 13: Enable simplify fixtures**

Enable:
- `math_inverse_cancel`
- `neg_math_no_simpfi`
- `neg_math_nonmath_neighbour`

- [ ] **Step 14: cargo fmt, clippy, test, commit**

---

## Post-Implementation Verification

After all 9 tasks are complete:

- [ ] **Run full simplify regression suite:** `cargo test -p ast-core --test test_simplify_regression`
- [ ] **Run wcsconv tests:** `cargo test -p ast-core --test test_wcsconv` — verify that tests requiring GrismMap/MathMap now pass
- [ ] **Run full test suite:** `cargo test --workspace`
- [ ] **Run clippy:** `cargo clippy --workspace -- -D warnings`

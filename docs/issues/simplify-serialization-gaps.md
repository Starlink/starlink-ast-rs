# Remaining Simplification Serialization Gaps

Status: 22 serialization mismatches remain (down from 116).
Date: 2026-05-08

All 22 produce numerically correct transforms (0 transform failures)
but the simplified structure differs from C — our simplify engine
doesn't cover all the merge/optimization paths the C library covers.

## Categories

### 1. Parallel WinMap merges not happening (7 cases)

The C library merges `WinMap || UnitMap`, `WinMap || ZoomMap`,
`WinMap || ShiftMap`, `WinMap || diag-MatrixMap` into a wider WinMap.
Our code keeps them as CmpMap(parallel).

- `win_unit_parallel_merge` — WinMap(1) || UnitMap(1) → WinMap(2)
- `win_zoom_parallel_merge_rev` — ZoomMap(1) || WinMap(1) → WinMap(2)
- `win_shift_parallel_merge_rev` — ShiftMap(1) || WinMap(1) → WinMap(2)
- `win_diagmatrix_parallel_merge_rev` — diag-MatrixMap(1) || WinMap(1) → WinMap(2)
- `win_unit_parallel_merge_rev` — UnitMap(1) || WinMap(1) → WinMap(2)
- `win_cmpmap_parallel_merge` — WinMap(1) || CmpMap(parallel, contains WinMap) → wider WinMap
- `win_upper_cmpmap_parallel_merge` — CmpMap(par) || WinMap → wider WinMap

**Fix**: Add `WinMap::try_merge` for the `!series` (parallel) case.
When merging with UnitMap/ZoomMap/ShiftMap/diag-MatrixMap in parallel,
produce a wider WinMap by concatenating shift/scale vectors. This is
the `winmap-31..37` merge pathway in the C library.

### 2. PermMap series composition not fully folding (4 cases)

When two PermMaps compose in series, the C library folds constants
through, eliminates identity sub-arrays, and propagates BAD entries.
Our PermMap `try_merge` handles basic composition but misses some
optimizations.

- `perm_bad_propagation` — BAD entries should propagate through composition
- `perm_identity_array_null` — identity sub-arrays should collapse to UnitMap or simpler PermMap
- `perm_outperm_constant_fold` — constants from first map should fold into second's output
- `perm_parallel_merge` — parallel PermMaps should merge into a wider PermMap

**Fix**: Enhance `PermMap::try_merge` to handle BAD propagation,
constant folding, and identity detection. Add parallel-merge path.

### 3. SphMap optimizations (3 cases)

- `sph_matrix_sandwich` — SphMap ; MatrixMap ; SphMap^-1 should simplify (rotation in Cartesian)
- `sph_fwd_inv_unitradius_cancel` — SphMap(fwd, unit_radius) ; SphMap(inv, unit_radius) → UnitMap
- `sph_zoom_sandwich` — SphMap ; ZoomMap ; SphMap^-1 should simplify

**Fix**: Add SphMap merge rules for "sandwich" patterns (spherical→cartesian→
transform→cartesian→spherical) and inverse cancellation with matching flags.

### 4. CmpMap structure differences (3 cases)

- `cmpmap_parallel_series_components` — parallel CmpMap whose components are series chains; C flattens further
- `cmpmap_nested_parallel_flatten` — nested parallel CmpMaps; C flattens to a single level
- `cmpmap_perm_parallel_swap` — PermMap used to reorder parallel components; C optimizes

**Fix**: Enhance CmpMap parallel flattening and the "swap past
parallel boundary" optimization (moving PermMaps past parallel
CmpMap boundaries to enable further merges).

### 5. Cross-type swap optimizations (3 cases)

- `win_swap_past_wcsmap` — WinMap should swap past WcsMap (coordinate re-tagging)
- `win_swap_simplifies` — After swapping, the new neighbours should merge
- `win_swap_outer_merge` — Outer neighbours should merge after internal swap

**Fix**: Implement the "swap" optimization where linear mappings
(WinMap/MatrixMap) can commute past certain mappings (WcsMap,
PermMap) when doing so enables further merges.

### 6. SlaMap merge gap (1 case)

- `sla_merge_no_cancel` — CmpMap(SlaMap ; SlaMap) where steps don't cancel
  but should merge into a single SlaMap with combined steps

**Fix**: Our `SlaMap::try_merge` already handles this, but the
issue is a comment difference — `Sla1a = 1950` should have the
comment "Besselian epoch of mean equinox (FK4)". (May already be
fixed; re-check.)

### 7. Zoom + next-MatrixMap absorption (1 case)

- `zoom_absorb_next_matrix` — ZoomMap ; MatrixMap should absorb the zoom into the matrix

**Fix**: Add ZoomMap→MatrixMap absorption in series (scale all
matrix elements). The reverse (MatrixMap→ZoomMap) already works.

### 8. brad (1 case)

Complex multi-frame FrameSet simplification with LutMap components.
Likely involves multiple missing optimizations interacting.

## Priority

1. **Parallel WinMap merges** (7 cases) — single fix, biggest impact
2. **PermMap folding** (4 cases) — important for wcsconv pipeline
3. **Cross-type swaps** (3 cases) — enables cascade simplifications
4. **SphMap sandwiches** (3 cases) — important for celestial WCS
5. **CmpMap flattening** (3 cases) — structural completeness
6. **ZoomMap absorption** (1 case) — straightforward

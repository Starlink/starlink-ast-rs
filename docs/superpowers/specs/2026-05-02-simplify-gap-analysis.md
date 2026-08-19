# Simplification Gap Analysis: Rust vs C

## Date: 2026-05-02

## Current State

803 tests passing. LSST benchmark (release, simplified):
- Forward 100K: 646ns/pt (C: 363ns/pt, 1.8x slower)
- Inverse 100K: 4,400ns/pt (C: 1,148ns/pt, 3.8x slower)

## Implemented Merge Rules

| Rule | Status |
|------|--------|
| UnitMap removal (series) | Done |
| UnitMap + UnitMap (parallel) | Done |
| ZoomMap + ZoomMap → accumulated | Done |
| ShiftMap + ShiftMap → accumulated | Done |
| WinMap + WinMap → composed | Done |
| MatrixMap + MatrixMap → product | Done |
| PermMap + PermMap → identity cancel | Done |
| SphMap + SphMap^-1 → UnitMap | Done |
| WcsMap + WcsMap^-1 → UnitMap | Done |
| SlaMap + SlaMap^-1 → UnitMap | Done |
| PolyMap + PolyMap^-1 → UnitMap | Done |

## Missing Rules by Tier

### Tier 1: Cross-Type Linear Merges (High Impact)

These are the most impactful for the LSST benchmark since the
mapping chain contains adjacent linear mappings of different types.

| Rule | C Location | Impact |
|------|-----------|--------|
| ZoomMap absorbed into MatrixMap | zoommap.c:900+ | High — LSST chain has ZoomMap next to MatrixMap |
| WinMap absorbs ZoomMap | winmap.c:884+ | Medium — cross-type linear composition |
| WinMap absorbs ShiftMap | winmap.c:884+ | Medium — ShiftMap is WinMap with unit scales |
| MatrixMap self-reduce → UnitMap | matrixmap.c:1852 | High — identity matrix becomes UnitMap |
| MatrixMap self-reduce → ZoomMap | matrixmap.c:1860 | Medium — equal-diagonal matrix becomes ZoomMap |
| MatrixMap self-reduce → diagonal | matrixmap.c:1887 | Low — optimize storage form |
| WinMap self-reduce → MatrixMap | winmap.c:1083 | Low — zero shifts become diagonal matrix |
| LutMap linear detect → WinMap | lutmap.c:1136 | Low — enables further merging |
| ZoomMap absorbed into WinMap | zoommap.c via ShiftMap pattern | Medium |

### Tier 2: Swapping Rules (Enable Non-Adjacent Merges)

Swapping reorders adjacent mappings to bring compatible types
together, enabling merges that wouldn't otherwise be possible.

| Rule | C Location | Impact |
|------|-----------|--------|
| MatrixMap ↔ PermMap swap | matrixmap.c:1998 | Medium |
| WinMap ↔ PermMap swap | winmap.c:383 | Medium |
| WinMap ↔ WcsMap swap | winmap.c:369 | High — moves linear scale outside projection |
| WcsMap ↔ PermMap swap | wcsmap.c:3040 | Medium — axis reorder around projection |
| DssMap ↔ WinMap swap | dssmap.c | Low — DSS projections uncommon |

### Tier 3: Internal Simplification

| Rule | C Location | Impact |
|------|-----------|--------|
| SlaMap + SlaMap → merged conversion chain | slamap.c:2535 | Medium |
| SlaMap internal step cancellation | slamap.c:2600+ | Medium |
| PolyMap coefficient combining | polymap.c:3607 | Low |
| PolyMap linear extraction → CmpMap(Shift+Matrix+residual) | polymap.c:3500+ | Low |
| SphMap + ZoomMap absorption (inverted SphMap) | sphmap.c:815 | Medium |
| SphMap + diagonal MatrixMap absorption | sphmap.c:830 | Medium |

### Tier 4: Types Not Yet in Rust

| Rule | Depends On |
|------|-----------|
| SpecMap + SpecMap merging | SpecMap (Step 7c) |
| TimeMap + TimeMap merging | TimeMap (Step 7c) |
| PcdMap merges (PcdMap+ZoomMap, PcdMap+PermMap) | PcdMap |
| GrismMap + ZoomMap absorption | GrismMap |
| MathMap + MathMap merging | MathMap |
| SplineMap merges | SplineMap |
| SwitchMap/SelectorMap merges | SwitchMap/SelectorMap |

## LSST Mapping Chain Analysis

The LSST FrameSet base→current mapping, after current
simplification, contains approximately:

```
PolyMap → [CmpMap chain] → SphMap → MatrixMap → SphMap^-1 → WcsMap → ZoomMap
```

The SphMap + SphMap^-1 pair cancels. But the remaining chain still
has:
- ZoomMap adjacent to WcsMap/MatrixMap (could merge into MatrixMap)
- WinMap (from WCS linear terms) adjacent to WcsMap (could swap)

### Specific optimizations that would help LSST:

1. **ZoomMap + MatrixMap → MatrixMap**: Multiply matrix rows by zoom
   factor. Eliminates one mapping evaluation per point.

2. **WinMap ↔ WcsMap swap**: Move the linear WCS scaling outside
   the nonlinear projection. This allows the WinMap to merge with
   other linear mappings, reducing the chain length.

3. **MatrixMap self-reduce**: If the composed matrix is identity
   (after merging ZoomMap into it), eliminate it entirely.

## C Library's Full MapMerge Coverage

### Types with MapMerge implementations:

UnitMap, ZoomMap, ShiftMap, MatrixMap, WinMap, PermMap, PolyMap,
LutMap, WcsMap, SlaMap, SphMap, PcdMap, GrismMap, DssMap, SpecMap,
TimeMap, IntraMap, UnitNormMap, MathMap, SplineMap, SwitchMap,
SelectorMap.

### Types using default (no merge):

ChebyMap, NormMap, RateMap, TranMap.

### Notable cross-type merge patterns in C:

- **Linear family** (ZoomMap, ShiftMap, WinMap, MatrixMap): All
  interconvert freely. ShiftMap→WinMap, ZoomMap→WinMap, WinMap→
  MatrixMap. MatrixMap absorbs ZoomMap. WinMap absorbs everything.

- **Projection family** (WcsMap, SphMap): Can swap with linear
  mappings (WinMap, PermMap) to enable merging.

- **Conversion chains** (SlaMap, SpecMap, TimeMap): Merge with
  same type by combining conversion steps.

## Recommendations

### Immediate (Tier 1 cross-type merges):
- WinMap absorbs ZoomMap and ShiftMap
- ZoomMap absorbed into MatrixMap
- MatrixMap self-reduction (identity→UnitMap, diagonal→ZoomMap)

### Next priority (Tier 2 swapping):
- WinMap ↔ WcsMap swap
- MatrixMap ↔ PermMap swap

### Later (as types are ported):
- SlaMap internal merging
- SpecMap/TimeMap merging (Step 7c)
- Remaining specialized types

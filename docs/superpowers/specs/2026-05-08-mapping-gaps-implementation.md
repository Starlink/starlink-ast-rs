# Mapping Implementation Gaps: Implementation Design

Date: 2026-05-08
Source: `docs/mapping-gaps-audit.md`
Branch: `tickets/DM-54793`

## Overview

This document specifies the implementation plan for fixing all mapping
correctness, equality, simplification, and performance gaps identified
in the audit. The work is organized into 10 dependency-ordered phases.

**Scope:** All items from §1–§5 of the audit, plus selected §6 items:
`map_split`, DoNotSimplify/Ident guard, WcsMap NatLat/NatLon, LutMap
Inf handling.

**Ordering principle:** The audit's §7 recommended order is followed —
correctness before equality before simplification, because downstream
phases depend on upstream correctness.

---

## Phase 1: Per-axis BAD-value handling

**Audit refs:** §1.1
**Files:** `matrix.rs`, `winmap.rs`, `zoom.rs`, `shift.rs`, `perm.rs`, `helpers.rs`

### Problem

`point_has_bad(inp)` checks if ANY input is BAD, then blanks ALL
outputs. The C library checks per-axis: outputs that don't depend on a
BAD input are still computed.

### Design

Each mapping type gets per-output-axis BAD logic inline in its
`transform_into` method. No shared "smart" helper — each type has
different dependency semantics:

**Diagonal mappings (WinMap, ZoomMap, ShiftMap):**
```
for axis in 0..nout {
    if is_bad(inp[axis]) {
        out[axis] = BAD_VALUE;
    } else {
        out[axis] = <compute for axis>;
    }
}
```

**MatrixMap (full matrix):**
```
for row in 0..nout {
    let mut bad = false;
    for col in 0..nin {
        if matrix[row][col] != 0.0 && is_bad(inp[col]) {
            bad = true;
            break;
        }
    }
    out[row] = if bad { BAD_VALUE } else { <dot product skipping zeros> };
}
```

**PermMap:**
```
for i in 0..nout {
    let src = outperm[i];
    if src < 0 {
        // Constant assignment — always valid
        out[i] = constants[decode(src)];
    } else {
        out[i] = if is_bad(inp[src - 1]) { BAD_VALUE } else { inp[src - 1] };
    }
}
```

### Testing

Oracle transform tests with mixed good/BAD multi-axis inputs will
converge. Add targeted unit tests for each pattern (diagonal with one
BAD axis, matrix with zero-coefficient masking, PermMap constant
assignment with BAD inputs).

---

## Phase 2: WcsMap parameters + LutMap flat inverse

**Audit refs:** §1.2, §1.3, §2.5

### 2a: WcsMap parameter comparison

**File:** `wcsmap.rs`

Add PV parameter iteration to:
- `equal()`: After checking projection type and axis layout, iterate
  over all projection parameter arrays and compare element-by-element.
- `try_merge()` cancellation check: Same parameter comparison before
  allowing inverse cancellation.

Follow the C pattern (`wcsmap.c:1515-1547`): iterate over `np[i]`
parameter counts, then compare `p[i][j]` values with `ast_equal()`.

### 2b: LutMap flat-section inverse

**File:** `lutmap.rs`

At line 264 (when `dv == 0.0` in inverse path), return `BAD_VALUE`
instead of the midpoint.

### Testing

- WcsMap: Two WcsMaps with same projection type but different PV2_1
  must report `equal() == false` and must NOT cancel in try_merge.
- LutMap: Inverse of a flat-section LUT must return BAD_VALUE.

---

## Phase 3: Equality bugs

**Audit refs:** §2.1–2.4
**Files:** `cmpmap.rs`, `matrix.rs`, `perm.rs`, `winmap.rs`

### Cross-cutting convention

All `equal()` implementations must compare *effective behavior* —
resolving the invert flag before comparison. Document this once in the
`Mapping` trait documentation.

### 3a: CmpMap structural → canonical equality

Flatten both CmpMaps into component lists (using the existing
`flatten()` function from `simplify.rs`) and compare component-by-
component. Two CmpMaps with different tree structure but equivalent
flat decomposition are equal.

### 3b: MatrixMap inversion + storage normalization

1. Resolve invert flag: if `self.inverted != other.inverted`, compute
   the effective forward matrix for both (one may need inversion).
2. Normalize storage: if one is diagonal and the other is full,
   expand the diagonal to full (or compare logically — check that the
   full matrix is diagonal and matches).
3. Compare elements with `ast_equal()`.

### 3c: PermMap effective permutation

1. Resolve invert flag: if inverted, swap inperm↔outperm.
2. Handle NULL arrays: treat missing inperm as identity.
3. Compare effective outperm arrays and effective constant values by
   output position.

### 3d: WinMap cross-inversion

Compute effective shifts and scales for both WinMaps (resolving
inversion: inverted WinMap has `shift' = -shift/scale`,
`scale' = 1/scale`). Compare the effective terms.

### Testing

For each type, test that `a.equal(&b)` returns true when the internal
representations differ but effective behavior matches (different tree
shape, different invert flags, diagonal vs full storage, etc.).

---

## Phase 4: SlaMap/TimeMap step concatenation

**Audit refs:** §3.1, §3.2
**Files:** `slamap.rs`, `timemap.rs`
**Template:** `specmap.rs:1143-1176`

### Design

Port the SpecMap `try_merge` pattern verbatim to SlaMap and TimeMap:

1. **Normalize:** Account for invert flags by swapping step codes/args
   (SlaMap has paired forward/inverse codes; TimeMap similarly).
2. **Concatenate:** Combine the two step lists into one.
3. **Simplify:** Run internal pair cancellation — adjacent inverse
   pairs with matching args cancel. For SlaMap, also combine adjacent
   precession steps (sum epoch intervals). For TimeMap, eliminate
   no-effect steps (e.g., MJDTOMJD with zero offset).
4. **Result:** Empty list → UnitMap. Otherwise → merged SlaMap/TimeMap.

Unlike the current code which only handles full inverse cancellation
(identical steps, opposite direction), this handles partial
cancellation and concatenation of non-identical SlaMaps/TimeMaps.

### SlaMap-specific simplification rules (from C slamap.c:2706-3194)

- Inverse pair cancellation (same conversion, opposite direction).
- Precession combination: adjacent `Prec(e1,e2)` + `Prec(e2,e3)` →
  `Prec(e1,e3)`.
- No-effect elimination: conversion where from==to epoch.

### TimeMap-specific simplification rules (from C timemap.c:2099-2428)

- Inverse pair cancellation.
- No-effect elimination: MJDTOMJD with zero offset, TAITOUTC/UTCTOTAI
  pairs, identity epoch shifts.

### Testing

Multi-SkyFrame alignment paths simplify: e.g.,
`[FK4→FK5, Prec(2000,2010)]` + `[Prec(2010,2020)]` →
`[FK4→FK5, Prec(2000,2020)]`.

---

## Phase 5: SphMap cancellation fixes

**Audit refs:** §3.3, §3.4
**File:** `sphmap.rs`

### 5a: Relaxed cancellation conditions

Current Rust requires `unit_radius` on BOTH maps. Fix:

- **Inverse→forward** (sph→cart followed by cart→sph): Only check
  `PolarLong` equality between the two maps. No UnitRadius check
  needed (palDcs2c always produces unit vectors).
- **Forward→inverse** (cart→sph followed by sph→cart): Only require
  `UnitRadius` on the first (forward) map.

### 5b: Three-mapping SphMap pattern

Detect `(inv-SphMap, diagonal-MatrixMap/ZoomMap, fwd-SphMap)` in the
simplification engine. When the diagonal elements all have equal
magnitude:

1. Extract the sign pattern from the diagonal.
2. Construct a WinMap that applies the lon/lat reflections directly
   (scaling by ±1, shifting by π where needed for longitude flip).
3. Replace the trio with this WinMap.
4. On the next simplification pass, the two SphMaps (now adjacent)
   can cancel via the relaxed rule from 5a.

Implementation location: This lives in the CmpMap simplification
engine's merge loop (`simplify.rs`). When considering a merge at
position `i`, check whether positions `i`, `i+1`, `i+2` match the
`(inv-SphMap, diagonal-scaling, fwd-SphMap)` pattern. This is a
3-mapping window merge, analogous to how C's `sphmap.c:798-950`
operates within MapMerge with `where == 3`.

### Testing

Common FITS-WCS patterns where `(inv-SphMap, ZoomMap(-1,1,1),
fwd-SphMap)` appears in the pipeline simplify correctly.

---

## Phase 6: Analytical rate()

**Audit refs:** §5.1
**Files:** `unit.rs`, `zoom.rs`, `shift.rs`, `matrix.rs`, `perm.rs`

Override `rate()` on each linear/trivial mapping:

| Mapping | Implementation |
|---------|---------------|
| UnitMap | `if ax1==ax2 { 1.0 } else { 0.0 }` |
| ZoomMap | `if ax1==ax2 { zoom (or 1/zoom if inv) } else { 0.0 }` |
| ShiftMap | `if ax1==ax2 { 1.0 } else { 0.0 }` |
| MatrixMap | `effective_matrix[ax1-1][ax2-1]` |
| PermMap | `if outperm[ax1-1]==ax2 { 1.0 } else { 0.0 }` |

Each override returns the analytical value directly, bypassing
`numeric_rate()`.

### Testing

Verify each analytical rate matches the numeric approximation to
within tolerance for multiple axis combinations.

---

## Phase 7: WinMap + MatrixMap merges

**Audit refs:** §3.5–3.10
**Files:** `matrix.rs`, `winmap.rs`, `simplify.rs`

### 7a: MatrixMap × PermMap merge (§3.5)

In MatrixMap's `try_merge`, when the neighbor is a PermMap:
1. Convert the PermMap to a permutation matrix (identity with
   permuted rows/columns).
2. Multiply with the MatrixMap (order depends on which side the
   PermMap is on).
3. Return the product as a single MatrixMap.

### 7b: Diagonal MatrixMap × WinMap merge (§3.6)

In MatrixMap's `try_merge`, when the neighbor is a WinMap and the
MatrixMap is diagonal:
1. Absorb the diagonal scales into the WinMap's per-axis scale
   factors.
2. Return a single WinMap with combined shifts and scales.

### 7c: Parallel WinMap merging (§3.7)

In WinMap's `try_merge` for `!series`:
1. Concatenate the axis terms (shifts and scales) from both WinMaps.
2. Return a single larger WinMap covering all axes.
3. Also handle parallel WinMap+ZoomMap, WinMap+ShiftMap,
   WinMap+UnitMap by converting the partner to WinMap form first.

### 7d: WinMap × diagonal MatrixMap series merge (§3.8)

Extend `extract_linear_params()` to recognize diagonal MatrixMap as a
linear mapping (extracting its diagonal as scales, zero shifts). Then
the existing WinMap series merge logic handles the composition.

### 7e: WinMap swap strategies (§3.9)

Implement two swap operations for WinMap:
- **WinMat:** Swap a WinMap past a MatrixMap by absorbing the WinMap's
  per-axis scale into the MatrixMap columns (pre-multiply by diagonal
  scale matrix).
- **WinPerm:** Swap a WinMap past a PermMap by reordering the WinMap's
  shift/scale arrays according to the permutation.

These are used by the simplification engine when a WinMap can't merge
with its immediate neighbor but could merge with a mapping further
away.

### 7f: WinMap + parallel CmpMap distribution (§3.10)

When a WinMap precedes a parallel CmpMap:
1. Determine which WinMap axes feed into which parallel branch (by
   dimension count of each branch).
2. Split the WinMap into sub-WinMaps, one per branch.
3. Insert each sub-WinMap at the start of its respective parallel
   branch.
4. Rebuild the CmpMap. On the next simplification pass, each
   sub-WinMap can merge with the first mapping in its branch.

### Testing

wcsconverter regression fixtures that involve pixel-scaling WinMaps
followed by parallel WCS projections will simplify correctly.

---

## Phase 8: CmpMap improvements

**Audit refs:** §4.1–4.5
**File:** `simplify.rs`, `cmpmap.rs`

### 8a: Re-expansion after merge (§4.1)

After a merge produces a new CmpMap of the same composition kind,
expand it back into the flat list (insert its children at the merge
position) and continue the merge loop from that position.

### 8b: Recursion guard (§4.2)

Maintain a thread-local stack of mapping fingerprints (a lightweight
hash of the flat component list — e.g., type tags + dimension
signature) currently being simplified. Before entering the merge
loop, compute the current mapping's fingerprint and check if it's
already on the stack. If yes, return unchanged (prevents infinite
recursion). Pop on exit.

This complements the existing `MAX_MERGE_PASSES = 50` hard limit
with a targeted structural guard. Keep `MAX_MERGE_PASSES` as a
safety net.

### 8c: PatternCheck / freeze (§4.3)

Track a modification counter for the flat list. After each pass, if
the list length oscillates (same length as 2 passes ago, different
from 1 pass ago), identify which mapping triggered the last merge and
mark it as frozen (skip it in subsequent merge attempts).

Implementation: add a `HashSet<usize>` of frozen indices to the merge
loop state.

### 8d: Relax equal-length constraint (§4.4)

In `try_merge_series_in_parallel`, use dimension-matching to pair up
components from the two series CmpMaps rather than requiring equal
list lengths. Walk both lists, pairing components whose nin/nout
dimensions match.

### 8e: PermMap constant transformation (§4.5)

When swapping a PermMap past a parallel CmpMap, transform the PermMap's
constant values through the appropriate CmpMap component (single-point
transform) rather than using BAD_VALUE as placeholder.

---

## Phase 9: Remaining medium-priority items

**Audit refs:** §3.11–3.13, §4.6–4.15

### 9a: WcsMap PermMap swapping (§3.11)

In WcsMap's `try_merge`, when the neighbor is a PermMap that merely
reorders axes to bring the lon/lat pair to the WcsMap's expected
positions, swap past the PermMap (absorbing the axis reordering into
the WcsMap's internal axis assignments).

### 9b: LutMap merge + linear replacement (§3.12–3.13)

- `try_merge`: Cancel with own inverse (same LUT, opposite invert →
  UnitMap).
- `simplified()`: Detect linear LUTs (all points on a line within
  tolerance) and replace with WinMap.

### 9c: PermMap parallel merge (§4.6)

Combine adjacent parallel PermMaps by offset-adjusting indices into a
single larger PermMap.

### 9d: PermMap simplified() checks inperm (§4.7)

Check both `outperm` AND `inperm` for identity before simplifying to
UnitMap.

### 9e: PolyMap coefficient consolidation (§4.8)

In `simplified()`: merge duplicate terms (same power combination),
sum their coefficients, remove zero-valued terms.

### 9f: PolyMap linear decomposition (§4.9)

When all terms are constant or linear, decompose into ShiftMap +
MatrixMap.

### 9g: ChebyMap try_merge (§4.15)

Delegate to PolyMap's merge logic (ChebyMap is a PolyMap subtype in
the C library; in Rust, share the implementation).

### 9h: SpecMap unconditional merge (§4.10)

Always merge adjacent SpecMaps (combine step lists) even when no
cancellation occurs. This enables future simplification.

### 9i: ZoomMap parallel merge (§4.11)

Adjacent parallel ZoomMaps with different factors → diagonal MatrixMap.

### 9j: WinMap self-simplification (§4.12)

In `simplified()`:
- All-zero-shift, non-unity-scale → diagonal MatrixMap.
- All-unity-scale, non-zero-shift → ShiftMap.
- Identity → UnitMap.

### 9k: MatrixMap swap-and-seek (§4.13)

Search past intermediate mappings (up to N hops) for a merge target.
Swap toward it one position at a time using the swap operations from
Phase 7.

### 9l: MatrixMap full→diagonal self-simplification (§4.14)

In `simplified()`: detect when a full matrix is actually diagonal and
convert to diagonal form.

---

## Phase 10: Performance + selected §6 items

**Audit refs:** §5.2–5.4, §6 selected

### 10a: Adaptive numeric_rate (§5.2)

Replace the 2-point central difference with the C's adaptive 9-point
algorithm: try multiple interval sizes, detect convergence, use the
best estimate. Port from `mapping.c:8859-9152`.

### 10b: SlaMap palMappa hoisting (§5.3)

Move `palMappa` computation outside the per-point loop. Compute once
per transform call (parameters are constant across all points).

### 10c: MatrixMap form-based fast paths (§5.4)

Add a `form` field to MatrixMap (Full/Diagonal/Unit, determined at
construction and after simplification). Use specialized O(N) or O(1)
transform paths for Diagonal and Unit forms.

### 10d: map_split (§6)

Implement `map_split()` on the `Mapping` trait: given a set of output
axes, extract the sub-mapping that produces those outputs (or return
None if inseparable). Implement for: CmpMap (parallel decomposition),
PermMap, WinMap, ZoomMap, ShiftMap, UnitMap, MatrixMap (if diagonal).

### 10e: DoNotSimplify / Ident guard (§6)

Add a `do_not_simplify` flag to mappings. When set, `simplified()`
returns `self` unchanged and `try_merge()` returns None. Used by
users to protect specific mappings from optimization.

### 10f: WcsMap NatLat/NatLon attributes (§6)

Add `nat_lat()` and `nat_lon()` accessors to WcsMap (native latitude
and longitude of the fiducial point). Needed by FitsChan for FITS-WCS
header generation.

### 10g: LutMap Inf handling (§6)

Extend `is_bad()` in `helpers.rs` to also catch `f64::INFINITY` and
`f64::NEG_INFINITY`. Add Inf sanitization at LutMap construction
(replace Inf entries with BAD_VALUE).

---

## Dependencies Between Phases

```
Phase 1 (BAD handling)
  └→ Phase 2 (WcsMap/LutMap correctness) [independent, but sequenced for simplicity]
      └→ Phase 3 (equality) [depends on correct BAD handling for testing]
          └→ Phase 4 (SlaMap/TimeMap) [depends on correct equality]
          └→ Phase 5 (SphMap) [depends on correct equality]
          └→ Phase 6 (rate) [independent, sequenced here for interleaving]
              └→ Phase 7 (WinMap/MatrixMap merges) [depends on equality]
                  └→ Phase 8 (CmpMap) [depends on Phase 7 merge operations]
                      └→ Phase 9 (remaining) [depends on CmpMap infrastructure]
                          └→ Phase 10 (performance + features) [can start after Phase 6]
```

Phases 4, 5, and 6 are mutually independent and can be worked in
parallel after Phase 3.

Phases 9 and 10 overlap — many §10 items don't depend on §9 and can
be started earlier if needed.

---

## Testing Strategy

Each phase is validated by:

1. **Unit tests** for the specific behavior change (added alongside
   the fix).
2. **Oracle transform tests** (`ast-oracle-tests`) — run after each
   phase to measure convergence improvement.
3. **Simplification regression tests** — the existing
   `simplify-regression` test suite, plus new fixtures where needed.
4. **wcsconverter regression** — the end-to-end WCS pipeline test
   (primary target for phases 5–7).

---

## Estimated Complexity

| Phase | Items | Relative effort |
|-------|-------|----------------|
| 1 | 5 mappings | Medium (architectural change per mapping) |
| 2 | 2 fixes | Small |
| 3 | 4 mappings | Medium |
| 4 | 2 mappings | Medium (port from SpecMap template) |
| 5 | 2 items | Medium (3-mapping pattern is non-trivial) |
| 6 | 5 mappings | Small (one-liner per mapping) |
| 7 | 6 sub-tasks | Large (most new merge logic) |
| 8 | 5 sub-tasks | Large (CmpMap engine changes) |
| 9 | 12 sub-tasks | Large (many small items) |
| 10 | 7 sub-tasks | Medium-Large |

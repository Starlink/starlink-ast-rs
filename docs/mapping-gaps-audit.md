# Mapping Implementation Gaps: Rust vs C Oracle

> **HISTORICAL DOCUMENT (superseded).** All §1–§5 items from this 2026-05-08
> audit have been verified fixed and folded into
> `docs/issues/frame-mapping-c-port-gaps.md` (§6g/6h/6i/6j/6l). Retained for
> provenance only; not an active work source.

Audit date: 2026-05-08
Branch: `tickets/DM-54793`

This document catalogs missing or divergent business logic in the Rust
mapping implementations compared to the C reference library
(`reference/starlink-ast/src/`). Organized by category and priority to
guide systematic fixes.

---

## 1. CORRECTNESS BUGS

These produce wrong transform/equality results and must be fixed first.

### 1.1 Per-axis BAD-value handling (all axis-independent mappings)

**Affected:** MatrixMap, WinMap, ZoomMap, ShiftMap, PermMap

**Problem:** The Rust uses `point_has_bad(inp)` which checks if ANY
input coordinate is BAD, then blanks ALL outputs. The C checks BAD
per-axis: if an output doesn't depend on a BAD input, it's still
computed.

**C behavior by type:**
- **WinMap/ZoomMap/ShiftMap** (diagonal): Each axis processed
  independently. BAD on axis 1 never affects axis 2's output.
- **MatrixMap** (full): If `matrix[row,col] == 0.0` AND `input[col]
  == BAD`, the BAD is skipped (zero coefficient masks it). Only
  non-zero coefficients propagate BAD.
- **PermMap**: Only the *selected* input axis matters. BAD on an
  unselected axis is completely irrelevant. Constants are assigned
  unconditionally regardless of input BAD values.

**Rust locations:**
- `matrix.rs:96-109` (`apply_matrix`)
- `winmap.rs:165-192` (`transform_into`)
- `zoom.rs:84-93` (`transform_into`)
- `shift.rs:73-88` (`transform_into`)
- `perm.rs:139-157` (`transform_into`)

**Fix approach:** Replace `point_has_bad(inp)` with per-output-axis
logic. For diagonal mappings, check only `inp[axis]`. For MatrixMap,
check only inputs with non-zero coefficients for that row. For
PermMap, check only the selected input index.

**C references:**
- `matrixmap.c:5173-5203`
- `winmap.c:2494-2547`
- `zoommap.c:1616-1624`
- `shiftmap.c:1039-1065`
- `permmap.c:1963-1999`

---

### 1.2 WcsMap ignores projection parameters in equality and merge

**Problem:** `equal()` and `try_merge()` compare only projection type
and axis layout, completely ignoring PV parameters. Two WcsMaps with
different PV2_1 (e.g., different TAN-SIP distortion) would:
- Report as equal (false positive)
- Cancel into UnitMap (incorrect simplification)

**Rust location:** `wcsmap.rs:379-402`

**C reference:** `wcsmap.c:892-912` (CanMerge), `wcsmap.c:1515-1547`
(Equal) — both iterate over `np[i]` and `p[i][j]`.

**Fix:** Add parameter comparison to both `equal()` and the
cancellation check in `try_merge()`.

---

### 1.3 LutMap flat-section inverse returns midpoint instead of BAD

**Problem:** When the inverse lookup hits a flat section (multiple LUT
entries with the same value), C returns BAD (undetermined). Rust
returns the midpoint of the bracketing indices.

**Rust location:** `lutmap.rs:264` (when `dv == 0.0`)
**C reference:** `lutmap.c:1669-1679`

**Fix:** Return `BAD_VALUE` when `dv == 0.0` in the inverse path.

---

## 2. EQUALITY BUGS

These cause `equal()` to give wrong answers, which then affects
simplification (since some merge decisions rely on equality checks).

### 2.1 CmpMap: structural vs canonical equality

**Problem:** Rust compares tree structure directly. C flattens both
CmpMaps into component lists (via `astMapList`) and compares
component-by-component. This means `(A;(B;C))` ≠ `((A;B);C)` in
Rust but they're equal in C.

**Rust location:** `cmpmap.rs:284-295`
**C reference:** `cmpmap.c:291-425`

**Fix:** Flatten both CmpMaps before comparing components.

---

### 2.2 MatrixMap: inversion and storage form not normalized

**Problem:** C compares the *effective forward matrix* (resolving
invert flag), and calls `ExpandMatrix` to normalize diagonal→full
form before comparing. Rust requires `inverted` flags to match
exactly and compares raw storage.

**Rust location:** `matrix.rs:290-315`
**C reference:** `matrixmap.c:827-955`

**Fix:** Resolve invert flag to select which matrix to compare, and
normalize storage form (or compare logically equivalent elements).

---

### 2.3 PermMap: raw array comparison vs effective permutation

**Problem:** C resolves invert flags (swapping inperm/outperm) and
uses `SAFE_PERMVAL` to handle NULL arrays and compare effective
constant values. Rust compares raw arrays and rejects differing
invert flags.

**Rust location:** `perm.rs:415-430`
**C reference:** `permmap.c:314-393`

**Fix:** Resolve invert flag, compare effective permutations, compare
constant values by output position (not by index into constants
array).

---

### 2.4 WinMap: cross-inversion equality

**Problem:** C computes effective terms via `astWinTerms` (resolving
inversion) and compares. Rust returns false when invert flags differ.

**Rust location:** `winmap.rs:218-221`
**C reference:** `winmap.c:576-599`

**Fix:** Compute effective shifts/scales for both, compare those.

---

### 2.5 WcsMap: projection parameters ignored

(Same as correctness bug 1.2 — listed here for completeness.)

---

## 3. SIMPLIFICATION GAPS — HIGH PRIORITY

These block simplification of real WCS/celestial pipelines.

### 3.1 SlaMap: no adjacent concatenation

**Problem:** `try_merge` only handles full inverse cancellation
(identical step lists, opposite direction). The C concatenates ALL
adjacent SlaMaps' step lists into one, normalizes inversion
(SWAP_CODES/SWAP_ARGS), then runs internal simplification (pair
cancellation, precession combination).

**Impact:** Multi-SkyFrame alignment paths won't simplify. E.g.,
`[FK4→FK5, Prec(2000,2010)]` followed by `[Prec(2010,2020)]` should
merge into `[FK4→FK5, Prec(2000,2020)]`.

**Note:** SpecMap already does this correctly (`specmap.rs:1143-1176`)
and serves as the template.

**Rust location:** `slamap.rs:1074-1105`
**C reference:** `slamap.c:2706-3194`

---

### 3.2 TimeMap: no step-level simplification

**Problem:** Same gap as SlaMap. `try_merge` only handles full inverse
cancellation. `simplified()` is a no-op. No step concatenation, no
internal pair cancellation, no no-effect step elimination
(`MJDTOMJD` with zero offset).

**Rust location:** `timemap.rs:1066-1104`
**C reference:** `timemap.c:2099-2428`

---

### 3.3 SphMap: cancellation too restrictive

**Problem:** Rust requires `unit_radius` on BOTH maps. C behavior:
- **Inverse→forward** (sph→cart, cart→sph): Only checks `PolarLong`
  equality. No UnitRadius needed (palDcs2c always produces unit
  vectors).
- **Forward→inverse** (cart→sph, sph→cart): Only requires
  `UnitRadius` on the first (forward) map.

**Impact:** Common FITS-WCS patterns where inv-SphMap precedes
fwd-SphMap fail to cancel.

**Rust location:** `sphmap.rs:211-213`
**C reference:** `sphmap.c:748-761`

---

### 3.4 SphMap: missing 3-mapping pattern

**Problem:** When `(inv-SphMap, diagonal-MatrixMap/ZoomMap,
fwd-SphMap)` appears and the matrix diagonal has elements of equal
magnitude, C replaces the trio with a WinMap encoding
longitude/latitude reflections. The two adjacent SphMaps then cancel
on the next pass.

**Rust location:** Not implemented.
**C reference:** `sphmap.c:798-950`

---

### 3.5 MatrixMap: missing PermMap merge

**Problem:** C's `MatPerm()` converts a PermMap to a permutation
matrix and multiplies with the MatrixMap, producing a single
MatrixMap. Important for WCS pipelines where axis permutations sit
adjacent to rotation matrices.

**Rust location:** `matrix.rs` `try_merge` — no PermMap case.
**C reference:** `matrixmap.c:2608`

---

### 3.6 MatrixMap: missing diagonal + WinMap merge

**Problem:** C's `MatWin2()` merges a diagonal MatrixMap with an
adjacent WinMap into a single WinMap (absorbing per-axis scales).

**Rust location:** Not implemented.
**C reference:** `matrixmap.c:3230`

---

### 3.7 WinMap: no parallel merging

**Problem:** C combines parallel WinMaps (and parallel
WinMap+ZoomMap, WinMap+ShiftMap, WinMap+UnitMap) by concatenating
axis terms into a single larger WinMap.

**Rust location:** `winmap.rs:231` — returns `None` for `!series`.
**C reference:** `winmap.c:1697-1813`

---

### 3.8 WinMap: no diagonal MatrixMap merge

**Problem:** C can merge WinMap with adjacent diagonal MatrixMap in
series. Rust's `extract_linear_params` only recognizes WinMap,
ZoomMap, ShiftMap — not MatrixMap.

**Rust location:** `winmap.rs:298` (`extract_linear_params`)
**C reference:** `winmap.c:1164-1229`, `winmap.c:2555` (`WinDiagMat`)

---

### 3.9 WinMap: no swap strategies (MatrixMap, PermMap)

**Problem:** C can swap a WinMap past a MatrixMap or PermMap to reach
a merge target further away in the chain. The swap preserves
equivalence by absorbing scale into the MatrixMap or reordering
terms per the PermMap.

**Rust location:** Not implemented.
**C reference:** `winmap.c:2793` (WinMat), `winmap.c:2973` (WinPerm)

---

### 3.10 WinMap: no WinMap + parallel CmpMap restructuring

**Problem:** When a WinMap precedes a parallel CmpMap, C splits the
WinMap by dimension groups, distributes into each parallel branch,
and simplifies. Critical for FITS-WCS where pixel scaling precedes
parallel (lon,lat) WcsMaps.

**Rust location:** Not implemented.
**C reference:** `winmap.c:1268-1416`

---

### 3.11 WcsMap: no PermMap swapping

**Problem:** C's WcsMap MapMerge can swap a WcsMap past adjacent
PermMaps to bring inverse WcsMaps together for cancellation.

**Rust location:** Not implemented.
**C reference:** `wcsmap.c:922-1145`, `wcsmap.c:3163-3313`

---

### 3.12 LutMap: no try_merge at all

**Problem:** LutMap has no merge logic. At minimum it should cancel
with its own inverse (same LUT, opposite invert flag → UnitMap).

**Rust location:** No `try_merge` override.
**C reference:** `lutmap.c:1174-1232`

---

### 3.13 LutMap: no linear→WinMap replacement

**Problem:** C's `GetLinear()` detects when a LUT is effectively
linear (all entries on a line within tolerance) and replaces with a
WinMap.

**Rust location:** No `simplified()` override.
**C reference:** `lutmap.c:512-629`, `lutmap.c:1138-1169`

---

## 4. SIMPLIFICATION GAPS — MEDIUM PRIORITY

### 4.1 CmpMap: no re-expansion of nested CmpMaps after merge

**Problem:** After a merge produces a new nested CmpMap (of matching
composition), it should be expanded back into the flat list for
further merging. Rust only flattens once at the start.

**C reference:** `cmpmap.c:1471-1525` (Strategy A)

---

### 4.2 CmpMap: no recursion guard

**Problem:** C maintains a thread-local stack of mappings currently
being simplified. Before simplifying, checks if the current mapping
equals any ancestor (prevents infinite recursion). Rust relies on
`MAX_MERGE_PASSES = 50`.

**C reference:** `cmpmap.c:3481-3493`

---

### 4.3 CmpMap: no PatternCheck / freeze mechanism

**Problem:** C detects oscillating simplification (one class's merge
undone by another) by tracking modification indices. When a cycle is
detected, the responsible mapping is frozen. Rust has no per-mapping
freeze.

**C reference:** `cmpmap.c:3589-3620`, `cmpmap.c:2851-2995`

---

### 4.4 CmpMap: try_merge_series_in_parallel equal-length constraint

**Problem:** Rust requires both series CmpMaps to have the same
number of components. C uses dimension-matching to pair up
components regardless of list length.

**Rust location:** `simplify.rs:467`
**C reference:** `cmpmap.c:1629-1780`

---

### 4.5 CmpMap: BAD_VALUE placeholder in PermMap swap

**Problem:** When swapping a PermMap past a parallel CmpMap, the C
transforms PermMap constants through the CmpMap components. Rust
uses `BAD_VALUE` as placeholder.

**Rust location:** `simplify.rs:682-685`
**C reference:** `cmpmap.c:1929-1949`

---

### 4.6 PermMap: no parallel composition merging

**Problem:** C combines adjacent parallel PermMaps by offset-adjusting
coordinates into a single larger PermMap. Rust returns `None` for
non-series.

**Rust location:** `perm.rs:310`
**C reference:** `permmap.c:1088-1280`

---

### 4.7 PermMap: simplified() doesn't check inperm

**Problem:** Only checks `outperm` for identity. A PermMap with
identity `outperm` but non-identity `inperm` would incorrectly
simplify to UnitMap.

**Rust location:** `perm.rs:289-299`
**C reference:** `permmap.c:1283-1333`

---

### 4.8 PolyMap: no coefficient consolidation

**Problem:** C merges duplicate polynomial terms (same power
combination) by summing coefficients and removing zero-valued terms.
Rust `simplified()` is a no-op.

**Rust location:** `polymap.rs:1009`
**C reference:** `polymap.c:3606-3717`

---

### 4.9 PolyMap: no linear decomposition

**Problem:** When a PolyMap contains only constant and linear terms,
C decomposes it into ShiftMap + MatrixMap (enabling further
downstream simplification).

**Rust location:** Not implemented.
**C reference:** `polymap.c:3722-3848`

---

### 4.10 SpecMap: doesn't unconditionally merge adjacent SpecMaps

**Problem:** C always merges adjacent SpecMaps (combining step lists)
even if no steps cancel. This enables future simplification when a
third SpecMap appears. Rust only merges if cancellation occurs.

**Rust location:** `specmap.rs:1164`
**C reference:** `specmap.c:2259-2288`, `specmap.c:2551-2566`

---

### 4.11 ZoomMap: no parallel merge → diagonal MatrixMap

**Problem:** Adjacent parallel ZoomMaps with different factors should
produce a diagonal MatrixMap.

**Rust location:** `zoom.rs` `try_merge` returns `None` for
`!series`.
**C reference:** `zoommap.c:823-938`

---

### 4.12 WinMap: self-simplification to ShiftMap/MatrixMap

**Problem:** C replaces all-zero-shift WinMap with diagonal MatrixMap,
and all-unity-scale WinMap with ShiftMap. These converted forms can
then participate in merges with different partner types.

**Rust location:** `winmap.rs:197` (`simplified()` is a no-op)
**C reference:** `winmap.c:1082-1131`, `winmap.c:1817-1858`

---

### 4.13 MatrixMap: no swap-and-seek strategy

**Problem:** C searches past intermediate mappings for a merge target
(counting hops), then swaps toward it. Rust only considers immediate
neighbors.

**C reference:** `matrixmap.c:2063-2273`

---

### 4.14 MatrixMap: no Full-to-Diagonal self-simplification

**Problem:** C detects when a full-form matrix happens to be diagonal
and converts to diagonal form (enabling ZoomMap/UnitMap
simplification on the next pass).

**C reference:** `matrixmap.c:1887-1906`

---

### 4.15 ChebyMap: no try_merge at all

**Problem:** ChebyMap inherits PolyMap's logic in C but has no merge
capability in Rust.

**Rust location:** No `try_merge` override in `chebymap.rs`.

---

## 5. PERFORMANCE REGRESSIONS

### 5.1 Analytical rate() missing on all simple mappings

Every linear/trivial mapping falls through to `numeric_rate()` (2-point
central difference) instead of returning the known analytical value.

| Mapping | Analytical rate |
|---------|----------------|
| UnitMap | `if ax1==ax2 { 1.0 } else { 0.0 }` |
| ZoomMap | `if ax1==ax2 { zoom_factor } else { 0.0 }` (reciprocal if inverted) |
| ShiftMap | `if ax1==ax2 { 1.0 } else { 0.0 }` |
| MatrixMap | `matrix[ax1, ax2]` (element of effective forward matrix) |
| PermMap | `if outperm[ax1]==ax2 { 1.0 } else { 0.0 }` |

**Fix:** Override `rate()` on each type.

---

### 5.2 Base numeric_rate is primitive

C uses an adaptive 9-point algorithm with interval searching
(`mapping.c:8859-9152`). Rust uses a fixed-step 2-point central
difference (`rate.rs:1-36`). Less robust for complex nonlinear
mappings.

---

### 5.3 SlaMap: palMappa called per-point

The `palMappa` computation for AMP/MAP conversions is called inside the
per-point loop. It should be hoisted outside (parameters are constant
for all points in a transform call).

**Rust location:** `slamap.rs:790-796`
**C reference:** `slamap.c:242-246` (caching mechanism)

---

### 5.4 MatrixMap: no diagonal/unit fast-path in transforms

C maintains a `form` field (FULL/DIAGONAL/UNIT) and uses specialized
O(N) or O(1) transform paths. Rust always stores the full matrix and
does N² operations.

---

## 6. LOW PRIORITY / FEATURE GAPS

| Item | Notes |
|------|-------|
| `map_split` not implemented on any mapping | Used by CmpMap simplification, Region, FrameSet |
| `is_linear` attribute missing | Optimization hint used by framework |
| `DoNotSimplify` / `Ident` guard missing | Protects user-tagged mappings |
| `IsSimple` cache flag missing | Avoids redundant re-simplification |
| Invert-flag normalization in `simplified()` (ZoomMap, ShiftMap, PermMap) | Cosmetic; slightly aids downstream merges |
| WinMap rounding tolerance in composition | Snap near-zero/near-one intermediate results |
| WinMap explicit cancellation shortcut | Avoid arithmetic roundoff for exact inverses |
| PolyMap `MergeShift` (absorb adjacent ShiftMap algebraically) | Uncommon in practice |
| PolyMap `PolyTran` (fit inverse polynomial via LM) | Feature gap, not correctness |
| LutMap `LutEpsilon` attribute | Controls linearity tolerance |
| LutMap `is_bad` doesn't catch Inf; no Inf sanitization at construction | `helpers.rs:6` |
| WcsMap `NatLat`/`NatLon` attributes | Needed by FitsChan |
| TimeMap no-effect step elimination (MJDTOMJD with zero offset) | Part of 3.2 |
| MatrixMap determinant/IsUnitDet cache | API surface gap |
| MatrixMap BAD matrix element handling | Defensive; elements shouldn't be BAD |

---

## 7. RECOMMENDED FIX ORDER

The following order minimizes rework and maximizes test coverage
improvement at each step:

1. **Per-axis BAD handling** (§1.1) — architectural fix in
   `helpers.rs` or per-mapping; unblocks oracle test convergence.
2. **WcsMap parameter comparison** (§1.2) — small, isolated fix.
3. **Equality bugs** (§2.1–2.5) — fix before simplification work,
   since simplification relies on correct equality.
4. **SlaMap/TimeMap concatenation** (§3.1–3.2) — follow SpecMap
   pattern; unblocks celestial/time alignment tests.
5. **SphMap cancellation** (§3.3–3.4) — unblocks WCS simplification
   tests.
6. **Analytical rate()** (§5.1) — quick wins, one per mapping.
7. **WinMap parallel merge + MatrixMap merges** (§3.7–3.10) —
   unblocks wcsconverter regression tests.
8. **CmpMap improvements** (§4.1–4.5) — depth of simplification.
9. **Remaining medium-priority items** — as needed per test failures.

---

## 8. CROSS-CUTTING PATTERNS

Three systemic issues recur across most mappings:

1. **`point_has_bad` is wrong for axis-independent mappings.** Each
   mapping should check only the axes it reads for a given output.
   One architectural fix propagates to ~6 types.

2. **`rate()` should be overridden analytically** for all
   linear/trivial mappings. Currently everything falls through to
   numerical differentiation.

3. **`equal()` should compare effective behavior, not internal
   storage.** The C consistently resolves invert flags before
   comparing; the Rust consistently rejects differing flags. This
   needs a trait-level convention.

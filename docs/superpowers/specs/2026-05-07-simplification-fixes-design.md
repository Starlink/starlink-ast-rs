# Simplification Test Fixes Design

Date: 2026-05-07

## Goal

Fix the simplification regression test suite to move from 156 passed / 74
skipped / 3 not-yet-simplified to approximately 164 passed / 66 skipped / 0
not-yet-simplified. The 66 remaining skips are blocked by 13 entirely
unimplemented mapping classes (out of scope here).

## Current State

- **156 passed** (including 3 "not-yet-simplified" that are correct but less aggressive than C)
- **74 skipped** (66 from missing classes + 8 from fixable issues)
- **0 failed**

## Issues to Fix

### Issue 1: SpecMap Step Merging (fixes spec_inverse_cancel, spec_partial_cancel)

**File:** `crates/starlink-ast/src/mappings/specmap.rs`

**Problem:** The current `try_merge` only handles whole-SpecMap inverse
cancellation (two identical SpecMaps with opposite invert flags). It cannot:
- Merge step lists from adjacent forward SpecMaps into one
- Normalize inversion by swapping step codes (FRTOVL↔VLTOFR)
- Cancel adjacent inverse step pairs within a merged list

**Fix (following C `specmap.c:MapMerge` lines 2280-2580):**

Rewrite `SpecMap::try_merge` to:

1. Accept any adjacent SpecMap in series (regardless of invert flags).

2. Normalize both SpecMaps to forward form:
   - If `self_inv` is true: reverse step order, swap each step code for its
     inverse, and for frame-change steps (XF2HL/HLF2X), reciprocate the cached
     correction factor (last arg slot).
   - Same for `other_inv`.

3. Concatenate the normalized step lists.

4. Run a simplification loop that scans adjacent step pairs and eliminates
   inverse pairs:
   - No-arg pairs: ENTOFR↔FRTOEN, WNTOFR↔FRTOWN, WVTOFR↔FRTOWV,
     AWTOFR↔FRTOAW, VRTOVL↔VLTOVR, VOTOVL↔VLTOVO, ZOTOVL↔VLTOZO,
     BTTOVL↔VLTOBT
   - 1-arg pairs: FRTOVL↔VLTOFR (check args[0] equal)
   - 2-arg pairs: LKF2HL↔HLF2LK, LDF2HL↔HLF2LD, LGF2HL↔HLF2LG,
     GLF2HL↔HLF2GL (check args[0..2] equal)
   - 3-arg pairs: GEF2HL↔HLF2GE, BYF2HL↔HLF2BY, USF2HL↔HLF2US
     (check args[0..3] equal)
   - 6-arg pairs: TPF2HL↔HLF2TP (check args[0..6] equal)

5. Return:
   - If no steps remain: `UnitMap(1)`
   - If step count reduced: a new SpecMap with remaining steps
   - If no simplification: `None` (don't replace)

**Helper needed:** Add `SpecCvtType::inverse() -> Self` that returns the
inverse conversion code (FRTOVL→VLTOFR, etc.), and
`SpecCvtType::nargs_to_compare() -> usize` that returns how many user-supplied
args must match for cancellation (excluding the cached correction slot).

### Issue 2: PermMap Series Composition (fixes perm_bad_propagation)

**File:** `crates/starlink-ast/src/mappings/perm.rs`

**Problem:** `try_merge` only handles exact inverse cancellation. Cannot
compose two PermMaps' permutation arrays to produce a merged result.

**Test case:** PermMap([1,0], [1,2]) then PermMap([2,1], [1,2]) should merge to
PermMap([0,1], [1,2]) — the BAD propagates through the swap.

**Fix (following C `permmap.c:MapMerge` lines 910-1085):**

Add a series composition path to `PermMap::try_merge`:

1. When `series == true` and `other` is a PermMap:

2. Determine the effective forward and inverse permutations for each mapping
   based on its invert flag:
   - Forward PermMap: forward uses `outperm`, inverse uses `inperm`
   - Inverted PermMap: forward uses `inperm` as effective outperm, inverse
     uses `outperm` as effective inperm

3. Compose the forward permutation (outperm of result):
   ```
   for each output coord c of the composed mapping:
       p = first_mapping.effective_outperm[c]
       if p == 0: result[c] = 0  (BAD propagates)
       elif p < 0: result[c] = constant_ref  (constant propagates)
       else: result[c] = second_mapping.effective_outperm[p - 1]
   ```

4. Compose the inverse permutation (inperm of result): same process in reverse
   order using effective inperm arrays.

5. Merge constants from both PermMaps, re-indexing references in the composed
   perm arrays.

6. If the result is identity (outperm = [1, 2, ..., n], no constants), return
   UnitMap. Otherwise return the new PermMap.

**Convention in our code:** `outperm[i] = j` where j>0 means output i comes
from input j (1-based). j=0 means output is BAD. j<0 means output is constant
`constants[(-j)-1]`.

### Issue 3: Missing SlaMap Conversion Types (fixes 6 tests)

**File:** `crates/starlink-ast/src/mappings/slamap.rs`

**Tests fixed:** sla_j2000_cancel, sla_helioecl_cancel, sla_azel_cancel,
sla_hpc_cancel, sla_hpr_cancel, neg_sla_4arg_mismatch

**Missing types:**
| Code | Int | Description | Inverse | Args |
|------|-----|-------------|---------|------|
| J2000H | 23 | Dynamical J2000 → ICRS | HJ2000 | 0 |
| HJ2000 | 24 | ICRS → Dynamical J2000 | J2000H | 0 |
| HEEQ | 21 | Helio-ecliptic → equatorial | EQHE | 1 (date) |
| EQHE | 22 | Equatorial → helio-ecliptic | HEEQ | 1 (date) |
| H2E | 25 | Horizon → equatorial | E2H | 4 (phi, h, last, diurab) |
| E2H | 26 | Equatorial → horizon | H2E | 4 |
| HPCEQ | 15 | Helioprojective-Cartesian → equatorial | EQHPC | 4 (date, obsx, obsy, obsz) |
| EQHPC | 16 | Equatorial → helioprojective-Cartesian | HPCEQ | 4 |
| HPREQ | 17 | Helioprojective-Radial → equatorial | EQHPR | 4 |
| EQHPR | 18 | Equatorial → helioprojective-Radial | HPREQ | 4 |

**Implementation:** For each pair:
1. Add enum variants to `SlaCvtType`
2. Add `from_code` / `to_code` string dispatch
3. Add `inverse()` pair
4. Add `nargs()` / `szargs()` returning appropriate counts
5. Implement the transform function using the vendored PAL/ERFA routines

**C reference:** `reference/starlink-ast/src/slamap.c` contains the transform
implementations. J2000H uses the frame-tie matrix. HEEQ/EQHE use ecliptic
coordinate transforms. H2E/E2H use palDh2e/palDe2h. HPCEQ/EQHPC/HPREQ/EQHPR
use solar coordinate geometry.

### Issue 4: WcsMap Blank Projection Type (fixes wcsmap_bad_to_unit)

**File:** `crates/starlink-ast/src/mappings/wcsmap.rs`

**Problem:** `wcsmap_bad_to_unit.map` contains a WcsMap with `Type = "   "`
(blank/spaces). The parser rejects this. In C, this is valid — it creates a
WcsMap that simplifies to UnitMap.

**Fix:**
1. In `WcsMap::from_node`, accept blank/unknown type by storing a sentinel
   (e.g., `WcsProjection::Unknown` variant).
2. In `WcsMap::simplified()`, if type is Unknown, return `UnitMap(nin)`.
3. The transform for Unknown type should produce all BAD values (or pass
   through as identity — match C behavior).

### Issue 5: rigby MatrixMap Inverse Error

**File:** `crates/starlink-ast/src/mappings/cmpmap.rs` (or `matrix.rs`)

**Problem:** The `rigby.map` fixture has a deeply nested CmpMap tree with
`InvA=1` flags. The forward transform requires inverting sub-mappings. An
error "MatrixMap inverse is not defined (non-square or singular matrix)" is
thrown, but all matrices in the fixture ARE square and non-singular.

**Root cause (to investigate):** The CmpMap inversion logic or dimension
propagation may have a bug where it attempts to invert a non-square PermMap
path as if it were a MatrixMap, or the inversion flags are not being composed
correctly through the nested CmpMap tree.

**Fix approach:**
1. Trace the exact transform path to identify which MatrixMap is failing
2. Likely issue: `InvA` deserialization or how `CmpMap::transform_into`
   handles the effective direction when both the CmpMap and its child have
   inverted flags
3. Fix the flag composition to correctly XOR all inversion flags

## Implementation Order

1. **SpecMap step merging** (Issue 1) — self-contained, largest test impact
2. **PermMap composition** (Issue 2) — self-contained, one test
3. **SlaMap types** (Issue 3) — 5 new transforms, 6 tests
4. **WcsMap blank type** (Issue 4) — small fix, 1 test
5. **rigby CmpMap inversion** (Issue 5) — requires investigation, 1 test

## Test Strategy

Each fix is verified by running:
```shell
cargo test -p starlink-ast --test test_simplify_regression -- --nocapture
```

**Starting state:** 156 passed (includes 3 not-yet-simplified), 74 skipped

The 3 not-yet-simplified (spec_inverse_cancel, spec_partial_cancel,
perm_bad_propagation) are already counted in the 156 "passed" since they
produce correct transforms. After Issues 1-2, they move from "not-yet" to
"fully simplified" (no change in pass count, but 0 not-yet-simplified).

The 8 currently-skipping-but-fixable tests gain new passes:

- After Issue 1: 0 additional passes (just removes not-yet flag)
- After Issue 2: 0 additional passes (just removes not-yet flag)
- After Issue 3: +6 passed (sla_j2000_cancel, sla_helioecl_cancel,
  sla_azel_cancel, sla_hpc_cancel, sla_hpr_cancel, neg_sla_4arg_mismatch)
- After Issue 4: +1 passed (wcsmap_bad_to_unit)
- After Issue 5: +1 passed (rigby)

**Final target:** 164 passed, 66 skipped, 0 failed, 0 not-yet-simplified.
The 66 skipped are all blocked by unimplemented mapping classes (PcdMap,
GrismMap, TranMap, NormMap, UnitNormMap, MathMap, IntraMap, SelectorMap,
SwitchMap, SplineMap, RateMap, BoxRegion, Interval).

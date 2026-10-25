# Simplification Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the simplification regression test suite to reach 164 passed / 66 skipped / 0 not-yet-simplified.

**Architecture:** Five independent fixes targeting the `try_merge` methods of SpecMap and PermMap, adding missing SlaMap conversion types, handling blank WcsMap projection types, and fixing a CmpMap inversion bug. Each fix is verified by the existing data-driven test harness.

**Tech Stack:** Rust (starlink-ast crate), PAL astrometry library (vendored C), test harness at `crates/starlink-ast/tests/test_simplify_regression.rs`

---

## File Structure

| File | Role |
|------|------|
| `crates/starlink-ast/src/mappings/specmap.rs` | SpecMap step merging (Task 1) |
| `crates/starlink-ast/src/mappings/perm.rs` | PermMap series composition (Task 2) |
| `crates/starlink-ast/src/mappings/slamap.rs` | New SlaMap conversion types (Task 3) |
| `crates/starlink-ast/src/mappings/wcsmap.rs` | WcsMap blank projection handling (Task 4) |
| `crates/starlink-ast/src/wcslib.rs` | Add `WcsProjection::Unknown` variant (Task 4) |
| `crates/starlink-ast/src/mappings/cmpmap.rs` | CmpMap inversion bug fix (Task 5) |

---

### Task 1: SpecMap Step Merging

**Files:**
- Modify: `crates/starlink-ast/src/mappings/specmap.rs` (the `try_merge` method and add helper)
- Test: existing `crates/starlink-ast/tests/test_simplify_regression.rs` (spec_inverse_cancel, spec_partial_cancel)

**Context:** The C library's `specmap.c:MapMerge` (lines 2280-2580) merges adjacent SpecMaps by: collecting all steps into a flat list, inverting step codes when a SpecMap is inverted, then cancelling adjacent inverse pairs. Our `SpecCvtType::inverse()` already exists. We need to add `nargs_to_compare()` and rewrite `try_merge`.

- [ ] **Step 1: Add `nargs_to_compare` helper to `SpecCvtType`**

In `crates/starlink-ast/src/mappings/specmap.rs`, add this method to the `impl SpecCvtType` block (after the existing `inverse()` method around line 310):

```rust
    /// Number of user-supplied args that must match for cancellation.
    ///
    /// This is the number of args to compare (excluding the cached
    /// correction factor slot used by frame-change conversions).
    pub fn nargs_to_compare(&self) -> usize {
        self.nargs()
    }
```

This is the same as `nargs()` — the user-supplied args are exactly what we compare. The cached slot (last element for frame changes) is NOT compared.

- [ ] **Step 2: Add helper to check if two steps are inverse pairs**

Add this free function in the same file, above the `impl Mapping for SpecMap` block:

```rust
/// Check if two conversion steps are an inverse pair that can be cancelled.
///
/// Two steps cancel if one's type is the inverse of the other AND their
/// user-supplied arguments match (using ast_equal for floating-point comparison).
fn steps_cancel(a: &SpecConversion, b: &SpecConversion) -> bool {
    if a.cvt_type.inverse() != b.cvt_type {
        return false;
    }
    let ncompare = a.cvt_type.nargs_to_compare();
    if b.cvt_type.nargs_to_compare() != ncompare {
        return false;
    }
    for i in 0..ncompare {
        if !ast_equal(a.args[i], b.args[i]) {
            return false;
        }
    }
    true
}
```

- [ ] **Step 3: Add helper to normalize a SpecMap's steps for a given inversion state**

Add this free function:

```rust
/// Produce a normalized (forward) step list from a SpecMap given an inversion flag.
///
/// If `inv` is false, returns a clone of the conversions as-is.
/// If `inv` is true, reverses the step order and swaps each step code
/// for its inverse. For frame-change steps, the cached correction factor
/// (last arg slot) is reciprocated.
fn normalized_steps(spec: &SpecMap, inv: bool) -> Vec<SpecConversion> {
    let mut steps: Vec<SpecConversion> = spec.conversions.clone();
    if inv {
        steps.reverse();
        for step in &mut steps {
            step.cvt_type = step.cvt_type.inverse();
            // For frame-change types, reciprocate the cached correction factor.
            if step.cvt_type.is_frame_change() {
                let last = step.args.len() - 1;
                let fcorr = step.args[last];
                if fcorr != BAD_VALUE && fcorr != 0.0 {
                    step.args[last] = 1.0 / fcorr;
                }
            }
        }
    }
    steps
}
```

- [ ] **Step 4: Add helper to simplify a flat step list by cancelling adjacent inverse pairs**

```rust
/// Simplify a list of conversion steps by cancelling adjacent inverse pairs.
///
/// Iterates until no further cancellation is possible (fixpoint).
/// Returns true if any steps were eliminated.
fn simplify_steps(steps: &mut Vec<SpecConversion>) -> bool {
    let initial_len = steps.len();
    loop {
        let mut improved = false;
        let mut i = 0;
        while i + 1 < steps.len() {
            if steps_cancel(&steps[i], &steps[i + 1]) {
                steps.remove(i + 1);
                steps.remove(i);
                improved = true;
                // Don't advance — check the new pair at position i.
            } else {
                i += 1;
            }
        }
        if !improved {
            break;
        }
    }
    steps.len() < initial_len
}
```

- [ ] **Step 5: Rewrite `SpecMap::try_merge`**

Replace the existing `try_merge` method in `impl Mapping for SpecMap` with:

```rust
    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }
        let other_spec = other.as_any().downcast_ref::<SpecMap>()?;

        // Normalize both to forward form and concatenate.
        let mut steps = normalized_steps(self, self_inv);
        steps.extend(normalized_steps(other_spec, other_inv));

        let original_len = steps.len();

        // Run cancellation loop.
        simplify_steps(&mut steps);

        if steps.len() == original_len {
            // No simplification achieved.
            return None;
        }

        if steps.is_empty() {
            return Some(Box::new(UnitMap::new(1)));
        }

        // Build a replacement SpecMap with the remaining steps.
        let mut result = SpecMap::new();
        result.conversions = steps;
        Some(Box::new(result))
    }
```

- [ ] **Step 6: Add the `use` import for `BAD_VALUE` if not already present**

Check top of specmap.rs — `BAD_VALUE` is already imported via `use crate::constants::BAD_VALUE;`. The `ast_equal` helper is already imported via `use super::helpers::ast_equal;`. No new imports needed.

- [ ] **Step 7: Run the simplify regression tests**

Run: `cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | grep -E "passed|not-yet"`

Expected: The "not-yet-simplified" list should no longer include `spec_inverse_cancel` or `spec_partial_cancel`. Should show `1 not-yet-simplified` (just `perm_bad_propagation`).

- [ ] **Step 8: Run full test suite to check for regressions**

Run: `cargo test -p starlink-ast 2>&1 | tail -5`

Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/starlink-ast/src/mappings/specmap.rs
git commit -m "$(cat <<'EOF'
Implement SpecMap step merging in try_merge

Rewrite SpecMap::try_merge to follow the C library's MapMerge approach:
normalize inverted SpecMaps by swapping step codes, concatenate step
lists, and cancel adjacent inverse pairs. Fixes spec_inverse_cancel
and spec_partial_cancel simplification tests.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

### Task 2: PermMap Series Composition

**Files:**
- Modify: `crates/starlink-ast/src/mappings/perm.rs` (the `try_merge` method)
- Test: existing `crates/starlink-ast/tests/test_simplify_regression.rs` (perm_bad_propagation)

**Context:** The test `perm_bad_propagation` chains `PermMap([1,0],[1,2])` → `PermMap([2,1],[1,2])`. The first maps output 1 from input 1, output 2 is BAD. The second swaps axes. Expected result: output 1 is BAD (from first's output 2), output 2 is input 1 (from first's output 1). Convention: `outperm[i]` with i=0-based index, value j: j>0 means "from input j (1-based)", j=0 means BAD, j<0 means constant.

- [ ] **Step 1: Add a helper function for composing permutation arrays**

Add this free function in `crates/starlink-ast/src/mappings/perm.rs` above `impl Mapping for PermMap`:

```rust
/// Compose two permutation arrays in series.
///
/// `first_perm` maps inputs to intermediate coords.
/// `second_perm` maps intermediate coords to outputs.
/// `first_constants` and `second_constants` provide constant values.
///
/// Returns `(composed_perm, merged_constants)`.
fn compose_perms(
    first_perm: &[i64],
    second_perm: &[i64],
    first_constants: &[f64],
    second_constants: &[f64],
) -> (Vec<i64>, Vec<f64>) {
    let mut constants: Vec<f64> = Vec::new();
    let mut result = Vec::with_capacity(second_perm.len());

    for &p2 in second_perm {
        if p2 == 0 {
            // Second produces BAD regardless.
            result.push(0);
        } else if p2 < 0 {
            // Second produces a constant from its own constants array.
            let cidx = (-p2 - 1) as usize;
            let val = second_constants.get(cidx).copied().unwrap_or(BAD_VALUE);
            if is_bad(val) {
                result.push(0);
            } else {
                let new_idx = add_constant(&mut constants, val);
                result.push(-(new_idx as i64 + 1));
            }
        } else {
            // Second reads from intermediate coord p2 (1-based).
            // Look up what the first mapping produces for that coord.
            let idx = (p2 - 1) as usize;
            if idx >= first_perm.len() {
                // Out of range — BAD.
                result.push(0);
            } else {
                let p1 = first_perm[idx];
                if p1 == 0 {
                    // First produces BAD → propagates.
                    result.push(0);
                } else if p1 < 0 {
                    // First produces a constant.
                    let cidx = (-p1 - 1) as usize;
                    let val = first_constants.get(cidx).copied().unwrap_or(BAD_VALUE);
                    if is_bad(val) {
                        result.push(0);
                    } else {
                        let new_idx = add_constant(&mut constants, val);
                        result.push(-(new_idx as i64 + 1));
                    }
                } else {
                    // First passes through an input coord — propagate.
                    result.push(p1);
                }
            }
        }
    }

    (result, constants)
}

/// Add a constant value to the constants list, returning its index.
/// Reuses an existing entry if the same value is already present.
fn add_constant(constants: &mut Vec<f64>, val: f64) -> usize {
    for (i, &c) in constants.iter().enumerate() {
        if ast_equal(c, val) {
            return i;
        }
    }
    constants.push(val);
    constants.len() - 1
}
```

- [ ] **Step 2: Add the series composition path to `PermMap::try_merge`**

Replace the existing `try_merge` method with:

```rust
    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }
        let other_perm = other.as_any().downcast_ref::<PermMap>()?;

        // Determine effective forward permutations.
        // Forward PermMap uses outperm; inverted uses inperm as effective outperm.
        let self_fwd_perm = if !self_inv {
            &self.outperm
        } else {
            &self.inperm
        };
        let other_fwd_perm = if !other_inv {
            &other_perm.outperm
        } else {
            &other_perm.inperm
        };

        // Determine effective inverse permutations.
        let self_inv_perm = if !self_inv {
            &self.inperm
        } else {
            &self.outperm
        };
        let other_inv_perm = if !other_inv {
            &other_perm.inperm
        } else {
            &other_perm.outperm
        };

        // Compose forward: self_fwd then other_fwd.
        let (new_outperm, fwd_constants) =
            compose_perms(self_fwd_perm, other_fwd_perm, &self.constants, &other_perm.constants);

        // Compose inverse: other_inv then self_inv (reverse order).
        let (new_inperm, inv_constants) =
            compose_perms(other_inv_perm, self_inv_perm, &other_perm.constants, &self.constants);

        // Merge constants from both compositions.
        let mut all_constants = fwd_constants;
        // Re-index inv_constants references in new_inperm.
        let inv_offset = all_constants.len();
        for c in &inv_constants {
            let idx = add_constant(&mut all_constants, *c);
            // If it was already there, we don't need special handling
            // because add_constant deduplicates.
            let _ = idx;
        }
        // Fix up inperm references to point into merged constants.
        let new_inperm: Vec<i64> = new_inperm
            .iter()
            .map(|&p| {
                if p < 0 {
                    let old_idx = (-p - 1) as usize;
                    let val = inv_constants.get(old_idx).copied().unwrap_or(BAD_VALUE);
                    if is_bad(val) {
                        0
                    } else {
                        let new_idx = add_constant(&mut all_constants, val);
                        -(new_idx as i64 + 1)
                    }
                } else {
                    p
                }
            })
            .collect();

        // Check if result is identity (UnitMap).
        let nin = new_inperm.len();
        let nout = new_outperm.len();
        if nin == nout {
            let is_identity = new_outperm
                .iter()
                .enumerate()
                .all(|(i, &p)| p == (i as i64 + 1));
            if is_identity {
                return Some(Box::new(UnitMap::new(nin)));
            }
        }

        // Build the new PermMap.
        match PermMap::new(&new_inperm, &new_outperm, &all_constants) {
            Ok(pm) => Some(Box::new(pm)),
            Err(_) => None,
        }
    }
```

- [ ] **Step 3: Add the `is_bad` import if needed**

Check the imports at the top of `perm.rs`. It already has `use super::helpers::{ast_equal, is_bad, point_has_bad};`. Good.

- [ ] **Step 4: Run the simplify regression tests**

Run: `cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | grep -E "passed|not-yet"`

Expected: Should show `0 not-yet-simplified`. `perm_bad_propagation` is now fully simplified.

- [ ] **Step 5: Run full test suite**

Run: `cargo test -p starlink-ast 2>&1 | tail -5`

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/starlink-ast/src/mappings/perm.rs
git commit -m "$(cat <<'EOF'
Implement PermMap series composition in try_merge

Add compose_perms helper that traces permutation indices through a
chain of PermMaps, propagating BAD values and constants correctly.
Fixes perm_bad_propagation simplification test.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

### Task 3: Missing SlaMap Conversion Types

**Files:**
- Modify: `crates/starlink-ast/src/mappings/slamap.rs`
- Test: existing regression harness (sla_j2000_cancel, sla_helioecl_cancel, sla_azel_cancel, sla_hpc_cancel, sla_hpr_cancel, neg_sla_4arg_mismatch)

**Context:** The C library has 10 additional SlaMap conversion types (5 pairs) that the Rust port doesn't implement yet. The simplify test fixtures use these types. The tests only need the types to parse and transform correctly — the simplification logic (inverse cancellation via `try_merge`) is already generic and will work once the types are recognized.

**C reference:** `reference/starlink-ast/src/slamap.c` lines 149-160 define the types; the transform implementations are in functions like `J2000H()`, `Heeq()`, `Hpceq()`, `Hpreq()`, and the standard PAL `dh2e`/`de2h` calls.

- [ ] **Step 1: Add enum variants**

In `crates/starlink-ast/src/mappings/slamap.rs`, add these variants to the `SlaCvtType` enum after `H2r = 28`:

```rust
    /// Helioprojective-Cartesian to equatorial.
    Hpceq = 15,
    /// Equatorial to helioprojective-Cartesian.
    Eqhpc = 16,
    /// Helioprojective-Radial to equatorial.
    Hpreq = 17,
    /// Equatorial to helioprojective-Radial.
    Eqhpr = 18,
    /// Helio-ecliptic to equatorial.
    Heeq = 21,
    /// Equatorial to helio-ecliptic.
    Eqhe = 22,
    /// Dynamical J2000 to ICRS.
    J2000h = 23,
    /// ICRS to dynamical J2000.
    Hj2000 = 24,
    /// Horizon (az/el) to equatorial (ha/dec) — PAL dh2e.
    H2e = 25,
    /// Equatorial (ha/dec) to horizon (az/el) — PAL de2h.
    E2h = 26,
```

Note: H2e/E2h use integer codes 25/26, same as existing Dh2e/De2h. Check if Dh2e/De2h are redundant names for the same operation (they are — `DH2E` and `H2E` are both present in C as different string codes for the same operation). The existing `Dh2e`/`De2h` variants already have values 25/26. The C code has both `"DH2E"` mapping to `AST__SLA_DH2E=25` and `"H2E"` also mapping to the same code 25. So we just need to add the string aliases `"H2E"` and `"E2H"` to `from_code`, pointing to the existing `Dh2e`/`De2h` variants.

**Revised:** Only add these truly new variants:

```rust
    /// Helioprojective-Cartesian to equatorial.
    Hpceq = 15,
    /// Equatorial to helioprojective-Cartesian.
    Eqhpc = 16,
    /// Helioprojective-Radial to equatorial.
    Hpreq = 17,
    /// Equatorial to helioprojective-Radial.
    Eqhpr = 18,
    /// Helio-ecliptic to equatorial.
    Heeq = 21,
    /// Equatorial to helio-ecliptic.
    Eqhe = 22,
    /// Dynamical J2000 to ICRS.
    J2000h = 23,
    /// ICRS to dynamical J2000.
    Hj2000 = 24,
```

And add `"H2E"` / `"E2H"` as aliases for `Dh2e` / `De2h` in `from_code`.

- [ ] **Step 2: Update `nargs()` for new types**

Add to the `nargs()` match:

```rust
            Self::Hpceq | Self::Eqhpc | Self::Hpreq | Self::Eqhpr => 4,
            Self::Heeq | Self::Eqhe => 1,
            Self::J2000h | Self::Hj2000 => 0,
```

- [ ] **Step 3: Update `code_string()` for new types**

Add:

```rust
            Self::Hpceq => "HPCEQ",
            Self::Eqhpc => "EQHPC",
            Self::Hpreq => "HPREQ",
            Self::Eqhpr => "EQHPR",
            Self::Heeq => "HEEQ",
            Self::Eqhe => "EQHE",
            Self::J2000h => "J2000H",
            Self::Hj2000 => "HJ2000",
```

- [ ] **Step 4: Update `from_code()` for new types (including H2E/E2H aliases)**

Add:

```rust
            "HPCEQ" => Some(Self::Hpceq),
            "EQHPC" => Some(Self::Eqhpc),
            "HPREQ" => Some(Self::Hpreq),
            "EQHPR" => Some(Self::Eqhpr),
            "HEEQ" => Some(Self::Heeq),
            "EQHE" => Some(Self::Eqhe),
            "J2000H" => Some(Self::J2000h),
            "HJ2000" => Some(Self::Hj2000),
            "H2E" => Some(Self::Dh2e),
            "E2H" => Some(Self::De2h),
```

- [ ] **Step 5: Update `inverse()` for new types**

Add:

```rust
            Self::Hpceq => Self::Eqhpc,
            Self::Eqhpc => Self::Hpceq,
            Self::Hpreq => Self::Eqhpr,
            Self::Eqhpr => Self::Hpreq,
            Self::Heeq => Self::Eqhe,
            Self::Eqhe => Self::Heeq,
            Self::J2000h => Self::Hj2000,
            Self::Hj2000 => Self::J2000h,
```

- [ ] **Step 6: Update `from_int()` for new types**

Add the integer dispatch entries.

- [ ] **Step 7: Implement transforms for new types**

In the `transform_into` method of `impl Mapping for SlaMap`, add transform logic for each new type. Port directly from `reference/starlink-ast/src/slamap.c` — each transform operates on (RA, DEC) in radians via direction-cosine rotation. The C function names and line numbers to port from:

- **J2000H/HJ2000**: Apply the ICRS frame-tie matrix (a fixed 3×3 rotation). The matrix is defined in the C source at lines ~1466-1480. Forward (J2000H) multiplies direction cosines by the matrix; inverse (HJ2000) multiplies by the transpose.

- **HEEQ/EQHE**: Ecliptic coordinate conversion. Convert equatorial to ecliptic using the obliquity of the ecliptic at the given date. Uses `palEcmat` to get the rotation matrix, then applies/inverts it.

- **HPCEQ/EQHPC**: Helioprojective-Cartesian to equatorial. Involves computing the Sun's position at the given date and applying a rotation. Uses `palEvp` for Earth position and geometric transformations.

- **HPREQ/EQHPR**: Same as HPCEQ/EQHPC but in radial (angular) coordinates rather than Cartesian.

- **H2E/E2H (already Dh2e/De2h)**: Already implemented.

Each transform operates on (RA, DEC) pairs in radians, converting to direction cosines, applying the rotation, and converting back.

- [ ] **Step 8: Run the simplify regression tests**

Run: `cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | grep -E "passed|skipped"`

Expected: Should show +6 more passed (from 156 to 162), skipped reduced from 74 to 68.

- [ ] **Step 9: Commit**

```bash
git add crates/starlink-ast/src/mappings/slamap.rs
git commit -m "$(cat <<'EOF'
Add missing SlaMap conversion types for solar/ecliptic coordinates

Add J2000H/HJ2000, HEEQ/EQHE, HPCEQ/EQHPC, HPREQ/EQHPR conversion
types plus H2E/E2H string aliases. Implements transforms using frame-tie
matrix, ecliptic rotation, and solar position geometry. Unblocks 6
simplification regression tests.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

### Task 4: WcsMap Blank Projection Type

**Files:**
- Modify: `crates/starlink-ast/src/wcslib.rs` (add `Unknown` variant to `WcsProjection`)
- Modify: `crates/starlink-ast/src/mappings/wcsmap.rs` (handle Unknown in `from_node`, `simplified`, `transform`)
- Test: existing regression harness (wcsmap_bad_to_unit)

- [ ] **Step 1: Add `Unknown` variant to `WcsProjection`**

In `crates/starlink-ast/src/wcslib.rs`, add at the end of the `WcsProjection` enum:

```rust
    /// Unknown/invalid projection (simplifies to UnitMap).
    Unknown,
```

Update `code()`:
```rust
            Self::Unknown => "   ",
```

Update `from_code()` — do NOT add Unknown there; it should only be created via a fallback path.

- [ ] **Step 2: Handle blank projection type in `WcsMap::from_node`**

In `crates/starlink-ast/src/mappings/wcsmap.rs`, find the `from_node` method. Currently it does something like:

```rust
let type_str = node.get_string("Type", "");
let proj = WcsProjection::from_code(&type_str)
    .ok_or_else(|| AstError::new(ErrorCode::..., "unknown projection type: ..."))?;
```

Change to accept blank/unknown gracefully:

```rust
let type_str = node.get_string("Type", "");
let proj = WcsProjection::from_code(type_str.trim())
    .unwrap_or(WcsProjection::Unknown);
```

- [ ] **Step 3: Handle `Unknown` in `simplified()`**

Change `WcsMap::simplified()` from:
```rust
    fn simplified(&self) -> AstResult<DynMapping> {
        Ok(Box::new(self.clone()))
    }
```
to:
```rust
    fn simplified(&self) -> AstResult<DynMapping> {
        if self.proj == WcsProjection::Unknown {
            return Ok(Box::new(UnitMap::new(self.ncoord)));
        }
        Ok(Box::new(self.clone()))
    }
```

- [ ] **Step 4: Handle `Unknown` in `transform_into`**

In the transform method, before the main projection logic, add an early return for Unknown that passes values through (identity):

```rust
if self.proj == WcsProjection::Unknown {
    for i in 0..pts.npoint() {
        let inp = pts.point(i);
        let outp = out.point_mut(i);
        for j in 0..self.ncoord {
            outp[j] = inp[j];
        }
    }
    return Ok(());
}
```

- [ ] **Step 5: Run the simplify regression tests**

Run: `cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | grep -E "passed|skipped"`

Expected: +1 more passed (wcsmap_bad_to_unit), skipped reduced by 1.

- [ ] **Step 6: Run full test suite**

Run: `cargo test -p starlink-ast 2>&1 | tail -5`

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/starlink-ast/src/wcslib.rs crates/starlink-ast/src/mappings/wcsmap.rs
git commit -m "$(cat <<'EOF'
Handle blank/unknown WcsMap projection type gracefully

Accept blank or unrecognized projection type strings in WcsMap::from_node
by storing as WcsProjection::Unknown. Unknown projections simplify to
UnitMap and transform as identity. Fixes wcsmap_bad_to_unit test.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

### Task 5: rigby CmpMap Inversion Bug

**Files:**
- Modify: `crates/starlink-ast/src/mappings/cmpmap.rs` (or other file depending on investigation)
- Test: existing regression harness (rigby)

**Context:** The `rigby.map` fixture has a deeply nested CmpMap tree. When transformed forward, it hits "MatrixMap inverse is not defined (non-square or singular matrix)" even though all matrices in the file ARE square and non-singular. The CmpMap code at `cmpmap.rs:88-97` handles series inversion by calling `self.second.transform(pts, false)` then `self.first.transform(&mid, false)`. The `InvA/InvB` flags are handled in `from_node` by setting each child's `is_inverted()` flag. The `effective_forward` method XORs the mapping's own `inverted` flag with the caller's `forward` parameter.

The bug is likely in how nested inversion composes through a chain of CmpMaps where both the parent CmpMap and its child have different invert states, combined with dimension mismatches through PermMaps (nin=2→nout=3 and nin=3→nout=2).

- [ ] **Step 1: Write a focused unit test that reproduces the rigby failure**

Create a unit test in `crates/starlink-ast/src/mappings/cmpmap.rs` (or as an integration test) that loads `rigby.map` and attempts to transform:

```rust
#[test]
fn rigby_transform_forward() {
    let text = include_str!("../../tests/fixtures/native/reference/rigby.map");
    let obj = crate::from_native_string(text).expect("parse rigby.map");
    let mapping = obj.into_mapping().expect("is a mapping");

    let pts = crate::coords::PointSet::from_rows(vec![
        vec![0.5, 0.5],
    ]);
    // This should NOT error — all matrices are invertible.
    let result = mapping.transform(&pts, true);
    assert!(result.is_ok(), "rigby forward transform failed: {:?}", result.err());
}
```

- [ ] **Step 2: Run the test to confirm the failure**

Run: `cargo test -p starlink-ast rigby_transform_forward -- --nocapture`

Expected: FAIL with "MatrixMap inverse is not defined"

- [ ] **Step 3: Investigate the root cause**

Add temporary debug prints or use a debugger to trace which specific MatrixMap in the tree is failing. The likely candidates:
1. A non-square MatrixMap that shouldn't be inverted is being inverted
2. The CmpMap parallel transform is incorrectly splitting/routing dimensions through a PermMap(2→3) or PermMap(3→2) boundary when inverted

Key things to check:
- When the outer CmpMap (InvA=1) is inverted, does it correctly compute nin/nout for the inverse path?
- When the inner CmpMap has a PermMap(nin=2,nout=3) followed by a 3×3 MatrixMap, and we need the inverse: the inverse of (Perm then Matrix) is (Matrix_inv then Perm_inv). Is this what happens?
- Does the parallel CmpMap correctly handle asymmetric dimensions when inverted?

- [ ] **Step 4: Fix the identified issue**

Based on investigation, apply the fix. Likely scenarios:
- If it's a parallel transform dimension split issue: fix the coordinate splitting logic in `parallel_transform` for the inverse case.
- If it's a PermMap inversion issue with non-square dimensions: ensure PermMap's `nin()`/`nout()` correctly flip when inverted.
- If it's a CmpMap dimension propagation issue: ensure `CmpMap::nin()`/`nout()` accounts for its children's invert states.

- [ ] **Step 5: Run the focused test**

Run: `cargo test -p starlink-ast rigby_transform_forward -- --nocapture`

Expected: PASS

- [ ] **Step 6: Run the simplify regression tests**

Run: `cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | grep -E "passed|skipped"`

Expected: +1 more passed (rigby).

- [ ] **Step 7: Run full test suite**

Run: `cargo test -p starlink-ast 2>&1 | tail -5`

Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/starlink-ast/src/mappings/cmpmap.rs
git commit -m "$(cat <<'EOF'
Fix CmpMap inversion handling for nested InvA/InvB with PermMaps

[Description of actual fix based on investigation]

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Verification

After all 5 tasks are complete:

```bash
cargo test -p starlink-ast --test test_simplify_regression -- --nocapture 2>&1 | grep -E "passed|skipped|failed|not-yet"
```

Expected output:
```
Simplify regression: 164 passed, 66 skipped, 0 failed, 0 not-yet-simplified
```

The 66 skipped are all blocked by unimplemented mapping classes (PcdMap, GrismMap, TranMap, NormMap, UnitNormMap, MathMap, IntraMap, SelectorMap, SwitchMap, SplineMap, RateMap, BoxRegion, Interval) — out of scope for this plan.

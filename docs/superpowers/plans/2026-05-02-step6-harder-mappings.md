# Step 6: Harder Mappings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement WinMap, LutMap, PolyMap, and ChebyMap with full Mapping trait impls, Serializable impls, dispatch updates, and tests.

**Architecture:** Four new concrete mapping types following the established pattern (struct + Mapping trait + Serializable). ChebyMap wraps PolyMap via composition. PolyMap's iterative inverse uses the `levenberg-marquardt` crate. All types are added to the AnyObject dispatch.

**Tech Stack:** Rust 2024 edition. New dependency: `levenberg-marquardt = "0.15"` for PolyMap iterative inverse.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `Cargo.toml` (workspace root) | Add `levenberg-marquardt` workspace dependency |
| `crates/starlink-ast/Cargo.toml` | Add `levenberg-marquardt` dependency |
| `crates/starlink-ast/src/mappings/winmap.rs` | WinMap struct, Mapping impl, Serializable impl |
| `crates/starlink-ast/src/mappings/lutmap.rs` | LutMap + LutInterp, Mapping impl, Serializable impl |
| `crates/starlink-ast/src/mappings/polymap.rs` | PolyMap + PolyTerm, Mapping impl, Serializable impl, LM inverse |
| `crates/starlink-ast/src/mappings/chebymap.rs` | ChebyMap (wraps PolyMap), Mapping impl, Serializable impl |
| `crates/starlink-ast/src/mappings/mod.rs` | Add new modules and re-exports |
| `crates/starlink-ast/src/serial/mod.rs` | Update all dispatch points for 4 new types |
| `crates/starlink-ast/tests/test_winmap.rs` | WinMap tests |
| `crates/starlink-ast/tests/test_lutmap.rs` | LutMap tests |
| `crates/starlink-ast/tests/test_polymap.rs` | PolyMap tests |
| `crates/starlink-ast/tests/test_chebymap.rs` | ChebyMap tests |

---

### Task 1: Add levenberg-marquardt Dependency

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/starlink-ast/Cargo.toml`

- [ ] **Step 1: Add workspace dependency**

In the workspace root `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
levenberg-marquardt = "0.15"
```

- [ ] **Step 2: Add to starlink-ast**

In `crates/starlink-ast/Cargo.toml`, add to `[dependencies]`:

```toml
levenberg-marquardt = { workspace = true }
```

The `levenberg-marquardt` crate depends on `nalgebra`. This is a
transitive dependency — no need to add it explicitly unless we use
it directly.

- [ ] **Step 3: Verify build**

Run: `cargo build -p starlink-ast`
Expected: success (the dependency is pulled but not yet used)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/starlink-ast/Cargo.toml Cargo.lock
git commit -m "Add levenberg-marquardt dependency for PolyMap iterative inverse"
```

---

### Task 2: WinMap

**Files:**
- Create: `crates/starlink-ast/src/mappings/winmap.rs`
- Modify: `crates/starlink-ast/src/mappings/mod.rs`
- Create: `crates/starlink-ast/tests/test_winmap.rs`

WinMap is a per-axis linear transformation: `out[i] = shift[i] + scale[i] * in[i]`.

- [ ] **Step 1: Create winmap.rs with struct and constructors**

Create `crates/starlink-ast/src/mappings/winmap.rs`:

```rust
//! WinMap — per-axis linear (shift + scale) transformation.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{DynMapping, Mapping};

/// Per-axis independent linear transformation.
///
/// Each axis is transformed by `output = shift + scale * input`.
#[derive(Debug, Clone)]
pub struct WinMap {
    shifts: Vec<f64>,
    scales: Vec<f64>,
    inverted: bool,
}

impl WinMap {
    /// Create a WinMap from explicit shift and scale arrays.
    pub fn new(shifts: &[f64], scales: &[f64]) -> AstResult<Self> {
        if shifts.len() != scales.len() || shifts.is_empty() {
            return Err(AstError::new(
                ErrorCode::Wmint,
                "WinMap shifts and scales must have equal non-zero length",
            ));
        }
        Ok(Self {
            shifts: shifts.to_vec(),
            scales: scales.to_vec(),
            inverted: false,
        })
    }

    /// Create a WinMap that maps one rectangular window to another.
    ///
    /// Computes shift and scale per axis from two corner points in
    /// the input and output coordinate systems.
    pub fn from_corners(
        corner1_in: &[f64],
        corner2_in: &[f64],
        corner1_out: &[f64],
        corner2_out: &[f64],
    ) -> AstResult<Self> {
        let n = corner1_in.len();
        if n == 0
            || corner2_in.len() != n
            || corner1_out.len() != n
            || corner2_out.len() != n
        {
            return Err(AstError::new(
                ErrorCode::Wmint,
                "WinMap corner arrays must have equal non-zero length",
            ));
        }
        let mut shifts = vec![0.0; n];
        let mut scales = vec![1.0; n];
        for i in 0..n {
            let denom = corner2_in[i] - corner1_in[i];
            if denom != 0.0
                && corner1_in[i] != BAD_VALUE
                && corner2_in[i] != BAD_VALUE
                && corner1_out[i] != BAD_VALUE
                && corner2_out[i] != BAD_VALUE
            {
                scales[i] = (corner2_out[i] - corner1_out[i]) / denom;
                shifts[i] = corner1_out[i] - scales[i] * corner1_in[i];
            } else {
                scales[i] = BAD_VALUE;
                shifts[i] = BAD_VALUE;
            }
        }
        Ok(Self {
            shifts,
            scales,
            inverted: false,
        })
    }

    /// Access the shift array.
    pub(crate) fn shifts(&self) -> &[f64] {
        &self.shifts
    }

    /// Access the scale array.
    pub(crate) fn scales(&self) -> &[f64] {
        &self.scales
    }
}
```

- [ ] **Step 2: Implement Mapping trait**

Add to the same file:

```rust
impl Mapping for WinMap {
    fn nin(&self) -> usize { self.shifts.len() }
    fn nout(&self) -> usize { self.shifts.len() }
    fn is_inverted(&self) -> bool { self.inverted }
    fn invert(&mut self) { self.inverted = !self.inverted; }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = forward != self.inverted;
        let ncoord = self.shifts.len();
        let npoint = pts.npoint();
        let mut out = PointSet::zeros(npoint, ncoord);

        for ip in 0..npoint {
            let p = pts.point(ip);
            let mut o = out.point_mut(ip);
            for ic in 0..ncoord {
                let v = p[ic];
                if v == BAD_VALUE
                    || self.shifts[ic] == BAD_VALUE
                    || self.scales[ic] == BAD_VALUE
                {
                    o[ic] = BAD_VALUE;
                } else if fwd {
                    o[ic] = self.shifts[ic] + self.scales[ic] * v;
                } else if self.scales[ic] != 0.0 {
                    o[ic] = (v - self.shifts[ic]) / self.scales[ic];
                } else {
                    o[ic] = BAD_VALUE;
                }
            }
        }
        Ok(out)
    }

    fn transform_into(
        &self, pts: &PointSet, forward: bool, out: &mut PointSet,
    ) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        // Rate of a linear map: scale if same axis, 0 otherwise
        if ax1 == ax2 && ax1 >= 1 && ax1 <= self.shifts.len() {
            let idx = ax1 - 1;
            if self.inverted {
                if self.scales[idx] != 0.0 {
                    Ok(1.0 / self.scales[idx])
                } else {
                    Ok(BAD_VALUE)
                }
            } else {
                Ok(self.scales[idx])
            }
        } else {
            Ok(0.0)
        }
    }
}
```

- [ ] **Step 3: Implement Serializable**

Add to the same file:

```rust
use crate::serial::{self, Node, Serializable};

impl Serializable for WinMap {
    fn class_name(&self) -> &'static str { "WinMap" }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new("WinMap");
        node.class_comment = Some("Map one window on to another".to_string());
        serial::write_mapping_base(&mut node, self);
        for (i, &s) in self.shifts.iter().enumerate() {
            if s != 0.0 {
                node.push_double(
                    &format!("Sft{}", i + 1), s,
                    Some(format!("Shift for axis {}", i + 1)),
                );
            }
        }
        for (i, &s) in self.scales.iter().enumerate() {
            if s != 1.0 {
                node.push_double(
                    &format!("Scl{}", i + 1), s,
                    Some(format!("Scale factor for axis {}", i + 1)),
                );
            }
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (nin, _nout, inverted) = serial::read_mapping_base(node);
        let mut shifts = vec![0.0; nin];
        let mut scales = vec![1.0; nin];
        for i in 0..nin {
            shifts[i] = node.get_double(&format!("Sft{}", i + 1), 0.0);
            scales[i] = node.get_double(&format!("Scl{}", i + 1), 1.0);
        }
        let mut m = WinMap { shifts, scales, inverted: false };
        if inverted { m.invert(); }
        Ok(m)
    }
}
```

- [ ] **Step 4: Add to mappings/mod.rs**

Add to `crates/starlink-ast/src/mappings/mod.rs`:

```rust
mod winmap;
pub use winmap::WinMap;
```

- [ ] **Step 5: Update serial dispatch**

In `crates/starlink-ast/src/serial/mod.rs`, add WinMap to:
- `use` imports
- `mapping_from_node`: `"WinMap" => Ok(Box::new(WinMap::from_node(node)?))`
- `MappingToNode for dyn Mapping`: downcast branch for WinMap
- `any_object_from_node`: `"WinMap" => Ok(AnyObject::WinMap(WinMap::from_node(node)?))`
- `AnyObject` enum: add `WinMap(WinMap)` variant
- `to_native_string`: add match arm
- `as_mapping`: add match arm

- [ ] **Step 6: Write tests**

Create `crates/starlink-ast/tests/test_winmap.rs` with tests for:
- Forward transform with known shifts/scales
- Inverse transform
- `from_corners` constructor
- Zero scale → BAD on inverse
- BAD_VALUE propagation
- Rate computation
- Serialization round-trip via `to_native_string`/`from_native_string`

- [ ] **Step 7: Verify and commit**

Run: `cargo test -p starlink-ast`
Expected: all tests pass

```bash
git add crates/starlink-ast/src/mappings/ crates/starlink-ast/src/serial/mod.rs crates/starlink-ast/tests/test_winmap.rs
git commit -m "Add WinMap (per-axis linear mapping)"
```

---

### Task 3: LutMap

**Files:**
- Create: `crates/starlink-ast/src/mappings/lutmap.rs`
- Modify: `crates/starlink-ast/src/mappings/mod.rs`
- Create: `crates/starlink-ast/tests/test_lutmap.rs`

LutMap is a 1D lookup table mapping with linear or nearest-neighbor
interpolation. Always nin = nout = 1.

- [ ] **Step 1: Create lutmap.rs with struct, LutInterp, and constructor**

Create `crates/starlink-ast/src/mappings/lutmap.rs`:

The `LutMap` struct holds:
- `lut: Vec<f64>` — table values
- `start: f64` — input coordinate at first entry
- `inc: f64` — input spacing between entries
- `interp: LutInterp` — interpolation method
- `monotonic: i8` — +1 increasing, -1 decreasing, 0 non-monotonic (computed at construction)
- `inverted: bool`

The `LutInterp` enum: `Linear` (default), `Nearest`.

Constructor: `LutMap::new(lut: &[f64], start: f64, inc: f64) -> AstResult<Self>`
- Validates lut non-empty, inc non-zero
- Computes monotonicity by scanning the table (ignoring BAD values)

- [ ] **Step 2: Implement Mapping trait**

Forward transform:
1. Compute fractional index: `idx = (x - start) / inc`
2. Linear: interpolate between `lut[floor(idx)]` and `lut[ceil(idx)]`; extrapolate at boundaries using slope of end entries
3. Nearest: `lut[round(idx)]`, BAD if outside `[0, nlut-1]`
4. BAD propagation: if input is BAD or bracketing entries are BAD, output is BAD

Inverse transform (only if monotonic != 0):
1. Binary search to find bracketing entries
2. Linear interpolation to find input value
3. Non-monotonic → output BAD

`nin()` and `nout()` both return 1.

- [ ] **Step 3: Implement Serializable**

Class `"LutMap"`. Attributes:
- `Nlut` (always), `Start` (if != 0), `Incr` (if != 1), `LutInt` (if not linear)
- `L1`..`LN` for non-BAD table values

`from_node`: read `Nlut`, reconstruct table from `L{i+1}` entries (missing entries default to BAD_VALUE).

- [ ] **Step 4: Add to mod.rs and serial dispatch**

Same pattern as WinMap — add module, re-export, update all dispatch points.

- [ ] **Step 5: Write tests**

Create `crates/starlink-ast/tests/test_lutmap.rs` (from `testlutmap.c`):
- Linear interpolation between entries
- Extrapolation at boundaries
- Nearest-neighbor mode
- BAD values in table
- Inverse (monotonic table) via binary search
- Non-monotonic → no inverse
- Flat sections in table
- Serialization round-trip

- [ ] **Step 6: Verify and commit**

Run: `cargo test -p starlink-ast`
Expected: all tests pass

```bash
git add crates/starlink-ast/src/mappings/ crates/starlink-ast/src/serial/mod.rs crates/starlink-ast/tests/test_lutmap.rs
git commit -m "Add LutMap (1D lookup table mapping)"
```

---

### Task 4: PolyMap

**Files:**
- Create: `crates/starlink-ast/src/mappings/polymap.rs`
- Modify: `crates/starlink-ast/src/mappings/mod.rs`
- Create: `crates/starlink-ast/tests/test_polymap.rs`

PolyMap evaluates N-D polynomial transformations with optional
Levenberg-Marquardt inverse fitting.

- [ ] **Step 1: Create polymap.rs with PolyTerm, PolyMap struct, and constructor**

Create `crates/starlink-ast/src/mappings/polymap.rs`:

```rust
/// A single polynomial term: coeff * x0^p0 * x1^p1 * ... * xN^pN.
#[derive(Debug, Clone, PartialEq)]
pub struct PolyTerm {
    /// Coefficient value.
    pub coeff: f64,
    /// Power of each input axis (length = nin).
    pub powers: Vec<usize>,
    /// 1-based output axis this term contributes to.
    pub out_axis: usize,
}
```

The `PolyMap` struct holds:
- `nin: usize`, `nout: usize`
- `forward: Vec<PolyTerm>` — forward polynomial terms
- `inverse: Option<Vec<PolyTerm>>` — explicit inverse terms
- `iter_inverse: bool` — use LM fitting for inverse
- `niter_inverse: usize` — max LM iterations (default 4)
- `tol_inverse: f64` — LM convergence tolerance (default 1e-6)
- `inverted: bool`

Constructor validates: nin > 0, nout > 0, all terms have correct
powers length, all out_axis in 1..=nout.

- [ ] **Step 2: Implement polynomial evaluation**

Private helper `eval_poly(terms: &[PolyTerm], nin: usize, nout: usize, input: &[f64]) -> Vec<f64>`:
- For each output axis, sum matching terms
- Each term: `coeff * product(input[i].powi(powers[i]) for i in 0..nin)`
- BAD propagation: any BAD input → all outputs BAD

- [ ] **Step 3: Implement Mapping trait**

Forward: call `eval_poly` with forward terms.
Inverse (explicit): call `eval_poly` with inverse terms (swapping nin/nout).
Inverse (iterative): implement LM fitting (Step 4).

`rate`: use `numeric_rate` (the existing default).

- [ ] **Step 4: Implement Levenberg-Marquardt iterative inverse**

When `iter_inverse == true` and no explicit inverse:

For each point to inverse-transform:
1. Define a `PolyResidual` struct implementing `levenberg_marquardt::LeastSquaresProblem`
2. Residual: `r(x) = forward(x) - target` (nout residuals)
3. Jacobian: `J[i][j] = d(forward_i)/d(x_j)` — computed analytically:
   for each term contributing to output i, `d/d(x_j) = coeff * powers[j] * x_j^(powers[j]-1) * product(x_k^powers[k] for k!=j)`
4. Initial guess: `x0 = target`
5. Solve with `LevenbergMarquardt::new().with_tol(tol).minimize(problem)`
6. Return solution if converged, BAD_VALUE otherwise

Uses `nalgebra` vectors/matrices (transitive dep from `levenberg-marquardt`).

- [ ] **Step 5: Implement Serializable**

Class `"PolyMap"`. The C format groups coefficients by output axis:
- `NCF1`..`NCFnout`: coefficient count per output
- `CF1`..`CFtotal`: coefficient values (sequentially, output-by-output)
- `PF{k}_{axis}`: power of input axis for coefficient k (only if > 0)
- Then inverse equivalents: `NCI`, `CI`, `PI`
- `IterInv`, `NiterInv`, `TolInv` if set

`to_node`: group forward terms by out_axis, write NCF counts, then
coefficients and non-zero powers in C order.

`from_node`: read NCF counts, reconstruct terms from CF and PF values.

- [ ] **Step 6: Add to mod.rs and serial dispatch**

Same pattern — add module, re-export `PolyMap` and `PolyTerm`,
update all dispatch points.

- [ ] **Step 7: Write tests**

Create `crates/starlink-ast/tests/test_polymap.rs` (from `testpolymap.c`):
- Simple 1D polynomial (y = 2x^2 + 3x + 1)
- 2D → 2D polynomial
- Explicit inverse and verify round-trip accuracy
- Iterative (LM) inverse and verify convergence
- BAD_VALUE propagation
- Zero-coefficient terms ignored
- Serialization round-trip

- [ ] **Step 8: Verify and commit**

Run: `cargo test -p starlink-ast`
Expected: all tests pass

```bash
git add Cargo.lock crates/starlink-ast/src/mappings/ crates/starlink-ast/src/serial/mod.rs crates/starlink-ast/tests/test_polymap.rs
git commit -m "Add PolyMap (N-D polynomial mapping with LM inverse)"
```

---

### Task 5: ChebyMap

**Files:**
- Create: `crates/starlink-ast/src/mappings/chebymap.rs`
- Modify: `crates/starlink-ast/src/mappings/mod.rs`
- Create: `crates/starlink-ast/tests/test_chebymap.rs`

ChebyMap wraps PolyMap with Chebyshev polynomial basis and domain
normalization to [-1, 1].

- [ ] **Step 1: Create chebymap.rs with struct and constructor**

```rust
pub struct ChebyMap {
    poly: PolyMap,
    lbnd_f: Vec<f64>,
    ubnd_f: Vec<f64>,
    scale_f: Vec<f64>,
    offset_f: Vec<f64>,
    lbnd_i: Option<Vec<f64>>,
    ubnd_i: Option<Vec<f64>>,
    scale_i: Option<Vec<f64>>,
    offset_i: Option<Vec<f64>>,
    inverted: bool,
}
```

Constructor: `ChebyMap::new(nin, nout, forward: &[PolyTerm],
inverse: Option<&[PolyTerm]>, lbnd_f: &[f64], ubnd_f: &[f64],
lbnd_i: Option<&[f64]>, ubnd_i: Option<&[f64]>) -> AstResult<Self>`

Computes normalization per axis:
- `scale[i] = 2.0 / (ubnd[i] - lbnd[i])`
- `offset[i] = -(ubnd[i] + lbnd[i]) / (ubnd[i] - lbnd[i])`

- [ ] **Step 2: Implement Chebyshev evaluation helper**

Private function `eval_chebyshev(n: usize, x: f64) -> f64`:
- `T_0(x) = 1`, `T_1(x) = x`
- `T_{k+1}(x) = 2*x*T_k(x) - T_{k-1}(x)`

Private function `eval_cheby_poly(terms, nin, nout, input, scale, offset) -> Vec<f64>`:
- Check bounds: if any input outside [-1, 1] after normalization → BAD
- For each term: `coeff * product(T_{powers[i]}(normalized[i]))`

- [ ] **Step 3: Implement Mapping trait**

Forward: normalize inputs, eval using Chebyshev basis, check bounds.
Inverse: if inverse terms exist, normalize using inverse scale/offset, eval.

Delegate `nin`, `nout`, `invert`, `simplified`, `rate` to the inner
PolyMap where appropriate, adjusting for the normalization.

- [ ] **Step 4: Implement Serializable**

Class `"ChebyMap"`. Write ChebyMap-specific attributes first, then
delegate to PolyMap's coefficient format:
- `FSCL1`..`FSCLnin`, `FOFF1`..`FOFFnin` (forward scale/offset)
- `ISCL1`..`ISCLnout`, `IOFF1`..`IOFFnout` (inverse, if exists)
- Then all PolyMap coefficient attributes (NCF, CF, PF, etc.)

`from_node`: read scale/offset arrays, then delegate coefficient
reading to PolyMap's logic, construct ChebyMap.

- [ ] **Step 5: Add to mod.rs and serial dispatch**

Add module, re-export `ChebyMap`, update all dispatch points.

- [ ] **Step 6: Write tests**

Create `crates/starlink-ast/tests/test_chebymap.rs` (from `testchebymap.c`):
- 1D constant function (T_0 = 1)
- 1D quadratic in Chebyshev basis
- 2D with mixed powers
- Out-of-bounds input → BAD
- Forward/inverse round-trip accuracy
- Serialization round-trip

- [ ] **Step 7: Verify and commit**

Run: `cargo test -p starlink-ast`
Expected: all tests pass

```bash
git add crates/starlink-ast/src/mappings/ crates/starlink-ast/src/serial/mod.rs crates/starlink-ast/tests/test_chebymap.rs
git commit -m "Add ChebyMap (Chebyshev polynomial mapping)"
```

---

### Task 6: Clippy, Fmt, Final Verification

**Files:**
- Possibly any file needing fixes

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Fix any issues.

- [ ] **Step 2: Run fmt**

Run: `cargo fmt --all -- --check`
If issues, run `cargo fmt --all`.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p starlink-ast`
Expected: all tests pass

- [ ] **Step 4: Verify the dispatch is complete**

Verify that all 4 new types appear in:
- `AnyObject` enum
- `MappingToNode for dyn Mapping`
- `mapping_from_node`
- `any_object_from_node`
- `to_native_string`
- `as_mapping`

- [ ] **Step 5: Commit fixes if any**

```bash
git add -u
git commit -m "Fix clippy and fmt issues in Step 6 mappings"
```

---

## Verification Checklist

After all tasks are complete:

1. `cargo build --workspace` — compiles
2. `cargo test -p starlink-ast` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. WinMap: forward/inverse transforms correct, from_corners works
6. LutMap: interpolation correct, inverse via binary search works
7. PolyMap: polynomial evaluation correct, LM inverse converges
8. ChebyMap: Chebyshev basis evaluation correct, domain bounds enforced
9. All 4 types serialize and deserialize correctly
10. All 4 types in AnyObject dispatch
11. `git status` — clean

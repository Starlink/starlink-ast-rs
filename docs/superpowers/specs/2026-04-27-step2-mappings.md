# Step 2: Simple Mappings — Design Spec

## Summary

Implement the six simplest Mapping classes: UnitMap, ZoomMap, ShiftMap,
MatrixMap, PermMap, and CmpMap. Each implements the `Mapping` trait with
full transform, inverse, simplification, and rate computation. Composition
via `then()`/`alongside()` is provided on all types.

## Deliverables

1. `UnitMap` — identity transformation
2. `ZoomMap` — uniform scale
3. `ShiftMap` — constant offset
4. `MatrixMap` — linear matrix transformation (ndarray-backed)
5. `PermMap` — axis permutation/selection/duplication
6. `CmpMap` — compound (series or parallel) mapping
7. `Composition` enum (Series, Parallel)
8. `then()`/`alongside()` on all concrete types and `Box<dyn Mapping>`
9. Generic numeric `rate()` implementation (central finite differences)
10. Basic simplification rules for trivial cases
11. BAD_VALUE handling in all transforms

## Module Structure

```
crates/starlink-ast/src/mappings/
├── mod.rs           # trait + DynMapping + re-exports + Box<dyn Mapping> methods
├── unit.rs          # UnitMap
├── zoom.rs          # ZoomMap
├── shift.rs         # ShiftMap
├── matrix.rs        # MatrixMap
├── perm.rs          # PermMap
├── cmpmap.rs        # CmpMap + Composition enum
└── rate.rs          # numeric_rate() shared helper
```

## BAD_VALUE Contract

Every `transform()` implementation must:
- Check each input point: if ANY coordinate is `BAD_VALUE` or `f64::NAN`,
  set ALL output coordinates for that point to `BAD_VALUE`.
- Never produce `f64::NAN` in output — always use `BAD_VALUE`.

A shared helper can encapsulate this:
```rust
fn is_bad(v: f64) -> bool {
    v == BAD_VALUE || v.is_nan()
}
```

## UnitMap (src/mappings/unit.rs)

```rust
pub struct UnitMap {
    ncoord: usize,
    inverted: bool,
}

impl UnitMap {
    pub fn new(ncoord: usize) -> Self;
}
```

- `nin()` = `nout()` = `ncoord`
- `transform()`: copy input to output unchanged (BAD_VALUE pass-through is correct)
- `simplified()`: returns clone of self
- Inversion is a no-op (identity is its own inverse)

## ZoomMap (src/mappings/zoom.rs)

```rust
pub struct ZoomMap {
    ncoord: usize,
    factor: f64,
    inverted: bool,
}

impl ZoomMap {
    pub fn new(ncoord: usize, factor: f64) -> AstResult<Self>;
}
```

- `nin()` = `nout()` = `ncoord`
- `transform(forward=true)`: multiply each coordinate by `factor`
- `transform(forward=false)`: divide each coordinate by `factor`
- Construction fails if `factor == 0.0` (non-invertible)
- `simplified()`: returns `UnitMap` if `factor == 1.0`, else self
- Handles `is_inverted`: swaps forward/inverse sense

## ShiftMap (src/mappings/shift.rs)

```rust
pub struct ShiftMap {
    shift: Vec<f64>,
    inverted: bool,
}

impl ShiftMap {
    pub fn new(shift: impl Into<Vec<f64>>) -> Self;
}
```

- `nin()` = `nout()` = `shift.len()`
- `transform(forward=true)`: add `shift[i]` to coordinate `i`
- `transform(forward=false)`: subtract `shift[i]` from coordinate `i`
- `simplified()`: returns `UnitMap` if all shifts are zero

## MatrixMap (src/mappings/matrix.rs)

```rust
pub struct MatrixMap {
    matrix: Array2<f64>,
    inverse: Option<Array2<f64>>,
    inverted: bool,
}

impl MatrixMap {
    pub fn new(matrix: Array2<f64>) -> AstResult<Self>;
}
```

- `nin()` = matrix columns, `nout()` = matrix rows
- `transform(forward=true)`: `output_point = matrix × input_point`
- `transform(forward=false)`: `output_point = inverse × input_point` (fails if not invertible)
- Constructor computes the inverse if the matrix is square and invertible; for non-square matrices, inverse is None (inverse transform returns error)
- `simplified()`: returns `UnitMap` if matrix is identity
- Matrix-vector multiply via ndarray's `dot()`

## PermMap (src/mappings/perm.rs)

```rust
pub struct PermMap {
    inperm: Vec<i64>,
    outperm: Vec<i64>,
    constants: Vec<f64>,
    inverted: bool,
}

impl PermMap {
    pub fn new(inperm: &[i64], outperm: &[i64], constants: &[f64]) -> AstResult<Self>;
}
```

- `nin()` = `inperm.len()`, `nout()` = `outperm.len()`
- Permutation encoding (matching C AST):
  - Positive value `j`: output axis gets input axis `j` (1-based)
  - Negative value `-j`: output axis gets `constants[j-1]`
  - Zero: output axis gets `BAD_VALUE`
- `transform(forward=true)` uses `outperm`
- `transform(forward=false)` uses `inperm`
- `simplified()`: returns `UnitMap` if it's the identity permutation

## CmpMap (src/mappings/cmpmap.rs)

```rust
pub enum Composition {
    Series,
    Parallel,
}

pub struct CmpMap {
    first: DynMapping,
    second: DynMapping,
    kind: Composition,
    inverted: bool,
}

impl CmpMap {
    pub fn new(first: DynMapping, second: DynMapping, kind: Composition) -> AstResult<Self>;
}
```

- **Series**: `nin()` = `first.nin()`, `nout()` = `second.nout()`
  - Constraint: `first.nout() == second.nin()`
  - Forward: apply first, then second
  - Inverse: apply second inverse, then first inverse
- **Parallel**: `nin()` = `first.nin() + second.nin()`, `nout()` = `first.nout() + second.nout()`
  - Forward: split input, transform each half, concatenate outputs
  - Inverse: same structure, each component inverted
- `simplified()`:
  - Simplify components
  - If both simplify to UnitMap, return UnitMap
  - Otherwise return new CmpMap with simplified components

## Composition Methods (then/alongside)

On `Box<dyn Mapping>`:
```rust
impl dyn Mapping {
    pub fn then(self: Box<Self>, next: DynMapping) -> AstResult<CmpMap>;
    pub fn alongside(self: Box<Self>, other: DynMapping) -> AstResult<CmpMap>;
}
```

On each concrete type (example for ZoomMap):
```rust
impl ZoomMap {
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap>;
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap>;
}
```

These are thin wrappers around `CmpMap::new()`.

## Numeric Rate (src/mappings/rate.rs)

```rust
pub fn numeric_rate(
    mapping: &dyn Mapping,
    at: &[f64],
    ax1: usize,  // 1-based output axis
    ax2: usize,  // 1-based input axis
) -> AstResult<f64>
```

Uses central finite differences:
- Step size: `h = max(abs(at[ax2-1]) * 1e-6, 1e-10)`
- Rate = `(f(at + h*e_ax2)[ax1-1] - f(at - h*e_ax2)[ax1-1]) / (2*h)`
- If either evaluation produces `BAD_VALUE`, returns `BAD_VALUE`

All concrete mappings delegate `rate()` to this shared function.

## Testing Strategy

For each mapping:
- Construction (valid and invalid parameters)
- Forward transform with known input/output
- Inverse transform
- BAD_VALUE propagation
- `is_inverted` / `invert()` interaction with transforms
- `simplified()` returning correct type for trivial cases

For CmpMap:
- Series composition correctness
- Parallel composition correctness
- Dimension mismatch errors

For composition methods:
- `then()` and `alongside()` produce correct CmpMap

## What This Step Does NOT Include

- Full simplification engine (cancelling inverse pairs, merging sequences)
- Serialization (to_node/from_node)
- Decompose, map_box, map_split, linear_approx, quad_approx methods
- Any other Mapping classes (WcsMap, PolyMap, etc.)

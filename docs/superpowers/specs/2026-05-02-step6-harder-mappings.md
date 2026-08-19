# Step 6: Harder Mappings — Design Spec

## Summary

Implement four mapping types: WinMap (per-axis linear), LutMap (1D
lookup table), PolyMap (N-D polynomial with Levenberg-Marquardt
inverse fitting), and ChebyMap (Chebyshev polynomial with domain
normalization). Add Serializable impls, dispatch updates, AnyObject
variants, and tests for all four.

MathMap (expression evaluator) is deferred to a later step.

## Deliverables

1. `WinMap` struct with per-axis shift+scale, exact inverse,
   from-corners constructor.
2. `LutMap` struct with 1D table, linear/nearest interpolation,
   binary-search inverse for monotonic tables.
3. `PolyMap` struct with N-D polynomial evaluation, explicit inverse,
   and iterative (Levenberg-Marquardt) inverse fitting. New
   dependency: `levenberg-marquardt` crate.
4. `ChebyMap` struct wrapping PolyMap with Chebyshev basis and
   `[-1, 1]` domain normalization.
5. `Serializable` impls for all four types.
6. Dispatch updates: `MappingToNode`, `mapping_from_node`,
   `any_object_from_node`, `AnyObject` enum.
7. Tests equivalent to C's `testpolymap.c`, `testlutmap.c`,
   `testchebymap.c`, plus WinMap tests.

## Module Structure

```
crates/ast-core/src/mappings/
├── winmap.rs       # WinMap
├── lutmap.rs       # LutMap + LutInterp enum
├── polymap.rs      # PolyMap + PolyTerm
├── chebymap.rs     # ChebyMap (wraps PolyMap)
├── mod.rs          # updated re-exports
```

## WinMap

Per-axis independent linear transformation:
`output[i] = shift[i] + scale[i] * input[i]`.

```rust
pub struct WinMap {
    shifts: Vec<f64>,
    scales: Vec<f64>,
    inverted: bool,
}
```

### Constructors

- `WinMap::new(shifts: &[f64], scales: &[f64]) -> AstResult<Self>` —
  validates both slices have equal length > 0.
- `WinMap::from_corners(corner1_in: &[f64], corner2_in: &[f64],
  corner1_out: &[f64], corner2_out: &[f64]) -> AstResult<Self>` —
  computes shifts/scales to map one window to another. All slices
  must have equal length.

### Transform

Forward: `out[i] = shifts[i] + scales[i] * in[i]`

Inverse: `in[i] = (out[i] - shifts[i]) / scales[i]`. Returns
BAD_VALUE for axes where `scales[i] == 0.0`.

BAD_VALUE propagation: if `in[i] == BAD_VALUE`, `out[i] = BAD_VALUE`.

### Serialisation

Class: `"WinMap"`.

| Key | Condition | Comment |
|-----|-----------|---------|
| `Sft1`..`SftN` | if != 0.0 | "Shift for axis N" |
| `Scl1`..`SclN` | if != 1.0 | "Scale factor for axis N" |

## LutMap

1-dimensional coordinate transformation via lookup table
interpolation. Always `nin = nout = 1`.

```rust
pub struct LutMap {
    lut: Vec<f64>,
    start: f64,
    inc: f64,
    interp: LutInterp,
    inverted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LutInterp {
    /// Linear interpolation (default). Extrapolates at boundaries.
    Linear,
    /// Nearest-neighbour. Returns BAD_VALUE outside bounds.
    Nearest,
}
```

### Constructor

`LutMap::new(lut: &[f64], start: f64, inc: f64) -> AstResult<Self>` —
validates lut non-empty, inc non-zero.

### Transform (Forward)

1. Compute fractional table index: `idx = (x - start) / inc`.
2. Linear: interpolate between `lut[floor(idx)]` and
   `lut[ceil(idx)]`. Extrapolate beyond table boundaries using
   the slope at the boundary.
3. Nearest: return `lut[round(idx)]`, or BAD_VALUE if outside
   `[0, nlut-1]`.
4. BAD_VALUE in table entries propagates (if either bracketing
   entry is BAD, output is BAD).

### Transform (Inverse)

Only available if the table is monotonic (strictly increasing or
decreasing, ignoring BAD entries and flat sections). Detected at
construction time.

Uses binary search on the table to find the bracketing entries,
then linear interpolation to find the input value.

If the table is not monotonic, inverse transform returns BAD_VALUE
for all inputs.

### Serialisation

Class: `"LutMap"`.

| Key | Condition | Comment |
|-----|-----------|---------|
| `Nlut` | always | "Number of lookup table elements" |
| `Start` | if != 0.0 | "Input value at first element" |
| `Incr` | if != 1.0 | "Input value increment between elements" |
| `LutInt` | if not Linear | "Interpolation method" |
| `L1`..`LN` | non-BAD entries | "Lookup table elements..." |

## PolyMap

N-dimensional polynomial transformation where each output coordinate
is a polynomial function of all input coordinates.

```rust
/// A single polynomial term.
#[derive(Debug, Clone, PartialEq)]
pub struct PolyTerm {
    /// Coefficient multiplier.
    pub coeff: f64,
    /// Power of each input axis (length = nin).
    pub powers: Vec<usize>,
    /// 1-based output axis this term contributes to.
    pub out_axis: usize,
}

pub struct PolyMap {
    nin: usize,
    nout: usize,
    forward: Vec<PolyTerm>,
    inverse: Option<Vec<PolyTerm>>,
    iter_inverse: bool,
    niter_inverse: usize,
    tol_inverse: f64,
    inverted: bool,
}
```

### Constructor

`PolyMap::new(nin: usize, nout: usize, forward: &[PolyTerm],
inverse: Option<&[PolyTerm]>) -> AstResult<Self>` — validates:
- nin > 0, nout > 0
- All forward terms have `powers.len() == nin`
- All forward terms have `out_axis` in 1..=nout
- If inverse provided, same validation against nout/nin (swapped)

### Transform (Forward)

For each output axis `j` (1-based):
```
out[j] = sum over terms where out_axis == j:
    term.coeff * product(in[i]^term.powers[i] for i in 0..nin)
```

BAD_VALUE propagation: if any input is BAD, all outputs are BAD.

### Transform (Inverse — Explicit)

Same polynomial evaluation using inverse terms, with nin/nout
swapped.

### Transform (Inverse — Iterative via Levenberg-Marquardt)

When `iter_inverse == true` and no explicit inverse exists:

For each target output point `y`:
1. Initial guess: `x0 = y` (or linear approximation if available).
2. Define residual function: `r(x) = forward(x) - y`.
3. Compute Jacobian analytically: `J[i][j] = d(forward_i)/d(x_j)`.
   For polynomial terms, the derivative with respect to `x_j` is
   `coeff * powers[j] * x_j^(powers[j]-1) * product(x_k^powers[k]
   for k != j)`.
4. Use `levenberg-marquardt` crate's solver to minimize `||r(x)||^2`.
5. Converge when relative change < `tol_inverse` or after
   `niter_inverse` iterations.

The `levenberg-marquardt` crate requires implementing the
`LeastSquaresProblem` trait. The PolyMap module provides a private
struct implementing this trait.

Default parameters: `niter_inverse = 4`, `tol_inverse = 1e-6`.

### Serialisation

Class: `"PolyMap"`.

The C library's serialisation format stores coefficients in a
specific grouped structure:

| Key | Description |
|-----|-------------|
| `NCF1`..`NCFnout` | Number of forward coefficients per output |
| `CF1`..`CFtotal` | Forward coefficient values |
| `PF1_1`..`PFtotal_nin` | Powers for each forward coefficient |
| `NCI1`..`NCInin` | Number of inverse coefficients per output |
| `CI1`..`CItotal` | Inverse coefficient values |
| `PI1_1`..`PItotal_nout` | Powers for each inverse coefficient |
| `IterInv` | if iterative inverse enabled |
| `NiterInv` | if != default |
| `TolInv` | if != default |

## ChebyMap

Wraps a PolyMap with Chebyshev polynomial basis and domain
normalization. Points outside the domain return BAD_VALUE.

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

### Constructor

`ChebyMap::new(nin, nout, forward: &[PolyTerm],
inverse: Option<&[PolyTerm]>, lbnd_f: &[f64], ubnd_f: &[f64],
lbnd_i: Option<&[f64]>, ubnd_i: Option<&[f64]>) -> AstResult<Self>`

Computes normalization:
- `scale[i] = 2.0 / (ubnd[i] - lbnd[i])`
- `offset[i] = -(ubnd[i] + lbnd[i]) / (ubnd[i] - lbnd[i])`

### Transform

1. Check inputs are within `[lbnd, ubnd]`. Out-of-bounds → BAD_VALUE.
2. Normalize: `x_norm[i] = input[i] * scale[i] + offset[i]`
   (maps to [-1, 1]).
3. Evaluate polynomial using Chebyshev basis: a term with
   `powers = [p1, p2, ...]` evaluates as
   `coeff * T_p1(x_norm[0]) * T_p2(x_norm[1]) * ...`
   where `T_n` is the Chebyshev polynomial of the first kind.
4. Chebyshev recurrence: `T_0(x) = 1`, `T_1(x) = x`,
   `T_{n+1}(x) = 2*x*T_n(x) - T_{n-1}(x)`.

### Serialisation

Class: `"ChebyMap"`. Inherits PolyMap's coefficient attributes plus:

| Key | Condition | Comment |
|-----|-----------|---------|
| `LBF1`..`LBFnin` | always | "Lower bound for forward input axis N" |
| `UBF1`..`UBFnin` | always | "Upper bound for forward input axis N" |
| `LBI1`..`LBInout` | if inverse exists | "Lower bound for inverse input axis N" |
| `UBI1`..`UBInout` | if inverse exists | "Upper bound for inverse input axis N" |

## Dependencies

New workspace dependency: `levenberg-marquardt` crate for PolyMap's
iterative inverse fitting.

```toml
# In workspace Cargo.toml [workspace.dependencies]
levenberg-marquardt = "0.14"

# In ast-core Cargo.toml [dependencies]
levenberg-marquardt = { workspace = true }
```

## Dispatch Updates

All four types need to be added to:
- `AnyObject` enum (4 new variants)
- `MappingToNode` dispatch (4 new downcast branches)
- `mapping_from_node` dispatch (4 new class-name matches)
- `any_object_from_node` dispatch (4 new class-name matches)
- `to_native_string` match (4 new arms)
- `AnyObject::as_mapping` (4 new arms)

## Testing Strategy

### WinMap Tests
- Forward/inverse with known shifts+scales
- `from_corners` constructor
- Zero-scale axis → BAD on inverse
- BAD_VALUE propagation
- Serialisation round-trip

### LutMap Tests (from testlutmap.c)
- Linear interpolation between entries
- Extrapolation at boundaries
- Nearest-neighbor mode
- BAD_VALUE in table
- Inverse via binary search (monotonic)
- Non-monotonic → no inverse
- Serialisation round-trip

### PolyMap Tests (from testpolymap.c)
- Simple 1D polynomial (y = 2x^2 + 3x + 1)
- 2D → 2D polynomial
- Explicit inverse
- Iterative (LM) inverse
- BAD_VALUE propagation
- Serialisation round-trip

### ChebyMap Tests (from testchebymap.c)
- Chebyshev basis matches known T_n values
- Domain normalization
- Out-of-bounds → BAD
- Forward/inverse with Chebyshev coefficients
- Serialisation round-trip

## What This Step Does NOT Include

- MathMap (expression parser + bytecode VM) — deferred.
- WcsMap (depends on wcslib port, Step 6a).
- Any changes to Frame types.

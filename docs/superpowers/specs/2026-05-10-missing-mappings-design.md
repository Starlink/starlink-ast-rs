# Missing Mappings Implementation Design

## Scope

Port 9 remaining Mapping classes from the C AST library to Rust.
Each mapping is delivered as a complete PR containing:

- Full transform (forward + inverse where defined by C)
- Serialization (`to_node` / `from_node` for Native format)
- `equal()` with full C-parity edge cases (inverted states, tolerance)
- `try_merge()` simplification rules matching C behavior exactly
- `simplified()` self-simplification
- `map_split()` where applicable
- Tier-1 Rust unit tests
- Tier-2 simplify regression fixtures enabled in `simplify_tests.txt`
- Tier-4 oracle tests (bit-exact with C via `ast-oracle-tests`)

**Excluded from this plan:**
- IntraMap (user-function wrapper; not used in FITS-WCS data)
- SelectorMap, SwitchMap (depend on Region infrastructure not yet built)

## Delivery Order

Strict dependency ordering. Each phase must complete before the next begins.

### Phase 1: Simple Standalone Mappings

These have no dependencies on other unimplemented types.

#### 1. PcdMap

2D pincushion/barrel distortion correction.

**Struct fields:**
- `disc: f64` — distortion coefficient
- `pcdcen: [f64; 2]` — distortion centre

**Transform:**
- Forward: applies radial distortion `r' = r * (1 + disc * r^2)` relative to centre
- Inverse: iterative Newton-Raphson to invert the cubic

**Simplify rules (from C `MapMerge`):**
- `disc == 0` → simplify to UnitMap
- PcdMap + inverse PcdMap → cancel (same disc and centre)
- PcdMap + PermMap → swap positions (PcdMap can commute past axis swaps)
- PcdMap + ZoomMap series → absorb zoom into PcdMap (adjust disc by zoom^2, shift centre)

**equal():** Compare disc within tolerance; compare centres within tolerance.
Handle inverted state (inverted PcdMap with disc `d` equals non-inverted with modified parameters).

**Serialization tokens:** `Disco`, `PcdCen(1)`, `PcdCen(2)`

---

#### 2. GrismMap

1D spectral dispersion via grating/prism/grism equation (FITS-WCS Paper III).

**Struct fields (8 attributes):**
- `grism_nr: f64` — refractive index at reference wavelength
- `grism_nrp: f64` — rate of change of refractive index with wavelength
- `grism_wave_r: f64` — reference wavelength
- `grism_alpha: f64` — angle of incidence of light on grating surface (radians)
- `grism_g: f64` — grating ruling spacing
- `grism_m: i32` — diffraction order
- `grism_eps: f64` — angle between normal to detector and grating surface (radians)
- `grism_theta: f64` — angle of incidence of light on detector (radians)

**Transform:**
- Forward (wavelength → detector position): closed-form dispersion equation
- Inverse (detector position → wavelength): Newton-Raphson iteration

**Simplify rules:**
- GrismMap + inverse GrismMap → cancel (all attributes equal)
- GrismMap(fwd) + ZoomMap → merge (absorb scale into GrismMap parameters)
- ZoomMap + GrismMap(inv) → merge

**equal():** Compare all 8 attributes within tolerance. Handle inverted state:
an inverted GrismMap is NOT simply a non-inverted one with different params,
so `equal()` must account for inversion flags explicitly.

**Serialization tokens:** `GrsmNR`, `GrsmNRP`, `GrsmWR`, `GrsmA`, `GrsmG`,
`GrsmM`, `GrsmEps`, `GrsmTh`

---

#### 3. UnitNormMap

Decomposes vectors into unit vector + norm (and inverse).

**Struct fields:**
- `centre: Vec<f64>` — centre point (length = nin)

**Transform:**
- Forward (nin→nin+1): subtract centre, compute norm, output unit vector components then norm
- Inverse (nin+1→nin): multiply unit vector by norm (last input), add centre

**Simplify rules:**
- Inverse-cancel (UnitNormMap + inverse UnitNormMap with same centre)
- UnitNormMap(fwd) + ShiftMap → merge (absorb shift into centre)
- UnitNormMap(inv) + ShiftMap → merge
- UnitNormMap(fwd) + WinMap → merge
- UnitNormMap(inv) + WinMap → merge
- Difference in centres → simplify to ShiftMap (when centres differ by a constant)

**equal():** Compare centres within tolerance. Handle nin/nout asymmetry.

**Serialization tokens:** `Centre(i)` for each axis

---

### Phase 2: Wrapper Mappings

These store `DynMapping` values internally (the trait already exists).

#### 4. TranMap

Combines forward transform of one Mapping with inverse of another.

**Struct fields:**
- `map1: DynMapping` — supplies the forward transformation
- `map2: DynMapping` — supplies the inverse transformation

**Constraints:** `map1.nout() == map2.nout()` (both define the same output space).
`nin = map1.nin()`, `nout = map1.nout()`.

**Transform:**
- Forward: apply `map1` forward
- Inverse: apply `map2` inverse

**has_forward / has_inverse:** determined by whether the stored mappings
have the required direction.

**Simplify rules:**
- If both components are equal → simplify to that single mapping
- Adjacent TranMap merge: TranMap(A,B) + TranMap(C,D) in series → TranMap(A·C, D·B)
  (compose the forward path and inverse path separately)
- Invert-normalize: inverted TranMap(A,B) = TranMap(B,A) with inversion cleared
- Component simplification: simplify each stored mapping independently

**equal():** Both stored mappings must be `equal()` (recursively). Handle
inverted state (inverted TranMap swaps the role of map1/map2).

**Serialization:** Two child nodes (the two stored mappings), inheriting
standard Mapping `IsA` chain.

---

#### 5. RateMap

Computes a single element of the Jacobian of an encapsulated Mapping.

**Struct fields:**
- `map: DynMapping` — the encapsulated mapping
- `ax1: usize` — 1-based output axis index
- `ax2: usize` — 1-based input axis index

**Transform:**
- Forward: for each input point, compute `map.rate(point, ax1, ax2)`
- Inverse: not defined (unless inverted, in which case forward is undefined
  and inverse becomes the rate computation)

**Nin/Nout:** `nin = map.nin()`, `nout = 1`

**Simplify rules:**
- Inverse-cancel (two equal RateMaps in series with opposite inversion)
- Interior simplification: simplify the encapsulated mapping

**equal():** Encapsulated mappings are equal AND same ax1, ax2 values.

**Serialization tokens:** `RatAx1`, `RatAx2`, plus child node for the
encapsulated mapping.

---

### Phase 3: Larger Standalone Mappings

#### 6. SplineMap

2D→2D mapping using a pair of bivariate B-splines.

**Struct fields:**
- `ncoeff: usize` — number of spline coefficients
- `nknot: [usize; 2]` — number of knots in each axis
- `knots: [Vec<f64>; 2]` — knot vectors (length nknot[i])
- `coeff: [Vec<f64>; 2]` — coefficient arrays (one per output axis)
- `order: usize` — spline order (typically 4 = cubic)
- `out_unit: Option<f64>` — value to use outside defined region (None = BAD)

**Transform:**
- Forward: evaluate both B-splines at the input (x, y) position
- Inverse: iterative (Newton-Raphson on the 2D system)

**Simplify rules:**
- Inverse-cancel only (two equal SplineMaps with opposite inversion)

**equal():** All knots, coefficients, and order must match within tolerance.

**Serialization tokens:** `SpOrd`, `NKnot1`, `NKnot2`, `Knot1(i)`, `Knot2(i)`,
`CoA(i)`, `CoB(i)`, `OutUnit`

---

#### 7. DssMap

2D pixel ↔ equatorial sky via Digitised Sky Survey plate solution.

**Struct fields (plate constants):**
- `plate_ra: f64` — plate centre RA (radians)
- `plate_dec: f64` — plate centre Dec (radians)
- `plate_scale: f64` — plate scale (arcsec/mm)
- `x_pixel_size: f64`, `y_pixel_size: f64` — pixel sizes (microns)
- `ppo: [f64; 6]` — orientation coefficients (PPO1–PPO6)
- `amd_x: [f64; 20]` — plate solution X coefficients (AMDX1–AMDX20)
- `amd_y: [f64; 20]` — plate solution Y coefficients (AMDY1–AMDY20)
- `x_coeff: f64`, `y_coeff: f64` — derived pixel offset
- Plus additional derived quantities cached at construction

**Transform:**
- Forward (pixel → RA, Dec): the `platepos` algorithm
- Inverse (RA, Dec → pixel): the `platepix` algorithm (iterative)

**Construction:** DssMap has no public constructor in C — it is created by
reading a FitsChan with DSS encoding. The Rust port follows the same pattern:
`DssMap::from_fits_keywords(keywords)`.

**Simplify rules:** Basic inverse-cancel only. The C library also has a
DssMap + WinMap merge (absorb pixel offset into WinMap) which should be ported.

**equal():** Compare all plate constants within tolerance.

**Serialization:** Custom token set matching the DSS FITS keywords stored
in the native representation.

**FitsChan integration:** The DSS encoding reader/writer in FitsChan must
be updated to construct/deconstruct DssMap objects.

---

### Phase 4: Frame-Dependent Mapping

#### 8. NormMap

Normalises coordinates using the `norm()` method of a stored Frame.

**Struct fields:**
- `frame: DynFrame` — the frame whose `norm()` method is called

**Transform:**
- Forward: call `frame.norm(point)` for each input point
- Inverse: identical to forward (normalization is idempotent)

**Nin/Nout:** both equal `frame.naxes()`

**Simplify rules:**
- If frame is a BasicFrame → simplify to UnitMap (BasicFrame::norm is identity)
- Inverse-cancel (NormMap + inverse NormMap with equal frames)
- Duplicate elimination (adjacent NormMaps with equal frames → single NormMap)

**equal():** Stored frames must be equal (using Frame::equal).

**Serialization:** Child node containing the serialized Frame.

---

### Phase 5: Expression Evaluator

#### 9. MathMap

Transforms coordinates using user-supplied mathematical expressions parsed
and evaluated at runtime.

**This is the largest and most complex mapping.** The C implementation is
~7,400 lines including a complete expression compiler and stack-based VM.

##### Architecture

```
                    ┌─────────────┐
  "y = sin(x1)*2"  │   Parser    │  → Vec<Token>
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │  Compiler   │  → CompiledExpr { opcodes, constants }
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
  input points ──→  │  Evaluator  │  ──→ output points
                    └─────────────┘
```

The implementation is a custom stack-based bytecode VM. No external crate
is suitable because the MathMap expression language has:
- Custom syntax (Fortran-like variable names, angle bracket constants)
- AST-specific BAD_VALUE propagation semantics
- Three-valued logic (AND/OR treat BAD as unknown)
- Bitwise operations on floats (reinterpret as 64-bit integers)
- A specific random number generator (must match C output exactly)
- Variable-arity `max(a,b,c,...)` and `min(a,b,c,...)`

##### Struct Fields

```rust
pub struct MathMap {
    fwd_exprs: Vec<String>,
    inv_exprs: Vec<String>,
    fwd_compiled: Vec<CompiledExpr>,
    inv_compiled: Vec<CompiledExpr>,
    rng: MathMapRng,
    simp_fi: Option<bool>,
    simp_if: Option<bool>,
}
```

##### Expression Language Specification

**Variables:**
- Input: `x1`, `x2`, ... `xN` (1-based, matching nin)
- Output: assigned via `y = expr` or `y1 = expr`, `y2 = expr` for
  multi-output (1-based indices matching nout)

**System constants** (angle-bracket syntax):
- `<bad>` — AST__BAD value
- `<dig>` — DBL_DIG
- `<epsilon>` — DBL_EPSILON
- `<mant_dig>` — DBL_MANT_DIG
- `<max>` — DBL_MAX
- `<max_10_exp>` — DBL_MAX_10_EXP
- `<max_exp>` — DBL_MAX_EXP
- `<min>` — DBL_MIN
- `<min_10_exp>` — DBL_MIN_10_EXP
- `<min_exp>` — DBL_MIN_EXP
- `<radix>` — FLT_RADIX
- `<rounds>` — FLT_ROUNDS
- `<e>` — Euler's number
- `<pi>` — pi

**Unary functions** (1 argument):
`abs`, `acos`, `acosd`, `acosh`, `acoth`, `acsch`, `aint` (truncate to int),
`asech`, `asin`, `asind`, `asinh`, `atan`, `atand`, `atanh`,
`ceil`, `cos`, `cosd`, `cosh`, `coth`, `csch`, `exp`, `floor`,
`int` (same as `aint`), `isbad`, `log`, `log10`, `nint`,
`poiss` (Poisson random), `sech`, `sin`, `sinc`, `sind`, `sinh`,
`sqr`, `sqrt`, `tan`, `tand`, `tanh`

**Binary functions** (2 arguments):
`atan2`, `atan2d`, `dim` (positive difference), `gauss` (Gaussian random),
`mod`, `pow`, `rand` (uniform random in [a,b]), `sign`

**Ternary operator:**
`a ? b : c` (C-style conditional — if `a` is true/nonzero, result is `b`, else `c`)

**Variable-arity functions:**
`max(a, b, ...)`, `min(a, b, ...)`

**Arithmetic operators** (standard precedence):
`+`, `-` (unary/binary), `*`, `/`, `**` (power)

**Bitwise operators** (float → int64 → operation → float):
`>>` (shift right), `<<` (shift left), `&` (AND), `|` (OR), `^` (XOR)

**Relational operators:** `==`, `!=`, `<`, `>`, `<=`, `>=`

**Logical operators:** `.and.`, `.or.`, `.not.`, `.eqv.`, `.xor.`
(three-valued: BAD propagates as unknown through AND/OR)

**BAD_VALUE propagation:**
- Any arithmetic with a BAD input produces BAD output
- `isbad(x)` returns 1.0 if x is BAD, 0.0 otherwise (does not propagate)
- `a ? b : c`: if `a` is BAD, result is BAD; only the selected branch propagates
- Three-valued AND: `BAD .and. false = false`, `BAD .and. true = BAD`
- Three-valued OR: `BAD .or. true = true`, `BAD .or. false = BAD`

**Math error handling:**
- Division by zero → BAD
- sqrt of negative → BAD
- log of non-positive → BAD
- Domain errors in trig inverses → BAD

##### Random Number Generator

The C implementation uses a combined generator (two linear congruential
generators + shuffle table) that must be reproduced exactly to match
oracle tests. The `Seed` attribute initializes the generator state.

```rust
struct MathMapRng {
    rand1: i64,
    rand2: i64,
    random_int: i64,
    table: [i64; 32],
    active: bool,
    seed: i32,
    seed_set: bool,
}
```

The `Rand`, `Gauss`, and `Poiss` functions draw from this generator.
The algorithm must be ported exactly from the C `Rand()`, `Gauss()`,
and `Poisson()` functions.

##### Parser and Compilation Strategy

**Dependencies:** `logos` (MIT/Apache-2.0) for tokenization via derive macro,
added to `ast-core/Cargo.toml` only (used exclusively by the MathMap module).
No parser library — the parser is a handwritten Pratt (precedence-climbing)
parser, which is the textbook fit for expression languages with many
precedence levels.

**Tokenizer (logos):** Define token variants for:
- Identifiers (`x1`, `sin`, `atan2`, etc.)
- Numbers (integer and float literals)
- Operators (`+`, `-`, `*`, `/`, `**`, `==`, `!=`, `<`, `>`, `<=`, `>=`,
  `<<`, `>>`, `&`, `|`, `^`)
- Logical operators (`.and.`, `.or.`, `.not.`, `.eqv.`, `.xor.`)
- Punctuation (`(`, `)`, `,`, `?`, `:`, `=`)
- System constants (`<bad>`, `<pi>`, `<e>`, etc.)

**Parser (custom Pratt, ~200 lines):** Precedence-climbing parser that
produces a postfix opcode stream. Operator precedence levels match C's
`mathmap.c` symbol table exactly (19 levels).

**Compiled representation:**

```rust
struct CompiledExpr {
    opcodes: Vec<Opcode>,
    constants: Vec<f64>,
    stack_size: usize,
}
```

The opcode set matches the C `OP_*` enum (approximately 80 opcodes
covering loads, unary functions, binary functions, ternary, and
variable-arity operations).

##### Simplification

- `SimpFI = true` → forward+inverse MathMap pair cancels to UnitMap
- `SimpIF = true` → inverse+forward MathMap pair cancels to UnitMap
- Only applies when both MathMaps have identical expression strings

##### Serialization Tokens

`Fwd(i)` — forward expression string i
`Inv(i)` — inverse expression string i
`Nfwd` — number of forward expressions
`Ninv` — number of inverse expressions
`Seed` — random seed (if set)
`SimpFI` — boolean
`SimpIF` — boolean

---

## Cross-Cutting Concerns

### Serialization Dispatch

Each new mapping requires:
1. A `from_node()` arm in `serial/mod.rs::mapping_from_node()`
2. A downcast arm in `serial/mod.rs::MappingToNode::to_node()`
3. The class-specific `Serializable` impl

### Simplify Test Fixtures

As each mapping lands, its corresponding entries in `simplify_tests.txt`
are un-commented/enabled. The fixture files already exist in the repo.
The negative-test entries (prefixed `neg_`) verify that invalid merges
are correctly rejected.

### Oracle Tests (Tier 4)

Each mapping gets an oracle test in `ast-oracle-tests` that:
1. Constructs identical objects in both Rust and C
2. Transforms a grid of test points in both
3. Asserts outputs match to ULP tolerance

### equal() Completeness

Every `equal()` implementation must handle:
- Inverted vs non-inverted comparison (where semantically meaningful)
- Tolerance-based floating-point comparison (using AST's standard tolerance)
- Recursive equality for wrapper mappings (TranMap, RateMap, NormMap)
- Class mismatch → always false

---

## Estimated Effort

| Phase | Mappings | Relative Complexity |
|-------|----------|-------------------|
| 1 | PcdMap, GrismMap, UnitNormMap | Medium (each ~300-600 lines Rust) |
| 2 | TranMap, RateMap | Low-medium (wrapper logic, ~200-400 lines) |
| 3 | SplineMap, DssMap | Medium-high (numerical algorithms, ~500-800 lines) |
| 4 | NormMap | Low (~200 lines) |
| 5 | MathMap | High (~1500-2500 lines for parser + VM + RNG) |

Total estimated: ~5,000-8,000 lines of Rust (excluding tests).

---

## Risks and Mitigations

1. **MathMap expression language edge cases**: The C parser has decades of
   accumulated behavior. Mitigation: extensive oracle testing + port the
   C test expressions directly.

2. **DssMap plate solution numerics**: The `platepos`/`platepix` algorithms
   use high-order polynomials that can diverge. Mitigation: port the C code
   line-by-line, verify with oracle tests on real DSS plate data.

3. **SplineMap inverse convergence**: Newton-Raphson for 2D B-spline inversion
   can fail to converge in pathological cases. Mitigation: match C iteration
   limits and fallback behavior exactly.

4. **Random number reproducibility (MathMap)**: The combined generator must
   produce the same sequence as C. Mitigation: unit test the RNG in isolation
   against a known sequence from the C implementation before integrating.

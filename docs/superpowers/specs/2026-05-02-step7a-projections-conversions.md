# Step 7a: Projection and Conversion Infrastructure — Design Spec

## Summary

Implement the foundational infrastructure for astronomy coordinate
handling: safe Rust FFI wrappers around the vendored C PAL and wcslib
libraries, plus two Mapping types that use them — WcsMap (FITS-WCS sky
projections) and SlaMap (celestial coordinate system conversions).

This is the first of three sub-projects leading to full SkyFrame
support:
- **Step 7a** (this): PAL FFI, wcslib FFI, WcsMap, SlaMap
- **Step 7b**: SkyFrame + SkyAxis
- **Step 7c**: SpecFrame, TimeFrame, FluxFrame, SpecFluxFrame

## Deliverables

1. `pal` module — safe Rust wrappers for vendored C PAL functions.
2. `wcslib` module — safe Rust wrappers for vendored C wcslib
   projections, `WcsProjection` enum, `PrjParams` struct.
3. `WcsMap` — Mapping type applying FITS-WCS sky projections (31
   types).
4. `SlaMap` — Mapping type chaining coordinate system conversions
   (FK4, FK5, Galactic, etc.) via PAL.
5. Serializable impls + dispatch updates for both mapping types.
6. Tests for all components.

## Module Structure

```
crates/starlink-ast/src/
├── pal.rs              # Safe Rust wrappers for PAL C functions
├── wcslib.rs           # Safe Rust wrappers for wcslib C projections
├── mappings/
│   ├── wcsmap.rs       # WcsMap
│   ├── slamap.rs       # SlaMap + SlaCvtType
│   └── mod.rs          # updated re-exports
```

## PAL FFI Wrapper

`crates/starlink-ast/src/pal.rs` — `pub(crate)` module.

Safe Rust functions wrapping the vendored C PAL library. The C
library is already compiled by `build.rs` into `libast_pal_erfa.a`
(from `vendor/palwrap.c`). PAL symbols are prefixed with `pal`
(e.g., `palEpj`, `palDcs2c`).

### Functions needed for Step 7a

SlaMap and WcsMap need these PAL functions:

**Epoch conversions:**
- `palEpj(mjd) -> f64` — MJD to Julian epoch
- `palEpj2d(epj) -> f64` — Julian epoch to MJD
- `palEpb(mjd) -> f64` — MJD to Besselian epoch
- `palEpb2d(epb) -> f64` — Besselian epoch to MJD

**Spherical/Cartesian conversions:**
- `palDcs2c(lon, lat) -> [f64; 3]` — spherical to direction cosines
- `palDcc2s(v: &[f64; 3]) -> (f64, f64)` — direction cosines to spherical

**Vector/matrix operations:**
- `palDmxv(dm: &[[f64; 3]; 3], va: &[f64; 3]) -> [f64; 3]` — matrix × vector
- `palDimxv(dm: &[[f64; 3]; 3], va: &[f64; 3]) -> [f64; 3]` — inverse matrix × vector
- `palDmxm(da: &[[f64; 3]; 3], db: &[[f64; 3]; 3]) -> [[f64; 3]; 3]` — matrix × matrix
- `palDvxv(va: &[f64; 3], vb: &[f64; 3]) -> [f64; 3]` — cross product
- `palDvdv(va: &[f64; 3], vb: &[f64; 3]) -> f64` — dot product
- `palDvn(v: &[f64; 3]) -> ([f64; 3], f64)` — normalize vector

**Coordinate system conversions:**
- `palFk45z(r1950, d1950, bepoch) -> (r2000, d2000)` — FK4 to FK5
- `palFk54z(r2000, d2000, bepoch) -> (r1950, d1950, dr, dd)` — FK5 to FK4
- `palFk5hz(r5, d5, epoch) -> (rh, dh)` — FK5 to ICRS
- `palHfk5z(rh, dh, epoch) -> (r5, d5, dr5, dd5)` — ICRS to FK5
- `palGaleq(dl, db) -> (dr, dd)` — Galactic to FK5
- `palEqgal(dr, dd) -> (dl, db)` — FK5 to Galactic
- `palGalsup(dl, db) -> (dsl, dsb)` — Galactic to Supergalactic
- `palSupgal(dsl, dsb) -> (dl, db)` — Supergalactic to Galactic

**Precession:**
- `palPrebn(bep0, bep1) -> [[f64; 3]; 3]` — Besselian precession matrix
- `palPrec(ep0, ep1) -> [[f64; 3]; 3]` — Julian (FK5) precession matrix

**Ecliptic:**
- `palEcmat(date) -> [[f64; 3]; 3]` — equatorial to ecliptic rotation matrix

**Aberration:**
- `palAddet(rm, dm, eq) -> (rc, dc)` — add E-terms
- `palSubet(rc, dc, eq) -> (rm, dm)` — remove E-terms
- `palMappa(eq, date) -> [f64; 21]` — mean-to-apparent parameters
- `palAmpqk(ra, da, amprms: &[f64; 21]) -> (rm, dm)` — apparent to mean
- `palMapqkz(rm, dm, amprms: &[f64; 21]) -> (ra, da)` — mean to apparent

**Earth position:**
- `palEvp(date, deqx) -> ([f64; 3], [f64; 3], [f64; 3], [f64; 3])` — Earth velocity/position

**Angular utilities:**
- `palDrange(angle) -> f64` — normalize to [-pi, pi]
- `palDranrm(angle) -> f64` — normalize to [0, 2pi]
- `palDeuler(order: &str, phi, theta, psi) -> [[f64; 3]; 3]` — Euler rotation matrix

**Horizon:**
- `palDe2h(ha, dec, phi) -> (az, el)` — equatorial to horizon
- `palDh2e(az, el, phi) -> (ha, dec)` — horizon to equatorial

Additional PAL functions needed later (Steps 7b, 7c) can be added
to this module as needed. The wrapper is designed to grow.

### Pattern

Each C function gets a safe Rust wrapper:

```rust
extern "C" {
    fn palEpj(date: f64) -> f64;
}

pub(crate) fn epj(date: f64) -> f64 {
    unsafe { palEpj(date) }
}
```

For functions that write to output pointers:

```rust
extern "C" {
    fn palDcs2c(a: f64, b: f64, v: *mut f64);
}

pub(crate) fn dcs2c(a: f64, b: f64) -> [f64; 3] {
    let mut v = [0.0; 3];
    unsafe { palDcs2c(a, b, v.as_mut_ptr()) };
    v
}
```

## wcslib FFI Wrapper

`crates/starlink-ast/src/wcslib.rs` — `pub(crate)` module.

Safe Rust wrappers for the vendored C wcslib projection library.
The C code is compiled by `build.rs` into `libast_wcslib.a` (from
`vendor/wcslib/proj.c`, `tpn.c`, `wcstrig.c`).

### WcsProjection Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub(crate) enum WcsProjection {
    Azp = 1,
    Szp = 2,
    Tan = 3,
    Stg = 4,
    Sin = 5,
    Arc = 6,
    Zpn = 7,
    Zea = 8,
    Air = 9,
    Cyp = 10,
    Cea = 11,
    Car = 12,
    Mer = 13,
    Sfl = 14,
    Par = 15,
    Mol = 16,
    Ait = 17,
    Cop = 18,
    Coe = 19,
    Cod = 20,
    Coo = 21,
    Bon = 22,
    Pco = 23,
    Tsc = 24,
    Csc = 25,
    Qsc = 26,
    Ncp = 27,   // deprecated SIN variant
    Gls = 28,   // deprecated SFL variant
    Tpn = 29,
    Hpx = 30,
    Xph = 31,
}
```

Each variant has a `code() -> &'static str` method returning the
3-letter FITS code (e.g., `Tan.code() == "TAN"`).

A `from_code(s: &str) -> Option<Self>` constructor parses the FITS
code.

A `from_int(i: i32) -> Option<Self>` constructor converts from the
integer type code.

### PrjParams Struct

```rust
pub(crate) struct PrjParams {
    inner: AstPrjPrm,  // the C struct, owned
}
```

The C struct `AstPrjPrm` is:
```c
struct AstPrjPrm {
    char code[4];
    int flag;
    double phi0, theta0;
    double r0;
    double *p;     // projection parameters (lon axis)
    double *p2;    // projection parameters (lat axis)
    double w[20];  // working storage
    int n;
    int (*astPRJfwd)(...);
    int (*astPRJrev)(...);
};
```

The Rust wrapper manages the parameter arrays and provides:

```rust
impl PrjParams {
    /// Initialize for a given projection type.
    pub fn new(proj: WcsProjection) -> AstResult<Self>;

    /// Set a projection parameter.
    pub fn set_param(&mut self, axis: usize, m: usize, value: f64);

    /// Get a projection parameter.
    pub fn get_param(&self, axis: usize, m: usize) -> Option<f64>;

    /// Forward projection: (phi_deg, theta_deg) → (x, y).
    pub fn forward(&mut self, phi: f64, theta: f64) -> AstResult<(f64, f64)>;

    /// Reverse projection: (x, y) → (phi_deg, theta_deg).
    pub fn reverse(&mut self, x: f64, y: f64) -> AstResult<(f64, f64)>;
}
```

The `forward`/`reverse` methods call through the C function pointers
stored in the `AstPrjPrm` struct. The `new` method calls the
appropriate `astXXXset` function to initialize the function pointers
and default parameters.

`PrjParams` is not `Send` (contains raw pointers in the C struct).
WcsMap will own it and is itself `Send` because it never shares the
`PrjParams` across threads.

## WcsMap

`crates/starlink-ast/src/mappings/wcsmap.rs`

A Mapping that applies a FITS-WCS sky projection. Transforms between
native spherical coordinates (longitude, latitude) and projected
plane coordinates (x, y).

```rust
pub struct WcsMap {
    proj: WcsProjection,
    lonax: usize,            // 0-based longitude axis
    latax: usize,            // 0-based latitude axis
    ncoord: usize,           // total axes (≥ 2)
    params: Vec<Vec<f64>>,   // PV parameters per axis
    prj: PrjParams,          // wcslib projection state
    inverted: bool,
}
```

### Constructor

`WcsMap::new(ncoord: usize, proj: WcsProjection, lonax: usize, latax: usize) -> AstResult<Self>`

- `lonax` and `latax` are 1-based (AST convention)
- Validates: ncoord ≥ 2, lonax != latax, both in 1..=ncoord

`set_pv(&mut self, axis: usize, m: usize, value: f64)` — set a
projection parameter (PVi_m in FITS notation). Axis is 1-based.

### Mapping trait

- `nin()` = `nout()` = `ncoord`
- Forward: extract (lon, lat) from input axes → convert radians to
  degrees → call `prj.forward()` → convert degrees to radians →
  place in output. Other axes pass through.
- Inverse: extract (x, y) from input axes → convert to degrees →
  call `prj.reverse()` → convert to radians → place in output.
- BAD_VALUE propagation as usual.
- `simplified()`: return clone
- `rate`: use `numeric_rate`

### Serializable

Class: `"WcsMap"`.

| Key | Condition | Comment |
|-----|-----------|---------|
| `Type` | always | FITS 3-letter projection code as string |
| `PVi_m` | if set | "Projection parameter m for axis i" |
| `WcsAx1` | if != default | "Index of celestial longitude axis" |
| `WcsAx2` | if != default | "Index of celestial latitude axis" |

## SlaMap

`crates/starlink-ast/src/mappings/slamap.rs`

A Mapping that chains celestial coordinate system conversions. Each
conversion step transforms direction cosines (3-vectors) using PAL
library functions.

```rust
/// A coordinate system conversion step.
#[derive(Debug, Clone)]
struct SlaConversion {
    cvt_type: SlaCvtType,
    args: Vec<f64>,
}

/// Supported conversion types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlaCvtType {
    Addet  = 1,   // Add E-terms of aberration
    Subet  = 2,   // Remove E-terms
    Prebn  = 3,   // Bessel-Newcomb precession (FK4)
    Prec   = 4,   // IAU 1976 precession (FK5)
    Fk45z  = 5,   // FK4 → FK5
    Fk54z  = 6,   // FK5 → FK4
    Amp    = 7,   // Geocentric apparent → mean
    Map    = 8,   // Mean → geocentric apparent
    Ecleq  = 9,   // Ecliptic → equatorial
    Eqecl  = 10,  // Equatorial → ecliptic
    Galeq  = 11,  // Galactic → equatorial
    Eqgal  = 12,  // Equatorial → galactic
    Galsup = 13,  // Galactic → supergalactic
    Supgal = 14,  // Supergalactic → galactic
    Hfk5z  = 19,  // ICRS → FK5
    Fk5hz  = 20,  // FK5 → ICRS
    Dh2e   = 25,  // Horizon → equatorial
    De2h   = 26,  // Equatorial → horizon
}

pub struct SlaMap {
    conversions: Vec<SlaConversion>,
    inverted: bool,
}
```

### Constructor and step addition

```rust
impl SlaMap {
    pub fn new() -> Self;
    pub fn add(&mut self, cvt_type: SlaCvtType, args: &[f64]);
}
```

Each conversion type has a known number of arguments:
- `Addet`, `Subet`: 1 arg (equinox as Besselian epoch)
- `Prebn`: 2 args (from/to Besselian epochs)
- `Prec`: 2 args (from/to Julian epochs)
- `Fk45z`, `Fk54z`: 1 arg (Besselian epoch)
- `Amp`, `Map`: 2 args (equinox, MJD)
- `Ecleq`, `Eqecl`: 1 arg (MJD)
- `Galeq`, `Eqgal`, `Galsup`, `Supgal`: 0 args
- `Hfk5z`, `Fk5hz`: 1 arg (Julian epoch)
- `Dh2e`, `De2h`: 1 arg (observer latitude in radians)

### Mapping trait

- `nin()` = `nout()` = 2 (longitude, latitude in radians)
- Forward transform for each point:
  1. (lon, lat) → 3-vector via `pal::dcs2c`
  2. Apply each conversion step in order (each modifies the 3-vector)
  3. 3-vector → (lon, lat) via `pal::dcc2s`
- Inverse: traverse chain in reverse, applying each step's inverse
- Each conversion step has a known inverse:
  `Addet ↔ Subet`, `Prebn(a,b) ↔ Prebn(b,a)`,
  `Prec(a,b) ↔ Prec(b,a)`, `Fk45z ↔ Fk54z`,
  `Amp ↔ Map`, `Ecleq ↔ Eqecl`, `Galeq ↔ Eqgal`,
  `Galsup ↔ Supgal`, `Hfk5z ↔ Fk5hz`, `Dh2e ↔ De2h`

### Serializable

Class: `"SlaMap"`.

| Key | Description |
|-----|-------------|
| `Nsla` | Number of conversion steps |
| `Sla1`..`SlaN` | String code per step (e.g., "ADDET", "EQGAL") |
| `Sla1A`, `Sla1B`, ... | Double arguments per step (suffix A, B, C...) |

The string codes are the uppercase names from `SlaCvtType`.

## Testing Strategy

### PAL wrapper tests
- Epoch round-trips: `epj(epj2d(2000.0))` ≈ 2000.0
- Spherical/Cartesian: `dcs2c(0, 0)` = [1, 0, 0], round-trip
- Known star: Galactic center ≈ (266.4°, -28.9°) in FK5

### wcslib wrapper tests
- TAN forward/reverse round-trip at reference point
- SIN, AIT, CAR round-trips
- TAN with known offset: verify against hand-calculated values

### WcsMap tests
- TAN projection: forward/inverse for known coordinates
- Multi-axis passthrough
- PV parameter setting
- Different projection types (SIN, AIT, CAR)
- Serialization round-trip

### SlaMap tests
- Single-step FK5→Galactic for known star, verify result
- Multi-step chain: FK4→FK5→Galactic round-trip
- Precession between epochs
- Inversion: chain then inverted chain is identity
- Serialization round-trip

## What This Step Does NOT Include

- SkyFrame / SkyAxis (Step 7b)
- SpecFrame, TimeFrame, FluxFrame (Step 7c)
- Pure Rust wcslib port (Step 6a, deferred)
- PAL functions only used by SpecFrame/TimeFrame (added in 7c)

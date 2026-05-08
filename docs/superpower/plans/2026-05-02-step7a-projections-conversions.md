# Step 7a: Projection and Conversion Infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement PAL and wcslib FFI wrappers, WcsMap (FITS-WCS sky projections), and SlaMap (celestial coordinate system conversions).

**Architecture:** Thin safe-Rust FFI wrappers over vendored C libraries (already compiled by build.rs). WcsMap uses wcslib for 31 projection types. SlaMap chains PAL-based conversion steps. Both implement the Mapping trait and Serializable following established patterns.

**Tech Stack:** Rust 2024 edition. Vendored C: PAL/ERFA (`libast_pal_erfa.a`), wcslib (`libast_wcslib.a`). No new Cargo dependencies.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/src/pal.rs` | Safe Rust wrappers for vendored PAL C functions |
| `crates/ast-core/src/wcslib.rs` | Safe Rust wrappers for vendored wcslib C projections |
| `crates/ast-core/src/mappings/wcsmap.rs` | WcsMap struct, Mapping impl, Serializable impl |
| `crates/ast-core/src/mappings/slamap.rs` | SlaMap + SlaCvtType, Mapping impl, Serializable impl |
| `crates/ast-core/src/mappings/mod.rs` | Updated re-exports |
| `crates/ast-core/src/serial/mod.rs` | Updated dispatch points |
| `crates/ast-core/src/lib.rs` | Add `pal` and `wcslib` modules |
| `crates/ast-core/tests/test_wcsmap.rs` | WcsMap tests |
| `crates/ast-core/tests/test_slamap.rs` | SlaMap tests |

---

### Task 1: PAL FFI Wrapper

**Files:**
- Create: `crates/ast-core/src/pal.rs`
- Modify: `crates/ast-core/src/lib.rs`

Create safe Rust wrappers for the vendored PAL C library. The C
library is already compiled by `build.rs` into `libast_pal_erfa.a`
(from `vendor/palwrap.c` which includes all individual PAL source
files). PAL function names are prefixed with `pal` (e.g. `palEpj`).

- [ ] **Step 1: Create pal.rs with extern declarations and safe wrappers**

Create `crates/ast-core/src/pal.rs`. The module is `pub(crate)`.

The file needs an `extern "C"` block declaring each PAL function,
then a safe Rust wrapper function for each. Group by category:

**Epoch conversions** (scalar in, scalar out — simplest wrappers):
```rust
extern "C" { fn palEpj(date: f64) -> f64; }
pub(crate) fn epj(date: f64) -> f64 { unsafe { palEpj(date) } }
```
Same pattern for: `palEpj2d`, `palEpb`, `palEpb2d`.

**Spherical ↔ Cartesian** (output via pointer):
```rust
extern "C" { fn palDcs2c(a: f64, b: f64, v: *mut f64); }
pub(crate) fn dcs2c(a: f64, b: f64) -> [f64; 3] {
    let mut v = [0.0; 3];
    unsafe { palDcs2c(a, b, v.as_mut_ptr()) };
    v
}

extern "C" { fn palDcc2s(v: *const f64, a: *mut f64, b: *mut f64); }
pub(crate) fn dcc2s(v: &[f64; 3]) -> (f64, f64) {
    let (mut a, mut b) = (0.0, 0.0);
    unsafe { palDcc2s(v.as_ptr(), &mut a, &mut b) };
    (a, b)
}
```

**Matrix/vector operations** (3×3 rotation matrices as `[[f64; 3]; 3]`):
```rust
extern "C" { fn palDmxv(dm: *const [f64; 3], va: *const f64, vb: *mut f64); }
pub(crate) fn dmxv(dm: &[[f64; 3]; 3], va: &[f64; 3]) -> [f64; 3] {
    let mut vb = [0.0; 3];
    unsafe { palDmxv(dm.as_ptr(), va.as_ptr(), vb.as_mut_ptr()) };
    vb
}
```
Same pattern for: `palDimxv`, `palDmxm`, `palDvxv`, `palDvdv`, `palDvn`.

**Coordinate system conversions** (pairs of angles in/out):
```rust
extern "C" {
    fn palGaleq(dl: f64, db: f64, dr: *mut f64, dd: *mut f64);
}
pub(crate) fn galeq(dl: f64, db: f64) -> (f64, f64) {
    let (mut dr, mut dd) = (0.0, 0.0);
    unsafe { palGaleq(dl, db, &mut dr, &mut dd) };
    (dr, dd)
}
```
Same pattern for: `palEqgal`, `palGalsup`, `palSupgal`, `palFk45z`
(has extra outputs), `palFk54z`, `palFk5hz`, `palHfk5z`.

**Precession matrices** (return 3×3):
```rust
extern "C" {
    fn palPrec(ep0: f64, ep1: f64, rmatp: *mut [f64; 3]);
}
pub(crate) fn prec(ep0: f64, ep1: f64) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    unsafe { palPrec(ep0, ep1, m.as_mut_ptr()) };
    m
}
```
Same pattern for: `palPrebn`, `palEcmat`, `palDeuler` (takes string
order + 3 angles).

**Aberration:**
```rust
extern "C" {
    fn palAddet(rm: f64, dm: f64, eq: f64, rc: *mut f64, dc: *mut f64);
}
pub(crate) fn addet(rm: f64, dm: f64, eq: f64) -> (f64, f64) { ... }
```
Same pattern for: `palSubet`, `palMappa` (returns 21-element array),
`palAmpqk`, `palMapqkz`.

**Angular normalisation:**
```rust
extern "C" { fn palDrange(angle: f64) -> f64; }
pub(crate) fn drange(angle: f64) -> f64 { unsafe { palDrange(angle) } }
```
Same for: `palDranrm`.

**Earth/observer:**
```rust
pub(crate) fn evp(date: f64, deqx: f64) -> ... { ... }
```

**Horizon conversions:**
```rust
pub(crate) fn de2h(ha: f64, dec: f64, phi: f64) -> (f64, f64) { ... }
pub(crate) fn dh2e(az: f64, el: f64, phi: f64) -> (f64, f64) { ... }
```

Implement all functions listed in the design spec's PAL section.
For functions with complex signatures (e.g., `palFk54z` returning 4
values), return a tuple or small struct.

- [ ] **Step 2: Add to lib.rs**

Add `pub(crate) mod pal;` to `crates/ast-core/src/lib.rs`.

- [ ] **Step 3: Write PAL wrapper tests**

Add a `#[cfg(test)] mod tests` block in `pal.rs` with tests:

```rust
#[test]
fn epj_j2000() {
    // MJD 51544.5 = J2000.0
    let result = super::epj(51544.5);
    assert!((result - 2000.0).abs() < 1e-6);
}

#[test]
fn epj_round_trip() {
    let mjd = 58000.0;
    let result = super::epj2d(super::epj(mjd));
    assert!((result - mjd).abs() < 1e-9);
}

#[test]
fn dcs2c_pole() {
    // North pole: lon=0, lat=pi/2 → [0, 0, 1]
    let v = super::dcs2c(0.0, std::f64::consts::FRAC_PI_2);
    assert!(v[0].abs() < 1e-15);
    assert!(v[1].abs() < 1e-15);
    assert!((v[2] - 1.0).abs() < 1e-15);
}

#[test]
fn dcs2c_dcc2s_round_trip() {
    let (lon, lat) = (1.23, 0.45);
    let v = super::dcs2c(lon, lat);
    let (lon2, lat2) = super::dcc2s(&v);
    assert!((lon2 - lon).abs() < 1e-14);
    assert!((lat2 - lat).abs() < 1e-14);
}

#[test]
fn galactic_centre_approx() {
    // Galactic centre (l=0, b=0) ≈ RA 266.4°, Dec -28.9° in FK5
    let (ra, dec) = super::galeq(0.0, 0.0);
    let ra_deg = ra.to_degrees();
    let dec_deg = dec.to_degrees();
    assert!((ra_deg - 266.4).abs() < 0.2);
    assert!((dec_deg - (-28.94)).abs() < 0.2);
}
```

- [ ] **Step 4: Verify**

Run: `cargo test -p ast-core`
Expected: all tests pass (including PAL tests)

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/pal.rs crates/ast-core/src/lib.rs
git commit -m "Add PAL FFI wrapper module with safe Rust functions"
```

---

### Task 2: wcslib FFI Wrapper

**Files:**
- Create: `crates/ast-core/src/wcslib.rs`
- Modify: `crates/ast-core/src/lib.rs`

Create safe Rust wrappers for the vendored wcslib C projection
library. The C code is compiled by `build.rs` into `libast_wcslib.a`.

- [ ] **Step 1: Create wcslib.rs with WcsProjection enum**

Create `crates/ast-core/src/wcslib.rs`. The module is `pub(crate)`.

Define the `WcsProjection` enum with all 31 projection types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WcsProjection {
    Azp, Szp, Tan, Stg, Sin, Arc, Zpn, Zea, Air,
    Cyp, Cea, Car, Mer, Sfl, Par, Mol, Ait,
    Cop, Coe, Cod, Coo, Bon, Pco,
    Tsc, Csc, Qsc,
    Ncp, Gls,  // deprecated variants
    Tpn, Hpx, Xph,
}
```

Implement `code(&self) -> &'static str` (returns 3-letter FITS code),
`from_code(s: &str) -> Option<Self>`, and
`type_code(&self) -> i32` (returns the integer constant matching C's
AST__XXX values: Azp=1, Szp=2, ..., Xph=31).

Also implement `from_type_code(i: i32) -> Option<Self>`.

- [ ] **Step 2: Add AstPrjPrm struct and extern declarations**

The C struct `AstPrjPrm` from `vendor/wcslib/proj.h`:
```c
struct AstPrjPrm {
    char code[4];
    int flag;
    double phi0, theta0;
    double r0;
    double *p;
    double *p2;
    double w[20];
    int n;
    int (*astPRJfwd)(...);
    int (*astPRJrev)(...);
};
```

Define the Rust FFI repr:
```rust
#[repr(C)]
struct AstPrjPrm {
    code: [u8; 4],
    flag: c_int,
    phi0: f64,
    theta0: f64,
    r0: f64,
    p: *mut f64,
    p2: *mut f64,
    w: [f64; 20],
    n: c_int,
    fwd: Option<unsafe extern "C" fn(f64, f64, *mut AstPrjPrm, *mut f64, *mut f64) -> c_int>,
    rev: Option<unsafe extern "C" fn(f64, f64, *mut AstPrjPrm, *mut f64, *mut f64) -> c_int>,
}
```

Declare extern functions:
```rust
extern "C" {
    fn astPRJset(code: *const u8, prm: *mut AstPrjPrm) -> c_int;
    fn astPRJfwd(phi: f64, theta: f64, prm: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astPRJrev(x: f64, y: f64, prm: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
}
```

- [ ] **Step 3: Implement PrjParams safe wrapper**

```rust
const WCSLIB_MXPAR: usize = 100;

pub(crate) struct PrjParams {
    inner: AstPrjPrm,
    p_storage: Vec<f64>,    // backing for inner.p
    p2_storage: Vec<f64>,   // backing for inner.p2
}
```

The `p` and `p2` fields in `AstPrjPrm` are raw pointers. `PrjParams`
owns the backing `Vec<f64>` storage and sets the pointers into them.
Must be careful not to move/reallocate after setting pointers.

```rust
impl PrjParams {
    pub fn new(proj: WcsProjection) -> AstResult<Self> {
        let mut p_storage = vec![0.0; WCSLIB_MXPAR];
        let mut p2_storage = vec![0.0; WCSLIB_MXPAR];
        let mut inner: AstPrjPrm = unsafe { std::mem::zeroed() };
        inner.p = p_storage.as_mut_ptr();
        inner.p2 = p2_storage.as_mut_ptr();
        inner.flag = 0;
        
        let code = proj.code();
        let mut code_buf = [0u8; 4];
        code_buf[..code.len()].copy_from_slice(code.as_bytes());
        inner.code = code_buf;
        
        let status = unsafe { astPRJset(code_buf.as_ptr(), &mut inner) };
        if status != 0 {
            return Err(AstError::new(ErrorCode::Wcsty, "wcslib projection setup failed"));
        }
        
        Ok(Self { inner, p_storage, p2_storage })
    }

    pub fn set_param(&mut self, axis: usize, m: usize, value: f64) {
        if axis == 0 && m < WCSLIB_MXPAR {
            self.p_storage[m] = value;
            self.inner.p = self.p_storage.as_mut_ptr();
        } else if axis == 1 && m < WCSLIB_MXPAR {
            self.p2_storage[m] = value;
            self.inner.p2 = self.p2_storage.as_mut_ptr();
        }
        self.inner.flag = 0; // force re-initialization on next use
    }

    pub fn forward(&mut self, phi: f64, theta: f64) -> AstResult<(f64, f64)> {
        let (mut x, mut y) = (0.0, 0.0);
        let status = unsafe { astPRJfwd(phi, theta, &mut self.inner, &mut x, &mut y) };
        if status != 0 { return Err(...); }
        Ok((x, y))
    }

    pub fn reverse(&mut self, x: f64, y: f64) -> AstResult<(f64, f64)> {
        let (mut phi, mut theta) = (0.0, 0.0);
        let status = unsafe { astPRJrev(x, y, &mut self.inner, &mut phi, &mut theta) };
        if status != 0 { return Err(...); }
        Ok((phi, theta))
    }
}
```

Important: `PrjParams` must NOT implement `Clone` (the inner pointers
would dangle). It also needs a custom `Drop` — but since p/p2 point
into owned Vecs, no explicit Drop is needed; the Vecs handle cleanup.

PrjParams is `!Send` because it contains raw pointers. We need to
manually `unsafe impl Send for PrjParams {}` since the data is fully
owned and not shared. Alternatively, WcsMap can manually impl Send.

- [ ] **Step 4: Add to lib.rs**

Add `pub(crate) mod wcslib;` to `crates/ast-core/src/lib.rs`.

- [ ] **Step 5: Write wcslib tests**

Tests in a `#[cfg(test)]` block:

```rust
#[test]
fn tan_reference_point() {
    let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
    // At the reference point (0, 90°): should give (0, 0)
    let (x, y) = prj.forward(0.0, 90.0).unwrap();
    assert!(x.abs() < 1e-10);
    assert!(y.abs() < 1e-10);
}

#[test]
fn tan_round_trip() {
    let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
    let (x, y) = prj.forward(10.0, 80.0).unwrap();
    let (phi, theta) = prj.reverse(x, y).unwrap();
    assert!((phi - 10.0).abs() < 1e-10);
    assert!((theta - 80.0).abs() < 1e-10);
}

#[test]
fn car_round_trip() {
    let mut prj = PrjParams::new(WcsProjection::Car).unwrap();
    let (x, y) = prj.forward(45.0, 30.0).unwrap();
    let (phi, theta) = prj.reverse(x, y).unwrap();
    assert!((phi - 45.0).abs() < 1e-10);
    assert!((theta - 30.0).abs() < 1e-10);
}

#[test]
fn sin_round_trip() {
    let mut prj = PrjParams::new(WcsProjection::Sin).unwrap();
    let (x, y) = prj.forward(5.0, 85.0).unwrap();
    let (phi, theta) = prj.reverse(x, y).unwrap();
    assert!((phi - 5.0).abs() < 1e-10);
    assert!((theta - 85.0).abs() < 1e-10);
}
```

- [ ] **Step 6: Verify and commit**

Run: `cargo test -p ast-core`

```bash
git add crates/ast-core/src/wcslib.rs crates/ast-core/src/lib.rs
git commit -m "Add wcslib FFI wrapper with WcsProjection enum and PrjParams"
```

---

### Task 3: WcsMap

**Files:**
- Create: `crates/ast-core/src/mappings/wcsmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Create: `crates/ast-core/tests/test_wcsmap.rs`

WcsMap applies a FITS-WCS sky projection. It transforms between
native spherical coordinates and projected plane coordinates.

- [ ] **Step 1: Create wcsmap.rs with struct and constructor**

```rust
pub struct WcsMap {
    proj: WcsProjection,
    lonax: usize,        // 0-based longitude axis index
    latax: usize,        // 0-based latitude axis index
    ncoord: usize,       // total number of axes
    params: Vec<Vec<Option<f64>>>,  // PV params: params[axis][m]
    prj: PrjParams,      // wcslib state
    inverted: bool,
}
```

Constructor: `WcsMap::new(ncoord, proj, lonax, latax)` where lonax
and latax are 1-based. Validates ncoord ≥ 2, lonax != latax, both
in range. Initializes PrjParams.

`set_pv(&mut self, axis: usize, m: usize, value: f64)` — axis is
1-based. Sets the parameter and marks PrjParams for re-init.

`projection(&self) -> WcsProjection` — accessor.

- [ ] **Step 2: Implement Mapping trait**

- `nin()` = `nout()` = ncoord
- Forward: extract lon/lat from axes lonax/latax (radians), convert
  to degrees, call `prj.forward()`, convert back to radians, place
  in output. Other axes pass through unchanged.
- Inverse: extract x/y, convert to degrees, call `prj.reverse()`,
  convert to radians. Other axes pass through.
- BAD_VALUE propagation on input.
- `simplified()`: return clone (need `Clone` — must handle PrjParams
  by creating a fresh PrjParams from the same WcsProjection and
  params).
- `rate`: use `numeric_rate`.

Note: because PrjParams has `&mut self` methods, WcsMap needs
interior mutability or must reinitialize PrjParams on each transform
call. The simplest approach: use `RefCell<PrjParams>` internally, or
make the PrjParams methods take `&mut self` and have WcsMap's
`transform` use internal mutation. Since Mapping::transform takes
`&self`, we need `Cell`/`RefCell` or `UnsafeCell`. `RefCell` is the
cleanest:

```rust
use std::cell::RefCell;

pub struct WcsMap {
    // ...
    prj: RefCell<PrjParams>,
    // ...
}
```

This is fine — WcsMap is `Send` but not `Sync` (per CLAUDE.md rules).

- [ ] **Step 3: Implement Serializable**

Class: `"WcsMap"`.

Attributes (from C Dump):
- `Type` — 3-letter FITS code string (always written)
- `PV{i+1}_{m}` — projection parameters (if set)
- `WcsAx1` — longitude axis index, 1-based (if != default 1)
- `WcsAx2` — latitude axis index, 1-based (if != default 2)

`from_node`: read Type (string → WcsProjection), WcsAx1/WcsAx2,
then scan for PV keys matching pattern `PV{i}_{m}`.

- [ ] **Step 4: Add to mod.rs and serial dispatch**

Add to `mappings/mod.rs`:
```rust
mod wcsmap;
pub use wcsmap::WcsMap;
```

Update `serial/mod.rs`: add WcsMap to all dispatch points. Also
re-export `WcsProjection` from `mappings/mod.rs` if needed by tests.

- [ ] **Step 5: Write tests**

Create `crates/ast-core/tests/test_wcsmap.rs`:

```rust
#[test]
fn tan_forward_inverse() {
    let mut wcs = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    let pts = PointSet::from_rows([[0.1, 1.4]]);  // (lon, lat) in radians
    let out = wcs.transform(&pts, true).unwrap();
    let back = wcs.transform(&out, false).unwrap();
    assert!((back.point(0)[0] - 0.1).abs() < 1e-10);
    assert!((back.point(0)[1] - 1.4).abs() < 1e-10);
}

#[test]
fn multi_axis_passthrough() {
    // 4 axes, lon=2, lat=3 — axes 1 and 4 pass through
    let wcs = WcsMap::new(4, WcsProjection::Car, 2, 3).unwrap();
    let pts = PointSet::from_rows([[99.0, 0.5, 1.0, 42.0]]);
    let out = wcs.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], 99.0);  // axis 1 passthrough
    assert_eq!(out.point(0)[3], 42.0);  // axis 4 passthrough
}

#[test]
fn serialisation_round_trip() {
    let wcs = WcsMap::new(2, WcsProjection::Ait, 1, 2).unwrap();
    let text = to_native_string(&AnyObject::WcsMap(wcs)).unwrap();
    assert!(text.contains("Begin WcsMap"));
    let restored = from_native_string(&text).unwrap();
    let m = restored.as_mapping().unwrap();
    assert_eq!(m.nin(), 2);
}
```

Additional tests: PV parameter setting, different projection types,
BAD_VALUE propagation, inversion.

- [ ] **Step 6: Verify and commit**

Run: `cargo test -p ast-core`

```bash
git add crates/ast-core/src/mappings/ crates/ast-core/src/serial/mod.rs crates/ast-core/tests/test_wcsmap.rs
git commit -m "Add WcsMap (FITS-WCS sky projections via wcslib)"
```

---

### Task 4: SlaMap

**Files:**
- Create: `crates/ast-core/src/mappings/slamap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Create: `crates/ast-core/tests/test_slamap.rs`

SlaMap chains celestial coordinate system conversions via the PAL
library.

- [ ] **Step 1: Create slamap.rs with SlaCvtType, SlaConversion, SlaMap**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlaCvtType {
    Addet  = 1,
    Subet  = 2,
    Prebn  = 3,
    Prec   = 4,
    Fk45z  = 5,
    Fk54z  = 6,
    Amp    = 7,
    Map    = 8,
    Ecleq  = 9,
    Eqecl  = 10,
    Galeq  = 11,
    Eqgal  = 12,
    Galsup = 13,
    Supgal = 14,
    Hfk5z  = 19,
    Fk5hz  = 20,
    Dh2e   = 25,
    De2h   = 26,
}

#[derive(Debug, Clone)]
struct SlaConversion {
    cvt_type: SlaCvtType,
    args: Vec<f64>,
}

pub struct SlaMap {
    conversions: Vec<SlaConversion>,
    inverted: bool,
}
```

Each cvt_type has a known number of args. Implement
`SlaCvtType::nargs(&self) -> usize` and
`SlaCvtType::code_string(&self) -> &'static str` (returns uppercase
name for serialisation: "ADDET", "SUBET", etc.).

`SlaCvtType::from_code(s: &str) -> Option<Self>` parses the string.

`SlaCvtType::inverse(&self) -> Self` returns the inverse conversion
type (Addet↔Subet, Fk45z↔Fk54z, etc.).

- [ ] **Step 2: Implement constructor and add method**

```rust
impl SlaMap {
    pub fn new() -> Self {
        Self { conversions: Vec::new(), inverted: false }
    }
    
    pub fn add(&mut self, cvt_type: SlaCvtType, args: &[f64]) {
        self.conversions.push(SlaConversion {
            cvt_type,
            args: args.to_vec(),
        });
    }
}
```

- [ ] **Step 3: Implement per-conversion-step transform**

Private method that applies a single conversion step to a 3-vector
(direction cosines):

```rust
fn apply_conversion(cvt: &SlaConversion, v: &mut [f64; 3], forward: bool) {
    let ct = if forward { cvt.cvt_type } else { cvt.cvt_type.inverse() };
    match ct {
        SlaCvtType::Galeq => {
            let (lon, lat) = crate::pal::dcc2s(v);
            let (ra, dec) = crate::pal::galeq(lon, lat);
            *v = crate::pal::dcs2c(ra, dec);
        }
        SlaCvtType::Eqgal => {
            let (lon, lat) = crate::pal::dcc2s(v);
            let (gl, gb) = crate::pal::eqgal(lon, lat);
            *v = crate::pal::dcs2c(gl, gb);
        }
        SlaCvtType::Prec => {
            let ep0 = cvt.args[0];
            let ep1 = cvt.args[1];
            let m = crate::pal::prec(ep0, ep1);
            *v = crate::pal::dmxv(&m, v);
        }
        // ... each conversion type calls the appropriate PAL function
    }
}
```

For inverted conversions: swap the argument order for Prec/Prebn
(swap ep0/ep1), or call the inverse PAL function (Fk45z→Fk54z).

Some conversions like Amp/Map need the `palMappa`/`palAmpqk`/
`palMapqkz` sequence — these operate on 3-vectors via intermediate
apparent-place parameters.

- [ ] **Step 4: Implement Mapping trait**

```rust
impl Mapping for SlaMap {
    fn nin(&self) -> usize { 2 }
    fn nout(&self) -> usize { 2 }
    
    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = forward != self.inverted;
        let npoint = pts.npoint();
        let mut out = PointSet::zeros(npoint, 2);
        
        for ip in 0..npoint {
            let p = pts.point(ip);
            if p[0] == BAD_VALUE || p[1] == BAD_VALUE {
                out.point_mut(ip)[0] = BAD_VALUE;
                out.point_mut(ip)[1] = BAD_VALUE;
                continue;
            }
            
            let mut v = crate::pal::dcs2c(p[0], p[1]);
            
            if fwd {
                for cvt in &self.conversions {
                    Self::apply_conversion(cvt, &mut v, true);
                }
            } else {
                for cvt in self.conversions.iter().rev() {
                    Self::apply_conversion(cvt, &mut v, false);
                }
            }
            
            let (lon, lat) = crate::pal::dcc2s(&v);
            out.point_mut(ip)[0] = lon;
            out.point_mut(ip)[1] = lat;
        }
        Ok(out)
    }
    // ... other trait methods
}
```

- [ ] **Step 5: Implement Serializable**

Class: `"SlaMap"`.

Attributes:
- `Nsla` — number of conversion steps
- `Sla1`..`SlaN` — string code per step
- `Sla1A`, `Sla1B`, ... — double arguments per step (suffix A=0, B=1, C=2...)

```rust
impl Serializable for SlaMap {
    fn class_name(&self) -> &'static str { "SlaMap" }
    
    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new("SlaMap");
        node.class_comment = Some("Sequence of celestial coordinate conversions".to_string());
        serial::write_mapping_base(&mut node, self);
        
        if !self.conversions.is_empty() {
            node.push_int("Nsla", self.conversions.len() as i64,
                Some("Number of conversion steps".to_string()));
        }
        
        let arg_suffix = ['A','B','C','D','E','F','G','H','I','J'];
        for (i, cvt) in self.conversions.iter().enumerate() {
            node.push_string(
                &format!("Sla{}", i + 1),
                cvt.cvt_type.code_string(),
                Some(cvt.cvt_type.description().to_string()),
            );
            for (j, &arg) in cvt.args.iter().enumerate() {
                node.push_double(
                    &format!("Sla{}{}", i + 1, arg_suffix[j]),
                    arg,
                    None,
                );
            }
        }
        Ok(node)
    }
    
    fn from_node(node: &Node) -> AstResult<Self> {
        let (_nin, _nout, inverted) = serial::read_mapping_base(node);
        let nsla = node.get_int("Nsla", 0) as usize;
        let mut sla = SlaMap::new();
        
        let arg_suffix = ['A','B','C','D','E','F','G','H','I','J'];
        for i in 0..nsla {
            let code = node.get_string(&format!("Sla{}", i + 1), "");
            let cvt_type = SlaCvtType::from_code(&code).ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, format!("unknown SlaMap code: {code}"))
            })?;
            let nargs = cvt_type.nargs();
            let mut args = Vec::with_capacity(nargs);
            for j in 0..nargs {
                args.push(node.get_double(
                    &format!("Sla{}{}", i + 1, arg_suffix[j]), 0.0));
            }
            sla.conversions.push(SlaConversion { cvt_type, args });
        }
        
        if inverted { sla.invert(); }
        Ok(sla)
    }
}
```

- [ ] **Step 6: Add to mod.rs and serial dispatch**

Add to `mappings/mod.rs` and update all dispatch points in
`serial/mod.rs`.

- [ ] **Step 7: Write tests**

Create `crates/ast-core/tests/test_slamap.rs`:

```rust
#[test]
fn galactic_to_fk5() {
    // Galactic centre (l=0, b=0) → FK5
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Galeq, &[]);
    let pts = PointSet::from_rows([[0.0, 0.0]]);
    let out = sla.transform(&pts, true).unwrap();
    let ra_deg = out.point(0)[0].to_degrees();
    let dec_deg = out.point(0)[1].to_degrees();
    assert!((ra_deg - 266.4).abs() < 0.2);
    assert!((dec_deg - (-28.94)).abs() < 0.2);
}

#[test]
fn fk5_galactic_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]);
    sla.add(SlaCvtType::Galeq, &[]);
    let pts = PointSet::from_rows([[1.0, 0.5]]);
    let out = sla.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((out.point(0)[1] - 0.5).abs() < 1e-10);
}

#[test]
fn precession_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]);
    let pts = PointSet::from_rows([[1.0, 0.5]]);
    let out = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&out, false).unwrap();
    assert!((back.point(0)[0] - 1.0).abs() < 1e-10);
    assert!((back.point(0)[1] - 0.5).abs() < 1e-10);
}

#[test]
fn inversion_is_identity() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Galeq, &[]);
    sla.add(SlaCvtType::Prec, &[2000.0, 2050.0]);
    let pts = PointSet::from_rows([[2.0, -0.3]]);
    let out = sla.transform(&pts, true).unwrap();
    let back = sla.transform(&out, false).unwrap();
    assert!((back.point(0)[0] - 2.0).abs() < 1e-10);
    assert!((back.point(0)[1] - (-0.3)).abs() < 1e-10);
}

#[test]
fn serialisation_round_trip() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Prec, &[2000.0, 2020.0]);
    sla.add(SlaCvtType::Eqgal, &[]);
    let text = to_native_string(&AnyObject::SlaMap(sla)).unwrap();
    assert!(text.contains("Begin SlaMap"));
    assert!(text.contains("PREC"));
    assert!(text.contains("EQGAL"));
    let restored = from_native_string(&text).unwrap();
    // Verify transform matches
}

#[test]
fn bad_value_propagation() {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Galeq, &[]);
    let pts = PointSet::from_rows([[BAD_VALUE, 0.5]]);
    let out = sla.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);
}
```

Additional tests: FK4↔FK5, multi-step chain, empty SlaMap (identity).

- [ ] **Step 8: Verify and commit**

Run: `cargo test -p ast-core`

```bash
git add crates/ast-core/src/mappings/ crates/ast-core/src/serial/mod.rs crates/ast-core/tests/test_slamap.rs
git commit -m "Add SlaMap (celestial coordinate system conversions via PAL)"
```

---

### Task 5: Clippy, Fmt, Final Verification

**Files:**
- Possibly any file needing fixes

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Fix any issues.

- [ ] **Step 2: Run fmt**

Run: `cargo fmt --all -- --check`

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p ast-core`
Expected: all tests pass

- [ ] **Step 4: Verify dispatch completeness**

Verify WcsMap and SlaMap appear in all dispatch points:
- `AnyObject` enum
- `MappingToNode for dyn Mapping`
- `mapping_from_node`
- `any_object_from_node`
- `to_native_string`
- `as_mapping` / `into_mapping`

- [ ] **Step 5: Commit fixes if any**

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p ast-core` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. PAL wrapper: epoch, spherical, galactic conversions verified
6. wcslib wrapper: TAN, SIN, AIT, CAR round-trip verified
7. WcsMap: projection forward/inverse, multi-axis passthrough, PV params
8. SlaMap: single-step, multi-step, precession, round-trip, inversion
9. Both types serialise and deserialise correctly
10. Both types in AnyObject dispatch
11. `git status` — clean

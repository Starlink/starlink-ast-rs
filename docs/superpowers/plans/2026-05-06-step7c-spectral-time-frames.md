# Step 7c: Spectral and Time Frames Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement SpecMap (34 spectral conversions), TimeMap (28 time conversions), SpecFrame, TimeFrame, FluxFrame, and SpecFluxFrame — completing all astronomy-specific Frame types.

**Architecture:** SpecMap and TimeMap follow the established SlaMap pattern (conversion chain with per-type args). SpecFrame and TimeFrame follow SkyFrame's pattern (Frame trait with system attribute, convert via Map, integrity via map_from). FluxFrame is simple (1-axis passive). SpecFluxFrame is a compound. All use the existing PAL FFI for astronomical calculations, following the C library's algorithmic approach.

**Tech Stack:** Rust 2024 edition. PAL FFI (`crate::pal`). Physical constants from the C header (c = 2.99792458e8, h = 6.6260755e-34).

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/src/pal.rs` | Add ~8 new PAL wrappers (dat, pvobs, rvgalc, rvlg, rvlsrd, rvlsrk, gmst, eqeqx) |
| `crates/ast-core/src/mappings/specmap.rs` | SpecMap + SpecCvtType (34 conversions) |
| `crates/ast-core/src/mappings/timemap.rs` | TimeMap + TimeCvtType (27 conversions) |
| `crates/ast-core/src/mappings/mod.rs` | Re-exports |
| `crates/ast-core/src/frames/specframe.rs` | SpecFrame + SpecSystem + StdOfRest |
| `crates/ast-core/src/frames/timeframe.rs` | TimeFrame + TimeSystem + TimeScale |
| `crates/ast-core/src/frames/fluxframe.rs` | FluxFrame + FluxSystem |
| `crates/ast-core/src/frames/specfluxframe.rs` | SpecFluxFrame (compound) |
| `crates/ast-core/src/frames/mod.rs` | Re-exports |
| `crates/ast-core/src/serial/mod.rs` | Dispatch updates for all 6 types |
| `crates/ast-core/tests/test_specmap.rs` | SpecMap tests |
| `crates/ast-core/tests/test_timemap.rs` | TimeMap tests |
| `crates/ast-core/tests/test_specframe.rs` | SpecFrame tests (ported from testspecframe.c) |
| `crates/ast-core/tests/test_timeframe.rs` | TimeFrame tests (ported from testtime.c) |

---

### Task 1: PAL Additions

**Files:**
- Modify: `crates/ast-core/src/pal.rs`

Add the PAL wrappers needed for SpecMap and TimeMap. Follow the
existing pattern in pal.rs (extern "C" declaration + safe wrapper).

- [ ] **Step 1: Add new PAL wrappers**

New functions to add:

```rust
// Leap seconds: TAI-UTC in seconds for a given MJD
extern "C" { fn palDat(dju: f64) -> f64; }
pub(crate) fn dat(mjd: f64) -> f64 { unsafe { palDat(mjd) } }

// Greenwich Mean Sidereal Time (radians) for a given UT1 MJD
extern "C" { fn palGmst(ut1: f64) -> f64; }
pub(crate) fn gmst(ut1: f64) -> f64 { unsafe { palGmst(ut1) } }

// Equation of the equinoxes (radians) for a given TDB MJD
extern "C" { fn palEqeqx(date: f64) -> f64; }
pub(crate) fn eqeqx(date: f64) -> f64 { unsafe { palEqeqx(date) } }

// Observer velocity (position+velocity 6-vector) for given lat/alt/LST
extern "C" { fn palPvobs(p: f64, h: f64, stl: f64, pv: *mut f64); }
pub(crate) fn pvobs(lat: f64, alt: f64, lst: f64) -> [f64; 6] {
    let mut pv = [0.0; 6];
    unsafe { palPvobs(lat, alt, lst, pv.as_mut_ptr()) };
    pv
}

// Velocity components for standard reference frames
extern "C" { fn palRvgalc(r2000: f64, d2000: f64) -> f64; }
pub(crate) fn rvgalc(ra: f64, dec: f64) -> f64 { unsafe { palRvgalc(ra, dec) } }

extern "C" { fn palRvlg(r2000: f64, d2000: f64) -> f64; }
pub(crate) fn rvlg(ra: f64, dec: f64) -> f64 { unsafe { palRvlg(ra, dec) } }

extern "C" { fn palRvlsrd(r2000: f64, d2000: f64) -> f64; }
pub(crate) fn rvlsrd(ra: f64, dec: f64) -> f64 { unsafe { palRvlsrd(ra, dec) } }

extern "C" { fn palRvlsrk(r2000: f64, d2000: f64) -> f64; }
pub(crate) fn rvlsrk(ra: f64, dec: f64) -> f64 { unsafe { palRvlsrk(ra, dec) } }
```

- [ ] **Step 2: Add PAL tests**

Add tests for the new functions:
- `dat(51544.5)` should return 32.0 (TAI-UTC at J2000.0 = 32 seconds)
- `gmst(51544.5)` should return a value near 0 (GMST at J2000.0 midnight)
- `rvlsrk(0.0, 0.0)` should return a non-zero velocity

- [ ] **Step 3: Verify and commit**

```bash
git add crates/ast-core/src/pal.rs
git commit -m "Add PAL wrappers for spectral/time conversions (dat, gmst, eqeqx, pvobs, rvgalc, rvlg, rvlsrd, rvlsrk)"
```

---

### Task 2: SpecMap

**Files:**
- Create: `crates/ast-core/src/mappings/specmap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Create: `crates/ast-core/tests/test_specmap.rs`

SpecMap is a 1D Mapping chaining spectral conversions. Follows
the SlaMap pattern exactly. Study the C implementation at
`reference/starlink-ast/src/specmap.c` for the conversion formulas.

- [ ] **Step 1: Create specmap.rs with SpecCvtType and SpecMap**

34 conversion types. Physical constants:
`const C_LIGHT: f64 = 2.99792458e8;`
`const H_PLANCK: f64 = 6.6260755e-34;`

Each type has: `nargs() -> usize`, `code_string() -> &'static str`,
`from_code(&str) -> Option<Self>`, `inverse() -> Self`.

The transform applies Doppler shift formulas (study specmap.c
`SystemChange` function at line 2945 and `FrameChange` at line 1373).

For air wavelength (types 9-10): use the IAU 1953 refractive
index formula from the C code.

- [ ] **Step 2: Implement Mapping trait**

`nin() = nout() = 1`. Transform iterates through the conversion
chain applying each step's formula to the input value.

- [ ] **Step 3: Implement Serializable**

Class `"SpecMap"`. Attributes: `Nspec`, `Spec1`..`SpecN` (6-letter
codes like "FRTOVL"), `Spec1a`..`SpecNx` (lowercase letter args).

- [ ] **Step 4: Add to mod.rs and serial dispatch**

- [ ] **Step 5: Write tests**

Test known conversions:
- `c / 5e14 Hz` = 600nm wavelength
- `h * 5e14` = energy in Joules
- Frequency → wavelength → frequency round-trip = identity
- LSRK velocity correction for known RA/Dec

- [ ] **Step 6: Verify and commit**

---

### Task 3: TimeMap

**Files:**
- Create: `crates/ast-core/src/mappings/timemap.rs`
- Modify: `crates/ast-core/src/mappings/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Create: `crates/ast-core/tests/test_timemap.rs`

TimeMap is a 1D Mapping chaining time conversions. Same pattern
as SlaMap/SpecMap. Study `reference/starlink-ast/src/timemap.c`
for the conversion formulas.

- [ ] **Step 1: Create timemap.rs with TimeCvtType and TimeMap**

27 conversion types. Key formulas:
- MJD↔JD: offset of 2400000.5
- MJD↔epoch: via palEpj/palEpb/palEpj2d/palEpb2d
- TAI↔UTC: via palDat (leap seconds)
- TT↔TAI: constant offset 32.184s
- TDB↔TT: relativistic correction (study C line 4200+)
- UT1↔GMST: via palGmst
- GMST↔LMST: longitude offset
- LAST↔LMST: equation of equinoxes via palEqeqx

- [ ] **Step 2: Implement Mapping trait**

`nin() = nout() = 1`. Transform iterates conversion chain.

- [ ] **Step 3: Implement Serializable**

Class `"TimeMap"`. Attributes: `Ntime`, `Time1`..`TimeN`,
`Time1a`..`TimeNx`.

- [ ] **Step 4: Add to dispatch**

- [ ] **Step 5: Write tests**

- MJD 51544.5 → JD 2451545.0
- MJD → Julian epoch → MJD round-trip
- TAI-UTC at J2000 = 32 seconds
- TT-TAI = 32.184 seconds

- [ ] **Step 6: Verify and commit**

---

### Task 4: SpecFrame

**Files:**
- Create: `crates/ast-core/src/frames/specframe.rs`
- Modify: `crates/ast-core/src/frames/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Create: `crates/ast-core/tests/test_specframe.rs`

1-axis Frame for spectral coordinates. Follows SkyFrame pattern.

- [ ] **Step 1: Create specframe.rs with enums and struct**

SpecSystem (10 types), StdOfRest (9 types), SpecFrame struct
with all attributes.

- [ ] **Step 2: Implement Frame trait**

`naxes() = 1`. `convert()` builds SpecMap chain:
1. Convert source system → frequency
2. Apply reference-frame correction (if std_of_rest differs)
3. Convert frequency → target system

`map_from()` produces delta SpecMap when system/rest changes.
Distance is 1D absolute difference. Format is numeric.

- [ ] **Step 3: Implement Serializable**

Class `"SpecFrame"`. Keywords: `SoR`, `AlSoR`, `RefRA`, `RefDec`,
`RstFrq`, `SrcVel`, `SrcVRF`, `SrcSys`, `AlSpOf`, `SpOrg`.

- [ ] **Step 4: Add to dispatch**

- [ ] **Step 5: Write tests (port from testspecframe.c)**

- Convert frequency to wavelength FrameSet
- Standard-of-rest correction
- Rest frequency handling
- Serialisation round-trip

- [ ] **Step 6: Verify and commit**

---

### Task 5: TimeFrame

**Files:**
- Create: `crates/ast-core/src/frames/timeframe.rs`
- Modify: `crates/ast-core/src/frames/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`
- Create: `crates/ast-core/tests/test_timeframe.rs`

1-axis Frame for time coordinates. Follows SkyFrame pattern.

- [ ] **Step 1: Create timeframe.rs with enums and struct**

TimeSystem (4 types), TimeScale (11 types), TimeFrame struct.

- [ ] **Step 2: Implement Frame trait**

`naxes() = 1`. `convert()` builds TimeMap chain:
1. Convert source system to MJD (if not already)
2. Convert source timescale → target timescale
3. Convert MJD to target system (if not MJD)

- [ ] **Step 3: Implement Serializable**

Class `"TimeFrame"`. Keywords: `TmScl`, `ATmScl`, `TmOrg`, `LTOff`.

- [ ] **Step 4: Add to dispatch**

- [ ] **Step 5: Write tests (port from testtime.c)**

- Convert between time scales
- Epoch round-trips
- Serialisation round-trip

- [ ] **Step 6: Verify and commit**

---

### Task 6: FluxFrame and SpecFluxFrame

**Files:**
- Create: `crates/ast-core/src/frames/fluxframe.rs`
- Create: `crates/ast-core/src/frames/specfluxframe.rs`
- Modify: `crates/ast-core/src/frames/mod.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`

- [ ] **Step 1: Create fluxframe.rs**

FluxSystem (4 types), FluxFrame struct. Simple 1-axis Frame.
`convert()` produces scaling between per-frequency and
per-wavelength flux densities.

Serialisation class `"FluxFrame"`. Keywords: `Sys`, `SpecVal`, `DefSV`.

- [ ] **Step 2: Create specfluxframe.rs**

Compound SpecFrame + FluxFrame. Delegates to CmpFrame-style
behaviour with attribute forwarding for SpecFrame attributes.

Serialisation class `"SpecFluxFrame"`.

- [ ] **Step 3: Add all to dispatch**

- [ ] **Step 4: Write tests**

- FluxFrame unit conversions
- SpecFluxFrame compound construction
- Serialisation round-trip

- [ ] **Step 5: Verify and commit**

---

### Task 7: Simplification and Final Verification

**Files:**
- Modify: `crates/ast-core/src/mappings/specmap.rs` (try_merge)
- Modify: `crates/ast-core/src/mappings/timemap.rs` (try_merge)

- [ ] **Step 1: Add try_merge for SpecMap and TimeMap**

Same pattern as SlaMap: inverse cancellation.
SpecMap + inverse(SpecMap) → UnitMap.
TimeMap + inverse(TimeMap) → UnitMap.

- [ ] **Step 2: Run simplification regression**

Verify `spec_*` and `time_*` tests in the simplification
regression suite now pass (they were previously skipped due
to missing types).

- [ ] **Step 3: Run full test suite and clippy**

- [ ] **Step 4: Commit**

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p ast-core` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. SpecMap: all 34 conversion types work, round-trip freq↔wavelength
6. TimeMap: all 27 conversion types work, round-trip MJD↔JD
7. SpecFrame: system conversion, std-of-rest correction
8. TimeFrame: timescale conversion
9. FluxFrame: unit scaling
10. SpecFluxFrame: compound construction
11. Simplification regression: spec_* and time_* tests pass
12. All serialisation round-trips work
13. `git status` — clean

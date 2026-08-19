# Step 7b: SkyFrame + SkyAxis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement SkyFrame (celestial coordinate frame with 12 systems, spherical geometry, sexagesimal formatting, offset coordinates, and FrameSet integrity) and SkyAxis (per-axis HMS/DMS formatting).

**Architecture:** SkyFrame is split across 3 files: the struct + Frame trait impl (`skyframe.rs`), the SlaMap pipeline builder (`sky_convert.rs`), and the axis formatter (`skyaxis.rs`). This keeps each file focused and under ~600 lines. The conversion pipeline uses the existing SlaMap and PAL infrastructure from Step 7a. Observer attributes are added to the Frame trait with default methods.

**Tech Stack:** Rust 2024 edition. Uses `crate::pal` (PAL FFI from Step 7a) and `crate::mappings::SlaMap` (from Step 7a).

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/src/frames/skyaxis.rs` | SkyAxis struct, HMS/DMS format/unformat |
| `crates/ast-core/src/frames/sky_convert.rs` | make_sky_mapping pipeline builder, system conversion chains |
| `crates/ast-core/src/frames/skyframe.rs` | SkyFrame struct, SkySystem, SkyRefIs, Frame trait impl, Serializable |
| `crates/ast-core/src/frames/mod.rs` | Frame trait additions (observer attrs, system), re-exports |
| `crates/ast-core/src/serial/mod.rs` | Dispatch updates for SkyFrame |
| `crates/ast-core/tests/test_skyframe.rs` | SkyFrame tests |

---

### Task 1: SkyAxis — Sexagesimal Formatting

**Files:**
- Create: `crates/ast-core/src/frames/skyaxis.rs`
- Modify: `crates/ast-core/src/frames/mod.rs` (add module declaration)

SkyAxis is the per-axis helper for sky coordinate formatting. It
can be built and tested independently before SkyFrame exists.

- [ ] **Step 1: Create skyaxis.rs with SkyAxis struct**

```rust
//! SkyAxis — per-axis display properties for celestial coordinates.

use crate::error::{AstError, AstResult, ErrorCode};

/// Per-axis properties for a sky coordinate axis.
#[derive(Debug, Clone)]
pub(crate) struct SkyAxis {
    /// Format as time (hours) rather than angle (degrees).
    pub as_time: Option<bool>,
    /// This is a latitude axis (constrains to ±90°).
    pub is_latitude: bool,
    /// Normalize to [-π,π] (true) or [0,2π] (false).
    pub centre_zero: Option<bool>,
}

impl SkyAxis {
    pub fn longitude() -> Self {
        Self { as_time: None, is_latitude: false, centre_zero: None }
    }
    pub fn latitude() -> Self {
        Self { as_time: None, is_latitude: true, centre_zero: None }
    }

    /// Effective as_time: default is true for longitude, false for latitude.
    pub fn effective_as_time(&self) -> bool {
        self.as_time.unwrap_or(!self.is_latitude)
    }
}
```

- [ ] **Step 2: Implement HMS/DMS formatting**

Add to skyaxis.rs:

```rust
/// Format a value in radians as sexagesimal (HMS or DMS).
///
/// `as_time=true`: output `HH:MM:SS.sss` (24h = 2π)
/// `as_time=false`: output `±DD:MM:SS.sss`
/// `precision`: decimal places on the seconds field.
pub(crate) fn format_sky(value: f64, as_time: bool, precision: usize) -> String {
    // ... implementation
}
```

HMS: `value_hours = value * 12.0 / PI`, normalize to [0, 24).
Split into hours, minutes, seconds. Format with specified precision.

DMS: `value_degrees = value * 180.0 / PI`. Preserve sign.
Split into degrees, arcminutes, arcseconds.

Handle BAD_VALUE → `"<bad>"`.

- [ ] **Step 3: Implement HMS/DMS unformatting (parsing)**

```rust
/// Parse a sexagesimal string back to radians.
///
/// Accepts: `HH:MM:SS.ss`, `HH MM SS.ss`, `±DD:MM:SS.ss`,
/// decimal hours/degrees.
/// Returns (value_in_radians, characters_consumed).
pub(crate) fn unformat_sky(text: &str, as_time: bool) -> AstResult<(f64, usize)> {
    // ... implementation
}
```

Parse patterns:
- `[±]DD:MM:SS[.sss]` or `[±]HH:MM:SS[.sss]`
- `[±]DD MM SS[.sss]` (space-separated)
- `[±]DD.ddd` (decimal degrees/hours)
- Leading whitespace skipped.

- [ ] **Step 4: Add module to frames/mod.rs**

Add `pub(crate) mod skyaxis;` to `crates/ast-core/src/frames/mod.rs`.

- [ ] **Step 5: Write unit tests in skyaxis.rs**

Add `#[cfg(test)]` module with tests:
- Format 0.0 rad as HMS → "0:00:00"
- Format π/2 rad as DMS → "+90:00:00"
- Format RA of 6h (π/2 rad) as HMS → "6:00:00"
- Format with precision: 3.14159... rad → specific DMS
- Parse "12:30:00" as time → π/2 + offset
- Parse "-45:30:15.5" as DMS → specific radians
- Round-trip: format → unformat → compare
- BAD_VALUE → "<bad>"

- [ ] **Step 6: Verify and commit**

Run: `cargo test -p ast-core`

```bash
git add crates/ast-core/src/frames/skyaxis.rs crates/ast-core/src/frames/mod.rs
git commit -m "Add SkyAxis with HMS/DMS sexagesimal formatting"
```

---

### Task 2: Frame Trait Additions

**Files:**
- Modify: `crates/ast-core/src/frames/mod.rs`
- Modify: `crates/ast-core/src/frames/basic.rs`
- Modify: `crates/ast-core/src/frames/cmpframe.rs`
- Modify: `crates/ast-core/src/frames/frameset.rs`

Add observer attributes and system accessor to the Frame trait.

- [ ] **Step 1: Add default methods to Frame trait**

In `crates/ast-core/src/frames/mod.rs`, add to the Frame trait:

```rust
pub trait Frame: AsAny + Send + fmt::Debug {
    // ... existing methods ...

    /// Observer geodetic latitude in radians.
    fn obs_lat(&self) -> Option<f64> { None }
    /// Observer geodetic longitude in radians.
    fn obs_lon(&self) -> Option<f64> { None }
    /// Observer altitude in metres.
    fn obs_alt(&self) -> Option<f64> { None }
    /// TAI-UTC in seconds.
    fn dtai(&self) -> Option<f64> { None }
    /// UT1-UTC in seconds.
    fn dut1(&self) -> Option<f64> { None }
    /// Coordinate system name (e.g., "FK5", "GALACTIC").
    fn system_string(&self) -> Option<&str> { None }
}
```

These have defaults returning None, so BasicFrame and CmpFrame
don't need changes. FrameSet must delegate to current frame.

- [ ] **Step 2: Update FrameSet to delegate new methods**

In `crates/ast-core/src/frames/frameset.rs`, add to the
`impl Frame for FrameSet` block:

```rust
fn obs_lat(&self) -> Option<f64> { self.current().obs_lat() }
fn obs_lon(&self) -> Option<f64> { self.current().obs_lon() }
fn obs_alt(&self) -> Option<f64> { self.current().obs_alt() }
fn dtai(&self) -> Option<f64> { self.current().dtai() }
fn dut1(&self) -> Option<f64> { self.current().dut1() }
fn system_string(&self) -> Option<&str> { self.current().system_string() }
```

- [ ] **Step 3: Verify and commit**

Run: `cargo test -p ast-core`

```bash
git add crates/ast-core/src/frames/
git commit -m "Add observer attributes and system_string to Frame trait"
```

---

### Task 3: Sky Conversion Pipeline (make_sky_mapping)

**Files:**
- Create: `crates/ast-core/src/frames/sky_convert.rs`
- Modify: `crates/ast-core/src/frames/mod.rs` (add module)

This is the core logic that builds SlaMap chains for coordinate
system conversion. It can be tested independently using SlaMap
transforms.

- [ ] **Step 1: Create sky_convert.rs with SkySystem and SkyRefIs enums**

```rust
//! Sky coordinate system conversion pipeline.

use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MatrixMap, SlaMap, SlaCvtType, UnitMap,
};

/// Celestial coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkySystem {
    Fk4, Fk4NoE, Fk5, Gappt, Ecliptic, Galactic,
    Supergalactic, Icrs, J2000, Helioecliptic, Azel, Unknown,
}

/// How offset coordinates relate to the standard system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyRefIs {
    Origin,
    Pole,
    Ignored,
}
```

Add methods: `SkySystem::from_str`, `as_str`, `needs_equinox`,
`needs_epoch`, `default_equinox_mjd`.
Add methods: `SkyRefIs::from_str`, `as_str`.

- [ ] **Step 2: Implement system-to-FK5 conversion builder**

Private function that adds SlaMap steps to convert from a given
system to FK5 J2000.0:

```rust
fn add_system_to_fk5(
    sla: &mut SlaMap,
    system: SkySystem,
    equinox_mjd: f64,
    epoch_mjd: f64,
    obs_lat: f64,
    last: f64,
) {
    match system {
        SkySystem::Fk5 => {
            let eq_jep = crate::pal::epj(equinox_mjd);
            if (eq_jep - 2000.0).abs() > 1e-10 {
                sla.add(SlaCvtType::Prec, &[eq_jep, 2000.0]);
            }
        }
        SkySystem::Fk4 => {
            let eq_bep = crate::pal::epb(equinox_mjd);
            sla.add(SlaCvtType::Subet, &[eq_bep]);
            sla.add(SlaCvtType::Prebn, &[eq_bep, 1950.0]);
            sla.add(SlaCvtType::Addet, &[1950.0]);
            sla.add(SlaCvtType::Fk45z, &[1950.0]);
        }
        SkySystem::Galactic => {
            sla.add(SlaCvtType::Galeq, &[]);
        }
        // ... all 12 systems
    }
}
```

And the reverse:

```rust
fn add_fk5_to_system(sla: &mut SlaMap, system: SkySystem, ...) {
    // Reverse chains
}
```

- [ ] **Step 3: Implement make_sky_mapping**

The public entry point:

```rust
pub(crate) fn make_sky_mapping(
    from_sys: SkySystem, from_eq_mjd: f64, from_ep_mjd: f64,
    to_sys: SkySystem, to_eq_mjd: f64, to_ep_mjd: f64,
    align_sys: SkySystem,
    from_obs_lat: f64, from_last: f64,
    to_obs_lat: f64, to_last: f64,
) -> AstResult<DynMapping>
```

Builds the 4-step pipeline:
1. from_system → FK5 J2000.0 (using from epoch/equinox)
2. FK5 J2000.0 → align_sys (using from epoch)
3. align_sys → FK5 J2000.0 (using to epoch)
4. FK5 J2000.0 → to_system (using to epoch/equinox)

Compose via CmpMap(Series). If result is effectively identity,
return UnitMap.

- [ ] **Step 4: Add module to mod.rs**

Add `pub(crate) mod sky_convert;` and re-export:
`pub use sky_convert::{SkySystem, SkyRefIs};`

- [ ] **Step 5: Write tests**

Tests in `#[cfg(test)]` module of sky_convert.rs:

- FK5→Galactic: galactic centre coords
- FK5→FK5 same equinox: UnitMap-like result
- FK5→FK5 different equinox: precession
- FK4→FK5: known star
- ICRS→FK5: near-identity
- Galactic→Supergalactic→Galactic round-trip
- Ecliptic→FK5
- All 12 systems: round-trip (system→FK5→system) preserves coords

- [ ] **Step 6: Verify and commit**

```bash
git add crates/ast-core/src/frames/sky_convert.rs crates/ast-core/src/frames/mod.rs
git commit -m "Add make_sky_mapping: SlaMap pipeline for coordinate system conversion"
```

---

### Task 4: SkyFrame Struct and Basic Frame Impl

**Files:**
- Create: `crates/ast-core/src/frames/skyframe.rs`
- Modify: `crates/ast-core/src/frames/mod.rs`

SkyFrame struct with all attributes, constructor, setters, and the
Frame trait implementation (spherical geometry, formatting, norm,
map_from, convert).

- [ ] **Step 1: Create skyframe.rs with struct and constructor**

Define SkyFrame with all fields from the spec. Constructor
`SkyFrame::new(system: SkySystem) -> Self` with defaults.

- [ ] **Step 2: Implement attribute accessors**

All the getters/setters: `system()`, `set_system()`, `equinox()`,
`set_equinox()`, `epoch()`, `set_epoch()`, `sky_ref()`,
`set_sky_ref()`, `sky_ref_is()`, `set_sky_ref_is()`, observer
attrs, etc.

Hot-path setters are O(1) field writes.

- [ ] **Step 3: Implement Frame trait — spherical geometry**

```rust
impl Frame for SkyFrame {
    fn naxes(&self) -> usize { 2 }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        // Great-circle distance via pal::dsep
        Ok(crate::pal::dsep(a[0], a[1], b[0], b[1]))
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        // Bearing difference at point b
        let ab = crate::pal::dbear(a[0], a[1], b[0], b[1]);
        let cb = crate::pal::dbear(c[0], c[1], b[0], b[1]);
        Ok(crate::pal::drange(ab - cb))
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        // 3-vector rotation along great circle
        // ... using pal::dcs2c, pal::dvxv, pal::dav2m, pal::dmxv, pal::dcc2s
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        // Longitude wrap, latitude fold through poles
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        // Delegate to skyaxis::format_sky
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        // Delegate to skyaxis::unformat_sky
    }

    fn snapshot(&self) -> DynFrame { Box::new(self.clone()) }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        // Downcast to SkyFrame, call make_sky_mapping
    }

    fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        // If to is SkyFrame, build mapping via make_sky_mapping
    }

    // Observer attributes
    fn obs_lat(&self) -> Option<f64> { self.obs_lat }
    fn obs_lon(&self) -> Option<f64> { self.obs_lon }
    fn obs_alt(&self) -> Option<f64> { self.obs_alt }
    fn dtai(&self) -> Option<f64> { self.dtai }
    fn dut1(&self) -> Option<f64> { self.dut1 }
    fn system_string(&self) -> Option<&str> { Some(self.system.as_str()) }
}
```

- [ ] **Step 4: Add to mod.rs**

```rust
mod skyframe;
pub use skyframe::SkyFrame;
```

(SkySystem and SkyRefIs already re-exported from sky_convert.)

- [ ] **Step 5: Verify and commit**

```bash
git add crates/ast-core/src/frames/
git commit -m "Add SkyFrame struct with Frame trait implementation"
```

---

### Task 5: SkyFrame Serialisation and Dispatch

**Files:**
- Modify: `crates/ast-core/src/frames/skyframe.rs`
- Modify: `crates/ast-core/src/serial/mod.rs`

- [ ] **Step 1: Implement Serializable for SkyFrame**

Class: `"SkyFrame"`. Write Frame base attributes (Naxes, System,
Epoch, Domain, Title, observer attrs, SkyAxis children, IsA Frame),
then SkyFrame-specific: Proj, NegLon, SkyTol, Eqnox, SRefIs,
SRef1, SRef2, SRefP1, SRefP2, AlOff.

Key details:
- `System` is written by the Frame base logic (via `system_string()`)
- `Eqnox` is a decimal epoch (Besselian if < 1984, Julian otherwise),
  NOT MJD. Convert using `pal::epj`/`pal::epb`.
- Axis children use class `"SkyAxis"` not `"Axis"`

`from_node`: read System string → SkySystem, read Eqnox (decimal
epoch → MJD), read all SkyFrame attrs, read SkyAxis children.

- [ ] **Step 2: Implement Serializable for SkyAxis**

In skyaxis.rs, add `to_node`/`from_node` for class `"SkyAxis"`.
Attributes: IsLat, CnZer, AsTime.

- [ ] **Step 3: Update serial dispatch**

In `serial/mod.rs`:
- Add SkyFrame to `FrameToNode` downcast
- Add `"SkyFrame"` to `frame_from_node`
- Add `SkyFrame(SkyFrame)` to `AnyObject` enum
- Add `"SkyFrame"` to `any_object_from_node`
- Update `to_native_string`, `as_frame`, `into_frame`
- Update `"SkyAxis"` handling in frame serialisation (when reading
  Axis children, check class name and create SkyAxis or plain Axis)

- [ ] **Step 4: Verify and commit**

```bash
git add crates/ast-core/src/frames/ crates/ast-core/src/serial/mod.rs
git commit -m "Add SkyFrame serialisation and dispatch"
```

---

### Task 6: Offset Coordinate Rotation

**Files:**
- Modify: `crates/ast-core/src/frames/sky_convert.rs`
- Modify: `crates/ast-core/src/frames/skyframe.rs`

When `sky_ref_is != Ignored`, SkyFrame prepends/appends a rotation
to the conversion mapping.

- [ ] **Step 1: Implement offset rotation mapping builder**

In sky_convert.rs, add:

```rust
pub(crate) fn make_offset_mapping(
    sky_ref: [f64; 2],
    sky_ref_p: [f64; 2],
    sky_ref_is: SkyRefIs,
) -> AstResult<Option<DynMapping>>
```

For `SkyRefIs::Origin`: build a 3×3 rotation MatrixMap that
rotates sky_ref to the pole (0, π/2), using sky_ref_p to define
the meridian orientation.

For `SkyRefIs::Pole`: sky_ref IS the pole — build rotation that
moves the standard pole to sky_ref.

For `SkyRefIs::Ignored`: return None.

The rotation is computed from:
1. Unit vectors for sky_ref and sky_ref_p (via `pal::dcs2c`)
2. Cross products and normalisation to build the rotation matrix
3. Wrap in MatrixMap

- [ ] **Step 2: Integrate offsets into make_sky_mapping**

Update `make_sky_mapping` (or the SkyFrame::map_from / convert
callers) to prepend the source's inverse offset rotation and
append the target's offset rotation to the pipeline.

- [ ] **Step 3: Write tests**

- SkyRefIs::Origin with known reference point, verify transform
- SkyRefIs::Pole, verify transform
- Offset round-trip: set offset, convert, verify coordinates

- [ ] **Step 4: Verify and commit**

```bash
git add crates/ast-core/src/frames/
git commit -m "Add offset coordinate rotation support for SkyFrame"
```

---

### Task 7: Comprehensive SkyFrame Tests

**Files:**
- Create: `crates/ast-core/tests/test_skyframe.rs`

- [ ] **Step 1: Write test file**

Comprehensive tests covering:

**Construction & attributes:**
- Create SkyFrame with each system, verify defaults
- Set/get equinox, epoch, sky_ref, observer attrs

**Coordinate system conversion:**
- FK5→Galactic via convert(): galactic centre ≈ (266.4°, -28.9°)
- FK5→ICRS: near-identity for modern data
- FK4→FK5: known star with epoch
- Ecliptic→FK5
- All 12 systems: round-trip preserves coordinates to < 1e-8 rad
- Epoch change: precess FK5 between epochs

**Spherical geometry:**
- Distance: poles are π apart
- Distance: equatorial points at known separation
- Angle: right angle on sphere
- Offset: walk 1 radian along great circle
- Norm: longitude wrapping, latitude pole-folding, neglon mode

**Formatting:**
- Format RA=0 as HMS → "0:00:00..."
- Format Dec=π/4 as DMS → "+45:00:00..."
- Parse "12:30:00" as HMS → correct radians
- Format/unformat round-trip

**FrameSet integrity:**
- Create FrameSet with SkyFrame current, BasicFrame base, ZoomMap
- Change epoch via with_integrity
- Verify transform output changes (mapping updated)

**Offset coordinates:**
- SkyRefIs::Origin, verify shifted origin
- SkyRefIs::Ignored, verify no effect

**Serialisation:**
- to_native_string → from_native_string round-trip
- Verify System, Equinox, SkyRef preserved
- Verify SkyAxis children have class "SkyAxis"
- Parse reference .ast files containing SkyFrame

- [ ] **Step 2: Run tests**

Run: `cargo test -p ast-core --test test_skyframe`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/tests/test_skyframe.rs
git commit -m "Add comprehensive SkyFrame tests"
```

---

### Task 8: Clippy, Fmt, Final Verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`

- [ ] **Step 2: Run fmt**

Run: `cargo fmt --all -- --check`

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p ast-core`
Expected: all tests pass

- [ ] **Step 4: Verify dispatch completeness**

Verify SkyFrame appears in all dispatch points.

- [ ] **Step 5: Commit fixes if any**

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p ast-core` — all tests pass
3. `cargo clippy --workspace -- -D warnings` — clean
4. `cargo fmt --all -- --check` — clean
5. SkyFrame construction with all 12 systems
6. Coordinate conversion: FK5↔Galactic, FK4↔FK5, all systems round-trip
7. Spherical geometry: distance, angle, offset, norm
8. HMS/DMS formatting and parsing
9. FrameSet integrity: epoch change updates mapping
10. Offset coordinates: Origin and Pole modes
11. Serialisation round-trip preserves all attributes
12. Observer attributes on Frame trait
13. `git status` — clean

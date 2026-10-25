# Step 7b: SkyFrame + SkyAxis — Design Spec

## Summary

Implement SkyFrame — the celestial coordinate frame — and SkyAxis
(its specialized axis type). SkyFrame is the most complex class in
the AST library: it handles 12 coordinate systems, builds SlaMap
conversion pipelines, provides spherical geometry, sexagesimal
formatting, offset coordinate support, and powers the FrameSet
integrity mechanism for per-time-slice attribute mutation.

This step also adds observer attributes (ObsLat, ObsLon, ObsAlt,
Dtai, Dut1) to the Frame trait.

## Deliverables

1. `SkyFrame` struct implementing the `Frame` trait.
2. `SkyAxis` internal helper for per-axis display/formatting.
3. `SkySystem` enum (12 coordinate systems).
4. `SkyRefIs` enum (offset coordinate modes).
5. `make_sky_mapping` — the SlaMap pipeline builder.
6. Spherical geometry: great-circle distance, bearing, geodesic
   offset, coordinate normalization.
7. Sexagesimal formatting (HMS/DMS) and parsing.
8. Offset coordinate rotation mappings.
9. Observer attributes on the Frame trait.
10. Serializable impl + dispatch updates.
11. Tests including FrameSet integrity validation.

## Module Structure

```
crates/starlink-ast/src/frames/
├── skyframe.rs       # SkyFrame struct, SkySystem, SkyRefIs
├── skyaxis.rs        # SkyAxis, HMS/DMS formatting
├── sky_convert.rs    # make_sky_mapping pipeline builder
├── mod.rs            # updated re-exports, Frame trait additions
```

## SkySystem Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkySystem {
    Fk4,
    Fk4NoE,
    Fk5,
    Gappt,
    Ecliptic,
    Galactic,
    Supergalactic,
    Icrs,
    J2000,
    Helioecliptic,
    Azel,
    Unknown,
}
```

Methods:
- `from_str(s: &str) -> Option<Self>` — case-insensitive parse
  ("FK5", "GALACTIC", "ICRS", etc.)
- `as_str(&self) -> &'static str` — canonical string form
- `needs_equinox(&self) -> bool` — true for FK4, FK4_NO_E, FK5,
  ECLIPTIC
- `needs_epoch(&self) -> bool` — true for all except UNKNOWN
- `default_equinox_mjd(&self) -> f64` — J2000.0 for FK5/ECLIPTIC,
  B1950.0 for FK4

## SkyRefIs Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyRefIs {
    Origin,   // SkyRef is the origin of the offset system
    Pole,     // SkyRef is the north pole of the offset system
    Ignored,  // No offset (standard system coordinates)
}
```

Methods:
- `from_str(s: &str) -> Option<Self>` — "Origin", "Pole", "Ignored"
- `as_str(&self) -> &'static str`

## SkyFrame Struct

```rust
pub struct SkyFrame {
    // Frame-level attributes
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>,           // MJD

    // Observer attributes (on Frame trait in C)
    obs_lat: Option<f64>,         // geodetic latitude (radians)
    obs_lon: Option<f64>,         // geodetic longitude (radians)
    obs_alt: Option<f64>,         // altitude (metres)
    dtai: Option<f64>,            // TAI-UTC (seconds)
    dut1: Option<f64>,            // UT1-UTC (seconds)

    // SkyFrame-specific
    system: SkySystem,
    equinox: Option<f64>,         // MJD of mean equinox
    projection: Option<String>,
    neglon: bool,
    sky_tol: Option<f64>,         // smallest significant distance (rad)
    sky_ref: [f64; 2],            // offset origin/pole (lon, lat in rad)
    sky_ref_p: [f64; 2],          // offset meridian point (lon, lat)
    sky_ref_is: SkyRefIs,
    align_offset: bool,

    // Per-axis properties
    axes: [SkyAxis; 2],           // [longitude, latitude]

    // Cached values (not serialised, recomputed on demand)
    last_cache: Option<f64>,      // local apparent sidereal time
    eplast_cache: Option<f64>,    // epoch used for LAST cache
}
```

### Constructor

`SkyFrame::new(system: SkySystem) -> Self` — creates a 2-axis
SkyFrame with defaults:
- epoch: None (defaults to J2000.0 MJD when accessed)
- equinox: None (defaults per system)
- sky_ref_is: Ignored
- sky_ref/sky_ref_p: [0, 0]
- neglon: false
- axes: [SkyAxis::longitude(), SkyAxis::latitude()]

### Attribute Accessors

Standard `&mut self` setters following the project convention:

```rust
pub fn set_system(&mut self, sys: SkySystem);
pub fn system(&self) -> SkySystem;
pub fn set_equinox(&mut self, eq: f64);  // MJD
pub fn equinox(&self) -> f64;            // MJD, returns default if unset
pub fn set_neglon(&mut self, neg: bool);
pub fn set_sky_ref(&mut self, lon: f64, lat: f64);
pub fn sky_ref(&self) -> [f64; 2];
pub fn set_sky_ref_is(&mut self, mode: SkyRefIs);
pub fn sky_ref_is(&self) -> SkyRefIs;
pub fn set_sky_ref_p(&mut self, lon: f64, lat: f64);
pub fn set_sky_tol(&mut self, tol: f64);
pub fn set_align_offset(&mut self, ao: bool);
pub fn set_projection(&mut self, proj: &str);
// Observer attrs
pub fn set_obs_lat(&mut self, lat: f64);
pub fn set_obs_lon(&mut self, lon: f64);
pub fn set_obs_alt(&mut self, alt: f64);
pub fn set_dtai(&mut self, dtai: f64);
pub fn set_dut1(&mut self, dut1: f64);
```

**Hot-path**: `set_epoch`, `set_sky_ref_is`, `set_system`,
`set_equinox` are all single field writes — O(1), no allocation,
no locking. The integrity cost is paid by FrameSet's
`with_integrity()`, not by the setter itself.

## SkyAxis

```rust
#[derive(Debug, Clone)]
pub(crate) struct SkyAxis {
    as_time: Option<bool>,
    is_latitude: bool,
    centre_zero: Option<bool>,
}
```

### Constructors

```rust
impl SkyAxis {
    pub fn longitude() -> Self {
        Self { as_time: None, is_latitude: false, centre_zero: None }
    }
    pub fn latitude() -> Self {
        Self { as_time: None, is_latitude: true, centre_zero: None }
    }
}
```

Default `as_time`: true for longitude (hours), false for latitude
(degrees). Default `centre_zero`: false for longitude, true for
latitude (but overridden by SkyFrame's `neglon` attribute).

### Sexagesimal Formatting

`format(value: f64, as_time: bool, precision: usize) -> String`

- `as_time=true`: Convert radians to hours (value * 12/π), format
  as `HH:MM:SS.ss` with `precision` decimal places on seconds.
- `as_time=false`: Convert radians to degrees (value * 180/π),
  format as `±DD:MM:SS.ss`.
- Negative values: prefix with `-` for latitude/DMS; for HMS,
  normalize to [0, 24h).

`unformat(text: &str, as_time: bool) -> AstResult<(f64, usize)>`

- Parse `HH:MM:SS.ss` or `±DD:MM:SS.ss` back to radians.
- Also accept `HH MM SS.ss`, `DDdMMmSS.ss`, decimal degrees/hours.
- Return (value_in_radians, characters_consumed).

### Serialization

Class: `"SkyAxis"`. Attributes:
- `IsLat` (if set)
- `CnZer` (if set)
- `AsTime` (if set)

## MakeSkyMapping — Conversion Pipeline

`crates/starlink-ast/src/frames/sky_convert.rs`

The core function that builds the SlaMap conversion chain between
two SkyFrames:

```rust
pub(crate) fn make_sky_mapping(
    from: &SkyFrame,
    to: &SkyFrame,
    align_sys: SkySystem,
) -> AstResult<DynMapping>
```

### Pipeline Architecture

```
from.system (from.epoch, from.equinox)
    → [Step 1: from.system → FK5 J2000.0]
    → [Step 2: FK5 J2000.0 → align_sys (from.epoch)]
    → [Step 3: align_sys (to.epoch) → FK5 J2000.0]
    → [Step 4: FK5 J2000.0 → to.system (to.epoch, to.equinox)]
```

Each step is an SlaMap with the appropriate conversion chain.

### System-to-FK5 Conversion Chains

| System | SlaMap steps (system → FK5 J2000.0) |
|--------|--------------------------------------|
| FK5 | Prec(equinox_jep, 2000.0) if equinox != J2000 |
| FK4 | Subet(equinox_bep) → Prebn(equinox_bep, 1950.0) → Addet(1950.0) → Fk45z(1950.0) |
| FK4_NO_E | Prebn(equinox_bep, 1950.0) → Addet(1950.0) → Fk45z(1950.0) |
| ICRS | Hfk5z(epoch_jep) |
| J2000 | special (J2000 dynamical ≈ ICRS via frame tie) → Hfk5z(epoch_jep) |
| GALACTIC | Galeq |
| SUPERGALACTIC | Supgal → Galeq |
| ECLIPTIC | Ecleq(equinox_mjd) |
| HELIOECLIPTIC | Ecleq(epoch_mjd) |
| GAPPT | Amp(2000.0, epoch_mjd) |
| AZEL | Dh2e(obs_lat) → (hour angle → RA via LAST) → Amp(2000.0, epoch_mjd) |
| UNKNOWN | UnitMap |

The FK5-to-system direction reverses the chain.

### Optimizations

- If `from.system == to.system` and `from.epoch == to.epoch` and
  `from.equinox == to.equinox`: return UnitMap.
- Steps 2+3 collapse if epochs are equal or if align_sys == FK5
  (because FK5 J2000.0 is the pivot).
- If either system matches align_sys: that step is skipped.
- The final result is simplified via CmpMap composition.

### Offset Coordinate Rotation

When `sky_ref_is != Ignored`, prepend/append rotation mappings:

For the source frame (if sky_ref_is != Ignored):
- Build a rotation CmpMap that maps from offset coords to standard
  coords (or vice versa) based on sky_ref, sky_ref_p, and the mode
  (Origin or Pole).
- The rotation uses `MatrixMap` with a 3×3 rotation matrix computed
  from the reference point.

### LAST Computation (for AZEL system)

Local Apparent Sidereal Time computed from:
- epoch (MJD), obs_lon, dtai, dut1
- Uses PAL functions for UT1→GMST→GAST→LAST conversion

Cached on the SkyFrame (`last_cache`, `eplast_cache`) and
invalidated when epoch changes.

## Frame Trait Implementation

### Spherical Geometry

- **distance**: `pal::dsep(lon1, lat1, lon2, lat2)` — great-circle
- **angle**: `pal::dbear(lon_a, lat_a, lon_b, lat_b)` minus
  `pal::dbear(lon_c, lat_c, lon_b, lat_b)` — angle at B
- **offset**: 3-vector rotation via cross product + `pal::dav2m`
  + `pal::dmxv` → geodesic walk along great circle
- **norm**: longitude wrap to [0, 2π] or [-π, π], latitude fold
  through poles

### map_from (Integrity Mechanism)

```rust
fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
    // Downcast previous to SkyFrame
    let prev = previous.as_any().downcast_ref::<SkyFrame>()
        .ok_or_else(|| ...)?;
    make_sky_mapping(prev, self, self.system)
}
```

When a SkyFrame attribute changes inside a FrameSet (via
`with_integrity`), this produces the delta mapping that compensates
all connected edges.

### convert

```rust
fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
    let to_sky = to.as_any().downcast_ref::<SkyFrame>();
    if let Some(to_sky) = to_sky {
        let mapping = make_sky_mapping(self, to_sky, SkySystem::Fk5)?;
        let mut fs = FrameSet::new(self.snapshot());
        fs.add_frame(FrameRef::Base, mapping, to.snapshot())?;
        Ok(Some(fs))
    } else {
        Ok(None)
    }
}
```

### Observer Attributes on Frame Trait

Add default methods to the `Frame` trait:

```rust
pub trait Frame: AsAny + Send + fmt::Debug {
    // ... existing methods ...
    fn obs_lat(&self) -> Option<f64> { None }
    fn obs_lon(&self) -> Option<f64> { None }
    fn obs_alt(&self) -> Option<f64> { None }
    fn dtai(&self) -> Option<f64> { None }
    fn dut1(&self) -> Option<f64> { None }
}
```

FrameSet delegates to current frame. BasicFrame and CmpFrame return
None (defaults).

## Serialization

### SkyFrame

Class: `"SkyFrame"`.

Frame base attributes first (Naxes, System, Epoch, Domain, Title,
observer attrs, per-axis SkyAxis children, IsA Frame), then:

| Key | Condition | Comment |
|-----|-----------|---------|
| `Proj` | if set | "Description of sky projection" |
| `NegLon` | if set | "Display negative longitude values" |
| `SkyTol` | if set | "Smallest significant distance (arc-sec)" |
| `Eqnox` | if set | Equinox as Besselian/Julian epoch decimal year |
| `SRefIs` | if set | "Pole" / "Origin" / "Ignored" |
| `SRef1` | if set | "Ref. pos. longitude" |
| `SRef2` | if set | "Ref. pos. latitude" |
| `SRefP1` | if set | "Ref. pos. P longitude" |
| `SRefP2` | if set | "Ref. pos. P latitude" |
| `AlOff` | if set | "Align in offset coords" |

Note: `Eqnox` is stored as a decimal epoch (Besselian if < 1984.0,
Julian otherwise), not as MJD. Conversion uses `pal::epj`/`pal::epb`.

### SkyAxis

Class: `"SkyAxis"` (vs plain `"Axis"` for BasicFrame).

| Key | Condition | Comment |
|-----|-----------|---------|
| `IsLat` | if set | "Latitude axis?" |
| `CnZer` | if set | "Center zero?" |
| `AsTime` | if set | "Format as time?" |

## Frame Trait Additions for Serialization

The Frame serialization needs to know the System attribute. Add to
the Frame trait:

```rust
pub trait Frame: AsAny + Send + fmt::Debug {
    // ... existing ...
    fn system(&self) -> Option<&str> { None }
}
```

SkyFrame returns `Some(self.system.as_str())`. BasicFrame and
CmpFrame return None.

## Testing Strategy

### SkyFrame Construction and Attributes
- Create with each system, verify defaults
- Set/get equinox, epoch, sky_ref, sky_ref_is
- Observer attributes

### Coordinate System Conversion (12 systems)
- FK5→Galactic: galactic center known coordinates
- FK5→ICRS: near-identity for modern data
- FK4→FK5: known star with epoch
- Ecliptic→FK5
- All 12 systems: round-trip through FK5 preserves coordinates
- Epoch change: precess FK5 between epochs, verify non-identity
  but small delta

### Spherical Geometry
- Distance: poles are π apart, equatorial points
- Angle: right angle on the sphere
- Offset: walk along great circle known distance
- Norm: longitude wrapping, latitude pole-folding, neglon mode

### Sexagesimal Formatting
- Format RA in HMS
- Format Dec in DMS
- Parse HMS and DMS back to radians
- Round-trip format→unformat

### FrameSet Integrity
- Create FrameSet with SkyFrame as current
- Change epoch via with_integrity
- Verify mapping updates (transform changes)
- Hot-path: set_epoch is O(1) field write

### Offset Coordinates
- SkyRefIs::Origin with known reference point
- SkyRefIs::Pole with known pole
- Round-trip through offset system

### Serialization
- to_native_string → from_native_string round-trip
- Verify System, Equinox, SkyRef preserved
- Parse reference .ast files containing SkyFrame

## What This Step Does NOT Include

- SpecFrame, TimeFrame, FluxFrame (Step 7c)
- SpecMap, TimeMap
- Pure Rust wcslib port (Step 6a)
- FitsChan / Channel (Step 8)

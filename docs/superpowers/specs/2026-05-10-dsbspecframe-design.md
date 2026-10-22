# DSBSpecFrame Implementation Design

## Overview

DSBSpecFrame is a specialization of SpecFrame for dual-sideband (heterodyne)
instruments. Each observed spectral point contains contributions from two
frequencies — one from the Lower Side Band (LSB) and one from the Upper Side
Band (USB) — equidistant on either side of a fixed Local Oscillator (LO)
frequency.

**C reference:** `reference/starlink-ast/src/dsbspecframe.c` (3266 lines).

## Approach: Composition with Delegation

DSBSpecFrame wraps a `SpecFrame` and implements the `Frame` trait by
delegating most methods, overriding `convert`, `map_from`, `snapshot`,
`equal`, and the domain/label defaults.

## Struct and Attributes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SideBand {
    USB,  //  1 in C
    LSB,  // -1 in C
    LO,   //  0 in C
}

#[derive(Debug, Clone)]
pub struct DSBSpecFrame {
    spec: SpecFrame,
    dsb_centre: Option<f64>,      // topocentric frequency in Hz
    ifr: Option<f64>,             // intermediate frequency in Hz
    sideband: Option<SideBand>,   // None = derive from IF sign
    align_sideband: Option<bool>, // None = default false
}
```

### Defaults (matching C)

| Attribute | Default | Notes |
|-----------|---------|-------|
| dsb_centre | 3.0e10 Hz (30 GHz) | Stored as topocentric Hz |
| ifr | 4.0e9 Hz (4 GHz) | Stored as topocentric Hz |
| sideband | Derived from IF sign | Positive IF → LSB, else USB |
| align_sideband | false | Sideband-aware alignment is opt-in |
| domain | "DSBSPECTRUM" | Overrides SpecFrame's "SPECTRUM" |

### ImagFreq (computed, read-only)

The image-sideband frequency corresponding to the rest frequency. Computed by
building a temporary mapping from the observed sideband to the image sideband
and transforming the rest frequency through it.

## Public API

```rust
impl DSBSpecFrame {
    pub fn new(system: SpecSystem) -> Self;

    // DSB-specific attributes
    pub fn dsb_centre(&self) -> f64;            // topo Hz (default 3e10)
    pub fn set_dsb_centre(&mut self, hz: f64);
    pub fn ifr(&self) -> f64;                   // topo Hz (default 4e9)
    pub fn set_ifr(&mut self, hz: f64);
    pub fn sideband(&self) -> SideBand;         // resolved (never None to caller)
    pub fn set_sideband(&mut self, sb: SideBand);
    pub fn align_sideband(&self) -> bool;
    pub fn set_align_sideband(&mut self, val: bool);
    pub fn imag_freq(&self) -> AstResult<f64>;  // computed, can fail

    // SpecFrame delegators
    pub fn system(&self) -> SpecSystem;
    pub fn set_system(&mut self, sys: SpecSystem);
    pub fn std_of_rest(&self) -> StdOfRest;
    pub fn set_std_of_rest(&mut self, sor: StdOfRest);
    pub fn rest_freq(&self) -> Option<f64>;
    pub fn set_rest_freq(&mut self, hz: f64);
    pub fn set_epoch(&mut self, mjd: f64);
    pub fn set_obs_lat(&mut self, lat: f64);
    pub fn set_obs_lon(&mut self, lon: f64);
    pub fn set_obs_alt(&mut self, alt: f64);
    // ... other SpecFrame delegators as needed

    // Access inner SpecFrame
    pub fn spec(&self) -> &SpecFrame;
    pub fn spec_mut(&mut self) -> &mut SpecFrame;
}
```

### Sideband resolution semantics

`sideband()` resolves the default: if unset, returns LSB when IF > 0, USB
otherwise (matching C line 2591). `set_sideband` stores the concrete value.
The C also accepts "observed"/"image" string forms — those resolve at set-time
in the CAPI string dispatch layer only.

## Sideband Conversion Algorithm

All conversions operate in topocentric frequency space.

### Fundamental relationships

- `f_lo = dsb_centre + ifr`
- `f_lsb = 2 * f_lo - f_usb` (reflection around f_lo)
- LO offset: `value = f_usb - f_lo` (or `f_lo - f_lsb`)

### Private methods

```rust
impl DSBSpecFrame {
    fn to_usb_mapping(&self) -> AstResult<DynMapping>;
    fn to_lsb_mapping(&self) -> AstResult<DynMapping>;
    fn to_lo_mapping(&self) -> AstResult<DynMapping>;
    fn to_topo_mapping(&self) -> AstResult<DynMapping>;
    fn lo_freq(&self) -> AstResult<f64>;
}
```

### Algorithm for `to_usb_mapping` (representative)

1. If already USB → return UnitMap.
2. Compute `f_lo = dsb_centre + ifr`.
3. Validate `f_lo >= |ifr|` (sanity check from C's `GetLO`).
4. Build `topo_map`: mapping from current system/SOR to topocentric frequency
   Hz. Creates a temporary SpecFrame copying this frame's observer attributes
   (epoch, obs_lat, obs_lon, obs_alt, ref_ra, ref_dec) but set to
   System=Freq, StdOfRest=Topocentric. Then calls
   `self.spec.make_spec_mapping(&topo_frame)` (promoted to `pub(crate)`).
5. Build `flip_map`: a 1-D WinMap that reflects around f_lo.
   - LSB→USB: `WinMap::new(&[2.0 * f_lo], &[-1.0])` — `f_usb = 2*f_lo - f_lsb`
   - LO→USB: `WinMap::new(&[f_lo], &[1.0])` — `f_usb = offset + f_lo`
6. Compose: `topo_map` → `flip_map` → `topo_map⁻¹` (CmpMap series).

### Edge cases

- **Unphysical LO** (`f_lo < |ifr|`): report error (C's `AST__BDFTS`).
- **FrameSet integrity override**: when `map_from` is called for integrity
  restoration, `align_sideband` is forced true regardless of stored value
  (matching C's `AST__INTFLAG` behavior).
- **Match with plain SpecFrame**: DSBSpecFrame behaves as SpecFrame (no
  sideband conversion applied). This is by design.

## Frame Trait Implementation

```rust
impl Frame for DSBSpecFrame {
    fn naxes(&self) -> usize { 1 }
    fn format/unformat/distance/angle/offset/norm → delegate to self.spec
    fn snapshot(&self) -> DynFrame { Box::new(self.clone()) }
    fn obs_lat/obs_lon/obs_alt → delegate
    fn system_string → delegate

    fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        // 1. Downcast `to` as DSBSpecFrame:
        //    If both have align_sideband=true:
        //      a. parent_map = self.spec.make_spec_mapping(&to.spec)
        //      b. map1 = self → self's "observed" sideband
        //      c. map2 = to → to's "observed" sideband (inverted)
        //      d. compose: map1 → parent_map → map2⁻¹
        //    If align_sideband=false in either:
        //      Delegate to SpecFrame alignment (ignore sideband)
        //
        // 2. Downcast `to` as SpecFrame (not DSBSpecFrame):
        //    Delegate to SpecFrame alignment
        //
        // 3. Otherwise: return None
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        // For FrameSet integrity: previous is pre-mutation DSBSpecFrame
        // snapshot. Force align_sideband=true, compute sideband correction.
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // Downcast to DSBSpecFrame, compare spec fields + DSB fields.
    }
}
```

### "Observed" sideband determination

The "observed" sideband is the one containing Dsbcenter:
- IF > 0 → LO is above Dsbcenter → Dsbcenter is in LSB → observed = LSB
- IF ≤ 0 → LO is below Dsbcenter → Dsbcenter is in USB → observed = USB

## Serialization

**Class name:** `"DSBSpecFrame"`

### Native format tokens

Written after SpecFrame parent data, before `IsA DSBSpecFrame`:

| Key | Attribute | Type | Comment |
|-----|-----------|------|---------|
| `DSBCen` | dsb_centre | double | "Central frequency (Hz topo)" |
| `IF` | ifr | double | "Intermediate frequency (Hz topo)" |
| `SideBn` | sideband | string | "USB" / "LSB" / "LO" |
| `AlSdBn` | align_sideband | int (0/1) | "Align sidebands?" |

### Serialization approach

`to_node`: Serialize SpecFrame fields first (reuse SpecFrame's attribute-
writing logic on the same Node), then append DSB-specific attributes. Unset
attributes written with `unset` flag per AST convention.

`from_node`: Read SpecFrame portion first, then read DSB-specific keys.
Missing keys treated as unset (defaults apply).

Inheritance markers: `IsA SpecFrame` before DSB attributes, `IsA
DSBSpecFrame` at end.

## Label Override

When no explicit label is set, append sideband indicator:

```rust
fn default_label(&self) -> String {
    let base = self.spec.default_label();
    let suffix = match self.sideband() {
        SideBand::USB => " (USB)",
        SideBand::LSB => " (LSB)",
        SideBand::LO  => " (LO)",
    };
    format!("{base}{suffix}")
}
```

## Registration Points

1. `frames/mod.rs`: Add `mod dsbspecframe;` and
   `pub use dsbspecframe::{DSBSpecFrame, SideBand};`
2. `serial/mod.rs` → `frame_from_node`: Add `"DSBSpecFrame"` arm.
3. `serial/mod.rs` → `AnyObject`: Add `DSBSpecFrame(DSBSpecFrame)` variant +
   all match arms.
4. `specframe.rs`: Promote `make_spec_mapping` to `pub(crate)`.

## Testing Strategy

### File 1: `crates/ast-core/tests/dsbspecframe.rs`

Standalone integration test:
- Construction and attribute defaults
- Sideband resolution from IF sign
- Dsbcenter/IF getter/setter round-trips
- ImagFreq computation
- Sideband conversion: transform frequency through USB→LSB, verify
  `f_lsb = 2*f_lo - f_usb`
- Alignment between two DSBSpecFrames with different sidebands
  (AlignSideBand=true)
- Alignment between DSBSpecFrame and plain SpecFrame
- Serialization round-trip (Native format write → read → compare)
- Error cases: unphysical LO, missing rest freq for ImagFreq

### File 2: Port from `testframeset.c`

Port the DSBSpecFrame-relevant sections from
`reference/starlink-ast/ast_tester/testframeset.c` — exercises FrameSet
integrity machinery (SideBand change propagation, alignment through
FrameSets).

### C oracle validation

Final validation against the C library to confirm numeric agreement on
sideband conversions and alignment results.

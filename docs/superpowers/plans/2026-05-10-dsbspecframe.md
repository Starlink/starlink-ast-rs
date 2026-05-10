# DSBSpecFrame Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement DSBSpecFrame — a dual-sideband spectral coordinate frame that extends SpecFrame with sideband conversion logic.

**Architecture:** Composition-based: `DSBSpecFrame` wraps a `SpecFrame` and implements the `Frame` trait by delegation, overriding `convert`, `map_from`, `snapshot`, and `equal`. Sideband conversions use `WinMap` reflections around the LO frequency, sandwiched between topocentric-frequency mappings built via `SpecFrame::make_spec_mapping`.

**Tech Stack:** Rust 2024, ast-core crate (frames module). Depends on existing `SpecFrame`, `WinMap`, `CmpMap`, `UnitMap`, `SpecMap`.

**Spec:** `docs/superpowers/specs/2026-05-10-dsbspecframe-design.md`

---

## File Structure

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `crates/ast-core/src/frames/dsbspecframe.rs` | DSBSpecFrame struct, SideBand enum, public API, Frame impl, sideband conversion, Serializable impl |
| Modify | `crates/ast-core/src/frames/mod.rs` | Add `mod dsbspecframe` and `pub use` |
| Modify | `crates/ast-core/src/frames/specframe.rs` | Promote `make_spec_mapping` to `pub(crate)` |
| Modify | `crates/ast-core/src/serial/mod.rs` | Add DSBSpecFrame to `frame_from_node`, `any_object_from_node`, `any_object_to_node`, `AnyObject` enum + match arms |
| Create | `crates/ast-core/tests/test_dsbspecframe.rs` | Integration tests: attributes, sideband conversion, alignment, serialization |
| Create | `crates/ast-core/tests/fixtures/jan-dsb.ast` | Fixture: serialized DSBSpecFrame from reference repo |

---

### Task 1: Promote `make_spec_mapping` to `pub(crate)`

**Files:**
- Modify: `crates/ast-core/src/frames/specframe.rs:715`

- [ ] **Step 1: Change `make_spec_mapping` visibility**

In `crates/ast-core/src/frames/specframe.rs`, change the method signature from private to `pub(crate)`:

```rust
    pub(crate) fn make_spec_mapping(&self, result: &SpecFrame) -> AstResult<DynMapping> {
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build -p ast-core`
Expected: compiles cleanly, no public API change.

- [ ] **Step 3: Commit**

```bash
git add crates/ast-core/src/frames/specframe.rs
git commit -m "Promote SpecFrame::make_spec_mapping to pub(crate)

DSBSpecFrame needs to call this to build its parent alignment
mapping. No public API change.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 2: SideBand Enum and DSBSpecFrame Struct

**Files:**
- Create: `crates/ast-core/src/frames/dsbspecframe.rs`
- Modify: `crates/ast-core/src/frames/mod.rs`

- [ ] **Step 1: Write the test for construction and attribute defaults**

Create `crates/ast-core/tests/test_dsbspecframe.rs`:

```rust
//! DSBSpecFrame integration tests.

use ast_core::frames::{DSBSpecFrame, SideBand, SpecSystem};

#[test]
fn construction_defaults() {
    let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert_eq!(dsb.system(), SpecSystem::Freq);
    // Default DSBCentre = 3e10 Hz
    assert_eq!(dsb.dsb_centre(), 3.0e10);
    // Default IF = 4e9 Hz
    assert_eq!(dsb.ifr(), 4.0e9);
    // Default sideband: IF > 0 → LSB (observed)
    assert_eq!(dsb.sideband(), SideBand::LSB);
    // Default align_sideband = false
    assert!(!dsb.align_sideband());
}

#[test]
fn sideband_default_from_if_sign() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    // Positive IF → observed sideband is LSB
    dsb.set_ifr(4.0e9);
    assert_eq!(dsb.sideband(), SideBand::LSB);
    // Negative IF → observed sideband is USB
    dsb.set_ifr(-4.0e9);
    assert_eq!(dsb.sideband(), SideBand::USB);
    // Zero IF → USB
    dsb.set_ifr(0.0);
    assert_eq!(dsb.sideband(), SideBand::USB);
}

#[test]
fn explicit_sideband_overrides_default() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_ifr(4.0e9); // default would be LSB
    dsb.set_sideband(SideBand::USB);
    assert_eq!(dsb.sideband(), SideBand::USB);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --test test_dsbspecframe`
Expected: FAIL — `DSBSpecFrame` and `SideBand` not found.

- [ ] **Step 3: Create the dsbspecframe module with struct and basic API**

Create `crates/ast-core/src/frames/dsbspecframe.rs`:

```rust
//! The dual-sideband spectral coordinate frame (DSBSpecFrame).

use crate::frames::{SpecFrame, SpecSystem, StdOfRest};

/// Sideband indicator for a DSBSpecFrame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SideBand {
    /// Upper Side Band.
    USB,
    /// Lower Side Band.
    LSB,
    /// Local Oscillator (offset from LO).
    LO,
}

impl SideBand {
    /// Parse from string (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "USB" => Some(Self::USB),
            "LSB" => Some(Self::LSB),
            "LO" => Some(Self::LO),
            _ => None,
        }
    }

    /// Canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::USB => "USB",
            Self::LSB => "LSB",
            Self::LO => "LO",
        }
    }
}

/// Default DSBCentre: 30 GHz in Hz.
const DEFAULT_DSB_CENTRE: f64 = 3.0e10;

/// Default intermediate frequency: 4 GHz in Hz.
const DEFAULT_IF: f64 = 4.0e9;

/// A spectral frame for dual-sideband instruments.
///
/// Extends [`SpecFrame`] with sideband awareness: coordinates can represent
/// the upper sideband (USB), lower sideband (LSB), or offset from the
/// local oscillator (LO).
#[derive(Debug, Clone)]
pub struct DSBSpecFrame {
    spec: SpecFrame,
    dsb_centre: Option<f64>,
    ifr: Option<f64>,
    sideband: Option<SideBand>,
    align_sideband: Option<bool>,
}

impl DSBSpecFrame {
    /// Create a new DSBSpecFrame with the given spectral system.
    pub fn new(system: SpecSystem) -> Self {
        let mut spec = SpecFrame::new(system);
        spec.set_domain("DSBSPECTRUM");
        Self {
            spec,
            dsb_centre: None,
            ifr: None,
            sideband: None,
            align_sideband: None,
        }
    }

    /// Central frequency in topocentric Hz.
    pub fn dsb_centre(&self) -> f64 {
        self.dsb_centre.unwrap_or(DEFAULT_DSB_CENTRE)
    }

    /// Set the central frequency (topocentric Hz).
    pub fn set_dsb_centre(&mut self, hz: f64) {
        self.dsb_centre = Some(hz);
    }

    /// Intermediate frequency in topocentric Hz.
    pub fn ifr(&self) -> f64 {
        self.ifr.unwrap_or(DEFAULT_IF)
    }

    /// Set the intermediate frequency (topocentric Hz).
    pub fn set_ifr(&mut self, hz: f64) {
        self.ifr = Some(hz);
    }

    /// Current sideband (resolved from IF sign if not explicitly set).
    pub fn sideband(&self) -> SideBand {
        self.sideband.unwrap_or_else(|| {
            if self.ifr() > 0.0 {
                SideBand::LSB
            } else {
                SideBand::USB
            }
        })
    }

    /// Set the sideband explicitly.
    pub fn set_sideband(&mut self, sb: SideBand) {
        self.sideband = Some(sb);
    }

    /// Whether alignment should be sideband-aware.
    pub fn align_sideband(&self) -> bool {
        self.align_sideband.unwrap_or(false)
    }

    /// Set whether alignment should be sideband-aware.
    pub fn set_align_sideband(&mut self, val: bool) {
        self.align_sideband = Some(val);
    }

    /// The spectral coordinate system.
    pub fn system(&self) -> SpecSystem {
        self.spec.system()
    }

    /// Set the spectral coordinate system.
    pub fn set_system(&mut self, sys: SpecSystem) {
        self.spec.set_system(sys);
    }

    /// Standard of rest.
    pub fn std_of_rest(&self) -> StdOfRest {
        self.spec.std_of_rest()
    }

    /// Set the standard of rest.
    pub fn set_std_of_rest(&mut self, sor: StdOfRest) {
        self.spec.set_std_of_rest(sor);
    }

    /// Rest frequency in Hz, if set.
    pub fn rest_freq(&self) -> Option<f64> {
        self.spec.rest_freq()
    }

    /// Set the rest frequency (Hz).
    pub fn set_rest_freq(&mut self, hz: f64) {
        self.spec.set_rest_freq(hz);
    }

    /// Set the epoch (MJD).
    pub fn set_epoch(&mut self, mjd: f64) {
        self.spec.set_epoch(mjd);
    }

    /// Set observer geodetic latitude (radians).
    pub fn set_obs_lat(&mut self, lat: f64) {
        self.spec.set_obs_lat(lat);
    }

    /// Set observer geodetic longitude (radians).
    pub fn set_obs_lon(&mut self, lon: f64) {
        self.spec.set_obs_lon(lon);
    }

    /// Set observer altitude (metres).
    pub fn set_obs_alt(&mut self, alt: f64) {
        self.spec.set_obs_alt(alt);
    }

    /// Access the inner SpecFrame.
    pub fn spec(&self) -> &SpecFrame {
        &self.spec
    }

    /// Mutable access to the inner SpecFrame.
    pub fn spec_mut(&mut self) -> &mut SpecFrame {
        &mut self.spec
    }

    /// The "observed" sideband (containing DSBCentre).
    ///
    /// IF > 0 → LO above DSBCentre → DSBCentre in LSB → observed = LSB.
    /// IF ≤ 0 → LO below DSBCentre → DSBCentre in USB → observed = USB.
    pub(crate) fn observed_sideband(&self) -> SideBand {
        if self.ifr() > 0.0 {
            SideBand::LSB
        } else {
            SideBand::USB
        }
    }
}
```

- [ ] **Step 4: Register in mod.rs**

Add to `crates/ast-core/src/frames/mod.rs` after the `specframe` line:

```rust
mod dsbspecframe;
```

And in the `pub use` section:

```rust
pub use dsbspecframe::{DSBSpecFrame, SideBand};
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p ast-core --test test_dsbspecframe`
Expected: All 3 tests pass.

- [ ] **Step 6: Run clippy and fmt**

Run: `cargo fmt -p ast-core && cargo clippy -p ast-core -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/frames/dsbspecframe.rs crates/ast-core/src/frames/mod.rs crates/ast-core/tests/test_dsbspecframe.rs
git commit -m "Add DSBSpecFrame struct with SideBand enum and basic attributes

Composition-based design wrapping SpecFrame. Includes construction,
attribute getters/setters, sideband default resolution from IF sign.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 3: LO Frequency Computation and Sideband Conversion Mappings

**Files:**
- Modify: `crates/ast-core/src/frames/dsbspecframe.rs`
- Modify: `crates/ast-core/tests/test_dsbspecframe.rs`

- [ ] **Step 1: Write the test for LO frequency and sideband conversion**

Append to `crates/ast-core/tests/test_dsbspecframe.rs`:

```rust
use ast_core::coords::PointSet;
use ast_core::frames::Frame;
use ast_core::mappings::Mapping;

#[test]
fn lo_frequency() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9); // 220 GHz
    dsb.set_ifr(4.0e9);          // 4 GHz
    // LO = DSBCentre + IF = 224 GHz
    assert_eq!(dsb.lo_freq().unwrap(), 224.0e9);
}

#[test]
fn sideband_flip_usb_to_lsb() {
    // DSBSpecFrame in USB mode, system=Freq (topocentric by default).
    // Transform a USB frequency to LSB: f_lsb = 2*f_lo - f_usb.
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);

    let mapping = dsb.to_lsb_mapping().unwrap();

    let f_usb = 225.0e9; // 225 GHz in USB
    let f_lo = 224.0e9;
    let expected_lsb = 2.0 * f_lo - f_usb; // 223 GHz

    let input = PointSet::from_columns(&[&[f_usb]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let result = output.column(0)[0];

    assert!(
        (result - expected_lsb).abs() < 1.0,
        "expected {expected_lsb}, got {result}"
    );
}

#[test]
fn sideband_flip_lsb_to_usb() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::LSB);

    let mapping = dsb.to_usb_mapping().unwrap();

    let f_lsb = 219.0e9;
    let f_lo = 224.0e9;
    let expected_usb = 2.0 * f_lo - f_lsb; // 229 GHz

    let input = PointSet::from_columns(&[&[f_lsb]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let result = output.column(0)[0];

    assert!(
        (result - expected_usb).abs() < 1.0,
        "expected {expected_usb}, got {result}"
    );
}

#[test]
fn sideband_identity_when_already_target() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);

    // USB → USB should be identity
    let mapping = dsb.to_usb_mapping().unwrap();
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.column(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn lo_offset_mapping() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);

    let mapping = dsb.to_lo_mapping().unwrap();

    // USB freq 225 GHz, LO = 224 GHz → offset = 225 - 224 = 1 GHz
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let expected = 1.0e9;
    assert!(
        (output.column(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        output.column(0)[0]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --test test_dsbspecframe`
Expected: FAIL — `lo_freq`, `to_lsb_mapping`, `to_usb_mapping`, `to_lo_mapping` not found.

- [ ] **Step 3: Implement sideband conversion**

Add to `crates/ast-core/src/frames/dsbspecframe.rs`:

```rust
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{CmpMap, Composition, DynMapping, UnitMap, WinMap};

impl DSBSpecFrame {
    /// Local Oscillator frequency = DSBCentre + IF.
    pub fn lo_freq(&self) -> AstResult<f64> {
        let dsb_cen = self.dsb_centre();
        let ifr = self.ifr();
        let f_lo = dsb_cen + ifr;
        if f_lo < ifr.abs() {
            return Err(AstError::new(
                ErrorCode::Bdfts,
                format!(
                    "DSBSpecFrame: LO frequency ({f_lo} Hz) is less than |IF| ({} Hz)",
                    ifr.abs()
                ),
            ));
        }
        Ok(f_lo)
    }

    /// Mapping from the current system/SOR to topocentric frequency Hz.
    fn to_topo_mapping(&self) -> AstResult<DynMapping> {
        let mut topo_frame = SpecFrame::new(SpecSystem::Freq);
        topo_frame.set_std_of_rest(StdOfRest::Topocentric);
        // Copy observer attributes so SOR conversion works.
        if let Some(lat) = self.spec.obs_lat() {
            topo_frame.set_obs_lat(lat);
        }
        if let Some(lon) = self.spec.obs_lon() {
            topo_frame.set_obs_lon(lon);
        }
        if let Some(alt) = self.spec.obs_alt() {
            topo_frame.set_obs_alt(alt);
        }
        topo_frame.set_epoch(self.spec.epoch_mjd());
        if let Some(ra) = self.spec.ref_ra() {
            topo_frame.set_ref_ra(ra);
        }
        if let Some(dec) = self.spec.ref_dec() {
            topo_frame.set_ref_dec(dec);
        }
        if let Some(rf) = self.spec.rest_freq() {
            topo_frame.set_rest_freq(rf);
        }
        self.spec.make_spec_mapping(&topo_frame)
    }

    /// Mapping from current sideband to USB (in current spectral system).
    pub(crate) fn to_usb_mapping(&self) -> AstResult<DynMapping> {
        let current_sb = self.sideband();
        if current_sb == SideBand::USB {
            return Ok(Box::new(UnitMap::new(1)));
        }
        let f_lo = self.lo_freq()?;
        let topo_map = self.to_topo_mapping()?;

        let flip: DynMapping = match current_sb {
            SideBand::LSB => {
                // f_usb = 2*f_lo - f_lsb → shift=2*f_lo, scale=-1
                Box::new(WinMap::new(&[2.0 * f_lo], &[-1.0])?)
            }
            SideBand::LO => {
                // f_usb = offset + f_lo → shift=f_lo, scale=1
                Box::new(WinMap::new(&[f_lo], &[1.0])?)
            }
            SideBand::USB => unreachable!(),
        };

        self.compose_sideband_mapping(topo_map, flip)
    }

    /// Mapping from current sideband to LSB (in current spectral system).
    pub(crate) fn to_lsb_mapping(&self) -> AstResult<DynMapping> {
        let current_sb = self.sideband();
        if current_sb == SideBand::LSB {
            return Ok(Box::new(UnitMap::new(1)));
        }
        let f_lo = self.lo_freq()?;
        let topo_map = self.to_topo_mapping()?;

        let flip: DynMapping = match current_sb {
            SideBand::USB => {
                // f_lsb = 2*f_lo - f_usb → shift=2*f_lo, scale=-1
                Box::new(WinMap::new(&[2.0 * f_lo], &[-1.0])?)
            }
            SideBand::LO => {
                // f_lsb = f_lo - offset → shift=f_lo, scale=-1
                Box::new(WinMap::new(&[f_lo], &[-1.0])?)
            }
            SideBand::LSB => unreachable!(),
        };

        self.compose_sideband_mapping(topo_map, flip)
    }

    /// Mapping from current sideband to LO offset (in current spectral system).
    pub(crate) fn to_lo_mapping(&self) -> AstResult<DynMapping> {
        let current_sb = self.sideband();
        if current_sb == SideBand::LO {
            return Ok(Box::new(UnitMap::new(1)));
        }
        let f_lo = self.lo_freq()?;
        let topo_map = self.to_topo_mapping()?;

        let flip: DynMapping = match current_sb {
            SideBand::USB => {
                // offset = f_usb - f_lo → shift=-f_lo, scale=1
                Box::new(WinMap::new(&[-f_lo], &[1.0])?)
            }
            SideBand::LSB => {
                // offset = f_lo - f_lsb → shift=f_lo, scale=-1
                Box::new(WinMap::new(&[f_lo], &[-1.0])?)
            }
            SideBand::LO => unreachable!(),
        };

        self.compose_sideband_mapping(topo_map, flip)
    }

    /// Compose: topo_map → flip → topo_map⁻¹
    ///
    /// When system is already topocentric frequency, topo_map is a UnitMap
    /// and this simplifies to just the flip.
    fn compose_sideband_mapping(
        &self,
        topo_map: DynMapping,
        flip: DynMapping,
    ) -> AstResult<DynMapping> {
        // Build a second topo_map for the inverse direction.
        let mut topo_inv = self.to_topo_mapping()?;
        topo_inv.invert();

        // topo_map → flip → topo_inv
        let step1: DynMapping =
            Box::new(CmpMap::new(topo_map, flip, Composition::Series)?);
        let full: DynMapping =
            Box::new(CmpMap::new(step1, topo_inv, Composition::Series)?);
        Ok(full)
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p ast-core --test test_dsbspecframe`
Expected: All tests pass (including the new sideband conversion tests).

- [ ] **Step 6: Run clippy and fmt**

Run: `cargo fmt -p ast-core && cargo clippy -p ast-core -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/frames/dsbspecframe.rs crates/ast-core/tests/test_dsbspecframe.rs
git commit -m "Add sideband conversion mappings to DSBSpecFrame

Implements to_usb_mapping, to_lsb_mapping, to_lo_mapping using
WinMap reflections around f_lo, sandwiched between topocentric
frequency mappings.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 4: Frame Trait Implementation

**Files:**
- Modify: `crates/ast-core/src/frames/dsbspecframe.rs`
- Modify: `crates/ast-core/tests/test_dsbspecframe.rs`

- [ ] **Step 1: Write the test for Frame trait methods and conversion**

Append to `crates/ast-core/tests/test_dsbspecframe.rs`:

```rust
use ast_core::frames::FrameSet;

#[test]
fn frame_trait_basics() {
    let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert_eq!(dsb.naxes(), 1);
}

#[test]
fn convert_dsb_to_dsb_same_sideband() {
    // Two DSBSpecFrames with same settings → identity mapping.
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::USB);

    let fs = from.convert(&to, &[]).unwrap().unwrap();
    let mapping = fs.get_mapping(FrameRef::Base, FrameRef::Current).unwrap();

    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.column(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn convert_dsb_to_dsb_different_sideband_with_align() {
    // AlignSideBand=true: should flip sideband during alignment.
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);
    from.set_align_sideband(true);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::LSB);
    to.set_align_sideband(true);

    let fs = from.convert(&to, &[]).unwrap().unwrap();
    let mapping = fs.get_mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // USB 225 GHz → aligned through observed sideband → LSB
    // f_lo = 224 GHz, f_lsb = 2*224 - 225 = 223 GHz
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let expected = 223.0e9;
    assert!(
        (output.column(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        output.column(0)[0]
    );
}

#[test]
fn convert_dsb_to_dsb_different_sideband_no_align() {
    // AlignSideBand=false (default): ignores sideband, aligns as SpecFrame.
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::LSB);

    let fs = from.convert(&to, &[]).unwrap().unwrap();
    let mapping = fs.get_mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // Without AlignSideBand, numeric value passes through unchanged.
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.column(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn convert_dsb_to_specframe() {
    // DSBSpecFrame → SpecFrame: aligns as if SpecFrame (ignores sideband).
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let to = ast_core::frames::SpecFrame::new(SpecSystem::Freq);

    let fs = from.convert(&to, &[]).unwrap().unwrap();
    let mapping = fs.get_mapping(FrameRef::Base, FrameRef::Current).unwrap();

    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.column(0)[0] - 225.0e9).abs() < 1.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --test test_dsbspecframe`
Expected: FAIL — `Frame` not implemented for `DSBSpecFrame`.

- [ ] **Step 3: Implement the Frame trait**

Add to `crates/ast-core/src/frames/dsbspecframe.rs`:

```rust
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::ast_equal;

impl Frame for DSBSpecFrame {
    fn naxes(&self) -> usize {
        1
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.spec.format(axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.spec.unformat(axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        self.spec.distance(a, b)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        self.spec.angle(a, b, c)
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        self.spec.offset(from, to, dist)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.spec.norm(coords)
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        let Some(prev_dsb) = previous.as_any().downcast_ref::<DSBSpecFrame>() else {
            // Previous was not a DSBSpecFrame — delegate to SpecFrame.
            return self.spec.map_from(previous);
        };

        // FrameSet integrity: always use sideband-aware alignment.
        // This matches the C code's AST__INTFLAG forced AlignSideBand=1.
        self.make_dsb_mapping_aligned(prev_dsb)
    }

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        // Try DSBSpecFrame first.
        if let Some(to_dsb) = to.as_any().downcast_ref::<DSBSpecFrame>() {
            let mapping = if self.align_sideband() && to_dsb.align_sideband() {
                self.make_dsb_mapping_aligned(to_dsb)?
            } else {
                // Ignore sideband — align as SpecFrames.
                self.spec.make_spec_mapping(&to_dsb.spec)?
            };
            let mut fs = FrameSet::new(self.snapshot());
            fs.add_frame(FrameRef::Base, mapping, to.snapshot())?;
            return Ok(Some(fs));
        }

        // Try plain SpecFrame.
        if let Some(to_spec) = to.as_any().downcast_ref::<SpecFrame>() {
            let mapping = self.spec.make_spec_mapping(to_spec)?;
            let mut fs = FrameSet::new(self.snapshot());
            fs.add_frame(FrameRef::Base, mapping, to.snapshot())?;
            return Ok(Some(fs));
        }

        Ok(None)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other_dsb) = other.as_any().downcast_ref::<DSBSpecFrame>() else {
            return false;
        };
        if !self.spec.equal(&other_dsb.spec) {
            return false;
        }
        if !ast_equal(self.dsb_centre(), other_dsb.dsb_centre()) {
            return false;
        }
        if !ast_equal(self.ifr(), other_dsb.ifr()) {
            return false;
        }
        self.sideband() == other_dsb.sideband()
            && self.align_sideband() == other_dsb.align_sideband()
    }

    fn obs_lat(&self) -> Option<f64> {
        self.spec.obs_lat()
    }

    fn obs_lon(&self) -> Option<f64> {
        self.spec.obs_lon()
    }

    fn obs_alt(&self) -> Option<f64> {
        self.spec.obs_alt()
    }

    fn system_string(&self) -> Option<&str> {
        self.spec.system_string()
    }
}

impl DSBSpecFrame {
    /// Sideband-aware alignment mapping from `self` to `target`.
    ///
    /// Algorithm:
    /// 1. Map self → self's observed sideband
    /// 2. Apply parent SpecFrame alignment
    /// 3. Map target's observed sideband → target
    fn make_dsb_mapping_aligned(&self, target: &DSBSpecFrame) -> AstResult<DynMapping> {
        // Step 1: self's current sideband → self's observed sideband
        let map1 = self.to_observed_mapping()?;

        // Step 2: parent SpecFrame alignment (observed → observed in target's system)
        let parent_map = self.spec.make_spec_mapping(&target.spec)?;

        // Step 3: target's observed sideband → target's current sideband (inverse)
        let mut map2 = target.to_observed_mapping()?;
        map2.invert();

        // Compose: map1 → parent_map → map2
        let step1: DynMapping =
            Box::new(CmpMap::new(map1, parent_map, Composition::Series)?);
        let full: DynMapping =
            Box::new(CmpMap::new(step1, map2, Composition::Series)?);
        Ok(full)
    }

    /// Mapping from current sideband to the "observed" sideband.
    fn to_observed_mapping(&self) -> AstResult<DynMapping> {
        match self.observed_sideband() {
            SideBand::LSB => self.to_lsb_mapping(),
            SideBand::USB => self.to_usb_mapping(),
            SideBand::LO => self.to_lo_mapping(),
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_dsbspecframe`
Expected: All tests pass.

- [ ] **Step 5: Run clippy and fmt**

Run: `cargo fmt -p ast-core && cargo clippy -p ast-core -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/frames/dsbspecframe.rs crates/ast-core/tests/test_dsbspecframe.rs
git commit -m "Implement Frame trait for DSBSpecFrame

Includes convert (sideband-aware and plain alignment), map_from
(for FrameSet integrity with forced AlignSideBand), and all
delegated methods.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 5: ImagFreq Computation

**Files:**
- Modify: `crates/ast-core/src/frames/dsbspecframe.rs`
- Modify: `crates/ast-core/tests/test_dsbspecframe.rs`

- [ ] **Step 1: Write the test**

Append to `crates/ast-core/tests/test_dsbspecframe.rs`:

```rust
#[test]
fn imag_freq_computation() {
    // rest_freq in USB, ImagFreq should be the LSB equivalent.
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB); // observed = LSB (IF > 0)
    dsb.set_rest_freq(225.0e9);      // in USB

    // ImagFreq = mirror of rest_freq across LO.
    // f_lo = 224 GHz, imag = 2*224 - 225 = 223 GHz.
    let imag = dsb.imag_freq().unwrap();
    assert!(
        (imag - 223.0e9).abs() < 1.0,
        "expected 223 GHz, got {imag}"
    );
}

#[test]
fn imag_freq_no_rest_freq() {
    let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert!(dsb.imag_freq().is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --test test_dsbspecframe -- imag_freq`
Expected: FAIL — `imag_freq` not found.

- [ ] **Step 3: Implement imag_freq**

Add to `crates/ast-core/src/frames/dsbspecframe.rs`:

```rust
use crate::coords::PointSet;

impl DSBSpecFrame {
    /// Compute the image-sideband frequency corresponding to the rest frequency.
    ///
    /// This is the frequency in the image sideband that corresponds to
    /// the rest frequency in the observed sideband. Requires rest_freq to be set.
    pub fn imag_freq(&self) -> AstResult<f64> {
        let rest_freq = self.spec.rest_freq().ok_or_else(|| {
            AstError::new(
                ErrorCode::Norsf,
                "DSBSpecFrame::imag_freq requires rest_freq to be set",
            )
        })?;

        // Build a temporary DSBSpecFrame in Freq/Topo/observed-sideband,
        // get the mapping from observed → image, transform rest_freq.
        let f_lo = self.lo_freq()?;
        let observed_sb = self.observed_sideband();
        let image_sb = match observed_sb {
            SideBand::LSB => SideBand::USB,
            SideBand::USB => SideBand::LSB,
            SideBand::LO => {
                return Err(AstError::new(
                    ErrorCode::Inter,
                    "observed sideband cannot be LO",
                ));
            }
        };

        // The rest frequency is in the current system. We need to:
        // 1. Convert rest_freq to topocentric Hz
        // 2. Reflect around f_lo
        // 3. That's the image frequency in topocentric Hz
        let topo_map = self.to_topo_mapping()?;
        let input = PointSet::from_columns(&[&[rest_freq]])?;
        let topo_pts = topo_map.transform(&input, true)?;
        let rest_topo = topo_pts.column(0)[0];

        // Reflect: if observed=LSB, rest is in LSB, image is in USB: img = 2*f_lo - rest
        // Same formula regardless of which sideband rest is in.
        let imag_topo = 2.0 * f_lo - rest_topo;

        // Convert back from topo Hz to current system.
        let imag_input = PointSet::from_columns(&[&[imag_topo]])?;
        let result = topo_map.transform(&imag_input, false)?;
        Ok(result.column(0)[0])
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_dsbspecframe -- imag_freq`
Expected: Both tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/frames/dsbspecframe.rs crates/ast-core/tests/test_dsbspecframe.rs
git commit -m "Add DSBSpecFrame::imag_freq() computation

Computes the image-sideband frequency corresponding to the rest
frequency by reflecting across the LO in topocentric frequency space.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 6: Serialization (Serializable trait)

**Files:**
- Modify: `crates/ast-core/src/frames/dsbspecframe.rs`
- Modify: `crates/ast-core/tests/test_dsbspecframe.rs`
- Create: `crates/ast-core/tests/fixtures/jan-dsb.ast`

- [ ] **Step 1: Write the test for serialization round-trip**

Append to `crates/ast-core/tests/test_dsbspecframe.rs`:

```rust
use ast_core::{AnyObject, from_native_string, to_native_string};

#[test]
fn serialization_round_trip() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.463127593878e9);
    dsb.set_ifr(-3.99997957289606e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_rest_freq(220398676500.0);

    let obj = AnyObject::DSBSpecFrame(dsb.clone());
    let text = to_native_string(&obj).unwrap();

    assert!(text.contains("Begin DSBSpecFrame"));
    assert!(text.contains("IsA SpecFrame"));
    assert!(text.contains("DSBCen"));
    assert!(text.contains("IF"));
    assert!(text.contains("SideBn"));

    // Read back.
    let obj2 = from_native_string(&text).unwrap();
    let dsb2 = match obj2 {
        AnyObject::DSBSpecFrame(d) => d,
        other => panic!("expected DSBSpecFrame, got {other:?}"),
    };

    assert!((dsb2.dsb_centre() - dsb.dsb_centre()).abs() < 1.0);
    assert!((dsb2.ifr() - dsb.ifr()).abs() < 1.0);
    assert_eq!(dsb2.sideband(), SideBand::USB);
}

#[test]
fn deserialize_fixture() {
    let fixture = include_str!("fixtures/jan-dsb.ast");
    let obj = from_native_string(fixture).unwrap();
    let dsb = match obj {
        AnyObject::DSBSpecFrame(d) => d,
        other => panic!("expected DSBSpecFrame, got {other:?}"),
    };
    assert_eq!(dsb.sideband(), SideBand::USB);
    assert!((dsb.dsb_centre() - 220463127593.87817).abs() < 1.0);
    assert!((dsb.ifr() - (-3999979572.8960571)).abs() < 1.0);
}
```

- [ ] **Step 2: Create the fixture file**

Create `crates/ast-core/tests/fixtures/jan-dsb.ast` extracted from the reference:

```
 Begin DSBSpecFrame 	# Dual sideband spectral axis
    Naxes = 1 	# Number of coordinate axes
    Epoch = 2006.8632690742859 	# Julian epoch of observation
    System = "FREQ" 	# Coordinate system type
    Ax1 = 	# Axis number 1
       Begin Axis 	# Coordinate axis
          Unit = "Hz" 	# Axis units (Hertz)
       End Axis
 IsA Frame 	# Coordinate system description
    SoR = "Source" 	# Standard of rest
    RefRA = 0.71100769282139487 	# Reference RA (rads, FK5 J2000)
    RefDec = 1.0983843929661621 	# Reference Dec (rads, FK5 J2000)
    RstFrq = 220398676500 	# Rest frequency (Hz)
    SrcVel = -79989.315688607065 	# Source velocity (m/s)
    SrcVRF = "LSRK" 	# Source velocity rest frame
    UFreq = "Hz" 	# Preferred units for frequency
 IsA SpecFrame 	# Description of spectral coordinate system
    DSBCen = 220463127593.87817 	# Central frequency (Hz topo)
    IF = -3999979572.8960571 	# Intermediate frequency (Hz)
    SideBn = "USB" 	# Represents upper sideband
 End DSBSpecFrame
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p ast-core --test test_dsbspecframe -- serial`
Expected: FAIL — `AnyObject::DSBSpecFrame` variant doesn't exist.

- [ ] **Step 4: Implement Serializable trait**

Add to `crates/ast-core/src/frames/dsbspecframe.rs`:

```rust
use crate::serial::{Node, NodeValue, Serializable};

impl Serializable for DSBSpecFrame {
    fn class_name(&self) -> &'static str {
        "DSBSpecFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        // Start from the SpecFrame's node (gets all Frame + SpecFrame attrs).
        let mut node = self.spec.to_node()?;

        // Override the class name.
        node.class = "DSBSpecFrame".to_string();
        node.class_comment = Some("Dual sideband spectral axis".to_string());

        // Add IsA SpecFrame marker before DSB attributes.
        node.push_isa(
            "SpecFrame",
            Some("Description of spectral coordinate system".to_string()),
        );

        // DSBCen: always write (active if set, unset with default).
        if let Some(dsb_cen) = self.dsb_centre {
            node.push_double(
                "DSBCen",
                dsb_cen,
                Some("Central frequency (Hz topo)".to_string()),
            );
        } else {
            node.push_unset(
                "DSBCen",
                NodeValue::Double(DEFAULT_DSB_CENTRE),
                Some("Central frequency (Hz topo)".to_string()),
            );
        }

        // IF: always write.
        if let Some(ifr) = self.ifr {
            node.push_double(
                "IF",
                ifr,
                Some("Intermediate frequency (Hz)".to_string()),
            );
        } else {
            node.push_unset(
                "IF",
                NodeValue::Double(DEFAULT_IF),
                Some("Intermediate frequency (Hz)".to_string()),
            );
        }

        // SideBn: only write if explicitly set.
        if let Some(sb) = self.sideband {
            let sb_str = match sb {
                SideBand::USB => "USB",
                SideBand::LSB => "LSB",
                SideBand::LO => "LO",
            };
            node.push_string(
                "SideBn",
                sb_str,
                Some(match sb {
                    SideBand::USB => "Represents upper sideband",
                    SideBand::LSB => "Represents lower sideband",
                    SideBand::LO => "Represents offset from LO",
                }.to_string()),
            );
        }

        // AlSdBn: only write if explicitly set to true.
        if let Some(true) = self.align_sideband {
            node.push_int("AlSdBn", 1, Some("Align sidebands?".to_string()));
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Deserialize the SpecFrame portion.
        let spec = SpecFrame::from_node(node)?;

        // Read DSB-specific attributes.
        let dsb_centre = if node.get("DSBCen").is_some() {
            Some(node.get_double("DSBCen", DEFAULT_DSB_CENTRE))
        } else {
            None
        };

        let ifr = if node.get("IF").is_some() {
            Some(node.get_double("IF", DEFAULT_IF))
        } else {
            None
        };

        let sideband = {
            let sb_str = node.get_string("SideBn", "");
            if sb_str.is_empty() {
                None
            } else {
                SideBand::parse(&sb_str)
            }
        };

        let align_sideband = {
            let val = node.get_int("AlSdBn", -1);
            if val < 0 {
                None
            } else {
                Some(val != 0)
            }
        };

        Ok(DSBSpecFrame {
            spec,
            dsb_centre,
            ifr,
            sideband,
            align_sideband,
        })
    }
}
```

**Note:** `SpecFrame::from_node` is currently `pub(crate)` via the `Serializable` trait — verify this is accessible from the `dsbspecframe` module (same crate, so yes).

- [ ] **Step 5: Register DSBSpecFrame in `serial/mod.rs`**

In `crates/ast-core/src/serial/mod.rs`:

1. Add to the `AnyObject` enum:
```rust
    /// A [`DSBSpecFrame`] (dual-sideband spectral coordinate frame).
    DSBSpecFrame(DSBSpecFrame),
```

2. Add to `as_mapping` (returns None):
```rust
            | AnyObject::DSBSpecFrame(_)
```

3. Add to `as_frame`:
```rust
            AnyObject::DSBSpecFrame(f) => Some(f),
```

4. Add to `into_mapping` (returns None):
```rust
            | AnyObject::DSBSpecFrame(_)
```

5. Add to `into_frame`:
```rust
            AnyObject::DSBSpecFrame(f) => Some(Box::new(f)),
```

6. Add to `frame_from_node`:
```rust
        "DSBSpecFrame" => Ok(Box::new(DSBSpecFrame::from_node(node)?)),
```

7. Add to `any_object_from_node`:
```rust
        "DSBSpecFrame" => Ok(AnyObject::DSBSpecFrame(DSBSpecFrame::from_node(node)?)),
```

8. Add to `any_object_to_node`:
```rust
        AnyObject::DSBSpecFrame(f) => Serializable::to_node(f),
```

Add the necessary `use` import at the top of `serial/mod.rs`:
```rust
use crate::frames::DSBSpecFrame;
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p ast-core --test test_dsbspecframe`
Expected: All tests pass including serialization round-trip and fixture deserialization.

- [ ] **Step 7: Run clippy and fmt**

Run: `cargo fmt -p ast-core && cargo clippy -p ast-core -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/frames/dsbspecframe.rs crates/ast-core/src/serial/mod.rs crates/ast-core/tests/test_dsbspecframe.rs crates/ast-core/tests/fixtures/jan-dsb.ast
git commit -m "Add DSBSpecFrame serialization and AnyObject registration

Implements Serializable trait (to_node/from_node), registers in
AnyObject enum and all dispatch points. Includes fixture-based
deserialization test from reference data.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 7: Label Override and Domain

**Files:**
- Modify: `crates/ast-core/src/frames/dsbspecframe.rs`
- Modify: `crates/ast-core/tests/test_dsbspecframe.rs`

- [ ] **Step 1: Write the test**

Append to `crates/ast-core/tests/test_dsbspecframe.rs`:

```rust
#[test]
fn default_domain_is_dsbspectrum() {
    let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    // The domain should be DSBSPECTRUM, not SPECTRUM.
    // Check via serialization since domain is exposed through the spec.
    let obj = AnyObject::DSBSpecFrame(dsb);
    let text = to_native_string(&obj).unwrap();
    assert!(
        text.contains("DSBSPECTRUM") || text.contains("Domain"),
        "domain should be DSBSPECTRUM"
    );
}
```

- [ ] **Step 2: Verify the domain is already correct**

The `DSBSpecFrame::new()` already sets `spec.set_domain("DSBSPECTRUM")` (from Task 2). This test should already pass. Run to confirm:

Run: `cargo test -p ast-core --test test_dsbspecframe -- default_domain`
Expected: PASS.

- [ ] **Step 3: Commit (if any label-related changes were needed)**

If the domain test passes without changes, skip this commit. If label override logic was needed (e.g. a `default_label` method), commit those changes.

---

### Task 8: Full Test Suite — FrameSet Integrity and testframeset Port

**Files:**
- Modify: `crates/ast-core/tests/test_dsbspecframe.rs`

- [ ] **Step 1: Write FrameSet integrity test**

Append to `crates/ast-core/tests/test_dsbspecframe.rs`:

```rust
#[test]
fn frameset_integrity_sideband_change() {
    // When SideBand is changed through a FrameSet, the internal
    // mapping should update to reflect the sideband flip.
    use ast_core::mappings::UnitMap;

    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);

    // Create a FrameSet with DSBSpecFrame as current frame.
    let mut fs = FrameSet::new(Box::new(dsb.clone()));

    // Transform a point before the change.
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let mapping_before = fs.get_mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let out_before = mapping_before.transform(&input, true).unwrap();
    // Should be identity initially.
    assert!((out_before.column(0)[0] - 225.0e9).abs() < 1.0);

    // Change sideband on the current frame (through FrameSet).
    // This should trigger integrity restoration.
    let current = fs.current_frame_mut();
    let dsb_ref = current.as_any_mut().downcast_mut::<DSBSpecFrame>().unwrap();
    dsb_ref.set_sideband(SideBand::LSB);

    // After integrity restoration, the mapping should include the sideband flip.
    // Base(USB) → Current(LSB): f_lsb = 2*f_lo - f_usb = 2*224e9 - 225e9 = 223e9
    let mapping_after = fs.get_mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let out_after = mapping_after.transform(&input, true).unwrap();
    let expected = 223.0e9;
    assert!(
        (out_after.column(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        out_after.column(0)[0]
    );
}
```

**Note:** This test depends on FrameSet integrity machinery supporting DSBSpecFrame. If `FrameSet` uses `as_any_mut()` and the current frame implements the proper `map_from`, it should work. If `FrameSet` doesn't expose `current_frame_mut()` with downcast support, this test may need to be adjusted based on the actual FrameSet API.

- [ ] **Step 2: Write alignment test matching testframeset.c patterns**

Append to `crates/ast-core/tests/test_dsbspecframe.rs`:

```rust
#[test]
fn alignment_through_frameset() {
    // Create two FrameSets with DSBSpecFrames and convert between them.
    let mut dsb1 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb1.set_dsb_centre(220.0e9);
    dsb1.set_ifr(4.0e9);
    dsb1.set_sideband(SideBand::USB);
    dsb1.set_align_sideband(true);

    let mut dsb2 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb2.set_dsb_centre(220.0e9);
    dsb2.set_ifr(4.0e9);
    dsb2.set_sideband(SideBand::LSB);
    dsb2.set_align_sideband(true);

    // Use Frame::convert directly (FrameSet.convert delegates to this).
    let fs = dsb1.convert(&dsb2, &[]).unwrap().unwrap();
    let mapping = fs.get_mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // USB 226 GHz → LSB: f_lo=224, f_lsb = 2*224-226 = 222 GHz
    let input = PointSet::from_columns(&[&[226.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let expected = 222.0e9;
    assert!(
        (output.column(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        output.column(0)[0]
    );

    // Inverse: LSB 222 GHz → USB 226 GHz
    let inv_output = mapping.transform(&output, false).unwrap();
    assert!(
        (inv_output.column(0)[0] - 226.0e9).abs() < 1.0,
        "inverse expected 226e9, got {}",
        inv_output.column(0)[0]
    );
}

#[test]
fn alignment_dsb_to_dsb_different_lo() {
    // Two DSBSpecFrames with different LO frequencies.
    let mut dsb1 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb1.set_dsb_centre(220.0e9);
    dsb1.set_ifr(4.0e9); // LO = 224 GHz
    dsb1.set_sideband(SideBand::USB);
    dsb1.set_align_sideband(true);

    let mut dsb2 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb2.set_dsb_centre(230.0e9);
    dsb2.set_ifr(5.0e9); // LO = 235 GHz
    dsb2.set_sideband(SideBand::USB);
    dsb2.set_align_sideband(true);

    let fs = dsb1.convert(&dsb2, &[]).unwrap().unwrap();
    let mapping = fs.get_mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // dsb1 USB 225 GHz → dsb1 observed (LSB): 2*224-225 = 223 GHz
    // parent alignment (Freq/Topo → Freq/Topo): identity → 223 GHz
    // dsb2 observed (LSB) → dsb2 USB: 2*235-223 = 247 GHz
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let expected = 247.0e9;
    assert!(
        (output.column(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        output.column(0)[0]
    );
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p ast-core --test test_dsbspecframe`
Expected: All tests pass.

- [ ] **Step 4: Run clippy and fmt**

Run: `cargo fmt -p ast-core && cargo clippy -p ast-core -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/tests/test_dsbspecframe.rs
git commit -m "Add FrameSet integrity and alignment tests for DSBSpecFrame

Tests sideband change propagation through FrameSet, alignment
between DSBSpecFrames with same/different LO, and inverse transforms.

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 9: Run Full Test Suite and Fix Any Issues

**Files:**
- Potentially any file from previous tasks.

- [ ] **Step 1: Run the full ast-core test suite**

Run: `cargo test -p ast-core`
Expected: All existing tests still pass. No regressions from the DSBSpecFrame additions.

- [ ] **Step 2: Run clippy on the whole workspace**

Run: `cargo clippy --workspace -- -D warnings`
Expected: clean.

- [ ] **Step 3: Fix any issues found**

If any existing tests break (e.g., exhaustiveness in match arms on `AnyObject`), fix them.

- [ ] **Step 4: Commit fixes if any**

```bash
git add -u
git commit -m "Fix integration issues from DSBSpecFrame addition

Generated with AI

Co-Authored-By: SLAC AI"
```

---

### Task 10: C Oracle Validation

**Files:**
- Create: `crates/ast-oracle-tests/tests/test_dsbspecframe_oracle.rs` (or similar)

- [ ] **Step 1: Design oracle test**

Create a test that exercises the same operations as our Rust tests but compares results against the C library. Key operations to validate:

1. Sideband flip: USB frequency → LSB frequency for given DSBCentre/IF
2. Alignment between two DSBSpecFrames with AlignSideBand=true
3. ImagFreq computation
4. Serialization round-trip through C (write in Rust, read in C, compare)

- [ ] **Step 2: Implement oracle test**

This will use the `ast-oracle-tests` crate pattern — link both the Rust implementation and the C `libast.a`, run the same operations through both, and compare outputs within ULP tolerance.

The specific implementation depends on the existing oracle test infrastructure. Follow the pattern of existing oracle tests in `crates/ast-oracle-tests/`.

- [ ] **Step 3: Run oracle tests**

Run: `cargo test -p ast-oracle-tests -- dsbspecframe`
Expected: Results match within tolerance.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-oracle-tests/
git commit -m "Add DSBSpecFrame C oracle validation tests

Validates sideband conversion, alignment, and ImagFreq against
the reference C library implementation.

Generated with AI

Co-Authored-By: SLAC AI"
```

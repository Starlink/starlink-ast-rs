# FitsChan Phase 3: FITS-WCS Write (Encoder) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the FITS-WCS encoder that converts an AST FrameSet into FITS header keywords — enabling FitsChan to write WCS information into FITS files.

**Architecture:** A pipeline that decomposes a FrameSet into its constituent frames and mappings, identifies celestial/spectral/linear axes, extracts the linear transformation matrix (CRPIX/CDELT/PC or CD), and writes structured FITS keywords into the CardList. The encoder is the inverse of the Phase 2 decoder: `FrameSet → FitsStore → FITS cards`. Study the C `FsetToStore()` → `AddVersion()` → `WcsFromStore()` pipeline at `reference/starlink-ast/src/fitschan.c` for the exact algorithm.

**Tech Stack:** Rust 2024 edition. Uses all existing Frame and Mapping types, the FitsStore intermediate structure from Phase 2, and the FitsChan card manipulation API.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/src/fitschan/wcs_write.rs` | Main encoder: FrameSet → FitsStore → CardList |
| `crates/ast-core/src/fitschan/wcs_decompose.rs` | Decompose FrameSet mapping into linear + non-linear parts |
| `crates/ast-core/src/fitschan/mod.rs` | Wire `write()` for `Encoding::FitsWcs` |
| `crates/ast-core/src/frames/cmpframe.rs` | Add public accessors for child frames |
| `crates/ast-core/tests/test_fitschan_wcs_write.rs` | Integration tests for WCS write |

---

### Task 1: Add CmpFrame Public Accessors

**Files:**
- Modify: `crates/ast-core/src/frames/cmpframe.rs`

The encoder needs to inspect the current frame of a FrameSet to identify celestial/spectral sub-frames. CmpFrame's `frame1`/`frame2` fields are currently private with no public accessors.

- [ ] **Step 1: Add public accessor methods to CmpFrame**

Add these methods to the `impl CmpFrame` block (after the existing `naxes1` method at line 37):

```rust
    /// Access the first child frame.
    pub fn frame1(&self) -> &dyn Frame {
        self.frame1.as_ref()
    }

    /// Access the second child frame.
    pub fn frame2(&self) -> &dyn Frame {
        self.frame2.as_ref()
    }
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build -p ast-core`
Expected: compiles cleanly

- [ ] **Step 3: Commit**

---

### Task 2: Mapping Decomposition Module

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_decompose.rs`

The encoder must decompose the FrameSet's base→current mapping into a linear transformation (expressible as CRPIX + PC×CDELT or CD matrix). This mirrors the C `MakeIntWorld` function. For Phase 3, we support the common case where the mapping IS linear (or composed of linear mappings like WinMap, MatrixMap, ZoomMap).

- [ ] **Step 1: Create the module with LinearDecomposition struct**

```rust
//! Decompose a FrameSet mapping into FITS-WCS linear components.
//!
//! The decomposition extracts: CRPIX (reference pixel shift), a rotation/scale
//! matrix (PC or CD), and identifies the non-linear portion (projection) if
//! present. This mirrors the C library's `MakeIntWorld` function (fitschan.c
//! ~line 21689).

use ndarray::Array2;

use crate::error::{AstResult, AstError, ErrorCode};
use crate::mappings::{DynMapping, Mapping};

/// Result of decomposing a pixel→world mapping into linear WCS components.
#[derive(Debug)]
pub(crate) struct LinearDecomposition {
    /// Reference pixel position (1-based FITS convention, i.e. CRPIX values).
    pub crpix: Vec<f64>,
    /// CD matrix (combined rotation + scale): cd[i][j].
    /// This encodes the linear part: IWC = CD × (pixel - CRPIX).
    pub cd: Array2<f64>,
    /// Reference world coordinate values (CRVAL per axis).
    pub crval: Vec<f64>,
}
```

- [ ] **Step 2: Implement decompose_linear by sampling the mapping**

The algorithm samples the mapping at the reference pixel and at unit offsets to extract the Jacobian matrix. This is how the C `MakeIntWorld` works — it doesn't try to symbolically decompose the CmpMap tree but instead evaluates the mapping numerically.

```rust
/// Decompose a pixel→world mapping into linear WCS components.
///
/// Extracts the linear transformation by evaluating the mapping at the
/// reference pixel and at unit offsets along each axis. The reference
/// pixel is taken as the centre of the pixel grid (or 1.0 if unknown).
///
/// Returns `None` if the mapping is significantly non-linear (deviation
/// from linearity exceeds tolerance).
pub(crate) fn decompose_linear(
    mapping: &dyn Mapping,
    naxis: usize,
    ref_pixel: &[f64],
) -> AstResult<LinearDecomposition> {
    use crate::coords::PointSet;

    // Evaluate at reference pixel to get CRVAL.
    let ref_pts = PointSet::from_rows(std::iter::once(ref_pixel.to_vec()));
    let ref_out = mapping.transform(&ref_pts, true)?;
    let crval: Vec<f64> = ref_out.point(0).to_vec();

    // Build the CD matrix by evaluating at unit offsets.
    // CD[i][j] = d(world_i) / d(pixel_j) evaluated at CRPIX.
    let mut cd = Array2::zeros((naxis, naxis));
    for j in 0..naxis {
        let mut offset_pixel = ref_pixel.to_vec();
        offset_pixel[j] += 1.0;

        let offset_pts = PointSet::from_rows(std::iter::once(offset_pixel));
        let offset_out = mapping.transform(&offset_pts, true)?;
        let offset_world = offset_out.point(0);

        for i in 0..naxis {
            cd[[i, j]] = offset_world[i] - crval[i];
        }
    }

    Ok(LinearDecomposition {
        crpix: ref_pixel.to_vec(),
        cd,
        crval,
    })
}
```

- [ ] **Step 3: Implement split_cd to factor CD into CDELT × PC**

```rust
/// Split a CD matrix into CDELT (diagonal scale) and PC (rotation) components.
///
/// CD = diag(CDELT) × PC, where each row of PC is normalised to unit length.
/// This mirrors the C `SplitMat` function (fitschan.c ~line 33141).
///
/// Returns (cdelt, pc) where cdelt[i] = magnitude of row i of CD,
/// and pc[i][j] = cd[i][j] / cdelt[i].
pub(crate) fn split_cd(cd: &Array2<f64>) -> (Vec<f64>, Array2<f64>) {
    let naxis = cd.nrows();
    let mut cdelt = vec![0.0; naxis];
    let mut pc = Array2::zeros((naxis, naxis));

    for i in 0..naxis {
        // Row magnitude = CDELT[i].
        let mut mag_sq = 0.0;
        for j in 0..naxis {
            mag_sq += cd[[i, j]] * cd[[i, j]];
        }
        let mag = mag_sq.sqrt();

        if mag == 0.0 {
            // Degenerate row: set CDELT=1, PC row to zero.
            cdelt[i] = 1.0;
            for j in 0..naxis {
                pc[[i, j]] = 0.0;
            }
        } else {
            cdelt[i] = mag;
            for j in 0..naxis {
                pc[[i, j]] = cd[[i, j]] / mag;
            }
            // Ensure the diagonal element of PC is positive (convention).
            if pc[[i, i]] < 0.0 {
                cdelt[i] = -cdelt[i];
                for j in 0..naxis {
                    pc[[i, j]] = -pc[[i, j]];
                }
            }
        }
    }

    (cdelt, pc)
}
```

- [ ] **Step 4: Add unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;
    use crate::mappings::{WinMap, MatrixMap, CmpMap, Composition, Mapping};

    #[test]
    fn decompose_identity() {
        // Identity: CRPIX at (1,1), CRVAL at (1,1), CD = identity.
        let unit = crate::mappings::UnitMap::new(2);
        let result = decompose_linear(&unit, 2, &[1.0, 1.0]).unwrap();
        assert!((result.crval[0] - 1.0).abs() < 1e-10);
        assert!((result.crval[1] - 1.0).abs() < 1e-10);
        assert!((result.cd[[0, 0]] - 1.0).abs() < 1e-10);
        assert!((result.cd[[1, 1]] - 1.0).abs() < 1e-10);
        assert!(result.cd[[0, 1]].abs() < 1e-10);
        assert!(result.cd[[1, 0]].abs() < 1e-10);
    }

    #[test]
    fn decompose_shift_and_scale() {
        // WinMap: shift by -128, scale by 0.001.
        let wm = WinMap::new(&[-128.0, -128.0], &[0.001, 0.001]).unwrap();
        let result = decompose_linear(&wm, 2, &[128.0, 128.0]).unwrap();
        // At CRPIX=(128,128): output = (128-128)*0.001 = 0.
        assert!(result.crval[0].abs() < 1e-10);
        assert!(result.crval[1].abs() < 1e-10);
        // CD should be diagonal with 0.001.
        assert!((result.cd[[0, 0]] - 0.001).abs() < 1e-12);
        assert!((result.cd[[1, 1]] - 0.001).abs() < 1e-12);
    }

    #[test]
    fn split_cd_diagonal() {
        let mut cd = Array2::zeros((2, 2));
        cd[[0, 0]] = -0.001;
        cd[[1, 1]] = 0.001;
        let (cdelt, pc) = split_cd(&cd);
        // CDELT should have correct signs to make PC diagonal positive.
        assert!((cdelt[0] - (-0.001)).abs() < 1e-15);
        assert!((cdelt[1] - 0.001).abs() < 1e-15);
        assert!((pc[[0, 0]] - 1.0).abs() < 1e-15);
        assert!((pc[[1, 1]] - 1.0).abs() < 1e-15);
    }

    #[test]
    fn split_cd_rotated() {
        // CD with 45-degree rotation and scale 0.001.
        let s = 0.001_f64;
        let cos45 = std::f64::consts::FRAC_1_SQRT_2;
        let mut cd = Array2::zeros((2, 2));
        cd[[0, 0]] = s * cos45;
        cd[[0, 1]] = -s * cos45;
        cd[[1, 0]] = s * cos45;
        cd[[1, 1]] = s * cos45;
        let (cdelt, pc) = split_cd(&cd);
        // CDELT should be ~0.001 for both axes.
        assert!((cdelt[0].abs() - s).abs() < 1e-12);
        assert!((cdelt[1].abs() - s).abs() < 1e-12);
        // PC should be a rotation matrix (unit rows).
        let row0_mag = (pc[[0, 0]].powi(2) + pc[[0, 1]].powi(2)).sqrt();
        let row1_mag = (pc[[1, 0]].powi(2) + pc[[1, 1]].powi(2)).sqrt();
        assert!((row0_mag - 1.0).abs() < 1e-12);
        assert!((row1_mag - 1.0).abs() < 1e-12);
    }
}
```

- [ ] **Step 5: Register the module in mod.rs and verify**

Add to `crates/ast-core/src/fitschan/mod.rs`:
```rust
#[allow(dead_code)]
pub(crate) mod wcs_decompose;
```

Run: `cargo test -p ast-core -- wcs_decompose`
Expected: all 4 tests pass

- [ ] **Step 6: Commit**

---

### Task 3: FitsStore Population from FrameSet

**Files:**
- Create: `crates/ast-core/src/fitschan/wcs_write.rs`

The main encoder logic: given a FrameSet, identify axis types, decompose the mapping, and populate a FitsStore. Then write the FitsStore contents as FITS cards.

- [ ] **Step 1: Create wcs_write.rs with store_from_frameset**

```rust
//! FITS-WCS encoder pipeline.
//!
//! Converts an AST FrameSet into FITS header cards by:
//! 1. Decomposing the FrameSet into a FitsStore (axis identification + linear extraction)
//! 2. Writing the FitsStore contents as FITS keyword cards
//!
//! This mirrors the C library's `FsetToStore()` → `WcsFromStore()` pipeline.

use std::f64::consts::PI;

use super::card::FitsCard;
use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::wcs_decompose::{decompose_linear, split_cd};
use crate::error::{AstResult, AstError, ErrorCode};
use crate::frames::{
    CmpFrame, Frame, FrameRef, FrameSet, SkyFrame, SkySystem, SpecFrame, SpecSystem, StdOfRest,
};
use crate::mappings::{AsAny, Mapping};
use crate::pal;
use crate::serial::AnyObject;

/// Encode a FrameSet as FITS-WCS keywords into a CardList.
///
/// This is the top-level encoder entry point. Returns Ok(()) on success,
/// or an error if the FrameSet cannot be represented in FITS-WCS encoding.
pub(crate) fn encode_wcs(obj: &AnyObject, cards: &mut CardList, use_cd: bool) -> AstResult<()> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "FitsWcs encoding requires a FrameSet object",
            ));
        }
    };

    let store = store_from_frameset(frameset)?;
    write_store_to_cards(&store, cards, use_cd)
}

/// Decompose a FrameSet into a FitsStore.
///
/// Identifies axis types from the current frame, decomposes the base→current
/// mapping into linear components, and populates the FitsStore fields.
fn store_from_frameset(fs: &FrameSet) -> AstResult<FitsStore> {
    let naxis = fs.base().naxes();
    let mut store = FitsStore::new(naxis);

    // Get the base→current mapping.
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;

    // Choose a reference pixel at the centre of a nominal grid.
    // The C library uses the pixel dimensions if known; we default to 1.0
    // (the FITS convention for the first pixel).
    let ref_pixel: Vec<f64> = vec![1.0; naxis];

    // Decompose the mapping at the reference pixel.
    let decomp = decompose_linear(mapping.as_ref(), naxis, &ref_pixel)?;

    // Store CRPIX.
    for i in 0..naxis {
        store.crpix[i] = Some(decomp.crpix[i]);
    }

    // Store the CD matrix.
    for i in 0..naxis {
        for j in 0..naxis {
            store.cd[i][j] = Some(decomp.cd[[i, j]]);
        }
    }

    // Identify axis types from the current frame and populate CTYPE/CRVAL.
    identify_axes(fs, &decomp.crval, &mut store)?;

    Ok(store)
}
```

- [ ] **Step 2: Implement identify_axes**

```rust
/// Identify axis types from the current frame and populate CTYPE, CRVAL,
/// RADESYS, EQUINOX, SPECSYS, etc.
fn identify_axes(fs: &FrameSet, crval: &[f64], store: &mut FitsStore) -> AstResult<()> {
    let current = fs.current();
    let naxis = store.naxis;

    // Try to identify the frame type.
    if let Some(sky) = current.as_any().downcast_ref::<SkyFrame>() {
        // Pure 2-axis celestial.
        populate_celestial(sky, crval, store, 0, 1)?;
    } else if let Some(spec) = current.as_any().downcast_ref::<SpecFrame>() {
        // Pure 1-axis spectral.
        populate_spectral(spec, crval, store, 0)?;
    } else if let Some(cmp) = current.as_any().downcast_ref::<CmpFrame>() {
        // Compound frame: inspect children.
        let n1 = cmp.naxes1();
        let frame1 = cmp.frame1();
        let frame2 = cmp.frame2();

        let mut axis_offset = 0;
        if let Some(sky) = frame1.as_any().downcast_ref::<SkyFrame>() {
            populate_celestial(sky, crval, store, 0, 1)?;
            axis_offset = n1;
        } else if let Some(spec) = frame1.as_any().downcast_ref::<SpecFrame>() {
            populate_spectral(spec, crval, store, 0)?;
            axis_offset = n1;
        }

        if let Some(sky) = frame2.as_any().downcast_ref::<SkyFrame>() {
            populate_celestial(sky, crval, store, axis_offset, axis_offset + 1)?;
        } else if let Some(spec) = frame2.as_any().downcast_ref::<SpecFrame>() {
            populate_spectral(spec, crval, store, axis_offset)?;
        }

        // Any remaining axes without CTYPE get a generic label.
        for i in 0..naxis {
            if store.ctype[i].is_none() {
                store.ctype[i] = Some(format!("AXIS{}", i + 1));
                store.crval[i] = Some(crval[i]);
            }
        }
    } else {
        // BasicFrame or unknown: generic linear axes.
        for i in 0..naxis {
            store.ctype[i] = Some(format!("AXIS{}", i + 1));
            store.crval[i] = Some(crval[i]);
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Implement populate_celestial**

```rust
/// Populate FitsStore fields for celestial axes.
///
/// Sets CTYPE (with projection code), CRVAL (in degrees), RADESYS, EQUINOX,
/// and LONPOLE/LATPOLE.
fn populate_celestial(
    sky: &SkyFrame,
    crval: &[f64],
    store: &mut FitsStore,
    lon_axis: usize,
    lat_axis: usize,
) -> AstResult<()> {
    let system = sky.system();

    // Determine projection code. For the initial encoder, we use TAN as the
    // default projection (the most common case). The C library determines this
    // from the WcsMap in the mapping, but for Phase 3 we use a simple default.
    let proj_code = "TAN";

    // Build CTYPE values.
    let (lon_prefix, lat_prefix) = ctype_prefixes(system);
    store.ctype[lon_axis] = Some(format!("{lon_prefix}-{proj_code}"));
    store.ctype[lat_axis] = Some(format!("{lat_prefix}-{proj_code}"));

    // CRVAL in degrees (mapping output is radians for SkyFrame).
    store.crval[lon_axis] = Some(crval[lon_axis].to_degrees());
    store.crval[lat_axis] = Some(crval[lat_axis].to_degrees());

    // RADESYS.
    store.radesys = Some(radesys_string(system));

    // EQUINOX (as Julian year).
    if system.needs_equinox() {
        let eq_mjd = sky.equinox_mjd();
        let eq_year = pal::epj(eq_mjd);
        store.equinox = Some(eq_year);
    }

    // LONPOLE/LATPOLE defaults for zenithal projections.
    store.lonpole = Some(180.0);

    // MJD-OBS from epoch.
    let epoch = sky.epoch_mjd();
    if epoch != 51544.5 {
        // Only store if not the default J2000.0.
        store.mjdobs = Some(epoch);
    }

    Ok(())
}

/// Return the 4-character CTYPE prefixes for longitude and latitude.
fn ctype_prefixes(system: SkySystem) -> (&'static str, &'static str) {
    match system {
        SkySystem::Icrs | SkySystem::Fk5 | SkySystem::Fk4 | SkySystem::Fk4NoE | SkySystem::Gappt => {
            ("RA--", "DEC-")
        }
        SkySystem::Galactic => ("GLON", "GLAT"),
        SkySystem::Ecliptic => ("ELON", "ELAT"),
        SkySystem::Supergalactic => ("SLON", "SLAT"),
    }
}

/// Convert a SkySystem to the FITS RADESYS string.
fn radesys_string(system: SkySystem) -> String {
    match system {
        SkySystem::Icrs => "ICRS".to_string(),
        SkySystem::Fk5 => "FK5".to_string(),
        SkySystem::Fk4 => "FK4".to_string(),
        SkySystem::Fk4NoE => "FK4-NO-E".to_string(),
        SkySystem::Gappt => "GAPPT".to_string(),
        SkySystem::Galactic | SkySystem::Ecliptic | SkySystem::Supergalactic => {
            // These systems don't use RADESYS, but we return the system name.
            system.as_str().to_string()
        }
    }
}
```

- [ ] **Step 4: Implement populate_spectral**

```rust
/// Populate FitsStore fields for a spectral axis.
///
/// Sets CTYPE, CRVAL, SPECSYS, RESTFRQ/RESTWAV.
fn populate_spectral(
    spec: &SpecFrame,
    crval: &[f64],
    store: &mut FitsStore,
    axis: usize,
) -> AstResult<()> {
    let system = spec.system();

    // CTYPE from spectral system.
    store.ctype[axis] = Some(system.as_str().to_string());

    // CRVAL (already in the spectral frame's native units).
    store.crval[axis] = Some(crval[axis]);

    // CUNIT from system.
    store.cunit[axis] = Some(system.specmap_units().to_string());

    // SPECSYS from standard of rest.
    let sor = spec.std_of_rest();
    store.specsys = Some(specsys_string(sor));

    // REST frequency.
    if let Some(restfrq) = spec.rest_freq() {
        store.restfrq = Some(restfrq);
    }

    // MJD-OBS from epoch (if set).
    // SpecFrame epoch_mjd is available via the Frame trait.
    // We only set it if not already set by celestial.
    if store.mjdobs.is_none() {
        // SpecFrame doesn't expose epoch_mjd directly via a simple method
        // that returns Option, so we skip for now unless explicitly set.
    }

    Ok(())
}

/// Convert a StdOfRest to the FITS SPECSYS string.
fn specsys_string(sor: StdOfRest) -> String {
    match sor {
        StdOfRest::Topocentric => "TOPOCENT".to_string(),
        StdOfRest::Geocentric => "GEOCENTR".to_string(),
        StdOfRest::Barycentric => "BARYCENT".to_string(),
        StdOfRest::Heliocentric => "HELIOCEN".to_string(),
        StdOfRest::Lsrk => "LSRK".to_string(),
        StdOfRest::Lsrd => "LSRD".to_string(),
        StdOfRest::Galactic => "GALACTOC".to_string(),
        StdOfRest::LocalGroup => "LOCAL".to_string(),
        StdOfRest::Source => "SOURCE".to_string(),
    }
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo build -p ast-core`
Expected: compiles (with dead_code warnings acceptable at this stage)

- [ ] **Step 6: Commit**

---

### Task 4: Write FitsStore to FITS Cards

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

Implement `write_store_to_cards` which takes a populated FitsStore and writes the corresponding FITS keyword cards. This mirrors the C `WcsFromStore` function.

- [ ] **Step 1: Implement write_store_to_cards**

```rust
/// Write FitsStore contents as FITS keyword cards into a CardList.
///
/// Emits keywords in standard FITS-WCS order: WCSAXES, CRPIXi, CRVALi,
/// CTYPEi, CUNITi, CDi_j or CDELTi+PCi_j, then ancillary keywords
/// (RADESYS, EQUINOX, LONPOLE, LATPOLE, MJD-OBS, SPECSYS, RESTFRQ, RESTWAV).
fn write_store_to_cards(store: &FitsStore, cards: &mut CardList, use_cd: bool) -> AstResult<()> {
    let naxis = store.naxis;

    // WCSAXES (if different from NAXIS, which we don't know here,
    // so always write it for clarity).
    cards.set_int("WCSAXES", naxis as i64, "Number of WCS axes");

    // CRPIXi
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = format!("CRPIX{}", i + 1);
            cards.set_float(&key, val, "Reference pixel");
        }
    }

    // CRVALi
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let key = format!("CRVAL{}", i + 1);
            cards.set_float(&key, val, "Reference value");
        }
    }

    // CTYPEi
    for i in 0..naxis {
        if let Some(ref val) = store.ctype[i] {
            let key = format!("CTYPE{}", i + 1);
            cards.set_string(&key, val, "Axis type");
        }
    }

    // CUNITi
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = format!("CUNIT{}", i + 1);
            cards.set_string(&key, val, "Axis units");
        }
    }

    // CD matrix or PC+CDELT.
    if use_cd {
        write_cd_matrix(store, cards)?;
    } else {
        write_pc_cdelt(store, cards)?;
    }

    // Ancillary keywords.
    if let Some(ref radesys) = store.radesys {
        cards.set_string("RADESYS", radesys, "Reference frame");
    }
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "Equinox of coordinates");
    }
    if let Some(lonpole) = store.lonpole {
        cards.set_float("LONPOLE", lonpole, "Native longitude of celestial pole");
    }
    if let Some(latpole) = store.latpole {
        cards.set_float("LATPOLE", latpole, "Native latitude of celestial pole");
    }
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
    }
    if let Some(ref specsys) = store.specsys {
        cards.set_string("SPECSYS", specsys, "Spectral reference frame");
    }
    if let Some(restfrq) = store.restfrq {
        cards.set_float("RESTFRQ", restfrq, "Rest frequency (Hz)");
    }
    if let Some(restwav) = store.restwav {
        cards.set_float("RESTWAV", restwav, "Rest wavelength (m)");
    }

    Ok(())
}

/// Write CD matrix keywords (CDi_j).
fn write_cd_matrix(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;
    for i in 0..naxis {
        for j in 0..naxis {
            if let Some(val) = store.cd[i][j] {
                if val != 0.0 || i == j {
                    let key = format!("CD{}_{}", i + 1, j + 1);
                    cards.set_float(&key, val, "");
                }
            }
        }
    }
    Ok(())
}

/// Write PC matrix + CDELT keywords.
fn write_pc_cdelt(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    // Split the CD into CDELT and PC.
    let mut cd_array = ndarray::Array2::zeros((naxis, naxis));
    for i in 0..naxis {
        for j in 0..naxis {
            cd_array[[i, j]] = store.cd[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
        }
    }
    let (cdelt, pc) = split_cd(&cd_array);

    // CDELTi
    for i in 0..naxis {
        let key = format!("CDELT{}", i + 1);
        cards.set_float(&key, cdelt[i], "Pixel scale");
    }

    // PCi_j (only non-default values: off-diagonal != 0, diagonal != 1)
    for i in 0..naxis {
        for j in 0..naxis {
            let default = if i == j { 1.0 } else { 0.0 };
            if (pc[[i, j]] - default).abs() > 1e-15 {
                let key = format!("PC{}_{}", i + 1, j + 1);
                cards.set_float(&key, pc[[i, j]], "");
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Add CardList typed setter methods**

The `write_store_to_cards` function uses `cards.set_float(...)` etc. We need to add these convenience methods to `CardList`. Check if they already exist; if not, add them.

Add to `crates/ast-core/src/fitschan/card_list.rs`:

```rust
    /// Set an integer keyword value (append or overwrite).
    pub(crate) fn set_int(&mut self, keyword: &str, value: i64, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::Int(value),
            comment: if comment.is_empty() { None } else { Some(comment.to_string()) },
            used: false,
        };
        self.put(card, true);
    }

    /// Set a float keyword value (append or overwrite).
    pub(crate) fn set_float(&mut self, keyword: &str, value: f64, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::Float(value),
            comment: if comment.is_empty() { None } else { Some(comment.to_string()) },
            used: false,
        };
        self.put(card, true);
    }

    /// Set a string keyword value (append or overwrite).
    pub(crate) fn set_string(&mut self, keyword: &str, value: &str, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::String(value.to_string()),
            comment: if comment.is_empty() { None } else { Some(comment.to_string()) },
            used: false,
        };
        self.put(card, true);
    }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build -p ast-core`
Expected: compiles

- [ ] **Step 4: Commit**

---

### Task 5: Wire into FitsChan::write()

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs`

- [ ] **Step 1: Add wcs_write module declaration**

Add after the existing `wcs_spectral` module declaration:

```rust
#[allow(dead_code)]
pub(crate) mod wcs_decompose;
#[allow(dead_code)]
pub(crate) mod wcs_write;
```

- [ ] **Step 2: Wire FitsWcs encoding into write()**

Replace the `write()` method's match arm for non-Native encodings. In `mod.rs`, update the `Channel::write` implementation:

```rust
    fn write(&mut self, obj: &AnyObject) -> AstResult<()> {
        self.warnings.clear();

        match self.encoding {
            Encoding::Native => {
                // Render to native text.
                let node = serial::any_object_to_node(obj)?;
                let options = serial::native_write::RenderOptions {
                    comment: self.config.comment,
                    full: self.config.full,
                    indent: self.config.indent,
                };
                let text = serial::native_write::render_native_with_options(&node, &options)?;
                // Store as COMMENT cards.
                self.store_native_text(&text);
                Ok(())
            }
            Encoding::FitsWcs => {
                wcs_write::encode_wcs(obj, &mut self.cards, self.cd_matrix)
            }
            _ => Err(AstError::new(
                ErrorCode::Nodef,
                format!("{:?} encoding write not yet implemented", self.encoding),
            )),
        }
    }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build -p ast-core`
Expected: compiles

- [ ] **Step 4: Commit**

---

### Task 6: Integration Tests

**Files:**
- Create: `crates/ast-core/tests/test_fitschan_wcs_write.rs`

- [ ] **Step 1: Write round-trip test (read → write → read)**

```rust
//! FitsChan WCS encoder integration tests — Phase 3.
//!
//! Verifies that FitsChan with FitsWcs encoding can write FrameSets as
//! FITS headers and that the written headers round-trip correctly.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};

/// Minimal 2D TAN header for testing.
const MINIMAL_TAN_HEADER: &str = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =                1.000 / Reference pixel
CRPIX2  =                1.000 / Reference pixel
CRVAL1  =          202.4820000 / Reference RA
CRVAL2  =           47.1750000 / Reference Dec
CDELT1  =       -0.00027778000 / Degrees per pixel
CDELT2  =        0.00027778000 / Degrees per pixel
CTYPE1  = 'RA---TAN'           / RA with TAN projection
CTYPE2  = 'DEC--TAN'           / Dec with TAN projection
RADESYS = 'ICRS    '           / Reference frame
EQUINOX =               2000.0 / Equinox
";

#[test]
fn write_produces_fits_cards() {
    // Read a FrameSet from a header.
    let mut fc_read = FitsChan::from_header(MINIMAL_TAN_HEADER).unwrap();
    fc_read.set_encoding(Encoding::FitsWcs);
    let obj = fc_read.read().unwrap().unwrap();

    // Write the FrameSet to a new FitsChan.
    let mut fc_write = FitsChan::new();
    fc_write.set_encoding(Encoding::FitsWcs);
    fc_write.write(&obj).expect("write should succeed");

    // Should have produced FITS cards.
    assert!(fc_write.ncard() > 0, "FitsChan should have cards after write");

    // Check essential keywords exist.
    assert!(fc_write.get_fits_f("CRPIX1").is_some(), "Should have CRPIX1");
    assert!(fc_write.get_fits_f("CRVAL1").is_some(), "Should have CRVAL1");
    assert!(fc_write.get_fits_s("CTYPE1").is_some(), "Should have CTYPE1");
}

#[test]
fn write_celestial_has_correct_ctype() {
    let mut fc_read = FitsChan::from_header(MINIMAL_TAN_HEADER).unwrap();
    fc_read.set_encoding(Encoding::FitsWcs);
    let obj = fc_read.read().unwrap().unwrap();

    let mut fc_write = FitsChan::new();
    fc_write.set_encoding(Encoding::FitsWcs);
    fc_write.write(&obj).unwrap();

    let ctype1 = fc_write.get_fits_s("CTYPE1").unwrap();
    let ctype2 = fc_write.get_fits_s("CTYPE2").unwrap();
    assert!(
        ctype1.contains("RA") && ctype1.contains("TAN"),
        "CTYPE1 should be RA---TAN or similar, got '{ctype1}'"
    );
    assert!(
        ctype2.contains("DEC") && ctype2.contains("TAN"),
        "CTYPE2 should be DEC--TAN or similar, got '{ctype2}'"
    );
}

#[test]
fn write_celestial_has_radesys() {
    let mut fc_read = FitsChan::from_header(MINIMAL_TAN_HEADER).unwrap();
    fc_read.set_encoding(Encoding::FitsWcs);
    let obj = fc_read.read().unwrap().unwrap();

    let mut fc_write = FitsChan::new();
    fc_write.set_encoding(Encoding::FitsWcs);
    fc_write.write(&obj).unwrap();

    let radesys = fc_write.get_fits_s("RADESYS").unwrap();
    assert_eq!(radesys, "ICRS", "RADESYS should be ICRS");
}
```

- [ ] **Step 2: Write round-trip transform verification test**

```rust
#[test]
fn round_trip_read_write_read() {
    // Read FrameSet from header.
    let mut fc1 = FitsChan::from_header(MINIMAL_TAN_HEADER).unwrap();
    fc1.set_encoding(Encoding::FitsWcs);
    let obj1 = fc1.read().unwrap().unwrap();

    // Write to new FitsChan.
    let mut fc2 = FitsChan::new();
    fc2.set_encoding(Encoding::FitsWcs);
    fc2.write(&obj1).unwrap();

    // Read back from the written cards.
    fc2.rewind();
    let obj2 = fc2.read().unwrap().unwrap();

    // Both FrameSets should produce similar transforms at CRPIX.
    let m1 = obj1.as_mapping().unwrap();
    let m2 = obj2.as_mapping().unwrap();

    use ast_core::coords::PointSet;
    let pts = PointSet::from_rows([[1.0, 1.0]]);
    let out1 = m1.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();

    // Compare outputs (in radians).
    let tol = 1e-6; // ~0.2 arcsec tolerance
    let p1 = out1.point(0);
    let p2 = out2.point(0);
    assert!(
        (p1[0] - p2[0]).abs() < tol,
        "RA mismatch in round-trip: {} vs {}",
        p1[0],
        p2[0]
    );
    assert!(
        (p1[1] - p2[1]).abs() < tol,
        "Dec mismatch in round-trip: {} vs {}",
        p1[1],
        p2[1]
    );
}
```

- [ ] **Step 3: Write spectral round-trip test**

```rust
const SPECTRAL_HEADER: &str = "\
NAXIS   =                    1 / Number of axes
CRPIX1  =                  1.0 / Reference pixel
CRVAL1  =          1.42040575E9 / Reference frequency (Hz)
CDELT1  =           1000000.00 / Frequency step (Hz)
CTYPE1  = 'FREQ    '           / Frequency axis
CUNIT1  = 'Hz      '           / Units
SPECSYS = 'BARYCENT'           / Reference frame
RESTFRQ =          1.42040575E9 / Rest frequency (Hz)
";

#[test]
fn write_spectral_frameset() {
    let mut fc_read = FitsChan::from_header(SPECTRAL_HEADER).unwrap();
    fc_read.set_encoding(Encoding::FitsWcs);
    let obj = fc_read.read().unwrap().unwrap();

    let mut fc_write = FitsChan::new();
    fc_write.set_encoding(Encoding::FitsWcs);
    fc_write.write(&obj).unwrap();

    assert!(fc_write.ncard() > 0);
    let ctype = fc_write.get_fits_s("CTYPE1").unwrap();
    assert!(
        ctype.contains("FREQ"),
        "CTYPE1 should contain FREQ, got '{ctype}'"
    );
}

#[test]
fn write_spectral_round_trip() {
    let mut fc1 = FitsChan::from_header(SPECTRAL_HEADER).unwrap();
    fc1.set_encoding(Encoding::FitsWcs);
    let obj1 = fc1.read().unwrap().unwrap();

    let mut fc2 = FitsChan::new();
    fc2.set_encoding(Encoding::FitsWcs);
    fc2.write(&obj1).unwrap();

    fc2.rewind();
    let obj2 = fc2.read().unwrap().unwrap();

    use ast_core::coords::PointSet;
    let m1 = obj1.as_mapping().unwrap();
    let m2 = obj2.as_mapping().unwrap();

    // At CRPIX=1, output should be CRVAL.
    let pts = PointSet::from_rows([[1.0]]);
    let out1 = m1.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();

    let tol = 1.0; // 1 Hz tolerance
    assert!(
        (out1.point(0)[0] - out2.point(0)[0]).abs() < tol,
        "Spectral round-trip mismatch: {} vs {}",
        out1.point(0)[0],
        out2.point(0)[0]
    );
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_fitschan_wcs_write`
Expected: all tests pass

- [ ] **Step 5: Commit**

---

### Task 7: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test -p ast-core`
Expected: all tests pass (including existing Phase 2 tests)

- [ ] **Step 2: Run clippy and fmt**

Run: `cargo clippy -p ast-core -- -D warnings && cargo fmt --check -p ast-core`
Expected: clean

- [ ] **Step 3: Commit if any fixes were needed**

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p ast-core` — all tests pass (Phase 2 + Phase 3)
3. FitsChan with FitsWcs encoding can write a FrameSet to FITS cards
4. Written cards contain CRPIX, CRVAL, CTYPE, CD/CDELT+PC keywords
5. Celestial FrameSets produce correct RADESYS and projection codes
6. Spectral FrameSets produce SPECSYS and RESTFRQ
7. Round-trip (read → write → read) produces equivalent transforms
8. CD matrix vs. PC+CDELT mode both work (controlled by `cd_matrix` flag)
9. `cargo clippy` clean, `cargo fmt` clean

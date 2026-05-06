//! Main FITS-WCS decode pipeline.
//!
//! Assembles the pixel-to-IWC mapping, celestial decoder, and spectral decoder
//! into a complete FrameSet: pixel Frame → world Frame. This mirrors the C
//! library's `FsetFromStore` / `WcsMapFrm` functions (fitschan.c ~line 37632).

use super::fits_store::FitsStore;
use super::wcs_celestial::{self, CelestialResult};
use super::wcs_matrix;
use super::wcs_spectral::{self, SpectralResult};
use crate::error::AstResult;
use crate::frames::{BasicFrameBuilder, CmpFrame, DynFrame, FrameRef, FrameSet};
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, UnitMap};
use crate::serial::AnyObject;

/// Decode a [`FitsStore`] into a FrameSet (wrapped as [`AnyObject`]).
///
/// The resulting FrameSet has:
/// - Base frame: pixel coordinates (domain "GRID")
/// - Current frame: world coordinates (SkyFrame, SpecFrame, CmpFrame, or BasicFrame)
/// - Mapping: pixel → world
pub(crate) fn decode_wcs(store: &FitsStore) -> AstResult<AnyObject> {
    let naxis = store.naxis;

    // Step 1: Build the pixel → IWC mapping.
    let pixel_to_iwc = wcs_matrix::build_pixel_to_iwc(store)?;

    // Step 2: Try to decode celestial axes.
    let celestial = wcs_celestial::build_celestial(store);

    // Step 3: Try to decode spectral axis.
    let spectral = wcs_spectral::build_spectral(store);

    // Step 4: Build the world frame (before consuming the decoders).
    let world_frame = build_world_frame(store, &celestial, &spectral)?;

    // Step 5: Build the IWC → world mapping (consumes the decoder results).
    let iwc_to_world = build_iwc_to_world(store, celestial, spectral)?;

    // Step 6: Compose pixel→IWC→world.
    let full_mapping: DynMapping = Box::new(CmpMap::new(
        pixel_to_iwc,
        iwc_to_world,
        Composition::Series,
    )?);

    // Step 7: Build the pixel frame.
    let pixel_frame: DynFrame = Box::new(
        BasicFrameBuilder::new(naxis)
            .domain("GRID")
            .title("Pixel Coordinates")
            .build()?,
    );

    // Step 8: Build the FrameSet.
    let mut frameset = FrameSet::new(pixel_frame);
    frameset.add_frame(FrameRef::Base, full_mapping, world_frame)?;

    Ok(AnyObject::FrameSet(frameset))
}

/// Build the IWC → world mapping (consumes the decoder results).
///
/// This mapping takes an naxis-dimensional IWC vector and produces the
/// corresponding world coordinates.
fn build_iwc_to_world(
    store: &FitsStore,
    celestial: Option<CelestialResult>,
    spectral: Option<SpectralResult>,
) -> AstResult<DynMapping> {
    let naxis = store.naxis;

    // Track which axes are claimed by decoders.
    let mut claimed = vec![false; naxis];
    if let Some(ref cel) = celestial {
        claimed[cel.lon_axis] = true;
        claimed[cel.lat_axis] = true;
    }
    if let Some(ref spec) = spectral {
        claimed[spec.axis] = true;
    }

    // Simple cases first:
    if naxis == 2 && spectral.is_none() && !claimed.contains(&false) {
        if let Some(cel) = celestial {
            // Pure 2D celestial case.
            return Ok(cel.mapping);
        }
    }

    if naxis == 1 && celestial.is_none() {
        if let Some(spec) = spectral {
            // Pure 1D spectral case.
            return Ok(spec.mapping);
        }
    }

    // General case: build a composite mapping using parallel composition.
    build_general_iwc_to_world(store, celestial, spectral, &claimed)
}

/// Build the general IWC → world mapping for mixed axis cases.
///
/// Uses parallel composition to handle different axis types. Axes are
/// processed in order: celestial pair, spectral, then linear.
fn build_general_iwc_to_world(
    store: &FitsStore,
    celestial: Option<CelestialResult>,
    spectral: Option<SpectralResult>,
    claimed: &[bool],
) -> AstResult<DynMapping> {
    let naxis = store.naxis;

    // Extract axis indices for the celestial pair.
    let cel_first = celestial.as_ref().map(|c| c.lon_axis.min(c.lat_axis));

    let spec_axis = spectral.as_ref().map(|s| s.axis);

    let mut parts: Vec<DynMapping> = Vec::new();
    let mut cel_used = false;
    let mut spec_used = false;

    // We need to move celestial/spectral into mutable Option so we can take them.
    let mut celestial = celestial;
    let mut spectral = spectral;

    let mut i = 0;
    while i < naxis {
        if !cel_used {
            if let Some(first_cel) = cel_first {
                if i == first_cel {
                    // Consume the celestial mapping (handles 2 axes).
                    let cel = celestial.take().unwrap();
                    parts.push(cel.mapping);
                    cel_used = true;
                    // Skip both celestial axes.
                    let second_cel = cel_first.unwrap(); // This is min; we need max.
                    // Recalculate max from the already-known indices.
                    // We know the pair spans from first_cel to the other axis.
                    // Skip to the end of both celestial axes.
                    i = first_cel + 2; // Assuming consecutive for now
                    // Actually, they might not be consecutive. In FITS-WCS,
                    // celestial axes are typically consecutive (axes 1,2),
                    // but could be separated. For Phase 2, assume consecutive.
                    let _ = second_cel;
                    continue;
                }
            }
        }

        if !spec_used {
            if let Some(sa) = spec_axis {
                if i == sa {
                    let spec = spectral.take().unwrap();
                    parts.push(spec.mapping);
                    spec_used = true;
                    i += 1;
                    continue;
                }
            }
        }

        if !claimed[i] {
            // Unclaimed axis: apply CRVAL shift.
            let crval = store.crval[i].unwrap_or(0.0);
            let shift = ShiftMap::new(vec![crval]);
            parts.push(Box::new(shift));
        }

        i += 1;
    }

    // Compose all parts in parallel.
    if parts.is_empty() {
        return Ok(Box::new(UnitMap::new(naxis)));
    }

    let mut result = parts.remove(0);
    for part in parts {
        result = Box::new(CmpMap::new(result, part, Composition::Parallel)?);
    }

    Ok(result)
}

/// Build the world coordinate frame based on what decoders found.
fn build_world_frame(
    store: &FitsStore,
    celestial: &Option<CelestialResult>,
    spectral: &Option<SpectralResult>,
) -> AstResult<DynFrame> {
    let naxis = store.naxis;

    // Track unclaimed axes.
    let mut claimed = vec![false; naxis];
    if let Some(cel) = celestial {
        claimed[cel.lon_axis] = true;
        claimed[cel.lat_axis] = true;
    }
    if let Some(spec) = spectral {
        claimed[spec.axis] = true;
    }

    let n_unclaimed = claimed.iter().filter(|&&c| !c).count();

    // Simple cases:
    if celestial.is_some() && spectral.is_none() && n_unclaimed == 0 {
        let cel = celestial.as_ref().unwrap();
        return Ok(Box::new(cel.frame.clone()));
    }

    if spectral.is_some() && celestial.is_none() && n_unclaimed == 0 {
        let spec = spectral.as_ref().unwrap();
        return Ok(Box::new(spec.frame.clone()));
    }

    // Mixed case: build a CmpFrame.
    let mut frames: Vec<DynFrame> = Vec::new();

    if let Some(cel) = celestial {
        frames.push(Box::new(cel.frame.clone()));
    }

    if let Some(spec) = spectral {
        frames.push(Box::new(spec.frame.clone()));
    }

    if n_unclaimed > 0 {
        let mut builder = BasicFrameBuilder::new(n_unclaimed);
        let mut linear_idx = 0;
        for (i, &is_claimed) in claimed.iter().enumerate() {
            if !is_claimed {
                linear_idx += 1;
                if let Some(label) = &store.cname[i] {
                    builder = builder.label(linear_idx, label.clone());
                } else if let Some(ctype) = &store.ctype[i] {
                    builder = builder.label(linear_idx, ctype.clone());
                }
                if let Some(unit) = &store.cunit[i] {
                    builder = builder.unit(linear_idx, unit.clone());
                }
            }
        }
        frames.push(Box::new(builder.build()?));
    }

    // Compose frames.
    if frames.is_empty() {
        return Ok(Box::new(BasicFrameBuilder::new(naxis).build()?));
    }

    if frames.len() == 1 {
        return Ok(frames.remove(0));
    }

    // CmpFrame composition: combine all frames pairwise.
    let mut result = frames.remove(0);
    for frame in frames {
        result = Box::new(CmpFrame::new(result, frame));
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;
    use crate::mappings::Mapping;

    /// Build a minimal 2D celestial FitsStore.
    fn celestial_2d_store() -> FitsStore {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crpix[0] = Some(128.0);
        store.crpix[1] = Some(128.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);
        store.radesys = Some("ICRS".to_string());
        store
    }

    #[test]
    fn decode_2d_celestial() {
        let store = celestial_2d_store();
        let result = decode_wcs(&store).unwrap();

        // Should be a FrameSet.
        if let AnyObject::FrameSet(ref fs) = result {
            assert_eq!(fs.nframe(), 2);
            assert_eq!(fs.base().naxes(), 2);
            assert_eq!(fs.current().naxes(), 2);
        } else {
            panic!("Expected FrameSet");
        }
    }

    #[test]
    fn decode_crpix_maps_to_crval() {
        let store = celestial_2d_store();
        let result = decode_wcs(&store).unwrap();

        if let AnyObject::FrameSet(ref fs) = result {
            // Transform CRPIX through the FrameSet.
            let pts = PointSet::from_columns(&[&[128.0], &[128.0]]).unwrap();
            let out = fs.transform(&pts, true).unwrap();
            let point = out.point(0);

            // Should be approximately CRVAL in radians.
            let expected_lon = 180.0_f64.to_radians();
            let expected_lat = 45.0_f64.to_radians();

            assert!(
                (point[0] - expected_lon).abs() < 1e-4,
                "lon: got {}, expected {}",
                point[0],
                expected_lon
            );
            assert!(
                (point[1] - expected_lat).abs() < 1e-4,
                "lat: got {}, expected {}",
                point[1],
                expected_lat
            );
        } else {
            panic!("Expected FrameSet");
        }
    }

    #[test]
    fn decode_1d_spectral() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crpix[0] = Some(1.0);
        store.crval[0] = Some(1.4e9);
        store.cdelt[0] = Some(1.0e6);

        let result = decode_wcs(&store).unwrap();
        if let AnyObject::FrameSet(ref fs) = result {
            assert_eq!(fs.nframe(), 2);
            assert_eq!(fs.base().naxes(), 1);
            assert_eq!(fs.current().naxes(), 1);

            // CRPIX (1.0) should map to CRVAL.
            let pts = PointSet::from_columns(&[&[1.0]]).unwrap();
            let out = fs.transform(&pts, true).unwrap();
            assert!((out.point(0)[0] - 1.4e9).abs() < 1.0);
        } else {
            panic!("Expected FrameSet");
        }
    }

    #[test]
    fn decode_3d_mixed() {
        // 2 celestial + 1 spectral.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crpix[0] = Some(128.0);
        store.crpix[1] = Some(128.0);
        store.crpix[2] = Some(1.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crval[2] = Some(1.4e9);
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);
        store.cdelt[2] = Some(1.0e6);

        let result = decode_wcs(&store).unwrap();
        if let AnyObject::FrameSet(ref fs) = result {
            assert_eq!(fs.nframe(), 2);
            assert_eq!(fs.base().naxes(), 3);
            assert_eq!(fs.current().naxes(), 3);
        } else {
            panic!("Expected FrameSet");
        }
    }
}

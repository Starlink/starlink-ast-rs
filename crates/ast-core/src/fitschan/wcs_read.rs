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
/// Uses parallel composition to handle different axis types.  Axes are
/// walked one at a time in FITS index order.  The celestial mapping is
/// pushed when the FIRST of the celestial pair is encountered; the second
/// is skipped.  After parallel composition a `PermMap` is appended when
/// the natural parallel-output order doesn't match FITS axis order (e.g.
/// when the celestial pair is non-adjacent, as in axes-swap.head).
fn build_general_iwc_to_world(
    store: &FitsStore,
    celestial: Option<CelestialResult>,
    spectral: Option<SpectralResult>,
    claimed: &[bool],
) -> AstResult<DynMapping> {
    use crate::mappings::PermMap;

    let naxis = store.naxis;

    // Walk axes in FITS index order.  For each axis push the appropriate
    // mapping into `parts`; record which FITS-axis-index each of that
    // mapping's outputs corresponds to in `output_to_fits` (length =
    // total output axes after parallel composition).
    let mut parts: Vec<DynMapping> = Vec::new();
    let mut output_to_fits: Vec<usize> = Vec::new();

    let mut celestial = celestial;
    let mut spectral = spectral;

    for (i, &is_claimed) in claimed.iter().enumerate() {
        // Celestial axis: emit the celestial mapping when we hit the
        // FIRST of the celestial pair (in FITS order); skip the second
        // encounter.
        if let Some(cel) = celestial.as_ref() {
            let cel_first = cel.lon_axis.min(cel.lat_axis);
            let cel_second = cel.lon_axis.max(cel.lat_axis);
            if i == cel_first {
                let cel = celestial.take().unwrap();
                let lon_axis = cel.lon_axis;
                let lat_axis = cel.lat_axis;
                parts.push(cel.mapping);
                // `build_celestial` adapts the mapping so its outputs
                // come out in FITS axis order: when lat_axis <
                // lon_axis the wrapped mapping emits (cel_lat,
                // cel_lon) at slots (0, 1); otherwise it stays at
                // (cel_lon, cel_lat). Match that by pushing
                // `output_to_fits` in the same order.
                if lat_axis < lon_axis {
                    output_to_fits.push(lat_axis);
                    output_to_fits.push(lon_axis);
                } else {
                    output_to_fits.push(lon_axis);
                    output_to_fits.push(lat_axis);
                }
                continue;
            }
            if i == cel_second {
                continue; // already handled above
            }
        }

        // Spectral axis.
        if let Some(spec) = spectral.as_ref() {
            if i == spec.axis {
                let spec = spectral.take().unwrap();
                let axis = spec.axis;
                parts.push(spec.mapping);
                output_to_fits.push(axis);
                continue;
            }
        }

        // Linear/unclaimed axis: emit a CRVAL shift.
        if !is_claimed {
            let crval = store.crval[i].unwrap_or(0.0);
            let shift = ShiftMap::new(vec![crval]);
            parts.push(Box::new(shift));
            output_to_fits.push(i);
        }
    }

    if parts.is_empty() {
        return Ok(Box::new(UnitMap::new(naxis)));
    }

    // Compose all parts in parallel.
    let mut result: DynMapping = parts.remove(0);
    for part in parts {
        result = Box::new(CmpMap::new(result, part, Composition::Parallel)?);
    }

    // If the parallel-output order differs from FITS axis order, append a
    // PermMap to permute parallel-output back to FITS axis order.
    let identity_order: Vec<usize> = (0..naxis).collect();
    if output_to_fits != identity_order {
        // Build forward permutation: for each FITS axis position `dest`,
        // find which parallel-output position holds that value.
        let mut outperm = vec![0i64; naxis];
        for (dest, entry) in outperm.iter_mut().enumerate() {
            let src = output_to_fits
                .iter()
                .position(|&fa| fa == dest)
                .ok_or_else(|| {
                    crate::error::AstError::new(
                        crate::error::ErrorCode::Bdfts,
                        format!(
                            "internal: FITS axis {dest} not produced by any decoder; \
                             output_to_fits = {output_to_fits:?}"
                        ),
                    )
                })?;
            *entry = (src + 1) as i64;
        }
        // Inverse permutation: for each parallel-output position `src`,
        // record which FITS axis it carries (1-based).
        let inperm: Vec<i64> = output_to_fits
            .iter()
            .map(|&dest_fits| (dest_fits + 1) as i64)
            .collect();
        let perm = PermMap::new(&inperm, &outperm, &[])?;
        result = Box::new(CmpMap::new(result, Box::new(perm), Composition::Series)?);
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

    // Mixed case: walk axes in FITS index order and push each frame at
    // its first encounter. This mirrors `build_general_iwc_to_world`'s
    // walk so the world-frame axis order matches the mapping output
    // order — without it, an input header where the spectral axis
    // comes before the celestial pair (e.g. CTYPE1=FREQ, CTYPE2=RA,
    // CTYPE3=DEC for class-* fixtures) would build the world CmpFrame
    // as (Sky, Spec) while the IWC→world mapping outputs (FREQ, RA,
    // DEC), producing an axis-ordering mismatch in the FrameSet.
    //
    // Consecutive linear (unclaimed) axes are grouped into a single
    // BasicFrame, mirroring the original two-frame layout that the
    // writer's `identify_axes` expects (the writer's CmpFrame
    // dispatch only handles two non-nested children, so splitting
    // each linear axis into its own frame would break headers like
    // `freq-pixel3d` whose linear-axes group sits after the
    // spectral axis).
    let mut frames: Vec<DynFrame> = Vec::new();
    let mut cel_pushed = false;
    let mut spec_pushed = false;
    let mut linear_run: Vec<usize> = Vec::new();

    let flush_linear = |run: &mut Vec<usize>, frames: &mut Vec<DynFrame>| -> AstResult<()> {
        if run.is_empty() {
            return Ok(());
        }
        let mut builder = BasicFrameBuilder::new(run.len());
        for (k, &axis) in run.iter().enumerate() {
            let lidx_1based = k + 1;
            if let Some(label) = &store.cname[axis] {
                builder = builder.label(lidx_1based, label.clone());
            } else if let Some(ctype) = &store.ctype[axis] {
                builder = builder.label(lidx_1based, ctype.clone());
            }
            if let Some(unit) = &store.cunit[axis] {
                builder = builder.unit(lidx_1based, unit.clone());
            }
        }
        frames.push(Box::new(builder.build()?));
        run.clear();
        Ok(())
    };

    for (i, &is_claimed) in claimed.iter().enumerate() {
        if let Some(cel) = celestial.as_ref() {
            if i == cel.lon_axis || i == cel.lat_axis {
                flush_linear(&mut linear_run, &mut frames)?;
                if !cel_pushed {
                    frames.push(Box::new(cel.frame.clone()));
                    cel_pushed = true;
                }
                continue;
            }
        }
        if let Some(spec) = spectral.as_ref() {
            if i == spec.axis {
                flush_linear(&mut linear_run, &mut frames)?;
                if !spec_pushed {
                    frames.push(Box::new(spec.frame.clone()));
                    spec_pushed = true;
                }
                continue;
            }
        }
        if !is_claimed {
            linear_run.push(i);
        }
    }
    flush_linear(&mut linear_run, &mut frames)?;

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

    #[test]
    fn build_pixel_to_world_handles_non_adjacent_celestial_axes() {
        // FITS axes: 0=DEC, 1=WAVE, 2=RA. Celestial pair non-adjacent
        // (separated by spectral axis). Reader must produce a working
        // 3D pixel-to-world mapping.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("DEC--TAN".to_string());
        store.ctype[1] = Some("WAVE    ".to_string());
        store.ctype[2] = Some("RA---TAN".to_string());
        store.crval[0] = Some(22.0);
        store.crval[1] = Some(5.0e-7);
        store.crval[2] = Some(83.6);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(25.5);
        store.crpix[2] = Some(50.5);
        store.cdelt[0] = Some(2.78e-4);
        store.cdelt[1] = Some(1.0e-10);
        store.cdelt[2] = Some(-2.78e-4);
        store.cunit[1] = Some("m".to_string());

        let obj = decode_wcs(&store).expect("decode succeeds");
        let AnyObject::FrameSet(ref fs) = obj else {
            panic!("expected FrameSet")
        };
        assert_eq!(fs.base().naxes(), 3);
        assert_eq!(fs.current().naxes(), 3);

        // At the reference pixel, output axis 0 is DEC (latitude in
        // radians, since SkyFrame stores radians internally) ~ 22°,
        // and output axis 2 is RA (longitude in radians) ~ 83.6°.
        let pts = PointSet::from_columns(&[&[50.5], &[25.5], &[50.5]]).unwrap();
        let out = fs.transform(&pts, true).unwrap();
        let point = out.point(0);
        let dec_deg = point[0].to_degrees();
        let ra_deg = point[2].to_degrees();
        assert!(
            (dec_deg - 22.0).abs() < 1.0,
            "axis 0 (DEC) at refpix expected ~22°, got {dec_deg}°"
        );
        assert!(
            (ra_deg - 83.6).abs() < 1.0,
            "axis 2 (RA) at refpix expected ~83.6°, got {ra_deg}°"
        );
    }
}

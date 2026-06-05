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
#[cfg(test)]
pub(crate) fn decode_wcs(store: &FitsStore) -> AstResult<AnyObject> {
    decode_wcs_with_def_b1950(store, false)
}

/// Encoding-aware variant of [`decode_wcs`]. `def_b1950` mirrors the
/// FitsChan `DefB1950` attribute (fitschan.c:42288): true for every
/// FITS encoding except FITS-WCS. Forwards into
/// [`wcs_celestial::build_celestial_with_def_b1950`] which uses the
/// flag to drive the NORADEC fallback for alt SkyFrames with no
/// RADESYS / EQUINOX / MJD-OBS.
pub(crate) fn decode_wcs_with_def_b1950(
    store: &FitsStore,
    def_b1950: bool,
) -> AstResult<AnyObject> {
    let naxis = store.naxis;

    // Step 1: Build the pixel → IWC mapping.
    let pixel_to_iwc = wcs_matrix::build_pixel_to_iwc(store)?;

    // Step 2: Try to decode celestial axes.
    let celestial = wcs_celestial::build_celestial_with_def_b1950(store, def_b1950);

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

    // Step 9: SpecFrame RefRA/RefDec from celestial fiducial. Mirrors
    // C `astSetRefPos(specfrm, reffrm, reflon, reflat)` at
    // fitschan.c:39535. C calls this on the standalone SpecFrame
    // BEFORE the FrameSet wraps it, so no FrameSet-integrity remap
    // fires. We achieve the same semantics by using `frame_mut` to
    // bypass our `with_integrity` machinery, since by this point the
    // SpecFrame already lives inside the FrameSet.
    set_spec_ref_from_celestial(&mut frameset, store)?;

    Ok(AnyObject::FrameSet(frameset))
}

/// Propagate the celestial reference position to a SpecFrame's
/// `RefRA`/`RefDec` attributes (FK5 J2000 radians). No-op if the
/// world frame doesn't contain a SpecFrame and a SkyFrame, or if the
/// SpecFrame already has both attributes set.
fn set_spec_ref_from_celestial(fs: &mut FrameSet, store: &FitsStore) -> AstResult<()> {
    // Locate spec/sky child indices and frame-axis indices within the
    // current world frame. If either is absent, nothing to do.
    let Some(info) = locate_spec_sky(fs.current()) else {
        return Ok(());
    };

    // If RefRA/RefDec already set on the SpecFrame, skip (mirrors C's
    // `astTestRefRA`/`astTestRefDec` gate).
    {
        let cur = fs.current();
        if let Some(spec) = peek_spec(cur)
            && spec.ref_ra().is_some()
            && spec.ref_dec().is_some()
        {
            return Ok(());
        }
    }

    // Step 1: compute the celestial fiducial in the SkyFrame's native
    // system. Mirrors C `WcsCelestial` (fitschan.c:35487+) which
    // returns reflon/reflat as CRVAL1/CRVAL2 in radians, in the
    // SkyFrame's coordinate system (RADESYS-determined). Here we feed
    // CRPIX (1-based in FITS, so the same value here) through the
    // pixel→world mapping: at CRPIX, the pixel→IWC step yields IWC=0
    // on the celestial axes, the WcsMap inverse yields the native
    // fiducial, and the post-WcsMap rotation maps that to the
    // celestial fiducial in the SkyFrame's native system (i.e.
    // FK4/Galactic/etc, not yet converted to FK5 J2000).
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let naxis = store.naxis;
    let crpix: Vec<f64> = (0..naxis).map(|i| store.crpix[i].unwrap_or(1.0)).collect();

    use crate::coords::PointSet;
    let pts_in = PointSet::from_rows([crpix.as_slice()]);
    let pts_out = mapping.transform(&pts_in, true)?;
    let world_pt = pts_out.point(0).to_vec();
    let reflon = world_pt[info.world_lon_axis];
    let reflat = world_pt[info.world_lat_axis];

    // Step 2: clone the SkyFrame so we can hand it to `set_ref_pos`
    // as the reference frame. Mirrors C's
    // `astSetRefPos(specfrm, reffrm, reflon, reflat)` at
    // fitschan.c:39535, where `reffrm` is a clone of the SkyFrame
    // built by WcsCelestial. `set_ref_pos` performs the
    // SkyFrame→FK5/J2000 conversion internally.
    let sky_clone = peek_sky(fs.current())
        .expect("locate_spec_sky guarantees a SkyFrame")
        .clone();

    // Step 3: call `set_ref_pos` on the SpecFrame via `frame_mut`.
    // C calls `astSetRefPos` directly on the standalone specfrm
    // before the FrameSet wraps it; we apply the same direct-set
    // semantics post-construction via `frame_mut`. `set_ref_pos`
    // converts (reflon, reflat) from sky_clone's system to FK5/J2000
    // and stores the result in RefRA/RefDec.
    let cur = fs.frame_mut(FrameRef::Current)?;
    if let Some(spec) = spec_mut(cur) {
        spec.set_ref_pos(Some(&sky_clone), reflon, reflat)?;
    }

    Ok(())
}

#[derive(Debug)]
struct SpecSkyInfo {
    /// 0-based axis index of celestial longitude in the world frame.
    world_lon_axis: usize,
    /// 0-based axis index of celestial latitude in the world frame.
    world_lat_axis: usize,
}

fn locate_spec_sky(frame: &dyn crate::frames::Frame) -> Option<SpecSkyInfo> {
    use crate::frames::{CmpFrame, SkyFrame, SpecFrame};
    let cmp = frame.as_any().downcast_ref::<CmpFrame>()?;
    // Search children for SkyFrame and SpecFrame at top level (the
    // typical layout produced by build_world_frame). For nested
    // CmpFrames recurse; this matches what we build, even if the
    // pure-C primary-frame walk is more general.
    fn find(
        frame: &dyn crate::frames::Frame,
        offset: usize,
        out: &mut (Option<usize>, Option<(usize, usize)>),
    ) {
        if let Some(_spec) = frame.as_any().downcast_ref::<SpecFrame>() {
            if out.0.is_none() {
                out.0 = Some(offset);
            }
        } else if let Some(_sky) = frame.as_any().downcast_ref::<SkyFrame>() {
            if out.1.is_none() {
                out.1 = Some((offset, offset + 1));
            }
        } else if let Some(c) = frame.as_any().downcast_ref::<CmpFrame>() {
            find(c.frame1(), offset, out);
            find(c.frame2(), offset + c.frame1().naxes(), out);
        }
    }
    let mut out = (None, None);
    find(cmp.frame1(), 0, &mut out);
    find(cmp.frame2(), cmp.frame1().naxes(), &mut out);

    let _spec_axis = out.0?;
    let (lon, lat) = out.1?;
    Some(SpecSkyInfo {
        world_lon_axis: lon,
        world_lat_axis: lat,
    })
}

fn peek_spec(frame: &dyn crate::frames::Frame) -> Option<&crate::frames::SpecFrame> {
    use crate::frames::{CmpFrame, SpecFrame};
    if let Some(s) = frame.as_any().downcast_ref::<SpecFrame>() {
        return Some(s);
    }
    if let Some(c) = frame.as_any().downcast_ref::<CmpFrame>() {
        return peek_spec(c.frame1()).or_else(|| peek_spec(c.frame2()));
    }
    None
}

fn peek_sky(frame: &dyn crate::frames::Frame) -> Option<&crate::frames::SkyFrame> {
    use crate::frames::{CmpFrame, SkyFrame};
    if let Some(s) = frame.as_any().downcast_ref::<SkyFrame>() {
        return Some(s);
    }
    if let Some(c) = frame.as_any().downcast_ref::<CmpFrame>() {
        return peek_sky(c.frame1()).or_else(|| peek_sky(c.frame2()));
    }
    None
}

fn spec_mut(frame: &mut dyn crate::frames::Frame) -> Option<&mut crate::frames::SpecFrame> {
    use crate::frames::{CmpFrame, SpecFrame};
    if frame.as_any().downcast_ref::<SpecFrame>().is_some() {
        return frame.as_any_mut().downcast_mut::<SpecFrame>();
    }
    if let Some(cmp) = frame.as_any_mut().downcast_mut::<CmpFrame>() {
        // Pick child up-front to avoid borrowing `cmp` twice.
        let pick_first = peek_spec(cmp.frame1()).is_some();
        if pick_first {
            return spec_mut(cmp.frame1_mut());
        }
        return spec_mut(cmp.frame2_mut());
    }
    None
}

/// Decode the primary plane plus zero or more alt planes into a
/// FrameSet whose base = pixel grid, current = primary world, and
/// extra frames = alt-plane world frames (one per detected version
/// letter).
///
/// Each alt frame is added with `add_frame(FrameRef::Base, …)`, so
/// the native dump shows `Lnk{i}=1` for every alt — matching the C
/// library's structure exactly. Each alt frame's `Ident` is the
/// version letter; the writer reads this back to recover the
/// per-plane suffix on round-trip.
///
/// Mirrors fitschan.c::WcsFromStore + the per-version `WcsSkyFrame`
/// loop.
pub(crate) fn decode_wcs_with_alts(
    primary: &FitsStore,
    alts: &[(char, FitsStore)],
) -> AstResult<AnyObject> {
    decode_wcs_with_alts_def_b1950(primary, alts, false)
}

/// Encoding-aware variant. See [`decode_wcs_with_def_b1950`].
pub(crate) fn decode_wcs_with_alts_def_b1950(
    primary: &FitsStore,
    alts: &[(char, FitsStore)],
    def_b1950: bool,
) -> AstResult<AnyObject> {
    // Step 1: build the primary FrameSet via the existing decoder.
    let primary_obj = decode_wcs_with_def_b1950(primary, def_b1950)?;
    let mut fs = match primary_obj {
        AnyObject::FrameSet(f) => f,
        _ => unreachable!("decode_wcs always returns AnyObject::FrameSet"),
    };

    // Remember the primary current frame index so we can restore it
    // after add_frame moves current to each newly-appended alt.
    // After the primary is built, it sits at frame index 2.
    let primary_index = 2;

    // Step 2: append one alt frame per (letter, alt_store).
    for (letter, alt_store) in alts {
        let pixel_to_iwc = wcs_matrix::build_pixel_to_iwc(alt_store)?;
        let celestial = wcs_celestial::build_celestial_with_def_b1950(alt_store, def_b1950);
        let spectral = wcs_spectral::build_spectral(alt_store);
        let mut world_frame = build_world_frame(alt_store, &celestial, &spectral)?;
        let iwc_to_world = build_iwc_to_world(alt_store, celestial, spectral)?;
        let full_mapping: DynMapping = Box::new(CmpMap::new(
            pixel_to_iwc,
            iwc_to_world,
            Composition::Series,
        )?);

        // Stamp the version letter onto the alt frame so the writer
        // can recover it.
        set_frame_ident(world_frame.as_mut(), &letter.to_string());

        fs.add_frame(FrameRef::Base, full_mapping, world_frame)?;
    }

    // Restore current to the primary frame (add_frame moved it).
    if !alts.is_empty() {
        fs.set_current(FrameRef::Index(primary_index))?;
    }

    Ok(AnyObject::FrameSet(fs))
}

/// Helper: stamp `ident` onto a boxed Frame, dispatching by concrete
/// type. Falls back silently for frame variants we don't yet label.
fn set_frame_ident(frame: &mut dyn crate::frames::Frame, ident: &str) {
    use crate::frames::{BasicFrame, CmpFrame, FluxFrame, SkyFrame, SpecFrame, TimeFrame};
    use crate::mappings::AsAny;
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<BasicFrame>() {
        f.set_ident(ident);
        return;
    }
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<SkyFrame>() {
        f.set_ident(ident);
        return;
    }
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<SpecFrame>() {
        f.set_ident(ident);
        return;
    }
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<TimeFrame>() {
        f.set_ident(ident);
        return;
    }
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<CmpFrame>() {
        f.set_ident(ident);
        return;
    }
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<FluxFrame>() {
        f.set_ident(ident);
    }
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

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

    // Step 6: Compose pixel→IWC→world and simplify. C `WcsMapFrm`
    // returns `astSimplify(map8)` (fitschan.c:37810) — the pixel→WCS
    // Mapping is simplified before it is added to the FrameSet, so the
    // stored edge (and any later native dump of it) is in reduced form.
    let composed: DynMapping = Box::new(CmpMap::new(
        pixel_to_iwc,
        iwc_to_world,
        Composition::Series,
    )?);
    let full_mapping: DynMapping = composed.simplified()?;

    // Step 7: Build the pixel frame. C labels each pixel axis
    // "Pixel axis <n>" (fitschan.c:11935-11938).
    let mut pixel_builder = BasicFrameBuilder::new(naxis)
        .domain("GRID")
        .title("Pixel Coordinates");
    for i in 0..naxis {
        pixel_builder = pixel_builder.label(i + 1, format!("Pixel axis {}", i + 1));
    }
    let pixel_frame: DynFrame = Box::new(pixel_builder.build()?);

    // Step 8: Build the FrameSet.
    let mut frameset = FrameSet::new(pixel_frame);
    frameset.add_frame(FrameRef::Base, full_mapping, world_frame)?;

    // Explicitly fix the GRID (pixel) Frame as the base, mirroring C
    // `astSetBase(ret, pixel)` (fitschan.c:11990). This marks the Base
    // attribute as set, so a subsequent native dump writes `Base` as an
    // active value rather than a commented-out default.
    frameset.set_base(FrameRef::Index(1))?;

    // Step 9: SpecFrame RefRA/RefDec from celestial fiducial. Mirrors
    // C `astSetRefPos(specfrm, reffrm, reflon, reflat)` at
    // fitschan.c:39535. C calls this on the standalone SpecFrame
    // BEFORE the FrameSet wraps it, so no FrameSet-integrity remap
    // fires. We achieve the same semantics by using `frame_mut` to
    // bypass our `with_integrity` machinery, since by this point the
    // SpecFrame already lives inside the FrameSet.
    set_spec_ref_from_celestial(&mut frameset, store)?;

    // Step 10: DSBSpecFrame setup. Mirrors C `WcsSpectral`
    // (fitschan.c:39585-39588), which calls `DSBSetUp` immediately after
    // `astSetRefPos` when the spectral frame is a DSBSpecFrame. The IF /
    // DSBCentre / SideBand attributes depend on the topocentric frequency
    // conversion, which in turn depends on the reference position set above.
    dsb_set_up_in_frameset(&mut frameset, store)?;

    Ok(AnyObject::FrameSet(frameset))
}

/// Run [`crate::frames::DSBSpecFrame::dsb_set_up`] on the DSBSpecFrame (if
/// any) inside the FrameSet's current world frame.
///
/// Mirrors C `WcsSpectral`'s `if( astIsADSBSpecFrame(specfrm) ) DSBSetUp(...)`
/// (fitschan.c:39585-39588). The `obscentre` argument C passes as `crval` is
/// `store->axref[i]` falling back to `store->crval[i]` (fitschan.c:39575-39581).
fn dsb_set_up_in_frameset(fs: &mut FrameSet, store: &FitsStore) -> AstResult<()> {
    let Some(imagfreq) = store.imagfreq else {
        return Ok(());
    };

    // obscentre: the spectral CRVAL value (no AXREF support yet — AXREF is
    // only written for -TAB axes with no inverse, which this reader does
    // not produce).
    let Some(spec_axis) = store.spectral_axis() else {
        return Ok(());
    };
    let crval = store.crval[spec_axis].unwrap_or(0.0);

    let cur = fs.frame_mut(FrameRef::Current)?;
    dsb_set_up_in_frame(cur, imagfreq, crval)
}

/// Locate the `DSBSpecFrame` within `frame` (recursing through any
/// `CmpFrame`) and run its `dsb_set_up`.
fn dsb_set_up_in_frame(
    frame: &mut dyn crate::frames::Frame,
    imagfreq: f64,
    crval: f64,
) -> AstResult<()> {
    use crate::frames::{CmpFrame, DSBSpecFrame};
    if let Some(dsb) = frame.as_any_mut().downcast_mut::<DSBSpecFrame>() {
        return dsb.dsb_set_up(imagfreq, crval);
    }
    if let Some(cmp) = frame.as_any_mut().downcast_mut::<CmpFrame>() {
        let in_first = cmp.frame1().as_any().is::<DSBSpecFrame>() || contains_dsb(cmp.frame1());
        if in_first {
            return dsb_set_up_in_frame(cmp.frame1_mut(), imagfreq, crval);
        }
        return dsb_set_up_in_frame(cmp.frame2_mut(), imagfreq, crval);
    }
    Ok(())
}

/// Whether `frame` contains a `DSBSpecFrame` (directly or nested).
fn contains_dsb(frame: &dyn crate::frames::Frame) -> bool {
    use crate::frames::{CmpFrame, DSBSpecFrame};
    if frame.as_any().is::<DSBSpecFrame>() {
        return true;
    }
    if let Some(c) = frame.as_any().downcast_ref::<CmpFrame>() {
        return contains_dsb(c.frame1()) || contains_dsb(c.frame2());
    }
    false
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
    set_spec_ref_pos(cur, &sky_clone, reflon, reflat)?;

    Ok(())
}

/// Apply `set_ref_pos` to the spectral frame (plain `SpecFrame` or
/// `DSBSpecFrame`) found within `frame`, recursing through any `CmpFrame`.
fn set_spec_ref_pos(
    frame: &mut dyn crate::frames::Frame,
    sky: &crate::frames::SkyFrame,
    reflon: f64,
    reflat: f64,
) -> AstResult<()> {
    use crate::frames::{CmpFrame, DSBSpecFrame, SpecFrame};
    if let Some(spec) = frame.as_any_mut().downcast_mut::<SpecFrame>() {
        spec.set_ref_pos(Some(sky), reflon, reflat)?;
        return Ok(());
    }
    if let Some(dsb) = frame.as_any_mut().downcast_mut::<DSBSpecFrame>() {
        dsb.set_ref_pos(Some(sky), reflon, reflat)?;
        return Ok(());
    }
    if let Some(cmp) = frame.as_any_mut().downcast_mut::<CmpFrame>() {
        let in_first = peek_spec(cmp.frame1()).is_some();
        if in_first {
            return set_spec_ref_pos(cmp.frame1_mut(), sky, reflon, reflat);
        }
        return set_spec_ref_pos(cmp.frame2_mut(), sky, reflon, reflat);
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
    //
    // For a SkyFrame, honour its `axis_permutation`: when the frame
    // has been `perm_axes`'d to lat-first (e.g. CTYPE1=DEC,
    // CTYPE2=RA), the world-frame's external axis 0 is lat and
    // axis 1 is lon. Mirrors C `astGetLonAxis` / `astGetLatAxis`.
    fn find(
        frame: &dyn crate::frames::Frame,
        offset: usize,
        out: &mut (Option<usize>, Option<(usize, usize)>),
    ) {
        use crate::frames::DSBSpecFrame;
        if frame.as_any().is::<SpecFrame>() || frame.as_any().is::<DSBSpecFrame>() {
            if out.0.is_none() {
                out.0 = Some(offset);
            }
        } else if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
            if out.1.is_none() {
                let lon_local = sky.lon_axis();
                let lat_local = sky.lat_axis();
                out.1 = Some((offset + lon_local, offset + lat_local));
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
    use crate::frames::{CmpFrame, DSBSpecFrame, SpecFrame};
    if let Some(s) = frame.as_any().downcast_ref::<SpecFrame>() {
        return Some(s);
    }
    if let Some(d) = frame.as_any().downcast_ref::<DSBSpecFrame>() {
        return Some(d.spec());
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

    // Tidy up offset SkyFrames. Mirrors C `TidyOffsets`
    // (fitschan.c:34368-34540): converts the SKY_POLE / SKY_ORIGIN
    // domain markers stamped by `WcsSkyFrame` into actual SkyRefIs
    // attribute values, and removes redundant SKY_OFFSETS Frames
    // when a corresponding absolute SkyFrame exists.
    tidy_offsets(&mut fs)?;

    Ok(AnyObject::FrameSet(fs))
}

/// Apply C `TidyOffsets` (fitschan.c:34368-34540) to a freshly-read
/// FrameSet. Two passes:
///
/// 1. For every Frame containing a SkyFrame whose Domain is
///    `SKY_POLE` or `SKY_ORIGIN`, set its SkyRefIs attribute and
///    clear the marker domain. Note which Frames hold absolute
///    SkyFrames vs offset (`SKY_OFFSETS`) ones.
/// 2. If at least one absolute and one offset SkyFrame are present,
///    remove every offset Frame from the FrameSet — the absolute
///    SkyFrame's SkyRefIs already encodes the offset interpretation,
///    so AST stores both forms in a single Frame.
///
/// When only offset Frames are present (e.g. a header that has
/// CTYPE = OFLN/OFLT and SREFIS but no separate absolute axis
/// description), the SkyFrame is left in place — its SkyRefIs is
/// promoted from the SKY_POLE/SKY_ORIGIN marker domain in pass 1.
fn tidy_offsets(fs: &mut FrameSet) -> AstResult<()> {
    use crate::frames::{CmpFrame, SkyFrame, SkyRefIs};
    // `current_idx()` is 0-based; FrameRef::Index is 1-based.
    let icurr_1 = fs.current_idx() + 1;

    // Pass 1: walk every Frame, look for a SkyFrame child, examine
    // its domain, classify (absolute / offset / marker), and apply
    // SkyRefIs to absolute SkyFrames carrying SKY_POLE/SKY_ORIGIN
    // markers. Mirrors C:34435-34471.
    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    enum FrameKind {
        Absolute,
        Offset,
        Other,
    }

    fn classify(frame: &dyn crate::frames::Frame) -> (FrameKind, Option<SkyRefIs>) {
        // Find the SkyFrame's domain. Recurse into CmpFrame children.
        fn first_sky_domain(frame: &dyn crate::frames::Frame) -> Option<String> {
            if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
                return Some(sky.domain().to_string());
            }
            if let Some(cmp) = frame.as_any().downcast_ref::<CmpFrame>() {
                if let Some(d) = first_sky_domain(cmp.frame1()) {
                    return Some(d);
                }
                if let Some(d) = first_sky_domain(cmp.frame2()) {
                    return Some(d);
                }
            }
            None
        }
        let Some(domain) = first_sky_domain(frame) else {
            return (FrameKind::Other, None);
        };
        match domain.as_str() {
            "SKY_OFFSETS" => (FrameKind::Offset, None),
            "SKY" | "" => (FrameKind::Absolute, None),
            "SKY_POLE" => (FrameKind::Absolute, Some(SkyRefIs::Pole)),
            "SKY_ORIGIN" => (FrameKind::Absolute, Some(SkyRefIs::Origin)),
            _ => (FrameKind::Other, None),
        }
    }

    fn apply_skyrefis(frame: &mut dyn crate::frames::Frame, skyrefis: SkyRefIs) {
        if let Some(sky) = frame.as_any_mut().downcast_mut::<SkyFrame>() {
            // C `TidyOffsets` (fitschan.c:34464-34468) sets only the
            // SkyRefIs attribute here; it does NOT alter the Domain. The
            // SKY_POLE / SKY_ORIGIN marker Domain is retained on the
            // Frame and is only cleared later, in the `hasabs && hasoff`
            // removal pass (`astClearDomain`, fitschan.c:34493) — which
            // does not run when the offset Frame is the only sky
            // description present. Keeping SKY_ORIGIN / SKY_POLE here is
            // load-bearing: the writer's `IsASkyFrame` macro
            // (fitschan.c:1365) requires `Domain == "SKY"`, so an
            // offset Frame with a non-"SKY" Domain is deliberately
            // routed through `OtherAxes` (emitting plain OFLN / OFLT
            // CTYPEs from the axis Symbols) rather than `CelestialAxes`.
            sky.set_sky_ref_is(skyrefis);
            return;
        }
        if let Some(cmp) = frame.as_any_mut().downcast_mut::<CmpFrame>() {
            apply_skyrefis(cmp.frame1_mut(), skyrefis);
            apply_skyrefis(cmp.frame2_mut(), skyrefis);
        }
    }

    let nfrm = fs.nframe();
    let mut kinds = Vec::with_capacity(nfrm);
    for ifrm in 1..=nfrm {
        let (kind, marker) = {
            let f = fs.frame(FrameRef::Index(ifrm))?;
            classify(f)
        };
        if let Some(skyrefis) = marker {
            // C `TidyOffsets` (fitschan.c:34464-34468) sets SkyRefIs
            // *through the FrameSet* so the FrameSet integrity machinery
            // re-maps the SkyFrame to offset coordinates. Here the
            // SkyFrame read from an OFLN/OFLT header already carries the
            // offset (linear) pixel→sky Mapping, so the attribute only
            // needs recording on the Frame itself — applying it through
            // FrameSet integrity would re-fuse the Mapping into a form
            // that loses the offset description. Set it directly,
            // leaving the Domain marker (SKY_ORIGIN / SKY_POLE) intact
            // so the writer's `IsASkyFrame` test routes the Frame
            // through `OtherAxes`.
            let f = fs.frame_mut(FrameRef::Index(ifrm))?;
            apply_skyrefis(f, skyrefis);
        }
        kinds.push(kind);
    }

    let has_abs = kinds.contains(&FrameKind::Absolute);
    let has_off = kinds.contains(&FrameKind::Offset);

    // Pass 2: only remove offset Frames when an absolute counterpart
    // exists — otherwise the offset Frame is the only sky description
    // we have and removing it would lose the SkyRef information.
    // Mirrors C:34476-34540.
    if has_abs && has_off {
        // Remove offsets in reverse order so the indices stay valid
        // as we delete.
        for ifrm in (1..=nfrm).rev() {
            if kinds[ifrm - 1] == FrameKind::Offset {
                fs.remove_frame(FrameRef::Index(ifrm))?;
            }
        }
        // remove_frame may have invalidated `icurr_1`; the C library
        // re-resolves the current index through the original Frame
        // pointer. We approximate by clamping to the last absolute
        // Frame if the original was an offset.
        let new_nfrm = fs.nframe();
        let original_was_offset = icurr_1 <= kinds.len() && kinds[icurr_1 - 1] == FrameKind::Offset;
        if icurr_1 > new_nfrm || original_was_offset {
            // Pick the highest-index non-removed absolute Frame.
            for ifrm in (1..=new_nfrm).rev() {
                if kinds[ifrm - 1] == FrameKind::Absolute {
                    fs.set_current(FrameRef::Index(ifrm))?;
                    break;
                }
            }
        } else if icurr_1 <= new_nfrm {
            fs.set_current(FrameRef::Index(icurr_1))?;
        }
    } else {
        // No removals — restore the original current.
        fs.set_current(FrameRef::Index(icurr_1))?;
    }

    Ok(())
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

    // General case: compose the per-class N-axis contributions in
    // series. See `build_general_iwc_to_world`.
    build_general_iwc_to_world(store, celestial, spectral, &claimed)
}

/// Build the general IWC → world mapping for mixed axis cases.
///
/// Each per-class mapping (celestial, spectral, linear shift) is built as
/// a full N-axis mapping with identity on the axes it doesn't own; the
/// contributions are composed in series. Mirrors C `WcsMapFrm`
/// (fitschan.c:37632), which composes N-axis WcsCelestial /
/// WcsSpectral / WcsOthers contributions in series and lets
/// `astSimplify` flatten the resulting chain.
fn build_general_iwc_to_world(
    store: &FitsStore,
    celestial: Option<CelestialResult>,
    spectral: Option<SpectralResult>,
    claimed: &[bool],
) -> AstResult<DynMapping> {
    let naxis = store.naxis;

    let mut parts: Vec<DynMapping> = Vec::new();

    let mut celestial = celestial;
    let mut spectral = spectral;

    for (i, &is_claimed) in claimed.iter().enumerate() {
        // Celestial axis: emit the celestial mapping when we hit the
        // FIRST of the celestial pair (in FITS order); skip the second
        // encounter. The celestial mapping is full N-axis with
        // identity on the non-celestial axes.
        if let Some(cel) = celestial.as_ref() {
            let cel_first = cel.lon_axis.min(cel.lat_axis);
            let cel_second = cel.lon_axis.max(cel.lat_axis);
            if i == cel_first {
                let cel = celestial.take().unwrap();
                parts.push(cel.mapping);
                continue;
            }
            if i == cel_second {
                continue;
            }
        }

        // Spectral axis: emit the spectral mapping when we hit it.
        // Like celestial, it's full N-axis with identity on the
        // non-spectral axes.
        if let Some(spec) = spectral.as_ref() {
            if i == spec.axis {
                let spec = spectral.take().unwrap();
                parts.push(spec.mapping);
                continue;
            }
        }

        // Linear/unclaimed axis: emit a CRVAL shift, lifted to N-axis
        // form. A length-`naxis` shift vector with zeros on the other
        // axes and CRVAL on `i` makes the contribution N-axis identity-
        // on-others, so series composition is correct.
        if !is_claimed {
            let mut shifts = vec![0.0; naxis];
            shifts[i] = store.crval[i].unwrap_or(0.0);
            let shift = ShiftMap::new(shifts);
            parts.push(Box::new(shift));
        }
    }

    if parts.is_empty() {
        return Ok(Box::new(UnitMap::new(naxis)));
    }

    // Compose all parts in SERIES. Each part is full N-axis with
    // identity on the axes it doesn't own, so series composition is
    // correct and produces a flat top-level chain that astSimplify can
    // flatten and merge across. Mirrors C `WcsMapFrm`
    // (fitschan.c:37632).
    let mut result: DynMapping = parts.remove(0);
    for part in parts {
        result = Box::new(CmpMap::new(result, part, Composition::Series)?);
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
        return Ok(spec.world_frame());
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
            // CTYPE → axis Symbol (mirrors fitschan.c:38837 —
            // `astSetSymbol(*frm, i, buf2)` where buf2 is the
            // CTYPE value with any algorithm code stripped).
            // Without this, the round-trip writer can't recover the
            // CTYPE because it falls back to GetDefaultSymbol's
            // `<Domain><axis>` format.
            if let Some(ctype) = &store.ctype[axis] {
                builder = builder.symbol(lidx_1based, ctype.clone());
            }
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
                    frames.push(spec.world_frame());
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

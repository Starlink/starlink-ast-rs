//! Main FITS-WCS decode pipeline.
//!
//! Assembles the pixel-to-IWC mapping, celestial decoder, and spectral decoder
//! into a complete FrameSet: pixel Frame → world Frame. This mirrors the C
//! library's `FsetFromStore` / `WcsMapFrm` functions (fitschan.c ~line 37632).

use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::wcs_celestial::{self, CelestialResult};
use super::wcs_keywords::key_for;
use super::wcs_matrix;
use super::wcs_spectral::{self, SpectralResult};
use crate::error::AstResult;
use crate::frames::{BasicFrameBuilder, CmpFrame, DynFrame, FrameRef, FrameSet};
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, UnitMap};
use crate::serial::AnyObject;

/// Read `RFVAL<i>` (primary plane) or `RFVAL<i><s>` (alt plane `s`) off
/// `cards`, unmarked, mirroring C `GetValue(this, FormatKey("RFVAL", axis,
/// -1, s, ...), ..., mark = 0, ...)` (fitschan.c:36091-36097).
///
/// `RFVAL` has no `WcsFcRead` branch and no `FitsStore` member: C reads it
/// fresh off the FitsChan at the point of use in `WcsCelestial`, via `this`
/// — the FitsChan passed to `FsetFromStore` (fitschan.c:25783) — rather than
/// through the extraction pipeline that populates the rest of the store
/// (see [`super::fits_store::FitsStore::axref`] for the contrasting `AXREF`
/// case, which does go through that pipeline). `RFVAL` is read but never
/// written anywhere in `fitschan.c`; its only three appearances are the
/// comment and the two `GetValue` calls at fitschan.c:36088-36097, so its
/// value is always whatever the caller supplied on the header.
fn read_rfval(cards: &CardList, naxis: usize, s: char) -> Vec<Option<f64>> {
    (0..naxis)
        .map(|i| cards.get_float(&key_for("RFVAL", Some(i + 1), s)))
        .collect()
}

/// Decode a [`FitsStore`] into a FrameSet (wrapped as [`AnyObject`]).
///
/// The resulting FrameSet has:
/// - Base frame: pixel coordinates (domain "GRID")
/// - Current frame: world coordinates (SkyFrame, SpecFrame, CmpFrame, or BasicFrame)
/// - Mapping: pixel → world
#[cfg(test)]
pub(crate) fn decode_wcs(store: &FitsStore) -> AstResult<AnyObject> {
    let (obj, _warnings) = decode_wcs_inner(store, false, &CardList::new(), ' ')?;
    Ok(obj)
}

/// Inner decoder that also returns any warning messages collected during
/// decoding (e.g. the `badcel` warning for unknown celestial systems,
/// fitschan.c:39358). Callers that need to surface warnings to the
/// FitsChan as ASTWARN cards use this variant directly.
///
/// `cards` is the FitsChan's own card list — the same `this` C's
/// `WcsCelestial` reads `RFVAL<i>` from (fitschan.c:36091-36097) — and `s`
/// is this plane's version character (`' '` for the primary plane, `'A'`..
/// `'Z'` for an alt plane).
pub(crate) fn decode_wcs_inner(
    store: &FitsStore,
    def_b1950: bool,
    cards: &CardList,
    s: char,
) -> AstResult<(AnyObject, Vec<String>)> {
    let naxis = store.naxis;

    // Step 1: Build the pixel → IWC mapping.
    let pixel_to_iwc = wcs_matrix::build_pixel_to_iwc(store)?;

    // Step 2: Try to decode celestial axes.
    let rfval = read_rfval(cards, naxis, s);
    let celestial = wcs_celestial::build_celestial_with_def_b1950(store, def_b1950, Some(&rfval));
    // Capture the celestial fiducial position (radians) before the decoder
    // result is consumed below; it is stamped onto the SkyFrame as SkyRef.
    let celestial_ref_pos = celestial.as_ref().and_then(|c| c.ref_pos);
    // AXREF forces SkyRef unconditionally, bypassing the astTestSkyRef gate
    // that CRVAL / RFVAL are subject to (fitschan.c:36099-36110 vs
    // 36209-36223).
    let celestial_ref_pos_forced = celestial.as_ref().is_some_and(|c| c.ref_pos_forced);
    // Collect warnings from the celestial decoder (e.g. badcel for unknown
    // celestial systems).
    let warnings: Vec<String> = celestial
        .as_ref()
        .map(|c| c.warnings.clone())
        .unwrap_or_default();

    // Step 3: Try to decode spectral axis.
    let spectral = wcs_spectral::build_spectral(store);

    // Step 4: Build the world frame (before consuming the decoders).
    let mut world_frame = build_world_frame(store, &celestial, &spectral)?;

    // Stamp the alternate-version character onto the world Frame's Ident, as
    // C `WcsMapFrm` does (`id[0] = s; astSetIdent(*frm, id)`,
    // fitschan.c:37756). For the primary axis description `s` is a space, so
    // the Frame carries `Ident = " "` — an explicitly-set blank identifier
    // that the native dump renders (with the inherited `IsA Object` block).
    set_frame_ident(world_frame.as_mut(), " ");

    // Stamp DUT1/DTAI onto the world Frame, as C `WcsMapFrm` does
    // immediately after the `astSetIdent` call above (fitschan.c:37883-37892).
    set_frame_dut1_dtai(
        world_frame.as_mut(),
        store.dut1.map(|v| v * 86_400.0),
        store.dtai,
    );

    // Step 5: Build the full pixel → world mapping (consumes the decoder
    // results). C `WcsMapFrm` (fitschan.c:37768-37810) returns
    // `astSimplify(map8)` — the pixel→WCS Mapping is simplified before it is
    // added to the FrameSet, so the stored edge (and any later native dump
    // of it) is in reduced form. Use `simplify()` (not `simplified()`) so the
    // returned Mapping carries `has_been_simplified` (→ `IsSimp = 1` in the
    // native dump), mirroring C `astSimplify` which sets the IsSimple flag on
    // its result.
    let composed: DynMapping = build_pixel_to_world(store, pixel_to_iwc, celestial, spectral)?;
    let full_mapping: DynMapping = composed.simplify()?;

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

    // Step 8b: stamp the celestial reference position onto the SkyFrame as
    // SkyRef + SkyRefIs=Ignored. Mirrors C `WcsCelestial`
    // (fitschan.c:36098-36110), which, when the SkyFrame's SkyRef has not
    // already been set, stores (reflon, reflat) and sets SkyRefIs=Ignored so
    // the SkyFrame still represents absolute celestial coordinates. Because
    // SkyRefIs=Ignored leaves the coordinate semantics unchanged, this adds
    // no remapping — we set it via `frame_mut` to bypass FrameSet integrity,
    // matching C which sets it on the Frame before the FrameSet wraps it.
    set_sky_ref_from_celestial(&mut frameset, celestial_ref_pos, celestial_ref_pos_forced)?;

    // Step 9: SpecFrame RefRA/RefDec from celestial fiducial. Mirrors
    // C `astSetRefPos(specfrm, reffrm, reflon, reflat)` at
    // fitschan.c:39535. C calls this on the standalone SpecFrame
    // BEFORE the FrameSet wraps it, so no FrameSet-integrity remap
    // fires. We achieve the same semantics by using `frame_mut` to
    // bypass our `with_integrity` machinery, since by this point the
    // SpecFrame already lives inside the FrameSet.
    set_spec_ref_from_celestial(&mut frameset, celestial_ref_pos)?;

    // Step 10: DSBSpecFrame setup. Mirrors C `WcsSpectral`
    // (fitschan.c:39585-39588), which calls `DSBSetUp` immediately after
    // `astSetRefPos` when the spectral frame is a DSBSpecFrame. The IF /
    // DSBCentre / SideBand attributes depend on the topocentric frequency
    // conversion, which in turn depends on the reference position set above.
    dsb_set_up_in_frameset(&mut frameset, store)?;

    Ok((AnyObject::FrameSet(frameset), warnings))
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
fn set_spec_ref_from_celestial(fs: &mut FrameSet, ref_pos: Option<(f64, f64)>) -> AstResult<()> {
    // Locate spec/sky child indices and frame-axis indices within the
    // current world frame. If either is absent, nothing to do.
    if locate_spec_sky(fs.current()).is_none() {
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

    // The reference position is the raw CRVAL pair (radians) in the SkyFrame's
    // native system, as returned by `WcsCelestial` and passed straight into
    // `astSetRefPos(specfrm, reffrm, reflon, reflat)` (fitschan.c:37780/39507).
    // C uses the same reflon/reflat for the SkyFrame's SkyRef and the
    // SpecFrame's RefRA/RefDec — so use the celestial decoder's value rather
    // than re-deriving it through the pixel→world Mapping (which loses ULPs).
    let Some((reflon, reflat)) = ref_pos else {
        return Ok(());
    };

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

/// Stamp the celestial reference position onto the SkyFrame in the current
/// world frame as `SkyRef` + `SkyRefIs=Ignored`, mirroring C `WcsCelestial`
/// (fitschan.c:36098-36110, 36209-36223).
///
/// The reference position is the celestial reference longitude / latitude the
/// decoder resolved — CRVAL by default, overridden by `RFVAL<i>` and then by
/// `AXREF<i>`. When `forced` is `false` (the CRVAL / RFVAL case) C only sets
/// it when the SkyFrame's SkyRef has not already been set (`astTestSkyRef`,
/// fitschan.c:36103), and skips offset SkyFrames; we honour the same gate.
/// When `forced` is `true` (the AXREF case) C calls `astSetSkyRef`
/// unconditionally (fitschan.c:36220-36221), outside that gate. Setting
/// `SkyRefIs=Ignored` leaves absolute-coordinate semantics unchanged, so this
/// is applied via `frame_mut` without triggering a FrameSet-integrity remap.
fn set_sky_ref_from_celestial(
    fs: &mut FrameSet,
    ref_pos: Option<(f64, f64)>,
    forced: bool,
) -> AstResult<()> {
    // The reference position (radians) as resolved by the celestial decoder,
    // mirroring C `WcsCelestial` (fitschan.c:36012-36015 for the CRVAL case).
    // Nothing to stamp if it could not be determined.
    let Some((reflon, reflat)) = ref_pos else {
        return Ok(());
    };

    // Only proceed when the current frame actually contains a SkyFrame.
    if locate_sky_axes(fs.current()).is_none() {
        return Ok(());
    }

    // C applies the `astTestSkyRef` gate and the offset-domain check to the
    // CRVAL / RFVAL reference position (fitschan.c:36104-36111) but not to
    // the AXREF one, which is stored unconditionally (fitschan.c:36214-
    // 36220).
    if !forced {
        // Skip if the SkyFrame's SkyRef is already set (C `astTestSkyRef`),
        // or if this is an offset SkyFrame whose SkyRef defines the offset
        // origin / pole (Domain SKY_OFFSETS / SKY_POLE / SKY_ORIGIN, set by
        // WcsSkyFrame before tidy_offsets runs). For those, the SkyRef is
        // the offset definition, not an "ignored" celestial reference, so
        // stamping the celestial fiducial would corrupt the offset
        // representation.
        let cur = fs.current();
        if let Some(sky) = peek_sky(cur) {
            if sky.test_sky_ref(0) {
                return Ok(());
            }
            if matches!(sky.domain(), "SKY_OFFSETS" | "SKY_POLE" | "SKY_ORIGIN") {
                return Ok(());
            }
        }
    }

    if reflon == crate::constants::BAD_VALUE || reflat == crate::constants::BAD_VALUE {
        return Ok(());
    }

    // Set SkyRef (lon, lat) and SkyRefIs=Ignored on the SkyFrame, bypassing
    // FrameSet integrity (SkyRefIs=Ignored does not change the transform).
    let cur = fs.frame_mut(FrameRef::Current)?;
    set_sky_ref_on_frame(cur, reflon, reflat);

    Ok(())
}

/// Set `SkyRef` + `SkyRefIs=Ignored` on the `SkyFrame` within `frame`,
/// recursing through any `CmpFrame`. The longitude/latitude values are passed
/// in the SkyFrame's own axis order (lon first); `set_sky_ref` stores them on
/// the lon/lat axes.
fn set_sky_ref_on_frame(frame: &mut dyn crate::frames::Frame, reflon: f64, reflat: f64) {
    use crate::frames::sky_convert::SkyRefIs;
    use crate::frames::{CmpFrame, SkyFrame};
    if let Some(sky) = frame.as_any_mut().downcast_mut::<SkyFrame>() {
        sky.set_sky_ref(reflon, reflat);
        sky.set_sky_ref_is(SkyRefIs::Ignored);
        return;
    }
    if let Some(cmp) = frame.as_any_mut().downcast_mut::<CmpFrame>() {
        if peek_sky(cmp.frame1()).is_some() {
            set_sky_ref_on_frame(cmp.frame1_mut(), reflon, reflat);
        } else {
            set_sky_ref_on_frame(cmp.frame2_mut(), reflon, reflat);
        }
    }
}

/// Locate the SkyFrame in `frame` and return its world (external) longitude
/// and latitude axis offsets, honouring axis permutation. Returns `None` if
/// no SkyFrame is present.
fn locate_sky_axes(frame: &dyn crate::frames::Frame) -> Option<(usize, usize)> {
    use crate::frames::SkyFrame;
    // Walk *external* axes via `primary_frame`, which resolves any
    // CmpFrame axis permutation. The returned `paxis` is the SkyFrame's
    // *internal physical* axis index (0 = longitude, 1 = latitude), so
    // we test it against those constants — not against the SkyFrame's
    // `lon_axis()`/`lat_axis()` which report *external* positions. This
    // keeps the returned indices aligned with the pixel→world Mapping's
    // external (FITS) output order even when the world CmpFrame and/or
    // the SkyFrame itself have been `perm_axes`'d.
    let mut lon: Option<usize> = None;
    let mut lat: Option<usize> = None;
    for iax in 0..frame.naxes() {
        let (pf, paxis) = frame.primary_frame(iax);
        if pf.as_any().is::<SkyFrame>() {
            if paxis == 0 {
                lon.get_or_insert(iax);
            } else if paxis == 1 {
                lat.get_or_insert(iax);
            }
        }
    }
    Some((lon?, lat?))
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

/// Confirm the world frame is a `CmpFrame` containing both a spectral frame
/// (`SpecFrame`/`DSBSpecFrame`) and a `SkyFrame` (longitude + latitude axes).
/// Returns `Some(())` when present — the reference position itself comes from
/// the celestial decoder's raw CRVAL, so no axis indices are needed here.
fn locate_spec_sky(frame: &dyn crate::frames::Frame) -> Option<()> {
    use crate::frames::{CmpFrame, DSBSpecFrame, SkyFrame, SpecFrame};
    // Require a CmpFrame (a single spec/sky frame has no companion).
    frame.as_any().downcast_ref::<CmpFrame>()?;

    // Walk *external* axes via `primary_frame`, which resolves any CmpFrame
    // axis permutation. `paxis` is the SkyFrame's *internal physical* axis
    // index (0 = longitude, 1 = latitude). Mirrors C's primary-frame walk in
    // WcsSpectral (fitschan.c:39620+).
    let mut spec_axis: Option<usize> = None;
    let mut lon: Option<usize> = None;
    let mut lat: Option<usize> = None;
    for iax in 0..frame.naxes() {
        let (pf, paxis) = frame.primary_frame(iax);
        if pf.as_any().is::<SpecFrame>() || pf.as_any().is::<DSBSpecFrame>() {
            spec_axis.get_or_insert(iax);
        } else if pf.as_any().is::<SkyFrame>() {
            if paxis == 0 {
                lon.get_or_insert(iax);
            } else if paxis == 1 {
                lat.get_or_insert(iax);
            }
        }
    }

    spec_axis?;
    lon?;
    lat?;
    Some(())
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
/// loop. Also returns any warning messages to surface as ASTWARN cards.
/// `def_b1950` mirrors the FitsChan `DefB1950` attribute.
///
/// Returns any warning messages (e.g. `badcel`) alongside the decoded object;
/// callers pass the warnings to [`super::FitsChan::read`] for insertion as
/// ASTWARN cards.
///
/// `cards` is the FitsChan's own card list, used to read `RFVAL<i>` /
/// `RFVAL<i><s>` for the primary plane and each alt plane respectively
/// (fitschan.c:36091-36097).
pub(crate) fn decode_wcs_with_alts_inner(
    primary: &FitsStore,
    alts: &[(char, FitsStore)],
    def_b1950: bool,
    cards: &CardList,
) -> AstResult<(AnyObject, Vec<String>)> {
    // Step 1: build the primary FrameSet via the inner decoder.
    let (primary_obj, mut warnings) = decode_wcs_inner(primary, def_b1950, cards, ' ')?;
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
        let rfval = read_rfval(cards, alt_store.naxis, *letter);
        let celestial =
            wcs_celestial::build_celestial_with_def_b1950(alt_store, def_b1950, Some(&rfval));
        // Capture the alt plane's celestial fiducial before the decoder is
        // consumed, so the alt SkyFrame gets the same SkyRef/SkyRefIs=Ignored
        // stamp as the primary (C applies WcsCelestial's SkyRef-from-fiducial
        // to every plane, not just the primary).
        let alt_ref_pos = celestial.as_ref().and_then(|c| c.ref_pos);
        let alt_ref_pos_forced = celestial.as_ref().is_some_and(|c| c.ref_pos_forced);
        // Collect warnings from alt planes too.
        if let Some(ref cel) = celestial {
            warnings.extend(cel.warnings.iter().cloned());
        }
        let spectral = wcs_spectral::build_spectral(alt_store);
        let mut world_frame = build_world_frame(alt_store, &celestial, &spectral)?;
        // Simplify the pixel→world Mapping, mirroring C `astSimplify` on each
        // plane (and the primary path above) — `simplify()` sets the
        // has_been_simplified flag so the native dump writes `IsSimp = 1`.
        let composed: DynMapping =
            build_pixel_to_world(alt_store, pixel_to_iwc, celestial, spectral)?;
        let full_mapping: DynMapping = composed.simplify()?;

        // Stamp the version letter onto the alt frame so the writer
        // can recover it.
        set_frame_ident(world_frame.as_mut(), &letter.to_string());

        // Stamp DUT1/DTAI onto the alt world Frame, exactly as for the
        // primary plane above — C `WcsMapFrm` runs once per plane
        // (fitschan.c:2641 loops over version characters).
        set_frame_dut1_dtai(
            world_frame.as_mut(),
            alt_store.dut1.map(|v| v * 86_400.0),
            alt_store.dtai,
        );

        // add_frame moves `current` to the newly-added alt frame, so the
        // SkyRef/RefRA-RefDec/DSB stamps below target it.
        fs.add_frame(FrameRef::Base, full_mapping, world_frame)?;
        set_sky_ref_from_celestial(&mut fs, alt_ref_pos, alt_ref_pos_forced)?;

        // SpecFrame RefRA/RefDec and DSBSpecFrame setup, exactly as for
        // the primary plane above — C `WcsSpectral`'s `astSetRefPos` /
        // `DSBSetUp` calls (fitschan.c:39574, 39585-39588) run once per
        // plane, driven by the per-plane `s` loop at fitschan.c:2641.
        set_spec_ref_from_celestial(&mut fs, alt_ref_pos)?;
        dsb_set_up_in_frameset(&mut fs, alt_store)?;
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

    Ok((AnyObject::FrameSet(fs), warnings))
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
            // *through the FrameSet*: it makes frame `ifrm` current, sets
            // SkyRefIs, then restores the original current frame. Setting
            // the attribute through the FrameSet lets the integrity
            // machinery re-map the SkyFrame into offset coordinates, so
            // the pixel→sky Mapping carries the SREF offset rotation. A
            // direct `frame_mut` set records the attribute but leaves the
            // Mapping pointing at the raw (unrotated) offset axes, which
            // diverges from C. The integrity remap only appends the
            // SkyRefIs delta; it leaves the SKY_ORIGIN / SKY_POLE marker
            // Domain intact so the writer's `IsASkyFrame` test still
            // routes the Frame through `OtherAxes`.
            fs.set_current(FrameRef::Index(ifrm))?;
            fs.with_integrity(|f| {
                apply_skyrefis(f, skyrefis);
                Ok(())
            })?;
            fs.set_current(FrameRef::Index(icurr_1))?;
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

/// Helper: stamp `ident` onto a boxed Frame. `Ident` is an Object-level
/// attribute in C (`object.c:5225`), so C's `astSetIdent( *frm, id )`
/// (fitschan.c:37756) applies whatever class the world Frame turned out to
/// be; the trait method carries the same property here.
fn set_frame_ident(frame: &mut dyn crate::frames::Frame, ident: &str) {
    frame.set_ident(ident);
}

/// Helper: stamp DUT1/DTAI onto a boxed Frame, dispatching by concrete
/// type, mirroring C `WcsMapFrm`'s calls to `astSetDut1` / `astSetDtai`
/// on `*frm` (fitschan.c:37883-37892). `dut1` is in seconds (the FITS
/// `DUT1` keyword is in days; the caller converts by `SPD` before calling
/// this).
///
/// In C, `Dut1` and `Dtai` are base-`Frame` attributes (`frame.h:622`,
/// `frame.c:12384-12387`), so every Frame class carries them. In the port
/// they are currently implemented on `SkyFrame`, `SpecFrame`, `TimeFrame`,
/// and `CmpFrame` only; `BasicFrame` and `FluxFrame` have no such fields, so
/// a Frame of either type is left unstamped.
fn set_frame_dut1_dtai(frame: &mut dyn crate::frames::Frame, dut1: Option<f64>, dtai: Option<f64>) {
    use crate::frames::{CmpFrame, SkyFrame, SpecFrame, TimeFrame};
    use crate::mappings::AsAny;
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<SkyFrame>() {
        if let Some(v) = dut1 {
            f.set_dut1(v);
        }
        if let Some(v) = dtai {
            f.set_dtai(v);
        }
        return;
    }
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<SpecFrame>() {
        if let Some(v) = dut1 {
            f.set_dut1(v);
        }
        if let Some(v) = dtai {
            f.set_dtai(v);
        }
        return;
    }
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<TimeFrame>() {
        if let Some(v) = dut1 {
            f.set_dut1(v);
        }
        if let Some(v) = dtai {
            f.set_dtai(v);
        }
        return;
    }
    if let Some(f) = AsAny::as_any_mut(frame).downcast_mut::<CmpFrame>() {
        if let Some(v) = dut1 {
            f.set_dut1(v);
        }
        if let Some(v) = dtai {
            f.set_dtai(v);
        }
    }
}

/// Build the full pixel → world mapping (consumes the decoder results).
///
/// Mirrors C `WcsMapFrm` (fitschan.c:37768-37810), which builds
/// `map8 = (((map1_pixel→IWC ; map2_celestial) ; map3_spectral) ; map4_others)`
/// — a left-associated series chain in class order, with `map4` (WcsOthers)
/// always composed (a bare `UnitMap(naxis)` when every axis is claimed).
/// `astSimplify` then flattens and merges the chain.
fn build_pixel_to_world(
    store: &FitsStore,
    pixel_to_iwc: DynMapping,
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

    // Compose factors in C's class order: pixel→IWC, celestial, spectral,
    // others. Each per-class mapping is full N-axis with identity on the
    // axes it doesn't own, so series composition is correct.
    let mut parts: Vec<DynMapping> = vec![pixel_to_iwc];
    if let Some(cel) = celestial {
        parts.push(cel.mapping);
    }
    if let Some(spec) = spectral {
        parts.push(spec.mapping);
    }
    if claimed.contains(&false) {
        // WcsOthers: one shift covering every unclaimed axis (zeros on the
        // claimed axes, CRVAL on the rest).
        let mut shifts = vec![0.0; naxis];
        for (i, &c) in claimed.iter().enumerate() {
            if !c {
                shifts[i] = store.crval[i].unwrap_or(0.0);
            }
        }
        parts.push(Box::new(ShiftMap::new(shifts)));
    } else {
        // C still composes map4 even when every axis is claimed.
        parts.push(Box::new(UnitMap::new(naxis)));
    }

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

    // Simple cases: a single class spanning all axes returns its bare frame.
    if celestial.is_some() && spectral.is_none() && n_unclaimed == 0 {
        let cel = celestial.as_ref().unwrap();
        return Ok(Box::new(cel.frame.clone()));
    }
    if spectral.is_some() && celestial.is_none() && n_unclaimed == 0 {
        let spec = spectral.as_ref().unwrap();
        return Ok(spec.world_frame());
    }
    if celestial.is_none() && spectral.is_none() {
        // All-linear: a single BasicFrame carrying the per-axis CTYPE info.
        return seed_linear_frame(store, &(0..naxis).collect::<Vec<_>>());
    }

    // Mixed case: replicate C `WcsMapFrm`'s in-place frame morph
    // (fitschan.c). A base N-axis Frame is restructured by each class
    // stage: pick the *other* axes into a sub-frame, wrap it in a
    // CmpFrame with the new (Sky/Spec) frame appended, then PermAxes to
    // restore the original FITS axis order externally. The IWC→world
    // *mapping* stays in FITS order (see `build_pixel_to_world`);
    // only the frame is reordered + permuted, so the AXP cards record the
    // FITS→internal mapping and the external axis order still matches the
    // mapping output. This produces the canonical component order
    // (others, Sky, Spec) and the `AXP{n}` permutation that C emits.

    // Base frame: a BasicFrame over all axes, each carrying its CTYPE
    // symbol / label / unit (the WcsOthers step, fitschan.c:38622 — it
    // only affects the linear axes, since the celestial/spectral axes are
    // replaced by their own frames below).
    let mut frame: DynFrame = seed_linear_frame(store, &(0..naxis).collect::<Vec<_>>())?;

    // --- Celestial stage (WcsCelestial, fitschan.c:36043-36086) ---
    if let Some(cel) = celestial.as_ref() {
        // `cel.frame` already carries an internal axis permutation that
        // makes its *external* axis order agree with ascending FITS axis
        // order (see `build_celestial`: a `DEC,RA` header perm_axes's the
        // SkyFrame to lat-first). `morph_in_sub_frame` expects `new_axes`
        // to list the FITS positions in the order of the new frame's
        // external axes, so we hand it the sky FITS axes in ascending
        // order — matching the SkyFrame's already-permuted external layout.
        let mut sky_axes = [cel.lon_axis, cel.lat_axis];
        sky_axes.sort_unstable();
        frame = morph_in_sub_frame(frame, Box::new(cel.frame.clone()), &sky_axes, naxis)?;
    }

    // --- Spectral stage (WcsSpectral, fitschan.c:39620-39658) ---
    if let Some(spec) = spectral.as_ref() {
        frame = morph_in_sub_frame(frame, spec.world_frame(), &[spec.axis], naxis)?;
    }

    Ok(frame)
}

/// Build a BasicFrame over the given FITS axes (0-based), carrying each
/// axis's CTYPE symbol, label (CNAME or CTYPE), and unit. Mirrors the
/// per-axis attribute setup C performs (WcsOthers `astSetSymbol`/`astSetLabel`,
/// fitschan.c:38622+).
fn seed_linear_frame(store: &FitsStore, axes: &[usize]) -> AstResult<DynFrame> {
    let mut builder = BasicFrameBuilder::new(axes.len());
    for (k, &axis) in axes.iter().enumerate() {
        let idx_1based = k + 1;
        if let Some(ctype) = &store.ctype[axis] {
            builder = builder.symbol(idx_1based, ctype.clone());
        }
        if let Some(label) = &store.cname[axis] {
            builder = builder.label(idx_1based, label.clone());
        } else if let Some(ctype) = &store.ctype[axis] {
            builder = builder.label(idx_1based, ctype.clone());
        }
        if let Some(unit) = &store.cunit[axis] {
            builder = builder.unit(idx_1based, unit.clone());
        }
    }
    Ok(Box::new(builder.build()?))
}

/// Morph `frame` by replacing the axes in `new_axes` (0-based FITS indices,
/// in their natural sub-frame order) with `new_frame`, mirroring one stage of
/// C `WcsMapFrm`:
///
/// 1. Pick the axes NOT in `new_axes` (in FITS order) into a sub-frame
///    (`astPickAxes`).
/// 2. Build `CmpFrame(others, new_frame)` — the new frame's axes are appended
///    at the end (positions `naxis - new_axes.len() ..`).
/// 3. `perm_axes` to send each external (FITS) axis to its built position: the
///    `new_axes` go to the trailing slots (in the order given), the others
///    fill `0,1,2,…` in FITS order. Mirrors C's `axes[]` + `astPermAxes`.
fn morph_in_sub_frame(
    frame: DynFrame,
    new_frame: DynFrame,
    new_axes: &[usize],
    naxis: usize,
) -> AstResult<DynFrame> {
    use crate::frames::Frame;

    // Other axes (FITS order), 0-based, as i32 for pick_axes_snapshot.
    let others: Vec<i32> = (0..naxis)
        .filter(|i| !new_axes.contains(i))
        .map(|i| i as i32)
        .collect();

    // If there are no other axes, the new frame spans everything (handled by
    // the simple-case short-circuits in the caller for the single-class case,
    // but this stage can still be reached when a prior stage consumed the
    // rest); just return the new frame.
    if others.is_empty() {
        return Ok(new_frame);
    }

    let ofrm = frame.pick_axes_snapshot(&others)?;
    let mut cmp = CmpFrame::new(ofrm, new_frame);

    // Build the external→internal permutation (1-based for `perm_axes`).
    // Built (internal) order is: others at 0..n_others, then new_axes at the
    // trailing slots in the order listed. C's `axes[fits_k]` gives the
    // 0-based internal slot; our `perm_axes` takes 1-based, so we add 1.
    let n_new = new_axes.len();
    let n_others = others.len();
    let mut perm = vec![0usize; naxis];
    let mut j = 0usize; // running index for "other" axes
    for (fits_k, slot) in perm.iter_mut().enumerate() {
        if let Some(pos_in_new) = new_axes.iter().position(|&a| a == fits_k) {
            // Trailing slot for this new-frame axis (0-based internal).
            *slot = n_others + pos_in_new + 1; // +1 → 1-based
        } else {
            *slot = j + 1; // +1 → 1-based
            j += 1;
        }
    }
    debug_assert_eq!(j, n_others);
    debug_assert_eq!(n_others + n_new, naxis);

    cmp.perm_axes(&perm)?;
    Ok(Box::new(cmp))
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

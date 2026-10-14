//! Main FITS-WCS decode pipeline.
//!
//! Assembles the pixel-to-IWC mapping, celestial decoder, and spectral decoder
//! into a complete FrameSet: pixel Frame → world Frame. This mirrors the C
//! library's `FsetFromStore` / `WcsMapFrm` functions (fitschan.c ~line 37632).

use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::warning::{Warning, Warnings};
use super::wcs_celestial::{self, CelestialResult};
use super::wcs_keywords::{WcsStores, key_for};
use super::wcs_matrix;
use super::wcs_spectral::{self, SpectralResult};
use super::wcs_tab::{self, TabLookup, TabReader};
use crate::error::AstResult;
use crate::frames::{BasicFrameBuilder, CmpFrame, DynFrame, FrameRef, FrameSet};
use crate::mappings::{CmpMap, Composition, DynMapping, PermMap, ShiftMap, UnitMap};
use crate::serial::AnyObject;

/// Read `RFVAL<i>` (primary plane) or `RFVAL<i><s>` (alt plane `s`) off
/// `cards`, unmarked, mirroring C `GetValue(this, FormatKey("RFVAL", axis,
/// -1, s, ...), ..., mark = 0, ...)` (fitschan.c:36220-36226).
///
/// `RFVAL` has no `WcsFcRead` branch and no `FitsStore` member: C reads it
/// fresh off the FitsChan at the point of use in `WcsCelestial`, via `this`
/// — the FitsChan passed to `FsetFromStore` (fitschan.c:25895) — rather than
/// through the extraction pipeline that populates the rest of the store
/// (see [`super::fits_store::FitsStore::axref`] for the contrasting `AXREF`
/// case, which does go through that pipeline). `RFVAL` is read but never
/// written anywhere in `fitschan.c`; its only three appearances are the
/// comment and the two `GetValue` calls at fitschan.c:36217-36226, so its
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
    decode_wcs_inner(
        store,
        store.naxis,
        false,
        false,
        &CardList::new(),
        ' ',
        None,
        &mut Vec::new(),
        &mut Vec::new(),
    )
}

/// Where C's current card is as an axis description is read, which decides
/// where the `ASTWARN` blocks it raises are written: for each change, the
/// index in the warnings of the first warning raised after it, and
/// `Some(n)` for the card `DistortMaps` leaves current after reading NAXIS1
/// to NAXIS<n>, or `None` for end-of-file.
pub(crate) type ReadCursors = Vec<(usize, Option<usize>)>;

/// Inner decoder that also adds to `warnings` each warning raised (e.g. the
/// `badcel` warning for unknown celestial systems, fitschan.c:39491), in
/// the order C's `Warn` stores them, keeping those raised before an error.
///
/// `cards` is the FitsChan's own card list — the same `this` C's
/// `WcsCelestial` reads `RFVAL<i>` from (fitschan.c:36220-36226) — and `s`
/// is this plane's version character (`' '` for the primary plane, `'A'`..
/// `'Z'` for an alt plane). The pixel Frame has `npix` axes. Moves of C's
/// current card that decide where the warnings go are added to `cursors`.
/// `tabs` gives the FitsChan's tables for any `-TAB` axes, and is `None`
/// for an encoding that has none.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_wcs_inner(
    store: &FitsStore,
    npix: usize,
    def_b1950: bool,
    iwc: bool,
    cards: &CardList,
    s: char,
    tabs: Option<&mut TabReader<'_>>,
    warnings: &mut Warnings,
    cursors: &mut ReadCursors,
) -> AstResult<AnyObject> {
    let naxis = store.naxis;

    // Step 1: Build the pixel → IWC mapping. `DistortMaps`, which C
    // `WcsIntWorld` calls first, strips any Paper IV distortion code from the
    // store's CTYPE values, and every later step reads the truncated form —
    // so work on a local copy and pass that on rather than the caller's store.
    let mut store = store.clone();
    // C's `WcsFrame` first makes the look-up Mapping of any `-TAB` axes,
    // which removes the `-TAB` from their CTYPE values.
    let mut tab = match tabs {
        Some(tabs) => wcs_tab::tab_mapping(&mut store, s, tabs)?,
        None => None,
    };
    // C `WcsMapFrm` calls `WcsIntWorld` before `WcsCelestial` and `WcsSpectral`
    // (fitschan.c:37889-37999), and `Warn` inserts each block in front of the
    // same current card, so these warnings precede the ones the axis decoders
    // raise below.
    let pixel_to_iwc = wcs_matrix::build_pixel_to_iwc(&mut store, cards, warnings, cursors)?;
    let iwc_map = iwc.then(|| pixel_to_iwc.clone());
    let store = &store;

    // Step 2: Try to decode celestial axes.
    let rfval = read_rfval(cards, naxis, s);
    let celestial = wcs_celestial::build_celestial_with_def_b1950(
        store,
        s,
        def_b1950,
        Some(&rfval),
        tab.as_mut(),
        warnings,
    )?;
    // Capture the celestial fiducial position (radians) before the decoder
    // result is consumed below; it is stamped onto the SkyFrame as SkyRef.
    let celestial_ref_pos = celestial.as_ref().and_then(|c| c.ref_pos);
    // AXREF forces SkyRef unconditionally, bypassing the astTestSkyRef gate
    // that CRVAL / RFVAL are subject to (fitschan.c:36228-36239 vs
    // 36209-36223).
    let celestial_ref_pos_forced = celestial.as_ref().is_some_and(|c| c.ref_pos_forced);

    // Step 3: Decode the spectral axes, each SpecFrame given the celestial
    // reference position as C's `WcsSpectral` gives it.
    let spectral = wcs_spectral::build_spectral(store, s, spectral_ref_pos(&celestial))?;
    // `WcsCelestial` runs before `WcsSpectral` in `WcsMapFrm`
    // (fitschan.c:37984-37990), so the spectral warnings follow the
    // celestial ones.
    for sp in &spectral {
        warnings.extend(sp.warnings.iter().cloned());
    }

    // Step 4: Build the world frame (before consuming the decoders).
    let mut world_frame = build_world_frame(store, &celestial, &spectral)?;
    let iwc_map = iwc_map.map(|m| {
        (
            m,
            iwc_axis_units(store, &celestial, &spectral, tab.as_ref()),
        )
    });

    // Stamp the alternate-version character onto the world Frame's Ident, as
    // C `WcsMapFrm` does (`id[0] = s; astSetIdent(*frm, id)`,
    // fitschan.c:37885). For the primary axis description `s` is a space, so
    // the Frame carries `Ident = " "` — an explicitly-set blank identifier
    // that the native dump renders (with the inherited `IsA Object` block).
    set_frame_ident(world_frame.as_mut(), " ")?;

    // Stamp DUT1/DTAI onto the world Frame, as C `WcsMapFrm` does
    // immediately after the `astSetIdent` call above (fitschan.c:38012-38021).
    set_frame_dut1_dtai(
        world_frame.as_mut(),
        store.dut1.map(|v| v * 86_400.0),
        store.dtai,
    )?;

    // Step 5: Build the full pixel → world mapping (consumes the decoder
    // results). C `WcsMapFrm` (fitschan.c:37897-37939) returns
    // `astSimplify(map8)` — the pixel→WCS Mapping is simplified before it is
    // added to the FrameSet, so the stored edge (and any later native dump
    // of it) is in reduced form. The returned Mapping carries
    // `has_been_simplified` (→ `IsSimp = 1` in the native dump), mirroring C
    // `astSimplify` which sets the IsSimple flag on its result.
    let composed: DynMapping =
        build_pixel_to_world(store, pixel_to_iwc, celestial, spectral, tab.map(|t| t.map))?;
    let full_mapping: DynMapping = composed.simplify()?;

    // Step 8: Build the FrameSet.
    let mut frameset = FrameSet::new(pixel_frame(npix)?);
    add_wcs_frame(
        &mut frameset,
        npix,
        store,
        s,
        full_mapping,
        world_frame,
        iwc_map,
    )?;

    // Explicitly fix the GRID (pixel) Frame as the base, mirroring C
    // `astSetBase(ret, pixel)` (fitschan.c:12084). This marks the Base
    // attribute as set, so a subsequent native dump writes `Base` as an
    // active value rather than a commented-out default.
    frameset.set_base(FrameRef::Index(1))?;

    // Step 8b: stamp the celestial reference position onto the SkyFrame as
    // SkyRef + SkyRefIs=Ignored. Mirrors C `WcsCelestial`
    // (fitschan.c:36227-36239), which, when the SkyFrame's SkyRef has not
    // already been set, stores (reflon, reflat) and sets SkyRefIs=Ignored so
    // the SkyFrame still represents absolute celestial coordinates. Because
    // SkyRefIs=Ignored leaves the coordinate semantics unchanged, this adds
    // no remapping — we set it via `frame_mut` to bypass FrameSet integrity,
    // matching C which sets it on the Frame before the FrameSet wraps it.
    set_sky_ref_from_celestial(&mut frameset, celestial_ref_pos, celestial_ref_pos_forced)?;

    Ok(AnyObject::FrameSet(frameset))
}

/// The pixel Frame of a FITS-WCS read, with `npix` axes. C labels each
/// pixel axis "Pixel axis <n>" (fitschan.c:12183-12187).
fn pixel_frame(npix: usize) -> AstResult<DynFrame> {
    let mut builder = BasicFrameBuilder::new(npix)
        .domain("GRID")
        .title("Pixel Coordinates");
    for i in 0..npix {
        builder = builder.label(i + 1, format!("Pixel axis {}", i + 1));
    }
    Ok(Box::new(builder.build()?))
}

/// Join a Mapping whose inputs are an axis description's WCS axes to the
/// `npix` pixel axes, as C's `AddFrame` does (fitschan.c:2835-2862): a
/// PermMap passes the axes they share, gives extra WCS axes the value 0,
/// the default CRPIX, and drops extra pixel axes.
fn join_pixel_axes(npix: usize, map: DynMapping) -> AstResult<DynMapping> {
    let nwcs = map.nin();
    if nwcs == npix {
        return Ok(map);
    }
    // The PermMap's axis indices are 1-based; -1 is the first constant.
    let inperm: Vec<i64> = (0..npix)
        .map(|i| if i < nwcs { i as i64 + 1 } else { -1 })
        .collect();
    let outperm: Vec<i64> = (0..nwcs)
        .map(|i| if i < npix { i as i64 + 1 } else { -1 })
        .collect();
    let pmap = PermMap::new(&inperm, &outperm, &[0.0])?;
    Ok(Box::new(CmpMap::new(
        Box::new(pmap),
        map,
        Composition::Series,
    )?))
}

/// Add the WCS Frame of axis description `s` to `fs`, connected to its
/// base (pixel) Frame, which has `npix` axes, by `pixel_to_wcs`, and make it
/// current. With
/// `pixel_to_iwc` (the FitsChan's Iwc attribute set) a Frame of
/// intermediate world coordinates, with Domain `IWC` followed by `s`, is
/// added too, as C's `WcsFrame` and `AddFrame` add it
/// (fitschan.c:38357-38375, 2777-2785): the two Frames form a FrameSet,
/// WCS first, joined by the Mapping from WCS through pixel to IWC
/// coordinates, which is added to `fs` through the pixel to IWC Mapping.
fn add_wcs_frame(
    fs: &mut FrameSet,
    npix: usize,
    store: &FitsStore,
    s: char,
    pixel_to_wcs: DynMapping,
    world_frame: DynFrame,
    pixel_to_iwc: Option<(DynMapping, Vec<Option<String>>)>,
) -> AstResult<()> {
    let Some((map1, units)) = pixel_to_iwc else {
        return fs.add_frame(
            FrameRef::Base,
            join_pixel_axes(npix, pixel_to_wcs)?,
            world_frame,
        );
    };
    let wcsaxes = store.wcsaxes.unwrap_or(store.naxis);
    let domain = if s == ' ' {
        "IWC".to_string()
    } else {
        format!("IWC{s}")
    };
    let mut builder = BasicFrameBuilder::new(wcsaxes)
        .title("FITS Intermediate World Coordinates")
        .domain(&domain);
    for (i, unit) in units.into_iter().enumerate() {
        if let Some(unit) = unit {
            builder = builder.unit(i + 1, unit);
        }
    }
    let iwc_frame: DynFrame = Box::new(builder.build()?);
    // C inverts `ret`, makes the CmpMap, which records the inversion as
    // `InvA`, and inverts `ret` back, so the CmpMap holds `ret` in its own
    // orientation (fitschan.c `WcsFrame`).
    let orig = pixel_to_wcs.is_inverted();
    let mut wcs_to_pixel = pixel_to_wcs;
    wcs_to_pixel.invert();
    let mut joined = CmpMap::new(wcs_to_pixel, map1.clone(), Composition::Series)?;
    joined.set_first_orientation(orig);
    let joined: DynMapping = Box::new(joined);
    let wcs_to_iwc = joined.simplify()?;
    let mut pair = FrameSet::new(world_frame);
    pair.add_frame(FrameRef::Base, wcs_to_iwc, iwc_frame)?;
    let nframe = fs.nframe();
    fs.add_frameset(FrameRef::Base, join_pixel_axes(npix, map1)?, pair)?;
    // The WCS Frame, the first of the pair, is made current.
    fs.set_current(FrameRef::Index(nframe + 1))
}

/// The units C gives the axes of the IWC Frame: degrees for celestial axes
/// (`WcsCelestial`, fitschan.c:36611), the SpecFrame's unit for a spectral
/// axis (`WcsSpectral`, :39919), and any CUNIT for the others (`WcsOthers`,
/// :39248).
fn iwc_axis_units(
    store: &FitsStore,
    celestial: &Option<CelestialResult>,
    spectral: &[SpectralResult],
    tab: Option<&TabLookup>,
) -> Vec<Option<String>> {
    (0..store.naxis)
        .map(|i| {
            if celestial
                .as_ref()
                .is_some_and(|c| c.lon_axis == i || c.lat_axis == i)
            {
                // The IWC of a celestial `-TAB` axis are in unknown units
                // (fitschan.c `WcsCelestial`).
                if tab.is_some_and(|t| t.axes[i]) {
                    None
                } else {
                    Some("deg".to_string())
                }
            } else if let Some(sp) = spectral.iter().find(|sp| sp.axis == i) {
                crate::frames::Frame::unit(&sp.frame, 1).ok()
            } else {
                store.cunit[i].clone()
            }
        })
        .collect()
}

/// Stamp the celestial reference position onto the SkyFrame in the current
/// world frame as `SkyRef` + `SkyRefIs=Ignored`, mirroring C `WcsCelestial`
/// (fitschan.c:36227-36239, 36209-36223).
///
/// The reference position is the celestial reference longitude / latitude the
/// decoder resolved — CRVAL by default, overridden by `RFVAL<i>` and then by
/// `AXREF<i>`. When `forced` is `false` (the CRVAL / RFVAL case) C only sets
/// it when the SkyFrame's SkyRef has not already been set (`astTestSkyRef`,
/// fitschan.c:36232), and skips offset SkyFrames; we honour the same gate.
/// When `forced` is `true` (the AXREF case) C calls `astSetSkyRef`
/// unconditionally (fitschan.c:36349-36350), outside that gate. Setting
/// `SkyRefIs=Ignored` leaves absolute-coordinate semantics unchanged, so this
/// is applied via `frame_mut` without triggering a FrameSet-integrity remap.
fn set_sky_ref_from_celestial(
    fs: &mut FrameSet,
    ref_pos: Option<(f64, f64)>,
    forced: bool,
) -> AstResult<()> {
    // The reference position (radians) as resolved by the celestial decoder,
    // mirroring C `WcsCelestial` (fitschan.c:36141-36144 for the CRVAL case).
    // Nothing to stamp if it could not be determined.
    let Some((reflon, reflat)) = ref_pos else {
        return Ok(());
    };

    // Only proceed when the current frame actually contains a SkyFrame.
    if locate_sky_axes(fs.current()).is_none() {
        return Ok(());
    }

    // C stores the CRVAL / RFVAL reference position only when SkyRef is not
    // already set (fitschan.c:36484), and the AXREF one unconditionally
    // (fitschan.c:36599). An offset SkyFrame gets it too: TidyOffsets later
    // sets its SkyRefIs, and the stored position is its origin or pole.
    if !forced {
        if let Some(sky) = peek_sky(fs.current()) {
            if sky.test_sky_ref(1)? {
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
        sky.set_sky_ref_lon_lat(reflon, reflat);
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
/// loop. Each warning raised is added to `warnings`, for the read to store
/// as ASTWARN cards. `def_b1950` mirrors the FitsChan `DefB1950`
/// attribute, and `ignore_bad_alt` its `IgnoreBadAlt` attribute.
///
/// `cards` is the FitsChan's own card list, used to read `RFVAL<i>` /
/// `RFVAL<i><s>` for the primary plane and each alt plane respectively
/// (fitschan.c:36220-36226).
///
/// With no pixel axes, C's `MakeFitsFrameSet` makes nothing
/// (fitschan.c:12178), and neither does this. Without a primary axis
/// description the FrameSet starts with the pixel Frame alone, which stays
/// current.
///
/// For each axis description read, `cursors` receives the index in
/// `warnings` of the first warning it raises and its number of WCS axes,
/// which decide where C writes those warnings.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_wcs_with_alts_inner(
    stores: &WcsStores,
    def_b1950: bool,
    ignore_bad_alt: bool,
    iwc: bool,
    cards: &CardList,
    tabs: &mut TabReader<'_>,
    warnings: &mut Warnings,
    cursors: &mut ReadCursors,
) -> AstResult<Option<AnyObject>> {
    let npix = stores.npix;
    if npix == 0 {
        return Ok(None);
    }
    let alts = &stores.alts;

    // Step 1: build the primary FrameSet via the inner decoder. The Frame
    // current once it is added, the primary WCS Frame or else the pixel
    // Frame, is made current again once the alternates are added.
    let (mut fs, primary_index) = match &stores.primary {
        Some(primary) => {
            cursors.push((warnings.len(), Some(primary.naxis)));
            match decode_wcs_inner(
                primary,
                npix,
                def_b1950,
                iwc,
                cards,
                ' ',
                Some(&mut *tabs),
                warnings,
                cursors,
            )? {
                AnyObject::FrameSet(f) => (f, 2),
                _ => unreachable!("decode_wcs always returns AnyObject::FrameSet"),
            }
        }
        None => {
            let mut fs = FrameSet::new(pixel_frame(npix)?);
            fs.set_base(FrameRef::Index(1))?;
            (fs, 1)
        }
    };

    // Step 2: append one alt frame per (letter, alt_store). Each is added
    // to a copy of the FrameSet, kept only if every step succeeds: with
    // IgnoreBadAlt set, an alternate description that cannot be read is
    // left out, as C's `AddFrame` clears the error from `WcsMapFrm`.
    for (letter, alt_store) in alts {
        cursors.push((warnings.len(), Some(alt_store.naxis)));
        let mut trial = fs.clone();
        let mut alt_warnings = Vec::new();
        let mut alt_cursors = Vec::new();
        let added = (|| -> AstResult<()> {
            let warnings = &mut alt_warnings;
            // As in `decode_wcs_inner`, `DistortMaps` rewrites the CTYPE values, so
            // this plane's store is copied and the copy carries on downstream.
            let mut alt_store = alt_store.clone();
            let mut tab = wcs_tab::tab_mapping(&mut alt_store, *letter, tabs)?;
            let pixel_to_iwc =
                wcs_matrix::build_pixel_to_iwc(&mut alt_store, cards, warnings, &mut alt_cursors)?;
            let iwc_map = iwc.then(|| pixel_to_iwc.clone());
            let alt_store = &alt_store;
            let rfval = read_rfval(cards, alt_store.naxis, *letter);
            let celestial = wcs_celestial::build_celestial_with_def_b1950(
                alt_store,
                *letter,
                def_b1950,
                Some(&rfval),
                tab.as_mut(),
                warnings,
            )?;
            // Capture the alt plane's celestial fiducial before the decoder is
            // consumed, so the alt SkyFrame gets the same SkyRef/SkyRefIs=Ignored
            // stamp as the primary (C applies WcsCelestial's SkyRef-from-fiducial
            // to every plane, not just the primary).
            let alt_ref_pos = celestial.as_ref().and_then(|c| c.ref_pos);
            let alt_ref_pos_forced = celestial.as_ref().is_some_and(|c| c.ref_pos_forced);
            let spectral =
                wcs_spectral::build_spectral(alt_store, *letter, spectral_ref_pos(&celestial))?;
            let mut world_frame = build_world_frame(alt_store, &celestial, &spectral)?;
            let iwc_map = iwc_map.map(|m| {
                (
                    m,
                    iwc_axis_units(alt_store, &celestial, &spectral, tab.as_ref()),
                )
            });
            // Simplify the pixel→world Mapping, mirroring C `astSimplify` on each
            // plane (and the primary path above) — `simplify()` sets the
            // has_been_simplified flag so the native dump writes `IsSimp = 1`.
            let composed: DynMapping = build_pixel_to_world(
                alt_store,
                pixel_to_iwc,
                celestial,
                spectral,
                tab.map(|t| t.map),
            )?;
            let full_mapping: DynMapping = composed.simplify()?;

            // Stamp the version letter onto the alt frame so the writer
            // can recover it.
            set_frame_ident(world_frame.as_mut(), &letter.to_string())?;

            // Stamp DUT1/DTAI onto the alt world Frame, exactly as for the
            // primary plane above — C `WcsMapFrm` runs once per plane
            // (fitschan.c:2710 loops over version characters).
            set_frame_dut1_dtai(
                world_frame.as_mut(),
                alt_store.dut1.map(|v| v * 86_400.0),
                alt_store.dtai,
            )?;

            // add_frame moves `current` to the newly-added alt frame, so the
            // SkyRef/RefRA-RefDec/DSB stamps below target it.
            add_wcs_frame(
                &mut trial,
                npix,
                alt_store,
                *letter,
                full_mapping,
                world_frame,
                iwc_map,
            )?;
            set_sky_ref_from_celestial(&mut trial, alt_ref_pos, alt_ref_pos_forced)?;

            Ok(())
        })();
        let base = warnings.len();
        cursors.extend(alt_cursors.into_iter().map(|(i, c)| (base + i, c)));
        warnings.append(&mut alt_warnings);
        match added {
            Ok(()) => fs = trial,
            Err(_) if ignore_bad_alt => warnings.push(Warning::new(
                "badalt",
                format!("Ignoring unusable FITS-WCS alternate axes label '{letter}'."),
            )),
            Err(e) => return Err(e),
        }
    }

    // Restore current to the primary frame (add_frame moved it).
    if !alts.is_empty() {
        fs.set_current(FrameRef::Index(primary_index))?;
    }

    // Tidy up offset SkyFrames. Mirrors C `TidyOffsets`
    // (fitschan.c:34497-34669): converts the SKY_POLE / SKY_ORIGIN
    // domain markers stamped by `WcsSkyFrame` into actual SkyRefIs
    // attribute values, and removes redundant SKY_OFFSETS Frames
    // when a corresponding absolute SkyFrame exists.
    tidy_offsets(&mut fs)?;

    Ok(Some(AnyObject::FrameSet(fs)))
}

/// Apply C `TidyOffsets` (fitschan.c:34497-34669) to a freshly-read
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
            // C `TidyOffsets` (fitschan.c:34593-34597) sets only the
            // SkyRefIs attribute here; it does NOT alter the Domain. The
            // SKY_POLE / SKY_ORIGIN marker Domain is retained on the
            // Frame and is only cleared later, in the `hasabs && hasoff`
            // removal pass (`astClearDomain`, fitschan.c:34622) — which
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
    // Which Frames carried a SKY_POLE / SKY_ORIGIN marker Domain: pass 2 clears
    // Ident and Domain on exactly these, and not on a plain "SKY" Frame.
    let mut markers = Vec::with_capacity(nfrm);
    for ifrm in 1..=nfrm {
        let (kind, marker) = {
            let f = fs.frame(FrameRef::Index(ifrm))?;
            classify(f)
        };
        markers.push(marker);
        if let Some(skyrefis) = marker {
            // C `TidyOffsets` (fitschan.c:34593-34597) sets SkyRefIs
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
    // Mirrors C:34541-34580.
    if has_abs && has_off {
        // C walks downwards so an `astRemoveFrame` never disturbs an index it
        // has yet to visit (fitschan.c:34673).
        let mut icurr_is_offset = icurr_1 <= kinds.len() && kinds[icurr_1 - 1] == FrameKind::Offset;
        for ifrm in (1..=nfrm).rev() {
            match kinds[ifrm - 1] {
                FrameKind::Offset => {
                    fs.remove_frame(FrameRef::Index(ifrm))?;
                }
                FrameKind::Absolute => {
                    // Only the marker Domains reach the clearing arm in C
                    // (fitschan.c:34687-34688): a Frame whose SkyFrame Domain
                    // was already "SKY" is left entirely alone.
                    if markers[ifrm - 1].is_none() {
                        continue;
                    }
                    // Ident holds the FITS-WCS alternate axis description
                    // character; the marker Domain has served its purpose once
                    // the offset Frames are going away. C clears Ident on the
                    // Frame and Domain on the *primary* Frame of the axis —
                    // i.e. the nested SkyFrame, not the compound wrapper.
                    let frame = fs.frame_mut(FrameRef::Index(ifrm))?;
                    frame.clear_ident()?;
                    clear_sky_domain(frame);

                    // If the Frame that was current is being deleted for being
                    // an offset Frame, hand current to the first absolute Frame
                    // reached instead (fitschan.c:34693-34696).
                    if icurr_is_offset {
                        fs.set_current(FrameRef::Index(ifrm))?;
                        icurr_is_offset = false;
                    }
                }
                FrameKind::Other => {}
            }
        }
        // C leaves Current wherever the loop put it: either the original Frame
        // (still present, its index tracked by astRemoveFrame) or the absolute
        // Frame promoted above. Restoring `icurr_1` by index here would instead
        // name whichever Frame the removals shifted into that slot.
        if !icurr_is_offset && kinds[icurr_1 - 1] != FrameKind::Offset {
            // Translate the original index through the removals.
            let removed_below = (1..icurr_1)
                .filter(|i| kinds[i - 1] == FrameKind::Offset)
                .count();
            fs.set_current(FrameRef::Index(icurr_1 - removed_below))?;
        }
    } else {
        // No removals — restore the original current.
        fs.set_current(FrameRef::Index(icurr_1))?;
    }

    Ok(())
}

/// Clear the Domain of the SkyFrame reached by `astPrimaryFrame` — the nested
/// SkyFrame within a compound Frame, or the Frame itself when it is one.
/// Mirrors C's `astClearDomain( pfrm )` (fitschan.c:34688), where `pfrm` is the
/// primary Frame of the axis being examined rather than the Frame itself.
fn clear_sky_domain(frame: &mut dyn crate::frames::Frame) {
    use crate::frames::{CmpFrame, SkyFrame};
    if let Some(sky) = frame.as_any_mut().downcast_mut::<SkyFrame>() {
        sky.clear_domain();
        return;
    }
    if let Some(cmp) = frame.as_any_mut().downcast_mut::<CmpFrame>() {
        clear_sky_domain(cmp.frame1_mut());
        clear_sky_domain(cmp.frame2_mut());
    }
}

/// Helper: stamp `ident` onto a boxed Frame. `Ident` is an Object-level
/// attribute in C (`object.c:5225`), so C's `astSetIdent( *frm, id )`
/// (fitschan.c:37885) applies whatever class the world Frame turned out to
/// be; the trait method carries the same property here.
fn set_frame_ident(frame: &mut dyn crate::frames::Frame, ident: &str) -> AstResult<()> {
    frame.set_ident(ident)
}

/// Helper: stamp DUT1/DTAI onto a boxed Frame, mirroring C `WcsMapFrm`'s
/// calls to `astSetDut1` / `astSetDtai` on `*frm` (fitschan.c:38012-38021).
/// Both are Frame-class attributes (`frame.h:622`), so every Frame class
/// takes them. `dut1` is in seconds (the FITS `DUT1` keyword is in days; the
/// caller converts by `SPD` before calling this).
fn set_frame_dut1_dtai(
    frame: &mut dyn crate::frames::Frame,
    dut1: Option<f64>,
    dtai: Option<f64>,
) -> AstResult<()> {
    if let Some(v) = dut1 {
        frame.set_dut1(v)?;
    }
    if let Some(v) = dtai {
        frame.set_dtai(v)?;
    }
    Ok(())
}

/// Build the full pixel → world mapping (consumes the decoder results).
///
/// Mirrors C `WcsMapFrm` (fitschan.c:38341-38348), which builds
/// `map8 = ((((map1 ; map2) ; map3) ; map4) ; tabmap)` from the stages in
/// their C shapes, since `astSimplify` can reach a different last digit from
/// a differently shaped chain:
///
/// - `map1`, pixel to IWC (`WcsIntWorld`);
/// - `map2`, the celestial stage, or `UnitMap(naxis)` when there are no
///   celestial axes (`WcsCelestial`);
/// - `map3`, one Mapping per axis in parallel, the spectral one where there is
///   a spectral axis and `UnitMap(1)` elsewhere (`WcsSpectral`);
/// - `map4`, `UnitMap(naxis)` when every axis is claimed, and otherwise one
///   Mapping per axis in parallel: [`others_axis_map`] for an unclaimed axis
///   and `UnitMap(1)` for a claimed one (`WcsOthers`);
/// - the look-ups of any `-TAB` axes.
fn build_pixel_to_world(
    store: &FitsStore,
    pixel_to_iwc: DynMapping,
    celestial: Option<CelestialResult>,
    spectral: Vec<SpectralResult>,
    tabmap: Option<DynMapping>,
) -> AstResult<DynMapping> {
    let naxis = store.naxis;

    // Track which axes are claimed by decoders.
    let mut claimed = vec![false; naxis];
    if let Some(ref cel) = celestial {
        claimed[cel.lon_axis] = true;
        claimed[cel.lat_axis] = true;
    }
    for spec in &spectral {
        claimed[spec.axis] = true;
    }

    let map2: DynMapping = match celestial {
        Some(cel) => cel.mapping,
        None => Box::new(UnitMap::new(naxis)),
    };
    let mut spectral_maps: Vec<Option<DynMapping>> = (0..naxis).map(|_| None).collect();
    for spec in spectral {
        spectral_maps[spec.axis] = Some(spec.mapping);
    }
    let map3: DynMapping = parallel_per_axis(
        spectral_maps
            .into_iter()
            .map(|m| Ok(m.unwrap_or_else(|| Box::new(UnitMap::new(1)) as DynMapping))),
    )?;
    let map4: DynMapping = if claimed.contains(&false) {
        parallel_per_axis((0..naxis).map(|i| {
            if claimed[i] {
                Ok(Box::new(UnitMap::new(1)) as DynMapping)
            } else {
                others_axis_map(store, i)
            }
        }))?
    } else {
        Box::new(UnitMap::new(naxis))
    };

    let mut result: DynMapping = pixel_to_iwc;
    for part in [Some(map2), Some(map3), Some(map4), tabmap]
        .into_iter()
        .flatten()
    {
        result = Box::new(CmpMap::new(result, part, Composition::Series)?);
    }
    Ok(result)
}

/// The per-axis Mappings `parts`, joined in parallel from the left, as C's
/// per-axis loops in `WcsSpectral` and `WcsOthers` join them.
fn parallel_per_axis(parts: impl Iterator<Item = AstResult<DynMapping>>) -> AstResult<DynMapping> {
    let mut result: Option<DynMapping> = None;
    for part in parts {
        let part = part?;
        result = Some(match result {
            None => part,
            Some(acc) => Box::new(CmpMap::new(acc, part, Composition::Parallel)?),
        });
    }
    Ok(result.expect("a FITS-WCS description has at least one axis"))
}

/// The IWC to WCS Mapping C's `WcsOthers` makes for an axis no other stage
/// claims (fitschan.c:39355-39368): `LogWcs` for a CTYPE ending in `-LOG`,
/// and otherwise `LinearWcs`, a ShiftMap adding CRVAL, or a UnitMap when
/// CRVAL is zero. A `-LOG` axis with a zero CRVAL has no logarithmic form and
/// is given a UnitMap, as `WcsSpectral` gives a spectral one.
fn others_axis_map(store: &FitsStore, axis: usize) -> AstResult<DynMapping> {
    let crval = store.crval[axis].unwrap_or(0.0);
    let ctype = store.ctype[axis].as_deref().unwrap_or("");
    if ctype.len() > 3 && ctype.ends_with("-LOG") {
        return Ok(wcs_spectral::log_wcs(crval).unwrap_or_else(|| Box::new(UnitMap::new(1))));
    }
    Ok(if crval != 0.0 {
        Box::new(ShiftMap::new(vec![crval]))
    } else {
        Box::new(UnitMap::new(1))
    })
}

/// The celestial reference position for `WcsSpectral`'s `astSetRefPos`:
/// the SkyFrame `WcsCelestial` made and its reference longitude and
/// latitude, when there are celestial axes with a reference position.
fn spectral_ref_pos(celestial: &Option<CelestialResult>) -> Option<wcs_spectral::RefPos<'_>> {
    let cel = celestial.as_ref()?;
    let (lon, lat) = cel.ref_pos?;
    Some((&cel.frame, lon, lat))
}

/// Build the world coordinate frame based on what decoders found.
fn build_world_frame(
    store: &FitsStore,
    celestial: &Option<CelestialResult>,
    spectral: &[SpectralResult],
) -> AstResult<DynFrame> {
    let naxis = store.naxis;

    // Track unclaimed axes.
    let mut claimed = vec![false; naxis];
    if let Some(cel) = celestial {
        claimed[cel.lon_axis] = true;
        claimed[cel.lat_axis] = true;
    }
    for spec in spectral {
        claimed[spec.axis] = true;
    }

    let n_unclaimed = claimed.iter().filter(|&&c| !c).count();

    // Simple cases: a single class spanning all axes returns its bare frame.
    if celestial.is_some() && spectral.is_empty() && n_unclaimed == 0 {
        let cel = celestial.as_ref().unwrap();
        return Ok(Box::new(cel.frame.clone()));
    }
    if let [spec] = spectral
        && celestial.is_none()
        && n_unclaimed == 0
    {
        return Ok(spec.world_frame());
    }
    // The axes no class stage claimed — the ones whose CTYPEs make up the
    // leftover Frame's Domain (C's sentinel-tagged axes, fitschan.c:38990).
    let unclaimed: Vec<usize> = (0..naxis).filter(|&i| !claimed[i]).collect();

    if celestial.is_none() && spectral.is_empty() {
        // All-linear: a single BasicFrame carrying the per-axis CTYPE info.
        let mut frame = seed_linear_frame(store, &(0..naxis).collect::<Vec<_>>(), &unclaimed)?;
        apply_wcsname(store, frame.as_mut())?;
        return Ok(frame);
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
    // symbol / label / unit (the WcsOthers step, fitschan.c:38751 — it
    // only affects the linear axes, since the celestial/spectral axes are
    // replaced by their own frames below).
    let mut frame: DynFrame =
        seed_linear_frame(store, &(0..naxis).collect::<Vec<_>>(), &unclaimed)?;

    // --- Celestial stage (WcsCelestial, fitschan.c:36172-36215) ---
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

    // --- Spectral stage (WcsSpectral, fitschan.c:39753-39791), one axis at
    // a time in axis order ---
    for spec in spectral {
        frame = morph_in_sub_frame(frame, spec.world_frame(), &[spec.axis], naxis)?;
    }

    // WcsOthers, which runs only when some axes are left to it.
    if !unclaimed.is_empty() {
        apply_wcsname(store, frame.as_mut())?;
    }
    Ok(frame)
}

/// C's `WcsOthers` ending: a WCSNAME keyword names the whole Frame, as its
/// Domain and in its Title.
fn apply_wcsname(store: &FitsStore, frame: &mut dyn crate::frames::Frame) -> AstResult<()> {
    if let Some(name) = store.wcsname.as_deref() {
        frame.set_domain(name)?;
        frame.set_title(&format!("{name} coordinates"))?;
    }
    Ok(())
}

/// The CTYPE value C `WcsOthers` works from for one axis: the keyword value
/// with any trailing `-LOG` removed, defaulting to `AXIS<n>` when the header
/// carries no CTYPE for the axis (fitschan.c:38995-39014). C uses this same
/// `buf2` string for the axis Symbol, for the Label fallback, and as the
/// contribution to the Frame's Domain.
fn others_axis_ctype(store: &FitsStore, axis: usize) -> String {
    let ctype = store.ctype[axis]
        .clone()
        .unwrap_or_else(|| format!("AXIS{}", axis + 1));
    match ctype.strip_suffix("-LOG") {
        Some(stripped) => stripped.to_string(),
        None => ctype,
    }
}

/// Build the Domain for the Frame holding the axes no class stage claimed.
///
/// C `WcsOthers` (fitschan.c:39016-39074) joins those axes' CTYPE values with
/// `-` separators — each already `-LOG`-stripped — and sets the result as the
/// Domain of the primary Frame containing them, or clears the Domain when no
/// axis contributed a non-blank value.
///
/// C identifies the axes by seeding the whole physical Frame with a sentinel
/// `Domain=AST_FITSCHAN` (fitschan.c:37952) and then testing which axes still
/// carry it once the class stages have replaced the rest. Here the caller
/// already holds the leftover axis list, so there is nothing for a sentinel to
/// determine.
fn ctype_domain(store: &FitsStore, axes: &[usize]) -> Option<String> {
    let mut parts: Vec<String> = Vec::with_capacity(axes.len());
    for &axis in axes {
        // C's guard is `astChrLen( ckeyval ) > 0` on the *raw* keyword value,
        // so a blank CTYPE contributes nothing to the Domain even though it
        // still supplies the axis Symbol.
        let raw_is_blank = store.ctype[axis]
            .as_deref()
            .is_some_and(|c| c.trim().is_empty());
        if raw_is_blank {
            continue;
        }
        parts.push(others_axis_ctype(store, axis));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("-"))
    }
}

/// Whether C `WcsOthers` labels the axes no class stage claimed (`others`,
/// ascending) with the comments of their CTYPE cards (fitschan.c:39258-39324):
/// only when there is more than one such axis, each has a non-blank comment,
/// and no comment repeats an earlier one.
///
/// C keeps the comments in an array indexed by axis but compares each with
/// the entries at the indices of the earlier *other* axes counted from zero,
/// which are the same axes only when the other axes come first. The
/// comparison here is C's.
fn use_ctype_comments(store: &FitsStore, others: &[usize]) -> bool {
    let naxes = store.ctype_com.len();
    let mut comms: Vec<Option<&str>> = vec![None; naxes];
    let mut usecom = true;
    let mut nother = 0;
    for i in 0..naxes {
        if !others.contains(&i) {
            continue;
        }
        nother += 1;
        match store.ctype_com[i].as_deref() {
            Some(com) if !com.trim_end().is_empty() => {
                if comms[..nother - 1].contains(&Some(com)) {
                    usecom = false;
                }
            }
            _ => usecom = false,
        }
        if usecom {
            comms[i] = store.ctype_com[i].as_deref();
        }
    }
    usecom && nother > 1
}

/// Build a BasicFrame over the given FITS axes (0-based), carrying each
/// axis's CTYPE symbol, label (CNAME, else the CTYPE comment as
/// [`use_ctype_comments`] decides, else CTYPE), and unit. Mirrors the
/// per-axis attribute setup C performs (WcsOthers `astSetSymbol`/`astSetLabel`,
/// fitschan.c:39029-39041).
///
/// `domain_axes` lists the axes whose CTYPEs make up the Frame's Domain — the
/// unclassified axes only, which is the subset C's sentinel walk arrives at.
/// It is a subset of `axes`, since the caller seeds the Frame over every axis
/// and lets the celestial / spectral stages replace the ones they claim.
fn seed_linear_frame(
    store: &FitsStore,
    axes: &[usize],
    domain_axes: &[usize],
) -> AstResult<DynFrame> {
    let usecom = use_ctype_comments(store, domain_axes);
    let mut builder = BasicFrameBuilder::new(axes.len());
    for (k, &axis) in axes.iter().enumerate() {
        let idx_1based = k + 1;
        let ctype = others_axis_ctype(store, axis);
        builder = builder.symbol(idx_1based, ctype.clone());
        let comment = store.ctype_com[axis]
            .as_ref()
            .filter(|_| usecom && domain_axes.contains(&axis));
        if let Some(label) = store.cname[axis].as_ref().or(comment) {
            builder = builder.label(idx_1based, label.clone());
        } else {
            builder = builder.label(idx_1based, ctype);
        }
        if let Some(unit) = &store.cunit[axis] {
            builder = builder.unit(idx_1based, unit.clone());
        }
    }
    if let Some(domain) = ctype_domain(store, domain_axes) {
        builder = builder.domain(&domain);
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

    // Other axes (FITS order), 0-based.
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

    // C's `astPickAxes`, whose SubFrame overlays the source Frame's
    // attributes, ActiveUnit among them, onto the Frame it makes.
    let pick: Vec<usize> = others.iter().map(|&i| i as usize + 1).collect();
    let (ofrm, _) = crate::frames::pick_axes(frame.as_ref(), &pick)?;
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

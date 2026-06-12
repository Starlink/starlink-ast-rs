//! Port of `astFsetToStore` and its callees (`CelestialAxes`,
//! `SpectralAxes`, `OtherAxes`, `MakeIntWorld`, `SIPIntWorld`).
//!
//! Builds a [`FitsStore`](super::fits_store::FitsStore) from a
//! `FrameSet` by constructing the `wcs→iwc` and `pixel→iwc` mappings
//! from chosen header values, then extracting CRPIX / CD / PC / CDELT
//! via a least-squares fit. Mirrors `fitschan.c` (function references
//! on each item).
//!
//! See `docs/superpower/specs/2026-05-25-makeintworld-port-design.md`
//! for the design.
//!
//! Naming note: the upstream-equivalent module name `wcs_keywords/`
//! collides with the existing `fitschan/wcs_keywords.rs` reading-side
//! helper; we use `fset_to_store/` (matching the C entry-point name)
//! to keep the read/write halves of the pipeline distinguishable.

// `fset_to_store` is wired into `wcs_write::store_from_frameset_to`
// in Task 7.2; until then nothing here is reachable from outside the
// cfg(test) block, so allow dead_code for the whole module. The
// index-style loops in snap_small_crval / round_crpix mirror the C
// source line-for-line; revisited in the post-validation idiomatic
// pass.
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

mod celestial_axes;
pub(crate) mod fiducial;
mod grism;
mod invertable;
mod is_map_linear;
mod log_axis;
mod make_int_world;
mod orth;
mod other_axes;
mod sip_int_world;
pub(crate) mod sky_pole;
mod sky_sys;
mod spectral_axes;
mod world_axes;

use crate::error::AstResult;
use crate::fitschan::Encoding;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{Frame, FrameRef, FrameSet};
use crate::mappings::{CmpMap, Composition, DynMapping};

use celestial_axes::{CelestialAxesArgs, celestial_axes};
use make_int_world::make_int_world;
use other_axes::{OtherAxesArgs, other_axes};
use spectral_axes::{SpectralAxesArgs, spectral_axes};
use world_axes::world_axes;

/// Default `FitsTol` value (matches the C library's FitsChan default).
/// Mirrors C `astGetFitsTol` default at fitschan.c:2898.
const FITS_TOL_DEFAULT: f64 = 0.1;

/// Build a [`FitsStore`] describing the primary plane of the supplied
/// FrameSet. Mirrors the body of `fitschan.c::AddVersion` (~2674) for
/// the `s == ' '` (primary) case — `FsetToStore` itself is a thin
/// wrapper that dispatches to `AddVersion` per coordinate-version
/// letter, but the heavy lifting (building the WCS→IWC mapping
/// segment-by-segment, then extracting CRPIX/CD/PC/CDELT) is here.
///
/// `dim[i]` carries each base-frame axis's pixel length (`None` when
/// unknown — mirrors C `dim[i] == AST__BAD`). The returned `FitsStore`
/// has length `naxis = fs.base().naxes()`.
///
/// `encoding` selects the FITS-WCS variant; this gates whether SIP
/// distortion headers may be emitted.
///
/// Currently emits primary-plane keywords only. Alternate axis
/// descriptions (the `s = 'A'..'Z'` loop in C `FsetToStore` after the
/// primary `AddVersion` call) are out of scope for this port.
pub(crate) fn fset_to_store(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
    s: char,
    input_crvals: &[Option<f64>],
) -> AstResult<FitsStore> {
    // Step 0: build the FITS-WCS-conforming FrameSet. Mirrors C
    // `MakeFitsFrameSet` (fitschan.c:21083). The relevant action for
    // bit-exactness is the SpecFrame/SkyFrame fiducial propagation:
    // when the WCS frame contains both, the SpecFrame's
    // RefRA/RefDec are overwritten with the celestial axes' fiducial
    // (computed via the WcsMap), through the FrameSet integrity
    // machinery — which re-fuses the pixel→spec mapping. Without
    // this, the bounds returned by astMapBox over the spec axis
    // can differ from C by O(1e-2 m/s) for typical millimetre-wave
    // velocity systems.
    let mut owned_fs = make_fits_frameset(fs, encoding)?;
    let fs = &mut owned_fs;

    // Step 1: pixel→WCS Mapping from base to current frame.
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;

    let nwcs = mapping.nout();
    // FitsStore needs to be wide enough to hold a slot for every WCS
    // axis as well as every base (pixel) axis: when nwcs > base.naxes()
    // the FrameSet has degenerate axes whose CRVAL / CTYPE still need
    // to be stored. The C library handles this via FitsStore's dynamic
    // SetItem; we pre-size to max(naxes, nwcs).
    let naxis = fs.base().naxes().max(nwcs);
    let mut store = FitsStore::new(naxis);

    // Reject inputs whose inverse is undefined (mirrors C:2782 — the
    // -TAB fallback path in C is not yet ported, so for now we mirror
    // the non-TAB return).
    if !mapping.has_inverse() {
        return Ok(store);
    }

    // Step 2: choose `wperm[i]` — the FITS axis index for each WCS
    // frame axis. Mirrors C:2792-2802 (FitsAxisOrder + WorldAxes
    // fallback). FitsAxisOrder isn't yet modelled; the auto path via
    // WorldAxes is what the C library defaults to.
    let mut wperm: Vec<usize> = match world_axes(mapping.as_ref(), dim, FITS_TOL_DEFAULT)? {
        Some(p) => p,
        None => (0..nwcs).collect(),
    };

    // Step 3: per-axis "this axis has been described" flags.
    let mut axis_done = vec![false; nwcs];

    // Step 4: per-axis chosen CRVAL — initialised from the caller-
    // supplied `input_crvals` (the input header's CRVAL{i}, mapped
    // by FITS axis index) and populated by celestial_axes /
    // spectral_axes for the axes they own; remaining `None` slots
    // carry through to other_axes which falls back to world(g0).
    // Mirrors C's `crvals` array (allocated by ReadCrval @ 2812).
    //
    // `input_crvals` is FITS-axis-indexed, but the per-axis writers
    // address `args.crvals[wcs_iax]`. Permute via `wperm`: the
    // FITS-axis index for WCS axis i is `wperm[i]`.
    let mut crvals: Vec<Option<f64>> = vec![None; nwcs];
    for i in 0..nwcs {
        let fits_i = wperm[i];
        crvals[i] = input_crvals.get(fits_i).copied().flatten();
    }

    // Step 5: build the WCS→IWC mapping segment by segment.
    //
    // The structure mirrors C:2826-2848:
    //   iwcmap = CelestialAxes(...)
    //   tmap   = SpectralAxes(...)
    //   iwcmap = iwcmap ∘ tmap
    //   tmap   = OtherAxes(...)
    //   iwcmap = iwcmap ∘ tmap
    //
    // Each segment defaults to UnitMap on the axes it doesn't claim,
    // so series composition leaves those axes untouched.
    // Detect whether the current Frame contains an offset SkyFrame
    // (SkyRefIs == Pole or Origin). Mirrors C `IsSkyOff`
    // (fitschan.c:20222), called from FsetToStore at line 12155
    // before the primary AddVersion. When set, SkySys writes
    // `OFLN`/`OFLT` CTYPEs and `WCSNAME = 'SKY_POLE'` /
    // `'SKY_ORIGIN'` instead of the standard equatorial CTYPEs.
    let isoff = sky_sys::is_sky_off(fs, fs.current_idx());

    let cel_seg = celestial_axes(CelestialAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        s,
        store: &mut store,
        axis_done: &mut axis_done,
        isoff,
    })?;

    let spec_seg = spectral_axes(SpectralAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        s,
        store: &mut store,
        crvals: &crvals,
        axis_done: &mut axis_done,
    })?;

    let other_seg = other_axes(OtherAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        store: &mut store,
        crvals: &mut crvals,
        axis_done: &mut axis_done,
    })?;

    let wcs_iwc: DynMapping = Box::new(CmpMap::new(cel_seg, spec_seg, Composition::Series)?);
    let wcs_iwc: DynMapping = Box::new(CmpMap::new(wcs_iwc, other_seg, Composition::Series)?);

    // Step 6: pixel→IWC = pixel→WCS ∘ WCS→IWC. Mirrors C:2853.
    let pixel_iwc: DynMapping = Box::new(CmpMap::new(
        mapping.clone_box(),
        wcs_iwc,
        Composition::Series,
    )?);

    // Step 7: extract CRPIX / CD / PC / CDELT (and SIP if enabled)
    // via make_int_world. Mirrors C:2870-2871.
    let sip_ok = matches!(encoding, Encoding::FitsWcs);
    let _ok = make_int_world(
        pixel_iwc.as_ref(),
        fs.current(),
        &wperm,
        &mut store,
        dim,
        FITS_TOL_DEFAULT,
        sip_ok,
    )?;

    // Step 8: post-processing.
    //
    // (a) Store the WCS Frame's Domain as WCSNAME when explicitly set.
    //     Mirrors C:2892-2895 (which uses astTestDomain). Each Frame
    //     class returns true from `test_domain` only when its domain
    //     attribute has been set by the user, distinguishing that
    //     from the per-class default ("SKY", "SPECTRUM", "TIME", …).
    //     Don't overwrite a value sky_sys already populated.
    if store.wcsname.is_none() {
        let cur = fs.current();
        if cur.test_domain() {
            let d = cur.domain();
            if !d.is_empty() {
                store.wcsname = Some(d.to_string());
            }
        }
    }

    // (b) Snap small-CRVAL residue to zero (when |CRVAL| is much
    //     smaller than the per-pixel scale). Mirrors C:2920-2942.
    snap_small_crval(&mut store, &wperm, nwcs);

    // (c) Round CRPIX values to the nearest microsecond pixel.
    //     Mirrors C:2945-2954.
    round_crpix(&mut store);

    // (d) FITS-CLASS spatial-CRVAL adjustment when the spatial pixel
    //     axes are degenerate. Mirrors fitschan.c:6465-6496 inside
    //     CLASSFromStore. Done here (rather than in the enc_class
    //     writer) because it mutates the FitsStore based on the
    //     FrameSet, which the writer does not have access to.
    if matches!(encoding, Encoding::FitsClass) {
        class_adjust_spatial_crval(fs, &mut store, dim, &wperm)?;
    }

    Ok(store)
}

/// Mirrors C `MakeFitsFrameSet` (fitschan.c:21083) — the writer-side
/// preconditioning that ensures the FrameSet matches FITS-WCS paper
/// requirements before per-axis encoding runs.
///
/// Currently models only the SpecFrame/SkyFrame fiducial propagation:
/// when the WCS Frame contains both a SpecFrame and a SkyFrame, the
/// SpecFrame's RefRA/RefDec are overwritten with the celestial axes'
/// fiducial position (computed via the WcsMap), through the FrameSet
/// integrity machinery — which re-fuses the pixel→spec mapping. C
/// always overwrites RefRA/RefDec when both attributes are already
/// set and a SkyFrame is present (see fitschan.c:21344-21509); we
/// mirror that exactly.
///
/// The other branches of C `MakeFitsFrameSet` — adding new sky axes
/// when the SpecFrame has RefRA/RefDec but no SkyFrame, and remapping
/// non-SkyFrame celestial axes from radians to degrees — are
/// out-of-scope for the current fixture coverage and tracked in
/// docs/issues/fset_to_store-port-gaps.md.
pub(crate) fn make_fits_frameset(fs: &FrameSet, encoding: Encoding) -> AstResult<FrameSet> {
    use crate::fitschan::fset_to_store::celestial_axes;
    use crate::frames::{SkySystem, SpecFrame};
    use crate::mappings::WcsMap;

    let mut owned = fs.clone();

    // Locate spec/sky in the world frame. Walk the axes via
    // primary_frame, in case the world frame is a CmpFrame whose
    // children are themselves CmpFrames.
    let nwcs = owned.current().naxes();
    let mut spec_axis: Option<usize> = None;
    let mut sky_lon: Option<usize> = None;
    let mut sky_lat: Option<usize> = None;
    for iax in 0..nwcs {
        let (pframe, paxis) = owned.current().primary_frame(iax);
        if pframe.as_any().downcast_ref::<SpecFrame>().is_some()
            || pframe
                .as_any()
                .downcast_ref::<crate::frames::DSBSpecFrame>()
                .is_some()
        {
            if spec_axis.is_some() {
                // C aborts MakeFitsFrameSet on duplicate spec axes;
                // for now just return the original FrameSet unchanged.
                return Ok(owned);
            }
            spec_axis = Some(iax);
        } else if pframe
            .as_any()
            .downcast_ref::<crate::frames::SkyFrame>()
            .is_some()
        {
            if paxis == 0 {
                sky_lon = Some(iax);
            } else {
                sky_lat = Some(iax);
            }
        }
    }

    // Only proceed when both a SpecFrame and a SkyFrame (lon+lat) are
    // present. Mirrors C's gate at fitschan.c:21344-21351 (in the
    // `else` branch where both RefRA AND RefDec are already set —
    // which for our wcsconv fixtures is always the case when a
    // DSBSpecFrame is present in the .ast).
    let (Some(_spec_iax), Some(ilon), Some(ilat)) = (spec_axis, sky_lon, sky_lat) else {
        return Ok(owned);
    };

    // C gate at fitschan.c:21344-21351: only proceed into the
    // RefRA/RefDec-overwrite branch if both attributes are already
    // explicitly set on the SpecFrame. Without this gate we trigger a
    // FrameSet integrity remap on every SpecFrame+SkyFrame fixture —
    // which is fine when SpecFrame::map_from collapses to identity
    // for a no-op RefRA change, but produces a non-trivial SpecMap
    // once MakeSpecMapping mirrors the C non-trivial step3+step6
    // behaviour (cf. specframe.c:2750-2754 + 2823+2847).
    let has_ref_radec = peek_spec(owned.current())
        .map(|s| s.ref_ra().is_some() && s.ref_dec().is_some())
        .unwrap_or(false);
    if !has_ref_radec {
        return Ok(owned);
    }

    // Save the SkyFrame's System / Equinox is-set state plus the
    // current values so we can restore them after computing the
    // FK5/J2000 fiducial. Mirrors C:21446-21459 — C uses `astTest`
    // and only re-sets the attribute if it was originally set.
    // We must not silently "restore" an unset attribute by calling
    // set_*: that would mark it as explicitly set with the default
    // value, which then trips downstream attribute-test gates (e.g.
    // `astTestEpoch` in SkySys at fitschan.c:29721, which controls
    // whether MJD-OBS is emitted).
    //
    // Epoch is intentionally NOT touched here — C's
    // MakeFitsFrameSet (fitschan.c:21441-21505) only saves and
    // restores System and Equinox on the celestial axes; it never
    // reaches Epoch.
    let (orig_sys_set, orig_sys, orig_eqn_set, orig_equinox) = {
        let cur = owned.current();
        let Some(sky) = peek_sky(cur) else {
            return Ok(owned);
        };
        (
            sky.test_system(),
            sky.system(),
            sky.test_equinox(),
            sky.equinox_mjd(),
        )
    };

    // Step 1: switch SkyFrame to FK5/J2000 via FrameSet integrity.
    // The CmpFrame::map_from delta picks up the SkyFrame change and
    // re-fuses the pixel→world mapping so the celestial axes now
    // produce FK5 J2000 RA/Dec. Mirrors C:21452-21459 (which sets
    // System and Equinox via astSetC on the FrameSet — Epoch is
    // intentionally not touched).
    owned.with_integrity(|cur| {
        if let Some(sky) = sky_mut(cur) {
            sky.set_system(SkySystem::Fk5);
            sky.set_equinox_year(2000.0);
        }
        Ok(())
    })?;

    // Step 2: compute the celestial fiducial via SplitMap +
    // GetFiducialWCS. Mirrors C:21468-21474. SplitMap finds the
    // (map1, WcsMap, map3) triple within the pixel→WCS mapping;
    // GetFiducialWCS feeds the WcsMap's native fiducial through
    // map3 to land in the celestial Frame.
    // Helper: restore SkyFrame system/equinox to their pre-step-1
    // is-set state. Mirrors C:21503-21505 — only re-set attributes
    // that were originally set; clear those that weren't, so the
    // FK5/J2000 override doesn't leak as an explicit value.
    let restore_sky = |cur: &mut dyn crate::frames::Frame| -> AstResult<()> {
        if let Some(sky) = sky_mut(cur) {
            if orig_sys_set {
                sky.set_system(orig_sys);
            } else {
                sky.set_system(orig_sys);
                sky.clear_system();
            }
            if orig_eqn_set {
                sky.set_equinox_mjd(orig_equinox);
            } else {
                sky.clear_equinox();
            }
        }
        Ok(())
    };

    let mapping = owned.mapping(FrameRef::Base, FrameRef::Current)?;
    let Some((_map1, map2, map3)) = celestial_axes::split_map2(mapping.as_ref()) else {
        // No WcsMap found — celestial axes have no reference point;
        // C falls through and returns the FrameSet as-is. Restore
        // the SkyFrame's original system/equinox first so we don't
        // leak the FK5/J2000 override.
        owned.with_integrity(restore_sky)?;
        return Ok(owned);
    };

    let Some(wcsmap) = map2.as_any().downcast_ref::<WcsMap>() else {
        owned.with_integrity(restore_sky)?;
        return Ok(owned);
    };

    let fid = fiducial::get_fiducial_wcs(wcsmap, map3.as_ref(), ilon, ilat)?;
    let Some((reflon, reflat)) = fid else {
        owned.with_integrity(restore_sky)?;
        return Ok(owned);
    };

    // Step 3: set RefRA/RefDec on the SpecFrame via FrameSet
    // integrity. The SpecFrame::map_from delta for RefRA/RefDec is
    // generally identity (these only affect rest-frame conversions),
    // but going through FrameSet integrity matches C's flow and
    // ensures any future SpecFrame integrity logic kicks in. Mirrors
    // C:21483-21484.
    //
    // C uses `astSetC(tfs, "RefRA", astFormat(tfs, ilon, reflon))` —
    // i.e. it formats the radian value through the SkyFrame's
    // sexagesimal HMS/DMS formatter at the SkyFrame's default
    // digit precision and then re-parses the result, lossily
    // quantising the value to ~0.1 s of time / ~1 arcsec. We mirror
    // this quirk so the resulting VELOSYS computation matches C's
    // bit-exact output (see `docs/issues/c-library-quirks.md` —
    // "VELOSYS drift via lossy `RefRA`/`RefDec` round-trip").
    //
    // After the format/reparse, also normalise to AST's canonical
    // ranges: RA in `[0, 2π)` (via `eraAnp`), Dec in `(-π, π]` (via
    // `eraAnpm`). Without this, the SlaMap output's `(-π, π]` RA
    // representation gives a different f64 from C's `[0, 2π)` one,
    // which after the f32-cast inside SpecMap's LSRK fcorr produces
    // a velocity drift of ~30 mm/s for sources near RA≈0/2π.
    use crate::frames::skyaxis::{normalise_dec, normalise_ra};
    let (reflon_quant, reflat_quant) = {
        let cur = owned.current();
        let sky = peek_sky(cur).expect("SkyFrame still present after switch to FK5/J2000");
        // Format/unformat by the SkyFrame's lon/lat axis indices, not
        // by the literal 1/2 FITS positions. For a perm_axes'd
        // lat-first SkyFrame (e.g. CTYPE1=DEC, CTYPE2=RA), external
        // axis 0 is lat and external axis 1 is lon. Format() picks
        // the as_time flag from the *physical* axis behind whichever
        // external slot is asked for, so without honouring the perm
        // a lon value gets DMS-formatted (or a lat value HMS), which
        // quantises to ~5e-6 rad of error — propagating downstream to
        // a wrong VELOSYS via SpecMap's RefRA/RefDec dependence.
        let lon_axis_1b = sky.lon_axis() + 1;
        let lat_axis_1b = sky.lat_axis() + 1;
        let lon_str = sky.format(lon_axis_1b, reflon).unwrap_or_default();
        let lat_str = sky.format(lat_axis_1b, reflat).unwrap_or_default();
        let lon_round = sky
            .unformat(lon_axis_1b, &lon_str)
            .map(|(v, _)| v)
            .unwrap_or(reflon);
        let lat_round = sky
            .unformat(lat_axis_1b, &lat_str)
            .map(|(v, _)| v)
            .unwrap_or(reflat);
        (normalise_ra(lon_round), normalise_dec(lat_round))
    };

    owned.with_integrity(|cur| {
        if let Some(spec) = spec_mut(cur) {
            spec.set_ref_ra(reflon_quant);
            spec.set_ref_dec(reflat_quant);
        }
        Ok(())
    })?;

    // Step 4: restore SkyFrame's original system/equinox is-set
    // state. Mirrors C:21503-21505.
    owned.with_integrity(restore_sky)?;

    // Step 5 (fitschan.c:21517 → AddEncodingFrame at fitschan.c:2435):
    // per-encoding postprocessing. For FITS-CLASS this swaps the
    // current Frame for a CmpFrame(SpecFrame{Freq,Source,Hz},
    // SkyFrame{FK5/FK4/Galactic}) so the downstream FitsStore
    // extraction sees Source-rest-frame Hz CRVALs.
    if matches!(encoding, Encoding::FitsClass) {
        add_encoding_frame_class(&mut owned)?;
    }

    Ok(owned)
}

/// Mirrors C `AddEncodingFrame` (fitschan.c:2359) for the FITS-CLASS
/// branch (lines 2433-2513).
///
/// When the FrameSet's current Frame contains both a SpecFrame and a
/// SkyFrame component, build a new `CmpFrame(SpecFrame{Freq,Source,
/// Hz}, SkyFrame{FK5/FK4/Galactic})` and add it as the new current
/// Frame.
///
/// The C path uses `astConvert(curfrm, cmpfrm)` to compute the
/// CmpFrame→CmpFrame conversion Mapping; that walks each component
/// pair and composes the per-component conversions in parallel. Our
/// CmpFrame::convert today returns a UnitMap for same-axes targets,
/// which would defeat the purpose. Instead we go a step lower:
/// compute the per-component Mappings (SpecFrame→SpecFrame using the
/// existing `SpecFrame::convert`, SkyFrame→SkyFrame using
/// `SkyFrame::convert`) and parallel-compose them via CmpMap. The
/// resulting Mapping shifts the CRVAL through the SpecMap chain that
/// converts Barycentric Hz to Source Hz.
fn add_encoding_frame_class(fs: &mut FrameSet) -> AstResult<()> {
    use crate::frames::{
        CmpFrame, DSBSpecFrame, SkyFrame, SkySystem, SpecFrame, SpecSystem, StdOfRest,
    };

    // Locate SpecFrame and SkyFrame components in the current Frame
    // by walking primary frames and copying the first match of each
    // (mirrors C:2440-2448 — `astPrimaryFrame` + `astCopy(pfrm)`).
    let cfrm_naxes = fs.current().naxes();
    let mut matched_spec: Option<SpecFrame> = None;
    let mut matched_sky: Option<SkyFrame> = None;
    for iax in 0..cfrm_naxes {
        let (pframe, _) = crate::frames::specframe::primary_frame_walk(fs.current(), iax);
        if matched_spec.is_none() {
            if let Some(spec) = pframe.as_any().downcast_ref::<SpecFrame>() {
                matched_spec = Some(spec.clone());
            } else if let Some(dsb) = pframe.as_any().downcast_ref::<DSBSpecFrame>() {
                // DSBSpecFrame inherits from SpecFrame; downgrade to its
                // SpecFrame component for the encoding target. The
                // sideband-specific attributes are not used by CLASS.
                matched_spec = Some(dsb.spec().clone());
            }
        }
        if matched_sky.is_none()
            && let Some(sky) = pframe.as_any().downcast_ref::<SkyFrame>()
        {
            matched_sky = Some(sky.clone());
        }
    }

    // Cannot proceed without both components. Mirrors C:2451.
    let (Some(matched_spec), Some(matched_sky)) = (matched_spec, matched_sky) else {
        return Ok(());
    };

    // Build the Source-Hz target SpecFrame: a copy of the matched
    // SpecFrame with System=Freq, StdOfRest=Source, Unit=Hz forced
    // (C:2456-2463).
    let mut source_spec = matched_spec.clone();
    source_spec.set_system(SpecSystem::Freq);
    source_spec.set_std_of_rest(StdOfRest::Source);
    source_spec.set_unit("Hz");

    // Mapping that converts the matched SpecFrame's coordinates to
    // the Source-Hz frame.
    let Some(spec_conv_fs) = matched_spec.convert(&source_spec, &[])? else {
        return Ok(());
    };
    let spec_mapping = spec_conv_fs.mapping(FrameRef::Base, FrameRef::Current)?;

    // Build a CLASS-acceptable target SkyFrame (C:2466-2477):
    // System ∈ {FK4, FK5, Galactic}; FK5 ⇒ J2000.0; FK4 ⇒ B1950.0.
    let mut target_sky = matched_sky.clone();
    let sys = target_sky.system();
    let class_sys = match sys {
        SkySystem::Fk4 | SkySystem::Fk5 | SkySystem::Galactic => sys,
        _ => SkySystem::Fk5,
    };
    if class_sys != sys {
        target_sky.set_system(class_sys);
    }
    match class_sys {
        SkySystem::Fk5 => target_sky.set_equinox_mjd(51544.5), // J2000.0
        SkySystem::Fk4 => target_sky.set_equinox_mjd(33281.92345905), // B1950.0
        _ => {}
    }

    let Some(sky_conv_fs) = matched_sky.convert(&target_sky, &[])? else {
        return Ok(());
    };
    let sky_mapping = sky_conv_fs.mapping(FrameRef::Base, FrameRef::Current)?;

    // Build the new CmpFrame(source_spec, target_sky) and the
    // parallel CmpMap composing the two per-component conversions.
    // The original CmpFrame's axis order is (spec, sky) when the
    // SpecFrame is the first component. For now, only handle that
    // common case by checking the original current Frame's component
    // order.
    let cur = fs.current();
    let cur_cmp = cur.as_any().downcast_ref::<CmpFrame>();
    let Some(cur_cmp) = cur_cmp else {
        // Non-CmpFrame current — skip; AddEncodingFrame is only
        // meaningful for the cube case.
        return Ok(());
    };
    let frame1_is_spec = cur_cmp
        .frame1()
        .as_any()
        .downcast_ref::<SpecFrame>()
        .is_some()
        || cur_cmp
            .frame1()
            .as_any()
            .downcast_ref::<DSBSpecFrame>()
            .is_some();
    let frame1_is_sky = cur_cmp
        .frame1()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .is_some();

    if !(frame1_is_spec || frame1_is_sky) {
        // Components are not in (spec, sky) or (sky, spec) layout —
        // bail out.
        return Ok(());
    }

    use crate::mappings::{CmpMap, Composition};
    let (new_cmp, parallel_map): (crate::frames::DynFrame, crate::mappings::DynMapping) =
        if frame1_is_spec {
            (
                Box::new(CmpFrame::new(Box::new(source_spec), Box::new(target_sky))),
                Box::new(CmpMap::new(
                    spec_mapping,
                    sky_mapping,
                    Composition::Parallel,
                )?),
            )
        } else {
            (
                Box::new(CmpFrame::new(Box::new(target_sky), Box::new(source_spec))),
                Box::new(CmpMap::new(
                    sky_mapping,
                    spec_mapping,
                    Composition::Parallel,
                )?),
            )
        };

    // Add the new Frame as the new current. Subsequent extraction
    // (CelestialAxes/SpectralAxes) operates on this new Frame.
    fs.add_frame(FrameRef::Current, parallel_map, new_cmp)?;

    Ok(())
}

fn peek_spec(frame: &dyn crate::frames::Frame) -> Option<&crate::frames::SpecFrame> {
    use crate::frames::{CmpFrame, DSBSpecFrame, SpecFrame};
    if let Some(s) = frame.as_any().downcast_ref::<SpecFrame>() {
        return Some(s);
    }
    if let Some(d) = frame.as_any().downcast_ref::<DSBSpecFrame>() {
        return Some(d.spec());
    }
    if let Some(cmp) = frame.as_any().downcast_ref::<CmpFrame>() {
        if let Some(s) = peek_spec(cmp.frame1()) {
            return Some(s);
        }
        return peek_spec(cmp.frame2());
    }
    None
}

fn peek_sky(frame: &dyn crate::frames::Frame) -> Option<&crate::frames::SkyFrame> {
    use crate::frames::{CmpFrame, SkyFrame};
    if let Some(s) = frame.as_any().downcast_ref::<SkyFrame>() {
        return Some(s);
    }
    if let Some(cmp) = frame.as_any().downcast_ref::<CmpFrame>() {
        if let Some(s) = peek_sky(cmp.frame1()) {
            return Some(s);
        }
        return peek_sky(cmp.frame2());
    }
    None
}

fn spec_mut(frame: &mut dyn crate::frames::Frame) -> Option<&mut crate::frames::SpecFrame> {
    use crate::frames::{CmpFrame, DSBSpecFrame, SpecFrame};
    if frame.as_any().downcast_ref::<SpecFrame>().is_some() {
        return frame.as_any_mut().downcast_mut::<SpecFrame>();
    }
    if frame.as_any().downcast_ref::<DSBSpecFrame>().is_some() {
        return frame
            .as_any_mut()
            .downcast_mut::<DSBSpecFrame>()
            .map(|d| d.spec_mut());
    }
    if let Some(cmp) = frame.as_any_mut().downcast_mut::<CmpFrame>() {
        let pick_first = peek_spec(cmp.frame1()).is_some();
        if pick_first {
            return spec_mut(cmp.frame1_mut());
        }
        return spec_mut(cmp.frame2_mut());
    }
    None
}

fn sky_mut(frame: &mut dyn crate::frames::Frame) -> Option<&mut crate::frames::SkyFrame> {
    use crate::frames::{CmpFrame, SkyFrame};
    if frame.as_any().downcast_ref::<SkyFrame>().is_some() {
        return frame.as_any_mut().downcast_mut::<SkyFrame>();
    }
    if let Some(cmp) = frame.as_any_mut().downcast_mut::<CmpFrame>() {
        let pick_first = peek_sky(cmp.frame1()).is_some();
        if pick_first {
            return sky_mut(cmp.frame1_mut());
        }
        return sky_mut(cmp.frame2_mut());
    }
    None
}

/// For each WCS axis: if `|CRVAL| < sqrt(DBL_EPSILON · cdelt²)`, set
/// CRVAL = 0. The squared-pixel-scale `cdelt2` multiplies the squared
/// CDELT by the squared L2 norm of the PC row. Mirrors fitschan.c:2920-2942.
fn snap_small_crval(store: &mut FitsStore, wperm: &[usize], nwcs: usize) {
    for iax in 0..nwcs {
        let fits_i = wperm[iax];
        let Some(crval) = store.crval.get(fits_i).copied().flatten() else {
            continue;
        };
        // L2 norm² of the PC row (defaulted to identity).
        let mut row_norm2 = 0.0_f64;
        for fits_j in 0..nwcs {
            let pc = store
                .pc
                .get(fits_i)
                .and_then(|row| row.get(fits_j))
                .copied()
                .flatten()
                .unwrap_or(if fits_i == fits_j { 1.0 } else { 0.0 });
            row_norm2 += pc * pc;
        }
        let cdelt = store.cdelt.get(fits_i).copied().flatten().unwrap_or(1.0);
        let cdelt2 = row_norm2 * cdelt * cdelt;
        if crval.abs() < (f64::EPSILON * cdelt2).sqrt() {
            store.crval[fits_i] = Some(0.0);
        }
    }
}

/// Find longitude / latitude / spectral axis indices in a FitsStore by
/// inspecting CTYPE prefixes. Mirrors `fitschan.c::FindLonLatSpecAxes`
/// (line 10571). Returns `(axlon, axlat, axspec)`, each `None` when no
/// matching axis is found.
fn find_lon_lat_spec_axes(store: &FitsStore) -> (Option<usize>, Option<usize>, Option<usize>) {
    let mut axlon: Option<usize> = None;
    let mut axlat: Option<usize> = None;
    let mut axspec: Option<usize> = None;
    let wcsaxes = store.wcsaxes.unwrap_or(store.naxis);
    let nax = wcsaxes.min(store.ctype.len());
    for i in 0..nax {
        let ctype = match store.ctype[i].as_deref() {
            Some(s) => s,
            None => continue,
        };
        // Spectral axes: handled by the spectral_axes writer; we
        // recognise "FREQ", "VRAD" etc. via a 4-char check on common
        // forms, plus AIPS-style "FELO-LSR" / "VELO-OBS". A simpler
        // pragmatic check matches the FITS-WCS Paper III spectral
        // CTYPE prefixes used by AST.
        let ct4: &str = if ctype.len() >= 4 { &ctype[..4] } else { ctype };
        if matches!(
            ct4,
            "FREQ" | "ENER" | "WAVN" | "VRAD" | "WAVE" | "VOPT" | "ZOPT" | "AWAV" | "VELO" | "BETA"
        ) {
            axspec = Some(i);
            continue;
        }
        // AIPS-style spectral codes "FELO-...", "VELO-..." both already
        // match the VELO branch above; explicit "FELO" prefix:
        if ctype.starts_with("FELO") {
            axspec = Some(i);
            continue;
        }
        // Celestial axes: 5th char must be '-' (e.g. "RA---TAN",
        // "GLON-TAN", "DEC--TAN"). C: ctype[4] == '-'.
        if ctype.len() <= 4 || ctype.as_bytes()[4] != b'-' {
            continue;
        }
        let b = ctype.as_bytes();
        // Longitude: "RA--", "AZ--", "xLON", "xxLN".
        let is_lon = (&b[..4] == b"RA--")
            || (&b[..4] == b"AZ--")
            || (b.len() >= 4 && &b[1..4] == b"LON")
            || (b.len() >= 4 && &b[2..4] == b"LN");
        if is_lon {
            axlon = Some(i);
            continue;
        }
        // Latitude: "DEC-", "EL--", "xLAT", "xxLT".
        let is_lat = (&b[..4] == b"DEC-")
            || (&b[..4] == b"EL--")
            || (b.len() >= 4 && &b[1..4] == b"LAT")
            || (b.len() >= 4 && &b[2..4] == b"LT");
        if is_lat {
            axlat = Some(i);
        }
    }
    (axlon, axlat, axspec)
}

/// FITS-CLASS spatial-CRVAL adjustment for degenerate spatial axes.
///
/// Mirrors `fitschan.c::CLASSFromStore` lines 6465-6496: when both
/// spatial pixel axes span exactly one pixel (`dim[axlon] == 1.0 &&
/// dim[axlat] == 1.0`), re-evaluate the WCS at pixel `(1, 1, 1)` and
/// update `store.crval[axlon]`, `store.crval[axlat]`, and the two
/// CRPIX values to put the reference at the centre of the one and
/// only spatial pixel.
fn class_adjust_spatial_crval(
    fs: &FrameSet,
    store: &mut FitsStore,
    dim: &[Option<f64>],
    wperm: &[usize],
) -> AstResult<()> {
    use crate::coords::PointSet;

    let (axlon, axlat, _axspec) = find_lon_lat_spec_axes(store);
    let (Some(axlon), Some(axlat)) = (axlon, axlat) else {
        return Ok(());
    };

    if store.naxis < 3 {
        return Ok(());
    }
    let lon_is_degen = matches!(dim.get(axlon).copied().flatten(), Some(d) if d == 1.0);
    let lat_is_degen = matches!(dim.get(axlat).copied().flatten(), Some(d) if d == 1.0);
    if !(lon_is_degen && lat_is_degen) {
        return Ok(());
    }

    // Transform pixel (1, 1, 1) through pixel→WCS. The resulting xout
    // is indexed by WCS axis (output of the FrameSet mapping), not by
    // FITS axis. C uses `astHasAttribute(curfrm, "IsLonAxis(i+1)")`
    // to find which output axis is lon vs lat; we already have wperm,
    // which maps WCS axis → FITS axis, so the WCS axis whose
    // wperm[wcs_iax] == axlon is the longitude output.
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let nin = mapping.nin();
    let nout = mapping.nout();
    if nin < 3 || nout < 3 {
        return Ok(());
    }
    let pts = PointSet::from_rows([vec![1.0; nin]]);
    let out = mapping.transform(&pts, true)?;
    let xout = out.point(0);

    let mut lon_world_idx: Option<usize> = None;
    let mut lat_world_idx: Option<usize> = None;
    for (wi, &fi) in wperm.iter().enumerate() {
        if fi == axlon {
            lon_world_idx = Some(wi);
        } else if fi == axlat {
            lat_world_idx = Some(wi);
        }
    }
    let (Some(lon_world_idx), Some(lat_world_idx)) = (lon_world_idx, lat_world_idx) else {
        return Ok(());
    };

    let lonval_rad = xout[lon_world_idx];
    let latval_rad = xout[lat_world_idx];
    if !lonval_rad.is_finite() || !latval_rad.is_finite() {
        return Ok(());
    }
    const D2R: f64 = std::f64::consts::PI / 180.0;
    store.crval[axlon] = Some(lonval_rad / D2R);
    store.crval[axlat] = Some(latval_rad / D2R);
    store.crpix[axlon] = Some(1.0);
    store.crpix[axlat] = Some(1.0);
    Ok(())
}

/// Round each CRPIX value to the nearest microsecond-of-a-pixel.
/// Mirrors fitschan.c:2945-2954.
fn round_crpix(store: &mut FitsStore) {
    for slot in store.crpix.iter_mut() {
        if let Some(v) = slot {
            *slot = Some((*v * 1.0e6 + 0.5).floor() * 1.0e-6);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{BasicFrameBuilder, FrameSet};
    use crate::mappings::{ShiftMap, UnitMap};

    /// Smoke test: a 2-axis BasicFrame → BasicFrame chain with no
    /// celestial / spectral axes, using a per-axis ShiftMap. Expect
    /// fset_to_store to return a populated FitsStore where
    /// other_axes covers both axes and make_int_world recovers the
    /// shift-only CRPIX/CD.
    #[test]
    fn basic_two_axis_shift() {
        let pixel = BasicFrameBuilder::new(2)
            .domain("GRID")
            .label(1, "x")
            .label(2, "y")
            .build()
            .unwrap();
        let world = BasicFrameBuilder::new(2)
            .domain("WORLD")
            .label(1, "wx")
            .label(2, "wy")
            .unit(1, "m")
            .unit(2, "m")
            .build()
            .unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(
            FrameRef::Base,
            Box::new(ShiftMap::new(vec![10.0, 20.0])),
            Box::new(world),
        )
        .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let store = fset_to_store(&fs, &dim, Encoding::FitsWcs, ' ', &[]).unwrap();

        // Both axes should have CTYPE / CUNIT populated by other_axes.
        assert!(store.ctype[0].is_some(), "CTYPE1 should be set");
        assert!(store.ctype[1].is_some(), "CTYPE2 should be set");
        // CRPIX populated.
        assert!(store.crpix[0].is_some(), "CRPIX1 should be set");
        assert!(store.crpix[1].is_some(), "CRPIX2 should be set");
        // WCSNAME from the WORLD domain.
        assert_eq!(store.wcsname.as_deref(), Some("WORLD"));
    }

    /// Identity mapping → CRPIX lands at the central pixel
    /// `floor(0.5*(1+dim))`. With dim=100 that's 50; CRVAL is then
    /// world(50) = 50, IWC at g0 is 0, and the CRPIX solve recovers
    /// pixel coord = 50 (where IWC = 0 under the identity).
    #[test]
    fn identity_chain_yields_central_pixel_crpix() {
        let pixel = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
        let world = BasicFrameBuilder::new(2)
            .domain("WORLD")
            .label(1, "wx")
            .label(2, "wy")
            .build()
            .unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(world))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let store = fset_to_store(&fs, &dim, Encoding::FitsWcs, ' ', &[]).unwrap();
        let crpix0 = store.crpix[0].unwrap();
        let crpix1 = store.crpix[1].unwrap();
        // dim=100 ⇒ g0 = floor(0.5*(1+100)) = 50.
        assert!((crpix0 - 50.0).abs() < 1e-6, "CRPIX1 = {crpix0}");
        assert!((crpix1 - 50.0).abs() < 1e-6, "CRPIX2 = {crpix1}");
    }
}

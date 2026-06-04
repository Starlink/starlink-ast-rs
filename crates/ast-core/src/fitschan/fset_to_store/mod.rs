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
    let mut owned_fs = make_fits_frameset(fs)?;
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
    let cel_seg = celestial_axes(CelestialAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        s,
        store: &mut store,
        axis_done: &mut axis_done,
        isoff: 0,
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
fn make_fits_frameset(fs: &FrameSet) -> AstResult<FrameSet> {
    use crate::fitschan::fset_to_store::celestial_axes;
    use crate::fitschan::fset_to_store::spectral_axes::primary_frame;
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
        let (pframe, paxis) = primary_frame(owned.current(), iax);
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

    // Save the SkyFrame's current System / Equinox / Epoch so we can
    // restore them after computing the FK5/J2000 fiducial. Mirrors
    // C:21446-21459.
    let (orig_sys, orig_equinox, orig_epoch) = {
        let cur = owned.current();
        let Some(sky) = peek_sky(cur) else {
            return Ok(owned);
        };
        (sky.system(), sky.equinox_mjd(), sky.epoch_mjd())
    };

    // Step 1: switch SkyFrame to FK5/J2000 via FrameSet integrity.
    // The CmpFrame::map_from delta picks up the SkyFrame change and
    // re-fuses the pixel→world mapping so the celestial axes now
    // produce FK5 J2000 RA/Dec. Mirrors C:21452-21459 (which sets
    // System and Equinox via astSetC on the FrameSet).
    owned.with_integrity(|cur| {
        if let Some(sky) = sky_mut(cur) {
            sky.set_system(SkySystem::Fk5);
            sky.set_equinox_year(2000.0);
            sky.set_epoch_year(2000.0);
        }
        Ok(())
    })?;

    // Step 2: compute the celestial fiducial via SplitMap +
    // GetFiducialWCS. Mirrors C:21468-21474. SplitMap finds the
    // (map1, WcsMap, map3) triple within the pixel→WCS mapping;
    // GetFiducialWCS feeds the WcsMap's native fiducial through
    // map3 to land in the celestial Frame.
    let mapping = owned.mapping(FrameRef::Base, FrameRef::Current)?;
    let Some((_map1, map2, map3)) = celestial_axes::split_map2(mapping.as_ref()) else {
        // No WcsMap found — celestial axes have no reference point;
        // C falls through and returns the FrameSet as-is. Restore
        // the SkyFrame's original system/equinox first so we don't
        // leak the FK5/J2000 override.
        owned.with_integrity(|cur| {
            if let Some(sky) = sky_mut(cur) {
                sky.set_system(orig_sys);
                sky.set_equinox_mjd(orig_equinox);
                sky.set_epoch_mjd(orig_epoch);
            }
            Ok(())
        })?;
        return Ok(owned);
    };

    let Some(wcsmap) = map2.as_any().downcast_ref::<WcsMap>() else {
        owned.with_integrity(|cur| {
            if let Some(sky) = sky_mut(cur) {
                sky.set_system(orig_sys);
                sky.set_equinox_mjd(orig_equinox);
                sky.set_epoch_mjd(orig_epoch);
            }
            Ok(())
        })?;
        return Ok(owned);
    };

    let fid = fiducial::get_fiducial_wcs(wcsmap, map3.as_ref(), ilon, ilat)?;
    let Some((reflon, reflat)) = fid else {
        owned.with_integrity(|cur| {
            if let Some(sky) = sky_mut(cur) {
                sky.set_system(orig_sys);
                sky.set_equinox_mjd(orig_equinox);
                sky.set_epoch_mjd(orig_epoch);
            }
            Ok(())
        })?;
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
    // SkyFrame's `format` / `unformat` index its own two axes (1 =
    // longitude, 2 = latitude in the 1-based external API). Use those
    // local indices regardless of where the celestial pair sits in
    // the surrounding pipeline.
    let (reflon_quant, reflat_quant) = {
        let cur = owned.current();
        let sky = peek_sky(cur).expect("SkyFrame still present after switch to FK5/J2000");
        let lon_str = sky.format(1, reflon).unwrap_or_default();
        let lat_str = sky.format(2, reflat).unwrap_or_default();
        let lon_round = sky.unformat(1, &lon_str).map(|(v, _)| v).unwrap_or(reflon);
        let lat_round = sky.unformat(2, &lat_str).map(|(v, _)| v).unwrap_or(reflat);
        (lon_round, lat_round)
    };

    owned.with_integrity(|cur| {
        if let Some(spec) = spec_mut(cur) {
            spec.set_ref_ra(reflon_quant);
            spec.set_ref_dec(reflat_quant);
        }
        Ok(())
    })?;

    // Step 4: restore SkyFrame's original system/equinox/epoch.
    // Mirrors C:21503-21505.
    owned.with_integrity(|cur| {
        if let Some(sky) = sky_mut(cur) {
            sky.set_system(orig_sys);
            sky.set_equinox_mjd(orig_equinox);
            sky.set_epoch_mjd(orig_epoch);
        }
        Ok(())
    })?;

    Ok(owned)
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

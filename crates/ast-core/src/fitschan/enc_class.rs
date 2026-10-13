//! FITS-CLASS encoding reader/writer.
//!
//! CLASS is the data format used by the GILDAS/CLASS radio spectral survey
//! software from Grenoble. Key features of this encoding:
//!
//! - Enforces 3 axes: 2 spatial + 1 spectral (FREQ)
//! - Spectral CRVAL is stored as an **offset** from RESTFREQ, not an absolute
//!   frequency: `absolute_freq = CRVAL + RESTFREQ`
//! - Requires DELTAV (velocity channel spacing in m/s) and VELO-LSR/VLSR
//! - Older projection codes: GLS (= SFL), ATF (= AIT)
//! - SPECSYS is typically "SOURCE" (source rest frame)
//!
//! This module provides:
//! - [`write_to_cards`]: write a FitsStore as CLASS keywords into a CardList

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::AstResult;
use crate::frames::{
    Frame, FrameRef, FrameSet, SkyFrame, SkySystem, SpecFrame, SpecSystem, StdOfRest,
};

/// Whether `store` can be written with FITS-CLASS: the checks C's
/// `CLASSFromStore` (fitschan.c:6612-6884) makes before writing, and the
/// CRPIX, CRVAL and CTYPE values it needs while writing. The spectral and
/// celestial axes must be the first three; the projection TAN, ARC, STG,
/// AIT or SFL (AIT and SFL at the origin), or SIN with no parameters; the
/// longitude RA or galactic; the spectral axis FREQ in the SOURCE frame,
/// in Hz, kHz, MHz or GHz; no rotation unless both spatial axes span one
/// pixel; FK4 or FK5 with an equinox the IAU 1984 rule allows; a rest
/// frequency; and, unless both spatial axes span one pixel, no LATPOLE
/// and no LONPOLE other than +180 or -180 degrees.
fn class_store_ok(store: &FitsStore, cards: &CardList) -> bool {
    use super::fits_store::names_start_of;
    use super::wcs_celestial::wcs_prj_type;
    use crate::wcslib::WcsProjection as P;

    let (axlon, axlat, axspec) = store.lon_lat_spec_axes();
    let (Some(axlon), Some(axlat), Some(axspec)) = (axlon, axlat, axspec) else {
        return false;
    };
    if axlon >= 3 || axlat >= 3 || axspec >= 3 {
        return false;
    }
    // C also clears `ok` for a missing CRVAL or longitude CTYPE, but the
    // test of the projection that follows sets it again whatever it was.
    let latval = store.crval[axlat];
    let lonval = store.crval[axlon];
    let lontype = store.ctype[axlon].as_deref();
    let prj = store.ctype[axlat]
        .as_deref()
        .and_then(|c| c.get(4..))
        .and_then(wcs_prj_type);
    let mut ok;
    match prj {
        None => ok = false,
        Some(P::Sin) => {
            let pv = |m: usize| {
                store
                    .pv
                    .get(axlat)
                    .and_then(|a| a.get(m))
                    .copied()
                    .flatten()
                    .unwrap_or(0.0)
            };
            ok = pv(1) == 0.0 && pv(2) == 0.0;
        }
        Some(p) => {
            ok = matches!(p, P::Tan | P::Arc | P::Stg | P::Ait | P::Sfl)
                && (!matches!(p, P::Ait | P::Sfl) || (latval == Some(0.0) && lonval == Some(0.0)));
        }
    }
    if ok && !lontype.is_some_and(|t| t.starts_with("RA--") || t.starts_with("GLON")) {
        ok = false;
    }
    match store.ctype[axspec].as_deref() {
        Some(c) if names_start_of(c, "FREQ") => {}
        _ => ok = false,
    }
    match store.specsys.as_deref() {
        None => ok = false,
        Some(c) if ok && !names_start_of(c, "SOURCE") => ok = false,
        Some(_) => {}
    }
    match store.cunit[axspec].as_deref() {
        None => ok = false,
        Some(c) if ok && !["Hz", "kHz", "MHz", "GHz"].contains(&c) => ok = false,
        Some(_) => {}
    }

    // Only the first three WCS axes are written.
    let naxis = store.max_crpix_axis().map_or(0, |m| m + 1).min(3);
    let naxis2 = cards.get_int(&format!("NAXIS{}", axlon + 1)).unwrap_or(0);
    let naxis3 = cards.get_int(&format!("NAXIS{}", axlat + 1)).unwrap_or(0);
    let degenerate = naxis2 == 1 && naxis3 == 1;
    if ok && !degenerate {
        'rows: for i in 0..naxis {
            for j in 0..naxis {
                if i != j && store.cd_element(i, j) != 0.0 {
                    ok = false;
                    break 'rows;
                }
            }
        }
    }
    if let Some(sys) = store.radesys.as_deref() {
        match (sys, store.equinox) {
            ("FK4", Some(e)) if e >= 1984.0 => ok = false,
            ("FK5", Some(e)) if e < 1984.0 => ok = false,
            ("FK4" | "FK5", _) => {}
            _ => ok = false,
        }
    }
    if store.restfrq.is_none() {
        ok = false;
    }
    if ok
        && !degenerate
        && (store.lonpole.is_some_and(|l| l != -180.0 && l != 180.0) || store.latpole.is_some())
    {
        ok = false;
    }
    ok && (0..naxis).all(|j| store.crpix[j].is_some())
        && (0..naxis).all(|i| store.crval[i].is_some())
        && (0..naxis).all(|i| store.ctype[i].is_some())
}

/// Write a [`FitsStore`] as CLASS-style WCS keywords into a [`CardList`].
///
/// Key behaviours:
/// - Writes NAXIS
/// - Translates standard projection codes SFL and AIT back to GLS and ATF
/// - Subtracts RESTFREQ from the spectral axis CRVAL (absolute → offset)
/// - Computes and writes DELTAV and VELO-LSR from the spectral axis parameters
/// - Writes EQUINOX and DATE-OBS, but not MJD-OBS, SPECSYS or RADESYS
///
/// `s` is the coordinate-version letter; `is_last` gates the
/// observation-global keyword emission. See `enc_pc::write_to_cards`
/// for the contract.
pub(crate) fn write_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<bool> {
    use super::wcs_keywords::key_for;
    if !class_store_ok(store, cards) {
        return Ok(false);
    }
    let naxis = store.naxis;

    // FITS-CLASS does not emit a plain NAXIS keyword. C
    // CLASSFromStore (fitschan.c:6772-6787) only sets NAXIS when
    // the spatial axes are degenerate (store->naxis == 1) and the
    // existing keyword needs to be force-3 with NAXIS2/NAXIS3 = 1.
    // Otherwise NAXIS is left to whatever the caller already had in
    // the CardList. The test fixtures provide explicit NAXIS1..3
    // entries via the input header, which carry through unchanged.
    let _ = is_last;

    // Find spectral axis and rest frequency for later use.
    let spec_idx = store.spectral_axis();
    let restfreq = store.restfrq;

    // Mirror the keyword order of fitschan.c CLASSFromStore
    // (lines 6726-6944): CRPIX → CRVAL → CTYPE → CDELT → EQUINOX
    // → DATE-OBS → RESTFREQ → IMAGFREQ → OBJECT/LINE → velocities
    // → AZIMUTH/ELEVATIO. MJD-OBS, NAXIS, SPECSYS, RADESYS, CUNIT
    // are deliberately NOT emitted (C never SetValue's them on the
    // CLASS path; they carry through from the input header
    // unchanged or stay absent).

    // CRPIXi (C:6730-6739)
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = key_for("CRPIX", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Reference pixel on axis {}", i + 1));
        }
    }

    // CRVALi (C:6744-6757) — subtract RESTFREQ from spectral axis
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let output_val = if Some(i) == spec_idx {
                if let Some(rf) = restfreq {
                    val - rf
                } else {
                    val
                }
            } else {
                val
            };
            let key = key_for("CRVAL", Some(i + 1), s);
            cards.set_float(
                &key,
                output_val,
                &format!("Value at ref. pixel on axis {}", i + 1),
            );
        }
    }

    // CTYPEi (C:6763-6784) — translate SFL→GLS, AIT→ATF for CLASS
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let class_ctype = translate_class_projection_write(ctype);
            let key = key_for("CTYPE", Some(i + 1), s);
            cards.set_string(
                &key,
                &class_ctype,
                &format!("Type of co-ordinate on axis {}", i + 1),
            );
        }
    }

    // CDELTi (C:6786-6791)
    for i in 0..naxis {
        if let Some(val) = store.cdelt[i] {
            let key = key_for("CDELT", Some(i + 1), s);
            cards.set_float(&key, val, "Pixel size");
        }
    }

    // EQUINOX (C:6794) — per-version-letter, but CLASS only writes
    // primary so equivalent to EQUINOX without suffix.
    if is_last && let Some(equinox) = store.equinox {
        let key = key_for("EQUINOX", None, s);
        cards.set_float(&key, equinox, "Epoch of reference equinox");
    }

    // DATE-OBS (C:6797-6821) — only on last plane.
    if is_last && let Some(ref dateobs) = store.dateobs {
        cards.set_string("DATE-OBS", dateobs, "Date of observation");
    }
    // A read leaves any MJD-OBS card in the header, since other
    // software may use it; update it so that it agrees with DATE-OBS.
    if is_last
        && cards.find("MJD-OBS").is_some()
        && let Some(mjdobs) = store.mjdobs
    {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
    }

    // RESTFREQ (C:6824) — only on last plane.
    if is_last && let Some(rf) = restfreq {
        cards.set_float("RESTFREQ", rf, "[Hz] Rest frequency");
    }

    // IMAGFREQ (C:6826-6831) — only on last plane, only for DSB.
    if is_last && let Some(im_freq) = store.imagfreq {
        cards.set_float("IMAGFREQ", im_freq, "[Hz] Image frequency");
    }

    // OBJECT and LINE placeholders (C:6833-6841) — only on last
    // plane, always emit a single-blank value if not already set.
    if is_last {
        if cards.find("OBJECT").is_none() {
            cards.set_string("OBJECT", " ", "");
        }
        if cards.find("LINE").is_none() {
            cards.set_string("LINE", " ", "");
        }
    }

    // VELO-LSR / VLSR / DELTAV (C:6843-6898) — emitted by
    // [`write_velocities_from_frameset`] via the SpecFrame find_frame
    // pipeline; can't be done here because we need the FrameSet, not
    // just the FitsStore. The dispatch site in fitschan/mod.rs calls
    // both functions in sequence.
    let _ = (spec_idx, restfreq);

    // AZIMUTH and ELEVATIO (C:6901-6944) — Az/El conversion for the
    // spatial reference point. Deferred to a follow-up; the simpler
    // CLASS fixtures (class-ait, class-sin, etc.) include
    // pre-computed AZIMUTH/ELEVATIO that carry through from the
    // input header unchanged.

    Ok(true)
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Translate standard projection codes back to CLASS format on write.
///
/// - SFL → GLS
/// - AIT → ATF
fn translate_class_projection_write(ctype: &str) -> String {
    if ctype.len() >= 8 {
        let base = &ctype[..5];
        let proj = &ctype[5..];
        let new_proj = match proj.to_uppercase().as_str() {
            "SFL" => "GLS",
            "AIT" => "ATF",
            _ => return ctype.to_string(),
        };
        return format!("{base}{new_proj}");
    }
    ctype
        .replace("SFL", "GLS")
        .replace("AIT", "ATF")
        .replace("sfl", "GLS")
        .replace("ait", "ATF")
}

/// FITS-CLASS velocity emission. Mirrors fitschan.c:6907-6962.
///
/// Computes VELO-LSR / VLSR / DELTAV from the FrameSet's SpecFrame
/// component using the find_frame + convert pipeline. Run after the
/// per-axis FitsStore extraction has already emitted CRPIX / CRVAL /
/// etc. via [`write_to_cards`]; this function adds the four velocity-
/// keyword cards in their canonical position.
pub(crate) fn write_velocities_from_frameset(fs: &FrameSet, cards: &mut CardList) -> AstResult<()> {
    use crate::coords::PointSet;

    // C `CLASSFromStore` receives the ORIGINAL input FrameSet (the
    // `(AstFrameSet*) object` passed to FitsFromStore at fitschan.c:40520),
    // NOT the post-MakeFitsFrameSet FrameSet. The modifications applied by
    // MakeFitsFrameSet (RefRA/RefDec format/parse roundtrip, AddEncodingFrame)
    // operate on a *copy* and feed only into the FitsStore extraction.
    // The SpecFrame used by `astFindFrame(curfrm, velofrm, "")` here keeps
    // its astRead-derived high-precision RefRA/RefDec.

    // Locate the matched SpecFrame in the cube via find_frame with a
    // velocity SpecFrame template. C constructs:
    //   velofrm = SpecFrame(System=vrad, StdOfRest=lsrk, Unit=m/s,
    //                       MinAxes=1, MaxAxes=3)
    let velo_template = {
        let mut v = SpecFrame::new(SpecSystem::Vradio);
        v.set_std_of_rest(StdOfRest::Lsrk)?;
        v.set_unit(1, "m/s")?;
        v.set_min_axes(1)?;
        v.set_max_axes(3)?;
        v
    };
    let Some(found) = Frame::find_frame_template(fs.current(), &velo_template, "")? else {
        return Ok(());
    };
    // The match is a copy of the cube's spectral Frame, which may be a
    // DSBSpecFrame; C uses it through the SpecFrame interface, and with
    // AlignSideBand unset a DSBSpecFrame converts as its SpecFrame part does.
    let velo_frame = crate::frames::specframe::as_spec_frame(found.current())
        .expect("find_frame returns a SpecFrame for a SpecFrame template")
        .clone();

    // Build the freq frame (Source-Hz) from a copy of velo_frame.
    let mut freq_frame = velo_frame.clone();
    freq_frame.set_system(SpecSystem::Freq)?;
    freq_frame.set_std_of_rest(StdOfRest::Source)?;
    freq_frame.set_unit(1, "Hz")?;

    // Convert freq → velo to get the Mapping that takes Source-Hz
    // to LSRK-vrad-m/s.
    let Some(conv_fs) = freq_frame.convert_to(&velo_frame, &[])? else {
        return Ok(());
    };
    let mapping = conv_fs.mapping(FrameRef::Base, FrameRef::Current)?;

    // Pull the spectral CRVAL and CDELT from the cards. The
    // spectral FITS axis is whichever CTYPEi is FREQ-/WAVE-/etc;
    // C uses the `axspec` index returned by FindLonLatSpecAxes
    // (fitschan.c:3264) and reads `crval[axspec]` and
    // `cdelt[axspec]`. We scan the CTYPE cards to find the same
    // 1-based index. Mirrors C:6881-6882.
    //
    // The FitsStore's CRVAL for the spectral axis was emitted as an
    // offset from RESTFREQ (see `write_to_cards` above), so adding
    // RESTFREQ recovers the absolute Source-Hz frequency.
    let restfreq = cards.get_float("RESTFREQ");
    let Some(rf) = restfreq else {
        return Ok(());
    };

    let mut spec_axis_1based: Option<usize> = None;
    for i in 1..=99 {
        let key = format!("CTYPE{i}");
        if let Some(ct) = cards.get_string(&key) {
            let prefix = ct.get(..4).unwrap_or("");
            if matches!(
                prefix,
                "FREQ"
                    | "ENER"
                    | "WAVN"
                    | "VRAD"
                    | "WAVE"
                    | "VOPT"
                    | "ZOPT"
                    | "AWAV"
                    | "VELO"
                    | "BETA"
            ) {
                spec_axis_1based = Some(i);
                break;
            }
        } else if i > 1 {
            break;
        }
    }
    let Some(axspec_1) = spec_axis_1based else {
        return Ok(());
    };

    let crval_key = format!("CRVAL{axspec_1}");
    let cdelt_key = format!("CDELT{axspec_1}");
    let crval = cards.get_float(&crval_key);
    let cdelt = cards.get_float(&cdelt_key);

    let (Some(crval), Some(cdelt)) = (crval, cdelt) else {
        return Ok(());
    };

    let aval0_in = crval + rf;
    let aval1_in = aval0_in + cdelt;

    // Push the two values through the mapping (Source-Hz → LSRK-vrad-m/s).
    let pts = PointSet::from_rows([vec![aval0_in], vec![aval1_in]]);
    let out = mapping.transform(&pts, true)?;
    let velo_lsr = out.point(0)[0];
    let velo_next = out.point(1)[0];

    cards.set_float("VELO-LSR", velo_lsr, "[m/s] Reference velocity");
    cards.set_float("VLSR", velo_lsr, "[m/s] Reference velocity");
    cards.set_float("DELTAV", velo_next - velo_lsr, "[m/s] Velocity resolution");

    Ok(())
}

/// FITS-CLASS AZIMUTH / ELEVATIO emission. Mirrors fitschan.c:6965-7008.
///
/// Takes the spatial reference point's (RA, Dec) in radians, converts
/// it to (Az, El) in radians, normalises, and emits AZIMUTH / ELEVATIO
/// in degrees.
///
/// The conversion goes through two find_frame calls:
/// 1. find_frame(curfrm, SkyFrame{Permute=0, MinAxes=3, MaxAxes=3})
///    → Mapping curfrm → matched RA/Dec frame, "fsconv1".
/// 2. find_frame(curfrm, SkyFrame{System=AzEl, Permute=0, MinAxes=3,
///    MaxAxes=3}) → Mapping curfrm → matched Az/El frame, "fsconv2".
///
/// Composing fsconv1.inverse with fsconv2 gives radec → azel.
///
/// The (lon, lat) values fed into the chain are the CRVAL of the
/// longitude / latitude FITS axes (located via [`find_lon_lat_axes`]),
/// matching C's `crval[axlon]` / `crval[axlat]` (fitschan.c:6986).
/// Find the 1-based longitude / latitude FITS axis indices by scanning the
/// CTYPE cards. Mirrors the celestial-axis identification in C
/// `FindLonLatSpecAxes` (fitschan.c:3267): the longitude axis CTYPE begins
/// with `RA--` or `xLON` (the `x` being the system code, e.g. `G` for
/// galactic, `E` for ecliptic), and the latitude axis CTYPE begins with
/// `DEC-` or `xLAT`. Returns `(None, None)` when either is absent.
fn find_lon_lat_axes(cards: &CardList) -> (Option<usize>, Option<usize>) {
    let is_lon =
        |ct: &str| -> bool { ct.starts_with("RA--") || (ct.len() >= 4 && &ct[1..4] == "LON") };
    let is_lat =
        |ct: &str| -> bool { ct.starts_with("DEC-") || (ct.len() >= 4 && &ct[1..4] == "LAT") };

    let mut axlon = None;
    let mut axlat = None;
    for i in 1..=99 {
        let key = format!("CTYPE{i}");
        let Some(ct) = cards.get_string(&key) else {
            if i > 1 {
                break;
            }
            continue;
        };
        if axlon.is_none() && is_lon(&ct) {
            axlon = Some(i);
        }
        if axlat.is_none() && is_lat(&ct) {
            axlat = Some(i);
        }
    }
    (axlon, axlat)
}

pub(crate) fn write_azel_from_frameset(fs: &FrameSet, cards: &mut CardList) -> AstResult<()> {
    use crate::coords::PointSet;
    use crate::mappings::{CmpMap, Composition};

    // Spatial reference point in degrees: C uses `crval[axlon]` /
    // `crval[axlat]` (fitschan.c:6986-6987), where `axlon` / `axlat` are
    // the longitude / latitude FITS axis indices found by
    // `FindLonLatSpecAxes` (fitschan.c:3267). Locate the same axes by
    // scanning the CTYPE cards for the standard celestial-axis prefixes
    // (RA--/DEC-, and the generic `xLON`/`xLAT` forms for galactic,
    // ecliptic, etc). The spatial axes are not necessarily axes 2 and 3.
    let (Some(axlon_1), Some(axlat_1)) = find_lon_lat_axes(cards) else {
        return Ok(());
    };
    let (Some(crval_lon), Some(crval_lat)) = (
        cards.get_float(&format!("CRVAL{axlon_1}")),
        cards.get_float(&format!("CRVAL{axlat_1}")),
    ) else {
        return Ok(());
    };

    // C `CLASSFromStore` receives the ORIGINAL input FrameSet, NOT the
    // post-MakeFitsFrameSet one (see `write_velocities_from_frameset`).
    // The current Frame here is the user-supplied frame as read by astRead.

    // C invokes astFindFrame on the current Frame (extracted via
    // astGetFrame), not on the FrameSet itself. So the dispatch is
    // through the Frame trait's find_frame_template method.
    let curfrm = fs.current();

    // SkyFrame template for the source celestial frame. Mirrors
    // C:6908 — `astSkyFrame("Permute=0,MinAxes=3,MaxAxes=3")`
    // leaves the System attribute unset, which lets it inherit from
    // the matched target axes. For class-gal that's Galactic; for
    // FK5 inputs it stays FK5. Without `unset_system` the template
    // would force the result to a specific system, miscomputing
    // AZIMUTH for galactic inputs by O(degrees) since the chain
    // would expect FK5 RA/Dec while CLASS feeds the lon/lat CRVAL
    // in whatever the CTYPE system is.
    let radec_template = {
        let mut sky = SkyFrame::unset_system();
        sky.set_permute(false)?;
        sky.set_min_axes(3)?;
        sky.set_max_axes(3)?;
        sky
    };
    let Some(fsconv1) = Frame::find_frame_template(curfrm, &radec_template, "")? else {
        return Ok(());
    };

    // SkyFrame template for the AzEl frame. Mirrors C:6912.
    let azel_template = {
        let mut sky = SkyFrame::new(SkySystem::Azel);
        sky.set_permute(false)?;
        sky.set_min_axes(3)?;
        sky.set_max_axes(3)?;
        sky
    };
    let Some(fsconv2) = Frame::find_frame_template(curfrm, &azel_template, "")? else {
        return Ok(());
    };

    // Compose fsconv1 (radec → curfrm via Current → Base) ∘
    // fsconv2 (curfrm → azel via Base → Current) = radec → azel.
    // Mirrors C:6918-6920.
    let map_radec_to_cur = fsconv1.mapping(FrameRef::Current, FrameRef::Base)?;
    let map_cur_to_azel = fsconv2.mapping(FrameRef::Base, FrameRef::Current)?;
    let map_radec_to_azel: crate::mappings::DynMapping = Box::new(CmpMap::new(
        map_radec_to_cur,
        map_cur_to_azel,
        Composition::Series,
    )?);

    // Convert (CRVAL_lon, CRVAL_lat) deg → rad and push through.
    const D2R: f64 = std::f64::consts::PI / 180.0;
    const R2D: f64 = 180.0 / std::f64::consts::PI;
    let pts = PointSet::from_rows([vec![crval_lon * D2R, crval_lat * D2R]]);
    let out = map_radec_to_azel.transform(&pts, true)?;
    let mut out_pair = [out.point(0)[0], out.point(0)[1]];

    // Normalise via the matched AzEl SkyFrame (C:6929) and read the
    // azimuth / elevation back from the SkyFrame's external axes
    // (perm-aware: a lat-first AzEl frame returns El at slot 0 and
    // Az at slot 1).
    let (az, el) = if let Some(azel_frame) = fsconv2.current().as_any().downcast_ref::<SkyFrame>() {
        azel_frame.norm(&mut out_pair)?;
        let lon = out_pair[azel_frame.lon_pos()];
        let lat = out_pair[azel_frame.lat_pos()];
        (lon, lat)
    } else {
        (out_pair[0], out_pair[1])
    };

    cards.set_float("AZIMUTH", az * R2D, "[Deg] Telescope azimuth");
    cards.set_float("ELEVATIO", el * R2D, "[Deg] Telescope elevation");

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::Encoding;
    use crate::fitschan::spec_trans::spec_trans;

    /// The store C's `WcsToStore` makes of a translated header.
    fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
        Ok(
            super::super::wcs_keywords::extract_wcs_keywords_with_alts(cards)?
                .primary
                .expect("a primary axis description"),
        )
    }

    /// Apply CLASS-encoding `spec_trans` translations to a header
    /// before reading. Tests that exercise CLASS-specific keyword
    /// translations (CRVAL→CRVAL+RESTFREQ shift, SSYSSRC/ZSOURCE
    /// synthesis, GLS→SFL CTYPE rewrites) must go through this
    /// pipeline because `read_from_cards` expects already-
    /// translated cards (mirrors C's runtime: `astRead` calls
    /// `SpecTrans` before the per-axis store extraction). Direct
    /// callers of `read_from_cards` on un-translated CLASS headers
    /// will see absolute frequencies that haven't been shifted.
    fn translated(header: &str) -> CardList {
        let cards = CardList::from_header(header).unwrap();
        spec_trans(
            &cards,
            Encoding::FitsClass,
            crate::fitschan::spec_trans::TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        )
    }

    /// Standard CLASS header for testing.
    const CLASS_HEADER: &str = "\
NAXIS   =                    3
CTYPE1  = 'FREQ    '
CRVAL1  =  0.000000000000E+000
CRPIX1  =              512.500
CDELT1  =  0.390625000000E+005
CTYPE2  = 'RA---TAN'
CRVAL2  =               40.738
CRPIX2  =                  0.5
CDELT2  =              -0.0100
CTYPE3  = 'DEC--TAN'
CRVAL3  =               62.933
CRPIX3  =                  0.5
CDELT3  =               0.0100
RADESYS = 'FK5     '
EQUINOX =               2000.0
SPECSYS = 'SOURCE  '
RESTFREQ=  0.110201354300E+012
VELO-LSR=  0.250000000000E+004
DELTAV  = -0.106265872717E+003
MJD-OBS =            52413.59
";

    #[test]
    fn read_class_offset_frequency() {
        // CRVAL1=0, RESTFREQ=1.102e11 → store.crval[0] ≈ 1.102e11
        let cards = translated(CLASS_HEADER);
        let store = read_from_cards(&cards).unwrap();

        let restfreq = 0.110201354300e12;
        // Offset is 0, so absolute = 0 + RESTFREQ
        let crval_spec = store.crval[0].unwrap();
        assert!(
            (crval_spec - restfreq).abs() < 1.0,
            "Expected crval[0] ≈ {restfreq}, got {crval_spec}"
        );
    }

    #[test]
    fn read_class_with_nonzero_offset() {
        // CRVAL1=1e6, RESTFREQ=1.102e11 → store.crval[0] ≈ 1.102e11 + 1e6
        let header = "\
NAXIS   =                    3
CTYPE1  = 'FREQ    '
CRVAL1  =  1.000000000000E+006
CRPIX1  =              512.500
CDELT1  =  0.390625000000E+005
CTYPE2  = 'RA---TAN'
CRVAL2  =               40.738
CRPIX2  =                  0.5
CDELT2  =              -0.0100
CTYPE3  = 'DEC--TAN'
CRVAL3  =               62.933
CRPIX3  =                  0.5
CDELT3  =               0.0100
RESTFREQ=  0.110201354300E+012
VELO-LSR=  0.250000000000E+004
DELTAV  = -0.106265872717E+003
";
        let cards = translated(header);
        let store = read_from_cards(&cards).unwrap();

        let restfreq = 0.110201354300e12;
        let offset = 1.0e6;
        let expected = restfreq + offset;
        let crval_spec = store.crval[0].unwrap();
        assert!(
            (crval_spec - expected).abs() < 1.0,
            "Expected crval[0] ≈ {expected}, got {crval_spec}"
        );
    }

    #[test]
    fn read_class_gls_to_sfl() {
        // CTYPE "GLON-GLS" → "GLON-SFL"
        let header = "\
NAXIS   =                    3
CTYPE1  = 'FREQ    '
CRVAL1  =  0.000000000000E+000
CRPIX1  =              512.500
CDELT1  =  0.390625000000E+005
CTYPE2  = 'GLON-GLS'
CRVAL2  =               40.738
CRPIX2  =                  0.5
CDELT2  =              -0.0100
CTYPE3  = 'GLAT-GLS'
CRVAL3  =               62.933
CRPIX3  =                  0.5
CDELT3  =               0.0100
RESTFREQ=  0.110201354300E+012
VELO-LSR=  0.250000000000E+004
DELTAV  = -0.106265872717E+003
";
        let cards = translated(header);
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.ctype[1], Some("GLON-SFL".to_string()));
        assert_eq!(store.ctype[2], Some("GLAT-SFL".to_string()));
    }

    #[test]
    fn read_class_ssyssrc_from_velo_lsr() {
        // CLASS data is always in SOURCE rest frame. VELO-LSR
        // present → ssyssrc="LSRK". Mirrors C `ClassTrans`
        // (fitschan.c:7128-7184).
        let cards = translated(CLASS_HEADER);
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.specsys, Some("SOURCE".to_string()));
        assert_eq!(store.ssyssrc, Some("LSRK".to_string()));
    }

    #[test]
    fn read_class_ssyssrc_from_velo_hel() {
        // VELO-HEL present → ssyssrc="BARYCENT" (specsys still SOURCE).
        let header = "\
NAXIS   =                    3
CTYPE1  = 'FREQ    '
CRVAL1  =  0.000000000000E+000
CRPIX1  =              512.500
CDELT1  =  0.390625000000E+005
CTYPE2  = 'RA---TAN'
CRVAL2  =               40.738
CRPIX2  =                  0.5
CDELT2  =              -0.0100
CTYPE3  = 'DEC--TAN'
CRVAL3  =               62.933
CRPIX3  =                  0.5
CDELT3  =               0.0100
RESTFREQ=  0.110201354300E+012
VELO-HEL=  0.250000000000E+004
DELTAV  = -0.106265872717E+003
";
        let cards = translated(header);
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.specsys, Some("SOURCE".to_string()));
        assert_eq!(store.ssyssrc, Some("BARYCENT".to_string()));
    }

    #[test]
    fn write_class_subtracts_restfreq() {
        // store.crval[spec]=1.102e11, restfreq=1.102e11 → output CRVAL≈0
        let restfreq = 0.110201354300e12;
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(restfreq); // absolute freq = restfreq → offset = 0
        store.crpix[0] = Some(512.5);
        store.cdelt[0] = Some(39062.5);
        store.ctype[1] = Some("RA---TAN".to_string());
        store.crval[1] = Some(40.738);
        store.crpix[1] = Some(0.5);
        store.cdelt[1] = Some(-0.01);
        store.ctype[2] = Some("DEC--TAN".to_string());
        store.crval[2] = Some(62.933);
        store.crpix[2] = Some(0.5);
        store.cdelt[2] = Some(0.01);
        store.restfrq = Some(restfreq);
        store.radesys = Some("FK5".to_string());
        store.equinox = Some(2000.0);
        store.specsys = Some("SOURCE".to_string());
        store.cunit[0] = Some("Hz".to_string());

        let mut cards = CardList::new();
        assert!(write_to_cards(&store, &mut cards, ' ', true).unwrap());

        // The output CRVAL1 should be approximately 0 (absolute - restfreq)
        let crval1 = cards.get_float("CRVAL1").unwrap();
        assert!(crval1.abs() < 1.0, "Expected CRVAL1 ≈ 0, got {crval1}");

        // Non-spectral axes should remain unchanged
        let crval2 = cards.get_float("CRVAL2").unwrap();
        assert!((crval2 - 40.738).abs() < 1e-10);
    }

    // DELTAV / VELO-LSR are no longer computed inside
    // `write_to_cards`; they're emitted by
    // `write_velocities_from_frameset`, which needs a FrameSet
    // (not a raw FitsStore) and the velocity-Mapping from
    // SpecFrame::convert. The end-to-end pipeline is exercised by
    // the wcsconv fixtures (class-ait, class-sin, etc.).

    #[test]
    fn write_class_sfl_to_gls() {
        // SFL → GLS on write, for an SFL projection at the origin, the
        // only one FITS-CLASS can describe.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.0e11);
        store.crpix[0] = Some(512.5);
        store.cdelt[0] = Some(39062.5);
        store.ctype[1] = Some("GLON-SFL".to_string());
        store.crval[1] = Some(0.0);
        store.crpix[1] = Some(0.5);
        store.cdelt[1] = Some(-0.01);
        store.ctype[2] = Some("GLAT-SFL".to_string());
        store.crval[2] = Some(0.0);
        store.crpix[2] = Some(0.5);
        store.cdelt[2] = Some(0.01);
        store.restfrq = Some(1.0e11);
        store.specsys = Some("SOURCE".to_string());
        store.cunit[0] = Some("Hz".to_string());

        let mut cards = CardList::new();
        assert!(write_to_cards(&store, &mut cards, ' ', true).unwrap());

        assert_eq!(cards.get_string("CTYPE2"), Some("GLON-GLS".to_string()));
        assert_eq!(cards.get_string("CTYPE3"), Some("GLAT-GLS".to_string()));
    }

    #[test]
    fn read_class_spatial_axes_unchanged() {
        // Spatial axes (RA, DEC) should not have offset correction applied.
        let cards = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards).unwrap();

        let crval_ra = store.crval[1].unwrap();
        let crval_dec = store.crval[2].unwrap();
        assert!((crval_ra - 40.738).abs() < 1e-10);
        assert!((crval_dec - 62.933).abs() < 1e-10);
    }

    #[test]
    fn read_class_equinox_and_radesys() {
        let cards = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.radesys, Some("FK5".to_string()));
    }

    #[test]
    fn read_class_mjdobs() {
        let cards = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.mjdobs, Some(52413.59));
    }

    #[test]
    fn round_trip_class() {
        // Read → write → read should preserve values when both reads
        // go through the spec_trans pipeline (which mirrors C's
        // runtime path).
        let cards_in = translated(CLASS_HEADER);
        let mut store = read_from_cards(&cards_in).unwrap();
        // The header gives no CUNIT1; a store made from a FrameSet has the
        // SpecFrame's unit, which FITS-CLASS needs.
        store.cunit[0] = Some("Hz".to_string());

        let mut cards_out = CardList::new();
        assert!(write_to_cards(&store, &mut cards_out, ' ', true).unwrap());

        let cards_out_translated = spec_trans(
            &cards_out,
            Encoding::FitsClass,
            crate::fitschan::spec_trans::TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        let store2 = read_from_cards(&cards_out_translated).unwrap();

        // Spectral axis should round-trip
        let crval1_orig = store.crval[0].unwrap();
        let crval1_rt = store2.crval[0].unwrap();
        assert!(
            (crval1_orig - crval1_rt).abs() < 1.0,
            "Spectral CRVAL round-trip failed: {crval1_orig} vs {crval1_rt}"
        );

        // Spatial axes should round-trip
        let crval2_orig = store.crval[1].unwrap();
        let crval2_rt = store2.crval[1].unwrap();
        assert!((crval2_orig - crval2_rt).abs() < 1e-10);
    }
}

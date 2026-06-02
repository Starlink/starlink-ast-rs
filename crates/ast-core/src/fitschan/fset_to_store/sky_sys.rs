//! Port of `fitschan.c::SkySys` (~29599) and `IsSkyOff` (~20222).
//!
//! `is_sky_off` classifies a Frame as offset/non-offset based on its
//! (or its embedded SkyFrame's) SkyRefIs attribute.
//!
//! `sky_sys` writes celestial-axis FITS keywords (`CTYPE` /
//! `CTYPE`-comment / `CNAME` / `CUNIT` / `RADESYS` / `EQUINOX` /
//! `MJD-OBS` / `TIMESYS`) into a [`FitsStore`].
//!
//! All known C branches in `SkySys` are now ported. The only
//! remaining infrastructure gap is the multi-timescale `TIMESYS`
//! header — `tdb_conv` covers `UTC / TAI / TT / TDB` (the timescales
//! the writer actually produces); `UT1` / `LT` / sidereal scales
//! aren't reachable from a SkyFrame today.

// Wired in by celestial_axes (Task 5.3); until then nothing here
// is reachable from the crate's non-test code.
#![allow(dead_code)]

use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::sky_convert::{SkyRefIs, SkySystem};
use crate::frames::{Frame, FrameRef, FrameSet, SkyFrame};
use crate::wcslib::WcsProjection;

/// Wcsproj name lookup matching `astWcsPrjName`. Returns the FITS
/// 4-character algorithm code for each projection, prefixed by `'-'`
/// (e.g. `"-TAN"`, `"-SIN"`).
fn wcs_prj_name(proj: WcsProjection) -> &'static str {
    match proj {
        WcsProjection::Azp => "-AZP",
        WcsProjection::Szp => "-SZP",
        WcsProjection::Tan => "-TAN",
        WcsProjection::Stg => "-STG",
        WcsProjection::Sin => "-SIN",
        WcsProjection::Arc => "-ARC",
        WcsProjection::Zpn => "-ZPN",
        WcsProjection::Zea => "-ZEA",
        WcsProjection::Air => "-AIR",
        WcsProjection::Cyp => "-CYP",
        WcsProjection::Cea => "-CEA",
        WcsProjection::Car => "-CAR",
        WcsProjection::Mer => "-MER",
        WcsProjection::Cop => "-COP",
        WcsProjection::Coe => "-COE",
        WcsProjection::Cod => "-COD",
        WcsProjection::Coo => "-COO",
        WcsProjection::Sfl => "-SFL",
        WcsProjection::Par => "-PAR",
        WcsProjection::Mol => "-MOL",
        WcsProjection::Ait => "-AIT",
        WcsProjection::Bon => "-BON",
        WcsProjection::Pco => "-PCO",
        WcsProjection::Tsc => "-TSC",
        WcsProjection::Csc => "-CSC",
        WcsProjection::Qsc => "-QSC",
        WcsProjection::Hpx => "-HPX",
        WcsProjection::Xph => "-XPH",
        WcsProjection::Tpn => "-TPN",
        // The wcslib default ("none") shouldn't normally reach SkySys,
        // but in case it does we fall back to the unknown placeholder.
        _ => "----",
    }
}

/// Classify a frame as containing an offset-SkyFrame.
///
/// Returns:
/// - `1` if `fset.frame(iframe)` is a SkyFrame whose `SkyRefIs`
///   is `Pole` or `Origin` — or if it's a CmpFrame containing such.
/// - `0` otherwise.
///
/// `iframe` is a 0-based frame index within the FrameSet.
///
/// Mirrors `fitschan.c::IsSkyOff` (~20222).
pub(crate) fn is_sky_off(fset: &FrameSet, iframe: usize) -> i32 {
    let frame = match fset.frame(FrameRef::Index(iframe + 1)) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    if frame_contains_offset_sky(frame) {
        1
    } else {
        0
    }
}

/// Recursive helper: does `frame` (or any embedded sub-frame) carry an
/// offset SkyFrame (`SkyRefIs == Pole | Origin`)?
fn frame_contains_offset_sky(frame: &dyn Frame) -> bool {
    if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
        return matches!(sky.sky_ref_is(), SkyRefIs::Pole | SkyRefIs::Origin);
    }
    if let Some(cmp) = frame.as_any().downcast_ref::<crate::frames::CmpFrame>() {
        return frame_contains_offset_sky(cmp.frame1()) || frame_contains_offset_sky(cmp.frame2());
    }
    false
}

/// SkySys-side encoding of the celestial system, mirroring the C
/// `isys` enumeration (`RADEC`, `ECLIP`, etc.) used to drive the
/// CTYPE prefix selection table at `fitschan.c:29818-29876`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CelestialKind {
    /// RA/Dec — FK4, FK4-NO-E, FK5, ICRS, GAPPT (plus J2000 in our
    /// extended SkySystem).
    RaDec,
    /// Ecliptic.
    Eclip,
    /// Helioecliptic.
    Heclip,
    /// Galactic.
    Galac,
    /// Supergalactic.
    Super,
    /// Azimuth/elevation.
    AzEl,
    /// Unknown / not classified.
    Nocel,
}

/// Map a `SkySystem` to the C `isys` enumeration plus the `RADESYS`
/// string (only present for `RaDec` systems).
fn classify_system(sys: SkySystem) -> (CelestialKind, Option<&'static str>) {
    match sys {
        SkySystem::Fk4 => (CelestialKind::RaDec, Some("FK4")),
        SkySystem::Fk4NoE => (CelestialKind::RaDec, Some("FK4-NO-E")),
        SkySystem::Fk5 => (CelestialKind::RaDec, Some("FK5")),
        SkySystem::Icrs => (CelestialKind::RaDec, Some("ICRS")),
        SkySystem::Gappt => (CelestialKind::RaDec, Some("GAPPT")),
        SkySystem::J2000 => (CelestialKind::RaDec, Some("ICRS")),
        SkySystem::Ecliptic => (CelestialKind::Eclip, None),
        SkySystem::Helioecliptic => (CelestialKind::Heclip, None),
        SkySystem::Galactic => (CelestialKind::Galac, None),
        SkySystem::Supergalactic => (CelestialKind::Super, None),
        SkySystem::Azel => (CelestialKind::AzEl, None),
        SkySystem::Unknown => (CelestialKind::Nocel, None),
    }
}

/// Build the 4-character CTYPE prefix pair for a celestial system.
/// Mirrors `fitschan.c:29818-29876` exactly (including the unknown-
/// system fallback rule that inspects the SkyFrame axis symbols).
///
/// `axes_one_based`: pass the (longitude, latitude) axis indices in
/// 1-based form so `SkyFrame::axis_symbol` can resolve them.
fn ctype_prefixes_for(
    kind: CelestialKind,
    isoff: i32,
    skyfrm: &SkyFrame,
    axes_one_based: (usize, usize),
) -> (String, String) {
    if isoff > 0 {
        return ("OFLN".to_string(), "OFLT".to_string());
    }
    match kind {
        CelestialKind::RaDec => ("RA--".to_string(), "DEC-".to_string()),
        CelestialKind::Eclip => ("ELON".to_string(), "ELAT".to_string()),
        CelestialKind::Heclip => ("HLON".to_string(), "HLAT".to_string()),
        CelestialKind::Galac => ("GLON".to_string(), "GLAT".to_string()),
        CelestialKind::Super => ("SLON".to_string(), "SLAT".to_string()),
        CelestialKind::AzEl => ("AZ--".to_string(), "EL--".to_string()),
        CelestialKind::Nocel => {
            // Inspect the SkyFrame axis symbols. FITS-WCS recognises
            // either xxLN/xxLT (4-char) or xLON/xLAT (4-char); if the
            // pair conforms, use them verbatim. Otherwise fall back
            // to UNLN/UNLT. Mirrors C:29844-29876.
            let (lon_ax, lat_ax) = axes_one_based;
            let lon_sym = skyfrm.axis_symbol(lon_ax).unwrap_or_default();
            let lat_sym = skyfrm.axis_symbol(lat_ax).unwrap_or_default();
            let conforms = if lon_sym.len() == 4 && lat_sym.len() == 4 {
                let lon_b = lon_sym.as_bytes();
                let lat_b = lat_sym.as_bytes();
                // xxLN / xxLT (last 2 chars LN / LT, first 2 match).
                let xx_form =
                    lat_b[2..] == *b"LT" && lon_b[2..] == *b"LN" && lon_b[..2] == lat_b[..2];
                // xLON / xLAT (last 3 chars LON/LAT, first char matches).
                let lon_form =
                    lat_b[1..] == *b"LAT" && lon_b[1..] == *b"LON" && lon_b[..1] == lat_b[..1];
                xx_form || lon_form
            } else {
                false
            };
            let (lon_use, lat_use) = if conforms {
                (lon_sym.as_str(), lat_sym.as_str())
            } else {
                ("UNLN", "UNLT")
            };
            // Pad to 4 chars with '-' if shorter (mirrors C:29867-29874).
            let pad4 = |s: &str| {
                let mut t = s.chars().take(4).collect::<String>();
                while t.len() < 4 {
                    t.push('-');
                }
                t
            };
            (pad4(lon_use), pad4(lat_use))
        }
    }
}

/// Convert the SkyFrame's stored EquinoxMJD into the FITS Equinox
/// year. C: `eq = palEpb(eq_mjd)` for FK4 (Besselian), `eq =
/// palEpj(eq_mjd)` for FK5/Ecliptic (Julian); BAD for ICRS/GAPPT/
/// galactic/etc. Matches the conditional branches at
/// `fitschan.c:29761-29800`.
fn fits_equinox_year(sys: SkySystem, eq_mjd: f64) -> Option<f64> {
    match sys {
        SkySystem::Fk4 | SkySystem::Fk4NoE => Some(crate::pal::epb(eq_mjd)),
        SkySystem::Fk5 | SkySystem::Ecliptic => Some(crate::pal::epj(eq_mjd)),
        _ => None,
    }
}

/// Write celestial-axis FITS keywords into `store`. Mirrors
/// `fitschan.c::SkySys` (~29599).
///
/// Parameters mirror the C signature:
/// - `wcstype`: `0` for `-TAB`, `1` for a WcsMap projection.
/// - `wcsproj`: only consulted when `wcstype == 1`.
/// - `axlon` / `axlat`: 0-based FITS-axis indices.
/// - `s`: coordinate-version character (`' '` for primary).
/// - `isoff`: as documented in the C source — `>0` ⇒ describe offset
///   coords; `<0` ⇒ describe absolute coords with offset metadata
///   stored in AST-extension keywords (deferred — not yet emitted on
///   the Rust side, see module head); `0` ⇒ ignore offset.
///
/// Returns `Ok(true)` on success.
#[allow(clippy::too_many_arguments)] // Mirrors the C signature (10 args) one-for-one.
pub(crate) fn sky_sys(
    skyfrm: &SkyFrame,
    wcstype: i32,
    wcsproj: WcsProjection,
    store: &mut FitsStore,
    axlon: usize,
    axlat: usize,
    s: char,
    isoff: i32,
) -> AstResult<bool> {
    // System + classification.
    let sys = skyfrm.system();
    let (kind, radesys) = classify_system(sys);

    // Equinox / epoch.
    let eq_mjd = skyfrm.equinox_mjd();
    let ep_mjd_explicit = skyfrm.test_epoch();
    let ep_tdb = if ep_mjd_explicit {
        Some(skyfrm.epoch_mjd())
    } else {
        None
    };

    // C converts ep_tdb → ep_utc via TDBConv. Mirrors fitschan.c:29724.
    let ep_utc =
        ep_tdb.and_then(|tdb| crate::pal::tdb_conv(tdb, crate::pal::TdbConvScale::Utc, true));

    // Default-date check: when ep_utc == eq, the FITS spec says
    // MJD-OBS / DATE-OBS default to the equinox epoch — so we don't
    // emit them. C: fitschan.c:29757 (defdate = astEQUAL(ep_utc, eq)).
    let defdate = match (ep_utc, sys) {
        (Some(e), s) if s.needs_equinox() => (e - eq_mjd).abs() < 1e-9,
        _ => false,
    };

    // Convert equinox to FITS year. None means "do not emit EQUINOX"
    // (RADEC systems other than FK4/FK5 leave it unset).
    let eq_year = fits_equinox_year(sys, eq_mjd);

    // Store RADESYS / EQUINOX / MJD-OBS.
    if let Some(rs) = radesys {
        store.radesys = Some(rs.to_string());
    }
    if let Some(y) = eq_year {
        store.equinox = Some(y);
    }
    if !defdate {
        if let Some(e) = ep_utc {
            store.mjdobs = Some(e);
        }
    }

    // Observation-global TAI/UT1 offsets — read from the SkyFrame and
    // copy into the store. Last-plane-wins per the global-card
    // contract (see FitsStore::mjdobs doc).
    if let Some(dtai) = skyfrm.dtai() {
        store.dtai = Some(dtai);
    }
    if let Some(dut1) = skyfrm.dut1() {
        store.dut1 = Some(dut1);
    }

    // CTYPE construction. Skip the projection suffix path when
    // wcstype == 0 (mirrors the C switch at line 29879).
    // Pass 1-based SkyFrame axis indices for the unknown-system
    // symbol lookup (lon = lon_axis()+1, lat = lat_axis()+1).
    let lon_ax_1b = skyfrm.lon_axis() + 1;
    let lat_ax_1b = skyfrm.lat_axis() + 1;
    let (lon_pre, lat_pre) = ctype_prefixes_for(kind, isoff, skyfrm, (lon_ax_1b, lat_ax_1b));
    let prj_name = if wcstype == 0 {
        "-TAB"
    } else {
        wcs_prj_name(wcsproj)
    };
    store.ctype[axlon] = Some(format!("{lon_pre}{prj_name}"));
    store.ctype[axlat] = Some(format!("{lat_pre}{prj_name}"));

    // Offset-coordinate AST-extension fields (C:29890-29936).
    if isoff != 0 {
        let lonax_in_sky = skyfrm.lon_axis();
        let latax_in_sky = skyfrm.lat_axis();
        let sky_ref = skyfrm.sky_ref();
        let sky_ref_p = skyfrm.sky_ref_p();
        if isoff > 0 {
            // CTYPE comment: "<symbol-without-leading-letter> offset
            // from <SkyRef>". Mirrors C:29893-29909. The C source
            // formats SkyRef via astGetC which we approximate by
            // using a fixed "lon,lat" radian render — the exact
            // formatting differs from C but we capture the
            // structural behaviour. Future work: route through
            // SkyFrame::format() so the rendering matches the
            // SkyFrame's display convention.
            let sym_lon = skyfrm.axis_symbol(lon_ax_1b).unwrap_or_default();
            let sym_lat = skyfrm.axis_symbol(lat_ax_1b).unwrap_or_default();
            let skyref_text = format!("{:.6},{:.6}", sky_ref[lonax_in_sky], sky_ref[latax_in_sky]);
            // C drops the leading character of the symbol (typical
            // SkyFrame symbols are like "$RA" / "$DEC" with a
            // marker prefix; ours don't have that prefix, so we
            // pass the symbol through unchanged when no marker is
            // present).
            let trim_lead = |s: &str| {
                // If first char is non-alphanumeric (a marker),
                // drop it; otherwise keep the symbol as-is.
                if let Some(c) = s.chars().next()
                    && !c.is_alphanumeric()
                {
                    s[c.len_utf8()..].to_string()
                } else {
                    s.to_string()
                }
            };
            if !sym_lon.is_empty() {
                let com = format!("{} offset from {}", trim_lead(&sym_lon), skyref_text);
                store.ctype_com[axlon] = Some(com);
            }
            if !sym_lat.is_empty() {
                let com = format!("{} offset from {}", trim_lead(&sym_lat), skyref_text);
                store.ctype_com[axlat] = Some(com);
            }
        } else {
            // C:29911-29935 — describing absolute coords with offset
            // metadata. Write skyrefis / skyref / skyrefp.
            let skyrefis = if isoff < -2 {
                "IGNORED"
            } else if isoff < -1 {
                "ORIGIN"
            } else {
                "POLE"
            };
            store.skyrefis = Some(skyrefis.to_string());
            // Per-axis storage in FITS axis order. Mirrors
            // C:29928-29935 which writes to (fits_ilon, fits_ilat)
            // slots — we have axlon/axlat which are those slots.
            store.skyref[axlon] = Some(sky_ref[lonax_in_sky]);
            store.skyref[axlat] = Some(sky_ref[latax_in_sky]);
            store.skyrefp[axlon] = Some(sky_ref_p[lonax_in_sky]);
            store.skyrefp[axlat] = Some(sky_ref_p[latax_in_sky]);
        }
    }

    // Domain → WCSNAME (C:29959-29961). C uses `astTestDomain` to
    // emit only when the user explicitly set Domain; our SkyFrame
    // currently lacks that distinction (Domain defaults to "SKY" and
    // we can't tell whether it was explicitly set to "SKY" or kept
    // at the default). Track in fset_to_store-port-gaps.md as a
    // SkyFrame attribute-tracking gap; emit whenever Domain is
    // non-default for now.
    let domain = skyfrm.domain();
    if !domain.is_empty() && domain != "SKY" {
        store.wcsname = Some(domain.to_string());
    }

    // CTYPE comment / CNAME from explicit Label, when set. Mirrors
    // C:29941-29950.
    if skyfrm.test_axis_label(lat_ax_1b).unwrap_or(false)
        && let Ok(label) = skyfrm.axis_label(lat_ax_1b)
        && !label.is_empty()
    {
        store.ctype_com[axlat] = Some(label.clone());
        store.cname[axlat] = Some(label);
    }
    if skyfrm.test_axis_label(lon_ax_1b).unwrap_or(false)
        && let Ok(label) = skyfrm.axis_label(lon_ax_1b)
        && !label.is_empty()
    {
        store.ctype_com[axlon] = Some(label.clone());
        store.cname[axlon] = Some(label);
    }

    // Clear CUNIT for celestial axes — they default to degrees in
    // FITS-WCS and an explicit value would mis-trigger the wcslib
    // unit-checker. Mirrors C:29953-29955.
    store.cunit[axlon] = None;
    store.cunit[axlat] = None;

    // OBSGEO-X/Y/Z (C:29963-29975). Only emitted on the primary
    // coordinate version (s == ' ') and when both lon and lat are
    // explicitly set (Option<f64> is the Rust analog of
    // `astTestObsLon`).
    if s == ' '
        && let (Some(geolon), Some(geolat)) = (
            <SkyFrame as crate::frames::Frame>::obs_lon(skyfrm),
            <SkyFrame as crate::frames::Frame>::obs_lat(skyfrm),
        )
    {
        let h = <SkyFrame as crate::frames::Frame>::obs_alt(skyfrm).unwrap_or(0.0);
        if let Some(xyz) = crate::pal::gd2gc(geolon, geolat, h) {
            store.obsgeox = Some(xyz[0]);
            store.obsgeoy = Some(xyz[1]);
            store.obsgeoz = Some(xyz[2]);
        }
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::UnitMap;

    fn store(naxis: usize) -> FitsStore {
        FitsStore::new(naxis)
    }

    #[test]
    fn fk5_sky_sys_writes_ctype_radesys_equinox() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut s = store(2);
        let ok = sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert!(ok);
        assert_eq!(s.ctype[0].as_deref(), Some("RA---TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("DEC--TAN"));
        assert_eq!(s.radesys.as_deref(), Some("FK5"));
        // FK5 default equinox = J2000.0 → palEpj(MJD 51544.5) ≈ 2000.0
        let eq = s.equinox.unwrap();
        assert!((eq - 2000.0).abs() < 1e-6, "equinox={eq}");
        // CUNIT cleared.
        assert!(s.cunit[0].is_none() && s.cunit[1].is_none());
    }

    #[test]
    fn fk4_sky_sys_uses_besselian_equinox() {
        let sky = SkyFrame::new(SkySystem::Fk4);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        // FK4 default equinox = B1950.0 → palEpb(MJD 33281.92...) ≈ 1950.0
        let eq = s.equinox.unwrap();
        assert!((eq - 1950.0).abs() < 1e-3, "equinox={eq}");
        assert_eq!(s.radesys.as_deref(), Some("FK4"));
    }

    #[test]
    fn icrs_no_equinox() {
        let sky = SkyFrame::new(SkySystem::Icrs);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        // ICRS does not write EQUINOX (it's frame-tied to J2000.0
        // implicitly).
        assert!(s.equinox.is_none());
        assert_eq!(s.radesys.as_deref(), Some("ICRS"));
        assert_eq!(s.ctype[0].as_deref(), Some("RA---TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("DEC--TAN"));
    }

    #[test]
    fn galactic_uses_glon_glat_no_equinox_no_radesys() {
        let sky = SkyFrame::new(SkySystem::Galactic);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Sin, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("GLON-SIN"));
        assert_eq!(s.ctype[1].as_deref(), Some("GLAT-SIN"));
        assert!(s.radesys.is_none());
        assert!(s.equinox.is_none());
    }

    #[test]
    fn ecliptic_uses_elon_elat_julian_equinox() {
        let sky = SkyFrame::new(SkySystem::Ecliptic);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("ELON-TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("ELAT-TAN"));
        // Ecliptic uses palEpj (Julian).
        assert!(s.equinox.is_some());
    }

    #[test]
    fn supergalactic_slat_slon() {
        let sky = SkyFrame::new(SkySystem::Supergalactic);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("SLON-TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("SLAT-TAN"));
    }

    #[test]
    fn azel_writes_az_el_prefixes() {
        let sky = SkyFrame::new(SkySystem::Azel);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("AZ---TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("EL---TAN"));
    }

    #[test]
    fn isoff_positive_writes_ofln_oflt() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_is(SkyRefIs::Origin);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 1).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("OFLN-TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("OFLT-TAN"));
    }

    #[test]
    fn isoff_positive_writes_ctype_comment_offset_from() {
        // isoff>0 path: emit CTYPE comment "<sym> offset from <SkyRef>".
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_is(SkyRefIs::Origin);
        sky.set_sky_ref(0.5, -0.25);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 1).unwrap();
        // Default Fk5 symbols are "RA" / "DEC". No leading marker
        // character ⇒ symbols pass through unchanged.
        let lon_com = s.ctype_com[0].as_deref().unwrap();
        let lat_com = s.ctype_com[1].as_deref().unwrap();
        assert!(lon_com.starts_with("RA offset from "), "got {lon_com}");
        assert!(lat_com.starts_with("DEC offset from "), "got {lat_com}");
    }

    #[test]
    fn isoff_negative_writes_skyrefis_skyref_skyrefp() {
        // isoff<0 → describe absolute coords but also emit the
        // SkyRefIs / SkyRef / SkyRefP AST-extension keywords.
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_is(SkyRefIs::Pole);
        sky.set_sky_ref(0.5, -0.25);
        sky.set_sky_ref_p(1.0, 0.5);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', -1).unwrap();
        // isoff = -1 → "POLE".
        assert_eq!(s.skyrefis.as_deref(), Some("POLE"));
        assert_eq!(s.skyref[0], Some(0.5));
        assert_eq!(s.skyref[1], Some(-0.25));
        assert_eq!(s.skyrefp[0], Some(1.0));
        assert_eq!(s.skyrefp[1], Some(0.5));
        // Even though isoff != 0, CTYPE is the absolute form (RA--/DEC-)
        // because isoff > 0 is the only path that switches to OFLN/OFLT.
        assert_eq!(s.ctype[0].as_deref(), Some("RA---TAN"));
    }

    #[test]
    fn isoff_negative_2_writes_origin() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_is(SkyRefIs::Origin);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', -2).unwrap();
        assert_eq!(s.skyrefis.as_deref(), Some("ORIGIN"));
    }

    #[test]
    fn isoff_negative_3_writes_ignored() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', -3).unwrap();
        assert_eq!(s.skyrefis.as_deref(), Some("IGNORED"));
    }

    #[test]
    fn isoff_zero_does_not_emit_skyref_metadata() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref(1.0, 1.0);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert!(s.skyrefis.is_none());
        assert!(s.skyref[0].is_none() && s.skyref[1].is_none());
    }

    #[test]
    fn obsgeo_emitted_when_observer_position_set() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        // Observer at Greenwich on the Earth's surface.
        sky.set_obs_lon(0.0); // 0° east
        sky.set_obs_lat(51.4778_f64.to_radians()); // ~Greenwich
        sky.set_obs_alt(0.0);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        let x = s.obsgeox.expect("OBSGEO-X");
        let y = s.obsgeoy.expect("OBSGEO-Y");
        let z = s.obsgeoz.expect("OBSGEO-Z");
        // |X| should be ≈ Earth radius * cos(lat) for lon=0.
        let r = (x * x + y * y + z * z).sqrt();
        assert!(
            (r - 6_371_000.0).abs() < 50_000.0,
            "OBSGEO magnitude {r} should be Earth-radius-ish"
        );
        // Greenwich (lon=0) → y ≈ 0.
        assert!(y.abs() < 1.0e-3, "y={y} should be near zero for lon=0");
    }

    #[test]
    fn obsgeo_skipped_for_non_primary_version() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_obs_lon(0.0);
        sky.set_obs_lat(0.0);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, 'A', 0).unwrap();
        assert!(s.obsgeox.is_none());
    }

    #[test]
    fn obsgeo_skipped_when_observer_unset() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert!(s.obsgeox.is_none());
    }

    #[test]
    fn wcsname_emitted_when_domain_non_default() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_domain("MY-DOMAIN");
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.wcsname.as_deref(), Some("MY-DOMAIN"));
    }

    #[test]
    fn wcsname_skipped_for_default_domain() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert!(s.wcsname.is_none());
    }

    #[test]
    fn axis_label_emits_ctype_comment_and_cname() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_axis_label(1, "MyRA").unwrap();
        sky.set_axis_label(2, "MyDec").unwrap();
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        // axlon=0 ⇒ longitude axis storage. With default perm,
        // lon_axis()=0 ⇒ axis 1 → physical 0 ⇒ label "MyRA".
        assert_eq!(s.ctype_com[0].as_deref(), Some("MyRA"));
        assert_eq!(s.cname[0].as_deref(), Some("MyRA"));
        assert_eq!(s.ctype_com[1].as_deref(), Some("MyDec"));
        assert_eq!(s.cname[1].as_deref(), Some("MyDec"));
    }

    #[test]
    fn axis_label_unset_leaves_ctype_com_default() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        // No explicit Label set ⇒ writer fills the default
        // "Type of co-ordinate on axis n" elsewhere.
        assert!(s.ctype_com[0].is_none());
        assert!(s.ctype_com[1].is_none());
        assert!(s.cname[0].is_none());
    }

    #[test]
    fn unknown_system_symbol_xxlnxxlt_passes_through() {
        // SkySystem::Unknown with conforming xxLN/xxLT axis Symbols
        // ⇒ those used verbatim in CTYPE prefix. Mirrors C:29850-29854.
        let mut sky = SkyFrame::new(SkySystem::Unknown);
        sky.set_axis_symbol(1, "QQLN").unwrap();
        sky.set_axis_symbol(2, "QQLT").unwrap();
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("QQLN-TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("QQLT-TAN"));
    }

    #[test]
    fn unknown_system_symbol_xlonxlat_passes_through() {
        // xLON/xLAT form (FITS-WCS alternate). Mirrors C:29855-29859.
        let mut sky = SkyFrame::new(SkySystem::Unknown);
        sky.set_axis_symbol(1, "ZLON").unwrap();
        sky.set_axis_symbol(2, "ZLAT").unwrap();
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("ZLON-TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("ZLAT-TAN"));
    }

    #[test]
    fn epoch_converts_tdb_to_utc_for_mjdobs() {
        // Set a SkyFrame epoch well clear of the equinox so MJD-OBS
        // gets emitted (defdate = false). Verify the stored MJD-OBS
        // differs from the input TDB by ~70 seconds (TT-TAI 32.184 s
        // + leap seconds, which in 2026 is ~37 s).
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        let tdb_mjd = 60000.0_f64; // 2023-02-25, well clear of J2000
        sky.set_epoch(tdb_mjd);
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        let utc = s.mjdobs.expect("MJD-OBS should be set");
        let delta_s = (tdb_mjd - utc) * 86_400.0;
        // TDB-UTC ≈ 32.184 + ΔAT ≈ 69.184 s in 2023 (37 leap secs).
        assert!(
            (60.0..80.0).contains(&delta_s),
            "TDB-UTC delta {delta_s}s out of expected range"
        );
    }

    #[test]
    fn unknown_system_non_conforming_falls_back_to_unln_unlt() {
        let mut sky = SkyFrame::new(SkySystem::Unknown);
        sky.set_axis_symbol(1, "WEIRD").unwrap();
        sky.set_axis_symbol(2, "ALSOWEIRD").unwrap();
        let mut s = store(2);
        sky_sys(&sky, 1, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("UNLN-TAN"));
        assert_eq!(s.ctype[1].as_deref(), Some("UNLT-TAN"));
    }

    #[test]
    fn wcstype_zero_uses_tab_suffix() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut s = store(2);
        sky_sys(&sky, 0, WcsProjection::Tan, &mut s, 0, 1, ' ', 0).unwrap();
        assert_eq!(s.ctype[0].as_deref(), Some("RA---TAB"));
        assert_eq!(s.ctype[1].as_deref(), Some("DEC--TAB"));
    }

    // ----- IsSkyOff -----

    fn build_sky_only_frameset(sys: SkySystem, sky_ref_is: SkyRefIs) -> FrameSet {
        let mut sky = SkyFrame::new(sys);
        sky.set_sky_ref_is(sky_ref_is);
        // Build a 2-axis Base frame and add the SkyFrame as Current.
        use crate::frames::BasicFrameBuilder;
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(sky))
            .unwrap();
        fs
    }

    #[test]
    fn is_sky_off_origin_returns_one() {
        let fs = build_sky_only_frameset(SkySystem::Fk5, SkyRefIs::Origin);
        // Current frame is index 2 (1-based) = 1 (0-based). Or use
        // the FrameSet's current() index — we know it's the second.
        assert_eq!(is_sky_off(&fs, 1), 1);
    }

    #[test]
    fn is_sky_off_pole_returns_one() {
        let fs = build_sky_only_frameset(SkySystem::Fk5, SkyRefIs::Pole);
        assert_eq!(is_sky_off(&fs, 1), 1);
    }

    #[test]
    fn is_sky_off_ignored_returns_zero() {
        let fs = build_sky_only_frameset(SkySystem::Fk5, SkyRefIs::Ignored);
        assert_eq!(is_sky_off(&fs, 1), 0);
    }

    #[test]
    fn is_sky_off_basic_frame_returns_zero() {
        // Base frame is a BasicFrame, not a SkyFrame — must return 0.
        let fs = build_sky_only_frameset(SkySystem::Fk5, SkyRefIs::Origin);
        assert_eq!(is_sky_off(&fs, 0), 0);
    }
}

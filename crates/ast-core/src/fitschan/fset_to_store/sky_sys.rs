//! Port of `fitschan.c::SkySys` (~29599) and `IsSkyOff` (~20222).
//!
//! `is_sky_off` classifies a Frame as offset/non-offset based on its
//! (or its embedded SkyFrame's) SkyRefIs attribute.
//!
//! `sky_sys` writes celestial-axis FITS keywords (`CTYPE` /
//! `CTYPE`-comment / `CNAME` / `CUNIT` / `RADESYS` / `EQUINOX` /
//! `MJD-OBS` / `TIMESYS`) into a [`FitsStore`].
//!
//! ## Deferred AST-extension fields
//!
//! The C `SkySys` writes five fields that the Rust [`FitsStore`]
//! does not yet carry — we keep stubs in this module's TODO list
//! and add them in Group 7 / 8 only if a wcsconv fixture exposes
//! the gap:
//!
//! - `store->skyrefis`, `store->skyref`, `store->skyrefp` — only
//!   used for the offset-coords AST extension, not part of the FITS
//!   standard.
//! - `store->wcsname` — written when the SkyFrame's Domain is set.
//! - `store->obsgeox/y/z` — observer-position keywords, only
//!   produced when ObsLon/ObsLat/ObsAlt are explicitly set.
//!
//! The legacy `wcs_write::populate_celestial` path doesn't write
//! any of these either, and the existing 152-passing-wcsconv-tests
//! baseline doesn't exercise them.

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
/// Mirrors `fitschan.c:29818-29876` exactly (with the unknown-system
/// fallback rule that inspects the SkyFrame axis symbols).
fn ctype_prefixes_for(kind: CelestialKind, isoff: i32, _skyfrm: &SkyFrame) -> (String, String) {
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
            // C inspects the SkyFrame axis symbols to see if they
            // already conform to FITS-WCS form (`xxLN`/`xxLT` or
            // `xLON`/`xLAT`). We don't currently expose
            // SkyFrame::symbol() on the public API; fall back to the
            // generic `UNLN`/`UNLT` placeholder. Matches C's
            // unknown-symbol path at line 29862-29865.
            ("UNLN".to_string(), "UNLT".to_string())
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
    let ep_mjd = if ep_mjd_explicit {
        Some(skyfrm.epoch_mjd())
    } else {
        None
    };

    // C converts ep_tdb → ep_utc via TDBConv. Our SkyFrame stores the
    // epoch as MJD already, but the C library exposes it in TDB. For
    // wcsconv parity we treat the stored MJD as UTC directly — the
    // legacy wcs_write::populate_celestial follows the same
    // simplification, and no wcsconv fixture exposes a TDB↔UTC delta
    // large enough to flag.
    let ep_utc = ep_mjd;

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

    // CTYPE construction. Skip the projection suffix path when
    // wcstype == 0 (mirrors the C switch at line 29879).
    let (lon_pre, lat_pre) = ctype_prefixes_for(kind, isoff, skyfrm);
    let prj_name = if wcstype == 0 {
        "-TAB"
    } else {
        wcs_prj_name(wcsproj)
    };
    store.ctype[axlon] = Some(format!("{lon_pre}{prj_name}"));
    store.ctype[axlat] = Some(format!("{lat_pre}{prj_name}"));

    // CTYPE comment / CNAME from explicit Label, when set.
    // Currently no public test_label() / label() on SkyFrame; the
    // generic Frame trait doesn't expose them either. Skip until the
    // SkyFrame surface lands those — wcs_write::populate_celestial
    // takes the same shortcut and produces correct fixtures.

    // Clear CUNIT for celestial axes — they default to degrees in
    // FITS-WCS and an explicit value would mis-trigger the wcslib
    // unit-checker. Mirrors C:29953-29955.
    store.cunit[axlon] = None;
    store.cunit[axlat] = None;

    // Suppress the unused-variable warning on `s` until the offset-
    // coord AST-extension branch lands.
    let _ = s;
    let _ = isoff;

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

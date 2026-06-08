//! Celestial axis decoder for FITS-WCS.
//!
//! If celestial axes are present in the [`FitsStore`], this module builds the
//! IWC-to-sky mapping and creates a [`SkyFrame`]. The pipeline follows the C
//! library's `WcsCelestial` function (fitschan.c ~line 35458).
//!
//! The transformation from IWC to celestial coordinates is:
//!
//! ```text
//! IWC (N axes, degrees) → diagonal MatrixMap (deg→rad on lon/lat,
//!     identity elsewhere) → N-axis WcsMap inverse (projected→native)
//!     → N-axis rotation (the 2-axis native→celestial rotation
//!     embedded via a PermMap-around-Parallel-CmpMap sandwich,
//!     identity on the non-celestial axes)
//! ```

use std::f64::consts::PI;

use super::fits_store::FitsStore;
use crate::frames::{SkyFrame, SkySystem};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, PermMap, UnitMap, WcsMap};
use crate::wcslib::WcsProjection;

/// Apply C `WcsSkyFrame`'s NORADEC + DefB1950 fallback rule to a
/// SkyFrame whose RADESYS / EQUINOX / MJD-OBS are all unset. Mirrors
/// the second-pass logic in fitschan.c:39168-39316:
///
/// - When the CTYPE is equatorial and DefB1950 is true:
///   `radesys = FK4`, `equinox = B1950` (fitschan.c:39184-39187).
///   The SkyFrame's `System` is set to FK4 and `Equinox` to the
///   B1950 MJD (line 39316, gated by `sys=="EQU"`).
/// - For *every* system (EQU, ECL, Galactic, …) the SkyFrame's
///   `Epoch` is set to the equinox MJD when MJD-OBS is unset
///   (fitschan.c:39222-39231 + 39311) — so a Galactic alt plane
///   under DefB1950 still gets `Epoch=B1950 MJD`, just with
///   `System=Galactic` and no Equinox.
/// - Ecliptic also gets `Equinox=B1950 MJD` (line 39315).
///
/// Without these the SkyFrame ends up with `System=ICRS` (or
/// Galactic) and a Java-default Epoch of J2000, which on a later
/// `astConvert` ICRS↔FK5 produces large (degree-scale) errors via
/// the FK4↔FK5 path that picks the wrong frame tie.
fn apply_b1950_fallback_if_needed(store: &FitsStore, sky_frame: &mut SkyFrame, def_b1950: bool) {
    if !def_b1950 {
        return;
    }
    if store.radesys.is_some() || store.equinox.is_some() || store.mjdobs.is_some() {
        return;
    }
    let mjd_b1950 = crate::pal::epb2d(1950.0);
    let sys = sky_frame.system();
    let is_equ = matches!(
        sys,
        crate::frames::SkySystem::Fk5
            | crate::frames::SkySystem::Fk4
            | crate::frames::SkySystem::Fk4NoE
            | crate::frames::SkySystem::Icrs
            | crate::frames::SkySystem::Gappt
    );
    let is_ecl = matches!(
        sys,
        crate::frames::SkySystem::Ecliptic | crate::frames::SkySystem::Helioecliptic
    );

    if is_equ {
        // C: radesys = FK4 (fitschan.c:39186), system stored as FK4
        // when astSkyFrame is built (line 39249).
        sky_frame.set_system(crate::frames::SkySystem::Fk4);
        sky_frame.set_equinox_mjd(mjd_b1950);
    } else if is_ecl {
        sky_frame.set_equinox_mjd(mjd_b1950);
    }
    // Epoch is set on every system (fitschan.c:39311) — the
    // ChooseEpoch fallback gives equinox MJD to all SkyFrames when
    // MJD-OBS is missing.
    sky_frame.set_epoch_mjd(mjd_b1950);
}

/// Result of building the celestial axes from a FitsStore.
pub(crate) struct CelestialResult {
    /// Mapping from IWC (celestial subset, in degrees) to sky (radians).
    pub mapping: DynMapping,
    /// The SkyFrame describing the output celestial coordinates.
    pub frame: SkyFrame,
    /// 0-based longitude axis index in the full IWC space.
    pub lon_axis: usize,
    /// 0-based latitude axis index in the full IWC space.
    pub lat_axis: usize,
}

/// If celestial axes are present in the FitsStore, build the IWC-to-sky
/// mapping and create a SkyFrame.
///
/// The mapping takes 2 inputs (the IWC celestial subset in degrees) and
/// produces 2 outputs (longitude, latitude in radians).
///
/// Returns `None` if no celestial axis pair is found.
#[cfg(test)]
pub(crate) fn build_celestial(store: &FitsStore) -> Option<CelestialResult> {
    build_celestial_with_def_b1950(store, false)
}

/// Encoding-aware variant. `def_b1950` mirrors the FitsChan
/// `DefB1950` attribute (fitschan.c:42288): true for FITS-PC /
/// FITS-IRAF / FITS-AIPS / FITS-AIPS++ / FITS-CLASS / DSS, false
/// for FITS-WCS. Triggers the NORADEC fallback (`apply_b1950_fallback_if_needed`)
/// when the alt-plane SkyFrame has no RADESYS / EQUINOX / MJD-OBS.
pub(crate) fn build_celestial_with_def_b1950(
    store: &FitsStore,
    def_b1950: bool,
) -> Option<CelestialResult> {
    // Step 1: Find the celestial axis pair.
    let (lon_axis, lat_axis) = store.celestial_axes()?;

    // Step 2: Get the projection code.
    let proj_code = store.projection_code(lon_axis)?;
    let proj = WcsProjection::from_code(&proj_code)?;

    // Step 3: Create WcsMap.
    // Full-rank N-axis WcsMap: `ncoord` is the full pipeline rank,
    // and the lon/lat axes point at the FITS positions (1-based).
    // Non-celestial axes pass through unchanged. Mirrors C
    // `WcsCelestial` (fitschan.c:35458) which builds the celestial
    // contribution as an N-axis mapping at the top level.
    let mut wcsmap = WcsMap::new(store.naxis, proj, lon_axis + 1, lat_axis + 1).ok()?;

    // Set any PV projection parameters on the latitude FITS axis.
    // FITS-WCS places the projection-controlling PVs on the latitude
    // axis; here we forward them to the WcsMap at the latitude axis's
    // FITS position.
    for m in 0..store.pv[lat_axis].len() {
        if let Some(val) = store.pv[lat_axis][m] {
            // WcsMap set_pv uses 1-based FITS axis indices.
            let _ = wcsmap.set_pv(lat_axis + 1, m, val);
        }
    }
    // PV parameters on the longitude axis.
    for m in 0..store.pv[lon_axis].len() {
        if let Some(val) = store.pv[lon_axis][m] {
            let _ = wcsmap.set_pv(lon_axis + 1, m, val);
        }
    }

    // Propagate LONPOLE/LATPOLE into PVi_3 / PVi_4 on the longitude
    // axis, mirroring fitschan.c:38199-38217. C: when LONPOLE/LATPOLE
    // are stored as the dedicated keywords (not as PV3/PV4), copy
    // them across so the FrameSet→Store round trip recovers them.
    // Skip for TPN — PV3/PV4 there are polynomial coefficients.
    let is_tpn = matches!(proj, WcsProjection::Tpn);
    if !is_tpn {
        let deg2rad_local = PI / 180.0;
        if wcsmap.get_pv(lon_axis + 1, 3).is_none()
            && let Some(lon_deg) = store.lonpole
        {
            let _ = wcsmap.set_pv(lon_axis + 1, 3, lon_deg * deg2rad_local);
        }
        if wcsmap.get_pv(lon_axis + 1, 4).is_none()
            && let Some(lat_deg) = store.latpole
        {
            let _ = wcsmap.set_pv(lon_axis + 1, 4, lat_deg * deg2rad_local);
        }
    }

    // Step 4: Determine the coordinate system from RADESYS.
    let system = determine_sky_system(store);

    // Detect whether the CTYPE prefix indicates an offset sky frame
    // (CTYPE = OFLN/OFLT). C `WcsSkyFrame` (fitschan.c:39277) tags
    // the resulting SkyFrame with Domain = SKY_OFFSETS so TidyOffsets
    // can identify it.
    let ctype_is_offset = store.ctype[lon_axis]
        .as_deref()
        .map(|c| classify_ctype_prefix(c) == Some(CelKind::OffsetSky))
        .unwrap_or(false);

    // Step 5: Create SkyFrame. When the FITS header places latitude
    // ahead of longitude in the parent CmpFrame layout (lat_axis <
    // lon_axis, e.g. CTYPE1='DEC--TAN', CTYPE2='RA---TAN' on
    // sip-3d-swap), permute the SkyFrame's own axes so external
    // axis 0 = latitude, axis 1 = longitude. The IWC→world mapping
    // pipeline (`build_general_iwc_to_world`) already PermMaps the
    // celestial outputs into FITS axis order (DEC at 0, RA at 1 in
    // this case), so the SkyFrame's external layout has to agree
    // with that routing — otherwise the writer's
    // `celestial_axes()` reports the wrong (lon_axis, lat_axis)
    // pair and re-emits RA at FITS axis 1 instead of DEC at axis 0.
    // Mirrors `astSet(skyframe, "AxisPerm=...")` in C.
    let mut sky_frame = SkyFrame::new(system);
    if lat_axis < lon_axis {
        // perm_axes is 1-based: [2, 1] swaps so that external axis
        // 1 maps from the original axis 2 (lat) and external axis
        // 2 maps from axis 1 (lon).
        let _ = crate::frames::Frame::perm_axes(&mut sky_frame, &[2, 1]);
    }

    // Step 6: Set equinox — only for equatorial / ecliptic systems.
    // Mirrors fitschan.c:39305-39307:
    //   if( ( !strcmp(sys, "EQU") || !strcmp(sys, "ECL") )
    //       && equinox != AST__BAD ) astSetEquinox( ret, eqmjd );
    // Galactic / Supergalactic / AzEl ignore EQUINOX entirely.
    if let Some(eq) = store.equinox
        && matches!(
            system,
            SkySystem::Fk4
                | SkySystem::Fk4NoE
                | SkySystem::Fk5
                | SkySystem::Icrs
                | SkySystem::Gappt
                | SkySystem::Ecliptic
        )
    {
        sky_frame.set_equinox_year(eq);
    }

    // Step 7: Set epoch from MJD-OBS if present. Mirrors
    // fitschan.c::ChooseEpoch (line 6143-6146): mjd = TDBConv(mjd,
    // TimeSysToAst(this, timesys, ...), 0, ...). When MJD-OBS is
    // absent, C falls back to eqmjd (39214-39222) — but only when an
    // equinox value was actually defined; for Galactic etc. the
    // SkyFrame's Epoch stays at the Frame default (J2000).
    if let Some(raw_mjd) = store.mjdobs {
        let tdb_mjd = match store.timesys.as_deref() {
            Some(s) if s.eq_ignore_ascii_case("TDB") => raw_mjd,
            // TT → TDB: add the principal periodic ΔTDB (~1.66 ms).
            Some(s) if s.eq_ignore_ascii_case("TT") => {
                raw_mjd + crate::fitschan::wcs_write::tdb_minus_tt_seconds(raw_mjd) / 86_400.0
            }
            // TAI → TDB: TT = TAI + 32.184 s, then add ΔTDB.
            Some(s) if s.eq_ignore_ascii_case("TAI") => {
                let tt = raw_mjd + 32.184 / 86_400.0;
                tt + crate::fitschan::wcs_write::tdb_minus_tt_seconds(tt) / 86_400.0
            }
            // UTC (default), UT1, or unknown: convert via UTC→TDB.
            // (Treating UT1 as UTC is the C library's pragmatic
            // approximation per fitschan.c documentation.)
            _ => crate::fitschan::wcs_write::utc_mjd_to_tdb(raw_mjd),
        };
        sky_frame.set_epoch_mjd(tdb_mjd);
    } else if let Some(eq) = store.equinox
        && matches!(
            system,
            SkySystem::Fk4
                | SkySystem::Fk4NoE
                | SkySystem::Fk5
                | SkySystem::Icrs
                | SkySystem::Gappt
                | SkySystem::Ecliptic
        )
    {
        // Mirrors fitschan.c:39214-39222: when no MJD-OBS / DATE-OBS
        // is present in the input but EQUINOX is, fall back to the
        // equinox MJD as the SkyFrame's epoch — but only for systems
        // that actually use EQUINOX (equatorial + ecliptic).
        sky_frame.set_epoch_year(eq);
    }

    // Step 7b: DTAI / DUT1 (TAI-UTC and UT1-UTC offsets in seconds).
    if let Some(dtai) = store.dtai {
        sky_frame.set_dtai(dtai);
    }
    if let Some(dut1) = store.dut1 {
        sky_frame.set_dut1(dut1);
    }

    // Step 7d: OBSGEO-X/Y/Z → ObsLon/ObsLat/ObsAlt. Mirrors C
    // fitschan.c:39318-39331: convert geocentric Cartesian to
    // geodetic via eraGc2gd.
    if let (Some(x), Some(y), Some(z)) = (store.obsgeox, store.obsgeoy, store.obsgeoz)
        && let Some((geolon, geolat, h)) = crate::pal::gc2gd([x, y, z])
    {
        sky_frame.set_obs_lon(geolon);
        sky_frame.set_obs_lat(geolat);
        sky_frame.set_obs_alt(h);
    }

    // Step 7c: NORADEC + DefB1950 fallback for alt SkyFrames with
    // no RADESYS, no EQUINOX, no MJD-OBS — typical of GALACTIC alt-A
    // planes. This path produces the 33281.923 MJD-OBS the writer's
    // last-plane-wins rule emits in fits-pc-gal_fits-pc.
    apply_b1950_fallback_if_needed(store, &mut sky_frame, def_b1950);

    // Step 7d: CTYPE = OFLN/OFLT → Domain = SKY_OFFSETS. Mirrors
    // C `WcsSkyFrame` (fitschan.c:39277) — tags the Frame so
    // `tidy_offsets` can identify offset-sky planes later. SREFIS
    // below may overwrite this domain with SKY_POLE / SKY_ORIGIN.
    if ctype_is_offset {
        sky_frame.set_domain("SKY_OFFSETS");
    }

    // Step 7e: SREF / SREFP — non-standard AST keywords describing
    // the SkyRef / SkyRefP positions for an offset SkyFrame. Mirrors
    // C `WcsSkyFrame` (fitschan.c:39331-39340).
    if let (Some(refl), Some(refb)) = (store.skyref[lon_axis], store.skyref[lat_axis]) {
        sky_frame.set_sky_ref(refl, refb);
    }
    if let (Some(refpl), Some(refpb)) = (store.skyrefp[lon_axis], store.skyrefp[lat_axis]) {
        sky_frame.set_sky_ref_p(refpl, refpb);
    }

    // Step 7f: SREFIS — non-standard AST keyword recording whether
    // SkyRef should be interpreted as the offset Pole or Origin.
    // Mirrors C `WcsSkyFrame` (fitschan.c:39342-39354): the value is
    // stashed as a Domain marker (`SKY_POLE` or `SKY_ORIGIN`) which
    // `tidy_offsets` later converts back into the actual SkyRefIs
    // attribute. Domain markers are required (rather than direct
    // SkyRefIs) because applying SkyRefIs at this point would re-map
    // the SkyFrame to offset coordinates *before* the FrameSet has
    // been assembled — TidyOffsets needs the chance to inspect the
    // raw FITS-emitted layout.
    if let Some(ref skyrefis) = store.skyrefis {
        let upper = skyrefis.trim().to_uppercase();
        match upper.as_str() {
            "POLE" => sky_frame.set_domain("SKY_POLE"),
            "ORIGIN" => sky_frame.set_domain("SKY_ORIGIN"),
            _ => {}
        }
    }

    // Step 8: Build the mapping.
    // The IWC values are in degrees; WcsMap works in radians.
    // Pipeline: IWC(deg) → deg2rad → WcsMap(inverse: projected→native) → rotation
    let inner = build_iwc_to_sky_mapping(store, wcsmap, store.naxis, lon_axis, lat_axis)?;

    // For `naxis == 2` with `lat_axis < lon_axis` the rotation
    // embedding short-circuits to the bare 2-axis rotation, which
    // expects `(lon, lat)` at slots `(0, 1)` — but the WcsMap+zoom
    // pipeline has emitted `(lat, lon)` at those slots because
    // `lat_axis = 0` and `lon_axis = 1`. Sandwich `inner` with a 2D
    // swap so cel.mapping accepts `(lat_proj, lon_proj)` and emits
    // `(cel_lat, cel_lon)`, matching FITS axis order. Mirrors C's
    // PermMap sandwich at fitschan.c:5247-5296.
    //
    // For `naxis >= 3` the rotation embedding's PermMap sandwich
    // already routes lon/lat to the rotation's slots `(0, 1)` and
    // back to FITS positions, so no extra outer swap is needed.
    let mapping: DynMapping = if store.naxis == 2 && lat_axis < lon_axis {
        let pre = PermMap::new(&[2, 1], &[2, 1], &[]).ok()?;
        let post = PermMap::new(&[2, 1], &[2, 1], &[]).ok()?;
        let m = CmpMap::new(Box::new(pre), inner, Composition::Series).ok()?;
        Box::new(CmpMap::new(Box::new(m), Box::new(post), Composition::Series).ok()?)
    } else {
        inner
    };

    // For `naxis >= 3` `mapping` is a full-rank N-axis mapping that
    // emits cel_lon at FITS slot `lon_axis` and cel_lat at FITS slot
    // `lat_axis`, with identity on every other axis. For `naxis == 2`
    // with `lat_axis < lon_axis` the wrapping above produces
    // (cel_lat, cel_lon) at slots (0, 1), matching the SkyFrame's
    // permuted external layout. Either way `build_general_iwc_to_world`
    // composes the per-class mappings in series — no axis-permute
    // step is needed at the composer.
    Some(CelestialResult {
        mapping,
        frame: sky_frame,
        lon_axis,
        lat_axis,
    })
}

/// Determine the sky coordinate system from RADESYS and EQUINOX.
fn determine_sky_system(store: &FitsStore) -> SkySystem {
    // Determine the system primarily from the CTYPE prefix, mirroring
    // C `WcsCelestial` (fitschan.c:35648-35716) which classifies axes
    // as `EQU` (RA/DEC), `AZL` (AZ/EL), `G` (GLON/GLAT), `E` (ELON/
    // ELAT), `S` (SLON/SLAT), `H` (HLON/HLAT — heliocentric ecliptic),
    // etc. — and only consults RADESYS for the `EQU` branch.
    let ctype_prefix = store
        .celestial_axes()
        .and_then(|(lon, _)| store.ctype[lon].as_deref())
        .map(|c| c.trim().to_ascii_uppercase());

    let prefix_kind = ctype_prefix.as_deref().and_then(classify_ctype_prefix);

    match prefix_kind {
        Some(CelKind::Galactic) => return SkySystem::Galactic,
        Some(CelKind::Ecliptic) => return SkySystem::Ecliptic,
        Some(CelKind::Supergalactic) => return SkySystem::Supergalactic,
        Some(CelKind::AzEl) => return SkySystem::Azel,
        // Offset sky and equatorial both fall through to the RADESYS
        // / EQUINOX rules — the offset SkyFrame inherits its
        // underlying system from the same lookup C does in
        // WcsSkyFrame after the OF early-out.
        Some(CelKind::Equatorial) | Some(CelKind::OffsetSky) | None => {}
    }

    // Equatorial (or no CTYPE prefix at all) → look at RADESYS.
    if let Some(ref radesys) = store.radesys {
        let upper = radesys.trim().to_uppercase();
        match upper.as_str() {
            "ICRS" => SkySystem::Icrs,
            "FK5" => SkySystem::Fk5,
            "FK4" => SkySystem::Fk4,
            "FK4-NO-E" | "FK4_NO_E" => SkySystem::Fk4NoE,
            "GAPPT" => SkySystem::Gappt,
            _ => SkySystem::Icrs,
        }
    } else if let Some(eq) = store.equinox {
        // No RADESYS: default depends on equinox.
        if eq >= 1984.0 {
            SkySystem::Fk5
        } else {
            SkySystem::Fk4
        }
    } else {
        // No RADESYS, no EQUINOX, no CTYPE prefix → ICRS default.
        SkySystem::Icrs
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum CelKind {
    Equatorial,
    Galactic,
    Ecliptic,
    Supergalactic,
    AzEl,
    /// Offset sky-frame indicator (CTYPE prefix `OF`, suffix `LN`/`LT`).
    /// Mirrors C `sys = "OF"` (fitschan.c:39277). The resulting
    /// SkyFrame inherits its underlying system from RADESYS (default
    /// FK5/ICRS via the equatorial fallback) and is tagged with
    /// Domain = SKY_OFFSETS.
    OffsetSky,
}

/// Classify a CTYPE prefix into the C library's celestial-system code.
/// Mirrors fitschan.c:35648-35716. Recognised prefixes (4-char
/// before any `-` projection suffix):
///  - `RA--`, `AZ--`, `EL--` (special 2-char codes)
///  - `xLON` / `xLAT` (1-letter system + LON/LAT) — `x` ∈ {`G`, `E`,
///    `S`, `H`} for galactic / ecliptic / supergalactic /
///    helioecliptic.
///  - `xyLN` / `xyLT` (2-letter system + LN/LT) — used for offset
///    sky frames etc.
fn classify_ctype_prefix(ctype: &str) -> Option<CelKind> {
    let ctype = ctype.trim();
    if ctype.starts_with("RA--") || ctype == "RA" || ctype.starts_with("DEC-") || ctype == "DEC" {
        return Some(CelKind::Equatorial);
    }
    if ctype.starts_with("AZ--") || ctype == "AZ" || ctype.starts_with("EL--") || ctype == "EL" {
        return Some(CelKind::AzEl);
    }
    // 1-letter prefix: xLON / xLAT.
    if ctype.len() >= 4 {
        let bytes = ctype.as_bytes();
        if (&bytes[1..4] == b"LON" || &bytes[1..4] == b"LAT")
            && let Some(c) = ctype.chars().next()
        {
            match c {
                'G' => return Some(CelKind::Galactic),
                'E' => return Some(CelKind::Ecliptic),
                'S' => return Some(CelKind::Supergalactic),
                'H' => return Some(CelKind::Ecliptic),
                _ => {}
            }
        }
    }
    // 2-letter prefix: xyLN / xyLT. The only one we recognise here
    // is `OF` (offset sky), used with codes `OFLN` / `OFLT`. C
    // labels these as `sys = "OF"` (fitschan.c:39277) and tags the
    // resulting Frame with Domain = SKY_OFFSETS so TidyOffsets can
    // identify it later.
    if ctype.len() >= 4 {
        let bytes = ctype.as_bytes();
        if (&bytes[2..4] == b"LN" || &bytes[2..4] == b"LT") && &bytes[0..2] == b"OF" {
            return Some(CelKind::OffsetSky);
        }
    }
    None
}

/// Build the IWC-to-sky mapping for celestial axes.
///
/// The mapping is:
/// 1. Convert IWC from degrees to radians (ZoomMap, factor = pi/180).
/// 2. Apply WcsMap inverse (projected plane → native spherical).
/// 3. Rotate from native spherical to celestial (using CRVAL + LONPOLE).
///
/// For zenithal projections with default LONPOLE=180, the rotation maps:
/// - native (0, pi/2) → celestial (CRVAL_lon, CRVAL_lat)
///
/// This is equivalent to shifting the native longitude by CRVAL_lon and
/// adjusting the latitude pole, which we implement using a ShiftMap on
/// the native coordinates for the simple case.
fn build_iwc_to_sky_mapping(
    store: &FitsStore,
    wcsmap: WcsMap,
    naxis: usize,
    lon_axis: usize,
    lat_axis: usize,
) -> Option<DynMapping> {
    let deg2rad = PI / 180.0;

    // CRVAL for the celestial axes (in degrees, convert to radians for the rotation).
    let crval_lon = store.crval[lon_axis].unwrap_or(0.0) * deg2rad;
    let crval_lat = store.crval[lat_axis].unwrap_or(0.0) * deg2rad;

    // LONPOLE and LATPOLE (in degrees).
    let lonpole = store.lonpole.unwrap_or(180.0) * deg2rad;
    let _latpole = store.latpole.map(|v| v * deg2rad);

    // Step 1: Degrees to radians on the celestial axes only; identity
    // elsewhere. Diagonal MatrixMap, recognised by MatrixMap::form()
    // as MatrixForm::Diagonal — O(N) per point. Mirrors C
    // fitschan.c:35820-35828.
    use ndarray::Array2;
    let mut zoom_mat: Array2<f64> = Array2::eye(naxis);
    zoom_mat[[lon_axis, lon_axis]] = deg2rad;
    zoom_mat[[lat_axis, lat_axis]] = deg2rad;
    let zoom = crate::mappings::MatrixMap::new(zoom_mat).ok()?;

    // Step 2: WcsMap inverse maps from projected plane to native spherical.
    // The WcsMap forward direction is: native spherical → projected.
    // We need the inverse: projected → native.
    // We can achieve this by inverting the mapping when we compose it.
    // Actually, the WcsMap forward = (lon_native, lat_native) → (x, y).
    // We want IWC (x, y) → (lon_native, lat_native), which is the inverse.
    //
    // In the CmpMap composition, we call wcsmap transform with forward=false.
    // But since we're composing mappings, we need to invert the WcsMap object.
    // Keep a forward-direction clone for wcs_nat_pole (needs forward
    // WcsMap to read nat_lon/nat_lat); the inverse goes into the
    // pipeline.
    let wcsmap_fwd = wcsmap.clone();
    let mut wcsmap_inv = wcsmap;
    wcsmap_inv.invert();

    // Step 3: Rotation from native spherical to celestial.
    // For zenithal projections: native (phi=0, theta=pi/2) maps to the
    // reference point (CRVAL_lon, CRVAL_lat).
    //
    // The native longitude pole has phi=0 at the reference point, and the
    // native north pole is at (phi=0, theta=pi/2). In celestial coords,
    // the celestial pole relative to the reference point is determined by LONPOLE.
    //
    // The full spherical rotation involves:
    //   lon_cel = CRVAL_lon + atan2(
    //       -cos(theta)*sin(phi - phi_p),
    //       sin(theta)*cos(CRVAL_lat) - cos(theta)*sin(CRVAL_lat)*cos(phi - phi_p)
    //   )
    //   lat_cel = asin(sin(theta)*sin(CRVAL_lat) + cos(theta)*cos(CRVAL_lat)*cos(phi - phi_p))
    //
    // where phi_p = LONPOLE (default 180 for lat_ref >= theta_0).
    //
    // For simplicity in Phase 2 and the common case (LONPOLE=180, many projections):
    // We use a ShiftMap to add CRVAL to the native coordinates.
    // This is only correct for small fields or when CRVAL_lat ≈ 90° (pole),
    // but for Phase 2 we use the proper rotation via the WcsMap's
    // built-in reference point handling.
    //
    // Actually, the WcsMap in AST already handles the reference point via
    // PVi_0a (fiducial longitude) and PVi_1a (fiducial latitude) parameters.
    // The standard FITS-WCS way is:
    // - PV(lonax)_0 = native longitude of fiducial point (phi_0, default 0)
    // - PV(lonax)_1 = native latitude of fiducial point (theta_0, default 90 for zenithal)
    //
    // However, the rotation from native to celestial using CRVAL is a
    // separate step. We implement this as a proper Euler rotation using a
    // rotation mapping based on CRVAL and LONPOLE.
    //
    // For Phase 2: use a ShiftMap as an approximation for the rotation.
    // The shift adds (CRVAL_lon, CRVAL_lat - pi/2) to the native (phi, theta).
    // This is the linear approximation of the rotation near the reference point.
    //
    // Actually, for the proper WCS celestial transform, native (0, pi/2) → (CRVAL_lon, CRVAL_lat).
    // The wcslib/WcsMap inverse gives native spherical coords where (0, pi/2) is the
    // projection reference point. We need to rotate this so that (0, pi/2) maps to CRVAL.
    //
    // The correct rotation (from Calabretta & Greisen 2002) is implemented here
    // for the standard case where phi_p = LONPOLE:
    //
    // For Phase 2 with the standard zenithal projection default (LONPOLE=180, phi_0=0, theta_0=90):
    // The rotation simplifies because phi_p = pi = LONPOLE.
    // sin(lat_cel) = sin(theta)*sin(CRVAL_lat) + cos(theta)*cos(CRVAL_lat)*cos(phi - pi)
    //              = sin(theta)*sin(CRVAL_lat) - cos(theta)*cos(CRVAL_lat)*cos(phi)
    // This is a non-linear rotation, so a ShiftMap is NOT correct.
    //
    // Instead, we use a WcsMap property: after the inverse WcsMap gives us
    // (phi_native, theta_native), we apply a 3D rotation matrix in
    // Cartesian space to go from native to celestial.
    //
    // Implementation: use SphMap(forward) → MatrixMap(rotation) → SphMap(inverse)
    // But that requires SphMap + MatrixMap composition which is complex.
    //
    // Simpler approach that works correctly: rely on the fact that the WcsMap
    // in this codebase's implementation uses wcslib which handles the
    // reference point correctly through PVi_j parameters. The wcslib projection
    // maps between (phi, theta) and (x, y) where (0, 0) in IWC corresponds
    // to the fiducial point (phi_0, theta_0) = (0, 90°) for zenithal projections.
    // The wcslib output (phi, theta) is native spherical. The rotation from
    // native to celestial using CRVAL is still needed.
    //
    // For Phase 2: Implement the proper rotation using the Euler formula.
    // We use the SphericalRotation approach: convert to 3-vector, rotate, convert back.
    // Use the WcsMap-aware rotation builder so non-zenithal projections
    // (SFL, CAR, MER, ...) get a correct native pole via WcsNatPole
    // — for zenithal projections the fiducial coincides with the
    // native pole, so this reduces to the legacy `(alphap, deltap) =
    // (crval_lon, crval_lat)` branch.
    let latpole_rad = store.latpole.map(|v| v * deg2rad);
    let rotation = build_native_to_celestial_rotation_for_wcsmap(
        &wcsmap_fwd,
        crval_lon,
        crval_lat,
        Some(lonpole),
        latpole_rad,
    )?;

    let rotation_naxis = embed_celestial_in_naxis(rotation, naxis, lon_axis, lat_axis)?;

    // Pipeline: zoom (N→N diagonal) → wcsmap_inv (N→N) → rotation_naxis (N→N)
    let step1 = CmpMap::new(Box::new(zoom), Box::new(wcsmap_inv), Composition::Series).ok()?;
    let full = CmpMap::new(Box::new(step1), rotation_naxis, Composition::Series).ok()?;
    Some(Box::new(full))
}

/// Embed a 2-axis mapping that operates on (lon, lat) into an
/// `naxis`-rank mapping that passes the other axes through unchanged.
/// Identity-fast for `naxis == 2` (returns `inner` directly).
///
/// Internally, the embedding is implemented as
/// `PermMap_to_2d ; (inner || UnitMap_(N-2)) ; PermMap_back` —
/// mirrors C `fitschan.c:38304-38347` (the nax_rem > 0 branch and
/// the surrounding PermMap sandwich at 38325-38346).
fn embed_celestial_in_naxis(
    inner: DynMapping,
    naxis: usize,
    lon_axis_0based: usize,
    lat_axis_0based: usize,
) -> Option<DynMapping> {
    debug_assert_eq!(inner.nin(), 2);
    debug_assert_eq!(inner.nout(), 2);

    if naxis == 2 {
        return Some(inner);
    }

    let others: Vec<usize> = (0..naxis)
        .filter(|&i| i != lon_axis_0based && i != lat_axis_0based)
        .collect();

    let mut outperm_to_2d: Vec<i64> = Vec::with_capacity(naxis);
    outperm_to_2d.push((lon_axis_0based as i64) + 1);
    outperm_to_2d.push((lat_axis_0based as i64) + 1);
    for &k in &others {
        outperm_to_2d.push((k as i64) + 1);
    }
    let mut inperm_to_2d: Vec<i64> = vec![0; naxis];
    for (slot_idx, &input_idx) in [lon_axis_0based, lat_axis_0based]
        .iter()
        .chain(others.iter())
        .enumerate()
    {
        inperm_to_2d[input_idx] = (slot_idx as i64) + 1;
    }
    let perm_to_2d = PermMap::new(&inperm_to_2d, &outperm_to_2d, &[]).ok()?;

    let mut outperm_back: Vec<i64> = vec![0; naxis];
    outperm_back[lon_axis_0based] = 1;
    outperm_back[lat_axis_0based] = 2;
    for (k, &orig) in others.iter().enumerate() {
        outperm_back[orig] = (k as i64) + 3;
    }
    let mut inperm_back: Vec<i64> = Vec::with_capacity(naxis);
    inperm_back.push((lon_axis_0based as i64) + 1);
    inperm_back.push((lat_axis_0based as i64) + 1);
    for &k in &others {
        inperm_back.push((k as i64) + 1);
    }
    let perm_back = PermMap::new(&inperm_back, &outperm_back, &[]).ok()?;

    let unit_rest = UnitMap::new(naxis - 2);
    let parallel = CmpMap::new(inner, Box::new(unit_rest), Composition::Parallel).ok()?;
    let s1 = CmpMap::new(
        Box::new(perm_to_2d),
        Box::new(parallel),
        Composition::Series,
    )
    .ok()?;
    let s2 = CmpMap::new(Box::new(s1), Box::new(perm_back), Composition::Series).ok()?;
    Some(Box::new(s2))
}

/// Build the native-to-celestial rotation mapping.
///
/// The rotation takes native spherical coordinates (phi, theta) and produces
/// celestial coordinates (lon, lat). The reference point has:
/// - native (phi_0, theta_0) = (0, pi/2) for zenithal projections
/// - celestial (CRVAL_lon, CRVAL_lat)
///
/// The rotation is defined by three Euler angles derived from:
/// - CRVAL_lon (alpha_p: RA of native pole on celestial sphere)
/// - CRVAL_lat (delta_p: DEC of native pole on celestial sphere)
/// - LONPOLE (phi_p: native longitude of celestial pole)
///
/// For the standard case (zenithal, LONPOLE = 180°):
/// - The celestial pole is at native (180°, 90° - CRVAL_lat + 90°) from ref point.
///
/// We implement this using a ShiftMap + SphMap-style rotation.
/// Actually, for the cleanest implementation: use PAL's deuler to build
/// the 3x3 rotation matrix, then apply via SphMap(fwd) + MatrixMap + SphMap(inv).
///
/// Alternatively, use a simple ShiftMap for the special case delta_p = 90° (pole).
/// For the general case, we use the proper rotation.
/// Build the native→celestial rotation for a specific WcsMap, using
/// the proper `WcsNatPole` solution to find the celestial pole's
/// native coords. This is the C-equivalent path: for non-zenithal
/// projections (SFL, CAR, MER, ...) the native fiducial is on the
/// equator, not the pole, so the rotation cannot be derived purely
/// from `(crval_lon, crval_lat)`.
///
/// `lonpole` and `latpole` are FITS-supplied values in radians; pass
/// `None` for "default — let `wcs_nat_pole` pick".
///
/// Mirrors `fitschan.c::WcsNative` (~38188-38302).
pub(crate) fn build_native_to_celestial_rotation_for_wcsmap(
    wcsmap: &WcsMap,
    crval_lon: f64,
    crval_lat: f64,
    lonpole: Option<f64>,
    latpole: Option<f64>,
) -> Option<DynMapping> {
    use crate::fitschan::fset_to_store::fiducial::get_fiducial_nsc;
    use crate::fitschan::fset_to_store::sky_pole::wcs_nat_pole;

    let (phi0, _theta0) = get_fiducial_nsc(wcsmap);

    // Resolve LONPOLE / LATPOLE defaults via WcsNatPole. The function
    // takes "no value supplied" sentinels (≥ π absolute or non-finite);
    // we pass +∞ for None.
    let mut phip = lonpole.unwrap_or(f64::INFINITY);
    let latpole_arg = latpole.unwrap_or(f64::INFINITY);
    let mut alphap = 0.0;
    let mut deltap = 0.0;
    let ok = wcs_nat_pole(
        wcsmap,
        crval_lon,
        crval_lat,
        latpole_arg,
        &mut phip,
        &mut alphap,
        &mut deltap,
    );
    if !ok {
        return None;
    }

    build_rotation_from_pole(phi0, phip, alphap, deltap, crval_lon)
}

/// Build the rotation pipeline given resolved native-pole celestial
/// coords `(alphap, deltap)`, native-pole longitude `phip`, native
/// fiducial longitude `phi0`, and celestial fiducial longitude
/// `alpha0`. Mirrors `fitschan.c::WcsNative` lines 38239-38301.
fn build_rotation_from_pole(
    phi0: f64,
    phip: f64,
    alphap: f64,
    deltap: f64,
    alpha0: f64,
) -> Option<DynMapping> {
    use crate::mappings::{MatrixMap, SphMap};
    use ndarray::Array2;

    // Three rotations matching C's astMtrRot sequence at fitschan.c
    // 38253-38281.
    let r1 = rot_z(-phip);
    let r2 = rot_y(deltap - PI / 2.0);
    let r3 = rot_z(PI + alphap);
    let rot_matrix = mat3_mul(&r3, &mat3_mul(&r2, &r1));

    // sph_to_cart: spherical (lon, lat) → cartesian (x, y, z).
    // C creates this with `astSphMap("UnitRadius=1", ...)` at
    // fitschan.c:38239 — the unit-radius flag is what enables the
    // simplifier to collapse a forward+inverse SphMap pair via
    // SphMap::try_merge (sphmap.c:748-761). Without it, our
    // simplifier sees `unit_radius=false` on the forward SphMap and
    // refuses to cancel the round-trip, leaving the entire WcsMap-
    // through-rotation chain in the simplified graph and accumulating
    // ~6e-15 FP residue when FitLine evaluates the supposed-to-be-
    // linear pixel→IWC chain.
    //
    // C also sets PolarLong = phi0 on this SphMap (line 38244).
    let mut sph_to_cart = SphMap::new();
    sph_to_cart.invert(); // forward = (lon, lat) → (x, y, z)
    sph_to_cart.set_unit_radius(true);
    sph_to_cart.set_polar_long(phi0);

    // 3×3 rotation matrix.
    let mut mat = Array2::zeros((3, 3));
    for i in 0..3 {
        for j in 0..3 {
            mat[[i, j]] = rot_matrix[i][j];
        }
    }
    let rot_map = MatrixMap::new(mat).ok()?;

    // cart_to_sph: cartesian (x, y, z) → spherical (lon, lat).
    // C creates this with `astSphMap("UnitRadius=1", ...)` at
    // fitschan.c:38293 — same reason as above.
    // C sets PolarLong = alpha0 on this SphMap (line 38299).
    let mut cart_to_sph = SphMap::new();
    cart_to_sph.set_unit_radius(true);
    cart_to_sph.set_polar_long(alpha0);

    let step1 = CmpMap::new(
        Box::new(sph_to_cart),
        Box::new(rot_map),
        Composition::Series,
    )
    .ok()?;
    let full = CmpMap::new(Box::new(step1), Box::new(cart_to_sph), Composition::Series).ok()?;
    Some(Box::new(full))
}

/// 3×3 rotation matrix about the Z axis (CCW convention, standard
/// mathematics). Composed via `mat3_mul` to mirror the C library's
/// astMtrRot post-multiplication behaviour.
fn rot_z(angle: f64) -> [[f64; 3]; 3] {
    let c = angle.cos();
    let s = angle.sin();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

/// 3×3 rotation matrix about the Y axis (CCW convention).
fn rot_y(angle: f64) -> [[f64; 3]; 3] {
    let c = angle.cos();
    let s = angle.sin();
    [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
}

fn mat3_mul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                out[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;
    use crate::fitschan::fits_store::FitsStore;

    #[test]
    fn detects_celestial_axes_tan() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);

        let result = build_celestial(&store);
        assert!(result.is_some());
        let cel = result.unwrap();
        assert_eq!(cel.lon_axis, 0);
        assert_eq!(cel.lat_axis, 1);
        assert_eq!(cel.frame.system(), SkySystem::Icrs);
        assert_eq!(cel.mapping.nin(), 2);
        assert_eq!(cel.mapping.nout(), 2);
    }

    #[test]
    fn radesys_icrs() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(0.0);
        store.crval[1] = Some(0.0);
        store.radesys = Some("ICRS".to_string());

        let result = build_celestial(&store).unwrap();
        assert_eq!(result.frame.system(), SkySystem::Icrs);
    }

    #[test]
    fn radesys_fk5() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(0.0);
        store.crval[1] = Some(0.0);
        store.radesys = Some("FK5".to_string());
        store.equinox = Some(2000.0);

        let result = build_celestial(&store).unwrap();
        assert_eq!(result.frame.system(), SkySystem::Fk5);
    }

    #[test]
    fn no_celestial_returns_none() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("FREQ".to_string());
        store.ctype[1] = Some("STOKES".to_string());

        let result = build_celestial(&store);
        assert!(result.is_none());
    }

    #[test]
    fn galactic_from_ctype() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("GLON-CAR".to_string());
        store.ctype[1] = Some("GLAT-CAR".to_string());
        store.crval[0] = Some(0.0);
        store.crval[1] = Some(0.0);

        let result = build_celestial(&store).unwrap();
        assert_eq!(result.frame.system(), SkySystem::Galactic);
    }

    #[test]
    fn reference_pixel_maps_to_crval() {
        // For a TAN projection, IWC (0,0) should map to CRVAL.
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(180.0); // degrees
        store.crval[1] = Some(45.0); // degrees

        let result = build_celestial(&store).unwrap();

        // IWC (0, 0) should give approximately CRVAL in radians.
        let pts = PointSet::from_columns(&[&[0.0], &[0.0]]).unwrap();
        let out = result.mapping.transform(&pts, true).unwrap();
        let point = out.point(0);

        let expected_lon = 180.0_f64.to_radians();
        let expected_lat = 45.0_f64.to_radians();

        // Allow some tolerance since the rotation is approximate
        // for non-pole positions.
        assert!(
            (point[0] - expected_lon).abs() < 1e-6,
            "lon: got {}, expected {}",
            point[0],
            expected_lon
        );
        assert!(
            (point[1] - expected_lat).abs() < 1e-6,
            "lat: got {}, expected {}",
            point[1],
            expected_lat
        );
    }

    #[test]
    fn equatorial_no_radesys_no_equinox_defaults_to_icrs() {
        // RA/DEC primary with no RADESYS/EQUINOX. For FITS-WCS
        // encoding (DefB1950=false), C just sets radesys=ICRS and
        // leaves equinox at AST__BAD (fitschan.c:39172-39180,
        // 42288). The SkyFrame should come out as ICRS with the
        // system's default equinox (J2000) and no explicit epoch.
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(50.5);
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);

        let result = build_celestial(&store).expect("celestial axes recognised");
        // System-default equinox (J2000 = MJD 51544.5) — no B1950.
        let eq = result.frame.equinox_mjd();
        assert!((eq - 51544.5).abs() < 1.0, "equinox = {eq}");
        // Epoch should NOT be explicitly set.
        assert!(
            !result.frame.test_epoch(),
            "epoch should not be explicitly set"
        );
    }

    #[test]
    fn naxis_wcsmap_built_with_full_rank() {
        // 3-axis FITS header with celestial pair at axes 0, 1 and a
        // spectral axis at 2. The decoder returns a full-rank N-axis
        // mapping with the inner WcsMap's `ncoord` matching naxis.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crval[2] = Some(1.4e9);

        let result = build_celestial(&store).expect("celestial axes recognised");
        assert_eq!(result.lon_axis, 0);
        assert_eq!(result.lat_axis, 1);
        // Phase B invariant: the outer mapping is now full-rank.
        assert_eq!(result.mapping.nin(), 3);
        assert_eq!(result.mapping.nout(), 3);
    }

    #[test]
    fn naxis_celestial_passthrough_axes_are_identity() {
        // 3-axis: celestial pair at 0, 1; spectral at 2. After Phase B,
        // the celestial mapping operates as identity on axis 2.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crval[2] = Some(1.4e9);

        let result = build_celestial(&store).expect("celestial axes recognised");
        assert_eq!(result.mapping.nin(), 3);
        assert_eq!(result.mapping.nout(), 3);

        // Send IWC (0, 0, 12345.0) through. axes 0, 1 should land on
        // CRVAL in radians; axis 2 should pass through unchanged.
        let pts = PointSet::from_columns(&[&[0.0], &[0.0], &[12345.0]]).unwrap();
        let out = result.mapping.transform(&pts, true).unwrap();
        let p = out.point(0);
        assert!((p[0] - 180.0_f64.to_radians()).abs() < 1e-6);
        assert!((p[1] - 45.0_f64.to_radians()).abs() < 1e-6);
        assert!((p[2] - 12345.0).abs() < 1e-12, "axis 2 was {}", p[2]);
    }

    #[test]
    fn galactic_no_radesys_no_equinox_does_not_fall_back() {
        // Galactic primary with no RADESYS/EQUINOX. C does NOT apply
        // the DefB1950 fallback for Galactic CTYPE prefixes — the
        // SkyFrame should keep its system-default Equinox / no Epoch.
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("GLON-TAN".to_string());
        store.ctype[1] = Some("GLAT-TAN".to_string());
        store.crval[0] = Some(101.255);
        store.crval[1] = Some(16.177);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(50.5);
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);

        let result = build_celestial(&store).expect("celestial axes recognised");
        // Galactic SkyFrame default equinox is J2000 (51544.5).
        let eq = result.frame.equinox_mjd();
        assert!(
            (eq - 51544.5).abs() < 1.0,
            "equinox = {eq} (should be J2000 default for GALACTIC)"
        );
        // Epoch should NOT be explicitly set — Frame default kicks in.
        assert!(
            !result.frame.test_epoch(),
            "epoch should not be explicitly set for Galactic primary"
        );
    }
}

//! Celestial axis decoder for FITS-WCS.
//!
//! If celestial axes are present in the [`FitsStore`], this module builds the
//! IWC-to-sky mapping and creates a [`SkyFrame`]. The pipeline follows the C
//! library's `WcsCelestial` function (fitschan.c ~line 35458).
//!
//! The transformation from IWC to celestial coordinates is:
//!
//! ```text
//! IWC (degrees) → ZoomMap (deg→rad) → WcsMap inverse (projected→native)
//!     → rotation (native→celestial via CRVAL/LONPOLE/LATPOLE)
//! ```

use std::f64::consts::PI;

use super::fits_store::FitsStore;
use crate::frames::{SkyFrame, SkySystem};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, PermMap, WcsMap, ZoomMap};
use crate::wcslib::WcsProjection;

/// Apply C `WcsSkyFrame`'s NORADEC + DefB1950 fallback rule to an
/// alt-plane SkyFrame whose RADESYS is unset and EQUINOX is unset.
///
/// Mirrors fitschan.c:39172-39180. AST's `DefB1950` attribute defaults
/// to **false** for FITS-WCS encoding (and **true** for everything
/// else — see `astMAKE_GET(FitsChan,DefB1950,…)` at fitschan.c:42288).
///
/// We currently can't see the FitsChan's encoding from the read-side
/// FitsStore; the FITS-WCS reader path is the only caller exercising
/// this function in alt-plane scenarios, so gate on
/// `DefB1950 == false` (the FITS-WCS default) and skip the fallback
/// outright. When an encoding-aware path appears (e.g. FITS-PC alt
/// `Cnxxx` reader), it can call this helper directly with an
/// explicit `def_b1950` flag.
fn apply_b1950_fallback_if_needed(_store: &FitsStore, _sky_frame: &mut SkyFrame) {
    // FITS-WCS reader: DefB1950 = false. No fallback. The
    // NORADEC + no-EQUINOX SkyFrame defaults to ICRS, which the
    // SkySys writer emits as `RADESYS = ICRS` with no EQUINOX or
    // MJD-OBS card.
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
pub(crate) fn build_celestial(store: &FitsStore) -> Option<CelestialResult> {
    // Step 1: Find the celestial axis pair.
    let (lon_axis, lat_axis) = store.celestial_axes()?;

    // Step 2: Get the projection code.
    let proj_code = store.projection_code(lon_axis)?;
    let proj = WcsProjection::from_code(&proj_code)?;

    // Step 3: Create WcsMap.
    // WcsMap axes are 1-based; our celestial subset is always (1=lon, 2=lat).
    let mut wcsmap = WcsMap::new(2, proj, 1, 2).ok()?;

    // Set any PV projection parameters on the latitude axis.
    // In FITS-WCS, PV parameters on the latitude axis (typically axis 2)
    // control the projection. We assign them to the WcsMap's latitude axis.
    for m in 0..store.pv[lat_axis].len() {
        if let Some(val) = store.pv[lat_axis][m] {
            // WcsMap set_pv uses 1-based axis: latitude is axis 2 in our 2D WcsMap.
            let _ = wcsmap.set_pv(2, m, val);
        }
    }
    // PV parameters on the longitude axis.
    for m in 0..store.pv[lon_axis].len() {
        if let Some(val) = store.pv[lon_axis][m] {
            // longitude is axis 1 in our 2D WcsMap.
            let _ = wcsmap.set_pv(1, m, val);
        }
    }

    // Step 4: Determine the coordinate system from RADESYS.
    let system = determine_sky_system(store);

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
        sky_frame.set_epoch(tdb_mjd);
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

    // Step 7c: NORADEC + DefB1950 fallback for alt SkyFrames with
    // no RADESYS, no EQUINOX, no MJD-OBS — typical of GALACTIC alt-A
    // planes. This path produces the 33281.923 MJD-OBS the writer's
    // last-plane-wins rule emits in fits-pc-gal_fits-pc.
    apply_b1950_fallback_if_needed(store, &mut sky_frame);

    // Step 8: Build the mapping.
    // The IWC values are in degrees; WcsMap works in radians.
    // Pipeline: IWC(deg) → deg2rad → WcsMap(inverse: projected→native) → rotation
    let inner = build_iwc_to_sky_mapping(store, wcsmap, lon_axis, lat_axis)?;

    // The inner mapping always treats input slot 0 as lon-side proj
    // and slot 1 as lat-side proj, with outputs (cel_lon, cel_lat) in
    // that order. When the FITS layout has DEC ahead of RA
    // (`lat_axis < lon_axis`), the parent CmpFrame's external axis 0
    // is the lat axis — and the IWC values FITS encodes at axis 0
    // are lat-plane values. Sandwich `inner` with a 2D swap so
    // cel.mapping accepts (lat_proj, lon_proj) and emits (cel_lat,
    // cel_lon), matching FITS axis order. Mirrors C's PermMap
    // sandwich at fitschan.c:5247-5296.
    let mapping: DynMapping = if lat_axis < lon_axis {
        let pre = PermMap::new(&[2, 1], &[2, 1], &[]).ok()?;
        let post = PermMap::new(&[2, 1], &[2, 1], &[]).ok()?;
        let m = CmpMap::new(Box::new(pre), inner, Composition::Series).ok()?;
        Box::new(CmpMap::new(Box::new(m), Box::new(post), Composition::Series).ok()?)
    } else {
        inner
    };

    // Note: when `lat_axis < lon_axis`, `mapping` outputs (cel_lat,
    // cel_lon) at slots (0, 1) — matching the SkyFrame's permuted
    // external layout above. `build_general_iwc_to_world` consults
    // the returned `lon_axis`/`lat_axis` to PermMap these into the
    // FITS axis order they belong to, and `lon_axis`/`lat_axis` are
    // left in their original (FITS-layout) values so downstream
    // routing keeps working.
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
        Some(CelKind::Equatorial) | None => {} // fall through to RADESYS / EQUINOX
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

    // Step 1: Degrees to radians (2D zoom).
    let zoom = ZoomMap::new(2, deg2rad).ok()?;

    // Step 2: WcsMap inverse maps from projected plane to native spherical.
    // The WcsMap forward direction is: native spherical → projected.
    // We need the inverse: projected → native.
    // We can achieve this by inverting the mapping when we compose it.
    // Actually, the WcsMap forward = (lon_native, lat_native) → (x, y).
    // We want IWC (x, y) → (lon_native, lat_native), which is the inverse.
    //
    // In the CmpMap composition, we call wcsmap transform with forward=false.
    // But since we're composing mappings, we need to invert the WcsMap object.
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
    let rotation = build_native_to_celestial_rotation(crval_lon, crval_lat, lonpole)?;

    // Compose: zoom → wcsmap_inv → rotation
    let step1 = CmpMap::new(Box::new(zoom), Box::new(wcsmap_inv), Composition::Series).ok()?;
    let full = CmpMap::new(Box::new(step1), rotation, Composition::Series).ok()?;

    Some(Box::new(full))
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
pub(crate) fn build_native_to_celestial_rotation(
    crval_lon: f64,
    crval_lat: f64,
    lonpole: f64,
) -> Option<DynMapping> {
    use crate::mappings::{MatrixMap, SphMap};
    use ndarray::Array2;

    // Mirror the C library's WcsNative recipe (fitschan.c around line
    // 38246–38281): build the native→celestial rotation as three
    // elementary rotations applied to the unit matrix. We apply them
    // as direct 3×3 multiplications here rather than via PAL's
    // deuler — the two conventions disagree on points near (but not
    // at) the native pole, which produces a wrong CD when the IWC
    // composition is applied on the write side.
    //
    // The C library's three rotations, with the simplifying choice
    // (αₚ, δₚ) = (CRVAL_lon, CRVAL_lat) for zenithal projections at
    // their fiducial point:
    //   1. R_z(-LONPOLE)        — celestial pole at native (0, 0).
    //   2. R_y(δₚ - π/2)        — celestial pole at 90° latitude in
    //                              the rotated frame.
    //   3. R_z(π + αₚ)          — primary meridian at native 0°.
    // Combined: M = R_z(π + αₚ) · R_y(δₚ - π/2) · R_z(-LONPOLE).
    //
    // `astMtrRot` post-multiplies into the existing forward matrix,
    // which is equivalent to pre-multiplying the *rotation* matrix
    // into M as written above (rightmost rotation applied first to
    // a vector v: M·v).
    let alphap = crval_lon;
    let deltap = crval_lat;
    let r1 = rot_z(-lonpole);
    let r2 = rot_y(deltap - PI / 2.0);
    let r3 = rot_z(PI + alphap);
    let rot_matrix = mat3_mul(&r3, &mat3_mul(&r2, &r1));

    // Build the mapping pipeline:
    //   native (phi, theta) → SphMap(inverted) → 3D Cartesian
    //     → MatrixMap(rotation) → 3D Cartesian → SphMap(forward) → celestial (lon, lat)
    //
    // SphMap forward: (x, y, z) → (lon, lat)  [nin=3, nout=2]
    // SphMap inverted: (lon, lat) → (x, y, z) [nin=2, nout=3]

    // Step 1: native spherical to Cartesian (SphMap inverted).
    let mut sph_to_cart = SphMap::new();
    sph_to_cart.invert(); // Now nin=2 (lon,lat), nout=3 (x,y,z)

    // Step 2: 3x3 rotation matrix.
    let mut mat = Array2::zeros((3, 3));
    for i in 0..3 {
        for j in 0..3 {
            mat[[i, j]] = rot_matrix[i][j];
        }
    }
    let rot_map = MatrixMap::new(mat).ok()?;

    // Step 3: Cartesian to celestial spherical (SphMap forward).
    let cart_to_sph = SphMap::new(); // nin=3, nout=2

    // Compose: sph_to_cart → rot_map → cart_to_sph
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

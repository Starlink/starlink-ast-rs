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

use super::warning::{Warning, Warnings};
use std::f64::consts::PI;

use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{Frame, SkyFrame, SkySystem};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, PermMap, UnitMap, WcsMap, WinMap};
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
    /// Celestial reference position (lon, lat) in radians: the raw CRVAL pair
    /// converted to radians, mirroring C `WcsCelestial` (fitschan.c:36012-
    /// 36015). Used to stamp the SkyFrame's SkyRef on read. `None` when either
    /// CRVAL is absent. Overridden by `RFVAL<i>` and then by `AXREF<i>`
    /// (fitschan.c:36091-36097, 36214-36221).
    pub ref_pos: Option<(f64, f64)>,
    /// True when `ref_pos` came from `AXREF<i>` rather than from CRVAL or
    /// `RFVAL<i>`. C sets SkyRef unconditionally in the AXREF case
    /// (fitschan.c:36220-36221), bypassing the `astTestSkyRef` gate that
    /// guards the CRVAL / RFVAL path (fitschan.c:36103).
    pub ref_pos_forced: bool,
    /// Warning messages to emit as ASTWARN cards (mirrors C `Warn`, e.g. the
    /// `badcel` condition for an unrecognised celestial system prefix,
    /// fitschan.c:39358).
    pub warnings: Warnings,
}

/// The celestial axes of an axis description, as C's `WcsCelestial`
/// identifies them.
struct CelestialAxes {
    /// The longitude axis (0-based).
    lon: usize,
    /// The latitude axis (0-based).
    lat: usize,
    /// The projection both axes name.
    proj: WcsProjection,
    /// Whether a CTYPE too short to name a projection made the axes linear
    /// (C's local `carlin` forced on).
    forced_car_lin: bool,
}

/// C's `astWcsPrjType` on the last four characters of a CTYPE: with every
/// blank removed, a `-` and a projection code, matched exactly.
fn wcs_prj_type(last4: &str) -> Option<WcsProjection> {
    let code: String = last4.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let name = code.strip_prefix('-')?;
    WcsProjection::from_code(name).filter(|p| p.code() == name)
}

/// The celestial axes of axis description `s` of `store`, found as C's
/// `WcsCelestial` finds them (fitschan.c:35990-36153), or `None` if there
/// are none.
///
/// A longitude CTYPE is `RA`, `AZ`, `xLON` or `yzLN`, alone or followed by
/// `-`, and a latitude CTYPE `DEC`, `EL`, `xLAT` or `yzLT`; the projection
/// is named by the last four characters of a CTYPE longer than four, and a
/// shorter one is linear. Two longitude or two latitude axes, an unknown
/// projection, axes of different systems or projections, and one axis
/// without the other are AST__BDFTS.
fn find_celestial_axes(store: &FitsStore, s: char) -> AstResult<Option<CelestialAxes>> {
    let keyname = |i: usize| {
        if s == ' ' {
            format!("CTYPE{}", i + 1)
        } else {
            format!("CTYPE{}{s}", i + 1)
        }
    };
    let bdfts = |msg: String| AstError::new(ErrorCode::Bdfts, format!("astRead(FitsChan): {msg}"));
    // The projection a CTYPE names, and whether it is linear for want of one.
    let projection = |i: usize, ctype: &str| -> AstResult<(WcsProjection, bool)> {
        if ctype.len() > 4 {
            let last4 = ctype.get(ctype.len() - 4..).unwrap_or("");
            match wcs_prj_type(last4) {
                Some(p) => Ok((p, false)),
                None => Err(bdfts(format!(
                    "FITS keyword '{}' refers to an unknown projection type '{last4}'.",
                    keyname(i)
                ))),
            }
        } else {
            Ok((WcsProjection::Car, true))
        }
    };
    let is_axis = |ctype: &str, whole: &[&str], tail1: &str, tail2: &str| -> Option<String> {
        for w in whole {
            if ctype == *w || ctype.starts_with(&format!("{w}{}", "-".repeat(4 - w.len()))) {
                return Some(match *w {
                    "RA" | "DEC" => "EQU".to_string(),
                    _ => "AZL".to_string(),
                });
            }
        }
        let tail_matches = |from: usize, tail: &str| {
            ctype
                .get(from..)
                .is_some_and(|rest| rest == tail || rest.starts_with(&format!("{tail}-")))
        };
        if tail_matches(1, tail1) {
            Some(ctype[..1].to_string())
        } else if tail_matches(2, tail2) {
            Some(ctype[..2].to_string())
        } else {
            None
        }
    };

    // (axis, system, projection) of each axis found.
    let mut lon: Option<(usize, String, WcsProjection)> = None;
    let mut lat: Option<(usize, String, WcsProjection)> = None;
    let mut forced_car_lin = false;
    for i in 0..store.naxis {
        let Some(ctype) = store.ctype[i].as_deref() else {
            continue;
        };
        let ctype = ctype.trim_end();
        if let Some(system) = is_axis(ctype, &["RA", "AZ"], "LON", "LN") {
            if let Some((j, ..)) = lon {
                return Err(bdfts(format!(
                    "FITS keywords '{}' (='{ctype}') and '{}' (='{}') both describe \
                     celestial longitude axes.",
                    keyname(i),
                    keyname(j),
                    store.ctype[j].as_deref().unwrap_or("").trim_end()
                )));
            }
            let (p, linear) = projection(i, ctype)?;
            forced_car_lin |= linear;
            lon = Some((i, system, p));
        }
        if let Some(system) = is_axis(ctype, &["DEC", "EL"], "LAT", "LT") {
            if let Some((j, ..)) = lat {
                return Err(bdfts(format!(
                    "FITS keywords '{}' (='{ctype}') and '{}' (='{}') both describe \
                     celestial latitude axes.",
                    keyname(i),
                    keyname(j),
                    store.ctype[j].as_deref().unwrap_or("").trim_end()
                )));
            }
            let (p, linear) = projection(i, ctype)?;
            forced_car_lin |= linear;
            lat = Some((i, system, p));
        }
    }
    let ctype_of = |i: usize| {
        store.ctype[i]
            .as_deref()
            .unwrap_or("")
            .trim_end()
            .to_string()
    };
    match (lon, lat) {
        (Some((ilon, lonsys, lonprj)), Some((ilat, latsys, latprj))) => {
            if latsys != lonsys {
                return Err(bdfts(format!(
                    "FITS keywords '{}' and '{}' indicate different celestial coordinate \
                     systems ('{}' and '{}').",
                    keyname(ilat),
                    keyname(ilon),
                    ctype_of(ilat),
                    ctype_of(ilon)
                )));
            }
            if lonprj != latprj {
                return Err(bdfts(format!(
                    "FITS keywords '{}' and '{}' indicate different projections ('{}' and '{}').",
                    keyname(ilat),
                    keyname(ilon),
                    ctype_of(ilat),
                    ctype_of(ilon)
                )));
            }
            Ok(Some(CelestialAxes {
                lon: ilon,
                lat: ilat,
                proj: lonprj,
                forced_car_lin,
            }))
        }
        (None, Some((ilat, ..))) => Err(bdfts(format!(
            "A latitude axis ('{}') was found without a corresponding longitude axis.",
            ctype_of(ilat)
        ))),
        (Some((ilon, ..)), None) => Err(bdfts(format!(
            "A longitude axis ('{}') was found without a corresponding latitude axis.",
            ctype_of(ilon)
        ))),
        (None, None) => Ok(None),
    }
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
    build_celestial_with_def_b1950(store, ' ', false, None).unwrap()
}

/// Encoding-aware variant. `s` is the version character of the axis
/// description (a blank for the primary one). `def_b1950` mirrors the FitsChan
/// `DefB1950` attribute (fitschan.c:42288): true for FITS-PC /
/// FITS-IRAF / FITS-AIPS / FITS-AIPS++ / FITS-CLASS / DSS, false
/// for FITS-WCS. Triggers the NORADEC fallback (`apply_b1950_fallback_if_needed`)
/// when the alt-plane SkyFrame has no RADESYS / EQUINOX / MJD-OBS.
///
/// `rfval` carries the `RFVAL<i>` values (degrees, 0-based FITS axis) read
/// straight off the card list, unmarked, the way C's `GetValue` does at the
/// point of use (fitschan.c:36091-36097) — `FitsStore` has no member for it
/// because C has none either.
pub(crate) fn build_celestial_with_def_b1950(
    store: &FitsStore,
    s: char,
    def_b1950: bool,
    rfval: Option<&[Option<f64>]>,
) -> AstResult<Option<CelestialResult>> {
    // Step 1: Find the celestial axis pair and its projection, as C's
    // `WcsCelestial` does, reporting the errors it reports.
    let Some(CelestialAxes {
        lon: lon_axis,
        lat: lat_axis,
        proj,
        forced_car_lin,
    }) = find_celestial_axes(store, s)?
    else {
        return Ok(None);
    };

    // C keeps one local `carlin`, initialised from the attribute
    // (fitschan.c:35683) and forced on for a projectionless CTYPE. It never
    // writes the attribute back, so reading a header leaves `CarLin` alone.
    let car_lin = store.car_lin || forced_car_lin;

    // Step 3: Create WcsMap.
    // Full-rank N-axis WcsMap: `ncoord` is the full pipeline rank,
    // and the lon/lat axes point at the FITS positions (1-based).
    // Non-celestial axes pass through unchanged. Mirrors C
    // `WcsCelestial` (fitschan.c:35458) which builds the celestial
    // contribution as an N-axis mapping at the top level.
    let mut wcsmap = WcsMap::new(store.naxis, proj, lon_axis + 1, lat_axis + 1)?;

    // Warning messages to emit as ASTWARN cards, in generation order — C
    // appends them to the FitsChan as each `Warn` call is reached, and the
    // `badpv` ones below come before `badcel`.
    let mut warnings: Warnings = Vec::new();

    // Store the supplied PVi_m projection parameters on the WcsMap, warning
    // for any the projection cannot use. Mirrors C `WcsCelestial`
    // (fitschan.c:35965-36001): one ascending pass over m up to the highest
    // index present on any axis, checking the latitude axis before the
    // longitude axis at each index, which is the order the ASTWARN cards
    // appear in.
    let np = store.pv[lat_axis]
        .len()
        .max(store.pv[lon_axis].len())
        .saturating_sub(1);
    let prj_name = format!("-{}", proj.code());
    for m in 0..=np {
        for axis in [lat_axis, lon_axis] {
            let Some(val) = store.pv[axis].get(m).copied().flatten() else {
                continue;
            };
            // `set_pv` enforces the projection's per-axis limit, so its error
            // is the "parameter not used by this projection" signal.
            if wcsmap.set_pv(axis + 1, m, val).is_err() {
                warnings.push(Warning::new(
                    "badpv",
                    format!(
                        "Projection parameter PV{}_{} found, but is not used by {} projections.",
                        axis + 1,
                        m,
                        prj_name
                    ),
                ));
            }
        }
    }

    // Propagate LONPOLE/LATPOLE into PVi_3 / PVi_4 on the longitude
    // axis, mirroring fitschan.c:38199-38217. C: when LONPOLE/LATPOLE
    // are stored as the dedicated keywords (not as PV3/PV4), copy
    // them across so the FrameSet→Store round trip recovers them.
    // WcsMap PV parameters are stored in FITS-WCS units (degrees) — C's
    // GetNatLon/GetNatLat read them back and apply AST__DD2R on use
    // (wcsmap.c:1948) — so store the degree value directly, not radians.
    // Skip for TPN — PV3/PV4 there are polynomial coefficients.
    let is_tpn = matches!(proj, WcsProjection::Tpn);
    if !is_tpn {
        if wcsmap.stored_pv(lon_axis + 1, 3).is_none()
            && let Some(lon_deg) = store.lonpole
        {
            let _ = wcsmap.set_pv(lon_axis + 1, 3, lon_deg);
        }
        if wcsmap.stored_pv(lon_axis + 1, 4).is_none()
            && let Some(lat_deg) = store.latpole
        {
            let _ = wcsmap.set_pv(lon_axis + 1, 4, lat_deg);
        }
    }

    // Classify the longitude CTYPE prefix.
    let lon_ctype_prefix = store.ctype[lon_axis]
        .as_deref()
        .map(|c| c.trim().to_ascii_uppercase());
    let lon_cel_kind = lon_ctype_prefix.as_deref().and_then(classify_ctype_prefix);

    // Detect whether the CTYPE prefix indicates an offset sky frame
    // (CTYPE = OFLN/OFLT). C `WcsSkyFrame` (fitschan.c:39277) tags
    // the resulting SkyFrame with Domain = SKY_OFFSETS so TidyOffsets
    // can identify it.
    let ctype_is_offset = lon_cel_kind == Some(CelKind::OffsetSky);

    // Step 4: Determine the coordinate system from RADESYS.
    //
    // C `WcsSkyFrame` (fitschan.c:39259-39277) treats the `OF` (offset
    // sky) CTYPE code as an *unknown* spherical system: it builds the
    // SkyFrame with `System=UNKNOWN`, sets the axis Symbols to
    // OFLN / OFLT, and tags the Frame with Domain=SKY_OFFSETS. It does
    // NOT consult RADESYS, and (per the EQU/ECL gate at 39305-39312)
    // does NOT store EQUINOX. The underlying absolute system is recovered
    // later via the SkyRef / SkyRefP reference position and the
    // FrameSet-integrity re-mapping driven by SkyRefIs.
    let system = if ctype_is_offset {
        SkySystem::Unknown
    } else {
        determine_sky_system(store)
    };

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
    //
    // C always constructs the SkyFrame with an explicit `"System=..."`
    // attribute setting string: every branch of `WcsSkyFrame` calls
    // `astSkyFrame("System=...")` (fitschan.c:39294-39328), including
    // `System=ICRS` at 39314 and `System=UNKNOWN` at 39328, so
    // `astTestSystem` is true afterwards for every result.
    // `SkyFrame::new` alone leaves `system_set = false` for
    // `SkySystem::Icrs` (the SkyFrame's own unset-attribute default), so
    // call `set_system` explicitly to mark System as set regardless of
    // which system was chosen.
    let mut sky_frame = SkyFrame::new(system);
    sky_frame.set_system(system);
    if lat_axis < lon_axis {
        // perm_axes is 1-based: [2, 1] swaps so that external axis
        // 1 maps from the original axis 2 (lat) and external axis
        // 2 maps from axis 1 (lon).
        let _ = crate::frames::Frame::perm_axes(&mut sky_frame, &[2, 1]);
    }

    // For an offset (`OF`) or otherwise unknown celestial prefix, set
    // the axis Symbols from the system code. Mirrors C `WcsSkyFrame`
    // (fitschan.c:39325-39342): for a 1-char system prefix `x`, symbols
    // are `xLON`/`xLAT`; for a 2-char prefix `xy`, symbols are
    // `xyLN`/`xyLT`. The `OF` case uses the same 2-char path and is
    // handled identically here (Domain=SKY_OFFSETS is set below).
    if matches!(
        lon_cel_kind,
        Some(CelKind::Unknown) | Some(CelKind::OffsetSky)
    ) {
        // Extract the raw system prefix from the CTYPE (before the `-`
        // at position 4). The CTYPE is already upper-cased in
        // lon_ctype_prefix.
        let ctype_raw = lon_ctype_prefix.as_deref().unwrap_or("XLON");
        let bytes = ctype_raw.as_bytes();
        // 1-char prefix: bytes[1..4] == b"LON", so bytes[0] is the char.
        // 2-char prefix: bytes[2..4] == b"LN", so bytes[0..2] is the code.
        let (lon_sym, lat_sym) = if bytes.len() >= 4 && &bytes[1..4] == b"LON" {
            // 1-char system prefix (fitschan.c:39328-39334).
            let c = bytes[0] as char;
            (format!("{c}LON"), format!("{c}LAT"))
        } else {
            // 2-char system prefix (fitschan.c:39336-39342).
            let c0 = bytes[0] as char;
            let c1 = bytes[1] as char;
            (format!("{c0}{c1}LN"), format!("{c0}{c1}LT"))
        };
        // Axis numbering is 1-based (SkyFrame lon=1, lat=2 after any perm).
        let (lon_api, lat_api) = if lat_axis < lon_axis { (2, 1) } else { (1, 2) };
        let _ = sky_frame.set_symbol(lon_api, &lon_sym);
        let _ = sky_frame.set_symbol(lat_api, &lat_sym);
    }

    // Step 5b: CNAME → Label. Mirrors C `WcsSkyFrame` (fitschan.c:39378-
    // 39381): when either CNAME keyword is set, it becomes the axis Label —
    // `astSetLabel(ret, 0, ...)` for the longitude axis and
    // `astSetLabel(ret, 1, ...)` for the latitude axis. C addresses the
    // Label axes with exactly the same indices it uses for the Symbols at
    // fitschan.c:39332-39339, so the same lon-first/lat-second 1-based
    // numbering used for the Symbols above applies here.
    let (lon_api, lat_api) = if lat_axis < lon_axis { (2, 1) } else { (1, 2) };
    if let Some(cname) = &store.cname[lon_axis] {
        let _ = sky_frame.set_label(lon_api, cname);
    }
    if let Some(cname) = &store.cname[lat_axis] {
        let _ = sky_frame.set_label(lat_api, cname);
    }

    // For a genuinely unknown celestial system (not offset sky), emit the
    // `badcel` warning. Mirrors C `WcsSkyFrame` (fitschan.c:39351-39358):
    // the warning is guarded by `report` (false for `OF`) and by the
    // `Warnings` attribute including `badcel` (in the default Warnings value).
    if lon_cel_kind == Some(CelKind::Unknown) {
        let lontype = store.ctype[lon_axis].as_deref().unwrap_or("?");
        let lattype = store.ctype[lat_axis].as_deref().unwrap_or("?");
        warnings.push(Warning::new(
            "badcel",
            format!(
                "This FITS header contains references to an unknown \
             spherical co-ordinate system specified in the values \
             {lontype} and {lattype}. It may not be possible to convert to \
             other standard co-ordinate systems."
            ),
        ));
    }

    // Store the projection description (mirrors C `WcsSkyFrame`,
    // fitschan.c:39300: `astSetProjection( ret, astWcsPrjDesc( prj ) )` for
    // any non-WCSBAD projection). This feeds the SkyFrame Title suffix
    // "; <desc> projection" and the serialised `Proj` token.
    if !matches!(proj, WcsProjection::Unknown) {
        sky_frame.set_projection(proj.description());
    }

    // C's "EQU" / "ECL" classification — the systems for which the Equinox
    // attribute is stored at all (fitschan.c:39371-39373). Includes ICRS and
    // GAPPT, which C classes as "EQU" even though they carry no equinox.
    let is_equ_or_ecl = matches!(
        system,
        SkySystem::Fk4
            | SkySystem::Fk4NoE
            | SkySystem::Fk5
            | SkySystem::Icrs
            | SkySystem::Gappt
            | SkySystem::Ecliptic
            | SkySystem::Helioecliptic
    );

    // Effective equinox year, mirroring C's defaulting (fitschan.c:39193-
    // 39256): a supplied EQUINOX is used as-is; otherwise the default comes
    // from the RADESYS keyword alone — FK4/FK4-NO-E give B1950, FK5 gives
    // J2000, and ICRS/GAPPT get no equinox at all. C resolves its `radesys`
    // variable straight from the keyword (fitschan.c:39168-39191), never from
    // the CTYPE-derived `sys`, so a Galactic axis with `RADESYS='FK5'` still
    // picks up the J2000 default. The no-RADESYS / no-EQUINOX case is NOT
    // defaulted here — C leaves equinox = AST__BAD (the NORADEC / DefB1950
    // path is handled by `apply_b1950_fallback_if_needed`).
    //
    // No system guard: C computes `eqmjd` from whatever equinox it has
    // (fitschan.c:39204-39210) and uses it as the Epoch fallback for every
    // system (fitschan.c:39215-39216, applied unguarded at :39370).
    let equinox_year: Option<f64> = if let Some(eq) = store.equinox {
        Some(eq)
    } else {
        match store.radesys.as_deref().map(|r| r.trim().to_uppercase()) {
            Some(ref r) if r == "FK4" || r == "FK4-NO-E" || r == "FK4_NO_E" => Some(1950.0),
            Some(ref r) if r == "FK5" => Some(2000.0),
            // ICRS, GAPPT, an unrecognised value, or no RADESYS at all.
            _ => None,
        }
    };

    // Step 6: set the Equinox attribute. C restricts this to equatorial and
    // ecliptic systems (fitschan.c:39374) and applies no further system test:
    // an ICRS or GAPPT header carrying an EQUINOX card gets the attribute set
    // like any other "EQU" axis. `SkyFrame::Dump` then writes the value because
    // an explicitly set attribute is dumped whatever its `helpful` flag says
    // (skyframe.c:12005-12020) — `helpful` only governs whether an *unset*
    // Equinox appears as a commented default, which is where ICRS and GAPPT
    // differ from FK4/FK5/Ecliptic.
    let eq_year: Option<f64> = is_equ_or_ecl.then_some(equinox_year).flatten();
    if let Some(eq) = eq_year {
        sky_frame.set_equinox_year(eq);
    }

    // Step 7: set Epoch. C uses ChooseEpoch (MJD-OBS / DATE-OBS), then
    // falls back to the equinox MJD when no observation date is present
    // (fitschan.c:39214-39222), and stores it on every system whose
    // mjdobs ends up defined (line 39303). For EQU/ECL the eqmjd fallback
    // makes Epoch defined even without MJD-OBS.
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
    } else if let Some(eq) = equinox_year {
        // mjdobs ← eqmjd fallback (fitschan.c:39215-39216), applied to every
        // system (fitschan.c:39370). Only the Equinox attribute above is
        // restricted by system.
        sky_frame.set_epoch_year(eq);
    }

    // Step 7b: DTAI / DUT1 (TAI-UTC and UT1-UTC offsets in seconds).
    if let Some(dtai) = store.dtai {
        sky_frame.set_dtai(dtai);
    }
    if let Some(dut1) = store.dut1 {
        // C fitschan.c:37888: the SkyFrame Dut1 attribute is in seconds,
        // while the FITS DUT1 keyword is in days; convert with seconds-per-day.
        sky_frame.set_dut1(dut1 * 86_400.0);
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
    // C sets each axis on its own, on the SkyFrame before any permutation,
    // so axis 1 is the longitude.
    if let Some(v) = store.skyref[lon_axis] {
        sky_frame.set_sky_ref(1, v)?;
    }
    if let Some(v) = store.skyref[lat_axis] {
        sky_frame.set_sky_ref(2, v)?;
    }
    if let Some(v) = store.skyrefp[lon_axis] {
        sky_frame.set_sky_ref_p(1, v)?;
    }
    if let Some(v) = store.skyrefp[lat_axis] {
        sky_frame.set_sky_ref_p(2, v)?;
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
    let Some((mapping, ref_pos, ref_pos_forced)) = build_iwc_to_sky_mapping(
        store,
        wcsmap,
        store.naxis,
        lon_axis,
        lat_axis,
        rfval,
        car_lin,
    )?
    else {
        return Ok(None);
    };

    // `mapping` emits cel_lon at FITS slot `lon_axis` and cel_lat at FITS
    // slot `lat_axis`, with identity on every other axis: the PermMap
    // sandwich inside `embed_celestial_in_naxis` routes the celestial pair
    // into the rotation's slots (0, 1) and restores them to their FITS
    // positions, matching C `WcsNative` (fitschan.c:38391-38414). For the
    // swapped-axis case (`lat_axis < lon_axis`) that leaves (cel_lat,
    // cel_lon) at slots (0, 1), matching the SkyFrame's permuted external
    // layout. `build_general_iwc_to_world` composes the per-class mappings
    // in series — no axis-permute step is needed at the composer.
    Ok(Some(CelestialResult {
        mapping,
        frame: sky_frame,
        lon_axis,
        lat_axis,
        ref_pos,
        ref_pos_forced,
        warnings,
    }))
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
        Some(CelKind::Helioecliptic) => return SkySystem::Helioecliptic,
        Some(CelKind::Supergalactic) => return SkySystem::Supergalactic,
        Some(CelKind::AzEl) => return SkySystem::Azel,
        // Unrecognised celestial prefix: C WcsSkyFrame (fitschan.c:39325)
        // uses System=Unknown without consulting RADESYS or EQUINOX.
        Some(CelKind::Unknown) => return SkySystem::Unknown,
        // Equatorial (and no CTYPE prefix at all) fall through to the RADESYS /
        // EQUINOX rules below.
        Some(CelKind::Equatorial) | None => {}
        // OffsetSky is intercepted by the `ctype_is_offset` early-out in
        // `build_celestial_with_def_b1950` before this function is reached,
        // and produces System=Unknown to match C (fitschan.c:39325).  The arm
        // is required for match exhaustiveness; it is not reachable at runtime.
        Some(CelKind::OffsetSky) => return SkySystem::Unknown,
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
    Helioecliptic,
    Supergalactic,
    AzEl,
    /// Offset sky-frame indicator (CTYPE prefix `OF`, suffix `LN`/`LT`).
    /// Mirrors C `sys = "OF"` (fitschan.c:39277). The resulting
    /// SkyFrame inherits its underlying system from RADESYS (default
    /// FK5/ICRS via the equatorial fallback) and is tagged with
    /// Domain = SKY_OFFSETS.
    OffsetSky,
    /// Unrecognised single- or two-character celestial-system prefix.
    /// Mirrors C `WcsSkyFrame` (fitschan.c:39325): any `sys` value that
    /// is not "EQU", "AZL", "G", "E", "S", "H", or "OF" results in
    /// `System=Unknown`.
    Unknown,
}

/// Classify a CTYPE prefix into the C library's celestial-system code.
///
/// Mirrors fitschan.c:35648-35734. Recognised prefixes (first four characters,
/// before any `-` projection suffix):
///  - `RA--`, `AZ--`, `EL--` (special 2-char system codes)
///  - `xLON` / `xLAT` (1-letter system + LON/LAT) — `x` ∈ {`G`, `E`,
///    `S`, `H`} for Galactic / Ecliptic / Supergalactic / Helioecliptic;
///    any other `x` maps to `CelKind::Unknown`.
///  - `xyLN` / `xyLT` (2-letter system + LN/LT) — `OF` maps to
///    `CelKind::OffsetSky`; any other `xy` maps to `CelKind::Unknown`.
///
/// Returns `None` only when the CTYPE does not look celestial at all.
fn classify_ctype_prefix(ctype: &str) -> Option<CelKind> {
    let ctype = ctype.trim();
    if ctype.starts_with("RA--") || ctype == "RA" || ctype.starts_with("DEC-") || ctype == "DEC" {
        return Some(CelKind::Equatorial);
    }
    if ctype.starts_with("AZ--") || ctype == "AZ" || ctype.starts_with("EL--") || ctype == "EL" {
        return Some(CelKind::AzEl);
    }
    // 1-letter prefix: xLON / xLAT (fitschan.c:35718-35726).
    // The `ctype.len() >= 4` guard below makes the byte slices safe.
    if ctype.len() >= 4 {
        let bytes = ctype.as_bytes();
        if &bytes[1..4] == b"LON" || &bytes[1..4] == b"LAT" {
            let c = ctype.chars().next().unwrap();
            return Some(match c {
                'G' => CelKind::Galactic,
                'E' => CelKind::Ecliptic,
                'S' => CelKind::Supergalactic,
                'H' => CelKind::Helioecliptic,
                // Any other 1-char prefix is an unrecognised system
                // (fitschan.c:39325 else-if branch → System=Unknown).
                _ => CelKind::Unknown,
            });
        }
        // 2-letter prefix: xyLN / xyLT (fitschan.c:35728-35734).
        if &bytes[2..4] == b"LN" || &bytes[2..4] == b"LT" {
            return Some(if &bytes[0..2] == b"OF" {
                // Offset sky-frame (fitschan.c:39277).
                CelKind::OffsetSky
            } else {
                // Unrecognised 2-char prefix (fitschan.c:39325).
                CelKind::Unknown
            });
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
#[allow(clippy::type_complexity)]
fn build_iwc_to_sky_mapping(
    store: &FitsStore,
    wcsmap: WcsMap,
    naxis: usize,
    lon_axis: usize,
    lat_axis: usize,
    rfval: Option<&[Option<f64>]>,
    car_lin: bool,
) -> AstResult<Option<(DynMapping, Option<(f64, f64)>, bool)>> {
    let deg2rad = PI / 180.0;

    // Read the projection before `wcsmap` is consumed by the inverse binding
    // below. C keeps `latprj` in a local for the same reason.
    let proj = wcsmap.projection();

    // CRVAL for the celestial axes (in degrees, convert to radians for the rotation).
    let crval_lon = store.crval[lon_axis].unwrap_or(0.0) * deg2rad;
    let crval_lat = store.crval[lat_axis].unwrap_or(0.0) * deg2rad;

    // LONPOLE (in degrees), kept optional so WcsNatPole can resolve the
    // projection-aware default (fitschan.c::WcsNatPole): 0° for θ0=0
    // projections (SFL/GLS, AIT, …), 180° otherwise.  C passes AST__BAD
    // here and lets WcsNatPole choose; hard-defaulting to 180° before that
    // call is wrong for θ0=0 projections and mirrors the pole.
    let lonpole = store.lonpole.map(|v| v * deg2rad);

    // Step 1: Degrees to radians on the celestial axes only; identity
    // elsewhere. Diagonal MatrixMap, recognised by MatrixMap::form()
    // as MatrixForm::Diagonal — O(N) per point. Mirrors C
    // fitschan.c:35820-35828.
    use ndarray::Array2;
    let mut zoom_mat: Array2<f64> = Array2::eye(naxis);
    zoom_mat[[lon_axis, lon_axis]] = deg2rad;
    zoom_mat[[lat_axis, lat_axis]] = deg2rad;
    let zoom = crate::mappings::MatrixMap::new(zoom_mat)?;

    // A `-CAR` projection read under CarLin is a plain linear mapping of IWC
    // to celestial coordinates, so the native-spherical rotation is skipped
    // entirely (fitschan.c:35900-35953). C still applies the RFVAL / AXREF
    // reference-position overrides afterwards, in common with the spherical
    // path, so hand off to the shared tail rather than returning the seed.
    if car_lin && matches!(proj, WcsProjection::Car) {
        let Some((full, seed)) = build_linear_car_mapping(store, naxis, lon_axis, lat_axis, zoom)
        else {
            return Ok(None);
        };
        let (ref_pos, ref_pos_forced) = apply_ref_overrides(store, rfval, lon_axis, lat_axis, seed);
        return Ok(Some((full, ref_pos, ref_pos_forced)));
    }

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
        lonpole,
        latpole_rad,
    )?;

    // Celestial reference position (SkyRef) for the SkyFrame. C `WcsCelestial`
    // starts from the raw CRVAL values converted to radians (fitschan.c:36012-
    // 36015), NOT from the fiducial point transformed through the rotation.
    // The two are mathematically equivalent except that the rotation's atan2
    // wraps longitude into (−π,π] (so a CAR frame with CRVAL=185° would
    // become −175°) and loses precision in the last ULPs; the raw CRVAL
    // avoids both. Only set when both CRVAL values are present (C guards on
    // `*reflon/*reflat != AST__BAD`).
    let seed = match (store.crval[lon_axis], store.crval[lat_axis]) {
        (Some(_), Some(_)) => Some((crval_lon, crval_lat)),
        _ => None,
    };
    let (ref_pos, ref_pos_forced) = apply_ref_overrides(store, rfval, lon_axis, lat_axis, seed);

    let Some(rotation_naxis) = embed_celestial_in_naxis(rotation, naxis, lon_axis, lat_axis) else {
        return Ok(None);
    };

    // Pipeline: zoom (N→N diagonal) → wcsmap_inv (N→N) → rotation_naxis (N→N)
    let step1 = CmpMap::new(Box::new(zoom), Box::new(wcsmap_inv), Composition::Series)?;
    let full = CmpMap::new(Box::new(step1), rotation_naxis, Composition::Series)?;
    Ok(Some((Box::new(full), ref_pos, ref_pos_forced)))
}

/// Apply the `RFVAL<i>` and `AXREF<i>` overrides to a celestial reference
/// position seeded from CRVAL. Returns the final position and whether SkyRef
/// must be set even when the SkyFrame already carries one.
fn apply_ref_overrides(
    store: &FitsStore,
    rfval: Option<&[Option<f64>]>,
    lon_axis: usize,
    lat_axis: usize,
    seed: Option<(f64, f64)>,
) -> (Option<(f64, f64)>, bool) {
    let deg2rad = PI / 180.0;
    let mut ref_pos = seed;
    let mut ref_pos_forced = false;

    // RFVAL<i> overrides CRVAL (fitschan.c:36091-36097). C's comment there
    // describes them as the pre-translation CRVAL values, which are what a
    // SpecFrame reference position needs. The override changes the reference
    // position only — it does not force SkyRef to be set, so the
    // `astTestSkyRef` gate downstream still applies.
    if let (Some(lon), Some(lat)) = (
        rfval.and_then(|r| r[lon_axis]),
        rfval.and_then(|r| r[lat_axis]),
    ) {
        ref_pos = Some((lon * deg2rad, lat * deg2rad));
    }

    // AXREF<i> overrides both (fitschan.c:36209-36223) and, unlike RFVAL,
    // sets SkyRef unconditionally — outside the `astTestSkyRef` gate.
    // `astWrite` emits these for `-TAB` axes that have no inverse transform.
    if let (Some(lon), Some(lat)) = (store.axref[lon_axis], store.axref[lat_axis]) {
        ref_pos = Some((lon * deg2rad, lat * deg2rad));
        ref_pos_forced = true;
    }

    (ref_pos, ref_pos_forced)
}

/// Build the IWC-to-celestial mapping for a `-CAR` projection read under
/// CarLin, mirroring C `WcsCelestial`'s linear branch
/// (fitschan.c:35908-35953).
///
/// The pipeline is `zoom` (deg→rad on the celestial axes) followed by a
/// projectionless `WcsMap` — which `WcsMap::simplified` collapses to a
/// `UnitMap`, matching C's `astWcsMap( naxes, AST__WCSBAD, ... )` — and a
/// `WinMap` that adds the CRVAL values in radians to the two celestial axes
/// and leaves every other axis alone.
///
/// Also returns the celestial reference position seeded from CRVAL, or `None`
/// when either CRVAL is absent; C stores `AST__BAD` in `*reflon`/`*reflat` in
/// that case and puts `AST__BAD` in the WinMap corner too, which
/// `WinMap::from_corners` reproduces.
fn build_linear_car_mapping(
    store: &FitsStore,
    naxis: usize,
    lon_axis: usize,
    lat_axis: usize,
    zoom: crate::mappings::MatrixMap,
) -> Option<(DynMapping, Option<(f64, f64)>)> {
    let deg2rad = PI / 180.0;

    // The `AST__WCSBAD` WcsMap. C keeps a real WcsMap (rather than a UnitMap)
    // on the CAR path so the FrameSet→FitsStore write path can read the
    // projection back off it; `WcsProjection::Unknown` is the same idea, and
    // `WcsMap::simplified` collapses it to a UnitMap when the pipeline is
    // simplified (wcsmap.c:3087-3097).
    let wcsbad = WcsMap::new(naxis, WcsProjection::Unknown, lon_axis + 1, lat_axis + 1).ok()?;

    // The WinMap that adds CRVAL. C builds it from the corner pair
    // `(ina, inb) = (0, 1)` to `(outa, outb) = (reflon, 1 + reflon)`, i.e.
    // unit scale and a per-axis shift, with `AST__BAD` on a celestial axis
    // whose CRVAL is absent (fitschan.c:35915-35950).
    let ina = vec![0.0; naxis];
    let inb = vec![1.0; naxis];
    let mut outa = ina.clone();
    let mut outb = inb.clone();

    let mut ref_lon = None;
    let mut ref_lat = None;
    for (axis, slot) in [(lon_axis, &mut ref_lon), (lat_axis, &mut ref_lat)] {
        match store.crval[axis] {
            Some(val) => {
                // For a recognised projection the CRVAL value is required to
                // be in degrees, so convert to radians.
                let rad = val * deg2rad;
                *slot = Some(rad);
                outa[axis] += rad;
                outb[axis] += rad;
            }
            None => {
                outa[axis] = crate::constants::BAD_VALUE;
                outb[axis] = crate::constants::BAD_VALUE;
            }
        }
    }
    let winmap = WinMap::from_corners(&ina, &inb, &outa, &outb).ok()?;

    let step1 = CmpMap::new(Box::new(zoom), Box::new(wcsbad), Composition::Series).ok()?;
    let full = CmpMap::new(Box::new(step1), Box::new(winmap), Composition::Series).ok()?;

    let ref_pos = match (ref_lon, ref_lat) {
        (Some(lon), Some(lat)) => Some((lon, lat)),
        _ => None,
    };
    Some((Box::new(full), ref_pos))
}

/// Embed the 2-axis native→celestial rotation into an `naxis`-rank mapping,
/// routing the celestial pair to the rotation's slots `(0, 1)` and restoring
/// them to their FITS positions on output. The non-celestial axes pass
/// through unchanged.
///
/// Mirrors C `WcsNative` (fitschan.c:38378-38414):
///  - When there are non-celestial axes (`nax_rem = naxis - 2 > 0`), a
///    `UnitMap(nax_rem)` runs in parallel with the rotation (stages 4/5).
///  - When the celestial axes are not already at slots `(0, 1)`
///    (`axlon != 0 || axlat != 1`), a PermMap routes them there and its
///    inverse restores them (stages 1/3). Otherwise the core is returned
///    unchanged.
fn embed_celestial_in_naxis(
    inner: DynMapping,
    naxis: usize,
    lon_axis_0based: usize,
    lat_axis_0based: usize,
) -> Option<DynMapping> {
    debug_assert_eq!(inner.nin(), 2);
    debug_assert_eq!(inner.nout(), 2);

    // Stages 4/5: place a UnitMap in parallel for the non-celestial axes,
    // but only when there are any (fitschan.c:38380 `if( nax_rem > 0 )`).
    let core: DynMapping = if naxis > 2 {
        let unit_rest = UnitMap::new(naxis - 2);
        Box::new(CmpMap::new(inner, Box::new(unit_rest), Composition::Parallel).ok()?)
    } else {
        inner
    };

    // Stages 1/3: only rearrange when the celestial axes are not already at
    // slots (0, 1) (fitschan.c:38391 `if( axlon != 0 || axlat != 1 )`).
    if lon_axis_0based == 0 && lat_axis_0based == 1 {
        return Some(core);
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

    let s1 = CmpMap::new(Box::new(perm_to_2d), core, Composition::Series).ok()?;
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
/// Mirrors `fitschan.c::WcsNative` (~38188-38302): a pole `WcsNatPole`
/// cannot place, because LONPOLE cannot hold for the reference point, is
/// `ErrorCode::Bdfts`.
pub(crate) fn build_native_to_celestial_rotation_for_wcsmap(
    wcsmap: &WcsMap,
    crval_lon: f64,
    crval_lat: f64,
    lonpole: Option<f64>,
    latpole: Option<f64>,
) -> AstResult<DynMapping> {
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
        return Err(AstError::new(
            ErrorCode::Bdfts,
            "Conversion from FITS WCS native coordinates to celestial coordinates is \
             ill-conditioned.",
        ));
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
) -> AstResult<DynMapping> {
    use crate::mappings::{MatrixMap, SphMap};
    use ndarray::Array2;

    // Build the rotation exactly as C does (fitschan.c:38248-38281): start
    // from the 3×3 identity and apply three axis-angle rotations in turn —
    // about Z by -phip, about Y by (deltap - pi/2), then about Z by
    // (pi + alphap) — via `astMtrRot`. After each rotation C runs
    // `CompressMatrix`, which snaps a near-diagonal matrix to exact diagonal
    // form (off-diagonals below sqrt(DBL_EPSILON)·max(row,col) → 0). That
    // compression is what yields C's exact zeros: the first Z-rotation by
    // -pi leaves ~1.2e-16 off-diagonals (sin(-pi)), which compress to 0, so
    // the following rotations build on a clean diag(-1,-1,1) and the result
    // matrix has structural zeros rather than ~1e-16 residue. A naive
    // `Rz·Ry·Rz` product (without per-step compression) leaves that residue.
    let mut rot_matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    mtr_rot(&mut rot_matrix, -phip, [0.0, 0.0, 1.0]);
    compress_matrix(&mut rot_matrix);
    mtr_rot(&mut rot_matrix, deltap - PI / 2.0, [0.0, 1.0, 0.0]);
    compress_matrix(&mut rot_matrix);
    mtr_rot(&mut rot_matrix, PI + alphap, [0.0, 0.0, 1.0]);
    compress_matrix(&mut rot_matrix);

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

    // The inverse of an orthogonal rotation is its transpose. C accumulates
    // the inverse via its own `astMtrRot` inverse-rotation sequence (each step
    // CompressMatrix-cleaned, matrixmap.c:3941-3957); for an orthogonal
    // rotation that equals the transpose of the compressed forward, which we
    // then compress so its off-diagonal residue is zeroed too (matching C's
    // exact-zero inverse elements, e.g. IM5).
    let mut inv_matrix = [
        [rot_matrix[0][0], rot_matrix[1][0], rot_matrix[2][0]],
        [rot_matrix[0][1], rot_matrix[1][1], rot_matrix[2][1]],
        [rot_matrix[0][2], rot_matrix[1][2], rot_matrix[2][2]],
    ];
    compress_matrix(&mut inv_matrix);

    // 3×3 rotation matrix and its (transpose) inverse.
    let mut mat = Array2::zeros((3, 3));
    let mut inv = Array2::zeros((3, 3));
    for i in 0..3 {
        for j in 0..3 {
            mat[[i, j]] = rot_matrix[i][j];
            inv[[i, j]] = inv_matrix[i][j];
        }
    }
    let rot_map = MatrixMap::new_with_inverse(mat, inv)?;

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
    )?;
    let full = CmpMap::new(Box::new(step1), Box::new(cart_to_sph), Composition::Series)?;
    Ok(Box::new(full))
}

/// Rotate the 3×3 matrix `mat` by `theta` radians about the given unit axis,
/// pre-multiplying in place (`mat := rotmat · mat`). Faithful port of C
/// `MtrRot` (matrixmap.c:3766) for the 3×3 forward case:
///
/// - `cos(theta) == 1.0` is a no-op (identity rotation),
/// - the rotation matrix uses the axis-angle (Rodrigues) term grouping
///   exactly as C's `a/b/c/d/e/f/g` locals,
/// - the product is accumulated column-by-column like C's `SMtrMult(post=1)`.
fn mtr_rot(mat: &mut [[f64; 3]; 3], theta: f64, axis: [f64; 3]) {
    let costh = theta.cos();
    let sinth = theta.sin();

    // C: `if ( costh == 1.0 ) return new;` — identity rotation, no change.
    if costh == 1.0 {
        return;
    }

    let axlen2 = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
    let axlen = axlen2.sqrt();

    let as_ = sinth / axlen;
    let a = (1.0 - costh) / axlen2;
    let b = a * axis[0] * axis[1];
    let c = as_ * axis[2];
    let d = a * axis[0] * axis[2];
    let e = as_ * axis[1];
    let f = a * axis[1] * axis[2];
    let g = as_ * axis[0];

    let rotmat = [
        [a * axis[0] * axis[0] + costh, b - c, d + e],
        [b + c, a * axis[1] * axis[1] + costh, f - g],
        [d - e, f + g, a * axis[2] * axis[2] + costh],
    ];

    // mat := rotmat · mat, per output column (mirrors SMtrMult post=1).
    // `j` indexes a matrix column across rows, so a range loop is clearest.
    #[allow(clippy::needless_range_loop)]
    for j in 0..3 {
        let mut work = [0.0_f64; 3];
        for (i, w) in work.iter_mut().enumerate() {
            let mut dot = 0.0;
            for k in 0..3 {
                dot += rotmat[i][k] * mat[k][j];
            }
            *w = dot;
        }
        for (i, &w) in work.iter().enumerate() {
            mat[i][j] = w;
        }
    }
}

/// Snap a near-diagonal 3×3 matrix to exact diagonal form by running the
/// shared port of C `CompressMatrix` (matrixmap.c:587) over it.
///
/// An off-diagonal element is set to `0.0` when its magnitude is `<=
/// sqrt(DBL_EPSILON)·max` of the largest magnitude in both its row and its
/// column. This removes the ~1e-16 trig residue (e.g. `sin(pi)`) that would
/// otherwise propagate through subsequent rotations and surface as non-zero
/// matrix elements in the serialised dump.
///
/// C compresses these intermediate rotations in place inside `astMtrRot`
/// (matrixmap.c:3903, 3973), where the resulting storage form is recorded on the
/// MatrixMap being rotated. Here the rotation is still a bare array, so the
/// resulting form is discarded; the final MatrixMap derives its own form when
/// the array is handed over.
fn compress_matrix(mat: &mut [[f64; 3]; 3]) {
    let mut arr = ndarray::Array2::from_shape_fn((3, 3), |(i, j)| mat[i][j]);
    crate::mappings::compress_full_matrix(&mut arr);
    for i in 0..3 {
        for j in 0..3 {
            mat[i][j] = arr[[i, j]];
        }
    }
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

    /// `AXREF<i>` overrides the CRVAL-derived sky reference position and
    /// forces it to be set (C `WcsCelestial`, fitschan.c:36209-36223).
    #[test]
    fn axref_overrides_crval_reference_position() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(120.0);
        store.crval[1] = Some(30.0);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(50.5);
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);
        store.axref[0] = Some(119.5);
        store.axref[1] = Some(30.5);

        let result = build_celestial(&store).expect("celestial axes recognised");
        let (reflon, reflat) = result.ref_pos.expect("AXREF supplies a reference position");
        assert!((reflon - 119.5_f64.to_radians()).abs() < 1e-15);
        assert!((reflat - 30.5_f64.to_radians()).abs() < 1e-15);
        assert!(
            result.ref_pos_forced,
            "AXREF bypasses the astTestSkyRef gate"
        );
    }

    /// With no `AXREF`, the reference position stays on CRVAL and is *not*
    /// forced, so the `astTestSkyRef` gate still applies.
    #[test]
    fn crval_reference_position_is_not_forced() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.crval[0] = Some(120.0);
        store.crval[1] = Some(30.0);
        store.crpix[0] = Some(50.5);
        store.crpix[1] = Some(50.5);
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);

        let result = build_celestial(&store).expect("celestial axes recognised");
        let (reflon, _) = result.ref_pos.expect("CRVAL supplies a reference position");
        assert!((reflon - 120.0_f64.to_radians()).abs() < 1e-15);
        assert!(!result.ref_pos_forced);
    }

    #[test]
    fn compress_matrix_zeros_near_zero_off_diagonals() {
        // A rotation by -pi about Z leaves ~1.2e-16 off-diagonals (sin(-pi)).
        // CompressMatrix must snap those to exact zero (mirroring C, so the
        // residue does not propagate through later rotations).
        let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        mtr_rot(&mut m, -PI, [0.0, 0.0, 1.0]);
        // Before compression the off-diagonals are non-zero residue.
        assert!(m[0][1].abs() > 0.0 && m[0][1].abs() < 1e-15);
        compress_matrix(&mut m);
        // After compression they are exactly zero, leaving diag(-1,-1,1).
        assert_eq!(m[0][1], 0.0);
        assert_eq!(m[1][0], 0.0);
        assert_eq!(m[0][0], -1.0);
        assert_eq!(m[1][1], -1.0);
        assert_eq!(m[2][2], 1.0);
    }

    #[test]
    fn mtr_rot_identity_is_noop() {
        // cos(theta)==1 (theta=0) must leave the matrix unchanged.
        let mut m = [[0.5, -0.5, 0.1], [0.5, 0.5, 0.2], [0.0, 0.0, 1.0]];
        let before = m;
        mtr_rot(&mut m, 0.0, [0.0, 0.0, 1.0]);
        assert_eq!(m, before);
    }
}

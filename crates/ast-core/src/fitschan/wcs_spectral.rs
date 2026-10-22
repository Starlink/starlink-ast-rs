//! Spectral axis decoder for FITS-WCS.
//!
//! If a spectral axis is present in the [`FitsStore`], this module builds the
//! IWC-to-spectral mapping and creates a [`SpecFrame`]. The pipeline follows
//! the C library's `WcsSpectral` function (fitschan.c ~line 39364).
//!
//! For a linear FITS-WCS spectral axis the IWC value represents the offset
//! from CRVAL in the spectral quantity's native units; the IWC→S mapping
//! is `IWC + CRVAL`. For non-linear `-X2P` axes (FITS-WCS paper III) the
//! axis is linearly sampled in some other system X (frequency, wavelength,
//! air wavelength, or relativistic velocity), so the IWC→S mapping is
//! `IWC → X (via WinMap or ZoomMap) → S (via SpecMap)`. See
//! [`nonlin_spec_wcs`] which mirrors C `NonLinSpecWcs` (fitschan.c:23597).

use super::fits_store::FitsStore;
use super::warning::{Warning, Warnings};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{Frame, FrameRef, SpecFrame, SpecSystem, StdOfRest};
use crate::mappings::{CmpMap, Composition, DynMapping, GrismMap, Mapping, ShiftMap, ZoomMap};

/// Degrees to radians. Mirrors C's `AST__DD2R` (ast_par.h).
const DD2R: f64 = std::f64::consts::PI / 180.0;

/// Result of building the spectral axis from a FitsStore.
pub(crate) struct SpectralResult {
    /// Mapping from the axis's IWC value to its spectral coordinate.
    pub mapping: DynMapping,
    /// The SpecFrame describing the output spectral coordinate.
    pub frame: SpecFrame,
    /// 0-based axis index in the full IWC space.
    pub axis: usize,
    /// The `DSBSpecFrame`, set up by `DSBSetUp`, that is this axis's world
    /// Frame when the FitsStore carries an `IMAGFREQ` value (fitschan.c
    /// `WcsSpectral`); otherwise the world Frame is [`Self::frame`].
    pub dsb: Option<crate::frames::DSBSpecFrame>,
    /// Warning messages to emit as ASTWARN cards. Mirrors C `Warn`'s
    /// `badctype` condition: the `-X2P` algorithm code's `P` character
    /// disagrees with the one the axis's own system implies
    /// (fitschan.c:23807-23815).
    pub warnings: Warnings,
}

impl SpectralResult {
    /// A copy of this axis's world Frame.
    pub fn world_frame(&self) -> crate::frames::DynFrame {
        match &self.dsb {
            Some(dsb) => Box::new(dsb.clone()),
            None => Box::new(self.frame.clone()),
        }
    }
}

/// The celestial reference position C's `WcsSpectral` gives each
/// SpecFrame with `astSetRefPos`: the SkyFrame `WcsCelestial` made, and the
/// reference longitude and latitude in it, in radians.
pub(crate) type RefPos<'a> = (&'a crate::frames::SkyFrame, f64, f64);

/// Build the IWC-to-spectral Mapping and the Frame of every spectral axis,
/// in axis order, as C's `WcsSpectral` does in its loop over the axes
/// (fitschan.c `WcsSpectral`). An axis whose CTYPE is not a spectral system
/// is passed over.
///
/// An algorithm code that cannot be implemented is AST__BDFTS, as C reports
/// it, and ends the loop. `s` is the axis description's version character,
/// and `reference` the celestial reference position, when there are
/// celestial axes. Each axis's warnings are collected as `policy` collects
/// them, so with Strict in force a reported one ends the loop as an error.
pub(crate) fn build_spectral(
    store: &FitsStore,
    s: char,
    reference: Option<RefPos<'_>>,
    policy: &Warnings,
) -> AstResult<Vec<SpectralResult>> {
    let mut result = Vec::new();
    for axis in 0..store.naxis {
        let spectral = store.ctype[axis]
            .as_deref()
            .is_some_and(super::fits_store::is_spectral_ctype);
        if spectral && let Some(r) = build_spectral_axis(store, s, axis, reference, policy)? {
            result.push(r);
        }
    }
    Ok(result)
}

/// One pass of C's `WcsSpectral` loop, for the spectral axis `axis`.
fn build_spectral_axis(
    store: &FitsStore,
    s: char,
    axis: usize,
    reference: Option<RefPos<'_>>,
    policy: &Warnings,
) -> AstResult<Option<SpectralResult>> {
    // Determine the spectral system + algorithm code from CTYPE.
    let Some((system, algcode)) = parse_spectral_ctype(store, axis) else {
        return Ok(None);
    };

    // Step 3: Build the SpecFrame (so we can pass it to the algorithm
    // dispatch, which may need its rest frequency and unit).
    let mut frame = SpecFrame::new(system);

    // The reference position, if there are celestial axes.
    if let Some((sky, reflon, reflat)) = reference {
        frame.set_ref_pos(Some(sky), reflon, reflat)?;
    }

    if let Some(ref specsys) = store.specsys {
        if let Some(sor) = StdOfRest::parse(specsys) {
            // A new SpecFrame has no SpecOrigin to re-express.
            frame
                .set_std_of_rest(sor)
                .expect("a SpecFrame with no SpecOrigin accepts any StdOfRest");
        }
    }

    if let Some(restfrq) = store.restfrq {
        frame.set_rest_freq(restfrq);
    } else if let Some(restwav) = store.restwav {
        const SPEED_OF_LIGHT: f64 = 299_792_458.0; // m/s
        let restfrq = SPEED_OF_LIGHT / restwav;
        frame.set_rest_freq(restfrq);
    }

    // The date of observation, as C's `ChooseEpoch` gives it.
    let mut epoch_warnings = policy.child();
    if let Some(mjd) = super::epoch::choose_epoch(store, &mut epoch_warnings)? {
        frame.set_epoch_mjd(mjd);
    }

    // OBSGEO-X/Y/Z → ObsLon/ObsLat/ObsAlt. Mirrors C
    // fitschan.c:39667-39683.
    if let (Some(x), Some(y), Some(z)) = (store.obsgeox, store.obsgeoy, store.obsgeoz)
        && let Some((geolon, geolat, h)) = crate::pal::gc2gd([x, y, z])
    {
        frame.set_obs_lon(geolon);
        frame.set_obs_lat(geolat);
        frame.set_obs_alt(h);
    }

    // Source velocity rest frame (SSYSSRC → SourceVRF) and source
    // velocity (ZSOURCE redshift → SourceVel). Mirrors C `WcsSpectral`
    // (fitschan.c:39680-39692). The source velocity defines the
    // Source standard of rest, which DSBSetUp's source-rest →
    // topocentric conversion depends on; without it the topocentric
    // image/rest frequencies (and hence the derived IF) are wrong.
    if let Some(ref ssyssrc) = store.ssyssrc
        && let Some(sor) = StdOfRest::parse(ssyssrc)
    {
        frame.set_source_vrf(sor)?;
    }
    if let Some(zsource) = store.zsource {
        // Convert redshift to velocity (relativistic Doppler), mirroring
        // C: v = c·((1+z)² − 1)/((1+z)² + 1).
        const SPEED_OF_LIGHT: f64 = 299_792_458.0; // m/s (AST__C)
        let mut v = zsource + 1.0;
        v *= v;
        let vsource = SPEED_OF_LIGHT * (v - 1.0) / (v + 1.0);
        frame.set_source_vel(vsource);
    }

    // CUNIT → SpecFrame unit. Recorded as the user-facing presentation
    // unit; the SpecMap that connects two SpecFrames handles the SI
    // conversion when systems differ. Mirrors specframe.c::SetUnit.
    //
    // C `WcsSpectral` (fitschan.c:39642-39650) always calls
    // `astSetUnit(specfrm, 0, cunit)`, falling back to the FITS-WCS
    // Paper III default unit (the value `IsSpectral` returns — "Hz" for
    // FREQ, "m" for WAVE, "m/s" for VRAD/VOPT/VELO, etc) when no
    // CUNIT keyword is present. Without this fallback the SpecFrame
    // carries no explicit Unit, and the writer falls back to the
    // SpecSystem's display default ("GHz" for FREQ) — diverging from
    // the C library's CUNIT round-trip.
    let cunit = store
        .cunit
        .get(axis)
        .and_then(|v| v.as_deref())
        .filter(|u| !u.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| system.specmap_units().to_string());
    if !cunit.is_empty() {
        frame.set_unit(1, &cunit)?;
    }

    // CNAME → axis Label. Mirrors C `WcsSpectral` (fitschan.c:39769-
    // 39638): when the CNAME keyword is set for the spectral axis, it
    // becomes the SpecFrame's (sole) axis Label.
    if let Some(cname) = &store.cname[axis] {
        frame.set_label(1, cname)?;
    }

    // Step 4: Build the IWC→S mapping, branching on the algorithm code as
    // C `WcsSpectral` does (fitschan.c:40138-40168). A code that is not
    // recognised, or whose parameters cannot be used, is an error.
    let crval = store.crval[axis].unwrap_or(0.0);
    let mut warnings: Warnings = epoch_warnings;
    let mapping: Option<DynMapping> = if algcode.is_empty() {
        Some(Box::new(ShiftMap::new(vec![crval])))
    } else if algcode == "-LOG" {
        log_wcs(crval)
    } else if is_nonlin_x2p(&algcode) {
        let (map, warns) = nonlin_spec_wcs(&algcode, &frame, crval, policy)?;
        warnings.append(warns);
        map
    } else if algcode == "-GRI" || algcode == "-GRA" {
        grism_spec_wcs(&algcode, store, axis, &frame)?
    } else {
        None
    };
    let Some(mapping) = mapping else {
        let key = super::wcs_keywords::key_for("CTYPE", Some(axis + 1), s);
        return Err(AstError::new(
            ErrorCode::Bdfts,
            format!(
                "astRead(FitsChan): Cannot implement spectral algorithm code '{algcode}' \
                 specified in FITS keyword '{key}'."
            ),
        ));
    };

    // C builds a DSBSpecFrame instead of a plain SpecFrame when the
    // FitsStore carries an IMAGFREQ value, and sets it up with `DSBSetUp`,
    // using AXREF for the observation centre in preference to CRVAL.
    let dsb = match store.imagfreq {
        Some(imagfreq) => {
            let mut dsb = crate::frames::DSBSpecFrame::from_spec_frame(frame.clone());
            let obscentre = store.axref[axis].or(store.crval[axis]).unwrap_or(0.0);
            dsb.dsb_set_up(imagfreq, obscentre)?;
            Some(dsb)
        }
        None => None,
    };

    Ok(Some(SpectralResult {
        mapping,
        frame,
        axis,
        dsb,
        warnings,
    }))
}

/// Whether the algorithm code is the FITS-WCS paper III "-X2P" non-linear
/// form (e.g. `-F2W`, `-V2F`).
///
/// Mirrors C's check at fitschan.c:40148 — the code starts with `'-'` and
/// has `'2'` at position 2.
fn is_nonlin_x2p(algcode: &str) -> bool {
    let bytes = algcode.as_bytes();
    bytes.len() >= 3 && bytes[0] == b'-' && bytes[2] == b'2'
}

/// Build the IWC→S mapping for a FITS-WCS paper III non-linear `-X2P`
/// spectral axis.
///
/// Mirrors `fitschan.c::NonLinSpecWcs` (line 23486). The axis is linearly
/// sampled in system `X` (frequency/wavelength/air-wavelength/relativistic
/// velocity, identified by `algcode[1]`) but expressed in the SpecFrame's
/// system `S`. The resulting mapping is `IWC → X (via WinMap or
/// ZoomMap) → S (via SpecMap S↔X inverted)`.
///
/// The mapping is `None` when the algorithm code's X system is unrecognised
/// or when the X-to-S rate of change is zero/undefined at the reference point.
/// The warnings are returned separately because C's `Warn` appends its cards
/// to the FitsChan as it runs, so a warning raised before a later failure
/// still reaches the header even though the mapping does not.
fn nonlin_spec_wcs(
    algcode: &str,
    specfrm: &SpecFrame,
    crval: f64,
    policy: &Warnings,
) -> AstResult<(Option<DynMapping>, Warnings)> {
    // Identify the "X" system from algcode[1] and build a SpecFrame
    // copy describing X. Mirrors C:23579-23600. Copying the supplied
    // SpecFrame ensures rest_freq, std_of_rest, etc. are preserved.
    let bytes = algcode.as_bytes();
    let x_system = match bytes.get(1).copied() {
        Some(b'F') => SpecSystem::Freq,
        Some(b'W') => SpecSystem::Wavelen,
        Some(b'V') => SpecSystem::Vrel,
        Some(b'A') => SpecSystem::Airwave,
        _ => return Ok((None, policy.child())),
    };
    let mut xfrm = specfrm.clone();
    xfrm.set_system(x_system)?;
    xfrm.set_unit(1, x_system.specmap_units())?;

    // Build the S → X mapping via Frame::convert (mirrors
    // C:23606-23612). This returns a FrameSet whose forward mapping
    // takes S values to X values.
    let fs = crate::frames::find_frame::convert(specfrm, &xfrm, &[])?;

    // Warn when the algorithm code's "P" character disagrees with the one the
    // axis's own system implies. Mirrors C:23675-23704, which runs this whether
    // or not the conversion above succeeded — the only guard is that the X
    // system was identified. AST interprets the illegal "P" system correctly,
    // so nothing about the mapping changes.
    let mut warnings = policy.child();
    let s_sys = specfrm.system();
    let pc = match s_sys {
        // C's final `else` raises AST__INTER because it cannot prove the four
        // groups cover every system. They do, so an exhaustive match makes
        // that a compile-time guarantee and the branch unreachable.
        SpecSystem::Freq | SpecSystem::Energy | SpecSystem::Wavenum | SpecSystem::Vradio => b'F',
        SpecSystem::Wavelen | SpecSystem::Voptical | SpecSystem::Redshift => b'W',
        SpecSystem::Airwave => b'A',
        SpecSystem::Beta | SpecSystem::Vrel => b'V',
    };
    if bytes.get(3).copied() != Some(pc) {
        // C's format is "%s%s ... %s%.3s%c": the system name, the full
        // algorithm code, then the system name, the code's first three
        // characters, and the required "P" character.
        let text = s_sys.as_str();
        let stem = &algcode[..algcode.len().min(3)];
        warnings.push(Warning::new(
            "badctype",
            format!(
                "The spectral CTYPE value {text}{algcode} is not legal - using \
             {text}{stem}{} instead.",
                pc as char
            ),
        ))?;
    }

    let Some(fs) = fs else {
        return Ok((None, warnings));
    };
    let s_to_x = fs.mapping(FrameRef::Base, FrameRef::Current)?;

    // Convert CRVAL (in S) to its equivalent in X. Mirrors C:23659.
    let crv_x = {
        let pts = crate::coords::PointSet::from_rows([[crval]]);
        let out = s_to_x.transform(&pts, true)?;
        out.point(0)[0]
    };

    // Invert so the forward direction takes X to S. Mirrors C:23663.
    let mut x_to_s = s_to_x;
    x_to_s.invert();

    // Rate of change of S with respect to X at X = crv_x. Mirrors
    // C:23667 — `astRate(map1, &crv_x, 0, 0)` after the invert.
    let ds = match x_to_s.rate(&[crv_x], 1, 1) {
        Ok(r) if r != 0.0 && r.is_finite() => r,
        _ => return Ok((None, warnings)),
    };

    // FITS-WCS paper III: dS/dw = 1 at the reference point, so
    // dX/dw = dX/dS = 1/ds. Build the IWC→X map (1D). Mirrors
    // C:23676-23686:
    //   crv_x != 0:  WinMap with (in_a=0, in_b=crv_x*ds) → (out_a=crv_x, out_b=2*crv_x)
    //   crv_x == 0:  ZoomMap(1/ds)
    let iwc_to_x: DynMapping = if crv_x != 0.0 {
        let in_a = 0.0;
        let in_b = crv_x * ds;
        let out_a = crv_x;
        let out_b = 2.0 * crv_x;
        let win = crate::mappings::WinMap::from_corners(&[in_a], &[in_b], &[out_a], &[out_b])?;
        Box::new(win)
    } else {
        Box::new(ZoomMap::new(1, 1.0 / ds)?)
    };

    // IWC → S = (IWC → X) ∘ (X → S). Mirrors C:23691.
    let iwc_to_s = CmpMap::new(iwc_to_x, x_to_s, Composition::Series)?;
    Ok((Some(Box::new(iwc_to_s)), warnings))
}

/// Build the IWC→S mapping for a FITS-WCS paper III `-GRI` or `-GRA` grism axis.
///
/// Mirrors C `GrismSpecWcs` (fitschan.c:14306-14468). `-GRI` disperses in
/// vacuum wavelength, `-GRA` in air wavelength. The returned mapping is
/// `w → S`, built from a GrismMap whose parameters come from PVi_0..6.
///
/// Returns `Ok(None)` when the SpecFrame cannot be converted to wavelength,
/// or when the rate of change `dg/dS` is zero or undefined at the reference
/// point (mirrors C returning NULL when `dg == AST__BAD || dg == 0.0`).
fn grism_spec_wcs(
    algcode: &str,
    store: &FitsStore,
    axis: usize,
    specfrm: &SpecFrame,
) -> AstResult<Option<DynMapping>> {
    // Determine whether we want vacuum wavelength (-GRI) or air wavelength
    // (-GRA). Mirrors C:14299.
    let wave_system = if algcode == "-GRI" {
        SpecSystem::Wavelen
    } else {
        SpecSystem::Airwave
    };

    // Copy the supplied SpecFrame (to inherit std_of_rest, epoch, etc.),
    // then set System to wavelength and Unit to metres. Mirrors C:14298-14300.
    let mut wfrm = specfrm.clone();
    wfrm.set_system(wave_system)?;
    wfrm.set_unit(1, "m")?;

    // Get the S → wavelength mapping via astConvert. Mirrors C:14301-14304.
    let Some(fs) = crate::frames::find_frame::convert(specfrm, &wfrm, &[])? else {
        return Ok(None);
    };
    let smap = fs.mapping(FrameRef::Base, FrameRef::Current)?;

    // CRVAL for the spectral axis (in S units), default 0. Mirrors C:14307-14308.
    let crv = store.crval[axis].unwrap_or(0.0);

    // Convert CRVAL to the wavelength system (vacuum or air) in metres.
    // Mirrors C:14311.
    let wcrv = {
        let out = smap.transform(&crate::coords::PointSet::from_rows([[crv]]), true)?;
        out.point(0)[0]
    };

    // Helper: read PVi_m from store (0-based axis, m). Absent → None.
    // Mirrors C `GetItem(&(store->pv), i, m, ...)` which returns AST__BAD
    // when absent.
    let pv =
        |m: usize| -> Option<f64> { store.pv.get(axis).and_then(|v| v.get(m)).copied().flatten() };

    // Build a GrismMap with parameters from PVi_0..6 and WaveR = wcrv.
    // Defaults and degrees→radians conversions mirror C:14323-14340.
    let g = pv(0).unwrap_or(0.0);
    // Round to nearest, not toward zero: a grism interference order is
    // commonly negative, and `(v + 0.5) as i32` turns -1 into 0
    // (fitschan.c:14422, fixed upstream at 1199880a).
    let m_order = pv(1).map(|v| v.round() as i32).unwrap_or(0);
    let alpha = pv(2).map(|v| v * DD2R).unwrap_or(0.0);
    let nr = pv(3).unwrap_or(1.0);
    let nrp = pv(4).unwrap_or(0.0);
    let eps = pv(5).map(|v| v * DD2R).unwrap_or(0.0);
    let theta = pv(6).map(|v| v * DD2R).unwrap_or(0.0);
    let gmap = GrismMap::new(nr, nrp, wcrv, alpha, g, m_order, eps, theta);

    // Invert the GrismMap to get wavelength → grism-parameter. Then compose
    // with smap (S → wavelength) to get map1 = S → grism-parameter.
    // Mirrors C:14341-14342.
    let mut gmap_inv = gmap.clone();
    gmap_inv.invert();
    let map1 = CmpMap::new(smap, Box::new(gmap_inv) as DynMapping, Composition::Series)?;

    // Convert the reference wavelength to grism-parameter. Mirrors C:14345.
    // Use a fresh inverted copy of gmap (forward: grism-param → wavelength;
    // inverted: wavelength → grism-param).
    let gcrv = {
        let mut gmap_inv2 = gmap.clone();
        gmap_inv2.invert();
        let out = gmap_inv2.transform(&crate::coords::PointSet::from_rows([[wcrv]]), true)?;
        out.point(0)[0]
    };

    // Rate of change dg/dS at the reference point. Mirrors C:14349-14350.
    let dg = match map1.rate(&[crv], 1, 1) {
        Ok(r) if r != 0.0 && r.is_finite() => r,
        _ => return Ok(None),
    };

    // FITS-WCS paper III: dS/dw = 1 at the reference point, so dg/dw = dg/dS.
    // Build map2 = ZoomMap(dg) ∘ ShiftMap(gcrv): w → grism-parameter.
    // Mirrors C:14355-14358.
    let map2a: DynMapping = Box::new(ZoomMap::new(1, dg)?);
    let map2b: DynMapping = Box::new(ShiftMap::new(vec![gcrv]));
    let map2 = CmpMap::new(map2a, map2b, Composition::Series)?;

    // Invert map1 to get grism-parameter → S, then compose map2 with it
    // to get the returned w → S mapping. Mirrors C:14363-14364.
    let mut map1_inv = map1;
    map1_inv.invert();
    let ret = CmpMap::new(
        Box::new(map2) as DynMapping,
        Box::new(map1_inv) as DynMapping,
        Composition::Series,
    )?;
    Ok(Some(Box::new(ret)))
}

/// Build the IWC→S mapping for a FITS-WCS paper III `-LOG` spectral axis.
///
/// Mirrors C `LogWcs` (fitschan.c:20777-20859): S = Sr*exp(w/Sr) forward,
/// w = Sr*log(S/Sr) inverse, where Sr is the axis CRVAL. A zero Sr has no
/// logarithmic form, so C returns NULL and the axis stays linear.
pub(super) fn log_wcs(crval: f64) -> Option<DynMapping> {
    // crval is already resolved (upstream `unwrap_or(0.0)` substitutes the
    // C AST__BAD default), so test == 0.0 directly, matching C after its
    // `if( crv == AST__BAD ) crv = 0.0;`.
    if crval == 0.0 {
        return None;
    }
    // C uses "%.*g" with AST__DBL_DIG (=17); format_double_g reproduces that
    // exactly so the serialised MathMap expression is byte-identical.
    let sr = crate::serial::native_write::format_double_g(crval, 17);
    let forexp = format!("s={sr}*exp(w/{sr})");
    let invexp = format!("w={sr}*log(s/{sr})");
    // C LogWcs sets SIMPFI=1 and SIMPIF=1 (fitschan.c:20852-20853), mirrored
    // here so the serialised native output is byte-identical. These flags only
    // affect the simplifier, not the transform itself.
    let mut map = crate::mappings::MathMap::new(1, 1, &[&forexp], &[&invexp]).ok()?;
    map.set_simp_fi(true);
    map.set_simp_if(true);
    Some(Box::new(map))
}

/// Parse CTYPE into (spectral system, algorithm code).
///
/// The first four characters give the spectral system code (FREQ, WAVE,
/// VOPT, …); any remaining characters form the algorithm code (e.g.
/// `-F2W`, `-LOG`, empty for a linear axis). Mirrors C `IsSpectral`
/// (fitschan.c:20419-20548).
fn parse_spectral_ctype(store: &FitsStore, axis: usize) -> Option<(SpecSystem, String)> {
    let ctype = store.ctype[axis].as_ref()?;
    if ctype.len() < 4 {
        return None;
    }
    let prefix = &ctype[..4];
    let algcode = if ctype.len() > 4 {
        ctype[4..].trim_end().to_string()
    } else {
        String::new()
    };
    let system = SpecSystem::parse(prefix)?;
    Some((system, algcode))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::fits_store::FitsStore;

    /// The first spectral axis `build_spectral` finds, with no celestial
    /// reference position.
    fn first(store: &FitsStore) -> Option<SpectralResult> {
        build_spectral(store, ' ', None, &Warnings::new())
            .unwrap()
            .into_iter()
            .next()
    }

    #[test]
    fn detects_freq_axis() {
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[2] = Some(1.4e9); // 1.4 GHz

        let result = first(&store);
        assert!(result.is_some());
        let spec = result.unwrap();
        assert_eq!(spec.axis, 2);
        assert_eq!(spec.frame.system(), SpecSystem::Freq);
        assert_eq!(spec.mapping.nin(), 1);
        assert_eq!(spec.mapping.nout(), 1);
    }

    #[test]
    fn detects_wave_axis() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("WAVE".to_string());
        store.crval[0] = Some(5.0e-7); // 500nm in metres

        let result = first(&store).unwrap();
        assert_eq!(result.frame.system(), SpecSystem::Wavelen);
    }

    #[test]
    fn detects_vrad_axis() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("VRAD".to_string());
        store.crval[0] = Some(0.0);

        let result = first(&store).unwrap();
        assert_eq!(result.frame.system(), SpecSystem::Vradio);
    }

    #[test]
    fn sets_std_of_rest() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.0e9);
        store.specsys = Some("LSRK".to_string());

        let result = first(&store).unwrap();
        assert_eq!(result.frame.std_of_rest(), StdOfRest::Lsrk);
    }

    #[test]
    fn sets_rest_freq() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("VRAD".to_string());
        store.crval[0] = Some(0.0);
        store.restfrq = Some(1.420405751786e9);

        let result = first(&store).unwrap();
        assert!((result.frame.rest_freq() - 1.420405751786e9).abs() < 1.0);
    }

    #[test]
    fn no_spectral_returns_none() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());

        let result = first(&store);
        assert!(result.is_none());
    }

    #[test]
    fn crval_shift_mapping() {
        use crate::coords::PointSet;

        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.4e9);

        let result = first(&store).unwrap();
        // IWC = 0 should map to CRVAL.
        let pts = PointSet::from_columns(&[&[0.0]]).unwrap();
        let out = result.mapping.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 1.4e9).abs() < 1.0);

        // IWC = 1e6 should map to CRVAL + 1e6.
        let pts2 = PointSet::from_columns(&[&[1.0e6]]).unwrap();
        let out2 = result.mapping.transform(&pts2, true).unwrap();
        assert!((out2.point(0)[0] - (1.4e9 + 1.0e6)).abs() < 1.0);
    }

    #[test]
    fn every_spectral_axis_in_axis_order() {
        // Two spectral axes, as C's `WcsSpectral` loop finds them: one
        // result each, in axis order, each with a 1-D Mapping.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("WAVE".to_string());
        store.ctype[1] = Some("RA---TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[0] = Some(1.0e-6);
        store.crval[2] = Some(1.4e9);

        let all = build_spectral(&store, ' ', None, &Warnings::new()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(
            (all[0].axis, all[0].frame.system()),
            (0, SpecSystem::Wavelen)
        );
        assert_eq!((all[1].axis, all[1].frame.system()), (2, SpecSystem::Freq));
        assert!(
            all.iter()
                .all(|r| r.mapping.nin() == 1 && r.mapping.nout() == 1)
        );
    }
}

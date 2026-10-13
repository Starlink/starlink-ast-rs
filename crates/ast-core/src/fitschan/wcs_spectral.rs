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
//! [`nonlin_spec_wcs`] which mirrors C `NonLinSpecWcs` (fitschan.c:23591).

use super::fits_store::FitsStore;
use super::warning::{Warning, Warnings};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{Frame, FrameRef, SpecFrame, SpecSystem, StdOfRest};
use crate::mappings::{
    CmpMap, Composition, DynMapping, GrismMap, Mapping, ShiftMap, UnitMap, ZoomMap,
};

/// Degrees to radians. Mirrors C's `AST__DD2R` (ast_par.h).
const DD2R: f64 = std::f64::consts::PI / 180.0;

/// Embed a 1-axis mapping into an N-axis mapping with `UnitMap`s on
/// the other axes. Identity-fast for `naxis == 1`. Mirrors C
/// `WcsSpectral` (fitschan.c:39783-39798), which composes one 1D map
/// per axis in parallel in FITS order (`ret = astCmpMap(ret, map1, 0,
/// …)`) — the non-spectral axes carry a `UnitMap(1)` each and only the
/// *Frame* is permuted, so no PermMap sandwich is needed here.
fn embed_1axis_in_naxis(
    inner: DynMapping,
    naxis: usize,
    axis_0based: usize,
) -> AstResult<DynMapping> {
    debug_assert_eq!(inner.nin(), 1);
    debug_assert_eq!(inner.nout(), 1);
    debug_assert!(axis_0based < naxis);

    if naxis == 1 {
        return Ok(inner);
    }

    let mut inner = Some(inner);
    let mut result: Option<DynMapping> = None;
    for k in 0..naxis {
        let part: DynMapping = if k == axis_0based {
            inner.take().unwrap()
        } else {
            Box::new(UnitMap::new(1))
        };
        result = Some(match result {
            None => part,
            Some(acc) => Box::new(CmpMap::new(acc, part, Composition::Parallel)?),
        });
    }
    Ok(result.unwrap())
}

/// Result of building the spectral axis from a FitsStore.
pub(crate) struct SpectralResult {
    /// Mapping from IWC (spectral subset, 1D) to spectral coordinate.
    pub mapping: DynMapping,
    /// The SpecFrame describing the output spectral coordinate.
    pub frame: SpecFrame,
    /// 0-based axis index in the full IWC space.
    pub axis: usize,
    /// Whether the world frame for this axis should be a `DSBSpecFrame`
    /// rather than a plain `SpecFrame`. Mirrors C `WcsSpectral`
    /// (fitschan.c:39620-39625), which builds a DSBSpecFrame when the
    /// FitsStore carries an `IMAGFREQ` value. The DSB-specific attributes
    /// (IF / DSBCentre / SideBand) are filled in by `dsb_set_up` after the
    /// reference position has been propagated from the celestial axes.
    pub is_dsb: bool,
    /// Warning messages to emit as ASTWARN cards. Mirrors C `Warn`'s
    /// `badctype` condition: the `-X2P` algorithm code's `P` character
    /// disagrees with the one the axis's own system implies
    /// (fitschan.c:23801-23809).
    pub warnings: Warnings,
}

impl SpectralResult {
    /// Build the world Frame for this spectral axis: a `DSBSpecFrame`
    /// wrapping [`Self::frame`] when [`Self::is_dsb`] is set, otherwise a
    /// plain clone of [`Self::frame`].
    pub fn world_frame(&self) -> crate::frames::DynFrame {
        if self.is_dsb {
            Box::new(crate::frames::DSBSpecFrame::from_spec_frame(
                self.frame.clone(),
            ))
        } else {
            Box::new(self.frame.clone())
        }
    }
}

/// If a spectral axis is present in the FitsStore, build the IWC-to-spectral
/// mapping and create a SpecFrame.
///
/// Returns `None` if no spectral axis is found, and AST__BDFTS for an
/// algorithm code that cannot be implemented, as C's `WcsSpectral` reports
/// it (fitschan.c:40149-40157). `s` is the axis description's version
/// character.
pub(crate) fn build_spectral(store: &FitsStore, s: char) -> AstResult<Option<SpectralResult>> {
    // Step 1: Find the spectral axis.
    let Some(axis) = store.spectral_axis() else {
        return Ok(None);
    };

    // Step 2: Determine the spectral system + algorithm code from CTYPE.
    let Some((system, algcode)) = parse_spectral_ctype(store, axis) else {
        return Ok(None);
    };

    // Step 3: Build the SpecFrame (so we can pass it to the algorithm
    // dispatch, which may need its rest frequency and unit).
    let mut frame = SpecFrame::new(system);

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
    let mut epoch_warnings: Warnings = Vec::new();
    if let Some(mjd) = super::epoch::choose_epoch(store, &mut epoch_warnings)? {
        frame.set_epoch_mjd(mjd);
    }

    // OBSGEO-X/Y/Z → ObsLon/ObsLat/ObsAlt. Mirrors C
    // fitschan.c:39656-39672.
    if let (Some(x), Some(y), Some(z)) = (store.obsgeox, store.obsgeoy, store.obsgeoz)
        && let Some((geolon, geolat, h)) = crate::pal::gc2gd([x, y, z])
    {
        frame.set_obs_lon(geolon);
        frame.set_obs_lat(geolat);
        frame.set_obs_alt(h);
    }

    // Source velocity rest frame (SSYSSRC → SourceVRF) and source
    // velocity (ZSOURCE redshift → SourceVel). Mirrors C `WcsSpectral`
    // (fitschan.c:39669-39681). The source velocity defines the
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
    // C `WcsSpectral` (fitschan.c:39631-39639) always calls
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

    // CNAME → axis Label. Mirrors C `WcsSpectral` (fitschan.c:39758-
    // 39638): when the CNAME keyword is set for the spectral axis, it
    // becomes the SpecFrame's (sole) axis Label.
    if let Some(cname) = &store.cname[axis] {
        frame.set_label(1, cname)?;
    }

    // Step 4: Build the IWC→S mapping, branching on the algorithm code as
    // C `WcsSpectral` does (fitschan.c:40127-40157). A code that is not
    // recognised, or whose parameters cannot be used, is an error.
    let crval = store.crval[axis].unwrap_or(0.0);
    let mut warnings: Warnings = epoch_warnings;
    let mapping: Option<DynMapping> = if algcode.is_empty() {
        Some(Box::new(ShiftMap::new(vec![crval])))
    } else if algcode == "-LOG" {
        log_wcs(crval)
    } else if is_nonlin_x2p(&algcode) {
        let (map, warns) = nonlin_spec_wcs(&algcode, &frame, crval)?;
        warnings.extend(warns);
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

    let naxis = store.naxis;
    let mapping = embed_1axis_in_naxis(mapping, naxis, axis)?;

    // C `WcsSpectral` (fitschan.c:39620-39625) builds a DSBSpecFrame
    // instead of a plain SpecFrame when the FitsStore carries an IMAGFREQ
    // value. The DSB-specific attributes are computed by `dsb_set_up`
    // after the celestial reference position has been propagated.
    let is_dsb = store.imagfreq.is_some();

    Ok(Some(SpectralResult {
        mapping,
        frame,
        axis,
        is_dsb,
        warnings,
    }))
}

/// Whether the algorithm code is the FITS-WCS paper III "-X2P" non-linear
/// form (e.g. `-F2W`, `-V2F`).
///
/// Mirrors C's check at fitschan.c:40137 — the code starts with `'-'` and
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
        _ => return Ok((None, Vec::new())),
    };
    let mut xfrm = specfrm.clone();
    xfrm.set_system(x_system)?;
    xfrm.set_unit(1, x_system.specmap_units())?;

    // Build the S → X mapping via Frame::convert (mirrors
    // C:23606-23612). This returns a FrameSet whose forward mapping
    // takes S values to X values.
    let fs = specfrm.convert_to(&xfrm, &[])?;

    // Warn when the algorithm code's "P" character disagrees with the one the
    // axis's own system implies. Mirrors C:23675-23704, which runs this whether
    // or not the conversion above succeeded — the only guard is that the X
    // system was identified. AST interprets the illegal "P" system correctly,
    // so nothing about the mapping changes.
    let mut warnings: Warnings = Vec::new();
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
        ));
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
/// Mirrors C `GrismSpecWcs` (fitschan.c:14300-14462). `-GRI` disperses in
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
    let Some(fs) = specfrm.convert_to(&wfrm, &[])? else {
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
    // (fitschan.c:14416, fixed upstream at 1199880a).
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
/// Mirrors C `LogWcs` (fitschan.c:20771-20853): S = Sr*exp(w/Sr) forward,
/// w = Sr*log(S/Sr) inverse, where Sr is the axis CRVAL. A zero Sr has no
/// logarithmic form, so C returns NULL and the axis stays linear.
fn log_wcs(crval: f64) -> Option<DynMapping> {
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
    // C LogWcs sets SIMPFI=1 and SIMPIF=1 (fitschan.c:20846-20847), mirrored
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
/// (fitschan.c:20413-20542).
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

    #[test]
    fn detects_freq_axis() {
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[2] = Some(1.4e9); // 1.4 GHz

        let result = build_spectral(&store, ' ').unwrap();
        assert!(result.is_some());
        let spec = result.unwrap();
        assert_eq!(spec.axis, 2);
        assert_eq!(spec.frame.system(), SpecSystem::Freq);
        assert_eq!(spec.mapping.nin(), 3);
        assert_eq!(spec.mapping.nout(), 3);
    }

    #[test]
    fn detects_wave_axis() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("WAVE".to_string());
        store.crval[0] = Some(5.0e-7); // 500nm in metres

        let result = build_spectral(&store, ' ').unwrap().unwrap();
        assert_eq!(result.frame.system(), SpecSystem::Wavelen);
    }

    #[test]
    fn detects_vrad_axis() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("VRAD".to_string());
        store.crval[0] = Some(0.0);

        let result = build_spectral(&store, ' ').unwrap().unwrap();
        assert_eq!(result.frame.system(), SpecSystem::Vradio);
    }

    #[test]
    fn sets_std_of_rest() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.0e9);
        store.specsys = Some("LSRK".to_string());

        let result = build_spectral(&store, ' ').unwrap().unwrap();
        assert_eq!(result.frame.std_of_rest(), StdOfRest::Lsrk);
    }

    #[test]
    fn sets_rest_freq() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("VRAD".to_string());
        store.crval[0] = Some(0.0);
        store.restfrq = Some(1.420405751786e9);

        let result = build_spectral(&store, ' ').unwrap().unwrap();
        assert!((result.frame.rest_freq() - 1.420405751786e9).abs() < 1.0);
    }

    #[test]
    fn no_spectral_returns_none() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());

        let result = build_spectral(&store, ' ').unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn crval_shift_mapping() {
        use crate::coords::PointSet;

        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.4e9);

        let result = build_spectral(&store, ' ').unwrap().unwrap();
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
    fn naxis_spectral_passthrough_other_axes() {
        use crate::coords::PointSet;

        // 3-axis: celestial pair at 0, 1; spectral at 2.
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[2] = Some(1.4e9);

        let spec = build_spectral(&store, ' ')
            .unwrap()
            .expect("spectral axis recognised");
        assert_eq!(spec.axis, 2);
        assert_eq!(spec.mapping.nin(), 3);
        assert_eq!(spec.mapping.nout(), 3);

        // Pass IWC (a, b, 0) — axes 0, 1 must come out unchanged;
        // axis 2 picks up CRVAL.
        let pts = PointSet::from_columns(&[&[42.0], &[7.0], &[0.0]]).unwrap();
        let out = spec.mapping.transform(&pts, true).unwrap();
        let p = out.point(0);
        assert!((p[0] - 42.0).abs() < 1e-12, "axis 0 was {}", p[0]);
        assert!((p[1] - 7.0).abs() < 1e-12, "axis 1 was {}", p[1]);
        assert!((p[2] - 1.4e9).abs() < 1.0);
    }
}

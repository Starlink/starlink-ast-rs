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
//! [`nonlin_spec_wcs`] which mirrors C `NonLinSpecWcs` (fitschan.c:23486).

use super::fits_store::FitsStore;
use crate::error::AstResult;
use crate::frames::{Frame, FrameRef, SpecFrame, SpecSystem, StdOfRest};
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, UnitMap, ZoomMap};

/// Embed a 1-axis mapping into an N-axis mapping with `UnitMap`s on
/// the other axes. Identity-fast for `naxis == 1`. Mirrors C
/// `WcsSpectral` (fitschan.c:39661-39676), which composes one 1D map
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
    /// (fitschan.c:39498-39503), which builds a DSBSpecFrame when the
    /// FitsStore carries an `IMAGFREQ` value. The DSB-specific attributes
    /// (IF / DSBCentre / SideBand) are filled in by `dsb_set_up` after the
    /// reference position has been propagated from the celestial axes.
    pub is_dsb: bool,
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
/// Returns `None` if no spectral axis is found.
pub(crate) fn build_spectral(store: &FitsStore) -> Option<SpectralResult> {
    // Step 1: Find the spectral axis.
    let axis = store.spectral_axis()?;

    // Step 2: Determine the spectral system + algorithm code from CTYPE.
    let (system, algcode) = parse_spectral_ctype(store, axis)?;

    // Step 3: Build the SpecFrame (so we can pass it to the algorithm
    // dispatch, which may need its rest frequency and unit).
    let mut frame = SpecFrame::new(system);

    if let Some(ref specsys) = store.specsys {
        if let Some(sor) = StdOfRest::parse(specsys) {
            frame.set_std_of_rest(sor);
        }
    }

    if let Some(restfrq) = store.restfrq {
        frame.set_rest_freq(restfrq);
    } else if let Some(restwav) = store.restwav {
        const SPEED_OF_LIGHT: f64 = 299_792_458.0; // m/s
        let restfrq = SPEED_OF_LIGHT / restwav;
        frame.set_rest_freq(restfrq);
    }

    // MJD-OBS → SpecFrame Epoch via TDBConv. Mirrors C's
    // `ChooseEpoch` (fitschan.c:6143-6146) which always returns a TDB
    // MJD, regardless of TIMESYS. Without this conversion the
    // SpecFrame's `add_sor_to_helio` / `add_helio_to_sor` chains run
    // with a UTC epoch and the topocentric Doppler correction is
    // wrong by ~63 s × Earth-orbit-rate.
    if let Some(raw_mjd) = store.mjdobs {
        let tdb_mjd = match store.timesys.as_deref() {
            Some(s) if s.eq_ignore_ascii_case("TDB") => raw_mjd,
            Some(s) if s.eq_ignore_ascii_case("TT") => {
                raw_mjd + crate::fitschan::wcs_write::tdb_minus_tt_seconds(raw_mjd) / 86_400.0
            }
            Some(s) if s.eq_ignore_ascii_case("TAI") => {
                let tt = raw_mjd + 32.184 / 86_400.0;
                tt + crate::fitschan::wcs_write::tdb_minus_tt_seconds(tt) / 86_400.0
            }
            // UTC (default), UT1, or unknown: convert via UTC→TDB.
            _ => crate::fitschan::wcs_write::utc_mjd_to_tdb(raw_mjd),
        };
        frame.set_epoch_mjd(tdb_mjd);
    }

    // OBSGEO-X/Y/Z → ObsLon/ObsLat/ObsAlt. Mirrors C
    // fitschan.c:39534-39550.
    if let (Some(x), Some(y), Some(z)) = (store.obsgeox, store.obsgeoy, store.obsgeoz)
        && let Some((geolon, geolat, h)) = crate::pal::gc2gd([x, y, z])
    {
        frame.set_obs_lon(geolon);
        frame.set_obs_lat(geolat);
        frame.set_obs_alt(h);
    }

    // Source velocity rest frame (SSYSSRC → SourceVRF) and source
    // velocity (ZSOURCE redshift → SourceVel). Mirrors C `WcsSpectral`
    // (fitschan.c:39547-39559). The source velocity defines the
    // Source standard of rest, which DSBSetUp's source-rest →
    // topocentric conversion depends on; without it the topocentric
    // image/rest frequencies (and hence the derived IF) are wrong.
    if let Some(ref ssyssrc) = store.ssyssrc
        && let Some(sor) = StdOfRest::parse(ssyssrc)
    {
        frame.set_source_vrf(sor);
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
    // C `WcsSpectral` (fitschan.c:39509-39517) always calls
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
        frame.set_unit(&cunit);
    }

    // Step 4: Build the IWC→S mapping. For a linear axis (no algcode)
    // this is just `IWC + CRVAL`. For an `-X2P` non-linear axis we
    // call the algorithm-specific builder (mirrors C `WcsSpectral`
    // dispatch at fitschan.c:39593-39610).
    let crval = store.crval[axis].unwrap_or(0.0);
    let mapping: DynMapping = if algcode.is_empty() {
        Box::new(ShiftMap::new(vec![crval]))
    } else if is_nonlin_x2p(&algcode) {
        match nonlin_spec_wcs(&algcode, &frame, crval) {
            Ok(Some(m)) => m,
            // Fallback: emit a linear shift if we couldn't build the
            // non-linear mapping (mirrors C's "unable to implement
            // algorithm" branch — but C errors out; we degrade to
            // linear so the FrameSet still loads).
            _ => Box::new(ShiftMap::new(vec![crval])),
        }
    } else if algcode == "-LOG" {
        match log_wcs(crval) {
            Some(m) => m,
            None => Box::new(ShiftMap::new(vec![crval])),
        }
    } else {
        // -GRI / -GRA (GrismSpecWcs) have no reader yet; fall back to
        // linear so the FrameSet still loads.
        Box::new(ShiftMap::new(vec![crval]))
    };

    let naxis = store.naxis;
    let mapping = match embed_1axis_in_naxis(mapping, naxis, axis) {
        Ok(m) => m,
        Err(_) => return None,
    };

    // C `WcsSpectral` (fitschan.c:39498-39503) builds a DSBSpecFrame
    // instead of a plain SpecFrame when the FitsStore carries an IMAGFREQ
    // value. The DSB-specific attributes are computed by `dsb_set_up`
    // after the celestial reference position has been propagated.
    let is_dsb = store.imagfreq.is_some();

    Some(SpectralResult {
        mapping,
        frame,
        axis,
        is_dsb,
    })
}

/// Whether the algorithm code is the FITS-WCS paper III "-X2P" non-linear
/// form (e.g. `-F2W`, `-V2F`).
///
/// Mirrors C's check at fitschan.c:39603 — the code starts with `'-'` and
/// has `'2'` at position 2.
fn is_nonlin_x2p(algcode: &str) -> bool {
    let bytes = algcode.as_bytes();
    bytes.len() >= 4 && bytes[0] == b'-' && bytes[2] == b'2'
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
/// Returns `Ok(None)` when the algorithm code's X system is unrecognised
/// or when the X-to-S rate of change is zero/undefined at the reference
/// point.
fn nonlin_spec_wcs(
    algcode: &str,
    specfrm: &SpecFrame,
    crval: f64,
) -> AstResult<Option<DynMapping>> {
    // Identify the "X" system from algcode[1] and build a SpecFrame
    // copy describing X. Mirrors C:23579-23600. Copying the supplied
    // SpecFrame ensures rest_freq, std_of_rest, etc. are preserved.
    let bytes = algcode.as_bytes();
    let x_system = match bytes.get(1).copied() {
        Some(b'F') => SpecSystem::Freq,
        Some(b'W') => SpecSystem::Wavelen,
        Some(b'V') => SpecSystem::Vrel,
        Some(b'A') => SpecSystem::Airwave,
        _ => return Ok(None),
    };
    let mut xfrm = specfrm.clone();
    xfrm.set_system(x_system);
    xfrm.set_unit(x_system.specmap_units());

    // Build the S → X mapping via Frame::convert (mirrors
    // C:23606-23612). This returns a FrameSet whose forward mapping
    // takes S values to X values.
    let Some(fs) = specfrm.convert_to(&xfrm, &[])? else {
        return Ok(None);
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
        _ => return Ok(None),
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
    Ok(Some(Box::new(iwc_to_s)))
}

/// Build the IWC→S mapping for a FITS-WCS paper III `-LOG` spectral axis.
///
/// Mirrors C `LogWcs` (fitschan.c:20666-20748): S = Sr*exp(w/Sr) forward,
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
    // C LogWcs sets SIMPFI=1 and SIMPIF=1 (fitschan.c:20741-20742), mirrored
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
/// (fitschan.c:20308-20437).
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

        let result = build_spectral(&store);
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

        let result = build_spectral(&store).unwrap();
        assert_eq!(result.frame.system(), SpecSystem::Wavelen);
    }

    #[test]
    fn detects_vrad_axis() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("VRAD".to_string());
        store.crval[0] = Some(0.0);

        let result = build_spectral(&store).unwrap();
        assert_eq!(result.frame.system(), SpecSystem::Vradio);
    }

    #[test]
    fn sets_std_of_rest() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.0e9);
        store.specsys = Some("LSRK".to_string());

        let result = build_spectral(&store).unwrap();
        assert_eq!(result.frame.std_of_rest(), StdOfRest::Lsrk);
    }

    #[test]
    fn sets_rest_freq() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("VRAD".to_string());
        store.crval[0] = Some(0.0);
        store.restfrq = Some(1.420405751786e9);

        let result = build_spectral(&store).unwrap();
        assert!((result.frame.rest_freq().unwrap() - 1.420405751786e9).abs() < 1.0);
    }

    #[test]
    fn no_spectral_returns_none() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());

        let result = build_spectral(&store);
        assert!(result.is_none());
    }

    #[test]
    fn crval_shift_mapping() {
        use crate::coords::PointSet;

        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.4e9);

        let result = build_spectral(&store).unwrap();
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

        let spec = build_spectral(&store).expect("spectral axis recognised");
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

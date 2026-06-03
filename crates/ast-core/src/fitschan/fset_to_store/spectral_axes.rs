//! Port of `fitschan.c::SpectralAxes` (~30050).
//!
//! Identifies any spectral axis in the FrameSet's current frame,
//! stores its FITS keywords (CTYPE / CUNIT / CRVAL / SPECSYS /
//! RESTFRQ / VELOSYS) in the supplied [`FitsStore`], marks
//! `axis_done`, and returns the WCS→IWC mapping segment for that
//! axis.
//!
//! ## TODO: branches not yet ported
//!
//! Lack of wcsconv fixture coverage is not an excuse for
//! under-porting — see [`feedback_test_coverage_is_separate_from_porting`]
//! in user memory. Tracked in
//! [`docs/issues/fset_to_store-port-gaps.md`](../../../../../docs/issues/fset_to_store-port-gaps.md):
//!
//! - **`-TAB` algorithm**. Blocked on FitsTable infrastructure.
//! - **`VELOSYS` calculation** — needs the celestial source position
//!   from `celestial_axes`. Group 7 wiring will deliver it; the
//!   existing `wcs_write::populate_velosys` math will be moved here
//!   when the celestial→spectral sequencing lands.
//! - **SpecFrame unit-conversion rewriting** when the spectral
//!   system requires a unit change to be FITS-compliant
//!   (C:30276-30311). Currently the user-set CUNIT is preserved
//!   unchanged.
//!
//! What IS now covered: linear-path with `IsMapLinear` gate,
//! `-X2P` non-linear algorithm cascade (mirrors C:30359-30454)
//! via `try_x2p_cascade`, `-LOG` algorithm via `LogAxis`, `-GRI`/
//! `-GRA` grism algorithm via `try_grism_cascade` + the
//! standalone `extract_grism_map` helper, RESTFRQ vs RESTWAV
//! choice, OBSGEO emission, axis-Label → CTYPE-comment/CNAME,
//! source-velocity → ZSOURCE/SSYSSRC.

// Wired in by `fset_to_store::fset_to_store` (Group 7); until then
// only reachable from cfg(test).
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::type_complexity)] // try_grism_cascade returns a 4-tuple of (Mapping, String, Vec, f64)

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{Frame, FrameRef, FrameSet, SpecFrame, SpecSystem, StdOfRest};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, ShiftMap, UnitMap, ZoomMap};

use crate::mappings::helpers::is_bad;

use super::is_map_linear::is_map_linear;
use super::log_axis::log_axis;

/// Speed of light in vacuum (m/s). Mirrors C `AST__C`.
const SPEED_OF_LIGHT: f64 = 299_792_458.0;

/// Arguments for [`spectral_axes`]. Same shape as
/// [`super::celestial_axes::CelestialAxesArgs`] minus the `isoff`
/// flag (spectral axes have no offset variant in FITS-WCS).
pub(crate) struct SpectralAxesArgs<'a> {
    pub fs: &'a FrameSet,
    pub dim: &'a [Option<f64>],
    pub wperm: &'a mut [usize],
    pub s: char,
    pub store: &'a mut FitsStore,
    /// Per-axis CRVAL override; `Some` if a higher-priority builder
    /// already chose a value for this axis. Read-only here.
    pub crvals: &'a [Option<f64>],
    pub axis_done: &'a mut [bool],
}

/// Detect and describe the spectral axis (if any). Mirrors
/// `fitschan.c::SpectralAxes`.
pub(crate) fn spectral_axes(args: SpectralAxesArgs<'_>) -> AstResult<DynMapping> {
    let wcsfrm = args.fs.current();
    let nwcs = wcsfrm.naxes();

    // Step 1: locate the spectral axis. Mirrors C:30243-30248
    // (`astIsASpecFrame`, which returns true for DSBSpecFrame too —
    // it's a subclass of SpecFrame in the C type system).
    let mut spec_axis: Option<usize> = None;
    for iax in 0..nwcs {
        let (pframe, _paxis) = primary_frame(wcsfrm, iax);
        if is_spec_or_dsb(pframe) {
            spec_axis = Some(iax);
            break;
        }
    }
    let Some(spec_iax) = spec_axis else {
        return Ok(Box::new(UnitMap::new(nwcs)));
    };

    // Re-resolve the SpecFrame so we can read its attributes. For a
    // DSBSpecFrame, walk to the inner SpecFrame — the FITS-WCS
    // mapping treats both as "spectral axis" but the per-attribute
    // accessors live on SpecFrame. The DSB-specific fields
    // (DSBCentre, IF, SideBand) are read separately later in the
    // pipeline (e.g. spectral_axes already calls
    // `astIsADSBSpecFrame(specfrm)` for IMAGFREQ).
    let (spec_holder, _) = primary_frame(wcsfrm, spec_iax);
    let specfrm = match spec_or_inner(spec_holder) {
        Some(s) => s,
        None => return Ok(Box::new(UnitMap::new(nwcs))),
    };

    let fits_i = args.wperm[spec_iax];

    // Step 2: hold the caller's CRVAL override (if any). The
    // bound-based fallback is computed AFTER unit-absorption (below)
    // so that the bounds are taken on the same map that the
    // algorithm cascade sees. Mirrors fitschan.c:30301-30329.
    let crval_input = args.crvals.get(spec_iax).copied().flatten();
    let mut crval: f64 = match crval_input {
        Some(v) if !is_bad(v) => v,
        _ => BAD_VALUE,
    };

    let system = specfrm.system();
    let orig_system_str = system.as_str();

    // Step 3a: algorithm cascade. Mirrors C:30340-30609 — try the
    // linear path first (CTYPE = orig_system); fall through to log
    // (CTYPE = orig_system + "-LOG"). Non-linear (-X2P), grism
    // (-GRI/-GRA), and -TAB paths are still TODO; tracked in
    // fset_to_store-port-gaps.md. When none of the algorithms
    // matches, fall back to the always-linear ShiftMap (the C
    // "fallback" doesn't exist in SpectralAxes — when nothing
    // matches, ctype stays empty and the axis isn't described.
    // We deviate slightly: build a ShiftMap anyway so callers can
    // proceed, but emit ctype = orig_system; make_int_world will
    // reject the resulting non-linear pixel→IWC).
    let mut map = args.fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let npix = map.nin();
    let lbnd_p: Vec<f64> = vec![1.0; npix];
    let ubnd_p: Vec<f64> = (0..npix)
        .map(|i| args.dim.get(i).copied().flatten().unwrap_or(500.0))
        .collect();

    // Unit-rewrite preprocessing. Mirrors fitschan.c::SpectralAxes
    // C:30276-30311. When the SpecFrame has a non-default Unit:
    //
    // 1. Build a unit-conversion mapping from user-units to the
    //    SpecMap canonical default (Hz / m / m/s).
    // 2. If linear (UnitMap or ZoomMap) → leave the chain alone;
    //    the writer will emit CUNIT = user-units directly.
    // 3. If non-linear (MathMap, e.g. log/sqrt wrappers) → absorb
    //    the conversion into the pixel→spec chain so the algorithm
    //    cascade sees a chain in canonical units. Override CUNIT and
    //    CRVAL accordingly.
    //
    // The C uses FrameSet auto-remap-on-`astClear` for step 3; our
    // FrameSet doesn't yet expose that integrity path, so we
    // compose locally.
    let default_unit = system.specmap_units();
    let mut absorbed_unit = false;
    // C calls `astGetUnit` here, which always returns a non-NULL string
    // — the explicit Unit when set, else the system's display default
    // (km/s for velocity, GHz for FREQ, etc.). Use `unit_or_default`
    // to match.
    let user_unit = specfrm.unit_or_default();
    if !user_unit.is_empty() && !default_unit.is_empty() && user_unit != default_unit {
        if let Some(unit_map) = crate::units::unit_mapper(user_unit, default_unit) {
            let is_linear = unit_map.as_any().is::<UnitMap>() || unit_map.as_any().is::<ZoomMap>();
            if !is_linear {
                // Compose pixel→spec[user] with spec[user]→spec[default],
                // embedded on the spec axis only.
                let embedded = embed_axis(unit_map.clone_box(), spec_iax, nwcs)?;
                map = Box::new(CmpMap::new(map, embedded, Composition::Series)?);
                // Transform CRVAL through the unit mapping (only if a
                // caller-supplied override was provided — otherwise
                // CRVAL is derived from the post-absorption map below).
                if !is_bad(crval) {
                    let pts = PointSet::from_rows([[crval]]);
                    let out = unit_map.transform(&pts, true)?;
                    crval = out.point(0)[0];
                }
                absorbed_unit = true;
            }
        }
    }

    // Step 2c: derive default CRVAL from the spectral-axis bounds of
    // the post-absorption map. Mirrors fitschan.c:30275 + 30308 +
    // 30323-30329: bounds via astMapBox; if no caller override and
    // dim is known, use mid-bound; else use lower bound.
    let bounds = map.map_box(&lbnd_p, &ubnd_p, true, spec_iax)?;
    let lbnd_s = bounds.lbnd_out;
    let ubnd_s = bounds.ubnd_out;
    if is_bad(crval) {
        let dim_known = args.dim.get(spec_iax).copied().flatten().is_some();
        if dim_known && !is_bad(lbnd_s) && !is_bad(ubnd_s) {
            crval = 0.5 * (lbnd_s + ubnd_s);
        } else if !is_bad(lbnd_s) {
            crval = lbnd_s;
        }
    }
    if is_bad(crval) {
        // No usable spectral coordinate available.
        return Ok(Box::new(UnitMap::new(nwcs)));
    }

    // Step 2d: snap crval so it corresponds to an integer pixel
    // along the spectral axis. Mirrors fitschan.c::SpectralAxes
    // line 30333 (`crval = NearestPix(map, crval, iax, status)`).
    crval = nearest_pix_value(map.as_ref(), crval, spec_iax)?;

    // Step 3: write keywords. Mirrors fitschan.c populate paths
    // (we cover the canonical-form subset that wcs_write::populate_spectral
    // also covers).
    args.store.crval[fits_i] = Some(crval);

    let (axmap, ctype): (DynMapping, String) =
        if is_map_linear(map.as_ref(), &lbnd_p, &ubnd_p, spec_iax)? {
            // Linear path: ShiftMap(-crval).
            let shift = ShiftMap::new(vec![-crval]);
            let mapping = embed_axis(Box::new(shift), spec_iax, nwcs)?;
            (mapping, orig_system_str.to_string())
        } else if let Some((axmap, ctype)) = try_x2p_cascade(
            specfrm,
            map.as_ref(),
            spec_iax,
            nwcs,
            &lbnd_p,
            &ubnd_p,
            crval,
            orig_system_str,
        )? {
            // Non-linear -X2P algorithm: try each of FREQ/WAVE/AWAV/
            // VELO as the linearly-related "X" system. Mirrors
            // C:30359-30454.
            (axmap, ctype)
        } else if let Some(lm) = log_axis(map.as_ref(), spec_iax, nwcs, &lbnd_p, &ubnd_p, crval)? {
            // -LOG algorithm. Mirrors C:30467-30477.
            let ctype = format!("{orig_system_str}-LOG");
            (lm, ctype)
        } else if let Some((axmap, ctype, grism_pv, new_crval)) = try_grism_cascade(
            specfrm,
            map.as_ref(),
            spec_iax,
            nwcs,
            &lbnd_p,
            &ubnd_p,
            orig_system_str,
        )? {
            // -GRI / -GRA grism algorithm. Mirrors C:30480-30599. Stores
            // the grism PV parameters; reuses the grism-derived CRVAL
            // (which the C path also overwrites — its `crval` is reset
            // to grism.wave_r converted into the original system).
            for (m, value) in grism_pv {
                if fits_i < args.store.pv.len() && m < args.store.pv[fits_i].len() {
                    args.store.pv[fits_i][m] = Some(value);
                }
            }
            crval = new_crval;
            args.store.crval[fits_i] = Some(crval);
            (axmap, ctype)
        } else {
            // Fallback: build the same ShiftMap as the linear path
            // (without an algorithm-suffix CTYPE). make_int_world
            // will reject the resulting non-linear pixel→IWC.
            // -TAB is not yet ported.
            let shift = ShiftMap::new(vec![-crval]);
            let mapping = embed_axis(Box::new(shift), spec_iax, nwcs)?;
            (mapping, orig_system_str.to_string())
        };

    args.store.ctype[fits_i] = Some(ctype);

    // CUNIT: result of C's astGetUnit (explicit unit, else display
    // default — km/s for velocities, GHz for FREQ, etc). When we
    // absorbed a non-linear unit conversion above, the chain now
    // produces canonical values, so force CUNIT to specmap_units.
    let cunit = if absorbed_unit {
        system.specmap_units().to_string()
    } else {
        specfrm.unit_or_default().to_string()
    };
    args.store.cunit[fits_i] = Some(cunit);

    // CTYPE-comment / CNAME from explicit Label override. Mirrors
    // C:30716-30722.
    if specfrm.test_axis_label() {
        let label = specfrm.axis_label();
        if !label.is_empty() {
            args.store.ctype_com[fits_i] = Some(label.clone());
            args.store.cname[fits_i] = Some(label);
        }
    }

    // OBSGEO-X/Y/Z from observer geodetic position. Only emitted on
    // the primary coord version. Mirrors C:30728-30739.
    if args.s == ' '
        && let (Some(geolon), Some(geolat)) = (
            <SpecFrame as Frame>::obs_lon(specfrm),
            <SpecFrame as Frame>::obs_lat(specfrm),
        )
    {
        let h = <SpecFrame as Frame>::obs_alt(specfrm).unwrap_or(0.0);
        if let Some(xyz) = crate::pal::gd2gc(geolon, geolat, h) {
            args.store.obsgeox = Some(xyz[0]);
            args.store.obsgeoy = Some(xyz[1]);
            args.store.obsgeoz = Some(xyz[2]);
        }
    }

    // RESTFRQ vs RESTWAV: pick which to emit based on the system.
    // Mirrors C:30740-30751.
    if let Some(rf) = specfrm.rest_freq() {
        if matches!(
            system,
            SpecSystem::Wavelen | SpecSystem::Voptical | SpecSystem::Redshift | SpecSystem::Airwave
        ) {
            // Wavelength-domain → emit RESTWAV (in metres) instead.
            args.store.restwav = Some(SPEED_OF_LIGHT / rf);
        } else {
            args.store.restfrq = Some(rf);
        }
    }

    // SPECSYS from the standard-of-rest.
    args.store.specsys = Some(specsys_string(specfrm.std_of_rest()));

    // SourceVel → ZSOURCE + SSYSSRC. Mirrors C:30761-30772.
    if let Some(vsource) = specfrm.source_vel()
        && vsource.abs() < SPEED_OF_LIGHT
    {
        // Convert m/s velocity to dimensionless redshift via the
        // relativistic Doppler formula (matches C:30764-30765).
        let zsource = ((SPEED_OF_LIGHT + vsource) / (SPEED_OF_LIGHT - vsource)).sqrt() - 1.0;
        args.store.zsource = Some(zsource);
        args.store.ssyssrc = Some(specsys_string(specfrm.source_vrf()));
    }

    // VELOSYS: topocentric apparent velocity of the SpecFrame's
    // standard of rest. Mirrors C:30774-30796. C achieves this by
    // temporarily overwriting SourceVRF/SourceVel and re-reading
    // SourceVel; we compute the equivalent value directly from the
    // SpecFrame's existing attributes via `velosys()`. Only emitted
    // on the primary coord version.
    if args.s == ' '
        && let Some(v) = specfrm.velosys()
    {
        args.store.velosys = Some(v);
    }

    // Mark done and return.
    args.axis_done[spec_iax] = true;

    let nd = axmap.simplified()?;
    Ok(nd)
}

/// Try the FITS-WCS paper III non-linear `-X2P` algorithm cascade.
/// For each trial X system in `{FREQ, WAVE, AWAV, VELO}`:
///   1. Build a SpecFrame copy with system = X, default unit.
///   2. Build the X→S mapping via the existing `make_spec_mapping`.
///   3. Build pixel→X by composing pixel→S with S→X (inverse of X→S),
///      embed at `spec_iax`.
///   4. If pixel→X is linear at `spec_iax`, accept this X system:
///      - xval = invert(X→S) applied to crval.
///      - dsbydx = rate of S w.r.t. X at xval (from X→S forward).
///      - WCS→IWC = S→X → ShiftMap(-xval) → ZoomMap(dsbydx),
///        embedded at spec_iax.
///      - CTYPE = `<orig>-X2P` per the C convention.
///
/// Returns `None` when no X system is linearly related to pixels.
/// Mirrors `fitschan.c::SpectralAxes` lines 30359-30454.
#[allow(clippy::too_many_arguments)]
fn try_x2p_cascade(
    specfrm: &SpecFrame,
    pixel_to_s_map: &dyn crate::mappings::Mapping,
    spec_iax: usize,
    nwcs: usize,
    lbnd_p: &[f64],
    ubnd_p: &[f64],
    crval: f64,
    orig_system_str: &str,
) -> AstResult<Option<(DynMapping, String)>> {
    // The 4 X systems mirroring the C `x_sys` array at C:30221-30224.
    const X_SYSTEMS: [SpecSystem; 4] = [
        SpecSystem::Freq,
        SpecSystem::Wavelen,
        SpecSystem::Airwave,
        SpecSystem::Vrel,
    ];

    for &x_sys in &X_SYSTEMS {
        // Skip the trivial case where X == orig_system: that's the
        // linear path and the caller has already tested it.
        if x_sys == specfrm.system() {
            continue;
        }

        // Build a SpecFrame copy with the trial X system + default
        // unit. Mirrors the C `astSetC(fs, system_attr, x_sys[ix]);
        // astClear(fs, unit_attr);` (line 30374-30375) — but we work
        // on a clone rather than mutating the FrameSet in place.
        let mut x_frame = specfrm.clone();
        x_frame.set_system(x_sys);
        x_frame.clear_unit();

        // X → S mapping (forward). Mirrors C:30412-30417 (which
        // builds the "S as a function of X" Mapping). Skip this
        // X system if the conversion isn't constructible.
        let x2s = match x_frame.make_spec_mapping(specfrm) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // S→X for the linearity probe and the IWC chain.
        let mut s2x = x2s.clone_box();
        s2x.invert();
        let s2x_padded = embed_axis(s2x.clone_box(), spec_iax, nwcs)?;
        let pixel_to_x: DynMapping = Box::new(CmpMap::new(
            pixel_to_s_map.clone_box(),
            s2x_padded,
            Composition::Series,
        )?);

        if !is_map_linear(pixel_to_x.as_ref(), lbnd_p, ubnd_p, spec_iax)? {
            continue;
        }

        // Found a linear X. Compute xval and dsbydx.
        // C:30422 — xval = X(crval), via inverse of X→S applied to
        // crval (which is in S units). x2s forward is X→S, so the
        // inverse direction takes S → X.
        let pts_s = PointSet::from_rows([[crval]]);
        let pts_x = match x2s.transform(&pts_s, false) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let xval = pts_x.point(0)[0];
        if !xval.is_finite() {
            continue;
        }

        // C:30426 — dsbydx via astRate(tmap5, &xval, 0, 0). tmap5
        // forward is X→S, so rate(at_xval, output_axis=0,
        // input_axis=0) = dS/dX at X=xval.
        let dsbydx = match x2s.rate(&[xval], 1, 1) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !dsbydx.is_finite() || dsbydx == 0.0 {
            continue;
        }

        // Build the WCS→IWC chain (1-D, on S inputs). Mirrors
        // C:30435-30445:
        //   S → X (s2x forward) → X − xval (ShiftMap) → (X − xval)·dsbydx (ZoomMap)
        let shift = ShiftMap::new(vec![-xval]);
        let zoom = crate::mappings::ZoomMap::new(1, dsbydx)?;
        let chain_1d: DynMapping = Box::new(CmpMap::new(
            Box::new(CmpMap::new(s2x, Box::new(shift), Composition::Series)?),
            Box::new(zoom),
            Composition::Series,
        )?);
        let axmap = embed_axis(chain_1d, spec_iax, nwcs)?;

        // Build the -X2P CTYPE. Mirrors C:30385-30409:
        //   First 4 chars = orig_system.
        //   Suffix is "-X2P" where:
        //     - X = F (FREQ), W (WAVE), A (AWAV), V (VELO).
        //     - P depends on orig_system.
        let x_char = match x_sys {
            SpecSystem::Freq => 'F',
            SpecSystem::Wavelen => 'W',
            SpecSystem::Airwave => 'A',
            SpecSystem::Vrel => 'V',
            _ => unreachable!("X_SYSTEMS only contains the 4 above"),
        };
        let p_char = match orig_system_str {
            "FREQ" | "ENER" | "WAVN" | "VRAD" => 'F',
            "WAVE" | "VOPT" | "ZOPT" => 'W',
            "AWAV" => 'A',
            _ => 'V',
        };
        let ctype = format!("{orig_system_str}-{x_char}2{p_char}");
        return Ok(Some((axmap, ctype)));
    }

    Ok(None)
}

/// Try the grism-algorithm cascade. For each candidate wavelength
/// system (`Wavelen`, `Airwave`):
///   1. Build a SpecFrame copy with that wavelength system + unit
///      "m", and the S→wavelength Mapping via `make_spec_mapping`.
///   2. Compose pixel→S with S→wave (embedded at `spec_iax`).
///   3. Try to extract a GrismMap from the resulting pixel→wave
///      chain via [`super::grism::extract_grism_map`].
///   4. If extracted AND the GrismMap-free chain is linear at
///      `spec_iax`, build the WCS→IWC mapping and emit `-GRI` (for
///      Wavelen) or `-GRA` (for Airwave) CTYPE plus the grism PV
///      parameters.
///
/// On success returns `Some((axmap, ctype, pv_pairs, new_crval))`.
/// `new_crval` is the grism's reference wavelength converted into
/// the SpecFrame's original system.
///
/// Mirrors `fitschan.c::SpectralAxes` C:30480-30599.
#[allow(clippy::too_many_arguments)]
fn try_grism_cascade(
    specfrm: &SpecFrame,
    pixel_to_s_map: &dyn crate::mappings::Mapping,
    spec_iax: usize,
    nwcs: usize,
    lbnd_p: &[f64],
    ubnd_p: &[f64],
    orig_system_str: &str,
) -> AstResult<Option<(DynMapping, String, Vec<(usize, f64)>, f64)>> {
    use crate::mappings::ZoomMap;
    use std::f64::consts::PI;

    const DR2D: f64 = 180.0 / PI;

    // (wavelength_system, "-GRI"/"-GRA" suffix). Mirrors C:30486-30487.
    const GRISM_SYSTEMS: [(SpecSystem, &str); 2] =
        [(SpecSystem::Wavelen, "-GRI"), (SpecSystem::Airwave, "-GRA")];

    for &(wave_sys, suffix) in &GRISM_SYSTEMS {
        // Build a SpecFrame copy in the trial wavelength system
        // with default unit. Mirrors C:30487-30488 (`astSetC(fs,
        // system_attr, ...); astSetC(fs, unit_attr, "m");`).
        let mut wave_frame = specfrm.clone();
        wave_frame.set_system(wave_sys);
        wave_frame.clear_unit();

        // S → wavelength Mapping. If this can't be constructed, skip.
        let s_to_wave = match specfrm.make_spec_mapping(&wave_frame) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Embed at spec_iax and compose with pixel→S to get the
        // pixel→wavelength chain we'll search for a GrismMap.
        let s_to_wave_padded = embed_axis(s_to_wave.clone_box(), spec_iax, nwcs)?;
        let pixel_to_wave: DynMapping = Box::new(CmpMap::new(
            pixel_to_s_map.clone_box(),
            s_to_wave_padded,
            Composition::Series,
        )?);

        // Simplify the chain — GrismMap usually shows up after
        // ZoomMap simplification. Mirrors C:30496 (astSimplify).
        let simplified = pixel_to_wave.simplified()?;

        // Try to extract a GrismMap producing output spec_iax.
        let (grism, sans_grism) =
            match super::grism::extract_grism_map(simplified.as_ref(), spec_iax)? {
                Some(p) => p,
                None => continue,
            };

        // The GrismMap-free chain must be linear in pixel coords on
        // the spectral axis. Mirrors C:30508.
        if !is_map_linear(sans_grism.as_ref(), lbnd_p, ubnd_p, spec_iax)? {
            continue;
        }

        // Recover the grism reference wavelength (in m) and convert
        // it to the SpecFrame's original system. Mirrors
        // C:30511-30529.
        let crval_wave = grism.grism_wave_r();
        if !crval_wave.is_finite() {
            continue;
        }
        // wave→S forward Mapping for the conversion.
        let wave_to_s = match wave_frame.make_spec_mapping(specfrm) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let pts_wave = PointSet::from_rows([[crval_wave]]);
        let pts_s = match wave_to_s.transform(&pts_wave, true) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let new_crval = pts_s.point(0)[0];
        if !new_crval.is_finite() {
            continue;
        }

        // Build S → grism_param. Mirrors C:30531-30536 (the C uses
        // tmap3 = S→wave then composes with grism.invert() = wave→
        // grism_param. We mirror that with a fresh S → wave and an
        // inverted grism clone).
        let mut grism_inv: DynMapping = Box::new(grism.clone());
        grism_inv.invert();
        let s_to_grism: DynMapping = Box::new(CmpMap::new(
            s_to_wave.clone_box(),
            grism_inv,
            Composition::Series,
        )?);

        // gval = grism parameter at the reference S value. Mirrors
        // C:30539.
        let pts_s_at_ref = PointSet::from_rows([[new_crval]]);
        let pts_g = match s_to_grism.transform(&pts_s_at_ref, true) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let gval = pts_g.point(0)[0];
        if !gval.is_finite() {
            continue;
        }

        // dgbyds at the reference. Mirrors C:30543.
        let dgbyds = match s_to_grism.rate(&[new_crval], 1, 1) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !dgbyds.is_finite() || dgbyds == 0.0 {
            continue;
        }

        // Build the W → grism_param Mapping then invert. Mirrors
        // C:30551-30556 (ZoomMap(dgbyds) ∘ ShiftMap(gval), then
        // invert to get grism_param → w).
        let zoom = ZoomMap::new(1, dgbyds)?;
        let shift = ShiftMap::new(vec![gval]);
        let mut g_to_w: DynMapping = Box::new(CmpMap::new(
            Box::new(zoom),
            Box::new(shift),
            Composition::Series,
        )?);
        g_to_w.invert();

        // S → grism_param → w (the IWC mapping). Mirrors C:30564.
        let s_to_w: DynMapping = Box::new(CmpMap::new(s_to_grism, g_to_w, Composition::Series)?);
        let axmap = embed_axis(s_to_w, spec_iax, nwcs)?;

        // Build the CTYPE: orig + "-GRI"/"-GRA". Mirrors C:30571-30574.
        let ctype = format!("{orig_system_str}{suffix}");

        // PV parameters from the GrismMap. Mirrors C:30579-30592 —
        // skip values that match the FITS-WCS paper III defaults.
        let mut pv_pairs: Vec<(usize, f64)> = Vec::new();
        let g = grism.grism_g();
        if g != 0.0 {
            pv_pairs.push((0, g));
        }
        let m = grism.grism_m() as f64;
        if m != 0.0 {
            pv_pairs.push((1, m));
        }
        let alpha = grism.grism_alpha();
        if alpha != 0.0 {
            pv_pairs.push((2, alpha * DR2D));
        }
        let nr = grism.grism_nr();
        if nr != 1.0 {
            pv_pairs.push((3, nr));
        }
        let nrp = grism.grism_nrp();
        if nrp != 0.0 {
            pv_pairs.push((4, nrp));
        }
        let eps = grism.grism_eps();
        if eps != 0.0 {
            pv_pairs.push((5, eps * DR2D));
        }
        let theta = grism.grism_theta();
        if theta != 0.0 {
            pv_pairs.push((6, theta * DR2D));
        }

        return Ok(Some((axmap, ctype, pv_pairs, new_crval)));
    }

    Ok(None)
}

/// Recursively descend `frame` to its primary leaf for `axis`
/// (0-based). Same helper used in celestial_axes / sky_sys.
/// Whether `frame` is a `SpecFrame` or a `DSBSpecFrame` (which is a
/// SpecFrame subclass in the C type system). Mirrors C's
/// `astIsASpecFrame` returning true for both.
fn is_spec_or_dsb(frame: &dyn Frame) -> bool {
    use crate::frames::DSBSpecFrame;
    frame.as_any().is::<SpecFrame>() || frame.as_any().is::<DSBSpecFrame>()
}

/// Return the underlying `SpecFrame` for a `SpecFrame` or for the
/// inner spec of a `DSBSpecFrame`. Mirrors C's idiom of upcasting a
/// DSBSpecFrame pointer to SpecFrame for use with SpecFrame
/// accessors.
fn spec_or_inner(frame: &dyn Frame) -> Option<&SpecFrame> {
    use crate::frames::DSBSpecFrame;
    if let Some(sf) = frame.as_any().downcast_ref::<SpecFrame>() {
        return Some(sf);
    }
    if let Some(dsb) = frame.as_any().downcast_ref::<DSBSpecFrame>() {
        return Some(dsb.spec());
    }
    None
}

pub(crate) fn primary_frame(frame: &dyn Frame, axis: usize) -> (&dyn Frame, usize) {
    if let Some(cmp) = frame.as_any().downcast_ref::<crate::frames::CmpFrame>() {
        let n1 = cmp.naxes1();
        if axis < n1 {
            primary_frame(cmp.frame1(), axis)
        } else {
            primary_frame(cmp.frame2(), axis - n1)
        }
    } else {
        (frame, axis)
    }
}

/// Embed a 1-axis mapping into an `nax`-axis Mapping at position `iax`,
/// padding with UnitMaps before/after. Same algorithm as
/// [`super::other_axes`]'s `add_unit_maps`, kept self-contained here
/// to avoid pulling in cross-module visibility.
fn embed_axis(map: DynMapping, iax: usize, nax: usize) -> AstResult<DynMapping> {
    let nmap = map.nin();
    let mut ret = map;
    if iax > 0 {
        ret = Box::new(CmpMap::new(
            Box::new(UnitMap::new(iax)),
            ret,
            Composition::Parallel,
        )?);
    }
    if iax + nmap < nax {
        ret = Box::new(CmpMap::new(
            ret,
            Box::new(UnitMap::new(nax - iax - nmap)),
            Composition::Parallel,
        )?);
    }
    Ok(ret)
}

/// Find an output-axis value `val` that corresponds to an integer
/// input-pixel value on `axis`. Mirrors `fitschan.c::NearestPix`
/// (line 23151-23276): inverse-transform `val` (with BAD on the
/// other output axes) to pixel space, round each input axis to the
/// nearest integer, then forward-transform back. If the round-trip
/// yields a finite value, return it; otherwise return `val`
/// unchanged.
///
/// For a multi-axis mapping where the celestial inverse can't run
/// with BAD celestial inputs, C uses `astMapSplit` to extract a
/// 1-input-1-output sub-mapping for `axis`. We do the same via
/// `Mapping::map_split`.
fn nearest_pix_value(map: &dyn Mapping, val: f64, axis: usize) -> AstResult<f64> {
    if !val.is_finite() {
        return Ok(val);
    }
    let nin = map.nin();
    let nout = map.nout();
    if axis >= nout {
        return Ok(val);
    }

    // Try a 1-axis split: invert the map, split on the output axis
    // (which becomes the input after inversion), then invert back.
    // Mirrors C `astMapSplit(map, 1, &axis, &tmap)` after `astInvert`.
    let mut inv_map = map.clone_box();
    inv_map.invert();
    let split = inv_map.map_split(&[axis]);

    if let Some((out_axes, mut sub_map)) = split {
        if out_axes.len() == 1 {
            // sub_map is 1-in (the WCS-axis value) → 1-out (the
            // pixel-axis value). Inverting it gives 1-in (pixel) →
            // 1-out (WCS). Use it directly.
            let pix_in = vec![val];
            let pts_in = PointSet::from_rows([pix_in]);
            // sub_map: WCS → pixel.
            let pset_pix = sub_map.transform(&pts_in, true)?;
            let p = pset_pix.point(0)[0];
            if !p.is_finite() || is_bad(p) {
                return Ok(val);
            }
            let p_round = p.round();
            // Now go back: pixel → WCS via inverted sub_map.
            sub_map.invert();
            let pts_back = PointSet::from_rows([[p_round]]);
            let pset_back = sub_map.transform(&pts_back, true)?;
            let result = pset_back.point(0)[0];
            if result.is_finite() && !is_bad(result) {
                return Ok(result);
            }
            return Ok(val);
        }
    }

    // Fallback: full multi-axis NearestPix (mirrors C's else branch
    // when no split is available — same maths but with BAD on the
    // other axes).
    let mut row = vec![BAD_VALUE; nout];
    row[axis] = val;
    let pts_out = PointSet::from_rows([row]);

    // Inverse: WCS → pixel.
    let pset_pix = match map.transform(&pts_out, false) {
        Ok(p) => p,
        Err(_) => return Ok(val),
    };

    // Round each finite pixel coordinate to nearest integer.
    let mut pix_row = vec![0.0_f64; nin];
    for i in 0..nin {
        let v = pset_pix.point(0)[i];
        if v.is_finite() && !is_bad(v) {
            pix_row[i] = v.round();
        } else {
            return Ok(val);
        }
    }
    let pts_pix = PointSet::from_rows([pix_row]);

    // Forward: pixel → WCS.
    let pset_back = match map.transform(&pts_pix, true) {
        Ok(p) => p,
        Err(_) => return Ok(val),
    };
    let result = pset_back.point(0)[axis];
    if result.is_finite() && !is_bad(result) {
        Ok(result)
    } else {
        Ok(val)
    }
}

fn specsys_string(sor: StdOfRest) -> String {
    match sor {
        StdOfRest::Topocentric => "TOPOCENT".to_string(),
        StdOfRest::Geocentric => "GEOCENTR".to_string(),
        StdOfRest::Barycentric => "BARYCENT".to_string(),
        StdOfRest::Heliocentric => "HELIOCEN".to_string(),
        StdOfRest::Lsrk => "LSRK".to_string(),
        StdOfRest::Lsrd => "LSRD".to_string(),
        StdOfRest::Galactic => "GALACTOC".to_string(),
        StdOfRest::LocalGroup => "LOCAL".to_string(),
        StdOfRest::Source => "SOURCE".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::SpecSystem;
    use crate::frames::{BasicFrameBuilder, FrameRef, FrameSet};

    fn build_2axis_spec_fs() -> FrameSet {
        // Pixel(2) → [BasicFrame, SpecFrame] via a CmpFrame.
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let basic = BasicFrameBuilder::new(1).build().unwrap();
        let spec = SpecFrame::new(SpecSystem::Freq);
        let cmp = crate::frames::CmpFrame::new(Box::new(basic), Box::new(spec));
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(cmp))
            .unwrap();
        fs
    }

    #[test]
    fn no_spectral_axis_returns_unitmap() {
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let other = BasicFrameBuilder::new(2).build().unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(other))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let crvals = vec![None, None];
        let mut axis_done = vec![false; 2];

        let result = spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        assert!(result.as_any().is::<UnitMap>());
        assert!(!axis_done[0] && !axis_done[1]);
        assert!(store.ctype[0].is_none() && store.ctype[1].is_none());
    }

    #[test]
    fn try_grism_cascade_emits_gri_pv_and_chain() {
        // Direct test of try_grism_cascade: build a chain
        // pixel → ShiftMap → GrismMap whose output is read as a
        // wavelength SpecFrame. The helper should detect the grism
        // and return the -GRI-tagged WCS→IWC mapping plus the PV
        // parameter list. (We test the helper directly here rather
        // than through `spectral_axes`, because for many GrismMap
        // parameter sets the X2P cascade fires first — the C
        // priority order makes the grism path unreachable in those
        // cases. The helper itself needs to be correct in case the
        // X2P fails on a future fixture.)
        use crate::mappings::{GrismMap, ShiftMap as Sm};
        let basic = BasicFrameBuilder::new(1).build().unwrap();
        let _ = basic; // silence unused-variable warning
        let spec = SpecFrame::new(SpecSystem::Wavelen);

        // Non-trivial GrismMap (default_map has g=0,m=0 ⇒ degenerate).
        let g = GrismMap::new(1.0, 0.0, 1.0e-6, 0.0, 1.0e-6, 1, 0.0, 0.0);

        // pixel → ShiftMap → GrismMap (1-D); we extend to 2-D for
        // realism with axis 0 identity.
        let chain_1d = crate::mappings::CmpMap::new(
            Box::new(Sm::new(vec![-50.0])),
            Box::new(g.clone()),
            crate::mappings::Composition::Series,
        )
        .unwrap();
        let chain_2d = crate::mappings::CmpMap::new(
            Box::new(crate::mappings::UnitMap::new(1)),
            Box::new(chain_1d),
            crate::mappings::Composition::Parallel,
        )
        .unwrap();

        let lbnd_p = vec![1.0, 1.0];
        let ubnd_p = vec![100.0, 100.0];

        let result = try_grism_cascade(&spec, &chain_2d, 1, 2, &lbnd_p, &ubnd_p, "WAVE")
            .unwrap()
            .expect("expected -GRI detection");

        let (axmap, ctype, pv_pairs, new_crval) = result;
        assert_eq!(ctype, "WAVE-GRI");
        // Returned mapping has the right shape.
        assert_eq!(axmap.nin(), 2);
        assert_eq!(axmap.nout(), 2);
        // CRVAL is the grism's reference wavelength.
        assert!((new_crval - g.grism_wave_r()).abs() < 1e-9);
        // PV parameters: with g != 0 and m != 0 we should emit
        // PV-0 (= G) and PV-1 (= m).
        let pv_g = pv_pairs.iter().find(|(m, _)| *m == 0);
        assert!(pv_g.is_some(), "expected PV-0 (G)");
        let pv_m = pv_pairs.iter().find(|(m, _)| *m == 1);
        assert!(pv_m.is_some(), "expected PV-1 (m)");
    }

    #[test]
    fn x2p_cascade_picks_freq_for_wavelength_axis() {
        // Build a FrameSet whose spectral axis is linear in FREQ but
        // whose displayed system is WAVELEN. Pixel → FREQ is linear
        // (pixel is the underlying coordinate); WAVELEN = c/FREQ is
        // non-linear in pixel. The -X2P cascade should pick FREQ as
        // the X system, emitting CTYPE = "WAVE-F2W".
        use crate::mappings::MathMap;
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let basic = BasicFrameBuilder::new(1).build().unwrap();
        let spec = SpecFrame::new(SpecSystem::Wavelen);
        let cmp = crate::frames::CmpFrame::new(Box::new(basic), Box::new(spec));
        let mut fs = FrameSet::new(Box::new(pixel));

        // Build pixel→spec with axis 1 linear in pixel→FREQ space:
        //   freq = 1.0e9 + 1.0e6 * pixel
        //   wave = c / freq        (the SpecFrame WAVELEN system reports wavelength)
        // Forward (pixel → wavelength on axis 1):
        //   axis 0: identity
        //   axis 1: c / (1.0e9 + 1.0e6 * x2)
        let c = SPEED_OF_LIGHT;
        let fwd_x = "s0=x1".to_string();
        let fwd_y = format!("s1={c}/(1.0e9+1.0e6*x2)");
        let inv_x = "x1=s0".to_string();
        let inv_y = format!("x2=({c}/s1-1.0e9)/1.0e6");
        let pixel_to_wave = MathMap::new(2, 2, &[&fwd_x, &fwd_y], &[&inv_x, &inv_y]).unwrap();
        // Without explicit pixel→spec mapping this becomes the
        // pixel→WAVELEN chain.
        fs.add_frame(FrameRef::Base, Box::new(pixel_to_wave), Box::new(cmp))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        // Explicit CRVAL in WAVELEN = c / 1.05e9 ≈ 0.2855 m.
        let crval_wave = c / 1.05e9;
        let crvals = vec![None, Some(crval_wave)];
        let mut axis_done = vec![false; 2];

        spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // CTYPE should be "WAVE-F2W" — orig=WAVE, X=F (FREQ), P=W (orig is wavelength domain).
        assert_eq!(
            store.ctype[1].as_deref(),
            Some("WAVE-F2W"),
            "expected WAVE-F2W, got {:?}",
            store.ctype[1]
        );
    }

    #[test]
    fn freq_spec_axis_writes_keywords() {
        let fs = build_2axis_spec_fs();
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        // Override CRVAL for the spectral axis (axis 1 in WCS).
        let crvals = vec![None, Some(1.42e9)];
        let mut axis_done = vec![false; 2];

        let result = spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // Axis 1 is the spectral axis (CmpFrame second component).
        assert!(!axis_done[0] && axis_done[1]);
        assert_eq!(store.ctype[1].as_deref(), Some("FREQ"));
        // CUNIT mirrors C's `astGetUnit` which falls back to the
        // SpecSystem's display default — "GHz" for FREQ.
        assert_eq!(store.cunit[1].as_deref(), Some("GHz"));
        assert_eq!(store.crval[1], Some(1.42e9));
        // SPECSYS — whatever the SpecFrame default is (typically
        // HELIOCEN). Just assert it was set.
        assert!(store.specsys.is_some());

        // Mapping: 2-in / 2-out, axis 0 identity, axis 1 ShiftMap.
        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);
        // At pixel (10, 1.42e9), output should be (10, 0).
        let pts = PointSet::from_rows([[10.0_f64, 1.42e9]]);
        let out = result.transform(&pts, true).unwrap();
        let p = out.point(0);
        assert!((p[0] - 10.0).abs() < 1e-9);
        assert!(p[1].abs() < 1e-3);
    }

    #[test]
    fn restwav_emitted_for_wavelength_systems() {
        // Wavelen system + rest_freq → emit RESTWAV (m), not RESTFRQ.
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let basic = BasicFrameBuilder::new(1).build().unwrap();
        let mut spec = SpecFrame::new(SpecSystem::Wavelen);
        spec.set_rest_freq(1.42e9); // 21cm line
        let cmp = crate::frames::CmpFrame::new(Box::new(basic), Box::new(spec));
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(cmp))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let crvals = vec![None, Some(0.21)]; // ~21 cm wavelength
        let mut axis_done = vec![false; 2];

        spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        assert!(store.restwav.is_some());
        // Wavelength of 1.42 GHz line ≈ 0.211 m.
        let rw = store.restwav.unwrap();
        assert!((rw - SPEED_OF_LIGHT / 1.42e9).abs() < 1e-9);
        // RESTFRQ NOT emitted on wavelength systems.
        assert!(store.restfrq.is_none());
    }

    #[test]
    fn obsgeo_emitted_when_observer_set() {
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let basic = BasicFrameBuilder::new(1).build().unwrap();
        let mut spec = SpecFrame::new(SpecSystem::Freq);
        spec.set_obs_lon(0.0);
        spec.set_obs_lat(51.4778_f64.to_radians());
        spec.set_obs_alt(0.0);
        let cmp = crate::frames::CmpFrame::new(Box::new(basic), Box::new(spec));
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(cmp))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let crvals = vec![None, Some(1.42e9)];
        let mut axis_done = vec![false; 2];

        spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        let r = (store.obsgeox.unwrap().powi(2)
            + store.obsgeoy.unwrap().powi(2)
            + store.obsgeoz.unwrap().powi(2))
        .sqrt();
        assert!((r - 6_371_000.0).abs() < 50_000.0);
    }

    #[test]
    fn label_override_emits_ctype_comment_and_cname() {
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let basic = BasicFrameBuilder::new(1).build().unwrap();
        let mut spec = SpecFrame::new(SpecSystem::Freq);
        spec.set_axis_label("MyFrequency");
        let cmp = crate::frames::CmpFrame::new(Box::new(basic), Box::new(spec));
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(cmp))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let crvals = vec![None, Some(1.42e9)];
        let mut axis_done = vec![false; 2];

        spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        assert_eq!(store.ctype_com[1].as_deref(), Some("MyFrequency"));
        assert_eq!(store.cname[1].as_deref(), Some("MyFrequency"));
    }

    #[test]
    fn source_velocity_emits_zsource_ssyssrc() {
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let basic = BasicFrameBuilder::new(1).build().unwrap();
        let mut spec = SpecFrame::new(SpecSystem::Freq);
        // 1000 km/s recession.
        spec.set_source_vel(1.0e6);
        let cmp = crate::frames::CmpFrame::new(Box::new(basic), Box::new(spec));
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(cmp))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let crvals = vec![None, Some(1.42e9)];
        let mut axis_done = vec![false; 2];

        spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        let z = store.zsource.expect("ZSOURCE");
        // Relativistic Doppler: z = sqrt((c+v)/(c-v)) - 1.
        let expected = ((SPEED_OF_LIGHT + 1.0e6) / (SPEED_OF_LIGHT - 1.0e6)).sqrt() - 1.0;
        assert!((z - expected).abs() < 1e-9);
        assert!(store.ssyssrc.is_some());
    }

    #[test]
    fn crval_default_uses_bounds_midpoint() {
        let fs = build_2axis_spec_fs();
        let dim = vec![Some(100.0), Some(200.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let crvals = vec![None, None];
        let mut axis_done = vec![false; 2];

        let _ = spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // Pixel→WCS is identity. Spec-axis bounds via astMapBox are
        // [1, 200]; midpoint = 100.5; NearestPix snaps to integer
        // pixel coordinate 101. Mirrors fitschan.c:30325 (mid value
        // when dim is known) + :30333 (NearestPix snap).
        assert_eq!(store.crval[1], Some(101.0));
    }

    /// Build a 2-axis FrameSet with a SpecFrame whose Unit attribute
    /// is set to `unit_str`. Pixel→WCS is identity, so the spec axis
    /// reads pixel value directly (in `unit_str` units).
    fn build_spec_fs_with_unit(unit_str: &str, system: SpecSystem) -> FrameSet {
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let basic = BasicFrameBuilder::new(1).build().unwrap();
        let mut spec = SpecFrame::new(system);
        spec.set_unit(unit_str);
        let cmp = crate::frames::CmpFrame::new(Box::new(basic), Box::new(spec));
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(cmp))
            .unwrap();
        fs
    }

    #[test]
    fn linear_unit_mhz_preserves_user_cunit() {
        // SpecFrame in MHz, FREQ system. Linear conversion → CUNIT
        // should remain "MHz" (not be overridden to Hz).
        let fs = build_spec_fs_with_unit("MHz", SpecSystem::Freq);
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let crvals = vec![None, None];
        let mut axis_done = vec![false; 2];

        spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        assert_eq!(store.cunit[1].as_deref(), Some("MHz"));
    }

    #[test]
    fn nonlinear_unit_log_hz_absorbed_into_chain() {
        // SpecFrame with Unit="log(Hz)" on a FREQ axis. Non-linear
        // conversion → spectral_axes should absorb it: CUNIT becomes
        // the canonical default "Hz", CRVAL is transformed (10^x).
        let fs = build_spec_fs_with_unit("log(Hz)", SpecSystem::Freq);
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        // Supply CRVAL = 9.0 (log10 of 1 GHz). The absorbed CRVAL
        // should be 10^9 = 1e9 Hz.
        let crvals = vec![None, Some(9.0)];
        let mut axis_done = vec![false; 2];

        spectral_axes(SpectralAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            crvals: &crvals,
            axis_done: &mut axis_done,
        })
        .unwrap();

        // CUNIT was overridden to the canonical default.
        assert_eq!(store.cunit[1].as_deref(), Some("Hz"));
        // CRVAL was transformed through 10^x: 9.0 → 1e9.
        let crval = store.crval[1].expect("crval");
        assert!(
            (crval - 1.0e9).abs() < 1.0,
            "CRVAL absorbed via 10^x: expected 1e9, got {crval}"
        );
    }
}

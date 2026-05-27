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
//! - **`GrismMap` handling** (`-GRI` / `-GRA`). Needs an
//!   `extract_grism_map(chain)` walker plus GrismMap parameter
//!   accessors.
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
//! via `try_x2p_cascade`, `-LOG` algorithm via `LogAxis`, RESTFRQ
//! vs RESTWAV choice, OBSGEO emission, axis-Label → CTYPE-comment
//! /CNAME, source-velocity → ZSOURCE/SSYSSRC.

// Wired in by `fset_to_store::fset_to_store` (Group 7); until then
// only reachable from cfg(test).
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{Frame, FrameRef, FrameSet, SpecFrame, SpecSystem, StdOfRest};
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, UnitMap};

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

    // Step 1: locate the spectral axis. Mirrors C:30243-30248.
    let mut spec_axis: Option<usize> = None;
    for iax in 0..nwcs {
        let (pframe, _paxis) = primary_frame(wcsfrm, iax);
        if pframe.as_any().is::<SpecFrame>() {
            spec_axis = Some(iax);
            break;
        }
    }
    let Some(spec_iax) = spec_axis else {
        return Ok(Box::new(UnitMap::new(nwcs)));
    };

    // Re-resolve the SpecFrame so we can read its attributes.
    let (spec_holder, _) = primary_frame(wcsfrm, spec_iax);
    let specfrm = match spec_holder.as_any().downcast_ref::<SpecFrame>() {
        Some(s) => s,
        None => return Ok(Box::new(UnitMap::new(nwcs))),
    };

    let fits_i = args.wperm[spec_iax];

    // Step 2: choose CRVAL — caller override (if non-BAD) wins, else
    // central-pixel WCS coord. Mirrors C:30331-30340 (which uses
    // crvals[iax] if non-BAD, else the WCS value at the central pixel).
    let crval_input = args.crvals.get(spec_iax).copied().flatten();
    let crval = match crval_input {
        Some(v) if !is_bad(v) => v,
        _ => {
            // Compute WCS value at the central pixel (mirrors the
            // OtherAxes central-pixel approach).
            let map = args.fs.mapping(FrameRef::Base, FrameRef::Current)?;
            let npix = map.nin();
            let centre: Vec<f64> = (0..npix)
                .map(|i| match args.dim.get(i).copied().flatten() {
                    Some(d) => (0.5 * d).floor(),
                    None => 1.0,
                })
                .collect();
            let pts = PointSet::from_rows([centre]);
            let out = map.transform(&pts, true)?;
            let v = out.point(0)[spec_iax];
            if is_bad(v) {
                // Can't proceed without a valid CRVAL.
                return Ok(Box::new(UnitMap::new(nwcs)));
            }
            v
        }
    };

    // Step 3: write keywords. Mirrors fitschan.c populate paths
    // (we cover the canonical-form subset that wcs_write::populate_spectral
    // also covers).
    args.store.crval[fits_i] = Some(crval);

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
    let map = args.fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let npix = map.nin();
    let lbnd_p: Vec<f64> = vec![1.0; npix];
    let ubnd_p: Vec<f64> = (0..npix)
        .map(|i| args.dim.get(i).copied().flatten().unwrap_or(500.0))
        .collect();

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
        } else {
            // Fallback: build the same ShiftMap as the linear path
            // (without an algorithm-suffix CTYPE). make_int_world
            // will reject the resulting non-linear pixel→IWC.
            // Grism / -TAB are not yet ported.
            let shift = ShiftMap::new(vec![-crval]);
            let mapping = embed_axis(Box::new(shift), spec_iax, nwcs)?;
            (mapping, orig_system_str.to_string())
        };

    args.store.ctype[fits_i] = Some(ctype);

    // CUNIT: explicit user-set unit if present, else the system's
    // SpecMap canonical unit (Hz / m / m/s).
    let cunit = specfrm
        .unit()
        .filter(|u| !u.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| system.specmap_units().to_string());
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

    // Step 4: VELOSYS — topocentric apparent velocity of the SPECSYS
    // rest frame. Mirrors C:30774-30796. Currently NOT YET WIRED:
    // VELOSYS depends on the celestial source position from
    // celestial_axes (RefRA/RefDec on the SpecFrame, plus any
    // SkyFrame source). The Group 7 wiring will deliver it. The
    // existing wcs_write::populate_velosys already does this math;
    // we'll port that into spectral_axes when the celestial→spectral
    // sequencing lands.

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

/// Recursively descend `frame` to its primary leaf for `axis`
/// (0-based). Same helper used in celestial_axes / sky_sys.
fn primary_frame(frame: &dyn Frame, axis: usize) -> (&dyn Frame, usize) {
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
        // Default CUNIT for FREQ system is "Hz".
        assert_eq!(store.cunit[1].as_deref(), Some("Hz"));
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
    fn crval_default_uses_central_pixel() {
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

        // Pixel→WCS is identity, so central-pixel WCS = floor(0.5*200) = 100.
        assert_eq!(store.crval[1], Some(100.0));
    }
}

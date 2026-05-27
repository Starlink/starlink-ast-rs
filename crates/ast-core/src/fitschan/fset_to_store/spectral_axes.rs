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
//! This is by far the largest gap in the port — most of the C
//! function is **not yet implemented**. The Rust body covers only
//! the canonical-form subset (CTYPE = system name, CUNIT = canonical
//! unit, single-axis ShiftMap WCS→IWC). Lack of wcsconv fixture
//! coverage is not an excuse for under-porting — see
//! [`feedback_test_coverage_is_separate_from_porting`] in user
//! memory.
//!
//! Tracked in
//! [`docs/issues/fset_to_store-port-gaps.md`](../../../../../docs/issues/fset_to_store-port-gaps.md):
//!
//! - **Algorithm-code suffix derivation** for non-canonical SpecMap
//!   chains: `-LOG`, `-F2W`, `-V2F`, `-OHL`, `-WAV-from-FRQ`, etc.
//!   (FITS-WCS paper III). Without this, headers like
//!   `vopt-f2w.head`, `wave-f2w.head`, `class-arc.head` get the
//!   wrong CTYPE.
//! - **`GrismMap` handling** (`-GRI` / `-GRA`).
//! - **`VELOSYS` calculation** — needs the celestial source position
//!   from `celestial_axes`. Wiring step is in Group 7.
//! - **SpecFrame unit-conversion rewriting** when the spectral
//!   system requires a unit change to be FITS-compliant.
//! - **`SourceVel` / `SourceVRF` / Redshift handling**.
//!
//! Until those land, the canonical-form subset matches what the
//! legacy `wcs_write::populate_spectral` covers, so Group 7 wiring
//! won't regress against the existing fixtures — but it will
//! produce wrong output for any new fixture that exercises a
//! deferred branch.

// Wired in by `fset_to_store::fset_to_store` (Group 7); until then
// only reachable from cfg(test).
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{Frame, FrameRef, FrameSet, SpecFrame, StdOfRest};
use crate::mappings::{CmpMap, Composition, DynMapping, ShiftMap, UnitMap};

use crate::mappings::helpers::is_bad;

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
    args.store.ctype[fits_i] = Some(system.as_str().to_string());

    // CUNIT: explicit user-set unit if present, else the system's
    // SpecMap canonical unit (Hz / m / m/s).
    let cunit = specfrm
        .unit()
        .filter(|u| !u.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| system.specmap_units().to_string());
    args.store.cunit[fits_i] = Some(cunit);

    // SPECSYS from the standard-of-rest.
    args.store.specsys = Some(specsys_string(specfrm.std_of_rest()));

    // RESTFRQ if set.
    if let Some(rf) = specfrm.rest_freq() {
        args.store.restfrq = Some(rf);
    }

    // Step 4: build the WCS→IWC mapping for the spectral axis. The
    // shape is a single-axis ShiftMap (subtract CRVAL) embedded into
    // a nwcs-axis mapping via parallel UnitMaps. Mirrors C's
    // wide-axis ShiftMap construction at the tail of SpectralAxes
    // (which combines the spectral-axis Mapping with UnitMaps on
    // non-spectral axes via parallel CmpMap and a PermMap to put
    // the spectral axis at its slot).
    let shift = ShiftMap::new(vec![-crval]);
    let mut nd = embed_axis(Box::new(shift), spec_iax, nwcs)?;

    // Mark done and return.
    args.axis_done[spec_iax] = true;

    nd = nd.simplified()?;
    Ok(nd)
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

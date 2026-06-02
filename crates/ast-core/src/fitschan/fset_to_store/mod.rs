//! Port of `astFsetToStore` and its callees (`CelestialAxes`,
//! `SpectralAxes`, `OtherAxes`, `MakeIntWorld`, `SIPIntWorld`).
//!
//! Builds a [`FitsStore`](super::fits_store::FitsStore) from a
//! `FrameSet` by constructing the `wcs→iwc` and `pixel→iwc` mappings
//! from chosen header values, then extracting CRPIX / CD / PC / CDELT
//! via a least-squares fit. Mirrors `fitschan.c` (function references
//! on each item).
//!
//! See `docs/superpower/specs/2026-05-25-makeintworld-port-design.md`
//! for the design.
//!
//! Naming note: the upstream-equivalent module name `wcs_keywords/`
//! collides with the existing `fitschan/wcs_keywords.rs` reading-side
//! helper; we use `fset_to_store/` (matching the C entry-point name)
//! to keep the read/write halves of the pipeline distinguishable.

// `fset_to_store` is wired into `wcs_write::store_from_frameset_to`
// in Task 7.2; until then nothing here is reachable from outside the
// cfg(test) block, so allow dead_code for the whole module. The
// index-style loops in snap_small_crval / round_crpix mirror the C
// source line-for-line; revisited in the post-validation idiomatic
// pass.
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

mod celestial_axes;
pub(crate) mod fiducial;
mod grism;
mod invertable;
mod is_map_linear;
mod log_axis;
mod make_int_world;
mod orth;
mod other_axes;
mod sip_int_world;
pub(crate) mod sky_pole;
mod sky_sys;
mod spectral_axes;
mod world_axes;

use crate::error::AstResult;
use crate::fitschan::Encoding;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{FrameRef, FrameSet};
use crate::mappings::{CmpMap, Composition, DynMapping};

use celestial_axes::{CelestialAxesArgs, celestial_axes};
use make_int_world::make_int_world;
use other_axes::{OtherAxesArgs, other_axes};
use spectral_axes::{SpectralAxesArgs, spectral_axes};
use world_axes::world_axes;

/// Default `FitsTol` value (matches the C library's FitsChan default).
/// Mirrors C `astGetFitsTol` default at fitschan.c:2898.
const FITS_TOL_DEFAULT: f64 = 0.1;

/// Build a [`FitsStore`] describing the primary plane of the supplied
/// FrameSet. Mirrors the body of `fitschan.c::AddVersion` (~2674) for
/// the `s == ' '` (primary) case — `FsetToStore` itself is a thin
/// wrapper that dispatches to `AddVersion` per coordinate-version
/// letter, but the heavy lifting (building the WCS→IWC mapping
/// segment-by-segment, then extracting CRPIX/CD/PC/CDELT) is here.
///
/// `dim[i]` carries each base-frame axis's pixel length (`None` when
/// unknown — mirrors C `dim[i] == AST__BAD`). The returned `FitsStore`
/// has length `naxis = fs.base().naxes()`.
///
/// `encoding` selects the FITS-WCS variant; this gates whether SIP
/// distortion headers may be emitted.
///
/// Currently emits primary-plane keywords only. Alternate axis
/// descriptions (the `s = 'A'..'Z'` loop in C `FsetToStore` after the
/// primary `AddVersion` call) are out of scope for this port.
pub(crate) fn fset_to_store(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
    s: char,
) -> AstResult<FitsStore> {
    // Step 1: pixel→WCS Mapping from base to current frame.
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;

    let nwcs = mapping.nout();
    // FitsStore needs to be wide enough to hold a slot for every WCS
    // axis as well as every base (pixel) axis: when nwcs > base.naxes()
    // the FrameSet has degenerate axes whose CRVAL / CTYPE still need
    // to be stored. The C library handles this via FitsStore's dynamic
    // SetItem; we pre-size to max(naxes, nwcs).
    let naxis = fs.base().naxes().max(nwcs);
    let mut store = FitsStore::new(naxis);

    // Reject inputs whose inverse is undefined (mirrors C:2782 — the
    // -TAB fallback path in C is not yet ported, so for now we mirror
    // the non-TAB return).
    if !mapping.has_inverse() {
        return Ok(store);
    }

    // Step 2: choose `wperm[i]` — the FITS axis index for each WCS
    // frame axis. Mirrors C:2792-2802 (FitsAxisOrder + WorldAxes
    // fallback). FitsAxisOrder isn't yet modelled; the auto path via
    // WorldAxes is what the C library defaults to.
    let mut wperm: Vec<usize> = match world_axes(mapping.as_ref(), dim, FITS_TOL_DEFAULT)? {
        Some(p) => p,
        None => (0..nwcs).collect(),
    };

    // Step 3: per-axis "this axis has been described" flags.
    let mut axis_done = vec![false; nwcs];

    // Step 4: per-axis chosen CRVAL — populated by celestial_axes /
    // spectral_axes for the axes they own; remaining `None` slots
    // carry through to other_axes which falls back to world(g0).
    // Mirrors C's `crvals` array (allocated by ReadCrval @ 2812).
    let mut crvals: Vec<Option<f64>> = vec![None; nwcs];

    // Step 5: build the WCS→IWC mapping segment by segment.
    //
    // The structure mirrors C:2826-2848:
    //   iwcmap = CelestialAxes(...)
    //   tmap   = SpectralAxes(...)
    //   iwcmap = iwcmap ∘ tmap
    //   tmap   = OtherAxes(...)
    //   iwcmap = iwcmap ∘ tmap
    //
    // Each segment defaults to UnitMap on the axes it doesn't claim,
    // so series composition leaves those axes untouched.
    let cel_seg = celestial_axes(CelestialAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        s,
        store: &mut store,
        axis_done: &mut axis_done,
        isoff: 0,
    })?;

    let spec_seg = spectral_axes(SpectralAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        s,
        store: &mut store,
        crvals: &crvals,
        axis_done: &mut axis_done,
    })?;

    let other_seg = other_axes(OtherAxesArgs {
        fs,
        dim,
        wperm: &mut wperm,
        store: &mut store,
        crvals: &mut crvals,
        axis_done: &mut axis_done,
    })?;

    let wcs_iwc: DynMapping = Box::new(CmpMap::new(cel_seg, spec_seg, Composition::Series)?);
    let wcs_iwc: DynMapping = Box::new(CmpMap::new(wcs_iwc, other_seg, Composition::Series)?);

    // Step 6: pixel→IWC = pixel→WCS ∘ WCS→IWC. Mirrors C:2853.
    let pixel_iwc: DynMapping = Box::new(CmpMap::new(
        mapping.clone_box(),
        wcs_iwc,
        Composition::Series,
    )?);

    // Step 7: extract CRPIX / CD / PC / CDELT (and SIP if enabled)
    // via make_int_world. Mirrors C:2870-2871.
    let sip_ok = matches!(encoding, Encoding::FitsWcs);
    let _ok = make_int_world(
        pixel_iwc.as_ref(),
        fs.current(),
        &wperm,
        &mut store,
        dim,
        FITS_TOL_DEFAULT,
        sip_ok,
    )?;

    // Step 8: post-processing.
    //
    // (a) Store the WCS Frame's Domain as WCSNAME when explicitly set.
    //     Mirrors C:2892-2895 (which uses astTestDomain). Each Frame
    //     class returns true from `test_domain` only when its domain
    //     attribute has been set by the user, distinguishing that
    //     from the per-class default ("SKY", "SPECTRUM", "TIME", …).
    //     Don't overwrite a value sky_sys already populated.
    if store.wcsname.is_none() {
        let cur = fs.current();
        if cur.test_domain() {
            let d = cur.domain();
            if !d.is_empty() {
                store.wcsname = Some(d.to_string());
            }
        }
    }

    // (b) Snap small-CRVAL residue to zero (when |CRVAL| is much
    //     smaller than the per-pixel scale). Mirrors C:2920-2942.
    snap_small_crval(&mut store, &wperm, nwcs);

    // (c) Round CRPIX values to the nearest microsecond pixel.
    //     Mirrors C:2945-2954.
    round_crpix(&mut store);

    Ok(store)
}

/// For each WCS axis: if `|CRVAL| < sqrt(DBL_EPSILON · cdelt²)`, set
/// CRVAL = 0. The squared-pixel-scale `cdelt2` multiplies the squared
/// CDELT by the squared L2 norm of the PC row. Mirrors fitschan.c:2920-2942.
fn snap_small_crval(store: &mut FitsStore, wperm: &[usize], nwcs: usize) {
    for iax in 0..nwcs {
        let fits_i = wperm[iax];
        let Some(crval) = store.crval.get(fits_i).copied().flatten() else {
            continue;
        };
        // L2 norm² of the PC row (defaulted to identity).
        let mut row_norm2 = 0.0_f64;
        for fits_j in 0..nwcs {
            let pc = store
                .pc
                .get(fits_i)
                .and_then(|row| row.get(fits_j))
                .copied()
                .flatten()
                .unwrap_or(if fits_i == fits_j { 1.0 } else { 0.0 });
            row_norm2 += pc * pc;
        }
        let cdelt = store.cdelt.get(fits_i).copied().flatten().unwrap_or(1.0);
        let cdelt2 = row_norm2 * cdelt * cdelt;
        if crval.abs() < (f64::EPSILON * cdelt2).sqrt() {
            store.crval[fits_i] = Some(0.0);
        }
    }
}

/// Round each CRPIX value to the nearest microsecond-of-a-pixel.
/// Mirrors fitschan.c:2945-2954.
fn round_crpix(store: &mut FitsStore) {
    for slot in store.crpix.iter_mut() {
        if let Some(v) = slot {
            *slot = Some((*v * 1.0e6 + 0.5).floor() * 1.0e-6);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{BasicFrameBuilder, FrameSet};
    use crate::mappings::{ShiftMap, UnitMap};

    /// Smoke test: a 2-axis BasicFrame → BasicFrame chain with no
    /// celestial / spectral axes, using a per-axis ShiftMap. Expect
    /// fset_to_store to return a populated FitsStore where
    /// other_axes covers both axes and make_int_world recovers the
    /// shift-only CRPIX/CD.
    #[test]
    fn basic_two_axis_shift() {
        let pixel = BasicFrameBuilder::new(2)
            .domain("GRID")
            .label(1, "x")
            .label(2, "y")
            .build()
            .unwrap();
        let world = BasicFrameBuilder::new(2)
            .domain("WORLD")
            .label(1, "wx")
            .label(2, "wy")
            .unit(1, "m")
            .unit(2, "m")
            .build()
            .unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(
            FrameRef::Base,
            Box::new(ShiftMap::new(vec![10.0, 20.0])),
            Box::new(world),
        )
        .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let store = fset_to_store(&fs, &dim, Encoding::FitsWcs, ' ').unwrap();

        // Both axes should have CTYPE / CUNIT populated by other_axes.
        assert!(store.ctype[0].is_some(), "CTYPE1 should be set");
        assert!(store.ctype[1].is_some(), "CTYPE2 should be set");
        // CRPIX populated.
        assert!(store.crpix[0].is_some(), "CRPIX1 should be set");
        assert!(store.crpix[1].is_some(), "CRPIX2 should be set");
        // WCSNAME from the WORLD domain.
        assert_eq!(store.wcsname.as_deref(), Some("WORLD"));
    }

    /// Identity mapping → CRPIX lands at the central pixel
    /// `floor(0.5*(1+dim))`. With dim=100 that's 50; CRVAL is then
    /// world(50) = 50, IWC at g0 is 0, and the CRPIX solve recovers
    /// pixel coord = 50 (where IWC = 0 under the identity).
    #[test]
    fn identity_chain_yields_central_pixel_crpix() {
        let pixel = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
        let world = BasicFrameBuilder::new(2)
            .domain("WORLD")
            .label(1, "wx")
            .label(2, "wy")
            .build()
            .unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(world))
            .unwrap();

        let dim = vec![Some(100.0), Some(100.0)];
        let store = fset_to_store(&fs, &dim, Encoding::FitsWcs, ' ').unwrap();
        let crpix0 = store.crpix[0].unwrap();
        let crpix1 = store.crpix[1].unwrap();
        // dim=100 ⇒ g0 = floor(0.5*(1+100)) = 50.
        assert!((crpix0 - 50.0).abs() < 1e-6, "CRPIX1 = {crpix0}");
        assert!((crpix1 - 50.0).abs() < 1e-6, "CRPIX2 = {crpix1}");
    }
}

//! Port of `fitschan.c::CelestialAxes` (~4890).
//!
//! Identifies the celestial pair of axes in the FrameSet's current
//! frame, stores celestial-axis FITS keywords (CRVAL / CTYPE / PV /
//! RADESYS / EQUINOX / MJD-OBS) in the supplied [`FitsStore`], and
//! returns the WCS→IWC mapping segment for those axes.
//!
//! ## TODO: branches not yet ported
//!
//! Several C branches are not yet covered. They MUST be ported (lack
//! of wcsconv fixture coverage is not an excuse — see
//! [`feedback_test_coverage_is_separate_from_porting`] in user
//! memory). Tracked in
//! [`docs/issues/fset_to_store-port-gaps.md`](../../../../../docs/issues/fset_to_store-port-gaps.md):
//!
//! - **`-TAB` algorithm fallback** (C:5396-5580). When no WcsMap is
//!   present in the pixel→WCS chain, C falls through to `IsMapTab2D`.
//!   The Rust port returns UnitMap, which is silently wrong for
//!   `-TAB`-describable celestial fixtures. Blocked on `FitsTable`
//!   infrastructure.
//! - **`SkyPole` / LONPOLE+LATPOLE storage** (C:5189). C writes
//!   `lonpole`/`latpole` keywords from the WcsMap's PV(lonax, 3/4)
//!   and the SkyFrame's `SkyRef`. Headers that depend on non-default
//!   LONPOLE/LATPOLE won't round-trip without these.
//!
//! Note on `FitsAxisOrder`: the C function gates the auto axis-swap
//! on `astChrMatch(astGetFitsAxisOrder(this), "<auto>")` (C:5318).
//! The Rust writer doesn't yet model the `FitsAxisOrder` FitsChan
//! attribute — the only mode reachable today is `"<auto>"`, which is
//! also the C library default. Once we add `FitsAxisOrder`, gate the
//! swap below on it.

// Wired in by `fset_to_store::fset_to_store` (Group 7); until then
// only reachable from cfg(test) and downstream sibling modules.
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_range_contains)] // mirrors C's `m < 3 || m > 4` form
#![allow(clippy::too_many_arguments)] // build_wcs_to_iwc mirrors C signature

use std::f64::consts::PI;

use crate::error::AstResult;
use crate::fitschan::fits_store::FitsStore;
use crate::frames::{Frame, FrameRef, FrameSet, SkyFrame};
use crate::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MatrixMap, PermMap, ShiftMap, UnitMap, WcsMap,
};
use crate::wcslib::WcsProjection;

use super::fiducial::{get_fiducial_nsc, get_fiducial_wcs};
use super::sky_pole::sky_pole;
use super::sky_sys::sky_sys;
use super::world_axes::world_axes;

/// The writer's celestial-axis predicate, mirroring the C macro
/// `#define IsASkyFrame(frame) (astIsASkyFrame(frame) &&
/// !strcmp("SKY",astGetDomain(frame)))` (fitschan.c:1365).
///
/// A SkyFrame is only treated as a *celestial* axis by `CelestialAxes`
/// when its Domain is exactly `"SKY"`. An offset SkyFrame carries a
/// `SKY_ORIGIN` / `SKY_POLE` Domain (stamped on read by `WcsSkyFrame`
/// and preserved by `TidyOffsets`), so it deliberately fails this test
/// and falls through to `OtherAxes`, which emits the linear OFLN / OFLT
/// axis description.
fn is_a_sky_frame(frame: &dyn Frame) -> bool {
    frame.as_any().is::<SkyFrame>() && frame.domain() == "SKY"
}

/// `MXPAR` from wcslib's PV-parameter table — mirrors the C
/// `WCSLIB_MXPAR` constant used in the PV-loop bound at
/// fitschan.c:5150. Our Rust `WcsMap::get_pv` exposes the same range.
const WCSLIB_MXPAR: usize = 100;

const DD2R: f64 = PI / 180.0;
const DR2D: f64 = 180.0 / PI;

/// Arguments to [`celestial_axes`]. Mirrors the C signature but
/// uses Rust-idiomatic option types.
pub(crate) struct CelestialAxesArgs<'a> {
    /// FrameSet whose base is FITS pixel coords and whose current
    /// is FITS WCS coords.
    pub fs: &'a FrameSet,
    /// Image dimensions per base axis.
    pub dim: &'a [Option<f64>],
    /// 0-based FITS-axis index for each WCS-frame axis.
    pub wperm: &'a mut [usize],
    /// Coordinate-version character (`' '` for primary).
    pub s: char,
    /// FITS keyword store; updated in place.
    pub store: &'a mut FitsStore,
    /// Per-axis "this axis has been described" flags; updated in
    /// place — the celestial pair is marked done on success.
    pub axis_done: &'a mut [bool],
    /// `isoff` flag, as documented in
    /// [`fset_to_store::sky_sys::sky_sys`](super::sky_sys::sky_sys).
    pub isoff: i32,
    /// Linearity tolerance in pixels, from the FitsChan's `FitsTol`
    /// attribute (C `astGetFitsTol`, fitschan.c:2987).
    pub fits_tol: f64,
}

/// Detect celestial axes in `args.fs`, populate the celestial
/// FITS-keyword set in `args.store`, and return the WCS→IWC mapping
/// segment for those axes (`UnitMap` if the FrameSet has no celestial
/// axes).
///
/// Mirrors `fitschan.c::CelestialAxes` (~4890). See the module head
/// for deferred branches.
pub(crate) fn celestial_axes(args: CelestialAxesArgs<'_>) -> AstResult<DynMapping> {
    let wcsfrm = args.fs.current();
    let nwcs = wcsfrm.naxes();

    // Step 1: locate the celestial axes via primary-frame walk.
    // Mirrors C:5050-5078.

    let mut ilon: Option<usize> = None;
    let mut ilat: Option<usize> = None;
    for iax in 0..nwcs {
        let (pframe, paxis) = wcsfrm.primary_frame(iax);
        if is_a_sky_frame(pframe) {
            if paxis == 0 {
                ilon = Some(iax);
            } else {
                ilat = Some(iax);
            }
            args.axis_done[iax] = true;
        }
    }

    let (ilon, ilat) = match (ilon, ilat) {
        (Some(a), Some(b)) => (a, b),
        _ => return Ok(Box::new(UnitMap::new(nwcs))),
    };

    // Re-locate the SkyFrame for downstream property reads. We can't
    // hold both an immutable wcsfrm borrow and the `ilon`/`ilat` we
    // captured above and then call methods on a downcast SkyFrame
    // through that immutable borrow — but the data is read-only so
    // a fresh resolve gives us the same reference.
    let (skyframe_holder, _) = wcsfrm.primary_frame(ilon);
    let skyfrm = match skyframe_holder.as_any().downcast_ref::<SkyFrame>() {
        Some(s) => s,
        None => return Ok(Box::new(UnitMap::new(nwcs))),
    };

    // Step 2: pixel→wcs mapping, simplified. Mirrors C:5092-5094.
    let map_unsimp = args.fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let map = map_unsimp.simplify()?;

    // Step 3: split the chain around its first WcsMap. Mirrors
    // C:5109-5111 (`SplitMap`). For the WcsMap-less case we fall
    // through to a deferred path (TAB or non-celestial fallback) and
    // return identity over the celestial axes.
    let (_pre, wm_lifted, post) = match split_around_wcsmap(map.as_ref()) {
        Some(parts) => parts,
        None => {
            // No WcsMap → `-TAB` path is not yet ported. Mirrors the
            // structure of fitschan.c:5396 (TAB branch) which we skip;
            // the caller (Group 7) will see a UnitMap return value
            // and rely on `make_int_world` to flag the residual
            // non-linearity downstream.
            return Ok(Box::new(UnitMap::new(nwcs)));
        }
    };

    // Use the *lifted* WcsMap returned by SplitMap2 so the
    // longitude / latitude axis indices report the full-pipeline
    // positions (mirrors C `astGetWcsAxis(map2, …)` after the lift
    // at C:33092-33106). Falling back to `find_wcsmap` would yield
    // the un-lifted 2-axis WcsMap embedded inside a parallel
    // composition, whose internal axes are 0,1 regardless of where
    // the celestial pair sits in the FrameSet.
    //
    // `wcsmap_full` carries the full-pipeline axis indices and is
    // what we hand to PV-aware reads (`get_pv`, `lonax`, `latax`).
    // `wcsmap` (a 2-axis copy with internal axes 0,1) is what
    // `build_wcs_to_iwc` uses to build the celestial rotation —
    // that helper already embeds the 2-axis chain into the
    // larger pipeline via PermMaps, so passing the lifted WcsMap
    // would double-count the embedding.
    let wcsmap_full = match wm_lifted.as_any().downcast_ref::<WcsMap>() {
        Some(w) => w.clone(),
        None => return Ok(Box::new(UnitMap::new(nwcs))),
    };
    let wcsmap = match find_wcsmap(map.as_ref()) {
        Some(w) => w,
        None => return Ok(Box::new(UnitMap::new(nwcs))),
    };
    let axlon_0 = wcsmap_full.lonax_0based();
    let axlat_0 = wcsmap_full.latax_0based();
    let axlon_1 = wcsmap_full.lonax();
    let axlat_1 = wcsmap_full.latax();
    let projection = wcsmap.projection();
    let tpn = projection == WcsProjection::Tpn;

    // Step 4: compute the celestial fiducial via `post` (native →
    // celestial). Mirrors C:5121.
    //
    // The "post" chain has WcsMap inputs as its inputs and WCS
    // outputs as its outputs. For zenithal projections the WcsMap's
    // fiducial native-spherical point is at (0, π/2); for non-zenithal
    // it can be PV-overridden — `get_fiducial_wcs` handles both.
    let (skyfid_lon_rad, skyfid_lat_rad) =
        match get_fiducial_wcs(&wcsmap_full, post.as_ref(), ilon, ilat)? {
            Some((a, b)) => (a, b),
            None => return Ok(Box::new(UnitMap::new(nwcs))),
        };

    // Normalise: replace BAD with 0, snap near-zero angles, then
    // call SkyFrame::norm to wrap each axis into its conventional
    // range. C:5134-5138.
    let mut skyfid = [skyfid_lon_rad, skyfid_lat_rad];
    let lonax_in_sky = skyfrm.lon_pos();
    let latax_in_sky = skyfrm.lat_pos();
    if zero_angle(skyfid[0]) {
        skyfid[0] = 0.0;
    }
    if zero_angle(skyfid[1]) {
        skyfid[1] = 0.0;
    }
    // Reorder into SkyFrame's native (lon, lat) layout for the call,
    // then read back from the SkyFrame-native order.
    let mut sky_native = [0.0_f64; 2];
    sky_native[lonax_in_sky] = skyfid[0];
    sky_native[latax_in_sky] = skyfid[1];
    skyfrm.norm(&mut sky_native)?;
    let crval_lon_deg = DR2D * sky_native[lonax_in_sky];
    let crval_lat_deg = DR2D * sky_native[latax_in_sky];

    let fits_ilon = args.wperm[ilon];
    let fits_ilat = args.wperm[ilat];
    args.store.crval[fits_ilon] = Some(crval_lon_deg);
    args.store.crval[fits_ilat] = Some(crval_lat_deg);

    // Step 5: copy PV parameters into the store. Mirrors C:5150-5163.
    // The longitude axis skips slots 3 and 4 (LONPOLE/LATPOLE
    // overrides) for non-TPN projections.
    //
    // PV reads use `wcsmap_full` (the lifted WcsMap) so the
    // `(axlon_1, axlat_1)` indices match the full-pipeline axis
    // numbering.
    for m in 0..WCSLIB_MXPAR {
        if let Some(pv) = wcsmap_full.stored_pv(axlon_1, m)
            && pv.is_finite()
            && (m < 3 || m > 4 || tpn)
            && fits_ilon < args.store.pv.len()
            && m < args.store.pv[fits_ilon].len()
        {
            args.store.pv[fits_ilon][m] = Some(pv);
        }
        if let Some(pv) = wcsmap_full.stored_pv(axlat_1, m)
            && pv.is_finite()
            && fits_ilat < args.store.pv.len()
            && m < args.store.pv[fits_ilat].len()
        {
            args.store.pv[fits_ilat][m] = Some(pv);
        }
    }

    // Step 6: PPC fiducial shift (only when WcsMap PV(lonax, 0) != 0
    // and not TPN). Mirrors C:5175-5182. Non-zero PV(lonax, 0)
    // signals that the fiducial point isn't the projection reference
    // point, so PPC ≠ IWC and a translation is needed. We compute it
    // by inverting the WcsMap on the native fiducial.
    let mut ppcfid = vec![0.0_f64; nwcs];
    if !tpn && wcsmap_full.stored_pv(axlon_1, 0).unwrap_or(0.0) != 0.0 {
        if let Some((x0, y0)) = compute_fiducial_ppc(wcsmap)? {
            // x0/y0 are radians at the native lon/lat axes — convert
            // to degrees for IWC and place at ilon/ilat positions in
            // the nwcs-wide vector.
            ppcfid[ilon] = DR2D * x0;
            ppcfid[ilat] = DR2D * y0;
        }
    }

    // Step 7: SkySys writes CTYPE / RADESYS / EQUINOX / MJD-OBS /
    // CUNIT. Mirrors C:5185-5186.
    let _ = sky_sys(
        skyfrm, 1, projection, args.store, fits_ilon, fits_ilat, args.s, args.isoff,
    )?;

    // Step 7b: SkyPole writes LONPOLE / LATPOLE (and the PV-3/PV-4
    // copies on the longitude axis for non-TPN projections). Mirrors
    // C:5189. Both zenithal and non-zenithal branches are ported;
    // the non-zenithal path inverts the post-WcsMap chain (`post`)
    // on the celestial north pole and runs wcs_nat_pole to choose
    // the default LATPOLE.
    sky_pole(
        wcsmap,
        post.as_ref(),
        ilon,
        ilat,
        axlon_0,
        axlat_0,
        fits_ilon,
        fits_ilat,
        args.s,
        args.store,
    )?;

    // Step 8: build the WCS→IWC mapping for the celestial pair.
    // Mirrors C:5206-5305 — we construct the IWC→WCS chain and
    // invert it.
    let ret = build_wcs_to_iwc(
        nwcs,
        ilon,
        ilat,
        axlon_0,
        axlat_0,
        projection,
        &wcsmap_full,
        &ppcfid,
        [crval_lon_deg, crval_lat_deg],
        args.store.lonpole,
        args.store.latpole,
    )?;

    // Step 8b: auto axis-swap. The spherical rotation can transpose
    // the IWC lon/lat axes (e.g. LONPOLE = 90°). Compose pixel→WCS
    // with WCS→IWC to get pixel→IWC, then ask `world_axes` which
    // pixel axis each IWC axis aligns with. If the celestial pair is
    // swapped relative to the supplied `wperm`, swap the FITS-side
    // bookkeeping. Mirrors C:5318-5378.
    let pixel_to_iwc: DynMapping = Box::new(CmpMap::new(
        map.clone_box(),
        ret.clone_box(),
        Composition::Series,
    )?);
    if let Some(tperm) = world_axes(pixel_to_iwc.as_ref(), args.dim, args.fits_tol)?
        && tperm[ilon] == fits_ilat
        && tperm[ilat] == fits_ilon
    {
        // Swap FITS axis indices for lon and lat.
        args.wperm[ilon] = fits_ilat;
        args.wperm[ilat] = fits_ilon;

        // Swap CRVAL.
        args.store.crval.swap(fits_ilon, fits_ilat);
        // Swap CTYPE / CNAME.
        args.store.ctype.swap(fits_ilon, fits_ilat);
        args.store.cname.swap(fits_ilon, fits_ilat);
        // Swap PV vectors at the two axes (the full per-axis vectors).
        args.store.pv.swap(fits_ilon, fits_ilat);
    }

    // Step 9: simplify and return. Mirrors C:5591-5595.
    ret.simplify()
}

/// C `ZEROANG(aa) (fabs(aa) < 1.0E-9)` from fitschan.c:1392.
fn zero_angle(v: f64) -> bool {
    v.abs() < 1.0e-9
}

/// Walk the mapping for the first `WcsMap` and return a reference to
/// it. Helper duplicated from `wcs_write::find_wcsmap` so this module
/// can stay self-contained.
fn find_wcsmap(mapping: &dyn Mapping) -> Option<&WcsMap> {
    if let Some(wm) = mapping.as_any().downcast_ref::<WcsMap>() {
        return Some(wm);
    }
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if let Some(wm) = find_wcsmap(cmp.first()) {
            return Some(wm);
        }
        if let Some(wm) = find_wcsmap(cmp.second()) {
            return Some(wm);
        }
    }
    None
}

/// Split the mapping around its first usable WcsMap, returning
/// `(pre, post)` chains of `nwcs`-axis Mappings where the WcsMap
/// itself is excluded from both. Returns `None` if no WcsMap is
/// present.
///
/// Mirrors C `SplitMap2` (fitschan.c:32850-33140): the search
/// algorithm allows for an arbitrary combination of series and
/// parallel CmpMaps. When a WcsMap is found inside a parallel
/// branch, the corresponding pre/post slices are padded with
/// UnitMaps on the other branches' axes, and the WcsMap itself
/// is lifted to the full axis count with its lon/lat axes shifted
/// by the WcsMap branch's offset (mirrors C:33092-33106). The
/// lifted WcsMap is returned alongside `(pre, post)` so callers
/// reading `axlon`/`axlat` see the full-pipeline axis indices.
fn split_around_wcsmap(mapping: &dyn Mapping) -> Option<(DynMapping, DynMapping, DynMapping)> {
    let (pre, wm, post) = split_map2(mapping)?;
    Some((pre, wm, post))
}

/// Decompose `mapping` into `(pre, wcs, post)` such that a series
/// composition `pre ; wcs ; post` is equivalent to the original.
///
/// Returns `None` when there is no usable WcsMap in the chain.
///
/// Mirrors C `SplitMap2` recursively: when the input is a series
/// CmpMap, walk components in order, accumulating into `pre` until
/// a WcsMap is found, then accumulate into `post`. When the input is
/// a parallel CmpMap, recurse into each branch; the first branch
/// that yields a WcsMap is the "wcs" branch, and the WcsMap is
/// widened from the branch's axis count to the full parallel axis
/// count via the inserts handled inside `lift_wcsmap_to_full_axes`.
pub(crate) fn split_map2(mapping: &dyn Mapping) -> Option<(DynMapping, DynMapping, DynMapping)> {
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == Composition::Series {
            return split_map2_series(cmp);
        }
        return split_map2_parallel(cmp);
    }
    if let Some(wm) = mapping.as_any().downcast_ref::<WcsMap>() {
        // Only a WcsMap with FITSProj set defines the header's FITS-WCS
        // projection (fitschan.c:33186). A WcsMap acting purely as a
        // distortion correction — the polynomial TPN produced for a `-ZPX`
        // header, say — has it cleared and must be treated as an ordinary
        // component of the surrounding chain.
        if !wm.fits_proj() {
            return None;
        }
        let nax = wm.nin();
        let pre: DynMapping = Box::new(UnitMap::new(nax));
        let post: DynMapping = Box::new(UnitMap::new(nax));
        return Some((pre, Box::new(wm.clone()) as DynMapping, post));
    }
    None
}

fn split_map2_series(cmp: &CmpMap) -> Option<(DynMapping, DynMapping, DynMapping)> {
    // Walk components in series order.
    let mut comps: Vec<DynMapping> = Vec::new();
    flatten_series(cmp, &mut comps);

    let mut pre_parts: Vec<DynMapping> = Vec::new();
    let mut post_parts: Vec<DynMapping> = Vec::new();
    let mut wcs: Option<DynMapping> = None;

    for comp in comps {
        if wcs.is_none() {
            // Try to extract a WcsMap from this component.
            if let Some((pre_comp, wm_comp, post_comp)) = split_map2(comp.as_ref()) {
                pre_parts.push(pre_comp);
                wcs = Some(wm_comp);
                post_parts.push(post_comp);
            } else {
                // No WcsMap in this component — it's pre-WcsMap.
                pre_parts.push(comp);
            }
        } else {
            // Already past the WcsMap.
            post_parts.push(comp);
        }
    }
    let wm = wcs?;
    let pre = compose_series(pre_parts).ok()?;
    let post = compose_series(post_parts).ok()?;
    Some((pre, wm, post))
}

fn split_map2_parallel(cmp: &CmpMap) -> Option<(DynMapping, DynMapping, DynMapping)> {
    // Decompose into a flat list of parallel branches.
    let mut branches: Vec<DynMapping> = Vec::new();
    flatten_parallel(cmp, &mut branches);

    let mut pre_branches: Vec<DynMapping> = Vec::new();
    let mut post_branches: Vec<DynMapping> = Vec::new();
    let mut found_wcs: Option<(DynMapping, usize, usize)> = None; // (wcsmap, axis_offset, branch_axes)
    let mut axis_offset = 0_usize;

    for branch in branches {
        let branch_axes = branch.nout();
        if found_wcs.is_none() {
            if let Some((pre_comp, wm_comp, post_comp)) = split_map2(branch.as_ref()) {
                pre_branches.push(pre_comp);
                post_branches.push(post_comp);
                found_wcs = Some((wm_comp, axis_offset, branch_axes));
            } else {
                // No WcsMap in this branch — pass-through on these axes.
                pre_branches.push(branch.clone_box());
                post_branches.push(Box::new(UnitMap::new(branch_axes)));
            }
        } else {
            // Past the WcsMap branch — also pass-through.
            pre_branches.push(branch.clone_box());
            post_branches.push(Box::new(UnitMap::new(branch_axes)));
        }
        axis_offset += branch_axes;
    }

    let (sub_wcs, wcs_offset, sub_axes) = found_wcs?;
    let total_axes = axis_offset;
    // Lift the WcsMap to act on `total_axes` axes: a new WcsMap with
    // the longitude/latitude axis indices shifted by wcs_offset, and
    // the remaining axes left unchanged. Mirrors C:33092-33106.
    let lifted_wm = lift_wcsmap_to_full_axes(sub_wcs.as_ref(), wcs_offset, total_axes, sub_axes)?;

    // Compose the per-branch pre/post lists in parallel.
    let pre = compose_parallel(pre_branches).ok()?;
    let post = compose_parallel(post_branches).ok()?;

    Some((pre, lifted_wm, post))
}

fn flatten_series(cmp: &CmpMap, out: &mut Vec<DynMapping>) {
    debug_assert_eq!(cmp.composition(), Composition::Series);
    push_or_recurse(extract_child_effective(cmp, true), Composition::Series, out);
    push_or_recurse(
        extract_child_effective(cmp, false),
        Composition::Series,
        out,
    );
}

fn flatten_parallel(cmp: &CmpMap, out: &mut Vec<DynMapping>) {
    debug_assert_eq!(cmp.composition(), Composition::Parallel);
    push_or_recurse(
        extract_child_effective(cmp, true),
        Composition::Parallel,
        out,
    );
    push_or_recurse(
        extract_child_effective(cmp, false),
        Composition::Parallel,
        out,
    );
}

/// Return a clone of the requested child (`first=true` for `cmp.first()`)
/// with its `inverted` flag normalised to the value that was captured
/// when the CmpMap was constructed. The CmpMap's runtime transform
/// composes the children using the captured `inv_a`/`inv_b` snapshots
/// (so post-construction `child.invert()` doesn't change the
/// composition); flatten_series / flatten_parallel must do the same to
/// avoid dimensional mismatches when the child has since been
/// inverted independently.
fn extract_child_effective(cmp: &CmpMap, first: bool) -> DynMapping {
    let (child, captured_inv) = if first {
        (cmp.first(), cmp.inv_a())
    } else {
        (cmp.second(), cmp.inv_b())
    };
    let mut clone = child.clone_box();
    if clone.is_inverted() != captured_inv {
        clone.invert();
    }
    clone
}

fn push_or_recurse(m: DynMapping, kind: Composition, out: &mut Vec<DynMapping>) {
    if let Some(cmp) = m.as_any().downcast_ref::<CmpMap>()
        && cmp.composition() == kind
    {
        if kind == Composition::Series {
            flatten_series(cmp, out);
        } else {
            flatten_parallel(cmp, out);
        }
        return;
    }
    out.push(m);
}

fn lift_wcsmap_to_full_axes(
    wm_dyn: &dyn Mapping,
    wcs_offset: usize,
    total_axes: usize,
    sub_axes: usize,
) -> Option<DynMapping> {
    let wm = wm_dyn.as_any().downcast_ref::<WcsMap>()?;
    if wcs_offset == 0 && sub_axes == total_axes {
        return Some(Box::new(wm.clone()));
    }
    // Build a new WcsMap of total_axes axes, with the lon/lat axes
    // shifted by wcs_offset (1-based in the C code; WcsMap::new takes
    // 1-based too). Carry the projection type and PV parameters across.
    let proj = wm.projection();
    let lon_1b = wm.lonax() + wcs_offset; // shift 1-based axis
    let lat_1b = wm.latax() + wcs_offset;
    let mut new_wm = WcsMap::new(total_axes, proj, lon_1b, lat_1b).ok()?;
    let inv = wm.is_inverted();
    // Copy PV parameters: in the source WcsMap, PVs live at
    // (axlon_1b, m) and (axlat_1b, m); after lift, they belong at
    // (lon_1b, m) and (lat_1b, m). Mirrors C:33094-33102 which copies
    // by `i + wcsaxis` (where `wcsaxis` is the parallel axis offset).
    let src_lon = wm.lonax();
    let src_lat = wm.latax();
    for m in 0..100 {
        if let Some(v) = wm.stored_pv(src_lon, m)
            && v.is_finite()
        {
            let _ = new_wm.set_pv(lon_1b, m, v);
        }
        if let Some(v) = wm.stored_pv(src_lat, m)
            && v.is_finite()
        {
            let _ = new_wm.set_pv(lat_1b, m, v);
        }
    }
    if inv {
        new_wm.invert();
    }
    let _ = sub_axes;
    Some(Box::new(new_wm))
}

fn compose_parallel(parts: Vec<DynMapping>) -> AstResult<DynMapping> {
    if parts.is_empty() {
        return Err(crate::error::AstError::new(
            crate::error::ErrorCode::Nodef,
            "compose_parallel requires at least one component",
        ));
    }
    let mut iter = parts.into_iter();
    let mut acc = iter.next().unwrap();
    for next in iter {
        acc = Box::new(CmpMap::new(acc, next, Composition::Parallel)?);
    }
    Ok(acc)
}

fn compose_series(mut parts: Vec<DynMapping>) -> AstResult<DynMapping> {
    let mut acc = parts.remove(0);
    for next in parts {
        acc = Box::new(CmpMap::new(acc, next, Composition::Series)?);
    }
    Ok(acc)
}

/// Compute the projection-plane-coordinate fiducial offset (radians).
/// Mirrors `fitschan.c::GetFiducialPPC` — invert the WcsMap on the
/// native fiducial.
fn compute_fiducial_ppc(wcsmap: &WcsMap) -> AstResult<Option<(f64, f64)>> {
    let (phi, theta) = get_fiducial_nsc(wcsmap);
    if !phi.is_finite() || !theta.is_finite() {
        return Ok(None);
    }
    use crate::coords::PointSet;
    let naxin = wcsmap.nin();
    if naxin == 0 {
        return Ok(None);
    }
    let mut input = vec![0.0_f64; naxin];
    let axlon = wcsmap.lonax_0based();
    let axlat = wcsmap.latax_0based();
    if axlon >= naxin || axlat >= naxin {
        return Ok(None);
    }
    input[axlon] = phi;
    input[axlat] = theta;
    let pts = PointSet::from_rows([input]);
    // Inverse direction: native → PPC.
    let out = wcsmap.transform(&pts, false)?;
    let row = out.point(0);
    if axlon >= row.len() || axlat >= row.len() {
        return Ok(None);
    }
    let (x, y) = (row[axlon], row[axlat]);
    if !x.is_finite() || !y.is_finite() {
        return Ok(None);
    }
    Ok(Some((x, y)))
}

/// Build the WCS→IWC mapping for the celestial pair. We construct
/// IWC→WCS first (matching the C source), then invert.
///
/// The returned mapping has `nwcs` inputs and `nwcs` outputs:
/// celestial axes are projected through `WcsMap_inverted ∘
/// WcsNative-rotation`, non-celestial axes pass through unchanged.
fn build_wcs_to_iwc(
    nwcs: usize,
    ilon: usize,
    ilat: usize,
    axlon: usize,
    axlat: usize,
    projection: WcsProjection,
    src_wcsmap: &WcsMap,
    ppcfid: &[f64],
    crval_deg: [f64; 2],
    store_lonpole: Option<f64>,
    store_latpole: Option<f64>,
) -> AstResult<DynMapping> {
    let _ = projection; // kept for callers; src_wcsmap.projection() is the source of truth.

    // ShiftMap from IWC → PPC: subtract ppcfid (so when PPC == IWC,
    // i.e. ppcfid is zero, this is identity). Mirrors C:5210.
    let shift_iwc_to_ppc = ShiftMap::new(ppcfid.to_vec());

    // MatrixMap that scales PPC degrees to radians on the celestial
    // axes only. Mirrors C:5212-5219 (diagonal MatrixMap with form=1
    // and entries 1.0 except DD2R on lon/lat). We construct a full
    // diagonal Array2 to use the existing MatrixMap::new signature.
    let mut diag = ndarray::Array2::<f64>::zeros((nwcs, nwcs));
    for i in 0..nwcs {
        diag[[i, i]] = 1.0;
    }
    diag[[ilon, ilon]] = DD2R;
    diag[[ilat, ilat]] = DD2R;
    let scale = MatrixMap::new(diag)?;

    // Native-spherical → WCS rotation. Build via the WcsMap-aware
    // path so non-zenithal projections (SFL, CAR, MER, ...) get the
    // correct celestial-pole solution from `WcsNatPole`. Mirrors C's
    // WcsNative which calls WcsNatPole (fitschan.c:38175-38231) and
    // then assembles the three rotations.
    //
    // Use the *source* WcsMap (with its PV parameters intact) so that
    // the simplifier can collapse the PPC→native→celestial→native→PPC
    // round-trip exactly when this chain is composed against the
    // pixel→WCS mapping. C does the same via `astClone(map2)` at
    // fitschan.c:5204.
    // C celestial_axes (fitschan.c:5189-5226) calls SkyPole to populate
    // store->lonpole / store->latpole, then makes a copy of the WcsMap
    // with PV3/PV4 cleared, then passes that copy to WcsNative. With
    // PV3/PV4 cleared, WcsNative (fitschan.c:38205-38216) falls through
    // to read LONPOLE/LATPOLE from store->lonpole / store->latpole.
    //
    // The Rust `build_native_to_celestial_rotation_for_wcsmap` accepts
    // lonpole/latpole as parameters — pass the FitsStore values
    // (degrees → radians) so the resulting native→celestial rotation
    // uses the same LONPOLE/LATPOLE as the read-side pipeline. Without
    // this the (pixel→WCS) ∘ (WCS→IWC) composition leaves a residual
    // rotation in IWC and `world_axes` mis-permutes celestial axes
    // (e.g. longslit's CTYPE2/3 swap).
    let lonpole_rad = store_lonpole.map(|v| v * DD2R);
    let latpole_rad = store_latpole.map(|v| v * DD2R);
    let wcs_native_rot =
        crate::fitschan::wcs_celestial::build_native_to_celestial_rotation_for_wcsmap(
            src_wcsmap,
            crval_deg[0] * DD2R,
            crval_deg[1] * DD2R,
            lonpole_rad,
            latpole_rad,
        )?;

    // Celestial chain (PPC → WCS):
    //   map2b (full N-axis WcsMap, inverted: PPC → native spherical)
    //   ∘ WcsNative (native spherical → WCS).
    //
    // Mirrors C `CelestialAxes` (fitschan.c:5199-5226). C uses the
    // *full* N-axis lifted WcsMap (`map2b`, a copy of `map2` with PV3/4
    // cleared for non-TPN) here, NOT a freshly-built 2-axis WcsMap.
    // This is load-bearing: the resulting WCS→IWC chain contains the
    // identical N-axis WcsMap that appears (inverted) in the pixel→WCS
    // Mapping, so when the two are composed the simplifier collapses
    // the WcsMap deproject/reproject round-trip to a UnitMap. A 2-axis
    // WcsMap, having a different `ncoord`, cannot merge with the N-axis
    // one and leaves a residual projection round-trip whose ~1e-15 FP
    // noise breaks the degenerate-axis FitLine linearity test in
    // MakeIntWorld (see degen1 wcsconv fixture).
    // `map2b` is a clone of the full N-axis WcsMap, inverted so its
    // forward direction is PPC → native spherical. C also clears the
    // longitude axis's PVi_3 / PVi_4 (LONPOLE / LATPOLE encoded as
    // projection parameters) for non-TPN projections so that WcsNative
    // reads those pole angles from the FitsStore instead
    // (fitschan.c:5199-5203). We do not clear them here: the
    // native→celestial rotation built above already receives the
    // store's LONPOLE / LATPOLE as explicit arguments, so the pole is
    // already correct; and keeping `map2b`'s PVs byte-identical to the
    // pixel→WCS WcsMap is exactly what lets the simplifier cancel the
    // deproject/reproject round-trip (clearing them would make the two
    // WcsMaps unequal and block the merge).
    //
    // `map2b` is used in the same orientation as C's `astCopy(map2)`
    // (fitschan.c:5199-5230): C composes `astCmpMap(map2b, WcsNative)`
    // without inverting map2b. The whole IWC→WCS chain is inverted at
    // the end to yield WCS→IWC, which flips map2b into the opposite
    // direction from the (inverted) pixel→WCS WcsMap, so the two cancel.
    let map2b = src_wcsmap.clone();

    // Embed the 2-axis native→celestial rotation into the N-axis frame.
    // Mirrors C `WcsNative` (fitschan.c:38334-38377): add a UnitMap in
    // parallel for the (naxis-2) non-celestial axes, then sandwich
    // between a PermMap and its inverse so the celestial pair sits at
    // (axlon, axlat). The rotation built above always has lon/lat at
    // its leading axes 0/1.
    let wcs_native: DynMapping = embed_native_rotation(wcs_native_rot, nwcs, axlon, axlat)?;

    // tmap3 = map2b ∘ WcsNative (PPC → WCS). Mirrors fitschan.c:5230.
    let cel_nd: DynMapping = Box::new(CmpMap::new(
        Box::new(map2b),
        wcs_native,
        Composition::Series,
    )?);

    // The pixel→WCS Mapping may include a PermMap that places the
    // celestial pair at WCS-Frame axes (ilon, ilat) which differ from
    // the WcsMap's own longitude/latitude axes (axlon, axlat) — this
    // happens whenever the world Frame's component/axis order has been
    // permuted relative to the WcsMap (e.g. FITS-CLASS spectral-cube
    // output, where AddEncodingFrame reorders the Frame to spec-first).
    // `cel_nd` above operates on the WcsMap's native (axlon, axlat),
    // but the surrounding `shift`/`scale` operate on (ilon, ilat).
    // When they differ, sandwich `cel_nd` between a PermMap (mapping
    // ilon→axlon, ilat→axlat) and its inverse so its celestial input /
    // output axes line up with (ilon, ilat). Mirrors fitschan.c:5246-5296
    // (`if( nwcsmap != nwcs || ilon != axlon || ilat != axlat )`).
    let cel_nd: DynMapping = if ilon != axlon || ilat != axlat {
        // inperm[i] = WcsMap-side axis fed from WCS-Frame axis i
        // (C inperm, fitschan.c:5257-5269; 0-based, PermMap entries
        // are 1-based so we add 1 below).
        let mut inperm = vec![-1_i64; nwcs];
        inperm[ilon] = axlon as i64;
        inperm[ilat] = axlat as i64;
        let mut iax = 0_usize;
        for slot in inperm.iter_mut() {
            if *slot == -1 {
                while iax == axlon || iax == axlat {
                    iax += 1;
                }
                *slot = iax as i64;
                iax += 1;
            }
        }
        // outperm[j] = WCS-Frame axis fed from WcsMap-side output j
        // (C outperm, fitschan.c:5272-5279).
        let mut outperm = vec![-1_i64; nwcs];
        outperm[axlon] = ilon as i64;
        outperm[axlat] = ilat as i64;
        let mut jax = 0_usize;
        for slot in outperm.iter_mut() {
            if *slot == -1 {
                while jax == ilon || jax == ilat {
                    jax += 1;
                }
                *slot = jax as i64;
                jax += 1;
            }
        }
        let inperm_1b: Vec<i64> = inperm.iter().map(|&v| v + 1).collect();
        let outperm_1b: Vec<i64> = outperm.iter().map(|&v| v + 1).collect();

        // tmap2 = PermMap(ilon→axlon, …). Sandwich:
        //   tmap2 ∘ cel_nd ∘ tmap2⁻¹  (series). Mirrors C:5283-5291.
        let perm_fwd = PermMap::new(&inperm_1b, &outperm_1b, &[])?;
        let mut perm_inv = PermMap::new(&inperm_1b, &outperm_1b, &[])?;
        perm_inv.invert();
        let pre = CmpMap::new(Box::new(perm_fwd), cel_nd, Composition::Series)?;
        Box::new(CmpMap::new(
            Box::new(pre),
            Box::new(perm_inv),
            Composition::Series,
        )?)
    } else {
        cel_nd
    };

    // Forward chain so far: IWC → PPC (shift) → degree-scaled (matrix)
    // → cel_nd (PPC → WCS). Compose in series to get IWC → WCS.
    let iwc_to_wcs = compose_series(vec![Box::new(shift_iwc_to_ppc), Box::new(scale), cel_nd])?;

    // Invert to get WCS → IWC. Mirrors C:5307.
    let mut wcs_to_iwc = iwc_to_wcs;
    wcs_to_iwc.invert();
    Ok(wcs_to_iwc)
}

/// Embed a 2-axis native→celestial rotation (lon at axis 0, lat at
/// axis 1) into an `naxis`-wide Mapping whose celestial pair sits at
/// `(axlon, axlat)` (0-based) and which passes the remaining axes
/// through unchanged. Mirrors the axis-embedding tail of C `WcsNative`
/// (fitschan.c:38334-38377): a UnitMap in parallel for the `naxis-2`
/// non-celestial axes (stage 5), then a PermMap / inverse-PermMap
/// sandwich (stages 1 and 3) so the rotation's leading lon/lat axes
/// land at `(axlon, axlat)`.
fn embed_native_rotation(
    rot_2d: DynMapping,
    naxis: usize,
    axlon: usize,
    axlat: usize,
) -> AstResult<DynMapping> {
    // Stage 5: add a UnitMap in parallel for the non-celestial axes.
    let nax_rem = naxis - 2;
    let embedded: DynMapping = if nax_rem > 0 {
        Box::new(CmpMap::new(
            rot_2d,
            Box::new(UnitMap::new(nax_rem)),
            Composition::Parallel,
        )?)
    } else {
        rot_2d
    };

    // If lon/lat already lead, no permutation is needed.
    if axlon == 0 && axlat == 1 {
        return Ok(embedded);
    }

    // Stages 1/3: build the PermMap that moves axis 0 → axlon, axis 1 →
    // axlat. Mirrors C's outperm/inperm construction (0-based there;
    // Rust PermMap uses 1-based entries so each value is incremented).
    //
    // `outperm[i]` = the input axis index feeding output axis i.
    let mut outperm: Vec<usize> = (0..naxis).collect();
    outperm.swap(axlon, 0);
    let new_axlat = if axlat == 0 { axlon } else { axlat };
    outperm.swap(new_axlat, 1);
    let mut inperm = vec![0_usize; naxis];
    for i in 0..naxis {
        inperm[outperm[i]] = i;
    }

    let outperm_1b: Vec<i64> = outperm.iter().map(|&v| (v + 1) as i64).collect();
    let inperm_1b: Vec<i64> = inperm.iter().map(|&v| (v + 1) as i64).collect();

    let perm = PermMap::new(&inperm_1b, &outperm_1b, &[])?;
    let mut perm_inv = PermMap::new(&inperm_1b, &outperm_1b, &[])?;
    perm_inv.invert();

    let chain = CmpMap::new(Box::new(perm), embedded, Composition::Series)?;
    Ok(Box::new(CmpMap::new(
        Box::new(chain),
        Box::new(perm_inv),
        Composition::Series,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::fset_to_store::FITS_TOL_DEFAULT;
    use crate::frames::sky_convert::SkySystem;
    use crate::frames::{BasicFrameBuilder, FrameRef, FrameSet};

    /// 2-axis FrameSet: pixel base + WcsMap-driven SkyFrame current.
    fn build_tan_frameset() -> FrameSet {
        let pixel = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
        let sky = SkyFrame::new(SkySystem::Fk5);
        let wcsmap = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        // pixel→sky: WcsMap inverted (PPC → native) then native rotation
        // hidden inside SkyFrame's intrinsic system. For test purposes we
        // attach just the WcsMap inverted; the inverse direction is what
        // the writer will exercise. Caller's `mapping(Base, Current)`
        // returns this forward mapping unchanged when current is the
        // SkyFrame.
        let mut wm = wcsmap;
        wm.invert();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(wm), Box::new(sky))
            .unwrap();
        fs
    }

    #[test]
    fn no_celestial_axes_returns_unitmap() {
        // Pure BasicFrame current → no SkyFrame → UnitMap return.
        let pixel = BasicFrameBuilder::new(2).build().unwrap();
        let other = BasicFrameBuilder::new(2).build().unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(other))
            .unwrap();
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut axis_done = vec![false; 2];

        let result = celestial_axes(CelestialAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            axis_done: &mut axis_done,
            isoff: 0,
            fits_tol: FITS_TOL_DEFAULT,
        })
        .unwrap();

        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);
        assert!(result.as_any().is::<UnitMap>());
        assert!(!axis_done[0] && !axis_done[1]);
        assert!(store.crval[0].is_none() && store.crval[1].is_none());
    }

    /// 2-axis FrameSet whose pixel→sky mapping pre-swaps axes. The
    /// celestial-axis code should detect this via `world_axes` and
    /// swap the FITS-side bookkeeping.
    fn build_swapped_tan_frameset() -> FrameSet {
        let pixel = BasicFrameBuilder::new(2).domain("GRID").build().unwrap();
        let sky = SkyFrame::new(SkySystem::Fk5);
        // PermMap that swaps axes 1↔2 (1-based).
        let swap = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
        let mut wcsmap = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
        wcsmap.invert();
        // pixel→sky: swap then WcsMap-inverted.
        let chain = CmpMap::new(Box::new(swap), Box::new(wcsmap), Composition::Series).unwrap();
        let mut fs = FrameSet::new(Box::new(pixel));
        fs.add_frame(FrameRef::Base, Box::new(chain), Box::new(sky))
            .unwrap();
        fs
    }

    #[test]
    fn auto_axis_swap_fires_when_pixel_to_iwc_transposes() {
        let fs = build_swapped_tan_frameset();
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut axis_done = vec![false; 2];

        let _ = celestial_axes(CelestialAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            axis_done: &mut axis_done,
            isoff: 0,
            fits_tol: FITS_TOL_DEFAULT,
        })
        .unwrap();

        // Swap should have fired: wperm reversed.
        assert_eq!(wperm, vec![1_usize, 0]);
        // CTYPE values appear at the swapped FITS positions.
        // Pre-swap: ctype[0]="RA---TAN", ctype[1]="DEC--TAN".
        // Post-swap: ctype[0]="DEC--TAN", ctype[1]="RA---TAN".
        assert_eq!(store.ctype[0].as_deref(), Some("DEC--TAN"));
        assert_eq!(store.ctype[1].as_deref(), Some("RA---TAN"));
    }

    #[test]
    fn tan_2axis_writes_celestial_keywords() {
        let fs = build_tan_frameset();
        let dim = vec![Some(100.0), Some(100.0)];
        let mut wperm = vec![0_usize, 1];
        let mut store = FitsStore::new(2);
        let mut axis_done = vec![false; 2];

        let result = celestial_axes(CelestialAxesArgs {
            fs: &fs,
            dim: &dim,
            wperm: &mut wperm,
            s: ' ',
            store: &mut store,
            axis_done: &mut axis_done,
            isoff: 0,
            fits_tol: FITS_TOL_DEFAULT,
        })
        .unwrap();

        // Both celestial axes claimed.
        assert!(axis_done[0] && axis_done[1]);
        // CTYPE for FK5 + TAN.
        assert_eq!(store.ctype[0].as_deref(), Some("RA---TAN"));
        assert_eq!(store.ctype[1].as_deref(), Some("DEC--TAN"));
        // RADESYS / EQUINOX set.
        assert_eq!(store.radesys.as_deref(), Some("FK5"));
        assert!(store.equinox.is_some());
        // CRVAL set (in degrees). The exact value depends on the
        // WcsMap's default fiducial; for a default TAN the fiducial
        // is at (0, 90)°. Rust-side build_native_to_celestial_rotation
        // is consistent with this convention.
        assert!(store.crval[0].is_some());
        assert!(store.crval[1].is_some());

        // Mapping is 2-in / 2-out; specific values would need a C
        // oracle for bit-exact comparison. We at least verify the
        // shape.
        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);
    }
}

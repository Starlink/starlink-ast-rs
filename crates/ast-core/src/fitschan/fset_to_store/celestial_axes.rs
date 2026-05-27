//! Port of `fitschan.c::CelestialAxes` (~4890).
//!
//! Identifies the celestial pair of axes in the FrameSet's current
//! frame, stores celestial-axis FITS keywords (CRVAL / CTYPE / PV /
//! RADESYS / EQUINOX / MJD-OBS) in the supplied [`FitsStore`], and
//! returns the WCS→IWC mapping segment for those axes.
//!
//! ## Deferred branches (mirrors plan §5.3 + 5.5)
//!
//! These C paths are intentionally not ported here — the legacy
//! `wcs_write::populate_celestial` doesn't cover them either, and
//! the existing 152-passing-wcsconv-tests baseline doesn't exercise
//! them. They land as Group 8 follow-ups if a fixture exposes a gap:
//!
//! - **`-TAB` algorithm** (no WcsMap in chain): C falls through to
//!   `IsMapTab2D`. We return identity over the celestial axes so the
//!   downstream pipeline can flag the linearity failure.
//! - **`SkyPole` / LONPOLE+LATPOLE storage**: the C call at line 5189
//!   stores the chosen LONPOLE / LATPOLE. Our writer uses the
//!   `WcsNative`-equivalent rotation built into the WCS→IWC mapping
//!   directly, so the keywords aren't yet needed in the Store.
//! - **Auto axis-swap** (C lines 5318-5378): when the IWC rotation
//!   transposes lon/lat axes, swap `wperm` / CRVAL / CTYPE / CNAME /
//!   PV between the two FITS slots. Triggered only when
//!   `FitsAxisOrder == "<auto>"` — we always behave as the
//!   non-auto path.

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
use super::sky_sys::sky_sys;

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
        let (pframe, paxis) = primary_frame(wcsfrm, iax);
        if pframe.as_any().is::<SkyFrame>() {
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
    let (skyframe_holder, _) = primary_frame(wcsfrm, ilon);
    let skyfrm = match skyframe_holder.as_any().downcast_ref::<SkyFrame>() {
        Some(s) => s,
        None => return Ok(Box::new(UnitMap::new(nwcs))),
    };

    // Step 2: pixel→wcs mapping, simplified. Mirrors C:5092-5094.
    let map_unsimp = args.fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let map = map_unsimp.simplified()?;

    // Step 3: split the chain around its first WcsMap. Mirrors
    // C:5109-5111 (`SplitMap`). For the WcsMap-less case we fall
    // through to a deferred path (TAB or non-celestial fallback) and
    // return identity over the celestial axes.
    let (_pre, post) = match split_around_wcsmap(map.as_ref()) {
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

    // Locate the WcsMap object inside `map` so we can read PV /
    // axes. Mirrors `astGetWcsAxis(map2, …)` and `astTestPV` /
    // `astGetPV` calls at C:5127, 5150-5163.
    let wcsmap = match find_wcsmap(map.as_ref()) {
        Some(w) => w,
        None => return Ok(Box::new(UnitMap::new(nwcs))),
    };
    let axlon_0 = wcsmap.lonax_0based();
    let axlat_0 = wcsmap.latax_0based();
    let axlon_1 = wcsmap.lonax();
    let axlat_1 = wcsmap.latax();
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
        match get_fiducial_wcs(wcsmap, post.as_ref(), ilon, ilat)? {
            Some((a, b)) => (a, b),
            None => return Ok(Box::new(UnitMap::new(nwcs))),
        };

    // Normalise: replace BAD with 0, snap near-zero angles, then
    // call SkyFrame::norm to wrap each axis into its conventional
    // range. C:5134-5138.
    let mut skyfid = [skyfid_lon_rad, skyfid_lat_rad];
    let lonax_in_sky = skyfrm.lon_axis();
    let latax_in_sky = skyfrm.lat_axis();
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
    for m in 0..WCSLIB_MXPAR {
        if let Some(pv) = wcsmap.get_pv(axlon_1, m)
            && pv.is_finite()
            && (m < 3 || m > 4 || tpn)
            && fits_ilon < args.store.pv.len()
            && m < args.store.pv[fits_ilon].len()
        {
            args.store.pv[fits_ilon][m] = Some(pv);
        }
        if let Some(pv) = wcsmap.get_pv(axlat_1, m)
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
    if !tpn && wcsmap.get_pv(axlon_1, 0).unwrap_or(0.0) != 0.0 {
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
        &ppcfid,
        [crval_lon_deg, crval_lat_deg],
    )?;

    // Step 9: simplify and return. Mirrors C:5591-5595.
    ret.simplified()
}

/// Recursively descend `frame` to its primary leaf frame for axis
/// `axis` (0-based). Mirrors `astPrimaryFrame`.
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

fn zero_angle(v: f64) -> bool {
    v.abs() < 1.0e-12
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

/// Split the mapping at the first WcsMap, returning `(pre, post)`
/// chains where the WcsMap itself is excluded from both. Returns
/// `None` if no WcsMap is present. Empty pre/post sides are filled
/// with a UnitMap of the right size — mirrors C's `SplitMap` which
/// returns map1 / map3 even when one side is trivial.
fn split_around_wcsmap(mapping: &dyn Mapping) -> Option<(DynMapping, DynMapping)> {
    let mut pre: Vec<DynMapping> = Vec::new();
    let mut post: Vec<DynMapping> = Vec::new();
    let mut found = false;
    walk_split_wcsmap(mapping, &mut pre, &mut post, &mut found);
    if !found {
        return None;
    }
    // Find the WcsMap so we know the axis-count for any identity
    // padding we need on either side.
    let wm = find_wcsmap(mapping)?;
    let nax = wm.nin();

    let pre_chain = if pre.is_empty() {
        Box::new(UnitMap::new(nax)) as DynMapping
    } else {
        compose_series(pre).ok()?
    };
    let post_chain = if post.is_empty() {
        Box::new(UnitMap::new(nax)) as DynMapping
    } else {
        compose_series(post).ok()?
    };
    Some((pre_chain, post_chain))
}

fn walk_split_wcsmap(
    mapping: &dyn Mapping,
    pre: &mut Vec<DynMapping>,
    post: &mut Vec<DynMapping>,
    found: &mut bool,
) {
    if mapping.as_any().downcast_ref::<WcsMap>().is_some() {
        *found = true;
        return;
    }
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>()
        && cmp.composition() == Composition::Series
    {
        walk_split_wcsmap(cmp.first(), pre, post, found);
        walk_split_wcsmap(cmp.second(), pre, post, found);
        return;
    }
    if *found {
        post.push(mapping.clone_box());
    } else {
        pre.push(mapping.clone_box());
    }
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
    ppcfid: &[f64],
    crval_deg: [f64; 2],
) -> AstResult<DynMapping> {
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

    // Native-spherical → WCS rotation. We use the existing
    // wcs_write::wcs_celestial helper since the rotation maths is
    // identical to what C's WcsNative produces. The helper expects
    // CRVAL in degrees.
    let lonpole_rad = PI;
    let wcs_native_rot = crate::fitschan::wcs_celestial::build_native_to_celestial_rotation(
        crval_deg[0],
        crval_deg[1],
        lonpole_rad,
    )
    .ok_or_else(|| {
        crate::error::AstError::new(
            crate::error::ErrorCode::Nodef,
            "celestial_axes: failed to build native→celestial rotation",
        )
    })?;

    // 2D celestial chain (PPC → WCS):
    //   WcsMap_inverted (PPC → native spherical) → wcs_native_rot
    let mut wcsmap_inv = WcsMap::new(2, projection, 1, 2)?;
    wcsmap_inv.invert();
    let cel_2d: DynMapping = Box::new(CmpMap::new(
        Box::new(wcsmap_inv),
        wcs_native_rot,
        Composition::Series,
    )?);

    // Embed cel_2d into nwcs with a UnitMap on non-celestial axes,
    // then bracket with PermMaps so the celestial pair sits at the
    // expected (axlon, axlat) positions of the rotation, regardless
    // of where (ilon, ilat) sit in the nwcs frame.
    let cel_nd: DynMapping = if nwcs == 2 && ilon == axlon && ilat == axlat {
        cel_2d
    } else {
        // Build PermMap that maps original frame layout → leading
        // (lon, lat, others...).
        let mut outperm: Vec<i64> = Vec::with_capacity(nwcs);
        outperm.push((ilon + 1) as i64);
        outperm.push((ilat + 1) as i64);
        for ax in 0..nwcs {
            if ax != ilon && ax != ilat {
                outperm.push((ax + 1) as i64);
            }
        }
        let mut inperm: Vec<i64> = vec![0; nwcs];
        for (out_i, &in_i) in outperm.iter().enumerate() {
            inperm[(in_i as usize) - 1] = (out_i + 1) as i64;
        }
        let perm_in = PermMap::new(&inperm, &outperm, &[])?;
        let mut perm_out = PermMap::new(&inperm, &outperm, &[])?;
        perm_out.invert();

        let extra = nwcs - 2;
        let parallel: DynMapping = if extra == 0 {
            cel_2d
        } else {
            Box::new(CmpMap::new(
                cel_2d,
                Box::new(UnitMap::new(extra)),
                Composition::Parallel,
            )?)
        };

        let chain1 = CmpMap::new(Box::new(perm_in), parallel, Composition::Series)?;
        Box::new(CmpMap::new(
            Box::new(chain1),
            Box::new(perm_out),
            Composition::Series,
        )?)
    };

    // Forward chain so far: IWC → PPC (shift) → degree-scaled (matrix)
    // → cel_nd (PPC → WCS). Compose in series to get IWC → WCS.
    let iwc_to_wcs = compose_series(vec![Box::new(shift_iwc_to_ppc), Box::new(scale), cel_nd])?;

    // Invert to get WCS → IWC. Mirrors C:5307.
    let mut wcs_to_iwc = iwc_to_wcs;
    wcs_to_iwc.invert();
    Ok(wcs_to_iwc)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        })
        .unwrap();

        assert_eq!(result.nin(), 2);
        assert_eq!(result.nout(), 2);
        assert!(result.as_any().is::<UnitMap>());
        assert!(!axis_done[0] && !axis_done[1]);
        assert!(store.crval[0].is_none() && store.crval[1].is_none());
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

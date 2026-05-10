//! FITS-WCS encoder pipeline.
//!
//! Converts an AST FrameSet into FITS header cards by:
//! 1. Decomposing the FrameSet into a FitsStore (axis identification + linear extraction)
//! 2. Writing the FitsStore contents as FITS keyword cards
//!
//! This mirrors the C library's `FsetToStore()` → `WcsFromStore()` pipeline.

use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::wcs_celestial::build_native_to_celestial_rotation;
use super::wcs_decompose::{decompose_linear, split_cd};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{
    BasicFrame, CmpFrame, FrameRef, FrameSet, SkyFrame, SkySystem, SpecFrame, StdOfRest,
};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, WcsMap, WcsProjection};
use crate::pal;
use crate::serial::AnyObject;

/// Encode a FrameSet as FITS-WCS keywords into a CardList.
///
/// This is the top-level encoder entry point.
pub(crate) fn encode_wcs(obj: &AnyObject, cards: &mut CardList, use_cd: bool) -> AstResult<()> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "FitsWcs encoding requires a FrameSet object",
            ));
        }
    };

    let store = store_from_frameset(frameset)?;
    write_store_to_cards(&store, cards, use_cd)
}

/// Encode a FrameSet via FitsStore with a custom card writer.
///
/// Used by alternative encodings that share the FrameSet→FitsStore
/// decomposition but write different keywords.
pub(crate) fn encode_wcs_to_store(
    obj: &AnyObject,
    cards: &mut CardList,
    writer: impl FnOnce(&FitsStore, &mut CardList) -> AstResult<()>,
) -> AstResult<()> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "FitsWcs encoding requires a FrameSet object",
            ));
        }
    };

    let store = store_from_frameset(frameset)?;
    writer(&store, cards)
}

/// Decompose a FrameSet into a FitsStore.
pub(crate) fn store_from_frameset(fs: &FrameSet) -> AstResult<FitsStore> {
    let naxis = fs.base().naxes();
    let mut store = FitsStore::new(naxis);

    // Get the base→current mapping (pixel→world) and simplify. The C
    // library's CelestialAxes (fitschan.c ~line 5093) calls astSimplify
    // before doing anything else, which collapses redundant
    // forward/inverse pairs and merges adjacent linear maps. Doing the
    // same here keeps subsequent numerical Jacobians from accumulating
    // floating-point noise across cancellable mapping steps.
    let mapping = fs
        .mapping(FrameRef::Base, FrameRef::Current)?
        .simplify()?;

    // First-pass extraction via the pre-WcsMap chain. This gives a CRPIX
    // estimate and lets us evaluate CRVAL on the full pixel→world
    // mapping. For non-celestial frames (no WcsMap in the chain) this is
    // the final answer; for celestial frames it's a stepping stone to
    // the full pixel→IWC composition below.
    let pre_mapping = split_pre_wcsmap(mapping.as_ref()).unwrap_or_else(|| mapping.clone());

    let g0: Vec<f64> = vec![1.0; naxis];
    let pre_decomp = decompose_linear(pre_mapping.as_ref(), naxis, &g0)?;
    let mut crpix = solve_crpix_from_decomp(&pre_decomp.cd, &pre_decomp.crval, &g0)
        .unwrap_or_else(|| g0.clone());
    let mut full_decomp = decompose_linear(mapping.as_ref(), naxis, &crpix)?;

    let projection = extract_projection_from_mapping(mapping.as_ref());

    // For celestial frames, follow the C library's recipe (fitschan.c
    // CelestialAxes ~5206 + MakeIntWorld): build the full WCS→IWC
    // mapping and compose with pixel→WCS to obtain pixel→IWC.
    // Decomposing that gives the FITS-correct CD matrix — including any
    // small rotation contributed by the post-WcsMap chain (e.g. timj's
    // FK4→FK5 SlaMap, which the pre-WcsMap path misses entirely).
    //
    // The C library reuses the FrameSet's own post-WcsMap chain rather
    // than reconstructing WcsNative from CRVAL — that's how it ends up
    // with bit-exact cancellation through astSimplify. We do the same:
    // split the mapping into (pre, post) around the WcsMap and build
    // WCS→IWC as the inverse of `(WcsMap_inverted ∘ post_chain)`.
    // Because the same `post_chain` object appears on both sides of
    // the composition, simplify cancels them exactly, leaving only the
    // linear pre-WcsMap chain (plus any non-cancellable residue, like
    // timj's SlaMap, which is the genuine extra rotation FITS needs to
    // capture in CD).
    //
    // CRVAL for the celestial axes comes from the same post chain at
    // the WcsMap's native fiducial point (mirrors GetFiducialWCS at
    // fitschan.c ~12823). This gives a CRVAL bit-exact to what the
    // FrameSet was built around, rather than re-evaluating through
    // numerical Jacobian on the full pixel→world chain.
    //
    // Linear-only / non-celestial frames keep the pre-WcsMap path.
    let mut cel_crval: Option<Vec<f64>> = None;
    let cd_decomp = if let Some((sky, lon_axis, lat_axis)) = celestial_axes(fs) {
        let parts = projection.and_then(|p| {
            split_around_wcsmap(mapping.as_ref()).map(|(_, post)| (p, post))
        });
        match parts {
            Some((proj, post)) => {
                let fiducial = fiducial_crval(post.as_ref(), proj, &full_decomp.crval, naxis);
                let crval_for_iwc = fiducial.as_ref().unwrap_or(&full_decomp.crval);
                match build_pixel_to_iwc(
                    mapping.as_ref(),
                    &sky,
                    crval_for_iwc,
                    lon_axis,
                    lat_axis,
                    Some(proj),
                ) {
                    Ok(pixel_iwc) => {
                        // Simplify before extracting the Jacobian (the C
                        // library's MakeIntWorld at fitschan.c ~21858 does
                        // the same). With the FrameSet's own post chain on
                        // both sides of the composition, simplify reduces
                        // the redundant rotation pair to a UnitMap exactly,
                        // leaving the linear pre-WcsMap CD matrix.
                        let pixel_iwc = pixel_iwc.simplify().unwrap_or(pixel_iwc);
                        let iwc_decomp = decompose_linear(pixel_iwc.as_ref(), naxis, &g0)?;
                        if let Some(c) =
                            solve_crpix_from_decomp(&iwc_decomp.cd, &iwc_decomp.crval, &g0)
                        {
                            crpix = c;
                            full_decomp = decompose_linear(mapping.as_ref(), naxis, &crpix)?;
                        }
                        cel_crval = fiducial;
                        iwc_decomp
                    }
                    Err(_) => pre_decomp,
                }
            }
            None => pre_decomp,
        }
    } else {
        pre_decomp
    };

    // For celestial frames, swap the numerically-evaluated CRVAL for
    // the fiducial-point CRVAL extracted via the post chain. Other
    // axes keep their numerical-Jacobian values.
    let mut crval_for_store = full_decomp.crval.clone();
    if let (Some(fid), Some((_, lon_axis, lat_axis))) = (cel_crval, celestial_axes(fs)) {
        crval_for_store[lon_axis] = fid[lon_axis];
        crval_for_store[lat_axis] = fid[lat_axis];
    }

    // Store CRPIX.
    for i in 0..naxis {
        store.crpix[i] = Some(crpix[i]);
    }

    // Store the CD matrix from the pixel→IWC mapping. For celestial
    // axes the values are still in radians-per-pixel here; the
    // radian→degree conversion happens in populate_celestial which
    // knows which axes are sky.
    for i in 0..naxis {
        for j in 0..naxis {
            store.cd[i][j] = Some(cd_decomp.cd[[i, j]]);
        }
    }

    // Identify axis types from the current frame and populate CTYPE/CRVAL.
    identify_axes(fs, &crval_for_store, &mut store, projection)?;

    Ok(store)
}

/// Compute the celestial coordinate at the WcsMap's native fiducial
/// point by transforming through the post-WcsMap chain.
///
/// Returns `None` if the post chain transform fails or the output isn't
/// well-formed. The result is in the SkyFrame's native units (radians).
/// Mirrors fitschan.c's `GetFiducialWCS` — extracting CRVAL this way
/// avoids the floating-point drift of a numerical-Jacobian evaluation
/// on the full pixel→world mapping, so reconstructing WcsNative from
/// the returned CRVAL yields a matrix that exactly cancels the
/// FrameSet's WcsNative under simplification.
///
/// `template_crval` supplies values for any non-celestial axes the
/// post chain may consume — only its first two slots get rewritten
/// with the fiducial native (φ₀, θ₀) before the transform.
fn fiducial_crval(
    post_mapping: &dyn Mapping,
    projection: WcsProjection,
    template_crval: &[f64],
    naxis: usize,
) -> Option<Vec<f64>> {
    use crate::coords::PointSet;

    let wcsmap = WcsMap::new(2, projection, 1, 2).ok()?;
    let phi0 = wcsmap.nat_lon();
    let theta0 = wcsmap.nat_lat();

    // The post chain's input dimensionality matches the WcsMap output
    // dimensionality; it's at most `naxis`. We feed the fiducial native
    // (φ₀, θ₀) into the celestial slots and use the template values for
    // any extras. For pure-2D celestial frames the template isn't
    // consulted because nin == 2.
    let nin = post_mapping.nin();
    if nin == 0 || template_crval.len() < naxis {
        return None;
    }
    let mut input = vec![0.0_f64; nin];
    input[0] = phi0;
    if nin >= 2 {
        input[1] = theta0;
    }
    for i in 2..nin.min(template_crval.len()) {
        input[i] = template_crval[i];
    }

    let pts = PointSet::from_rows(std::iter::once(input));
    let out = post_mapping.transform(&pts, true).ok()?;
    if out.npoint() == 0 {
        return None;
    }
    let result = out.point(0).to_vec();
    if result.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(result)
}

/// Find the SkyFrame and its (lon_axis, lat_axis) within a FrameSet's
/// current frame, when one exists.
fn celestial_axes(fs: &FrameSet) -> Option<(SkyFrame, usize, usize)> {
    let current = fs.current();
    if let Some(sky) = current.as_any().downcast_ref::<SkyFrame>() {
        return Some((sky.clone(), 0, 1));
    }
    if let Some(cmp) = current.as_any().downcast_ref::<CmpFrame>() {
        if let Some(sky) = cmp.frame1().as_any().downcast_ref::<SkyFrame>() {
            return Some((sky.clone(), 0, 1));
        }
        let n1 = cmp.naxes1();
        if let Some(sky) = cmp.frame2().as_any().downcast_ref::<SkyFrame>() {
            return Some((sky.clone(), n1, n1 + 1));
        }
    }
    None
}

/// Compose pixel→world with WCS→IWC to obtain pixel→IWC.
///
/// Mirrors the C library (fitschan.c CelestialAxes ~5206): build
///   IWC→WCS = WcsMap_inverted (projected→native)
///           ∘ WcsNative (native→celestial, rebuilt from CRVAL)
/// then invert. We deliberately *rebuild* WcsNative from CRVAL rather
/// than reusing the FrameSet's post chain — using the FrameSet's chain
/// would cancel any extra rotations baked into the post chain (timj's
/// FK4→FK5 SlaMap), but those are precisely what FITS needs to capture
/// in the off-diagonal CD entries. Rebuilding from CRVAL means the
/// WcsNative pair cancels exactly only when the post chain is *just*
/// WcsNative; any extra rotations remain visible in the Jacobian.
///
/// Restricted to the pure-2D celestial case for now.
fn build_pixel_to_iwc(
    mapping: &dyn Mapping,
    sky: &SkyFrame,
    crval_world: &[f64],
    lon_axis: usize,
    lat_axis: usize,
    projection: Option<WcsProjection>,
) -> AstResult<DynMapping> {
    let naxis = mapping.nout();
    if naxis != 2 || lon_axis != 0 || lat_axis != 1 {
        return Err(AstError::new(
            ErrorCode::Nodef,
            "build_pixel_to_iwc only handles 2D celestial frames",
        ));
    }
    let proj = projection.ok_or_else(|| {
        AstError::new(
            ErrorCode::Nodef,
            "build_pixel_to_iwc requires a projection",
        )
    })?;

    let crval_lon = crval_world[lon_axis];
    let crval_lat = crval_world[lat_axis];
    let lonpole = std::f64::consts::PI;

    let wcs_native =
        build_native_to_celestial_rotation(crval_lon, crval_lat, lonpole).ok_or_else(|| {
            AstError::new(
                ErrorCode::Nodef,
                "failed to build native→celestial rotation",
            )
        })?;

    // IWC→WCS chain (forward direction):
    //   WcsMap_inverted (projected→native) → WcsNative (native→celestial)
    let mut wcsmap_inv = WcsMap::new(2, proj, 1, 2)?;
    wcsmap_inv.invert();
    let iwc_to_wcs = CmpMap::new(Box::new(wcsmap_inv), wcs_native, Composition::Series)?;
    let mut wcs_to_iwc = iwc_to_wcs;
    wcs_to_iwc.invert();

    // pixel→IWC = pixel→WCS ∘ WCS→IWC.
    let pixel_to_iwc =
        CmpMap::new(mapping.clone_box(), Box::new(wcs_to_iwc), Composition::Series)?;

    let _ = sky;
    Ok(Box::new(pixel_to_iwc))
}

/// Solve the C library's CRPIX equation `w₀ = CD · (g₀ − CRPIX)` for CRPIX.
///
/// Returns `None` if the CD matrix is singular — caller should fall back
/// to using `g₀` as CRPIX. Mirrors the `palDmat` step in fitschan.c's
/// MakeIntWorld function.
fn solve_crpix_from_decomp(cd: &ndarray::Array2<f64>, w0: &[f64], g0: &[f64]) -> Option<Vec<f64>> {
    let n = g0.len();
    if cd.nrows() != n || cd.ncols() != n || w0.len() != n {
        return None;
    }

    // Copy CD into a row-major flat buffer for the in-place solver, and
    // copy w₀ into the rhs buffer. The solver overwrites `rhs` with the
    // solution `x` of `CD · x = w₀`.
    let mut mat: Vec<f64> = (0..n)
        .flat_map(|i| (0..n).map(move |j| cd[[i, j]]))
        .collect();
    let mut rhs: Vec<f64> = w0.to_vec();
    if !crate::mappings::polymap::solve_linear_system(&mut mat, &mut rhs, n) {
        return None;
    }

    // CRPIX = g₀ − x.
    Some(g0.iter().zip(rhs.iter()).map(|(g, x)| g - x).collect())
}

/// Split the pixel→world mapping at the first [`WcsMap`] in its series
/// chain and return the pre-WcsMap (pixel→IWC) sub-mapping.
///
/// Returns `None` if no WcsMap is present in the chain — caller should
/// then use the full pixel→world mapping for linear extraction.
///
/// Mirrors the role of `SplitMap` in the C library (fitschan.c): for a
/// celestial WCS the structure is
///
/// ```text
/// pixel → CRPIX shift → CD matrix → IWC → ZoomMap(deg→rad) → WcsMap → ... → WCS
/// ```
///
/// and the FITS-WCS keywords CRPIX/CD/PC/CDELT describe the pixel→IWC
/// portion only. Walking only Series CmpMap children matches the C
/// library's behaviour for the common case; Parallel composition (e.g.
/// CmpFrame mixing sky+spec axes) is handled because the WcsMap appears
/// inside one of the series components in practice.
fn split_pre_wcsmap(mapping: &dyn Mapping) -> Option<DynMapping> {
    let mut pre: Vec<DynMapping> = Vec::new();
    let mut post: Vec<DynMapping> = Vec::new();
    let mut found = false;
    walk_split_wcsmap(mapping, &mut pre, &mut post, &mut found);
    if found && !pre.is_empty() {
        compose_series(pre).ok()
    } else {
        None
    }
}

/// Split the pixel→world mapping at the first [`WcsMap`] and return both
/// the pre-WcsMap (pixel→WcsMap input) chain and the post-WcsMap
/// (WcsMap output→world) chain.
///
/// The WcsMap itself is not included in either chain. Returns `None` if
/// no WcsMap is present, or if the chain on either side is empty (which
/// shouldn't happen in practice for a well-formed celestial mapping).
///
/// Used to compute the fiducial CRVAL: transform the WcsMap's native
/// fiducial native point (e.g. `(0, π/2)` for zenithal projections)
/// through the post chain to get the exact celestial coordinate the
/// FrameSet was built around. Mirrors the C library's `GetFiducialWCS`
/// approach (fitschan.c ~line 12823) which avoids the floating-point
/// drift that a numerical-Jacobian extraction would introduce.
fn split_around_wcsmap(mapping: &dyn Mapping) -> Option<(DynMapping, DynMapping)> {
    let mut pre: Vec<DynMapping> = Vec::new();
    let mut post: Vec<DynMapping> = Vec::new();
    let mut found = false;
    walk_split_wcsmap(mapping, &mut pre, &mut post, &mut found);
    if !found {
        return None;
    }
    let pre_chain = if pre.is_empty() {
        None
    } else {
        compose_series(pre).ok()
    }?;
    let post_chain = if post.is_empty() {
        None
    } else {
        compose_series(post).ok()
    }?;
    Some((pre_chain, post_chain))
}

/// Walk the mapping in execution order, accumulating components into
/// `pre` (everything before the first WcsMap) and `post` (everything
/// after it). The WcsMap itself is omitted from both chains. Sets
/// `*found = true` once the WcsMap is encountered; further components
/// then accumulate into `post`.
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
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == Composition::Series {
            walk_split_wcsmap(cmp.first(), pre, post, found);
            walk_split_wcsmap(cmp.second(), pre, post, found);
            return;
        }
        // Parallel composition isn't handled here — the caller falls
        // back to the pre-WcsMap chain.
        return;
    }
    // Leaf mapping (not a WcsMap and not a Series CmpMap): goes into
    // pre or post depending on whether we've passed the WcsMap.
    if *found {
        post.push(mapping.clone_box());
    } else {
        pre.push(mapping.clone_box());
    }
}

/// Compose a non-empty list of mappings in series order.
fn compose_series(mut parts: Vec<DynMapping>) -> AstResult<DynMapping> {
    let mut acc = parts.remove(0);
    for next in parts {
        acc = Box::new(CmpMap::new(acc, next, Composition::Series)?);
    }
    Ok(acc)
}

/// Walk the mapping tree to find the first [`WcsMap`] and return its
/// projection type.
///
/// Returns `None` if the mapping contains no `WcsMap` (e.g. a purely
/// linear pixel→world mapping). Walks both Series and Parallel CmpMap
/// children — a celestial projection is usually nested inside one of
/// them after axis-permutation and matrix steps.
fn extract_projection_from_mapping(mapping: &dyn Mapping) -> Option<WcsProjection> {
    if let Some(wm) = mapping.as_any().downcast_ref::<WcsMap>() {
        return Some(wm.projection());
    }
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if let Some(p) = extract_projection_from_mapping(cmp.first()) {
            return Some(p);
        }
        if let Some(p) = extract_projection_from_mapping(cmp.second()) {
            return Some(p);
        }
    }
    None
}

/// Identify axis types from the current frame and populate CTYPE, CRVAL, etc.
#[allow(clippy::needless_range_loop)]
fn identify_axes(
    fs: &FrameSet,
    crval: &[f64],
    store: &mut FitsStore,
    projection: Option<WcsProjection>,
) -> AstResult<()> {
    let current = fs.current();
    let naxis = store.naxis;

    if let Some(sky) = current.as_any().downcast_ref::<SkyFrame>() {
        populate_celestial(sky, crval, store, 0, 1, projection)?;
    } else if let Some(spec) = current.as_any().downcast_ref::<SpecFrame>() {
        populate_spectral(spec, crval, store, 0)?;
    } else if let Some(cmp) = current.as_any().downcast_ref::<CmpFrame>() {
        let n1 = cmp.naxes1();
        let frame1 = cmp.frame1();
        let frame2 = cmp.frame2();

        if let Some(sky) = frame1.as_any().downcast_ref::<SkyFrame>() {
            populate_celestial(sky, crval, store, 0, 1, projection)?;
        } else if let Some(spec) = frame1.as_any().downcast_ref::<SpecFrame>() {
            populate_spectral(spec, crval, store, 0)?;
        }

        if let Some(sky) = frame2.as_any().downcast_ref::<SkyFrame>() {
            populate_celestial(sky, crval, store, n1, n1 + 1, projection)?;
        } else if let Some(spec) = frame2.as_any().downcast_ref::<SpecFrame>() {
            populate_spectral(spec, crval, store, n1)?;
        }

        // Remaining axes: pull a per-axis CTYPE from the linear frame
        // attached to those axes (CmpFrame's frame1 if its axes come first,
        // else frame2). We fall back to the frame domain, then to AXISn.
        let n1 = cmp.naxes1();
        for i in 0..naxis {
            if store.ctype[i].is_none() {
                let (sub_frame, sub_axis) = if i < n1 {
                    (frame1, i + 1)
                } else {
                    (frame2, i - n1 + 1)
                };
                store.ctype[i] = Some(
                    leftover_ctype_for_axis(sub_frame, sub_axis)
                        .unwrap_or_else(|| format!("AXIS{}", i + 1)),
                );
                store.crval[i] = Some(crval[i]);
            }
        }
    } else {
        // BasicFrame or unknown: derive CTYPE per axis from the frame's
        // label (which the FITS reader sets from CTYPE), or the frame
        // domain, falling back to AXISn.
        for i in 0..naxis {
            store.ctype[i] = Some(
                leftover_ctype_for_axis(current, i + 1).unwrap_or_else(|| format!("AXIS{}", i + 1)),
            );
            store.crval[i] = Some(crval[i]);
        }
    }

    Ok(())
}

/// Derive a CTYPE value for one axis of a non-celestial, non-spectral frame.
///
/// Tries the per-axis label first (the FITS reader copies CTYPE into the
/// label), then the frame domain (e.g. `"PIXEL"`). Returns `None` when
/// nothing useful is set, letting the caller fall back to a generic AXISn.
/// The 1-based `axis` index follows the AST convention.
fn leftover_ctype_for_axis(frame: &dyn crate::frames::Frame, axis: usize) -> Option<String> {
    let basic = frame.as_any().downcast_ref::<BasicFrame>()?;
    if let Ok(label) = basic.label(axis) {
        if !label.is_empty() {
            return Some(label.to_string());
        }
    }
    let domain = basic.domain();
    if !domain.is_empty() {
        return Some(domain.to_string());
    }
    None
}

/// Populate FitsStore fields for celestial axes.
fn populate_celestial(
    sky: &SkyFrame,
    crval: &[f64],
    store: &mut FitsStore,
    lon_axis: usize,
    lat_axis: usize,
    projection: Option<WcsProjection>,
) -> AstResult<()> {
    let system = sky.system();
    // Use the projection from the WcsMap if available, falling back to
    // TAN when the mapping is purely linear (no projection step).
    let proj_code = projection.map_or("TAN", |p| p.code());

    let (lon_prefix, lat_prefix) = ctype_prefixes(system);
    store.ctype[lon_axis] = Some(format!("{lon_prefix}-{proj_code}"));
    store.ctype[lat_axis] = Some(format!("{lat_prefix}-{proj_code}"));

    // CRVAL in degrees (mapping output is radians for SkyFrame). The
    // numerical evaluation can land outside the SkyFrame's canonical
    // longitude range, e.g. when the projection rotation wraps past 2π —
    // for timj.fits-wcs this turns 309.755° into −50.245°. Normalising
    // through SkyFrame::norm before the rad→deg conversion mirrors the
    // C library's astNorm step inside CelestialAxes (fitschan.c around
    // line 5138) and keeps CRVAL in the conventional FITS range.
    use crate::frames::Frame;
    let mut sky_pair = [0.0_f64; 2];
    // SkyFrame is intrinsically 2D with axis 1 = longitude, 2 = latitude.
    sky_pair[0] = crval[lon_axis];
    sky_pair[1] = crval[lat_axis];
    sky.norm(&mut sky_pair)?;
    store.crval[lon_axis] = Some(sky_pair[0].to_degrees());
    store.crval[lat_axis] = Some(sky_pair[1].to_degrees());

    // CD rows for celestial axes were extracted from the pre-WcsMap
    // (pixel→IWC) mapping in store_from_frameset, where IWC is in
    // radians (the WcsMap input convention). FITS-WCS expects CD/CDELT
    // for celestial axes in degrees of projected x/y, so scale these
    // two rows by 180/π. The pre-WcsMap mapping does not include the
    // spherical deprojection, so there is no cos(δ) factor or sign
    // flip to undo — only the radians→degrees unit conversion.
    let scale = 180.0_f64 / std::f64::consts::PI;
    let naxis = store.naxis;
    for j in 0..naxis {
        if let Some(v) = store.cd[lon_axis][j] {
            store.cd[lon_axis][j] = Some(v * scale);
        }
        if let Some(v) = store.cd[lat_axis][j] {
            store.cd[lat_axis][j] = Some(v * scale);
        }
    }

    // RADESYS.
    store.radesys = Some(radesys_string(system));

    // EQUINOX (as Julian year).
    if system.needs_equinox() {
        let eq_mjd = sky.equinox_mjd();
        let eq_year = pal::epj(eq_mjd);
        store.equinox = Some(eq_year);
    }

    // LONPOLE default for zenithal projections.
    store.lonpole = Some(180.0);

    // MJD-OBS from epoch (skip if default J2000.0).
    let epoch = sky.epoch_mjd();
    if (epoch - 51544.5).abs() > 0.001 {
        store.mjdobs = Some(epoch);
    }

    Ok(())
}

/// Return the 4-character CTYPE prefixes for longitude and latitude.
fn ctype_prefixes(system: SkySystem) -> (&'static str, &'static str) {
    match system {
        SkySystem::Icrs
        | SkySystem::Fk5
        | SkySystem::Fk4
        | SkySystem::Fk4NoE
        | SkySystem::Gappt
        | SkySystem::J2000 => ("RA--", "DEC-"),
        SkySystem::Galactic => ("GLON", "GLAT"),
        SkySystem::Ecliptic | SkySystem::Helioecliptic => ("ELON", "ELAT"),
        SkySystem::Supergalactic => ("SLON", "SLAT"),
        SkySystem::Azel => ("AZ--", "EL--"),
        SkySystem::Unknown => ("XLON", "XLAT"),
    }
}

/// Convert a SkySystem to the FITS RADESYS string.
fn radesys_string(system: SkySystem) -> String {
    match system {
        SkySystem::Icrs => "ICRS".to_string(),
        SkySystem::Fk5 => "FK5".to_string(),
        SkySystem::Fk4 => "FK4".to_string(),
        SkySystem::Fk4NoE => "FK4-NO-E".to_string(),
        SkySystem::Gappt => "GAPPT".to_string(),
        _ => system.as_str().to_string(),
    }
}

/// Populate FitsStore fields for a spectral axis.
fn populate_spectral(
    spec: &SpecFrame,
    crval: &[f64],
    store: &mut FitsStore,
    axis: usize,
) -> AstResult<()> {
    let system = spec.system();

    store.ctype[axis] = Some(system.as_str().to_string());
    store.crval[axis] = Some(crval[axis]);
    store.cunit[axis] = Some(system.specmap_units().to_string());

    let sor = spec.std_of_rest();
    store.specsys = Some(specsys_string(sor));

    if let Some(restfrq) = spec.rest_freq() {
        store.restfrq = Some(restfrq);
    }

    Ok(())
}

/// Convert a StdOfRest to the FITS SPECSYS string.
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

/// Write FitsStore contents as FITS keyword cards.
fn write_store_to_cards(store: &FitsStore, cards: &mut CardList, use_cd: bool) -> AstResult<()> {
    let naxis = store.naxis;

    // WCSAXES
    cards.set_int("WCSAXES", naxis as i64, "Number of WCS axes");

    // CRPIXi
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = format!("CRPIX{}", i + 1);
            let comment = format!("Reference pixel on axis {}", i + 1);
            cards.set_float(&key, val, &comment);
        }
    }

    // CRVALi
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let key = format!("CRVAL{}", i + 1);
            let comment = format!("Value at ref. pixel on axis {}", i + 1);
            cards.set_float(&key, val, &comment);
        }
    }

    // CTYPEi
    for i in 0..naxis {
        if let Some(ref val) = store.ctype[i] {
            let key = format!("CTYPE{}", i + 1);
            let comment = format!("Type of co-ordinate on axis {}", i + 1);
            cards.set_string(&key, val, &comment);
        }
    }

    // CUNITi
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = format!("CUNIT{}", i + 1);
            let comment = format!("Units for axis {}", i + 1);
            cards.set_string(&key, val, &comment);
        }
    }

    // CD matrix or PC+CDELT.
    if use_cd {
        write_cd_matrix(store, cards)?;
    } else {
        write_pc_cdelt(store, cards)?;
    }

    // Ancillary keywords.
    if let Some(ref radesys) = store.radesys {
        cards.set_string("RADESYS", radesys, "Reference frame for RA/DEC values");
    }
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "[yr] Epoch of reference equinox");
    }
    if let Some(lonpole) = store.lonpole {
        cards.set_float("LONPOLE", lonpole, "Native longitude of celestial pole");
    }
    if let Some(latpole) = store.latpole {
        cards.set_float("LATPOLE", latpole, "Native latitude of celestial pole");
    }
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
    }
    if let Some(ref specsys) = store.specsys {
        cards.set_string("SPECSYS", specsys, "Spectral reference frame");
    }
    if let Some(restfrq) = store.restfrq {
        cards.set_float("RESTFRQ", restfrq, "Rest frequency (Hz)");
    }
    if let Some(restwav) = store.restwav {
        cards.set_float("RESTWAV", restwav, "Rest wavelength (m)");
    }

    Ok(())
}

/// Write CD matrix keywords.
fn write_cd_matrix(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;
    for i in 0..naxis {
        for j in 0..naxis {
            if let Some(val) = store.cd[i][j] {
                if val != 0.0 || i == j {
                    let key = format!("CD{}_{}", i + 1, j + 1);
                    cards.set_float(&key, val, "Transformation matrix element");
                }
            }
        }
    }
    Ok(())
}

/// Write PC matrix + CDELT keywords.
fn write_pc_cdelt(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    let mut cd_array = ndarray::Array2::zeros((naxis, naxis));
    for i in 0..naxis {
        for j in 0..naxis {
            cd_array[[i, j]] = store.cd[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
        }
    }
    let (cdelt, pc) = split_cd(&cd_array);

    // CDELTi
    for (i, &val) in cdelt.iter().enumerate() {
        let key = format!("CDELT{}", i + 1);
        cards.set_float(&key, val, "Pixel scale");
    }

    // PCi_j (only non-default values)
    for i in 0..naxis {
        for j in 0..naxis {
            let default = if i == j { 1.0 } else { 0.0 };
            if (pc[[i, j]] - default).abs() > 1e-15 {
                let key = format!("PC{}_{}", i + 1, j + 1);
                cards.set_float(&key, pc[[i, j]], "");
            }
        }
    }

    Ok(())
}

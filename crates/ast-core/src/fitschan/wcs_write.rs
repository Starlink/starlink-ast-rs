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
    BasicFrame, CmpFrame, Frame, FrameRef, FrameSet, SkyFrame, SkySystem, SpecFrame, StdOfRest,
};
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, WcsMap, WcsProjection};
use crate::pal;
use crate::serial::AnyObject;

// ---------------------------------------------------------------------------
// Key-formatting helpers
// ---------------------------------------------------------------------------

/// Format an axis-indexed FITS key with optional version-letter suffix.
///
/// `s == ' '` returns the bare key (primary plane); other letters
/// append the suffix. Mirrors fitschan.c::FormatKey.
fn fmt_axis_key(name: &str, axis: usize, s: char) -> String {
    if s == ' ' {
        format!("{name}{axis}")
    } else {
        format!("{name}{axis}{s}")
    }
}

/// Format a matrix-element key (e.g. CD1_2A) with optional version letter.
fn fmt_matrix_key(name: &str, i: usize, j: usize, s: char) -> String {
    if s == ' ' {
        format!("{name}{i}_{j}")
    } else {
        format!("{name}{i}_{j}{s}")
    }
}

/// Format a non-axis FITS key (e.g. WCSAXES, RADESYS) with optional version letter.
fn fmt_global_key(name: &str, s: char) -> String {
    if s == ' ' {
        name.to_string()
    } else {
        format!("{name}{s}")
    }
}

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

    let (primary, alts) = store_from_frameset_with_alts(frameset)?;
    write_store_to_cards(&primary, cards, use_cd, ' ', None)?;
    for alt in &alts {
        write_store_to_cards(&alt.store, cards, use_cd, alt.letter, Some(&alt.domain))?;
    }
    Ok(())
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

/// Decompose a FrameSet into a FitsStore using the current frame as the
/// WCS target (pixel→current mapping).
pub(crate) fn store_from_frameset(fs: &FrameSet) -> AstResult<FitsStore> {
    store_from_frameset_to(fs, fs.current_idx())
}

/// Decompose a FrameSet into a FitsStore using `target_idx` (0-based) as
/// the WCS target frame (pixel→target_frame mapping).
///
/// Mirrors fitschan.c::AddVersion (line 12241) which takes a frame index
/// and builds the axis/keyword store for that frame.
fn store_from_frameset_to(fs: &FrameSet, target_idx: usize) -> AstResult<FitsStore> {
    let naxis = fs.base().naxes();
    let mut store = FitsStore::new(naxis);

    // Get the target frame for axis identification and celestial axes.
    let target_frame = fs.frame(FrameRef::Index(target_idx + 1))?;

    // Get the base→target mapping (pixel→world) and simplify. The C
    // library's CelestialAxes (fitschan.c ~line 5093) calls astSimplify
    // before doing anything else, which collapses redundant
    // forward/inverse pairs and merges adjacent linear maps. Doing the
    // same here keeps subsequent numerical Jacobians from accumulating
    // floating-point noise across cancellable mapping steps.
    let mapping = fs
        .mapping(FrameRef::Base, FrameRef::Index(target_idx + 1))?
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
    let cd_decomp = if let Some((sky, lon_axis, lat_axis)) = celestial_axes(target_frame) {
        let parts = projection
            .and_then(|p| split_around_wcsmap(mapping.as_ref()).map(|(_, post)| (p, post)));
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
    if let (Some(fid), Some((_, lon_axis, lat_axis))) = (cel_crval, celestial_axes(target_frame)) {
        crval_for_store[lon_axis] = fid[lon_axis];
        crval_for_store[lat_axis] = fid[lat_axis];
    }

    // Store CRPIX.
    for (i, &v) in crpix.iter().enumerate().take(naxis) {
        store.crpix[i] = Some(v);
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

    // Identify axis types from the target frame and populate CTYPE/CRVAL.
    identify_axes(target_frame, &crval_for_store, &mut store, projection)?;

    // Apply C-library-style numerical cleanups (SplitMat 1e-7, CRVAL
    // near-zero, CRPIX micro-pixel rounding) so all encoders see clean
    // values and emit CD/CRVAL/CRPIX cards without FP residue.
    clean_store_residue(&mut store);

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
    let extra = nin.min(template_crval.len()).saturating_sub(2);
    if extra > 0 {
        input[2..2 + extra].copy_from_slice(&template_crval[2..2 + extra]);
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

/// Find the SkyFrame and its (lon_axis, lat_axis) within a frame, when one exists.
fn celestial_axes(frame: &dyn Frame) -> Option<(SkyFrame, usize, usize)> {
    if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
        return Some((sky.clone(), 0, 1));
    }
    if let Some(cmp) = frame.as_any().downcast_ref::<CmpFrame>() {
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
        AstError::new(ErrorCode::Nodef, "build_pixel_to_iwc requires a projection")
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
    let pixel_to_iwc = CmpMap::new(
        mapping.clone_box(),
        Box::new(wcs_to_iwc),
        Composition::Series,
    )?;

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

/// Identify axis types from the target frame and populate CTYPE, CRVAL, etc.
#[allow(clippy::needless_range_loop)]
fn identify_axes(
    frame: &dyn Frame,
    crval: &[f64],
    store: &mut FitsStore,
    projection: Option<WcsProjection>,
) -> AstResult<()> {
    let naxis = store.naxis;

    if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
        populate_celestial(sky, crval, store, 0, 1, projection)?;
    } else if let Some(spec) = frame.as_any().downcast_ref::<SpecFrame>() {
        populate_spectral(spec, crval, store, 0)?;
    } else if let Some(cmp) = frame.as_any().downcast_ref::<CmpFrame>() {
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
                leftover_ctype_for_axis(frame, i + 1).unwrap_or_else(|| format!("AXIS{}", i + 1)),
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

    // RADESYS: only emit for coordinate systems where RA/DEC axes carry
    // a reference-frame meaning. Galactic, Supergalactic, Ecliptic, AzEl,
    // and Unknown don't use RADESYS in FITS (mirrors C library policy in
    // fitschan.c WcsFromStore).
    if matches!(
        system,
        SkySystem::Icrs
            | SkySystem::Fk5
            | SkySystem::Fk4
            | SkySystem::Fk4NoE
            | SkySystem::Gappt
            | SkySystem::J2000
    ) {
        store.radesys = Some(radesys_string(system));
    }

    // EQUINOX (as Julian year).
    if system.needs_equinox() {
        let eq_mjd = sky.equinox_mjd();
        let eq_year = pal::epj(eq_mjd);
        store.equinox = Some(eq_year);
    }

    // LONPOLE default for zenithal projections.
    store.lonpole = Some(180.0);

    // MJD-OBS from epoch (skip if default J2000.0). The SkyFrame epoch
    // is in TDB scale (matching C library convention, fitschan.c:29718);
    // FITS MJD-OBS is UTC. The C library does the same conversion at
    // fitschan.c:29723-29744 via TDBConv. Also populate the formatted
    // DATE-OBS string here; an encoder later emits both.
    let epoch_tdb = sky.epoch_mjd();
    if (epoch_tdb - 51544.5).abs() > 0.001 {
        let epoch_utc = tdb_mjd_to_utc(epoch_tdb);
        store.mjdobs = Some(epoch_utc);
        store.dateobs = Some(mjd_to_date_str(epoch_utc));
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
///
/// `s` is the FITS version letter: `' '` = primary plane (bare keys),
/// `'A'`–`'Z'` = alt plane (suffixed keys). `frame_name` drives
/// WCSNAME{S} emission for alt planes; ignored for the primary plane.
fn write_store_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    use_cd: bool,
    s: char,
    frame_name: Option<&str>,
) -> AstResult<()> {
    let naxis = store.naxis;

    // WCSAXES{S}
    cards.set_int(
        &fmt_global_key("WCSAXES", s),
        naxis as i64,
        "Number of WCS axes",
    );

    // WCSNAME{S} for alt planes: frame domain.
    if s != ' ' {
        if let Some(name) = frame_name {
            if !name.is_empty() {
                cards.set_string(
                    &fmt_global_key("WCSNAME", s),
                    name,
                    "Reference name for the coord. frame",
                );
            }
        }
    }

    // CRPIXi{S}
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = fmt_axis_key("CRPIX", i + 1, s);
            let comment = format!("Reference pixel on axis {}", i + 1);
            cards.set_float(&key, val, &comment);
        }
    }

    // CRVALi{S}
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let key = fmt_axis_key("CRVAL", i + 1, s);
            let comment = format!("Value at ref. pixel on axis {}", i + 1);
            cards.set_float(&key, val, &comment);
        }
    }

    // CTYPEi{S}
    for i in 0..naxis {
        if let Some(ref val) = store.ctype[i] {
            let key = fmt_axis_key("CTYPE", i + 1, s);
            let comment = format!("Type of co-ordinate on axis {}", i + 1);
            cards.set_string(&key, val, &comment);
        }
    }

    // CUNITi{S}
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = fmt_axis_key("CUNIT", i + 1, s);
            let comment = format!("Units for axis {}", i + 1);
            cards.set_string(&key, val, &comment);
        }
    }

    // CD matrix or PC+CDELT.
    if use_cd {
        write_cd_matrix(store, cards, s)?;
    } else {
        write_pc_cdelt(store, cards, s)?;
    }

    // Plane-shared cards: only emit on primary plane.
    // Mirrors C library WcsFromStore order (fitschan.c:36867+):
    // CD matrix, then MJD-OBS / DATE-OBS, then RADESYS / EQUINOX.
    if s == ' ' {
        if let Some(mjdobs) = store.mjdobs {
            cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
        }
        // DATE-OBS companion to MJD-OBS. Mirrors fitschan.c:3610-3613.
        if let Some(ref dateobs) = store.dateobs {
            cards.set_string("DATE-OBS", dateobs, "Date of observation");
        }
    }

    // Ancillary keywords.
    if let Some(ref radesys) = store.radesys {
        cards.set_string(
            &fmt_global_key("RADESYS", s),
            radesys,
            "Reference frame for RA/DEC values",
        );
    }
    if let Some(equinox) = store.equinox {
        cards.set_float(
            &fmt_global_key("EQUINOX", s),
            equinox,
            "[yr] Epoch of reference equinox",
        );
    }
    // LONPOLE: only emit when non-default. FITS-WCS Paper II §2.4: the
    // default φp is 180° for zenithal projections with native fiducial
    // θ₀ ≥ 0° (TAN/SIN/ZEA/etc.). The C library suppresses the redundant
    // default value; emitting it would diverge from the canonical output.
    if let Some(lonpole) = store.lonpole {
        if lonpole != 180.0 {
            cards.set_float(
                &fmt_global_key("LONPOLE", s),
                lonpole,
                "Native longitude of celestial pole",
            );
        }
    }
    if let Some(latpole) = store.latpole {
        cards.set_float(
            &fmt_global_key("LATPOLE", s),
            latpole,
            "Native latitude of celestial pole",
        );
    }
    if let Some(restfrq) = store.restfrq {
        cards.set_float(
            &fmt_global_key("RESTFRQ", s),
            restfrq,
            "Rest frequency (Hz)",
        );
    }
    if let Some(restwav) = store.restwav {
        cards.set_float(
            &fmt_global_key("RESTWAV", s),
            restwav,
            "Rest wavelength (m)",
        );
    }
    if let Some(ref specsys) = store.specsys {
        cards.set_string(
            &fmt_global_key("SPECSYS", s),
            specsys,
            "Spectral reference frame",
        );
    }

    Ok(())
}

/// Write CD matrix keywords.
fn write_cd_matrix(store: &FitsStore, cards: &mut CardList, s: char) -> AstResult<()> {
    let naxis = store.naxis;
    for i in 0..naxis {
        for j in 0..naxis {
            if let Some(val) = store.cd[i][j] {
                if val != 0.0 || i == j {
                    let key = fmt_matrix_key("CD", i + 1, j + 1, s);
                    cards.set_float(&key, val, "Transformation matrix element");
                }
            }
        }
    }
    Ok(())
}

/// Write PC matrix + CDELT keywords.
fn write_pc_cdelt(store: &FitsStore, cards: &mut CardList, s: char) -> AstResult<()> {
    let naxis = store.naxis;

    let mut cd_array = ndarray::Array2::zeros((naxis, naxis));
    for i in 0..naxis {
        for j in 0..naxis {
            cd_array[[i, j]] = store.cd[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
        }
    }
    let (cdelt, pc) = split_cd(&cd_array);

    // CDELTi{S}
    for (i, &val) in cdelt.iter().enumerate() {
        let key = fmt_axis_key("CDELT", i + 1, s);
        cards.set_float(&key, val, "Pixel scale");
    }

    // PCi_j{S} (only non-default values)
    for i in 0..naxis {
        for j in 0..naxis {
            let default = if i == j { 1.0 } else { 0.0 };
            if (pc[[i, j]] - default).abs() > 1e-15 {
                let key = fmt_matrix_key("PC", i + 1, j + 1, s);
                cards.set_float(&key, pc[[i, j]], "");
            }
        }
    }

    Ok(())
}

/// Apply the C library's CRVAL near-zero snap: if `|crval[i]|` is below
/// `sqrt(DBL_EPSILON · cdelt²[i])` where `cdelt²[i] = Σ_j CD[i][j]²`,
/// set `crval[i]` to 0.
///
/// Mirrors `fitschan.c::WcsFromStore` lines 2939-2958. Threshold is
/// scale-aware: for CD scaled at 1e-3, threshold ≈ 1.5e-11; for CD at
/// 1.0, threshold ≈ 1.5e-8. This catches FP residue while preserving
/// physically-meaningful small values.
fn snap_crval_residue(crval: &[f64], cd: &ndarray::Array2<f64>) -> Vec<f64> {
    let naxis = crval.len();
    let mut out = crval.to_vec();
    for i in 0..naxis {
        let cdelt2: f64 = (0..naxis).map(|j| cd[[i, j]] * cd[[i, j]]).sum();
        let threshold = (f64::EPSILON * cdelt2).sqrt();
        if out[i].abs() < threshold {
            out[i] = 0.0;
        }
    }
    out
}

/// Apply the C library's CRPIX micro-pixel rounding: round each CRPIX
/// to the nearest millionth of a pixel.
///
/// Mirrors `fitschan.c::WcsFromStore` line 2966:
/// `floor( crpix*1.0E6 + 0.5 )*1.0E-6`.
///
/// This removes sub-millionth-pixel round-trip drift from numerical
/// CRPIX solving (`solve_crpix_from_decomp`). Real CRPIX values are
/// physically meaningful only at pixel-fraction precision, never at
/// sub-millionth.
fn round_crpix_to_micropixel(crpix: &[f64]) -> Vec<f64> {
    crpix
        .iter()
        .map(|x| (x * 1.0e6 + 0.5).floor() * 1.0e-6)
        .collect()
}

/// Apply C-library-style numerical cleanups to the FitsStore before
/// encoder dispatch. Mirrors three cleanups in fitschan.c:
///
/// 1. CD residue snap via SplitMat 1e-7 threshold (line 33238).
/// 2. CRVAL near-zero zeroing with scale-aware threshold (lines 2939-2958).
/// 3. CRPIX micro-pixel rounding (line 2966).
///
/// Without these, FP rounding noise at the 1e-15 level reaches the
/// encoder and gets emitted as `CD1_2 = -7.4E-15` cards. The C library
/// applies these in WcsFromStore; we mirror it so all encodings (FITS-WCS,
/// AIPS, AIPS++, IRAF, PC) emit clean values.
fn clean_store_residue(store: &mut FitsStore) {
    let naxis = store.naxis;

    // Build a CD matrix from store.cd, applying defaults for unset cells.
    let mut cd_array = ndarray::Array2::zeros((naxis, naxis));
    for i in 0..naxis {
        for j in 0..naxis {
            cd_array[[i, j]] = store.cd[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
        }
    }

    // 1. Snap small CD residue via SplitMat-style 1e-7 PC threshold.
    let cleaned_cd = crate::fitschan::wcs_decompose::clean_cd_residue(&cd_array);
    for i in 0..naxis {
        for j in 0..naxis {
            store.cd[i][j] = Some(cleaned_cd[[i, j]]);
        }
    }

    // 2. CRVAL near-zero snap (scale-aware threshold based on cleaned CD).
    let crval_in: Vec<f64> = (0..naxis).map(|i| store.crval[i].unwrap_or(0.0)).collect();
    let crval_out = snap_crval_residue(&crval_in, &cleaned_cd);
    for (slot, val) in store.crval.iter_mut().zip(crval_out.iter()) {
        if slot.is_some() {
            *slot = Some(*val);
        }
    }

    // 3. CRPIX micro-pixel rounding.
    let crpix_in: Vec<f64> = (0..naxis).map(|i| store.crpix[i].unwrap_or(1.0)).collect();
    let crpix_out = round_crpix_to_micropixel(&crpix_in);
    for (slot, val) in store.crpix.iter_mut().zip(crpix_out.iter()) {
        if slot.is_some() {
            *slot = Some(*val);
        }
    }
}

// ---------------------------------------------------------------------------
// Calendar helpers
// ---------------------------------------------------------------------------

/// Principal periodic term of TDB − TT in seconds.
///
/// SkyFrame.Epoch in the C library is stored on the TDB scale (see
/// fitschan.c:29718 "The epoch is in TDB"). TDB and TT differ by a
/// small periodic offset of amplitude ~1.66 ms — comparable to FITS
/// DATE-OBS millisecond precision, so it cannot be ignored.
///
/// The full series (eraDtdb) has hundreds of terms; the dominant term
/// is the Fairhead & Bretagnon (1990) annual periodic, accurate to
/// ~10 µs — well below DATE-OBS precision:
///
///   ΔTDB = 0.001658 · sin(g + 0.0167 · sin(g))   [seconds]
///   g    = 357.53° + 0.9856003° · (JD − 2 451 545.0)   (Earth mean anomaly)
///
/// Argument `mjd` is interpreted as TT or TDB (their difference is
/// well below the sensitivity of the formula).
pub(crate) fn tdb_minus_tt_seconds(mjd: f64) -> f64 {
    let d = mjd + 2_400_000.5 - 2_451_545.0;
    let g_rad = (357.53 + 0.985_600_3 * d).to_radians();
    0.001_658 * (g_rad + 0.0167 * g_rad.sin()).sin()
}

/// Convert TDB (Barycentric Dynamical Time) MJD to UTC MJD.
///
/// SkyFrame.Epoch is on the TDB scale to match the C library
/// (fitschan.c:29718). TT = TAI + 32.184 s; UTC = TAI − dat where dat
/// is the cumulative TAI-UTC leap-second offset; TT = TDB − ΔTDB. So
/// UTC = TDB − ΔTDB − (dat + 32.184) seconds, where ΔTDB is computed
/// by [`tdb_minus_tt_seconds`].
///
/// `pal::dat(mjd)` returns dat as seconds. The function takes the MJD
/// in any nearby scale (TAI, TT, UTC, TDB); the discrepancy at the
/// leap-second boundary is at most 0.5 s and doesn't matter for
/// MJD-OBS at our 3-decimal-second formatting precision.
///
/// Mirrors fitschan.c:29723-29744 (TDBConv with output scale UTC).
pub(crate) fn tdb_mjd_to_utc(tdb_mjd: f64) -> f64 {
    let dat_seconds = pal::dat(tdb_mjd);
    let tdb_tt = tdb_minus_tt_seconds(tdb_mjd);
    tdb_mjd - (dat_seconds + 32.184 + tdb_tt) / 86_400.0
}

/// Convert UTC MJD to TDB (Barycentric Dynamical Time) MJD.
///
/// Inverse of [`tdb_mjd_to_utc`]. Uses `pal::dat` and the principal
/// TDB-TT periodic term so round-trips are exact to within the formula
/// precision (≪1 µs over the date range we care about).
///
/// Mirrors `fitschan.c::TDBConv` for the UTC→TDB direction.
pub(crate) fn utc_mjd_to_tdb(utc_mjd: f64) -> f64 {
    let dat_seconds = pal::dat(utc_mjd);
    let tdb_tt = tdb_minus_tt_seconds(utc_mjd);
    utc_mjd + (dat_seconds + 32.184 + tdb_tt) / 86_400.0
}

/// Decompose an MJD (UTC, Modified Julian Date) into a Gregorian
/// calendar date + time tuple `(year, month, day, hour, min, sec_frac)`.
///
/// Uses the standard Meeus algorithm (Astronomical Algorithms, §7).
/// MJD's day boundary is at midnight (JD's day boundary is at noon, so
/// the algorithm shifts by 0.5).
fn mjd_to_gregorian(mjd: f64) -> (i32, u32, u32, u32, u32, f64) {
    let jd = mjd + 2_400_000.5_f64;
    let z = (jd + 0.5).floor() as i64;
    let frac = jd + 0.5 - z as f64; // 0.0 = midnight

    // Gregorian calendar conversion (Meeus §7).
    let a = if z < 2_299_161 {
        z
    } else {
        let alpha = ((z as f64 - 1_867_216.25) / 36_524.25).floor() as i64;
        z + 1 + alpha - alpha / 4
    };
    let b = a + 1524;
    let c = ((b as f64 - 122.1) / 365.25).floor() as i64;
    let d = (365.25 * c as f64).floor() as i64;
    let e = ((b - d) as f64 / 30.6001).floor() as i64;

    let day = (b - d - (30.6001 * e as f64).floor() as i64) as u32;
    let month = if e < 14 {
        (e - 1) as u32
    } else {
        (e - 13) as u32
    };
    let year = if month > 2 {
        (c - 4716) as i32
    } else {
        (c - 4715) as i32
    };

    // Time of day from fractional day.
    let total_secs = frac * 86400.0;
    let hour = (total_secs / 3600.0).floor() as u32;
    let rem = total_secs - hour as f64 * 3600.0;
    let min = (rem / 60.0).floor() as u32;
    let sec_frac = rem - min as f64 * 60.0;

    (year, month, day, hour, min, sec_frac)
}

/// Format an MJD (UTC) as a FITS DATE-OBS string.
///
/// Mirrors fitschan.c:3594-3608. Cutoff is MJD 51179 (1999-01-01);
/// modern dates use ISO-8601 `YYYY-MM-DDTHH:MM:SS.SSS`, legacy dates
/// use `DD/MM/YY`.
fn mjd_to_date_str(mjd: f64) -> String {
    const CUTOFF_MJD: f64 = 51179.0; // palCaldj(99, 1, 1)
    let (year, month, day, hour, min, sec_frac) = mjd_to_gregorian(mjd);
    if mjd >= CUTOFF_MJD {
        // Modern ISO-8601, 3-decimal-place seconds.
        let int_sec = sec_frac.floor() as u32;
        let ms = ((sec_frac - int_sec as f64) * 1000.0).round() as u32;
        // Handle rounding overflow (e.g. 59.9996 → ms=1000). Roll up.
        let (int_sec, ms, min, hour, day, month, year) = if ms == 1000 {
            // Defer: simplest is to add 1 second and re-format.
            let mjd_bumped = mjd + 1.0 / 86400.0;
            let (y2, mo2, d2, h2, mn2, sf2) = mjd_to_gregorian(mjd_bumped);
            let s2 = sf2.floor() as u32;
            (s2, 0, mn2, h2, d2, mo2, y2)
        } else {
            (int_sec, ms, min, hour, day, month, year)
        };
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{int_sec:02}.{ms:03}")
    } else {
        // Legacy DD/MM/YY: two-digit year (year % 100 with century strip).
        let yy = year.unsigned_abs() % 100;
        format!("{day:02}/{month:02}/{yy:02}")
    }
}

/// Parse a FITS DATE-OBS string back to UTC MJD.
///
/// Inverse of [`mjd_to_date_str`]. Accepts:
/// - ISO-8601 `YYYY-MM-DDTHH:MM:SS[.SSS]` (modern format)
/// - ISO-8601 `YYYY-MM-DD` (date-only; assumes 00:00:00)
/// - Legacy `DD/MM/YY` (assumes 00:00:00; 2-digit year mapped to
///   1900–1999 if YY >= 50, else 2000–2049)
///
/// Returns `None` if the string doesn't parse.
///
/// Mirrors fitschan.c::DateObs at line 8352.
pub(crate) fn date_str_to_mjd(s: &str) -> Option<f64> {
    let s = s.trim();
    // Legacy DD/MM/YY format.
    if let Some(parts) = parse_legacy_date(s) {
        let (year, month, day) = parts;
        return gregorian_to_mjd(year, month, day, 0, 0, 0.0);
    }
    // ISO-8601: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS[.SSS][Z]
    let (date_part, time_part) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let date_segs: Vec<&str> = date_part.split('-').collect();
    if date_segs.len() != 3 {
        return None;
    }
    let year: i32 = date_segs[0].parse().ok()?;
    let month: u32 = date_segs[1].parse().ok()?;
    let day: u32 = date_segs[2].parse().ok()?;

    let (hour, min, sec_frac) = if let Some(tp) = time_part {
        let tp = tp.trim_end_matches('Z');
        let time_segs: Vec<&str> = tp.split(':').collect();
        if time_segs.len() != 3 {
            return None;
        }
        let h: u32 = time_segs[0].parse().ok()?;
        let m: u32 = time_segs[1].parse().ok()?;
        let s: f64 = time_segs[2].parse().ok()?;
        (h, m, s)
    } else {
        (0, 0, 0.0)
    };

    gregorian_to_mjd(year, month, day, hour, min, sec_frac)
}

fn parse_legacy_date(s: &str) -> Option<(i32, u32, u32)> {
    let segs: Vec<&str> = s.split('/').collect();
    if segs.len() != 3 {
        return None;
    }
    let day: u32 = segs[0].parse().ok()?;
    let month: u32 = segs[1].parse().ok()?;
    let yy: i32 = segs[2].parse().ok()?;
    if !(0..=99).contains(&yy) {
        return None;
    }
    let year = if yy < 50 { 2000 + yy } else { 1900 + yy };
    Some((year, month, day))
}

/// Convert (Gregorian Y/M/D + H/M/S) to MJD.
///
/// Inverse of `mjd_to_gregorian`. Uses Meeus's algorithm.
fn gregorian_to_mjd(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec_frac: f64,
) -> Option<f64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (y, m) = if month <= 2 {
        (year - 1, month + 12)
    } else {
        (year, month)
    };
    let a = (y as f64 / 100.0).floor() as i64;
    let b =
        if year < 1582 || (year == 1582 && month < 10) || (year == 1582 && month == 10 && day < 15)
        {
            0
        } else {
            2 - a + a / 4
        };

    let jd = (365.25 * (y as f64 + 4716.0)).floor()
        + (30.6001 * (m as f64 + 1.0)).floor()
        + day as f64
        + b as f64
        - 1524.5;
    let frac = (hour as f64 * 3600.0 + min as f64 * 60.0 + sec_frac) / 86400.0;
    Some(jd + frac - 2_400_000.5)
}

/// Identify which frames in the FrameSet are eligible for FITS alt-plane
/// emission, and assign each a version letter ('A'..='Z').
///
/// Mirrors the loop at fitschan.c:12172-12230:
///   - Skip base and current frames (they're emitted as the primary plane).
///   - Skip frames with domain "IWC" (intermediate world coords; not externalised).
///   - First pass: if a frame's `ident()` is a single uppercase letter and
///     not yet used, use it.
///   - Second pass: assign remaining letters A, B, C, ... in frame-index
///     order, skipping any already used in the first pass.
///
/// Returns a Vec of (1-based frame index, version letter) pairs.
pub(crate) fn frames_with_version_letters(fs: &FrameSet) -> Vec<(usize, char)> {
    let nframe = fs.nframe();
    let base_idx = fs.base_idx() + 1; // 1-based to match C
    let curr_idx = fs.current_idx() + 1;

    let mut used = [false; 26]; // index 0 = 'A', 25 = 'Z'
    let mut assignment: Vec<(usize, Option<char>)> = Vec::new();

    // First pass: explicit Ident.
    for ifrm in 1..=nframe {
        if ifrm == base_idx || ifrm == curr_idx {
            continue;
        }
        let frame = match fs.frame(FrameRef::Index(ifrm)) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if frame.domain().eq_ignore_ascii_case("IWC") {
            continue;
        }
        let ident = frame.ident();
        let letter = if ident.len() == 1 {
            let c = ident.chars().next().unwrap();
            if c.is_ascii_uppercase() && !used[(c as u8 - b'A') as usize] {
                used[(c as u8 - b'A') as usize] = true;
                Some(c)
            } else {
                None
            }
        } else {
            None
        };
        assignment.push((ifrm, letter));
    }

    // Second pass: assign remaining letters in order.
    let mut next = b'A';
    for (_, opt_letter) in assignment.iter_mut() {
        if opt_letter.is_none() {
            while next <= b'Z' && used[(next - b'A') as usize] {
                next += 1;
            }
            if next > b'Z' {
                break; // out of letters; remaining frames don't get planes
            }
            *opt_letter = Some(next as char);
            used[(next - b'A') as usize] = true;
            next += 1;
        }
    }

    assignment
        .into_iter()
        .filter_map(|(idx, opt)| opt.map(|c| (idx, c)))
        .collect()
}

/// One FITS alt-WCS plane: a version letter (A-Z), the frame domain
/// (used to populate WCSNAME{S}), and a FitsStore holding axis-indexed
/// values for that plane.
#[allow(dead_code)]
pub(crate) struct AltPlane {
    pub(crate) letter: char,
    pub(crate) domain: String,
    pub(crate) store: FitsStore,
}

/// Build the primary FitsStore plus per-frame alt planes.
///
/// Returns `(primary_store, alt_planes)` where `alt_planes` is a `Vec` of
/// [`AltPlane`] values. Alt planes are computed using the same
/// `store_from_frameset_to` logic with each non-base/non-current frame as
/// the target.
///
/// Mirrors `fitschan.c::FsetToStore` (lines 12169-12259).
#[allow(dead_code)]
pub(crate) fn store_from_frameset_with_alts(
    fs: &FrameSet,
) -> AstResult<(FitsStore, Vec<AltPlane>)> {
    let primary = store_from_frameset(fs)?;
    let assignments = frames_with_version_letters(fs);

    let mut alts = Vec::with_capacity(assignments.len());
    for (frame_idx_1based, letter) in assignments {
        let target_idx = frame_idx_1based - 1;
        let alt_store = store_from_frameset_to(fs, target_idx)?;
        let frame = fs.frame(FrameRef::Index(frame_idx_1based))?;
        let domain = frame.domain().to_string();
        alts.push(AltPlane {
            letter,
            domain,
            store: alt_store,
        });
    }

    Ok((primary, alts))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_crval_near_zero_to_exact_zero() {
        let cd = ndarray::array![[1e-3, 0.0], [0.0, 1e-3]];
        let crval = vec![1e-15, 5.0];
        let snapped = snap_crval_residue(&crval, &cd);
        // 1e-15 with cdelt² = 1e-6: threshold = sqrt(eps * 1e-6) ≈ 1.5e-11.
        // 1e-15 < 1.5e-11 so should snap to 0.
        assert_eq!(snapped[0], 0.0);
        // 5.0 is way above any threshold; stays.
        assert_eq!(snapped[1], 5.0);
    }

    #[test]
    fn snap_crval_preserves_real_small_values() {
        let cd = ndarray::array![[1e-3, 0.0], [0.0, 1e-3]];
        let crval = vec![1e-9, 5.0];
        // 1e-9 is above threshold (1.5e-11) so should be preserved.
        let snapped = snap_crval_residue(&crval, &cd);
        assert_eq!(snapped[0], 1e-9);
        assert_eq!(snapped[1], 5.0);
    }

    #[test]
    fn round_crpix_to_micropixel_truncates_subpixel_drift() {
        let crpix = vec![100.0000001234, 50.5];
        let rounded = round_crpix_to_micropixel(&crpix);
        // 100.0000001234 quantizes to 100.0 (within 1e-7 of nearest 1e-6).
        assert!((rounded[0] - 100.0).abs() < 1e-9);
        // 50.5 stays exactly.
        assert_eq!(rounded[1], 50.5);
    }

    #[test]
    fn round_crpix_negative_values() {
        let crpix = vec![-10.5000002];
        let rounded = round_crpix_to_micropixel(&crpix);
        // -10.5000002 quantizes to -10.500000 (the 2e-7 drift goes).
        assert!((rounded[0] - (-10.5)).abs() < 1e-9);
    }

    #[test]
    fn mjd_to_gregorian_y2k() {
        // 2000-01-01 00:00:00 UTC = MJD 51544.0
        let (y, mo, d, h, mn, s) = mjd_to_gregorian(51544.0);
        assert_eq!(y, 2000);
        assert_eq!(mo, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 0);
        assert_eq!(mn, 0);
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn mjd_to_gregorian_j2000_noon() {
        // 2000-01-01 12:00:00 UTC = MJD 51544.5
        let (y, mo, d, h, mn, s) = mjd_to_gregorian(51544.5);
        assert_eq!((y, mo, d, h, mn), (2000, 1, 1, 12, 0));
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn mjd_to_gregorian_pre_1900() {
        // 1858-11-17 00:00:00 UTC = MJD 0
        let (y, mo, d, _, _, _) = mjd_to_gregorian(0.0);
        assert_eq!((y, mo, d), (1858, 11, 17));
    }

    #[test]
    fn mjd_to_date_str_modern_iso() {
        // MJD ≥ 51179 (1999-01-01) → ISO-8601 with millisecond precision.
        let s = mjd_to_date_str(51544.5);
        assert_eq!(s, "2000-01-01T12:00:00.000");
    }

    #[test]
    fn mjd_to_date_str_modern_with_ms_precision() {
        let s = mjd_to_date_str(52414.59);
        assert_eq!(&s[0..10], "2002-05-20");
        assert!(s.starts_with("2002-05-20T"));
        assert_eq!(&s[11..16], "14:09");
    }

    #[test]
    fn mjd_to_date_str_legacy_format() {
        // MJD < 51179 (1999-01-01) → DD/MM/YY.
        // MJD 40000 = 1968-05-24
        let s = mjd_to_date_str(40000.0);
        assert_eq!(s, "24/05/68");
    }

    #[test]
    fn mjd_to_date_str_cutoff_boundary() {
        // MJD 51179 = 1999-01-01 → modern format.
        let s = mjd_to_date_str(51179.0);
        assert!(s.starts_with("1999-01-01T"));

        // MJD 51178 = 1998-12-31 → legacy format.
        let s = mjd_to_date_str(51178.0);
        assert_eq!(s, "31/12/98");
    }

    #[test]
    fn date_str_to_mjd_iso_modern() {
        let mjd = date_str_to_mjd("2000-01-01T12:00:00.000").unwrap();
        assert!((mjd - 51544.5).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_iso_no_time() {
        let mjd = date_str_to_mjd("2000-01-01").unwrap();
        assert!((mjd - 51544.0).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_iso_with_trailing_z() {
        let mjd = date_str_to_mjd("2000-01-01T12:00:00.000Z").unwrap();
        assert!((mjd - 51544.5).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_legacy() {
        // 24/05/68 → 1968-05-24 = MJD 40000
        let mjd = date_str_to_mjd("24/05/68").unwrap();
        assert!((mjd - 40000.0).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_roundtrip_modern() {
        let original = 52414.59f64;
        let s = mjd_to_date_str(original);
        let parsed = date_str_to_mjd(&s).unwrap();
        assert!((parsed - original).abs() < 1e-3);
    }

    #[test]
    fn date_str_to_mjd_roundtrip_legacy() {
        let original = 40000.0f64;
        let s = mjd_to_date_str(original);
        let parsed = date_str_to_mjd(&s).unwrap();
        assert!((parsed - original).abs() < 1e-6);
    }

    #[test]
    fn utc_tdb_roundtrip() {
        let utc = 51544.0_f64;
        let tdb = utc_mjd_to_tdb(utc);
        let utc_back = tdb_mjd_to_utc(tdb);
        assert!((utc - utc_back).abs() < 1e-12);
    }

    #[test]
    fn tdb_tt_principal_term_amplitude() {
        // The Fairhead-Bretagnon principal term of TDB-TT has amplitude
        // ~1.66 ms. Sample one year of MJDs and check the bound holds.
        let mut max_abs: f64 = 0.0;
        for d in 0..365 {
            let mjd = 52000.0 + d as f64;
            max_abs = max_abs.max(tdb_minus_tt_seconds(mjd).abs());
        }
        assert!(max_abs > 0.0015, "amplitude too small: {max_abs}");
        assert!(max_abs < 0.002, "amplitude too large: {max_abs}");
    }

    #[test]
    fn store_from_frameset_with_alts_emits_two_alts_for_timj() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/wcsconv/inputs/timj.ast"
        ))
        .expect("read timj.ast");
        let obj = crate::serial::from_native_string(&text).expect("parse timj");
        let fs = match obj {
            crate::serial::AnyObject::FrameSet(fs) => fs,
            _ => panic!("expected FrameSet"),
        };
        let (_primary, alts) = store_from_frameset_with_alts(&fs).expect("alt planes");
        assert_eq!(alts.len(), 2, "timj has 2 alt planes (PIXEL, AXIS)");
        assert_eq!(alts[0].letter, 'A');
        assert_eq!(alts[1].letter, 'B');
        // Domain naming may vary; just check both PIXEL and AXIS are present.
        let domains: Vec<&str> = alts.iter().map(|a| a.domain.as_str()).collect();
        assert!(
            domains.iter().any(|d| d.eq_ignore_ascii_case("PIXEL")),
            "domains: {:?}",
            domains
        );
        assert!(
            domains.iter().any(|d| d.eq_ignore_ascii_case("AXIS")),
            "domains: {:?}",
            domains
        );
    }

    #[test]
    fn frames_with_version_letters_timj() {
        use crate::serial::AnyObject;
        // timj.ast has 4 frames: GRID(base), PIXEL, AXIS, SKY(current).
        // Expect: PIXEL → A, AXIS → B (both letters, in frame-index order).
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/wcsconv/inputs/timj.ast"
        ))
        .expect("read timj.ast");
        let obj = crate::serial::from_native_string(&text).expect("parse timj");
        let fs = match obj {
            AnyObject::FrameSet(fs) => fs,
            _ => panic!("expected FrameSet"),
        };
        let assignments = frames_with_version_letters(&fs);
        assert!(
            !assignments.is_empty(),
            "timj should have at least 1 alt plane, got {}",
            assignments.len()
        );
        // First two letters should be A then B (or whichever first two
        // alphabetically given Ident may override).
        let letters: Vec<char> = assignments.iter().map(|(_, c)| *c).collect();
        eprintln!("timj alt assignments: {:?}", assignments);
        assert!(
            letters.contains(&'A') || letters.contains(&'B'),
            "letters: {:?}",
            letters
        );
    }
}

// ---------------------------------------------------------------------------
// Diagnostic capture tests for fixture generation
// ---------------------------------------------------------------------------

#[cfg(test)]
mod diag_tests {
    //! Diagnostic tests that capture mappings from the wcsconv write path
    //! for offline fixture generation. Each test is `#[ignore]`d so it
    //! doesn't run in normal test suites; run explicitly with --ignored.
    //!
    //! Run a capture test with, e.g.:
    //!   cargo test -p ast-core -- fitschan::wcs_write::diag_tests::capture_gappt_pre_simplify_mapping --ignored --nocapture
    //!
    //! The captured `.map` file is then fed to the C oracle's simplify
    //! binary to generate a `.simp` reference for the simplify regression
    //! manifest.
    use super::{
        build_pixel_to_iwc, celestial_axes, extract_projection_from_mapping, fiducial_crval,
        solve_crpix_from_decomp, split_around_wcsmap, split_pre_wcsmap,
    };
    use crate::channel::Channel;
    use crate::fitschan::wcs_decompose::decompose_linear;
    use crate::fitschan::{Encoding, FitsChan};
    use crate::frames::FrameRef;
    use crate::mappings::{
        CmpMap, LutMap, Mapping, MatrixMap, PermMap, ShiftMap, SlaMap, UnitMap, WcsMap, WinMap,
        ZoomMap,
    };
    use crate::serial::{AnyObject, to_native_string};

    /// Convert a `dyn Mapping` reference to `AnyObject` by downcasting to
    /// the known concrete types used in the wcsconv path.
    fn mapping_to_any(m: &dyn Mapping) -> AnyObject {
        let a = m.as_any();
        if let Some(v) = a.downcast_ref::<CmpMap>() {
            return AnyObject::CmpMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<UnitMap>() {
            return AnyObject::UnitMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<ZoomMap>() {
            return AnyObject::ZoomMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<ShiftMap>() {
            return AnyObject::ShiftMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<MatrixMap>() {
            return AnyObject::MatrixMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<WinMap>() {
            return AnyObject::WinMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<SlaMap>() {
            return AnyObject::SlaMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<WcsMap>() {
            return AnyObject::WcsMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<PermMap>() {
            return AnyObject::PermMap(v.clone());
        }
        if let Some(v) = a.downcast_ref::<LutMap>() {
            return AnyObject::LutMap(v.clone());
        }
        panic!("mapping_to_any: unknown concrete Mapping type — extend this helper");
    }

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wcsconv/inputs")
    }

    /// Captures the pre-simplify and post-simplify pixel→IWC mapping from
    /// the `gappt_fits-wcs` wcsconv path, replicating the computation in
    /// `store_from_frameset` up to but not including the `simplify()` call
    /// at `wcs_write.rs:138`.
    ///
    /// Writes:
    ///   /tmp/gappt_pre_simplify.map      — input for C oracle
    ///   /tmp/gappt_post_simplify_rust.map — Rust simplify output for diff
    #[test]
    #[ignore = "diagnostic only — run manually with --ignored"]
    fn capture_gappt_pre_simplify_mapping() {
        let text =
            std::fs::read_to_string(fixtures_dir().join("gappt.head")).expect("gappt.head found");
        let mut fc = FitsChan::from_header(&text).expect("FitsChan from gappt.head");
        fc.set_encoding(Encoding::FitsWcs);
        let obj = fc.read().expect("read ok").expect("object present");
        let AnyObject::FrameSet(fs) = obj else {
            panic!("expected FrameSet from gappt.head");
        };

        let naxis = fs.base().naxes();
        let g0 = vec![1.0_f64; naxis];

        // Mirror store_from_frameset: simplify the pixel→world mapping first.
        let mapping = fs
            .mapping(FrameRef::Base, FrameRef::Current)
            .expect("base→current mapping")
            .simplify()
            .expect("simplify pixel→world");

        // Compute CRPIX estimate (same as store_from_frameset lines 80-86).
        let pre_wcs = split_pre_wcsmap(mapping.as_ref()).unwrap_or_else(|| mapping.clone());
        let pre_decomp = decompose_linear(pre_wcs.as_ref(), naxis, &g0).expect("pre decomp");
        let crpix = solve_crpix_from_decomp(&pre_decomp.cd, &pre_decomp.crval, &g0)
            .unwrap_or_else(|| g0.clone());
        let full_decomp = decompose_linear(mapping.as_ref(), naxis, &crpix).expect("full decomp");

        // Locate projection and celestial axes.
        let proj = extract_projection_from_mapping(mapping.as_ref())
            .expect("WcsMap projection in gappt mapping");
        let (sky, lon_axis, lat_axis) =
            celestial_axes(fs.current()).expect("celestial axes in gappt FrameSet");

        // Compute fiducial CRVAL via the post chain (same as store_from_frameset).
        let (_, post) = split_around_wcsmap(mapping.as_ref()).expect("WcsMap found in mapping");
        let fiducial = fiducial_crval(post.as_ref(), proj, &full_decomp.crval, naxis);
        let crval_for_iwc = fiducial.as_ref().unwrap_or(&full_decomp.crval).clone();

        // Build pixel→IWC — this is the mapping passed to simplify() at line 138.
        let pixel_iwc = build_pixel_to_iwc(
            mapping.as_ref(),
            &sky,
            &crval_for_iwc,
            lon_axis,
            lat_axis,
            Some(proj),
        )
        .expect("build_pixel_to_iwc succeeded");

        // Serialize the pre-simplify CmpMap.
        let cm = pixel_iwc
            .as_ref()
            .as_any()
            .downcast_ref::<CmpMap>()
            .expect("pixel_iwc is a CmpMap");
        let pre_text = to_native_string(&AnyObject::CmpMap(cm.clone()))
            .expect("serialize pre-simplify CmpMap");
        std::fs::write("/tmp/gappt_pre_simplify.map", &pre_text)
            .expect("write /tmp/gappt_pre_simplify.map");
        eprintln!(
            "Pre-simplify mapping → /tmp/gappt_pre_simplify.map ({} chars, {} lines)",
            pre_text.len(),
            pre_text.lines().count()
        );
        eprintln!(
            "First 10 lines:\n{}",
            pre_text.lines().take(10).collect::<Vec<_>>().join("\n")
        );

        // Serialize the Rust post-simplify result for comparison.
        let simplified = pixel_iwc.simplify().expect("simplify pixel_iwc");
        let simp_obj = mapping_to_any(simplified.as_ref());
        let simp_text = to_native_string(&simp_obj).expect("serialize post-simplify mapping");
        std::fs::write("/tmp/gappt_post_simplify_rust.map", &simp_text)
            .expect("write /tmp/gappt_post_simplify_rust.map");
        eprintln!(
            "Rust simplified mapping → /tmp/gappt_post_simplify_rust.map ({} chars, {} lines)",
            simp_text.len(),
            simp_text.lines().count()
        );
    }
}

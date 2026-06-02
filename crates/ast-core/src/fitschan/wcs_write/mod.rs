//! FITS-WCS encoder pipeline.
//!
//! Converts an AST FrameSet into FITS header cards by:
//! 1. Decomposing the FrameSet into a FitsStore (axis identification + linear extraction)
//! 2. Writing the FitsStore contents as FITS keyword cards
//!
//! This mirrors the C library's `FsetToStore()` → `WcsFromStore()` pipeline.

mod plane_stores;
#[allow(unused_imports)] // wired into encoders in Group 5
pub(crate) use plane_stores::{PlaneStore, collect_plane_stores};

use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::wcs_celestial::build_native_to_celestial_rotation;
use super::wcs_decompose::{decompose_linear, split_cd};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{
    BasicFrame, CmpFrame, Frame, FrameRef, FrameSet, SkyFrame, SkySystem, SpecFrame, StdOfRest,
};
use crate::mappings::helpers::ast_equal;
use crate::mappings::{
    CmpMap, Composition, DynMapping, Mapping, PermMap, ShiftMap, UnitMap, WcsMap, WcsProjection,
};
use crate::pal;
use crate::serial::AnyObject;

/// Per-axis projection parameters extracted from a `WcsMap`, in
/// `(m, value)` pairs. Indexed by FITS PV{i}_{m}: the longitude axis
/// is conventionally `i = 1`, the latitude axis `i = 2`. AST stores
/// them on both axes; FITS-WCS Paper II uses both.
type WcsmapPvSlice<'a> = (&'a [(usize, f64)], &'a [(usize, f64)]);

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
/// `dim` is the per-axis NAXIS{i} dimension array (in WCS axis order)
/// extracted from the input header. `None` entries indicate unknown
/// dimensions; the encoder uses `(1, …, 1)` as the FindBasisVectors
/// fallback in that case (mirrors fitschan.c:10526).
///
/// This is the top-level encoder entry point.
pub(crate) fn encode_wcs(
    obj: &AnyObject,
    cards: &mut CardList,
    use_cd: bool,
    dim: &[Option<f64>],
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

    let primary_crvals = extract_input_crvals(cards, ' ', frameset.base().naxes());
    let (primary, alts) = store_from_frameset_with_alts(frameset, dim, &primary_crvals, cards)?;
    write_store_to_cards(&primary, cards, use_cd, ' ', None)?;
    for alt in &alts {
        write_store_to_cards(&alt.store, cards, use_cd, alt.letter, Some(&alt.domain))?;
    }
    Ok(())
}

/// Read the input chan's `CRVAL{i}{s}` cards into a per-axis array.
/// Mirrors fitschan.c::ReadCrval (line 25775+) — the C library extracts
/// these *before* per-axis processing so that `OtherAxes`/`SpectralAxes`
/// can adopt them as the WCS→IWC ShiftMap offsets, and `MakeIntWorld`
/// then back-solves CRPIX to match. Without an input chan (e.g. `.ast`
/// inputs), all entries are `None` and the writer falls back to
/// `world(g0)` per axis.
fn extract_input_crvals(cards: &CardList, s: char, naxis: usize) -> Vec<Option<f64>> {
    (0..naxis)
        .map(|i| {
            let key = if s == ' ' {
                format!("CRVAL{}", i + 1)
            } else {
                format!("CRVAL{}{s}", i + 1)
            };
            cards.get_float(&key)
        })
        .collect()
}

/// Encode a FrameSet via FitsStore with a custom card writer.
///
/// Used by alternative encodings that share the FrameSet→FitsStore
/// decomposition but write different keywords.
pub(crate) fn encode_wcs_to_store(
    obj: &AnyObject,
    cards: &mut CardList,
    dim: &[Option<f64>],
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

    let input_crvals = extract_input_crvals(cards, ' ', frameset.base().naxes());
    let store = store_from_frameset(frameset, dim, &input_crvals)?;
    writer(&store, cards)
}

/// Decompose a FrameSet into a FitsStore using the current frame as the
/// WCS target (pixel→current mapping).
///
/// `input_crvals` carries per-axis CRVAL{i} values from the input
/// chan (matching the primary plane); see [`extract_input_crvals`]
/// for the rationale.
pub(crate) fn store_from_frameset(
    fs: &FrameSet,
    dim: &[Option<f64>],
    input_crvals: &[Option<f64>],
) -> AstResult<FitsStore> {
    store_from_frameset_to(fs, fs.current_idx(), dim, input_crvals)
}

/// Decompose a FrameSet into a FitsStore using `target_idx` (0-based) as
/// the WCS target frame (pixel→target_frame mapping).
///
/// `dim` carries per-axis NAXIS{i} values (or `None` if unknown); used
/// to choose the FindBasisVectors root grid position.
///
/// Mirrors fitschan.c::AddVersion (line 12241) which takes a frame index
/// and builds the axis/keyword store for that frame.
fn store_from_frameset_to(
    fs: &FrameSet,
    target_idx: usize,
    dim: &[Option<f64>],
    input_crvals: &[Option<f64>],
) -> AstResult<FitsStore> {
    // The new path (fset_to_store) reads the FrameSet's current frame
    // directly; for alt-plane callers (target_idx != current_idx) we
    // clone the FrameSet and bump its current pointer to the requested
    // frame before dispatching. The legacy path used a `target_idx`
    // parameter to thread this through every helper; the new path
    // localises it here.
    //
    // `input_crvals` is no longer consulted: the new path picks each
    // axis's CRVAL structurally (fiducial point for celestial axes,
    // central-pixel WCS for others), mirroring the C library's
    // GetFiducialWCS / world(g0) cascade. Input CRVAL overrides from
    // the original header are intentionally ignored — the writer's
    // job is to describe the FrameSet, not to round-trip arbitrary
    // header text.
    let _ = input_crvals;
    let encoding = crate::fitschan::Encoding::FitsWcs;
    if target_idx == fs.current_idx() {
        return crate::fitschan::fset_to_store::fset_to_store(fs, dim, encoding, ' ');
    }
    let mut alt_fs = fs.clone();
    alt_fs.set_current(FrameRef::Index(target_idx + 1))?;
    crate::fitschan::fset_to_store::fset_to_store(&alt_fs, dim, encoding, ' ')
}

#[allow(dead_code)]
fn _legacy_store_from_frameset_to(
    fs: &FrameSet,
    target_idx: usize,
    dim: &[Option<f64>],
    input_crvals: &[Option<f64>],
) -> AstResult<FitsStore> {
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
    //
    // `pre_split` records whether the WcsMap split succeeded. When it
    // does, `pre_mapping` outputs IWC (post-projection) coordinates
    // for celestial axes — small numbers near 0 at the fiducial — so
    // `pre_decomp.crval` for those axes is *not* a celestial angle,
    // and we cannot substitute the input CRVAL (degrees → radians)
    // for them in the CRPIX solve below. When the split fails (e.g.
    // a 3-D mapping with a parallel CmpMap that hides the WcsMap
    // from the structural splitter), `pre_mapping == mapping` and
    // celestial axes carry sky-radian values.
    let split_pre = split_pre_wcsmap(mapping.as_ref());
    let pre_split_succeeded = split_pre.is_some();
    let pre_mapping = split_pre.unwrap_or_else(|| mapping.clone());

    // FindBasisVectors root grid position (fitschan.c:10506-10528).
    // C uses `g0[i] = 0.5 · (1 + dim[i])` (no floor), then runs
    // `FitLine` to extract the CD matrix as a least-squares fit over
    // the whole axis range, then solves with `palDmat`. We match the
    // CRVAL substitution and the structural bits, but the CD-extraction
    // path is still finite-difference-at-`g0`, which carries an
    // O((Δpixel)²) residual for non-linear projections (TAN, SIP).
    // Floor `g0` to land on an integer pixel: when the input header's
    // CRPIX is integer (sip-3d, ncp, skyspec3d), `world(g0)` lands at
    // CRVAL and the linear solve recovers CRPIX exactly. When CRPIX
    // sits on the FITS `.5` half-pixel convention (vopt-geo,
    // freq-ghz-bary), the recovered CRPIX is off by a few µpixel — a
    // separate failure mode that requires the FitLine + SIPIntWorld
    // port to mirror C exactly. Tracked in PLAN.md.
    let g0: Vec<f64> = (0..naxis)
        .map(|i| match dim.get(i).copied().flatten() {
            Some(d) => (0.5 * (1.0 + d)).floor(),
            None => 1.0,
        })
        .collect();
    let pre_decomp = decompose_linear(pre_mapping.as_ref(), naxis, &g0)?;

    // Choose CRVAL per axis. Mirrors fitschan.c:24217 (`OtherAxes`) and
    // fitschan.c:30299 (`SpectralAxes`):
    //   crval[i] = crvals ? crvals[i] : world(central_pixel)[i]
    // The C library extracts crvals via ReadCrval (line 25775+) before
    // any per-axis processing; we feed it in via `input_crvals`. For
    // axes with no input override the chosen CRVAL falls back to
    // world(g0). The celestial branch below replaces the celestial
    // entries with the projection's fiducial CRVAL (mirrors
    // GetFiducialWCS at fitschan.c:12823) when one is available.
    let cel = celestial_axes(target_frame);
    let mut chosen_crval = pre_decomp.crval.clone();
    for (i, (slot, input)) in chosen_crval
        .iter_mut()
        .zip(input_crvals.iter())
        .take(naxis)
        .enumerate()
    {
        let is_celestial = matches!(cel, Some((_, lon, lat)) if i == lon || i == lat);
        // Celestial axes only take the input override when the WcsMap
        // split failed (i.e. `pre_mapping == mapping`). In that case
        // `pre_decomp.crval` carries sky-radian values, so feeding the
        // input CRVAL (FITS-WCS degrees → radians) into the linear
        // solve recovers the input CRPIX exactly — which is how the
        // standard `.5` half-pixel CRPIX convention round-trips. When
        // the split does succeed, `pre_decomp.crval` for celestial
        // axes is in IWC space (small post-projection coordinates
        // near 0), and the input CRVAL belongs to a different
        // reference frame: substituting it would produce a wildly
        // wrong CRPIX from the solve.
        if is_celestial && !pre_split_succeeded {
            if let Some(c) = *input {
                *slot = c.to_radians();
            }
        } else if !is_celestial {
            if let Some(c) = *input {
                *slot = c;
            }
        }
    }

    // CRPIX is the linear-algebra solution to
    //   pre_decomp.cd · (g0 − CRPIX) = world(g0) − chosen_crval
    // i.e. find CRPIX such that world(CRPIX) = chosen_crval. Mirrors
    // MakeIntWorld's palDmat solve (fitschan.c:21943-22095). For axes
    // without an input override, world(g0) − chosen_crval = 0 and the
    // solve gives CRPIX[i] = g0[i] (which matches what `OtherAxes`
    // produces when no `crvals[i]` is supplied).
    //
    // Wrap longitude differences into [−π, π] for celestial axes so a
    // 2π discontinuity between `pre_decomp.crval[lon]` (the mapping's
    // numerical evaluation, which can land outside the canonical FITS
    // range, e.g. −2.749 rad / +3.534 rad ≡ same RA) and the input
    // CRVAL doesn't blow up the solve. Sip-3d's pre-WcsMap chain at
    // CRPIX returns the celestial coordinate in the (−π, π] branch,
    // while the input is in the [0, 2π) branch — without unwrapping,
    // `w0[lon] = ±2π` and the solve produces a CRPIX off by ~10⁵
    // pixels.
    let w0: Vec<f64> = pre_decomp
        .crval
        .iter()
        .zip(chosen_crval.iter())
        .enumerate()
        .map(|(i, (w, c))| {
            let diff = w - c;
            let is_lon = matches!(cel, Some((_, lon, _)) if i == lon);
            if is_lon && diff.is_finite() {
                let two_pi = 2.0 * std::f64::consts::PI;
                diff - two_pi * (diff / two_pi).round()
            } else {
                diff
            }
        })
        .collect();
    let mut crpix = solve_crpix_from_decomp(&pre_decomp.cd, &w0, &g0).unwrap_or_else(|| g0.clone());
    let full_decomp = decompose_linear(mapping.as_ref(), naxis, &crpix)?;

    let projection = extract_projection_from_mapping(mapping.as_ref());
    // Collect non-default PV parameters on the longitude AND latitude
    // axes. FITS-WCS Paper II uses both: most projections only carry
    // PV{lat,m} (e.g. SIN's PV2_1, PV2_2 or ZPN's PV2_0..20), but a
    // few — TAN/ARC/STG with non-default fiducial-point offset — use
    // PV{lon,0} as well (e.g. tan-pv0's `PV1_0 = 0.1`).
    type WcsmapPvOwned = (Vec<(usize, f64)>, Vec<(usize, f64)>);
    let wcsmap_pv: Option<WcsmapPvOwned> = find_wcsmap(mapping.as_ref()).map(|wm| {
        let lon = wm.lonax();
        let lat = wm.latax();
        let lon_pv: Vec<(usize, f64)> = (0..16)
            .filter_map(|m| wm.get_pv(lon, m).map(|v| (m, v)))
            .collect();
        let lat_pv: Vec<(usize, f64)> = (0..16)
            .filter_map(|m| wm.get_pv(lat, m).map(|v| (m, v)))
            .collect();
        (lon_pv, lat_pv)
    });

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
    // SIP distortion: locate the 2-in/2-out PolyMap (if any) and
    // build a "stripped" copy of the mapping with that PolyMap
    // replaced by a UnitMap(2). The stripped mapping feeds
    // `build_pixel_to_iwc` so the linear CD/CRPIX extraction sees
    // the FITS-WCS linear model the SIP convention sits on top of.
    // Mirrors `SIPIntWorld`'s "Mapping equivalent to the input but
    // without the PolyMap" return value (fitschan.c:29009-29014).
    let stripped_mapping = strip_sip_polymap(mapping.as_ref());
    let cd_extract_mapping: &dyn Mapping = stripped_mapping
        .as_deref()
        .unwrap_or_else(|| mapping.as_ref());

    let mut cel_crval: Option<Vec<f64>> = None;
    let cd_decomp = if let Some((sky, lon_axis, lat_axis)) = celestial_axes(target_frame) {
        match projection {
            Some(proj) => {
                // Fiducial CRVAL: prefer extracting via the post-WcsMap
                // chain (mirrors C's GetFiducialWCS, fitschan.c:12823),
                // which gives a value bit-exact to what the FrameSet
                // was built around. For mappings whose WcsMap is buried
                // inside a parallel CmpMap (3D celestial+spectral),
                // split_around_wcsmap returns None and we fall back to
                // the numerical world(g0) value — still correct, just
                // less precise.
                let post = split_around_wcsmap(cd_extract_mapping).map(|(_, post)| post);
                let fiducial = post
                    .as_ref()
                    .and_then(|p| fiducial_crval(p.as_ref(), proj, &full_decomp.crval, naxis));
                let crval_for_iwc = fiducial.as_ref().unwrap_or(&full_decomp.crval);
                cel_crval = fiducial.clone();
                match build_pixel_to_iwc(
                    cd_extract_mapping,
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
                        }
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

    // CRVAL for the store: start from the chosen array (input override
    // or world(g0)), then for celestial axes substitute the fiducial
    // CRVAL extracted via the post chain. The fiducial value is
    // bit-exact to what the FrameSet was built around, mirroring
    // fitschan.c::GetFiducialWCS (~12823); we prefer it over the
    // input override for celestial axes because the fiducial point
    // is a property of the projection, not the file header.
    //
    // When `cel_crval` is `None` (e.g. 3D mapping where
    // `split_around_wcsmap` can't isolate the WcsMap from a parallel
    // CmpMap), `chosen_crval` for celestial axes still holds
    // `pre_decomp.crval = world(g0)`. With our `g0` set to the
    // central pixel `floor(0.5*(1+dim))`, that's offset half a pixel
    // from CRPIX in each direction — the projection then drifts by
    // ~0.5 × CDELT, e.g. ~0.0005° on the freq-ghz-bary fixture.
    // Mirror what C does in this case (it stores the input CRVAL
    // directly, not the world(g0) value): when an input override is
    // available for a celestial axis, take it (deg → rad) instead.
    let mut crval_for_store = chosen_crval;
    let cel = celestial_axes(target_frame);
    if let (Some(fid), Some((_, lon_axis, lat_axis))) = (&cel_crval, &cel) {
        crval_for_store[*lon_axis] = fid[*lon_axis];
        crval_for_store[*lat_axis] = fid[*lat_axis];
    } else if let Some((_, lon_axis, lat_axis)) = cel {
        // No fiducial — prefer input CRVAL for celestial axes too.
        for ax in [lon_axis, lat_axis] {
            if let Some(Some(deg)) = input_crvals.get(ax) {
                crval_for_store[ax] = deg.to_radians();
            }
        }
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

    // SIP distortion: if the FrameSet's mapping carries a 2-in/2-out
    // PolyMap (the AST shape produced when reading FITS-WCS headers
    // with -SIP CTYPE codes), extract its coefficients into the store
    // before identifying axes — `identify_axes` consults
    // `store.has_sip()` to decide whether to append "-SIP" to the
    // celestial CTYPEs. Mirrors `SIPIntWorld` (fitschan.c:28699-28990).
    let has_sip = if let Some(pm) = find_sip_polymap(mapping.as_ref()) {
        extract_sip_into_store(pm, &mut store)
    } else {
        false
    };

    // Identify axis types from the target frame and populate CTYPE/CRVAL.
    let pv_refs = wcsmap_pv
        .as_ref()
        .map(|(lon, lat)| (lon.as_slice(), lat.as_slice()));
    identify_axes(
        target_frame,
        &crval_for_store,
        input_crvals,
        &mut store,
        projection,
        pv_refs,
    )?;

    // Append `-SIP` to the celestial CTYPEs when SIP coefficients were
    // extracted (mirrors fitschan.c:28992-29007).
    if has_sip && let Some((_, lon_axis, lat_axis)) = celestial_axes(target_frame) {
        for ax in [lon_axis, lat_axis] {
            if let Some(ref mut ct) = store.ctype[ax] {
                if !ct.ends_with("-SIP") {
                    ct.push_str("-SIP");
                }
            }
        }
    }

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

/// Find the SkyFrame and its `(lon_axis, lat_axis)` indices within
/// the parent frame. Mirrors `astGetLonAxis`/`astGetLatAxis` —
/// for a SkyFrame whose `LatAxis=0, LonAxis=1` (the sip-3d-swap
/// case), the returned indices reflect that permutation rather than
/// always reporting `(0, 1)`.
fn celestial_axes(frame: &dyn Frame) -> Option<(SkyFrame, usize, usize)> {
    if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
        let (lon, lat) = (sky.lon_axis(), sky.lat_axis());
        return Some((sky.clone(), lon, lat));
    }
    if let Some(cmp) = frame.as_any().downcast_ref::<CmpFrame>() {
        if let Some(sky) = cmp.frame1().as_any().downcast_ref::<SkyFrame>() {
            let (lon, lat) = (sky.lon_axis(), sky.lat_axis());
            return Some((sky.clone(), lon, lat));
        }
        let n1 = cmp.naxes1();
        if let Some(sky) = cmp.frame2().as_any().downcast_ref::<SkyFrame>() {
            let (lon, lat) = (sky.lon_axis(), sky.lat_axis());
            return Some((sky.clone(), n1 + lon, n1 + lat));
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
/// Handles N-D celestial+other frames. The 2D celestial chain
/// (`WcsMap_inv ∘ WcsNative`) is composed in parallel with a UnitMap
/// over the remaining N−2 axes, sandwiched between PermMaps that
/// move the celestial pair to/from the leading two positions —
/// mirroring the PermMap sandwich at fitschan.c:5247-5296. This
/// produces a WCS→IWC mapping that operates on all `naxis` axes
/// (celestial axes get projected, non-celestial axes pass through).
///
/// As in our 2D code path, we leave the deg↔rad scale on the
/// celestial axes implicit: the resulting CD rows for celestial axes
/// come out in radians-per-pixel, and `populate_celestial` later
/// multiplies them by 180/π to land in FITS-WCS degrees.
fn build_pixel_to_iwc(
    mapping: &dyn Mapping,
    sky: &SkyFrame,
    crval_world: &[f64],
    lon_axis: usize,
    lat_axis: usize,
    projection: Option<WcsProjection>,
) -> AstResult<DynMapping> {
    let naxis = mapping.nout();
    if naxis < 2 || lon_axis >= naxis || lat_axis >= naxis || lon_axis == lat_axis {
        return Err(AstError::new(
            ErrorCode::Nodef,
            "build_pixel_to_iwc requires a 2D celestial pair within naxis ≥ 2",
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

    // 2D celestial IWC→WCS chain (forward direction):
    //   WcsMap_inverted (projected→native) → WcsNative (native→celestial)
    let mut wcsmap_inv = WcsMap::new(2, proj, 1, 2)?;
    wcsmap_inv.invert();
    let cel_2d: DynMapping = Box::new(CmpMap::new(
        Box::new(wcsmap_inv),
        wcs_native,
        Composition::Series,
    )?);

    // Lift to N-D: parallel-compose with a UnitMap over the remaining
    // axes, then bracket with PermMaps so the celestial pair sits at
    // the leading two positions of the parallel composition.
    let iwc_to_wcs_nd: DynMapping = if naxis == 2 && lon_axis == 0 && lat_axis == 1 {
        cel_2d
    } else {
        // Permutation: original axis order → (lon, lat, others_in_order).
        // PermMap forward uses outperm; element i is the input axis
        // (1-based) to copy into output position i.
        let mut outperm: Vec<i64> = Vec::with_capacity(naxis);
        outperm.push((lon_axis + 1) as i64);
        outperm.push((lat_axis + 1) as i64);
        for ax in 0..naxis {
            if ax != lon_axis && ax != lat_axis {
                outperm.push((ax + 1) as i64);
            }
        }
        // inperm is the inverse: position k of inperm tells where
        // original input k landed in the permuted layout (also 1-based).
        let mut inperm: Vec<i64> = vec![0; naxis];
        for (out_i, &in_i) in outperm.iter().enumerate() {
            inperm[(in_i as usize) - 1] = (out_i + 1) as i64;
        }
        let perm_in = PermMap::new(&inperm, &outperm, &[])?;
        let mut perm_out = PermMap::new(&inperm, &outperm, &[])?;
        perm_out.invert();

        // Parallel composition: 2D celestial chain || ShiftMap of
        // CRVAL on each remaining axis. The shift mirrors C's
        // `OtherAxes` / `SpectralAxes` step (fitschan.c:24241,
        // ShiftMap with -crval): they emit a ShiftMap that subtracts
        // CRVAL when going WCS→IWC, so IWC sits at zero at the
        // reference pixel. We're constructing IWC→WCS forward here,
        // so the ShiftMap forward direction *adds* CRVAL. After
        // inversion of the whole chain, non-celestial axes get the
        // CRVAL subtracted — without this, `solve_crpix` ends up
        // dividing the absolute world value by CDELT and yields
        // CRPIX ≈ −CRVAL/CDELT instead of the input pixel.
        let extra = naxis - 2;
        let parallel: DynMapping = if extra == 0 {
            cel_2d
        } else {
            // Collect CRVAL for the non-celestial axes in their permuted
            // (post-perm_in) order, which matches outperm[2..].
            let mut other_shifts: Vec<f64> = Vec::with_capacity(extra);
            for &ax_1based in &outperm[2..] {
                let ax = (ax_1based as usize) - 1;
                other_shifts.push(crval_world.get(ax).copied().unwrap_or(0.0));
            }
            let other_shift = ShiftMap::new(other_shifts);
            Box::new(CmpMap::new(
                cel_2d,
                Box::new(other_shift),
                Composition::Parallel,
            )?)
        };

        // perm_in (original→permuted) → parallel → perm_out (permuted→original)
        let chain1 = CmpMap::new(Box::new(perm_in), parallel, Composition::Series)?;
        Box::new(CmpMap::new(
            Box::new(chain1),
            Box::new(perm_out),
            Composition::Series,
        )?)
    };

    let mut wcs_to_iwc = iwc_to_wcs_nd;
    wcs_to_iwc.invert();

    // pixel→IWC = pixel→WCS ∘ WCS→IWC.
    let pixel_to_iwc = CmpMap::new(mapping.clone_box(), wcs_to_iwc, Composition::Series)?;

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

/// Walk the mapping tree and return a reference to the first
/// `WcsMap` found. Companion to [`extract_projection_from_mapping`]
/// for callers that need the full WcsMap (e.g. to pull out PV
/// parameters in addition to the projection code).
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

/// Walk the mapping tree and return a reference to the first 2-in,
/// 2-out [`PolyMap`] (the SIP-distortion shape). Mirrors the C
/// library's `astMapList` walk in `SIPIntWorld` (fitschan.c:28699-28716)
/// — we recurse through both Series and Parallel CmpMaps because in
/// 3D inputs (e.g. RA/DEC + FREQ) the SIP PolyMap sits inside the
/// celestial branch of a parallel composition.
fn find_sip_polymap(mapping: &dyn Mapping) -> Option<&crate::mappings::PolyMap> {
    if let Some(pm) = mapping.as_any().downcast_ref::<crate::mappings::PolyMap>() {
        if Mapping::nin(pm) == 2 && Mapping::nout(pm) == 2 {
            return Some(pm);
        }
    }
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if let Some(pm) = find_sip_polymap(cmp.first()) {
            return Some(pm);
        }
        if let Some(pm) = find_sip_polymap(cmp.second()) {
            return Some(pm);
        }
    }
    None
}

/// Walk the mapping tree and produce a copy with the first 2-in,
/// 2-out [`PolyMap`] replaced by a [`UnitMap`] of width 2. Returns
/// `None` if no such PolyMap is present. Mirrors the "Mapping
/// without the PolyMap" return value at the end of `SIPIntWorld`
/// (fitschan.c:29009): downstream code can decompose the result
/// linearly to recover CRPIX and CD without the polynomial
/// distortion fighting back.
fn strip_sip_polymap(mapping: &dyn Mapping) -> Option<DynMapping> {
    if let Some(pm) = mapping.as_any().downcast_ref::<crate::mappings::PolyMap>() {
        if Mapping::nin(pm) == 2 && Mapping::nout(pm) == 2 {
            return Some(Box::new(UnitMap::new(2)));
        }
    }
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if let Some(new_first) = strip_sip_polymap(cmp.first()) {
            return CmpMap::new(new_first, cmp.second().clone_box(), cmp.composition())
                .ok()
                .map(|c| Box::new(c) as DynMapping);
        }
        if let Some(new_second) = strip_sip_polymap(cmp.second()) {
            return CmpMap::new(cmp.first().clone_box(), new_second, cmp.composition())
                .ok()
                .map(|c| Box::new(c) as DynMapping);
        }
    }
    None
}

/// Extract SIP forward (and inverse, when present) coefficients from
/// a 2-in, 2-out PolyMap into the FitsStore's `sip_a` / `sip_b` /
/// `sip_ap` / `sip_bp` matrices. Mirrors the coefficient extraction
/// loop in `SIPIntWorld` at fitschan.c:28897-28990.
///
/// The PolyMap our reader builds (and any SIP-shaped PolyMap) carries
/// the `+u` and `+v` linear identity terms inline; the SIP convention
/// stores them as the *additive* correction (so A_1_0 and B_0_1 are 0
/// when the linear part is identity). We subtract 1.0 from those two
/// terms when copying them out.
///
/// Returns `true` if any forward coefficients were stored. The caller
/// uses that to decide whether to append `-SIP` to the celestial CTYPEs.
fn extract_sip_into_store(pm: &crate::mappings::PolyMap, store: &mut FitsStore) -> bool {
    use crate::mappings::PolyTerm;
    let emit = |terms: &[PolyTerm], a_buf: &mut Vec<Vec<f64>>, b_buf: &mut Vec<Vec<f64>>| {
        let mut a_max = 0usize;
        let mut b_max = 0usize;
        for t in terms {
            if t.powers.len() < 2 {
                continue;
            }
            let p = t.powers[0];
            let q = t.powers[1];
            match t.out_axis {
                1 => a_max = a_max.max(p + q),
                2 => b_max = b_max.max(p + q),
                _ => {}
            }
        }
        let a_size = a_max + 1;
        let b_size = b_max + 1;
        *a_buf = vec![vec![0.0; a_size]; a_size];
        *b_buf = vec![vec![0.0; b_size]; b_size];
        for t in terms {
            if t.powers.len() < 2 {
                continue;
            }
            let p = t.powers[0];
            let q = t.powers[1];
            let mut val = t.coeff;
            // SIP stores additive corrections; subtract the identity
            // contribution that the inline PolyMap carries.
            if t.out_axis == 1 && p == 1 && q == 0 {
                val -= 1.0;
            }
            if t.out_axis == 2 && p == 0 && q == 1 {
                val -= 1.0;
            }
            if val == 0.0 {
                continue;
            }
            match t.out_axis {
                1 => a_buf[p][q] = val,
                2 => b_buf[p][q] = val,
                _ => {}
            }
        }
        (a_max, b_max)
    };

    let (a_max_f, b_max_f) = emit(pm.forward_terms(), &mut store.sip_a, &mut store.sip_b);
    if !pm.forward_terms().is_empty() {
        store.sip_a_order = Some(a_max_f);
        store.sip_b_order = Some(b_max_f);
    }

    if let Some(inv) = pm.inverse_terms() {
        let (a_max_i, b_max_i) = emit(inv, &mut store.sip_ap, &mut store.sip_bp);
        if !inv.is_empty() {
            store.sip_ap_order = Some(a_max_i);
            store.sip_bp_order = Some(b_max_i);
        }
    }

    !pm.forward_terms().is_empty()
}

/// Identify axis types from the target frame and populate CTYPE, CRVAL, etc.
#[allow(clippy::needless_range_loop)]
fn identify_axes(
    frame: &dyn Frame,
    crval: &[f64],
    input_crvals: &[Option<f64>],
    store: &mut FitsStore,
    projection: Option<WcsProjection>,
    wcsmap_pv: Option<WcsmapPvSlice<'_>>,
) -> AstResult<()> {
    let naxis = store.naxis;

    // For SkyFrames, honour the per-frame lon/lat permutation
    // (`astGetLonAxis`/`astGetLatAxis`) so an input with
    // CTYPE1='DEC--TAN', CTYPE2='RA---TAN' lands DEC at FITS axis 1
    // and RA at FITS axis 2.
    if let Some(sky) = frame.as_any().downcast_ref::<SkyFrame>() {
        let lon = sky.lon_axis();
        let lat = sky.lat_axis();
        populate_celestial(sky, crval, store, lon, lat, projection, wcsmap_pv)?;
    } else if let Some(spec) = frame.as_any().downcast_ref::<SpecFrame>() {
        populate_spectral(spec, crval, store, 0)?;
    } else if let Some(cmp) = frame.as_any().downcast_ref::<CmpFrame>() {
        let n1 = cmp.naxes1();
        let frame1 = cmp.frame1();
        let frame2 = cmp.frame2();

        // Track the celestial source position (radians) so that any
        // SpecFrame in the same CmpFrame can compute VELOSYS — the
        // topocentric velocity of the SPECSYS rest frame depends on
        // the source direction and the observation epoch.
        let mut src_ra_dec: Option<(f64, f64)> = None;

        // For VELOSYS we want the celestial source position to match
        // C's `RefRA`/`RefDec` exactly. C transfers these via
        // `astSetC( tfs, "RefRA", astFormat( tfs, ilon, reflon ) )`
        // (fitschan.c:21483) — i.e. it formats with the SkyFrame's
        // own formatter and parses the string back. That string
        // round trip quantises the value to whatever precision the
        // SkyFrame's `Format`/`Digits` attributes specify (default
        // is HMS at 0.1-sec precision for RA, DMS at 1-arcsec for
        // Dec), and the rounded RA/Dec is what feeds VELOSYS. To
        // stay bit-compatible with C-generated fixtures we replay
        // the same round trip here using the SkyFrame's own
        // `format`/`unformat` — this way the quantisation tracks
        // whatever precision rule the formatter uses (currently
        // hardcoded to default Digits in our SkyFrame, but if/when
        // we add Digits-aware formatting the quantisation here
        // follows automatically).
        let format_round_trip = |sky_frame: &SkyFrame, axis: usize, value: f64| -> f64 {
            // axis is 1-based per Frame::format / Frame::unformat.
            sky_frame
                .format(axis, value)
                .ok()
                .and_then(|s| sky_frame.unformat(axis, &s).ok())
                .map(|(v, _)| v)
                .unwrap_or(value)
        };
        let cap_src = |sky: &SkyFrame, lon_idx: usize, lat_idx: usize| -> (f64, f64) {
            let ra = input_crvals
                .get(lon_idx)
                .and_then(|v| *v)
                .map(|d| d.to_radians())
                .unwrap_or(crval[lon_idx]);
            let dec = input_crvals
                .get(lat_idx)
                .and_then(|v| *v)
                .map(|d| d.to_radians())
                .unwrap_or(crval[lat_idx]);
            // axis 1 = lon (RA-like), axis 2 = lat (Dec-like) in
            // SkyFrame's external numbering.
            let lon_ext = sky.lon_axis() + 1;
            let lat_ext = sky.lat_axis() + 1;
            (
                format_round_trip(sky, lon_ext, ra),
                format_round_trip(sky, lat_ext, dec),
            )
        };

        if let Some(sky) = frame1.as_any().downcast_ref::<SkyFrame>() {
            let lon = sky.lon_axis();
            let lat = sky.lat_axis();
            src_ra_dec = Some(cap_src(sky, lon, lat));
            populate_celestial(sky, crval, store, lon, lat, projection, wcsmap_pv)?;
        } else if let Some(spec) = frame1.as_any().downcast_ref::<SpecFrame>() {
            populate_spectral(spec, crval, store, 0)?;
        }

        if let Some(sky) = frame2.as_any().downcast_ref::<SkyFrame>() {
            let lon = sky.lon_axis();
            let lat = sky.lat_axis();
            src_ra_dec = Some(cap_src(sky, n1 + lon, n1 + lat));
            populate_celestial(sky, crval, store, n1 + lon, n1 + lat, projection, wcsmap_pv)?;
        } else if let Some(spec) = frame2.as_any().downcast_ref::<SpecFrame>() {
            populate_spectral(spec, crval, store, n1)?;
        }

        // Now that both frames have been visited, fill in VELOSYS
        // when both a SpecFrame and a SkyFrame are present.
        if let Some((ra, dec)) = src_ra_dec {
            let spec_opt = frame1
                .as_any()
                .downcast_ref::<SpecFrame>()
                .or_else(|| frame2.as_any().downcast_ref::<SpecFrame>());
            if let Some(spec) = spec_opt {
                populate_velosys(spec, ra, dec, store);
            }
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
                populate_other_axis(sub_frame, sub_axis, i, store);
                store.crval[i] = Some(crval[i]);
            }
        }
    } else {
        // BasicFrame or unknown: derive CTYPE per axis from the frame's
        // symbol (mirrors C's OtherAxes, fitschan.c:24364).
        for i in 0..naxis {
            populate_other_axis(frame, i + 1, i, store);
            store.crval[i] = Some(crval[i]);
        }
    }

    Ok(())
}

/// Populate CTYPE / CTYPE comment / CUNIT for one non-celestial,
/// non-spectral axis using the C library's `OtherAxes` recipe
/// (fitschan.c:24364-24390):
///
/// - CTYPE = axis Symbol (or `"AXIS{n}"` if none).
/// - CTYPE comment = axis Label when explicitly set and different
///   from Symbol; otherwise `"Type of co-ordinate on axis {n}"` (left
///   as None so the writer fills the default).
/// - CUNIT = axis Unit when set.
///
/// `axis` is the 1-based axis index within `frame`; `store_axis` is the
/// 0-based slot in the store to write to.
fn populate_other_axis(
    frame: &dyn crate::frames::Frame,
    axis: usize,
    store_axis: usize,
    store: &mut FitsStore,
) {
    let basic = match frame.as_any().downcast_ref::<BasicFrame>() {
        Some(b) => b,
        None => {
            store.ctype[store_axis] = Some(format!("AXIS{axis}"));
            return;
        }
    };

    let symbol = basic.symbol(axis).ok().unwrap_or("");
    let label = basic.label(axis).ok().unwrap_or("");
    let label_set = basic.test_label(axis).ok().unwrap_or(false);

    let ctype = if !symbol.is_empty() {
        symbol.to_string()
    } else {
        format!("AXIS{axis}")
    };
    store.ctype[store_axis] = Some(ctype);

    if label_set && !label.is_empty() && label != symbol {
        store.ctype_com[store_axis] = Some(label.to_string());
        if store.cname[store_axis].is_none() {
            store.cname[store_axis] = Some(label.to_string());
        }
    }

    if basic.test_unit(axis).ok().unwrap_or(false)
        && let Ok(u) = basic.unit(axis)
        && !u.is_empty()
        && store.cunit[store_axis].is_none()
    {
        store.cunit[store_axis] = Some(u.to_string());
    }
}

/// Populate FitsStore fields for celestial axes.
fn populate_celestial(
    sky: &SkyFrame,
    crval: &[f64],
    store: &mut FitsStore,
    lon_axis: usize,
    lat_axis: usize,
    projection: Option<WcsProjection>,
    wcsmap_pv: Option<WcsmapPvSlice<'_>>,
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
    // sky.norm uses the SkyFrame's *external* axis ordering (via
    // lon_pos / lat_pos), which is also what `lon_axis`/`lat_axis`
    // here describe. Lay out sky_pair in that ordering so the lon/lat
    // wrapping doesn't swap them when the SkyFrame has been permuted
    // (sip-3d-swap layout).
    let mut sky_pair = [0.0_f64; 2];
    let sky_lon = sky.lon_axis();
    let sky_lat = sky.lat_axis();
    sky_pair[sky_lon] = crval[lon_axis];
    sky_pair[sky_lat] = crval[lat_axis];
    sky.norm(&mut sky_pair)?;
    store.crval[lon_axis] = Some(sky_pair[sky_lon].to_degrees());
    store.crval[lat_axis] = Some(sky_pair[sky_lat].to_degrees());

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

    // EQUINOX. Stored in the form the encoder will emit, mirroring
    // fitschan.c:29761-29800: FK5 → Julian year, FK4/FK4-NO-E → Besselian
    // year, Ecliptic → MJD (no conversion). Helioecliptic and the rest
    // set `eq = AST__BAD` in C (no EQUINOX/EPOCH emitted), but our
    // `needs_equinox()` excludes them already.
    if system.needs_equinox() {
        let eq_mjd = sky.equinox_mjd();
        let eq = match system {
            SkySystem::Fk4 | SkySystem::Fk4NoE => pal::epb(eq_mjd),
            SkySystem::Ecliptic => eq_mjd,
            _ => pal::epj(eq_mjd),
        };
        store.equinox = Some(eq);
    }

    // PV parameters from the WcsMap. AST stores them on both lon and
    // lat axes; FITS-WCS Paper II uses both: most projections only
    // populate PV{lat,m} (e.g. SIN's PV2_2 for the legacy NCP form, or
    // ZPN's PV2_0..20), but a few — TAN/ARC/STG with non-default
    // fiducial-point offset — use PV{lon,0} as well (e.g. PV1_0 = 0.1).
    if let Some((lon_pvs, lat_pvs)) = wcsmap_pv {
        for &(m, val) in lon_pvs {
            if m < store.pv[lon_axis].len() {
                store.pv[lon_axis][m] = Some(val);
            }
        }
        for &(m, val) in lat_pvs {
            if m < store.pv[lat_axis].len() {
                store.pv[lat_axis][m] = Some(val);
            }
        }
    }

    // LONPOLE default for zenithal projections.
    store.lonpole = Some(180.0);

    // MJD-OBS / DATE-OBS gated by C's `defdate` flag
    // (fitschan.c:29721, 29757, 29805):
    //
    //   ep_tdb  = test_epoch ? get_epoch : BAD
    //   ep_utc  = TDBConv(ep_tdb, UTC)
    //   eq_mjd  = astGetEquinox(skyframe)            // SkyFrame
    //                                                // default applies
    //                                                // (B1950 MJD for
    //                                                // FK4*, J2000 MJD
    //                                                // otherwise).
    //   defdate = astEQUAL(ep_utc, eq_mjd)
    //   if (!defdate) SetItem(store->mjdobs, ep_utc)
    //
    // - When the SkyFrame's epoch was never explicitly set,
    //   `sky.test_epoch()` is false: skip emission entirely. C's
    //   ep_tdb is BAD in that case and SetItem(mjdobs, BAD) is a
    //   no-op.
    // - When the explicit epoch *equals* the default equinox MJD
    //   (e.g. an input MJD-OBS=51544.5 for a GAPPT/Galactic frame
    //   whose default Equinox attribute is also J2000.0 → 51544.5),
    //   defdate is true and MJD-OBS is suppressed.
    // - For systems that pin a non-default equinox (FK4 → B1950
    //   MJD), the comparison naturally returns false and an explicit
    //   epoch falls through to MJD-OBS emission.
    if sky.test_epoch() {
        let epoch_tdb = sky.epoch_mjd();
        let epoch_utc = tdb_mjd_to_utc(epoch_tdb);
        let eq_default = sky.equinox_mjd();
        if !ast_equal(epoch_utc, eq_default) {
            store.mjdobs = Some(epoch_utc);
            store.dateobs = Some(mjd_to_date_str(epoch_utc));
        }
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
    // Prefer the SpecFrame's user-facing unit (e.g. "GHz") set by the
    // reader from CUNIT; fall back to the system's default SI unit
    // ("Hz", "m", "m/s") when none was set.
    store.cunit[axis] = Some(
        spec.unit()
            .filter(|u| !u.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| system.specmap_units().to_string()),
    );

    let sor = spec.std_of_rest();
    store.specsys = Some(specsys_string(sor));

    if let Some(restfrq) = spec.rest_freq() {
        store.restfrq = Some(restfrq);
    }

    // VELOSYS: topocentric apparent velocity of the SPECSYS rest
    // frame. Mirrors fitschan.c:30774-30796 — set the source vel to
    // 0 in the main rest frame, then compute the source vel after
    // switching to TPSOR. Needs the SkyFrame source position
    // (passed via `cel_ra_dec_rad`), the observer's geodetic position
    // (defaults to geocenter when OBSGEO is absent), and the epoch.
    Ok(())
}

/// Set `store.velosys` from a SpecFrame plus the celestial source
/// position (in radians). Mirrors C's VELOSYS computation in
/// `SpectralAxes` (fitschan.c:30774-30796): the velocity returned is
/// what the source's Doppler velocity would be in TPSOR if it sits
/// at rest in the SPECSYS rest frame.
fn populate_velosys(
    spec: &crate::frames::SpecFrame,
    src_ra_rad: f64,
    src_dec_rad: f64,
    store: &mut FitsStore,
) {
    use crate::frames::Frame;
    let obs_lon = Frame::obs_lon(spec).unwrap_or(0.0);
    let obs_lat = Frame::obs_lat(spec).unwrap_or(0.0);
    let obs_alt = Frame::obs_alt(spec).unwrap_or(0.0);
    let epoch = spec.epoch_mjd();
    store.velosys = Some(crate::mappings::specmap::velosys_for_sor(
        spec.std_of_rest(),
        src_ra_rad,
        src_dec_rad,
        obs_lon,
        obs_lat,
        obs_alt,
        epoch,
    ));
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

    // WCSAXES{S} — mirrors fitschan.c:36987-37000. Emit when:
    //   1. the store explicitly carries a WCSAXES value (the
    //      MakeIntWorld degenerate-axis case at fitschan.c:22263), OR
    //   2. the input chan's NAXIS differs from `naxis` — including
    //      "no NAXIS at all" (e.g. `.ast` inputs whose chan starts
    //      empty, where the C library treats naxis as -1 and emits).
    let input_naxis = cards.get_int("NAXIS").map(|v| v as usize);
    let emit_wcsaxes = store.wcsaxes.is_some()
        || match input_naxis {
            Some(n) => n != naxis,
            None => true,
        };
    if emit_wcsaxes {
        let key = fmt_global_key("WCSAXES", s);
        cards.set_int(&key, naxis as i64, "Number of WCS axes");
    }

    // WCSNAME{S}: store.wcsname (set by `sky_sys` from the SkyFrame's
    // Domain) takes precedence over the FrameSet's frame_name. When
    // either is present, emit on both primary and alt planes. Mirrors
    // fitschan.c::WcsFromStore lines 37005-37011 — the C library
    // unconditionally emits WCSNAME (with a 1-axis special case to
    // suppress when WCSNAME == CTYPE1).
    let wcsname_to_emit: Option<&str> = store
        .wcsname
        .as_deref()
        .or_else(|| frame_name.filter(|n| !n.is_empty()));
    if let Some(name) = wcsname_to_emit {
        // 1-axis suppression: if naxis == 1 and CTYPE0 equals WCSNAME
        // (case-insensitive), the C library skips the redundant
        // WCSNAME card. Mirrors the `Similar` check at C:37008.
        let suppress = naxis == 1
            && store
                .ctype
                .first()
                .and_then(|c| c.as_deref())
                .is_some_and(|c| c.eq_ignore_ascii_case(name));
        if !suppress {
            cards.set_string(
                &fmt_global_key("WCSNAME", s),
                name,
                "Reference name for the coord. frame",
            );
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
            let comment = store.ctype_com[i]
                .clone()
                .unwrap_or_else(|| format!("Type of co-ordinate on axis {}", i + 1));
            cards.set_string(&key, val, &comment);
        }
    }

    // CD matrix or PC+CDELT.
    // Emitted before CUNIT to match the C library's WcsFromStore order
    // (fitschan.c:36867+: CRPIX, CRVAL, CTYPE, CNAME, CD, CDELT, PC, CUNIT).
    if use_cd {
        write_cd_matrix(store, cards, s)?;
    } else {
        write_pc_cdelt(store, cards, s)?;
    }

    // CUNITi{S}
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = fmt_axis_key("CUNIT", i + 1, s);
            let comment = format!("Units for axis {}", i + 1);
            cards.set_string(&key, val, &comment);
        }
    }

    // Plane-shared cards: only emit on primary plane.
    // Mirrors C library WcsFromStore order (fitschan.c:36867+):
    // CD matrix, then MJD-OBS / DATE-OBS, then RADESYS / EQUINOX.
    //
    // If the input chan supplied MJD-AVG, also preserve it: the C
    // library carries it through via ignore-used semantics, but our
    // sweep would drop it. We emit MJD-AVG before MJD-OBS so the
    // position matches the input (MJD-AVG was originally next to the
    // other time-related cards, before the encoder-added MJD-OBS).
    // Likewise for TIMESYS — the C library doesn't re-emit it but it
    // survives in the input chan.
    if s == ' ' {
        if cards.find("MJD-AVG").is_some()
            && let Some(mjdobs) = store.mjdobs
        {
            cards.set_float("MJD-AVG", mjdobs, "Average MJD of observation");
        }
        if let Some(ts) = cards.get_string("TIMESYS") {
            cards.set_string("TIMESYS", &ts, "Time scale");
        }
        if let Some(mjdobs) = store.mjdobs {
            cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
        }
        // DATE-OBS companion to MJD-OBS. Mirrors fitschan.c:3610-3613.
        if let Some(ref dateobs) = store.dateobs {
            cards.set_string("DATE-OBS", dateobs, "Date of observation");
        }
    }

    // PV{i}_{m} numerical projection parameters. Mirrors WcsFromStore
    // (fitschan.c:37257-37284) — emitted between TIMESYS/PV (if any)
    // and RADESYS/EQUINOX. The store's `pv` is keyed by 0-based axis
    // index, m index; FITS uses 1-based axis numbering.
    for i in 0..naxis {
        for (m, val) in store.pv[i].iter().enumerate() {
            if let Some(v) = val {
                let key = if s == ' ' {
                    format!("PV{}_{m}", i + 1)
                } else {
                    format!("PV{}_{m}{s}", i + 1)
                };
                cards.set_float(&key, *v, "Projection parameter");
            }
        }
    }

    // Ancillary keywords. Emit BEFORE the SIP block so any new
    // cards (VELOSYS, MJD-OBS, ...) land between the per-axis WCS
    // section and the SIP coefficients, matching C's WcsFromStore
    // ordering (fitschan.c:37200-37348).
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
            "[Hz] Rest frequency",
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
    if let Some(velosys) = store.velosys {
        cards.set_float(
            &fmt_global_key("VELOSYS", s),
            velosys,
            "[m/s] Topo. apparent velocity of rest frame",
        );
    }

    // SSYSSRC / ZSOURCE — source-velocity AST extension. Mirrors
    // fitschan.c::WcsFromStore C:37330-37338.
    if let Some(ref ssyssrc) = store.ssyssrc {
        cards.set_string(
            &fmt_global_key("SSYSSRC", s),
            ssyssrc,
            "Standard of rest for source redshift",
        );
    }
    if let Some(zsource) = store.zsource {
        cards.set_float(
            &fmt_global_key("ZSOURCE", s),
            zsource,
            "[] Redshift of source",
        );
    }

    // IMAGFREQ — DSB image-sideband frequency (AST extension, JACH).
    // Mirrors C:37358-37361. Primary-only in the C source (no
    // FormatKey suffix variant exists on this card).
    if s == ' ' {
        if let Some(imagfreq) = store.imagfreq {
            cards.set_float("IMAGFREQ", imagfreq, "[Hz] Image frequency");
        }
    }

    // OBSGEO-X/Y/Z — observer geocentric coordinates. FITS-WCS
    // standard (Calabretta+ 2002 §3.1). Always primary-only — these
    // describe the observatory, not a coord-version property.
    // Mirrors C:37363-37372.
    if s == ' ' {
        if let Some(x) = store.obsgeox {
            cards.set_float("OBSGEO-X", x, "[m] Observatory geocentric X");
        }
        if let Some(y) = store.obsgeoy {
            cards.set_float("OBSGEO-Y", y, "[m] Observatory geocentric Y");
        }
        if let Some(z) = store.obsgeoz {
            cards.set_float("OBSGEO-Z", z, "[m] Observatory geocentric Z");
        }
    }

    // SREFIS / SREF{i} / SREFP{i} — AST-extension SkyFrame
    // offset-coords metadata. Note the AST keyword names (8-char
    // budget: SREFIS / SREF / SREFP) — not SKYREFIS / SKYREF /
    // SKYREFP. Mirrors C:37185-37216.
    if let Some(ref skyrefis) = store.skyrefis {
        cards.set_string(
            &fmt_global_key("SREFIS", s),
            skyrefis,
            "Is SkyRef used as pole or origin?",
        );
    }
    for i in 0..naxis {
        if let Some(v) = store.skyref[i] {
            let key = fmt_axis_key("SREF", i + 1, s);
            let comment = format!("Sky reference position on axis {}", i + 1);
            cards.set_float(&key, v, &comment);
        }
    }
    for i in 0..naxis {
        if let Some(v) = store.skyrefp[i] {
            let key = fmt_axis_key("SREFP", i + 1, s);
            let comment = format!("Sky primary meridian position on axis {}", i + 1);
            cards.set_float(&key, v, &comment);
        }
    }

    // SIP distortion coefficients. Mirrors the SetItem/SetValue loops
    // for `store->asip`/`bsip`/`apsip`/`bpsip` (fitschan.c:28939-28986
    // and the matching writer that converts them to A_*/B_*/AP_*/BP_*
    // cards). Only emit on the primary plane (alt-plane SIP isn't part
    // of the spec).
    if s == ' ' {
        write_sip_block(store.sip_a_order, &store.sip_a, "A", cards);
        write_sip_block(store.sip_b_order, &store.sip_b, "B", cards);
        write_sip_block(store.sip_ap_order, &store.sip_ap, "AP", cards);
        write_sip_block(store.sip_bp_order, &store.sip_bp, "BP", cards);
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

/// Emit one SIP coefficient block (A_*, B_*, AP_*, BP_*).
///
/// Mirrors `SIPIntWorld`'s store loops (fitschan.c:28939-28986): emit
/// `<prefix>_ORDER` only when at least one non-zero coefficient exists,
/// then iterate over the (i, j) grid and emit `<prefix>_i_j` cards
/// for every non-zero entry. The forward block uses comments
/// "SIP forward distortion coeff"; the inverse blocks "SIP inverse
/// distortion coeff" — matching the C library's SIP writer.
fn write_sip_block(order: Option<usize>, coeffs: &[Vec<f64>], prefix: &str, cards: &mut CardList) {
    let Some(order) = order else { return };
    if coeffs.is_empty() {
        return;
    }
    let order_comment = match prefix {
        "A" => "SIP polynomial order axis 1",
        "B" => "SIP polynomial order axis 2",
        "AP" => "SIP inverse polynomial order axis 1",
        "BP" => "SIP inverse polynomial order axis 2",
        _ => "SIP polynomial order",
    };
    let coeff_comment = match prefix {
        "AP" | "BP" => "SIP inverse distortion coeff",
        _ => "SIP forward distortion coeff",
    };
    cards.set_int(&format!("{prefix}_ORDER"), order as i64, order_comment);
    for i in 0..=order {
        let row = match coeffs.get(i) {
            Some(r) => r,
            None => continue,
        };
        for j in 0..=order {
            let val = row.get(j).copied().unwrap_or(0.0);
            if val == 0.0 {
                continue;
            }
            cards.set_float(&format!("{prefix}_{i}_{j}"), val, coeff_comment);
        }
    }
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

/// Difference TDB − TT in seconds at the geocentre.
///
/// SkyFrame.Epoch in the C library is stored on the TDB scale (see
/// fitschan.c:29718 "The epoch is in TDB"). TDB and TT differ by a
/// small periodic offset of amplitude ~1.66 ms — comparable to FITS
/// DATE-OBS millisecond precision, so it cannot be ignored.
///
/// Delegates to [`pal::dtdb_geocentre`], which wraps ERFA's full
/// Fairhead & Bretagnon (1990) series (≤±3 ns absolute, 1950-2050).
/// AST's `timemap.c::Rcc` is the same algorithm.
pub(crate) fn tdb_minus_tt_seconds(mjd: f64) -> f64 {
    pal::dtdb_geocentre(mjd)
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
    dim: &[Option<f64>],
    primary_crvals: &[Option<f64>],
    cards: &CardList,
) -> AstResult<(FitsStore, Vec<AltPlane>)> {
    let naxis = fs.base().naxes();
    let primary = store_from_frameset(fs, dim, primary_crvals)?;
    let assignments = frames_with_version_letters(fs);

    let mut alts = Vec::with_capacity(assignments.len());
    for (frame_idx_1based, letter) in assignments {
        let target_idx = frame_idx_1based - 1;
        // Each alt plane gets its own input CRVALs from CRVAL{i}{S}
        // cards; mirrors the C library re-running ReadCrval per
        // version letter inside FsetToStore.
        let alt_crvals = extract_input_crvals(cards, letter, naxis);
        let alt_store = store_from_frameset_to(fs, target_idx, dim, &alt_crvals)?;
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
    use crate::fitschan::card_list::CardList;

    /// Helper: build a minimal store with the AST-extension fields
    /// populated and emit it via `write_store_to_cards`.
    fn write_extension_keywords(s: char) -> CardList {
        let mut store = FitsStore::new(2);
        // Required for the ID/CTYPE bookkeeping in write_store_to_cards.
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(0.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());

        // AST-extension fields under test.
        store.wcsname = Some("MYDOMAIN".to_string());
        store.obsgeox = Some(1234567.0);
        store.obsgeoy = Some(2345678.0);
        store.obsgeoz = Some(3456789.0);
        store.ssyssrc = Some("LSRK".to_string());
        store.zsource = Some(0.025);
        store.imagfreq = Some(2.3e11);
        store.skyrefis = Some("ORIGIN".to_string());
        store.skyref[0] = Some(1.5);
        store.skyref[1] = Some(0.25);
        store.skyrefp[0] = Some(1.6);
        store.skyrefp[1] = Some(0.5);

        let mut cards = CardList::new();
        write_store_to_cards(&store, &mut cards, false, s, None).expect("write");
        cards
    }

    #[test]
    fn primary_plane_emits_ast_extension_keywords() {
        let cards = write_extension_keywords(' ');

        assert_eq!(cards.get_string("WCSNAME").as_deref(), Some("MYDOMAIN"));
        assert!((cards.get_float("OBSGEO-X").unwrap() - 1234567.0).abs() < 1e-9);
        assert!((cards.get_float("OBSGEO-Y").unwrap() - 2345678.0).abs() < 1e-9);
        assert!((cards.get_float("OBSGEO-Z").unwrap() - 3456789.0).abs() < 1e-9);
        assert_eq!(cards.get_string("SSYSSRC").as_deref(), Some("LSRK"));
        assert!((cards.get_float("ZSOURCE").unwrap() - 0.025).abs() < 1e-12);
        assert!((cards.get_float("IMAGFREQ").unwrap() - 2.3e11).abs() < 1.0);
        assert_eq!(cards.get_string("SREFIS").as_deref(), Some("ORIGIN"));
        assert!((cards.get_float("SREF1").unwrap() - 1.5).abs() < 1e-12);
        assert!((cards.get_float("SREF2").unwrap() - 0.25).abs() < 1e-12);
        assert!((cards.get_float("SREFP1").unwrap() - 1.6).abs() < 1e-12);
        assert!((cards.get_float("SREFP2").unwrap() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn alt_plane_emits_suffixed_versions_and_no_obsgeo() {
        let cards = write_extension_keywords('A');

        // Per-plane keys: suffix 'A'.
        assert_eq!(cards.get_string("WCSNAMEA").as_deref(), Some("MYDOMAIN"));
        assert_eq!(cards.get_string("SSYSSRCA").as_deref(), Some("LSRK"));
        assert!(cards.get_float("ZSOURCEA").is_some());
        assert_eq!(cards.get_string("SREFISA").as_deref(), Some("ORIGIN"));
        assert!(cards.get_float("SREF1A").is_some());
        assert!(cards.get_float("SREF2A").is_some());
        assert!(cards.get_float("SREFP1A").is_some());
        assert!(cards.get_float("SREFP2A").is_some());

        // OBSGEO-{X,Y,Z} and IMAGFREQ are primary-only — not emitted on alt.
        assert!(cards.find("OBSGEO-X").is_none());
        assert!(cards.find("OBSGEO-Y").is_none());
        assert!(cards.find("OBSGEO-Z").is_none());
        assert!(cards.find("IMAGFREQ").is_none());
    }

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
        let cards = crate::fitschan::card_list::CardList::new();
        let (_primary, alts) =
            store_from_frameset_with_alts(&fs, &[], &[], &cards).expect("alt planes");
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

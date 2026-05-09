//! FITS-WCS encoder pipeline.
//!
//! Converts an AST FrameSet into FITS header cards by:
//! 1. Decomposing the FrameSet into a FitsStore (axis identification + linear extraction)
//! 2. Writing the FitsStore contents as FITS keyword cards
//!
//! This mirrors the C library's `FsetToStore()` → `WcsFromStore()` pipeline.

use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::wcs_decompose::{decompose_linear, split_cd};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{
    BasicFrame, CmpFrame, FrameRef, FrameSet, SkyFrame, SkySystem, SpecFrame, StdOfRest,
};
use crate::mappings::{CmpMap, Composition, Mapping, ShiftMap, WcsMap, WcsProjection, WinMap};
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

    // Get the base→current mapping.
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;

    // Find the best reference pixel. For non-linear mappings (e.g. TAN
    // projections), this is the pixel where the mapping is most locally
    // linear — the projection centre. For linear mappings, any pixel works
    // and we fall back to (1,1,...).
    let ref_pixel = find_reference_pixel(mapping.as_ref(), naxis)?;

    // Decompose the mapping at the reference pixel.
    let decomp = decompose_linear(mapping.as_ref(), naxis, &ref_pixel)?;

    // Store CRPIX.
    for i in 0..naxis {
        store.crpix[i] = Some(decomp.crpix[i]);
    }

    // Store the CD matrix.
    for i in 0..naxis {
        for j in 0..naxis {
            store.cd[i][j] = Some(decomp.cd[[i, j]]);
        }
    }

    // Identify the celestial projection from any WcsMap embedded in the
    // pixel→world mapping. None means there is no celestial deprojection
    // step (linear-only or non-celestial frame), in which case the axis-
    // identification path either skips CTYPE for celestial axes or falls
    // back to TAN if a SkyFrame appears with no projection mapping.
    let projection = extract_projection_from_mapping(mapping.as_ref());

    // Identify axis types from the current frame and populate CTYPE/CRVAL.
    identify_axes(fs, &decomp.crval, &mut store, projection)?;

    Ok(store)
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

/// Find the best reference pixel for WCS encoding.
///
/// Extracts the CRPIX from the pixel-to-world mapping structure. For FrameSets
/// constructed from FITS headers, the mapping starts with a WinMap or ShiftMap
/// whose shift encodes -CRPIX. We walk the CmpMap tree to find this shift.
///
/// If the mapping structure doesn't contain a recognizable CRPIX shift, falls
/// back to pixel (1,1,...) as the reference.
fn find_reference_pixel(mapping: &dyn Mapping, naxis: usize) -> AstResult<Vec<f64>> {
    // Try to extract CRPIX from the mapping structure.
    if let Some(crpix) = extract_crpix_from_mapping(mapping, naxis) {
        // Note: the extracted CRPIX may include a 0.5 offset from the
        // GRID→PIXEL conversion in the FrameSet's internal mapping path.
        // We keep it as-is because decompose_linear must evaluate at this
        // exact pixel to get the correct CRVAL.
        return Ok(crpix);
    }

    // Fallback: use pixel (1,1,...).
    Ok(vec![1.0; naxis])
}

/// Walk the mapping tree to find the leading shift that encodes CRPIX.
///
/// For a pixel-to-world mapping constructed by the WCS reader, the structure is:
///   CmpMap(Series): pixel_to_iwc → iwc_to_world
/// where pixel_to_iwc starts with a WinMap(shift=-CRPIX, scale=1).
///
/// We recursively descend through the "first" component of Series CmpMaps
/// until we find a WinMap or ShiftMap at the leaf.
fn extract_crpix_from_mapping(mapping: &dyn Mapping, naxis: usize) -> Option<Vec<f64>> {
    // Check if this mapping is itself a WinMap with unit scales.
    if let Some(wm) = mapping.as_any().downcast_ref::<WinMap>() {
        let shifts = wm.shifts();
        let scales = wm.scales();
        if shifts.len() == naxis && scales.iter().all(|&s| (s - 1.0).abs() < 1e-10) {
            // WinMap with unit scales: forward(pixel) = pixel + shift.
            // At CRPIX: output = CRPIX + shift = 0 → shift = -CRPIX → CRPIX = -shift.
            let crpix: Vec<f64> = shifts.iter().map(|&s| -s).collect();
            return Some(crpix);
        }
        // WinMap with non-unit scales: CRPIX = -shift/scale.
        if shifts.len() == naxis {
            let crpix: Vec<f64> = shifts
                .iter()
                .zip(scales.iter())
                .map(|(&s, &sc)| if sc.abs() > 1e-30 { -s / sc } else { 1.0 })
                .collect();
            return Some(crpix);
        }
    }

    // Check if this is a ShiftMap.
    if let Some(sm) = mapping.as_any().downcast_ref::<ShiftMap>() {
        let shifts = sm.shifts();
        if shifts.len() == naxis {
            // ShiftMap forward(pixel) = pixel + shift → CRPIX = -shift.
            let crpix: Vec<f64> = shifts.iter().map(|&s| -s).collect();
            return Some(crpix);
        }
    }

    // Check if this is a CmpMap in Series composition.
    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        if cmp.composition() == Composition::Series {
            // The first component in a series CmpMap is applied first (to pixel coords).
            // Recursively check it for a shift/WinMap.
            if let Some(crpix) = extract_crpix_from_mapping(cmp.first(), naxis) {
                return Some(crpix);
            }
            // If the first component is itself a CmpMap that didn't yield a result,
            // try the second component in case the structure is different.
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

    // CRVAL in degrees (mapping output is radians for SkyFrame).
    store.crval[lon_axis] = Some(crval[lon_axis].to_degrees());
    store.crval[lat_axis] = Some(crval[lat_axis].to_degrees());

    // NOTE: CD rows for celestial axes are still in radians/pixel and
    // include the projection's cos(δ) and sign-flip baked in by the
    // numerical Jacobian on the full pixel→world mapping. FITS-WCS
    // expects CD/CDELT in degrees of *projected* x — converting from
    // world α requires intercepting the mapping at the IWC step (after
    // the linear pixel→IWC matrix, before the WcsMap), not scaling the
    // numerical Jacobian after the fact. That fix lives at the
    // store_from_frameset / decompose_linear architecture level and is
    // intentionally out of scope here. Until it lands, fits-aips and
    // friends emit celestial CDELT in radians.

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

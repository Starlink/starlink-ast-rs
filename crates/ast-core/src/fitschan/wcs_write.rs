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
use crate::frames::{CmpFrame, FrameRef, FrameSet, SkyFrame, SkySystem, SpecFrame, StdOfRest};
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

/// Decompose a FrameSet into a FitsStore.
fn store_from_frameset(fs: &FrameSet) -> AstResult<FitsStore> {
    let naxis = fs.base().naxes();
    let mut store = FitsStore::new(naxis);

    // Get the base→current mapping.
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;

    // Use pixel 1.0 as reference (FITS first-pixel convention).
    let ref_pixel: Vec<f64> = vec![1.0; naxis];

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

    // Identify axis types from the current frame and populate CTYPE/CRVAL.
    identify_axes(fs, &decomp.crval, &mut store)?;

    Ok(store)
}

/// Identify axis types from the current frame and populate CTYPE, CRVAL, etc.
#[allow(clippy::needless_range_loop)]
fn identify_axes(fs: &FrameSet, crval: &[f64], store: &mut FitsStore) -> AstResult<()> {
    let current = fs.current();
    let naxis = store.naxis;

    if let Some(sky) = current.as_any().downcast_ref::<SkyFrame>() {
        populate_celestial(sky, crval, store, 0, 1)?;
    } else if let Some(spec) = current.as_any().downcast_ref::<SpecFrame>() {
        populate_spectral(spec, crval, store, 0)?;
    } else if let Some(cmp) = current.as_any().downcast_ref::<CmpFrame>() {
        let n1 = cmp.naxes1();
        let frame1 = cmp.frame1();
        let frame2 = cmp.frame2();

        if let Some(sky) = frame1.as_any().downcast_ref::<SkyFrame>() {
            populate_celestial(sky, crval, store, 0, 1)?;
        } else if let Some(spec) = frame1.as_any().downcast_ref::<SpecFrame>() {
            populate_spectral(spec, crval, store, 0)?;
        }

        if let Some(sky) = frame2.as_any().downcast_ref::<SkyFrame>() {
            populate_celestial(sky, crval, store, n1, n1 + 1)?;
        } else if let Some(spec) = frame2.as_any().downcast_ref::<SpecFrame>() {
            populate_spectral(spec, crval, store, n1)?;
        }

        // Remaining axes get generic labels.
        for i in 0..naxis {
            if store.ctype[i].is_none() {
                store.ctype[i] = Some(format!("AXIS{}", i + 1));
                store.crval[i] = Some(crval[i]);
            }
        }
    } else {
        // BasicFrame or unknown: generic linear axes.
        for i in 0..naxis {
            store.ctype[i] = Some(format!("AXIS{}", i + 1));
            store.crval[i] = Some(crval[i]);
        }
    }

    Ok(())
}

/// Populate FitsStore fields for celestial axes.
fn populate_celestial(
    sky: &SkyFrame,
    crval: &[f64],
    store: &mut FitsStore,
    lon_axis: usize,
    lat_axis: usize,
) -> AstResult<()> {
    let system = sky.system();
    let proj_code = "TAN";

    let (lon_prefix, lat_prefix) = ctype_prefixes(system);
    store.ctype[lon_axis] = Some(format!("{lon_prefix}-{proj_code}"));
    store.ctype[lat_axis] = Some(format!("{lat_prefix}-{proj_code}"));

    // CRVAL in degrees (mapping output is radians for SkyFrame).
    store.crval[lon_axis] = Some(crval[lon_axis].to_degrees());
    store.crval[lat_axis] = Some(crval[lat_axis].to_degrees());

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
            cards.set_float(&key, val, "Reference pixel");
        }
    }

    // CRVALi
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let key = format!("CRVAL{}", i + 1);
            cards.set_float(&key, val, "Reference value");
        }
    }

    // CTYPEi
    for i in 0..naxis {
        if let Some(ref val) = store.ctype[i] {
            let key = format!("CTYPE{}", i + 1);
            cards.set_string(&key, val, "Axis type");
        }
    }

    // CUNITi
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = format!("CUNIT{}", i + 1);
            cards.set_string(&key, val, "Axis units");
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
        cards.set_string("RADESYS", radesys, "Reference frame");
    }
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "Equinox of coordinates");
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
                    cards.set_float(&key, val, "");
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

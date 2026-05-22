//! FITS-AIPS and FITS-AIPS++ encoding reader/writer.
//!
//! AIPS-style WCS headers use CROTA (rotation angle in degrees) instead of
//! a PC/CD matrix, and EPOCH instead of EQUINOX. They also use a distinctive
//! spectral CTYPE convention: "VELO-LSR", "FREQ-HEL", "FELO-GEO", etc.
//!
//! FITS-AIPS++ is an extension that allows CDi_j matrix, PV parameters,
//! LONPOLE/LATPOLE, and uses EQUINOX instead of EPOCH.
//!
//! This module provides:
//! - [`read_from_cards`]: extract AIPS/AIPS++ keywords from a CardList into a FitsStore
//! - [`write_to_cards`]: write a FitsStore as plain AIPS keywords into a CardList
//! - [`write_to_cards_pp`]: write a FitsStore as AIPS++ keywords into a CardList

use ndarray::Array2;

use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::guess_encoding::is_aips_spectral;
use super::wcs_decompose::split_cd;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read AIPS/AIPS++ style WCS keywords from a [`CardList`] into a [`FitsStore`].
///
/// This handles both plain AIPS (with CROTA/EPOCH) and AIPS++ (with CDi_j/EQUINOX)
/// headers. The distinguishing feature is the AIPS spectral CTYPE convention.
///
/// Key behaviours:
/// - AIPS spectral CTYPEs (e.g. "VELO-LSR") are translated to standard form
///   (e.g. "VRAD") and the SPECSYS is extracted from the suffix
/// - CROTA2 (or CROTA1 fallback) is converted to a PC rotation matrix
/// - EPOCH is used to set equinox, applying the IAU 1984 rule for RADESYS
/// - CDi_j matrix is also read (for AIPS++ headers)
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    // Determine the number of axes from WCSAXES or NAXIS.
    let wcsaxes = cards.get_int("WCSAXES").map(|v| v as usize);
    let naxis_val = cards.get_int("NAXIS").map(|v| v as usize);

    let naxis = match (wcsaxes, naxis_val) {
        (Some(w), _) => w,
        (None, Some(n)) => n,
        (None, None) => {
            // Fallback: infer from highest-numbered NAXISn keyword.
            let mut max_n = 0;
            for i in 1..=9 {
                let key = format!("NAXIS{i}");
                if cards.find(&key).is_some() {
                    max_n = i;
                }
            }
            if max_n == 0 {
                return Err(AstError::new(
                    ErrorCode::Bdfts,
                    "cannot determine number of WCS axes: neither WCSAXES nor NAXIS found",
                ));
            }
            max_n
        }
    };

    if naxis == 0 {
        return Err(AstError::new(ErrorCode::Bdfts, "NAXIS/WCSAXES is zero"));
    }

    let mut store = FitsStore::new(naxis);
    store.wcsaxes = wcsaxes;

    // Extract per-axis keywords.
    for i in 0..naxis {
        let axis_num = i + 1; // FITS is 1-based

        // CRPIX
        let key = format!("CRPIX{axis_num}");
        store.crpix[i] = cards.get_float(&key);

        // CRVAL
        let key = format!("CRVAL{axis_num}");
        store.crval[i] = cards.get_float(&key);

        // CDELT
        let key = format!("CDELT{axis_num}");
        store.cdelt[i] = cards.get_float(&key);

        // CTYPE — translate AIPS spectral types and extract SPECSYS
        let key = format!("CTYPE{axis_num}");
        if let Some(ctype_raw) = cards.get_string(&key) {
            if is_aips_spectral(&ctype_raw) {
                let (standard_ctype, specsys) = translate_aips_spectral_read(&ctype_raw);
                store.ctype[i] = Some(standard_ctype);
                // Only set SPECSYS if not already set from a previous axis or keyword
                if store.specsys.is_none() {
                    store.specsys = Some(specsys);
                }
            } else {
                store.ctype[i] = Some(ctype_raw);
            }
        }

        // CUNIT
        let key = format!("CUNIT{axis_num}");
        store.cunit[i] = cards.get_string(&key);

        // CDi_j matrix (for AIPS++ headers)
        for j in 0..naxis {
            let col_num = j + 1;
            let key = format!("CD{axis_num}_{col_num}");
            store.cd[i][j] = cards.get_float(&key);
        }
    }

    // CROTA → PC rotation matrix.
    // Prefer CROTA2, fall back to CROTA1.
    let crota = cards
        .get_float("CROTA2")
        .or_else(|| cards.get_float("CROTA1"));
    if let Some(angle_deg) = crota {
        let angle_rad = angle_deg.to_radians();
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();
        // Only set if we have at least 2 axes
        if naxis >= 2 {
            store.pc[0][0] = Some(cos_a);
            store.pc[0][1] = Some(-sin_a);
            store.pc[1][0] = Some(sin_a);
            store.pc[1][1] = Some(cos_a);
        }
    }

    // EPOCH → equinox + RADESYS via IAU 1984 rule
    let epoch = cards.get_float("EPOCH");
    let equinox_direct = cards.get_float("EQUINOX");

    // Prefer EQUINOX if present (AIPS++ style), otherwise use EPOCH
    if let Some(eq) = equinox_direct {
        store.equinox = Some(eq);
    } else if let Some(ep) = epoch {
        // EPOCH=0 is treated as 1950.0 (common AIPS convention)
        let eq = if ep == 0.0 { 1950.0 } else { ep };
        store.equinox = Some(eq);
    }

    // RADESYS/RADECSYS if present (overrides IAU rule)
    store.radesys = cards
        .get_string("RADESYS")
        .or_else(|| cards.get_string("RADECSYS"));

    // If RADESYS not explicitly set, apply IAU 1984 rule based on equinox
    if store.radesys.is_none() {
        if let Some(eq) = store.equinox {
            store.radesys = Some(if eq < 1984.0 {
                "FK4".to_string()
            } else {
                "FK5".to_string()
            });
        }
    }

    // LONPOLE, LATPOLE (AIPS++ may have these)
    store.lonpole = cards.get_float("LONPOLE");
    store.latpole = cards.get_float("LATPOLE");

    // Spectral keywords (explicit SPECSYS overrides the one derived from CTYPE)
    if let Some(specsys) = cards.get_string("SPECSYS") {
        store.specsys = Some(specsys);
    }
    store.restfrq = cards
        .get_float("RESTFRQ")
        .or_else(|| cards.get_float("RESTFREQ"));
    store.restwav = cards.get_float("RESTWAV");

    // MJD-OBS
    store.mjdobs = cards
        .get_float("MJD-OBS")
        .or_else(|| cards.get_float("MJD_OBS"));

    // PV parameters (AIPS++: PVi_m)
    for i in 0..naxis {
        let axis_num = i + 1;
        for m in 0..store.pv[i].len() {
            let key = format!("PV{axis_num}_{m}");
            store.pv[i][m] = cards.get_float(&key);
        }
    }

    Ok(store)
}

/// Write a [`FitsStore`] as plain AIPS-style WCS keywords into a [`CardList`].
///
/// The plain AIPS encoding writes:
/// - CRPIX, CRVAL, CDELT per axis
/// - CTYPE with AIPS spectral convention (e.g. "VELO-LSR")
/// - CROTA2 derived from the PC matrix rotation angle
/// - EPOCH (not EQUINOX)
/// - RESTFREQ if a rest frequency is present
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    // CRPIXi
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = format!("CRPIX{}", i + 1);
            cards.set_float(&key, val, &format!("Reference pixel on axis {}", i + 1));
        }
    }

    // CRVALi
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let key = format!("CRVAL{}", i + 1);
            cards.set_float(&key, val, &format!("Value at ref. pixel on axis {}", i + 1));
        }
    }

    // Plain AIPS does not store a matrix; it uses CDELT and a single
    // CROTAk. Factor the store's CD matrix into (CDELT, PC) when only CD
    // is present, then derive the rotation angle from PC.
    let (cdelt_vals, pc_vals) = derive_cdelt_pc(store);

    // CDELTi — always emitted (one per axis).
    for (i, &cdelt_val) in cdelt_vals.iter().enumerate() {
        let key = format!("CDELT{}", i + 1);
        cards.set_float(&key, cdelt_val, "Pixel size");
    }

    // CTYPEi — convert standard types back to AIPS spectral format using SPECSYS
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let aips_ctype = translate_aips_spectral_write(ctype, store.specsys.as_deref());
            let key = format!("CTYPE{}", i + 1);
            cards.set_string(
                &key,
                &aips_ctype,
                &format!("Type of co-ordinate on axis {}", i + 1),
            );
        }
    }

    // CUNITi
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = format!("CUNIT{}", i + 1);
            cards.set_string(&key, val, &format!("Units for axis {}", i + 1));
        }
    }

    // CROTAk from PC matrix: atan2(PC[1][0], PC[1][1]) in degrees, where k
    // is the latitude axis if the frame is celestial, else axis 1. AIPS
    // emits CROTA always (including 0.0) so downstream readers know the
    // rotation is exactly zero rather than unspecified.
    //
    // Snap rotations smaller than ~1e-9° to exactly zero. The IWC
    // composition for celestial CD reconstructs WcsNative from CRVAL
    // and composes with the FrameSet's existing WcsNative — those two
    // matrices are mathematically identical when the CRVAL extraction
    // round-trips bit-exactly, so they should cancel under
    // `astSimplify`. The C library's simplifier deep-traverses nested
    // CmpMap structures and recognises the internal inverse-pair as
    // identity, leaving no numerical residue. Our `Mapping::simplify`
    // is shallower (it only merges adjacent siblings, not arbitrarily
    // nested chains), so a tiny residue reaches `decompose_linear`
    // and surfaces here as ~1e-11° CROTA. Snapping is correct
    // physically — no astronomical use case distinguishes 0° from
    // 1e-9° — and matches the C library's effective behaviour, even
    // though it gets there by a different (deeper) simplification path.
    if naxis >= 2 {
        let mut crota = pc_vals[[1, 0]].atan2(pc_vals[[1, 1]]).to_degrees();
        if crota.abs() < 1e-9 {
            crota = 0.0;
        }
        let k = crota_axis(store);
        cards.set_float(&format!("CROTA{k}"), crota, "Axis rotation");
    }

    // EPOCH (not EQUINOX for plain AIPS)
    if let Some(equinox) = store.equinox {
        cards.set_float("EPOCH", equinox, "Epoch of reference equinox");
    }

    // RESTFREQ (AIPS uses RESTFREQ, not RESTFRQ)
    if let Some(restfrq) = store.restfrq {
        cards.set_float("RESTFREQ", restfrq, "Rest frequency (Hz)");
    }

    Ok(())
}

/// Derive CDELT and PC values for emission, factoring the store's CD
/// matrix into (CDELT × PC) when CDELT/PC are not stored directly.
///
/// Mirrors the same logic as `enc_pc::write_to_cards`. Returns identity
/// defaults when neither matrix form is populated.
fn derive_cdelt_pc(store: &FitsStore) -> (Vec<f64>, Array2<f64>) {
    let naxis = store.naxis;
    if store.has_pc_matrix() || store.cdelt.iter().any(|v| v.is_some()) {
        let cdelt: Vec<f64> = (0..naxis).map(|i| store.cdelt[i].unwrap_or(1.0)).collect();
        let mut pc = Array2::zeros((naxis, naxis));
        for i in 0..naxis {
            for j in 0..naxis {
                pc[[i, j]] = store.pc[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
            }
        }
        (cdelt, pc)
    } else if store.has_cd_matrix() {
        let mut cd = Array2::zeros((naxis, naxis));
        for i in 0..naxis {
            for j in 0..naxis {
                cd[[i, j]] = store.cd[i][j].unwrap_or(0.0);
            }
        }
        split_cd(&cd)
    } else {
        (vec![1.0; naxis], Array2::eye(naxis))
    }
}

/// Choose the axis index `k` for CROTAk emission.
///
/// Returns the 1-based latitude axis when the store contains a celestial
/// CTYPE pair (`DEC--`, `GLAT-`, `ELAT-`, `SLAT-`, `EL---`); otherwise
/// returns 1. Matches the C library convention: CROTA is paired with the
/// latitude axis for celestial frames and with axis 1 for purely linear
/// frames.
fn crota_axis(store: &FitsStore) -> usize {
    for (i, ctype) in store.ctype.iter().enumerate() {
        if let Some(s) = ctype {
            let prefix = s.get(..4).unwrap_or("");
            if matches!(prefix, "DEC-" | "GLAT" | "ELAT" | "SLAT" | "EL--") {
                return i + 1;
            }
        }
    }
    1
}

/// Write a [`FitsStore`] as AIPS++ style WCS keywords into a [`CardList`].
///
/// The AIPS++ encoding writes:
/// - CRPIX, CRVAL per axis
/// - CTYPE with AIPS spectral convention
/// - CDi_j matrix (if store has CD use it directly, else compute PC x CDELT)
/// - EQUINOX (not EPOCH)
/// - LONPOLE, LATPOLE if present
/// - PV parameters
/// - RESTFREQ, SPECSYS
pub(crate) fn write_to_cards_pp(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    // CRPIXi
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = format!("CRPIX{}", i + 1);
            cards.set_float(&key, val, &format!("Reference pixel on axis {}", i + 1));
        }
    }

    // CRVALi
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let key = format!("CRVAL{}", i + 1);
            cards.set_float(&key, val, &format!("Value at ref. pixel on axis {}", i + 1));
        }
    }

    // CTYPEi — AIPS spectral convention
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let aips_ctype = translate_aips_spectral_write(ctype, store.specsys.as_deref());
            let key = format!("CTYPE{}", i + 1);
            cards.set_string(
                &key,
                &aips_ctype,
                &format!("Type of co-ordinate on axis {}", i + 1),
            );
        }
    }

    // CUNITi
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = format!("CUNIT{}", i + 1);
            cards.set_string(&key, val, &format!("Units for axis {}", i + 1));
        }
    }

    // CDi_j matrix: use CD directly if available, otherwise compute from PC x CDELT
    write_aipspp_cd_matrix(store, cards);

    // EQUINOX (not EPOCH for AIPS++)
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "Epoch of reference equinox");
    }

    // RADESYS
    if let Some(ref radesys) = store.radesys {
        cards.set_string("RADESYS", radesys, "Reference frame for RA/DEC values");
    }

    // LONPOLE: only emit when non-default. 180° is the FITS-WCS Paper II
    // default for zenithal projections; the C library suppresses it.
    if let Some(lonpole) = store.lonpole {
        if lonpole != 180.0 {
            cards.set_float("LONPOLE", lonpole, "Native longitude of celestial pole");
        }
    }
    if let Some(latpole) = store.latpole {
        cards.set_float("LATPOLE", latpole, "Native latitude of celestial pole");
    }

    // PV parameters
    for i in 0..naxis {
        let axis_num = i + 1;
        for (m, pv_val) in store.pv[i].iter().enumerate() {
            if let Some(val) = pv_val {
                let key = format!("PV{axis_num}_{m}");
                cards.set_float(&key, *val, "");
            }
        }
    }

    // RESTFREQ
    if let Some(restfrq) = store.restfrq {
        cards.set_float("RESTFREQ", restfrq, "Rest frequency (Hz)");
    }

    // SPECSYS
    if let Some(ref specsys) = store.specsys {
        cards.set_string("SPECSYS", specsys, "Spectral reference frame");
    }

    // MJD-OBS
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Write the CD matrix for AIPS++.
///
/// If the store has CD values directly, use them. Otherwise compute
/// CD[i][j] = PC[i][j] x CDELT[i].
fn write_aipspp_cd_matrix(store: &FitsStore, cards: &mut CardList) {
    let naxis = store.naxis;

    if store.has_cd_matrix() {
        // Write CD values directly from the store.
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
    } else {
        // Compute CD from PC x CDELT.
        for i in 0..naxis {
            let cdelt_i = store.cdelt[i].unwrap_or(1.0);
            for j in 0..naxis {
                let pc_ij = store.pc[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
                let cd_val = pc_ij * cdelt_i;
                if cd_val != 0.0 || i == j {
                    let key = format!("CD{}_{}", i + 1, j + 1);
                    cards.set_float(&key, cd_val, "Transformation matrix element");
                }
            }
        }
    }
}

/// Translate an AIPS spectral CTYPE to standard form, returning (standard_ctype, specsys).
///
/// For example: "VELO-LSR" -> ("VRAD", "LSRK")
fn translate_aips_spectral_read(ctype: &str) -> (String, String) {
    let prefix = &ctype[..4];
    let frame = &ctype[5..8];

    let standard_ctype = match prefix {
        "VELO" => "VRAD",
        "FREQ" => "FREQ",
        "FELO" => "VOPT",
        _ => "VRAD", // fallback
    };

    let specsys = match frame {
        "LSR" => "LSRK",
        "LSD" => "LSRD",
        "HEL" => "BARYCENT",
        "GEO" => "GEOCENTR",
        "OBS" | "TOP" | "EAR" => "TOPOCENT",
        _ => "LSRK", // fallback
    };

    (standard_ctype.to_string(), specsys.to_string())
}

/// Translate a standard spectral CTYPE back to AIPS format using the given SPECSYS.
///
/// For example: ("VRAD", Some("LSRK")) -> "VELO-LSR"
///
/// If the CTYPE is not a spectral type that maps to an AIPS convention, it is
/// returned unchanged.
fn translate_aips_spectral_write(ctype: &str, specsys: Option<&str>) -> String {
    let aips_prefix = match ctype {
        "VRAD" => Some("VELO"),
        "FREQ" => Some("FREQ"),
        "VOPT" => Some("FELO"),
        _ => None,
    };

    let Some(prefix) = aips_prefix else {
        return ctype.to_string();
    };

    let frame = match specsys {
        Some("LSRK") => "LSR",
        Some("LSRD") => "LSD",
        Some("BARYCENT") => "HEL",
        Some("GEOCENTR") => "GEO",
        Some("TOPOCENT") => "OBS",
        _ => "LSR", // default to LSR if SPECSYS not specified
    };

    format!("{prefix}-{frame}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_aips_crota() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CRVAL1  =          180.000000  / Reference value
CRVAL2  =          -45.000000  / Reference value
CDELT1  =        -0.0002778    / Pixel scale
CDELT2  =         0.0002778    / Pixel scale
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CROTA2  =               30.0   / Rotation angle
EPOCH   =             2000.0   / Epoch
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.naxis, 2);

        // CROTA2=30 degrees -> PC matrix with cos(30)/sin(30)
        let cos30 = 30.0_f64.to_radians().cos();
        let sin30 = 30.0_f64.to_radians().sin();

        let pc00 = store.pc[0][0].unwrap();
        let pc01 = store.pc[0][1].unwrap();
        let pc10 = store.pc[1][0].unwrap();
        let pc11 = store.pc[1][1].unwrap();

        assert!((pc00 - cos30).abs() < 1e-10, "PC[0][0] = cos(30)");
        assert!((pc01 - (-sin30)).abs() < 1e-10, "PC[0][1] = -sin(30)");
        assert!((pc10 - sin30).abs() < 1e-10, "PC[1][0] = sin(30)");
        assert!((pc11 - cos30).abs() < 1e-10, "PC[1][1] = cos(30)");
    }

    #[test]
    fn read_aips_epoch_fk5() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CRVAL1  =          180.000000  / Reference value
CRVAL2  =          -45.000000  / Reference value
CDELT1  =        -0.0002778    / Pixel scale
CDELT2  =         0.0002778    / Pixel scale
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
EPOCH   =             2000.0   / Epoch
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // EPOCH=2000 >= 1984 -> FK5
        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.radesys, Some("FK5".to_string()));
    }

    #[test]
    fn read_aips_epoch_fk4() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CRVAL1  =          180.000000  / Reference value
CRVAL2  =          -45.000000  / Reference value
CDELT1  =        -0.0002778    / Pixel scale
CDELT2  =         0.0002778    / Pixel scale
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
EPOCH   =             1950.0   / Epoch
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // EPOCH=1950 < 1984 -> FK4
        assert_eq!(store.equinox, Some(1950.0));
        assert_eq!(store.radesys, Some("FK4".to_string()));
    }

    #[test]
    fn read_aips_spectral_velo_lsr() {
        let header = "\
NAXIS   =                    1 / Number of axes
CRPIX1  =                1.0   / Reference pixel
CRVAL1  =            10000.0   / Reference value
CDELT1  =             1000.0   / Channel width
CTYPE1  = 'VELO-LSR'           / Axis type
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // VELO-LSR -> VRAD + LSRK
        assert_eq!(store.ctype[0], Some("VRAD".to_string()));
        assert_eq!(store.specsys, Some("LSRK".to_string()));
    }

    #[test]
    fn read_aips_spectral_freq_hel() {
        let header = "\
NAXIS   =                    1 / Number of axes
CRPIX1  =                1.0   / Reference pixel
CRVAL1  =         1.4e9        / Reference value
CDELT1  =         1.0e6        / Channel width
CTYPE1  = 'FREQ-HEL'           / Axis type
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // FREQ-HEL -> FREQ + BARYCENT
        assert_eq!(store.ctype[0], Some("FREQ".to_string()));
        assert_eq!(store.specsys, Some("BARYCENT".to_string()));
    }

    #[test]
    fn read_aips_spectral_felo_geo() {
        let header = "\
NAXIS   =                    1 / Number of axes
CRPIX1  =                1.0   / Reference pixel
CRVAL1  =            5000.0    / Reference value
CDELT1  =              500.0   / Channel width
CTYPE1  = 'FELO-GEO'           / Axis type
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // FELO-GEO -> VOPT + GEOCENTR
        assert_eq!(store.ctype[0], Some("VOPT".to_string()));
        assert_eq!(store.specsys, Some("GEOCENTR".to_string()));
    }

    #[test]
    fn read_aips_epoch_zero_treated_as_1950() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CRVAL1  =          180.000000  / Reference value
CRVAL2  =          -45.000000  / Reference value
CDELT1  =        -0.0002778    / Pixel scale
CDELT2  =         0.0002778    / Pixel scale
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
EPOCH   =                0.0   / Epoch
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // EPOCH=0 treated as 1950 -> FK4
        assert_eq!(store.equinox, Some(1950.0));
        assert_eq!(store.radesys, Some("FK4".to_string()));
    }

    #[test]
    fn read_aips_explicit_radesys_overrides() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CRVAL1  =          180.000000  / Reference value
CRVAL2  =          -45.000000  / Reference value
CDELT1  =        -0.0002778    / Pixel scale
CDELT2  =         0.0002778    / Pixel scale
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
EPOCH   =             2000.0   / Epoch
RADESYS = 'ICRS'               / Reference frame
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // Explicit RADESYS overrides the IAU 1984 rule
        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.radesys, Some("ICRS".to_string()));
    }

    #[test]
    fn write_aips_crota() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(512.0);
        store.crpix[1] = Some(512.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-45.0);
        store.cdelt[0] = Some(-0.0002778);
        store.cdelt[1] = Some(0.0002778);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        // Rotation by 30 degrees
        let angle = 30.0_f64.to_radians();
        store.pc[0][0] = Some(angle.cos());
        store.pc[0][1] = Some(-angle.sin());
        store.pc[1][0] = Some(angle.sin());
        store.pc[1][1] = Some(angle.cos());
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // CROTA2 should be approximately 30 degrees
        let crota2 = cards.get_float("CROTA2").unwrap();
        assert!(
            (crota2 - 30.0).abs() < 1e-10,
            "CROTA2 should be ~30, got {crota2}"
        );

        // Should NOT have PC or CD keywords
        assert!(cards.get_float("PC1_1").is_none());
        assert!(cards.get_float("CD1_1").is_none());
    }

    #[test]
    fn write_aips_epoch() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        store.crval[0] = Some(45.0);
        store.crval[1] = Some(60.0);
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // Should write EPOCH (not EQUINOX)
        assert_eq!(cards.get_float("EPOCH"), Some(2000.0));
        assert!(cards.get_float("EQUINOX").is_none());
    }

    #[test]
    fn write_aips_spectral() {
        let mut store = FitsStore::new(1);
        store.crpix[0] = Some(1.0);
        store.crval[0] = Some(10000.0);
        store.cdelt[0] = Some(1000.0);
        store.ctype[0] = Some("VRAD".to_string());
        store.specsys = Some("LSRK".to_string());

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // VRAD + LSRK -> VELO-LSR
        assert_eq!(cards.get_string("CTYPE1"), Some("VELO-LSR".to_string()));
    }

    #[test]
    fn write_aips_spectral_freq_barycent() {
        let mut store = FitsStore::new(1);
        store.crpix[0] = Some(1.0);
        store.crval[0] = Some(1.4e9);
        store.cdelt[0] = Some(1.0e6);
        store.ctype[0] = Some("FREQ".to_string());
        store.specsys = Some("BARYCENT".to_string());

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // FREQ + BARYCENT -> FREQ-HEL
        assert_eq!(cards.get_string("CTYPE1"), Some("FREQ-HEL".to_string()));
    }

    #[test]
    fn write_aipspp_cd_matrix() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(256.0);
        store.crpix[1] = Some(256.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-30.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.cd[0][0] = Some(-0.0005);
        store.cd[1][1] = Some(0.0005);
        store.equinox = Some(2000.0);
        store.radesys = Some("FK5".to_string());

        let mut cards = CardList::new();
        write_to_cards_pp(&store, &mut cards).unwrap();

        // Should have CDi_j keywords
        assert_eq!(cards.get_float("CD1_1"), Some(-0.0005));
        assert_eq!(cards.get_float("CD2_2"), Some(0.0005));
        // Should use EQUINOX (not EPOCH)
        assert_eq!(cards.get_float("EQUINOX"), Some(2000.0));
        assert!(cards.get_float("EPOCH").is_none());
        // Should have RADESYS
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
        // Should NOT have CDELT or CROTA
        assert!(cards.get_float("CDELT1").is_none());
        assert!(cards.get_float("CROTA2").is_none());
    }

    #[test]
    fn write_aipspp_computes_cd_from_pc() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        store.crval[0] = Some(45.0);
        store.crval[1] = Some(60.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        // PC rotation by 30 degrees
        store.pc[0][0] = Some(0.866025); // cos(30)
        store.pc[0][1] = Some(-0.5); // -sin(30)
        store.pc[1][0] = Some(0.5); // sin(30)
        store.pc[1][1] = Some(0.866025); // cos(30)
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards_pp(&store, &mut cards).unwrap();

        // CD = PC x CDELT: CD[i][j] = PC[i][j] x CDELT[i]
        let cd11 = cards.get_float("CD1_1").unwrap();
        let cd12 = cards.get_float("CD1_2").unwrap();
        let cd21 = cards.get_float("CD2_1").unwrap();
        let cd22 = cards.get_float("CD2_2").unwrap();

        // CD1_1 = PC1_1 * CDELT1 = 0.866025 * -0.001
        assert!((cd11 - (-0.000866025)).abs() < 1e-10);
        // CD1_2 = PC1_2 * CDELT1 = -0.5 * -0.001
        assert!((cd12 - 0.0005).abs() < 1e-10);
        // CD2_1 = PC2_1 * CDELT2 = 0.5 * 0.001
        assert!((cd21 - 0.0005).abs() < 1e-10);
        // CD2_2 = PC2_2 * CDELT2 = 0.866025 * 0.001
        assert!((cd22 - 0.000866025).abs() < 1e-10);

        // Should use EQUINOX (not EPOCH)
        assert_eq!(cards.get_float("EQUINOX"), Some(2000.0));
        assert!(cards.get_float("EPOCH").is_none());
    }

    #[test]
    fn write_aipspp_with_lonpole_latpole() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(256.0);
        store.crpix[1] = Some(256.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-30.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.cd[0][0] = Some(-0.001);
        store.cd[1][1] = Some(0.001);
        // 180° is the FITS-WCS default for zenithal projections; suppress it.
        store.lonpole = Some(180.0);
        store.latpole = Some(-30.0);

        let mut cards = CardList::new();
        write_to_cards_pp(&store, &mut cards).unwrap();

        // Default LONPOLE (180°) must NOT be emitted — matches C library.
        assert_eq!(cards.get_float("LONPOLE"), None);
        // Non-default LATPOLE must be emitted.
        assert_eq!(cards.get_float("LATPOLE"), Some(-30.0));
    }

    #[test]
    fn write_aipspp_non_default_lonpole_emitted() {
        // A non-180 LONPOLE is genuinely non-default and must be emitted.
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(256.0);
        store.crpix[1] = Some(256.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-30.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.cd[0][0] = Some(-0.001);
        store.cd[1][1] = Some(0.001);
        store.lonpole = Some(45.0);

        let mut cards = CardList::new();
        write_to_cards_pp(&store, &mut cards).unwrap();

        assert_eq!(cards.get_float("LONPOLE"), Some(45.0));
    }

    #[test]
    fn write_aipspp_spectral_with_specsys() {
        let mut store = FitsStore::new(3);
        store.crpix[0] = Some(512.0);
        store.crpix[1] = Some(512.0);
        store.crpix[2] = Some(1.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-30.0);
        store.crval[2] = Some(10000.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("VRAD".to_string());
        store.cd[0][0] = Some(-0.001);
        store.cd[1][1] = Some(0.001);
        store.cd[2][2] = Some(1000.0);
        store.specsys = Some("LSRK".to_string());

        let mut cards = CardList::new();
        write_to_cards_pp(&store, &mut cards).unwrap();

        // Spectral axis should be written in AIPS format
        assert_eq!(cards.get_string("CTYPE3"), Some("VELO-LSR".to_string()));
        // SPECSYS should also be written explicitly
        assert_eq!(cards.get_string("SPECSYS"), Some("LSRK".to_string()));
    }

    #[test]
    fn read_aips_crota1_fallback() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CRVAL1  =          180.000000  / Reference value
CRVAL2  =          -45.000000  / Reference value
CDELT1  =        -0.0002778    / Pixel scale
CDELT2  =         0.0002778    / Pixel scale
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CROTA1  =               45.0   / Rotation angle
EPOCH   =             2000.0   / Epoch
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // CROTA1=45 fallback when CROTA2 is absent
        let cos45 = 45.0_f64.to_radians().cos();
        let sin45 = 45.0_f64.to_radians().sin();

        let pc00 = store.pc[0][0].unwrap();
        let pc01 = store.pc[0][1].unwrap();
        let pc10 = store.pc[1][0].unwrap();
        let pc11 = store.pc[1][1].unwrap();

        assert!((pc00 - cos45).abs() < 1e-10);
        assert!((pc01 - (-sin45)).abs() < 1e-10);
        assert!((pc10 - sin45).abs() < 1e-10);
        assert!((pc11 - cos45).abs() < 1e-10);
    }

    #[test]
    fn read_aipspp_cd_with_equinox() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              256.0   / Reference pixel
CRPIX2  =              256.0   / Reference pixel
CRVAL1  =          180.000000  / Reference value
CRVAL2  =          -30.000000  / Reference value
CTYPE1  = 'VELO-LSR'           / Axis type (AIPS spectral)
CTYPE2  = 'DEC--TAN'           / Axis type
CD1_1   =           1000.0     / CD matrix
CD2_2   =            0.001     / CD matrix
EQUINOX =             2000.0   / Equinox
LONPOLE =              180.0   / Native longitude of pole
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // Should read CD matrix
        assert_eq!(store.cd[0][0], Some(1000.0));
        assert_eq!(store.cd[1][1], Some(0.001));
        // Should use EQUINOX directly
        assert_eq!(store.equinox, Some(2000.0));
        // Should read LONPOLE
        assert_eq!(store.lonpole, Some(180.0));
        // Should translate spectral CTYPE
        assert_eq!(store.ctype[0], Some("VRAD".to_string()));
        assert_eq!(store.specsys, Some("LSRK".to_string()));
    }
}

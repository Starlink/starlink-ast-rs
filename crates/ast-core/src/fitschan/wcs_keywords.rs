//! Extraction of FITS-WCS keywords from a CardList into a FitsStore.
//!
//! This module implements the first phase of the FITS-WCS decoding pipeline:
//! reading individual FITS keywords from the header cards and populating the
//! intermediate [`FitsStore`] structure.

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Extract FITS-WCS keywords from a [`CardList`] into a [`FitsStore`].
///
/// The returned store contains all WCS keyword values found in the header.
/// Missing keywords are represented as `None`. The function determines
/// the number of WCS axes from `WCSAXES` if present, otherwise from `NAXIS`.
///
/// If neither `WCSAXES` nor `NAXIS` is found, returns an error.
pub(crate) fn extract_wcs_keywords(cards: &CardList) -> AstResult<FitsStore> {
    // Determine the number of axes.
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
        let axis_num = i + 1; // FITS 1-based

        // CRPIX
        let key = format!("CRPIX{axis_num}");
        store.crpix[i] = cards.get_float(&key);

        // CRVAL
        let key = format!("CRVAL{axis_num}");
        store.crval[i] = cards.get_float(&key);

        // CDELT
        let key = format!("CDELT{axis_num}");
        store.cdelt[i] = cards.get_float(&key);

        // CTYPE
        let key = format!("CTYPE{axis_num}");
        store.ctype[i] = cards.get_string(&key);

        // CUNIT
        let key = format!("CUNIT{axis_num}");
        store.cunit[i] = cards.get_string(&key);

        // CNAME
        let key = format!("CNAME{axis_num}");
        store.cname[i] = cards.get_string(&key);

        // CD matrix row i
        for j in 0..naxis {
            let key = format!("CD{}_{}", axis_num, j + 1);
            store.cd[i][j] = cards.get_float(&key);
        }

        // PC matrix row i
        for j in 0..naxis {
            let key = format!("PC{}_{}", axis_num, j + 1);
            store.pc[i][j] = cards.get_float(&key);
        }

        // PV parameters
        for m in 0..store.pv[i].len() {
            let key = format!("PV{}_{m}", axis_num);
            store.pv[i][m] = cards.get_float(&key);
        }
    }

    // Scalar keywords.
    store.radesys = cards.get_string("RADESYS");
    store.equinox = cards.get_float("EQUINOX");
    store.lonpole = cards.get_float("LONPOLE");
    store.latpole = cards.get_float("LATPOLE");
    store.mjdobs = cards
        .get_float("MJD-OBS")
        .or_else(|| cards.get_float("MJD_OBS"));
    store.specsys = cards.get_string("SPECSYS");
    store.restfrq = cards.get_float("RESTFRQ");
    store.restwav = cards.get_float("RESTWAV");

    // Handle SIP distortion: strip "-SIP" from CTYPE and extract coefficients.
    extract_sip(cards, &mut store);

    Ok(store)
}

/// Check for SIP distortion and extract coefficients.
///
/// If CTYPE values end with "-SIP", the suffix is stripped and the SIP
/// polynomial coefficients are read from the header.
fn extract_sip(cards: &CardList, store: &mut FitsStore) {
    // Check if CTYPE values contain "-SIP".
    let has_sip = store.ctype.iter().take(2).any(|ct| {
        ct.as_ref()
            .is_some_and(|s| s.to_uppercase().ends_with("-SIP"))
    });

    if !has_sip {
        return;
    }

    // Strip "-SIP" from CTYPE values (first two axes only, per convention).
    for s in store.ctype.iter_mut().take(2).flatten() {
        let upper = s.to_uppercase();
        if upper.ends_with("-SIP") {
            let new_len = s.len() - 4;
            s.truncate(new_len);
        }
    }

    // Forward A coefficients.
    if let Some(order) = cards.get_int("A_ORDER") {
        let order = order as usize;
        store.sip_a_order = Some(order);
        let size = order + 1;
        store.sip_a = vec![vec![0.0; size]; size];
        for i in 0..size {
            for j in 0..size {
                if i + j <= order {
                    let key = format!("A_{i}_{j}");
                    if let Some(val) = cards.get_float(&key) {
                        store.sip_a[i][j] = val;
                    }
                }
            }
        }
    }

    // Forward B coefficients.
    if let Some(order) = cards.get_int("B_ORDER") {
        let order = order as usize;
        store.sip_b_order = Some(order);
        let size = order + 1;
        store.sip_b = vec![vec![0.0; size]; size];
        for i in 0..size {
            for j in 0..size {
                if i + j <= order {
                    let key = format!("B_{i}_{j}");
                    if let Some(val) = cards.get_float(&key) {
                        store.sip_b[i][j] = val;
                    }
                }
            }
        }
    }

    // Inverse AP coefficients.
    if let Some(order) = cards.get_int("AP_ORDER") {
        let order = order as usize;
        store.sip_ap_order = Some(order);
        let size = order + 1;
        store.sip_ap = vec![vec![0.0; size]; size];
        for i in 0..size {
            for j in 0..size {
                if i + j <= order {
                    let key = format!("AP_{i}_{j}");
                    if let Some(val) = cards.get_float(&key) {
                        store.sip_ap[i][j] = val;
                    }
                }
            }
        }
    }

    // Inverse BP coefficients.
    if let Some(order) = cards.get_int("BP_ORDER") {
        let order = order as usize;
        store.sip_bp_order = Some(order);
        let size = order + 1;
        store.sip_bp = vec![vec![0.0; size]; size];
        for i in 0..size {
            for j in 0..size {
                if i + j <= order {
                    let key = format!("BP_{i}_{j}");
                    if let Some(val) = cards.get_float(&key) {
                        store.sip_bp[i][j] = val;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a CardList from a multi-line header string.
    fn cards_from(text: &str) -> CardList {
        CardList::from_header(text).unwrap()
    }

    #[test]
    fn minimal_header() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =                128.0 / Reference pixel axis 1
CRPIX2  =                256.0 / Reference pixel axis 2
CRVAL1  =              180.000 / Reference value axis 1
CRVAL2  =              -45.000 / Reference value axis 2
CDELT1  =            -0.001000 / Pixel scale axis 1
CDELT2  =             0.001000 / Pixel scale axis 2
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
END";
        let cards = cards_from(header);
        let store = extract_wcs_keywords(&cards).unwrap();

        assert_eq!(store.naxis, 2);
        assert_eq!(store.crpix[0], Some(128.0));
        assert_eq!(store.crpix[1], Some(256.0));
        assert_eq!(store.crval[0], Some(180.0));
        assert_eq!(store.crval[1], Some(-45.0));
        assert!((store.cdelt[0].unwrap() - (-0.001)).abs() < 1e-10);
        assert!((store.cdelt[1].unwrap() - 0.001).abs() < 1e-10);
        assert_eq!(store.ctype[0], Some("RA---TAN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--TAN".to_string()));
        assert!(!store.has_cd_matrix());
        assert!(!store.has_pc_matrix());
        assert!(!store.has_sip());
    }

    #[test]
    fn cd_matrix_header() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =                128.0 / Reference pixel
CRPIX2  =                128.0 / Reference pixel
CRVAL1  =              202.000 / Reference value
CRVAL2  =               47.000 / Reference value
CD1_1   =          0.000250000 / CD matrix element
CD1_2   =          0.000230000 / CD matrix element
CD2_1   =          0.000230000 / CD matrix element
CD2_2   =         -0.000250000 / CD matrix element
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
END";
        let cards = cards_from(header);
        let store = extract_wcs_keywords(&cards).unwrap();

        assert!(store.has_cd_matrix());
        assert!(!store.has_pc_matrix());
        assert!((store.cd[0][0].unwrap() - 0.000250).abs() < 1e-10);
        assert!((store.cd[0][1].unwrap() - 0.000230).abs() < 1e-10);
        assert!((store.cd[1][0].unwrap() - 0.000230).abs() < 1e-10);
        assert!((store.cd[1][1].unwrap() - (-0.000250)).abs() < 1e-10);
    }

    #[test]
    fn wcsaxes_overrides_naxis() {
        let header = "\
NAXIS   =                    4 / Physical axes
WCSAXES =                    2 / WCS axes
CRPIX1  =                  1.0
CRPIX2  =                  1.0
END";
        let cards = cards_from(header);
        let store = extract_wcs_keywords(&cards).unwrap();
        assert_eq!(store.naxis, 2);
        assert_eq!(store.wcsaxes, Some(2));
    }

    #[test]
    fn sip_header_extraction() {
        // Use the fixture.
        let sip_header = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/fits/headers/sip.head"
        ))
        .unwrap();
        let cards = cards_from(&sip_header);
        let store = extract_wcs_keywords(&cards).unwrap();

        // Verify basic keywords.
        assert_eq!(store.naxis, 2);
        assert_eq!(store.crpix[0], Some(128.0));
        assert_eq!(store.crpix[1], Some(128.0));
        assert!((store.crval[0].unwrap() - 202.482322805429).abs() < 1e-9);
        assert!((store.crval[1].unwrap() - 47.1751189300101).abs() < 1e-9);

        // SIP suffix should be stripped from CTYPE.
        assert_eq!(store.ctype[0], Some("RA---TAN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--TAN".to_string()));

        // SIP should be detected.
        assert!(store.has_sip());
        assert_eq!(store.sip_a_order, Some(3));
        assert_eq!(store.sip_b_order, Some(3));
        assert_eq!(store.sip_ap_order, Some(3));
        assert_eq!(store.sip_bp_order, Some(3));

        // Check a few SIP coefficients.
        assert!((store.sip_a[0][2] - 2.9656e-06).abs() < 1e-12);
        assert!((store.sip_a[2][0] - (-2.3863e-05)).abs() < 1e-12);
        assert!((store.sip_b[0][2] - 2.31e-05).abs() < 1e-12);

        // CD matrix.
        assert!(store.has_cd_matrix());
        assert!((store.cd[0][0].unwrap() - 0.000249756880272355).abs() < 1e-15);

        // Scalar keywords.
        assert_eq!(store.radesys, Some("ICRS".to_string()));
        assert_eq!(store.equinox, Some(2000.0));
        assert!((store.mjdobs.unwrap() - 54280.135805).abs() < 1e-6);
    }

    #[test]
    fn no_naxis_returns_error() {
        let header = "SIMPLE  =                    T / Standard\nEND";
        let cards = cards_from(header);
        let result = extract_wcs_keywords(&cards);
        assert!(result.is_err());
    }

    #[test]
    fn pv_parameters() {
        let header = "\
NAXIS   =                    2
CRPIX1  =                  1.0
CRPIX2  =                  1.0
PV2_1   =                  0.0
PV2_2   =                  0.0
END";
        let cards = cards_from(header);
        let store = extract_wcs_keywords(&cards).unwrap();
        assert_eq!(store.pv[1][1], Some(0.0));
        assert_eq!(store.pv[1][2], Some(0.0));
        assert_eq!(store.pv[0][0], None);
    }
}

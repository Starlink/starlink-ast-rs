//! FITS-CLASS encoding reader/writer.
//!
//! CLASS is the data format used by the GILDAS/CLASS radio spectral survey
//! software from Grenoble. Key features of this encoding:
//!
//! - Enforces 3 axes: 2 spatial + 1 spectral (FREQ)
//! - Spectral CRVAL is stored as an **offset** from RESTFREQ, not an absolute
//!   frequency: `absolute_freq = CRVAL + RESTFREQ`
//! - Requires DELTAV (velocity channel spacing in m/s) and VELO-LSR/VLSR
//! - Older projection codes: GLS (= SFL), ATF (= AIT)
//! - SPECSYS is typically "SOURCE" (source rest frame)
//!
//! This module provides:
//! - [`read_from_cards`]: extract CLASS keywords from a CardList into a FitsStore
//! - [`write_to_cards`]: write a FitsStore as CLASS keywords into a CardList

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Speed of light in m/s (IAU definition).
const C_LIGHT: f64 = 299_792_458.0;

/// Read CLASS-style WCS keywords from a [`CardList`] into a [`FitsStore`].
///
/// Key behaviours:
/// - Older projection codes GLS and ATF are translated to SFL and AIT
/// - The spectral CRVAL is converted from offset to absolute frequency:
///   `store.crval[spec] = header_CRVAL + RESTFREQ`
/// - SPECSYS is derived from the velocity keyword present in the header
/// - RADESYS/RADECSYS and EQUINOX are read directly
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    // Determine the number of axes from NAXIS.
    let naxis = cards.get_int("NAXIS").map(|v| v as usize).ok_or_else(|| {
        AstError::new(ErrorCode::Bdfts, "NAXIS keyword not found in CLASS header")
    })?;

    if naxis == 0 {
        return Err(AstError::new(ErrorCode::Bdfts, "NAXIS is zero"));
    }

    let mut store = FitsStore::new(naxis);

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

        // CTYPE — translate CLASS-specific projection codes
        let key = format!("CTYPE{axis_num}");
        if let Some(ctype_raw) = cards.get_string(&key) {
            store.ctype[i] = Some(translate_class_projection_read(&ctype_raw));
        }

        // CUNIT
        let key = format!("CUNIT{axis_num}");
        store.cunit[i] = cards.get_string(&key);
    }

    // Read RESTFREQ (or RESTFRQ) — required for offset correction.
    let restfreq = cards
        .get_float("RESTFREQ")
        .or_else(|| cards.get_float("RESTFRQ"));
    store.restfrq = restfreq;

    // Find the spectral axis and apply offset correction.
    if let Some(spec_idx) = store.spectral_axis() {
        if let (Some(crval), Some(rf)) = (store.crval[spec_idx], restfreq) {
            // CLASS convention: header CRVAL is offset from RESTFREQ
            store.crval[spec_idx] = Some(crval + rf);
        }
    }

    // Determine SPECSYS from velocity keywords.
    store.specsys = determine_specsys_from_velocity(cards);

    // RADESYS / RADECSYS
    store.radesys = cards
        .get_string("RADESYS")
        .or_else(|| cards.get_string("RADECSYS"));

    // EQUINOX
    store.equinox = cards.get_float("EQUINOX");

    // MJD-OBS
    store.mjdobs = cards
        .get_float("MJD-OBS")
        .or_else(|| cards.get_float("MJD_OBS"));

    // LONPOLE, LATPOLE
    store.lonpole = cards.get_float("LONPOLE");
    store.latpole = cards.get_float("LATPOLE");

    Ok(store)
}

/// Write a [`FitsStore`] as CLASS-style WCS keywords into a [`CardList`].
///
/// Key behaviours:
/// - Writes NAXIS
/// - Translates standard projection codes SFL and AIT back to GLS and ATF
/// - Subtracts RESTFREQ from the spectral axis CRVAL (absolute → offset)
/// - Computes and writes DELTAV and VELO-LSR from the spectral axis parameters
/// - Writes SPECSYS, RADESYS, EQUINOX, MJD-OBS
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    // NAXIS
    cards.set_int("NAXIS", naxis as i64, "Number of axes");

    // Find spectral axis and rest frequency for later use.
    let spec_idx = store.spectral_axis();
    let restfreq = store.restfrq;

    // CRPIXi
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = format!("CRPIX{}", i + 1);
            cards.set_float(&key, val, "Reference pixel");
        }
    }

    // CTYPEi — translate SFL→GLS, AIT→ATF for CLASS
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let class_ctype = translate_class_projection_write(ctype);
            let key = format!("CTYPE{}", i + 1);
            cards.set_string(&key, &class_ctype, "Axis type");
        }
    }

    // CUNITi
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = format!("CUNIT{}", i + 1);
            cards.set_string(&key, val, "Axis units");
        }
    }

    // RESTFREQ
    if let Some(rf) = restfreq {
        cards.set_float("RESTFREQ", rf, "Rest frequency (Hz)");
    }

    // CRVALi — subtract RESTFREQ from spectral axis
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let output_val = if Some(i) == spec_idx {
                if let Some(rf) = restfreq {
                    val - rf
                } else {
                    val
                }
            } else {
                val
            };
            let key = format!("CRVAL{}", i + 1);
            cards.set_float(&key, output_val, "Reference value");
        }
    }

    // CDELTi
    for i in 0..naxis {
        if let Some(val) = store.cdelt[i] {
            let key = format!("CDELT{}", i + 1);
            cards.set_float(&key, val, "Pixel scale");
        }
    }

    // Compute DELTAV and VELO-LSR from spectral axis parameters.
    if let (Some(si), Some(rf)) = (spec_idx, restfreq) {
        if rf != 0.0 {
            // DELTAV = -c * CDELT_freq / RESTFREQ (radio velocity convention)
            if let Some(cdelt_freq) = store.cdelt[si] {
                let deltav = -C_LIGHT * cdelt_freq / rf;
                cards.set_float("DELTAV", deltav, "Velocity channel spacing (m/s)");
            }

            // VELO-LSR = c * (1 - CRVAL_freq / RESTFREQ)
            if let Some(crval_freq) = store.crval[si] {
                let velo_lsr = C_LIGHT * (1.0 - crval_freq / rf);
                cards.set_float("VELO-LSR", velo_lsr, "Source velocity (m/s)");
            }
        }
    }

    // SPECSYS
    if let Some(ref specsys) = store.specsys {
        cards.set_string("SPECSYS", specsys, "Spectral reference frame");
    }

    // RADESYS
    if let Some(ref radesys) = store.radesys {
        cards.set_string("RADESYS", radesys, "Reference frame");
    }

    // EQUINOX
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "Equinox of coordinates");
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

/// Translate CLASS-specific projection codes on read.
///
/// CLASS uses older projection codes that must be translated:
/// - GLS → SFL (Sanson-Flamsteed)
/// - ATF → AIT (Aitoff/Hammer-Aitoff)
fn translate_class_projection_read(ctype: &str) -> String {
    // Replace GLS with SFL and ATF with AIT in the projection portion.
    // FITS CTYPE format: "XXXX-YYY" where YYY is the projection code.
    if ctype.len() >= 8 {
        let base = &ctype[..5]; // e.g. "GLON-" or "RA---"
        let proj = &ctype[5..];
        let new_proj = match proj.to_uppercase().as_str() {
            "GLS" => "SFL",
            "ATF" => "AIT",
            _ => return ctype.to_string(),
        };
        return format!("{base}{new_proj}");
    }
    // Try matching in longer CTYPEs (e.g. "GLON-GLS")
    ctype
        .replace("GLS", "SFL")
        .replace("ATF", "AIT")
        .replace("gls", "SFL")
        .replace("atf", "AIT")
}

/// Translate standard projection codes back to CLASS format on write.
///
/// - SFL → GLS
/// - AIT → ATF
fn translate_class_projection_write(ctype: &str) -> String {
    if ctype.len() >= 8 {
        let base = &ctype[..5];
        let proj = &ctype[5..];
        let new_proj = match proj.to_uppercase().as_str() {
            "SFL" => "GLS",
            "AIT" => "ATF",
            _ => return ctype.to_string(),
        };
        return format!("{base}{new_proj}");
    }
    ctype
        .replace("SFL", "GLS")
        .replace("AIT", "ATF")
        .replace("sfl", "GLS")
        .replace("ait", "ATF")
}

/// Determine SPECSYS from velocity keywords present in the header.
///
/// Priority:
/// - VELO-LSR or VLSR → "LSRK"
/// - VELO-HEL → "BARYCENT"
/// - VELO-OBS or VELO-EAR → "TOPOCENT"
/// - Explicit SPECSYS keyword (fallback)
fn determine_specsys_from_velocity(cards: &CardList) -> Option<String> {
    if cards.find("VELO-LSR").is_some() || cards.find("VLSR").is_some() {
        return Some("LSRK".to_string());
    }
    if cards.find("VELO-HEL").is_some() {
        return Some("BARYCENT".to_string());
    }
    if cards.find("VELO-OBS").is_some() || cards.find("VELO-EAR").is_some() {
        return Some("TOPOCENT".to_string());
    }
    // Fall back to explicit SPECSYS keyword if present.
    cards.get_string("SPECSYS")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Standard CLASS header for testing.
    const CLASS_HEADER: &str = "\
NAXIS   =                    3
CTYPE1  = 'FREQ    '
CRVAL1  =  0.000000000000E+000
CRPIX1  =              512.500
CDELT1  =  0.390625000000E+005
CTYPE2  = 'RA---TAN'
CRVAL2  =               40.738
CRPIX2  =                  0.5
CDELT2  =              -0.0100
CTYPE3  = 'DEC--TAN'
CRVAL3  =               62.933
CRPIX3  =                  0.5
CDELT3  =               0.0100
RADESYS = 'FK5     '
EQUINOX =               2000.0
SPECSYS = 'SOURCE  '
RESTFREQ=  0.110201354300E+012
VELO-LSR=  0.250000000000E+004
DELTAV  = -0.106265872717E+003
MJD-OBS =            52413.59
";

    #[test]
    fn read_class_offset_frequency() {
        // CRVAL1=0, RESTFREQ=1.102e11 → store.crval[0] ≈ 1.102e11
        let cards = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards).unwrap();

        let restfreq = 0.110201354300e12;
        // Offset is 0, so absolute = 0 + RESTFREQ
        let crval_spec = store.crval[0].unwrap();
        assert!(
            (crval_spec - restfreq).abs() < 1.0,
            "Expected crval[0] ≈ {restfreq}, got {crval_spec}"
        );
    }

    #[test]
    fn read_class_with_nonzero_offset() {
        // CRVAL1=1e6, RESTFREQ=1.102e11 → store.crval[0] ≈ 1.102e11 + 1e6
        let header = "\
NAXIS   =                    3
CTYPE1  = 'FREQ    '
CRVAL1  =  1.000000000000E+006
CRPIX1  =              512.500
CDELT1  =  0.390625000000E+005
CTYPE2  = 'RA---TAN'
CRVAL2  =               40.738
CRPIX2  =                  0.5
CDELT2  =              -0.0100
CTYPE3  = 'DEC--TAN'
CRVAL3  =               62.933
CRPIX3  =                  0.5
CDELT3  =               0.0100
RESTFREQ=  0.110201354300E+012
VELO-LSR=  0.250000000000E+004
DELTAV  = -0.106265872717E+003
";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        let restfreq = 0.110201354300e12;
        let offset = 1.0e6;
        let expected = restfreq + offset;
        let crval_spec = store.crval[0].unwrap();
        assert!(
            (crval_spec - expected).abs() < 1.0,
            "Expected crval[0] ≈ {expected}, got {crval_spec}"
        );
    }

    #[test]
    fn read_class_gls_to_sfl() {
        // CTYPE "GLON-GLS" → "GLON-SFL"
        let header = "\
NAXIS   =                    3
CTYPE1  = 'FREQ    '
CRVAL1  =  0.000000000000E+000
CRPIX1  =              512.500
CDELT1  =  0.390625000000E+005
CTYPE2  = 'GLON-GLS'
CRVAL2  =               40.738
CRPIX2  =                  0.5
CDELT2  =              -0.0100
CTYPE3  = 'GLAT-GLS'
CRVAL3  =               62.933
CRPIX3  =                  0.5
CDELT3  =               0.0100
RESTFREQ=  0.110201354300E+012
VELO-LSR=  0.250000000000E+004
DELTAV  = -0.106265872717E+003
";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.ctype[1], Some("GLON-SFL".to_string()));
        assert_eq!(store.ctype[2], Some("GLAT-SFL".to_string()));
    }

    #[test]
    fn read_class_specsys_from_velo() {
        // VELO-LSR present → specsys="LSRK"
        let cards = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.specsys, Some("LSRK".to_string()));
    }

    #[test]
    fn read_class_specsys_from_velo_hel() {
        // VELO-HEL present → specsys="BARYCENT"
        let header = "\
NAXIS   =                    3
CTYPE1  = 'FREQ    '
CRVAL1  =  0.000000000000E+000
CRPIX1  =              512.500
CDELT1  =  0.390625000000E+005
CTYPE2  = 'RA---TAN'
CRVAL2  =               40.738
CRPIX2  =                  0.5
CDELT2  =              -0.0100
CTYPE3  = 'DEC--TAN'
CRVAL3  =               62.933
CRPIX3  =                  0.5
CDELT3  =               0.0100
RESTFREQ=  0.110201354300E+012
VELO-HEL=  0.250000000000E+004
DELTAV  = -0.106265872717E+003
";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.specsys, Some("BARYCENT".to_string()));
    }

    #[test]
    fn write_class_subtracts_restfreq() {
        // store.crval[spec]=1.102e11, restfreq=1.102e11 → output CRVAL≈0
        let restfreq = 0.110201354300e12;
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(restfreq); // absolute freq = restfreq → offset = 0
        store.crpix[0] = Some(512.5);
        store.cdelt[0] = Some(39062.5);
        store.ctype[1] = Some("RA---TAN".to_string());
        store.crval[1] = Some(40.738);
        store.crpix[1] = Some(0.5);
        store.cdelt[1] = Some(-0.01);
        store.ctype[2] = Some("DEC--TAN".to_string());
        store.crval[2] = Some(62.933);
        store.crpix[2] = Some(0.5);
        store.cdelt[2] = Some(0.01);
        store.restfrq = Some(restfreq);
        store.radesys = Some("FK5".to_string());
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // The output CRVAL1 should be approximately 0 (absolute - restfreq)
        let crval1 = cards.get_float("CRVAL1").unwrap();
        assert!(crval1.abs() < 1.0, "Expected CRVAL1 ≈ 0, got {crval1}");

        // Non-spectral axes should remain unchanged
        let crval2 = cards.get_float("CRVAL2").unwrap();
        assert!((crval2 - 40.738).abs() < 1e-10);
    }

    #[test]
    fn write_class_deltav() {
        // CDELT=39062.5 Hz, RESTFREQ=1.102e11 → DELTAV computed correctly
        let restfreq = 0.110201354300e12;
        let cdelt_freq = 39062.5;
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(restfreq);
        store.crpix[0] = Some(512.5);
        store.cdelt[0] = Some(cdelt_freq);
        store.ctype[1] = Some("RA---TAN".to_string());
        store.crval[1] = Some(40.738);
        store.crpix[1] = Some(0.5);
        store.cdelt[1] = Some(-0.01);
        store.ctype[2] = Some("DEC--TAN".to_string());
        store.crval[2] = Some(62.933);
        store.crpix[2] = Some(0.5);
        store.cdelt[2] = Some(0.01);
        store.restfrq = Some(restfreq);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // DELTAV = -c * CDELT / RESTFREQ
        let expected_deltav = -C_LIGHT * cdelt_freq / restfreq;
        let deltav = cards.get_float("DELTAV").unwrap();
        assert!(
            (deltav - expected_deltav).abs() < 1e-6,
            "Expected DELTAV ≈ {expected_deltav}, got {deltav}"
        );
    }

    #[test]
    fn write_class_velo_lsr() {
        // CRVAL=RESTFREQ → VELO-LSR≈0 (at rest freq, velocity is 0)
        let restfreq = 0.110201354300e12;
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(restfreq); // at rest frequency → v=0
        store.crpix[0] = Some(512.5);
        store.cdelt[0] = Some(39062.5);
        store.ctype[1] = Some("RA---TAN".to_string());
        store.crval[1] = Some(40.738);
        store.crpix[1] = Some(0.5);
        store.cdelt[1] = Some(-0.01);
        store.ctype[2] = Some("DEC--TAN".to_string());
        store.crval[2] = Some(62.933);
        store.crpix[2] = Some(0.5);
        store.cdelt[2] = Some(0.01);
        store.restfrq = Some(restfreq);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // VELO-LSR = c * (1 - CRVAL/RESTFREQ) = c * (1 - 1) = 0
        let velo_lsr = cards.get_float("VELO-LSR").unwrap();
        assert!(
            velo_lsr.abs() < 1e-3,
            "Expected VELO-LSR ≈ 0, got {velo_lsr}"
        );
    }

    #[test]
    fn write_class_sfl_to_gls() {
        // SFL → GLS on write
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.0e11);
        store.crpix[0] = Some(512.5);
        store.cdelt[0] = Some(39062.5);
        store.ctype[1] = Some("GLON-SFL".to_string());
        store.crval[1] = Some(40.738);
        store.crpix[1] = Some(0.5);
        store.cdelt[1] = Some(-0.01);
        store.ctype[2] = Some("GLAT-SFL".to_string());
        store.crval[2] = Some(62.933);
        store.crpix[2] = Some(0.5);
        store.cdelt[2] = Some(0.01);
        store.restfrq = Some(1.0e11);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        assert_eq!(cards.get_string("CTYPE2"), Some("GLON-GLS".to_string()));
        assert_eq!(cards.get_string("CTYPE3"), Some("GLAT-GLS".to_string()));
    }

    #[test]
    fn read_class_spatial_axes_unchanged() {
        // Spatial axes (RA, DEC) should not have offset correction applied.
        let cards = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards).unwrap();

        let crval_ra = store.crval[1].unwrap();
        let crval_dec = store.crval[2].unwrap();
        assert!((crval_ra - 40.738).abs() < 1e-10);
        assert!((crval_dec - 62.933).abs() < 1e-10);
    }

    #[test]
    fn read_class_equinox_and_radesys() {
        let cards = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.radesys, Some("FK5".to_string()));
    }

    #[test]
    fn read_class_mjdobs() {
        let cards = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.mjdobs, Some(52413.59));
    }

    #[test]
    fn round_trip_class() {
        // Read → write → read should preserve values
        let cards_in = CardList::from_header(CLASS_HEADER).unwrap();
        let store = read_from_cards(&cards_in).unwrap();

        let mut cards_out = CardList::new();
        write_to_cards(&store, &mut cards_out).unwrap();

        let store2 = read_from_cards(&cards_out).unwrap();

        // Spectral axis should round-trip
        let crval1_orig = store.crval[0].unwrap();
        let crval1_rt = store2.crval[0].unwrap();
        assert!(
            (crval1_orig - crval1_rt).abs() < 1.0,
            "Spectral CRVAL round-trip failed: {crval1_orig} vs {crval1_rt}"
        );

        // Spatial axes should round-trip
        let crval2_orig = store.crval[1].unwrap();
        let crval2_rt = store2.crval[1].unwrap();
        assert!((crval2_orig - crval2_rt).abs() < 1e-10);
    }
}

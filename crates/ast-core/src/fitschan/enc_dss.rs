//! DSS (Digitized Sky Survey) plate solution encoding reader/writer.
//!
//! DSS encoding uses plate solution keywords from digitized photographic
//! plate scans. It is always:
//! - 2D only (RA + DEC)
//! - FK5 J2000 equinox
//! - TAN projection
//! - No rotation allowed (diagonal pixel scale only)
//!
//! Key DSS keywords:
//! - Plate centre: PLTRAH/PLTRAM/PLTRAS (RA), PLTDECD/PLTDECM/PLTDECS/PLTDECSN (DEC)
//! - Pixel size: XPIXELSZ, YPIXELSZ (microns)
//! - Plate offset: PPO3, PPO6 (microns — position of plate centre)
//! - Corner pixel: CNPIX1, CNPIX2 (default 0.0)
//! - Plate scale: AMDX1, AMDY1 (arcsec/mm), PLTSCALE (informational)
//!
//! This module provides:
//! - [`read_from_cards`]: extract DSS plate keywords from a CardList into a FitsStore
//! - [`write_to_cards`]: write a FitsStore as DSS plate keywords into a CardList

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Default DSS plate scale in arcsec/mm.
const DEFAULT_PLATE_SCALE: f64 = 67.2;

/// Read DSS plate solution keywords from a [`CardList`] into a [`FitsStore`].
///
/// The DSS encoding is always 2D with RA---TAN and DEC--TAN axes,
/// FK5 equinox J2000, and LONPOLE = 180.
///
/// Required keywords: PLTRAH, PLTRAM, PLTRAS (RA); PLTDECD, PLTDECM,
/// PLTDECS (DEC); XPIXELSZ, YPIXELSZ; PPO3, PPO6; and at least one of
/// AMDX1, AMDY1, or PLTSCALE for the plate scale.
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    // --- Plate centre RA (hours → degrees) ---
    let pltrah = cards
        .get_float("PLTRAH")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS encoding: PLTRAH keyword not found"))?;
    let pltram = cards
        .get_float("PLTRAM")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS encoding: PLTRAM keyword not found"))?;
    let pltras = cards
        .get_float("PLTRAS")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS encoding: PLTRAS keyword not found"))?;

    let ra_deg = 15.0 * (pltrah + pltram / 60.0 + pltras / 3600.0);

    // --- Plate centre DEC (degrees) ---
    let pltdecd = cards.get_float("PLTDECD").ok_or_else(|| {
        AstError::new(ErrorCode::Bdfts, "DSS encoding: PLTDECD keyword not found")
    })?;
    let pltdecm = cards.get_float("PLTDECM").ok_or_else(|| {
        AstError::new(ErrorCode::Bdfts, "DSS encoding: PLTDECM keyword not found")
    })?;
    let pltdecs = cards.get_float("PLTDECS").ok_or_else(|| {
        AstError::new(ErrorCode::Bdfts, "DSS encoding: PLTDECS keyword not found")
    })?;

    // Sign from PLTDECSN: '+' (default) or '-'
    let sign = match cards.get_string("PLTDECSN") {
        Some(s) if s.starts_with('-') => -1.0,
        _ => 1.0,
    };

    let dec_deg = sign * (pltdecd + pltdecm / 60.0 + pltdecs / 3600.0);

    // --- Pixel size (microns) ---
    let xpixelsz = cards.get_float("XPIXELSZ").ok_or_else(|| {
        AstError::new(ErrorCode::Bdfts, "DSS encoding: XPIXELSZ keyword not found")
    })?;
    let ypixelsz = cards.get_float("YPIXELSZ").ok_or_else(|| {
        AstError::new(ErrorCode::Bdfts, "DSS encoding: YPIXELSZ keyword not found")
    })?;

    // --- Plate offset (microns) ---
    let ppo3 = cards
        .get_float("PPO3")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS encoding: PPO3 keyword not found"))?;
    let ppo6 = cards
        .get_float("PPO6")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS encoding: PPO6 keyword not found"))?;

    // --- Corner pixel (optional, default 0.0) ---
    let cnpix1 = cards.get_float("CNPIX1").unwrap_or(0.0);
    let cnpix2 = cards.get_float("CNPIX2").unwrap_or(0.0);

    // --- Plate scale (arcsec/mm) ---
    // Try AMDX1, then AMDY1, then PLTSCALE
    let plate_scale = cards
        .get_float("AMDX1")
        .or_else(|| cards.get_float("AMDY1"))
        .or_else(|| cards.get_float("PLTSCALE"))
        .ok_or_else(|| {
            AstError::new(
                ErrorCode::Bdfts,
                "DSS encoding: no plate scale found (need AMDX1, AMDY1, or PLTSCALE)",
            )
        })?;

    // --- Compute CRPIX ---
    // PPO3/PPO6 are in microns, XPIXELSZ/YPIXELSZ are in microns.
    // Division gives pixels directly.
    let crpix1 = (ppo3 / xpixelsz) - cnpix1 + 0.5;
    let crpix2 = (ppo6 / ypixelsz) - cnpix2 + 0.5;

    // --- Compute CDELT ---
    // plate_scale is arcsec/mm, pixel size is microns (= 0.001 mm)
    // CDELT in degrees: 0.001 * pixelsz * plate_scale / 3600
    let cdelt1 = -0.001 * xpixelsz * plate_scale / 3600.0;
    let cdelt2 = 0.001 * ypixelsz * plate_scale / 3600.0;

    // --- Build FitsStore ---
    let mut store = FitsStore::new(2);

    store.crpix[0] = Some(crpix1);
    store.crpix[1] = Some(crpix2);
    store.crval[0] = Some(ra_deg);
    store.crval[1] = Some(dec_deg);
    store.cdelt[0] = Some(cdelt1);
    store.cdelt[1] = Some(cdelt2);
    store.ctype[0] = Some("RA---TAN".to_string());
    store.ctype[1] = Some("DEC--TAN".to_string());
    store.radesys = Some("FK5".to_string());
    store.equinox = Some(2000.0);
    store.lonpole = Some(180.0);

    Ok(store)
}

/// Write a [`FitsStore`] as DSS plate solution keywords into a [`CardList`].
///
/// The store must have exactly 2 axes (RA + DEC). The writer decomposes
/// the reference coordinates and pixel scale back into DSS plate
/// solution keywords.
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    // DSS is always 2D.
    if store.naxis != 2 {
        return Err(AstError::new(
            ErrorCode::Bdfts,
            format!("DSS encoding requires exactly 2 axes, got {}", store.naxis),
        ));
    }

    let crval0 = store.crval[0].unwrap_or(0.0);
    let crval1 = store.crval[1].unwrap_or(0.0);
    let crpix0 = store.crpix[0].unwrap_or(1.0);
    let crpix1 = store.crpix[1].unwrap_or(1.0);
    let cdelt0 = store.cdelt[0].unwrap_or(-1.0);
    let cdelt1 = store.cdelt[1].unwrap_or(1.0);

    // --- Decompose RA (degrees → hours/min/sec) ---
    let ra_hours = crval0 / 15.0;
    let pltrah = ra_hours.floor();
    let ra_remainder = (ra_hours - pltrah) * 60.0;
    let pltram = ra_remainder.floor();
    let pltras = (ra_remainder - pltram) * 60.0;

    // --- Decompose DEC (degrees → deg/arcmin/arcsec + sign) ---
    let dec_sign = if crval1 < 0.0 { "-" } else { "+" };
    let dec_abs = crval1.abs();
    let pltdecd = dec_abs.floor();
    let dec_remainder = (dec_abs - pltdecd) * 60.0;
    let pltdecm = dec_remainder.floor();
    let pltdecs = (dec_remainder - pltdecm) * 60.0;

    // --- Plate scale ---
    // Derive from CDELT, or use a default.
    let plate_scale = DEFAULT_PLATE_SCALE;

    // --- Pixel sizes (microns) ---
    // |CDELT| = 0.001 * pixelsz * plate_scale / 3600
    // pixelsz = |CDELT| * 3600 / plate_scale / 0.001
    let xpixelsz = cdelt0.abs() * 3600.0 / plate_scale * 1000.0;
    let ypixelsz = cdelt1.abs() * 3600.0 / plate_scale * 1000.0;

    // --- Corner pixels (arbitrary, use 1.0) ---
    let cnpix1 = 1.0;
    let cnpix2 = 1.0;

    // --- Plate offset (microns) ---
    // CRPIX = (PPO / pixelsz) - CNPIX + 0.5
    // PPO = pixelsz * (CRPIX + CNPIX - 0.5)
    let ppo3 = xpixelsz * (crpix0 + cnpix1 - 0.5);
    let ppo6 = ypixelsz * (crpix1 + cnpix2 - 0.5);

    // --- Write keywords ---
    cards.set_float("PLTRAH", pltrah, "Plate centre RA hours");
    cards.set_float("PLTRAM", pltram, "Plate centre RA minutes");
    cards.set_float("PLTRAS", pltras, "Plate centre RA seconds");
    cards.set_float("PLTDECD", pltdecd, "Plate centre DEC degrees");
    cards.set_float("PLTDECM", pltdecm, "Plate centre DEC arcminutes");
    cards.set_float("PLTDECS", pltdecs, "Plate centre DEC arcseconds");
    cards.set_string("PLTDECSN", dec_sign, "Plate centre DEC sign");
    cards.set_float("XPIXELSZ", xpixelsz, "Pixel size X (microns)");
    cards.set_float("YPIXELSZ", ypixelsz, "Pixel size Y (microns)");
    cards.set_float("PPO3", ppo3, "Plate offset X (microns)");
    cards.set_float("PPO6", ppo6, "Plate offset Y (microns)");
    cards.set_float("CNPIX1", cnpix1, "Corner pixel X");
    cards.set_float("CNPIX2", cnpix2, "Corner pixel Y");
    cards.set_float("AMDX1", plate_scale, "Plate scale X (arcsec/mm)");
    cards.set_float("AMDY1", plate_scale, "Plate scale Y (arcsec/mm)");
    cards.set_float("PLTSCALE", plate_scale, "Plate scale (arcsec/mm)");

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a CardList from a standard DSS test header.
    fn dss_test_cards() -> CardList {
        let header = "\
PLTRAH  =                   10 / Plate centre RA hours
PLTRAM  =                   47 / Plate centre RA minutes
PLTRAS  =                 57.3 / Plate centre RA seconds
PLTDECD =                   60 / Plate centre DEC degrees
PLTDECM =                   15 / Plate centre DEC arcminutes
PLTDECS =                51.85 / Plate centre DEC arcseconds
PLTDECSN= '-'                  / Plate centre DEC sign
XPIXELSZ=                25.28 / Pixel size X (microns)
YPIXELSZ=                25.28 / Pixel size Y (microns)
PPO3    =            -18825.9  / Plate offset X (microns)
PPO6    =            -22216.5  / Plate offset Y (microns)
CNPIX1  =                  1.0 / Corner pixel X
CNPIX2  =                  1.0 / Corner pixel Y
AMDX1   =                67.23 / Plate scale X (arcsec/mm)
AMDY1   =                67.23 / Plate scale Y (arcsec/mm)
PLTSCALE=                67.23 / Plate scale (arcsec/mm)
END";
        CardList::from_header(header).unwrap()
    }

    #[test]
    fn read_dss_plate_centre() {
        let cards = dss_test_cards();
        let store = read_from_cards(&cards).unwrap();

        // RA = 15 * (10 + 47/60 + 57.3/3600)
        let expected_ra = 15.0 * (10.0 + 47.0 / 60.0 + 57.3 / 3600.0);
        let ra = store.crval[0].unwrap();
        assert!(
            (ra - expected_ra).abs() < 1e-8,
            "RA: expected {expected_ra}, got {ra}"
        );
    }

    #[test]
    fn read_dss_negative_dec() {
        let cards = dss_test_cards();
        let store = read_from_cards(&cards).unwrap();

        // DEC = -(60 + 15/60 + 51.85/3600)
        let expected_dec = -(60.0 + 15.0 / 60.0 + 51.85 / 3600.0);
        let dec = store.crval[1].unwrap();
        assert!(
            (dec - expected_dec).abs() < 1e-8,
            "DEC: expected {expected_dec}, got {dec}"
        );
    }

    #[test]
    fn read_dss_positive_dec() {
        // Same header but with PLTDECSN = '+'
        let header = "\
PLTRAH  =                   10 / Plate centre RA hours
PLTRAM  =                   47 / Plate centre RA minutes
PLTRAS  =                 57.3 / Plate centre RA seconds
PLTDECD =                   60 / Plate centre DEC degrees
PLTDECM =                   15 / Plate centre DEC arcminutes
PLTDECS =                51.85 / Plate centre DEC arcseconds
PLTDECSN= '+'                  / Plate centre DEC sign
XPIXELSZ=                25.28 / Pixel size X (microns)
YPIXELSZ=                25.28 / Pixel size Y (microns)
PPO3    =            -18825.9  / Plate offset X (microns)
PPO6    =            -22216.5  / Plate offset Y (microns)
CNPIX1  =                  1.0 / Corner pixel X
CNPIX2  =                  1.0 / Corner pixel Y
AMDX1   =                67.23 / Plate scale X (arcsec/mm)
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        let expected_dec = 60.0 + 15.0 / 60.0 + 51.85 / 3600.0;
        let dec = store.crval[1].unwrap();
        assert!(
            (dec - expected_dec).abs() < 1e-8,
            "DEC: expected {expected_dec}, got {dec}"
        );
    }

    #[test]
    fn read_dss_crpix() {
        let cards = dss_test_cards();
        let store = read_from_cards(&cards).unwrap();

        // CRPIX1 = (PPO3 / XPIXELSZ) - CNPIX1 + 0.5
        //        = (-18825.9 / 25.28) - 1.0 + 0.5
        let expected_crpix1 = (-18825.9 / 25.28) - 1.0 + 0.5;
        let crpix1 = store.crpix[0].unwrap();
        assert!(
            (crpix1 - expected_crpix1).abs() < 1e-6,
            "CRPIX1: expected {expected_crpix1}, got {crpix1}"
        );

        // CRPIX2 = (PPO6 / YPIXELSZ) - CNPIX2 + 0.5
        let expected_crpix2 = (-22216.5 / 25.28) - 1.0 + 0.5;
        let crpix2 = store.crpix[1].unwrap();
        assert!(
            (crpix2 - expected_crpix2).abs() < 1e-6,
            "CRPIX2: expected {expected_crpix2}, got {crpix2}"
        );
    }

    #[test]
    fn read_dss_cdelt() {
        let cards = dss_test_cards();
        let store = read_from_cards(&cards).unwrap();

        // CDELT1 = -0.001 * XPIXELSZ * plate_scale / 3600
        //        = -0.001 * 25.28 * 67.23 / 3600
        let expected_cdelt1 = -0.001 * 25.28 * 67.23 / 3600.0;
        let cdelt1 = store.cdelt[0].unwrap();
        assert!(
            (cdelt1 - expected_cdelt1).abs() < 1e-12,
            "CDELT1: expected {expected_cdelt1}, got {cdelt1}"
        );

        // CDELT2 = 0.001 * YPIXELSZ * plate_scale / 3600
        let expected_cdelt2 = 0.001 * 25.28 * 67.23 / 3600.0;
        let cdelt2 = store.cdelt[1].unwrap();
        assert!(
            (cdelt2 - expected_cdelt2).abs() < 1e-12,
            "CDELT2: expected {expected_cdelt2}, got {cdelt2}"
        );

        // CDELT1 should be negative
        assert!(cdelt1 < 0.0, "CDELT1 should be negative");
        // CDELT2 should be positive
        assert!(cdelt2 > 0.0, "CDELT2 should be positive");
    }

    #[test]
    fn read_dss_always_fk5() {
        let cards = dss_test_cards();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.naxis, 2);
        assert_eq!(store.ctype[0], Some("RA---TAN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--TAN".to_string()));
        assert_eq!(store.radesys, Some("FK5".to_string()));
        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.lonpole, Some(180.0));
    }

    #[test]
    fn read_dss_fallback_plate_scale() {
        // Header with only PLTSCALE (no AMDX1 or AMDY1)
        let header = "\
PLTRAH  =                   12 / Plate centre RA hours
PLTRAM  =                    0 / Plate centre RA minutes
PLTRAS  =                  0.0 / Plate centre RA seconds
PLTDECD =                   45 / Plate centre DEC degrees
PLTDECM =                    0 / Plate centre DEC arcminutes
PLTDECS =                  0.0 / Plate centre DEC arcseconds
XPIXELSZ=                25.0  / Pixel size X (microns)
YPIXELSZ=                25.0  / Pixel size Y (microns)
PPO3    =              1000.0  / Plate offset X (microns)
PPO6    =              1000.0  / Plate offset Y (microns)
PLTSCALE=                67.0  / Plate scale (arcsec/mm)
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // Should use PLTSCALE as the plate scale
        let expected_cdelt2 = 0.001 * 25.0 * 67.0 / 3600.0;
        let cdelt2 = store.cdelt[1].unwrap();
        assert!(
            (cdelt2 - expected_cdelt2).abs() < 1e-12,
            "CDELT2: expected {expected_cdelt2}, got {cdelt2}"
        );
    }

    #[test]
    fn read_dss_default_cnpix() {
        // Header without CNPIX1/CNPIX2 — should default to 0.0
        let header = "\
PLTRAH  =                   12 / Plate centre RA hours
PLTRAM  =                    0 / Plate centre RA minutes
PLTRAS  =                  0.0 / Plate centre RA seconds
PLTDECD =                   45 / Plate centre DEC degrees
PLTDECM =                    0 / Plate centre DEC arcminutes
PLTDECS =                  0.0 / Plate centre DEC arcseconds
XPIXELSZ=                25.0  / Pixel size X (microns)
YPIXELSZ=                25.0  / Pixel size Y (microns)
PPO3    =              1000.0  / Plate offset X (microns)
PPO6    =              2000.0  / Plate offset Y (microns)
AMDX1   =                67.0  / Plate scale (arcsec/mm)
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // CRPIX1 = (1000/25) - 0 + 0.5 = 40.5
        let crpix1 = store.crpix[0].unwrap();
        assert!(
            (crpix1 - 40.5).abs() < 1e-10,
            "CRPIX1 with default CNPIX: expected 40.5, got {crpix1}"
        );

        // CRPIX2 = (2000/25) - 0 + 0.5 = 80.5
        let crpix2 = store.crpix[1].unwrap();
        assert!(
            (crpix2 - 80.5).abs() < 1e-10,
            "CRPIX2 with default CNPIX: expected 80.5, got {crpix2}"
        );
    }

    #[test]
    fn write_dss_ra_decomposition() {
        // RA = 161.98875 degrees = 10h 47m 57.3s
        let mut store = FitsStore::new(2);
        store.crval[0] = Some(161.98875);
        store.crval[1] = Some(45.0);
        store.crpix[0] = Some(512.0);
        store.crpix[1] = Some(512.0);
        store.cdelt[0] = Some(-0.000472);
        store.cdelt[1] = Some(0.000472);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // RA decomposition: 161.98875 / 15 = 10.79925 hours
        // PLTRAH = 10
        let pltrah = cards.get_float("PLTRAH").unwrap();
        assert!(
            (pltrah - 10.0).abs() < 1e-10,
            "PLTRAH: expected 10, got {pltrah}"
        );

        // PLTRAM = floor((10.79925 - 10) * 60) = floor(47.955) = 47
        let pltram = cards.get_float("PLTRAM").unwrap();
        assert!(
            (pltram - 47.0).abs() < 1e-10,
            "PLTRAM: expected 47, got {pltram}"
        );

        // PLTRAS = (47.955 - 47) * 60 = 57.3
        let pltras = cards.get_float("PLTRAS").unwrap();
        assert!(
            (pltras - 57.3).abs() < 0.01,
            "PLTRAS: expected ~57.3, got {pltras}"
        );
    }

    #[test]
    fn write_dss_negative_dec() {
        let mut store = FitsStore::new(2);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-60.264);
        store.crpix[0] = Some(512.0);
        store.crpix[1] = Some(512.0);
        store.cdelt[0] = Some(-0.0005);
        store.cdelt[1] = Some(0.0005);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        let decsn = cards.get_string("PLTDECSN").unwrap();
        assert_eq!(decsn, "-", "PLTDECSN should be '-' for negative DEC");

        let pltdecd = cards.get_float("PLTDECD").unwrap();
        assert!(
            (pltdecd - 60.0).abs() < 1e-10,
            "PLTDECD: expected 60, got {pltdecd}"
        );
    }

    #[test]
    fn write_dss_positive_dec() {
        let mut store = FitsStore::new(2);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.5);
        store.crpix[0] = Some(256.0);
        store.crpix[1] = Some(256.0);
        store.cdelt[0] = Some(-0.0005);
        store.cdelt[1] = Some(0.0005);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        let decsn = cards.get_string("PLTDECSN").unwrap();
        assert_eq!(decsn, "+", "PLTDECSN should be '+' for positive DEC");
    }

    #[test]
    fn write_dss_requires_2d() {
        let store = FitsStore::new(3);
        let mut cards = CardList::new();
        let result = write_to_cards(&store, &mut cards);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message().contains("2 axes"),
            "Error should mention 2 axes: {}",
            err.message()
        );
    }

    #[test]
    fn write_dss_requires_2d_1axis() {
        let store = FitsStore::new(1);
        let mut cards = CardList::new();
        let result = write_to_cards(&store, &mut cards);
        assert!(result.is_err());
    }

    #[test]
    fn write_dss_pixel_sizes() {
        // Verify pixel size computation from CDELT
        let mut store = FitsStore::new(2);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        // |CDELT| * 3600 / plate_scale * 1000 = pixel_size
        // 0.0005 * 3600 / 67.2 * 1000 = 26.7857...
        store.cdelt[0] = Some(-0.0005);
        store.cdelt[1] = Some(0.0005);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        let xpix = cards.get_float("XPIXELSZ").unwrap();
        let expected_xpix = 0.0005 * 3600.0 / DEFAULT_PLATE_SCALE * 1000.0;
        assert!(
            (xpix - expected_xpix).abs() < 1e-6,
            "XPIXELSZ: expected {expected_xpix}, got {xpix}"
        );

        let ypix = cards.get_float("YPIXELSZ").unwrap();
        assert!(
            (ypix - expected_xpix).abs() < 1e-6,
            "YPIXELSZ: expected {expected_xpix}, got {ypix}"
        );
    }

    #[test]
    fn read_dss_missing_keyword_error() {
        // Header missing XPIXELSZ
        let header = "\
PLTRAH  =                   10 / Plate centre RA hours
PLTRAM  =                   47 / Plate centre RA minutes
PLTRAS  =                 57.3 / Plate centre RA seconds
PLTDECD =                   60 / Plate centre DEC degrees
PLTDECM =                   15 / Plate centre DEC arcminutes
PLTDECS =                51.85 / Plate centre DEC arcseconds
YPIXELSZ=                25.28 / Pixel size Y (microns)
PPO3    =            -18825.9  / Plate offset X (microns)
PPO6    =            -22216.5  / Plate offset Y (microns)
AMDX1   =                67.23 / Plate scale (arcsec/mm)
END";
        let cards = CardList::from_header(header).unwrap();
        let result = read_from_cards(&cards);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message().contains("XPIXELSZ"),
            "Error should mention XPIXELSZ: {}",
            err.message()
        );
    }

    #[test]
    fn round_trip_preserves_coordinates() {
        // Write a store, then read it back and verify coordinates are preserved.
        let mut original = FitsStore::new(2);
        original.crval[0] = Some(161.98875); // RA
        original.crval[1] = Some(-60.264); // DEC
        original.crpix[0] = Some(512.0);
        original.crpix[1] = Some(512.0);
        original.cdelt[0] = Some(-0.000472);
        original.cdelt[1] = Some(0.000472);

        // Write
        let mut cards = CardList::new();
        write_to_cards(&original, &mut cards).unwrap();

        // Read back
        let recovered = read_from_cards(&cards).unwrap();

        // RA should be preserved to ~arcsecond accuracy
        let ra_orig = original.crval[0].unwrap();
        let ra_recov = recovered.crval[0].unwrap();
        assert!(
            (ra_orig - ra_recov).abs() < 1.0 / 3600.0, // < 1 arcsecond
            "RA round-trip: original {ra_orig}, recovered {ra_recov}"
        );

        // DEC should be preserved to ~arcsecond accuracy
        let dec_orig = original.crval[1].unwrap();
        let dec_recov = recovered.crval[1].unwrap();
        assert!(
            (dec_orig - dec_recov).abs() < 1.0 / 3600.0,
            "DEC round-trip: original {dec_orig}, recovered {dec_recov}"
        );
    }
}

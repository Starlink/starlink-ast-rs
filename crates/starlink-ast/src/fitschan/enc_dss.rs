// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! DSS (Digitized Sky Survey) plate solution encoding reader/writer.
//!
//! DSS encoding stores plate-solution keywords from digitized photographic
//! plate scans. Mirroring C `fitschan.c` `DSSToStore` / `DSSFromStore`, the
//! conversion is **lossless**: the plate-solution polynomial (AMDX1-20 /
//! AMDY1-20) is mapped to/from a "TAN with polynomial corrections" (TPN)
//! projection via the FitsStore PV parameters, NOT collapsed into a plain
//! TAN. DSS is always:
//! - 2D only (RA + DEC),
//! - FK5, equinox J2000, LONPOLE = 180,
//! - a `RA---TPN` / `DEC--TPN` projection (a plain TAN is the special case
//!   where the only non-zero PV term is `PVi_1 = 1`),
//! - free of rotation (diagonal PC matrix only).
//!
//! The plate co-ordinate convention uses "lambda = 1" (plate co-ordinates in
//! mm, not degrees), so the AMD coefficients are scaled by 1/3600 into the
//! degree-based PV parameters.
//!
//! Note: C's FitsChan DSS path does not use the `DssMap` class — DSS is a
//! pure FitsStore↔keyword conversion. This module mirrors that.

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read DSS plate-solution keywords from a [`CardList`] into a [`FitsStore`].
///
/// Ports C `DSSToStore` (fitschan.c:9458). The result is a 2-D `RA---TPN` /
/// `DEC--TPN` store with the AMDX/AMDY polynomial encoded as PV parameters,
/// FK5 / equinox 2000 / LONPOLE 180.
///
/// Required keywords: PPO3, PPO6, XPIXELSZ, YPIXELSZ, PLTRAH/M/S,
/// PLTDECD/M/S, PLTDECSN. Optional (default 0.0): CNPIX1, CNPIX2. The AMDX/
/// AMDY polynomial coefficients are optional and default to "absent".
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    // Optional corner-pixel keywords, default 0.0. All DSS plate-solution
    // keywords are consumed by the read (mark=1) — they are excluded from any
    // subsequent output, mirroring C WcsFcRead.
    let cnpix1 = cards.get_float_marked("CNPIX1").unwrap_or(0.0);
    let cnpix2 = cards.get_float_marked("CNPIX2").unwrap_or(0.0);

    // Required keywords.
    let ppo3 = req(cards, "PPO3")?;
    let ppo6 = req(cards, "PPO6")?;
    let xpixelsz = req(cards, "XPIXELSZ")?;
    let ypixelsz = req(cards, "YPIXELSZ")?;
    let pltrah = req(cards, "PLTRAH")?;
    let pltram = req(cards, "PLTRAM")?;
    let pltras = req(cards, "PLTRAS")?;
    let pltdecd = req(cards, "PLTDECD")?;
    let pltdecm = req(cards, "PLTDECM")?;
    let pltdecs = req(cards, "PLTDECS")?;

    // Sign of the declination from the first non-blank char of PLTDECSN.
    let sign = match cards.get_string_marked("PLTDECSN") {
        Some(s) => {
            let t = s.trim_start();
            if t.starts_with('-') { -1.0 } else { 1.0 }
        }
        None => {
            return Err(AstError::new(
                ErrorCode::Bdfts,
                "DSS encoding: PLTDECSN keyword not found",
            ));
        }
    };

    // The polynomial co-efficients. Absent ones default to None (C: AST__BAD),
    // which means the corresponding PV term is left unset.
    let mut amdx = [None; 20];
    let mut amdy = [None; 20];
    for i in 0..20 {
        amdx[i] = cards.get_float_marked(&format!("AMDX{}", i + 1));
        amdy[i] = cards.get_float_marked(&format!("AMDY{}", i + 1));
    }

    let mut store = FitsStore::new(2);

    // Map the AMD coefficients to PV parameters (pv = amd / 3600). Each axis
    // uses the same index pattern (fitschan.c:9591-9682).
    set_pv_from_amd(&mut store, 0, &amdx);
    set_pv_from_amd(&mut store, 1, &amdy);

    // CRPIX from the plate offset and pixel sizes.
    if xpixelsz == 0.0 {
        return Err(AstError::new(
            ErrorCode::Bdfts,
            "FITS keyword XPIXELSZ has illegal value 0.0",
        ));
    }
    if ypixelsz == 0.0 {
        return Err(AstError::new(
            ErrorCode::Bdfts,
            "FITS keyword YPIXELSZ has illegal value 0.0",
        ));
    }
    store.crpix[0] = Some((ppo3 / xpixelsz) - cnpix1 + 0.5);
    store.crpix[1] = Some((ppo6 / ypixelsz) - cnpix2 + 0.5);

    // CRVAL from the plate-center RA/DEC.
    store.crval[0] = Some(15.0 * (pltrah + pltram / 60.0 + pltras / 3600.0));
    store.crval[1] = Some(sign * (pltdecd + pltdecm / 60.0 + pltdecs / 3600.0));

    // PC matrix carries the pixel sizes (in mm, lambda=1); CDELT is unity.
    store.pc[0][0] = Some(-0.001 * xpixelsz);
    store.pc[1][1] = Some(0.001 * ypixelsz);
    store.cdelt[0] = Some(1.0);
    store.cdelt[1] = Some(1.0);

    // Constant celestial framing.
    store.lonpole = Some(180.0);
    store.equinox = Some(2000.0);
    store.radesys = Some("FK5".to_string());
    store.ctype[0] = Some("RA---TPN".to_string());
    store.ctype[1] = Some("DEC--TPN".to_string());

    Ok(store)
}

/// Fetch a required float keyword, or a `Bdfts` "not found" error.
fn req(cards: &CardList, key: &str) -> AstResult<f64> {
    cards.get_float_marked(key).ok_or_else(|| {
        AstError::new(
            ErrorCode::Bdfts,
            format!("DSS encoding: {key} keyword not found"),
        )
    })
}

/// Map one axis's AMD coefficients into the store's PV parameters.
///
/// Mirrors the per-axis block in C `DSSToStore` (fitschan.c:9591-9636 for X,
/// 9543-9588 for Y — identical index pattern). `pv = amd / 3600`, each term
/// set only when its source coefficient(s) are present.
fn set_pv_from_amd(store: &mut FitsStore, axis: usize, amd: &[Option<f64>; 20]) {
    let mut set = |idx: usize, v: f64| store.pv[axis][idx] = Some(v / 3600.0);

    if let Some(v) = amd[2] {
        set(0, v);
    }
    if let Some(v) = amd[0] {
        set(1, v);
    }
    if let Some(v) = amd[1] {
        set(2, v);
    }
    if let (Some(a), Some(b)) = (amd[3], amd[6]) {
        set(4, a + b);
    }
    if let Some(v) = amd[4] {
        set(5, v);
    }
    if let (Some(a), Some(b)) = (amd[5], amd[6]) {
        set(6, a + b);
    }
    if let (Some(a), Some(b)) = (amd[7], amd[11]) {
        set(7, a + b);
    }
    if let Some(v) = amd[8] {
        set(8, v);
    }
    if let (Some(a), Some(b)) = (amd[9], amd[11]) {
        set(9, a + b);
    }
    if let Some(v) = amd[10] {
        set(10, v);
    }
    if let Some(v) = amd[12] {
        set(17, v);
        set(19, 2.0 * v);
        set(21, v);
    }
}

/// Write a [`FitsStore`] as DSS plate-solution keywords into a [`CardList`].
///
/// Ports C `DSSFromStore` (fitschan.c:9160). The store must describe a 2-D
/// FK5 `RA---TAN`/`DEC--TAN` or `RA---TPN`/`DEC--TPN` frame with no rotation;
/// otherwise nothing is written and `false` is returned, as C returns 0. The PV parameters are converted back to the AMDX/AMDY polynomial.
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<bool> {
    // DSS is always 2-D: the last CRPIX is the second.
    if store.max_crpix_axis() != Some(1) {
        return Ok(false);
    }

    // CTYPE: both TAN or both TPN.
    let ct0 = store.ctype[0].as_deref().unwrap_or("");
    let ct1 = store.ctype[1].as_deref().unwrap_or("");
    let gottpn = ct0 == "RA---TPN";
    if ct0 != "RA---TAN" && !gottpn {
        return Ok(false);
    }
    if gottpn {
        if ct1 != "DEC--TPN" {
            return Ok(false);
        }
    } else if ct1 != "DEC--TAN" {
        return Ok(false);
    }

    // LONPOLE undefined or 180.
    if let Some(v) = store.lonpole {
        if v != 180.0 {
            return Ok(false);
        }
    }
    // RADESYS must be FK5.
    if store.radesys.as_deref() != Some("FK5") {
        return Ok(false);
    }
    // EQUINOX undefined or 2000.
    if let Some(v) = store.equinox {
        if v != 2000.0 {
            return Ok(false);
        }
    }

    // Pixel sizes from CDELT * PC. Both CDELTs must be defined and non-zero.
    let Some(cdelt0) = store.cdelt[0] else {
        return Ok(false);
    };
    let pc00 = store.pc[0][0].unwrap_or(1.0);
    let Some(cdelt1) = store.cdelt[1] else {
        return Ok(false);
    };
    let pc11 = store.pc[1][1].unwrap_or(1.0);
    let mut xpixelsz = cdelt0 * pc00;
    let mut ypixelsz = cdelt1 * pc11;
    if xpixelsz == 0.0 || ypixelsz == 0.0 {
        return Ok(false);
    }
    xpixelsz *= -1000.0;
    ypixelsz *= 1000.0;

    // No rotation: off-diagonal PC must be unset or zero.
    if let Some(v) = store.pc[0][1] {
        if v != 0.0 {
            return Ok(false);
        }
    }
    if let Some(v) = store.pc[1][0] {
        if v != 0.0 {
            return Ok(false);
        }
    }

    // Read PV[axis][0..22]. For a plain TAN, force the identity (pv[1]=1).
    let read_pv = |axis: usize| -> [f64; 22] {
        let mut pv = [0.0; 22];
        for (m, slot) in pv.iter_mut().enumerate() {
            let raw = store.pv[axis].get(m).copied().flatten();
            *slot = match raw {
                Some(v) if gottpn => v,
                _ => {
                    if m == 1 {
                        1.0
                    } else {
                        0.0
                    }
                }
            };
        }
        pv
    };
    let pvx = read_pv(0);
    let pvy = read_pv(1);

    // No PV term beyond index 21 may be set.
    if max_pv_index(store) > 21 {
        return Ok(false);
    }

    // Specific parameters must take their required zero values.
    if pvx[3] != 0.0 || pvy[3] != 0.0 {
        return Ok(false);
    }
    for m in 11..17 {
        if pvx[m] != 0.0 || pvy[m] != 0.0 {
            return Ok(false);
        }
    }
    if pvx[18] != 0.0 || pvy[18] != 0.0 {
        return Ok(false);
    }
    if pvx[20] != 0.0 || pvy[20] != 0.0 {
        return Ok(false);
    }
    // Related-parameter constraints.
    if !ast_equal(2.0 * pvx[17], pvx[19]) || !ast_equal(pvx[17], pvx[21]) {
        return Ok(false);
    }
    if !ast_equal(2.0 * pvy[17], pvy[19]) || !ast_equal(pvy[17], pvy[21]) {
        return Ok(false);
    }

    // Convert PV back to AMD coefficients (amd = 3600 * pv). AMDX/Y7 and
    // AMDX/Y12 are deliberately left zero (redundant).
    let mut amdx = [0.0_f64; 20];
    let mut amdy = [0.0_f64; 20];
    amd_from_pv(&mut amdx, &pvx);
    amd_from_pv(&mut amdy, &pvy);

    // The plate scale is the mean of the leading X and Y coefficients.
    let pltscl = 0.5 * (amdx[0] + amdy[0]);

    // Arbitrary corner pixel of 1; derive the matching plate-center offset.
    let cnpix1 = 1.0;
    let cnpix2 = 1.0;
    let Some(crpix0) = store.crpix[0] else {
        return Ok(false);
    };
    let Some(crpix1) = store.crpix[1] else {
        return Ok(false);
    };
    let ppo3 = xpixelsz * (crpix0 + cnpix1 - 0.5);
    let ppo6 = ypixelsz * (crpix1 + cnpix2 - 0.5);

    // Decompose CRVAL1 (RA degrees) into hours/min/sec, wrapped to 0..24.
    let Some(crval0) = store.crval[0] else {
        return Ok(false);
    };
    let mut ra_hours = crval0 / 15.0;
    while ra_hours < 0.0 {
        ra_hours += 24.0;
    }
    while ra_hours >= 24.0 {
        ra_hours -= 24.0;
    }
    let pltrah = ra_hours.trunc();
    let ra_rem = 60.0 * (ra_hours - pltrah);
    let pltram = ra_rem.trunc();
    let pltras = 60.0 * (ra_rem - pltram);

    // Decompose CRVAL2 (DEC degrees) into deg/min/sec + sign, wrapped to
    // -180..180.
    let Some(mut dec) = store.crval[1] else {
        return Ok(false);
    };
    while dec < -180.0 {
        dec += 360.0;
    }
    while dec >= 180.0 {
        dec -= 360.0;
    }
    let pltdecsn = if dec > 0.0 { "+" } else { "-" };
    if dec <= 0.0 {
        dec = -dec;
    }
    let pltdecd = dec.trunc();
    let dec_rem = 60.0 * (dec - pltdecd);
    let pltdecm = dec_rem.trunc();
    let pltdecs = 60.0 * (dec_rem - pltdecm);

    // Emit the keywords in C's order, commenting only the first of each group.
    cards.set_float("CNPIX1", cnpix1, "X corner (pixels)");
    cards.set_float("CNPIX2", cnpix2, "Y corner (pixels)");
    cards.set_float("PPO1", 0.0, "Orientation co-efficients");
    cards.set_float("PPO2", 0.0, "");
    cards.set_float("PPO3", ppo3, "");
    cards.set_float("PPO4", 0.0, "");
    cards.set_float("PPO5", 0.0, "");
    cards.set_float("PPO6", ppo6, "");
    cards.set_float("XPIXELSZ", xpixelsz, "X pixel size (microns)");
    cards.set_float("YPIXELSZ", ypixelsz, "Y pixel size (microns)");
    cards.set_float("PLTRAH", pltrah, "RA at plate centre");
    cards.set_float("PLTRAM", pltram, "");
    cards.set_float("PLTRAS", pltras, "");
    cards.set_float("PLTDECD", pltdecd, "DEC at plate centre");
    cards.set_float("PLTDECM", pltdecm, "");
    cards.set_float("PLTDECS", pltdecs, "");
    cards.set_string("PLTDECSN", pltdecsn, "");
    cards.set_float("PLTSCALE", pltscl, "Plate scale (arcsec/mm)");
    let mut comm = "Plate solution x co-efficients";
    for (i, &c) in amdx.iter().enumerate() {
        cards.set_float(&format!("AMDX{}", i + 1), c, comm);
        comm = "";
    }
    comm = "Plate solution y co-efficients";
    for (i, &c) in amdy.iter().enumerate() {
        cards.set_float(&format!("AMDY{}", i + 1), c, comm);
        comm = "";
    }

    Ok(true)
}

/// Inverse of [`set_pv_from_amd`]: fill the AMD coefficients from one axis's
/// PV parameters (`amd = 3600 * pv`). Mirrors C `DSSFromStore`
/// (fitschan.c:9336-9357). Indices 6 (AMD7) and 11 (AMD12) stay zero.
fn amd_from_pv(amd: &mut [f64; 20], pv: &[f64; 22]) {
    amd[0] = 3600.0 * pv[1];
    amd[1] = 3600.0 * pv[2];
    amd[2] = 3600.0 * pv[0];
    amd[3] = 3600.0 * pv[4];
    amd[4] = 3600.0 * pv[5];
    amd[5] = 3600.0 * pv[6];
    amd[7] = 3600.0 * pv[7];
    amd[8] = 3600.0 * pv[8];
    amd[9] = 3600.0 * pv[9];
    amd[10] = 3600.0 * pv[10];
    amd[12] = 3600.0 * pv[17];
}

/// Highest PV parameter index that is set, across both axes (C `GetMaxJM`).
/// Returns 0 when no PV term is set.
fn max_pv_index(store: &FitsStore) -> usize {
    let mut max = 0;
    for axis in 0..store.pv.len() {
        for (m, slot) in store.pv[axis].iter().enumerate() {
            if slot.is_some() {
                max = max.max(m);
            }
        }
    }
    max
}

/// AST's standard floating-point equality with tolerance, used by C's
/// `astEQUAL` in the related-parameter checks.
fn ast_equal(a: f64, b: f64) -> bool {
    crate::mappings::helpers::ast_equal(a, b)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a CardList from a standard DSS test header (subset of the
    /// wcsconv `dss.fits-dss` fixture).
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
AMDX1   =                67.23 / Plate solution x co-efficients
AMDX3   =                  1.36
AMDY1   =                67.23 / Plate solution y co-efficients
AMDY3   =                 0.74
END";
        CardList::from_header(header).unwrap()
    }

    #[test]
    fn read_dss_plate_centre() {
        let cards = dss_test_cards();
        let store = read_from_cards(&cards).unwrap();

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

        let expected_dec = -(60.0 + 15.0 / 60.0 + 51.85 / 3600.0);
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

        let expected_crpix1 = (-18825.9 / 25.28) - 1.0 + 0.5;
        let crpix1 = store.crpix[0].unwrap();
        assert!(
            (crpix1 - expected_crpix1).abs() < 1e-6,
            "CRPIX1: expected {expected_crpix1}, got {crpix1}"
        );

        let expected_crpix2 = (-22216.5 / 25.28) - 1.0 + 0.5;
        let crpix2 = store.crpix[1].unwrap();
        assert!(
            (crpix2 - expected_crpix2).abs() < 1e-6,
            "CRPIX2: expected {expected_crpix2}, got {crpix2}"
        );
    }

    #[test]
    fn read_dss_is_tpn_with_unit_cdelt() {
        // C `DSSToStore` always emits TPN with CDELT=1 and the pixel sizes
        // carried by the PC matrix (not a plain TAN with a scaled CDELT).
        let cards = dss_test_cards();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.naxis, 2);
        assert_eq!(store.ctype[0].as_deref(), Some("RA---TPN"));
        assert_eq!(store.ctype[1].as_deref(), Some("DEC--TPN"));
        assert_eq!(store.radesys.as_deref(), Some("FK5"));
        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.lonpole, Some(180.0));
        assert_eq!(store.cdelt[0], Some(1.0));
        assert_eq!(store.cdelt[1], Some(1.0));
        assert_eq!(store.pc[0][0], Some(-0.001 * 25.28));
        assert_eq!(store.pc[1][1], Some(0.001 * 25.28));
    }

    #[test]
    fn read_dss_pv_from_amd() {
        // AMDX1 -> PVX_1, AMDX3 -> PVX_0, all /3600 (fitschan.c:9595/9497).
        let cards = dss_test_cards();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.pv[0][1], Some(67.23 / 3600.0)); // amdx[0] -> pv[0][1]
        assert_eq!(store.pv[0][0], Some(1.36 / 3600.0)); // amdx[2] -> pv[0][0]
        assert_eq!(store.pv[1][1], Some(67.23 / 3600.0)); // amdy[0] -> pv[1][1]
        assert_eq!(store.pv[1][0], Some(0.74 / 3600.0)); // amdy[2] -> pv[1][0]
    }

    #[test]
    fn read_dss_default_cnpix() {
        let header = "\
PLTRAH  =                   12 / Plate centre RA hours
PLTRAM  =                    0 / Plate centre RA minutes
PLTRAS  =                  0.0 / Plate centre RA seconds
PLTDECD =                   45 / Plate centre DEC degrees
PLTDECM =                    0 / Plate centre DEC arcminutes
PLTDECS =                  0.0 / Plate centre DEC arcseconds
PLTDECSN= '+'
XPIXELSZ=                25.0  / Pixel size X (microns)
YPIXELSZ=                25.0  / Pixel size Y (microns)
PPO3    =              1000.0  / Plate offset X (microns)
PPO6    =              2000.0  / Plate offset Y (microns)
AMDX1   =                67.0  / Plate scale (arcsec/mm)
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // CRPIX1 = (1000/25) - 0 + 0.5 = 40.5
        assert!((store.crpix[0].unwrap() - 40.5).abs() < 1e-10);
        // CRPIX2 = (2000/25) - 0 + 0.5 = 80.5
        assert!((store.crpix[1].unwrap() - 80.5).abs() < 1e-10);
    }

    #[test]
    fn write_dss_ra_decomposition() {
        // RA = 161.98875 degrees = 10h 47m 57.3s
        let mut store = FitsStore::new(2);
        store.crval[0] = Some(161.98875);
        store.crval[1] = Some(45.0);
        store.crpix[0] = Some(512.0);
        store.crpix[1] = Some(512.0);
        store.cdelt[0] = Some(1.0);
        store.cdelt[1] = Some(1.0);
        store.pc[0][0] = Some(-0.001 * 25.0);
        store.pc[1][1] = Some(0.001 * 25.0);
        store.ctype[0] = Some("RA---TPN".to_string());
        store.ctype[1] = Some("DEC--TPN".to_string());
        store.radesys = Some("FK5".to_string());
        store.pv[0][1] = Some(67.0 / 3600.0);
        store.pv[1][1] = Some(67.0 / 3600.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        assert!((cards.get_float("PLTRAH").unwrap() - 10.0).abs() < 1e-10);
        assert!((cards.get_float("PLTRAM").unwrap() - 47.0).abs() < 1e-10);
        assert!((cards.get_float("PLTRAS").unwrap() - 57.3).abs() < 0.01);
    }

    #[test]
    fn write_dss_negative_dec_sign() {
        let mut store = tpn_store();
        store.crval[1] = Some(-60.264);
        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        assert_eq!(cards.get_string("PLTDECSN").unwrap(), "-");
        assert!((cards.get_float("PLTDECD").unwrap() - 60.0).abs() < 1e-10);
    }

    #[test]
    fn write_dss_positive_dec_sign() {
        let mut store = tpn_store();
        store.crval[1] = Some(45.5);
        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();
        assert_eq!(cards.get_string("PLTDECSN").unwrap(), "+");
    }

    #[test]
    fn write_dss_emits_full_amd_arrays() {
        // All 20 AMDX and 20 AMDY keywords must be written.
        let store = tpn_store();
        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();
        for i in 1..=20 {
            assert!(
                cards.get_float(&format!("AMDX{i}")).is_some(),
                "AMDX{i} missing"
            );
            assert!(
                cards.get_float(&format!("AMDY{i}")).is_some(),
                "AMDY{i} missing"
            );
        }
        // PPO1-6 present.
        for i in 1..=6 {
            assert!(
                cards.get_float(&format!("PPO{i}")).is_some(),
                "PPO{i} missing"
            );
        }
    }

    #[test]
    fn write_dss_rejects_rotation() {
        let mut store = tpn_store();
        store.pc[0][1] = Some(0.1);
        let mut cards = CardList::new();
        assert!(!write_to_cards(&store, &mut cards).unwrap());
        assert_eq!(cards.len(), 0);
    }

    #[test]
    fn write_dss_requires_2d() {
        let mut store = tpn_store();
        store.crpix.push(Some(1.0));
        let mut cards = CardList::new();
        assert!(!write_to_cards(&store, &mut cards).unwrap());
    }

    #[test]
    fn write_dss_requires_fk5_tan_or_tpn() {
        // A non-celestial / non-FK5 store cannot be DSS-encoded.
        let mut store = tpn_store();
        store.radesys = Some("GALACTIC".to_string());
        let mut cards = CardList::new();
        assert!(!write_to_cards(&store, &mut cards).unwrap());
    }

    #[test]
    fn read_dss_missing_keyword_error() {
        let header = "\
PLTRAH  =                   10 / Plate centre RA hours
PLTRAM  =                   47 / Plate centre RA minutes
PLTRAS  =                 57.3 / Plate centre RA seconds
PLTDECD =                   60 / Plate centre DEC degrees
PLTDECM =                   15 / Plate centre DEC arcminutes
PLTDECS =                51.85 / Plate centre DEC arcseconds
PLTDECSN= '-'
YPIXELSZ=                25.28 / Pixel size Y (microns)
PPO3    =            -18825.9  / Plate offset X (microns)
PPO6    =            -22216.5  / Plate offset Y (microns)
AMDX1   =                67.23 / Plate scale (arcsec/mm)
END";
        let cards = CardList::from_header(header).unwrap();
        let err = read_from_cards(&cards).unwrap_err();
        assert!(err.message().contains("XPIXELSZ"));
    }

    #[test]
    fn round_trip_preserves_coordinates() {
        // Write a TPN store, read it back, verify the plate-center coordinates.
        let mut original = tpn_store();
        original.crval[0] = Some(161.98875);
        original.crval[1] = Some(-60.264);

        let mut cards = CardList::new();
        write_to_cards(&original, &mut cards).unwrap();
        let recovered = read_from_cards(&cards).unwrap();

        assert!((original.crval[0].unwrap() - recovered.crval[0].unwrap()).abs() < 1.0 / 3600.0);
        assert!((original.crval[1].unwrap() - recovered.crval[1].unwrap()).abs() < 1.0 / 3600.0);
    }

    /// A minimal valid TPN DSS store (plate scale only).
    fn tpn_store() -> FitsStore {
        let mut store = FitsStore::new(2);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.crpix[0] = Some(512.0);
        store.crpix[1] = Some(512.0);
        store.cdelt[0] = Some(1.0);
        store.cdelt[1] = Some(1.0);
        store.pc[0][0] = Some(-0.001 * 25.0);
        store.pc[1][1] = Some(0.001 * 25.0);
        store.ctype[0] = Some("RA---TPN".to_string());
        store.ctype[1] = Some("DEC--TPN".to_string());
        store.radesys = Some("FK5".to_string());
        store.pv[0][1] = Some(67.0 / 3600.0);
        store.pv[1][1] = Some(67.0 / 3600.0);
        store
    }
}

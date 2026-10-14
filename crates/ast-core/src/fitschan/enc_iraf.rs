//! FITS-IRAF encoding reader/writer.
//!
//! IRAF-style WCS headers use the CD matrix (`CDi_j`) directly rather than
//! separate PC + CDELT. They also use the older `RADECSYS` keyword (a
//! historical misspelling) instead of `RADESYS`, and support legacy projection
//! codes NCP (mapped to SIN with specific PV parameters) and GLS (mapped to
//! SFL).
//!
//! This module provides:
//! - [`read_from_cards`]: extract IRAF keywords from a CardList into a FitsStore
//! - [`write_to_cards`]: write a FitsStore as IRAF-style keywords into a CardList

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read IRAF-style WCS keywords from a [`CardList`] into a [`FitsStore`].
///
/// The IRAF encoding differs from standard FITS-WCS in that:
/// - The CD matrix (`CDi_j`) is used directly (no separate PC + CDELT)
/// - `RADECSYS` is checked before `RADESYS`
/// - `EPOCH` is used as fallback for `EQUINOX`
/// - Legacy projection codes NCP and GLS are translated to SIN and SFL
/// - The 3-digit CD format (`CD001001`) is also checked
///
/// Now that `spec_trans` normalises FITS-IRAF keywords, the live
/// read path no longer calls this function. Kept for tests.
#[allow(dead_code)]
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    // Determine the number of axes from WCSAXES or NAXIS.
    // Rounded, not truncated — see the note in `wcs_keywords.rs`.
    let wcsaxes = cards.get_float("WCSAXES").map(|v| v.round() as usize);
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

        // CTYPE — translate NCP→SIN, GLS→SFL
        let key = format!("CTYPE{axis_num}");
        store.ctype[i] = cards
            .get_string(&key)
            .map(|ct| translate_iraf_ctype_read(&ct));

        // CUNIT
        let key = format!("CUNIT{axis_num}");
        store.cunit[i] = cards.get_string(&key);

        // CD matrix: try standard `CDi_j` format first, then 3-digit `CD00i00j`
        for j in 0..naxis {
            let col_num = j + 1;
            let key_std = format!("CD{axis_num}_{col_num}");
            let key_3digit = format!("CD{axis_num:03}{col_num:03}");
            store.cd[i][j] = cards
                .get_float(&key_std)
                .or_else(|| cards.get_float(&key_3digit));
        }
    }

    // RADECSYS (IRAF misspelling) with fallback to RADESYS (standard)
    store.radesys = cards
        .get_string("RADECSYS")
        .or_else(|| cards.get_string("RADESYS"));

    // EQUINOX with fallback to EPOCH
    store.equinox = cards
        .get_float("EQUINOX")
        .or_else(|| cards.get_float("EPOCH"));

    // Epoch of observation: TIMESYS + MJD-AVG / MJD-OBS / DATE-OBS /
    // DATE-AVG priority chain.
    super::wcs_keywords::read_epoch_into_store(cards, &mut store);

    // LONPOLE, LATPOLE (may be present even in IRAF headers)
    store.lonpole = cards.get_float("LONPOLE");
    store.latpole = cards.get_float("LATPOLE");

    // Spectral keywords
    store.specsys = cards.get_string("SPECSYS");
    store.restfrq = cards
        .get_float("RESTFRQ")
        .or_else(|| cards.get_float("RESTFREQ"));
    store.restwav = cards.get_float("RESTWAV");

    Ok(store)
}

/// Whether `store` can be written with FITS-IRAF: the checks C's
/// `IRAFFromStore` (fitschan.c:19126-19236) makes before writing, and the
/// CRPIX, CRVAL and CTYPE values it needs while writing. Celestial axes
/// need a projection other than TPN with no parameters, or a SIN one
/// equivalent to SIN or NCP; RA, galactic or ecliptic longitude; and no
/// LONPOLE or LATPOLE. There must be a CRPIX for each axis and no other.
fn iraf_store_ok(store: &FitsStore) -> bool {
    use super::wcs_celestial::wcs_prj_type;
    use crate::wcslib::WcsProjection as P;

    let mut ok = true;
    let (axlon, axlat, _) = store.lon_lat_spec_axes();
    if let (Some(axlon), Some(axlat)) = (axlon, axlat) {
        let (Some(lontype), Some(lattype)) =
            (store.ctype[axlon].as_deref(), store.ctype[axlat].as_deref())
        else {
            return false;
        };
        ok = match lattype.get(4..).and_then(wcs_prj_type) {
            None | Some(P::Tpn) => false,
            Some(P::Sin) => {
                let pv = |m: usize| {
                    store
                        .pv
                        .get(axlat)
                        .and_then(|a| a.get(m))
                        .copied()
                        .flatten()
                        .unwrap_or(0.0)
                };
                let (p1, p2) = (pv(1), pv(2));
                match store.crval[axlat] {
                    Some(val) => {
                        p1 == 0.0
                            && (p2 == 0.0
                                || (p2.abs() >= 1.0e14 && val == 0.0)
                                || (p2 * val.to_radians().tan() - 1.0).abs() < 0.01)
                    }
                    None => false,
                }
            }
            Some(_) => store.max_pv_m().is_none(),
        };
        if !(lontype.starts_with("RA--")
            || lontype.starts_with("GLON")
            || lontype.starts_with("ELON"))
        {
            ok = false;
        }
        if store.latpole.is_some() || store.lonpole.is_some() {
            ok = false;
        }
    }
    let naxis = store.max_crpix_axis().map_or(0, |m| m + 1);
    if naxis != store.naxis || !ok {
        return false;
    }
    (0..naxis).all(|i| store.crpix[i].is_some())
        && (0..naxis).all(|i| store.crval[i].is_some())
        && (0..naxis).all(|i| {
            store.ctype[i]
                .as_deref()
                .is_some_and(|c| !(c.len() > 4 && c.get(4..) == Some("-TAB")))
        })
}

/// Write a [`FitsStore`] as IRAF-style WCS keywords into a [`CardList`].
///
/// The IRAF encoding writes:
/// - CD matrix directly (computing `CD[i][j] = PC[i][j] * CDELT[i]` if needed)
/// - `RADECSYS` (the IRAF misspelling, not `RADESYS`)
/// - SFL projection is translated back to GLS
/// - No WCSAXES, LONPOLE, or LATPOLE keywords
///
/// `s` is the coordinate-version letter (`' '` primary, `'A'..'Z'`
/// alt). `is_last` indicates the alphabetically-last plane: only it
/// emits the observation-global keywords.
pub(crate) fn write_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<bool> {
    use super::wcs_keywords::key_for;
    if !iraf_store_ok(store) {
        return Ok(false);
    }
    let naxis = store.naxis;

    // CRPIXi
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = key_for("CRPIX", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Reference pixel on axis {}", i + 1));
        }
    }

    // CRVALi
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let key = key_for("CRVAL", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Value at ref. pixel on axis {}", i + 1));
        }
    }

    // CTYPEi — translate SFL→GLS and SIN-with-NCP-PV back to NCP for IRAF.
    let ncp_axes = super::wcs_keywords::sin_should_emit_as_ncp(store);
    for i in 0..naxis {
        if let Some(ref val) = store.ctype[i] {
            let mut iraf_ctype = translate_iraf_ctype_write(val);
            if ncp_axes && iraf_ctype.len() >= 8 && iraf_ctype[5..8].eq_ignore_ascii_case("SIN") {
                iraf_ctype.replace_range(5.., "NCP");
            }
            let key = key_for("CTYPE", Some(i + 1), s);
            let comment = store.ctype_com[i]
                .clone()
                .unwrap_or_else(|| format!("Type of co-ordinate on axis {}", i + 1));
            cards.set_string(&key, &iraf_ctype, &comment);
        }
    }

    // CD matrix: use CD directly if available, otherwise compute from PC × CDELT
    write_iraf_cd_matrix(store, cards, s);

    // CUNITi (mirror C IRAFFromStore: CUNIT is emitted after the CD matrix)
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = key_for("CUNIT", Some(i + 1), s);
            cards.set_string(&key, val, &format!("Units for axis {}", i + 1));
        }
    }

    // RADECSYS (the IRAF misspelling). Per-version-letter (truncated
    // to fit the 8-char limit when alt).
    if let Some(ref radesys) = store.radesys {
        let key = if s == ' ' {
            "RADECSYS".to_string()
        } else {
            format!("RADECSY{s}")
        };
        cards.set_string(&key, radesys, "Reference frame for RA/DEC values");
    }

    // EQUINOX
    if let Some(equinox) = store.equinox {
        let key = key_for("EQUINOX", None, s);
        cards.set_float(&key, equinox, "Epoch of reference equinox");
    }

    if is_last {
        // DATE-OBS — IRAF emits only DATE-OBS, not MJD-OBS (mirrors C
        // IRAFFromStore at fitschan.c:19199-19223). The pre-formatted
        // string in store.dateobs was derived from MJD-OBS upstream.
        if let Some(ref dateobs) = store.dateobs {
            cards.set_string("DATE-OBS", dateobs, "Date of observation");
        }
        // A read leaves any MJD-OBS card in the header, since other
        // software may use it; update it so that it agrees with DATE-OBS.
        if cards.find("MJD-OBS").is_some()
            && let Some(mjdobs) = store.mjdobs
        {
            cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
        }
    }

    Ok(true)
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Translate IRAF legacy projection codes on read:
/// - NCP → SIN (North Celestial Pole variant of SIN)
/// - GLS → SFL (Global Sinusoidal → Sanson-Flamsteed)
#[allow(dead_code)]
fn translate_iraf_ctype_read(ctype: &str) -> String {
    if ctype.len() >= 8 {
        let suffix = &ctype[5..8];
        if suffix.eq_ignore_ascii_case("NCP") {
            return format!("{}-SIN", &ctype[..4]);
        }
        if suffix.eq_ignore_ascii_case("GLS") {
            return format!("{}-SFL", &ctype[..4]);
        }
    }
    ctype.to_string()
}

/// Translate standard projections back to IRAF legacy codes on write:
/// - SFL → GLS
fn translate_iraf_ctype_write(ctype: &str) -> String {
    if ctype.len() >= 8 {
        let suffix = &ctype[5..8];
        if suffix.eq_ignore_ascii_case("SFL") {
            return format!("{}-GLS", &ctype[..4]);
        }
    }
    ctype.to_string()
}

/// Write the CD matrix: each row of the PC matrix multiplied by its CDELT
/// value, with an element that is zero left out (fitschan.c
/// `IRAFFromStore`).
fn write_iraf_cd_matrix(store: &FitsStore, cards: &mut CardList, s: char) {
    use super::wcs_keywords::key_for_2;
    let naxis = store.naxis;
    for i in 0..naxis {
        for j in 0..naxis {
            let cd_val = store.cd_element(i, j);
            if cd_val != 0.0 {
                let key = key_for_2("CD", i + 1, j + 1, s);
                cards.set_float(&key, cd_val, "Transformation matrix element");
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

    #[test]
    fn read_iraf_cd_matrix() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CRVAL1  =          180.000000  / Reference value
CRVAL2  =          -45.000000  / Reference value
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CD1_1   =         -0.001       / CD matrix
CD1_2   =          0.0         / CD matrix
CD2_1   =          0.0         / CD matrix
CD2_2   =          0.001       / CD matrix
RADECSYS= 'FK5'                / Reference frame
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.naxis, 2);
        assert_eq!(store.cd[0][0], Some(-0.001));
        assert_eq!(store.cd[0][1], Some(0.0));
        assert_eq!(store.cd[1][0], Some(0.0));
        assert_eq!(store.cd[1][1], Some(0.001));
        assert_eq!(store.radesys, Some("FK5".to_string()));
        assert_eq!(store.crpix[0], Some(512.0));
        assert_eq!(store.crpix[1], Some(512.0));
    }

    #[test]
    fn read_iraf_ncp_to_sin() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              100.0   / Reference pixel
CRPIX2  =              100.0   / Reference pixel
CRVAL1  =           10.000000  / Reference value
CRVAL2  =           20.000000  / Reference value
CTYPE1  = 'RA---NCP'           / Axis type
CTYPE2  = 'DEC--NCP'           / Axis type
CD1_1   =         -0.001       / CD matrix
CD2_2   =          0.001       / CD matrix
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // NCP should be translated to SIN
        assert_eq!(store.ctype[0], Some("RA---SIN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--SIN".to_string()));
    }

    #[test]
    fn read_iraf_gls_to_sfl() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              100.0   / Reference pixel
CRPIX2  =              100.0   / Reference pixel
CRVAL1  =           10.000000  / Reference value
CRVAL2  =           20.000000  / Reference value
CTYPE1  = 'GLON-GLS'           / Axis type
CTYPE2  = 'GLAT-GLS'           / Axis type
CD1_1   =         -0.01        / CD matrix
CD2_2   =          0.01        / CD matrix
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // GLS should be translated to SFL
        assert_eq!(store.ctype[0], Some("GLON-SFL".to_string()));
        assert_eq!(store.ctype[1], Some("GLAT-SFL".to_string()));
    }

    #[test]
    fn write_iraf_produces_cd() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(256.0);
        store.crpix[1] = Some(256.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-30.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.cdelt[0] = Some(-0.0005);
        store.cdelt[1] = Some(0.0005);
        store.radesys = Some("FK5".to_string());
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

        // Should have CD keywords
        assert_eq!(cards.get_float("CD1_1"), Some(-0.0005));
        assert_eq!(cards.get_float("CD2_2"), Some(0.0005));
        // Should use RADECSYS (not RADESYS)
        assert_eq!(cards.get_string("RADECSYS"), Some("FK5".to_string()));
        assert!(cards.get_string("RADESYS").is_none());
        // Should NOT have CDELT or PC keywords
        assert!(cards.get_float("CDELT1").is_none());
        assert!(cards.get_float("CDELT2").is_none());
        assert!(cards.get_float("PC1_1").is_none());
        // Should NOT have WCSAXES or LONPOLE
        assert!(cards.get_int("WCSAXES").is_none());
        assert!(cards.get_float("LONPOLE").is_none());
    }

    #[test]
    fn write_iraf_computes_cd_from_pc() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        store.crval[0] = Some(45.0);
        store.crval[1] = Some(60.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        // PC is a rotation by 30 degrees
        store.pc[0][0] = Some(0.866025); // cos(30)
        store.pc[0][1] = Some(-0.5); // -sin(30)
        store.pc[1][0] = Some(0.5); // sin(30)
        store.pc[1][1] = Some(0.866025); // cos(30)
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

        // CD = PC × CDELT: CD[i][j] = PC[i][j] × CDELT[i]
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
    }

    #[test]
    fn translate_sfl_to_gls() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        store.crval[0] = Some(0.0);
        store.crval[1] = Some(0.0);
        store.ctype[0] = Some("GLON-SFL".to_string());
        store.ctype[1] = Some("GLAT-SFL".to_string());
        store.cdelt[0] = Some(-0.01);
        store.cdelt[1] = Some(0.01);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

        // SFL should be translated to GLS on IRAF write
        assert_eq!(cards.get_string("CTYPE1"), Some("GLON-GLS".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("GLAT-GLS".to_string()));
    }

    #[test]
    fn read_iraf_3digit_cd_format() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              50.0    / Reference pixel
CRPIX2  =              50.0    / Reference pixel
CRVAL1  =           90.000000  / Reference value
CRVAL2  =            0.000000  / Reference value
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CD001001=         -0.002       / CD matrix
CD001002=          0.0         / CD matrix
CD002001=          0.0         / CD matrix
CD002002=          0.002       / CD matrix
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.cd[0][0], Some(-0.002));
        assert_eq!(store.cd[0][1], Some(0.0));
        assert_eq!(store.cd[1][0], Some(0.0));
        assert_eq!(store.cd[1][1], Some(0.002));
    }

    #[test]
    fn read_iraf_epoch_fallback() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              100.0   / Reference pixel
CRPIX2  =              100.0   / Reference pixel
CRVAL1  =           10.000000  / Reference value
CRVAL2  =           20.000000  / Reference value
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CD1_1   =         -0.001       / CD matrix
CD2_2   =          0.001       / CD matrix
EPOCH   =             1950.0   / Epoch of observation
RADECSYS= 'FK4'                / Reference frame
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.equinox, Some(1950.0));
        assert_eq!(store.radesys, Some("FK4".to_string()));
    }
}

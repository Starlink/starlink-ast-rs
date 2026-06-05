//! FITS-PC encoding reader/writer.
//!
//! The FITS-PC encoding is the pre-2002 WCS standard that uses:
//! - **3-digit axis numbering** for the PC matrix: `PC001001`, `PC001002`, etc.
//!   (8 chars total: "PC" + 3-digit row + 3-digit column)
//! - **Separate CDELT** from the PC matrix (unlike IRAF which combines them
//!   into a single CD matrix)
//! - **`RADECSYS`** (historical misspelling) instead of `RADESYS`
//! - **Older projection codes**: GLS (= SFL), ATF (= AIT)
//! - Also accepts the `PCi_j` underscore format on read for tolerance
//!
//! This module provides:
//! - [`read_from_cards`]: extract FITS-PC keywords from a CardList into a FitsStore
//! - [`write_to_cards`]: write a FitsStore as FITS-PC keywords into a CardList

use ndarray::Array2;

use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::wcs_decompose::split_cd;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read FITS-PC style WCS keywords from a [`CardList`] into a [`FitsStore`].
///
/// The FITS-PC encoding differs from standard FITS-WCS in that:
/// - The PC matrix uses 3-digit numbering (`PC001001`) or the underscore
///   format (`PC1_1`) as a fallback
/// - `CDELT` is always separate from the PC matrix
/// - `RADECSYS` (misspelling) is checked before `RADESYS`
/// - Legacy projection codes GLS and ATF are translated to SFL and AIT
///
/// Now that `spec_trans` normalises FITS-PC keywords on the read side
/// of every encoder, this single-plane reader is only kept for
/// historical / testing access; the live read path runs through
/// `wcs_keywords::extract_wcs_keywords_with_alts`.
#[allow(dead_code)]
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

        // CTYPE — translate GLS→SFL, ATF→AIT
        let key = format!("CTYPE{axis_num}");
        store.ctype[i] = cards
            .get_string(&key)
            .map(|ct| translate_pc_ctype_read(&ct));

        // CUNIT
        let key = format!("CUNIT{axis_num}");
        store.cunit[i] = cards.get_string(&key);

        // PC matrix: try 3-digit format first (`PC001001`), then underscore (`PC1_1`)
        for j in 0..naxis {
            let col_num = j + 1;
            let key_3digit = format!("PC{axis_num:03}{col_num:03}");
            let key_underscore = format!("PC{axis_num}_{col_num}");
            store.pc[i][j] = cards
                .get_float(&key_3digit)
                .or_else(|| cards.get_float(&key_underscore));
        }
    }

    // RADECSYS (PC misspelling) with fallback to RADESYS (standard)
    store.radesys = cards
        .get_string("RADECSYS")
        .or_else(|| cards.get_string("RADESYS"));

    // EQUINOX
    store.equinox = cards.get_float("EQUINOX");

    // Epoch of observation: TIMESYS + MJD-AVG / MJD-OBS / DATE-OBS /
    // DATE-AVG priority chain.
    super::wcs_keywords::read_epoch_into_store(cards, &mut store);

    // LONPOLE, LATPOLE
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

/// Write a [`FitsStore`] as FITS-PC style WCS keywords into a [`CardList`].
///
/// The FITS-PC encoding writes:
/// - `CRPIX`, `CRVAL`, `CDELT` per axis
/// - PC matrix in 3-digit format (`PC001001`)
/// - `RADECSYS` (the historical misspelling, not `RADESYS`)
/// - SFL projection is translated back to GLS; AIT to ATF
///
/// `s` is the coordinate-version letter (`' '` primary, `'A'..='Z'`
/// alt). Per-axis cards are emitted with the suffix appended.
/// `is_last` indicates whether this is the alphabetically-last
/// plane: only that plane emits the observation-global keywords
/// (MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS, DATE-OBS) — last-plane-wins
/// per the C overwrite contract at fitschan.c:29805.
pub(crate) fn write_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<()> {
    use super::wcs_keywords::key_for;
    let naxis = store.naxis;

    // Derive CDELT and PC. If the store has PC + CDELT directly, use them.
    // Otherwise factor a CD matrix via split_cd.
    let (cdelt_vals, pc_vals) = if store.has_pc_matrix() || store.cdelt.iter().any(|v| v.is_some())
    {
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
        let cdelt = vec![1.0; naxis];
        let pc = Array2::eye(naxis);
        (cdelt, pc)
    };

    // PC matrix in 3-digit format. Identity values (1.0 on diagonal,
    // 0.0 off-diagonal) are skipped — mirrors fitschan.c:24586-24598.
    // Card emission order matches PCFromStore (line 24553+): PC, CDELT,
    // CRPIX, CRVAL, CTYPE, CUNIT, RADECSYS, EQUINOX, MJD-OBS, DATE-OBS.
    for i in 0..naxis {
        for j in 0..naxis {
            let val = pc_vals[[i, j]];
            let default = if i == j { 1.0 } else { 0.0 };
            if (val - default).abs() > 1e-15 {
                let key = if s == ' ' {
                    format!("PC{:03}{:03}", i + 1, j + 1)
                } else {
                    format!("PC{:03}{:03}{s}", i + 1, j + 1)
                };
                // FITS-PC PC cards carry no comment (fitschan.c:24595).
                cards.set_float(&key, val, "");
            }
        }
    }

    // CDELTi.
    for (i, &cdelt_val) in cdelt_vals.iter().enumerate() {
        let key = key_for("CDELT", Some(i + 1), s);
        cards.set_float(&key, cdelt_val, &format!("Pixel scale on axis {}", i + 1));
    }

    // CRPIXi.
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = key_for("CRPIX", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Reference pixel on axis {}", i + 1));
        }
    }

    // CRVALi.
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let key = key_for("CRVAL", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Value at ref. pixel on axis {}", i + 1));
        }
    }

    // CTYPEi — translate SFL→GLS, AIT→ATF.
    for i in 0..naxis {
        if let Some(ref val) = store.ctype[i] {
            let pc_ctype = translate_pc_ctype_write(val);
            let key = key_for("CTYPE", Some(i + 1), s);
            let comment = store.ctype_com[i]
                .clone()
                .unwrap_or_else(|| format!("Type of co-ordinate on axis {}", i + 1));
            cards.set_string(&key, &pc_ctype, &comment);
        }
    }

    // CUNITi.
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = key_for("CUNIT", Some(i + 1), s);
            cards.set_string(&key, val, &format!("Units for axis {}", i + 1));
        }
    }

    // RADECSYS (the PC misspelling). Per-version-letter.
    if let Some(ref radesys) = store.radesys {
        let key = if s == ' ' {
            "RADECSYS".to_string()
        } else {
            format!("RADECSY{s}") // 8-char limit: RADECSYS + s would overflow
        };
        cards.set_string(&key, radesys, "Reference frame for RA/DEC values");
    }

    // EQUINOX. Per-version-letter.
    if let Some(equinox) = store.equinox {
        let key = key_for("EQUINOX", None, s);
        cards.set_float(&key, equinox, "Epoch of reference equinox");
    }

    // LATPOLE — fitschan.c:24782-24786. Mirrors C: written by primary
    // axis only (s == ' ').
    if s == ' '
        && let Some(latpole) = store.latpole
    {
        cards.set_float("LATPOLE", latpole, "Latitude of native north pole");
    }

    // LONGPOLE — fitschan.c:24796-24800. Note C uses keyword "LONGPOLE"
    // (not "LONPOLE") for FITS-PC encoding.
    if s == ' '
        && let Some(lonpole) = store.lonpole
    {
        cards.set_float("LONGPOLE", lonpole, "Longitude of native north pole");
    }

    // Observation-global cards: only emitted on the last plane.
    if is_last {
        if let Some(mjdobs) = store.mjdobs {
            cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
        }
        if let Some(ref dateobs) = store.dateobs {
            cards.set_string("DATE-OBS", dateobs, "Date of observation");
        }
    }

    // Old-style PROJPm projection parameters — only on the latitude
    // axis, only for the primary axis description, and only when the
    // projection isn't TAN. Mirrors fitschan.c:24862-24887.
    //
    // PC encoding ignores PVs on the longitude axis. PVs on any axis
    // other than the celestial latitude/longitude pair, or PVs at
    // m >= 10 on the latitude axis, would cause C to fall back to
    // FITS-WCS encoding (the PCFromStore returns 0). We don't model
    // that fallback here yet — projects with such PV layouts are
    // already known not to round-trip via FITS-PC.
    if s == ' '
        && let Some((_axlon, axlat)) = store.celestial_axes()
    {
        // Determine projection from CTYPE on the latitude axis.
        let prj_is_tan = store
            .ctype
            .get(axlat)
            .and_then(|c| c.as_deref())
            .is_some_and(|ct| ct.len() >= 8 && ct[5..8].eq_ignore_ascii_case("TAN"));
        if !prj_is_tan && axlat < store.pv.len() {
            for m in 0..store.pv[axlat].len().min(10) {
                if let Some(val) = store.pv[axlat][m] {
                    let key = format!("PROJP{m}");
                    cards.set_float(&key, val, "Projection parameter");
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Translate FITS-PC legacy projection codes on read:
/// - GLS → SFL (Global Sinusoidal → Sanson-Flamsteed)
/// - ATF → AIT (old Aitoff → Hammer-Aitoff)
#[allow(dead_code)]
fn translate_pc_ctype_read(ctype: &str) -> String {
    if ctype.len() >= 8 {
        let suffix = &ctype[5..8];
        if suffix.eq_ignore_ascii_case("GLS") {
            return format!("{}-SFL", &ctype[..4]);
        }
        if suffix.eq_ignore_ascii_case("ATF") {
            return format!("{}-AIT", &ctype[..4]);
        }
    }
    ctype.to_string()
}

/// Translate standard projections back to FITS-PC legacy codes on write:
/// - SFL → GLS
/// - AIT → ATF
fn translate_pc_ctype_write(ctype: &str) -> String {
    if ctype.len() >= 8 {
        let suffix = &ctype[5..8];
        if suffix.eq_ignore_ascii_case("SFL") {
            return format!("{}-GLS", &ctype[..4]);
        }
        if suffix.eq_ignore_ascii_case("AIT") {
            return format!("{}-ATF", &ctype[..4]);
        }
    }
    ctype.to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_pc_3digit_format() {
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
PC001001=              1.0     / PC matrix
PC001002=              0.0     / PC matrix
PC002001=              0.0     / PC matrix
PC002002=              1.0     / PC matrix
RADECSYS= 'FK5'                / Reference frame
EQUINOX =             2000.0   / Equinox
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        assert_eq!(store.naxis, 2);
        assert_eq!(store.pc[0][0], Some(1.0));
        assert_eq!(store.pc[0][1], Some(0.0));
        assert_eq!(store.pc[1][0], Some(0.0));
        assert_eq!(store.pc[1][1], Some(1.0));
        assert_eq!(store.cdelt[0], Some(-0.0002778));
        assert_eq!(store.cdelt[1], Some(0.0002778));
        assert_eq!(store.radesys, Some("FK5".to_string()));
        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.crpix[0], Some(512.0));
        assert_eq!(store.crpix[1], Some(512.0));
    }

    #[test]
    fn read_pc_underscore_fallback() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              256.0   / Reference pixel
CRPIX2  =              256.0   / Reference pixel
CRVAL1  =           90.000000  / Reference value
CRVAL2  =            0.000000  / Reference value
CDELT1  =         -0.001       / Pixel scale
CDELT2  =          0.001       / Pixel scale
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
PC1_1   =              1.0     / PC matrix
PC1_2   =              0.0     / PC matrix
PC2_1   =              0.0     / PC matrix
PC2_2   =              1.0     / PC matrix
RADESYS = 'FK5'                / Reference frame
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // Underscore format should still be read correctly
        assert_eq!(store.pc[0][0], Some(1.0));
        assert_eq!(store.pc[0][1], Some(0.0));
        assert_eq!(store.pc[1][0], Some(0.0));
        assert_eq!(store.pc[1][1], Some(1.0));
        assert_eq!(store.cdelt[0], Some(-0.001));
        assert_eq!(store.cdelt[1], Some(0.001));
        assert_eq!(store.radesys, Some("FK5".to_string()));
    }

    #[test]
    fn read_pc_gls_to_sfl() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              100.0   / Reference pixel
CRPIX2  =              100.0   / Reference pixel
CRVAL1  =           10.000000  / Reference value
CRVAL2  =           20.000000  / Reference value
CDELT1  =         -0.01        / Pixel scale
CDELT2  =          0.01        / Pixel scale
CTYPE1  = 'GLON-GLS'           / Axis type
CTYPE2  = 'GLAT-GLS'           / Axis type
PC001001=              1.0     / PC matrix
PC002002=              1.0     / PC matrix
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // GLS should be translated to SFL
        assert_eq!(store.ctype[0], Some("GLON-SFL".to_string()));
        assert_eq!(store.ctype[1], Some("GLAT-SFL".to_string()));
    }

    #[test]
    fn write_pc_3digit() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(512.0);
        store.crpix[1] = Some(512.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-45.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.cdelt[0] = Some(-0.0002778);
        store.cdelt[1] = Some(0.0002778);
        // Rotation by 30 degrees
        store.pc[0][0] = Some(0.866025);
        store.pc[0][1] = Some(-0.5);
        store.pc[1][0] = Some(0.5);
        store.pc[1][1] = Some(0.866025);
        store.radesys = Some("FK5".to_string());
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

        // Should have PC keywords in 3-digit format
        assert!(cards.get_float("PC001001").is_some());
        assert!(cards.get_float("PC001002").is_some());
        assert!(cards.get_float("PC002001").is_some());
        assert!(cards.get_float("PC002002").is_some());
        // Should NOT have underscore PC format
        assert!(cards.get_float("PC1_1").is_none());
        // Should have CDELTi
        assert_eq!(cards.get_float("CDELT1"), Some(-0.0002778));
        assert_eq!(cards.get_float("CDELT2"), Some(0.0002778));
        // Should use RADECSYS (not RADESYS)
        assert_eq!(cards.get_string("RADECSYS"), Some("FK5".to_string()));
        assert!(cards.get_string("RADESYS").is_none());
        // Should NOT have CD keywords
        assert!(cards.get_float("CD1_1").is_none());
    }

    #[test]
    fn write_pc_from_cd() {
        // Store has only CD matrix (no PC/CDELT) — should factor via split_cd.
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(256.0);
        store.crpix[1] = Some(256.0);
        store.crval[0] = Some(45.0);
        store.crval[1] = Some(60.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        // Diagonal CD matrix (no rotation)
        store.cd[0][0] = Some(-0.001);
        store.cd[1][1] = Some(0.001);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

        // Should have CDELTi (factored from CD)
        let cdelt1 = cards.get_float("CDELT1").unwrap();
        let cdelt2 = cards.get_float("CDELT2").unwrap();
        assert!((cdelt1 - (-0.001)).abs() < 1e-12);
        assert!((cdelt2 - 0.001).abs() < 1e-12);

        // For a pure diagonal CD, the PC matrix is identity, so no PC keywords
        // should be written (identity is the default).
        assert!(cards.get_float("PC001001").is_none());
        assert!(cards.get_float("PC001002").is_none());
        assert!(cards.get_float("PC002001").is_none());
        assert!(cards.get_float("PC002002").is_none());

        // Should NOT have CD keywords in output
        assert!(cards.get_float("CD1_1").is_none());
    }

    #[test]
    fn write_pc_sfl_to_gls() {
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

        // SFL should be translated to GLS on PC write
        assert_eq!(cards.get_string("CTYPE1"), Some("GLON-GLS".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("GLAT-GLS".to_string()));
    }

    #[test]
    fn read_pc_atf_to_ait() {
        let header = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              100.0   / Reference pixel
CRPIX2  =              100.0   / Reference pixel
CRVAL1  =            0.000000  / Reference value
CRVAL2  =            0.000000  / Reference value
CDELT1  =         -1.0         / Pixel scale
CDELT2  =          1.0         / Pixel scale
CTYPE1  = 'GLON-ATF'           / Axis type
CTYPE2  = 'GLAT-ATF'           / Axis type
PC001001=              1.0     / PC matrix
PC002002=              1.0     / PC matrix
END";
        let cards = CardList::from_header(header).unwrap();
        let store = read_from_cards(&cards).unwrap();

        // ATF should be translated to AIT
        assert_eq!(store.ctype[0], Some("GLON-AIT".to_string()));
        assert_eq!(store.ctype[1], Some("GLAT-AIT".to_string()));
    }

    #[test]
    fn write_pc_identity_not_written() {
        // When PC is identity, no PC keywords should be written.
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(50.0);
        store.crpix[1] = Some(50.0);
        store.crval[0] = Some(10.0);
        store.crval[1] = Some(20.0);
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        // Explicitly set identity PC
        store.pc[0][0] = Some(1.0);
        store.pc[1][1] = Some(1.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

        // Identity PC values should not be written
        assert!(cards.get_float("PC001001").is_none());
        assert!(cards.get_float("PC001002").is_none());
        assert!(cards.get_float("PC002001").is_none());
        assert!(cards.get_float("PC002002").is_none());
    }
}

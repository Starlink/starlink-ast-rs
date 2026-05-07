//! Encoding auto-detection for FITS headers.
//!
//! Examines keyword signatures in a [`CardList`] to determine which WCS
//! encoding format the header uses. Detection priority matches the C
//! library's `GetEncoding` function (fitschan.c line 12466).

use super::Encoding;
use super::card::FitsValue;
use super::card_list::CardList;

/// Examine keyword signatures to determine the WCS encoding of a header.
///
/// Detection priority (earlier matches take precedence):
/// 1. Native (AST comment markers)
/// 2. FITS-CLASS
/// 3. FITS-AIPS++
/// 4. FITS-AIPS (spectral)
/// 5. DSS
/// 6. FITS-PC (3-digit format)
/// 7. FITS-IRAF (3-digit CD format)
/// 8. FITS-IRAF (CDi_j + RADECSYS/PROJPi)
/// 9. FITS-WCS (CDi_j without old-style)
/// 10. FITS-PC (RADECSYS/PROJPi without matrix)
/// 11. FITS-AIPS (CROTAi)
/// 12. FITS-WCS (any CRVALi)
/// 13. Native (fallback)
pub(crate) fn guess_encoding(cards: &CardList) -> Encoding {
    if has_native_markers(cards) {
        return Encoding::Native;
    }

    let has_wcs = has_wcs_indicator(cards);

    if has_wcs && is_class(cards) {
        return Encoding::FitsClass;
    }

    let aips_spectral = has_aips_spectral_ctype(cards);
    let has_cd = has_cd_matrix(cards);
    let has_projp = has_projp_keyword(cards);

    if has_wcs && aips_spectral && (has_cd || has_projp || has_lonlatpole(cards)) {
        return Encoding::FitsAipsPP;
    }

    if has_wcs && aips_spectral {
        return Encoding::FitsAips;
    }

    if has_dss(cards) {
        return Encoding::Dss;
    }

    if has_wcs && has_pc_3digit(cards) {
        return Encoding::FitsPc;
    }

    if has_wcs && has_cd_3digit(cards) {
        return Encoding::FitsIraf;
    }

    if has_wcs && has_cd && (has_radecsys_old(cards) || has_projp) {
        return Encoding::FitsIraf;
    }

    if has_wcs && has_cd {
        return Encoding::FitsWcs;
    }

    if has_wcs && (has_radecsys_old(cards) || has_projp) && !has_pc_matrix(cards) {
        return Encoding::FitsPc;
    }

    if has_wcs && has_crota(cards) {
        return Encoding::FitsAips;
    }

    if has_wcs && has_crval(cards) {
        return Encoding::FitsWcs;
    }

    Encoding::Native
}

/// Return true if a CTYPE value represents an AIPS spectral axis.
///
/// The value must be exactly 8 characters with first 4 being one of
/// `FREQ`/`VELO`/`FELO`, character 5 being `-`, and the last 3 being
/// a reference frame: `LSR`/`LSD`/`HEL`/`GEO`/`EAR`/`OBS`/`TOP`.
pub(crate) fn is_aips_spectral(ctype: &str) -> bool {
    if ctype.len() != 8 {
        return false;
    }

    let prefix = &ctype[..4];
    if prefix != "FREQ" && prefix != "VELO" && prefix != "FELO" {
        return false;
    }

    if ctype.as_bytes()[4] != b'-' {
        return false;
    }

    let frame = &ctype[5..8];
    matches!(frame, "LSR" | "LSD" | "HEL" | "GEO" | "EAR" | "OBS" | "TOP")
}

/// Check for AST native encoding markers in COMMENT cards.
fn has_native_markers(cards: &CardList) -> bool {
    cards.iter().any(|card| {
        if card.keyword == "COMMENT" {
            if let FitsValue::Comment(text) = &card.value {
                return text.contains("AST +++");
            }
        }
        false
    })
}

/// Check for basic WCS presence: CTYPE1, CRPIX1, or CRVAL1.
fn has_wcs_indicator(cards: &CardList) -> bool {
    cards.find("CTYPE1").is_some()
        || cards.find("CRPIX1").is_some()
        || cards.find("CRVAL1").is_some()
}

/// Check for FITS-CLASS encoding: DELTAV + velocity keyword.
fn is_class(cards: &CardList) -> bool {
    if cards.find("DELTAV").is_none() {
        return false;
    }
    cards.find("VLSR").is_some()
        || cards.find("VELO-LSR").is_some()
        || cards.find("VELO-OBS").is_some()
        || cards.find("VELO-HEL").is_some()
        || cards.find("VELO-EAR").is_some()
}

/// Check whether any CTYPEi has an AIPS spectral value.
fn has_aips_spectral_ctype(cards: &CardList) -> bool {
    for i in 1..=99 {
        let key = format!("CTYPE{i}");
        if let Some(val) = cards.get_string(&key) {
            if is_aips_spectral(&val) {
                return true;
            }
        }
    }
    false
}

/// Check for CDi_j matrix keywords (e.g. CD1_1, CD2_1).
fn has_cd_matrix(cards: &CardList) -> bool {
    for i in 1..=9 {
        for j in 1..=9 {
            let key = format!("CD{i}_{j}");
            if cards.find(&key).is_some() {
                return true;
            }
        }
    }
    false
}

/// Check for PROJPi keywords (e.g. PROJP1, PROJP2).
fn has_projp_keyword(cards: &CardList) -> bool {
    for i in 0..=99 {
        let key = format!("PROJP{i}");
        if cards.find(&key).is_some() {
            return true;
        }
    }
    false
}

/// Check for LONPOLE or LATPOLE keywords.
fn has_lonlatpole(cards: &CardList) -> bool {
    cards.find("LONPOLE").is_some() || cards.find("LATPOLE").is_some()
}

/// Check for DSS encoding: PLTRAH keyword.
fn has_dss(cards: &CardList) -> bool {
    cards.find("PLTRAH").is_some()
}

/// Check for 3-digit PC matrix format (PC001001 style).
///
/// A keyword matches if it is exactly 8 characters, starts with "PC",
/// and the remaining 6 characters are all digits.
fn has_pc_3digit(cards: &CardList) -> bool {
    cards.iter().any(|card| is_pc_3digit_keyword(&card.keyword))
}

/// Check whether a keyword is a 3-digit PC keyword (e.g. PC001001).
fn is_pc_3digit_keyword(keyword: &str) -> bool {
    keyword.len() == 8
        && keyword.starts_with("PC")
        && keyword[2..].bytes().all(|b| b.is_ascii_digit())
}

/// Check for 3-digit CD matrix format (CD001001 style).
fn has_cd_3digit(cards: &CardList) -> bool {
    cards.iter().any(|card| is_cd_3digit_keyword(&card.keyword))
}

/// Check whether a keyword is a 3-digit CD keyword (e.g. CD001001).
fn is_cd_3digit_keyword(keyword: &str) -> bool {
    keyword.len() == 8
        && keyword.starts_with("CD")
        && keyword[2..].bytes().all(|b| b.is_ascii_digit())
}

/// Check for old-style RADECSYS keyword (misspelled, missing the 'A').
fn has_radecsys_old(cards: &CardList) -> bool {
    cards.find("RADECSYS").is_some()
}

/// Check for PCi_j matrix keywords (e.g. PC1_1, PC2_1).
fn has_pc_matrix(cards: &CardList) -> bool {
    for i in 1..=9 {
        for j in 1..=9 {
            let key = format!("PC{i}_{j}");
            if cards.find(&key).is_some() {
                return true;
            }
        }
    }
    false
}

/// Check for CROTAi keywords (e.g. CROTA1, CROTA2).
fn has_crota(cards: &CardList) -> bool {
    for i in 1..=99 {
        let key = format!("CROTA{i}");
        if cards.find(&key).is_some() {
            return true;
        }
    }
    false
}

/// Check for any CRVALi keyword.
fn has_crval(cards: &CardList) -> bool {
    for i in 1..=99 {
        let key = format!("CRVAL{i}");
        if cards.find(&key).is_some() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_fitswcs_cdelt() {
        let header = "\
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CRVAL1  =              180.0   / Reference value
CRVAL2  =              -45.0   / Reference value
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CDELT1  =           -0.00028   / Pixel scale
CDELT2  =            0.00028   / Pixel scale
RADESYS = 'FK5'                / Reference frame";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::FitsWcs);
    }

    #[test]
    fn detect_fitswcs_cd_matrix() {
        let header = "\
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CRVAL1  =              180.0   / Reference value
CRVAL2  =              -45.0   / Reference value
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CD1_1   =           -0.00028   / CD matrix
CD2_2   =            0.00028   / CD matrix
RADESYS = 'FK5'                / Reference frame";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::FitsWcs);
    }

    #[test]
    fn detect_iraf_radecsys() {
        let header = "\
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CRVAL1  =              180.0   / Reference value
CRVAL2  =              -45.0   / Reference value
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CD1_1   =           -0.00028   / CD matrix
CD2_2   =            0.00028   / CD matrix
RADECSYS= 'FK5'                / Old-style reference frame";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::FitsIraf);
    }

    #[test]
    fn detect_pc_3digit() {
        let header = "\
CTYPE1  = 'RA---TAN'           / Axis type
CRVAL1  =              180.0   / Reference value
CRPIX1  =              512.0   / Reference pixel
PC001001=                  1.0 / PC matrix element";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::FitsPc);
    }

    #[test]
    fn detect_aips_crota() {
        let header = "\
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CRVAL1  =              180.0   / Reference value
CRVAL2  =              -45.0   / Reference value
CRPIX1  =              512.0   / Reference pixel
CRPIX2  =              512.0   / Reference pixel
CDELT1  =           -0.00028   / Pixel scale
CDELT2  =            0.00028   / Pixel scale
CROTA2  =                0.0   / Rotation
EPOCH   =             2000.0   / Epoch of observation";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::FitsAips);
    }

    #[test]
    fn detect_aips_spectral() {
        let header = "\
CTYPE1  = 'VELO-LSR'           / Axis type
CRVAL1  =            10000.0   / Reference value
CRPIX1  =                1.0   / Reference pixel
CDELT1  =             1000.0   / Channel width";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::FitsAips);
    }

    #[test]
    fn detect_aipspp_spectral_with_cd() {
        let header = "\
CTYPE1  = 'VELO-LSR'           / Axis type
CRVAL1  =            10000.0   / Reference value
CRPIX1  =                1.0   / Reference pixel
CD1_1   =             1000.0   / CD matrix";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::FitsAipsPP);
    }

    #[test]
    fn detect_class() {
        let header = "\
CTYPE1  = 'RA---TAN'           / Axis type
CRVAL1  =              180.0   / Reference value
CRPIX1  =              512.0   / Reference pixel
DELTAV  =             1000.0   / Velocity increment
VELO-LSR=            10000.0   / Source velocity";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::FitsClass);
    }

    #[test]
    fn detect_dss() {
        let header = "\
PLTRAH  =                   12 / Plate center RA hours
PLTRAM  =                   30 / Plate center RA minutes
PLTRAS  =                 0.0  / Plate center RA seconds
PLTDECSN= '+'                  / Plate center Dec sign";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::Dss);
    }

    #[test]
    fn detect_native() {
        let header = "\
COMMENT  AST +++++++++++++++++++++++++++++++++ AST
COMMENT  AST    Begin ZoomMap                   AST
COMMENT  AST --------------------------------- AST";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::Native);
    }

    #[test]
    fn is_aips_spectral_valid() {
        assert!(is_aips_spectral("VELO-LSR"));
        assert!(is_aips_spectral("FREQ-OBS"));
        assert!(is_aips_spectral("FELO-HEL"));
        assert!(is_aips_spectral("VELO-TOP"));
        assert!(is_aips_spectral("FREQ-GEO"));
        assert!(is_aips_spectral("FELO-EAR"));
        assert!(is_aips_spectral("VELO-LSD"));

        assert!(!is_aips_spectral("VELO-XYZ"));
        assert!(!is_aips_spectral("RA---TAN"));
        assert!(!is_aips_spectral("VELO"));
        assert!(!is_aips_spectral("VELOCITY"));
        assert!(!is_aips_spectral("FREQ+LSR"));
    }

    #[test]
    fn fallback_to_native() {
        let header = "\
SIMPLE  =                    T / Standard FITS
BITPIX  =                  -32 / Bits per pixel";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::Native);
    }
}

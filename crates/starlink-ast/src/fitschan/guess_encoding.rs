// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Encoding auto-detection for FITS headers: C's `GetEncoding`
//! (fitschan.c), which looks for keyword signatures among the cards a read
//! has not consumed.

use super::Encoding;
use super::card_list::CardList;
use super::card_value::{Got, Want, cnv_type};

/// The encoding C's `GetEncoding` chooses when none is set, from the
/// keywords present (`astKeyFields`, which skips cards a read consumed).
pub(crate) fn guess_encoding(cards: &CardList) -> Encoding {
    let kf = |template: &str| cards.key_fields(template, 0).0 > 0;
    let haswcs = kf("CTYPE%d%0c") || kf("CRPIX%d%0c") || kf("CRVAL%d%0c");
    let hascd = kf("CD%1d_%1d%0c");
    let haspc = kf("PC%1d_%1d%0c");
    // RADECSYS, PROJPi or CmVALi without their FITS-WCS equivalents.
    let old_style = || {
        (kf("RADECSYS") && !kf("RADESYS")) || (kf("PROJP%d") && !kf("PV%d_%d")) || kf("C%1dVAL%d")
    };
    if kf("BEGAST%2f") {
        Encoding::Native
    } else if haswcs && looks_like_class(cards) {
        Encoding::FitsClass
    } else if haswcs && has_aips_spec_axis(cards) {
        if hascd || kf("PROJP%d") || kf("LONPOLE") || kf("LATPOLE") {
            Encoding::FitsAipsPP
        } else {
            Encoding::FitsAips
        }
    } else if kf("PLTRAH") {
        Encoding::Dss
    } else if haswcs && kf("PC%3d%3d") {
        Encoding::FitsPc
    } else if haswcs && kf("CD%3d%3d") {
        Encoding::FitsIraf
    } else if haswcs && hascd {
        if old_style() {
            Encoding::FitsIraf
        } else {
            Encoding::FitsWcs
        }
    } else if haswcs && !haspc && !hascd && old_style() {
        Encoding::FitsPc
    } else if haswcs && kf("CROTA%d") {
        Encoding::FitsAips
    } else if haswcs && kf("CRVAL%d%0c") {
        Encoding::FitsWcs
    } else {
        Encoding::Native
    }
}

/// C's `LooksLikeClass`: a DELTAV keyword and a velocity keyword.
fn looks_like_class(cards: &CardList) -> bool {
    let kf = |template: &str| cards.key_fields(template, 0).0 > 0;
    kf("DELTAV")
        && (kf("VLSR") || kf("VELO-OBS") || kf("VELO-HEL") || kf("VELO-EAR") || kf("VELO-LSR"))
}

/// C's `HasAIPSSpecAxis`: whether any of the single-digit CTYPE keywords
/// holds an AIPS spectral axis type.
fn has_aips_spec_axis(cards: &CardList) -> bool {
    let (n, lo, hi) = cards.key_fields("CTYPE%1d", 1);
    if n == 0 {
        return false;
    }
    (lo[0]..=hi[0]).any(|j| {
        cards
            .first_visible(&format!("CTYPE{j}"))
            .and_then(|card| {
                cnv_type(Some(&card.value), Want::String, 10, "")
                    .ok()
                    .flatten()
            })
            .is_some_and(|got| matches!(got, Got::Str(ctype) if is_aips_spectral(&ctype).is_some()))
    })
}

/// C's `IsAIPSSpectral`: for a CTYPE of 8 characters (ignoring trailing
/// spaces) of the form `AAAA-BBB` used by AIPS for a spectral axis, the
/// FITS-WCS CTYPE and SPECSYS it stands for.
pub(crate) fn is_aips_spectral(ctype: &str) -> Option<(&'static str, &'static str)> {
    if ctype.trim_end_matches(' ').len() != 8 {
        return None;
    }
    let b = ctype.as_bytes();
    let wctype = if b.starts_with(b"FREQ") {
        "FREQ    "
    } else if b.starts_with(b"VELO") {
        "VRAD    "
    } else if b.starts_with(b"FELO") {
        "VOPT-F2W"
    } else if b.starts_with(b"WAVELENG") {
        "WAVE    "
    } else {
        return None;
    };
    let rest = &b[4..];
    let wspecsys = if rest.starts_with(b"-LSR") || rest.starts_with(b"LSRK") {
        "LSRK"
    } else if rest.starts_with(b"-LSD") {
        "LSRD"
    } else if rest.starts_with(b"-HEL") {
        "BARYCENT"
    } else if rest.starts_with(b"-EAR") || rest.starts_with(b"-GEO") {
        "GEOCENTR"
    } else if rest.starts_with(b"-OBS") || rest.starts_with(b"-TOP") {
        "TOPOCENT"
    } else {
        return None;
    };
    Some((wctype, wspecsys))
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
        assert!(is_aips_spectral("VELO-LSR").is_some());
        assert!(is_aips_spectral("FREQ-OBS").is_some());
        assert!(is_aips_spectral("FELO-HEL").is_some());
        assert!(is_aips_spectral("VELO-TOP").is_some());
        assert!(is_aips_spectral("FREQ-GEO").is_some());
        assert!(is_aips_spectral("FELO-EAR").is_some());
        assert!(is_aips_spectral("VELO-LSD").is_some());

        assert!(is_aips_spectral("VELO-XYZ").is_none());
        assert!(is_aips_spectral("RA---TAN").is_none());
        assert!(is_aips_spectral("VELO").is_none());
        assert!(is_aips_spectral("VELOCITY").is_none());
        assert!(is_aips_spectral("FREQ+LSR").is_none());
    }

    #[test]
    fn fallback_to_native() {
        let header = "\
SIMPLE  =                    T / Standard FITS
BITPIX  =                  -32 / Bits per pixel";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::Native);
    }

    #[test]
    fn keyword_native_detected_as_native_encoding() {
        let header = "\
BEGAST_A= 'FrameSet'
NFRAME_A=                    2
ENDAST_A= 'FrameSet'
END
";
        let cards = CardList::from_header(header).unwrap();
        assert_eq!(guess_encoding(&cards), Encoding::Native);
    }
}

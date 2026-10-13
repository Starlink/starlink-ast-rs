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

use super::card_list::CardList;
use super::fits_store::FitsStore;
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

/// Write the axis descriptions `planes` (primary first) with FITS-PC: C's
/// `PCFromStore` (fitschan.c:24802). FITS-PC holds at most ten
/// descriptions, the primary and `A` to `I`; an alternate one is written
/// as `C<n>ELT<i>`, `C<n>PIX<i>`, `C<n>VAL<i>`, `C<n>YPE<i>` and
/// `C<n>NIT<i>`, `<n>` being 1 for `A`, and must share the primary
/// description's PC matrix, RADECSYS, EQUINOX, LATPOLE, LONGPOLE, MJD-OBS
/// and projection parameters. A description that cannot be written leaves
/// no cards; an alternate one needs the primary one, and the write fails
/// when that cannot be written. Returns whether anything was written.
pub(crate) fn write_pc(planes: &[&FitsStore], letters: &[char], cards: &mut CardList) -> bool {
    use super::card_list::NewScope;
    use super::fits_store::equal_ang;
    use super::wcs_celestial::wcs_prj_type;
    use crate::constants::BAD_VALUE;
    use crate::mappings::helpers::ast_equal;
    use crate::wcslib::WcsProjection as P;

    if letters.iter().any(|&s| s > 'I') {
        return false;
    }
    let mut ret = false;
    let mut primpc: Vec<f64> = Vec::new();
    let mut primsys: Option<String> = None;
    let (mut primeq, mut primlt, mut primln, mut primdt) =
        (BAD_VALUE, BAD_VALUE, BAD_VALUE, BAD_VALUE);
    let mut primpv = [BAD_VALUE; 10];
    // C bounds every description's search for projection parameters by
    // the primary description's highest parameter index.
    let primmaxm = planes.first().and_then(|p| p.max_pv_m());

    for (&store, &s) in planes.iter().zip(letters) {
        let primary = s == ' ';
        let is = if primary {
            0
        } else {
            s as u32 - 'A' as u32 + 1
        };
        let key = |base: &str, alt: &str, i: usize| {
            if primary {
                format!("{base}{i}")
            } else {
                format!("C{is}{alt}{i}")
            }
        };
        let ok = (|| {
            let naxis = match store.wcsaxes {
                Some(n) => {
                    let key = if primary {
                        "WCSAXES".to_string()
                    } else {
                        format!("WCSAXES{s}")
                    };
                    cards.set_int(&key, n as i64, "Number of WCS axes");
                    n
                }
                None => store.max_crpix_axis().map_or(0, |m| m + 1),
            };

            // One PC matrix, the primary description's, for all.
            let matrix: Vec<f64> = (0..naxis * naxis)
                .map(|k| {
                    let (i, j) = (k / naxis, k % naxis);
                    store
                        .pc
                        .get(i)
                        .and_then(|row| row.get(j))
                        .copied()
                        .flatten()
                        .unwrap_or(if i == j { 1.0 } else { 0.0 })
                })
                .collect();
            if primary {
                for (k, &val) in matrix.iter().enumerate() {
                    let (i, j) = (k / naxis, k % naxis);
                    if !ast_equal(val, if i == j { 1.0 } else { 0.0 }) {
                        cards.set_float(&format!("PC{:03}{:03}", i + 1, j + 1), val, "");
                    }
                }
                primpc = matrix;
            } else if !primpc.is_empty()
                && !matrix.iter().zip(&primpc).all(|(a, b)| ast_equal(*a, *b))
            {
                return false;
            }

            for i in 0..naxis {
                let Some(val) = store.cdelt.get(i).copied().flatten() else {
                    return false;
                };
                cards.set_float(
                    &key("CDELT", "ELT", i + 1),
                    val,
                    &format!("Pixel scale on axis {}", i + 1),
                );
            }
            for j in 0..naxis {
                let Some(val) = store.crpix.get(j).copied().flatten() else {
                    return false;
                };
                cards.set_float(
                    &key("CRPIX", "PIX", j + 1),
                    val,
                    &format!("Reference pixel on axis {}", j + 1),
                );
            }
            for i in 0..naxis {
                let Some(val) = store.crval.get(i).copied().flatten() else {
                    return false;
                };
                cards.set_float(
                    &key("CRVAL", "VAL", i + 1),
                    val,
                    &format!("Value at ref. pixel on axis {}", i + 1),
                );
            }
            for i in 0..naxis {
                let Some(cval) = store.ctype.get(i).and_then(|c| c.as_deref()) else {
                    return false;
                };
                let nc = cval.len();
                if nc > 4 && cval.get(4..) == Some("-TAB") {
                    return false;
                }
                let comment = store.ctype_com[i]
                    .clone()
                    .unwrap_or_else(|| format!("Type of co-ordinate on axis {}", i + 1));
                // No `xxLT` or `xxLN` celestial axes, and no `-TAB`.
                if cval.get(2..5) == Some("LT-")
                    || cval.get(2..5) == Some("LN-")
                    || (nc > 4 && cval.get(4..).is_some_and(|t| t.starts_with("-TAB")))
                {
                    return false;
                }
                let prj = if nc > 4 {
                    cval.get(4..).and_then(wcs_prj_type)
                } else {
                    None
                };
                // SFL is written with the older code GLS, and TPN not at
                // all.
                let cval = match prj {
                    Some(P::Sfl) => format!("{}-GLS", &cval[..4]),
                    Some(P::Tpn) => return false,
                    _ => cval.to_string(),
                };
                cards.set_string(&key("CTYPE", "YPE", i + 1), &cval, &comment);
            }
            for i in 0..naxis {
                if let Some(cval) = store.cunit.get(i).and_then(|c| c.as_deref()) {
                    cards.set_string(
                        &key("CUNIT", "NIT", i + 1),
                        cval,
                        &format!("Units for axis {}", i + 1),
                    );
                }
            }
            if let Some(sys) = store.radesys.as_deref() {
                if primary {
                    primsys = Some(sys.to_string());
                    cards.set_string("RADECSYS", sys, "Reference frame for RA/DEC values");
                } else if primsys.as_deref() != Some(sys) {
                    return false;
                }
            }
            let val = store.equinox.unwrap_or(BAD_VALUE);
            if primary {
                primeq = val;
                if val != BAD_VALUE {
                    cards.set_float("EQUINOX", val, "Epoch of reference equinox");
                }
            } else if !ast_equal(val, primeq) {
                return false;
            }
            let val = store.latpole.unwrap_or(BAD_VALUE);
            if primary {
                primlt = val;
                if val != BAD_VALUE {
                    cards.set_float("LATPOLE", val, "Latitude of native north pole");
                }
            } else if !equal_ang(val, primlt) {
                return false;
            }
            let val = store.lonpole.unwrap_or(BAD_VALUE);
            if primary {
                primln = val;
                if val != BAD_VALUE {
                    cards.set_float("LONGPOLE", val, "Longitude of native north pole");
                }
            } else if !equal_ang(val, primln) {
                return false;
            }
            // MJD-OBS is kept for the primary description only; an
            // alternate one has none, and shares the primary one's.
            let val = if primary {
                store.mjdobs.unwrap_or(BAD_VALUE)
            } else {
                BAD_VALUE
            };
            if primary {
                primdt = val;
                if val != BAD_VALUE {
                    cards.set_float("MJD-OBS", val, "Modified Julian Date of observation");
                    cards.set_string(
                        "DATE-OBS",
                        &super::wcs_write::mjd_to_date_str(val),
                        "Date of observation",
                    );
                }
            } else if val != BAD_VALUE && !ast_equal(val, primdt) {
                return false;
            }

            // Projection parameters, only on the latitude axis, only the
            // first 10, and none for TAN.
            let (axlon, axlat, _) = store.lon_lat_spec_axes();
            if let (Some(axlon), Some(axlat), Some(maxm)) = (axlon, axlat, primmaxm) {
                let prj = store.ctype[axlat]
                    .as_deref()
                    .and_then(|c| c.get(4..))
                    .and_then(wcs_prj_type);
                for i in 0..naxis {
                    if i == axlon {
                        continue;
                    }
                    // `m` runs beyond the ten parameters `primpv` holds.
                    #[allow(clippy::needless_range_loop)]
                    for m in 0..=maxm {
                        let val = store
                            .pv
                            .get(i)
                            .and_then(|a| a.get(m))
                            .copied()
                            .flatten()
                            .unwrap_or(BAD_VALUE);
                        if primary {
                            if val != BAD_VALUE {
                                if i != axlat || prj == Some(P::Tan) || m >= 10 {
                                    return false;
                                }
                                cards.set_float(&format!("PROJP{m}"), val, "Projection parameter");
                            }
                            if i == axlat && m < 10 {
                                primpv[m] = val;
                            }
                        } else if ((i != axlat || m >= 10) && val != BAD_VALUE)
                            || (i == axlat && m < 10 && !ast_equal(val, primpv[m]))
                        {
                            return false;
                        }
                    }
                }
            }
            true
        })();
        if ok {
            ret = true;
        }
        cards.fix_new(NewScope::Version, !ok);
        if primary && !ok {
            break;
        }
    }
    ret
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
        assert!(write_pc(&[&store], &[' '], &mut cards));

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
    fn write_pc_needs_cdelt() {
        // FITS-PC writes CDELT for every axis, so a store without them
        // writes nothing, and the cards written before the defect was
        // found are removed again.
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(256.0);
        store.crpix[1] = Some(256.0);
        store.crval[0] = Some(45.0);
        store.crval[1] = Some(60.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.pc[0][1] = Some(0.5);

        let mut cards = CardList::new();
        cards.begin_marking_new();
        assert!(!write_pc(&[&store], &[' '], &mut cards));
        assert_eq!(cards.len(), 0);
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
        assert!(write_pc(&[&store], &[' '], &mut cards));

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
        assert!(write_pc(&[&store], &[' '], &mut cards));

        // Identity PC values should not be written
        assert!(cards.get_float("PC001001").is_none());
        assert!(cards.get_float("PC001002").is_none());
        assert!(cards.get_float("PC002001").is_none());
        assert!(cards.get_float("PC002002").is_none());
    }
}

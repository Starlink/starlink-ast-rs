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

    // Extract per-axis keywords. These WCS-description keywords are consumed
    // by the read (mark=1, via the `*_marked` getters) so they are excluded
    // from any subsequent output, mirroring C WcsFcRead.
    for i in 0..naxis {
        let axis_num = i + 1; // FITS is 1-based

        // CRPIX
        let key = format!("CRPIX{axis_num}");
        store.crpix[i] = cards.get_float_marked(&key);

        // CRVAL
        let key = format!("CRVAL{axis_num}");
        store.crval[i] = cards.get_float_marked(&key);

        // CDELT
        let key = format!("CDELT{axis_num}");
        store.cdelt[i] = cards.get_float_marked(&key);

        // CTYPE — translate AIPS spectral types and extract SPECSYS.
        // C's WcsFcRead applies an unconditional trailing-space strip
        // (fitschan.c:36906, see `CardList::strip_wcs_store_string`)
        // before storing any recognised string keyword; apply it here
        // before the AIPS-specific branching so both branches see the
        // stripped value, matching C.
        let key = format!("CTYPE{axis_num}");
        if let Some(ctype_raw) = CardList::strip_wcs_store_string(cards.get_string_marked(&key)) {
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
        store.cunit[i] = CardList::strip_wcs_store_string(cards.get_string_marked(&key));

        // CDi_j matrix (for AIPS++ headers)
        for j in 0..naxis {
            let col_num = j + 1;
            let key = format!("CD{axis_num}_{col_num}");
            store.cd[i][j] = cards.get_float_marked(&key);
        }

        // PCi_j matrix. CROTA rotation is normalised into PC keywords by
        // SpecTrans (`translate_crota`, mirroring C's WcsFcRead which runs
        // SpecTrans on every encoding before WcsToStore), so — like C — the
        // reader never interprets CROTA itself; it consumes the PC cards
        // SpecTrans produced on the celestial axes. Try the 3-digit format
        // (`PC003002`) first, then underscore (`PC3_2`).
        for j in 0..naxis {
            let col_num = j + 1;
            let key_3digit = format!("PC{axis_num:03}{col_num:03}");
            let key_underscore = format!("PC{axis_num}_{col_num}");
            store.pc[i][j] = cards
                .get_float_marked(&key_3digit)
                .or_else(|| cards.get_float_marked(&key_underscore));
        }
    }

    // EPOCH → equinox + RADESYS via IAU 1984 rule
    let epoch = cards.get_float_marked("EPOCH");
    let equinox_direct = cards.get_float_marked("EQUINOX");

    // Prefer EQUINOX if present (AIPS++ style), otherwise use EPOCH
    if let Some(eq) = equinox_direct {
        store.equinox = Some(eq);
    } else if let Some(ep) = epoch {
        // EPOCH=0 is treated as 1950.0 (common AIPS convention)
        let eq = if ep == 0.0 { 1950.0 } else { ep };
        store.equinox = Some(eq);
    }

    // RADESYS/RADECSYS if present (overrides IAU rule)
    store.radesys = CardList::strip_wcs_store_string(
        cards
            .get_string_marked("RADESYS")
            .or_else(|| cards.get_string_marked("RADECSYS")),
    );

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
    store.lonpole = cards.get_float_marked("LONPOLE");
    store.latpole = cards.get_float_marked("LATPOLE");

    // Spectral keywords (explicit SPECSYS overrides the one derived from CTYPE)
    if let Some(specsys) = CardList::strip_wcs_store_string(cards.get_string_marked("SPECSYS")) {
        store.specsys = Some(specsys);
    }
    store.restfrq = cards
        .get_float_marked("RESTFRQ")
        .or_else(|| cards.get_float_marked("RESTFREQ"));
    store.restwav = cards.get_float_marked("RESTWAV");

    // Epoch of observation (mark=0): TIMESYS + MJD-AVG / MJD-OBS / DATE-OBS /
    // DATE-AVG priority chain. Not consumed — survives into output.
    super::wcs_keywords::read_epoch_into_store(cards, &mut store);

    // PV parameters (AIPS++: PVi_m, also legacy PROJP{m} on the latitude
    // axis — fitschan.c writes PROJP{m} for AIPS++, see line 4148-4155).
    for i in 0..naxis {
        let axis_num = i + 1;
        for m in 0..store.pv[i].len() {
            let key = format!("PV{axis_num}_{m}");
            store.pv[i][m] = cards.get_float_marked(&key);
        }
    }

    // NCP → SIN normalisation (legacy AIPS alias).
    super::wcs_keywords::normalise_ncp_to_sin(&mut store);

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
///
/// `s` is the coordinate-version letter; `is_last` gates the
/// observation-global keyword emission. See `enc_pc::write_to_cards`
/// for the contract.
pub(crate) fn write_to_cards(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<()> {
    use super::wcs_keywords::key_for;
    let naxis = store.naxis;

    // CRPIXi
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = key_for("CRPIX", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Reference pixel on axis {}", i + 1));
        }
    }

    // Determine the spectral axis and its unit-canonicalisation factor.
    // Mirrors fitschan.c:3350-3376 (factor & canonical unit) +
    // :3526 (CRVAL *= specfactor) + :3560 (CDELT *= specfactor) +
    // :3570 (CUNIT replaced with canonical "HZ"/"m/s").
    let axspec = store.spectral_axis();
    let spec_remap = axspec
        .and_then(|i| store.cunit[i].as_deref())
        .and_then(aips_spectral_unit);

    // CRVALi
    for i in 0..naxis {
        if let Some(mut val) = store.crval[i] {
            if let (Some(ax), Some((_, factor))) = (axspec, spec_remap)
                && i == ax
            {
                val *= factor;
            }
            let key = key_for("CRVAL", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Value at ref. pixel on axis {}", i + 1));
        }
    }

    // CTYPEi — convert standard types back to AIPS spectral format using SPECSYS.
    // Also fold SIN-with-NCP-PV back to the legacy `-NCP` projection
    // suffix (mirrors fitschan.c:3270-3293).
    let ncp_axes = super::wcs_keywords::sin_should_emit_as_ncp(store);
    // SFL → GLS rename when CRVAL = (0, 0). FITS-AIPS encodes
    // Sanson-Flamsteed at the celestial origin using the legacy GLS
    // projection code. Mirrors fitschan.c::AIPSFromStore lines
    // 3257-3267: when the lat/lon axis pair has prj==SFL and both
    // CRVAL values are zero, rewrite the projection suffix to GLS.
    let sfl_to_gls = if let Some((lon_axis, lat_axis)) = store.celestial_axes() {
        let lon_zero = store.crval[lon_axis].unwrap_or(0.0) == 0.0;
        let lat_zero = store.crval[lat_axis].unwrap_or(0.0) == 0.0;
        let is_sfl = store.ctype[lon_axis]
            .as_deref()
            .is_some_and(|c| c.len() >= 8 && c[5..8].eq_ignore_ascii_case("SFL"));
        is_sfl && lon_zero && lat_zero
    } else {
        false
    };

    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let mut aips_ctype = translate_aips_spectral_write(ctype, store.specsys.as_deref());
            if ncp_axes && aips_ctype.len() >= 8 && aips_ctype[5..8].eq_ignore_ascii_case("SIN") {
                aips_ctype.replace_range(5.., "NCP");
            }
            if sfl_to_gls && aips_ctype.len() >= 8 && aips_ctype[5..8].eq_ignore_ascii_case("SFL") {
                aips_ctype.replace_range(5.., "GLS");
            }
            let key = key_for("CTYPE", Some(i + 1), s);
            let comment = store.ctype_com[i]
                .clone()
                .unwrap_or_else(|| format!("Type of co-ordinate on axis {}", i + 1));
            cards.set_string(&key, &aips_ctype, &comment);
        }
    }

    // Plain AIPS does not store a matrix; it uses CDELT and a single
    // CROTAk. Build a CD matrix from the store, derive CROTA via the
    // C library's two-rho-average (fitschan.c:3415-3467), and start
    // CDELT from CD's diagonal — for a rotated celestial frame
    //   CD[i][i] = cdelt_orig · cos(CROTA),
    // so dividing by cos(CROTA) recovers the original CDELT.
    let mut cdelt_vals = build_cdelt_diag(store);
    let cd = build_cd_matrix(store);
    let crota_deg = compute_crota_and_adjust_cdelt(&mut cdelt_vals, &cd, store);

    // CDELTi — always emitted (one per axis).
    if let (Some(ax), Some((_, factor))) = (axspec, spec_remap) {
        cdelt_vals[ax] *= factor;
    }
    for (i, &cdelt_val) in cdelt_vals.iter().enumerate() {
        let key = key_for("CDELT", Some(i + 1), s);
        cards.set_float(&key, cdelt_val, "Pixel size");
    }

    // CUNITi
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = key_for("CUNIT", Some(i + 1), s);
            let emitted = if let (Some(ax), Some((canon, _))) = (axspec, spec_remap)
                && i == ax
            {
                canon.to_string()
            } else {
                val.clone()
            };
            cards.set_string(&key, &emitted, &format!("Units for axis {}", i + 1));
        }
    }

    if is_last {
        // MJD-OBS — `AIPSFromStore` (fitschan.c:3590-3614) reads
        // store->mjdobs but only emits DATE-OBS; the input header's
        // MJD-OBS is left in place untouched. Mirror that exactly:
        // mark the existing card used so the sweep keeps it, but do
        // NOT re-emit. Re-emitting would round-trip the value through
        // SkyFrame::epoch (TDB) and back via `tdb_mjd_to_utc`, which
        // is asymmetric in the rubber-second era (pre-MJD-41317).
        // See `docs/issues/c-library-quirks.md` for the C-library
        // origin of that asymmetry.
        let _ = cards.mark_used("MJD-OBS");
    }

    // CROTAk — paired with the latitude axis for celestial frames or with
    // axis 1 for purely linear frames (mirrors fitschan.c:3578-3584).
    if let Some(crota) = crota_deg
        && naxis >= 2
    {
        let k = crota_axis(store);
        let key = if s == ' ' {
            format!("CROTA{k}")
        } else {
            format!("CROTA{k}{s}")
        };
        cards.set_float(&key, crota, "Axis rotation");
    }

    // EPOCH (not EQUINOX for plain AIPS). Per-version-letter.
    if let Some(equinox) = store.equinox {
        let key = key_for("EPOCH", None, s);
        cards.set_float(&key, equinox, "Epoch of reference equinox");
    }

    if is_last {
        // DATE-OBS computed from MJD-OBS (mirrors AIPSFromStore at
        // fitschan.c:3590-3614). The pre-formatted string in store.dateobs
        // was derived from MJD-OBS upstream.
        if let Some(ref dateobs) = store.dateobs {
            cards.set_string("DATE-OBS", dateobs, "Date of observation");
        }

        // RESTFREQ (AIPS uses RESTFREQ, not RESTFRQ). Mirrors fitschan.c:3622.
        if let Some(restfrq) = store.restfrq {
            cards.set_float("RESTFREQ", restfrq, "[Hz] Rest frequency");
        }
    }

    Ok(())
}

/// Build a CD matrix from the store, defaulting absent off-diagonals
/// to 0 and absent diagonals to 1. Falls back to CDELT × PC when no CD
/// values are present.
fn build_cd_matrix(store: &FitsStore) -> Array2<f64> {
    let naxis = store.naxis;
    if store.has_cd_matrix() {
        let mut cd = Array2::zeros((naxis, naxis));
        for i in 0..naxis {
            for j in 0..naxis {
                cd[[i, j]] = store.cd[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
            }
        }
        cd
    } else {
        let cdelt: Vec<f64> = (0..naxis).map(|i| store.cdelt[i].unwrap_or(1.0)).collect();
        let mut pc = Array2::zeros((naxis, naxis));
        for i in 0..naxis {
            for j in 0..naxis {
                pc[[i, j]] = store.pc[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
            }
        }
        let mut cd = Array2::zeros((naxis, naxis));
        for i in 0..naxis {
            for j in 0..naxis {
                cd[[i, j]] = cdelt[i] * pc[[i, j]];
            }
        }
        cd
    }
}

/// Initial CDELT for the AIPS encoder: the diagonal of the CD matrix.
/// Mirrors fitschan.c:3422 `cdelt[i] = val` (overwrites with `pc[i][i] *
/// store->cdelt[i]`, which equals CD[i][i]).
fn build_cdelt_diag(store: &FitsStore) -> Vec<f64> {
    let cd = build_cd_matrix(store);
    (0..store.naxis).map(|i| cd[[i, i]]).collect()
}

/// Choose the rotation-axis pair `(axrot1, axrot2)` for the AIPS
/// CROTA decomposition. Mirrors fitschan.c:3397-3412:
/// - Celestial pair present: axrot1=lon, axrot2=lat.
/// - No celestial pair, naxis>2 with a spectral axis: skip the spec
///   axis and pick the first two remaining axes.
/// - No celestial pair, naxis>1: axrot2=0, axrot1=1.
/// - Otherwise: no rotation possible.
fn aips_rotation_axes(store: &FitsStore) -> Option<(usize, usize)> {
    if let Some((lon, lat)) = store.celestial_axes() {
        return Some((lon, lat));
    }
    let naxis = store.naxis;
    let axspec = store.spectral_axis().map(|i| i as isize).unwrap_or(-1);
    if axspec >= 0 && naxis > 2 {
        let axspec = axspec as usize;
        let axrot2 = if axspec == 0 { 1 } else { 0 };
        let mut axrot1 = axrot2 + 1;
        if axrot1 == axspec {
            axrot1 += 1;
        }
        if axrot1 < naxis {
            return Some((axrot1, axrot2));
        }
    }
    if axspec < 0 && naxis > 1 {
        return Some((1, 0));
    }
    None
}

/// Compute CROTA in degrees from the CD matrix and adjust the CDELT
/// values so that the implicit rotation cancels out. Mirrors the C
/// library `AIPSFromStore` block at fitschan.c:3415-3467.
///
/// CROTA is the average of two estimates — rho_a from `CD[axrot2,
/// axrot1]` and rho_b from `CD[axrot1, axrot2]` — after which
/// `CDELT[axrot1]` and `CDELT[axrot2]` are divided by cos(CROTA) (or
/// recomputed from sin(CROTA) when |sin| > |cos|). For non-celestial
/// frames the rotation pair is the first two non-spectral axes
/// (fitschan.c:3397-3412), so a rotated PIXEL grid encodes as
/// CDELT + CROTA1 just like a celestial frame.
///
/// Returns `None` when:
/// - the two CROTA estimates disagree by ≥ 1e-2 rad (rotation not
///   expressible as a single CROTA), or
/// - any non-(axrot1,axrot2) off-diagonal is non-zero (rotation
///   spreads beyond the chosen plane). Both cases force the AIPS++
///   writer to fall back to a CDi_j matrix.
///
/// Returns `Some(0.0)` when no rotation pair exists (1-D or all-degenerate).
fn compute_crota_and_adjust_cdelt(
    cdelt: &mut [f64],
    cd: &Array2<f64>,
    store: &FitsStore,
) -> Option<f64> {
    let naxis = cdelt.len();
    let (axrot1, axrot2) = match aips_rotation_axes(store) {
        Some(pair) => pair,
        None => return Some(0.0),
    };

    // Validate that all off-diagonal CD entries outside the chosen
    // (axrot1, axrot2) pair are zero — anything else is not encodable
    // as a single CROTA. Mirrors fitschan.c:3424-3429.
    for i in 0..naxis {
        for j in 0..naxis {
            if i == j {
                continue;
            }
            let on_pair = (i == axrot2 && j == axrot1) || (i == axrot1 && j == axrot2);
            if !on_pair && cd[[i, j]] != 0.0 {
                return None;
            }
        }
    }

    let cdlat_lon = cd[[axrot2, axrot1]];
    let cdlon_lat = cd[[axrot1, axrot2]];

    let rho_a = if cdlat_lon > 0.0 {
        cdlat_lon.atan2(cdelt[axrot1])
    } else if cdlat_lon == 0.0 {
        0.0
    } else {
        (-cdlat_lon).atan2(-cdelt[axrot1])
    };
    let rho_b = if cdlon_lat > 0.0 {
        cdlon_lat.atan2(-cdelt[axrot2])
    } else if cdlon_lat == 0.0 {
        0.0
    } else {
        (-cdlon_lat).atan2(cdelt[axrot2])
    };

    if crate::pal::drange(rho_a - rho_b).abs() >= 1.0e-2 {
        return None;
    }
    let crota = 0.5 * (crate::pal::dranrm(rho_a) + crate::pal::dranrm(rho_b));
    let coscro = crota.cos();
    let sincro = crota.sin();
    if coscro.abs() > sincro.abs() {
        cdelt[axrot2] /= coscro;
        cdelt[axrot1] /= coscro;
    } else {
        cdelt[axrot2] = -cdlon_lat / sincro;
        cdelt[axrot1] = cdlat_lon / sincro;
    }
    Some(crota.to_degrees())
}

/// Choose the axis index `k` for CROTAk emission.
///
/// Returns the 1-based latitude axis when the store contains a celestial
/// CTYPE pair (`DEC--`, `GLAT-`, `ELAT-`, `SLAT-`, `EL---`); otherwise
/// returns 1. Matches the C library convention: CROTA is paired with the
/// Canonical AIPS spectral unit + scale factor.
///
/// Mirrors fitschan.c:3350-3376 / 3854-3880. The AIPS encoder normalises
/// the spectral axis CUNIT to either "HZ" or "m/s", scaling CRVAL and
/// CDELT by the matching factor. Returns `None` if the unit is not one
/// of the recognised AIPS spectral units (which makes the encoding
/// invalid; the caller leaves the unit unchanged).
fn aips_spectral_unit(unit: &str) -> Option<(&'static str, f64)> {
    match unit {
        "Hz" => Some(("HZ", 1.0)),
        "kHz" => Some(("HZ", 1.0e3)),
        "MHz" => Some(("HZ", 1.0e6)),
        "GHz" => Some(("HZ", 1.0e9)),
        "m/s" => Some(("m/s", 1.0)),
        "km/s" => Some(("m/s", 1.0e3)),
        _ => None,
    }
}

/// 1-based axis index `k` for CROTAk emission.
///
/// Mirrors fitschan.c:3577-3584: when a rotation pair exists,
/// CROTA is paired with `axrot2 + 1` — the latitude axis for
/// celestial frames, axis 1 for pure-linear N-D frames (axrot2=0),
/// or the first non-spectral axis when a spectral axis is present.
/// Falls back to 1 when no rotation pair was selected.
fn crota_axis(store: &FitsStore) -> usize {
    aips_rotation_axes(store)
        .map(|(_, axrot2)| axrot2 + 1)
        .unwrap_or(1)
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
pub(crate) fn write_to_cards_pp(
    store: &FitsStore,
    cards: &mut CardList,
    s: char,
    is_last: bool,
) -> AstResult<()> {
    use super::wcs_keywords::{key_for, key_for_2};
    let naxis = store.naxis;

    // CRPIXi.
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            let key = key_for("CRPIX", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Reference pixel on axis {}", i + 1));
        }
    }

    // Spectral-axis unit canonicalisation (see write_to_cards above).
    let axspec = store.spectral_axis();
    let spec_remap = axspec
        .and_then(|i| store.cunit[i].as_deref())
        .and_then(aips_spectral_unit);

    // CRVALi.
    for i in 0..naxis {
        if let Some(mut val) = store.crval[i] {
            if let (Some(ax), Some((_, factor))) = (axspec, spec_remap)
                && i == ax
            {
                val *= factor;
            }
            let key = key_for("CRVAL", Some(i + 1), s);
            cards.set_float(&key, val, &format!("Value at ref. pixel on axis {}", i + 1));
        }
    }

    // CTYPEi — AIPS spectral convention. Use ctype_com when set.
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let aips_ctype = translate_aips_spectral_write(ctype, store.specsys.as_deref());
            let key = key_for("CTYPE", Some(i + 1), s);
            let comment = store.ctype_com[i]
                .clone()
                .unwrap_or_else(|| format!("Type of co-ordinate on axis {}", i + 1));
            cards.set_string(&key, &aips_ctype, &comment);
        }
    }

    // Compute CROTA via the two-rho-average. If consistent (Some), the
    // adjusted CDELT absorbs the cosine factor and we emit CDELT + CROTA.
    // Otherwise (None: rotation isn't expressible as a single CROTA),
    // fall back to the CDi_j matrix. Mirrors fitschan.c:3950-3971,
    // 4081-4106.
    let mut cdelt_vals = build_cdelt_diag(store);
    let cd = build_cd_matrix(store);
    let crota_deg = compute_crota_and_adjust_cdelt(&mut cdelt_vals, &cd, store);

    // CDELTi — always emitted.
    if let (Some(ax), Some((_, factor))) = (axspec, spec_remap) {
        cdelt_vals[ax] *= factor;
    }
    for (i, &cdelt_val) in cdelt_vals.iter().enumerate() {
        let key = key_for("CDELT", Some(i + 1), s);
        cards.set_float(&key, cdelt_val, "Pixel size");
    }

    // CUNITi.
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            let key = key_for("CUNIT", Some(i + 1), s);
            let emitted = if let (Some(ax), Some((canon, _))) = (axspec, spec_remap)
                && i == ax
            {
                canon.to_string()
            } else {
                val.clone()
            };
            cards.set_string(&key, &emitted, &format!("Units for axis {}", i + 1));
        }
    }

    if is_last {
        // MJD-OBS — `AIPSPPFromStore` (fitschan.c:4122) only emits
        // DATE-OBS; the input MJD-OBS is left at its original
        // position. Mark the existing card used so the sweep keeps
        // it; do not re-emit (would re-round-trip through TDB and
        // drift in the rubber-second era — see
        // `docs/issues/c-library-quirks.md`).
        let _ = cards.mark_used("MJD-OBS");
    }

    // CD matrix or CROTA.
    match crota_deg {
        None => {
            // Rotation inconsistent — emit CDi_j (mirrors fitschan.c:4082-4095).
            for i in 0..naxis {
                for j in 0..naxis {
                    let val = cd[[i, j]];
                    if val != 0.0 {
                        let key = key_for_2("CD", i + 1, j + 1, s);
                        cards.set_float(&key, val, "Transformation matrix element");
                    }
                }
            }
        }
        Some(crota) if crota != 0.0 && naxis >= 2 => {
            let k = crota_axis(store);
            let key = if s == ' ' {
                format!("CROTA{k}")
            } else {
                format!("CROTA{k}{s}")
            };
            cards.set_float(&key, crota, "Axis rotation");
        }
        _ => {}
    }

    // EPOCH (NOT EQUINOX — AIPS++ uses EPOCH per fitschan.c:4109).
    if let Some(equinox) = store.equinox {
        let key = key_for("EPOCH", None, s);
        cards.set_float(&key, equinox, "Epoch of reference equinox");
    }

    // LATPOLE / LONPOLE if set (mirrors fitschan.c:4112-4120). LONPOLE
    // default 180° for zenithal projections is suppressed.
    if let Some(latpole) = store.latpole {
        let key = key_for("LATPOLE", None, s);
        cards.set_float(&key, latpole, "Latitude of native north pole");
    }
    if let Some(lonpole) = store.lonpole
        && lonpole != 180.0
    {
        let key = key_for("LONPOLE", None, s);
        cards.set_float(&key, lonpole, "Longitude of native north pole");
    }

    if is_last {
        // DATE-OBS — AIPS++ emits only DATE-OBS, not MJD-OBS (fitschan.c:4122).
        if let Some(ref dateobs) = store.dateobs {
            cards.set_string("DATE-OBS", dateobs, "Date of observation");
        }
    }

    // Projection parameters: PROJP{m} for celestial latitude axis only
    // (fitschan.c:4148-4155).
    if let Some((_, lat_axis)) = store.celestial_axes() {
        for (m, pv_val) in store.pv[lat_axis].iter().enumerate() {
            if let Some(val) = pv_val {
                let key = if s == ' ' {
                    format!("PROJP{m}")
                } else {
                    format!("PROJP{m}{s}")
                };
                cards.set_float(&key, *val, "Projection parameter");
            }
        }
    }

    if is_last {
        // RESTFREQ for spectral axis.
        if let Some(restfrq) = store.restfrq {
            cards.set_float("RESTFREQ", restfrq, "[Hz] Rest frequency");
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

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
    // Mirrors fitschan.c::AIPSFromStore lines 3320-3327. The AIPS
    // encoder only knows three modern spectral CTYPE values, all
    // matched against the trimmed prefix:
    //   FREQ      → FREQ
    //   VRAD      → VELO
    //   VOPT-F2W  → FELO
    // Anything else falls through unchanged; the encoder's caller
    // marks the result as not-AIPS-encodable.
    let trimmed = ctype.trim_end();
    let aips_prefix = match trimmed {
        "FREQ" => "FREQ",
        "VRAD" => "VELO",
        "VOPT-F2W" => "FELO",
        _ => return ctype.to_string(),
    };

    let frame = match specsys {
        Some(s) if s.starts_with("LSRK") => "LSR",
        Some(s) if s.starts_with("LSRD") => "LSD",
        Some(s) if s.starts_with("BARYCENT") => "HEL",
        Some(s) if s.starts_with("GEOCENTR") => "GEO",
        Some(s) if s.starts_with("TOPOCENT") => "OBS",
        _ => return ctype.to_string(),
    };

    format!("{aips_prefix}-{frame}")
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
        // The AIPS read path runs SpecTrans first (mirroring C WcsFcRead),
        // which converts CROTA2 into PC keywords on the celestial axes and
        // deletes the CROTA cards. `read_from_cards` then consumes the PC
        // cards — it never interprets CROTA itself.
        let cards = CardList::from_header(header).unwrap();
        let trans = crate::fitschan::spec_trans::spec_trans(
            &cards,
            crate::fitschan::Encoding::FitsAips,
            false,
            &mut Vec::new(),
        );
        let store = read_from_cards(&trans).unwrap();

        assert_eq!(store.naxis, 2);

        // CROTA2=30 degrees, with CDELT1=-CDELT2 (|lambda|=1), yields a PC
        // matrix carrying cos(30)/sin(30). SpecTrans stores off-diagonals as
        // PC2_1 = sin/lambda and PC1_2 = -sin*lambda with lambda=CDELT2/CDELT1.
        let cos30 = 30.0_f64.to_radians().cos();
        let sin30 = 30.0_f64.to_radians().sin();
        let lambda = 0.0002778 / -0.0002778; // CDELT2 / CDELT1 = -1

        let pc00 = store.pc[0][0].unwrap();
        let pc01 = store.pc[0][1].unwrap();
        let pc10 = store.pc[1][0].unwrap();
        let pc11 = store.pc[1][1].unwrap();

        assert!((pc00 - cos30).abs() < 1e-10, "PC[0][0] = cos(30)");
        assert!(
            (pc01 - (-sin30 * lambda)).abs() < 1e-10,
            "PC[0][1] = -sin(30)*lambda"
        );
        assert!(
            (pc10 - sin30 / lambda).abs() < 1e-10,
            "PC[1][0] = sin(30)/lambda"
        );
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
        // Both CDELTs positive so the C library's two-rho-average lands
        // on +30° rather than the equivalent (CROTA=150°, CDELT signs
        // flipped) representation.
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(512.0);
        store.crpix[1] = Some(512.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(-45.0);
        store.cdelt[0] = Some(0.0002778);
        store.cdelt[1] = Some(0.0002778);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        let angle = 30.0_f64.to_radians();
        store.pc[0][0] = Some(angle.cos());
        store.pc[0][1] = Some(-angle.sin());
        store.pc[1][0] = Some(angle.sin());
        store.pc[1][1] = Some(angle.cos());
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

        let crota2 = cards.get_float("CROTA2").unwrap();
        assert!(
            (crota2 - 30.0).abs() < 1e-10,
            "CROTA2 should be ~30, got {crota2}"
        );

        // Should NOT have PC or CD keywords.
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
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

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
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

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
        write_to_cards(&store, &mut cards, ' ', true).unwrap();

        // FREQ + BARYCENT -> FREQ-HEL
        assert_eq!(cards.get_string("CTYPE1"), Some("FREQ-HEL".to_string()));
    }

    #[test]
    fn write_aipspp_cdelt_for_diagonal_cd() {
        // Pure diagonal CD with no rotation: AIPSPPFromStore (C library
        // line 3633+) emits CDELT and EPOCH (not CDi_j and EQUINOX).
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
        write_to_cards_pp(&store, &mut cards, ' ', true).unwrap();

        // No rotation → CROTA suppressed (= 0). CDELT emitted from CD diagonal.
        assert_eq!(cards.get_float("CDELT1"), Some(-0.0005));
        assert_eq!(cards.get_float("CDELT2"), Some(0.0005));
        assert!(cards.get_float("CROTA2").is_none());
        assert!(cards.get_float("CD1_1").is_none());
        // EPOCH (AIPS++ uses EPOCH, not EQUINOX).
        assert_eq!(cards.get_float("EPOCH"), Some(2000.0));
        assert!(cards.get_float("EQUINOX").is_none());
        // No RADESYS field for AIPS++ (mirrors fitschan.c:3633+).
        assert!(cards.get_string("RADESYS").is_none());
    }

    #[test]
    fn write_aipspp_emits_crota_when_rotation_consistent() {
        // PC rotation that is expressible as a single CROTA — the
        // two-rho-average lands on a real angle and AIPSPPFromStore
        // emits CDELT + CROTA, no CDi_j (mirrors fitschan.c:4097-4106).
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        store.crval[0] = Some(45.0);
        store.crval[1] = Some(60.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        let angle = 30.0_f64.to_radians();
        store.pc[0][0] = Some(angle.cos());
        store.pc[0][1] = Some(-angle.sin());
        store.pc[1][0] = Some(angle.sin());
        store.pc[1][1] = Some(angle.cos());
        // Both CDELTs positive so the two-rho average lands on +30°
        // rather than the equivalent (CROTA=150°, CDELTs flipped) form.
        store.cdelt[0] = Some(0.001);
        store.cdelt[1] = Some(0.001);
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards_pp(&store, &mut cards, ' ', true).unwrap();

        let crota2 = cards.get_float("CROTA2").unwrap();
        assert!(
            (crota2 - 30.0).abs() < 1e-10,
            "CROTA2 should be ~30, got {crota2}"
        );
        // CD matrix not emitted when CROTA captures the rotation.
        assert!(cards.get_float("CD1_1").is_none());
        // EPOCH (not EQUINOX).
        assert_eq!(cards.get_float("EPOCH"), Some(2000.0));
        assert!(cards.get_float("EQUINOX").is_none());
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
        write_to_cards_pp(&store, &mut cards, ' ', true).unwrap();

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
        write_to_cards_pp(&store, &mut cards, ' ', true).unwrap();

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
        write_to_cards_pp(&store, &mut cards, ' ', true).unwrap();

        // Spectral axis should be written in AIPS format. AIPSPPFromStore
        // bakes the SPECSYS into the CTYPE (e.g. "VELO-LSR") and doesn't
        // emit a separate SPECSYS card (mirrors fitschan.c:4148-4164).
        assert_eq!(cards.get_string("CTYPE3"), Some("VELO-LSR".to_string()));
        assert!(cards.get_string("SPECSYS").is_none());
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
        // As with CROTA2, SpecTrans normalises CROTA1 into PC keywords before
        // the AIPS reader runs. C reads CROTA on the latitude axis first, then
        // the longitude axis, so CROTA1 (on the longitude axis) is used when
        // CROTA2 is absent.
        let cards = CardList::from_header(header).unwrap();
        let trans = crate::fitschan::spec_trans::spec_trans(
            &cards,
            crate::fitschan::Encoding::FitsAips,
            false,
            &mut Vec::new(),
        );
        let store = read_from_cards(&trans).unwrap();

        // CROTA1=45 fallback when CROTA2 is absent
        let cos45 = 45.0_f64.to_radians().cos();
        let sin45 = 45.0_f64.to_radians().sin();
        let lambda = 0.0002778 / -0.0002778; // CDELT2 / CDELT1 = -1

        let pc00 = store.pc[0][0].unwrap();
        let pc01 = store.pc[0][1].unwrap();
        let pc10 = store.pc[1][0].unwrap();
        let pc11 = store.pc[1][1].unwrap();

        assert!((pc00 - cos45).abs() < 1e-10);
        assert!((pc01 - (-sin45 * lambda)).abs() < 1e-10);
        assert!((pc10 - sin45 / lambda).abs() < 1e-10);
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

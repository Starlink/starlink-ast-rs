//! Extraction of FITS-WCS keywords from a CardList into a FitsStore.
//!
//! This module implements the first phase of the FITS-WCS decoding pipeline:
//! reading individual FITS keywords from the header cards and populating the
//! intermediate [`FitsStore`] structure.

use super::card::FitsValue;
use super::card_list::CardList;
use super::fits_match::match_keyword;
use super::fits_store::FitsStore;
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};

/// Build a FITS keyword name with the version-letter suffix.
///
/// `s == ' '` means the primary plane (no suffix).
/// `s` in `'A'..='Z'` produces the alt-plane key (e.g. `CRVAL1A`).
///
/// Mirrors `fitschan.c::FormatKey` for the (axis-indexed, scalar)
/// keyword shapes used by FITS-WCS.
pub(crate) fn key_for(prefix: &str, axis: Option<usize>, s: char) -> String {
    match (axis, s) {
        (Some(i), ' ') => format!("{prefix}{i}"),
        (Some(i), c) => format!("{prefix}{i}{c}"),
        (None, ' ') => prefix.to_string(),
        (None, c) => format!("{prefix}{c}"),
    }
}

/// Build a FITS keyword name for a 2-index keyword (`PC{i}_{j}{S}`,
/// `CD{i}_{j}{S}`).
pub(crate) fn key_for_2(prefix: &str, i: usize, j: usize, s: char) -> String {
    if s == ' ' {
        format!("{prefix}{i}_{j}")
    } else {
        format!("{prefix}{i}_{j}{s}")
    }
}

/// Look up a value from `cards` at the version-suffixed key.
///
/// Mirrors `fitschan.c::GetItem` (line 13008): no primary-fallback —
/// each coordinate version has its own slot, and the lookup returns
/// `None` when the alt-suffixed key is absent (C returns AST__BAD
/// in that case). The CTYPE-prefix-driven defaults that make alt
/// planes "inherit" most values are applied later in
/// `wcs_celestial::build_celestial` / `wcs_spectral::build_spectral`,
/// not at the keyword-extraction layer.
pub(crate) fn lookup_with_fallback<T, F>(
    cards: &CardList,
    prefix: &str,
    axis: Option<usize>,
    s: char,
    f: F,
) -> Option<T>
where
    F: Fn(&CardList, &str) -> Option<T>,
{
    f(cards, &key_for(prefix, axis, s))
}

/// Read the FITS TIMESYS and the MJD-AVG / MJD-OBS / DATE-OBS priority
/// chain into `store.timesys` and `store.mjdobs`, from which
/// [`super::epoch::choose_epoch`] gives the TDB date of observation.
///
/// The chain (highest priority first):
///   1. MJD-AVG (mid-exposure time, preferred when present)
///   2. MJD-OBS (start of exposure)
///   3. DATE-OBS (parsed via date_str_to_mjd), which C's `SpecTrans`
///      translates to MJD-OBS when there is none
pub(crate) fn read_epoch_into_store(cards: &CardList, store: &mut FitsStore) {
    // TIMESYS is mark=0 in C (fitschan.c:36938-36945, never consumed) but
    // still passes through WcsFcRead's unconditional string strip before
    // being stored — mark only gates card consumption, not the strip. See
    // `CardList::strip_wcs_store_string` for the C citation.
    store.timesys = CardList::strip_wcs_store_string(cards.get_string("TIMESYS"));
    // C only recognises the hyphenated FITS-WCS spellings (MJD-AVG /
    // MJD-OBS / DATE-OBS / DATE-AVG). Non-standard underscored
    // variants like MJD_OBS or DATE_OBS (e.g. Spitzer headers) are
    // ignored — they're carried through as plain header cards but
    // not interpreted as WCS time values.
    store.mjdobs = cards
        .get_float("MJD-AVG")
        .or_else(|| cards.get_float("MJD-OBS"))
        .or_else(|| {
            cards
                .get_string("DATE-OBS")
                .as_deref()
                .and_then(super::wcs_write::date_str_to_mjd)
        });
}

/// Extract FITS-WCS keywords from a [`CardList`] into a [`FitsStore`].
///
/// The returned store contains all WCS keyword values found in the header.
/// Missing keywords are represented as `None`. The store has the axis
/// description's `WCSAXES` axes, or `npix`, the number of pixel axes, as C
/// takes the number of WCS axes (fitschan.c:10902-10906).
///
/// `s` is the coordinate-version letter:
/// - `' '` (primary) reads bare keys (`CRVAL1`, etc.).
/// - `'A'..='Z'` (alt plane) reads suffixed keys (`CRVAL1A`); per-axis
///   keys absent at the alt suffix fall back to the primary key (mirrors
///   C `GetItem`'s primary-fallback rule).
pub(crate) fn extract_wcs_keywords(cards: &CardList, s: char, npix: usize) -> AstResult<FitsStore> {
    // Determine the number of axes.
    // C reads WCSAXES as a double and rounds (fitschan.c:37169, fixed
    // upstream at e447556a): a value stored as a FITS float marginally below
    // an integer must not truncate to the integer below.
    let wcsaxes = cards
        .get_float(&key_for("WCSAXES", None, s))
        .map(|v| v.round() as usize);
    let naxis = wcsaxes.unwrap_or(npix);
    if naxis == 0 {
        return Err(AstError::new(ErrorCode::Bdfts, "WCSAXES is zero"));
    }

    let mut store = FitsStore::new(naxis);
    store.wcsaxes = wcsaxes;
    // WCSAXES is consumed by the read (C WcsFcRead marks it mark=1).
    if wcsaxes.is_some() {
        cards.record_consumed(&key_for("WCSAXES", None, s));
    }

    // The WCS-description keywords below are all read with `mark=1` (the
    // `*_marked` getters): they are consumed by the read and excluded from
    // any subsequent output, mirroring C `WcsFcRead`'s default. The
    // observation-detail keywords (epoch chain, DUT1, DTAI, OBSGEO-*) are read
    // with the plain `get_*` getters (`mark=0`) so they survive into output.

    // Extract per-axis keywords with primary fallback when s != ' '.
    for i in 0..naxis {
        let axis_num = i + 1; // FITS 1-based

        store.crpix[i] = lookup_with_fallback(cards, "CRPIX", Some(axis_num), s, |c, k| {
            c.get_float_marked(k)
        });
        store.crval[i] = lookup_with_fallback(cards, "CRVAL", Some(axis_num), s, |c, k| {
            c.get_float_marked(k)
        });
        store.cdelt[i] = lookup_with_fallback(cards, "CDELT", Some(axis_num), s, |c, k| {
            c.get_float_marked(k)
        });
        // NAXISn is structural (not a WCS keyword); never consumed.
        store.dims[i] = cards.get_float(&format!("NAXIS{axis_num}"));
        store.ctype[i] = CardList::strip_wcs_store_string(lookup_with_fallback(
            cards,
            "CTYPE",
            Some(axis_num),
            s,
            |c, k| c.get_string_marked(k),
        ));
        store.cunit[i] = CardList::strip_wcs_store_string(lookup_with_fallback(
            cards,
            "CUNIT",
            Some(axis_num),
            s,
            |c, k| c.get_string_marked(k),
        ));
        store.cname[i] = CardList::strip_wcs_store_string(lookup_with_fallback(
            cards,
            "CNAME",
            Some(axis_num),
            s,
            |c, k| c.get_string_marked(k),
        ));

        // CD / PC / PV: per-version, no primary fallback (mirrors C
        // GetItem semantics — see lookup_with_fallback's doc).
        for j in 0..naxis {
            store.pc[i][j] = cards.get_float_marked(&key_for_2("PC", axis_num, j + 1, s));
        }
        for m in 0..store.pv[i].len() {
            store.pv[i][m] = cards.get_float_marked(&key_for_2("PV", axis_num, m, s));
        }
        for m in 0..store.ps[i].len() {
            store.ps[i][m] = CardList::strip_wcs_store_string(
                cards.get_string_marked(&key_for_2("PS", axis_num, m, s)),
            );
        }
    }

    // Scalar WCS keywords (mark=1). String-typed fields get the
    // unconditional WcsFcRead strip (fitschan.c:37028) via
    // `strip_wcs_store_string` — see that function's doc comment.
    store.radesys = CardList::strip_wcs_store_string(lookup_with_fallback(
        cards,
        "RADESYS",
        None,
        s,
        |c, k| c.get_string_marked(k),
    ));
    store.equinox = lookup_with_fallback(cards, "EQUINOX", None, s, |c, k| c.get_float_marked(k));
    store.lonpole = lookup_with_fallback(cards, "LONPOLE", None, s, |c, k| c.get_float_marked(k));
    store.latpole = lookup_with_fallback(cards, "LATPOLE", None, s, |c, k| c.get_float_marked(k));
    // Epoch of observation (TIMESYS + MJD-AVG / MJD-OBS / DATE-OBS /
    // DATE-AVG priority chain). These are observation-global keywords read
    // with mark=0 (C WcsFcRead: MJD-OBS line 36519, MJD-AVG 36715, TIMESYS
    // 36751) so they are NOT consumed — they survive into the output header.
    read_epoch_into_store(cards, &mut store);
    store.specsys = CardList::strip_wcs_store_string(lookup_with_fallback(
        cards,
        "SPECSYS",
        None,
        s,
        |c, k| c.get_string_marked(k),
    ));
    store.restfrq = lookup_with_fallback(cards, "RESTFRQ", None, s, |c, k| c.get_float_marked(k));
    store.restwav = lookup_with_fallback(cards, "RESTWAV", None, s, |c, k| c.get_float_marked(k));
    // Observation-global TAI/UT1 offsets — mark=0 (C WcsFcRead: DUT1
    // fitschan.c:36684-36685, DTAI fitschan.c:36693-36695). The DTAI *card*
    // is never consumed either way (C never sets mark=1 for it), but C only
    // stores the value into the FitsStore — and hence only sets the Dtai
    // attribute on the resulting Frame (fitschan.c:38012-38014) — when a
    // TELESCOP card is present and names JCMT or UKIRT, since other
    // telescopes may use DTAI for unrelated purposes. TELESCOP itself is
    // read unmarked (mark=0 in the C `GetValue` call) and always as the
    // primary (unsuffixed) keyword, regardless of the alt-plane letter `s`
    // being decoded.
    let telescop_is_jcmt_or_ukirt = cards
        .get_string("TELESCOP")
        .is_some_and(|t| t.starts_with("JCMT") || t.starts_with("UKIRT"));
    store.dtai = if telescop_is_jcmt_or_ukirt {
        cards.get_float(&key_for("DTAI", None, s))
    } else {
        None
    };
    store.dut1 = cards.get_float(&key_for("DUT1", None, s));

    // AST-extension keywords. Mirrors WcsFromStore writes at
    // fitschan.c:37127-37494. WCSNAME, OBSGEO-{X,Y,Z}, SSYSSRC,
    // ZSOURCE, IMAGFREQ, SREFIS / SREF{i} / SREFP{i}.
    store.wcsname = CardList::strip_wcs_store_string(lookup_with_fallback(
        cards,
        "WCSNAME",
        None,
        s,
        |c, k| c.get_string_marked(k),
    ));
    // OBSGEO-{X,Y,Z} are observation-global, mark=0 (C WcsFcRead lines
    // 36724/33/42) — never consumed.
    store.obsgeox = cards.get_float("OBSGEO-X");
    store.obsgeoy = cards.get_float("OBSGEO-Y");
    store.obsgeoz = cards.get_float("OBSGEO-Z");
    // SSYSSRC / ZSOURCE / IMAGFREQ / SREF* / AXREF* are WCS-description
    // (mark=1). RFVAL is deliberately absent: C has no WcsFcRead branch for
    // it (fitschan.c only reads it at the point of use, unmarked).
    store.ssyssrc = CardList::strip_wcs_store_string(cards.get_string_marked("SSYSSRC"));
    store.zsource = cards.get_float_marked("ZSOURCE");
    store.imagfreq = cards.get_float_marked("IMAGFREQ");
    store.skyrefis =
        CardList::strip_wcs_store_string(cards.get_string_marked(&key_for("SREFIS", None, s)));
    for i in 0..naxis {
        let axis_num = i + 1;
        store.skyref[i] = cards.get_float_marked(&key_for("SREF", Some(axis_num), s));
        store.skyrefp[i] = cards.get_float_marked(&key_for("SREFP", Some(axis_num), s));
        store.axref[i] = cards.get_float_marked(&key_for("AXREF", Some(axis_num), s));
    }

    // Handle SIP distortion: strip "-SIP" from CTYPE and extract coefficients.
    extract_sip(cards, &mut store, s);

    // NCP → SIN normalisation (legacy alias). See [`normalise_ncp_to_sin`].
    normalise_ncp_to_sin(&mut store);

    Ok(store)
}

/// The axis descriptions of a FITS-WCS header: the number of pixel axes,
/// the primary description if C's `MakeFitsFrameSet` adds a Frame for it,
/// and the alternate ones it adds, in order (see
/// [`super::wcs_mark::wcs_versions`]).
pub(crate) struct WcsStores {
    /// The number of pixel axes.
    pub npix: usize,
    /// The primary axis description.
    pub primary: Option<FitsStore>,
    /// The alternate axis descriptions, with their letters.
    pub alts: Vec<(char, FitsStore)>,
}

/// Extract every axis description of a FITS-WCS header.
pub(crate) fn extract_wcs_keywords_with_alts(cards: &CardList) -> AstResult<WcsStores> {
    let versions = super::wcs_mark::wcs_versions(cards);
    let npix = versions.npix;
    let primary = if versions.primary {
        Some(extract_wcs_keywords(cards, ' ', npix)?)
    } else {
        None
    };
    let mut alts = Vec::with_capacity(versions.alternates.len());
    for s in versions.alternates {
        alts.push((s, extract_wcs_keywords(cards, s, npix)?));
    }
    Ok(WcsStores {
        npix,
        primary,
        alts,
    })
}

/// Test whether a SIN projection in `store` should be re-emitted as
/// the legacy `-NCP` form. Mirrors fitschan.c:3337-3360. The rewrite
/// applies when:
/// - PV{lat,1} is 0 (or unset),
/// - PV{lat,2} matches `cot(δ_ref)` to within 0.01 (the historical
///   NCP definition), or has |value| ≥ 1e14 with δ_ref = 0 (the
///   north-celestial-pole degenerate case).
///
/// Returns `false` for any non-SIN projection.
pub(crate) fn sin_should_emit_as_ncp(store: &FitsStore) -> bool {
    let Some((_, lat_axis)) = store.celestial_axes() else {
        return false;
    };
    let ctype_is_sin = matches!(&store.ctype[lat_axis],
        Some(ct) if ct.len() >= 8 && ct[5..8].eq_ignore_ascii_case("SIN"));
    if !ctype_is_sin {
        return false;
    }
    let Some(crval_lat_deg) = store.crval[lat_axis] else {
        return false;
    };
    let p1 = store.pv[lat_axis].get(1).copied().flatten().unwrap_or(0.0);
    let p2 = store.pv[lat_axis].get(2).copied().flatten().unwrap_or(0.0);
    if p1 != 0.0 {
        return false;
    }
    if p2 == 0.0 {
        return false;
    }
    if p2.abs() >= 1.0e14 && crval_lat_deg == 0.0 {
        return true;
    }
    (p2 * crval_lat_deg.to_radians().tan() - 1.0).abs() < 0.01
}

/// Translate any `*-NCP` celestial CTYPE pair to `*-SIN` and synthesise
/// the corresponding PV parameters: PV2_1 = 0, PV2_2 = cot(δ_ref).
///
/// Mirrors fitschan.c::SpecialTrans (line 31682-31713). The legacy AIPS
/// NCP projection is the special case of SIN where the projection
/// reference position lies at the celestial pole. AST's internal
/// representation only carries the standard projections, so we
/// canonicalise NCP to SIN at read time.
pub(crate) fn normalise_ncp_to_sin(store: &mut FitsStore) {
    let Some((lon_axis, lat_axis)) = store.celestial_axes() else {
        return;
    };
    let is_ncp = matches!(&store.ctype[lat_axis],
        Some(ct) if ct.len() >= 8 && ct[5..8].eq_ignore_ascii_case("NCP"));
    if !is_ncp {
        return;
    }
    let Some(crval_lat_deg) = store.crval[lat_axis] else {
        return;
    };
    let sinval = crval_lat_deg.to_radians().sin();
    if sinval == 0.0 {
        return;
    }
    let cot = crval_lat_deg.to_radians().cos() / sinval;
    if let Some(ct) = &mut store.ctype[lon_axis] {
        ct.replace_range(4.., "-SIN");
    }
    if let Some(ct) = &mut store.ctype[lat_axis] {
        ct.replace_range(4.., "-SIN");
    }
    if store.pv[lat_axis].len() > 2 {
        store.pv[lat_axis][1] = Some(0.0);
        store.pv[lat_axis][2] = Some(cot);
    }
}

/// Read the SIP coefficient cards for one keyword prefix into a table.
///
/// Mirrors the `A_%d_%d` / `A_%d_%d%1c` branches of C `WcsFcRead`
/// (fitschan.c:36949-37014) and their B / AP / BP counterparts: a matching
/// card is stored at `[i][jm]`, the two `%d` fields, in the slot belonging to
/// the coordinate version named by the trailing letter (the primary version
/// when the template has none). Slots with no card hold [`BAD_VALUE`], which
/// is what `SIPMapping`'s `GetItem` reports for them, and an axis with no
/// cards at all leaves an empty row where C leaves a NULL pointer.
fn read_sip_coeffs(cards: &CardList, prefix: &str, s: char) -> Vec<Vec<f64>> {
    let pattern = if s == ' ' {
        format!("{prefix}_%d_%d")
    } else {
        format!("{prefix}_%d_%d%1c")
    };

    let mut table: Vec<Vec<f64>> = Vec::new();
    for card in cards.iter() {
        let Some(fields) = match_keyword(&card.keyword, &pattern, 2) else {
            continue;
        };
        if fields.len() != 2 || fields[0] < 0 || fields[1] < 0 {
            continue;
        }
        if s != ' ' && !card.keyword.ends_with(s) {
            continue;
        }
        // C converts the card to AST__FLOAT via CnvValue and skips the card
        // when the conversion fails (fitschan.c:37022-37035).
        let value = match &card.value {
            FitsValue::Float(v) => *v,
            FitsValue::Int(v) | FitsValue::KInt(v) => *v as f64,
            FitsValue::String(text) => match text.trim().parse::<f64>() {
                Ok(v) => v,
                Err(_) => continue,
            },
            _ => continue,
        };

        let (i, jm) = (fields[0] as usize, fields[1] as usize);
        if table.len() <= i {
            table.resize(i + 1, Vec::new());
        }
        let row = &mut table[i];
        if row.len() <= jm {
            row.resize(jm + 1, BAD_VALUE);
        }
        row[jm] = value;
    }
    table
}

/// Check for SIP distortion and extract coefficients.
///
/// Mirrors the `-SIP` branch of C `DistortMaps` (fitschan.c:8906-8939): the
/// distortion is present only when *both* of the first two CTYPE values end
/// with `-SIP`, and the suffix is stripped from each so later steps see a plain
/// projection code. The `A_ORDER` / `B_ORDER` / `AP_ORDER` / `BP_ORDER` cards
/// are deliberately not read: `SIPMapping` derives the powers it needs from the
/// coefficient cards themselves (`GetMaxI` / `GetMaxJM`, fitschan.c:29488-29509),
/// and the ORDER keywords appear on the write side only. Leaving them unread
/// also leaves them unconsumed, so they survive into a re-written header.
fn extract_sip(cards: &CardList, store: &mut FitsStore, s: char) {
    fn strip_sip(ctype: Option<&mut String>) -> bool {
        let Some(ct) = ctype else { return false };
        if !ct.trim_end().to_uppercase().ends_with("-SIP") {
            return false;
        }
        let nc = ct.trim_end().len() - 4;
        ct.truncate(nc);
        true
    }

    // C tests and strips axis 1 first, and only looks at axis 2 when axis 1
    // carried the code — so a lone `-SIP` on axis 1 is still stripped but
    // yields no distortion.
    if !strip_sip(store.ctype.first_mut().and_then(|c| c.as_mut())) {
        return;
    }
    if !strip_sip(store.ctype.get_mut(1).and_then(|c| c.as_mut())) {
        return;
    }

    store.sip = true;
    store.sip_a = read_sip_coeffs(cards, "A", s);
    store.sip_b = read_sip_coeffs(cards, "B", s);
    store.sip_ap = read_sip_coeffs(cards, "AP", s);
    store.sip_bp = read_sip_coeffs(cards, "BP", s);
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
    fn key_for_primary_no_suffix() {
        assert_eq!(key_for("CRVAL", Some(1), ' '), "CRVAL1");
        assert_eq!(key_for("RADESYS", None, ' '), "RADESYS");
    }

    #[test]
    fn key_for_alt_appends_letter() {
        assert_eq!(key_for("CRVAL", Some(1), 'A'), "CRVAL1A");
        assert_eq!(key_for("RADESYS", None, 'A'), "RADESYSA");
    }

    #[test]
    fn key_for_2_handles_pc_cd() {
        assert_eq!(key_for_2("PC", 1, 2, ' '), "PC1_2");
        assert_eq!(key_for_2("CD", 1, 1, 'B'), "CD1_1B");
    }

    #[test]
    fn wcs_store_string_strip_is_unconditional_unlike_generic_accessor() {
        // C keeps two distinct string-trimming rules (fitschan.c:37028):
        // the generic accessor (`astGetFitsS`/MAKE_FGET, fitschan.c
        // ~15407-15420) strips trailing spaces only when the value is
        // <= 8 characters, but WcsFcRead's FitsStore-store path strips
        // them unconditionally, regardless of length or `mark`:
        //
        //     if( CnvValue( fc, AST__STRING, undef, &cval, method, status ) ) {
        //        cval[ astChrLen( cval ) ] = 0;  /* Exclude trailing spaces */
        //        SetItemC( (char *****) item, i, jm, s, cval, status );
        //
        // A 13-character padded SPECSYS value (well over the 8-char
        // gate) must arrive in the FitsStore stripped, while the same
        // raw card value read through the generic accessor keeps its
        // padding.
        let header = "\
NAXIS   =                    2
CTYPE1  = 'RA---TAN'
CTYPE2  = 'DEC--TAN'
SPECSYS = 'BARYCENTRIC  '
END";
        let cards = cards_from(header);

        // Generic accessor: no unconditional strip, padding survives.
        assert_eq!(
            cards.get_string("SPECSYS"),
            Some("BARYCENTRIC  ".to_string())
        );

        // WCS-store path: unconditional strip removes the padding.
        let store = extract_wcs_keywords(&cards, ' ', 2).unwrap();
        assert_eq!(store.specsys, Some("BARYCENTRIC".to_string()));
    }

    #[test]
    fn versions_follow_crpix_and_crval() {
        use super::super::wcs_mark::{WcsVersions, wcs_versions};
        // The pixel axes come from the highest CRPIX index, NAXIS aside; an
        // alternate description is read up to the highest letter with a
        // CRVAL, so `B`, with only a CRPIX, is not.
        let header = "\
NAXIS   =                    2
CRPIX1  =                  1.0
CRPIX3  =                  1.0
CRVAL1  =                  1.0
CTYPE1A = 'GLON-TAN'
CRVAL2A =                  1.0
CRPIX1B =                100.0
END";
        assert_eq!(
            wcs_versions(&cards_from(header)),
            WcsVersions {
                npix: 3,
                primary: true,
                alternates: vec!['A'],
            }
        );
        // Without a CRPIX there are no pixel axes and nothing is read.
        let header = "\
NAXIS   =                    2
CTYPE1  = 'RA---TAN'
CRVAL1  =                  1.0
END";
        assert_eq!(
            wcs_versions(&cards_from(header)),
            WcsVersions {
                npix: 0,
                primary: false,
                alternates: vec![],
            }
        );
    }

    #[test]
    fn extract_with_alts_no_primary_fallback() {
        // Mirrors C GetItem semantics: alt-plane axis keywords are
        // strictly per-version, no primary fallback. An alt header
        // that omits CRVAL1A leaves alt.crval[0] = None even though
        // CRVAL1 is set on the primary.
        let header = "\
NAXIS   =                    2
CTYPE1  = 'RA---TAN'
CTYPE2  = 'DEC--TAN'
CRVAL1  =                180.0
CRVAL2  =                 45.0
CRPIX1  =                 50.5
CRPIX2  =                 50.5
CDELT1  =                -0.01
CDELT2  =                 0.01
CTYPE1A = 'GLON-TAN'
CTYPE2A = 'GLAT-TAN'
CRVAL2A =                  1.0
END";
        let cards = cards_from(header);
        let stores = extract_wcs_keywords_with_alts(&cards).unwrap();
        let (primary, alts) = (stores.primary.unwrap(), stores.alts);
        assert_eq!(primary.ctype[0].as_deref(), Some("RA---TAN"));
        assert_eq!(alts.len(), 1);
        let (letter, alt) = &alts[0];
        assert_eq!(*letter, 'A');
        assert_eq!(alt.ctype[0].as_deref(), Some("GLON-TAN"));
        // Strict per-version: alt did not override CRVAL/CRPIX/CDELT.
        assert_eq!(alt.crval[0], None);
        assert_eq!(alt.crpix[0], None);
        assert_eq!(alt.cdelt[0], None);
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
        let store = extract_wcs_keywords(&cards, ' ', 2).unwrap();

        assert_eq!(store.naxis, 2);
        assert_eq!(store.crpix[0], Some(128.0));
        assert_eq!(store.crpix[1], Some(256.0));
        assert_eq!(store.crval[0], Some(180.0));
        assert_eq!(store.crval[1], Some(-45.0));
        assert!((store.cdelt[0].unwrap() - (-0.001)).abs() < 1e-10);
        assert!((store.cdelt[1].unwrap() - 0.001).abs() < 1e-10);
        assert_eq!(store.ctype[0], Some("RA---TAN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--TAN".to_string()));
        assert!(store.cd.iter().flatten().all(Option::is_none));
        assert!(!store.has_pc_matrix());
        assert!(!store.has_sip());
    }

    #[test]
    fn cd_matrix_header_arrives_as_pc_and_cdelt() {
        // A CD matrix never reaches the store as such: `SpecTrans` normalises
        // each row to unit length, the row length becoming that row's CDELT.
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
        let mut cards = cards_from(header);
        super::super::spec_trans::spec_trans_inplace(
            &mut cards,
            crate::Encoding::FitsWcs,
            crate::fitschan::spec_trans::TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        let store = extract_wcs_keywords(&cards, ' ', 2).unwrap();

        assert!(store.has_pc_matrix());
        // Row 2's CDELT is negative because its diagonal element is.
        let cdelt = 0.00033970575502926056;
        assert!((store.cdelt[0].unwrap() - cdelt).abs() < 1e-17);
        assert!((store.cdelt[1].unwrap() + cdelt).abs() < 1e-17);
        // PC * CDELT reproduces the original CD matrix.
        for (i, j, cd) in [
            (0, 0, 0.000250),
            (0, 1, 0.000230),
            (1, 0, 0.000230),
            (1, 1, -0.000250),
        ] {
            let got = store.pc[i][j].unwrap() * store.cdelt[i].unwrap();
            assert!((got - cd).abs() < 1e-15, "PC*CDELT[{i}][{j}] = {got}");
        }
    }

    #[test]
    fn wcsaxes_sets_the_number_of_wcs_axes() {
        let header = "\
NAXIS   =                    4 / Physical axes
WCSAXES =                    2 / WCS axes
CRPIX1  =                  1.0
CRPIX2  =                  1.0
END";
        let cards = cards_from(header);
        let store = extract_wcs_keywords(&cards, ' ', 4).unwrap();
        assert_eq!(store.naxis, 2);
        assert_eq!(store.wcsaxes, Some(2));
    }

    #[test]
    fn sip_header_extraction() {
        // Use the fixture.
        let sip_header = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/wcsconv/inputs/sip.head"
        ))
        .unwrap();
        let mut cards = cards_from(&sip_header);
        super::super::spec_trans::spec_trans_inplace(
            &mut cards,
            crate::Encoding::FitsWcs,
            crate::fitschan::spec_trans::TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        let store = extract_wcs_keywords(&cards, ' ', 2).unwrap();

        // Verify basic keywords.
        assert_eq!(store.naxis, 2);
        assert_eq!(store.crpix[0], Some(128.0));
        assert_eq!(store.crpix[1], Some(128.0));
        assert!((store.crval[0].unwrap() - 202.482322805429).abs() < 1e-9);
        assert!((store.crval[1].unwrap() - 47.1751189300101).abs() < 1e-9);

        // SIP suffix should be stripped from CTYPE.
        assert_eq!(store.ctype[0], Some("RA---TAN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--TAN".to_string()));

        // SIP should be detected. The read path does not look at the ORDER
        // keywords, so the coefficient tables are the only record of it.
        assert!(store.has_sip());
        assert_eq!(store.sip_a_order, None);
        assert_eq!(store.sip_b_order, None);
        assert_eq!(store.sip_ap_order, None);
        assert_eq!(store.sip_bp_order, None);

        // Check a few SIP coefficients.
        assert!((store.sip_a[0][2] - 2.9656e-06).abs() < 1e-12);
        assert!((store.sip_a[2][0] - (-2.3863e-05)).abs() < 1e-12);
        assert!((store.sip_b[0][2] - 2.31e-05).abs() < 1e-12);

        // The CD matrix reaches the store as PC + CDELT.
        assert!(store.has_pc_matrix());
        let cd00 = store.pc[0][0].unwrap() * store.cdelt[0].unwrap();
        assert!((cd00 - 0.000249756880272355).abs() < 1e-15);

        // Scalar keywords.
        assert_eq!(store.radesys, Some("ICRS".to_string()));
        assert_eq!(store.equinox, Some(2000.0));
        // The Spitzer-style fixture carries MJD_OBS/DATE_OBS with
        // underscores rather than the FITS-WCS hyphenated form. C
        // ignores those, so our reader does too — `store.mjdobs`
        // stays unset.
        assert!(store.mjdobs.is_none());
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
        let store = extract_wcs_keywords(&cards, ' ', 2).unwrap();
        assert_eq!(store.pv[1][1], Some(0.0));
        assert_eq!(store.pv[1][2], Some(0.0));
        assert_eq!(store.pv[0][0], None);
    }
}

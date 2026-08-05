//! Extraction of FITS-WCS keywords from a CardList into a FitsStore.
//!
//! This module implements the first phase of the FITS-WCS decoding pipeline:
//! reading individual FITS keywords from the header cards and populating the
//! intermediate [`FitsStore`] structure.

use super::card_list::CardList;
use super::fits_store::FitsStore;
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

/// Discover the set of alt-plane version letters present in a
/// FITS-WCS card list.
///
/// Returns letters in alphabetical order. A letter is included iff
/// at least one of `CTYPE{i}{S}`, `CRVAL{i}{S}`, `CRPIX{i}{S}`,
/// `CDELT{i}{S}` is present for some axis `i ∈ 1..=9`.
///
/// Mirrors C `WcsFromStore`'s per-version axis-key scan.
pub(crate) fn detect_alt_letters(cards: &CardList) -> Vec<char> {
    let mut found = Vec::new();
    for c in 'A'..='Z' {
        let any = (1..=9).any(|i| {
            cards.find(&format!("CTYPE{i}{c}")).is_some()
                || cards.find(&format!("CRVAL{i}{c}")).is_some()
                || cards.find(&format!("CRPIX{i}{c}")).is_some()
                || cards.find(&format!("CDELT{i}{c}")).is_some()
        });
        if any {
            found.push(c);
        }
    }
    found
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

/// Infer the number of WCS axes from the highest-indexed FITS keyword
/// found in the header.
///
/// Scans `NAXIS{n}`, `CTYPE{n}`, `CRVAL{n}`, `CRPIX{n}`, `CDELT{n}` for
/// `n` in 1..=9 and returns the maximum index seen, or `None` if none
/// are present. Mirrors the C library's last-resort axis-count guess in
/// `fitschan.c::AstroSysCelOrder` and related call sites: when no
/// explicit count is available, the presence of axis-indexed keywords
/// up to `n` implies at least `n` axes exist.
pub(crate) fn infer_axis_count_from_indexed_keywords(cards: &CardList) -> Option<usize> {
    let prefixes = ["NAXIS", "CTYPE", "CRVAL", "CRPIX", "CDELT"];
    let mut max_n = 0;
    for n in 1..=9 {
        for prefix in &prefixes {
            let key = format!("{prefix}{n}");
            if cards.find(&key).is_some() {
                max_n = n;
                break;
            }
        }
    }
    if max_n == 0 { None } else { Some(max_n) }
}

/// Read the FITS TIMESYS and the MJD-AVG / MJD-OBS / DATE-OBS /
/// DATE-AVG priority chain into `store.timesys` and `store.mjdobs`.
///
/// Mirrors `fitschan.c::ChooseEpoch` (line 6066-6149) but populates
/// the FitsStore directly instead of returning a converted MJD —
/// the time-scale conversion to TT happens later in
/// `wcs_celestial::build_celestial_frame` once we know whether the
/// frame is celestial.
///
/// The chain (highest priority first):
///   1. MJD-AVG (mid-exposure time, preferred when present)
///   2. MJD-OBS (start of exposure)
///   3. DATE-OBS (parsed via date_str_to_mjd)
///   4. DATE-AVG (parsed via date_str_to_mjd)
pub(crate) fn read_epoch_into_store(cards: &CardList, store: &mut FitsStore) {
    // TIMESYS is mark=0 in C (fitschan.c:36816-36823, never consumed) but
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
        })
        .or_else(|| {
            cards
                .get_string("DATE-AVG")
                .as_deref()
                .and_then(super::wcs_write::date_str_to_mjd)
        });
}

/// Extract FITS-WCS keywords from a [`CardList`] into a [`FitsStore`].
///
/// The returned store contains all WCS keyword values found in the header.
/// Missing keywords are represented as `None`. The function determines
/// the number of WCS axes from `WCSAXES` if present, otherwise from `NAXIS`.
///
/// If neither `WCSAXES` nor `NAXIS` is found, returns an error.
///
/// `s` is the coordinate-version letter:
/// - `' '` (primary) reads bare keys (`CRVAL1`, etc.).
/// - `'A'..='Z'` (alt plane) reads suffixed keys (`CRVAL1A`); per-axis
///   keys absent at the alt suffix fall back to the primary key (mirrors
///   C `GetItem`'s primary-fallback rule).
pub(crate) fn extract_wcs_keywords(cards: &CardList, s: char) -> AstResult<FitsStore> {
    // Determine the number of axes.
    let wcsaxes = cards
        .get_int(&key_for("WCSAXES", None, s))
        .map(|v| v as usize);
    let naxis_val = cards.get_int("NAXIS").map(|v| v as usize);

    let naxis = match (wcsaxes, naxis_val) {
        (Some(w), _) => w,
        (None, Some(n)) => n,
        (None, None) => {
            infer_axis_count_from_indexed_keywords(cards).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Bdfts,
                    "cannot determine number of WCS axes: neither WCSAXES, NAXIS, NAXISn, CTYPEn, CRVALn, CRPIXn, nor CDELTn found",
                )
            })?
        }
    };

    if naxis == 0 {
        return Err(AstError::new(ErrorCode::Bdfts, "NAXIS/WCSAXES is zero"));
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
    }

    // Scalar WCS keywords (mark=1). String-typed fields get the
    // unconditional WcsFcRead strip (fitschan.c:36906) via
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
    // Observation-global TAI/UT1 offsets — mark=0 (C WcsFcRead: DUT1 line
    // 36497, DTAI 36506). Never consumed. DTAI in C is only handled at all for
    // JCMT/UKIRT; reading it unmarked here leaves it in the output regardless,
    // which matches C (it is never marked mark=1).
    store.dtai = cards.get_float(&key_for("DTAI", None, s));
    store.dut1 = cards.get_float(&key_for("DUT1", None, s));

    // AST-extension keywords. Mirrors WcsFromStore writes at
    // fitschan.c:37005-37372. WCSNAME, OBSGEO-{X,Y,Z}, SSYSSRC,
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
    extract_sip(cards, &mut store);

    // NCP → SIN normalisation (legacy alias). See [`normalise_ncp_to_sin`].
    normalise_ncp_to_sin(&mut store);

    Ok(store)
}

/// Extract primary + every detected alt-plane FitsStore from a
/// CardList.
///
/// Wired into `FitsChan::read` for FITS-WCS in Task 3.3.
///
/// The returned alt list is in alphabetical letter order. Each alt
/// FitsStore was extracted with `extract_wcs_keywords(cards, s)`,
/// which falls back to the primary keyword when the alt-suffixed
/// version is absent — see [`extract_wcs_keywords`] for the
/// fallback rule. Mirrors fitschan.c::WcsFromStore's per-version
/// loop.
pub(crate) fn extract_wcs_keywords_with_alts(
    cards: &CardList,
) -> AstResult<(FitsStore, Vec<(char, FitsStore)>)> {
    let primary = extract_wcs_keywords(cards, ' ')?;
    let alt_letters = detect_alt_letters(cards);
    let mut alts = Vec::with_capacity(alt_letters.len());
    for s in alt_letters {
        let store = extract_wcs_keywords(cards, s)?;
        alts.push((s, store));
    }
    Ok((primary, alts))
}

/// Test whether a SIN projection in `store` should be re-emitted as
/// the legacy `-NCP` form. Mirrors fitschan.c:3270-3293. The rewrite
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

/// Check for SIP distortion and extract coefficients.
///
/// If CTYPE values end with "-SIP", the suffix is stripped and the SIP
/// polynomial coefficients are read from the header.
fn extract_sip(cards: &CardList, store: &mut FitsStore) {
    // Check if CTYPE values contain "-SIP".
    let has_sip = store.ctype.iter().take(2).any(|ct| {
        ct.as_ref()
            .is_some_and(|s| s.to_uppercase().ends_with("-SIP"))
    });

    if !has_sip {
        return;
    }

    // Strip "-SIP" from CTYPE values (first two axes only, per convention).
    for s in store.ctype.iter_mut().take(2).flatten() {
        let upper = s.to_uppercase();
        if upper.ends_with("-SIP") {
            let new_len = s.len() - 4;
            s.truncate(new_len);
        }
    }

    // Forward A coefficients.
    if let Some(order) = cards.get_int_marked("A_ORDER") {
        let order = order as usize;
        store.sip_a_order = Some(order);
        let size = order + 1;
        store.sip_a = vec![vec![0.0; size]; size];
        for i in 0..size {
            for j in 0..size {
                if i + j <= order {
                    let key = format!("A_{i}_{j}");
                    if let Some(val) = cards.get_float_marked(&key) {
                        store.sip_a[i][j] = val;
                    }
                }
            }
        }
    }

    // Forward B coefficients.
    if let Some(order) = cards.get_int_marked("B_ORDER") {
        let order = order as usize;
        store.sip_b_order = Some(order);
        let size = order + 1;
        store.sip_b = vec![vec![0.0; size]; size];
        for i in 0..size {
            for j in 0..size {
                if i + j <= order {
                    let key = format!("B_{i}_{j}");
                    if let Some(val) = cards.get_float_marked(&key) {
                        store.sip_b[i][j] = val;
                    }
                }
            }
        }
    }

    // Inverse AP coefficients.
    if let Some(order) = cards.get_int_marked("AP_ORDER") {
        let order = order as usize;
        store.sip_ap_order = Some(order);
        let size = order + 1;
        store.sip_ap = vec![vec![0.0; size]; size];
        for i in 0..size {
            for j in 0..size {
                if i + j <= order {
                    let key = format!("AP_{i}_{j}");
                    if let Some(val) = cards.get_float_marked(&key) {
                        store.sip_ap[i][j] = val;
                    }
                }
            }
        }
    }

    // Inverse BP coefficients.
    if let Some(order) = cards.get_int_marked("BP_ORDER") {
        let order = order as usize;
        store.sip_bp_order = Some(order);
        let size = order + 1;
        store.sip_bp = vec![vec![0.0; size]; size];
        for i in 0..size {
            for j in 0..size {
                if i + j <= order {
                    let key = format!("BP_{i}_{j}");
                    if let Some(val) = cards.get_float_marked(&key) {
                        store.sip_bp[i][j] = val;
                    }
                }
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
        // C keeps two distinct string-trimming rules (fitschan.c:36906):
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
        let store = extract_wcs_keywords(&cards, ' ').unwrap();
        assert_eq!(store.specsys, Some("BARYCENTRIC".to_string()));
    }

    #[test]
    fn detect_alt_letters_empty_when_only_primary() {
        let header = "\
NAXIS   =                    2
CTYPE1  = 'RA---TAN'
CTYPE2  = 'DEC--TAN'
END";
        let cards = cards_from(header);
        assert!(detect_alt_letters(&cards).is_empty());
    }

    #[test]
    fn detect_alt_letters_finds_a_and_b() {
        let header = "\
NAXIS   =                    2
CTYPE1A = 'GLON-TAN'
CRPIX1B =                100.0
END";
        let cards = cards_from(header);
        let letters = detect_alt_letters(&cards);
        assert_eq!(letters, vec!['A', 'B']);
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
END";
        let cards = cards_from(header);
        let (primary, alts) = extract_wcs_keywords_with_alts(&cards).unwrap();
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
        let store = extract_wcs_keywords(&cards, ' ').unwrap();

        assert_eq!(store.naxis, 2);
        assert_eq!(store.crpix[0], Some(128.0));
        assert_eq!(store.crpix[1], Some(256.0));
        assert_eq!(store.crval[0], Some(180.0));
        assert_eq!(store.crval[1], Some(-45.0));
        assert!((store.cdelt[0].unwrap() - (-0.001)).abs() < 1e-10);
        assert!((store.cdelt[1].unwrap() - 0.001).abs() < 1e-10);
        assert_eq!(store.ctype[0], Some("RA---TAN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--TAN".to_string()));
        assert!(!store.has_cd_matrix());
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
        super::super::spec_trans::spec_trans_inplace(&mut cards, crate::Encoding::FitsWcs);
        let store = extract_wcs_keywords(&cards, ' ').unwrap();

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
    fn wcsaxes_overrides_naxis() {
        let header = "\
NAXIS   =                    4 / Physical axes
WCSAXES =                    2 / WCS axes
CRPIX1  =                  1.0
CRPIX2  =                  1.0
END";
        let cards = cards_from(header);
        let store = extract_wcs_keywords(&cards, ' ').unwrap();
        assert_eq!(store.naxis, 2);
        assert_eq!(store.wcsaxes, Some(2));
    }

    #[test]
    fn sip_header_extraction() {
        // Use the fixture.
        let sip_header = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/fits/headers/sip.head"
        ))
        .unwrap();
        let mut cards = cards_from(&sip_header);
        super::super::spec_trans::spec_trans_inplace(&mut cards, crate::Encoding::FitsWcs);
        let store = extract_wcs_keywords(&cards, ' ').unwrap();

        // Verify basic keywords.
        assert_eq!(store.naxis, 2);
        assert_eq!(store.crpix[0], Some(128.0));
        assert_eq!(store.crpix[1], Some(128.0));
        assert!((store.crval[0].unwrap() - 202.482322805429).abs() < 1e-9);
        assert!((store.crval[1].unwrap() - 47.1751189300101).abs() < 1e-9);

        // SIP suffix should be stripped from CTYPE.
        assert_eq!(store.ctype[0], Some("RA---TAN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--TAN".to_string()));

        // SIP should be detected.
        assert!(store.has_sip());
        assert_eq!(store.sip_a_order, Some(3));
        assert_eq!(store.sip_b_order, Some(3));
        assert_eq!(store.sip_ap_order, Some(3));
        assert_eq!(store.sip_bp_order, Some(3));

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
    fn no_naxis_returns_error() {
        let header = "SIMPLE  =                    T / Standard\nEND";
        let cards = cards_from(header);
        let result = extract_wcs_keywords(&cards, ' ');
        assert!(result.is_err());
    }

    #[test]
    fn axis_count_inferred_from_ctype_when_naxis_missing() {
        let header = "\
CTYPE1  = 'GLON-CAR'
CRVAL1  = 1.85e2
CDELT1  = -1.25e-1
CRPIX1  = 200.0
CTYPE2  = 'GLAT-CAR'
CRVAL2  = -3.5e1
CDELT2  = 1.25e-1
CRPIX2  = 200.0
END
";
        let cards = cards_from(header);
        assert_eq!(infer_axis_count_from_indexed_keywords(&cards), Some(2));
    }

    #[test]
    fn axis_count_returns_none_when_no_indexed_keywords() {
        let header = "\
SIMPLE  =                    T
EXTEND  =                    T
END
";
        let cards = cards_from(header);
        assert_eq!(infer_axis_count_from_indexed_keywords(&cards), None);
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
        let store = extract_wcs_keywords(&cards, ' ').unwrap();
        assert_eq!(store.pv[1][1], Some(0.0));
        assert_eq!(store.pv[1][2], Some(0.0));
        assert_eq!(store.pv[0][0], None);
    }
}

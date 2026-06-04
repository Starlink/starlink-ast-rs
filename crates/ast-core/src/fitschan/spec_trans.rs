//! Port of fitschan.c::SpecTrans (line 30828-32545).
//!
//! Translates non-standard FITS-WCS keywords into their standard
//! equivalents. The caller is the FitsChan reader: invoke
//! [`spec_trans`] once, before any FitsStore extraction, so that
//! downstream encoders see only canonical keyword names.
//!
//! Mirrors C `SpecTrans` except that translations write into the
//! supplied `CardList` directly rather than allocating a separate
//! return FitsChan. Original non-standard cards are removed once
//! translated; this is equivalent to C's "mark as used" mechanism
//! since the read pipeline is one-shot.

// `spec_trans` is wired into each FITS reader at the end of this
// port (Tasks 26-29). Until then the function is unreachable from
// outside this module; allow dead_code module-wide.
#![allow(dead_code)]

use super::Encoding;
use super::card_list::CardList;

/// Apply all SpecTrans translations to `cards` in place.
///
/// `encoding` selects which translations apply: a few are gated to
/// specific encodings (e.g. `Cnxxx` → `*A` is FITS-PC / FITS-IRAF
/// only; AIPS spectral CTYPE rewrites are AIPS-only). Translations
/// not gated by encoding apply to all FITS encodings uniformly.
pub(crate) fn spec_trans(cards: &mut CardList, encoding: Encoding) {
    // Translation 1 (fitschan.c:31137-31144): RADECSYS → RADESYS.
    // FITS-PC and FITS-IRAF only.
    if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
        rename_keyword(cards, "RADECSYS", "RADESYS");
    }

    // Translation 2 (fitschan.c:31148-31154): LONGPOLE → LONPOLE.
    // FITS-PC and FITS-IRAF only.
    if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
        rename_keyword(cards, "LONGPOLE", "LONPOLE");
    }

    // Translation 3 (fitschan.c:31206-31238): CDjjjiii → PCj_i +
    // CDELT=1.0. FITS-IRAF only — other encodings silently ignore
    // the legacy 6-digit form. Mirrors C's strict
    // `encoding == FITSIRAF_ENCODING` gate at fitschan.c:31223.
    if encoding == Encoding::FitsIraf {
        translate_cdjjjiii(cards);
    }

    // Translation 4 (fitschan.c:31298-31380): CROTA → PCi_j matrix.
    // Primary plane only. Requires celestial axes and CDELTs on both
    // celestial axes. The latitude axis's CROTA wins over the
    // longitude axis's when both are present.
    translate_crota(cards);

    // Translation 5 (fitschan.c:31386-31435): PROJPi → PV<axlat>_i.
    // Primary plane only, FITS-PC / FITS-IRAF only, gated on
    // PROJP-vs-PV disagreement heuristic.
    if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
        translate_projp(cards);
    }

    // Translation 6 (fitschan.c:31437-31578): C{m}{VAL/PIX/YPE/NIT/ELT}{i}
    // → CRVAL{i}{S} / CRPIX{i}{S} / CTYPE{i}{S} / CUNIT{i}{S} /
    // CDELT{i}{S} where S = 'A' + (m-1). FITS-PC / FITS-IRAF only.
    if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
        translate_cnxxx(cards);
    }

    // Translations 7, 8, 10 (fitschan.c:31580-31633): EPOCH /
    // EQUINOX normalisation. Primary plane only; no encoding gate.
    translate_epoch_equinox(cards);

    // Translation 9 (fitschan.c:31634-31655): DATE-OBS → MJD-OBS
    // when MJD-OBS is absent. Primary plane only; no encoding gate.
    translate_date_obs(cards);

    // Translation 11 (fitschan.c:31682-31775): AIPS NCP/GLS
    // projections. Per-version letter; no encoding gate.
    translate_ncp_gls(cards);

    // Translation 12 (fitschan.c:31082-31132): early paper-III
    // spectral CTYPE algorithm codes. Per-version letter, no
    // encoding gate. CTYPE values like "WAVE-FRQ" become
    // "WAVE-F2W" (the modern -X2P form).
    translate_early_paper_iii_spectral(cards);

    // Translation 13 (fitschan.c:31660-31680): SAO COi_j → PV<axlat>_m
    // distortion + CTYPE TAN→TPN. Per-version letter, no encoding gate.
    translate_sao_co(cards);

    // Translation 14 (fitschan.c:31716-31734): CLASS "ATF" projection
    // → AIT. Per-version letter, no encoding gate.
    translate_atf_to_ait(cards);

    // Translation 15 (fitschan.c:31774-31420): QV<i>_<m> → PV<i>_<m>.
    // If any QV is found and the projection is TAN, rename TAN to
    // TPN as well. Per-version letter, no encoding gate.
    translate_qv_to_pv(cards);

    // Translation 16 (fitschan.c:31425-31447): TPV → TPN; TAN with
    // PolyTan attribute non-zero → TPN. Per-version letter, no
    // encoding gate. (PolyTan is an AST attribute, defaulting to
    // false; we mirror by treating it as off.)
    translate_tpv_and_polytan_to_tpn(cards);

    // Translation 17 (fitschan.c:31444-31538): IRAF ZPX projection.
    // Replace -ZPX with -ZPN-ZPX (ZPN with ZPX distortion code) and
    // parse PROJP coefficients out of the WAT strings on the
    // celestial axes. Per-version letter, no encoding gate.
    translate_iraf_zpx(cards);

    // Translation 18 (fitschan.c:31488-31570): IRAF TNX projection.
    // Replace -TNX with -TPN and parse polynomial coefficients out
    // of the WAT strings; if not exactly representable in FITS-WCS
    // an `ASTWARN` warning is issued. Per-version letter, no
    // encoding gate.
    translate_iraf_tnx(cards);

    // Translation 19 (fitschan.c:31541-31571): MSX CAR projection.
    // Some MSX images have CAR projections whose CRPIX puts the
    // valid native longitude range entirely outside the image; C
    // shifts CRPIX1 by ±360°/CDELT1 to bring it back in range.
    // Per-version letter, no encoding gate.
    translate_msx_car(cards);

    // Translation 20 (fitschan.c:31573-31614): RESTFREQ → RESTFRQ
    // with MHz/GHz unit detection from the comment. Per-version
    // letter, no encoding gate.
    translate_restfreq(cards);

    // Translation 21 (fitschan.c:31616-31647): AIPS spectral CTYPE
    // (e.g. VELO-LSR → VRAD with SPECSYS=LSRK) and
    // LAMBDA → WAVE. Per-version letter, no encoding gate.
    translate_aips_spectral_ctype(cards);

    // Translation 22 (fitschan.c:31649-31681): CUNIT case-insensitive
    // canonicalisation: "Hz", "Angstrom", "km/s", "m/s" → canonical
    // case. Per-version letter, no encoding gate.
    translate_cunit_case(cards);

    // Translation 23 (fitschan.c:31256-31328): CLASS-specific
    // rewrites. Mirrors the `ClassTrans` helper called inside the
    // per-version loop; only fires for primary plane + FITS-CLASS
    // encoding.
    if encoding == Encoding::FitsClass {
        translate_class(cards);
    }

    // Translation 24 (fitschan.c:31686-31710): IRAF mini-WCS
    // keyword strip (WAT*, MWCS*, WSV*, LTM*, LTV*). Outside the
    // per-version loop in C; runs once. No encoding gate.
    translate_iraf_mini_wcs_strip(cards);
}

/// Translation #3 helper: rewrite the legacy 6-digit `CD{jjj}{iii}`
/// matrix into `PC{j}_{i}` + `CDELT{j} = 1.0`. Mirrors fitschan.c
/// :31206-31238. FITS-IRAF only — the caller is gated.
fn translate_cdjjjiii(cards: &mut CardList) {
    // C iterates the same 9×9 grid bounded by `astKeyFields`'s max
    // index detection; we use a fixed 9×9 since the 6-digit form
    // can encode at most that.
    let mut emitted_any = false;
    for j in 1..=9 {
        for i in 1..=9 {
            let from = format!("CD{j:03}{i:03}");
            let Some(value) = cards.get_float(&from) else {
                continue;
            };
            cards.delete(&from);
            cards.set_float(&format!("PC{j}_{i}"), value, "PC matrix element");
            emitted_any = true;
        }
    }
    if emitted_any {
        // C resets CDELT for every covered row to 1.0 (fitschan.c
        // :31226-31229). Find the max row index touched.
        for j in 1..=9 {
            // Only set CDELT for rows that had at least one PC value.
            let mut had_value = false;
            for i in 1..=9 {
                if cards.find(&format!("PC{j}_{i}")).is_some() {
                    had_value = true;
                    break;
                }
            }
            if had_value {
                cards.set_float(&format!("CDELT{j}"), 1.0, "Pixel scale");
            }
        }
    }
}

/// Test whether a CTYPE prefix denotes a celestial axis (longitude
/// or latitude) per the FITS-WCS conventions C `SpecTrans` walks
/// (fitschan.c:31040-31060). Returns `Some((true, _))` for
/// longitude axes, `Some((false, _))` for latitude, or `None`.
fn classify_celestial(ctype: &str) -> Option<bool> {
    let nc = ctype.len();
    if nc < 4 {
        return None;
    }
    if &ctype[..4] == "RA--" || &ctype[..4] == "AZ--" {
        return Some(true);
    }
    if &ctype[..4] == "DEC-" || &ctype[..4] == "EL--" {
        return Some(false);
    }
    if nc > 1 && &ctype[1..(1 + 3).min(nc)] == "LON" {
        return Some(true);
    }
    if nc > 1 && &ctype[1..(1 + 3).min(nc)] == "LAT" {
        return Some(false);
    }
    if nc > 2 && &ctype[2..(2 + 2).min(nc)] == "LN" {
        return Some(true);
    }
    if nc > 2 && &ctype[2..(2 + 2).min(nc)] == "LT" {
        return Some(false);
    }
    None
}

/// Locate the (longitude, latitude) celestial axis pair (1-based)
/// within version letter `s`. Returns `None` when one or both are
/// absent. Mirrors the celestial-axis identification at the top of
/// C `SpecTrans`'s per-version loop (fitschan.c:31040-31060).
fn celestial_axes(cards: &CardList, s: char) -> Option<(usize, usize)> {
    let mut axlon = None;
    let mut axlat = None;
    for j in 1..=9 {
        let key = if s == ' ' {
            format!("CTYPE{j}")
        } else {
            format!("CTYPE{j}{s}")
        };
        let Some(ctype) = cards.get_string(&key) else {
            continue;
        };
        match classify_celestial(ctype.trim()) {
            Some(true) => axlon = Some(j),
            Some(false) => axlat = Some(j),
            None => {}
        }
    }
    axlon.zip(axlat)
}

/// Highest CTYPE axis index in the primary version (mirrors C
/// `astKeyFields(template, 1, &naxis, lbnd)` on `CTYPE%d`).
fn primary_naxis(cards: &CardList) -> usize {
    let mut max = 0;
    for j in 1..=9 {
        if cards.find(&format!("CTYPE{j}")).is_some() || cards.find(&format!("CDELT{j}")).is_some()
        {
            max = j;
        }
    }
    max
}

// =============================================================
// Translations 12-24: stubs (each ported in its own commit
// following this one). The stubs are no-ops so the entry-point
// `spec_trans` compiles while the per-translation work proceeds.
// Each stub is replaced by the real port before this file is
// considered complete.
// =============================================================

/// Translation #12 helper: early paper-III spectral CTYPE codes.
///
/// Maps the obsolete algorithm codes `-WAV`, `-FRQ`, `-VEL` to the
/// modern `-X2P` form. The "X" character comes from the algorithm
/// code (W/F/V), and the "P" character from the system prefix:
/// FREQ/ENER/WAVN/VRAD → P=F; WAVE/VOPT/ZOPT → P=W; AWAV → P=A;
/// VELO/BETA → P=V.
///
/// When X equals P (e.g. WAVE-WAV would yield WAVE-W2W), C replaces
/// the algorithm code with blanks (or `----` for 12-char CTYPE) to
/// avoid the redundant code.
///
/// Mirrors fitschan.c:31082-31132. Per-version letter, no encoding gate.
fn translate_early_paper_iii_spectral(cards: &mut CardList) {
    for s in std::iter::once(' ').chain('A'..='Z') {
        for j in 1..=9 {
            let key = if s == ' ' {
                format!("CTYPE{j}")
            } else {
                format!("CTYPE{j}{s}")
            };
            let Some(ctype) = cards.get_string(&key) else {
                continue;
            };
            // Skip celestial axes — the per-version loop in C does
            // this implicitly because axlon/axlat consume those CTYPEs
            // earlier.
            if classify_celestial(ctype.trim()).is_some() {
                continue;
            }
            if ctype.len() < 8 {
                continue;
            }
            let suffix = &ctype[4..8];
            let x_char = match suffix {
                "-WAV" => 'W',
                "-FRQ" => 'F',
                "-VEL" => 'V',
                _ => continue,
            };
            let prefix = &ctype[..4];
            let p_char = match prefix {
                "FREQ" | "ENER" | "WAVN" | "VRAD" => 'F',
                "WAVE" | "VOPT" | "ZOPT" => 'W',
                "AWAV" => 'A',
                "VELO" | "BETA" => 'V',
                _ => continue,
            };
            let mut new_ctype = String::with_capacity(8);
            new_ctype.push_str(prefix);
            if x_char == p_char {
                // C blanks the algorithm code when X == P (or fills
                // with '-' for 12-char CTYPEs, which is uncommon).
                if ctype.len() > 8 {
                    new_ctype.push_str("----");
                } else {
                    new_ctype.push_str("    ");
                }
            } else {
                new_ctype.push('-');
                new_ctype.push(x_char);
                new_ctype.push('2');
                new_ctype.push(p_char);
            }
            let comment = cards
                .find(&key)
                .and_then(|c| c.comment.clone())
                .unwrap_or_default();
            cards.set_string(&key, &new_ctype, &comment);
        }
    }
}
/// Translation #13 helper: SAO COi_j → PV<axis>_m + CTYPE TAN→TPV.
///
/// The SAO platepos.c representation encodes a distorted TAN
/// projection in 13 polynomial coefficients per axis (CO1_1..CO1_13,
/// CO2_1..CO2_13). Translate to FITS-WCS paper II PV form and
/// rewrite the CTYPE projection from `-TAN` to `-TPV`.
///
/// Mirrors fitschan.c::SAOTrans (line 26630-26880). Per-version
/// letter is C's responsibility; the SAO header form is primary
/// only by convention.
fn translate_sao_co(cards: &mut CardList) {
    // Probe for any CO1_1 / CO2_1 — the SAO header form requires
    // both to be present.
    if cards.find("CO1_1").is_none() && cards.find("CO2_1").is_none() {
        return;
    }
    // Read the 13 coefficients per axis. Missing values (after
    // index 3) are treated as 0; missing values at indices 1-3
    // mark this as "not an SAO header" and abort.
    const NC: usize = 13;
    let mut co = [[0.0_f64; NC]; 2];
    let mut ok = true;
    let mut is_sao = true;
    for (i, row) in co.iter_mut().enumerate() {
        let mut row_ok = false;
        for (m, slot) in row.iter_mut().enumerate() {
            let key = format!("CO{}_{}", i + 1, m + 1);
            match cards.get_float(&key) {
                Some(v) => {
                    *slot = v;
                    if v != 0.0 && m > 0 {
                        row_ok = true;
                    }
                }
                None => {
                    if m < 3 {
                        is_sao = false;
                    }
                    break;
                }
            }
        }
        if !row_ok {
            ok = false;
        }
    }
    if !is_sao {
        return;
    }
    if !ok {
        // Mirror C: Warn message; we silently drop the CO cards
        // (no warning machinery in this port yet).
        return;
    }

    // Build PV<i>_m values per the SAO→TPV mapping at fitschan.c
    // :26730-26835. Each PV value is a sum of named CO coefficients;
    // we list them as (output_key, [co_indices_to_sum]) where CO
    // indices are 1-based to mirror the C source verbatim.
    let pv_axis1: &[(&str, &[(usize, usize)])] = &[
        ("PV1_0", &[(0, 0)]),
        ("PV1_1", &[(0, 1)]),
        ("PV1_2", &[(0, 2)]),
        ("PV1_4", &[(0, 3), (0, 10)]),
        ("PV1_5", &[(0, 5)]),
        ("PV1_6", &[(0, 4), (0, 10)]),
        ("PV1_7", &[(0, 6), (0, 11)]),
        ("PV1_8", &[(0, 8), (0, 12)]),
        ("PV1_9", &[(0, 9), (0, 11)]),
        ("PV1_10", &[(0, 7), (0, 12)]),
    ];
    let pv_axis2: &[(&str, &[(usize, usize)])] = &[
        ("PV2_0", &[(1, 0)]),
        ("PV2_1", &[(1, 2)]),
        ("PV2_2", &[(1, 1)]),
        ("PV2_4", &[(1, 4), (1, 10)]),
        ("PV2_5", &[(1, 5)]),
        ("PV2_6", &[(1, 3), (1, 10)]),
        ("PV2_7", &[(1, 7), (1, 12)]),
        ("PV2_8", &[(1, 9), (1, 11)]),
        ("PV2_9", &[(1, 8), (1, 12)]),
        ("PV2_10", &[(1, 6), (1, 11)]),
    ];
    for (key, indices) in pv_axis1.iter().chain(pv_axis2.iter()) {
        let pv: f64 = indices.iter().map(|(a, m)| co[*a][*m]).sum();
        cards.set_float(key, pv, "Projection parameter");
    }

    // Mark all CO cards consumed.
    for i in 0..2 {
        for m in 1..=NC {
            cards.delete(&format!("CO{}_{}", i + 1, m));
        }
    }

    // The SAO polynomial subsumes the CD/PC/CDELT/CROTA scaling, so
    // C marks all of them as used and replaces with identity values
    // (mirrors C:26845-26880).
    for key in [
        "CD1_1", "CD1_2", "CD2_1", "CD2_2", "PC1_1", "PC1_2", "PC2_1", "PC2_2", "CDELT1", "CDELT2",
        "CROTA1", "CROTA2",
    ] {
        cards.delete(key);
    }
    cards.set_float("PC1_1", 1.0, "PC matrix element");
    cards.set_float("PC2_2", 1.0, "PC matrix element");
    cards.set_float("CDELT1", 1.0, "Pixel scale");
    cards.set_float("CDELT2", 1.0, "Pixel scale");

    // Convert CTYPE TAN → TPV. Per-version letter not used here
    // because SAO headers are always primary (no SAO version
    // letters exist in practice).
    if let Some((axlon, axlat)) = celestial_axes(cards, ' ') {
        for ax in [axlon, axlat] {
            let key = format!("CTYPE{ax}");
            replace_projection_suffix(cards, &key, "TPV");
        }
    }
}
/// Translation #14 helper: CLASS "ATF" projection → AIT.
///
/// Mirrors fitschan.c:31716-31734. Per-version letter; no encoding gate.
fn translate_atf_to_ait(cards: &mut CardList) {
    for s in std::iter::once(' ').chain('A'..='Z') {
        let Some((axlon, axlat)) = celestial_axes(cards, s) else {
            continue;
        };
        let key = |prefix: &str, n: usize| {
            if s == ' ' {
                format!("{prefix}{n}")
            } else {
                format!("{prefix}{n}{s}")
            }
        };
        let Some(ctype) = cards.get_string(&key("CTYPE", axlon)) else {
            continue;
        };
        if projection_suffix(ctype.trim()).is_some_and(|p| p.eq_ignore_ascii_case("ATF")) {
            replace_projection_suffix(cards, &key("CTYPE", axlon), "AIT");
            replace_projection_suffix(cards, &key("CTYPE", axlat), "AIT");
        }
    }
}

/// Translation #15 helper: QV<i>_<m> → PV<i>_<m>. If any QV card is
/// found while the projection is `-TAN`, the projection is also
/// rewritten to `-TPN` (mirrors fitschan.c:31774-31420).
///
/// Per-version letter; no encoding gate.
fn translate_qv_to_pv(cards: &mut CardList) {
    for s in std::iter::once(' ').chain('A'..='Z') {
        // Collect (key_from, key_to) pairs first since modifying the
        // CardList during iteration is awkward.
        let mut pairs: Vec<(String, String)> = Vec::new();
        for c in cards.iter() {
            let kw = &c.keyword;
            // Match QV{i}_{m}{s} where s == version letter.
            if !kw.starts_with("QV") {
                continue;
            }
            let body = &kw[2..];
            // Split off the version letter (last char) when s != ' '.
            let (numeric, suffix) = if s == ' ' {
                (body, "")
            } else if let Some(stripped) = body.strip_suffix(s) {
                (stripped, &kw[kw.len() - 1..])
            } else {
                continue;
            };
            // numeric should look like "<i>_<m>".
            if !numeric.contains('_') {
                continue;
            }
            let pv_key = format!("PV{}{}", numeric, suffix);
            pairs.push((kw.clone(), pv_key));
        }
        if pairs.is_empty() {
            continue;
        }
        let any_qv = true;
        for (from, to) in pairs {
            // Don't overwrite an existing PV card.
            if cards.find(&to).is_some() {
                cards.delete(&from);
                continue;
            }
            if let Some(value) = cards.get_float(&from) {
                let comment = cards
                    .find(&from)
                    .and_then(|c| c.comment.clone())
                    .unwrap_or_default();
                cards.delete(&from);
                cards.set_float(&to, value, &comment);
            }
        }
        // If QV was found and the projection is TAN, rewrite to TPN.
        if any_qv {
            if let Some((axlon, axlat)) = celestial_axes(cards, s) {
                let key = |prefix: &str, n: usize| {
                    if s == ' ' {
                        format!("{prefix}{n}")
                    } else {
                        format!("{prefix}{n}{s}")
                    }
                };
                let Some(ctype) = cards.get_string(&key("CTYPE", axlon)) else {
                    continue;
                };
                if projection_suffix(ctype.trim()).is_some_and(|p| p.eq_ignore_ascii_case("TAN")) {
                    replace_projection_suffix(cards, &key("CTYPE", axlon), "TPN");
                    replace_projection_suffix(cards, &key("CTYPE", axlat), "TPN");
                }
            }
        }
    }
}

/// Translation #16 helper: TPV → TPN, and TAN+PolyTan → TPN.
///
/// Mirrors fitschan.c:31425-31447. The PolyTan attribute is an AST
/// FitsChan setting; in the absence of an explicit AST attribute
/// model we mirror the default (PolyTan = false), so only the
/// `-TPV` rename branch fires.
///
/// Per-version letter; no encoding gate.
fn translate_tpv_and_polytan_to_tpn(cards: &mut CardList) {
    for s in std::iter::once(' ').chain('A'..='Z') {
        let Some((axlon, axlat)) = celestial_axes(cards, s) else {
            continue;
        };
        let key = |prefix: &str, n: usize| {
            if s == ' ' {
                format!("{prefix}{n}")
            } else {
                format!("{prefix}{n}{s}")
            }
        };
        let Some(ctype) = cards.get_string(&key("CTYPE", axlon)) else {
            continue;
        };
        if projection_suffix(ctype.trim()).is_some_and(|p| p.eq_ignore_ascii_case("TPV")) {
            replace_projection_suffix(cards, &key("CTYPE", axlon), "TPN");
            replace_projection_suffix(cards, &key("CTYPE", axlat), "TPN");
        }
        // PolyTan branch deliberately omitted — defaults to false in
        // C (`GetUsedPolyTan` returns 0 when the attribute is unset).
    }
}
/// Translation #17 helper: IRAF -ZPX projections.
///
/// Replace `-ZPX` with `-ZPN-ZPX` (ZPN projection with ZPX
/// distortion code) on both celestial CTYPEs, then look at the
/// `WAT1_*` and `WAT2_*` strings on each axis for `projp{n}=value`
/// terms; each one becomes `PV<axlat>_n`.
///
/// Mirrors fitschan.c:31444-31538. Primary plane only; no encoding
/// gate.
fn translate_iraf_zpx(cards: &mut CardList) {
    let Some((axlon, axlat)) = celestial_axes(cards, ' ') else {
        return;
    };
    // Confirm projection is ZPX on the longitude axis (C only checks
    // longitude; lon/lat are usually consistent).
    let Some(ctype_lon) = cards.get_string(&format!("CTYPE{axlon}")) else {
        return;
    };
    if !projection_suffix(ctype_lon.trim()).is_some_and(|p| p.eq_ignore_ascii_case("ZPX")) {
        return;
    }
    // Replace CTYPE -ZPX with -ZPN-ZPX. Stretch the suffix to the
    // 12-char form (4 chars system + 4 chars projection + 4 chars
    // distortion). C uses `lontype + 4 = "-ZPN-ZPX"` which is a
    // mid-string write into a 12-char buffer.
    for ax in [axlon, axlat] {
        let key = format!("CTYPE{ax}");
        let Some(ctype) = cards.get_string(&key) else {
            continue;
        };
        let prefix = ctype[..4.min(ctype.len())].to_string();
        let new_ctype = format!("{prefix}-ZPN-ZPX");
        let comment = cards
            .find(&key)
            .and_then(|c| c.comment.clone())
            .unwrap_or_default();
        cards.set_string(&key, &new_ctype, &comment);
    }
    // Walk WAT strings for both axes, extracting `projp{n}=value`.
    for ax in [axlon, axlat] {
        let watmem = concat_wat(cards, ax);
        if watmem.is_empty() {
            continue;
        }
        for n in 0..10 {
            let needle = format!("projp{n}=");
            if let Some(pos) = watmem.find(&needle) {
                let rest = &watmem[pos + needle.len()..];
                let value: f64 = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
                    .collect::<String>()
                    .parse()
                    .unwrap_or(f64::NAN);
                if value.is_finite() {
                    cards.set_float(&format!("PV{axlat}_{n}"), value, "ZPN projection parameter");
                }
            }
        }
    }
}

/// Translation #18 helper: IRAF -TNX projections.
///
/// Replace `-TNX` with `-TPN` on both celestial CTYPEs and parse
/// the polynomial coefficients from the `WAT1_*` and `WAT2_*`
/// strings into `PV<axis>_m` cards (mirrors fitschan.c:31488-31538).
///
/// When the polynomial cannot be represented as a TPN (i.e. order
/// greater than 7, asymmetric axis orders, or full cross-terms) C
/// issues an `ASTWARN`; we silently leave the partially-translated
/// CTYPE in place. The caller's downstream reader will reject the
/// resulting broken TPN.
fn translate_iraf_tnx(cards: &mut CardList) {
    let Some((axlon, axlat)) = celestial_axes(cards, ' ') else {
        return;
    };
    let Some(ctype_lon) = cards.get_string(&format!("CTYPE{axlon}")) else {
        return;
    };
    if !projection_suffix(ctype_lon.trim()).is_some_and(|p| p.eq_ignore_ascii_case("TNX")) {
        return;
    }
    for ax in [axlon, axlat] {
        let key = format!("CTYPE{ax}");
        replace_projection_suffix(cards, &key, "TPN");
    }
    // Parse the polynomial coefficients out of the WAT strings.
    // Mirrors fitschan.c:31508-31538.
    for (i, axis) in [(0_usize, axlon), (1_usize, axlat)].iter().copied() {
        let watmem = concat_wat(cards, axis);
        if watmem.is_empty() {
            continue;
        }
        if let Some(pvs) = wat_coeffs(&watmem, i) {
            for (m, value) in pvs {
                cards.set_float(&format!("PV{axis}_{m}"), value, "TAN projection parameter");
            }
        }
    }
}

/// Port of fitschan.c::WATCoeffs (line 35102-35355). Extract
/// polynomial coefficients from the IRAF WAT-string `lngcor` /
/// `latcor` block and convert them into a list of (m, value) pairs
/// suitable for FITS-WCS `PVi_m` keys.
///
/// `iaxis` is 0 for longitude, 1 for latitude.
///
/// Returns `None` when the polynomial cannot be represented as a
/// TPN (corresponds to C `*ok = 0`).
fn wat_coeffs(watstr: &str, iaxis: usize) -> Option<Vec<(usize, f64)>> {
    // Look for `<axis>cor = "..."` and extract the quoted body.
    // C uses regex `cor *= *"(.*)"`. We approximate by hunting for
    // the first `cor = "` after either `lngcor` or `latcor`.
    let body_start = watstr.find("cor")?;
    let after_cor = &watstr[body_start + 3..];
    let q1 = after_cor.find('"')?;
    let after_q1 = &after_cor[q1 + 1..];
    let q2 = after_q1.find('"')?;
    let body = &after_q1[..q2];

    // Split into whitespace-delimited words.
    let words: Vec<&str> = body.split_ascii_whitespace().collect();
    if words.len() < 8 {
        return None;
    }
    let parsed: Vec<f64> = words
        .iter()
        .map(|w| w.parse::<f64>().unwrap_or(f64::NAN))
        .collect();
    if parsed.iter().any(|v| !v.is_finite()) {
        return None;
    }

    // word 0: surface type. 1 = chebyshev, 3 = simple polynomial,
    // 2 = legendre (unsupported).
    let cheb = match parsed[0] as i64 {
        1 => true,
        3 => false,
        _ => return None,
    };
    // words 1, 2: orders in xi/eta; must be equal, in 1..=7.
    let order = parsed[1] as i64;
    if !(1..=7).contains(&order) {
        return None;
    }
    if parsed[2] as i64 != order {
        return None;
    }
    let order = order as usize;
    // word 3: cross-term type; 2 = half-cross terms (only one
    // supported).
    if parsed[3] as i64 != 2 {
        return None;
    }
    // words 4-7: ximin, ximax, etamin, etamax.
    let ximin = parsed[4];
    let ximax = parsed[5];
    let etamin = parsed[6];
    let etamax = parsed[7];

    // Number of half-cross-term coefficients per order. Indexed
    // by porder = order - 1.
    const NAB: [usize; 8] = [1, 3, 6, 10, 15, 21, 28, 36];
    let porder = order - 1;
    if words.len() != 8 + NAB[porder] {
        return None;
    }

    // Fill an order×order matrix as if full cross terms were given.
    // Half-cross-term layout: xi power decreases from `order` down
    // to zero as we move through the coefficients (eta increments
    // when xi rolls under).
    let mut coeff = vec![0.0_f64; order * order];
    let mut xiorder = 0_usize;
    let mut etaorder = 0_usize;
    for &dval in parsed.iter().skip(8) {
        if etaorder >= order {
            break;
        }
        coeff[xiorder + order * etaorder] = dval;
        xiorder += 1;
        if xiorder == order - etaorder {
            xiorder = 0;
            etaorder += 1;
        }
    }

    // Convert from chebyshev to simple polynomial form if needed.
    if cheb {
        coeff = cheb2poly(&coeff, order, order, ximin, ximax, etamin, etamax);
    }

    // The polynomials are corrections; bump the linear coefficient
    // on the active axis by 1 so the result is the full mapping.
    if iaxis == 1 {
        coeff[order] += 1.0;
    } else {
        coeff[1] += 1.0;
    }

    // Walk the coefficient grid, emitting non-default entries.
    let mut out: Vec<(usize, f64)> = Vec::new();
    let mut pc = 0_usize;
    for et in 0..order {
        for xi in 0..order {
            let mn = xi + et;
            let default = if mn == 1 { 1.0 } else { 0.0 };
            if coeff[pc] != default {
                let mut m = mn * (1 + mn) / 2 + mn / 2;
                m += if iaxis == 1 { xi } else { et };
                out.push((m, coeff[pc]));
            }
            pc += 1;
        }
    }
    Some(out)
}

/// Convert a 2-D Chebyshev polynomial to standard polynomial form.
/// Mirrors fitschan.c::Cheb2Poly (line 5677-5825). `c` holds the
/// Chebyshev coefficients in column-major order
/// (`c[i + j*nx]` = coefficient of `Ti(u)·Tj(v)`); the returned
/// vector uses the same layout for power coefficients
/// (`d[i + j*nx]` = coefficient of `x^i · y^j`).
fn cheb2poly(
    c: &[f64],
    nx: usize,
    ny: usize,
    xmin: f64,
    xmax: f64,
    ymin: f64,
    ymax: f64,
) -> Vec<f64> {
    let mut d = vec![0.0_f64; nx * ny];

    // Process each row (chebyshev in u for fixed Tj(v)).
    for j in 0..ny {
        let row_in: &[f64] = &c[j * nx..(j + 1) * nx];
        let mut tmp = vec![0.0_f64; nx];
        chpc1(row_in, &mut tmp);
        let mut row_out = vec![0.0_f64; nx];
        shpc1(xmin, xmax, &tmp, &mut row_out);
        d[j * nx..(j + 1) * nx].copy_from_slice(&row_out);
    }

    // Process each column (chebyshev in v for fixed power-of-x).
    for i in 0..nx {
        let mut col_in = vec![0.0_f64; ny];
        for j in 0..ny {
            col_in[j] = d[i + j * nx];
        }
        let mut tmp = vec![0.0_f64; ny];
        chpc1(&col_in, &mut tmp);
        let mut col_out = vec![0.0_f64; ny];
        shpc1(ymin, ymax, &tmp, &mut col_out);
        for j in 0..ny {
            d[i + j * nx] = col_out[j];
        }
    }
    d
}

/// Mirrors fitschan.c::Chpc1 (line 6152-6260). Convert a 1-D
/// Chebyshev polynomial to the equivalent simple polynomial.
fn chpc1(c: &[f64], d: &mut [f64]) {
    let n = c.len();
    debug_assert_eq!(n, d.len());
    if n == 0 {
        return;
    }
    for slot in d.iter_mut() {
        *slot = 0.0;
    }
    if n == 1 {
        d[0] = c[0];
        return;
    }
    let mut w0 = vec![0_i64; n];
    let mut w1 = vec![0_i64; n];
    w0[0] = 1;
    w1[1] = 1;
    d[0] = c[0];
    d[1] = c[1];
    for k in 1..(n - 1) {
        // Update w1 to coefficients of T_{k+1}.
        for j in (1..n).rev() {
            let sv = w0[j];
            w0[j] = w1[j];
            w1[j] = 2 * w1[j - 1] - sv;
        }
        let sv = w0[0];
        w0[0] = w1[0];
        w1[0] = -sv;
        let coef = c[k + 1];
        for j in 0..=(k + 1) {
            d[j] += coef * (w1[j] as f64);
        }
    }
}

/// Mirrors fitschan.c::Shpc1 (line 28140-28265). Rescale a 1-D
/// polynomial defined on the unit interval to one defined on
/// `[xmin, xmax]`.
fn shpc1(xmin: f64, xmax: f64, d: &[f64], w: &mut [f64]) {
    let n = d.len();
    debug_assert_eq!(n, w.len());
    if n == 0 {
        return;
    }
    let a = 2.0 / (xmax - xmin);
    let b = (xmin + xmax) / (xmin - xmax);
    for slot in w.iter_mut() {
        *slot = 0.0;
    }
    w[0] = d[n - 1];
    for i in 1..n {
        for j in (1..=i).rev() {
            w[j] = b * w[j] + a * w[j - 1];
        }
        w[0] = d[n - i - 1] + b * w[0];
    }
}

/// Concatenate all `WAT{axis}_{nnn}` strings for the given 1-based
/// axis. Mirrors C `ConcatWAT` (fitschan.c:7999-8090) — IRAF emits
/// the WAT keys with 3-digit zero-padded indices.
fn concat_wat(cards: &CardList, axis: usize) -> String {
    let mut out = String::new();
    for n in 1..=999 {
        let key = format!("WAT{axis}_{n:03}");
        let Some(value) = cards.get_string(&key) else {
            break;
        };
        out.push_str(&value);
    }
    out
}
/// Translation #19 helper: MSX CAR projection longitude wrap.
///
/// CAR images that span more than 360° in pixel space need their
/// CRPIX shifted so the image's mid-pixel longitude lies in
/// [-180°, 180°]. Mirrors fitschan.c:31944-31972. Primary plane only;
/// no encoding gate. The C code is gated on `astGetCarLin(this)`
/// being false (the default); we mirror this default unconditionally
/// since FitsChan attribute support isn't wired through to
/// `spec_trans` yet.
fn translate_msx_car(cards: &mut CardList) {
    let Some((axlon, _axlat)) = celestial_axes(cards, ' ') else {
        return;
    };
    let Some(ctype_lon) = cards.get_string(&format!("CTYPE{axlon}")) else {
        return;
    };
    if !projection_suffix(ctype_lon.trim()).is_some_and(|p| p.eq_ignore_ascii_case("CAR")) {
        return;
    }
    let Some(cdelti) = cards.get_float(&format!("CDELT{axlon}")) else {
        return;
    };
    let Some(crpix_old) = cards.get_float(&format!("CRPIX{axlon}")) else {
        return;
    };
    if cdelti == 0.0 {
        return;
    }
    let dim = cards.get_int(&format!("NAXIS{axlon}")).unwrap_or(0) as f64;
    let offset = 0.5 * (dim + 1.0);
    // C: dval = offset + DR2D * palDrange(DD2R * (dval - offset) * cdelti) / cdelti.
    // palDrange wraps to [-π, π]. We use `rem_euclid` then shift.
    let theta_rad = (crpix_old - offset) * cdelti.to_radians();
    let wrapped = crate::pal::drange(theta_rad);
    let crpix_new = offset + wrapped.to_degrees() / cdelti;
    let comment = cards
        .find(&format!("CRPIX{axlon}"))
        .and_then(|c| c.comment.clone())
        .unwrap_or_default();
    cards.set_float(&format!("CRPIX{axlon}"), crpix_new, &comment);
}

/// Translation #20 helper: RESTFREQ → RESTFRQ with MHz/GHz unit
/// detection from the comment string. Mirrors fitschan.c:31975-32008.
/// Per-version letter; no encoding gate.
fn translate_restfreq(cards: &mut CardList) {
    for s in std::iter::once(' ').chain('A'..='Z') {
        let from = if s == ' ' {
            "RESTFREQ".to_string()
        } else {
            format!("RESTFREQ{s}")
        };
        let to = if s == ' ' {
            "RESTFRQ".to_string()
        } else {
            format!("RESTFRQ{s}")
        };
        let Some(value) = cards.get_float(&from) else {
            continue;
        };
        let comment = cards
            .find(&from)
            .and_then(|c| c.comment.clone())
            .unwrap_or_default();
        // Detect MHz / GHz suffix in the original comment and scale.
        let (scaled, new_comment) = if comment.contains("GHz") {
            (value * 1.0e9, "[Hz] Rest Frequency".to_string())
        } else if comment.contains("MHz") {
            (value * 1.0e6, "[Hz] Rest Frequency".to_string())
        } else {
            (value, comment)
        };
        cards.delete(&from);
        if cards.find(&to).is_none() {
            cards.set_float(&to, scaled, &new_comment);
        }
    }
}

/// Translation #21 helper: AIPS spectral CTYPE → modern FITS-WCS
/// equivalents (`VELO-LSR` → `VRAD` + SPECSYS=LSRK etc.) plus
/// `LAMBDA` → `WAVE`.
///
/// Mirrors fitschan.c:32021-32046 + the `IsAIPSSpectral` helper at
/// fitschan.c:20122-20218. Per-version letter; no encoding gate.
fn translate_aips_spectral_ctype(cards: &mut CardList) {
    for s in std::iter::once(' ').chain('A'..='Z') {
        for j in 1..=9 {
            let key = if s == ' ' {
                format!("CTYPE{j}")
            } else {
                format!("CTYPE{j}{s}")
            };
            let Some(ctype) = cards.get_string(&key) else {
                continue;
            };
            let trimmed = ctype.trim_end();
            if trimmed.len() == 8 {
                if let Some((wctype, wspecsys)) = is_aips_spectral(trimmed) {
                    let comment = cards
                        .find(&key)
                        .and_then(|c| c.comment.clone())
                        .unwrap_or_default();
                    cards.set_string(&key, wctype, &comment);
                    let specsys_key = if s == ' ' {
                        "SPECSYS".to_string()
                    } else {
                        format!("SPECSYS{s}")
                    };
                    cards.set_string(&specsys_key, wspecsys, "Spectral reference frame");
                    // C: `break` out of the per-axis loop after the
                    // first AIPS spectral hit.
                    break;
                }
            }
            if trimmed == "LAMBDA" {
                let comment = cards
                    .find(&key)
                    .and_then(|c| c.comment.clone())
                    .unwrap_or_default();
                cards.set_string(&key, "WAVE", &comment);
                break;
            }
        }
    }
}

/// Mirrors fitschan.c::IsAIPSSpectral (line 20122-20218). Returns
/// `Some((wctype, wspecsys))` for an AIPS spectral 8-char CTYPE,
/// otherwise `None`.
fn is_aips_spectral(ctype: &str) -> Option<(&'static str, &'static str)> {
    if ctype.len() != 8 {
        return None;
    }
    let prefix = &ctype[..4];
    let suffix = &ctype[4..];
    let wctype = match prefix {
        "FREQ" => "FREQ    ",
        "VELO" => "VRAD    ",
        "FELO" => "VOPT-F2W",
        "WAVELENG" => "WAVE    ",
        _ => return None,
    };
    let wspecsys = if suffix.starts_with("-LSR") || suffix.starts_with("LSRK") {
        "LSRK"
    } else if suffix.starts_with("-LSD") {
        "LSRD"
    } else if suffix.starts_with("-HEL") {
        "BARYCENT"
    } else if suffix.starts_with("-EAR") || suffix.starts_with("-GEO") {
        "GEOCENTR"
    } else if suffix.starts_with("-OBS") || suffix.starts_with("-TOP") {
        "TOPOCENT"
    } else {
        return None;
    };
    // C also requires both wctype and wspecsys to be set (already
    // ensured by the early-returns above).
    Some((wctype, wspecsys))
}

/// Translation #22 helper: case-insensitive CUNIT canonicalisation.
///
/// Maps "Hz" / "ANGSTROM" / "Km/S" etc. variants to the canonical
/// case. Mirrors fitschan.c:32049-32081. Per-version letter; no
/// encoding gate.
fn translate_cunit_case(cards: &mut CardList) {
    for s in std::iter::once(' ').chain('A'..='Z') {
        for j in 1..=9 {
            let key = if s == ' ' {
                format!("CUNIT{j}")
            } else {
                format!("CUNIT{j}{s}")
            };
            let Some(value) = cards.get_string(&key) else {
                continue;
            };
            let trimmed = value.trim_end();
            let canon = if trimmed.is_empty() {
                None
            } else if trimmed.eq_ignore_ascii_case("Hz") {
                Some("Hz")
            } else if trimmed.eq_ignore_ascii_case("Angstrom") {
                Some("Angstrom")
            } else if trimmed.eq_ignore_ascii_case("km/s") {
                Some("km/s")
            } else if trimmed.eq_ignore_ascii_case("m/s") {
                Some("m/s")
            } else {
                None
            };
            if let Some(c) = canon {
                let comment = cards
                    .find(&key)
                    .and_then(|c| c.comment.clone())
                    .unwrap_or_default();
                cards.set_string(&key, c, &comment);
            }
        }
    }
}

/// Translation #23 helper: CLASS-specific rewrites.
///
/// Mirrors fitschan.c::ClassTrans (line 6955-7129):
///
///  1. The spectral axis (the one that's neither lon nor lat) must
///     be `FREQ`. CRVAL on that axis is shifted by RESTFREQ (CLASS
///     stores spectral CRVAL as an offset from the rest frequency).
///  2. SPECSYS becomes `SOURCE`.
///  3. If CTYPE for the celestial pair has an empty projection
///     suffix, default to `-GLS` (which translation #11 then maps to
///     `-SFL`). C runs ClassTrans from inside the SpecTrans loop
///     before the GLS rewrite, so the rewrite chain works.
///  4. Pick a velocity reference from VELO-HEL / VELO-OBS / VELO-TOP
///     / VELO-EAR / VELO-GEO / VELO-LSR / VLSR; emit SSYSSRC and
///     ZSOURCE based on that reference.
///
/// FitsClass-only; primary plane only.
fn translate_class(cards: &mut CardList) {
    // Need the celestial pair to determine which axis is spectral.
    let Some((axlon, axlat)) = celestial_axes(cards, ' ') else {
        return;
    };
    // Spectral axis = the one that's neither lon nor lat among
    // {1, 2, 3}. Mirrors C:7027-7035 (uses 1-based axis indices).
    let axspec = match (axlon, axlat) {
        (1, 2) | (2, 1) => 3,
        (1, 3) | (3, 1) => 2,
        (2, 3) | (3, 2) => 1,
        _ => return, // unsupported axis layout
    };

    // Get RESTFREQ. Try the standard then the AIPS-spelling.
    let restfreq = cards
        .get_float("RESTFRQ")
        .or_else(|| cards.get_float("RESTFREQ"));
    let Some(restfreq) = restfreq else {
        // C errors out; we silently skip. Downstream readers will
        // produce a clearer error.
        return;
    };

    // Grab CRVAL on the spectral axis up-front so we can compute
    // v0 below, even after we mutate the card.
    let crval_spec_orig = cards.get_float(&format!("CRVAL{axspec}")).unwrap_or(0.0);

    // Spectral CTYPE check + CRVAL shift.
    if let Some(ctype_spec) = cards.get_string(&format!("CTYPE{axspec}")) {
        let trimmed = ctype_spec.trim();
        if trimmed.eq_ignore_ascii_case("FREQ") {
            // CRVAL_spec += RESTFREQ.
            let new_crval = crval_spec_orig + restfreq;
            let comment = cards
                .find(&format!("CRVAL{axspec}"))
                .and_then(|c| c.comment.clone())
                .unwrap_or_default();
            cards.set_float(&format!("CRVAL{axspec}"), new_crval, &comment);
        }
        // C errors out for non-FREQ axes; we silently skip.
    }

    // SPECSYS = SOURCE. CLASS frequency axes describe source-frame
    // frequencies.
    cards.set_string("SPECSYS", "SOURCE", "Standard of rest");

    // Default empty projection suffix to -GLS (later remapped to
    // -SFL by translation #11). The CTYPE may have trailing blanks
    // already that look like an "empty" suffix.
    for ax in [axlon, axlat] {
        let key = format!("CTYPE{ax}");
        if let Some(ctype) = cards.get_string(&key)
            && ctype.len() > 4
            && ctype[4..].chars().all(|c| c == ' ' || c == '\0')
        {
            let mut new_ctype = ctype[..4].to_string();
            new_ctype.push_str("-GLS");
            let comment = cards
                .find(&key)
                .and_then(|c| c.comment.clone())
                .unwrap_or_default();
            cards.set_string(&key, &new_ctype, &comment);
        }
    }

    // Look for velocity-reference keywords.
    let (ssyssrc, vref) = if let Some(v) = cards.get_float("VELO-HEL") {
        ("BARYCENT", v)
    } else if let Some(v) = cards
        .get_float("VELO-OBS")
        .or_else(|| cards.get_float("VELO-TOP"))
    {
        ("TOPOCENT", v)
    } else if let Some(v) = cards
        .get_float("VELO-EAR")
        .or_else(|| cards.get_float("VELO-GEO"))
    {
        ("GEOCENTR", v)
    } else if let Some(v) = cards
        .get_float("VELO-LSR")
        .or_else(|| cards.get_float("VLSR"))
    {
        ("LSRK", v)
    } else {
        ("LSRK", 0.0)
    };

    // Compute radio velocity at the reference channel:
    //   v0 = c * (restfreq - crval_spec_orig) / restfreq
    // (C uses the *original* spectral CRVAL, not the post-shift
    // value — see fitschan.c:7117).
    const C_LIGHT_M_S: f64 = 299_792_458.0;
    let v0 = C_LIGHT_M_S * (restfreq - crval_spec_orig) / restfreq;
    let vsource = vref - v0;

    cards.set_string("SSYSSRC", ssyssrc, "Source standard of rest");

    // Convert from radio velocity to redshift; ZSOURCE = c/(c-vsource) - 1.
    let zsource = C_LIGHT_M_S / (C_LIGHT_M_S - vsource) - 1.0;
    cards.set_float("ZSOURCE", zsource, "Redshift of source");
}

/// Translation #24 helper: strip IRAF mini-WCS keywords (`WAT*`,
/// `MWCS*`, `WSV*`, `LTM*`, `LTV*`). These are IRAF-specific
/// equivalents of the AST native serialisation; carrying them
/// through the read pipeline causes encoding ambiguity.
///
/// Mirrors fitschan.c:32084-32110. No encoding gate; runs once.
fn translate_iraf_mini_wcs_strip(cards: &mut CardList) {
    let mut to_remove: Vec<String> = Vec::new();
    for c in cards.iter() {
        let kw = c.keyword.as_str();
        let starts_with_digit_pair = |prefix: &str| {
            // Match patterns like "WAT1_001" / "MWCS3" / "WSV1_LEN".
            kw.starts_with(prefix) && {
                let rest = &kw[prefix.len()..];
                rest.chars().next().is_some_and(|c| c.is_ascii_digit())
            }
        };
        if starts_with_digit_pair("WAT")
            || starts_with_digit_pair("MWCS")
            || starts_with_digit_pair("LTM")
            || starts_with_digit_pair("LTV")
            || starts_with_digit_pair("WSV")
        {
            to_remove.push(kw.to_string());
        }
    }
    for kw in to_remove {
        cards.delete(&kw);
    }
}

/// Translation #11 helper: AIPS `-NCP` / `-GLS` projections.
///
/// `-NCP` is replaced with `-SIN` and PV<axlat>_1=0, PV<axlat>_2=cot(δ_ref)
/// per FITS-WCS paper II Sec. 5.1.5. Skipped when sin(δ_ref) is zero
/// (the limit doesn't apply at the equator).
///
/// `-GLS` is renamed to `-SFL`. The C source has a "shifted" branch
/// for non-origin CRVAL but it's permanently disabled (`if( 0 &&
/// shifted )`); we mirror the disabled state.
///
/// Mirrors fitschan.c:31682-31775. Per-version letter; no encoding gate.
fn translate_ncp_gls(cards: &mut CardList) {
    for s in std::iter::once(' ').chain('A'..='Z') {
        let Some((axlon, axlat)) = celestial_axes(cards, s) else {
            continue;
        };
        let key = |prefix: &str, n: usize| {
            if s == ' ' {
                format!("{prefix}{n}")
            } else {
                format!("{prefix}{n}{s}")
            }
        };
        let lon_ctype = match cards.get_string(&key("CTYPE", axlon)) {
            Some(c) => c.trim().to_string(),
            None => continue,
        };
        let lat_ctype = match cards.get_string(&key("CTYPE", axlat)) {
            Some(c) => c.trim().to_string(),
            None => continue,
        };
        let lon_prj = projection_suffix(&lon_ctype);
        let lat_prj = projection_suffix(&lat_ctype);
        if lon_prj != lat_prj {
            continue;
        }
        let Some(prj) = lon_prj else { continue };
        if prj.eq_ignore_ascii_case("NCP") {
            // Need CRVAL{axlat}{s} for the cot calculation.
            let Some(crval_lat_deg) = cards.get_float(&key("CRVAL", axlat)) else {
                continue;
            };
            let sinval = crval_lat_deg.to_radians().sin();
            if sinval == 0.0 {
                continue;
            }
            let cot = crval_lat_deg.to_radians().cos() / sinval;
            // Replace -NCP with -SIN on both CTYPEs.
            replace_projection_suffix(cards, &key("CTYPE", axlon), "SIN");
            replace_projection_suffix(cards, &key("CTYPE", axlat), "SIN");
            // PV{axlat}_1 = 0, PV{axlat}_2 = cot(δ_ref).
            let pv_key = |i: usize, m: usize| {
                if s == ' ' {
                    format!("PV{i}_{m}")
                } else {
                    format!("PV{i}_{m}{s}")
                }
            };
            cards.set_float(&pv_key(axlat, 1), 0.0, "Projection parameter");
            cards.set_float(&pv_key(axlat, 2), cot, "Projection parameter");
        } else if prj.eq_ignore_ascii_case("GLS") {
            // Rename -GLS to -SFL on both CTYPEs.
            replace_projection_suffix(cards, &key("CTYPE", axlon), "SFL");
            replace_projection_suffix(cards, &key("CTYPE", axlat), "SFL");
        }
    }
}

/// Return the 3-letter projection suffix at characters 5-7 of a CTYPE
/// (e.g. "TAN" for "RA---TAN"). Returns `None` when the CTYPE is
/// shorter than 8 characters or has no `-` at position 4.
fn projection_suffix(ctype: &str) -> Option<&str> {
    if ctype.len() < 8 {
        return None;
    }
    if &ctype[4..5] != "-" {
        return None;
    }
    Some(&ctype[5..8])
}

/// Replace the 3-letter projection suffix in `cards[key]` with `new_prj`.
/// No-op if the card is missing or its value isn't an 8-char CTYPE.
fn replace_projection_suffix(cards: &mut CardList, key: &str, new_prj: &str) {
    let Some(ctype) = cards.get_string(key) else {
        return;
    };
    let ctype = ctype.trim_end();
    if ctype.len() < 8 {
        return;
    }
    let mut new_ctype = ctype[..5].to_string();
    new_ctype.push_str(new_prj);
    let comment = cards
        .find(key)
        .and_then(|c| c.comment.clone())
        .unwrap_or_default();
    cards.set_string(key, &new_ctype, &comment);
}

/// Translation #9 helper: DATE-OBS → MJD-OBS (fitschan.c:31634-31655).
///
/// Only fires when DATE-OBS is present and MJD-OBS is absent. The
/// DATE-OBS card is left in place — C marks it as "used" but doesn't
/// delete it; downstream readers see only MJD-OBS take effect because
/// MJD-OBS has priority in `read_time_keywords`.
fn translate_date_obs(cards: &mut CardList) {
    if cards.find("MJD-OBS").is_some() {
        return;
    }
    let Some(date) = cards.get_string("DATE-OBS") else {
        return;
    };
    let Some(mjd) = crate::fitschan::wcs_write::date_str_to_mjd(&date) else {
        return;
    };
    cards.set_float("MJD-OBS", mjd, "Date of observation");
}

/// Translations 7, 8, 10 (fitschan.c:31580-31633). Three steps that
/// run in sequence on the primary plane:
///
///  1. EPOCH (float) → EQUINOX (float) when EQUINOX is absent. A
///     value of 0.0 is treated as 1950.0 (mirrors C:31585-31592).
///
///  2. String-valued EQUINOX with a 'B' or 'J' prefix is parsed,
///     converted to Julian via palEpj(palEpb2d(...)) when Besselian,
///     and stored as a float. C does this *after* the EPOCH branch
///     so a string EQUINOX wins over an EPOCH value.
///
///  3. EQUINOX = 0.0 (float) is replaced with 1950.0 (mirrors
///     C:31618-31625).
fn translate_epoch_equinox(cards: &mut CardList) {
    // Step 1: EPOCH → EQUINOX (when EQUINOX absent).
    if let Some(epoch) = cards.get_float("EPOCH") {
        let comment = cards
            .find("EPOCH")
            .and_then(|c| c.comment.clone())
            .unwrap_or_else(|| "Reference equinox".to_string());
        cards.delete("EPOCH");
        let value = if epoch == 0.0 { 1950.0 } else { epoch };
        if cards.find("EQUINOX").is_none() {
            cards.set_float("EQUINOX", value, &comment);
        }
        // Else: EQUINOX wins; EPOCH already consumed by delete above.
    }

    // Step 2: String EQUINOX 'B...' or 'J...' → numeric Julian.
    if let Some(s) = cards.get_string("EQUINOX") {
        let s = s.trim();
        let prefix = s.chars().next().map(|c| c.to_ascii_uppercase());
        if matches!(prefix, Some('B') | Some('J')) {
            let bj = prefix.unwrap();
            if let Ok(year) = s[1..].trim().parse::<f64>() {
                let julian = if bj == 'B' {
                    crate::pal::epj(crate::pal::epb2d(year))
                } else {
                    year
                };
                let comment = cards
                    .find("EQUINOX")
                    .and_then(|c| c.comment.clone())
                    .unwrap_or_else(|| "Reference equinox".to_string());
                cards.delete("EQUINOX");
                cards.set_float("EQUINOX", julian, &comment);
            }
        }
    }

    // Step 3: EQUINOX = 0 → 1950.
    if let Some(0.0) = cards.get_float("EQUINOX") {
        let comment = cards
            .find("EQUINOX")
            .and_then(|c| c.comment.clone())
            .unwrap_or_else(|| "Reference equinox".to_string());
        cards.delete("EQUINOX");
        cards.set_float("EQUINOX", 1950.0, &comment);
    }
}

/// Translation #6 helper: rewrite the FITS-PC alt-axis form
/// `C{m}{VAL,PIX,YPE,NIT,ELT}{i}` to the standard
/// `{CRVAL,CRPIX,CTYPE,CUNIT,CDELT}{i}{S}` form (S = 'A' + m-1).
/// Mirrors fitschan.c:31437-31578.
///
/// The CmELTi → CDELTi{S} branch additionally backfills zero
/// CDELT values from the corresponding CRVAL (× 0.01), or 1.0 when
/// CRVAL is also zero — to match C's "redundant axis" workaround
/// (fitschan.c:31543-31548).
///
/// The CmELTi branch in C also propagates a per-version PC matrix
/// from any 6-digit `PC{jjj}{iii}` cards present (fitschan.c:31556-
/// 31573). That branch only ever fires under FITS-IRAF (which is
/// the only encoding that emits `PC{jjj}{iii}` form on write); we
/// port it as-is so the round-trip is exact.
fn translate_cnxxx(cards: &mut CardList) {
    // Discover (m, axis_max) ranges. m ∈ 1..=9 (alt letter A..I).
    for m in 1..=9 {
        let alt = (b'A' + (m - 1) as u8) as char;
        // Translate value-style cards (CRVAL/CRPIX/CTYPE/CUNIT) one-for-one.
        for kind in ["VAL", "PIX", "YPE", "NIT"] {
            let dst_prefix = match kind {
                "VAL" => "CRVAL",
                "PIX" => "CRPIX",
                "YPE" => "CTYPE",
                "NIT" => "CUNIT",
                _ => unreachable!(),
            };
            for i in 1..=9 {
                let from = format!("C{m}{kind}{i}");
                let to = format!("{dst_prefix}{i}{alt}");
                rename_keyword(cards, &from, &to);
            }
        }
        // CmELTi → CDELTi{S} with the zero-fix and PC propagation.
        // First find naxis (highest C{m}ELT index).
        let mut naxis_m = 0;
        for j in 1..=9 {
            if cards.find(&format!("C{m}ELT{j}")).is_some() {
                naxis_m = j;
            }
        }
        if naxis_m == 0 {
            continue;
        }
        // Per-row CDELT.
        for j in 1..=naxis_m {
            let cdelt_key = format!("C{m}ELT{j}");
            let mut cdeltj = cards.get_float(&cdelt_key).unwrap_or(0.0);
            cards.delete(&cdelt_key);
            if cdeltj == 0.0 {
                // Fill in from CRVAL{j}{S} (already translated above).
                let crval_alt = cards.get_float(&format!("CRVAL{j}{alt}")).unwrap_or(0.0);
                cdeltj = 0.01 * crval_alt;
                if cdeltj == 0.0 {
                    cdeltj = 1.0;
                }
            }
            cards.set_float(&format!("CDELT{j}{alt}"), cdeltj, "Pixel scale");
        }
        // PCjjjiii → PCj_i{S} for the CmELTi block (mirrors C:31556-31573).
        for j in 1..=naxis_m {
            for i in 1..=naxis_m {
                let pc_3digit = format!("PC{j:03}{i:03}");
                let value = cards.get_float(&pc_3digit);
                let dval = match value {
                    Some(v) => v,
                    None => {
                        if i == j {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                cards.set_float(&format!("PC{j}_{i}{alt}"), dval, "PC matrix element");
            }
        }
    }
    // Drop any leftover PCjjjiii cards (consumed above for FITS-IRAF
    // alt-axis CDELT branch). C marks them used; we delete.
    for j in 1..=9 {
        for i in 1..=9 {
            cards.delete(&format!("PC{j:03}{i:03}"));
        }
    }
}

/// Translation #5 helper: PROJPi → PV<axlat>_i. Mirrors
/// fitschan.c:31386-31435. Primary plane only.
fn translate_projp(cards: &mut CardList) {
    // Need celestial axes (specifically the latitude axis) to know
    // where to write PV cards.
    let Some((axlon, axlat)) = celestial_axes(cards, ' ') else {
        return;
    };
    // Determine which PROJP indices are present.
    let mut projp_indices: Vec<usize> = Vec::new();
    for i in 0..=9 {
        if cards.find(&format!("PROJP{i}")).is_some() {
            projp_indices.push(i);
        }
    }
    if projp_indices.is_empty() {
        return;
    }
    // Decide whether to use PROJP. Default: yes. C's
    // disagreement-detection at fitschan.c:31390-31419: if any PV
    // values exist anywhere in the chan, default to using them — but
    // re-enable PROJP when PV{axlon}_{0..2} indicate the user's PV
    // set is incomplete.
    let any_pv = (1..=9).any(|i| (0..=9).any(|m| cards.find(&format!("PV{i}_{m}")).is_some()));
    let mut use_projp = true;
    if any_pv {
        use_projp = false;
        let pv_lon_0 = cards.find(&format!("PV{axlon}_0")).is_some();
        let pv_lon_1 = cards.find(&format!("PV{axlon}_1")).is_some();
        let pv_lon_2 = cards.find(&format!("PV{axlon}_2")).is_some();
        if pv_lon_1 || pv_lon_2 {
            // PV present but PV0 missing → use PROJP.
            if !pv_lon_0 {
                use_projp = true;
            }
        } else if pv_lon_0 {
            // Only PV0 present → use PROJP.
            use_projp = true;
        }
    }
    if !use_projp {
        // Drop the PROJP cards anyway (they've been "used").
        for i in projp_indices {
            cards.delete(&format!("PROJP{i}"));
        }
        return;
    }
    for i in projp_indices {
        let from = format!("PROJP{i}");
        let Some(value) = cards.get_float(&from) else {
            continue;
        };
        let comment = cards
            .find(&from)
            .and_then(|c| c.comment.clone())
            .unwrap_or_default();
        cards.delete(&from);
        cards.set_float(&format!("PV{axlat}_{i}"), value, &comment);
    }
}

fn translate_crota(cards: &mut CardList) {
    let Some((axlon, axlat)) = celestial_axes(cards, ' ') else {
        return;
    };
    // C reads CROTA on the longitude axis first, then the latitude
    // axis — so latitude wins when both are present (fitschan.c:31298-31307).
    let crota_deg = cards
        .get_float(&format!("CROTA{axlat}"))
        .or_else(|| cards.get_float(&format!("CROTA{axlon}")));
    let Some(crota_deg) = crota_deg else { return };
    // If a PC matrix is already present, C still consumes CROTA but
    // does not derive a new PC matrix (fitschan.c:31280-31300). Drop
    // the CROTA cards so they don't pollute downstream readers.
    let has_pc = cards.find(&format!("PC{axlon}_{axlon}")).is_some()
        || cards.find(&format!("PC{axlon}_{axlat}")).is_some()
        || cards.find(&format!("PC{axlat}_{axlon}")).is_some()
        || cards.find(&format!("PC{axlat}_{axlat}")).is_some();
    if !has_pc {
        let cdelt_lon = cards.get_float(&format!("CDELT{axlon}"));
        let cdelt_lat = cards.get_float(&format!("CDELT{axlat}"));
        if let (Some(cdelt_lon), Some(cdelt_lat)) = (cdelt_lon, cdelt_lat) {
            let cosrota = crota_deg.to_radians().cos();
            let sinrota = crota_deg.to_radians().sin();
            let lambda = cdelt_lat / cdelt_lon;
            let naxis = primary_naxis(cards);
            // Diagonal: cos(crota) on celestial axes, 1.0 elsewhere.
            for i in 1..=naxis {
                let val = if i == axlon || i == axlat {
                    cosrota
                } else {
                    1.0
                };
                cards.set_float(&format!("PC{i}_{i}"), val, "PC matrix element");
            }
            // Off-diagonal celestial entries.
            cards.set_float(
                &format!("PC{axlat}_{axlon}"),
                sinrota / lambda,
                "PC matrix element",
            );
            cards.set_float(
                &format!("PC{axlon}_{axlat}"),
                -sinrota * lambda,
                "PC matrix element",
            );
        }
    }
    // Drop all CROTA cards so they don't reach the downstream reader.
    for j in 1..=9 {
        cards.delete(&format!("CROTA{j}"));
    }
}

/// Move a keyword's value and comment from `from` to `to`, deleting
/// the original card.
///
/// If `to` is already present, the source is dropped (matches C
/// `SpecTrans` semantics: an existing standard value is never
/// overwritten by its non-standard alias). Both string-valued and
/// float-valued cards are handled; integer values are formatted
/// as floats since the destinations in this module are all
/// continuous numeric quantities.
///
/// Returns `true` if a rename actually occurred (i.e. `from` was
/// present and `to` was absent).
fn rename_keyword(cards: &mut CardList, from: &str, to: &str) -> bool {
    if cards.find(to).is_some() {
        // Destination present — discard the source.
        cards.delete(from);
        return false;
    }
    let comment = cards
        .find(from)
        .and_then(|c| c.comment.clone())
        .unwrap_or_default();
    if let Some(value) = cards.get_float(from) {
        cards.delete(from);
        cards.set_float(to, value, &comment);
        return true;
    }
    if let Some(value) = cards.get_string(from) {
        cards.delete(from);
        cards.set_string(to, &value, &comment);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_trans_is_a_no_op_when_all_keywords_are_already_standard() {
        let mut cards = CardList::from_header(
            "CRVAL1  =                180.0\n\
             CRVAL2  =                 45.0\n\
             CTYPE1  = 'RA---TAN'\n\
             CTYPE2  = 'DEC--TAN'\n\
             RADESYS = 'FK5     '\n\
             EQUINOX =               2000.0\n\
             END",
        )
        .unwrap();
        let before = cards.iter().count();
        spec_trans(&mut cards, Encoding::FitsWcs);
        let after = cards.iter().count();
        assert_eq!(before, after);
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn rename_keyword_moves_value_and_comment_to_new_name() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK5     '           / Reference frame\nEND").unwrap();
        assert!(rename_keyword(&mut cards, "RADECSYS", "RADESYS"));
        assert!(cards.find("RADECSYS").is_none());
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn rename_keyword_no_op_when_source_absent() {
        let mut cards = CardList::from_header("END").unwrap();
        assert!(!rename_keyword(&mut cards, "RADECSYS", "RADESYS"));
        assert!(cards.find("RADECSYS").is_none());
        assert!(cards.find("RADESYS").is_none());
    }

    #[test]
    fn rename_keyword_no_op_when_destination_already_present() {
        let mut cards = CardList::from_header(
            "RADECSYS= 'FK4     '           / Old\n\
             RADESYS = 'FK5     '           / New\n\
             END",
        )
        .unwrap();
        assert!(!rename_keyword(&mut cards, "RADECSYS", "RADESYS"));
        // Source consumed; destination preserved.
        assert!(cards.find("RADECSYS").is_none());
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn translates_radecsys_to_radesys() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK5     '           / Old-style frame\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("RADECSYS").is_none());
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn keeps_radesys_when_both_present() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK4     '\nRADESYS = 'FK5     '\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }

    #[test]
    fn translates_longpole_to_lonpole() {
        let mut cards =
            CardList::from_header("LONGPOLE=                180.0 / Old name\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("LONGPOLE").is_none());
        assert_eq!(cards.get_float("LONPOLE"), Some(180.0));
    }

    #[test]
    fn translates_cdjjjiii_to_pc_with_unit_cdelt_for_fits_iraf() {
        let mut cards = CardList::from_header(
            "CD001001=                 -2.0\n\
             CD001002=                  0.0\n\
             CD002001=                  0.0\n\
             CD002002=                  3.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsIraf);
        assert!(cards.find("CD001001").is_none());
        assert_eq!(cards.get_float("PC1_1"), Some(-2.0));
        assert_eq!(cards.get_float("PC2_2"), Some(3.0));
        // CDELT is forced to 1.0 for every row that had a CDjjjiii.
        assert_eq!(cards.get_float("CDELT1"), Some(1.0));
        assert_eq!(cards.get_float("CDELT2"), Some(1.0));
    }

    #[test]
    fn cdjjjiii_ignored_for_non_iraf_encoding() {
        // C only translates CDjjjiii under FITS-IRAF. For other
        // encodings the legacy keys are silently left in place; the
        // downstream readers won't recognise them.
        let mut cards = CardList::from_header(
            "CD001001=                  1.0\n\
             CD002002=                  1.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("CD001001").is_some());
        assert!(cards.find("PC1_1").is_none());
    }

    #[test]
    fn translates_crota_to_pc_matrix() {
        // CROTA2 = 30°, CDELT2/CDELT1 = -1.0 → lambda = -1.
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---TAN'\n\
             CTYPE2  = 'DEC--TAN'\n\
             CRPIX1  =                  0.0\n\
             CRPIX2  =                  0.0\n\
             CRVAL1  =                  0.0\n\
             CRVAL2  =                  0.0\n\
             CDELT1  =                 -1.0\n\
             CDELT2  =                  1.0\n\
             CROTA2  =                 30.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("CROTA2").is_none());
        let cos30 = 30.0_f64.to_radians().cos();
        assert!((cards.get_float("PC1_1").unwrap() - cos30).abs() < 1e-10);
        assert!((cards.get_float("PC2_2").unwrap() - cos30).abs() < 1e-10);
        // CDELT preserved.
        assert_eq!(cards.get_float("CDELT1"), Some(-1.0));
        assert_eq!(cards.get_float("CDELT2"), Some(1.0));
    }

    #[test]
    fn crota_drops_when_pc_already_present() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---TAN'\n\
             CTYPE2  = 'DEC--TAN'\n\
             CDELT1  =                  1.0\n\
             CDELT2  =                  1.0\n\
             PC1_1   =                  1.0\n\
             PC2_2   =                  1.0\n\
             CROTA2  =                 30.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        // CROTA card was consumed.
        assert!(cards.find("CROTA2").is_none());
        // PC values were not overwritten.
        assert_eq!(cards.get_float("PC1_1"), Some(1.0));
    }

    #[test]
    fn translates_projp_to_pv_on_latitude_axis() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---ZPN'\n\
             CTYPE2  = 'DEC--ZPN'\n\
             PROJP0  =                  0.0\n\
             PROJP1  =                  1.0\n\
             PROJP2  =                 -0.4\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("PROJP0").is_none());
        assert!(cards.find("PROJP1").is_none());
        assert_eq!(cards.get_float("PV2_0"), Some(0.0));
        assert_eq!(cards.get_float("PV2_1"), Some(1.0));
        assert_eq!(cards.get_float("PV2_2"), Some(-0.4));
    }

    #[test]
    fn projp_skipped_when_consistent_pv_set_present() {
        // PV0+PV1+PV2 all set on lon → C trusts PV, PROJP dropped.
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---ZPN'\n\
             CTYPE2  = 'DEC--ZPN'\n\
             PROJP0  =                999.0\n\
             PV1_0   =                  0.0\n\
             PV1_1   =                  1.0\n\
             PV1_2   =                 -0.4\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        // PROJP card consumed, PV2_0 not written from PROJP.
        assert!(cards.find("PROJP0").is_none());
        assert!(cards.find("PV2_0").is_none());
    }

    #[test]
    fn projp_used_when_pv_set_is_incomplete() {
        // PV1+PV2 set but PV0 missing → C says PV is incomplete,
        // falls back to PROJP for the parameter set.
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---ZPN'\n\
             CTYPE2  = 'DEC--ZPN'\n\
             PROJP0  =                  0.0\n\
             PV1_1   =                  1.0\n\
             PV1_2   =                 -0.4\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        // PROJP0 consumed and rewritten as PV2_0.
        assert_eq!(cards.get_float("PV2_0"), Some(0.0));
    }

    #[test]
    fn translates_cnxxx_to_alt_letter_form_for_fits_pc() {
        let mut cards = CardList::from_header(
            "C1YPE1  = 'GLON-TAN'\n\
             C1YPE2  = 'GLAT-TAN'\n\
             C1VAL1  =              101.255\n\
             C1VAL2  =               16.177\n\
             C1PIX1  =                 50.5\n\
             C1PIX2  =                 50.5\n\
             C1ELT1  =                -0.01\n\
             C1ELT2  =                 0.01\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("C1YPE1").is_none());
        assert!(cards.find("C1ELT1").is_none());
        assert_eq!(cards.get_string("CTYPE1A"), Some("GLON-TAN".to_string()));
        assert_eq!(cards.get_string("CTYPE2A"), Some("GLAT-TAN".to_string()));
        assert_eq!(cards.get_float("CRVAL1A"), Some(101.255));
        assert_eq!(cards.get_float("CRPIX1A"), Some(50.5));
        assert_eq!(cards.get_float("CDELT1A"), Some(-0.01));
        // Default-identity PC matrix for an alt CDELT block (no
        // PCjjjiii present in input, so diagonals = 1, off-diags = 0).
        assert_eq!(cards.get_float("PC1_1A"), Some(1.0));
        assert_eq!(cards.get_float("PC1_2A"), Some(0.0));
        assert_eq!(cards.get_float("PC2_2A"), Some(1.0));
    }

    #[test]
    fn cnxxx_translation_skipped_for_fits_wcs() {
        // FITS-WCS uses *A..*Z directly; Cnxxx is foreign and should
        // be left alone.
        let mut cards = CardList::from_header("C1YPE1  = 'GLON-TAN'\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsWcs);
        assert!(cards.find("C1YPE1").is_some());
        assert!(cards.find("CTYPE1A").is_none());
    }

    #[test]
    fn cnxxx_zero_cdelt_falls_back_to_crval_or_one() {
        // CmELTi = 0 should be replaced by 0.01 * CRVAL or 1.0
        // (mirrors C:31543-31548).
        let mut cards = CardList::from_header(
            "C1YPE1  = 'GLON-TAN'\n\
             C1VAL1  =                100.0\n\
             C1ELT1  =                  0.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_float("CDELT1A"), Some(1.0));
    }

    #[test]
    fn translates_epoch_to_equinox() {
        let mut cards = CardList::from_header("EPOCH   =               1995.0\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("EPOCH").is_none());
        assert_eq!(cards.get_float("EQUINOX"), Some(1995.0));
    }

    #[test]
    fn epoch_zero_becomes_b1950_equinox() {
        let mut cards = CardList::from_header("EPOCH   =                  0.0\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_float("EQUINOX"), Some(1950.0));
    }

    #[test]
    fn epoch_consumed_when_equinox_present() {
        let mut cards = CardList::from_header(
            "EPOCH   =               1995.0\nEQUINOX =               2000.0\nEND",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("EPOCH").is_none());
        assert_eq!(cards.get_float("EQUINOX"), Some(2000.0));
    }

    #[test]
    fn equinox_zero_becomes_b1950() {
        let mut cards = CardList::from_header("EQUINOX =                  0.0\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_float("EQUINOX"), Some(1950.0));
    }

    #[test]
    fn translates_equinox_b1995_string_to_julian_number() {
        let mut cards = CardList::from_header("EQUINOX = 'B1995.0'\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        let eq = cards.get_float("EQUINOX").unwrap_or(-1.0);
        // B1995 → MJD ≈ 49718.2 → J ≈ 1994.9988 (Besselian and
        // Julian years drift ~1 day per ~70 years between epochs).
        assert!(
            (eq - 1994.9988293).abs() < 1e-4,
            "expected ~1994.9988, got {eq}"
        );
    }

    #[test]
    fn translates_equinox_j2000_string_to_plain_number() {
        let mut cards = CardList::from_header("EQUINOX = 'J2000.0'\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_float("EQUINOX"), Some(2000.0));
    }

    #[test]
    fn translates_date_obs_to_mjd_obs() {
        let mut cards = CardList::from_header("DATE-OBS= '2000-01-15T12:00:00'\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        let mjd = cards.get_float("MJD-OBS").unwrap();
        assert!((mjd - 51558.5).abs() < 1e-6);
        // DATE-OBS stays in the chan; downstream readers ignore it
        // when MJD-OBS is present.
        assert!(cards.find("DATE-OBS").is_some());
    }

    #[test]
    fn date_obs_skipped_when_mjd_obs_present() {
        let mut cards = CardList::from_header(
            "DATE-OBS= '2000-01-15T12:00:00'\nMJD-OBS =              50000.0\nEND",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_float("MJD-OBS"), Some(50000.0));
    }

    #[test]
    fn translates_ncp_to_sin_with_pv2_2() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---NCP'\n\
             CTYPE2  = 'DEC--NCP'\n\
             CRVAL2  =                 30.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---SIN".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--SIN".to_string()));
        let cot30 = 1.0 / 30.0_f64.to_radians().tan();
        assert!((cards.get_float("PV2_2").unwrap() - cot30).abs() < 1e-10);
        assert_eq!(cards.get_float("PV2_1"), Some(0.0));
    }

    #[test]
    fn translates_gls_to_sfl() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---GLS'\n\
             CTYPE2  = 'DEC--GLS'\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---SFL".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--SFL".to_string()));
    }

    #[test]
    fn ncp_at_equator_left_alone() {
        // sin(0) = 0 → C gives up rather than divide by zero. We
        // mirror that: the CTYPE keeps its original NCP form.
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---NCP'\n\
             CTYPE2  = 'DEC--NCP'\n\
             CRVAL2  =                  0.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--NCP".to_string()));
    }

    #[test]
    fn early_paper_iii_wave_frq_becomes_wave_f2w() {
        let mut cards = CardList::from_header("CTYPE3  = 'WAVE-FRQ'\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE3"), Some("WAVE-F2W".to_string()));
    }

    #[test]
    fn early_paper_iii_velo_vel_redundant_blanks() {
        // VELO with X='V' and P='V' (because VELO maps to V) → blanks.
        let mut cards = CardList::from_header("CTYPE3  = 'VELO-VEL'\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        // Trailing-blank padding to 8 chars.
        assert_eq!(cards.get_string("CTYPE3"), Some("VELO    ".to_string()));
    }

    #[test]
    fn translates_sao_co_to_pv_and_tpv() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---TAN'\n\
             CTYPE2  = 'DEC--TAN'\n\
             CO1_1   =                  0.0\n\
             CO1_2   =                  1.0\n\
             CO1_3   =                  0.0\n\
             CO1_5   =                  0.5\n\
             CO2_1   =                  0.0\n\
             CO2_2   =                  1.0\n\
             CO2_3   =                  0.0\n\
             CO2_5   =                  0.7\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        // CTYPE rewrites: SAO writes TPV, then translation 16 rewrites
        // TPV→TPN before this assertion runs.
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---TPN".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--TPN".to_string()));
        // PV1_5 = co[0][5] (1-based: CO1_6 — but only CO1_5=0.5 set).
        // PV1_1 = co[0][1] = CO1_2 = 1.0; PV1_2 = co[0][2] = CO1_3 = 0.0.
        assert_eq!(cards.get_float("PV1_1"), Some(1.0));
        assert_eq!(cards.get_float("PV2_2"), Some(1.0));
        // Identity PC + unit CDELT (the SAO polynomial subsumes them).
        assert_eq!(cards.get_float("PC1_1"), Some(1.0));
        assert_eq!(cards.get_float("CDELT1"), Some(1.0));
    }

    #[test]
    fn translates_atf_to_ait() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---ATF'\n\
             CTYPE2  = 'DEC--ATF'\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsClass);
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---AIT".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--AIT".to_string()));
    }

    #[test]
    fn translates_qv_to_pv_and_tan_to_tpn() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---TAN'\n\
             CTYPE2  = 'DEC--TAN'\n\
             QV2_1   =                  0.5\n\
             QV2_2   =                  0.7\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---TPN".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--TPN".to_string()));
        assert_eq!(cards.get_float("PV2_1"), Some(0.5));
        assert_eq!(cards.get_float("PV2_2"), Some(0.7));
        assert!(cards.find("QV2_1").is_none());
    }

    #[test]
    fn translates_tpv_to_tpn() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---TPV'\n\
             CTYPE2  = 'DEC--TPV'\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---TPN".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--TPN".to_string()));
    }

    #[test]
    fn translates_zpx_to_zpn_and_extracts_projp() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---ZPX'\n\
             CTYPE2  = 'DEC--ZPX'\n\
             WAT2_001= 'projp1=1.0 projp2=-0.4'\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---ZPN-ZPX".to_string()));
        assert_eq!(cards.get_float("PV2_1"), Some(1.0));
        assert_eq!(cards.get_float("PV2_2"), Some(-0.4));
    }

    #[test]
    fn translates_tnx_to_tpn() {
        let mut cards =
            CardList::from_header("CTYPE1  = 'RA---TNX'\nCTYPE2  = 'DEC--TNX'\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---TPN".to_string()));
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--TPN".to_string()));
    }

    #[test]
    fn translates_msx_car_wraps_crpix() {
        // CRPIX so far outside [-180, 180] that the image would
        // be empty; expect CRPIX to wrap.
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---CAR'\n\
             CTYPE2  = 'DEC--CAR'\n\
             NAXIS1  =                  100\n\
             CRPIX1  =               1000.0\n\
             CDELT1  =                 -1.0\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        // CRPIX should now lie close to the image (within one full
        // wrap cycle of the original).
        let crpix1 = cards.get_float("CRPIX1").unwrap();
        assert!(crpix1.abs() < 200.0 + 50.0, "CRPIX1 not wrapped: {crpix1}");
    }

    #[test]
    fn translates_restfreq_to_restfrq_with_ghz_scale() {
        let mut cards =
            CardList::from_header("RESTFREQ=                1.42 / Rest frequency in GHz\nEND")
                .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("RESTFREQ").is_none());
        let restfrq = cards.get_float("RESTFRQ").unwrap();
        assert!((restfrq - 1.42e9).abs() < 1.0);
    }

    #[test]
    fn translates_velo_lsr_to_vrad_plus_specsys() {
        let mut cards = CardList::from_header("CTYPE3  = 'VELO-LSR'\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE3"), Some("VRAD    ".to_string()));
        assert_eq!(cards.get_string("SPECSYS"), Some("LSRK".to_string()));
    }

    #[test]
    fn translates_lambda_to_wave() {
        let mut cards = CardList::from_header("CTYPE3  = 'LAMBDA  '\nEND").unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CTYPE3"), Some("WAVE".to_string()));
    }

    #[test]
    fn translates_cunit_case_insensitively() {
        let mut cards = CardList::from_header(
            "CUNIT1  = 'HZ      '\n\
             CUNIT2  = 'M/S     '\n\
             CUNIT3  = 'KM/S    '\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert_eq!(cards.get_string("CUNIT1"), Some("Hz".to_string()));
        assert_eq!(cards.get_string("CUNIT2"), Some("m/s".to_string()));
        assert_eq!(cards.get_string("CUNIT3"), Some("km/s".to_string()));
    }

    #[test]
    fn iraf_mini_wcs_keywords_stripped() {
        let mut cards = CardList::from_header(
            "WAT1_001= 'foo'\nWAT2_001= 'bar'\nMWCS1   =                   1\nLTM1_1  =                  1.0\nLTV1    =                  0.0\nWSV1_LEN=                   3\nEND",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsPc);
        assert!(cards.find("WAT1_001").is_none());
        assert!(cards.find("WAT2_001").is_none());
        assert!(cards.find("MWCS1").is_none());
        assert!(cards.find("LTM1_1").is_none());
        assert!(cards.find("LTV1").is_none());
        assert!(cards.find("WSV1_LEN").is_none());
    }

    #[test]
    fn translates_class_spectral_crval_and_specsys() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---GLS'\n\
             CTYPE2  = 'DEC--GLS'\n\
             CTYPE3  = 'FREQ    '\n\
             CRVAL1  =                  0.0\n\
             CRVAL2  =                  0.0\n\
             CRVAL3  =                  0.0\n\
             RESTFREQ=                1.0E9 / [Hz] Rest frequency\n\
             VELO-LSR=               1.0E4\n\
             END",
        )
        .unwrap();
        spec_trans(&mut cards, Encoding::FitsClass);
        // Spectral CRVAL was 0; after CLASS shift = RESTFREQ.
        assert_eq!(cards.get_float("CRVAL3"), Some(1.0e9));
        assert_eq!(cards.get_string("SPECSYS"), Some("SOURCE".to_string()));
        assert_eq!(cards.get_string("SSYSSRC"), Some("LSRK".to_string()));
        // ZSOURCE non-zero (vsource non-zero because vref=1e4 and v0=0
        // when crval_orig == restfreq, but here crval_orig=0 so v0=c).
        assert!(cards.find("ZSOURCE").is_some());
    }

    #[test]
    fn rename_keyword_handles_float_values() {
        let mut cards =
            CardList::from_header("LONGPOLE=                180.0 / Old name\nEND").unwrap();
        assert!(rename_keyword(&mut cards, "LONGPOLE", "LONPOLE"));
        assert_eq!(cards.get_float("LONPOLE"), Some(180.0));
        assert!(cards.find("LONGPOLE").is_none());
    }
}

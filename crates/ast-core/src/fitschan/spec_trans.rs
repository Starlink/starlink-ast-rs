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
fn translate_sao_co(_cards: &mut CardList) {}
fn translate_atf_to_ait(_cards: &mut CardList) {}
fn translate_qv_to_pv(_cards: &mut CardList) {}
fn translate_tpv_and_polytan_to_tpn(_cards: &mut CardList) {}
fn translate_iraf_zpx(_cards: &mut CardList) {}
fn translate_iraf_tnx(_cards: &mut CardList) {}
fn translate_msx_car(_cards: &mut CardList) {}
fn translate_restfreq(_cards: &mut CardList) {}
fn translate_aips_spectral_ctype(_cards: &mut CardList) {}
fn translate_cunit_case(_cards: &mut CardList) {}
fn translate_class(_cards: &mut CardList) {}
fn translate_iraf_mini_wcs_strip(_cards: &mut CardList) {}

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
    fn rename_keyword_handles_float_values() {
        let mut cards =
            CardList::from_header("LONGPOLE=                180.0 / Old name\nEND").unwrap();
        assert!(rename_keyword(&mut cards, "LONGPOLE", "LONPOLE"));
        assert_eq!(cards.get_float("LONPOLE"), Some(180.0));
        assert!(cards.find("LONGPOLE").is_none());
    }
}

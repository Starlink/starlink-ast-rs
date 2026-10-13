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
use super::fits_match::match_keyword;
use super::warning::{Warning, Warnings};

/// The FitsChan attributes the translations depend on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TransAttrs {
    /// CarLin: a `-CAR` projection is linear, so its CRPIX is not wrapped.
    pub car_lin: bool,
    /// PolyTan: whether a `-TAN` projection with PV distortion terms is
    /// read as `-TPN`; negative to decide from the PV cards present.
    pub poly_tan: i32,
}

impl Default for TransAttrs {
    fn default() -> Self {
        TransAttrs {
            car_lin: false,
            poly_tan: -1,
        }
    }
}

/// Build the temporary "translations" CardList for `cards`, mirroring
/// C `SpecTrans` (fitschan.c:30931).
///
/// C creates a separate FitsChan `ret` and writes standard equivalents
/// of any non-standard keywords found in the input `this` into `ret`.
/// `WcsToStore` then reads keyword values preferring `ret` over `this`,
/// so the original input layout is preserved untouched.
///
/// We mirror that exactly: clone `cards` into the returned CardList and
/// apply the in-place translation rewrites there. Callers then read
/// keyword values from the returned list, which contains both the
/// originals and the translated overrides. The list records the keywords
/// the translations write ([`CardList::take_written_names`]), which are
/// those C's `ret` holds.
///
/// `encoding` selects which translations apply: a few are gated to
/// specific encodings (e.g. `Cnxxx` → `*A` is FITS-PC / FITS-IRAF
/// only; AIPS spectral CTYPE rewrites are AIPS-only). Translations
/// not gated by encoding apply to all FITS encodings uniformly.
///
/// `warnings` collects the text of any `ASTWARN` block a translation raises.
/// C's `Warn` calls inside `SpecTrans` all target the *source* chan `this`
/// rather than the translation chan `ret` (e.g. `GetUsedPolyTan`,
/// fitschan.c:14105, :14073), so the caller pushes these onto the original
/// card list rather than onto the returned one.
/// The warnings `SpecTrans` raises, by where C's cursor is when it raises
/// them. Those raised before the scan for `QV` cards in the primary axis
/// description (`ClassTrans` and `SAOTrans`) are written in front of the
/// card the read started at, since C's `Read` does not move the cursor
/// first; that scan leaves the cursor at end-of-file, where the rest go.
#[derive(Debug, Default)]
pub(crate) struct TransWarnings {
    /// Warnings written in front of the card the read started at.
    pub entry: Warnings,
    /// Warnings written at end-of-file.
    pub end: Warnings,
}

pub(crate) fn spec_trans(
    cards: &CardList,
    encoding: Encoding,
    attrs: TransAttrs,
    warnings: &mut TransWarnings,
) -> CardList {
    let mut trans = cards.clone();
    trans.record_writes();
    spec_trans_inplace(&mut trans, encoding, attrs, warnings);
    trans
}

/// In-place translation worker shared between [`spec_trans`] and the
/// module's tests. Most consumers should call [`spec_trans`] which
/// preserves the input list.
pub(crate) fn spec_trans_inplace(
    cards: &mut CardList,
    encoding: Encoding,
    attrs: TransAttrs,
    warnings: &mut TransWarnings,
) {
    // C `SpecTrans` never writes to the source chan `this` — every `SetValue`
    // and `DeleteCard` in its body targets the return chan `ret`. Translations
    // here rewrite `cards` in place instead, so a translation that must consult
    // the *untranslated* header (C's `GetValue( this, … )` rather than
    // `GetValue2( ret, this, … )`) needs this snapshot to read.
    let src = cards.clone();

    // Translation 1 (fitschan.c:31240-31247): RADECSYS → RADESYS.
    // FITS-PC and FITS-IRAF only.
    if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
        rename_keyword(cards, "RADECSYS", "RADESYS");
    }

    // Translation 2 (fitschan.c:31251-31257): LONGPOLE → LONPOLE.
    // FITS-PC and FITS-IRAF only.
    if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
        rename_keyword(cards, "LONGPOLE", "LONPOLE");
    }

    // Zero CDELT (fitschan.c:31261-31296): replace any zero CDELT with a
    // non-zero value so the pixel→IWC matrix stays invertible (a zero on the
    // diagonal makes the Mapping non-simplifiable and unusable by a Plot).
    // Zero CDELTs typically mark "redundant" degenerate axes (e.g. a single
    // wavelength plane of a 3D cube). Runs once per version character; no
    // encoding gate. C sits this inside the per-version loop at 31038.
    translate_zero_cdelt(cards);

    // Translations 3-4 (fitschan.c:31366-31548): the whole family of
    // PC-producing conversions — CDjjjiii, CDj_i, PCjjjiii and CROTAi —
    // under C's single `gotpcij` gate. Run once per coordinate version;
    // no encoding gate (the CDjjjiii branch alone is FITS-IRAF only).
    for s in std::iter::once(' ').chain('A'..='Z') {
        translate_pc_family(cards, s, encoding);
    }

    // Translation 5 (fitschan.c:31552-31605): PROJPi → PV<axlat>_i.
    // Primary plane only. C guards the disagreement heuristic on
    // `axlat != -1` (fitschan.c:31557) but not on encoding — the PV
    // write itself is what's gated to FITS-PC / FITS-IRAF
    // (fitschan.c:31598-31599), so run this unconditionally and let
    // the function apply that gate internally.
    translate_projp(cards, encoding);

    // Translation 6 (fitschan.c:31540-31681): C{m}{VAL/PIX/YPE/NIT/ELT}{i}
    // → CRVAL{i}{S} / CRPIX{i}{S} / CTYPE{i}{S} / CUNIT{i}{S} /
    // CDELT{i}{S} where S = 'A' + (m-1). FITS-PC / FITS-IRAF only.
    if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
        translate_cnxxx(cards);
    }

    // Translations 7, 8, 10 (fitschan.c:31683-31736): EPOCH /
    // EQUINOX normalisation. Primary plane only; no encoding gate.
    translate_epoch_equinox(cards);

    // Translation 9 (fitschan.c:31737-31758): DATE-OBS → MJD-OBS
    // when MJD-OBS is absent. Primary plane only; no encoding gate.
    translate_date_obs(cards);

    // Translation 11 (fitschan.c:31785-31878): AIPS NCP/GLS
    // projections. Per-version letter; no encoding gate.
    translate_ncp_gls(cards);

    // Translation 12 (fitschan.c:31185-31235): early paper-III
    // spectral CTYPE algorithm codes. Per-version letter, no
    // encoding gate. CTYPE values like "WAVE-FRQ" become
    // "WAVE-F2W" (the modern -X2P form).
    translate_early_paper_iii_spectral(cards);

    // Translation 13 (fitschan.c:32154-32170): SAO COi_j → PVi_m
    // distortion + CTYPE TAN→TPN, for a primary `-TAN` projection.
    if let Some((axlon, axlat)) = celestial_axes(cards, ' ')
        && cards
            .get_string(&format!("CTYPE{axlon}"))
            .and_then(|c| c.get(4..).map(str::to_string))
            .is_some_and(|prj| prj.eq_ignore_ascii_case("-TAN"))
        && translate_sao_co(cards, &mut warnings.entry)
    {
        for ax in [axlon, axlat] {
            replace_projection_suffix(cards, &format!("CTYPE{ax}"), "TPN");
        }
    }

    // Translation 14 (fitschan.c:31819-31837): CLASS "ATF" projection
    // → AIT. Per-version letter, no encoding gate.
    translate_atf_to_ait(cards);

    // Translation 15 (fitschan.c:31877-31523): QV<i>_<m> → PV<i>_<m>.
    // If any QV is found and the projection is TAN, rename TAN to
    // TPN as well. Per-version letter, no encoding gate.
    translate_qv_to_pv(cards);

    // Translation 16 (fitschan.c:31528-31550): TPV → TPN; TAN with
    // PolyTan attribute non-zero → TPN. Per-version letter, no
    // encoding gate. (PolyTan is an AST attribute, defaulting to
    // false; we mirror by treating it as off.)
    translate_tpv_and_polytan_to_tpn(cards, &src, attrs.poly_tan, &mut warnings.end);

    // Translation 17 (fitschan.c:31547-31641): IRAF ZPX projection.
    // Replace -ZPX with -ZPN-ZPX (ZPN with ZPX distortion code) and
    // parse PROJP coefficients out of the WAT strings on the
    // celestial axes. Per-version letter, no encoding gate.
    translate_iraf_zpx(cards);

    // Translation 18 (fitschan.c:31591-31673): IRAF TNX projection.
    // Replace -TNX with -TPN and parse polynomial coefficients out
    // of the WAT strings; if not exactly representable in FITS-WCS
    // an `ASTWARN` warning is issued. Per-version letter, no
    // encoding gate.
    translate_iraf_tnx(cards, &mut warnings.end);

    // Translation 19 (fitschan.c:31644-31674): MSX CAR projection.
    // Some MSX images have CAR projections whose CRPIX puts the
    // valid native longitude range entirely outside the image; C
    // shifts CRPIX1 by ±360°/CDELT1 to bring it back in range.
    // Suppressed when CarLin is set, because a linear CAR reading has no
    // restricted native longitude range to bring the array into.
    // Per-version letter, no encoding gate.
    if !attrs.car_lin {
        translate_msx_car(cards);
    }

    // Translation 20 (fitschan.c:31676-31717): RESTFREQ → RESTFRQ
    // with MHz/GHz unit detection from the comment. Per-version
    // letter, no encoding gate.
    translate_restfreq(cards);

    // Translation 21 (fitschan.c:31719-31750): AIPS spectral CTYPE
    // (e.g. VELO-LSR → VRAD with SPECSYS=LSRK) and
    // LAMBDA → WAVE. Per-version letter, no encoding gate.
    translate_aips_spectral_ctype(cards);

    // Translation 22 (fitschan.c:31752-31784): CUNIT case-insensitive
    // canonicalisation: "Hz", "Angstrom", "km/s", "m/s" → canonical
    // case. Per-version letter, no encoding gate.
    translate_cunit_case(cards);

    // Translation 23 (fitschan.c:31359-31431): CLASS-specific
    // rewrites. Mirrors the `ClassTrans` helper called inside the
    // per-version loop; only fires for primary plane + FITS-CLASS
    // encoding.
    if encoding == Encoding::FitsClass {
        translate_class(cards, &mut warnings.entry);
    }

    // Translation 24 (fitschan.c:31789-31813): IRAF mini-WCS
    // keyword strip (WAT*, MWCS*, WSV*, LTM*, LTV*). Outside the
    // per-version loop in C; runs once. No encoding gate.
    translate_iraf_mini_wcs_strip(cards);
}

/// Zero-CDELT helper: replace any zero `CDELT{i}{S}` with a non-zero value.
/// Mirrors fitschan.c:31261-31296, run once per coordinate version.
///
/// A zero on the CDELT diagonal would make the pixel→IWC matrix singular, so
/// the resulting Mapping could not be simplified or used by a Plot. Zero
/// CDELTs conventionally mark "redundant" degenerate axes (a single-pixel
/// plane), so the exact replacement value does not matter.
///
/// The C source re-reads the (zero) CDELT into `dval` when computing
/// `1.0E-6*dval` — the prologue comment says "1.0E-6 of CRVAL" but the code
/// uses CDELT, so the product is always zero and the `1.0` fallback always
/// wins. This faithfully reproduces that quirk rather than the comment.
fn translate_zero_cdelt(cards: &mut CardList) {
    // Primary (' ') plus alternate versions 'A'..='Z' (C: s = 'A'-1 .. 'Z',
    // with 'A'-1 mapped to ' ').
    for s in std::iter::once(' ').chain('A'..='Z') {
        let suffix = if s == ' ' {
            String::new()
        } else {
            s.to_string()
        };
        // C uses astKeyFields to find the highest CDELT index for this
        // version; pass on if there are none.
        let mut naxis = 0;
        for j in 1..=9 {
            if cards.find(&format!("CDELT{j}{suffix}")).is_some() {
                naxis = j;
            }
        }
        if naxis == 0 {
            continue;
        }
        for j in 1..=naxis {
            let key = format!("CDELT{j}{suffix}");
            let Some(cdeltj) = cards.get_float(&key) else {
                continue;
            };
            if cdeltj == 0.0 {
                // C: cdeltj = 1.0E-6*dval (dval = the zero CDELT) → 0.0;
                // then `if( cdeltj == 0.0 ) cdeltj = 1.0`.
                cards.set_float(&key, 1.0, "");
            }
        }
    }
}

/// Test whether a CTYPE prefix denotes a celestial axis (longitude
/// or latitude) per the FITS-WCS conventions C `SpecTrans` walks
/// (fitschan.c:31143-31163). Returns `Some((true, _))` for
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
/// C `SpecTrans`'s per-version loop (fitschan.c:31143-31163).
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
/// Mirrors fitschan.c:31185-31235. Per-version letter, no encoding gate.
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
/// C's `SAOTrans` (fitschan.c:26983): translate the 13 polynomial
/// coefficients per axis (`CO1_1`..`CO2_13`) by which SAO's platepos.c
/// describes a distorted TAN projection into `PVi_m` values, and replace the
/// CD, PC, CDELT and CROTA values, which the polynomial includes, by unit
/// values. Returns whether the header was such a one, which needs exactly
/// two CTYPE keywords and the first three coefficients on each axis; one
/// with every coefficient but the constant zero on an axis is warned of and
/// not translated. The coefficients read are consumed either way.
fn translate_sao_co(cards: &mut CardList, warnings: &mut Warnings) -> bool {
    const NC: usize = 13;
    if cards.key_fields("CTYPE%d", 0).0 != 2 {
        return false;
    }
    let mut co = [[0.0_f64; NC]; 2];
    let mut ok = true;
    let mut is_sao = true;
    for (i, row) in co.iter_mut().enumerate() {
        if !(ok && is_sao) {
            break;
        }
        ok = false;
        for (m, slot) in row.iter_mut().enumerate() {
            match cards.get_float_marked(&format!("CO{}_{}", i + 1, m + 1)) {
                Some(v) => {
                    *slot = v;
                    if v != 0.0 && m > 0 {
                        ok = true;
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
    }
    if !is_sao {
        return false;
    }
    if !ok {
        warnings.push(Warning::new(
            "badpv",
            "This FITS header describes an SAO encoded distorted TAN projection, but all \
             the distortion coefficients for at least one axis are zero.",
        ));
        return false;
    }

    // Each PV value is a sum of the coefficients listed, as (axis, index),
    // 0-based (fitschan.c:27093-27198).
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

    // The SAO polynomial subsumes the CD/PC/CDELT/CROTA scaling, so
    // C marks all of them as used and replaces with identity values
    // (mirrors C:26845-26880).
    for key in [
        "CD1_1", "CD1_2", "CD2_1", "CD2_2", "PC1_1", "PC1_2", "PC2_1", "PC2_2", "CDELT1", "CDELT2",
        "CROTA1", "CROTA2",
    ] {
        cards.delete_consumed(key);
    }
    cards.set_float("PC1_1", 1.0, "PC matrix element");
    cards.set_float("PC2_2", 1.0, "PC matrix element");
    cards.set_float("PC1_2", 0.0, "PC matrix element");
    cards.set_float("PC2_1", 0.0, "PC matrix element");
    cards.set_float("CDELT1", 1.0, "Pixel scale");
    cards.set_float("CDELT2", 1.0, "Pixel scale");

    true
}
/// Translation #14 helper: CLASS "ATF" projection → AIT.
///
/// Mirrors fitschan.c:31819-31837. Per-version letter; no encoding gate.
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
/// rewritten to `-TPN` (mirrors fitschan.c:31877-31523).
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
                cards.delete_consumed(&from);
                continue;
            }
            if let Some(value) = cards.get_float(&from) {
                let comment = cards
                    .find(&from)
                    .and_then(|c| c.comment.clone())
                    .unwrap_or_default();
                cards.delete_consumed(&from);
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

/// Resolve the auto (`PolyTan = -1`) decision from the PV cards, mirroring
/// C `GetUsedPolyTan` (fitschan.c:13963-14146).
///
/// Returns `true` when the distorted-TAN (TPN) convention applies: a PVi_m
/// present on the latitude axis, or a PVi_m with `m > 4` on the longitude
/// axis.  Returns `false` when no qualifying PV cards are found, or when all
/// distortion coefficients (`m >= 1`) on the deciding axis are zero — in that
/// case the zero-valued PV cards are deleted to prevent later mis-use
/// (fitschan.c:14110-14122, 14058-14073).
///
/// The `PolyTan` AST attribute (positive/zero explicit values) is not exposed
/// in this port, so only the auto (`ret < 0` → `ret = 0`, then promote)
/// branch exists.  The function mirrors C's sequential both-axes guard: if the
/// latitude axis passes, the longitude axis is checked next; if either is
/// all-zero, the function demotes to standard TAN and deletes that axis's PV
/// cards.
///
/// Every PV read here comes from `src`, the untranslated header: C's
/// `astKeyFields` and `GetValue` calls (fitschan.c:14052, :14020) take `this`,
/// not the `GetValue2( ret, this, … )` pair the surrounding translations use,
/// so a PV card an earlier translation synthesised (e.g. `PROJPm` →
/// `PV<axlat>_m`, fitschan.c:31598) is invisible to the decision. The
/// deletions still target `cards`, matching C's `DeleteCard( this, … )` in a
/// port where the translations rewrite the header in place.
fn get_used_poly_tan(
    cards: &mut CardList,
    src: &CardList,
    poly_tan: i32,
    axlon: usize,
    axlat: usize,
    s: char,
    warnings: &mut Warnings,
) -> bool {
    // A PolyTan that is set decides on its own.
    if poly_tan >= 0 {
        return poly_tan != 0;
    }

    // Build the PVi_m keyword name for axis ax, order m, version s.
    let pv_name = |ax: usize, m: usize| -> String {
        if s == ' ' {
            format!("PV{ax}_{m}")
        } else {
            format!("PV{ax}_{m}{s}")
        }
    };

    // C's `astKeyFields` on "PV<ax>_%d": whether any such card is present,
    // and the largest m.
    let find_ubnd = |ax: usize| -> Option<usize> {
        let template = if s == ' ' {
            format!("PV{ax}_%d")
        } else {
            format!("PV{ax}_%d{s}")
        };
        let (n, _, hi) = src.key_fields(&template, 1);
        (n > 0).then(|| hi[0].max(0) as usize)
    };

    let mut ret: i32 = 0;

    let ubnd_lat = find_ubnd(axlat);
    let ubnd_lon = find_ubnd(axlon);
    let nfound1 = ubnd_lat.is_some();
    let nfound2 = ubnd_lon.is_some();

    // C: if any lat PV present, or lon PV present with m > 4, use TPN.
    if nfound1 || (nfound2 && ubnd_lon.is_some_and(|u| u > 4)) {
        ret = 1;
    }

    // C: if ret > 0, guard that at least one m>=1 coefficient is non-zero.
    if ret > 0 {
        // Latitude axis first (fitschan.c:14077-14122).
        let mut ok = false;
        let ulat = ubnd_lat.unwrap_or(0);
        let mut m = 1usize;
        while m <= ulat && !ok {
            let pval = src
                .get_float(&pv_name(axlat, m))
                .unwrap_or(if m == 1 { 1.0 } else { 0.0 });
            if pval != 0.0 {
                ok = true;
            }
            m += 1;
        }

        if !ok {
            warnings.push(Warning::new(
                "badpv",
                "This FITS header describes a distorted TAN projection, but \
                 all the distortion coefficients (the PVi_m headers) on the \
                 latitude axis are zero.",
            ));
            ret = 0;
            // Delete all PVlat_m (m = 1..=ubnd_lat) to prevent mis-use.
            for m in 1..=ulat {
                cards.delete(&pv_name(axlat, m));
            }
        } else {
            // Latitude OK; now check longitude (fitschan.c:14124-14146).
            ok = false;
            let ulon = ubnd_lon.unwrap_or(0);
            let mut m = 1usize;
            while m <= ulon && !ok {
                let pval =
                    src.get_float(&pv_name(axlon, m))
                        .unwrap_or(if m == 1 { 1.0 } else { 0.0 });
                if pval != 0.0 {
                    ok = true;
                }
                m += 1;
            }
            if !ok {
                warnings.push(Warning::new(
                    "badpv",
                    "This FITS header describes a distorted TAN projection, but \
                     all the distortion coefficients (the PVi_m headers) on the \
                     longitude axis are zero.",
                ));
                ret = 0;
                // Delete all PVlon_m (m = 1..=ubnd_lon) to prevent mis-use.
                for m in 1..=ulon {
                    cards.delete(&pv_name(axlon, m));
                }
            }
        }
    }

    ret == 1
}

/// Translation #16 helper: TPV → TPN, and TAN+PolyTan → TPN.
///
/// Mirrors fitschan.c:31528-31550. For the `-TPV` rename, the CTYPE suffix
/// is changed directly. For the TAN case, the C default `PolyTan = -1`
/// (auto-detect) is resolved by `get_used_poly_tan`: if PVi_m distortion
/// cards are present on the latitude axis (or on the longitude axis with
/// `m > 4`), the projection is promoted from TAN to TPN.
///
/// Per-version letter; no encoding gate. `src` is the untranslated header,
/// which is what `get_used_poly_tan` reads its PV values from.
fn translate_tpv_and_polytan_to_tpn(
    cards: &mut CardList,
    src: &CardList,
    poly_tan: i32,
    warnings: &mut Warnings,
) {
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
        let should_rename =
            if projection_suffix(ctype.trim()).is_some_and(|p| p.eq_ignore_ascii_case("TPV")) {
                // Explicit TPV distortion suffix: always promote to TPN.
                true
            } else {
                // Standard TAN: promote to TPN only when PVi_m distortion
                // cards are present and non-zero (PolyTan = -1 auto-detect,
                // fitschan.c:13963).
                projection_suffix(ctype.trim()).is_some_and(|p| p.eq_ignore_ascii_case("TAN"))
                    && get_used_poly_tan(cards, src, poly_tan, axlon, axlat, s, warnings)
            };
        if should_rename {
            replace_projection_suffix(cards, &key("CTYPE", axlon), "TPN");
            replace_projection_suffix(cards, &key("CTYPE", axlat), "TPN");
        }
    }
}
/// Translation #17 helper: IRAF -ZPX projections.
///
/// Replace `-ZPX` with `-ZPN-ZPX` (ZPN projection with ZPX
/// distortion code) on both celestial CTYPEs, then look at the
/// `WAT1_*` and `WAT2_*` strings on each axis for `projp{n}=value`
/// terms; each one becomes `PV<axlat>_n`.
///
/// Mirrors fitschan.c:31547-31641. Primary plane only; no encoding
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
/// strings into `PV<axis>_m` cards (mirrors fitschan.c:31591-31641).
///
/// When the polynomial cannot be represented as a TPN (i.e. order
/// greater than 7, asymmetric axis orders, or full cross-terms) the
/// partially-translated CTYPE is left in place and a `tnx` warning is
/// raised, as C does (fitschan.c:32099-32103).
fn translate_iraf_tnx(cards: &mut CardList, warnings: &mut Warnings) {
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
    // Mirrors fitschan.c:32074-32109.
    for (i, axis) in [(0_usize, axlon), (1_usize, axlat)].iter().copied() {
        let watmem = concat_wat(cards, axis);
        let (pvs, ok) = wat_coeffs(&watmem, i);
        if ok {
            for (m, value) in pvs {
                cards.set_float(&format!("PV{axis}_{m}"), value, "TAN projection parameter");
            }
        } else {
            warnings.push(Warning::new("tnx", TNX_WARNING));
        }
    }
}

/// The `tnx` warning text (fitschan.c:32100-32103). `tnx` is one of the
/// conditions enabled by the default `Warnings` value (fitschan.c:43423), so
/// it is always emitted.
const TNX_WARNING: &str = "This FITS header includes, or was derived from, a TNX projection which \
     requires unsupported IRAF-specific corrections. The WCS information may \
     therefore be incorrect.";

/// Port of fitschan.c::WATCoeffs (line 35102-35355). Extract
/// polynomial coefficients from the IRAF WAT-string `lngcor` /
/// `latcor` block and convert them into a list of (m, value) pairs
/// suitable for FITS-WCS `PVi_m` keys.
///
/// `iaxis` is 0 for longitude, 1 for latitude.
///
/// Returns the coefficient list and C's `*ok` flag. The two outputs are
/// independent: an empty list with `ok` still set means the WAT string
/// carries no correction surface, which callers treat as "no distortion",
/// whereas `ok == false` means a surface was found but uses features AST
/// cannot express as a TPN projection.
pub(crate) fn wat_coeffs(watstr: &str, iaxis: usize) -> (Vec<(usize, f64)>, bool) {
    // C looks for the regex `cor *= *"(.*)"` (fitschan.c:35368). When it does
    // not match, `WATCoeffs` returns zero coefficients with `*ok` left set.
    let Some(body) = correction_surface(watstr) else {
        return (Vec::new(), true);
    };

    // Split into whitespace-delimited words and convert each to a double.
    // C reports AST__BDFTS and abandons the read when `astChr2Double` fails
    // (fitschan.c:35389-35394); this reader has no error channel at the point
    // of use, so an unreadable word is reported as "not representable".
    let words: Vec<&str> = body.split_ascii_whitespace().collect();
    let mut parsed: Vec<f64> = Vec::with_capacity(words.len());
    for w in &words {
        match w.parse::<f64>() {
            Ok(v) if v.is_finite() => parsed.push(v),
            _ => return (Vec::new(), false),
        }
    }
    let nword = parsed.len();

    // Number of lngcor/latcor values needed for each order, indexed by
    // porder = order - 1.
    const NAB: [usize; 8] = [1, 3, 6, 10, 15, 21, 28, 36];

    let mut ok = true;
    let mut cheb = false;
    let mut order: i64 = 0;
    let mut porder: i64 = -1;
    let mut coeff: Vec<f64> = Vec::new();
    let mut ximin = 0.0;
    let mut ximax = 0.0;
    let mut etamin = 0.0;
    let mut etamax = 0.0;
    let mut xiorder: i64 = 0;
    let mut etaorder: i64 = 0;

    for (iword, &dval) in parsed.iter().enumerate() {
        if !ok {
            break;
        }
        match iword {
            // Surface type. Only type 2 (legendre) is rejected outright; C
            // recognises type 1 as chebyshev and treats everything else as
            // the simple-polynomial form (fitschan.c:35399-35404).
            0 => {
                if dval == 1.0 {
                    cheb = true;
                } else if dval == 2.0 {
                    ok = false;
                }
            }
            // Order of the polynomial in xi.
            1 => {
                order = dval as i64;
                porder = order - 1;
            }
            // Order in eta; must match the xi order and lie in 0..=7.
            2 => {
                if dval as i64 - 1 != porder || !(0.0..=7.0).contains(&dval) {
                    ok = false;
                }
            }
            // Cross-term type; only 2 (half-cross terms) is supported. The
            // coefficient array is sized here, holding the terms as if full
            // cross terms had been supplied.
            3 => {
                if dval != 2.0 {
                    ok = false;
                } else if order > 0 {
                    coeff = vec![0.0_f64; (order * order) as usize];
                }
            }
            // Region of validity in IRAF's xi/eta space, used only for a
            // chebyshev surface.
            4 => ximin = dval,
            5 => ximax = dval,
            6 => etamin = dval,
            7 => etamax = dval,
            // The polynomial coefficients, stored so that the power of xi
            // increases fastest. Half-cross terms mean the maximum power of
            // xi decreases from `order` to zero through the list.
            _ => {
                if order > 0 && etaorder < order {
                    coeff[(xiorder + order * etaorder) as usize] = dval;
                    xiorder += 1;
                    if xiorder == order - etaorder {
                        xiorder = 0;
                        etaorder += 1;
                    }
                }
            }
        }
    }

    // Check that all the required coefficients were found. The `porder`
    // range test is not in C, which indexes `nab[porder]` unconditionally;
    // it is reachable when the word list stops before the order check at
    // word 2, so the bound is needed to keep the lookup in range.
    if !(0..8).contains(&porder) || nword != 8 + NAB[porder as usize] {
        ok = false;
    }
    if !ok || coeff.len() < 2 {
        // C adds 1.0 to `coeff[iaxis ? order : 1]` without a bounds check, so
        // a first-order (constant-only) surface indexes past the end of its
        // 1x1 array. Reject it rather than reproducing the overrun.
        return (Vec::new(), false);
    }
    let order = order as usize;

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
    (out, true)
}

/// Extract the body of the `cor = "..."` assignment in an IRAF WAT string,
/// mirroring C's `astChrSplitRE( watstr, "cor *= *\"(.*)\"", ... )`
/// (fitschan.c:35368). `.*` is greedy, so the body runs to the *last* quote
/// in the string. Returns `None` when there is no correction surface.
fn correction_surface(watstr: &str) -> Option<&str> {
    let cor = watstr.find("cor")?;
    let rest = &watstr[cor + 3..];
    let mut it = rest.char_indices().skip_while(|&(_, c)| c == ' ');
    let (eq, '=') = it.next()? else {
        return None;
    };
    let after_eq = &rest[eq + 1..];
    let open = after_eq.find(|c| c != ' ')?;
    if after_eq.as_bytes()[open] != b'"' {
        return None;
    }
    let body = &after_eq[open + 1..];
    let close = body.rfind('"')?;
    Some(&body[..close])
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
/// axis. Mirrors C `ConcatWAT` (fitschan.c:8070-8161) — IRAF emits
/// the WAT keys with 3-digit zero-padded indices.
pub(crate) fn concat_wat(cards: &CardList, axis: usize) -> String {
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
/// [-180°, 180°]. Mirrors fitschan.c:32047-32075. Primary plane only;
/// no encoding gate. The caller applies C's `!astGetCarLin( this )` gate.
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
    // C `SpecTrans` converts a CD matrix to PC + CDELT (CDELT = row-vector
    // length, fitschan.c:31434) before this block, so its longitude scale
    // comes from CDELT even for a CDi_j-only header. The Rust store keeps the
    // CD matrix directly, so derive the effective longitude CDELT the same way
    // when the CDELT keyword is absent.
    let Some(cdelti) = cards
        .get_float(&format!("CDELT{axlon}"))
        .or_else(|| cd_row_cdelt(cards, axlon))
    else {
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

/// Effective CDELT for a matrix row derived from CD keywords, matching C's
/// CD→PC/CDELT conversion (fitschan.c:31420-31434): the length of the row
/// vector, signed by the diagonal element. Returns `None` when the header
/// carries no `CD{row}_{i}` keywords, so the caller falls through as it would
/// for a header with neither CDELT nor CD.
fn cd_row_cdelt(cards: &CardList, row: usize) -> Option<f64> {
    let naxis = cards.get_int("NAXIS").unwrap_or(0).max(0) as usize;
    let mut rowsum2 = 0.0;
    let mut diag = 1.0;
    let mut found = false;
    for i in 1..=naxis {
        if let Some(v) = cards.get_float(&format!("CD{row}_{i}")) {
            found = true;
            rowsum2 += v * v;
            if i == row {
                diag = if v >= 0.0 { 1.0 } else { -1.0 };
            }
        }
    }
    found.then(|| rowsum2.sqrt() * diag)
}

/// Translation #20 helper: RESTFREQ → RESTFRQ with MHz/GHz unit
/// detection from the comment string. Mirrors fitschan.c:32078-32111.
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
        cards.delete_consumed(&from);
        if cards.find(&to).is_none() {
            cards.set_float(&to, scaled, &new_comment);
        }
    }
}

/// Translation #21 helper: AIPS spectral CTYPE → modern FITS-WCS
/// equivalents (`VELO-LSR` → `VRAD` + SPECSYS=LSRK etc.) plus
/// `LAMBDA` → `WAVE`.
///
/// Mirrors fitschan.c:32124-32149 + the `IsAIPSSpectral` helper at
/// fitschan.c:20210-20306. Per-version letter; no encoding gate.
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
            {
                if let Some((wctype, wspecsys)) = super::guess_encoding::is_aips_spectral(&ctype) {
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

/// Translation #22 helper: case-insensitive CUNIT canonicalisation.
///
/// Maps "Hz" / "ANGSTROM" / "Km/S" etc. variants to the canonical
/// case. Mirrors fitschan.c:32152-32184. Per-version letter; no
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
/// Mirrors fitschan.c::ClassTrans (line 6990-7163):
///
///  1. If the spectral axis (the one that is neither lon nor lat) has a
///     `FREQ` CTYPE, shift its CRVAL by RESTFREQ — CLASS stores the spectral
///     CRVAL as an offset from the rest frequency — and set SPECSYS to
///     `SOURCE`. Both live in the same branch, so a header whose spectral axis
///     index does not resolve to a CTYPE card gets neither.
///  2. If CTYPE for the celestial pair has an empty projection
///     suffix, default to `-GLS` (which translation #11 then maps to
///     `-SFL`). C runs ClassTrans from inside the SpecTrans loop
///     before the GLS rewrite, so the rewrite chain works.
///  3. Unconditionally pick a velocity reference from VELO-HEL / VELO-OBS /
///     VELO-TOP / VELO-EAR / VELO-GEO / VELO-LSR / VLSR and emit SSYSSRC and
///     ZSOURCE from it. This block runs even when the header has no celestial
///     axes and step 1 found no spectral CTYPE.
///
/// FitsClass-only; primary plane only.
fn translate_class(cards: &mut CardList, warnings: &mut Warnings) {
    // Axis indices are 0-based here, as in C, and are -1 when the
    // corresponding axis is absent: C's `SpecTrans` sets `axlon = axlat = -1`
    // before each coordinate version (fitschan.c:31213-31214) and calls
    // `ClassTrans` unconditionally for the primary plane (fitschan.c:31829),
    // so a header with no celestial axes still runs this translation. The
    // `+ 1` at each keyword lookup mirrors C's `FormatKey( "CTYPE",
    // axlon + 1, ... )`, which yields an unmatchable `CTYPE0` for an absent
    // axis and so makes the corresponding block a no-op.
    let (axlon, axlat) = match celestial_axes(cards, ' ') {
        Some((lon, lat)) => (lon as i32 - 1, lat as i32 - 1),
        None => (-1, -1),
    };
    // Spectral axis = the one that is neither lon nor lat. Mirrors
    // fitschan.c:7131-7137; the final `else` is a catch-all, reached both for
    // the {0,2} celestial pair and for a header with no celestial axes at all.
    let axspec: i32 = if axlat + axlon == 1 {
        2
    } else if axlat + axlon == 3 {
        0
    } else {
        1
    };

    // Get RESTFREQ. Try the standard then the AIPS-spelling. Every read in this
    // function goes through C's `GetValue2`, which marks the source card as used
    // (fitschan.c:18433-18435), so the `_marked` getters are the faithful
    // analogue: the cards these reads consume are excluded from the output.
    let restfreq = cards
        .get_float_marked("RESTFRQ")
        .or_else(|| cards.get_float_marked("RESTFREQ"));
    let Some(restfreq) = restfreq else {
        // C errors out; we silently skip. Downstream readers will
        // produce a clearer error.
        return;
    };

    // Reference-channel frequency, used below to derive the source velocity.
    //
    // C declares `crval` without an initialiser (fitschan.c:7109) and assigns
    // it only inside the spectral-CTYPE branch below. When that branch does not
    // run — no `CTYPE<axspec+1>` card, as for a 1-axis CLASS header whose only
    // axis lands outside `axspec` — C reads the uninitialised stack slot at
    // fitschan.c:7225. That is undefined behaviour in C, confirmed under
    // valgrind ("uninitialised value ... created by a stack allocation in
    // ClassTrans", flowing into SetValue for ZSOURCE). In the reference build
    // the slot reads as zero, which is the value the committed `class-spec` and
    // `class-hel` expectations encode, so zero is what this port uses. Unlike
    // C's, this read is defined.
    let mut crval = 0.0_f64;

    // Spectral CTYPE check + CRVAL shift: CLASS stores the spectral CRVAL as an
    // offset from the rest frequency. C increments `crval` in place
    // (fitschan.c:7157) and reuses that incremented value for the
    // reference-channel radio velocity `v0` (fitschan.c:7225).
    if let Some(ctype_spec) = cards.get_string_marked(&format!("CTYPE{}", axspec + 1)) {
        if ctype_spec.trim().eq_ignore_ascii_case("FREQ") {
            let key = format!("CRVAL{}", axspec + 1);
            if let Some(orig) = cards.get_float_marked(&key) {
                crval = orig + restfreq;
                let comment = cards
                    .find(&key)
                    .and_then(|c| c.comment.clone())
                    .unwrap_or_default();
                cards.set_float(&key, crval, &comment);
            }
        }
        // C errors out for non-FREQ axes; we silently skip.

        // SPECSYS = SOURCE. CLASS frequency axes describe source-frame
        // frequencies. C sets this inside the same CTYPE branch
        // (fitschan.c:7164-7166), so a header without that CTYPE keeps
        // whatever SPECSYS it already carried.
        cards.set_string("SPECSYS", "SOURCE", "Standard of rest");
    }

    // Mirrors C ClassTrans (fitschan.c:7224-7246): a celestial CTYPE whose
    // projection code is blank, such as 'RA--    ', defaults to "-GLS",
    // which SpecTrans translation #11 later remaps to "-SFL". C tests for
    // four spaces after the first four characters of the value as stored,
    // trailing spaces included. The coordinate name is padded to 4 chars
    // before "-GLS" is appended, matching C's strncpy(newtype, cval, 4).
    for ax in [axlon, axlat] {
        let key = format!("CTYPE{}", ax + 1);
        if let Some(ctype) = cards.get_string_marked(&key)
            && ctype.len() > 4
            && ctype.as_bytes().get(4..8) == Some(b"    ".as_slice())
        {
            let coord = format!("{:<4}", &ctype[..4]);
            let new_ctype = format!("{coord}-GLS");
            let comment = cards
                .find(&key)
                .and_then(|c| c.comment.clone())
                .unwrap_or_default();
            cards.set_string(&key, &new_ctype, &comment);
        }
    }

    // Look for velocity-reference keywords. The standard of rest is determined
    // by which keyword name matched, so the variants are tried in C's order
    // (fitschan.c:7192-7218) and each `or_else` mirrors C's short-circuiting
    // `||`: the second variant is only read when the first is absent, so only
    // the keyword actually used is consumed.
    let (ssyssrc, vref) = if let Some(v) = cards.get_float_marked("VELO-HEL") {
        ("BARYCENT", v)
    } else if let Some(v) = cards
        .get_float_marked("VELO-OBS")
        .or_else(|| cards.get_float_marked("VELO-TOP"))
    {
        ("TOPOCENT", v)
    } else if let Some(v) = cards
        .get_float_marked("VELO-EAR")
        .or_else(|| cards.get_float_marked("VELO-GEO"))
    {
        ("GEOCENTR", v)
    } else if let Some(v) = cards
        .get_float_marked("VELO-LSR")
        .or_else(|| cards.get_float_marked("VLSR"))
    {
        ("LSRK", v)
    } else {
        warnings.push(Warning::new(
            "badval",
            "A FITS-CLASS header was identified (DELTAV and VLSR/VELO-xxx present) but no \
             usable velocity reference keyword was found. Assuming LSRK.",
        ));
        ("LSRK", 0.0)
    };

    // Radio velocity at the reference channel, in the source rest frame:
    //   v0 = c * (restfreq - crval) / restfreq
    // Mirrors fitschan.c:7225.
    const C_LIGHT_M_S: f64 = 299_792_458.0;
    let v0 = C_LIGHT_M_S * (restfreq - crval) / restfreq;
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
/// Mirrors fitschan.c:32187-32213. No encoding gate; runs once.
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
        cards.delete_consumed(&kw);
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
/// Mirrors fitschan.c:31785-31878. Per-version letter; no encoding gate.
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

/// Translation #9 helper: DATE-OBS → MJD-OBS (fitschan.c:31927-31949).
///
/// DATE-OBS is read unconditionally, and only then is MJD-OBS consulted: a
/// header carrying both keeps its MJD-OBS, but its DATE-OBS is consumed either
/// way. MJD-OBS is read here purely as a test for its presence, so it is *not*
/// consumed — `WcsFcRead`'s own MJD-OBS branch deliberately leaves the card
/// unmarked so it survives a read, and whether it does must not depend on
/// whether DATE-OBS happens to sit alongside it.
///
/// The DATE-OBS card is left in the working list rather than deleted; MJD-OBS
/// takes priority in `read_time_keywords`, so the translated value is the one
/// that reaches the store.
fn translate_date_obs(cards: &mut CardList) {
    let Some(date) = cards.get_string_marked("DATE-OBS") else {
        return;
    };
    if cards.get_float("MJD-OBS").is_some() {
        return;
    }
    let Some(mjd) = crate::fitschan::wcs_write::date_str_to_mjd(&date) else {
        return;
    };
    cards.set_float("MJD-OBS", mjd, "Date of observation");
}

/// Translations 7, 8, 10 (fitschan.c:31683-31736). Three steps that
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
        cards.delete_consumed("EPOCH");
        let value = if epoch == 0.0 { 1950.0 } else { epoch };
        if cards.find("EQUINOX").is_none() {
            cards.set_float("EQUINOX", value, &comment);
        }
        // Else: EQUINOX wins; EPOCH already consumed by delete above.
    }

    // Step 2: String EQUINOX 'B...' or 'J...' → numeric Julian. C reads
    // EQUINOX with `GetValue2`, which consumes the source cards.
    if let Some(s) = cards.get_string_marked("EQUINOX") {
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
    if let Some(0.0) = cards.get_float_marked("EQUINOX") {
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
/// Mirrors fitschan.c:31540-31681.
///
/// The CmELTi → CDELTi{S} branch additionally backfills zero
/// CDELT values from the corresponding CRVAL (× 0.01), or 1.0 when
/// CRVAL is also zero — to match C's "redundant axis" workaround
/// (fitschan.c:31646-31651).
///
/// The CmELTi branch in C also propagates a per-version PC matrix
/// from any 6-digit `PC{jjj}{iii}` cards present (fitschan.c:31659-
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
            cards.delete_consumed(&cdelt_key);
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
                // Absent elements take the unit-matrix default. Only the
                // elements actually read are consumed, so a header whose
                // 6-digit matrix is never reached keeps its cards.
                let key = format!("PC{j:03}{i:03}");
                let dval = match cards.get_float(&key) {
                    Some(v) => {
                        cards.delete_consumed(&key);
                        v
                    }
                    None if i == j => 1.0,
                    None => 0.0,
                };
                cards.set_float(&format!("PC{j}_{i}{alt}"), dval, "PC matrix element");
            }
        }
    }
}

/// Translation #5 helper: PROJPi → PV<axlat>_i. Mirrors
/// fitschan.c:31489-31605. Primary plane only.
fn translate_projp(cards: &mut CardList, encoding: Encoding) {
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
    // disagreement-detection at fitschan.c:31493-31522: if any PV
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
        // C's PROJP block (fitschan.c:31552-31605) is only entered at
        // all when `use_projp` is true; when it's false the PROJP
        // cards are simply left alone.
        return;
    }
    for i in projp_indices {
        let from = format!("PROJP{i}");
        // C reads PROJP with `GetValue2(ret, this, ...)`
        // (fitschan.c:31596-31597), which marks the card as used
        // only when the value comes from `this` (the source
        // FitsChan) — see the GetValue2 contract at
        // fitschan.c:18433-18435. `ret` never holds a PROJP value at
        // this point, so every hit here is a `this` hit and gets
        // marked, but the card itself is never deleted.
        let Some(value) = cards.get_float_marked(&from) else {
            continue;
        };
        let comment = cards
            .find(&from)
            .and_then(|c| c.comment.clone())
            .unwrap_or_default();
        // C writes the PV equivalent only for the two encodings that
        // read PROJP in the first place (fitschan.c:31598-31599).
        if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
            cards.set_float(&format!("PV{axlat}_{i}"), value, &comment);
        }
    }
}

/// Count the cards whose keyword matches `template`, and return the highest
/// value taken by each of the template's integer (`%d`) fields.
///
/// Port of C `astKeyFields` (fitschan.c:14450). The whole card list is
/// searched irrespective of the current-card position; `ubnd[i]` is the maximum
/// value seen for the i'th `%d` field, or `-1` when nothing matched. C also
/// returns lower bounds, which no `SpecTrans` caller reads.
fn key_fields(cards: &CardList, template: &str, nfld: usize) -> (usize, Vec<i32>) {
    let mut nmatch = 0;
    let mut ubnd = vec![-1i32; nfld];
    for card in cards.iter() {
        if let Some(fields) = match_keyword(&card.keyword, template, nfld) {
            nmatch += 1;
            for (u, f) in ubnd.iter_mut().zip(fields) {
                *u = (*u).max(f);
            }
        }
    }
    (nmatch, ubnd)
}

/// Version-suffixed form of a template or keyword root: unchanged for the
/// primary plane (`s == ' '`), otherwise with `s` appended.
fn versioned(root: &str, s: char) -> String {
    if s == ' ' {
        root.to_string()
    } else {
        format!("{root}{s}")
    }
}

/// The version character as a keyword suffix: empty for the primary plane.
fn suffix(s: char) -> String {
    if s == ' ' {
        String::new()
    } else {
        s.to_string()
    }
}

/// The family of translations that produce `PCi_j` keywords, run once per
/// coordinate version. Mirrors the whole `if( !gotpcij )` block at
/// fitschan.c:31366-31548.
///
/// A single gate covers all four sources, in C's precedence order: the legacy
/// 6-digit `CDjjjiii` matrix, `CDj_i`, the 6-digit `PCjjjiii` matrix, and
/// `CROTAi`. The gate opens only when the header carries no `PCi_j` keyword for
/// this version, and each source that emits a matrix closes it against the
/// remaining ones — so, for example, a header with both `CDj_i` and `CROTAi`
/// takes its rotation from the CD matrix and leaves the CROTA cards untouched.
fn translate_pc_family(cards: &mut CardList, s: char, encoding: Encoding) {
    if key_fields(cards, &versioned("PC%d_%d", s), 0).0 > 0 {
        return;
    }
    let mut gotpcij = false;

    // CDjjjiii (fitschan.c:31403-31439). Primary plane only, and FITS-IRAF
    // only: that is the sole encoding which translates the legacy matrix into
    // PCj_i, so under any other encoding the scan is skipped entirely and the
    // cards stay unread — and therefore still reach the output.
    if encoding == Encoding::FitsIraf && s == ' ' {
        let (nmatch, ubnd) = key_fields(cards, "CD%3d%3d", 1);
        if nmatch > 0 {
            let naxis = ubnd[0].max(0) as usize;
            for j in 1..=naxis {
                for i in 1..=naxis {
                    let key = format!("CD{j:03}{i:03}");
                    let Some(dval) = cards.get_float(&key) else {
                        continue;
                    };
                    cards.delete_consumed(&key);
                    cards.set_float(&format!("PC{j}_{i}"), dval, "PC matrix element");
                    // The CD matrix carries the pixel scale, so CDELT reverts
                    // to its default of 1.0.
                    cards.set_float(&format!("CDELT{j}{}", suffix(s)), 1.0, "Pixel scale");
                    gotpcij = true;
                }
            }
        }
    }

    // CDj_i (fitschan.c:31406-31453). Each row is normalised to a unit vector,
    // its length becoming that row's CDELT so the pixel scale stays meaningful.
    if !gotpcij {
        let (nmatch, ubnd) = key_fields(cards, &versioned("CD%d_%d", s), 2);
        if nmatch > 0 {
            let naxis = ubnd[0].max(ubnd[1]).max(0) as usize;
            for j in 1..=naxis {
                // Row-vector length, signed by the diagonal element so the
                // diagonal PC element comes out positive.
                let mut rowsum2 = 0.0;
                let mut diag = 1.0;
                for i in 1..=naxis {
                    if let Some(dval) = cards.get_float(&versioned(&format!("CD{j}_{i}"), s)) {
                        rowsum2 += dval * dval;
                        if i == j {
                            diag = if dval >= 0.0 { 1.0 } else { -1.0 };
                        }
                    }
                }
                let cdelti = rowsum2.sqrt() * diag;
                cards.set_float(&versioned(&format!("CDELT{j}"), s), cdelti, "Pixel scale");
                for i in 1..=naxis {
                    // Every CD element defaults to zero, diagonal included.
                    let key = versioned(&format!("CD{j}_{i}"), s);
                    let mut dval = cards.get_float(&key).unwrap_or(0.0);
                    cards.delete_consumed(&key);
                    if cdelti != 0.0 {
                        dval /= cdelti;
                    }
                    cards.set_float(
                        &versioned(&format!("PC{j}_{i}"), s),
                        dval,
                        "PC matrix element",
                    );
                    gotpcij = true;
                }
            }
        }
    }

    // PCjjjiii and CROTAi (fitschan.c:31456-31547). Both need CDELT keywords:
    // the 6-digit rename because the matrix alone carries no scale, and the
    // CROTA derivation because it works from the CDELT ratio.
    if gotpcij {
        return;
    }
    let (nmatch, ubnd) = key_fields(cards, &versioned("CDELT%d", s), 1);
    if nmatch == 0 {
        return;
    }
    let naxis = ubnd[0].max(0) as usize;

    // C reads CROTA on the longitude axis first, then the latitude axis, so
    // latitude wins when both are present. Both are read either way, which
    // consumes them so neither reaches the output.
    let axes = celestial_axes(cards, s);
    let mut crota = None;
    if let Some((axlon, axlat)) = axes {
        for ax in [axlon, axlat] {
            let key = versioned(&format!("CROTA{ax}"), s);
            if let Some(v) = cards.get_float(&key) {
                crota = Some(v);
                cards.delete_consumed(&key);
            }
        }
    }

    // 6-digit PCjjjiii → PCj_i. Primary plane only; absent elements take the
    // unit-matrix default rather than being skipped.
    if s == ' ' && key_fields(cards, "PC%3d%3d", 0).0 > 0 {
        for j in 1..=naxis {
            for i in 1..=naxis {
                let key = format!("PC{j:03}{i:03}");
                let dval = match cards.get_float(&key) {
                    Some(v) => {
                        cards.delete_consumed(&key);
                        v
                    }
                    None if i == j => 1.0,
                    None => 0.0,
                };
                cards.set_float(&format!("PC{j}_{i}"), dval, "PC matrix element");
            }
        }
        return;
    }

    // CROTA → PCi_j. Needs celestial axes and a CDELT on each of them.
    let (Some((axlon, axlat)), Some(crota)) = (axes, crota) else {
        return;
    };
    let (Some(cdelti), Some(cdeltj)) = (
        cards.get_float(&versioned(&format!("CDELT{axlon}"), s)),
        cards.get_float(&versioned(&format!("CDELT{axlat}"), s)),
    ) else {
        return;
    };
    let cosrota = crota.to_radians().cos();
    let sinrota = crota.to_radians().sin();
    let lambda = cdeltj / cdelti;
    // Diagonal: cos(crota) on the celestial axes, 1.0 elsewhere.
    for i in 1..=naxis {
        let dval = if i == axlon || i == axlat {
            cosrota
        } else {
            1.0
        };
        cards.set_float(
            &versioned(&format!("PC{i}_{i}"), s),
            dval,
            "PC matrix element",
        );
    }
    cards.set_float(
        &versioned(&format!("PC{axlat}_{axlon}"), s),
        sinrota / lambda,
        "PC matrix element",
    );
    cards.set_float(
        &versioned(&format!("PC{axlon}_{axlat}"), s),
        -sinrota * lambda,
        "PC matrix element",
    );
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
        cards.delete_consumed(from);
        return false;
    }
    let comment = cards
        .find(from)
        .and_then(|c| c.comment.clone())
        .unwrap_or_default();
    if let Some(value) = cards.get_float(from) {
        cards.delete_consumed(from);
        cards.set_float(to, value, &comment);
        return true;
    }
    if let Some(value) = cards.get_string(from) {
        cards.delete_consumed(from);
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsWcs,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        let after = cards.iter().count();
        assert_eq!(before, after);
        assert_eq!(cards.get_string("RADESYS"), Some("FK5     ".to_string()));
    }

    #[test]
    fn zero_cdelt_is_replaced_with_one() {
        let mut cards = CardList::from_header(
            "CTYPE1  = 'WAVE    '\n\
             CTYPE2  = 'RA---TAN'\n\
             CTYPE3  = 'DEC--TAN'\n\
             CDELT1  =            1.000E-10\n\
             CDELT2  =                  0.0\n\
             CDELT3  =                  0.0\n\
             END",
        )
        .unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsWcs,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        // Non-zero CDELT untouched; zero CDELTs become 1.0 (the C fallback,
        // since 1.0E-6 * the zero CDELT is still zero).
        assert_eq!(cards.get_float("CDELT1"), Some(1.0e-10));
        assert_eq!(cards.get_float("CDELT2"), Some(1.0));
        assert_eq!(cards.get_float("CDELT3"), Some(1.0));
    }

    #[test]
    fn zero_cdelt_handles_alternate_versions() {
        let mut cards = CardList::from_header(
            "CDELT1A =                  0.0\n\
             CDELT2A =                  2.5\n\
             END",
        )
        .unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsWcs,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_float("CDELT1A"), Some(1.0));
        assert_eq!(cards.get_float("CDELT2A"), Some(2.5));
    }

    #[test]
    fn rename_keyword_moves_value_and_comment_to_new_name() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK5     '           / Reference frame\nEND").unwrap();
        assert!(rename_keyword(&mut cards, "RADECSYS", "RADESYS"));
        assert!(cards.find("RADECSYS").is_none());
        assert_eq!(cards.get_string("RADESYS"), Some("FK5     ".to_string()));
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
        assert_eq!(cards.get_string("RADESYS"), Some("FK5     ".to_string()));
    }

    #[test]
    fn translates_radecsys_to_radesys() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK5     '           / Old-style frame\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert!(cards.find("RADECSYS").is_none());
        assert_eq!(cards.get_string("RADESYS"), Some("FK5     ".to_string()));
    }

    #[test]
    fn keeps_radesys_when_both_present() {
        let mut cards =
            CardList::from_header("RADECSYS= 'FK4     '\nRADESYS = 'FK5     '\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_string("RADESYS"), Some("FK5     ".to_string()));
    }

    #[test]
    fn translates_longpole_to_lonpole() {
        let mut cards =
            CardList::from_header("LONGPOLE=                180.0 / Old name\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsIraf,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert!(cards.find("CD001001").is_none());
        assert_eq!(cards.get_float("PC1_1"), Some(-2.0));
        assert_eq!(cards.get_float("PC2_2"), Some(3.0));
        // CDELT is forced to 1.0 for every row that had a CDjjjiii.
        assert_eq!(cards.get_float("CDELT1"), Some(1.0));
        assert_eq!(cards.get_float("CDELT2"), Some(1.0));
    }

    #[test]
    fn cdjjjiii_survives_an_encoding_that_does_not_translate_it() {
        // Only FITS-IRAF turns CDjjjiii into a PC matrix, and the scan sits
        // inside that encoding test (fitschan.c:31410), so under any other
        // encoding the cards are neither read nor consumed and stay available
        // to the output rather than having their rotation silently dropped.
        let mut cards = CardList::from_header(
            "CD001001=                  1.0\n\
             CD002002=                  1.0\n\
             END",
        )
        .unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert!(cards.find("CD001001").is_some());
        assert!(cards.find("CD002002").is_some());
        assert!(cards.find("PC1_1").is_none());
    }

    #[test]
    fn cdjjjiii_translated_and_consumed_under_fits_iraf() {
        // The positive control for the case above: FITS-IRAF is the encoding
        // that uses the values, so there the cards are read, translated into a
        // PC matrix, and consumed.
        let mut cards = CardList::from_header(
            "CD001001=                  1.0\n\
             CD002002=                  1.0\n\
             END",
        )
        .unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsIraf,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert!(cards.find("CD001001").is_none());
        assert!(cards.find("CD002002").is_none());
        assert_eq!(cards.get_float("PC1_1"), Some(1.0));
        assert_eq!(cards.get_float("PC2_2"), Some(1.0));
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert!(cards.find("CROTA2").is_none());
        let cos30 = 30.0_f64.to_radians().cos();
        assert!((cards.get_float("PC1_1").unwrap() - cos30).abs() < 1e-10);
        assert!((cards.get_float("PC2_2").unwrap() - cos30).abs() < 1e-10);
        // CDELT preserved.
        assert_eq!(cards.get_float("CDELT1"), Some(-1.0));
        assert_eq!(cards.get_float("CDELT2"), Some(1.0));
    }

    #[test]
    fn crota_survives_when_pc_already_present() {
        // The CROTA reads live inside the `!gotpcij` gate, so an existing PC
        // matrix means CROTA is never looked at — and an unread card is an
        // unconsumed one, which reaches the output.
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_float("CROTA2"), Some(30.0));
        // PC values were not overwritten.
        assert_eq!(cards.get_float("PC1_1"), Some(1.0));
    }

    #[test]
    fn crota_suppressed_by_pc_on_a_non_celestial_axis() {
        // The gate is `astKeyFields( this, "PC%d_%d", ... )` — a template sweep
        // over every card, so a PC keyword naming only the spectral axis still
        // suppresses the CROTA translation, leaving the CROTA card in place.
        let mut cards = CardList::from_header(
            "CTYPE1  = 'RA---TAN'\n\
             CTYPE2  = 'DEC--TAN'\n\
             CTYPE3  = 'FREQ'\n\
             CDELT1  =                 -1.0\n\
             CDELT2  =                  1.0\n\
             CDELT3  =                  1.0\n\
             PC3_3   =                  1.0\n\
             CROTA2  =                 30.0\n\
             END",
        )
        .unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_float("CROTA2"), Some(30.0));
        assert!(cards.find("PC1_1").is_none());
        assert!(cards.find("PC1_2").is_none());
        assert!(cards.find("PC2_1").is_none());
        assert!(cards.find("PC2_2").is_none());
        assert_eq!(cards.get_float("PC3_3"), Some(1.0));
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        // C reads PROJP with `GetValue2(ret, this, ...)` (fitschan.c:
        // 31493-31494), which marks the source card used but never
        // deletes it (fitschan.c:18433-18435) — the card list itself
        // is untouched here; only the read pipeline's later
        // `fix_used(promote, remove)` pass (for a NATIVE-encoding
        // read) would actually strip it from output.
        assert!(cards.find("PROJP0").is_some());
        assert!(cards.find("PROJP1").is_some());
        assert_eq!(cards.get_float("PV2_0"), Some(0.0));
        assert_eq!(cards.get_float("PV2_1"), Some(1.0));
        assert_eq!(cards.get_float("PV2_2"), Some(-0.4));
    }

    #[test]
    fn projp_skipped_when_consistent_pv_set_present() {
        // PV0+PV1+PV2 all set on lon → C trusts PV, PROJP left alone
        // (fitschan.c:31552-31605 is never entered for this plane).
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        // PROJP card untouched, no PV2_0 written from it.
        assert!(cards.find("PROJP0").is_some());
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsWcs,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_float("CDELT1A"), Some(1.0));
    }

    #[test]
    fn translates_epoch_to_equinox() {
        let mut cards = CardList::from_header("EPOCH   =               1995.0\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert!(cards.find("EPOCH").is_none());
        assert_eq!(cards.get_float("EQUINOX"), Some(1995.0));
    }

    #[test]
    fn epoch_zero_becomes_b1950_equinox() {
        let mut cards = CardList::from_header("EPOCH   =                  0.0\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_float("EQUINOX"), Some(1950.0));
    }

    #[test]
    fn epoch_consumed_when_equinox_present() {
        let mut cards = CardList::from_header(
            "EPOCH   =               1995.0\nEQUINOX =               2000.0\nEND",
        )
        .unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert!(cards.find("EPOCH").is_none());
        assert_eq!(cards.get_float("EQUINOX"), Some(2000.0));
    }

    #[test]
    fn equinox_zero_becomes_b1950() {
        let mut cards = CardList::from_header("EQUINOX =                  0.0\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_float("EQUINOX"), Some(1950.0));
    }

    #[test]
    fn translates_equinox_b1995_string_to_julian_number() {
        let mut cards = CardList::from_header("EQUINOX = 'B1995.0'\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_float("EQUINOX"), Some(2000.0));
    }

    #[test]
    fn translates_date_obs_to_mjd_obs() {
        let mut cards = CardList::from_header("DATE-OBS= '2000-01-15T12:00:00'\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_string("CTYPE2"), Some("DEC--NCP".to_string()));
    }

    #[test]
    fn early_paper_iii_wave_frq_becomes_wave_f2w() {
        let mut cards = CardList::from_header("CTYPE3  = 'WAVE-FRQ'\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_string("CTYPE3"), Some("WAVE-F2W".to_string()));
    }

    #[test]
    fn early_paper_iii_velo_vel_redundant_blanks() {
        // VELO with X='V' and P='V' (because VELO maps to V) → blanks.
        let mut cards = CardList::from_header("CTYPE3  = 'VELO-VEL'\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsClass,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_string("CTYPE1"), Some("RA---ZPN-ZPX".to_string()));
        assert_eq!(cards.get_float("PV2_1"), Some(1.0));
        assert_eq!(cards.get_float("PV2_2"), Some(-0.4));
    }

    #[test]
    fn translates_tnx_to_tpn() {
        let mut cards =
            CardList::from_header("CTYPE1  = 'RA---TNX'\nCTYPE2  = 'DEC--TNX'\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        // CRPIX should now lie close to the image (within one full
        // wrap cycle of the original).
        let crpix1 = cards.get_float("CRPIX1").unwrap();
        assert!(crpix1.abs() < 200.0 + 50.0, "CRPIX1 not wrapped: {crpix1}");
    }

    #[test]
    fn carlin_suppresses_the_msx_car_wrap() {
        // C fitschan.c:32115 gates the whole block on
        // `!astGetCarLin( this )`. car4.head is the fixture that needs it:
        // CRPIX1 = 211076 with CD1_1 = -0.0016666667 gives an unwrapped
        // shift of 6.1399520760316237 rad, which is what C emits.
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/wcsconv/inputs/car4.head",
        ))
        .expect("car4.head should be readable");
        let base = CardList::from_header(&text).unwrap();

        let mut wrapped = base.clone();
        spec_trans_inplace(
            &mut wrapped,
            Encoding::FitsWcs,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_ne!(
            wrapped.get_float("CRPIX1"),
            Some(211076.0),
            "CarLin off: the wrap must rewrite CRPIX1"
        );

        let mut kept = base.clone();
        spec_trans_inplace(
            &mut kept,
            Encoding::FitsWcs,
            TransAttrs {
                car_lin: true,
                ..TransAttrs::default()
            },
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(
            kept.get_float("CRPIX1"),
            Some(211076.0),
            "CarLin on: CRPIX1 must be left alone"
        );
    }

    #[test]
    fn translates_restfreq_to_restfrq_with_ghz_scale() {
        let mut cards =
            CardList::from_header("RESTFREQ=                1.42 / Rest frequency in GHz\nEND")
                .unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert!(cards.find("RESTFREQ").is_none());
        let restfrq = cards.get_float("RESTFRQ").unwrap();
        assert!((restfrq - 1.42e9).abs() < 1.0);
    }

    #[test]
    fn translates_velo_lsr_to_vrad_plus_specsys() {
        let mut cards = CardList::from_header("CTYPE3  = 'VELO-LSR'\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
        assert_eq!(cards.get_string("CTYPE3"), Some("VRAD    ".to_string()));
        assert_eq!(cards.get_string("SPECSYS"), Some("LSRK".to_string()));
    }

    #[test]
    fn translates_lambda_to_wave() {
        let mut cards = CardList::from_header("CTYPE3  = 'LAMBDA  '\nEND").unwrap();
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsPc,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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
        spec_trans_inplace(
            &mut cards,
            Encoding::FitsClass,
            TransAttrs::default(),
            &mut crate::fitschan::spec_trans::TransWarnings::default(),
        );
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

# FitsChan Phase 4: Other Encodings + Guess Dispatcher

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement read+write support for all remaining FITS WCS encoding formats (FITS-IRAF, FITS-PC, FITS-AIPS, FITS-AIPS++, FITS-CLASS, DSS) and an auto-detection dispatcher that identifies the encoding from header keywords.

**Architecture:** Each encoding has its own module with `read_from_cards(cards) -> AstResult<FitsStore>` and `write_to_cards(store, cards) -> AstResult<()>` functions. All encodings funnel through FitsStore as the intermediate representation — the decode_wcs/encode_wcs pipeline from Phases 2/3 is reused unchanged. A guess_encoding module examines keyword signatures to detect which format a header uses. Study the C `GetEncoding`, `SpecTrans`, and per-encoding `*FromStore`/`StoreTo*` functions in `reference/starlink-ast/src/fitschan.c`.

**Tech Stack:** Rust 2024 edition. Reuses existing FitsStore, decode_wcs, encode_wcs infrastructure.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/starlink-ast/src/fitschan/guess_encoding.rs` | Auto-detect encoding from header keywords |
| `crates/starlink-ast/src/fitschan/enc_iraf.rs` | FITS-IRAF read/write (CD matrix + RADECSYS) |
| `crates/starlink-ast/src/fitschan/enc_pc.rs` | FITS-PC read/write (PCiiijjj 3-digit format) |
| `crates/starlink-ast/src/fitschan/enc_aips.rs` | FITS-AIPS + FITS-AIPS++ read/write (CROTA, EPOCH, AIPS spectral) |
| `crates/starlink-ast/src/fitschan/enc_class.rs` | FITS-CLASS read/write (DELTAV, VELO-LSR, offset freq) |
| `crates/starlink-ast/src/fitschan/enc_dss.rs` | DSS read/write (PLTRAH, AMDX/Y polynomials) |
| `crates/starlink-ast/src/fitschan/mod.rs` | Wire guess + all encodings into read()/write() |
| `crates/starlink-ast/tests/test_fitschan_encodings.rs` | Integration tests for all encodings |

---

### Task 1: Encoding Guess Dispatcher

**Files:**
- Create: `crates/starlink-ast/src/fitschan/guess_encoding.rs`
- Modify: `crates/starlink-ast/src/fitschan/mod.rs`

The guess dispatcher examines FITS header cards and determines which WCS encoding they use. The algorithm matches the C `GetEncoding` function (fitschan.c line 12466). Priority order: Native → CLASS → AIPS/AIPS++ → DSS → PC(3-digit) → IRAF(3-digit) → IRAF(CD+RADECSYS) → WCS(CD) → PC(RADECSYS) → AIPS(CROTA) → WCS(CRVAL) → Native.

- [ ] **Step 1: Create guess_encoding.rs**

```rust
//! Auto-detect WCS encoding from FITS header keywords.
//!
//! Examines keyword signatures to determine which encoding format a FITS
//! header uses. Mirrors the C `GetEncoding` function (fitschan.c ~line 12466).
//! Priority order is critical: earlier matches take precedence.

use super::card_list::CardList;
use super::Encoding;

/// Guess the WCS encoding used in the given header cards.
///
/// Returns `None` if no WCS-like keywords are found at all (no object to read).
/// The detection priority matches the C library exactly:
///
/// 1. Native (AST comment markers)
/// 2. FITS-CLASS (DELTAV + velocity keywords)
/// 3. FITS-AIPS++ (AIPS spectral CTYPE + CD/PROJP/LONPOLE/LATPOLE)
/// 4. FITS-AIPS (AIPS spectral CTYPE without CD etc.)
/// 5. DSS (PLTRAH keyword)
/// 6. FITS-PC (PC with 3-digit format: PC001001)
/// 7. FITS-IRAF (CD with 3-digit format: CD001001)
/// 8. FITS-IRAF (CDi_j + RADECSYS or PROJPi)
/// 9. FITS-WCS (CDi_j without RADECSYS)
/// 10. FITS-PC (RADECSYS/PROJPi without CD or PC)
/// 11. FITS-AIPS (CROTAi)
/// 12. FITS-WCS (CRVALi present)
/// 13. Native (fallback)
pub(crate) fn guess_encoding(cards: &CardList) -> Encoding {
    // Check for Native AST markers first.
    if has_native_markers(cards) {
        return Encoding::Native;
    }

    // Pre-scan for WCS-like structure.
    let has_wcs = has_wcs_keywords(cards);
    let has_cd_underscore = has_cd_matrix_keywords(cards);
    let has_pc_underscore = has_pc_matrix_keywords(cards);

    // FITS-CLASS: DELTAV + velocity keyword.
    if has_wcs && looks_like_class(cards) {
        return Encoding::FitsClass;
    }

    // FITS-AIPS/AIPS++: AIPS spectral CTYPE.
    if has_wcs && has_aips_spectral_ctype(cards) {
        if has_cd_underscore || has_projp_keywords(cards)
            || cards.find("LONPOLE").is_some()
            || cards.find("LATPOLE").is_some()
        {
            return Encoding::FitsAipsPP;
        }
        return Encoding::FitsAips;
    }

    // DSS: PLTRAH keyword (no has_wcs required).
    if cards.find("PLTRAH").is_some() {
        return Encoding::Dss;
    }

    // FITS-PC: 3-digit format (PC001001).
    if has_wcs && has_pc_3digit(cards) {
        return Encoding::FitsPc;
    }

    // FITS-IRAF: 3-digit CD format (CD001001).
    if has_wcs && has_cd_3digit(cards) {
        return Encoding::FitsIraf;
    }

    // FITS-IRAF: CDi_j with RADECSYS (misspelled) or PROJPi.
    if has_wcs && has_cd_underscore
        && (cards.find("RADECSYS").is_some() || has_projp_keywords(cards))
    {
        return Encoding::FitsIraf;
    }

    // FITS-WCS: CDi_j without old-style keywords.
    if has_wcs && has_cd_underscore {
        return Encoding::FitsWcs;
    }

    // FITS-PC: RADECSYS/PROJPi without CD or PC matrix.
    if has_wcs && !has_cd_underscore && !has_pc_underscore
        && (cards.find("RADECSYS").is_some() || has_projp_keywords(cards))
    {
        return Encoding::FitsPc;
    }

    // FITS-AIPS: CROTAi keyword.
    if has_wcs && has_crota(cards) {
        return Encoding::FitsAips;
    }

    // FITS-WCS: any CRVALi.
    if has_wcs {
        return Encoding::FitsWcs;
    }

    // Fallback: Native.
    Encoding::Native
}

/// Check for AST native comment markers ("AST +++").
fn has_native_markers(cards: &CardList) -> bool {
    cards.iter().any(|c| {
        c.keyword == "COMMENT" && matches!(&c.value,
            super::card::FitsValue::Comment(text) if text.contains("AST +++"))
    })
}

/// Check if any WCS-like keywords (CTYPE, CRPIX, CRVAL) are present.
fn has_wcs_keywords(cards: &CardList) -> bool {
    cards.find("CTYPE1").is_some()
        || cards.find("CRPIX1").is_some()
        || cards.find("CRVAL1").is_some()
}

/// Check for CDi_j keywords (underscore format).
fn has_cd_matrix_keywords(cards: &CardList) -> bool {
    cards.find("CD1_1").is_some()
        || cards.find("CD2_1").is_some()
        || cards.find("CD1_2").is_some()
}

/// Check for PCi_j keywords (underscore format).
fn has_pc_matrix_keywords(cards: &CardList) -> bool {
    cards.find("PC1_1").is_some()
        || cards.find("PC2_1").is_some()
        || cards.find("PC1_2").is_some()
}

/// Check for 3-digit PC format (PC001001, PC001002, etc).
fn has_pc_3digit(cards: &CardList) -> bool {
    cards.iter().any(|c| {
        c.keyword.len() == 8
            && c.keyword.starts_with("PC")
            && c.keyword[2..].chars().all(|ch| ch.is_ascii_digit())
    })
}

/// Check for 3-digit CD format (CD001001, CD001002, etc).
fn has_cd_3digit(cards: &CardList) -> bool {
    cards.iter().any(|c| {
        c.keyword.len() == 8
            && c.keyword.starts_with("CD")
            && c.keyword[2..].chars().all(|ch| ch.is_ascii_digit())
    })
}

/// Check for FITS-CLASS signatures: DELTAV + velocity keyword.
fn looks_like_class(cards: &CardList) -> bool {
    if cards.find("DELTAV").is_none() {
        return false;
    }
    cards.find("VLSR").is_some()
        || cards.find("VELO-LSR").is_some()
        || cards.find("VELO-OBS").is_some()
        || cards.find("VELO-HEL").is_some()
        || cards.find("VELO-EAR").is_some()
}

/// Check for AIPS-style spectral CTYPE (e.g., "VELO-LSR", "FREQ-LSR").
///
/// AIPS spectral format: 8 chars, first 4 are spectral type, then "-",
/// then 3-char reference frame code.
fn has_aips_spectral_ctype(cards: &CardList) -> bool {
    for i in 1..=9 {
        let key = format!("CTYPE{i}");
        if let Some(ctype) = cards.get_string(&key) {
            if is_aips_spectral(&ctype) {
                return true;
            }
        }
    }
    false
}

/// Check if a CTYPE value is an AIPS spectral type.
///
/// Format: "AAAA-BBB" where AAAA is FREQ/VELO/FELO and BBB is a
/// reference frame (LSR, LSD, HEL, GEO, EAR, OBS, TOP).
pub(crate) fn is_aips_spectral(ctype: &str) -> bool {
    let ct = ctype.trim();
    if ct.len() < 8 {
        return false;
    }
    let prefix = &ct[..4];
    let sep = ct.as_bytes()[4];
    let suffix = &ct[5..8];

    if sep != b'-' {
        return false;
    }

    let valid_prefix = matches!(prefix, "FREQ" | "VELO" | "FELO");
    let valid_suffix = matches!(suffix, "LSR" | "LSD" | "HEL" | "GEO" | "EAR" | "OBS" | "TOP");

    valid_prefix && valid_suffix
}

/// Check for PROJPi keywords (old projection parameters).
fn has_projp_keywords(cards: &CardList) -> bool {
    cards.iter().any(|c| c.keyword.starts_with("PROJP"))
}

/// Check for CROTAi keywords.
fn has_crota(cards: &CardList) -> bool {
    cards.find("CROTA1").is_some()
        || cards.find("CROTA2").is_some()
        || cards.find("CROTA3").is_some()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::card_list::CardList;

    fn cards_from(text: &str) -> CardList {
        CardList::from_header(text).unwrap()
    }

    #[test]
    fn detect_fitswcs_cdelt() {
        let cards = cards_from(
            "CTYPE1  = 'RA---TAN'\nCRVAL1  =  180.0\nCRPIX1  =  50.0\n\
             CDELT1  = -0.001\nCTYPE2  = 'DEC--TAN'\nCRVAL2  =  45.0\n\
             CRPIX2  =  50.0\nCDELT2  =  0.001\nRADESYS = 'ICRS'\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::FitsWcs);
    }

    #[test]
    fn detect_fitswcs_cd_matrix() {
        let cards = cards_from(
            "CTYPE1  = 'RA---TAN'\nCRVAL1  = 180.0\nCRPIX1  = 50.0\n\
             CTYPE2  = 'DEC--TAN'\nCRVAL2  = 45.0\nCRPIX2  = 50.0\n\
             CD1_1   = -0.001\nCD2_2   = 0.001\nRADESYS = 'ICRS'\nEND"
        );
        // CD + RADESYS (modern spelling) → FITS-WCS
        // Note: RADESYS (correct) vs RADECSYS (IRAF misspelling)
        assert_eq!(guess_encoding(&cards), Encoding::FitsWcs);
    }

    #[test]
    fn detect_iraf_radecsys() {
        let cards = cards_from(
            "CTYPE1  = 'RA---TAN'\nCRVAL1  = 180.0\nCRPIX1  = 50.0\n\
             CTYPE2  = 'DEC--TAN'\nCRVAL2  = 45.0\nCRPIX2  = 50.0\n\
             CD1_1   = -0.001\nCD2_2   = 0.001\nRADECSYS= 'FK5'\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::FitsIraf);
    }

    #[test]
    fn detect_pc_3digit() {
        let cards = cards_from(
            "CTYPE1  = 'RA---TAN'\nCRVAL1  = 180.0\nCRPIX1  = 50.0\n\
             CTYPE2  = 'DEC--TAN'\nCRVAL2  = 45.0\nCRPIX2  = 50.0\n\
             CDELT1  = -0.001\nCDELT2  = 0.001\nPC001001=  1.0\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::FitsPc);
    }

    #[test]
    fn detect_aips_crota() {
        let cards = cards_from(
            "CTYPE1  = 'RA---TAN'\nCRVAL1  = 180.0\nCRPIX1  = 50.0\n\
             CTYPE2  = 'DEC--TAN'\nCRVAL2  = 45.0\nCRPIX2  = 50.0\n\
             CDELT1  = -0.001\nCDELT2  = 0.001\nCROTA2  = 30.0\n\
             EPOCH   = 2000.0\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::FitsAips);
    }

    #[test]
    fn detect_aips_spectral() {
        let cards = cards_from(
            "CTYPE1  = 'VELO-LSR'\nCRVAL1  = 0.0\nCRPIX1  = 512.0\n\
             CDELT1  = 1000.0\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::FitsAips);
    }

    #[test]
    fn detect_aipspp_spectral_with_cd() {
        let cards = cards_from(
            "CTYPE1  = 'VELO-LSR'\nCRVAL1  = 0.0\nCRPIX1  = 512.0\n\
             CD1_1   = 1000.0\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::FitsAipsPP);
    }

    #[test]
    fn detect_class() {
        let cards = cards_from(
            "CTYPE1  = 'FREQ'\nCRVAL1  = 0.0\nCRPIX1  = 512.0\n\
             CDELT1  = 39062.5\nDELTAV  = -106.0\nVELO-LSR= 2500.0\n\
             RESTFREQ= 1.102E+11\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::FitsClass);
    }

    #[test]
    fn detect_dss() {
        let cards = cards_from(
            "PLTRAH  = 10.0\nPLTRAM  = 47.0\nPLTRAS  = 57.3\n\
             PLTDECD = 60.0\nPLTDECM = 15.0\nPLTDECS = 51.8\n\
             PLTDECSN= '-'\nXPIXELSZ= 25.28\nYPIXELSZ= 25.28\n\
             PPO3    = -18825.0\nPPO6    = -22216.0\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::Dss);
    }

    #[test]
    fn detect_native() {
        let cards = cards_from(
            "COMMENT  AST +++++++++++++++++++++++++++++++++ AST\n\
             COMMENT  AST    Begin ZoomMap                   AST\nEND"
        );
        assert_eq!(guess_encoding(&cards), Encoding::Native);
    }

    #[test]
    fn is_aips_spectral_valid() {
        assert!(is_aips_spectral("VELO-LSR"));
        assert!(is_aips_spectral("FREQ-HEL"));
        assert!(is_aips_spectral("FELO-GEO"));
        assert!(!is_aips_spectral("RA---TAN"));
        assert!(!is_aips_spectral("FREQ"));
        assert!(!is_aips_spectral("VRAD"));
    }
}
```

- [ ] **Step 2: Register module and verify**

Add to `mod.rs`:
```rust
pub(crate) mod guess_encoding;
```

Run: `cargo test -p starlink-ast -- guess_encoding`
Expected: all 10 tests pass

- [ ] **Step 3: Commit**

---

### Task 2: FITS-IRAF Encoding (Read + Write)

**Files:**
- Create: `crates/starlink-ast/src/fitschan/enc_iraf.rs`

FITS-IRAF uses `CDi_j` matrix (product of PC×CDELT), `RADECSYS` (misspelled), and older projection codes (NCP→SIN, GLS→SFL). On read, translate to standard FitsStore. On write, compute CD from PC×CDELT and use RADECSYS.

- [ ] **Step 1: Create enc_iraf.rs with read function**

```rust
//! FITS-IRAF encoding reader/writer.
//!
//! FITS-IRAF is similar to FITS-WCS but uses:
//! - CD matrix (combined PC×CDELT) instead of separate PC + CDELT
//! - `RADECSYS` (misspelled) instead of `RADESYS`
//! - Older projection codes: NCP (= SIN with params), GLS (= SFL)
//!
//! Mirrors the C `IRAFFromStore` (fitschan.c ~line 18815).

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read FITS-IRAF encoded keywords from a CardList into a FitsStore.
///
/// IRAF uses CDi_j combined matrix and RADECSYS keyword.
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    let naxis = determine_naxis(cards)?;
    let mut store = FitsStore::new(naxis);

    // CRPIX
    for i in 0..naxis {
        let key = format!("CRPIX{}", i + 1);
        store.crpix[i] = cards.get_float(&key);
    }

    // CRVAL
    for i in 0..naxis {
        let key = format!("CRVAL{}", i + 1);
        store.crval[i] = cards.get_float(&key);
    }

    // CTYPE — translate old projection codes.
    for i in 0..naxis {
        let key = format!("CTYPE{}", i + 1);
        if let Some(ctype) = cards.get_string(&key) {
            store.ctype[i] = Some(translate_iraf_ctype(&ctype));
        }
    }

    // CUNIT
    for i in 0..naxis {
        let key = format!("CUNIT{}", i + 1);
        store.cunit[i] = cards.get_string(&key);
    }

    // CD matrix → stored as CD in FitsStore.
    for i in 0..naxis {
        for j in 0..naxis {
            let key = format!("CD{}_{}", i + 1, j + 1);
            store.cd[i][j] = cards.get_float(&key);
        }
    }
    // Also check 3-digit format: CD001001.
    if !store.has_cd_matrix() {
        for i in 0..naxis {
            for j in 0..naxis {
                let key = format!("CD{:03}{:03}", i + 1, j + 1);
                store.cd[i][j] = cards.get_float(&key);
            }
        }
    }

    // RADECSYS (misspelled IRAF form) or RADESYS (standard).
    store.radesys = cards.get_string("RADECSYS")
        .or_else(|| cards.get_string("RADESYS"));

    // EQUINOX or EPOCH (fallback).
    store.equinox = cards.get_float("EQUINOX")
        .or_else(|| cards.get_float("EPOCH"));

    // MJD-OBS
    store.mjdobs = cards.get_float("MJD-OBS");

    // LONPOLE, LATPOLE (not strictly IRAF but tolerate).
    store.lonpole = cards.get_float("LONPOLE");
    store.latpole = cards.get_float("LATPOLE");

    // Spectral keywords.
    store.specsys = cards.get_string("SPECSYS");
    store.restfrq = cards.get_float("RESTFRQ")
        .or_else(|| cards.get_float("RESTFREQ"));
    store.restwav = cards.get_float("RESTWAV");

    Ok(store)
}

/// Write FitsStore to FITS-IRAF keywords.
///
/// Writes CD matrix (= PC × CDELT) and uses RADECSYS.
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    // CRPIX
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            cards.set_float(&format!("CRPIX{}", i + 1), val, "Reference pixel");
        }
    }

    // CRVAL
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            cards.set_float(&format!("CRVAL{}", i + 1), val, "Reference value");
        }
    }

    // CTYPE — translate back to IRAF codes.
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let iraf_ctype = translate_to_iraf_ctype(ctype);
            cards.set_string(&format!("CTYPE{}", i + 1), &iraf_ctype, "Axis type");
        }
    }

    // CUNIT
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            cards.set_string(&format!("CUNIT{}", i + 1), val, "Axis units");
        }
    }

    // CD matrix: CDi_j = PC[i][j] * CDELT[i].
    // If store has CD directly, use it; otherwise compute from PC×CDELT.
    if store.has_cd_matrix() {
        for i in 0..naxis {
            for j in 0..naxis {
                if let Some(val) = store.cd[i][j] {
                    if val != 0.0 || i == j {
                        cards.set_float(&format!("CD{}_{}", i + 1, j + 1), val, "");
                    }
                }
            }
        }
    } else {
        // Compute CD = PC × CDELT.
        for i in 0..naxis {
            let cdelt_i = store.cdelt[i].unwrap_or(1.0);
            for j in 0..naxis {
                let pc_default = if i == j { 1.0 } else { 0.0 };
                let pc_ij = store.pc[i][j].unwrap_or(pc_default);
                let cd_val = pc_ij * cdelt_i;
                if cd_val != 0.0 || i == j {
                    cards.set_float(&format!("CD{}_{}", i + 1, j + 1), cd_val, "");
                }
            }
        }
    }

    // RADECSYS (IRAF convention).
    if let Some(ref radesys) = store.radesys {
        cards.set_string("RADECSYS", radesys, "Reference frame");
    }

    // EQUINOX.
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "Equinox of coordinates");
    }

    // MJD-OBS.
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date");
    }

    Ok(())
}

/// Determine number of axes from NAXIS or WCSAXES.
fn determine_naxis(cards: &CardList) -> AstResult<usize> {
    let wcsaxes = cards.get_int("WCSAXES").map(|v| v as usize);
    let naxis = cards.get_int("NAXIS").map(|v| v as usize);
    match (wcsaxes, naxis) {
        (Some(w), _) => Ok(w),
        (None, Some(n)) => Ok(n),
        (None, None) => Err(AstError::new(
            ErrorCode::Bdfts,
            "cannot determine number of axes",
        )),
    }
}

/// Translate IRAF CTYPE values to standard WCS equivalents.
///
/// - "-NCP" → "-SIN" (NCP is SIN with specific parameters)
/// - "-GLS" → "-SFL" (GLS is the old name for SFL)
fn translate_iraf_ctype(ctype: &str) -> String {
    let mut result = ctype.to_string();
    if result.contains("-NCP") {
        result = result.replace("-NCP", "-SIN");
    }
    if result.contains("-GLS") {
        result = result.replace("-GLS", "-SFL");
    }
    result
}

/// Translate standard CTYPE back to IRAF equivalents.
///
/// - "-SFL" → "-GLS"
fn translate_to_iraf_ctype(ctype: &str) -> String {
    let mut result = ctype.to_string();
    if result.contains("-SFL") {
        result = result.replace("-SFL", "-GLS");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards_from(text: &str) -> CardList {
        CardList::from_header(text).unwrap()
    }

    #[test]
    fn read_iraf_cd_matrix() {
        let cards = cards_from(
            "NAXIS   = 2\nCRPIX1  = 50.5\nCRPIX2  = 50.5\n\
             CRVAL1  = 180.0\nCRVAL2  = 45.0\n\
             CTYPE1  = 'RA---TAN'\nCTYPE2  = 'DEC--TAN'\n\
             CD1_1   = -0.001\nCD2_2   = 0.001\n\
             RADECSYS= 'FK5'\nEQUINOX = 2000.0\nEND"
        );
        let store = read_from_cards(&cards).unwrap();
        assert_eq!(store.naxis, 2);
        assert_eq!(store.cd[0][0], Some(-0.001));
        assert_eq!(store.cd[1][1], Some(0.001));
        assert_eq!(store.radesys, Some("FK5".to_string()));
    }

    #[test]
    fn read_iraf_ncp_to_sin() {
        let cards = cards_from(
            "NAXIS   = 2\nCRPIX1  = 1.0\nCRPIX2  = 1.0\n\
             CRVAL1  = 0.0\nCRVAL2  = 0.0\n\
             CTYPE1  = 'RA---NCP'\nCTYPE2  = 'DEC--NCP'\n\
             CD1_1   = -0.001\nCD2_2   = 0.001\nEND"
        );
        let store = read_from_cards(&cards).unwrap();
        assert_eq!(store.ctype[0], Some("RA---SIN".to_string()));
        assert_eq!(store.ctype[1], Some("DEC--SIN".to_string()));
    }

    #[test]
    fn write_iraf_produces_cd() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(50.0);
        store.crpix[1] = Some(50.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.cd[0][0] = Some(-0.001);
        store.cd[1][1] = Some(0.001);
        store.radesys = Some("FK5".to_string());

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        assert!(cards.get_float("CD1_1").is_some());
        assert_eq!(cards.get_string("RADECSYS"), Some("FK5".to_string()));
        // Should NOT have CDELT or PC keywords.
        assert!(cards.get_float("CDELT1").is_none());
        assert!(cards.get_float("PC1_1").is_none());
    }

    #[test]
    fn translate_ctype_ncp() {
        assert_eq!(translate_iraf_ctype("RA---NCP"), "RA---SIN");
        assert_eq!(translate_iraf_ctype("DEC--NCP"), "DEC--SIN");
    }

    #[test]
    fn translate_ctype_gls() {
        assert_eq!(translate_iraf_ctype("GLON-GLS"), "GLON-SFL");
        assert_eq!(translate_to_iraf_ctype("GLON-SFL"), "GLON-GLS");
    }
}
```

- [ ] **Step 2: Register module and verify**

Add to `mod.rs`:
```rust
pub(crate) mod enc_iraf;
```

Run: `cargo test -p starlink-ast -- enc_iraf`
Expected: all tests pass

- [ ] **Step 3: Commit**

---

### Task 3: FITS-PC Encoding (Read + Write)

**Files:**
- Create: `crates/starlink-ast/src/fitschan/enc_pc.rs`

FITS-PC uses 3-digit axis numbering (`PC001001` instead of `PC1_1`), keeps PC and CDELT separate, and uses `RADECSYS`. On read, parse the 3-digit keywords into standard FitsStore PC + CDELT. On write, format with 3-digit axes.

- [ ] **Step 1: Create enc_pc.rs**

```rust
//! FITS-PC encoding reader/writer.
//!
//! FITS-PC is the pre-2002 WCS standard using 3-digit axis numbering
//! for the PC matrix (e.g., PC001001, PC001002) with separate CDELT.
//! Also uses `RADECSYS` and older projection codes.
//!
//! Mirrors the C `PCFromStore` (fitschan.c ~line 24423).

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read FITS-PC keywords into a FitsStore.
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    let naxis = determine_naxis(cards)?;
    let mut store = FitsStore::new(naxis);

    // CRPIX
    for i in 0..naxis {
        store.crpix[i] = cards.get_float(&format!("CRPIX{}", i + 1));
    }

    // CRVAL
    for i in 0..naxis {
        store.crval[i] = cards.get_float(&format!("CRVAL{}", i + 1));
    }

    // CDELT
    for i in 0..naxis {
        store.cdelt[i] = cards.get_float(&format!("CDELT{}", i + 1));
    }

    // CTYPE — translate old codes.
    for i in 0..naxis {
        if let Some(ctype) = cards.get_string(&format!("CTYPE{}", i + 1)) {
            store.ctype[i] = Some(translate_pc_ctype(&ctype));
        }
    }

    // CUNIT
    for i in 0..naxis {
        store.cunit[i] = cards.get_string(&format!("CUNIT{}", i + 1));
    }

    // PC matrix: try 3-digit format first, then underscore format.
    for i in 0..naxis {
        for j in 0..naxis {
            let key_3digit = format!("PC{:03}{:03}", i + 1, j + 1);
            let key_underscore = format!("PC{}_{}", i + 1, j + 1);
            store.pc[i][j] = cards.get_float(&key_3digit)
                .or_else(|| cards.get_float(&key_underscore));
        }
    }

    // RADECSYS or RADESYS.
    store.radesys = cards.get_string("RADECSYS")
        .or_else(|| cards.get_string("RADESYS"));
    store.equinox = cards.get_float("EQUINOX");
    store.mjdobs = cards.get_float("MJD-OBS");
    store.lonpole = cards.get_float("LONPOLE");
    store.latpole = cards.get_float("LATPOLE");
    store.specsys = cards.get_string("SPECSYS");
    store.restfrq = cards.get_float("RESTFRQ")
        .or_else(|| cards.get_float("RESTFREQ"));
    store.restwav = cards.get_float("RESTWAV");

    Ok(store)
}

/// Write FitsStore to FITS-PC keywords.
///
/// Uses 3-digit PC matrix format and separate CDELT.
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    // CRPIX
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            cards.set_float(&format!("CRPIX{}", i + 1), val, "Reference pixel");
        }
    }

    // CRVAL
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            cards.set_float(&format!("CRVAL{}", i + 1), val, "Reference value");
        }
    }

    // CDELT
    for i in 0..naxis {
        if let Some(val) = store.cdelt[i] {
            cards.set_float(&format!("CDELT{}", i + 1), val, "Pixel scale");
        } else if store.has_cd_matrix() {
            // Compute CDELT from CD row magnitude.
            let mut mag_sq = 0.0;
            for j in 0..naxis {
                let v = store.cd[i][j].unwrap_or(0.0);
                mag_sq += v * v;
            }
            let mag = mag_sq.sqrt();
            if mag > 0.0 {
                cards.set_float(&format!("CDELT{}", i + 1), mag, "Pixel scale");
            }
        }
    }

    // CTYPE — translate to PC-era codes.
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let pc_ctype = translate_to_pc_ctype(ctype);
            cards.set_string(&format!("CTYPE{}", i + 1), &pc_ctype, "Axis type");
        }
    }

    // CUNIT
    for i in 0..naxis {
        if let Some(ref val) = store.cunit[i] {
            cards.set_string(&format!("CUNIT{}", i + 1), val, "Axis units");
        }
    }

    // PC matrix in 3-digit format. If CD matrix is stored, factor it.
    if store.has_cd_matrix() {
        // Factor CD into PC (from split_cd logic).
        let cd = build_cd_array(store);
        let (cdelt, pc) = super::wcs_decompose::split_cd(&cd);
        // Write CDELTi (overwrite the ones above with correct values).
        for i in 0..naxis {
            cards.set_float(&format!("CDELT{}", i + 1), cdelt[i], "Pixel scale");
        }
        // Write PC matrix.
        for i in 0..naxis {
            for j in 0..naxis {
                let default = if i == j { 1.0 } else { 0.0 };
                if (pc[[i, j]] - default).abs() > 1e-15 {
                    let key = format!("PC{:03}{:03}", i + 1, j + 1);
                    cards.set_float(&key, pc[[i, j]], "");
                }
            }
        }
    } else if store.has_pc_matrix() {
        // Write stored PC matrix directly.
        for i in 0..naxis {
            for j in 0..naxis {
                if let Some(val) = store.pc[i][j] {
                    let default = if i == j { 1.0 } else { 0.0 };
                    if (val - default).abs() > 1e-15 {
                        let key = format!("PC{:03}{:03}", i + 1, j + 1);
                        cards.set_float(&key, val, "");
                    }
                }
            }
        }
    }

    // RADECSYS.
    if let Some(ref radesys) = store.radesys {
        cards.set_string("RADECSYS", radesys, "Reference frame");
    }
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "Equinox");
    }
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date");
    }

    Ok(())
}

fn determine_naxis(cards: &CardList) -> AstResult<usize> {
    let wcsaxes = cards.get_int("WCSAXES").map(|v| v as usize);
    let naxis = cards.get_int("NAXIS").map(|v| v as usize);
    match (wcsaxes, naxis) {
        (Some(w), _) => Ok(w),
        (None, Some(n)) => Ok(n),
        (None, None) => Err(AstError::new(ErrorCode::Bdfts, "cannot determine naxis")),
    }
}

fn build_cd_array(store: &FitsStore) -> ndarray::Array2<f64> {
    let n = store.naxis;
    let mut cd = ndarray::Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            cd[[i, j]] = store.cd[i][j].unwrap_or(if i == j { 1.0 } else { 0.0 });
        }
    }
    cd
}

/// Translate FITS-PC CTYPE to modern equivalents.
fn translate_pc_ctype(ctype: &str) -> String {
    let mut result = ctype.to_string();
    if result.contains("-GLS") {
        result = result.replace("-GLS", "-SFL");
    }
    if result.contains("-ATF") {
        result = result.replace("-ATF", "-AIT");
    }
    result
}

/// Translate modern CTYPE to FITS-PC equivalents.
fn translate_to_pc_ctype(ctype: &str) -> String {
    let mut result = ctype.to_string();
    if result.contains("-SFL") {
        result = result.replace("-SFL", "-GLS");
    }
    if result.contains("-AIT") {
        result = result.replace("-AIT", "-ATF");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards_from(text: &str) -> CardList {
        CardList::from_header(text).unwrap()
    }

    #[test]
    fn read_pc_3digit_format() {
        let cards = cards_from(
            "NAXIS   = 2\nCRPIX1  = 50.5\nCRPIX2  = 50.5\n\
             CRVAL1  = 180.0\nCRVAL2  = 45.0\n\
             CDELT1  = -0.0002778\nCDELT2  = 0.0002778\n\
             CTYPE1  = 'RA---TAN'\nCTYPE2  = 'DEC--TAN'\n\
             PC001001= 1.0\nRADECSYS= 'FK5'\nEQUINOX = 2000.0\nEND"
        );
        let store = read_from_cards(&cards).unwrap();
        assert_eq!(store.naxis, 2);
        assert_eq!(store.pc[0][0], Some(1.0));
        assert_eq!(store.cdelt[0], Some(-0.0002778));
        assert_eq!(store.radesys, Some("FK5".to_string()));
    }

    #[test]
    fn write_pc_3digit() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(50.0);
        store.crpix[1] = Some(50.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        // Rotated PC matrix.
        let cos30 = 30.0_f64.to_radians().cos();
        let sin30 = 30.0_f64.to_radians().sin();
        store.pc[0][0] = Some(cos30);
        store.pc[0][1] = Some(-sin30);
        store.pc[1][0] = Some(sin30);
        store.pc[1][1] = Some(cos30);
        store.radesys = Some("FK5".to_string());

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // Should have 3-digit PC keywords.
        assert!(cards.find("PC001001").is_some());
        assert!(cards.find("PC001002").is_some());
        // Should have CDELT.
        assert!(cards.get_float("CDELT1").is_some());
        // Should NOT have CDi_j.
        assert!(cards.find("CD1_1").is_none());
    }
}
```

- [ ] **Step 2: Register module and verify**

- [ ] **Step 3: Commit**

---

### Task 4: FITS-AIPS Encoding (Read + Write)

**Files:**
- Create: `crates/starlink-ast/src/fitschan/enc_aips.rs`

FITS-AIPS uses CROTA (rotation angle) instead of PC/CD matrix, EPOCH instead of EQUINOX, and AIPS-specific spectral CTYPE values ("VELO-LSR", "FREQ-HEL"). AIPS++ extends this with CD/PV parameters.

- [ ] **Step 1: Create enc_aips.rs**

```rust
//! FITS-AIPS and FITS-AIPS++ encoding reader/writer.
//!
//! FITS-AIPS uses:
//! - CROTA (rotation angle in degrees) instead of PC/CD matrix
//! - EPOCH instead of EQUINOX (with IAU 1984 rule for RADESYS)
//! - AIPS spectral CTYPE: "VELO-LSR", "FREQ-HEL", "FELO-GEO"
//!
//! FITS-AIPS++ adds CD matrix, PV parameters, LONPOLE/LATPOLE.
//!
//! Mirrors C `AIPSFromStore`/`AIPSPPFromStore` (fitschan.c ~line 3069).

use super::card_list::CardList;
use super::fits_store::FitsStore;
use super::guess_encoding::is_aips_spectral;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read FITS-AIPS keywords into a FitsStore.
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    let naxis = determine_naxis(cards)?;
    let mut store = FitsStore::new(naxis);

    // Standard per-axis keywords.
    for i in 0..naxis {
        store.crpix[i] = cards.get_float(&format!("CRPIX{}", i + 1));
        store.crval[i] = cards.get_float(&format!("CRVAL{}", i + 1));
        store.cdelt[i] = cards.get_float(&format!("CDELT{}", i + 1));
        store.cunit[i] = cards.get_string(&format!("CUNIT{}", i + 1));
    }

    // CTYPE: translate AIPS spectral codes to standard.
    for i in 0..naxis {
        if let Some(ctype) = cards.get_string(&format!("CTYPE{}", i + 1)) {
            let (translated, specsys) = translate_aips_ctype(&ctype);
            store.ctype[i] = Some(translated);
            if let Some(ss) = specsys {
                store.specsys = Some(ss);
            }
        }
    }

    // CROTA → PC matrix conversion.
    // CROTA applies to the latitude (second celestial) axis.
    let crota = cards.get_float("CROTA2")
        .or_else(|| cards.get_float("CROTA1"));
    if let Some(angle_deg) = crota {
        let angle = angle_deg.to_radians();
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        // Standard CROTA→PC conversion for 2D celestial case.
        // PC[0][0] = cos(CROTA), PC[0][1] = -sin(CROTA)
        // PC[1][0] = sin(CROTA), PC[1][1] = cos(CROTA)
        if naxis >= 2 {
            store.pc[0][0] = Some(cos_a);
            store.pc[0][1] = Some(-sin_a);
            store.pc[1][0] = Some(sin_a);
            store.pc[1][1] = Some(cos_a);
        }
    }

    // CD matrix (AIPS++ only).
    for i in 0..naxis {
        for j in 0..naxis {
            let key = format!("CD{}_{}", i + 1, j + 1);
            store.cd[i][j] = cards.get_float(&key);
        }
    }

    // EPOCH → EQUINOX with IAU 1984 rule.
    if let Some(epoch) = cards.get_float("EPOCH") {
        let epoch_val = if epoch == 0.0 { 1950.0 } else { epoch };
        store.equinox = Some(epoch_val);
        // IAU 1984 rule: epoch < 1984 → FK4, else FK5.
        if store.radesys.is_none() {
            store.radesys = Some(if epoch_val < 1984.0 {
                "FK4".to_string()
            } else {
                "FK5".to_string()
            });
        }
    }
    // Also check EQUINOX directly (AIPS++ may have it).
    if store.equinox.is_none() {
        store.equinox = cards.get_float("EQUINOX");
    }
    if store.radesys.is_none() {
        store.radesys = cards.get_string("RADESYS")
            .or_else(|| cards.get_string("RADECSYS"));
    }

    // LONPOLE, LATPOLE (AIPS++ only).
    store.lonpole = cards.get_float("LONPOLE");
    store.latpole = cards.get_float("LATPOLE");

    // Spectral keywords.
    store.restfrq = cards.get_float("RESTFRQ")
        .or_else(|| cards.get_float("RESTFREQ"));
    store.restwav = cards.get_float("RESTWAV");
    if store.specsys.is_none() {
        store.specsys = cards.get_string("SPECSYS");
    }
    store.mjdobs = cards.get_float("MJD-OBS");

    // PV parameters (AIPS++ only).
    for i in 0..naxis {
        for m in 0..store.pv[i].len() {
            let key = format!("PV{}_{}", i + 1, m);
            store.pv[i][m] = cards.get_float(&key);
        }
    }

    Ok(store)
}

/// Write FitsStore to FITS-AIPS keywords.
///
/// Uses CROTA, EPOCH, and AIPS spectral CTYPE values.
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    // CRPIX, CRVAL, CDELT.
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            cards.set_float(&format!("CRPIX{}", i + 1), val, "Reference pixel");
        }
        if let Some(val) = store.crval[i] {
            cards.set_float(&format!("CRVAL{}", i + 1), val, "Reference value");
        }
        if let Some(val) = store.cdelt[i] {
            cards.set_float(&format!("CDELT{}", i + 1), val, "Pixel scale");
        }
    }

    // CTYPE: translate to AIPS spectral codes.
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let aips_ctype = translate_to_aips_ctype(ctype, store.specsys.as_deref());
            cards.set_string(&format!("CTYPE{}", i + 1), &aips_ctype, "Axis type");
        }
    }

    // CROTA from PC matrix.
    if store.has_pc_matrix() && naxis >= 2 {
        let pc10 = store.pc[1][0].unwrap_or(0.0);
        let pc11 = store.pc[1][1].unwrap_or(1.0);
        let crota = pc10.atan2(pc11).to_degrees();
        if crota.abs() > 1e-10 {
            cards.set_float("CROTA2", crota, "Rotation angle");
        }
    }

    // EPOCH (instead of EQUINOX for plain AIPS).
    if let Some(equinox) = store.equinox {
        cards.set_float("EPOCH", equinox, "Epoch of coordinates");
    }

    // Spectral keywords.
    if let Some(restfrq) = store.restfrq {
        cards.set_float("RESTFREQ", restfrq, "Rest frequency (Hz)");
    }

    Ok(())
}

/// Write FitsStore to FITS-AIPS++ keywords.
///
/// Like AIPS but adds CD matrix, PV parameters, LONPOLE/LATPOLE.
pub(crate) fn write_to_cards_pp(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    // CRPIX, CRVAL.
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            cards.set_float(&format!("CRPIX{}", i + 1), val, "Reference pixel");
        }
        if let Some(val) = store.crval[i] {
            cards.set_float(&format!("CRVAL{}", i + 1), val, "Reference value");
        }
    }

    // CTYPE: AIPS spectral.
    for i in 0..naxis {
        if let Some(ref ctype) = store.ctype[i] {
            let aips_ctype = translate_to_aips_ctype(ctype, store.specsys.as_deref());
            cards.set_string(&format!("CTYPE{}", i + 1), &aips_ctype, "Axis type");
        }
    }

    // CD matrix (AIPS++ uses CDi_j).
    if store.has_cd_matrix() {
        for i in 0..naxis {
            for j in 0..naxis {
                if let Some(val) = store.cd[i][j] {
                    if val != 0.0 || i == j {
                        cards.set_float(&format!("CD{}_{}", i + 1, j + 1), val, "");
                    }
                }
            }
        }
    } else {
        // Compute CD = PC × CDELT.
        for i in 0..naxis {
            let cdelt_i = store.cdelt[i].unwrap_or(1.0);
            for j in 0..naxis {
                let pc_default = if i == j { 1.0 } else { 0.0 };
                let pc_ij = store.pc[i][j].unwrap_or(pc_default);
                let cd_val = pc_ij * cdelt_i;
                if cd_val != 0.0 || i == j {
                    cards.set_float(&format!("CD{}_{}", i + 1, j + 1), cd_val, "");
                }
            }
        }
    }

    // EQUINOX (AIPS++ uses EQUINOX, not EPOCH).
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "Equinox");
    }

    // LONPOLE, LATPOLE.
    if let Some(lonpole) = store.lonpole {
        cards.set_float("LONPOLE", lonpole, "Native longitude of celestial pole");
    }
    if let Some(latpole) = store.latpole {
        cards.set_float("LATPOLE", latpole, "Native latitude of celestial pole");
    }

    // PV parameters.
    for i in 0..naxis {
        for m in 0..store.pv[i].len() {
            if let Some(val) = store.pv[i][m] {
                cards.set_float(&format!("PV{}_{}", i + 1, m), val, "");
            }
        }
    }

    // Spectral.
    if let Some(restfrq) = store.restfrq {
        cards.set_float("RESTFREQ", restfrq, "Rest frequency (Hz)");
    }
    if let Some(ref specsys) = store.specsys {
        cards.set_string("SPECSYS", specsys, "Spectral reference frame");
    }

    Ok(())
}

fn determine_naxis(cards: &CardList) -> AstResult<usize> {
    cards.get_int("WCSAXES").map(|v| v as usize)
        .or_else(|| cards.get_int("NAXIS").map(|v| v as usize))
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "cannot determine naxis"))
}

/// Translate AIPS spectral CTYPE to standard WCS equivalents.
///
/// "VELO-LSR" → ("VRAD", Some("LSRK"))
/// "FREQ-HEL" → ("FREQ", Some("BARYCENT"))
/// "FELO-GEO" → ("VOPT", Some("GEOCENTR"))
fn translate_aips_ctype(ctype: &str) -> (String, Option<String>) {
    let ct = ctype.trim();
    if ct.len() >= 8 && is_aips_spectral(ct) {
        let prefix = &ct[..4];
        let suffix = &ct[5..8];

        let standard_type = match prefix {
            "VELO" => "VRAD",
            "FELO" => "VOPT",
            "FREQ" => "FREQ",
            _ => return (ct.to_string(), None),
        };

        let specsys = match suffix {
            "LSR" => "LSRK",
            "LSD" => "LSRD",
            "HEL" => "BARYCENT",
            "GEO" => "GEOCENTR",
            "EAR" | "OBS" | "TOP" => "TOPOCENT",
            _ => return (ct.to_string(), None),
        };

        (standard_type.to_string(), Some(specsys.to_string()))
    } else {
        (ct.to_string(), None)
    }
}

/// Translate standard CTYPE + SPECSYS to AIPS spectral format.
fn translate_to_aips_ctype(ctype: &str, specsys: Option<&str>) -> String {
    let ct = ctype.trim();
    let prefix = if ct.len() >= 4 { &ct[..4] } else { ct };

    let aips_prefix = match prefix {
        "VRAD" => "VELO",
        "VOPT" => "FELO",
        "FREQ" => "FREQ",
        _ => return ct.to_string(),
    };

    let aips_suffix = match specsys.unwrap_or("") {
        "LSRK" | "LSR" => "LSR",
        "LSRD" => "LSD",
        "BARYCENT" | "HELIOCEN" => "HEL",
        "GEOCENTR" => "GEO",
        "TOPOCENT" => "OBS",
        _ => "LSR",
    };

    format!("{aips_prefix}-{aips_suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards_from(text: &str) -> CardList {
        CardList::from_header(text).unwrap()
    }

    #[test]
    fn read_aips_crota() {
        let cards = cards_from(
            "NAXIS   = 2\nCRPIX1  = 50.0\nCRPIX2  = 50.0\n\
             CRVAL1  = 180.0\nCRVAL2  = 45.0\n\
             CDELT1  = -0.001\nCDELT2  = 0.001\n\
             CTYPE1  = 'RA---TAN'\nCTYPE2  = 'DEC--TAN'\n\
             CROTA2  = 30.0\nEPOCH   = 2000.0\nEND"
        );
        let store = read_from_cards(&cards).unwrap();
        // CROTA=30 → PC[1][0] = sin(30) ≈ 0.5.
        let pc10 = store.pc[1][0].unwrap();
        assert!((pc10 - 0.5).abs() < 1e-10);
        // EPOCH=2000 → equinox=2000, radesys=FK5.
        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.radesys, Some("FK5".to_string()));
    }

    #[test]
    fn read_aips_spectral() {
        let cards = cards_from(
            "NAXIS   = 1\nCTYPE1  = 'VELO-LSR'\nCRVAL1  = 0.0\n\
             CRPIX1  = 512.0\nCDELT1  = 1000.0\nEND"
        );
        let store = read_from_cards(&cards).unwrap();
        assert_eq!(store.ctype[0], Some("VRAD".to_string()));
        assert_eq!(store.specsys, Some("LSRK".to_string()));
    }

    #[test]
    fn read_aips_epoch_fk4() {
        let cards = cards_from(
            "NAXIS   = 2\nCTYPE1  = 'RA---TAN'\nCTYPE2  = 'DEC--TAN'\n\
             CRVAL1  = 180.0\nCRVAL2  = 45.0\nCRPIX1  = 1.0\nCRPIX2  = 1.0\n\
             CDELT1  = -0.001\nCDELT2  = 0.001\nEPOCH   = 1950.0\nEND"
        );
        let store = read_from_cards(&cards).unwrap();
        assert_eq!(store.equinox, Some(1950.0));
        assert_eq!(store.radesys, Some("FK4".to_string()));
    }

    #[test]
    fn translate_aips_ctype_velo() {
        let (ct, ss) = translate_aips_ctype("VELO-LSR");
        assert_eq!(ct, "VRAD");
        assert_eq!(ss, Some("LSRK".to_string()));
    }

    #[test]
    fn translate_aips_ctype_freq_hel() {
        let (ct, ss) = translate_aips_ctype("FREQ-HEL");
        assert_eq!(ct, "FREQ");
        assert_eq!(ss, Some("BARYCENT".to_string()));
    }

    #[test]
    fn translate_to_aips_vrad() {
        let result = translate_to_aips_ctype("VRAD", Some("LSRK"));
        assert_eq!(result, "VELO-LSR");
    }

    #[test]
    fn write_aips_crota() {
        let mut store = FitsStore::new(2);
        store.crpix[0] = Some(50.0);
        store.crpix[1] = Some(50.0);
        store.crval[0] = Some(180.0);
        store.crval[1] = Some(45.0);
        store.cdelt[0] = Some(-0.001);
        store.cdelt[1] = Some(0.001);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        // 30-degree rotation.
        let cos30 = 30.0_f64.to_radians().cos();
        let sin30 = 30.0_f64.to_radians().sin();
        store.pc[0][0] = Some(cos30);
        store.pc[0][1] = Some(-sin30);
        store.pc[1][0] = Some(sin30);
        store.pc[1][1] = Some(cos30);
        store.equinox = Some(2000.0);

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        let crota = cards.get_float("CROTA2").unwrap();
        assert!((crota - 30.0).abs() < 1e-6);
        assert!(cards.get_float("EPOCH").is_some());
        assert!(cards.find("CD1_1").is_none());
        assert!(cards.find("PC001001").is_none());
    }
}
```

- [ ] **Step 2: Register module and verify**

- [ ] **Step 3: Commit**

---

### Task 5: FITS-CLASS Encoding (Read + Write)

**Files:**
- Create: `crates/starlink-ast/src/fitschan/enc_class.rs`

FITS-CLASS is used by the GILDAS/CLASS radio spectral survey software. It enforces 3 axes (2 spatial + 1 spectral), uses offset frequency (CRVAL3 is offset from RESTFREQ), requires VELO-LSR/DELTAV, and uses SPECSYS="SOURCE". On read, add RESTFREQ back to get absolute CRVAL. On write, subtract RESTFREQ.

- [ ] **Step 1: Create enc_class.rs**

```rust
//! FITS-CLASS encoding reader/writer.
//!
//! FITS-CLASS is the GILDAS/CLASS spectral survey format. Key features:
//! - Enforces 3 axes: 2 spatial + 1 spectral (FREQ)
//! - Spectral CRVAL is offset from RESTFREQ (not absolute)
//! - Requires DELTAV (velocity channel spacing) and VELO-LSR
//! - Uses SPECSYS="SOURCE" (source rest frame)
//! - Older projection codes: GLS (=SFL), ATF (=AIT)
//!
//! Mirrors C `CLASSFromStore`/`ClassTrans` (fitschan.c ~line 6324/6955).

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read FITS-CLASS keywords into a FitsStore.
///
/// Translates CLASS conventions to standard WCS:
/// - Adds RESTFREQ to offset CRVAL to get absolute frequency
/// - Translates VELO-* to SPECSYS
/// - Translates GLS→SFL, ATF→AIT projection codes
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    let naxis = cards.get_int("NAXIS").unwrap_or(3) as usize;
    let mut store = FitsStore::new(naxis);

    // Standard per-axis keywords.
    for i in 0..naxis {
        store.crpix[i] = cards.get_float(&format!("CRPIX{}", i + 1));
        store.crval[i] = cards.get_float(&format!("CRVAL{}", i + 1));
        store.cdelt[i] = cards.get_float(&format!("CDELT{}", i + 1));
        store.cunit[i] = cards.get_string(&format!("CUNIT{}", i + 1));
    }

    // CTYPE — translate CLASS codes.
    for i in 0..naxis {
        if let Some(ctype) = cards.get_string(&format!("CTYPE{}", i + 1)) {
            store.ctype[i] = Some(translate_class_ctype(&ctype));
        }
    }

    // Find the spectral axis (FREQ).
    let spec_axis = store.ctype.iter().position(|ct| {
        ct.as_ref().map_or(false, |s| s.starts_with("FREQ"))
    });

    // RESTFREQ: required for CLASS. Apply offset correction.
    let restfreq = cards.get_float("RESTFREQ")
        .or_else(|| cards.get_float("RESTFRQ"));
    if let Some(rf) = restfreq {
        store.restfrq = Some(rf);
        // CLASS CRVAL is offset frequency: absolute = CRVAL + RESTFREQ.
        if let Some(axis) = spec_axis {
            if let Some(offset_crval) = store.crval[axis] {
                store.crval[axis] = Some(offset_crval + rf);
            }
        }
    }

    // SPECSYS from VELO-* keywords.
    store.specsys = if cards.find("VELO-LSR").is_some() || cards.find("VLSR").is_some() {
        Some("LSRK".to_string())
    } else if cards.find("VELO-HEL").is_some() {
        Some("BARYCENT".to_string())
    } else if cards.find("VELO-OBS").is_some() || cards.find("VELO-EAR").is_some() {
        Some("TOPOCENT".to_string())
    } else {
        cards.get_string("SPECSYS")
    };

    // Coordinate system.
    store.radesys = cards.get_string("RADESYS")
        .or_else(|| cards.get_string("RADECSYS"));
    store.equinox = cards.get_float("EQUINOX");
    store.mjdobs = cards.get_float("MJD-OBS");
    store.lonpole = cards.get_float("LONPOLE");
    store.latpole = cards.get_float("LATPOLE");

    Ok(store)
}

/// Write FitsStore to FITS-CLASS keywords.
///
/// Converts to CLASS conventions:
/// - Subtracts RESTFREQ from spectral CRVAL (offset frequency)
/// - Computes DELTAV (velocity channel spacing)
/// - Writes VELO-LSR
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    let naxis = store.naxis;

    cards.set_int("NAXIS", naxis as i64, "Number of axes");

    // Write per-axis keywords.
    for i in 0..naxis {
        if let Some(val) = store.crpix[i] {
            cards.set_float(&format!("CRPIX{}", i + 1), val, "Reference pixel");
        }

        // CTYPE — translate to CLASS codes.
        if let Some(ref ctype) = store.ctype[i] {
            let class_ctype = translate_to_class_ctype(ctype);
            cards.set_string(&format!("CTYPE{}", i + 1), &class_ctype, "Axis type");
        }
    }

    // Find spectral axis.
    let spec_axis = store.ctype.iter().position(|ct| {
        ct.as_ref().map_or(false, |s| s.starts_with("FREQ"))
    });

    // RESTFREQ.
    let restfreq = store.restfrq.unwrap_or(0.0);
    if restfreq > 0.0 {
        cards.set_float("RESTFREQ", restfreq, "Rest frequency (Hz)");
    }

    // CRVAL — subtract RESTFREQ for spectral axis.
    for i in 0..naxis {
        if let Some(val) = store.crval[i] {
            let output_val = if Some(i) == spec_axis && restfreq > 0.0 {
                val - restfreq
            } else {
                val
            };
            cards.set_float(&format!("CRVAL{}", i + 1), output_val, "Reference value");
        }
    }

    // CDELT.
    for i in 0..naxis {
        if let Some(val) = store.cdelt[i] {
            cards.set_float(&format!("CDELT{}", i + 1), val, "Pixel scale");
        }
    }

    // SPECSYS.
    if let Some(ref specsys) = store.specsys {
        cards.set_string("SPECSYS", specsys, "Spectral reference frame");
    }

    // DELTAV: velocity channel spacing from frequency CDELT.
    // deltav = -c * cdelt_freq / restfreq (radio velocity convention).
    if let Some(axis) = spec_axis {
        if let Some(cdelt_freq) = store.cdelt[axis] {
            if restfreq > 0.0 {
                const SPEED_OF_LIGHT: f64 = 299_792_458.0;
                let deltav = -SPEED_OF_LIGHT * cdelt_freq / restfreq;
                cards.set_float("DELTAV", deltav, "Velocity spacing (m/s)");
            }
        }
    }

    // VELO-LSR: velocity at reference channel.
    // velo = c * (1 - crval_freq / restfreq) (radio convention).
    if let Some(axis) = spec_axis {
        if let Some(crval_freq) = store.crval[axis] {
            if restfreq > 0.0 {
                const SPEED_OF_LIGHT: f64 = 299_792_458.0;
                let velo = SPEED_OF_LIGHT * (1.0 - crval_freq / restfreq);
                cards.set_float("VELO-LSR", velo, "Velocity at ref channel (m/s)");
            }
        }
    }

    // Coordinate metadata.
    if let Some(ref radesys) = store.radesys {
        cards.set_string("RADESYS", radesys, "Reference frame");
    }
    if let Some(equinox) = store.equinox {
        cards.set_float("EQUINOX", equinox, "Equinox");
    }
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date");
    }

    Ok(())
}

/// Translate CLASS CTYPE to standard WCS equivalents.
fn translate_class_ctype(ctype: &str) -> String {
    let mut result = ctype.to_string();
    if result.contains("-GLS") {
        result = result.replace("-GLS", "-SFL");
    }
    if result.contains("-ATF") {
        result = result.replace("-ATF", "-AIT");
    }
    result
}

/// Translate standard CTYPE to CLASS equivalents.
fn translate_to_class_ctype(ctype: &str) -> String {
    let mut result = ctype.to_string();
    if result.contains("-SFL") {
        result = result.replace("-SFL", "-GLS");
    }
    if result.contains("-AIT") {
        result = result.replace("-AIT", "-ATF");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards_from(text: &str) -> CardList {
        CardList::from_header(text).unwrap()
    }

    #[test]
    fn read_class_offset_frequency() {
        let cards = cards_from(
            "NAXIS   = 3\n\
             CTYPE1  = 'FREQ'\nCRVAL1  = 0.0\nCRPIX1  = 512.5\nCDELT1  = 39062.5\n\
             CTYPE2  = 'RA---TAN'\nCRVAL2  = 40.738\nCRPIX2  = 0.5\nCDELT2  = -0.01\n\
             CTYPE3  = 'DEC--TAN'\nCRVAL3  = 62.933\nCRPIX3  = 0.5\nCDELT3  = 0.01\n\
             RESTFREQ= 1.102013543E+11\nDELTAV  = -106.0\nVELO-LSR= 2500.0\n\
             RADESYS = 'FK5'\nEQUINOX = 2000.0\nEND"
        );
        let store = read_from_cards(&cards).unwrap();
        // CRVAL1 should be RESTFREQ + offset (0.0) = RESTFREQ.
        let crval1 = store.crval[0].unwrap();
        assert!((crval1 - 1.102013543e11).abs() < 1.0);
        assert_eq!(store.specsys, Some("LSRK".to_string()));
        assert_eq!(store.restfrq, Some(1.102013543e11));
    }

    #[test]
    fn read_class_gls_to_sfl() {
        let cards = cards_from(
            "NAXIS   = 3\n\
             CTYPE1  = 'FREQ'\nCRVAL1  = 0.0\nCRPIX1  = 1.0\nCDELT1  = 1e6\n\
             CTYPE2  = 'GLON-GLS'\nCRVAL2  = 0.0\nCRPIX2  = 1.0\nCDELT2  = 0.01\n\
             CTYPE3  = 'GLAT-GLS'\nCRVAL3  = 0.0\nCRPIX3  = 1.0\nCDELT3  = 0.01\n\
             RESTFREQ= 1.4e9\nDELTAV  = -100.0\nVLSR    = 0.0\nEND"
        );
        let store = read_from_cards(&cards).unwrap();
        assert_eq!(store.ctype[1], Some("GLON-SFL".to_string()));
        assert_eq!(store.ctype[2], Some("GLAT-SFL".to_string()));
    }

    #[test]
    fn write_class_subtracts_restfreq() {
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.102013543e11); // absolute frequency
        store.crpix[0] = Some(512.5);
        store.cdelt[0] = Some(39062.5);
        store.ctype[1] = Some("RA---TAN".to_string());
        store.crval[1] = Some(40.738);
        store.crpix[1] = Some(0.5);
        store.cdelt[1] = Some(-0.01);
        store.ctype[2] = Some("DEC--TAN".to_string());
        store.crval[2] = Some(62.933);
        store.crpix[2] = Some(0.5);
        store.cdelt[2] = Some(0.01);
        store.restfrq = Some(1.102013543e11);
        store.specsys = Some("LSRK".to_string());

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // CRVAL1 should be offset (≈ 0).
        let crval1 = cards.get_float("CRVAL1").unwrap();
        assert!(crval1.abs() < 1.0);
        // DELTAV should be computed.
        assert!(cards.get_float("DELTAV").is_some());
        // VELO-LSR should be computed.
        assert!(cards.get_float("VELO-LSR").is_some());
    }
}
```

- [ ] **Step 2: Register module and verify**

- [ ] **Step 3: Commit**

---

### Task 6: DSS Encoding (Read + Write)

**Files:**
- Create: `crates/starlink-ast/src/fitschan/enc_dss.rs`

DSS (Digitized Sky Survey) uses plate solution keywords (PLTRAH/M/S, PLTDEC*, AMDX/Y polynomials, XPIXELSZ/YPIXELSZ, PPO3/PPO6, CNPIX). Always FK5 J2000, TAN projection, 2D only, no rotation.

- [ ] **Step 1: Create enc_dss.rs**

```rust
//! DSS (Digitized Sky Survey) encoding reader/writer.
//!
//! DSS uses plate solution keywords specific to photographic plate scans:
//! - Plate centre: PLTRAH/M/S, PLTDECD/M/S, PLTDECSN
//! - Pixel size: XPIXELSZ, YPIXELSZ (microns)
//! - Plate centre offset: PPO3, PPO6 (mm)
//! - Corner pixel: CNPIX1, CNPIX2
//! - Polynomial distortion: AMDX1..20, AMDY1..20
//!
//! Always FK5 J2000, TAN projection, 2D, no rotation.
//! Mirrors C `DSSToStore`/`DSSFromStore` (fitschan.c ~line 9066/9364).

use super::card_list::CardList;
use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};

/// Read DSS plate solution keywords into a FitsStore.
pub(crate) fn read_from_cards(cards: &CardList) -> AstResult<FitsStore> {
    let mut store = FitsStore::new(2);

    // Plate centre RA in degrees.
    let pltrah = cards.get_float("PLTRAH")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS: missing PLTRAH"))?;
    let pltram = cards.get_float("PLTRAM").unwrap_or(0.0);
    let pltras = cards.get_float("PLTRAS").unwrap_or(0.0);
    let ra_deg = 15.0 * (pltrah + pltram / 60.0 + pltras / 3600.0);

    // Plate centre DEC in degrees.
    let pltdecd = cards.get_float("PLTDECD").unwrap_or(0.0);
    let pltdecm = cards.get_float("PLTDECM").unwrap_or(0.0);
    let pltdecs = cards.get_float("PLTDECS").unwrap_or(0.0);
    let decsn = cards.get_string("PLTDECSN").unwrap_or_else(|| "+".to_string());
    let sign = if decsn.trim().starts_with('-') { -1.0 } else { 1.0 };
    let dec_deg = sign * (pltdecd + pltdecm / 60.0 + pltdecs / 3600.0);

    // Pixel sizes (microns).
    let xpixelsz = cards.get_float("XPIXELSZ")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS: missing XPIXELSZ"))?;
    let ypixelsz = cards.get_float("YPIXELSZ")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS: missing YPIXELSZ"))?;

    // Plate centre cartesian offsets (mm).
    let ppo3 = cards.get_float("PPO3")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS: missing PPO3"))?;
    let ppo6 = cards.get_float("PPO6")
        .ok_or_else(|| AstError::new(ErrorCode::Bdfts, "DSS: missing PPO6"))?;

    // Corner pixel (default 0).
    let cnpix1 = cards.get_float("CNPIX1").unwrap_or(0.0);
    let cnpix2 = cards.get_float("CNPIX2").unwrap_or(0.0);

    // CRPIX: reference pixel from PPO and pixel size.
    // PPO3/PPO6 are in microns (same unit as XPIXELSZ/YPIXELSZ).
    store.crpix[0] = Some((ppo3 / xpixelsz) - cnpix1 + 0.5);
    store.crpix[1] = Some((ppo6 / ypixelsz) - cnpix2 + 0.5);

    // CRVAL: plate centre.
    store.crval[0] = Some(ra_deg);
    store.crval[1] = Some(dec_deg);

    // CDELT: pixel scale from pixel size and plate scale.
    // Pixel size in mm = XPIXELSZ/1000. AMDX1 is plate scale in arcsec/mm.
    // CDELT = pixel_size_mm * plate_scale_arcsec_per_mm / 3600.
    let amdx1 = cards.get_float("AMDX1").unwrap_or(0.0);
    let amdy1 = cards.get_float("AMDY1").unwrap_or(0.0);
    let plate_scale = if amdx1 > 0.0 { amdx1 } else if amdy1 > 0.0 { amdy1 } else {
        cards.get_float("PLTSCALE").unwrap_or(67.0)
    };
    store.cdelt[0] = Some(-0.001 * xpixelsz * plate_scale / 3600.0);
    store.cdelt[1] = Some(0.001 * ypixelsz * plate_scale / 3600.0);

    // CTYPE: always TAN for DSS.
    store.ctype[0] = Some("RA---TAN".to_string());
    store.ctype[1] = Some("DEC--TAN".to_string());

    // Always FK5 J2000.
    store.radesys = Some("FK5".to_string());
    store.equinox = Some(2000.0);
    store.lonpole = Some(180.0);

    Ok(store)
}

/// Write FitsStore to DSS plate solution keywords.
///
/// Only valid for 2D FK5 J2000 TAN projections with no rotation.
pub(crate) fn write_to_cards(store: &FitsStore, cards: &mut CardList) -> AstResult<()> {
    if store.naxis != 2 {
        return Err(AstError::new(ErrorCode::Nodef, "DSS requires exactly 2 axes"));
    }

    let ra_deg = store.crval[0].unwrap_or(0.0);
    let dec_deg = store.crval[1].unwrap_or(0.0);

    // Convert RA to hours/minutes/seconds.
    let ra_hours = ra_deg / 15.0;
    let pltrah = ra_hours.floor();
    let ra_rem = (ra_hours - pltrah) * 60.0;
    let pltram = ra_rem.floor();
    let pltras = (ra_rem - pltram) * 60.0;

    // Convert DEC to degrees/minutes/seconds.
    let dec_sign = if dec_deg < 0.0 { "-" } else { "+" };
    let dec_abs = dec_deg.abs();
    let pltdecd = dec_abs.floor();
    let dec_rem = (dec_abs - pltdecd) * 60.0;
    let pltdecm = dec_rem.floor();
    let pltdecs = (dec_rem - pltdecm) * 60.0;

    // Pixel sizes: compute from CDELT.
    // |CDELT| = 0.001 * pixelsz * plate_scale / 3600
    // Assume plate_scale ≈ 67.2 arcsec/mm (typical DSS).
    let cdelt0 = store.cdelt[0].unwrap_or(-0.001);
    let cdelt1 = store.cdelt[1].unwrap_or(0.001);
    let plate_scale = 67.2; // arcsec/mm — nominal DSS value
    let xpixelsz = (cdelt0.abs() * 3600.0 / plate_scale) * 1000.0;
    let ypixelsz = (cdelt1.abs() * 3600.0 / plate_scale) * 1000.0;

    // CNPIX defaults to 1.
    let cnpix1 = 1.0;
    let cnpix2 = 1.0;

    // PPO from CRPIX.
    let crpix0 = store.crpix[0].unwrap_or(1.0);
    let crpix1 = store.crpix[1].unwrap_or(1.0);
    let ppo3 = xpixelsz * (crpix0 + cnpix1 - 0.5);
    let ppo6 = ypixelsz * (crpix1 + cnpix2 - 0.5);

    // Write DSS keywords.
    cards.set_float("CNPIX1", cnpix1, "X corner");
    cards.set_float("CNPIX2", cnpix2, "Y corner");
    cards.set_float("PPO3", ppo3, "Plate centre X offset");
    cards.set_float("PPO6", ppo6, "Plate centre Y offset");
    cards.set_float("XPIXELSZ", xpixelsz, "X pixel size (microns)");
    cards.set_float("YPIXELSZ", ypixelsz, "Y pixel size (microns)");
    cards.set_float("PLTRAH", pltrah, "Plate centre RA hours");
    cards.set_float("PLTRAM", pltram, "Plate centre RA minutes");
    cards.set_float("PLTRAS", pltras, "Plate centre RA seconds");
    cards.set_float("PLTDECD", pltdecd, "Plate centre DEC degrees");
    cards.set_float("PLTDECM", pltdecm, "Plate centre DEC arcmin");
    cards.set_float("PLTDECS", pltdecs, "Plate centre DEC arcsec");
    cards.set_string("PLTDECSN", dec_sign, "DEC sign");
    cards.set_float("PLTSCALE", plate_scale, "Plate scale (arcsec/mm)");
    cards.set_float("AMDX1", plate_scale, "Plate solution X coefficient");
    cards.set_float("AMDY1", plate_scale, "Plate solution Y coefficient");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards_from(text: &str) -> CardList {
        CardList::from_header(text).unwrap()
    }

    #[test]
    fn read_dss_plate_centre() {
        let cards = cards_from(
            "PLTRAH  = 10.0\nPLTRAM  = 47.0\nPLTRAS  = 57.3\n\
             PLTDECD = 60.0\nPLTDECM = 15.0\nPLTDECS = 51.85\n\
             PLTDECSN= '-'\nXPIXELSZ= 25.28\nYPIXELSZ= 25.28\n\
             PPO3    = -18825.9\nPPO6    = -22216.5\n\
             CNPIX1  = 1.0\nCNPIX2  = 1.0\n\
             AMDX1   = 67.23\nAMDY1   = 67.23\nPLTSCALE= 67.23\nEND"
        );
        let store = read_from_cards(&cards).unwrap();

        // RA = 15 * (10 + 47/60 + 57.3/3600) ≈ 161.989°
        let ra = store.crval[0].unwrap();
        assert!((ra - 161.989).abs() < 0.01);

        // DEC = -(60 + 15/60 + 51.85/3600) ≈ -60.264°
        let dec = store.crval[1].unwrap();
        assert!((dec - (-60.264)).abs() < 0.01);

        // Always FK5 J2000.
        assert_eq!(store.radesys, Some("FK5".to_string()));
        assert_eq!(store.equinox, Some(2000.0));
        assert_eq!(store.ctype[0], Some("RA---TAN".to_string()));
    }

    #[test]
    fn write_dss_ra_decomposition() {
        let mut store = FitsStore::new(2);
        store.crval[0] = Some(161.989); // RA ≈ 10h 47m 57.3s
        store.crval[1] = Some(-60.264); // DEC
        store.crpix[0] = Some(100.0);
        store.crpix[1] = Some(100.0);
        store.cdelt[0] = Some(-0.000469); // typical DSS
        store.cdelt[1] = Some(0.000469);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());

        let mut cards = CardList::new();
        write_to_cards(&store, &mut cards).unwrap();

        // PLTRAH should be 10 (hours).
        let pltrah = cards.get_float("PLTRAH").unwrap();
        assert!((pltrah - 10.0).abs() < 0.01);
        // PLTDECSN should be "-".
        let sign = cards.get_string("PLTDECSN").unwrap();
        assert_eq!(sign.trim(), "-");
    }
}
```

- [ ] **Step 2: Register module and verify**

- [ ] **Step 3: Commit**

---

### Task 7: Wire All Encodings into FitsChan

**Files:**
- Modify: `crates/starlink-ast/src/fitschan/mod.rs`

Wire the guess dispatcher and all encoding modules into the `read()` and `write()` methods.

- [ ] **Step 1: Register all encoding modules in mod.rs**

Add after existing module declarations:
```rust
pub(crate) mod enc_aips;
pub(crate) mod enc_class;
pub(crate) mod enc_dss;
pub(crate) mod enc_iraf;
pub(crate) mod enc_pc;
pub(crate) mod guess_encoding;
```

- [ ] **Step 2: Update FitsChan::read() to handle all encodings**

Replace the read() match:
```rust
    fn read(&mut self) -> AstResult<Option<AnyObject>> {
        self.warnings.clear();

        // If encoding is FitsWcs but could be another format, auto-detect.
        let encoding = self.encoding;

        match encoding {
            Encoding::Native => {
                let text = match self.extract_native_text() {
                    Some(t) => t,
                    None => return Ok(None),
                };
                let node = serial::native_read::parse_native(&text)?;
                let obj = serial::any_object_from_node(&node)?;
                Ok(Some(obj))
            }
            Encoding::FitsWcs | Encoding::FitsPc => {
                let store = wcs_keywords::extract_wcs_keywords(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::FitsIraf => {
                let store = enc_iraf::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::FitsAips | Encoding::FitsAipsPP => {
                let store = enc_aips::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::FitsClass => {
                let store = enc_class::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
            Encoding::Dss => {
                let store = enc_dss::read_from_cards(&self.cards)?;
                let obj = wcs_read::decode_wcs(&store)?;
                Ok(Some(obj))
            }
        }
    }
```

- [ ] **Step 3: Update FitsChan::write() to handle all encodings**

Replace the write() match:
```rust
    fn write(&mut self, obj: &AnyObject) -> AstResult<()> {
        self.warnings.clear();

        match self.encoding {
            Encoding::Native => {
                let node = serial::any_object_to_node(obj)?;
                let options = serial::native_write::RenderOptions {
                    comment: self.config.comment,
                    full: self.config.full,
                    indent: self.config.indent,
                };
                let text = serial::native_write::render_native_with_options(&node, &options)?;
                self.store_native_text(&text);
                Ok(())
            }
            Encoding::FitsWcs => {
                wcs_write::encode_wcs(obj, &mut self.cards, self.cd_matrix)
            }
            Encoding::FitsIraf => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, |store, cards| {
                    enc_iraf::write_to_cards(store, cards)
                })
            }
            Encoding::FitsPc => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, |store, cards| {
                    enc_pc::write_to_cards(store, cards)
                })
            }
            Encoding::FitsAips => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, |store, cards| {
                    enc_aips::write_to_cards(store, cards)
                })
            }
            Encoding::FitsAipsPP => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, |store, cards| {
                    enc_aips::write_to_cards_pp(store, cards)
                })
            }
            Encoding::FitsClass => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, |store, cards| {
                    enc_class::write_to_cards(store, cards)
                })
            }
            Encoding::Dss => {
                wcs_write::encode_wcs_to_store(obj, &mut self.cards, |store, cards| {
                    enc_dss::write_to_cards(store, cards)
                })
            }
        }
    }
```

- [ ] **Step 4: Add encode_wcs_to_store helper in wcs_write.rs**

This helper decomposes the FrameSet into a FitsStore and then delegates card writing to the encoding-specific function:

```rust
/// Encode a FrameSet via FitsStore with a custom card writer.
///
/// Used by alternative encodings (IRAF, PC, AIPS, CLASS, DSS) that share
/// the FrameSet→FitsStore decomposition but write different keywords.
pub(crate) fn encode_wcs_to_store(
    obj: &AnyObject,
    cards: &mut CardList,
    writer: impl FnOnce(&FitsStore, &mut CardList) -> AstResult<()>,
) -> AstResult<()> {
    let frameset = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "FitsWcs encoding requires a FrameSet object",
            ));
        }
    };

    let store = store_from_frameset(frameset)?;
    writer(&store, cards)
}
```

Also make `store_from_frameset` `pub(crate)` if it isn't already.

- [ ] **Step 5: Add a guess() method to FitsChan**

```rust
    /// Auto-detect the encoding format from the current header cards.
    ///
    /// Examines keyword signatures to determine which WCS encoding the
    /// header uses. Updates the encoding attribute accordingly.
    pub fn guess_encoding(&mut self) -> Encoding {
        let enc = guess_encoding::guess_encoding(&self.cards);
        self.encoding = enc;
        enc
    }
```

- [ ] **Step 6: Verify build**

Run: `cargo build -p starlink-ast`
Expected: compiles

- [ ] **Step 7: Commit**

---

### Task 8: Integration Tests

**Files:**
- Create: `crates/starlink-ast/tests/test_fitschan_encodings.rs`
- Copy test fixtures from `reference/starlink-ast/ast_tester/` to `crates/starlink-ast/tests/fixtures/fits/headers/`

- [ ] **Step 1: Copy test fixtures**

Copy these files:
- `reference/starlink-ast/ast_tester/velo-aips.head` → `crates/starlink-ast/tests/fixtures/fits/headers/velo-aips.head`
- `reference/starlink-ast/ast_tester/pc-sparse.head` → `crates/starlink-ast/tests/fixtures/fits/headers/pc-sparse.head`
- `reference/starlink-ast/ast_tester/dss.dss` → `crates/starlink-ast/tests/fixtures/fits/headers/dss.dss`
- `reference/starlink-ast/ast_tester/class-write.head` → `crates/starlink-ast/tests/fixtures/fits/headers/class-write.head`
- `reference/starlink-ast/ast_tester/serpens.head` → `crates/starlink-ast/tests/fixtures/fits/headers/serpens.head`

- [ ] **Step 2: Write integration tests**

```rust
//! Integration tests for alternative FITS WCS encodings.

use starlink_ast::channel::Channel;
use starlink_ast::fitschan::{Encoding, FitsChan};

fn load_fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/fits/headers/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} should be readable"))
}

// --- Guess encoding ---

#[test]
fn guess_encoding_iraf() {
    let text = load_fixture("serpens.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
    assert_eq!(enc, Encoding::FitsIraf);
}

#[test]
fn guess_encoding_pc() {
    let text = load_fixture("pc-sparse.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
    assert_eq!(enc, Encoding::FitsPc);
}

#[test]
fn guess_encoding_aips() {
    let text = load_fixture("velo-aips.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
    assert_eq!(enc, Encoding::FitsAips);
}

#[test]
fn guess_encoding_class() {
    let text = load_fixture("class-write.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
    assert_eq!(enc, Encoding::FitsClass);
}

#[test]
fn guess_encoding_dss() {
    let text = load_fixture("dss.dss");
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
    assert_eq!(enc, Encoding::Dss);
}

// --- Read tests ---

#[test]
fn read_iraf_produces_frameset() {
    let text = load_fixture("serpens.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsIraf);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "IRAF read should produce a FrameSet");
}

#[test]
fn read_pc_produces_frameset() {
    let text = load_fixture("pc-sparse.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsPc);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "FITS-PC read should produce a FrameSet");
}

#[test]
fn read_aips_spectral() {
    let text = load_fixture("velo-aips.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsAips);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "AIPS read should produce a FrameSet");
}

#[test]
fn read_class_produces_frameset() {
    let text = load_fixture("class-write.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsClass);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "CLASS read should produce a FrameSet");
}

#[test]
fn read_dss_produces_frameset() {
    let text = load_fixture("dss.dss");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::Dss);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "DSS read should produce a FrameSet");
}
```

- [ ] **Step 3: Run tests and fix issues**

Run: `cargo test -p starlink-ast --test test_fitschan_encodings`
Expected: all tests pass

- [ ] **Step 4: Commit**

---

### Task 9: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test -p starlink-ast`
Expected: all tests pass

- [ ] **Step 2: Clippy and fmt**

Run: `cargo clippy -p starlink-ast -- -D warnings && cargo fmt --check -p starlink-ast`
Expected: clean

- [ ] **Step 3: Fix any issues and commit**

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p starlink-ast` — all tests pass
3. Encoding auto-detection correctly identifies: Native, FITS-WCS, FITS-IRAF, FITS-PC, FITS-AIPS, FITS-AIPS++, FITS-CLASS, DSS
4. Each encoding can read its fixture header into a FrameSet
5. AIPS CROTA → PC matrix conversion is correct
6. AIPS spectral CTYPE translation (VELO-LSR→VRAD+LSRK) works
7. CLASS offset frequency (CRVAL + RESTFREQ) correction works
8. DSS plate center H/M/S conversion is correct
9. IRAF NCP→SIN and GLS→SFL translations work
10. PC 3-digit format (PC001001) parsing works
11. All existing Phase 2/3 tests still pass
12. `cargo clippy` clean, `cargo fmt` clean

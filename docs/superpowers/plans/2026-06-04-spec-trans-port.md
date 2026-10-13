# SpecTrans Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port C `fitschan.c::SpecTrans` line-by-line so all FITS-* readers consume normalised cards via a single pre-pass, replacing the scattered, partial translations spread across `enc_pc.rs`, `enc_iraf.rs`, `enc_aips.rs`, and per-call-site fallbacks. The port unblocks the FITS-PC `Cnxxx` alt-axis fixtures and provides a single, auditable place for the 24 non-standard-WCS translations C performs.

**Architecture:** A new `crates/ast-core/src/fitschan/spec_trans.rs` module holds the entry point `pub(crate) fn spec_trans(cards: &mut CardList, encoding: Encoding)` that walks the card list, identifies non-standard keywords, and writes the standard equivalents into the same CardList (matching C's "create a returned FitsChan with translated cards while marking originals as used"). All FITS-encoding read paths call `spec_trans` once, before alt-axis detection or store extraction. Non-standard keywords are marked as "used" so they don't round-trip back into outputs. The module is one file with one private function per translation (24 total), keyed by section comments mirroring the C source line ranges.

**Tech Stack:** Rust 2024, the existing `CardList` API (`set_string`, `set_float`, `find`, `get_float`, `get_string`), the `Encoding` enum.

---

## Background

C `SpecTrans` (fitschan.c:30950-32667) runs once at the start of every FITS read and performs 24 distinct keyword translations. The current Rust port has a scattered subset of these in per-encoder `read_from_cards` functions — for example, `enc_pc.rs` checks RADECSYS-then-RADESYS inline, `enc_iraf.rs` has its own CTYPE translator, `enc_aips.rs` has its own translation table. This duplication has three problems:

1. **Each encoder reimplements a subset.** Translations applied for one encoding (e.g. RESTFREQ→RESTFRQ in enc_pc) are missed for others.
2. **Alt-axis support is structurally blocked** because `Cnxxx` cards must be rewritten to `*A..*Z` form *before* `detect_alt_letters` runs. With the rewrites scattered downstream of detection, FITS-PC alt planes are silently dropped.
3. **Drift from C is invisible.** Each scattered fragment makes its own choices about when (and whether) to apply a translation; bugs surface as "the C library does X for fixture Y" rather than as a single audit trail.

The port consolidates the translations into one module that mirrors the C source structure. The per-encoder cleanups happen in tasks 25-28 (after the new pipeline is proven against the existing wcsconv suite).

## Spec Coverage Mapping

The 24 SpecTrans translations from `fitschan.c:30965-31073`:

| # | C section | Translation | Task |
|---|-----------|-------------|------|
| 1 | `RADECSYS keywords` | `RADECSYS` → `RADESYS` | Task 3 |
| 2 | `LONGPOLE keywords` | `LONGPOLE` → `LONPOLE` | Task 4 |
| 3 | `CDjjjiii / CDj_i` | `CDjjjiii` → `CDj_i` (PC matrix, 3-digit form) | Task 5 |
| 4 | `CROTA` | `CROTAj` → `PCi_j` (with CDELT preserved) | Task 6 |
| 5 | `PROJP keywords` | `PROJPi` → `PV<axlat>_i` | Task 7 |
| 6 | `CmVALi/PIXi/YPEi/NITi/ELTi` | `Cnxxx*` → `xxxxx*A..*Z` (FITS-PC alt-axes) | Task 8 |
| 7 | `EQUINOX string values` | `EQUINOX = "B1995.0"` → `EQUINOX = 1995.0` (Julian) | Task 9 |
| 8 | `EPOCH keywords` | `EPOCH` → `EQUINOX` (when EQUINOX absent) | Task 10 |
| 9 | `DATE-OBS keywords` | `DATE-OBS` → `MJD-OBS` (when MJD-OBS absent) | Task 11 |
| 10 | (in EPOCH section) | EPOCH/EQUINOX = 0 → B1950 | Task 10 |
| 11 | AIPS NCP/GLS | `xxx-NCP` → `xxx-SIN` + PV; `xxx-GLS` → `xxx-SFL` | Task 12 |
| 12 | IRAF ZPX | `xxx-ZPX` → `xxx-ZPN` + WAT-derived PV | Task 13 |
| 13 | IRAF TNX | `xxx-TNX` → `xxx-TPN` + WAT-derived PV | Task 14 |
| 14 | IRAF mini-WCS | Strip `WAT/MWCS/CD/CDELT[A-Z]/CRVAL[A-Z]` mini-WCS keys | Task 15 |
| 15 | QV → PV | `QVi_m` → `PVi_m` (AUTOASTROM and -TAB) | Task 16 |
| 16 | RESTFREQ | `RESTFREQ` → `RESTFRQ` | Task 17 |
| 17 | early paper-III spectral | `xxx-WAV/-FRQ/-VEL` → `xxx-X2P` form | Task 18 |
| 18 | AIPS spectral CTYPE | `VELO-LSR` etc → `VRAD-LSRK` etc | Task 19 |
| 19 | OBSRA / OBSDEC | Synthesise RA/DEC celestial axes from `OBSRA`/`OBSDEC` | Task 20 |
| 20 | CUNIT case-insensitive | "Hz", "Angstrom", "km/s", "M/S" → canonical form | Task 21 |
| 21 | FITS-CLASS specific | Misc CLASS rewrites | Task 22 |
| 22 | SAO TAN with COi_j | `xxx-TAN` + `COi_j` → `xxx-TPN` + `PVi_m` | Task 23 |
| 23 | LAMBDA → WAVE | `CTYPE = "LAMBDA"` → `CTYPE = "WAVE"` | Task 24 |
| 24 | TAN/TPV polynomial | TAN+`PolyTan` or TPV (SCAMP) → TPN | Task 25 |

Tasks 1-2 set up the module skeleton + helpers. Tasks 26-28 wire in callers and remove duplicate code from `enc_*.rs`.

## Test Strategy

Each translation gets a unit test in `crates/ast-core/src/fitschan/spec_trans.rs` (cfg(test) block) using small synthetic CardLists. Each task's tests exercise the specific translation in isolation. A final integration task (29) verifies the full wcsconv suite gains tests as expected (Bucket A: FITS-PC alt-axis = 8 fixtures; CLASS bucket; AIPS variants; etc).

## File Structure

- **Create:** `crates/ast-core/src/fitschan/spec_trans.rs` — the new module, ~1500 lines covering all 24 translations
- **Modify:** `crates/ast-core/src/fitschan/mod.rs` — register the module, call `spec_trans` from each FITS encoding's read path
- **Modify:** `crates/ast-core/src/fitschan/enc_pc.rs` — remove RADECSYS-fallback and RESTFREQ-fallback (tasks 26)
- **Modify:** `crates/ast-core/src/fitschan/enc_iraf.rs` — remove inline CTYPE translator and RESTFREQ-fallback (task 27)
- **Modify:** `crates/ast-core/src/fitschan/enc_aips.rs` — remove inline CTYPE translator and RESTFREQ-fallback (task 28)

---

### Task 1: Module skeleton + entry point

**Files:**
- Create: `crates/ast-core/src/fitschan/spec_trans.rs`
- Modify: `crates/ast-core/src/fitschan/mod.rs`

- [ ] **Step 1: Write the failing integration test**

```rust
// In crates/ast-core/src/fitschan/spec_trans.rs (new file, cfg(test) block):

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::Encoding;
    use crate::fitschan::card_list::CardList;

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
        // Standard FITS-WCS keywords pass through unchanged.
        let after = cards.iter().count();
        assert_eq!(before, after);
        assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::spec_trans_is_a_no_op_when_all_keywords_are_already_standard 2>&1 | tail -15`
Expected: FAIL with "could not find function `spec_trans`".

- [ ] **Step 3: Write the entry-point stub**

```rust
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

use super::Encoding;
use super::card_list::CardList;

/// Apply all SpecTrans translations to `cards` in place.
///
/// `encoding` selects which translations apply: a few are gated to
/// specific encodings (e.g. `Cnxxx` → `*A` is FITS-PC / FITS-IRAF
/// only; AIPS spectral CTYPE rewrites are AIPS-only). Translations
/// not gated by encoding apply to all FITS encodings uniformly.
pub(crate) fn spec_trans(cards: &mut CardList, encoding: Encoding) {
    let _ = cards;
    let _ = encoding;
    // Translations are added one per task in this module's order.
}

#[cfg(test)]
mod tests { /* ... as in step 1 ... */ }
```

- [ ] **Step 4: Register the module**

In `crates/ast-core/src/fitschan/mod.rs`, add to the module list (near `mod wcs_keywords;`):

```rust
pub(crate) mod spec_trans;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo build -p ast-core 2>&1 | tail -3 && cargo test -p ast-core --lib spec_trans::tests::spec_trans_is_a_no_op_when_all_keywords_are_already_standard 2>&1 | tail -5`
Expected: build clean, test PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs crates/ast-core/src/fitschan/mod.rs
git commit -m "fitschan: introduce spec_trans module skeleton"
```

---

### Task 2: Card-rename helper

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

- [ ] **Step 1: Write the failing test**

In `spec_trans.rs` test block:

```rust
#[test]
fn rename_keyword_moves_value_and_comment_to_new_name() {
    let mut cards = CardList::from_header(
        "RADECSYS= 'FK5     '           / Reference frame\n\
         END",
    )
    .unwrap();
    rename_keyword(&mut cards, "RADECSYS", "RADESYS");
    assert!(cards.find("RADECSYS").is_none());
    assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
}

#[test]
fn rename_keyword_no_op_when_source_absent() {
    let mut cards = CardList::from_header("END").unwrap();
    rename_keyword(&mut cards, "RADECSYS", "RADESYS");
    assert!(cards.find("RADECSYS").is_none());
    assert!(cards.find("RADESYS").is_none());
}

#[test]
fn rename_keyword_no_op_when_destination_already_present() {
    // C SpecTrans: don't overwrite an existing standard value.
    let mut cards = CardList::from_header(
        "RADECSYS= 'FK4     '           / Old\n\
         RADESYS = 'FK5     '           / New\n\
         END",
    )
    .unwrap();
    rename_keyword(&mut cards, "RADECSYS", "RADESYS");
    // RADECSYS is consumed; RADESYS keeps its existing value.
    assert!(cards.find("RADECSYS").is_none());
    assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans:: 2>&1 | tail -10`
Expected: FAIL with "no function `rename_keyword`".

- [ ] **Step 3: Implement `rename_keyword` and a string/float rename helper**

```rust
/// Move a keyword's value and comment from `from` to `to`, deleting
/// the original. If `to` is already present, drop the source value
/// (matches C SpecTrans, which only translates when the destination
/// is absent — see e.g. fitschan.c:31171 for the EPOCH→EQUINOX gate).
///
/// Both string and float-valued cards are handled. Returns true if a
/// rename occurred.
pub(super) fn rename_keyword(cards: &mut CardList, from: &str, to: &str) -> bool {
    if cards.find(to).is_some() {
        // Destination present — discard source per C semantics.
        cards.remove(from);
        return false;
    }
    if let Some(value) = cards.get_float(from) {
        let comment = cards
            .find(from)
            .and_then(|c| c.comment.clone())
            .unwrap_or_default();
        cards.set_float(to, value, &comment);
        cards.remove(from);
        return true;
    }
    if let Some(value) = cards.get_string(from) {
        let comment = cards
            .find(from)
            .and_then(|c| c.comment.clone())
            .unwrap_or_default();
        cards.set_string(to, &value, &comment);
        cards.remove(from);
        return true;
    }
    false
}
```

(If `CardList::remove` does not exist, add a `pub(crate) fn remove(&mut self, keyword: &str)` to `card_list.rs` that filters out matching entries; mention this in the commit message.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans:: 2>&1 | tail -10`
Expected: 4 tests PASS (the no-op + 3 rename tests).

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs crates/ast-core/src/fitschan/card_list.rs
git commit -m "fitschan/spec_trans: rename_keyword helper"
```

---

### Task 3: Translation #1 — RADECSYS → RADESYS

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn translates_radecsys_to_radesys() {
    let mut cards = CardList::from_header(
        "RADECSYS= 'FK5     '           / Old-style frame\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert!(cards.find("RADECSYS").is_none());
    assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
}

#[test]
fn keeps_radesys_when_both_present() {
    let mut cards = CardList::from_header(
        "RADECSYS= 'FK4     '\n\
         RADESYS = 'FK5     '\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert_eq!(cards.get_string("RADESYS"), Some("FK5".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_radecsys 2>&1 | tail -10`
Expected: FAIL — RADECSYS is still present.

- [ ] **Step 3: Implement the translation**

In `spec_trans`:

```rust
pub(crate) fn spec_trans(cards: &mut CardList, encoding: Encoding) {
    let _ = encoding;
    // Translation 1 (fitschan.c:31236-31242): RADECSYS → RADESYS.
    rename_keyword(cards, "RADECSYS", "RADESYS");
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans:: 2>&1 | tail -10`
Expected: all spec_trans tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #1 RADECSYS -> RADESYS"
```

---

### Task 4: Translation #2 — LONGPOLE → LONPOLE

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn translates_longpole_to_lonpole() {
    let mut cards = CardList::from_header(
        "LONGPOLE=                180.0 / Old-style native lon\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert!(cards.find("LONGPOLE").is_none());
    assert_eq!(cards.get_float("LONPOLE"), Some(180.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_longpole 2>&1 | tail -5`
Expected: FAIL.

- [ ] **Step 3: Implement**

Append to `spec_trans` body:

```rust
    // Translation 2 (fitschan.c:31247-31253): LONGPOLE → LONPOLE.
    rename_keyword(cards, "LONGPOLE", "LONPOLE");
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans:: 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #2 LONGPOLE -> LONPOLE"
```

---

### Task 5: Translation #3 — CDjjjiii → CDj_i

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

C source: fitschan.c:31259-31317. The 6-digit `CDjjjiii` form has `j` and `i` as zero-padded 3-digit fields. We rewrite to `CDj_i` (the standard form). Per-axis-version: only happens for primary plane (no version letter).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn translates_cdjjjiii_to_cdj_i() {
    let mut cards = CardList::from_header(
        "CD001001=                  1.0\n\
         CD001002=                  0.0\n\
         CD002001=                  0.0\n\
         CD002002=                  1.0\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert!(cards.find("CD001001").is_none());
    assert_eq!(cards.get_float("CD1_1"), Some(1.0));
    assert_eq!(cards.get_float("CD2_2"), Some(1.0));
}

#[test]
fn cdjjjiii_does_not_overwrite_cdj_i() {
    let mut cards = CardList::from_header(
        "CD001001=                  1.0\n\
         CD1_1   =                  2.0\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    // The CDj_i form wins.
    assert_eq!(cards.get_float("CD1_1"), Some(2.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_cdjjjiii 2>&1 | tail -10`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
    // Translation 3 (fitschan.c:31259-31317): CDjjjiii → CDj_i.
    // Iterate the 9×9 grid that fits in 6-digit indexed names.
    for j in 1..=9 {
        for i in 1..=9 {
            let from = format!("CD{j:03}{i:03}");
            let to = format!("CD{j}_{i}");
            rename_keyword(cards, &from, &to);
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_cdjjjiii 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #3 CDjjjiii -> CDj_i"
```

---

### Task 6: Translation #4 — CROTAj → PCi_j

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

C source: fitschan.c:31304-31482. CROTA encodes celestial-axis rotation as a single angle. The rewrite produces a 2×2 PC matrix using cos/sin of CROTA on the celestial axes, leaving non-celestial axes as identity, plus the original CDELT values intact.

This translation requires identifying which axes are celestial (lon/lat) — use the standard CTYPE prefixes (`RA---`, `DEC--`, `xLON`, `xLAT`, `xxLN`, `xxLT`, `AZ--`, `EL--`).

The rewrite happens per-version-letter — but only when a celestial pair is present and at least one CROTA card exists.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn translates_crota_to_pc_matrix() {
    // CROTA2 = 30°: PC1_1 = cos(30°), PC1_2 = -sin(30°)·CDELT2/CDELT1, etc.
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
    let sin30 = 30.0_f64.to_radians().sin();
    assert!((cards.get_float("PC1_1").unwrap() - cos30).abs() < 1e-10);
    assert!((cards.get_float("PC2_2").unwrap() - cos30).abs() < 1e-10);
    // CDELT preserved.
    assert_eq!(cards.get_float("CDELT1"), Some(-1.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_crota 2>&1 | tail -10`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
fn axis_is_celestial(ctype: &str) -> bool {
    let nc = ctype.len();
    if nc >= 4 && (
        &ctype[..4] == "RA--"
        || &ctype[..4] == "DEC-"
        || &ctype[..4] == "AZ--"
        || &ctype[..4] == "EL--"
    ) {
        return true;
    }
    if nc > 1 {
        let pat3 = &ctype[1..(1+3).min(nc)];
        if pat3.eq("LON") || pat3.eq("LAT") { return true; }
    }
    if nc > 2 {
        let pat2 = &ctype[2..(2+2).min(nc)];
        if pat2.eq("LN") || pat2.eq("LT") { return true; }
    }
    false
}

fn celestial_axes(cards: &CardList, s: char) -> Option<(usize, usize)> {
    let mut axlon = None;
    let mut axlat = None;
    for j in 1..=9 {
        let key = if s == ' ' { format!("CTYPE{j}") } else { format!("CTYPE{j}{s}") };
        let Some(ctype) = cards.get_string(&key) else { continue };
        let ctype = ctype.trim();
        if ctype.starts_with("RA--") || ctype.starts_with("AZ--")
           || (ctype.len() > 1 && &ctype[1..(4.min(ctype.len()))] == "LON")
           || (ctype.len() > 2 && &ctype[2..(4.min(ctype.len()))] == "LN") {
            axlon = Some(j);
        } else if ctype.starts_with("DEC-") || ctype.starts_with("EL--")
           || (ctype.len() > 1 && &ctype[1..(4.min(ctype.len()))] == "LAT")
           || (ctype.len() > 2 && &ctype[2..(4.min(ctype.len()))] == "LT") {
            axlat = Some(j);
        }
    }
    axlon.zip(axlat)
}

fn translate_crota(cards: &mut CardList) {
    // C: fitschan.c:31304-31482. Walk per-version-letter.
    for s in std::iter::once(' ').chain('A'..='Z') {
        let crota_key = |j: usize| {
            if s == ' ' { format!("CROTA{j}") } else { format!("CROTA{j}{s}") }
        };
        // Find the CROTA card on the latitude axis.
        let Some((axlon, axlat)) = celestial_axes(cards, s) else { continue };
        let lat_crota = cards.get_float(&crota_key(axlat));
        let lon_crota = cards.get_float(&crota_key(axlon));
        let crota_deg = match (lat_crota, lon_crota) {
            (Some(v), _) => v,
            (None, Some(v)) => -v,  // CROTA on lon means opposite sign
            _ => continue,
        };
        let cos = crota_deg.to_radians().cos();
        let sin = crota_deg.to_radians().sin();
        // Read CDELT values (default 1.0) for the celestial pair to
        // build the off-diagonal PC.
        let cdelt_key = |j: usize| {
            if s == ' ' { format!("CDELT{j}") } else { format!("CDELT{j}{s}") }
        };
        let cdelt_lon = cards.get_float(&cdelt_key(axlon)).unwrap_or(1.0);
        let cdelt_lat = cards.get_float(&cdelt_key(axlat)).unwrap_or(1.0);
        let pc_key = |i: usize, j: usize| {
            if s == ' ' { format!("PC{i}_{j}") } else { format!("PC{i}_{j}{s}") }
        };
        // Mirrors fitschan.c:31368-31402.
        cards.set_float(&pc_key(axlon, axlon), cos, "PC matrix element");
        cards.set_float(&pc_key(axlat, axlat), cos, "PC matrix element");
        cards.set_float(&pc_key(axlon, axlat), -sin * cdelt_lat / cdelt_lon, "PC matrix element");
        cards.set_float(&pc_key(axlat, axlon), sin * cdelt_lon / cdelt_lat, "PC matrix element");
        // Drop all CROTAj cards for this version.
        for j in 1..=9 {
            cards.remove(&crota_key(j));
        }
    }
}
```

Then in `spec_trans` body:

```rust
    translate_crota(cards);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans:: 2>&1 | tail -10`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #4 CROTA -> PC matrix"
```

---

### Task 7: Translation #5 — PROJPi → PV<axlat>_i

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

C source: fitschan.c:31484-31547. `PROJPi` (i = 0..9) was an early-paper-II projection-parameter syntax. The standard form is `PV<axlat>_i` — i.e. on the latitude axis.

- [ ] **Step 1: Write the failing test**

```rust
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
    assert_eq!(cards.get_float("PV2_0"), Some(0.0));
    assert_eq!(cards.get_float("PV2_1"), Some(1.0));
    assert_eq!(cards.get_float("PV2_2"), Some(-0.4));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_projp 2>&1 | tail -10`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
fn translate_projp(cards: &mut CardList) {
    // C: fitschan.c:31484-31547. PROJPi → PV<axlat>_i (primary version
    // only; alt versions used CROTAj-style historical syntax that
    // never had PROJP equivalents).
    let Some((_, axlat)) = celestial_axes(cards, ' ') else { return };
    for i in 0..=9 {
        let from = format!("PROJP{i}");
        let to = format!("PV{axlat}_{i}");
        rename_keyword(cards, &from, &to);
    }
}
```

In `spec_trans`: `translate_projp(cards);`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_projp 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #5 PROJP -> PV"
```

---

### Task 8: Translation #6 — Cnxxx (FITS-PC alt-axis) → *A..*Z

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

C source: fitschan.c:31562-31722. The FITS-PC alternate-axis convention uses the 8-character forms `CmYPEi`, `CmVALi`, `CmPIXi`, `CmNITi`, `CmELTi` where `m` is a digit 1..=9 indicating alt-A..alt-I. We rewrite these to `CTYPE{i}{S}` etc. with `S = 'A' + (m-1)`.

Gated to FITS-PC and FITS-IRAF encodings — other encodings ignore these keywords.

- [ ] **Step 1: Write the failing test**

```rust
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
    assert_eq!(cards.get_string("CTYPE1A"), Some("GLON-TAN".to_string()));
    assert_eq!(cards.get_string("CTYPE2A"), Some("GLAT-TAN".to_string()));
    assert_eq!(cards.get_float("CRVAL1A"), Some(101.255));
    assert_eq!(cards.get_float("CRPIX1A"), Some(50.5));
    assert_eq!(cards.get_float("CDELT1A"), Some(-0.01));
}

#[test]
fn cnxxx_translation_skipped_for_fits_wcs() {
    // FITS-WCS uses *A..*Z directly; Cnxxx is foreign and should be
    // left alone (the encoder will mark them as unused).
    let mut cards = CardList::from_header(
        "C1YPE1  = 'GLON-TAN'\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsWcs);
    assert!(cards.find("C1YPE1").is_some());
    assert!(cards.find("CTYPE1A").is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_cnxxx 2>&1 | tail -10`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
fn translate_cnxxx(cards: &mut CardList) {
    // C: fitschan.c:31562-31722. Map (m, axis_i) → (CXX, axis_i, alt_letter).
    // m=1 → 'A', m=2 → 'B', ...
    for m in 1..=9 {
        let alt = (b'A' + (m - 1) as u8) as char;
        for i in 1..=9 {
            for (cn_kind, cxx_kind) in [
                ("VAL", "CRVAL"),
                ("PIX", "CRPIX"),
                ("YPE", "CTYPE"),
                ("NIT", "CUNIT"),
                ("ELT", "CDELT"),
            ] {
                let from = format!("C{m}{cn_kind}{i}");
                let to = format!("{cxx_kind}{i}{alt}");
                rename_keyword(cards, &from, &to);
            }
        }
    }
}
```

In `spec_trans`, gate to FITS-PC / FITS-IRAF:

```rust
    if matches!(encoding, Encoding::FitsPc | Encoding::FitsIraf) {
        translate_cnxxx(cards);
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_cnxxx 2>&1 | tail -10 && cargo test -p ast-core --lib spec_trans::tests::cnxxx_translation_skipped 2>&1 | tail -5`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #6 Cnxxx alt-axis form"
```

---

### Task 9: Translation #7 — `EQUINOX = "B1995.0"` string → numeric

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

C source: fitschan.c:31724-31772. Some old FITS files store `EQUINOX = 'B1995.0'` (a string with a B/J prefix) instead of the numeric form. The B-prefix → Besselian (convert via `palEpb2d`/`palEpj` round-trip to Julian); the J-prefix → strip to a plain Julian number.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn translates_equinox_b1995_string_to_julian_number() {
    let mut cards = CardList::from_header(
        "EQUINOX = 'B1995.0'           / Besselian epoch\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    let eq = cards.get_float("EQUINOX").unwrap();
    // B1995 → MJD via palEpb2d, then J via palEpj. The Julian
    // equivalent of B1995.0 is approximately 1995.0007.
    assert!((eq - 1995.000696...).abs() < 1e-3);
}

#[test]
fn translates_equinox_j2000_string_to_plain_number() {
    let mut cards = CardList::from_header(
        "EQUINOX = 'J2000.0'\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert_eq!(cards.get_float("EQUINOX"), Some(2000.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_equinox 2>&1 | tail -10`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
fn translate_equinox_string(cards: &mut CardList) {
    // C: fitschan.c:31724-31772.
    let Some(s) = cards.get_string("EQUINOX") else { return };
    let s = s.trim();
    let (bj, num_str) = match s.chars().next() {
        Some('B') | Some('b') => ('B', &s[1..]),
        Some('J') | Some('j') => ('J', &s[1..]),
        _ => return,  // already numeric
    };
    let Ok(year) = num_str.parse::<f64>() else { return };
    let julian = if bj == 'B' {
        // B → Julian via MJD round-trip.
        crate::pal::epj(crate::pal::epb2d(year))
    } else {
        year
    };
    let comment = cards
        .find("EQUINOX")
        .and_then(|c| c.comment.clone())
        .unwrap_or_default();
    cards.remove("EQUINOX");
    cards.set_float("EQUINOX", julian, &comment);
}
```

In `spec_trans`: `translate_equinox_string(cards);`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_equinox 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #7 EQUINOX 'BYYYY' string"
```

---

### Task 10: Translations #8 & #10 — EPOCH → EQUINOX (and zero → B1950)

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

C source: fitschan.c:31774-31842. EPOCH (an old AIPS-style attribute) is renamed to EQUINOX *only when EQUINOX is absent*. Per C: also handle the case where the value is exactly 0.0 — that's an old AIPS convention meaning "B1950".

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn translates_epoch_to_equinox() {
    let mut cards = CardList::from_header(
        "EPOCH   =               1995.0\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert!(cards.find("EPOCH").is_none());
    assert_eq!(cards.get_float("EQUINOX"), Some(1995.0));
}

#[test]
fn epoch_zero_becomes_b1950_equinox() {
    let mut cards = CardList::from_header(
        "EPOCH   =                  0.0\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert_eq!(cards.get_float("EQUINOX"), Some(1950.0));
}

#[test]
fn epoch_translation_skipped_when_equinox_present() {
    let mut cards = CardList::from_header(
        "EPOCH   =               1995.0\n\
         EQUINOX =               2000.0\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert_eq!(cards.get_float("EQUINOX"), Some(2000.0));
}

#[test]
fn equinox_zero_becomes_b1950() {
    let mut cards = CardList::from_header(
        "EQUINOX =                  0.0\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert_eq!(cards.get_float("EQUINOX"), Some(1950.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_epoch 2>&1 | tail -10`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
fn translate_epoch(cards: &mut CardList) {
    // C: fitschan.c:31774-31842.
    // EPOCH → EQUINOX when EQUINOX absent.
    if cards.find("EQUINOX").is_none() {
        if let Some(v) = cards.get_float("EPOCH") {
            cards.remove("EPOCH");
            let v = if v == 0.0 { 1950.0 } else { v };
            cards.set_float("EQUINOX", v, "Epoch of reference equinox");
        }
    } else {
        // EPOCH is now redundant; mark consumed.
        cards.remove("EPOCH");
    }
    // EQUINOX = 0 → B1950 even when EPOCH wasn't involved.
    if let Some(0.0) = cards.get_float("EQUINOX") {
        let comment = cards
            .find("EQUINOX")
            .and_then(|c| c.comment.clone())
            .unwrap_or_default();
        cards.remove("EQUINOX");
        cards.set_float("EQUINOX", 1950.0, &comment);
    }
}
```

In `spec_trans`: `translate_epoch(cards);` (after `translate_equinox_string`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans:: 2>&1 | tail -15`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translations #8 + #10 EPOCH/EQUINOX zero"
```

---

### Task 11: Translation #9 — DATE-OBS → MJD-OBS

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

C source: fitschan.c:31847-31897. DATE-OBS is parsed (we already have `date_str_to_mjd` in `wcs_write/mod.rs`) and stored as MJD-OBS, but only when MJD-OBS is absent.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn translates_date_obs_to_mjd_obs() {
    let mut cards = CardList::from_header(
        "DATE-OBS= '2000-01-15T12:00:00'\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    let mjd = cards.get_float("MJD-OBS").unwrap();
    // J2000 + 14.5 days
    assert!((mjd - 51558.5).abs() < 1e-6);
    // DATE-OBS preserved (C marks it consumed but leaves the card so
    // it can pass through to output).
    assert!(cards.find("DATE-OBS").is_some());
}

#[test]
fn date_obs_translation_skipped_when_mjd_obs_present() {
    let mut cards = CardList::from_header(
        "DATE-OBS= '2000-01-15T12:00:00'\n\
         MJD-OBS =              50000.0\n\
         END",
    )
    .unwrap();
    spec_trans(&mut cards, Encoding::FitsPc);
    assert_eq!(cards.get_float("MJD-OBS"), Some(50000.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_date_obs 2>&1 | tail -10`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
fn translate_date_obs(cards: &mut CardList) {
    // C: fitschan.c:31847-31897.
    if cards.find("MJD-OBS").is_some() { return }
    let Some(date) = cards.get_string("DATE-OBS") else { return };
    let Some(mjd) = crate::fitschan::wcs_write::date_str_to_mjd(&date) else { return };
    cards.set_float("MJD-OBS", mjd, "Modified Julian date of observation");
}
```

In `spec_trans`: `translate_date_obs(cards);`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans:: 2>&1 | tail -15`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #9 DATE-OBS -> MJD-OBS"
```

---

### Task 12: Translation #11 — AIPS NCP/GLS projections

**Files:**
- Modify: `crates/ast-core/src/fitschan/spec_trans.rs`

C source: fitschan.c:31902-32022. CTYPE values ending `-NCP` are equivalent to SIN with a specific PV2_2 (`cot(δ_ref)`). CTYPE values ending `-GLS` are equivalent to SFL.

- [ ] **Step 1: Write the failing test**

```rust
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
    // PV2_1 = 0 per FITS-WCS paper II for the SIN-as-NCP case.
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ast-core --lib spec_trans::tests::translates_ncp 2>&1 | tail -10 && cargo test -p ast-core --lib spec_trans::tests::translates_gls 2>&1 | tail -5`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
fn translate_ncp_gls(cards: &mut CardList) {
    // C: fitschan.c:31902-32022. Per-version-letter walk.
    for s in std::iter::once(' ').chain('A'..='Z') {
        let Some((axlon, axlat)) = celestial_axes(cards, s) else { continue };
        let key = |prefix: &str, n: usize| {
            if s == ' ' { format!("{prefix}{n}") } else { format!("{prefix}{n}{s}") }
        };
        let lon_ctype = cards.get_string(&key("CTYPE", axlon)).unwrap_or_default();
        let lat_ctype = cards.get_string(&key("CTYPE", axlat)).unwrap_or_default();
        if !lon_ctype.ends_with("-NCP") && !lon_ctype.ends_with("-GLS") {
            continue;
        }
        if lon_ctype.ends_with("-NCP") {
            // Replace -NCP with -SIN.
            let new_lon = lon_ctype.replace("-NCP", "-SIN");
            let new_lat = lat_ctype.replace("-NCP", "-SIN");
            let lon_comment = cards.find(&key("CTYPE", axlon)).and_then(|c| c.comment.clone()).unwrap_or_default();
            let lat_comment = cards.find(&key("CTYPE", axlat)).and_then(|c| c.comment.clone()).unwrap_or_default();
            cards.set_string(&key("CTYPE", axlon), &new_lon, &lon_comment);
            cards.set_string(&key("CTYPE", axlat), &new_lat, &lat_comment);
            // PV2_1 = 0, PV2_2 = cot(δ_ref) (mirrors C:31840-31860).
            let crval_lat = cards.get_float(&key("CRVAL", axlat)).unwrap_or(0.0);
            let pv_key = |i: usize, m: usize| {
                if s == ' ' { format!("PV{i}_{m}") } else { format!("PV{i}_{m}{s}") }
            };
            cards.set_float(&pv_key(axlat, 1), 0.0, "");
            let cot = 1.0 / crval_lat.to_radians().tan();
            cards.set_float(&pv_key(axlat, 2), cot, "");
        } else {
            // -GLS → -SFL.
            let new_lon = lon_ctype.replace("-GLS", "-SFL");
            let new_lat = lat_ctype.replace("-GLS", "-SFL");
            let lon_comment = cards.find(&key("CTYPE", axlon)).and_then(|c| c.comment.clone()).unwrap_or_default();
            let lat_comment = cards.find(&key("CTYPE", axlat)).and_then(|c| c.comment.clone()).unwrap_or_default();
            cards.set_string(&key("CTYPE", axlon), &new_lon, &lon_comment);
            cards.set_string(&key("CTYPE", axlat), &new_lat, &lat_comment);
        }
    }
}
```

In `spec_trans`: `translate_ncp_gls(cards);`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ast-core --lib spec_trans:: 2>&1 | tail -15`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/spec_trans.rs
git commit -m "fitschan/spec_trans: translation #11 AIPS NCP/GLS"
```

---

### Tasks 13-25: Remaining translations

Each follows the same pattern as Task 12: a TDD cycle (test → fail → implement → pass → commit). The translations in remaining tasks are:

- **Task 13:** IRAF ZPX (fitschan.c:32027-32152). Read WAT2_NNN keywords, parse projection params, emit PV<axlat>_*. Convert `xxx-ZPX` → `xxx-ZPN`.
- **Task 14:** IRAF TNX (fitschan.c:32157-32317). Same pattern as ZPX but for TNX → TPN. Includes WAT-string parsing for WCS coefficients.
- **Task 15:** IRAF mini-WCS strip (fitschan.c:32322-32342). Remove `WAT*`, `MWCS*`, mini-WCS `CD*[A-Z]`, `CRVAL*[A-Z]` keys.
- **Task 16:** QV → PV (fitschan.c:32347-32382). Rename pattern `QVi_m` → `PVi_m`.
- **Task 17:** RESTFREQ → RESTFRQ (fitschan.c:32387-32397). Single rename.
- **Task 18:** Early paper-III spectral CTYPE (fitschan.c:31135-31212). `xxx-WAV/-FRQ/-VEL` → `xxx-X2P` form (the inverse of what `wcs_spectral.rs::nonlin_spec_wcs` already handles on the read side).
- **Task 19:** AIPS spectral CTYPE (fitschan.c:32402-32512). Read pattern `[VRAD|FREQ|VOPT|FELO|VELO]-XXX` (XXX ∈ {LSR, LSD, HEL, GEO, OBS}), translate to modern form (FREQ/VRAD/VOPT-F2W) + emit SPECSYS.
- **Task 20:** OBSRA / OBSDEC (fitschan.c:32522-32612). When the header lacks celestial axes but has `OBSRA`/`OBSDEC`, synthesise a CmYPE/CmVAL/CmPIX/CmELT pair on a free coordinate version to attach the celestial info.
- **Task 21:** CUNIT case-insensitive normalisation (fitschan.c:32617-32632). "M/S" → "m/s", "Angstrom" → "Angstrom" (canonical case), etc.
- **Task 22:** FITS-CLASS specific rewrites (fitschan.c:32637-32667). Remove RESTFREQ, swap CRVAL3/RESTFRQ when needed.
- **Task 23:** SAO COi_j → TPN (fitschan.c:32044-32102). When CTYPE is `xxx-TAN` *and* `COi_j` keys are present, convert to `xxx-TPN` with `PVi_m` from the polynomial coefficients.
- **Task 24:** LAMBDA → WAVE (fitschan.c:32107-32114). Single CTYPE rename when CTYPE = "LAMBDA".
- **Task 25:** TAN/TPV with PolyTan or SCAMP-TPV (fitschan.c:32122-32222). Convert `xxx-TPV` to `xxx-TPN`; convert `xxx-TAN` (PolyTan) to `xxx-TPN`.

**For each of tasks 13-25, follow the Task 12 template exactly:**
1. Write a failing test using a synthetic CardList that exhibits the input keyword pattern
2. Verify the test fails with the appropriate "no translation" message
3. Add a private function `translate_<name>(cards: &mut CardList)` mirroring the C source line range cited
4. Wire it into `spec_trans`'s body in C source order
5. Run all spec_trans tests and ensure no regression
6. Commit with message `fitschan/spec_trans: translation #N <description>`

Each task should produce one commit covering its translation (with a unit test) and not regress any other test.

---

### Task 26: Wire spec_trans into FITS-PC reader

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs`
- Modify: `crates/ast-core/src/fitschan/enc_pc.rs`

- [ ] **Step 1: Write the failing integration test**

In `crates/ast-core/tests/test_wcsconv.rs` (the existing integration runner already covers this via the `fits_pc_gal_fits_pc` fixture). Verify the test currently fails:

Run: `cargo test -p ast-core --test test_wcsconv wcsconv_cases::fits_pc_gal_fits_pc -- --include-ignored --nocapture 2>&1 | tail -8`
Expected: FAIL with the MJD-OBS / RA-Dec discrepancy from the diff captured during planning.

- [ ] **Step 2: Wire `spec_trans` into the FITS-PC read path**

In `crates/ast-core/src/fitschan/mod.rs`, modify the `Encoding::FitsPc` arm of `Channel::read`:

```rust
Encoding::FitsPc => {
    // Apply SpecTrans to normalise non-standard keywords (Cnxxx
    // alt-axis form, CDjjjiii → CDj_i, CROTA → PC, etc.).
    super::spec_trans::spec_trans(&mut self.cards, Encoding::FitsPc);
    // After spec_trans, alt-axis keywords are in the *A..*Z form,
    // so we can drive the multi-plane FitsWcs reader instead of
    // the legacy single-plane enc_pc reader.
    let (primary, alts) = wcs_keywords::extract_wcs_keywords_with_alts(&self.cards)?;
    let obj = wcs_read::decode_wcs_with_alts(&primary, &alts)?;
    Ok(Some(obj))
}
```

- [ ] **Step 3: Remove duplicate translation logic from `enc_pc.rs`**

Delete the inline RADECSYS-fallback (line ~104) and the inline RESTFREQ-fallback (line ~122) — both are now handled by spec_trans. Update inline comments to point to `spec_trans` for the rationale.

- [ ] **Step 4: Run the integration test**

Run: `cargo test -p ast-core --test test_wcsconv wcsconv_cases::fits_pc_gal_fits_pc -- --include-ignored 2>&1 | tail -5`
Expected: PASS.

Run: `cargo test -p ast-core --test test_wcsconv wcsconv_cases::fits_pc_altgal_fits_wcs -- --include-ignored 2>&1 | tail -5`
Expected: PASS.

Run: `cargo test -p ast-core --test test_wcsconv -- --include-ignored 2>&1 | grep "test result"`
Expected: a higher pass count than 166 (Bucket A's 8 fixtures should now pass).

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/mod.rs crates/ast-core/src/fitschan/enc_pc.rs
git commit -m "fitschan: route FITS-PC reads through spec_trans + multi-plane decoder"
```

---

### Task 27: Wire spec_trans into FITS-IRAF reader

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs`
- Modify: `crates/ast-core/src/fitschan/enc_iraf.rs`

Same pattern as Task 26: in `Channel::read::Encoding::FitsIraf`, call `spec_trans` first, then route through the alt-aware reader. Remove inline IRAF CTYPE translator (`translate_iraf_ctype_read`) and RESTFREQ-fallback from `enc_iraf.rs`.

- [ ] **Step 1: Verify the affected fixture currently fails**

Run: `cargo test -p ast-core --test test_wcsconv wcsconv_cases::a20070718_fits_iraf -- --include-ignored 2>&1 | tail -5`
Expected: FAIL.

- [ ] **Step 2: Wire spec_trans into FITS-IRAF read path**

```rust
Encoding::FitsIraf => {
    super::spec_trans::spec_trans(&mut self.cards, Encoding::FitsIraf);
    let (primary, alts) = wcs_keywords::extract_wcs_keywords_with_alts(&self.cards)?;
    let obj = wcs_read::decode_wcs_with_alts(&primary, &alts)?;
    Ok(Some(obj))
}
```

- [ ] **Step 3: Remove duplicate logic from `enc_iraf.rs`**

Delete `translate_iraf_ctype_read` (~line 230) and the RESTFREQ-fallback (~line 114).

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_wcsconv wcsconv_cases::a20070718_fits_iraf -- --include-ignored 2>&1 | tail -5`
Expected: PASS.

Run: `cargo test -p ast-core --test test_wcsconv -- --include-ignored 2>&1 | grep "test result"`
Expected: continued improvement.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/mod.rs crates/ast-core/src/fitschan/enc_iraf.rs
git commit -m "fitschan: route FITS-IRAF reads through spec_trans"
```

---

### Task 28: Wire spec_trans into FITS-AIPS / AIPS++ readers

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs`
- Modify: `crates/ast-core/src/fitschan/enc_aips.rs`

Same pattern. AIPS doesn't use `Cnxxx` alt-axis form, but does benefit from RADECSYS / EPOCH / DATE-OBS / AIPS-spectral-CTYPE / OBSRA-OBSDEC translations.

- [ ] **Step 1: Verify a representative AIPS fixture currently fails**

(Use any class_*_fits_class or remaining AIPS fixture that's still failing — e.g. classifier doc lists Bucket B fixtures.)

- [ ] **Step 2: Wire spec_trans**

```rust
Encoding::FitsAips | Encoding::FitsAipsPP => {
    super::spec_trans::spec_trans(&mut self.cards, self.encoding);
    let (primary, alts) = wcs_keywords::extract_wcs_keywords_with_alts(&self.cards)?;
    let obj = wcs_read::decode_wcs_with_alts(&primary, &alts)?;
    Ok(Some(obj))
}
```

- [ ] **Step 3: Remove duplicate logic from `enc_aips.rs`**

Delete `translate_aips_spectral_read` (~line 734) and the RESTFREQ-fallback. The CTYPE rewriting is now handled by spec_trans translation #19.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ast-core --test test_wcsconv -- --include-ignored 2>&1 | grep "test result"`
Expected: maintain or improve total pass count.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/mod.rs crates/ast-core/src/fitschan/enc_aips.rs
git commit -m "fitschan: route FITS-AIPS reads through spec_trans"
```

---

### Task 29: Wire spec_trans into FITS-WCS / FITS-CLASS / DSS readers

**Files:**
- Modify: `crates/ast-core/src/fitschan/mod.rs`

Apply spec_trans at the entry of every FITS read path. FITS-WCS doesn't need most translations (it uses standard keywords) but the OBSRA/OBSDEC, RESTFREQ, LAMBDA-WAVE, and SAO-TPV cases still apply.

- [ ] **Step 1: Wire spec_trans into the remaining read arms**

In `mod.rs`, prepend `super::spec_trans::spec_trans(&mut self.cards, self.encoding);` to each of:
- `Encoding::FitsWcs`
- `Encoding::FitsClass`
- `Encoding::Dss`

- [ ] **Step 2: Run the full wcsconv suite**

Run: `cargo test -p ast-core --test test_wcsconv -- --include-ignored 2>&1 | grep "test result"`
Expected: total pass count higher than 166. Document the new count.

- [ ] **Step 3: Run the full ast-core suite to check no regression**

Run: `cargo test -p ast-core 2>&1 | grep -E "test result:|FAILED" | tail -20`
Expected: only known pre-existing failures (the wcsconv subset still failing).

- [ ] **Step 4: Run fmt + clippy**

Run: `cargo fmt && cargo clippy -p ast-core --all-targets -- -D warnings 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/mod.rs
git commit -m "fitschan: route all FITS encodings through spec_trans"
```

---

### Task 30: Update wcsconv classification doc

**Files:**
- Modify: `docs/issues/wcsconv-residual-classification.md`

- [ ] **Step 1: Re-run the full wcsconv suite and capture the new failure list**

```bash
cargo test -p ast-core --test test_wcsconv -- --include-ignored 2>&1 | grep FAILED | grep -v "test result" | sort -u > /tmp/wcsconv_failures.txt
wc -l /tmp/wcsconv_failures.txt
```

- [ ] **Step 2: Update the bucket counts and "recommended attack order" section**

Reflect the new total in the doc's summary table. Note which buckets are now empty (Bucket A specifically expects to be cleared by SpecTrans).

- [ ] **Step 3: Commit**

```bash
git add docs/issues/wcsconv-residual-classification.md
git commit -m "docs: refresh wcsconv classification after SpecTrans port"
```

---

## Self-Review

Read through the spec coverage table again and the per-task content. Each of the 24 SpecTrans translations has its own task (Tasks 3-25). Tasks 1-2 set up the module and Card-rename helper. Tasks 26-29 wire spec_trans into each FITS-* read path. Task 30 refreshes documentation.

Type / function name consistency check:
- `rename_keyword` (Task 2) is used in Tasks 3, 4, 5, 7, 8 (all simple-rename translations).
- `celestial_axes` (Task 6) is used in Tasks 6, 7, 12 (all axis-aware translations).
- `axis_is_celestial` is referenced in Task 6 but unused after; either delete it from the implementation or keep it for clarity.
- `translate_X` private helpers in Tasks 6, 7, 8, 9, 10, 11, 12 are wired into `spec_trans` body in C-source order.

Placeholder scan: searched for "TBD", "fill in", "appropriate error handling" — only the Tasks 13-25 grouping uses the abbreviated form, but each line cites the specific C source range, the input pattern, and the output pattern. The agent has enough information to follow the Task 12 template; the cross-references to Task 12 are explicit.

Test code is concrete in every code-bearing step; no "write a test for the above" phrasing.

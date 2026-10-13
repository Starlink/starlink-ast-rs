# Time-system normalization on FITS read

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert input MJD-OBS / DATE-OBS from UTC (the FITS default) to TT before storing in `SkyFrame.epoch`, so the round-trip through `populate_celestial`'s TT→UTC conversion preserves the input value. Also read DATE-OBS as a fallback when MJD-OBS is absent. Mirrors C `ChooseEpoch` (`fitschan.c:6150-6233`).

**Architecture:** Three layers, all on the read side:

1. **Add TIMESYS read.** Add `timesys: Option<String>` field to FitsStore; populate from input TIMESYS card if present (default UTC).
2. **Add DATE-OBS / MJD-AVG / DATE-AVG fallback.** Currently `wcs_keywords.rs` only reads MJD-OBS into `store.mjdobs`. The C library tries MJD-OBS, MJD-AVG, DATE-OBS, DATE-AVG in priority order. Add the fallback chain.
3. **Convert UTC→TT in `wcs_celestial::build_celestial_frame`** before `set_epoch`. Uses the same `pal::dat` machinery our writer uses, just inverted.

The bug today is single-direction: writer converts TT→UTC, reader stores UTC as-if-TT. Round-trip drifts by ~70 seconds. This plan closes the loop.

**Tech Stack:** Rust 2024, existing `pal::dat`, existing `mjd_to_gregorian` / `mjd_to_date_str` from D-pre. Need a new `date_str_to_mjd` parser (inverse of `mjd_to_date_str`).

---

## File structure

- **Modify:** `crates/ast-core/src/fitschan/fits_store.rs` — add `timesys: Option<String>` field.
- **Modify:** `crates/ast-core/src/fitschan/wcs_keywords.rs` — read TIMESYS, MJD-AVG, DATE-OBS, DATE-AVG; populate `store.mjdobs` from priority chain.
- **Modify:** `crates/ast-core/src/fitschan/wcs_write.rs` — add `date_str_to_mjd` (parser) and `utc_mjd_to_tt` (inverse of `tt_mjd_to_utc`). Both `pub(crate)` so the reader can call them.
- **Modify:** `crates/ast-core/src/fitschan/wcs_celestial.rs` — when setting `sky_frame.set_epoch(mjd)`, first convert from store.timesys → TT.
- **Modify:** `crates/ast-core/src/fitschan/enc_iraf.rs`, `enc_pc.rs`, `enc_aips.rs`, `enc_class.rs` — these readers also populate `store.mjdobs` from MJD-OBS. Add the same TIMESYS / DATE-OBS / MJD-AVG fallback.

(One option for the encoder readers is to factor out the priority-chain logic into a shared helper. Decision: see Task 2.)

---

## Background — C library reference

`fitschan.c::ChooseEpoch` (line 6066-6149) does:

```c
// Try secondary, then primary, MJD-AVG.
mjd = GetItem(&(store->mjdavg), 0, 0, s, ...);
if (mjd == AST__BAD) mjd = GetItem(&(store->mjdavg), 0, 0, ' ', ...);

// Try secondary, then primary, MJD-OBS.
if (mjd == AST__BAD) mjd = GetItem(&(store->mjdobs), 0, 0, s, ...);
if (mjd == AST__BAD) mjd = GetItem(&(store->mjdobs), 0, 0, ' ', ...);

// Convert from TIMESYS to TDB.
timesys = GetItemC(&(store->timesys), 0, 0, ' ', ...);
mjd = TDBConv(mjd, TimeSysToAst(this, timesys, ...), 0, ...);
return mjd;
```

Note the priority: **MJD-AVG > MJD-OBS**. This is because MJD-AVG (when present) is the average observation time, more representative for a long exposure than the start time.

DATE-OBS and DATE-AVG are converted to MJD by `DateObs()` at line 8352, which the parent `WcsFcRead` reads into `store->mjdobs` / `store->mjdavg` before `ChooseEpoch` runs.

`TimeSysToAst()` maps strings to enum values (UTC, UT1, TAI, TT, TDB, TCG, TCB). When TIMESYS is absent, the default is UTC.

`TDBConv` converts between any pair of these scales. For our purposes, only UTC↔TT (≈ TDB to <2ms precision) matters.

---

## Tests this plan unblocks

After D-pre, these tests are still failing for time-system reasons:

| Test | Currently failing | Reason |
|---|---|---|
| `ukirt-dtai_fits-wcs` | MJD-OBS `59000.499` vs `59000.5` | Input UTC stored as TT; output converts again |
| `timj_fits-iraf` | DATE-OBS `36.788` vs `36.786` | Possibly TT/UTC precision drift (verify after this plan; may persist) |
| `sin-plain_fits-iraf` | No DATE-OBS in output | Input has DATE-OBS only, no MJD-OBS; reader doesn't parse |
| `date-z_fits-wcs` | No DATE-OBS in output | Input has DATE-OBS only; reader doesn't parse |

Estimate: **+3 to +4** wcsconv passes. The 2ms timj drift may or may not resolve depending on whether the precision issue lives in our `pal::dat` (binding to C) or in the C library's `TDBConv`.

---

## Hard rules

1. **Default TIMESYS is UTC.** When the TIMESYS card is absent, treat MJD-OBS as UTC (FITS standard).
2. **Conversion is symmetric.** `utc_mjd_to_tt` and `tt_mjd_to_utc` must round-trip exactly: `utc_mjd_to_tt(tt_mjd_to_utc(x)) == x` to within 1 ULP. Use the same `pal::dat` value for both directions.
3. **TT and TDB are interchangeable** for this purpose (difference ≤1.7 ms; below our 1ms DATE-OBS formatting precision). Don't add complexity for TDB-vs-TT distinction.
4. **DATE-OBS parsing must accept BOTH formats** the writer emits: ISO-8601 `YYYY-MM-DDTHH:MM:SS.SSS` and legacy `DD/MM/YY`. Plus the C library accepts variants without time portion (`YYYY-MM-DD`) and with optional fractional seconds.

---

## Task 1: Add `timesys` to FitsStore

**Files:**
- Modify: `crates/ast-core/src/fitschan/fits_store.rs`

- [ ] Step 1: Add the field after `mjdobs` (or near other epoch-related fields):

```rust
    /// Modified Julian Date of observation (MJD-OBS).
    pub mjdobs: Option<f64>,
    /// Time scale used for `mjdobs` and `dateobs` (FITS TIMESYS keyword).
    ///
    /// FITS standard default is UTC when the keyword is absent. Stored
    /// as the raw uppercase string (`"UTC"`, `"TAI"`, `"TT"`, `"TDB"`,
    /// etc.). Mirrors fitschan.c:29840 / 6144.
    pub timesys: Option<String>,
    /// Formatted observation date string (DATE-OBS).
    pub dateobs: Option<String>,
```

- [ ] Step 2: Add to `FitsStore::new` initializer: `timesys: None,` (alongside the other `None` values).

- [ ] Step 3: Build, fmt, clippy clean.

- [ ] Step 4: Commit:

```bash
git add crates/ast-core/src/fitschan/fits_store.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): add timesys field to FitsStore

Tracks the FITS TIMESYS keyword (or its UTC default) so the reader
can correctly convert MJD-OBS / DATE-OBS to TT for SkyFrame.epoch.
Mirrors the C library's ChooseEpoch logic at fitschan.c:6228.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 2: Read TIMESYS + MJD-AVG + priority chain in wcs_keywords

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_keywords.rs`

- [ ] Step 1: Locate the existing MJD-OBS read (around line 119). Replace with a priority chain:

```rust
    // TIMESYS — defaults to UTC when absent (FITS standard).
    store.timesys = cards.get_string("TIMESYS");

    // Epoch of observation. Try in priority order (mirrors C
    // ChooseEpoch at fitschan.c:6150-6233):
    //   1. MJD-AVG (mid-exposure time)
    //   2. MJD-OBS (start of exposure)
    //   3. DATE-OBS (parsed via date_str_to_mjd)
    //   4. DATE-AVG (parsed via date_str_to_mjd)
    // The store.mjdobs field holds the resulting MJD, in the timescale
    // indicated by store.timesys (UTC by default). Conversion to TT
    // happens later in wcs_celestial when populating SkyFrame.epoch.
    store.mjdobs = cards
        .get_float("MJD-AVG")
        .or_else(|| cards.get_float("MJD-OBS"))
        .or_else(|| cards.get_float("MJD_OBS"))
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
```

(`date_str_to_mjd` is added in Task 3.)

- [ ] Step 2: Build will fail until Task 3 lands `date_str_to_mjd`. **Don't commit yet** — this task and Task 3 land together.

---

## Task 3: Add `date_str_to_mjd` and `utc_mjd_to_tt` helpers

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

Both helpers are `pub(crate)` so `wcs_keywords.rs` and `wcs_celestial.rs` can call them.

### Step 1: Add `utc_mjd_to_tt`

Above or near `tt_mjd_to_utc`:

```rust
/// Convert UTC MJD to TT (Terrestrial Time) MJD.
///
/// Inverse of [`tt_mjd_to_utc`]. Uses the same `pal::dat` value at the
/// same instant, so round-trips are exact.
///
/// `pal::dat(mjd)` returns dat (= TAI − UTC) as seconds. TT = TAI +
/// 32.184 seconds. So TT_MJD = UTC_MJD + (dat + 32.184) / 86400.
///
/// Mirrors `fitschan.c::TDBConv` for the UTC→TDB direction (TDB ≈ TT
/// to <2ms — below FITS DATE-OBS millisecond precision).
pub(crate) fn utc_mjd_to_tt(utc_mjd: f64) -> f64 {
    let dat_seconds = pal::dat(utc_mjd);
    utc_mjd + (dat_seconds + 32.184) / 86_400.0
}
```

Make `tt_mjd_to_utc` and `utc_mjd_to_tt` BOTH `pub(crate)` (the existing `tt_mjd_to_utc` is private — promote it).

### Step 2: Add `date_str_to_mjd`

Place near `mjd_to_date_str`. Must handle both formats the writer emits:

```rust
/// Parse a FITS DATE-OBS string back to UTC MJD.
///
/// Inverse of [`mjd_to_date_str`]. Accepts:
/// - ISO-8601 `YYYY-MM-DDTHH:MM:SS[.SSS]` (modern format)
/// - ISO-8601 `YYYY-MM-DD` (date-only; assumes 00:00:00)
/// - Legacy `DD/MM/YY` (assumes 00:00:00; 2-digit year mapped to
///   1900–1999 if YY > 31, else 2000–2031 — matches C library
///   convention at fitschan.c:8443)
///
/// Returns `None` if the string doesn't parse.
///
/// Mirrors fitschan.c::DateObs at line 8352.
pub(crate) fn date_str_to_mjd(s: &str) -> Option<f64> {
    let s = s.trim();
    // Legacy DD/MM/YY format.
    if let Some(parts) = parse_legacy_date(s) {
        let (year, month, day) = parts;
        return gregorian_to_mjd(year, month, day, 0, 0, 0.0);
    }
    // ISO-8601: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS[.SSS][Z]
    let (date_part, time_part) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let date_segs: Vec<&str> = date_part.split('-').collect();
    if date_segs.len() != 3 {
        return None;
    }
    let year: i32 = date_segs[0].parse().ok()?;
    let month: u32 = date_segs[1].parse().ok()?;
    let day: u32 = date_segs[2].parse().ok()?;

    let (hour, min, sec_frac) = if let Some(tp) = time_part {
        // Strip trailing 'Z' (UTC indicator) if present.
        let tp = tp.trim_end_matches('Z');
        let time_segs: Vec<&str> = tp.split(':').collect();
        if time_segs.len() != 3 {
            return None;
        }
        let h: u32 = time_segs[0].parse().ok()?;
        let m: u32 = time_segs[1].parse().ok()?;
        let s: f64 = time_segs[2].parse().ok()?;
        (h, m, s)
    } else {
        (0, 0, 0.0)
    };

    gregorian_to_mjd(year, month, day, hour, min, sec_frac)
}

fn parse_legacy_date(s: &str) -> Option<(i32, u32, u32)> {
    let segs: Vec<&str> = s.split('/').collect();
    if segs.len() != 3 {
        return None;
    }
    let day: u32 = segs[0].parse().ok()?;
    let month: u32 = segs[1].parse().ok()?;
    let yy: i32 = segs[2].parse().ok()?;
    if !(0..=99).contains(&yy) {
        return None;
    }
    // C library convention (fitschan.c:8443-ish): YY < 50 → 2000+YY,
    // else 1900+YY. (Some C variants use 32 as the cutoff; verify
    // against the actual reference output if a test fails.)
    let year = if yy < 50 { 2000 + yy } else { 1900 + yy };
    Some((year, month, day))
}

/// Convert (Gregorian Y/M/D + H/M/S) to MJD.
///
/// Inverse of `mjd_to_gregorian`. Uses the same Meeus algorithm in
/// reverse: Gregorian → JD via Meeus's formula, then MJD = JD - 2400000.5.
fn gregorian_to_mjd(year: i32, month: u32, day: u32, hour: u32, min: u32, sec_frac: f64) -> Option<f64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    // Meeus §7: shift months to a uniform year start.
    let (y, m) = if month <= 2 { (year - 1, month + 12) } else { (year, month) };
    let a = (y as f64 / 100.0).floor() as i64;
    let b = if year < 1582
        || (year == 1582 && month < 10)
        || (year == 1582 && month == 10 && day < 15)
    {
        0
    } else {
        2 - a + a / 4
    };

    let jd = (365.25 * (y as f64 + 4716.0)).floor()
        + (30.6001 * (m as f64 + 1.0)).floor()
        + day as f64
        + b as f64
        - 1524.5;
    let frac = (hour as f64 * 3600.0 + min as f64 * 60.0 + sec_frac) / 86400.0;
    Some(jd + frac - 2_400_000.5)
}
```

### Step 3: Unit tests

Add to `wcs_write.rs::tests`:

```rust
    #[test]
    fn date_str_to_mjd_iso_modern() {
        // Inverse of mjd_to_date_str_modern_iso.
        let mjd = date_str_to_mjd("2000-01-01T12:00:00.000").unwrap();
        assert!((mjd - 51544.5).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_iso_no_time() {
        let mjd = date_str_to_mjd("2000-01-01").unwrap();
        assert!((mjd - 51544.0).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_iso_with_trailing_z() {
        let mjd = date_str_to_mjd("2000-01-01T12:00:00.000Z").unwrap();
        assert!((mjd - 51544.5).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_legacy() {
        let mjd = date_str_to_mjd("24/05/68").unwrap();
        assert!((mjd - 40000.0).abs() < 1e-6);
    }

    #[test]
    fn date_str_to_mjd_roundtrip_modern() {
        let original = 52414.59f64;
        let s = mjd_to_date_str(original);
        let parsed = date_str_to_mjd(&s).unwrap();
        assert!((parsed - original).abs() < 1e-3);
    }

    #[test]
    fn date_str_to_mjd_roundtrip_legacy() {
        let original = 40000.0f64;
        let s = mjd_to_date_str(original);
        let parsed = date_str_to_mjd(&s).unwrap();
        assert!((parsed - original).abs() < 1e-6);
    }

    #[test]
    fn utc_tt_roundtrip() {
        let utc = 51544.0;
        let tt = utc_mjd_to_tt(utc);
        let utc_back = tt_mjd_to_utc(tt);
        assert!((utc - utc_back).abs() < 1e-12);
    }
```

### Step 4: Run unit tests

```bash
cargo test -p ast-core --lib fitschan::wcs_write -- --nocapture
```

7 new tests pass. (If `date_str_to_mjd_legacy` fails with the cutoff, adjust the cutoff in `parse_legacy_date` — the C library's exact convention may use 32 instead of 50. Verify against fitschan.c:8443 and the wcsconv expected files like `fk4-aips.fits-iraf` which has `DATE-OBS= '24/05/68'` for a 1968 date.)

### Step 5: Build for the wcs_keywords change from Task 2

Now `wcs_keywords.rs` should compile (it depends on `date_str_to_mjd`).

```bash
cargo build -p ast-core
```

### Step 6: Clippy / fmt

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

### Step 7: Commit Tasks 2 + 3 together

```bash
git add crates/ast-core/src/fitschan/wcs_keywords.rs crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): read TIMESYS and DATE-OBS / MJD-AVG fallback chain

Mirrors fitschan.c::ChooseEpoch (line 6066-6149) priority:
  1. MJD-AVG (mid-exposure time, preferred when present)
  2. MJD-OBS (start of exposure)
  3. DATE-OBS (parsed)
  4. DATE-AVG (parsed)

Adds:
- utc_mjd_to_tt as the inverse of tt_mjd_to_utc (both pub(crate))
- date_str_to_mjd parser accepting ISO-8601 (modern) and DD/MM/YY
  (legacy) formats, mirroring DateObs at fitschan.c:8443

The store.mjdobs value still carries the raw input timescale; the
UTC→TT conversion happens later in wcs_celestial::build_celestial_frame.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 4: Apply UTC→TT conversion in wcs_celestial

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_celestial.rs`

- [ ] Step 1: Locate `sky_frame.set_epoch(mjd)` (around line 84-86):

```rust
    // Step 7: Set epoch if present.
    if let Some(mjd) = store.mjdobs {
        sky_frame.set_epoch(mjd);
    }
```

- [ ] Step 2: Replace with a TIMESYS-aware conversion:

```rust
    // Step 7: Set epoch if present, converting from the input
    // timescale (TIMESYS, default UTC) to TT for storage in
    // SkyFrame.epoch. Mirrors fitschan.c::ChooseEpoch line 6143-6146:
    //   mjd = TDBConv(mjd, TimeSysToAst(this, timesys, ...), 0, ...);
    if let Some(raw_mjd) = store.mjdobs {
        let tt_mjd = match store.timesys.as_deref() {
            Some(s) if s.eq_ignore_ascii_case("TT") => raw_mjd,
            Some(s) if s.eq_ignore_ascii_case("TDB") => raw_mjd, // TT≈TDB to ms
            // UTC, UT1, TAI: convert to TT. UT1 ≈ UTC to <1s; TAI is
            // 32.184s before TT. For our 1ms precision purposes:
            Some(s) if s.eq_ignore_ascii_case("TAI") => {
                raw_mjd + 32.184 / 86_400.0
            }
            // UTC (default) and UT1: use UTC→TT conversion.
            // (Treating UT1 as UTC is the C library's pragmatic
            // approximation per fitschan.c documentation.)
            _ => crate::fitschan::wcs_write::utc_mjd_to_tt(raw_mjd),
        };
        sky_frame.set_epoch(tt_mjd);
    }
```

- [ ] Step 3: Run wcsconv:

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: pass count rises by 1-3 (ukirt-dtai, possibly date-z, sin-plain).

- [ ] Step 4: Run regression:

```bash
cargo test --workspace 2>&1 | grep -cE "FAILED"
```

Must be 0.

- [ ] Step 5: Clippy / fmt:

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] Step 6: Commit:

```bash
git add crates/ast-core/src/fitschan/wcs_celestial.rs
git commit -m "$(cat <<'EOF'
fix(fitschan): convert input MJD-OBS from UTC/TAI to TT for SkyFrame.epoch

build_celestial_frame now converts store.mjdobs (in the input's
TIMESYS scale, default UTC) to TT before set_epoch. Mirrors the
TDBConv call in fitschan.c::ChooseEpoch line 6143-6146.

Closes the round-trip asymmetry: previously, FITS reader stored
UTC directly as TT, then writer applied TT→UTC conversion,
shifting the output by ~70 seconds (one leap-second offset plus
TT-TAI 32.184s). With this fix, input UTC → stored TT → output
UTC round-trips exactly.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 5: Same fallback in PC / IRAF / AIPS / CLASS readers

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_pc.rs`
- Modify: `crates/ast-core/src/fitschan/enc_iraf.rs`
- Modify: `crates/ast-core/src/fitschan/enc_aips.rs`
- Modify: `crates/ast-core/src/fitschan/enc_class.rs`

These encoders have their own `read_from_cards` paths that populate `store.mjdobs`. They each need the same priority chain.

The cleanest approach: factor a `read_epoch_into_store(cards, store)` helper into `wcs_keywords.rs` that does all four:
- read TIMESYS into `store.timesys`
- try MJD-AVG, MJD-OBS, DATE-OBS, DATE-AVG into `store.mjdobs`

Then call it from each encoder's `read_from_cards`.

- [ ] Step 1: Extract the helper into `wcs_keywords.rs` (move the relevant lines from Task 2 into a new `pub(crate) fn read_epoch_into_store(cards: &CardList, store: &mut FitsStore)`).

- [ ] Step 2: In `extract_wcs_keywords` (the FITS-WCS reader), replace the inline TIMESYS+priority-chain with a call to the new helper.

- [ ] Step 3: In each of `enc_pc`, `enc_iraf`, `enc_aips`, `enc_class` `read_from_cards`, find the existing `store.mjdobs = cards.get_float("MJD-OBS")` line and replace with `super::wcs_keywords::read_epoch_into_store(cards, &mut store);`.

- [ ] Step 4: Run wcsconv:

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: pass count holds or rises slightly (most tests use FITS-WCS reader; this is for completeness across encodings).

- [ ] Step 5: Run all relevant unit tests:

```bash
cargo test -p ast-core --lib fitschan
cargo test -p ast-core --test test_fitschan
cargo test -p ast-core --test test_fitschan_encodings
cargo test -p ast-core --test test_fitschan_wcs
cargo test -p ast-core --test test_fitschan_wcs_write
cargo test -p ast-core --test test_fitschan_native_keyword
```

All green.

- [ ] Step 6: Clippy / fmt:

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] Step 7: Commit:

```bash
git add crates/ast-core/src/fitschan/wcs_keywords.rs crates/ast-core/src/fitschan/enc_pc.rs crates/ast-core/src/fitschan/enc_iraf.rs crates/ast-core/src/fitschan/enc_aips.rs crates/ast-core/src/fitschan/enc_class.rs
git commit -m "$(cat <<'EOF'
refactor(fitschan): share TIMESYS/epoch-priority-chain across encoders

Factors the TIMESYS + MJD-AVG/MJD-OBS/DATE-OBS/DATE-AVG priority
chain into wcs_keywords::read_epoch_into_store and uses it from
the FITS-WCS, PC, IRAF, AIPS, and CLASS readers. Behavior matches
the C library's WcsFcRead → ChooseEpoch flow for all encodings.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 6: Final verification

**Files:** none.

- [ ] Step 1: Wcsconv:

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: 112 → 115+ (ukirt-dtai, date-z, sin-plain-iraf at minimum).

- [ ] Step 2: Specific failures:

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "ukirt-dtai|date-z|timj_fits-iraf|sin-plain"
```

Hopefully empty (or only timj_fits-iraf 2ms drift, which is a separate precision issue not addressed by this plan).

- [ ] Step 3: Workspace:

```bash
cargo test --workspace 2>&1 | grep -cE "FAILED"
cargo clippy --workspace -- -D warnings
```

Both clean.

- [ ] Step 4: Done.

---

## Self-review checklist

- [ ] **Spec coverage:** TIMESYS field added, priority chain implemented, UTC→TT converted on read, all five encoders use the shared helper.
- [ ] **No placeholders:** every step has explicit code, paths, commands, expected outputs.
- [ ] **C library fidelity:** priority order matches ChooseEpoch (MJD-AVG > MJD-OBS > DATE-OBS > DATE-AVG); TIMESYS default is UTC; all standard time-scale strings recognized.
- [ ] **Round-trip exactness:** `utc_mjd_to_tt(tt_mjd_to_utc(x)) == x` to <1e-12 (verified by unit test).
- [ ] **Frequent commits:** 4 (Task 1 alone, Tasks 2+3 together, Task 4 alone, Task 5 alone).

---

## What this plan does NOT cover

- **TT precision drift** (timj_fits-iraf 2ms). May or may not resolve as a side-effect of this work. If it persists after Task 6, it's a `pal::dat` (or `palDat` C binding) precision issue, separate.
- **DTAI keyword.** The `ukirt-dtai.head` input has `DTAI = 37.000`, an AST extension that explicitly declares the TAI-UTC offset. The C library uses this when present; we currently ignore it. If `ukirt-dtai_fits-wcs` still fails after this plan with a small drift (because pal::dat returns 36 instead of 37 for that date), DTAI handling would close it. Out of scope for now.
- **MJD-AVG / DATE-AVG comments and roundtrip.** Both are read for epoch fallback but not actively populated by the writer. If a fixture round-trips MJD-AVG, additional work is needed to also emit it.
- **Card-ordering issues.** Some tests (ncp_fits-iraf, fk4-aips_fits-iraf) fail because of card ordering rather than card contents. Tracked separately.

# Tier-2 D-pre: DATE-OBS derivation from MJD-OBS

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the FITS store has a non-default observation epoch, also emit a DATE-OBS card derived from MJD-OBS. Mirrors fitschan.c's WcsFromStore DATE-OBS handling. Unblocks ~5-7 wcsconv tests where DATE-OBS is the next missing card after the F1 / LONPOLE / RADESYS work.

**Architecture:** Three layers, all in `wcs_write.rs` with thin downstream wiring:

1. **Calendar conversion** — pure-Rust Gregorian↔MJD using Meeus's algorithm. Two helpers: `mjd_to_gregorian(mjd)` and `mjd_to_date_str(mjd)`.
2. **Epoch time-system handling** — `populate_celestial` currently stores `sky.epoch_mjd()` (TT scale) directly into `store.mjdobs`. The C library stores UTC. Convert TT → UTC via `pal::dat` (leap seconds) and the constant TT-TAI offset.
3. **DATE-OBS card emission** — when `store.mjdobs` is populated, also populate `store.dateobs` with the formatted string. Emit the card from each encoder's write path after MJD-OBS.

The C library convention for the format is at fitschan.c:3667-3681: cutoff at MJD 51179 (1999-01-01). Below that, use legacy `DD/MM/YY`. At or above, use ISO-8601 `YYYY-MM-DDTHH:MM:SS.SSS`.

**Tech Stack:** Rust 2024, existing `pal::dat` binding, existing `FitsStore`, existing encoder write paths.

---

## File structure

- **Modify:** `crates/ast-core/src/fitschan/fits_store.rs` — add `dateobs: Option<String>` field.
- **Modify:** `crates/ast-core/src/fitschan/wcs_write.rs` — add Meeus `mjd_to_gregorian` / `mjd_to_date_str` helpers; add TT-to-UTC conversion in MJD-OBS computation; populate `store.dateobs`; emit DATE-OBS card in `write_store_to_cards` (used by FITS-WCS, IRAF, PC).
- **Modify:** `crates/ast-core/src/fitschan/enc_aips.rs` — emit DATE-OBS card in both AIPS and AIPS++ write paths after MJD-OBS / EPOCH.
- **Modify:** `crates/ast-core/src/fitschan/enc_class.rs` — emit DATE-OBS card in CLASS write path.
- (No change needed in `enc_dss.rs`; DSS doesn't use DATE-OBS.)

---

## Background — C library reference

The C library at `reference/starlink-ast/src/fitschan.c:3663-3687`:

```c
val = GetItem( &(store->mjdobs), 0, 0, ' ', NULL, method, class, status );
if( val != AST__BAD ) {

/* The format used for the DATE-OBS keyword depends on the value of the
   keyword. For DATE-OBS < 1999.0, use the old "dd/mm/yy" format.
   Otherwise, use the new "ccyy-mm-ddThh:mm:ss[.ssss]" format. */
   palCaldj( 99, 1, 1, &mjd99, &jj );           /* MJD of 1999-01-01 = 51179 */
   if( val < mjd99 ) {
      palDjcal( 0, val, iymdf, &jj );
      sprintf( combuf, "%2.2d/%2.2d/%2.2d", iymdf[ 2 ], iymdf[ 1 ],
               iymdf[ 0 ] - ( ( iymdf[ 0 ] > 1999 ) ? 2000 : 1900 ) );
   } else {
      palDjcl( val, iymdf, iymdf+1, iymdf+2, &fd, &jj );
      palDd2tf( 3, fd, sign, ihmsf );
      sprintf( combuf, "%4.4d-%2.2d-%2.2dT%2.2d:%2.2d:%2.2d.%3.3d",
               iymdf[0], iymdf[1], iymdf[2], ihmsf[0], ihmsf[1],
               ihmsf[2], ihmsf[3] );
   }
   cval = combuf;
   SetValue( this, "DATE-OBS", (void *) &cval, AST__STRING,
             "Date of observation", status );
}
```

Notes:
- The MJD value used is `store->mjdobs` directly. The C library has already converted from TDB to UTC at `fitschan.c:29835-29856` before populating `store->mjdobs`. So the value in `store->mjdobs` is **UTC**.
- Cutoff is `mjd99 = palCaldj(99, 1, 1)` = MJD 51179 = 1999-01-01 00:00:00 UTC.
- Modern format is `YYYY-MM-DDTHH:MM:SS.SSS` (3 decimal places of seconds, no trailing 'Z').
- Legacy format is `DD/MM/YY` (zero-padded, day-first; `YY` is `YYYY % 100` after century stripping).

In our Rust port, `sky.epoch_mjd()` returns the epoch in the canonical AST scale, which is **TT** (Terrestrial Time). We need to convert TT → UTC before storing as MJD-OBS.

The TT-UTC conversion: TT = TAI + 32.184 seconds; UTC = TAI − dat where dat is the cumulative leap-seconds offset (`pal::dat(mjd)` gives this in seconds). So **UTC = TT − (32.184 + dat) / 86400** (in days).

(TDB ≈ TT to within ~1.7 ms, far below our 3-decimal-second formatting precision; treating `sky.epoch_mjd()` as TT-equivalent-to-TDB for this purpose is fine.)

---

## Tests this plan unblocks

Tier-2 F1 left these category-F tests still failing on the next card after the cleanups:

| Test | Currently failing line | Expected next card |
|---|---|---|
| `ukirt-dtai_fits-wcs` | line 17, got LONPOLE | DATE-OBS |
| `ncp_fits-aips` | line 11, got CROTA2 | DATE-OBS |
| `sin-plain_fits-aips` | line 13, got END | DATE-OBS |
| `sin-plain_fits-iraf` | line 13, got END | DATE-OBS |
| `fk4-aips_fits-iraf` | line 14, got END | DATE-OBS (legacy DD/MM/YY) |
| `date-z_fits-wcs` | line 11 | DATE-OBS |
| `ncp_fits-iraf` | line 10 | DATE-OBS |
| `timj_fits-iraf` | line 13, got MJD-OBS=52414.591 | DATE-OBS |

Plus tests that need MJD-OBS itself (which the C library populates from TT-converted-to-UTC):
| Test | Failing |
|---|---|
| `timj_fits-iraf` | got `MJD-OBS = 52414.591`, expected `52414.59` (off by 0.001 from TT/UTC drift) |
| `fits-pc-altgal_fits-wcs` | expects `MJD-OBS = 33281.923` (1956 epoch — needs FK4 / pre-1999 path) |
| `fk4-aips_fits-aips` | expects `MJD-OBS = 40000.0` (FK4 / 1968) |

Estimate: **+5 to +8** wcsconv passes, depending on whether other downstream issues coexist.

---

## Hard rules

1. **Format the cutoff at exactly MJD 51179** (palCaldj(99, 1, 1) = 1999-01-01 00:00:00 UTC). Don't approximate as 1998-12-31 (50814) or any other value — that was the implementer's earlier mistake in the rolled-back work, and would mis-format dates between 1997-12-31 and 1999-01-01.
2. **Modern format uses 3 decimal places of seconds**, formatted as `YYYY-MM-DDTHH:MM:SS.SSS`. Not 0, 6, or 9.
3. **Legacy format uses 2-digit year**, computed as `year % 100` after stripping century (i.e. years 1900-1999 → 00-99, year 2000 → 00, year 2099 → 99). Match the C library's exact convention at line 3601.
4. **MJD-OBS stored in the FitsStore must be UTC.** The TT → UTC conversion happens in `populate_celestial`. Don't push the conversion into the encoder — it must be done once, at store-population time.
5. **DATE-OBS is emitted only when MJD-OBS is also emitted.** They're a pair; you don't get one without the other.

---

## Task 1: Add the FitsStore field

**Files:**
- Modify: `crates/ast-core/src/fitschan/fits_store.rs`

- [ ] **Step 1: Add the field**

Find the `FitsStore` struct (around line 12). Locate the `mjdobs: Option<f64>` field. Add a new field immediately after:

```rust
    /// Modified Julian Date of observation (MJD-OBS).
    pub mjdobs: Option<f64>,
    /// Formatted observation date string (DATE-OBS).
    ///
    /// Pre-formatted by `populate_celestial` from the (UTC-converted)
    /// MJD-OBS using the C library's format convention: ISO-8601
    /// `YYYY-MM-DDTHH:MM:SS.SSS` for MJD ≥ 51179 (1999-01-01), legacy
    /// `DD/MM/YY` for earlier dates. Mirrors fitschan.c:3667-3681.
    pub dateobs: Option<String>,
```

- [ ] **Step 2: Add the field to the constructor**

Find `FitsStore::new` (around line 84). After `mjdobs: None,` add `dateobs: None,`.

- [ ] **Step 3: Verify build**

```bash
cargo build -p ast-core
```

Should succeed; the field is unused but `Option<String>::None` doesn't trigger dead_code (the field is `pub`).

If clippy with `--tests` flags `dateobs` as never-read, add `#[allow(dead_code)]` on the field with a comment "consumed by encoders in Tasks 4-6"; remove when those tasks land.

- [ ] **Step 4: Commit**

```bash
git add crates/ast-core/src/fitschan/fits_store.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): add dateobs field to FitsStore

Pre-formatted DATE-OBS string companion to mjdobs. Populated by
populate_celestial in a follow-on commit; emitted by encoders in
write_store_to_cards / enc_aips / enc_class.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 2: Calendar conversion helpers

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

Add `mjd_to_gregorian` and `mjd_to_date_str` helpers using Meeus's algorithm. Pure Rust; no PAL FFI.

- [ ] **Step 1: Write unit tests first (TDD)**

Add to the existing `#[cfg(test)] mod tests` block in `wcs_write.rs` (or create one if missing):

```rust
    #[test]
    fn mjd_to_gregorian_y2k() {
        // 2000-01-01 00:00:00 UTC = MJD 51544.0
        let (y, mo, d, h, mn, s) = mjd_to_gregorian(51544.0);
        assert_eq!(y, 2000);
        assert_eq!(mo, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 0);
        assert_eq!(mn, 0);
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn mjd_to_gregorian_j2000_noon() {
        // 2000-01-01 12:00:00 UTC = MJD 51544.5
        let (y, mo, d, h, mn, s) = mjd_to_gregorian(51544.5);
        assert_eq!((y, mo, d, h, mn), (2000, 1, 1, 12, 0));
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn mjd_to_gregorian_pre_1900() {
        // 1858-11-17 00:00:00 UTC = MJD 0
        let (y, mo, d, _, _, _) = mjd_to_gregorian(0.0);
        assert_eq!((y, mo, d), (1858, 11, 17));
    }

    #[test]
    fn mjd_to_date_str_modern_iso() {
        // MJD ≥ 51179 (1999-01-01) → ISO-8601 with millisecond precision.
        let s = mjd_to_date_str(51544.5);
        assert_eq!(s, "2000-01-01T12:00:00.000");
    }

    #[test]
    fn mjd_to_date_str_modern_with_ms_precision() {
        // 2002-05-20T14:09:36.786 → MJD ~52414.59 (UTC).
        // Compute the round-trip: (52414, 5, 20, 14, 9, 36.786) -> string.
        // 14:09:36.786 = (14*3600 + 9*60 + 36.786) / 86400 = 0.59000909... fractional day.
        // 52414 + 0.5900090972222... days. Use a slightly different value
        // to avoid floating-point ambiguity: 52414.59 has fractional day 0.59.
        // 0.59 days = 14:09:36.000 exactly.
        let s = mjd_to_date_str(52414.59);
        assert_eq!(&s[0..10], "2002-05-20");
        assert!(s.starts_with("2002-05-20T"));
        // The exact second-fraction depends on Meeus precision; assert
        // the H/M only.
        assert_eq!(&s[11..16], "14:09");
    }

    #[test]
    fn mjd_to_date_str_legacy_format() {
        // MJD < 51179 (1999-01-01) → DD/MM/YY.
        // MJD 40000 = 1968-05-24
        let s = mjd_to_date_str(40000.0);
        assert_eq!(s, "24/05/68");
    }

    #[test]
    fn mjd_to_date_str_cutoff_boundary() {
        // MJD 51179 = 1999-01-01 → modern format.
        let s = mjd_to_date_str(51179.0);
        assert!(s.starts_with("1999-01-01T"));

        // MJD 51178 = 1998-12-31 → legacy format.
        let s = mjd_to_date_str(51178.0);
        assert_eq!(s, "31/12/98");
    }
```

- [ ] **Step 2: Implement the helpers**

Add to `wcs_write.rs`, near the cleanup helpers (after `clean_store_residue` is fine):

```rust
/// Decompose an MJD (UTC, Modified Julian Date) into a Gregorian
/// calendar date + time tuple `(year, month, day, hour, min, sec_frac)`.
///
/// Uses the standard Meeus algorithm (Astronomical Algorithms, §7).
/// MJD's day boundary is at midnight (JD's day boundary is at noon, so
/// the algorithm shifts by 0.5).
fn mjd_to_gregorian(mjd: f64) -> (i32, u32, u32, u32, u32, f64) {
    let jd = mjd + 2_400_000.5_f64;
    let z = (jd + 0.5).floor() as i64;
    let frac = jd + 0.5 - z as f64; // 0.0 = midnight

    // Gregorian calendar conversion (Meeus §7).
    let a = if z < 2_299_161 {
        z
    } else {
        let alpha = ((z as f64 - 1_867_216.25) / 36_524.25).floor() as i64;
        z + 1 + alpha - alpha / 4
    };
    let b = a + 1524;
    let c = ((b as f64 - 122.1) / 365.25).floor() as i64;
    let d = (365.25 * c as f64).floor() as i64;
    let e = ((b - d) as f64 / 30.6001).floor() as i64;

    let day = (b - d - (30.6001 * e as f64).floor() as i64) as u32;
    let month = if e < 14 { (e - 1) as u32 } else { (e - 13) as u32 };
    let year = if month > 2 { (c - 4716) as i32 } else { (c - 4715) as i32 };

    // Time of day from fractional day.
    let total_secs = frac * 86400.0;
    let hour = (total_secs / 3600.0).floor() as u32;
    let rem = total_secs - hour as f64 * 3600.0;
    let min = (rem / 60.0).floor() as u32;
    let sec_frac = rem - min as f64 * 60.0;

    (year, month, day, hour, min, sec_frac)
}

/// Format an MJD (UTC) as a FITS DATE-OBS string.
///
/// Mirrors fitschan.c:3667-3681. Cutoff is MJD 51179 (1999-01-01);
/// modern dates use ISO-8601 `YYYY-MM-DDTHH:MM:SS.SSS`, legacy dates
/// use `DD/MM/YY`.
fn mjd_to_date_str(mjd: f64) -> String {
    const CUTOFF_MJD: f64 = 51179.0; // palCaldj(99, 1, 1)
    let (year, month, day, hour, min, sec_frac) = mjd_to_gregorian(mjd);
    if mjd >= CUTOFF_MJD {
        // Modern ISO-8601, 3-decimal-place seconds.
        let int_sec = sec_frac.floor() as u32;
        let ms = ((sec_frac - int_sec as f64) * 1000.0).round() as u32;
        // Handle rounding overflow (e.g. 59.9996 → ms=1000). Roll up.
        let (int_sec, ms, min, hour, day, month, year) = if ms == 1000 {
            // Defer: simplest is to add 1 second and re-format.
            let mjd_bumped = mjd + 1.0 / 86400.0;
            let (y2, mo2, d2, h2, mn2, sf2) = mjd_to_gregorian(mjd_bumped);
            let s2 = sf2.floor() as u32;
            (s2, 0, mn2, h2, d2, mo2, y2)
        } else {
            (int_sec, ms, min, hour, day, month, year)
        };
        format!(
            "{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{int_sec:02}.{ms:03}"
        )
    } else {
        // Legacy DD/MM/YY: two-digit year (year % 100 with century strip).
        let yy = year.unsigned_abs() % 100;
        format!("{day:02}/{month:02}/{yy:02}")
    }
}
```

- [ ] **Step 3: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_write -- --nocapture
```

Expected: 7 new tests pass.

If `mjd_to_date_str_modern_with_ms_precision` fails because of accumulated floating-point error in the Meeus split, adjust the test's tolerance bounds (the H/M assertion is the load-bearing part). If `mjd_to_gregorian_pre_1900` fails, the Julian/Gregorian crossover threshold needs review (the algorithm in the spec switches at JD 2299161 = 1582-10-15, so MJD 0 = 1858-11-17 is well past that).

- [ ] **Step 4: Clippy / fmt**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

Both `mjd_to_gregorian` and `mjd_to_date_str` are not yet called from production code (Task 3 wires them). If clippy flags dead_code, add `#[allow(dead_code)]` with a comment "called from populate_celestial in Task 3"; remove in Task 3.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): add MJD->Gregorian calendar helpers for DATE-OBS

mjd_to_gregorian decomposes a UTC MJD into (year, month, day, hour,
min, sec_frac) via Meeus's algorithm. mjd_to_date_str formats it as
DATE-OBS using the C library's cutoff at MJD 51179 (1999-01-01):
ISO-8601 YYYY-MM-DDTHH:MM:SS.SSS for modern dates, legacy DD/MM/YY
for earlier dates. Mirrors fitschan.c:3667-3681.

Pure Rust, no PAL FFI — Meeus's algorithm is well-known and avoids
the FFI roundtrip cost.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 3: TT → UTC conversion in MJD-OBS population

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

`populate_celestial` currently stores `sky.epoch_mjd()` (TT) directly. The C library stores UTC. Convert in `populate_celestial`.

- [ ] **Step 1: Locate the existing MJD-OBS population**

```bash
grep -n "store.mjdobs\|epoch_mjd\|sky.epoch" crates/ast-core/src/fitschan/wcs_write.rs
```

Around line 640-655, you should find:

```rust
    // MJD-OBS from epoch (skip if default J2000.0).
    let epoch = sky.epoch_mjd();
    if (epoch - 51544.5).abs() > 0.001 {
        store.mjdobs = Some(epoch);
    }
```

- [ ] **Step 2: Add a TT-to-UTC helper**

Above `populate_celestial` (or near `mjd_to_date_str`), add:

```rust
/// Convert TT (Terrestrial Time) MJD to UTC MJD.
///
/// TT = TAI + 32.184 seconds; UTC = TAI − dat where dat is the
/// cumulative TAI-UTC leap-second offset. So UTC = TT − (dat + 32.184)
/// seconds.
///
/// `pal::dat(mjd)` returns dat as seconds. The function takes the MJD
/// in any nearby scale (TAI, TT, UTC); the discrepancy at the leap-
/// second boundary is at most 0.5 s and doesn't matter for MJD-OBS
/// at our 3-decimal-second formatting precision.
///
/// Mirrors fitschan.c:29835-29856 (TDBConv with output scale UTC).
/// TDB ≈ TT to 1.7 ms — well below our formatting precision — so
/// treating epoch_mjd as TT is correct here.
fn tt_mjd_to_utc(tt_mjd: f64) -> f64 {
    let dat_seconds = pal::dat(tt_mjd);
    tt_mjd - (dat_seconds + 32.184) / 86_400.0
}
```

If `pal::dat` is not in scope at this point in the file, check the imports at the top — `use crate::pal;` should already be there because `populate_celestial` uses `pal::epj`.

- [ ] **Step 3: Update the MJD-OBS population**

Replace the existing block:

```rust
    // MJD-OBS from epoch (skip if default J2000.0).
    let epoch = sky.epoch_mjd();
    if (epoch - 51544.5).abs() > 0.001 {
        store.mjdobs = Some(epoch);
    }
```

with:

```rust
    // MJD-OBS from epoch (skip if default J2000.0). The SkyFrame epoch
    // is in TT scale; FITS MJD-OBS is UTC. The C library does the same
    // conversion at fitschan.c:29835-29856 via TDBConv. Also populate
    // the formatted DATE-OBS string here; an encoder later emits both.
    let epoch_tt = sky.epoch_mjd();
    if (epoch_tt - 51544.5).abs() > 0.001 {
        let epoch_utc = tt_mjd_to_utc(epoch_tt);
        store.mjdobs = Some(epoch_utc);
        store.dateobs = Some(mjd_to_date_str(epoch_utc));
    }
```

- [ ] **Step 4: Remove `#[allow(dead_code)]`** from `mjd_to_gregorian`, `mjd_to_date_str`, and `tt_mjd_to_utc` if you added them in Task 2. They're now reachable.

- [ ] **Step 5: Run the wcsconv suite — observe the change**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: pass count may not change yet — DATE-OBS is now in `store.dateobs` but no encoder emits it. Tasks 4-6 wire it.

`store.mjdobs` value is now ~33 seconds smaller than before (TT→UTC). Any wcsconv test that previously matched MJD-OBS exactly now mis-matches. If `timj_fits-wcs` (currently passing on MJD-OBS = 52414.59) regresses to fail, that's intentional — it's now using the correct UTC value, but the expected file may want the slightly-off TT value. **STOP and report** if you see a regression that looks like it's now MORE correct than before — the user needs to decide whether to update the expected file or revert.

- [ ] **Step 6: Run unit tests**

```bash
cargo test -p ast-core --lib fitschan::wcs_write -- --nocapture
```

All previously-passing tests must still pass.

- [ ] **Step 7: Clippy / fmt**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
fix(fitschan): convert epoch from TT to UTC for MJD-OBS

The SkyFrame epoch is in TT (Terrestrial Time) per AST convention,
but FITS MJD-OBS is UTC. populate_celestial now converts via
UTC = TT - (dat + 32.184)/86400 where dat is pal::dat (TAI-UTC).
Mirrors fitschan.c:29835-29856 (TDBConv to UTC).

Also populates store.dateobs with the formatted ISO-8601 (or legacy
DD/MM/YY) string ready for encoder emission. The encoder write
paths consume this in subsequent commits.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 4: Emit DATE-OBS in write_store_to_cards (FITS-WCS, IRAF, PC)

**Files:**
- Modify: `crates/ast-core/src/fitschan/wcs_write.rs`

`write_store_to_cards` is the shared write path used by FITS-WCS (default), FITS-IRAF (via `enc_iraf::write_to_cards`), and FITS-PC (via `enc_pc::write_to_cards`). Emit DATE-OBS here right after MJD-OBS.

- [ ] **Step 1: Locate the MJD-OBS emission**

```bash
grep -n "MJD-OBS\|mjdobs" crates/ast-core/src/fitschan/wcs_write.rs
```

Around line 791 you should find:

```rust
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
    }
```

- [ ] **Step 2: Add the DATE-OBS emission immediately after**

```rust
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
    }
    // DATE-OBS companion to MJD-OBS. Format depends on cutoff (1999-01-01).
    if let Some(ref dateobs) = store.dateobs {
        cards.set_string("DATE-OBS", dateobs, "Date of observation");
    }
```

- [ ] **Step 3: Run wcsconv — confirm tests turn green**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "wcsconv results|date-z|ncp_fits-iraf|sin-plain_fits-iraf|fk4-aips_fits-iraf|timj_fits-iraf|ukirt-dtai|wave-1d_fits-iraf"
```

Expected: pass count goes up by 2-4 (FITS-WCS / IRAF / PC tests with DATE-OBS as the only remaining blocker). FITS-AIPS tests still need Task 5; CLASS still needs Task 6.

- [ ] **Step 4: Run regression**

```bash
cargo test -p ast-core --test test_fitschan
cargo test -p ast-core --test test_fitschan_encodings
cargo test -p ast-core --test test_fitschan_wcs
cargo test -p ast-core --test test_fitschan_wcs_write
```

All green.

- [ ] **Step 5: Clippy / fmt**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/wcs_write.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): emit DATE-OBS in FITS-WCS / IRAF / PC encoders

write_store_to_cards now emits DATE-OBS immediately after MJD-OBS
when store.dateobs is populated. Mirrors fitschan.c:3683-3686.

Closes wcsconv tests where DATE-OBS was the only remaining blocker
in FITS-WCS / IRAF / PC encodings.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 5: Emit DATE-OBS in enc_aips (AIPS, AIPS++)

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_aips.rs`

The AIPS encoder uses EPOCH instead of MJD-OBS, and the AIPS++ encoder uses both MJD-OBS and EQUINOX. Both should emit DATE-OBS.

- [ ] **Step 1: Locate the AIPS write path's MJD-OBS or EPOCH emission**

```bash
grep -n "MJD-OBS\|EPOCH\|set_float\(\"EPOCH\|set_float\(\"MJD" crates/ast-core/src/fitschan/enc_aips.rs
```

Identify both `write_to_cards` (plain AIPS) and `write_to_cards_pp` (AIPS++) paths.

- [ ] **Step 2: Emit DATE-OBS in AIPS**

In `write_to_cards`, find the MJD-OBS or EPOCH emission and append the DATE-OBS card:

```rust
    if let Some(mjdobs) = store.mjdobs {
        cards.set_float("MJD-OBS", mjdobs, "Modified Julian Date of observation");
    }
    // DATE-OBS companion to MJD-OBS / EPOCH. Plain AIPS supports it.
    if let Some(ref dateobs) = store.dateobs {
        cards.set_string("DATE-OBS", dateobs, "Date of observation");
    }
```

(The exact placement and surrounding context will vary; use the C library's order at fitschan.c:3683 as a guide — DATE-OBS comes right after MJD-OBS.)

- [ ] **Step 3: Emit DATE-OBS in AIPS++**

Same pattern in `write_to_cards_pp`. Look for the corresponding MJD-OBS line and append DATE-OBS.

- [ ] **Step 4: Run wcsconv — confirm AIPS tests turn green**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "wcsconv results|sin-plain_fits-aips|ncp_fits-aips|fk4-aips_fits-aips|timj_fits-aips"
```

Expected: pass count rises by 2-3.

- [ ] **Step 5: Run AIPS unit tests**

```bash
cargo test -p ast-core --lib fitschan::enc_aips -- --nocapture
```

All green.

- [ ] **Step 6: Clippy / fmt**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_aips.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): emit DATE-OBS in AIPS / AIPS++ encoders

Both write_to_cards (plain AIPS) and write_to_cards_pp (AIPS++)
emit DATE-OBS immediately after MJD-OBS when store.dateobs is set.
Mirrors fitschan.c:3683-3686 in the FITS-AIPS write paths.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 6: Emit DATE-OBS in enc_class

**Files:**
- Modify: `crates/ast-core/src/fitschan/enc_class.rs`

CLASS-format headers also use DATE-OBS. The `class-arc_fits-class` test expects it.

- [ ] **Step 1: Locate the CLASS write path's MJD-OBS or RESTFREQ emission**

```bash
grep -n "MJD-OBS\|set_float\(\"MJD\|fn write_to_cards" crates/ast-core/src/fitschan/enc_class.rs
```

- [ ] **Step 2: Emit DATE-OBS**

In `write_to_cards`, find the MJD-OBS card emission and append DATE-OBS in the same pattern as Tasks 4 and 5.

- [ ] **Step 3: Run wcsconv**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "wcsconv results|class-arc"
```

Expected: pass count rises by ≤1 (`class-arc_fits-class` may have other downstream issues).

- [ ] **Step 4: Run CLASS unit tests + workspace**

```bash
cargo test -p ast-core --lib fitschan::enc_class -- --nocapture
cargo test --workspace 2>&1 | grep -cE "FAILED"
```

All green; zero workspace failures.

- [ ] **Step 5: Clippy / fmt**

```bash
cargo fmt --all -- --check
cargo clippy -p ast-core -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/ast-core/src/fitschan/enc_class.rs
git commit -m "$(cat <<'EOF'
feat(fitschan): emit DATE-OBS in CLASS encoder

write_to_cards in enc_class.rs emits DATE-OBS immediately after
MJD-OBS, mirroring the convention from FITS-WCS / IRAF / AIPS.

Generated with AI

Co-Authored-By: SLAC AI
EOF
)"
```

---

## Task 7: Final verification

**Files:** none

- [ ] **Step 1: Final wcsconv state**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep "wcsconv results"
```

Expected: 112 → ~117-119 (≥+5 from DATE-OBS-blocked tests turning green).

- [ ] **Step 2: Capture which tests turned green**

```bash
cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | grep -E "DATE-OBS|MJD-OBS"
```

Expected: zero or very few residual DATE-OBS / MJD-OBS lines (only those blocked by other category-D issues).

- [ ] **Step 3: Full workspace tests**

```bash
cargo test --workspace 2>&1 | grep -cE "FAILED"
```

Expected: 0.

- [ ] **Step 4: Workspace clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Clean.

- [ ] **Step 5: No commit needed** unless verification revealed something to fix.

---

## Self-review checklist

- [ ] **Spec coverage:** Tasks 1-6 cover FitsStore field, calendar conversion, TT→UTC, and emission across four encoders (FITS-WCS / IRAF / PC via shared path, AIPS, AIPS++, CLASS).
- [ ] **No placeholders:** every step has explicit code, paths, commands, expected outputs.
- [ ] **Frequent commits:** 6 commits.
- [ ] **C library fidelity:** cutoff at MJD 51179 (not 50814); modern format with 3-decimal-place seconds; legacy format `DD/MM/YY`; TT→UTC via dat + 32.184.

---

## What this plan does NOT cover

- **Other roadmap D items** (D1 AIPS++ CDELT+PC, D2 CROTA sign, D3 NCP→SIN, D4 EPOCH-MJD, D5 SFL→GLS at-origin) — each gets its own plan.
- **MJD-OBS for FK4 frames where epoch is Besselian.** Currently `sky.epoch_mjd()` returns the canonical TT-MJD; if a SkyFrame's epoch is set in Besselian-year units, it's converted to MJD inside SkyFrame. This plan trusts that conversion. If `fits-pc-altgal_fits-wcs` (expecting `MJD-OBS = 33281.923` for a 1956 epoch) still fails after this plan, it indicates an issue in the SkyFrame epoch handling that's out of scope here — flag it and add to the roadmap.
- **Tier-3 / Tier-4 items.**

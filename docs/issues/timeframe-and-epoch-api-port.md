# TimeFrame port and Epoch/Equinox API audit

Two related concerns surfaced during the wcsconv work that need
follow-up.
Captured here so they survive across sessions.

> **Status update.** Issue A is **RESOLVED**: the DATE-OBS drift was a C
> `DateObs` `sscanf` bug (leading-zero drop), not a Rust defect; the Rust parse
> is correct, the ΔTDB Fairhead-Bretagnon term was added (`985ce1b`), the
> fixture was corrected (`349d18d`), and wcsconv passes 192/192. Issue B is
> mostly resolved — Epoch is now documented as TDB MJD on the Frame trait
> (`frames/mod.rs:1231`). Two minor residual doc/API-hygiene items remain: the
> "Equinox is in UTC" contract note, and the public-vs-private decision on the
> `set_epoch_year`/`set_equinox_year` setters.

## Issue A — DATE-OBS TDB↔UTC round-trip drift

**Symptom.**
`azel_fits_wcs` (and likely several CLASS / spectral fixtures) emit a
DATE-OBS string that differs from the C oracle by a fraction of a
second.
The MJD-OBS rounding (`%.8g` → 4 decimal places) hides the difference,
but the formatted DATE-OBS string is precise to a millisecond and
shows it.

Concrete example (azel_fits_wcs):

```
got: DATE-OBS= '2017-07-21T08:38:39.087' / Date of observation
exp: DATE-OBS= '2017-07-21T08:38:39.870' / Date of observation
```

The 0.783 s difference is roughly `dt(C) - dt(Rust)` between our
`utc_mjd_to_tdb` / `tdb_mjd_to_utc` (built around `pal::dat`
+ `pal::dtdb_geocentre`) and the C library's full `TimeMap` chain
that goes UTC → TAI → TT → TDB with proper leap-second / TDB-TT
handling.

**Status.**
C's `TDBConv` is ported as `epoch::tdb_conv`, which converts through
a `TimeFrame` as C does.
That gives us the same entry point C uses but the underlying
TimeMap conversions in
`crates/ast-core/src/mappings/timemap.rs` may not be bit-exact with
the C oracle yet.

**To do.**
Run a parametric comparison of `epoch::tdb_conv(MJD, UTC, false)` and
the C `TDBConv` for a grid of MJDs (around the leap-second
boundaries 1972, 2017-01-01, …) and characterise the discrepancy.
Likely failure modes:
- `TimeMap::TtToTai` and `TaiToTt` constants vs. C's leap-second
  table.
- `TdbToTt` formula precision (Fairhead-Bretagnon vs. our
  `pal::dtdb_geocentre`).
- DUT1 / Δ-AT defaulting when `obs_*` attributes aren't set on the
  TimeFrame.

The "right" port is to rerun a representative sample of fixtures
through both pipelines with extra C-side instrumentation to log the
intermediate MJD values, then trace where Rust diverges.

## Issue B — Epoch / Equinox API audit

**Concern.**
The Rust API on `SkyFrame` exposes `set_epoch` (MJD), `set_epoch_year`
(year, IAU 1984 rule), `epoch_mjd`, `set_equinox` (MJD),
`set_equinox_year` (year), `equinox_mjd`.
The C oracle has only `astSetEpoch(MJD)` / `astGetEpoch` (and
similarly for Equinox).
C parses years via the string-attribute path (`astSetC(frame,
"Epoch", "J2000")` invokes `astReadDateTime`).

Risks:
- The `*_year` setters use IAU 1984 (Julian for ≥ 1984, Besselian
  earlier).
  C's `astReadDateTime` has the same convention but supports more
  formats (`B1950`, `J2000.5`, raw MJD numbers, etc.).
- Unclear whether the stored Epoch is in TDB.
  C's frame-level docstring says Epoch is TDB — we treat it the
  same way but it's not enforced or documented at the type level.
- Unclear whether the stored Equinox is in UTC.
  C's `WriteSkyFrame` says "It is assumed the Equinox is in UTC"
  (fitschan.c:29717).
  We don't have that contract written down.

**To do.**
- Document the timescale interpretation of stored Epoch and Equinox
  on the `SkyFrame` / `Frame` types.
- Audit every call site of `set_epoch_year` to confirm the input
  year really is in the expected sense (Julian / Besselian / raw
  MJD).
- Decide whether the `_year` setters are part of the public API
  (they have no C analogue) or should move to a private impl
  invoked only by the deserialiser.

Both items are bookmarked alongside the wcsconv fixture work but
deserve a focused pass before the FITS-WCS path is declared
complete.

# `fset_to_store` Port Gaps — C Branches Not Yet Ported

This document tracks every C branch in `fitschan.c` that is referenced
by the `fset_to_store` pipeline but is **not yet ported** to the
Rust side. Earlier commits in Groups 3–5 of the
[MakeIntWorld port plan](../superpower/plans/2026-05-25-makeintworld-port.md)
labelled some of these as "deferred because the existing wcsconv
fixtures don't exercise them" — that's the wrong rationale. **Lack
of fixture coverage means the fixture set is insufficient, not that
the C logic is optional.** Every entry below MUST be ported.

When you pick up an item:

1. Read the C source at the cited line range in full.
2. Add (or reactivate) test fixtures that exercise the branch.
3. Port faithfully.
4. Remove the entry from this list.

Group references map back to plan tasks; "Plan §X.Y" means
"the task numbered X.Y of `2026-05-25-makeintworld-port.md`".

---

## Group 3 — `other_axes.rs`

**Source:** `crates/ast-core/src/fitschan/fset_to_store/other_axes.rs`
mirrors `fitschan.c::OtherAxes` (~24030).

### `IsMapLinear` linearity gate — DONE

Ported in
`crates/ast-core/src/fitschan/fset_to_store/is_map_linear.rs`,
wired into `other_axes` as the first branch of the algorithm
cascade.

### `LogAxis` (-LOG algorithm) — DONE

Ported in
`crates/ast-core/src/fitschan/fset_to_store/log_axis.rs`, wired
into `other_axes` as the second branch (after `is_map_linear`).
Appends `-LOG` to the CTYPE on success.

### `IsMapTab1D` (-TAB algorithm)

C: `IsMapTab1D(...)` at line 24266. 1D tabular look-up tables.
Requires `FitsTable` infrastructure (`store->tables`, `astSetFitsI`
on table headers, `astColumnName`, `astMapPut0A`). Most of that
infrastructure does not yet exist on the Rust side.

**Action:** scoped as a future group — needs `FitsTable` /
`AstFitsTable` Rust port first.

---

## Group 5.1 — `sky_sys.rs`

**Source:** `crates/ast-core/src/fitschan/fset_to_store/sky_sys.rs`
mirrors `fitschan.c::SkySys` (~29599).

### Missing `FitsStore` fields for offset-coord AST extension

C lines 29927–29935 write `store->skyrefis`, `store->skyref`,
`store->skyrefp` when `isoff < 0` — the AST extension that lets a
header round-trip a SkyFrame with `SkyRefIs == Pole` or `Origin`
without losing the offset metadata. The Rust `FitsStore` does not
yet carry these fields.

**Action:** add `skyrefis: Option<String>`, `skyref:
Vec<Option<f64>>`, `skyrefp: Vec<Option<f64>>` to `FitsStore`;
port the C logic into `sky_sys`; teach the encoders (Native, PC,
CLASS, IRAF, AIPS, DSS) to read/write the AST-extension keywords.

### `WCSNAME` from SkyFrame `Domain`

C lines 29959–29961 write `store->wcsname` when the SkyFrame's
`Domain` attribute is set.

**Action:** add `wcsname: Option<String>` to `FitsStore`; emit it
on the writer side; round-trip-test with a Domain-set SkyFrame.

### `OBSGEO-X/Y/Z` (observer geocentric position)

C lines 29965–29975 emit `OBSGEO-X/Y/Z` from `ObsLon`/`ObsLat`/
`ObsAlt` via `eraGd2gc`. Rust `FitsStore` lacks the `obsgeox/y/z`
fields and the encoder side doesn't support the keywords.

**Action:** add fields, port the geodetic→geocentric conversion
(we have `pal::dranrm` etc. but need to verify we expose
`eraGd2gc`'s equivalent), and wire through the encoders.

### `TIMESYS` — TDB↔UTC timescale conversion — DONE

`crate::pal::tdb_conv(mjd, scale, from_tdb)` ports the C `TDBConv`
for the {UTC, TAI, TT, TDB} subset that the FITS-WCS writer
actually produces. `sky_sys` now converts the SkyFrame's stored
TDB epoch to UTC before writing `MJD-OBS`. UT1 / LT / sidereal
scales aren't reachable from a SkyFrame today; if they become
reachable later, route through `TimeFrame::convert_*` instead of
extending `tdb_conv`.

### Unknown-system CTYPE prefix from axis `Symbol` — DONE

`SkyFrame::axis_symbol(axis)` getter added with default-from-system
plus an override slot. `sky_sys::ctype_prefixes_for` now inspects
the axis Symbols in the `NOCEL` branch and accepts the
`xxLN/xxLT` / `xLON/xLAT` forms, falling back to `UNLN/UNLT`
otherwise. Mirrors C:29844-29876.

### SkyFrame `Label` → CTYPE comment / CNAME — DONE

`SkyFrame::axis_label(axis)` / `set_axis_label(axis, ...)` /
`test_axis_label(axis)` getters added.  `sky_sys` now emits the
explicit Label as `ctype_com` and `cname` when set. Mirrors
C:29941-29950.

### `isoff > 0` CTYPE comment "`<sym> offset from <SkyRef>`" — DONE

The C C:29893-29909 path emits a CTYPE comment derived from the
axis Symbol and the SkyRef position when describing offset
coordinates (`isoff > 0`). Rust now emits the same shape using
the `axis_symbol` getter. The SkyRef rendering uses a fixed
`"%.6,%.6"` radian format rather than C's `astGetC(skyfrm,
"SkyRef")` SkyFrame-formatter dispatch — when SkyFrame's
formatter chain matures, route through it.

---

## Group 5.3 — `celestial_axes.rs`

**Source:**
`crates/ast-core/src/fitschan/fset_to_store/celestial_axes.rs`
mirrors `fitschan.c::CelestialAxes` (~4890).

### `-TAB` algorithm fallback (C:5396–5580)

When the pixel→WCS chain has no WcsMap, C falls through to
`IsMapTab2D` for a 2-D `-TAB` description. The Rust port returns a
UnitMap and lets `make_int_world` flag the residual non-linearity.
That's silently wrong for WcsMap-less celestial fixtures — they
should be describable via `-TAB`, not rejected.

**Action:** port `IsMapTab2D` (depends on `FitsTable`
infrastructure — same blocker as `IsMapTab1D`).

### `SkyPole` — LONPOLE / LATPOLE storage — DONE

Ported in
`crates/ast-core/src/fitschan/fset_to_store/sky_pole.rs`. Both
zenithal (PV-3/PV-4 read) and non-zenithal (`wcs_nat_pole` math
helper applying FITS-WCS paper II equations 8-9-10 plus a
post-WcsMap-chain inverse on the celestial north pole) branches
are covered. Wired into `celestial_axes` after `sky_sys`.

### Auto axis-swap with `WorldAxes` (C:5318–5378)

When `FitsAxisOrder == "<auto>"` and the celestial-rotation
transposes lon/lat IWC axes, C re-asses the FITS axis numbering
via `WorldAxes`, swapping `wperm` / CRVAL / CTYPE / CNAME / PV.
The Rust port skips this entirely (effectively pinning
`FitsAxisOrder == "lon-lat"` or whatever the SkyFrame's intrinsic
order is). A header where the FITS layout truly wants the swap
will get axes labelled wrong.

**Action:** port `WorldAxes` and the swap block. `FitsAxisOrder`
is a `FitsChan` attribute we don't yet model on the Rust side.

---

## Group 5.4 — `spectral_axes.rs`

**Source:**
`crates/ast-core/src/fitschan/fset_to_store/spectral_axes.rs`
mirrors `fitschan.c::SpectralAxes` (~30050). This is the largest
gap by far — most of the C function is **not yet ported**.

### Linear / `-LOG` algorithm cascade — DONE

`spectral_axes` now mirrors the C C:30340-30477 algorithm
cascade for the linear and log paths:

  1. `is_map_linear` on the pixel→spectral mapping → CTYPE =
     `<orig_system>` + ShiftMap.
  2. `log_axis` → CTYPE = `<orig_system>-LOG` + log mapping.

### `SourceVel` → `ZSOURCE` / `SSYSSRC` — DONE

`spectral_axes` now reads `SpecFrame::source_vel` /
`source_vrf`, converts m/s velocity to dimensionless redshift via
the relativistic Doppler formula, and emits ZSOURCE + SSYSSRC.

### RESTFRQ vs RESTWAV — DONE

`spectral_axes` now picks RESTWAV (= `c / rest_freq`, in metres)
for wavelength-domain systems (`WAVE`, `AWAV`, `VOPT`, `ZOPT`)
and RESTFRQ (Hz) for the rest. Mirrors C:30740-30751.

### `OBSGEO-X/Y/Z` from SpecFrame observer — DONE

`spectral_axes` now emits OBSGEO from `SpecFrame::obs_lon/lat/alt`
on the primary coord version (using `pal::gd2gc`).

### Axis Label → CTYPE-comment / CNAME — DONE

`SpecFrame::axis_label` / `set_axis_label` / `test_axis_label`
added; `spectral_axes` emits the explicit Label when set.

### `-X2P` non-linear algorithm cascade

C:30359-30454 tries each of FREQ/WAVE/AWAV/VELO as the X system
linearly related to pixels and emits a `XXXX-X2P` CTYPE on
success. The Rust port does NOT yet do this — fixtures whose
spectral CTYPE includes an algorithm suffix (`vopt-f2w.head`,
`vopt-geo.head`, `vrad-lsrd.head`, `wave-f2w.head`,
`freq-ghz-bary.head`, `class-arc.head`) will get the bare system
name as CTYPE.

**Action:** port the X-system cascade. Needs a SpecFrame
`convert_to_system(X)`-style Mapping helper that produces the
`S(X)` chain — i.e. the mapping from a copy of the SpecFrame
in system X back to the original system. This functionality
exists on the C side via `astConvert` but isn't exposed yet on
our SpecFrame API.

### `GrismMap` handling

C C:30480-30599 detects a `GrismMap` in the chain after the
SpecFrame and emits `-GRI`/`-GRA` algorithm codes plus PV
parameters. The Rust port doesn't yet do this — needs an
`extract_grism_map(chain)` walker plus accessors for
`GrismMap::g`/`m`/`alpha`/`nr`/`nrp`/`eps`/`theta`.

### `VELOSYS` calculation

C:30774-30796 — topocentric apparent velocity of the SPECSYS
rest frame. Needs the celestial source position from
`celestial_axes` (Group 7 wiring), plus the existing
`wcs_write::populate_velosys` math. The integration step lives
in Group 7.

### SpecFrame unit-conversion chain rewriting

C C:30276-30311 mutates the input FrameSet to rewrite the
spectral unit when the existing one isn't FITS-compliant
(e.g. user-set `MHz` becomes `Hz` so IWC arithmetic stays in
canonical SI units). Rust currently preserves the user's CUNIT
unchanged. Needs SpecFrame's `astConvert` machinery exposed plus
the FrameSet `remap_frame` plumbing.

---

## Group 4 — `make_int_world.rs`

**Source:**
`crates/ast-core/src/fitschan/fset_to_store/make_int_world.rs`
mirrors `fitschan.c::MakeIntWorld` (~21689).

### SIP-distortion branch (planned for Group 6)

C lines 21882-21895 + 21999-22004 — the `SIPIntWorld` call and
its post-processing. This is a **planned** deferral (Group 6 of
the plan); it's correctly noted as such in the module head and
not a "fixture-coverage" deferral.

**Action:** Group 6 (`SIPIntWorld` body).

---

## Reframing the in-source comments

The module-head comments currently say things like "deferred — the
existing 152-passing-wcsconv-tests baseline doesn't exercise these"
or "Group 8 follow-ups if a wcsconv fixture exposes a gap". Both
phrases imply the work is conditional on fixture coverage. **They
should be reworded** to make clear the work is unconditional:
"TODO — see [`fset_to_store-port-gaps.md`](../../docs/issues/fset_to_store-port-gaps.md)"
and pointing at the relevant entry in this document.

When picking up a deferral, the right sequence is:

1. Add the missing fixture coverage that exercises the C branch.
2. Port the C branch faithfully.
3. Remove the entry from this document.
4. Remove the "TODO" annotation from the in-source module head.

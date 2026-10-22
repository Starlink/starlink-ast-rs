# `fset_to_store` Port Gaps — C Branches Not Yet Ported

**Status: MOSTLY DONE.** SIPIntWorld, VELOSYS, and all Group 3/5 branches are
ported. Re-checked against the tree at the AST 9.5.0 pin (2026-09-25).

The `-TAB` branches (`IsMapTab1D` / `IsMapTab2D`, `AdaptLut`, ForceTab, and
the forward-only branch of `WorldAxes`) are ported in
`fset_to_store/map_tab.rs` (2026-10-13), recorded in the `PLAN.md` §3 Table /
FitsTable row.

Two parameters this pipeline threads were plumbed but unreachable, and both
became reachable in the 9.5.0 sync, so the body's account of them is stale:
`FitsTol` was fixed at `FITS_TOL_DEFAULT = 0.1` and now has
`set_fits_tol` / `fits_tol` accessors; `SipOK` was hardcoded `true` and is now
derived from the encoding at `fset_to_store/mod.rs:260`. Upstream's SIP
validation fixes (`0cd7f725`, `c384289f`, `121d12d2`) are what reach them, gated
in `test_fitschan_wcs_write.rs`.

This document tracks every C branch in `fitschan.c` that is referenced
by the `fset_to_store` pipeline but is **not yet ported** to the
Rust side. Earlier commits in Groups 3–5 of the
[MakeIntWorld port plan](../superpowers/plans/2026-05-25-makeintworld-port.md)
labeled some of these as "deferred because the existing wcsconv
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

### `IsMapTab1D` (-TAB algorithm) — DONE

C: `IsMapTab1D(...)` at line 24266. Ported as `map_tab::is_map_tab_1d`
and wired into `other_axes` after the linear and `-LOG` algorithms,
which ForceTab skips.

---

## Group 5.1 — `sky_sys.rs`

**Source:** `crates/ast-core/src/fitschan/fset_to_store/sky_sys.rs`
mirrors `fitschan.c::SkySys` (~29599).

### Missing `FitsStore` fields for offset-coord AST extension — DONE

`FitsStore` now carries `skyrefis: Option<String>`, `skyref:
Vec<Option<f64>>`, `skyrefp: Vec<Option<f64>>`. `sky_sys` writes
them on the `isoff < 0` path. The FITS-WCS encoder/decoder
(`wcs_write::write_store_to_cards` /
`wcs_keywords::read_wcs_into_store`) emit and read `SREFISa /
SREFi[a] / SREFPi[a]` — note the AST 8-char-budget keyword names
(not `SKYREFIS`). Mirrors C:37185-37216. The PC/IRAF/AIPS/DSS
encoders don't carry these AST extensions; they're lossy by
design (they target legacy formats), so no work needed there.

### `WCSNAME` from SkyFrame `Domain` — DONE

`store.wcsname` is populated by `sky_sys` from the SkyFrame's
Domain. The FITS-WCS writer now emits it on both primary and alt
planes (with the C 1-axis-suppression rule when WCSNAME ==
CTYPE1 case-insensitively); the reader populates it. Mirrors
C:37005-37011.

### `OBSGEO-X/Y/Z` (observer geocentric position) — DONE

`store.obsgeox/y/z` populated by `sky_sys` (and `spectral_axes`)
via `pal::gd2gc` from `ObsLon`/`ObsLat`/`ObsAlt`. The FITS-WCS
writer emits primary-only `OBSGEO-X/Y/Z`; the reader populates
them. Mirrors C:37363-37372.

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

### `-TAB` algorithm fallback (C:5396–5580) — DONE

When the pixel→WCS chain has no WcsMap, or ForceTab is set, the pair
is described through `map_tab::is_map_tab_2d`, as C does, in
`celestial_axes::celestial_tab`.

### `SkyPole` — LONPOLE / LATPOLE storage — DONE

Ported in
`crates/ast-core/src/fitschan/fset_to_store/sky_pole.rs`. Both
zenithal (PV-3/PV-4 read) and non-zenithal (`wcs_nat_pole` math
helper applying FITS-WCS paper II equations 8-9-10 plus a
post-WcsMap-chain inverse on the celestial north pole) branches
are covered. Wired into `celestial_axes` after `sky_sys`.

### Auto axis-swap with `WorldAxes` (C:5318–5378) — DONE

`WorldAxes` (bidirectional branch) ported in
`crates/ast-core/src/fitschan/fset_to_store/world_axes.rs`. The
swap is wired into `celestial_axes` after `build_wcs_to_iwc`:
pixel→WCS is composed with WCS→IWC, `world_axes` returns the
pixel-axis association per WCS axis, and if that transposes
lon/lat the FITS-side `wperm` / CRVAL / CTYPE / CNAME / PV slots
are swapped. The C `FitsAxisOrder` attribute gate is implicit —
the only mode reachable today is the C-default `"<auto>"`. When
the writer grows a `FitsAxisOrder` knob, gate the swap on it.

The C TAB-only `WorldAxes` branch (forward-only mappings via
`astMapSplit`) is ported as `world_axes_forward`, used when TabOK is
positive.

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

`spectral_axes` now picks RESTWAV (= `c / rest_freq`, in meters)
for wavelength-domain systems (`WAVE`, `AWAV`, `VOPT`, `ZOPT`)
and RESTFRQ (Hz) for the rest. Mirrors C:30740-30751.

### `OBSGEO-X/Y/Z` from SpecFrame observer — DONE

`spectral_axes` now emits OBSGEO from `SpecFrame::obs_lon/lat/alt`
on the primary coord version (using `pal::gd2gc`).

### Axis Label → CTYPE-comment / CNAME — DONE

`SpecFrame::axis_label` / `set_axis_label` / `test_axis_label`
added; `spectral_axes` emits the explicit Label when set.

### `-X2P` non-linear algorithm cascade — DONE

Ported in `spectral_axes::try_x2p_cascade`. Loops over FREQ /
WAVE / AWAV / VELO as the trial X system; for each, builds the
S→X mapping via `SpecFrame::make_spec_mapping`, composes
`pixel→S → S→X` and tests it with `is_map_linear` at the
spectral axis. On success builds the WCS→IWC chain
`S → X → −xval → ·dsbydx` and emits `<orig>-<X>2<P>` CTYPE per
the C suffix table. Mirrors C:30359-30454.

### `GrismMap` handling — DONE

`extract_grism_map` walks a Mapping chain looking for an
un-inverted GrismMap that produces a specified output directly,
mirroring `fitschan.c::ExtractGrismMap`. `try_grism_cascade` in
`spectral_axes` invokes it for each of WAVELEN / AIRWAVE,
verifies the GrismMap-free chain is linear in pixel coords, then
builds the `S → grism_param → w` IWC mapping and emits the
`-GRI` / `-GRA` CTYPE plus the corresponding PV parameters
(G / m / alpha / NR / NRP / eps / theta). Mirrors C:30480-30599.

### `VELOSYS` calculation — DONE

`spectral_axes` makes C's SourceVRF and SourceVel changes
(fitschan.c:31294-31316) on a copy of the SpecFrame and stores the
resulting SourceVel, for every description and in the source frame too.

### SpecFrame unit-conversion chain rewriting — DONE

Three-commit port of `reference/starlink-ast/src/unit.c` (~6.2k lines)
landed on `tickets/DM-54793`:

- **Commit 1:** parser + 61-entry `KnownUnit` + 20-entry SI prefix +
  `RemakeTree` + dim-anal + linear-only `unit_mapper`. 48 tests.
- **Commit 2:** full C pipeline — `InvertTree`, `ConcatTree`,
  `CmpTree`, `FindFactors`/`CombineFactors`, `ComplicateTree`,
  `MakeExp`, `MakeMapping`. `unit_mapper` now produces `MathMap` for
  non-linear conversions (`log(Hz)→Hz`, `sqrt(Hz)→Hz`, etc.).
- **Commit 3:** wired `unit_mapper` into:
  - `SpecFrame::make_spec_mapping` — prepends/appends unit-conversion
    stages at the source/target boundaries when Unit attributes
    differ from the SpecMap canonical default. Returns
    `AstError::Badun` on dimensionally-incompatible units (mirrors C
    `AST__BADUN`).
  - `spectral_axes` (C:30276-30311) — when SpecFrame has a non-linear
    user-Unit (e.g. `log(Hz)`), composes the unit-conversion mapping
    into the pixel→spec chain, transforms CRVAL through it, and
    overrides CUNIT to the canonical default. Linear user-Units
    (e.g. `MHz`) are emitted as CUNIT directly without modifying the
    chain.
  - 4 SpecFrame `convert` tests + 2 spectral_axes integration tests.

**Note on FrameSet auto-remap:** the C library uses
`astClear(fs, "Unit(2)")` to trigger automatic re-composition of the
touching edge. Our FrameSet doesn't yet expose that integrity path;
`spectral_axes` does the equivalent composition manually. Adding
general FrameSet auto-remap-on-attribute-clear is a separate
follow-up if a class needs it beyond the spectral writer pipeline.

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

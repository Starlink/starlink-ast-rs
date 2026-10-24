# Step 7c: SpecFrame, TimeFrame, FluxFrame, SpecFluxFrame — Design Spec

## Summary

Implement the remaining astronomy-specific Frame types: SpecFrame
(spectral coordinates — wavelength/frequency/velocity), TimeFrame
(time coordinates — MJD/JD/UTC/TAI), FluxFrame (flux/brightness
units), and SpecFluxFrame (compound spectral+flux). Also implement
SpecMap (34 spectral conversion types) and TimeMap (28 time
conversion types) — the conversion Mapping types that power these
frames, analogous to SlaMap for SkyFrame.

## Deliverables

1. `SpecMap` — 34 spectral conversion types via PAL.
2. `TimeMap` — 28 time system conversions via PAL.
3. `SpecFrame` — 1-axis spectral frame (10 systems, 9 standards of
   rest).
4. `TimeFrame` — 1-axis time frame (4 systems, 11 time scales).
5. `FluxFrame` — 1-axis passive flux frame (4 systems).
6. `SpecFluxFrame` — compound SpecFrame + FluxFrame.
7. Additional PAL wrappers for new functions.
8. Serializable impls + dispatch updates for all types.
9. Tests ported from C's `testspecframe.c`, `testtime.c`,
   `testflux.c`, `testspecflux.c`.
10. Simplification merge rules (SpecMap + SpecMap, TimeMap + TimeMap).

## Module Structure

```
crates/starlink-ast/src/
├── pal.rs                    # Add ~8 new PAL wrappers
├── mappings/
│   ├── specmap.rs            # SpecMap + SpecCvtType (34 conversions)
│   ├── timemap.rs            # TimeMap + TimeCvtType (28 conversions)
│   └── mod.rs                # Re-exports
├── frames/
│   ├── specframe.rs          # SpecFrame + SpecSystem + StdOfRest
│   ├── timeframe.rs          # TimeFrame + TimeSystem + TimeScale
│   ├── fluxframe.rs          # FluxFrame + FluxSystem
│   ├── specfluxframe.rs      # SpecFluxFrame (compound)
│   └── mod.rs                # Re-exports
├── serial/mod.rs             # Dispatch updates
└── tests/
    ├── test_specmap.rs
    ├── test_timemap.rs
    ├── test_specframe.rs
    ├── test_timeframe.rs
    ├── test_fluxframe.rs
```

## SpecMap

A 1-dimensional Mapping that chains spectral coordinate conversions.
`nin = nout = 1`. Follows the SlaMap pattern exactly.

### SpecCvtType (34 types)

**Direct conversions (arithmetic):**
- FrToVl(1): freq → relativistic velocity: `v = c * (1 - (f0/f)²) / (1 + (f0/f)²)`
- VlToFr(2): velocity → freq: `f = f0 * sqrt((1-v/c)/(1+v/c))`
- EnToFr(3): energy → freq: `f = E / h`
- FrToEn(4): freq → energy: `E = h * f`
- WnToFr(5): wavenumber → freq: `f = c * wn`
- FrToWn(6): freq → wavenumber: `wn = f / c`
- WvToFr(7): wavelength(vac) → freq: `f = c / λ`
- FrToWv(8): freq → wavelength(vac): `λ = c / f`
- AwToFr(9): wavelength(air) → freq (refractive index correction)
- FrToAw(10): freq → wavelength(air)
- VrToVl(11): radio vel → relativistic: `v_rel = ...`
- VlToVr(12): relativistic → radio vel
- VoToVl(13): optical vel → relativistic
- VlToVo(14): relativistic → optical vel
- ZoToVl(15): redshift → relativistic vel
- VlToZo(16): relativistic vel → redshift
- BtToVl(17): beta → relativistic vel: `v = beta * c`
- VlToBt(18): relativistic vel → beta: `beta = v / c`

**Reference-frame velocity corrections (via PAL):**
- UsF2Hl(19), HlF2Us(20): user-defined ↔ heliocentric
- TpF2Hl(21), HlF2Tp(22): topocentric ↔ heliocentric
- GeF2Hl(23), HlF2Ge(24): geocentric ↔ heliocentric
- ByF2Hl(25), HlF2By(26): barycentric ↔ heliocentric
- LkF2Hl(27), HlF2Lk(28): LSRK ↔ heliocentric
- LdF2Hl(29), HlF2Ld(30): LSRD ↔ heliocentric
- LgF2Hl(31), HlF2Lg(32): local group ↔ heliocentric
- GlF2Hl(33), HlF2Gl(34): galactic ↔ heliocentric

The reference-frame conversions apply a Doppler shift based on the
observer/Earth/solar velocity projected along the line of sight.
They use PAL functions for Earth position/velocity (palEvp, palPvobs)
and standard velocities (palRvlsrk, palRvlsrd, palRvgalc, palRvlg).

### Arguments per conversion

Each conversion carries arguments (up to 7):
- Direct (1-18): 1 arg (rest frequency f0)
- User-defined (19-20): 3 args (velocity offset, RA, Dec)
- Topocentric (21-22): 6 args (obs_lon, obs_lat, obs_alt, epoch, RA, Dec)
- Geocentric (23-24): 3 args (epoch, RA, Dec)
- Barycentric (25-26): 3 args (epoch, RA, Dec)
- LSRK/LSRD/LG/Galactic (27-34): 2 args (RA, Dec)

### Serialization

Class: `"SpecMap"`. Attributes: `Nspec` (count), `Spec1`..`SpecN`
(type string), `Spec1A`..`SpecNA` (arguments with A/B/C suffixes).

### try_merge

SpecMap + inverse(SpecMap) → UnitMap (same pattern as SlaMap).
Internal step cancellation (adjacent inverse pairs within one
SpecMap) can be added later.

## TimeMap

A 1-dimensional Mapping for time system conversions. `nin = nout = 1`.
Follows the same SlaMap/SpecMap pattern.

### TimeCvtType (28 types)

**Date conversions (arithmetic):**
- MjdToMjd(1): MJD offset adjustment
- MjdToJd(2), JdToMjd(3): MJD ↔ JD (+/- 2400000.5)
- MjdToBep(4), BepToMjd(5): MJD ↔ Besselian epoch (via palEpb/palEpb2d)
- MjdToJep(6), JepToMjd(7): MJD ↔ Julian epoch (via palEpj/palEpj2d)

**Atomic time (via palDat for leap seconds):**
- TaiToUtc(8), UtcToTai(9): TAI ↔ UTC
- TtToTai(10), TaiToTt(11): TT ↔ TAI (constant offset 32.184s)

**Dynamical time:**
- TdbToTt(12), TtToTdb(13): TDB ↔ TT (relativistic correction)
- TcgToTt(14), TtToTcg(15): TCG ↔ TT
- TcbToTdb(16), TdbToTcb(17): TCB ↔ TDB

**Sidereal time:**
- UtToGmst(18), GmstToUt(19): UT1 ↔ GMST (via palGmst)
- GmstToLmst(20), LmstToGmst(21): GMST ↔ LMST (longitude offset)
- LastToLmst(22), LmstToLast(23): LAST ↔ LMST (equation of equinoxes)

**Clock corrections:**
- UtToUtc(24), UtcToUt(25): UT1 ↔ UTC (DUT1 correction)
- LtToUtc(26), UtcToLt(27): Local Time ↔ UTC (timezone offset)

### Arguments per conversion

Variable (0-7 args): MJD offset, observer position, DTAI, DUT1,
LT offset, etc.

### Serialization

Class: `"TimeMap"`. Attributes: `Ntime` (count), `Time1`..`TimeN`
(type string), `Time1A`..`TimeNA` (arguments).

### try_merge

TimeMap + inverse(TimeMap) → UnitMap. Internal step cancellation
for adjacent inverse pairs.

## SpecFrame

1-axis Frame for spectral coordinates.

### SpecSystem (10 types)

Freq, Energy, Wavenum, Wavelen, Airwave, Vradio, Voptical,
Redshift, Beta, Vrel.

### StdOfRest (9 types)

Topocentric, Geocentric, Barycentric, Heliocentric, Lsrk, Lsrd,
Galactic, LocalGroup, Source.

### Key attributes

- `system`: SpecSystem
- `std_of_rest`: StdOfRest
- `rest_freq`: Hz (needed for velocity↔frequency conversions)
- `ref_ra`, `ref_dec`: source direction FK5 J2000 (for Doppler)
- `source_vel`, `source_vrf`, `source_sys`: source velocity info
- `spec_origin`: origin for offset coordinates
- Observer attrs inherited from Frame trait

### Frame trait implementation

- `naxes() = 1`
- `convert()`: builds SpecMap chain via system→frequency→system
  with std-of-rest correction in between if rest frames differ
- `map_from()`: delta SpecMap when system/rest-frame changes
- `distance()`: absolute difference (1D Euclidean)
- `format()`/`unformat()`: numeric with units

### Serialization

Class: `"SpecFrame"`. Attributes: StdORst, RestFrq, RefRA, RefDec,
SrcVel, SrcVRF, SrcSys, SpecOrigin, AlnStd, AlnSOff.

## TimeFrame

1-axis Frame for time coordinates.

### TimeSystem (4 types)

Mjd, Jd, Jepoch, Bepoch.

### TimeScale (11 types)

Tai, Utc, Ut1, Gmst, Last, Lmst, Tt, Tdb, Tcb, Tcg, Lt.

### Key attributes

- `system`: TimeSystem
- `timescale`: TimeScale
- `time_origin`: zero point
- `lt_offset`: Local Time offset from UTC (hours)
- `align_timescale`: TimeScale for alignment
- Observer attrs inherited

### Frame trait implementation

- `naxes() = 1`
- `convert()`: builds TimeMap chain
- `map_from()`: delta TimeMap when scale/system changes
- `distance()`: absolute difference
- `format()`: ISO date or decimal MJD depending on format string

### Serialization

Class: `"TimeFrame"`. Attributes: TmScl, TmOrg, AlTmSc, LTOff.

## FluxFrame

1-axis passive Frame for flux values.

### FluxSystem (4 types)

FluxDen, FluxDenW, SBright, SBrightW.

### Key attributes

- `system`: FluxSystem
- `spec_val`: spectral position of this flux measurement
- `spec_frame`: Optional<SpecFrame> describing spec_val coordinates

### Frame trait

- `naxes() = 1`
- `convert()`: scaling between flux density per frequency and per
  wavelength (Fλ = Fν * c/λ²)
- Simple 1D Euclidean distance

### Serialization

Class: `"FluxFrame"`. Attributes: SpcVl, plus embedded SpecFrame
if present.

## SpecFluxFrame

Compound of SpecFrame (axis 1) + FluxFrame (axis 2). Essentially
a CmpFrame with attribute forwarding.

### Implementation

```rust
pub struct SpecFluxFrame {
    spec_frame: SpecFrame,
    flux_frame: FluxFrame,
}
```

Implements Frame by delegating to a CmpFrame-style compound.
Special: setting "StdOfRest" on the SpecFluxFrame forwards to
the SpecFrame component.

### Serialization

Class: `"SpecFluxFrame"`. Contains embedded SpecFrame and FluxFrame
as child objects.

## PAL Additions

Add to `crates/starlink-ast/src/pal.rs`:

```rust
pub(crate) fn dat(mjd: f64) -> f64;        // TAI-UTC (leap seconds)
pub(crate) fn pvobs(lat: f64, alt: f64, lst: f64) -> [f64; 6]; // observer velocity
pub(crate) fn rvgalc(ra: f64, dec: f64) -> f64;  // galactic rotation vel
pub(crate) fn rvlg(ra: f64, dec: f64) -> f64;    // local group vel
pub(crate) fn rvlsrd(ra: f64, dec: f64) -> f64;  // LSRD vel
pub(crate) fn rvlsrk(ra: f64, dec: f64) -> f64;  // LSRK vel
pub(crate) fn gmst(mjd: f64) -> f64;       // Greenwich mean sidereal time
pub(crate) fn eqeqx(mjd: f64) -> f64;      // equation of equinoxes
```

## Testing Strategy

### Ported from C test suite:

- `testspecframe.c` → `test_specframe.rs`: system conversions,
  standard-of-rest corrections, rest frequency handling
- `testtime.c` → `test_timeframe.rs`: time scale conversions,
  epoch round-trips, leap second handling
- `testflux.c` → `test_fluxframe.rs`: flux unit conversions
- `testspecflux.c` → `test_specfluxframe.rs`: compound frame

### Unit tests:

- SpecMap: all 34 conversion types with known physical constants
  (c = 299792458 m/s, h = 6.62607015e-34 J·s)
- TimeMap: all 28 conversions, leap second table validation
- Round-trip: freq → wavelength → freq = identity
- Round-trip: MJD → JD → MJD = identity
- Serialization round-trip for all types
- Simplification regression: `spec_*` and `time_*` tests should
  start passing

## What This Step Does NOT Include

- MathMap (expression evaluator) — separate step
- Channel/FitsChan — Step 8
- Regions — Step 9
- DSS/Grism/PCD specialized mappings

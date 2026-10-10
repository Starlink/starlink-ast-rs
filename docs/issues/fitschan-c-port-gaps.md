# `fitschan.c` C-vs-Rust Port Gaps

**Status: PARTIALLY DONE**, and the shape of what remains changed between this
document's body and the code. Re-checked against the tree at the AST 9.5.0 pin
(2026-09-25); the body below still describes several things as gaps that are now
ported, so read this header first where the two disagree.

Ported since the body was written: DSS (`enc_dss.rs`), SAO, DSBSetUp,
SipReplace, the `-ZPX` distortion PolyMap, DTAI, AXREF and Warn-card emission.
All 194 wcsconv cases are byte-compared with `NATIVE_READ_EXCLUDED` empty.
`CarLin` is not "silently ignored" either: it is a struct field with
`set_car_lin`, honoured by the harness at `test_wcsconv.rs:246`.
`FitsTol` and `SipOK` were plumbed but unreachable — fixed at
`FITS_TOL_DEFAULT = 0.1` and hardcoded `true`; both became reachable in the
9.5.0 sync, `FitsTol` through `set_fits_tol`/`fits_tol` and `SipOK` derived from
the encoding at `fset_to_store/mod.rs:260`.

What remains is the **attribute surface**, not the read-path algorithms:

- **No accessor at all** — `Card`, `Clean`, `Iwc`, `PolyTan`, `FitsAxisOrder`,
  `IgnoreBadAlt`, `AltAxes`, `Warnings`. `Card` is the substantive one: 540
  internal references to card position and no `set_card` / `get_card`, so a
  client cannot drive the cursor.
- **Blocked on FitsTable** — `TabOK`, `ForceTab`.
- **About thirteen public functions** — `astPurgeWCS`,
  `astSetCard` / `astGetCard`, `astTestFits`, `astShowFits`, and the typed card
  accessors `astSetFitsCF` / `CI` / `CN` / `T` / `K`. `astRetainFits` has its
  foundations in the consumed-card work but is not ported.

`-TAB` is three unported things rather than a branch to enable: `Table`,
`FitsTable`, and the algorithm (`AdaptLut`, `PutTable`, `IsMapTab1D` / `2D`).
`PLAN.md` §3 is where that blocker is recorded.

This document catalogues C `fitschan.c` logic that is not yet ported
to the Rust `crates/ast-core/src/fitschan/` tree.
Companion to the existing
[`fset_to_store-port-gaps.md`](fset_to_store-port-gaps.md), which
covers the `fset_to_store/` writer-side branches in detail; this
file covers the rest of the C file.

Audit method: enumerate C's static functions, cross-reference each
against the Rust source.
Functions whose C name has zero or near-zero Rust mentions are
investigated individually.
The fset_to_store gaps are not duplicated here.

The same rule from `fset_to_store-port-gaps.md` applies:
**lack of fixture coverage is not a reason to skip a C branch.**
Each entry below should ultimately become a test fixture + faithful
port + entry deletion.

---

## Reader gaps

### DSS — non-linear plate solution

**File:** `crates/ast-core/src/fitschan/enc_dss.rs`
**C reference:** `DSSToStore` (fitschan.c:9066+) and `DSSFromStore`.

Rust `read_from_cards` only reads `AMDX1`/`AMDY1` (linear plate scale)
and ignores the 20-element AMDX2..AMDX20 / AMDY2..AMDY20 polynomial
plate-solution coefficients.
C builds a `DssMap` from those coefficients to model the photographic
plate distortion.

**Affects test fixtures:** `dss_ast_dss`, `dss_ast_fits_wcs`, `dss_fits_dss_ast`
(all currently failing, all listed as out-of-scope in
`docs/issues/wcsmap-naxis-refactor.md`).

**Action:** port `DSSToStore` and `DSSFromStore` faithfully.
Requires the underlying `DssMap` Rust class to have its full
plate-solution form (it currently exists at
`crates/ast-core/src/mappings/dssmap.rs` — verify whether it
covers the polynomial path).

---

### `-ZPX` distortion mapping

**File:** none (gap).
**C reference:** `DistortMaps` (fitschan.c:8668+) `-ZPX` branch
(lines 8836-8884) plus `ZPXMapping` and `WATCoeffs` helpers.

Rust spec_trans translates `-ZPX` CTYPE to `-ZPN-ZPX` and decodes
WAT coefficients into PVi parameters
(`spec_trans.rs::wat_coeffs`), but the **distortion-mapping
construction itself is not ported**.
C `ZPXMapping` builds a 2-axis distortion `PolyMap` post-WcsMap and
adds it in series with map4 (post-CDELT IWC) of the
pixel→IWC chain.

The current `zpx_native` test passes because it uses an
`astequal`-style numerical compare (manifest's `yes` flag for
skip_string_compare).
A bit-exact `_fits-wcs` round-trip would expose the gap.

**Action:** port `ZPXMapping`, hook into the read path's
post-WcsMap chain.

---

### SAO encoding translator

**File:** none (gap).
**C reference:** `SAOTrans` (fitschan.c:26630+).

C `SAOTrans` translates SAO-encoded headers (SAO Image WCS — uses
`CO1_NN` / `CO2_NN` polynomial coefficient keywords) into
TPN-encoded headers, run as part of `SpecTrans`.
Rust has no SAO-format support; SAO-encoded headers cannot
currently be read.

**Action:** port `SAOTrans` into `crates/ast-core/src/fitschan/spec_trans.rs`.
Add a SAO test fixture
(WCSTools' `imhead`/`platepos` output is the canonical source).

---

### DSBSpecFrame setup from FITS-CLASS

**File:** `crates/ast-core/src/fitschan/wcs_spectral.rs`
**C reference:** `DSBSetUp` (fitschan.c:8930+) called from
`WcsSpectral`.

Rust `build_spectral` always constructs a plain `SpecFrame`, even
when the input header carries `IMAGFREQ` (the marker for
double-sideband data).
C, when IMAGFREQ is present, builds a `DSBSpecFrame` and uses the
RESTFREQ + IMAGFREQ pair to compute the local-oscillator frequency
and configure DSBCentre / IF / SideBand.

The writer side reads IMAGFREQ from a DSBSpecFrame and re-emits it
when present, so a FrameSet that *already* contains a
DSBSpecFrame (e.g. native-input) round-trips correctly.
A FITS-CLASS input read via the Rust reader, however, loses the
DSB structure.

**Affects test fixtures:** `jan_fits_class_ast`, `jan_fits_class_rt`
(both currently failing, listed as out-of-scope in
`wcsmap-naxis-refactor.md`).

**Action:** port `DSBSetUp` and wire it into `wcs_spectral`'s
SpecFrame construction.

---

### TELESCOP-gated `DTAI` keyword

**File:** `crates/ast-core/src/fitschan/wcs_keywords.rs:231`
**C reference:** `WcsFcRead` (fitschan.c:36166+) DTAI branch.

C only reads `DTAI` when `TELESCOP` matches `JCMT*` or `UKIRT*`,
because other telescopes may use the keyword for unrelated
purposes.
Rust reads it unconditionally.

**Impact:** non-JCMT/UKIRT headers that carry an unrelated `DTAI`
keyword (rare but possible) will get a spurious TAI-UTC offset
applied to their epochs.
No current fixture exercises this; latent.

**Action:** gate the Rust read on `TELESCOP` prefix matching the
JCMT/UKIRT set.
Two-line fix.

---

### `AXREF{i}` keyword (for `-TAB` axes)

**File:** none (gap).
**C reference:** `WcsFcRead` (fitschan.c ~36400) `AXREF%d` branch.

`AXREF{i}` carries the pixel reference position for `-TAB`
algorithm axes.
Rust does not read it.
Same blocker family as the `-TAB` algorithm (which depends on
`FitsTable` infrastructure, see `fset_to_store-port-gaps.md`).

**Action:** ports as part of the `-TAB` work.

---

### `Warn` / `ASTWARN` card emission

**File:** scattered (e.g. `crates/ast-core/src/fitschan/spec_trans.rs:135, 679`).
**C reference:** `Warn` (fitschan.c) and the `Warnings` attribute
machinery.

C emits an `ASTWARN` comment card to the FitsChan when something
non-fatal needs the caller's attention (unrecognised distortion,
incomplete translation, ambiguous keyword, etc).
The set of warnings to emit is governed by the `Warnings`
attribute (a comma-separated list of warning categories).

Rust's `FitsChan::warnings` is a `Vec<String>` with no public
accessors and no `Warn` helper.
Comments in `spec_trans.rs` say "C issues an ASTWARN; we
silently leave..." in several places.

**Impact:** debugging information lost on the Rust path.
Some downstream tooling may rely on the warning cards being
present.

**Action:** add a `Warn(category, message)` helper that gates on
`Warnings` and pushes both to the `warnings` Vec (already there)
and as an `ASTWARN` card to the card list.
Wire it into the call sites where the comments say "we silently leave".

### Axis count of a foreign-encoding header

**File:** `crates/ast-core/src/fitschan/enc_aips.rs` (and `enc_pc.rs`, `enc_iraf.rs`).
**C reference:** `FitsToStore` (fitschan.c:11517-11540).

C reads every foreign encoding other than DSS through `SpecTrans` and `WcsToStore`, then counts the axes as the highest index of any primary or alternate CRPIX keyword.
The port's FITS-AIPS, PC and IRAF readers count them from `WCSAXES`, `NAXIS` or the highest `NAXISn`, and refuse a header with none of those.

**Impact:** the FITS-WCS header of `testtrangrid.c` (CROTA1, no NAXIS), which C reads, is refused.
`test_trangrid.rs::tan_header_without_naxis_is_not_read` fails when the port reads it.

**Action:** count the axes as C does.
The FITS-WCS reader's `infer_axis_count_from_indexed_keywords` is a different rule and should be checked against C's at the same time.

---

## Channel-level gaps to take up with this audit

PLAN.md §5, row "Channels as objects", lists what remains after `NativeChannel` and `MocChan` became objects (2026-10-09): FitsChan is not an `AnyObject` variant and cannot be dumped, the Object `ID` attribute is absent, `NativeChannel` ignores `Skip` and `Strict` and reads all its text at once, and no class writes non-helpful unset attributes at `Full` above zero.
FitsChan's own `Dump` writes its cards and attributes, so the first item belongs with this audit.

---

## FitsChan attribute coverage

C exposes 18 attributes via `Get`/`Set`/`Test`/`Clear` quartets.
Rust currently models 5.

| Attribute | C | Rust | Notes |
|---|---|---|---|
| `Encoding` | yes | **yes** | |
| `FitsDigits` | yes | **yes** | |
| `FitsRounding` | yes | **yes** | |
| `CDMatrix` | yes | **yes** | |
| `CarLin` | yes | **partial** | wcsconv harness has fixtures using `carlin=1` (car1, car2, car4_native) but Rust silently ignores the attribute |
| `Card` (current-card position) | yes | no | no `set_card`/`get_card`; clients can't iterate the FitsChan via the cursor |
| `Clean` | yes | no | controls whether read consumes input cards |
| `FitsAxisOrder` | yes | no | only "<auto>" mode reachable; documented in fset_to_store gap doc |
| `FitsTol` | yes | no | linearity tolerance for FITS-WCS extraction; default 0.1 used |
| `ForceTab` | yes | no | force `-TAB` algorithm; blocked on FitsTable port |
| `IgnoreBadAlt` | yes | no | controls behaviour on bad alt-plane |
| `Iwc` | yes | no | controls whether the IWC frame is added to the FrameSet |
| `PolyTan` | yes | no | enables `-PV` polynomial TAN (silently ignored — fixture `tan-polytan_native` passes via astequal) |
| `SipOK` | yes | **partial** | the harness reads `SipOK=1` from the manifest but ignores it; SIP path is on by default and SipOK off is unsupported |
| `SipReplace` | yes | **partial** | same — silent-ignore; default behaviour assumes `sipreplace=1` |
| `TabOK` | yes | no | controls `-TAB` enablement; FitsTable blocker |
| `Warnings` | yes | **partial** | warnings vec exists; no public attribute |
| `AltAxes` | yes | no | controls alt-plane axis ordering |

The wcsconv test harness
(`crates/ast-core/tests/test_wcsconv.rs:160-178`) explicitly
notes "carlin, sipreplace, sipok -- not yet supported.".
Several fixtures pass these attributes through the manifest;
they're silently ignored.

**Action:** for each attribute that's referenced by an existing
fixture, port the Get/Set/Test/Clear quartet and wire it into
the relevant read/write code path.
The `tan-polytan_native` and `sip-alt_native` fixtures pass via
astequal (the `yes` flag in the manifest); a bit-exact
`_fits-wcs` round-trip would expose the SipReplace / PolyTan
attribute gaps.

---

## Public API gaps

These C public functions have no Rust counterpart in
`crates/ast-core/src/fitschan/mod.rs`:

| C function | Purpose | Rust |
|---|---|---|
| `astPurgeWCS` | delete all WCS cards from the FitsChan | not ported |
| `astRetainFits` | mark current card as not-used so sweep skips it | not ported |
| `astSetCard` / `astGetCard` | move/read current-card cursor | not ported |
| `astTestFits` | does keyword exist with expected type? | not ported |
| `astShowFits` | print all cards (debug) | not ported |
| `astReadFits` / `astWriteFits` | per-card iterator helpers | not ported (CFFI-side has these) |
| `astSetFitsCF` / `astGetFitsCF` | complex-float values | not ported |
| `astSetFitsCI` / `astGetFitsCI` | complex-int values | not ported |
| `astSetFitsCN` / `astGetFitsCN` | continued-string values | not ported |
| `astSetFitsT` / `astGetFitsT` | table-reference values | not ported |
| `astSetFitsK` / `astGetFitsK` | 64-bit integer values | not ported (`get_fits_i` / `set_fits_i` use i64 internally — the gap is the dedicated 64-bit `K` typecode) |

Most matter only for niche FITS dialects (continued strings, complex
amplitudes, table references); a few are necessary for general
client iteration over the chan (`astSetCard` / `astGetCard`).

**Action:** scope each individually as a downstream Rust API decision.
For a port targeting bit-exact round-tripping of the existing
fixture set, none of these are blockers.

---

## Catalog of low-or-zero-mention C functions

The audit's cross-reference scan flagged these C symbols with zero
or near-zero matches in Rust source.
Each is summarised here for completeness; the substantive ones
have their own section above.

| C function | Rust mentions | Status |
|---|---|---|
| `AdaptLut` | 0 | `-TAB` blocker (FitsTable infrastructure) |
| `DistortMaps` | 0 | `-SIP` ported; `-ZPX` not (see [§-ZPX](#-zpx-distortion-mapping)) |
| `DSBSetUp` | 0 | gap (see [§DSBSpecFrame](#dsbspecframe-setup-from-fits-class)) |
| `DSSFromStore` | 0 | partial (see [§DSS](#dss--non-linear-plate-solution)) |
| `DSSToStore` | 0 | partial (see [§DSS](#dss--non-linear-plate-solution)) |
| `HasAIPSSpecAxis` | 0 | ported by behaviour (`guess_encoding::has_aips_spectral_ctype`); just no name match |
| `LooksLikeClass` | 0 | ported by behaviour (`guess_encoding::looks_like_class`); just no name match |
| `PutTable` / `PutTables` | 0 | `-TAB` blocker |
| `DateObs` | 1 | ported (`mjd_to_date_str` and the inverse in `wcs_keywords.rs::read_epoch_into_store`) — just a different name |
| `WATCoeffs` | 1 | ported as `wat_coeffs` in `spec_trans.rs` |
| `WcsFcRead` | 1 | ported as `wcs_keywords::extract_wcs_keywords`; verified keyword set covers TELESCOP/JCMT/UKIRT (gap noted in [§DTAI](#telescop-gated-dtai-keyword)) and AXREF (gap noted in [§AXREF](#axrefi-keyword-for--tab-axes)) but otherwise comparable scope |
| `SAOTrans` | 1 | gap (see [§SAO](#sao-encoding-translator)) |
| `IsAIPSSpectral` | 2 | ported by behaviour as `is_aips_spectral` |
| `PCFromStore` | 2 | ported as `enc_pc::write_to_cards` |
| `SplitMat` | 2 | ported as `wcs_decompose::split_cd` |
| `WcsToStore` | 2 | ported as part of `enc_*::read_from_cards` |
| `IRAFFromStore` | 3 | ported as `enc_iraf::write_to_cards` |
| `SplitMap` | 3 | ported as `celestial_axes::split_map2` |
| `AddEncodingFrame` | 4 | ported as `fset_to_store::add_encoding_frame_class` |
| `AIPSPPFromStore` | 4 | ported as `enc_aips::write_to_cards_pp` |
| `ClassTrans` | 4 | ported as `spec_trans::translate_class` |
| `FindLonLatSpecAxes` | 4 | ported as `find_lon_lat_spec_axes` (in `fset_to_store/mod.rs`) |
| `AIPSFromStore` | 5 | ported as `enc_aips::write_to_cards` |
| `FitsAxisOrder` | 5 | attribute not ported (only "<auto>" reachable) — see attribute table |
| `MakeBasisVectors` | 6 | ported as `wcs_decompose::make_basis_vectors` |
| `NearestPix` | 6 | ported as `spectral_axes::nearest_pix_value` |
| `AddVersion` | 7 | ported as `wcs_write::encode_wcs` (which iterates per coord-version letter) |
| `FindWcs` | 7 | ported as `card_list::is_findwcs_keyword` (the keyword classifier) |
| `TDBConv` | 7 | ported as `wcs_write::tdb_mjd_to_utc` / `utc_mjd_to_tdb` |
| `CLASSFromStore` | 9 | ported as `enc_class::write_to_cards` |
| `TidyOffsets` | 9 | ported as `wcs_read::tidy_offsets` |
| `IsSkyOff` | 11 | ported as `sky_sys::is_sky_off` |
| `FindBasisVectors` | 15 | ported as `wcs_decompose::find_basis_vectors` |
| `WcsFromStore` | 15 | ported as `wcs_write::encode_wcs` and per-encoding writers |
| `SkyPole` | 16 | ported as `fset_to_store::sky_pole::compute_sky_pole` |
| `WorldAxes` | 19 | ported as `fset_to_store::world_axes::world_axes` |
| `AnalysePoly` | 20 | ported as `mappings::helpers::analyse_poly::analyse_poly` |
| `WcsNatPole` | 21 | ported as `fset_to_store::sky_pole::wcs_nat_pole` |
| `IsMapLinear` | 25 | ported as `fset_to_store::is_map_linear::is_map_linear` |
| `MakeIntWorld` | 41 | ported as `fset_to_store::make_int_world::make_int_world` |
| `AddFrame` | 44 | ported as `FrameSet::add_frame` (different module) |
| `SkySys` | 61 | ported as `fset_to_store::sky_sys::sky_sys` |
| `Split` | 69 | ported as multiple Rust string utilities |

---

## Priorities

If a future session picks up these gaps, suggested order
(highest impact first):

1. **DSS polynomial plate solution** — closes 3 currently-failing
   wcsconv tests (`dss_ast_dss`, `dss_ast_fits_wcs`,
   `dss_fits_dss_ast`).
2. **DSBSetUp** + DSBSpecFrame on the read path — closes
   `jan_fits_class_*` (2 tests).
3. **TELESCOP-gated DTAI** — small fix, removes a latent bug.
4. **`-ZPX` distortion mapping** — adds bit-exact ZPX round-trip
   support (the current `zpx_native` is astequal-only).
5. **SipReplace / PolyTan attributes** — required for bit-exact
   round-trip of `tan-polytan` and `sip-alt` fixtures (currently
   astequal-only).
6. **SAOTrans** — adds support for SAO-encoded headers.
7. **`-TAB` family** (AdaptLut, IsMapTab1D/2D, AXREF, ForceTab,
   TabOK, PutTable/PutTables) — blocked on FitsTable port; do
   together once that's available.
8. **Public-API gaps** (`astPurgeWCS`, `astRetainFits`,
   `astSetCard`, etc.) — only when downstream Rust client code
   needs them.
9. **`Warn` / `ASTWARN` card emission** — quality-of-life, not
   correctness.

## Write-path gaps found alongside the 9.5.0 SIP fixes

Recorded while porting upstream's `c384289f` (report a FITS-WCS write that could
not describe the object). None of these is reached by the current corpus, so each
needs the fixture named as the thing that would expose it.

- **The SIP block sits outside the per-version loop.**
  `WcsFromStore` emits SIP coefficients once; C reaches its SIP writer inside the
  loop over version letters. A header whose alternate description also carries
  distortion would differ. **What would expose it**: a FrameSet with a SIP
  distortion on a non-blank version letter.
- **`AltAxes` is unmodelled.** The attribute has no accessor and the writer has
  no equivalent of the per-version axis selection it controls.
- **`write_cd_matrix` writes a diagonal zero where C writes nothing.**
  The port's guard is `val != 0.0 || i == j` (`wcs_write/mod.rs:799`); C's is
  `val != 0.0` alone. A store with an explicit zero on the diagonal therefore
  gains a `CDi_i = 0.0` card the C library omits. **What would expose it**: a
  store with a zero diagonal CD element — which the zero-CDELT→1.0 substitution
  on the read path normally prevents, which is why it is latent.
- **`write_pc_cdelt` compares against a literal `1e-15`** where C uses
  `astEQUAL` (`wcs_write/mod.rs:892`). The two differ in scale handling:
  `astEQUAL` is relative to the operands, the literal is absolute, so they
  disagree for PC elements far from unity. This is a tolerance with no analogue
  in the C source and should become `ast_equal`.
- **`write_pc_cdelt` cannot fail where C's CDELT branch can.**
  C's `PCFromStore` CDELT loop sets `ok = 0; goto next` on a bad value
  (`fitschan.c:24833-24838`), abandoning the whole version; the port's function
  returns `AstResult<()>` but has no such path — a bad CDELT becomes a written
  card. **What would expose it**: a store with `AST__BAD` in `cdelt`.
- **C's `PCFromStore` loops version letters where the port marks FitsPc
  primary-only.** C runs `for( s = ' '; s <= sup; s++ )`
  (`fitschan.c:24742`) and checks each secondary version's PC matrix against the
  primary with `astEQUAL` (`:24815`), rejecting the version when they differ,
  because the encoding shares one PC matrix across versions. The port has no
  equivalent check. **What would expose it**: a FITS-PC-encoded FrameSet with two
  descriptions whose PC matrices disagree, which C withholds and the port would
  write.

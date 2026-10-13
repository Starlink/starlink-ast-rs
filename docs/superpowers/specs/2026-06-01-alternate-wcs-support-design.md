# Alternate-WCS support — design

## Why

The FITS-WCS standard lets a single header carry up to 26 alternate
coordinate descriptions. The primary plane uses bare `CTYPE1`,
`CRVAL1`, etc., and each alternate plane uses a single-letter suffix
(`CTYPE1A`, `CRVAL1A`, ..., `CTYPE1Z`). The C library models this by
representing every per-axis keyword in the FitsStore as
`store->ctype[axis][version]` — read populates the matrix; write
loops `s = ' ' .. 'Z'` and emits one card-set per non-empty version.

Our Rust port currently:

1. **Read side** discards every `*A`–`*Z` axis keyword
   (`extract_wcs_keywords` reads only `CRVAL{i}`, never `CRVAL{i}A`),
   so a multi-WCS input header parses into a 2-frame FrameSet
   (base + primary current) instead of the
   `2 + nalt` frames the C library produces.
2. **Write side** has alt-plane orchestration *only* on the FITS-WCS
   encoder (`store_from_frameset_with_alts`), and even then the orchestrator
   has no alt frames to enumerate because the reader didn't build any.
   The non-FITS-WCS encoders (FITS-PC, FITS-IRAF, FITS-AIPS,
   FITS-CLASS) emit only the primary plane.
3. **Global keyword overwrite semantics.** C's
   `SetItem(&store->mjdobs, 0, 0, ' ', ep, status)` (fitschan.c:29903)
   forces an `s=' '` slot regardless of which version letter SkySys
   is processing — so when the reader has built (FK5 primary +
   GALACTIC alt-A), the alt-A SkySys call overwrites the primary's
   MJD-OBS with the GALACTIC default-equinox-derived value
   (B1950 ≈ MJD 33281.923 for the `fits-pc-gal` fixture). Our port
   skips this overwrite entirely, so alt-bearing fixtures fail on
   the global keywords.

This drops a lot of wcsconv coverage: every fixture with `*A` keywords
(`multi-wcs`, `multi-wcs-dup`, `fits-pc-gal`, `timj`, `lsst`, etc.)
fails. The hole is structural — the legacy heuristic writer didn't
read alt planes either, so the regression is pre-existing rather than
introduced by the `make_int_world` rewiring.

## Goals

- Reader builds a FrameSet with one frame per non-empty version
  letter (primary + each alt), wired to the base via direct
  `Lnk{i} = base` edges (matching the C library's native dump).
- Each alt frame carries `Ident=letter` so the writer can recover
  the version letter on round-trip.
- Writer loops the version-letter set, runs the existing
  `fset_to_store` pipeline once per letter, emits per-axis cards
  with the appropriate suffix, and resolves the *global* keywords
  (MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS, DTAI, DUT1) once after every
  plane has been processed — last-plane-wins.
- All five FITS-style encoders (FITS-WCS, FITS-PC, FITS-IRAF,
  FITS-AIPS / FITS-AIPS++, FITS-CLASS) honour alt planes.
- Bit-exact match against the C oracle's primary axis-indexed
  keywords for every fixture; bit-exact match for the alt-axis
  keywords on the FITS-WCS encoder. Per-fixture residual
  classification (bucket A/B/C/D) walks the rest in Group 6.

## Non-goals

- Read-side parsing of header text outside of `*A..*Z` axis keywords
  (e.g. NCP normalisation per alt plane is out of scope; the existing
  `normalise_ncp_to_sin` call only needs to run once on the primary
  store as today).
- Separate per-plane MJD-OBS storage in the FitsStore. The chosen
  design uses a single global slot whose value is overwritten by
  each successive plane's SkySys call, matching C exactly.

## Architecture

### High-level data flow

```
header cards (CardList)
  ├── extract_wcs_keywords(cards, ' ')             → primary FitsStore
  └── for letter ∈ detected_alt_letters(cards):
        extract_wcs_keywords(cards, letter)        → alt FitsStore
                                                     (per-axis fields fall
                                                      back to primary when
                                                      the *{letter} key
                                                      is absent — matches
                                                      C GetItem)

decode_wcs_with_alts(primary, alts)
  ├── primary frame   → current frame, Ident=" "
  └── per alt store   → SkyFrame / SpecFrame with Ident=letter,
                        added with Lnk = base
                        (FrameRef::Base parent)

⤵ FrameSet (Nframe = 2 + len(alts))

writer (per encoding):
  planes = [(' ', current_idx)] ++ frames_with_version_letters(fs)
  per_plane_stores = []
  for (s, idx) in planes:
    fs.set_current(idx)
    store = fset_to_store(fs, dim, encoding, s)
    per_plane_stores.push((s, store))

  emit:
    for (s, store) in per_plane_stores:
      emit_axis_indexed_cards_with_suffix(store, s, encoder)
    emit_global_cards_from(per_plane_stores.last())
```

### Component changes

#### 1. `wcs_keywords::extract_wcs_keywords` (read)

Add a `s: char` parameter. Construct keys via a helper:

```rust
fn key_for(prefix: &str, axis: usize, s: char) -> String {
    if s == ' ' { format!("{prefix}{axis}") }
    else { format!("{prefix}{axis}{s}") }
}
```

Per-axis fields query `key_for(...)`; a card-not-found result for an
alt plane (`s != ' '`) falls back to the primary key
`format!("{prefix}{axis}")`, mirroring C `GetItem`. Scalar fields
(LONPOLE, LATPOLE, EQUINOX, RADESYS) get the same `s`-suffix
treatment with primary fallback.

Add `extract_wcs_keywords_with_alts(cards) -> (FitsStore, Vec<(char, FitsStore)>)`.
The detected alt letters are the set of `c ∈ 'A'..='Z'` for which any
of `CTYPE1c`, `CTYPE2c`, …, `CRPIX1c`, `CRVAL1c` appears in the cards
(matches C's per-version-letter axis-keyword scan in `WcsFromStore`).

#### 2. `wcs_read::decode_wcs_with_alts`

```rust
pub(crate) fn decode_wcs_with_alts(
    primary: &FitsStore,
    alts: &[(char, FitsStore)],
) -> AstResult<AnyObject>
```

Builds the primary FrameSet via the existing `decode_wcs` body. For
each `(letter, store)`:

- Build the alt world frame and its pixel→alt-world mapping using the
  same `wcs_celestial::build_celestial` / `wcs_spectral::build_spectral`
  / `build_world_frame` helpers — these are already pure functions of
  a `FitsStore`.
- Set `Ident = letter.to_string()` on the alt frame
  (see component 3).
- `frameset.add_frame(FrameRef::Base, alt_mapping, alt_world_frame)`.
  This produces `Lnk{i} = 1` in the native dump. `add_frame` stores
  the mapping verbatim; the writer's `mapping(FrameRef::Base, ...)`
  query then calls `simplified()` on retrieval — same path the
  primary frame already takes — so primary and alt are treated
  symmetrically.

For an alt plane that resolves to a SkyFrame with no RADESYS, no
EQUINOX, and `DefB1950=true` (the AST default), the SkyFrame's
Equinox attribute is set to `palEpb2d(1950)` and Epoch to the same
value (mirroring `WcsSkyFrame` at fitschan.c:39288-39296,
39214-39222). This is what produces the 33281.923 MJD-OBS in the
fits-pc-gal fixture: the GALACTIC alt's `WcsSkyFrame` runs the
NORADEC + DefB1950 fallback and stamps Epoch = B1950 on the
GALACTIC SkyFrame.

`FitsChan::read` is updated to call `decode_wcs_with_alts` for every
FITS-* encoding.

#### 3. Frame `Ident` setters

Add public `set_ident(&mut self, ident: impl Into<String>)` methods on
`BasicFrame`, `SkyFrame`, `SpecFrame`, `TimeFrame`, `CmpFrame`. These
write to the existing `ident: Option<String>` field. The native
serialiser already round-trips `Ident=`; no Channel-format changes
needed.

#### 4. `fset_to_store::fset_to_store` (write)

Currently has the signature:

```rust
pub(crate) fn fset_to_store(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
) -> AstResult<FitsStore>
```

Hardcodes `s = ' '` at every internal SkySys / spectral_axes /
other_axes call. Add `s: char` as the new last parameter, threaded
through to those callees. They already accept `s` (it was wired
during the original port for exactly this future use).

The `naxis = max(fs.base().naxes(), nwcs)` calculation is unchanged
because each plane writes its own per-axis cards.

#### 5. Per-encoder write retrofit

A new shared helper consolidates the loop:

```rust
pub(crate) struct PlaneStore {
    pub letter: char,
    pub store: FitsStore,
}

pub(crate) fn collect_plane_stores(
    fs: &FrameSet,
    dim: &[Option<f64>],
    encoding: Encoding,
) -> AstResult<Vec<PlaneStore>>
```

This returns `[(' ', primary_store), ('A', alt_a_store), ...]` in
plane-iteration order: primary first, then alphabetic alt order.

Each FITS-style encoder is updated to:

1. Call `collect_plane_stores`.
2. Loop the planes, emitting per-axis cards with the suffix
   appended to each keyword name. `' '` means no suffix.
3. After the loop, emit the *global* cards (see below) once,
   reading from `per_plane_stores.last().unwrap().store`. This
   gives the last-plane-wins behaviour the C library produces by
   re-using a single `s=' '` slot.

The five encoders affected are:

- `wcs_write::encode_wcs` (FITS-WCS) — already plane-aware via
  `store_from_frameset_with_alts`, but the global-card emission
  needs to switch from `primary.mjdobs` to `last_plane.mjdobs`.
- `enc_pc::write_to_cards` (FITS-PC).
- `enc_iraf::write_to_cards` (FITS-IRAF).
- `enc_aips::write_to_cards` (FITS-AIPS / AIPS++).
- `enc_class::write_to_cards` (FITS-CLASS).

Each encoder currently consumes a single `&FitsStore`. The retrofit
gives each one a `&[PlaneStore]` and lets the encoder render its
own plane-aware card stream.

#### 6. Global keyword set

These FitsStore fields are *global to the observation*, not per-axis-
version. The writer emits each at most once per FrameSet, taking the
value from the **last** plane store after the per-plane loop — last
plane (alphabetical order) wins, matching C's hardcoded
`SetItem(..., ' ', ...)` overwrite.

| Field | FITS card | C source |
|-------|-----------|----------|
| `mjdobs` | MJD-OBS | fitschan.c:29903 |
| `obsgeox` | OBSGEO-X | C `SkySys` body, near OBSGEO writes |
| `obsgeoy` | OBSGEO-Y | C `SkySys` body, near OBSGEO writes |
| `obsgeoz` | OBSGEO-Z | C `SkySys` body, near OBSGEO writes |
| `timesys` | TIMESYS | fitschan.c:29819 |
| `dtai` | DTAI | C `SkySys` body |
| `dut1` | DUT1 | C `SkySys` body |

`mjdobs`, `obsgeox/y/z`, `timesys` already exist on the Rust
`FitsStore`. `dtai` and `dut1` do **not** yet — they need to be
added (alongside their Frame-attribute counterparts on
`BasicFrame`/`SkyFrame`) so that round-trip through
`Channel`/`FitsChan` preserves them. This is necessary for fully
green wcsconv: fixtures that exercise dtai/dut1 cannot pass without
the storage. Group split:

- **Group 5** (writer retrofit): treats every present global field
  identically — last-plane-wins emission. If `dtai`/`dut1` are not
  yet on `FitsStore`, the writer simply doesn't emit them, matching
  current behaviour.
- **Group 6** (validation, this design): adds `dtai`/`dut1` storage
  to `FitsStore` (and the Frame-side getters/setters so SkySys can
  populate them), wires reader and writer, then walks the wcsconv
  residual list. Adding the fields late means the rest of the
  groups don't carry an empty-shell schema change.

Header references in the table above are approximate; re-grep for
each keyword inside `SkySys` to pin the exact lines when porting.

A doc-comment on `FitsStore::mjdobs` (and the other six fields)
records the contract:

```rust
/// Global per-FrameSet field. When alt-WCS planes are present, the
/// writer's last-plane-wins rule (alphabetical order) chooses which
/// plane's value lands in the emitted MJD-OBS card. Mirrors C's
/// `SetItem(&store->mjdobs, 0, 0, ' ', ...)` at fitschan.c:29903
/// which forces s=' ' regardless of which coordinate-version
/// SkySys is processing.
pub mjdobs: Option<f64>,
```

### Round-trip invariants

For any header H processable by the C library:

1. `read(H)` → FrameSet with `Nframe = 2 + nalt(H)`. Frame indices:
   - 1: GRID (base, pixel coords)
   - 2: primary current frame, `Ident=" "`
   - 3..N+2: alt frames in alphabetical letter order, `Ident="A".."Z"`
2. `write(read(H))` → cards bit-exact to C `wcsconverter(H)`
   on every per-axis keyword (`CTYPE{i}{s}`, `CRVAL{i}{s}`,
   `CRPIX{i}{s}`, `CD{i}_{j}{s}`, `PC{i}_{j}{s}`, `CDELT{i}{s}`,
   `CUNIT{i}{s}`, `CNAME{i}{s}`, `RADESYS{s}`, `EQUINOX{s}`,
   `LONPOLE{s}`, `LATPOLE{s}`, `WCSAXES{s}`, `WCSNAME{s}`).
3. Global cards (MJD-OBS, OBSGEO-{X,Y,Z}, TIMESYS, DTAI, DUT1)
   match C bit-exactly because both sides write the alphabetically-
   last plane's value.

### Error handling and edge cases

- **No alt planes.** The reader's `decode_wcs_with_alts` produces a
  2-frame FrameSet identical to today's `decode_wcs` output. The
  writer's `collect_plane_stores` returns a single-element vec.
  Existing fixtures stay green.
- **Alt plane with axis-count mismatch.** C uses `WCSAXES{s}` to
  override the per-version axis count; if absent, defaults to
  primary `WCSAXES`/`NAXIS`. The reader honours this exactly.
- **Alt plane SkyFrame can't be built (no RADESYS/EQUINOX, no
  GALACTIC/ECLIPTIC system, no zenithal projection).** Reader
  emits the warning C does (`badcel`) and builds a generic
  `Frame` with `System=UNKNOWN`. Same code path as primary.
- **Letters skip an entry (header has `*A` and `*C`, no `*B`).**
  C still admits this — the alt frame loop iterates A through Z
  unconditionally. The reader follows suit; the assigned letter
  on the resulting alt frames is the actual header letter, not
  a re-densification.
- **`Ident` collisions on round-trip.** The reader stamps `Ident`
  with the source header's letter. If the user manually mutates a
  FrameSet to put two frames at the same letter, the writer's
  `frames_with_version_letters` skips duplicates and re-assigns —
  exactly what it does today.

### Cleanup

The legacy `_legacy_store_from_frameset_to` body in `wcs_write.rs`
becomes truly dead with this change (it was retained as a fallback
during the make_int_world port). It is deleted as part of group 5,
along with its helpers (`split_pre_wcsmap`, `decompose_linear`
callsites in wcs_write, `solve_crpix_from_decomp`, etc.) that have
no other callers.

`store_from_frameset_with_alts` and `frames_with_version_letters`
in `wcs_write.rs` move into the new shared `collect_plane_stores`
helper module.

## Decomposition

Six commit groups, each independently shippable. Earlier groups land
green tests; mid groups may flip a few currently-passing fixtures
red until group 4 lands the writer side.

1. **Frame Ident setters.** Public `set_ident` on every Frame variant.
   No behavioural change. Pure plumbing.

2. **Reader: multi-version FitsStore extraction.** `extract_wcs_keywords`
   gains `s: char`. New `extract_wcs_keywords_with_alts` discovers
   letters and runs the per-letter extraction with primary fallback.
   No FrameSet shape change yet — just `(FitsStore, Vec<(char, FitsStore)>)`
   plumbing. Added unit tests for the discovery + fallback behaviour.

3. **Reader: alt-plane FrameSets.** `decode_wcs_with_alts`. Wires alt
   stores into alt frames added to the FrameSet via
   `add_frame(FrameRef::Base, ...)`. Includes the
   NORADEC + DefB1950 default-equinox path on the alt SkyFrame.
   `FitsChan::read` switches to the alt-aware decoder. The native
   round-trip of a multi-WCS header now produces a multi-frame
   FrameSet matching C's dump byte-for-byte.

4. **Writer: thread `s` through fset_to_store.** Previously hardcoded
   `' '`; now a parameter. Add the `collect_plane_stores` helper
   that runs `fset_to_store` per detected version letter and
   returns `Vec<PlaneStore>`. No encoder behaviour change yet;
   the helper is present and unit-tested but not yet called from
   the encoders.

5. **Writer: per-encoder retrofit + global last-wins.** Each FITS
   encoder switches its top-level entry from
   `store_from_frameset(...)` to `collect_plane_stores(...)`,
   emits per-axis cards with suffixes for every plane, and emits
   global cards once from the alphabetically-last plane. Legacy
   `_legacy_store_from_frameset_to` deleted. Test residue on each
   encoder rebaselines.

6. **DTAI/DUT1 + validation.** Add `dtai` and `dut1` fields to the
   Rust `FitsStore`, with matching `BasicFrame::set_dtai/set_dut1`
   getters/setters and `SkyFrame` mirrors so that `SkySys` can
   populate them on the write side and `WcsSkyFrame` can consume
   them on the read side. Wire the read+write paths. Then walk the
   wcsconv residual list (currently 83 failing of 192): for each,
   classify (A: numeric value mismatch — fix; B/C/D: out of scope
   per the existing roadmap — leave). Document any new C-library
   quirks surfaced in `docs/issues/c-library-quirks.md`. Final
   wcsconv pass count target: ≥ 170/192 (every alt-aware fixture's
   per-axis keywords correct, residual ≈ structural out-of-scope
   items per the roadmap).

## Test strategy

- **Unit:** every module-level helper added in groups 1–5 has at
  least one test exercising the canonical alt-aware path.
- **Integration:** the wcsconv harness drives every change through
  the full `read → write` pipeline against the C-oracle expected
  files.
- **Native round-trip:** `read(H) → to_native_string(...) →
  parse_native(...) → write(H) → cards == C's cards`. The native
  intermediary catches FrameSet-shape regressions.

## Key C-source line references

| Topic | C source |
|-------|----------|
| `FsetToStore` outer per-version loop | fitschan.c:12251-12341 |
| `AddVersion` per-letter dispatch | fitschan.c:12323 |
| `WcsFromStore` reader | fitschan.c:36116+ |
| `WcsSkyFrame` per-version SkyFrame build | fitschan.c:39124-39436 |
| `SkySys` MJD-OBS overwrite (forces `s=' '`) | fitschan.c:29903 |
| NORADEC + DefB1950 default | fitschan.c:39288-39296 |
| nomjd-obs fallback to eqmjd | fitschan.c:39330-39338 |
| `GetItem` primary-fallback semantics | fitschan.c:11682+ |

## Risks

- **Encoder coupling.** Five encoders need similar plumbing.
  Mitigation: a single `collect_plane_stores` + per-encoder
  `emit_planes(planes, encoder_fn)` helper. Group 5 lands all
  five encoders together so any drift is caught in the same
  commit.
- **Mapping simplification through `add_frame`.** When the writer
  adds an alt frame, the FrameSet's integrity rebuild may
  simplify the supplied mapping. The native dump compares before/
  after via the existing `wcsconv_*_iwc_residue` simplifier
  fixtures. Group 3 adds fixtures for the alt-frame insertion
  path if simplification differs.
- **Hidden Ident dependencies.** Some serialisation tests may
  assert `Ident=""` for primary frames. Group 1 expands the
  test set when the setter lands.

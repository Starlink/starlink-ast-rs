# wcsconv Native-Read Parity — Design

Date: 2026-08-04
Branch: tickets/DM-54793
Status: approved for planning

## Goal

Empty `NATIVE_READ_EXCLUDED`, so every one of the 192 wcsconv fixture cases
is byte-compared against its C-generated reference with no exclusions.

The list is the last place in the FITS/wcsconv area where a fixture is
carried without being compared.
Its 45 entries each name a real read-path divergence from C, and a port that
excludes 45 of 192 native cases is not at parity however green the summary
line looks.
When the list is empty it joins `HEAD_ORACLE_EXCLUDED`,
`SLAMAP_ORACLE_EXCLUDED` and `FRAMESET_FIXTURE_EXCLUDED`, all already empty,
and the existing two-sided gate keeps it that way.

A second, smaller goal: make the 24 C-oracle fixtures we cannot yet consume
visible.
They sit in `simplify_fixtures_unsupported/`, grouped by unported class,
mentioned in no test output and no tracker.
That is a quieter form of the same problem this branch has been closing.

## Scope

Three independent deliverables.

**D1 — Empty `NATIVE_READ_EXCLUDED`.**
All 45 native wcsconv cases compare byte-exact under the existing
comparator (FITS encodings exact-string; NATIVE-family ≤4-ULP per numeric
token).
The constant stays as an empty slice with a comment explaining what would
justify a new entry, matching the other three exclusion lists.

**D2 — Harness rule for the `IsSimp`/`Invert` aliasing quirk.**
Four cases differ only in where C's shared-pointer aliasing leaves an
`IsSimp` or `Invert` tag.
This is a C artifact the port deliberately does not reproduce, so those
four become passing tests with one narrow licensed difference rather than
four exclusions.

**D3 — Deferred-fixture visibility.**
A PLAN.md section listing the three unported classes with file counts and
what each blocks, plus a test that walks `simplify_fixtures_unsupported/`,
requires every subdirectory to be registered, and prints the count on every
run.
Also resolve `offset-sref.head`, an imported C fixture currently referenced
by nothing.

Out of scope: porting IntraMap, XphMap or StcResourceProfile; the
`-TAB`/FitsTable work; anything in the already-empty oracle exclusion lists.

## Success criteria

Measured, not asserted:

- `cargo test -p ast-core --test test_wcsconv` → 192 passed, 0 failed.
- `WCSCONV_NO_EXCLUDE=1 cargo test -p ast-core --test test_wcsconv` → the
  **same** 192 passed, 0 failed.
  With an empty list the environment variable is a no-op, and that equality
  is the proof the list is genuinely empty rather than merely deleted.
- `NATIVE_READ_EXCLUDED` is an empty slice.
- The deferred-fixture test prints its count and fails on an unregistered
  subdirectory.
- Full `cargo test --workspace` clean; `cargo fmt --all`; clippy
  `-D warnings`.

## What exploration found

The 45 entries were re-measured by extracting each case's first differing
line (`WCSCONV_NO_EXCLUDE=1`, parsing the per-case panic output).
They collapse into thirteen root causes summing to exactly 45, and three of
the recorded reasons in `test_wcsconv.rs` are wrong about the cause.

### Cause 1 — read-consumed cards not deleted (2 cases)

`specflux`, `timeplot`.

Recorded as "line count: 54 vs 27 (flux frame)" and "(time frame)".
Neither has anything to do with flux or time frames.
The output is *exactly* twice the reference because these inputs are already
native AST dumps: `read()` consumes the cards but leaves all of them in the
chan, so the subsequent `write()` to the same chan appends a second copy.

Measured:

```
specflux: ncard after read=27  reuse=27   (input .head = 28 cards)
timeplot: ncard after read=33  reuse=33   (input .head = 34 cards)
```

The chan is not doubling in card *count*.
`read()` leaves the consumed cards in place, and the subsequent `write()` to the
same chan emits them a second time — so the doubling is in the written output,
not in `ncard`.
The fix is the same either way: the native read path must mark the cards it
consumes so the consumption finisher deletes them, as the FITS arms already do.

C marks each card as it is consumed.
`GetNextData` (fitschan.c:17793) calls `MarkCard` (fitschan.c:23117), which
sets `PROVISIONALLY_USED` on the current card, guarded by the `PROTECTED`
flag:

```c
   flags = ( (FitsCard *) this->card )->flags;
   if( !( flags & PROTECTED ) ) {
      ( (FitsCard *) this->card )->flags = flags | PROVISIONALLY_USED;
   }
```

The port already has the `CardFlags` model and name-based marking from the
consumed-card work.
What is missing is the native read path setting the flag as it consumes.
The `PROTECTED` guard must be ported too, or protected cards get swept.

This is the highest-value single fix in the set: two cases, one cause, and
the fix is in machinery that already exists.

### Cause 2 — CONTINUE long-string split (2 cases)

`polco`, `polco2`.

We emit `CONTINUE  '"",1)     ""'`, C emits `CONTINUE  '",1)     "'`.
An escaped-quote doubling bug at the split boundary.

### Cause 3 — string-value padding (2 cases)

`tnx`, `tnx-cheb`.

We emit `RA = '07:21:44.00'`, C emits `RA = '07:21:44.00       '`.

The defect is on the parse side, not the write side.
`format_string_value` already pads short values to 8 columns, matching C
`EncodeValue`.
C strips trailing spaces from a string keyword value only when the value is 8
characters or fewer (fitschan.c:15433-15442 and the `MAKE_FGET` macro at
~15408); we strip unconditionally.
`tnx.head`'s `RA` value is 18 characters, so C preserves its padding.

### Cause 4 — documented C aliasing quirk (4 cases)

`wave-f2w`, `vopt-f2w`, `serpens`, `cd-cross`.

These differ only in which interior node carries an `IsSimp` or `Invert`
tag.
C's `astMapList` returns reference-counted pointers to the same live
objects, so `astSetInvert` during a read-only probe clears `IsSimple` on a
shared node, and which node keeps its tag depends on C's aliasing history
rather than on tree shape.
The port uses owned values (`flatten` clones), so probe-time inversions
touch throwaway copies and the asymmetry is unreproducible by construction.
`cd-cross`'s first diff (`ISA_K = 'Mapping '` where the reference has
`INVERT_B= 1`) reads like the mapping-invert shape divergence in cause 9,
but measurement shows dropping the `ISSIMP`/`INVERT` tag cards from both
sides leaves no diff at all — it belongs here.

Already recorded in `docs/issues/c-library-quirks.md` under "CmpMap
`IsSimp` tag placement".
Mirroring C's aliasing in Rust is explicitly rejected.

### Cause 5 — SkyRef fiducial from AXREF/RFVAL (2 cases)

`axref`, `rfval`.

Exact and well understood.
We emit `SREF1_A = 2.0943951023931953`, which is 120.0° taken from `CRVAL1`.
C emits `2.0856684561332237`, which is 119.5° taken from `AXREF1` — and the
header does carry `AXREF1 = 119.500`.
The fiducial override is not plumbed.

### Cause 6 — time attributes (2 cases)

`azel`, `multi-wcs-spec`.

`azel`'s 0.783 s epoch difference is a **stale fixture**, not a port defect.
Both sides apply the same +69.1835 s TT−UTC offset for 2017; the difference is
entirely in the parsed seconds field of `DATE-OBS= '2017-07-21T08:38:39.087'` —
the fixture's epoch corresponds to `.870`, ours to `.087`.
That is C's old `DateObs` bug, in which the fractional-seconds field was read
with `"%d"` into an `int` and lost its leading zero.
The reference submodule fixed it on 20-JUN-2026 (commit `254d4c99`, changelog
at fitschan.c:1354-1359) by capturing the field width with `"%n"`; the fixture
predates the fix.
Probing `astRead` on `azel.head` against the pinned submodule returns
`Epoch = 2017.5519807675655`, matching our port to 2 ULP.
`azel.native` is the only stale expected file in the corpus — the other 44
native cases regenerate byte-identical to what is committed.

`multi-wcs-spec` is a genuine port gap: a missing `DTAI_A = 37.0` card on the
top-level world Frame, plus an ungated `DTAI` read where C gates on `TELESCOP`.

`offset-sky` and `offset-sky-pole` carry epoch reasons in the current
exclusion list but are classified under cause 8 by their measured first diff
(`SYSTEM_A = 'Unknown'` where C has `EPOCH_A`).
They are the clearest candidates to need a second fix from this unit after
cause 8 lands.

### Cause 7 — ASTWARN insertion position (8 cases)

`projp`, `projp-nopv0`, `projp-pv0only`, `unknown-cel`, `unknown-cel2`,
`awav-a2f`, `freq-f2w`, `wave-w2f`.

We append ASTWARN cards at the end; C inserts them in front of the current
card.
`Warn` (fitschan.c:35135) states it: the text is "stored in the FitsChan as
a series of ASTWARN cards, in front of the current card".
The implementation saves the card index, rewinds, checks whether an
identical adjacent ASTWARN block already exists, writes the cards, and
restores the index (fitschan.c:35178-35286).

Both failure shapes in this group are the same bug seen from either side:
some cases show `NAXIS1` where `ASTWARN` is expected, others the reverse.

### Cause 8 — Frame vs SkyFrame and attribute defaulting (10 cases)

`alt-spectral`, `origin`, `sip-alt`, `unknown-spec`, `axes-swap`,
`offset-sky`, `offset-sky-pole`, `multi-wcs`, `class-noproj`, `cname`.

We build a plain `Frame` where C builds a typed one, so the expected
`SYSTEM`/`DOMAIN` card is absent from our output and the diff surfaces at
whatever card we emit instead (`AX1_x`, `ACTUNT_x`, `SYSTEM_A='Unknown'`).

Two specific recognition gaps are known from the Phase 4 reconciliation:

- Generic 1-character `xLON`/`xLAT` CTYPE convention (`XLON-TAN`,
  `XLAT-TAN`) not recognized as celestial.
- Blank projection code (`RA--`, `DEC-`, CLASS style) should assume
  old-fashioned CAR — C fitschan.c:1082, change dated 28-AUG-2013.

`class-noproj` additionally shows a projection *description* string
difference (`Sanson-Flamsteed` vs `sinusoidal`), which may be a separate
one-line fix behind the same first diff.
`cname` is the same shape one level down — an axis attribute rather than a
frame one: we close the `SkyAxis` (`ENDAST_D`) where C first emits
`LABEL_C = 'Right Ascension'`, so the axis label is not being set.

This unit is the one most likely to split: ten cases share a first-diff
*shape*, which is weaker evidence of a shared cause than the other groups
have.

### Cause 9 — object shape, CmpMap vs WinMap (5 cases)

`car1`, `car2`, `car4`, `zpx`, `tan-zeropv`.

`car1/2/4` expect `BEGAST_H='WinMap'` where we emit `'CmpMap'`.
`car4` is an MSX header (`TELESCOP='MSX'`), and C's `SpecTrans` has an
MSX-CAR CRPIX translation (fitschan.c:32132, guarded by `!astGetCarLin`,
default 0) that brings a huge out-of-domain CRPIX into the valid CAR range
via `palDrange` and builds a bare WinMap.
We skip it and build `CmpMap(WinMap, WcsMap[CAR,Invert=1])`, whose CAR
inverse leg returns BAD.

**The fix belongs in the reader's `SpecTrans`, not in a WcsMap CAR
`map_merge`.**
C wcsmap.c:141 (10-AUG-2000) records that MapMerge deliberately does *not*
collapse CAR to a UnitMap, because that would lose the cyclic 2π≡0
property.
Stated here so it is not rediscovered the wrong way.

`zpx` (ShiftMap vs MatrixMap) and `tan-zeropv` (WcsMap shape) are shape
divergences in the same family but not necessarily the same fix.

### Cause 10 — `Ident` set-ness (4 cases)

`class-hel`, `class-spec`, `tidy-offsets`, `tidy-pole`.

One bug seen from both sides.
In `class-hel` and `class-spec` C emits `IDENT_A = '" "'` and we omit it, so
our `NAXES_B` lands where the reference has `IDENT_A`.
In `tidy-offsets` and `tidy-pole` the reverse: we emit `IDENT_A` (`'" "'`
and `'A'` respectively) where C has already moved on to `NAXES_B`.

So the port and C disagree about *when* `Ident` counts as set and therefore
gets dumped — not about its value.
Two of the four cases show we can produce the right string, which suggests
the defect is in the set-ness predicate that gates the dump rather than in
the attribute plumbing.

### Cause 11 — SpecFrame reference RA/Dec (2 cases)

`fits-pc-alt`, `fits-pc-unit`.

C emits `REFRA_B = 3.1527470058218849 / Reference RA (rads, FK5 J2000)`
before the preferred-units cards; we skip straight to `UWAVE_B`.
The spectral reference position is not being carried onto the SpecFrame.

Related to cause 5 (both are reference-position plumbing) but on a different
class and from a different source card, so kept separate until measured.

### Cause 12 — space in a keyword name warns when C does not (1 case)

`car6`.

`car6.head` line 1 is the bare text `FITS headers in CHIPASS_Equ.fits:`, whose
8-column name field is `FITS hea`.
C `CheckFitsName` (fitschan.c:6066-6089) branches on the first non-`isFits`
character three ways — `== '='` errors, `< ' '` warns "illegal non-printing
character", `> ' '` warns "illegal character" — then `break`s.
A space matches none of the three, so C issues no warning.
Our port's final arm has no `> ' '` guard, so we emit a four-card ASTWARN block
C never wrote.

### Cause 13 — PROJP cards deleted when C retains them (1 case)

`projp-pv`.

Our output tail is `NAXIS1`, `NAXIS2`, `END`; C's is `NAXIS1`, `NAXIS2`,
`PROJP0`, `PROJP1`, `PROJP2`.
`translate_projp` deletes the PROJP cards in its `!use_projp` branch, which C
does not do, and omits C's `encoding == FITSPC_ENCODING || encoding ==
FITSIRAF_ENCODING` gate on the `PV<axlat>_<i>` write (fitschan.c:31571-31624).

### Unexercised imported fixtures

`inputs/offset-sref.head` is present but referenced by no manifest row —
one of the 132 C `ast_tester` `.head` fixtures, imported and then never
used.
Every other C `.head` is exercised.

The 24 files in `simplify_fixtures_unsupported/` are staged out of reach by
directory, with a README explaining the layout and a re-enabling procedure:

- `intramap/` — 13 files, 8 cases, `IntraMap` unported.
- `xphmap/` — 9 files, 6 cases, `XphMap` unported.
- `stcresourceprofile/` — 2 files, 1 case, `StcResourceProfile` unported.

The directory-level staging is honest but invisible: no test mentions them
and PLAN.md has no row.
No wcsconv case is blocked on an unported class, so this is separate from
the 45.

## Approach

Cheapest-first by cause.
Each unit is one commit that ends by re-measuring and removing the
now-passing entries.
The exclusion list shrinks monotonically, each commit is independently
reviewable, and if the reader clusters prove deep the cheap wins are
already banked.

The alternative — hardest-first, on the theory that the reader clusters may
subsume smaller diffs — was rejected because nothing lands for a long time
and a stall leaves the list untouched.
Strictly per-case commits were rejected because they fight the evidence: the
four cause-1 cases are one bug.

### Work units

One unit per cause, numbered to match: unit U*n* addresses cause *n*.
The tiers reorder the units by effort, so the numbers are not consecutive
down the tables.

Tier 1 — writer and harness, self-contained:

| Unit | Cause | Cases |
|---|---|---|
| U1 | Read-consumed cards not deleted | 2 |
| U2 | CONTINUE long-string split | 2 |
| U3 | String-value padding | 2 |
| U4 | `IsSimp`/`Invert` aliasing harness rule (D2) | 4 |
| U12 | Space in a keyword name warns when C does not | 1 |
| U13 | PROJP cards deleted when C retains them | 1 |

Tier 2 — narrow reader fixes:

| Unit | Cause | Cases |
|---|---|---|
| U5 | SkyRef fiducial from AXREF/RFVAL | 2 |
| U6 | Time attributes | 2 |
| U10 | `Ident` set-ness | 4 |
| U11 | SpecFrame reference RA/Dec | 2 |

Tier 3 — the real port work:

| Unit | Cause | Cases |
|---|---|---|
| U7 | ASTWARN insertion position | 8 |
| U8 | Frame vs SkyFrame, attribute defaulting | 10 |
| U9 | Object shape, CmpMap vs WinMap | 5 |

U7 is writer-side but sits in tier 3 because insertion position interacts
with the card ordering U1 touches.

U10 is a strong tier-2 candidate to attempt early: four cases, and two of
them already produce the right string, so the fix is likely confined to the
set-ness predicate.

D3 is independent of all thirteen units and may land at any point.

### Case counts are upper bounds

The thirteen causes each account for a disjoint set of cases and sum to
exactly 45, but they were derived from each case's **first** differing line.
A case leaves the exclusion list only when it is fully byte-exact, so a
fixed case may reveal a second diff belonging to another cause — for example
`offset-sky` is classified under cause 8 by its first diff and its reason
string names an epoch difference, so it may need cause 6's fix as well.
The counts are therefore upper bounds on progress per unit, and thirteen units
will not necessarily land 45 cases.

**Re-measure protocol.** After each unit, re-extract the first differing
line for every still-excluded case and update the plan if the picture
changed.
A unit is done when its cause no longer appears as any case's first diff —
not when a target count is reached.

If cause 8 turns out to be two or three distinct recognition gaps, split
the unit rather than stretching one commit.

## Components

**FitsChan write path** — U1 (native read path sets the used flag as
`GetNextData` consumes, with C's `PROTECTED` guard), U2 and U3 (card
stringification), U7 (`Warn` save-index / rewind / insert-before-current /
restore-index).

**FitsChan read path and `spec_trans`** — U5 (AXREF/RFVAL precedence over
CRVAL), U8 (celestial-axis recognition and axis labels), U9 (MSX-CAR
`SpecTrans` translation, guarded by `!astGetCarLin` as C is), U11 (SpecFrame
reference RA/Dec).

**Frames** — U6 (azel epoch offset, DTAI card), U10 (`Ident` set-ness
predicate, which gates whether the attribute is dumped).

**Test harness** — U4 reuses the licensed-difference shape the simplify
harness already uses for this same quirk (drop the one `IsSimp` line,
byte-compare everything else); shared logic belongs in `tests/common/`
rather than a second parallel mechanism.
D3 adds the `simplify_fixtures_unsupported/` walker and its
`DEFERRED_CLASSES` table.

**Docs** — `docs/issues/c-library-quirks.md` gains a note that the harness
now absorbs the aliasing quirk instead of excluding the fixtures.
`PLAN.md` gains the deferred-fixture section and its FitsChan row is updated
as units land.
`docs/issues/fitschan-c-port-gaps.md` is the existing FitsChan gap
catalogue and should be updated rather than duplicated by this spec.

**Parallelism** — U2 and U3 are independent of everything.
U1 must precede U7.
U5, U6, U8, U9, U10 and U11 are mutually independent.
U4 and D3 are independent of all code units.

## Verification

**Per unit.** `cargo test -p ast-core --test test_wcsconv` must stay
192/192 — a unit that fixes excluded cases must not break included ones.
Then `WCSCONV_NO_EXCLUDE=1` for the real count, then remove the now-passing
entries.
The existing two-sided gate does the bookkeeping: a stale entry (excluded
but passing) is caught by the same cross-check that found `grism_fits-wcs`.

**Regression breadth.** U1, U2, U3 and U7 touch the shared FitsChan write
path, so re-run every corpus that consumes it: the three serialization
round-trips (24 + 139 + 719 byte-exact), the 66 `test_fitschan*` tests, and
the simplify regression (427).

U8 and U9 change what the reader builds, so they additionally need the
transform oracle.
`HEAD_ORACLE_EXCLUDED` is empty today and must stay empty.
The 17 head oracle failures recorded in the Phase 4 reconciliation are
downstream of exactly these read-path gaps, so U8 and U9 should *improve*
the oracle report — that report is the cross-check that a fix is real rather
than fixture-shaped.

**Final.** `NATIVE_READ_EXCLUDED` empty; both wcsconv runs 192/192 and
equal; full workspace suite clean; `cargo fmt --all`; clippy `-D warnings`
via the isolated-target-dir workaround
(`CARGO_TARGET_DIR=target-clippy RUSTC=$HOME/.cargo/bin/rustc`).

### Two standing requirements

**C-oracle probes for U9, and likely U8.**
Reading `SpecTrans` may not settle what object C actually builds.
The recipe is `docs/issues/c-oracle-probe-recipe.md`; link against the
repo's own build, not a stale conda `libast`.
Editing `reference/starlink-ast/src/` in place is permitted — the submodule
state is recorded and restorable, so instrumenting the C source directly is
a legitimate probe technique.

**No fixture edits to make a test pass.**
The expected files are C-generated ground truth.
A fixture that looks wrong is a finding to investigate and record, not to
overwrite.
The only sanctioned regeneration is a documented C-behavior question
settled by probe.

## Risks

**Cause 8 may not be one cause.**
Ten cases share a first-diff shape, which is weaker evidence than the
other groups have.
Mitigation: the re-measure protocol, and splitting the unit if it splits.

**Cause 9 needs oracle probing, not just reading.**
The MSX-CAR path is guarded and interacts with `CarLin` defaulting.
Mitigation: probe first, and resist the documented wrong fix (WcsMap CAR
`map_merge`).

**A fix could be fixture-shaped rather than real.**
Mitigation: the transform oracle is an independent check on the reader
units — it consumes the same headers through a different comparison.

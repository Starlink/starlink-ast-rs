# Closed PLAN.md findings

The write-up of every finding `PLAN.md` has closed, kept verbatim and under its
original section number so a `PLAN.md §N` citation elsewhere in the repository
still resolves to the analysis it was written against.
`PLAN.md` carries a one-sentence resolution per entry and links here; nothing in
this file is outstanding work.

One entry is not here because part of it is still open, and it stays in
`PLAN.md` in full: §10.2's linear ChebyMap replacement.

---

## 2. Frames and Mappings

### `NormUnit` — the one unported Frame attribute — CLOSED

The row as it stood when it was opened:

| `NormUnit` — the one unported Frame attribute | its own spec, not yet written | OPEN — no `norm_unit` anywhere in `ast-core`, while all 15 Frame-level attributes and 9 of the 10 per-axis attributes are ported, so this is a bounded gap rather than an audit. C derives it from the Unit the Frame reports (`frame.c:13125 (fn GetNormUnit)`), with the normalise-or-return-unchanged policy in the internal `astNormUnit` (`unit.c:5936`) shared by `axis.c` and `frame.c`, and SkyAxis returning a sexagesimal description unchanged (`skyaxis.c:3095 (fn GetAxisNormUnit)`) because it describes axis values rather than being a unit expression. Upstream reworked all three in `89bf58e2`, `d4a55144` and `51ab0542`. **What would expose it**: a test asserting the documented relationship — `NormUnit` equals `Unit` when no simplification is possible and a simplification of it otherwise — across a default `Frame`, both `SkyFrame` axes, a `SpecFrame` and a `TimeFrame`. Asserting the individual strings instead is how the C defect survived eleven years, because the strings recorded what the library returned rather than what the attribute documents. Nothing fails on account of the gap today, which is why it is written down |

Closed by `Frame::norm_unit` (`crates/ast-core/src/frames/mod.rs`), which
normalises the Unit the Frame reports, as `astGetNormUnit` (`frame.c:13149`)
does since upstream `d4a55144`, and `SkyFrame::norm_unit`, which returns a Unit
that describes the axis values unchanged, as `SkyAxis::GetAxisNormUnit`
(`skyaxis.c:3095`) does since `51ab0542`.
The normaliser itself was not in the port: `astUnitNormaliser`, the
`mathmap = 2` style of `MakeExp` and `ModifyPrefix` are in `crates/ast-core/src/units/`.

The test the row asked for is in `crates/ast-core/tests/test_frame.rs`: the
relationship across a default Frame, both SkyFrame axes, a SpecFrame and a
TimeFrame, plus a table of 46 `(Unit, NormUnit)` pairs captured from the C
library.
The table found three `unit.c` defects, fixed upstream on `u/timj/norm-unit`
and in the port, and recorded under "Resolved upstream" in
`docs/issues/c-library-quirks.md`: a constant numerator was not inverted, so
`2/m` converted as `0.5/m` (`965b7587`); a result that merely started with a
number was blanked, so `2*m` normalised to `""` (`25bd4924`); and
`ModifyPrefix` recursed without end on `2/m` and similar (`8c734154`).
What the port cannot yet test is the SkyFrame `Format` block of `testframe.c`,
because the Rust SkyFrame has no `Format` attribute; that is its own row in
`PLAN.md` §2.

---

## 8. Upstream sync at `f6762005` — done

The pin moved from `50c2b21b` to `f6762005`: 41 commits, 25 `src/` files,
+872/−235, nineteen defect fixes, and 103 changed files under `ast_tester/`.
Every fix in the range had an entry in `docs/issues/c-library-quirks.md`, which
is what drove the sync — for each entry, either the port mirrored the bug and
the mirror had to go, or the port already did the right thing and needed a test
pinning the agreement.

Landed in `2efde3a`, `e0cb246`, `e029927` and `3a0a9d9`. 97 fixtures were
copied across byte-identical.

| C change | What the port did |
|---|---|
| `IsSimp` placement becomes deterministic (`e40e71b4`) | Ported: per-class `has_been_simplified` became a two-orientation `SimplifiedRecord`, and all six tag-clearing routines that emulated the old behaviour were deleted. `ALIASING_TAG_LICENSED` retired |
| `WcsNative` no longer inverts the encapsulated PermMap (`f54ff1b7`) | No code change — the port's shape was already the corrected one. `serpens_native` is now byte-exact and the `Invert` half of the old licence is gone |
| `EncodeFloat` justification fixed (`68bcb7db`) | Mirror removed |
| Duplicate-keyword consumption (`6301311f`) | Mirror removed; the write-side sweep gained the matching rule so the newly-surviving MJD-OBS reaches the output |
| `CDjjjiii` consumption (`fd92f9d2`) | Mirror removed |
| Mapping immutability (`AST__IMMUT`) | Not applicable to `ast-core`; the work belongs to `ast-capi`. `testimmutable.c` is deferred with that reason |
| Discard the simplified record on mutation (`6a4bc292`, `b2de3a33`) | Ported for the six classes with a settable attribute here; the other five have no `&mut self` setter, so the stale record cannot be constructed |
| KeyMap dumps in key order (`890707a8`) | Already the port's design; now gated |
| KeyMap: four mirrored defects (`6d114495`, `a76964c7`, `b7135abc`, `f444583f`, `d8edc072`) | All un-mirrored — see the commit and the quirk entries |
| `round()` rounding sweep (`e4e6376d`, `1199880a`, `e447556a`, `7b084bbf`) | Ported at the four sites the port has: the grism interference order, PermMap axis indices, the global-bounds threshold, and WCSAXES |

**One divergence, recorded rather than closed.** Four wcsconv cases differ from
C in `IsSimp` card placement, C emitting more cards than the port in each. It is
not a simplifier difference:
hand C the composed pixel→world Mapping this port builds *before* it simplifies
and C produces exactly this port's tags on exactly the same nodes. What differs
is that `fitschan.c` simplifies in about ten places while assembling, so its
closing `astSimplify` starts from an already-reduced tree, while this port
composes the whole chain and simplifies once. The extra cards are a record of
C's own division of labour inside its reader.

Not worth mirroring: the Mapping, its structure and its transforms are
identical, and reproducing the card means shaping `build_pixel_to_world` around
C's internal call sequence rather than around the WCS it assembles. Logged as a
quirk (`docs/issues/c-library-quirks.md`, "`IsSimp` placement in a FITS-WCS read
records the order the reader simplified in") and licensed with both card counts
pinned (§6), so a *shape* difference could never hide behind it.

`cap_specmap_02` on the simplify corpus is one card too few, from the same input
on both sides, and the card is one the port cannot write. `IsSimp` records the
set of sub-Mappings that were at some instant the entire result of a
simplification, and one of C's instants is inside a merge probe it goes on to
throw away: `CmpMap::MapMerge`'s series-in-parallel probe assembles its trial
CmpMaps by cloning (`cmpmap.c:626-627`, `cmpmap.c:4650`, `cmpmap.c:4657`), so a
trial holds the very sub-Mappings the live tree holds, the recursive
`astSimplify` stamps one of them (`cmpmap.c:1443`), and the stamp survives the
probe's annulment (`cmpmap.c:1624-1628`). The neighbouring parallel-in-series
probe guards against exactly this by copying instead of cloning
(`cmpmap.c:1650-1651`); this one does not. Owned values give each Rust tree its
own children, so there is no route by which the port could reach that node.
`ABANDONED_PROBE_STAMP_LICENSED` drops the one card, positionally, on the five
fixtures it affects and compares every other line. Six earlier fixes were tried
and measured before the mechanism was found, and all were reverted — including
tagging every `simplify_mapping` result, which is `astSimplify_`'s literal
contract, and deferring leaf reduction entirely; they are listed in
`test_simplify_regression.rs` so they are not retried.

The `.ast` FrameSet corpus (`frameset_fixtures/`) is byte-exact throughout and
could not have caught either: it round-trips read → write and never
re-simplifies.

**Not in this range, still open:** the `CmpMap` `InvA`/`InvB` quirk. It cost
`cap_cmpmap_01` and `cap_cmpmap_04` their whole byte comparison until the
closeout replaced their `skip_string_compare` with `INVAB_ALIASING_LICENSED`,
which drops the one node it affects and compares the other 349 lines.

General rule this instantiated, and it held: before moving the pin, diff the
incoming range for serialization changes and check
`docs/issues/c-library-quirks.md` for entries the port *mirrors*. Four of the
five mirrors found this way were invisible to every existing test, because the
fixtures had been generated by the buggy C.

Second rule, from the next pin move: freshness must be checked on every corpus
the pin feeds. The simplify corpus was not covered by a freshness check when the
pin moved from `f6762005` to `3e60abaa`, and two references were stale for the
whole of that interval — `cap_winmap_03`'s `Sft2` residue and `rigby`'s single
drifted value were both read as port defects and cost real time.
`tools/oracle_fixtures/check_simplify_fresh.sh` now covers it, and must be run
alongside `check_corpus_fresh.sh` after any pin move.

---

## 9. Findings from the fixture regeneration

### 9.0 The instrument was broken, and it hid eleven failures — CLOSED

`test_simplify_regression.rs` and `test_regions.rs` each hand-rolled their own
`&dyn Mapping` -> `AnyObject` dispatch. The simplify harness's version covered
18 Mapping classes and no Region, FrameSet or SwitchMap, and returned `None`
for anything else — from inside an `if let Some(..)` that guarded *both* the
byte comparison and the `IsSimp` count check. Every fixture whose simplified
result was a Region or a SwitchMap therefore skipped both, silently.

Both now call `AnyObject::from_mapping`, one authority in `ast-core` that is
the exact inverse of `AnyObject::as_mapping`, and an uncovered class is a
counted failure in the harness and a panic in the region test. Never a skip.

Turning the gate on exposed eleven pre-existing byte-comparison failures and
seven `IsSimp` count disagreements. The pinned C reproduces all eleven `.simp`
files byte-exactly, so every one is a port defect, not a stale fixture. They
are enumerated with causes in `SERIAL_UNGATED`, now empty — every cause below
is closed:

| cause | fixtures | what is wrong |
|---|---|---|
| A | 4 SwitchMap cases — CLOSED | `IsSimp` missing on the FSMap / ISMap sub-Mappings; `SwitchMap::simplified` now stamps them |
| B | 2 `cefit_circle_*`, `neg_box_asymmetric_2d` — CLOSED | Region simplify tagged a Mapping inside the encapsulated FrameSet that C leaves bare, and did not tag the returned Region at all. Both halves were one misreading of where the card comes from: C's loader routes a validated attribute through the setter that discards the record (§9.2), so the `cefit_circle_*` cards were never C's to emit; and C's `IsSimple` getter is overridden on the Frame class to return zero (`frame.c:5590`), so a Region is never tagged and `neg_box_asymmetric_2d` was correct all along |
| C | `nullregion_unc_fillfactor_merge` — CLOSED | emitted a `Prism` uncertainty where C emits a `Box`, because `NullRegion::region_merge` returned the other Region with its own uncertainty rather than building C's merged NullRegion. It now follows `MergeNullRegion` (`nullregion.c:571`): the result is a NullRegion built on a CmpFrame of the two base Frames, `astRegOverlay` carries the supplied Region's attributes across, the two base-Frame uncertainties combine as `astPrism` followed by `astSimplify` — which is what collapses the pair to a single Box — and `astMapRegion` then carries the whole thing into the combined current Frame. `box_unc_fillfactor_merge` was here, declined by the structural-hash recursion guard's one collision in the corpus, and is byte-exact now that the guard runs C's `astEqual` (§10.19) |
| D | `cap_zoommap_04` — CLOSED | emitted 59 lines of `FrmSet` inside `Unc` that C omits, and `IsSimp = 3` where C writes 0. Both came from one test: C asks `sunc != unc` after simplifying the uncertainty, and the port asked `Mapping::equal`, which is conversion-based and cannot see a reduction that changes only structure. The uncertainty's own base→current Mapping collapses to a UnitMap here, which is exactly the condition under which `astSetUnc` clears its `RegionFS` and the dump drops the block (§10.23) |
| E | `cap_interval_01` — CLOSED | the 4-d uncertainty Box's corner point: `X7` was 86400× the reference because the time uncertainty's base→current ZoomMap was not applied. `Interval::simplified` reclassified a bounded Interval to a Box by hand, labelling the base-Frame numbers with the current Frame; C hands the equivalent Box to `astSimplify` and lets the Box carry the FrameSet, which is what re-expresses seconds as days (§10.25). `X5`/`X6` remain, differing in the 12th significant figure, and are licensed under `NUMERIC_PLATFORM_DRIFT`: the port reproduces this machine's C capture on both to the last digit. The `Closed = 1` card C omits retired when `combine_uncertainties` began simplifying the combined uncertainty through the Prism, whose own `astRegOverlay` carries its unset Closed onto the result (§10.24). `cmpmap_parallel_series_dim_mismatch` was here — two `IsSimp` cards on the leaves where C has them on the intermediate CmpMaps — and is byte-exact now that the branch simplifies where C simplifies (§10.3) |
| F | `cap_pointlist_01` — CLOSED | the sky half of the merged uncertainty Box's corner. `PointList::region_merge` assigned the combined uncertainty straight into the merged Region's fields, so C's `astSetUnc` never ran and the corner survived verbatim; C re-centres the uncertainty on the merged PointList's defining point (`region.c:10871-10876`) and `Box::RegCentre` regenerates the corner from the cached geodesic half-lengths, which is what moves the last digits (§10.26). The merge now goes through the same shared tail as `MergeBox` and `MergeInterval`. `X5` / `X6` remain, differing in the 12th significant figure, and are licensed under `NUMERIC_PLATFORM_DRIFT`: the port reproduces this machine's C capture on both to the last digit |
| G | `parpair_one_side_reduces` — CLOSED | one or two extra `IsSimp` cards, and nothing else in the tree, on a leaf the port simplifies and C hands on as it stands. Diagnosed as `try_self_reduce` firing through the `simplified()` gate and predicted to close with §10.16; §10.16 landed and the counts did not move, so that diagnosis was wrong. The sources were two `.simplify()` calls with no C counterpart, both in CmpMap's parallel handling (§10.3): the first closed `cmpmap_perm_parallel_swap` and `cmpmap_perm_swap_{a,b}constants`, the second closed this fixture byte-exact. `cap_slamap_03`, `cap_wcsmap_01`, `cap_cmpmap_01` and `cap_cmpmap_04` did close with §10.16, when PermMap's rebuild moved into `map_merge`, and `lsst` closed the same way when PolyMap's coefficient consolidation and affine rebuild moved |

`equal()` passed all eleven, but that is not an `equal()` defect. C's
`region.c::Equal` compares class name, `points`, base Frame, current Frame, the
base->current Mapping, and `Negated`/`Closed` — and deliberately not the
uncertainty. Causes C and D both differ inside `Unc`, so C's `astEqual` returns
1 for them too, and ours agrees. Both languages *do* distinguish Box from Prism
at any compared position: `region.c::Equal` opens with `strcmp` on the class
names, and our `equal` downcasts.

What this establishes is narrower but still worth the gate: the byte comparison
covers fields `astEqual` is designed to ignore, so it is the only check that
will ever see a wrong uncertainty class.

---

### 9.1 `cefit_circle_*` — CLOSED: the card is discarded on load

The port emitted an `IsSimp` card on the FrameSet's inner ZoomMap
(`cefit_circle_conformal`) and MathMap (`cefit_circle_nonconformal_keep`) that
the pinned C does not.

The instrumentation had established that C dumps an object with `flags = 0`
even though the input `.map` carries `IsSimp = 1` on that Mapping and
`astLoadMapping` honours the card (`mapping.c:24137`), and concluded that the
object must have been freshly constructed. It was not. It is the same object,
whose flag its own loader discarded: `astLoadZoomMap` re-applies the validated
`Zoom` value through `SetZoom` (`zoommap.c:2237-2238`), and that setter calls
`astClearIsSimple` under the convention in §9.2. `astLoadMathMap` does the same
for `SimpFI`, `SimpIF` and `Seed` (`mathmap.c:7392, 7397, 7410`).

A stamp can still reappear afterwards, which is why the corpus looks
inconsistent at first reading. `astSimplify_` stamps whatever the vtab slot
returns (`mapping.c:24783`) and the base `Simplify` returns `astClone(this)` —
the same pointer — so the stamp lands on the object already in the tree.
`FrameSet::Simplify` then keeps it only when some edge reduced:
`astClone( simpler ? new : this )` at `frameset.c:10226` discards the entire
deep copy otherwise, stamps and all. `Region::Simplify` does the same at
`region.c:11042`.

The corpus discriminates the rule on two matched pairs — same class, same
nesting depth, differing only in whether a clearing attribute is present:

| fixture | class | clearing attribute in the `.map` | `IsSimp` in C's `.simp` |
|---|---|---|---|
| `cefit_circle_conformal` | ZoomMap | `Zoom = 2` | absent |
| `cap_poly_unc_frame` | ZoomMap | `Zoom = 4` | present — the copy carrying the re-stamp is the one adopted |
| `cefit_circle_nonconformal_keep` | MathMap | `SimpFI = 1`, `SimpIF = 1` | absent |
| `cap_poly_bent_edges` | MathMap | none, so `TestSimpFI` is false | present |

The dump/load asymmetry is C's own and is recorded in
`docs/issues/c-library-quirks.md` ("The `IsSimp` card does not survive being
read back, for twelve classes").

A second rule surfaced with it and closed `NO_ISSIMP_CARD_UNGATED`: no
Frame-kind object ever carries a top-level `IsSimp` card, because
`astGetIsSimple` is overridden on the Frame class to return zero unconditionally
(`frame.c:5590`, "always zero because Frames are not immutable"), and FrameSet
and every Region inherit that override. `test_simplify_regression.rs` now
requires a top-level card exactly when the result is not Frame-kind, on every
negative fixture in the corpus.

---

### 9.2 The clear-on-set convention — PORTED, and every reachable attribute has a setter — DONE

Upstream `e40e71b4` added the rule: a setter for an attribute that feeds the
first part of that class's own `astMapMerge` discards both simplified records
(`mapping.h:449-460`).
C's own comment there gives the reason and the latitude — nothing else reads the
records, since a Mapping is offered to `astMapMerge` by its neighbours whatever
it holds, and a record can only ever suppress a simplification and never make
one wrong, so C errs towards discarding.
It applies the rule in 11 classes:

| class | attributes | port |
|---|---|---|
| ZoomMap | `Zoom` | `set_zoom` |
| LutMap | `LutInterp` | `set_interp` |
| LutMap | `LutEpsilon` | `set_lut_epsilon` |
| MathMap | `Seed`, `SimpFI`, `SimpIF` | `set_seed`, `clear_seed`, `set_simp_fi`, `set_simp_if` |
| PolyMap | `IterInverse`, `NiterInverse`, `TolInverse` | `set_iter_inverse`, `clear_iter_inverse`, `set_niter_inverse`, `set_tol_inverse` |
| SplineMap | `InvNiter`, `InvTol`, `OutUnit` | `set_inv_niter`, `set_inv_tol`, `set_out_unit` |
| SphMap | `PolarLong`, `UnitRadius` | `set_polar_long`, `set_unit_radius` |
| WcsMap | `FITSProj`, `LonCheck` | `set_fits_proj`, `set_lon_check` |
| PcdMap | `Disco`, `PcdCen` | `set_disc`, `set_pcd_cen` |
| PermMap | `PermSplit` | `set_perm_split`, `clear_perm_split` |
| IntraMap | `IntraFlag` | no setter |
| GrismMap | (macro form, `grismmap.c:165,255`) | `set_grism_nr`, `set_grism_nrp`, `set_grism_wave_r`, `set_grism_alpha`, `set_grism_g`, `set_grism_m`, `set_grism_eps`, `set_grism_theta` |

Every setter and clearer the port has discards the records, and each cites the
`astMAKE_SET` / `astMAKE_CLEAR` site it mirrors together with what the attribute
changes: whether it feeds that class's own `MapMerge` (MathMap's `SimpFI` /
`SimpIF`, SphMap's `PolarLong` / `UnitRadius`) or only the transformation, which
is C's err-towards-it case.
GrismMap's eight setters come from a class-local `MAKE_SET`
(`grismmap.c:236-262`) whose body opens with the same `astClearIsSimple` and ends
by recomputing the derived constants, so each of them does both; C's own merge
builds its result with `astCopy` followed by a setter per adjusted attribute
(`grismmap.c:573-592`), which the port's `merge_with_zoom` mirrors by discarding
the records on the copy it returns.

**A fixture expresses half of it.** The simplify corpus dumps, simplifies and
dumps again, so it has no "set an attribute after simplifying" step and cannot
reach a setter directly. It does reach the *loader*, which C routes through the
same setters: `cefit_circle_conformal` (ZoomMap) and
`cefit_circle_nonconformal_keep` (MathMap) both dumped a card C discards on load,
and `cap_poly_bent_edges` — whose MathMap carries no `SimpFI` / `SimpIF` /
`Seeded` card, so `TestSimpFI` is false and nothing is routed — is the control
that keeps its card (§9.1).

The direct half is gated by `crates/ast-core/tests/test_clear_on_set.rs`: a case
per setter, plus `record_survives_invert_and_back`, which pins an operation C
does *not* cover (`mapping.h:424-437`) so the other assertions cannot pass on a
port that never records anything, plus `load_discards_the_record_for_every_clear_on_set_class`,
which dumps a stamped Mapping with the attribute set, reads it back and requires
the record gone — with ShiftMap, which clears nothing, as the control that must
keep it.
C's behaviour is confirmed by the probe recorded in
`docs/issues/c-oracle-probe-recipe.md` ("Worked probe — the clear-on-set
convention"), which reads `IsSimple` through the public string interface across a
set, a clear and the invert control.

**One attribute remains without a setter, and it is not reachable.**
`IntraMap::IntraFlag` has none because `IntraMap` itself is not ported.

**Where the dump can see the difference, the setter carries C's `Test*`
predicate.**
GrismMap, LutMap and PcdMap store the value C's getter would report rather than
C's unset sentinel, and carry the sentinel test as a flag beside it, so passing
`BAD_VALUE` — `INT_MAX` for `GrismM` — restores the default and leaves the
attribute untested, exactly as `astSet` with the sentinel does.
GrismMap's dump writes an untested attribute commented out
(`grismmap.c:1733-1737`), PcdMap writes all three of its cards with the flag C's
`TestDisco` / `TestPcdCen` supplies (`pcdmap.c:2705-2721`), and LutMap writes the
`LutEps` card only when the attribute is set (`lutmap.c:2009-2013`).
`ZoomMap::Zoom` needs no flag: C's sentinel is `0.0`, which its setter rejects,
so a constructed ZoomMap has no untested state to report.

**The loaders route through the setters, which is how a fixture reaches them.**
C's `Load` assigns each raw value and then re-applies it through the private
setter when its `Test*` predicate reports the card present — `grismmap.c:2548-2571`,
`lutmap.c:2572-2577`, `pcdmap.c:3192-3205` — and that is what discards the record
`astLoadMapping` has just installed from the `IsSimp` card.
Routing the port's `from_node` the same way is what closed the four fixtures that
had been licensed under `ROUNDTRIP_DISCARD_LICENSED` (§6): `grism_zoom_merge`,
`grism_zoom_inv_merge`, `pcd_swap_zoom_simplifies` and `pcd_unit_series_merge`
now round-trip byte-exactly, and backing the routing out fails those four and no
others.
The composition of the two — a validating re-application that discards a record
the same loader just installed — is not a decision either half's upstream
history claims, so it is logged as a mirrored quirk with its revisit trigger in
`docs/issues/c-library-quirks.md` under "The `IsSimp` card does not survive being
read back, for twelve classes".

`set_zoom`, `set_lut_epsilon`, `set_disc`, `set_pcd_cen` and GrismMap's eight
have no `clear_*` counterpart, because for each of them the sentinel already
reaches the state a clearer would: `astClearDisco` and `astSetDisco( map,
AST__BAD )` store the same `AST__BAD` (`pcdmap.c:2583-2586`), and the same holds
for `LutEpsilon` and for every GrismMap attribute.
None of the setters raises `AST__IMMUT`, for a structural reason: `&mut self` is
what C's `astGetRefCount( this ) > 1` guard tests for.

---

### 9.3 24 of the 109 NATIVE-family wcsconv references were stale — DONE

Verified by building `ast_tester/wcsconverter.c` against the pinned C and
replaying every NATIVE-family manifest row *with its `attrs` field* — omitting
the attributes produces false structural differences.
94 of the 109 rows now reproduce byte-exactly and the rest are Linux captures.

**No keyword sequence differed in any of the 109.**
Nothing about core WCS structure changed upstream; every difference was either
an `IsSimp` card or a last-bits value.

| bucket | count | disposition |
|---|---|---|
| `IsSimp` cards only | 17 native + `jan.ast` | regenerated from the pinned oracle; written into the submodule tree for upstream |
| last-bits values *and* stale `IsSimp` | 6 | cards taken from a fresh capture, values kept, via `tools/oracle_fixtures/merge_card_hunks.py` |
| last-bits values only, `IsSimp` correct | 9 | not stale — Linux captures; documented in the manifest header, untouched |
| structural | 0 | — |

Mixed: `scp`, `tnx`, `tnx-cheb`, `ssysobs`, `skyspec3d-class`, `wave-f2w`.
Platform-only: `timj`, `multi-wcs`, `skyref`, `iraf-longpole`, `skyref-alt`,
`tidy-offsets`, `tidy-pole`, `offset-sref`, `azel`.

This is why the ShiftMap merge-revert fix looked like a six-case regression.
Measured against the pinned C rather than the committed reference:

| case | port | committed | pinned C |
|---|---|---|---|
| scp, serpens, tnx, tnx-cheb | 2 | 1 | **2** |
| crota-3d | 5 | 3 | **5** |
| cd-cross | 3 | 2 | 4 |

**Outcome.** With the corpus refreshed, 20 of the 24 `ISSIMP_PLACEMENT_LICENSED`
rows were pinned against references the pinned C no longer reproduced. They were
deleted rather than adjusted, and the port matches C on all of them. Four
survive — `axes-swap` (2 vs 4), `cd-cross` (3 vs 4), `vopt-f2w` (4 vs 5),
`wave-f2w` (4 vs 5) — all in the FitsChan read path, all with C emitting more
cards than the port. Which of C's ten mid-assembly `astSimplify` calls produced
which card is not attributed per case; that needs
`tools/oracle_fixtures/build_issimp_instrumented.sh`.

`tools/oracle_fixtures/check_corpus_fresh.sh` is the standing gate. It scans
every `native` and `ast` row, classifies cards and values independently so a
stale card cannot hide behind platform drift, and is part of the upstream-sync
protocol in `CLAUDE.md`. The FITS encodings carry no `IsSimp` record and are
left to the byte comparison in `test_wcsconv.rs`.

Specified in
[`docs/superpowers/specs/2026-08-21-simplify-mapmerge-audit-design.md`](../superpowers/specs/2026-08-21-simplify-mapmerge-audit-design.md)
as W3.

---

### 9.5 Audit checklist citations — CLOSED (cluster 7)

All 37 checklists in `docs/audits/mapmerge/` are anchored against pin `3e60abaa`
and declare `Citations: verified against pin 3e60abaa` and
`Extension points: classified`.
Six gates in `crates/ast-core/tests/test_audit_citations.rs` hold it: each
citation resolves to exactly one file, names the item it claims, every
architecture row lands on a trait method, and every C step comment inside a cited
body is covered by a row or named under `Uncited C steps:`.
`every_checklist_declares_both_markers` makes the markers the entry condition, so
a checklist added later is gated on arrival, and `no_row_is_left_at_gap` holds
the row verdicts.

Citations are resolved by unique path suffix, not basename: `simplify.rs` has two
candidates in this tree and `mod.rs` has thirteen, so citations name
`mappings/simplify.rs` and `regions/mod.rs`.

**When the pin moves**, re-anchor with
`cargo run -p audit-tools --bin anchor-citations -- --write docs/audits/mapmerge/*.md`
and hand-check every row the tool cannot anchor.
Do not remap mechanically by text similarity: that was tried, and it relocated 25
rows confidently wrong, which is how the rot went unnoticed in the first place.
`simplify.rs:1151-1246`, cited identically by those 25 rows for the shared
neighbour-pair driver, was already wrong when written — at those revisions it
lands inside `perm_map_merge`, while the driver the rows describe is
`default_map_merge`.
Update the pin in both marker lines in the same commit range, so a checklist
never claims a pin it was not checked against.

---

## 10. Gaps named by the `MapMerge` / `Simplify` audit

The closeout that landed these entries is specified in
[`docs/superpowers/specs/2026-08-24-simplify-parity-closeout-design.md`](../superpowers/specs/2026-08-24-simplify-parity-closeout-design.md),
with the task breakdown in
[`docs/superpowers/plans/2026-08-24-simplify-parity-closeout.md`](../superpowers/plans/2026-08-24-simplify-parity-closeout.md):
fourteen tasks, one per spec item, each landing its fixtures with the code that
needs them.

Every item the spec inventoried closed: A1 (§10.3's two extra simplifications),
A2 (§10.19), A3 (§10.20), A4 (§10.4), A5 (§10.21), A6 (§10.22's dead rewrite),
A7 (§10.17's "the result is new"), B1 (§10.2's consolidation half), B2 (§10.13),
B3 (§10.18), B4 (§10.17's NormMap branch), B5 (§10.12) and C1 (§9.2's
convention). Thirteen fixtures were added and one retired; the transform-oracle
corpus went from 1614 sections to 1752 with every exclusion list still empty.

§10.3's seven remaining CmpMap tags closed with it. There was no WinMap
under-merge: the port forms the CmpMaps C tags, and was raising C's stamps on
objects it then discarded. What survives is one card per fixture that a merge
probe C abandoned left on a shared sub-Mapping, licensed positionally in
`PLAN.md` §6.

### 10.2 ChebyMap's `MapMerge` — FIXED, both halves

In C, `chebymap.c` defines no `MapMerge`; only `polymap.c:2457` fills the slot,
so a ChebyMap's `MapMerge` is PolyMap's whole body at `polymap.c:3626-3967`. In
the port, `ChebyMap` is its own struct, and it had no `map_merge` at all, so it
took the trait default and reached neither of PolyMap's replace-with-simpler
halves.

The coefficient consolidation (`polymap.c:3644-3765`) combines coefficients that
use the same set of input powers. That is basis-independent — two `T_2` terms for
one output add exactly as two `x^2` terms do — so it ports as it stands, and now
does. `PolyMap::consolidated_terms` is C's first half split out of
`replace_with_simpler` unchanged, and `ChebyMap::map_merge` calls it and rebuilds
a ChebyMap with the same bounds. `cap_cheby_consolidate` is the fixture — two
forward `T_2` coefficients of 1.5 and 2.5 that C writes back as one of 4, with a
`T_3` term present so C stays out of the linear branch — and it fails
byte-exactly with `ChebyMap::map_merge` removed.

The linear replacement (`polymap.c:3767-3918`) rebuilds a transformation whose
every remaining term is constant or first-order as a `MatrixMap` and a
`ShiftMap`. It was deferred for two AST releases, because C read a ChebyMap's
`coeff_f` / `power_f` with nothing that distinguished a Chebyshev basis from a
power basis, and so dropped the scale and offset that map the input onto
`[-1,1]`: a one-input ChebyMap over `[0,10]` holding the single term `2*T1`
computes `0.4x - 2`, and `astSimplify` returned a ZoomMap of 2. The port declined
the step rather than mirror the defect, and `cheby_linear_lone_reduce` was its
one `C_DEFECT_EXCLUDED` row.

Upstream fixed it in `abe5baae`, and the port ported the fix at the 9.5.0 sync.
Only `T0(z) = 1` and `T1(z) = z` have degree one or less, so a term the linearity
test accepts is linear in the input as well as in the normalised `z`, and a term
`c*T1(z_j)` contributes `c*scale_j` to the matrix and `c*offset_j` to the
constant term; both arrays accumulate rather than assign, because an output can
collect a constant contribution from each of its linear terms as well as from its
own constant term (`polymap.c:3858-3877`). C's half is one shared
`PolyMap::linear_replacement(terms, norm)` whose `norm` argument is C's
`norm_scale` / `norm_offset` pair, so the seam sits where C's does: `ChebyMap`
passes its `scale_f` / `offset_f` and `PolyMap` passes `None`. The `[0,10]`
ChebyMap now reduces to a WinMap with `Scl1 = 0.4` and `Sft1 = -2`.

The exclusion's own gate is what reported the fix: `c_defect_rows_still_diverge`
fails when an excluded row starts matching its reference, so the row moved into
the compared corpus rather than staying silently excluded, and
`C_DEFECT_EXCLUDED` is now empty. Two tests in `test_chebymap.rs` pin the two
parts of the fix separately — `a_linear_chebymap_reduces_with_its_normalisation_folded_in`
fails when the normalisation is dropped, and
`a_linear_chebymap_accumulates_each_outputs_constant_contributions` fails when
either array is assigned rather than accumulated.

---

### 10.1 RateMap's self-reduction is unreachable from the nominate loop — FIXED

`ratemap.c:717-727` opens `MapMerge` by simplifying the encapsulated Mapping and
rebuilding the RateMap around the result. The port had the logic in
`simplified()` and no `map_merge`, so it never ran on a RateMap nominated from
within a list. `RateMap::map_merge` now uses `self_reduce_map_merge`, which is
C's order: reduce first, then cancel against an inverse neighbour in series.

`rate_inner_reduce_series` is the fixture — a RateMap over a series ZoomMap pair
that fuses to one, inside a CmpMap so the RateMap is a list element. The pinned C
reduces it to `RateMap(ZoomMap(6))`. `rate_inner_reduce_lone` is the same RateMap
alone and passed from the start, which identified the seam rather than the
arithmetic as the gap.

**The second half of the fix, and why two earlier attempts failed.** Adding the
seam alone made `cap_cmpmap_01` and `cap_cmpmap_04` diverge structurally. C sets
the encapsulated Mapping's Invert to `map->invert` — the value it held when the
RateMap was built, which the `Inv` card carries — simplifies at *that*
orientation, and restores the stored value afterwards (`ratemap.c:710-715`,
`:806-808`). The port simplified at whatever the child currently stored.

In those two fixtures the encapsulated CmpMap stores `Invert = 1` while the
snapshot is 0, so C reduces the forward form and finds nothing, while the port
reduced the inverted form and folded the inversion into the component order.
`RateMap::reduced_child` — the step `map_merge` opens with — now orients to
`invert_snapshot` first, the field the port already had for the `Inv` card and was
using only for serialisation. That is the H3 hazard: two C names kept as two
fields, with only one of them wired up.

An earlier attempt reduced the *forward* form unconditionally. That is right only
when the snapshot is 0, which is true of every fixture in the corpus, so it went
green for the wrong reason and was reverted before this.

---

### 10.3 The parallel-in-series branch now simplifies where C simplifies — FIXED

All three divergences in the port's handling of C's `cmpmap.c:1624-1783` are
closed, so rows 22, 23 and 26 of `docs/audits/mapmerge/cmpmap.md` are `ported`.
Two fixtures came byte-exact with the change — `cmpmap_parallel_series_dim_mismatch`
and `parpair_one_side_reduces` — and their `SERIAL_UNGATED` rows are deleted,
taking that list from nine cases to seven. `cap_specmap_02` went from ten
`IsSimp` cards to seven against C's eight. The remaining card was read as a
WinMap under-merge — a CmpMap the port never forms — and it was not: the port
forms `CmpMap(SpecMap, ZoomMap)` byte for byte, and the missing card is the
abandoned probe's, closed below.

The entry is still open, for a reason the change itself exposed.

**`cap_cmpmap_01` and `cap_cmpmap_04` never agreed with C.** They read 18 cards
against C's 18 and now read 16 and 15. Both were `skip_string_compare` while this
was written, so the card count was the only gate on them — and it is a total, which two errors in
opposite directions can satisfy (H10). The port was tagging leaf classes C never
tags while already missing CmpMap tags C sets. Per class, the two sides now
differ in one place only:

| fixture | port | C |
|---|---|---|
| `cap_cmpmap_01` | 14 CmpMap, 1 MatrixMap, 1 UnitMap | 16 CmpMap, 1 MatrixMap, 1 UnitMap |
| `cap_cmpmap_04` | 13 CmpMap, 1 MatrixMap, 1 UnitMap | 16 CmpMap, 1 MatrixMap, 1 UnitMap |

Before the change the port tagged 14 CmpMaps in each and made both totals up to
18 with tags C does not set at all — a MathMap and a TranMap in `_01`, a TranMap
and a second UnitMap in `_04`. Dropping the per-member simplify removed those,
which is C's shape, and `_04` lost one of its CmpMap tags with them.

A positional diff against the reference is now short enough to itemise. `_01`
differs in three `IsSimp = 1` lines C emits on inner CmpMaps and the port does
not, plus one object where the port emits `IsSimp = 1` and C emits `Invert = 1`
— the open `InvA`/`InvB` dump quirk
(`docs/issues/c-library-quirks.md:320`), not a stamping difference. `_04` differs
in four such `IsSimp` lines and the same one `Invert` line.

**Closed.** The missing `IsSimp` lines were two causes, not the seven-CmpMap
mystery this section described. Most of them were stamps the port raised and then
threw away: C reaches each nominated element through `astSimplify`
(`cmpmap.c:1443`), which stamps its result, and when nothing reduced that result
*is* the element — C's `new == map` — so every stamp the recursion left on its
descendants is already on the object the closing fold encapsulates.
`CmpMap::map_merge` now adopts that object rather than tagging only its list
entry, and the tag is written at the orientation C stamps, `invert_list[where]`.
`_01` and `_04` reached 18 cards against C's 18 with that.

What is left in each is one card the port cannot raise, from a merge probe C
abandoned but whose mark survives on a shared sub-Mapping, and the one
`InvA`/`InvB` node. Both are licensed positionally and per fixture
(`ABANDONED_PROBE_STAMP_LICENSED`, `INVAB_ALIASING_LICENSED`, §6), so both
fixtures gave up `skip_string_compare` and every other line of both files is now
byte-compared — the gate this section said they could not have.

#### What C does in this branch

C's shape at `cmpmap.c:1624-1783` is: match the two parallel lists into aligned
sublists, `CombineMaps` each pair in series **without simplifying the members**,
`tmap2 = astSimplify( tmap )` on the combined pair (`:1728`), set `simpler` from
the identity test `tmap != tmap2 || astGetInvert( tmap ) != astGetInvert( tmap2 )`
(`:1731-1733`) *before* annulling `tmap`, and assemble the survivors into one
parallel CmpMap with no further simplification. The port diverged in three
places, all now closed:

1. **Per-member simplify.** `dimension_match_parallel_lists` simplified every
   member of both sublists. C passes them to `CombineMaps( …, 0, … )` as they
   stand — that `0` is the series flag, not a simplify flag
   (`cmpmap.c:1694-1712`). The port now clones them unchanged
   (`simplify.rs:2127-2135`).
2. **The `simpler` test.** A three-branch structural test ending in a comparison
   of flat leaf counts stood in for C's identity test — a threshold with no
   analogue in `cmpmap.c` (stop signal 1), deciding whether the whole restructure
   fired. C's test is ported at `simplify.rs:3047-3049`:
   `simplify_mapping_reporting_change` (`simplify.rs:3289-3384`) reports whether
   the result differs from its input, which in an owned-value port is a fact the
   engine knows rather than a pointer comparison, and the Invert flags are
   compared beside it.
3. **Trailing simplify of the rebuilt CmpMap.** The port simplified the
   reassembled parallel CmpMap before returning it; C hands `new` to the list as
   built (`cmpmap.c:1738-1746`) and lets the nominate loop reach it when it is
   nominated in its own right (`simplify.rs:3062-3064`). Closing this took
   `cap_cmpmap_03` from six cards to **seven, which is C's count**.

The per-pair `astSimplify` of `:1728` was always ported, at
`simplify.rs:3047-3048`; the recursion was never the missing piece. An earlier
reading of divergence 1 had it standing in for a missing recursion, and that was
wrong — it was standing in for divergences 2 and 3 in the same function, which is
why removing it alone moved fixtures in both directions.

The same over-simplification was in `try_swap_permmap_parallel_at`, which
simplified each surviving component before rebuilding the swapped CmpMap. C
assembles the new CmpMap from `cmpmap2->map1` / `map2` directly
(`cmpmap.c:2053-2070`) and never simplifies them; the nominate loop reaches them
afterwards, through the rebuilt CmpMap. Removing those two calls closed
`cmpmap_perm_parallel_swap`, `cmpmap_perm_swap_aconstants` and
`cmpmap_perm_swap_bconstants` byte-exact with no losses elsewhere.

Seven approaches to `cap_cmpmap_03` were measured and reverted before divergence
3 was found, six recorded in `test_simplify_regression.rs` plus a blanket stamp
in `CmpMap::simplified`. Every one of them tried to add a stamp; what closed it
was removing a simplification C does not perform. The blanket stamp is the
clearest case: it moved `cap_cmpmap_04` one card closer *and* gave
`cap_zoommap_01` a card C does not emit, because it stamps nodes the port
produced and C did not while still missing the ones C produced and the port did
not.

#### The residual is over-nomination, not a missing merge

Both engines are instrumented — `build_simplify_trace.sh` for C,
`simplify::trace` behind the `tracing` feature for the port — and log each
nomination and what it returned. Re-measured on `cap_specmap_02` at pin
`07fbe2dc` with all three divergences closed:

**The two traces are not directly comparable, and the raw counts mislead.** When
`ShiftMap::MapMerge` converts to a WinMap and calls `astMapMerge` on it
(`shiftmap.c:671-745`), C's instrumentation emits a *second* nomination event for
the delegated WinMap at the same nesting depth. The port calls `win_map_merge`
directly, so only the ShiftMap event appears. Nine of C's raw nominations are
those echoes, and all nine are productive — so C's raw 96/23 is 87/14 in genuine
nominations. Detect an echo as a nomination whose immediately following
nomination is a different class at the same indentation.

| class | C noms | port noms | C productive | port productive |
|---|---:|---:|---:|---:|
| CmpMap | 22 | 23 | 5 | 5 |
| LutMap | 16 | 20 | 0 | 0 |
| PermMap | 9 | 11 | 0 | 0 |
| WinMap | 9 | 10 | 1 | 2 |
| ShiftMap | 9 | 11 | 3 | 3 |
| SpecMap | 8 | 14 | 1 | 1 |
| ZoomMap | 7 | 13 | 1 | 1 |
| MatrixMap | 4 | 4 | 0 | 0 |
| UnitMap | 3 | 3 | 3 | 3 |
| **total** | **87** | **109** | **14** | **15** |

The port's CmpMap row is 12 series plus 11 parallel; C reports both compositions
under the one class name.

**There is no productive deficit.** Both engines perform the same merges, and the
port's one extra productive WinMap is a *second* occurrence of the same
`WinMap where=1 series=0 nmap=2` merge C performs once. What remains is
over-nomination: 109 against C's 87, concentrated in LutMap (20 vs 16), SpecMap
(14 vs 8), ZoomMap (13 vs 7) and PermMap (11 vs 9) — every one of which is
productive **zero** times on this fixture. The port is not missing a merge rule;
it re-walks lists C walks once. Closing the three divergences removed two of
those extra passes, taking the port from 153 nominations to 109 while leaving the
productive count at 15, and brought the number of nominations of an
already-stamped Mapping from 51 to 15, against C's 16.

**Upstream has since fixed a C bug in the same branch.** At pin `8d4f526f`,
`cmpmap.c` commit `4f2fc0ed` — *"test for simplification before annulling the
pointer under test"* — repairs the parallel-CmpMaps-in-series case: the annul of
`tmap` ran before the `tmap != tmap2` test, so the test was always true and the
pair was rebuilt as a parallel CmpMap of series compositions **even when no
component reduced**. The sibling branch 120 lines earlier had it in the right
order. Any conclusion drawn about this branch before pin `8d4f526f` was drawn
against a buggy oracle. The regenerated
`cmpmap_parallel_series_dim_mismatch.simp` carries two more `IsSimp` cards than
the stale copy, and the port now emits all three — that fixture is one of the two
this entry closed.

**The ShiftMap delegation is not the divergence.** An earlier revision of this
entry narrowed the deficit to the `ShiftMap where=0 series=1 nmap=4` nomination,
on the reading that C's list still held four elements afterwards and the port's
held three. The C trace at pin `07fbe2dc` shows otherwise:

```
MAPMERGE ShiftMap where=0 series=1 nmap=4 issimp=0     # c_trace:108
MAPMERGE WinMap   where=0 series=1 nmap=4 issimp=0     # the delegation echo
MAPMERGE-RESULT WinMap   result=0
MAPMERGE-RESULT ShiftMap result=0
MAPMERGE WinMap   where=0 series=1 nmap=3 issimp=0     # C's list is 3 too
MAPMERGE-RESULT WinMap   result=-1
```

The `nmap=4` WinMap line is the delegation echo described above, not a separate
opportunity, and C's next genuine nomination is `WinMap where=0 series=1 nmap=3`
— exactly the port's. `ShiftMap::map_merge` (`shift.rs:168-238`) delegates and
reverts as C does, and the two agree on class, index, `issimp`, result and
resulting list length at that step.

---

### 10.4 The CmpMap/PermMap swap was hung off the wrong nominee — FIXED

`cmpmap.c:1816-2119` swaps a parallel CmpMap in front of an adjacent PermMap. It
runs when the **CmpMap** is nominated and the PermMap is its lower neighbour, as
the last block of `CmpMap::MapMerge`, reached only when nothing earlier
simplified (`result == -1`). There is no counterpart on the PermMap side:
`permmap.c` contains no reference to `CmpMap` at all, so a nominated PermMap
never attempts this.

The port had it the other way round: the swap lived in `perm_map_merge` and fired
when a **PermMap** was nominated, looking at the `(where_, where_ + 1)` pair. The
arithmetic was already C's. The branch now sits at the end of `cmp_map_merge`
(`simplify.rs:768-790`) behind C's four conditions, and
`try_swap_permmap_parallel_at` is unchanged.

The record's original premise was wrong about what this cost. It said the two
triggers report different indices and so resume one element apart. They do not:
C returns `result = imap1 = where - 1` from the CmpMap's nomination
(`cmpmap.c:2119`), the port returned `Some(where_)` from the PermMap's, and
because `run_nominate_loop` advances before clamping (`simplify.rs:1463`,
`:1498-1500`) both name the PermMap's slot and both resume there. Nor is a pair
reachable in C but not here: C's `result == -1` guard *defers* the swap to a
later round rather than forbidding it, so the port's earlier trigger only ever
preempted a merge C would reach on its next pass over the same list. No corpus
fixture and no purpose-built candidate separates the two on output.

What the move is visible in is the nomination trace. `cap_cmpperm_resume`
(`[Zoom2, PermMap, parallel CmpMap, Zoom3]` in series) and its inverted-PermMap
sibling `cap_cmpperm_resume_inv` are byte-exact positive controls on the swap's
result; against the instrumented oracle the port now matches nomination for
nomination — `PermMap where=1` returning no change, then `CmpMap where=2`
returning 1 — where before it returned 1 from the PermMap's own nomination. That
trace agreement, not a fixture that flipped, is the evidence the seam moved.

This entry replaces the plan's §10.4, which duplicated §9.2.

---

### 10.6 A parallel UnitMap's Invert flag — NOT A GAP, retracted

Recorded as a gap on the strength of reading `UnitMap::map_merge` alone, which
handles the lone-UnitMap case and the series case and otherwise falls through to
`default_map_merge`, where a nominee with no UnitMap neighbour returns `None`.
Nothing there clears the flag, so `unitmap.c:579-587` looked unported.

It is ported, elsewhere. C clears `invert_list[ where ]` — the *list's* applied
direction, not the object's own attribute — and that surfaces in the dump as the
parent CmpMap dropping its `InvA` or `InvB`. The port carries the applied
direction as a separate value in `MergeCtx` and folds it into the leaf when the
tree is rebuilt (`simplify.rs:3112-3114`, applied at `:3132-3134` and
`:3142-3144`), reaching the same dump.

Two fixtures prove it, on independent configurations: `unit_parallel_invert_clear`
(a MathMap neighbour, 2 axes), which predates this audit, and
`unit_parallel_invert_pcd` (a PcdMap neighbour, 3 axes), added to check the first
was not passing by accident. `neg_unit_parallel_forward` is the control. All
three are byte-exact against the pinned C.

One residual difference is not observable in any fixture: C reports the clearing
as a change and so restarts its nominate loop, where the port reports `None`. If
that ever matters it will show as a resume-point difference, not as a wrong
Mapping.

**Correction, 2026-10-13: the retraction was wrong.** The rebuild did not reach
C's dump; `normalize_unitmap_children` (§10.22) did. With that rewrite disabled,
`unit_parallel_invert_clear` and `unit_parallel_invert_pcd` both fail, the
parent keeping its `InvB`. The fixtures passed, but not through the code this
entry credited, and the residual difference above was the gap itself.
`UnitMap::map_merge` now ports the whole parallel branch (`unitmap.c:539-624`):
it brackets the run of adjacent UnitMaps, replaces it in one step, and with no
neighbour clears the entry's applied direction while keeping the UnitMap's own
`Invert`, reporting the change as C does.

---

### 10.7 PcdMap's zero-distortion reduction is unreachable from the nominate loop — FIXED

`pcdmap.c:1417-1433` opens `MapMerge` by replacing a PcdMap whose distortion
coefficient is zero with a UnitMap, before any neighbour is considered. The port
had that logic in `PcdMap::simplified`, while `pcd_map_merge` tried the swap and
then `default_map_merge` and consulted neither. A zero-distortion PcdMap
nominated from inside a list therefore stayed a PcdMap.

This is the same shape as §10.1: the class reduction exists, and the seam that
would reach it from the nominate loop does not.

Exposed and closed. `pcd_zero_disc_series` is
`[MatrixMap(2,3), PcdMap(disc=0), MatrixMap(5,7)]`, which the pinned C collapses
to a single diagonal `MatrixMap(10,21)`. `pcd_zero_disc_lone` is the same PcdMap
on its own, and it passed from the start — the logic was in `simplified()` all
along and only the seam was missing, which is exactly what the pair
discriminates.

`pcd_map_merge` now performs C's opening move itself, before the swap
(`simplify.rs:2656-2664`), and returns immediately: C makes the reduction the `if`
of an if/else whose `else` holds the whole neighbour search, so a reduced PcdMap
never looks at a neighbour in the same pass. `self_reduce_map_merge` cannot
express that ordering, which is why the reduction is inline rather than delegated.

---

### 10.8 LutMap's linearity test used a hardcoded tolerance — FIXED

C's `GetLinear` (`lutmap.c:598-627`) decides whether a lookup table is linear,
and `MapMerge` replaces a linear LutMap with an equivalent WinMap. Its tolerance
is scaled by the `LutEpsilon` attribute — the stated relative error of the table
values, `DBL_EPSILON` by default — and taken as the larger of two estimates, one
from the table's full range and one from the interpolated value at the element
under test, with linearity accepted within twice that.

The port used a hardcoded `1e-10`, a `1e-20` floor with no analogue in C, and one
estimate formed from the end values rather than the table's extremes. Roughly six
orders of magnitude looser, so it replaced nearly-linear LutMaps with WinMaps
where C keeps the LutMap — a shape difference in the simplified tree, not a
numeric one.

`GetLinear` is now ported as it stands, including the `hi == lo` constant-table
case, and `LutMap` carries a `lut_epsilon` field defaulting to `f64::EPSILON` and
read from the `LutEps` card (`lutmap.c:2012`) so a dump round-trips.

Two fixtures. `neg_lut_near_linear` departs from linear by about 3e-13 relative —
above `DBL_EPSILON`, far below `1e-10` — and C keeps it a LutMap; the port
converted it. `lut_exactly_linear` is the control C does replace with a WinMap,
without which the negative case would pass for a port that never converts a
LutMap at all.

No public setter was added. C's `LutEpsilon` setter is one of the eleven
clear-on-set sites (§9.2), so adding one means adding the clear; the attribute is
read from the dump and otherwise fixed at construction.

---

### 10.9 TimeMap drops one no-effect conversion where C drops three — FIXED

`timemap.c:2260-2266` eliminates a change-of-system step whose derived net offset
is zero, and applies it to `AST__MJDTOMJD`, `AST__MJDTOJD` and `AST__JDTOMJD`
alike, testing `cvtargs[ istep ][ 2 ]`. That third argument is derived at
`timemap.c:503,507,511`:

- `MJDTOMJD`: `args[0] - args[1]`
- `MJDTOJD`: `args[0] - args[1] + 2400000.5`
- `JDTOMJD`: `args[0] - args[1] - 2400000.5`

`time_step_is_no_effect` (`timemap.rs:1163-1174`) tests `args[0] == args[1]` and
only for `MjdToMjd`. For that type the two tests are the same condition. For the
other two they are not: a `MjdToJd` whose JD offset absorbs the 2400000.5
constant is a genuine no-op that C removes and the port keeps, leaving an extra
conversion step in the rebuilt TimeMap.

The port already derives `args[2]` identically (`timemap.rs:492-501`), so closing
this is a matter of testing that value for all three types instead of comparing
the first two arguments of one.

Exposed and closed. `time_mjdtojd_noop` and `time_jdtomjd_noop` are lone steps
whose offsets absorb the constant; the pinned C simplifies each to a UnitMap and
the port did not. `neg_time_mjdtojd_real` is the control, an MJDTOJD whose
offsets do not absorb the constant and which must survive.

Why the existing corpus missed it: `time_2arg_swapped_cancel` holds
`MJDTOJD {0.0, 2400000.5}` followed by `JDTOMJD {2400000.5, 0.0}`. Both steps
have a zero derived offset, so C drops them by this rule — but the port drops
them by the inverse-pair rule, reaching the same UnitMap for a different reason.
A lone step is what separates the two.

---

### 10.10 SwitchMap left its simplified children untagged — FIXED

This entry originally blamed the fused either/or: C's
`if( invert_list[where] ) { normalise; } else { simplify children; }`
(`switchmap.c:1052-1100`) makes those alternative outcomes of one nomination,
where the port did both in one call. Tracing C confirmed the shape — the same
SwitchMap is nominated at `inv=1`, then again at `inv=0`, then a third time once
the children were reduced — but that was not the cause.

Giving `map_merge` C's either/or was implemented and reverted twice: once in the
wrong order, which broke `switchmap_inverse_cancel` on both `equal()` and
*transform* comparisons because C attempts the neighbour cancellation first
(`switchmap.c:992`) and gates the self-simplify half on "no change made above"
(`:1054`); and once in the right order, which changed no fixture at all.

The cause was the tags. C reaches each encapsulated Mapping through
`astSimplify`, which records whatever it returns (`mapping.c:24763`), and the
port used `simplified()` — the untagged virtual — for the selectors and the route
Mappings. So C emitted 4 cards on `switchmap_invert_normalize` and the port
emitted 1. The trace made this readable directly: three `SIMPLIFY-ENTER ZoomMap`
entries plus the top-level SwitchMap is exactly C's 4.

Both child paths now stamp what they get back, as `RateMap` and `TranMap` already
did. All four fixtures — `switchmap_invert_normalize`,
`switchmap_internal_simplify`, `switchmap_fwdonly_simplify`,
`switchmap_inv_invonly` — pass byte-exact, and cause (A) is retired from both
pinned lists in `test_simplify_regression.rs`. Simplify serialisation failures
drop from 11 to 7.

§10.16 then moved the whole self-simplification into `SwitchMap::map_merge`, where
C keeps it, and gave it C's either/or after all: the third revert had selected the
arms on the SwitchMap's own `Invert` flag, but `flatten` resets every leaf to
forward and carries the inversion in the merge list, so the selector is
`ctx.entry(where_).1 != self.is_inverted()`. The corpus is unchanged by that move,
and the top-level SwitchMap normalisation the public entry point used to perform
is now dead code and removed.

---

### 10.11 TranMap's `map_merge` reached one of C's self-simplification cases — FIXED

`tranmap.c:834-897` gives a nominated TranMap four outcomes: normalise an
inverted TranMap to the equal forward form and stop; simplify the two component
Mappings; collapse a TranMap whose components are inverses of each other to one
of them; or fall through to the neighbour merge.

All four used to live in `TranMap::simplified`, and `TranMap::map_merge`
implemented only the component simplification, inline. A TranMap nominated from
inside a list therefore kept its inversion where C replaces it, and was not
collapsed when its two components are inverses.

§10.16 moved the whole body into `TranMap::map_merge`
(`mappings/tranmap.rs:262-339`), where C keeps it, with C's three exclusive cases
selected on `ctx.entry(where_).1` — `flatten` resets every leaf to forward and
carries the inversion in the merge list, so the requested orientation is the
merge entry's flag rather than the object's own. Case 3 stores the surviving
component normalised to forward with its intrinsic flag beside it, so the
replacement takes part in further merging exactly as C's does.

Two fixtures gate the cases the corpus could not reach before, because both need
the TranMap to be nominated from inside a list rather than as the whole Mapping:
`tranmap_invert_in_list` for `tranmap.c:837-850` and
`tranmap_equal_components_in_list` for `tranmap.c:864-887`. Both come out of C as
predicted — the first serialises a forward TranMap with the components swapped
and inverted, the second collapses to a single `ZoomMap` with `Zoom = 10` once
the exposed component merges with its neighbour — and both are byte-exact in the
port.

---

### 10.12 `GetDefUnc`'s unbounded-axis guard missed the upper bound — FIXED

`region.c:3560-3573` builds the default uncertainty as a Box covering 1.0E-6 of
the Region's base-Frame bounding box, and skips any axis whose bound is
`±DBL_MAX`, setting both output bounds to zero for it.

The port's guard was `!is_bad(ubnd[i]) && !is_bad(lbnd[i])`, and `is_bad(v)` is
`v == BAD_VALUE || !v.is_finite()` (`mappings/helpers/mod.rs:11-13`) with
`BAD_VALUE == -f64::MAX`. That catches an unbounded *lower* bound, which is
`-f64::MAX` and therefore `BAD_VALUE`. It did not catch an unbounded *upper*
bound: `f64::MAX` is finite and is not `BAD_VALUE`, so the axis took the bounded
branch and received a half-width of roughly `0.5e-6 * f64::MAX`, about 9e301,
where C gives it zero.

`regions/mod.rs:471` now carries C's asymmetric test as it stands —
`ubnd[i] != f64::MAX && lbnd[i] != -f64::MAX` — alongside the `is_bad` handling
the surrounding code needs. The magnitude form (`ubnd[i].abs() == f64::MAX`)
would have been wrong in the other direction: C takes the bounded branch for
`lbnd[i] == +DBL_MAX`.

The gate is `an_axis_unbounded_above_gets_a_zero_width_default_uncertainty` in
`crates/ast-core/tests/test_regions.rs`, which builds a one-axis `Interval` with
no upper limit, confirms `reg_base_box2` reports `+f64::MAX` for it, and asserts
both default-uncertainty bounds are zero. Before the fix it measured a half-width
of about 9e307; the guard is wrong on its own terms whether or not a Region using
the base rule can reach an unbounded axis today, so the direct unit test is the
gate rather than a fixture.

---

### 10.13 Box's PermMap simplification branch was unported — FIXED

`box.c:3719-3882` is an explicit branch for the case where a Box's base-to-current
Mapping is a PermMap. It reads the PermMap's permutation arrays and constants
directly and builds the current-Frame Region axis by axis:

- an output fed a constant gets that constant as both its limits;
- an output fed `AST__BAD` is unbounded, so the result is an Interval, not a Box;
- an output fed an input takes that input's limits;
- a missing bound likewise makes the result an Interval;
- when base axes are absent from the current Frame, the slice the PermMap selects
  is checked for intersecting the Box, which can make the result a NullRegion.

`BoxRegion::simplify_over_permmap` (`box_region.rs:426-525`) now carries all five
cases in C's order, ahead of the generic best-fitting-Box path, and the branch is
a downcast on the base-to-current Mapping (`box_region.rs:1629`). A permutation
that is not bi-directional returns `None`, which clears `simpler` and so discards
the parent simplification along with it, as `box.c:4064` does.

Two conventions had to be translated rather than copied. The port stores the
public 1-based permutation form (`+j` names axis `j`, `0` names `AST__BAD`, `-j`
names `constants[j-1]`) where C stores the 0-based form and `astPermMapId_`
(`permmap.c:2678-2707`) turns a public `0` into `nin` / `nout`; C's `feed >= nin`
test is therefore `feed == 0 || feed > nin` here. And C's `isNull` is an
assignment rather than an accumulation, so the last constant-fed input decides —
mirrored deliberately and recorded in `docs/issues/c-library-quirks.md`.

Three fixtures gate it: `cap_box_permmap_const` (an output pinned to a constant),
`cap_box_permmap_null` (a slice that misses the Box, giving a NullRegion) and
`cap_box_permmap_nonbidi` (a non-bidirectional permutation, its own reference
because C does not simplify it).

**Which construction the generator needed.** The public `astMapRegion` is
`astMapRegionId_`, whose tail simplifies the Region it is about to return
(`region.c:14020-14024`), so a fixture built through it holds the *answer* in its
`.map` and proves nothing — the same defect §10.17 records for
`normmap_frame_simplifies`. `astBox` on a FrameSet is no escape either: `astInitRegion`
resolves a supplied FrameSet to its current Frame. The three fixtures therefore call
the protected `astMapRegion_` (`region.c:5375`), which does not simplify, reaching it
through `astMakePointer` and using the protected `astWrite_` / `astSimplify_` on the
true Object pointer it returns. Any future fixture over a Region's base-to-current
Mapping needs the same construction.

**The unbounded exit has no oracle fixture.** `astMapRegion` refuses to build a
Region whose forward transformation gives a bad axis value, reporting
`AST__NODEF` (`region.c:5539`), and it is the only public way to install a
base-to-current PermMap — so no C program can produce a Box whose PermMap feeds
an output with `AST__BAD` (audit rows 21 and 23). It is covered instead by
`box_region.rs::tests::permmap_bad_output_simplifies_to_unbounded_interval`,
which performs the same FrameSet edge surgery `map_region` performs, without the
`AST__NODEF` check, and asserts the result is an Interval unbounded on the
affected axis. What would expose a divergence against C: a C path that installs a
base-to-current PermMap without that check — reading a Native Region dump whose
`FrmSet` already carries one is the only candidate, and nothing in the corpus
writes such a dump.

---

### 10.14 Whether the port over-reaches C's nominate loop — RETRACTED

Three versions of this entry claimed a reach or structure divergence, and a trace
of the C library disproved all of them.

`tools/oracle_fixtures/build_simplify_trace.sh` instruments the three dispatchers
in `mapping.c` — `astSimplify_`, `astMapMerge_` and `astMapList_` — so one build
reports every nomination, decomposition and short-circuit, for every class. Run
on `cap_cmpmap_01` it prints, at depth:

```
MAPMERGE RateMap where=1 series=0 nmap=3 issimp=0 inv=0
SIMPLIFY-ENTER CmpMap issimp=0 dns=0 inv=0 nin=1 nout=1
```

So C **does** nominate that RateMap, its `MapMerge` **does** run, and it **does**
call `astSimplify` on the encapsulated CmpMap, unwalled. Nothing about reach
differs. What differs is the orientation: C enters with `inv=0` on a CmpMap whose
dump says `Invert = 1`, because `astSetInvert( emap, map->invert )` put it at the
construction-time orientation first. That is §10.1's second half, and fixing it
there closed this.

What the three wrong versions had in common: each inferred a rule from a
fixture's output instead of watching the library run. The trace script exists so
the next question of this shape costs one command.

Two by-products are kept. `parpair_aligned_reduces`, `parpair_one_side_reduces`
and `neg_parpair_aligned_irreducible` pin C's parallel-pair expansion rule, which
the port already implements correctly. And one latent finding stands: no Region
class checks the `IsSimple` bit (`box_region`, `circle`, `cmpregion`, `ellipse`,
`interval`, `nullregion`, `pointlist`, `polygon`, `prism`, `regions/mod.rs`), so
the port re-simplifies a Region C returns untouched — measured as unobservable on
this corpus, and not shipped.

---

### 10.15 The `C-ARTIFACT` set: three layers redundant, the rest load-bearing — RESOLVED

`cmpmap.rs`'s module doc defined a `C-ARTIFACT` marker for items that "exist only
to reproduce a serialised value that C emits as a side effect of its memory
model". Nine sites, all about `InvA`/`InvB`. Auditing them the way
`dump_simplified` was audited — disable, measure — found **four** mechanisms
pushing an inverted CmpMap's inversion into its structure, of which three were
redundant.

Measured individually and then all together, against the full suite including all
194 `wcsconv` byte comparisons:

| layer | verdict |
|---|---|
| `apply_effective`'s structural branch in `reconstruct` | redundant |
| the canonicalisation in `CmpMap`'s recorded-orientation fixup | redundant |
| `canonicalize_inverted_cmpmaps`, the tree-wide pass | redundant |
| `normalize_unitmap_children` | **load-bearing** |

All three redundant layers are deleted, along with `invert_into_structure` and
`invert_parallel_child`, which had no other callers. `normalize_unitmap_children`
survives and its doc now records what demands it: removing it costs four
`equal()` failures — `cap_cmpmap_01`, `cap_cmpmap_04`, `cap_wcsmap_01` and
`parcomp_inverted_nested_reemitted` — plus two serialisation failures.

**Two things are worth keeping from how this went.** The three deleted layers
each carried a note saying they were dump-shaping and measured; the one that
turned out to matter was the one marked "not independently measured". And the
measurement only became meaningful once the corpus demanded the behaviour:
`parcomp_inverted_reemitted` and `parcomp_inverted_nested_reemitted` were
generated for it — an inverted parallel CmpMap that must be re-emitted while the
tree is rebuilt around it, since the trailing ZoomMaps merge and the parallel
component merges with nothing. C's output pushes the inversion down: no CmpMap
carries `Invert`, and the flags appear as `InvA` on the components.

**The rest of the set, measured.** `set_first_orientation` and
`set_second_orientation`, with their `winmap_merge.rs` caller, are strongly
load-bearing: disabling the flip costs 12 further simplify serialisation failures
*and 5 `wcsconv` byte comparisons* — `grism`, `skyref`, `skyref-alt`,
`tidy-offsets` and `tidy-pole`, all native. Their note claimed "8 serialisation
differences", which understated it and evidently never covered `wcsconv`, the
strict gate. `equal()` and transform comparisons stay clean either way, so the
"dump-only" label is accurate; "removable" was not.

Both `parcomp_*` fixtures are among the 12, so these setters are the second of
the two layers that produce C's inversion push-down, alongside
`normalize_unitmap_children`.

Every surviving note now names the fixtures that demand it, and the module doc
records that the set has been audited rather than leaving "candidates for
deletion" to be read as "safe to delete".

**Why this mattered beyond tidiness.** Every layer here existed to make a dump
match, and the dump is the contract, so a redundant layer is a place where a
correct change looks like a regression. That is how §10.1 cost three attempts:
`RateMap::invert_snapshot` was documented as existing "for serialization
compatibility" and was therefore ignored by the simplify path that needed it.

---

### 10.16 The port has a per-class `simplified()` seam that C does not — FIXED

C has **two** per-class seams in the simplify machinery:

- `Simplify`, overridden by `CmpMap`, `FrameSet` and the nine Region classes and
  by nobody else. For every other class the base implementation
  (`mapping.c:16752`) builds a one-element list and calls
  `astMapMerge( map, 0, 1, &nmap, … )` **repeatedly** until nothing changes or
  the list grows past one (`mapping.c:16871-16883`).
- `MapMerge`, overridden per class.

So for a class outside that 11, every reduction C performs lives in `MapMerge`,
and `astSimplify` reaches it only by nominating the Mapping in a list of one.

The port had **three**: the public `simplify()`, a per-class `simplified()`, and a
per-class `map_merge()`. `simplified()` had no C counterpart for the other 25
classes, and the port put per-class reductions there.
`grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c` is the
authority: the split was exactly right for 11 classes and exactly wrong for 25.

**This is the H1 hazard, and it produced three bugs.**
RateMap's encapsulated-Mapping reduction (§10.1) and PcdMap's zero-distortion
replacement (§10.7) both lived in `simplified()` with no `map_merge` to reach
them, so neither ran on a nominee inside a list — C's only route. Both fixes were
the same shape: give `map_merge` the reduction C keeps there. §10.3's
blanket-stamp failure is the third: stamping `simplified()` over-tags precisely
because it is not `astSimplify`.

H1 warns about *collapsing* C's seams into one central `match`; here a seam was
*added*. The failure mode is identical, because what H1 is really about is the set
of seams differing from C's.

Planned in
[`docs/superpowers/specs/2026-08-23-simplify-seam-realignment.md`](../superpowers/specs/2026-08-23-simplify-seam-realignment.md),
with the task breakdown in
[`docs/superpowers/plans/2026-08-23-simplify-seam-realignment.md`](../superpowers/plans/2026-08-23-simplify-seam-realignment.md):
six tasks, fixtures first, one class per commit, and a test that holds the keeper
list so the seam cannot drift back.

**Done.** Twenty-five class bodies moved out of `simplified()` into `map_merge`,
one commit each, and `simplified()` now survives only in the eleven classes whose
C counterpart overrides the `Simplify` vtab slot. The plan's count was 24; the
sweep found a twenty-fifth, `ChebyMap`, which had inherited a body nobody listed.

Two things the entry above did not anticipate:

- **The single-caller half is a compile error, not a test.**
  `Mapping::simplified` takes a `ViaSimplify`, a zero-sized token only
  `mappings::simplify_entry` can construct, so the seam has exactly one possible
  caller — C's `mapping.c:24772` — and reaching it any other way does not build.
  `test_simplify_seams.rs` is left holding the keeper list, which a type cannot
  express.
- **The gate is now unconditional.** Every reduction passes through
  `simplify_entry`, so every one short-circuits on `IsSimple` and stamps its
  result at `mapping.c:24763`'s equivalent. Serialisation failures in the simplify
  harness went 12 to 9. It also brought `cap_cmpmap_01` and `cap_cmpmap_04` to
  C's card count of 18, which §10.3 has since shown was a coincidence of two
  errors in opposite directions.

What it did not fix: what §10.3 then called the WinMap under-merge, which the
entry above implied was downstream of the seam and is not — and which turned out
not to be an under-merge at all (§10.3). Cause (G) in §9.2 named the
`simplified()` gate as the source of five fixtures' extra cards and predicted they
would close here. They did not; the real source was two `.simplify()` calls in
CmpMap's parallel handling with no C counterpart, both of which have since been
removed (§10.3).

---

### 10.17 NormMap did not simplify its encapsulated Frame — FIXED

`normmap.c:619-631` is the *first* thing C's `MapMerge` does: `astSimplify` the
encapsulated Frame, and if that returns a different object, replace the NormMap
with a new one wrapping the simplified Frame and report a change. Only if the
Frame did not simplify does C reach the basic-Frame test that turns the NormMap
into a UnitMap (`:632-640`), and only after that does it consider neighbours.
The class C reads for that second test is `astGetClass( smap )`, the class of
the *simplified* Frame, so the two tests are one call apart rather than one
pass.

`NormMap::map_merge` (`normmap.rs:210-221`) performs both, in that order, and
the module comment records why the order is load-bearing.

**The seam came first.** `Simplify` is a Mapping vtab slot every Frame inherits
(`mapping.c:16853`), which is what lets `CmpFrame` override it to simplify its
components and rebuild (`cmpframe.c:9018-9034`), `FrameSet` override it
(`frameset.c:10111`) and every Region override it. The port had no Frame-level
simplify at all: `FrameSet` was the only type under
`crates/ast-core/src/frames/` implementing `Mapping`, so `CmpFrame` had nothing
to override. `Frame::simplify_frame` (`frames/mod.rs:240`) is the port of the
slot, with the `Option` convention `Mapping::simplified` uses — `None` is what C
signals by handing back the pointer it was given, which is `normmap.c:626`'s
identity test. The default is `None`; `CmpFrame` folds its two components;
`FrameSet` and the nine Region classes delegate to their Mapping side through
`simplify_frame_via_mapping` (`frames/mod.rs:50`), which asks
`simplify_reporting_change` whether anything reduced and drops a result that is
no longer a Frame — C's `astIsAFrame` guard (`cmpframe.c:9024`).

**The fixture that could not discriminate is retired.**
`normmap_frame_simplifies` built a NormMap over a Box whose FrameSet held
`CmpMap(ZoomMap(2), ZoomMap(3))`, expecting the Box to reduce. It did not:
`astBox` resolves a supplied FrameSet to its current Frame at construction
(`region.c:13311-13323`), so the dumped `.map` already held a Box over a plain
`ZOOMED` Frame with the bounds transformed, both halves of the fixture carried
the same Box, and the port passed it by declining a branch it had not ported. It
is deleted, generator case and both references.

`cap_normmap_cmpframe` replaces it: a NormMap over a CmpFrame whose first
component is a FrameSet whose base→current Mapping is `CmpMap(ZoomMap(2),
ZoomMap(3))`. The CmpFrame does not absorb the reduction at construction, so
`astSimplify` has a different Frame to return and C takes the rebuild branch —
the `.simp` differs from the `.map` in the FrameSet's edge Mapping, `ZoomMap(6)`
against the CmpMap. A CmpFrame holding a *Region* was the first shape tried and
was rejected: C's `astEqual` returns 0 for a CmpFrame containing a Region
compared with its own copy, so the harness's `equal()` gate could not have
applied to it. The FrameSet component compares equal and still discriminates.

**Where the rebuild was being discarded.** `try_self_reduce`
(`mappings/simplify.rs:1111`) installed a class's replacement only when it
compared unequal to the Mapping it replaced. Frame equality is conversion-based,
so a NormMap rebuilt around a Frame whose *internal* Mapping reduced compares
equal to the original and the rebuild was dropped. C's test is on the pointer
`astSimplify` handed back and C makes it in the class body, which is where the
port's three callers make it too — each returns `None` when nothing changed — so
the helper now installs whatever it is given.

**Two defects the fixture exposed on the way.** `FrameSet::snapshot` returned
`current().snapshot()`, and `snapshot` doubles as the deep-copy hook for a `dyn
Frame`, so cloning any object holding a FrameSet as a Frame silently downgraded
it to a plain Frame; it now clones the FrameSet, as `astCopy` does, and
`FrameSet::map_from` unwraps a FrameSet `previous` so an integrity delta stays a
delta between current Frames. `RegionData::new` had been relying on that lossy
snapshot for C's supplied-Frame resolution, which is now ported explicitly
(`regions/data.rs`): a Region contributes its own encapsulated Frame, a FrameSet
its current Frame, and any other Frame is used as supplied.

**FrameSet's half.** The three `frame_frameset.md` rows that shared this section
— 10, 12 and 14, C's `tmp != map` per edge, the `simpler` flag it raises and the
`astClone( simpler ? new : this )` return — were gaps because the seam returned
a Mapping without saying whether it had reduced anything, so
`FrameSet::simplified` handed back its working copy unconditionally.
`Mapping::simplified` returns `(DynMapping, bool)` and all three are ported.
`cap_frameset_nochange` is the fixture: a FrameSet whose one edge is a UnitMap,
gated by `frameset_with_no_reducible_edge_returns_the_original` in
`test_simplify_seams.rs` rather than the simplify manifest, because that harness
reduces a FrameSet input to its base→current Mapping first.

---

### 10.18 Polygon's vertex-transform simplification skipped both of C's guards — FIXED

`polygon.c:5379-5437` re-expresses a Polygon in its current Frame by
transforming the vertices, and guards the result twice before adopting it. The
port transformed the vertices and adopted the result unconditionally, and built
the new Polygon with the uncertainty as stored, in the base Frame, where C
passes `astGetUncFrm( new, AST__CURRENT )` — the uncertainty mapped into the
Frame the new vertices live in (`:5384`). `SimpVertices` did not exist at all:
no attribute, no accessor, no `SimpVT` card.

All four are now ported in `Polygon::simplified` (`polygon.rs:1151-1253`), and
`docs/audits/mapmerge/region_polygon.md` rows 7, 11, 12 and 13 are `ported`.

- **The uncertainty Frame.** `get_unc_frm(FrameRef::Current)` is called
  unconditionally, as C calls it: a Polygon with no explicit uncertainty gets
  its default here, and the `reg_overlay` in the tail restores the unset state.
  `cap_poly_unc_frame` pins it — a Polygon with a ±0.1 Box uncertainty over a
  ZoomMap of 4, whose simplified `Unc` block is the Box scaled by the zoom, not
  the base-Frame Box.
- **Bent edges.** With `SimpVertices` zero the original Polygon is meshed and
  every mesh point must lie on the replacement's boundary (`:5412-5428`), which
  is what catches a Mapping that carries the vertices to the right places while
  bowing the edges between them. `cap_poly_bent_edges` is a Polygon over
  `y1 = x1 + 0.005*x2*x2` with the attribute cleared and a ±0.01 uncertainty:
  the 0.005 bow over the 20-unit edge exceeds the 0.01 uncertainty, so
  `astRegPins` rejects it and C keeps the original.
  `cap_poly_bend_allowed` is the same geometry with the attribute left at its
  default, where C adopts the transformed Polygon — the pair is what makes the
  branch falsifiable, since a check that always failed would pass the first
  fixture alone.
- **`SimpVertices`.** Attribute, accessors and the `SimpVT` card C writes at
  `polygon.c:6692`, written only when the attribute is set, so an unset one
  leaves the round-trip untouched. The default is 1 (`polygon.c:6511`, where
  `-INT_MAX` is the unset sentinel).

**The bad-vertex guard is ported but cannot be fixture-pinned.** C clears `ok`
when a transformed vertex has `AST__BAD` on axis 1 and builds nothing in that
case (`:5400-5407`); axis 2 is copied without being tested, and the port
reproduces that asymmetry rather than tidying it. No C program can reach the
guard: a Polygon cannot hold a bad vertex (`polygon.c:7131` rejects one, which
`PolygonBuilder::build` now does too), and `astMapRegion` refuses any Mapping
that sends a defining point to `AST__BAD` (`region.c:5546`), so neither public
API can install a base-to-current Mapping that produces one. What would expose a
divergence: a path that installs such a Mapping without that check — the same
candidate §10.13 records for Box's unbounded PermMap exit, and nothing in the
corpus writes it.

**One shared primitive changed with it.** C reaches the uncertainty through
`SetUnc`, whose `Conv` helper (`region.c:2315`) aligns in the Domain of the
target Frame and then re-instates both FrameSets' base Frame index with
`astSetBase` (`:2373-2374`). Restoring it that way leaves the attribute *set*
even when it was unset on entry, so every uncertainty assembled through
`set_uncertainty` dumps `Base` as an active card rather than a commented
default. `region.c:2367-2374` is ported in `Region::set_uncertainty`
(`regions/mod.rs:366-386`); without it both new fixtures differ from C on that
one card.

---

### 10.19 Two pieces of the simplify engine had no counterpart in C — both closed

Both sat in `mappings/simplify.rs`, in the port of `cmpmap.c::Simplify`. They are
listed together because the question they raised is the same: what did the engine
do that C's does not, and what showed it?

**A recursion-depth cap of ten, and a structural hash standing in for
`astEqual` — CLOSED.** `SimplifyGuard::enter` refused to simplify at all once ten
nested `simplify_mapping` calls were on the stack, returning the mapping as
supplied, and compared a structural hash (`mapping_fingerprint`) where C compares
Mappings. C has neither: `cmpmap.c:3487-3498` keeps `simplify_stackmaps` purely to
detect a Mapping *already being simplified*, holds one `astClone`d Mapping per
depth, tests each with `astEqual`, and grows the array with `astGrow` without
bound. Both substitutes declined reductions C performs — the cap unconditionally
past its depth, the hash whenever two Mappings of the same class and shape differ
only in their numbers. A constant with no analogue in the C source is stop signal
1, and this one changed the answer rather than merely costing time.

The guard is now C's: a `Vec<DynMapping>` pushed and popped by the same RAII
guard, with `Mapping::equal` as the test.

The hash half closed by demonstration. Instrumenting the guard to run both tests
and report where they part company found exactly one disagreement across all 454
corpus fixtures — `box_unc_fillfactor_merge`, at depth 1, where the hash claimed
a cycle and `equal` did not — and that fixture is byte-exact against the pinned C
now that the guard recurses there. Its `SERIAL_UNGATED` row under cause C is
retired; `nullregion_unc_fillfactor_merge` remains, so the Prism-for-Box
uncertainty defect in §6 cause C is a smaller thing than the pair suggested.

The depth half closed by construction, not by demonstration, and the reason is
worth recording because it says something about the port's shape. Nesting depth in
the *input* does not drive recursion depth here: `flatten` is iterative, so an
eleven-deep left-nested series CmpMap is levelled in one pass and enters the guard
once. `cap_deep_nest_zoom` was generated to reach eleven levels — eleven nested
series CmpMaps each holding a ZoomMap, which C collapses to a single ZoomMap — and
it passed at HEAD with the cap still in place. Instrumenting the refusal path
measured the real ceiling: the whole tier-1/2 suite never exceeds depth 7, and
only `cap_cmpmap_01` and `cap_cmpmap_04` reach that. The depth the engine does
reach comes from `map_merge`-internal simplify calls on trees under construction,
not from how deeply the caller nested its input, so exposing the old cap would
have needed a fixture that drives *that* number past ten. `cap_deep_nest_zoom`
stays in the corpus as a positive control for the collapse C performs, not as
coverage of the cap.

Removing the cap trades a silently-declined simplification for a stack overflow
on a genuine non-terminating recursion. That is C's exposure too, and a crash is
findable; if one appears the bug is in whichever `equal` fails to recognise the
repeat.

**A UnitMap strip that duplicated `UnitMap::map_merge` — CLOSED.**
`merge_loop_restricted` stripped `UnitMap` entries from a series list twice — inside
a fixpoint loop that re-ran the whole nominate loop after each strip, and again
after it — where C strips a series UnitMap in `UnitMap::MapMerge`, when the UnitMap
is nominated (`unit.rs:112-115`, `unit.md` rows 5-7). Both outer strips are gone;
`merge_loop_restricted` is now the nominate loop and nothing else.

The strip itself still happens, from the nominate loop, which is where C does it:
the whole tier-1/2 suite is green, including the two unit tests written against the
outer strips (`merge_loop_strips_unitmaps_from_series`,
`merge_loop_all_unitmaps_preserves_one`). Removing them also removes a source of
extra nominations, since the in-loop `continue` re-ran the loop over the whole
list after every strip (§10.3).

What they would have got wrong: under a restricted simplify a UnitMap that does
not carry `AllowSimplify` is never nominated, so C cannot strip it, while neither
outer strip consulted the flag. Nothing in `ast-core` sets `RestrictedSimplify`
except reading a `ReSimp` card (`cmpmap.rs:1034`), and nothing sets
`AllowSimplify` except reading an `AlSimp` card (`serial/mod.rs:160`), so that
case had no fixture in either direction until the three restricted-mode pairs of
§10.20 supplied one.

---

### 10.20 The restricted-simplify path dropped three of C's steps — FIXED

C's `Simplify` treats a restricted simplify as a distinct mode. Three of the
things it does in that mode had no counterpart in the port; all three are now
ported, and each has a fixture pair that fails without it.

**`simpler` is reset — FIXED (`simplify.rs:3336`).** `astMapList` sets `simpler`
whenever it meets an inverted CmpMap during the decomposition, and C then
discards that flag under a restricted simplify — it cannot know yet whether the
inverted CmpMap will be eligible, so it declines to count the decomposition
itself as an improvement (`cmpmap.c:3526-3530`). The port computed the same flag
and never consulted `restricted`, so where C returns `astClone( this )` the port
rebuilt the tree: components got swapped and inverted, and the result
right-associated. `cap_restricted_no_eligible` is the gate — a restricted CmpMap
holding an inverted CmpMap with nothing marked eligible, whose `.simp` differs
from its `.map` only in the dropped `ReSimp = 1` card.

**`AllowSimplify` is cleared on each component — FIXED
(`simplify.rs:3378-3382`).** C clears the flag on every component as it folds the
result back into a CmpMap (`cmpmap.c:3689-3692`), explicitly so the protected
flag does not appear in a public dump. C does this only in its multi-element
branch; a single-element result is an `astCopy` (`cmpmap.c:3651`) whose flag
`astSimplify` clears on the result instead. `cap_restricted_component_flags` is
the gate — three eligible components under a restricted CmpMap, `AlSimp` on
three cards in the `.map` and on none in the `.simp`.

**`astSimplify_`'s tail is C's — FIXED (`simplify_entry.rs:105`).** Under a
restricted simplify C clears `RestrictedSimplify` on the result and does *not*
set `IsSimple` (`mapping.c:24779-24786`), then clears `AllowSimplify` on the
result whichever branch ran (`mapping.c:24788`). `cap_restricted_inverted` is the
gate: its `.simp` carries neither `ReSimp` nor `IsSimp` nor `AlSimp`.

The mode's fourth consult point, the base driver's opening
`if( astRestrictedSimplify(this) ) { if( !astAllowSimplify(this) ) return
astClone(this); }` (`mapping.c:16853-16855`), is ported at `simplify.rs:1146-1148`.
`RestrictedSimplify` moved from a `CmpMap` inherent method to a `Mapping` trait
pair, matching C's shared `flags` word (`mapping.h:415-482`); only `CmpMap` has
storage for it, the only class that can hold components to restrict.

The fixtures are generated by `gen_audit_gap_fixtures` in the submodule's
`gen_simplify_fixtures.c`. Setting the two protected flags needs a separate
translation unit — `gen_simplify_flags.c`, compiled with `astCLASS` and the
internal headers, converting IDs with `astMakePointer_` — because the generated
`build/ast.h` strips every `astCLASS` section, so defining the macro in a program
that includes only `ast.h` does nothing. That TU must also be compiled with
`THREAD_SAFE`, which is `PRIVATE` to the `ast` cmake target: without it the
`AstObject` layout it sees differs from the library's and the flag macros write
over the lock field.

`YamlChan` remains the only producer of these flags in C (`yamlchan.c:10998` for
`RestrictedSimplify`, `:9505`, `:9627` and `:10220` for the rad/deg ZoomMaps it
flags with `AllowSimplify`); in the port they still arrive only by
deserialisation (`cmpmap.rs:1034`, `serial/mod.rs:160`). Porting `YamlChan` would
exercise the mode as C's own callers do.

---

### 10.21 `astDoNotSimplify` and the Mapping-level `Ident` it reads — FIXED

C gates simplification on a per-class protected method with three callers, all in
generic drivers.
`astSimplify_` refuses to reach the `Simplify` slot when it returns non-zero
(`mapping.c:24764`), `astMapList` refuses to decompose a CmpMap when it does
(`cmpmap.c:1154`), and `astMapMerge_` refuses to reach the `MapMerge` slot
(`mapping.c:24257`), so a flagged Mapping is neither simplified, taken apart nor
merged. The third site is §10.29.
The base rule is "the `Ident` attribute is set" (`mapping.c:1107-1152`), on the
reasoning that a user who named a Mapping wants it to survive intact.
`Frame` overrides it to return zero always, so Frames are simplified despite a set
`Ident` (`frame.c:3511-3554`).

The port had neither the seam nor the attribute it reads, so nothing in it could
put a Mapping into the state C checks for — the gap was inexpressible rather than
merely untested.
Both are now `Mapping` trait methods: `ident` / `set_ident` / `clear_ident` /
`test_ident` over `Option<String>` storage on all 26 concrete classes, and
`do_not_simplify`, whose default body is `self.test_ident()` exactly as C's is
`astTestIdent( this )`.
The predicate is consulted at both of C's sites, sharing the `astClone( this )`
exit with the `IsSimple` short-circuit in the gate (`simplify_entry.rs`) and
guarding the decomposing branch of `flatten`, the port of `astMapList`.
Every `Region` and `FrameSet` overrides it to `false`, mirroring C's `Frame`
override.
Storage is `Option<String>` rather than a `String` tested for emptiness because
C's rule is `astTestIdent`, which is `this->ident != NULL` (`object.c:5229`) and
so distinguishes an `Ident` set to `""` from one never set.
Serialisation follows C's Object dump — the card is written when set and
non-empty, before the `IsA Object` marker (`object.c:1697`) — and the reader
applies it generically for every Mapping class.

Two things surfaced while closing it.
`FrameSet` now owns its own `Ident` instead of forwarding to its current Frame,
which is what C has done since its 30-OCT-2009 change (`frameset.c:2759`,
`:4841`, `:10830` all pass `this`); `test_frameset_repair.rs`'s
`frameset_delegates_domain_but_owns_ident` pins both halves.
And `CmpMap::equal` needed a termination guard: it uses `flatten` as a
canonicaliser, so two `Ident`-bearing same-kind CmpMaps each flatten to a
one-element list holding themselves, and the element-wise comparison
(`cmpmap.c:399`) is then the comparison it started with.
C has the same unbounded recursion; it is reachable only when *both* sides
refuse, and there the port decomposes one level of children instead, which is the
same comparison C would make with either side's flag cleared.

Two fixtures cover it, both generated by `gen_simplify_fixtures.c`.
`cap_ident_cmpmap` is the gate: a series `CmpMap(CmpMap(Zoom2, Zoom3), Zoom5)`
with `Ident` on the inner CmpMap, which C leaves whole — including the outer
ZoomMap, which cannot merge across a named component.
`cap_ident_frame` is the negative control for the `Frame` override: a FrameSet
with `Ident` set on the FrameSet itself, whose edge C still simplifies to a
single `ZoomMap Zoom = 6`.
Registering the second one required `test_simplify_regression.rs` to stop
pre-reducing every FrameSet input to its base→current Mapping; it now takes the
comparison shape from the reference's class, so a reference that is itself a
FrameSet is compared against simplifying the FrameSet.

---

### 10.23 The uncertainty simplification was tested with the wrong comparator — FIXED

`Region::Simplify` simplifies the encapsulated uncertainty and then, only if it
changed, runs the stability check and installs the result through `astSetUnc`.
C's test for "it changed" is `sunc != unc` (`region.c:11193-11196`) — pointer
identity on what the per-class slot handed back, so any reduction counts,
including one that leaves the transform alone and rewrites only the structure.

The port asked `Mapping::equal` instead. That predicate is conversion-based: it
composes the two Mappings and asks whether the result simplifies to a UnitMap.
An uncertainty whose own base→current Mapping collapses to a UnitMap therefore
reported *no change*, and the whole body was skipped — the simplified
uncertainty discarded, `astSetUnc` never reached, and `simpler` never set.

That single wrong comparator produced both halves of `cap_zoommap_04`'s
divergence: 59 lines of `FrmSet` inside `Unc` that C omits, because
`astSetUnc`'s `astSetRegionFS( this->unc, 0 )` (`region.c:10863-10869`) is what
drops the block, and `IsSimp = 3` against C's 0, because the stamps hang off the
FrameSet the port kept. Asking `simplify_reporting_change` — whose `false` *is*
C's `astClone( this )` exit, the one C's generic drivers detect by pointer
identity — makes the fixture byte-equal and retires both its `SERIAL_UNGATED`
and its `ISSIMP_DISAGREEMENTS` row.

The branch is now reachable for every Region rather than only for uncertainties
whose transform changes, so a Region fixture that moves here is one where the
stability check or `set_uncertainty` diverges, not this test.

---

### 10.24 A merged Region's combined uncertainty skipped C's Prism — FIXED

`MergeInterval` and `MergeBox` build the merged Region's uncertainty from the
two components' base-Frame uncertainties as `astSimplify( astPrism( u1, u2 ) )`
(`interval.c:1547-1565`, `box.c:1993-2011`). The port went straight to the
parallel CmpMap that `Prism::Simplify` starts from (`prism.c:3305-3336`),
reasoning that the Prism contributes nothing beyond that head.

It contributes the tail. `Prism::Simplify` ends with
`astRegOverlay( result, (AstRegion *) this_mapping, 0 )` (`prism.c:3513`), and
`astRegOverlay` copies raw fields — `Closed`, `Adaptive`, `RegionFS`, and, on an
axis-count change, `MeshSize` and `FillFactor` — from the Prism onto the reduced
Region. Every one of those sits at its constructor default on a Prism built
inside a merge, so the overlay is what clears them.

`cap_interval_01` is where it showed. Its time uncertainty is a bounded Interval,
which reclassifies to a Box through `Interval::Cache` — and `Cache` ends with an
unconditional `astSetClosed( this->box, astGetClosed( this ) )`
(`interval.c:512`), so that Box legitimately carries an explicit `Closed`, in C
as in the port. C's Prism overlay then wipes it; the port's CmpMap shortcut left
it, and the card surfaced in the merged Interval's `Unc` block.

`combine_uncertainties` now builds the Prism and simplifies it. The shortcut was
not an optimisation of C's path — it was a different path that happened to agree
whenever neither component had an attribute worth clobbering.

---

### 10.25 A bounded Interval reclassified to a Box without simplifying it — FIXED

C's `Interval::Simplify` opens by asking `Cache` for the equivalent Box and, if
there is one, returns `astSimplify( box )` and nothing else
(`interval.c:3196-3201`). The unbounded machinery below it never runs for a
bounded Interval.

`Cache` is what makes that work (`interval.c:441-513`). It builds the Box in the
Interval's **base** Frame from the resolved axis limits, and when the base→current
Mapping is not a UnitMap it maps it into the current Frame with
`astMapRegion( bbox, map, cfrm )` — which extends the Box's FrameSet rather than
moving its points. So the Box that reaches `astSimplify` carries the Interval's
whole base→current Mapping, and `Box::Simplify` is what re-expresses the bounds
in the current Frame's units.

The port had three copies of `Cache`'s classification — in `get_def_unc`, in
`reg_pins_mask`, and inline at the head of `simplified()` — and the third was
not `Cache`. It built a Box from the base-Frame numbers, labelled it with the
**current** Frame, copied `Adaptive`, `MeshSize` and `FillFactor` across (C's
`astBox` copies none of them), forced `Closed` on, and returned without
simplifying. The base→current Mapping was dropped on the floor.

`cap_interval_01` is where it showed. The time axis of its uncertainty is a
bounded Interval on a TimeFrame whose base→current Mapping is a
`Zoom = 1.1574074074074073e-05` ZoomMap — seconds to days. C's Box carries that
ZoomMap into `astSimplify` and emits `X7 = 5.7870370370370369e-10`; the port
emitted the unscaled `4.9999999999999996e-05`, 86400× larger.

There is now one `cache()` returning what C leaves in `this->lbnd`, `this->ubnd`
and `this->box`, and all three call sites use it. The condition is C's
`isBox && !neg` rather than the port's own `!negated && closed`, the uncertainty
is installed through `set_uncertainty` as `astBox`'s own constructor argument is,
and `simplified()` hands the result to the simplify gate.

Its `SERIAL_UNGATED` row is deleted. What remains on the fixture is `X5` / `X6`,
two sky half-widths the port reproduces from this machine's C to the last digit
and which the reference — captured elsewhere — has differing from each other;
those are licensed under `NUMERIC_PLATFORM_DRIFT` (§6).

---

### 10.26 `MergePointList` installed its combined uncertainty by assignment — FIXED

C's four parallel Region merges share one tail: build the merged Region on a
CmpFrame of the two **base** Frames, `astRegOverlay` the supplied Region's
attributes onto it, set the Negated flag, install the combined uncertainty with
`astSetUnc`, and `astMapRegion` the result into a CmpFrame of the two current
Frames through the parallel CmpMap of their base→current Mappings
(`pointlist.c:1359-1465`, `box.c:1983-2085`, `interval.c:1537-1639`,
`nullregion.c:571-776`).

`astSetUnc` is not a field write. It ends by re-centring the uncertainty on the
Region's first defining point (`region.c:10871-10876`):

```c
   if( this->points ) {
      ptr_reg = astGetPoints( this->points );
      astRegCentre( this->unc, NULL, ptr_reg, 0, AST__CURRENT );
   }
```

and `Box::RegCentre`'s set path does not merely translate the Box. It resolves
the new centre, then rebuilds the corner from the cached geodesic half-lengths
with `corner = GeoCorner( frm, ncb, bc, this->geolen, NULL, status )` and writes
both points. Re-centring a Box on the point it is already centred on therefore
still regenerates its corner, through whatever `astOffset` the base Frame
provides.

`PointList::region_merge` was the one merge that did not go through that tail. It
built the merged PointList on a CmpFrame of the two **current** Frames, applied
Closed and Adaptive as conjunctions of the two Regions rather than as
`astRegOverlay`'s copy from `this`, and assigned the combined uncertainty
straight into the merged Region's fields — so the re-centre never ran.

`cap_pointlist_01` is where it showed. Its 3-d TIME-SKY Prism merges a TimeFrame
PointList with an ICRS SkyFrame PointList, each carrying a Box uncertainty. C's
`astSetUnc` re-centres the combined Box through a `CmpFrame(TimeFrame,
SkyFrame)`, and the sky corner comes back from `palDcs2c`/`palDcc2s` moved in its
last digits: `X6 = -0.20069734795284888` against the input's
`-0.20069734797760203`. The port emitted the input corner verbatim.

The merge now calls the same `finish_region_merge` as `MergeBox` and
`MergeInterval`, which required one addition: `MergeOverlay` carries the Negated
value the owning merge sets, because `MergePointList` propagates the two
PointLists' common Negated flag (`pointlist.c:1367-1371`) where the other two
clear it. The pre-mapping variant of the FillFactor/MeshSize scaling went with
the hand-rolled path — C applies the scaling to what `astMapRegion` returns, so
one variant is all there was ever a caller for.

`cap_pointlist_01` now reproduces this machine's C capture byte-for-byte, which
empties `SERIAL_UNGATED`. Its `X5` / `X6` differ from the reference in the 12th
significant figure and are licensed under `NUMERIC_PLATFORM_DRIFT` (§6): the
reference was captured on another platform, whose libm parts company with this
one inside that geodesic round trip.

---

### 10.27 PcdMap's swap half was a sketch of C's — FIXED

`pcdmap.c::MapMerge` spends two thirds of its body on the swap: when neither
neighbour merges, it swaps the PcdMap past an adjacent ZoomMap or axis-swapping
PermMap, either to bring it next to a Mapping it can merge with, or — with no
such Mapping in reach — on the speculation that one of the two swapped Mappings
then simplifies on its own.

The port had the first half of that and none of the second. Five divergences,
found by re-anchoring `docs/audits/mapmerge/pcdmap.md` against the code:

- **Ordering.** C tries the direct merge with a neighbour first and reaches the
  swap only when it fails (`pcdmap.c:1446-1524`). `pcd_map_merge` tried the swap
  first and fell through to `default_map_merge`.
- **Parallel.** C's whole neighbour search is inside `if( series )`, and
  `pcdmap.c:1729-1731` says a PcdMap combines in parallel with nothing. The port
  reached `default_map_merge` with `series == false`.
- **The step walk.** C counts the Mappings between the PcdMap and its nearest
  merge target with class tests only, stepping over any PermMap
  (`pcdmap.c:1546-1564`). The port used `CanSwap`'s stricter test, so a PermMap
  that is not an axis swap hid a target behind it instead of ending the walk.
- **The nested merge.** C follows a target-directed swap with `astMapMerge` over
  the sub-list that begins at the new adjacency (`pcdmap.c:1628-1647`), so the
  PcdMap that has just moved next to its target merges in the same pass. The
  port made the swap and returned, leaving the merge to a later iteration of the
  nominate loop — which reaches it only if the loop nominates that PcdMap again.
- **The speculative swap.** `pcdmap.c:1654-1726` was unported. C copies the pair,
  swaps the copies, simplifies both, and keeps the swap only if a simplified
  class differs from the swapped class. `MatrixMap` and `WinMap` both have this
  branch (`matrix_merge.rs:912-947`, `winmap_merge.rs:766-807`); PcdMap did not.

`pcd_swap_strategy` (`simplify.rs:2761-2819`) is now C's body in C's order, with
`pcd_steps_to_merge_target` (`:2830-2857`) as C's class-only walk,
`pcd_swap_pair` (`:2931-2987`) as `PcdZoom` and `PcdPerm` on a copied pair, and
`pcd_speculative_swap` (`:2863-2900`) as C's two-attempt speculation. The
`pcd_swap_at` / `do_pcd_swap_first` / `do_pcd_swap_second` trio it replaces is
retired.

One arithmetic divergence surfaced with it and was mirrored at the time:
C read the neighbour ZoomMap's stored `Zoom` and cleared the replacement's Invert
flag, so an inverted neighbour came out of the swap scaling by its factor rather
than by the reciprocal. Upstream fixed that in `3a4a2a7b` at AST 9.5.0, and the
mirror was inverted with the sync — `pcd_swap_pair` now takes the reciprocal when
the neighbour's list flag is set, gated by
`pcdmap_swap_past_an_inverted_zoommap_preserves_the_transformation` in
`crates/ast-core/tests/test_pcdmap.rs` and by the `pcd_inverted_zoom_swap`
simplify fixture. `docs/issues/c-library-quirks.md` carries the history.

---

### 10.28 LutMap's linearity test and merge tail diverged from C's — FIXED

Re-anchoring `docs/audits/mapmerge/lutmap.md` against the code found four
divergences in a class whose checklist had reported every row ported.

- **Nearest-neighbour tables.** `GetLinear` refuses a nearest-neighbour LutMap
  before it looks at a single value, because the transformation is a staircase
  and the discontinuities sit at the ends of the table (`lutmap.c:578`).
  `table_is_linear` had no such test, so an exactly linear table with
  `LutInterp` set to nearest was replaced by a WinMap — a Mapping that returns
  `BAD_VALUE` outside the table became one that extrapolates.
  `lutmap_nearest_linear_does_not_simplify` is the negative control.
- **The reported index.** C returns `where`, the index of the nominated LutMap,
  even when the pair it replaced began at `where - 1` (`lutmap.c:1234`). The port
  returned `where_.min(ilo)`, which rewinds the nominate loop
  (`cmpmap.c:3625`) to an element C leaves alone and freezes a different entry
  under a repeating pattern (`cmpmap.c:3614`).
- **The merge tail.** C's LutMap `MapMerge` has two branches and no third:
  nothing else in the library merges with a LutMap, and an adjacent UnitMap is
  removed when the UnitMap is itself nominated (`unitmap.c:519`). The port fell
  through to `default_map_merge`, which offers the pair to the neighbour.
- **The second cancellation rule.** `LutMap::try_merge` implemented the same
  equal-but-opposite cancellation as the inline branch, by comparing fields
  rather than by folding both applied directions in, and was reachable only
  through the `default_map_merge` tail above. It is retired; the four tests that
  called it now drive the branch through `simplify`.

`table_is_linear` (`lutmap.rs:167-201`) and `map_merge`
(`lutmap.rs:692-749`) are now C's two branches in C's order, with C's return
value.

---

### 10.29 PolyMap's merge half, and the third `astDoNotSimplify` site — FIXED

Re-anchoring `docs/audits/mapmerge/polymap.md` against the code found three
divergences, one of them in the shared driver rather than in PolyMap.

- **The `MapMerge` dispatcher had no guard.** `astDoNotSimplify` has a third
  caller besides the two named in §10.21: C's public `astMapMerge_` returns
  without reaching the class's `MapMerge` slot when the nominated Mapping's
  `Ident` is set (`mapping.c:24257`). Because the replace-with-simpler halves
  live *inside* the per-class method, that guard also stops a named Mapping
  self-reducing while nominated inside a list — not just merging with a
  neighbour. The port called `map_merge` directly from four sites, so an
  `Ident`-set PolyMap nominated in a CmpMap consolidated its coefficients and
  lost the attribute the flag exists to preserve. `dispatch_map_merge`
  (`mappings/simplify.rs:1207`) is now the only way in, and
  `ident_set_polymap_nominated_in_a_list_is_left_alone` is the control. C's
  `AllowSimplify` save-and-propagate block (`mapping.c:24263-24385`) is
  deliberately not part of it: the port decides restricted-simplify eligibility
  live in `MergeCtx::may_nominate`.
- **The linear half's guard had a term C does not have.** C asks only whether
  the forward coefficients exist and the coordinate counts match
  (`polymap.c:3785`), and fills a zero-initialised matrix and offset vector from
  whatever terms remain. The port additionally required the consolidated term
  list to be non-empty, so a PolyMap whose coefficients all cancel stayed an
  empty PolyMap where C produces a diagonal MatrixMap of zeroes.
  `polymap_whose_terms_all_cancel_becomes_a_matrixmap` is the control.
- **The cancellation test was hand-rolled.** C forces both PolyMaps' `Invert`
  flags to the direction the list applies to the nominated one and calls
  `astEqual` (`polymap.c:3965-3967`). `PolyMap::try_merge` compared the
  coordinate counts and the forward terms itself, so it cancelled a pair whose
  inverse transformations have different coefficient counts — a pair C leaves
  alone, because its `Equal` compares `ncoeff_i` and `mxpow_i`
  (`polymap.c:776-794`). It now mirrors C's forcing and calls `equal`.
  `polymap_does_not_cancel_against_a_different_inverse` is the control, with a
  non-linear forward transformation so the linear half above does not pre-empt
  the comparison.

Closing the third one moved `PolyMap`'s `Equal` onto C's comparison exactly,
which meant adopting a C bug: the block that should compare the inverse
coefficients and powers compared the *forward* ones a second time, so two
PolyMaps whose inverse coefficient values differ compared equal and cancelled.
Upstream `ec86cfaf` fixed it at AST 9.5.0 and the mirror was inverted at that
sync; `polymap_equal_compares_the_inverse_coefficients` is now the gate, and
`docs/issues/c-library-quirks.md` carries the history.

---

### 10.30 ChebyMap's cancellation test was hand-rolled too — FIXED

`chebymap.c` sets no `MapMerge` slot, so a ChebyMap's is PolyMap's whole body,
and the neighbour half there calls `astEqual` (`polymap.c:3965-3967`) — which for
a ChebyMap reaches `chebymap.c::Equal`: the comparison inherited from PolyMap
plus the two bounding boxes (`chebymap.c:407-515`).

`ChebyMap::try_merge` compared the two bounding boxes and the forward terms
itself and never looked at the inverse ones, so it cancelled a pair whose inverse
transformations have different coefficient counts and maximum powers — the same
divergence §10.29 closed in PolyMap, reached by a different route. The oracle
leaves such a pair as a CmpMap of two ChebyMaps; the port returned a UnitMap.
`try_merge` now forces both stored `Invert` flags to the direction the list
applies to the nominated Mapping, as C does, and calls `equal`.

`chebymap_does_not_cancel_when_the_inverse_shapes_differ` is the control, with a
quadratic forward transformation so C's linear half does not pre-empt the
comparison. Routing through `equal` also inherits whatever the PolyMap comparison does with
the inverse coefficient *values*, which upstream `ec86cfaf` fixed at AST 9.5.0;
`chebymap_does_not_cancel_when_only_inverse_coefficient_values_differ` is the
gate.

---

### 10.31 WcsMap's swap driver was two attempts, not C's search — FIXED

`wcsmap.c:3171-3348` is a search, not a pair of attempts. It asks `CanSwap`
about each neighbour in turn, walks a run of PermMaps in both directions
counting how many Mappings separate the WcsMap from one it could cancel against
once swapped, moves it towards the nearer of the two, and — when neither
direction reaches a target — swaps copies of the nearer pair and keeps the
result only if simplifying either half changes its class.

The port asked one fused primitive about the higher neighbour and then the lower
one, taking whichever answered first, and gated each on a narrower "is this
worthwhile" test of its own: the swapped pair is simpler, or the Mapping
immediately beyond the neighbour is a WcsMap. Three narrowings, all closed by
porting C's driver into `wcs_map_merge`:

- **Neighbour preference.** C prefers the lower neighbour when the swap with it
  is intrinsically simpler, or when its target is nearer (`wcsmap.c:3259-3272`).
  The port always tried the higher one first.
- **Search depth.** C's `nstep1` / `nstep2` walks run the length of a PermMap
  run (`wcsmap.c:3196-3222`, `:3226-3255`). The port looked one entry beyond the
  neighbour.
- **The speculative swap.** C's `else if( swaphi || swaplo )` branch
  (`wcsmap.c:3283-3342`) has no analogue in the port, which simply declined.

C's three functions are now three: `can_merge_wcsmaps` for `CanMerge`
(`wcsmap.c:820-928`), `can_swap_wcsmap_past_permmap` for `CanSwap`
(`wcsmap.c:930-1153`), returning C's `*simpler` flag and its `*newwcsmap`, and
`wcs_perm_at` for `WcsPerm` (`wcsmap.c:4471-4706`). The fused
`try_swap_wcsmap_past_permmap_at` is retired.

`neg_wcsmap_perm_allconst` and `neg_wcsmap_perm_both_sides` are the controls.
The first is a WcsMap ahead of a PermMap whose axes are all constants — a swap
C can perform, declines because it is not simpler, and does not rescue
speculatively because the swapped halves keep their classes. The second puts a
bidirectional axis-swapping PermMap on *both* sides, so both neighbours are
swappable and neither is preferred. Both pin a decision the driver makes and no
other fixture reaches: loosening the target choice to "swap with whichever
neighbour allows it" fails exactly these two and nothing else in the corpus.

The step walks stay latent at depths above one, and so does the speculative
branch: two adjacent PermMaps always compose, so a stable list never holds a run
for the walk to step through, and the pair a swap leaves behind is a PermMap the
nominate loop has already canonicalised beside a WcsMap or a UnitMap, none of
which reduce further. Exposing either needs a Mapping the WcsMap can swap past
that is not a PermMap — which C's `CanSwap` does not admit — so the fixtures
that would pin them cannot be built against this pin.

---

### 10.22 The public entry performed two tree rewrites C's `astSimplify_` does not — FIXED

`mapping.c:24755-24808` adds no algorithm: an `IsSimple` short-circuit, the
`Simplify` call, the stamp, and the two flag clears of §10.20. The port's public
simplify entry also rewrote the result tree. Both rewrites are gone.

#### `normalize_unitmap_children`

It dropped `InvA` / `InvB` on a parallel CmpMap whose corresponding child was a
UnitMap, and was measured load-bearing (§10.15): without it four `equal()`
results and two dumps differed from C. Its job had a C owner all along.
`UnitMap::map_merge` ported only the lone and series branches of
`unitmap.c` `MapMerge`; the parallel branch (`unitmap.c:539-624`) was left to
`UnitMap::try_merge` for the run and to nothing for the flag, and the rewrite
reproduced the flag clearing after the fact, at the public entry and in one
no-change exit of `cmp_simplify`. §10.6 had credited the generic rebuild with
that clearing; it was the rewrite.

The parallel branch is now ported in `UnitMap::map_merge`, and the rewrite, both
call sites and `UnitMap::try_merge` are deleted: the corpus, all `wcsconv` byte
comparisons and the rest of the suite pass without them. Three negative controls
fire: not clearing the flag, clearing the UnitMap's own `Invert` with it, and
not merging the run. The lone branch was made faithful in the same change (it
tested the UnitMap's own `Invert` as well as the list's flag, and replaced the
Mapping); no fixture distinguishes the two, because a lone top-level UnitMap
always has both flags equal.

#### `narrow_unit_scale_winmap_children`

The public simplify entry also performed a tree rewrite that converted an
all-unit-scale WinMap child to a ShiftMap at the outermost call, standing in for
C's final `winmap.c:1819` step. It was measured dead at pin `07fbe2dc` and
deleted along with `PublicSimplifyGuard` and the method itself: 0 failing
suites, the 453-fixture corpus at `0 equal() / 0 transform / 9 serialization /
1 other`, and all 194 `wcsconv` byte comparisons, identical before and after.
The narrowing happens where C does it, in `win_map_merge`'s closing pass reached
through `flatten`'s `defer_winmap`;
`unit_scale_winmap_in_a_parallel_branch_narrows_to_a_shiftmap` (`simplify.rs`)
holds that, using a PermMap sibling the WinMap cannot merge with so the
narrowing cannot be a side effect of a series merge.

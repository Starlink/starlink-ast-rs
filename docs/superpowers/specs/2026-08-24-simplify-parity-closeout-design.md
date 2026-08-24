# Closing the simplify parity gap

**Status:** proposed, 2026-08-24, against submodule pin `07fbe2dc`.
**Sits under** [`2026-04-20-rust-port-design.md`](2026-04-20-rust-port-design.md).
**Follows** [`2026-08-21-simplify-mapmerge-audit-design.md`](2026-08-21-simplify-mapmerge-audit-design.md),
which built the record, and
[`2026-08-23-simplify-seam-realignment.md`](2026-08-23-simplify-seam-realignment.md),
which fixed the seam the record exposed.
**Supersedes nothing.**

## Why this spec exists now

The audit is complete and enforced.
Every class implementing `MapMerge` or `Simplify` in C has a checklist under
`docs/audits/mapmerge/`, 37 files and 1025 rows, each row carrying one of four
verdicts at C's own comment-delimited granularity.
`test_mapmerge_audit.rs` fails if a class in the audit universe has no checklist,
if a row has no verdict, if a `gap:` verdict names a PLAN.md section that does not
exist, or if a checklist names no class.

So the outstanding work is now a finite, machine-checked list: **44 `gap:` rows
across 13 checklists, citing 10 PLAN.md sections**, plus two engine divergences
found while writing this spec that no checklist covers because C keeps them in
`mapping.c` rather than in a per-class body.

That is what makes a spec possible.
Before the audit, "what is left in the simplifier" could only be answered by
listing symptoms — fixtures whose `IsSimp` count differed, licence rows, harness
opt-outs.
Now it can be answered by naming C algorithms that are not ported, which is a
different kind of list: it is closed, it is ordered by dependency, and each entry
has a falsifiable exit condition.

## What is left

| # | item | PLAN | rows | reachable today |
|---|---|---|---:|---|
| A1 | Two extra simplifications in the parallel-CmpMaps-in-series branch | §10.3 | 3 | yes — `cap_specmap_02` |
| A2 | Recursion cap of ten, and a structural hash where C uses `astEqual` | §10.19 | 2 | no |
| A3 | Restricted simplify drops two of C's steps | §10.20 | 2 | no — needs a `ReSimp`/`AlSimp` fixture |
| A4 | The CmpMap/PermMap swap fires on the wrong nominee | §10.4 | 8 | yes, structurally |
| A5 | `astDoNotSimplify` unported; no Mapping-level `Ident` | §10.21 | 0 | no — inexpressible |
| A6 | The public entry rewrites the result tree twice; C's does not | §10.22 | 0 | measured: one call is dead |
| A7 | The port always returns a new object where C returns `astClone(this)` | §10.17 | 3 | dump-visible only |
| B1 | ChebyMap does not inherit PolyMap's coefficient consolidation | §10.2 | 6 | yes |
| B2 | Box's PermMap simplification branch | §10.13 | 9 | yes |
| B3 | Polygon's two vertex-transform guards, and `SimpVertices` | §10.18 | 4 | partly — one guard needs a new attribute |
| B4 | NormMap does not simplify its encapsulated Frame | §10.17 | 1 | no — no Frame-level simplify seam exists |
| B5 | `GetDefUnc`'s unbounded-axis guard misses the upper bound | §10.12 | 1 | no — latent, but wrong on its own terms |
| C1 | The clear-on-set convention, 11 classes | §9.2 | 5 | no — most attributes have no setter |

Three bands, and the band is what decides the order of work, not the size of the
item.
**Band A** is the driver: `mappings/simplify.rs` and `mappings/simplify_entry.rs`,
where a divergence changes what fires for every class at once.
**Band B** is per-class `MapMerge` / `Simplify` bodies, where a divergence is
confined to one class and one fixture.
**Band C** is a cross-class convention that is latent everywhere.

## Ordering

**Wave 1 — measurable today, no new surface required.**
A6 (delete the dead call), B5 (one unit test and a one-line guard fix), A1.
A1 is the one that matters: it is the only remaining cause of a licensed
`IsSimp` disagreement, and it is the largest single contributor to the
over-nomination measured in §10.3.

**Wave 2 — needs new C-oracle fixtures, but no new port surface.**
A2, A4, B1, B2.
Each is a self-contained port of a C body or guard whose gate is a fixture that
`gen_simplify_fixtures.c` can generate today.

**Wave 3 — needs new expressive surface before a gate can exist at all.**
A3 (a native fixture carrying `ReSimp` and `AlSimp`), A5 (Mapping-level `Ident`),
B3's bent-edge guard (`SimpVertices` attribute plus the `SimpVT` card), A7 (a
decision about what "a result is new" means in an owned-value port).
These items are not harder to write than wave 2; they are harder to *falsify*,
and shipping an unfalsifiable change is what §6's licence rows exist to prevent.

**Wave 4 — architecture.**
B4 alone.
It needs a Frame-level simplify seam that does not exist, which is a public-trait
question and therefore an amendment to the design spec rather than a local port.

## Band A — the driver

### A1 — port C's identity test, and delete both substitutes (§10.3)

C's parallel-CmpMaps-in-series branch simplifies in exactly one place, the pair it
has just combined (`cmpmap.c:1728`), and decides whether the restructure was worth
anything with a two-term identity test (`cmpmap.c:1730-1734`):

```c
if( tmap != tmap2 || astGetInvert( tmap ) != astGetInvert( tmap2 ) ) simpler = 1;
```

The port keeps two things in place of that.
`dimension_match_parallel_lists` simplifies every member of both aligned sublists
(`simplify.rs:2313`, `:2323`) where C passes them to `CombineMaps( ..., 0, ... )`
as they stand.
And `compute_merge_parallel_pair_in_series` replaces the identity test with a
three-branch structural test ending in `flat.len() < pre_len`
(`simplify.rs:2226-2231`), a leaf-count threshold with no analogue anywhere in
`cmpmap.c` — stop signal 1 — which decides whether the whole restructure fires.
The comment at `simplify.rs:2223` claims it mirrors C; it does not.

**Design.** C's first term asks "did `astSimplify` return a different object",
which in an owned-value port is not a pointer comparison but a fact the engine
already knows: `run_nominate_loop` returns whether anything changed, and
`simplify_mapping` discards it.
Surface it.
Add an internal `simplify_mapping_reporting_change(&dyn Mapping) -> AstResult<(DynMapping, bool)>`
and make `simplify_mapping` a wrapper that drops the flag, then set `simpler` from
that flag or from `simplified.is_inverted()` — C's second term, since the
pre-simplify `series_box` is forward by construction.
With `simpler` faithful, the per-member `.simplify()` calls have nothing left to
compensate for and both come out in the same change.

**Why together.** Measured separately they trade one gap for another: removing the
per-member call alone moves `cap_specmap_02` from 10 cards to 7 against C's 8, and
costs `cap_cmpmap_01`, `cap_cmpmap_04` and `cmpmap_parallel_series_dim_mismatch`
the cards they currently get from the extra pass (§10.3's table).
Those cards must come back from the identity test firing where C's fires, not from
a compensating stamp — that distinction is the whole content of this item, and
seven measured-and-reverted attempts at `cap_cmpmap_03` were on the wrong side of
it.

**Gate.** `cap_specmap_02` reaches C's 8 cards and its
`ISSIMP_PLACEMENT_LICENSED` row is deleted, taking the licence to zero; the three
fixtures above stay byte-exact; `test_simplify_regression`,
`test_native_roundtrip_all`, `test_wcsconv` and `test_simplify_seams` stay green.
Re-measure the nomination counts with `build_simplify_trace.sh` and the `tracing`
feature, and record the new totals in §10.3 — the prediction is that the port's
153 nominations fall toward C's 87, and this is the change that should show it.

**Record.** Re-verdict `cmpmap.md` rows 22, 23 and 26, and rewrite the
"Rows 22, 23 and 26" prose section.
Note that C's own ordering here was wrong until upstream `4f2fc0ed` (the annul
preceded the test, so `simpler` was always true); the port must match the fixed
order, and every conclusion drawn about this branch before pin `8d4f526f` was
drawn against a buggy oracle.

### A2 — port `simplify_stackmaps` as C keeps it (§10.19)

`MAX_SIMPLIFY_DEPTH = 10` (`simplify.rs:131`) makes the engine decline to simplify
at all once ten nested `simplify_mapping` calls are on the stack.
C has no depth limit: `cmpmap.c:3471-3499` keeps `simplify_stackmaps` purely to
detect a Mapping *already being simplified*, comparing with `astEqual` and growing
the stack without bound.
Beside the cap, `mapping_fingerprint` (`simplify.rs:93`) stands in for that
`astEqual` comparison with a structural hash, so a collision declines a
simplification C performs, where an equality test cannot.

**Design.** Replace both with C's shape: a stack of the Mappings currently being
simplified, compared with `Mapping::equal`, unbounded.
The port has `equal` on every class and `frames_equal` for the Frame side, so
nothing new is required.
Recursion termination then rests where C rests it — on the equality hit — and the
guard stops being a policy.

**Gate.** A fixture C collapses through more than ten levels: eleven nested series
CmpMaps each holding a `ZoomMap`, which C reduces to one `ZoomMap`.
Today the port returns the tree as supplied, and with a `panic!` in place of the
refusal the whole tier-1/2 suite passes, so nothing in the corpus reaches it.
A second fixture should pair two structurally distinct Mappings that the
fingerprint hashes alike, if one can be constructed cheaply; if it cannot, say so
in the checklist rather than implying the collision path is tested.

**Record.** `cmpmap.md` rows 44 and 45, and its "Rows 44 and 45" prose section.

### A3 — treat a restricted simplify as C's distinct mode (§10.20)

Two steps have no counterpart in the port.
C discards the `simpler` flag `astMapList` set when it met an inverted CmpMap
during decomposition, because under a restricted simplify it cannot yet know
whether that CmpMap is eligible (`cmpmap.c:3526-3530`); the port computes the same
flag in `decomposition_is_simpler` (`simplify.rs:3598`) and never consults
`restricted`, so where C returns `astClone( this )` the port rebuilds the tree and
the result right-associates.
And C clears `AllowSimplify` on every component as it folds the result
(`cmpmap.c:3689-3692`) and on the result itself (`mapping.c:24786-24788`),
explicitly so a protected flag cannot appear in a public dump; the port clears it
in neither place, so an `AlSimp` card survives a simplify and is re-emitted by
`serial/mod.rs:83`.

**Design.** Consult `restricted` at `simplify.rs:3598` and clear `AllowSimplify`
at both of C's sites.
Both are small; the work is the fixture.

**Gate.** One native fixture, hand-built or generated, whose CmpMap carries
`ReSimp 1` with components carrying `AlSimp 1`, one of them an inverted CmpMap.
It must assert three things: the tree is returned unrestructured, no `AlSimp` card
survives, and no `IsSimp` card is added (a restricted simplify does not stamp).
That fixture is the only route into the mode today — C's producers are all in
`yamlchan.c` (`:10998`, `:9505`, `:9627`, `:10220`), which is not ported, and the
port's only producers are the two deserialisation sites (`cmpmap.rs:1034`,
`serial/mod.rs:160`).

**Record.** `cmpmap.md` rows 50 and 70, and its "Rows 50 and 70" prose section.

### A4 — hang the CmpMap/PermMap swap off C's nominee (§10.4)

`cmpmap.c:1816-2119` runs when the **CmpMap** is nominated and the PermMap is its
lower neighbour; `permmap.c` never mentions `CmpMap`, so a nominated PermMap does
not attempt it in C.
The port has it the other way round: `CmpMap::map_merge` has no swap branch, and
the swap lives in `perm_map_merge` (`simplify.rs:1130`), firing when a **PermMap**
is nominated and looking at `(where_, where_ + 1)`.
The arithmetic is C's; the trigger is the opposite element.

**Design.** Move the branch into `cmp_map_merge`, looking *behind* the nominee, and
delete the `perm_map_merge` trigger.
This is H2 in the hazards list — a structurally different route to the same
Mapping — so the observable is not `astEqual` but the index reported, where the
loop resumes, and the `IsSimp` cards that fall out of both.

**Gate.** A series list `[A, PermMap, parallel CmpMap, B]` where `A` and `B` can
each merge with what the swap brings next to them, so the resume point decides
which merge happens first, replayed through the oracle and compared on cards and
tree shape.
A nominated CmpMap whose PermMap neighbour the cursor has already passed is
reached in C and not in the port today; that case belongs in the same fixture.

**Record.** `cmpmap.md` rows 31 to 40.

### A5 — port the `DoNotSimplify` seam, and give Mappings an `Ident` (§10.21)

C gates simplification on a per-class protected method with two generic callers:
`astSimplify_` refuses to reach the `Simplify` slot when it returns non-zero
(`mapping.c:24764`), and `astMapList` refuses to decompose a CmpMap
(`cmpmap.c:1154`).
The base rule is "`Ident` is set" (`mapping.c:1107-1152`); `Frame` overrides it to
zero always, so a named Frame is still simplified (`frame.c:3511-3554`).
By the rule in CLAUDE.md this is architecture, not ceremony — two generic drivers
dispatch through it — and the port has neither the seam nor the attribute.

**Design.** Private `Ident` bookkeeping on Mappings first, which the design spec
already anticipates as a private `Object` super-trait concern
(`2026-04-20-rust-port-design.md:390-396`), then a `do_not_simplify()` trait method
consulted at the `simplify_entry` gate and in `flatten`.
`Frame`-implementing types override it to `false`.
Note the ordering: without the attribute the seam cannot be tested, and a seam
that cannot be tested is what §10.17's mis-verdict was made of.

**Gate.** A native fixture holding a series CmpMap of two ZoomMaps where the inner
CmpMap carries `Ident`: C leaves it whole, the port collapses it.
A companion with `Ident` on a Frame inside a FrameSet proves the override still
simplifies.

**Record.** New; §10.21 was written for this spec.
`Ident` also touches the serialisation reader (`serial/mod.rs:774` currently skips
the card), so the round-trip corpus is part of the gate.

### A6 — delete the entry-point rewrite nothing demands (§10.22)

`astSimplify_` adds no algorithm.
`<dyn Mapping>::simplify` (`simplify_entry.rs:36-102`) rewrites the result tree
twice: `normalize_unitmap_children`, which §10.15 measured as load-bearing, and
`narrow_unit_scale_winmap_children` at `:90`, gated on
`PublicSimplifyGuard::is_outermost()`.

Measured at pin `07fbe2dc` with the second call disabled, the whole `ast-core`
suite is green: 98 test binaries, the 453-fixture simplify corpus, all 194
`wcsconv` byte comparisons, the native round-trip.
That is consistent with the narrowing having moved into `win_map_merge`'s final
pass through `flatten`'s `defer_winmap`, which is where C keeps it
(`winmap.c:1820-1822`).

**Design.** Delete the call, the `is_outermost()` gate it is the only user of, and
`PublicSimplifyGuard` if nothing else needs it.
A layer nothing demands is worse than a missing one, because a correct change made
near it looks like a regression — §10.15 is the evidence.
If a fixture turns up that demands it, the alternative is not to keep it silently
but to name that fixture in its doc comment, as §10.15 now requires of every
surviving dump-shaping layer.

**Gate.** The suite above, plus one deliberate check that the WinMap→ShiftMap
narrowing still happens for a WinMap isolated in a parallel branch — the case the
call was written for.

### A7 — decide what "the result is new" means (§10.17)

C returns `astClone( this )` when nothing reduced, and `astSimplify_` then stamps
*that* pointer — which is the input object, so no new object appears and the
caller's Mapping acquires `IsSimple`.
`FrameSet::simplified` (`frameset.rs:1598-1607`) returns its working copy
unconditionally, dropping C's rows 10, 12 and 14, and `simplify()` stamps whatever
comes back.
NormMap will need the same decision the moment B4 gives it something to compare.

**Design.** Give the per-class seam a way to say "nothing changed" — the natural
shape in an owned-value port is `Option<DynMapping>`, `None` meaning C's
`astClone( this )` — and let `simplify_entry` stamp `self`'s clone in that case.
This is a trait-signature change on `Mapping::simplified`, so it is the one Band A
item that needs the design spec amended alongside it.
It is also what makes A1's `simpler` flag and B4's Frame seam expressible in the
same vocabulary, which is the argument for doing it before wave 3 rather than
inside it.

**Gate.** A FrameSet whose edges are all already simple, simplified and dumped:
C's output carries `IsSimp` on the original object and the port's must too, and
the dump must not gain a second FrameSet.
Aliasing artifacts — the fact that C mutates the caller's object — stay out of the
port and go in `docs/issues/c-library-quirks.md` if they surface in a fixture.

## Band B — per-class bodies

### B1 — ChebyMap's consolidation half only (§10.2)

`chebymap.c` defines no `MapMerge`, so a ChebyMap's is PolyMap's whole body
(`polymap.c:3597-3961`).
The port's `ChebyMap` takes the trait default and reaches none of it.

Port the coefficient consolidation (`polymap.c:3624-3745`), which is
basis-independent: two `T_2` terms for one output add exactly as two `x^2` terms
do.
Do **not** port the linear replacement (`polymap.c:3747-3875`).
It is a C defect, settled on the oracle and recorded in
`docs/issues/c-library-quirks.md`: C reads the coefficients with nothing that
distinguishes a Chebyshev basis from a power basis, so a one-input ChebyMap over
`[0,10]` holding `2 * T_1` simplifies to `ZoomMap(2)`, which is a different
function.
The port leaves a linear ChebyMap alone whatever its bounds.

**Design note.** The work is a way into PolyMap's body that skips the linear
branch, not a copy of it — a parameter or a split function, decided when the code
is in front of you, but *not* a ChebyMap test inside the shared body, which would
put a class check where C has none.

**Gate.** A ChebyMap with two forward terms using the same input powers for one
output, nominated inside a list, whose `.simp` C writes with the terms combined.
The existing `cheby_linear_lone_reduce` pair stays out of the port's corpus,
because a positive fixture there would assert the port reproduces the wrong
Mapping.

**Record.** `chebymap.md` rows 4 to 8 and 12.

### B2 — Box's PermMap branch (§10.13)

`box.c:3719-3882` handles a Box whose base-to-current Mapping is a PermMap by
reading the permutation arrays and constants directly and building the
current-Frame Region axis by axis: a constant-fed output gets equal limits, a
`AST__BAD`-fed output is unbounded so the result is an Interval, a missing bound
likewise, and absent base axes make the PermMap's slice a candidate NullRegion.
`box_region.rs` contains no reference to `PermMap`; it goes straight to the
generic best-fitting-Box path (`box_region.rs:1539-1566`).

A plain axis permutation already works, because it is exact and the mesh
comparison succeeds.
What the generic path cannot produce is a constant-valued axis, an unbounded axis,
or a result that is an `Interval` or `NullRegion` rather than a `Box` — so this is
a class-of-result gap, not a precision gap.

**Gate.** A 3-D Box sliced to 2-D by a PermMap with a constant third output,
simplified and serialised; C writes an Interval or equal limits, the port writes
the mesh fit or declines.
No corpus fixture builds a Box over a PermMap with constants today.

**Record.** `region_box_region.md` rows 16 to 24.

### B3 — Polygon's two guards, and `SimpVertices` (§10.18)

`polygon.c:5379-5437` transforms the vertices and guards the result twice.
It clears `ok` if any transformed axis-1 value is `AST__BAD` and builds the new
Polygon only when `ok` (`:5400-5407`; axis 2 is not tested, a C asymmetry rather
than a rule).
And when `SimpVertices` is zero it meshes the *original* Polygon and requires
`astRegPins` to place every mesh point on the new Polygon's boundary
(`:5412-5428`) — the whole purpose of the attribute, since a non-linear Mapping
can carry vertices to the right places while bowing the edges between them.
`Polygon::simplified` (`polygon.rs:1095-1149`) does neither, and builds the new
Polygon with the uncertainty as stored, in the base Frame, where C passes
`astGetUncFrm( new, AST__CURRENT )`.

Split this in two.
The `ok` guard and the uncertainty Frame are portable now and each has a fixture:
one vertex mapping to `AST__BAD`, and a Polygon with an explicit uncertainty over
a non-UnitMap base-to-current Mapping, compared on the `Unc` value written under
the simplified Polygon.
The bend check needs `SimpVertices` to exist first — attribute, accessor, and the
`SimpVT` card C writes at `polygon.c:6692` — because on its default of 1 the check
is skipped, so the missing guard is invisible until something sets it to zero,
which the port cannot currently express.

**Gate.** For the bend check: a Polygon whose base-to-current Mapping is
non-linear enough to bow the edges (a `WcsMap`, or a `PolyMap` with a quadratic
term), `SimpVertices` cleared, and an uncertainty large enough for `astRegPins` to
be the deciding test.
C keeps the original Polygon; the port returns the transformed one.

**Record.** `region_polygon.md` rows 7, 11, 12 and 13.

### B4 — NormMap's Frame simplification, and the seam it needs (§10.17)

`normmap.c:621-634` is the *first* thing C's `MapMerge` does: `astSimplify` the
encapsulated Frame, and if it comes back different, replace the NormMap with one
wrapping the simplified Frame and report a change.
Only then does C reach the basic-Frame test that turns a NormMap into a UnitMap
(`:635-640`), which is all `NormMap::map_merge` (`normmap.rs:188-205`) does.

The branch is reachable in C because a Frame *can* simplify to a different Frame:
`CmpFrame` overrides `Simplify` to simplify both components and rebuild
(`cmpframe.c:9019-9034`), and every Region class overrides it too.
The port cannot express that.
`NormMap` holds a `DynFrame` (`normmap.rs:42`), `dyn Frame` has no simplify
method, and `FrameSet` is the only type under `crates/ast-core/src/frames/` that
implements `Mapping` (`frameset.rs:1561`) — so `CmpFrame` has no `simplified` to
override.
Closing this means adding the seam, not just the branch.

**Design.** A Frame-level simplify entry that mirrors C's arithmetic: in C every
Frame is a Mapping and inherits the `Simplify` slot, so the port needs a
`dyn Frame` method — default returning "unchanged", overridden by `CmpFrame`,
`FrameSet` and the Region family, which already have the bodies.
Whether that is a method on `trait Frame`, or `trait Frame: …` gaining a private
super-trait, is a public-API question: the design spec fixes `Frame` and `Mapping`
as independent siblings, and this touches that boundary.
**Amend the design spec in the same change**, per its own instruction, and settle
the shape there before writing the NormMap branch.
Take A7's `Option`-shaped "nothing changed" signal as the return type so the two
seams speak the same vocabulary.

**Gate.** The existing `normmap_frame_simplifies` fixture
(`gen_simplify_fixtures.c:2612-2632`) does **not** work: `astBox` resolves its
FrameSet to the current Frame at construction, so the reduction happens before
`astSimplify` is called and both `.map` and `.simp` hold the same Box.
The port passes it today by declining a branch it has not ported, which is how the
mis-verdict survived.
The replacement needs an encapsulated Frame whose reduction is *not* absorbed by
its own constructor — a CmpFrame holding a Region that reduces is the direct
route — compared on the NormMap's `Frame` value, not the card count.

**Record.** `normmap.md` row 5, and `frame_frameset.md` rows 10, 12 and 14 once
A7 settles.

### B5 — the unbounded-axis guard (§10.12)

`region.c:3560-3573` builds the default uncertainty as a Box covering 1.0E-6 of
the base-Frame bounding box and zeroes both bounds on any axis whose bound is
`±DBL_MAX`.
The port tests `!is_bad(ubnd[i]) && !is_bad(lbnd[i])` (`regions/mod.rs:466`), and
`is_bad` is `v == BAD_VALUE || !v.is_finite()`.
That catches an unbounded lower bound, which is `-f64::MAX` and therefore
`BAD_VALUE`; it does not catch an unbounded *upper* bound, which is finite and not
`BAD_VALUE`, so the axis takes the bounded branch and gets a half-width around
9e301 where C gives zero.
The correct test is against `f64::MAX` in magnitude, as `Interval` already does
(`interval.rs:160-163`).

Latent: the Regions that can be unbounded all override `get_def_unc`, and those
using the base rule have finite bounds.
Fix it anyway with a direct unit test on `default_bounding_box_unc`, because the
guard is wrong on its own terms and the test costs one function call.

**Record.** `region_mod.md` row 33.

## Band C — the clear-on-set convention (§9.2)

Upstream `e40e71b4` added a rule with no counterpart here: a setter whose value
feeds the first part of that class's own `astMapMerge` discards both simplified
records (`mapping.h:449-460`), applied in 11 classes.
Most of those attributes have no setter in the port, so the gap is latent, and the
five checklist rows that cite it are the classes whose checklists reached the
attribute list.

Do this **last, and as one change across all 11 classes**, not per class as the
attributes land.
The reason is the convention itself: a rule applied in six of eleven places is
harder to reason about than one applied in none, and the audit rows would read
`ported` in a way that hides the asymmetry.
Its gate is a fixture that sets one of these attributes on an already-simplified
Mapping and re-simplifies; none exists today, and one is enough to cover the
convention if it is written against a class whose setter exists.

## Fixture capability this depends on

Nine of the thirteen items need a fixture that does not exist.
Two need a port-side attribute before a fixture can even be read (`SimpVertices`
for B3, `Ident` for A5), and one needs a native fixture written by hand because
C's only producer of the flags is unported (A3).

Two rules from `CLAUDE.md` govern all of them, and both have already been paid for
here.
A fixture that cannot discriminate is worse than a missing one — `normmap_frame_simplifies`
passed for months while the branch it targeted was unported.
And a harness that can pass without comparing is not a gate: every new case goes
in a named exclusion list with a per-case reason plus a companion test that fails
when an excluded case starts passing, never a skip.

Generation goes through `ast_tester/gen_simplify_fixtures.c` against the pinned
oracle, and `tools/oracle_fixtures/check_corpus_fresh.sh` afterwards.
The two consumers of the simplify corpus — the manifest-driven test and the
auto-discovering byte-exact round-trip — both run on every fixture landing.

## Risks

**Structural equivalence is not equivalence (H2).**
A1 and A4 both change *which* nomination performs a merge and where the loop
resumes.
The Mapping can be right while the tree, the reported index and the `IsSimp` cards
move, and it surfaces far from the change — in the serialiser or `MapSplit`, not
in `astEqual`.
Every one of these lands with a full-corpus re-measure, not a target-fixture
check.

**Cards move in both directions.**
A1's table already shows three fixtures losing cards when one divergence is
removed alone.
A change that moves the target fixture to C's count while another fixture leaves
it has not closed anything; it has relocated the licence.

**Negative controls.**
Several of these gaps are guarded by fixtures asserting C does *not* simplify
something.
Never loosen one to pass a positive case; two or three broken in a row means the
rule being added is not C's rule.

**Thresholds.**
A1 and A2 both exist to delete a constant with no analogue in the C source.
Neither replacement may introduce another.

**The record is part of the deliverable.**
`test_mapmerge_audit` enforces that every `gap:` verdict names a live PLAN.md
section, so a closure that does not re-verdict its rows in the same commit either
fails the gate or leaves the record claiming work that is done.

## Out of scope

- **`YamlChan`.** It is C's only producer of `RestrictedSimplify` and
  `AllowSimplify`, so porting it would exercise A3 for real, but it is a Channel
  port and belongs to its own step.
- **PolyMap's linear replacement for ChebyMap.** A C defect, reproduced on the
  oracle, recorded for upstream, deliberately not mirrored.
- **C's aliasing artifacts.** Where C mutates the caller's object (A7) or emits a
  card as a side effect of reference counting, the port does not follow; those go
  in `docs/issues/c-library-quirks.md`.
- **A performance gate.** The over-nomination in §10.3 is a fidelity question
  here, not a speed one. `ast-oracle-bench` is still a placeholder, and wiring it
  up is tracked separately.

## What to read first

- `PLAN.md` §10 — the 22 numbered entries this spec orders, each with its "what
  would expose it".
- `docs/audits/mapmerge/README.md` — the checklist grammar and the four verdicts.
- `docs/audits/mapmerge/cmpmap.md` — A1, A2, A3 and A4 all live in this one file's
  rows.
- `docs/porting-hazards.md` — H2 for A1 and A4, H9 for the fixture rules, H10-H12
  for the gates.
- `2026-08-23-simplify-seam-realignment.md` — why the seam is now singular, which
  is what makes Band A tractable at all.

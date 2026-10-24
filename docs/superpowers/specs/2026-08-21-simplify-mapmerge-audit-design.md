# Closing the simplify / `MapMerge` gaps, and the gates that hid them

**Status:** design, approved 2026-08-21.
Supersedes the audit half of
[`docs/issues/simplifier-mapmerge-rebuild-plan.md`](../../issues/simplifier-mapmerge-rebuild-plan.md),
whose header claims the per-class work landed and whose body says Phases 2-5
never started.
That contradiction is the reason this document exists.

## The problem in one paragraph

The Rust simplifier has C's *shape* — a nominate loop and a per-class
`map_merge` seam — but its per-class bodies were never compared step by step
against C's.
Gaps were therefore found in whatever order the fixture corpus happened to
expose them, which felt like whack-a-mole and repeatedly looked like a
regression when it was not.
Two separate measurement faults made that worse: two reference corpora went
stale when the `reference/starlink-ast` pin moved, and the simplify harness
silently stopped comparing any fixture whose result was a Region or a
SwitchMap.
This design fixes the instrument first, then the corpora, then the port, and
replaces the prose audit with one a test can enforce.

## Scope

Three workstreams, all in this design, in this order.

- **W3 — corpus hygiene.**
  Regenerate the references that the pinned C no longer reproduces, and
  distinguish those from references that merely differ in the last bits because
  they were captured on Linux.
  Runs first so that everything downstream is measured against a sound oracle,
  and so the upstream-owned files can go to a Starlink PR early.
- **W1 — the executable audit.**
  A per-class checklist of C's `MapMerge` / `Simplify` steps that a test
  enforces for completeness.
- **W2 — the defects.**
  The eleven ungated byte-comparison failures (six distinct causes) and the
  four remaining `IsSimp` under-tagging cases, fixed as audit outputs rather
  than as fixture-driven patches.

## Non-goals

- Emulating C's incremental simplify-as-you-assemble order in the FitsChan
  reader.
  C's `IsSimp` placement in that path records the order its reader reduced in,
  which is an internal implementation detail; the agreed done-criterion permits
  a documented residue there.
- The other nine architecture seams CLAUDE.md names (`Transform`, `Equal`,
  `MapSplit`, `Match`, `SubFrame`, `Overlay`, `RegPins`, `RegBaseMesh`,
  `RegBaseBox`).
  The audit covers `MapMerge`, `Simplify`, `Region::Simplify` and `GetDefUnc`,
  which is the smallest scope containing every known defect.
- Regenerating the nine `.native` references whose only difference is
  platform arithmetic.
  They are not stale.

## Evidence base

Everything below was measured this session, against the submodule pinned at
`f6762005`.
It is recorded here because several of these facts are cheap to re-derive
wrongly.

### C mechanics that the port must mirror

| fact | C source |
|---|---|
| `astSimplify_` returns `astClone( this )` — the *same pointer* — when the Mapping is already flagged simple, so a caller's `fs != this->frameset` test reads "unchanged" | `mapping.c` ~24801-24805 |
| The `IsSimp` record is per-orientation: `astIsSimpleFlag` selects `AST__ISSIMPLEINV_FLAG` when `invert==1`, else `AST__ISSIMPLE_FLAG` | `mapping.h:441-447` |
| `astClearIsSimple` discards **both** records, and is required of any attribute setter feeding the first part of that class's own `astMapMerge` | `mapping.h:449-460` |
| The loader stamps the flag when it reads an `IsSimp` card, in the orientation the card was written | `mapping.c:24138` |
| `ShiftMap::MapMerge` reverts with `astClone( this )` forward, or `astCopy( this )` plus negated shifts when inverted — both carry `flags` | `shiftmap.c:721-741` |
| The Mapping copy constructor clears only `report`; `flags` survive a copy, so a Mapping with cleared flags was *constructed*, not copied | `mapping.c:23608-23648` |
| `Object::Equal` compares structure size and class name; every subclass chains to its parent | `object.c` |
| `Region::Equal` compares class name, `points`, base Frame, current Frame, the base→current Mapping, `Negated` and `Closed` — and deliberately **not** the uncertainty | `region.c::Equal` |

The last row matters for how the defects below are gated.
Causes C and D differ only inside `Unc`, so C's own `astEqual` returns 1 for
them, and ours agrees.
`equal()` is not defective; the byte comparison is simply the only check that
covers fields `astEqual` is designed to ignore.

### The clear-on-set convention is unported

Upstream added the rule at `mapping.h:449-457` and applies it in eleven
classes.
The port implements it nowhere.

| class | attributes |
|---|---|
| ZoomMap | `Zoom` |
| LutMap | `LutInterp`, `LutEpsilon` |
| MathMap | `Seed`, `SimpFI`, `SimpIF` |
| PolyMap | `IterInverse`, `NiterInverse`, `TolInverse` |
| SplineMap | `InvNiter`, `InvTol`, `OutUnit` |
| SphMap | `PolarLong`, `UnitRadius` |
| WcsMap | `FITSProj`, `LonCheck` |
| PcdMap | `Disco` |
| PermMap | `PermSplit` |
| IntraMap | `IntraFlag` |
| GrismMap | macro form, `grismmap.c:165,255` |

Several of these attributes have no Rust setter yet, so most of the gap is
latent.
It is listed because a latent gap that nothing would expose is exactly what an
audit is for.

### The instrument was broken

`test_simplify_regression.rs` and `test_regions.rs` each hand-rolled a
`&dyn Mapping` → `AnyObject` dispatch.
The harness's covered eighteen Mapping classes and no Region, FrameSet or
SwitchMap, and returned `None` for anything else — from inside an
`if let Some(..)` guarding *both* the byte comparison and the `IsSimp` count
check.
Every fixture whose simplified result was a Region or a SwitchMap therefore
skipped both checks silently.

Fixed in `050fbef`: both now call `AnyObject::from_mapping`, one authority in
`starlink-ast` that is the exact inverse of `AnyObject::as_mapping`, and an
uncovered class is a counted failure in the harness and a panic in the region
test.
Turning the gate on exposed eleven byte failures and seven additional `IsSimp`
disagreements, all pre-existing.

### The corpora

`.simp` — eight of 299 pairs were not reproduced by the pinned C.
Four were upstream's own files, byte-identical to upstream HEAD and not
reproducible by upstream's own library, because upstream's suite compares with
`astEqual` and never byte-for-byte.
Regenerated in `13c6be4` and carried to a Starlink PR.

`.native` — of 107 `native` manifest rows, 75 reproduce byte-exactly and 32 do
not.
**No keyword sequence differs in any of the 107**, so nothing about core WCS
structure changed upstream; every difference is either an `IsSimp` card or a
last-bits value.

| bucket | count | disposition |
|---|---|---|
| `IsSimp` cards only | 17 | genuinely stale; regenerate anywhere; goes upstream |
| last-bits values **and** stale `IsSimp` | 6 | regenerate, keep the card hunks only — see W3.2 |
| last-bits values only, `IsSimp` already correct | 9 | not stale; document, do not touch |
| structural | 0 | — |

The six mixed cases are `scp`, `tnx`, `tnx-cheb`, `ssysobs`,
`skyspec3d-class`, `wave-f2w`.
The nine platform-only cases are `timj`, `multi-wcs`, `skyref`,
`iraf-longpole`, `skyref-alt`, `tidy-offsets`, `tidy-pole`, `offset-sref`,
`azel`.

Two earlier figures in `PLAN.md` §9.3 are wrong and are corrected by this work:
"36 stale" (it conflated platform noise with staleness) and the claim that only
`scp.native` had non-`IsSimp` differences (the sweep behind it failed to pass
the manifest's FitsChan attributes to `wcsconverter`).

### What the ShiftMap fix actually did

`ShiftMap::map_merge` rebuilt a fresh `ShiftMap` on its revert path, discarding
the simplified record where C clones or copies and keeps it.
Fixing it looked like a six-case wcsconv regression.
Measured against the pinned C rather than the committed reference:

| case | port | committed | pinned C |
|---|---|---|---|
| `scp`, `serpens`, `tnx`, `tnx-cheb` | 2 | 1 | **2** |
| `crota-3d` | 5 | 3 | **5** |
| `cd-cross` | 3 | 2 | 4 |

Five of six now match C exactly; `cd-cross` is one card closer.
This is the canonical example of the failure mode this design exists to
prevent: a C-faithful fix scored against a stale yardstick reads as a
regression.

### CI

Run 32511602164 on `dba0eed` fails on both `macos-latest` and `ubuntu-latest`
with the same two `test_regions` cases, `simplify_circle_conformal_keeps_circle`
and `simplify_circle_nonconformal_keeps_circle` — cause B below, and therefore
platform-independent.
`050fbef` turns CI green by dropping the `IsSimp` line from those two
comparisons while `test_simplify_regression.rs` pins the counts, so pushing it
clears the red without pretending the defect is fixed.

## W3 — corpus hygiene

### W3.1 Regenerate the 17 card-only references

Card-only differences are platform-independent, so this machine is a valid
capture host.
Write each regenerated file to both `crates/starlink-ast/tests/fixtures/wcsconv/expected/`
and `reference/starlink-ast/ast_tester/`, in one commit, so the upstream PR is
a clean subset.

Expect most of the 24 rows in `ISSIMP_PLACEMENT_LICENSED` to become deletable:
they are pinned against references that this step replaces.
Re-measure after regenerating and delete every row that no longer diverges
rather than adjusting its numbers.
The four rows added for `scp`, `serpens`, `tnx` and `tnx-cheb` are explicitly
temporary and must not survive W3.

### W3.2 The six mixed cases — take the card hunks, drop the value hunks

Their `IsSimp` staleness is real and their value differences are platform
noise.
The two never share a line, so they are separable: regenerate on this machine,
then keep only the hunks that add or remove `ISSIMP` cards and discard the
hunks that change numeric values.
`git add -p` does this by hand; the plan should script it so it is
reproducible and reviewable.

The result is exactly what a Linux run of the pinned C would have produced,
because the only upstream change affecting these files is the `IsSimp`
placement and the only local change is the last bits.
That reconstruction is sound *only* while both halves of that sentence hold,
so the step must assert it: the merged file may differ from the committed
reference in `ISSIMP` lines and nothing else, and its `ISSIMP` cards must match
the freshly generated output exactly.
A step that cannot satisfy both assertions is not separable and falls back to a
Linux capture.

This removes the need for a Linux capture job, and these six carry no license
row.

### W3.3 Document the nine platform-only cases

Add a manifest comment recording that they are Linux captures whose last bits
differ on this platform, and that this is not staleness.
Several already use the `astequal` compare mode for exactly this reason; the
note should say which and why.

### W3.4 A freshness guard

`tools/oracle_fixtures/check_corpus_fresh.sh` builds the pinned C, replays
every `native` manifest row with its attributes, and classifies each
difference as card-only, numeric, or structural.
Card-only or structural drift is an error; numeric drift is reported and
tolerated.
This is the check that would have caught both corpora going stale when the pin
moved, and it belongs in the upstream-sync protocol in `CLAUDE.md`.

## W1 — the executable audit

### Form

One file per class: `docs/audits/mapmerge/<class>.md`.
Each row is one of C's own comment-delimited steps in that class's `MapMerge`
or `Simplify` body.

| column | meaning |
|---|---|
| step | C's own wording for the step |
| C site | `file.c:line` range |
| Rust site | `path.rs:line`, or `—` |
| verdict | `ported`, `n/a` + reason, or `gap` + `PLAN.md` row |

AST's C is densely commented with named steps, so this is 5-20 rows per class,
not a line-by-line transcription.
A `gap` verdict must name what would expose it, following the existing
`PLAN.md` convention — "latent; needs a sky-polygon fixture" — rather than a
bare `TODO`.

### Enforcement

`crates/starlink-ast/tests/test_mapmerge_audit.rs`:

- every Rust mapping class that implements `map_merge` or overrides
  `simplified` has a checklist file;
- every row in every checklist has a verdict from the closed set;
- every `gap` verdict names a `PLAN.md` row that exists;
- the class list is derived by scanning `crates/starlink-ast/src/mappings/`, so a
  new class without a checklist fails.

This is the same pattern as `test_verification_ledger.rs`, which caught two
bookkeeping drifts this session, and it makes the `simplifier-mapmerge-rebuild-plan.md`
failure mode — a document asserting completion it cannot back — structurally
impossible.

### Coverage and first targets

All Mapping classes, plus `Region::Simplify` and `GetDefUnc`.

Known suspects, to be done first because they are already diagnosed:

- **RateMap** — C's `MapMerge` is a 206-line body; the Rust class has no
  `map_merge` and a comment covering only its opening 13 lines
  (`ratemap.c:1094-1107`).
- **ChebyMap** — no Rust `map_merge`; in C it inherits PolyMap's.
  Confirm the Rust equivalent inherits, or record why not.
- **The clear-on-set convention** — eleven classes, table above.
- **CmpMap** — the four remaining under-tagging cases point here; C stamps the
  surviving sub-Mapping in `CmpMap::MapMerge`'s self-simplify head
  (`cmpmap.c:1437`).

## W2 — the defects

Every fix waits on its class's checklist, so we learn whether a symptom is one
gap or several.
The eleven ungated failures group into six causes, enumerated in
`SERIAL_UNGATED` in `test_simplify_regression.rs`.

| cause | fixtures | defect | order |
|---|---|---|---|
| A | 4 SwitchMap cases | `IsSimp` missing on the FSMap / ISMap sub-Mappings | 1 |
| B | 2 `cefit_circle_*`, `neg_box_asymmetric_2d` | Region simplify tags a Mapping inside the encapsulated FrameSet that C leaves bare, and never tags the returned Region | 1 |
| C | `box_unc_fillfactor_merge`, `nullregion_unc_fillfactor_merge` | emits a `Prism` uncertainty where C emits a `Box` | 2 |
| D | `cap_zoommap_04` | emits `FrmSet`/FrameSet where C emits `Points`/PointSet | 2 |
| E | `cap_interval_01` | emits `Closed = 1` where C omits it | 3 |
| F | `cap_pointlist_01` | two mesh coordinates diverge past 4 ulp | 3 |

A and B are pure `IsSimp` placement, clear the CI red, and unblock the most
rows.
C and D are genuine simplification gaps that only the byte comparison can see.
E and F are small and independent.

The four under-tagging cases — `cap_cmpmap_01`, `cap_cmpmap_03`,
`cap_cmpmap_04`, `cap_specmap_02` — are outputs of the CmpMap checklist, not
separate tasks.
Six approaches to `cap_cmpmap_03` were measured and reverted previously; those
are recorded in the harness and must not be retried without a new reason.

### What is known about cause B

C's dump of `cefit_circle_conformal` carries no `IsSimp` at all, and the
instrumented build shows the ZoomMap it writes is a freshly constructed object
with `flags = 0`, not the flagged one the loader produced.
Since C's copy constructor preserves `flags`, some constructor on the Region
simplify path builds that ZoomMap.
Which one is not yet identified.
Identify it from the C source before writing Rust; do not infer a rule from the
fixture.

## Definition of done

- `SERIAL_UNGATED` is empty.
- The simplify `IsSimp` license — `ISSIMP_PLACEMENT_LICENSED` in
  `test_simplify_regression.rs` — is empty, and `ISSIMP_DISAGREEMENTS` is
  empty.
- `ISSIMP_PLACEMENT_LICENSED` in `test_wcsconv.rs` contains only FitsChan
  reader-order cases, each row naming the C line that causes it.
  No row may cite a stale reference as its reason.
- Every mapping class has a checklist with no unverdicted rows, and every
  `gap` has a live `PLAN.md` row.
- `check_corpus_fresh.sh` reports no card-only or structural drift, and the
  nine platform-only cases are the only numeric drift it reports.
- `PLAN.md` §6 and the relaxation ledger agree with the code, and §9.3's two
  incorrect figures are corrected.

## Risks

- **The audit finds more than six causes.**
  Likely, and the point.
  New findings become `gap` rows with a named exposing fixture; they do not
  block the six known fixes.
- **A fix looks like a regression against a reference we have not yet
  verified.**
  This has now happened three times.
  The standing rule: before believing a regression, compare three numbers —
  port, committed fixture, pinned C.
- **W3.2's hunk split rests on an assumption.**
  That the only upstream change to those six files is `IsSimp` placement and
  the only local change is the last bits.
  The two assertions in W3.2 test it directly; if either fails, that file needs
  a genuine Linux capture and the assumption was wrong for it.
- **Cause F may be irreducible.**
  If the divergence is platform arithmetic rather than a port defect, it
  belongs in the `astequal` compare mode with a reason, not in a fix.
  Decide it with the C oracle, not by loosening a tolerance.

## Tooling of record

- `tools/oracle_fixtures/build_issimp_instrumented.sh` (`46b8360`) — builds a
  copy of the pinned C that logs every `astSetIsSimple` with a resolved
  backtrace, plus a `gen_simp` driver.
  Verified to produce byte-identical output to the stock build across the whole
  `simplify_fixtures` corpus.
- `ast_tester/wcsconverter.c` built against a cmake build of the submodule —
  the oracle for every `.native` reference.
  Pass the manifest's `attrs` field; omitting it produces false structural
  differences.
- `AnyObject::from_mapping` — the single serialization dispatch both harnesses
  use.

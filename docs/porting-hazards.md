# Porting hazards

Failure modes that have already cost this project real time, indexed by **when they apply** rather than by when they happened.
Each entry names the check that would have caught it and the commits that prove the cost.

Two ports are drawn on: the current Rust port (`tickets/DM-54793`, 1,087 commits from 2026-04-27) and the earlier prototype (`../starlink-astrs-proto`, 366 commits, March 2026, assessed in `rust-prototype-port-assessment.md`).

## How to use this file

Do not read it end to end.
Find the trigger that matches what you are about to do, and read that entry.

The reason it exists in this form is itself a finding.
The prototype's post-mortem documented the single most expensive mistake in this project — collapsing C's per-class extension points — and the current port then made the same mistake, in the same subsystem, with that document sitting in the repository.
A narrative retrospective did not transfer.
A trigger-indexed check has a chance.

## The enforcement hierarchy

Prefer, in order:

1. **A test that fails.** Nothing to remember, nothing to read.
   Two exist and are the model: `test_deferred_fixtures.rs` fails if the deferred-fixture set changes without a matching `DEFERRED_CLASSES` row, and `native_read_excluded_entries_still_fail` fails when an excluded wcsconv case starts passing.
2. **A check in `CLAUDE.md`.** Read every session; keep it to the imperative.
3. **An entry here.** For hazards that cannot be mechanised — where the judgement is the whole content.

Any entry below that can be moved up this list should be.
When you convert one, say so in the entry rather than deleting it: the evidence is what makes the test worth keeping.

---

## A. Before porting a class or algorithm

### H1 — Collapsing C's per-class extension points into one central function

**Trigger.** You are about to implement, in one place, behaviour that C spreads across a per-class `vtab` method.

**Hazard.** Some `vtab` entries are ceremony (`astGetAttrib` string dispatch, per-attribute dump/load, loader glue) and collapsing them is the point of the port.
Others are the extension mechanism a generic driver in another file calls back into, and collapsing those makes the driver's control flow unportable.
The resulting engine passes most fixtures and diverges on any case that depends on the *order and interaction* of steps — and every such divergence presents as a missing rule, so the wrong fix is always cheap and available.

**Evidence.** The Rust simplifier was grown as standalone helpers over a flattened adjacency list (`5fd7c78`, then roughly thirty strategies on 2026-05-08 alone, plus an oscillation detector `81fc844` and a fingerprint recursion guard `0d0be70`).
It was rebuilt onto C's nominate loop with a per-class `map_merge` between 2026-06-26 and 2026-07-19 — 47 commits — ending with `0b31272`, "retire the legacy global-helper scaffold".
128 of the port's 1,087 commits touch simplification.
Full diagnosis in `docs/issues/simplifier-mapmerge-rebuild-plan.md`.

The prototype made the identical mistake: `cmp_map.rs` reached 139% of `cmpmap.c`'s code while covering fewer classes, with 51 runtime type interrogations against C's 10.

**Check.** Before writing Rust for a class, list its `vtab` entries in the C source and classify each as ceremony or architecture.
The test for architecture is mechanical: **does a generic driver in another file call this through the vtab?** If yes, the Rust port needs the same seam at the same granularity, even when one central `match` would pass today's fixtures.

Known architecture, not boilerplate: `MapMerge`, `Transform`, `Simplify`, `Equal`, `MapSplit`, `Match`, `SubFrame`, `Overlay`, `RegPins`, `RegBaseMesh`, `RegBaseBox`, `GetDefUnc`.

### H2 — Reshaping the structure C builds, because the reshape is equivalent

**Trigger.** You are about to build a mapping chain, frame tree or object graph in a shape that differs from C's, having satisfied yourself it is mathematically equivalent.

**Hazard.** It is equivalent *as a function* and not equivalent *as an input to whatever consumes the structure* — the simplifier, the serializer, `MapSplit`.
The divergence then surfaces far from the change, and the natural suspects are all the wrong ones.

**Evidence.** The FITS reader built the celestial contribution as a 2-axis mapping wrapped in a parallel `CmpMap`, where C builds an N-axis mapping that passes the other axes through.
A redundant FK5↔ICRS SlaMap pair, buried in two *separate* parallel `CmpMap`s, could then never cancel: `sip-3d-swap` simplified to nine elements where C reached three.
Refactored in `d9cbfd8`→`95eac6a`.
`docs/issues/wcsmap-naxis-refactor.md` records the investigation.

**Check.** Match C's construction shape, not just its arithmetic.
When a divergence appears downstream of a reshape, suspect the shape before the consumer — and note the model that investigation followed: it verified the simplifier (`neg`/`parallel_in_series_with_blocking_delta`), the FrameSet integrity port (`8576be7`) and the SphMap merge rules (`ad16938`) as correct *first*, each with a named test, before concluding.

### H3 — Merging two C concepts into one Rust field because they usually agree

**Trigger.** Two C members, or a C member and a serialized token, look like the same thing.

**Hazard.** They diverge in exactly the cases you have no fixture for, and because the field is load-bearing in two subsystems, untangling it later touches both.

**Evidence.** The serialized `IsSimp` token and the internal "already-simplified, do not re-merge" barrier were one field.
Separating them took `ca743c1` ("port IsSimple semantics — rename flag, clear on invert"), `e9ac139` ("use Frozen (not IsSimple) as the component merge barrier"), `d923dee` ("decouple serialised IsSimp tag from merge-guard flag"), and a series of tag-placement fixes — 14 commits over three months.
Part of the residue then turned out to be a genuine C quirk rather than a Rust bug, so the port was chasing a moving target for part of that time.

**Check.** If the C source has two names, keep two fields until a fixture proves they cannot diverge.
Cheap to split early, expensive to split late.

### H4 — Not modelling a C-wide mutable-state convention before relying on it

**Trigger.** You are about to depend on a C convention that has no natural equivalent in the owned-value design — `Invert`, `Frozen`, `RESTRICTED_SIMPLIFY`, reference-counted sharing.

**Hazard.** With no model written down, each call site re-derives the convention locally, and they disagree.
The disagreement is invisible until something composes them.

**Evidence.** C's `Invert` is a mutable per-object bit with defined behaviour under encapsulation: the parent records the child's orientation at construction and forces it back before transforming.
No Rust model of this was written.
The result was most of July spent on orientation repair — `6866eb8` (push parallel inversion to deepest leaves), `ff319fd` (preserve leaf orientation across an invert-normalising `simplified()`), `7899e17` (materialise inverted PermMap leaves forward in flatten), `ff1a785` (restore leaf orientation in parallel reconstruct), `f24edc5` (store simplified elements at their returned orientation), `aa7242c`, `1cdb4d5`, `2804c6f`.
27 commits mention invert/orientation, and they landed *after* the H1 rebuild, so the same ground was covered twice.

**Check.** Write the mapping from the C convention to the Rust representation before the first call site — a paragraph in the module doc is enough.
Name what holds the state, who is allowed to change it, and what happens on clone, encapsulation and serialization.

---

## B. While making a failing case pass

Three stop signals.
Each is cheap to check, and each has been ignored here at measurable cost.
When one fires, stop patching and go read the C source.

### H5 — A threshold, tolerance or iteration count with no analogue in the C source

**Hazard.** If C gets the right answer without it, C is doing something structurally different.
The heuristic will get closer and never arrive.

**Evidence and the good outcome.** CRPIX/CD extraction went through five rounds in eighteen days, ending in a Newton refinement with a 1e-9 threshold.
`5d9aaa6` removed it, and is the model commit for this whole file: it names the heuristic, explains that C reaches bit-exact CRPIX *without iteration* by extracting the CD matrix from a least-squares straight-line fit over the full pixel range, lists the three coordinated ports required (`FindBasisVectors`, `FitLine`, `SIPIntWorld`), records exactly what stays broken meanwhile (half-pixel CRPIX formats as `50.499998`), and schedules the work.
The real ports landed within the week (`1f9f3b7`, `6f219a4`).
`ea3b5df` deleted a CROTA snap-to-zero workaround the same way.

**Check.** Grep the C source for the constant.
If it is not there, it is not the algorithm.
If you must ship the heuristic to unblock something, do what `5d9aaa6` did: name it, say what it costs, and schedule the port.

### H6 — Your fix makes the target case pass and breaks a negative control

**Hazard.** Negative-control fixtures encode "C does *not* simplify / merge / convert this".
Breaking one means the rule you just added is not the rule C has.
Two or three in a row is an architectural mismatch, not a missing rule.

**Evidence.** This is the signal that went unread for seven weeks in H1.
The rebuild plan states it plainly: each attempt to fix a case by adding or loosening a standalone helper "either misses the case ... or breaks a negative-test invariant (e.g. `neg_win_swap_no_simplify`)".

**Check.** Treat a broken negative control with the same gravity as H5.
Never loosen a negative control to make a positive case pass.

### H7 — The error shrinks monotonically across fixes and never reaches zero

**Hazard.** You are curve-fitting, not fixing.
The cause is usually two layers away from the symptom.

**Evidence.** The prototype's most expensive bug: a wrong spectral value chased through four rounds of increasingly sophisticated heuristics, driving the error from 1.3 kHz to 295 Hz without reaching zero.
The cause was a missing UTC→TDB conversion, found by reading `fitschan.c`.
Three of the four rounds were spent tuning against a FITS keyword that C AST reads and then ignores.

**Check.** Stop at the second round.
Go and find the C function that computes the quantity, and read what it does *before* the arithmetic you are looking at.

### H8 — Accreting one-off conversions instead of porting the conversion function

**Trigger.** You are adding a second or third unit, epoch or time-scale conversion at a call site.

**Hazard.** Each one is individually justifiable and the set is never self-consistent.

**Evidence.** Over 2026-05-22/23 three separate conversions were bolted into the FITS writer: `fd19ef7` (epoch TT→UTC for MJD-OBS), `61a8e2b` (input MJD-OBS UTC/TAI→TT), `985ce1b` (TDB−TT delta).
Resolved properly by porting C's `TDBConv` wholesale (`7bd8efe`) and then routing the path through `TimeFrame`'s `astConvert` (`8df8ca0`).
Three days, not four rounds — because the answer was looked for in `fitschan.c` rather than in the residual.
This is H7 caught early, and it is the counter-example worth imitating.

**Check.** At the second conversion, find C's conversion function and port it.

---

## C. When landing many classes in one pass

### H9 — A breadth sweep that passes the existing tests looks finished

**Trigger.** You are about to land several classes in a day.

**Hazard.** Breadth sweeps are legitimate — they unblock fixtures and surface shape problems early.
They are also how both ports accumulated their largest debts, because the tests that exist do not exercise what was skipped, so nothing records that anything was skipped.

**Evidence.** Nine Region classes landed on 2026-05-11 in 31 commits (`d60047e`..`3abd301`) with planar geometry and placeholder simplification, and passed the `testregions.c`-derived tests.
The bill arrived twelve weeks later as a dedicated parity campaign on 2026-08-01/02: great-circle line geometry (`d6c22de`), on-boundary `astRegPins` (`b022546`), SkyFrame `ResolvePoints` (`d6372da`), geometric `OverlapX` (`d834b81`), `astSetUnc` (`d5e78b6`), per-class `astGetDefUnc` (`c608570`), Circle/Ellipse simplify re-fit (`a96bc80`) — plus nine previously-ignored tests finally enabled.
One item is still open: `MESH-GEO`, Polygon's `reg_base_mesh` planar where C's is geodesic.

The prototype's post-mortem identified its own one-day breadth sprint as the source of most of its stubs.

**Check.** Record the debt in the same commit range, three ways:

- Name the C algorithm you did *not* port, per class, in the module doc, with the C file and line.
  `polygon.rs`'s planar-mesh note citing `polygon.c:4329` is the model.
- Add a `PLAN.md` row per gap, stating **what would expose it**, not just what is missing.
  "Latent; needs a sky-polygon fixture" names the missing test.
- Prefer a `todo!()` or an error to a plausible return value.
  The prototype's `BoxRegion::overlap()` returned a hardcoded `4` for weeks; a panic would have been found in minutes.

---

## D. When writing or changing a test harness

### H10 — A harness that can pass without comparing

**Hazard.** Not a gate.
Three in this repository could, and the one that survived longest hid a real defect.

**Evidence.** `dc87527` retired the wcsconv manifest's skip mode ("so no case can be silently masked"); `ea65074` added the companion test that fails when an excluded case starts passing; `9bbfa68` made the simplify regression harness hard-fail rather than printing a count and returning `Ok`.

**Check.** No skip mode.
A case that cannot be compared yet goes in a named exclusion list with a per-case reason string, plus a test that fails when it starts passing.
`NATIVE_READ_EXCLUDED` in `test_wcsconv.rs` is the pattern, and it is currently empty.

### H11 — A comparator that normalises away a class of difference

**Hazard.** Whatever the comparator normalises becomes unfalsifiable, and the corpus will not save you if it contains no instance of it either.

**Evidence.** `native_round_trip_fixture` compares with `trim()` and `split_whitespace()`, so trailing whitespace is invisible.
That is exactly the shape of the defect fixed in `86810ce`: C's `WriteObject` emits `" ="` for a comment-less nested object where Rust emitted `" = "`.
The corpus could not have caught it either — `grep -rhcE " =$"` returns zero across every scanned fixture directory, and the four fixtures that do contain such a card sit outside all of them.
**Still open**; both halves are recorded in `PLAN.md` §5.

**Check.** If a comparator normalises anything, the corpus needs a fixture that exercises what was normalised.
Relaxations are per case, named in source, and justified in a comment — never a widened global tolerance.

### H12 — Declaring a gate that does not hold

**Hazard.** Worse than having no gate, because it stops anyone looking.

**Evidence.** The prototype declared `#![deny(clippy::all)]` and ran red: 87 findings, ~49 genuine.
The current `CLAUDE.md` claimed tier-5 oracle benches fail CI at >10% hot-path regression; `ast-oracle-bench` is a six-line criterion placeholder and CI runs no benchmark at all.
Meanwhile the 2026-05-19 snapshot records several cheap mappings 5–42× slower than C at bulk N.

**Check.** Before documenting a gate, run it.
If it does not exist, document its absence instead — that is what a reader needs in order to compensate.

---

## E. When syncing the C reference

### H13 — A faithfully mirrored C quirk becomes a bug when upstream fixes it

**Trigger.** You are about to move the `reference/starlink-ast` pin.

**Hazard.** The port deliberately mirrors some C behaviour that is wrong, to stay byte-exact.
Each such mirror is a scheduled future defect, and it fires silently — the fixtures still agree with each other, just not with the new C.

**Evidence.** `bfacdfe` mirrors `EncodeFloat`'s length-preserving exponent-zero strip; `36f0863` licenses the `IsSimp` aliasing quirk as astequal-only.
Both are being fixed on C's `u/timj/misc-bugs` branch, along with two more output-changing fixes.
`PLAN.md` §8 carries the per-row action and the required sequencing.

**Check.** Before moving the pin: diff the incoming range for serialization changes, and grep `docs/issues/c-library-quirks.md` for entries the port *mirrors* rather than merely records.
Regenerate all affected corpora in one commit range — piecemeal pin moves make each regeneration ambiguous.

**Marker convention.** Code that exists *only* to reproduce such a value carries a **`C-ARTIFACT`** tag in its doc comment, so `grep -rn C-ARTIFACT` enumerates the removable emulation surface.
That is a much narrower claim than the ~1,500 "mirrors C" comments elsewhere, which mostly record a faithful algorithm port and are not removable.
Tag the item, say whether the dump-only property is measured or assumed, and point at the quirks entry.
The `CmpMap` `InvA`/`InvB` cluster is the worked example: `cmpmap.rs`'s module docs define the marker, and `PLAN.md` §2 carries the measurements and a proposed unification.

---

## F. Review signals worth measuring

### H14 — A Rust subsystem larger in code than the C it replaces

**Hazard.** Not a defect by itself, but an indicator that functionality was accreted rather than ported, and therefore that H1 or H2 may be present.

**Evidence.** The metric that condemned the prototype's `cmp_map.rs`: 139% of `cmpmap.c` while covering fewer classes, with 51 runtime type interrogations against C's 10.

**Measure it correctly, or it produces false positives.** The first application of this check in this repo reported `fitschan/` at 1.12× of `fitschan.c` and named it the one subsystem that had failed to achieve a density win.
That was wrong. The Rust figure included 8,582 lines of inline `#[cfg(test)]` unit tests, while the C figure counted only `src/` — C's tests live in `ast_tester/`.
Stripping test modules puts `fitschan/` at **0.73×**, and every subsystem in a 0.41–0.94× band.

Two controls are therefore mandatory before the signal means anything:

- **Strip `#[cfg(test)]` modules from the Rust side.** They are 18,998 lines across `ast-core`, unevenly distributed — 8,582 in `fitschan/`, 65 in `regions/`. Comparing with them included is not a like-for-like measure.
- **Count code lines, not total lines.** The AST C sources are 52.6% comment; a `wc -l` comparison flatters C by about a factor of two.

**Expect Rust to use ~1.8× the lines per statement.** Measured on `fitschan`: 0.36 semicolons per code line against C's 0.65, because `rustfmt` gives every closing delimiter its own line (21% of Rust's lines are structural only).
Rust wins on totals anyway because it needs ~40% as many statements — no `int *status` threading (456 signatures, 1,989 call sites, 291 `!astOK` guards in C, replaced by 377 `?` operators), and no C89 declaration preambles (2,049 lines, 11% of `fitschan.c`).
So a ratio near 1.0× is the thing to investigate, and a ratio *below* 0.5× is unremarkable rather than impressive.

**Check.** `scc` the subsystem against the C files it replaces, in code lines (the C sources are 52.6% comment, so total-line comparisons are meaningless).
Above ~1.0×, look for a collapsed extension point or a reshaped structure before concluding the subsystem is merely verbose.

# starlink-astrs: Progress, Development and Verification Review

**Reviewed:** 2026-08-18 · **Tree:** `tickets/DM-54793` @ `bc620a6`
**C reference:** pinned at `50c2b21b` (2026-07-06), with `u/timj/misc-bugs` read at 40 commits ahead
**Prototype:** `../starlink-astrs-proto`, 366 commits, 2026-03-03 → 2026-03-28

Every figure below was measured on the current trees.
Test counts come from a full `cargo test --workspace` run; line counts from `scc`; history from both git repositories.
Nothing is quoted from a planning document without being re-derived.

Line counts are given as **code lines** (blanks and comments excluded) unless stated otherwise.
This matters more than usual here: the AST C sources are 52.6% comment by volume, so any total-line comparison between the two trees flatters C by roughly a factor of two.

Cross-repository attribution is inferred from timing and content.
The C-side commits deliberately do not reference the Rust port, so no claim here rests on commit-message wording.

---

## Contents

1. [Where the port stands](#1-where-the-port-stands)
2. [Timeline: two histories, interleaved](#2-timeline-two-histories-interleaved)
3. [The oracle relationship: from consumer to co-verifier](#3-the-oracle-relationship-from-consumer-to-co-verifier)
4. [Mistakes, and what each one cost](#4-mistakes-and-what-each-one-cost)
5. [What became of the prototype's findings](#5-what-became-of-the-prototypes-findings)
6. [Status against the oracle](#6-status-against-the-oracle)
7. [Forward risks](#7-forward-risks)
8. [The pattern across four months](#8-the-pattern-across-four-months)
9. [Appendix A: what was added to CLAUDE.md and docs/porting-hazards.md](#appendix-a-what-was-added-to-claudemd-and-docsporting-hazardsmd)
10. [Appendix B: what this review contributed to PLAN.md](#appendix-b-what-this-review-contributed-to-planmd)

---

## 1. Where the port stands

The port covers one coordinate-systems pipeline in real depth.
Read a FITS header, build a `FrameSet`, transform coordinates, simplify the mapping, write the header back out.
It now does that across eight FITS encodings in both directions, with byte-exact agreement against C on every case in the corpus.
Regions, the full `Frame` base class, spectral and time frames, and `KeyMap` have all landed since May.

What has not started is the thing the design exists for.
The stated goal is binary ABI compatibility: existing applications — pyast, the Starlink suite, DS9 — link against the Rust build with no source change.
`ast-capi/src/lib.rs` is a one-line comment.
Everything verified so far is verified through the Rust API; no C program has ever linked this library.

| Measure | Value |
|---|---|
| Commits | 1,087 (2026-04-27 → 2026-08-08, 53 active days) |
| `ast-core` source | **82,025 code lines** across 144 files (120,466 total; 27,334 comment, 22.7%) |
| C reference `src/` | **184,817 code lines** across 163 `.c` files (499,874 total; 263,063 comment, 52.6%) |
| Port vs C, whole library | 44% by code volume |
| Port vs C, subsystems with a Rust counterpart | 70% by code volume (82,025 vs 116,845) |
| Tests | 3,280 passing, 0 failing, 1 ignored (investigative only) |
| wcsconv round-trip cases | 194 compared, 0 excluded |
| Simplify fixtures byte-exact | 427 |
| Transform-oracle sections gated | 1,606 (259 headers + 20 framesets + 1,327 simplify) |
| C defects and quirks catalogued | 36 (13 fixed upstream, ~19 in flight) |
| Workspace crates still stubs | 4 of 6 |
| C-ABI symbols exported | 0 |

Per-subsystem code lines, against the C files they replace.
The ratio column is the interesting one, and it is not uniform.

| Subsystem | Rust code | C code | Ratio | C files counted |
|---|---:|---:|---:|---|
| `fitschan/` | 20,294 | 18,093 | **1.12×** | `fitschan.c` |
| `units/` | 2,581 | 2,495 | **1.03×** | `unit.c` |
| `serial/` + `channel/` | 3,049 | 3,224 | 0.95× | `channel.c` |
| `mappings/` | 26,601 | 42,602 | 0.62× | 27 mapping classes + `mapping.c` |
| `keymap/` | 3,019 | 4,958 | 0.61× | `keymap.c` |
| `frames/` | 16,236 | 27,315 | 0.59× | 11 frame/axis classes |
| `regions/` | 8,420 | 18,158 | 0.46× | region base + 9 subclasses |

The 0.46–0.62× band is what a faithful port of a C OO system should look like once `astGetAttrib` string dispatch, per-attribute dump/load boilerplate and loader glue are gone.
Two subsystems sit outside it, and both are worth noting.

`fitschan/` is **larger in code than the C file it replaces** — 20,294 against 18,093.
That is the same metric that condemned the prototype's `cmp_map.rs` (139% of `cmpmap.c`),
appearing now in the subsystem that absorbed 285 of the port's 1,087 commits and where functionality was accreted fixture-by-fixture before the `fset_to_store` port imposed structure.
It is not evidence of a defect — FitsChan is byte-exact on all 194 cases — but it is the one place where the port has not achieved a density win, and it is the natural first candidate for a consolidation pass.

The comparison has a known upward bias in the port's favour: the C column counts whole files, several of which contain algorithms the port has not implemented — `astResample`, `astRebin` and `astRebinSeq` inside `mapping.c` (10,826 code lines for that file alone), and the `-TAB` family inside `fitschan.c`.
So 70% is an upper bound on coverage-by-volume within these subsystems, not a completion figure.

---

## 2. Timeline: two histories, interleaved

`Port` rows are this repository.
`AST (C)` rows are the C library.
The port never ran ahead of its reference for long: each time it demanded something the C side could not supply, the C side changed.

| Date | Track | Event |
|---|---|---|
| 2026-04-20 | AST (C) | **The C verification campaign is already underway.** Since `b158c7ff`, the C library has taken 349 commits and 18 pull requests that are overwhelmingly test material: +116,260 lines against −780, with `src/` under 3% of the diff. The port arrives into a reference that is being instrumented, not a static one. |
| 2026-04-27 | Port | **Bootstrap through `FrameSet`, in one day (53 commits).** Vendored PAL, ERFA and wcslib; `ErrorCode` generated from `ast_err.msg`; the sibling `Mapping`/`Frame`/`Region` traits; `PointSet`; six simple mappings; `BasicFrame`, `CmpFrame`, `FrameSet` with graph storage and integrity. The submodule is pinned at `7c12422f`, the C FitsChan test expansion merged the same day. |
| 2026-05-01/02 | Port | **Native serialization, projections, `SkyFrame` (42 commits).** Native parser and renderer with the `Serializable` trait; WinMap, LutMap, PolyMap, ChebyMap; safe wrappers over vendored PAL and wcslib; WcsMap, SlaMap, SphMap; `SkyFrame` and `SkyAxis` with sexagesimal formatting. |
| 2026-05-03 → 05-09 | Port | **Simplifier v1, byte-exact output, FitsChan v1 (141 commits).** A central `try_merge` simplification engine; C-generated `.simp` fixtures; the byte-exact campaign that drove native round-trip mismatches from 22 to 0; spectral and time frames; `Channel`; all five FitsChan phases including every encoding. The wcsconv harness runs for the first time on 7 May at **40 of 192**. |
| 2026-05-06 | AST (C) | **Pin moves to `e0abc0fa`.** PR #45 begins the C `astSimplify` branch-coverage campaign that eventually produces roughly 695 fixtures and a permanent `simplify_pathways.md`. The port consumes it the day after it lands. |
| 2026-05-08 | AST (C) | **C gains a fixture generator written for the port** (`f9500e79`, *"Add gen_native_fixtures for Rust port byte-exact testing"*). The port needed C to emit reference dumps on demand; C grew the tool, and the pin moved the same day. First commit where the influence runs backwards. |
| 2026-05-10/11 | Port | **Two breadth sweeps (69 commits).** Nine remaining mapping classes in one day (Pcd, Grism, UnitNorm, Tran, Rate, Spline, Dss, Norm, Math) plus `DSBSpecFrame`; then nine Region classes in a single day. Both bought coverage cheaply and both came back for payment — see finding 7. |
| 2026-05-18/19 | Port | **Performance and the NaN-as-bad protocol (35 commits).** PolyMap gains a SIMD forward evaluator and Newton–Raphson inverse; the `BAD_VALUE` → NaN branchless transform protocol is swept across every mapping class with parity tests pinning it. A 24-variant Rust-vs-C per-class benchmark harness is built and reported. |
| 2026-05-19 | AST (C) | **C fixtures regenerated under C11 for the port** (three pin bumps in one day). Byte-exactness collided with a standards hazard: `DBL_DECIMAL_DIG` differed between C99 and C11, so C AST emitted different digit counts depending on how the oracle was built. Sixteen days later the C library retired the hazard outright (`d0a0d827`, PR #53): `AST__DBL_DIG` is now 17 under both standards. |
| 2026-05-20 → 06-07 | Port | **The FitsChan write side — the largest single block of work (320 commits).** `astMapSplit`, `astLinearApprox`, `AnalysePoly`; then `fset_to_store` ported function by function against `fitschan.c` — `OtherAxes`, `MakeIntWorld`, `CelestialAxes`, `SpectralAxes`, `SkyPole`, `SIPIntWorld`; `unit.c`; alternate-WCS plane emission; all 24 `SpecTrans` keyword translations; the N-axis WcsMap refactor. 285 of the port's 1,087 commits touch FitsChan. |
| 2026-06-09 → 06-15 | Port | **Audit-driven `Frame` base port (147 commits).** A function-by-function audit of the C Frame and Mapping classes is written first, then worked: `astFindFrame`, `astMatch`, `astSubFrame`, `astConvertX`, the `%G` coordinate formatter with `Digits` and `Direction`, `astPolyTran`, `astMapBox`, the FrameSet `Variants` subsystem. `testconvert.c` is ported as the acceptance gate. |
| 2026-06-19/22 | AST (C) | **First batch of port-found defects lands in C** (PRs #57–#61): `astMask` disagreeing with `astTranN` for large sky polygons, RefRA/RefDec precision loss, and a quirks batch. |
| 2026-07-01/06 | AST (C) | **C builds a transform-output oracle** (PR #63): Halton low-discrepancy input sampling, golden forward and inverse checks, masked equivalence and a round-trip accuracy gate, with tens of thousands of lines of recorded values. PR #64 closes the simplify coverage campaign. `50c2b21b` becomes the port's pin, and remains C master's head today. |
| 2026-06-25 → 07-19 | Port | **The simplifier is rebuilt on C's architecture (86 commits).** The central pattern-matcher is replaced by C's nominate loop plus a per-class `map_merge` on every Mapping class, and the legacy global-helper scaffold is retired. Mid-rebuild, the upstream sync: seven commits marked *"match fixed C"* adopt C's fixes for bugs the port had faithfully reproduced, and 64 new simplify fixtures are imported. |
| 2026-07-30 → 08-05 | Port | **Region parity, transform oracle, wcsconv closed out (129 commits).** Great-circle geometry, winding-relative polygon membership, on-boundary `astRegPins`, `astSetUnc`, per-class `astGetDefUnc`, geometric `OverlapX`, Circle/Ellipse simplify re-fit. The C transform-oracle checker is ported and its 1,606 sections put behind asserting tests. wcsconv reaches 194 of 194 with the exclusion list empty and gated so it cannot refill. |
| 2026-08-06 → 08-08 | Port | **`KeyMap`, and 17 new C bug reports (50 commits).** Ported from a written spec: hash function, conversion matrix, scalar and vector accessors, seven `SortBy` modes, Native serialization, `testkeymap.c` in full. Porting it surfaced seventeen C defects, from undefined behavior in out-of-range double-to-int casts to an `astMapIterate` that never terminates when `SortBy` is set. |
| 2026-08-08 → 08-18 | AST (C) | **C fixes the KeyMap and FitsChan findings.** On the unmerged `u/timj/misc-bugs` branch: nineteen defects across `keymap.c`, `fitschan.c` and `mapping.c`, 25 source files, +872/−235. `astMapGetElem<X>` on an undefined entry was fixed in C on 8 August, the same day the port changed its own behavior and filed the report. The loop has closed to about a day. |

### Where the calendar is empty

Six multi-day gaps punctuate the work: 12–17 May, 28–30 May, 16–24 June, 27 June – 12 July, 20–29 July.
The two longest (16 days, then 14) both fall inside the simplifier rebuild, which is the single stretch where progress was structural rather than fixture-driven and therefore had nothing to show per day.

---

## 3. The oracle relationship: from consumer to co-verifier

The port began as a pure consumer.
It pinned a C commit, generated fixtures from it, and treated any difference as a Rust bug.
That is no longer the relationship.

Of the 36 entries in `docs/issues/c-library-quirks.md`, 13 have been fixed upstream, one turned out not to be a C bug, and roughly 19 more sit on an unmerged C branch awaiting review.
The port has become the C library's most productive defect source.

Three distinct kinds of interaction appear in the history, and they carry different lessons.

### 3.1 The port demanded instruments C did not have

Byte-exactness is a harder contract than the C test suite ever imposed on itself.
Satisfying it required new C-side machinery: a fixture generator (`f9500e79`), fixtures regenerated under a pinned C standard, and ultimately the removal of the standard-dependence itself.
None of that was on the C roadmap.

### 3.2 C built instruments the port then adopted

The traffic runs both ways.
C's `astSimplify` coverage campaign produced roughly 695 fixtures that became the port's regression corpus.
C's transform-output oracle (PR #63, 3 July) was ported into Rust as `test_transform_oracle.rs` on 5 August — parser, comparison primitives, override grammar and all — giving the port 1,606 gated sections of recorded C behavior it did not have to invent.

### 3.3 Reading C closely enough to port it finds bugs that reading it casually does not

The catalogued defects cluster exactly where the port went deepest.
KeyMap accounts for 17 entries, all found in three days of porting it; FitsChan and the simplifier's flag handling account for most of the rest.
Several are undefined behavior that happens to work on glibc — out-of-range double-to-int casts in `ConvertValue`, numeric-string overflow into `%d`, an `Entry1X` read as an `Entry0X` for a zero-length vector.
A faithful port is a line-by-line audit with a machine checking the reader's comprehension.

### 3.4 The interesting hard case: when the port refuses to mirror C

Two C quirks produce serialized values determined by C's object lifetimes rather than by the mapping.
Which node in a simplified tree carries `IsSimp = 1` depends on which pointers happened to be aliased during simplification.
`WcsNative` dumps an `Invert` flag on a PermMap it shares between two stages, acquired *after* encapsulation.

Mirroring either would mean encoding C's reference-counting history into Rust read paths.
The port instead declined, documented both in the quirks catalogue with the C line numbers and the upstream suggestion, and licensed five wcsconv cases to drop those tag lines from the comparison.
C is now fixing both: deterministic `IsSimp` placement, and no inversion of the encapsulated PermMap.
The judgement call was right, and it will let those five relaxations be retired.

---

## 4. Mistakes, and what each one cost

Ordered by cost.
The first is the most expensive thing that happened, and it is the one the project had already been warned about in writing.

### Finding 1 — the simplifier was built as a central pattern-matcher, the same mistake the prototype post-mortem had already named

C's per-class `MapMerge` method looks like object-oriented boilerplate, and both the prototype and this port treated it as such.
The Rust simplifier grew as standalone helpers over a flattened adjacency list — `try_swap_winmap_past_matrixmap`, `try_swap_wcsmap_past_permmap`, `try_distribute_winmap_into_parallel` — one per fixture that failed.
On 8 May alone, roughly thirty such strategies landed, along with a fingerprint-based recursion guard and an oscillation detector to stop the loop cycling.

The failure signature is precise, and `docs/issues/simplifier-mapmerge-rebuild-plan.md` states it in its own words.
Cases that depend on the *order and interaction* of simplification steps diverged, and every attempt to fix one by adding or loosening another helper either missed the case or broke a negative-control fixture.
That is an architectural mismatch, not a missing rule.
C runs a per-class, nominate-driven rewrite loop; Rust was pattern-matching on adjacencies.

**Cost.**
The engine was rebuilt between 26 June and 19 July, 47 commits in that window, adding `Mapping::map_merge` to every class, a faithful port of C's `PatternCheck`, and the nominate loop, then retiring the whole legacy scaffold.
128 of the port's 1,087 commits touch simplification.

**Compounding.**
Because control flow diverged, every symptom presented as a missing rule, so the wrong fix was cheap and available every time.
The prototype's post-mortem had documented this exact trap, in this exact subsystem, six weeks earlier.

### Finding 2 — `Invert` and orientation semantics were never modelled, so they were re-derived at 27 call sites

In C, `Invert` is a mutable per-object bit with defined behavior when the object is encapsulated in a CmpMap.
The parent records the child's orientation at construction and forces it back before transforming.
The port's owned-value design has no equivalent, and no model of one was written.

The consequence is a July spent almost entirely on orientation repair: pushing parallel-CmpMap inversion down to leaves, preserving leaf orientation across an invert-normalising `simplified()`, materialising inverted PermMap leaves forward during flatten, restoring child orientation in parallel reconstruct, storing simplified elements at their returned orientation.

**Cost.**
27 commits, concentrated *after* the rebuild rather than before it, meaning the same ground was covered twice.
A short design note on how C's `Invert` maps onto owned values would have been cheaper than either pass.

### Finding 3 — the serialized `IsSimp` token and the internal merge guard were one field

C has two related but distinct concepts: a flag serialized into the dump, and a barrier that stops an already-simplified component being re-merged.
The port used a single field for both.

Untangling it took *"use Frozen (not IsSimple) as the component merge barrier"*, *"decouple serialised IsSimp tag from merge-guard flag"*, *"port IsSimple semantics — rename flag, clear on invert"*, and a series of tag-placement fixes.

**Cost.**
14 commits across three months.
Part of the residue turned out to be a genuine C quirk rather than a Rust bug (see §3.4), which is why it took so long to resolve — the port was chasing a target that was itself wrong.

### Finding 4 — equivalent-but-reshaped is not equivalent downstream

The FITS reader built the celestial contribution as a 2-axis mapping wrapped in a parallel `CmpMap` alongside the spectral and other-axis pieces.
C builds it as an N-axis mapping that passes the non-celestial axes through unchanged.
Mathematically identical; structurally different.

The consequence surfaced far away.
A redundant FK5↔ICRS SlaMap pair, buried inside two *separate* parallel `CmpMap`s, could not cancel, so `sip-3d-swap` simplified to nine elements where C reached three.

The investigation is worth reading as a model.
`docs/issues/wcsmap-naxis-refactor.md` verifies the simplifier, the FrameSet integrity port and the SphMap merge rules as correct *first*, with a named test for each, before identifying the shape as the cause.

**Cost.**
A six-commit refactor lifting the celestial and spectral chains to N-axis end-to-end, plus the investigation that preceded it.
The generalisable rule: when the C structure is reshaped for idiomatic reasons, the divergence surfaces in whatever consumes the structure — here, the simplifier — not where it was introduced.

### Finding 5 — the heuristic ladder in the FITS writer, and the discipline that ended it

CRPIX and CD extraction went through five rounds in eighteen days.
Extract from a WinMap or ShiftMap, then solve algebraically, then mirror `GetFiducialWCS` with `astSimplify`, then preserve half-pixel CRPIX, then add a Newton refinement with a 1e-9 threshold.

The commit that removed the Newton step (`5d9aaa6`) is the best single artifact in the repository.
It names the heuristic; explains that C reaches bit-exact CRPIX *without iteration* by extracting the CD matrix from a least-squares straight-line fit over the full pixel range; lists the three coordinated ports required to do it properly; records exactly what stays broken in the interim (half-pixel CRPIX formats as `50.499998`); and schedules the work.
`FindBasisVectors`, `FitLine` and the full `MakeIntWorld` port landed over the following week.
A CROTA snap-to-zero workaround was deleted the same way once C's generic 1e-7 `SplitMat` snap was in place.

**What it cost, and what it saved.**
The ladder itself cost about a week of encoder churn.
The stop signal — a threshold with no analogue in the C source — is cheap to check and was checked.
Compare finding 1, where the equivalent signal (a fix that breaks a negative control) went unread for seven weeks.

### Finding 6 — time-scale conversions were accreted, then ported: the prototype's DSB saga in miniature

The prototype's most expensive bug was four rounds of increasingly sophisticated heuristics chasing a spectral error that came from a missing UTC→TDB conversion two layers away.
The same subsystem produced the same class of error here.
Over 22–23 May, three separate one-off conversions were bolted into the FITS writer: epoch TT→UTC for MJD-OBS, input MJD-OBS UTC/TAI→TT, then a TDB−TT delta.

The difference is what happened next.
On 27 May C's `TDBConv` was ported wholesale and wired in; on 2 June the whole path was rerouted through `TimeFrame`'s `astConvert`.
Three days of accretion, not four rounds of curve-fitting, because the answer was looked for in `fitschan.c` rather than in the size of the residual.

**Cost.**
Small, and worth recording as the improvement it is.
The prototype's lesson — an error that shrinks monotonically without reaching zero means you are curve-fitting — visibly transferred.

### Finding 7 — breadth sweeps bought coverage on credit, and the bill came twelve weeks later

Nine Region classes landed on 11 May in 31 commits: planar geometry, crossing-number polygon membership, placeholder simplification.
That was enough to make the `testregions.c`-derived tests pass, and enough to look finished.

The bill arrived on 1–2 August as a dedicated parity campaign: SkyFrame great-circle line geometry, winding-relative polygon membership, on-boundary `astRegPins`, SkyFrame `ResolvePoints`, geometric `OverlapX`, `astSetUnc` validate/remap/recentre, per-class `astGetDefUnc`, Circle and Ellipse simplify re-fit, great-circle bounds.
Nine previously-ignored tests were finally enabled.

One item from that sweep is still open.
Polygon's `reg_base_mesh` is planar where C's is geodesic (`polygon.rs:623` vs `polygon.c:4329`).
It is latent because no sky-polygon fixture currently reaches it, and it is flagged highest-priority in `PLAN.md` because it will affect overlap and MOC results when one does.

**Cost.**
Roughly the same work done twice, plus one live latent defect.
The prototype did the same thing on its own one-day breadth sprint, and its post-mortem identified that sprint as the source of most of its stubs.
The pattern repeated even though it had been named.

### Finding 8 — three gates that could not fail; two closed, one still open

The wcsconv manifest had a skip mode, so a case could be silently masked.
It was retired on 4 August, and a companion test now fails when an excluded case starts passing.
The simplify regression harness did not hard-fail; fixed the same day.
The exclusion list is now empty and gated so it cannot refill.

The third is still open, and it is instructive.
`native_round_trip_fixture` compares with `trim()` and `split_whitespace()`, so trailing whitespace is invisible to it.
That is precisely the shape of the bug found on 8 August: C's `WriteObject` emits `" ="` for a comment-less nested object where the Rust writer emitted `" = "`.
The comparator could not see it, and neither could the corpus, because `grep -rhcE " =$"` returns zero across every scanned fixture directory.
The four fixtures that do contain such a card all sit outside them.

**Cost of the open one.**
Unknown, which is the point.
A comparator that normalises away a class of difference, combined with a corpus containing no instance of it, is a gate with a hole in a known shape.
`PLAN.md` §5 records both halves of the fix required.

---

## 5. What became of the prototype's findings

The prototype ran 366 commits in March 2026 and was assessed as "a deep vertical slice, not a port".
This repository started fresh on 27 April with that assessment in hand.
Most of its findings were answered.
Two were not, and one strength was lost.

| Dimension | Prototype, March 2026 | Now | Verdict |
|---|---|---|---|
| `AST__BAD` handling (prototype finding 3) | Handled nowhere. Feeding `AST__BAD` through the LSST frame set returned a plausible, finite, entirely fictitious sky position. Found by a checksum in a benchmark, not by any of 400 tests. | `BAD_VALUE` in `constants` from day one; a per-axis BAD-handling fix swept every mapping on 8 May; a NaN-as-bad branchless transform protocol with dedicated parity tests; oracle comparators treat BAD as matching only BAD. | **Answered** |
| Simplifier architecture (prototype finding 2) | `cmp_map.rs` at 139% of the C file it replaced, 51 runtime type interrogations against C's 10, growing 21% in the final two days under commit messages reading *"Flailing with simplification"*. | Repeated, then fixed properly: per-class `map_merge` on every Mapping, C's nominate loop, C's `PatternCheck`. The three dedicated simplifier files are 4,563 code lines; C's equivalent logic is distributed across `cmpmap.c`'s `Simplify` driver (1,703 code lines for the whole file) plus a `MapMerge` in 32 class files (~2,236 code lines) plus per-class helpers that cannot be cleanly isolated, so no single ratio is meaningful. What is meaningful: structurally aligned, and byte-exact on 427 fixtures. | **Repeated first** |
| Error model | 11 `AstError` variants against C's 155 message codes. | ~179 `ErrorCode` variants generated from `ast_err.msg` by `build.rs`; hand-editing designed out. | **Answered** |
| Coordinate formatting | `Frame::format_axis` was literally `format!("{value}")`. `astFormat` could not produce sexagesimal output at all. | C's `%G` formatter core, `ParseAxisFormat` flag parsing, `Digits` and `Direction`, HMS/DMS, ISO dates, per-system `UsedUnits`, the `GetTitle`/`GetLabel` default tables. | **Answered** |
| FitsChan | Read only, 3,247 lines, no `astWrite`. Required exactly one longitude and one latitude axis, so spectral-only, time-only and linear-only headers failed. | 20,294 code lines. Read and write across eight encodings, 24 `SpecTrans` translations, alternate-WCS planes, SIP, TPN, ZPX, DSS, grism, `-LOG`, card consumption tracking, ASTWARN emission. | **Answered** |
| Regions | Stubs. 605 Rust lines against ~44,000 C. No base class, no uncertainty regions, no `astMapRegion`; `BoxRegion::overlap()` returned a hardcoded `4`. | Nine classes, 8,420 code lines. Mesh-based and geometric overlap, uncertainty machinery, great-circle sky geometry, `astRegPins`, masking. One geodesic-mesh gap remains open. | **Answered** |
| Declared quality gates | `#![deny(clippy::all)]` declared and red: 87 findings, ~49 genuine. A gate that does not hold. | `-D warnings` workspace-wide, clippy and fmt enforced in CI on both Linux and macOS, green. | **Answered** |
| Test posture | 369 tests; a plain `cargo test` ran exactly one live-oracle comparison; simplify parity tests gated behind 16 minutes of C recompilation; one harness compared the Rust reader against the Rust writer and so could not falsify either. | 3,280 tests, all running by default, all reading frozen C output. No gated-because-slow tier. Every relaxation is a named entry in a source list with a recorded reason. | **Answered** |
| The C ABI | 454 `astrs_*` entry points over `u64` handles. Not `ast.h`-compatible and no reference counting — the assessment called it "a test harness, not an ABI" — but it existed and worked. | `ast-capi/src/lib.rs` is one comment line. No exported symbols, no header, no `astClone`/`astAnnul`, no `catch_unwind` boundary. `ast-plugins`, `ast-ffi-tests` and `ast-oracle-tests` are likewise empty. | **Lost** |
| Performance | The prototype's headline result and strongest argument: 15–20% faster than C on bulk transforms, 1.5× on a full 4096² inverse grid, 3–8.5× on single points, with no vectorisation at all. | The small-N win holds — all 24 benchmarked variants beat C at N=16. At N=100k the picture inverts for cheap mappings: PermMap forward 42× slower, WinMap 6.6–7.6×, ShiftMap 5.7–7.6×, diagonal MatrixMap 5.5×, SlaMap 3.9×, WcsMap TAN forward 4.2×. PolyMap — the LSST and SCUBA-2 workhorse — is still 1.5× *faster* than C. | **Regressed** |

### Three things worth saying plainly about the performance row

**The documented hot path is not the casualty.**
`CLAUDE.md` protects the SCUBA-2 pipeline's per-time-slice `SkyFrame` mutation and the bulk PolyMap transforms, and both are fine.
The regressions are on mappings whose entire body is two arithmetic operations, where any per-point overhead dominates.

**But the gate that was supposed to catch this does not exist.**
`CLAUDE.md` states that tier-5 oracle benches fail CI if Rust is more than 10% slower than C on these paths.
`crates/ast-oracle-bench/benches/oracle.rs` is a six-line criterion placeholder, and CI runs `build`, `test -p ast-core`, `clippy` and `fmt` — no benchmark at all.
The only performance data is a hand-run snapshot dated 2026-05-19, three months stale, and none of its optimisation targets has been actioned.

**One hypothesis worth testing before optimising class by class.**
`PointSet` is row-major `(npoint, ncoord)`, where C keeps one contiguous array per axis.
For a mapping whose body is a scale and a shift, strided per-axis access and the resulting loss of vectorisation would plausibly explain the whole cheap-mapping cluster at once.
`docs/issues/mapping-perf-vs-c-oracle.md` does not consider layout — it attributes each case to per-class dispatch — so this is unverified, but it is one measurement away and it would change what the fix is.

---

## 6. Status against the oracle

The design specifies five test tiers.
Two are built and carrying real weight.
Three are empty crates.

| Tier | What it is meant to prove | Actual state | Status |
|---|---|---|---|
| 1 · Idiomatic Rust | The public Rust API behaves, and serves as worked examples. | 3,280 tests across 90 integration binaries plus 1,110 unit tests and 13 doctests. Zero failures. One ignored test, and it is the investigative oracle report, not a skipped assertion. | **Green** |
| 2 · Byte-exact regression | Rust output is byte-identical to C's on a real corpus. | 427 simplify fixtures byte-exact (7 with string comparison waived); 194 wcsconv round-trip cases with the exclusion list empty and gated; 18 native round-trip corpora; 1,606 transform-oracle sections behind three asserting tests plus a proof that the partition covers the corpus. | **Green** |
| 3 · C tests vs Rust libast | The real `ast_tester` programs link the Rust library and pass. | `crates/ast-ffi-tests/src/lib.rs` contains one attribute line. Nothing to run, because `ast-capi` exports nothing. | **Not started** |
| 4 · Bit/ULP oracle | Rust and C linked into one binary and compared value by value. | Same: an empty crate. | **Not started** |
| 5 · Performance gate | Criterion benches fail CI if Rust regresses more than 10% on the hot path. | A six-line placeholder benchmark. Not wired into CI. | **Not started** |

### How C is actually used today

Not by linking.
The port compares against *frozen* C output: 12 MB of committed fixtures, including 41,809 lines of `.oracle` data captured from C AST, plus eight small single-purpose C probe programs under `tools/oracle_fixtures/` built by hand when the C source alone cannot settle a question.
That workflow is written down in `docs/issues/c-oracle-probe-recipe.md`, including the stale-conda-libast trap and how to reach protected APIs.

This has a real virtue the prototype lacked.
The whole suite runs on a fresh clone with no submodule, in seconds, so nothing is gated behind a 16-minute C rebuild.
It also has a real limit: **no test in this repository has ever executed C AST code and compared the result live.**
Every comparison is against a recording, and recordings do not notice when the thing they recorded changes.

### The relaxations, counted honestly

This is the part of a parity claim that usually goes unstated.
Out of 193 wcsconv manifest cases:

- **8 cases** compare at `astEqual` tolerance rather than byte-exact or 4-ULP.
  All are attributed to FMA contraction or libm differences against references captured on x86-64 Linux — and the manifest notes that C AST built on arm64 macOS does not reproduce them either.
- **5 cases** drop `IsSimp` and `Invert` tag lines from both sides: the two C aliasing quirks now being fixed upstream.
- **0 cases** are skipped.
  There is no skip mode; it was deliberately removed.

The transform oracle carries exactly two overrides.
`tnx-cheb`'s round-trip check is disabled because a ChebyMap genuinely cannot represent its own inverse (the derivative of a first-kind Chebyshev `T_n` is a second-kind `U_(n-1)`, which ChebyMap's storage cannot hold).
One UnitNormMap inverse golden is waived at a point where the answer is BAD-or-rounding-noise depending on architecture.
Both carry written explanations.
The simplify corpus waives string comparison on seven of its 427 fixtures, `lsst` and `rigby` among them.

This is a disciplined posture.
The relaxations are few, named in source, individually justified, and mostly attributable to floating-point reproducibility or to C defects rather than to Rust gaps.

### What is absent from the port entirely

**52,168 code lines of C** (133,444 total lines) have no Rust counterpart: `Plot`, `Plot3D` and every `grf` backend, `YamlChan`, `XmlChan`, `Moc` and `MocChan`, `StcsChan`, all `Stc*` regions, `Table` and `FitsTable`, `IntraMap`, `XphMap`.
That is 28% of the C library by code volume, and it is the reason the 70% subsystem figure above and the 44% whole-library figure differ so widely.
Also missing: `astResample`, `astRebin`, `astRebinSeq`, `astTranGrid` and `astQuadApprox`.

Two of these gate work already in flight.
`FitsTable` blocks the remaining `-TAB` branches in both FitsChan and `fset_to_store`.
`IntraMap`, `XphMap` and `StcResourceProfile` hold 24 upstream simplify fixtures out of the active corpus, staged in a separate directory, counted, and enforced by a test that fails if the set changes without a matching entry.

Step 6a, the wcslib port, has not begun.
3,327 code lines of vendored wcslib, 16,626 of ERFA and 1,386 of PAL (plus a 234-line `palwrap.c`) are still compiled and linked into every build.

---

## 7. Forward risks

### Risk 1 — the ABI shim is the project's purpose and has no code

Every correctness result in this report was obtained through the Rust API.
The C contract is entirely unexercised: `ast.h` signatures, `astClone`/`astAnnul` reference counting, `astBegin`/`astEnd` handle stacks, thread-local status, variadic `astSet` parsing, a `catch_unwind` boundary on every entry point, and the two CI drift checks that are supposed to guard the header.
Tiers 3 and 4 cannot start until it exists, which means the single strongest available form of verification is blocked on it.

### Risk 2 — the next C merge changes serialized output in four places, and the corpus predates it

The unmerged `u/timj/misc-bugs` branch alters what C writes in four ways: `IsSimp` placement becomes deterministic, KeyMap dumps become key-ordered, `EncodeFloat` justification changes, and the `WcsNative` PermMap `Invert` flag disappears.
Task 15 on that branch alone regenerated 69 `.native` fixtures.

Two of the four are currently *mirrored* in Rust.
The port faithfully reproduced `EncodeFloat`'s length-preserving exponent-zero strip (`bfacdfe`), and licensed the `IsSimp` aliasing behavior in five wcsconv cases.
When the branch merges, those two go from correct to wrong, and the whole frozen corpus needs regenerating in one coordinated move.
Worth planning before the pin moves rather than discovering after.

### Risk 3 — a tier-2 gate with a hole of known shape

The trailing-whitespace blind spot in `native_round_trip_fixture` is documented in `PLAN.md` §5 along with the second half of the problem: no fixture in any scanned directory contains a comment-less nested-object card, so even a strict comparator would not have caught the bug found on 8 August.
Both halves need fixing together, and until they are, the byte-exactness claim carries a caveat.

### Risk 4 — two latent correctness gaps are being carried deliberately

Polygon's planar rather than geodesic `reg_base_mesh` is the one to watch.
It is correct for every fixture the port currently runs and wrong for sky polygons, and it will surface through overlap or MOC results rather than as an obvious failure.
`PLAN.md` ranks it highest of the seven open region findings.
The other six are gated behind unstarted work (Plot backends, the overlap pass, STC-S) and are appropriately deferred.

### Risk 5 — the performance story is undefended

A three-month-old hand-run snapshot, a CI gate that is documented but does not exist, and a 42× outlier that reproduces across runs.
None of it threatens the named hot path, but the project's original headline argument was speed, and there is currently no mechanism that would notice if that argument stopped being true.
Wiring `ast-oracle-bench` to something — even a handful of shapes at one N — would cost little and restore the claim.

---

## 8. The pattern across four months

The port's best work has the same shape every time.
Read the C function, port it whole, delete whatever heuristic was standing in for it, and write down what C does that looks wrong.
`MakeIntWorld`, `SpecTrans`, `TDBConv`, `astConvertX`, the `MapMerge` rebuild and the `KeyMap` port all follow it, and all produced byte-exact results plus, in several cases, upstream bug reports.

Its worst work has the same shape too.
Build a plausible Rust-shaped structure instead of C's, discover months later that something downstream depended on C's shape, and pay for the rebuild plus every wrong fix made in between.
The simplifier, `Invert` semantics, the 2-axis celestial mapping and the Region sweep are four instances of one mistake.

What is uncomfortable is that the prototype post-mortem had already named that mistake, in that subsystem, and this repository was started specifically with that document in hand.
Knowing a lesson in the abstract did not prevent repeating it.
That suggests the guardrail needs to be mechanical rather than remembered — which is what Appendix A proposes.

Set against that, the direction of travel is good and measurable.
Every one of the prototype's named findings except the ABI and performance has been answered.
Verification is materially stronger than the prototype's and honest about its own relaxations.
And the relationship with the C library has inverted: the port started by consuming C's fixtures and now supplies C with defect reports faster than they can be merged.

---

## Appendix A: what was added to CLAUDE.md and docs/porting-hazards.md

`CLAUDE.md` previously told a session *what the design is* and *what not to do to the API surface*.
It did not encode the failure modes in §4, all of which are process failures rather than design violations — a session could follow every rule in the old document and still make all eight.

That gap is now closed in two places, with a deliberate division of labour.

**`docs/porting-hazards.md`** (new) holds the evidence.
Fourteen hazards drawn from both ports, indexed by **trigger** rather than by
chronology, each with the commits that prove the cost and the check that would
have caught it.
It opens with an enforcement hierarchy — a test that fails beats a `CLAUDE.md`
check, which beats a hazards entry — and asks that any entry which can be moved up
that list should be.

The trigger-indexed form is itself a finding.
The prototype's post-mortem documented the most expensive mistake in this project
and the port then repeated it with that document in the repository, so a narrative
retrospective demonstrably does not transfer.
Of the fourteen entries, three are candidates for conversion into tests (the
comparator-normalisation hole, the perf gate, a script diffing an incoming
upstream range for serialization changes), and two — the `Invert` modelling gap and
the `IsSimp` conflation — will only ever be narrative, which is what the file is
for.

**`CLAUDE.md`** (edited) carries only the imperative, with pointers to the evidence.
Six changes:

| Change | Finding it encodes |
|---|---|
| New section "Before porting a class — find C's extension points": classify each `vtab` entry as ceremony or architecture, with the mechanical test (*does a generic driver in another file dispatch to this?*) and the list of known-architecture methods. Plus the shape rule, the two-names-two-fields rule, and the mutable-state-convention rule | 1, 2, 3, 4 |
| New section "Stop signals": three signals that mean stop patching and read the C source, plus the second-conversion corollary | 5, 6, and the prototype's DSB saga |
| New subsection "Landing several classes in one pass" under the class-adding recipe: how to record sweep debt in the same commit range | 7 |
| New subsection "Comparators and skip modes" under the test-porting recipe: no skip mode, hard-fail, per-case relaxations, and the normalisation rule | 8 |
| Hot-path section amended to state that **no automated performance gate exists**, with the manual procedure and the caveat that the snapshot already records 5–42× regressions off the hot path | The false gate (§5) |
| Four rows added to the "What NOT to do" table; upstream-sync protocol and a hazards-file pointer added to "Important pointers" | 1, 5, 6, 8 and risk 2 |

The one thing deliberately *not* done: no mistake-archaeology was added to source
comments or docstrings.
Code comments describe the code as it is today; the history lives in these two
documents and in §4 above.

## Appendix B: what this review contributed to PLAN.md

`PLAN.md` describes itself as "the single index of active, audited, outstanding work".
It was accurate for everything it covered, but three areas of outstanding work were absent from it entirely, and one was recorded in a form that had gone stale.
Those have been added:

- **A new section 7, "Delivery-step work not yet started"**, covering the `ast-capi` C-ABI shim, tiers 3/4/5, `ast-plugins`, and the wcslib port — with the tier table making explicit that the two strongest verification tiers are blocked on the shim.
- **A new section 8, "Pending upstream sync"**, recording the four output-changing C fixes on `u/timj/misc-bugs`, the two the port currently mirrors, and the regeneration order required when the pin moves.
- **A verification-posture ledger** in section 6, counting the relaxations (8 astequal, 5 aliasing-licensed, 2 oracle overrides, 7 string-compare waivers) so the total cannot grow silently.
- **The performance row in section 2** updated from "SNAPSHOT, opt targets unactioned" to name the missing CI gate as the tracked item, since the absence of the gate is the actionable part.

And the reverse direction: writing the report surfaced two figures that PLAN.md and this document now share — the relaxation ledger, and the code-line density comparison in §1 that identified `fitschan/` as the one subsystem larger than the C it replaces.

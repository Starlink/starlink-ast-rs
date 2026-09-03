# Simplify parity: what is verified, and what is not

Date: 2026-09-03.
C pin: `3e60abaa` (`reference/starlink-ast`, present and checked out).

## The question this answers

Is the simplifier's logic complete against the C library, and is that verified
rather than asserted?

**Yes for everything the corpus reaches, by byte-equality of simplify output,
with four named classes of exclusion and one open design question.**
The rest of this report is what that sentence rests on.

The standard is deliberate.
Earlier reports rested on `astEqual` agreement, and that standard passed while
`MapSplit` and CmpMap's flatten logic were missing entirely: two objects can be
equal without having been built the same way, so `astEqual` cannot see a missing
algorithm.
The standard here is that C's logic exists in Rust in the same conceptual form,
confirmed by byte-equal `.map` and `.simp` output against the C oracle's own
dumps.
`astEqual` compliance is not evidence and is not cited.

## What holds it

Measured on 2026-09-03: `cargo test -p ast-core` is 3384 passed, 0 failed, 1
ignored across 101 result lines, with `cargo fmt --all -- --check` clean and
`cargo clippy --workspace --all-targets -- -D warnings` silent.
The one ignored test is `transform_oracle_report`, an investigative reporter run
with `--ignored`; every gate below runs by default.

| Gate | What it compares | Cases | Exclusions |
|---|---|---:|---|
| `test_simplify_regression` | `astSimplify` output, byte-exact against the oracle's dump | 471 manifest cases, 30 tests | `SERIAL_UNGATED` 0, `ISSIMP_DISAGREEMENTS` 3, `ABANDONED_PROBE_STAMP_LICENSED` 5, `INVAB_ALIASING_LICENSED` 2, `NUMERIC_PLATFORM_DRIFT` 5, negative-fixture `IsSimp` strip 144 |
| `test_transform_oracle` | forward and inverse transforms, and round trips | 1752 sections, 25 tests | all three `*_ORACLE_EXCLUDED` lists empty; `RTRIP_REFERENCE_GAPS_LICENSED` 2 entries (25 rows), `FMA_SUBSTITUTIONS_LICENSED` 2 rows |
| `test_serialisation` | Native serialisation, byte-exact against C's own round trip | 794 simplify fixtures, 139 frameset fixtures, 24 generated; 77 tests | `FRAMESET_FIXTURE_EXCLUDED` 0 |
| `test_wcsconv` | FITS read path against the oracle's FrameSet | 194 | `NATIVE_READ_EXCLUDED` 0, `ISSIMP_PLACEMENT_LICENSED` 3, manifest `astequal` compare mode 8 |
| `test_mapmerge_audit` | every C `MapMerge` / `Simplify` seam has a named cover | 37 checklists, 5 tests | `undecided:` 1 (`cmpframe.c`) |
| `test_audit_citations` | every checklist row's citations resolve, and every C step is covered | 37 checklists, 6 gates | none |
| `test_verification_ledger` | every relaxation in the tree appears in the ledger at its stated size | 15 rows plus 3 manifest counts | none |

`test_simplify_seams` (2 tests), `test_regions` (114), `test_deferred_fixtures`
(1) and `test_frame_in_map_slot` (3) run alongside them; all pass.

Every cluster that changed behaviour carries a revert-check: the fix backed out,
the fixtures confirmed to fail, the fix restored.
Clusters 6 and 7 discharge it differently — cluster 6's two licences each carry
a companion test that fails when the excuse would cover a case the reference does
not also fail, and cluster 7 changed gates rather than behaviour, so each of its
three closed skip modes was made to fail by hand instead.

Three properties make these gates rather than reports:

- **No skip mode.**
  A case that cannot be compared is in a named list with a per-case reason, and
  every list is counted in `PLAN.md` §6 so the total cannot drift.
  `test_simplify_regression`'s three remaining skip modes — a missing manifest, a
  manifest case with no fixture, and a fixture that fails to parse — were closed
  while they read zero, and each was checked by hand to fail.
- **Every gate has a floor.**
  A gate that compared nothing would pass; each asserts a minimum case count, so
  a corpus that stops being read fails.
- **Every licence has a falsifiability control.**
  Each exclusion list has a companion test that fails when an excluded case
  starts passing, or when the excuse would cover a case it was not measured on.

## What was not run, and is not claimed

The submodule is present at pin `3e60abaa`, but `cargo test -p ast-oracle-tests`
executes **zero tests**, and `ast-ffi-tests` likewise: both crates are
documented placeholders (`PLAN.md` §7).
No test in this repository executes C code and compares the result live.
Every C comparison in the table above is against frozen corpora committed under
`crates/ast-core/tests/fixtures/`, captured from the pinned oracle.
A recording cannot notice when the thing it recorded changes, which is why the
corpus-freshness scripts under `tools/oracle_fixtures/` exist and why moving the
pin is a planned operation rather than a bump.

## The audit

`docs/audits/mapmerge/` holds one checklist per C class with a `MapMerge` or
`Simplify` vtab entry: 37 files, 1444 rows, each naming a C step, its C site with
the enclosing function, its Rust site, whether the step is an extension point or
ceremony, and a verdict.

Verdicts are `ported` (1075 rows), `n/a: <reason>` (366) for a step whose C
purpose does not exist in Rust — ambient status, pointer annulment, buffer
marshalling — or `divergence: <quirks citation>` (3) for a step the port refuses
because performing it would reproduce a defect in C's own.
There is no `gap` verdict: the form was removed once the last row left it, so
recording a new gap takes a deliberate change to the gate.
`deferred:`, `driver:` and the spec's reserved `unreachable:` were withdrawn from
the row grammar at the same time, all three unused.

Four gates hold the checklists.
Citations resolve to exactly one file and name the item they claim; architecture
rows land on a trait method; and every C step comment inside a cited body is
covered by a row or named under `Uncited C steps:` with a reason.
Citations are anchored against pin `3e60abaa`; when the pin moves, re-anchor with
`cargo run -p audit-tools --bin anchor-citations -- --write docs/audits/mapmerge/*.md`
and hand-check every row the tool cannot place.
Mechanical remapping by text similarity was tried and relocated 25 rows
confidently wrong.

The extension-point column exists because collapsing a per-class seam into one
central match is the most expensive mistake this port has made, twice (H1 in
`docs/porting-hazards.md`).
1057 rows are classified `architecture` and 387 `ceremony`, and each architecture
row lands on a trait method a generic driver dispatches to.

## What is excluded, and why each is not a hole

Transcribed from `LEDGER` in `crates/ast-core/tests/test_verification_ledger.rs`,
which is the authority; the counts are asserted there.

| Source | Marker | Count | What it is |
|---|---|---:|---|
| `test_wcsconv.rs` | `ISSIMP_PLACEMENT_LICENSED` | 3 | C's reader reduces Mappings as it assembles them, so its cards record the order that reader worked in; this port simplifies once, at the end. Counts pinned on both sides against the pinned oracle; every other card, `Invert` included, is byte-compared |
| `test_wcsconv.rs` | `NATIVE_READ_EXCLUDED` | 0 | no skip mode: kept empty and gated against refilling |
| `test_simplify_regression.rs` | `ABANDONED_PROBE_STAMP_LICENSED` | 5 | one reference-only `IsSimp` card per fixture, stamped by C on a sub-Mapping inside a merge probe it then abandoned. The probe's CmpMaps are assembled by cloning, so the object it marked is one the live tree still holds; owned values give each tree its own children, so no probe in the port can leave a mark that outlives it. Spent positionally, one line per fixture |
| `test_simplify_regression.rs` | `INVAB_ALIASING_LICENSED` | 2 | one node each in `cap_cmpmap_01` and `_04` whose dump carries `Invert` in C and `IsSimp` in the port. C's `CombineMaps` records a shared child's `Invert` on the CmpMap it builds, which is reference-count history rather than a property of the Mapping. Consumed as a pair at one position |
| `test_serialisation.rs` | `FRAMESET_FIXTURE_EXCLUDED` | 0 | every wcsconv object round-trips byte-exact |
| `test_transform_oracle.rs` | `SLAMAP_ORACLE_EXCLUDED` | 0 | no SlaMap oracle section is excluded |
| `test_transform_oracle.rs` | `HEAD_ORACLE_EXCLUDED` | 0 | no head oracle section is excluded |
| `test_transform_oracle.rs` | `SIMPLIFY_ORACLE_EXCLUDED` | 0 | no simplify oracle section is excluded |
| `test_transform_oracle.rs` | `RTRIP_REFERENCE_GAPS_LICENSED` | 2 | two fixtures, 25 rows, where C's own recorded inverse does not recover the pixel either: `tnx-cheb.head` (all 24 rows — a ChebyMap has no usable inverse) and `tsc.head` row 10 (a 360 deg alias). Asserted as an exact set, reached only after the live result has failed both the direct and the aliased comparison |
| `test_transform_oracle.rs` | `FMA_SUBSTITUTIONS_LICENSED` | 2 | two rows of one UnitNormMap inverse section where the generator host's compiler fused the multiply-add at `unitnormmap.c:1079` and stepped past C's own zero-norm branch. The substituted expectation is the unfused value C's source prescribes, and a test fails if the recorded values ever stop differing from it |
| `test_simplify_regression.rs` | `NUMERIC_PLATFORM_DRIFT` | 5 | one inverse-matrix element of `rigby`, the two sky half-widths of `cap_interval_01`'s uncertainty Box, and the two sky coordinates of `cap_pointlist_01`'s re-centred uncertainty corner, whose references were captured on arm64: an x86-64 build of the pinned C does not reproduce their values, the port reproduces its own host's capture to the last digit, and the corpus-freshness rule keeps a cross-platform reference rather than replacing it. Each entry names the reference's value exactly and bounds how far the port's own value may sit from it |
| `test_simplify_regression.rs` | `SERIAL_UNGATED` | 0 | no fixture opts out of the byte comparison: every simplified dump is compared against C's `.simp` in full, and a difference fails |
| `test_simplify_regression.rs` | `ISSIMP_DISAGREEMENTS` | 3 | fixtures whose `IsSimp` card count differs from C's, each by one card C wrote inside a merge probe it then abandoned. The byte comparison licenses all three through `ABANDONED_PROBE_STAMP_LICENSED`, so the count is their only gate — which is why the set is pinned on its own |
| `test_frame_in_map_slot.rs` | `FRAME_IN_MAP_SLOT_UNDECIDED` | 1 | one C-written dump the reader refuses: a bare Frame in a `Map<n>` slot. Whether to support it is an open design question, stated with its artifact in `docs/issues/frame-in-a-mapping-slot.md` |
| `test_deferred_fixtures.rs` | `DEFERRED_CLASSES` | 3 | IntraMap, XphMap and StcResourceProfile are unported classes, not relaxations of a gate that could otherwise pass |
| simplify manifest | `skip_string_compare` | 0 | no skip mode: every row in the manifest is byte-compared, and the counter is gated so a `yes` cannot come back |
| simplify manifest | negative fixtures stripping `IsSimp` | 144 | structural, not a port gap: the reference *is* the input, which was never simplified. Guarded by requiring the result to carry at least one card |
| wcsconv manifest | `astequal` compare mode | 8 | FMA contraction / libm differences against x86-64 Linux captures that C AST on arm64 does not reproduce either |

Four classes of exclusion, all narrow:

1. **Deferred classes** — IntraMap, XphMap, StcResourceProfile.
   Not ported at all, counted in `DEFERRED_CLASSES`, and no fixture claims
   otherwise.
2. **C behaviour that cannot be reproduced from owned values** — the `InvA` /
   `InvB` aliasing dump line in `cap_cmpmap_01` and `cap_cmpmap_04`, and the
   abandoned-probe `IsSimp` stamp, both of which depend on two C pointers being
   the same object.
   Recorded in `docs/issues/c-library-quirks.md`, never worked around in Rust
   code.
3. **C behaviour the port declines to reproduce** — ChebyMap's linear `MapMerge`
   reduction, which mirrors PolyMap's and loses the Chebyshev domain scaling.
   Marked in `ChebyMap::map_merge` and tracked at `PLAN.md` §10.2.
   The fix belongs upstream and is deferred until the pin moves, because upstream
   has reorganised the fixtures.
4. **Reader-order artefacts** — the three `ISSIMP_PLACEMENT_LICENSED` wcsconv
   cases, where the card order depends on the order C's reader happened to
   construct objects in.

Numeric drift is not in that list because it is not a port claim at all: five
values captured on arm64, which an x86-64 build of the pinned C does not
reproduce either.

## The one open design question

A bare Frame in a Mapping slot.
C accepts it; this port makes Region the only Frame that is also a Mapping.
The question, its C citations, the cost of each answer and the artifact are in
`docs/issues/frame-in-a-mapping-slot.md`, and `PLAN.md` §9.6 points there.
It is a question about C's intent, and the corpus supplies no evidence that the
construct is used: every `CmpFrame` in it is a Region's own Frame, and the 33
Region-in-`CmpMap` nestings read today.

## What would falsify this report

- Any of the seven gates failing.
- A ledger row whose count no longer matches the list it names — the ledger test
  checks this, so a drift is a failure rather than a discrepancy.
- A `.map` or `.simp` fixture regenerated from the oracle that no longer matches
  byte-for-byte.
- A new C fixture exercising a simplify path the audit records as `ported`, that
  does not match.
  The audit is a claim about C's logic being present, and the corpus is what
  tests it; a new fixture is the way to attack the claim.
- Tier 3 or tier 4 being implemented and disagreeing with the frozen corpora.

## Handoff

Two items need a human:

- The maintainer question in `docs/issues/frame-in-a-mapping-slot.md`.
- The upstream fixture sync.
  New reference fixtures written by the C oracle during this work are candidates
  for the upstream branch, so upstream's corpus and this one stay in step.
  The sectionless fixtures — the 51 `.map` / `.simp` files lacking a
  transform-oracle section, of which 42 are Rust-only and 9 cannot have one
  (`PLAN.md` §9.4) — resolve on that sync, as does carrying the ChebyMap fix
  upstream.

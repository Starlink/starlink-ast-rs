# What the simplify audit actually found, and how it kept being wrong

Covers 2026-08-21 to 2026-08-23, 34 commits from `03fffeb`.
Written to be read before resuming, because the most useful output of the work is
not the fixes.

## Summary

The task was to close the simplify / `MapMerge` gaps catalogued by a per-class
audit. Five closed. Three of the audit's own conclusions turned out to be wrong,
one of them three times over. The audit was reliable at pointing at *places to
look* and unreliable at saying *what was wrong there*.

Of eleven gaps investigated:

| outcome | count | entries |
|---|---:|---|
| fixed as diagnosed | 2 | §10.8, §10.9 |
| fixed, diagnosis wrong | 3 | §10.1, §10.7, §10.10 |
| retracted, no defect | 2 | §10.6, §10.14 |
| resolved as dead code | 1 | §10.15 |
| narrowed, still open | 1 | §10.3 |
| untouched | 2 | §10.2, §10.11 — plus §10.4, §10.12, §10.13 |

## The one thing to carry forward

**Every diagnosis that turned out to be right came from reading C's source or
watching C run. Every diagnosis that turned out to be wrong came from reading a
fixture's output and inferring a rule.**

§10.14 is the case study. Three successive versions of that entry each inferred a
rule from a dump:

1. "the engine absorbs an inverted CmpMap's direction where C keeps it as a flag"
   — refuted by generating the obvious fixture; C folds it in exactly as the port
   does.
2. "the port reaches deeper than C" — true-sounding, and it never said what stops
   C.
3. "the parallel-pair expansion helpers nominate where C only composes" — refuted
   by reading the helper, which composes exactly as `cmpmap.c:1717` does.

An instrumented C library answered it in one command. The trace showed C
nominating the node everyone assumed it skipped, and entering it at a *different
orientation* — which was the real difference and had not been considered.

The evidence that had misled all three attempts was one card nobody checked: the
node kept `Invert = 1` **and carried no `IsSimp` card**, so C had never simplified
it. A rule had been inferred from a node C left untouched.

## Tools built, and why they matter more than the fixes

- `tools/oracle_fixtures/check_corpus_fresh.sh` — classifies every NATIVE-family
  reference against the pinned C as reproduced, card-only, mixed, numeric or
  structural. Found 24 stale references that nothing else would have.
- `tools/oracle_fixtures/build_simplify_trace.sh` — instruments `astSimplify_`,
  `astMapMerge_` and `astMapList_`, three dispatchers in `mapping.c` that cover
  every class, and logs each nomination's *outcome*.
- `simplify::trace` — the same three events from the port, as `tracing` events
  behind the existing feature, with matching field names so one recipe diffs both.

The instrumentation is permanent now. Re-adding it per question is what let three
wrong diagnoses of §10.14 stand.

## Findings worth more than the fixes

### The corpus was passing for the wrong reasons

Repeatedly. `time_2arg_swapped_cancel` held an MJDTOJD step with a zero derived
offset — C drops it by the no-effect rule, the port dropped it by inverse-pair
cancellation, and both reached the same UnitMap. `cap_cmpmap_01` and
`cap_cmpmap_04` passed only because the port never reduced a sub-Mapping at all.
`unit_parallel_invert_clear` already covered a rule the audit recorded as
unported.

A fixture that passes proves the answer, not the route. Where the route is the
contract — and for `IsSimp` placement it is — a lone case that cannot reach the
answer another way is what discriminates.

### Documentation caused a bug

`RateMap::invert_snapshot` was described as "used for serialization compatibility
with the C library". It is C's `map->invert`, and `MapMerge` reduces the
encapsulated Mapping at *that* orientation (`ratemap.c:710-715`). The simplify
path ignored a field it needed because the comment said the field was about
serialisation. Three attempts at §10.1 failed on it.

### Dump-shaping code accumulates in strata

Four mechanisms pushed an inverted CmpMap's inversion into its structure. Three
were redundant, individually *and* together, and are deleted along with
`invert_into_structure` — about 110 lines. Each carried a note calling it
dump-shaping and measured; **the one that turned out to matter was the one marked
"not independently measured"**.

Each layer had been added to fix a real dump mismatch and none was removed when a
later one subsumed it. A redundant layer is not untidiness: it is a place where a
correct change looks like a regression.

### The port has a seam C does not

The structural finding, and the one with a plan:
[`2026-08-23-simplify-seam-realignment.md`](../superpowers/specs/2026-08-23-simplify-seam-realignment.md).
Three of the defects above are one defect. `simplified()` was introduced in the
first commit that defined the `Mapping` trait, before C's seam structure had been
read; `simplify()` was layered on 196 commits later as a serialisation fix.

## Where §10.3 stands

Narrowed to a single nomination and left open deliberately.

Both engines merge `ShiftMap where=0 series=1 nmap=4` and both report `result=0`;
afterwards C's Mapping list holds four elements and the port's holds three. C then
nominates `WinMap` at index 0 of 4 and merges productively; the port nominates at
index 0 of 3 and declines. Every other class's nominations agree exactly, outcome
for outcome.

Seven approaches to `cap_cmpmap_03` have now been measured and reverted. Stop
signal 3 applies: the eighth should wait for the seam realignment, because six of
the seven were attempts to fix tagging and the counts are downstream of the
merges.

## Corpus and gates

32 fixtures added, all generated from the pinned C by
`ast_tester/gen_simplify_fixtures.c` so the upstream copies are byte-identical.
445 simplify fixtures pass; simplify serialisation failures went 13 → 7.

Three gates earned their keep and should not be relaxed:

- `test_verification_ledger` caught an unrecorded relaxation three times.
- `ISSIMP_DISAGREEMENTS` caught every unintended tag movement, in both directions.
- `test_mapmerge_audit` refuses a class without a checklist, a row without a
  verdict, a `gap` citing a `PLAN.md` section that does not exist, and an orphaned
  checklist. Its gap-citation test was added mid-session and negative-controlled.

## If picking this up cold

1. Read the seam realignment spec. Most of what is left depends on it.
2. Use the tracers before forming a hypothesis. Both sides are instrumented and
   the recipe is in `tools/oracle_fixtures/README.md`.
3. Treat the audit checklists as a map of where to look, not as findings. Nine
   of their conclusions were tested this session and three were wrong.
4. When a fixture passes, ask whether it could pass for the wrong reason before
   concluding the behaviour is ported.

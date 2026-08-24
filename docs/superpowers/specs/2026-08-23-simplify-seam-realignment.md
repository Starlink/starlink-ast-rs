# Realigning the simplify seams with C

**Status:** proposed, not started.
**Supersedes nothing.** Sits under
[`2026-04-20-rust-port-design.md`](2026-04-20-rust-port-design.md), which does not
address this.

## The problem in one paragraph

C has two per-class seams in the simplify machinery; this port has three, and the
extra one corresponds to nothing in C.
Per-class reductions were put in the extra seam, where the nominate loop — C's
only route to them — cannot reach.
Three defects closed in August 2026 were the same defect seen from three angles,
and one is still open for the same reason.
The fix is to delete the extra seam and put every per-class reduction where C
keeps it.

## What C does

Two seams, and only two:

- **`Simplify`** (`mapping.c:16752`), overridden by `CmpMap` and the Region
  family and by nobody else. For every other class the base implementation builds
  a one-element Mapping list and calls
  `astMapMerge( map, 0, 1, &nmap, &map_list, &invert_list )` **repeatedly** until
  nothing changes or the list grows past one (`mapping.c:16871-16883`).
- **`MapMerge`**, overridden per class.

So for a non-CmpMap class, **every reduction C performs lives in `MapMerge`**, and
`astSimplify` reaches it only by nominating the Mapping in a list of one.
`astSimplify_` itself (`mapping.c:24755`) adds two things and no algorithm: the
`IsSimple` short-circuit, and the stamp on whatever `Simplify` returned.

## What the port does

Three seams:

| seam | C counterpart |
|---|---|
| `Mapping::simplify()` (public, `mappings/mod.rs:578`) | `astSimplify` |
| `Mapping::simplified()` (per class, 36 impls) | **none** |
| `Mapping::map_merge()` (per class, 31 impls) | `MapMerge` |

`simplified()` is the port's own invention. Its doc comments call it "the untagged
virtual", which is accurate about what it does and silent about what it
corresponds to — because it corresponds to nothing.

**The decisive measurement is the caller count, not the vtab structure.** C's
`Simplify` slot *is* a per-class dispatch, so it can be made to look like
`simplified()`'s counterpart. It is not one, because it has exactly one caller in
the entire library:

```
$ grep -rn ',Mapping,Simplify)' reference/starlink-ast/src/*.c
mapping.c:24772:      result = (**astMEMBER(this,Mapping,Simplify))( this, status );
```

That call sits inside `astSimplify_`, after the `IsSimple` short-circuit and
before the stamp. Nothing else in C can reach a per-class `Simplify` body, so
every reduction in the library is short-circuited and recorded.

The port's `simplified()` has **65 call sites against `simplify()`'s 8**, across
`frames/`, `regions/`, `fitschan/` and the merge core. The raw virtual is the
default choice and the real entry point is the exception, so 65 sites each decide
for themselves whether a reduction is recorded.

That is the confusion this document is about, and it is worse than a naming
problem: it is why the port emits a different set of `IsSimp` cards from C, and
why no stamping rule has ever aligned them (`PLAN.md` §10.3).

## How this happened

Two commits, 196 apart.

`2b2ada2` (2026-04-27), *Add Mapping trait and PointSet*, introduced
`simplified()` as **the** per-class simplify hook, in the first commit that
defined the `Mapping` trait at all — before anything in this repo had been checked
against `mapping.c`'s `Simplify`/`MapMerge` split.

`9078e86` (2026-05-08), *Serialize IsSimp flag and fix byte-exact serialization
issues*, added `simplify()`. Its own message says what it is:

> Add public `simplify()` method on dyn Mapping that **wraps `simplified()`** and
> sets `do_not_simplify(true)` on the result (equivalent to C's `astSimplify`
> which sets IsSimp on the final output)

and what it was for:

> Reduces serialization mismatches from 116 to 40.

So the public entry point was added as a **serialisation fix**, layered on top of
an invented hook, and the author correctly identified it with `astSimplify` while
leaving the invented hook in place underneath. Neither commit came from reading
C's seam structure; the first predates any such reading and the second was
chasing a dump mismatch.

This is the H1 hazard in `docs/porting-hazards.md` — a per-class `vtab` method
ported at the wrong granularity — with an inversion worth noting: H1 warns about
*collapsing* C's seams into one central `match`. Here a seam was *added*. The
failure mode is identical, because what H1 is really about is the set of seams
differing from C's.

It is also the pattern the `C-ARTIFACT` audit (§10.15) found twice over: a change
made to fix a dump, layered rather than reconciled, and never revisited once the
dump matched.

## What it has cost

| entry | defect | how it presented |
|---|---|---|
| §10.1 | RateMap's encapsulated-Mapping reduction lived in `simplified()` with no `map_merge` | never ran on a nominee inside a list; three attempts, two reverted |
| §10.7 | PcdMap's zero-distortion replacement, identically | same |
| §10.3 | blanket-stamping `simplified()` to fix `IsSimp` counts | over-tags nodes the port produced and C did not, while still missing C's |
| §10.16 | the structural finding itself | — |

§10.3 is still open, narrowed to a single nomination: after both engines merge
`ShiftMap where=0 series=1 nmap=4` and both report `result=0`, C's Mapping list
holds four elements and the port's holds three.

Six classes have a `simplified()` and no `map_merge` at all, so each is a latent
§10.1: `chebymap`, and the Region classes `circle`, `cmpregion`, `ellipse`,
`polygon`, `prism`.

## The proposal

Delete `simplified()`. Give every class one seam, `map_merge`, and give the driver
C's base `Simplify`.

### Step 1 — pin the current behaviour

The corpus is the specification: 445 simplify fixtures, 194 `wcsconv` byte
comparisons, the transform oracle. Nothing below may change any of them except
where a fixture is *wrong*, which means regenerating it from the pinned C and
saying so.

Add the missing fixtures first, from the oracle, for every class whose reduction
currently lives only in `simplified()` — the six above plus any the sweep in
step 2 finds. A reduction with no fixture cannot be moved safely, because nothing
would notice if the move lost it.

### Step 2 — sweep

For each of the 36 `simplified()` impls, classify the body:

- **a reduction C performs in `MapMerge`** — the common case. Moves.
- **a recursion into encapsulated Mappings** (RateMap, TranMap, SwitchMap,
  SelectorMap, NormMap) — C does this inside `MapMerge` too, at the
  construction-time orientation where one is recorded (§10.1).
- **a Region `Simplify`** — C *does* override `Simplify` here, so these keep a
  per-class seam. It should be named for what it is rather than sharing a name
  with the Mapping hook.
- **genuinely nothing** — delete.

`test_mapmerge_audit`'s checklists already record each class's `map_merge` rows,
so the sweep has a written starting point per class.

### Step 3 — give the driver C's base `Simplify`

One function: build a one-element list, call `map_merge` repeatedly until it
reports no change or the list grows past one, then apply the `IsSimple`
short-circuit and the stamp exactly where `astSimplify_` does. Every non-CmpMap
class then reaches its reduction the way C's does, once, from one place.

### Step 4 — move the bodies, one class per commit

Ordered by fixture coverage, not by size: a class whose reduction is pinned by a
fixture moves safely, and one that is not needs step 1 first. `ZoomMap` and
`UnitMap` are the natural first two — small, heavily covered, and both already
have a `map_merge`.

### Step 5 — delete `simplified()` from the trait

At that point the `IsSimp` bookkeeping has one reader and one writer, and
§10.3's stamping question answers itself: the port stamps where `astSimplify_`
stamps, because there is nowhere else left to do it.

## Risks

- **This is the merge core.** Every fixture runs through it. The mitigation is
  step 1 and one class per commit, not care.
- **The `IsSimp` counts will move while the work is in progress**, in both
  directions. `ISSIMP_DISAGREEMENTS` in `test_simplify_regression.rs` pins them as
  a set, so each move must be explained in the commit that causes it. That gate
  is what makes the work auditable; do not relax it.
- **Region `Simplify` is a real C seam and must survive.** The temptation, having
  decided `simplified()` is wrong for Mappings, will be to delete it for Regions
  too. C overrides `Simplify` there.
- **Two classes' reductions are orientation-sensitive** (§10.1's
  `invert_snapshot`). Moving a body without carrying the orientation reproduces
  the bug the move is meant to fix.

## What to read first

- `docs/porting-hazards.md` H1, and this document's inversion of it.
- `PLAN.md` §10.16 for the finding, §10.1 and §10.7 for the two closed instances,
  §10.3 for the open one.
- `tools/oracle_fixtures/README.md` for the two tracers. Every diagnosis in this
  document that turned out to be right came from them, and every one that turned
  out to be wrong came from reading dumps and inferring.

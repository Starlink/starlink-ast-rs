# `MapMerge` audit — NormMap

C source: reference/starlink-ast/src/normmap.c
Rust: crates/starlink-ast/src/mappings/normmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin fe26cc9b
Extension points: classified

Uncited C steps:
- normmap.c:595 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- normmap.c:614 (fn MapMerge) — initialization to silence a compiler warning; the Rust locals are initialized where they are declared

C's body runs `normmap.c:608-738 (fn MapMerge)`: rebuild the NormMap around its encapsulated
Frame if that Frame simplifies; failing that, replace a NormMap over a basic
Frame with a UnitMap; cancel a NormMap against its own inverse; and collapse a
run of identical NormMaps to one, since `astNorm` is idempotent.

`NormMap::map_merge` (`normmap.rs:208-242 (fn map_merge)`) holds the replace-with-simpler step
itself, as C's body does, and hands the neighbor work to
`NormMap::try_merge` through `self_reduce_map_merge`. Both halves of that step
are there, in C's order: the Frame simplification of row 5 first, and the
basic-Frame test of row 6 only when the Frame did not reduce.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | normmap.c:608-609 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | normmap.c:611-612 (fn MapMerge) | -- | architecture | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this NormMap | normmap.c:617-619 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Temporarily set its Invert flag to the requested value | normmap.c:620-623 (fn MapMerge) | -- | architecture | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 5 | Try to simplify the NormMap by simplifying its encapsulated Frame, taking that Frame's class | normmap.c:624-636 (fn MapMerge) | normmap.rs:223-227 (fn map_merge) | architecture | ported |
| 6 | A basic Frame's `astNorm` does nothing, so the NormMap becomes a UnitMap | normmap.c:637-645 (fn MapMerge) | normmap.rs:231-234 (fn map_merge) | architecture | ported |
| 7 | Otherwise: cancel with its own inverse in series, or drop duplicates | normmap.c:646-650 (fn MapMerge) | normmap.rs:244-279 (fn try_merge) | architecture | ported |
| 8 | Start with nothing to cancel against | normmap.c:651-653 (fn MapMerge) | -- | architecture | n/a: the port's pairwise hook is asked about one neighbor at a time, so there is no candidate index to initialize |
| 9 | Consider the lower neighbor | normmap.c:654-656 (fn MapMerge) | mappings/simplify.rs:1597-1606 (fn default_map_merge) | architecture | ported |
| 10 | Require the two Invert flags to be opposite | normmap.c:657-661 (fn MapMerge) | normmap.rs:262-267 (fn try_merge) | architecture | ported |
| 11 | Require the encapsulated Frames to be equal | normmap.c:662-669 (fn MapMerge) | normmap.rs:263 (fn try_merge) | architecture | ported |
| 12 | Likewise consider the upper neighbor | normmap.c:670-683 (fn MapMerge) | mappings/simplify.rs:1584-1593 (fn default_map_merge) | architecture | ported |
| 13 | On a cancel, replace both with UnitMaps and report the lower index | normmap.c:684-693 (fn MapMerge) | normmap.rs:264 (fn try_merge) | architecture | ported |
| 14 | Otherwise look for a run of identical NormMaps following the nominated one | normmap.c:694-698 (fn MapMerge) | normmap.rs:269-276 (fn try_merge) | architecture | ported |
| 15 | Walk forward until the list ends or a non-NormMap is reached | normmap.c:699-706 (fn MapMerge) | -- | architecture | n/a: the run collapses a pair at a time, the driver re-nominating the survivor until a pass makes no change (`mappings/simplify.rs:1855-1914 (fn run_nominate_loop)`) |
| 16 | Require the Invert flags to be equal | normmap.c:707-709 (fn MapMerge) | normmap.rs:262-272 (fn try_merge) | architecture | ported |
| 17 | Require the Frames to be equal | normmap.c:710-713 (fn MapMerge) | normmap.rs:272 (fn try_merge) | architecture | ported |
| 18 | Replace each later NormMap with a UnitMap | normmap.c:714-718 (fn MapMerge) | normmap.rs:273-275 (fn try_merge) | architecture | ported |
| 19 | Report the index of the *first* modified Mapping, so a later change does not overwrite it | normmap.c:719-727 (fn MapMerge) | mappings/simplify.rs:1663-1665 (fn default_map_merge) | architecture | ported |
| 20 | Free resources | normmap.c:728-730 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 21 | Re-instate the nominated NormMap's Invert attribute | normmap.c:731-733 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 22 | Clear the result value if an error occurred | normmap.c:734-736 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 23 | Return the result | normmap.c:737-738 (fn MapMerge) | normmap.rs:241 (fn map_merge) | ceremony | ported |

## Rows 14 to 19 — idempotence, one pair at a time

C walks the whole following run of identical NormMaps and replaces each with a
UnitMap in one pass. The port's `try_merge` collapses an adjacent identical pair
into a single NormMap (`normmap.rs:269-276 (fn try_merge)`), so a run of four reduces in three
passes rather than one. The result is the same because `astNorm` is idempotent,
which is C's own justification for the step.

## Row 6 — the Frame's class is the whole test

C does not ask the Frame whether its `astNorm` is a no-op; it asks whether the
Frame is a basic Frame, and assumes so if it is. The port checks the same thing
by downcasting to `BasicFrame`. Widening it to any Frame whose `norm` happens to
be identity would not be C's rule.

## Rows 5 and 6 — two tests, in this order, each reading a different Frame

C's `class` comes from `astGetClass( smap )` — the *simplified* Frame — so the
two tests cannot be collapsed into one pass. A Frame that reduces to a basic
Frame takes row 5's rebuild on this call and row 6's UnitMap on the next one;
reading the class of the unsimplified Frame instead yields a UnitMap where C
yields a NormMap. `cap_normmap_cmpframe` is what holds the order: its CmpFrame
reduces, so C rebuilds rather than reaching row 6.

Row 5 needs a Frame-level `astSimplify`, which C has because `Simplify` is a
Mapping slot every Frame inherits (`mapping.c:16979 (fn Simplify)`) and `CmpFrame`
(`cmpframe.c:9148 (fn Simplify)`), `FrameSet` and every Region override. `Frame::simplify_frame`
(`frames/mod.rs:736 (fn simplify_frame)`) is the port of that slot, and `None` is what C signals by
handing back the pointer it was given, which is the `smap != map->frame` test at
`normmap.c:631 (fn MapMerge)`.

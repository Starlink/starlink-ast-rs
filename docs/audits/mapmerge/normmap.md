# `MapMerge` audit — NormMap

C source: reference/starlink-ast/src/normmap.c
Rust: crates/ast-core/src/mappings/normmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `normmap.c:603-733`: rebuild the NormMap around its encapsulated
Frame if that Frame simplifies; failing that, replace a NormMap over a basic
Frame with a UnitMap; cancel a NormMap against its own inverse; and collapse a
run of identical NormMaps to one, since `astNorm` is idempotent.

`NormMap::map_merge` (`normmap.rs:192-224`) holds the replace-with-simpler step
itself, as C's body does, and hands the neighbour work to
`NormMap::try_merge` through `self_reduce_map_merge`. Both halves of that step
are there, in C's order: the Frame simplification of row 5 first, and the
basic-Frame test of row 6 only when the Frame did not reduce.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | normmap.c:603-604 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | normmap.c:606-607 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this NormMap | normmap.c:612-614 | -- | n/a: the Rust method already has `&self` |
| 4 | Temporarily set its Invert flag to the requested value | normmap.c:615-618 | -- | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 5 | Try to simplify the NormMap by simplifying its encapsulated Frame, taking that Frame's class | normmap.c:619-631 | normmap.rs:210-214 | ported |
| 6 | A basic Frame's `astNorm` does nothing, so the NormMap becomes a UnitMap | normmap.c:632-640 | normmap.rs:215-219 | ported |
| 7 | Otherwise: cancel with its own inverse in series, or drop duplicates | normmap.c:641-645 | normmap.rs:226-261 | ported |
| 8 | Start with nothing to cancel against | normmap.c:646-648 | -- | n/a: the port's pairwise hook is asked about one neighbour at a time, so there is no candidate index to initialise |
| 9 | Consider the lower neighbour | normmap.c:649-651 | simplify.rs:1155-1250 | ported |
| 10 | Require the two Invert flags to be opposite | normmap.c:652-656 | normmap.rs:244-249 | ported |
| 11 | Require the encapsulated Frames to be equal | normmap.c:657-664 | normmap.rs:245 | ported |
| 12 | Likewise consider the upper neighbour | normmap.c:665-678 | simplify.rs:1155-1250 | ported |
| 13 | On a cancel, replace both with UnitMaps and report the lower index | normmap.c:679-688 | normmap.rs:246 | ported |
| 14 | Otherwise look for a run of identical NormMaps following the nominated one | normmap.c:689-693 | normmap.rs:251-258 | ported |
| 15 | Walk forward until the list ends or a non-NormMap is reached | normmap.c:694-701 | simplify.rs:1155-1250 | ported |
| 16 | Require the Invert flags to be equal | normmap.c:702-704 | normmap.rs:244-254 | ported |
| 17 | Require the Frames to be equal | normmap.c:705-708 | normmap.rs:254 | ported |
| 18 | Replace each later NormMap with a UnitMap | normmap.c:709-713 | normmap.rs:255-257 | ported |
| 19 | Report the index of the *first* modified Mapping, so a later change does not overwrite it | normmap.c:714-722 | simplify.rs:1155-1250 | ported |
| 20 | Free resources | normmap.c:723-725 | -- | n/a: owned values are dropped |
| 21 | Re-instate the nominated NormMap's Invert attribute | normmap.c:726-728 | -- | n/a: nothing was mutated, so nothing is restored |
| 22 | Clear the result value if an error occurred | normmap.c:729-731 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 23 | Return the result | normmap.c:732-733 | normmap.rs:223 | ported |

## Rows 14 to 19 — idempotence, one pair at a time

C walks the whole following run of identical NormMaps and replaces each with a
UnitMap in one pass. The port's `try_merge` collapses an adjacent identical pair
into a single NormMap (`normmap.rs:231-236`), so a run of four reduces in three
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
Mapping slot every Frame inherits (`mapping.c:16853`) and `CmpFrame`
(`cmpframe.c:9018`), `FrameSet` and every Region override. `Frame::simplify_frame`
(`frames/mod.rs:240`) is the port of that slot, and `None` is what C signals by
handing back the pointer it was given, which is the `smap != map->frame` test at
`normmap.c:626`.

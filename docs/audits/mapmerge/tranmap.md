# `MapMerge` audit — TranMap

C source: reference/starlink-ast/src/tranmap.c
Rust: crates/ast-core/src/mappings/tranmap.rs
Audited: 2026-08-24 against pin d0346c0d

C's body runs `tranmap.c:814-1014`. It simplifies the TranMap on its own — an
either/or between normalising an inverted TranMap and simplifying its two
component Mappings, with a third case that collapses a TranMap whose components
are inverses of each other — and then, failing that, merges with a neighbouring
TranMap by composing the two forward components and the two inverse components.

All of that is `TranMap::map_merge` (`tranmap.rs:256-333`); the neighbour
merge's own rule is `TranMap::try_merge` (`tranmap.rs:335-404`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | tranmap.c:814-815 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | tranmap.c:817-818 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this TranMap | tranmap.c:820-822 | -- | n/a: the Rust method already has `&self` |
| 4 | Take the two component Mappings, temporarily restoring their construction Invert values | tranmap.c:823-833 | -- | n/a: the components are stored with the Invert flags they had at construction (`tranmap.rs:83-84`) and the merge never mutates them, so there is nothing to set or put back |
| 5 | Simplify the TranMap on its own | tranmap.c:834-836 | tranmap.rs:262-328 | ported |
| 6 | If the TranMap is inverted, replace it with an equal forward one by inverting and swapping the components, and stop there | tranmap.c:837-850 | tranmap.rs:271-279 | ported |
| 7 | Otherwise try to simplify each component Mapping | tranmap.c:851-855 | tranmap.rs:288-289 | ported |
| 8 | Assume something simplified if either pointer changed | tranmap.c:856-858 | tranmap.rs:290-294 | ported |
| 9 | Build a new TranMap from the simplified components | tranmap.c:859-863 | tranmap.rs:295-299 | ported |
| 10 | Otherwise, if both components are defined in both directions | tranmap.c:864-867 | tranmap.rs:305-309 | ported |
| 11 | Form a series CmpMap of the two components with the second inverted | tranmap.c:868-873 | tranmap.rs:310-313 | ported |
| 12 | If that CmpMap simplifies to a UnitMap the components are equal, so replace the whole TranMap with one of them | tranmap.c:874-887 | tranmap.rs:314-327 | ported |
| 13 | Release resources | tranmap.c:888-897 | -- | n/a: owned values are dropped |
| 14 | Merge with a neighbouring TranMap, in series only and only if nothing changed above | tranmap.c:898-903 | tranmap.rs:330-332 | ported |
| 15 | Require the higher neighbour to be a TranMap | tranmap.c:904-907 | tranmap.rs:335-404 | ported |
| 16 | Take the higher TranMap's components, restoring their construction Invert values | tranmap.c:908-921 | tranmap.rs:335-404 | ported |
| 17 | Resolve which component gives the forward and which the inverse transformation of the lower TranMap, honouring its applied direction | tranmap.c:922-935 | tranmap.rs:335-404 | ported |
| 18 | Do the same for the higher TranMap | tranmap.c:936-947 | tranmap.rs:335-404 | ported |
| 19 | Compose the two forward Mappings into a series CmpMap and simplify it | tranmap.c:948-952 | tranmap.rs:335-404 | ported |
| 20 | Do the same for the two inverse Mappings | tranmap.c:953-956 | tranmap.rs:335-404 | ported |
| 21 | Take the merge only if either composition actually simplified | tranmap.c:957-960 | tranmap.rs:335-404 | ported |
| 22 | Construct a new TranMap from the two simplified compositions | tranmap.c:961-967 | tranmap.rs:335-404 | ported |
| 23 | Free resources and re-instate the higher TranMap's component Invert values | tranmap.c:968-978 | -- | n/a: owned values are dropped and nothing was mutated |
| 24 | Replace the first of the pair with the merged TranMap and set its invert flag | tranmap.c:979-985 | simplify.rs:1422-1430 | ported |
| 25 | Annul the second and shuffle the rest down, clearing the vacated slot | tranmap.c:986-997 | simplify.rs:1422-1430 | ported |
| 26 | Decrement the Mapping count and return the index of the first modified element | tranmap.c:998-1005 | simplify.rs:1422-1430 | ported |
| 27 | Re-instate the nominated TranMap's component Invert values | tranmap.c:1006-1009 | -- | n/a: nothing was mutated, so nothing is restored |
| 28 | Clear the result value if an error occurred | tranmap.c:1010-1012 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 29 | Return the result | tranmap.c:1013-1014 | tranmap.rs:332 | ported |

## Rows 6 and 10 to 12 — all three self-simplification cases are one either/or

C's three cases are mutually exclusive and ordered: an inverted TranMap is
normalised and nothing else is tried; otherwise the components are simplified;
otherwise, and only when both are defined in both directions, the
equal-components collapse is attempted. `map_merge` keeps that order and the
early returns that make it exclusive (`tranmap.rs:271-328`), so a nominated
TranMap reaches the same case C does.

Case 1 reads the applied inversion from the list (`ctx.entry(where_).1`), which
is C's `(*invert_list)[where]`, not the object's own flag: the list holds every
entry in its forward state, so the nominated TranMap is not itself inverted.

## Row 17 — which component is forward depends on the applied direction

A TranMap holds one Mapping for each direction, so its applied inversion decides
which is which. C resolves that before composing (`tranmap.c:922-947`) and the
port does the same at `tranmap.rs:335-404`. Composing without resolving it would
pair a forward component with an inverse one.

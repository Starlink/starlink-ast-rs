# `MapMerge` audit — TranMap

C source: reference/starlink-ast/src/tranmap.c
Rust: crates/starlink-ast/src/mappings/tranmap.rs
Audited: 2026-08-24 against pin d0346c0d
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- tranmap.c:793 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `tranmap.c:818-1018 (fn MapMerge)`. It simplifies the TranMap on its own — an
either/or between normalizing an inverted TranMap and simplifying its two
component Mappings, with a third case that collapses a TranMap whose components
are inverses of each other — and then, failing that, merges with a neighboring
TranMap by composing the two forward components and the two inverse components.

All of that is `TranMap::map_merge` (`tranmap.rs:527-571 (fn map_merge)`); the self-reduction
is `TranMap::reduced_on_own` (`tranmap.rs:353-414 (fn reduced_on_own)`) and the neighbor merge
is `TranMap::merge_with_higher` (`tranmap.rs:283-345 (fn merge_with_higher)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | tranmap.c:818-819 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | tranmap.c:821-822 (fn MapMerge) | -- | architecture | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this TranMap | tranmap.c:824-826 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Take the two component Mappings, temporarily restoring their construction Invert values | tranmap.c:827-837 (fn MapMerge) | -- | architecture | n/a: the components are stored with the Invert flags they had at construction (`tranmap.rs:128-129 (fn new)`) and the merge never mutates them, so there is nothing to set or put back |
| 5 | Simplify the TranMap on its own | tranmap.c:838-840 (fn MapMerge) | tranmap.rs:533-547 (fn map_merge) | architecture | ported |
| 6 | If the TranMap is inverted, replace it with an equal forward one by inverting and swapping the components, and stop there | tranmap.c:841-854 (fn MapMerge) | tranmap.rs:358-365 (fn reduced_on_own) | architecture | ported |
| 7 | Otherwise try to simplify each component Mapping | tranmap.c:855-859 (fn MapMerge) | tranmap.rs:374-375 (fn reduced_on_own) | architecture | ported |
| 8 | Assume something simplified if either pointer changed | tranmap.c:860-862 (fn MapMerge) | tranmap.rs:376-379 (fn reduced_on_own) | architecture | ported |
| 9 | Build a new TranMap from the simplified components | tranmap.c:863-867 (fn MapMerge) | tranmap.rs:381-384 (fn reduced_on_own) | architecture | ported |
| 10 | Otherwise, if both components are defined in both directions | tranmap.c:868-871 (fn MapMerge) | tranmap.rs:390-394 (fn reduced_on_own) | architecture | ported |
| 11 | Form a series CmpMap of the two components with the second inverted | tranmap.c:872-877 (fn MapMerge) | tranmap.rs:395-397 (fn reduced_on_own) | architecture | ported |
| 12 | If that CmpMap simplifies to a UnitMap the components are equal, so replace the whole TranMap with one of them | tranmap.c:878-891 (fn MapMerge) | tranmap.rs:398-412 (fn reduced_on_own) | architecture | ported |
| 13 | Release resources | tranmap.c:892-901 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 14 | Merge with a neighboring TranMap, in series only and only if nothing changed above | tranmap.c:902-907 (fn MapMerge) | tranmap.rs:549-570 (fn map_merge) | architecture | ported |
| 15 | Require the higher neighbor to be a TranMap | tranmap.c:908-911 (fn MapMerge) | tranmap.rs:553-555 (fn map_merge) | architecture | ported |
| 16 | Take the higher TranMap's components, restoring their construction Invert values | tranmap.c:912-925 (fn MapMerge) | tranmap.rs:303-311 (fn merge_with_higher) | architecture | ported |
| 17 | Resolve which component gives the forward and which the inverse transformation of the lower TranMap, honoring its applied direction | tranmap.c:926-939 (fn MapMerge) | tranmap.rs:289-300 (fn merge_with_higher) | architecture | ported |
| 18 | Do the same for the higher TranMap | tranmap.c:940-951 (fn MapMerge) | tranmap.rs:302-311 (fn merge_with_higher) | architecture | ported |
| 19 | Compose the two forward Mappings into a series CmpMap and simplify it | tranmap.c:952-956 (fn MapMerge) | tranmap.rs:319-321 (fn merge_with_higher) | architecture | ported |
| 20 | Do the same for the two inverse Mappings | tranmap.c:957-960 (fn MapMerge) | tranmap.rs:323-327 (fn merge_with_higher) | architecture | ported |
| 21 | Take the merge only if either composition actually simplified | tranmap.c:961-964 (fn MapMerge) | tranmap.rs:335-338 (fn merge_with_higher) | architecture | ported |
| 22 | Construct a new TranMap from the two simplified compositions | tranmap.c:965-971 (fn MapMerge) | tranmap.rs:340-341 (fn merge_with_higher) | architecture | ported |
| 23 | Free resources and re-instate the higher TranMap's component Invert values | tranmap.c:972-982 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped and nothing was mutated |
| 24 | Replace the first of the pair with the merged TranMap and set its invert flag | tranmap.c:983-989 (fn MapMerge) | tranmap.rs:560 (fn map_merge) | architecture | ported |
| 25 | Annul the second and shuffle the rest down, clearing the vacated slot | tranmap.c:990-1001 (fn MapMerge) | tranmap.rs:560 (fn map_merge) | architecture | ported |
| 26 | Decrement the Mapping count and return the index of the first modified element | tranmap.c:1002-1009 (fn MapMerge) | tranmap.rs:561 (fn map_merge) | architecture | ported |
| 27 | Re-instate the nominated TranMap's component Invert values | tranmap.c:1010-1013 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 28 | Clear the result value if an error occurred | tranmap.c:1014-1016 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 29 | Return the result | tranmap.c:1017-1018 (fn MapMerge) | tranmap.rs:570 (fn map_merge) | ceremony | ported |

## Rows 6 and 10 to 12 — all three self-simplification cases are one either/or

C's three cases are mutually exclusive and ordered: an inverted TranMap is
normalized and nothing else is tried; otherwise the components are simplified;
otherwise, and only when both are defined in both directions, the
equal-components collapse is attempted. `reduced_on_own` keeps that order and the
early returns that make it exclusive (`tranmap.rs:358-413 (fn reduced_on_own)`), so a nominated
TranMap reaches the same case C does.

Case 1 reads the applied inversion from the list (`ctx.entry(where_).1`), which
is C's `(*invert_list)[where]`, not the object's own flag: the list holds every
entry in its forward state, so the nominated TranMap is not itself inverted.

## Row 17 — which component is forward depends on the applied direction

A TranMap holds one Mapping for each direction, so its applied inversion decides
which is which. C resolves that before composing (`tranmap.c:926-951 (fn MapMerge)`) and the
port does the same at `tranmap.rs:289-311 (fn merge_with_higher)`. Composing without resolving it would
pair a forward component with an inverse one.

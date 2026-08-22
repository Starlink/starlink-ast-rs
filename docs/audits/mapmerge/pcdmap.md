# `MapMerge` audit — PcdMap

C source: reference/starlink-ast/src/pcdmap.c
Rust: crates/ast-core/src/mappings/pcdmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `pcdmap.c:1406-1736`: replace a zero-distortion PcdMap with a
UnitMap; merge directly with its own inverse or a UnitMap; swap toward a distant
merge partner; swap speculatively; and decline everything in parallel.

`PcdMap::map_merge` (`pcdmap.rs:255-262`) delegates to `simplify::pcd_map_merge`
(`simplify.rs:2719-2733`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | pcdmap.c:1406-1407 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | pcdmap.c:1409-1410 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | See if the PcdMap can be replaced by something simpler, without reference to its neighbours | pcdmap.c:1417-1419 | pcdmap.rs:225-236 | gap: PLAN.md §10.7 |
| 4 | A zero distortion coefficient means the PcdMap is a UnitMap | pcdmap.c:1420-1423 | pcdmap.rs:230-232 | gap: PLAN.md §10.7 |
| 5 | Replace it in the list with a forward UnitMap and return the index | pcdmap.c:1424-1433 | -- | gap: PLAN.md §10.7 |
| 6 | Otherwise store the neighbouring Mappings' classes | pcdmap.c:1434-1441 | simplify.rs:1155-1250 | ported |
| 7 | In series, look for a neighbour the PcdMap can merge with | pcdmap.c:1442-1469 | simplify.rs:2732 | ported |
| 8 | Create the merged Mapping | pcdmap.c:1470-1473 | pcdmap.rs:264-320 | ported |
| 9 | A PcdMap neighbour must be this one's inverse, and the pair becomes a UnitMap | pcdmap.c:1474-1479 | pcdmap.rs:276-289 | ported |
| 10 | A UnitMap neighbour merges to a clone of the nominated PcdMap | pcdmap.c:1480-1486 | pcdmap.rs:290-300 | ported |
| 11 | Replace the first of the pair with the result, annul the second, close the gap and return the first index | pcdmap.c:1487-1514 | simplify.rs:1155-1250 | ported |
| 12 | If no direct merge, consider swapping the PcdMap toward a Mapping it could merge with | pcdmap.c:1515-1525 | simplify.rs:2725-2729 | ported |
| 13 | Set a flag for whether the higher neighbour can be swapped with | pcdmap.c:1526-1535 | simplify.rs:2751-2756 | ported |
| 14 | Walk forward counting steps to the nearest mergeable target | pcdmap.c:1536-1567 | simplify.rs:2751-2756 | ported |
| 15 | Do the same walking backward | pcdmap.c:1568-1599 | simplify.rs:2758-2765 | ported |
| 16 | Choose the neighbour so the PcdMap moves toward the nearer target | pcdmap.c:1600-1613 | simplify.rs:2742-2790 | ported |
| 17 | With a target present, swap the pair and then merge them | pcdmap.c:1614-1650 | simplify.rs:2742-2790 | ported |
| 18 | With no target, swap speculatively with each neighbour in turn | pcdmap.c:1651-1691 | simplify.rs:2742-2790 | ported |
| 19 | Take copies of the Mapping and Invert arrays so the supplied values are not disturbed | pcdmap.c:1678-1684 | -- | n/a: `pcd_swap_at` mutates the list in place and reports whether it changed it, so there is nothing to copy back |
| 20 | If neither swapped Mapping simplifies further, discard the swap | pcdmap.c:1692-1702 | simplify.rs:2742-2790 | ported |
| 21 | Otherwise store the swapped pair and report the first index | pcdmap.c:1703-1728 | simplify.rs:2727-2728 | ported |
| 22 | In parallel, a PcdMap combines with nothing | pcdmap.c:1729-1734 | pcdmap.rs:271-273 | ported |
| 23 | Return the result | pcdmap.c:1735-1736 | simplify.rs:2732 | ported |

## Rows 3 to 5 — the same unreachable self-reduction as RateMap

`PcdMap::simplified` turns a zero-distortion PcdMap into a UnitMap, and that is
C's first step. But `pcd_map_merge` goes straight to the swap and then to
`default_map_merge`, neither of which consults `simplified()`, so the
replacement never happens to a PcdMap nominated from inside a list.
`self_reduce_map_merge` (`simplify.rs:1137`) is the shape that would reach it.
See PLAN.md §10.7.

## Row 22 — declining in parallel is a step, not an omission

C states explicitly that PcdMaps cannot combine in parallel with anything. The
port's `try_merge` returns `None` for `!series` before looking at the neighbour
at all, which is the same rule rather than an unimplemented branch.

# `MapMerge` audit — PcdMap

C source: reference/starlink-ast/src/pcdmap.c
Rust: crates/ast-core/src/mappings/pcdmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `pcdmap.c:1406-1736`: replace a zero-distortion PcdMap with a
UnitMap; merge directly with its own inverse or a UnitMap; swap toward a distant
merge partner; swap speculatively; and decline everything in parallel.

`PcdMap::map_merge` (`pcdmap.rs:241-248`) delegates to `simplify::pcd_map_merge`
(`mappings/simplify.rs:2896-2921`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | pcdmap.c:1406-1407 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | pcdmap.c:1409-1410 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | See if the PcdMap can be replaced by something simpler, without reference to its neighbours | pcdmap.c:1417-1419 | mappings/simplify.rs:2902-2911 | ported |
| 4 | A zero distortion coefficient means the PcdMap is a UnitMap | pcdmap.c:1420-1423 | mappings/simplify.rs:2907 | ported |
| 5 | Replace it in the list with a forward UnitMap and return the index | pcdmap.c:1424-1433 | mappings/simplify.rs:2908-2910 | ported |
| 6 | Otherwise store the neighbouring Mappings' classes | pcdmap.c:1434-1441 | mappings/simplify.rs:1151-1246 | ported |
| 7 | In series, look for a neighbour the PcdMap can merge with | pcdmap.c:1442-1469 | mappings/simplify.rs:2920 | ported |
| 8 | Create the merged Mapping | pcdmap.c:1470-1473 | pcdmap.rs:250-284 | ported |
| 9 | A PcdMap neighbour must be this one's inverse, and the pair becomes a UnitMap | pcdmap.c:1474-1479 | pcdmap.rs:261-273 | ported |
| 10 | A UnitMap neighbour merges to a clone of the nominated PcdMap | pcdmap.c:1480-1486 | pcdmap.rs:275-281 | ported |
| 11 | Replace the first of the pair with the result, annul the second, close the gap and return the first index | pcdmap.c:1487-1514 | mappings/simplify.rs:1151-1246 | ported |
| 12 | If no direct merge, consider swapping the PcdMap toward a Mapping it could merge with | pcdmap.c:1515-1525 | mappings/simplify.rs:2913-2918 | ported |
| 13 | Set a flag for whether the higher neighbour can be swapped with | pcdmap.c:1526-1535 | mappings/simplify.rs:2939-2943 | ported |
| 14 | Walk forward counting steps to the nearest mergeable target | pcdmap.c:1536-1567 | mappings/simplify.rs:2939-2943 | ported |
| 15 | Do the same walking backward | pcdmap.c:1568-1599 | mappings/simplify.rs:2946-2954 | ported |
| 16 | Choose the neighbour so the PcdMap moves toward the nearer target | pcdmap.c:1600-1613 | mappings/simplify.rs:2956-2969 | ported |
| 17 | With a target present, swap the pair and then merge them | pcdmap.c:1614-1650 | mappings/simplify.rs:2956-2969 | ported |
| 18 | With no target, swap speculatively with each neighbour in turn | pcdmap.c:1651-1691 | mappings/simplify.rs:2956-2969 | ported |
| 19 | Take copies of the Mapping and Invert arrays so the supplied values are not disturbed | pcdmap.c:1678-1684 | -- | n/a: `pcd_swap_at` mutates the list in place and reports whether it changed it, so there is nothing to copy back |
| 20 | If neither swapped Mapping simplifies further, discard the swap | pcdmap.c:1692-1702 | mappings/simplify.rs:2956-2969 | ported |
| 21 | Otherwise store the swapped pair and report the first index | pcdmap.c:1703-1728 | mappings/simplify.rs:2960-2966 | ported |
| 22 | In parallel, a PcdMap combines with nothing | pcdmap.c:1729-1734 | pcdmap.rs:257-259 | ported |
| 23 | Return the result | pcdmap.c:1735-1736 | mappings/simplify.rs:2920 | ported |

## Rows 3 to 5 — the reduction, and the ordering around it

`pcd_map_merge` turns a zero-distortion PcdMap into a UnitMap at its head
(`mappings/simplify.rs:2902-2911`), before the swap, where C has it. `pcdmap.c` has no
`Simplify` vtab slot, so this is the only place the reduction exists and a lone
PcdMap reaches it the way C does — the base `Simplify` nominates it in a list of
one (`mapping.c:16871-16883`).

C makes the reduction the `if` of an if/else whose `else` holds the entire
neighbour search, so a PcdMap that reduces looks at no neighbour in the same
pass; the early return is that structure. It also clears the entry's `Invert`
flag along with the replacement (`pcdmap.c:1429`), which is why the new pair is
stored with both flags false rather than inheriting the entry's.

`pcd_zero_disc_series` and `pcd_zero_disc_lone` are the fixtures. The lone case
passed before the fix and the series case did not, which is what identified the
seam rather than the arithmetic as what was missing.

## Row 22 — declining in parallel is a step, not an omission

C states explicitly that PcdMaps cannot combine in parallel with anything. The
port's `try_merge` returns `None` for `!series` before looking at the neighbour
at all, which is the same rule rather than an unimplemented branch.

# `MapMerge` audit — IntraMap

C source: reference/starlink-ast/src/intramap.c
Rust: crates/ast-core/src/mappings/intramap.rs
Audited: 2026-10-08 against pin 3b9dc639
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- intramap.c:1493 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- intramap.c:1515 (fn MapMerge) — fetching the thread-specific globals, which hold the registered functions; the Rust IntraMap holds its transformation itself

C's body runs `intramap.c:1509-1630 (fn MapMerge)`: in series, merge this
IntraMap with the next Mapping when that is an IntraMap of the same
transformation function and IntraFlag, the pair keeps the number of
coordinates, and the function's flags say the direction pair is the
identity (SIMPFI for forward then inverse, SIMPIF for inverse then forward);
the pair becomes a UnitMap.

`IntraMap::map_merge` (`intramap.rs:527-564 (fn map_merge)`) is the whole of it.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result | intramap.c:1509-1510 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status | intramap.c:1512-1513 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Further initialisation | intramap.c:1518-1521 (fn MapMerge) | -- | ceremony | n/a: the Rust locals are initialised where they are declared |
| 4 | Only IntraMaps in series, merging the nominated IntraMap with the Mapping that follows it | intramap.c:1523-1526 (fn MapMerge) | intramap.rs:533-535 (fn map_merge) | architecture | ported |
| 5 | Obtain the indices of the two potential IntraMaps and a pointer to the first | intramap.c:1528-1532 (fn MapMerge) | intramap.rs:536-538 (fn map_merge) | architecture | ported |
| 6 | Determine if the second Mapping is an IntraMap and obtain a pointer to it | intramap.c:1534-1540 (fn MapMerge) | intramap.rs:539 (fn map_merge) | architecture | ported |
| 7 | Check that the two IntraMaps use the same transformation function and IntraFlag | intramap.c:1542-1546 (fn MapMerge) | intramap.rs:540-545 (fn map_merge) | architecture | ported |
| 8 | Determine the number of input coordinates of the first IntraMap in the requested orientation | intramap.c:1548-1559 (fn MapMerge) | intramap.rs:548 (fn map_merge) | architecture | ported |
| 9 | Similarly, the number of output coordinates of the second | intramap.c:1561-1570 (fn MapMerge) | intramap.rs:549 (fn map_merge) | architecture | ported |
| 10 | Check that applying the two IntraMaps preserves the number of coordinates | intramap.c:1572-1574 (fn MapMerge) | intramap.rs:550-552 (fn map_merge) | architecture | ported |
| 11 | Forward then inverse can be simplified with SIMPFI, inverse then forward with SIMPIF | intramap.c:1576-1590 (fn MapMerge) | intramap.rs:553-561 (fn map_merge) | architecture | ported |
| 12 | If the two IntraMaps can be simplified, create a UnitMap to replace them | intramap.c:1592-1605 (fn MapMerge) | intramap.rs:562 (fn map_merge) | architecture | ported |
| 13 | Close the gap, clear the vacated elements and decrement the Mapping count | intramap.c:1607-1620 (fn MapMerge) | -- | ceremony | n/a: `MergeCtx::replace_pair_with` keeps the list compact itself |
| 14 | Return the index of the first modified element | intramap.c:1621 (fn MapMerge) | intramap.rs:563 (fn map_merge) | ceremony | ported |
| 15 | If an error occurred, clear the returned result | intramap.c:1626-1627 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`; a merge cannot fail part-way |
| 16 | Return the result | intramap.c:1629-1630 (fn MapMerge) | intramap.rs:563 (fn map_merge) | ceremony | ported |

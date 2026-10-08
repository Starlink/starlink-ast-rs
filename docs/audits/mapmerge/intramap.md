# `MapMerge` audit — IntraMap

C source: reference/starlink-ast/src/intramap.c
Rust: crates/ast-core/src/mappings/intramap.rs
Audited: 2026-10-08 against pin 3b9dc639
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- intramap.c:1489 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- intramap.c:1511 (fn MapMerge) — fetching the thread-specific globals, which hold the registered functions; the Rust IntraMap holds its transformation itself

C's body runs `intramap.c:1505-1626 (fn MapMerge)`: in series, merge this
IntraMap with the next Mapping when that is an IntraMap of the same
transformation function and IntraFlag, the pair keeps the number of
coordinates, and the function's flags say the direction pair is the
identity (SIMPFI for forward then inverse, SIMPIF for inverse then forward);
the pair becomes a UnitMap.

`IntraMap::map_merge` (`intramap.rs:385-422 (fn map_merge)`) is the whole of it.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result | intramap.c:1505-1506 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status | intramap.c:1508-1509 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Further initialisation | intramap.c:1514-1517 (fn MapMerge) | -- | ceremony | n/a: the Rust locals are initialised where they are declared |
| 4 | Only IntraMaps in series, merging the nominated IntraMap with the Mapping that follows it | intramap.c:1519-1522 (fn MapMerge) | intramap.rs:391-393 (fn map_merge) | architecture | ported |
| 5 | Obtain the indices of the two potential IntraMaps and a pointer to the first | intramap.c:1524-1528 (fn MapMerge) | intramap.rs:394-396 (fn map_merge) | architecture | ported |
| 6 | Determine if the second Mapping is an IntraMap and obtain a pointer to it | intramap.c:1530-1536 (fn MapMerge) | intramap.rs:397 (fn map_merge) | architecture | ported |
| 7 | Check that the two IntraMaps use the same transformation function and IntraFlag | intramap.c:1538-1542 (fn MapMerge) | intramap.rs:398-403 (fn map_merge) | architecture | ported |
| 8 | Determine the number of input coordinates of the first IntraMap in the requested orientation | intramap.c:1544-1555 (fn MapMerge) | intramap.rs:406 (fn map_merge) | architecture | ported |
| 9 | Similarly, the number of output coordinates of the second | intramap.c:1557-1566 (fn MapMerge) | intramap.rs:407 (fn map_merge) | architecture | ported |
| 10 | Check that applying the two IntraMaps preserves the number of coordinates | intramap.c:1568-1570 (fn MapMerge) | intramap.rs:408-410 (fn map_merge) | architecture | ported |
| 11 | Forward then inverse can be simplified with SIMPFI, inverse then forward with SIMPIF | intramap.c:1572-1586 (fn MapMerge) | intramap.rs:411-419 (fn map_merge) | architecture | ported |
| 12 | If the two IntraMaps can be simplified, create a UnitMap to replace them | intramap.c:1588-1601 (fn MapMerge) | intramap.rs:420 (fn map_merge) | architecture | ported |
| 13 | Close the gap, clear the vacated elements and decrement the Mapping count | intramap.c:1603-1616 (fn MapMerge) | -- | ceremony | n/a: `MergeCtx::replace_pair_with` keeps the list compact itself |
| 14 | Return the index of the first modified element | intramap.c:1617 (fn MapMerge) | intramap.rs:421 (fn map_merge) | ceremony | ported |
| 15 | If an error occurred, clear the returned result | intramap.c:1622-1623 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`; a merge cannot fail part-way |
| 16 | Return the result | intramap.c:1625-1626 (fn MapMerge) | intramap.rs:421 (fn map_merge) | ceremony | ported |

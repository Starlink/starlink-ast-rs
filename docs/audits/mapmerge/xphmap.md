# `MapMerge` audit — XphMap

C source: reference/starlink-ast/src/xphmap.c
Rust: crates/ast-core/src/mappings/xphmap.rs
Audited: 2026-10-07 against pin 3b9dc639
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- xphmap.c:502 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- xphmap.c:519-521 (fn MapMerge) — a note that an HPX12 XphMap is not a UnitMap, which explains why there is no step replacing one; the Rust method's doc carries it

C's body runs `xphmap.c:513-606 (fn MapMerge)`: in series only, find an
XphMap next to this one, the previous Mapping first, and replace the pair
with a 2-D UnitMap when, in the orientations the merge list gives them, they
are equal and opposite.

`XphMap::map_merge` (`xphmap.rs:364-394 (fn map_merge)`) is the whole of it;
no neighbour's `try_merge` is involved.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result | xphmap.c:513-514 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status | xphmap.c:516-517 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Handle the case where the Mappings are connected in series | xphmap.c:523-526 (fn MapMerge) | xphmap.rs:370-372 (fn map_merge) | architecture | ported |
| 4 | Look for an adjacent XphMap, the previous Mapping first | xphmap.c:528-546 (fn MapMerge) | xphmap.rs:373-380 (fn map_merge) | architecture | ported |
| 5 | If one was found, check to see if the two adjacent XphMaps cancel out | xphmap.c:548-549 (fn MapMerge) | xphmap.rs:374-380 (fn map_merge) | architecture | ported |
| 6 | Ensure they have the required Invert flags | xphmap.c:551-556 (fn MapMerge) | xphmap.rs:381-386 (fn map_merge) | architecture | ported |
| 7 | They cancel out if they are equal and opposite: invert one and compare them for equality | xphmap.c:558-562 (fn MapMerge) | xphmap.rs:387-391 (fn map_merge) | architecture | ported |
| 8 | Reinstate the original Invert flags | xphmap.c:564-566 (fn MapMerge) | -- | ceremony | n/a: the comparison works on oriented copies, so the merge list's Mappings are never mutated |
| 9 | Replace the two XphMaps with a UnitMap if they cancel | xphmap.c:568-581 (fn MapMerge) | xphmap.rs:392 (fn map_merge) | architecture | ported |
| 10 | Close the gap, clear the vacated elements and decrement the Mapping count | xphmap.c:583-596 (fn MapMerge) | -- | ceremony | n/a: `MergeCtx::replace_pair_with` keeps the list compact itself |
| 11 | Return the index of the first modified element | xphmap.c:597 (fn MapMerge) | xphmap.rs:393 (fn map_merge) | ceremony | ported |
| 12 | If an error occurred, clear the returned result | xphmap.c:602-603 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`; a merge cannot fail part-way |
| 13 | Return the result | xphmap.c:605-606 (fn MapMerge) | xphmap.rs:393 (fn map_merge) | ceremony | ported |

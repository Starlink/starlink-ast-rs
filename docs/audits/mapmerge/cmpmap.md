# `MapMerge` audit — CmpMap

C source: reference/starlink-ast/src/cmpmap.c
Rust: crates/ast-core/src/mappings/cmpmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `cmpmap.c:1410-2125` in five sections, named by its own banner
comments: simplify the CmpMap on its own; decompose a series CmpMap into the
list; merge adjacent CmpMaps; update the list for that merge; and swap a series
CmpMap past a neighbouring PermMap.

The Rust `CmpMap::map_merge` (`cmpmap.rs:922-928`) delegates to
`simplify::cmp_map_merge` (`simplify.rs:719-802`), so Rust sites below are in
`simplify.rs` unless stated.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned index to -1 | cmpmap.c:1410-1411 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | cmpmap.c:1413-1414 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Simplify the CmpMap on its own: obtain a pointer to the nominated CmpMap | cmpmap.c:1416-1421 | simplify.rs:725-730 | ported |
| 4 | Note whether the Invert attribute is set, and its value | cmpmap.c:1422-1426 | simplify.rs:726 | ported |
| 5 | Change Invert to the value the list requests, so simplification can absorb it | cmpmap.c:1427-1435 | simplify.rs:730-733 | ported |
| 6 | Simplify the Mapping and read the new Invert value | cmpmap.c:1436-1439 | simplify.rs:737,747 | ported |
| 7 | Restore the original Mapping's Invert attribute | cmpmap.c:1440-1449 | -- | n/a: `oriented` is a clone, so the nominee is never mutated and there is nothing to restore |
| 8 | Determine whether simplification occurred, by pointer identity or a changed Invert | cmpmap.c:1450-1457 | simplify.rs:734,746 | ported |
| 9 | On success, replace the list entry with the new Mapping and its invert flag, and return | cmpmap.c:1458-1468 | simplify.rs:757-765 | ported |
| 10 | Otherwise annul the new Mapping pointer | cmpmap.c:1469-1471 | -- | n/a: owned values are dropped |
| 11 | If a series CmpMap is being combined in series, replace it in the list with its two components | cmpmap.c:1473-1527 | simplify.rs:3397 | ported |
| 12 | If no simplification was done, look for a neighbouring CmpMap to merge with | cmpmap.c:1528-1536 | simplify.rs:784 | ported |
| 13 | Obtain the pair of indices to merge, taking (0,1) when the nominee is first | cmpmap.c:1537-1547 | simplify.rs:785-786 | ported |
| 14 | Require the neighbour to be a CmpMap, and obtain both pointers and their invert flags | cmpmap.c:1548-1576 | simplify.rs:787-793 | ported |
| 15 | Series CmpMaps in parallel: re-arrange the sub-Mappings into two parallel CmpMaps and simplify each | cmpmap.c:1577-1623 | simplify.rs:792 | ported |
| 16 | Parallel CmpMaps in series: divide both into aligned sub-lists, combine each pair in series and simplify | cmpmap.c:1624-1783 | simplify.rs:789-790 | ported |
| 17 | Update the list: annul the pair, insert the replacement, close the gap and return the first modified index | cmpmap.c:1784-1815 | simplify.rs:795-797 | ported |
| 18 | If merging in series and the previous Mapping is a PermMap, try swapping the CmpMap in front of it | cmpmap.c:1816-1845 | -- | gap: PLAN.md §10.4 |
| 19 | Read both Mappings' Invert attributes and set them to the requested values | cmpmap.c:1846-1858 | -- | gap: PLAN.md §10.4 |
| 20 | Read the shapes of the PermMap and of the CmpMap's two components | cmpmap.c:1855-1871 | -- | gap: PLAN.md §10.4 |
| 21 | Require the PermMap outputs feeding each component to come from a contiguous input block | cmpmap.c:1872-1912 | -- | gap: PLAN.md §10.4 |
| 22 | Transform a set of bad inputs through the PermMap to recover its constant outputs | cmpmap.c:1927-1950 | simplify.rs:1002 | ported |
| 23 | Substitute a UnitMap for a component whose inputs are all constant | cmpmap.c:1951-1966 | -- | gap: PLAN.md §10.4 |
| 24 | Build the replacement PermMap's permutation and constant arrays for both components | cmpmap.c:1967-2043 | -- | gap: PLAN.md §10.4 |
| 25 | Create the new PermMap and the new CmpMap | cmpmap.c:2044-2068 | -- | gap: PLAN.md §10.4 |
| 26 | Re-instate the component and neighbour Invert attributes | cmpmap.c:2069-2095 | -- | n/a: nothing was mutated, so nothing is restored |
| 27 | On a successful swap, store the new CmpMap and PermMap in the two slots and return the first index | cmpmap.c:2096-2119 | -- | gap: PLAN.md §10.4 |
| 28 | Clear the result value if an error occurred | cmpmap.c:2120-2122 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 29 | Return the result | cmpmap.c:2123-2124 | simplify.rs:801 | ported |

## Row 11 — the same decomposition, a different seam

C splices the two components into the Mapping list from inside `MapMerge`,
growing the list by one. The port decomposes before the nominate loop instead,
in `flatten` (`simplify.rs:231-310`, called at `simplify.rs:3397`), so by the
time a CmpMap is nominated the list is already flat. The reduction is the same;
what differs is that C's version reports a change and restarts the loop, while
the port's happens once up front.

## Rows 18 to 27 — only the PermMap side of the swap exists

C implements this swap twice, once for each nominee: here, when the CmpMap is
nominated and the PermMap is its lower neighbour, and again in
`permmap.c::MapMerge` when the PermMap is nominated. The port has only the
second, as `perm_map_merge` (`simplify.rs:1010-1030`), which looks at the
`(where_, where_ + 1)` pair alone. Row 22 is marked ported because the port
borrows exactly this section's constant-transform arithmetic, cited at
`simplify.rs:1002`.

Because the nominate loop offers every element in turn, a `[PermMap, CmpMap]`
pair does get swapped — when the PermMap's turn comes, not the CmpMap's. What
differs is which index the merge reports, and therefore where the loop resumes.
See PLAN.md §10.4.

## The four under-tagging fixtures

Rows 3 to 9 are the section the instrumented C shows stamping the surviving
sub-Mapping, and they are the ones `cap_cmpmap_01`, `cap_cmpmap_03`,
`cap_cmpmap_04` and `cap_specmap_02` turn on. They are marked `ported` because
the port performs each step; the disagreement is over which object ends up
being a whole simplification result, not over a missing step. See PLAN.md §10.3.

# `MapMerge` audit — WinMap

C source: reference/starlink-ast/src/winmap.c
Rust: crates/ast-core/src/mappings/winmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- winmap.c:1029 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- winmap.c:1081 (fn MapMerge) — initialisation to silence a compiler warning; the Rust locals are initialised where they are declared

C's body runs `winmap.c:1078-1895 (fn MapMerge)`, the longest `MapMerge` in the library. Six
parts: replace a zero-shift WinMap with a diagonal MatrixMap; merge directly with
a neighbour in series; distribute the WinMap into a neighbouring parallel CmpMap;
swap toward the nearest mergeable Mapping; merge with a neighbour in parallel;
and, only if nothing else fired, convert an all-unit-scale WinMap to a ShiftMap.

`WinMap::map_merge` (`winmap.rs:405-412 (fn map_merge)`) delegates to
`winmap_merge::win_map_merge` (`winmap_merge.rs:372-439 (fn win_map_merge)`), so Rust sites are in
`winmap_merge.rs` unless stated.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1, then check the global error status and bail out | winmap.c:1075-1079 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index, and errors travel in `AstResult` rather than an ambient status |
| 2 | Get the WinMap's axis count and a typed pointer to it | winmap.c:1086-1091 (fn MapMerge) | winmap_merge.rs:378-379 (fn win_map_merge) | architecture | ported |
| 3 | See if the WinMap can be replaced by something simpler, without reference to its neighbours | winmap.c:1092-1094 (fn MapMerge) | winmap_merge.rs:381-384 (fn win_map_merge) | architecture | ported |
| 4 | Test whether every shift term is zero, using the raw stored terms | winmap.c:1095-1106 (fn MapMerge) | winmap_merge.rs:385 (fn win_map_merge) | architecture | ported |
| 5 | Temporarily set the WinMap's Invert attribute to the supplied value | winmap.c:1107-1114 (fn MapMerge) | winmap_merge.rs:379 (fn win_map_merge),384 (fn win_map_merge) | architecture | ported |
| 6 | Take the effective scale terms and build a diagonal MatrixMap from them | winmap.c:1115-1120 (fn MapMerge) | winmap_merge.rs:386-390 (fn win_map_merge) | architecture | ported |
| 7 | Restore the Invert attribute and free the scale terms | winmap.c:1121-1126 (fn MapMerge) | -- | ceremony | n/a: `extract_linear_params` reads the entry without mutating it, and owned values are dropped |
| 8 | Replace the WinMap in the list with the MatrixMap, clear the invert flag, and return the index | winmap.c:1127-1136 (fn MapMerge) | winmap_merge.rs:391-393 (fn win_map_merge) | architecture | ported |
| 9 | Note that the all-unit-scale to ShiftMap conversion is deferred to the end, to avoid a loop with `ShiftMap::MapMerge` | winmap.c:1137-1146 (fn MapMerge) | winmap_merge.rs:417-418 (fn win_map_merge) | architecture | ported |
| 10 | Otherwise store the neighbouring Mappings' classes, and whether a MatrixMap neighbour is diagonal | winmap.c:1147-1166 (fn MapMerge) | winmap_merge.rs:47-76 (fn is_mergeable) | architecture | ported |
| 11 | In series, look for a neighbour the WinMap can merge with, reducing the list by one | winmap.c:1167-1197 (fn MapMerge) | winmap_merge.rs:396-399 (fn win_map_merge) | architecture | ported |
| 12 | Create the merged Mapping | winmap.c:1198-1248 (fn MapMerge) | winmap_merge.rs:646-678 (fn merge_linear_pair) | architecture | ported |
| 13 | Replace the first of the pair with it, annul the second, close the gap and return the first index | winmap.c:1249-1275 (fn MapMerge) | winmap_merge.rs:465-470 (fn try_direct_merge) | architecture | ported |
| 14 | If a neighbour is a parallel CmpMap, convert the WinMap into an equivalent parallel CmpMap and merge that instead | winmap.c:1276-1304 (fn MapMerge) | winmap_merge.rs:401-406 (fn win_map_merge) | architecture | ported |
| 15 | Take the nominated WinMap's effective scale and zero terms, and the CmpMap's two components | winmap.c:1305-1324 (fn MapMerge) | winmap_merge.rs:513-556 (fn try_distribute_into_parallel) | architecture | ported |
| 16 | Build one new WinMap per component from the matching slice of scale and zero terms | winmap.c:1325-1356 (fn MapMerge) | winmap_merge.rs:558-582 (fn try_distribute_into_parallel) | architecture | ported |
| 17 | Combine each component with its WinMap in series, and simplify both series CmpMaps | winmap.c:1357-1378 (fn MapMerge) | winmap_merge.rs:589-608 (fn try_distribute_into_parallel) | architecture | ported |
| 18 | Only proceed if at least one of them actually simplified | winmap.c:1379-1388 (fn MapMerge) | winmap_merge.rs:609-621 (fn try_distribute_into_parallel) | architecture | ported |
| 19 | Re-instate the component Invert attributes and free resources | winmap.c:1389-1410 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 20 | Store the new parallel CmpMap over the merged pair | winmap.c:1411-1429 (fn MapMerge) | winmap_merge.rs:623-636 (fn try_distribute_into_parallel) | architecture | ported |
| 21 | If no direct merge, consider swapping the WinMap toward a Mapping it could merge with | winmap.c:1430-1453 (fn MapMerge) | winmap_merge.rs:409-414 (fn win_map_merge) | architecture | ported |
| 22 | Walk forward counting steps to the nearest mergeable target | winmap.c:1454-1489 (fn MapMerge) | winmap_merge.rs:698-711 (fn try_swap_strategy) | architecture | ported |
| 23 | Do the same walking backward | winmap.c:1490-1526 (fn MapMerge) | winmap_merge.rs:713-726 (fn try_swap_strategy) | architecture | ported |
| 24 | Choose the neighbour so the WinMap moves toward the nearer target | winmap.c:1527-1541 (fn MapMerge) | winmap_merge.rs:728-736 (fn try_swap_strategy) | architecture | ported |
| 25 | Swap the pair, then merge them if possible | winmap.c:1542-1581 (fn MapMerge) | winmap_merge.rs:338-356 (fn swap_pair), winmap_merge.rs:742-759 (fn try_swap_strategy) | architecture | ported |
| 26 | With no target, swap speculatively with each neighbour in turn | winmap.c:1582-1626 (fn MapMerge) | winmap_merge.rs:766-807 (fn try_speculative_swap) | architecture | ported |
| 27 | After a speculative swap, check whether the two neighbours can now merge with each other | winmap.c:1627-1669 (fn MapMerge) | winmap_merge.rs:816-849 (fn swap_is_worthwhile) | architecture | ported |
| 28 | Failing that, check whether either swapped Mapping simplifies on its own | winmap.c:1670-1683 (fn MapMerge) | winmap_merge.rs:851-859 (fn swap_is_worthwhile) | architecture | ported |
| 29 | Keep the swap only if it bought something, otherwise discard it | winmap.c:1684-1704 (fn MapMerge) | winmap_merge.rs:794-803 (fn try_speculative_swap) | architecture | ported |
| 30 | In parallel, a WinMap merges with a neighbouring WinMap, ZoomMap or UnitMap | winmap.c:1705-1736 (fn MapMerge) | winmap_merge.rs:396-399 (fn win_map_merge) | architecture | ported |
| 31 | Create that merged Mapping | winmap.c:1737-1797 (fn MapMerge) | winmap_merge.rs:646-678 (fn merge_linear_pair) | architecture | ported |
| 32 | Store it over the pair, close the gap and return the first index | winmap.c:1798-1822 (fn MapMerge) | winmap_merge.rs:465-470 (fn try_direct_merge) | architecture | ported |
| 33 | If nothing simplified, test whether every scale term is 1.0 | winmap.c:1829-1842 (fn MapMerge) | winmap_merge.rs:419 (fn win_map_merge) | architecture | ported |
| 34 | Take the effective shift terms and construct the equivalent ShiftMap | winmap.c:1843-1881 (fn MapMerge) | winmap_merge.rs:420-422 (fn win_map_merge) | architecture | ported |
| 35 | Replace the WinMap with it, mark it forward, and return the index | winmap.c:1882-1890 (fn MapMerge) | winmap_merge.rs:422-424 (fn win_map_merge) | architecture | ported |
| 36 | Return the result | winmap.c:1894-1895 (fn MapMerge) | winmap_merge.rs:438 (fn win_map_merge) | ceremony | ported |

## The lone WinMap has no reduction of its own

C gives WinMap no `Simplify` vtab slot, so a WinMap nominated on its own reaches
the same body as one in a run, and both reductions it can undergo are rows in the
table above. A zero-shift WinMap becomes a diagonal MatrixMap built from the
effective scale terms (row 4, `winmap_merge.rs:381-395 (fn win_map_merge)`), which then reduces on
through `matrixmap.c`'s own merge — to a ZoomMap when the scales are uniform, and
from there to a UnitMap when they are all 1.0. An all-unit-scale WinMap becomes a
ShiftMap holding the effective shifts (rows 33 to 35). Between them those two
cover every WinMap that reduces at all.

## Row 9 — the deferral is C's own design, not a port workaround

C explains at `winmap.c:1137-1146 (fn MapMerge)` why the WinMap-to-ShiftMap conversion cannot
happen in the replace-with-simpler part: `ShiftMap::MapMerge` turns a ShiftMap
straight back into a WinMap and relies on this function to do the work, so an
early conversion would ping-pong. C therefore does it last, only when nothing
else fired. The port keeps that position (`winmap_merge.rs:417-424 (fn win_map_merge)`), and the
oscillation break itself is the merge loop's `Frozen` mechanism rather than
anything in either class.

## Rows 4 and 33 — raw terms for the tests, effective terms for the result

Both predicates read the WinMap's *stored* terms, matching C testing
`oldwm->a` and `oldwm->b` directly, while the replacement is built from the
effective terms after the orientation is applied. Testing the effective terms
instead would fire on a different set of WinMaps.

## Row 27 — the swap is judged by what it unblocks

C's speculative swap is kept only if something can be simplified afterwards, and
it checks two different things in order: whether the two Mappings that were
separated by the WinMap can now merge with each other, and only then whether
either swapped Mapping simplifies on its own. `swap_is_worthwhile`
(`winmap_merge.rs:808-862 (fn swap_is_worthwhile)`) keeps both tests in that order.

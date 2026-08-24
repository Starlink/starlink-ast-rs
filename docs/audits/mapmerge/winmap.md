# `MapMerge` audit — WinMap

C source: reference/starlink-ast/src/winmap.c
Rust: crates/ast-core/src/mappings/winmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `winmap.c:1070-1865`, the longest `MapMerge` in the library. Six
parts: replace a zero-shift WinMap with a diagonal MatrixMap; merge directly with
a neighbour in series; distribute the WinMap into a neighbouring parallel CmpMap;
swap toward the nearest mergeable Mapping; merge with a neighbour in parallel;
and, only if nothing else fired, convert an all-unit-scale WinMap to a ShiftMap.

`WinMap::map_merge` (`winmap.rs:367`) delegates to
`winmap_merge::win_map_merge` (`winmap_merge.rs:372-439`), so Rust sites are in
`winmap_merge.rs` unless stated.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Check the global error status and bail out | winmap.c:1070-1071 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 2 | Get the WinMap's axis count and a typed pointer to it | winmap.c:1078-1083 | winmap_merge.rs:378-379 | ported |
| 3 | See if the WinMap can be replaced by something simpler, without reference to its neighbours | winmap.c:1084-1086 | winmap_merge.rs:381-384 | ported |
| 4 | Test whether every shift term is zero, using the raw stored terms | winmap.c:1087-1098 | winmap_merge.rs:385 | ported |
| 5 | Temporarily set the WinMap's Invert attribute to the supplied value | winmap.c:1099-1106 | winmap_merge.rs:379,384 | ported |
| 6 | Take the effective scale terms and build a diagonal MatrixMap from them | winmap.c:1107-1112 | winmap_merge.rs:386-390 | ported |
| 7 | Restore the Invert attribute and free the scale terms | winmap.c:1113-1118 | -- | n/a: `extract_linear_params` reads the entry without mutating it, and owned values are dropped |
| 8 | Replace the WinMap in the list with the MatrixMap, clear the invert flag, and return the index | winmap.c:1119-1128 | winmap_merge.rs:391-393 | ported |
| 9 | Note that the all-unit-scale to ShiftMap conversion is deferred to the end, to avoid a loop with `ShiftMap::MapMerge` | winmap.c:1129-1138 | winmap_merge.rs:417-418 | ported |
| 10 | Otherwise store the neighbouring Mappings' classes, and whether a MatrixMap neighbour is diagonal | winmap.c:1139-1158 | winmap_merge.rs:47-76 | ported |
| 11 | In series, look for a neighbour the WinMap can merge with, reducing the list by one | winmap.c:1159-1189 | winmap_merge.rs:396-399 | ported |
| 12 | Create the merged Mapping | winmap.c:1190-1240 | winmap_merge.rs:197-337,654-690 | ported |
| 13 | Replace the first of the pair with it, annul the second, close the gap and return the first index | winmap.c:1241-1267 | winmap_merge.rs:444-487 | ported |
| 14 | If a neighbour is a parallel CmpMap, convert the WinMap into an equivalent parallel CmpMap and merge that instead | winmap.c:1268-1296 | winmap_merge.rs:401-406 | ported |
| 15 | Take the nominated WinMap's effective scale and zero terms, and the CmpMap's two components | winmap.c:1297-1316 | winmap_merge.rs:488-653 | ported |
| 16 | Build one new WinMap per component from the matching slice of scale and zero terms | winmap.c:1317-1348 | winmap_merge.rs:488-653 | ported |
| 17 | Combine each component with its WinMap in series, and simplify both series CmpMaps | winmap.c:1349-1370 | winmap_merge.rs:488-653 | ported |
| 18 | Only proceed if at least one of them actually simplified | winmap.c:1371-1380 | winmap_merge.rs:488-653 | ported |
| 19 | Re-instate the component Invert attributes and free resources | winmap.c:1381-1402 | -- | n/a: nothing was mutated, so nothing is restored |
| 20 | Store the new parallel CmpMap over the merged pair | winmap.c:1403-1421 | winmap_merge.rs:488-653 | ported |
| 21 | If no direct merge, consider swapping the WinMap toward a Mapping it could merge with | winmap.c:1422-1445 | winmap_merge.rs:409-414 | ported |
| 22 | Walk forward counting steps to the nearest mergeable target | winmap.c:1446-1481 | winmap_merge.rs:691-773 | ported |
| 23 | Do the same walking backward | winmap.c:1482-1518 | winmap_merge.rs:691-773 | ported |
| 24 | Choose the neighbour so the WinMap moves toward the nearer target | winmap.c:1519-1533 | winmap_merge.rs:691-773 | ported |
| 25 | Swap the pair, then merge them if possible | winmap.c:1534-1573 | winmap_merge.rs:338-356,691-773 | ported |
| 26 | With no target, swap speculatively with each neighbour in turn | winmap.c:1574-1618 | winmap_merge.rs:774-815 | ported |
| 27 | After a speculative swap, check whether the two neighbours can now merge with each other | winmap.c:1619-1661 | winmap_merge.rs:816-874 | ported |
| 28 | Failing that, check whether either swapped Mapping simplifies on its own | winmap.c:1662-1675 | winmap_merge.rs:875-890 | ported |
| 29 | Keep the swap only if it bought something, otherwise discard it | winmap.c:1676-1696 | winmap_merge.rs:774-815 | ported |
| 30 | In parallel, a WinMap merges with a neighbouring WinMap, ZoomMap or UnitMap | winmap.c:1697-1728 | winmap_merge.rs:396-399 | ported |
| 31 | Create that merged Mapping | winmap.c:1729-1789 | winmap_merge.rs:654-690 | ported |
| 32 | Store it over the pair, close the gap and return the first index | winmap.c:1790-1814 | winmap_merge.rs:444-487 | ported |
| 33 | If nothing simplified, test whether every scale term is 1.0 | winmap.c:1821-1834 | winmap_merge.rs:419 | ported |
| 34 | Take the effective shift terms and construct the equivalent ShiftMap | winmap.c:1835-1851 | winmap_merge.rs:420-422 | ported |
| 35 | Replace the WinMap with it, mark it forward, and return the index | winmap.c:1852-1860 | winmap_merge.rs:422-424 | ported |
| 36 | Return the result | winmap.c:1864-1865 | winmap_merge.rs:438 | ported |

## The lone WinMap has no reduction of its own

C gives WinMap no `Simplify` vtab slot, so a WinMap nominated on its own reaches
the same body as one in a run, and both reductions it can undergo are rows in the
table above. A zero-shift WinMap becomes a diagonal MatrixMap built from the
effective scale terms (row 4, `winmap_merge.rs:381-395`), which then reduces on
through `matrixmap.c`'s own merge — to a ZoomMap when the scales are uniform, and
from there to a UnitMap when they are all 1.0. An all-unit-scale WinMap becomes a
ShiftMap holding the effective shifts (rows 33 to 35). Between them those two
cover every WinMap that reduces at all.

## Row 9 — the deferral is C's own design, not a port workaround

C explains at `winmap.c:1129-1138` why the WinMap-to-ShiftMap conversion cannot
happen in the replace-with-simpler part: `ShiftMap::MapMerge` turns a ShiftMap
straight back into a WinMap and relies on this function to do the work, so an
early conversion would ping-pong. C therefore does it last, only when nothing
else fired. The port keeps that position (`winmap_merge.rs:417-424`), and the
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
(`winmap_merge.rs:816-874`) keeps both tests in that order.

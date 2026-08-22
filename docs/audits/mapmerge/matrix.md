# `MapMerge` audit — MatrixMap

C source: reference/starlink-ast/src/matrixmap.c
Rust: crates/ast-core/src/mappings/matrix.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `matrixmap.c:1830-2290` in four parts: replace the MatrixMap with
something simpler; merge it directly with a neighbour; swap it toward the nearest
Mapping it could merge with; and, failing that, swap speculatively with either
neighbour if the swap itself simplifies something.

`MatrixMap::map_merge` (`matrix.rs:884-891`) delegates to
`matrix_merge::matrix_map_merge` (`matrix_merge.rs:40-106`), so Rust sites are
in `matrix_merge.rs` unless stated.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | matrixmap.c:1830-1831 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | matrixmap.c:1833-1834 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get the Invert attribute for the nominated Mapping | matrixmap.c:1841-1843 | matrix_merge.rs:46 | ported |
| 4 | Get the input and output axis counts, swapped when the list's invert flag differs from the object's own | matrixmap.c:1844-1856 | matrix_merge.rs:48-55 | ported |
| 5 | See if the MatrixMap can be replaced by something simpler, without reference to its neighbours | matrixmap.c:1857-1862 | matrix_merge.rs:57-61 | ported |
| 6 | A square unit matrix becomes a UnitMap | matrixmap.c:1863-1867 | matrix.rs:673-718 | ported |
| 7 | A square diagonal matrix with equal diagonal terms becomes a ZoomMap | matrixmap.c:1868-1895 | matrix.rs:673-718 | ported |
| 8 | A full matrix whose off-diagonal elements are all zero becomes a diagonal MatrixMap | matrixmap.c:1896-1918 | matrix.rs:673-718 | ported |
| 9 | On a replacement, store the new Mapping in the list and report a change | matrixmap.c:1919-1929 | matrix_merge.rs:58-60 | ported |
| 10 | Otherwise, in series, store the neighbouring Mappings' classes | matrixmap.c:1930-1942 | matrix_merge.rs:63-72 | ported |
| 11 | Look for a neighbour the MatrixMap can merge with directly, reducing the list by one | matrixmap.c:1943-1966 | matrix_merge.rs:73-76 | ported |
| 12 | Accept a PermMap neighbour only when its forward and inverse mappings are consistent | matrixmap.c:1967-1972 | matrix_merge.rs:385-410 | ported |
| 13 | A diagonal MatrixMap may also merge with a WinMap | matrixmap.c:1973-1987 | matrix_merge.rs:123-147 | ported |
| 14 | Create the merged Mapping | matrixmap.c:1988-2040 | matrix_merge.rs:194-330 | ported |
| 15 | Replace the first of the pair with the merged Mapping, annul the second, close the gap and return the first index | matrixmap.c:2041-2065 | matrix_merge.rs:148-193 | ported |
| 16 | If no direct merge, consider swapping the MatrixMap with a neighbour to move it toward one it can merge with | matrixmap.c:2066-2076 | matrix_merge.rs:78-81 | ported |
| 17 | Set a flag for each direction saying whether swapping would simplify either Mapping | matrixmap.c:2077-2088 | matrix_merge.rs:644-653 | ported |
| 18 | Walk forward from the MatrixMap counting steps to the nearest Mapping it could merge with | matrixmap.c:2089-2121 | matrix_merge.rs:655-671 | ported |
| 19 | Do the same walking backward | matrixmap.c:2122-2155 | matrix_merge.rs:673-687 | ported |
| 20 | Choose the neighbour to swap with so the MatrixMap moves toward the nearer target | matrixmap.c:2156-2170 | matrix_merge.rs:689-696 | ported |
| 21 | If a target exists, swap the pair and then merge them | matrixmap.c:2171-2207 | matrix_merge.rs:749-903 | ported |
| 22 | With no target, swap with each neighbour in turn on the chance the swap itself simplifies something | matrixmap.c:2208-2249 | matrix_merge.rs:904-960 | ported |
| 23 | Take copies of the Mapping and Invert arrays so the supplied values are not disturbed | matrixmap.c:2236-2242 | matrix_merge.rs:904-960 | ported |
| 24 | If neither swapped Mapping simplifies further, discard the swap | matrixmap.c:2250-2260 | matrix_merge.rs:904-960 | ported |
| 25 | Otherwise store the swapped pair and report the first index | matrixmap.c:2261-2288 | matrix_merge.rs:904-960 | ported |
| 26 | Return the result | matrixmap.c:2289-2290 | matrix_merge.rs:105 | ported |

## Rows 11 to 15 — the neighbour list is a closed set, and the fallback respects it

C's merge only considers a neighbour whose class it names: MatrixMap, ZoomMap,
UnitMap, an invertable PermMap, or — when this MatrixMap is diagonal — a WinMap.
Anything else takes the `else` branch and merges nothing.

That is load-bearing, not an optimisation. A ShiftMap neighbour is deliberately
absent: C waits for the ShiftMap's own merge to turn it into a WinMap first.
Folding the pair early would compose through the MatrixMap's effective direction
and materialise the inverse into the stored values, so the Mapping would
serialise with the inverse matrix stored forward where C stores the forward
matrix and records `InvB` on the parent.

`matrix_merge.rs:97-105` is what keeps the port to that rule: the fall-through to
`default_map_merge` is gated on the nominee actually having a neighbour C would
accept, so the generic pairwise path cannot merge a class C's MatrixMap declines.

## Row 18 — a WinMap is swapped over, never sought

On the forward walk C's merge targets are MatrixMap, ZoomMap, an invertable
PermMap and UnitMap. A WinMap is a swappable class but not a target, so the walk
passes over it rather than stopping. `is_seek_target` and `is_swappable_class`
(`matrix_merge.rs:411-415,720-736`) keep the two lists distinct for that reason.

# `MapMerge` audit — MatrixMap

C source: reference/starlink-ast/src/matrixmap.c
Rust: crates/starlink-ast/src/mappings/matrix.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- matrixmap.c:1797 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- matrixmap.c:1836 (fn MapMerge) — initialization to silence a compiler warning; the Rust locals are initialized where they are declared

C's body runs `matrixmap.c:1830-2290 (fn MapMerge)` in four parts: replace the MatrixMap with
something simpler; merge it directly with a neighbor; swap it toward the nearest
Mapping it could merge with; and, failing that, swap speculatively with either
neighbor if the swap itself simplifies something.

`MatrixMap::map_merge` (`matrix.rs:1077-1084 (fn map_merge)`) delegates to
`matrix_merge::matrix_map_merge` (`matrix_merge.rs:45-121 (fn matrix_map_merge)`), so Rust sites are
in `matrix_merge.rs` unless stated.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | matrixmap.c:1830-1831 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | matrixmap.c:1833-1834 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get the Invert attribute for the nominated Mapping | matrixmap.c:1841-1843 (fn MapMerge) | matrix_merge.rs:51 (fn matrix_map_merge) | architecture | ported |
| 4 | Get the input and output axis counts, swapped when the list's invert flag differs from the object's own | matrixmap.c:1844-1856 (fn MapMerge) | matrix_merge.rs:53-60 (fn matrix_map_merge) | architecture | ported |
| 5 | See if the MatrixMap can be replaced by something simpler, without reference to its neighbors | matrixmap.c:1857-1862 (fn MapMerge) | matrix_merge.rs:62-67 (fn matrix_map_merge) | architecture | ported |
| 6 | A square unit matrix becomes a UnitMap | matrixmap.c:1863-1867 (fn MapMerge) | matrix_merge.rs:1016-1019 (fn replace_with_simpler) | architecture | ported |
| 7 | A square diagonal matrix with equal diagonal terms becomes a ZoomMap | matrixmap.c:1868-1895 (fn MapMerge) | matrix_merge.rs:1021-1040 (fn replace_with_simpler) | architecture | ported |
| 8 | A full matrix whose off-diagonal elements are all zero becomes a diagonal MatrixMap | matrixmap.c:1896-1918 (fn MapMerge) | matrix_merge.rs:1043-1061 (fn replace_with_simpler) | architecture | ported |
| 9 | On a replacement, store the new Mapping in the list and report a change | matrixmap.c:1919-1929 (fn MapMerge) | matrix_merge.rs:63-66 (fn matrix_map_merge) | architecture | ported |
| 10 | Otherwise, in series, store the neighboring Mappings' classes | matrixmap.c:1930-1942 (fn MapMerge) | matrix_merge.rs:72-81 (fn matrix_map_merge) | architecture | ported |
| 11 | Look for a neighbor the MatrixMap can merge with directly, reducing the list by one | matrixmap.c:1943-1966 (fn MapMerge) | matrix_merge.rs:82-88 (fn matrix_map_merge) | architecture | ported |
| 12 | Accept a PermMap neighbor only when its forward and inverse mappings are consistent | matrixmap.c:1967-1972 (fn MapMerge) | matrix_merge.rs:405-432 (fn perm_ok) | architecture | ported |
| 13 | A diagonal MatrixMap may also merge with a WinMap | matrixmap.c:1973-1987 (fn MapMerge) | matrix_merge.rs:138-162 (fn is_mergeable) | architecture | ported |
| 14 | Create the merged Mapping | matrixmap.c:1988-2040 (fn MapMerge) | matrix_merge.rs:210-332 (fn merge_matrix_pair) | architecture | ported |
| 15 | Replace the first of the pair with the merged Mapping, annul the second, close the gap and return the first index | matrixmap.c:2041-2065 (fn MapMerge) | matrix_merge.rs:203-204 (fn try_direct_merge) | architecture | ported |
| 16 | If no direct merge, consider swapping the MatrixMap with a neighbor to move it toward one it can merge with | matrixmap.c:2066-2076 (fn MapMerge) | matrix_merge.rs:90-93 (fn matrix_map_merge) | architecture | ported |
| 17 | Set a flag for each direction saying whether swapping would simplify either Mapping | matrixmap.c:2077-2088 (fn MapMerge) | matrix_merge.rs:668-677 (fn try_swap_strategy) | architecture | ported |
| 18 | Walk forward from the MatrixMap counting steps to the nearest Mapping it could merge with | matrixmap.c:2089-2121 (fn MapMerge) | matrix_merge.rs:679-695 (fn try_swap_strategy) | architecture | ported |
| 19 | Do the same walking backward | matrixmap.c:2122-2155 (fn MapMerge) | matrix_merge.rs:697-711 (fn try_swap_strategy) | architecture | ported |
| 20 | Choose the neighbor to swap with so the MatrixMap moves toward the nearer target | matrixmap.c:2156-2170 (fn MapMerge) | matrix_merge.rs:713-720 (fn try_swap_strategy) | architecture | ported |
| 21 | If a target exists, swap the pair and then merge them | matrixmap.c:2171-2207 (fn MapMerge) | matrix_merge.rs:727-737 (fn try_swap_strategy), matrix_merge.rs:776-798 (fn swap_pair) | architecture | ported |
| 22 | With no target, swap with each neighbor in turn on the chance the swap itself simplifies something | matrixmap.c:2208-2249 (fn MapMerge) | matrix_merge.rs:944-967 (fn try_speculative_swap) | architecture | ported |
| 23 | Take copies of the Mapping and Invert arrays so the supplied values are not disturbed | matrixmap.c:2236-2242 (fn MapMerge) | matrix_merge.rs:995-999 (fn clone_pair) | architecture | ported |
| 24 | If neither swapped Mapping simplifies further, discard the swap | matrixmap.c:2250-2260 (fn MapMerge) | matrix_merge.rs:969-986 (fn try_speculative_swap) | architecture | ported |
| 25 | Otherwise store the swapped pair and report the first index | matrixmap.c:2261-2288 (fn MapMerge) | matrix_merge.rs:986-989 (fn try_speculative_swap) | architecture | ported |
| 26 | Return the result | matrixmap.c:2289-2290 (fn MapMerge) | matrix_merge.rs:120 (fn matrix_map_merge) | ceremony | ported |

## Rows 6 to 8 — the only reduction a lone MatrixMap has

C gives MatrixMap no `Simplify` vtab slot, so a MatrixMap nominated on its own
reaches the same body as one in a run, and these three replacements are every
reduction it can undergo. They live in `replace_with_simpler`
(`matrix_merge.rs:1004-1065 (fn replace_with_simpler)`), reached as step 1 of `matrix_map_merge`.

The applied direction is the list's, `(*invert_list)[where]`. C reads it only to
pick the zoom factor — the structural tests (uniformity, off-diagonal-zero) run
on the stored forward matrix either way — and clears it on the replacement
(`matrixmap.c:1925 (fn MapMerge)`). So an inverted uniform diagonal yields a *forward*
ZoomMap holding the reciprocal, not an inverted ZoomMap holding the original
factor.

Reaching a UnitMap from a full identity matrix therefore takes three passes, as
it does in C: full → diagonal MatrixMap → ZoomMap(1.0) → UnitMap, the last step
belonging to `zoom.rs`'s own merge. The nominate loop re-runs while a change is
reported, which is what makes the ladder converge.

## Rows 11 to 15 — the neighbor list is a closed set, and the fallback respects it

C's merge only considers a neighbor whose class it names: MatrixMap, ZoomMap,
UnitMap, an invertable PermMap, or — when this MatrixMap is diagonal — a WinMap.
Anything else takes the `else` branch and merges nothing.

That is load-bearing, not an optimization. A ShiftMap neighbor is deliberately
absent: C waits for the ShiftMap's own merge to turn it into a WinMap first.
Folding the pair early would compose through the MatrixMap's effective direction
and materialize the inverse into the stored values, so the Mapping would
serialize with the inverse matrix stored forward where C stores the forward
matrix and records `InvB` on the parent.

`matrix_merge.rs:112-120 (fn matrix_map_merge)` is what keeps the port to that rule: the fall-through to
`default_map_merge` is gated on the nominee actually having a neighbor C would
accept, so the generic pairwise path cannot merge a class C's MatrixMap declines.

## Row 18 — a WinMap is swapped over, never sought

On the forward walk C's merge targets are MatrixMap, ZoomMap, an invertable
PermMap and UnitMap. A WinMap is a swappable class but not a target, so the walk
passes over it rather than stopping. `is_seek_target` and `is_swappable_class`
(`matrix_merge.rs:433-437 (fn is_swappable_class),745-761 (fn is_seek_target)`) keep the two lists distinct for that reason.

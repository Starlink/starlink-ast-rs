# `MapMerge` audit — PcdMap

C source: reference/starlink-ast/src/pcdmap.c
Rust: crates/ast-core/src/mappings/pcdmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- pcdmap.c:1389 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- pcdmap.c:1418 (fn MapMerge) — initialises the two index variables to silence a compiler warning; the Rust indices are bound where they are computed

C's body runs `pcdmap.c:1412-1742 (fn MapMerge)` in four parts, in this order:
replace a zero-distortion PcdMap with a UnitMap without reference to its
neighbours; merge directly with an adjacent UnitMap or with the PcdMap's own
inverse; swap past an adjacent ZoomMap or axis-swapping PermMap to move the
PcdMap towards a distant Mapping it could merge with, then merge at the new
adjacency; and, with no such Mapping in reach, swap speculatively and keep the
swap only if one of the swapped Mappings then simplifies. A PcdMap combines in
parallel with nothing.

`PcdMap::map_merge` (`pcdmap.rs:341-348 (fn map_merge)`) delegates to
`simplify::pcd_map_merge` (`mappings/simplify.rs:2770-2804 (fn pcd_map_merge)`), whose swap half is
`pcd_swap_strategy` (`mappings/simplify.rs:2813-2871 (fn pcd_swap_strategy)`), so Rust sites below
are in `mappings/simplify.rs` unless stated.

C's `CanMerge`, `CanSwap`, `PcdZoom` and `PcdPerm` are file-local helpers that
`MapMerge` calls; they are described in the prose sections below rather than
cited per row.

The last row is not part of `MapMerge` but of the rule that governs it: an
attribute setter whose value feeds the first part of the class's own `MapMerge`
must discard the simplified record.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | pcdmap.c:1412-1414 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | pcdmap.c:1415-1417 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | See whether the PcdMap can be replaced by a simpler Mapping without reference to its neighbours | pcdmap.c:1423-1425 (fn MapMerge) | mappings/simplify.rs:2776-2784 (fn pcd_map_merge) | architecture | ported |
| 4 | A zero distortion coefficient means the PcdMap is a UnitMap | pcdmap.c:1426-1429 (fn MapMerge) | mappings/simplify.rs:2781 (fn pcd_map_merge) | architecture | ported |
| 5 | Annul the PcdMap in the list, replace it with a two-axis UnitMap and clear the entry's Invert flag | pcdmap.c:1430-1436 (fn MapMerge) | mappings/simplify.rs:2782 (fn pcd_map_merge) | architecture | ported |
| 6 | Return the index of the first modified element | pcdmap.c:1437-1439 (fn MapMerge) | mappings/simplify.rs:2783 (fn pcd_map_merge) | architecture | ported |
| 7 | If the PcdMap itself could not be simplified, see whether it merges with the Mappings on either side | pcdmap.c:1440-1443 (fn MapMerge) | mappings/simplify.rs:2792-2799 (fn pcd_map_merge) | architecture | ported |
| 8 | Store the classes of the neighbouring Mappings in the list | pcdmap.c:1444-1447 (fn MapMerge) | mappings/simplify.rs:1370-1392 (fn default_map_merge) | ceremony | ported |
| 9 | Everything that follows is in series | pcdmap.c:1448-1451 (fn MapMerge) | mappings/simplify.rs:2788-2790 (fn pcd_map_merge) | architecture | ported |
| 10 | Look on each side for a neighbour the PcdMap can merge with directly — its own inverse, or a UnitMap | pcdmap.c:1452-1475 (fn MapMerge) | pcdmap.rs:350-384 (fn try_merge) | architecture | ported |
| 11 | If it can, create the merged Mapping | pcdmap.c:1476-1479 (fn MapMerge) | pcdmap.rs:361-381 (fn try_merge) | architecture | ported |
| 12 | A PcdMap neighbour must be the nominated Mapping's inverse, so the pair becomes a UnitMap | pcdmap.c:1480-1485 (fn MapMerge) | pcdmap.rs:362-373 (fn try_merge) | architecture | ported |
| 13 | A UnitMap neighbour merges to a clone of the nominated PcdMap, carrying the entry's Invert flag | pcdmap.c:1486-1492 (fn MapMerge) | pcdmap.rs:376-381 (fn try_merge) | architecture | ported |
| 14 | Proceed only if the merged Mapping was built successfully | pcdmap.c:1493-1495 (fn MapMerge) | mappings/simplify.rs:2797 (fn pcd_map_merge) | ceremony | ported |
| 15 | Annul the first of the two Mappings, replace it with the merged Mapping and set its Invert flag | pcdmap.c:1496-1501 (fn MapMerge) | mappings/simplify.rs:1445-1450 (fn default_map_merge) | architecture | ported |
| 16 | Annul the second and shuffle the rest of the list down to fill the gap | pcdmap.c:1502-1509 (fn MapMerge) | mappings/simplify.rs:1451 (fn default_map_merge) | architecture | ported |
| 17 | Clear the vacated element at the end | pcdmap.c:1510-1513 (fn MapMerge) | -- | ceremony | n/a: removing the element shortens the `Vec`, so there is no vacated slot to clear |
| 18 | Decrement the Mapping count and return the index of the first modified element | pcdmap.c:1514-1520 (fn MapMerge) | mappings/simplify.rs:1452-1453 (fn default_map_merge) | architecture | ported |
| 19 | If no direct merge was possible, consider whether swapping the PcdMap with a neighbour would be worthwhile | pcdmap.c:1521-1531 (fn MapMerge) | mappings/simplify.rs:2803 (fn pcd_map_merge) | architecture | ported |
| 20 | Set a flag for whether the PcdMap could swap with its higher neighbour | pcdmap.c:1532-1541 (fn MapMerge) | mappings/simplify.rs:2821 (fn pcd_swap_strategy) | architecture | ported |
| 21 | If so, step forward through the following Mappings looking for one the PcdMap could merge with | pcdmap.c:1542-1551 (fn MapMerge) | mappings/simplify.rs:2827-2831 (fn pcd_swap_strategy) | architecture | ported |
| 22 | On finding a UnitMap or PcdMap, note the number of Mappings separating the two and leave the loop | pcdmap.c:1552-1561 (fn MapMerge) | mappings/simplify.rs:2898-2903 (fn pcd_steps_to_merge_target) | architecture | ported |
| 23 | On finding a Mapping the PcdMap could not swap past, leave with -1 steps; only ZoomMaps and PermMaps may be stepped over | pcdmap.c:1562-1573 (fn MapMerge) | mappings/simplify.rs:2905-2907 (fn pcd_steps_to_merge_target) | architecture | ported |
| 24 | Do the same working from the PcdMap towards the start of the list | pcdmap.c:1574-1605 (fn MapMerge) | mappings/simplify.rs:2822 (fn pcd_swap_strategy),2832-2836 (fn pcd_swap_strategy) | architecture | ported |
| 25 | Choose which neighbour to swap with so the PcdMap moves towards the nearest Mapping it can merge with | pcdmap.c:1606-1619 (fn MapMerge) | mappings/simplify.rs:2839-2845 (fn pcd_swap_strategy) | architecture | ported |
| 26 | With a target Mapping in reach, replace the supplied pair with the swapped pair | pcdmap.c:1620-1624 (fn MapMerge) | mappings/simplify.rs:2848-2850 (fn pcd_swap_strategy) | architecture | ported |
| 27 | Swap the Mappings, by the ZoomMap route or the PermMap route | pcdmap.c:1625-1632 (fn MapMerge) | mappings/simplify.rs:2983-3039 (fn pcd_swap_pair) | architecture | ported |
| 28 | And then merge them, running `astMapMerge` over the sub-list that starts at the new adjacency | pcdmap.c:1633-1656 (fn MapMerge) | mappings/simplify.rs:2857-2862 (fn pcd_swap_strategy) | architecture | ported |
| 29 | With no Mapping available for merging, a swap may still pay because the swapped Mappings may be simpler | pcdmap.c:1657-1661 (fn MapMerge) | mappings/simplify.rs:2867-2869 (fn pcd_swap_strategy) | architecture | ported |
| 30 | Try swapping with each possible neighbour in turn | pcdmap.c:1662-1664 (fn MapMerge) | mappings/simplify.rs:2921 (fn pcd_speculative_swap) | architecture | ported |
| 31 | Set up the class and pointers for the pair to be swapped, the lower neighbour first and then the upper | pcdmap.c:1665-1680 (fn MapMerge) | mappings/simplify.rs:2922-2928 (fn pcd_speculative_swap) | architecture | ported |
| 32 | Proceed only when there is a Mapping to swap with | pcdmap.c:1681-1683 (fn MapMerge) | mappings/simplify.rs:2926-2927 (fn pcd_speculative_swap) | architecture | ported |
| 33 | Take copies of the Mappings and Invert flags so the supplied values are not changed | pcdmap.c:1684-1690 (fn MapMerge) | mappings/simplify.rs:2969-2973 (fn pcd_clone_pair) | architecture | ported |
| 34 | Swap these Mappings | pcdmap.c:1691-1697 (fn MapMerge) | mappings/simplify.rs:2930-2932 (fn pcd_speculative_swap) | architecture | ported |
| 35 | If neither swapped Mapping simplifies to a different class there is no point in swapping, so discard the copies | pcdmap.c:1698-1708 (fn MapMerge) | mappings/simplify.rs:2937-2943 (fn pcd_speculative_swap) | architecture | ported |
| 36 | Otherwise annul the supplied Mappings, store the swapped pair with its Invert flags, record the first index and stop | pcdmap.c:1709-1725 (fn MapMerge) | mappings/simplify.rs:2945-2949 (fn pcd_speculative_swap) | architecture | ported |
| 37 | Annul the simplified Mappings | pcdmap.c:1726-1734 (fn MapMerge) | -- | ceremony | n/a: the simplification is an owned value dropped at the end of the worthwhile test |
| 38 | In parallel, a PcdMap cannot combine with any other Mapping | pcdmap.c:1735-1740 (fn MapMerge) | mappings/simplify.rs:2788-2790 (fn pcd_map_merge) | architecture | ported |
| 39 | Return the result | pcdmap.c:1741-1742 (fn MapMerge) | mappings/simplify.rs:2803 (fn pcd_map_merge) | ceremony | ported |
| 40 | Setting or clearing Disco clears the simplified record | pcdmap.c:2594 (fn astMAKE_CLEAR1),2597 (fn astMAKE_SET1) | pcdmap.rs:130-136 (fn set_disc) | ceremony | ported |

## Rows 3 to 6 — the reduction, and the ordering around it

`pcd_map_merge` turns a zero-distortion PcdMap into a UnitMap at its head, before
the direct merge and before the swap, where C has it. `pcdmap.c` has no
`Simplify` vtab slot, so this is the only place the reduction exists, and a lone
PcdMap reaches it the way C does — the base `Simplify` nominates it in a list of
one (`mapping.c:16871-16883 (fn Simplify)`).

C makes the reduction the `if` of an if/else whose `else` holds the entire
neighbour search, so a PcdMap that reduces looks at no neighbour in the same
pass; the early return is that structure. It also clears the entry's `Invert`
flag along with the replacement (`pcdmap.c:1435 (fn MapMerge)`), which is why the new pair
is stored with both flags false rather than inheriting the entry's.

`pcd_zero_disc_series` and `pcd_zero_disc_lone` are the fixtures. The lone case
passed before the fix and the series case did not, which is what identified the
seam rather than the arithmetic as what was missing.

## Rows 10 to 13 — `CanMerge` and the Mapping built from the pair

C splits the direct merge in two: `CanMerge` decides whether the pair can
combine, and `MapMerge` then builds the replacement. `PcdMap::try_merge` is both
halves at once — it returns the merged Mapping, and `None` where `CanMerge`
would return zero — and `default_map_merge` offers the pair on the lower side
and then on the upper side, which is C's two `CanMerge` calls in C's order.

`CanMerge` accepts a PcdMap neighbour only when the two have the same `Disco`
and `PcdCen` and opposite effective directions, and a UnitMap neighbour
unconditionally. `try_merge` applies the same two tests to the pair it is given,
with the effective directions taken from the list flags rather than the objects,
because `flatten` stores every leaf forward.

## Rows 21 to 25 — the step walk uses C's class tests, not `CanSwap`'s

`pcd_steps_to_merge_target` is C's two loops. It stops at the first UnitMap or
PcdMap and reports how many Mappings separate it from the nominated PcdMap, and
stops with -1 at the first Mapping that is neither a ZoomMap nor a PermMap.

The class tests inside the walk are deliberately looser than `CanSwap`: C tests
only the class name, so *any* PermMap can be stepped over here, while `CanSwap`
accepts a PermMap only when it simply swaps the two axes. A non-swapping PermMap
in the middle of the run therefore does not hide a target behind it — the walk
counts past it, the swap towards the target then stops at it, and the next
invocation makes no further progress. Narrowing the walk to `CanSwap`'s test
would change which direction C picks when both sides have a target.

## Row 28 — the nested merge is part of the swap

C does not stop at the swap. Having moved the PcdMap one place towards the
target, it calls `astMapMerge` again over the sub-list that starts at the new
adjacency, so the PcdMap merges with the target in the same pass rather than
waiting for the driver's next sweep. Which sub-list depends on the direction of
the swap: merging upwards it starts at `where + 1`, merging downwards at
`where - 2`, and in both cases the nominated element is index 0 of that
sub-list, so the nested merge cannot reach back past where the sub-list begins.

`MergeCtx::merge_subrange` is that scoping. The observable difference from
running the swap alone is the number of driver passes a chain needs to reach its
normal form, and hence — through the simplified records the driver sets on the
way — which intermediate structures get frozen.

## Rows 29 to 36 — the speculative swap

With no target in reach, C tries the swap anyway: first with the lower
neighbour, then with the upper one. It works on copies, simplifies each of the
two swapped Mappings, and keeps the swap only when one of them simplifies to a
*different class*; otherwise the copies are discarded and the list is left
exactly as it was. The first swap that qualifies wins, and C breaks out of the
loop.

`pcd_speculative_swap` mirrors that structure, including the copy-then-commit
shape: `pcd_clone_pair` takes the pair out of the list without disturbing it,
and `ctx.set_pair` writes back only on success. The class comparison is a
`TypeId` comparison on the simplification against the original, which is what
C's `astGetClass( smc0 ) == astGetClass( mc[0] )` compares — C interns class
names, so that is a pointer comparison of identities, not a string compare.

This is the same rule `MatrixMap` and `WinMap` apply in their own swap halves
(`matrix_merge.rs:912-947 (fn try_speculative_swap)`, `winmap_merge.rs:766-807 (fn try_speculative_swap)`), and
PcdMap now shares their shape rather than declining the case.

## Row 27 and row 34 — the swap arithmetic forms the factor the list applies

`PcdZoom` builds the swapped pair by reading the neighbour's `Zoom` attribute and
composing it with the PcdMap's centre and distortion. `Zoom` is direction
independent — the getter returns the stored factor whichever way the ZoomMap is
being applied, and it is `Transform` that reciprocates — so the factor the merge
list actually applies has to be formed explicitly
(`pcdmap.c:2374-2380 (fn PcdZoom)`), and the ZoomMap the swap creates is stored
with a cleared Invert flag so it holds that factor directly. The port does the
same in `mappings/simplify.rs:3005 (fn pcd_swap_pair)`, gated by
`pcdmap_swap_past_an_inverted_zoommap_preserves_the_transformation` in
`crates/ast-core/tests/test_pcdmap.rs`. C read the stored factor raw until
`3a4a2a7b`; `docs/issues/c-library-quirks.md` carries the history.

## Row 38 — declining in parallel is a step, not an omission

C states explicitly that PcdMaps cannot combine in parallel with anything, and
expresses it by putting the whole neighbour search inside `if( series )`. The
port returns `None` for `!series` before looking at a neighbour at all, which is
the same rule rather than an unimplemented branch.

## Row 40 — the convention, not just this class

`astMAKE_SET1` and `astMAKE_CLEAR1` for `Disco` both call `astClearIsSimple`, the
rule being that a setter whose value feeds the first part of the class's own
`MapMerge` must discard the record. `Disco` feeds it directly: a zero
coefficient is the reduction in rows 3 to 6.

`PcdCen` is set and cleared through class-local `MAKE_SET` and `MAKE_CLEAR`
macros rather than the `mapping.h` ones, because the attribute is axis indexed;
those macros clear the record in the same comma expression, and
`PcdMap::set_pcd_cen` (`pcdmap.rs:148-162 (fn set_pcd_cen)`) does the same. It matters because
`PcdCen` feeds `CanMerge`'s equality test.

See PLAN.md §9.2 for the whole convention and the classes it covers.

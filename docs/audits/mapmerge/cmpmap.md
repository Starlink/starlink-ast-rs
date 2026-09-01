# `MapMerge` audit — CmpMap

C source: reference/starlink-ast/src/cmpmap.c
Rust: crates/ast-core/src/mappings/cmpmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c (rows 1-42, `MapMerge`)
Audited: 2026-08-24 against pin 07fbe2dc (rows 43-76, `Simplify`)

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
| 16 | Parallel CmpMaps in series: require both Mappings to be parallel CmpMaps | cmpmap.c:1630-1635 | simplify.rs:2215-2227 | ported |
| 17 | Expand each into its list of parallel components with `astMapList` | cmpmap.c:1637-1645 | simplify.rs:2233-2238 | ported |
| 18 | Separate each list's Invert flags from its Mapping pointers | cmpmap.c:1647-1651 | simplify.rs:2233-2238 | ported |
| 19 | Assume at first that nothing will simplify | cmpmap.c:1653-1659 | simplify.rs:2245 | ported |
| 20 | Initialise the two list indices and the two empty sub-lists | cmpmap.c:1661-1668 | simplify.rs:2355-2361 | ported |
| 21 | Walk both lists, swapping Nin and Nout for a component applied inverted | cmpmap.c:1670-1691 | simplify.rs:2368-2381 | ported |
| 22 | Sub-list 1 has too few outputs: combine the next component into it, unsimplified | cmpmap.c:1693-1702 | simplify.rs:2244-2247 | ported |
| 23 | Sub-list 2 has too few inputs: combine the next component into it, unsimplified | cmpmap.c:1704-1713 | simplify.rs:2248-2251 | ported |
| 24 | Skip the pair on the first pass, when neither sub-list exists yet | cmpmap.c:1715-1721 | -- | n/a: the port advances one list before testing the boundary, so a pair is never formed empty |
| 25 | Combine the aligned pair in series and simplify the combination | cmpmap.c:1723-1729 | simplify.rs:2144-2159 | ported |
| 26 | Note that something simplified, if the pointer or the Invert flag changed | cmpmap.c:1730-1734 | simplify.rs:2161-2166 | ported |
| 27 | Add the simplified pair into the replacement parallel CmpMap | cmpmap.c:1736-1747 | simplify.rs:2252-2253,2260-2265 | ported |
| 28 | Reset both sub-lists for the next pair, or stop when either list runs out | cmpmap.c:1749-1767 | simplify.rs:2406-2415 | ported |
| 29 | Free the two component lists and their invert flags | cmpmap.c:1770-1783 | -- | n/a: owned values are dropped |
| 30 | Update the list: annul the pair, insert the replacement, close the gap and return the first modified index | cmpmap.c:1784-1815 | simplify.rs:906-908 (via `replace_pair_with`, simplify.rs:714) | ported |
| 31 | If merging in series and the previous Mapping is a PermMap, try swapping the CmpMap in front of it | cmpmap.c:1816-1845 | simplify.rs:827-849 | ported |
| 32 | Read both Mappings' Invert attributes and set them to the requested values | cmpmap.c:1846-1858 | simplify.rs:2287-2301 | ported |
| 33 | Read the shapes of the PermMap and of the CmpMap's two components | cmpmap.c:1855-1871 | simplify.rs:2303-2320 | ported |
| 34 | Require the PermMap outputs feeding each component to come from a contiguous input block | cmpmap.c:1872-1912 | simplify.rs:2321-2400 | ported |
| 35 | Transform a set of bad inputs through the PermMap to recover its constant outputs | cmpmap.c:1927-1950 | simplify.rs:1002 | ported |
| 36 | Substitute a UnitMap for a component whose inputs are all constant | cmpmap.c:1951-1966 | simplify.rs:2321-2400 | ported |
| 37 | Build the replacement PermMap's permutation and constant arrays for both components | cmpmap.c:1967-2043 | simplify.rs:2321-2400 | ported |
| 38 | Create the new PermMap and the new CmpMap | cmpmap.c:2044-2068 | simplify.rs:2321-2400 | ported |
| 39 | Re-instate the component and neighbour Invert attributes | cmpmap.c:2069-2095 | -- | n/a: nothing was mutated, so nothing is restored |
| 40 | On a successful swap, store the new CmpMap and PermMap in the two slots and return the first index | cmpmap.c:2096-2119 | simplify.rs:845-849 | ported |
| 41 | Clear the result value if an error occurred | cmpmap.c:2120-2122 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 42 | Return the result | cmpmap.c:2123-2124 | simplify.rs:801 | ported |

## Row 11 — the same decomposition, a different seam

C splices the two components into the Mapping list from inside `MapMerge`,
growing the list by one. The port decomposes before the nominate loop instead,
in `flatten` (`simplify.rs:231-310`, called at `simplify.rs:3397`), so by the
time a CmpMap is nominated the list is already flat. The reduction is the same;
what differs is that C's version reports a change and restarts the loop, while
the port's happens once up front.

## Rows 31 to 40 — the swap runs from C's nominee

C has this swap only on the CmpMap side: `permmap.c` contains no reference to
`CmpMap`, so a nominated PermMap never attempts it. The port now matches, with
the branch at the end of `cmp_map_merge` (`simplify.rs:827-849`) behind C's four
conditions — nothing simplified earlier, the nominee is not first in the list,
the nominee is a *parallel* CmpMap, and its lower neighbour is a PermMap — and
reporting `where_ - 1`, the PermMap's old slot, as C's `result = imap1` does.
Rows 32 to 38 point at `try_swap_permmap_parallel_at`, which holds the
arithmetic and was not changed. Row 32's set-and-restore has no counterpart
there: the applied direction is a flag beside each Mapping in the list, so the
helper reads it and picks the matching permutation arrays instead of mutating an
attribute.

Moving it did not change any fixture's output. Both triggers reach the same pair
and report the same slot, so the nominate loop resumes in the same place; what
differs is one nomination of ordering, visible in the trace and not in the
result. `cap_cmpperm_resume` pins that trace: PermMap at 1 returning -1 and the
parallel CmpMap at 2 returning 1, matching the instrumented C line for line
where the port previously returned 1 from the PermMap's own nomination. See
PLAN.md §10.4.

## The under-tagging fixtures

Rows 3 to 9 are the section the instrumented C shows stamping the surviving
sub-Mapping, and they are the ones `cap_cmpmap_01`, `cap_cmpmap_03`,
`cap_cmpmap_04` and `cap_specmap_02` turn on. They are marked `ported` because
the port performs each step; the disagreement is over which object ends up
being a whole simplification result, not over a missing step. Three of the four
now agree with C; `cap_specmap_02` is the one left. See PLAN.md §10.3.

## Rows 22, 23 and 26 — the branch now simplifies where C simplifies

C's parallel-CmpMaps-in-series branch simplifies in exactly one place: the pair it
has just combined (`tmap2 = astSimplify( tmap )`, `cmpmap.c:1728`, ported at
`simplify.rs:2156`). It accumulates sub-list members with
`CombineMaps( ..., 0, status )` and never simplifies them, sets `simpler` from an
identity test on the pointer and the Invert flag (`cmpmap.c:1731-1733`), and hands
the assembled parallel CmpMap to the list as built.

All three points now match. The sub-list members are cloned as they stand
(`simplify.rs:2244-2251`); the identity test is C's two terms — the result differs
from the input, or its stored orientation changed (`simplify.rs:2161-2166`), the
first term being the flag `simplify_mapping_reporting_change` returns rather than a
pointer comparison an owned-value port cannot make; and the reassembled parallel
CmpMap goes into the list unsimplified (row 30).

What the port had instead were two extra simplification passes, each running its
own merge loop — the mechanism behind the nomination counts in PLAN.md §10.3 — and
a leaf-count threshold in place of the identity test, which had no analogue
anywhere in `cmpmap.c` and decided whether the whole restructure fired.

Removing them made `cmpmap_parallel_series_dim_mismatch` and
`parpair_one_side_reduces` byte-exact, and moved `cap_specmap_02` from ten
`IsSimp` cards to seven against C's eight. It also showed that
`cap_cmpmap_01`/`_04` agreed with C on card *count* only by coincidence: the
per-member simplify tagged leaf classes C never tags in those trees while the
port was already missing CmpMap tags C sets. Both are compared by `astEqual`
only, so the count was the only gate on them. PLAN.md §10.3 carries the
measurements.

## `Simplify` — cmpmap.c:3396-3762

C gives CmpMap a `Simplify` body of its own, reached from `astSimplify_`
(`mapping.c:24772`), which is the slot's only caller. It is the simplification
engine for every composite Mapping: decompose the tree into a series or parallel
list, drive each component's `astMapMerge` from a nominate loop until the list
stops improving, and fold the survivors back into a right-associated CmpMap.

The Rust `CmpMap::simplified` (`cmpmap.rs:825-830`) delegates to
`simplify::simplify_mapping` (`simplify.rs:3403-3496`), so Rust sites below are in
`simplify.rs` unless stated. The `has_been_simplified` test at `cmpmap.rs:788` is
C's `astGetIsSimple` short-circuit, which C performs one level up in
`astSimplify_` (`mapping.c:24802-24804`) and the port performs both there
(`simplify_entry.rs:54`) and here.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 43 | Check the inherited status and bail out | cmpmap.c:3466 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 44 | Compare the supplied Mapping against every Mapping being simplified higher up the call stack, and return a clone on a match | cmpmap.c:3471-3491 | simplify.rs:126-131,3372-3380 | ported |
| 45 | Increment the recursion depth and record the supplied Mapping for deeper invocations to compare against | cmpmap.c:3493-3499 | simplify.rs:131-132 | ported |
| 46 | Obtain a pointer to the CmpMap structure | cmpmap.c:3501-3502 | simplify.rs:3557-3559 | ported |
| 47 | Initialise the Mapping-pointer and Invert-flag arrays | cmpmap.c:3504-3508 | -- | n/a: `flatten` returns an owned list of (Mapping, invert, orientation) triples, so there are no arrays to pre-initialise |
| 48 | Decompose the CmpMap into a series or parallel sequence with `astMapList`, setting `simpler` if any inverted CmpMap was found on the way | cmpmap.c:3510-3516 | simplify.rs:3579,3598 | ported |
| 49 | Read the RestrictedSimplify flag | cmpmap.c:3518-3524 | simplify.rs:3577 | ported |
| 50 | Under a restricted simplify, discard the `simpler` flag the decomposition set | cmpmap.c:3526-3530 | -- | gap: PLAN.md §10.20 |
| 51 | Make the list entries independent of one another, so modifying one does not modify another | cmpmap.c:3532-3535 | simplify.rs:370 | ported |
| 52 | Clear the Frozen flag on every component | cmpmap.c:3537-3545 | simplify.rs:627 | ported |
| 53 | Initialise the modified-index and Mapping-count histories | cmpmap.c:3547-3551 | simplify.rs:1708-1709 | ported |
| 54 | Loop until a complete pass produces no improvement | cmpmap.c:3553-3557 | simplify.rs:1595-1597 | ported |
| 55 | Loop to nominate each Mapping in the sequence in turn | cmpmap.c:3559-3561 | simplify.rs:1598-1599 | ported |
| 56 | Skip a frozen component, and under a restricted simplify one without AllowSimplify | cmpmap.c:3563-3573 | simplify.rs:1603-1606 | ported |
| 57 | Clone the nominated Mapping and call `astMapMerge` on it | cmpmap.c:3575-3580 | simplify.rs:1610-1612 | ported |
| 58 | Move on to nominate the next Mapping | cmpmap.c:3582-3583 | simplify.rs:1616 | ported |
| 59 | On a change, append the modified index and the new Mapping count to the two histories and check each for a repeating pattern | cmpmap.c:3585-3596 | simplify.rs:1618-1626 | ported |
| 60 | Order the two wavelengths and require the larger to be an integer multiple of the smaller | cmpmap.c:3598-3609 | simplify.rs:1627-1636 | ported |
| 61 | On a repeating pattern, freeze the modified Mapping so it stops contributing to the argument | cmpmap.c:3611-3614 | simplify.rs:1638-1641 | ported |
| 62 | Otherwise note the improvement, and step back to the modified Mapping if it precedes the next nominee | cmpmap.c:3616-3626 | simplify.rs:1643-1649 | ported |
| 63 | Free the two histories | cmpmap.c:3631-3633 | -- | n/a: owned `Vec`s are dropped |
| 64 | If nothing simplified, clone the supplied CmpMap rather than rebuilding it | cmpmap.c:3635-3641 | simplify.rs:3607-3623 | ported |
| 65 | A single-element list is returned as a copy, not a clone, so the result is independent of the component Mappings | cmpmap.c:3643-3658 | simplify.rs:3396 | ported |
| 66 | Set or clear the copy's Invert attribute as the list requires | cmpmap.c:3660-3666 | simplify.rs:3401-3403 | ported |
| 67 | For a longer list, note whether the last Mapping's Invert attribute is set, and its value | cmpmap.c:3668-3680 | simplify.rs:3396,3406 | ported |
| 68 | Set the last Mapping's Invert attribute to the value the list requires | cmpmap.c:3682-3683 | simplify.rs:3401-3403 | ported |
| 69 | Loop through the sequence in reverse, so each Mapping is combined with the CmpMap built so far | cmpmap.c:3685-3687 | simplify.rs:3394,3408 | ported |
| 70 | Clear the protected AllowSimplify flag on each component, now the sequence is simplified | cmpmap.c:3689-3692 | -- | gap: PLAN.md §10.20 |
| 71 | Clone the pointer to the last Mapping in the sequence, which is encountered first | cmpmap.c:3694-3697 | simplify.rs:3396 | ported |
| 72 | For each earlier Mapping, save its Invert state, set the value required, combine it into the CmpMap, and restore the state | cmpmap.c:3699-3723 | simplify.rs:3408-3448 | ported |
| 73 | Restore the last Mapping's Invert state once the whole sequence has been merged | cmpmap.c:3726-3733 | simplify.rs:3436-3438 | ported |
| 74 | Annul the Mapping pointers in the list and free the dynamic arrays | cmpmap.c:3738-3745 | -- | n/a: the owned list is dropped |
| 75 | Decrement the recursion depth, release the recorded Mapping, and free the stack at depth zero | cmpmap.c:3747-3755 | simplify.rs:136-142 | ported |
| 76 | Annul the result if an error occurred, and return it | cmpmap.c:3757-3761 | -- | n/a: an error is a `Result::Err`, so there is no result value to discard alongside it |

## Rows 44 and 45 — the recursion guard

C's guard is exact and unbounded: `astEqual` against each Mapping currently being
simplified higher up (`cmpmap.c:3487-3491`), over a stack that grows to whatever
depth the recursion reaches (`astGrow`, `cmpmap.c:3496`).

The port keeps the same list, as a `Vec<DynMapping>` pushed and popped by an RAII
guard, and tests it with `Mapping::equal`. C's separate depth counter has no
counterpart because the stack's own length is that number; nothing in the port
reads it.

The port previously compared a `u64` structural hash and refused outright past ten
nested simplifies. PLAN.md §10.19 records what those cost and how each was closed.

## Row 51 — the list is fully cloned, and the tags are copied back by hand

C's `SeparateMappings` (`cmpmap.c:3326`) deep-copies only *duplicate* component
pointers; every other list entry keeps aliasing the live child of the CmpMap being
simplified. `flatten` clones every leaf, so the port's list aliases nothing.

That difference is visible in one place. When the nominate loop's `astSimplify`
tags an entry `IsSimple` (`cmpmap.c:1437`), C writes through the alias onto the
real child, and the tag is still there when row 64 returns `astClone( this )`. The
port has to transplant those tags onto its fresh clone explicitly, which is what
`apply_component_stamps` (`simplify.rs:3620`) does, walking the tree in the order
`flatten` built the list.

The same ownership settles row 65, where C makes a point of `astCopy` rather than
`astClone` so that inverting a component through a pre-existing pointer cannot
affect the returned Mapping: `reconstruct` is handed the list by value, so the leaf
it returns is already independent of the input tree.

## Rows 50 and 70 — nothing in the port distinguishes a restricted simplify here

Row 56 is the one place the port honours `RestrictedSimplify`: a component without
`AllowSimplify` is never nominated. C uses the mode twice more — it declines to
count the decomposition itself as an improvement (row 50), and it clears
`AllowSimplify` off each component as it folds the result (row 70, with
`astSimplify_` clearing it on the result at `mapping.c:24786-24788`). Neither is
ported, so under a restricted simplify the port rebuilds trees C clones, and an
`AlSimp` card survives into the output. PLAN.md §10.20 records what would exercise
the mode: today nothing in the port sets either flag except deserialisation.

## No C row — where the series UnitMap strip lives

C removes a UnitMap from a series sequence in `UnitMap::MapMerge`
(`unitmap.c:516-538`), when the UnitMap is nominated, and the port ports that
(`unit.rs:112-115`, `unit.md` rows 5-7). That is now the only place the port
strips one: `merge_loop_restricted` is the nominate loop and nothing else.

It used to strip series UnitMaps twice more, at neither of which C strips
anything — inside a fixpoint loop whose `continue` re-ran the whole nominate loop,
and again after it. Both are removed. The whole tier-1/2 suite stays green,
including the two unit tests written against them
(`merge_loop_strips_unitmaps_from_series`,
`merge_loop_all_unitmaps_preserves_one`), which now exercise the strip through
`UnitMap::map_merge`. What the outer strips could not have got right: a UnitMap
without `AllowSimplify` is never nominated under a restricted simplify, so C cannot
strip it, while neither outer strip consulted the flag. PLAN.md §10.19.

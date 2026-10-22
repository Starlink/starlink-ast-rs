# `MapMerge` audit — CmpMap

C source: reference/starlink-ast/src/cmpmap.c
Rust: crates/ast-core/src/mappings/cmpmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c (rows 1-42, `MapMerge`)
Audited: 2026-08-24 against pin 07fbe2dc (rows 43-76, `Simplify`)
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- cmpmap.c:1540 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- cmpmap.c:2117 (fn MapMerge) — a section label whose four sentinel assignments are C's own; the condition it introduces is row 34 and its body is rows 35 to 38
- cmpmap.c:3633 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
- cmpmap.c:3666 (fn Simplify) — `astGET_GLOBALS` fetches C's thread-local block, which the port has no equivalent of

C's body runs `cmpmap.c:1614-2329 (fn MapMerge)` in five sections, named by its own banner
comments: simplify the CmpMap on its own; decompose a series CmpMap into the
list; merge adjacent CmpMaps; update the list for that merge; and swap a series
CmpMap past a neighboring PermMap.

The Rust `CmpMap::map_merge` (`cmpmap.rs:976-983 (fn map_merge)`) delegates to
`simplify::cmp_map_merge` (`mappings/simplify.rs:782-969 (fn cmp_map_merge)`), so Rust sites below are in
`mappings/simplify.rs` unless stated.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned index to -1 | cmpmap.c:1614-1615 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | cmpmap.c:1617-1618 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Simplify the CmpMap on its own: obtain a pointer to the nominated CmpMap | cmpmap.c:1620-1625 (fn MapMerge) | mappings/simplify.rs:793 (fn cmp_map_merge) | architecture | ported |
| 4 | Note whether the Invert attribute is set, and its value | cmpmap.c:1626-1630 (fn MapMerge) | mappings/simplify.rs:792 (fn cmp_map_merge) | architecture | ported |
| 5 | Change Invert to the value the list requests, so simplification can absorb it | cmpmap.c:1631-1639 (fn MapMerge) | mappings/simplify.rs:794-796 (fn cmp_map_merge) | architecture | ported |
| 6 | Simplify the Mapping and read the new Invert value | cmpmap.c:1640-1643 (fn MapMerge) | mappings/simplify.rs:801-829 (fn cmp_map_merge) | architecture | ported |
| 7 | Restore the original Mapping's Invert attribute | cmpmap.c:1644-1653 (fn MapMerge) | -- | ceremony | n/a: `oriented` is a clone, so the nominee is never mutated and there is nothing to restore |
| 8 | Determine whether simplification occurred, by pointer identity or a changed Invert | cmpmap.c:1654-1661 (fn MapMerge) | mappings/simplify.rs:808-817 (fn cmp_map_merge) | architecture | ported |
| 9 | On success, replace the list entry with the new Mapping and its invert flag, and return | cmpmap.c:1662-1672 (fn MapMerge) | mappings/simplify.rs:846-847 (fn cmp_map_merge) | architecture | ported |
| 10 | Otherwise annul the new Mapping pointer | cmpmap.c:1673-1675 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 11 | If a series CmpMap is being combined in series, replace it in the list with its two components | cmpmap.c:1677-1731 (fn MapMerge) | mappings/simplify.rs:903 (fn cmp_map_merge) | architecture | ported |
| 12 | If no simplification was done, look for a neighboring CmpMap to merge with | cmpmap.c:1732-1740 (fn MapMerge) | mappings/simplify.rs:921 (fn cmp_map_merge) | architecture | ported |
| 13 | Obtain the pair of indices to merge, taking (0,1) when the nominee is first | cmpmap.c:1741-1751 (fn MapMerge) | mappings/simplify.rs:922-923 (fn cmp_map_merge) | architecture | ported |
| 14 | Require the neighbor to be a CmpMap, and obtain both pointers and their invert flags | cmpmap.c:1752-1780 (fn MapMerge) | mappings/simplify.rs:924-930 (fn cmp_map_merge) | architecture | ported |
| 15 | Series CmpMaps in parallel: re-arrange the sub-Mappings into two parallel CmpMaps and simplify each | cmpmap.c:1781-1827 (fn MapMerge) | mappings/simplify.rs:929 (fn cmp_map_merge) | architecture | ported |
| 16 | Parallel CmpMaps in series: require both Mappings to be parallel CmpMaps | cmpmap.c:1828-1833 (fn MapMerge) | mappings/simplify.rs:2367-2380 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 17 | Expand each into its list of parallel components with `astMapList` | cmpmap.c:1835-1843 (fn MapMerge) | mappings/simplify.rs:2385-2386 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 18 | Ensure the two lists' Mappings are independent of one another | cmpmap.c:1845-1849 (fn MapMerge) | mappings/simplify.rs:398 (fn leaf_entry) | architecture | ported |
| 19 | Assume at first that nothing will simplify | cmpmap.c:1851-1857 (fn MapMerge) | mappings/simplify.rs:2396 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 20 | Initialize the two list indices and the two empty sub-lists | cmpmap.c:1859-1866 (fn MapMerge) | mappings/simplify.rs:2465-2471 (fn dimension_match_parallel_lists) | architecture | ported |
| 21 | Walk both lists, swapping Nin and Nout for a component applied inverted | cmpmap.c:1868-1889 (fn MapMerge) | mappings/simplify.rs:2478-2491 (fn dimension_match_parallel_lists) | architecture | ported |
| 22 | Sub-list 1 has too few outputs: combine the next component into it, unsimplified | cmpmap.c:1891-1900 (fn MapMerge) | mappings/simplify.rs:2480-2483 (fn dimension_match_parallel_lists) | architecture | ported |
| 23 | Sub-list 2 has too few inputs: combine the next component into it, unsimplified | cmpmap.c:1902-1911 (fn MapMerge) | mappings/simplify.rs:2484-2487 (fn dimension_match_parallel_lists) | architecture | ported |
| 24 | Skip the pair on the first pass, when neither sub-list exists yet | cmpmap.c:1913-1919 (fn MapMerge) | -- | architecture | n/a: the port advances one list before testing the boundary, so a pair is never formed empty |
| 25 | Combine the aligned pair in series and simplify the combination | cmpmap.c:1921-1927 (fn MapMerge) | mappings/simplify.rs:2399-2409 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 26 | Note that something simplified, if the pointer or the Invert flag changed | cmpmap.c:1928-1932 (fn MapMerge) | mappings/simplify.rs:2414-2419 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 27 | Add the simplified pair into the replacement parallel CmpMap | cmpmap.c:1934-1945 (fn MapMerge) | mappings/simplify.rs:2440-2452 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 28 | Reset both sub-lists for the next pair, or stop when either list runs out | cmpmap.c:1947-1965 (fn MapMerge) | mappings/simplify.rs:2507-2515 (fn dimension_match_parallel_lists) | architecture | ported |
| 29 | Free the two component lists and their invert flags | cmpmap.c:1968-1981 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 30 | Update the list: annul the pair, insert the replacement, close the gap and return the first modified index | cmpmap.c:1988-2019 (fn MapMerge) | mappings/simplify.rs:678-679 (fn replace_pair_with), mappings/simplify.rs:932-933 (fn cmp_map_merge) | architecture | ported |
| 31 | If merging in series and the previous Mapping is a PermMap, try swapping the CmpMap in front of it | cmpmap.c:2020-2049 (fn MapMerge) | mappings/simplify.rs:954-966 (fn cmp_map_merge) | architecture | ported |
| 32 | Read both Mappings' Invert attributes and set them to the requested values | cmpmap.c:2050-2062 (fn MapMerge) | mappings/simplify.rs:2624-2633 (fn swap_permmap_parallel_at) | architecture | ported |
| 33 | Read the shapes of the PermMap and of the CmpMap's two components, taken with the Invert values they had when the CmpMap was created | cmpmap.c:2059-2075 (fn MapMerge), cmpmap.c:2124-2129 (fn MapMerge) | mappings/simplify.rs:2635-2659 (fn swap_permmap_parallel_at) | architecture | ported |
| 34 | Require the PermMap outputs feeding each component to come from a contiguous input block | cmpmap.c:2076-2116 (fn MapMerge) | mappings/simplify.rs:2661-2694 (fn swap_permmap_parallel_at) | architecture | ported |
| 35 | Transform a set of bad inputs through the PermMap to recover its constant outputs | cmpmap.c:2131-2154 (fn MapMerge) | mappings/simplify.rs:2718-2746 (fn swap_permmap_parallel_at) | architecture | ported |
| 36 | Substitute a UnitMap for a component whose inputs are all constant | cmpmap.c:2155-2170 (fn MapMerge) | mappings/simplify.rs:2843-2860 (fn swap_permmap_parallel_at) | architecture | ported |
| 37 | Build the replacement PermMap's permutation and constant arrays for both components | cmpmap.c:2171-2247 (fn MapMerge) | mappings/simplify.rs:2749-2828 (fn swap_permmap_parallel_at) | architecture | ported |
| 38 | Create the new PermMap and the new CmpMap | cmpmap.c:2248-2272 (fn MapMerge) | mappings/simplify.rs:2862-2882 (fn swap_permmap_parallel_at) | architecture | ported |
| 39 | Free the workspace, then re-instate the component and neighbor Invert attributes | cmpmap.c:2273-2299 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and nothing was mutated so nothing is restored |
| 40 | On a successful swap, store the new CmpMap and PermMap in the two slots and return the first index | cmpmap.c:2300-2323 (fn MapMerge) | mappings/simplify.rs:2875-2887 (fn swap_permmap_parallel_at), mappings/simplify.rs:963-965 (fn cmp_map_merge) | architecture | ported |
| 41 | Clear the result value if an error occurred | cmpmap.c:2324-2326 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 42 | Return the result | cmpmap.c:2327-2328 (fn MapMerge) | mappings/simplify.rs:968 (fn cmp_map_merge) | ceremony | ported |

## Row 11 — the same decomposition, a different seam

C splices the two components into the Mapping list from inside `MapMerge`,
growing the list by one. The port decomposes before the nominate loop instead,
in `flatten` (`mappings/simplify.rs:363-368 (fn flatten)`, called at `mappings/simplify.rs:3881 (fn simplify_cmpmap_into)`), so by the
time a CmpMap is nominated the list is already flat. The reduction is the same;
what differs is that C's version reports a change and restarts the loop, while
the port's happens once up front.

## Rows 31 to 40 — the swap runs from C's nominee

C has this swap only on the CmpMap side: `permmap.c` contains no reference to
`CmpMap`, so a nominated PermMap never attempts it. The port now matches, with
the branch at the end of `cmp_map_merge` (`mappings/simplify.rs:954-966 (fn cmp_map_merge)`) behind C's four
conditions — nothing simplified earlier, the nominee is not first in the list,
the nominee is a *parallel* CmpMap, and its lower neighbor is a PermMap — and
reporting `where_ - 1`, the PermMap's old slot, as C's `result = imap1` does.
Rows 32 to 38 point at `swap_permmap_parallel_at`, which holds the arithmetic;
its transforms of the PermMap's constants (row 35) return their errors, which
`try_swap_permmap_parallel_at` records on the merge context so the
simplification fails as C's does. Row 32's set-and-restore has no counterpart
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
has just combined (`tmap2 = astSimplify( tmap )`, `cmpmap.c:1926 (fn MapMerge)`, ported at
`mappings/simplify.rs:2409 (fn compute_merge_parallel_pair_in_series)`). It accumulates sub-list members with
`CombineMaps( ..., 0, status )` and never simplifies them, sets `simpler` from an
identity test on the pointer and the Invert flag (`cmpmap.c:1929-1931 (fn MapMerge)`), and hands
the assembled parallel CmpMap to the list as built.

All three points now match. The sub-list members are cloned as they stand
(`mappings/simplify.rs:2498-2505 (fn dimension_match_parallel_lists)`); the identity test is C's two terms — the result differs
from the input, or its stored orientation changed (`mappings/simplify.rs:2414-2419 (fn compute_merge_parallel_pair_in_series)`), the
first term being the outcome `simplify_mapping_with_outcome` returns rather than a
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

## `Simplify` — cmpmap.c:3594-3960 (fn Simplify)

C gives CmpMap a `Simplify` body of its own, reached from `astSimplify_`
(`mapping.c:24772 (fn astSimplify_)`), which is the slot's only caller. It is the simplification
engine for every composite Mapping: decompose the tree into a series or parallel
list, drive each component's `astMapMerge` from a nominate loop until the list
stops improving, and fold the survivors back into a right-associated CmpMap.

The Rust `CmpMap::simplified` (`cmpmap.rs:969-974 (fn simplified)`) passes its gate to
`simplify::simplify_cmpmap` (`mappings/simplify.rs:3855-3946 (fn simplify_cmpmap_into)`), so Rust sites below are in
`mappings/simplify.rs` unless stated. The `has_been_simplified` test at `cmpmap.rs:970 (fn simplified)` is
C's `astGetIsSimple` short-circuit, which C performs one level up in
`astSimplify_` (`mapping.c:24802-24804 (fn astSimplify_)`) and the port performs both there
(`simplify_entry.rs:134 (fn simplify_with_outcome)`) and here.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 43 | Initialize the result, check the inherited status and bail out | cmpmap.c:3660-3664 (fn Simplify) | -- | ceremony | n/a: the Rust signature hands back an owned Mapping in an `AstResult`, so there is no result pointer to initialize and no ambient status to test on entry |
| 44 | Compare the supplied Mapping against every Mapping being simplified higher up the call stack, and return a clone on a match | cmpmap.c:3669-3689 (fn Simplify) | mappings/simplify.rs:60-63 (fn enter), mappings/simplify.rs:3865-3872 (fn simplify_cmpmap_into) | architecture | ported |
| 45 | Increment the recursion depth and record the supplied Mapping for deeper invocations to compare against | cmpmap.c:3691-3697 (fn Simplify) | mappings/simplify.rs:64 (fn enter) | architecture | ported |
| 46 | Obtain a pointer to the CmpMap structure | cmpmap.c:3699-3700 (fn Simplify) | cmpmap.rs:969-974 (fn simplified), mappings/simplify.rs:3854-3858 (fn simplify_cmpmap_into) | ceremony | ported |
| 47 | Initialize the Mapping-pointer and Invert-flag arrays | cmpmap.c:3702-3706 (fn Simplify) | -- | ceremony | n/a: `flatten` returns an owned list of (Mapping, invert, orientation) triples, so there are no arrays to pre-initialize |
| 48 | Decompose the CmpMap into a series or parallel sequence with `astMapList`, setting `simpler` if any inverted CmpMap was found on the way | cmpmap.c:3708-3714 (fn Simplify) | mappings/simplify.rs:3881 (fn simplify_cmpmap_into), mappings/simplify.rs:3911 (fn simplify_cmpmap_into) | architecture | ported |
| 49 | Read the RestrictedSimplify flag | cmpmap.c:3716-3722 (fn Simplify) | mappings/simplify.rs:3875 (fn simplify_cmpmap_into) | architecture | ported |
| 50 | Under a restricted simplify, discard the `simpler` flag the decomposition set | cmpmap.c:3724-3728 (fn Simplify) | mappings/simplify.rs:3911 (fn simplify_cmpmap_into) | architecture | ported |
| 51 | Make the list entries independent of one another, so modifying one does not modify another | cmpmap.c:3730-3733 (fn Simplify) | mappings/simplify.rs:398 (fn leaf_entry) | architecture | ported |
| 52 | Clear the Frozen flag on every component | cmpmap.c:3735-3743 (fn Simplify) | mappings/simplify.rs:525 (fn new) | architecture | ported |
| 53 | Initialize the modified-index and Mapping-count histories | cmpmap.c:3745-3749 (fn Simplify) | mappings/simplify.rs:1941-1942 (fn merge_loop_restricted) | architecture | ported |
| 54 | Loop until a complete pass produces no improvement | cmpmap.c:3751-3755 (fn Simplify) | mappings/simplify.rs:1836-1838 (fn run_nominate_loop) | architecture | ported |
| 55 | Loop to nominate each Mapping in the sequence in turn | cmpmap.c:3757-3759 (fn Simplify) | mappings/simplify.rs:1839-1840 (fn run_nominate_loop) | architecture | ported |
| 56 | Skip a frozen component, and under a restricted simplify one without AllowSimplify | cmpmap.c:3761-3771 (fn Simplify) | mappings/simplify.rs:1845-1848 (fn run_nominate_loop) | architecture | ported |
| 57 | Clone the nominated Mapping and call `astMapMerge` on it | cmpmap.c:3773-3778 (fn Simplify) | mappings/simplify.rs:1851-1855 (fn run_nominate_loop) | architecture | ported |
| 58 | Move on to nominate the next Mapping | cmpmap.c:3780-3781 (fn Simplify) | mappings/simplify.rs:1863 (fn run_nominate_loop) | architecture | ported |
| 59 | On a change, append the modified index and the new Mapping count to the two histories and check each for a repeating pattern | cmpmap.c:3783-3794 (fn Simplify) | mappings/simplify.rs:1872-1873 (fn run_nominate_loop) | architecture | ported |
| 60 | Order the two wavelengths and require the larger to be an integer multiple of the smaller | cmpmap.c:3796-3807 (fn Simplify) | mappings/simplify.rs:1874-1883 (fn run_nominate_loop) | architecture | ported |
| 61 | On a repeating pattern, freeze the modified Mapping so it stops contributing to the argument | cmpmap.c:3809-3812 (fn Simplify) | mappings/simplify.rs:1885-1889 (fn run_nominate_loop) | architecture | ported |
| 62 | Otherwise note the improvement, and step back to the modified Mapping if it precedes the next nominee | cmpmap.c:3814-3824 (fn Simplify) | mappings/simplify.rs:1890-1898 (fn run_nominate_loop) | architecture | ported |
| 63 | Free the two histories | cmpmap.c:3829-3831 (fn Simplify) | -- | ceremony | n/a: owned `Vec`s are dropped |
| 64 | If nothing simplified, clone the supplied CmpMap rather than rebuilding it | cmpmap.c:3833-3839 (fn Simplify) | mappings/simplify.rs:3911-3925 (fn simplify_cmpmap_into) | architecture | ported |
| 65 | A single-element list is returned as a copy, not a clone, so the result is independent of the component Mappings | cmpmap.c:3841-3856 (fn Simplify) | mappings/simplify.rs:3576-3583 (fn reconstruct_inner) | architecture | ported |
| 66 | Set or clear the copy's Invert attribute as the list requires | cmpmap.c:3858-3864 (fn Simplify) | mappings/simplify.rs:3581-3583 (fn reconstruct_inner) | architecture | ported |
| 67 | For a longer list, note whether the last Mapping's Invert attribute is set, and its value | cmpmap.c:3866-3878 (fn Simplify) | mappings/simplify.rs:3576-3586 (fn reconstruct_inner) | architecture | ported |
| 68 | Set the last Mapping's Invert attribute to the value the list requires | cmpmap.c:3880-3881 (fn Simplify) | mappings/simplify.rs:3581-3583 (fn reconstruct_inner) | architecture | ported |
| 69 | Loop through the sequence in reverse, so each Mapping is combined with the CmpMap built so far | cmpmap.c:3883-3885 (fn Simplify) | mappings/simplify.rs:3588 (fn reconstruct_inner) | architecture | ported |
| 70 | Clear the protected AllowSimplify flag on each component, now the sequence is simplified | cmpmap.c:3887-3890 (fn Simplify) | mappings/simplify.rs:3941-3945 (fn simplify_cmpmap_into) | architecture | ported |
| 71 | Clone the pointer to the last Mapping in the sequence, which is encountered first | cmpmap.c:3892-3895 (fn Simplify) | mappings/simplify.rs:3576 (fn reconstruct_inner) | architecture | ported |
| 72 | For each earlier Mapping, save its Invert state, set the value required, combine it into the CmpMap, and restore the state | cmpmap.c:3897-3921 (fn Simplify) | mappings/simplify.rs:3588-3631 (fn reconstruct_inner) | architecture | ported |
| 73 | Restore the last Mapping's Invert state once the whole sequence has been merged | cmpmap.c:3924-3931 (fn Simplify) | mappings/simplify.rs:3616-3618 (fn reconstruct_inner) | architecture | ported |
| 74 | Annul the Mapping pointers in the list and free the dynamic arrays | cmpmap.c:3936-3943 (fn Simplify) | -- | ceremony | n/a: the owned list is dropped |
| 75 | Decrement the recursion depth, release the recorded Mapping, and free the stack at depth zero | cmpmap.c:3945-3953 (fn Simplify) | mappings/simplify.rs:70-73 (fn drop) | architecture | ported |
| 76 | Annul the result if an error occurred, and return it | cmpmap.c:3955-3959 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, so there is no result value to discard alongside it |

## Rows 44 and 45 — the recursion guard

C's guard is exact and unbounded: `astEqual` against each Mapping currently being
simplified higher up (`cmpmap.c:3685-3689 (fn Simplify)`), over a stack that grows to whatever
depth the recursion reaches (`astGrow`, `cmpmap.c:3694 (fn Simplify)`).

The port keeps the same list, as a `Vec<DynMapping>` pushed and popped by an RAII
guard, and tests it with `Mapping::equal`. C's separate depth counter has no
counterpart because the stack's own length is that number; nothing in the port
reads it.

The port previously compared a `u64` structural hash and refused outright past ten
nested simplifies. PLAN.md §10.19 records what those cost and how each was closed.

## Row 51 — the list is fully cloned, and the tags are copied back by hand

C's `SeparateMappings` (`cmpmap.c:3530 (fn SeparateMappings)`) deep-copies only *duplicate* component
pointers; every other list entry keeps aliasing the live child of the CmpMap being
simplified. `flatten` clones every leaf, so the port's list aliases nothing.

That difference is visible in one place. When the nominate loop's `astSimplify`
tags an entry `IsSimple` (`cmpmap.c:1641 (fn MapMerge)`), C writes through the alias onto the
real child, and the tag is still there when row 64 returns `astClone( this )`. The
port has to transplant those tags onto its fresh clone explicitly, which is what
`stamped_tags` (`mappings/simplify.rs:3742-3795 (fn stamped_tags)`) computes, walking the tree in the order
`flatten` built the list.

The same ownership settles row 65, where C makes a point of `astCopy` rather than
`astClone` so that inverting a component through a pre-existing pointer cannot
affect the returned Mapping: `reconstruct` is handed the list by value, so the leaf
it returns is already independent of the input tree.

## Rows 50 and 70 — the three places the mode changes the answer

C consults `RestrictedSimplify` three times, and the port now does all three. A
component without `AllowSimplify` is never nominated (row 56). The decomposition
on its own does not count as an improvement, because C cannot yet know whether an
inverted CmpMap it met will be eligible (row 50) — so a restricted simplify that
merges nothing returns `astClone( this )` and the supplied tree survives
unswapped. And the protected `AllowSimplify` flag is cleared off each component
as the sequence is folded back into a CmpMap (row 70), with `astSimplify_`
clearing it on the result whichever branch ran
(`mapping.c:24786-24788 (fn astSimplify_)`, `simplify_entry.rs:181 (fn record_simplified)`) so it cannot reach a public
dump. The base driver's own gate for an ineligible Mapping
(`mapping.c:16853-16855 (fn Simplify)`) is at `mappings/simplify.rs:1446-1448 (fn base_simplify)`.

Three fixtures hold the mode, one per behavior: `cap_restricted_no_eligible`
(nothing eligible, so the inverted CmpMap comes back as supplied),
`cap_restricted_inverted` (one eligible ZoomMap whose own merge reaches across
its ineligible neighbors and collapses the chain), and
`cap_restricted_component_flags` (a rebuild that keeps an eligible component,
whose `AlSimp` card the fold clears).

## No C row — where the series UnitMap strip lives

C removes a UnitMap from a series sequence in `UnitMap::MapMerge`
(`unitmap.c:516-538 (fn MapMerge)`), when the UnitMap is nominated, and the port ports that
(`unit.rs:168-171 (fn map_merge)`, `unit.md` rows 5-7). That is now the only place the port
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

## Rows 11 and 48 — the `DoNotSimplify` gate on the decomposition

C's `MapList` decomposes a CmpMap only when it is the right kind *and*
`astDoNotSimplify` returns zero (`cmpmap.c:1352 (fn MapList)`); otherwise it falls through to
the base `MapList`, which appends the Mapping itself as one leaf. The port has
the same condition on `flatten`'s decomposing branch (`mappings/simplify.rs:365 (fn flatten)`), and the
non-decomposing path was already the single-leaf append, so a named CmpMap
reaches the nominate loop intact. The companion gate in the public entry
(`mapping.c:24764 (fn astSimplify_)`, `simplify_entry.rs:134 (fn simplify_with_outcome)`) shares its `astClone( this )` exit
with the `IsSimple` short-circuit, as C's one expression does.

`CmpMap::Equal` (`cmpmap.c:458 (fn Equal)`) is the reason the port needed a second
decomposition entry point. C compares two CmpMaps by decomposing both with
`astMapList` and walking the lists element by element (`cmpmap.c:537 (fn Equal)`, `cmpmap.c:544 (fn Equal)`, `cmpmap.c:596 (fn Equal)`).
When both sides refuse to decompose, each list holds one element — the CmpMap it
came from — so the element-wise comparison is the comparison it started with and
C recurses without bound. It is unreachable from C's own callers only because
one side almost never carries an `Ident`. The port terminates instead: when both
sides refuse, `equal` calls `flatten_children` (`mappings/simplify.rs:425 (fn flatten_children)`), which runs
the body of C's decomposing branch on the two children directly. That is the
comparison C makes with either side's flag cleared, one level down, so no answer
C can actually produce changes.

`cap_ident_cmpmap` holds the gate and `cap_ident_frame` the `Frame` override.
PLAN.md §10.21.

# `MapMerge` audit — CmpMap

C source: reference/starlink-ast/src/cmpmap.c
Rust: crates/starlink-ast/src/mappings/cmpmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c (rows 1-42, `MapMerge`)
Audited: 2026-08-24 against pin 07fbe2dc (rows 43-76, `Simplify`)
Citations: verified against pin fe26cc9b
Extension points: classified

Uncited C steps:
- cmpmap.c:1571 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- cmpmap.c:2172 (fn MapMerge) — a section label whose four sentinel assignments are C's own; the condition it introduces is row 34 and its body is rows 35 to 38
- cmpmap.c:3737 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
- cmpmap.c:3770 (fn Simplify) — `astGET_GLOBALS` fetches C's thread-local block, which the port has no equivalent of


C's body runs `cmpmap.c:1645-2376 (fn MapMerge)` in five sections, named by its own banner
comments: simplify the CmpMap on its own; decompose a series CmpMap into the
list; merge adjacent CmpMaps; update the list for that merge; and swap a series
CmpMap past a neighboring PermMap.

The Rust `CmpMap::map_merge` (`cmpmap.rs:1048-1055 (fn map_merge)`) delegates to
`simplify::cmp_map_merge` (`mappings/simplify.rs:793-980 (fn cmp_map_merge)`), so Rust sites below are in
`mappings/simplify.rs` unless stated.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned index to -1 | cmpmap.c:1645-1646 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | cmpmap.c:1648-1649 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Simplify the CmpMap on its own: obtain a pointer to the nominated CmpMap | cmpmap.c:1651-1656 (fn MapMerge) | mappings/simplify.rs:804 (fn cmp_map_merge) | architecture | ported |
| 4 | Note whether the Invert attribute is set, and its value | cmpmap.c:1657-1661 (fn MapMerge) | mappings/simplify.rs:803 (fn cmp_map_merge) | architecture | ported |
| 5 | Change Invert to the value the list requests, so simplification can absorb it | cmpmap.c:1662-1670 (fn MapMerge) | mappings/simplify.rs:805-807 (fn cmp_map_merge) | architecture | ported |
| 6 | Simplify the Mapping and read the new Invert value | cmpmap.c:1671-1674 (fn MapMerge) | mappings/simplify.rs:812-840 (fn cmp_map_merge) | architecture | ported |
| 7 | Restore the original Mapping's Invert attribute | cmpmap.c:1675-1684 (fn MapMerge) | -- | ceremony | n/a: `oriented` is a clone, so the nominee is never mutated and there is nothing to restore |
| 8 | Determine whether simplification occurred, by pointer identity or a changed Invert | cmpmap.c:1685-1692 (fn MapMerge) | mappings/simplify.rs:819-828 (fn cmp_map_merge) | architecture | ported |
| 9 | On success, replace the list entry with the new Mapping and its invert flag, and return | cmpmap.c:1693-1703 (fn MapMerge) | mappings/simplify.rs:857-858 (fn cmp_map_merge) | architecture | ported |
| 10 | Otherwise annul the new Mapping pointer | cmpmap.c:1704-1706 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 11 | If a series CmpMap is being combined in series, replace it in the list with its two components | cmpmap.c:1708-1762 (fn MapMerge) | mappings/simplify.rs:914 (fn cmp_map_merge) | architecture | ported |
| 12 | If no simplification was done, look for a neighboring CmpMap to merge with | cmpmap.c:1763-1771 (fn MapMerge) | mappings/simplify.rs:932 (fn cmp_map_merge) | architecture | ported |
| 13 | Obtain the pair of indices to merge, taking (0,1) when the nominee is first | cmpmap.c:1772-1782 (fn MapMerge) | mappings/simplify.rs:933-934 (fn cmp_map_merge) | architecture | ported |
| 14 | Require the neighbor to be a CmpMap, and obtain both pointers and their invert flags | cmpmap.c:1783-1811 (fn MapMerge) | mappings/simplify.rs:935-941 (fn cmp_map_merge) | architecture | ported |
| 15 | Series CmpMaps in parallel: re-arrange the sub-Mappings into two parallel CmpMaps and simplify each | cmpmap.c:1812-1858 (fn MapMerge) | mappings/simplify.rs:940 (fn cmp_map_merge) | architecture | ported |
| 16 | Parallel CmpMaps in series: require both Mappings to be parallel CmpMaps | cmpmap.c:1859-1864 (fn MapMerge) | mappings/simplify.rs:2484-2497 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 17 | Expand each into its list of parallel components with `astMapList` | cmpmap.c:1866-1874 (fn MapMerge) | mappings/simplify.rs:2502-2503 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 18 | Ensure the two lists' Mappings are independent of one another | cmpmap.c:1876-1880 (fn MapMerge) | mappings/simplify.rs:403 (fn leaf_entry) | architecture | ported |
| 19 | Assume at first that nothing will simplify | cmpmap.c:1882-1888 (fn MapMerge) | mappings/simplify.rs:2513 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 20 | Initialize the two list indices and the two empty sub-lists | cmpmap.c:1890-1897 (fn MapMerge) | mappings/simplify.rs:2582-2588 (fn dimension_match_parallel_lists) | architecture | ported |
| 21 | Walk both lists, swapping Nin and Nout for a component applied inverted | cmpmap.c:1899-1920 (fn MapMerge) | mappings/simplify.rs:2595-2608 (fn dimension_match_parallel_lists) | architecture | ported |
| 22 | Sub-list 1 has too few outputs: combine the next component into it, unsimplified | cmpmap.c:1922-1931 (fn MapMerge) | mappings/simplify.rs:2597-2600 (fn dimension_match_parallel_lists) | architecture | ported |
| 23 | Sub-list 2 has too few inputs: combine the next component into it, unsimplified | cmpmap.c:1933-1942 (fn MapMerge) | mappings/simplify.rs:2601-2604 (fn dimension_match_parallel_lists) | architecture | ported |
| 24 | Skip the pair on the first pass, when neither sub-list exists yet | cmpmap.c:1944-1950 (fn MapMerge) | -- | architecture | n/a: the port advances one list before testing the boundary, so a pair is never formed empty |
| 25 | Combine the aligned pair in series and simplify the combination | cmpmap.c:1952-1958 (fn MapMerge) | mappings/simplify.rs:2516-2526 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 26 | Note that something simplified, if the pointer or the Invert flag changed | cmpmap.c:1959-1963 (fn MapMerge) | mappings/simplify.rs:2531-2536 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 27 | Add the simplified pair into the replacement parallel CmpMap | cmpmap.c:1965-1976 (fn MapMerge) | mappings/simplify.rs:2557-2569 (fn compute_merge_parallel_pair_in_series) | architecture | ported |
| 28 | Reset both sub-lists for the next pair, or stop when either list runs out | cmpmap.c:1978-1996 (fn MapMerge) | mappings/simplify.rs:2624-2632 (fn dimension_match_parallel_lists) | architecture | ported |
| 29 | Free the two component lists and their invert flags | cmpmap.c:1999-2012 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 30 | Update the list: annul the pair, insert the replacement, close the gap and return the first modified index | cmpmap.c:2019-2050 (fn MapMerge) | mappings/simplify.rs:689-690 (fn replace_pair_with), mappings/simplify.rs:943-944 (fn cmp_map_merge) | architecture | ported |
| 31 | If merging in series and the previous Mapping is a PermMap, try swapping the CmpMap in front of it | cmpmap.c:2051-2080 (fn MapMerge) | mappings/simplify.rs:965-977 (fn cmp_map_merge) | architecture | ported |
| 32 | Read both Mappings' Invert attributes and set them to the requested values | cmpmap.c:2081-2093 (fn MapMerge) | mappings/simplify.rs:2741-2750 (fn swap_permmap_parallel_at) | architecture | ported |
| 33 | Read the shapes of the PermMap and of the CmpMap's two components, taken with the Invert values the CmpMap uses them with (the opposite of those it was created with when it is used inverted) | cmpmap.c:2090-2121 (fn MapMerge) | mappings/simplify.rs:2752-2774 (fn swap_permmap_parallel_at) | architecture | ported |
| 34 | Require the PermMap outputs feeding each component to come from a contiguous input block, or to be constant with none of the inputs that would feed them copied from an output; refuse when every output is constant | cmpmap.c:2122-2171 (fn MapMerge) | mappings/simplify.rs:2776-2820 (fn swap_permmap_parallel_at) | architecture | ported |
| 35 | Transform a set of bad inputs through the PermMap to recover its constant outputs | cmpmap.c:2179-2202 (fn MapMerge) | mappings/simplify.rs:2845-2873 (fn swap_permmap_parallel_at) | architecture | ported |
| 36 | Substitute a UnitMap for a component whose inputs are all constant | cmpmap.c:2203-2218 (fn MapMerge) | mappings/simplify.rs:2970-2987 (fn swap_permmap_parallel_at) | architecture | ported |
| 37 | Build the replacement PermMap's permutation and constant arrays for both components | cmpmap.c:2219-2295 (fn MapMerge) | mappings/simplify.rs:2876-2955 (fn swap_permmap_parallel_at) | architecture | ported |
| 38 | Create the new PermMap and the new CmpMap | cmpmap.c:2296-2320 (fn MapMerge) | mappings/simplify.rs:2988-3007 (fn swap_permmap_parallel_at) | architecture | ported |
| 39 | Free the workspace, then re-instate the component and neighbor Invert attributes | cmpmap.c:2321-2346 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and nothing was mutated so nothing is restored |
| 40 | On a successful swap, store the new CmpMap and PermMap in the two slots and return the first index | cmpmap.c:2347-2370 (fn MapMerge) | mappings/simplify.rs:3000-3010 (fn swap_permmap_parallel_at), mappings/simplify.rs:974-976 (fn cmp_map_merge) | architecture | ported |
| 41 | Clear the result value if an error occurred | cmpmap.c:2371-2373 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 42 | Return the result | cmpmap.c:2374-2375 (fn MapMerge) | mappings/simplify.rs:979 (fn cmp_map_merge) | ceremony | ported |

## Row 11 — the same decomposition, a different seam

C splices the two components into the Mapping list from inside `MapMerge`,
growing the list by one. The port decomposes before the nominate loop instead,
in `flatten` (`mappings/simplify.rs:368-373 (fn flatten)`, called at `mappings/simplify.rs:4018 (fn simplify_cmpmap_into)`), so by the
time a CmpMap is nominated the list is already flat. The reduction is the same;
what differs is that C's version reports a change and restarts the loop, while
the port's happens once up front.

## Rows 31 to 40 — the swap runs from C's nominee

C has this swap only on the CmpMap side: `permmap.c` contains no reference to
`CmpMap`, so a nominated PermMap never attempts it. The port now matches, with
the branch at the end of `cmp_map_merge` (`mappings/simplify.rs:965-977 (fn cmp_map_merge)`) behind C's four
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
has just combined (`tmap2 = astSimplify( tmap )`, `cmpmap.c:1957 (fn MapMerge)`, ported at
`mappings/simplify.rs:2526 (fn compute_merge_parallel_pair_in_series)`). It accumulates sub-list members with
`CombineMaps( ..., 0, status )` and never simplifies them, sets `simpler` from an
identity test on the pointer and the Invert flag (`cmpmap.c:1960-1962 (fn MapMerge)`), and hands
the assembled parallel CmpMap to the list as built.

All three points now match. The sub-list members are cloned as they stand
(`mappings/simplify.rs:2615-2622 (fn dimension_match_parallel_lists)`); the identity test is C's two terms — the result differs
from the input, or its stored orientation changed (`mappings/simplify.rs:2531-2536 (fn compute_merge_parallel_pair_in_series)`), the
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

## `Simplify` — cmpmap.c:3698-4064 (fn Simplify)

C gives CmpMap a `Simplify` body of its own, reached from `astSimplify_`
(`mapping.c:25121 (fn astSimplify_)`), which is the slot's only caller. It is the simplification
engine for every composite Mapping: decompose the tree into a series or parallel
list, drive each component's `astMapMerge` from a nominate loop until the list
stops improving, and fold the survivors back into a right-associated CmpMap.

The Rust `CmpMap::simplified` (`cmpmap.rs:1035-1040 (fn simplified)`) passes its gate to
`simplify::simplify_cmpmap` (`mappings/simplify.rs:3992-4083 (fn simplify_cmpmap_into)`), so Rust sites below are in
`mappings/simplify.rs` unless stated. The `has_been_simplified` test at `cmpmap.rs:1036 (fn simplified)` is
C's `astGetIsSimple` short-circuit, which C performs one level up in
`astSimplify_` (`mapping.c:25151-25153 (fn astSimplify_)`) and the port performs both there
(`simplify_entry.rs:137 (fn simplify_with_outcome)`) and here.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 43 | Initialize the result, check the inherited status and bail out | cmpmap.c:3764-3768 (fn Simplify) | -- | ceremony | n/a: the Rust signature hands back an owned Mapping in an `AstResult`, so there is no result pointer to initialize and no ambient status to test on entry |
| 44 | Compare the supplied Mapping against every Mapping being simplified higher up the call stack, and return a clone on a match | cmpmap.c:3773-3793 (fn Simplify) | mappings/simplify.rs:63-66 (fn enter), mappings/simplify.rs:4002-4009 (fn simplify_cmpmap_into) | architecture | ported |
| 45 | Increment the recursion depth and record the supplied Mapping for deeper invocations to compare against | cmpmap.c:3795-3801 (fn Simplify) | mappings/simplify.rs:67 (fn enter) | architecture | ported |
| 46 | Obtain a pointer to the CmpMap structure | cmpmap.c:3803-3804 (fn Simplify) | cmpmap.rs:1035-1040 (fn simplified), mappings/simplify.rs:3991-3995 (fn simplify_cmpmap_into) | ceremony | ported |
| 47 | Initialize the Mapping-pointer and Invert-flag arrays | cmpmap.c:3806-3810 (fn Simplify) | -- | ceremony | n/a: `flatten` returns an owned list of (Mapping, invert, orientation) triples, so there are no arrays to pre-initialize |
| 48 | Decompose the CmpMap into a series or parallel sequence with `astMapList`, setting `simpler` if any inverted CmpMap was found on the way | cmpmap.c:3812-3818 (fn Simplify) | mappings/simplify.rs:4018 (fn simplify_cmpmap_into), mappings/simplify.rs:4048 (fn simplify_cmpmap_into) | architecture | ported |
| 49 | Read the RestrictedSimplify flag | cmpmap.c:3820-3826 (fn Simplify) | mappings/simplify.rs:4012 (fn simplify_cmpmap_into) | architecture | ported |
| 50 | Under a restricted simplify, discard the `simpler` flag the decomposition set | cmpmap.c:3828-3832 (fn Simplify) | mappings/simplify.rs:4048 (fn simplify_cmpmap_into) | architecture | ported |
| 51 | Make the list entries independent of one another, so modifying one does not modify another | cmpmap.c:3834-3837 (fn Simplify) | mappings/simplify.rs:403 (fn leaf_entry) | architecture | ported |
| 52 | Clear the Frozen flag on every component | cmpmap.c:3839-3847 (fn Simplify) | mappings/simplify.rs:530 (fn new) | architecture | ported |
| 53 | Initialize the modified-index and Mapping-count histories | cmpmap.c:3849-3853 (fn Simplify) | mappings/simplify.rs:2058-2059 (fn merge_loop_restricted) | architecture | ported |
| 54 | Loop until a complete pass produces no improvement | cmpmap.c:3855-3859 (fn Simplify) | mappings/simplify.rs:1950-1952 (fn run_nominate_loop) | architecture | ported |
| 55 | Loop to nominate each Mapping in the sequence in turn | cmpmap.c:3861-3863 (fn Simplify) | mappings/simplify.rs:1953-1954 (fn run_nominate_loop) | architecture | ported |
| 56 | Skip a frozen component, and under a restricted simplify one without AllowSimplify | cmpmap.c:3865-3875 (fn Simplify) | mappings/simplify.rs:1959-1962 (fn run_nominate_loop) | architecture | ported |
| 57 | Clone the nominated Mapping and call `astMapMerge` on it | cmpmap.c:3877-3882 (fn Simplify) | mappings/simplify.rs:1965-1969 (fn run_nominate_loop) | architecture | ported |
| 58 | Move on to nominate the next Mapping | cmpmap.c:3884-3885 (fn Simplify) | mappings/simplify.rs:1977 (fn run_nominate_loop) | architecture | ported |
| 59 | On a change, append the modified index and the new Mapping count to the two histories and check each for a repeating pattern | cmpmap.c:3887-3898 (fn Simplify) | mappings/simplify.rs:1986-1987 (fn run_nominate_loop) | architecture | ported |
| 60 | Order the two wavelengths and require the larger to be an integer multiple of the smaller | cmpmap.c:3900-3911 (fn Simplify) | mappings/simplify.rs:1988-1997 (fn run_nominate_loop) | architecture | ported |
| 61 | On a repeating pattern, freeze the modified Mapping so it stops contributing to the argument | cmpmap.c:3913-3916 (fn Simplify) | mappings/simplify.rs:1991-1995 (fn run_nominate_loop) | architecture | ported |
| 62 | Otherwise note the improvement, and step back to the modified Mapping if it precedes the next nominee | cmpmap.c:3918-3928 (fn Simplify) | mappings/simplify.rs:2007-2015 (fn run_nominate_loop) | architecture | ported |
| 63 | Free the two histories | cmpmap.c:3933-3935 (fn Simplify) | -- | ceremony | n/a: owned `Vec`s are dropped |
| 64 | If nothing simplified, clone the supplied CmpMap rather than rebuilding it | cmpmap.c:3937-3943 (fn Simplify) | mappings/simplify.rs:4048-4062 (fn simplify_cmpmap_into) | architecture | ported |
| 65 | A single-element list is returned as a copy, not a clone, so the result is independent of the component Mappings | cmpmap.c:3945-3960 (fn Simplify) | mappings/simplify.rs:3713-3720 (fn reconstruct_inner) | architecture | ported |
| 66 | Set or clear the copy's Invert attribute as the list requires | cmpmap.c:3962-3968 (fn Simplify) | mappings/simplify.rs:3718-3720 (fn reconstruct_inner) | architecture | ported |
| 67 | For a longer list, note whether the last Mapping's Invert attribute is set, and its value | cmpmap.c:3970-3982 (fn Simplify) | mappings/simplify.rs:3713-3723 (fn reconstruct_inner) | architecture | ported |
| 68 | Set the last Mapping's Invert attribute to the value the list requires | cmpmap.c:3984-3985 (fn Simplify) | mappings/simplify.rs:3718-3720 (fn reconstruct_inner) | architecture | ported |
| 69 | Loop through the sequence in reverse, so each Mapping is combined with the CmpMap built so far | cmpmap.c:3987-3989 (fn Simplify) | mappings/simplify.rs:3725 (fn reconstruct_inner) | architecture | ported |
| 70 | Clear the protected AllowSimplify flag on each component, now the sequence is simplified | cmpmap.c:3991-3994 (fn Simplify) | mappings/simplify.rs:4078-4082 (fn simplify_cmpmap_into) | architecture | ported |
| 71 | Clone the pointer to the last Mapping in the sequence, which is encountered first | cmpmap.c:3996-3999 (fn Simplify) | mappings/simplify.rs:3713 (fn reconstruct_inner) | architecture | ported |
| 72 | For each earlier Mapping, save its Invert state, set the value required, combine it into the CmpMap, and restore the state | cmpmap.c:4001-4025 (fn Simplify) | mappings/simplify.rs:3725-3768 (fn reconstruct_inner) | architecture | ported |
| 73 | Restore the last Mapping's Invert state once the whole sequence has been merged | cmpmap.c:4028-4035 (fn Simplify) | mappings/simplify.rs:3753-3755 (fn reconstruct_inner) | architecture | ported |
| 74 | Annul the Mapping pointers in the list and free the dynamic arrays | cmpmap.c:4040-4047 (fn Simplify) | -- | ceremony | n/a: the owned list is dropped |
| 75 | Decrement the recursion depth, release the recorded Mapping, and free the stack at depth zero | cmpmap.c:4049-4057 (fn Simplify) | mappings/simplify.rs:73-76 (fn drop) | architecture | ported |
| 76 | Annul the result if an error occurred, and return it | cmpmap.c:4059-4063 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, so there is no result value to discard alongside it |

## Rows 44 and 45 — the recursion guard

C's guard is exact and unbounded: `astEqual` against each Mapping currently being
simplified higher up (`cmpmap.c:3789-3793 (fn Simplify)`), over a stack that grows to whatever
depth the recursion reaches (`astGrow`, `cmpmap.c:3798 (fn Simplify)`).

The port keeps the same list, as a `Vec<DynMapping>` pushed and popped by an RAII
guard, and tests it with `Mapping::equal`. C's separate depth counter has no
counterpart because the stack's own length is that number; nothing in the port
reads it.

The port previously compared a `u64` structural hash and refused outright past ten
nested simplifies. PLAN.md §10.19 records what those cost and how each was closed.

## Row 51 — the list is fully cloned, and the tags are copied back by hand

C's `SeparateMappings` (`cmpmap.c:3634 (fn SeparateMappings)`) deep-copies only *duplicate* component
pointers; every other list entry keeps aliasing the live child of the CmpMap being
simplified. `flatten` clones every leaf, so the port's list aliases nothing.

That difference is visible in one place. When the nominate loop's `astSimplify`
tags an entry `IsSimple` (`cmpmap.c:1672 (fn MapMerge)`), C writes through the alias onto the
real child, and the tag is still there when row 64 returns `astClone( this )`. The
port has to transplant those tags onto its fresh clone explicitly, which is what
`stamped_tags` (`mappings/simplify.rs:3879-3932 (fn stamped_tags)`) computes, walking the tree in the order
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
(`mapping.c:25135-25137 (fn astSimplify_)`, `simplify_entry.rs:184 (fn record_simplified)`) so it cannot reach a public
dump. The base driver's own gate for an ineligible Mapping
(`mapping.c:16979-16981 (fn Simplify)`) is at `mappings/simplify.rs:1460-1462 (fn base_simplify)`.

Three fixtures hold the mode, one per behavior: `cap_restricted_no_eligible`
(nothing eligible, so the inverted CmpMap comes back as supplied),
`cap_restricted_inverted` (one eligible ZoomMap whose own merge reaches across
its ineligible neighbors and collapses the chain), and
`cap_restricted_component_flags` (a rebuild that keeps an eligible component,
whose `AlSimp` card the fold clears).

## No C row — where the series UnitMap strip lives

C removes a UnitMap from a series sequence in `UnitMap::MapMerge`
(`unitmap.c:520-542 (fn MapMerge)`), when the UnitMap is nominated, and the port ports that
(`unit.rs:185-188 (fn map_merge)`, `unit.md` rows 5-7). That is now the only place the port
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
`astDoNotSimplify` returns zero (`cmpmap.c:1383 (fn MapList)`); otherwise it falls through to
the base `MapList`, which appends the Mapping itself as one leaf. The port has
the same condition on `flatten`'s decomposing branch (`mappings/simplify.rs:370 (fn flatten)`), and the
non-decomposing path was already the single-leaf append, so a named CmpMap
reaches the nominate loop intact. The companion gate in the public entry
(`mapping.c:25113 (fn astSimplify_)`, `simplify_entry.rs:137 (fn simplify_with_outcome)`) shares its `astClone( this )` exit
with the `IsSimple` short-circuit, as C's one expression does.

`CmpMap::Equal` (`cmpmap.c:489 (fn Equal)`) is the reason the port needed a second
decomposition entry point. C compares two CmpMaps by decomposing both with
`astMapList` and walking the lists element by element (`cmpmap.c:568 (fn Equal)`, `cmpmap.c:575 (fn Equal)`, `cmpmap.c:627 (fn Equal)`).
When both sides refuse to decompose, each list holds one element — the CmpMap it
came from — so the element-wise comparison is the comparison it started with and
C recurses without bound. It is unreachable from C's own callers only because
one side almost never carries an `Ident`. The port terminates instead: when both
sides refuse, `equal` calls `flatten_children` (`mappings/simplify.rs:430 (fn flatten_children)`), which runs
the body of C's decomposing branch on the two children directly. That is the
comparison C makes with either side's flag cleared, one level down, so no answer
C can actually produce changes.

`cap_ident_cmpmap` holds the gate and `cap_ident_frame` the `Frame` override.
PLAN.md §10.21.

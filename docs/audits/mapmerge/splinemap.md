# `MapMerge` audit — SplineMap

C source: reference/starlink-ast/src/splinemap.c
Rust: crates/ast-core/src/mappings/splinemap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- splinemap.c:1753 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `splinemap.c:1764-1855 (fn MapMerge)` and does one thing: cancel a SplineMap
against an adjacent SplineMap used in the opposite direction, replacing both with
UnitMaps.

`SplineMap::map_merge` (`splinemap.rs:1064-1076 (fn map_merge)`) calls `default_map_merge`
(`mappings/simplify.rs:1511-1604 (fn default_map_merge)`), whose neighbor attempt reaches
`SplineMap::try_merge` (`splinemap.rs:1136-1184 (fn try_merge)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | splinemap.c:1764-1765 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | splinemap.c:1767-1768 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated SplineMap | splinemap.c:1770-1772 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | The only merge available is with its own inverse, in series | splinemap.c:1773-1778 (fn MapMerge) | splinemap.rs:1143-1145 (fn try_merge) | architecture | ported |
| 5 | Temporarily set the nominated SplineMap's Invert flag to the required value, saving the original | splinemap.c:1779-1784 (fn MapMerge) | -- | ceremony | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 6 | Read its input and output counts at that orientation, to size the replacement | splinemap.c:1785-1789 (fn MapMerge) | splinemap.rs:1180 (fn try_merge) | architecture | ported |
| 7 | Take each neighbor in turn | splinemap.c:1790-1792 (fn MapMerge) | mappings/simplify.rs:1522-1544 (fn default_map_merge) | architecture | ported |
| 8 | Skip a neighbor that does not exist | splinemap.c:1793-1795 (fn MapMerge) | mappings/simplify.rs:1514-1516 (fn default_map_merge) | architecture | ported |
| 9 | Skip a neighbor that is not a SplineMap | splinemap.c:1796-1798 (fn MapMerge) | splinemap.rs:1148 (fn try_merge) | architecture | ported |
| 10 | Get a typed pointer to it | splinemap.c:1799-1801 (fn MapMerge) | splinemap.rs:1148 (fn try_merge) | ceremony | ported |
| 11 | Require the two to be used in opposite directions | splinemap.c:1802-1805 (fn MapMerge) | splinemap.rs:1150-1152 (fn try_merge) | architecture | ported |
| 12 | Set the neighbor's Invert flag so the equality test compares like with like | splinemap.c:1806-1815 (fn MapMerge) | splinemap.rs:1153 (fn try_merge) | architecture | ported |
| 13 | Use astEqual to compare the coefficients and the rest | splinemap.c:1816-1819 (fn MapMerge) | splinemap.rs:1154-1179 (fn try_merge) | architecture | ported |
| 14 | Re-instate the neighbor's original Invert flag | splinemap.c:1820-1822 (fn MapMerge) | -- | ceremony | n/a: the neighbor entry is read without being mutated |
| 15 | Move on if the neighbor differs from the nominated SplineMap | splinemap.c:1823-1826 (fn MapMerge) | splinemap.rs:1159-1178 (fn try_merge) | architecture | ported |
| 16 | Otherwise replace each of the two with a UnitMap | splinemap.c:1827-1844 (fn MapMerge) | splinemap.rs:1180 (fn try_merge) | architecture | ported |
| 17 | Leave the neighbor loop | splinemap.c:1845-1848 (fn MapMerge) | mappings/simplify.rs:1568 (fn default_map_merge) | architecture | ported |
| 18 | Re-instate the nominated SplineMap's Invert flag if it was not replaced | splinemap.c:1849-1853 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 19 | Return the result | splinemap.c:1854-1855 (fn MapMerge) | splinemap.rs:1075 (fn map_merge) | ceremony | ported |
| 20 | Setting or clearing InvNiter, InvTol or OutUnit clears the simplified record | splinemap.c:2560-2562 (fn astMAKE_CLEAR1), splinemap.c:2601-2603 (fn astMAKE_CLEAR1), splinemap.c:2647-2649 (fn astMAKE_CLEAR1) | splinemap.rs:392-405 (fn set_inv_niter), splinemap.rs:429-437 (fn set_inv_tol), splinemap.rs:463-515 (fn set_out_unit) | ceremony | ported |

## Row 12 — the flag C sets, and the comparison Rust makes without it

C sets the neighbor's Invert flag before calling `astEqual` so that the two
objects are compared in the same orientation. `SplineMap::try_merge` compares the
stored knots and coefficients directly and never looks at either object's
`inverted` field, so the like-with-like comparison holds without a flag to set;
the direction requirement is row 11's separate test on `self_inv` and
`other_inv`, which are the list's flags rather than the objects'.

## Row 16 — two UnitMaps, or one merged Mapping

C replaces both list entries with a `UnitMap` each, leaving the surrounding
CmpMap to drop them. `SplineMap::try_merge` returns one Mapping for the pair
instead. Both reduce to the same result once UnitMaps are removed; the difference
is how many passes it takes.

## There is no self-reduction step, in C or here

C's SplineMap `MapMerge` is the neighbor cancellation and nothing else, and
SplineMap does not override the `Simplify` vtab slot, so a nominated SplineMap
reduces only against a neighbor. `SplineMap::map_merge` matches that.

A SplineMap reduction added later belongs at the top of `map_merge`, which is
where C would put one, and where the base `Simplify` driver
(`mappings/simplify.rs:1383-1427 (fn base_simplify)`) reaches it.

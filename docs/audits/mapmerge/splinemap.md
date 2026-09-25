# `MapMerge` audit — SplineMap

C source: reference/starlink-ast/src/splinemap.c
Rust: crates/ast-core/src/mappings/splinemap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- splinemap.c:1746 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `splinemap.c:1757-1848 (fn MapMerge)` and does one thing: cancel a SplineMap
against an adjacent SplineMap used in the opposite direction, replacing both with
UnitMaps.

`SplineMap::map_merge` (`splinemap.rs:793-805 (fn map_merge)`) calls `default_map_merge`
(`mappings/simplify.rs:1321-1416 (fn default_map_merge)`), whose neighbour attempt reaches
`SplineMap::try_merge` (`splinemap.rs:863-911 (fn try_merge)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | splinemap.c:1757-1758 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | splinemap.c:1760-1761 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated SplineMap | splinemap.c:1763-1765 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | The only merge available is with its own inverse, in series | splinemap.c:1766-1771 (fn MapMerge) | splinemap.rs:870-872 (fn try_merge) | architecture | ported |
| 5 | Temporarily set the nominated SplineMap's Invert flag to the required value, saving the original | splinemap.c:1772-1777 (fn MapMerge) | -- | ceremony | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 6 | Read its input and output counts at that orientation, to size the replacement | splinemap.c:1778-1782 (fn MapMerge) | splinemap.rs:907 (fn try_merge) | architecture | ported |
| 7 | Take each neighbour in turn | splinemap.c:1783-1785 (fn MapMerge) | mappings/simplify.rs:1332-1354 (fn default_map_merge) | architecture | ported |
| 8 | Skip a neighbour that does not exist | splinemap.c:1786-1788 (fn MapMerge) | mappings/simplify.rs:1324-1326 (fn default_map_merge) | architecture | ported |
| 9 | Skip a neighbour that is not a SplineMap | splinemap.c:1789-1791 (fn MapMerge) | splinemap.rs:875 (fn try_merge) | architecture | ported |
| 10 | Get a typed pointer to it | splinemap.c:1792-1794 (fn MapMerge) | splinemap.rs:875 (fn try_merge) | ceremony | ported |
| 11 | Require the two to be used in opposite directions | splinemap.c:1795-1798 (fn MapMerge) | splinemap.rs:877-879 (fn try_merge) | architecture | ported |
| 12 | Set the neighbour's Invert flag so the equality test compares like with like | splinemap.c:1799-1808 (fn MapMerge) | splinemap.rs:880 (fn try_merge) | architecture | ported |
| 13 | Use astEqual to compare the coefficients and the rest | splinemap.c:1809-1812 (fn MapMerge) | splinemap.rs:881-906 (fn try_merge) | architecture | ported |
| 14 | Re-instate the neighbour's original Invert flag | splinemap.c:1813-1815 (fn MapMerge) | -- | ceremony | n/a: the neighbour entry is read without being mutated |
| 15 | Move on if the neighbour differs from the nominated SplineMap | splinemap.c:1816-1819 (fn MapMerge) | splinemap.rs:886-905 (fn try_merge) | architecture | ported |
| 16 | Otherwise replace each of the two with a UnitMap | splinemap.c:1820-1837 (fn MapMerge) | splinemap.rs:907 (fn try_merge) | architecture | ported |
| 17 | Leave the neighbour loop | splinemap.c:1838-1841 (fn MapMerge) | mappings/simplify.rs:1378 (fn default_map_merge) | architecture | ported |
| 18 | Re-instate the nominated SplineMap's Invert flag if it was not replaced | splinemap.c:1842-1846 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 19 | Return the result | splinemap.c:1847-1848 (fn MapMerge) | splinemap.rs:804 (fn map_merge) | ceremony | ported |
| 20 | Setting or clearing InvNiter, InvTol or OutUnit clears the simplified record | splinemap.c:2547-2549 (fn astMAKE_CLEAR1), splinemap.c:2588-2590 (fn astMAKE_CLEAR1), splinemap.c:2634-2636 (fn astMAKE_CLEAR1) | splinemap.rs:176-189 (fn set_inv_niter), splinemap.rs:197-203 (fn set_inv_tol), splinemap.rs:214-221 (fn set_out_unit) | ceremony | ported |

## Row 12 — the flag C sets, and the comparison Rust makes without it

C sets the neighbour's Invert flag before calling `astEqual` so that the two
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

C's SplineMap `MapMerge` is the neighbour cancellation and nothing else, and
SplineMap does not override the `Simplify` vtab slot, so a nominated SplineMap
reduces only against a neighbour. `SplineMap::map_merge` matches that.

A SplineMap reduction added later belongs at the top of `map_merge`, which is
where C would put one, and where the base `Simplify` driver
(`mappings/simplify.rs:1252-1294 (fn base_simplify)`) reaches it.

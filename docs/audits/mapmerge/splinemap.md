# `MapMerge` audit — SplineMap

C source: reference/starlink-ast/src/splinemap.c
Rust: crates/ast-core/src/mappings/splinemap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `splinemap.c:1757-1848` and does one thing: cancel a SplineMap
against an adjacent SplineMap used in the opposite direction, replacing both with
UnitMaps.

`SplineMap::map_merge` (`splinemap.rs:783-790`) routes through
`self_reduce_map_merge`, whose neighbour attempt reaches
`SplineMap::try_merge` (`splinemap.rs:855-900`) through `default_map_merge`.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | splinemap.c:1757-1758 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | splinemap.c:1760-1761 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated SplineMap | splinemap.c:1763-1765 | -- | n/a: the Rust method already has `&self` |
| 4 | The only merge available is with its own inverse, in series | splinemap.c:1766-1771 | splinemap.rs:862-863 | ported |
| 5 | Temporarily set the nominated SplineMap's Invert flag to the required value, saving the original | splinemap.c:1772-1777 | -- | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 6 | Read its input and output counts at that orientation | splinemap.c:1778-1782 | splinemap.rs:866-880 | ported |
| 7 | Take each neighbour in turn | splinemap.c:1783-1785 | simplify.rs:1155-1250 | ported |
| 8 | Skip a neighbour that does not exist | splinemap.c:1786-1788 | simplify.rs:1155-1250 | ported |
| 9 | Skip a neighbour that is not a SplineMap | splinemap.c:1789-1791 | splinemap.rs:867 | ported |
| 10 | Get a typed pointer to it | splinemap.c:1792-1794 | splinemap.rs:867 | ported |
| 11 | Require the two to be used in opposite directions | splinemap.c:1795-1798 | splinemap.rs:869-871 | ported |
| 12 | Set the neighbour's Invert flag so the equality test compares like with like | splinemap.c:1799-1808 | splinemap.rs:873-880 | ported |
| 13 | Use astEqual to compare the coefficients and the rest | splinemap.c:1809-1812 | splinemap.rs:873-880 | ported |
| 14 | Re-instate the neighbour's original Invert flag | splinemap.c:1813-1815 | -- | n/a: the neighbour entry is read without being mutated |
| 15 | Move on if the neighbour differs from the nominated SplineMap | splinemap.c:1816-1819 | splinemap.rs:873-880 | ported |
| 16 | Otherwise replace each of the two with a UnitMap | splinemap.c:1820-1837 | splinemap.rs:882-900 | ported |
| 17 | Leave the neighbour loop | splinemap.c:1838-1841 | simplify.rs:1155-1250 | ported |
| 18 | Re-instate the nominated SplineMap's Invert flag if it was not replaced | splinemap.c:1842-1846 | -- | n/a: nothing was mutated, so nothing is restored |
| 19 | Return the result | splinemap.c:1847-1848 | splinemap.rs:790 | ported |
| 20 | Setting or clearing InvNiter, InvTol or OutUnit clears the simplified record | splinemap.c, per mapping.h:449-457 | -- | gap: PLAN.md §9.2 |

## Row 16 — two UnitMaps, or one merged Mapping

C replaces both list entries with a `UnitMap` each, leaving the surrounding
CmpMap to drop them. `SplineMap::try_merge` returns one Mapping for the pair
instead. Both reduce to the same result once UnitMaps are removed; the difference
is how many passes it takes.

## The self-reduction seam is present, and empty

`self_reduce_map_merge` consults `simplify()`, and behind it `simplified()`,
which returns an unchanged clone
because C gives SplineMap no replace-with-simpler step either. The seam being
present is the point: unlike RateMap (PLAN.md §10.1) and PcdMap (§10.7), a
reduction added here would be reached from the nominate loop.

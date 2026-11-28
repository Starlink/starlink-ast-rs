# `MapMerge` audit — SplineMap

C source: reference/starlink-ast/src/splinemap.c
Rust: crates/starlink-ast/src/mappings/splinemap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 4029b011
Extension points: classified

Uncited C steps:
- splinemap.c:1919 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `splinemap.c:1930-2021 (fn MapMerge)` and does one thing: cancel a SplineMap
against an adjacent SplineMap used in the opposite direction, replacing both with
UnitMaps.

`SplineMap::map_merge` (`splinemap.rs:1068-1080 (fn map_merge)`) calls `default_map_merge`
(`mappings/simplify.rs:1672-1765 (fn default_map_merge)`), whose neighbor attempt reaches
`SplineMap::try_merge` (`splinemap.rs:1140-1188 (fn try_merge)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | splinemap.c:1930-1931 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | splinemap.c:1933-1934 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated SplineMap | splinemap.c:1936-1938 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | The only merge available is with its own inverse, in series | splinemap.c:1939-1944 (fn MapMerge) | splinemap.rs:1147-1149 (fn try_merge) | architecture | ported |
| 5 | Temporarily set the nominated SplineMap's Invert flag to the required value, saving the original | splinemap.c:1945-1950 (fn MapMerge) | -- | ceremony | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 6 | Read its input and output counts at that orientation, to size the replacement | splinemap.c:1951-1955 (fn MapMerge) | splinemap.rs:1184 (fn try_merge) | architecture | ported |
| 7 | Take each neighbor in turn | splinemap.c:1956-1958 (fn MapMerge) | mappings/simplify.rs:1683-1705 (fn default_map_merge) | architecture | ported |
| 8 | Skip a neighbor that does not exist | splinemap.c:1959-1961 (fn MapMerge) | mappings/simplify.rs:1675-1677 (fn default_map_merge) | architecture | ported |
| 9 | Skip a neighbor that is not a SplineMap | splinemap.c:1962-1964 (fn MapMerge) | splinemap.rs:1152 (fn try_merge) | architecture | ported |
| 10 | Get a typed pointer to it | splinemap.c:1965-1967 (fn MapMerge) | splinemap.rs:1152 (fn try_merge) | ceremony | ported |
| 11 | Require the two to be used in opposite directions | splinemap.c:1968-1971 (fn MapMerge) | splinemap.rs:1154-1156 (fn try_merge) | architecture | ported |
| 12 | Set the neighbor's Invert flag so the equality test compares like with like | splinemap.c:1972-1981 (fn MapMerge) | splinemap.rs:1157 (fn try_merge) | architecture | ported |
| 13 | Use astEqual to compare the coefficients and the rest | splinemap.c:1982-1985 (fn MapMerge) | splinemap.rs:1158-1183 (fn try_merge) | architecture | ported |
| 14 | Re-instate the neighbor's original Invert flag | splinemap.c:1986-1988 (fn MapMerge) | -- | ceremony | n/a: the neighbor entry is read without being mutated |
| 15 | Move on if the neighbor differs from the nominated SplineMap | splinemap.c:1989-1992 (fn MapMerge) | splinemap.rs:1163-1182 (fn try_merge) | architecture | ported |
| 16 | Otherwise replace each of the two with a UnitMap | splinemap.c:1993-2010 (fn MapMerge) | splinemap.rs:1184 (fn try_merge) | architecture | ported |
| 17 | Leave the neighbor loop | splinemap.c:2011-2014 (fn MapMerge) | mappings/simplify.rs:1729 (fn default_map_merge) | architecture | ported |
| 18 | Re-instate the nominated SplineMap's Invert flag if it was not replaced | splinemap.c:2015-2019 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 19 | Return the result | splinemap.c:2020-2021 (fn MapMerge) | splinemap.rs:1079 (fn map_merge) | ceremony | ported |
| 20 | Setting or clearing InvNiter, InvTol or OutUnit clears the simplified record | splinemap.c:2726-2728 (fn astMAKE_CLEAR1), splinemap.c:2767-2769 (fn astMAKE_CLEAR1), splinemap.c:2813-2815 (fn astMAKE_CLEAR1) | splinemap.rs:380-393 (fn set_inv_niter), splinemap.rs:417-425 (fn set_inv_tol), splinemap.rs:451-503 (fn set_out_unit) | ceremony | ported |

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
(`mappings/simplify.rs:1456-1500 (fn base_simplify)`) reaches it.

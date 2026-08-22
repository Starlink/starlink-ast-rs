# `MapMerge` audit — ChebyMap

C source: reference/starlink-ast/src/polymap.c
Rust: crates/ast-core/src/mappings/chebymap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

`chebymap.c` defines no `MapMerge`. Only `polymap.c:2439` sets the slot, so a
ChebyMap's `MapMerge` *is* PolyMap's, `polymap.c:3597-3961`. The rows below are
that body, audited against the Rust ChebyMap.

The Rust ChebyMap does not delegate to `PolyMap::map_merge`
(`polymap.rs:1359`); it has no `map_merge` and takes the trait default, and its
`simplified()` returns an unchanged clone. So C's whole replace-with-simpler
half is absent, and only the neighbour-cancellation half is present, through
`try_merge`.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned index to -1 | polymap.c:3597-3598 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | polymap.c:3600-3601 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated PolyMap | polymap.c:3603-3604 | -- | n/a: the Rust method already has `&self` |
| 4 | See if the PolyMap can be replaced by a simpler Mapping, without reference to its neighbours | polymap.c:3606-3608 | chebymap.rs:454-460 | gap: PLAN.md §10.2 |
| 5 | Get the input and output counts of the transformation as constructed | polymap.c:3610-3618 | -- | gap: PLAN.md §10.2 |
| 6 | Take a deep copy to modify | polymap.c:3620-3622 | -- | gap: PLAN.md §10.2 |
| 7 | Combine coefficients that refer to the same combination of input powers, forward then inverse | polymap.c:3624-3731 | -- | gap: PLAN.md §10.2 |
| 8 | If any transformation changed, store the new PolyMap in place of the old and report a change | polymap.c:3732-3745 | -- | gap: PLAN.md §10.2 |
| 9 | Test whether the forward transformation is linear, accumulating an offset vector and a matrix | polymap.c:3747-3842 | -- | gap: PLAN.md §10.2 |
| 10 | If it is linear, build the ShiftMap and MatrixMap and compose them | polymap.c:3843-3855 | -- | gap: PLAN.md §10.2 |
| 11 | Replace the PolyMap in the list with that composition | polymap.c:3856-3863 | -- | gap: PLAN.md §10.2 |
| 12 | Return the index of the first modified element | polymap.c:3864-3866 | -- | gap: PLAN.md §10.2 |
| 13 | Free the offset and matrix arrays | polymap.c:3868-3875 | -- | n/a: owned values are dropped |
| 14 | Otherwise see if the PolyMap can be merged with a neighbour | polymap.c:3877-3879 | chebymap.rs:462-468 | ported |
| 15 | The only merge available is a PolyMap with its own inverse, in series | polymap.c:3881-3886 | chebymap.rs:469-475 | ported |
| 16 | Temporarily set the nominated PolyMap's Invert flag to the requested value | polymap.c:3887-3892 | -- | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 17 | Take each neighbour in turn, skipping one that does not exist or is not a PolyMap | polymap.c:3893-3908 | chebymap.rs:476 | ported |
| 18 | Require the neighbour to be used in the opposite direction | polymap.c:3909-3911 | chebymap.rs:473-475 | ported |
| 19 | Use astEqual to check the coefficients and the rest are equal | polymap.c:3912-3925 | chebymap.rs:478-524 | ported |
| 20 | Replace both entries with UnitMaps and leave the loop | polymap.c:3933-3954 | chebymap.rs:525 | ported |
| 21 | Re-instate the nominated PolyMap's Invert flag if it was not replaced | polymap.c:3955-3959 | -- | n/a: nothing was mutated, so nothing is restored |
| 22 | Return the result | polymap.c:3960-3961 | chebymap.rs:525 | ported |

## Row 19 — the port checks more than C does

C compares two PolyMaps with `astEqual`, and ChebyMap's `Equal` covers its
bounds. The Rust `try_merge` compares the forward bounds, the inverse bounds and
the inner polynomial explicitly rather than calling `equal`, which reaches the
same predicate by a longer route.

## Rows 4 to 12 — do not port this blind

C's linearity test reads `pmap0->ncoeff_f` and the coefficient arrays directly
(`polymap.c:3747`) with nothing that distinguishes a Chebyshev basis from a
power basis. A first-order Chebyshev term is affine in the input, but only after
the domain scaling the ChebyMap's `lbnd`/`ubnd` define, and that scaling takes no
part in the matrix and offset C accumulates. Whether C's inherited path produces
a correct MatrixMap for a ChebyMap, or silently drops the scaling, is not settled
here, and mirroring it without settling that would import the answer either way.
See PLAN.md §10.2.

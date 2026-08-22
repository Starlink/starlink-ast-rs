# `MapMerge` audit — PolyMap

C source: reference/starlink-ast/src/polymap.c
Rust: crates/ast-core/src/mappings/polymap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `polymap.c:3597-3961` in two halves: consolidate the coefficients
and, if the result is linear, replace the PolyMap with a ShiftMap and MatrixMap;
then, failing that, cancel the PolyMap against an adjacent inverse PolyMap.

`PolyMap::map_merge` is `polymap.rs:1359-1395`; the reduction itself is
`PolyMap::simplified` (`polymap.rs:1398-1556`).

ChebyMap inherits this whole body in C and does not in the port; that is audited
separately in `chebymap.md`.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | polymap.c:3597-3598 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | polymap.c:3600-3601 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated PolyMap | polymap.c:3603-3604 | -- | n/a: the Rust method already has `&self` |
| 4 | See if the PolyMap can be replaced by something simpler, without reference to its neighbours | polymap.c:3606-3608 | polymap.rs:1387-1394 | ported |
| 5 | Get the input and output counts of the transformation as constructed | polymap.c:3610-3618 | polymap.rs:1398-1420 | ported |
| 6 | Take a deep copy to modify | polymap.c:3620-3622 | polymap.rs:1398-1420 | ported |
| 7 | Combine coefficients that refer to the same combination of input powers, forward then inverse | polymap.c:3624-3731 | polymap.rs:1421-1470 | ported |
| 8 | If any transformation changed, store the new PolyMap in place of the old and report a change | polymap.c:3732-3745 | polymap.rs:1379-1394 | ported |
| 9 | Test whether the forward transformation is linear, accumulating an offset vector and a matrix | polymap.c:3747-3842 | polymap.rs:1471-1540 | ported |
| 10 | If it is linear, build the ShiftMap and MatrixMap and compose them | polymap.c:3843-3855 | polymap.rs:1471-1540 | ported |
| 11 | Replace the PolyMap in the list with that composition | polymap.c:3856-3863 | polymap.rs:1390-1393 | ported |
| 12 | Return the index of the first modified element | polymap.c:3864-3866 | polymap.rs:1393 | ported |
| 13 | Free the offset and matrix arrays | polymap.c:3868-3875 | -- | n/a: owned values are dropped |
| 14 | Otherwise see if the PolyMap can be merged with a neighbour | polymap.c:3877-3879 | polymap.rs:1395 | ported |
| 15 | The only merge available is a PolyMap with its own inverse, in series | polymap.c:3881-3886 | polymap.rs:1564-1570 | ported |
| 16 | Temporarily set the nominated PolyMap's Invert flag to the requested value | polymap.c:3887-3892 | -- | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 17 | Read the nominated PolyMap's shape at that orientation | polymap.c:3893-3896 | polymap.rs:1572-1582 | ported |
| 18 | Take each neighbour in turn, skipping one that does not exist or is not a PolyMap | polymap.c:3897-3908 | simplify.rs:1155-1250 | ported |
| 19 | Require the neighbour to be used in the opposite direction | polymap.c:3909-3911 | polymap.rs:1568-1570 | ported |
| 20 | Set the neighbour's Invert flag so the equality test compares like with like | polymap.c:3912-3921 | polymap.rs:1590-1600 | ported |
| 21 | Use astEqual to check the coefficients and the rest are equal | polymap.c:3922-3925 | polymap.rs:1590-1600 | ported |
| 22 | Re-instate the neighbour's Invert flag | polymap.c:3926-3928 | -- | n/a: the neighbour entry is read without being mutated |
| 23 | Move on if the neighbour differs | polymap.c:3929-3932 | polymap.rs:1590-1600 | ported |
| 24 | Otherwise replace both with UnitMaps and leave the loop | polymap.c:3933-3954 | polymap.rs:1578-1582 | ported |
| 25 | Re-instate the nominated PolyMap's Invert flag if it was not replaced | polymap.c:3955-3959 | -- | n/a: nothing was mutated, so nothing is restored |
| 26 | Return the result | polymap.c:3960-3961 | polymap.rs:1395 | ported |
| 27 | Setting or clearing IterInverse, NiterInverse or TolInverse clears the simplified record | polymap.c, per mapping.h:449-457 | -- | gap: PLAN.md §9.2 |

## Row 7 — both directions, not just the applied one

C loops `for( check_fwd = 0; check_fwd < 2; check_fwd++ )` so the forward and
inverse coefficient sets are both consolidated. `PolyMap::simplified` does the
same. Consolidating only the current direction leaves a SIP inverse polynomial
holding its identity term and its linear correction as two separate
coefficients, which the SIP extractor then reads one of.

## Row 8 — "did anything change?" is a flag, not a fingerprint

C carries a `mod` flag through the consolidation. The port cannot use a
structural fingerprint in its place: consolidating the *inverse* coefficients
leaves the forward transformation, and so the fingerprint, untouched. The
`changed` closure at `polymap.rs:1379-1385` compares the forward set, the inverse
set and the invert flag separately for that reason.

## Row 4 — built from the constructed transformation, not the applied one

C builds the replacement from the transformation as constructed, before any
inversion, and leaves `invert_list` alone, so the replacement inherits the
entry's applied direction rather than absorbing it. `PolyMap::simplified` works
from the stored orientation to match, which is what lets the entry's flags carry
over at `polymap.rs:1390-1392`.

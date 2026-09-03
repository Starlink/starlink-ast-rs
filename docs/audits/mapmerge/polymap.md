# `MapMerge` audit — PolyMap

C source: reference/starlink-ast/src/polymap.c
Rust: crates/ast-core/src/mappings/polymap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3e60abaa
Extension points: classified

Uncited C steps:
- polymap.c:3558 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `polymap.c:3597-3961 (fn MapMerge)` in two halves: consolidate the
coefficients and, if the result is linear, replace the PolyMap with a ShiftMap and
MatrixMap; then, failing that, cancel the PolyMap against an adjacent inverse
PolyMap.

`PolyMap::map_merge` is `polymap.rs:1537-1559 (fn map_merge)`; the two reductions
it runs before it looks at a neighbour are `PolyMap::replace_with_simpler`
(`polymap.rs:891-994 (fn replace_with_simpler)`), and the neighbour half is
`PolyMap::try_merge` (`polymap.rs:1565-1603 (fn try_merge)`) reached through
`default_map_merge`.

ChebyMap inherits this whole body in C and does not in the port; that is audited
separately in `chebymap.md`.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | polymap.c:3597-3598 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | polymap.c:3600-3601 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated PolyMap | polymap.c:3603-3604 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | See if the PolyMap can be replaced by something simpler, without reference to its neighbours | polymap.c:3606-3608 (fn MapMerge) | polymap.rs:1543-1552 (fn map_merge) | architecture | ported |
| 5 | Get the input and output counts of the transformation as constructed | polymap.c:3610-3618 (fn MapMerge) | polymap.rs:924-925 (fn replace_with_simpler) | architecture | ported |
| 6 | Take a deep copy to modify | polymap.c:3620-3622 (fn MapMerge) | polymap.rs:972-993 (fn replace_with_simpler) | architecture | ported |
| 7 | Combine coefficients that refer to the same combination of input powers, forward then inverse | polymap.c:3624-3731 (fn MapMerge) | polymap.rs:845-889 (fn consolidated_terms) | architecture | ported |
| 8 | If any transformation changed, store the new PolyMap in place of the old and report a change | polymap.c:3732-3745 (fn MapMerge) | polymap.rs:884-888 (fn consolidated_terms), polymap.rs:968-970 (fn replace_with_simpler) | architecture | ported |
| 9 | Test whether the forward transformation is linear, accumulating an offset vector and a matrix | polymap.c:3747-3842 (fn MapMerge) | polymap.rs:915-950 (fn replace_with_simpler) | architecture | ported |
| 10 | If it is linear, build the ShiftMap and MatrixMap and compose them | polymap.c:3843-3855 (fn MapMerge) | polymap.rs:952-962 (fn replace_with_simpler) | architecture | ported |
| 11 | Replace the PolyMap in the list with that composition | polymap.c:3856-3863 (fn MapMerge) | polymap.rs:1553-1554 (fn map_merge) | architecture | ported |
| 12 | Return the index of the first modified element | polymap.c:3864-3866 (fn MapMerge) | polymap.rs:1555 (fn map_merge) | architecture | ported |
| 13 | Free the offset and matrix arrays | polymap.c:3868-3875 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 14 | Otherwise see if the PolyMap can be merged with a neighbour | polymap.c:3877-3879 (fn MapMerge) | polymap.rs:1558 (fn map_merge) | architecture | ported |
| 15 | The only merge available is a PolyMap with its own inverse, in series | polymap.c:3881-3886 (fn MapMerge) | polymap.rs:1572-1579 (fn try_merge) | architecture | ported |
| 16 | Temporarily set the nominated PolyMap's Invert flag to the requested value | polymap.c:3887-3892 (fn MapMerge) | polymap.rs:1587-1591 (fn try_merge) | architecture | ported |
| 17 | Read the nominated PolyMap's shape at that orientation | polymap.c:3893-3896 (fn MapMerge) | polymap.rs:1599-1601 (fn try_merge) | architecture | ported |
| 18 | Take each neighbour in turn, skipping one that does not exist or is not a PolyMap | polymap.c:3897-3908 (fn MapMerge) | mappings/simplify.rs:1332-1353 (fn default_map_merge) | architecture | ported |
| 19 | Require the neighbour to be used in the opposite direction | polymap.c:3909-3911 (fn MapMerge) | polymap.rs:1576-1578 (fn try_merge) | architecture | ported |
| 20 | Set the neighbour's Invert flag so the equality test compares like with like | polymap.c:3912-3921 (fn MapMerge) | polymap.rs:1592-1594 (fn try_merge) | architecture | ported |
| 21 | Use astEqual to check the coefficients and the rest are equal | polymap.c:3922-3925 (fn MapMerge) | polymap.rs:1595-1597 (fn try_merge), polymap.rs:1605-1637 (fn equal) | architecture | ported |
| 22 | Re-instate the neighbour's Invert flag | polymap.c:3926-3928 (fn MapMerge) | -- | ceremony | n/a: the two PolyMaps are compared as copies, so the list entries are never mutated and nothing is restored |
| 23 | Move on if the neighbour differs | polymap.c:3929-3932 (fn MapMerge) | polymap.rs:1595-1597 (fn try_merge) | architecture | ported |
| 24 | Otherwise replace both with UnitMaps and leave the loop | polymap.c:3933-3954 (fn MapMerge) | polymap.rs:1601-1602 (fn try_merge), mappings/simplify.rs:1407-1415 (fn default_map_merge) | architecture | ported |
| 25 | Re-instate the nominated PolyMap's Invert flag if it was not replaced | polymap.c:3955-3959 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 26 | Return the result | polymap.c:3960-3961 (fn MapMerge) | polymap.rs:1558 (fn map_merge) | ceremony | ported |
| 27 | Setting or clearing IterInverse, NiterInverse or TolInverse clears the simplified record | polymap.c:6340 (fn astMAKE_CLEAR1),6345 | polymap.rs:249-258 (fn set_iter_inverse), polymap.rs:274-277 (fn clear_iter_inverse), polymap.rs:293-297 (fn set_niter_inverse), polymap.rs:306-310 (fn set_tol_inverse) | ceremony | ported |

## Row 7 — both directions, not just the applied one

C loops `for( check_fwd = 0; check_fwd < 2; check_fwd++ )` so the forward and
inverse coefficient sets are both consolidated. `replace_with_simpler` does the
same. Consolidating only the current direction leaves a SIP inverse polynomial
holding its identity term and its linear correction as two separate
coefficients, which the SIP extractor then reads one of.

## Row 8 — "did anything change?" is a flag, not a fingerprint

C carries a `mod` flag through the consolidation. The port cannot use a
structural fingerprint in its place: consolidating the *inverse* coefficients
leaves the forward transformation, and so the fingerprint, untouched.
`consolidated_terms` compares both coefficient counts against the originals
(`polymap.rs:880-886 (fn consolidated_terms)`) for that reason, and returns
`None` — no replacement — only when neither moved.

## Row 4 — built from the constructed transformation, not the applied one

C builds the replacement from the transformation as constructed, before any
inversion, and leaves `invert_list` alone, so the replacement inherits the
entry's applied direction rather than absorbing it. `replace_with_simpler` reads
`self.forward` / `self.inverse` and the struct's own coordinate counts, never the
orientation-aware accessors, so the stored `Invert` flag selects nothing; the
entry's flag is carried over unchanged at `polymap.rs:1553-1554 (fn map_merge)`.

## Row 9 — what makes a PolyMap linear is the forward coefficients, nothing else

C's guard is `if( pmap0->ncoeff_f && nout == nin )` (`polymap.c:3757 (fn MapMerge)`):
a constructed forward transformation and matching coordinate counts. It then
fills a zero-initialised offset vector and coefficient matrix from the
consolidated terms, so a forward transformation whose coefficients all cancel —
`+1.0 x` and `-1.0 x` over the same power — is linear with every element zero,
and C reduces it to a diagonal MatrixMap of zeros. `is_affine`
(`polymap.rs:926-931 (fn replace_with_simpler)`) tests `forward_defined()` and
`nin == nout` and nothing else for that reason; requiring a non-empty
consolidated set instead left an empty PolyMap where C leaves a MatrixMap.

## Row 10 — the composition's shape is `astSimplify`'s answer, not one chosen here

C does not decide what a linear PolyMap becomes. It builds the ShiftMap and the
MatrixMap from the accumulated offsets and matrix, composes them in series and
simplifies the pair:

```c
         sm = astShiftMap( nout, offsets, " ", status );
         mm = astMatrixMap( nin, nout, 0, matrix, " ", status );
         cm = astCmpMap( mm, sm, 1, " ", status );
         new = astSimplify( cm );
```

`replace_with_simpler` builds the same two Mappings, composes them the same way
round and calls `simplify` on the pair (`polymap.rs:957-962 (fn replace_with_simpler)`). Deriving the
shape here instead — a bare MatrixMap when every offset is zero, a WinMap when
the matrix is diagonal — reaches the same Mapping by a different route and loses
what the call also does: `astSimplify` stamps whatever it returns
(`mapping.c:24783 (fn astSimplify_)`), so the replacement carries the `IsSimp` card C's reference
shows on it. `lsst.simp` is what pins that.

## Rows 4 to 12 — the reduction is a step of the merge

Both replace-with-simpler steps are the head of C's `MapMerge`, not a separate
`Simplify` override: `polymap.c` has no `Simplify` vtab slot, and the base
`Simplify` reaches this body by nominating the PolyMap in a list of one
(`mapping.c:16871-16883 (fn Simplify)`). `replace_with_simpler` is called from
`map_merge` alone (`polymap.rs:1552 (fn map_merge)`), so a PolyMap reduces only inside the merge
loop — which is what stops the `IsSimp` card landing on a leaf C never stamps.

It is also what makes the `Ident` guard reach this half. C's public dispatcher
returns before any per-class `MapMerge` runs when the nominated Mapping's `Ident`
attribute is set (`mapping.c:24257 (fn astMapMerge_)`), so an identified PolyMap
keeps its coefficients as constructed even when the tree around it simplifies.
The port applies the same guard in one place, `dispatch_map_merge`
(`mappings/simplify.rs:1296-1319 (fn dispatch_map_merge)`), which is how every
call site gets it; see PLAN.md §10.29.

## Rows 16 to 21 — the Invert forcing is what makes the comparison possible

`PolyMap`'s own `Equal` refuses a pair whose `Invert` flags differ
(`polymap.c:806-812 (fn Equal)`), and the two list entries being considered here
are used in opposite directions by construction (row 19). The forcing at rows 16
and 20 is therefore not an optimisation: without it the comparison could never
succeed. What it compares, once both flags read the same, is each transformation
as constructed.

`try_merge` forces the two directions on clones rather than on the list entries,
which is why rows 22 and 25 have nothing to restore. The comparison itself is
`PolyMap::equal`, C's `Equal` slot.

That slot has a copy-paste bug: the block that should compare the inverse
coefficient values and power patterns compares the forward ones a second time
(`polymap.c:778-804 (fn Equal)`), so two PolyMaps agreeing on their forward
transformations and on the *shape* of their inverses cancel however far apart
their inverse coefficients are. The oracle confirms the cancellation, so
`PolyMap::equal` reproduces it through `inverse_shape`
(`polymap.rs:90-101 (fn inverse_shape)`), which reduces a term list to C's
`ncoeff_i` and `mxpow_i`. `docs/issues/c-library-quirks.md` records it for
upstream.

## Row 24 — C leaves two UnitMaps, the port leaves one entry

C overwrites both list elements with a UnitMap and returns the lower index, so a
cancelling pair becomes two adjacent UnitMaps that a later pass removes. The port
composes the pair into a single UnitMap and closes the gap in `default_map_merge`,
returning the same index. The merge loop runs to convergence either way, and C's
own next pass strips the redundant UnitMap, so both routes settle on the same
list. The dimension is the one C uses: the input count of the pair as applied,
which is the nominated PolyMap's effective `nin` (`polymap.rs:1599-1601 (fn try_merge)`).

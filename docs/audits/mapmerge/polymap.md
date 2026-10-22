# `MapMerge` audit — PolyMap

C source: reference/starlink-ast/src/polymap.c
Rust: crates/ast-core/src/mappings/polymap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- polymap.c:3582 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `polymap.c:3623-4010 (fn MapMerge)` in two halves: consolidate the
coefficients and, if the result is linear, replace the PolyMap with a ShiftMap and
MatrixMap; then, failing that, cancel the PolyMap against an adjacent inverse
PolyMap.

`PolyMap::map_merge` is `polymap.rs:1880-1902 (fn map_merge)`; the two reductions
it runs before it looks at a neighbor are `PolyMap::replace_with_simpler`
(`polymap.rs:1056-1100 (fn replace_with_simpler)`), and the neighbor half is
`PolyMap::try_merge` (`polymap.rs:1909-1947 (fn try_merge)`) reached through
`default_map_merge`.

ChebyMap inherits this whole body in C, and in the port too: both halves are
shared, the consolidation through `PolyMap::consolidated_terms` and the linear
replacement through `PolyMap::linear_replacement`, which takes the ChebyMap's
input normalization as an argument. What differs is only which of them
`ChebyMap::map_merge` calls and with what, which is audited separately in
`chebymap.md`.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | polymap.c:3623-3624 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | polymap.c:3626-3627 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated PolyMap | polymap.c:3629-3630 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | See if the PolyMap can be replaced by something simpler, without reference to its neighbors | polymap.c:3632-3634 (fn MapMerge) | polymap.rs:1886-1899 (fn map_merge) | architecture | ported |
| 5 | Get the input and output counts of the transformation as constructed | polymap.c:3636-3644 (fn MapMerge) | polymap.rs:991-992 (fn linear_replacement) | architecture | ported |
| 6 | Take a deep copy to modify | polymap.c:3646-3648 (fn MapMerge) | polymap.rs:1078-1099 (fn replace_with_simpler) | architecture | ported |
| 7 | Combine coefficients that refer to the same combination of input powers, forward then inverse | polymap.c:3650-3756 (fn MapMerge) | polymap.rs:919-963 (fn consolidated_terms) | architecture | ported |
| 8 | If any transformation changed, store the new PolyMap in place of the old and report a change | polymap.c:3758-3771 (fn MapMerge) | polymap.rs:958-962 (fn consolidated_terms), polymap.rs:1060-1065 (fn replace_with_simpler) | architecture | ported |
| 9 | Test whether the forward transformation is linear, accumulating an offset vector and a matrix with the input normalization folded in | polymap.c:3773-3890 (fn MapMerge) | polymap.rs:983-1028 (fn linear_replacement) | architecture | ported |
| 10 | If it is linear, build the ShiftMap and MatrixMap and compose them | polymap.c:3892-3903 (fn MapMerge) | polymap.rs:1030-1040 (fn linear_replacement) | architecture | ported |
| 11 | Replace the PolyMap in the list with that composition | polymap.c:3905-3912 (fn MapMerge) | polymap.rs:1895-1897 (fn map_merge) | architecture | ported |
| 12 | Return the index of the first modified element | polymap.c:3913-3915 (fn MapMerge) | polymap.rs:1898 (fn map_merge) | architecture | ported |
| 13 | Free the offset and matrix arrays | polymap.c:3917-3920 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 14 | Otherwise see if the PolyMap can be merged with a neighbor | polymap.c:3926-3928 (fn MapMerge) | polymap.rs:1901 (fn map_merge) | architecture | ported |
| 15 | The only merge available is a PolyMap with its own inverse, in series | polymap.c:3930-3934 (fn MapMerge) | polymap.rs:1916-1923 (fn try_merge) | architecture | ported |
| 16 | Temporarily set the nominated PolyMap's Invert flag to the requested value | polymap.c:3936-3940 (fn MapMerge) | polymap.rs:1931-1935 (fn try_merge) | architecture | ported |
| 17 | Read the nominated PolyMap's shape at that orientation | polymap.c:3942-3944 (fn MapMerge) | polymap.rs:1943-1945 (fn try_merge) | architecture | ported |
| 18 | Take each neighbor in turn, skipping one that does not exist or is not a PolyMap | polymap.c:3946-3956 (fn MapMerge) | mappings/simplify.rs:1581-1602 (fn default_map_merge) | architecture | ported |
| 19 | Require the neighbor to be used in the opposite direction | polymap.c:3958-3959 (fn MapMerge) | polymap.rs:1920-1922 (fn try_merge) | architecture | ported |
| 20 | Set the neighbor's Invert flag so the equality test compares like with like | polymap.c:3961-3969 (fn MapMerge) | polymap.rs:1936-1938 (fn try_merge) | architecture | ported |
| 21 | Use astEqual to check the coefficients and the rest are equal | polymap.c:3971-3973 (fn MapMerge) | polymap.rs:1939-1941 (fn try_merge), polymap.rs:1949-1985 (fn equal) | architecture | ported |
| 22 | Re-instate the neighbor's Invert flag | polymap.c:3975-3976 (fn MapMerge) | -- | ceremony | n/a: the two PolyMaps are compared as copies, so the list entries are never mutated and nothing is restored |
| 23 | Move on if the neighbor differs | polymap.c:3978-3980 (fn MapMerge) | polymap.rs:1939-1941 (fn try_merge) | architecture | ported |
| 24 | Otherwise replace both with UnitMaps and leave the loop | polymap.c:3982-4001 (fn MapMerge) | polymap.rs:1945-1946 (fn try_merge), mappings/simplify.rs:1655-1662 (fn default_map_merge) | architecture | ported |
| 25 | Re-instate the nominated PolyMap's Invert flag if it was not replaced | polymap.c:4004-4006 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 26 | Return the result | polymap.c:4009-4010 (fn MapMerge) | polymap.rs:1901 (fn map_merge) | ceremony | ported |
| 27 | Setting or clearing IterInverse, NiterInverse or TolInverse clears the simplified record | polymap.c:6401 (fn astMAKE_CLEAR1), polymap.c:6406-6411 (fn astMAKE_SET1) | polymap.rs:345-346 (fn set_iter_inverse), polymap.rs:366-369 (fn clear_iter_inverse), polymap.rs:404-406 (fn set_niter_inverse), polymap.rs:455-457 (fn set_tol_inverse) | ceremony | ported |

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
(`polymap.rs:954-960 (fn consolidated_terms)`) for that reason, and returns
`None` — no replacement — only when neither moved.

## Row 4 — built from the constructed transformation, not the applied one

C builds the replacement from the transformation as constructed, before any
inversion, and leaves `invert_list` alone, so the replacement inherits the
entry's applied direction rather than absorbing it. `replace_with_simpler` reads
`self.forward` / `self.inverse` and the struct's own coordinate counts, never the
orientation-aware accessors, so the stored `Invert` flag selects nothing; the
entry's flag is carried over unchanged at `polymap.rs:1896-1897 (fn map_merge)`.

## Row 9 — what makes a PolyMap linear is the forward coefficients, nothing else

C's guard is `if( pmap0->ncoeff_f && nout == nin )` (`polymap.c:3791 (fn MapMerge)`):
a constructed forward transformation and matching coordinate counts. It then
fills a zero-initialized offset vector and coefficient matrix from the
consolidated terms, so a forward transformation whose coefficients all cancel —
`+1.0 x` and `-1.0 x` over the same power — is linear with every element zero,
and C reduces it to a diagonal MatrixMap of zeros. The linearity test
(`polymap.rs:993-998 (fn linear_replacement)`) reads `forward_defined()` and
`nin == nout` and nothing else for that reason; requiring a non-empty
consolidated set instead left an empty PolyMap where C leaves a MatrixMap.

Both arrays accumulate rather than assign (`polymap.c:3864-3883 (fn MapMerge)`),
which is what lets one output collect a constant contribution from several terms.
For a PolyMap that only matters when the consolidation has left two terms over
the same powers; for a ChebyMap, whose coefficients belong to a series in the
normalized input, every linear term carries a constant with it, so the
accumulation is load-bearing. That normalization is the `norm` argument
`linear_replacement` takes, and `chebymap.md` audits the ChebyMap side of it.

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

`linear_replacement` builds the same two Mappings, composes them the same way
round and calls `simplify` on the pair (`polymap.rs:1035-1040 (fn linear_replacement)`). Deriving the
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
`map_merge` alone (`polymap.rs:1895 (fn map_merge)`), so a PolyMap reduces only inside the merge
loop — which is what stops the `IsSimp` card landing on a leaf C never stamps.

It is also what makes the `Ident` guard reach this half. C's public dispatcher
returns before any per-class `MapMerge` runs when the nominated Mapping's `Ident`
attribute is set (`mapping.c:24257 (fn astMapMerge_)`), so an identified PolyMap
keeps its coefficients as constructed even when the tree around it simplifies.
The port applies the same guard in one place, `dispatch_map_merge`
(`mappings/simplify.rs:1488-1568 (fn dispatch_map_merge)`), which is how every
call site gets it; see PLAN.md §10.29.

## Rows 16 to 21 — the Invert forcing is what makes the comparison possible

`PolyMap`'s own `Equal` refuses a pair whose `Invert` flags differ
(`polymap.c:830-835 (fn Equal)`), and the two list entries being considered here
are used in opposite directions by construction (row 19). The forcing at rows 16
and 20 is therefore not an optimization: without it the comparison could never
succeed. What it compares, once both flags read the same, is each transformation
as constructed.

`try_merge` forces the two directions on clones rather than on the list entries,
which is why rows 22 and 25 have nothing to restore. The comparison itself is
`PolyMap::equal`, C's `Equal` slot.

That slot had a copy-paste bug until AST 9.5.0: the block that should compare the
inverse coefficient values and power patterns compared the forward ones a second
time, so two PolyMaps agreeing on their forward transformations and on the *shape*
of their inverses canceled however far apart their inverse coefficients were.
`PolyMap::equal` mirrored it while it stood. Upstream `ec86cfaf` compares the
inverse transformation's own coefficients and powers
(`polymap.c:782-800 (fn Equal)`), and the port now compares the inverse terms
exactly as it compares the forward ones
(`polymap.rs:1968-1984 (fn equal)`). `docs/issues/c-library-quirks.md` carries
the history.

## Row 24 — C leaves two UnitMaps, the port leaves one entry

C overwrites both list elements with a UnitMap and returns the lower index, so a
cancelling pair becomes two adjacent UnitMaps that a later pass removes. The port
composes the pair into a single UnitMap and closes the gap in `default_map_merge`,
returning the same index. The merge loop runs to convergence either way, and C's
own next pass strips the redundant UnitMap, so both routes settle on the same
list. The dimension is the one C uses: the input count of the pair as applied,
which is the nominated PolyMap's effective `nin` (`polymap.rs:1943-1945 (fn try_merge)`).

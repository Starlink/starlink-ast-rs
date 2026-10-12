# `MapMerge` audit — ChebyMap

C source: reference/starlink-ast/src/polymap.c
Rust: crates/ast-core/src/mappings/chebymap.rs
Audited: 2026-08-24 against pin d0346c0d
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- polymap.c:3582 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

`chebymap.c` defines no `MapMerge`. Only `polymap.c:2463 (fn astInitPolyMapVtab_)`
fills the slot, so a ChebyMap's `MapMerge` *is* PolyMap's,
`polymap.c:3623-4010 (fn MapMerge)`. The rows below are that body, audited
against the Rust ChebyMap.

Neither `chebymap.c` nor `polymap.c` sets the `Simplify` slot, so every reduction
a ChebyMap has is reached through this body: the base `Simplify` nominates a lone
ChebyMap in a list of one (`mapping.c:16871-16883 (fn Simplify)`) and calls
`MapMerge` on it. The port matches that by leaving `Mapping::simplified` at its
default.

The Rust ChebyMap does not delegate to `PolyMap::map_merge`
(`polymap.rs:1513-1535 (fn map_merge)`). It has its own
(`chebymap.rs:444-508 (fn map_merge)`), which runs the coefficient consolidation
through `PolyMap::consolidated_terms`
(`polymap.rs:769-813 (fn consolidated_terms)`) and then falls through to the
shared direct merge; the neighbour cancellation is
`ChebyMap::try_merge` (`chebymap.rs:403-442 (fn try_merge)`).

C's linear replacement is there too, reached through
`PolyMap::linear_replacement` (`polymap.rs:828-891 (fn linear_replacement)`) with
the ChebyMap's own `scale_f` / `offset_f` passed as the normalisation. Until AST
9.5.0 that half dropped the Chebyshev domain scaling and so changed the
transformation, and the port declined it; upstream `abe5baae` folds the
normalisation into the matrix and the offsets, and the port follows.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned index to -1 | polymap.c:3623-3624 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | polymap.c:3626-3627 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated PolyMap | polymap.c:3629-3630 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | See if the PolyMap can be replaced by a simpler Mapping, without reference to its neighbours | polymap.c:3632-3634 (fn MapMerge) | chebymap.rs:466-505 (fn map_merge) | architecture | ported |
| 5 | Get the input and output counts of the transformation as constructed | polymap.c:3636-3644 (fn MapMerge) | chebymap.rs:484-488 (fn map_merge) | architecture | ported |
| 6 | Take a deep copy to modify | polymap.c:3646-3648 (fn MapMerge) | chebymap.rs:489-502 (fn map_merge) | architecture | ported |
| 7 | Combine coefficients that refer to the same combination of input powers, forward then inverse | polymap.c:3650-3756 (fn MapMerge) | polymap.rs:769-813 (fn consolidated_terms) | architecture | ported |
| 8 | If any transformation changed, store the new PolyMap in place of the old and report a change | polymap.c:3758-3771 (fn MapMerge) | polymap.rs:808-812 (fn consolidated_terms), chebymap.rs:503-504 (fn map_merge) | architecture | ported |
| 9 | Test whether the forward transformation is linear, accumulating an offset vector and a matrix with the input normalisation folded in | polymap.c:3773-3890 (fn MapMerge) | polymap.rs:833-878 (fn linear_replacement), chebymap.rs:475-477 (fn map_merge) | architecture | ported |
| 10 | If it is linear, build the ShiftMap and MatrixMap and compose them | polymap.c:3892-3903 (fn MapMerge) | polymap.rs:880-890 (fn linear_replacement) | architecture | ported |
| 11 | Replace the PolyMap in the list with that composition | polymap.c:3905-3912 (fn MapMerge) | chebymap.rs:479 (fn map_merge) | architecture | ported |
| 12 | Return the index of the first modified element | polymap.c:3913-3915 (fn MapMerge) | chebymap.rs:480 (fn map_merge) | architecture | ported |
| 13 | Free the offset and matrix arrays | polymap.c:3917-3920 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 14 | Otherwise see if the PolyMap can be merged with a neighbour | polymap.c:3926-3928 (fn MapMerge) | chebymap.rs:507 (fn map_merge) | architecture | ported |
| 15 | The only merge available is a PolyMap with its own inverse, in series | polymap.c:3930-3934 (fn MapMerge) | chebymap.rs:410-417 (fn try_merge) | architecture | ported |
| 16 | Temporarily set the nominated PolyMap's Invert flag to the requested value | polymap.c:3936-3940 (fn MapMerge) | chebymap.rs:427-431 (fn try_merge) | architecture | ported |
| 17 | Read the nominated PolyMap's shape at that orientation | polymap.c:3942-3944 (fn MapMerge) | chebymap.rs:440-441 (fn try_merge) | architecture | ported |
| 18 | Take each neighbour in turn, skipping one that does not exist or is not a PolyMap | polymap.c:3946-3956 (fn MapMerge) | mappings/simplify.rs:1374-1395 (fn default_map_merge) | architecture | ported |
| 19 | Require the neighbour to be used in the opposite direction | polymap.c:3958-3959 (fn MapMerge) | chebymap.rs:414-416 (fn try_merge) | architecture | ported |
| 20 | Set the neighbour's Invert flag so the equality test compares like with like | polymap.c:3961-3969 (fn MapMerge) | chebymap.rs:432-434 (fn try_merge) | architecture | ported |
| 21 | Use astEqual to check the coefficients and the rest are equal | polymap.c:3971-3973 (fn MapMerge) | chebymap.rs:435-437 (fn try_merge), chebymap.rs:515-559 (fn equal) | architecture | ported |
| 22 | Re-instate the neighbour's Invert flag | polymap.c:3975-3976 (fn MapMerge) | -- | ceremony | n/a: the two ChebyMaps are compared as copies, so the list entries are never mutated and nothing is restored |
| 23 | Move on if the neighbour differs | polymap.c:3978-3980 (fn MapMerge) | chebymap.rs:435-437 (fn try_merge) | architecture | ported |
| 24 | Replace both entries with UnitMaps and leave the loop | polymap.c:3982-4001 (fn MapMerge) | chebymap.rs:440-441 (fn try_merge), mappings/simplify.rs:1449-1457 (fn default_map_merge) | architecture | ported |
| 25 | Re-instate the nominated PolyMap's Invert flag if it was not replaced | polymap.c:4004-4006 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 26 | Return the result | polymap.c:4009-4010 (fn MapMerge) | chebymap.rs:507 (fn map_merge) | ceremony | ported |

## Rows 16 to 21 — the comparison is `astEqual`, both here and in C

C forces both ChebyMaps' stored `Invert` flags to the direction the list applies
to the nominated one and then calls `astEqual`
(`polymap.c:3936-3969 (fn MapMerge)`). For a ChebyMap that reaches
`chebymap.c::Equal` (`chebymap.c:413-521 (fn Equal)`): the comparison inherited
from PolyMap, then the two bounding boxes. The inherited half rejects a pair whose
`Invert` flags differ (`polymap.c:830-835 (fn Equal)`) and the two entries here
are used in opposite directions by construction (row 19), so the forcing is not
an optimisation — without it the comparison could never succeed. What it compares,
once both flags read the same, is each transformation as constructed.

`ChebyMap::try_merge` forces the two directions on clones and calls `equal`, which
is why rows 22 and 25 have nothing to restore. Comparing the bounding boxes and
the forward terms in the merge instead reaches a *weaker* predicate: the inherited
half also compares the inverse coefficient counts and maximum powers
(`polymap.c:782-800 (fn Equal)`), so a pair whose inverse transformations have
different shapes cancels under the hand-rolled test and stands in C. The oracle
leaves such a pair as a CmpMap of two ChebyMaps; PLAN.md §10.30 records the fix
and `chebymap_does_not_cancel_when_the_inverse_shapes_differ` is the control.

Going through `equal` also inherits whatever the PolyMap comparison does with the
inverse coefficient *values*. Until AST 9.5.0 that block compared the forward
ones a second time, so a pair agreeing on their forward transformations, their
bounding boxes and the *shape* of their inverses cancelled however far apart their
inverse coefficients were; upstream `ec86cfaf` compares the inverse
transformation's own coefficients and powers, and ChebyMap picks that up through
`self.poly.equal` (`chebymap.rs:515 (fn equal)`).
`docs/issues/c-library-quirks.md` carries the history.

## Rows 9 to 11 — the linear replacement, with the normalisation folded in

C's linearity test reads `pmap0->ncoeff_f` and the coefficient arrays directly
(`polymap.c:3791 (fn MapMerge)`). Until AST 9.5.0 nothing there distinguished a
Chebyshev basis from a power basis: a first-order Chebyshev term is affine in the
input, but only after the domain scaling the ChebyMap's `lbnd`/`ubnd` define, and
that scaling took no part in the matrix and offset C accumulated. Settled on the
oracle at the time: it dropped the scaling. A one-input ChebyMap over bounds
`[0,10]` holding the single term `2 * T_1` computes `0.4x - 2`, and `astSimplify`
returned a ZoomMap with `Zoom = 2`, so `x = 0, 1, 10` gave `0, 2, 20` where the
ChebyMap gives `-2, -1.6, 2`. The port declined the step, and
`cheby_linear_lone_reduce` was its one `C_DEFECT_EXCLUDED` fixture row.

Upstream `abe5baae` fetches the normalisation from the nominated Mapping when it
is a ChebyMap (`polymap.c:3793-3799 (fn MapMerge)`) and folds it in: only
`T0(z) = 1` and `T1(z) = z` have degree one or less, so a term the linearity test
accepts is linear in the input as well as in `z`, and a term `c*T1(z_j)`
contributes `c*scale_j` to the matrix and `c*offset_j` to the constant term. Both
arrays accumulate rather than assign, because an output can now collect a constant
contribution from each of its linear terms as well as from its own constant term
(`polymap.c:3864-3883 (fn MapMerge)`).

The port has C's one seam, not two: `PolyMap::linear_replacement(terms, norm)`
takes C's `norm_scale` / `norm_offset` pair as its `norm` argument, `ChebyMap`
passes its `scale_f` / `offset_f` and `PolyMap` passes `None`. The `[0,10]`
ChebyMap now reduces to a WinMap with `Scl1 = 0.4` and `Sft1 = -2`, which is what
`cheby_linear_lone_reduce` records, and that row is compared byte for byte like
any other. `a_linear_chebymap_reduces_with_its_normalisation_folded_in` and
`a_linear_chebymap_accumulates_each_outputs_constant_contributions` in
`test_chebymap.rs` pin the two parts of the fix separately.

## Rows 4 to 8 and 12 — the consolidation half, ported

Combining coefficients that use the same set of input powers
(`polymap.c:3650-3771 (fn MapMerge)`) is basis-independent: two `T_2` terms for
one output add exactly as two `x^2` terms do. `PolyMap::consolidated_terms` is
that half on its own, reachable from both classes, and `ChebyMap::map_merge` calls
it and rebuilds a ChebyMap with the same bounds. There is no ChebyMap test inside
PolyMap's body and no PolyMap test inside ChebyMap's; each class calls what
applies to it.

Row 6 is C's deep copy. The Rust consolidation returns new term vectors instead,
so nothing is copied to be edited in place, and the rebuilt ChebyMap carries the
stored `Invert` flag as C's copy does. Row 8's replacement inherits the list
entry's applied direction rather than absorbing it, matching
`PolyMap::map_merge`.

`cap_cheby_consolidate` is the fixture: two forward `T_2` coefficients of 1.5 and
2.5 become one of 4, with a `T_3` term present so C stays out of the linear
branch. It fails byte-exactly without `ChebyMap::map_merge`.

## Rows 4 to 12 — the reduction is a step of the merge

The replace-with-simpler half is the head of C's `MapMerge`, not a separate
`Simplify` override, so a ChebyMap reduces only inside the merge loop — which is
what stops the `IsSimp` card landing on a leaf C never stamps. It is also what
makes the `Ident` guard reach this half: C's public dispatcher returns before any
per-class `MapMerge` runs when the nominated Mapping's `Ident` is set
(`mapping.c:24257 (fn astMapMerge_)`), so an identified ChebyMap keeps its
coefficients as constructed even when the tree around it simplifies. The port
applies that guard in `dispatch_map_merge`
(`mappings/simplify.rs:1312-1361 (fn dispatch_map_merge)`), which is how
`ChebyMap::map_merge` gets it; see PLAN.md §10.29.

## Row 24 — C leaves two UnitMaps, the port leaves one entry

C overwrites both list elements with a UnitMap and returns the lower index, so a
cancelling pair becomes two adjacent UnitMaps that a later pass removes. The port
composes the pair into a single UnitMap and closes the gap in `default_map_merge`,
returning the same index. The merge loop runs to convergence either way, and C's
own next pass strips the redundant UnitMap, so both routes settle on the same
list. The dimension is the one C uses: the input count of the pair as applied,
which is the nominated ChebyMap's effective `nin`
(`chebymap.rs:439-440 (fn try_merge)`).

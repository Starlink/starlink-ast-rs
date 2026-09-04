# `MapMerge` audit — ChebyMap

C source: reference/starlink-ast/src/polymap.c
Rust: crates/ast-core/src/mappings/chebymap.rs
Audited: 2026-08-24 against pin d0346c0d
Citations: verified against pin ca610685 (re-derived from file content at the
fixture-reorg sync; the numbers before that were correct against pin 3e60abaa)
Extension points: classified

Uncited C steps:
- polymap.c:3558 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

`chebymap.c` defines no `MapMerge`. Only `polymap.c:2439 (fn astInitPolyMapVtab_)`
fills the slot, so a ChebyMap's `MapMerge` *is* PolyMap's,
`polymap.c:3597-3961 (fn MapMerge)`. The rows below are that body, audited
against the Rust ChebyMap.

Neither `chebymap.c` nor `polymap.c` sets the `Simplify` slot, so every reduction
a ChebyMap has is reached through this body: the base `Simplify` nominates a lone
ChebyMap in a list of one (`mapping.c:16871-16883 (fn Simplify)`) and calls
`MapMerge` on it. The port matches that by leaving `Mapping::simplified` at its
default.

The Rust ChebyMap does not delegate to `PolyMap::map_merge`
(`polymap.rs:1544-1566 (fn map_merge)`). It has its own
(`chebymap.rs:506-555 (fn map_merge)`), which runs the coefficient consolidation
through `PolyMap::consolidated_terms`
(`polymap.rs:852-896 (fn consolidated_terms)`) and then falls through to the
shared direct merge; the neighbour cancellation is
`ChebyMap::try_merge` (`chebymap.rs:465-504 (fn try_merge)`).

C's linear replacement is deliberately not there. It drops the Chebyshev domain
scaling and so changes the transformation, which is a C defect settled on the
oracle rather than a shape the port should reproduce. Both halves are covered
below.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned index to -1 | polymap.c:3597-3598 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | polymap.c:3600-3601 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated PolyMap | polymap.c:3603-3604 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | See if the PolyMap can be replaced by a simpler Mapping, without reference to its neighbours | polymap.c:3606-3608 (fn MapMerge) | chebymap.rs:529-551 (fn map_merge) | architecture | ported |
| 5 | Get the input and output counts of the transformation as constructed | polymap.c:3610-3618 (fn MapMerge) | chebymap.rs:530-534 (fn map_merge) | architecture | ported |
| 6 | Take a deep copy to modify | polymap.c:3620-3622 (fn MapMerge) | chebymap.rs:535-548 (fn map_merge) | architecture | ported |
| 7 | Combine coefficients that refer to the same combination of input powers, forward then inverse | polymap.c:3624-3731 (fn MapMerge) | polymap.rs:852-896 (fn consolidated_terms) | architecture | ported |
| 8 | If any transformation changed, store the new PolyMap in place of the old and report a change | polymap.c:3732-3745 (fn MapMerge) | polymap.rs:891-895 (fn consolidated_terms), chebymap.rs:549-550 (fn map_merge) | architecture | ported |
| 9 | Test whether the forward transformation is linear, accumulating an offset vector and a matrix | polymap.c:3747-3842 (fn MapMerge) | -- | architecture | divergence: C mirrors PolyMap here and loses the Chebyshev domain scaling; docs/issues/c-library-quirks.md |
| 10 | If it is linear, build the ShiftMap and MatrixMap and compose them | polymap.c:3843-3855 (fn MapMerge) | -- | architecture | divergence: C mirrors PolyMap here and loses the Chebyshev domain scaling; docs/issues/c-library-quirks.md |
| 11 | Replace the PolyMap in the list with that composition | polymap.c:3856-3863 (fn MapMerge) | -- | architecture | divergence: C mirrors PolyMap here and loses the Chebyshev domain scaling; docs/issues/c-library-quirks.md |
| 12 | Return the index of the first modified element | polymap.c:3864-3866 (fn MapMerge) | chebymap.rs:551 (fn map_merge) | architecture | ported |
| 13 | Free the offset and matrix arrays | polymap.c:3868-3875 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 14 | Otherwise see if the PolyMap can be merged with a neighbour | polymap.c:3877-3879 (fn MapMerge) | chebymap.rs:554 (fn map_merge) | architecture | ported |
| 15 | The only merge available is a PolyMap with its own inverse, in series | polymap.c:3881-3886 (fn MapMerge) | chebymap.rs:472-478 (fn try_merge) | architecture | ported |
| 16 | Temporarily set the nominated PolyMap's Invert flag to the requested value | polymap.c:3887-3892 (fn MapMerge) | chebymap.rs:489-493 (fn try_merge) | architecture | ported |
| 17 | Read the nominated PolyMap's shape at that orientation | polymap.c:3893-3896 (fn MapMerge) | chebymap.rs:501-502 (fn try_merge) | architecture | ported |
| 18 | Take each neighbour in turn, skipping one that does not exist or is not a PolyMap | polymap.c:3897-3908 (fn MapMerge) | mappings/simplify.rs:1332-1353 (fn default_map_merge) | architecture | ported |
| 19 | Require the neighbour to be used in the opposite direction | polymap.c:3909-3911 (fn MapMerge) | chebymap.rs:476-478 (fn try_merge) | architecture | ported |
| 20 | Set the neighbour's Invert flag so the equality test compares like with like | polymap.c:3912-3921 (fn MapMerge) | chebymap.rs:494-496 (fn try_merge) | architecture | ported |
| 21 | Use astEqual to check the coefficients and the rest are equal | polymap.c:3922-3925 (fn MapMerge) | chebymap.rs:497-499 (fn try_merge), chebymap.rs:561-605 (fn equal) | architecture | ported |
| 22 | Re-instate the neighbour's Invert flag | polymap.c:3926-3928 (fn MapMerge) | -- | ceremony | n/a: the two ChebyMaps are compared as copies, so the list entries are never mutated and nothing is restored |
| 23 | Move on if the neighbour differs | polymap.c:3929-3932 (fn MapMerge) | chebymap.rs:497-499 (fn try_merge) | architecture | ported |
| 24 | Replace both entries with UnitMaps and leave the loop | polymap.c:3933-3954 (fn MapMerge) | chebymap.rs:501-503 (fn try_merge), mappings/simplify.rs:1407-1415 (fn default_map_merge) | architecture | ported |
| 25 | Re-instate the nominated PolyMap's Invert flag if it was not replaced | polymap.c:3955-3959 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 26 | Return the result | polymap.c:3960-3961 (fn MapMerge) | chebymap.rs:554 (fn map_merge) | ceremony | ported |

## Rows 16 to 21 — the comparison is `astEqual`, both here and in C

C forces both ChebyMaps' stored `Invert` flags to the direction the list applies
to the nominated one and then calls `astEqual`
(`polymap.c:3888-3920 (fn MapMerge)`). For a ChebyMap that reaches
`chebymap.c::Equal` (`chebymap.c:462-507 (fn Equal)`): the comparison inherited
from PolyMap, then the two bounding boxes. The inherited half rejects a pair whose
`Invert` flags differ (`polymap.c:806-812 (fn Equal)`) and the two entries here
are used in opposite directions by construction (row 19), so the forcing is not
an optimisation — without it the comparison could never succeed. What it compares,
once both flags read the same, is each transformation as constructed.

`ChebyMap::try_merge` forces the two directions on clones and calls `equal`, which
is why rows 22 and 25 have nothing to restore. Comparing the bounding boxes and
the forward terms in the merge instead reaches a *weaker* predicate: the inherited
half also compares the inverse coefficient counts and maximum powers
(`polymap.c:758-776 (fn Equal)`), so a pair whose inverse transformations have
different shapes cancels under the hand-rolled test and stands in C. The oracle
leaves such a pair as a CmpMap of two ChebyMaps; PLAN.md §10.30 records the fix
and `chebymap_does_not_cancel_when_the_inverse_shapes_differ` is the control.

Going through `equal` also adopts a C bug. The block that should compare the
inverse coefficient values and power patterns compares the forward ones a second
time (`polymap.c:778-804 (fn Equal)`), so two ChebyMaps agreeing on their forward
transformations, their bounding boxes and the *shape* of their inverses cancel
however far apart their inverse coefficients are. The oracle confirms the
cancellation, so `PolyMap::equal` reproduces it and ChebyMap inherits it through
`self.poly.equal` (`chebymap.rs:569 (fn equal)`).
`docs/issues/c-library-quirks.md` records it for upstream.

## Rows 9 to 11 — C's linear replacement is unsound for a Chebyshev basis

C's linearity test reads `pmap0->ncoeff_f` and the coefficient arrays directly
(`polymap.c:3747 (fn MapMerge)`) with nothing that distinguishes a Chebyshev basis
from a power basis. A first-order Chebyshev term is affine in the input, but only
after the domain scaling the ChebyMap's `lbnd`/`ubnd` define, and that scaling
takes no part in the matrix and offset C accumulates.

Settled on the oracle: it drops the scaling. A one-input ChebyMap over bounds
`[0,10]` holding the single term `2 * T_1` computes `2 * (0.2x - 1)`, and
`astSimplify` returns a ZoomMap with `Zoom = 2`, so `x = 0, 1, 10` gives
`0, 2, 20` where the ChebyMap gives `-2, -1.6, 2`. The reduction is sound only
when the bounds are `[-1,1]`, which makes the normalisation the identity, and the
branch has no ChebyMap guard. Recorded in `docs/issues/c-library-quirks.md`; the
reproduction is `cheby_linear_lone_reduce` and its non-linear control in
`ast_tester/gen_simplify_fixtures.c`, which stay out of the port's simplify
corpus because consuming them means reproducing the wrong Mapping.

The `[-1,1]` case is therefore a reduction C gets right and the port does not
make: a linear ChebyMap over those bounds should become a MatrixMap and a
ShiftMap here and stays a ChebyMap. That is marked `UNPORTED` in
`chebymap.rs:512-528 (fn map_merge)` and tracked as PLAN.md §10.2; it is deferred
rather than closed, because the fixtures that would pin it are part of the
upstream reorganisation the C pin is waiting on.

## Rows 4 to 8 and 12 — the consolidation half, ported

Combining coefficients that use the same set of input powers
(`polymap.c:3624-3745 (fn MapMerge)`) is basis-independent: two `T_2` terms for
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
(`mappings/simplify.rs:1309-1319 (fn dispatch_map_merge)`), which is how
`ChebyMap::map_merge` gets it; see PLAN.md §10.29.

## Row 24 — C leaves two UnitMaps, the port leaves one entry

C overwrites both list elements with a UnitMap and returns the lower index, so a
cancelling pair becomes two adjacent UnitMaps that a later pass removes. The port
composes the pair into a single UnitMap and closes the gap in `default_map_merge`,
returning the same index. The merge loop runs to convergence either way, and C's
own next pass strips the redundant UnitMap, so both routes settle on the same
list. The dimension is the one C uses: the input count of the pair as applied,
which is the nominated ChebyMap's effective `nin`
(`chebymap.rs:501-502 (fn try_merge)`).

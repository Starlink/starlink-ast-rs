# `MapMerge` audit — ChebyMap

C source: reference/starlink-ast/src/polymap.c
Rust: crates/ast-core/src/mappings/chebymap.rs
Audited: 2026-08-24 against pin d0346c0d

`chebymap.c` defines no `MapMerge`. Only `polymap.c:2439` sets the slot, so a
ChebyMap's `MapMerge` *is* PolyMap's, `polymap.c:3597-3961`. The rows below are
that body, audited against the Rust ChebyMap.

Neither `chebymap.c` nor `polymap.c` sets the `Simplify` slot, so every
reduction a ChebyMap has is reached through this body: the base `Simplify`
nominates a lone ChebyMap in a list of one (`mapping.c:16871-16883`) and calls
`MapMerge` on it. The port matches that by leaving `Mapping::simplified` at its
default.

The Rust ChebyMap does not delegate to `PolyMap::map_merge`
(`polymap.rs:1359`); it has no `map_merge` and takes the trait default, which is
the shared direct merge. So C's whole replace-with-simpler half is absent, and
only the neighbour-cancellation half is present, through `try_merge`.

Half of what is absent is a gap and half is deliberate. The coefficient
consolidation is basis-independent and belongs here; the linear replacement drops
the Chebyshev domain scaling and changes the transformation, so the port does not
perform it. Both are covered below.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned index to -1 | polymap.c:3597-3598 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | polymap.c:3600-3601 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Save a pointer to the nominated PolyMap | polymap.c:3603-3604 | -- | n/a: the Rust method already has `&self` |
| 4 | See if the PolyMap can be replaced by a simpler Mapping, without reference to its neighbours | polymap.c:3606-3608 | -- | gap: PLAN.md §10.2 |
| 5 | Get the input and output counts of the transformation as constructed | polymap.c:3610-3618 | -- | gap: PLAN.md §10.2 |
| 6 | Take a deep copy to modify | polymap.c:3620-3622 | -- | gap: PLAN.md §10.2 |
| 7 | Combine coefficients that refer to the same combination of input powers, forward then inverse | polymap.c:3624-3731 | -- | gap: PLAN.md §10.2 |
| 8 | If any transformation changed, store the new PolyMap in place of the old and report a change | polymap.c:3732-3745 | -- | gap: PLAN.md §10.2 |
| 9 | Test whether the forward transformation is linear, accumulating an offset vector and a matrix | polymap.c:3747-3842 | -- | divergence: C mirrors PolyMap here and loses the Chebyshev domain scaling; docs/issues/c-library-quirks.md |
| 10 | If it is linear, build the ShiftMap and MatrixMap and compose them | polymap.c:3843-3855 | -- | divergence: C mirrors PolyMap here and loses the Chebyshev domain scaling; docs/issues/c-library-quirks.md |
| 11 | Replace the PolyMap in the list with that composition | polymap.c:3856-3863 | -- | divergence: C mirrors PolyMap here and loses the Chebyshev domain scaling; docs/issues/c-library-quirks.md |
| 12 | Return the index of the first modified element | polymap.c:3864-3866 | -- | gap: PLAN.md §10.2 |
| 13 | Free the offset and matrix arrays | polymap.c:3868-3875 | -- | n/a: owned values are dropped |
| 14 | Otherwise see if the PolyMap can be merged with a neighbour | polymap.c:3877-3879 | chebymap.rs:454-460 | ported |
| 15 | The only merge available is a PolyMap with its own inverse, in series | polymap.c:3881-3886 | chebymap.rs:461-467 | ported |
| 16 | Temporarily set the nominated PolyMap's Invert flag to the requested value | polymap.c:3887-3892 | -- | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 17 | Take each neighbour in turn, skipping one that does not exist or is not a PolyMap | polymap.c:3893-3908 | chebymap.rs:468 | ported |
| 18 | Require the neighbour to be used in the opposite direction | polymap.c:3909-3911 | chebymap.rs:465-467 | ported |
| 19 | Use astEqual to check the coefficients and the rest are equal | polymap.c:3912-3925 | chebymap.rs:470-516 | ported |
| 20 | Replace both entries with UnitMaps and leave the loop | polymap.c:3933-3954 | chebymap.rs:517 | ported |
| 21 | Re-instate the nominated PolyMap's Invert flag if it was not replaced | polymap.c:3955-3959 | -- | n/a: nothing was mutated, so nothing is restored |
| 22 | Return the result | polymap.c:3960-3961 | chebymap.rs:517 | ported |

## Row 19 — the port checks more than C does

C compares two PolyMaps with `astEqual`, and ChebyMap's `Equal` covers its
bounds. The Rust `try_merge` compares the forward bounds, the inverse bounds and
the inner polynomial explicitly rather than calling `equal`, which reaches the
same predicate by a longer route.

## Rows 9 to 11 — C's linear replacement is unsound for a Chebyshev basis

C's linearity test reads `pmap0->ncoeff_f` and the coefficient arrays directly
(`polymap.c:3747`) with nothing that distinguishes a Chebyshev basis from a
power basis. A first-order Chebyshev term is affine in the input, but only after
the domain scaling the ChebyMap's `lbnd`/`ubnd` define, and that scaling takes no
part in the matrix and offset C accumulates.

Settled on the oracle: it drops the scaling. A one-input ChebyMap over bounds
`[0,10]` holding the single term `2 * T_1` computes `2 * (0.2x - 1)`, and
`astSimplify` returns a ZoomMap with `Zoom = 2`, so `x = 0, 1, 10` gives
`0, 2, 20` where the ChebyMap gives `-2, -1.6, 2`. The reduction is sound only
when the bounds are `[-1,1]`, which makes the normalisation the identity, and the
branch has no ChebyMap guard. Recorded in `docs/issues/c-library-quirks.md`; the
reproduction is `cheby_linear_lone_reduce` and its non-linear control in
`ast_tester/gen_simplify_fixtures.c`, which stay out of the port's simplify
corpus because consuming them means reproducing the wrong Mapping.

## Rows 4 to 8 and 12 — the consolidation half is still portable

Combining coefficients that use the same set of input powers
(`polymap.c:3624-3745`) is basis-independent: two `T_2` terms for one output add
exactly as two `x^2` terms do. That half remains a gap, tracked in PLAN.md §10.2,
and porting it requires reaching PolyMap's body without its linear branch.

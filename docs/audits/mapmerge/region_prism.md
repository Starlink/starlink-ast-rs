# `Simplify` / `GetDefUnc` audit — Prism

C source: reference/starlink-ast/src/prism.c
Rust: crates/ast-core/src/regions/prism.rs
Audited: 2026-08-21 against pin 3cf4bc5c

Two C bodies. `Simplify` (`prism.c:3286-3520`) tries to express the Prism's two
component Regions inside the parent's current Frame, collapsing the Prism to a
single Region or a simpler Prism, and only then calls the parent Region simplify.
`GetDefUnc` (`prism.c:1251-1279`) builds a Prism from the two components'
uncertainties.

Prism has no `MapMerge` of its own; it takes the Region base implementation.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned pointer | prism.c:3286-3287 | -- | n/a: the Rust returns `AstResult<DynMapping>` |
| 2 | Check the global error status and bail out | prism.c:3289-3290 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Region structure | prism.c:3292-3294 | -- | n/a: the Rust method already has `&self` |
| 4 | Get the two component Regions and the Prism's Negated flag | prism.c:3295-3297 | prism.rs:845-852 | ported |
| 5 | Get the parent's current Frame and its base-to-current Mapping | prism.c:3298-3304 | prism.rs:853-859 | ported |
| 6 | Combine the two Regions into a parallel CmpMap and simplify it, driving the per-class merges | prism.c:3305-3312 | prism.rs:861-870 | ported |
| 7 | Assume at first that no new Region can be formed from the result | prism.c:3313-3316 | prism.rs:871-872 | ported |
| 8 | If the result is a Region, use it | prism.c:3317-3320 | prism.rs:873-877 | ported |
| 9 | If it is a parallel CmpMap whose two components are Regions and differ from the originals, use those | prism.c:3321-3332 | prism.rs:878-896 | ported |
| 10 | Free resources | prism.c:3333-3341 | -- | n/a: owned values are dropped |
| 11 | Remap each component into the parent's current Frame and simplify it | prism.c:3342-3369 | prism.rs:897-905 | ported |
| 12 | Do the same for the second component | prism.c:3370-3379 | prism.rs:897-905 | ported |
| 13 | Use `astMapSplit` to find which current-Frame axes each component feeds | prism.c:3349-3383 | prism.rs:906-920 | ported |
| 14 | Assume the components cannot be simplified, in which case clone the supplied Prism | prism.c:3380-3383 | prism.rs:921-940 | ported |
| 15 | Determine the number of outputs from those Mappings | prism.c:3384-3395 | prism.rs:921-940 | ported |
| 16 | Determine the number of axes in the Prism's current Frame | prism.c:3396-3398 | prism.rs:921-940 | ported |
| 17 | If the second component contributes no axes to the Prism, ignore it | prism.c:3399-3415 | prism.rs:941-958 | ported |
| 18 | If the first component contributes none, ignore that instead | prism.c:3416-3432 | prism.rs:959-975 | ported |
| 19 | If both components occupy distinct subspaces, express each in its own subspace | prism.c:3433-3437 | prism.rs:976-988 | ported |
| 20 | Build a Frame for the subspace matching the first component's axes | prism.c:3438-3441 | prism.rs:976-988 | ported |
| 21 | Remap the first component into that subspace | prism.c:3442-3445 | prism.rs:976-988 | ported |
| 22 | Simplify the remapped Region | prism.c:3446-3448 | prism.rs:976-988 | ported |
| 23 | Do the same for the second component | prism.c:3449-3454 | prism.rs:976-988 | ported |
| 24 | If either simplified, build a new Prism from the results | prism.c:3455-3460 | prism.rs:989-1003 | ported |
| 25 | Give the new Prism the original's Negated attribute | prism.c:3461-3463 | prism.rs:989-1003 | ported |
| 26 | Give it the original current Frame's axis order | prism.c:3464-3474 | prism.rs:1004-1018 | ported |
| 27 | Free resources | prism.c:3475-3490 | -- | n/a: owned values are dropped |
| 28 | Preserve any explicitly stored uncertainty on the new Region | prism.c:3491-3499 | prism.rs:1019-1030 | ported |
| 29 | Only now invoke the parent Region simplify, on that Region | prism.c:3500-3505 | prism.rs:1019-1030 | ported |
| 30 | Free resources | prism.c:3506-3511 | -- | n/a: owned values are dropped |
| 31 | If anything simplified, overlay the original Region's attributes and return it | prism.c:3512-3515 | prism.rs:1038-1044 | ported |
| 32 | Annul the returned Mapping if an error occurred | prism.c:3516-3518 | -- | n/a: an error is a `Result::Err` |
| 33 | Return the result | prism.c:3519-3520 | prism.rs:1044 | ported |
| 34 | `GetDefUnc`: initialise and check the status | prism.c:1251-1256 | -- | n/a: the Rust returns `AstResult<DynRegion>` |
| 35 | Get a pointer to the Prism structure | prism.c:1257-1259 | -- | n/a: the Rust method already has `&self` |
| 36 | Build a default uncertainty from the two components' uncertainty Regions | prism.c:1260-1267 | prism.rs:627-660 | ported |
| 37 | Combine them into a Prism | prism.c:1268-1270 | prism.rs:627-660 | ported |
| 38 | Free resources and return NULL on error | prism.c:1271-1277 | -- | n/a: owned values are dropped and an error is a `Result::Err` |
| 39 | Return the Prism | prism.c:1278-1279 | prism.rs:660 | ported |

## Row 29 — the parent comes last here, not first

Every other Region calls the parent Region simplify first. Prism calls it *last*,
on whatever Region the component work produced, because the components' own
simplification changes which FrameSet the parent would be simplifying. The port
keeps that ordering (`prism.rs:1019-1030`); calling the parent first would
simplify a FrameSet that is about to be replaced.

## Row 31 — the attribute overlay does not carry the uncertainty

C passes `unc = 0` at `prism.c:3514`, unlike Box which passes 1: the uncertainty
has already been dealt with explicitly at row 28, so the overlay must not
overwrite it. The port matches, and records the reason at `prism.rs:1038`.

## Row 6 — the collapse is driven through the merge machinery

C builds a parallel CmpMap from the two Regions — using the fact that a Region is
a Mapping — and simplifies it, so the per-class `MapMerge` methods do the work of
deciding whether the two Regions combine. That is why `Interval`, `Box`,
`NullRegion` and `PointList` each need a Region `MapMerge` at all.

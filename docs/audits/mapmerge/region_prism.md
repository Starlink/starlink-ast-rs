# `Simplify` / `GetDefUnc` audit — Prism

C source: reference/starlink-ast/src/prism.c
Rust: crates/starlink-ast/src/regions/prism.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- prism.c:3254 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
- prism.c:1250 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

Two C bodies. `Simplify` (`prism.c:3292-3527 (fn Simplify)`) tries to express the
Prism's two component Regions inside the parent's current Frame, collapsing the
Prism to a single Region or a simpler Prism, and only then calls the parent Region
simplify. `GetDefUnc` (`prism.c:1256-1285 (fn GetDefUnc)`) builds a Prism from the
two components' uncertainties.

The Rust bodies are `prism.rs:1246-1458 (fn simplified)` and
`prism.rs:923-932 (fn get_def_unc)`.

Prism has no `MapMerge` of its own; it takes the Region base implementation.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `Simplify`: initialize the returned pointer | prism.c:3292-3294 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no pointer to pre-set |
| 2 | Check the global error status and bail out | prism.c:3295-3297 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Region structure | prism.c:3298-3300 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Get the two component Regions and the Prism's Negated flag | prism.c:3301-3303 (fn Simplify) | prism.rs:1252-1254 (fn simplified) | architecture | ported |
| 5 | Get the parent FrameSet's current Frame and its base-to-current Mapping, since the components describe areas in that FrameSet's base Frame | prism.c:3304-3310 (fn Simplify) | prism.rs:1257-1261 (fn simplified) | architecture | ported |
| 6 | Combine the two Regions into a parallel CmpMap, using the fact that a Region is a Mapping, and simplify it so the per-class `astMapMerge` methods decide whether they combine | prism.c:3311-3318 (fn Simplify) | prism.rs:1266-1271 (fn simplified) | architecture | ported |
| 7 | Initially assume no new Region can be formed from the simplified CmpMap | prism.c:3319-3322 (fn Simplify) | prism.rs:1275 (fn simplified) | architecture | ported |
| 8 | If the result is a Region, use it | prism.c:3323-3326 (fn Simplify) | prism.rs:1278-1279 (fn simplified) | architecture | ported |
| 9 | If it is a parallel CmpMap whose two components are Regions differing from the originals, form a new Prism from them | prism.c:3327-3338 (fn Simplify) | prism.rs:1283-1297 (fn simplified) | architecture | ported |
| 10 | Free resources | prism.c:3339-3345 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 11 | If that produced a simplified Region, map it into the parent's current Frame and simplify it | prism.c:3346-3353 (fn Simplify) | prism.rs:1302-1308 (fn simplified) | architecture | ported |
| 12 | If it did not, try a different approach | prism.c:3354-3356 (fn Simplify) | prism.rs:1309 (fn simplified) | architecture | ported |
| 13 | Get the number of axes in each component Region | prism.c:3357-3360 (fn Simplify) | prism.rs:1313-1314 (fn simplified) | architecture | ported |
| 14 | Use `astMapSplit` to see whether the first component's axes correspond to a distinct set of current-Frame axes | prism.c:3361-3375 (fn Simplify) | prism.rs:1319-1320 (fn simplified) | architecture | ported |
| 15 | Do the same for the second component | prism.c:3376-3385 (fn Simplify) | prism.rs:1321-1322 (fn simplified) | architecture | ported |
| 16 | Assume for the moment that the components cannot be simplified, in which case a clone of the supplied Prism is the answer | prism.c:3386-3389 (fn Simplify) | prism.rs:1326 (fn simplified) | architecture | ported |
| 17 | Determine the number of outputs from those two Mappings, counting a missing Mapping as none | prism.c:3390-3401 (fn Simplify) | prism.rs:1328-1329 (fn simplified),1313-1320 (fn simplified) | architecture | ported |
| 18 | Determine the number of axes in the Prism's current Frame | prism.c:3402-3404 (fn Simplify) | prism.rs:1330 (fn simplified) | architecture | ported |
| 19 | If the second component contributes no axes to the Prism, drop it: pick the first component's subspace Frame, remap and simplify the component into it, negate it if the Prism was negated, and restore the axis order | prism.c:3405-3421 (fn Simplify) | prism.rs:1343-1361 (fn simplified) | architecture | ported |
| 20 | If the first component contributes none, drop that one the same way instead | prism.c:3422-3438 (fn Simplify) | prism.rs:1362-1378 (fn simplified) | architecture | ported |
| 21 | If both components correspond to a distinct subspace of the current Frame, try expressing each within the current Frame | prism.c:3439-3443 (fn Simplify) | prism.rs:1379 (fn simplified) | architecture | ported |
| 22 | Create a Frame for the subspace matching the first component's axes | prism.c:3444-3447 (fn Simplify) | prism.rs:1383-1384 (fn simplified) | architecture | ported |
| 23 | Remap the first component so that it represents an area in that subspace | prism.c:3448-3451 (fn Simplify) | prism.rs:1385 (fn simplified) | architecture | ported |
| 24 | Attempt to simplify the remapped Region | prism.c:3452-3454 (fn Simplify) | prism.rs:1386 (fn simplified) | architecture | ported |
| 25 | Do the same for the second component Region | prism.c:3455-3460 (fn Simplify) | prism.rs:1388-1391 (fn simplified) | architecture | ported |
| 26 | If either component simplified, create a new Prism from the two results | prism.c:3461-3466 (fn Simplify) | prism.rs:1396-1404 (fn simplified) | architecture | ported |
| 27 | Give the new Prism the original's Negated attribute | prism.c:3467-3469 (fn Simplify) | prism.rs:1406-1408 (fn simplified) | architecture | ported |
| 28 | Give it the original current Frame's axis order | prism.c:3470-3480 (fn Simplify) | prism.rs:1411-1418 (fn simplified) | architecture | ported |
| 29 | Free resources | prism.c:3481-3489 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 30 | Free resources | prism.c:3490-3496 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 31 | Pass any uncertainty stored explicitly on the supplied Prism on to the new Region | prism.c:3497-3505 (fn Simplify) | prism.rs:1432-1434 (fn simplified) | architecture | ported |
| 32 | Only now invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and uncertainty | prism.c:3506-3511 (fn Simplify) | prism.rs:1435-1438 (fn simplified) | architecture | ported |
| 33 | Free resources | prism.c:3512-3517 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 34 | If anything simplified, copy the supplied Region's attributes onto the result and return it | prism.c:3518-3521 (fn Simplify) | prism.rs:1448-1451 (fn simplified) | architecture | ported |
| 35 | Annul the returned Mapping if an error occurred | prism.c:3522-3524 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 36 | Return the result | prism.c:3525-3527 (fn Simplify) | prism.rs:1453 (fn simplified) | architecture | ported |
| 37 | `GetDefUnc`: initialize the returned pointer | prism.c:1256-1258 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no pointer to pre-set |
| 38 | Check the global error status and bail out | prism.c:1259-1261 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 39 | Get a pointer to the Prism structure | prism.c:1262-1264 (fn GetDefUnc) | -- | ceremony | n/a: the Rust method already has `&self` |
| 40 | Take the uncertainty Regions of the two components, whose current Frames are the components of the Prism's base Frame | prism.c:1265-1272 (fn GetDefUnc) | prism.rs:928-929 (fn get_def_unc) | architecture | ported |
| 41 | Combine them into a Prism | prism.c:1273-1275 (fn GetDefUnc) | prism.rs:930-931 (fn get_def_unc) | architecture | ported |
| 42 | Free resources | prism.c:1276-1279 (fn GetDefUnc) | -- | ceremony | n/a: owned values are dropped |
| 43 | Return NULL if an error occurred | prism.c:1280-1282 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 44 | Return the required pointer | prism.c:1283-1285 (fn GetDefUnc) | prism.rs:931 (fn get_def_unc) | architecture | ported |

## Row 6 — the collapse is driven through the merge machinery

C builds a parallel CmpMap from the two Regions — using the fact that a Region is
a Mapping — and simplifies it, so the per-class `MapMerge` methods do the work of
deciding whether the two Regions combine. That is why `Interval`, `Box`,
`NullRegion` and `PointList` each need a Region `MapMerge` at all, and why
`region_map_merge` (`mappings/simplify.rs:1692-1756 (fn region_map_merge)`) must be
reachable from a parallel CmpMap's own merge.

## Rows 14 and 15 — `MapSplit` is the seam, not a convenience

The second approach turns on whether each component's axes reach a *distinct* set
of current-Frame axes, which is exactly what `astMapSplit` answers and nothing
else does. The port calls `Mapping::map_split`
(`prism.rs:1319-1322 (fn simplified)`), the trait method every class implements at C's
granularity, rather than re-deriving the answer from the Mapping's class. A
component whose split fails contributes zero outputs (row 17), which is how the
`naxout1 == naxt && naxout2 == 0` tests in rows 19 and 20 can fire at all.

## Rows 19, 20 and 21 — an `else if` chain, in C's order

C tests the three cases in one chain: second component contributes nothing, then
first contributes nothing, then both split successfully. The order matters,
because a component that splits to zero outputs also satisfies `nmap1 && nmap2`
when its split succeeded with no outputs, and the earlier arms must claim it
first. The port keeps the chain and its order
(`prism.rs:1343 (fn simplified)`, `prism.rs:1362 (fn simplified)`, `prism.rs:1379 (fn simplified)`), and the final
`else` is absent in both: with none of the three matching, `new` keeps the clone
row 16 assigned.

## Row 32 — the parent comes last here, not first

Every other Region calls the parent Region simplify first. Prism calls it *last*,
on whatever Region the component work produced, because the components' own
simplification changes which FrameSet the parent would be simplifying. The port
keeps that ordering (`prism.rs:1435-1438 (fn simplified)`); calling the parent first
would simplify a FrameSet that is about to be replaced.

The C split path always leaves `new` non-NULL, since row 16 assigns a clone of the
supplied Prism before any arm runs, so the parent simplify is always reached. It
returns its input unchanged when nothing reduces, which is what makes row 34's
single comparison sufficient.

## Row 34 — the attribute overlay does not carry the uncertainty

C passes `unc = 0` at `prism.c:3520 (fn Simplify)`, unlike Box which passes 1: the
uncertainty was dealt with explicitly at row 31, so the overlay must not overwrite
it. The port passes the same flag (`prism.rs:1450 (fn simplified)`).

Prism keeps no `simpler` flag: C decides both the overlay and whether anything
reduced from the single test `result != this_mapping`
(`prism.c:3519 (fn Simplify)`). Owned values make that a structural comparison
(`prism.rs:1448 (fn simplified)`) rather than a pointer test, which is stricter — a
rebuilt but identical Prism reports no change where C would report one.

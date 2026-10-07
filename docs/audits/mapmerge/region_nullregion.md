# `Simplify` / `MapMerge` / `GetDefUnc` audit — NullRegion

C source: reference/starlink-ast/src/nullregion.c
Rust: crates/ast-core/src/regions/nullregion.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- nullregion.c:471 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- nullregion.c:1358 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
- nullregion.c:211 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

Three short C bodies. `MapMerge` (`nullregion.c:479-569 (fn MapMerge)`) is the Region
shape: self-simplify first, and only failing that merge with a neighbour, in
parallel only. `Simplify` (`nullregion.c:1365-1408 (fn Simplify)`) calls the parent and,
when the base-to-current Mapping is not a UnitMap, rebuilds the NullRegion on the
current Frame. `GetDefUnc` (`nullregion.c:218-235 (fn GetDefUnc)`) is a zero-radius Circle
at the origin.

`NullRegion::map_merge` (`nullregion.rs:728-731 (fn map_merge)`) delegates to
`simplify::region_map_merge` (`mappings/simplify.rs:1443-1490 (fn region_map_merge)`), which every
Region shares; `NullRegion::simplified` (`nullregion.rs:622-690 (fn simplified)`) and
`NullRegion::get_def_unc` (`nullregion.rs:309-319 (fn get_def_unc)`) are the other two.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `MapMerge`: initialise the result and the merged-pair index | nullregion.c:479-482 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | nullregion.c:483-485 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the NullRegion | nullregion.c:486-488 (fn MapMerge) | -- | ceremony | n/a: the Rust function already has `this: &dyn Region` |
| 4 | Banner: first see whether the NullRegion reduces on its own, without reference to its neighbours | nullregion.c:489-492 (fn MapMerge) | -- | ceremony | n/a: a section banner, not a step |
| 5 | Simplify the NullRegion, assuming a changed pointer means something happened | nullregion.c:493-497 (fn MapMerge) | mappings/simplify.rs:1455-1456 (fn region_map_merge) | architecture | ported |
| 6 | Annul the NullRegion pointer in the list, replace it with the new Region, and mark it forward | nullregion.c:498-504 (fn MapMerge) | mappings/simplify.rs:1457 (fn region_map_merge) | architecture | ported |
| 7 | Return the index of the first modified element | nullregion.c:505-507 (fn MapMerge) | mappings/simplify.rs:1458 (fn region_map_merge) | architecture | ported |
| 8 | Failing that, merge with the Regions on either side — which is possible in parallel only | nullregion.c:508-512 (fn MapMerge) | mappings/simplify.rs:1463 (fn region_map_merge) | architecture | ported |
| 9 | Annul the unwanted simplified pointer | nullregion.c:513-514 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 10 | Attempt to merge the NullRegion with its lower neighbour, if that neighbour is a Region | nullregion.c:515-524 (fn MapMerge) | mappings/simplify.rs:1465-1473 (fn region_map_merge) | architecture | ported |
| 11 | Failing that, attempt the upper neighbour the same way | nullregion.c:525-535 (fn MapMerge) | mappings/simplify.rs:1477-1486 (fn region_map_merge) | architecture | ported |
| 12 | Act only if one of the two produced a merged Region | nullregion.c:536-538 (fn MapMerge) | mappings/simplify.rs:1470 (fn region_map_merge),1482 (fn region_map_merge) | architecture | ported |
| 13 | Annul the first of the two Mappings, replace it with the merged Region, and clear its invert flag | nullregion.c:539-544 (fn MapMerge) | mappings/simplify.rs:589 (fn replace_pair_with) | architecture | ported |
| 14 | Annul the second and shuffle the rest of the list down to fill the gap | nullregion.c:545-552 (fn MapMerge) | mappings/simplify.rs:590-591 (fn replace_pair_with) | architecture | ported |
| 15 | Clear the vacated element at the end | nullregion.c:553-556 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entry leaves no slot to blank |
| 16 | Decrement the Mapping count and return the index of the first modified element | nullregion.c:557-562 (fn MapMerge) | mappings/simplify.rs:1472 (fn region_map_merge),1484 (fn region_map_merge) | architecture | ported |
| 17 | In series, annul the simplified pointer and do nothing | nullregion.c:563-566 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and the series case falls through to `None` |
| 18 | Return the result | nullregion.c:567-569 (fn MapMerge) | mappings/simplify.rs:1489 (fn region_map_merge) | architecture | ported |
| 19 | `Simplify`: initialise the returned pointer | nullregion.c:1365-1367 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no sentinel to initialise |
| 20 | Check the global error status and bail out | nullregion.c:1368-1370 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 21 | Get a pointer to the supplied Region structure | nullregion.c:1371-1373 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 22 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and the uncertainty | nullregion.c:1374-1377 (fn Simplify) | nullregion.rs:622-635 (fn simplified) | architecture | ported |
| 23 | If the base-to-current Mapping is a UnitMap, no further simplification is possible, so clone the parent's result | nullregion.c:1378-1383 (fn Simplify) | nullregion.rs:639-646 (fn simplified) | architecture | ported |
| 24 | Otherwise take the other branch | nullregion.c:1384-1385 (fn Simplify) | nullregion.rs:647 (fn simplified) | architecture | ported |
| 25 | Create a NullRegion from the current Frame, carrying the parent's uncertainty | nullregion.c:1386-1389 (fn Simplify) | nullregion.rs:650-671 (fn simplified) | architecture | ported |
| 26 | Free the Frame | nullregion.c:1390-1393 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 27 | Annul the Mapping and the parent's Region | nullregion.c:1394-1396 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 28 | If anything simplified, overlay the supplied Region's attributes onto the result | nullregion.c:1397-1402 (fn Simplify) | nullregion.rs:683-685 (fn simplified) | architecture | ported |
| 29 | Annul the returned pointer if an error occurred | nullregion.c:1403-1405 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 30 | Return the result | nullregion.c:1406-1408 (fn Simplify) | nullregion.rs:686 (fn simplified),667 (fn simplified) | architecture | ported |
| 31 | `GetDefUnc`: initialise | nullregion.c:218-220 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no sentinel to initialise |
| 32 | Check the inherited status | nullregion.c:221-223 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 33 | Create a Circle centred on the base Frame origin with zero radius | nullregion.c:224-232 (fn GetDefUnc) | nullregion.rs:313-318 (fn get_def_unc) | architecture | ported |
| 34 | Return the default uncertainty Region | nullregion.c:233-235 (fn GetDefUnc) | nullregion.rs:318 (fn get_def_unc) | architecture | ported |

## Row 33 — a Circle, not the bounding box

A NullRegion has no extent, so the base-class bounding-box rule has nothing to
measure. C overrides it with a degenerate Circle at the base Frame origin, and
the port does the same. That also means this class is not affected by the
base-class guard defect in PLAN.md §10.12.

## Row 25 — the current Frame, carrying the parent's uncertainty

The rebuilt NullRegion is created on the *parent-simplified* current Frame and
carries the parent's uncertainty, not the original's, so the FrameSet and
uncertainty simplification the parent performed is not discarded by the rebuild.
The remaining Region attributes are copied across the rebuild as well
(`nullregion.rs:653-660 (fn simplified)`), because a fresh `RegionData` would otherwise
default them; row 28's `reg_overlay` then restores the *supplied* Region's
values over the top, which is what C's `astRegOverlay` does.

## Rows 23, 24 and 28 — what counts as a simplification

C has no `simpler` flag: it compares `result != this_mapping`
(`nullregion.c:1401 (fn Simplify)`), which is true whenever the non-UnitMap branch built a
fresh NullRegion, and true through the UnitMap branch only when the parent
returned something other than the supplied pointer — `astClone( new )` is that
pointer exactly when the parent simplified nothing. `nullregion.rs:683 (fn simplified)`
spells the same condition as `simpler || !map.as_any().is::<UnitMap>()`, where
`simpler` records whether the parent changed anything.

## Rows 10 to 12 — the lower neighbour comes first, and why it needs its own path

C offers the lower neighbour before the upper, and passes `intfirst = 0` for the
first and `1` for the second, which is the order the merged Region's axes end up
in. `region_map_merge` keeps that order and that flag
(`mappings/simplify.rs:1468 (fn region_map_merge)`,1480). The lower-neighbour case cannot come
through `NullRegion::try_merge` (`nullregion.rs:709-722 (fn try_merge)`), because
`default_map_merge` only ever offers a nominee the entry at `where + 1`; sharing
this body across the Region classes is what gives every one of them C's
lower-neighbour branch.

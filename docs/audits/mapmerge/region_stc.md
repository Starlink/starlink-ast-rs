# `Simplify` / `GetDefUnc` audit — Stc

C source: reference/starlink-ast/src/stc.c
Rust: crates/ast-core/src/regions/stc.rs
Audited: 2026-10-09 against pin f24ee843
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- stc.c:2640 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
- stc.c:937 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

stc.c has no `MapMerge`; an Stc reaches a neighbour only through the Region it
encapsulates. `Simplify` (`stc.c:2653-2802 (fn Simplify)`) simplifies the Region
part, moves any negation onto the encapsulated Region, re-defines that Region in
the current Frame, and does the same for each Region in the AstroCoords
elements. `GetDefUnc` (`stc.c:941-966 (fn GetDefUnc)`) prefers the encapsulated
Region's own uncertainty.

`Stc::simplified` (`stc.rs:1304-1374 (fn simplified)`) and `Stc::get_def_unc`
(`stc.rs:1089-1095 (fn get_def_unc)`) are the two.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `Simplify`: check the global error status | stc.c:2653-2654 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 2 | Invoke the parent Region `Simplify`, which simplifies the FrameSet and the uncertainty | stc.c:2656-2658 (fn Simplify) | stc.rs:1305-1314 (fn simplified) | architecture | ported |
| 3 | If the Stc is negated, move the negation onto the encapsulated Region | stc.c:2660-2662 (fn Simplify) | stc.rs:1323 (fn simplified) | architecture | ported |
| 4 | Copy the supplied Stc first if the parent returned it unchanged | stc.c:2664-2670 (fn Simplify) | stc.rs:1316-1321 (fn simplified),1324 (fn simplified) | architecture | ported |
| 5 | Negate both the Stc and its encapsulated Region | stc.c:2672-2677 (fn Simplify) | stc.rs:1325-1326 (fn simplified) | architecture | ported |
| 6 | Get the base->current Mapping and the current Frame | stc.c:2679-2682 (fn Simplify) | stc.rs:1329-1334 (fn simplified) | architecture | ported |
| 7 | Where that Mapping is not a UnitMap, map the encapsulated Region into the current Frame and simplify it | stc.c:2684-2692 (fn Simplify) | stc.rs:1341-1343 (fn simplified),1347 (fn simplified) | architecture | ported |
| 8 | Otherwise simplify the encapsulated Region as it is | stc.c:2694-2699 (fn Simplify) | stc.rs:1339-1340 (fn simplified) | architecture | ported |
| 9 | If the encapsulated Region changed, store it in the returned Stc | stc.c:2701-2703 (fn Simplify) | stc.rs:1348 (fn simplified) | architecture | ported |
| 10 | Copy the supplied Stc first if that has not been done | stc.c:2705-2711 (fn Simplify) | stc.rs:1349 (fn simplified) | architecture | ported |
| 11 | Store the new Region, annulling the old | stc.c:2713-2717 (fn Simplify) | stc.rs:1350 (fn simplified) | architecture | ported |
| 12 | The Region now describes the current Frame, so give the Stc a FrameSet of that Frame alone | stc.c:2719-2725 (fn Simplify) | stc.rs:1351-1353 (fn simplified) | architecture | ported |
| 13 | Free the simplified Region pointer | stc.c:2727-2728 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 14 | Loop round the AstroCoords elements | stc.c:2730-2733 (fn Simplify) | stc.rs:1356 (fn simplified) | architecture | ported |
| 15 | Get a pointer to the element's KeyMap | stc.c:2735-2737 (fn Simplify) | -- | ceremony | n/a: the loop indexes the Stc's KeyMaps where they are stored |
| 16 | Loop round the keys that may hold a Region | stc.c:2739-2741 (fn Simplify) | stc.rs:1357 (fn simplified) | architecture | ported |
| 17 | Get the Region the KeyMap holds for the key, if any | stc.c:2743-2745 (fn Simplify) | stc.rs:1358-1360 (fn simplified) | architecture | ported |
| 18 | Map the Region into the new base Frame unless the Mapping is a UnitMap | stc.c:2747-2762 (fn Simplify) | stc.rs:1361 (fn simplified),1341-1343 (fn simplified) | architecture | ported |
| 19 | Simplify the Region | stc.c:2764-2765 (fn Simplify) | stc.rs:1361 (fn simplified),1340 (fn simplified) | architecture | ported |
| 20 | If it changed, copy the supplied Stc first if needed, and store it back with its comment | stc.c:2767-2782 (fn Simplify) | stc.rs:1362-1365 (fn simplified) | architecture | ported |
| 21 | Free the Region pointers, and close the loops | stc.c:2784-2792 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 22 | Free the Mapping and Frame | stc.c:2794-2796 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 23 | Annul the result if an error occurred | stc.c:2798-2799 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 24 | Return the result | stc.c:2801-2802 (fn Simplify) | stc.rs:1369-1373 (fn simplified) | architecture | ported |
| 25 | `GetDefUnc`: initialise | stc.c:941-942 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no sentinel to initialise |
| 26 | Check the global error status | stc.c:944-945 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 27 | Get a pointer to the Stc | stc.c:947-948 (fn GetDefUnc) | -- | ceremony | n/a: the Rust method already has `&self` |
| 28 | Use the encapsulated Region's own uncertainty, in its current Frame, which is the Stc's base Frame | stc.c:950-955 (fn GetDefUnc) | stc.rs:1090-1091 (fn get_def_unc) | architecture | ported |
| 29 | Otherwise use the Region default | stc.c:957-960 (fn GetDefUnc) | stc.rs:1092-1094 (fn get_def_unc) | architecture | ported |
| 30 | Annul the result if an error occurred | stc.c:962-963 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 31 | Return the default uncertainty Region | stc.c:965-966 (fn GetDefUnc) | stc.rs:1090-1094 (fn get_def_unc) | architecture | ported |

## Rows 3 to 5 — moving the negation

An Stc's Negated is read from and set on its encapsulated Region, so after
`astNegate( stc )` and `astNegate( stc->region )` the Region is negated once
more and the Stc's own field is cleared to false: the Stc stays negated, through
its Region. The port's `negate` sets both, as stc.c `SetNegated` does
(`stc.rs:1325-1326 (fn simplified)`), and the net state is the same.

## Rows 4, 10 and 20 — the copy

C copies the supplied Stc the first time it is about to change it, unless the
parent `Simplify` already returned a new one. `astCopy` clears `RegionFS` on the
copy and on each Region it holds, so the port clears it there too, once
(`stc.rs:1316-1321 (fn simplified)`), and returns the copy, with `true` for
"simplified", exactly when C returns a pointer other than the one supplied.

## Rows 7, 8, 18 and 19 — "changed"

C tests whether `astSimplify` gave back a different pointer. Where the Mapping is
not a UnitMap the Region simplified is a new, mapped one, so the test always
passes; where it is a UnitMap the port asks `simplify_reporting_change`
(`stc.rs:1339-1344 (fn simplified)`).

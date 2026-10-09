# `Simplify` / `GetDefUnc` audit — CmpRegion

C source: reference/starlink-ast/src/cmpregion.c
Rust: crates/ast-core/src/regions/cmpregion.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- cmpregion.c:3564 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
- cmpregion.c:1128 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

Two C bodies. `Simplify` (`cmpregion.c:3584-3789 (fn Simplify)`) calls the parent,
simplifies the two component Regions, and then works through the ways a boolean
combination of two Regions collapses: a null or infinite component, no overlap
under AND, a negation pair under OR, identical components, and one component
contained in the other. `GetDefUnc` (`cmpregion.c:1132-1163 (fn GetDefUnc)`) takes the
default uncertainty from whichever component carries an explicit one.

The Rust counterparts are `simplified` (`cmpregion.rs:1263-1431 (fn simplified)`) and
`get_def_unc` (`cmpregion.rs:1002-1014 (fn get_def_unc)`).

CmpRegion has no `MapMerge` of its own; it takes the Region base implementation.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned pointer | cmpregion.c:3584-3586 (fn Simplify) | -- | ceremony | n/a: the Rust signature returns `AstResult<(DynMapping, bool)>`, so there is no pointer to pre-set |
| 2 | Check the global error status and bail out | cmpregion.c:3587-3589 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Invoke the parent Region simplify, which simplifies the encapsulated FrameSet and uncertainty, giving a Region in the current Frame | cmpregion.c:3590-3596 (fn Simplify) | cmpregion.rs:1268-1270 (fn simplified) | architecture | ported |
| 4 | Note whether that simplified, by comparing the returned pointer with the supplied one | cmpregion.c:3597-3601 (fn Simplify) | cmpregion.rs:1271 (fn simplified) | architecture | ported |
| 5 | Initialise the base-Frame candidate to "not produced" | cmpregion.c:3602-3609 (fn Simplify) | cmpregion.rs:1308 (fn simplified) | architecture | ported |
| 6 | Get the component Regions, how they should be combined, and the Negated values they should be used with, all in the base Frame | cmpregion.c:3610-3616 (fn Simplify) | cmpregion.rs:1282 (fn simplified) | architecture | ported |
| 7 | Where the first component does not carry its required `Negated` value, use its negation instead | cmpregion.c:3617-3625 (fn Simplify) | cmpregion.rs:1286-1288 (fn simplified) | architecture | ported |
| 8 | The same for the second component | cmpregion.c:3626-3634 (fn Simplify) | cmpregion.rs:1289-1291 (fn simplified) | architecture | ported |
| 9 | Simplify each of the two components | cmpregion.c:3635-3638 (fn Simplify) | cmpregion.rs:1294-1297 (fn simplified) | architecture | ported |
| 10 | Note whether either component simplified, keeping any earlier flag | cmpregion.c:3639-3641 (fn Simplify) | cmpregion.rs:1302-1304 (fn simplified) | architecture | ported |
| 11 | A null or infinite component can be excluded from the result | cmpregion.c:3642-3645 (fn Simplify) | cmpregion.rs:1312-1314 (fn simplified) | architecture | ported |
| 12 | Separate the null component from the other one, which stays valid when both are null | cmpregion.c:3646-3655 (fn Simplify) | cmpregion.rs:1317-1321 (fn simplified) | architecture | ported |
| 13 | An un-negated null component: under AND the result is null in the other Region's Frame, under OR it is the other Region | cmpregion.c:3656-3671 (fn Simplify) | cmpregion.rs:1323-1334 (fn simplified) | architecture | ported |
| 14 | A negated (infinite) component: under AND the result is the other Region, under OR it is infinite | cmpregion.c:3672-3688 (fn Simplify) | cmpregion.rs:1335-1347 (fn simplified) | architecture | ported |
| 15 | Flag that simplification took place | cmpregion.c:3689-3691 (fn Simplify) | cmpregion.rs:1348 (fn simplified) | architecture | ported |
| 16 | With neither component null or infinite, take the overlap between them as the grounds for removing one or both | cmpregion.c:3692-3697 (fn Simplify) | cmpregion.rs:1352-1353 (fn simplified) | architecture | ported |
| 17 | No overlap, or exact negation, combined with AND: the result is null | cmpregion.c:3698-3704 (fn Simplify) | cmpregion.rs:1357-1360 (fn simplified) | architecture | ported |
| 18 | One component the negation of the other, combined with OR: the result is infinite, written as a negated null Region | cmpregion.c:3705-3712 (fn Simplify) | cmpregion.rs:1364-1367 (fn simplified) | architecture | ported |
| 19 | The two components are identical | cmpregion.c:3713-3716 (fn Simplify) | cmpregion.rs:1370 (fn simplified) | architecture | ported |
| 20 | Under AND or OR, identical components collapse to either one | cmpregion.c:3717-3727 (fn Simplify) | cmpregion.rs:1371-1372 (fn simplified) | architecture | ported |
| 21 | The first component lies inside the second: AND keeps the first, OR keeps the second | cmpregion.c:3728-3734 (fn Simplify) | cmpregion.rs:1376-1383 (fn simplified) | architecture | ported |
| 22 | The second lies inside the first: AND keeps the second, OR keeps the first | cmpregion.c:3735-3741 (fn Simplify) | cmpregion.rs:1386-1393 (fn simplified) | architecture | ported |
| 23 | Otherwise build a new CmpRegion from the simplified components, but only where something simplified; leaving the base-Frame candidate unset makes the parent-simplified copy the answer | cmpregion.c:3742-3754 (fn Simplify) | cmpregion.rs:1396-1406 (fn simplified) | architecture | ported |
| 24 | Something simplified: use the base-Frame candidate if there is one, mapping it into the current Frame with the parent FrameSet's base-to-current Mapping, otherwise use the parent-simplified copy | cmpregion.c:3755-3771 (fn Simplify) | cmpregion.rs:1413-1424 (fn simplified) | architecture | ported |
| 25 | Nothing simplified: return a clone of the supplied Region | cmpregion.c:3772-3776 (fn Simplify) | cmpregion.rs:1413-1415 (fn simplified) | architecture | ported |
| 26 | Free the component and parent-simplified pointers | cmpregion.c:3777-3783 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 27 | Annul the result if an error occurred | cmpregion.c:3784-3786 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 28 | Return the result | cmpregion.c:3787-3789 (fn Simplify) | cmpregion.rs:1426 (fn simplified) | ceremony | ported |
| 29 | Initialise the returned pointer | cmpregion.c:1132-1134 (fn GetDefUnc) | -- | ceremony | n/a: the Rust signature returns `AstResult<DynRegion>`, so there is no pointer to pre-set |
| 30 | Check the global error status and bail out | cmpregion.c:1135-1137 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 31 | Get a pointer to the CmpRegion structure | cmpregion.c:1138-1140 (fn GetDefUnc) | -- | ceremony | n/a: `&self` is already the CmpRegion |
| 32 | Where the first component has a non-default uncertainty, take it as the CmpRegion's default; an uncertainty's current Frame is the CmpRegion's base Frame | cmpregion.c:1141-1147 (fn GetDefUnc) | cmpregion.rs:1003-1004 (fn get_def_unc) | architecture | ported |
| 33 | Otherwise, where the second component has a non-default uncertainty, take that | cmpregion.c:1148-1152 (fn GetDefUnc) | cmpregion.rs:1005-1006 (fn get_def_unc) | architecture | ported |
| 34 | Otherwise defer to the parent method | cmpregion.c:1153-1157 (fn GetDefUnc) | cmpregion.rs:1007-1008 (fn get_def_unc) | architecture | ported |
| 35 | Return NULL if an error occurred | cmpregion.c:1158-1160 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 36 | Return the required pointer | cmpregion.c:1161-1163 (fn GetDefUnc) | -- | ceremony | n/a: the chosen branch is the tail expression of the `if`/`else if`/`else`, so there is no separate return |

## Rows 4 and 10 — the flag is set by comparing values, not pointers

C decides both times by pointer identity: `newc != this_mapping` at
`cmpregion.c:3600 (fn Simplify)` and `sreg1 != reg1 || sreg2 != reg2` at
`cmpregion.c:3640 (fn Simplify)`, each true whenever `astSimplify` returned a new
object rather than a clone. Owned values have no such identity, so the port
compares structurally (`cmpregion.rs:1271 (fn simplified)` and
`cmpregion.rs:1302-1304 (fn simplified)`). That is the stricter test: a
simplification that produced an equal value would set C's flag and not the
port's. Both flags then feed only the choice between a rebuilt Region and a clone
of the input, so the stricter reading cannot lose a reduction — it can only
decline to announce one that changed nothing.

## Rows 5, 23 and 24 — two candidate results, in two Frames

C carries two pointers: `newb`, a Region in the parent's *base* Frame, and
`newc`, one in the current Frame. Which it returns depends on which was produced,
and a base-Frame result has to be mapped into the current Frame first. The port
keeps both candidates for the same reason (`cmpregion.rs:1308 (fn simplified)` and
`cmpregion.rs:1417-1424 (fn simplified)`); collapsing them to one would return a Region
defined in the wrong Frame for whichever branch produced it.

## Rows 6 to 8 — the required Negated value is not the stored one

`GetRegions` (`cmpregion.c:981 (fn GetRegions)`) returns the Negated value each
component *should* be used with, which need not be the value the component
stores: a negated CmpRegion is rewritten by De Morgan, swapping AND for OR and
toggling both component flags. C substitutes the negation of the component where
the two disagree, and the port does the same (`cmpregion.rs:1286-1291 (fn simplified)`)
before any of the overlap tests — otherwise every subsequent test would be asking
about the wrong region.

C reads the components from `newc`, the parent-simplified copy, rather than from
the supplied Region; the port reads them from `self`
(`cmpregion.rs:1282 (fn simplified)`). The two agree because the parent
`Simplify` (`region.c:11060 (fn Simplify)`) rewrites only the encapsulated FrameSet and
the uncertainty, and `newc` holds deep copies of the components either way.

## Rows 13, 14 and 20 — C's unknown-operator error has nowhere to arise

Each of these three branches ends in an `else` that raises `AST__INTER` for an
operator that is neither AND nor OR (`cmpregion.c:3665 (fn Simplify)`,
`cmpregion.c:3681 (fn Simplify)`, `cmpregion.c:3721 (fn Simplify)`), because C stores the
operator as an `int`. The port's `CmpOperator` has exactly the two variants, so
the `match` arms are exhaustive and there is no third case to report. Nothing is
dropped: the error was C's guard against a value the Rust type cannot hold.

## Row 18 — why the negation test is restricted

C notes that "one component is the negation of the other" is only recognised in
the OR case. The port keeps that restriction rather than generalising it, which
is the negative-control side of this rule: an AND of a Region and its negation is
null, and that is reached by row 17's no-overlap test instead, whose guard admits
the negation code alongside the no-overlap one
(`cmpregion.rs:1357 (fn simplified)`).

## Row 24 — the mapped result is returned as it stands

`astMapRegion` (`region.c:5390 (fn MapRegion)`) re-expresses the base-Frame candidate
in the current Frame by adding the Mapping and Frame to a copy of its FrameSet.
C returns that copy directly (`cmpregion.c:3764 (fn Simplify)`) without simplifying it
again, and the port matches (`cmpregion.rs:1418 (fn simplified)`). This is the point
where a second `astSimplify` would be easy to add and wrong: the components were
already simplified at row 9, the base-to-current Mapping was already simplified
by the parent at row 3, and re-simplifying the assembled Region would let a
class-level conversion — a Box becoming a Polygon, say — run one step further
than C runs it, changing the dump.

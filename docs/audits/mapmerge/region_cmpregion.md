# `Simplify` / `GetDefUnc` audit — CmpRegion

C source: reference/starlink-ast/src/cmpregion.c
Rust: crates/ast-core/src/regions/cmpregion.rs
Audited: 2026-08-21 against pin 3cf4bc5c

Two C bodies. `Simplify` (`cmpregion.c:3584-3788`) calls the parent, simplifies
the two component Regions, and then works through the ways a boolean combination
of two Regions collapses: a null or infinite component, no overlap under AND, a
negation pair under OR, identical components, and one component contained in the
other. `GetDefUnc` (`cmpregion.c:1090-1162`) builds the default uncertainty from
the two components'.

CmpRegion has no `MapMerge` of its own; it takes the Region base implementation.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned pointer | cmpregion.c:3584-3585 | -- | n/a: the Rust returns `AstResult<DynMapping>` |
| 2 | Check the global error status and bail out | cmpregion.c:3587-3588 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Invoke the parent Region simplify | cmpregion.c:3590-3596 | cmpregion.rs:1093-1100 | ported |
| 4 | Note whether that simplified anything | cmpregion.c:3597-3601 | cmpregion.rs:1100-1115 | ported |
| 5 | Note that a new Region may be produced in the parent's base Frame rather than the current one | cmpregion.c:3602-3609 | cmpregion.rs:1138-1141 | ported |
| 6 | Get the component Regions, how they combine, and the Negated values to use with them | cmpregion.c:3610-3616 | cmpregion.rs:1116-1123 | ported |
| 7 | Replace the first component with its negation when its stored Negated value is not the required one | cmpregion.c:3617-3625 | cmpregion.rs:1116-1123 | ported |
| 8 | Do the same for the second component | cmpregion.c:3626-3633 | cmpregion.rs:1116-1123 | ported |
| 9 | Simplify each of the two components | cmpregion.c:3635-3638 | cmpregion.rs:1124-1131 | ported |
| 10 | Note whether either simplified | cmpregion.c:3639-3641 | cmpregion.rs:1132-1137 | ported |
| 11 | A null or infinite component can be excluded from the result | cmpregion.c:3642-3645 | cmpregion.rs:1142-1150 | ported |
| 12 | Take the non-null Region, which stays valid even when both are null or infinite | cmpregion.c:3646-3655 | cmpregion.rs:1142-1160 | ported |
| 13 | Handle the null case | cmpregion.c:3656-3671 | cmpregion.rs:1161-1170 | ported |
| 14 | Handle the infinite case | cmpregion.c:3672-3688 | cmpregion.rs:1171-1180 | ported |
| 15 | Flag that some simplification took place | cmpregion.c:3689-3691 | cmpregion.rs:1181 | ported |
| 16 | With neither component null or infinite, try removing one or both on overlap grounds | cmpregion.c:3692-3697 | cmpregion.rs:1182-1186 | ported |
| 17 | No overlap combined with AND makes the CmpRegion null | cmpregion.c:3698-3704 | cmpregion.rs:1187-1193 | ported |
| 18 | One component the negation of the other, combined with OR, makes it infinite | cmpregion.c:3705-3712 | cmpregion.rs:1194-1199 | ported |
| 19 | Identical components | cmpregion.c:3713-3716 | cmpregion.rs:1200-1205 | ported |
| 20 | Under AND or OR, identical components collapse to either one of them | cmpregion.c:3717-3727 | cmpregion.rs:1200-1205 | ported |
| 21 | The first contained in the second, under AND or OR, collapses to one of them | cmpregion.c:3728-3734 | cmpregion.rs:1206-1215 | ported |
| 22 | The second contained in the first likewise | cmpregion.c:3735-3741 | cmpregion.rs:1216-1225 | ported |
| 23 | Otherwise create a new CmpRegion, or leave the base-Frame pointer null so a clone is returned | cmpregion.c:3742-3754 | cmpregion.rs:1226-1241 | ported |
| 24 | Choose between the base-Frame and current-Frame results, mapping the base-Frame one into the current Frame | cmpregion.c:3755-3771 | cmpregion.rs:1242-1249 | ported |
| 25 | With nothing simplified, return a clone of the supplied pointer | cmpregion.c:3772-3776 | cmpregion.rs:1250-1256 | ported |
| 26 | Free resources | cmpregion.c:3777-3783 | -- | n/a: owned values are dropped |
| 27 | Annul the returned Mapping if an error occurred | cmpregion.c:3784-3786 | -- | n/a: an error is a `Result::Err` |
| 28 | Return the result | cmpregion.c:3787-3788 | cmpregion.rs:1257 | ported |
| 29 | `GetDefUnc`: build a default uncertainty from the two components' uncertainty Regions | cmpregion.c:1090-1162 | cmpregion.rs:927-965 | ported |

## Rows 5, 23 and 24 — two candidate results, in two Frames

C carries two pointers: `newb`, a Region in the parent's *base* Frame, and
`newc`, one in the current Frame. Which it returns depends on which was produced,
and a base-Frame result has to be mapped into the current Frame first. The port
keeps both candidates for the same reason (`cmpregion.rs:1138-1141,1242-1249`);
collapsing them to one would return a Region defined in the wrong Frame for
whichever branch produced it.

## Rows 6 to 8 — the required Negated value is not the stored one

`astGetRegions` returns the Negated value each component *should* be used with,
which need not be the value the component stores. C substitutes the negation of
the component when they differ, and the port does the same before any of the
overlap tests — otherwise every subsequent test would be asking about the wrong
region.

## Row 18 — why the negation test is restricted

C notes that "one component is the negation of the other" is only recognised in
the OR case. The port keeps that restriction rather than generalising it, which
is the negative-control side of this rule: an AND of a Region and its negation is
null, and that is reached by row 17's no-overlap test instead.

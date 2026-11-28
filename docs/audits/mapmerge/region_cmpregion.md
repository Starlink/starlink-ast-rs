# `Simplify` / `GetDefUnc` audit — CmpRegion

C source: reference/starlink-ast/src/cmpregion.c
Rust: crates/starlink-ast/src/regions/cmpregion.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 4029b011
Extension points: classified

Uncited C steps:
- cmpregion.c:3583 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step
- cmpregion.c:1147 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step

Two C bodies. `Simplify` (`cmpregion.c:3603-3808 (fn Simplify)`) calls the parent,
simplifies the two component Regions, and then works through the ways a boolean
combination of two Regions collapses: a null or infinite component, no overlap
under AND, a negation pair under OR, identical components, and one component
contained in the other. `GetDefUnc` (`cmpregion.c:1151-1182 (fn GetDefUnc)`) takes the
default uncertainty from whichever component carries an explicit one.

The Rust counterparts are `simplified` (`cmpregion.rs:1799-1967 (fn simplified)`) and
`get_def_unc` (`cmpregion.rs:1543-1555 (fn get_def_unc)`).

CmpRegion has no `MapMerge` of its own; it takes the Region base implementation.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned pointer | cmpregion.c:3603-3605 (fn Simplify) | -- | ceremony | n/a: the Rust signature returns `AstResult<(DynMapping, bool)>`, so there is no pointer to pre-set |
| 2 | Check the global error status and bail out | cmpregion.c:3606-3608 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Invoke the parent Region simplify, which simplifies the encapsulated FrameSet and uncertainty, giving a Region in the current Frame | cmpregion.c:3609-3615 (fn Simplify) | cmpregion.rs:1804-1806 (fn simplified) | architecture | ported |
| 4 | Note whether that simplified, by comparing the returned pointer with the supplied one | cmpregion.c:3616-3620 (fn Simplify) | cmpregion.rs:1807 (fn simplified) | architecture | ported |
| 5 | Initialize the base-Frame candidate to "not produced" | cmpregion.c:3621-3628 (fn Simplify) | cmpregion.rs:1844 (fn simplified) | architecture | ported |
| 6 | Get the component Regions, how they should be combined, and the Negated values they should be used with, all in the base Frame | cmpregion.c:3629-3635 (fn Simplify) | cmpregion.rs:1818 (fn simplified) | architecture | ported |
| 7 | Where the first component does not carry its required `Negated` value, use its negation instead | cmpregion.c:3636-3644 (fn Simplify) | cmpregion.rs:1822-1824 (fn simplified) | architecture | ported |
| 8 | The same for the second component | cmpregion.c:3645-3653 (fn Simplify) | cmpregion.rs:1825-1827 (fn simplified) | architecture | ported |
| 9 | Simplify each of the two components | cmpregion.c:3654-3657 (fn Simplify) | cmpregion.rs:1830-1833 (fn simplified) | architecture | ported |
| 10 | Note whether either component simplified, keeping any earlier flag | cmpregion.c:3658-3660 (fn Simplify) | cmpregion.rs:1838-1840 (fn simplified) | architecture | ported |
| 11 | A null or infinite component can be excluded from the result | cmpregion.c:3661-3664 (fn Simplify) | cmpregion.rs:1848-1850 (fn simplified) | architecture | ported |
| 12 | Separate the null component from the other one, which stays valid when both are null | cmpregion.c:3665-3674 (fn Simplify) | cmpregion.rs:1853-1857 (fn simplified) | architecture | ported |
| 13 | An un-negated null component: under AND the result is null in the other Region's Frame, under OR it is the other Region | cmpregion.c:3675-3690 (fn Simplify) | cmpregion.rs:1859-1870 (fn simplified) | architecture | ported |
| 14 | A negated (infinite) component: under AND the result is the other Region, under OR it is infinite | cmpregion.c:3691-3707 (fn Simplify) | cmpregion.rs:1871-1883 (fn simplified) | architecture | ported |
| 15 | Flag that simplification took place | cmpregion.c:3708-3710 (fn Simplify) | cmpregion.rs:1884 (fn simplified) | architecture | ported |
| 16 | With neither component null or infinite, take the overlap between them as the grounds for removing one or both | cmpregion.c:3711-3716 (fn Simplify) | cmpregion.rs:1888-1889 (fn simplified) | architecture | ported |
| 17 | No overlap, or exact negation, combined with AND: the result is null | cmpregion.c:3717-3723 (fn Simplify) | cmpregion.rs:1893-1896 (fn simplified) | architecture | ported |
| 18 | One component the negation of the other, combined with OR: the result is infinite, written as a negated null Region | cmpregion.c:3724-3731 (fn Simplify) | cmpregion.rs:1900-1903 (fn simplified) | architecture | ported |
| 19 | The two components are identical | cmpregion.c:3732-3735 (fn Simplify) | cmpregion.rs:1906 (fn simplified) | architecture | ported |
| 20 | Under AND or OR, identical components collapse to either one | cmpregion.c:3736-3746 (fn Simplify) | cmpregion.rs:1907-1908 (fn simplified) | architecture | ported |
| 21 | The first component lies inside the second: AND keeps the first, OR keeps the second | cmpregion.c:3747-3753 (fn Simplify) | cmpregion.rs:1912-1919 (fn simplified) | architecture | ported |
| 22 | The second lies inside the first: AND keeps the second, OR keeps the first | cmpregion.c:3754-3760 (fn Simplify) | cmpregion.rs:1922-1929 (fn simplified) | architecture | ported |
| 23 | Otherwise build a new CmpRegion from the simplified components, but only where something simplified; leaving the base-Frame candidate unset makes the parent-simplified copy the answer | cmpregion.c:3761-3773 (fn Simplify) | cmpregion.rs:1932-1942 (fn simplified) | architecture | ported |
| 24 | Something simplified: use the base-Frame candidate if there is one, mapping it into the current Frame with the parent FrameSet's base-to-current Mapping, otherwise use the parent-simplified copy | cmpregion.c:3774-3790 (fn Simplify) | cmpregion.rs:1949-1960 (fn simplified) | architecture | ported |
| 25 | Nothing simplified: return a clone of the supplied Region | cmpregion.c:3791-3795 (fn Simplify) | cmpregion.rs:1949-1951 (fn simplified) | architecture | ported |
| 26 | Free the component and parent-simplified pointers | cmpregion.c:3796-3802 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 27 | Annul the result if an error occurred | cmpregion.c:3803-3805 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 28 | Return the result | cmpregion.c:3806-3808 (fn Simplify) | cmpregion.rs:1962 (fn simplified) | ceremony | ported |
| 29 | Initialize the returned pointer | cmpregion.c:1151-1153 (fn GetDefUnc) | -- | ceremony | n/a: the Rust signature returns `AstResult<DynRegion>`, so there is no pointer to pre-set |
| 30 | Check the global error status and bail out | cmpregion.c:1154-1156 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 31 | Get a pointer to the CmpRegion structure | cmpregion.c:1157-1159 (fn GetDefUnc) | -- | ceremony | n/a: `&self` is already the CmpRegion |
| 32 | Where the first component has a non-default uncertainty, take it as the CmpRegion's default; an uncertainty's current Frame is the CmpRegion's base Frame | cmpregion.c:1160-1166 (fn GetDefUnc) | cmpregion.rs:1544-1545 (fn get_def_unc) | architecture | ported |
| 33 | Otherwise, where the second component has a non-default uncertainty, take that | cmpregion.c:1167-1171 (fn GetDefUnc) | cmpregion.rs:1546-1547 (fn get_def_unc) | architecture | ported |
| 34 | Otherwise defer to the parent method | cmpregion.c:1172-1176 (fn GetDefUnc) | cmpregion.rs:1548-1549 (fn get_def_unc) | architecture | ported |
| 35 | Return NULL if an error occurred | cmpregion.c:1177-1179 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 36 | Return the required pointer | cmpregion.c:1180-1182 (fn GetDefUnc) | -- | ceremony | n/a: the chosen branch is the tail expression of the `if`/`else if`/`else`, so there is no separate return |

## Rows 4 and 10 — the flag is set by comparing values, not pointers

C decides both times by pointer identity: `newc != this_mapping` at
`cmpregion.c:3619 (fn Simplify)` and `sreg1 != reg1 || sreg2 != reg2` at
`cmpregion.c:3659 (fn Simplify)`, each true whenever `astSimplify` returned a new
object rather than a clone. Owned values have no such identity, so the port
compares structurally (`cmpregion.rs:1807 (fn simplified)` and
`cmpregion.rs:1838-1840 (fn simplified)`). That is the stricter test: a
simplification that produced an equal value would set C's flag and not the
port's. Both flags then feed only the choice between a rebuilt Region and a clone
of the input, so the stricter reading cannot lose a reduction — it can only
decline to announce one that changed nothing.

## Rows 5, 23 and 24 — two candidate results, in two Frames

C carries two pointers: `newb`, a Region in the parent's *base* Frame, and
`newc`, one in the current Frame. Which it returns depends on which was produced,
and a base-Frame result has to be mapped into the current Frame first. The port
keeps both candidates for the same reason (`cmpregion.rs:1844 (fn simplified)` and
`cmpregion.rs:1953-1960 (fn simplified)`); collapsing them to one would return a Region
defined in the wrong Frame for whichever branch produced it.

## Rows 6 to 8 — the required Negated value is not the stored one

`GetRegions` (`cmpregion.c:1000 (fn GetRegions)`) returns the Negated value each
component *should* be used with, which need not be the value the component
stores: a negated CmpRegion is rewritten by De Morgan, swapping AND for OR and
toggling both component flags. C substitutes the negation of the component where
the two disagree, and the port does the same (`cmpregion.rs:1822-1827 (fn simplified)`)
before any of the overlap tests — otherwise every subsequent test would be asking
about the wrong region.

C reads the components from `newc`, the parent-simplified copy, rather than from
the supplied Region; the port reads them from `self`
(`cmpregion.rs:1818 (fn simplified)`). The two agree because the parent
`Simplify` (`region.c:11276 (fn Simplify)`) rewrites only the encapsulated FrameSet and
the uncertainty, and `newc` holds deep copies of the components either way.

## Rows 13, 14 and 20 — C's unknown-operator error has nowhere to arise

Each of these three branches ends in an `else` that raises `AST__INTER` for an
operator that is neither AND nor OR (`cmpregion.c:3684 (fn Simplify)`,
`cmpregion.c:3700 (fn Simplify)`, `cmpregion.c:3740 (fn Simplify)`), because C stores the
operator as an `int`. The port's `CmpOperator` has exactly the two variants, so
the `match` arms are exhaustive and there is no third case to report. Nothing is
dropped: the error was C's guard against a value the Rust type cannot hold.

## Row 18 — why the negation test is restricted

C notes that "one component is the negation of the other" is only recognized in
the OR case. The port keeps that restriction rather than generalising it, which
is the negative-control side of this rule: an AND of a Region and its negation is
null, and that is reached by row 17's no-overlap test instead, whose guard admits
the negation code alongside the no-overlap one
(`cmpregion.rs:1893 (fn simplified)`).

## Row 24 — the mapped result is returned as it stands

`astMapRegion` (`region.c:5495 (fn MapRegion)`) re-expresses the base-Frame candidate
in the current Frame by adding the Mapping and Frame to a copy of its FrameSet.
C returns that copy directly (`cmpregion.c:3783 (fn Simplify)`) without simplifying it
again, and the port matches (`cmpregion.rs:1954 (fn simplified)`). This is the point
where a second `astSimplify` would be easy to add and wrong: the components were
already simplified at row 9, the base-to-current Mapping was already simplified
by the parent at row 3, and re-simplifying the assembled Region would let a
class-level conversion — a Box becoming a Polygon, say — run one step further
than C runs it, changing the dump.

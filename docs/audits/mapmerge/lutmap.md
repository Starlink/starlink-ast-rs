# `MapMerge` audit — LutMap

C source: reference/starlink-ast/src/lutmap.c
Rust: crates/ast-core/src/mappings/lutmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- lutmap.c:552 (fn GetLinear) — `Local Variables:` is the local-variable declaration block, not a step
- lutmap.c:1115 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `lutmap.c:1131-1243 (fn MapMerge)`: replace a linear LutMap with an
equivalent WinMap; failing that, cancel a LutMap against an adjacent equal LutMap
applied the other way. The linearity test itself is `GetLinear`
(`lutmap.c:516-633 (fn GetLinear)`), reached only from this body, so it is audited
here too.

The last row is not part of `MapMerge` but of the rule that governs it: an
attribute setter whose value feeds the first part of the class's own `MapMerge`
must discard the simplified record.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | lutmap.c:1131-1133 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | lutmap.c:1134-1136 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a typed pointer to the nominated LutMap | lutmap.c:1137-1139 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | See if the LutMap is linear, and only then try the replacement | lutmap.c:1140-1144 (fn MapMerge) | lutmap.rs:244 (fn linear_equivalent) | architecture | ported |
| 5 | Take the input range spanned by the first and last table elements | lutmap.c:1145-1149 (fn MapMerge) | lutmap.rs:248-249 (fn linear_equivalent) | architecture | ported |
| 6 | Take the matching output range and require the two ends to differ | lutmap.c:1150-1155 (fn MapMerge) | lutmap.rs:250-251 (fn linear_equivalent),225 (fn table_is_linear) | architecture | ported |
| 7 | Build a WinMap for that linear mapping, swapping the ranges when the invert flag is set | lutmap.c:1156-1163 (fn MapMerge) | lutmap.rs:252-256 (fn linear_equivalent) | architecture | ported |
| 8 | Replace the LutMap with it and clear the associated invert flag | lutmap.c:1164-1170 (fn MapMerge) | lutmap.rs:589-590 (fn map_merge) | architecture | ported |
| 9 | Assign the result value | lutmap.c:1171-1175 (fn MapMerge) | lutmap.rs:591 (fn map_merge) | architecture | ported |
| 10 | Otherwise, in series only, look for an adjacent LutMap to cancel against | lutmap.c:1176-1179 (fn MapMerge) | lutmap.rs:598 (fn map_merge) | architecture | ported |
| 11 | Check the higher neighbour first, noting the lower index of the pair | lutmap.c:1180-1187 (fn MapMerge) | lutmap.rs:599-601 (fn map_merge) | architecture | ported |
| 12 | Failing that, check the lower neighbour the same way, and otherwise give up | lutmap.c:1188-1199 (fn MapMerge) | lutmap.rs:602-606 (fn map_merge) | architecture | ported |
| 13 | Cancel the pair only if the two LutMaps are equal with opposite invert flags | lutmap.c:1200-1210 (fn MapMerge) | lutmap.rs:618-623 (fn map_merge) | architecture | ported |
| 14 | Replace the first of the pair with a UnitMap and clear its invert flag | lutmap.c:1211-1218 (fn MapMerge) | lutmap.rs:624 (fn map_merge) | architecture | ported |
| 15 | Annul the second and shuffle the rest of the list down | lutmap.c:1219-1226 (fn MapMerge) | mappings/simplify.rs:588-590 (fn replace_pair_with) | ceremony | ported |
| 16 | Clear the vacated element at the end | lutmap.c:1227-1230 (fn MapMerge) | -- | ceremony | n/a: the entry is removed from a `Vec`, so no slot is vacated to be cleared |
| 17 | Decrement the Mapping count and return the index of the nominated element | lutmap.c:1231-1238 (fn MapMerge) | lutmap.rs:627 (fn map_merge) | architecture | ported |
| 18 | Clear the returned result if an error occurred | lutmap.c:1239-1241 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 19 | Return the result | lutmap.c:1242-1243 (fn MapMerge) | lutmap.rs:635 (fn map_merge) | architecture | ported |
| 20 | `GetLinear`: initialise the answer to "not linear" | lutmap.c:567-569 (fn GetLinear) | -- | ceremony | n/a: each path returns its own `bool` |
| 21 | Check the global error status and bail out | lutmap.c:570-572 (fn GetLinear) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 22 | Obtain a typed pointer to the LutMap structure | lutmap.c:573-575 (fn GetLinear) | -- | ceremony | n/a: the Rust method already has `&self` |
| 23 | A nearest-neighbour LutMap is never linear, because of the discontinuities at the ends of the table | lutmap.c:576-579 (fn GetLinear) | lutmap.rs:194-196 (fn table_is_linear) | architecture | ported |
| 24 | Obtain the lookup table and its length | lutmap.c:580-583 (fn GetLinear) | lutmap.rs:197 (fn table_is_linear) | architecture | ported |
| 25 | Loop to find the largest and smallest values in the table | lutmap.c:584-592 (fn GetLinear) | lutmap.rs:201-209 (fn table_is_linear) | architecture | ported |
| 26 | A table whose values are all the same is linear, inverse or no inverse | lutmap.c:593-597 (fn GetLinear) | lutmap.rs:212-214 (fn table_is_linear) | architecture | ported |
| 27 | Take the relative error of the table values from the LutEpsilon attribute | lutmap.c:598-600 (fn GetLinear) | lutmap.rs:215 (fn table_is_linear) | architecture | ported |
| 28 | Form a tolerance from the overall range of table values times that error | lutmap.c:601-604 (fn GetLinear) | lutmap.rs:216 (fn table_is_linear) | architecture | ported |
| 29 | Loop over every table element except the first and the last | lutmap.c:605-609 (fn GetLinear) | lutmap.rs:217-218 (fn table_is_linear) | architecture | ported |
| 30 | Take the fractional position of the current element within the table | lutmap.c:610-613 (fn GetLinear) | lutmap.rs:219 (fn table_is_linear) | architecture | ported |
| 31 | Interpolate linearly between the first and last values to get the value the element should have | lutmap.c:614-617 (fn GetLinear) | lutmap.rs:220 (fn table_is_linear) | architecture | ported |
| 32 | Form a second tolerance from the interpolated value, and use whichever is larger | lutmap.c:618-623 (fn GetLinear) | lutmap.rs:221 (fn table_is_linear) | architecture | ported |
| 33 | Accept linearity within twice that tolerance, and stop at the first element that fails | lutmap.c:624-630 (fn GetLinear) | lutmap.rs:222-224 (fn table_is_linear) | architecture | ported |
| 34 | Return the result | lutmap.c:631-632 (fn GetLinear) | lutmap.rs:226 (fn table_is_linear) | architecture | ported |
| 35 | Setting or clearing LutInterp or LutEpsilon clears the simplified record, both orientations | lutmap.c:1793 (fn astMAKE_CLEAR1),1796 (fn astMAKE_SET1) | lutmap.rs:109-124 (fn set_interp) | ceremony | ported |

## Rows 4 and 20 to 34 — the linearity test is `GetLinear`

`table_is_linear` (`lutmap.rs:193-227 (fn table_is_linear)`) is C's `GetLinear`:
the tolerance is scaled by `lut_epsilon`, C's `LutEpsilon` attribute defaulting to
`f64::EPSILON`, and taken as the larger of an estimate from the table's full range
and one from the interpolated value at the element under test, with linearity
accepted within twice it. The constant-table shortcut
(`lutmap.c:595 (fn GetLinear)`) is included.

`neg_lut_near_linear` and `lut_exactly_linear` are the pair that pins the
tolerance: a table departing from linear by ~3e-13 relative, which C keeps as a
LutMap, and one linear to the last bit, which C replaces with a WinMap. The
port's earlier hardcoded `1e-10` converted both.

`GetLinear` is only ever called from `MapMerge`
(`lutmap.c:1142 (fn MapMerge)`), so the port folds it into the same seam:
`linear_equivalent` (`lutmap.rs:239-257 (fn linear_equivalent)`) asks
`table_is_linear` and, when the answer is yes, returns the WinMap C would build.
The two-stage shape keeps C's ordering — the linearity test decides which half of
`MapMerge` runs, and the `b1 != b2` guard (`lutmap.c:1154 (fn MapMerge)`) sits
after it, not inside it.

## Row 23 — nearest neighbour is never linear

C's first act in `GetLinear` is to refuse a nearest-neighbour table outright
(`lutmap.c:578 (fn GetLinear)`): the transformation is a staircase, and the values
of its treads say nothing about that. The port had no such test, so a LutMap whose
table happened to be linear was replaced by a WinMap even with
`LutInterp` set to nearest — a Mapping that returns `BAD_VALUE` outside the table
turned into one that extrapolates. `table_is_linear` now answers `false` for
`LutInterp::Nearest` before it looks at the table at all, and
`lutmap_nearest_linear_does_not_simplify` is the negative control: an exactly
linear table plus nearest-neighbour interpolation, which must not become a WinMap.

## Row 17 — the higher neighbour first, but `where` is reported

C checks the upper neighbour before the lower, and in both cases replaces the
*lower* of the two entries — yet what it returns is `where`, the index of the
nominated LutMap, not `ilo` (`lutmap.c:1234 (fn MapMerge)`). When the pair was the
lower one, C therefore names an element one past the first it modified.

The returned index is what the nominate loop rewinds to — `nominated` moves back
only when the reported index is lower than it (`cmpmap.c:3823 (fn Simplify)`) —
and it is also the element a repeating pattern freezes
(`cmpmap.c:3812 (fn Simplify)`). Reporting `where` therefore declines the rewind
that reporting `ilo` would have caused, and points both readers at the entry after
the replacement rather than at the replacement itself. The port returns
`Some(where_)` for that reason; an earlier `where_.min(ilo)` was a divergence, not
a tidying, and it changed which element the next pass considered.

## Rows 4 to 9 — a lone LutMap has only the WinMap conversion

C gives LutMap no `Simplify` vtab slot, so a LutMap nominated on its own reaches
this same body. The linear-table replacement (rows 4 to 9) needs no neighbour and
therefore fires for a lone LutMap too, keyed on the list's invert flag; the
cancellation against an adjacent LutMap (rows 10 to 17) simply finds no
neighbour.

## Rows 10 to 14 — one entry point, and C's comparison by value

C's cancellation temporarily forces the neighbour's `Invert` to the value beside
it in the list, inverts it, compares with `astEqual`, and puts the flag back
(`lutmap.c:1204-1209 (fn MapMerge)`). Owned values make that a comparison rather
than a mutation: `map_merge` clones the neighbour, folds the list flag and the
inversion into the clone, and hands it to `applied_equal`
(`lutmap.rs:262-268 (fn applied_equal)`), which folds the nominated LutMap's own
applied direction in the same way before calling `equal`. `equal` compares stored
orientation, so both sides have to be brought to their applied one first.

C's `MapMerge` has no third branch. Nothing else in the library merges with a
LutMap, and an adjacent UnitMap is removed when the UnitMap is itself nominated
(`unitmap.c:519 (fn MapMerge)`), so the port returns `None` rather than falling
through to `default_map_merge` — offering the pair to a neighbour would be a
merge C does not perform. For the same reason `LutMap` implements no `try_merge`:
the only nomination that can present two adjacent LutMaps is a LutMap, which
handles the pair inline, and a second rule that could disagree with this one is
worth nothing. `lutmap_cancels_with_inverse` and its three negative controls
drive the branch through `simplify` instead of calling a helper directly.

## Row 35 — the convention, not just this class

The class prologue records that the simplified record must be discarded when
either `LutInterp` or `LutEpsilon` is set or cleared, "since both change what the
LutMap does" — `LutEpsilon` because it is the linearity tolerance, `LutInterp`
because it decides whether the table is a staircase. Both attributes get the rule
through the `astMAKE_SET1` and `astMAKE_CLEAR1` macros, whose `astClearIsSimple`
term runs before the value is stored.

`set_interp` (`lutmap.rs:109-124 (fn set_interp)`) discards both records, as
`astSetLutInterp` does. `set_lut_epsilon`
(`lutmap.rs:164-174 (fn set_lut_epsilon)`) does the same, and treats `BAD_VALUE`
as the unset sentinel so the default is C's `DBL_EPSILON` and the `LutEps` card is
omitted — the value feeds row 27 directly, which is the first thing `MapMerge`
consults.

See PLAN.md §9.2 for the whole convention and the classes it covers.

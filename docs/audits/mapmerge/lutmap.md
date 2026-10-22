# `MapMerge` audit — LutMap

C source: reference/starlink-ast/src/lutmap.c
Rust: crates/ast-core/src/mappings/lutmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- lutmap.c:556 (fn GetLinear) — `Local Variables:` is the local-variable declaration block, not a step
- lutmap.c:1119 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `lutmap.c:1135-1247 (fn MapMerge)`: replace a linear LutMap with an
equivalent WinMap; failing that, cancel a LutMap against an adjacent equal LutMap
applied the other way. The linearity test itself is `GetLinear`
(`lutmap.c:520-637 (fn GetLinear)`), reached only from this body, so it is audited
here too.

The last row is not part of `MapMerge` but of the rule that governs it: an
attribute setter whose value feeds the first part of the class's own `MapMerge`
must discard the simplified record.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | lutmap.c:1135-1137 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | lutmap.c:1138-1140 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a typed pointer to the nominated LutMap | lutmap.c:1141-1143 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | See if the LutMap is linear, and only then try the replacement | lutmap.c:1144-1148 (fn MapMerge) | lutmap.rs:376 (fn linear_equivalent) | architecture | ported |
| 5 | Take the input range spanned by the first and last table elements | lutmap.c:1149-1153 (fn MapMerge) | lutmap.rs:380-381 (fn linear_equivalent) | architecture | ported |
| 6 | Take the matching output range and require the two ends to differ | lutmap.c:1154-1159 (fn MapMerge) | lutmap.rs:382-383 (fn linear_equivalent),325 (fn table_is_linear) | architecture | ported |
| 7 | Build a WinMap for that linear mapping, swapping the ranges when the invert flag is set | lutmap.c:1160-1167 (fn MapMerge) | lutmap.rs:384-388 (fn linear_equivalent) | architecture | ported |
| 8 | Replace the LutMap with it and clear the associated invert flag | lutmap.c:1168-1174 (fn MapMerge) | lutmap.rs:738-739 (fn map_merge) | architecture | ported |
| 9 | Assign the result value | lutmap.c:1175-1179 (fn MapMerge) | lutmap.rs:740 (fn map_merge) | architecture | ported |
| 10 | Otherwise, in series only, look for an adjacent LutMap to cancel against | lutmap.c:1180-1183 (fn MapMerge) | lutmap.rs:747 (fn map_merge) | architecture | ported |
| 11 | Check the higher neighbor first, noting the lower index of the pair | lutmap.c:1184-1191 (fn MapMerge) | lutmap.rs:748-750 (fn map_merge) | architecture | ported |
| 12 | Failing that, check the lower neighbor the same way, and otherwise give up | lutmap.c:1192-1203 (fn MapMerge) | lutmap.rs:751-755 (fn map_merge) | architecture | ported |
| 13 | Cancel the pair only if the two LutMaps are equal with opposite invert flags | lutmap.c:1204-1214 (fn MapMerge) | lutmap.rs:767-772 (fn map_merge) | architecture | ported |
| 14 | Replace the first of the pair with a UnitMap and clear its invert flag | lutmap.c:1215-1222 (fn MapMerge) | lutmap.rs:773 (fn map_merge) | architecture | ported |
| 15 | Annul the second and shuffle the rest of the list down | lutmap.c:1223-1230 (fn MapMerge) | mappings/simplify.rs:660-661 (fn replace_pair_with) | ceremony | ported |
| 16 | Clear the vacated element at the end | lutmap.c:1231-1234 (fn MapMerge) | -- | ceremony | n/a: the entry is removed from a `Vec`, so no slot is vacated to be cleared |
| 17 | Decrement the Mapping count and return the index of the nominated element | lutmap.c:1235-1242 (fn MapMerge) | lutmap.rs:776 (fn map_merge) | architecture | ported |
| 18 | Clear the returned result if an error occurred | lutmap.c:1243-1245 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 19 | Return the result | lutmap.c:1246-1247 (fn MapMerge) | lutmap.rs:784 (fn map_merge) | architecture | ported |
| 20 | `GetLinear`: initialize the answer to "not linear" | lutmap.c:571-573 (fn GetLinear) | -- | ceremony | n/a: each path returns its own `bool` |
| 21 | Check the global error status and bail out | lutmap.c:574-576 (fn GetLinear) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 22 | Obtain a typed pointer to the LutMap structure | lutmap.c:577-579 (fn GetLinear) | -- | ceremony | n/a: the Rust method already has `&self` |
| 23 | A nearest-neighbor LutMap is never linear, because of the discontinuities at the ends of the table | lutmap.c:580-583 (fn GetLinear) | lutmap.rs:326-328 (fn table_is_linear) | architecture | ported |
| 24 | Obtain the lookup table and its length | lutmap.c:584-587 (fn GetLinear) | lutmap.rs:329 (fn table_is_linear) | architecture | ported |
| 25 | Loop to find the largest and smallest values in the table | lutmap.c:588-596 (fn GetLinear) | lutmap.rs:333-341 (fn table_is_linear) | architecture | ported |
| 26 | A table whose values are all the same is linear, inverse or no inverse | lutmap.c:597-601 (fn GetLinear) | lutmap.rs:344-346 (fn table_is_linear) | architecture | ported |
| 27 | Take the relative error of the table values from the LutEpsilon attribute | lutmap.c:602-604 (fn GetLinear) | lutmap.rs:347 (fn table_is_linear) | architecture | ported |
| 28 | Form a tolerance from the overall range of table values times that error | lutmap.c:605-608 (fn GetLinear) | lutmap.rs:348 (fn table_is_linear) | architecture | ported |
| 29 | Loop over every table element except the first and the last | lutmap.c:609-613 (fn GetLinear) | lutmap.rs:349-350 (fn table_is_linear) | architecture | ported |
| 30 | Take the fractional position of the current element within the table | lutmap.c:614-617 (fn GetLinear) | lutmap.rs:351 (fn table_is_linear) | architecture | ported |
| 31 | Interpolate linearly between the first and last values to get the value the element should have | lutmap.c:618-621 (fn GetLinear) | lutmap.rs:352 (fn table_is_linear) | architecture | ported |
| 32 | Form a second tolerance from the interpolated value, and use whichever is larger | lutmap.c:622-627 (fn GetLinear) | lutmap.rs:353 (fn table_is_linear) | architecture | ported |
| 33 | Accept linearity within twice that tolerance, and stop at the first element that fails | lutmap.c:628-634 (fn GetLinear) | lutmap.rs:354-356 (fn table_is_linear) | architecture | ported |
| 34 | Return the result | lutmap.c:635-636 (fn GetLinear) | lutmap.rs:358 (fn table_is_linear) | architecture | ported |
| 35 | Setting or clearing LutInterp or LutEpsilon clears the simplified record, both orientations | lutmap.c:1802 (fn astMAKE_CLEAR1),1805 (fn astMAKE_SET1) | lutmap.rs:177-192 (fn set_interp) | ceremony | ported |

## Rows 4 and 20 to 34 — the linearity test is `GetLinear`

`table_is_linear` (`lutmap.rs:325-359 (fn table_is_linear)`) is C's `GetLinear`:
the tolerance is scaled by `lut_epsilon`, C's `LutEpsilon` attribute defaulting to
`f64::EPSILON`, and taken as the larger of an estimate from the table's full range
and one from the interpolated value at the element under test, with linearity
accepted within twice it. The constant-table shortcut
(`lutmap.c:599 (fn GetLinear)`) is included.

`neg_lut_near_linear` and `lut_exactly_linear` are the pair that pins the
tolerance: a table departing from linear by ~3e-13 relative, which C keeps as a
LutMap, and one linear to the last bit, which C replaces with a WinMap. The
port's earlier hardcoded `1e-10` converted both.

`GetLinear` is only ever called from `MapMerge`
(`lutmap.c:1146 (fn MapMerge)`), so the port folds it into the same seam:
`linear_equivalent` (`lutmap.rs:371-389 (fn linear_equivalent)`) asks
`table_is_linear` and, when the answer is yes, returns the WinMap C would build.
The two-stage shape keeps C's ordering — the linearity test decides which half of
`MapMerge` runs, and the `b1 != b2` guard (`lutmap.c:1158 (fn MapMerge)`) sits
after it, not inside it.

## Row 23 — nearest neighbor is never linear

C's first act in `GetLinear` is to refuse a nearest-neighbor table outright
(`lutmap.c:582 (fn GetLinear)`): the transformation is a staircase, and the values
of its treads say nothing about that. The port had no such test, so a LutMap whose
table happened to be linear was replaced by a WinMap even with
`LutInterp` set to nearest — a Mapping that returns `BAD_VALUE` outside the table
turned into one that extrapolates. `table_is_linear` now answers `false` for
`LutInterp::Nearest` before it looks at the table at all, and
`lutmap_nearest_linear_does_not_simplify` is the negative control: an exactly
linear table plus nearest-neighbor interpolation, which must not become a WinMap.

## Row 17 — the higher neighbor first, but `where` is reported

C checks the upper neighbor before the lower, and in both cases replaces the
*lower* of the two entries — yet what it returns is `where`, the index of the
nominated LutMap, not `ilo` (`lutmap.c:1238 (fn MapMerge)`). When the pair was the
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
this same body. The linear-table replacement (rows 4 to 9) needs no neighbor and
therefore fires for a lone LutMap too, keyed on the list's invert flag; the
cancellation against an adjacent LutMap (rows 10 to 17) simply finds no
neighbor.

## Rows 10 to 14 — one entry point, and C's comparison by value

C's cancellation temporarily forces the neighbor's `Invert` to the value beside
it in the list, inverts it, compares with `astEqual`, and puts the flag back
(`lutmap.c:1208-1213 (fn MapMerge)`). Owned values make that a comparison rather
than a mutation: `map_merge` clones the neighbor, folds the list flag and the
inversion into the clone, and hands it to `applied_equal`
(`lutmap.rs:394-400 (fn applied_equal)`), which folds the nominated LutMap's own
applied direction in the same way before calling `equal`. `equal` compares stored
orientation, so both sides have to be brought to their applied one first.

C's `MapMerge` has no third branch. Nothing else in the library merges with a
LutMap, and an adjacent UnitMap is removed when the UnitMap is itself nominated
(`unitmap.c:519 (fn MapMerge)`), so the port returns `None` rather than falling
through to `default_map_merge` — offering the pair to a neighbor would be a
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

`set_interp` (`lutmap.rs:177-192 (fn set_interp)`) discards both records, as
`astSetLutInterp` does. `set_lut_epsilon`
(`lutmap.rs:266-284 (fn set_lut_epsilon)`) does the same, and treats `BAD_VALUE`
as the unset sentinel so the default is C's `DBL_EPSILON` and the `LutEps` card is
omitted — the value feeds row 27 directly, which is the first thing `MapMerge`
consults.

See PLAN.md §9.2 for the whole convention and the classes it covers.

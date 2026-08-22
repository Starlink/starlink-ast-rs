# `MapMerge` audit — LutMap

C source: reference/starlink-ast/src/lutmap.c
Rust: crates/ast-core/src/mappings/lutmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `lutmap.c:1131-1243`: replace a linear LutMap with an equivalent
WinMap; failing that, cancel a LutMap against an adjacent equal LutMap applied
the other way. The linearity test itself is `GetLinear` (`lutmap.c:516-633`), so
it is audited here too.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | lutmap.c:1131-1132 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | lutmap.c:1134-1135 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Obtain a typed pointer to the nominated LutMap | lutmap.c:1137-1139 | -- | n/a: the Rust method already has `&self` |
| 4 | See if the LutMap is linear | lutmap.c:1140-1144 | lutmap.rs:116-160 | ported |
| 5 | Take the input range spanned by the first and last table elements | lutmap.c:1145-1149 | lutmap.rs:129-130 | ported |
| 6 | Take the matching output range and require the two ends to differ | lutmap.c:1150-1155 | lutmap.rs:125,131-132 | ported |
| 7 | Build a WinMap for that linear mapping, swapping the ranges when the invert flag is set | lutmap.c:1156-1163 | lutmap.rs:133-138 | ported |
| 8 | Replace the LutMap with it and clear the associated invert flag | lutmap.c:1164-1170 | lutmap.rs:618-620 | ported |
| 9 | Assign the result value | lutmap.c:1171-1175 | lutmap.rs:620 | ported |
| 10 | Otherwise, in series, look for an adjacent LutMap to cancel against | lutmap.c:1176-1179 | lutmap.rs:623-627 | ported |
| 11 | Check the higher neighbour first, noting the lower index of the pair | lutmap.c:1180-1187 | lutmap.rs:627-650 | ported |
| 12 | Failing that, check the lower neighbour the same way | lutmap.c:1188-1199 | lutmap.rs:627-650 | ported |
| 13 | Cancel the pair only if the two LutMaps are equal with opposite invert flags | lutmap.c:1200-1210 | lutmap.rs:140-160 | ported |
| 14 | Replace the first of the pair with a UnitMap and set its invert flag | lutmap.c:1211-1218 | lutmap.rs:627-650 | ported |
| 15 | Annul the second and shuffle the rest down, clearing the vacated slot | lutmap.c:1219-1230 | lutmap.rs:627-650 | ported |
| 16 | Decrement the Mapping count and return the lower of the two indices | lutmap.c:1231-1238 | lutmap.rs:654 | ported |
| 17 | Clear the returned result if an error occurred | lutmap.c:1239-1241 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 18 | Return the result | lutmap.c:1242-1243 | lutmap.rs:659 | ported |
| 19 | `GetLinear`: take the relative error of the table values from the LutEpsilon attribute | lutmap.c:598-600 | lutmap.rs:116-160 | ported |
| 20 | Form a tolerance from the overall range of table values times that error | lutmap.c:601-604 | lutmap.rs:140-146 | ported |
| 21 | For each interior element, interpolate linearly between the first and last values by fractional position | lutmap.c:606-616 | lutmap.rs:124 | ported |
| 22 | Form a second tolerance from the interpolated value, and use whichever is larger | lutmap.c:617-622 | lutmap.rs:116-160 | ported |
| 23 | Accept linearity within twice that tolerance | lutmap.c:623-627 | lutmap.rs:147-155 | ported |
| 24 | Setting or clearing LutInterp or LutEpsilon clears the simplified record | lutmap.c:1842, and :120 | -- | gap: PLAN.md §9.2 |

## Rows 4 and 19 to 23 — the linearity test now matches C's

`table_is_linear` (`lutmap.rs:116-160`) is C's `GetLinear`: the tolerance is
scaled by `lut_epsilon`, C's `LutEpsilon` attribute defaulting to
`f64::EPSILON`, and taken as the larger of an estimate from the table's full
range and one from the interpolated value at the element under test, with
linearity accepted within twice it. The constant-table shortcut at
`lutmap.c:595` is included.

`neg_lut_near_linear` and `lut_exactly_linear` are the pair that pins it: a table
departing from linear by ~3e-13 relative, which C keeps as a LutMap, and one
linear to the last bit, which C replaces with a WinMap. The port's earlier
hardcoded `1e-10` converted both.

## Row 24 — two attributes, both governed

`lutmap.c:120` records that the simplified record must be discarded when either
`LutInterp` or `LutEpsilon` is set or cleared, "since both change what MapMerge
does" — `LutEpsilon` because it is the linearity tolerance, `LutInterp` because
the table's meaning changes. The port has `set_interp` (`lutmap.rs:87`) and no
`LutEpsilon` at all, and neither clears the record. This is the one row in the
audit where the convention is a live gap rather than a latent one, because the
setter exists. See PLAN.md §9.2.

## Row 11 — the higher neighbour first, but the lower index reported

C checks the upper neighbour before the lower, yet in both cases replaces the
*lower* of the two entries and reports its index. `lutmap.rs:654` returns
`where_.min(ilo)` for that reason.

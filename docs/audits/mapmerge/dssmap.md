# `MapMerge` audit — DssMap

C source: reference/starlink-ast/src/dssmap.c
Rust: crates/ast-core/src/mappings/dssmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- dssmap.c:978 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `dssmap.c:997-1145 (fn MapMerge)` and does exactly one thing: absorb a WinMap
that maps pixel coordinates immediately before an un-inverted DssMap, by folding
the WinMap's scale and shift into the DssMap's pixel origin and pixel size, and
rebuilding the DssMap from the amended FITS cards.

`DssMap::map_merge` (`dssmap.rs:548-560 (fn map_merge)`) calls `default_map_merge`
(`mappings/simplify.rs:1425-1518 (fn default_map_merge)`), whose neighbour attempt reaches
`DssMap::try_merge` (`dssmap.rs:598-678 (fn try_merge)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | dssmap.c:997-998 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | dssmap.c:1000-1001 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | The only simplification available: a WinMap adjacent to a DssMap, in series | dssmap.c:1003-1017 (fn MapMerge) | dssmap.rs:605-619 (fn try_merge) | architecture | ported |
| 4 | Temporarily set the WinMap's Invert attribute to the supplied value | dssmap.c:1018-1022 (fn MapMerge) | dssmap.rs:622 (fn try_merge) | ceremony | ported |
| 5 | Take a copy of the WinMap's scale and shift terms | dssmap.c:1023-1025 (fn MapMerge) | dssmap.rs:409-428 (fn get_winmap_terms) | architecture | ported |
| 6 | Check the scale and shift terms are usable | dssmap.c:1026-1030 (fn MapMerge) | dssmap.rs:626-634 (fn try_merge) | architecture | ported |
| 7 | Read the keywords defining the DssMap's pixel origin and scales | dssmap.c:1031-1040 (fn MapMerge) | dssmap.rs:636-639 (fn try_merge) | architecture | ported |
| 8 | Compute new values that account for the WinMap | dssmap.c:1041-1054 (fn MapMerge) | dssmap.rs:643-655 (fn try_merge) | architecture | ported |
| 9 | CNPIX1 and CNPIX2 are integer keywords, so accept only values integral to one hundredth of a pixel | dssmap.c:1055-1060 (fn MapMerge) | dssmap.rs:658-662 (fn try_merge) | architecture | ported |
| 10 | Take a copy of the FitsChan holding the DssMap's defining cards | dssmap.c:1061-1066 (fn MapMerge) | -- | ceremony | n/a: the port stores the plate parameters as fields, so it amends those directly instead of round-tripping a FitsChan; the round trip's one numeric effect, the plate centre passing through its sexagesimal cards, is reproduced by `plate_centre_through_fits` (`tests/test_dss_merge_oracle.rs`) |
| 11 | Update each changed keyword in that copy | dssmap.c:1067-1097 (fn MapMerge) | dssmap.rs:666-672 (fn try_merge) | architecture | ported |
| 12 | If every keyword updated, build the new DssMap from the amended cards | dssmap.c:1098-1102 (fn MapMerge) | dssmap.rs:666-674 (fn try_merge) | architecture | ported |
| 13 | Replace the DssMap in the list with the new one, leaving the invert flag alone | dssmap.c:1103-1107 (fn MapMerge) | mappings/simplify.rs:1510-1515 (fn default_map_merge) | architecture | ported |
| 14 | Annul the WinMap and shuffle the remaining Mappings down, clearing the vacated slot | dssmap.c:1108-1119 (fn MapMerge) | mappings/simplify.rs:1515 (fn default_map_merge) | architecture | ported |
| 15 | Decrement the Mapping count and return the index of the first modified element | dssmap.c:1120-1126 (fn MapMerge) | mappings/simplify.rs:1516-1517 (fn default_map_merge) | architecture | ported |
| 16 | Annul the amended FitsChan | dssmap.c:1127-1131 (fn MapMerge) | -- | ceremony | n/a: no FitsChan is created |
| 17 | Free the scale and shift arrays | dssmap.c:1132-1135 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 18 | Re-instate the WinMap's original Invert attribute | dssmap.c:1136-1143 (fn MapMerge) | -- | ceremony | n/a: the WinMap entry is read without being mutated |
| 19 | Return the result | dssmap.c:1144-1145 (fn MapMerge) | dssmap.rs:559 (fn map_merge) | ceremony | ported |

## Row 9 — the tolerance is C's

One hundredth of a pixel is C's own figure, stated in the comment at
`dssmap.c:1055-1057 (fn MapMerge)` and applied at `dssmap.c:1058-1059 (fn MapMerge)`.
`dssmap.rs:658-662 (fn try_merge)` uses the same 0.01, so this is not a port heuristic.

## Rows 3 and 8 — which side the WinMap is on

C requires the WinMap to precede the DssMap and the DssMap to be un-inverted,
because the fold is defined on the pixel side of the plate solution. The port
reaches the same two arrangements from one branch: an un-inverted DssMap folds a
WinMap applied to its inputs, and an inverted one folds a WinMap applied to its
outputs, which is the same geometry read from the other end. That is the `self_inv`
test at `dssmap.rs:643-655 (fn try_merge)`, and the two arms are the forward and
reciprocal forms of one relation.

## Rows 5 and 4 — the direction is an argument, not a flag

C sets the WinMap's Invert attribute, reads its terms, and puts the flag back.
`DssMap::get_winmap_terms` (`dssmap.rs:408-429 (fn get_winmap_terms)`) takes the
applied direction as a value and returns the terms already resolved for it, so
nothing is mutated and nothing is restored.

## There is no self-reduction step, in C or here

C's DssMap `MapMerge` is the neighbour absorption and nothing else: it has no
replace-with-simpler step, and DssMap does not override the `Simplify` vtab slot,
so a nominated DssMap reduces only against a neighbour.
`DssMap::map_merge` matches that, going straight to the pairwise merge. Row 19's
Rust site is that call.

A DssMap reduction added later belongs at the top of `map_merge`, which is where
C would put one, and where the base `Simplify` driver
(`mappings/simplify.rs:1300-1341 (fn base_simplify)`) reaches it.

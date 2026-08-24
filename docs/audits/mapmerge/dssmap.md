# `MapMerge` audit — DssMap

C source: reference/starlink-ast/src/dssmap.c
Rust: crates/ast-core/src/mappings/dssmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `dssmap.c:997-1145` and does exactly one thing: absorb a WinMap
that maps pixel coordinates immediately before an un-inverted DssMap, by folding
the WinMap's scale and shift into the DssMap's pixel origin and pixel size, and
rebuilding the DssMap from the amended FITS cards.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | dssmap.c:997-998 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | dssmap.c:1000-1001 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | The only simplification available: a WinMap preceding an un-inverted DssMap, in series | dssmap.c:1003-1017 | dssmap.rs:568-587 | ported |
| 4 | Temporarily set the WinMap's Invert attribute to the supplied value | dssmap.c:1018-1022 | dssmap.rs:568-587 | ported |
| 5 | Take a copy of the WinMap's scale and shift terms | dssmap.c:1023-1025 | dssmap.rs:588-603 | ported |
| 6 | Check the scale and shift terms are usable | dssmap.c:1026-1030 | dssmap.rs:588-603 | ported |
| 7 | Read the keywords defining the DssMap's pixel origin and scales | dssmap.c:1031-1040 | dssmap.rs:606-609 | ported |
| 8 | Compute new values that account for the WinMap | dssmap.c:1041-1054 | dssmap.rs:615-625 | ported |
| 9 | CNPIX1 and CNPIX2 are integer keywords, so accept only values integral to one hundredth of a pixel | dssmap.c:1055-1060 | dssmap.rs:627-632 | ported |
| 10 | Take a copy of the FitsChan holding the DssMap's defining cards | dssmap.c:1061-1066 | -- | n/a: the port stores the plate parameters as fields, so it amends those directly instead of round-tripping a FitsChan |
| 11 | Update each changed keyword in that copy | dssmap.c:1067-1097 | dssmap.rs:634-638 | ported |
| 12 | If every keyword updated, build the new DssMap from the amended cards | dssmap.c:1098-1102 | dssmap.rs:634-642 | ported |
| 13 | Replace the DssMap in the list with the new one, leaving the invert flag alone | dssmap.c:1103-1107 | simplify.rs:1155-1250 | ported |
| 14 | Annul the WinMap and shuffle the remaining Mappings down, clearing the vacated slot | dssmap.c:1108-1119 | simplify.rs:1155-1250 | ported |
| 15 | Decrement the Mapping count and return the index of the first modified element | dssmap.c:1120-1126 | simplify.rs:1155-1250 | ported |
| 16 | Annul the amended FitsChan | dssmap.c:1127-1131 | -- | n/a: no FitsChan is created |
| 17 | Free the scale and shift arrays | dssmap.c:1132-1135 | -- | n/a: owned values are dropped |
| 18 | Re-instate the WinMap's original Invert attribute | dssmap.c:1136-1143 | -- | n/a: the WinMap entry is read without being mutated |
| 19 | Return the result | dssmap.c:1144-1145 | dssmap.rs:530 | ported |

## Row 9 — the tolerance is C's

One hundredth of a pixel is C's own figure, stated in the comment at
`dssmap.c:1055-1057` and applied at `:1058-1059`. `dssmap.rs:627-632` uses the
same 0.01, so this is not a port heuristic.

## Row 3 — the direction matters

C requires the WinMap to precede the DssMap and the DssMap to be un-inverted,
because the fold is defined on the pixel side of the plate solution. The port
checks the same two conditions before touching anything.

## There is no self-reduction step, in C or here

C's DssMap `MapMerge` is the neighbour absorption and nothing else: it has no
replace-with-simpler step, and DssMap does not override the `Simplify` vtab slot,
so a nominated DssMap reduces only against a neighbour.
`DssMap::map_merge` (`dssmap.rs:519-531`) matches that, going straight to the
pairwise merge. Row 19's Rust site is that call.

A DssMap reduction added later belongs at the top of `map_merge`, which is where
C would put one, and where the base `Simplify` driver
(`simplify.rs::base_simplify`) reaches it.

# `MapMerge` audit — WcsMap

C source: reference/starlink-ast/src/wcsmap.c
Rust: crates/ast-core/src/mappings/wcsmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `wcsmap.c:3078-3347`: replace an `AST__WCSBAD` WcsMap with a
UnitMap; cancel a WcsMap against its own inverse; swap it past a PermMap toward a
distant merge partner; and swap speculatively. The swap predicate is `CanSwap`
(`wcsmap.c:930-1152`), which admits only a PermMap.

`WcsMap::map_merge` (`wcsmap.rs:909-916`) delegates to
`simplify::wcs_map_merge` (`simplify.rs:1185-1237`); the swap body is
`try_swap_wcsmap_past_permmap_at` (`simplify.rs:1857-2060`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | wcsmap.c:3078-3079 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | wcsmap.c:3081-3082 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get the WcsMap's axis count | wcsmap.c:3089-3091 | simplify.rs:1196 | ported |
| 4 | See if the WcsMap can be replaced by something simpler, without reference to its neighbours | wcsmap.c:3092-3094 | simplify.rs:1191-1194 | ported |
| 5 | A WcsMap of type `AST__WCSBAD` is equivalent to a UnitMap | wcsmap.c:3095-3118 | simplify.rs:1195-1198 | ported |
| 6 | Otherwise a WcsMap can only combine with its own inverse; note that no neighbour is found yet | wcsmap.c:3119-3123 | simplify.rs:1200-1208 | ported |
| 7 | Check the lower neighbour first | wcsmap.c:3124-3131 | simplify.rs:1205-1209 | ported |
| 8 | Failing that, check the upper neighbour | wcsmap.c:3132-3140 | simplify.rs:1205-1209 | ported |
| 9 | Either match is this WcsMap's inverse, so the pair becomes a UnitMap | wcsmap.c:3141-3149 | wcsmap.rs:918-944 | ported |
| 10 | Store the UnitMap in place of the first WcsMap | wcsmap.c:3150-3154 | simplify.rs:1219 | ported |
| 11 | Shuffle the rest down, clear the vacated slot, and return the first modified index | wcsmap.c:3155-3170 | simplify.rs:1219-1220 | ported |
| 12 | If no direct merge, consider swapping the WcsMap toward a Mapping it could merge with | wcsmap.c:3171-3180 | simplify.rs:1224-1233 | ported |
| 13 | Set a flag for whether swapping with the higher neighbour would simplify either Mapping | wcsmap.c:3181-3195 | simplify.rs:1873-1911 | ported |
| 14 | Walk forward counting steps to the nearest mergeable target | wcsmap.c:3196-3225 | simplify.rs:1873-1887 | ported |
| 15 | Do the same walking backward | wcsmap.c:3226-3258 | simplify.rs:1873-1887 | ported |
| 16 | Choose the neighbour so the WcsMap moves toward the nearer target | wcsmap.c:3259-3273 | simplify.rs:1225-1233 | ported |
| 17 | With a target present, swap the pair and record the first modified index | wcsmap.c:3274-3282 | simplify.rs:1985-1987, 2034-2036 | ported |
| 18 | With no target, swap speculatively, preferring the lower neighbour when both are suitable | wcsmap.c:3283-3299 | simplify.rs:1225-1233 | ported |
| 19 | Take copies of the Mapping and Invert arrays so the supplied values are not disturbed | wcsmap.c:3300-3306 | -- | n/a: `try_swap_wcsmap_past_permmap_at` mutates the list in place and reports whether it changed it |
| 20 | If neither swapped Mapping simplifies further, discard the swap | wcsmap.c:3307-3320 | simplify.rs:1873-1911 | ported |
| 21 | Otherwise store the swapped pair and report the first index | wcsmap.c:3321-3345 | simplify.rs:1227,1236 | ported |
| 22 | Return the result | wcsmap.c:3346-3347 | simplify.rs:1236 | ported |
| 23 | `CanSwap`: a WcsMap can swap only with a PermMap | wcsmap.c:1055-1060 | simplify.rs:1862-1869 | ported |
| 24 | Require the longitude and latitude axes to have bi-directional links in the PermMap, or both be unassigned | wcsmap.c:1061-1091 | simplify.rs:1919-1959 | ported |
| 25 | Handle the WcsMap-applied-first and WcsMap-applied-second cases separately | wcsmap.c:1092-1111 | simplify.rs:1967-2056 | ported |
| 26 | The swap is worthwhile only when it reduces the shape: a preceding PermMap must have more outputs than inputs, and the mirror case likewise | wcsmap.c:1112-1121 | simplify.rs:1873-1911 | ported |
| 27 | Re-instate the original Invert settings | wcsmap.c:1130-1134 | -- | n/a: the entries are read without being mutated |
| 28 | Produce the equivalent swapped Mappings and return the swapped WcsMap | wcsmap.c:1135-1151 | simplify.rs:1998-2009, 2047-2057 | ported |

## Row 23 — PermMap-only is C's rule, not a narrowing

`CanSwap` tests the non-WcsMap member for being a PermMap and nothing else, so
the port swapping only past a PermMap is faithful rather than a reduced version
of the generic swap machinery MatrixMap and WinMap use.

## Rows 7, 8 and 17 — both pairs, both directions

C checks the lower pair before the upper for the direct merge, and the port keeps
that order (`simplify.rs:1205-1209`). For the swap the port tries
`(where_, where_+1)` and then `(where_-1, where_)` (`simplify.rs:1225-1233`),
covering the same two pairs; C reaches them through its step-counting choice
instead, so the pair attempted first can differ when both are eligible.

## Row 5 — the reduction is the head of the merge

The `AST__WCSBAD` replacement is C's opening move, before any neighbour is
considered, and `simplify.rs:1195-1198` puts it in the same position.
`wcsmap.c` has no `Simplify` vtab slot, so the base `Simplify` reaches this
`MapMerge` by nominating the WcsMap in a list of one
(`mapping.c:16871-16883`). C makes the replacement the `if` of an if/else whose
`else` holds the whole neighbour search (`wcsmap.c:3097,3113`), which is what the
early return encodes: a reduced WcsMap never looks at a neighbour in the same
pass. The replacement carries a cleared Invert flag (`wcsmap.c:3104`), so both
flags are stored false.

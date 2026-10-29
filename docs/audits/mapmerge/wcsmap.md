# `MapMerge` audit — WcsMap

C source: reference/starlink-ast/src/wcsmap.c
Rust: crates/starlink-ast/src/mappings/wcsmap.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3d3f255f
Extension points: classified

Uncited C steps:
- wcsmap.c:3100 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- wcsmap.c:1008 (fn CanSwap) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `wcsmap.c:3123-3392 (fn MapMerge)` in four parts: replace an
`AST__WCSBAD` WcsMap with a UnitMap; cancel a WcsMap against its own inverse;
swap it past a PermMap towards a distant WcsMap it could cancel against; and,
failing that, swap speculatively because the swapped pair may itself be simpler.
The swap predicate is `CanSwap` (`wcsmap.c:949-1172 (fn CanSwap)`), enumerated as
rows 37 to 58, and it admits only a PermMap.

`WcsMap::map_merge` (`wcsmap.rs:1240-1247 (fn map_merge)`) delegates to
`simplify::wcs_map_merge` (`mappings/simplify.rs:1207-1377 (fn wcs_map_merge)`), so Rust
sites below are in `mappings/simplify.rs` unless stated. The driver calls three
helpers, one per C function it needs: `can_swap_wcsmap_past_permmap`
(`mappings/simplify.rs:2105-2174 (fn can_swap_wcsmap_past_permmap)`) is `CanSwap`,
`can_merge_wcsmaps` (`mappings/simplify.rs:2065-2080 (fn can_merge_wcsmaps)`) is
`CanMerge` (`wcsmap.c:839-947 (fn CanMerge)`) applied to a list entry, and
`wcs_perm_at` (`mappings/simplify.rs:2181-2232 (fn wcs_perm_at)`) is `WcsPerm`
(`wcsmap.c:4516-4751 (fn WcsPerm)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | wcsmap.c:3123-3124 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | wcsmap.c:3126-3127 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Initialize the pair indices to avoid "used of uninitialized variable" messages from dumb compilers | wcsmap.c:3129-3132 (fn MapMerge) | -- | ceremony | n/a: the Rust indices are computed where they are used |
| 4 | Get the number of axes for the WcsMap | wcsmap.c:3134-3136 (fn MapMerge) | mappings/simplify.rs:1220 (fn wcs_map_merge) | architecture | ported |
| 5 | See whether the WcsMap can be replaced by a simpler Mapping, without reference to the neighboring Mappings | wcsmap.c:3137-3139 (fn MapMerge) | mappings/simplify.rs:1219-1222 (fn wcs_map_merge) | architecture | ported |
| 6 | A WcsMap with map type `AST__WCSBAD` is equivalent to a UnitMap | wcsmap.c:3140-3143 (fn MapMerge) | mappings/simplify.rs:1219 (fn wcs_map_merge) | architecture | ported |
| 7 | Annul the WcsMap pointer and replace it with a UnitMap used in the forward sense | wcsmap.c:3144-3150 (fn MapMerge) | mappings/simplify.rs:1220 (fn wcs_map_merge) | architecture | ported |
| 8 | Return the index of the first modified element | wcsmap.c:3151-3153 (fn MapMerge) | mappings/simplify.rs:1221 (fn wcs_map_merge) | architecture | ported |
| 9 | Otherwise try the neighbors, which for a WcsMap can only be done in series | wcsmap.c:3154-3158 (fn MapMerge) | mappings/simplify.rs:1224 (fn wcs_map_merge) | architecture | ported |
| 10 | Store the classes of the neighboring Mappings | wcsmap.c:3160-3163 (fn MapMerge) | mappings/simplify.rs:1237-1239 (fn wcs_map_merge) | architecture | ported |
| 11 | A WcsMap can only combine with its own inverse; note that no mergeable neighbor has been found yet | wcsmap.c:3164-3168 (fn MapMerge) | mappings/simplify.rs:1229-1233 (fn wcs_map_merge) | architecture | ported |
| 12 | Check the lower neighbor first | wcsmap.c:3169-3176 (fn MapMerge) | mappings/simplify.rs:1230 (fn wcs_map_merge),1231-1238 (fn wcs_map_merge) | architecture | ported |
| 13 | If the lower neighbor did not merge, check the upper neighbor the same way | wcsmap.c:3177-3185 (fn MapMerge) | mappings/simplify.rs:1231 (fn wcs_map_merge),1230 (fn wcs_map_merge) | architecture | ported |
| 14 | A neighbor that passed the checks is this WcsMap's inverse, so the pair can be replaced by a single UnitMap | wcsmap.c:3186-3190 (fn MapMerge) | wcsmap.rs:1249-1275 (fn try_merge) | architecture | ported |
| 15 | Annul the two WcsMaps | wcsmap.c:3191-3194 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 16 | Create the UnitMap and store it in place of the first WcsMap, with a cleared Invert flag | wcsmap.c:3195-3199 (fn MapMerge) | wcsmap.rs:1271 (fn try_merge) | architecture | ported |
| 17 | Shuffle the remaining Mappings down to fill the hole left by the second WcsMap | wcsmap.c:3200-3206 (fn MapMerge) | mappings/simplify.rs:1243 (fn wcs_map_merge) | architecture | ported |
| 18 | Clear the vacated element at the end | wcsmap.c:3207-3210 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so removing the second entry leaves no slot to clear |
| 19 | Decrement the Mapping count and return the index of the first modified element | wcsmap.c:3211-3215 (fn MapMerge) | mappings/simplify.rs:1243-1244 (fn wcs_map_merge) | architecture | ported |
| 20 | If neither neighbor merged directly, consider whether swapping the WcsMap with a neighbor would bring it next to a Mapping it can merge with | wcsmap.c:3216-3225 (fn MapMerge) | mappings/simplify.rs:1248-1251 (fn wcs_map_merge) | architecture | ported |
| 21 | Set a flag if the WcsMap could swap with its higher neighbor, and a second flag if the swap would simplify either Mapping | wcsmap.c:3226-3240 (fn MapMerge) | mappings/simplify.rs:1253-1265 (fn wcs_map_merge) | architecture | ported |
| 22 | Step through the Mappings which follow the WcsMap, looking for one it could merge with directly | wcsmap.c:3241-3250 (fn MapMerge) | mappings/simplify.rs:1272-1274 (fn wcs_map_merge) | architecture | ported |
| 23 | On finding one, note the number of steps between the two Mappings and leave the loop | wcsmap.c:3251-3258 (fn MapMerge) | mappings/simplify.rs:1275-1278 (fn wcs_map_merge) | architecture | ported |
| 24 | Leave the loop with no step count when a Mapping is reached that the WcsMap could not swap past, since WcsMaps swap only with some PermMaps | wcsmap.c:3259-3270 (fn MapMerge) | mappings/simplify.rs:1279-1281 (fn wcs_map_merge) | architecture | ported |
| 25 | Do the same working from the WcsMap towards the start of the list | wcsmap.c:3271-3303 (fn MapMerge) | mappings/simplify.rs:1286-1310 (fn wcs_map_merge) | architecture | ported |
| 26 | Choose which neighbor to swap with so the WcsMap moves towards the nearest Mapping it can merge with | wcsmap.c:3304-3318 (fn MapMerge) | mappings/simplify.rs:1317-1323 (fn wcs_map_merge) | architecture | ported |
| 27 | With a target Mapping in the list, replace the pair with the swapped Mappings | wcsmap.c:3319-3324 (fn MapMerge) | mappings/simplify.rs:1326-1327 (fn wcs_map_merge) | architecture | ported |
| 28 | Store the index of the first modified Mapping | wcsmap.c:3325-3327 (fn MapMerge) | mappings/simplify.rs:1327 (fn wcs_map_merge) | architecture | ported |
| 29 | With no Mapping available for merging, a swap may still be advantageous because the swapped Mappings may be simpler | wcsmap.c:3328-3332 (fn MapMerge) | mappings/simplify.rs:1340 (fn wcs_map_merge) | architecture | ported |
| 30 | Choose a neighbor to swap with, the lower one when both are suitable | wcsmap.c:3333-3344 (fn MapMerge) | mappings/simplify.rs:1341 (fn wcs_map_merge) | architecture | ported |
| 31 | Take copies of the Mapping and Invert flag arrays so the supplied values are not changed | wcsmap.c:3345-3351 (fn MapMerge) | mappings/simplify.rs:1341-1348 (fn wcs_map_merge) | architecture | ported |
| 32 | Swap the copied Mappings | wcsmap.c:3352-3354 (fn MapMerge) | mappings/simplify.rs:1350 (fn wcs_map_merge) | architecture | ported |
| 33 | If neither swapped Mapping simplifies further, there is no point in swapping, so annul the copies | wcsmap.c:3355-3365 (fn MapMerge) | mappings/simplify.rs:1354-1366 (fn wcs_map_merge) | architecture | ported |
| 34 | If one or both could be simplified, return the swapped Mappings and store the index of the first modified Mapping | wcsmap.c:3366-3382 (fn MapMerge) | mappings/simplify.rs:1368-1371 (fn wcs_map_merge) | architecture | ported |
| 35 | Annul the simplified Mappings | wcsmap.c:3383-3390 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 36 | Return the result | wcsmap.c:3391-3392 (fn MapMerge) | mappings/simplify.rs:1376 (fn wcs_map_merge) | ceremony | ported |
| 37 | `CanSwap` initializes the returned flag, the simpler flag and the swapped WcsMap | wcsmap.c:1027-1031 (fn CanSwap) | -- | ceremony | n/a: the helper returns `Option<(bool, Option<WcsMap>)>`, so "cannot swap" is `None` rather than a cleared flag |
| 38 | Check the global error status and bail out | wcsmap.c:1032-1034 (fn CanSwap) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 39 | Temporarily set both Mappings' Invert attributes to the supplied values | wcsmap.c:1035-1042 (fn CanSwap) | mappings/simplify.rs:2121 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 40 | Get the classes of the two Mappings | wcsmap.c:1043-1047 (fn CanSwap) | mappings/simplify.rs:2109-2110 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 41 | Get a pointer to the non-WcsMap Mapping, noting which of the pair the WcsMap is | wcsmap.c:1048-1060 (fn CanSwap) | mappings/simplify.rs:2114-2119 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 42 | The Mappings can be swapped only if the other one is a PermMap, and then only on two conditions | wcsmap.c:1061-1068 (fn CanSwap) | mappings/simplify.rs:2109-2113 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 43 | Get the PermMap's input and output coordinate counts | wcsmap.c:1069-1072 (fn CanSwap) | mappings/simplify.rs:2122-2123 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 44 | Get the PermMap's axis permutation arrays and constants array | wcsmap.c:1073-1077 (fn CanSwap) | mappings/simplify.rs:2086-2092 (fn effective_perms), via mappings/simplify.rs:2121 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 45 | Indicate we can swap with the PermMap | wcsmap.c:1078-1080 (fn CanSwap) | -- | ceremony | n/a: the helper returns `Some` only where C's flag would still be set, so there is no optimistic flag to clear on the way out |
| 46 | Check each output axis, rejecting the swap if a link between axes is not bi-directional | wcsmap.c:1081-1091 (fn CanSwap) | mappings/simplify.rs:2128-2132 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 47 | Check each input axis the same way | wcsmap.c:1092-1102 (fn CanSwap) | mappings/simplify.rs:2133-2137 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 48 | Check that the longitude and latitude axes both have bi-directional links, or are both unassigned | wcsmap.c:1103-1106 (fn CanSwap) | mappings/simplify.rs:2139-2164 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 49 | Get the indices of the longitude and latitude axes in the WcsMap | wcsmap.c:1107-1110 (fn CanSwap) | mappings/simplify.rs:2142-2143 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 50 | With the WcsMap applied first, read the two links through the PermMap's `inperm` | wcsmap.c:1111-1119 (fn CanSwap) | mappings/simplify.rs:2144-2148 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 51 | With the WcsMap applied second, read them through `outperm` against the input count instead | wcsmap.c:1120-1130 (fn CanSwap) | mappings/simplify.rs:2144-2157 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 52 | Where the swap is allowed, report whether the swapped Mappings are intrinsically simpler, which is when the PermMap loses axes in the direction it is applied | wcsmap.c:1131-1140 (fn CanSwap) | mappings/simplify.rs:2166-2172 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 53 | Free the axis permutation and constants arrays | wcsmap.c:1141-1148 (fn CanSwap) | -- | ceremony | n/a: the arrays are borrowed from the PermMap rather than copied out of it |
| 54 | Re-instate the original settings of the Invert attributes | wcsmap.c:1149-1153 (fn CanSwap) | -- | ceremony | n/a: the list entries are read, never mutated, so there is nothing to put back |
| 55 | Where the Mappings can swap, get the equivalent swapped Mappings | wcsmap.c:1154-1161 (fn CanSwap) | wcsmap.rs:970-997 (fn with_permuted_axes), via mappings/simplify.rs:2163 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 56 | Return a pointer to the swapped WcsMap | wcsmap.c:1162-1164 (fn CanSwap) | mappings/simplify.rs:2154-2164 (fn can_swap_wcsmap_past_permmap) | architecture | ported |
| 57 | Free the swapped copies | wcsmap.c:1165-1169 (fn CanSwap) | -- | ceremony | n/a: owned values are dropped |
| 58 | Return the answer | wcsmap.c:1170-1171 (fn CanSwap) | mappings/simplify.rs:2173 (fn can_swap_wcsmap_past_permmap) | architecture | ported |

## Rows 42 and 55 — one predicate, one swap, both C's

`CanSwap` tests the non-WcsMap member for being a PermMap and nothing else, so
swapping only past a PermMap is faithful rather than a reduced version of the
generic swap machinery MatrixMap and WinMap use. The rule shows up twice in the
port, because it shows up twice in C: `can_swap_wcsmap_past_permmap` is the
predicate, and `wcs_perm_at` is the transformation, which C reaches by calling
`WcsPerm` on clones inside `CanSwap` (row 55) and again on the list itself (rows
27 and 32).

The port splits the two differently from C in one respect. C obtains
`*newwcsmap` by performing the whole swap on a cloned pair and cloning the WcsMap
out of the result; the port builds it directly with
`WcsMap::with_permuted_axes`, the same call `wcs_perm_at` makes for the
list entry. The value is the same object either way: `WcsPerm`'s WcsMap half is
that rebuild and nothing else (`wcsmap.c:4704-4747 (fn WcsPerm)`).

## Rows 22 to 28 — the step walk decides which neighbor moves

C does not simply swap with whichever neighbor allows it. It walks forward from
the WcsMap over the run of PermMaps counting how many Mappings separate it from
a WcsMap the swapped WcsMap could cancel against, walks backward the same way,
and swaps towards the nearer of the two — with `do1`/`do2`, the "this swap is
itself simpler" flags, overriding the counts. `nstep2 > nstep1` breaks the tie
towards the lower neighbor, and so does `do1` being tested before `do2`.

Both walks are latent above a separation of one at this pin, and so is the
speculative branch of rows 29 to 34: two adjacent PermMaps always compose, so a
stable list never holds a run for the walk to step through, and the swapped
halves of a WcsMap/PermMap pair never change class. What the corpus does pin is
the choice itself, through `neg_wcsmap_perm_allconst` and
`neg_wcsmap_perm_both_sides`, which C declines to simplify and which start
simplifying if the choice is loosened. PLAN.md §10.31 records what a fixture
would need to reach the rest.

## Rows 12 and 13 — the lower pair is checked at this nomination

C checks the lower pair `(where-1, where)` before the upper pair
`(where, where+1)`, and the port keeps that order through the candidate list at
`mappings/simplify.rs:1229-1233 (fn wcs_map_merge)`. Doing the lower check here rather than
waiting for the lower WcsMap's own nomination matters beyond saving a pass: the
returned index cascades the nominate loop back through the canceled pair, which
determines the association order of the linear folds that follow.

## Row 6 — the reduction is the head of the merge

The `AST__WCSBAD` replacement is C's opening move, before any neighbor is
considered, and `mappings/simplify.rs:1219-1222 (fn wcs_map_merge)` puts it in the same
position. `wcsmap.c` has no `Simplify` vtab slot, so the base `Simplify` reaches
this `MapMerge` by nominating the WcsMap in a list of one
(`mapping.c:16871-16883 (fn Simplify)`). C makes the replacement the `if` of an if/else
whose `else` holds the whole neighbor search (`wcsmap.c:3142 (fn MapMerge)`,3113), which
is what the early return encodes: a reduced WcsMap never looks at a neighbor in
the same pass. The replacement carries a cleared Invert flag
(`wcsmap.c:3149 (fn MapMerge)`), so both flags are stored false.

## Row 36 — the fall-through is wider than C's return

C returns -1 here and the class is done. The port falls through to
`default_map_merge`, the generic pairwise neighbor attempt, which for a WcsMap
can only reach `WcsMap::try_merge` — the same inverse-pair cancellation rows 12
to 14 already tried, against the same two neighbors. It therefore adds no
reduction C would not make; it is the shared tail every class's merge body ends
in.

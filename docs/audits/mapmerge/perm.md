# `MapMerge` audit — PermMap

C source: reference/starlink-ast/src/permmap.c
Rust: crates/starlink-ast/src/mappings/perm.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 4029b011
Extension points: classified

Uncited C steps:
- permmap.c:909 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- permmap.c:953 (fn MapMerge) — clears C's own sentinels, a null replacement Mapping and empty permutation and constant arrays; the Rust locals are initialized where they are declared

C's body runs `permmap.c:947-1492 (fn MapMerge)`. It brackets a whole run of adjacent PermMaps
and UnitMaps, composes their permutations and constants into one replacement, and
replaces the run in a single step. Series and parallel are separate
accumulations of the same shape, sharing the tail that decides whether anything
was actually simplified.

`PermMap::map_merge` (`perm.rs:728-736 (fn map_merge)`) delegates to `simplify::perm_map_merge`
(`mappings/simplify.rs:1173-1200 (fn perm_map_merge)`), which reaches the composition pairwise through
`default_map_merge` and `PermMap::try_merge` rather than bracketing a run.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | permmap.c:947-948 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | permmap.c:950-951 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | In series, search adjacent lower-numbered Mappings until one is neither a PermMap nor a UnitMap | permmap.c:959-975 (fn MapMerge) | mappings/simplify.rs:1696-1705 (fn default_map_merge) | architecture | ported |
| 4 | Search adjacent higher-numbered Mappings the same way | permmap.c:976-986 (fn MapMerge) | mappings/simplify.rs:1683-1692 (fn default_map_merge) | architecture | ported |
| 5 | Take the first Mapping found and how its Invert attribute is applied | permmap.c:987-991 (fn MapMerge) | -- | architecture | n/a: a pairwise merge composes two entries at a time, so there is no run to take endpoints from |
| 6 | Use it to fix the replacement's input coordinate count | permmap.c:992-1000 (fn MapMerge) | perm.rs:1198-1203 (fn try_merge) | architecture | ported |
| 7 | Repeat for the last Mapping found to fix the output coordinate count | permmap.c:1001-1010 (fn MapMerge) | perm.rs:1189-1194 (fn try_merge) | architecture | ported |
| 8 | Allocate the replacement's input and output permutation arrays and its constants list | permmap.c:1011-1020 (fn MapMerge) | perm.rs:561-562 (fn compose_perms) | architecture | ported |
| 9 | Loop twice, building the forward and then the inverse permutation array | permmap.c:1021-1032 (fn MapMerge) | perm.rs:1188-1203 (fn try_merge) | architecture | ported |
| 10 | Set the scan limits so the run is walked forward or in reverse as the direction requires | permmap.c:1033-1038 (fn MapMerge) | -- | architecture | n/a: a pair has no scan order to choose; the two entries are composed in list order |
| 11 | Walk the run, taking each Mapping and the Invert value to apply, inverted for the reverse pass | permmap.c:1039-1051 (fn MapMerge) | perm.rs:1164-1186 (fn try_merge) | architecture | ported |
| 12 | For a PermMap take the relevant permutation array; for a UnitMap treat it as the identity | permmap.c:1052-1061 (fn MapMerge) | perm.rs:922-930 (fn try_merge) | architecture | ported |
| 13 | Obtain each Mapping's effective output count for the direction being built | permmap.c:1062-1075 (fn MapMerge) | perm.rs:561 (fn compose_perms) | architecture | ported |
| 14 | Accumulate the current Mapping's effect into the running permutation array | permmap.c:1076-1088 (fn MapMerge) | perm.rs:564-603 (fn compose_perms) | architecture | ported |
| 15 | Take the current Mapping's permuting effect, allowing for a NULL permutation array | permmap.c:1089-1093 (fn MapMerge) | perm.rs:921-931 (fn try_merge) | architecture | ported |
| 16 | Accumulate a permuted index that lands on a valid output | permmap.c:1094-1099 (fn MapMerge) | perm.rs:577-602 (fn compose_perms) | architecture | ported |
| 17 | Otherwise decide whether the index means a bad value or a constant | permmap.c:1100-1110 (fn MapMerge) | perm.rs:565-576 (fn compose_perms) | architecture | ported |
| 18 | Mark a bad result in the accumulated array with -1 | permmap.c:1111-1116 (fn MapMerge) | perm.rs:580-587 (fn compose_perms) | architecture | ported |
| 19 | Append an unseen constant to the constants list | permmap.c:1117-1125 (fn MapMerge) | perm.rs:525-533 (fn add_constant) | architecture | ported |
| 20 | Store a negative reference to that constant in the accumulated array | permmap.c:1126-1141 (fn MapMerge) | perm.rs:591-597 (fn compose_perms) | architecture | ported |
| 21 | In parallel, take the nominated PermMap and how its Invert attribute is applied | permmap.c:1142-1151 (fn MapMerge) | mappings/simplify.rs:1189-1190 (fn perm_map_merge) | architecture | ported |
| 22 | Initialize the replacement's coordinate counts from it | permmap.c:1152-1162 (fn MapMerge) | perm.rs:954-955 (fn try_merge) | architecture | ported |
| 23 | Search both directions for adjacent PermMaps and UnitMaps, accumulating their effective coordinate counts | permmap.c:1163-1209 (fn MapMerge) | mappings/simplify.rs:1683-1705 (fn default_map_merge) | architecture | ported |
| 24 | Allocate the replacement arrays and constants list, then build both directions as in series | permmap.c:1210-1339 (fn MapMerge) | perm.rs:957-1003 (fn try_merge) | architecture | ported |
| 25 | Accumulate each Mapping's input and output coordinate offsets across the run | permmap.c:1330-1339 (fn MapMerge) | perm.rs:973 (fn try_merge),983 (fn try_merge) | architecture | ported |
| 26 | Inspect the accumulated `inperm` to decide whether the replacement needs to store it | permmap.c:1340-1356 (fn MapMerge) | perm.rs:264-265 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 27 | Convert -1 entries back to non-existent positive indices meaning bad values | permmap.c:1357-1366 (fn MapMerge) | perm.rs:351-370 (fn rebuild_perms), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 28 | Do the same for `outperm` | permmap.c:1367-1380 (fn MapMerge) | perm.rs:371-373 (fn rebuild_perms), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 29 | Count how many adjacent Mappings the merge eliminates | permmap.c:1381-1384 (fn MapMerge) | -- | architecture | n/a: a pairwise merge always removes exactly one entry, so there is nothing to count |
| 30 | Decide whether the replacement PermMap reduces further to a UnitMap | permmap.c:1385-1391 (fn MapMerge) | perm.rs:264-271 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 31 | Determine whether any simplification actually happened, so a no-op is not reported as one | permmap.c:1392-1403 (fn MapMerge) | perm.rs:302-310 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 32 | Treat a single initial PermMap with its Invert flag set as simplified | permmap.c:1404-1414 (fn MapMerge) | perm.rs:248 (fn canonical_null_form),301 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 33 | Failing that, compare the original and replacement `inperm` arrays in detail | permmap.c:1415-1425 (fn MapMerge) | perm.rs:305 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 34 | Compare the `outperm` arrays likewise | permmap.c:1426-1434 (fn MapMerge) | perm.rs:306 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 35 | Do nothing more unless something simplified | permmap.c:1435-1437 (fn MapMerge) | perm.rs:308-310 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 36 | If the run collapses to a UnitMap, create that | permmap.c:1438-1442 (fn MapMerge) | perm.rs:269-271 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 37 | Otherwise create the replacement PermMap, passing NULL for every argument that can be omitted | permmap.c:1443-1451 (fn MapMerge) | perm.rs:312-324 (fn canonical_null_form), via mappings/simplify.rs:1194 (fn perm_map_merge) | architecture | ported |
| 38 | Annul the replaced pointers, insert the new one with a cleared invert flag, close the gap and return the first index | permmap.c:1452-1482 (fn MapMerge) | mappings/simplify.rs:1197-1198 (fn perm_map_merge) | architecture | ported |
| 39 | Free the workspace arrays | permmap.c:1483-1487 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 40 | Clear the returned value if an error occurred | permmap.c:1488-1490 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 41 | Return the result | permmap.c:1491-1492 (fn MapMerge) | mappings/simplify.rs:1199 (fn perm_map_merge) | ceremony | ported |

## A run at once, or a pair at a time

C brackets the entire run of adjacent PermMaps and UnitMaps and composes it in
one pass, which is why so many of its steps are about scan limits, endpoint
Mappings and elimination counts. The port composes pairwise, repeatedly, through
`PermMap::try_merge`. Permutation composition is associative and the constants
list is built the same way at each step, so the run reduces to the same
replacement in as many passes as it has elements. The rows about bracketing are
`n/a` for that reason, not because the composition is missing.

## Rows 26 to 34 — the canonical form is a simplification in its own right

C reports a simplification even when no neighbor merged, if the nominated
PermMap's stored form is not canonical: a redundant stored permutation array is
dropped, a set Invert flag is cleared, or the whole thing collapses to a UnitMap.
`canonical_null_form` (`perm.rs:243-325 (fn canonical_null_form)`) is that rebuild, and
`perm_map_merge` applies it after the neighbor attempt so the surrounding tree
is reconstructed with the canonical leaf rather than the stored form.

The direction it rebuilds from is the list's, `(*invert_list)[where]`, passed in
as `eff_inv` — clause (c) of C's `simpler` test reads that flag and not the
object's own. The two differ here because a list element is always stored
forward, so a PermMap applied inverted carries the inversion in the flag beside
it; keying the rebuild on the object would leave such a PermMap unchanged and
report no simplification.

## The CmpMap swap does not live here

`permmap.c` never mentions `CmpMap`: the swap of a parallel CmpMap in front of an
adjacent PermMap is run from the CmpMap's own `MapMerge` (`cmpmap.c:2045-2370 (fn MapMerge)`),
so a nominated PermMap never attempts it. The port matches, with the branch in
`cmp_map_merge` (`mappings/simplify.rs:965-977 (fn cmp_map_merge)`). It is audited as rows 31 to 40 of
`cmpmap.md`.

# `MapMerge` audit — PermMap

C source: reference/starlink-ast/src/permmap.c
Rust: crates/ast-core/src/mappings/perm.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- permmap.c:882 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- permmap.c:926 (fn MapMerge) — clears C's own sentinels, a null replacement Mapping and empty permutation and constant arrays; the Rust locals are initialised where they are declared

C's body runs `permmap.c:920-1460 (fn MapMerge)`. It brackets a whole run of adjacent PermMaps
and UnitMaps, composes their permutations and constants into one replacement, and
replaces the run in a single step. Series and parallel are separate
accumulations of the same shape, sharing the tail that decides whether anything
was actually simplified.

`PermMap::map_merge` (`perm.rs:573-581 (fn map_merge)`) delegates to `simplify::perm_map_merge`
(`mappings/simplify.rs:1059-1086 (fn perm_map_merge)`), which reaches the composition pairwise through
`default_map_merge` and `PermMap::try_merge` rather than bracketing a run.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | permmap.c:920-921 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | permmap.c:923-924 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | In series, search adjacent lower-numbered Mappings until one is neither a PermMap nor a UnitMap | permmap.c:932-948 (fn MapMerge) | mappings/simplify.rs:1449-1458 (fn default_map_merge) | architecture | ported |
| 4 | Search adjacent higher-numbered Mappings the same way | permmap.c:949-959 (fn MapMerge) | mappings/simplify.rs:1436-1445 (fn default_map_merge) | architecture | ported |
| 5 | Take the first Mapping found and how its Invert attribute is applied | permmap.c:960-964 (fn MapMerge) | -- | architecture | n/a: a pairwise merge composes two entries at a time, so there is no run to take endpoints from |
| 6 | Use it to fix the replacement's input coordinate count | permmap.c:965-973 (fn MapMerge) | perm.rs:1043-1048 (fn try_merge) | architecture | ported |
| 7 | Repeat for the last Mapping found to fix the output coordinate count | permmap.c:974-983 (fn MapMerge) | perm.rs:1034-1039 (fn try_merge) | architecture | ported |
| 8 | Allocate the replacement's input and output permutation arrays and its constants list | permmap.c:984-993 (fn MapMerge) | perm.rs:425-426 (fn compose_perms) | architecture | ported |
| 9 | Loop twice, building the forward and then the inverse permutation array | permmap.c:994-1005 (fn MapMerge) | perm.rs:1033-1048 (fn try_merge) | architecture | ported |
| 10 | Set the scan limits so the run is walked forward or in reverse as the direction requires | permmap.c:1006-1011 (fn MapMerge) | -- | architecture | n/a: a pair has no scan order to choose; the two entries are composed in list order |
| 11 | Walk the run, taking each Mapping and the Invert value to apply, inverted for the reverse pass | permmap.c:1012-1024 (fn MapMerge) | perm.rs:1009-1031 (fn try_merge) | architecture | ported |
| 12 | For a PermMap take the relevant permutation array; for a UnitMap treat it as the identity | permmap.c:1025-1035 (fn MapMerge) | perm.rs:762-770 (fn try_merge) | architecture | ported |
| 13 | Obtain each Mapping's effective output count for the direction being built | permmap.c:1036-1046 (fn MapMerge) | perm.rs:425 (fn compose_perms) | architecture | ported |
| 14 | Accumulate the current Mapping's effect into the running permutation array | permmap.c:1047-1059 (fn MapMerge) | perm.rs:428-458 (fn compose_perms) | architecture | ported |
| 15 | Take the current Mapping's permuting effect, allowing for a NULL permutation array | permmap.c:1060-1064 (fn MapMerge) | perm.rs:761-771 (fn try_merge) | architecture | ported |
| 16 | Accumulate a permuted index that lands on a valid output | permmap.c:1065-1070 (fn MapMerge) | perm.rs:437-457 (fn compose_perms) | architecture | ported |
| 17 | Otherwise decide whether the index means a bad value or a constant | permmap.c:1071-1081 (fn MapMerge) | perm.rs:429-436 (fn compose_perms) | architecture | ported |
| 18 | Mark a bad result in the accumulated array with -1 | permmap.c:1082-1087 (fn MapMerge) | perm.rs:440-447 (fn compose_perms) | architecture | ported |
| 19 | Append an unseen constant to the constants list | permmap.c:1088-1096 (fn MapMerge) | perm.rs:397-405 (fn add_constant) | architecture | ported |
| 20 | Store a negative reference to that constant in the accumulated array | permmap.c:1097-1112 (fn MapMerge) | perm.rs:448-452 (fn compose_perms) | architecture | ported |
| 21 | In parallel, take the nominated PermMap and how its Invert attribute is applied | permmap.c:1113-1122 (fn MapMerge) | mappings/simplify.rs:1075-1076 (fn perm_map_merge) | architecture | ported |
| 22 | Initialise the replacement's coordinate counts from it | permmap.c:1123-1133 (fn MapMerge) | perm.rs:794-795 (fn try_merge) | architecture | ported |
| 23 | Search both directions for adjacent PermMaps and UnitMaps, accumulating their effective coordinate counts | permmap.c:1134-1180 (fn MapMerge) | mappings/simplify.rs:1436-1458 (fn default_map_merge) | architecture | ported |
| 24 | Allocate the replacement arrays and constants list, then build both directions as in series | permmap.c:1181-1307 (fn MapMerge) | perm.rs:797-847 (fn try_merge) | architecture | ported |
| 25 | Accumulate each Mapping's input and output coordinate offsets across the run | permmap.c:1298-1307 (fn MapMerge) | perm.rs:814 (fn try_merge),825 (fn try_merge) | architecture | ported |
| 26 | Inspect the accumulated `inperm` to decide whether the replacement needs to store it | permmap.c:1308-1324 (fn MapMerge) | perm.rs:141-142 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 27 | Convert -1 entries back to non-existent positive indices meaning bad values | permmap.c:1325-1334 (fn MapMerge) | perm.rs:228-247 (fn rebuild_perms), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 28 | Do the same for `outperm` | permmap.c:1335-1348 (fn MapMerge) | perm.rs:248-250 (fn rebuild_perms), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 29 | Count how many adjacent Mappings the merge eliminates | permmap.c:1349-1352 (fn MapMerge) | -- | architecture | n/a: a pairwise merge always removes exactly one entry, so there is nothing to count |
| 30 | Decide whether the replacement PermMap reduces further to a UnitMap | permmap.c:1353-1359 (fn MapMerge) | perm.rs:141-148 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 31 | Determine whether any simplification actually happened, so a no-op is not reported as one | permmap.c:1360-1371 (fn MapMerge) | perm.rs:179-187 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 32 | Treat a single initial PermMap with its Invert flag set as simplified | permmap.c:1372-1382 (fn MapMerge) | perm.rs:125 (fn canonical_null_form),178 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 33 | Failing that, compare the original and replacement `inperm` arrays in detail | permmap.c:1383-1393 (fn MapMerge) | perm.rs:182 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 34 | Compare the `outperm` arrays likewise | permmap.c:1394-1402 (fn MapMerge) | perm.rs:183 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 35 | Do nothing more unless something simplified | permmap.c:1403-1405 (fn MapMerge) | perm.rs:185-187 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 36 | If the run collapses to a UnitMap, create that | permmap.c:1406-1410 (fn MapMerge) | perm.rs:146-148 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 37 | Otherwise create the replacement PermMap, passing NULL for every argument that can be omitted | permmap.c:1411-1419 (fn MapMerge) | perm.rs:189-201 (fn canonical_null_form), via mappings/simplify.rs:1080 (fn perm_map_merge) | architecture | ported |
| 38 | Annul the replaced pointers, insert the new one with a cleared invert flag, close the gap and return the first index | permmap.c:1420-1450 (fn MapMerge) | mappings/simplify.rs:1083-1084 (fn perm_map_merge) | architecture | ported |
| 39 | Free the workspace arrays | permmap.c:1451-1455 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 40 | Clear the returned value if an error occurred | permmap.c:1456-1458 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 41 | Return the result | permmap.c:1459-1460 (fn MapMerge) | mappings/simplify.rs:1085 (fn perm_map_merge) | ceremony | ported |

## A run at once, or a pair at a time

C brackets the entire run of adjacent PermMaps and UnitMaps and composes it in
one pass, which is why so many of its steps are about scan limits, endpoint
Mappings and elimination counts. The port composes pairwise, repeatedly, through
`PermMap::try_merge`. Permutation composition is associative and the constants
list is built the same way at each step, so the run reduces to the same
replacement in as many passes as it has elements. The rows about bracketing are
`n/a` for that reason, not because the composition is missing.

## Rows 26 to 34 — the canonical form is a simplification in its own right

C reports a simplification even when no neighbour merged, if the nominated
PermMap's stored form is not canonical: a redundant stored permutation array is
dropped, a set Invert flag is cleared, or the whole thing collapses to a UnitMap.
`canonical_null_form` (`perm.rs:120-202 (fn canonical_null_form)`) is that rebuild, and
`perm_map_merge` applies it after the neighbour attempt so the surrounding tree
is reconstructed with the canonical leaf rather than the stored form.

The direction it rebuilds from is the list's, `(*invert_list)[where]`, passed in
as `eff_inv` — clause (c) of C's `simpler` test reads that flag and not the
object's own. The two differ here because a list element is always stored
forward, so a PermMap applied inverted carries the inversion in the flag beside
it; keying the rebuild on the object would leave such a PermMap unchanged and
report no simplification.

## The CmpMap swap does not live here

`permmap.c` never mentions `CmpMap`: the swap of a parallel CmpMap in front of an
adjacent PermMap is run from the CmpMap's own `MapMerge` (`cmpmap.c:2014-2323 (fn MapMerge)`),
so a nominated PermMap never attempts it. The port matches, with the branch in
`cmp_map_merge` (`mappings/simplify.rs:853-865 (fn cmp_map_merge)`). It is audited as rows 31 to 40 of
`cmpmap.md`.

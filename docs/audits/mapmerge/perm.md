# `MapMerge` audit — PermMap

C source: reference/starlink-ast/src/permmap.c
Rust: crates/ast-core/src/mappings/perm.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `permmap.c:916-1456`. It brackets a whole run of adjacent PermMaps
and UnitMaps, composes their permutations and constants into one replacement, and
replaces the run in a single step. Series and parallel are separate
accumulations of the same shape, sharing the tail that decides whether anything
was actually simplified.

`PermMap::map_merge` (`perm.rs:534-541`) delegates to `simplify::perm_map_merge`
(`simplify.rs:1010-1030`), which reaches the composition pairwise through
`default_map_merge` and `PermMap::try_merge` rather than bracketing a run.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | permmap.c:916-917 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | permmap.c:919-920 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | In series, search adjacent lower-numbered Mappings until one is neither a PermMap nor a UnitMap | permmap.c:928-944 | simplify.rs:1155-1250 | ported |
| 4 | Search adjacent higher-numbered Mappings the same way | permmap.c:945-955 | simplify.rs:1155-1250 | ported |
| 5 | Take the first Mapping found and how its Invert attribute is applied | permmap.c:956-960 | -- | n/a: a pairwise merge composes two entries at a time, so there is no run to take endpoints from |
| 6 | Use it to fix the replacement's input coordinate count | permmap.c:961-969 | perm.rs:751-850 | ported |
| 7 | Repeat for the last Mapping found to fix the output coordinate count | permmap.c:970-979 | perm.rs:751-850 | ported |
| 8 | Allocate the replacement's input and output permutation arrays and its constants list | permmap.c:980-989 | perm.rs:751-850 | ported |
| 9 | Loop twice, building the forward and then the inverse permutation array | permmap.c:990-1001 | perm.rs:751-850 | ported |
| 10 | Set the scan limits so the run is walked forward or in reverse as the direction requires | permmap.c:1002-1007 | -- | n/a: a pair has no scan order to choose; the two entries are composed in list order |
| 11 | Walk the run, taking each Mapping and the Invert value to apply, inverted for the reverse pass | permmap.c:1008-1020 | perm.rs:751-850 | ported |
| 12 | For a PermMap take the relevant permutation array; for a UnitMap treat it as the identity | permmap.c:1021-1031 | perm.rs:766-772 | ported |
| 13 | Obtain each Mapping's effective output count for the direction being built | permmap.c:1032-1042 | perm.rs:751-850 | ported |
| 14 | Accumulate the current Mapping's effect into the running permutation array | permmap.c:1043-1055 | perm.rs:751-850 | ported |
| 15 | Take the current Mapping's permuting effect, allowing for a NULL permutation array | permmap.c:1056-1060 | perm.rs:766-772 | ported |
| 16 | Accumulate a permuted index that lands on a valid output | permmap.c:1061-1066 | perm.rs:751-850 | ported |
| 17 | Otherwise decide whether the index means a bad value or a constant | permmap.c:1067-1077 | perm.rs:751-850 | ported |
| 18 | Mark a bad result in the accumulated array with -1 | permmap.c:1078-1083 | perm.rs:751-850 | ported |
| 19 | Append an unseen constant to the constants list | permmap.c:1084-1092 | perm.rs:751-850 | ported |
| 20 | Store a negative reference to that constant in the accumulated array | permmap.c:1093-1108 | perm.rs:751-850 | ported |
| 21 | In parallel, take the nominated PermMap and how its Invert attribute is applied | permmap.c:1109-1118 | simplify.rs:1020-1024 | ported |
| 22 | Initialise the replacement's coordinate counts from it | permmap.c:1119-1129 | perm.rs:758-790 | ported |
| 23 | Search both directions for adjacent PermMaps and UnitMaps, accumulating their effective coordinate counts | permmap.c:1130-1176 | simplify.rs:1155-1250 | ported |
| 24 | Allocate the replacement arrays and constants list, then build both directions as in series | permmap.c:1177-1303 | perm.rs:758-850 | ported |
| 25 | Accumulate each Mapping's input and output coordinate offsets across the run | permmap.c:1294-1303 | perm.rs:758-850 | ported |
| 26 | Inspect the accumulated `inperm` to decide whether the replacement needs to store it | permmap.c:1304-1320 | perm.rs:106-160 | ported |
| 27 | Convert -1 entries back to non-existent positive indices meaning bad values | permmap.c:1321-1330 | perm.rs:106-160 | ported |
| 28 | Do the same for `outperm` | permmap.c:1331-1344 | perm.rs:106-160 | ported |
| 29 | Count how many adjacent Mappings the merge eliminates | permmap.c:1345-1348 | -- | n/a: a pairwise merge always removes exactly one entry, so there is nothing to count |
| 30 | Decide whether the replacement PermMap reduces further to a UnitMap | permmap.c:1349-1355 | perm.rs:543-570 | ported |
| 31 | Determine whether any simplification actually happened, so a no-op is not reported as one | permmap.c:1356-1367 | simplify.rs:1020-1029 | ported |
| 32 | Treat a single initial PermMap with its Invert flag set as simplified | permmap.c:1368-1378 | perm.rs:106-115 | ported |
| 33 | Failing that, compare the original and replacement `inperm` arrays in detail | permmap.c:1379-1389 | perm.rs:106-160 | ported |
| 34 | Compare the `outperm` arrays likewise | permmap.c:1390-1398 | perm.rs:106-160 | ported |
| 35 | Do nothing more unless something simplified | permmap.c:1399-1401 | simplify.rs:1024-1029 | ported |
| 36 | If the run collapses to a UnitMap, create that | permmap.c:1402-1406 | perm.rs:543-570 | ported |
| 37 | Otherwise create the replacement PermMap, passing NULL for every argument that can be omitted | permmap.c:1407-1415 | perm.rs:106-160 | ported |
| 38 | Annul the replaced pointers, insert the new one with a cleared invert flag, close the gap and return the first index | permmap.c:1416-1446 | simplify.rs:1026-1029 | ported |
| 39 | Free the workspace arrays | permmap.c:1447-1451 | -- | n/a: owned values are dropped |
| 40 | Clear the returned value if an error occurred | permmap.c:1452-1454 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 41 | Return the result | permmap.c:1455-1456 | simplify.rs:1029 | ported |

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
`canonical_null_form` (`perm.rs:106-160`) is that rebuild, and
`perm_map_merge` applies it after the neighbour attempt so the surrounding tree
is reconstructed with the canonical leaf rather than the stored form.

## The CmpMap swap that lives here

`perm_map_merge` also performs the CmpMap-past-PermMap swap, which in C belongs
to `cmpmap.c::MapMerge` and fires on the other element. It is audited as rows 18
to 27 of `cmpmap.md`; see PLAN.md §10.4.

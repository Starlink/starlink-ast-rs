# `MapMerge` audit — UnitMap

C source: reference/starlink-ast/src/unitmap.c
Rust: crates/ast-core/src/mappings/unit.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3e60abaa
Extension points: classified

Uncited C steps:
- unitmap.c:490 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `unitmap.c:500-626 (fn MapMerge)` in three parts: a lone UnitMap has its Invert
flag cleared; a UnitMap in series with anything is eliminated; a UnitMap in
parallel is coalesced with adjacent UnitMaps.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | unitmap.c:500-501 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | unitmap.c:503-504 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | If the sequence is a single UnitMap, clear its Invert flag if set | unitmap.c:506-511 (fn MapMerge) | unit.rs:102-112 (fn map_merge) | architecture | ported |
| 4 | Note whether the sequence was modified | unitmap.c:512-515 (fn MapMerge) | unit.rs:107-109 (fn map_merge) | architecture | ported |
| 5 | In series, a UnitMap next to anything else is eliminated, so annul its pointer | unitmap.c:516-523 (fn MapMerge) | unit.rs:114-117 (fn map_merge) | architecture | ported |
| 6 | Shuffle the following pointers and Invert flags down to fill the gap, clearing the vacated slot | unitmap.c:524-533 (fn MapMerge) | unit.rs:115 (fn map_merge) | architecture | ported |
| 7 | Decrement the Mapping count and return the index of the first modified element | unitmap.c:534-538 (fn MapMerge) | unit.rs:115-116 (fn map_merge) | architecture | ported |
| 8 | In parallel, a UnitMap can be merged with UnitMaps on either side | unitmap.c:539-544 (fn MapMerge) | unit.rs:119 (fn map_merge) | architecture | ported |
| 9 | Initialise indices for the adjacent UnitMaps and the coordinate count of the replacement | unitmap.c:545-549 (fn MapMerge) | -- | architecture | n/a: the port's pairwise hook is asked about one neighbour at a time, so there is no run to bracket |
| 10 | Inspect earlier Mappings, stopping at the first that is not a UnitMap | unitmap.c:550-556 (fn MapMerge) | mappings/simplify.rs:1218-1227 (fn default_map_merge) | architecture | ported |
| 11 | Extend the run's start index and accumulate its coordinate count | unitmap.c:557-562 (fn MapMerge) | unit.rs:139-142 (fn try_merge) | architecture | ported |
| 12 | Repeat for later Mappings in the sequence | unitmap.c:563-571 (fn MapMerge) | mappings/simplify.rs:1231-1261 (fn default_map_merge) | architecture | ported |
| 13 | Compute the net reduction and check whether it is zero, meaning no adjacent UnitMaps | unitmap.c:572-578 (fn MapMerge) | -- | architecture | n/a: a pairwise hook that finds no UnitMap neighbour returns `None`, which is the same answer without a count |
| 14 | With no adjacent UnitMaps, clear the nominated UnitMap's Invert flag if set | unitmap.c:579-587 (fn MapMerge) | -- | architecture | n/a: the flag is the list's, folded into the leaf by the generic rebuild rather than by a UnitMap seam (`mappings/simplify.rs:3125-3135 (fn reconstruct_inner)`) |
| 15 | If UnitMaps can be merged, create the replacement UnitMap | unitmap.c:588-592 (fn MapMerge) | unit.rs:141-142 (fn try_merge) | architecture | ported |
| 16 | Annul the pointers to all the UnitMaps being replaced | unitmap.c:593-597 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 17 | Insert the new pointer and its Invert flag, close the gap, and clear the vacated slots | unitmap.c:598-614 (fn MapMerge) | mappings/simplify.rs:1293-1299 (fn default_map_merge) | architecture | ported |
| 18 | Decrement the Mapping count and return the index of the first modified element | unitmap.c:615-624 (fn MapMerge) | mappings/simplify.rs:1299-1301 (fn default_map_merge) | architecture | ported |
| 19 | Return the result | unitmap.c:625-626 (fn MapMerge) | unit.rs:119 (fn map_merge) | ceremony | ported |

## Rows 5 to 7 — removing in the loop, not after it

`UnitMap::map_merge` removes the entry inside the nominate loop rather than in a
post-loop strip, because that is what advances C's cursor the same way: after it
returns `where_`, the Mapping that shuffled into `where_` is nominated next. For
a Win-Unit-Matrix run that is how C reaches `MatrixMap::MapMerge` instead of the
WinMap's own merge.

## Rows 8 to 18 — the parallel run, one pair at a time

C brackets the whole run of adjacent UnitMaps and replaces it in one step. The
port reaches the same result through `UnitMap::try_merge` in parallel mode
(`unit.rs:139-142 (fn try_merge)`), called repeatedly by `default_map_merge`, so a run of three
coalesces in two passes rather than one. The rows about bracketing the run are
`n/a` for that reason, not because the reduction is missing.

## Row 14 — the flag C clears is the list's, not the object's

`invert_list[ where ]` is the direction the enclosing CmpMap will apply, so
clearing it surfaces in the dump as the parent dropping its `InvA` or `InvB`, and
not as a change to the UnitMap's own `Invert` card. The port keeps that direction
as a separate value in `MergeCtx` and folds it into the leaf when the tree is
rebuilt, which reaches the same dump — `unit_parallel_invert_clear` and
`unit_parallel_invert_pcd` are byte-exact against the pinned C on two different
neighbour classes.

`UnitMap::map_merge` does not itself clear anything here, which is why this row
was first read as a gap. See PLAN.md §10.6 for the retraction.

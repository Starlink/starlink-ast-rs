# `MapMerge` audit — UnitMap

C source: reference/starlink-ast/src/unitmap.c
Rust: crates/starlink-ast/src/mappings/unit.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin fe26cc9b
Extension points: classified

Uncited C steps:
- unitmap.c:494 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `unitmap.c:504-630 (fn MapMerge)` in three parts: a lone UnitMap has its Invert
flag cleared; a UnitMap in series with anything is eliminated; a UnitMap in
parallel is coalesced with adjacent UnitMaps.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | unitmap.c:504-505 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | unitmap.c:507-508 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | If the sequence is a single UnitMap, clear its Invert flag if set | unitmap.c:510-515 (fn MapMerge) | unit.rs:167-178 (fn map_merge) | architecture | ported |
| 4 | Note whether the sequence was modified | unitmap.c:516-519 (fn MapMerge) | unit.rs:175-177 (fn map_merge) | architecture | ported |
| 5 | In series, a UnitMap next to anything else is eliminated, so annul its pointer | unitmap.c:520-527 (fn MapMerge) | unit.rs:186-189 (fn map_merge) | architecture | ported |
| 6 | Shuffle the following pointers and Invert flags down to fill the gap, clearing the vacated slot | unitmap.c:528-537 (fn MapMerge) | unit.rs:187 (fn map_merge) | architecture | ported |
| 7 | Decrement the Mapping count and return the index of the first modified element | unitmap.c:538-542 (fn MapMerge) | unit.rs:187-188 (fn map_merge) | architecture | ported |
| 8 | In parallel, a UnitMap can be merged with UnitMaps on either side | unitmap.c:543-548 (fn MapMerge) | unit.rs:191-194 (fn map_merge) | architecture | ported |
| 9 | Initialize indices for the adjacent UnitMaps and the coordinate count of the replacement | unitmap.c:549-553 (fn MapMerge) | unit.rs:195-196 (fn map_merge) | architecture | ported |
| 10 | Inspect earlier Mappings, stopping at the first that is not a UnitMap | unitmap.c:554-560 (fn MapMerge) | unit.rs:197 (fn map_merge) | architecture | ported |
| 11 | Extend the run's start index and accumulate its coordinate count | unitmap.c:561-566 (fn MapMerge) | unit.rs:198-199 (fn map_merge) | architecture | ported |
| 12 | Repeat for later Mappings in the sequence | unitmap.c:567-575 (fn MapMerge) | unit.rs:201-204 (fn map_merge) | architecture | ported |
| 13 | Compute the net reduction and check whether it is zero, meaning no adjacent UnitMaps | unitmap.c:576-582 (fn MapMerge) | unit.rs:206 (fn map_merge) | architecture | ported |
| 14 | With no adjacent UnitMaps, clear the nominated UnitMap's Invert flag if set | unitmap.c:583-591 (fn MapMerge) | unit.rs:207-218 (fn map_merge) | architecture | ported |
| 15 | If UnitMaps can be merged, create the replacement UnitMap | unitmap.c:592-596 (fn MapMerge) | unit.rs:222 (fn map_merge) | architecture | ported |
| 16 | Annul the pointers to all the UnitMaps being replaced | unitmap.c:597-601 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 17 | Insert the new pointer and its Invert flag, close the gap, and clear the vacated slots | unitmap.c:602-618 (fn MapMerge) | unit.rs:222 (fn map_merge) | architecture | ported |
| 18 | Decrement the Mapping count and return the index of the first modified element | unitmap.c:619-628 (fn MapMerge) | unit.rs:223 (fn map_merge) | architecture | ported |
| 19 | Return the result | unitmap.c:629-630 (fn MapMerge) | unit.rs:223 (fn map_merge) | ceremony | ported |

## Rows 5 to 7 — removing in the loop, not after it

`UnitMap::map_merge` removes the entry inside the nominate loop rather than in a
post-loop strip, because that is what advances C's cursor the same way: after it
returns `where_`, the Mapping that shuffled into `where_` is nominated next. For
a Win-Unit-Matrix run that is how C reaches `MatrixMap::MapMerge` instead of the
WinMap's own merge.

## Rows 8 to 18 — the parallel run, bracketed as C does

`UnitMap::map_merge` brackets the whole run of adjacent UnitMaps and replaces it
in one step, as C does, so a run of three coalesces in one pass. The run is
found by exact class, as C's `strcmp` on the class name finds it.

## Row 14 — the flag C clears is the list's, not the object's

`invert_list[ where ]` is the direction the enclosing CmpMap will apply, so
clearing it surfaces in the dump as the parent dropping its `InvA` or `InvB`,
while the UnitMap keeps its own `Invert` card. The port clears the
`effective_inv` of the `MergeCtx` entry and keeps `orig_inv`, which reaches the
same dump: `unit_parallel_invert_clear` and `unit_parallel_invert_pcd` are
byte-exact against the pinned C on two different neighbor classes.

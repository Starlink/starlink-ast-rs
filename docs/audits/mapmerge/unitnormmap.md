# `MapMerge` audit — UnitNormMap

C source: reference/starlink-ast/src/unitnormmap.c
Rust: crates/ast-core/src/mappings/unitnormmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `unitnormmap.c:820-883`. It walks the pairs from `where - 1` to the
end of the list, asking `MakeMergedMap` (`unitnormmap.c:323-505`) about each, and
splices in the first result. Both are audited here.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | unitnormmap.c:820-821 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | unitnormmap.c:823-824 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | In series, try to merge the UnitNormMap with either of its neighbours | unitnormmap.c:828-847 | unitnormmap.rs:436-444 | ported |
| 4 | Set both members of each pair to the Invert values the list requests, then restore them | unitnormmap.c:838-845 | unitnormmap.rs:439-441 | ported |
| 5 | Stop at the first pair `MakeMergedMap` accepts | unitnormmap.c:846-852 | unitnormmap.rs:441-443 | ported |
| 6 | Replace the first of the pair with the merged Mapping and update its invert flag | unitnormmap.c:854-859 | unitnormmap.rs:442 | ported |
| 7 | Annul the second and shuffle the rest down, clearing the vacated slot | unitnormmap.c:860-871 | unitnormmap.rs:442 | ported |
| 8 | Decrement the Mapping count and return the index of the first modified element | unitnormmap.c:872-877 | unitnormmap.rs:443 | ported |
| 9 | In parallel, a UnitNormMap combines with nothing | unitnormmap.c:878-881 | unitnormmap.rs:424-426 | ported |
| 10 | Return the result | unitnormmap.c:882-883 | unitnormmap.rs:446 | ported |
| 11 | `MakeMergedMap`: initialise the returned Mapping to NULL | unitnormmap.c:378-380 | -- | n/a: the Rust helper returns `Option<DynMapping>` |
| 12 | Classify both Mappings as UnitNormMap, ShiftMap or other | unitnormmap.c:381-392 | unitnormmap.rs:466-512 | ported |
| 13 | ShiftMap followed by a forward UnitNormMap gives a forward UnitNormMap | unitnormmap.c:393-409 | unitnormmap.rs:466-512 | ported |
| 14 | A unit-scale WinMap followed by a forward UnitNormMap likewise, declining when the scales are not unity | unitnormmap.c:410-430 | unitnormmap.rs:466-512 | ported |
| 15 | An inverted UnitNormMap followed by a ShiftMap gives an inverted UnitNormMap | unitnormmap.c:431-447 | unitnormmap.rs:466-512 | ported |
| 16 | An inverted UnitNormMap followed by a WinMap likewise, declining on non-unit scales | unitnormmap.c:448-468 | unitnormmap.rs:466-512 | ported |
| 17 | Two UnitNormMaps in opposite directions | unitnormmap.c:469-483 | unitnormmap.rs:466-512 | ported |
| 18 | With identical centres they cancel to a UnitMap | unitnormmap.c:484-488 | unitnormmap.rs:466-512 | ported |
| 19 | Otherwise a forward then an inverted UnitNormMap gives a ShiftMap | unitnormmap.c:489-505 | unitnormmap.rs:466-512 | ported |

## Row 3 — the scan reaches past the nominee

C's loop starts at `MAX( where - 1, 0 )` and runs to the end of the list rather
than stopping after the nominee's own two pairs, so a merge anywhere downstream
is taken on this nomination. `unitnormmap.rs:437` walks the same range.

## Why this class needs its own `map_merge`

Neither `default_map_merge` nor the neighbours' own merges cover the lower pair.
WinMap's routes every case through `merge_linear_pair`, which handles only linear
classes, and a UnitNormMap is not one. So the `<linear> ; UnitNormMap(forward)`
absorptions would be missed entirely if this class relied on the generic path.

## A note C leaves in place

`unitnormmap.c:359` records that C's own ShiftMap-plus-UnitNormMap branch never
runs, because a ShiftMap has become a WinMap by the time this is reached — see
row 3 of `shift.md`. The branch is kept in C and ported here for the same reason:
it is the documented rule even where the path is unreachable.

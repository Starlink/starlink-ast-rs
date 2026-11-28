# `MapMerge` audit — UnitNormMap

C source: reference/starlink-ast/src/unitnormmap.c
Rust: crates/starlink-ast/src/mappings/unitnormmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 4029b011
Extension points: classified

Uncited C steps:
- unitnormmap.c:358 (fn MakeMergedMap) — the function prologue, not a step; its leading blank field keeps it out of the tool's prologue test

C's body runs `unitnormmap.c:854-917 (fn MapMerge)`. It walks the pairs from `where - 1` to the
end of the list, asking `MakeMergedMap` (`unitnormmap.c:357-539 (fn MakeMergedMap)`) about each, and
splices in the first result. Both are audited here.

`UnitNormMap::map_merge` (`unitnormmap.rs:563-595 (fn map_merge)`) walks the same pairs and calls
`make_merged_map` (`unitnormmap.rs:654-682 (fn make_merged_map)`), which dispatches to
`UnitNormMap::try_merge` (`unitnormmap.rs:428-561 (fn try_merge)`) or to
`try_merge_into_unitnormmap` (`unitnormmap.rs:689-751 (fn try_merge_into_unitnormmap)`) according to
which member of the pair is the UnitNormMap. The splice itself is
`MergeCtx::replace_pair_with` (`mappings/simplify.rs:677-690 (fn replace_pair_with)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | unitnormmap.c:854-856 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | unitnormmap.c:857-859 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | In series, walk the pairs from `where - 1` to the end of the list | unitnormmap.c:862-868 (fn MapMerge) | unitnormmap.rs:583-585 (fn map_merge) | architecture | ported |
| 4 | Set both members of each pair to the Invert values the list requests, then restore them | unitnormmap.c:869-875 (fn MapMerge) | unitnormmap.rs:586-588 (fn map_merge) | architecture | ported |
| 5 | Ask `MakeMergedMap` about the pair, stop at the first it accepts, and report no change when none does | unitnormmap.c:876-886 (fn MapMerge) | unitnormmap.rs:588-594 (fn map_merge) | architecture | ported |
| 6 | Replace the first of the pair with the merged Mapping and update its invert flag | unitnormmap.c:888-893 (fn MapMerge) | unitnormmap.rs:589 (fn map_merge), mappings/simplify.rs:685-689 (fn replace_pair_with) | architecture | ported |
| 7 | Annul the second of the pair, shuffle the rest down and clear the vacated element | unitnormmap.c:894-905 (fn MapMerge) | unitnormmap.rs:589 (fn map_merge), mappings/simplify.rs:689 (fn replace_pair_with) | architecture | ported |
| 8 | Decrement the Mapping count and return the index of the first modified element | unitnormmap.c:906-908 (fn MapMerge) | unitnormmap.rs:590 (fn map_merge) | architecture | ported |
| 9 | In parallel, a UnitNormMap combines with nothing | unitnormmap.c:912-914 (fn MapMerge) | unitnormmap.rs:571-573 (fn map_merge) | architecture | ported |
| 10 | Return the result | unitnormmap.c:916-917 (fn MapMerge) | unitnormmap.rs:586 (fn map_merge),590 (fn map_merge) | ceremony | ported |
| 11 | `MakeMergedMap`: check the global error status and bail out | unitnormmap.c:409-411 (fn MakeMergedMap) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 12 | Initialize the returned Mapping to NULL | unitnormmap.c:412-414 (fn MakeMergedMap) | -- | ceremony | n/a: the Rust helper returns `Option<DynMapping>`, so "no merge" is `None` |
| 13 | Classify both Mappings as UnitNormMap, ShiftMap, WinMap or other, and decline unless both are classified and at least one is a UnitNormMap | unitnormmap.c:415-426 (fn MakeMergedMap) | unitnormmap.rs:660-670 (fn make_merged_map) | architecture | ported |
| 14 | ShiftMap followed by a forward UnitNormMap gives a forward UnitNormMap | unitnormmap.c:427-443 (fn MakeMergedMap) | unitnormmap.rs:702-720 (fn try_merge_into_unitnormmap) | architecture | ported |
| 15 | A unit-scale WinMap followed by a forward UnitNormMap likewise, declining when the scales are not unity | unitnormmap.c:444-464 (fn MakeMergedMap) | unitnormmap.rs:722-748 (fn try_merge_into_unitnormmap) | architecture | ported |
| 16 | An inverted UnitNormMap followed by a ShiftMap gives an inverted UnitNormMap | unitnormmap.c:465-481 (fn MakeMergedMap) | unitnormmap.rs:493-517 (fn try_merge) | architecture | ported |
| 17 | An inverted UnitNormMap followed by a WinMap likewise, declining on non-unit scales | unitnormmap.c:482-502 (fn MakeMergedMap) | unitnormmap.rs:520-551 (fn try_merge) | architecture | ported |
| 18 | Two UnitNormMaps merge only in opposite directions; compare their centers | unitnormmap.c:503-517 (fn MakeMergedMap) | unitnormmap.rs:445-459 (fn try_merge) | architecture | ported |
| 19 | With identical centers they cancel to a UnitMap | unitnormmap.c:518-522 (fn MakeMergedMap) | unitnormmap.rs:461-470 (fn try_merge) | architecture | ported |
| 20 | Otherwise a forward then an inverted UnitNormMap gives a ShiftMap | unitnormmap.c:523-539 (fn MakeMergedMap) | unitnormmap.rs:465-476 (fn try_merge) | architecture | ported |

## Row 3 — the scan reaches past the nominee

C's loop starts at `MAX( where - 1, 0 )` and runs to the end of the list rather
than stopping after the nominee's own two pairs, so a merge anywhere downstream
is taken on this nomination. `unitnormmap.rs:583-584 (fn map_merge)` walks the same range.

## Row 4 — the flag C sets, and the argument Rust passes

C has one object per list element and a single `Invert` flag on it, so before
asking `MakeMergedMap` about a pair it writes the list's flags into both objects
and writes the objects' own flags back afterwards. The port passes the two flags
as arguments instead: `unitnormmap.rs:588 (fn map_merge)` hands `*inv1` and `*inv2` to
`make_merged_map`, which forwards them to whichever helper takes the pair, and
each helper folds the argument against the object's stored `inverted` to get the
effective direction — `unitnormmap.rs:442 (fn try_merge)` and `unitnormmap.rs:695 (fn try_merge_into_unitnormmap)`.
Nothing is mutated, so there is nothing to restore.

## Rows 14 to 17 — which member of the pair is the UnitNormMap

C's four absorbing branches are selected by `type1` and `type2` together: a
ShiftMap or WinMap *before* a forward UnitNormMap, or one *after* an inverted
UnitNormMap. The port splits the same selection across two functions rather than
four branches. `try_merge_into_unitnormmap` takes the lower pair, where the
UnitNormMap is second, and declines unless it is effectively forward;
`UnitNormMap::try_merge` takes the upper pair, where it is first, and declines
unless it is effectively inverted. The center arithmetic differs by the sign of
the shift for that reason: the lower pair subtracts the neighbor's shift from
the center and the upper pair adds it.

## Rows 18 to 20 — the two-UnitNormMap case

C compares the centers over `MIN( astGetNin( map1 ), astGetNin( map2 ) )` axes,
which for a composable pair is the shared coordinate count, since a forward
UnitNormMap has `nin == ncoord` and an inverted one `nin == ncoord + 1`. The port
tests `self.ncoord() == other_unm.ncoord()` first and compares every axis, which
is the same comparison for any pair a series CmpMap can hold and an explicit
decline for one it cannot.

## The UnitMap case Rust answers and C declines

`unitnormmap.rs:554-558 (fn try_merge)` merges an adjacent UnitMap by returning the
UnitNormMap unchanged. C's `GetMappingType` calls a UnitMap type 0, so
`MakeMergedMap` declines the pair and `unitmap.c`'s own `MapMerge` eliminates the
UnitMap instead; C records that division of labour in `MakeMergedMap`'s notes.
The outcome is the same list either way, and the Rust branch is unreachable
through `map_merge` because `make_merged_map`'s classification gate excludes a
UnitMap before the branch is offered the pair. It is reached only from
`default_map_merge`, where it stands in for C's UnitMap elimination.

## A lone UnitNormMap has no reduction of its own

Every rule above needs a neighbor: an adjacent ShiftMap or unit-scale WinMap to
absorb, or an opposite-direction UnitNormMap to cancel against.
`unitnormmap.c` has no `Simplify` vtab slot, so a UnitNormMap on its own is
returned unchanged, and the base `Simplify` reaches this `MapMerge` by nominating
it in a list of one (`mapping.c:16997-17009 (fn Simplify)`). The port matches that by
implementing `map_merge` and leaving `Mapping::simplified` at its default, so a
lone UnitNormMap can never be stamped as simplified in isolation.

## Why this class needs its own `map_merge`

Neither `default_map_merge` nor the neighbors' own merges cover the lower pair.
WinMap's routes every case through `merge_linear_pair`, which handles only linear
classes, and a UnitNormMap is not one. So the `<linear> ; UnitNormMap(forward)`
absorptions would be missed entirely if this class relied on the generic path.

## A note C leaves in place

`unitnormmap.c:393 (fn MakeMergedMap)` records that C's own ShiftMap-plus-UnitNormMap branch never
runs, because a ShiftMap has become a WinMap by the time this is reached — see
row 3 of `shift.md`. The branch is kept in C and ported here for the same reason:
it is the documented rule even where the path is unreachable.

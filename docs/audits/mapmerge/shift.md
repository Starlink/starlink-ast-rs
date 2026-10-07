# `MapMerge` audit — ShiftMap

C source: reference/starlink-ast/src/shiftmap.c
Rust: crates/ast-core/src/mappings/shift.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- shiftmap.c:653 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's whole body is a conversion to WinMap and back, so every merge rule a
ShiftMap participates in lives in `winmap.c`'s `MapMerge` and is audited in
`winmap.md`.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned index to -1 | shiftmap.c:665-666 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | shiftmap.c:668-669 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | A ShiftMap is equivalent to a WinMap with unit scaling; create a WinMap with undefined corners | shiftmap.c:671-678 (fn MapMerge) | shift.rs:189-204 (fn map_merge) | architecture | ported |
| 4 | Store unit scale terms and the shift terms, negating them when the ShiftMap is applied inverted | shiftmap.c:680-694 (fn MapMerge) | shift.rs:195-200 (fn map_merge) | architecture | ported |
| 5 | Replace the nominated ShiftMap in the list with the WinMap, recording the old invert flag, and clear that flag | shiftmap.c:695-701 (fn MapMerge) | shift.rs:190 (fn map_merge),207 (fn map_merge) | architecture | ported |
| 6 | Delegate to WinMap's MapMerge, recording the list length first | shiftmap.c:703-707 (fn MapMerge) | shift.rs:206-209 (fn map_merge) | architecture | ported |
| 7 | Detect the round trip where the merge merely turned the WinMap back into an equivalent lone ShiftMap | shiftmap.c:709-719 (fn MapMerge) | shift.rs:211-221 (fn map_merge) | architecture | ported |
| 8 | Also revert when the merge reported no change at all | shiftmap.c:720-722 (fn MapMerge) | shift.rs:223 (fn map_merge) | architecture | ported |
| 9 | Revert an inverted ShiftMap with astCopy plus negated shift terms, and report a change | shiftmap.c:724-732 (fn MapMerge) | shift.rs:234-238 (fn map_merge) | architecture | ported |
| 10 | Revert a forward ShiftMap with astClone, and report no change | shiftmap.c:733-739 (fn MapMerge) | shift.rs:234-238 (fn map_merge) | architecture | ported |
| 11 | Leave the reverted ShiftMap to be used in a forward sense | shiftmap.c:740-742 (fn MapMerge) | shift.rs:236-237 (fn map_merge) | architecture | ported |
| 12 | Return the result | shiftmap.c:746-747 (fn MapMerge) | shift.rs:241 (fn map_merge) | ceremony | ported |

## The lone ShiftMap has no reduction of its own

C gives ShiftMap no `Simplify` vtab slot, so a ShiftMap nominated on its own
reaches the same body as one in a run: it becomes a WinMap and `winmap.c`'s
`MapMerge` decides. Both reductions a lone ShiftMap can undergo come from there.
An effectively inverted one is rebuilt forward with negated shift terms by the
revert path (rows 9 and 11, `shift.rs:223-239 (fn map_merge)`). An all-zero one never comes
back at all: `win_map_merge`'s replace-with-simpler step sees every shift zero
and yields a diagonal `MatrixMap`, which reduces on to a `UnitMap`
(`winmap_merge.rs:381-395 (fn win_map_merge)`, C `winmap.c:1827 (fn MapMerge)`).

## Notes on rows 9 and 10

C reverts with `astClone` or `astCopy`, and both carry the object's `flags`.
The Rust revert clones `self`, which carries the same record, so a ShiftMap that
reached the list already recorded as simplified stays recorded across the WinMap
round trip.
Building a fresh `ShiftMap` here would drop the record and suppress the `IsSimp`
card C writes for it.

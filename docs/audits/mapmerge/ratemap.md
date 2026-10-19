# `MapMerge` audit — RateMap

C source: reference/starlink-ast/src/ratemap.c
Rust: crates/ast-core/src/mappings/ratemap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- ratemap.c:678 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- ratemap.c:700 (fn MapMerge) — initialisation to silence a compiler warning; the Rust locals are initialised where they are declared

C's body runs `ratemap.c:678-817 (fn MapMerge)`. Rows cover its executable
comment-delimited steps; the declaration blocks at 678-693 and 700-701 are not
steps.

`RateMap::map_merge` (`ratemap.rs:309-319 (fn map_merge)`) holds the reduction of the
encapsulated Mapping, as C's body does, and hands the neighbour cancellation to
`RateMap::try_merge` through `self_reduce_map_merge`.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned index to -1 | ratemap.c:694-695 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | ratemap.c:697-698 (fn MapMerge) | -- | architecture | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to this RateMap | ratemap.c:703-704 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self`; C is downcasting from `AstMapping *` |
| 4 | Temporarily set the RateMap's Invert flag to the value the list requests | ratemap.c:706-708 (fn MapMerge) | -- | architecture | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 5 | Get the encapsulated Mapping and temporarily restore the Invert value it had at construction | ratemap.c:710-715 (fn MapMerge) | -- | architecture | n/a: `self.map` is owned and holds its construction orientation already, so there is nothing to save and restore |
| 6 | First try to simplify the RateMap by simplifying its encapsulated Mapping | ratemap.c:717-719 (fn MapMerge) | ratemap.rs:159-168 (fn reduced_child) | architecture | ported |
| 7 | If that simplified anything, build a new RateMap around the simplified Mapping and report a change | ratemap.c:721-727 (fn MapMerge) | ratemap.rs:170-180 (fn reduced_child) | architecture | ported |
| 8 | Otherwise, in series only, try to cancel this RateMap with its own inverse | ratemap.c:728-730 (fn MapMerge) | ratemap.rs:328-331 (fn try_merge) | architecture | ported |
| 9 | Start with nothing to cancel against | ratemap.c:732-733 (fn MapMerge) | -- | architecture | n/a: the port's pairwise hook is asked about one neighbour at a time, so there is no candidate index to initialise |
| 10 | Consider the lower neighbour, if it is a RateMap | ratemap.c:735-737 (fn MapMerge) | ratemap.rs:334 (fn try_merge) | architecture | ported |
| 11 | Require the two Invert flags to be opposite | ratemap.c:738-741 (fn MapMerge) | ratemap.rs:336-339 (fn try_merge) | architecture | ported |
| 12 | Require the input and output axis indices to be equal | ratemap.c:743-745 (fn MapMerge) | ratemap.rs:341-344 (fn try_merge) | architecture | ported |
| 13 | Require the encapsulated Mappings to be equal, comparing them at their construction orientations | ratemap.c:747-762 (fn MapMerge) | ratemap.rs:346-350 (fn try_merge) | architecture | ported |
| 14 | Likewise consider the upper neighbour | ratemap.c:765-789 (fn MapMerge) | ratemap.rs:334 (fn try_merge) | architecture | ported |
| 15 | On a cancel, replace both list entries with a UnitMap and report the lower index | ratemap.c:791-802 (fn MapMerge) | ratemap.rs:410-412 (fn try_merge) | architecture | ported |
| 16 | Free resources | ratemap.c:803-805 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 17 | Re-instate the encapsulated Mapping's original Invert attribute | ratemap.c:806-808 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 18 | Re-instate the RateMap's original Invert attribute | ratemap.c:809-811 (fn MapMerge) | -- | ceremony | n/a: nothing was mutated, so nothing is restored |
| 19 | Clear the result value if an error occurred | ratemap.c:812-814 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 20 | Return the result | ratemap.c:815-816 (fn MapMerge) | ratemap.rs:318 (fn map_merge) | ceremony | ported |

## Rows 6 and 7 — the reduction, and the orientation it runs at

The reduction runs on a RateMap nominated from within a list, which is where C
runs it, and it runs inside `map_merge` rather than behind `simplify()`. The
reconstruction was already right: C's `astRateMap( smap, map->iout, map->iin, … )`
maps `ax1` to `iout` and `ax2` to `iin` (`ratemap.c:1830-1831 (fn astInitRateMap_)`), and the Rust
`ax1` is the output axis and `ax2` the input.

`reduced_child()` orients the encapsulated Mapping to `invert_snapshot` before
reducing it and restores the stored orientation afterwards, which is C's
`astSetInvert( emap, map->invert )` … `astSetInvert( emap, old_inv )` pair
(`ratemap.c:710-715 (fn MapMerge)`, `ratemap.c:806-808 (fn MapMerge)`). The snapshot is C's `map->invert`, the value
the child held at construction and what the `Inv` card carries — not whatever the
child stores now. Tracing C on `cap_cmpmap_01` shows the difference directly: the
encapsulated CmpMap dumps `Invert = 1` and C enters `astSimplify` on it with
`inv=0`.

## Row 15 — the same cancellation, a different result shape

C rewrites two list slots, putting a `UnitMap(nax)` in each, and picks `nax`
from `astGetNout` of the nominated RateMap for the lower neighbour and
`astGetNin` for the upper. The port's `try_merge` returns one Mapping that
replaces the pair, and derives its width from the merge direction instead. Both
reduce to the same Mapping once the surrounding CmpMap drops UnitMaps, but no
corpus fixture puts two RateMaps next to each other, so neither form is
exercised.

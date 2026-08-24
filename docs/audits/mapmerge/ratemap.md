# `MapMerge` audit — RateMap

C source: reference/starlink-ast/src/ratemap.c
Rust: crates/ast-core/src/mappings/ratemap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `ratemap.c:678-817`. Rows cover its executable
comment-delimited steps; the declaration blocks at 678-693 and 700-701 are not
steps.

`RateMap::map_merge` (`ratemap.rs:304-314`) holds the reduction of the
encapsulated Mapping, as C's body does, and hands the neighbour cancellation to
`RateMap::try_merge` through `self_reduce_map_merge`.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned index to -1 | ratemap.c:694-695 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | ratemap.c:697-698 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to this RateMap | ratemap.c:703-704 | -- | n/a: the Rust method already has `&self`; C is downcasting from `AstMapping *` |
| 4 | Temporarily set the RateMap's Invert flag to the value the list requests | ratemap.c:706-708 | -- | n/a: the requested orientation is a separate value in `MergeCtx`, so the object is never mutated to carry it |
| 5 | Get the encapsulated Mapping and temporarily restore the Invert value it had at construction | ratemap.c:710-715 | -- | n/a: `self.map` is owned and holds its construction orientation already, so there is nothing to save and restore |
| 6 | First try to simplify the RateMap by simplifying its encapsulated Mapping | ratemap.c:717-719 | ratemap.rs:152-162 | ported |
| 7 | If that simplified anything, build a new RateMap around the simplified Mapping and report a change | ratemap.c:721-727 | ratemap.rs:164-174 | ported |
| 8 | Otherwise, in series only, try to cancel this RateMap with its own inverse | ratemap.c:728-730 | ratemap.rs:323-326 | ported |
| 9 | Start with nothing to cancel against | ratemap.c:732-733 | -- | n/a: the port's pairwise hook is asked about one neighbour at a time, so there is no candidate index to initialise |
| 10 | Consider the lower neighbour, if it is a RateMap | ratemap.c:735-737 | ratemap.rs:329 | ported |
| 11 | Require the two Invert flags to be opposite | ratemap.c:738-741 | ratemap.rs:331-334 | ported |
| 12 | Require the input and output axis indices to be equal | ratemap.c:743-745 | ratemap.rs:336-339 | ported |
| 13 | Require the encapsulated Mappings to be equal, comparing them at their construction orientations | ratemap.c:747-762 | ratemap.rs:341-345 | ported |
| 14 | Likewise consider the upper neighbour | ratemap.c:765-789 | ratemap.rs:329 | ported |
| 15 | On a cancel, replace both list entries with a UnitMap and report the lower index | ratemap.c:791-802 | ratemap.rs:405-407 | ported |
| 16 | Free resources | ratemap.c:803-805 | -- | n/a: owned values are dropped |
| 17 | Re-instate the encapsulated Mapping's original Invert attribute | ratemap.c:806-808 | -- | n/a: nothing was mutated, so nothing is restored |
| 18 | Re-instate the RateMap's original Invert attribute | ratemap.c:809-811 | -- | n/a: nothing was mutated, so nothing is restored |
| 19 | Clear the result value if an error occurred | ratemap.c:812-814 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 20 | Return the result | ratemap.c:815-816 | ratemap.rs:313 | ported |

## Rows 6 and 7 — the reduction, and the orientation it runs at

The reduction runs on a RateMap nominated from within a list, which is where C
runs it, and it runs inside `map_merge` rather than behind `simplify()`. The
reconstruction was already right: C's `astRateMap( smap, map->iout, map->iin, … )`
maps `ax1` to `iout` and `ax2` to `iin` (`ratemap.c:1830-1831`), and the Rust
`ax1` is the output axis and `ax2` the input.

`reduced_child()` orients the encapsulated Mapping to `invert_snapshot` before
reducing it and restores the stored orientation afterwards, which is C's
`astSetInvert( emap, map->invert )` … `astSetInvert( emap, old_inv )` pair
(`ratemap.c:710-715`, `:806-808`). The snapshot is C's `map->invert`, the value
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

# `MapMerge` audit — ZoomMap

C source: reference/starlink-ast/src/zoommap.c
Rust: crates/ast-core/src/mappings/zoom.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- zoommap.c:703 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- zoommap.c:735 (fn MapMerge) — clears C's own sentinels, a null replacement Mapping and the elimination count and simplified flag; the Rust locals are initialised where they are declared
- zoommap.c:986 (fn MapMerge) — the error path annuls the replacement C allocated; a Rust replacement is an owned value dropped on the error return

C's body runs `zoommap.c:729-1167 (fn MapMerge)` in three parts: condense a run of ZoomMaps
and UnitMaps in series; the same in parallel; and, when neither condensed
anything, absorb the ZoomMap into an adjacent MatrixMap, WinMap or ShiftMap.

The Rust `ZoomMap::map_merge` (`zoom.rs:166-173 (fn map_merge)`) delegates to
`simplify::zoom_map_merge` (`mappings/simplify.rs:839-998 (fn zoom_map_merge)`), so Rust sites below are in
`mappings/simplify.rs` unless stated.

The last row is not part of `MapMerge` but of the rule that governs it: an
attribute setter whose value feeds the first part of the class's own `MapMerge`
must discard the simplified record.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | zoommap.c:729-730 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | zoommap.c:732-733 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | In series: obtain the effective zoom factor of the nominated Mapping | zoommap.c:740-749 (fn MapMerge) | mappings/simplify.rs:865-869 (fn zoom_map_merge) | architecture | ported |
| 4 | Search adjacent lower-numbered Mappings, accumulating the zoom factors of ZoomMaps and treating UnitMaps as unity | zoommap.c:750-777 (fn MapMerge) | mappings/simplify.rs:871-882 (fn zoom_map_merge) | architecture | ported |
| 5 | Search adjacent higher-numbered Mappings the same way | zoommap.c:778-797 (fn MapMerge) | mappings/simplify.rs:884-895 (fn zoom_map_merge) | architecture | ported |
| 6 | Count how many Mappings condensing the run would eliminate | zoommap.c:798-801 (fn MapMerge) | mappings/simplify.rs:897 (fn zoom_map_merge) | architecture | ported |
| 7 | Decide whether the replacement can be a UnitMap | zoommap.c:802-806 (fn MapMerge) | mappings/simplify.rs:898 (fn zoom_map_merge) | architecture | ported |
| 8 | Decide whether simplification is possible at all | zoommap.c:807-816 (fn MapMerge) | mappings/simplify.rs:899 (fn zoom_map_merge) | architecture | ported |
| 9 | Obtain the input and output counts for the replacement Mapping | zoommap.c:817-826 (fn MapMerge) | mappings/simplify.rs:920-924 (fn zoom_map_merge) | architecture | ported |
| 10 | In parallel: obtain the nominated Mapping's coordinate counts | zoommap.c:827-834 (fn MapMerge) | mappings/simplify.rs:962-964 (fn zoom_map_merge) | architecture | ported |
| 11 | Search both directions for adjacent ZoomMaps and UnitMaps, accumulating coordinate counts | zoommap.c:835-868 (fn MapMerge) | mappings/simplify.rs:935-950 (fn zoom_map_merge) | architecture | ported |
| 12 | Build a per-axis zoom factor array, unity for each UnitMap axis | zoommap.c:869-910 (fn MapMerge) | mappings/simplify.rs:952-967 (fn zoom_map_merge) | architecture | ported |
| 13 | Note the minimum and maximum zoom factors encountered | zoommap.c:905-910 (fn MapMerge) | mappings/simplify.rs:968-973 (fn zoom_map_merge) | architecture | ported |
| 14 | Decide whether the replacement can be a single ZoomMap, a UnitMap, or a diagonal MatrixMap | zoommap.c:911-933 (fn MapMerge) | mappings/simplify.rs:976-979 (fn zoom_map_merge) | architecture | ported |
| 15 | Create that replacement | zoommap.c:934-948 (fn MapMerge) | mappings/simplify.rs:985-995 (fn zoom_map_merge) | architecture | ported |
| 16 | Free the zoom factor array | zoommap.c:949-952 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 17 | Replace the condensed run with the new Mapping, close the gap, and return the first modified index | zoommap.c:953-985 (fn MapMerge) | mappings/simplify.rs:996-997 (fn zoom_map_merge) | architecture | ported |
| 18 | If nothing condensed, and the ZoomMap is not first, try merging it with the previous Mapping | zoommap.c:992-1002 (fn MapMerge) | mappings/simplify.rs:909-917 (fn zoom_map_merge) | architecture | ported |
| 19 | Absorb into a previous MatrixMap by scaling its elements | zoommap.c:1003-1018 (fn MapMerge) | matrix.rs:734-751 (fn try_merge) | architecture | ported |
| 20 | Absorb into a previous WinMap by scaling its coefficients | zoommap.c:1019-1035 (fn MapMerge) | zoom.rs:278-292 (fn try_merge) | architecture | ported |
| 21 | Absorb into a previous ShiftMap, producing a WinMap | zoommap.c:1036-1064 (fn MapMerge) | zoom.rs:278-292 (fn try_merge) | architecture | ported |
| 22 | If the previous Mapping absorbed nothing, and the ZoomMap is not last, consider the next Mapping | zoommap.c:1066-1071 (fn MapMerge) | mappings/simplify.rs:917 (fn zoom_map_merge) | architecture | ported |
| 23 | Absorb into a following MatrixMap by scaling its elements | zoommap.c:1072-1085 (fn MapMerge) | matrix.rs:734-751 (fn try_merge) | architecture | ported |
| 24 | Absorb into a following WinMap by scaling its coefficients | zoommap.c:1086-1101 (fn MapMerge) | zoom.rs:278-292 (fn try_merge) | architecture | ported |
| 25 | Absorb into a following ShiftMap, producing a WinMap | zoommap.c:1102-1128 (fn MapMerge) | zoom.rs:278-292 (fn try_merge) | architecture | ported |
| 26 | If anything absorbed, annul the neighbour and the ZoomMap, and store the new Mapping over the neighbour with its Invert flag | zoommap.c:1130-1143 (fn MapMerge) | mappings/simplify.rs:1445-1451 (fn default_map_merge) | architecture | ported |
| 27 | Shuffle the later Mappings down, clear the vacated element, decrement the count and return the lower of the two indices | zoommap.c:1145-1159 (fn MapMerge) | mappings/simplify.rs:1451-1453 (fn default_map_merge) | architecture | ported |
| 28 | Clear the returned result if an error occurred | zoommap.c:1163-1164 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 29 | Return the result | zoommap.c:1166-1167 (fn MapMerge) | mappings/simplify.rs:913-917 (fn zoom_map_merge) | architecture | ported |
| 30 | Setting or clearing Zoom clears the simplified record, both orientations | zoommap.c:1698 (fn astMAKE_CLEAR1),1704 (fn astMAKE_SET1) | zoom.rs:72-84 (fn set_zoom) | ceremony | ported |

## Rows 7 and 8 — the lone ZoomMap is a run of one

`simpler = ngone > 0 || unit || where_inv` is what a ZoomMap nominated on its own
reduces by: a unit effective factor becomes a `UnitMap`, and an effectively
inverted one is rebuilt forward with the reciprocal factor. That is the whole of
C's replace-with-simpler half, and it is reached with `nmap == 1` because C's base
`Simplify` nominates the Mapping in a list of one (`mapping.c:16871-16883 (fn Simplify)`). The
`where_inv` term is the list's flag, not the object's, since `flatten` resets
every leaf to forward.

## Rows 18 to 25 — the ordering is deliberate

C tries the previous neighbour before the next one, and the port keeps that
order at `mappings/simplify.rs:909-917 (fn zoom_map_merge)`. It is observable when only one side is eligible,
for instance under a restricted simplify where the trailing Mapping is not
flagged. The pairwise fusion itself goes through `default_map_merge`, which
offers the pair to the neighbour first and to the ZoomMap second. That is why
rows 19 and 23 land in `matrix.rs`: scaling a *general* matrix by the zoom factor
is `MatrixMap::try_merge`'s MatZoom branch, which keeps an inverted MatrixMap
inverted and scales its stored values. Rows 20, 21, 24 and 25 land in
`ZoomMap::try_merge`'s cross-type branch, where the ZoomMap and a WinMap or
ShiftMap are both reduced to per-axis `(shift, scale)` terms and composed.

C writes each of the three absorbing branches twice, once against the previous
neighbour and once against the next, because it reaches into the neighbour's
representation itself. The port reaches the same six cases through one pairwise
`try_merge` per neighbour, so rows 19 to 21 and rows 23 to 25 land on the same
Rust sites; the side is the argument `default_map_merge` is called with.

## Row 30 — the convention, not just this class

`astMAKE_SET1` and `astMAKE_CLEAR1` for `Zoom` both call `astClearIsSimple`,
whose defining comment in `mapping.h` states the rule: a setter whose value feeds
the first part of the class's own `MapMerge` must discard the record. `Zoom` is
the whole of the transformation, so it feeds the run-condensing half of
`zoom_map_merge` directly.

`ZoomMap::set_zoom` clears both simplified records before it validates the new
factor, which is the order of C's comma expression — `astClearIsSimple` is its
first term, so a rejected zero still discards the record. There is no
`clear_zoom`: a constructed `ZoomMap` always carries a factor, so the attribute
has no unset state to return to.

See PLAN.md §9.2 for the whole convention and the classes it covers.

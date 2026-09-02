# `MapMerge` audit — ZoomMap

C source: reference/starlink-ast/src/zoommap.c
Rust: crates/ast-core/src/mappings/zoom.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `zoommap.c:729-1122` in three parts: condense a run of ZoomMaps
and UnitMaps in series; the same in parallel; and, when neither condensed
anything, absorb the ZoomMap into an adjacent MatrixMap, WinMap or ShiftMap.

The Rust `ZoomMap::map_merge` (`zoom.rs:124-131`) delegates to
`simplify::zoom_map_merge` (`simplify.rs:955-1118`), so Rust sites below are in
`simplify.rs` unless stated.

The last row is not part of `MapMerge` but of the rule that governs it: an
attribute setter whose value feeds the first part of the class's own `MapMerge`
must discard the simplified record.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | zoommap.c:729-730 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | zoommap.c:732-733 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | In series: obtain the effective zoom factor of the nominated Mapping | zoommap.c:740-749 | simplify.rs:985-989 | ported |
| 4 | Search adjacent lower-numbered Mappings, accumulating the zoom factors of ZoomMaps and treating UnitMaps as unity | zoommap.c:750-777 | simplify.rs:992-1002 | ported |
| 5 | Search adjacent higher-numbered Mappings the same way | zoommap.c:778-797 | simplify.rs:1005-1015 | ported |
| 6 | Count how many Mappings condensing the run would eliminate | zoommap.c:798-801 | simplify.rs:1017 | ported |
| 7 | Decide whether the replacement can be a UnitMap | zoommap.c:802-806 | simplify.rs:1018 | ported |
| 8 | Decide whether simplification is possible at all | zoommap.c:807-816 | simplify.rs:1019 | ported |
| 9 | Obtain the input and output counts for the replacement Mapping | zoommap.c:817-826 | simplify.rs:1041-1044 | ported |
| 10 | In parallel: obtain the nominated Mapping's coordinate counts | zoommap.c:827-834 | simplify.rs:1081-1084 | ported |
| 11 | Search both directions for adjacent ZoomMaps and UnitMaps, accumulating coordinate counts | zoommap.c:835-868 | simplify.rs:1055-1070 | ported |
| 12 | Build a per-axis zoom factor array, unity for each UnitMap axis | zoommap.c:869-910 | simplify.rs:1072-1086 | ported |
| 13 | Note the minimum and maximum zoom factors encountered | zoommap.c:905-910 | simplify.rs:1088-1093 | ported |
| 14 | Decide whether the replacement can be a single ZoomMap, a UnitMap, or a diagonal MatrixMap | zoommap.c:911-933 | simplify.rs:1096-1115 | ported |
| 15 | Create that replacement | zoommap.c:934-948 | simplify.rs:1096-1115 | ported |
| 16 | Free the zoom factor array | zoommap.c:949-952 | -- | n/a: owned values are dropped |
| 17 | Replace the condensed run with the new Mapping, close the gap, and return the first modified index | zoommap.c:953-985 | simplify.rs:1050-1051 | ported |
| 18 | If nothing condensed, and the ZoomMap is not first, try merging it with the previous Mapping | zoommap.c:992-1002 | simplify.rs:1029-1037 | ported |
| 19 | Absorb into a previous MatrixMap by scaling its elements | zoommap.c:1003-1018 | zoom.rs:171-219 | ported |
| 20 | Absorb into a previous WinMap by scaling its coefficients | zoommap.c:1019-1035 | zoom.rs:171-219 | ported |
| 21 | Absorb into a previous ShiftMap, producing a WinMap | zoommap.c:1036-1122 | zoom.rs:171-219 | ported |
| 22 | Setting or clearing Zoom clears the simplified record, both orientations | zoommap.c:1698,1704 | -- | n/a: the Rust class has no Zoom setter; the factor is fixed at construction and `ZoomMap` exposes no way to change it |

## Rows 7 and 8 — the lone ZoomMap is a run of one

`simpler = ngone > 0 || unit || where_inv` is what a ZoomMap nominated on its own
reduces by: a unit effective factor becomes a `UnitMap`, and an effectively
inverted one is rebuilt forward with the reciprocal factor. That is the whole of
C's replace-with-simpler half, and it is reached with `nmap == 1` because C's base
`Simplify` nominates the Mapping in a list of one (`mapping.c:16871-16883`). The
`where_inv` term is the list's flag, not the object's, since `flatten` resets
every leaf to forward.

## Rows 18 to 21 — the ordering is deliberate

C tries the previous neighbour before the next one, and the port keeps that
order at `simplify.rs:1029-1037`. It is observable when only one side is eligible,
for instance under a restricted simplify where the trailing Mapping is not
flagged. The pairwise fusion itself is `ZoomMap::try_merge` reached through
`default_map_merge`, so rows 19 to 21 share one Rust site.

## Row 22 — the convention, not just this class

`astMAKE_SET1` and `astMAKE_CLEAR1` for `Zoom` both call `astClearIsSimple`, per
the rule at `mapping.h:449-457`: a setter whose value feeds the first part of the
class's own `MapMerge` must discard the record. C applies it in eleven classes.
The port applies it nowhere, and here the row is `n/a` rather than a gap only
because `ZoomMap` has no setter for the value at all. Adding one without the
clear would turn this row into a gap, which is the reason it is written down.
See PLAN.md §9.2 for the whole convention and the classes it covers.

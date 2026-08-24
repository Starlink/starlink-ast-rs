# `MapMerge` audit — GrismMap

C source: reference/starlink-ast/src/grismmap.c
Rust: crates/ast-core/src/mappings/grismmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's `MapMerge` body runs `grismmap.c:1198-1269` and is thin: it asks `CanMerge`
about the lower neighbour, then the upper, and splices in whatever comes back.
The rule itself lives in `CanMerge` (`grismmap.c:491-600`), so both are audited
here.

`GrismMap::map_merge` (`grismmap.rs:404-411`) delegates to
`simplify::grism_map_merge` (`simplify.rs:2870-2890`), whose per-pair body is
`grism_zoom_merge_at` (`simplify.rs:2798-2860`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | grismmap.c:1198-1199 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | grismmap.c:1201-1202 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | A GrismMap can only merge in series, so do nothing in parallel | grismmap.c:1209-1213 | simplify.rs:2876 | ported |
| 4 | Note that no mergeable neighbour has been found yet | grismmap.c:1214-1217 | -- | n/a: the Rust flow returns on the first success instead of carrying a found-flag |
| 5 | Ask `CanMerge` about the lower neighbour first | grismmap.c:1218-1225 | simplify.rs:2877-2881 | ported |
| 6 | If that failed, ask about the upper neighbour | grismmap.c:1226-1234 | simplify.rs:2882-2886 | ported |
| 7 | Replace the merged pair with the single Mapping `CanMerge` returned | grismmap.c:1235-1239 | simplify.rs:2798-2860 | ported |
| 8 | Annul the two Mappings | grismmap.c:1240-1243 | -- | n/a: owned values are dropped |
| 9 | Store the merged Mapping in place of the first of the pair | grismmap.c:1244-1248 | simplify.rs:2823,2853 | ported |
| 10 | Shuffle the remaining Mappings down and clear the vacated slot | grismmap.c:1249-1259 | simplify.rs:2824,2854 | ported |
| 11 | Decrement the Mapping count and return the index of the first modified element | grismmap.c:1260-1267 | simplify.rs:2880,2885 | ported |
| 12 | Return the result | grismmap.c:1268-1269 | simplify.rs:2889 | ported |
| 13 | `CanMerge`: if the first Mapping is a GrismMap | grismmap.c:491-494 | simplify.rs:2804 | ported |
| 14 | Two GrismMaps merge to a UnitMap when one is the inverse of the other | grismmap.c:495-499 | grismmap.rs:425-441 | ported |
| 15 | Require the two GrismMaps to have equal attribute values | grismmap.c:500-509 | grismmap.rs:429-436 | ported |
| 16 | Require them to be applied in opposite senses, then return a UnitMap | grismmap.c:510-514 | grismmap.rs:437-439 | ported |
| 17 | A GrismMap followed by a non-GrismMap merges only if the GrismMap is un-inverted, so its output is the wavelength | grismmap.c:515-523 | simplify.rs:2804 | ported |
| 18 | Note that the merged Mapping will not need inverting | grismmap.c:524-527 | simplify.rs:2823 | ported |
| 19 | Require the second Mapping to be a ZoomMap, and take its zoom factor at the right Invert setting | grismmap.c:528-538 | simplify.rs:2805-2818 | ported |
| 20 | A non-GrismMap followed by a GrismMap merges only if the GrismMap is inverted | grismmap.c:539-548 | simplify.rs:2831 | ported |
| 21 | Re-express that case as an un-inverted GrismMap followed by a ZoomMap, inverting the result at the end | grismmap.c:549-555 | simplify.rs:2838-2851 | ported |
| 22 | Require the first Mapping to be a ZoomMap at the opposite Invert setting, and take its factor | grismmap.c:556-566 | simplify.rs:2830-2848 | ported |
| 23 | Build the merged Mapping by folding the zoom factor into the GrismMap's attributes | grismmap.c:567-571 | grismmap.rs:234-264 | ported |
| 24 | Ensure a forward GrismMap before doing so | grismmap.c:572-575 | simplify.rs:2804,2831 | ported |
| 25 | Read the GrismMap attribute values the fold needs | grismmap.c:576-580 | grismmap.rs:247-250 | ported |
| 26 | Apply the zoom to the wavelength scale, producing the amended attributes | grismmap.c:581-593 | grismmap.rs:248-254 | ported |
| 27 | Invert the resulting GrismMap if the original pair required it | grismmap.c:594-598 | grismmap.rs:256-258 | ported |
| 28 | Return the merged Mapping, or NULL | grismmap.c:599-600 | simplify.rs:2859 | ported |

## A lone GrismMap has no reduction of its own

Every rule above needs a neighbour: two GrismMaps that cancel, or a GrismMap and
a ZoomMap that fold together. `grismmap.c` has no `Simplify` vtab slot, so a
GrismMap on its own is returned unchanged, and the base `Simplify` reaches this
`MapMerge` by nominating it in a list of one (`mapping.c:16871-16883`). The port
matches that by implementing `map_merge` and leaving `Mapping::simplified` at its
default, which means a lone GrismMap can never be stamped as simplified in
isolation.

## Why the GrismMap/ZoomMap merge is not in `try_merge`

`GrismMap::try_merge` handles only the GrismMap-with-its-own-inverse case. The
ZoomMap case lives in `grism_zoom_merge_at` because C's rule is
order-sensitive — an un-inverted GrismMap must come *before* the ZoomMap, and the
mirror case is re-expressed into that form — and `try_merge` is called without
telling the callee which side of the pair it is. The comment at
`grismmap.rs:444-447` records that.

## Rows 5 and 6 — the order is observable

C asks about the lower neighbour first and only then the upper. The port keeps
that order at `simplify.rs:2877-2886`, which matters when both neighbours are
eligible: the pair chosen decides the index reported and where the nominate loop
resumes.

## The clear-on-set convention

`grismmap.c:165,255` applies `astClearIsSimple` in macro form to every GrismMap
attribute setter, per `mapping.h:449-457`. The port implements the convention
nowhere; the GrismMap attributes it exposes are set at construction. See PLAN.md
§9.2.

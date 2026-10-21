# `MapMerge` audit — GrismMap

C source: reference/starlink-ast/src/grismmap.c
Rust: crates/ast-core/src/mappings/grismmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- grismmap.c:1191 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- grismmap.c:466 (fn CanMerge) — `Local Variables:` is the local-variable declaration block, not a step
- grismmap.c:423 (fn CanMerge) — the function prologue, not a step; a lone `*` line ahead of `Name:` distinguishes it from the prologue form the sweep recognises

C's `MapMerge` body runs `grismmap.c:1191-1269 (fn MapMerge)` and is thin: it asks
`CanMerge` about the lower neighbour, then the upper, and splices in whatever
comes back. The rule itself lives in `CanMerge`
(`grismmap.c:466-600 (fn CanMerge)`), so both are audited here.

`GrismMap::map_merge` (`grismmap.rs:861-868 (fn map_merge)`) delegates to
`simplify::grism_map_merge` (`mappings/simplify.rs:2871-2889 (fn grism_map_merge)`),
whose per-pair body is
`grism_zoom_merge_at` (`mappings/simplify.rs:2785-2845 (fn grism_zoom_merge_at)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | grismmap.c:1198-1199 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | grismmap.c:1201-1202 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Initialise the pair indices to silence compiler warnings | grismmap.c:1204-1207 (fn MapMerge) | -- | ceremony | n/a: the Rust locals are initialised where they are declared |
| 4 | A GrismMap can only merge in series, so do nothing in parallel | grismmap.c:1209-1213 (fn MapMerge) | mappings/simplify.rs:2877 (fn grism_map_merge) | architecture | ported |
| 5 | Note that no mergeable neighbour has been found yet | grismmap.c:1214-1217 (fn MapMerge) | -- | ceremony | n/a: the Rust flow returns on the first success instead of carrying a found-flag |
| 6 | Ask `CanMerge` about the lower neighbour first | grismmap.c:1218-1225 (fn MapMerge) | mappings/simplify.rs:2878-2881 (fn grism_map_merge) | architecture | ported |
| 7 | If that failed, ask about the upper neighbour | grismmap.c:1226-1234 (fn MapMerge) | mappings/simplify.rs:2882-2885 (fn grism_map_merge) | architecture | ported |
| 8 | Replace the merged pair with the single Mapping `CanMerge` returned | grismmap.c:1235-1239 (fn MapMerge) | mappings/simplify.rs:2785-2845 (fn grism_zoom_merge_at) | architecture | ported |
| 9 | Annul the two Mappings | grismmap.c:1240-1243 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 10 | Store the merged Mapping in place of the first of the pair | grismmap.c:1244-1248 (fn MapMerge) | mappings/simplify.rs:2859 (fn store_merged), via mappings/simplify.rs:2810 (fn grism_zoom_merge_at),2836 (fn grism_zoom_merge_at) | architecture | ported |
| 11 | Shuffle the remaining Mappings down and clear the vacated slot | grismmap.c:1249-1259 (fn MapMerge) | mappings/simplify.rs:2860 (fn store_merged), via mappings/simplify.rs:2810 (fn grism_zoom_merge_at),2836 (fn grism_zoom_merge_at) | architecture | ported |
| 12 | Decrement the Mapping count and return the index of the first modified element | grismmap.c:1260-1267 (fn MapMerge) | mappings/simplify.rs:2880 (fn grism_map_merge),2882 (fn grism_map_merge) | architecture | ported |
| 13 | Return the result | grismmap.c:1268-1269 (fn MapMerge) | mappings/simplify.rs:2888 (fn grism_map_merge) | ceremony | ported |
| 14 | `CanMerge`: initialise the returned Mapping to NULL | grismmap.c:476-478 (fn CanMerge) | -- | ceremony | n/a: "no merge" is `false` from `grism_zoom_merge_at`, not a null pointer |
| 15 | Check the global error status and bail out | grismmap.c:479-480 (fn CanMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 16 | Initialise the GrismMap pointer and the invert flag to silence compiler warnings | grismmap.c:482-485 (fn CanMerge) | -- | ceremony | n/a: the Rust locals are initialised where they are declared |
| 17 | Initialise the adjacent ZoomMap's factor to mean "none found" | grismmap.c:487-489 (fn CanMerge) | -- | ceremony | n/a: each pattern reads its own zoom factor inside the branch that matched, so there is no sentinel spanning the branches |
| 18 | If the first Mapping is a GrismMap | grismmap.c:491-494 (fn CanMerge) | mappings/simplify.rs:2791 (fn grism_zoom_merge_at), grismmap.rs:882 (fn try_merge) | architecture | ported |
| 19 | Two GrismMaps merge to a UnitMap when one is the inverse of the other | grismmap.c:495-499 (fn CanMerge) | grismmap.rs:882 (fn try_merge) | architecture | ported |
| 20 | Require the two GrismMaps to have equal attribute values | grismmap.c:500-509 (fn CanMerge) | grismmap.rs:886-893 (fn try_merge) | architecture | ported |
| 21 | Require them to be applied in opposite senses, then return a UnitMap | grismmap.c:510-514 (fn CanMerge) | grismmap.rs:894-896 (fn try_merge) | architecture | ported |
| 22 | A GrismMap followed by a non-GrismMap merges only if the GrismMap is un-inverted, so its output is the wavelength | grismmap.c:515-523 (fn CanMerge) | mappings/simplify.rs:2791-2794 (fn grism_zoom_merge_at) | architecture | ported |
| 23 | Note that the merged Mapping will not need inverting | grismmap.c:524-527 (fn CanMerge) | mappings/simplify.rs:2808 (fn grism_zoom_merge_at) | architecture | ported |
| 24 | Require the second Mapping to be a ZoomMap, and take its zoom factor at the right Invert setting | grismmap.c:528-538 (fn CanMerge) | mappings/simplify.rs:2792-2805 (fn grism_zoom_merge_at) | architecture | ported |
| 25 | A non-GrismMap followed by a GrismMap merges only if the GrismMap is inverted | grismmap.c:539-548 (fn CanMerge) | mappings/simplify.rs:2816-2819 (fn grism_zoom_merge_at) | architecture | ported |
| 26 | Re-express that case as an un-inverted GrismMap followed by a ZoomMap, inverting the result at the end | grismmap.c:549-555 (fn CanMerge) | mappings/simplify.rs:2837 (fn grism_zoom_merge_at) | architecture | ported |
| 27 | Require the first Mapping to be a ZoomMap at the opposite Invert setting, and take its factor | grismmap.c:556-566 (fn CanMerge) | mappings/simplify.rs:2820-2834 (fn grism_zoom_merge_at) | architecture | ported |
| 28 | Build the merged Mapping by folding the zoom factor into the GrismMap's attributes | grismmap.c:567-571 (fn CanMerge) | grismmap.rs:678-710 (fn merge_with_zoom), via mappings/simplify.rs:2808 (fn grism_zoom_merge_at) | architecture | ported |
| 29 | Ensure a forward GrismMap before doing so | grismmap.c:572-575 (fn CanMerge) | grismmap.rs:691 (fn merge_with_zoom), via mappings/simplify.rs:2791 (fn grism_zoom_merge_at) | architecture | ported |
| 30 | Read the GrismMap attribute values the fold needs | grismmap.c:576-580 (fn CanMerge) | grismmap.rs:692-694 (fn merge_with_zoom), via mappings/simplify.rs:2808 (fn grism_zoom_merge_at) | architecture | ported |
| 31 | Apply the zoom to the wavelength scale, producing the amended attributes | grismmap.c:581-593 (fn CanMerge) | grismmap.rs:692-698 (fn merge_with_zoom), via mappings/simplify.rs:2808 (fn grism_zoom_merge_at) | architecture | ported |
| 32 | Invert the resulting GrismMap if the original pair required it | grismmap.c:594-598 (fn CanMerge) | grismmap.rs:700-702 (fn merge_with_zoom), via mappings/simplify.rs:2837 (fn grism_zoom_merge_at) | architecture | ported |
| 33 | Return the merged Mapping, or NULL | grismmap.c:599-600 (fn CanMerge) | mappings/simplify.rs:2811 (fn grism_zoom_merge_at),2837 (fn grism_zoom_merge_at) | architecture | ported |

## A lone GrismMap has no reduction of its own

Every rule above needs a neighbour: two GrismMaps that cancel, or a GrismMap and
a ZoomMap that fold together. `grismmap.c` has no `Simplify` vtab slot, so a
GrismMap on its own is returned unchanged, and the base `Simplify` reaches this
`MapMerge` by nominating it in a list of one
(`mapping.c:16871-16883 (fn Simplify)`). The port matches that by implementing
`map_merge` and leaving `Mapping::simplified` at its default, which means a lone
GrismMap can never be stamped as simplified in isolation.

## Why the GrismMap/ZoomMap merge is not in `try_merge`

`GrismMap::try_merge` handles only the GrismMap-with-its-own-inverse case. The
ZoomMap case lives in `grism_zoom_merge_at` because C's rule is
order-sensitive — an un-inverted GrismMap must come *before* the ZoomMap, and the
mirror case is re-expressed into that form — and `try_merge` is called without
telling the callee which side of the pair it is. The comment at
`grismmap.rs:901-904 (fn try_merge)` records that.

## Rows 6 and 7 — the order is observable

C asks about the lower neighbour first and only then the upper. The port keeps
that order at `mappings/simplify.rs:2878-2885 (fn grism_map_merge)`, which
matters when both neighbours are eligible: the pair chosen decides the index
reported and where the nominate loop resumes.

## Rows 22, 25 and 29 — which flag says "un-inverted"

C reads the pair's applied directions, `inv1` and `inv2`, to decide which
Mapping is the GrismMap producing wavelength, and then forces the copy forward
with `astSetInvert( ret, 0 )` because the object it copied may itself be stored
inverted. The port reads the list flags the same way — `!list[i].1` for pattern 1
and `list[i + 1].1` for pattern 2 — but has no forcing step to match row 29,
because `flatten` leaves every leaf stored forward and carries the applied
direction in the flag beside it. `merge_with_zoom`'s `self.clone()` is therefore
already the forward transformation C constructs. That invariant is what makes the
two readings equivalent; a GrismMap reached without going through `flatten` would
need the forcing.

## Rows 28 to 32 — the fold, and the record it discards

C copies the GrismMap and re-sets `GrismG`, `GrismNRP` and `GrismWaveR` through
their setters, so each assignment runs `astClearIsSimple` and the merged GrismMap
starts with both simplified records empty. `merge_with_zoom` writes the three
fields directly and then clears the record explicitly
(`grismmap.rs:707 (fn merge_with_zoom)`), reaching the same state; it also marks
the three attributes as explicitly set, which is what C's setters do and what
decides whether they are dumped.

## The clear-on-set convention

`grismmap.c`'s `MAKE_CLEAR` and `MAKE_SET` macros put `astClearIsSimple` in every
generated GrismMap attribute clearer and setter, following the rule stated in the
macro's own defining comment in `mapping.h`: a setter whose value feeds the first
part of the class's `MapMerge` must discard the record. Every GrismMap attribute
does feed it, through `CanMerge`'s attribute comparison and the wavelength fold.

The port implements the convention in all eight setters — `set_grism_nr`
(`grismmap.rs:463-488 (fn set_grism_nr)`) is the shape the other seven repeat —
and exposes no clearers, because a constructed GrismMap always carries a value
for each attribute and has no unset state to return to. See PLAN.md §9.2 for the
whole convention and the classes it covers.

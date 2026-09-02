# `MapMerge` audit — SwitchMap

C source: reference/starlink-ast/src/switchmap.c
Rust: crates/ast-core/src/mappings/switchmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `switchmap.c:955-1125` in two parts: cancel the SwitchMap against
an adjacent equal SwitchMap used the other way; and, only if that did nothing,
simplify the SwitchMap on its own. The second part is itself an either/or — an
inverted SwitchMap is normalised to a forward one and nothing else happens on
that pass, and only a forward one has its encapsulated Mappings simplified.

`SwitchMap::map_merge` (`switchmap.rs:628-672`) holds both parts in C's order:
`default_map_merge` for the neighbour cancellation, then the either/or, whose
second arm calls `SwitchMap::reduced_children` (`switchmap.rs:344-385`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | switchmap.c:955-956 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | switchmap.c:958-959 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this SwitchMap and the route Mapping count | switchmap.c:961-964 | -- | n/a: the Rust method already has `&self` |
| 4 | Temporarily restore every encapsulated Mapping's Invert flag to its construction value | switchmap.c:965-991 | switchmap.rs:27-50, switchmap.rs:355-365 | ported |
| 5 | Merge with a neighbouring SwitchMap, in series only | switchmap.c:992-996 | switchmap.rs:639-641 | ported |
| 6 | Check the higher neighbour first, noting the lower index of the pair | switchmap.c:997-1003 | simplify.rs:1151-1246 | ported |
| 7 | Failing that, check the lower neighbour the same way | switchmap.c:1004-1014 | simplify.rs:1151-1246 | ported |
| 8 | Cancel the pair only if the two SwitchMaps are equal with opposite invert flags | switchmap.c:1015-1023 | switchmap.rs:674-720 | ported |
| 9 | Replace the first of the pair with a UnitMap and set its invert flag | switchmap.c:1024-1031 | switchmap.rs:674-720 | ported |
| 10 | Annul the second and shuffle the rest down, clearing the vacated slot | switchmap.c:1032-1043 | simplify.rs:1151-1246 | ported |
| 11 | Decrement the Mapping count and return the index of the first modified element | switchmap.c:1044-1051 | simplify.rs:1151-1246 | ported |
| 12 | Attempt to simplify the SwitchMap on its own, only if no change was made above | switchmap.c:1052-1056 | switchmap.rs:639-641 | ported |
| 13 | If the SwitchMap is inverted, replace it with an equal forward one by swapping the selectors and inverting the route Mappings, and stop there | switchmap.c:1057-1076 | switchmap.rs:652-660, switchmap.rs:234-254 | ported |
| 14 | Otherwise try to simplify each encapsulated Mapping, noting whether anything changed | switchmap.c:1077-1091 | switchmap.rs:344-365 | ported |
| 15 | If anything simplified, build a new SwitchMap from the simplified Mappings | switchmap.c:1092-1100 | switchmap.rs:367-384 | ported |
| 16 | Release resources | switchmap.c:1101-1110 | -- | n/a: owned values are dropped |
| 17 | Re-instate the encapsulated Mappings' original Invert values | switchmap.c:1111-1120 | switchmap.rs:27-50, switchmap.rs:355-365 | ported |
| 18 | Clear the result value if an error occurred | switchmap.c:1121-1123 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 19 | Return the result | switchmap.c:1124-1125 | switchmap.rs:671 | ported |

## Rows 13 and 14 — the either/or, and what selects between the arms

C's `if( invert_list[where] ) { … } else { … }` makes normalising an inverted
SwitchMap and simplifying its children alternative outcomes of one nomination.
Tracing C shows it: the same SwitchMap is nominated at `inv=1`, then again at
`inv=0`, then a third time once the children were reduced.

The port selects between the arms on `ctx.entry(where_).1 != self.is_inverted()`
rather than on the SwitchMap's own `Invert` flag. `flatten` resets every leaf it
extracts to forward and carries the inversion in the list's effective-invert
flag, so C's `(*invert_list)[where]` is that flag, not the object's.

The second arm reports C's own `simp` — whether any child's simplification
returned a different Mapping — rather than comparing the rebuilt SwitchMap with
the original. C compares child pointers (`sfsmap != map->fsmap`), and a rebuilt
SwitchMap can be behaviourally equal to the original: an inverted child replaced
by its forward equal transforms identically.

Both child paths stamp what they get back, because C reaches each encapsulated
Mapping through `astSimplify`, which records its result whether or not it reduced
anything (`mapping.c:24763`).

## Row 4 — why C restores the Invert flags rather than inverting

C uses `astSetInvert` rather than `astInvert` throughout, because two or more of
the stored pointers may point at the same Mapping and `astInvert` would flip it
more than once. The port holds owned children, so the hazard does not exist; what
it must preserve instead is the *effective* direction, `recorded_inv !=
is_inverted()`, when a child's own simplification changes its stored orientation.
`simplify_child_with_invert` and the route loop in `reduced_children` re-derive
each recorded flag for that reason.

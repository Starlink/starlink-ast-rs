# `MapMerge` audit — SwitchMap

C source: reference/starlink-ast/src/switchmap.c
Rust: crates/ast-core/src/mappings/switchmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- switchmap.c:938 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `switchmap.c:955-1125 (fn MapMerge)` in two parts: cancel the SwitchMap against
an adjacent equal SwitchMap used the other way; and, only if that did nothing,
simplify the SwitchMap on its own. The second part is itself an either/or — an
inverted SwitchMap is normalised to a forward one and nothing else happens on
that pass, and only a forward one has its encapsulated Mappings simplified.

`SwitchMap::map_merge` (`switchmap.rs:734-778 (fn map_merge)`) holds both parts in C's order:
`merge_with_neighbour` (`switchmap.rs:922-948 (fn merge_with_neighbour)`) for the neighbour cancellation, then the either/or, whose
second arm calls `SwitchMap::reduced_children` (`switchmap.rs:369-410 (fn reduced_children)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | switchmap.c:955-956 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | switchmap.c:958-959 (fn MapMerge) | -- | architecture | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this SwitchMap and the route Mapping count | switchmap.c:961-964 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Temporarily restore every encapsulated Mapping's Invert flag to its construction value | switchmap.c:965-991 (fn MapMerge) | switchmap.rs:27-50 (fn simplify_child_with_invert), switchmap.rs:380-389 (fn reduced_children) | architecture | ported |
| 5 | Merge with a neighbouring SwitchMap, in series only | switchmap.c:992-996 (fn MapMerge) | switchmap.rs:745 (fn map_merge) | architecture | ported |
| 6 | Check the higher neighbour first, noting the lower index of the pair | switchmap.c:997-1003 (fn MapMerge) | switchmap.rs:923-925 (fn merge_with_neighbour) | architecture | ported |
| 7 | Failing that, check the lower neighbour the same way | switchmap.c:1004-1014 (fn MapMerge) | switchmap.rs:926-930 (fn merge_with_neighbour) | architecture | ported |
| 8 | Cancel the pair only if the SwitchMap equals the inverted neighbour, each with its own Invert attribute | switchmap.c:1015-1023 (fn MapMerge) | switchmap.rs:931-944 (fn merge_with_neighbour) | architecture | ported |
| 9 | Replace the first of the pair with a UnitMap and set its invert flag | switchmap.c:1024-1031 (fn MapMerge) | switchmap.rs:945-946 (fn merge_with_neighbour) | architecture | ported |
| 10 | Annul the second and shuffle the rest down, clearing the vacated slot | switchmap.c:1032-1043 (fn MapMerge) | switchmap.rs:946 (fn merge_with_neighbour) | architecture | ported |
| 11 | Decrement the Mapping count and return the index of the first modified element | switchmap.c:1044-1051 (fn MapMerge) | switchmap.rs:947 (fn merge_with_neighbour) | architecture | ported |
| 12 | Attempt to simplify the SwitchMap on its own, only if no change was made above | switchmap.c:1052-1056 (fn MapMerge) | switchmap.rs:745-747 (fn map_merge) | architecture | ported |
| 13 | If the SwitchMap is inverted, replace it with an equal forward one by swapping the selectors and inverting the route Mappings, and stop there | switchmap.c:1057-1076 (fn MapMerge) | switchmap.rs:758-766 (fn map_merge), switchmap.rs:259-279 (fn normalize_inverted) | architecture | ported |
| 14 | Otherwise try to simplify each encapsulated Mapping, noting whether anything changed | switchmap.c:1077-1091 (fn MapMerge) | switchmap.rs:369-390 (fn reduced_children) | architecture | ported |
| 15 | If anything simplified, build a new SwitchMap from the simplified Mappings | switchmap.c:1092-1100 (fn MapMerge) | switchmap.rs:392-409 (fn reduced_children) | architecture | ported |
| 16 | Release resources | switchmap.c:1101-1110 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 17 | Re-instate the encapsulated Mappings' original Invert values | switchmap.c:1111-1120 (fn MapMerge) | switchmap.rs:27-50 (fn simplify_child_with_invert), switchmap.rs:380-389 (fn reduced_children) | ceremony | ported |
| 18 | Clear the result value if an error occurred | switchmap.c:1121-1123 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 19 | Return the result | switchmap.c:1124-1125 (fn MapMerge) | switchmap.rs:777 (fn map_merge) | ceremony | ported |

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
anything (`mapping.c:24763 (fn astSimplify_)`).

## Row 4 — why C restores the Invert flags rather than inverting

C uses `astSetInvert` rather than `astInvert` throughout, because two or more of
the stored pointers may point at the same Mapping and `astInvert` would flip it
more than once. The port holds owned children, so the hazard does not exist; what
it must preserve instead is the *effective* direction, `recorded_inv !=
is_inverted()`, when a child's own simplification changes its stored orientation.
`simplify_child_with_invert` and the route loop in `reduced_children` re-derive
each recorded flag for that reason.

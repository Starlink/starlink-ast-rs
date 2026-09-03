# `MapMerge` audit — SelectorMap

C source: reference/starlink-ast/src/selectormap.c
Rust: crates/ast-core/src/mappings/selectormap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin 3e60abaa
Extension points: classified

Uncited C steps:
- selectormap.c:671 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `selectormap.c:683-788 (fn MapMerge)`: simplify the encapsulated Regions and
rebuild if any changed; failing that, cancel the SelectorMap against an adjacent
equal SelectorMap used the other way.

Note the order — unlike SwitchMap, C does the self-simplification *first* here and
only then looks at the neighbour.

`SelectorMap::map_merge` (`selectormap.rs:325-335 (fn map_merge)`) computes the
self-simplification itself, in `SelectorMap::reduced_children`
(`selectormap.rs:116-162 (fn reduced_children)`), and `self_reduce_map_merge` falls through to
`default_map_merge` only when that made no change, so the two are in C's order.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | selectormap.c:683-684 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | selectormap.c:686-687 (fn MapMerge) | -- | architecture | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this SelectorMap and its Region count | selectormap.c:689-692 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Attempt to simplify the SelectorMap on its own | selectormap.c:693-695 (fn MapMerge) | selectormap.rs:334 (fn map_merge) | architecture | ported |
| 5 | Simplify each encapsulated Region, noting whether anything changed | selectormap.c:696-705 (fn MapMerge) | selectormap.rs:117-150 (fn reduced_children) | architecture | ported |
| 6 | If anything simplified, build a new SelectorMap from the results | selectormap.c:706-715 (fn MapMerge) | selectormap.rs:152-162 (fn reduced_children) | architecture | ported |
| 7 | Release resources | selectormap.c:716-722 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 8 | Otherwise merge with a neighbouring SelectorMap, in series only | selectormap.c:723-728 (fn MapMerge) | selectormap.rs:337-367 (fn try_merge) | architecture | ported |
| 9 | Check the higher neighbour first, noting the lower index of the pair | selectormap.c:729-735 (fn MapMerge) | mappings/simplify.rs:1218-1227 (fn default_map_merge) | architecture | ported |
| 10 | Failing that, check the lower neighbour the same way | selectormap.c:736-746 (fn MapMerge) | mappings/simplify.rs:1231-1240 (fn default_map_merge) | architecture | ported |
| 11 | Cancel the pair only if the two SelectorMaps are equal with opposite invert flags | selectormap.c:747-755 (fn MapMerge) | selectormap.rs:348-364 (fn try_merge) | architecture | ported |
| 12 | Replace the first of the pair with a UnitMap and set its invert flag | selectormap.c:756-763 (fn MapMerge) | selectormap.rs:366 (fn try_merge) | architecture | ported |
| 13 | Annul the second and shuffle the rest down, clearing the vacated slot | selectormap.c:764-775 (fn MapMerge) | mappings/simplify.rs:1298-1299 (fn default_map_merge) | architecture | ported |
| 14 | Decrement the Mapping count and return the index of the first modified element | selectormap.c:776-783 (fn MapMerge) | mappings/simplify.rs:1299-1301 (fn default_map_merge) | architecture | ported |
| 15 | Clear the result value if an error occurred | selectormap.c:784-786 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 16 | Return the result | selectormap.c:787-788 (fn MapMerge) | selectormap.rs:334 (fn map_merge) | ceremony | ported |

## Rows 4 and 8 — the order is the opposite of SwitchMap's

C simplifies the encapsulated Regions first and only then considers the
neighbour. `self_reduce_map_merge` has that shape by construction: it takes the
candidate the class computed and returns on a change, reaching
`default_map_merge` only when nothing reduced. SwitchMap's C body is the other
way round, and takes the neighbour first — which is why the two classes use
different Rust entry points despite looking alike.

## Row 5 — a child that stops being a Region is not taken

A SelectorMap's children are Regions, and a Region's simplification can return a
plain Mapping. `selectormap.rs:117-150 (fn reduced_children)` keeps the original Region for that slot
rather than storing a non-Region, so the rebuilt SelectorMap is always
well-formed. C reaches the same place by calling `astSimplify` on the Region
pointer and relying on a Region simplifying to a Region.

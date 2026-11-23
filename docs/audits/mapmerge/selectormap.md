# `MapMerge` audit — SelectorMap

C source: reference/starlink-ast/src/selectormap.c
Rust: crates/starlink-ast/src/mappings/selectormap.rs
Audited: 2026-08-21 against pin 3cf4bc5c
Citations: verified against pin fe26cc9b
Extension points: classified

Uncited C steps:
- selectormap.c:681 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `selectormap.c:693-798 (fn MapMerge)`: simplify the encapsulated Regions and
rebuild if any changed; failing that, cancel the SelectorMap against an adjacent
equal SelectorMap used the other way.

Note the order — unlike SwitchMap, C does the self-simplification *first* here and
only then looks at the neighbor.

`SelectorMap::map_merge` (`selectormap.rs:427-437 (fn map_merge)`) computes the
self-simplification itself, in `SelectorMap::reduced_children`
(`selectormap.rs:189-238 (fn reduced_children)`), and `self_reduce_map_merge` falls through to
`default_map_merge` only when that made no change, so the two are in C's order.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | selectormap.c:693-694 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | selectormap.c:696-697 (fn MapMerge) | -- | architecture | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this SelectorMap and its Region count | selectormap.c:699-702 (fn MapMerge) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | Attempt to simplify the SelectorMap on its own | selectormap.c:703-705 (fn MapMerge) | selectormap.rs:436 (fn map_merge) | architecture | ported |
| 5 | Simplify each encapsulated Region, noting whether anything changed | selectormap.c:706-715 (fn MapMerge) | selectormap.rs:190-223 (fn reduced_children) | architecture | ported |
| 6 | If anything simplified, build a new SelectorMap from the results | selectormap.c:716-725 (fn MapMerge) | selectormap.rs:222-234 (fn reduced_children) | architecture | ported |
| 7 | Release resources | selectormap.c:726-732 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 8 | Otherwise merge with a neighboring SelectorMap, in series only | selectormap.c:733-738 (fn MapMerge) | selectormap.rs:439-469 (fn try_merge) | architecture | ported |
| 9 | Check the higher neighbor first, noting the lower index of the pair | selectormap.c:739-745 (fn MapMerge) | mappings/simplify.rs:1672-1681 (fn default_map_merge) | architecture | ported |
| 10 | Failing that, check the lower neighbor the same way | selectormap.c:746-756 (fn MapMerge) | mappings/simplify.rs:1685-1694 (fn default_map_merge) | architecture | ported |
| 11 | Cancel the pair only if the two SelectorMaps are equal with opposite invert flags | selectormap.c:757-765 (fn MapMerge) | selectormap.rs:450-466 (fn try_merge) | architecture | ported |
| 12 | Replace the first of the pair with a UnitMap and set its invert flag | selectormap.c:766-773 (fn MapMerge) | selectormap.rs:468 (fn try_merge) | architecture | ported |
| 13 | Annul the second and shuffle the rest down, clearing the vacated slot | selectormap.c:774-785 (fn MapMerge) | mappings/simplify.rs:1750-1751 (fn default_map_merge) | architecture | ported |
| 14 | Decrement the Mapping count and return the index of the first modified element | selectormap.c:786-793 (fn MapMerge) | mappings/simplify.rs:1751-1752 (fn default_map_merge) | architecture | ported |
| 15 | Clear the result value if an error occurred | selectormap.c:794-796 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 16 | Return the result | selectormap.c:797-798 (fn MapMerge) | selectormap.rs:436 (fn map_merge) | ceremony | ported |

## Rows 4 and 8 — the order is the opposite of SwitchMap's

C simplifies the encapsulated Regions first and only then considers the
neighbor. `self_reduce_map_merge` has that shape by construction: it takes the
candidate the class computed and returns on a change, reaching
`default_map_merge` only when nothing reduced. SwitchMap's C body is the other
way round, and takes the neighbor first — which is why the two classes use
different Rust entry points despite looking alike.

## Row 5 — a child that stops being a Region is not taken

A SelectorMap's children are Regions, and a Region's simplification can return a
plain Mapping. `selectormap.rs:190-223 (fn reduced_children)` keeps the original Region for that slot
rather than storing a non-Region, so the rebuilt SelectorMap is always
well-formed. C reaches the same place by calling `astSimplify` on the Region
pointer and relying on a Region simplifying to a Region.

# `MapMerge` audit — SelectorMap

C source: reference/starlink-ast/src/selectormap.c
Rust: crates/ast-core/src/mappings/selectormap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `selectormap.c:683-788`: simplify the encapsulated Regions and
rebuild if any changed; failing that, cancel the SelectorMap against an adjacent
equal SelectorMap used the other way.

Note the order — unlike SwitchMap, C does the self-simplification *first* here and
only then looks at the neighbour.

`SelectorMap::map_merge` (`selectormap.rs:301-311`) computes the
self-simplification itself, in `SelectorMap::reduced_children`
(`selectormap.rs:114-157`), and `self_reduce_map_merge` falls through to
`default_map_merge` only when that made no change, so the two are in C's order.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | selectormap.c:683-684 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | selectormap.c:686-687 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this SelectorMap and its Region count | selectormap.c:689-692 | -- | n/a: the Rust method already has `&self` |
| 4 | Attempt to simplify the SelectorMap on its own | selectormap.c:693-695 | selectormap.rs:310 | ported |
| 5 | Simplify each encapsulated Region, noting whether anything changed | selectormap.c:696-705 | selectormap.rs:115-148 | ported |
| 6 | If anything simplified, build a new SelectorMap from the results | selectormap.c:706-715 | selectormap.rs:150-157 | ported |
| 7 | Release resources | selectormap.c:716-722 | -- | n/a: owned values are dropped |
| 8 | Otherwise merge with a neighbouring SelectorMap, in series only | selectormap.c:723-728 | selectormap.rs:313-343 | ported |
| 9 | Check the higher neighbour first, noting the lower index of the pair | selectormap.c:729-735 | simplify.rs:1151-1246 | ported |
| 10 | Failing that, check the lower neighbour the same way | selectormap.c:736-746 | simplify.rs:1151-1246 | ported |
| 11 | Cancel the pair only if the two SelectorMaps are equal with opposite invert flags | selectormap.c:747-755 | selectormap.rs:324-340 | ported |
| 12 | Replace the first of the pair with a UnitMap and set its invert flag | selectormap.c:756-763 | selectormap.rs:342 | ported |
| 13 | Annul the second and shuffle the rest down, clearing the vacated slot | selectormap.c:764-775 | simplify.rs:1151-1246 | ported |
| 14 | Decrement the Mapping count and return the index of the first modified element | selectormap.c:776-783 | simplify.rs:1151-1246 | ported |
| 15 | Clear the result value if an error occurred | selectormap.c:784-786 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 16 | Return the result | selectormap.c:787-788 | selectormap.rs:310 | ported |

## Rows 4 and 8 — the order is the opposite of SwitchMap's

C simplifies the encapsulated Regions first and only then considers the
neighbour. `self_reduce_map_merge` has that shape by construction: it takes the
candidate the class computed and returns on a change, reaching
`default_map_merge` only when nothing reduced. SwitchMap's C body is the other
way round, and takes the neighbour first — which is why the two classes use
different Rust entry points despite looking alike.

## Row 5 — a child that stops being a Region is not taken

A SelectorMap's children are Regions, and a Region's simplification can return a
plain Mapping. `selectormap.rs:115-148` keeps the original Region for that slot
rather than storing a non-Region, so the rebuilt SelectorMap is always
well-formed. C reaches the same place by calling `astSimplify` on the Region
pointer and relying on a Region simplifying to a Region.

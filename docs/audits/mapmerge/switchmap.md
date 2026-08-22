# `MapMerge` audit — SwitchMap

C source: reference/starlink-ast/src/switchmap.c
Rust: crates/ast-core/src/mappings/switchmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `switchmap.c:955-1125` in two parts: cancel the SwitchMap against
an adjacent equal SwitchMap used the other way; and, only if that did nothing,
simplify the SwitchMap on its own. The second part is itself an either/or — an
inverted SwitchMap is normalised to a forward one and nothing else happens on
that pass, and only a forward one has its encapsulated Mappings simplified.

`SwitchMap::map_merge` (`switchmap.rs:564-571`) routes through
`self_reduce_map_merge`, which consults `SwitchMap::simplified`
(`switchmap.rs:573-628`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | switchmap.c:955-956 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | switchmap.c:958-959 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this SwitchMap and the route Mapping count | switchmap.c:961-964 | -- | n/a: the Rust method already has `&self` |
| 4 | Temporarily restore every encapsulated Mapping's Invert flag to its construction value | switchmap.c:965-991 | switchmap.rs:598-608 | ported |
| 5 | Merge with a neighbouring SwitchMap, in series only | switchmap.c:992-996 | switchmap.rs:630-660 | ported |
| 6 | Check the higher neighbour first, noting the lower index of the pair | switchmap.c:997-1003 | simplify.rs:1155-1250 | ported |
| 7 | Failing that, check the lower neighbour the same way | switchmap.c:1004-1014 | simplify.rs:1155-1250 | ported |
| 8 | Cancel the pair only if the two SwitchMaps are equal with opposite invert flags | switchmap.c:1015-1023 | switchmap.rs:630-660 | ported |
| 9 | Replace the first of the pair with a UnitMap and set its invert flag | switchmap.c:1024-1031 | switchmap.rs:630-660 | ported |
| 10 | Annul the second and shuffle the rest down, clearing the vacated slot | switchmap.c:1032-1043 | simplify.rs:1155-1250 | ported |
| 11 | Decrement the Mapping count and return the index of the first modified element | switchmap.c:1044-1051 | simplify.rs:1155-1250 | ported |
| 12 | Attempt to simplify the SwitchMap on its own, only if no change was made above | switchmap.c:1052-1056 | switchmap.rs:564-571 | ported |
| 13 | If the SwitchMap is inverted, replace it with an equal forward one by swapping the selectors and inverting the route Mappings, and stop there | switchmap.c:1057-1076 | switchmap.rs:228-247 | gap: PLAN.md §10.10 |
| 14 | Otherwise try to simplify each encapsulated Mapping, noting whether anything changed | switchmap.c:1077-1091 | switchmap.rs:590-616 | gap: PLAN.md §10.10 |
| 15 | If anything simplified, build a new SwitchMap from the simplified Mappings | switchmap.c:1092-1100 | switchmap.rs:617-624 | ported |
| 16 | Release resources | switchmap.c:1101-1110 | -- | n/a: owned values are dropped |
| 17 | Re-instate the encapsulated Mappings' original Invert values | switchmap.c:1111-1120 | switchmap.rs:598-608 | ported |
| 18 | Clear the result value if an error occurred | switchmap.c:1121-1123 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 19 | Return the result | switchmap.c:1124-1125 | switchmap.rs:570 | ported |

## Rows 13 and 14 — C's either/or is one pass here

C's `if( ( *invert_list )[ where ] ) { … } else { … }` makes normalising an
inverted SwitchMap and simplifying its children *alternative* outcomes of a
single nomination. The normalisation returns `where` and the nominate loop comes
back, so the children are simplified on a later pass over a different object.

The port fuses them. `normalize_inverted` (`switchmap.rs:228-247`) does exactly
C's swap-and-invert, but it is called from the public `simplify()` entry point
(`mappings/mod.rs:607-613`) and immediately followed by `simplified()` on the
result, so both happen within one call. `SwitchMap::simplified` itself keeps
`self.inverted` (`switchmap.rs:621-623`) and only recurses into the children.

Two consequences. The intermediate normalised SwitchMap is never the whole result
of a simplification, so it never carries the record C stamps on it. And because
the normalisation lives at the top level rather than in `map_merge`, an inverted
SwitchMap nominated from inside a list is never normalised at all. See PLAN.md
§10.10.

## Row 4 — why C restores the Invert flags rather than inverting

C uses `astSetInvert` rather than `astInvert` throughout, because two or more of
the stored pointers may point at the same Mapping and `astInvert` would flip it
more than once. The port holds owned children, so the hazard does not exist; what
it must preserve instead is the *effective* direction, `recorded_inv !=
is_inverted()`, when a child's own simplification changes its stored orientation.
`simplify_child_with_invert` and the route loop at `switchmap.rs:598-616` re-derive
each recorded flag for that reason.

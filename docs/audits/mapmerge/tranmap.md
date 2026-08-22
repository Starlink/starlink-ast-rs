# `MapMerge` audit — TranMap

C source: reference/starlink-ast/src/tranmap.c
Rust: crates/ast-core/src/mappings/tranmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `tranmap.c:814-1014`. It simplifies the TranMap on its own — an
either/or between normalising an inverted TranMap and simplifying its two
component Mappings, with a third case that collapses a TranMap whose components
are inverses of each other — and then, failing that, merges with a neighbouring
TranMap by composing the two forward components and the two inverse components.

`TranMap::map_merge` is `tranmap.rs:326-361`; the self-simplification is
`TranMap::simplified` (`tranmap.rs:256-324`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | tranmap.c:814-815 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the inherited status and bail out | tranmap.c:817-818 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a typed pointer to this TranMap | tranmap.c:820-822 | -- | n/a: the Rust method already has `&self` |
| 4 | Take the two component Mappings, temporarily restoring their construction Invert values | tranmap.c:823-833 | tranmap.rs:340-341 | ported |
| 5 | Simplify the TranMap on its own | tranmap.c:834-836 | tranmap.rs:333-345 | ported |
| 6 | If the TranMap is inverted, replace it with an equal forward one by inverting and swapping the components, and stop there | tranmap.c:837-850 | tranmap.rs:261-270 | gap: PLAN.md §10.11 |
| 7 | Otherwise try to simplify each component Mapping | tranmap.c:851-855 | tranmap.rs:340-341 | ported |
| 8 | Assume something simplified if either pointer changed | tranmap.c:856-858 | tranmap.rs:342-345 | ported |
| 9 | Build a new TranMap from the simplified components | tranmap.c:859-863 | tranmap.rs:347-358 | ported |
| 10 | Otherwise, if both components are defined in both directions | tranmap.c:864-867 | tranmap.rs:297-303 | gap: PLAN.md §10.11 |
| 11 | Form a series CmpMap of the two components with the second inverted | tranmap.c:868-873 | tranmap.rs:304-315 | gap: PLAN.md §10.11 |
| 12 | If that CmpMap simplifies to a UnitMap the components are equal, so replace the whole TranMap with one of them | tranmap.c:874-887 | tranmap.rs:316-322 | gap: PLAN.md §10.11 |
| 13 | Release resources | tranmap.c:888-897 | -- | n/a: owned values are dropped |
| 14 | Merge with a neighbouring TranMap, in series only and only if nothing changed above | tranmap.c:898-903 | tranmap.rs:360 | ported |
| 15 | Require the higher neighbour to be a TranMap | tranmap.c:904-907 | tranmap.rs:363-420 | ported |
| 16 | Take the higher TranMap's components, restoring their construction Invert values | tranmap.c:908-921 | tranmap.rs:363-420 | ported |
| 17 | Resolve which component gives the forward and which the inverse transformation of the lower TranMap, honouring its applied direction | tranmap.c:922-935 | tranmap.rs:363-420 | ported |
| 18 | Do the same for the higher TranMap | tranmap.c:936-947 | tranmap.rs:363-420 | ported |
| 19 | Compose the two forward Mappings into a series CmpMap and simplify it | tranmap.c:948-952 | tranmap.rs:363-420 | ported |
| 20 | Do the same for the two inverse Mappings | tranmap.c:953-956 | tranmap.rs:363-420 | ported |
| 21 | Take the merge only if either composition actually simplified | tranmap.c:957-960 | tranmap.rs:363-420 | ported |
| 22 | Construct a new TranMap from the two simplified compositions | tranmap.c:961-967 | tranmap.rs:363-420 | ported |
| 23 | Free resources and re-instate the higher TranMap's component Invert values | tranmap.c:968-978 | -- | n/a: owned values are dropped and nothing was mutated |
| 24 | Replace the first of the pair with the merged TranMap and set its invert flag | tranmap.c:979-985 | simplify.rs:1155-1250 | ported |
| 25 | Annul the second and shuffle the rest down, clearing the vacated slot | tranmap.c:986-997 | simplify.rs:1155-1250 | ported |
| 26 | Decrement the Mapping count and return the index of the first modified element | tranmap.c:998-1005 | simplify.rs:1155-1250 | ported |
| 27 | Re-instate the nominated TranMap's component Invert values | tranmap.c:1006-1009 | -- | n/a: nothing was mutated, so nothing is restored |
| 28 | Clear the result value if an error occurred | tranmap.c:1010-1012 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 29 | Return the result | tranmap.c:1013-1014 | tranmap.rs:360 | ported |

## Rows 6 and 10 to 12 — three of C's four self-simplification cases are unreachable from the list

All four exist in `TranMap::simplified` (`tranmap.rs:256-324`): the inversion
normalisation at `:261-270`, the component simplification at `:272-295`, and the
equal-components collapse at `:297-322`. But `TranMap::map_merge`
(`tranmap.rs:326-361`) implements only the middle one, inline, and preserves
`self.inverted` on the rebuild (`tranmap.rs:349-351`). It never calls
`simplified()`.

So a TranMap nominated from inside a list gets its components simplified and
nothing else: it keeps its inversion where C replaces it with the equal forward
form, and a TranMap whose two components are inverses of each other is not
collapsed to one of them.

The inversion case has the same fused shape as SwitchMap's besides:
`tranmap.rs:269` returns `normalized.simplified()`, so the normalised TranMap is
never the whole result of a simplification on its own. See PLAN.md §10.11.

## Row 17 — which component is forward depends on the applied direction

A TranMap holds one Mapping for each direction, so its applied inversion decides
which is which. C resolves that before composing (`tranmap.c:922-947`) and the
port does the same at `tranmap.rs:363-420`. Composing without resolving it would
pair a forward component with an inverse one.

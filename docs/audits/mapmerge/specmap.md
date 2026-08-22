# `MapMerge` audit — SpecMap

C source: reference/starlink-ast/src/specmap.c
Rust: crates/ast-core/src/mappings/specmap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `specmap.c:2256-2637`, the same architecture as SlaMap: gather the
conversion steps of a run of adjacent SpecMaps, normalise each for its applied
direction, cancel adjacent inverse pairs until nothing more goes, and rebuild one
SpecMap or a UnitMap.

`SpecMap::map_merge` (`specmap.rs:1390-1397`) delegates to `spec_map_merge`
(`specmap.rs:1168-1235`); the helpers are `normalized_steps`
(`specmap.rs:1086-1121`), `steps_cancel` (`specmap.rs:1063-1077`) and
`simplify_steps` (`specmap.rs:1135-1167`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | specmap.c:2256-2257 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | specmap.c:2259-2260 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | SpecMaps merge only in series, or when there is a single Mapping | specmap.c:2262-2266 | specmap.rs:1173-1175 | ported |
| 4 | Save the nominated SpecMap's input count | specmap.c:2267-2269 | -- | n/a: it is only used for the axis-count guard in rows 5 and 6, which cannot fire here |
| 5 | Search adjacent lower-numbered Mappings for SpecMaps of the same axis count, accumulating step counts | specmap.c:2274-2285 | specmap.rs:1181-1184 | ported |
| 6 | Search adjacent higher-numbered Mappings the same way | specmap.c:2286-2295 | specmap.rs:1185-1188 | ported |
| 7 | Remember the initial number of steps | specmap.c:2296-2298 | specmap.rs:1192,1197 | ported |
| 8 | Allocate a list to accumulate every step from every SpecMap found | specmap.c:2299-2305 | specmap.rs:1191 | ported |
| 9 | Take each SpecMap in the run and note whether it is applied inverted | specmap.c:2306-2314 | specmap.rs:1195-1199 | ported |
| 10 | Scan its steps forward or in reverse as the direction requires | specmap.c:2315-2321 | specmap.rs:1086-1121 | ported |
| 11 | Copy each step's type code and its user-supplied arguments | specmap.c:2322-2334 | specmap.rs:1199-1201 | ported |
| 12 | For an inverted SpecMap, also invert each step, not merely reverse their order | specmap.c:2335-2350 | specmap.rs:1086-1121 | ported |
| 13 | Some inversions also reciprocate an argument | specmap.c:2351-2371 | specmap.rs:1086-1121 | ported |
| 14 | Some inversions also negate an argument | specmap.c:2372-2388 | specmap.rs:1086-1121 | ported |
| 15 | Exchange each transformation code for its inverse | specmap.c:2389-2402 | specmap.rs:349-360 | ported |
| 16 | For the frequency-correction conversions, also reciprocate the correction factor | specmap.c:2403-2415 | specmap.rs:1086-1121 | ported |
| 17 | Count the accumulated steps | specmap.c:2421-2425 | specmap.rs:1197 | ported |
| 18 | Loop, simplifying the step sequence until no further improvement is possible | specmap.c:2426-2430 | specmap.rs:1135-1167 | ported |
| 19 | Examine each remaining step in turn, assuming at first it will be kept | specmap.c:2431-2437 | specmap.rs:1135-1167 | ported |
| 20 | Every simplification in this class combines adjacent steps, so apply them only while two remain | specmap.c:2438-2454 | specmap.rs:1135-1167 | ported |
| 21 | Eliminate a correction followed by its inverse when the user-supplied arguments are unchanged, ignoring the cached ones | specmap.c:2455-2469 | specmap.rs:1063-1077 | ported |
| 22 | Apply that test to conversions with one user-supplied argument | specmap.c:2470-2476 | specmap.rs:1067-1075 | ported |
| 23 | With two arguments | specmap.c:2477-2488 | specmap.rs:1067-1075 | ported |
| 24 | With three arguments | specmap.c:2489-2501 | specmap.rs:1067-1075 | ported |
| 25 | With six arguments | specmap.c:2502-2521 | specmap.rs:1067-1075 | ported |
| 26 | Compact the surviving steps down over the eliminated ones | specmap.c:2527-2545 | specmap.rs:1135-1167 | ported |
| 27 | Stop iterating when a pass reduced nothing | specmap.c:2546-2550 | specmap.rs:1135-1167 | ported |
| 28 | Note how many steps remain | specmap.c:2551-2554 | specmap.rs:1207 | ported |
| 29 | Count how many Mappings condensing the run eliminates | specmap.c:2555-2559 | specmap.rs:1205 | ported |
| 30 | Decide whether the replacement can be a UnitMap, when no steps remain | specmap.c:2560-2573 | specmap.rs:1207 | ported |
| 31 | Do nothing more unless something simplified | specmap.c:2574-2576 | specmap.rs:1209-1211 | ported |
| 32 | Create the UnitMap replacement if that is the answer | specmap.c:2577-2581 | specmap.rs:1214-1216 | ported |
| 33 | Otherwise create a replacement SpecMap holding the surviving steps | specmap.c:2582-2591 | specmap.rs:1217-1222 | ported |
| 34 | Annul the eliminated Mappings and insert the replacement with its invert value | specmap.c:2592-2601 | specmap.rs:1224-1234 | ported |
| 35 | Close the gap, blank the tail, and return the index of the first eliminated Mapping | specmap.c:2602-2618 | specmap.rs:1224-1234 | ported |
| 36 | Annul the new Mapping if an error occurred | specmap.c:2619-2625 | -- | n/a: an error is a `Result::Err`, and owned values are dropped |
| 37 | Free the step list | specmap.c:2626-2632 | -- | n/a: owned values are dropped |
| 38 | Clear the returned value if an error occurred | specmap.c:2633-2635 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 39 | Return the result | specmap.c:2636-2637 | specmap.rs:1235 | ported |

## Rows 4 to 6 — C's axis-count guard cannot fire here

C stops the run at a SpecMap whose `astGetNin` differs from the nominee's,
because `astInitMapping( …, nin, nin, 1, 1 )` (`specmap.c:4429-4431`) gives a
SpecMap however many axes it was constructed with. Two consequences: adjacent
SpecMaps in a *series* list must already agree, since a SpecMap's input and output
counts are equal; and the port's `SpecMap` is fixed at one axis
(`specmap.rs:1235-1241`), so no two of them can differ. `is_spec`
(`specmap.rs:1177`) therefore tests only the class, and the guard is unreachable
rather than dropped.

The one-axis restriction is a class limitation, not a `MapMerge` gap, and is not
tracked here.

## Rows 21 to 25 — the argument count is read from the type

C writes a separate test per argument count because its macro needs the count
spelled out. `steps_cancel` (`specmap.rs:1063-1077`) asks the conversion type for
its `nargs()` and compares that many, which covers one, two, three and six
without enumerating them. The cached arguments beyond `nargs()` are excluded from
the comparison, which is C's "we do not need to test values stored internally".

`reset_cached_args` (`specmap.rs:1122-1130`) then clears those slots on the
rebuilt SpecMap, so a cached correction factor from before the merge cannot leak
into the replacement.

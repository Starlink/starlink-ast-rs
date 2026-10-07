# `MapMerge` audit — SpecMap

C source: reference/starlink-ast/src/specmap.c
Rust: crates/ast-core/src/mappings/specmap.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- specmap.c:2222 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `specmap.c:2256-2637 (fn MapMerge)`, the same architecture as SlaMap and
TimeMap: gather the conversion steps of a run of adjacent SpecMaps, normalise
each for its applied direction, cancel adjacent inverse pairs until nothing more
goes, and rebuild one SpecMap or a UnitMap.

`SpecMap::map_merge` (`specmap.rs:1469-1476 (fn map_merge)`) delegates to `spec_map_merge`
(`specmap.rs:1252-1319 (fn spec_map_merge)`); the helpers are `normalized_steps`
(`specmap.rs:1170-1194 (fn normalized_steps)`), `steps_cancel`
(`specmap.rs:1147-1161 (fn steps_cancel)`), `simplify_steps`
(`specmap.rs:1219-1240 (fn simplify_steps)`) and `reset_cached_args`
(`specmap.rs:1206-1213 (fn reset_cached_args)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | specmap.c:2256-2258 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | specmap.c:2259-2261 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | SpecMaps merge only in series, or when only one Mapping is present | specmap.c:2262-2266 (fn MapMerge) | specmap.rs:1257-1259 (fn spec_map_merge) | architecture | ported |
| 4 | Save the nominated SpecMap's input count | specmap.c:2267-2269 (fn MapMerge) | specmap.rs:1263 (fn spec_map_merge) | architecture | ported |
| 5 | Initialise the step count to the number in the nominated SpecMap | specmap.c:2270-2273 (fn MapMerge) | specmap.rs:1289 (fn spec_map_merge) | architecture | ported |
| 6 | Search adjacent lower-numbered Mappings for SpecMaps of the same axis count, accumulating step counts | specmap.c:2274-2285 (fn MapMerge) | specmap.rs:1269-1272 (fn spec_map_merge) | architecture | ported |
| 7 | Search adjacent higher-numbered Mappings the same way | specmap.c:2286-2295 (fn MapMerge) | specmap.rs:1273-1276 (fn spec_map_merge) | architecture | ported |
| 8 | Remember the initial number of transformation steps | specmap.c:2296-2298 (fn MapMerge) | specmap.rs:1281 (fn spec_map_merge),1289 (fn spec_map_merge) | architecture | ported |
| 9 | Allocate memory for the accumulated step list | specmap.c:2299-2305 (fn MapMerge) | -- | ceremony | n/a: a `Vec` grows as the steps are appended |
| 10 | Loop to obtain the transformation data for each SpecMap being merged | specmap.c:2306-2309 (fn MapMerge) | specmap.rs:1283-1291 (fn spec_map_merge) | architecture | ported |
| 11 | Take each SpecMap and note whether it is being applied in its inverse direction | specmap.c:2310-2314 (fn MapMerge) | specmap.rs:1284-1287 (fn spec_map_merge) | architecture | ported |
| 12 | Set loop limits and an increment so the steps are scanned forward or in reverse as that direction dictates | specmap.c:2315-2321 (fn MapMerge) | specmap.rs:1174-1176 (fn normalized_steps) | architecture | ported |
| 13 | Loop through each transformation step in the SpecMap | specmap.c:2322-2324 (fn MapMerge) | specmap.rs:1174-1177 (fn normalized_steps) | architecture | ported |
| 14 | Store the step's type code, obtain its argument count from `CvtString`, and copy the arguments | specmap.c:2325-2334 (fn MapMerge) | specmap.rs:1179 (fn normalized_steps),1188 (fn normalized_steps) | architecture | ported |
| 15 | An inverted SpecMap must not only accumulate its steps in reverse but apply each in reverse; define macros for the changes that needs | specmap.c:2335-2341 (fn MapMerge) | specmap.rs:1171-1173 (fn normalized_steps),1176 (fn normalized_steps) | architecture | ported |
| 16 | Macro exchanging a transformation type code for its inverse | specmap.c:2342-2350 (fn MapMerge) | specmap.rs:1178 (fn normalized_steps) | architecture | ported |
| 17 | Macro exchanging the code and reciprocating a specified argument | specmap.c:2351-2371 (fn MapMerge) | specmap.rs:1181-1187 (fn normalized_steps) | architecture | ported |
| 18 | Macro exchanging the code and negating a specified argument | specmap.c:2372-2388 (fn MapMerge) | -- | ceremony | n/a: `SWAP_CODES3` is defined at 2374 and undefined at 2419 without ever being invoked, so no conversion in the class uses it |
| 19 | Apply the macros only where the SpecMap is inverted | specmap.c:2389-2391 (fn MapMerge) | specmap.rs:1171-1173 (fn normalized_steps) | architecture | ported |
| 20 | Exchange the nine code pairs whose inversion leaves the arguments alone | specmap.c:2392-2402 (fn MapMerge) | specmap.rs:349-360 (fn inverse) | architecture | ported |
| 21 | Exchange the eight code pairs whose inversion also reciprocates the frequency correction factor | specmap.c:2403-2415 (fn MapMerge) | specmap.rs:1181-1187 (fn normalized_steps) | architecture | ported |
| 22 | Undefine the local macros | specmap.c:2416-2420 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 23 | Count the accumulated transformation steps | specmap.c:2421-2425 (fn MapMerge) | specmap.rs:1290 (fn spec_map_merge) | architecture | ported |
| 24 | Loop, simplifying the step sequence until no further improvement is possible | specmap.c:2426-2430 (fn MapMerge) | specmap.rs:1221-1238 (fn simplify_steps) | architecture | ported |
| 25 | Examine each remaining transformation step in turn | specmap.c:2431-2434 (fn MapMerge) | specmap.rs:1224 (fn simplify_steps) | architecture | ported |
| 26 | Initially assume the current step will be retained | specmap.c:2435-2437 (fn MapMerge) | -- | ceremony | n/a: the Rust pass removes the eliminated steps instead of marking the survivors |
| 27 | Every simplification in this class combines adjacent steps, so apply them only while two steps remain | specmap.c:2438-2442 (fn MapMerge) | specmap.rs:1224 (fn simplify_steps) | architecture | ported |
| 28 | Macro testing whether two adjacent type codes have given values | specmap.c:2443-2448 (fn MapMerge) | specmap.rs:1148 (fn steps_cancel) | architecture | ported |
| 29 | Macro testing the same pair either way round | specmap.c:2449-2454 (fn MapMerge) | specmap.rs:1148 (fn steps_cancel) | architecture | ported |
| 30 | Eliminate a correction followed by its inverse, starting with the conversions that have no user-supplied arguments | specmap.c:2455-2469 (fn MapMerge) | specmap.rs:1152-1153 (fn steps_cancel) | architecture | ported |
| 31 | The same for conversions with a single user-supplied argument | specmap.c:2470-2476 (fn MapMerge) | specmap.rs:1152-1159 (fn steps_cancel) | architecture | ported |
| 32 | The same with two user-supplied arguments | specmap.c:2477-2488 (fn MapMerge) | specmap.rs:1152-1159 (fn steps_cancel) | architecture | ported |
| 33 | The same with three user-supplied arguments | specmap.c:2489-2501 (fn MapMerge) | specmap.rs:1152-1159 (fn steps_cancel) | architecture | ported |
| 34 | The same with six user-supplied arguments | specmap.c:2502-2521 (fn MapMerge) | specmap.rs:1152-1159 (fn steps_cancel) | architecture | ported |
| 35 | Undefine the local macros | specmap.c:2522-2526 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 36 | A step being kept advances the index identifying its new location | specmap.c:2527-2532 (fn MapMerge) | specmap.rs:1232 (fn simplify_steps) | architecture | ported |
| 37 | Copy the step's data down when its new location differs from its current one | specmap.c:2533-2545 (fn MapMerge) | specmap.rs:1226-1227 (fn simplify_steps) | architecture | ported |
| 38 | Quit when a pass reduced no steps | specmap.c:2546-2550 (fn MapMerge) | specmap.rs:1235-1237 (fn simplify_steps) | architecture | ported |
| 39 | Note how many transformation steps now remain | specmap.c:2551-2554 (fn MapMerge) | -- | ceremony | n/a: the surviving steps are the `Vec`'s contents, so its length is the count |
| 40 | Determine how many Mappings condensing the run eliminates | specmap.c:2555-2559 (fn MapMerge) | specmap.rs:1293 (fn spec_map_merge) | architecture | ported |
| 41 | Determine whether the replacement can be a UnitMap, which needs every step eliminated | specmap.c:2560-2564 (fn MapMerge) | specmap.rs:1295 (fn spec_map_merge) | architecture | ported |
| 42 | Determine whether simplification is possible, on any of the four clauses | specmap.c:2565-2573 (fn MapMerge) | specmap.rs:1297 (fn spec_map_merge) | architecture | ported |
| 43 | Do nothing more unless simplification is possible | specmap.c:2574-2576 (fn MapMerge) | specmap.rs:1298-1300 (fn spec_map_merge) | architecture | ported |
| 44 | Create the UnitMap replacement where that is the answer | specmap.c:2577-2581 (fn MapMerge) | specmap.rs:1302-1303 (fn spec_map_merge) | architecture | ported |
| 45 | Otherwise create a replacement SpecMap and add each surviving step to it | specmap.c:2582-2591 (fn MapMerge) | specmap.rs:1307-1316 (fn spec_map_merge) | architecture | ported |
| 46 | Annul the pointers to the Mappings being eliminated | specmap.c:2592-2597 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 47 | Insert the replacement pointer with a cleared invert value | specmap.c:2598-2601 (fn MapMerge) | specmap.rs:1317 (fn spec_map_merge) | architecture | ported |
| 48 | Move the subsequent Mapping information down to close the gap | specmap.c:2602-2607 (fn MapMerge) | specmap.rs:1317 (fn spec_map_merge) | architecture | ported |
| 49 | Blank out the information left at the end of the arrays | specmap.c:2608-2613 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entries leave no slots to blank |
| 50 | Decrement the Mapping count and return the index of the first eliminated Mapping | specmap.c:2614-2618 (fn MapMerge) | specmap.rs:1317-1318 (fn spec_map_merge) | architecture | ported |
| 51 | Annul the new Mapping if an error occurred | specmap.c:2619-2625 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 52 | Free the memory used for the transformation steps | specmap.c:2626-2632 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 53 | Clear the returned value if an error occurred | specmap.c:2633-2635 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 54 | Return the result | specmap.c:2636-2637 (fn MapMerge) | specmap.rs:1318 (fn spec_map_merge) | ceremony | ported |

## Rows 4, 6 and 7 — C's axis-count guard

C stops the run at a SpecMap whose `astGetNin` differs from the nominee's: a
SpecMap has 1 or 3 axes (`specmap.c:4416-4421 (fn astInitSpecMap_)`). The port
saves the nominee's count and `is_spec` tests it beside the class
(`specmap.rs:1263-1265 (fn spec_map_merge)`), and the SpecMap or UnitMap that
replaces the run has that many axes.

In a *series* list the guard does not change the result, since a SpecMap's input
and output counts are equal, so adjacent SpecMaps already agree. It is kept
because C has it.

## Rows 30 to 34 — the argument count is read from the type

C writes a separate test per argument count because its macro needs the count
spelled out. `steps_cancel` asks the conversion type for its `nargs()`
(`specmap.rs:1152 (fn steps_cancel)`) and compares that many, which covers none, one, two,
three and six without enumerating them. The cached arguments beyond `nargs()` are
excluded from the comparison, which is C's "we do not need to test values stored
internally".

`reset_cached_args` then clears those slots on the rebuilt SpecMap
(`specmap.rs:1208-1211 (fn reset_cached_args)`), which is what C's `AddSpecCvt` does when a
step is added to a new SpecMap (`specmap.c:708 (fn AddSpecCvt)`), so a cached correction
factor from before the merge cannot leak into the replacement.

## Rows 25, 36 and 37 — compaction in place, or removal

C rewrites the step array in place: a surviving step is copied down over the
eliminated ones and `ikeep` tracks where the next survivor lands, so the pass
ends with the survivors packed at the front and `nstep` reset to `ikeep + 1`.
`simplify_steps` removes the eliminated steps from the `Vec` instead, which packs
the survivors by construction. The two agree on the surviving sequence because
both keep the steps in their original relative order and both consume the second
step of a cancelling pair — C by incrementing `istep`, the port by removing both
entries at once.

## Row 18 — a macro C defines and never uses

`SWAP_CODES3` negates an argument rather than reciprocating it. No `SpecMap`
conversion needs that on inversion, so C invokes the macro nowhere between its
definition and its `#undef`. There is nothing to port, and the port's inversion
table (`specmap.rs:349-360 (fn inverse)`) correspondingly has no negating case.

## Row 3 — the `*nmap == 1` disjunct is unreachable

C's guard admits a parallel nomination when the list holds a single Mapping. The
only caller that passes `series == 0` is a parallel CmpMap's own `MapMerge`, whose
list always holds both components, and the base `Simplify` nominates with
`series = 1` (`mapping.c:16871-16883 (fn Simplify)`). So the disjunct cannot be reached, and
`spec_map_merge` delegating every parallel nomination to `default_map_merge` is
equivalent rather than narrower.

## A lone SpecMap reaches the same body

C gives SpecMap no `Simplify` vtab slot, so a SpecMap nominated on its own arrives
here as a run of one: `ngone` is zero and the `simpler` test reduces to "the step
list shortened, emptied, or the SpecMap is applied inverted". Cancelling adjacent
inverse steps and rebuilding forward — with the cached argument slots blanked by
the rebuild, as C's `AddSpecCvt` does — is every reduction a lone SpecMap has. The
`where_inv` term is the list's flag, not the object's, since `flatten` resets every
leaf to forward.

# `MapMerge` audit — SpecMap

C source: reference/starlink-ast/src/specmap.c
Rust: crates/starlink-ast/src/mappings/specmap.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin fe26cc9b
Extension points: classified

Uncited C steps:
- specmap.c:2245 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `specmap.c:2279-2660 (fn MapMerge)`, the same architecture as SlaMap and
TimeMap: gather the conversion steps of a run of adjacent SpecMaps, normalize
each for its applied direction, cancel adjacent inverse pairs until nothing more
goes, and rebuild one SpecMap or a UnitMap.

`SpecMap::map_merge` (`specmap.rs:1649-1656 (fn map_merge)`) delegates to `spec_map_merge`
(`specmap.rs:1447-1516 (fn spec_map_merge)`); the helpers are `normalized_steps`
(`specmap.rs:1365-1389 (fn normalized_steps)`), `steps_cancel`
(`specmap.rs:1342-1356 (fn steps_cancel)`), `simplify_steps`
(`specmap.rs:1414-1435 (fn simplify_steps)`) and `reset_cached_args`
(`specmap.rs:1401-1408 (fn reset_cached_args)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | specmap.c:2279-2281 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | specmap.c:2282-2284 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | SpecMaps merge only in series, or when only one Mapping is present | specmap.c:2285-2289 (fn MapMerge) | specmap.rs:1452-1454 (fn spec_map_merge) | architecture | ported |
| 4 | Save the nominated SpecMap's input count | specmap.c:2290-2292 (fn MapMerge) | specmap.rs:1458 (fn spec_map_merge) | architecture | ported |
| 5 | Initialize the step count to the number in the nominated SpecMap | specmap.c:2293-2296 (fn MapMerge) | specmap.rs:1484 (fn spec_map_merge) | architecture | ported |
| 6 | Search adjacent lower-numbered Mappings for SpecMaps of the same axis count, accumulating step counts | specmap.c:2297-2308 (fn MapMerge) | specmap.rs:1464-1467 (fn spec_map_merge) | architecture | ported |
| 7 | Search adjacent higher-numbered Mappings the same way | specmap.c:2309-2318 (fn MapMerge) | specmap.rs:1468-1471 (fn spec_map_merge) | architecture | ported |
| 8 | Remember the initial number of transformation steps | specmap.c:2319-2321 (fn MapMerge) | specmap.rs:1476 (fn spec_map_merge),1483 (fn spec_map_merge) | architecture | ported |
| 9 | Allocate memory for the accumulated step list | specmap.c:2322-2328 (fn MapMerge) | -- | ceremony | n/a: a `Vec` grows as the steps are appended |
| 10 | Loop to obtain the transformation data for each SpecMap being merged | specmap.c:2329-2332 (fn MapMerge) | specmap.rs:1478-1486 (fn spec_map_merge) | architecture | ported |
| 11 | Take each SpecMap and note whether it is being applied in its inverse direction | specmap.c:2333-2337 (fn MapMerge) | specmap.rs:1479-1482 (fn spec_map_merge) | architecture | ported |
| 12 | Set loop limits and an increment so the steps are scanned forward or in reverse as that direction dictates | specmap.c:2338-2344 (fn MapMerge) | specmap.rs:1369-1371 (fn normalized_steps) | architecture | ported |
| 13 | Loop through each transformation step in the SpecMap | specmap.c:2345-2347 (fn MapMerge) | specmap.rs:1369-1372 (fn normalized_steps) | architecture | ported |
| 14 | Store the step's type code, obtain its argument count from `CvtString`, and copy the arguments | specmap.c:2348-2357 (fn MapMerge) | specmap.rs:1374 (fn normalized_steps),1382 (fn normalized_steps) | architecture | ported |
| 15 | An inverted SpecMap must not only accumulate its steps in reverse but apply each in reverse; define macros for the changes that needs | specmap.c:2358-2364 (fn MapMerge) | specmap.rs:1366-1368 (fn normalized_steps),1370 (fn normalized_steps) | architecture | ported |
| 16 | Macro exchanging a transformation type code for its inverse | specmap.c:2365-2373 (fn MapMerge) | specmap.rs:1373 (fn normalized_steps) | architecture | ported |
| 17 | Macro exchanging the code and reciprocating a specified argument | specmap.c:2374-2394 (fn MapMerge) | specmap.rs:1376-1382 (fn normalized_steps) | architecture | ported |
| 18 | Macro exchanging the code and negating a specified argument | specmap.c:2395-2411 (fn MapMerge) | -- | ceremony | n/a: `SWAP_CODES3` is defined at 2374 and undefined at 2419 without ever being invoked, so no conversion in the class uses it |
| 19 | Apply the macros only where the SpecMap is inverted | specmap.c:2412-2414 (fn MapMerge) | specmap.rs:1366-1368 (fn normalized_steps) | architecture | ported |
| 20 | Exchange the nine code pairs whose inversion leaves the arguments alone | specmap.c:2415-2425 (fn MapMerge) | specmap.rs:410-421 (fn inverse) | architecture | ported |
| 21 | Exchange the eight code pairs whose inversion also reciprocates the frequency correction factor | specmap.c:2426-2438 (fn MapMerge) | specmap.rs:1376-1382 (fn normalized_steps) | architecture | ported |
| 22 | Undefine the local macros | specmap.c:2439-2443 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 23 | Count the accumulated transformation steps | specmap.c:2444-2448 (fn MapMerge) | specmap.rs:1485 (fn spec_map_merge) | architecture | ported |
| 24 | Loop, simplifying the step sequence until no further improvement is possible | specmap.c:2449-2453 (fn MapMerge) | specmap.rs:1416-1433 (fn simplify_steps) | architecture | ported |
| 25 | Examine each remaining transformation step in turn | specmap.c:2454-2457 (fn MapMerge) | specmap.rs:1419 (fn simplify_steps) | architecture | ported |
| 26 | Initially assume the current step will be retained | specmap.c:2458-2460 (fn MapMerge) | -- | ceremony | n/a: the Rust pass removes the eliminated steps instead of marking the survivors |
| 27 | Every simplification in this class combines adjacent steps, so apply them only while two steps remain | specmap.c:2461-2465 (fn MapMerge) | specmap.rs:1419 (fn simplify_steps) | architecture | ported |
| 28 | Macro testing whether two adjacent type codes have given values | specmap.c:2466-2471 (fn MapMerge) | specmap.rs:1343 (fn steps_cancel) | architecture | ported |
| 29 | Macro testing the same pair either way round | specmap.c:2472-2477 (fn MapMerge) | specmap.rs:1343 (fn steps_cancel) | architecture | ported |
| 30 | Eliminate a correction followed by its inverse, starting with the conversions that have no user-supplied arguments | specmap.c:2478-2492 (fn MapMerge) | specmap.rs:1347-1348 (fn steps_cancel) | architecture | ported |
| 31 | The same for conversions with a single user-supplied argument | specmap.c:2493-2499 (fn MapMerge) | specmap.rs:1347-1354 (fn steps_cancel) | architecture | ported |
| 32 | The same with two user-supplied arguments | specmap.c:2500-2511 (fn MapMerge) | specmap.rs:1347-1354 (fn steps_cancel) | architecture | ported |
| 33 | The same with three user-supplied arguments | specmap.c:2512-2524 (fn MapMerge) | specmap.rs:1347-1354 (fn steps_cancel) | architecture | ported |
| 34 | The same with six user-supplied arguments | specmap.c:2525-2544 (fn MapMerge) | specmap.rs:1347-1354 (fn steps_cancel) | architecture | ported |
| 35 | Undefine the local macros | specmap.c:2545-2549 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 36 | A step being kept advances the index identifying its new location | specmap.c:2550-2555 (fn MapMerge) | specmap.rs:1427 (fn simplify_steps) | architecture | ported |
| 37 | Copy the step's data down when its new location differs from its current one | specmap.c:2556-2568 (fn MapMerge) | specmap.rs:1421-1422 (fn simplify_steps) | architecture | ported |
| 38 | Quit when a pass reduced no steps | specmap.c:2569-2573 (fn MapMerge) | specmap.rs:1430-1432 (fn simplify_steps) | architecture | ported |
| 39 | Note how many transformation steps now remain | specmap.c:2574-2577 (fn MapMerge) | -- | ceremony | n/a: the surviving steps are the `Vec`'s contents, so its length is the count |
| 40 | Determine how many Mappings condensing the run eliminates | specmap.c:2578-2582 (fn MapMerge) | specmap.rs:1488 (fn spec_map_merge) | architecture | ported |
| 41 | Determine whether the replacement can be a UnitMap, which needs every step eliminated | specmap.c:2583-2587 (fn MapMerge) | specmap.rs:1490 (fn spec_map_merge) | architecture | ported |
| 42 | Determine whether simplification is possible, on any of the four clauses | specmap.c:2588-2596 (fn MapMerge) | specmap.rs:1492 (fn spec_map_merge) | architecture | ported |
| 43 | Do nothing more unless simplification is possible | specmap.c:2597-2599 (fn MapMerge) | specmap.rs:1493-1495 (fn spec_map_merge) | architecture | ported |
| 44 | Create the UnitMap replacement where that is the answer | specmap.c:2600-2604 (fn MapMerge) | specmap.rs:1497-1498 (fn spec_map_merge) | architecture | ported |
| 45 | Otherwise create a replacement SpecMap and add each surviving step to it | specmap.c:2605-2614 (fn MapMerge) | specmap.rs:1502-1513 (fn spec_map_merge) | architecture | ported |
| 46 | Annul the pointers to the Mappings being eliminated | specmap.c:2615-2620 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 47 | Insert the replacement pointer with a cleared invert value | specmap.c:2621-2624 (fn MapMerge) | specmap.rs:1514 (fn spec_map_merge) | architecture | ported |
| 48 | Move the subsequent Mapping information down to close the gap | specmap.c:2625-2630 (fn MapMerge) | specmap.rs:1514 (fn spec_map_merge) | architecture | ported |
| 49 | Blank out the information left at the end of the arrays | specmap.c:2631-2636 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entries leave no slots to blank |
| 50 | Decrement the Mapping count and return the index of the first eliminated Mapping | specmap.c:2637-2641 (fn MapMerge) | specmap.rs:1514-1515 (fn spec_map_merge) | architecture | ported |
| 51 | Annul the new Mapping if an error occurred | specmap.c:2642-2648 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 52 | Free the memory used for the transformation steps | specmap.c:2649-2655 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 53 | Clear the returned value if an error occurred | specmap.c:2656-2658 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 54 | Return the result | specmap.c:2659-2660 (fn MapMerge) | specmap.rs:1515 (fn spec_map_merge) | ceremony | ported |

## Rows 4, 6 and 7 — C's axis-count guard

C stops the run at a SpecMap whose `astGetNin` differs from the nominee's: a
SpecMap has 1 or 3 axes (`specmap.c:4440-4445 (fn astInitSpecMap_)`). The port
saves the nominee's count and `is_spec` tests it beside the class
(`specmap.rs:1458-1460 (fn spec_map_merge)`), and the SpecMap or UnitMap that
replaces the run has that many axes.

In a *series* list the guard does not change the result, since a SpecMap's input
and output counts are equal, so adjacent SpecMaps already agree. It is kept
because C has it.

## Rows 30 to 34 — the argument count is read from the type

C writes a separate test per argument count because its macro needs the count
spelled out. `steps_cancel` asks the conversion type for its `nargs()`
(`specmap.rs:1347 (fn steps_cancel)`) and compares that many, which covers none, one, two,
three and six without enumerating them. The cached arguments beyond `nargs()` are
excluded from the comparison, which is C's "we do not need to test values stored
internally".

`reset_cached_args` then clears those slots on the rebuilt SpecMap
(`specmap.rs:1403-1406 (fn reset_cached_args)`), which is what C's `AddSpecCvt` does when a
step is added to a new SpecMap (`specmap.c:731 (fn AddSpecCvt)`), so a cached correction
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
table (`specmap.rs:410-421 (fn inverse)`) correspondingly has no negating case.

## Row 3 — the `*nmap == 1` disjunct is unreachable

C's guard admits a parallel nomination when the list holds a single Mapping. The
only caller that passes `series == 0` is a parallel CmpMap's own `MapMerge`, whose
list always holds both components, and the base `Simplify` nominates with
`series = 1` (`mapping.c:16997-17009 (fn Simplify)`). So the disjunct cannot be reached, and
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

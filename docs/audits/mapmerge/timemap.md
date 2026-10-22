# `MapMerge` audit — TimeMap

C source: reference/starlink-ast/src/timemap.c
Rust: crates/ast-core/src/mappings/timemap.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- timemap.c:2078 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `timemap.c:2109-2470 (fn MapMerge)`, the same architecture as SlaMap and
SpecMap: gather the conversion steps of a run of adjacent TimeMaps, normalize
each for its applied direction, drop no-effect steps and cancel adjacent inverse
pairs until nothing more goes, then rebuild one TimeMap or a UnitMap.

`TimeMap::map_merge` (`timemap.rs:1508-1515 (fn map_merge)`) delegates to `time_map_merge`
(`timemap.rs:1344-1394 (fn time_map_merge)`); the helpers are `time_normalized_steps`
(`timemap.rs:1194-1233 (fn time_normalized_steps)`), `time_steps_cancel`
(`timemap.rs:1236-1273 (fn time_steps_cancel)`), `time_step_is_no_effect`
(`timemap.rs:1278-1290 (fn time_step_is_no_effect)`) and `time_simplify_steps`
(`timemap.rs:1297-1334 (fn time_simplify_steps)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialize the returned result to -1 | timemap.c:2109-2111 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | timemap.c:2112-2114 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | TimeMaps merge only in series, or when only one Mapping is present | timemap.c:2115-2119 (fn MapMerge) | timemap.rs:1349-1351 (fn time_map_merge) | architecture | ported |
| 4 | Initialize the step count to the number in the nominated TimeMap | timemap.c:2120-2123 (fn MapMerge) | timemap.rs:1366 (fn time_map_merge),1372 (fn time_map_merge) | architecture | ported |
| 5 | Search adjacent lower-numbered Mappings for TimeMaps, accumulating their step counts | timemap.c:2124-2134 (fn MapMerge) | timemap.rs:1356-1359 (fn time_map_merge) | architecture | ported |
| 6 | Search adjacent higher-numbered Mappings the same way | timemap.c:2135-2143 (fn MapMerge) | timemap.rs:1360-1363 (fn time_map_merge) | architecture | ported |
| 7 | Remember the initial number of transformation steps | timemap.c:2144-2146 (fn MapMerge) | timemap.rs:1366 (fn time_map_merge),1372 (fn time_map_merge) | architecture | ported |
| 8 | Allocate memory for the accumulated step list | timemap.c:2147-2153 (fn MapMerge) | -- | ceremony | n/a: a `Vec` grows as the steps are appended |
| 9 | Loop to obtain the transformation data for each TimeMap being merged | timemap.c:2154-2157 (fn MapMerge) | timemap.rs:1368-1375 (fn time_map_merge) | architecture | ported |
| 10 | Take each TimeMap and note whether it is being applied in its inverse direction | timemap.c:2158-2162 (fn MapMerge) | timemap.rs:1369-1371 (fn time_map_merge) | architecture | ported |
| 11 | Set loop limits and an increment so the steps are scanned forward or in reverse as that direction dictates | timemap.c:2163-2169 (fn MapMerge) | timemap.rs:1198-1200 (fn time_normalized_steps) | architecture | ported |
| 12 | Loop through each transformation step in the TimeMap | timemap.c:2170-2172 (fn MapMerge) | timemap.rs:1198-1201 (fn time_normalized_steps) | architecture | ported |
| 13 | Store the step's type code, obtain its argument count from `CvtString`, and copy the arguments | timemap.c:2173-2183 (fn MapMerge) | timemap.rs:1213-1223 (fn time_normalized_steps) | architecture | ported |
| 14 | An inverted TimeMap must not only accumulate its steps in reverse but apply each in reverse; define macros for the changes that needs | timemap.c:2184-2190 (fn MapMerge) | timemap.rs:1195-1197 (fn time_normalized_steps),1200-1201 (fn time_normalized_steps) | architecture | ported |
| 15 | Macro exchanging a transformation type code for its inverse | timemap.c:2191-2201 (fn MapMerge) | timemap.rs:1202 (fn time_normalized_steps),1225 (fn time_normalized_steps) | architecture | ported |
| 16 | Macro exchanging the code and swapping the order of its two arguments | timemap.c:2202-2218 (fn MapMerge) | timemap.rs:1203-1219 (fn time_normalized_steps) | architecture | ported |
| 17 | Apply the macros only where the TimeMap is inverted | timemap.c:2219-2221 (fn MapMerge) | timemap.rs:1195-1197 (fn time_normalized_steps) | architecture | ported |
| 18 | Exchange the ten code pairs whose inversion leaves the arguments alone | timemap.c:2222-2233 (fn MapMerge) | timemap.rs:1202 (fn time_normalized_steps) | architecture | ported |
| 19 | Exchange the four code pairs whose inversion also swaps the offset values | timemap.c:2234-2242 (fn MapMerge) | timemap.rs:1203-1219 (fn time_normalized_steps) | architecture | ported |
| 20 | Undefine the local macros | timemap.c:2243-2246 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 21 | Count the accumulated transformation steps | timemap.c:2247-2251 (fn MapMerge) | timemap.rs:1373-1374 (fn time_map_merge) | architecture | ported |
| 22 | Loop, simplifying the step sequence until no further improvement is possible | timemap.c:2252-2256 (fn MapMerge) | timemap.rs:1299-1332 (fn time_simplify_steps) | architecture | ported |
| 23 | Examine each remaining transformation step in turn | timemap.c:2257-2260 (fn MapMerge) | timemap.rs:1303-1319 (fn time_simplify_steps) | architecture | ported |
| 24 | Initially assume the current step will be retained | timemap.c:2261-2263 (fn MapMerge) | -- | ceremony | n/a: the Rust passes remove the eliminated steps instead of marking the survivors |
| 25 | Eliminate a change of system that has no effect: MJDTOMJD, MJDTOJD or JDTOMJD with a zero net offset | timemap.c:2264-2270 (fn MapMerge) | timemap.rs:1278-1290 (fn time_step_is_no_effect) | architecture | ported |
| 26 | Every other simplification combines adjacent steps, so apply them only while two steps remain | timemap.c:2271-2275 (fn MapMerge) | timemap.rs:1304 (fn time_simplify_steps) | architecture | ported |
| 27 | Macro testing whether two adjacent type codes have given values | timemap.c:2276-2281 (fn MapMerge) | timemap.rs:1241 (fn time_steps_cancel) | architecture | ported |
| 28 | Macro testing the same pair either way round | timemap.c:2282-2287 (fn MapMerge) | timemap.rs:1241 (fn time_steps_cancel),1247-1256 (fn time_steps_cancel) | architecture | ported |
| 29 | Eliminate a correction followed by its inverse where the one user-supplied argument is unchanged | timemap.c:2288-2302 (fn MapMerge) | timemap.rs:1266-1272 (fn time_steps_cancel) | architecture | ported |
| 30 | The same for conversions with two user-supplied arguments that inversion swaps | timemap.c:2303-2315 (fn MapMerge) | timemap.rs:1248-1264 (fn time_steps_cancel) | architecture | ported |
| 31 | The same for two user-supplied arguments that inversion does not swap | timemap.c:2316-2325 (fn MapMerge) | timemap.rs:1266-1272 (fn time_steps_cancel) | architecture | ported |
| 32 | The same with three user-supplied arguments | timemap.c:2326-2338 (fn MapMerge) | timemap.rs:1266-1272 (fn time_steps_cancel) | architecture | ported |
| 33 | The same with five user-supplied arguments | timemap.c:2339-2354 (fn MapMerge) | timemap.rs:1266-1272 (fn time_steps_cancel) | architecture | ported |
| 34 | Undefine the local macros | timemap.c:2355-2359 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 35 | A step being kept advances the index identifying its new location | timemap.c:2360-2365 (fn MapMerge) | timemap.rs:1314 (fn time_simplify_steps),1324 (fn time_simplify_steps) | architecture | ported |
| 36 | Copy the step's data down when its new location differs from its current one | timemap.c:2366-2378 (fn MapMerge) | timemap.rs:1306-1307 (fn time_simplify_steps),1320 (fn time_simplify_steps) | architecture | ported |
| 37 | Quit when a pass reduced no steps | timemap.c:2379-2383 (fn MapMerge) | timemap.rs:1329-1331 (fn time_simplify_steps) | architecture | ported |
| 38 | Note how many transformation steps now remain | timemap.c:2384-2387 (fn MapMerge) | -- | ceremony | n/a: the surviving steps are the `Vec`'s contents, so its length is the count |
| 39 | Determine how many Mappings condensing the run eliminates | timemap.c:2388-2392 (fn MapMerge) | timemap.rs:1377 (fn time_map_merge) | architecture | ported |
| 40 | Determine whether the replacement can be a UnitMap, which needs every step eliminated | timemap.c:2393-2397 (fn MapMerge) | timemap.rs:1379 (fn time_map_merge) | architecture | ported |
| 41 | Determine whether simplification is possible, on any of the four clauses | timemap.c:2398-2406 (fn MapMerge) | timemap.rs:1380 (fn time_map_merge) | architecture | ported |
| 42 | Do nothing more unless simplification is possible | timemap.c:2407-2409 (fn MapMerge) | timemap.rs:1381-1383 (fn time_map_merge) | architecture | ported |
| 43 | Create the UnitMap replacement where that is the answer | timemap.c:2410-2414 (fn MapMerge) | timemap.rs:1385-1386 (fn time_map_merge) | architecture | ported |
| 44 | Otherwise create a replacement TimeMap and add each surviving step to it | timemap.c:2415-2424 (fn MapMerge) | timemap.rs:1387-1391 (fn time_map_merge) | architecture | ported |
| 45 | Annul the pointers to the Mappings being eliminated | timemap.c:2425-2430 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 46 | Insert the replacement pointer with a cleared invert value | timemap.c:2431-2434 (fn MapMerge) | timemap.rs:1392 (fn time_map_merge) | architecture | ported |
| 47 | Move the subsequent Mapping information down to close the gap | timemap.c:2435-2440 (fn MapMerge) | timemap.rs:1392 (fn time_map_merge) | architecture | ported |
| 48 | Blank out the information left at the end of the arrays | timemap.c:2441-2446 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entries leave no slots to blank |
| 49 | Decrement the Mapping count and return the index of the first eliminated Mapping | timemap.c:2447-2451 (fn MapMerge) | timemap.rs:1392-1393 (fn time_map_merge) | architecture | ported |
| 50 | Annul the new Mapping if an error occurred | timemap.c:2452-2458 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 51 | Free the memory used for the transformation steps | timemap.c:2459-2465 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 52 | Clear the returned value if an error occurred | timemap.c:2466-2468 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 53 | Return the result | timemap.c:2469-2470 (fn MapMerge) | timemap.rs:1393 (fn time_map_merge) | ceremony | ported |

## Row 25 — the test is on the derived offset, not the supplied pair

C drops a step whose derived net offset is zero, and applies that to all three of
`AST__MJDTOMJD`, `AST__MJDTOJD` and `AST__JDTOMJD`, testing `cvtargs[istep][2]`.
`time_step_is_no_effect` tests the same three types against the same `args[2]`.

Testing the supplied pair instead would agree only for `MjdToMjd`, where
`args[2] = args[0] - args[1]`. For the other two, `args[2]` carries the 2400000.5
constant (`timemap.c:511 (fn AddArgs)`,511), so an `MjdToJd` whose output offset exactly
absorbs that constant is a no-op only the derived value can see.

## Rows 29 to 33 — the swap set is what distinguishes the argument counts

C separates two-argument conversions into those inversion swaps and those it does
not, and then handles three and five arguments. `time_steps_cancel`
(`timemap.rs:1236-1273 (fn time_steps_cancel)`) carries the swapping set explicitly
(`timemap.rs:1248-1257 (fn time_steps_cancel)`) and otherwise compares `nargs()` arguments in
order, which covers the remaining counts without enumerating them. The set is the
same one row 19 uses for the inversion itself, which is why the two lists agree
term for term.

## Rows 23, 35 and 36 — compaction in place, or removal

C rewrites the step array in place: a surviving step is copied down over the
eliminated ones and `ikeep` tracks where the next survivor lands, so the pass
ends with the survivors packed at the front and `nstep` reset to `ikeep + 1`.
`time_simplify_steps` removes the eliminated steps from the `Vec` instead, which
packs the survivors by construction. The two agree on the surviving sequence
because both keep the steps in their original relative order and both consume the
second step of a cancelling pair — C by advancing `istep`, the port by removing
both entries at once.

## Row 3 — the `*nmap == 1` disjunct is unreachable

C's guard admits a parallel nomination when the list holds a single Mapping. The
only caller that passes `series == 0` is a parallel CmpMap's own `MapMerge`, whose
list always holds both components, and the base `Simplify` nominates with
`series = 1` (`mapping.c:16871-16883 (fn Simplify)`). So the disjunct cannot be reached, and
`time_map_merge` delegating every parallel nomination to `default_map_merge` is
equivalent rather than narrower.

## A lone TimeMap reaches the same body

C gives TimeMap no `Simplify` vtab slot, so a TimeMap nominated on its own arrives
here as a run of one: `ngone` is zero and the `simpler` test reduces to "the step
list shortened, emptied, or the TimeMap is applied inverted". Dropping no-effect
steps, cancelling adjacent inverse pairs and rebuilding forward is every reduction
a lone TimeMap has. The `where_inv` term is the list's flag, not the object's,
since `flatten` resets every leaf to forward.

# `MapMerge` audit — TimeMap

C source: reference/starlink-ast/src/timemap.c
Rust: crates/ast-core/src/mappings/timemap.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3e60abaa
Extension points: classified

Uncited C steps:
- timemap.c:2074 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step

C's body runs `timemap.c:2105-2466 (fn MapMerge)`, the same architecture as SlaMap and
SpecMap: gather the conversion steps of a run of adjacent TimeMaps, normalise
each for its applied direction, drop no-effect steps and cancel adjacent inverse
pairs until nothing more goes, then rebuild one TimeMap or a UnitMap.

`TimeMap::map_merge` (`timemap.rs:1371-1378 (fn map_merge)`) delegates to `time_map_merge`
(`timemap.rs:1231-1281 (fn time_map_merge)`); the helpers are `time_normalized_steps`
(`timemap.rs:1081-1120 (fn time_normalized_steps)`), `time_steps_cancel`
(`timemap.rs:1123-1160 (fn time_steps_cancel)`), `time_step_is_no_effect`
(`timemap.rs:1165-1177 (fn time_step_is_no_effect)`) and `time_simplify_steps`
(`timemap.rs:1184-1221 (fn time_simplify_steps)`).

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | timemap.c:2105-2107 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | timemap.c:2108-2110 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | TimeMaps merge only in series, or when only one Mapping is present | timemap.c:2111-2115 (fn MapMerge) | timemap.rs:1236-1238 (fn time_map_merge) | architecture | ported |
| 4 | Initialise the step count to the number in the nominated TimeMap | timemap.c:2116-2119 (fn MapMerge) | timemap.rs:1253 (fn time_map_merge),1260 | architecture | ported |
| 5 | Search adjacent lower-numbered Mappings for TimeMaps, accumulating their step counts | timemap.c:2120-2130 (fn MapMerge) | timemap.rs:1243-1246 (fn time_map_merge) | architecture | ported |
| 6 | Search adjacent higher-numbered Mappings the same way | timemap.c:2131-2139 (fn MapMerge) | timemap.rs:1247-1250 (fn time_map_merge) | architecture | ported |
| 7 | Remember the initial number of transformation steps | timemap.c:2140-2142 (fn MapMerge) | timemap.rs:1253 (fn time_map_merge),1260 | architecture | ported |
| 8 | Allocate memory for the accumulated step list | timemap.c:2143-2149 (fn MapMerge) | -- | ceremony | n/a: a `Vec` grows as the steps are appended |
| 9 | Loop to obtain the transformation data for each TimeMap being merged | timemap.c:2150-2153 (fn MapMerge) | timemap.rs:1255-1262 (fn time_map_merge) | architecture | ported |
| 10 | Take each TimeMap and note whether it is being applied in its inverse direction | timemap.c:2154-2158 (fn MapMerge) | timemap.rs:1256-1258 (fn time_map_merge) | architecture | ported |
| 11 | Set loop limits and an increment so the steps are scanned forward or in reverse as that direction dictates | timemap.c:2159-2165 (fn MapMerge) | timemap.rs:1085-1087 (fn time_normalized_steps) | architecture | ported |
| 12 | Loop through each transformation step in the TimeMap | timemap.c:2166-2168 (fn MapMerge) | timemap.rs:1085-1088 (fn time_normalized_steps) | architecture | ported |
| 13 | Store the step's type code, obtain its argument count from `CvtString`, and copy the arguments | timemap.c:2169-2179 (fn MapMerge) | timemap.rs:1100-1110 (fn time_normalized_steps) | architecture | ported |
| 14 | An inverted TimeMap must not only accumulate its steps in reverse but apply each in reverse; define macros for the changes that needs | timemap.c:2180-2186 (fn MapMerge) | timemap.rs:1082-1084 (fn time_normalized_steps),1088-1089 | architecture | ported |
| 15 | Macro exchanging a transformation type code for its inverse | timemap.c:2187-2197 (fn MapMerge) | timemap.rs:1089 (fn time_normalized_steps),1113 | architecture | ported |
| 16 | Macro exchanging the code and swapping the order of its two arguments | timemap.c:2198-2214 (fn MapMerge) | timemap.rs:1090-1106 (fn time_normalized_steps) | architecture | ported |
| 17 | Apply the macros only where the TimeMap is inverted | timemap.c:2215-2217 (fn MapMerge) | timemap.rs:1082-1084 (fn time_normalized_steps) | architecture | ported |
| 18 | Exchange the ten code pairs whose inversion leaves the arguments alone | timemap.c:2218-2229 (fn MapMerge) | timemap.rs:1089 (fn time_normalized_steps) | architecture | ported |
| 19 | Exchange the four code pairs whose inversion also swaps the offset values | timemap.c:2230-2238 (fn MapMerge) | timemap.rs:1090-1106 (fn time_normalized_steps) | architecture | ported |
| 20 | Undefine the local macros | timemap.c:2239-2242 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 21 | Count the accumulated transformation steps | timemap.c:2243-2247 (fn MapMerge) | timemap.rs:1260-1261 (fn time_map_merge) | architecture | ported |
| 22 | Loop, simplifying the step sequence until no further improvement is possible | timemap.c:2248-2252 (fn MapMerge) | timemap.rs:1186-1219 (fn time_simplify_steps) | architecture | ported |
| 23 | Examine each remaining transformation step in turn | timemap.c:2253-2256 (fn MapMerge) | timemap.rs:1190-1206 (fn time_simplify_steps) | architecture | ported |
| 24 | Initially assume the current step will be retained | timemap.c:2257-2259 (fn MapMerge) | -- | ceremony | n/a: the Rust passes remove the eliminated steps instead of marking the survivors |
| 25 | Eliminate a change of system that has no effect: MJDTOMJD, MJDTOJD or JDTOMJD with a zero net offset | timemap.c:2260-2266 (fn MapMerge) | timemap.rs:1165-1177 (fn time_step_is_no_effect) | architecture | ported |
| 26 | Every other simplification combines adjacent steps, so apply them only while two steps remain | timemap.c:2267-2271 (fn MapMerge) | timemap.rs:1191 (fn time_simplify_steps) | architecture | ported |
| 27 | Macro testing whether two adjacent type codes have given values | timemap.c:2272-2277 (fn MapMerge) | timemap.rs:1128 (fn time_steps_cancel) | architecture | ported |
| 28 | Macro testing the same pair either way round | timemap.c:2278-2283 (fn MapMerge) | timemap.rs:1128 (fn time_steps_cancel),1135-1144 | architecture | ported |
| 29 | Eliminate a correction followed by its inverse where the one user-supplied argument is unchanged | timemap.c:2284-2298 (fn MapMerge) | timemap.rs:1153-1159 (fn time_steps_cancel) | architecture | ported |
| 30 | The same for conversions with two user-supplied arguments that inversion swaps | timemap.c:2299-2311 (fn MapMerge) | timemap.rs:1135-1151 (fn time_steps_cancel) | architecture | ported |
| 31 | The same for two user-supplied arguments that inversion does not swap | timemap.c:2312-2321 (fn MapMerge) | timemap.rs:1153-1159 (fn time_steps_cancel) | architecture | ported |
| 32 | The same with three user-supplied arguments | timemap.c:2322-2334 (fn MapMerge) | timemap.rs:1153-1159 (fn time_steps_cancel) | architecture | ported |
| 33 | The same with five user-supplied arguments | timemap.c:2335-2350 (fn MapMerge) | timemap.rs:1153-1159 (fn time_steps_cancel) | architecture | ported |
| 34 | Undefine the local macros | timemap.c:2351-2355 (fn MapMerge) | -- | ceremony | n/a: the Rust code has no macros to scope |
| 35 | A step being kept advances the index identifying its new location | timemap.c:2356-2361 (fn MapMerge) | timemap.rs:1201 (fn time_simplify_steps),1212 | architecture | ported |
| 36 | Copy the step's data down when its new location differs from its current one | timemap.c:2362-2374 (fn MapMerge) | timemap.rs:1193-1194 (fn time_simplify_steps),1208 | architecture | ported |
| 37 | Quit when a pass reduced no steps | timemap.c:2375-2379 (fn MapMerge) | timemap.rs:1216-1218 (fn time_simplify_steps) | architecture | ported |
| 38 | Note how many transformation steps now remain | timemap.c:2380-2383 (fn MapMerge) | -- | ceremony | n/a: the surviving steps are the `Vec`'s contents, so its length is the count |
| 39 | Determine how many Mappings condensing the run eliminates | timemap.c:2384-2388 (fn MapMerge) | timemap.rs:1264 (fn time_map_merge) | architecture | ported |
| 40 | Determine whether the replacement can be a UnitMap, which needs every step eliminated | timemap.c:2389-2393 (fn MapMerge) | timemap.rs:1266 (fn time_map_merge) | architecture | ported |
| 41 | Determine whether simplification is possible, on any of the four clauses | timemap.c:2394-2402 (fn MapMerge) | timemap.rs:1267 (fn time_map_merge) | architecture | ported |
| 42 | Do nothing more unless simplification is possible | timemap.c:2403-2405 (fn MapMerge) | timemap.rs:1268-1270 (fn time_map_merge) | architecture | ported |
| 43 | Create the UnitMap replacement where that is the answer | timemap.c:2406-2410 (fn MapMerge) | timemap.rs:1272-1273 (fn time_map_merge) | architecture | ported |
| 44 | Otherwise create a replacement TimeMap and add each surviving step to it | timemap.c:2411-2420 (fn MapMerge) | timemap.rs:1274-1278 (fn time_map_merge) | architecture | ported |
| 45 | Annul the pointers to the Mappings being eliminated | timemap.c:2421-2426 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 46 | Insert the replacement pointer with a cleared invert value | timemap.c:2427-2430 (fn MapMerge) | timemap.rs:1279 (fn time_map_merge) | architecture | ported |
| 47 | Move the subsequent Mapping information down to close the gap | timemap.c:2431-2436 (fn MapMerge) | timemap.rs:1279 (fn time_map_merge) | architecture | ported |
| 48 | Blank out the information left at the end of the arrays | timemap.c:2437-2442 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entries leave no slots to blank |
| 49 | Decrement the Mapping count and return the index of the first eliminated Mapping | timemap.c:2443-2447 (fn MapMerge) | timemap.rs:1279-1280 (fn time_map_merge) | architecture | ported |
| 50 | Annul the new Mapping if an error occurred | timemap.c:2448-2454 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 51 | Free the memory used for the transformation steps | timemap.c:2455-2461 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 52 | Clear the returned value if an error occurred | timemap.c:2462-2464 (fn MapMerge) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 53 | Return the result | timemap.c:2465-2466 (fn MapMerge) | timemap.rs:1280 (fn time_map_merge) | ceremony | ported |

## Row 25 — the test is on the derived offset, not the supplied pair

C drops a step whose derived net offset is zero, and applies that to all three of
`AST__MJDTOMJD`, `AST__MJDTOJD` and `AST__JDTOMJD`, testing `cvtargs[istep][2]`.
`time_step_is_no_effect` tests the same three types against the same `args[2]`.

Testing the supplied pair instead would agree only for `MjdToMjd`, where
`args[2] = args[0] - args[1]`. For the other two, `args[2]` carries the 2400000.5
constant (`timemap.c:507 (fn AddArgs)`,511), so an `MjdToJd` whose output offset exactly
absorbs that constant is a no-op only the derived value can see.

## Rows 29 to 33 — the swap set is what distinguishes the argument counts

C separates two-argument conversions into those inversion swaps and those it does
not, and then handles three and five arguments. `time_steps_cancel`
(`timemap.rs:1123-1160 (fn time_steps_cancel)`) carries the swapping set explicitly
(`timemap.rs:1135-1144 (fn time_steps_cancel)`) and otherwise compares `nargs()` arguments in
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

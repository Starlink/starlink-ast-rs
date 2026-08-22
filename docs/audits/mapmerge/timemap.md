# `MapMerge` audit — TimeMap

C source: reference/starlink-ast/src/timemap.c
Rust: crates/ast-core/src/mappings/timemap.rs
Audited: 2026-08-21 against pin 3cf4bc5c

C's body runs `timemap.c:2105-2466`, the same architecture as SlaMap and SpecMap:
gather the conversion steps of a run of adjacent TimeMaps, normalise each for its
applied direction, drop no-effect steps and cancel adjacent inverse pairs until
nothing more goes, then rebuild one TimeMap or a UnitMap.

`TimeMap::map_merge` (`timemap.rs:1386-1393`) delegates to `time_map_merge`
(`timemap.rs:1222-1300`); the helpers are `time_normalized_steps`
(`timemap.rs:1079-1120`), `time_steps_cancel` (`timemap.rs:1121-1160`),
`time_step_is_no_effect` (`timemap.rs:1163-1174`) and `time_simplify_steps`
(`timemap.rs:1175-1221`).

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Initialise the returned result to -1 | timemap.c:2105-2106 | -- | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | timemap.c:2108-2109 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | TimeMaps merge only in series, or when there is a single Mapping | timemap.c:2111-2115 | timemap.rs:1227-1228 | ported |
| 4 | Initialise the step count from the nominated TimeMap | timemap.c:2116-2119 | timemap.rs:1243-1244 | ported |
| 5 | Search adjacent lower-numbered Mappings for TimeMaps, accumulating their step counts | timemap.c:2120-2130 | timemap.rs:1234-1237 | ported |
| 6 | Search adjacent higher-numbered Mappings the same way | timemap.c:2131-2139 | timemap.rs:1238-1241 | ported |
| 7 | Remember the initial number of steps | timemap.c:2140-2142 | timemap.rs:1244,1255 | ported |
| 8 | Allocate a list to accumulate every step from every TimeMap found | timemap.c:2143-2149 | timemap.rs:1243 | ported |
| 9 | Take each TimeMap in the run and note whether it is applied inverted | timemap.c:2150-2158 | timemap.rs:1246-1250 | ported |
| 10 | Scan its steps forward or in reverse as the direction requires | timemap.c:2159-2165 | timemap.rs:1079-1120 | ported |
| 11 | Copy each step's type code and its user-supplied arguments | timemap.c:2166-2179 | timemap.rs:1250-1252 | ported |
| 12 | For an inverted TimeMap, also invert each step, not merely reverse their order | timemap.c:2180-2197 | timemap.rs:1079-1120 | ported |
| 13 | Some inversions also swap the order of the two arguments | timemap.c:2198-2214 | timemap.rs:1131-1145 | ported |
| 14 | Exchange each transformation code for its inverse | timemap.c:2215-2229 | timemap.rs:1079-1120 | ported |
| 15 | For the offset conversions, also swap the offset values | timemap.c:2230-2238 | timemap.rs:1131-1145 | ported |
| 16 | Count the accumulated steps | timemap.c:2243-2247 | timemap.rs:1255 | ported |
| 17 | Loop, simplifying the step sequence until no further improvement is possible | timemap.c:2248-2252 | timemap.rs:1175-1221 | ported |
| 18 | Examine each remaining step in turn, assuming at first it will be kept | timemap.c:2253-2259 | timemap.rs:1175-1221 | ported |
| 19 | Eliminate a change of system that has no effect, for MJDTOMJD, MJDTOJD or JDTOMJD with a zero net offset | timemap.c:2260-2266 | timemap.rs:1163-1174 | gap: PLAN.md §10.9 |
| 20 | Every other simplification combines adjacent steps, so apply them only while two remain | timemap.c:2267-2283 | timemap.rs:1175-1221 | ported |
| 21 | Eliminate a correction followed by its inverse when the user-supplied arguments are unchanged | timemap.c:2284-2298 | timemap.rs:1121-1160 | ported |
| 22 | Apply that test to conversions with two arguments that inversion swaps | timemap.c:2299-2311 | timemap.rs:1131-1145 | ported |
| 23 | And to conversions with two arguments that inversion does not swap | timemap.c:2312-2321 | timemap.rs:1151-1159 | ported |
| 24 | With three arguments | timemap.c:2322-2334 | timemap.rs:1151-1159 | ported |
| 25 | With five arguments | timemap.c:2335-2350 | timemap.rs:1151-1159 | ported |
| 26 | Compact the surviving steps down over the eliminated ones | timemap.c:2356-2388 | timemap.rs:1175-1221 | ported |
| 27 | Note how many steps remain, and how many Mappings condensing the run eliminates | timemap.c:2389-2393 | timemap.rs:1255-1258 | ported |
| 28 | Decide whether anything simplified | timemap.c:2394-2402 | timemap.rs:1259 | ported |
| 29 | Do nothing more unless something simplified | timemap.c:2403-2405 | timemap.rs:1260-1262 | ported |
| 30 | Create the UnitMap replacement if that is the answer | timemap.c:2406-2410 | timemap.rs:1264-1266 | ported |
| 31 | Otherwise create a replacement TimeMap holding the surviving steps | timemap.c:2411-2420 | timemap.rs:1267-1272 | ported |
| 32 | Annul the eliminated Mappings and insert the replacement with its invert value | timemap.c:2421-2430 | timemap.rs:1274-1290 | ported |
| 33 | Close the gap, blank the tail, and return the index of the first eliminated Mapping | timemap.c:2431-2447 | timemap.rs:1274-1290 | ported |
| 34 | Annul the new Mapping if an error occurred | timemap.c:2448-2454 | -- | n/a: an error is a `Result::Err`, and owned values are dropped |
| 35 | Free the step list | timemap.c:2455-2461 | -- | n/a: owned values are dropped |
| 36 | Clear the returned value if an error occurred | timemap.c:2462-2464 | -- | n/a: an error is a `Result::Err`, not a value to be cleared alongside one |
| 37 | Return the result | timemap.c:2465-2466 | timemap.rs:1300 | ported |

## Row 19 — two of C's three no-effect cases are missing

C drops a step whose derived net offset is zero, and applies that to all three of
`AST__MJDTOMJD`, `AST__MJDTOJD` and `AST__JDTOMJD`, testing `cvtargs[istep][2]`.
The port tests `args[0] == args[1]` on `MjdToMjd` alone.

For `MjdToMjd` the two tests agree, because `args[2] = args[0] - args[1]`. For
the other two they do not: `args[2]` carries the 2400000.5 constant
(`timemap.c:507,511`), so an `MjdToJd` whose output offset exactly absorbs that
constant is a no-op C removes and the port keeps.

The port already derives `args[2]` the same way (`timemap.rs:492-501`) — it just
does not use it here. See PLAN.md §10.9.

## Rows 22 to 25 — the swap set is what distinguishes the argument counts

C separates two-argument conversions into those inversion swaps and those it does
not, and then handles three and five arguments. `time_steps_cancel`
(`timemap.rs:1121-1160`) carries the swapping set explicitly
(`timemap.rs:1131-1145`) and otherwise compares `nargs()` arguments in order,
which covers the rest without enumerating counts.

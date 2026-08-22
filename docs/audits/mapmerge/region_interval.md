# `Simplify` / `MapMerge` / `GetDefUnc` audit — Interval

C source: reference/starlink-ast/src/interval.c
Rust: crates/ast-core/src/regions/interval.rs
Audited: 2026-08-21 against pin 3cf4bc5c

Three C bodies. `MapMerge` (`interval.c:1146-1221`) is the Region shape: replace
with a simpler Region, else merge with a neighbour in parallel. `Simplify`
(`interval.c:3184-3525`) either defers to the equivalent Box or, for an unbounded
Interval, re-defines it in its current Frame by transforming a bounding box and
reading the transformed limits back. `GetDefUnc` (`interval.c:658-703`) builds the
default uncertainty from the Interval's own limits.

`Interval::map_merge` (`interval.rs:1484`) delegates to
`simplify::region_map_merge`; `Interval::simplified` is `interval.rs:1371-1449`,
whose remap half is the `simplify_remapped` override at `interval.rs:203-424`.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | `MapMerge`: simplify the Interval, assuming a changed pointer means something happened | interval.c:1146-1150 | simplify.rs:1292-1301 | ported |
| 2 | Replace it in the list with the new Region, marking it forward | interval.c:1151-1157 | simplify.rs:1298-1299 | ported |
| 3 | Return the index of the first modified element | interval.c:1158-1160 | simplify.rs:1299 | ported |
| 4 | Otherwise merge with a neighbouring Region, in parallel only | interval.c:1161-1167 | simplify.rs:1303-1305 | ported |
| 5 | Try the lower neighbour first | interval.c:1168-1177 | simplify.rs:1306-1315 | ported |
| 6 | Failing that, try the upper neighbour | interval.c:1178-1188 | simplify.rs:1316-1330 | ported |
| 7 | On success, replace the first of the pair with the merged Region and clear its invert flag | interval.c:1189-1197 | simplify.rs:1331-1345 | ported |
| 8 | Annul the second and shuffle the rest down, clearing the vacated slot | interval.c:1198-1209 | simplify.rs:1331-1345 | ported |
| 9 | Decrement the Mapping count and return the index of the first modified element | interval.c:1210-1219 | simplify.rs:1345 | ported |
| 10 | Return the result | interval.c:1220-1221 | simplify.rs:1349 | ported |
| 11 | `Simplify`: if the Interval is equivalent to a Box, use the Box's implementation | interval.c:3196-3201 | interval.rs:1373-1406 | ported |
| 12 | Otherwise use the implementation for unbounded intervals | interval.c:3202-3205 | interval.rs:1408-1449 | ported |
| 13 | Invoke the parent Region simplify | interval.c:3206-3210 | interval.rs:1409-1412 | ported |
| 14 | Note whether that simplified anything | interval.c:3211-3215 | interval.rs:1412-1425 | ported |
| 15 | If the base-to-current Mapping is not a UnitMap, re-define the Interval in its current Frame | interval.c:3216-3220 | interval.rs:1426-1440 | ported |
| 16 | Copy the bounds, replacing missing limits with arbitrary non-BAD values | interval.c:3221-3235 | interval.rs:211-241 | ported |
| 17 | Ensure a good upper bound on each axis | interval.c:3236-3251 | interval.rs:211-241 | ported |
| 18 | Ensure a good lower bound on each axis | interval.c:3252-3266 | interval.rs:211-241 | ported |
| 19 | Transform the corners into the current Frame and back, and grow the box so it encloses both | interval.c:3267-3297 | interval.rs:242-270 | ported |
| 20 | Limit that box so it does not exceed the Interval's own limits | interval.c:3298-3325 | interval.rs:271-307 | ported |
| 21 | Ensure the bounds are not equal on any axis | interval.c:3326-3334 | interval.rs:308-316 | ported |
| 22 | Create a Box for the box found above | interval.c:3335-3340 | interval.rs:317-367 | ported |
| 23 | Give that Box the same current Frame as this Interval | interval.c:3341-3344 | interval.rs:317-367 | ported |
| 24 | Try simplifying the Box, proceeding only if it changed | interval.c:3345-3350 | interval.rs:317-367 | ported |
| 25 | Return the simplified Box straight away if it is a NullRegion | interval.c:3351-3356 | interval.rs:368-424 | ported |
| 26 | Otherwise handle a simplified Box or Interval, reading its bounds | interval.c:3357-3376 | interval.rs:368-424 | ported |
| 27 | Build a PointSet of the lower bounds plus one point displaced along each axis | interval.c:3377-3395 | interval.rs:368-424 | ported |
| 28 | Transform it into this Interval's base Frame through the inverse Mapping | interval.c:3396-3401 | interval.rs:368-424 | ported |
| 29 | For each current-Frame axis, find the base-Frame axis it corresponds to | interval.c:3402-3426 | interval.rs:368-424 | ported |
| 30 | A zero largest increment means that axis is absent from the base Frame, so its limits are unconstrained | interval.c:3427-3442 | interval.rs:368-424 | ported |
| 31 | With a corresponding axis found, see which of its limits were specified | interval.c:3443-3449 | interval.rs:368-424 | ported |
| 32 | Keep equal limits equal, undoing the adjustment made earlier | interval.c:3450-3455 | interval.rs:368-424 | ported |
| 33 | Keep an inclusion an inclusion, swapping the limits if needed | interval.c:3456-3464 | interval.rs:368-424 | ported |
| 34 | Keep an exclusion an exclusion likewise | interval.c:3465-3476 | interval.rs:368-424 | ported |
| 35 | Create the simplified Interval from those current-Frame limits | interval.c:3477-3485 | interval.rs:368-424 | ported |
| 36 | Free resources | interval.c:3486-3506 | -- | n/a: owned values are dropped |
| 37 | If anything simplified, overlay the original Region's attributes and return it | interval.c:3507-3520 | interval.rs:1441-1448 | ported |
| 38 | Annul the returned pointer if an error occurred | interval.c:3521-3523 | -- | n/a: an error is a `Result::Err` |
| 39 | Return the result | interval.c:3524-3525 | interval.rs:1448 | ported |
| 40 | `GetDefUnc`: if the Interval is equivalent to a Box, use that Box's default uncertainty | interval.c:583-657 | interval.rs:1017-1019 | ported |
| 41 | Get the base-Frame axis count and allocate the uncertainty bounds | interval.c:658-665 | interval.rs:1021-1030 | ported |
| 42 | Refresh the cached bounds | interval.c:666-668 | interval.rs:1021-1030 | ported |
| 43 | On an axis with both limits, use 1.0E-6 of the difference between them | interval.c:669-681 | interval.rs:1031-1042 | ported |
| 44 | Otherwise use zero | interval.c:682-688 | interval.rs:1038-1042 | ported |
| 45 | Create the Box | interval.c:689-692 | interval.rs:1043-1050 | ported |
| 46 | Free resources and return NULL on error | interval.c:693-701 | -- | n/a: owned values are dropped and an error is a `Result::Err` |
| 47 | Return the Box | interval.c:702-703 | interval.rs:1050 | ported |

## Rows 43 and 44 — the guard the base class gets wrong

Interval tests `lb != -f64::MAX && ub != f64::MAX` (`interval.rs:1035`), which is
the correct rendering of C's `±DBL_MAX` test in both directions. The base-class
`default_bounding_box_unc` uses `is_bad` instead and so misses the upper case —
see PLAN.md §10.12. This class is the reference for what that guard should look
like.

## Row 11 — the Box path comes first

C dispatches on Box-equivalence before doing any remap work, and the port keeps
that order (`interval.rs:1373-1406`). It narrows the condition slightly: a
Negated or non-Closed Interval stays an Interval, because `BoxRegion`'s contract
is a closed, non-negated axis-aligned Region. C's `astIntervalBox` reaches the
same place through the Box it constructs.

## Rows 16 to 21 — arbitrary limits, then reined back in

The re-definition has to transform a *bounded* box, so C fills each missing limit
with an arbitrary value, transforms, and then limits the result back to the
Interval's real limits. Skipping the second half would widen the Interval; the
port does both (`interval.rs:211-307`).

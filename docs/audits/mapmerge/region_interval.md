# `Simplify` / `MapMerge` audit — Interval

C source: reference/starlink-ast/src/interval.c
Rust: crates/ast-core/src/regions/interval.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 3e60abaa
Extension points: classified

Uncited C steps:
- interval.c:624 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:1124 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:1265 (fn MergeInterval) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:3145 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Four C bodies. `GetDefUnc` (`interval.c:636-703 (fn GetDefUnc)`) is the default
uncertainty: the equivalent Box's own default where the Interval is a Box, and
otherwise a Box spanning 1.0E-6 of each bounded axis interval. `MapMerge`
(`interval.c:1132-1221 (fn MapMerge)`) is the Region shape shared by every Region
class: self-simplify first, and only failing that merge with a neighbour, in
parallel only. `MergeInterval` (`interval.c:1308-1658 (fn MergeInterval)`) is the
merge it delegates a neighbouring Region to — four arms, one per admissible
neighbour class. `Simplify` (`interval.c:3184-3525 (fn Simplify)`) simplifies the
equivalent Box where there is one, and otherwise re-defines the Interval within
its current Frame by way of a fully-bounded Box.

`Interval::get_def_unc` (`interval.rs:1047-1085 (fn get_def_unc)`) is `GetDefUnc`.
`Interval::map_merge` (`interval.rs:1563-1570 (fn map_merge)`) delegates to
`simplify::region_map_merge` (`mappings/simplify.rs:1443-1490 (fn region_map_merge)`),
which every Region shares. `Interval::region_merge`
(`interval.rs:1167-1343 (fn region_merge)`) is `MergeInterval`, and
`Interval::simplified` (`interval.rs:1463-1528 (fn simplified)`) is `Simplify`, with
its unbounded branch in `simplify_remapped`
(`interval.rs:255-525 (fn simplify_remapped)`).

`MergeInterval` shares its whole tail with `MergeBox` and `MergePointList`, so the
rows from each arm's constructor onwards cite `merge_util.rs`, where that tail
lives once.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `GetDefUnc`: initialise the returned pointer | interval.c:636-638 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no sentinel to initialise |
| 2 | Check the global error status and bail out | interval.c:639-641 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Interval structure | interval.c:642-644 (fn GetDefUnc) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | If this Interval is equivalent to a Box, return that Box's default uncertainty | interval.c:645-650 (fn GetDefUnc) | interval.rs:1047-1050 (fn get_def_unc) | architecture | ported |
| 5 | Otherwise use a box covering 1.0E-6 of each axis interval, centred on the origin | interval.c:651-654 (fn GetDefUnc) | interval.rs:1052-1056 (fn get_def_unc) | architecture | ported |
| 6 | Get a pointer to the base Frame | interval.c:655-657 (fn GetDefUnc) | interval.rs:1057 (fn get_def_unc) | architecture | ported |
| 7 | Get the number of base Frame axes | interval.c:658-660 (fn GetDefUnc) | interval.rs:1056 (fn get_def_unc) | architecture | ported |
| 8 | Allocate arrays to hold the bounds of the uncertainty Box | interval.c:661-665 (fn GetDefUnc) | interval.rs:1059-1060 (fn get_def_unc) | ceremony | ported |
| 9 | Ensure the cached bounds are up to date | interval.c:666-668 (fn GetDefUnc) | interval.rs:1047 (fn get_def_unc) | architecture | ported |
| 10 | Do each axis in turn | interval.c:669-671 (fn GetDefUnc) | interval.rs:1061 (fn get_def_unc) | architecture | ported |
| 11 | Where the axis has both limits, use 1.0E-6 of their difference about their mean, falling back to a fraction of the centre when that difference is zero | interval.c:672-681 (fn GetDefUnc) | interval.rs:1064-1071 (fn get_def_unc) | architecture | ported |
| 12 | Otherwise use zero for both bounds | interval.c:682-688 (fn GetDefUnc) | interval.rs:1072-1075 (fn get_def_unc) | architecture | ported |
| 13 | Create the Box | interval.c:689-692 (fn GetDefUnc) | interval.rs:1078-1080 (fn get_def_unc) | architecture | ported |
| 14 | Free the bounds arrays and the base Frame | interval.c:693-698 (fn GetDefUnc) | -- | ceremony | n/a: owned values are dropped |
| 15 | Return NULL if an error occurred | interval.c:699-701 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be annulled alongside one |
| 16 | Return the required pointer | interval.c:702-703 (fn GetDefUnc) | interval.rs:1078 (fn get_def_unc) | architecture | ported |
| 17 | `MapMerge`: initialise the result and the merged-pair index | interval.c:1132-1135 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 18 | Check the global error status and bail out | interval.c:1136-1138 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 19 | Get a pointer to the Interval | interval.c:1139-1141 (fn MapMerge) | -- | ceremony | n/a: the Rust function already has `this: &dyn Region` |
| 20 | Banner: first see whether the Interval reduces on its own, without reference to its neighbours | interval.c:1142-1145 (fn MapMerge) | -- | ceremony | n/a: a section banner, not a step |
| 21 | Simplify the Interval, assuming a changed pointer means something happened | interval.c:1146-1149 (fn MapMerge) | mappings/simplify.rs:1455-1456 (fn region_map_merge) | architecture | ported |
| 22 | Annul the Interval pointer in the list, replace it with the new Region, and mark it forward | interval.c:1150-1156 (fn MapMerge) | mappings/simplify.rs:1457 (fn region_map_merge) | architecture | ported |
| 23 | Return the index of the first modified element | interval.c:1157-1160 (fn MapMerge) | mappings/simplify.rs:1458 (fn region_map_merge) | architecture | ported |
| 24 | Failing that, merge with the Regions on either side — which is possible in parallel only | interval.c:1161-1165 (fn MapMerge) | mappings/simplify.rs:1463 (fn region_map_merge) | architecture | ported |
| 25 | Annul the unwanted simplified pointer | interval.c:1166-1167 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 26 | Attempt to merge the Interval with its lower neighbour, if that neighbour is a Region | interval.c:1168-1176 (fn MapMerge) | mappings/simplify.rs:1465-1473 (fn region_map_merge) | architecture | ported |
| 27 | Failing that, attempt the upper neighbour the same way | interval.c:1177-1187 (fn MapMerge) | mappings/simplify.rs:1477-1486 (fn region_map_merge) | architecture | ported |
| 28 | Act only if one of the two produced a merged Region | interval.c:1188-1190 (fn MapMerge) | mappings/simplify.rs:1470 (fn region_map_merge),1482 | architecture | ported |
| 29 | Annul the first of the two Mappings, replace it with the merged Region, and clear its invert flag | interval.c:1191-1196 (fn MapMerge) | mappings/simplify.rs:589 (fn replace_pair_with) | architecture | ported |
| 30 | Annul the second and shuffle the rest of the list down to fill the gap | interval.c:1197-1204 (fn MapMerge) | mappings/simplify.rs:590-591 (fn replace_pair_with) | architecture | ported |
| 31 | Clear the vacated element at the end | interval.c:1205-1208 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entry leaves no slot to blank |
| 32 | Decrement the Mapping count and return the index of the first modified element | interval.c:1209-1214 (fn MapMerge) | mappings/simplify.rs:1472 (fn region_map_merge),1484 | architecture | ported |
| 33 | In series, annul the simplified pointer and do nothing | interval.c:1215-1218 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and the series case falls through to `None` |
| 34 | Return the result | interval.c:1219-1221 (fn MapMerge) | mappings/simplify.rs:1489 (fn region_map_merge) | architecture | ported |
| 35 | `MergeInterval`: initialise the returned pointer and the bounds arrays | interval.c:1308-1312 (fn MergeInterval) | -- | ceremony | n/a: the Rust returns `Option<DynMapping>`, so "no merge" is `None` |
| 36 | Check the local error status and bail out | interval.c:1313-1315 (fn MergeInterval) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 37 | Both Regions must agree on Closed, and either both or neither must have a defined uncertainty | interval.c:1316-1322 (fn MergeInterval) | interval.rs:1172-1176 (fn region_merge) | architecture | ported |
| 38 | Get the Negated attributes of the two Regions | interval.c:1323-1326 (fn MergeInterval) | interval.rs:1178 (fn region_merge) | architecture | ported |
| 39 | Get the number of axes in the two supplied Regions | interval.c:1327-1330 (fn MergeInterval) | interval.rs:1179-1180 (fn region_merge) | architecture | ported |
| 40 | Get the number of axes the combination will have | interval.c:1331-1334 (fn MergeInterval) | interval.rs:1187 (fn region_merge) | architecture | ported |
| 41 | Get the base Frames from the two Region FrameSets | interval.c:1335-1339 (fn MergeInterval) | interval.rs:1262-1263 (fn region_merge) | architecture | ported |
| 42 | Combine them into a single CmpFrame, in the requested order, for use in creating any new Region | interval.c:1340-1345 (fn MergeInterval) | interval.rs:1264-1268 (fn region_merge) | architecture | ported |
| 43 | Annul the two base Frame pointers | interval.c:1346-1348 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 44 | Indicate we do not yet have a merged Region | interval.c:1349-1351 (fn MergeInterval) | -- | ceremony | n/a: the Rust builds an `Option<MergedRegion>`, so "no merge yet" is the `None` any arm can return |
| 45 | First attempt to merge with another Interval, which needs both Intervals un-negated | interval.c:1352-1355 (fn MergeInterval) | interval.rs:1198-1203 (fn region_merge) | architecture | ported |
| 46 | Allocate memory to store the bounds of the returned Interval | interval.c:1356-1359 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 47 | Copy the limits from the two supplied Intervals into those arrays, in the requested order | interval.c:1360-1369 (fn MergeInterval) | interval.rs:1204-1205 (fn region_merge),1186-1196 | architecture | ported |
| 48 | Create the new Interval, initially with no uncertainty | interval.c:1370-1373 (fn MergeInterval) | merge_util.rs:152-156 (fn finish_region_merge),149 | architecture | ported |
| 49 | Free the bounds arrays | interval.c:1374-1377 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 50 | Now attempt to merge with a Box, giving an Interval, which needs both Regions un-negated | interval.c:1378-1381 (fn MergeInterval) | interval.rs:1208-1212 (fn region_merge) | architecture | ported |
| 51 | Allocate memory to store the bounds of the returned Interval | interval.c:1382-1385 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 52 | Get the bounds from the Interval into its part of those arrays | interval.c:1386-1392 (fn MergeInterval) | interval.rs:1214-1215 (fn region_merge) | architecture | ported |
| 53 | Copy the centre and corner from the supplied Box into the other part | interval.c:1393-1403 (fn MergeInterval) | interval.rs:1213 (fn region_merge) | architecture | ported |
| 54 | Convert those centre and corner positions into upper and lower bounds, swapping any pair the conversion inverts | interval.c:1404-1415 (fn MergeInterval) | box_region.rs:844 (fn reg_base_box) | architecture | ported |
| 55 | Create the new Interval, initially with no uncertainty | interval.c:1416-1419 (fn MergeInterval) | merge_util.rs:152-156 (fn finish_region_merge),149 | architecture | ported |
| 56 | Free the bounds arrays | interval.c:1420-1423 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 57 | Now attempt to merge with a NullRegion, giving an Interval; the NullRegion must be negated and the Interval must not | interval.c:1424-1427 (fn MergeInterval) | interval.rs:1218-1222 (fn region_merge) | architecture | ported |
| 58 | Allocate memory to store the bounds of the returned Interval | interval.c:1428-1431 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 59 | Copy the Interval's limits in and store bad values on the NullRegion's axes, marking them unbounded | interval.c:1432-1447 (fn MergeInterval) | interval.rs:1224-1226 (fn region_merge) | architecture | ported |
| 60 | Create the new Interval, initially with no uncertainty | interval.c:1448-1451 (fn MergeInterval) | merge_util.rs:152-156 (fn finish_region_merge),149 | architecture | ported |
| 61 | Free the bounds arrays | interval.c:1452-1455 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 62 | Now attempt to merge with a PointList, giving a PointList, which needs both Regions un-negated | interval.c:1456-1459 (fn MergeInterval) | interval.rs:1229-1234 (fn region_merge) | architecture | ported |
| 63 | Get the Interval's own base bounding box into the two bounds arrays | interval.c:1460-1465 (fn MergeInterval) | -- | architecture | n/a: every value this writes is overwritten at interval.c:1470 (fn MergeInterval) before it is read |
| 64 | Get the size of the Interval's base-Frame uncertainty, over those same two arrays | interval.c:1466-1471 (fn MergeInterval) | interval.rs:1243-1244 (fn region_merge) | architecture | ported |
| 65 | Refuse the merge unless the Interval has zero width — a width below a quarter of the uncertainty — on every axis, replacing the lower bounds with the axis centres as it goes | interval.c:1472-1487 (fn MergeInterval) | interval.rs:1245 (fn region_merge) | architecture | ported |
| 66 | Go on to create a new PointList only if the Interval is a point | interval.c:1488-1490 (fn MergeInterval) | -- | architecture | n/a: the test at interval.c:1479 (fn MergeInterval) subtracts a value from itself, so this condition is always true |
| 67 | Get a PointSet holding the supplied PointList's axis values, its point count and pointers to its arrays | interval.c:1491-1497 (fn MergeInterval) | interval.rs:1246-1249 (fn region_merge) | architecture | ported |
| 68 | Create a new PointSet with room for the same points but the extra axes, and get pointers to its arrays | interval.c:1498-1502 (fn MergeInterval) | interval.rs:1250-1251 (fn region_merge) | architecture | ported |
| 69 | Copy the PointList axis values into the new PointSet, in the requested order | interval.c:1503-1512 (fn MergeInterval) | interval.rs:1250 (fn region_merge),1186-1196 | architecture | ported |
| 70 | Add the axis values the Interval defines to every point | interval.c:1513-1518 (fn MergeInterval) | interval.rs:1245 (fn region_merge),1250 | architecture | ported |
| 71 | Create the new PointList, initially with no uncertainty | interval.c:1519-1523 (fn MergeInterval) | merge_util.rs:162-166 (fn finish_region_merge),149 | architecture | ported |
| 72 | Free the two PointSets | interval.c:1524-1528 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 73 | Free the bounds arrays and the uncertainty | interval.c:1529-1536 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 74 | If a new Region was created above, propagate the supplied Region's remaining attributes to it | interval.c:1537-1541 (fn MergeInterval) | merge_util.rs:140-147 (fn finish_region_merge) | architecture | ported |
| 75 | Ensure the Negated attribute the constructors set is not the one the overlay left | interval.c:1542-1546 (fn MergeInterval) | merge_util.rs:146 (fn finish_region_merge) | architecture | ported |
| 76 | Assign the new Region an uncertainty only when both supplied Regions have one | interval.c:1547-1550 (fn MergeInterval) | merge_util.rs:290 (fn combine_uncertainties) | architecture | ported |
| 77 | Get the base-Frame uncertainties from the two supplied Regions | interval.c:1551-1554 (fn MergeInterval) | interval.rs:1310-1313 (fn region_merge) | architecture | ported |
| 78 | Combine them into a single Prism, in the correct order | interval.c:1555-1561 (fn MergeInterval) | merge_util.rs:291 (fn combine_uncertainties),293-295 | architecture | ported |
| 79 | Attempt to simplify the Prism | interval.c:1562-1564 (fn MergeInterval) | merge_util.rs:296 (fn combine_uncertainties) | architecture | ported |
| 80 | Use the simplified Prism as the uncertainty for the returned Region | interval.c:1565-1567 (fn MergeInterval) | merge_util.rs:169-171 (fn finish_region_merge) | architecture | ported |
| 81 | Free the uncertainty resources | interval.c:1568-1574 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 82 | Get the current Frames from the two Region FrameSets and combine them into one CmpFrame | interval.c:1575-1585 (fn MergeInterval) | interval.rs:1271-1277 (fn region_merge) | architecture | ported |
| 83 | Get the two base-to-current Mappings and combine them into one parallel CmpMap connecting the two combined Frames | interval.c:1586-1599 (fn MergeInterval) | interval.rs:1280-1307 (fn region_merge) | architecture | ported |
| 84 | Map the new Region into the new current Frame | interval.c:1600-1602 (fn MergeInterval) | merge_util.rs:173 (fn finish_region_merge) | architecture | ported |
| 85 | The result's FillFactor is the product of the two supplied FillFactors, set only when either was set | interval.c:1603-1609 (fn MergeInterval) | merge_util.rs:65-67 (fn apply_merge_scaling_to) | architecture | ported |
| 86 | Scale the result's MeshSize only when either supplied MeshSize was set | interval.c:1610-1617 (fn MergeInterval) | merge_util.rs:69 (fn apply_merge_scaling_to) | architecture | ported |
| 87 | Get the two MeshSize values, then clear them so the defaults are returned | interval.c:1618-1625 (fn MergeInterval) | merge_util.rs:70-71 (fn apply_merge_scaling_to) | architecture | ported |
| 88 | Form each Region's ratio of used to default MeshSize | interval.c:1626-1630 (fn MergeInterval) | merge_util.rs:72-73 (fn apply_merge_scaling_to) | architecture | ported |
| 89 | Set the result's MeshSize to its own default scaled by the product of the two ratios | interval.c:1631-1634 (fn MergeInterval) | merge_util.rs:74-75 (fn apply_merge_scaling_to) | architecture | ported |
| 90 | Re-instate the original MeshSize values on the supplied Regions | interval.c:1635-1640 (fn MergeInterval) | -- | ceremony | n/a: nothing was cleared on the supplied Regions, so there is nothing to restore |
| 91 | Free the Frame, Mapping and intermediate-Region resources | interval.c:1641-1649 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 92 | Free the combined base Frame | interval.c:1650-1653 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 93 | Annul the returned pointer if an error occurred | interval.c:1654-1656 (fn MergeInterval) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 94 | Return the result | interval.c:1657-1658 (fn MergeInterval) | merge_util.rs:179 (fn finish_region_merge) | architecture | ported |
| 95 | `Simplify`: initialise the returned pointer | interval.c:3184-3186 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no sentinel to initialise |
| 96 | Check the global error status and bail out | interval.c:3187-3189 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 97 | Get a pointer to the supplied Region structure | interval.c:3190-3192 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 98 | Get a pointer to the supplied Interval structure | interval.c:3193-3195 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 99 | If this Interval is equivalent to a Box, simplify that Box and return the result | interval.c:3196-3201 (fn Simplify) | interval.rs:1465-1468 (fn simplified) | architecture | ported |
| 100 | Otherwise use the implementation appropriate for unbounded intervals | interval.c:3202-3205 (fn Simplify) | interval.rs:1467 (fn simplified) | architecture | ported |
| 101 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and uncertainty | interval.c:3206-3210 (fn Simplify) | interval.rs:1473 (fn simplified) | architecture | ported |
| 102 | Note whether any simplification took place, from whether the returned pointer changed | interval.c:3211-3215 (fn Simplify) | interval.rs:1487 (fn simplified) | architecture | ported |
| 103 | Attempt the re-definition only when the base-to-current Mapping is not a UnitMap | interval.c:3216-3220 (fn Simplify) | interval.rs:1489-1494 (fn simplified) | architecture | ported |
| 104 | Copy the base-Frame bounds, giving a complete box by replacing any missing limit with an arbitrary non-bad value | interval.c:3221-3235 (fn Simplify) | interval.rs:264-290 (fn simplify_remapped) | architecture | ported |
| 105 | Ensure a good upper bound on each axis, from the lower bound's sign where there is one | interval.c:3236-3251 (fn Simplify) | interval.rs:270-280 (fn simplify_remapped) | architecture | ported |
| 106 | Ensure a good lower bound on each axis the same way | interval.c:3252-3266 (fn Simplify) | interval.rs:281-289 (fn simplify_remapped) | architecture | ported |
| 107 | Transform the box corners into the current Frame and back, then widen the box to enclose both the original and the returned positions | interval.c:3267-3297 (fn Simplify) | interval.rs:296-320 (fn simplify_remapped) | architecture | ported |
| 108 | Limit the box to the Interval's own limits, handling an excluded axis by collapsing to whichever side is wider | interval.c:3298-3325 (fn Simplify) | interval.rs:325-358 (fn simplify_remapped) | architecture | ported |
| 109 | Ensure the two bounds on each axis are not equal | interval.c:3326-3334 (fn Simplify) | interval.rs:360-365 (fn simplify_remapped) | architecture | ported |
| 110 | Create a Box on the base Frame from the box found above, carrying the Interval's base-Frame uncertainty where it has one | interval.c:3335-3340 (fn Simplify) | interval.rs:371-389 (fn simplify_remapped) | architecture | ported |
| 111 | Modify that Box so it has the same current Frame as this Interval | interval.c:3341-3344 (fn Simplify) | interval.rs:390-391 (fn simplify_remapped) | architecture | ported |
| 112 | Try simplifying the Box | interval.c:3345-3347 (fn Simplify) | interval.rs:392 (fn simplify_remapped) | architecture | ported |
| 113 | Proceed only if the Box was simplified | interval.c:3348-3350 (fn Simplify) | interval.rs:405-434 (fn simplify_remapped) | architecture | ported |
| 114 | Return the simplified Box where it is a NullRegion | interval.c:3351-3356 (fn Simplify) | interval.rs:402-404 (fn simplify_remapped) | architecture | ported |
| 115 | Otherwise act where it is a Box or an Interval | interval.c:3357-3359 (fn Simplify) | interval.rs:405-406 (fn simplify_remapped) | architecture | ported |
| 116 | Get the bounds of the simplified Region, from its defining points for an Interval and from its base bounding box for a Box | interval.c:3360-3376 (fn Simplify) | interval.rs:410-428 (fn simplify_remapped) | architecture | ported |
| 117 | Create a PointSet holding one point per axis of the simplified Box, plus an extra | interval.c:3377-3382 (fn Simplify) | interval.rs:442 (fn simplify_remapped) | architecture | ported |
| 118 | Put its lower bounds in the first point and displace each remaining point along one axis by that axis's box length | interval.c:3383-3395 (fn Simplify) | interval.rs:443-448 (fn simplify_remapped) | architecture | ported |
| 119 | Transform that PointSet into this Interval's base Frame with the inverse Mapping | interval.c:3396-3401 (fn Simplify) | interval.rs:449 (fn simplify_remapped) | architecture | ported |
| 120 | For each current Frame axis, find the base Frame axis with the largest increment between the two corresponding base positions | interval.c:3402-3426 (fn Simplify) | interval.rs:452-469 (fn simplify_remapped) | architecture | ported |
| 121 | A zero largest increment means the axis is absent from the base Frame, so give it equal limits at the centre | interval.c:3427-3442 (fn Simplify) | interval.rs:471-479 (fn simplify_remapped) | architecture | ported |
| 122 | Otherwise transfer the base Frame axis's missing limits to the current Frame axis | interval.c:3443-3449 (fn Simplify) | interval.rs:480-488 (fn simplify_remapped) | architecture | ported |
| 123 | Where the original limits were equal, make the new limits equal too | interval.c:3450-3455 (fn Simplify) | interval.rs:489-490 (fn simplify_remapped) | architecture | ported |
| 124 | Where the original interval was an inclusion, keep the new one an inclusion by swapping the limits if required | interval.c:3456-3464 (fn Simplify) | interval.rs:491-495 (fn simplify_remapped) | architecture | ported |
| 125 | Where it was an exclusion, keep the new one an exclusion the same way | interval.c:3465-3476 (fn Simplify) | interval.rs:496-499 (fn simplify_remapped) | architecture | ported |
| 126 | Create the simplified Interval from those current Frame limits, carrying the current-Frame uncertainty, and use it in place of the original | interval.c:3477-3485 (fn Simplify) | interval.rs:503-524 (fn simplify_remapped) | architecture | ported |
| 127 | Free the two PointSets and the simplified bounds arrays | interval.c:3486-3493 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 128 | Free the Frames, the two Boxes, the simplified Region and the bounds arrays | interval.c:3494-3503 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 129 | Free the base-to-current Mapping | interval.c:3504-3506 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 130 | If anything simplified, overlay the supplied Region's attributes onto the result and return it; otherwise return a clone of the supplied pointer | interval.c:3507-3520 (fn Simplify) | interval.rs:1506-1523 (fn simplified) | architecture | ported |
| 131 | Annul the returned pointer if an error occurred | interval.c:3521-3523 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 132 | Return the result | interval.c:3524-3525 (fn Simplify) | interval.rs:1520 (fn simplified),1522 | architecture | ported |

## Rows 11 and 12 — the guard the base class gets wrong

Interval tests `lb != -f64::MAX && ub != f64::MAX`
(`interval.rs:1064 (fn get_def_unc)`), which is the correct rendering of C's
`±DBL_MAX` test in both directions. The base-class `default_bounding_box_unc` uses
`is_bad` instead and so misses the upper case — see PLAN.md §10.12. This class is
the reference for what that guard should look like.

## Row 99 — the Box path comes first, and it hands the Box to the simplifier

C dispatches on Box-equivalence before doing any remap work, and the port keeps
that order (`interval.rs:1465-1468 (fn simplified)`). The condition is C's: every
resolved limit finite, every axis sharing axis 1's included/excluded orientation,
and axis 1 including rather than excluding its interval. A Negated Interval still
yields a Box — C negates the Box (`interval.c:509 (fn Cache)`) rather than
declining — and the Interval's `Closed` value transfers with an explicit
`astSetClosed` (`interval.c:512 (fn Cache)`).

What the branch returns is `astSimplify( box )`, not the Box. That matters
because `Cache` builds the Box in the **base** Frame and, when the base→current
Mapping is not a UnitMap, maps it into the current Frame with `astMapRegion`
(`interval.c:495-499 (fn Cache)`) — which extends the Box's FrameSet rather than
moving its points. The Mapping therefore reaches `Box::Simplify`, which is what
re-expresses the bounds in the current Frame's units. A reclassification that
labelled the base-Frame numbers with the current Frame and returned would drop
that Mapping; PLAN.md §10.25 records what such a divergence costs.

C applies no `astRegOverlay` on this branch, so nothing from the Interval is
carried onto the result beyond what `Cache` put on the Box.

## Rows 9 and 99 and the `reg_pins_mask` delegation — one `Cache`

`Cache` populates two things: the resolved limit arrays and, conditionally, the
equivalent Box. Three C bodies read them — `Simplify`
(`interval.c:3198 (fn Simplify)`), `GetDefUnc` (`interval.c:646 (fn GetDefUnc)`,
`interval.c:668 (fn GetDefUnc)`) and `RegPins` (`interval.c:2682 (fn RegPins)`) —
so `Interval::cache` returns the pair of arrays alongside the optional Box and all
three call sites take it from there. `interval.rs:177-240 (fn cache)` is the only
copy of the classification, reached from `interval.rs:785 (fn reg_pins_mask)`,
`interval.rs:1047 (fn get_def_unc)` and `interval.rs:1465 (fn simplified)`.

## Rows 104 to 109 — arbitrary limits, then reined back in

The re-definition has to transform a *bounded* box, so C fills each missing limit
with an arbitrary value, transforms, and then limits the result back to the
Interval's real limits. Skipping the second half would widen the Interval; the
port does both (`interval.rs:262-366 (fn simplify_remapped)`).

## Row 110 — the Box carries the Interval's base-Frame uncertainty

C reads `astGetUncFrm( new, AST__BASE )` and hands it to `astBox`
(`interval.c:3337 (fn Simplify)`), so the Box whose simplification decides the
whole branch is built with the Interval's uncertainty rather than with none. That
matters twice: `Box::Simplify` consults the uncertainty when deciding what the Box
reduces to, and `astRegOverlay` never installs an uncertainty of its own — its
only uncertainty action is to *clear* one (`region.c:9917 (fn RegOverlay)`) — so a
Box built without it stays without it. The port passes the same Region
(`interval.rs:383-389 (fn simplify_remapped)`), read through `get_unc_frm`
(`regions/mod.rs:569 (fn get_unc_frm)`), which is `astGetUncFrm`.

## Rows 113 and 114 — the port's "was it simplified?" signal

C compares pointers: `sreg != box2` (`interval.c:3349 (fn Simplify)`) is true
whenever `astSimplify` returned anything other than a clone of what it was given.
`BoxRegion::simplified` returns an unchanged *clone* when it cannot reduce the Box,
so a pointer comparison has no analogue here. The equivalent signal is the one
thing a successful Box simplification always achieves: its base→current Mapping
becomes a UnitMap. The port therefore tests that
(`interval.rs:410-428 (fn simplify_remapped)`) and treats a Box or Interval whose
Mapping is still non-trivial as unsimplified, which is C's `else` at
`interval.c:3502 (fn Simplify)`.

A NullRegion is checked before that test (`interval.rs:402-404 (fn simplify_remapped)`)
because C takes it as the answer in its own right, cloning it over `new` and
setting `simpler` (`interval.c:3351-3355 (fn Simplify)`). It is already expressed
in the current Frame, so there are no limits to recover and no axis
correspondence to work out. Testing for it first is safe: an unsimplified result
is always the Box that went in.

## Rows 63 to 66 — the PointList arm reads its own uncertainty

C allocates two arrays, fills them with the Interval's base bounding box
(`interval.c:1464 (fn MergeInterval)`), and then immediately overwrites both with
the *uncertainty's* bounds (`interval.c:1470 (fn MergeInterval)`) — it passes
`lbnd`/`ubnd` where the two `_unc` arrays it allocated for the purpose were
intended. Two consequences follow, and both are C's observable behaviour rather
than anything the port may improve on:

- The zero-width test at `interval.c:1479 (fn MergeInterval)` subtracts
  `lbnd[ i ]` from itself, so it compares zero against a positive quarter-width
  and always passes. The arm never refuses a merge on width grounds.
- The axis values the merged PointList receives are the centre of the
  uncertainty, not of the Interval (`interval.c:1484 (fn MergeInterval)`).

`interval.rs:1235-1252 (fn region_merge)` mirrors both, and the aliasing is
recorded in `docs/issues/c-library-quirks.md` so the port can be corrected in step
with an upstream fix rather than ahead of one.

## Rows 74 and 75 — the constructor sets Negated, then the overlay is undone

Every arm's constructor builds an un-negated Region, and `astRegOverlay` would
copy the supplied Interval's `Negated` onto it, so C clears it again
(`interval.c:1545 (fn MergeInterval)`). The port never sets it in the first place:
`finish_region_merge` takes `negated: false` from the caller
(`interval.rs:1334 (fn region_merge)`) and applies it with the rest of the overlay
(`merge_util.rs:146 (fn finish_region_merge)`). The arms that require a negated
neighbour — the NullRegion arm at `interval.c:1426 (fn MergeInterval)` — encode
that negation in the bounds they build, not in the result's flag.

# `Simplify` / `MapMerge` audit — Interval

C source: reference/starlink-ast/src/interval.c
Rust: crates/ast-core/src/regions/interval.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- interval.c:633 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:1136 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:1277 (fn MergeInterval) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:3159 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Four C bodies. `GetDefUnc` (`interval.c:645-715 (fn GetDefUnc)`) is the default
uncertainty: the equivalent Box's own default where the Interval is a Box, and
otherwise a Box spanning 1.0E-6 of each bounded axis interval. `MapMerge`
(`interval.c:1144-1233 (fn MapMerge)`) is the Region shape shared by every Region
class: self-simplify first, and only failing that merge with a neighbour, in
parallel only. `MergeInterval` (`interval.c:1320-1672 (fn MergeInterval)`) is the
merge it delegates a neighbouring Region to — four arms, one per admissible
neighbour class. `Simplify` (`interval.c:3198-3539 (fn Simplify)`) simplifies the
equivalent Box where there is one, and otherwise re-defines the Interval within
its current Frame by way of a fully-bounded Box.

`Interval::get_def_unc` (`interval.rs:1342-1385 (fn get_def_unc)`) is `GetDefUnc`.
`Interval::map_merge` (`interval.rs:1919-1922 (fn map_merge)`) delegates to
`simplify::region_map_merge` (`mappings/simplify.rs:1481-1528 (fn region_map_merge)`),
which every Region shares. `Interval::region_merge`
(`interval.rs:1467-1651 (fn region_merge)`) is `MergeInterval`, and
`Interval::simplified` (`interval.rs:1819-1880 (fn simplified)`) is `Simplify`, with
its unbounded branch in `simplify_remapped`
(`interval.rs:542-812 (fn simplify_remapped)`).

`MergeInterval` shares its whole tail with `MergeBox` and `MergePointList`, so the
rows from each arm's constructor onwards cite `merge_util.rs`, where that tail
lives once.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `GetDefUnc`: initialise the returned pointer | interval.c:645-647 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no sentinel to initialise |
| 2 | Check the global error status and bail out | interval.c:648-650 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Interval structure | interval.c:651-653 (fn GetDefUnc) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | If this Interval is equivalent to a Box, return that Box's default uncertainty | interval.c:654-659 (fn GetDefUnc) | interval.rs:1342-1345 (fn get_def_unc) | architecture | ported |
| 5 | Otherwise use a box covering 1.0E-6 of each axis interval, centred on the origin | interval.c:660-663 (fn GetDefUnc) | interval.rs:1347-1351 (fn get_def_unc) | architecture | ported |
| 6 | Get a pointer to the base Frame | interval.c:664-666 (fn GetDefUnc) | interval.rs:1352 (fn get_def_unc) | architecture | ported |
| 7 | Get the number of base Frame axes | interval.c:667-669 (fn GetDefUnc) | interval.rs:1351 (fn get_def_unc) | architecture | ported |
| 8 | Allocate arrays to hold the bounds of the uncertainty Box | interval.c:670-674 (fn GetDefUnc) | interval.rs:1354-1355 (fn get_def_unc) | ceremony | ported |
| 9 | Ensure the cached bounds are up to date | interval.c:675-677 (fn GetDefUnc) | interval.rs:1342 (fn get_def_unc) | architecture | ported |
| 10 | Do each axis in turn | interval.c:678-680 (fn GetDefUnc) | interval.rs:1358 (fn get_def_unc) | architecture | ported |
| 11 | Where the axis has both limits, use 1.0E-6 of their difference about their mean, falling back to a fraction of the centre when that difference is zero | interval.c:681-693 (fn GetDefUnc) | interval.rs:1361-1368 (fn get_def_unc) | architecture | ported |
| 12 | Otherwise use zero for both bounds | interval.c:694-700 (fn GetDefUnc) | interval.rs:1369-1375 (fn get_def_unc) | architecture | ported |
| 13 | Create the Box | interval.c:701-704 (fn GetDefUnc) | interval.rs:1378-1380 (fn get_def_unc) | architecture | ported |
| 14 | Free the bounds arrays and the base Frame | interval.c:705-710 (fn GetDefUnc) | -- | ceremony | n/a: owned values are dropped |
| 15 | Return NULL if an error occurred | interval.c:711-713 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be annulled alongside one |
| 16 | Return the required pointer | interval.c:714-715 (fn GetDefUnc) | interval.rs:1378 (fn get_def_unc) | architecture | ported |
| 17 | `MapMerge`: initialise the result and the merged-pair index | interval.c:1144-1147 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 18 | Check the global error status and bail out | interval.c:1148-1150 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 19 | Get a pointer to the Interval | interval.c:1151-1153 (fn MapMerge) | -- | ceremony | n/a: the Rust function already has `this: &dyn Region` |
| 20 | Banner: first see whether the Interval reduces on its own, without reference to its neighbours | interval.c:1154-1157 (fn MapMerge) | -- | ceremony | n/a: a section banner, not a step |
| 21 | Simplify the Interval, assuming a changed pointer means something happened | interval.c:1158-1161 (fn MapMerge) | mappings/simplify.rs:1493-1494 (fn region_map_merge) | architecture | ported |
| 22 | Annul the Interval pointer in the list, replace it with the new Region, and mark it forward | interval.c:1162-1168 (fn MapMerge) | mappings/simplify.rs:1495 (fn region_map_merge) | architecture | ported |
| 23 | Return the index of the first modified element | interval.c:1169-1172 (fn MapMerge) | mappings/simplify.rs:1496 (fn region_map_merge) | architecture | ported |
| 24 | Failing that, merge with the Regions on either side — which is possible in parallel only | interval.c:1173-1177 (fn MapMerge) | mappings/simplify.rs:1501 (fn region_map_merge) | architecture | ported |
| 25 | Annul the unwanted simplified pointer | interval.c:1178-1179 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 26 | Attempt to merge the Interval with its lower neighbour, if that neighbour is a Region | interval.c:1180-1188 (fn MapMerge) | mappings/simplify.rs:1503-1511 (fn region_map_merge) | architecture | ported |
| 27 | Failing that, attempt the upper neighbour the same way | interval.c:1189-1199 (fn MapMerge) | mappings/simplify.rs:1515-1524 (fn region_map_merge) | architecture | ported |
| 28 | Act only if one of the two produced a merged Region | interval.c:1200-1202 (fn MapMerge) | mappings/simplify.rs:1508 (fn region_map_merge),1520 (fn region_map_merge) | architecture | ported |
| 29 | Annul the first of the two Mappings, replace it with the merged Region, and clear its invert flag | interval.c:1203-1208 (fn MapMerge) | mappings/simplify.rs:588 (fn replace_pair_with) | architecture | ported |
| 30 | Annul the second and shuffle the rest of the list down to fill the gap | interval.c:1209-1216 (fn MapMerge) | mappings/simplify.rs:589-590 (fn replace_pair_with) | architecture | ported |
| 31 | Clear the vacated element at the end | interval.c:1217-1220 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entry leaves no slot to blank |
| 32 | Decrement the Mapping count and return the index of the first modified element | interval.c:1221-1226 (fn MapMerge) | mappings/simplify.rs:1510 (fn region_map_merge),1522 (fn region_map_merge) | architecture | ported |
| 33 | In series, annul the simplified pointer and do nothing | interval.c:1227-1230 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and the series case falls through to `None` |
| 34 | Return the result | interval.c:1231-1233 (fn MapMerge) | mappings/simplify.rs:1527 (fn region_map_merge) | architecture | ported |
| 35 | `MergeInterval`: initialise the returned pointer and the bounds arrays | interval.c:1320-1324 (fn MergeInterval) | -- | ceremony | n/a: the Rust returns `Option<DynMapping>`, so "no merge" is `None` |
| 36 | Check the local error status and bail out | interval.c:1325-1327 (fn MergeInterval) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 37 | Both Regions must agree on Closed, and either both or neither must have a defined uncertainty | interval.c:1328-1334 (fn MergeInterval) | interval.rs:1472-1476 (fn region_merge) | architecture | ported |
| 38 | Get the Negated attributes of the two Regions | interval.c:1335-1338 (fn MergeInterval) | interval.rs:1478 (fn region_merge) | architecture | ported |
| 39 | Get the number of axes in the two supplied Regions | interval.c:1339-1342 (fn MergeInterval) | interval.rs:1479-1480 (fn region_merge) | architecture | ported |
| 40 | Get the number of axes the combination will have | interval.c:1343-1346 (fn MergeInterval) | interval.rs:1487 (fn region_merge) | architecture | ported |
| 41 | Get the base Frames from the two Region FrameSets | interval.c:1347-1351 (fn MergeInterval) | interval.rs:1570-1571 (fn region_merge) | architecture | ported |
| 42 | Combine them into a single CmpFrame, in the requested order, for use in creating any new Region | interval.c:1352-1357 (fn MergeInterval) | interval.rs:1572-1576 (fn region_merge) | architecture | ported |
| 43 | Annul the two base Frame pointers | interval.c:1358-1360 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 44 | Indicate we do not yet have a merged Region | interval.c:1361-1363 (fn MergeInterval) | -- | ceremony | n/a: the Rust builds an `Option<MergedRegion>`, so "no merge yet" is the `None` any arm can return |
| 45 | First attempt to merge with another Interval, which needs both Intervals un-negated | interval.c:1364-1367 (fn MergeInterval) | interval.rs:1498-1503 (fn region_merge) | architecture | ported |
| 46 | Allocate memory to store the bounds of the returned Interval | interval.c:1368-1371 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 47 | Copy the limits from the two supplied Intervals into those arrays, in the requested order | interval.c:1372-1381 (fn MergeInterval) | interval.rs:1504-1505 (fn region_merge),1186-1196 (fn set_closed) | architecture | ported |
| 48 | Create the new Interval, initially with no uncertainty | interval.c:1382-1385 (fn MergeInterval) | merge_util.rs:152-156 (fn finish_region_merge),149 (fn finish_region_merge) | architecture | ported |
| 49 | Free the bounds arrays | interval.c:1386-1389 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 50 | Now attempt to merge with a Box, giving an Interval, which needs both Regions un-negated | interval.c:1390-1393 (fn MergeInterval) | interval.rs:1508-1512 (fn region_merge) | architecture | ported |
| 51 | Allocate memory to store the bounds of the returned Interval | interval.c:1394-1397 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 52 | Get the bounds from the Interval into its part of those arrays | interval.c:1398-1404 (fn MergeInterval) | interval.rs:1514-1515 (fn region_merge) | architecture | ported |
| 53 | Copy the centre and corner from the supplied Box into the other part | interval.c:1405-1415 (fn MergeInterval) | interval.rs:1513 (fn region_merge) | architecture | ported |
| 54 | Convert those centre and corner positions into upper and lower bounds, swapping any pair the conversion inverts | interval.c:1416-1427 (fn MergeInterval) | box_region.rs:848 (fn reg_base_box) | architecture | ported |
| 55 | Create the new Interval, initially with no uncertainty | interval.c:1428-1431 (fn MergeInterval) | merge_util.rs:152-156 (fn finish_region_merge),149 (fn finish_region_merge) | architecture | ported |
| 56 | Free the bounds arrays | interval.c:1432-1435 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 57 | Now attempt to merge with a NullRegion, giving an Interval; the NullRegion must be negated and the Interval must not | interval.c:1436-1439 (fn MergeInterval) | interval.rs:1518-1522 (fn region_merge) | architecture | ported |
| 58 | Allocate memory to store the bounds of the returned Interval | interval.c:1440-1443 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 59 | Copy the Interval's limits in and store bad values on the NullRegion's axes, marking them unbounded | interval.c:1444-1459 (fn MergeInterval) | interval.rs:1524-1526 (fn region_merge) | architecture | ported |
| 60 | Create the new Interval, initially with no uncertainty | interval.c:1460-1463 (fn MergeInterval) | merge_util.rs:152-156 (fn finish_region_merge),149 (fn finish_region_merge) | architecture | ported |
| 61 | Free the bounds arrays | interval.c:1464-1467 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 62 | Now attempt to merge with a PointList, giving a PointList, which needs both Regions un-negated | interval.c:1468-1471 (fn MergeInterval) | interval.rs:1529-1534 (fn region_merge) | architecture | ported |
| 63 | Get the Interval's own base bounding box into the two bounds arrays | interval.c:1472-1477 (fn MergeInterval) | interval.rs:1546 (fn region_merge) | architecture | ported |
| 64 | Get the size of the Interval's base-Frame uncertainty into its own two arrays | interval.c:1478-1483 (fn MergeInterval) | interval.rs:1547-1548 (fn region_merge) | architecture | ported |
| 65 | Refuse the merge unless the Interval's half-width is under a quarter of the uncertainty width on every axis, replacing the lower bounds with the axis centres as it goes | interval.c:1484-1501 (fn MergeInterval) | interval.rs:1550-1556 (fn region_merge) | architecture | ported |
| 66 | Go on to create a new PointList only if the Interval is a point | interval.c:1502-1504 (fn MergeInterval) | interval.rs:1553 (fn region_merge) | architecture | ported |
| 67 | Get a PointSet holding the supplied PointList's axis values, its point count and pointers to its arrays | interval.c:1505-1511 (fn MergeInterval) | interval.rs:1558-1561 (fn region_merge) | architecture | ported |
| 68 | Create a new PointSet with room for the same points but the extra axes, and get pointers to its arrays | interval.c:1512-1516 (fn MergeInterval) | interval.rs:1491 (fn region_merge) | architecture | ported |
| 69 | Copy the PointList axis values into the new PointSet, in the requested order | interval.c:1517-1526 (fn MergeInterval) | interval.rs:1490-1500 (fn region_merge),1562 (fn region_merge) | architecture | ported |
| 70 | Add the axis values the Interval defines to every point | interval.c:1527-1532 (fn MergeInterval) | interval.rs:1555 (fn region_merge),1562 (fn region_merge) | architecture | ported |
| 71 | Create the new PointList, initially with no uncertainty | interval.c:1533-1537 (fn MergeInterval) | merge_util.rs:162-166 (fn finish_region_merge),149 (fn finish_region_merge) | architecture | ported |
| 72 | Free the two PointSets | interval.c:1538-1542 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 73 | Free the bounds arrays and the uncertainty | interval.c:1543-1550 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 74 | If a new Region was created above, propagate the supplied Region's remaining attributes to it | interval.c:1551-1555 (fn MergeInterval) | merge_util.rs:140-147 (fn finish_region_merge) | architecture | ported |
| 75 | Ensure the Negated attribute the constructors set is not the one the overlay left | interval.c:1556-1560 (fn MergeInterval) | merge_util.rs:146 (fn finish_region_merge) | architecture | ported |
| 76 | Assign the new Region an uncertainty only when both supplied Regions have one | interval.c:1561-1564 (fn MergeInterval) | merge_util.rs:290 (fn combine_uncertainties) | architecture | ported |
| 77 | Get the base-Frame uncertainties from the two supplied Regions | interval.c:1565-1568 (fn MergeInterval) | interval.rs:1618-1621 (fn region_merge) | architecture | ported |
| 78 | Combine them into a single Prism, in the correct order | interval.c:1569-1575 (fn MergeInterval) | merge_util.rs:291 (fn combine_uncertainties),293-295 (fn combine_uncertainties) | architecture | ported |
| 79 | Attempt to simplify the Prism | interval.c:1576-1578 (fn MergeInterval) | merge_util.rs:296 (fn combine_uncertainties) | architecture | ported |
| 80 | Use the simplified Prism as the uncertainty for the returned Region | interval.c:1579-1581 (fn MergeInterval) | merge_util.rs:169-171 (fn finish_region_merge) | architecture | ported |
| 81 | Free the uncertainty resources | interval.c:1582-1588 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 82 | Get the current Frames from the two Region FrameSets and combine them into one CmpFrame | interval.c:1589-1599 (fn MergeInterval) | interval.rs:1579-1585 (fn region_merge) | architecture | ported |
| 83 | Get the two base-to-current Mappings and combine them into one parallel CmpMap connecting the two combined Frames | interval.c:1600-1613 (fn MergeInterval) | interval.rs:1588-1615 (fn region_merge) | architecture | ported |
| 84 | Map the new Region into the new current Frame | interval.c:1614-1616 (fn MergeInterval) | merge_util.rs:173 (fn finish_region_merge) | architecture | ported |
| 85 | The result's FillFactor is the product of the two supplied FillFactors, set only when either was set | interval.c:1617-1623 (fn MergeInterval) | merge_util.rs:65-67 (fn apply_merge_scaling_to) | architecture | ported |
| 86 | Scale the result's MeshSize only when either supplied MeshSize was set | interval.c:1624-1631 (fn MergeInterval) | merge_util.rs:69 (fn apply_merge_scaling_to) | architecture | ported |
| 87 | Get the two MeshSize values, then clear them so the defaults are returned | interval.c:1632-1639 (fn MergeInterval) | merge_util.rs:70-71 (fn apply_merge_scaling_to) | architecture | ported |
| 88 | Form each Region's ratio of used to default MeshSize | interval.c:1640-1644 (fn MergeInterval) | merge_util.rs:72-73 (fn apply_merge_scaling_to) | architecture | ported |
| 89 | Set the result's MeshSize to its own default scaled by the product of the two ratios | interval.c:1645-1648 (fn MergeInterval) | merge_util.rs:74-75 (fn apply_merge_scaling_to) | architecture | ported |
| 90 | Re-instate the original MeshSize values on the supplied Regions | interval.c:1649-1654 (fn MergeInterval) | -- | ceremony | n/a: nothing was cleared on the supplied Regions, so there is nothing to restore |
| 91 | Free the Frame, Mapping and intermediate-Region resources | interval.c:1655-1663 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 92 | Free the combined base Frame | interval.c:1664-1667 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 93 | Annul the returned pointer if an error occurred | interval.c:1668-1670 (fn MergeInterval) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 94 | Return the result | interval.c:1671-1672 (fn MergeInterval) | merge_util.rs:179 (fn finish_region_merge) | architecture | ported |
| 95 | `Simplify`: initialise the returned pointer | interval.c:3198-3200 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no sentinel to initialise |
| 96 | Check the global error status and bail out | interval.c:3201-3203 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 97 | Get a pointer to the supplied Region structure | interval.c:3204-3206 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 98 | Get a pointer to the supplied Interval structure | interval.c:3207-3209 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 99 | If this Interval is equivalent to a Box, simplify that Box and return the result | interval.c:3210-3215 (fn Simplify) | interval.rs:1821-1820 (fn simplified) | architecture | ported |
| 100 | Otherwise use the implementation appropriate for unbounded intervals | interval.c:3216-3219 (fn Simplify) | interval.rs:1819 (fn simplified) | architecture | ported |
| 101 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and uncertainty | interval.c:3220-3224 (fn Simplify) | interval.rs:1825 (fn simplified) | architecture | ported |
| 102 | Note whether any simplification took place, from whether the returned pointer changed | interval.c:3225-3229 (fn Simplify) | interval.rs:1839 (fn simplified) | architecture | ported |
| 103 | Attempt the re-definition only when the base-to-current Mapping is not a UnitMap | interval.c:3230-3234 (fn Simplify) | interval.rs:1841-1846 (fn simplified) | architecture | ported |
| 104 | Copy the base-Frame bounds, giving a complete box by replacing any missing limit with an arbitrary non-bad value | interval.c:3235-3249 (fn Simplify) | interval.rs:551-577 (fn simplify_remapped) | architecture | ported |
| 105 | Ensure a good upper bound on each axis, from the lower bound's sign where there is one | interval.c:3250-3265 (fn Simplify) | interval.rs:557-567 (fn simplify_remapped) | architecture | ported |
| 106 | Ensure a good lower bound on each axis the same way | interval.c:3266-3280 (fn Simplify) | interval.rs:568-576 (fn simplify_remapped) | architecture | ported |
| 107 | Transform the box corners into the current Frame and back, then widen the box to enclose both the original and the returned positions | interval.c:3281-3311 (fn Simplify) | interval.rs:583-607 (fn simplify_remapped) | architecture | ported |
| 108 | Limit the box to the Interval's own limits, handling an excluded axis by collapsing to whichever side is wider | interval.c:3312-3339 (fn Simplify) | interval.rs:612-645 (fn simplify_remapped) | architecture | ported |
| 109 | Ensure the two bounds on each axis are not equal | interval.c:3340-3348 (fn Simplify) | interval.rs:647-652 (fn simplify_remapped) | architecture | ported |
| 110 | Create a Box on the base Frame from the box found above, carrying the Interval's base-Frame uncertainty where it has one | interval.c:3349-3354 (fn Simplify) | interval.rs:658-676 (fn simplify_remapped) | architecture | ported |
| 111 | Modify that Box so it has the same current Frame as this Interval | interval.c:3355-3358 (fn Simplify) | interval.rs:677-678 (fn simplify_remapped) | architecture | ported |
| 112 | Try simplifying the Box | interval.c:3359-3361 (fn Simplify) | interval.rs:679 (fn simplify_remapped) | architecture | ported |
| 113 | Proceed only if the Box was simplified | interval.c:3362-3364 (fn Simplify) | interval.rs:692-721 (fn simplify_remapped) | architecture | ported |
| 114 | Return the simplified Box where it is a NullRegion | interval.c:3365-3370 (fn Simplify) | interval.rs:689-691 (fn simplify_remapped) | architecture | ported |
| 115 | Otherwise act where it is a Box or an Interval | interval.c:3371-3373 (fn Simplify) | interval.rs:692-693 (fn simplify_remapped) | architecture | ported |
| 116 | Get the bounds of the simplified Region, from its defining points for an Interval and from its base bounding box for a Box | interval.c:3374-3390 (fn Simplify) | interval.rs:697-715 (fn simplify_remapped) | architecture | ported |
| 117 | Create a PointSet holding one point per axis of the simplified Box, plus an extra | interval.c:3391-3396 (fn Simplify) | interval.rs:729 (fn simplify_remapped) | architecture | ported |
| 118 | Put its lower bounds in the first point and displace each remaining point along one axis by that axis's box length | interval.c:3397-3409 (fn Simplify) | interval.rs:730-735 (fn simplify_remapped) | architecture | ported |
| 119 | Transform that PointSet into this Interval's base Frame with the inverse Mapping | interval.c:3410-3415 (fn Simplify) | interval.rs:736 (fn simplify_remapped) | architecture | ported |
| 120 | For each current Frame axis, find the base Frame axis with the largest increment between the two corresponding base positions | interval.c:3416-3440 (fn Simplify) | interval.rs:739-756 (fn simplify_remapped) | architecture | ported |
| 121 | A zero largest increment means the axis is absent from the base Frame, so give it equal limits at the centre | interval.c:3441-3456 (fn Simplify) | interval.rs:758-766 (fn simplify_remapped) | architecture | ported |
| 122 | Otherwise transfer the base Frame axis's missing limits to the current Frame axis | interval.c:3457-3463 (fn Simplify) | interval.rs:767-775 (fn simplify_remapped) | architecture | ported |
| 123 | Where the original limits were equal, make the new limits equal too | interval.c:3464-3469 (fn Simplify) | interval.rs:776-777 (fn simplify_remapped) | architecture | ported |
| 124 | Where the original interval was an inclusion, keep the new one an inclusion by swapping the limits if required | interval.c:3470-3478 (fn Simplify) | interval.rs:778-782 (fn simplify_remapped) | architecture | ported |
| 125 | Where it was an exclusion, keep the new one an exclusion the same way | interval.c:3479-3490 (fn Simplify) | interval.rs:783-786 (fn simplify_remapped) | architecture | ported |
| 126 | Create the simplified Interval from those current Frame limits, carrying the current-Frame uncertainty, and use it in place of the original | interval.c:3491-3499 (fn Simplify) | interval.rs:790-811 (fn simplify_remapped) | architecture | ported |
| 127 | Free the two PointSets and the simplified bounds arrays | interval.c:3500-3507 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 128 | Free the Frames, the two Boxes, the simplified Region and the bounds arrays | interval.c:3508-3517 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 129 | Free the base-to-current Mapping | interval.c:3518-3520 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 130 | If anything simplified, overlay the supplied Region's attributes onto the result and return it; otherwise return a clone of the supplied pointer | interval.c:3521-3534 (fn Simplify) | interval.rs:1858-1875 (fn simplified) | architecture | ported |
| 131 | Annul the returned pointer if an error occurred | interval.c:3535-3537 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 132 | Return the result | interval.c:3538-3539 (fn Simplify) | interval.rs:1872 (fn simplified),1527 (fn region_merge) | architecture | ported |

## Rows 11 and 12 — the guard the base class gets wrong

Interval tests `lb != -f64::MAX && ub != f64::MAX`
(`interval.rs:1361 (fn get_def_unc)`), which is the correct rendering of C's
`±DBL_MAX` test in both directions. The base-class `default_bounding_box_unc` uses
`is_bad` instead and so misses the upper case — see PLAN.md §10.12. This class is
the reference for what that guard should look like.

## Row 99 — the Box path comes first, and it hands the Box to the simplifier

C dispatches on Box-equivalence before doing any remap work, and the port keeps
that order (`interval.rs:1821-1820 (fn simplified)`). The condition is C's: every
resolved limit finite, every axis sharing axis 1's included/excluded orientation,
and axis 1 including rather than excluding its interval. A Negated Interval still
yields a Box — C negates the Box (`interval.c:518 (fn Cache)`) rather than
declining — and the Interval's `Closed` value transfers with an explicit
`astSetClosed` (`interval.c:521 (fn Cache)`).

What the branch returns is `astSimplify( box )`, not the Box. That matters
because `Cache` builds the Box in the **base** Frame and, when the base→current
Mapping is not a UnitMap, maps it into the current Frame with `astMapRegion`
(`interval.c:504-508 (fn Cache)`) — which extends the Box's FrameSet rather than
moving its points. The Mapping therefore reaches `Box::Simplify`, which is what
re-expresses the bounds in the current Frame's units. A reclassification that
labelled the base-Frame numbers with the current Frame and returned would drop
that Mapping; PLAN.md §10.25 records what such a divergence costs.

C applies no `astRegOverlay` on this branch, so nothing from the Interval is
carried onto the result beyond what `Cache` put on the Box.

## Rows 9 and 99 and the `reg_pins_mask` delegation — one `Cache`

`Cache` populates two things: the resolved limit arrays and, conditionally, the
equivalent Box. Three C bodies read them — `Simplify`
(`interval.c:3212 (fn Simplify)`), `GetDefUnc` (`interval.c:655 (fn GetDefUnc)`,
`interval.c:677 (fn GetDefUnc)`) and `RegPins` (`interval.c:2696 (fn RegPins)`) —
so `Interval::cache` returns the pair of arrays alongside the optional Box and all
three call sites take it from there. `interval.rs:464-527 (fn cache)` is the only
copy of the classification, reached from `interval.rs:1080 (fn reg_pins_mask)`,
`interval.rs:1342 (fn get_def_unc)` and `interval.rs:1821 (fn simplified)`.

## Rows 104 to 109 — arbitrary limits, then reined back in

The re-definition has to transform a *bounded* box, so C fills each missing limit
with an arbitrary value, transforms, and then limits the result back to the
Interval's real limits. Skipping the second half would widen the Interval; the
port does both (`interval.rs:549-653 (fn simplify_remapped)`).

## Row 110 — the Box carries the Interval's base-Frame uncertainty

C reads `astGetUncFrm( new, AST__BASE )` and hands it to `astBox`
(`interval.c:3351 (fn Simplify)`), so the Box whose simplification decides the
whole branch is built with the Interval's uncertainty rather than with none. That
matters twice: `Box::Simplify` consults the uncertainty when deciding what the Box
reduces to, and `astRegOverlay` never installs an uncertainty of its own — its
only uncertainty action is to *clear* one (`region.c:9928 (fn RegOverlay)`) — so a
Box built without it stays without it. The port passes the same Region
(`interval.rs:670-676 (fn simplify_remapped)`), read through `get_unc_frm`
(`regions/mod.rs:639 (fn get_unc_frm)`), which is `astGetUncFrm`.

## Rows 113 and 114 — the port's "was it simplified?" signal

C compares pointers: `sreg != box2` (`interval.c:3363 (fn Simplify)`) is true
whenever `astSimplify` returned anything other than a clone of what it was given.
`BoxRegion::simplified` returns an unchanged *clone* when it cannot reduce the Box,
so a pointer comparison has no analogue here. The equivalent signal is the one
thing a successful Box simplification always achieves: its base→current Mapping
becomes a UnitMap. The port therefore tests that
(`interval.rs:697-715 (fn simplify_remapped)`) and treats a Box or Interval whose
Mapping is still non-trivial as unsimplified, which is C's `else` at
`interval.c:3516 (fn Simplify)`.

A NullRegion is checked before that test (`interval.rs:689-691 (fn simplify_remapped)`)
because C takes it as the answer in its own right, cloning it over `new` and
setting `simpler` (`interval.c:3365-3369 (fn Simplify)`). It is already expressed
in the current Frame, so there are no limits to recover and no axis
correspondence to work out. Testing for it first is safe: an unsimplified result
is always the Box that went in.

## Rows 63 to 66 — the PointList arm measures the Interval

Attaching one constant per Interval axis to every point is equivalent only when
the Interval is a point, so C reads its base bounding box
(`interval.c:1476 (fn MergeInterval)`), reads its base-Frame uncertainty into two
arrays of their own (`interval.c:1482 (fn MergeInterval)`), and refuses the merge
on any axis whose half-width exceeds a quarter of the uncertainty width
(`interval.c:1493-1494 (fn MergeInterval)`) — the test `MergeBox` applies to a
Box's half-width. The axis values the merged PointList receives are the centres
of the Interval's own axes (`interval.c:1498 (fn MergeInterval)`).

`interval.rs:1546-1556 (fn region_merge)` is that test and those values. Only an
unbounded Interval reaches the branch with non-zero width, because a bounded one
simplifies to a Box first and `MergeBox` merges a Box that is a point, so the arm
can only ever refuse; a missing limit is stored as `±f64::MAX` and fails any
width test. `neg_interval_unbounded_pointlist` pins the refusal.

C read the uncertainty over the arrays holding the Interval's own bounds until
`9916e19e`, so the width test subtracted a bound from itself and the constants
were the centre of the uncertainty Region; `docs/issues/c-library-quirks.md`
carries the history.

## Rows 74 and 75 — the constructor sets Negated, then the overlay is undone

Every arm's constructor builds an un-negated Region, and `astRegOverlay` would
copy the supplied Interval's `Negated` onto it, so C clears it again
(`interval.c:1559 (fn MergeInterval)`). The port never sets it in the first place:
`finish_region_merge` takes `negated: false` from the caller
(`interval.rs:1642 (fn region_merge)`) and applies it with the rest of the overlay
(`merge_util.rs:146 (fn finish_region_merge)`). The arms that require a negated
neighbour — the NullRegion arm at `interval.c:1438 (fn MergeInterval)` — encode
that negation in the bounds they build, not in the result's flag.

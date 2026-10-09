# `Simplify` / `MapMerge` audit — Interval

C source: reference/starlink-ast/src/interval.c
Rust: crates/ast-core/src/regions/interval.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- interval.c:639 (fn GetDefUnc) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:1142 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:1283 (fn MergeInterval) — `Local Variables:` is the local-variable declaration block, not a step
- interval.c:3165 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Four C bodies. `GetDefUnc` (`interval.c:651-721 (fn GetDefUnc)`) is the default
uncertainty: the equivalent Box's own default where the Interval is a Box, and
otherwise a Box spanning 1.0E-6 of each bounded axis interval. `MapMerge`
(`interval.c:1150-1239 (fn MapMerge)`) is the Region shape shared by every Region
class: self-simplify first, and only failing that merge with a neighbour, in
parallel only. `MergeInterval` (`interval.c:1326-1678 (fn MergeInterval)`) is the
merge it delegates a neighbouring Region to — four arms, one per admissible
neighbour class. `Simplify` (`interval.c:3204-3545 (fn Simplify)`) simplifies the
equivalent Box where there is one, and otherwise re-defines the Interval within
its current Frame by way of a fully-bounded Box.

`Interval::get_def_unc` (`interval.rs:1334-1377 (fn get_def_unc)`) is `GetDefUnc`.
`Interval::map_merge` (`interval.rs:1934-1937 (fn map_merge)`) delegates to
`simplify::region_map_merge` (`mappings/simplify.rs:1484-1531 (fn region_map_merge)`),
which every Region shares. `Interval::region_merge`
(`interval.rs:1459-1651 (fn region_merge)`) is `MergeInterval`, and
`Interval::simplified` (`interval.rs:1838-1899 (fn simplified)`) is `Simplify`, with
its unbounded branch in `simplify_remapped`
(`interval.rs:567-839 (fn simplify_remapped)`).

`MergeInterval` shares its whole tail with `MergeBox` and `MergePointList`, so the
rows from each arm's constructor onwards cite `merge_util.rs`, where that tail
lives once.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `GetDefUnc`: initialise the returned pointer | interval.c:651-653 (fn GetDefUnc) | -- | ceremony | n/a: the Rust returns `AstResult<DynRegion>`, so there is no sentinel to initialise |
| 2 | Check the global error status and bail out | interval.c:654-656 (fn GetDefUnc) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Interval structure | interval.c:657-659 (fn GetDefUnc) | -- | ceremony | n/a: the Rust method already has `&self` |
| 4 | If this Interval is equivalent to a Box, return that Box's default uncertainty | interval.c:660-665 (fn GetDefUnc) | interval.rs:1334-1337 (fn get_def_unc) | architecture | ported |
| 5 | Otherwise use a box covering 1.0E-6 of each axis interval, centred on the origin | interval.c:666-669 (fn GetDefUnc) | interval.rs:1339-1343 (fn get_def_unc) | architecture | ported |
| 6 | Get a pointer to the base Frame | interval.c:670-672 (fn GetDefUnc) | interval.rs:1344 (fn get_def_unc) | architecture | ported |
| 7 | Get the number of base Frame axes | interval.c:673-675 (fn GetDefUnc) | interval.rs:1343 (fn get_def_unc) | architecture | ported |
| 8 | Allocate arrays to hold the bounds of the uncertainty Box | interval.c:676-680 (fn GetDefUnc) | interval.rs:1346-1347 (fn get_def_unc) | ceremony | ported |
| 9 | Ensure the cached bounds are up to date | interval.c:681-683 (fn GetDefUnc) | interval.rs:1334 (fn get_def_unc) | architecture | ported |
| 10 | Do each axis in turn | interval.c:684-686 (fn GetDefUnc) | interval.rs:1350 (fn get_def_unc) | architecture | ported |
| 11 | Where the axis has both limits, use 1.0E-6 of their difference about their mean, falling back to a fraction of the centre when that difference is zero | interval.c:687-699 (fn GetDefUnc) | interval.rs:1353-1360 (fn get_def_unc) | architecture | ported |
| 12 | Otherwise use zero for both bounds | interval.c:700-706 (fn GetDefUnc) | interval.rs:1361-1367 (fn get_def_unc) | architecture | ported |
| 13 | Create the Box | interval.c:707-710 (fn GetDefUnc) | interval.rs:1370-1372 (fn get_def_unc) | architecture | ported |
| 14 | Free the bounds arrays and the base Frame | interval.c:711-716 (fn GetDefUnc) | -- | ceremony | n/a: owned values are dropped |
| 15 | Return NULL if an error occurred | interval.c:717-719 (fn GetDefUnc) | -- | ceremony | n/a: an error is a `Result::Err`, not a value to be annulled alongside one |
| 16 | Return the required pointer | interval.c:720-721 (fn GetDefUnc) | interval.rs:1370 (fn get_def_unc) | architecture | ported |
| 17 | `MapMerge`: initialise the result and the merged-pair index | interval.c:1150-1153 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 18 | Check the global error status and bail out | interval.c:1154-1156 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 19 | Get a pointer to the Interval | interval.c:1157-1159 (fn MapMerge) | -- | ceremony | n/a: the Rust function already has `this: &dyn Region` |
| 20 | Banner: first see whether the Interval reduces on its own, without reference to its neighbours | interval.c:1160-1163 (fn MapMerge) | -- | ceremony | n/a: a section banner, not a step |
| 21 | Simplify the Interval, assuming a changed pointer means something happened | interval.c:1164-1167 (fn MapMerge) | mappings/simplify.rs:1496-1497 (fn region_map_merge) | architecture | ported |
| 22 | Annul the Interval pointer in the list, replace it with the new Region, and mark it forward | interval.c:1168-1174 (fn MapMerge) | mappings/simplify.rs:1498 (fn region_map_merge) | architecture | ported |
| 23 | Return the index of the first modified element | interval.c:1175-1178 (fn MapMerge) | mappings/simplify.rs:1499 (fn region_map_merge) | architecture | ported |
| 24 | Failing that, merge with the Regions on either side — which is possible in parallel only | interval.c:1179-1183 (fn MapMerge) | mappings/simplify.rs:1504 (fn region_map_merge) | architecture | ported |
| 25 | Annul the unwanted simplified pointer | interval.c:1184-1185 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 26 | Attempt to merge the Interval with its lower neighbour, if that neighbour is a Region | interval.c:1186-1194 (fn MapMerge) | mappings/simplify.rs:1506-1514 (fn region_map_merge) | architecture | ported |
| 27 | Failing that, attempt the upper neighbour the same way | interval.c:1195-1205 (fn MapMerge) | mappings/simplify.rs:1518-1527 (fn region_map_merge) | architecture | ported |
| 28 | Act only if one of the two produced a merged Region | interval.c:1206-1208 (fn MapMerge) | mappings/simplify.rs:1511 (fn region_map_merge),1520 (fn region_map_merge) | architecture | ported |
| 29 | Annul the first of the two Mappings, replace it with the merged Region, and clear its invert flag | interval.c:1209-1214 (fn MapMerge) | mappings/simplify.rs:591 (fn replace_pair_with) | architecture | ported |
| 30 | Annul the second and shuffle the rest of the list down to fill the gap | interval.c:1215-1222 (fn MapMerge) | mappings/simplify.rs:592-593 (fn replace_pair_with) | architecture | ported |
| 31 | Clear the vacated element at the end | interval.c:1223-1226 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entry leaves no slot to blank |
| 32 | Decrement the Mapping count and return the index of the first modified element | interval.c:1227-1232 (fn MapMerge) | mappings/simplify.rs:1513 (fn region_map_merge),1522 (fn region_map_merge) | architecture | ported |
| 33 | In series, annul the simplified pointer and do nothing | interval.c:1233-1236 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and the series case falls through to `None` |
| 34 | Return the result | interval.c:1237-1239 (fn MapMerge) | mappings/simplify.rs:1530 (fn region_map_merge) | architecture | ported |
| 35 | `MergeInterval`: initialise the returned pointer and the bounds arrays | interval.c:1326-1330 (fn MergeInterval) | -- | ceremony | n/a: the Rust returns `Option<DynMapping>`, so "no merge" is `None` |
| 36 | Check the local error status and bail out | interval.c:1331-1333 (fn MergeInterval) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 37 | Both Regions must agree on Closed, and either both or neither must have a defined uncertainty | interval.c:1334-1340 (fn MergeInterval) | interval.rs:1464-1468 (fn region_merge) | architecture | ported |
| 38 | Get the Negated attributes of the two Regions | interval.c:1341-1344 (fn MergeInterval) | interval.rs:1470 (fn region_merge) | architecture | ported |
| 39 | Get the number of axes in the two supplied Regions | interval.c:1345-1348 (fn MergeInterval) | interval.rs:1471-1472 (fn region_merge) | architecture | ported |
| 40 | Get the number of axes the combination will have | interval.c:1349-1352 (fn MergeInterval) | interval.rs:1479 (fn region_merge) | architecture | ported |
| 41 | Get the base Frames from the two Region FrameSets | interval.c:1353-1357 (fn MergeInterval) | interval.rs:1562-1563 (fn region_merge) | architecture | ported |
| 42 | Combine them into a single CmpFrame, in the requested order, for use in creating any new Region | interval.c:1358-1363 (fn MergeInterval) | interval.rs:1564-1568 (fn region_merge) | architecture | ported |
| 43 | Annul the two base Frame pointers | interval.c:1364-1366 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 44 | Indicate we do not yet have a merged Region | interval.c:1367-1369 (fn MergeInterval) | -- | ceremony | n/a: the Rust builds an `Option<MergedRegion>`, so "no merge yet" is the `None` any arm can return |
| 45 | First attempt to merge with another Interval, which needs both Intervals un-negated | interval.c:1370-1373 (fn MergeInterval) | interval.rs:1490-1495 (fn region_merge) | architecture | ported |
| 46 | Allocate memory to store the bounds of the returned Interval | interval.c:1374-1377 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 47 | Copy the limits from the two supplied Intervals into those arrays, in the requested order | interval.c:1378-1387 (fn MergeInterval) | interval.rs:1496-1497 (fn region_merge),1185-1195 (fn set_closed) | architecture | ported |
| 48 | Create the new Interval, initially with no uncertainty | interval.c:1388-1391 (fn MergeInterval) | merge_util.rs:162-166 (fn finish_region_merge),159 (fn finish_region_merge) | architecture | ported |
| 49 | Free the bounds arrays | interval.c:1392-1395 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 50 | Now attempt to merge with a Box, giving an Interval, which needs both Regions un-negated | interval.c:1396-1399 (fn MergeInterval) | interval.rs:1500-1504 (fn region_merge) | architecture | ported |
| 51 | Allocate memory to store the bounds of the returned Interval | interval.c:1400-1403 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 52 | Get the bounds from the Interval into its part of those arrays | interval.c:1404-1410 (fn MergeInterval) | interval.rs:1506-1507 (fn region_merge) | architecture | ported |
| 53 | Copy the centre and corner from the supplied Box into the other part | interval.c:1411-1421 (fn MergeInterval) | interval.rs:1505 (fn region_merge) | architecture | ported |
| 54 | Convert those centre and corner positions into upper and lower bounds, swapping any pair the conversion inverts | interval.c:1422-1433 (fn MergeInterval) | box_region.rs:889 (fn reg_base_box) | architecture | ported |
| 55 | Create the new Interval, initially with no uncertainty | interval.c:1434-1437 (fn MergeInterval) | merge_util.rs:162-166 (fn finish_region_merge),159 (fn finish_region_merge) | architecture | ported |
| 56 | Free the bounds arrays | interval.c:1438-1441 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 57 | Now attempt to merge with a NullRegion, giving an Interval; the NullRegion must be negated and the Interval must not | interval.c:1442-1445 (fn MergeInterval) | interval.rs:1510-1514 (fn region_merge) | architecture | ported |
| 58 | Allocate memory to store the bounds of the returned Interval | interval.c:1446-1449 (fn MergeInterval) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 59 | Copy the Interval's limits in and store bad values on the NullRegion's axes, marking them unbounded | interval.c:1450-1465 (fn MergeInterval) | interval.rs:1516-1518 (fn region_merge) | architecture | ported |
| 60 | Create the new Interval, initially with no uncertainty | interval.c:1466-1469 (fn MergeInterval) | merge_util.rs:162-166 (fn finish_region_merge),159 (fn finish_region_merge) | architecture | ported |
| 61 | Free the bounds arrays | interval.c:1470-1473 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 62 | Now attempt to merge with a PointList, giving a PointList, which needs both Regions un-negated | interval.c:1474-1477 (fn MergeInterval) | interval.rs:1521-1526 (fn region_merge) | architecture | ported |
| 63 | Get the Interval's own base bounding box into the two bounds arrays | interval.c:1478-1483 (fn MergeInterval) | interval.rs:1538 (fn region_merge) | architecture | ported |
| 64 | Get the size of the Interval's base-Frame uncertainty into its own two arrays | interval.c:1484-1489 (fn MergeInterval) | interval.rs:1539-1540 (fn region_merge) | architecture | ported |
| 65 | Refuse the merge unless the Interval's half-width is under a quarter of the uncertainty width on every axis, replacing the lower bounds with the axis centres as it goes | interval.c:1490-1507 (fn MergeInterval) | interval.rs:1542-1548 (fn region_merge) | architecture | ported |
| 66 | Go on to create a new PointList only if the Interval is a point | interval.c:1508-1510 (fn MergeInterval) | interval.rs:1545 (fn region_merge) | architecture | ported |
| 67 | Get a PointSet holding the supplied PointList's axis values, its point count and pointers to its arrays | interval.c:1511-1517 (fn MergeInterval) | interval.rs:1550-1553 (fn region_merge) | architecture | ported |
| 68 | Create a new PointSet with room for the same points but the extra axes, and get pointers to its arrays | interval.c:1518-1522 (fn MergeInterval) | interval.rs:1483 (fn region_merge) | architecture | ported |
| 69 | Copy the PointList axis values into the new PointSet, in the requested order | interval.c:1523-1532 (fn MergeInterval) | interval.rs:1482-1492 (fn region_merge),1505 (fn region_merge) | architecture | ported |
| 70 | Add the axis values the Interval defines to every point | interval.c:1533-1538 (fn MergeInterval) | interval.rs:1547 (fn region_merge),1505 (fn region_merge) | architecture | ported |
| 71 | Create the new PointList, initially with no uncertainty | interval.c:1539-1543 (fn MergeInterval) | merge_util.rs:172-176 (fn finish_region_merge),159 (fn finish_region_merge) | architecture | ported |
| 72 | Free the two PointSets | interval.c:1544-1548 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 73 | Free the bounds arrays and the uncertainty | interval.c:1549-1556 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 74 | If a new Region was created above, propagate the supplied Region's remaining attributes to it | interval.c:1557-1561 (fn MergeInterval) | merge_util.rs:148-157 (fn finish_region_merge) | architecture | ported |
| 75 | Ensure the Negated attribute the constructors set is not the one the overlay left | interval.c:1562-1566 (fn MergeInterval) | merge_util.rs:155 (fn finish_region_merge) | architecture | ported |
| 76 | Assign the new Region an uncertainty only when both supplied Regions have one | interval.c:1567-1570 (fn MergeInterval) | merge_util.rs:300 (fn combine_uncertainties) | architecture | ported |
| 77 | Get the base-Frame uncertainties from the two supplied Regions | interval.c:1571-1574 (fn MergeInterval) | interval.rs:1610-1613 (fn region_merge) | architecture | ported |
| 78 | Combine them into a single Prism, in the correct order | interval.c:1575-1581 (fn MergeInterval) | merge_util.rs:301 (fn combine_uncertainties),303-305 (fn combine_uncertainties) | architecture | ported |
| 79 | Attempt to simplify the Prism | interval.c:1582-1584 (fn MergeInterval) | merge_util.rs:306 (fn combine_uncertainties) | architecture | ported |
| 80 | Use the simplified Prism as the uncertainty for the returned Region | interval.c:1585-1587 (fn MergeInterval) | merge_util.rs:179-181 (fn finish_region_merge) | architecture | ported |
| 81 | Free the uncertainty resources | interval.c:1588-1594 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 82 | Get the current Frames from the two Region FrameSets and combine them into one CmpFrame | interval.c:1595-1605 (fn MergeInterval) | interval.rs:1571-1577 (fn region_merge) | architecture | ported |
| 83 | Get the two base-to-current Mappings and combine them into one parallel CmpMap connecting the two combined Frames | interval.c:1606-1619 (fn MergeInterval) | interval.rs:1580-1607 (fn region_merge) | architecture | ported |
| 84 | Map the new Region into the new current Frame | interval.c:1620-1622 (fn MergeInterval) | merge_util.rs:183 (fn finish_region_merge) | architecture | ported |
| 85 | The result's FillFactor is the product of the two supplied FillFactors, set only when either was set | interval.c:1623-1629 (fn MergeInterval) | merge_util.rs:69-71 (fn apply_merge_scaling_to) | architecture | ported |
| 86 | Scale the result's MeshSize only when either supplied MeshSize was set | interval.c:1630-1637 (fn MergeInterval) | merge_util.rs:73 (fn apply_merge_scaling_to) | architecture | ported |
| 87 | Get the two MeshSize values, then clear them so the defaults are returned | interval.c:1638-1645 (fn MergeInterval) | merge_util.rs:74-75 (fn apply_merge_scaling_to) | architecture | ported |
| 88 | Form each Region's ratio of used to default MeshSize | interval.c:1646-1650 (fn MergeInterval) | merge_util.rs:76-77 (fn apply_merge_scaling_to) | architecture | ported |
| 89 | Set the result's MeshSize to its own default scaled by the product of the two ratios | interval.c:1651-1654 (fn MergeInterval) | merge_util.rs:78-79 (fn apply_merge_scaling_to) | architecture | ported |
| 90 | Re-instate the original MeshSize values on the supplied Regions | interval.c:1655-1660 (fn MergeInterval) | -- | ceremony | n/a: nothing was cleared on the supplied Regions, so there is nothing to restore |
| 91 | Free the Frame, Mapping and intermediate-Region resources | interval.c:1661-1669 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 92 | Free the combined base Frame | interval.c:1670-1673 (fn MergeInterval) | -- | ceremony | n/a: owned values are dropped |
| 93 | Annul the returned pointer if an error occurred | interval.c:1674-1676 (fn MergeInterval) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 94 | Return the result | interval.c:1677-1678 (fn MergeInterval) | merge_util.rs:189 (fn finish_region_merge) | architecture | ported |
| 95 | `Simplify`: initialise the returned pointer | interval.c:3204-3206 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no sentinel to initialise |
| 96 | Check the global error status and bail out | interval.c:3207-3209 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 97 | Get a pointer to the supplied Region structure | interval.c:3210-3212 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 98 | Get a pointer to the supplied Interval structure | interval.c:3213-3215 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 99 | If this Interval is equivalent to a Box, simplify that Box and return the result | interval.c:3216-3221 (fn Simplify) | interval.rs:1840-1839 (fn simplified) | architecture | ported |
| 100 | Otherwise use the implementation appropriate for unbounded intervals | interval.c:3222-3225 (fn Simplify) | interval.rs:1838 (fn simplified) | architecture | ported |
| 101 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and uncertainty | interval.c:3226-3230 (fn Simplify) | interval.rs:1844 (fn simplified) | architecture | ported |
| 102 | Note whether any simplification took place, from whether the returned pointer changed | interval.c:3231-3235 (fn Simplify) | interval.rs:1858 (fn simplified) | architecture | ported |
| 103 | Attempt the re-definition only when the base-to-current Mapping is not a UnitMap | interval.c:3236-3240 (fn Simplify) | interval.rs:1860-1865 (fn simplified) | architecture | ported |
| 104 | Copy the base-Frame bounds, giving a complete box by replacing any missing limit with an arbitrary non-bad value | interval.c:3241-3255 (fn Simplify) | interval.rs:576-602 (fn simplify_remapped) | architecture | ported |
| 105 | Ensure a good upper bound on each axis, from the lower bound's sign where there is one | interval.c:3256-3271 (fn Simplify) | interval.rs:582-592 (fn simplify_remapped) | architecture | ported |
| 106 | Ensure a good lower bound on each axis the same way | interval.c:3272-3286 (fn Simplify) | interval.rs:593-601 (fn simplify_remapped) | architecture | ported |
| 107 | Transform the box corners into the current Frame and back, then widen the box to enclose both the original and the returned positions | interval.c:3287-3317 (fn Simplify) | interval.rs:608-632 (fn simplify_remapped) | architecture | ported |
| 108 | Limit the box to the Interval's own limits, handling an excluded axis by collapsing to whichever side is wider | interval.c:3318-3345 (fn Simplify) | interval.rs:637-670 (fn simplify_remapped) | architecture | ported |
| 109 | Ensure the two bounds on each axis are not equal | interval.c:3346-3354 (fn Simplify) | interval.rs:672-677 (fn simplify_remapped) | architecture | ported |
| 110 | Create a Box on the base Frame from the box found above, carrying the Interval's base-Frame uncertainty where it has one | interval.c:3355-3360 (fn Simplify) | interval.rs:683-701 (fn simplify_remapped) | architecture | ported |
| 111 | Modify that Box so it has the same current Frame as this Interval | interval.c:3361-3364 (fn Simplify) | interval.rs:702-703 (fn simplify_remapped) | architecture | ported |
| 112 | Try simplifying the Box | interval.c:3365-3367 (fn Simplify) | interval.rs:704 (fn simplify_remapped) | architecture | ported |
| 113 | Proceed only if the Box was simplified | interval.c:3368-3370 (fn Simplify) | interval.rs:717-746 (fn simplify_remapped) | architecture | ported |
| 114 | Return the simplified Box where it is a NullRegion | interval.c:3371-3376 (fn Simplify) | interval.rs:714-716 (fn simplify_remapped) | architecture | ported |
| 115 | Otherwise act where it is a Box or an Interval | interval.c:3377-3379 (fn Simplify) | interval.rs:717-718 (fn simplify_remapped) | architecture | ported |
| 116 | Get the bounds of the simplified Region, from its defining points for an Interval and from its base bounding box for a Box | interval.c:3380-3396 (fn Simplify) | interval.rs:722-740 (fn simplify_remapped) | architecture | ported |
| 117 | Create a PointSet holding one point per axis of the simplified Box, plus an extra | interval.c:3397-3402 (fn Simplify) | interval.rs:754 (fn simplify_remapped) | architecture | ported |
| 118 | Put its lower bounds in the first point and displace each remaining point along one axis by that axis's box length | interval.c:3403-3415 (fn Simplify) | interval.rs:755-760 (fn simplify_remapped) | architecture | ported |
| 119 | Transform that PointSet into this Interval's base Frame with the inverse Mapping | interval.c:3416-3421 (fn Simplify) | interval.rs:761 (fn simplify_remapped) | architecture | ported |
| 120 | For each current Frame axis, find the base Frame axis with the largest increment between the two corresponding base positions | interval.c:3422-3446 (fn Simplify) | interval.rs:764-781 (fn simplify_remapped) | architecture | ported |
| 121 | A zero largest increment means the axis is absent from the base Frame, so give it equal limits at the centre | interval.c:3447-3462 (fn Simplify) | interval.rs:783-791 (fn simplify_remapped) | architecture | ported |
| 122 | Otherwise transfer the base Frame axis's missing limits to the current Frame axis | interval.c:3463-3469 (fn Simplify) | interval.rs:792-800 (fn simplify_remapped) | architecture | ported |
| 123 | Where the original limits were equal, make the new limits equal too | interval.c:3470-3475 (fn Simplify) | interval.rs:801-802 (fn simplify_remapped) | architecture | ported |
| 124 | Where the original interval was an inclusion, keep the new one an inclusion by swapping the limits if required | interval.c:3476-3484 (fn Simplify) | interval.rs:803-807 (fn simplify_remapped) | architecture | ported |
| 125 | Where it was an exclusion, keep the new one an exclusion the same way | interval.c:3485-3496 (fn Simplify) | interval.rs:808-811 (fn simplify_remapped) | architecture | ported |
| 126 | Create the simplified Interval from those current Frame limits, carrying the current-Frame uncertainty, and use it in place of the original | interval.c:3497-3505 (fn Simplify) | interval.rs:815-838 (fn simplify_remapped) | architecture | ported |
| 127 | Free the two PointSets and the simplified bounds arrays | interval.c:3506-3513 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 128 | Free the Frames, the two Boxes, the simplified Region and the bounds arrays | interval.c:3514-3523 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 129 | Free the base-to-current Mapping | interval.c:3524-3526 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 130 | If anything simplified, overlay the supplied Region's attributes onto the result and return it; otherwise return a clone of the supplied pointer | interval.c:3527-3540 (fn Simplify) | interval.rs:1877-1894 (fn simplified) | architecture | ported |
| 131 | Annul the returned pointer if an error occurred | interval.c:3541-3543 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 132 | Return the result | interval.c:3544-3545 (fn Simplify) | interval.rs:1891 (fn simplified),1470 (fn region_merge) | architecture | ported |

## Rows 11 and 12 — the guard the base class gets wrong

Interval tests `lb != -f64::MAX && ub != f64::MAX`
(`interval.rs:1353 (fn get_def_unc)`), which is the correct rendering of C's
`±DBL_MAX` test in both directions. The base-class `default_bounding_box_unc` uses
`is_bad` instead and so misses the upper case — see PLAN.md §10.12. This class is
the reference for what that guard should look like.

## Row 99 — the Box path comes first, and it hands the Box to the simplifier

C dispatches on Box-equivalence before doing any remap work, and the port keeps
that order (`interval.rs:1840-1839 (fn simplified)`). The condition is C's: every
resolved limit finite, every axis sharing axis 1's included/excluded orientation,
and axis 1 including rather than excluding its interval. A Negated Interval still
yields a Box — C negates the Box (`interval.c:524 (fn Cache)`) rather than
declining — and the Interval's `Closed` value transfers with an explicit
`astSetClosed` (`interval.c:527 (fn Cache)`).

What the branch returns is `astSimplify( box )`, not the Box. That matters
because `Cache` builds the Box in the **base** Frame and, when the base→current
Mapping is not a UnitMap, maps it into the current Frame with `astMapRegion`
(`interval.c:510-514 (fn Cache)`) — which extends the Box's FrameSet rather than
moving its points. The Mapping therefore reaches `Box::Simplify`, which is what
re-expresses the bounds in the current Frame's units. A reclassification that
labelled the base-Frame numbers with the current Frame and returned would drop
that Mapping; PLAN.md §10.25 records what such a divergence costs.

C applies no `astRegOverlay` on this branch, so nothing from the Interval is
carried onto the result beyond what `Cache` put on the Box.

## Rows 9 and 99 and the `reg_pins_mask` delegation — one `Cache`

`Cache` populates two things: the resolved limit arrays and, conditionally, the
equivalent Box. Three C bodies read them — `Simplify`
(`interval.c:3218 (fn Simplify)`), `GetDefUnc` (`interval.c:661 (fn GetDefUnc)`,
`interval.c:683 (fn GetDefUnc)`) and `RegPins` (`interval.c:2702 (fn RegPins)`) —
so `Interval::cache` returns the pair of arrays alongside the optional Box and all
three call sites take it from there. `interval.rs:489-552 (fn cache)` is the only
copy of the classification, reached from `interval.rs:1068 (fn reg_pins_mask)`,
`interval.rs:1334 (fn get_def_unc)` and `interval.rs:1840 (fn simplified)`.

## Rows 104 to 109 — arbitrary limits, then reined back in

The re-definition has to transform a *bounded* box, so C fills each missing limit
with an arbitrary value, transforms, and then limits the result back to the
Interval's real limits. Skipping the second half would widen the Interval; the
port does both (`interval.rs:574-678 (fn simplify_remapped)`).

## Row 110 — the Box carries the Interval's base-Frame uncertainty

C reads `astGetUncFrm( new, AST__BASE )` and hands it to `astBox`
(`interval.c:3357 (fn Simplify)`), so the Box whose simplification decides the
whole branch is built with the Interval's uncertainty rather than with none. That
matters twice: `Box::Simplify` consults the uncertainty when deciding what the Box
reduces to, and `astRegOverlay` never installs an uncertainty of its own — its
only uncertainty action is to *clear* one (`region.c:9945 (fn RegOverlay)`) — so a
Box built without it stays without it. The port passes the same Region
(`interval.rs:695-701 (fn simplify_remapped)`), read through `get_unc_frm`
(`regions/mod.rs:788 (fn get_unc_frm)`), which is `astGetUncFrm`.

## Rows 113 and 114 — the port's "was it simplified?" signal

C compares pointers: `sreg != box2` (`interval.c:3369 (fn Simplify)`) is true
whenever `astSimplify` returned anything other than a clone of what it was given.
`BoxRegion::simplified` returns an unchanged *clone* when it cannot reduce the Box,
so a pointer comparison has no analogue here. The equivalent signal is the one
thing a successful Box simplification always achieves: its base→current Mapping
becomes a UnitMap. The port therefore tests that
(`interval.rs:722-740 (fn simplify_remapped)`) and treats a Box or Interval whose
Mapping is still non-trivial as unsimplified, which is C's `else` at
`interval.c:3522 (fn Simplify)`.

A NullRegion is checked before that test (`interval.rs:714-716 (fn simplify_remapped)`)
because C takes it as the answer in its own right, cloning it over `new` and
setting `simpler` (`interval.c:3371-3375 (fn Simplify)`). It is already expressed
in the current Frame, so there are no limits to recover and no axis
correspondence to work out. Testing for it first is safe: an unsimplified result
is always the Box that went in.

## Rows 63 to 66 — the PointList arm measures the Interval

Attaching one constant per Interval axis to every point is equivalent only when
the Interval is a point, so C reads its base bounding box
(`interval.c:1482 (fn MergeInterval)`), reads its base-Frame uncertainty into two
arrays of their own (`interval.c:1488 (fn MergeInterval)`), and refuses the merge
on any axis whose half-width exceeds a quarter of the uncertainty width
(`interval.c:1499-1500 (fn MergeInterval)`) — the test `MergeBox` applies to a
Box's half-width. The axis values the merged PointList receives are the centres
of the Interval's own axes (`interval.c:1504 (fn MergeInterval)`).

`interval.rs:1538-1548 (fn region_merge)` is that test and those values. Only an
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
(`interval.c:1565 (fn MergeInterval)`). The port never sets it in the first place:
`finish_region_merge` takes `negated: false` from the caller
(`interval.rs:1634 (fn region_merge)`) and applies it with the rest of the overlay
(`merge_util.rs:155 (fn finish_region_merge)`). The arms that require a negated
neighbour — the NullRegion arm at `interval.c:1444 (fn MergeInterval)` — encode
that negation in the bounds they build, not in the result's flag.

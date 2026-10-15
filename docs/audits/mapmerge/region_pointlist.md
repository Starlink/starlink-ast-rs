# `Simplify` / `MapMerge` audit — PointList

C source: reference/starlink-ast/src/pointlist.c
Rust: crates/ast-core/src/regions/pointlist.rs
Audited: 2026-09-02 against pin 3e60abaa
Citations: verified against pin 3b9dc639
Extension points: classified

Uncited C steps:
- pointlist.c:1120 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- pointlist.c:1261 (fn MergePointList) — `Local Variables:` is the local-variable declaration block, not a step
- pointlist.c:2220 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Three C bodies. `MapMerge` (`pointlist.c:1128-1217 (fn MapMerge)`) is the Region shape:
self-simplify first, and only failing that merge with a neighbour, in parallel
only. `MergePointList` (`pointlist.c:1294-1502 (fn MergePointList)`) is the merge it
delegates a neighbouring Region to — two single-point PointLists become one
PointList over the combined axes. `Simplify`
(`pointlist.c:2232-2304 (fn Simplify)`) calls the parent and, unless the base-to-current
Mapping is already a UnitMap, transforms the defining points into the current
Frame and rebuilds the PointList there, so the result needs only a UnitMap.

`PointList::map_merge` (`regions/pointlist.rs:990-993 (fn map_merge)`) delegates to
`simplify::region_map_merge` (`mappings/simplify.rs:1501-1547 (fn region_map_merge)`), which
every Region shares. `PointList::region_merge`
(`regions/pointlist.rs:582-705 (fn region_merge)`) is `MergePointList`, and
`PointList::simplified` (`regions/pointlist.rs:904-959 (fn simplified)`) is `Simplify`.

`MergePointList` shares its whole tail with `MergeBox` and `MergeInterval`, so the
rows from the constructor onwards cite `merge_util.rs`, where that tail lives once.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `MapMerge`: initialise the result and the merged-pair index | pointlist.c:1128-1131 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | pointlist.c:1132-1134 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the PointList | pointlist.c:1135-1137 (fn MapMerge) | -- | ceremony | n/a: the Rust function already has `this: &dyn Region` |
| 4 | Banner: first see whether the PointList reduces on its own, without reference to its neighbours | pointlist.c:1138-1141 (fn MapMerge) | -- | ceremony | n/a: a section banner, not a step |
| 5 | Simplify the PointList, assuming a changed pointer means something happened | pointlist.c:1142-1145 (fn MapMerge) | mappings/simplify.rs:1513 (fn region_map_merge) | architecture | ported |
| 6 | Annul the PointList pointer in the list, replace it with the new Region, and mark it forward | pointlist.c:1146-1152 (fn MapMerge) | mappings/simplify.rs:1514 (fn region_map_merge) | architecture | ported |
| 7 | Return the index of the first modified element | pointlist.c:1153-1156 (fn MapMerge) | mappings/simplify.rs:1515 (fn region_map_merge) | architecture | ported |
| 8 | Failing that, merge with the Regions on either side — which is possible in parallel only | pointlist.c:1157-1161 (fn MapMerge) | mappings/simplify.rs:1520 (fn region_map_merge) | architecture | ported |
| 9 | Annul the unwanted simplified pointer | pointlist.c:1162-1163 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 10 | Attempt to merge the PointList with its lower neighbour, if that neighbour is a Region | pointlist.c:1164-1172 (fn MapMerge) | mappings/simplify.rs:1522-1530 (fn region_map_merge) | architecture | ported |
| 11 | Failing that, attempt the upper neighbour the same way | pointlist.c:1173-1183 (fn MapMerge) | mappings/simplify.rs:1534-1543 (fn region_map_merge) | architecture | ported |
| 12 | Act only if one of the two produced a merged Region | pointlist.c:1184-1186 (fn MapMerge) | mappings/simplify.rs:1527 (fn region_map_merge),1536 (fn region_map_merge) | architecture | ported |
| 13 | Annul the first of the two Mappings, replace it with the merged Region, and clear its invert flag | pointlist.c:1187-1192 (fn MapMerge) | mappings/simplify.rs:590 (fn replace_pair_with) | architecture | ported |
| 14 | Annul the second and shuffle the rest of the list down to fill the gap | pointlist.c:1193-1200 (fn MapMerge) | mappings/simplify.rs:590 (fn replace_pair_with) | architecture | ported |
| 15 | Clear the vacated element at the end | pointlist.c:1201-1204 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entry leaves no slot to blank |
| 16 | Decrement the Mapping count and return the index of the first modified element | pointlist.c:1205-1210 (fn MapMerge) | mappings/simplify.rs:1529 (fn region_map_merge),1538 (fn region_map_merge) | architecture | ported |
| 17 | In series, annul the simplified pointer and do nothing | pointlist.c:1211-1214 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and the series case falls through to `None` |
| 18 | Return the result | pointlist.c:1215-1217 (fn MapMerge) | mappings/simplify.rs:1546 (fn region_map_merge) | architecture | ported |
| 19 | `MergePointList`: initialise the returned pointer | pointlist.c:1294-1296 (fn MergePointList) | -- | ceremony | n/a: the Rust returns `Option<DynMapping>`, so "no merge" is `None` |
| 20 | Check the local error status and bail out | pointlist.c:1297-1299 (fn MergePointList) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 21 | Both Regions must agree on Closed, and either both or neither must have a defined uncertainty | pointlist.c:1300-1306 (fn MergePointList) | regions/pointlist.rs:592-593 (fn region_merge) | architecture | ported |
| 22 | Get the Negated attributes of the two Regions | pointlist.c:1307-1310 (fn MergePointList) | regions/pointlist.rs:594 (fn region_merge) | architecture | ported |
| 23 | Merge only with another PointList, of equal Negated, each holding a single point | pointlist.c:1311-1318 (fn MergePointList) | regions/pointlist.rs:591 (fn region_merge),598-600 (fn region_merge) | architecture | ported |
| 24 | Get the number of axes in the two supplied Regions | pointlist.c:1319-1322 (fn MergePointList) | regions/pointlist.rs:601-602 (fn region_merge) | architecture | ported |
| 25 | Get the number of axes the combination will have | pointlist.c:1323-1325 (fn MergePointList) | regions/pointlist.rs:609 (fn region_merge) | architecture | ported |
| 26 | Combine the two base Frames into one CmpFrame, in the requested order | pointlist.c:1326-1336 (fn MergePointList) | regions/pointlist.rs:618-624 (fn region_merge) | architecture | ported |
| 27 | Annul the two base Frame pointers | pointlist.c:1337-1339 (fn MergePointList) | -- | ceremony | n/a: owned values are dropped |
| 28 | Get pointers to the coordinate data in the two PointLists | pointlist.c:1340-1345 (fn MergePointList) | regions/pointlist.rs:611-615 (fn region_merge) | architecture | ported |
| 29 | Create a PointSet to hold the single point of the returned PointList | pointlist.c:1346-1348 (fn MergePointList) | regions/pointlist.rs:609 (fn region_merge) | architecture | ported |
| 30 | Get pointers to its data | pointlist.c:1349-1351 (fn MergePointList) | -- | ceremony | n/a: a `Vec<f64>` is written directly, with no separate data-pointer step |
| 31 | Check the pointers can be used safely | pointlist.c:1352-1354 (fn MergePointList) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test |
| 32 | Copy the point positions of the selected axes across in the requested order | pointlist.c:1355-1373 (fn MergePointList) | regions/pointlist.rs:610-616 (fn region_merge) | architecture | ported |
| 33 | Create the new PointList on the combined base Frame | pointlist.c:1374-1377 (fn MergePointList) | merge_util.rs:172-176 (fn finish_region_merge),159 (fn finish_region_merge) | architecture | ported |
| 34 | Propagate the remaining attributes of the supplied Region to it | pointlist.c:1378-1380 (fn MergePointList) | merge_util.rs:148-157 (fn finish_region_merge) | architecture | ported |
| 35 | Ensure the Negated flag is set correctly in the returned PointList | pointlist.c:1381-1387 (fn MergePointList) | merge_util.rs:155 (fn finish_region_merge) | architecture | ported |
| 36 | Assign the result an uncertainty only when both supplied Regions have one | pointlist.c:1388-1391 (fn MergePointList) | merge_util.rs:300 (fn combine_uncertainties) | architecture | ported |
| 37 | Get the base-Frame uncertainties from the two supplied Regions | pointlist.c:1392-1395 (fn MergePointList) | regions/pointlist.rs:668-672 (fn region_merge) | architecture | ported |
| 38 | Combine them into a single Prism, in the correct order | pointlist.c:1396-1402 (fn MergePointList) | merge_util.rs:301 (fn combine_uncertainties),303-305 (fn combine_uncertainties) | architecture | ported |
| 39 | Attempt to simplify the Prism | pointlist.c:1403-1405 (fn MergePointList) | merge_util.rs:306 (fn combine_uncertainties) | architecture | ported |
| 40 | Use the simplified Prism as the uncertainty for the returned Region | pointlist.c:1406-1408 (fn MergePointList) | merge_util.rs:179-181 (fn finish_region_merge) | architecture | ported |
| 41 | Free the uncertainty resources | pointlist.c:1409-1414 (fn MergePointList) | -- | ceremony | n/a: owned values are dropped |
| 42 | Combine the two current Frames into one CmpFrame, in the same order | pointlist.c:1415-1425 (fn MergePointList) | regions/pointlist.rs:628-634 (fn region_merge) | architecture | ported |
| 43 | Combine the two base-to-current Mappings into one parallel CmpMap | pointlist.c:1426-1439 (fn MergePointList) | regions/pointlist.rs:637-664 (fn region_merge) | architecture | ported |
| 44 | Map the new Region into the new current Frame | pointlist.c:1440-1442 (fn MergePointList) | merge_util.rs:183 (fn finish_region_merge) | architecture | ported |
| 45 | The result's FillFactor is the product of the two supplied FillFactors, set only when either was set | pointlist.c:1443-1449 (fn MergePointList) | merge_util.rs:69-71 (fn apply_merge_scaling_to) | architecture | ported |
| 46 | Scale the result's MeshSize only when either supplied MeshSize was set | pointlist.c:1450-1457 (fn MergePointList) | merge_util.rs:73 (fn apply_merge_scaling_to) | architecture | ported |
| 47 | Get the two MeshSize values, then clear them so the defaults are returned | pointlist.c:1458-1465 (fn MergePointList) | merge_util.rs:74-75 (fn apply_merge_scaling_to) | architecture | ported |
| 48 | Form each Region's ratio of used to default MeshSize | pointlist.c:1466-1470 (fn MergePointList) | merge_util.rs:76-77 (fn apply_merge_scaling_to) | architecture | ported |
| 49 | Set the result's MeshSize to its own default scaled by the product of the two ratios | pointlist.c:1471-1474 (fn MergePointList) | merge_util.rs:78-79 (fn apply_merge_scaling_to) | architecture | ported |
| 50 | Re-instate the original MeshSize values on the supplied Regions | pointlist.c:1475-1480 (fn MergePointList) | -- | ceremony | n/a: nothing was cleared on the supplied Regions, so there is nothing to restore |
| 51 | Free the Frame, Mapping and intermediate-Region resources | pointlist.c:1481-1490 (fn MergePointList) | -- | ceremony | n/a: owned values are dropped |
| 52 | Free the combined base Frame and the PointSet | pointlist.c:1491-1495 (fn MergePointList) | -- | ceremony | n/a: owned values are dropped |
| 53 | Annul the returned pointer if an error occurred | pointlist.c:1496-1499 (fn MergePointList) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 54 | Return the result | pointlist.c:1500-1502 (fn MergePointList) | merge_util.rs:189 (fn finish_region_merge) | architecture | ported |
| 55 | `Simplify`: initialise the returned pointer | pointlist.c:2232-2234 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so there is no sentinel to initialise |
| 56 | Check the global error status and bail out | pointlist.c:2235-2237 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 57 | Obtain a pointer to the Region structure | pointlist.c:2238-2240 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 58 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and uncertainty | pointlist.c:2241-2244 (fn Simplify) | regions/pointlist.rs:904 (fn simplified) | architecture | ported |
| 59 | Note whether any simplification took place, from whether the returned pointer changed | pointlist.c:2245-2249 (fn Simplify) | regions/pointlist.rs:918 (fn simplified) | architecture | ported |
| 60 | Get the base-to-current Mapping and act only when it is not already a UnitMap | pointlist.c:2250-2256 (fn Simplify) | regions/pointlist.rs:920-924 (fn simplified) | architecture | ported |
| 61 | Get a pointer to the current Region Frame | pointlist.c:2257-2259 (fn Simplify) | regions/pointlist.rs:936 (fn simplified) | architecture | ported |
| 62 | Get the PointSet holding the base Frame positions that define this PointList | pointlist.c:2260-2263 (fn Simplify) | regions/pointlist.rs:938 (fn simplified) | architecture | ported |
| 63 | Transform the PointSet using that Mapping | pointlist.c:2264-2266 (fn Simplify) | regions/pointlist.rs:935 (fn simplified) | architecture | ported |
| 64 | Get the positional uncertainty of the simplified PointList in its current Frame | pointlist.c:2267-2270 (fn Simplify) | regions/pointlist.rs:947 (fn simplified) | architecture | ported |
| 65 | Create a new PointList from the transformed points and that uncertainty, use it in place of the original, and record that something simplified | pointlist.c:2271-2276 (fn Simplify) | regions/pointlist.rs:948-950 (fn simplified),937 (fn simplified) | architecture | ported |
| 66 | Free the Frame, PointSet and uncertainty resources | pointlist.c:2277-2281 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 67 | Free the base-to-current Mapping | pointlist.c:2282-2285 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 68 | If anything simplified, overlay the supplied Region's attributes onto the result and return it | pointlist.c:2286-2293 (fn Simplify) | regions/pointlist.rs:951 (fn simplified),937 (fn simplified) | architecture | ported |
| 69 | Otherwise return a clone of the supplied pointer | pointlist.c:2294-2298 (fn Simplify) | regions/pointlist.rs:926-931 (fn simplified) | architecture | ported |
| 70 | Annul the returned pointer if an error occurred | pointlist.c:2299-2301 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 71 | Return the result | pointlist.c:2302-2304 (fn Simplify) | regions/pointlist.rs:958 (fn simplified) | architecture | ported |

## Row 60 — why the rebuild is worth doing

C's comment gives the reason: rebuilding the PointList in the current Frame means
the simplified Region needs only a UnitMap, which lets the rest of the simplifier
treat it as already reduced and lets the serialiser record one Frame instead of a
whole FrameSet. The port keeps the same short-circuit when the Mapping is already
a UnitMap (`regions/pointlist.rs:924 (fn simplified)`), so the rebuild happens exactly when it
buys something.

## Rows 64 and 65 — the uncertainty is installed through the constructor

C reads the uncertainty with `astGetUncFrm( new, AST__CURRENT )` and hands it to
`astPointList`, whose `astInitRegion` calls the private `SetUnc`
(`region.c:10781 (fn SetUnc)`). That maps the supplied uncertainty into the new
PointList's base Frame, ensures it is bounded, and re-centres it on the first
defining position (`region.c:10898-10903 (fn SetUnc)`). Reading in the current Frame is
what makes the mapping meaningful: the rebuilt PointList's base Frame *is* the old
current Frame. The port takes the same two steps —
`get_unc_frm(FrameRef::Current)` then `set_uncertainty`
(`regions/pointlist.rs:947 (fn simplified)`,874) — where `set_uncertainty` is the port of that
private `SetUnc`. The `reg_overlay` of row 68 then clears the result's uncertainty
again when the supplied Region had none of its own, which is why row 64 needs no
`astTestUnc` guard.

## Rows 68 and 69 — the tail is spelled twice

C reaches one tail from both branches, because its UnitMap test only decides
whether `new` was rebuilt. The port returns from inside the UnitMap branch
(`regions/pointlist.rs:928-936 (fn simplified)`) and again at the end
(`regions/pointlist.rs:955-958 (fn simplified)`), so the `reg_overlay`-and-return pair appears
twice. The conditions agree: the second copy is reached only through the
non-UnitMap branch, where C has already set `simpler = 1` unconditionally, so its
`true` is not an assumption.

## Rows 33 to 54 — why the tail is shared, and what it cost to route around it

Everything from the constructor onwards is what `MergeBox` and `MergeInterval` do
as well, in the same order, so it lives once in
`merge_util.rs:139-190 (fn finish_region_merge)`. Row 40 is the one that matters. C installs
the combined uncertainty with `astSetUnc`, and `astSetUnc` re-centres the
uncertainty on the merged Region's first defining point
(`region.c:10898-10903 (fn SetUnc)`); for a Box uncertainty, `Box`'s `RegCentre` rebuilds
the corner as a geodesic offset in the combined base Frame
(`box.c:2894 (fn RegCentre)`). A merge that assigns the uncertainty into place instead
performs rows 36 to 39, 44 and 54 correctly and still produces a different
Region, because the corner it carries is the one it was handed rather than the one
C regenerates.

Row 35 is the one place the three merges differ: `MergePointList` propagates the
Negated value its two PointLists share, where `MergeBox` and `MergeInterval` clear
it outright (`box.c:1960 (fn MergeBox)`, `interval.c:1558 (fn MergeInterval)`).
`MergeOverlay`'s `negated` field (`merge_util.rs:155 (fn finish_region_merge)`) is what
carries that difference through the shared tail.

## Rows 47 and 50 — the defaults are computed, not cleared and restored

C has no way to ask a Region for its *default* MeshSize while a value is set, so
it clears both attributes, reads the defaults back, then re-instates the original
values — a round trip that mutates the two supplied Regions and undoes it.
`apply_merge_scaling_to` asks `default_mesh_size` for the same number from the axis
count (`merge_util.rs:74-75 (fn apply_merge_scaling_to)`), which is the function C's
`astGetMeshSize` default branch computes, so nothing is mutated and row 50 has
nothing to restore. The scaling arithmetic and the result's own default
(`merge_util.rs:78-79 (fn apply_merge_scaling_to)`) are C's, unchanged.

## `cap_pointlist_01`

The fixture nests one-point PointLists, each carrying a Box uncertainty, so
`MergePointList` runs on a pair of them. What remains of its divergence is two
values: `X5` and `X6` of the merged uncertainty Box's corner, the sky half of the
geodesic round trip row 40 performs. Both are licensed in
`NUMERIC_PLATFORM_DRIFT` as platform drift, each entry naming the two values and
bounding their difference; every other card of the fixture is byte-equal.
`PLAN.md` §10.26 records the diagnosis. Cause F in `PLAN.md` §9 is closed.

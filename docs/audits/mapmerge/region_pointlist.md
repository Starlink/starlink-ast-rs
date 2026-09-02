# `Simplify` / `MapMerge` audit — PointList

C source: reference/starlink-ast/src/pointlist.c
Rust: crates/ast-core/src/regions/pointlist.rs
Audited: 2026-09-02 against pin 3e60abaa

Three C bodies. `MapMerge` (`pointlist.c:972-1202`) is the Region shape.
`MergePointList` (`pointlist.c:1205-1471`) is the merge it delegates a
neighbouring Region to: two single-point PointLists become one PointList over the
combined axes.
`Simplify` (`pointlist.c:2223-2288`) calls the parent, then — unless the
base-to-current Mapping is already a UnitMap — transforms the defining points into
the current Frame and rebuilds the PointList there, so the result needs only a
UnitMap.

`MergePointList` shares its whole tail with `MergeBox` and `MergeInterval`, so
rows 16-18, 20-22, 25 and 26 cite `merge_util.rs`, where that tail lives once.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | `MapMerge`: simplify the PointList, assuming a changed pointer means something happened | pointlist.c:972-1100 | simplify.rs:1292-1301 | ported |
| 2 | Replace it in the list with the new Region, marking it forward | pointlist.c:1101-1130 | simplify.rs:1298-1299 | ported |
| 3 | Otherwise merge with a neighbouring Region, in parallel only | pointlist.c:1131-1160 | simplify.rs:1303-1305 | ported |
| 4 | Try the lower neighbour, then the upper | pointlist.c:1161-1180 | simplify.rs:1306-1330 | ported |
| 5 | On success replace the pair, close the gap, and return the first index | pointlist.c:1181-1199 | simplify.rs:1331-1345 | ported |
| 6 | Return the result | pointlist.c:1200-1202 | simplify.rs:1349 | ported |
| 7 | `MergePointList`: initialise the result and check the global error status | pointlist.c:1281-1286 | -- | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 8 | Require both Regions to agree on Closed and to be both with or both without an uncertainty | pointlist.c:1289-1290 | pointlist.rs:599-606 | ported |
| 9 | Get the Negated flags of the two Regions | pointlist.c:1293-1294 | pointlist.rs:601 | ported |
| 10 | Merge only with another PointList, of equal Negated, each holding exactly one point | pointlist.c:1300-1302 | pointlist.rs:598-603 | ported |
| 11 | Get the two axis counts and their sum | pointlist.c:1305-1309 | pointlist.rs:608-609 | ported |
| 12 | Combine the two *base* Frames into the CmpFrame the merged Region is built on, in merge order | pointlist.c:1313-1320 | pointlist.rs:625-631 | ported |
| 13 | Get the coordinate data of the two PointLists | pointlist.c:1324-1329 | pointlist.rs:618-622 | ported |
| 14 | Create the one-point PointSet the merged PointList is defined by | pointlist.c:1332-1336 | pointlist.rs:616 | n/a: the merged point is a `Vec<f64>` the constructor takes directly |
| 15 | Copy the two points' axis values into it in merge order | pointlist.c:1338-1358 | pointlist.rs:616-623 | ported |
| 16 | Create the new PointList on the combined base Frame | pointlist.c:1360-1362 | merge_util.rs:162-166 | ported |
| 17 | `astRegOverlay( new, this, 1 )` — carry the supplied PointList's attributes onto it | pointlist.c:1364-1365 | merge_util.rs:140-147 | ported |
| 18 | Set the Negated flag to the value both supplied Regions share | pointlist.c:1367-1372 | merge_util.rs:146 | ported |
| 19 | Only when both Regions have an uncertainty, get both in their base Frames | pointlist.c:1375-1380 | pointlist.rs:675-679 | ported |
| 20 | Combine the two into a Prism in merge order | pointlist.c:1382-1387 | merge_util.rs:293-295 | ported |
| 21 | Simplify the Prism | pointlist.c:1389-1390 | merge_util.rs:296 | ported |
| 22 | `astSetUnc( new, sbunc )` — install it, which re-centres it on the merged point | pointlist.c:1392-1393 | merge_util.rs:169-171 | ported |
| 23 | Combine the two *current* Frames into a CmpFrame, in the same order | pointlist.c:1398-1410 | pointlist.rs:635-641 | ported |
| 24 | Combine the two base-to-current Mappings into a parallel CmpMap | pointlist.c:1412-1424 | pointlist.rs:644-671 | ported |
| 25 | `astMapRegion` the merged Region into the combined current Frame | pointlist.c:1427 | merge_util.rs:173 | ported |
| 26 | Set the result's FillFactor to the product of the two, and its MeshSize to the result default scaled by both used/default ratios, when either was set | pointlist.c:1429-1464 | merge_util.rs:65-76 | ported |
| 27 | `Simplify`: obtain a pointer to the Region structure | pointlist.c:2223-2225 | -- | n/a: the Rust method already has `&self` |
| 28 | Invoke the parent Region simplify | pointlist.c:2226-2229 | pointlist.rs:828 | ported |
| 29 | Note whether that simplified anything | pointlist.c:2230-2234 | pointlist.rs:842 | ported |
| 30 | Get the base-to-current Mapping; the PointList can be modified so a UnitMap suffices | pointlist.c:2235-2241 | pointlist.rs:844-856 | ported |
| 31 | Get the current Region Frame | pointlist.c:2242-2244 | pointlist.rs:860 | ported |
| 32 | Get the PointSet of base-Frame positions defining the PointList | pointlist.c:2245-2248 | pointlist.rs:858 | ported |
| 33 | Transform it through the Mapping | pointlist.c:2249-2251 | pointlist.rs:859 | ported |
| 34 | Get the positional uncertainty in the current Frame | pointlist.c:2252-2255 | pointlist.rs:867 | ported |
| 35 | Create a new PointList and use it in place of the original | pointlist.c:2256-2261 | pointlist.rs:862-868 | ported |
| 36 | Free resources | pointlist.c:2262-2270 | -- | n/a: owned values are dropped |
| 37 | If anything simplified, overlay the original Region's attributes and return it | pointlist.c:2271-2283 | pointlist.rs:869-872 | ported |
| 38 | Annul the returned pointer if an error occurred | pointlist.c:2284-2286 | -- | n/a: an error is a `Result::Err` |
| 39 | Return the result | pointlist.c:2287-2288 | pointlist.rs:872 | ported |

## Row 30 — why the transform is always worth doing

C's comment gives the reason: rebuilding the PointList in the current Frame means
the simplified Region needs only a UnitMap, which lets the rest of the simplifier
treat it as already reduced. The port keeps the same short-circuit when the
Mapping is already a UnitMap (`pointlist.rs:848`), so the rebuild happens exactly
when it buys something.

## Rows 16-26 — why the tail is shared, and what it cost to route around it

Everything from the constructor onwards is what `MergeBox` and `MergeInterval` do
as well, in the same order, so it lives once in
`merge_util.rs::finish_region_merge`. Row 22 is the reason that matters. C installs
the combined uncertainty with `astSetUnc`, and `astSetUnc` re-centres the
uncertainty on the merged Region's first defining point
(`region.c:10871-10876`); for a Box uncertainty, `Box`'s `RegCentre` rebuilds the
corner as `GeoCorner( frm, ncb, bc, this->geolen )`, a geodesic offset in the
combined base Frame. A merge that assigns the uncertainty into place instead
performs rows 16, 17, 18, 25 and 26 correctly and still produces a different
Region, because the corner it carries is the one it was handed rather than the one
C regenerates.

Row 18 is the one place the three merges differ: `MergePointList` propagates the
Negated value its two PointLists share, where `MergeBox` and `MergeInterval` clear
it outright (`box.c:1990`, `interval.c:1544`). `MergeOverlay::negated` is what
carries that difference through the shared tail.

## `cap_pointlist_01`

The fixture nests one-point PointLists, each carrying a Box uncertainty, so
`MergePointList` runs on a pair of them. What remains of its divergence is two
values: `X5` and `X6` of the merged uncertainty Box's corner, the sky half of the
geodesic round trip row 22 performs. Both are licensed in
`NUMERIC_PLATFORM_DRIFT` as platform drift, each entry naming the two values and
bounding their difference; every other card of the fixture is byte-equal.
`PLAN.md` §10.26 records the diagnosis. Cause F in `PLAN.md` §9 is closed.

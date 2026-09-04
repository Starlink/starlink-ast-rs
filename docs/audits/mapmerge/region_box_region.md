# `MapMerge` / `Simplify` audit — Box

C source: reference/starlink-ast/src/box.c
Rust: crates/ast-core/src/regions/box_region.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin 756d5ac0
Extension points: classified

Uncited C steps:
- box.c:1558 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- box.c:1698 (fn MergeBox) — `Local Variables:` is the local-variable declaration block, not a step
- box.c:3650 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Three C bodies. `MapMerge` (`box.c:1566-1655 (fn MapMerge)`) first tries to replace the
Box with a simpler Region on its own, then merges it with a neighbouring Region
in parallel. `MergeBox` (`box.c:1739-2104 (fn MergeBox)`) is the cross-type merge that
second half calls: Box + Box, Box + Interval, Box + NullRegion and
Box + PointList, each built on a CmpFrame of the two base Frames and mapped into
the combined current Frame. `Simplify` (`box.c:3694-4074 (fn Simplify)`) calls the parent
Region simplify, then works through four ways of re-expressing the Box: an
explicit PermMap axis swap, a best-fitting Box in the current Frame, a Polygon
for the 2-D case, and a split into a Prism.

`BoxRegion::map_merge` (`box_region.rs:1844 (fn map_merge)`) delegates to
`simplify::region_map_merge` (`mappings/simplify.rs:1445-1489 (fn region_map_merge)`);
`BoxRegion::region_merge` is `box_region.rs:1375-1544 (fn region_merge)` and
`BoxRegion::simplified` is `box_region.rs:1664-1809 (fn simplified)`. The four
merge helpers shared with `Interval`, `NullRegion` and `PointList` live in
`merge_util.rs:64-77 (fn apply_merge_scaling_to)`,
`merge_util.rs:131-180 (fn finish_region_merge)`,
`merge_util.rs:227-263 (fn region_is_point)` and
`merge_util.rs:285-301 (fn combine_uncertainties)`.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `MapMerge`: initialise the result and the merged-pair index | box.c:1566-1569 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | box.c:1570-1572 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Box | box.c:1573-1575 (fn MapMerge) | -- | ceremony | n/a: the Rust function already has `this: &dyn Region` |
| 4 | Banner: first see whether the Box reduces on its own, without reference to its neighbours | box.c:1576-1579 (fn MapMerge) | -- | ceremony | n/a: a section banner, not a step |
| 5 | Simplify the Box, assuming a changed pointer means something happened | box.c:1580-1583 (fn MapMerge) | mappings/simplify.rs:1455-1456 (fn region_map_merge) | architecture | ported |
| 6 | Annul the Box pointer in the list, replace it with the new Region, and mark it forward | box.c:1584-1590 (fn MapMerge) | mappings/simplify.rs:1457 (fn region_map_merge) | architecture | ported |
| 7 | Return the index of the first modified element | box.c:1591-1594 (fn MapMerge) | mappings/simplify.rs:1458 (fn region_map_merge) | architecture | ported |
| 8 | Failing that, merge with the Regions on either side — which is possible in parallel only | box.c:1595-1599 (fn MapMerge) | mappings/simplify.rs:1463 (fn region_map_merge) | architecture | ported |
| 9 | Annul the unwanted simplified pointer | box.c:1600-1601 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 10 | Attempt to merge the Box with its lower neighbour, if that neighbour is a Region | box.c:1602-1610 (fn MapMerge) | mappings/simplify.rs:1465-1473 (fn region_map_merge) | architecture | ported |
| 11 | Failing that, attempt the upper neighbour the same way | box.c:1611-1621 (fn MapMerge) | mappings/simplify.rs:1477-1486 (fn region_map_merge) | architecture | ported |
| 12 | Act only if one of the two produced a merged Region | box.c:1622-1624 (fn MapMerge) | mappings/simplify.rs:1470 (fn region_map_merge),1482 | architecture | ported |
| 13 | Annul the first of the two Mappings, replace it with the merged Region, and clear its invert flag | box.c:1625-1630 (fn MapMerge) | mappings/simplify.rs:589 (fn replace_pair_with) | architecture | ported |
| 14 | Annul the second and shuffle the rest of the list down to fill the gap | box.c:1631-1638 (fn MapMerge) | mappings/simplify.rs:590-591 (fn replace_pair_with) | architecture | ported |
| 15 | Clear the vacated element at the end | box.c:1639-1642 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entry leaves no slot to blank |
| 16 | Decrement the Mapping count and return the index of the first modified element | box.c:1643-1648 (fn MapMerge) | mappings/simplify.rs:1472 (fn region_map_merge),1484 | architecture | ported |
| 17 | In series, annul the simplified pointer and do nothing | box.c:1649-1652 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and the series case falls through to `None` |
| 18 | Return the result | box.c:1653-1655 (fn MapMerge) | mappings/simplify.rs:1489 (fn region_map_merge) | architecture | ported |
| 19 | `MergeBox`: initialise the returned pointer and the working arrays | box.c:1739-1741 (fn MergeBox) | -- | ceremony | n/a: the Rust returns `Option<DynMapping>`, so "no merge" is `None` |
| 20 | Check the local error status and bail out | box.c:1742-1744 (fn MergeBox) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 21 | Both Regions must agree on Closed, and either both or neither must have a defined uncertainty | box.c:1745-1751 (fn MergeBox) | box_region.rs:1380-1384 (fn region_merge) | architecture | ported |
| 22 | Get the Negated attributes of the two Regions | box.c:1752-1755 (fn MergeBox) | box_region.rs:1386 (fn region_merge) | architecture | ported |
| 23 | Get the number of axes in the two supplied Regions | box.c:1756-1759 (fn MergeBox) | box_region.rs:1387-1388 (fn region_merge) | architecture | ported |
| 24 | Get the number of axes the combination will have | box.c:1760-1763 (fn MergeBox) | box_region.rs:1393 (fn region_merge) | architecture | ported |
| 25 | Get the base Frames from the two Region FrameSets | box.c:1764-1768 (fn MergeBox) | box_region.rs:1463-1464 (fn region_merge) | architecture | ported |
| 26 | Combine them into a single CmpFrame, in the requested order, for use in creating any new Region | box.c:1769-1774 (fn MergeBox) | box_region.rs:1465-1469 (fn region_merge) | architecture | ported |
| 27 | Annul the two base Frame pointers | box.c:1775-1777 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 28 | Indicate we do not yet have a merged Region | box.c:1778-1780 (fn MergeBox) | -- | ceremony | n/a: the Rust builds an `Option<MergedRegion>`, so "no merge yet" is the `None` any arm can return |
| 29 | First attempt to merge with another Box, giving a Box, which needs both Boxes un-negated | box.c:1781-1784 (fn MergeBox) | box_region.rs:1404-1406 (fn region_merge) | architecture | ported |
| 30 | Allocate memory to store the centre and corner of the returned Box | box.c:1785-1788 (fn MergeBox) | -- | ceremony | n/a: a `Vec` grows as the values are appended |
| 31 | Copy the centres and corners from the two supplied Boxes into those arrays, in the requested order | box.c:1789-1798 (fn MergeBox) | box_region.rs:1409-1410 (fn region_merge),1391-1402 | architecture | ported |
| 32 | Create the new Box, initially with no uncertainty | box.c:1799-1802 (fn MergeBox) | merge_util.rs:157-161 (fn finish_region_merge),149 | architecture | ported |
| 33 | Free the centre and corner arrays | box.c:1803-1806 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 34 | Now attempt to merge with an Interval, giving an Interval, which needs both Regions un-negated | box.c:1807-1810 (fn MergeBox) | box_region.rs:1413-1415 (fn region_merge) | architecture | ported |
| 35 | Allocate memory to store the bounds of the returned Interval | box.c:1811-1814 (fn MergeBox) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 36 | Copy the centre and corner from the supplied Box into its part of those arrays | box.c:1815-1825 (fn MergeBox) | box_region.rs:1418 (fn region_merge) | architecture | ported |
| 37 | Convert those centre and corner positions into upper and lower bounds, swapping any pair the conversion inverts | box.c:1826-1837 (fn MergeBox) | box_region.rs:541-557 (fn bounds) | architecture | ported |
| 38 | Get the bounds from the Interval and add them into those arrays | box.c:1838-1844 (fn MergeBox) | box_region.rs:1419-1420 (fn region_merge) | architecture | ported |
| 39 | Create the new Interval, initially with no uncertainty | box.c:1845-1848 (fn MergeBox) | merge_util.rs:152-156 (fn finish_region_merge),149 | architecture | ported |
| 40 | Free the bounds arrays | box.c:1849-1852 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 41 | Now attempt to merge with a NullRegion, giving an Interval; the NullRegion must be negated and the Box must not | box.c:1853-1856 (fn MergeBox) | box_region.rs:1423-1426 (fn region_merge) | architecture | ported |
| 42 | Allocate memory to store the bounds of the returned Interval | box.c:1857-1860 (fn MergeBox) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 43 | Copy the centre and corner from the supplied Box into its part of those arrays | box.c:1861-1871 (fn MergeBox) | box_region.rs:1429 (fn region_merge) | architecture | ported |
| 44 | Convert those centre and corner positions into upper and lower bounds | box.c:1872-1882 (fn MergeBox) | box_region.rs:541-557 (fn bounds) | architecture | ported |
| 45 | Fill the NullRegion's axes with bad values to mark them unbounded | box.c:1883-1895 (fn MergeBox) | box_region.rs:1430-1432 (fn region_merge) | architecture | ported |
| 46 | Create the new Interval, initially with no uncertainty | box.c:1896-1900 (fn MergeBox) | merge_util.rs:152-156 (fn finish_region_merge),149 | architecture | ported |
| 47 | Free the bounds arrays | box.c:1901-1904 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 48 | Now attempt to merge with a PointList, giving a PointList, which needs both Regions un-negated | box.c:1905-1908 (fn MergeBox) | box_region.rs:1435-1439 (fn region_merge) | architecture | ported |
| 49 | The Box must have zero width on every axis; get its centre and corner | box.c:1909-1914 (fn MergeBox) | box_region.rs:1442 (fn region_merge) | architecture | ported |
| 50 | Get the size of the Box's base-Frame uncertainty region | box.c:1915-1920 (fn MergeBox) | merge_util.rs:235-255 (fn region_is_point) | architecture | ported |
| 51 | Refuse the merge unless every axis width is below a quarter of the uncertainty on that axis | box.c:1921-1933 (fn MergeBox) | merge_util.rs:257-261 (fn region_is_point) | architecture | ported |
| 52 | Go on to create a new PointList only if the Box is a point | box.c:1934-1936 (fn MergeBox) | box_region.rs:1443-1445 (fn region_merge) | architecture | ported |
| 53 | Get a PointSet holding the supplied PointList's axis values, its point count and pointers to its arrays | box.c:1937-1943 (fn MergeBox) | box_region.rs:1446-1449 (fn region_merge) | architecture | ported |
| 54 | Create a new PointSet with room for the same points but the extra axes, and get pointers to its arrays | box.c:1944-1948 (fn MergeBox) | -- | ceremony | n/a: the merged points are a `Vec<Vec<f64>>` built one point at a time, so there is no PointSet to size in advance |
| 55 | Copy the PointList axis values into the new PointSet, in the requested order | box.c:1949-1958 (fn MergeBox) | box_region.rs:1450 (fn region_merge),1391-1402 | architecture | ported |
| 56 | Add the axis values the Box centre defines to every point | box.c:1959-1964 (fn MergeBox) | box_region.rs:1389 (fn region_merge),1450 | architecture | ported |
| 57 | Create the new PointList, initially with no uncertainty | box.c:1965-1969 (fn MergeBox) | merge_util.rs:162-166 (fn finish_region_merge),149 | architecture | ported |
| 58 | Free the two PointSets, the centre, corner and bounds arrays and the uncertainty | box.c:1970-1982 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 59 | If a new Region was created above, propagate the supplied Region's remaining attributes to it | box.c:1983-1987 (fn MergeBox) | merge_util.rs:140-147 (fn finish_region_merge) | architecture | ported |
| 60 | Ensure the Negated attribute the constructors set is not the one the overlay left | box.c:1988-1992 (fn MergeBox) | merge_util.rs:146 (fn finish_region_merge) | architecture | ported |
| 61 | Assign the new Region an uncertainty only when both supplied Regions have one | box.c:1993-1996 (fn MergeBox) | merge_util.rs:290 (fn combine_uncertainties) | architecture | ported |
| 62 | Get the base-Frame uncertainties from the two supplied Regions | box.c:1997-2000 (fn MergeBox) | box_region.rs:1511-1515 (fn region_merge) | architecture | ported |
| 63 | Combine them into a single Prism, in the correct order | box.c:2001-2007 (fn MergeBox) | merge_util.rs:291 (fn combine_uncertainties),293-295 | architecture | ported |
| 64 | Attempt to simplify the Prism | box.c:2008-2010 (fn MergeBox) | merge_util.rs:296 (fn combine_uncertainties) | architecture | ported |
| 65 | Use the simplified Prism as the uncertainty for the returned Region | box.c:2011-2013 (fn MergeBox) | merge_util.rs:169-171 (fn finish_region_merge) | architecture | ported |
| 66 | Free the uncertainty resources | box.c:2014-2020 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 67 | Get the current Frames from the two Region FrameSets and combine them into one CmpFrame | box.c:2021-2031 (fn MergeBox) | box_region.rs:1471-1478 (fn region_merge) | architecture | ported |
| 68 | Get the two base-to-current Mappings and combine them into one parallel CmpMap connecting the two combined Frames | box.c:2032-2045 (fn MergeBox) | box_region.rs:1480-1508 (fn region_merge) | architecture | ported |
| 69 | Map the new Region into the new current Frame | box.c:2046-2048 (fn MergeBox) | merge_util.rs:173 (fn finish_region_merge) | architecture | ported |
| 70 | The result's FillFactor is the product of the two supplied FillFactors | box.c:2049-2055 (fn MergeBox) | merge_util.rs:65-67 (fn apply_merge_scaling_to) | architecture | ported |
| 71 | Scale the result's MeshSize only when either supplied MeshSize was set | box.c:2056-2063 (fn MergeBox) | merge_util.rs:69 (fn apply_merge_scaling_to) | architecture | ported |
| 72 | Get the two MeshSize values, then clear them so the defaults are returned | box.c:2064-2071 (fn MergeBox) | merge_util.rs:70-71 (fn apply_merge_scaling_to) | architecture | ported |
| 73 | Form each Region's ratio of used to default MeshSize | box.c:2072-2076 (fn MergeBox) | merge_util.rs:72-73 (fn apply_merge_scaling_to) | architecture | ported |
| 74 | Set the result's MeshSize to its own default scaled by the product of the two ratios | box.c:2077-2080 (fn MergeBox) | merge_util.rs:74-75 (fn apply_merge_scaling_to) | architecture | ported |
| 75 | Re-instate the original MeshSize values on the supplied Regions | box.c:2081-2086 (fn MergeBox) | -- | ceremony | n/a: nothing was cleared on the supplied Regions, so there is nothing to restore |
| 76 | Free the Frame, Mapping and intermediate-Region resources | box.c:2087-2098 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 77 | Annul the returned pointer if an error occurred | box.c:2099-2101 (fn MergeBox) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 78 | Return the result | box.c:2102-2104 (fn MergeBox) | merge_util.rs:179 (fn finish_region_merge) | architecture | ported |
| 79 | `Simplify`: initialise the returned pointer and the working pointers | box.c:3694-3696 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so "no change" is the `false` flag |
| 80 | Check the global error status and bail out | box.c:3697-3699 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 81 | Obtain a pointer to the Region structure | box.c:3700-3702 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 82 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and uncertainty | box.c:3703-3706 (fn Simplify) | box_region.rs:1665 (fn simplified) | architecture | ported |
| 83 | Note whether that simplified anything, by pointer inequality | box.c:3707-3711 (fn Simplify) | box_region.rs:1666 (fn simplified) | architecture | ported |
| 84 | Get the Mapping from base to current Frame | box.c:3712-3714 (fn Simplify) | box_region.rs:1681-1685 (fn simplified) | architecture | ported |
| 85 | Get that Mapping's input and output counts | box.c:3715-3718 (fn Simplify) | box_region.rs:1773 (fn simplified) | architecture | ported |
| 86 | If the Mapping is a PermMap, swap the Box axes explicitly rather than fitting through a mesh | box.c:3719-3723 (fn Simplify) | box_region.rs:1693 (fn simplified) | architecture | ported |
| 87 | See whether the new Box is Negated and Closed | box.c:3724-3727 (fn Simplify) | box_region.rs:436-437 (fn simplify_over_permmap) | architecture | ported |
| 88 | Get a pointer to the Box structure | box.c:3728-3730 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 89 | Ensure the cached centre and extent are up to date | box.c:3731-3733 (fn Simplify) | box_region.rs:438 (fn simplify_over_permmap) | ceremony | ported |
| 90 | Get the input and output permutation arrays and the constants from the PermMap | box.c:3734-3739 (fn Simplify) | box_region.rs:442-444 (fn simplify_over_permmap) | architecture | ported |
| 91 | Allocate memory to hold the current-Frame axis bounds, unset | box.c:3740-3744 (fn Simplify) | box_region.rs:446-447 (fn simplify_over_permmap) | architecture | ported |
| 92 | Check the pointers can be used safely | box.c:3745-3747 (fn Simplify) | -- | ceremony | n/a: `Vec` allocation either succeeds or aborts, so there is no pointer to test |
| 93 | Set the flags saying the Box simplifies and gives neither a NullRegion nor an Interval | box.c:3748-3753 (fn Simplify) | box_region.rs:448-449 (fn simplify_over_permmap),1702 | architecture | ported |
| 94 | Check each output, which is an axis of the current Frame | box.c:3754-3757 (fn Simplify) | box_region.rs:456 (fn simplify_over_permmap) | architecture | ported |
| 95 | Find the input feeding that output under the forward transformation | box.c:3758-3761 (fn Simplify) | box_region.rs:457 (fn simplify_over_permmap) | architecture | ported |
| 96 | An output fed a constant gets both limits equal to that constant | box.c:3762-3767 (fn Simplify) | box_region.rs:458-463 (fn simplify_over_permmap) | architecture | ported |
| 97 | An output fed `AST__BAD` is unbounded, so the result becomes an Interval rather than a Box | box.c:3768-3774 (fn Simplify) | box_region.rs:464-467 (fn simplify_over_permmap) | architecture | ported |
| 98 | An output fed the value of an input takes that input's limits | box.c:3775-3781 (fn Simplify) | box_region.rs:468-473 (fn simplify_over_permmap) | architecture | ported |
| 99 | A missing bound on either side makes the result an Interval | box.c:3782-3788 (fn Simplify) | box_region.rs:475-478 (fn simplify_over_permmap) | architecture | ported |
| 100 | Any base-Frame axis absent from the current Frame means the PermMap selects a slice, which need not meet the Box; check each entry of the input permutation | box.c:3789-3795 (fn Simplify) | box_region.rs:481-483 (fn simplify_over_permmap) | architecture | ported |
| 101 | Find the output feeding that input under the inverse transformation | box.c:3796-3799 (fn Simplify) | box_region.rs:484 (fn simplify_over_permmap) | architecture | ported |
| 102 | An input fed a constant gives a NullRegion unless the constant lies within the Box's limits on that axis | box.c:3800-3816 (fn Simplify) | box_region.rs:485-498 (fn simplify_over_permmap) | architecture | ported |
| 103 | An input fed `AST__BAD` extends beyond the Box, so the result is a NullRegion | box.c:3817-3823 (fn Simplify) | box_region.rs:499-502 (fn simplify_over_permmap) | architecture | ported |
| 104 | An input fed an output requires the relationship to be bi-directional, or the Box cannot be simplified | box.c:3824-3832 (fn Simplify) | box_region.rs:503-506 (fn simplify_over_permmap) | architecture | ported |
| 105 | If the Box can be simplified, create a new Region of an appropriate class | box.c:3833-3836 (fn Simplify) | box_region.rs:519 (fn simplify_over_permmap) | architecture | ported |
| 106 | Get any uncertainty from the Box, already simplified by the parent Simplify | box.c:3837-3840 (fn Simplify) | box_region.rs:512-516 (fn simplify_over_permmap) | architecture | ported |
| 107 | Get the Frame the Region is defined in | box.c:3841-3843 (fn Simplify) | box_region.rs:517 (fn simplify_over_permmap) | architecture | ported |
| 108 | Annul the original pointer, since the simplified Region replaces it | box.c:3844-3847 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 109 | Create a NullRegion where the slice misses the Box | box.c:3848-3851 (fn Simplify) | box_region.rs:519-520 (fn simplify_over_permmap) | architecture | ported |
| 110 | Create an Interval where any axis is unbounded | box.c:3852-3854 (fn Simplify) | box_region.rs:521-522 (fn simplify_over_permmap) | architecture | ported |
| 111 | Otherwise create a Box from the permuted bounds | box.c:3855-3858 (fn Simplify) | box_region.rs:523-525 (fn simplify_over_permmap) | architecture | ported |
| 112 | Negate the new Region if the original Box was negated | box.c:3859-3861 (fn Simplify) | box_region.rs:529-531 (fn simplify_over_permmap) | architecture | ported |
| 113 | Free the uncertainty and Frame pointers | box.c:3862-3868 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 114 | Free the permutation arrays, the constants and the bounds arrays | box.c:3869-3875 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 115 | If the Mapping is neither a PermMap nor a UnitMap, try re-defining the Box within its current Frame, testing the result by transforming its boundary | box.c:3876-3882 (fn Simplify) | box_region.rs:1706 (fn simplified) | architecture | ported |
| 116 | Get a pointer to the current Frame | box.c:3883-3885 (fn Simplify) | box_region.rs:1751 (fn simplified) | architecture | ported |
| 117 | Get a mesh of points covering the Box in its current Frame | box.c:3886-3888 (fn Simplify) | box_region.rs:1747-1749 (fn simplified) | architecture | ported |
| 118 | Get the Region describing the positional uncertainty in the current Frame | box.c:3889-3892 (fn Simplify) | box_region.rs:1750 (fn simplified) | architecture | ported |
| 119 | Find the best-fitting current-Frame Box through those points | box.c:3893-3896 (fn Simplify) | box_region.rs:1753 (fn simplified),627 | architecture | ported |
| 120 | See whether every mesh point falls on that Box's boundary, to within the uncertainty | box.c:3897-3900 (fn Simplify) | box_region.rs:1754 (fn simplified) | architecture | ported |
| 121 | If so, check the converse, transforming the fitted Box's own mesh into the original base Frame for `astRegPins` | box.c:3901-3908 (fn Simplify) | box_region.rs:1760-1763 (fn simplified) | architecture | ported |
| 122 | If so, use the new Box in place of the original | box.c:3909-3914 (fn Simplify) | box_region.rs:1764-1766 (fn simplified) | architecture | ported |
| 123 | Free the fitted Box's mesh | box.c:3915-3917 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 124 | Otherwise, for a 2-D Box, see whether a Polygon represents it accurately | box.c:3918-3922 (fn Simplify) | box_region.rs:1773 (fn simplified) | architecture | ported |
| 125 | Create a PointSet holding the base-Frame axis values at the four corners of the Box | box.c:3923-3927 (fn Simplify) | -- | ceremony | n/a: the four corners are a fixed-size array rebuilt per winding at box_region.rs:366-380 (fn convert_to_polygon), so there is no PointSet to allocate up front |
| 126 | Get the Box structure and refresh its cached centre and extent | box.c:3928-3930 (fn Simplify) | box_region.rs:332-335 (fn convert_to_polygon) | architecture | ported |
| 127 | For a SkyFrame, transform the Box centre into the current Frame to test the winding against | box.c:3931-3939 (fn Simplify) | box_region.rs:329 (fn convert_to_polygon),347-353 | architecture | ported |
| 128 | The vertex order decides whether the polygon's inside is its interior; try clockwise first and anti-clockwise second | box.c:3940-3947 (fn Simplify) | box_region.rs:364 (fn convert_to_polygon) | architecture | ported |
| 129 | Store the four base-Frame corners in clockwise order | box.c:3948-3960 (fn Simplify) | box_region.rs:366-372 (fn convert_to_polygon) | architecture | ported |
| 130 | Store them in anti-clockwise order instead | box.c:3961-3974 (fn Simplify) | box_region.rs:373-380 (fn convert_to_polygon) | architecture | ported |
| 131 | Transform the Box corners into the current Frame | box.c:3975-3979 (fn Simplify) | box_region.rs:383-385 (fn convert_to_polygon) | architecture | ported |
| 132 | Create a Polygon from those points | box.c:3980-3986 (fn Simplify) | box_region.rs:387-391 (fn convert_to_polygon) | architecture | ported |
| 133 | Decide whether the inside is the interior: for a SkyFrame by transforming the centre through the Polygon, otherwise by its bounded flag | box.c:3987-3995 (fn Simplify) | box_region.rs:396-400 (fn convert_to_polygon) | architecture | ported |
| 134 | Break out of the winding loop as soon as one is right way round | box.c:3996-4001 (fn Simplify) | box_region.rs:402-405 (fn convert_to_polygon) | architecture | ported |
| 135 | Free the rejected Polygon and its transformed corners | box.c:4002-4005 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 136 | Free the SkyFrame centre-probe PointSets | box.c:4006-4011 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 137 | See whether every point of the Box mesh falls on the Polygon's boundary, to within the uncertainty | box.c:4012-4015 (fn Simplify) | box_region.rs:415 (fn convert_to_polygon) | architecture | ported |
| 138 | If so, use the new Polygon in place of the original Box | box.c:4016-4021 (fn Simplify) | box_region.rs:416-419 (fn convert_to_polygon),1774-1777 | architecture | ported |
| 139 | Free the Polygon and the corner PointSet | box.c:4022-4028 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 140 | Failing everything so far, split the Box across two axis subsets and join the two simplified halves into a Prism | box.c:4029-4037 (fn Simplify) | box_region.rs:1784-1785 (fn simplified),222 | architecture | ported |
| 141 | Adopt the Prism if it is not a clone of the Box, and annul it otherwise | box.c:4038-4046 (fn Simplify) | box_region.rs:1785-1788 (fn simplified),311 | architecture | ported |
| 142 | Free the Frame, mesh, uncertainty, fitted Box and the base-to-current Mapping | box.c:4047-4055 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 143 | If anything simplified, overlay the supplied Region's attributes onto the result and return it | box.c:4056-4063 (fn Simplify) | box_region.rs:1798-1800 (fn simplified) | architecture | ported |
| 144 | Otherwise annul the working Region and return a clone of the supplied pointer | box.c:4064-4068 (fn Simplify) | box_region.rs:1802-1803 (fn simplified) | architecture | ported |
| 145 | Annul the returned pointer if an error occurred | box.c:4069-4071 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 146 | Return the result | box.c:4072-4074 (fn Simplify) | box_region.rs:1800 (fn simplified),1803 | architecture | ported |

## Row 1 — the merged-pair index starts at zero

`box.c::MapMerge` and `interval.c::MapMerge` are the same function at a constant
offset: `diff` over `box.c:1566-1655 (fn MapMerge)` and
`interval.c:1132-1221 (fn MapMerge)` shows one substantive difference, which is
that the Box initialises `i1 = 0` (`box.c:1568 (fn MapMerge)`) where the Interval
initialises `i1 = -1` (`interval.c:1134 (fn MapMerge)`). Neither value is ever read
before it is assigned by a successful merge attempt, so the difference is
invisible: both bodies test `i1 > -1` only after the lower-neighbour and
upper-neighbour attempts have run, and both attempts set `i1` when they succeed.
The port returns `Option<usize>` and so carries no sentinel at all.

## Row 8 — parallel only

C wraps the whole neighbour-merge half of `MapMerge` in a parallel test, because
two Regions in series are not a Region: the first Region's output feeds the
second's input, which is a constraint on a transformed position rather than a
region of a combined Frame. `region_map_merge`
(`mappings/simplify.rs:1463 (fn region_map_merge)`) makes the same test and returns
`None` for series, which sends the pair back to the generic path.

## Rows 49 to 51 — a zero-width Box, tested without aliasing

Merging a Box with a PointList requires the Box to be a point, which C tests by
comparing each axis width against the Box's own uncertainty. `box.c:1915-1920 (fn MergeBox)`
loads `lbnd` and `ubnd` with the bounds of the base-Frame uncertainty Region and
`box.c:1921-1933 (fn MergeBox)` refuses the merge unless every axis width is under a
quarter of the corresponding uncertainty width. Both arrays hold what their names
say, and `astGetUncFrm` supplies a default uncertainty when the Box has none, so
the test is meaningful for every Box.

This is the un-aliased counterpart of the same test in the Interval, where the
arrays have already been overwritten and the comparison reduces to subtracting a
value from itself (`interval.c:1470 (fn MergeInterval)` and
`interval.c:1479 (fn MergeInterval)`, rows 63 and 66 of `region_interval.md`). The
port implements C's Box form once, in `merge_util.rs:227-263 (fn region_is_point)`,
including the default-uncertainty fallback C gets from `astGetUncFrm`
(`merge_util.rs:243-255 (fn region_is_point)`), and both classes call it.

## Rows 71 to 75 — `MeshSize` is never cleared

C's merge reads each supplied Region's `MeshSize`, clears it so the default is
returned for the ratio, and re-instates the original afterwards
(`box.c:2056-2086 (fn MergeBox)`). The port computes the same ratios without
mutating the supplied Regions: `effective_mesh_size` reports the value in use and
`default_mesh_size(naxes)` supplies the denominator directly
(`merge_util.rs:69-75 (fn apply_merge_scaling_to)`), so there is nothing to restore.

## Rows 86 to 114 — the PermMap branch

A PermMap between base and current Frame permutes, drops and supplies axes, and
`simplify_over_permmap` (`box_region.rs:435-534 (fn simplify_over_permmap)`) reads the
PermMap's own arrays rather than fitting a Box through a transformed mesh. That
expresses three outcomes the generic best-fit path cannot reach: an axis fed a
constant gets equal limits, an axis fed `AST__BAD` or missing a bound makes the
result an Interval rather than a Box, and a slice that misses the Box makes it a
NullRegion. A permutation that is not bi-directional refuses the simplification
altogether, and because C clears `simpler` at `box.c:4064 (fn Simplify)` rather than
merely declining to improve on it, that refusal discards the parent Region
simplification with it. The port clears the same flag at
`box_region.rs:1704 (fn simplified)`.

Two representational differences. The port stores the public 1-based permutation
form, where `+j` names axis `j`, `0` names `AST__BAD` and `-j` names
`constants[j-1]`, while C stores the internal 0-based form and turns a public `0`
into `nin` or `nout`; C's `feed >= nin` test is therefore written
`feed == 0 || feed as usize > nin` here (`box_region.rs:464-467 (fn simplify_over_permmap)`).
And `isNull` is an assignment in C (`box.c:3812 (fn Simplify)`) rather than an
accumulation, so where several inputs are fed constants the last one decides
whether the result is a NullRegion; the port mirrors the assignment deliberately
(`box_region.rs:494-498 (fn simplify_over_permmap)`) and the behaviour is recorded in
`docs/issues/c-library-quirks.md`.

`cap_box_permmap_const`, `cap_box_permmap_null` and `cap_box_permmap_nonbidi` gate
the constant, NullRegion and non-bidirectional exits. The unbounded exit has no
oracle fixture, because `astMapRegion` refuses to build a Region whose forward
transformation gives a bad axis value (`region.c:5539 (fn MapRegion)`), so it is
covered by the in-crate Rust test
`box_region.rs::tests::permmap_bad_output_simplifies_to_unbounded_interval` — see
PLAN.md §10.13.

## Rows 115 to 123 — a corner transform before the best-fit Box

C has one way to re-express a Box in its current Frame: fit the best Box through
a mesh covering the original (`box.c:3893-3896 (fn Simplify)`), check that every mesh
point lies on the fitted Box's boundary, then check the converse by transforming
the fitted Box's own mesh back into the base Frame
(`box.c:3901-3908 (fn Simplify)`). The port tries a cheaper route first: transform the
Box's centre and corner directly and accept the result only if
`verify_remap_exact` confirms the round trip
(`box_region.rs:1709-1737 (fn simplified)`). That check is C's inverse-direction test
(`regions/data.rs:478-493 (fn verify_remap_exact)`), and C's forward `astRegPins` test
is not applied on the fast path. When it fails, the port falls back to C's
sequence in full: `best_box` (`box_region.rs:1753 (fn simplified)`), the forward
`reg_pins` (`box_region.rs:1754 (fn simplified)`) and the inverse mesh check
(`box_region.rs:1760-1763 (fn simplified)`).

## Rows 124 to 139 — the two windings are exhaustive

A Polygon's vertices decide which side of its boundary is its inside, so C builds
the four corners twice, clockwise and anti-clockwise, and keeps the winding whose
inside is the Box's interior (`box.c:3940-4001 (fn Simplify)`). For a SkyFrame it
decides by transforming the Box centre through the candidate Polygon; otherwise it
asks whether the Polygon is bounded, since the complement of a polygon is not.

Where neither winding satisfies that test, C annuls the candidate on each
iteration (`box.c:4002-4005 (fn Simplify)`) and reaches `astRegPins` with a null
Polygon pointer (`box.c:4012-4015 (fn Simplify)`), while
`convert_to_polygon` returns `Ok(None)` (`box_region.rs:408-410 (fn convert_to_polygon)`).
The two orders are exhaustive — exactly one of them is bounded, and for a SkyFrame
the transformed centre lies inside exactly one — so neither path is reachable and
the difference is in the unreachable branch only.

## Rows 140 and 141 — where the Prism attempt sits

C's `astConvertToPrism` attempt is guarded by `if( !simpler )`
(`box.c:4035 (fn Simplify)`) at the same nesting level as the best-fit and Polygon
attempts; the brace at `box.c:4052 (fn Simplify)` closes the enclosing "neither
PermMap nor UnitMap" branch. The port nests it one level deeper, inside the
else-arm of the corner-transform check (`box_region.rs:1784-1785 (fn simplified)`).
The two are equivalent because the fast path sets `simpler` when it succeeds, so
C's guard is false on exactly the paths where the port never reaches the arm.

## Row 143 — overlaying attributes, and not acquiring an uncertainty

`astRegOverlay` at `box.c:4060 (fn Simplify)` is called with `unc = 1`, which clears
the result's uncertainty where the supplied Region had none. The port passes the
same flag (`box_region.rs:1798-1800 (fn simplified)`), so a Box fitted through a mesh
derived from a *default* uncertainty does not come back carrying that uncertainty
as an explicit one. Without it, simplifying a Box would set an attribute the
original did not have, and the difference would show in the dump.

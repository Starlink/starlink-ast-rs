# `MapMerge` / `Simplify` audit — Box

C source: reference/starlink-ast/src/box.c
Rust: crates/starlink-ast/src/regions/box_region.rs
Audited: 2026-09-03 against pin 3e60abaa
Citations: verified against pin fe26cc9b
Extension points: classified

Uncited C steps:
- box.c:1537 (fn MapMerge) — `Local Variables:` is the local-variable declaration block, not a step
- box.c:1677 (fn MergeBox) — `Local Variables:` is the local-variable declaration block, not a step
- box.c:3629 (fn Simplify) — `Local Variables:` is the local-variable declaration block, not a step

Three C bodies. `MapMerge` (`box.c:1545-1634 (fn MapMerge)`) first tries to replace the
Box with a simpler Region on its own, then merges it with a neighboring Region
in parallel. `MergeBox` (`box.c:1718-2083 (fn MergeBox)`) is the cross-type merge that
second half calls: Box + Box, Box + Interval, Box + NullRegion and
Box + PointList, each built on a CmpFrame of the two base Frames and mapped into
the combined current Frame. `Simplify` (`box.c:3673-4059 (fn Simplify)`) calls the parent
Region simplify, then works through four ways of re-expressing the Box: an
explicit PermMap axis swap, a best-fitting Box in the current Frame, a Polygon
for the 2-D case, and a split into a Prism.

`BoxRegion::map_merge` (`box_region.rs:2081 (fn map_merge)`) delegates to
`simplify::region_map_merge` (`mappings/simplify.rs:1783-1847 (fn region_map_merge)`);
`BoxRegion::region_merge` is `box_region.rs:1590-1766 (fn region_merge)` and
`BoxRegion::simplified` is `box_region.rs:1948-2044 (fn simplified)`. The four
merge helpers shared with `Interval`, `NullRegion` and `PointList` live in
`merge_util.rs:68-85 (fn apply_merge_scaling_to)`,
`merge_util.rs:142-191 (fn finish_region_merge)`,
`merge_util.rs:238-274 (fn region_is_point)` and
`merge_util.rs:296-312 (fn combine_uncertainties)`.

| # | C step | C site | Rust site | extension point | verdict |
|---|---|---|---|---|---|
| 1 | `MapMerge`: initialize the result and the merged-pair index | box.c:1545-1548 (fn MapMerge) | -- | ceremony | n/a: the Rust signature returns `Option<usize>`, so "no change" is `None` rather than a sentinel index |
| 2 | Check the global error status and bail out | box.c:1549-1551 (fn MapMerge) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 3 | Get a pointer to the Box | box.c:1552-1554 (fn MapMerge) | -- | ceremony | n/a: the Rust function already has `this: &dyn Region` |
| 4 | Banner: first see whether the Box reduces on its own, without reference to its neighbors | box.c:1555-1558 (fn MapMerge) | -- | ceremony | n/a: a section banner, not a step |
| 5 | Simplify the Box, assuming a changed pointer means something happened | box.c:1559-1562 (fn MapMerge) | mappings/simplify.rs:1795-1796 (fn region_map_merge) | architecture | ported |
| 6 | Annul the Box pointer in the list, replace it with the new Region, and mark it forward | box.c:1563-1569 (fn MapMerge) | mappings/simplify.rs:1797 (fn region_map_merge) | architecture | ported |
| 7 | Return the index of the first modified element | box.c:1570-1573 (fn MapMerge) | mappings/simplify.rs:1798 (fn region_map_merge) | architecture | ported |
| 8 | Failing that, merge with the Regions on either side — which is possible in parallel only | box.c:1574-1578 (fn MapMerge) | mappings/simplify.rs:1808 (fn region_map_merge) | architecture | ported |
| 9 | Annul the unwanted simplified pointer | box.c:1579-1580 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped |
| 10 | Attempt to merge the Box with its lower neighbor, if that neighbor is a Region | box.c:1581-1589 (fn MapMerge) | mappings/simplify.rs:1811-1825 (fn region_map_merge) | architecture | ported |
| 11 | Failing that, attempt the upper neighbor the same way | box.c:1590-1600 (fn MapMerge) | mappings/simplify.rs:1829-1844 (fn region_map_merge) | architecture | ported |
| 12 | Act only if one of the two produced a merged Region | box.c:1601-1603 (fn MapMerge) | mappings/simplify.rs:1819 (fn region_map_merge),1822 (fn region_map_merge) | architecture | ported |
| 13 | Annul the first of the two Mappings, replace it with the merged Region, and clear its invert flag | box.c:1604-1609 (fn MapMerge) | mappings/simplify.rs:681 (fn replace_pair_with) | architecture | ported |
| 14 | Annul the second and shuffle the rest of the list down to fill the gap | box.c:1610-1617 (fn MapMerge) | mappings/simplify.rs:681 (fn replace_pair_with) | architecture | ported |
| 15 | Clear the vacated element at the end | box.c:1618-1621 (fn MapMerge) | -- | ceremony | n/a: the list is a `Vec`, so the removed entry leaves no slot to blank |
| 16 | Decrement the Mapping count and return the index of the first modified element | box.c:1622-1627 (fn MapMerge) | mappings/simplify.rs:1821 (fn region_map_merge),1824 (fn region_map_merge) | architecture | ported |
| 17 | In series, annul the simplified pointer and do nothing | box.c:1628-1631 (fn MapMerge) | -- | ceremony | n/a: owned values are dropped, and the series case falls through to `None` |
| 18 | Return the result | box.c:1632-1634 (fn MapMerge) | mappings/simplify.rs:1847 (fn region_map_merge) | architecture | ported |
| 19 | `MergeBox`: initialize the returned pointer and the working arrays | box.c:1718-1720 (fn MergeBox) | -- | ceremony | n/a: the Rust returns `Option<DynMapping>`, so "no merge" is `None` |
| 20 | Check the local error status and bail out | box.c:1721-1723 (fn MergeBox) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 21 | Both Regions must agree on Closed, and either both or neither must have a defined uncertainty | box.c:1724-1730 (fn MergeBox) | box_region.rs:1599-1603 (fn region_merge) | architecture | ported |
| 22 | Get the Negated attributes of the two Regions | box.c:1731-1734 (fn MergeBox) | box_region.rs:1605 (fn region_merge) | architecture | ported |
| 23 | Get the number of axes in the two supplied Regions | box.c:1735-1738 (fn MergeBox) | box_region.rs:1606-1607 (fn region_merge) | architecture | ported |
| 24 | Get the number of axes the combination will have | box.c:1739-1742 (fn MergeBox) | box_region.rs:1612 (fn region_merge) | architecture | ported |
| 25 | Get the base Frames from the two Region FrameSets | box.c:1743-1747 (fn MergeBox) | box_region.rs:1682-1685 (fn region_merge) | architecture | ported |
| 26 | Combine them into a single CmpFrame, in the requested order, for use in creating any new Region | box.c:1748-1753 (fn MergeBox) | box_region.rs:1686-1690 (fn region_merge) | architecture | ported |
| 27 | Annul the two base Frame pointers | box.c:1754-1756 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 28 | Indicate we do not yet have a merged Region | box.c:1757-1759 (fn MergeBox) | -- | ceremony | n/a: the Rust builds an `Option<MergedRegion>`, so "no merge yet" is the `None` any arm can return |
| 29 | First attempt to merge with another Box, giving a Box, which needs both Boxes un-negated | box.c:1760-1763 (fn MergeBox) | box_region.rs:1623-1625 (fn region_merge) | architecture | ported |
| 30 | Allocate memory to store the center and corner of the returned Box | box.c:1764-1767 (fn MergeBox) | -- | ceremony | n/a: a `Vec` grows as the values are appended |
| 31 | Copy the centers and corners from the two supplied Boxes into those arrays, in the requested order | box.c:1768-1777 (fn MergeBox) | box_region.rs:1628-1629 (fn region_merge),1614-1625 (fn region_merge) | architecture | ported |
| 32 | Create the new Box, initially with no uncertainty | box.c:1778-1781 (fn MergeBox) | merge_util.rs:168-172 (fn finish_region_merge) | architecture | ported |
| 33 | Free the center and corner arrays | box.c:1782-1785 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 34 | Now attempt to merge with an Interval, giving an Interval, which needs both Regions un-negated | box.c:1786-1789 (fn MergeBox) | box_region.rs:1632-1634 (fn region_merge) | architecture | ported |
| 35 | Allocate memory to store the bounds of the returned Interval | box.c:1790-1793 (fn MergeBox) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 36 | Copy the center and corner from the supplied Box into its part of those arrays | box.c:1794-1804 (fn MergeBox) | box_region.rs:1637 (fn region_merge) | architecture | ported |
| 37 | Convert those center and corner positions into upper and lower bounds, swapping any pair the conversion inverts | box.c:1805-1816 (fn MergeBox) | box_region.rs:656-676 (fn bounds) | architecture | ported |
| 38 | Get the bounds from the Interval and add them into those arrays | box.c:1817-1823 (fn MergeBox) | box_region.rs:1638-1639 (fn region_merge) | architecture | ported |
| 39 | Create the new Interval, initially with no uncertainty | box.c:1824-1827 (fn MergeBox) | merge_util.rs:163-167 (fn finish_region_merge) | architecture | ported |
| 40 | Free the bounds arrays | box.c:1828-1831 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 41 | Now attempt to merge with a NullRegion, giving an Interval; the NullRegion must be negated and the Box must not | box.c:1832-1835 (fn MergeBox) | box_region.rs:1642-1645 (fn region_merge) | architecture | ported |
| 42 | Allocate memory to store the bounds of the returned Interval | box.c:1836-1839 (fn MergeBox) | -- | ceremony | n/a: a `Vec` grows as the bounds are appended |
| 43 | Copy the center and corner from the supplied Box into its part of those arrays | box.c:1840-1850 (fn MergeBox) | box_region.rs:1648 (fn region_merge) | architecture | ported |
| 44 | Convert those center and corner positions into upper and lower bounds | box.c:1851-1861 (fn MergeBox) | box_region.rs:656-676 (fn bounds) | architecture | ported |
| 45 | Fill the NullRegion's axes with bad values to mark them unbounded | box.c:1862-1874 (fn MergeBox) | box_region.rs:1649-1651 (fn region_merge) | architecture | ported |
| 46 | Create the new Interval, initially with no uncertainty | box.c:1875-1879 (fn MergeBox) | merge_util.rs:163-167 (fn finish_region_merge) | architecture | ported |
| 47 | Free the bounds arrays | box.c:1880-1883 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 48 | Now attempt to merge with a PointList, giving a PointList, which needs both Regions un-negated | box.c:1884-1887 (fn MergeBox) | box_region.rs:1654-1658 (fn region_merge) | architecture | ported |
| 49 | The Box must have zero width on every axis; get its center and corner | box.c:1888-1893 (fn MergeBox) | box_region.rs:1661 (fn region_merge) | architecture | ported |
| 50 | Get the size of the Box's base-Frame uncertainty region | box.c:1894-1899 (fn MergeBox) | merge_util.rs:246-266 (fn region_is_point) | architecture | ported |
| 51 | Refuse the merge unless every axis width is below a quarter of the uncertainty on that axis | box.c:1900-1912 (fn MergeBox) | merge_util.rs:268-272 (fn region_is_point) | architecture | ported |
| 52 | Go on to create a new PointList only if the Box is a point | box.c:1913-1915 (fn MergeBox) | box_region.rs:1662-1664 (fn region_merge) | architecture | ported |
| 53 | Get a PointSet holding the supplied PointList's axis values, its point count and pointers to its arrays | box.c:1916-1922 (fn MergeBox) | box_region.rs:1650-1653 (fn region_merge) | architecture | ported |
| 54 | Create a new PointSet with room for the same points but the extra axes, and get pointers to its arrays | box.c:1923-1927 (fn MergeBox) | -- | ceremony | n/a: the merged points are a `Vec<Vec<f64>>` built one point at a time, so there is no PointSet to size in advance |
| 55 | Copy the PointList axis values into the new PointSet, in the requested order | box.c:1928-1937 (fn MergeBox) | box_region.rs:1669 (fn region_merge),1614-1625 (fn region_merge) | architecture | ported |
| 56 | Add the axis values the Box center defines to every point | box.c:1938-1943 (fn MergeBox) | box_region.rs:1608 (fn region_merge),1673 (fn region_merge) | architecture | ported |
| 57 | Create the new PointList, initially with no uncertainty | box.c:1944-1948 (fn MergeBox) | merge_util.rs:173-177 (fn finish_region_merge) | architecture | ported |
| 58 | Free the two PointSets, the center, corner and bounds arrays and the uncertainty | box.c:1949-1961 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 59 | If a new Region was created above, propagate the supplied Region's remaining attributes to it | box.c:1962-1966 (fn MergeBox) | merge_util.rs:151-160 (fn finish_region_merge) | architecture | ported |
| 60 | Ensure the Negated attribute the constructors set is not the one the overlay left | box.c:1967-1971 (fn MergeBox) | merge_util.rs:158 (fn finish_region_merge) | architecture | ported |
| 61 | Assign the new Region an uncertainty only when both supplied Regions have one | box.c:1972-1975 (fn MergeBox) | merge_util.rs:301-303 (fn combine_uncertainties) | architecture | ported |
| 62 | Get the base-Frame uncertainties from the two supplied Regions | box.c:1976-1979 (fn MergeBox) | box_region.rs:1724-1728 (fn region_merge) | architecture | ported |
| 63 | Combine them into a single Prism, in the correct order | box.c:1980-1986 (fn MergeBox) | merge_util.rs:304 (fn combine_uncertainties),306 (fn combine_uncertainties) | architecture | ported |
| 64 | Attempt to simplify the Prism | box.c:1987-1989 (fn MergeBox) | merge_util.rs:304 (fn combine_uncertainties) | architecture | ported |
| 65 | Use the simplified Prism as the uncertainty for the returned Region | box.c:1990-1992 (fn MergeBox) | merge_util.rs:180-182 (fn finish_region_merge) | architecture | ported |
| 66 | Free the uncertainty resources | box.c:1993-1999 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 67 | Get the current Frames from the two Region FrameSets and combine them into one CmpFrame | box.c:2000-2010 (fn MergeBox) | box_region.rs:1692-1699 (fn region_merge) | architecture | ported |
| 68 | Get the two base-to-current Mappings and combine them into one parallel CmpMap connecting the two combined Frames | box.c:2011-2024 (fn MergeBox) | box_region.rs:1701-1723 (fn region_merge) | architecture | ported |
| 69 | Map the new Region into the new current Frame | box.c:2025-2027 (fn MergeBox) | merge_util.rs:181 (fn finish_region_merge) | architecture | ported |
| 70 | The result's FillFactor is the product of the two supplied FillFactors | box.c:2028-2034 (fn MergeBox) | merge_util.rs:72-74 (fn apply_merge_scaling_to) | architecture | ported |
| 71 | Scale the result's MeshSize only when either supplied MeshSize was set | box.c:2035-2042 (fn MergeBox) | merge_util.rs:76 (fn apply_merge_scaling_to) | architecture | ported |
| 72 | Get the two MeshSize values, then clear them so the defaults are returned | box.c:2043-2050 (fn MergeBox) | merge_util.rs:77-78 (fn apply_merge_scaling_to) | architecture | ported |
| 73 | Form each Region's ratio of used to default MeshSize | box.c:2051-2055 (fn MergeBox) | merge_util.rs:79-80 (fn apply_merge_scaling_to) | architecture | ported |
| 74 | Set the result's MeshSize to its own default scaled by the product of the two ratios | box.c:2056-2059 (fn MergeBox) | merge_util.rs:81-82 (fn apply_merge_scaling_to) | architecture | ported |
| 75 | Re-instate the original MeshSize values on the supplied Regions | box.c:2060-2065 (fn MergeBox) | -- | ceremony | n/a: nothing was cleared on the supplied Regions, so there is nothing to restore |
| 76 | Free the Frame, Mapping and intermediate-Region resources | box.c:2066-2077 (fn MergeBox) | -- | ceremony | n/a: owned values are dropped |
| 77 | Annul the returned pointer if an error occurred | box.c:2078-2080 (fn MergeBox) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 78 | Return the result | box.c:2081-2083 (fn MergeBox) | merge_util.rs:187 (fn finish_region_merge) | architecture | ported |
| 79 | `Simplify`: initialize the returned pointer and the working pointers | box.c:3673-3675 (fn Simplify) | -- | ceremony | n/a: the Rust returns `AstResult<(DynMapping, bool)>`, so "no change" is the `false` flag |
| 80 | Check the global error status and bail out | box.c:3676-3678 (fn Simplify) | -- | ceremony | n/a: errors travel in `AstResult`, so there is no ambient status to test on entry |
| 81 | Obtain a pointer to the Region structure | box.c:3679-3681 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 82 | Invoke the parent Region Simplify, which simplifies the encapsulated FrameSet and uncertainty | box.c:3682-3685 (fn Simplify) | box_region.rs:1949 (fn simplified) | architecture | ported |
| 83 | Note whether that simplified anything, by pointer inequality | box.c:3686-3690 (fn Simplify) | box_region.rs:1950 (fn simplified) | architecture | ported |
| 84 | Get the Mapping from base to current Frame | box.c:3691-3693 (fn Simplify) | box_region.rs:1961-1965 (fn simplified) | architecture | ported |
| 85 | Get that Mapping's input and output counts | box.c:3694-3697 (fn Simplify) | box_region.rs:2018 (fn simplified) | architecture | ported |
| 86 | If the Mapping is a PermMap, swap the Box axes explicitly rather than fitting through a mesh | box.c:3698-3702 (fn Simplify) | box_region.rs:1973 (fn simplified) | architecture | ported |
| 87 | See whether the new Box is Negated and Closed | box.c:3703-3706 (fn Simplify) | box_region.rs:542-543 (fn simplify_over_permmap) | architecture | ported |
| 88 | Get a pointer to the Box structure | box.c:3707-3709 (fn Simplify) | -- | ceremony | n/a: the Rust method already has `&self` |
| 89 | Ensure the cached center and extent are up to date | box.c:3710-3712 (fn Simplify) | box_region.rs:544 (fn simplify_over_permmap) | ceremony | ported |
| 90 | Get the input and output permutation arrays and the constants from the PermMap | box.c:3713-3718 (fn Simplify) | box_region.rs:548-550 (fn simplify_over_permmap) | architecture | ported |
| 91 | Allocate memory to hold the current-Frame axis bounds, unset | box.c:3719-3723 (fn Simplify) | box_region.rs:552-553 (fn simplify_over_permmap) | architecture | ported |
| 92 | Check the pointers can be used safely | box.c:3724-3726 (fn Simplify) | -- | ceremony | n/a: `Vec` allocation either succeeds or aborts, so there is no pointer to test |
| 93 | Set the flags saying the Box simplifies and gives neither a NullRegion nor an Interval | box.c:3727-3732 (fn Simplify) | box_region.rs:554-555 (fn simplify_over_permmap),1986 (fn simplified) | architecture | ported |
| 94 | Check each output, which is an axis of the current Frame | box.c:3733-3736 (fn Simplify) | box_region.rs:562 (fn simplify_over_permmap) | architecture | ported |
| 95 | Find the input feeding that output under the forward transformation | box.c:3737-3740 (fn Simplify) | box_region.rs:563 (fn simplify_over_permmap) | architecture | ported |
| 96 | An output fed a constant gets both limits equal to that constant | box.c:3741-3746 (fn Simplify) | box_region.rs:564-569 (fn simplify_over_permmap) | architecture | ported |
| 97 | An output fed `AST__BAD` is unbounded, so the result becomes an Interval rather than a Box | box.c:3747-3753 (fn Simplify) | box_region.rs:570-573 (fn simplify_over_permmap) | architecture | ported |
| 98 | An output fed the value of an input takes that input's limits | box.c:3754-3760 (fn Simplify) | box_region.rs:574-579 (fn simplify_over_permmap) | architecture | ported |
| 99 | A missing bound on either side makes the result an Interval | box.c:3761-3767 (fn Simplify) | box_region.rs:581-584 (fn simplify_over_permmap) | architecture | ported |
| 100 | Any base-Frame axis absent from the current Frame means the PermMap selects a slice, which need not meet the Box; check each entry of the input permutation | box.c:3768-3774 (fn Simplify) | box_region.rs:587-589 (fn simplify_over_permmap) | architecture | ported |
| 101 | Find the output feeding that input under the inverse transformation | box.c:3775-3778 (fn Simplify) | box_region.rs:590 (fn simplify_over_permmap) | architecture | ported |
| 102 | An input fed a constant gives a NullRegion unless the constant lies within the Box's limits on that axis | box.c:3779-3798 (fn Simplify) | box_region.rs:591-605 (fn simplify_over_permmap) | architecture | ported |
| 103 | An input fed `AST__BAD` extends beyond the Box, so the result is a NullRegion | box.c:3799-3805 (fn Simplify) | box_region.rs:609-612 (fn simplify_over_permmap) | architecture | ported |
| 104 | An input fed an output requires the relationship to be bi-directional, or the Box cannot be simplified | box.c:3806-3814 (fn Simplify) | box_region.rs:613-616 (fn simplify_over_permmap) | architecture | ported |
| 105 | If the Box can be simplified, create a new Region of an appropriate class | box.c:3815-3818 (fn Simplify) | box_region.rs:629 (fn simplify_over_permmap) | architecture | ported |
| 106 | Get any uncertainty from the Box, already simplified by the parent Simplify | box.c:3819-3822 (fn Simplify) | box_region.rs:622-626 (fn simplify_over_permmap) | architecture | ported |
| 107 | Get the Frame the Region is defined in | box.c:3823-3825 (fn Simplify) | box_region.rs:627 (fn simplify_over_permmap) | architecture | ported |
| 108 | Annul the original pointer, since the simplified Region replaces it | box.c:3826-3829 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 109 | Create a NullRegion where the slice misses the Box | box.c:3830-3833 (fn Simplify) | box_region.rs:629-630 (fn simplify_over_permmap) | architecture | ported |
| 110 | Create an Interval where any axis is unbounded | box.c:3834-3836 (fn Simplify) | box_region.rs:631-632 (fn simplify_over_permmap) | architecture | ported |
| 111 | Otherwise create a Box from the permuted bounds | box.c:3837-3840 (fn Simplify) | box_region.rs:633-635 (fn simplify_over_permmap) | architecture | ported |
| 112 | Negate the new Region if the original Box was negated | box.c:3841-3843 (fn Simplify) | box_region.rs:639-641 (fn simplify_over_permmap) | architecture | ported |
| 113 | Free the uncertainty and Frame pointers | box.c:3844-3850 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 114 | Free the permutation arrays, the constants and the bounds arrays | box.c:3851-3857 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 115 | If the Mapping is neither a PermMap nor a UnitMap, try re-defining the Box within its current Frame, testing the result by transforming its boundary | box.c:3858-3864 (fn Simplify) | box_region.rs:1994 (fn simplified) | architecture | ported |
| 116 | Get a pointer to the current Frame | box.c:3865-3867 (fn Simplify) | box_region.rs:2004 (fn simplified) | architecture | ported |
| 117 | Get a mesh of points covering the Box in its current Frame | box.c:3868-3870 (fn Simplify) | box_region.rs:2000-2002 (fn simplified) | architecture | ported |
| 118 | Get the Region describing the positional uncertainty in the current Frame | box.c:3871-3874 (fn Simplify) | box_region.rs:2003 (fn simplified) | architecture | ported |
| 119 | Find the best-fitting current-Frame Box through those points | box.c:3875-3878 (fn Simplify) | box_region.rs:2005 (fn simplified),761 (fn best_box) | architecture | ported |
| 120 | See whether every mesh point falls on that Box's boundary, to within the uncertainty | box.c:3879-3882 (fn Simplify) | box_region.rs:2006-2009 (fn simplified) | architecture | ported |
| 121 | If so, check the converse, transforming the fitted Box's own mesh into the original base Frame for `astRegPins` | box.c:3883-3890 (fn Simplify) | box_region.rs:2011-2014 (fn simplified) | architecture | ported |
| 122 | If so, use the new Box in place of the original | box.c:3891-3896 (fn Simplify) | box_region.rs:2015-2016 (fn simplified) | architecture | ported |
| 123 | Free the fitted Box's mesh | box.c:3897-3899 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 124 | Otherwise, for a 2-D Box, see whether a Polygon represents it accurately | box.c:3900-3904 (fn Simplify) | box_region.rs:2018 (fn simplified) | architecture | ported |
| 125 | Create a PointSet holding the base-Frame axis values at the four corners of the Box | box.c:3905-3909 (fn Simplify) | -- | ceremony | n/a: the four corners are a fixed-size array rebuilt per winding at box_region.rs:472-486 (fn convert_to_polygon), so there is no PointSet to allocate up front |
| 126 | Get the Box structure and refresh its cached center and extent | box.c:3910-3912 (fn Simplify) | box_region.rs:438-441 (fn convert_to_polygon) | architecture | ported |
| 127 | For a SkyFrame, transform the Box center into the current Frame to test the winding against | box.c:3913-3921 (fn Simplify) | box_region.rs:435 (fn convert_to_polygon),453-459 (fn convert_to_polygon) | architecture | ported |
| 128 | The vertex order decides whether the polygon's inside is its interior; try clockwise first and anti-clockwise second | box.c:3922-3930 (fn Simplify) | box_region.rs:470 (fn convert_to_polygon) | architecture | ported |
| 129 | Store the four base-Frame corners in clockwise order | box.c:3931-3943 (fn Simplify) | box_region.rs:472-478 (fn convert_to_polygon) | architecture | ported |
| 130 | Store them in anti-clockwise order instead | box.c:3944-3957 (fn Simplify) | box_region.rs:479-486 (fn convert_to_polygon) | architecture | ported |
| 131 | Transform the Box corners into the current Frame | box.c:3958-3962 (fn Simplify) | box_region.rs:489-491 (fn convert_to_polygon) | architecture | ported |
| 132 | Create a Polygon from those points | box.c:3963-3969 (fn Simplify) | box_region.rs:493-497 (fn convert_to_polygon) | architecture | ported |
| 133 | Decide whether the inside is the interior: for a SkyFrame by transforming the center through the Polygon, otherwise by its bounded flag | box.c:3970-3978 (fn Simplify) | box_region.rs:502-506 (fn convert_to_polygon) | architecture | ported |
| 134 | Break out of the winding loop as soon as one is right way round | box.c:3979-3984 (fn Simplify) | box_region.rs:508-511 (fn convert_to_polygon) | architecture | ported |
| 135 | Free the rejected Polygon and its transformed corners | box.c:3985-3988 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 136 | Free the SkyFrame center-probe PointSets | box.c:3989-3994 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 137 | See whether every point of the Box mesh falls on the Polygon's boundary, to within the uncertainty | box.c:3995-4000 (fn Simplify) | box_region.rs:521 (fn convert_to_polygon) | architecture | ported |
| 138 | If so, use the new Polygon in place of the original Box | box.c:4001-4006 (fn Simplify) | box_region.rs:522-525 (fn convert_to_polygon),2021-2024 (fn simplified) | architecture | ported |
| 139 | Free the Polygon and the corner PointSet | box.c:4007-4013 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 140 | Failing everything so far, split the Box across two axis subsets and join the two simplified halves into a Prism | box.c:4014-4022 (fn Simplify) | box_region.rs:2030 (fn simplified),328 (fn convert_to_prism) | architecture | ported |
| 141 | Adopt the Prism if it is not a clone of the Box, and annul it otherwise | box.c:4023-4031 (fn Simplify) | box_region.rs:2030-2032 (fn simplified),417 (fn convert_to_prism) | architecture | ported |
| 142 | Free the Frame, mesh, uncertainty, fitted Box and the base-to-current Mapping | box.c:4032-4040 (fn Simplify) | -- | ceremony | n/a: owned values are dropped |
| 143 | If anything simplified, overlay the supplied Region's attributes onto the result and return it | box.c:4041-4048 (fn Simplify) | box_region.rs:2041-2043 (fn simplified) | architecture | ported |
| 144 | Otherwise annul the working Region and return a clone of the supplied pointer | box.c:4049-4053 (fn Simplify) | box_region.rs:2044-2046 (fn simplified) | architecture | ported |
| 145 | Annul the returned pointer if an error occurred | box.c:4054-4056 (fn Simplify) | -- | ceremony | n/a: an error is a `Result::Err`, and owned values are dropped |
| 146 | Return the result | box.c:4057-4059 (fn Simplify) | box_region.rs:2043 (fn simplified),2046 (fn simplified) | architecture | ported |

## Row 1 — the merged-pair index starts at zero

`box.c::MapMerge` and `interval.c::MapMerge` are the same function at a constant
offset: `diff` over `box.c:1545-1634 (fn MapMerge)` and
`interval.c:1160-1249 (fn MapMerge)` shows one substantive difference, which is
that the Box initializes `i1 = 0` (`box.c:1547 (fn MapMerge)`) where the Interval
initializes `i1 = -1` (`interval.c:1162 (fn MapMerge)`). Neither value is ever read
before it is assigned by a successful merge attempt, so the difference is
invisible: both bodies test `i1 > -1` only after the lower-neighbor and
upper-neighbor attempts have run, and both attempts set `i1` when they succeed.
The port returns `Option<usize>` and so carries no sentinel at all.

## Row 8 — parallel only

C wraps the whole neighbor-merge half of `MapMerge` in a parallel test, because
two Regions in series are not a Region: the first Region's output feeds the
second's input, which is a constraint on a transformed position rather than a
region of a combined Frame. `region_map_merge`
(`mappings/simplify.rs:1808 (fn region_map_merge)`) makes the same test and returns
`None` for series, which sends the pair back to the generic path.

## Rows 49 to 51 — a zero-width Box, tested without aliasing

Merging a Box with a PointList requires the Box to be a point, which C tests by
comparing each axis width against the Box's own uncertainty. `box.c:1894-1899 (fn MergeBox)`
loads `lbnd` and `ubnd` with the bounds of the base-Frame uncertainty Region and
`box.c:1900-1912 (fn MergeBox)` refuses the merge unless every axis width is under a
quarter of the corresponding uncertainty width. Both arrays hold what their names
say, and `astGetUncFrm` supplies a default uncertainty when the Box has none, so
the test is meaningful for every Box.

The Interval's PointList arm applies the same test to its own half-width
(`interval.c:1509-1510 (fn MergeInterval)`, rows 63 to 66 of
`region_interval.md`). The port implements C's Box form once, in
`merge_util.rs:238-274 (fn region_is_point)`, including the default-uncertainty
fallback C gets from `astGetUncFrm`
(`merge_util.rs:254-266 (fn region_is_point)`), and both classes call it.

## Rows 71 to 75 — `MeshSize` is never cleared

C's merge reads each supplied Region's `MeshSize`, clears it so the default is
returned for the ratio, and re-instates the original afterwards
(`box.c:2035-2065 (fn MergeBox)`). The port computes the same ratios without
mutating the supplied Regions: `effective_mesh_size` reports the value in use and
`default_mesh_size(naxes)` supplies the denominator directly
(`merge_util.rs:76-82 (fn apply_merge_scaling_to)`), so there is nothing to restore.

## Rows 86 to 114 — the PermMap branch

A PermMap between base and current Frame permutes, drops and supplies axes, and
`simplify_over_permmap` (`box_region.rs:541-644 (fn simplify_over_permmap)`) reads the
PermMap's own arrays rather than fitting a Box through a transformed mesh. That
expresses three outcomes the generic best-fit path cannot reach: an axis fed a
constant gets equal limits, an axis fed `AST__BAD` or missing a bound makes the
result an Interval rather than a Box, and a slice that misses the Box makes it a
NullRegion. A permutation that is not bi-directional refuses the simplification
altogether, and because C clears `simpler` at `box.c:4049 (fn Simplify)` rather than
merely declining to improve on it, that refusal discards the parent Region
simplification with it. The port clears the same flag at
`box_region.rs:1984 (fn simplified)`.

One representational difference. The port stores the public 1-based permutation
form, where `+j` names axis `j`, `0` names `AST__BAD` and `-j` names
`constants[j-1]`, while C stores the internal 0-based form and turns a public `0`
into `nin` or `nout`; C's `feed >= nin` test is therefore written
`feed == 0 || feed as usize > nin` here (`box_region.rs:570-573 (fn simplify_over_permmap)`).

The NullRegion verdict accumulates. Each constant-fed input is a separate plane
the slice must lie in, so any one of them falling outside the Box empties the
intersection (`box.c:3794-3796 (fn Simplify)`), as does an unbounded input
(`box.c:3804 (fn Simplify)`); `box_region.rs:606-612 (fn simplify_over_permmap)`
sets the flag in both places and never clears it. C assigned the verdict instead
until `83925af2`, which let the last constant-fed input discard every earlier
one; `docs/issues/c-library-quirks.md` carries the history.

`cap_box_permmap_const`, `cap_box_permmap_null` and `cap_box_permmap_nonbidi` gate
the constant, NullRegion and non-bidirectional exits, and
`cap_box_permmap_null_first` gates the accumulation by putting the outside
constant first, where an assignment would lose it. The four
`simplify_permmap_slice_*` cases in `crates/starlink-ast/tests/test_box_region.rs`
port upstream's `checkBoxPermMapSlices`. The unbounded exit has no
oracle fixture, because `astMapRegion` refuses to build a Region whose forward
transformation gives a bad axis value (`region.c:5663 (fn MapRegion)`), so it is
covered by the in-crate Rust test
`box_region.rs::tests::permmap_bad_output_simplifies_to_unbounded_interval` — see
PLAN.md §10.13.

## Rows 115 to 124 — the best-fit Box, then the Polygon only if no Box fits

C has one way to re-express a Box in its current Frame: fit the best Box through
a mesh covering the original (`box.c:3875-3878 (fn Simplify)`), check that every mesh
point lies on the fitted Box's boundary, then check the converse by transforming
the fitted Box's own mesh back into the base Frame
(`box.c:3883-3890 (fn Simplify)`).
The port does the same, with no other route: `best_box`, the forward `reg_pins`
and the inverse mesh check (`box_region.rs:2005-2014 (fn simplified)`).
C tries the Polygon only in the `else` of the forward test
(`box.c:3900-3904 (fn Simplify)`), so a fitted Box that passes the forward test and
fails the converse goes on to the Prism split without trying a Polygon; the
port's `else if` keeps that (`box_region.rs:2018 (fn simplified)`).
The Prism split that follows is tried only when nothing has simplified, the
parent's FrameSet and uncertainty simplification included, as C's `simpler`
says (`box_region.rs:2030 (fn simplified)`).

## Rows 125 to 139 — the two windings are exhaustive

A Polygon's vertices decide which side of its boundary is its inside, so C builds
the four corners twice, clockwise and anti-clockwise, and keeps the winding whose
inside is the Box's interior (`box.c:3922-3984 (fn Simplify)`). For a SkyFrame it
decides by transforming the Box center through the candidate Polygon; otherwise it
asks whether the Polygon is bounded, since the complement of a polygon is not.

Where neither winding satisfies that test, C annuls the candidate on each
iteration (`box.c:3985-3988 (fn Simplify)`) and reaches `astRegPins` with a null
Polygon pointer (`box.c:3995-4000 (fn Simplify)`), while
`convert_to_polygon` returns `Ok(None)` (`box_region.rs:514-516 (fn convert_to_polygon)`).
The two orders are exhaustive — exactly one of them is bounded, and for a SkyFrame
the transformed center lies inside exactly one — so neither path is reachable and
the difference is in the unreachable branch only.

## Rows 140 and 141 — where the Prism attempt sits

C's `astConvertToPrism` attempt is guarded by `if( !simpler )`
(`box.c:4020 (fn Simplify)`) at the same nesting level as the best-fit and Polygon
attempts, inside the "neither PermMap nor UnitMap" branch that the brace at
`box.c:4037 (fn Simplify)` closes.
The port has the same guard at the same level (`box_region.rs:2030 (fn simplified)`).
`simpler` includes the parent's simplification, so a Box whose FrameSet or
uncertainty simplified is not split.

## Row 143 — overlaying attributes, and not acquiring an uncertainty

`astRegOverlay` at `box.c:4045 (fn Simplify)` is called with `unc = 1`, which clears
the result's uncertainty where the supplied Region had none. The port passes the
same flag (`box_region.rs:2033-2035 (fn simplified)`), so a Box fitted through a mesh
derived from a *default* uncertainty does not come back carrying that uncertainty
as an explicit one. Without it, simplifying a Box would set an attribute the
original did not have, and the difference would show in the dump.

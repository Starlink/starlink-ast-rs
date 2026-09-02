# `Simplify` / `MapMerge` audit — Box

C source: reference/starlink-ast/src/box.c
Rust: crates/ast-core/src/regions/box_region.rs
Audited: 2026-08-21 against pin 3cf4bc5c

Two C bodies. `MapMerge` (`box.c:1573-1655`) tries to replace the Box with a
simpler Region and otherwise merges it with a neighbouring Region in parallel.
`Simplify` (`box.c:3694-4073`) calls the parent Region simplify, then works
through four ways of re-expressing the Box: an explicit PermMap axis swap, a
best-fitting Box in the current Frame, a Polygon for the 2-D case, and a split
into a Prism.

`BoxRegion::map_merge` (`box_region.rs:1775-1782`) delegates to
`simplify::region_map_merge` (`simplify.rs:1281-1346`); `BoxRegion::simplified`
is `box_region.rs:1596-1740`.

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | `MapMerge`: get a pointer to the Box | box.c:1573-1575 | -- | n/a: the Rust method already has `&self` |
| 2 | See if the Box can be replaced by a simpler Region, without reference to its neighbours | box.c:1576-1579 | simplify.rs:1288-1297 | ported |
| 3 | Simplify the Box, assuming a changed pointer means something happened | box.c:1580-1584 | simplify.rs:1292-1294 | ported |
| 4 | Replace the Box in the list with the new Region, marking it forward | box.c:1585-1591 | simplify.rs:1294-1295 | ported |
| 5 | Return the index of the first modified element | box.c:1592-1594 | simplify.rs:1295 | ported |
| 6 | Otherwise merge with a neighbouring Region, in parallel only | box.c:1595-1601 | simplify.rs:1299-1301 | ported |
| 7 | Try the lower neighbour first | box.c:1602-1611 | simplify.rs:1302-1311 | ported |
| 8 | Failing that, try the upper neighbour | box.c:1612-1622 | simplify.rs:1312-1326 | ported |
| 9 | On success, replace the first of the pair with the merged Region and clear its invert flag | box.c:1623-1631 | simplify.rs:1327-1341 | ported |
| 10 | Annul the second and shuffle the rest down, clearing the vacated slot | box.c:1632-1643 | simplify.rs:1327-1341 | ported |
| 11 | Decrement the Mapping count and return the index of the first modified element | box.c:1644-1653 | simplify.rs:1341 | ported |
| 12 | Return the result | box.c:1654-1655 | simplify.rs:1345 | ported |
| 13 | `Simplify`: invoke the parent Region simplify, which handles the FrameSet and uncertainty | box.c:3703-3706 | box_region.rs:1601 | ported |
| 14 | Note whether that simplified anything, by pointer inequality | box.c:3707-3711 | box_region.rs:1601-1615 | ported |
| 15 | Get the base-to-current Mapping and its input and output counts | box.c:3712-3718 | box_region.rs:1617-1621, 431-432 | ported |
| 16 | If that Mapping is a PermMap, swap the Box axes explicitly to build a new Box, Interval or NullRegion | box.c:3719-3744 | box_region.rs:1629-1641, 500-524 | ported |
| 17 | Note whether the new Region is Negated and Closed, and refresh the cached information | box.c:3724-3733 | box_region.rs:427-429 | ported |
| 18 | Take the PermMap's input and output permutation arrays and its constants | box.c:3734-3747 | box_region.rs:431-435 | ported |
| 19 | For each current-Frame axis, find the base-Frame axis feeding it | box.c:3748-3761 | box_region.rs:447-448 | ported |
| 20 | An output fed a constant gets both limits equal to that constant | box.c:3762-3767 | box_region.rs:449-454 | ported |
| 21 | An output fed `AST__BAD` is unbounded, so the result becomes an Interval | box.c:3768-3774 | box_region.rs:455-458 | ported |
| 22 | An output fed an input takes that input's limits | box.c:3775-3781 | box_region.rs:459-464 | ported |
| 23 | A missing bound makes the result an Interval rather than a Box | box.c:3782-3788 | box_region.rs:466-470 | ported |
| 24 | When base axes are absent from the current Frame, check the PermMap's slice actually intersects the Box | box.c:3789-3882 | box_region.rs:472-498 | ported |
| 25 | Get the current Frame | box.c:3883-3885 | box_region.rs:1687 | ported |
| 26 | Get a mesh of points covering the Box in its current Frame | box.c:3886-3888 | box_region.rs:1683-1685 | ported |
| 27 | Get the positional uncertainty in the current Frame | box.c:3889-3892 | box_region.rs:1686 | ported |
| 28 | Find the best-fitting current-Frame Box through those points | box.c:3893-3896 | box_region.rs:1689 | ported |
| 29 | Check every mesh point falls on that Box's boundary, within the uncertainty | box.c:3897-3900 | box_region.rs:1690 | ported |
| 30 | Check the converse, transforming the new Box's mesh back into the original base Frame | box.c:3901-3908 | box_region.rs:1696-1699 | ported |
| 31 | If both hold, use the new Box | box.c:3909-3917 | box_region.rs:1700-1703 | ported |
| 32 | Otherwise, for a 2-D Box, try a Polygon through the four transformed corners | box.c:3918-3939 | box_region.rs:1709-1715 | ported |
| 33 | Choose the vertex order so the polygon's interior is the Box's inside | box.c:3940-3974 | box_region.rs:1709-1715 | ported |
| 34 | Transform the corners into the current Frame and build the Polygon | box.c:3975-3986 | box_region.rs:1709-1715 | ported |
| 35 | Stop as soon as a bounded Polygon is produced | box.c:3987-4011 | box_region.rs:1709-1715 | ported |
| 36 | Check every mesh point falls on the Polygon's boundary, within the uncertainty | box.c:4012-4015 | box_region.rs:1709-1715 | ported |
| 37 | If so, use the Polygon | box.c:4016-4028 | box_region.rs:1709-1715 | ported |
| 38 | Failing everything, split the Box into two Regions on distinct axis subsets and join them as a Prism | box.c:4029-4055 | box_region.rs:1720-1725 | ported |
| 39 | If anything simplified, overlay the original Region's attributes onto the result | box.c:4056-4068 | box_region.rs:1734-1736 | ported |
| 40 | Annul the returned pointer if an error occurred | box.c:4069-4071 | -- | n/a: an error is a `Result::Err` |
| 41 | Return the result | box.c:4072-4073 | box_region.rs:1736, 1738 | ported |

## Rows 16 to 24 — the PermMap branch

`BoxRegion::simplify_over_permmap` (`box_region.rs:426-525`) reads the PermMap's
permutation arrays and constants directly rather than fitting a Box through a
mesh, so it can express what the generic best-fit path cannot: a current-Frame
axis fed a constant gets that constant as both its limits, an axis fed
`AST__BAD` or missing a bound makes the result an Interval, a PermMap slice that
misses the Box gives a NullRegion, and a permutation that is not
bi-directional clears `simpler`, which discards the parent simplification with
it (`box.c:4064`).

The port stores the public 1-based permutation form (`+j` names axis `j`, `0`
names `AST__BAD`, `-j` names `constants[j-1]`) where C stores the 0-based form
and turns a public `0` into `nin` / `nout`; row 21's `feed >= nin` test is
therefore `feed == 0 || feed > nin` here. Row 24's `isNull` is an assignment in
C rather than an accumulation, so the last constant-fed input decides — mirrored
deliberately and recorded in `docs/issues/c-library-quirks.md`.

`cap_box_permmap_const`, `cap_box_permmap_null` and `cap_box_permmap_nonbidi`
gate rows 20, 24 and the non-bidirectional exit against the C oracle. Rows 21
and 23 have no oracle fixture: `astMapRegion` refuses to build a Region whose
forward transformation gives a bad axis value (`region.c:5539`), which is the
only public way to install a base-to-current PermMap, so the unbounded exit is
covered by an in-crate Rust test instead
(`box_region.rs::tests::permmap_bad_output_simplifies_to_unbounded_interval`).
See PLAN.md §10.13.

## Row 39 — overlaying attributes, and not acquiring an uncertainty

C copies the supplied Region's attributes onto the result with `astRegOverlay`.
The port's call passes `unc = true`, which clears the result's uncertainty when
the original had none — so a Box fitted through a mesh derived from a *default*
uncertainty does not come back carrying that as an explicit one.

## Row 6 — parallel only

C wraps the whole neighbour-merge half in a parallel test, because two Regions in
series are not a Region. `region_map_merge` returns to the generic path for
series at `simplify.rs:1299-1301`.

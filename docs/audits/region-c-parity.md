# Region classes: C-parity audit

Date: 2026-10-08.
Scope: the Region base class (`region.c`) and the nine ported Region classes (Box, Circle, Ellipse, Polygon, Interval, PointList, NullRegion, CmpRegion, Prism), their attributes, methods and per-class extension points.
`MapMerge` and `Simplify` are out of scope: each class's are audited in `docs/audits/mapmerge/region_*.md` and `docs/reports/2026-09-03-simplify-parity-final.md`.
`Moc` and the `Stc*` classes were out of scope because they were not ported; this audit was started because porting `StcResourceProfile` needed Region seams the port did not have. `StcResourceProfile`, the other Stc classes and the core of `Moc` have since landed (see Progress).
`ast-capi` is out of scope: no string-attribute dispatch exists, so C's `GetAttrib`, `SetAttrib`, `TestAttrib` and `ClearAttrib` overrides have no counterpart by design, but the typed accessor for every attribute is in scope.
Plot-only methods (`RegTrace`, `RegSplit`, `BndMesh`) are listed as gated.

The audit was made against C on branch `u/timj/norm-unit` at `436f069f` and Rust at `d250706f`.
Line numbers are re-anchored as the code changes, so they refer to the current `reference/starlink-ast` checkout (relative to `src/`) and the current `crates/ast-core/src/`; a finding describes the code as it was when audited unless the Progress table says it is fixed.
These older Region audits are superseded by this one, and the status of each of their findings is in the last section of Appendices A and B: `region-base.md`, `region-box-interval-pointlist.md`, `region-circle-ellipse-polygon-null.md`, `region-cmpregion-prism.md`, `region-synthesis-plan.md` and `region-class-coverage-audit.md`.
The last of these recorded `RegSetAttrib`/`RegClearAttrib` and the transform direction as matching C; neither did.

## Method

1. Every `vtab->X = X` assignment in `region.c` and in each class's C source, every attribute, and every public function was listed and mapped onto a Rust trait method, helper, inherent method, or "no counterpart", and classified as ceremony or architecture (a method a generic driver dispatches per class).
2. Each C method with a Rust counterpart was compared by reading both implementations, in two groups: the base class, then the nine classes.
3. Every HIGH finding was run through paired probes: a C program linked against the built reference `libast`, and a scratch Rust program depending on `ast-core`, given the same inputs (Appendix C).
   Thirteen were confirmed, two partly, and one (PL-1) not, and is withdrawn.

Ratings: HIGH means wrong membership or coordinates, a panic, hang or stack overflow, or silent data loss.
MED means a wrong default, a missing error, a wrong dump, a missing per-class seam, or missing public API.
LOW means cosmetic, latent, or internal-only.
Findings not probed are marked UNVERIFIED in the appendices, with the probe that would settle them.

## Summary

The transform direction defect found while porting Stc was fixed first, in `d250706f`: every Region now masks on the inverse transformation as on the forward one.

Base class: 21 findings, 5 HIGH, 9 MED, 7 LOW (Appendix A); RB-20 and RB-21 were found while fixing others.
Classes: 40 findings, 10 HIGH (after PL-1 is withdrawn), 20 MED, 10 LOW (Appendix B); X-1, IN-4, CR-6 and PR-2 are the class-side views of RB-4, RB-1, RB-3 and RB-7.

| Theme | What is wrong | Findings |
|---|---|---|
| A. Missing per-class seams | C methods a generic driver dispatches per class have no trait method in Rust: OverlapX, BndBaseMesh, RegSetAttrib and RegClearAttrib, RegMesh, GetFillFactor, the Closed and MeshSize setters a compound Region propagates, Mask, Decompose, and the Frame methods SubFrame and PickAxes. Each is centralised, stubbed, or falls back to a planar default. | RB-1, RB-3, RB-5, RB-7, RB-12, CR-4, CR-6, PR-2, PR-4 |
| B. Uncertainty ignored in membership | C widens a zero-width or boundary test by the Region's uncertainty; Rust tests bit-exactly. | BX-2, IN-1 |
| C. Planar geometry on curved Frames | Box extents, Ellipse membership, Polygon meshes, Interval and PointList bounds, and the Frame methods a Region forwards treat sky axes as Cartesian. | BX-1, EL-1, PG-1, IN-1, PL-2, RB-5 |
| D. Wrong Frame | A value C computes in the current Frame is computed in the base Frame. | RB-4, RB-13, CR-3, X-1 |
| E. Construction and load | The CmpRegion constructor does not convert its second Region into the first's Frame, XOR is lost on load, and constructors and loaders skip C's checks. | CR-1, CR-2, BX-4, CI-1, RB-9, RB-10, PR-3 |
| F. Identity | `equal` ignores the base Frame and the base-to-current Mapping, so a Region and a remapped copy compare equal and overlap as "same". | RB-2 |

**Why the tests missed these.** The Region tests build Regions in Cartesian Frames whose base-to-current Mapping is a UnitMap, and test points well inside or well outside.
A sky Frame, a remapped Region, a point within the uncertainty of a boundary, and a C dump of each class with its less common options (XOR, form-0 Ellipse, an excluded Interval) would have caught every HIGH finding.

## Proposed order of work

Seams first, because the HIGH findings in themes B to F, and the Stc port, are fixed by porting into them:

1. OverlapX and BndBaseMesh as trait methods (RB-1, RB-7, IN-4, PR-2), with overlap code 0 kept distinct (RB-6).
2. RegSetAttrib and RegClearAttrib as trait methods honouring Adaptive (RB-3, CR-6), and the Closed, MeshSize and FillFactor propagation of the compound Regions (CR-4, PR-4).
3. The Frame methods a Region forwards (RB-5), with PickAxes returning a Region.
4. Region `equal` as one base comparison that classes extend (RB-2).
5. The generic GetRegionDisc (RB-4, X-1), current-Frame points (RB-13), and the Mask direction and seam (RB-12).
6. The per-class HIGH findings: BX-1, BX-2, EL-1, PG-1, IN-1 to IN-3, CR-1 to CR-3.
7. MED and LOW findings.

## Progress

| IDs | Status | Evidence |
|---|---|---|
| (transform direction) | FIXED 2026-10-08 in `d250706f`: the inverse transformation masks as the forward does | `tests/test_box_region.rs` (`inverse_transform_masks_as_forward_does`) and the `*_masks_as_forward` test of each class |
| RB-1, RB-6, RB-7, RB-8, IN-4, PR-2 | FIXED 2026-10-08: `overlap`, `overlap_x` and `bnd_base_mesh` are Region trait methods with C's defaults (region.c `Overlap`, `OverlapX`, `BndBaseMesh`), so the generic overlap meshes an unbounded Interval through Interval's own `BndBaseMesh` instead of recursing, and NullRegion and Prism override them as C's do. Code 0 is `Overlap::Incomparable`, which CmpRegion's bounds and simplification treat as C does. A mesh that fails or has no good points is discarded and the Regions swapped as C does, and the generic `BndBaseMesh` returns one bad point when the boundary misses the box. C's Prism fallback passed the Regions to `OverlapX` the wrong way round; fixed upstream (0516a13d) and ported with the fix | `tests/test_region_overlap_oracle.rs`: 10 Region pairs in both orders against C's codes from `tools/oracle_fixtures/region_overlap_probe.c` |
| RB-3, CR-6, PR-3 | FIXED 2026-10-08: a Frame attribute set or cleared through a Region is made through its FrameSet, remapping the Region, only when the Region is Adaptive; otherwise through the `reg_frame_attrib` seam (region.c `RegSetAttrib` and `RegClearAttrib`, which differ only in the change made), which makes it to the current Frame, to the base-Frame axis the current-Frame axis splits onto, and to the uncertainty, ignoring AST__BADAT where C annuls it; CmpRegion and Prism pass it on to their components as C's overrides do. The inherent `with_integrity` takes the same path. The Prism builder clears the RegionFS of a component with a unit base->current Mapping, as `astInitPrism` does. A Region's dump writes `ActUnt` as set, as frame.c `Dump` does, and no `Prm` cards, which belong to the current Frame. C lower-cased the value of a setting made to a Region that is not Adaptive; fixed upstream (bb287a61) and ported with the fix | `tests/test_region_attrib_oracle.rs`: 8 Regions after a setting or clearing, Adaptive or not, through a permutation, with an uncertainty, and in a CmpRegion and a Prism, dumps byte-exact and membership against C from `tools/oracle_fixtures/region_attrib_probe.c` |
| RB-5, RB-21 | FIXED 2026-10-08: a Region forwards to the current Frame of its FrameSet the Frame methods region.c overrides: `offset2`, `resolve`, `resolve_points`, `intersect`, the `line_*` methods, `norm_box`, `sub_frame`, `system_string`, the `ax_*` methods and `validate_frame_axis`; it is never a unit Frame; `perm_axes` permutes its FrameSet; and `pick_axes_snapshot` gives a Region where the class can form one, as `astPickAxes` does. A FrameSet's `perm_axes` remaps its current Frame through the PermMap with `remap_frame`, as frameset.c `PermAxes` does through `astRemapFrame`, so the result is simplified | `tests/test_region_frame_oracle.rs`: Offset2, Resolve, AxDistance, AxOffset, System, PickAxes and astConvert in both directions for an ICRS Circle, bit-exact against C from `tools/oracle_fixtures/region_frame_probe.c`; `tests/test_region_attrib_oracle.rs` (`permuted_box`) |
| RB-2 | FIXED 2026-10-08: every Region's equality starts from one port of region.c `Equal` (`data::region_equal`): the same class, defining points equal within pointset.c `Equal`'s default accuracy, equal base and current Frames, an equal base->current Mapping, and the same Negated and Closed values as the class reports them; CmpRegion and Prism then compare their components, and CmpRegion its operator, as their C overrides do. The simplify regression's equality check now compares the simplified Mapping as read back from its dump, as the C harness's `ast_astequal` does, since a dump written with `Frm` does not carry the base Frame equality compares | `tests/test_region_equal_oracle.rs`: 5 pairs, equality and overlap, against C from `tools/oracle_fixtures/region_equal_probe.c` |
| RB-4, X-1, RB-13, RB-18 (reg_centre), NR-1, CI-2 | FIXED 2026-10-08: `get_region_disc` is one port of region.c `GetRegionDisc`, which no class overrides (`regions::region_disc`): AST__INVAR unless 2-dimensional, the Region taken un-negated, a bad centre and radius when unbounded, and otherwise the centre found in a local system along the current-Frame boundary mesh and the radius the greatest distance to it, times 1.000001. The nine per-class versions, and the `RegionGeometry::reg_centre` centroids only they used, are gone. `region_points` gives the defining points in the current Frame, as `astGetRegionPoints` does; `get_region_points` stays the base-Frame points the port's algorithms use. NullRegion's `reg_mesh` is AST__INTER, as its C `RegMesh` is, so its disc is that error. `Circle::circle_pars` is `astCirclePars`: the centre and circumference point in the current Frame and the Frame's distance between them. A sky Ellipse built by `EllipseBuilder` with angle 0 has its major axis along the first axis where C's has it along the second (EL-2), so the C-oracle case reads C's dump | `tests/test_region_disc_oracle.rs`: 10 Regions against C from `tools/oracle_fixtures/region_disc_probe.c` (the sky Polygon and bounded Interval wait on PG-1 and IN-3, whose meshes the disc is built from); `tests/test_region_frame_oracle.rs` (`region_points_are_in_the_current_frame`) |
| RB-12 | FIXED 2026-10-08: `mask` is `astMask<X>`: its Mapping is optional and goes from the Region's Frame to the grid, the grid's dimensions and bounds are checked with C's AST__NGDIN and AST__GBDIN, and which pixels it sets comes from the per-class `mask_plan` seam: region.c `Mask<X>` by default (the bounding-box fill and, within the box, the pixels whose centres the Region maps to bad, which is what C's nearest-neighbour `astResample` through the Region sets) and pointlist.c `Mask<X>` for a PointList. Pixel centres on a Region's boundary are masked neither inside nor outside, as in C. C wrote beyond the array for a PointList point outside the grid; fixed upstream (ba5a6199) and ported with the fix | `tests/test_region_mask_oracle.rs`: 10 masks, pixels and counts, against C from `tools/oracle_fixtures/region_mask_probe.c` |
| BX-1, BX-2 | FIXED 2026-10-08: a Box's limits are box.c `Cache`'s, its extent on each axis the base Frame's axis distance from centre to corner, so a sky Box whose corner wraps through RA 0 spans the short way round; and its membership test widens a copy of the limits to the width of the uncertainty when Negated and Closed differ, as box.c `Transform` does. C made that widening in the shared cache, and only when the cache was stale, so it depended on call order and leaked into the bounding box; fixed upstream (d0853dcd) and ported with the fix | `tests/test_region_box_oracle.rs`: membership of 16 points in 4 Boxes and a sky Box's bounds, against C from `tools/oracle_fixtures/region_box_probe.c` |
| BX-3 | FIXED 2026-10-08: a Box's boundary mesh is box.c `RegBaseMesh`: in 2-D, MeshSize points shared between the edges by their lengths, measured along each edge in the base Frame's own distance unless the Frame is flat, when the axes have one measure of distance, and a quarter to each edge otherwise, stepped from each corner by repeated addition; in N-D, a grid on each face, the upper face of each axis first. C meshed a single-point Box with the first axis's value on every axis; fixed upstream (4785102b) and ported with the fix | `tests/test_region_box_oracle.rs` (`meshes_match_the_c_library`): 5 meshes bit-exact against C |
| IN-1, IN-2, IN-3 | FIXED 2026-10-08: an Interval equivalent to a Box is tested and meshed as that Box (interval.c `Transform`, `RegBaseMesh`), so a sky Interval wraps through RA 0 and takes the Box's uncertainty widening; otherwise equal limits are separated by the width of the uncertainty for a closed un-negated or open negated Interval, as C's per-axis test does, and an Interval with no equivalent Box has no mesh (AST__INTER). An Interval is bounded exactly when it has an equivalent Box (interval.c `GetBounded`), so one excluding every axis is not | `tests/test_region_interval_oracle.rs`: membership and Bounded for 5 Intervals and the mesh of [0,1]x[0,1], against C from `tools/oracle_fixtures/region_interval_probe.c` |
| EL-1 | FIXED 2026-10-08: an Ellipse's axes come from ellipse.c `CalcPars` (the base Frame's distance to point 1 and point 2 resolved along and across the primary axis, so point 2 need only lie on the Ellipse), and membership resolves each point with the Frame's `resolve_points`, as ellipse.c `Transform` does, so a sky Ellipse is measured on the sky. `RegPins` builds its larger and smaller Ellipses with a port of C's form-1 constructor, and the bounding box's singularity probe and the mesh use the same axes | `tests/test_region_ellipse_oracle.rs`: 3115 points against two Ellipses read from C's dumps, from `tools/oracle_fixtures/region_ellipse_probe.c` |
| PG-1 | FIXED 2026-10-08: a Polygon's boundary mesh is polygon.c `RegBaseMesh`: each edge's length is the base Frame's distance, an edge gets `1 + (int)(len*mp)` points, starting at its normalised first vertex and stepped along it with the Frame's `offset`, so a sky Polygon's mesh follows great circles; a boundary of no length is AST__BADIN | `tests/test_region_polygon_oracle.rs`: 3 meshes bit-exact against C from `tools/oracle_fixtures/region_polygon_probe.c`; the sky Polygon's disc in `tests/test_region_disc_oracle.rs` |
| CR-1, CR-2, CR-3 | FIXED 2026-10-08: the CmpRegion builder is `astInitCmpRegion`: the second Region is mapped into the first's Frame when a Mapping joins them (AST__INTRD when none does), and `CmpOperator::Xor` is made, as C makes it, as the OR of each Region ANDed with the negation of the other, keeping the two Regions XORed; the dump writes those with Operator 3, and the loader rebuilds the XOR from them. The CmpRegion's mesh is cmpregion.c `RegBaseMesh`: each component's current-Frame mesh, or region.c `BndMesh` for a component with no finite boundary, kept where it lies inside (AND) or outside (OR) the other component, with C's AST__INTER where no mesh can be made | `tests/test_region_cmpregion_oracle.rs`: an FK5 Circle ANDed with a Galactic Box (membership and a 200-point mesh, bit-exact) and an XOR of two Boxes (membership, dump byte-exact, and reading C's dump), from `tools/oracle_fixtures/region_cmpregion_probe.c` |
| EL-2 | FIXED 2026-10-08: `EllipseBuilder` takes the arguments of `astEllipse` form 1: the angle from the second axis to the primary axis, positive towards the first, and the axis ends placed with the Frame's `offset2`, so a sky Ellipse's axes are arcs. It measured the angle from the first axis and placed the ends on a plane; tests that built Ellipses take their angles in C's convention | `tests/test_region_ellipse_oracle.rs` (`builder_matches_astellipse_form1`): a sky and a Cartesian Ellipse built by `EllipseBuilder` dump exactly as C's |
| RB-9, RB-10, BX-4, CI-1 | FIXED 2026-10-08: `set_uncertainty` returns `AstResult`. A shape that cannot be re-centred, or a Frame that cannot be converted to the Region's base Frame, is AST__NCPIN, the status C's second `astError` leaves, and C's first message is the error's message. Either way the Region is left with no uncertainty, as C annuls the old one first. `set_fill_factor` is AST__ATSER outside 0 to 1 and leaves the attribute unchanged, and `reg_overlay`, which C makes through `astSetFillFactor`, passes that on. The loader rejects an out-of-range `Fill` the same way and raises a `MeshSz` below 5 to 5, where a negative one became a huge mesh. `BoxRegionBuilder::new` and `from_bounds` reject a BAD (or NaN) coordinate in either supplied point as AST__BADIN, and `CircleBuilder::build` does so for the centre and the circumference point, which a BAD radius makes BAD. Still open: the parallel merges (`region_merge` and `finish_region_merge`) return `Option`, so an uncertainty or FillFactor error inside one declines the merge where C's status would end the simplification; a failing uncertainty in a parallel `CmpMap` of Boxes would expose it | `tests/test_region_errors_oracle.rs`: 8 cases against C's status codes and repaired values from `tools/oracle_fixtures/region_errors_probe.c` |
| PR-1, PL-3, IN-6, PL-2 | FIXED 2026-10-08: a PointList is bounded as region.c `GetBounded` decides, so a negated PointList is unbounded unless its Frame is a SkyFrame. A negated Prism is bounded only if both components are bounded once negated, as prism.c `GetBounded` tests them. An Interval's `reg_base_box` is its limits as stored, so an excluded axis keeps its lower limit above its upper where it was widened to the whole axis. A PointList's `reg_base_box` is pointlist.c `RegBaseBox`: the least and greatest `ax_distance` from the first point, turned back into axis values with `ax_offset`, so a sky PointList either side of RA 0 has a box that wraps, and a default uncertainty about 600 times smaller than the raw minimum and maximum gave | `tests/test_region_bounded_oracle.rs`: bounded flags and bounding boxes against C from `tools/oracle_fixtures/region_bounded_probe.c` |
| CR-4, PR-4 | FIXED 2026-10-08: setting or clearing Closed or MeshSize on a CmpRegion or Prism sets or clears it on both components too, as their `MAKE_SET` and `MAKE_CLEAR` accessors do. The Region trait gains `clear_closed`, C's `astClearClosed`, which it lacked. An unset FillFactor is the first component's for a CmpRegion and the product of the components' for a Prism. The CmpRegion builder copies the first component's set MeshSize and Closed onto the CmpRegion, and so onto both components, as `astInitCmpRegion` does; `astInitPrism` copies nothing | `tests/test_region_compound_attr_oracle.rs`: a CmpRegion and a Prism dumped byte-exact against C as made, after setting and after clearing, and their FillFactors, from `tools/oracle_fixtures/region_compound_attr_probe.c` |
| RB-11, RB-20 | FIXED 2026-10-08: C's protected `astMapRegion` is `reg_map_region`, hidden like the other protected Region methods, and the port's algorithms call it. The public `map_region` is `astMapRegionId_`: the same, simplified, so a Box mapped through a PermMap comes back as a Box in the new Frame. `reg_map_region` drops axes only when the split it makes keeps as many axes as are wanted, so a Region with a BAD defining point, which has no flags saying which axes give good values, keeps every axis as C's does. Matching C's dump of that case exposed a simplifier divergence: the PermMap `MapMerge` test of whether an array was already null (`NullPerm`) compared the public encoding, where C stores a bad entry as the number of coordinates on the other side, so a bad entry at that index counts as null; a 3-in, 2-out PermMap with inperm (1, 2, 0) now simplifies to itself as in C | `tests/test_region_mapregion_oracle.rs`: both forms and the PermMap against C's dumps from `tools/oracle_fixtures/region_mapregion_probe.c` |
| EL-3 | FIXED 2026-10-09: `Ellipse::ellipse_pars` is `astEllipsePars`: the centre and axis ends in the current Frame, the half-lengths and the angle from ellipse.c `CalcPars` in that Frame, with the second point replaced by the end of the secondary axis as `CalcPars` replaces it, and BAD half-lengths and angle when either half-length is not positive | `tests/test_region_ellipsepars_oracle.rs`: a Cartesian and a sky Ellipse, a form-0 Ellipse and an Ellipse mapped through a ZoomMap, exact against C from `tools/oracle_fixtures/region_ellipsepars_probe.c` |
| PG-3 | FIXED 2026-10-09: `Polygon::convex` is `astConvex<X>`, generic over the pixel type, with every comparison as `PixelOper` and the `starpix` convention: a port of polygon.c `ConvexHull`, `FindBoxEdge` and `PartHull`, with C's vertex order and repeated corner vertices, a PIXEL Frame, no `EnsureInside`, and `None` when no pixel is selected; `convex_hull_f32` is removed | `tests/test_region_convex_oracle.rs`: all six comparisons, `starpix`, an int array, one pixel and none, exact against C dumps from `tools/oracle_fixtures/region_convex_probe.c`; `test_regions.rs` `convex_hull_basic` checks checkConvex's vertex order |
| PG-2 | FIXED 2026-10-09: `Polygon::downsize` is `astDownsize`, a port of polygon.c `DownsizePoly` with `FindMax` (plane geometry in a plain Frame, `resolve_points` otherwise) and the Segment chain ordered by error; `Polygon::outline` is `astOutline<X>`, a port of `FindInsidePoint`'s spiral search, the rightward search that crosses holes, `TraceEdge`, `SmoothPoly` and the box-size rules, in a PIXEL Frame; both take any `PixelValue` type, whose C type code names the function in error messages | `tests/test_region_outline_oracle.rs`: downsizing by error, vertex count, both and neither, and on the sky; outlines for every comparison, smoothed and downsized, `starpix`, across a hole, by spiral search and for an int array, exact against C dumps from `tools/oracle_fixtures/region_outline_probe.c`, and the `AST__NONIN` message |
| PG-6, CR-7, PG-4, EL-4, IN-5, NR-3 | FIXED 2026-10-09: `Region::is_bounded` and `RegionGeometry::is_bounded_inner` return `AstResult<bool>`, as C's GetBounded can raise, so a Polygon's geometry error and a CmpRegion's overlap error propagate instead of reading as bounded and unbounded; CmpRegion's `is_bounded_inner` is its `get_bounded`. The Polygon constructor reports a non-2-D Frame and a BAD vertex as `AST__BADIN` with C's messages, checking every X before any Y, no vertices as `AST__NPTIN`, and accepts one or two vertices. The Ellipse constructor checks the axis count and BAD centre, axis and angle values with C's codes and messages, accepts negative axes, and leaves points that do not determine an ellipse to be rejected on first use, as ellipse.c `Cache` rejects them, which also covers a loaded Ellipse. The PointList constructor reports a point of the wrong length as `AST__NCPIN` and no points as `AST__NPTIN`, with C's messages. C's Interval `RegCentre` query returned the base Frame centre whatever Frame was asked for, and C's NullRegion `GetDefUnc` built the uncertainty in the current Frame; both fixed upstream (93803141, 74e0e51c), and the port already behaved as the fixes do | `tests/test_region_errors_oracle.rs`: the Polygon and Ellipse cases against C codes from `tools/oracle_fixtures/region_errors_probe.c`, with C's messages; `tests/test_interval.rs` and `tests/test_nullregion.rs` port the new testregions checks |
| NR-2, RB-14 | FIXED 2026-10-09: a NullRegion's `transform` is nullregion.c `Transform`: the input copied unchanged when negated, as the inherited unit Transform copies it, BAD axes and all and whatever the FrameSet, and every value BAD otherwise; `point_in_region` is a point left good. Negated, Adaptive and RegionFS carry an explicitly-set state, with `test_negated`/`set_negated`/`clear_negated`, `test_adaptive`/`clear_adaptive` and `test_region_fs`/`clear_region_fs` beside the existing accessors: negating sets Negated, as `astNegate` does through `astSetNegated`; `reg_overlay` copies all three raw with their set state, as `astRegOverlay` copies the fields; the merges clear Negated or set it only when true, as C does; Load sets RegionFS and reads Negate and Adapt as set only when uncommented; and a copy clears RegionFS | `tests/test_region_nulltran_oracle.rs`: negated, mapped and un-negated NullRegions against C from `tools/oracle_fixtures/region_nulltran_probe.c`; `tests/test_region_attr_test_oracle.rs`: 11 states of a Box against C from `tools/oracle_fixtures/region_attr_test_probe.c` |
| (Adapt dump) | FIXED 2026-10-09: C's Dump wrote Adapt as set only when non-zero, so a Region set to Adaptive=0 restored as Adaptive; fixed upstream (09ffc7e5) to write it as set when non-zero or set explicitly, which leaves every other dump unchanged, and the port's writer follows | `tests/test_region_attr_test_oracle.rs`: the restored set state, and the byte-exact dump of a Box set to Adaptive=0, against `tools/oracle_fixtures/region_attr_test_probe.c` |
| RB-15 (frame-less uncertainty) | FIXED 2026-10-09: C's Load left an uncertainty dumped without a Frame with its dummy Frame when the Region was read with a FrameSet; fixed upstream (e07c87ce), and the port's loader already gave it the base Frame | `tests/test_circle.rs` (`loaded_frameless_uncertainty_takes_the_base_frame`) ports testregions `checkLoadedUncFrame` |
| RB-15, RB-16, RB-17 | FIXED 2026-10-09: the loader reads `Frm` before `FrmSet`, as region.c `Load` does, and reports points whose length differs from the base Frame's axis count as `AST__REGIN` with C's messages; the dump tests the simplified base->current Mapping for a UnitMap, as `Dump` tests `astRegMapping`; and `set_uncertainty` converts the uncertainty's FrameSet to this Region's inverted FrameSet, as `Conv` does, so any Frame of the uncertainty may match, and maps it into the converted FrameSet's current Frame | `tests/test_region_errors_oracle.rs`: a Box dump with 3-D points and an uncertainty matched through its base Frame, the latter's dump byte-exact, against `tools/oracle_fixtures/region_errors_probe.c`; `tests/test_region_mapregion_oracle.rs`: a Circle through an identity chain dumps only its Frame, against `tools/oracle_fixtures/region_mapregion_probe.c` |
| CR-5, (XORCheck) | FIXED 2026-10-09: `CmpRegion::reg_cmp_region_list` is cmpregion.c `CmpRegionList`; `operator`, the dump and the list find an OR of two ANDs that is equivalent to an XOR as `XORCheck` does, so such a CmpRegion dumps as `Operator = 3` with the XORed Regions; and the CmpRegion and Prism builders clear RegionFS throughout their components, as the deep copies `astInitCmpRegion` and `astInitPrism` take clear it. C listed an XOR component of an OR CmpRegion as two Regions of the OR; fixed upstream (41c78908) and ported with the fix | `tests/test_region_cmplist_oracle.rs`: six CmpRegions' lists, every listed Region's dump byte-exact, and the dump of an OR of two ANDs equivalent to an XOR, against `tools/oracle_fixtures/region_cmplist_probe.c` |
| RB-18, (Box Simplify), (Region MapMerge) | FIXED 2026-10-09: `parent_region_simplify` and the Region `MapMerge` self-simplify step (`region_map_merge`) decide whether anything simplified with `simplify_reporting_change`, C's pointer test, instead of a conversion-based `equal`, which cannot see a Region re-expressed in its current Frame. The Box `Simplify` re-expresses a Box in its current Frame only as box.c `Simplify` does: the best-fit Box with `astRegPins` both ways, a Polygon only when no Box fits the forward test, and the Prism split only when nothing, the parent included, has simplified. The corner transform tried first and its `verify_remap_exact` check, which have no C analogue, are gone; with them gone, `cap_interval_01` failed until the `MapMerge` test was fixed, since the shortcut had been giving the right dump from a wrongly kept unsimplified uncertainty. `reg_centre` went with RB-4 | `tests/test_simplify_regression.rs`: the 498-case corpus, `cap_interval_01` among them; a C probe of that case (box.c and interval.c instrumented) gives the same sequence of Interval and Box simplifications as the port |
| RB-19 (ShowMesh) | FIXED 2026-10-09: `Region::show_mesh` is `astShowMesh`, writing to standard output the table `show_mesh_text` gives: the title between blank lines, the current-Frame boundary mesh normalised as one contiguous group, each axis value `<bad>`, formatted by the axis Format, or C's `%g`, followed by a tab, then `ENDMESH`. `RegTrace`, `RegSplit` and `BndMesh` stay GATED(Plot) | `tests/test_region_showmesh_oracle.rs`: 7 tables against C from `tools/oracle_fixtures/region_showmesh_probe.c`, among them a sky Box straddling RA 0, which fails with the normalisation made non-contiguous |
| (RegBaseGrid NULL) | NO CHANGE 2026-10-09: region.c `RegBaseGrid` returns NULL without an error for an unbounded Region where the port's `get_region_grid` returns AST__MBBNF. The NULL never reaches a caller as a result: the public `astGetRegionMesh` checks the Region is bounded first and reports AST__MBBNF, as `region_grid` does, and the protected callers (`RegGrid`, the Prism grid) pass it to `astClone` or `astTransform`, which report an error. Only which error an internal path reports differs. The AST__MBBNF message now names the class, as C's does | region.c `GetRegionMesh` (the bounded check before `astRegBaseGrid`) and `RegGrid`; `tests/test_interval.rs` (`mesh_of_an_unbounded_interval_names_the_class`), its message checked against the C library |
| PG-5 | DEFERRED 2026-10-09: a performance matter (the Polygon edge cache), left until the port turns to performance |
| (Interval Simplify) | FIXED 2026-10-09: an Interval that is not equivalent to a Box is re-defined in its current Frame as interval.c `Simplify` does. The port's test of the simplified Box looked at the type of the box holding it rather than the Region, so it never matched and the Interval was always returned unsimplified; it now asks whether the Box simplified (`sreg != box2`) and reads the limits from the Box or Interval that results. The UnitMap test that decides whether to try is made on the base->current Mapping as `astGetMapping` gives it, unsimplified, as C makes it | `tests/test_region_interval_remap_oracle.rs`: the public `map_region` of 3 Intervals with a missing limit, through a ZoomMap, a PermMap and a WinMap, dumps byte-exact against C from `tools/oracle_fixtures/region_interval_remap_probe.c` |
| (StcResourceProfile) | DONE 2026-10-09: `Stc<K>` is stc.c, with `StcResourceProfile` its first class. The encapsulated Region decides membership through a per-class `reg_base_mask` seam, which region.c `BTransform` also reaches, as C's Stc `Transform` does; Overlap, OverlapX, RegBaseBox, RegBaseMesh, RegBasePick, RegPins, GetBounded, GetDefUnc and SetRegFS forward to it as stc.c's do. Negated, Closed, MeshSize and FillFactor are read from it and set on both, and `raw_negated`/`raw_closed` give RegOverlay and Dump the Stc's own fields, as region.c reads them. `reg_frame_attrib` passes a setting, but not a clearing, on to the AstroCoords Regions, as stc.c `RegSetAttrib` and `RegClearAttrib` differ. C's GetRegion cancelled an Stc's negation; fixed upstream (f24ee843) and ported with the fix | `tests/test_region_stc_oracle.rs`: 6 Stcs (plain, negated, sky with an AstroCoords Error Region from another SkyFrame, mapped, made from a mapped one, and simplified), dumps, read-back dumps, transformed points and Bounded byte-exact against C from `tools/oracle_fixtures/region_stc_probe.c`, with overlap, `get_stc_coord` and the AST__STCIND and AST__STCKEY errors; `cap_timemap_02` in the simplify corpus, the transform oracle and the round-trip sweep |
| (Stc classes) | DONE 2026-10-09: `StcSearchLocation`, `StcCatalogEntryLocation` and `StcObsDataLocation` are `Stc<K>` with their class names and dump comments, as their C classes add nothing to stc.c but a name, and in `StcObsDataLocation` an observatory position: an optional PointList that `stc_set_obs` gives (C's protected `astStcSetObs`), written as `ObsLoc`, read back, and copied with the Stc. C crashed writing an `StcObsDataLocation` with no position; fixed upstream (a4f0566e) and ported with the fix | `tests/test_region_stc_oracle.rs`: one of each, and an `StcObsDataLocation` with and without a position, dumps, read-back dumps, transformed points and Bounded byte-exact against C from `tools/oracle_fixtures/region_stc_probe.c` |
| (Moc, core) | DONE 2026-10-09: `Moc` is moc.c's class core: cells kept as ranges of nested indices at MaxOrder, combined by `add_cell` (AND, OR, XOR) through `MergeRanges`, `NegateRanges` and `CombineRanges`; the normalised NUNIQ form (`GetNorm`), `get_cell` and `test_cell`; the MaxOrder, MinOrder, MaxRes, MinRes, MocArea, MocLength and MocType attributes; membership through the HPX12 grid Mapping (`Transform`); the boundary mesh (`RegBaseMesh` with `MakeCorners`), `RegBaseBox`, `RegPins`, `GetDefUnc` and `Equal`; and its dump. `AddRegion` and `AddPixelMask<X>` (with `PutCell`, `IncorporateCells`, `FindSkyAxes`, `GetPixelArea`, `TestPixels`) landed in a second pass; C's `AddRegion` read out of bounds with MaxOrder 0 and its `IncorporateCells` negated at the oversampled order, both fixed upstream (28473801) and ported with the fixes. The MOC string, JSON and data forms and the FITS header (`AddMocString`, `astAddMocText`, `GetMocString`, `astGetMocText`, `AddMocData`, `GetMocData`, `GetMocHeader`) landed in a third pass; C's text reader read past its lists for a string MOC with no order, asked for about 2^64 bytes for a descending final range, narrowed orders before checking them, did not check NPIX values or digit overflow, rejected `{}` and left the Moc counting a range it did not hold after a parse error; its writer wrote an empty JSON MOC as `}`; and `AddMocData` shifted out of range for NUNIQ values below 4: all fixed upstream (7beab7b1, 7d06331e) and ported with the fixes. The forms were also cross-checked against the CDS `moc` crate (0.18) on 300 random MOCs in both directions; it reads only MOC 2.0 (space separated) strings and rejects overlapping cells, which AST merges. Not yet: and `RegTrace` with the perimeter trace of `RegBaseMesh`, which only Plot uses. C's `RegBaseBox` hung on an empty Moc, its `RegPins` mask flagged the wrong points, its mesh depended on `qsort`, and an empty Moc's `Transform` read out of bounds; fixed upstream (2b631e08, c95aa801) and ported with the fixes | `tests/test_region_moc_oracle.rs`: four Mocs (cells at several orders, one needing 8 byte values, one in Galactic, and an empty one), dumps, read-back dumps, attributes, cells, membership of a grid of positions plain and negated, meshes and bounds byte-exact against C from `tools/oracle_fixtures/region_moc_probe.c`, with `test_cell`, overlap with a Circle, the `RegPins` mask and the errors; `tests/test_region_moc_add_oracle.rs`: 21 Mocs built by `add_region` and `add_pixel_mask` (Circles, Polygon, negated, polar, Galactic, CmpFrame Box, oversampled, Moc into Moc, auto MaxOrder, masks of three types under each operator, bad values, swapped sky axes) byte-exact against `tools/oracle_fixtures/region_moc_add_probe.c`, with the NOSKY and INVAR errors; `tests/test_region_moc_text_oracle.rs`: 18 Mocs read from text and data, each compared by both text forms, MocType and data, the forms of an empty Moc, the pieces written through a small buffer, the FITS headers, and 21 malformed texts and 2 malformed data arrays against `tools/oracle_fixtures/region_moc_text_probe.c` |
| PL-1 | WITHDRAWN: a C probe masks the same two pixels as the port with a UnitMap; the Mapping direction is RB-12 | Appendix C |


## Appendix A: base class (region.c)

The base-class reader's report.
C-side claims marked UNVERIFIED were established by reading region.c.

### 1. vtab / attribute map

| C name (region.c:line) | Rust counterpart | Kind | Status |
|---|---|---|---|
| Negated Get/Set (attr 12533-12536) | `Region::is_negated/negate/negated_copy` (regions/mod.rs:163-169) | ceremony | PORTED (no set(bool)) |
| TestNegated/ClearNegated | `test_negated`/`clear_negated` | ceremony | PORTED (RB-14 fixed) |
| RegionFS Get/Set (12614-12620) | `region_fs_flag/set_region_fs_flag` (mod.rs:440-446, doc-hidden) | ceremony | PORTED |
| TestRegionFS/ClearRegionFS | `test_region_fs`/`clear_region_fs` | ceremony | PORTED (RB-14 fixed) |
| Closed Get/Set/Test (12773-12778) | `is_closed/set_closed/test_closed/set_closed_raw` (mod.rs:176-190) | ceremony | PORTED |
| ClearClosed | `clear_closed` | ceremony | PORTED (RB-14 fixed) |
| MeshSize G/S/T/C (12722-12725) | `mesh_size/set_mesh_size/test_mesh_size/clear_mesh_size` (mod.rs:211-220; data.rs:249-286) | ceremony | PORTED |
| Adaptive Get/Set (12489-12494) | `is_adaptive/set_adaptive` (mod.rs:226-229) | ceremony | DIVERGENT: has no effect (RB-3) |
| TestAdaptive/ClearAdaptive | `test_adaptive`/`clear_adaptive` | ceremony | PORTED (RB-14 fixed) |
| FillFactor G/S/T/C (12658-12666) | `fill_factor/set_fill_factor/test_fill_factor/clear_fill_factor` (mod.rs:196-205) | ceremony | PORTED (RB-10 fixed) |
| Bounded (read-only) / GetBounded (3364) | `is_bounded` per class + `RegionData::closed_curve_bounded` (data.rs:331) | architecture | PORTED (SkyFrame rule present) |
| ResetCache (10219) | `RegionData::invalidate_cache` (data.rs:295) | ceremony | PORTED |
| RegTrace (11605) | NONE | architecture | GATED(Plot) |
| TestUnc (4100) | `test_unc` (mod.rs:615) | ceremony | PORTED |
| ClearUnc (2755) | `store_uncertainty(None)` (mod.rs:430) | ceremony | PORTED |
| GetRegionFrame (4401) | `get_region_frame` per class | ceremony | PORTED |
| GetRegionFrameSet (4472) | `get_region_frameset` per class | ceremony | PORTED |
| MapRegion (5386) | `Region::reg_map_region` default; public `astMapRegion` is `Region::map_region` | architecture (not overridden in C) | PORTED (RB-11, RB-20 fixed) |
| Overlap (6923) | `Region::overlap` per class -> `overlap::compute_overlap` (overlap.rs:45) | architecture | DIVERGENT (RB-1, RB-6, RB-7) |
| OverlapX (7007) | `overlap::overlap_x` (overlap.rs:104), free fn; NullRegion special-cased centrally (overlap.rs:53) | architecture | DIVERGENT: no per-class seam (RB-7), error paths (RB-8) |
| Negate (6483) | `negate` per class | ceremony | PORTED |
| BndMesh (1790) | NONE | architecture | GATED(Plot) |
| BndBaseMesh (1689) | `overlap::bnd_base_mesh` (overlap.rs:395), free fn | architecture (Interval overrides) | DIVERGENT: no seam, stack overflow (RB-1) |
| RegBaseGrid (7971) | `data::region_base_grid` (data.rs:573) + Box override | architecture | PORTED (no cache) |
| RegBaseMesh (8232) | `RegionGeometry::reg_base_mesh` (data.rs:393) | architecture | PORTED (seam) |
| RegSplit (8161) | NONE | architecture | GATED(Plot) |
| RegBaseBox (7866) | `RegionGeometry::reg_base_box` / `region_base_box` (data.rs:388; mod.rs:265) | architecture | PORTED |
| RegBaseBox2 (7920) | `reg_base_box2` (mod.rs:274), CmpRegion override | architecture | PORTED |
| RegBasePick (8288) | `reg_base_pick_axes` (mod.rs:1017) | architecture | PORTED |
| RegCentre (8408) | `reg_centre_set` / `reg_centre_query` (mod.rs:786, 763) | architecture | PORTED |
| RegGrid (8656) | `reg_grid` (mod.rs:362) | ceremony | PORTED |
| RegMesh (8742) | `reg_mesh` (mod.rs:355) | architecture (NullRegion overrides) | PORTED |
| RegClearAttrib (8495) | NONE | architecture (CmpRegion/Prism/Stc override) | MISSING (RB-3) |
| RegSetAttrib (9934) | `with_region_integrity` non-adaptive branch (data.rs:473-497) | architecture (CmpRegion/Prism/Stc override) | STUB (RB-3) |
| GetDefUnc (3494) | `get_def_unc` / `default_bounding_box_unc` (mod.rs:638, 628) + 4 overrides | architecture | PORTED |
| GetNegation (3601) | `negated_copy` | ceremony | PORTED |
| GetUncFrm (3923) | `get_unc_frm` (mod.rs:685) | ceremony | PORTED |
| SetUnc (10765) | `set_uncertainty` (mod.rs:501) | ceremony | PORTED (RB-9 fixed) |
| GetUnc (3795, public) | `get_uncertainty` (base Frame, explicit only) + `get_unc_frm(Current)` | ceremony | PORTED in shape (no `def` flag) |
| ShowMesh (10924) | `Region::show_mesh`, `show_mesh_text` (mod.rs) | ceremony (debug) | PORTED (RB-19) |
| GetRegionBounds (8959) | `region_bounds_current` (mod.rs:1285) | ceremony | PORTED |
| GetRegionBounds2 (9150) | `get_region_bounds2` (mod.rs:285) | ceremony | PORTED |
| GetRegionMesh (9248) | `region_mesh` / `region_grid` (mod.rs:323, 286) | ceremony | PORTED |
| GetRegionPoints (9442) | `get_region_points` per class (base Frame) | ceremony | DIVERGENT (RB-13) |
| GetRegionDisc (9647) | required per-class `get_region_disc` (mod.rs:309) | ceremony (generic, never overridden in C) | DIVERGENT (RB-4) |
| PointInRegion (7761) | per-class `point_in_region` | ceremony | PORTED (duplicated per class) |
| RegOverlay (9872) | `reg_overlay` (mod.rs:1207) | ceremony | PORTED |
| RegFrame (4145) | `get_region_frame` | ceremony | PORTED |
| RegDummyFS (8825) | `has_dummy_fs` | ceremony | PORTED |
| RegMapping (4190) | `to_current_frame` (mod.rs:370) | ceremony | PORTED |
| RegPins (8890) | `reg_pins` / `reg_pins_mask` (mod.rs:726, 696) | architecture | PORTED (seam) |
| RegTransform (11936) | `to_current_frame`, `reg_tran_point` (mod.rs:759) | ceremony | PORTED |
| BTransform (1876) | `data::region_base_mask` (data.rs:550) | ceremony | PORTED |
| GetRegFS / SetRegFS (3654 / 10685) | `RegionData::set_region_fs` (data.rs:219) | architecture (many overrides) | PORTED |
| MaskB..MaskUS, MaskLD (5979 macro) | `masking::mask` free fn (masking.rs:42) | architecture (PointList overrides) | DIVERGENT (RB-12) |
| object Equal (2637) | per-class `Mapping::equal` (box_region.rs:1896 etc.) | architecture (subclasses chain to it) | DIVERGENT (RB-2) |
| object Get/Set/Test/ClearAttrib | NONE (by design) | ceremony | OUT-OF-SCOPE |
| object GetUseDefs (4036) | `region_frame_attrs!` use_defs (frames/frame_attrs.rs:1037-1063) | ceremony | PORTED |
| object Cast / GetObjSize / ManageLock / Delete | NONE | ceremony | OUT-OF-SCOPE |
| Copy (astClearRegionFS) | `copy_region` / `clear_region_fs_deep` (mod.rs:459, 428) | ceremony | PORTED |
| Dump (13003) | `write_region_base` (serial/mod.rs:662), `region_to_node_skeleton` (serial/mod.rs:1136) | ceremony | PORTED (RB-16 latent) |
| Load (13490-13620) | `read_region_base` (serial/mod.rs:927) | ceremony | DIVERGENT minor (RB-15) |
| mapping ReportPoints | per-class `report_points` | ceremony | PORTED |
| mapping RemoveRegions (10105) | `remove_regions` (mod.rs:1488) | ceremony | PORTED |
| mapping Simplify (11125) | `parent_region_simplify` (mod.rs:1079) | architecture | OUT-OF-SCOPE (see note in RB-18) |
| frame Get/Set/Test/Clear of Digits, Domain, Title, Label, Symbol, Unit, Format, Direction, Top, Bottom, Epoch, ObsLat/Lon/Alt, MatchEnd, Min/MaxAxes, Permute, PreserveAxes, ActiveUnit (MAKE_* macros, 330-830) | `region_frame_attrs!` -> `forward_frame_attrs!` to FrameSet (frame_attrs.rs:980) | ceremony | PORTED (always integrity, RB-3) |
| frame Get/Set/Test/Clear System, AlignSystem | NONE generic (only via `with_integrity` + downcast) | ceremony | PORTED-partial |
| frame Format, Unformat, Distance, Angle, Offset, Norm, NormalPoints, Overlay, GetAxis, GetNaxes, PermAxes, Match, ValidateAxis | per class / macro (box_region.rs:1569-1656) | Match/GetAxis/Overlay architecture | PORTED |
| frame Offset2, Resolve, ResolvePoints, Intersect, LineDef/Contains/Crossing/Offset, NormBox, SubFrame, PickAxes, SystemString/SystemCode, AxAngle (region.c:4724-4804) | NONE: Frame-trait Cartesian/BasicFrame defaults | SubFrame/PickAxes architecture | DIVERGENT (RB-5) |
| frame Abbrev, Gap, Centre, CheckPerm, FindFrame, GetPerm, MatchAxes(X), IsUnitFrame, ValidateSystem, SetAxis, ConvertX/Convert | not individually checked (Gap/Centre/Abbrev are Plot-axis helpers) | — | GATED(Plot) for Gap/Centre/Abbrev; rest not audited |

### 2. Findings

#### RB-1 HIGH — Overlap recurses without bound (stack overflow) when one Region has no finite mesh
- C: `OverlapX` calls `astRegBaseMesh(reg1)` with errors suppressed; if that fails it calls the **vtab** `astBndBaseMesh(reg1, …)` (region.c:7333-7349). Interval overrides `BndBaseMesh` (interval.c, vtab line 822) and builds the mesh directly from its limits, with no `astOverlap` call; Interval's `RegBaseMesh` raises AST__INTER for an unbounded Interval, which is what routes C there. The base `BndBaseMesh` (region.c:1693-1740) returns a one-point all-BAD PointSet, not NULL, when the box does not overlap, so OverlapX still produces a code.
- Rust: `bnd_base_mesh` is a free function with no per-class seam (overlap.rs:395-419). It calls `reg1.overlap(box)`, and `Interval::overlap` (interval.rs:1042-1101) falls back to `compute_overlap`, which re-enters `overlap_x` and `bnd_base_mesh` with a box twice the size. That recursion never terminates. It also returns `None`, which is turned into AST__INTER (overlap.rs:333-339), where C returns one bad point.
- Probe P7: an Interval `x>=0` (unbounded) against a Box at x in [-10,-9]. Both `box.overlap(&interval)` and `interval.overlap(&box)` abort with a stack overflow. The lldb backtrace shows 98 cycles of `Interval::overlap -> compute_overlap -> overlap_x -> bnd_base_mesh`.
- Exposure: any overlap involving a partially unbounded Interval where the bounded Region does not reach the Interval's boundary. The C oracle should return 1 (UNVERIFIED).

#### RB-2 HIGH — Region Equal ignores the base Frame and the base->current Mapping, so Overlap reports Same for different Regions
- C `Equal` (region.c:2641-2704) requires the same class, equal PointSets, an equal **base** Frame, an equal current Frame, an equal **base->current Mapping**, and equal `astGetNegated` and `astGetClosed` (through the vtab).
- Rust: every class copies the same body: raw `negated`, raw `closed`, points `==`, and the current Frame only. See box_region.rs:1896-1904, circle.rs:163-172, ellipse.rs:159, polygon.rs:184, interval.rs:146, pointlist.rs:136, nullregion.rs:153, cmpregion.rs:253, prism.rs:393. `Frame::equal` on a Region compares the current Frame only (box_region.rs:1655).
- Probe P9: `b = a.map_region(ZoomMap(2), sameFrame)` gives `a.equal(b) == true` and `a.overlap(b) == Same`. However `b.bounds = [0,2]²`, `b` contains (1.5,1.5) and `a` does not. C would fail `Equal` and return 2.
- Raw `closed` also differs from C for PointList, whose GetClosed override (pointlist.c:380) is honoured in C's comparison.
- Exposure: an overlap test, or a simplify cancellation, between a Region and a remapped copy of it.

#### RB-3 HIGH — the `Adaptive` attribute has no effect, and RegSetAttrib/RegClearAttrib are absent
- C public `astSet`/`astClear` on a Region use `astSet(this->frameset, …)` (remap) when Adaptive is set, else `astRegSetAttrib`/`astRegClearAttrib` (region.c:10616-10625, 2308-2317).
  - `RegSetAttrib` (9934-10095) sets the attribute on the current Frame without remapping. It also sets it on the **base** Frame, translating an axis index via `astMapSplit`, and on the **uncertainty** Region (recursively), then resets the cache.
  - CmpRegion and Prism override it to reach their components (cmpregion.c:1280-1283, prism.c:1403-1407).
- Rust public Frame setters on a Region (`set_unit`, `set_epoch_mjd`, `set_label`, …) are generated by `region_frame_attrs!` (frames/frame_attrs.rs:980-984). They forward to `FrameSet`'s Frame impl, which always runs `with_integrity` (frames/frameset.rs:1223-1265), so Adaptive is never consulted.
  - Probe P1: a Box [0,1]² in metres with ActiveUnit and Adaptive=false, then `set_unit(1,"km")`. The result is identical to Adaptive=true: (0.5,0.5) is outside, base unit stays "m", current unit is "km". C's RegSetAttrib would give both Frames "km" with a UnitMap, so (0.5,0.5) would be inside.
- The only Adaptive-aware path is the inherent `with_integrity` (for example box_region.rs:204) via `with_region_integrity`. Its non-adaptive branch (data.rs:473-497) applies the closure to the current Frame only. The base Frame, the uncertainty and the axis translation are left alone, and the code comments say so.
  - Probe P2: the base label stays "Axis 1" while the current label is "NEWLABEL".
- There is no trait seam for per-class RegSetAttrib/RegClearAttrib.
- Mitigating note: C's protected typed setters (the MAKE_SET macros, 633-705) also always remap. The divergence is against C's public attribute interface, which is what the Rust typed setters stand in for.
- Old-audit status: still OPEN. `region-class-coverage-audit.md` §"Confirmed present and matching C" is wrong to list RegSetAttrib/RegClearAttrib.

#### RB-4 HIGH — GetRegionDisc: per-class inventions in the base Frame
- C: one generic implementation that no class overrides (only region.c, moc.c and fregion.c mention it).
  - It reports AST__INVAR unless the Region is 2-D, and temporarily clears Negated.
  - It returns radius AST__BAD for an unbounded Region.
  - Otherwise it meshes the boundary in the **current** Frame (`astRegMesh`), builds a local frame with `astResolve`/`astAngle`/`astOffset2`, takes the max `astDistance` from that centre, and multiplies by 1.000001 (region.c:9651-9866).
- Rust: required per-class `get_region_disc` (mod.rs:309), with these implementations:
  - Box/Interval: the half-diagonal of the base box (box_region.rs:1216, interval.rs:1308).
  - Circle: base centre plus radius (circle.rs:701).
  - Ellipse: `max(a,b)` (ellipse.rs:843).
  - Polygon: Euclidean distance from the vertex centroid (polygon.rs:1003).
  - PointList: distance from the centroid (pointlist.rs:488).
  - CmpRegion/Prism: from the base box, with AST__REGCN errors (cmpregion.rs:856, prism.rs:697).
  - NullRegion: AST__REGCN (nullregion.rs:262).
  - None of them checks for 2-D, uses the current Frame, returns BAD for an unbounded Region, or applies the 1.000001 factor. Polygon and Box ignore sky geometry.
- Probe P6: a Box [0,2]² mapped by ZoomMap(10) returns `([1,1], 1.414)`. Its current-Frame bounds are [0,20]², so C would give a centre near (10,10) and a radius near 14.14.
- Also used internally by `Prism::reg_centre` (prism.rs:373) and `CmpRegion` (cmpregion.rs:583). Both take component **base** coordinates where the parent's base Frame is the components' current Frame.

#### RB-5 HIGH — Region does not forward the Frame geometry and architecture methods; it falls back to Cartesian/BasicFrame defaults
- C region.c overrides these to act on the current Frame of the FrameSet (vtab 4720-4800): Offset2, Resolve, ResolvePoints, Intersect, LineDef, LineContains, LineCrossing, LineOffset, NormBox, SubFrame, PickAxes, SystemString, SystemCode, AxAngle.
- Rust: no Region class defines any of `offset2`, `resolve`, `resolve_points`, `intersect`, `line_*`, `norm_box`, `sub_frame`, `pick_axes_snapshot` or `system_string` (checked by grep over regions/). They therefore get the `Frame` trait defaults, which are planar.
- Probe P8 (Circle on an ICRS SkyFrame):
  - `offset2`: the Region gives (1.42, 1.47) where the SkyFrame gives (2.38, 1.15).
  - `resolve`: the Region gives d1=0.2 where the SkyFrame gives 0.076.
  - `pick_axes_snapshot` returns a BasicFrame, not a SkyFrame.
  - `system_string()` returns `None` (probe P9 on a Box).
  - `SkyFrame(Galactic).convert_to(&circle)` returns `None` although `circle.convert_to(&galactic)` succeeds. The likely cause is the default `sub_frame` returning `None`; SubFrame is on CLAUDE.md's architecture list. Cause UNVERIFIED.
- Exposure: any geometry call on a sky Region, or `astConvert(frame, region)`.

#### RB-6 MED — Overlap code 0 ("cannot compare") is collapsed into `Overlap::None` ("no overlap")
- overlap.rs:21-30 maps code 0 to `None`, and there is no enum variant for 0. C returns 0 when `astConvert` fails (region.c:7092-7093).
- Probe P3: Boxes in Domain AAA and Domain BBB give `Ok(None)`.
- Effect: a caller cannot tell "disjoint" from "incomparable". Any C-ported logic that treats 1 as a positive disjointness result (for example the CmpRegion simplify tables) is fed a false 1.

#### RB-7 MED — no OverlapX dispatch seam; NullRegion is special-cased centrally
- C: `Overlap` (region.c:6927) is a vtab method dispatched on `this`'s class. It calls the vtab `astOverlapX(that, this)`, dispatched on `that`'s class. Overrides: NullRegion Overlap+OverlapX (nullregion.c:317-318), Prism Overlap+OverlapX (prism.c:1420, 1396; 1663, 1919), Interval Overlap (interval.c:801).
- Rust has no `overlap_x` trait method. `compute_overlap` (overlap.rs:45-59) hard-codes `is::<NullRegion>()` on either operand and otherwise runs the generic `overlap_x`. `Prism::overlap` (prism.rs:339) just calls `compute_overlap`, so Prism's own algorithm is unreachable whether the Prism is `this` or `that`. Interval's override is reachable only as `this`, which matches C.
- Exposure: Prism-vs-Region overlap fixtures against the C oracle. The per-class Prism port is for the other auditor, but the missing seam is a base-class gap.

#### RB-8 MED — OverlapX error and fallback paths differ (UNVERIFIED)
- Mesh errors: C suppresses errors from `astRegMesh(reg2)` and treats them as "swap and retry" (region.c:7107-7124). It does the same for `astRegBaseMesh(reg1)` and falls through to BndBaseMesh (7316-7320). Rust uses `?` in both places (overlap.rs:165, 318), so a class whose mesh method errors makes overlap fail instead of swapping or falling back.
- No good points on the second attempt: C annuls the mesh and reports "Neither … has a finite boundary" (region.c:7159-7190). Rust breaks out of the loop and continues with an all-BAD `ps1` (overlap.rs:218-226).
- Settle with a C-oracle probe on a Region pair where the second attempt yields only bad points (for example, a mapping that sends the mesh entirely bad).

#### RB-9 MED — `set_uncertainty` drops errors silently
- C `SetUnc` reports AST__BADIN when the shape is not re-centrable (region.c:10913-10916). It reports AST__BADIN plus AST__NCPIN when the Frames cannot be converted (10897-10904).
- Rust `set_uncertainty` returns `()`: in both cases it removes the existing uncertainty and returns (mod.rs:512-572). A user who passes a Polygon or PointList uncertainty gets no error and silently loses the uncertainty. Every builder routes through it.
- Exposure: `set_uncertainty(PointList)`, then check `test_unc()`.

#### RB-10 MED — FillFactor range check and Load validation are missing
- `set_fill_factor` (data.rs:277) stores any value. C reports AST__ATSER outside 0..1 (region.c:12669-12673).
- `read_region_base` reads `Fill` and `MeshSz` raw (serial/mod.rs:940-947). C passes them through `SetFillFactor` and `SetMeshSize`, which clamp MeshSz to at least 5 (region.c:12730, Load 13520-13528). So `MeshSz = 3` loads as 3 and re-dumps as 3, where C gives 5.

#### RB-11 MED — MapRegion axis reduction runs when `axflags` is absent (UNVERIFIED)
- C counts `ntotal` only when `axflags` exists. With no `axflags` (defining points contain BAD values, or there are no points), `ntotal = 0 != nkept` and neither branch runs, so the Region is not axis-reduced (region.c:5616-5704).
- Rust takes the reduction branch when `axflags.is_none()` (mod.rs:956).
- Exposure: map an Interval with an unbounded (BAD) limit, or a Prism/CmpRegion/NullRegion, through a Mapping with fewer outputs (for example a PermMap that drops an axis). Then compare the dump with C.

#### RB-12 MED — Mask: the Mapping runs the opposite way to C, and there is no per-class seam
- (a) C `Mask<X>` requires `map` to go from Region Frame to grid: `nin == naxes(region)`, `nout == ndim`, then `astMapRegion(this, map, gridframe)` (region.c:5983-6044). Rust `mask` takes grid to Region (masking.rs:24-25, check at 64). The same Mapping object therefore gives a different mask, or a dimension error. `map` is also optional in C but required in Rust.
- (b) PointList overrides every MaskX in C (pointlist.c:641-652) to stamp the listed points. Rust `mask` is a free function using `point_in_region` per pixel centre, so a PointList masks almost nothing.
- (c) The error codes are AST__NGDIN/AST__GBDIN in C but `Dimin` in Rust.
- Exposure: the C ast_tester mask tests (testregions.c), run with the same Mapping.

#### RB-13 MED — `get_region_points` returns base-Frame points
- C public `astGetRegionPoints` transforms `this->points` through the FrameSet into the **current** Frame (region.c:9446-9504).
- Rust `get_region_points` returns the raw base points in every class (for example box_region.rs:1268). The doc comment says "base frame", and there is no current-Frame equivalent.
- Same pattern for `get_region_mesh`, which is base-Frame, although `region_mesh` exists for the current Frame.
- Exposure: any remapped Region.

#### RB-14 LOW — Missing typed accessors
- No `test_negated`/`clear_negated`/`set_negated(bool)`, `test_adaptive`/`clear_adaptive`, `clear_closed`, or `test`/`clear` for RegionFS.
- `reg_overlay` forces Adaptive to "set" (mod.rs:1218), whereas C copies the raw field including the unset sentinel. This is not visible in dumps, because Dump writes Negate and Adapt based on value (region.c:13065, 13088).

#### RB-15 LOW — Load differences
- No AST__REGIN check that `naxpt == nax` (region.c:13612-13619).
- Rust checks FrmSet before Frm (serial/mod.rs:987 vs region.c:13574).
- A frame-less uncertainty is re-associated with the base Frame even when the parent has a FrmSet (serial/mod.rs:1049-1057). C does this only through `astSetRegFS` in the Frm branch (region.c:13578-13580, 10745), so in C such an uncertainty keeps its dummy BasicFrame. UNVERIFIED; settle by loading a mapped Circle with a frame-less Unc in C and inspecting `astGetUncFrm`.

#### RB-16 LOW (latent) — Dump tests for a UnitMap on the unsimplified Mapping
- serial/mod.rs:717-721 tests `is::<UnitMap>()` on `frameset.mapping()`. That builds a `CmpMap` chain without simplifying (frames/frameset.rs:1132-1141). C uses `astRegMapping`, which is simplified (region.c:13108, 4247-4250).
- Probe P4 with ZoomMap(1) still wrote `Frm`, because the edge had already been simplified, so this is not exposed today. A FrameSet with a multi-edge base->current path would dump `FrmSet` where C dumps `Frm`.

#### RB-17 LOW — SetUnc's Conv aligns only the uncertainty's current Frame
- mod.rs:528-532 uses `unc.get_region_frame().convert_to(target, [domain])`.
- C `Conv` (region.c:2325-2382) runs `astConvert` FrameSet-to-FrameSet, which can match any Frame in the uncertainty's FrameSet. The two agree whenever the current Frame matches.

#### RB-18 LOW — Helpers with no C analogue
- `RegionGeometry::reg_centre` (data.rs:396) is a centroid used only by the invented `get_region_disc`s.
- `RegionGeometry::verify_remap_exact` (data.rs:422) is an interior-membership check used by Simplify (out of scope).
- `parent_region_simplify` detects a FrameSet change with conversion-based `Mapping::equal` (mod.rs:1091) where C tests pointer identity (11140). It uses `simplify_reporting_change` for the uncertainty, two lines later. Noted for the Simplify owners.
- Found while fixing it: the Region `MapMerge` self-simplify step (`region_map_merge`) made the same conversion-based test where C compares pointers, so a Region re-expressed in its current Frame was discarded as unchanged.

#### RB-19 LOW — Not ported
- `ShowMesh` (debug) is missing.
- `RegTrace`, `RegSplit` and `BndMesh` are GATED(Plot).

#### RB-20 MED — the public `map_region` does not simplify its result
- C's public `astMapRegion` (`astMapRegionId_`, region.c) simplifies the Region the protected `astMapRegion` returns; a Box mapped through a PermMap comes back as a Box in the new Frame with its points permuted.
- Rust has one `Region::map_region`, the protected form, which the port's own algorithms rely on not simplifying, so a caller of the public API gets the unsimplified Region C never returns.
- Found while writing `tests/test_region_attrib_oracle.rs`. Exposure: dump the result of `map_region` through a PermMap and compare with C's.

#### RB-21 LOW — `PermAxes` on a Region leaves an unsimplified base->current Mapping
- After `astPermAxes` on a Box, C's FrameSet links base and current Frames by a simplified PermMap; the port's `reg_perm_axes` leaves `CmpMap(UnitMap, PermMap)`, which dumps differently though it transforms the same.
- Found while writing `tests/test_region_attrib_oracle.rs`, whose permuted case reads C's dump of the permuted Box for that reason. Exposure: dump a Box after `reg_perm_axes(&[2, 1])`.

#### Dump against overridden getters: no defect (with a caveat)
- `write_region_base` reads the raw `RegionData` fields: `negated`, `fill_factor_set`/`fill_factor`, `mesh_size_set`/`mesh_size`, `closed_set`/`closed`, `adaptive` (serial/mod.rs:667-713). That matches C's private `TestX`/`GetX`. The trait getters remain per-class and overridable (for example PointList `is_closed`, pointlist.rs:410), so the two stay distinct.
- Caveat 1: per-class `Mapping::equal` uses the raw `closed` where C uses `astGetClosed` (in RB-2).
- Caveat 2: CmpRegion and Prism override GetFillFactor, SetClosed, SetMeshSize, ClearClosed and ClearMeshSize in C (cmpregion.c:1264-1277, prism.c:1385-1398). The Rust impls are plain field writes (cmpregion.rs:745, prism.rs:545). The seam exists but the propagation is not ported; that belongs to the per-class auditor.

#### Transform direction
Fixed in `d250706f`; see Progress.

### 3. Status of findings from older audits (base class only)

| Old finding | Status at HEAD | Evidence |
|---|---|---|
| R-PINS: `reg_pins` was an interior test | FIXED | `reg_pins_mask(pts, unc)` seam (mod.rs:742); per-class boundary and uncertainty ports (box_region.rs:1036, circle.rs:534, …) |
| R-OVL: 90% threshold, no astConvert, no code 6 | FIXED, with residuals RB-1, RB-6, RB-7, RB-8 | overlap.rs:104-378 (astConvert at 129, code 6 at 121, touch/Closed rule at 378) |
| MapRegion drops uncertainty / no axis reduction | FIXED (RB-11 too) | mod.rs:835-998 (`copy_region` keeps unc; split/axflags 880-935) |
| RegBasePick default always None | FIXED | mod.rs:1017-1059 |
| RegOverlay: missing RegionFS, `unc` flag, else-clear | FIXED | mod.rs:1207-1233 |
| Parent Simplify uncertainty stability (SIMP-STAB) | FIXED | mod.rs:1079-1195 |
| GetUncFrm: no default, no Frame mapping | FIXED | mod.rs:685-703 |
| GetDefUnc absent (DEFUNC) | FIXED | mod.rs:638-676, plus overrides in interval/nullregion/prism/cmpregion |
| SetUnc a bare store | FIXED (RB-9) | mod.rs:501-605 |
| GetBounded lacks the SkyFrame rule (GBND-SKY) | FIXED | data.rs:331-337 |
| RegGrid / RegBaseGrid odometer heuristic (GRID-FRM) | FIXED (3-try refinement); no cache, which is a performance matter only | data.rs:573-623 |
| RegBaseBox2 absent | FIXED | mod.rs:274, CmpRegion override |
| GetRegionBounds returned the base box | FIXED | mod.rs:1285-1375, used by all classes except NullRegion and Prism (their own C overrides) |
| RegCentre get-only | FIXED | mod.rs:786-811 |
| astConvert alignment absent in overlap | FIXED | overlap.rs:119-125 |
| RegSetAttrib STUB (non-adaptive `with_region_integrity`) | **OPEN**, and wider than recorded (RB-3) | data.rs:473-497; frame_attrs.rs:980 |
| Mask: zero-volume / PointList | **OPEN**, plus the Mapping-direction issue (RB-12) | masking.rs:42 |
| BND-INT: Interval BndBaseMesh missing ("reported") | **OPEN, now HIGH**: stack overflow (RB-1) | overlap.rs:395 |
| RT-TRACE, REG-SPLIT | OPEN, GATED(Plot) | — |
| Coverage audit: "RegSetAttrib/RegClearAttrib confirmed matching C" | **WRONG** | see RB-3 |

## Appendix B: the nine classes

The class reader's report.
Its coverage tables, findings, and the status of the earlier audits' class findings follow.

### Coverage tables

#### Box (box.c vtab 1209-1257)
| C name | Rust | kind | status |
|---|---|---|---|
| BoxPoints (251, protected) | `get_region_points` (box_region.rs:1268) | ceremony | PORTED |
| GetObjSize (1078) | NONE | ceremony | OUT-OF-SCOPE |
| Transform (4082) + Cache (673) | `reg_base_inside` (813) via `region_transform` | architecture | DIVERGENT (BX-1, BX-2) |
| Simplify (3609) | `simplified` | architecture | OUT-OF-SCOPE |
| SetNegated/SetClosed/ClearNegated/ClearClosed/SetRegFS/SetUnc/ResetCache (3378-3607) | cache invalidation in `RegionData` | ceremony | PORTED |
| MapMerge (1428) | `region_merge` | architecture | OUT-OF-SCOPE |
| RegBaseGrid (2182) | `get_region_grid` (1257) | architecture | PORTED (GRID-FRM fixed); unbounded -> Err vs C NULL: no observable difference (see Progress) |
| RegBaseMesh (2280) | `reg_base_mesh` (852) | architecture | DIVERGENT (BX-3) |
| RegBasePick (2658) | `reg_base_pick_axes` (1359) | architecture | PORTED |
| RegBaseBox (2109) | `reg_base_box` (848) -> `bounds()` (551) | architecture | DIVERGENT (BX-1) |
| RegPins (2955) | `reg_pins_mask` (1040) | architecture | PORTED (extent per BX-1) |
| RegTrace (3219) | NONE | architecture | GATED(Plot) |
| RegCentre (2781) | `reg_centre_query/set` (1307/1317) | architecture | PORTED |
| astInitBox (4748) | `BoxRegionBuilder` (43-153) | ctor | PORTED (BX-4 fixed) |
| Dump (4415)/Load (4928) | serial/mod.rs:1171 | ceremony | PORTED (no class cards) |

#### Circle (circle.c vtab 714-737)
| C name | Rust | kind | status |
|---|---|---|---|
| CirclePars (479, public) | NONE | public API | MISSING (CI-2) |
| Transform (1939)/Cache (291)/CalcPars (390) | `reg_base_inside` (circle.rs:410), `radius()` (263) | architecture | PORTED |
| Simplify (1769) | `simplified` (1007) | architecture | OUT-OF-SCOPE |
| SetRegFS/ResetCache (1688/1723) | RegionData | ceremony | PORTED |
| RegPins (1289) | `reg_pins_mask` (538) | architecture | PORTED |
| RegTrace (1544) | NONE | architecture | GATED(Plot) |
| RegBaseMesh (850) | `base_mesh_bounds` (276) | architecture | PORTED |
| RegBaseBox (757) | `reg_base_box` (446) | architecture | PORTED |
| RegCentre (1114) | `reg_centre_query/set` (769/779) | architecture | PORTED |
| astInitCircle (2528) | `CircleBuilder` (110-226) | ctor | PORTED (CI-1 fixed) |
| Dump/Load (2226/2714) | serial/mod.rs:1155 | ceremony | PORTED |

#### Ellipse (ellipse.c vtab 543-566)
| C name | Rust | kind | status |
|---|---|---|---|
| EllipsePars (793, public) | `Ellipse::ellipse_pars` | public API | PORTED (EL-3 fixed) |
| Transform (2052)/Cache (586)/CalcPars (702) | `reg_base_inside` (ellipse.rs:477) + `ellipse_params` (435) | architecture | DIVERGENT (EL-1) |
| Simplify (1884) | `simplified` | architecture | OUT-OF-SCOPE |
| SetRegFS/ResetCache | RegionData | ceremony | PORTED |
| RegPins (1386) | `reg_pins_mask` (675) | architecture | DIVERGENT (EL-1: planar params) |
| RegBaseMesh (1005) | `reg_base_mesh` (582) | architecture | PORTED |
| RegBaseBox (938) | `reg_base_box` (513) | architecture | PORTED |
| RegCentre (1211) | `reg_centre_query/set` (910/920) | architecture | PORTED |
| RegTrace (1636) | NONE | architecture | GATED(Plot) |
| astInitEllipse (2668) | `EllipseBuilder` (238-364), `from_defining_points` (380) | ctor | DIVERGENT (EL-2, EL-4) |
| Dump/Load (2345) | serial/mod.rs:1187 | ceremony | PORTED (EL-4 fixed: validated on first use, as C validates) |

#### Polygon (polygon.c vtab 2897-2935)
| C name | Rust | kind | status |
|---|---|---|---|
| Downsize (1326, public astDownsize) | `Polygon::downsize` | public API | PORTED (PG-2 fixed) |
| astOutline<X>/astOutline8<X> (public) | `Polygon::outline` | public API | PORTED (PG-2 fixed) |
| astConvex<X>/astConvex8<X> (public) | `Polygon::convex` | public API | PORTED (PG-3 fixed) |
| Transform (6235) + Cache (575) | `base_inside` (436), `cache` (516) | architecture | PORTED (PG-5 perf) |
| Simplify (5291) | `simplified` | architecture | OUT-OF-SCOPE |
| SetRegFS/ResetCache | RegionData | ceremony | PORTED |
| Get/Set/Test/ClearAttrib (SimpVertices) | `simp_vertices` etc. (322-338) | ceremony / typed accessor | PORTED |
| RegPins (4562) | `reg_pins_mask` (836) | architecture | PORTED |
| RegBaseMesh (4331) | `reg_base_mesh` (712) | architecture | DIVERGENT (PG-1 = MESH-GEO) |
| RegBaseBox (4180) | `reg_base_box` (659) | architecture | PORTED |
| RegTrace (4820) | NONE | architecture | GATED(Plot) |
| GetBounded (2749) | `bounded_impl` (601) | architecture | PORTED (PG-6) |
| EnsureInside (1768), SkyArea (5469) | `ensure_inside` (622), `sky_area` (567) | internal | PORTED |
| astInitPolygon (7100) | `PolygonBuilder` (173-309) | ctor | PORTED (PG-4 fixed) |
| Dump (6638)/Load (SimpVT, Order) | serial/mod.rs:1203-1256 | ceremony | PORTED |

#### Interval (interval.c vtab 781-824)
| C name | Rust | kind | status |
|---|---|---|---|
| IntervalPoints (protected) | `get_region_points` (interval.rs:1356) | ceremony | PORTED |
| Transform (3542) + Cache (379) | `reg_base_inside` (828) | architecture | DIVERGENT (IN-1) |
| Simplify (3101) | `simplified` | architecture | OUT-OF-SCOPE |
| Overlap (1765) | `overlap` (1042) -> `interval_overlap` (218) | architecture | PORTED |
| SetRegFS/ResetCache/SetUnc (3060) | RegionData | ceremony | PORTED |
| MapMerge | `region_merge` | architecture | OUT-OF-SCOPE |
| RegCentre (2465) | `reg_centre_query/set` (1387/1402) | architecture | PORTED (IN-5: C fixed upstream) |
| GetBounded (537) | `is_bounded_inner` (956) | architecture | DIVERGENT (IN-2) |
| GetDefUnc (592) | `get_def_unc` (1342) | architecture | PORTED |
| RegPins (2580) | `reg_pins_mask` (1054) | architecture | PORTED |
| RegTrace (2887) | NONE | architecture | GATED(Plot) |
| RegBaseMesh (2271) | `reg_base_mesh` (904) | architecture | DIVERGENT (IN-3) |
| BndBaseMesh (224) | NONE (generic `overlap.rs:395`) | architecture | MISSING (IN-4 = BND-INT) |
| RegBaseBox (2205) | `reg_base_box` (881) | architecture | PORTED (IN-6 fixed) |
| RegBasePick (2328) | `reg_base_pick_axes` (1436) | architecture | PORTED |
| ManageLock | NONE | ceremony | OUT-OF-SCOPE |
| Dump/Load (4042) | serial/mod.rs:1258 | ceremony | PORTED |

#### PointList (pointlist.c vtab 598-648)
| C name | Rust | kind | status |
|---|---|---|---|
| GetListSize (425) / ListSize attr | `list_size` (pointlist.rs:144) | typed accessor | PORTED |
| PointListPoints (protected) | `get_region_points` | ceremony | PORTED |
| Get/Set/Test/ClearAttrib (ListSize read-only) | n/a | ceremony | OUT-OF-SCOPE |
| Transform (2359) | `reg_base_inside` (162) | architecture | PORTED |
| Simplify (2161), MapMerge (972) | | architecture | OUT-OF-SCOPE |
| RegBaseMesh (1667) | `reg_base_mesh` (246) | architecture | PORTED |
| RegBaseBox (1532) | `reg_base_box` (222) | architecture | PORTED (PL-2 fixed) |
| RegBasePick | `reg_base_pick_axes` (566) | architecture | PORTED |
| RegPins (1868) | `reg_pins_mask` (338) | architecture | PORTED |
| GetClosed (376) | `is_closed` (414) | architecture | PORTED |
| GetBounded (inherited base) | `is_bounded_inner` (282) | architecture | PORTED (PL-3 fixed) |
| Mask<X>/MaskLD (797) | NONE (generic `masking::mask`) | architecture | MISSING (PL-1) |
| astInitPointList | `PointListBuilder` | ctor | PORTED (NCPIN and NPTIN as C) |

#### NullRegion (nullregion.c vtab 307-323)
| C name | Rust | kind | status |
|---|---|---|---|
| Transform (1410) | `reg_base_inside` (nullregion.rs:108) + `region_transform` | architecture | PORTED (NR-2 fixed) |
| Simplify (1314), MapMerge (338) | | architecture | OUT-OF-SCOPE |
| GetDefUnc (170) | `get_def_unc` (309) | architecture | PORTED (NR-3: C fixed upstream) |
| Overlap/OverlapX (798/874) | hard-coded in `overlap.rs:45`/`null_overlap` (67) | architecture | PORTED (no seam; fine for NullRegion) |
| RegBaseBox (961) | `reg_base_box` (113) | architecture | PORTED |
| RegBaseMesh (1024) | `get_region_mesh` (262) | architecture | PORTED |
| GetRegionBounds (1203) | `get_region_bounds` (246) | architecture | PORTED |
| RegMesh (1266, errors) | NONE (base `reg_mesh`) | architecture | MISSING (NR-1) |
| RegBasePick (1106) | `reg_base_pick_axes` (321) | architecture | PORTED |

#### CmpRegion (cmpregion.c vtab 1230-1295)
| C name | Rust | kind | status |
|---|---|---|---|
| astInitCmpRegion (4700) | `CmpRegionBuilder` (cmpregion.rs:44-149) | ctor | DIVERGENT (CR-1, CR-2, CR-5) |
| XOR operator / XORCheck (4005) | NONE (`CmpOperator` has And/Or, mod.rs:70) | public API | MISSING (CR-2) |
| CmpRegionList (228, protected) | `CmpRegion::reg_cmp_region_list` | architecture (stcschan, simplify) | PORTED (CR-5 fixed) |
| Transform (3791) | `reg_base_inside` (295) | architecture | PORTED |
| Simplify (3517) | | architecture | OUT-OF-SCOPE |
| GetDefUnc (1090) | `get_def_unc` (960) | architecture | PORTED |
| SetRegFS (3463)/ResetCache (3115) | `set_region_fs` (811) | ceremony | PORTED |
| Equal (502) | `Mapping::equal` | architecture | PORTED |
| GetObjSize/ManageLock | NONE | ceremony | OUT-OF-SCOPE |
| Set/ClearClosed, Set/ClearMeshSize (620-702) | `set_closed`, `clear_closed`, `set_mesh_size`, `clear_mesh_size` | architecture | PORTED (CR-4 fixed) |
| GetFillFactor (848) | `fill_factor` | architecture | PORTED (CR-4 fixed) |
| RegSetAttrib/RegClearAttrib (2380/3030) | NONE | architecture | MISSING (CR-6) |
| Decompose (408) | NONE (no Mapping decompose API) | architecture | MISSING (LOW; `region1()/region2()/operator()` accessors exist) |
| RegBaseBox (1508) | `reg_base_box` (344) | architecture | PORTED |
| RegBaseBox2 (1719) | `reg_base_box2` (833) | architecture | PORTED |
| RegBaseMesh (1824) | `reg_base_mesh` (502) | architecture | DIVERGENT (CR-3) |
| RegSplit (2472) | NONE | architecture | GATED(Plot) |
| RegPins (2185) | `reg_pins_mask` (667) | architecture | PORTED |
| RegTrace (2685) | NONE | architecture | GATED(Plot) |
| GetBounded (703) | `get_bounded` (251) via `is_bounded` (795) | architecture | PORTED (CR-7 dead code) |
| RegBasePick (2096) | `reg_base_pick_axes` (978) | architecture | PORTED |
| MatchRegion (1406), GetRegions (981) | `match_region` (219), `get_regions` (191) | internal | PORTED |
| Dump (4232)/Load | serial/mod.rs:1306-1383 | ceremony | DIVERGENT (CR-2) |

#### Prism (prism.c vtab 1355-1416)
| C name | Rust | kind | status |
|---|---|---|---|
| astInitPrism (4121) | `PrismBuilder` (prism.rs:56-109) | ctor | DIVERGENT (PR-3) |
| Transform (3523) | `reg_base_inside` (163) | architecture | PORTED |
| Simplify (3204), astConvertToPrism (232) | | architecture | OUT-OF-SCOPE |
| MapList (1527) | `map_list_decompose` (1216) | architecture | PORTED |
| GetDefUnc (1207) | `get_def_unc` (686) | architecture | PORTED |
| SetRegFS (3123) | `set_region_fs` (497) | ceremony | PORTED |
| Equal (516) | `Mapping::equal` (~1200) | architecture | PORTED |
| Set/ClearClosed, Set/ClearMeshSize (630-713) | `set_closed`, `clear_closed`, `set_mesh_size`, `clear_mesh_size` | architecture | PORTED (PR-4 fixed) |
| GetFillFactor (871) | `fill_factor` | architecture | PORTED (PR-4 fixed) |
| Overlap (1663) / OverlapX (1919) | NONE (`overlap` (339) -> generic `compute_overlap`) | architecture | MISSING (PR-2) |
| RegSetAttrib/RegClearAttrib (2990/2865) | NONE | architecture | MISSING (CR-6 applies) |
| GetRegionBounds (941) | `get_region_bounds` (527) | architecture | PORTED |
| Decompose (423) | NONE | architecture | MISSING (LOW) |
| RegBaseBox (1969) | `reg_base_box` (194) | architecture | PORTED |
| RegBaseMesh (2046) | `reg_base_mesh` (214) | architecture | PORTED |
| RegPins (2627) | `reg_pins_mask` (343) | architecture | PORTED |
| GetBounded (779) | `is_bounded_inner` (281) | architecture | PORTED (PR-1 fixed) |
| RegCentre (2451) | `reg_centre_query/set` (697/719) | architecture | PORTED |
| RegBasePick (2287) | `reg_base_pick_axes` (742) | architecture | PORTED |
| Dump (3787)/Load (4266) | serial/mod.rs:1385-1460 | ceremony | DIVERGENT (PR-3) |

### Findings

#### Cross-class

**X-1 MED — `get_region_disc` is a per-class ad-hoc reimplementation of a single base method, in the wrong Frame.**
C has one `GetRegionDisc` (region.c:9651): rejects non-2-D with `AST__INVAR` (9739), temporarily clears Negated (9750), returns BAD radius when unbounded, and works on `astRegMesh` (current Frame, 9757) in a local tangent system.
Rust: Box (box_region.rs:1216, base-Frame centre + Euclidean half-diagonal), Circle (circle.rs:701, base-Frame centre/radius), Ellipse (ellipse.rs:843, planar a/b), Polygon (polygon.rs:1003, vertex mean, Euclidean), PointList (pointlist.rs:488), Interval (interval.rs:1308), CmpRegion/Prism (bbox midpoint, cmpregion.rs:856, prism.rs:697).
Effect: any Region whose base->current Mapping is not a UnitMap (every `map_region` result) returns its disc in base coordinates; sky regions get Euclidean radii; non-2-D input is accepted; negation is ignored.
Exposed by: `astGetRegionDisc` on a `map_region`-ed Circle, or on a Polygon in a SkyFrame. UNVERIFIED; probe: C `astGetRegionDisc` on `astMapRegion(circle, zoommap(2), frame)` vs Rust.

#### Box

**BX-1 HIGH — Box extent uses the raw axis difference, not `astAxDistance`.**
C `Cache` (box.c:768): `extent[i] = fabs(astAxDistance(frm, i+1, corner, centre))`; used by Transform (4200), RegBaseBox (2109), RegPins.
Rust `bounds()` (box_region.rs:551-567): `extent = (corner - centre).abs()`; feeds `reg_base_inside` (816), `reg_base_box` (848), `reg_base_mesh` (854), `reg_pins_mask` (1048).
Effect: a sky Box whose stored corner is the wrapped representation (e.g. centre 6.27 rad, corner 0.037 rad — exactly what `reg_centre_set` produces, since `geo_corner` uses `Frame::offset`, which normalises, box_region.rs:620) gets extent ~6.23 instead of 0.05, so it covers the whole longitude range: wrong membership, bounding box, mesh and RegPins band.
Reachable through `set_uncertainty`, which re-centres the uncertainty Box on the Region's first defining point.
Exposed by: a SkyFrame Box re-centred at RA≈0, or any default/explicit sky uncertainty Box re-centred near RA=0/2π. UNVERIFIED; probe: C `astRegCentre` on a sky Box then `astTransform` of a point at RA=3 rad.

**BX-2 HIGH — Transform ignores the uncertainty-width floor (`Cache(box, 1)`).**
C box.c:807-826: for an unnegated closed or negated open Box, each axis extent is raised to at least half the uncertainty bounding-box width before the membership test.
Rust `reg_base_inside` (813) uses the bare bounds.
Effect: zero-width or thin Boxes (e.g. the equal-limit axes `simplify_over_permmap` creates at box_region.rs:465-469, or degenerate Boxes from PermMap constants) accept only bit-exact coordinates in Rust; C accepts points within the uncertainty.
Exposed by: `astBox(frame,1,[1,5],[2,5])` then transform `(1.5, 5+1e-12)`. UNVERIFIED; that is the probe.

**BX-3 MED — 2-D/N-D boundary mesh distribution is not C's.**
C RegBaseMesh (box.c:2247): (a) non-flat Frames measure edges geodesically with 50 samples (2516); (b) when the axes have different Units and different primary Frames, every edge gets `np/4` points (2470, 2553); (c) zero perimeter is `AST__INTER`; (d) N-D uses integer `np/(2*naxes)` (2605), upper face before lower (2624-2628), and accumulates `lbnd + i*step`; (e) single-point Box fills every axis with `0.5*(lbnd[0]+ubnd[0])` (2408, C quirk).
Rust (box_region.rs:880-972): always Euclidean edge lengths, always proportional, no error, float division, lower face first, `lbnd + (ubnd-lbnd)*k/(n-1)`, single point = centre.
Effect: different mesh point count/positions/order from `get_region_mesh`/`region_mesh`, and different inputs to mesh-driven overlap, BestBox and RegPins checks; most visible for SkyFrame Boxes and mixed-unit (e.g. spectral × spatial) Boxes.
Exposed by: comparing `astGetRegionMesh` of a 2-D SpecFrame×SkyAxis Box and of a SkyFrame Box. UNVERIFIED.

**BX-4 MED — constructor does not reject BAD coordinates.**
C astInitBox box.c:4879/4882 raises `AST__BADIN` for any BAD centre/corner/bound; Rust `BoxRegionBuilder::new/from_bounds/build` (43-153) accepts them and builds a Region with BAD bounds.
Exposed by: `BoxRegionBuilder::from_bounds(frame,&[BAD,0],&[1,1])`.

#### Circle

**CI-1 MED — constructor does not reject BAD coordinates.**
C circle.c:2672/2676 `AST__BADIN` for BAD centre/circumference values (also covers a BAD radius via CircumPoint); Rust `CircleBuilder::build` (181-225) has no check, and reports a missing edge/radius as `NAXIN`.

**CI-2 MED — public `astCirclePars` missing.**
C circle.c:479 (public in circle.h) returns centre, radius and circumference point in the current Frame; no Rust equivalent (ACC-PARS, still open).

#### Ellipse

**EL-1 HIGH — membership and RegPins use planar parameters that assume point 3 is the minor-axis end.**
C Cache/CalcPars (ellipse.c:766-772) derive `a = astDistance(centre, p1)` and `b = a*y/sqrt(a²-x²)` from `astResolve(centre,p1,p2)`, so point 3 may be any boundary point; Transform resolves each test point geodesically (`astResolvePoints`, 2161).
Rust `ellipse_params` (ellipse.rs:435-461) uses Euclidean `a`, `atan2` angle and `b = |p2-centre|`; `reg_base_inside` (477) and `reg_pins_mask` (713-727) rebuild planar ellipses from those.
Effects: (a) wrong membership for any SkyFrame Ellipse; (b) wrong shape for any Ellipse whose point 3 is not the minor-axis end — every C form-0 Ellipse loaded from a dump; (c) the base mesh (582, which is geodesic and uses resolve) and the membership test describe different ellipses, so the Region disagrees with its own boundary.
Exposed by: loading a C dump of `astEllipse(frame,0,c,p1,p2)` with p2 off the minor axis; any SkyFrame Ellipse membership. UNVERIFIED; probe: C `astTransform` of a point near the boundary of such an Ellipse.

**EL-2 MED — position-angle convention differs from C.**
C astEllipse form 1 (ellipse.c:2449-2463; offsets via `astOffset2`): the angle is from axis 2 toward axis 1, and points are geodesic offsets.
Rust `EllipseBuilder` (238-251, 345-350): angle from axis 1 toward axis 2, planar `centre + a(cos,sin)`.
Effect: the same numeric angle gives a different orientation, and a SkyFrame Ellipse gets wrong defining points. It is documented, but it will need translation at any C-ABI layer.

**EL-3 MED — public `astEllipsePars` missing** (ellipse.c:793; ACC-PARS still open).

**EL-4 LOW — constructor and load validation gaps.**
No BAD check on the centre (C 2839-2848 `AST__BADIN`).
`from_node` (serial/mod.rs:1196) does not run C's Cache validation, so a degenerate loaded Ellipse is accepted, where C raises `AST__BADIN` "do not determine an ellipse" (ellipse.c:619).

#### Polygon

**PG-1 HIGH (sky) / MED (Cartesian) — MESH-GEO still open.**
C RegBaseMesh polygon.c:4331-4546: `astDistance` edge lengths, `astOffset` geodesic interpolation, `astNorm` of vertices, `n = 1+(int)(len*mp)` (4484), `AST__BADIN` on zero perimeter (4546).
Rust polygon.rs:712-760: Euclidean lengths, linear interpolation, `round().max(1)`, returns a single vertex on zero perimeter.
Effect: the mesh of a sky Polygon is off the great-circle boundary, which feeds overlap, RegPins checks of simplified Polygons and `region_mesh`; the point count differs even in Cartesian Frames.
Exposed by: `astGetRegionMesh` on any Polygon (count differs), or a SkyFrame Polygon overlap near an edge midpoint.

**PG-2 MED — public `astDownsize` and `astOutline<X>`/`astOutline8<X>` missing** (polygon.c:1326, Outline macros ~3400-3700).
PLY-DOWN's Simplify concern is refuted: `DownsizePoly` is called only from Downsize (polygon.c:1408) and the Outline macro (3668), never from Simplify.

**PG-3 MED — `astConvex` port is partial and algorithmically different.**
C `astConvex<X>` (polygon.c:777-910) takes `oper` (LT/LE/EQ/NE/GE/GT), `lbnd`/`ubnd`, and `starpix` (pixel-centre vs Starlink corner convention, the offsets at 4002-4008), for all data types plus 8-byte-dimension variants, and traces the hull with its own scanline algorithm.
Rust `convex_hull_f32` (374-430): f32 only, EQ/NE only, no `starpix`, Andrew monotone chain over pixel centres, errors `NGDIN` when fewer than 3 pixels match.
Vertex order, start vertex and handling of 1-2 pixel inputs are UNVERIFIED; probe: C `astConvexF` on a small mask, compare `astGetRegionPoints`.

**PG-4 LOW — constructor error codes.**
Non-2-D is `NAXIN` in Rust and `AST__BADIN` in C (7122).
Fewer than 3 vertices is rejected in Rust; C has no such check.

**PG-5 LOW (perf) — `cache()` is recomputed on every `base_inside`/`bounded_impl` call** (polygon.rs:447, 606).
That is O(nv²) line intersections per transform or `is_bounded` call; C caches it (`stale`).

**PG-6 LOW — `is_bounded_inner` swallows geometry errors as `true`** (polygon.rs:779).

#### Interval

**IN-1 HIGH — Transform skips both C paths that widen membership.**
C Transform (interval.c:3634-3635) delegates to the equivalent Box when one exists, which inherits Box `Cache(lohi=1)` uncertainty widening and `astAxIn` sky wrapping.
Otherwise, for closed-unnegated and open-negated Intervals, every axis with `lb == ub` is widened by half the uncertainty width (3700, 3866).
Rust `reg_base_inside` (interval.rs:828-879) uses plain comparisons on the raw limits for every case.
Effect: degenerate (slice) axes accept only bit-exact values, and bounded sky Intervals do not wrap longitude.
Exposed by: `IntervalBuilder::new(frame,[BAD,5],[BAD,5])` and transform `(0, 5+1e-12)`. UNVERIFIED; that is the probe.

**IN-2 HIGH — `is_bounded` is true for an all-excluded Interval.**
C GetBounded (537) is true only when Cache builds a Box, i.e. `isBox && !neg`, where `neg` means axis 1 is excluded (`lb > ub`).
Rust `is_bounded_inner` (956-989) accepts a consistently *excluded* orientation, so an Interval excluding a box on every axis reports bounded.
Effect: a mesh of 2^n corners is produced (904), `reg_centre_query/set` (1391/1404) succeed where C returns no centre or raises `AST__REGCN`, and overlap and grid treat the Region as bounded.
Exposed by: `Interval([2,2],[1,1])`, then `is_bounded()` / `astGetRegionMesh`.

**IN-3 HIGH — boundary mesh is just the 2^n corners.**
C RegBaseMesh (2271) is the equivalent Box's mesh (~MeshSize edge-distributed points) and raises `AST__INTER` when unbounded.
Rust (904-934) returns 2^naxes corners, or an empty Vec when unbounded (then a single BAD point, 1288).
Effect: `region_mesh` differs grossly, and mesh-based overlap of bounded Intervals sees only corners, so edge crossings are missed.
Exposed by: `astGetRegionMesh` of a 2-D Interval (4 vs ~200 points); overlap of an Interval with a thin Box crossing one edge and containing no corner.

**IN-4 MED — BND-INT still open.**
C Interval overrides BndBaseMesh (interval.c:224): it uses the equivalent Box, or for an unbounded Interval clips `astMapBox` of the supplied box by the finite limits and meshes that Box.
Rust has no seam; `overlap.rs:395` builds `CmpRegion(reg1 AND box)` and meshes it, and CR-3 then loses the unbounded Interval's own boundary.
Effect: overlap of a half-plane Interval with a bounded Region misses the Interval edge inside the bounded Region's bbox.
UNVERIFIED; probe: C `astOverlap(circle, interval(x>0))` with the circle straddling x=0.

**IN-5 LOW — RegCentre query ignores `ifrm` in C.**
C interval.c:2585 calls `astRegCentre(box, NULL, NULL, 0, AST__BASE)` whatever `ifrm` was requested (C quirk).
Rust (1397) transforms the centre to the current Frame when asked.

**IN-6 MED — `reg_base_box` widens excluded axes to ±MAX.**
C RegBaseBox (2205) returns the cached limits as stored, with `lb > ub` for excluded axes.
Rust (881-902) rewrites excluded axes to [-MAX, MAX].
`get_region_bounds` therefore differs for excluded Intervals.

#### PointList

**PL-1 HIGH — Mask override missing (silent data loss).**
C `Mask<X>` (pointlist.c:801-975) stamps exactly the pixels containing the listed points (or everything except them).
Rust uses the generic `masking::mask` (masking.rs:42), which tests each pixel centre for membership within the tiny uncertainty, so essentially no pixel matches and nothing is masked.
Note for the base auditor: `masking::mask` documents its Mapping as grid->region, while C `astMask` takes region->grid (`astMapRegion(this, map, grid_frame)`, pointlist.c:863).
Exposed by: `mask(pointlist, unitmap, true, …)` on a grid containing one of the points.

**PL-2 MED — RegBaseBox ignores longitude wrap.**
C (pointlist.c:1648) uses `astAxDistance`/`astAxOffset` relative to the first point.
Rust (pointlist.rs:222-244) takes raw min/max.
Effect: a sky PointList straddling RA=0 gets a ~2π-wide box, and the base default uncertainty (built from that bbox) becomes ~600× too large, which loosens PointList membership.
UNVERIFIED; probe: C `astGetRegionBounds` and `astGetUncFrm` on a PointList at RA 0.01 and 6.27.

**PL-3 MED — `is_bounded` always true.**
PointList inherits base GetBounded in C: a negated non-sky PointList is unbounded.
Rust (pointlist.rs:282) always returns true, so mesh/grid/overlap treat a negated PointList as bounded.

#### NullRegion

**NR-1 LOW — RegMesh override (error) missing.**
C nullregion.c:1266 raises `AST__INTER`.
Rust inherits `reg_mesh`/`region_mesh`; an unnegated NullRegion passes `bounded_for_mesh` (closed_curve_bounded) and returns one BAD point.

**NR-2 LOW — negated NullRegion Transform differs on partially-bad or unmappable points.**
C (nullregion.c:1410-1460) leaves the input copy untouched when negated.
Rust `region_transform` maps through the FrameSet and sets the whole point BAD if any axis is BAD (data.rs:560).
`point_in_region` (nullregion.rs:149) returns `negated` even for BAD input.

**NR-3 LOW — GetDefUnc built on the base Frame.**
C builds the zero-radius Circle with `astCircle(this, …)`, i.e. in the NullRegion's current Frame with its naxes (nullregion.c:222-227).
Rust (309-319) uses the base Frame.

#### CmpRegion

**CR-1 HIGH — constructor does not convert region2 into region1's Frame.**
C astInitCmpRegion (cmpregion.c:4752-4769) runs `astConvert(reg2, reg1)` and, if the Mapping is not a UnitMap, replaces reg2 with `astMapRegion(reg2, smap, frm)`; with no conversion it raises `AST__INTRD`.
Rust `CmpRegionBuilder::new/from_dyn/build` (44-149) only checks axis counts.
`reg_base_inside`/`point_in_region` (295, 615) then pass region1-Frame coordinates straight to region2.
Effect: a CmpRegion of an FK5 Circle and a Galactic Box (or ICRS vs FK5, or different units) tests region2 with the wrong coordinates (wrong membership); incompatible Frames give no error.
Exposed by: `CmpRegion(Circle in FK5, Box in GALACTIC, AND)` membership. UNVERIFIED; probe: C `astTransform` of the same point.

**CR-2 HIGH (load) / MED (API) — XOR unsupported.**
C supports `AST__XOR` (cmpregion.c:4785-4806): it stores the OR of two ANDs and keeps `xor1/xor2`, and Dump writes `Operator = 3` with RegionA/B = the original operands (4232ff).
Rust `CmpOperator` has only And/Or (mod.rs:70-75), and `from_node` maps any operator other than 2 to And (serial/mod.rs:1347-1351).
Effect: loading a C-written XOR CmpRegion yields `A AND B` silently (wrong region); XOR cannot be constructed; illegal operator codes are not rejected (C `AST__INTRD`, 4743).
Exposed by: a C dump of `astCmpRegion(a,b,AST__XOR,"")`.

**CR-3 HIGH — RegBaseMesh uses component *base*-Frame meshes and drops unbounded-component handling.**
C (cmpregion.c:1957-1968) uses `astRegMesh(reg)` (the component's current Frame = the CmpRegion base Frame); when one component has no mesh even when negated it uses `astBndMesh` within the other's bounds; it raises `AST__INTER` for no mesh or for OR with a mesh-less component.
Rust (502-567) calls `get_region_mesh()`, documented as the component *base* Frame (mod.rs:311-315), with no BndMesh path and no errors.
Effect: wrong mesh coordinates whenever a component has a non-UnitMap FrameSet (any `map_region` result used as a component); missing boundary for unbounded components such as half-plane Intervals. This also corrupts the bbox (`reg_base_box` bounded path, 353) and the generic `bnd_base_mesh` (IN-4).
Exposed by: `CmpRegion(circle.map_region(zoom2), box, AND)` then `astGetRegionMesh`.

**CR-4 MED — Closed/MeshSize propagation and FillFactor default missing.**
C `Set/ClearClosed`, `Set/ClearMeshSize` (cmpregion.c:620-702) also set them on both components; `GetFillFactor` (848) defaults to region1's; the constructor copies region1's set MeshSize/Closed onto the new CmpRegion (`astInitCmpRegion` tail).
Rust: plain RegionData setters (749-793), `fill_factor` default 1.0, no constructor copy.
Effect: `set_closed(false)` on a CmpRegion has no effect on boundary membership (components keep Closed); mesh sizes and fill factor differ.

**CR-5 LOW — `CmpRegionList` (cmpregion.c:228) missing.** Protected; used by stcschan (gated) and internally.

**CR-6 MED — RegSetAttrib/RegClearAttrib component propagation missing (CmpRegion and Prism).**
C (cmpregion.c:2380, prism.c:2996) forward a base-Frame attribute setting into both components.
Rust has no seam; the non-adaptive `with_region_integrity` path is a stub (data.rs:473-497, base auditor), so with Adaptive=0 the component Frames are never updated.

**CR-7 LOW — stale `is_bounded_inner` (cmpregion.rs:593-600)** with the pre-GetRegions logic.
It is dead for `is_bounded` (795 uses `get_bounded`), but any `RegionGeometry` caller would get the wrong answer.

#### Prism

**PR-1 MED — GetBounded ignores the Prism's Negated flag.**
C (prism.c:784-825) negates both components when the Prism is negated and requires both to be bounded.
Rust `is_bounded_inner` (prism.rs:381-384) ignores `data.negated`.
Effect: a negated Prism of bounded components reports bounded, so the mesh/grid/overlap paths proceed where C treats it as unbounded.

**PR-2 MED — Prism Overlap/OverlapX seam missing.**
C prism.c:1668-1924 splits the other Region on the Prism's base/extrusion axes and combines the two sub-overlaps through `rtable` plus the negation tables; this handles unbounded extrusions exactly.
Rust `overlap` (339) goes straight to the mesh-based `compute_overlap`, which errors via `reg_base_mesh` (219) for mesh-less components and is approximate otherwise.
Exposed by: `astOverlap(prism(circle, interval(t>0)), box)`.

**PR-3 MED — constructor does not clear component RegionFS (dump not byte-exact).**
C astInitPrism (prism.c:4244-4250) calls `astSetRegionFS(reg, 0)` on each component whose base->current Mapping is a UnitMap, so a freshly built Prism dumps its components without `Frm`.
Rust `PrismBuilder::build` (86-108) does not, while `CmpRegionBuilder` does (cmpregion.rs:114-123).
Effect: a fresh Prism's dump carries `Frm` blocks in RegionA/RegionB that C omits. Simplify output already has them in C, because astCopy resets RegionFS (as in cap_slamap_02.simp).
UNVERIFIED; probe: `astShow(astPrism(box, interval))`.

**PR-4 MED — Closed/MeshSize propagation and FillFactor default missing.**
This is the same as CR-4: C prism.c:635-718 propagates Set/Clear to components; Rust prism.rs:545-589 does not.
GetFillFactor (871) defaults to the product of the components' FillFactors, not to region1's as CR-4's does.

### Status of findings from the earlier audits
- MESH-GEO: OPEN (PG-1).
- PLY-DOWN: Simplify parity concern REFUTED (DownsizePoly not used by Simplify); public `astDownsize`/`astOutline` still MISSING (PG-2).
- GRID-FRM: FIXED (Box `get_region_grid` box_region.rs:1253 uses `frame_grid`; base iterative grid data.rs:573).
- BND-INT: OPEN (IN-4).
- RT-TRACE, RS-SPLIT: OPEN, GATED(Plot).
- ACC-PARS: the public `astCirclePars` and `astEllipsePars` are ported (CI-2, EL-3); still OPEN for the protected `CmpRegionList` (CR-5). The protected point accessors are covered by `get_region_points`.
- PointList Mask (lower-confidence note): CONFIRMED (PL-1).
- region-box-interval-pointlist.md:
  - FIXED: Box RegPins (1040), RegCentre set (1317); Interval Overlap (218/1042), RegPins (1054), RegCentre set (1402); PointList RegPins (338) and uncertainty-driven Transform (162).
  - FIXED, out of scope for this audit: Box PermMap/Polygon/BestBox (441/331/637).
  - STILL OPEN: Box RegBaseMesh geodesic edges (BX-3); Interval RegBaseMesh (IN-3); Interval RegBaseBox (now IN-6, a different divergence, since fixed); PointList RegBaseBox wrap (PL-2, since fixed).
  - Per-class `map_region` uncertainty drop: superseded by the shared `Region::map_region` (mod.rs:835), base-auditor scope.
- region-circle-ellipse-polygon-null.md:
  - FIXED Circle: RegPins (538), RegBaseBox (446), RegBaseMesh (276), RegCentre (779).
  - FIXED Ellipse: RegBaseBox (513), RegBaseMesh (582), RegCentre (920).
  - FIXED Polygon: EnsureInside (622), Cache/acw (516), GetBounded (601), RegPins (836), RegBaseBox (659), Transform (436), SimpVertices/Order (322, serial:1203), SkyArea (567).
  - FIXED NullRegion: GetDefUnc (309), Overlap code 6 (`Overlap::Negation`).
  - STILL OPEN: Ellipse RegPins/Transform parameters (EL-1); Polygon RegBaseMesh (PG-1).
- region-cmpregion-prism.md:
  - FIXED: CmpRegion GetRegions (191), GetBounded (251), RegPins (667), RegBaseBox (344), MatchRegion (219); Prism RegPins (343).
  - STILL OPEN: Prism Overlap/OverlapX (PR-2); CmpRegion RegBaseMesh (CR-3, a new divergence); CmpRegionList (CR-5).
  - Prism `map_region` stub: superseded by the shared base `map_region`.

## Appendix C: probe verification of the HIGH findings

The C probes and Rust probes were scratch programs given identical inputs, the C ones run with `AST_REPLACE_NAN=1`.
Each fix commits its case as a C-oracle fixture under `tools/oracle_fixtures/`, as the Mapping audit's fixes did.

| ID | verdict | C output | Rust output | note |
|---|---|---|---|---|
| RB-1 | CONFIRMED | Interval x>=0 vs Box x in [-10,-9]: overlap(box,iv)=1, overlap(iv,box)=1 | debug build: both abort with stack overflow (SIGABRT); release build: both `Err(Inter "neither ... has a finite boundary")` | release presumably survives until the doubling box goes non-finite (not traced); either way no code 1 |
| RB-2 | CONFIRMED | Box [0,1]^2 vs same mapped by ZoomMap(2): astEqual=0, astOverlap=2; (1.5,1.5) IN b, OUT a | equal=true, overlap=Same; (1.5,1.5) IN b, OUT a | Rust says "same" for regions with different membership |
| RB-3 | CONFIRMED | m-unit Box [0,1]^2, ActiveUnit on Frame, set Unit(1)=km. Adaptive=1: (0.5,0.5) OUT, base m/cur km. Adaptive=0: (0.5,0.5) IN, base km/cur km | Adaptive true and false identical: (0.5,0.5) OUT, base m/cur km | Adaptive=0 ignored |
| RB-4 | CONFIRMED | Box [0,2]^2 via ZoomMap(10): centre (10,10), r=14.14214977 | centre (1,1), r=1.41421356 | Rust reports base-Frame disc, no 1.000001 factor |
| RB-5 | CONFIRMED | ICRS Circle: offset2 region = sky = (2.38323,1.14766) ang 2.30471; resolve region = sky d1=0.0763454 d2=0.0897722; PickAxes class=Circle; System=ICRS; Convert(gal,circle) found, Convert(circle,gal) found | offset2 region (1.42074,1.47015) ang 1.0 vs sky (2.38323,1.14766); resolve region d1=0.2 d2=0.1 vs sky 0.07635/0.08977; pick_axes is neither SkyFrame nor Circle; system_string=None; convert(gal,circle)=None, convert(circle,gal)=Some | all planar defaults; C PickAxes on a Region returns a Region (Circle), not just a Frame |
| BX-1 | CONFIRMED | ICRS Box form 0 centre (6.27,0.5) corner (0.037,0.52): (3,0.5) OUT, (6.28,0.51) IN, (0.02,0.51) IN, (6.0,0.51) OUT; bounds lon [6.21981,6.32019] | (3,0.5) IN, (6.0,0.51) IN, others IN; bounds lon [0.037,12.503] | reproduced with the plain constructor, no RegCentre needed |
| BX-2 | CONFIRMED | Box [1,2]x[5,5]: y=5 IN, 5+1e-12 IN, 5+1e-6 IN, 5+1e-3 OUT | y=5 IN, 5+1e-12 OUT, 5+1e-6 OUT, 5+1e-3 OUT | no uncertainty widening |
| EL-1 | CONFIRMED | form-0 Ellipse c=(0,0) p1=(2,0) p2=(1,0.866) (a=2,b=1): (0,1.2) OUT, (0,0.95) IN; sky Ellipse (1,0.8),(0.2,0.1),0.3 on 61x51 grid: 913 IN | loaded C dump: form-0 (0,1.2) IN; sky grid 929 IN, 82/3111 points disagree (e.g. (0.84,0.72) C OUT, Rust IN) | 7 hand-picked sky points agreed; grid needed |
| PG-1 | PARTLY | ICRS Polygon (0,.2)(1,.2)(1,.8)(0,.8): mesh 201 pts, starts (0,0.8),(0.019064,0.805087), mid (0.855764,0.213231); Cartesian: 202 pts, (0,0.2),(0.015873,0.2), mid (1,0.8) | sky: 202 pts, (0,0.8),(0.015873,0.8), mid (1,0.2) (planar); Cartesian: 202 pts, identical first points and mid | sky confirmed (count+positions); Cartesian count difference not reproduced for this square |
| IN-1 | CONFIRMED | Interval y in [5,5]: 5+1e-12 IN, 5+1e-6 IN, 5+1e-3 OUT; ICRS Interval lon [6.2,6.4]: (0.05,0.2) IN | 5+1e-12 OUT, 5+1e-6 OUT; (0.05,0.2) OUT | both widening paths missing |
| IN-2 | CONFIRMED | Interval lb=(2,2) ub=(1,1): Bounded=0; mesh error AST__MBBNF | is_bounded=true; mesh 4 corner points | membership agrees ((1.5,1.5) OUT, (5,5) IN) |
| IN-3 | CONFIRMED (bounded) / NOT CONFIRMED (unbounded) | Interval [0,1]^2 mesh 200 pts; unbounded Interval mesh -> AST__MBBNF | bounded mesh 4 pts; unbounded -> error "Mapping bounding box not found" (MBBNF) | unbounded error matches C |
| PL-1 | NOT CONFIRMED | PointList (2,2),(3,4), grid [1..5]^2: astMaskD(map=NULL, inside=1) = 2 (pix (2,2),(3,4)); inside=0 with UnitMap = 23 | mask(UnitMap, inside=true) = 2 (same pixels); inside=false = 23 | generic mask matches C here (d250706f mask change); Mapping-direction issue (RB-12) not tested by UnitMap |
| CR-1 | CONFIRMED | FK5 Circle (1,0.5) r=0.1 AND Galactic Box centred on same position (gal (2.85724,-0.34593), corner +0.2): (1,0.5) IN, (1.05,0.5) IN, (1.3,0.5) OUT | build ok; all three OUT | region2 tested with FK5 coordinates |
| CR-2 | CONFIRMED | XOR of [0,2]^2 and [1,3]^2: (0.5,0.5) IN, (1.5,1.5) OUT, (2.5,2.5) IN, (4,4) OUT | C dump loaded: (0.5,0.5) OUT, (1.5,1.5) IN, (2.5,2.5) OUT, (4,4) OUT | XOR silently loaded as AND |
| CR-3 | CONFIRMED | Circle r=1 via ZoomMap(2) AND Box [0,3]x[-0.5,0.5]: mesh 137 pts, first (1.93717,0.49738); bounds x [0,2] | mesh 155 pts, first (0.876307,0.481754) (radius-1 base circle); bounds x [0,1.92] | component base-Frame mesh used |

### New observations
- C quirk: `astSetActiveUnit(region, 1)` always fails with AST__BADAT ("activeunit=1 is invalid for a Frame"), because region.c MAKE_SET(ActiveUnit,int,I) (region.c:12829) forwards through the string `astSetI(frameset,"ActiveUnit",...)` and ActiveUnit is not a string attribute. The RB-3 probe sets ActiveUnit on the Frame before building the Box. Rust `set_active_unit` on a Region succeeds, so it diverges from C (arguably in Rust's favour).
- C `astPickAxes` on a Circle returns a Circle (a Region), not a SkyFrame; RB-5's fix needs to return a Region from RegBasePick, not just the current Frame.
- RB-1 is build-profile dependent: release returns `Err(Inter)` instead of overflowing (presumably smaller frames let the doubling box go non-finite before the stack runs out; not traced), so release-only tests will not see a crash.

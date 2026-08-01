//! The Region trait and associated types.
//!
//! A Region represents an area within a coordinate frame. It combines
//! the [`Frame`] trait (describing the coordinate system) with the
//! [`Mapping`] trait (transforming points via the inside/outside
//! predicate: interior points pass through, exterior points become
//! [`BAD_VALUE`](crate::constants::BAD_VALUE)).

pub(crate) mod data;
pub mod masking;
pub(crate) mod overlap;

mod box_region;
mod circle;
mod cmpregion;
mod ellipse;
mod interval;
mod merge_util;
mod nullregion;
mod pointlist;
mod polygon;
mod prism;
pub use box_region::{BoxRegion, BoxRegionBuilder};
pub use circle::{Circle, CircleBuilder};
pub use cmpregion::{CmpRegion, CmpRegionBuilder};
pub use ellipse::{Ellipse, EllipseBuilder};
pub use interval::{Interval, IntervalBuilder};
pub use nullregion::{NullRegion, NullRegionBuilder};
pub use pointlist::{PointList, PointListBuilder};
pub use polygon::{Polygon, PolygonBuilder};
pub use prism::{Prism, PrismBuilder};

pub use masking::mask;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::is_bad;
use crate::mappings::{CmpMap, DynMapping, Mapping, UnitMap};

/// An owned, dynamically-dispatched Region.
pub type DynRegion = Box<dyn Region>;

/// Result of comparing two regions for overlap.
///
/// These values mirror the C library's `AST__OVERLAP_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlap {
    /// No overlap between the regions (neither contains any points of the other).
    None,
    /// The first region is entirely inside the second.
    FirstInside,
    /// The second region is entirely inside the first.
    SecondInside,
    /// Partial overlap (some points of each lie inside the other).
    Partial,
    /// The regions are identical to within their uncertainties (C code 5).
    Same,
    /// The second region is the exact negation of the first to within their
    /// uncertainties (C code 6): the two regions are mutually exclusive and
    /// together cover the whole coordinate system.
    Negation,
}

/// Comparison operator for combining regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOperator {
    /// Boolean AND: interior is the intersection.
    And,
    /// Boolean OR: interior is the union.
    Or,
}

/// A region of a coordinate frame.
///
/// Regions implement both [`Frame`] (they describe a coordinate system)
/// and [`Mapping`] (they transform points via the inside/outside
/// predicate). A forward transform passes interior points unchanged
/// and sets exterior points to [`BAD_VALUE`](crate::constants::BAD_VALUE);
/// the inverse always passes points unchanged.
///
/// # Negation
///
/// A negated region inverts the inside/outside sense: points that were
/// interior become exterior and vice-versa. Negation is a lightweight
/// flag — it does not recompute geometry.
///
/// # Closed vs Open
///
/// A closed region (the default) includes boundary points; an open
/// region excludes them. For most practical purposes with floating-point
/// coordinates, boundary membership is academic.
pub trait Region: Frame + Mapping {
    // ------------------------------------------------------------------
    // Point membership
    // ------------------------------------------------------------------

    /// Test whether a single point is inside this region.
    ///
    /// `point` must have length equal to `self.naxes()`. The coordinates
    /// are in the current frame of the region's encapsulated FrameSet.
    /// Returns `true` if the point is inside (respecting negation).
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool>;

    /// Determine the overlap between this region and another.
    ///
    /// Both regions must have the same number of axes. The method
    /// accounts for any difference in coordinate frame between the two
    /// regions by aligning them before comparison.
    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap>;

    // ------------------------------------------------------------------
    // Negation
    // ------------------------------------------------------------------

    /// Return a deep-copy of this region with the negation flag toggled.
    fn negated_copy(&self) -> DynRegion;

    /// Toggle the negation flag in place.
    fn negate(&mut self);

    /// Whether this region is currently negated.
    fn is_negated(&self) -> bool;

    // ------------------------------------------------------------------
    // Attributes: Closed, FillFactor, MeshSize, Bounded, Adaptive
    // ------------------------------------------------------------------

    /// Whether boundary points are included (closed) or excluded (open).
    fn is_closed(&self) -> bool;

    /// Set the closed/open flag.
    fn set_closed(&mut self, closed: bool);

    /// The fill factor (fraction of the bounding box that is interior).
    ///
    /// Used by the mesh and overlap algorithms to estimate point density.
    /// Defaults to 1.0 (the region fills its bounding box).
    fn fill_factor(&self) -> f64;

    /// Set the fill factor.
    fn set_fill_factor(&mut self, factor: f64);

    /// The number of points used to represent the boundary mesh.
    fn mesh_size(&self) -> usize;

    /// Set the boundary mesh size.
    fn set_mesh_size(&mut self, size: usize);

    /// Whether this region is bounded (has finite extent in all directions).
    fn is_bounded(&self) -> bool;

    /// Whether adaptive meshing is enabled.
    fn is_adaptive(&self) -> bool;

    /// Set the adaptive meshing flag.
    fn set_adaptive(&mut self, adaptive: bool);

    /// Whether this region currently carries a *dummy* FrameSet, i.e. it
    /// was deserialised without its own `Frm`/`FrmSet` and inherits the
    /// frame of its container. Mirrors C `astRegDummyFS`
    /// (`region.c:8874`). The parent compound loader must re-associate the
    /// real frame via [`Region::set_region_fs`] before use.
    fn has_dummy_fs(&self) -> bool;

    /// Replace this region's (dummy) FrameSet with a real one built from
    /// `frame`, clearing the dummy-FS flag. Mirrors C `astSetRegFS`
    /// (`region.c:10674`). Also re-associates a dummy-FS uncertainty
    /// region with the same frame.
    fn set_region_fs(&mut self, frame: &dyn Frame);

    // ------------------------------------------------------------------
    // Geometry queries
    // ------------------------------------------------------------------

    /// Get the bounding box of this region in the base frame.
    ///
    /// Returns `(lower, upper)` where each is a vector of length `naxes()`.
    /// Axes that are unbounded use [`BAD_VALUE`](crate::constants::BAD_VALUE).
    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)>;

    /// Get a bounding disc for this region.
    ///
    /// Returns `(centre, radius)` where `centre` has length `naxes()`
    /// and `radius` is a single scalar angular radius (for sky regions)
    /// or Cartesian distance.
    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)>;

    /// Get a mesh of points on the boundary of this region.
    ///
    /// The mesh is in the base frame of the encapsulated FrameSet.
    /// The number of points is approximately [`mesh_size()`](Region::mesh_size).
    fn get_region_mesh(&self) -> AstResult<PointSet>;

    /// Get a grid of points filling the interior of this region.
    ///
    /// Returns approximately `npoints` points spread through the volume of
    /// this region in the base frame. Used by compound regions (e.g. Prism)
    /// to build cross-product meshes.
    ///
    /// The default implementation generates a regular grid in the bounding box
    /// and filters to keep only interior points. Returns an empty Vec for
    /// unbounded regions.
    fn get_region_grid(&self, npoints: usize) -> AstResult<Vec<Vec<f64>>>;

    /// Get the defining points of this region in the base frame.
    ///
    /// The meaning depends on the concrete region type (e.g., centre +
    /// edge for Circle, corners for Box).
    fn get_region_points(&self) -> AstResult<PointSet>;

    // ------------------------------------------------------------------
    // Frame access
    // ------------------------------------------------------------------

    /// Get a copy of the current frame of this region's encapsulated FrameSet.
    fn get_region_frame(&self) -> DynFrame;

    /// Get a copy of the encapsulated FrameSet (base → current).
    fn get_region_frameset(&self) -> FrameSet;

    /// Mutable access to the encapsulated FrameSet.
    ///
    /// Used by the shared [`map_region`](Region::map_region) default to
    /// perform the base→current edge surgery C's `astMapRegion` does on
    /// `result->frameset` directly. Every concrete class returns
    /// `&mut self.data.frameset`.
    #[doc(hidden)]
    fn region_frameset_mut(&mut self) -> &mut FrameSet;

    // ------------------------------------------------------------------
    // Uncertainty
    // ------------------------------------------------------------------

    /// Set the uncertainty region associated with boundary positions.
    ///
    /// The uncertainty region defines the positional uncertainty of
    /// points on the boundary. It must have the same number of axes
    /// as this region and be centred at the origin.
    fn set_uncertainty(&mut self, unc: DynRegion);

    /// Get a copy of the uncertainty region, if one has been set.
    fn get_uncertainty(&self) -> Option<DynRegion>;

    /// Return the default uncertainty Region for this Region, in its base
    /// Frame.
    ///
    /// Mirrors C `astGetDefUnc` (region.c:3488): a Box covering `1.0E-6` of
    /// this Region's base-Frame bounding box, centred on the box centre.
    /// For each axis with finite bounds the half-width is
    /// `|0.5E-6 * (ubnd - lbnd)|`, or `c * 0.5E-6` (with `c` the axis centre)
    /// when that width is zero; axes with infinite bounds collapse to a
    /// zero-width `[0, 0]` interval.
    ///
    /// The default implementation is the C `GetDefUnc` body; only Regions
    /// with a bespoke default (none in this port) need override it.
    fn get_def_unc(&self) -> AstResult<DynRegion> {
        let (mut lbnd, mut ubnd) = self.get_region_bounds()?;
        let nax = lbnd.len();
        let bfrm = self.get_region_frameset().base().snapshot();
        for i in 0..nax {
            if !is_bad(ubnd[i]) && !is_bad(lbnd[i]) {
                let mut hw = (0.5e-6 * (ubnd[i] - lbnd[i])).abs();
                let c = 0.5 * (ubnd[i] + lbnd[i]);
                if hw == 0.0 {
                    hw = c * 0.5e-6;
                }
                ubnd[i] = c + hw;
                lbnd[i] = c - hw;
            } else {
                ubnd[i] = 0.0;
                lbnd[i] = 0.0;
            }
        }
        Ok(Box::new(BoxRegion::from_bounds_frame(bfrm, &lbnd, &ubnd)?))
    }

    /// Return the uncertainty Region for this Region in the requested Frame.
    ///
    /// Mirrors C `astGetUncFrm` (region.c:3912): the explicit uncertainty if
    /// one is set, otherwise the default ([`get_def_unc`](Region::get_def_unc)).
    /// For [`FrameRef::Base`] the uncertainty (which is always defined in the
    /// base Frame) is returned directly; for [`FrameRef::Current`] it is
    /// mapped through the base→current Mapping unless that is a `UnitMap`.
    fn get_unc_frm(&self, ifrm: FrameRef) -> AstResult<DynRegion> {
        let unc = match self.get_uncertainty() {
            Some(u) => u,
            None => self.get_def_unc()?,
        };
        match ifrm {
            FrameRef::Base => Ok(unc),
            _ => {
                let fs = self.get_region_frameset();
                let map = fs.mapping(FrameRef::Base, FrameRef::Current)?;
                if map.as_any().is::<UnitMap>() {
                    Ok(unc)
                } else {
                    let frm = fs.current().snapshot();
                    unc.map_region(map.as_ref(), frm.as_ref())
                }
            }
        }
    }

    /// Check whether every point in `pts` (given in this Region's *base*
    /// Frame) lies on this Region's boundary, to within the joint
    /// uncertainty of this Region and the supplied `unc`.
    ///
    /// Mirrors C `astRegPins` (region.c:8879 abstract). The tolerance band on
    /// each axis is half the total width of the two uncertainty bounding
    /// boxes (this Region's uncertainty plus the supplied `unc`); with
    /// `unc = None`, only this Region's own uncertainty contributes. A point
    /// is "on the boundary" iff it lies inside a Region widened by that band
    /// and outside a Region narrowed by it.
    ///
    /// This is a *boundary* test, distinct from the interior membership test
    /// in [`RegionGeometry::reg_base_inside`](data::RegionGeometry::reg_base_inside).
    /// The abstract default reports [`ErrorCode::Inter`]; every concrete class
    /// used by Simplify/Overlap overrides it.
    ///
    /// Returns the overall flag only. Callers needing the per-point boundary
    /// mask (C `astRegPins`'s `int **mask` argument) call [`reg_pins_mask`]
    /// directly. The default forwards to it and discards the mask.
    ///
    /// [`reg_pins_mask`]: Region::reg_pins_mask
    fn reg_pins(&self, pts: &PointSet, unc: Option<&dyn Region>) -> AstResult<bool> {
        Ok(self.reg_pins_mask(pts, unc)?.0)
    }

    /// Like [`reg_pins`](Region::reg_pins), but also returns the per-point
    /// boundary mask.
    ///
    /// Mirrors the full C `astRegPins(this, pset, unc, int **mask)` signature:
    /// the returned `bool` is non-zero iff *every* supplied point lies on this
    /// Region's boundary, and `mask[i]` is `true` iff point `i` individually
    /// lies on the boundary. `OverlapX` (region.c:6996) uses the mask to split
    /// a mesh into on- and off-boundary sub-meshes, so the per-point flags must
    /// be computed even when the overall flag is already known to be false.
    ///
    /// The abstract default reports [`ErrorCode::Inter`]; every concrete class
    /// used by Simplify/Overlap overrides it.
    fn reg_pins_mask(
        &self,
        _pts: &PointSet,
        _unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        Err(AstError::new(
            ErrorCode::Inter,
            "reg_pins: this Region class does not implement the astRegPins method".to_string(),
        ))
    }

    /// Shift the centre of this Region to the supplied base-Frame position.
    ///
    /// `cen` holds the new centre coordinates in this Region's *base* Frame
    /// (length `naxes()`); any [`BAD_VALUE`](crate::constants::BAD_VALUE)
    /// component leaves the corresponding centre axis unchanged. Mirrors the
    /// set-path of C `astRegCentre` (region.c:8397 abstract) with `ifrm =
    /// AST__BASE`: the base-Frame enquiry/return path is not exposed here
    /// because the only caller (PointList membership) supplies a new centre.
    ///
    /// The abstract default reports [`ErrorCode::Inter`], matching the C
    /// behaviour of erroring when the set-path is invoked on a Region class
    /// that has no centre. Only classes that carry a centre (Box) override it.
    fn reg_centre_set(&mut self, _cen: &[f64]) -> AstResult<()> {
        Err(AstError::new(
            ErrorCode::Inter,
            "reg_centre_set: this Region class has no centre".to_string(),
        ))
    }

    // ------------------------------------------------------------------
    // Remapping
    // ------------------------------------------------------------------

    /// Map this region into a new frame.
    ///
    /// The `mapping` transforms coordinates from this region's current Frame
    /// to the new `frame`. Returns a new Region representing the same area in
    /// the new coordinate system.
    ///
    /// Faithful port of C `astMapRegion` / `MapRegion` (region.c:5375). The C
    /// method is defined **once** on the Region base class (`vtab->MapRegion =
    /// MapRegion`, region.c:4620) and is *not* overridden per subclass, so this
    /// is a single shared default rather than a per-class implementation.
    ///
    /// Crucially, it does **not** bake `mapping` into the Region's defining
    /// points. It deep-copies the Region (preserving the original base Frame,
    /// defining points, and any subclass sub-Regions), then installs `mapping`
    /// as the base→current edge of the copy's FrameSet by adding `frame` as a
    /// new current Frame connected via `mapping` and removing the original
    /// current Frame. The base and current Frames of the result therefore
    /// differ in general (so the Region dumps its full FrameSet).
    fn map_region(&self, mapping: &dyn Mapping, frame: &dyn Frame) -> AstResult<DynRegion> {
        // The supplied Mapping must define both a forward and an inverse
        // transformation (region.c:5488-5498).
        if !mapping.has_inverse() {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "map_region: the supplied Mapping does not define an inverse transformation"
                    .to_string(),
            ));
        }
        if !mapping.has_forward() {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "map_region: the supplied Mapping does not define a forward transformation"
                    .to_string(),
            ));
        }

        // Number of axes in the supplied Region and in the returned Region.
        let nold = self.naxes();
        let nnew = frame.naxes();

        // The forward transformation must not introduce any bad axis values.
        // We can only test this reliably if the supplied Region's defining
        // points are all good (region.c:5507-5581). `axflags[i]` is non-zero
        // if the inverse transformation produces any good value for base axis
        // `i`; it is only consulted by the axis-dropping block below.
        let mut axflags: Option<Vec<bool>> = None;
        let src_pts = self.get_region_points()?;
        if src_pts.npoint() > 0 && src_pts.ncoord() > 0 {
            let np = src_pts.npoint();
            let ncoord = src_pts.ncoord();
            let mut all_good = true;
            'outer: for i in 0..ncoord {
                for j in 0..np {
                    if is_bad(src_pts.point(j)[i]) {
                        all_good = false;
                        break 'outer;
                    }
                }
            }
            if all_good {
                // Transform the defining points into the current Frame, then
                // through the supplied Mapping into the new Frame.
                let fs = self.get_region_frameset();
                let pst = fs.transform(&src_pts, true)?;
                let ps2 = mapping.transform(&pst, true)?;

                // Any bad axis value in the new Frame means the result is
                // undefined (region.c:5539-5551).
                let mut ok = true;
                'chk: for i in 0..nnew {
                    for j in 0..np {
                        if is_bad(ps2.point(j)[i]) {
                            ok = false;
                            break 'chk;
                        }
                    }
                }
                if !ok {
                    return Err(AstError::new(
                        ErrorCode::Nodef,
                        "map_region: the region which results from the supplied Mapping is \
                         undefined"
                            .to_string(),
                    ));
                }

                // Inverse-transform back to the Region's Frame and record
                // which base axes received any good value.
                let back = mapping.transform(&ps2, false)?;
                let mut flags = vec![false; nold];
                for (i, flag) in flags.iter_mut().enumerate().take(nold.min(back.ncoord())) {
                    for j in 0..np {
                        if !is_bad(back.point(j)[i]) {
                            *flag = true;
                            break;
                        }
                    }
                }
                axflags = Some(flags);
            }
        }

        // Assume we will use the supplied Region and Mapping unchanged.
        let mut usethis: DynRegion = self.clone_region();
        let mut usemap: DynMapping = mapping.clone_box();

        // If the new Frame has fewer axes than the old Frame, try to discard
        // some base-Frame axes (region.c:5587-5701). We only do this if the
        // inverse would otherwise supply bad values for the unused axes.
        if nnew < nold {
            // astMapSplit selects inputs, so invert to select outputs; work
            // on a clone so the original `mapping` is left untouched.
            let mut inv = mapping.clone_box();
            inv.invert();

            // Required inputs of the inverted map: the new-Frame axes 0..nnew
            // (1-based for map_split).
            let inax: Vec<usize> = (1..=nnew).collect();
            if let Some((outax_1, tmap)) = inv.map_split(&inax) {
                // 1-based original old axes retained by the split mapping.
                let outax: Vec<usize> = outax_1.iter().map(|&a| a - 1).collect();
                let nkept = tmap.nout();

                // Ensure any unused axis that still generates good values is
                // retained (region.c:5612-5647).
                let mut keep: Vec<usize> = Vec::new();
                if let Some(flags) = &axflags {
                    for i in 0..nold {
                        // Keep an old axis if it survives the split, or if it
                        // was dropped but still yields good values through the
                        // Mapping (region.c:5612-5647).
                        let retained = outax.iter().take(nkept).any(|&a| a == i);
                        if retained || flags.get(i).copied().unwrap_or(false) {
                            keep.push(i);
                        }
                    }
                }
                let ntotal = keep.len();

                if axflags.is_none() || ntotal == nkept {
                    // The split mapping can be used as-is: pick the retained
                    // old axes from the Region and use the (re-inverted) split
                    // mapping (region.c:5651-5668).
                    let pick: Vec<usize> = outax.iter().take(nkept).copied().collect();
                    if let PickedAxes::Region(r) = pick_axes_region(usethis.as_ref(), &pick)? {
                        let mut sub = tmap;
                        sub.invert();
                        usethis = r;
                        usemap = sub;
                    }
                } else if ntotal > nkept {
                    // Retain the extra good-value axes: split the forward map
                    // on the chosen old axes (region.c:5672-5692).
                    if let Some((_out2, tmap2)) = mapping.map_split(&{
                        let mut k: Vec<usize> = keep.iter().map(|&a| a + 1).collect();
                        k.sort_unstable();
                        k
                    }) && let PickedAxes::Region(r) = pick_axes_region(usethis.as_ref(), &keep)?
                    {
                        usethis = r;
                        usemap = tmap2;
                    }
                }
            }
        }

        // Take a deep copy of the (possibly axis-reduced) Region and perform
        // the base→current edge surgery on its FrameSet (region.c:5703-5722).
        let mut result: DynRegion = usethis.clone_region();
        {
            let fs = result.region_frameset_mut();
            let icurr = fs.current_idx() + 1; // 1-based for FrameRef::Index
            fs.add_frame(FrameRef::Current, usemap, frame.snapshot())?;
            fs.remove_frame(FrameRef::Index(icurr))?;
        }

        Ok(result)
    }

    // ------------------------------------------------------------------
    // Axis selection (PickAxes / RegBasePick)
    // ------------------------------------------------------------------

    /// Return a Region formed by picking selected *base* Frame axes from
    /// this Region, or `None` if no same-class sub-region can be formed.
    ///
    /// `axes` holds zero-based base-Frame axis indices. Mirrors C
    /// `astRegBasePick` (region.c:13679 default → `None`); classes that can
    /// split their defining geometry per-axis (Box, Interval, PointList,
    /// NullRegion) override this. Classes whose geometry cannot be sliced
    /// axis-wise (Circle, Ellipse, Polygon, CmpRegion, Prism) use the
    /// default and return `None`.
    fn reg_base_pick_axes(&self, _axes: &[usize]) -> AstResult<Option<DynRegion>> {
        Ok(None)
    }

    /// Copy selected properties from `that` onto this Region.
    ///
    /// Mirrors C `astRegOverlay` (region.c:9861): copies Negated, Closed,
    /// and Adaptive; copies MeshSize/FillFactor only when the axis counts
    /// match. The `unc` flag controls uncertainty clearing (unused here as
    /// the picked regions carry their own uncertainty).
    fn reg_overlay(&mut self, that: &dyn Region) {
        if self.is_negated() != that.is_negated() {
            self.negate();
        }
        self.set_closed(that.is_closed());
        self.set_adaptive(that.is_adaptive());
        if self.naxes() == that.naxes() {
            self.set_mesh_size(that.mesh_size());
            self.set_fill_factor(that.fill_factor());
        }
    }

    /// Permute the axes of this region's current Frame, updating the
    /// encapsulated FrameSet and defining points to preserve the region
    /// integrity invariant. Mirrors C `astPermAxes` applied to a Region.
    ///
    /// `perm` holds zero-based target indices, one per current axis.
    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()>;

    // ------------------------------------------------------------------
    // Parallel merge (MergeXxx)
    // ------------------------------------------------------------------

    /// Attempt to combine this Region with `other` into a Region of higher
    /// dimensionality, for a parallel `CmpMap` of the two.
    ///
    /// `intfirst` selects the axis order in the combined Region: when true,
    /// this Region's axes go first and `other`'s axes second; when false,
    /// `other`'s axes go first. Returns the merged Region as a `DynMapping`,
    /// or `None` if the two Regions cannot be merged.
    ///
    /// Mirrors the per-class `MergeXxx` helpers in the C reference
    /// (`interval.c::MergeInterval`, `box.c::MergeBox`,
    /// `pointlist.c::MergePointList`, `nullregion.c::MergeNullRegion`), whose
    /// `intfirst`/`boxfirst`/`plsfirst` parameter this mirrors. The default
    /// returns `None` (classes that never merge in parallel — Circle,
    /// Ellipse, Polygon, CmpRegion, Prism).
    #[doc(hidden)]
    fn region_merge(&self, _other: &dyn Region, _intfirst: bool) -> Option<DynMapping> {
        None
    }

    // ------------------------------------------------------------------
    // Cloning (object-safe deep copy)
    // ------------------------------------------------------------------

    /// Clone this region into a boxed trait object.
    #[doc(hidden)]
    fn clone_region(&self) -> DynRegion;
}

/// Result of picking axes from a Region: either a same-class sub-Region
/// (when the current→base Mapping splits cleanly) or a plain Frame.
///
/// Mirrors the dual return of C `region.c::PickAxes` (region.c:7579), which
/// returns a Frame that `astIsARegion` may or may not accept.
pub enum PickedAxes {
    /// A Region spanning the selected axes.
    Region(DynRegion),
    /// A plain Frame (the Mapping did not split, or no sub-region formed).
    Frame(DynFrame),
}

/// Pick `axes` (zero-based current-Frame indices) from `region`, returning a
/// [`PickedAxes::Region`] when the current→base Mapping splits cleanly on the
/// selected axes and the region's class can form a base sub-region, else a
/// [`PickedAxes::Frame`].
///
/// Faithful port of C `region.c::PickAxes` (region.c:7579).
pub(crate) fn pick_axes_region(region: &dyn Region, axes: &[usize]) -> AstResult<PickedAxes> {
    use crate::frames::FrameRef;

    // Pick the required axes from the current Frame. The C code passes the
    // axis list straight to astPickAxes (frame.c) which uses zero-based
    // indices; pick_axes_snapshot takes i32 with -1 meaning "default axis".
    let cfrm = region.get_region_frame();
    let target: Vec<i32> = axes.iter().map(|&a| a as i32).collect();
    let picked_frame = cfrm.pick_axes_snapshot(&target)?;

    // No default axes are supported for the Region path (C returns a basic
    // Frame if any default axes were requested). We only reach here with
    // concrete indices, so proceed to the split test.
    //
    // See if the requested current-Frame axes correspond to a unique set of
    // base-Frame axes by attempting to split the current->base Mapping.
    let cbmap = region
        .get_region_frameset()
        .mapping(FrameRef::Current, FrameRef::Base)?;

    // map_split expects 1-based axis indices and returns the (1-based)
    // output axes of `cbmap` — i.e. the base-Frame axes generated by the
    // selected current-Frame axes.
    let one_based: Vec<usize> = axes.iter().map(|&a| a + 1).collect();
    if let Some((out_axes, fsmap)) = cbmap.map_split(&one_based) {
        // Convert the returned base-Frame axis indices to zero-based for
        // reg_base_pick_axes, which slices the base defining geometry.
        let base_axes: Vec<usize> = out_axes.iter().map(|&a| a - 1).collect();

        if let Some(breg) = region.reg_base_pick_axes(&base_axes)? {
            // Invert fsmap so it maps selected base axes -> selected current
            // axes, then map the base region into the picked current Frame.
            let mut fsmap = fsmap;
            fsmap.invert();
            let mut creg = breg.map_region(fsmap.as_ref(), picked_frame.as_ref())?;
            // Copy properties from the old Region to the new Region.
            creg.reg_overlay(region);
            return Ok(PickedAxes::Region(creg));
        }
    }

    Ok(PickedAxes::Frame(picked_frame))
}

impl Clone for Box<dyn Region> {
    fn clone(&self) -> Self {
        self.clone_region()
    }
}

/// Borrow a `dyn Mapping` as a `dyn Region` if it is a concrete Region
/// type, else return `None`. The C reference uses `astIsARegion` for the
/// same purpose in the region merge code.
pub(crate) fn as_region(mapping: &dyn Mapping) -> Option<&dyn Region> {
    let a = mapping.as_any();
    if let Some(r) = a.downcast_ref::<Circle>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<BoxRegion>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<Ellipse>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<Polygon>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<Interval>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<NullRegion>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<CmpRegion>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<PointList>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<Prism>() {
        Some(r)
    } else {
        None
    }
}

/// Test whether a `dyn Mapping` is a Region.
fn is_region(mapping: &dyn Mapping) -> bool {
    let a = mapping.as_any();
    a.is::<Circle>()
        || a.is::<BoxRegion>()
        || a.is::<Ellipse>()
        || a.is::<Polygon>()
        || a.is::<Interval>()
        || a.is::<NullRegion>()
        || a.is::<CmpRegion>()
        || a.is::<PointList>()
        || a.is::<Prism>()
}

/// Remove all Region nodes from a mapping tree.
///
/// Walks the mapping tree recursively. Any node that is a Region is
/// replaced with a [`UnitMap`] of matching dimensionality, removing
/// the region's point-clipping behaviour while preserving the overall
/// dimensionality of the coordinate transformation.
///
/// For a [`CmpMap`], both children are processed recursively.
/// Other non-Region mappings are returned as clones.
pub fn remove_regions(mapping: &dyn Mapping) -> DynMapping {
    if is_region(mapping) {
        return Box::new(UnitMap::new(mapping.nin()));
    }

    if let Some(cmp) = mapping.as_any().downcast_ref::<CmpMap>() {
        let mut first = remove_regions(cmp.first());
        let mut second = remove_regions(cmp.second());
        // Preserve the effective inversion state of each child.
        if cmp.inv_a() != cmp.first().is_inverted() {
            first.invert();
        }
        if cmp.inv_b() != cmp.second().is_inverted() {
            second.invert();
        }
        let mut new_cmp = CmpMap::new(first, second, cmp.composition())
            .expect("remove_regions: CmpMap reconstruction failed");
        if mapping.is_inverted() {
            new_cmp.invert();
        }
        return Box::new(new_cmp);
    }

    if let Some(sw) = mapping
        .as_any()
        .downcast_ref::<crate::mappings::SwitchMap>()
    {
        let new_fs = sw.forward_selector().map(remove_regions);
        let new_is = sw.inverse_selector().map(remove_regions);
        let new_routes: Vec<DynMapping> = sw
            .route_maps()
            .iter()
            .map(|r| remove_regions(r.as_ref()))
            .collect();

        let fs_changed = match (sw.forward_selector(), &new_fs) {
            (Some(orig), Some(new)) => !new.equal(orig),
            _ => false,
        };
        let is_changed = match (sw.inverse_selector(), &new_is) {
            (Some(orig), Some(new)) => !new.equal(orig),
            _ => false,
        };
        let routes_changed = sw
            .route_maps()
            .iter()
            .zip(new_routes.iter())
            .any(|(orig, new)| !new.equal(orig.as_ref()));

        if fs_changed || is_changed || routes_changed {
            let mut new_sw = crate::mappings::SwitchMap::new(new_fs, new_is, new_routes)
                .expect("remove_regions: SwitchMap reconstruction failed");
            if mapping.is_inverted() {
                new_sw.invert();
            }
            return Box::new(new_sw);
        }

        return mapping.clone_box();
    }

    mapping.clone_box()
}

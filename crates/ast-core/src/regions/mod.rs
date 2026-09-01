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

    /// Whether the closed/open flag has been explicitly set (C `TestClosed`,
    /// `this->closed != -INT_MAX`).
    fn test_closed(&self) -> bool;

    /// Copy the closed/open flag together with its explicitly-set state,
    /// mirroring C's raw field assignment `this->closed = that->closed`
    /// (which propagates the `-INT_MAX` unset sentinel). Unlike
    /// [`set_closed`](Region::set_closed) this does not force the attribute to
    /// appear explicitly set.
    fn set_closed_raw(&mut self, closed: bool, set: bool);

    /// The fill factor (fraction of the bounding box that is interior).
    ///
    /// Used by the mesh and overlap algorithms to estimate point density.
    /// Defaults to 1.0 (the region fills its bounding box).
    fn fill_factor(&self) -> f64;

    /// Set the fill factor.
    fn set_fill_factor(&mut self, factor: f64);

    /// Whether the fill factor has been explicitly set (C `TestFillFactor`).
    fn test_fill_factor(&self) -> bool;

    /// Clear the fill factor, reverting to the default (C `ClearFillFactor`).
    fn clear_fill_factor(&mut self);

    /// The number of points used to represent the boundary mesh.
    ///
    /// When unset, this is the naxes-dependent default (1-D → 2, 2-D → 200,
    /// ≥3-D → 2000), mirroring C `astGetMeshSize`.
    fn mesh_size(&self) -> usize;

    /// Set the boundary mesh size.
    fn set_mesh_size(&mut self, size: usize);

    /// Whether the mesh size has been explicitly set (C `TestMeshSize`).
    fn test_mesh_size(&self) -> bool;

    /// Clear the mesh size, reverting to the default (C `ClearMeshSize`).
    fn clear_mesh_size(&mut self);

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

    /// Bounding box of the *un-negated* region in the base Frame, ignoring the
    /// Negated attribute of any component Regions.
    ///
    /// Mirrors C `astRegBaseBox2` (region.c:7909). The base-class behaviour is
    /// identical to [`get_region_bounds`](Region::get_region_bounds); only
    /// compound classes (CmpRegion) override it so the un-negated assumption is
    /// propagated to their components.
    fn reg_base_box2(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.get_region_bounds()
    }

    /// Bounding box of the un-negated region in the *current* Frame, ignoring
    /// the Negated attribute of any component Regions.
    ///
    /// Mirrors C `astGetRegionBounds2` (region.c:9139): compute the base-Frame
    /// box via [`reg_base_box2`](Region::reg_base_box2), then map it through the
    /// simplified base→current Mapping (a `UnitMap` copies the bounds directly;
    /// otherwise each output axis is bounded with `astMapBox`).
    fn get_region_bounds2(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        let (lbndb, ubndb) = self.reg_base_box2()?;
        let fs = self.get_region_frameset();
        let map = fs.mapping(FrameRef::Base, FrameRef::Current)?;
        let smap = map.simplify()?;
        let ncur = fs.current().naxes();
        if smap.as_any().is::<UnitMap>() {
            return Ok((lbndb, ubndb));
        }
        let mut lbnd = vec![0.0; ncur];
        let mut ubnd = vec![0.0; ncur];
        for (i, (lo, hi)) in lbnd.iter_mut().zip(ubnd.iter_mut()).enumerate() {
            let mb = smap.map_box(&lbndb, &ubndb, true, i)?;
            *lo = mb.lbnd_out;
            *hi = mb.ubnd_out;
        }
        Ok((lbnd, ubnd))
    }

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

    /// Store (or clear) the raw uncertainty Region without validation or
    /// remapping, invalidating any cached information.
    ///
    /// This is the low-level field write used by
    /// [`set_uncertainty`](Region::set_uncertainty); it corresponds to the C
    /// `this->unc = ...` assignments plus `astResetCache`. Callers that want
    /// the full C `astSetUnc` semantics (class validation, remap into the base
    /// Frame, negate-if-unbounded, RegionFS optimisation, re-centre) must use
    /// [`set_uncertainty`](Region::set_uncertainty).
    #[doc(hidden)]
    fn store_uncertainty(&mut self, unc: Option<DynRegion>);

    /// Set the boolean `RegionFS` attribute, controlling whether the
    /// encapsulated FrameSet is included in a dump.
    ///
    /// Mirrors C `astSetRegionFS` (region.c:12603): setting it to `false`
    /// causes the FrameSet to be omitted when the base→current Mapping is a
    /// UnitMap. This is distinct from [`set_region_fs`](Region::set_region_fs),
    /// which installs a real FrameSet.
    #[doc(hidden)]
    fn set_region_fs_flag(&mut self, value: bool);

    /// The current value of the boolean `RegionFS` attribute (whether the
    /// encapsulated FrameSet is included in a dump). Mirrors reading C's
    /// `regionfs` field, used by [`reg_overlay`](Region::reg_overlay).
    #[doc(hidden)]
    fn region_fs_flag(&self) -> bool;

    /// Set the uncertainty region associated with boundary positions.
    ///
    /// Faithful port of C `astSetUnc` (region.c:10754). Each Region may carry
    /// an uncertainty Region describing the positional uncertainty of points
    /// on its boundary. The supplied `unc` must be of a centro-symmetric class
    /// (`Box`, `Circle`, `Ellipse`) or a `Prism` of such — i.e. a class that
    /// can be re-centred; otherwise an [`ErrorCode::Badin`] error is reported.
    ///
    /// The supplied Region is deep-copied and remapped into this Region's base
    /// Frame, negated if it is unbounded (so it becomes bounded), tagged to omit
    /// its FrameSet from dumps when the base→current Mapping simplifies to a
    /// UnitMap, and re-centred at this Region's first defining point.
    ///
    /// This is a shared default; the C body is fully generic (it touches only
    /// the FrameSet, defining points, uncertainty field, and cache). Only the
    /// construction-time uncertainty supplied through a builder bypasses this
    /// path (C routes it through `SetUnc` at region.c:13372; the builders store
    /// it directly, which the existing serialization corpus does not exercise
    /// under a non-trivial Frame relationship).
    fn set_uncertainty(&mut self, unc: DynRegion) {
        // Validate that the supplied Region is of a re-centreable
        // (centro-symmetric) class. C tests this via
        // astRegCentre(unc, NULL, NULL, 0, CURRENT) returning non-NULL
        // (region.c:10836).
        let usable = match unc.reg_centre_query(FrameRef::Current) {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => false,
        };

        if !usable {
            // Annul any existing uncertainty and report the error, mirroring
            // C (region.c:10826-10832, 10899-10904). The bad shape leaves the
            // Region with no uncertainty.
            if self.get_uncertainty().is_some() {
                self.store_uncertainty(None);
            }
            return;
        }

        // Map the supplied uncertainty into this Region's base Frame. C
        // inverts this->frameset, runs Conv(unc->frameset, ...), then
        // astMapRegion (region.c:10840-10850). The base Frame of this Region
        // is the target coordinate system, and `Conv` aligns in the Domain of
        // that Frame (region.c:2367-2370).
        let mut unc = unc;
        let target = self.get_region_frameset().base().snapshot();
        let domain = target.domain().to_string();
        let converted = unc
            .get_region_frame()
            .convert_to(target.as_ref(), &[&domain]);

        // `Conv` re-instates each FrameSet's original base Frame index with
        // astSetBase (region.c:2373-2374). That restores the index it read,
        // but leaves the attribute *set* even when it was unset on entry, so
        // the uncertainty's FrameSet from here on dumps `Base` as an active
        // card. The other FrameSet `Conv` restores is this Region's own, which
        // is inverted across the call, so there astSetBase writes the Current
        // slot instead (frameset.c:9836) — an index this port always dumps as
        // active, so it needs no flag of its own.
        let _ = unc.region_frameset_mut().set_base(FrameRef::Base);

        let conv = match converted {
            Ok(Some(fs)) => fs,
            _ => {
                // Frames not convertible: annul existing, mirroring C's error
                // branch (region.c:10889-10896) which leaves no uncertainty.
                if self.get_uncertainty().is_some() {
                    self.store_uncertainty(None);
                }
                return;
            }
        };
        let map = match conv.mapping(FrameRef::Base, FrameRef::Current) {
            Ok(m) => m,
            Err(_) => {
                if self.get_uncertainty().is_some() {
                    self.store_uncertainty(None);
                }
                return;
            }
        };
        let mut new_unc = match unc.map_region(map.as_ref(), target.as_ref()) {
            Ok(u) => u,
            Err(_) => {
                if self.get_uncertainty().is_some() {
                    self.store_uncertainty(None);
                }
                return;
            }
        };

        // Ensure the uncertainty is bounded. Negating an unbounded
        // uncertainty makes it bounded because it consists of Circles, Boxes
        // and/or Ellipses (region.c:10853-10856).
        if !new_unc.is_bounded() {
            new_unc.negate();
        }

        // If the uncertainty's base→current Mapping simplifies to a UnitMap,
        // its current Frame equals this Region's base Frame, so the FrameSet
        // can be omitted from the dump (region.c:10858-10869).
        if let Ok(m2) = new_unc
            .get_region_frameset()
            .mapping(FrameRef::Base, FrameRef::Current)
            && let Ok(smap) = m2.simplify()
            && smap.as_any().is::<UnitMap>()
        {
            new_unc.set_region_fs_flag(false);
        }

        // Re-centre the uncertainty at this Region's first defining point
        // (region.c:10871-10876). The point is expressed in this Region's
        // base Frame, which is the uncertainty's current Frame, so it is
        // supplied to the uncertainty's set-path with `ifrm = Current`.
        if let Ok(points) = self.get_region_points()
            && points.npoint() > 0
        {
            let first = points.point(0).to_vec();
            let _ = new_unc.reg_centre_set(&first, FrameRef::Current);
        }

        self.store_uncertainty(Some(new_unc));
    }

    /// Get a copy of the uncertainty region, if one has been set.
    fn get_uncertainty(&self) -> Option<DynRegion>;

    /// Whether an explicit (non-default) uncertainty has been assigned.
    ///
    /// Mirrors C `astTestUnc` (region.c): true iff a specific uncertainty
    /// Region has been stored, false when the Region relies on its default
    /// uncertainty ([`get_def_unc`](Region::get_def_unc)).
    fn test_unc(&self) -> bool {
        self.get_uncertainty().is_some()
    }

    /// The base-class default uncertainty: a Box covering `1.0E-6` of this
    /// Region's base-Frame bounding box, centred on the box centre.
    ///
    /// Mirrors C base `GetDefUnc` (region.c:3488). For each axis with finite
    /// bounds the half-width is `|0.5E-6 * (ubnd - lbnd)|`, or `c * 0.5E-6`
    /// (with `c` the axis centre) when that width is zero; axes with infinite
    /// bounds collapse to a zero-width `[0, 0]` interval.
    ///
    /// The bounding box is taken from [`reg_base_box2`](Region::reg_base_box2)
    /// (not [`get_region_bounds`](Region::get_region_bounds)) because C uses
    /// `astRegBaseBox2` here: for compound Regions the un-negated assumption
    /// must be propagated to the component Regions.
    ///
    /// Classes that override [`get_def_unc`](Region::get_def_unc) call this to
    /// reach the inherited (parent) behaviour when their bespoke rule defers.
    fn default_bounding_box_unc(&self) -> AstResult<DynRegion> {
        let (mut lbnd, mut ubnd) = self.reg_base_box2()?;
        let nax = lbnd.len();
        let bfrm = self.get_region_frameset().base().snapshot();
        for i in 0..nax {
            // C tests the two bounds asymmetrically (`region.c:3564`): an axis is
            // unbounded when its upper bound is `+DBL_MAX` or its lower bound is
            // `-DBL_MAX`. `AST__BAD` *is* `-DBL_MAX`, so `is_bad(lbnd[i])` already
            // covers the lower half; the upper half needs its own test because
            // `+DBL_MAX` is an ordinary finite number.
            if !is_bad(ubnd[i]) && !is_bad(lbnd[i]) && ubnd[i] != f64::MAX && lbnd[i] != -f64::MAX {
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

    /// Return the default uncertainty Region for this Region, in its base
    /// Frame.
    ///
    /// Mirrors C `astGetDefUnc` (region.c:3488). The base-class rule is the
    /// bounding-box Box in [`default_bounding_box_unc`](Region::default_bounding_box_unc);
    /// Interval, NullRegion, Prism, and CmpRegion override this with their own
    /// C-defined defaults.
    fn get_def_unc(&self) -> AstResult<DynRegion> {
        self.default_bounding_box_unc()
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
    /// in `RegionGeometry::reg_base_inside`.
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

    /// Transform a single point between this Region's base and current Frames.
    ///
    /// Faithful port of C `astRegTranPoint` (region.c:11788). When `forward` is
    /// true the supplied point is in the base Frame and is transformed to the
    /// current Frame; when false it is in the current Frame and is transformed
    /// to the base Frame.
    fn reg_tran_point(&self, point: &[f64], forward: bool) -> AstResult<Vec<f64>> {
        let fs = self.get_region_frameset();
        let map = if forward {
            fs.mapping(FrameRef::Base, FrameRef::Current)?
        } else {
            fs.mapping(FrameRef::Current, FrameRef::Base)?
        };
        let pset = PointSet::from_rows(std::iter::once(point));
        let out = map.transform(&pset, true)?;
        Ok(out.point(0).to_vec())
    }

    /// Shift the centre of this Region to the supplied position.
    ///
    /// `cen` holds the new centre coordinates (length equal to the number of
    /// axes in the base Frame when `ifrm` is [`FrameRef::Base`], or the current
    /// Frame when [`FrameRef::Current`]); any
    /// [`BAD_VALUE`](crate::constants::BAD_VALUE) component leaves the
    /// corresponding centre axis unchanged. Mirrors the set-path of C
    /// `astRegCentre` (region.c:8397 abstract): when `ifrm` is
    /// [`FrameRef::Current`] the supplied point is first transformed into the
    /// base Frame via the region's base→current Mapping.
    ///
    /// The abstract default reports [`ErrorCode::Inter`], matching the C
    /// behaviour of erroring when the set-path is invoked on a Region class
    /// that has no centre. Only centro-symmetric classes (`Box`, `Circle`,
    /// `Ellipse`, `Interval`-when-a-Box, `Prism`-of-such) override it.
    fn reg_centre_set(&mut self, _cen: &[f64], _ifrm: FrameRef) -> AstResult<()> {
        Err(AstError::new(
            ErrorCode::Inter,
            "reg_centre_set: this Region class has no centre".to_string(),
        ))
    }

    /// Return the centre of this Region, or `None` if the class has no centre.
    ///
    /// Mirrors the *query* path of C `astRegCentre` (region.c:8397 abstract),
    /// invoked with both `cen` and `ptr` NULL. The centre is returned in the
    /// base Frame for [`FrameRef::Base`] and transformed through the
    /// base→current Mapping for [`FrameRef::Current`]. Classes that are not
    /// centro-symmetric — the same set that cannot serve as an uncertainty
    /// Region (`PointList`, `Polygon`, `CmpRegion`, `NullRegion`) — return
    /// `None`, matching the C base method which returns a NULL pointer without
    /// error for the query path. `Box`, `Circle`, `Ellipse` always return a
    /// centre; `Interval` only when it is effectively a `Box`; `Prism` only
    /// when both component Regions are re-centreable.
    ///
    /// This is distinct from the geometric-centroid
    /// `RegionGeometry::reg_centre`, which
    /// always returns a value.
    fn reg_centre_query(&self, _ifrm: FrameRef) -> AstResult<Option<Vec<f64>>> {
        Ok(None)
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
    /// `astRegBasePick` (region.c:8277). The base implementation returns a
    /// same-class copy only when *all* base-Frame axes are selected, possibly
    /// in a permuted order; a strict subset yields `None`. Classes that can
    /// slice their defining geometry per-axis (Box, Interval, PointList,
    /// NullRegion) override this to support subset picks; the compound classes
    /// (CmpRegion, Prism) override it to recurse into their components.
    /// Classes whose geometry cannot be sliced axis-wise (Circle, Ellipse,
    /// Polygon) inherit this default, so an all-axes permutation still
    /// preserves the Region type.
    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        // Port of the base astRegBasePick (region.c:8277). We only proceed if
        // every base-Frame axis is selected exactly once (a genuine
        // permutation); otherwise no same-class sub-region can be formed.
        let nax = self.get_region_frameset().base().naxes();
        if axes.len() != nax {
            return Ok(None);
        }

        // Verify `axes` is a genuine permutation of 0..nax: each index appears
        // exactly once. Track whether it is the identity (a unit map).
        let mut unit = true;
        for i in 0..nax {
            let mut found = false;
            for (j, &a) in axes.iter().enumerate() {
                if a == i {
                    if found {
                        return Ok(None);
                    }
                    found = true;
                    if i != j {
                        unit = false;
                    }
                }
            }
            if !found {
                return Ok(None);
            }
        }

        // Genuine permutation: copy the Region and reset it to represent its
        // base Frame (astSetRegFS), then permute the axes if not the identity.
        let base_frame = self.get_region_frameset().base().snapshot();
        let mut result = self.clone_region();
        result.set_region_fs(base_frame.as_ref());
        if !unit {
            // C astPermAxes uses zero-based `axes`; reg_perm_axes takes a
            // 1-based permutation.
            let perm: Vec<usize> = axes.iter().map(|&a| a + 1).collect();
            result.reg_perm_axes(&perm)?;
        }
        Ok(Some(result))
    }

    /// Simplify the encapsulated FrameSet and any explicit uncertainty Region.
    ///
    /// Faithful port of the parent `Region::Simplify` (region.c:11042). Each
    /// concrete Region's [`simplified`](Mapping::simplified) invokes this first
    /// (matching C, where every subclass `Simplify` calls `(*parent_simplify)`),
    /// then continues re-fitting from the returned Region.
    ///
    /// Returns `Some(new)` when either the FrameSet or the uncertainty was
    /// simplified, and `None` when nothing changed (the caller should keep the
    /// original Region). A default uncertainty is regenerated on demand, so it
    /// needs no explicit deletion here; only an explicit uncertainty
    /// ([`test_unc`](Region::test_unc)) is simplified.
    ///
    /// The uncertainty is simplified only if its simplified form is *stable*
    /// across the extent of this Region: the simplified uncertainty is
    /// re-centred at opposite corners of a test box (this Region's base
    /// bounding box when bounded, else a box 100× the uncertainty size) and its
    /// per-axis bounding-box widths must agree to within 0.5% of their sum.
    fn parent_region_simplify(&self) -> AstResult<Option<DynRegion>> {
        // Deep-copy the supplied Region so the result differs from the input
        // when simplification occurs (region.c:11131).
        let mut new = self.clone_region();
        let mut simpler = false;

        // Simplify the encapsulated FrameSet, noting whether it changed
        // (region.c:11135-11143). FrameSet::simplified simplifies each internal
        // edge Mapping and returns a FrameSet.
        let fs = new.get_region_frameset();
        let simplified_map = <dyn Mapping>::simplify(&fs)?;
        if let Some(new_fs) = simplified_map.as_any().downcast_ref::<FrameSet>()
            && !Mapping::equal(&fs, new_fs)
        {
            *new.region_frameset_mut() = new_fs.clone();
            simpler = true;
        }

        // An explicit uncertainty is simplified in place; a default uncertainty
        // is regenerated on demand, so there is nothing to delete for it
        // (region.c:11145-11148).
        if new.test_unc() {
            // The uncertainty in this Region's base Frame (region.c:11155).
            let mut unc = new.get_unc_frm(FrameRef::Base)?;

            // The base->current Mapping of the original Region (region.c:11158).
            let map = self
                .get_region_frameset()
                .mapping(FrameRef::Base, FrameRef::Current)?;
            let nin = map.nin();
            let nout = map.nout();

            // A slicing Mapping (nin != nout, e.g. a PermMap) requires the
            // uncertainty to be re-centred on the slice: transform its current
            // centre forward then back through the base->current Mapping
            // (region.c:11165-11188).
            if nin != nout
                && let Ok(Some(cen)) = unc.reg_centre_query(FrameRef::Current)
            {
                let pset1 = PointSet::from_rows(std::iter::once(cen.as_slice()));
                let pset2 = map.transform(&pset1, true)?;
                let back = map.transform(&pset2, false)?;
                let recentre = back.point(0).to_vec();
                unc.reg_centre_set(&recentre, FrameRef::Current)?;
            }

            // Try simplifying the uncertainty. Only proceed if it changed
            // (region.c:11193-11196).
            let sunc_map = <dyn Mapping>::simplify(unc.as_ref())?;
            let unc_changed = !Mapping::equal(unc.as_ref(), sunc_map.as_ref());
            if unc_changed && let Some(mut sunc) = merge_util::mapping_into_region(sunc_map) {
                // The stability check (region.c:11213-11306): re-centre the
                // simplified uncertainty at opposite corners of a test box and
                // require its per-axis bounding-box widths to agree.
                let orig_cen = sunc.reg_centre_query(FrameRef::Base)?;
                let naxb = self.get_region_frameset().base().naxes();

                // The test box: this Region's base bounding box if bounded,
                // else a box 100× the uncertainty size centred on the
                // uncertainty centre (region.c:11221-11236).
                let (lbnd, ubnd) = if self.is_bounded() {
                    self.get_region_bounds()?
                } else {
                    let (slb, sub) = sunc.get_region_bounds()?;
                    let cen = orig_cen.clone().unwrap_or_else(|| vec![0.0; naxb]);
                    let mut lb = vec![0.0; naxb];
                    let mut ub = vec![0.0; naxb];
                    for ic in 0..naxb {
                        let delta = 0.5 * (sub[ic] - slb[ic]).abs();
                        lb[ic] = cen[ic] - delta;
                        ub[ic] = cen[ic] + delta;
                    }
                    (lb, ub)
                };

                // Re-centre at the lower corner and record the bounding-box
                // widths (region.c:11238-11246).
                sunc.reg_centre_set(&lbnd, FrameRef::Current)?;
                let (s1_lbnd, s1_ubnd) = sunc.get_region_bounds()?;

                // Re-centre at the upper corner and record the widths
                // (region.c:11248-11255).
                sunc.reg_centre_set(&ubnd, FrameRef::Current)?;
                let (s2_lbnd, s2_ubnd) = sunc.get_region_bounds()?;

                // The widths must agree to within 0.5% of their sum on every
                // axis (region.c:11260-11285). The 0.005 factor is taken
                // directly from the C source (its comment says 0.1%).
                let mut ok = true;
                for ic in 0..naxb {
                    let w1 = s1_ubnd[ic] - s1_lbnd[ic];
                    let w2 = s2_ubnd[ic] - s2_lbnd[ic];
                    if (w1 - w2).abs() > 0.005 * (w1.abs() + w2.abs()) {
                        ok = false;
                        break;
                    }
                }

                // Reinstate the original centre (region.c:11289).
                if let Some(oc) = orig_cen {
                    sunc.reg_centre_set(&oc, FrameRef::Base)?;
                }

                // If stable, install the simplified uncertainty
                // (region.c:11303-11306).
                if ok {
                    simpler = true;
                    new.set_uncertainty(sunc);
                }
            }
        }

        if simpler { Ok(Some(new)) } else { Ok(None) }
    }

    /// Copy selected properties from `that` onto this Region.
    ///
    /// Faithful port of C `astRegOverlay` (region.c:9861): copies Negated,
    /// Closed, RegionFS, and Adaptive; when the axis counts match, copies
    /// MeshSize/FillFactor only where `that` has an explicit value, otherwise
    /// clears both. When `clear_unc` is true and `that` has no *explicit*
    /// uncertainty ([`test_unc`](Region::test_unc) false), this Region's
    /// uncertainty is cleared — mirroring the C `if( unc && !astTestUnc(that) )
    /// astClearUnc(this)` branch, which is how a simplified Region built from a
    /// default-uncertainty source ends up carrying no explicit uncertainty.
    fn reg_overlay(&mut self, that: &dyn Region, clear_unc: bool) {
        if self.is_negated() != that.is_negated() {
            self.negate();
        }
        // C `astRegOverlay` copies `this->closed = that->closed` as a raw
        // field assignment, propagating the `-INT_MAX` unset sentinel; mirror
        // that by copying both the value and its explicitly-set state rather
        // than forcing the attribute set (which would spuriously emit `Closed`
        // on re-dump).
        self.set_closed_raw(that.is_closed(), that.test_closed());
        self.set_region_fs_flag(that.region_fs_flag());
        self.set_adaptive(that.is_adaptive());
        if self.naxes() == that.naxes() {
            if that.test_mesh_size() {
                self.set_mesh_size(that.mesh_size());
            }
            if that.test_fill_factor() {
                self.set_fill_factor(that.fill_factor());
            }
        } else {
            self.clear_mesh_size();
            self.clear_fill_factor();
        }
        if clear_unc && !that.test_unc() {
            self.store_uncertainty(None);
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
pub fn pick_axes_region(region: &dyn Region, axes: &[usize]) -> AstResult<PickedAxes> {
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
            // Copy properties from the old Region to the new Region
            // (region.c:7723 passes unc=0: uncertainty is left untouched).
            creg.reg_overlay(region, false);
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
    any_as_region(mapping.as_any())
}

/// Borrow a `dyn Any` as a `dyn Region` if it is a concrete Region type, else
/// return `None`. Reached where the value arrives as a `dyn Frame`, which every
/// Region also implements.
pub(crate) fn any_as_region(a: &dyn std::any::Any) -> Option<&dyn Region> {
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

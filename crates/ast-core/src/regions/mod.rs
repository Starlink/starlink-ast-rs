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
mod moc;
mod nullregion;
mod pointlist;
mod polygon;
mod prism;
mod stc;
pub use box_region::{BoxRegion, BoxRegionBuilder};
pub use circle::{Circle, CircleBuilder};
pub(crate) use cmpregion::Oper as CmpOper;
pub use cmpregion::{CmpRegion, CmpRegionBuilder};
pub use ellipse::{Ellipse, EllipseBuilder, EllipsePars};
pub use interval::{Interval, IntervalBuilder};
pub use moc::{MOC_MAX_ORDER, Moc, MocData};
pub use nullregion::{NullRegion, NullRegionBuilder};
pub use pointlist::{PointList, PointListBuilder};
pub use polygon::{PixelOper, PixelValue, Polygon, PolygonBuilder};
pub use prism::{Prism, PrismBuilder};
pub(crate) use stc::stc_set_coord_frames;
pub use stc::{
    CatalogEntryLocation, ObsDataLocation, ResourceProfile, STC_ERROR, STC_NAME, STC_PIXSZ,
    STC_RES, STC_SIZE, STC_VALUE, SearchLocation, Stc, StcBuilder, StcCatalogEntryLocation,
    StcCatalogEntryLocationBuilder, StcKind, StcObsDataLocation, StcObsDataLocationBuilder,
    StcResourceProfile, StcResourceProfileBuilder, StcSearchLocation, StcSearchLocationBuilder,
};

pub use masking::{MaskValue, mask};

#[doc(hidden)]
pub use data::FrameSetting;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::is_bad;
use crate::mappings::{DynMapping, Mapping, UnitMap};

/// An owned, dynamically-dispatched Region.
pub type DynRegion = Box<dyn Region>;

/// How two Regions overlap, as [`Region::overlap`] reports it.
///
/// # See also
///
/// `astOverlap`, which returns these as the codes 0 to 6, in the order they
/// are declared here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlap {
    /// The check could not be made, because the second Region could not be
    /// mapped into the coordinate system of the first.
    Incomparable,
    /// The Regions do not overlap.
    None,
    /// The first Region is entirely inside the second.
    FirstInside,
    /// The second Region is entirely inside the first.
    SecondInside,
    /// The Regions overlap partially.
    Partial,
    /// The Regions are identical to within their uncertainties.
    Same,
    /// The second Region is the exact negation of the first to within their
    /// uncertainties: the two Regions do not overlap and together cover the
    /// whole coordinate system.
    Negation,
}

/// Comparison operator for combining regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOperator {
    /// Boolean AND: interior is the intersection.
    And,
    /// Boolean OR: interior is the union.
    Or,
    /// Boolean XOR: interior is where exactly one of the two is inside.
    /// It is made as the OR of each Region ANDed with the negation of the
    /// other.
    // C: cmpregion.c builds XOR the same way.
    Xor,
}

/// A view of a Region as `&dyn Region`, which a default method of
/// [`Region`] cannot form from its own `&self`. Implemented for every Region.
#[doc(hidden)]
pub trait AsDynRegion {
    /// `self` as `&dyn Region`.
    fn as_dyn_region(&self) -> &dyn Region;
}

impl<T: Region> AsDynRegion for T {
    fn as_dyn_region(&self) -> &dyn Region {
        self
    }
}

/// A region within a coordinate system.
///
/// `Region` is implemented by a family of shapes, such as [`Circle`],
/// [`BoxRegion`], [`Ellipse`], [`Polygon`] and [`Interval`]; there is no
/// plain Region to construct.
/// Every Region is built with a Frame, the *encapsulated Frame*, which
/// describes the coordinate system the Region is defined in, and usually
/// with one or more positions in that Frame that fix its place and extent.
///
/// # A Region as a Frame
///
/// A Region implements [`Frame`], so it can be used wherever a Frame is
/// expected.
/// Its Frame methods act on its encapsulated Frame: [`Frame::format`], for
/// instance, formats an axis value as that Frame would.
///
/// # A Region as a Mapping
///
/// A Region also implements [`Mapping`], and transforming points through it
/// is the way to find which of many points are inside it
/// ([`point_in_region`](Region::point_in_region) tests one point).
/// A point inside the Region is passed through unchanged, and a point
/// outside it gives [`BAD_VALUE`](crate::constants::BAD_VALUE) on every
/// axis.
/// The inverse transformation does the same, so it is not a true inverse:
/// a point outside the Region transformed forward and then back gives bad
/// values, not the point.
/// [`remove_regions`] takes this masking out of a compound Mapping or
/// FrameSet by replacing each Region in it with a
/// [`UnitMap`] or its Frame.
///
/// # Changing the coordinate system
///
/// Setting a Frame attribute through a Region's [`Frame`] methods, such as
/// its System or an axis Unit, changes the coordinate system the Region
/// describes.
/// While the Region is [adaptive](Region::is_adaptive), which it is by
/// default, the area it covers is mapped into the new coordinate system,
/// so it keeps covering the same physical area.
/// For instance, a Circle defined in FK4 equatorial coordinates on the sky
/// whose System is set to FK5 then describes the same area in FK5
/// coordinates.
/// The mapping can change the shape: a circle becomes an ellipse when the
/// new system scales its axes differently, so the class of a Region does not
/// guarantee its shape in its current coordinate system.
/// Simplifying a Region (`simplify`, as for any Mapping) tries to give a
/// Region of a class that describes the shape in the current coordinate
/// system, which is not always possible.
/// Setting [`Adaptive`](Region::is_adaptive) to false makes such changes
/// leave the numbers that define the Region as they are, so the Region
/// then covers a different physical area.
///
/// [`get_region_frameset`](Region::get_region_frameset) returns the Frame
/// the Region was defined in and the Frame it now describes, with the
/// Mapping between them.
///
/// # Negation
///
/// A Region can be [negated](Region::negate), so that it covers every part
/// of its Frame except the area it was created with.
///
/// # Attributes
///
/// Besides the attributes of its encapsulated Frame, every Region has:
///
/// - [`Adaptive`](Region::is_adaptive): whether the area adapts to changes
///   in the coordinate system;
/// - [`Negated`](Region::is_negated): whether the original area has been
///   negated;
/// - [`Closed`](Region::is_closed): whether the boundary is inside the
///   Region;
/// - [`MeshSize`](Region::mesh_size): the number of points in a mesh
///   covering the Region;
/// - [`FillFactor`](Region::fill_factor): the fraction of the Region that is
///   of interest;
/// - [`Bounded`](Region::is_bounded): whether the Region is bounded
///   (read-only).
///
/// # Functions
///
/// Besides the [`Frame`] and [`Mapping`] methods, every Region has:
///
/// - [`get_region_bounds`](Region::get_region_bounds): a box containing the
///   Region;
/// - [`get_region_disc`](Region::get_region_disc): a disc containing a 2-D
///   Region;
/// - [`get_region_frame`](Region::get_region_frame): a copy of the Frame
///   the Region describes;
/// - [`get_region_frameset`](Region::get_region_frameset): a copy of the
///   FrameSet the Region holds;
/// - [`region_mesh`](Region::region_mesh) and
///   [`region_grid`](Region::region_grid): meshes of points covering the
///   boundary or the interior;
/// - [`region_points`](Region::region_points): the positions that define
///   the Region;
/// - [`get_unc`](Region::get_unc) and
///   [`set_uncertainty`](Region::set_uncertainty): the uncertainty of the
///   boundary;
/// - [`map_region`](Region::map_region): the Region in another coordinate
///   system;
/// - [`negate`](Region::negate): toggle Negated;
/// - [`overlap`](Region::overlap): how two Regions overlap;
/// - [`point_in_region`](Region::point_in_region): whether one point is
///   inside;
/// - [`show_mesh`](Region::show_mesh): print a mesh of boundary points;
/// - [`mask`]: mask the pixels of a data grid inside or outside the Region.
///
/// # Examples
///
/// Changing the unit of a 1-D Interval maps its bounds into the new unit
/// while it is adaptive, and leaves the numbers alone when it is not:
///
/// ```
/// use ast_core::frames::{BasicFrameBuilder, Frame};
/// use ast_core::regions::{IntervalBuilder, Region};
///
/// let mut frame = BasicFrameBuilder::new(1).build()?;
/// frame.set_unit(1, "Angstrom")?;
/// frame.set_active_unit(true);
///
/// let mut interval = IntervalBuilder::new(frame.clone(), vec![2000.0], vec![4000.0])?.build()?;
/// interval.set_unit(1, "nm")?;
/// let (lbnd, ubnd) = interval.get_region_bounds()?;
/// assert!((lbnd[0] - 200.0).abs() < 1e-9 && (ubnd[0] - 400.0).abs() < 1e-9);
///
/// let mut fixed = IntervalBuilder::new(frame, vec![2000.0], vec![4000.0])?.build()?;
/// fixed.set_adaptive(false);
/// fixed.set_unit(1, "nm")?;
/// let (lbnd, ubnd) = fixed.get_region_bounds()?;
/// assert_eq!((lbnd[0], ubnd[0]), (2000.0, 4000.0));
/// # Ok::<(), ast_core::error::AstError>(())
/// ```
pub trait Region: Frame + Mapping + AsDynRegion {
    // ------------------------------------------------------------------
    // Point membership
    // ------------------------------------------------------------------

    /// Whether a single point is inside this Region.
    ///
    /// `point` holds one value for each axis of the Region, in the
    /// coordinate system the Region describes.
    /// The answer takes account of [`Negated`](Region::is_negated).
    /// A point with [`BAD_VALUE`](crate::constants::BAD_VALUE) on any axis is
    /// outside.
    ///
    /// To test many points, it is usually faster to transform them all
    /// through the Region as a [`Mapping`]: a point outside the Region comes
    /// out with bad values.
    ///
    /// # Errors
    ///
    /// Passes on the error of transforming the point into the Frame the
    /// Region was defined in.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::BasicFrameBuilder;
    /// use ast_core::regions::{CircleBuilder, Region};
    ///
    /// let frame = BasicFrameBuilder::new(2).build()?;
    /// let circle = CircleBuilder::new(frame, vec![0.0, 0.0])?.radius(1.0).build()?;
    /// assert!(circle.point_in_region(&[0.5, 0.5])?);
    /// assert!(!circle.point_in_region(&[2.0, 0.0])?);
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    #[doc(alias = "astPointInRegion")]
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool>;

    /// How this Region and `that` overlap.
    ///
    /// The two Regions are converted to a common coordinate system first.
    /// If that is not possible, for instance because they describe areas in
    /// different Domains, the result is [`Overlap::Incomparable`].
    /// [`Overlap::FirstInside`] means this Region is inside `that`, and
    /// [`Overlap::SecondInside`] the reverse.
    /// [`Overlap::Same`] and [`Overlap::Negation`] do not take account of
    /// the [`Closed`](Region::is_closed) attribute of either Region.
    ///
    /// The check tests a mesh of points on the boundary of one Region
    /// against the other, so its reliability depends on
    /// [`MeshSize`](Region::mesh_size).
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Inter`] if neither Region has a finite boundary, or if
    ///   the overlap cannot be determined from the boundary of either.
    /// - Otherwise passes on the errors of converting the Regions and of
    ///   meshing their boundaries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::BasicFrameBuilder;
    /// use ast_core::regions::{CircleBuilder, Overlap, Region};
    ///
    /// let frame = BasicFrameBuilder::new(2).build()?;
    /// let big = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])?.radius(2.0).build()?;
    /// let small = CircleBuilder::new(frame.clone(), vec![0.5, 0.0])?.radius(0.5).build()?;
    /// let apart = CircleBuilder::new(frame, vec![5.0, 0.0])?.radius(1.0).build()?;
    /// assert_eq!(small.overlap(&big)?, Overlap::FirstInside);
    /// assert_eq!(big.overlap(&small)?, Overlap::SecondInside);
    /// assert_eq!(big.overlap(&apart)?, Overlap::None);
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    // C: region.c `Overlap`, which dispatches on the class of `that` through
    // `overlap_x` (region.c `OverlapX`); a class with its own algorithm for
    // the case where it is the first Region overrides this.
    #[doc(alias = "astOverlap")]
    fn overlap(&self, that: &dyn Region) -> AstResult<Overlap> {
        that.overlap_x(self.as_dyn_region())
    }

    /// The overlap of `this` with this Region, `astOverlap(this, self)`,
    /// computed by the class of the second Region (region.c `OverlapX`).
    ///
    /// The default is the generic algorithm, which meshes the boundary of
    /// one Region and tests the mesh against the other. A class whose
    /// boundary cannot be meshed, or which knows the answer directly,
    /// overrides it.
    #[doc(hidden)]
    fn overlap_x(&self, this: &dyn Region) -> AstResult<Overlap> {
        overlap::region_overlap_x(self.as_dyn_region(), this)
    }

    /// A mesh on the part of this Region's boundary inside the current-Frame
    /// box `[lbnd, ubnd]`, in the base Frame (region.c `BndBaseMesh`). A
    /// boundary that does not reach the box gives one bad point.
    ///
    /// The generic overlap uses it when this Region has no finite boundary
    /// mesh of its own. The default intersects the Region with the box; a
    /// class whose `overlap` cannot handle that box overrides it.
    #[doc(hidden)]
    fn bnd_base_mesh(&self, lbnd: &[f64], ubnd: &[f64]) -> AstResult<PointSet> {
        overlap::region_bnd_base_mesh(self.as_dyn_region(), lbnd, ubnd)
    }

    // ------------------------------------------------------------------
    // Negation
    // ------------------------------------------------------------------

    /// A copy of this Region with [`Negated`](Region::is_negated) toggled.
    fn negated_copy(&self) -> DynRegion;

    /// Negate the area this Region represents, in place.
    ///
    /// Points that were inside the Region are then outside it, and points
    /// that were outside are inside.
    /// This toggles [`Negated`](Region::is_negated), which is then set.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::BasicFrameBuilder;
    /// use ast_core::regions::{CircleBuilder, Region};
    ///
    /// let frame = BasicFrameBuilder::new(2).build()?;
    /// let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])?.radius(1.0).build()?;
    /// circle.negate();
    /// assert!(circle.is_negated());
    /// assert!(!circle.point_in_region(&[0.0, 0.0])?);
    /// assert!(circle.point_in_region(&[2.0, 0.0])?);
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    #[doc(alias = "astNegate")]
    fn negate(&mut self);

    /// The Negated attribute: whether this Region represents the outside of
    /// the area it was created with rather than the inside.
    ///
    /// [`negate`](Region::negate) toggles it.
    /// Whether the boundary is inside the Region is controlled by
    /// [`Closed`](Region::is_closed), which negation does not change: the
    /// boundary of a closed Region is inside it whether or not it is
    /// negated.
    ///
    /// Default: false.
    ///
    /// - Stc: the Negated attribute of the Region it encapsulates.
    #[doc(alias = "Negated")]
    fn is_negated(&self) -> bool;

    /// Whether [`Negated`](Region::is_negated) has been set.
    /// Negating a Region sets it.
    fn test_negated(&self) -> bool;

    /// Store `Negated` together with whether it is set.
    #[doc(hidden)]
    // C: the raw field assignment `this->negated = that->negated`.
    fn set_negated_raw(&mut self, negated: bool, set: bool);

    /// Set [`Negated`](Region::is_negated).
    fn set_negated(&mut self, negated: bool) {
        self.set_negated_raw(negated, true);
    }

    /// Clear [`Negated`](Region::is_negated), reverting to its default of
    /// false.
    fn clear_negated(&mut self) {
        self.set_negated_raw(false, false);
    }

    /// The Negated value held in this Region's own fields and whether it is
    /// set, C's `this->negated`, which region.c `RegOverlay` and `Dump` read
    /// directly. It is what [`is_negated`](Region::is_negated) and
    /// [`test_negated`](Region::test_negated) report, except in a class
    /// whose Negated accessors read another Region (Stc).
    #[doc(hidden)]
    fn raw_negated(&self) -> (bool, bool) {
        (self.is_negated(), self.test_negated())
    }

    /// The Closed value held in this Region's own fields and whether it is
    /// set, C's `this->closed`; see [`raw_negated`](Region::raw_negated).
    #[doc(hidden)]
    fn raw_closed(&self) -> (bool, bool) {
        (self.is_closed(), self.test_closed())
    }

    // ------------------------------------------------------------------
    // Attributes: Closed, FillFactor, MeshSize, Bounded, Adaptive
    // ------------------------------------------------------------------

    /// The Closed attribute: whether points on the boundary of this Region
    /// are inside it.
    ///
    /// A closed Region contains its boundary, and an open one does not.
    ///
    /// Default: true.
    ///
    /// - PointList: the attribute is ignored; an un-negated PointList is
    ///   always closed and a negated one always open, since points have no
    ///   volume and are all boundary.
    /// - CmpRegion: a CmpRegion is built with the Closed value of its first
    ///   component Region, if that has been set; setting or clearing it also
    ///   sets or clears it on both component Regions.
    /// - Stc: the Closed attribute of the Region it encapsulates.
    /// - Moc: the attribute is ignored, and whether a point on the boundary
    ///   is inside is undefined.
    #[doc(alias = "Closed")]
    fn is_closed(&self) -> bool;

    /// Set [`Closed`](Region::is_closed).
    fn set_closed(&mut self, closed: bool);

    /// Whether [`Closed`](Region::is_closed) has been set.
    fn test_closed(&self) -> bool;

    /// Store [`Closed`](Region::is_closed) together with whether it is set.
    ///
    /// Unlike [`set_closed`](Region::set_closed), this can leave the
    /// attribute unset, and it does not pass the value on to any component
    /// Regions.
    // C: the raw field assignment `this->closed = that->closed`, which
    // copies the `-INT_MAX` unset sentinel.
    fn set_closed_raw(&mut self, closed: bool, set: bool);

    /// Clear [`Closed`](Region::is_closed), reverting to its default of true.
    fn clear_closed(&mut self) {
        self.set_closed_raw(true, false);
    }

    /// The FillFactor attribute: the fraction of this Region that is of
    /// interest.
    ///
    /// It is not used by any calculation in this crate; it could record, for
    /// instance, the fraction of the Region for which data is available.
    /// Its value lies in the range 0.0 to 1.0.
    ///
    /// Default: 1.0.
    ///
    /// - CmpRegion: the default is the FillFactor of its first component
    ///   Region.
    /// - Prism: the default is the product of the FillFactors of its two
    ///   component Regions.
    /// - Stc: the FillFactor of the Region it encapsulates.
    #[doc(alias = "FillFactor")]
    fn fill_factor(&self) -> f64;

    /// Set [`FillFactor`](Region::fill_factor).
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Atser`] if `factor` is outside the range 0.0 to 1.0; the
    /// attribute is then unchanged.
    // C: region.c `astSetFillFactor`, region.c:12669.
    fn set_fill_factor(&mut self, factor: f64) -> AstResult<()>;

    /// Whether [`FillFactor`](Region::fill_factor) has been set.
    fn test_fill_factor(&self) -> bool;

    /// Clear [`FillFactor`](Region::fill_factor), reverting to its default.
    fn clear_fill_factor(&mut self);

    /// The MeshSize attribute: the number of points in a mesh covering the
    /// boundary or the interior of this Region.
    ///
    /// [`region_mesh`](Region::region_mesh) and
    /// [`region_grid`](Region::region_grid) give such meshes.
    /// [`overlap`](Region::overlap) also tests each point of a boundary mesh
    /// of one Region against the other, so a low value can let an overlap go
    /// unnoticed, and a high one makes the test slow.
    /// A value below 5 is stored as 5.
    ///
    /// Default: 200 for a 2-D Region, 2000 for a Region of three or more
    /// dimensions, and 2 for a 1-D Region, whose boundary is two points.
    ///
    /// - CmpRegion: a CmpRegion is built with the MeshSize of its first
    ///   component Region, if that has been set; setting or clearing it also
    ///   sets or clears it on both component Regions.
    /// - Moc: a mesh covering the boundary has a point at the outer corners
    ///   of every HEALPix cell, at the Moc's maximum order, that touches the
    ///   boundary, whatever the MeshSize; a mesh covering the interior uses
    ///   MeshSize as other classes do.
    /// - Stc: the MeshSize of the Region it encapsulates.
    // C: region.c `astGetMeshSize`, `astSetMeshSize`.
    #[doc(alias = "MeshSize")]
    fn mesh_size(&self) -> usize;

    /// Set [`MeshSize`](Region::mesh_size).
    fn set_mesh_size(&mut self, size: usize);

    /// Whether [`MeshSize`](Region::mesh_size) has been set.
    fn test_mesh_size(&self) -> bool;

    /// Clear [`MeshSize`](Region::mesh_size), reverting to its default.
    fn clear_mesh_size(&mut self);

    /// The Bounded attribute: whether this Region lies entirely within some
    /// finite box.
    ///
    /// It is read-only.
    ///
    /// - Box, Circle, Ellipse, NullRegion and PointList: bounded unless
    ///   negated, and always bounded in a sky Frame, where the complement of
    ///   a finite area is finite too.
    /// - Moc: always bounded, since it is defined on the sky.
    /// - Polygon: always bounded in a sky Frame; otherwise it depends on the
    ///   order of the vertices and on negation.
    /// - Interval: bounded when every axis has both limits and none is an
    ///   excluded interval (a lower limit above the upper one); Negated is
    ///   not consulted.
    /// - CmpRegion and Prism: worked out from their component Regions.
    /// - Stc: whether the Region it encapsulates is bounded.
    ///
    /// # Errors
    ///
    /// Passes on the errors of the class's test, which for compound Regions
    /// may need their component Regions in a common Frame.
    #[doc(alias = "Bounded")]
    fn is_bounded(&self) -> AstResult<bool>;

    /// The Adaptive attribute: whether the area this Region covers adapts to
    /// changes in its coordinate system.
    ///
    /// The coordinate system a Region describes changes when a Frame
    /// attribute, such as System or an axis Unit, is set through its
    /// [`Frame`] methods.
    /// A Region on the sky in ICRS coordinates, for instance, may have its
    /// System set to Galactic.
    ///
    /// While Adaptive is true, the numbers that define the area are mapped
    /// from the old coordinate system into the new one, so the Region goes on
    /// representing the same physical area.
    /// A Region covering 2000 to 4000 Angstrom whose Unit is changed to nm
    /// then covers 200 to 400 nm.
    ///
    /// While Adaptive is false, the numbers are left as they are, so the
    /// physical area usually changes: the same Region would cover 2000 to
    /// 4000 nm.
    /// This is how to correct an attribute that was wrong when the Region
    /// was made.
    /// A Region made before its Epoch was known, for instance, would be
    /// mapped from the default Epoch to the correct one if the Epoch were
    /// set while it is adaptive, so Adaptive should be set false for the
    /// change.
    ///
    /// Default: true.
    ///
    /// See the [`Region`] example.
    #[doc(alias = "Adaptive")]
    fn is_adaptive(&self) -> bool;

    /// Whether [`Adaptive`](Region::is_adaptive) has been set.
    fn test_adaptive(&self) -> bool;

    /// Store `Adaptive` together with whether it is set.
    #[doc(hidden)]
    // C: the raw field assignment `this->adaptive = that->adaptive`.
    fn set_adaptive_raw(&mut self, adaptive: bool, set: bool);

    /// Set [`Adaptive`](Region::is_adaptive).
    fn set_adaptive(&mut self, adaptive: bool) {
        self.set_adaptive_raw(adaptive, true);
    }

    /// Clear [`Adaptive`](Region::is_adaptive), reverting to its default of
    /// true.
    fn clear_adaptive(&mut self) {
        self.set_adaptive_raw(true, false);
    }

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

    /// The bounds of a box that just contains this Region, as
    /// `(lower, upper)`, each with one value per axis.
    ///
    /// The bounds are axis values in the Frame the Region represents.
    /// [`Negated`](Region::is_negated) is ignored: the box contains the
    /// Region as it would be if it were not negated.
    /// An axis with no lower limit gives `-f64::MAX`, the same value as
    /// [`BAD_VALUE`](crate::constants::BAD_VALUE), and one with no upper
    /// limit gives `f64::MAX`.
    /// An axis on which the Region has no extent gives a lower bound above
    /// its upper bound, which differs from an axis on which the Region has a
    /// constant value: both bounds are then that value.
    /// An axis whose bounds cannot be determined gives `BAD_VALUE` for both.
    ///
    /// # Errors
    ///
    /// Passes on the errors of meshing the boundary and of transforming the
    /// mesh into the Frame the Region represents.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::BasicFrameBuilder;
    /// use ast_core::regions::{CircleBuilder, Region};
    ///
    /// let frame = BasicFrameBuilder::new(2).build()?;
    /// let circle = CircleBuilder::new(frame, vec![1.0, 2.0])?.radius(1.0).build()?;
    /// let (lbnd, ubnd) = circle.get_region_bounds()?;
    /// assert_eq!((lbnd, ubnd), (vec![0.0, 1.0], vec![2.0, 3.0]));
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    #[doc(alias = "astGetRegionBounds")]
    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)>;

    /// Bounding box of this Region in its *base* Frame.
    ///
    /// Mirrors C `astRegBaseBox` (region.c:7855). The Negated attribute is
    /// ignored, and the bounds are un-mapped: the base→current Mapping is not
    /// applied. Where the two Frames coincide this is the same box
    /// [`get_region_bounds`](Region::get_region_bounds) returns.
    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)>;

    /// Bounding box of the *un-negated* region in the base Frame, ignoring the
    /// Negated attribute of any component Regions.
    ///
    /// Mirrors C `astRegBaseBox2` (region.c:7909). The base-class behaviour is
    /// identical to [`region_base_box`](Region::region_base_box); only compound
    /// classes (CmpRegion) override it so the un-negated assumption is
    /// propagated to their components.
    fn reg_base_box2(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.region_base_box()
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
            let mb = smap.map_box(&lbndb, &ubndb, true, i + 1)?;
            *lo = mb.lbnd_out;
            *hi = mb.ubnd_out;
        }
        Ok((lbnd, ubnd))
    }

    /// The center and radius of a disc that just contains this 2-D Region,
    /// as `(center, radius)`.
    ///
    /// The center is a pair of axis values in the Frame the Region
    /// represents, and the radius is a distance as that Frame measures it,
    /// an arc on the sky.
    /// [`Negated`](Region::is_negated) is ignored: the disc contains the
    /// Region as it would be if it were not negated.
    /// An unbounded Region gives [`BAD_VALUE`](crate::constants::BAD_VALUE)
    /// for the center and the radius.
    /// It is [`region_disc`] applied to this Region.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Invar`] if the Region is not 2-dimensional.
    /// - Otherwise passes on the errors of [`region_disc`].
    // C: region.c `GetRegionDisc`, which no class overrides.
    #[doc(alias = "astGetRegionDisc")]
    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        region_disc(self.as_dyn_region())
    }

    /// A mesh of points on the boundary of this Region, in the Frame it was
    /// defined in (the base Frame of its FrameSet).
    ///
    /// The number of points is about [`MeshSize`](Region::mesh_size).
    /// [`region_mesh`](Region::region_mesh) gives the mesh in the Frame the
    /// Region represents.
    fn get_region_mesh(&self) -> AstResult<PointSet>;

    /// A mesh of points covering the boundary of this Region, in the Frame
    /// it represents.
    ///
    /// The number of points is set by [`MeshSize`](Region::mesh_size).
    /// If the coordinate system has changed since the Region was made, the
    /// points are in the new system; a non-linear change can distort the
    /// shape, so it may not match the class of the Region.
    /// The points are normalized as one group
    /// ([`Frame::norm_points`]), so the mesh of a Region on the sky that
    /// crosses zero longitude does not jump by 2 pi: it includes longitudes
    /// below zero or above 2 pi instead.
    /// [`region_grid`](Region::region_grid) fills the interior instead.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Mbbnf`] if the Region is unbounded.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::BasicFrameBuilder;
    /// use ast_core::regions::{CircleBuilder, Region};
    ///
    /// let frame = BasicFrameBuilder::new(2).build()?;
    /// let mut circle = CircleBuilder::new(frame, vec![0.0, 0.0])?.radius(1.0).build()?;
    /// circle.set_mesh_size(20);
    /// let mesh = circle.region_mesh()?;
    /// assert_eq!(mesh.npoint(), 20);
    /// for i in 0..mesh.npoint() {
    ///     let p = mesh.point(i);
    ///     assert!((p[0].hypot(p[1]) - 1.0).abs() < 1e-9);
    /// }
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    ///
    /// # See also
    ///
    /// `astGetRegionMesh`, with its `surface` argument set.
    // C: region.c `GetRegionMesh` with `surface` set: the current-Frame
    // boundary mesh (`reg_mesh`) normalized by `norm_points` as one group.
    #[doc(alias = "astGetRegionMesh")]
    fn region_mesh(&self) -> AstResult<PointSet> {
        self.bounded_for_mesh()?;
        self.norm_points(&self.reg_mesh()?, true)
    }

    /// A mesh of points filling the interior of this Region, in the Frame it
    /// represents.
    ///
    /// The number of points is about [`MeshSize`](Region::mesh_size).
    /// The points are normalized as for [`region_mesh`](Region::region_mesh),
    /// which covers the boundary instead.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Mbbnf`] if the Region is unbounded.
    ///
    /// # See also
    ///
    /// `astGetRegionMesh`, with its `surface` argument unset.
    // C: region.c `GetRegionMesh` with `surface` unset: the current-Frame
    // grid (`reg_grid`) normalized as one group.
    #[doc(alias = "astGetRegionMesh")]
    fn region_grid(&self) -> AstResult<PointSet> {
        self.bounded_for_mesh()?;
        self.norm_points(&self.reg_grid()?, true)
    }

    /// Write to standard output a table of the axis values at a mesh of
    /// points covering the boundary of this Region.
    ///
    /// The table is [`show_mesh_text`](Region::show_mesh_text): the title
    /// `ttl`, if given, then one row of tab-separated axis values per point,
    /// one for each axis of the Frame the Region represents, then a line
    /// holding `ENDMESH`.
    /// The values are formatted by each axis's Format when `format` is true,
    /// and as plain floating point values otherwise.
    /// The number of points is set by [`MeshSize`](Region::mesh_size).
    ///
    /// # Errors
    ///
    /// Passes on the errors of meshing the boundary and of formatting the
    /// values.
    // C: region.c `ShowMesh`.
    #[doc(alias = "astShowMesh")]
    fn show_mesh(&self, format: bool, ttl: Option<&str>) -> AstResult<()> {
        print!("{}", self.show_mesh_text(format, ttl)?);
        Ok(())
    }

    /// The table [`show_mesh`](Region::show_mesh) writes.
    ///
    /// It is the title, if any, between blank lines; one line per point of
    /// the boundary mesh in the Frame the Region represents, normalized as
    /// one group, giving each axis value followed by a tab; then `ENDMESH`
    /// and a blank line.
    /// A bad value is `<bad>`; otherwise a value is formatted by the axis
    /// Format when `format` is true, and as the `%g` format gives it when it
    /// is not.
    /// The number of points is set by [`MeshSize`](Region::mesh_size).
    ///
    /// # Errors
    ///
    /// Passes on the errors of meshing the boundary and of formatting the
    /// values.
    // C: region.c `ShowMesh`.
    fn show_mesh_text(&self, format: bool, ttl: Option<&str>) -> AstResult<String> {
        let mesh = self.norm_points(&self.reg_mesh()?, true)?;
        let mut out = String::new();
        if let Some(ttl) = ttl {
            out.push_str(&format!("\n{ttl}\n\n"));
        }
        for j in 0..mesh.npoint() {
            for (i, &value) in mesh.point(j).iter().enumerate() {
                if value == crate::constants::BAD_VALUE {
                    out.push_str("<bad>");
                } else if format {
                    out.push_str(&self.format(i + 1, value)?);
                } else {
                    out.push_str(&crate::frames::axis_format::format_value("%g", 6, value)?);
                }
                out.push('\t');
            }
            out.push('\n');
        }
        out.push_str("ENDMESH\n\n");
        Ok(out)
    }

    /// `AST__MBBNF` unless the region is bounded, as `astGetRegionMesh`
    /// requires.
    #[doc(hidden)]
    fn bounded_for_mesh(&self) -> AstResult<()> {
        if self.is_bounded()? {
            Ok(())
        } else {
            let class = class_name(self.as_any());
            Err(AstError::new(
                ErrorCode::Mbbnf,
                format!(
                    "astGetRegionMesh({class}): The supplied {class} is unbounded so no \
                     mesh can be created to cover it."
                ),
            ))
        }
    }

    /// The base-Frame boundary mesh transformed into the current Frame by
    /// the simplified base->current Mapping (region.c `RegMesh`).
    #[doc(hidden)]
    fn reg_mesh(&self) -> AstResult<PointSet> {
        self.to_current_frame(self.get_region_mesh()?)
    }

    /// The base-Frame grid transformed into the current Frame by the
    /// simplified base->current Mapping (region.c `RegGrid`).
    #[doc(hidden)]
    fn reg_grid(&self) -> AstResult<PointSet> {
        self.to_current_frame(self.get_region_grid()?)
    }

    /// Base-Frame points in the current Frame, by the simplified
    /// base->current Mapping (region.c `RegMapping`); unchanged when that
    /// Mapping is a UnitMap.
    #[doc(hidden)]
    fn to_current_frame(&self, base: PointSet) -> AstResult<PointSet> {
        let map = self
            .get_region_frameset()
            .mapping(FrameRef::Base, FrameRef::Current)?
            .simplify()?;
        if map.as_any().is::<UnitMap>() {
            Ok(base)
        } else {
            map.transform(&base, true)
        }
    }

    /// A grid of about [`mesh_size()`](Region::mesh_size) points spread
    /// through this region in the base frame. Mirrors C `astRegBaseGrid`, a
    /// per-class method: a Box spaces the points over itself with its base
    /// Frame's `astFrameGrid`; every other class keeps the points of a
    /// covering Box's grid that it contains, adjusting the Box's grid size
    /// up to three times to bring the count near MeshSize (region.c). Fails
    /// with `AST__MBBNF` for an unbounded region.
    fn get_region_grid(&self) -> AstResult<PointSet>;

    /// The positions that define this Region, in the Frame it was defined
    /// in (the base Frame of its FrameSet), as the Region stores them.
    ///
    /// [`region_points`](Region::region_points) gives them in the Frame the
    /// Region represents, and says what they are for each class.
    // C: the Region's `points` PointSet.
    fn get_region_points(&self) -> AstResult<PointSet>;

    /// The positions that define this Region, in the Frame it represents.
    ///
    /// If the coordinate system has changed since the Region was made, the
    /// points are in the new system; a non-linear change can distort the
    /// shape, so it may not match the class of the Region.
    /// What the points are depends on the class:
    ///
    /// - Box: the center, then a corner.
    /// - Circle: the center, then a point on the circumference.
    /// - Ellipse: the center, then the end of one axis of the ellipse, then
    ///   another point on the circumference.
    /// - Interval: the lower bounds, then the upper bounds, as it was made
    ///   with them; on an axis with an excluded interval the lower bound is
    ///   above the upper one.
    /// - PointList: the points it was made with.
    /// - Polygon: the vertices it was made with.
    /// - CmpRegion, Prism, NullRegion, Moc and Stc: no points; the component
    ///   Regions of a CmpRegion or Prism have points of their own.
    ///
    /// # Errors
    ///
    /// Passes on the error of transforming the points.
    ///
    /// # See also
    ///
    /// `astGetRegionPoints`.
    // C: region.c `GetRegionPoints`: the stored base-Frame points
    // transformed by the FrameSet.
    #[doc(alias = "astGetRegionPoints")]
    fn region_points(&self) -> AstResult<PointSet> {
        self.get_region_frameset()
            .transform(&self.get_region_points()?, true)
    }

    // ------------------------------------------------------------------
    // Frame access
    // ------------------------------------------------------------------

    /// A copy of the Frame this Region represents.
    ///
    /// Changing the copy has no effect on the Region; to change the Region,
    /// set the attributes through its own [`Frame`] methods.
    #[doc(alias = "astGetRegionFrame")]
    fn get_region_frame(&self) -> DynFrame;

    /// A copy of the FrameSet this Region holds.
    ///
    /// Its base Frame is the Frame the Region was defined in, and its current
    /// Frame is the Frame the Region now represents, the same Frame as
    /// [`get_region_frame`](Region::get_region_frame) gives.
    /// Changing the copy has no effect on the Region.
    #[doc(alias = "astGetRegionFrameSet")]
    fn get_region_frameset(&self) -> FrameSet;

    /// Mutable access to the encapsulated FrameSet.
    ///
    /// Used by the shared [`reg_map_region`](Region::reg_map_region) default to
    /// perform the base→current edge surgery C's `astMapRegion` does on
    /// `result->frameset` directly. Every concrete class returns
    /// `&mut self.data.frameset`.
    #[doc(hidden)]
    fn region_frameset_mut(&mut self) -> &mut FrameSet;

    /// C's `astMask<X>` for this Region: region.c `Mask<X>` by default,
    /// which PointList overrides. See [`mask`](crate::regions::mask).
    #[doc(hidden)]
    fn mask_data(
        &self,
        map: Option<&dyn Mapping>,
        inside: bool,
        lbnd: &[i64],
        ubnd: &[i64],
        data: masking::MaskData<'_>,
    ) -> AstResult<usize> {
        masking::region_mask(self.as_dyn_region(), map, inside, lbnd, ubnd, data)
    }

    /// Make a change to a Frame attribute without remapping the Region, as
    /// region.c `RegSetAttrib` and `RegClearAttrib` do: to the current Frame
    /// and to the base Frame (translating an axis index between them), and to
    /// the uncertainty. Returns the change as made to the base Frame.
    ///
    /// The change is the same closure whether it sets or clears, so this one
    /// method is both C methods. A Region holding other Regions in its base
    /// Frame overrides it to pass the base-Frame change on to them, as C's
    /// CmpRegion and Prism do.
    #[doc(hidden)]
    fn reg_frame_attrib<'a>(&mut self, setting: &FrameSetting<'a>) -> AstResult<FrameSetting<'a>>;

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
    fn set_region_fs_flag(&mut self, value: bool) {
        self.set_region_fs_raw(value, true);
    }

    /// Whether `RegionFS` has been explicitly set (C `TestRegionFS`).
    #[doc(hidden)]
    fn test_region_fs(&self) -> bool;

    /// Store `RegionFS` together with its explicitly-set state, as C's raw
    /// field assignment `this->regionfs = that->regionfs` does.
    #[doc(hidden)]
    fn set_region_fs_raw(&mut self, value: bool, set: bool);

    /// Clear `RegionFS`, reverting to its default of true (C
    /// `astClearRegionFS`).
    #[doc(hidden)]
    fn clear_region_fs(&mut self) {
        self.set_region_fs_raw(true, false);
    }

    /// The current value of the boolean `RegionFS` attribute (whether the
    /// encapsulated FrameSet is included in a dump). Mirrors reading C's
    /// `regionfs` field, used by [`reg_overlay`](Region::reg_overlay).
    #[doc(hidden)]
    fn region_fs_flag(&self) -> bool;

    /// Clear `RegionFS` on this Region and on every Region nested inside it.
    ///
    /// C's Region `Copy` ends with `astClearRegionFS( out )`
    /// (region.c:12936-12940), and each Region nested in the copy is itself a
    /// copy, so the clear reaches all of them. A cleared `RegionFS` yields the
    /// default of 1, so a deep copy always dumps its own Frame — which is what
    /// restores the Frame on a component Region that had it suppressed.
    ///
    /// This deliberately is not part of [`Clone`]: the port also clones where C
    /// takes a new reference with `astClone`, and those must leave `RegionFS`
    /// alone. Compound classes override this to reach their component Regions.
    fn clear_region_fs_deep(&mut self) {
        self.clear_region_fs();
        if let Some(mut unc) = self.get_uncertainty() {
            unc.clear_region_fs_deep();
            self.store_uncertainty(Some(unc));
        }
    }

    /// A deep copy of this Region, as C's `astCopy` produces one.
    ///
    /// [`clone_region`](Region::clone_region) alone is the port of `astClone` —
    /// a second reference to the same Region. `astCopy` differs in one
    /// observable way: it clears `RegionFS` throughout the copy
    /// (region.c:12936-12940), so use this wherever the C source says
    /// `astCopy`.
    fn copy_region(&self) -> DynRegion {
        let mut out = self.clone_region();
        out.clear_region_fs_deep();
        out
    }

    /// Give this Region a new uncertainty.
    ///
    /// Every Region has an uncertainty, the uncertainty of the positions on
    /// its boundary, which is itself a Region.
    /// The uncertainty of a boundary point is the area the uncertainty
    /// Region covers when it is shifted to be centered on that point; it is
    /// taken to be the same for every point.
    /// An uncertainty is usually given when a Region is built, but this
    /// replaces it at any time.
    /// A Region with none set uses a default (see
    /// [`get_unc`](Region::get_unc)).
    ///
    /// `unc` must be of a class that has a center, that is, one whose every
    /// instance is centro-symmetric: a Box, Circle or Ellipse, an Interval
    /// equivalent to a Box, or a Prism of such Regions.
    /// It is mapped into the Frame this Region was defined in, so its Frame
    /// must be convertible to that one, and it is centered on the first point
    /// that defines this Region.
    /// An unbounded `unc` is negated, which makes it bounded.
    /// Later changes to the `unc` passed in have no effect on this Region.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Ncpin`] if `unc` has no center, or its Frame cannot be
    ///   converted to the Frame this Region was defined in.
    ///   This Region then has no uncertainty set.
    /// - Otherwise passes on the errors of converting and mapping `unc`.
    // C: region.c `SetUnc` (region.c:10754). The body is fully generic: it
    // touches only the FrameSet, defining points, uncertainty field and
    // cache, so this is a shared default. Every route that installs an
    // uncertainty goes through here, as in C: the builders (C's
    // constructor, region.c:13372), the simplifier (region.c:11303-11305)
    // and the cross-class merges. The one exception is the loader, which
    // stores what the Channel read verbatim (region.c:13559), so reading a
    // Region back is an identity. Ncpin is the status C's second `astError`
    // leaves, and C annuls the old uncertainty first. The uncertainty is
    // tagged to omit its FrameSet from dumps when its base->current Mapping
    // simplifies to a UnitMap.
    #[doc(alias = "astSetUnc")]
    fn set_uncertainty(&mut self, unc: DynRegion) -> AstResult<()> {
        // C annuls any existing uncertainty before validating the new one
        // (region.c:10826-10832), so every failure below leaves the Region
        // with none.
        if self.get_uncertainty().is_some() {
            self.store_uncertainty(None);
        }

        // The supplied Region must be of a re-centreable (centro-symmetric)
        // class. C tests this via astRegCentre(unc, NULL, NULL, 0, CURRENT)
        // returning non-NULL (region.c:10836); its errors propagate.
        if unc.reg_centre_query(FrameRef::Current)?.is_none() {
            return Err(AstError::new(
                ErrorCode::Ncpin,
                "astSetUnc: Bad uncertainty shape supplied.",
            )
            .with_context(
                "The uncertainty Region must be an instance of a centro-symetric \
                 subclass of Region (e.g. Box, Circle, Ellipse, etc).",
            ));
        }

        // Map the supplied uncertainty into this Region's base Frame. C
        // inverts this->frameset, runs Conv(unc->frameset, ...), then
        // astMapRegion (region.c:10840-10850). `Conv` converts FrameSet to
        // FrameSet, so any Frame of the uncertainty's FrameSet may match, and
        // aligns in the Domain of the current Frame of the inverted FrameSet,
        // which is this Region's base Frame (region.c:2367-2370).
        let mut unc = unc;
        let mut to = self.get_region_frameset();
        to.invert();
        let domain = to.current().domain().to_string();
        let converted = unc.get_region_frameset().convert_to(&to, &[&domain])?;

        // `Conv` re-instates each FrameSet's original base Frame index with
        // astSetBase (region.c:2373-2374). That restores the index it read,
        // but leaves the attribute *set* even when it was unset on entry, so
        // the uncertainty's FrameSet from here on dumps `Base` as an active
        // card. The other FrameSet `Conv` restores is this Region's own, which
        // is inverted across the call, so there astSetBase writes the Current
        // slot instead (frameset.c:9836) — an index this port always dumps as
        // active, so it needs no flag of its own.
        unc.region_frameset_mut().set_base(FrameRef::Base)?;

        // Frames that cannot be converted are an error (region.c:10889-10896).
        let Some(conv) = converted else {
            let title = unc.title()?;
            return Err(AstError::new(
                ErrorCode::Ncpin,
                format!(
                    "astSetUnc: Bad {} dimensional uncertainty Frame ({} {}) supplied.",
                    unc.naxes(),
                    unc.domain(),
                    title
                ),
            )
            .with_context("Cannot convert it to the Frame of the new Region."));
        };
        let map = conv.mapping(FrameRef::Base, FrameRef::Current)?;
        let frm = conv.current().snapshot();
        let mut new_unc = unc.reg_map_region(map.as_ref(), frm.as_ref())?;

        // Ensure the uncertainty is bounded. Negating an unbounded
        // uncertainty makes it bounded because it consists of Circles, Boxes
        // and/or Ellipses (region.c:10853-10856).
        if !new_unc.is_bounded()? {
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
            new_unc.reg_centre_set(&first, FrameRef::Current)?;
        }

        self.store_uncertainty(Some(new_unc));
        Ok(())
    }

    /// A copy of the uncertainty Region set on this Region, if one has been
    /// set, as the Region stores it: in the Frame the Region was defined in.
    ///
    /// [`get_unc`](Region::get_unc) gives the uncertainty in the Frame the
    /// Region represents.
    fn get_uncertainty(&self) -> Option<DynRegion>;

    /// The uncertainty of positions in this Region, as a Region in the Frame
    /// this Region represents.
    ///
    /// See [`set_uncertainty`](Region::set_uncertainty) for what an
    /// uncertainty is.
    /// If an uncertainty has been set, it is returned, mapped into the
    /// current coordinate system of this Region if that has changed since
    /// it was set (by setting a Frame attribute such as System, or by
    /// [`map_region`](Region::map_region)).
    /// If none has been set, `def` true gives the default uncertainty that
    /// is used in its place, and `def` false gives `None`.
    ///
    /// The default uncertainty is:
    ///
    /// - most classes: a Box 1.0E-6 of the size of the Region's bounding
    ///   box on each axis, centered on the box; on an axis where the box has
    ///   no width, 1.0E-6 of the axis value, or 1.0E-12 where that value is
    ///   zero;
    /// - CmpRegion: the uncertainty set on its first component Region, or
    ///   else the one set on its second, or else as for most classes;
    /// - Prism: the uncertainties of its two component Regions combined, each
    ///   the one set or else that component's default;
    /// - Interval: as for most classes, measured on the Interval's own
    ///   limits;
    /// - NullRegion: a Circle of zero radius at the origin.
    ///
    /// # Errors
    ///
    /// Passes on the errors of building the default and of mapping the
    /// uncertainty into the Frame this Region represents.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::BasicFrameBuilder;
    /// use ast_core::regions::{CircleBuilder, Region};
    ///
    /// let frame = BasicFrameBuilder::new(2).build()?;
    /// let mut circle = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])?.radius(1.0).build()?;
    /// assert!(circle.get_unc(false)?.is_none());
    /// assert!(circle.get_unc(true)?.is_some());
    ///
    /// let unc = CircleBuilder::new(frame, vec![0.0, 0.0])?.radius(0.01).build()?;
    /// circle.set_uncertainty(Box::new(unc))?;
    /// let (lbnd, ubnd) = circle.get_unc(false)?.unwrap().get_region_bounds()?;
    /// assert!((ubnd[0] - lbnd[0] - 0.02).abs() < 1e-12);
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    #[doc(alias = "astGetUnc")]
    fn get_unc(&self, def: bool) -> AstResult<Option<DynRegion>> {
        // C: region.c `GetUnc`, which copies the result (clearing RegionFS
        // throughout) and clears RegionFS again, so a dump of it carries its
        // Frame.
        if !def && !self.test_unc() {
            return Ok(None);
        }
        Ok(Some(self.get_unc_frm(FrameRef::Current)?.copy_region()))
    }

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
    /// (with `c` the axis centre) when that width is zero, or `0.5E-12` when
    /// that is zero too — the absolute floor `astEQUAL` applies to values near
    /// zero (region.c:3578). At zero there is no scale to be proportional to,
    /// and a zero-width uncertainty makes a membership test at the Region's own
    /// points fail. Axes with infinite bounds collapse to a zero-width `[0, 0]`
    /// interval.
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
                if hw == 0.0 {
                    hw = 0.5e-12;
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

    /// The uncertainty Region this Region holds, if it holds one, by
    /// reference.
    #[doc(hidden)]
    fn explicit_unc(&self) -> Option<&DynRegion> {
        None
    }

    /// Where this Region keeps its default uncertainty once built (C's
    /// `defunc`), if it keeps one.
    #[doc(hidden)]
    fn def_unc_cache(&self) -> Option<&std::cell::RefCell<Option<DynRegion>>> {
        None
    }

    /// Return the uncertainty Region for this Region in the requested Frame.
    ///
    /// Mirrors C `astGetUncFrm` (region.c:3912): the explicit uncertainty if
    /// one is set, otherwise the default ([`get_def_unc`](Region::get_def_unc)).
    /// For [`FrameRef::Base`] the uncertainty (which is always defined in the
    /// base Frame) is returned directly; for [`FrameRef::Current`] it is
    /// mapped through the base→current Mapping unless that is a `UnitMap`.
    fn get_unc_frm(&self, ifrm: FrameRef) -> AstResult<DynRegion> {
        let explicit = match self.explicit_unc() {
            // Work out the stored Region's bounds on the stored Region itself,
            // once, so the copy handed out carries them (C's callers share
            // the one Region and its cache).
            Some(u) => {
                u.get_region_bounds()?;
                Some(u.clone())
            }
            None => self.get_uncertainty(),
        };
        let unc = match (explicit, self.def_unc_cache()) {
            (Some(u), _) => u,
            // Built once and kept, as C keeps `defunc` (region.c:4006).
            (None, Some(slot)) => {
                if slot.borrow().is_none() {
                    let unc = self.get_def_unc()?;
                    // C's callers share the one cached Region, so what it
                    // caches about itself persists; here each caller gets a
                    // copy, so its bounds are worked out once now and every
                    // copy carries them.
                    unc.get_region_bounds()?;
                    *slot.borrow_mut() = Some(unc);
                }
                slot.borrow().clone().expect("built above")
            }
            (None, None) => self.get_def_unc()?,
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
                    unc.reg_map_region(map.as_ref(), frm.as_ref())
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

    /// This Region described in another coordinate system.
    ///
    /// `mapping` transforms positions from the coordinate system this Region
    /// represents to the one `frame` describes.
    /// It must have both a forward and an inverse transformation, and they
    /// should be true inverses of each other: a position transformed forward
    /// and then back should come back unchanged.
    /// Some Mappings, such as a PermMap, MathMap or SphMap, can fail that.
    ///
    /// The result is a new Region covering, in `frame`, the area that
    /// corresponds to this Region.
    /// It is simplified, so it need not be of the same class: a Box mapped
    /// through a PermMap comes back as a Box, but a Circle mapped through a
    /// Mapping that scales its axes differently does not stay a Circle.
    /// The uncertainty of this Region is mapped too.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Nodef`] if `mapping` lacks a forward or an inverse
    ///   transformation, or if the forward transformation gives bad values
    ///   for the points that define this Region.
    /// - [`ErrorCode::Inter`] if simplifying the result does not give a
    ///   Region, which would be an internal error.
    /// - Otherwise passes on the errors of transforming the points and of
    ///   simplifying the result.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::BasicFrameBuilder;
    /// use ast_core::mappings::ZoomMap;
    /// use ast_core::regions::{CircleBuilder, Region};
    ///
    /// let frame = BasicFrameBuilder::new(2).build()?;
    /// let circle = CircleBuilder::new(frame.clone(), vec![1.0, 1.0])?.radius(1.0).build()?;
    /// let mapped = circle.map_region(&ZoomMap::new(2, 2.0)?, &frame)?;
    /// let (lbnd, ubnd) = mapped.get_region_bounds()?;
    /// assert_eq!((lbnd, ubnd), (vec![0.0, 0.0], vec![4.0, 4.0]));
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    // C: the public `astMapRegion` (`astMapRegionId_`, region.c:13947): the
    // Region `reg_map_region` returns, simplified.
    #[doc(alias = "astMapRegion")]
    fn map_region(&self, mapping: &dyn Mapping, frame: &dyn Frame) -> AstResult<DynRegion> {
        let mapped = self.reg_map_region(mapping, frame)?;
        let simplified = (mapped.as_ref() as &dyn Mapping).simplify()?;
        merge_util::mapping_into_region(simplified).ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "astMapRegion: simplifying a Region did not give a Region (internal AST \
                 programming error).",
            )
        })
    }

    /// Map this region into a new frame without simplifying the result.
    ///
    /// The `mapping` transforms coordinates from this region's current Frame
    /// to the new `frame`.
    ///
    /// Faithful port of C's protected `astMapRegion` / `MapRegion` (region.c:5375). The C
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
    #[doc(hidden)]
    fn reg_map_region(&self, mapping: &dyn Mapping, frame: &dyn Frame) -> AstResult<DynRegion> {
        // The supplied Mapping must define both a forward and an inverse
        // transformation (region.c:5488-5498).
        if !mapping.has_inverse() {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "reg_map_region: the supplied Mapping does not define an inverse transformation"
                    .to_string(),
            ));
        }
        if !mapping.has_forward() {
            return Err(AstError::new(
                ErrorCode::Nodef,
                "reg_map_region: the supplied Mapping does not define a forward transformation"
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
                        "reg_map_region: the region which results from the supplied Mapping is \
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
            if let Some((outax_1, tmap)) = inv.map_split(&inax)? {
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

                // Without `axflags` (some defining point is bad) `ntotal` is 0,
                // so neither branch runs unless the split kept no axes, and the
                // Region keeps all its axes (region.c:5631-5652).
                if ntotal == nkept {
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
                    })? && let PickedAxes::Region(r) = pick_axes_region(usethis.as_ref(), &keep)?
                    {
                        usethis = r;
                        usemap = tmap2;
                    }
                }
            }
        }

        // Take a deep copy of the (possibly axis-reduced) Region and perform
        // the base→current edge surgery on its FrameSet (region.c:5703-5722).
        // The base and current Frames of the result generally differ, so the
        // result includes its FrameSet in a dump (region.c:5721
        // `astSetRegionFS( result, 1 )`) — which is also what the copy's own
        // cleared `RegionFS` leaves it at.
        let mut result: DynRegion = usethis.copy_region();
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
        let mut new = self.copy_region();
        let mut simpler = false;

        // Simplify the encapsulated FrameSet, noting whether it changed
        // (region.c:11160-11170). C's test is `fs != new->frameset`, pointer
        // identity, which `simplify_reporting_change` reports. FrameSet's
        // simplification simplifies each internal edge Mapping and returns a
        // FrameSet.
        let fs = new.get_region_frameset();
        let (simplified_map, fs_changed) = <dyn Mapping>::simplify_reporting_change(&fs)?;
        if fs_changed && let Some(new_fs) = simplified_map.as_any().downcast_ref::<FrameSet>() {
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
            // (region.c:11193-11196). C's test is `sunc != unc`, pointer
            // identity on what the per-class slot handed back, so a reduction
            // that changes only structure counts. `Mapping::equal` would not
            // see one: it is conversion-based, and collapsing an uncertainty's
            // FrameSet to a UnitMap leaves the transform unchanged.
            let (sunc_map, unc_changed) = <dyn Mapping>::simplify_reporting_change(unc.as_ref())?;
            if unc_changed && let Some(mut sunc) = merge_util::mapping_into_region(sunc_map) {
                // The stability check (region.c:11213-11306): re-centre the
                // simplified uncertainty at opposite corners of a test box and
                // require its per-axis bounding-box widths to agree.
                let orig_cen = sunc.reg_centre_query(FrameRef::Base)?;
                let naxb = self.get_region_frameset().base().naxes();

                // The test box: this Region's base bounding box if bounded,
                // else a box 100× the uncertainty size centred on the
                // uncertainty centre (region.c:11221-11236).
                let (lbnd, ubnd) = if self.is_bounded()? {
                    self.region_base_box()?
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
                    new.set_uncertainty(sunc)?;
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
    fn reg_overlay(&mut self, that: &dyn Region, clear_unc: bool) -> AstResult<()> {
        // C `astRegOverlay` copies `negated`, `closed`, `regionfs` and
        // `adaptive` as raw field assignments, propagating the `-INT_MAX`
        // unset sentinel; mirror that by copying each value with its
        // explicitly-set state.
        let (negated, negated_set) = that.raw_negated();
        self.set_negated_raw(negated, negated_set);
        let (closed, closed_set) = that.raw_closed();
        self.set_closed_raw(closed, closed_set);
        self.set_region_fs_raw(that.region_fs_flag(), that.test_region_fs());
        self.set_adaptive_raw(that.is_adaptive(), that.test_adaptive());
        if self.naxes() == that.naxes() {
            if that.test_mesh_size() {
                self.set_mesh_size(that.mesh_size());
            }
            if that.test_fill_factor() {
                self.set_fill_factor(that.fill_factor())?;
            }
        } else {
            self.clear_mesh_size();
            self.clear_fill_factor();
        }
        if clear_unc && !that.test_unc() {
            self.store_uncertainty(None);
        }
        Ok(())
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

    /// The Region an Stc encapsulates (stc.c `region`), or `None` for a
    /// Region that is not an Stc, as `astIsAStc` decides.
    #[doc(hidden)]
    fn stc_region(&self) -> Option<&dyn Region> {
        None
    }

    /// Clone this region into a boxed trait object.
    #[doc(hidden)]
    fn clone_region(&self) -> DynRegion;
}

/// Bounding box of a Region within its *current* Frame.
///
/// Faithful port of C `GetRegionBounds` (region.c:8948), the base-class
/// implementation every Region shares. The Negated attribute is ignored, so
/// the box encloses the un-negated Region.
///
/// When the simplified base→current Mapping is a `UnitMap` the base-Frame box
/// is the answer already. Otherwise points on the Region's boundary are
/// transformed into the current Frame and bounded there: a bounded Region
/// supplies a boundary mesh, and an unbounded one, which has no mesh, supplies
/// the corners of its base bounding box instead.
pub(crate) fn region_bounds_current<R>(this: &R) -> AstResult<(Vec<f64>, Vec<f64>)>
where
    R: Region + data::RegionGeometry + ?Sized,
{
    use crate::constants::BAD_VALUE;

    // The simplified base->current Mapping (region.c:9036).
    let fs = this.get_region_frameset();
    let smap = fs.mapping(FrameRef::Base, FrameRef::Current)?.simplify()?;

    // A UnitMap leaves the base-Frame box as the answer (region.c:9040-9041).
    if smap.as_any().is::<UnitMap>() {
        return this.reg_base_box();
    }

    // A bounded Region has a genuine mesh of points on its boundary
    // (region.c:9048-9049).
    let bmesh = if this.is_bounded()? {
        let mesh = this.reg_base_mesh()?;
        PointSet::from_rows(mesh.iter().map(|p| p.as_slice()))
    } else {
        // An unbounded Region has none, so the corners of its base-Frame
        // bounding box are used instead (region.c:9053-9092).
        let nbase = smap.nin();
        let (blbnd, bubnd) = this.reg_base_box()?;
        let npos = 1usize << nbase;
        let mut pset = PointSet::zeros(npos, nbase);
        let mut jmax = 1usize;
        for i in 0..nbase {
            let mut lo = true;
            let mut j = 0usize;
            for ip in 0..npos {
                if j == jmax {
                    lo = !lo;
                    j = 0;
                }
                pset.point_mut(ip)[i] = if lo { blbnd[i] } else { bubnd[i] };
                j += 1;
            }
            jmax *= 2;
        }
        pset
    };

    // Transform the points into the current Frame (region.c:9096).
    let cmesh = smap.transform(&bmesh, true)?;

    // The points may span a singularity such as the RA=0 line of a SkyFrame,
    // so normalise each axis into the shortest possible range
    // (region.c:9098-9110).
    let frm = fs.current();
    let ncur = frm.naxes();
    let mut cols: Vec<Vec<f64>> = (0..ncur).map(|i| cmesh.axis_iter(i).collect()).collect();
    for (i, col) in cols.iter_mut().enumerate() {
        frm.ax_norm(i + 1, 1, col)?;
    }

    // Bound the normalised points (astBndPoints, pointset.c:681): an axis with
    // no good value is returned bad.
    let mut lbnd = vec![BAD_VALUE; ncur];
    let mut ubnd = vec![BAD_VALUE; ncur];
    for (i, col) in cols.iter().enumerate() {
        for &v in col {
            if v == BAD_VALUE {
                continue;
            }
            if lbnd[i] == BAD_VALUE {
                lbnd[i] = v;
                ubnd[i] = v;
            } else if v < lbnd[i] {
                lbnd[i] = v;
            } else if v > ubnd[i] {
                ubnd[i] = v;
            }
        }
    }

    // The bounds may themselves span the singularity, so take the width from
    // the Frame and rebuild the upper bound from it (region.c:9112-9124).
    for i in 0..ncur {
        let width = frm.ax_distance(i + 1, lbnd[i], ubnd[i])?;
        if width != BAD_VALUE {
            ubnd[i] = lbnd[i] + width;
        } else {
            ubnd[i] = BAD_VALUE;
            lbnd[i] = BAD_VALUE;
        }
    }

    Ok((lbnd, ubnd))
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
    if let Some((out_axes, fsmap)) = cbmap.map_split(&one_based)? {
        // Convert the returned base-Frame axis indices to zero-based for
        // reg_base_pick_axes, which slices the base defining geometry.
        let base_axes: Vec<usize> = out_axes.iter().map(|&a| a - 1).collect();

        if let Some(breg) = region.reg_base_pick_axes(&base_axes)? {
            // Invert fsmap so it maps selected base axes -> selected current
            // axes, then map the base region into the picked current Frame.
            let mut fsmap = fsmap;
            fsmap.invert();
            let mut creg = breg.reg_map_region(fsmap.as_ref(), picked_frame.as_ref())?;
            // Copy properties from the old Region to the new Region
            // (region.c:7723 passes unc=0: uncertainty is left untouched).
            creg.reg_overlay(region, false)?;
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
    } else if let Some(r) = a.downcast_ref::<Moc>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<StcResourceProfile>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<StcSearchLocation>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<StcCatalogEntryLocation>() {
        Some(r)
    } else if let Some(r) = a.downcast_ref::<StcObsDataLocation>() {
        Some(r)
    } else {
        None
    }
}

/// The AST class name of a Region, as C's `astGetClass` gives it.
pub(crate) fn class_name(region: &dyn std::any::Any) -> &'static str {
    if region.is::<BoxRegion>() {
        "Box"
    } else if region.is::<Circle>() {
        "Circle"
    } else if region.is::<CmpRegion>() {
        "CmpRegion"
    } else if region.is::<Ellipse>() {
        "Ellipse"
    } else if region.is::<Interval>() {
        "Interval"
    } else if region.is::<NullRegion>() {
        "NullRegion"
    } else if region.is::<PointList>() {
        "PointList"
    } else if region.is::<Polygon>() {
        "Polygon"
    } else if region.is::<Prism>() {
        "Prism"
    } else if region.is::<Moc>() {
        "Moc"
    } else if region.is::<StcResourceProfile>() {
        "StcResourceProfile"
    } else if region.is::<StcSearchLocation>() {
        "StcSearchLocation"
    } else if region.is::<StcCatalogEntryLocation>() {
        "StcCatalogEntryLocation"
    } else if region.is::<StcObsDataLocation>() {
        "StcObsDataLocation"
    } else {
        "Region"
    }
}

/// The center and radius of a disc that just contains a 2-D Region, as
/// [`Region::get_region_disc`] gives them.
///
/// The Region is taken un-negated.
/// Its boundary mesh in the Frame it represents may lie on the sky, so its
/// center is found in a local system: distances parallel and perpendicular
/// to the line from the first mesh point to the one half way round, from
/// [`Frame::resolve`].
/// The center of the bounding box of about 200 mesh points in that system is
/// taken back into the Frame with [`Frame::offset2`], and the radius is the
/// greatest distance from it to a mesh point, increased by one part in 10^6
/// for rounding.
/// A center and radius that cannot be found (for an unbounded Region, or
/// bad first mesh points) are [`BAD_VALUE`](crate::constants::BAD_VALUE).
///
/// # Errors
///
/// - [`ErrorCode::Invar`] if the Region is not 2-dimensional.
/// - Otherwise passes on the errors of meshing the boundary and of the
///   Frame's geometry methods.
// C: region.c `GetRegionDisc`. For an unbounded Region C leaves the
// caller's center array unchanged; the port returns bad values.
#[doc(alias = "astGetRegionDisc")]
pub fn region_disc(region: &dyn Region) -> AstResult<(Vec<f64>, f64)> {
    if region.naxes() != 2 {
        return Err(AstError::new(
            ErrorCode::Invar,
            "astGetRegionDisc(Region): Supplied Region is not 2-dimensional.",
        ));
    }
    let unnegated;
    let region: &dyn Region = if region.is_negated() {
        unnegated = region.negated_copy();
        unnegated.as_ref()
    } else {
        region
    };
    let unknown = Ok((
        vec![crate::constants::BAD_VALUE; 2],
        crate::constants::BAD_VALUE,
    ));
    if !region.is_bounded()? {
        return unknown;
    }

    let mesh = region.reg_mesh()?;
    let npoint = mesh.npoint();
    let point1 = mesh.point(0).to_vec();
    let point2 = mesh.point(npoint / 2).to_vec();
    if point1.iter().chain(&point2).any(|&v| is_bad(v)) {
        return unknown;
    }

    // The bounding box of about 200 mesh points in the local system.
    let step = if npoint > 200 { npoint / 200 } else { 1 };
    let (mut dxmax, mut dxmin, mut dymax, mut dymin) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for ipoint in (step..npoint).step_by(step) {
        let point3 = mesh.point(ipoint);
        if point3.iter().any(|&v| is_bad(v)) {
            continue;
        }
        let (point4, dx, mut dy) = region.resolve(&point1, &point2, &point3)?;
        // `resolve` gives |dy|; its sign is that of the angle at point4.
        if region.angle(&point1, &point4, &point3)? < 0.0 {
            dy = -dy;
        }
        dxmax = dxmax.max(dx);
        dxmin = dxmin.min(dx);
        dymax = dymax.max(dy);
        dymin = dymin.min(dy);
    }
    let dx = 0.5 * (dxmax + dxmin);
    let dy = 0.5 * (dymax + dymin);

    // Back into the Region's Frame: along the basis vector by dx, then at
    // right angles by dy.
    let dist = region.distance(&point1, &point2)?;
    let north = [point1[0], point1[1] + 0.001 * dist];
    let angle = region.angle(&north, &point1, &point2)?;
    let (point3, angle) = region.offset2(&point1, angle, dx)?;
    let (centre, _) = region.offset2(&point3, angle - std::f64::consts::FRAC_PI_2, dy)?;

    let mut radius = 0.0f64;
    for ipoint in 0..npoint {
        let d = region.distance(&centre, &mesh.point(ipoint))?;
        if !is_bad(d) {
            radius = radius.max(d);
        }
    }
    Ok((centre, radius * 1.000001))
}

/// A copy of a Mapping with every Region in it removed.
///
/// The Mapping, which may be a compound one such as a
/// [`CmpMap`](crate::mappings::CmpMap), is searched for components that are
/// Regions.
/// Each one is dropped where it can simply be removed, as from a series
/// CmpMap, and otherwise replaced by a [`UnitMap`] with its number of axes,
/// so the Mapping keeps its dimensionality but no longer clips points.
/// A Region itself becomes such a UnitMap, and a
/// [`FrameSet`] has the Regions removed from every
/// Mapping between its Frames and every Region among its Frames replaced by
/// its Frame.
/// A Mapping holding no Region is returned unchanged.
#[doc(alias = "astRemoveRegions")]
pub fn remove_regions(mapping: &dyn Mapping) -> DynMapping {
    // C returns the Region's Frame, which is a unit Mapping too; the port's
    // Frames are not Mappings (PLAN.md §9.6). Each class that holds other
    // Mappings or Frames removes them from its own components, as its C
    // `RemoveRegions` does.
    mapping
        .mapping_without_regions()
        .unwrap_or_else(|| mapping.clone_box())
}

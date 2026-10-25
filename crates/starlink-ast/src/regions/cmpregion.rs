// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! CmpRegion: a compound region combining two sub-regions with AND, OR or
//! XOR.
//!
//! A [`CmpRegion`] represents the intersection (AND) or union (OR) of two
//! component regions, an XOR being made from those.
//! The second component is mapped into the Frame of the first.
//!
//! For AND (intersection): a point is inside the CmpRegion if it is
//! inside *both* component regions.
//!
//! For OR (union): a point is inside the CmpRegion if it is inside
//! *either* (or both) component regions.

use crate::constants::BAD_VALUE;
use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::{CmpOperator, DynRegion, NullRegion, NullRegionBuilder, Overlap, Region};

/// A combination of two Regions within a single Frame.
///
/// The two component Regions, of any class, are combined with a boolean
/// operator, a [`CmpOperator`]:
///
/// - AND: a position is inside the CmpRegion only if it is inside both
///   components.
/// - OR: a position is inside if it is inside either component, or both.
/// - XOR: a position is inside if it is inside one component but not both.
///
/// Other operators can be formed by negating one or both components before
/// combining them.
/// Since a CmpRegion is itself a Region, it can be a component of further
/// CmpRegions, so Regions of any complexity can be built from simple ones.
///
/// The two components need not be in the same Frame, but there must be a
/// Mapping between their Frames, as
/// [`convert_to`](crate::frames::FrameSearch::convert_to) finds it.
/// For instance, a CmpRegion may combine a Region in an ICRS
/// [`SkyFrame`](crate::frames::SkyFrame) with one in a Galactic SkyFrame.
/// The second component is then mapped into the Frame of the first, which
/// is the Frame the CmpRegion represents.
/// An XOR is held as the OR of two ANDs, each of one component with the
/// negation of the other, which [`operator`](Self::operator) still reports
/// as XOR.
///
/// A CmpRegion has no defining points of its own, so
/// [`region_points`](Region::region_points) returns none; its components
/// have theirs.
/// Unless one is set on it, its uncertainty is the one set on its first
/// component, or else the one set on its second, or else the default every
/// Region has.
/// It takes the [Closed](Region::is_closed) and
/// [MeshSize](Region::mesh_size) values of its first component when they
/// are set there, and the default of its
/// [FillFactor](Region::fill_factor) from that component too.
/// It has no attributes or methods beyond those of every [`Region`], except
/// for access to its components.
///
/// A Frame attribute set through the CmpRegion changes the coordinate
/// system it is described in; while the CmpRegion is
/// [Adaptive](Region::is_adaptive) it keeps covering the same area, and
/// otherwise the change is passed on to its components.
/// [`with_integrity`](Self::with_integrity) makes any other change to its
/// Frame in the same way.
///
/// # Examples
///
/// ```
/// use starlink_ast::frames::BasicFrameBuilder;
/// use starlink_ast::regions::{CircleBuilder, CmpOperator, CmpRegionBuilder, Region};
///
/// let frame = BasicFrameBuilder::new(2).build()?;
/// let left = CircleBuilder::new(frame.clone(), vec![0.0, 0.0])?.radius(2.0).build()?;
/// let right = CircleBuilder::new(frame, vec![2.0, 0.0])?.radius(2.0).build()?;
/// let lens = CmpRegionBuilder::new(left, right, CmpOperator::And)?.build()?;
/// assert!(lens.point_in_region(&[1.0, 0.0])?);
/// assert!(!lens.point_in_region(&[-1.0, 0.0])?);
/// # Ok::<(), starlink_ast::error::AstError>(())
/// ```
#[derive(Debug, Clone)]
pub struct CmpRegion {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
    /// First component region.
    pub(crate) region1: DynRegion,
    /// Second component region.
    pub(crate) region2: DynRegion,
    /// Boolean operator combining the two regions: AND or OR, as C's
    /// `oper` is, an XOR being made from them.
    pub(crate) operator: Oper,
    /// For a CmpRegion made with XOR, the two Regions XORed (C's `xor1` and
    /// `xor2`), which its dump writes in place of the components.
    pub(crate) xor: Option<(DynRegion, DynRegion)>,
}

/// The operator a CmpRegion applies to its two components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Oper {
    /// Boolean AND.
    And,
    /// Boolean OR.
    Or,
}

/// Builder for constructing a [`CmpRegion`].
pub struct CmpRegionBuilder {
    region1: DynRegion,
    region2: DynRegion,
    operator: CmpOperator,
    uncertainty: Option<DynRegion>,
}

impl CmpRegionBuilder {
    /// Start a CmpRegion combining `region1` and `region2` with `operator`.
    ///
    /// The CmpRegion takes ownership of the two Regions.
    ///
    /// # Errors
    ///
    /// None: [`build`](Self::build) checks that the Regions can be combined.
    #[doc(alias = "astCmpRegion")]
    pub fn new(
        region1: impl Region + 'static,
        region2: impl Region + 'static,
        operator: CmpOperator,
    ) -> AstResult<Self> {
        Ok(CmpRegionBuilder {
            region1: Box::new(region1),
            region2: Box::new(region2),
            operator,
            uncertainty: None,
        })
    }

    /// [`new`](Self::new) for Regions already boxed.
    ///
    /// # Errors
    ///
    /// As for [`new`](Self::new).
    pub fn from_dyn(
        region1: DynRegion,
        region2: DynRegion,
        operator: CmpOperator,
    ) -> AstResult<Self> {
        Ok(CmpRegionBuilder {
            region1,
            region2,
            operator,
            uncertainty: None,
        })
    }

    /// Give the CmpRegion an uncertainty: a Region that, centered on any
    /// point of the boundary, covers the possible positions of that point.
    ///
    /// [`Region::set_uncertainty`] describes the Regions that can be used,
    /// and [`build`](Self::build) fails as that method does with one that
    /// cannot.
    /// Without one, the CmpRegion uses the uncertainty of one of its
    /// components, as [`CmpRegion`] describes.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the CmpRegion.
    ///
    /// The second Region is mapped into the Frame of the first when the two
    /// differ.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Intrd`] if there is no Mapping between the Frames of
    ///   the two Regions, as when they have different numbers of axes.
    /// - Passes on the errors of finding that Mapping, of mapping the second
    ///   Region, and of [`Region::set_uncertainty`] for an uncertainty given
    ///   to [`uncertainty`](Self::uncertainty).
    // C: astInitCmpRegion.
    pub fn build(self) -> AstResult<CmpRegion> {
        // astInitCmpRegion takes deep copies of the components, and a copy
        // clears RegionFS throughout (cmpregion.c:4758-4759, region.c Copy).
        let mut reg1 = self.region1;
        let mut reg2 = self.region2;
        reg1.clear_region_fs_deep();
        reg2.clear_region_fs_deep();
        let Some(fs) = crate::frames::find_frame::convert(
            reg2.get_region_frame().as_ref(),
            reg1.get_region_frame().as_ref(),
            &[],
        )?
        else {
            return Err(AstError::new(
                ErrorCode::Intrd,
                "astInitCmpRegion(CmpRegion): No Mapping can be found between the two \
                 supplied Regions.",
            ));
        };
        let frame = fs.current().snapshot();
        let smap = fs
            .mapping(
                crate::frames::FrameRef::Base,
                crate::frames::FrameRef::Current,
            )?
            .simplify()?;
        if !smap.as_any().is::<crate::mappings::UnitMap>() {
            reg2 = reg2.reg_map_region(smap.as_ref(), frame.as_ref())?;
        }

        let (oper, xor) = match self.operator {
            CmpOperator::And => (Oper::And, None),
            CmpOperator::Or => (Oper::Or, None),
            CmpOperator::Xor => {
                let and = |a: DynRegion, b: DynRegion| -> AstResult<DynRegion> {
                    Ok(Box::new(
                        CmpRegionBuilder::from_dyn(a, b, CmpOperator::And)?.build()?,
                    ))
                };
                let new1 = and(reg1.negated_copy(), reg2.clone_region())?;
                let new2 = and(reg1.clone_region(), reg2.negated_copy())?;
                let xor = (reg1, reg2);
                reg1 = new1;
                reg2 = new2;
                (Oper::Or, Some(xor))
            }
        };

        // A component whose base->current Mapping is a UnitMap need not
        // dump its FrameSet, so its RegionFS is cleared (cmpregion.c:4855).
        for region in [&mut reg1, &mut reg2] {
            let fs = region.get_region_frameset();
            let map = fs.mapping(
                crate::frames::FrameRef::Base,
                crate::frames::FrameRef::Current,
            )?;
            if map.as_any().is::<crate::mappings::UnitMap>() {
                region.set_region_fs_flag(false);
            }
        }

        let data = RegionData::new(frame, Vec::new());
        let mut region = CmpRegion {
            data,
            region1: reg1,
            region2: reg2,
            operator: oper,
            xor,
        };

        // astSetUnc, as C's constructor calls it (region.c:13372).
        if let Some(unc) = self.uncertainty {
            region.set_uncertainty(unc)?;
        }

        // The first component's MeshSize and Closed, where set, become the
        // CmpRegion's, and so through its setters both components'
        // (cmpregion.c:4867-4872).
        if region.region1.test_mesh_size() {
            region.set_mesh_size(region.region1.mesh_size());
        }
        if region.region1.test_closed() {
            region.set_closed(region.region1.is_closed());
        }

        Ok(region)
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl CmpRegion {
    /// Make a change to the Frame this CmpRegion represents, keeping the
    /// CmpRegion consistent with it.
    ///
    /// `f` is applied as a Frame attribute set through the CmpRegion is.
    /// While the CmpRegion is [Adaptive](Region::is_adaptive), the change is
    /// made to the Frame it represents, and the CmpRegion is remapped into the
    /// new coordinate system so that it covers the same area.
    /// Otherwise the change is made to both the Frame it represents and the
    /// Frame it was defined in, and to its component Regions, so the numbers
    /// that define it keep their values.
    /// `f` makes the change to each Frame it is made to, so it may be called
    /// more than once.
    ///
    /// # Errors
    ///
    /// Passes on the error returned by `f`, and the error of remapping the
    /// CmpRegion.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: Fn(&mut dyn Frame) -> AstResult<()>,
    {
        super::data::with_region_integrity(self, f)
    }

    /// Permute the axes of this region's current frame.
    pub fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.data.perm_axes(perm)
    }

    /// The boolean operator this CmpRegion combines its Regions with.
    ///
    /// It is XOR for a CmpRegion made with XOR, whose components are then the
    /// two ANDs it is made from, and for an OR of two ANDs that is equivalent
    /// to an XOR.
    // C: cmpregion.c `XORCheck` finds the equivalent XOR.
    pub fn operator(&self) -> CmpOperator {
        if self.xor_regions().is_some() {
            return CmpOperator::Xor;
        }
        match self.operator {
            Oper::And => CmpOperator::And,
            Oper::Or => CmpOperator::Or,
        }
    }

    /// The two Regions XORed: those a CmpRegion made with XOR was given, or
    /// for an OR of two ANDs that is equivalent to an XOR, the two Regions
    /// cmpregion.c `XORCheck` finds, with whichever of them was negated
    /// un-negated. C negates the first Region of the first AND in place to
    /// compare it, and back, which leaves its Negated attribute set; the
    /// copies compared here leave it untouched.
    pub(crate) fn xor_regions(&self) -> Option<(DynRegion, DynRegion)> {
        if let Some((a, b)) = &self.xor {
            return Some((a.clone_region(), b.clone_region()));
        }
        if self.operator != Oper::Or {
            return None;
        }
        let c1 = self.region1.as_any().downcast_ref::<CmpRegion>()?;
        let c2 = self.region2.as_any().downcast_ref::<CmpRegion>()?;
        if c1.operator != Oper::And || c2.operator != Oper::And {
            return None;
        }
        let same = |a: &DynRegion, b: &DynRegion| Mapping::equal(a.as_ref(), b.as_ref());
        let neg1 = c1.region1.negated_copy();
        let neg2 = c1.region2.negated_copy();
        let xor = if same(&neg1, &c2.region1) {
            same(&neg2, &c2.region2)
        } else if same(&neg1, &c2.region2) {
            same(&neg2, &c2.region1)
        } else {
            false
        };
        if !xor {
            return None;
        }
        let mut xor1 = c1.region1.clone_region();
        let mut xor2 = c1.region2.clone_region();
        if xor1.is_negated() {
            xor1.negate();
        } else {
            xor2.negate();
        }
        Some((xor1, xor2))
    }

    /// cmpregion.c `CmpRegionList`: the Regions this CmpRegion combines and
    /// the one operator that combines them. A component CmpRegion that uses
    /// the same operator is replaced by its own Regions, recursively, unless
    /// it is equivalent to an XOR; a CmpRegion equivalent to an XOR gives
    /// its two XORed Regions and XOR.
    #[doc(hidden)]
    pub fn reg_cmp_region_list(&self) -> (CmpOperator, Vec<DynRegion>) {
        let mut list = Vec::new();
        let oper = self.cmp_region_list_into(&mut list);
        (oper, list)
    }

    fn cmp_region_list_into(&self, list: &mut Vec<DynRegion>) -> CmpOperator {
        if let Some((xor1, xor2)) = self.xor_regions() {
            list.push(xor1);
            list.push(xor2);
            return CmpOperator::Xor;
        }
        for component in [&self.region1, &self.region2] {
            match component.as_any().downcast_ref::<CmpRegion>() {
                Some(cmp) if cmp.operator == self.operator && cmp.xor_regions().is_none() => {
                    cmp.cmp_region_list_into(list);
                }
                _ => list.push(component.clone_region()),
            }
        }
        match self.operator {
            Oper::And => CmpOperator::And,
            Oper::Or => CmpOperator::Or,
        }
    }

    /// The first component Region, in the Frame the CmpRegion was defined
    /// in.
    ///
    /// For a CmpRegion made with XOR, it is the first of the two ANDs the XOR
    /// is made from.
    pub fn region1(&self) -> &dyn Region {
        self.region1.as_ref()
    }

    /// The second component Region, in the Frame the CmpRegion was defined
    /// in.
    ///
    /// For a CmpRegion made with XOR, it is the second of the two ANDs the
    /// XOR is made from.
    pub fn region2(&self) -> &dyn Region {
        self.region2.as_ref()
    }

    /// Return the two component Regions, the combining operator, and the
    /// `Negated` flags that should be used with each component so that the
    /// pair — combined with `oper` — is equivalent to this (possibly negated)
    /// CmpRegion.
    ///
    /// Faithful port of C `GetRegions` (cmpregion.c:981): if the CmpRegion is
    /// itself negated the De Morgan rewrite is applied (AND<->OR with both
    /// component `Negated` flags toggled). The returned component clones carry
    /// their *stored* `Negated` flags; the returned `neg1`/`neg2` are the
    /// *required* flags — a caller that needs a component to actually carry
    /// its required flag replaces it with [`Region::negated_copy`] when the two
    /// differ (cmpregion.c:775-788, 3617).
    fn get_regions(&self) -> (DynRegion, DynRegion, Oper, bool, bool) {
        let reg1 = self.region1.clone_region();
        let reg2 = self.region2.clone_region();
        let mut oper = self.operator;
        let mut neg1 = self.region1.is_negated();
        let mut neg2 = self.region2.is_negated();
        if self.data.negated {
            oper = match oper {
                Oper::And => Oper::Or,
                Oper::Or => Oper::And,
            };
            neg1 = !neg1;
            neg2 = !neg2;
        }
        (reg1, reg2, oper, neg1, neg2)
    }

    /// Map the Region `that` into the base (or current) Frame of the Region
    /// `into`.
    ///
    /// Faithful port of C `MatchRegion` (cmpregion.c:1406): find the conversion
    /// FrameSet from `that`'s current Frame to the requested Frame of `into`
    /// (its base Frame when `use_base` is true, else its current Frame), then
    /// [`reg_map_region`](Region::reg_map_region) `that` through the base->current
    /// Mapping of that FrameSet. Used to express a supplied point uncertainty
    /// in the base Frame of each component Region.
    fn match_region(into: &dyn Region, use_base: bool, that: &dyn Region) -> AstResult<DynRegion> {
        let into_fs = into.get_region_frameset();
        let target_frm = if use_base {
            into_fs.base().snapshot()
        } else {
            into_fs.current().snapshot()
        };
        let that_frm = that.get_region_frame();
        let fs = crate::frames::find_frame::convert(that_frm.as_ref(), target_frm.as_ref(), &[])?
            .ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "MatchRegion: cannot convert between the two supplied coordinate Frames"
                    .to_string(),
            )
        })?;
        let map = fs.mapping(FrameRef::Base, FrameRef::Current)?;
        that.reg_map_region(map.as_ref(), target_frm.as_ref())
    }

    /// Whether this CmpRegion is bounded.
    ///
    /// Faithful port of C `GetBounded` (cmpregion.c:703). The component Regions
    /// (with De Morgan applied via [`get_regions`](CmpRegion::get_regions), each
    /// forced to carry its required `Negated` flag) are examined together with
    /// the operator:
    ///
    /// * AND: bounded if either component is bounded; if neither is, the AND is
    ///   unbounded when the two components have partial, no, or exactly-negating
    ///   overlap (C codes 1, 4, 6) and bounded otherwise.
    /// * OR: unbounded if either component is unbounded, bounded otherwise.
    fn get_bounded(&self) -> AstResult<bool> {
        let (mut reg1, mut reg2, oper, neg1, neg2) = self.get_regions();
        if neg1 != reg1.is_negated() {
            reg1 = reg1.negated_copy();
        }
        if neg2 != reg2.is_negated() {
            reg2 = reg2.negated_copy();
        }

        let reg1b = reg1.is_bounded()?;
        let reg2b = reg2.is_bounded()?;

        let result = match oper {
            Oper::And => {
                if reg1b || reg2b {
                    true
                } else {
                    // Neither component is bounded: the AND is unbounded when
                    // they have partial, no, or exactly-negating overlap.
                    !matches!(
                        reg1.overlap(reg2.as_ref())?,
                        Overlap::None | Overlap::Partial | Overlap::Negation
                    )
                }
            }
            Oper::Or => reg1b && reg2b,
        };
        Ok(result)
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for CmpRegion {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    /// cmpregion.c `Transform`: every base-Frame point goes through both
    /// component Regions, all points at once, and the two results are
    /// combined per point. A Region makes every coordinate of a point it
    /// excludes bad, so a point is in a component where its output has no
    /// bad coordinate.
    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let out1 = self.region1.transform(pts, true)?;
        let out2 = self.region2.transform(pts, true)?;
        Ok((0..npoint)
            .map(|i| {
                let in1 = !point_has_bad(&out1.point(i));
                let in2 = !point_has_bad(&out2.point(i));
                match self.operator {
                    Oper::And => in1 && in2,
                    Oper::Or => in1 || in2,
                }
            })
            .collect())
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // Faithful port of C `cmpregion.c::RegBaseBox` (cmpregion.c:1508).
        // Returns the bounding box of the *un-negated* CmpRegion in its base
        // Frame: the parent Negated attribute is ignored here, but each
        // component's own Negated flag matters.
        let naxes = self.data.frameset.base().naxes();

        // If the CmpRegion is bounded, find the bounding box from a mesh of
        // points spread evenly over its boundary.
        if self.get_bounded()? {
            let mesh = self.reg_base_mesh()?;
            let mut lbnd = vec![f64::MAX; naxes];
            let mut ubnd = vec![-f64::MAX; naxes];
            for point in &mesh {
                for i in 0..naxes {
                    let v = point[i];
                    if !is_bad(v) {
                        if v < lbnd[i] {
                            lbnd[i] = v;
                        }
                        if v > ubnd[i] {
                            ubnd[i] = v;
                        }
                    }
                }
            }
            return Ok((lbnd, ubnd));
        }

        // The CmpRegion is not bounded, so look at each axis individually.
        // Get the component bounding boxes and their raw Negated flags. The
        // component `get_region_bounds` already ignores each component's own
        // Negated attribute, so a negated component's box describes the
        // *excluded* interval on each axis.
        let neg1 = self.region1.is_negated();
        let neg2 = self.region2.is_negated();
        let (mut clbnd1, mut cubnd1) = self.region1.get_region_bounds()?;
        let (mut clbnd2, mut cubnd2) = self.region2.get_region_bounds()?;

        let mut lbnd = vec![0.0; naxes];
        let mut ubnd = vec![0.0; naxes];

        // Component `get_region_bounds` uses BAD_VALUE (= -f64::MAX) as its
        // lower-infinity marker and +f64::MAX as its upper-infinity marker.
        // C's RegBaseBox reasons in terms of -DBL_MAX / +DBL_MAX, so treat
        // both -f64::MAX and BAD_VALUE consistently as -infinity.
        let neg_inf = -f64::MAX;
        let pos_inf = f64::MAX;
        let is_neg_inf = |v: f64| v == neg_inf || is_bad(v);

        for i in 0..naxes {
            // If the first component has been negated, its bounds describe an
            // *excluded* interval. If either end is infinite we can swap it to
            // an included interval; if both ends are finite we cannot, so —
            // when there is more than one axis — assume the gap is filled on
            // another axis and treat this axis as an unbounded included
            // interval. With a single axis the interval stays excluded.
            let mut inc1 = true;
            if neg1 {
                let lb = clbnd1[i];
                let ub = cubnd1[i];
                if is_neg_inf(lb) {
                    clbnd1[i] = ub;
                }
                if ub == pos_inf {
                    cubnd1[i] = lb;
                }
                if !is_neg_inf(lb) && ub != pos_inf {
                    if naxes == 1 {
                        inc1 = false;
                    } else {
                        clbnd1[i] = neg_inf;
                        cubnd1[i] = pos_inf;
                    }
                }
            }

            // Likewise for the second component.
            let mut inc2 = true;
            if neg2 {
                let lb = clbnd2[i];
                let ub = cubnd2[i];
                if is_neg_inf(lb) {
                    clbnd2[i] = ub;
                }
                if ub == pos_inf {
                    cubnd2[i] = lb;
                }
                if !is_neg_inf(lb) && ub != pos_inf {
                    if naxes == 1 {
                        inc2 = false;
                    } else {
                        clbnd2[i] = neg_inf;
                        cubnd2[i] = pos_inf;
                    }
                }
            }

            match self.operator {
                Oper::And => {
                    // AND: overlap of the axis intervals, depending on whether
                    // each interval is included or excluded.
                    if inc1 {
                        if inc2 {
                            lbnd[i] = clbnd1[i].max(clbnd2[i]);
                            ubnd[i] = cubnd1[i].min(cubnd2[i]);
                        } else {
                            lbnd[i] = if clbnd1[i] < clbnd2[i] {
                                clbnd1[i]
                            } else {
                                cubnd2[i]
                            };
                            ubnd[i] = if cubnd1[i] > cubnd2[i] {
                                cubnd1[i]
                            } else {
                                clbnd2[i]
                            };
                        }
                    } else if inc2 {
                        lbnd[i] = if clbnd2[i] < clbnd1[i] {
                            clbnd2[i]
                        } else {
                            cubnd1[i]
                        };
                        ubnd[i] = if cubnd2[i] > cubnd1[i] {
                            cubnd2[i]
                        } else {
                            clbnd1[i]
                        };
                    } else {
                        lbnd[i] = if clbnd1[i] < clbnd2[i] {
                            clbnd1[i]
                        } else {
                            cubnd2[i]
                        };
                        ubnd[i] = if cubnd1[i] > cubnd2[i] {
                            cubnd1[i]
                        } else {
                            clbnd2[i]
                        };
                    }
                }
                Oper::Or => {
                    // OR: union of the axis intervals.
                    if inc1 && inc2 {
                        lbnd[i] = clbnd1[i].min(clbnd2[i]);
                        ubnd[i] = cubnd1[i].max(cubnd2[i]);
                    } else {
                        lbnd[i] = neg_inf;
                        ubnd[i] = pos_inf;
                    }
                }
            }
        }

        Ok((lbnd, ubnd))
    }

    /// cmpregion.c `RegBaseMesh`: each component's boundary mesh in its
    /// current Frame (the CmpRegion's base Frame), or, for a component with
    /// no finite boundary, the part of its boundary within the other's
    /// bounding box (region.c `BndMesh`); then the points of each mesh inside
    /// the other component for AND, or outside it for OR. A component has a
    /// mesh if it is bounded either way round; with neither, or with only one
    /// under OR, there is no mesh (AST__INTER).
    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        self.data.cached_mesh(|| {
            let reg1 = self.region1.as_ref();
            let reg2 = self.region2.as_ref();
            let has_mesh = |r: &dyn Region| -> AstResult<bool> {
                Ok(r.is_bounded()? || r.negated_copy().is_bounded()?)
            };
            let (has1, has2) = (has_mesh(reg1)?, has_mesh(reg2)?);
            if !has1 && !has2 {
                return Err(AstError::new(
                    ErrorCode::Inter,
                    "astRegBaseMesh(CmpRegion): No mesh can be produced for the CmpRegion \
                 bacause neither of its component Regions has a mesh (internal AST \
                 programming error).",
                ));
            }
            if (!has1 || !has2) && self.operator == Oper::Or {
                return Err(AstError::new(
                    ErrorCode::Inter,
                    "astRegBaseMesh(CmpRegion): No mesh can be produced for the CmpRegion \
                 bacause one its component Regions has no mesh and the union of the \
                 Regions is required (internal AST programming error).",
                ));
            }

            // region.c `BndMesh`: the base-Frame boundary mesh within a box,
            // taken into the current Frame.
            let bnd_mesh = |r: &dyn Region, other: &dyn Region| -> AstResult<PointSet> {
                let (lbnd, ubnd) = other.get_region_bounds()?;
                r.to_current_frame(r.bnd_base_mesh(&lbnd, &ubnd)?)
            };
            let (mesh1, mesh2) = if has1 {
                let mesh1 = reg1.reg_mesh()?;
                let mesh2 = if has2 {
                    reg2.reg_mesh()?
                } else {
                    bnd_mesh(reg2, reg1)?
                };
                (mesh1, mesh2)
            } else {
                let mesh2 = reg2.reg_mesh()?;
                (bnd_mesh(reg1, reg2)?, mesh2)
            };

            // The points of each mesh inside the other component (AND), or
            // outside it (OR), the components negated to find those.
            let (filter1, filter2): (DynRegion, DynRegion) = match self.operator {
                Oper::And => (reg1.clone_region(), reg2.clone_region()),
                Oper::Or => (reg1.negated_copy(), reg2.negated_copy()),
            };
            let mesh1b = filter2.transform(&mesh1, true)?;
            let mesh2b = filter1.transform(&mesh2, true)?;

            Ok([mesh1b, mesh2b]
                .iter()
                .flat_map(|m| (0..m.npoint()).map(move |i| m.point(i)))
                .filter(|p| !p.iter().any(|&v| is_bad(v)))
                .map(|p| p.to_vec())
                .collect())
        })
    }

    fn is_bounded_inner(&self) -> AstResult<bool> {
        self.get_bounded()
    }

    fn remap_defining_points(&mut self, _mapping: &dyn Mapping, _forward: bool) -> AstResult<()> {
        // CmpRegion has no defining points of its own.
        // The sub-regions are immutable once constructed.
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region trait
// ============================================================================

impl Region for CmpRegion {
    fn def_unc_cache(&self) -> Option<&std::cell::RefCell<Option<DynRegion>>> {
        Some(&self.data.def_unc)
    }

    fn explicit_unc(&self) -> Option<&DynRegion> {
        self.data.uncertainty.as_ref()
    }

    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        // Check for BAD coordinates in the input.
        if point_has_bad(point) {
            return Ok(false);
        }

        // Points arrive in the CmpRegion's current frame. Transform to
        // base frame for geometry testing.
        let naxes = self.data.frameset.base().naxes();
        let pts = PointSet::from_rows(std::iter::once(point));
        let base_pts = self.data.frameset.transform(&pts, false)?;
        let base_point = base_pts.point(0);

        // Check the transformed point for BAD values.
        if base_point[..naxes].iter().any(|&v| is_bad(v)) {
            return Ok(false);
        }

        // Test against both sub-regions. The sub-regions' point_in_region
        // takes points in their current frame. Since the CmpRegion's base
        // frame = sub-regions' current frame, pass the base point directly.
        let inside = match self.operator {
            Oper::And => {
                let in1 = self.region1.point_in_region(&base_point[..naxes])?;
                if !in1 {
                    false
                } else {
                    self.region2.point_in_region(&base_point[..naxes])?
                }
            }
            Oper::Or => {
                let in1 = self.region1.point_in_region(&base_point[..naxes])?;
                if in1 {
                    true
                } else {
                    self.region2.point_in_region(&base_point[..naxes])?
                }
            }
        };

        // Apply negation.
        if self.data.negated {
            Ok(!inside)
        } else {
            Ok(inside)
        }
    }

    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        // Faithful port of C cmpregion.c::RegPins (cmpregion.c:2185). The
        // supplied points are in the base Frame of this CmpRegion, which is
        // equivalent to the current Frame of both component Regions.
        let reg1 = self.region1.as_ref();
        let reg2 = self.region2.as_ref();

        // Boundary mask of each component: transform the points into the
        // component's base Frame (astRegTransform, forward=0) and map the
        // supplied uncertainty into that same base Frame (MatchRegion).
        let psetb1 = reg1.get_region_frameset().transform(pts, false)?;
        let mask1 = match unc {
            Some(u) => {
                let unc1 = Self::match_region(reg1, true, u)?;
                reg1.reg_pins_mask(&psetb1, Some(unc1.as_ref()))?.1
            }
            None => reg1.reg_pins_mask(&psetb1, None)?.1,
        };

        let psetb2 = reg2.get_region_frameset().transform(pts, false)?;
        let mask2 = match unc {
            Some(u) => {
                let unc2 = Self::match_region(reg2, true, u)?;
                reg2.reg_pins_mask(&psetb2, Some(unc2.as_ref()))?.1
            }
            None => reg2.reg_pins_mask(&psetb2, None)?.1,
        };

        // Transform the supplied points through each component *as a Mapping*
        // (membership test): the first-axis output value is BAD iff the point
        // is outside that component. For OR the components are temporarily
        // negated so the test reports "outside" rather than "inside".
        let (pset1, pset2) = match self.operator {
            Oper::And => (reg1.transform(pts, true)?, reg2.transform(pts, true)?),
            Oper::Or => {
                let neg1 = reg1.negated_copy();
                let neg2 = reg2.negated_copy();
                (neg1.transform(pts, true)?, neg2.transform(pts, true)?)
            }
        };

        // Boundary rule (cmpregion.c:2339): a point is on the CmpRegion
        // boundary iff it is on component 1's boundary and non-BAD through
        // component 2, or on component 2's boundary and non-BAD through
        // component 1.
        let np = pts.npoint();
        let mut mask = Vec::with_capacity(np);
        let mut result = true;
        for ip in 0..np {
            let p1_bad = is_bad(pset1.point(ip)[0]);
            let p2_bad = is_bad(pset2.point(ip)[0]);
            let on = (mask1[ip] && !p2_bad) || (mask2[ip] && !p1_bad);
            mask.push(on);
            if !on {
                result = false;
            }
        }
        Ok((result, mask))
    }

    fn negated_copy(&self) -> DynRegion {
        let mut copy = self.clone();
        copy.data.negate();
        Box::new(copy)
    }

    fn negate(&mut self) {
        self.data.negate();
    }

    fn is_negated(&self) -> bool {
        self.data.negated
    }

    fn test_negated(&self) -> bool {
        self.data.negated_set
    }

    fn set_negated_raw(&mut self, negated: bool, set: bool) {
        self.data.negated = negated;
        self.data.negated_set = set;
        self.data.invalidate_cache();
    }

    /// The Closed attribute of this CmpRegion: see [`Region::is_closed`].
    ///
    /// A CmpRegion is built with the Closed value of its first component
    /// Region, if that has been set, and setting or clearing it also sets or
    /// clears it on both component Regions.
    fn is_closed(&self) -> bool {
        self.data.closed
    }

    /// Set [`Closed`](Region::is_closed) on this CmpRegion and on both its
    /// component Regions.
    // C: cmpregion.c `SetClosed`.
    fn set_closed(&mut self, closed: bool) {
        self.data.closed = closed;
        self.data.closed_set = true;
        self.data.invalidate_cache();
        self.region1.set_closed(closed);
        self.region2.set_closed(closed);
    }

    /// Clear [`Closed`](Region::is_closed) on this CmpRegion and on both its
    /// component Regions.
    // C: cmpregion.c `ClearClosed`.
    fn clear_closed(&mut self) {
        self.data.closed = true;
        self.data.closed_set = false;
        self.data.invalidate_cache();
        self.region1.clear_closed();
        self.region2.clear_closed();
    }

    fn test_closed(&self) -> bool {
        self.data.closed_set
    }

    fn set_closed_raw(&mut self, closed: bool, set: bool) {
        self.data.closed = closed;
        self.data.closed_set = set;
        self.data.invalidate_cache();
    }

    /// The FillFactor attribute of this CmpRegion: see
    /// [`Region::fill_factor`].
    ///
    /// Default: the FillFactor of its first component Region.
    // C: cmpregion.c `GetFillFactor`.
    fn fill_factor(&self) -> f64 {
        if self.data.fill_factor_set {
            self.data.fill_factor
        } else {
            self.region1.fill_factor()
        }
    }

    fn set_fill_factor(&mut self, factor: f64) -> AstResult<()> {
        self.data.set_fill_factor(factor)
    }

    fn test_fill_factor(&self) -> bool {
        self.data.fill_factor_set
    }

    fn clear_fill_factor(&mut self) {
        self.data.clear_fill_factor();
    }

    /// The MeshSize attribute of this CmpRegion: see [`Region::mesh_size`].
    ///
    /// A CmpRegion is built with the MeshSize of its first component Region,
    /// if that has been set, and setting or clearing it also sets or clears
    /// it on both component Regions.
    fn mesh_size(&self) -> usize {
        self.data.effective_mesh_size()
    }

    /// Set [`MeshSize`](Region::mesh_size) on this CmpRegion and on both its
    /// component Regions.
    // C: cmpregion.c `SetMeshSize`.
    fn set_mesh_size(&mut self, size: usize) {
        self.data.set_mesh_size(size);
        self.region1.set_mesh_size(size);
        self.region2.set_mesh_size(size);
    }

    fn test_mesh_size(&self) -> bool {
        self.data.mesh_size_set
    }

    /// Clear [`MeshSize`](Region::mesh_size) on this CmpRegion and on both
    /// its component Regions.
    // C: cmpregion.c `ClearMeshSize`.
    fn clear_mesh_size(&mut self) {
        self.data.clear_mesh_size();
        self.region1.clear_mesh_size();
        self.region2.clear_mesh_size();
    }

    /// Whether this CmpRegion is bounded: see [`Region::is_bounded`].
    ///
    /// It is worked out from its component Regions, each negated first if
    /// the CmpRegion is negated, when AND and OR also swap.
    /// An OR is bounded when both components are.
    /// An AND is bounded when either component is; when neither is, it is
    /// bounded unless the two overlap partially, do not overlap, or are each
    /// other's negation, as [`overlap`](Region::overlap) finds them.
    // C: cmpregion.c `GetBounded`.
    // An AND of two unbounded Regions, one inside the other, is reported as
    // bounded, as C reports it (docs/issues/c-library-quirks.md).
    fn is_bounded(&self) -> AstResult<bool> {
        self.get_bounded()
    }

    fn is_adaptive(&self) -> bool {
        self.data.adaptive
    }

    fn test_adaptive(&self) -> bool {
        self.data.adaptive_set
    }

    fn set_adaptive_raw(&mut self, adaptive: bool, set: bool) {
        self.data.adaptive = adaptive;
        self.data.adaptive_set = set;
    }

    fn has_dummy_fs(&self) -> bool {
        self.data.dummy_fs
    }

    fn set_region_fs(&mut self, frame: &dyn Frame) {
        // Store the FrameSet on the parent Region structure, then recurse
        // into any dummy-FS component with the same frame (both components
        // span the same axes). Mirrors C `cmpregion.c` SetRegFS
        // (3463-3513).
        self.data.set_region_fs(frame);
        if self.region1.has_dummy_fs() {
            self.region1.set_region_fs(frame);
        }
        if self.region2.has_dummy_fs() {
            self.region2.set_region_fs(frame);
        }
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        super::region_bounds_current(self)
    }

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn reg_base_box2(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // Faithful port of C `cmpregion.c::RegBaseBox2` (cmpregion.c:1719):
        // combine the component boxes assuming neither the CmpRegion nor its
        // components have been negated. The component `get_region_bounds2`
        // gives each component's box in the CmpRegion's base Frame (the
        // components' current Frame); AND intersects, other operators union.
        let naxes = self.data.frameset.base().naxes();
        let (clbnd1, cubnd1) = self.region1.get_region_bounds2()?;
        let (clbnd2, cubnd2) = self.region2.get_region_bounds2()?;

        let mut lbnd = vec![0.0; naxes];
        let mut ubnd = vec![0.0; naxes];
        for i in 0..naxes {
            match self.operator {
                Oper::And => {
                    lbnd[i] = clbnd1[i].max(clbnd2[i]);
                    ubnd[i] = cubnd1[i].min(cubnd2[i]);
                }
                Oper::Or => {
                    lbnd[i] = clbnd1[i].min(clbnd2[i]);
                    ubnd[i] = cubnd1[i].max(cubnd2[i]);
                }
            }
        }
        Ok((lbnd, ubnd))
    }

    fn base_mesh(&self) -> AstResult<PointSet> {
        // Check for a cached mesh first.
        {
            let cached = self.data.cached_base_mesh.borrow();
            if let Some(mesh) = cached.as_ref() {
                return Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())));
            }
        }

        // Compute the mesh.
        let mesh = self.reg_base_mesh()?;
        if mesh.is_empty() {
            // Return a single BAD point (matching C behavior for empty meshes).
            let naxes = self.data.frameset.base().naxes();
            let bad_point = vec![BAD_VALUE; naxes];
            return Ok(PointSet::from_rows(std::iter::once(bad_point.as_slice())));
        }
        let result = PointSet::from_rows(mesh.iter().map(|p| p.as_slice()));

        // Cache it.
        *self.data.cached_base_mesh.borrow_mut() = Some(mesh);

        Ok(result)
    }

    fn base_grid(&self) -> AstResult<PointSet> {
        crate::regions::data::region_base_grid(self)
    }

    fn base_points(&self) -> AstResult<PointSet> {
        // CmpRegion has no defining points of its own.
        let naxes = self.data.frameset.base().naxes();
        Ok(PointSet::zeros(0, naxes))
    }

    fn get_region_frame(&self) -> DynFrame {
        self.data.frameset.current().snapshot()
    }

    fn get_region_frameset(&self) -> FrameSet {
        self.data.frameset.clone()
    }

    fn region_frameset_mut(&mut self) -> &mut FrameSet {
        &mut self.data.frameset
    }

    fn store_uncertainty(&mut self, unc: Option<DynRegion>) {
        self.data.uncertainty = unc;
        self.data.invalidate_cache();
    }

    fn test_region_fs(&self) -> bool {
        self.data.region_fs_set
    }

    fn set_region_fs_raw(&mut self, value: bool, set: bool) {
        self.data.region_fs = value;
        self.data.region_fs_set = set;
    }

    fn region_fs_flag(&self) -> bool {
        self.data.region_fs
    }

    fn clear_region_fs_deep(&mut self) {
        // C copies both component Regions (cmpregion.c:4165-4166), and each copy clears its own
        // `RegionFS` (region.c:12936-12940).
        self.clear_region_fs();
        if let Some(mut unc) = self.get_uncertainty() {
            unc.clear_region_fs_deep();
            self.store_uncertainty(Some(unc));
        }
        self.region1.clear_region_fs_deep();
        self.region2.clear_region_fs_deep();
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn get_def_unc(&self) -> AstResult<DynRegion> {
        // Port of cmpregion.c::GetDefUnc (cmpregion.c:1090). Both component
        // Regions share the CmpRegion's base Frame in their current Frame. If a
        // component carries an explicit uncertainty, use it; otherwise defer to
        // the base-class bounding-box default.
        if self.region1.test_unc() {
            self.region1.get_unc_frm(FrameRef::Current)
        } else if self.region2.test_unc() {
            self.region2.get_unc_frm(FrameRef::Current)
        } else {
            self.default_bounding_box_unc()
        }
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        // Port of cmpregion.c::RegBasePick (cmpregion.c:2096). Both
        // encapsulated regions refer to the same Frame (the base Frame of the
        // parent Region), so pick the same axes from each. Only if both picks
        // yield Regions can we form the sub-CmpRegion.
        let picked1 = match super::pick_axes_region(self.region1.as_ref(), axes)? {
            super::PickedAxes::Region(r) => r,
            super::PickedAxes::Frame(_) => return Ok(None),
        };
        let picked2 = match super::pick_axes_region(self.region2.as_ref(), axes)? {
            super::PickedAxes::Region(r) => r,
            super::PickedAxes::Frame(_) => return Ok(None),
        };

        let result = CmpRegionBuilder::from_dyn(picked1, picked2, self.operator.into())?.build()?;
        Ok(Some(Box::new(result)))
    }

    /// cmpregion.c `RegSetAttrib` and `RegClearAttrib`: the change is made
    /// to the CmpRegion's own Frames, then its base-Frame form to both
    /// components, whose current Frames are that base Frame. A component
    /// without the attribute (AST__BADAT) ends the propagation, the error
    /// annulled, as C's status does.
    fn reg_frame_attrib<'a>(
        &mut self,
        setting: &super::FrameSetting<'a>,
    ) -> AstResult<super::FrameSetting<'a>> {
        let base = super::data::region_frame_attrib(self, setting)?;
        let made = self
            .region1
            .reg_frame_attrib(&base)
            .and_then(|_| self.region2.reg_frame_attrib(&base));
        match made {
            Err(e) if !super::data::is_bad_attribute(&e) => Err(e),
            _ => Ok(base),
        }
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

/// Build a null (empty) or infinite (negated null) Region in the Frame of
/// `othereg`, carrying `othereg`'s uncertainty.
///
/// Mirrors C `astNullRegion( othereg, astGetUnc( othereg, 0 ), ... )`
/// (cmpregion.c:3659): the new NullRegion is defined in `othereg`'s current
/// Frame and takes `othereg`'s explicit uncertainty when one is set (the
/// `def=0` argument to `astGetUnc` means no default uncertainty is substituted).
fn build_null_like(othereg: &DynRegion, negated: bool) -> AstResult<DynRegion> {
    let frame = othereg.get_region_frame();
    let mut builder = NullRegionBuilder::from_dyn(frame);
    if let Some(unc) = othereg.get_uncertainty() {
        builder = builder.uncertainty(unc);
    }
    let mut null = builder.build()?;
    if negated {
        null.negate();
    }
    Ok(Box::new(null) as DynRegion)
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for CmpRegion {
    region_frame_attrs!();

    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    // Mirrors C `Region::MatchAxesX` (region.c:6413): the axes of the
    // current Frame of the Region's FrameSet are matched.
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        self.data.frameset.current().match_axes_x(frm1)
    }

    // Mirrors C `Region::Match` (region.c:6214): the current Frame of the
    // Region's FrameSet is matched, through `astMatch` and so with its
    // cast retry.
    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
        crate::frames::find_frame::match_with_subclass(
            self.data.frameset.current(),
            target,
            match_sub,
        )
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        self.data.frameset.current().primary_frame(axis)
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.data.frameset.current().format(axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.data.frameset.current().unformat(axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().distance(a, b)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().angle(a, b, c)
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        self.data.frameset.current().offset(from, to, dist)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.data.frameset.current().norm(coords)
    }

    // A Region's axes are its encapsulated current Frame's axes. Mirrors
    // C `Region::AxDistance` / `AxOffset` / `AxNorm` (region.c:1457, 1610,
    // 1536), which all forward to that Frame.
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.data.frameset.current().get_axis(axis)
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame.
    fn frame_without_regions(&self) -> Option<DynFrame> {
        Some(self.get_region_frame())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    // C has one `Simplify` slot, so a Region simplifies through its Mapping
    // side and the caller keeps the result because it is still a Frame
    // (`astIsAFrame`, `cmpframe.c:9024`).
    fn simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        simplify_frame_via_mapping(self)
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        self.data.frameset.current().map_from(previous)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        if let Some(other_cmp) = other.as_any().downcast_ref::<CmpRegion>() {
            self.data
                .frameset
                .current()
                .equal(other_cmp.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for CmpRegion {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.data.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.data.mapping_flags
    }

    // A Region is a Frame, which C's frame.c `GetIsLinear` reports as
    // linear.
    fn is_linear(&self) -> bool {
        true
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame, which as a
    /// Mapping is a UnitMap with its number of axes.
    fn mapping_without_regions(&self) -> Option<crate::mappings::DynMapping> {
        Some(Box::new(crate::mappings::UnitMap::new(self.nin())))
    }

    // region.c `ReportPoints`: the current Frame formats the values.
    fn report_points(&self, _forward: bool, inp: &PointSet, out: &PointSet) {
        let frame = self.data.frameset.current();
        crate::mappings::report::write_report(&crate::frames::frame_report_lines(
            frame, frame, inp, out,
        ));
    }

    region_mapping_ident!();

    // C's `Frame` overrides `astDoNotSimplify` to zero (frame.c:3511-3554): a
    // set `Ident` is a user label on a coordinate system, not a request to keep
    // a Mapping intact, so a named Frame is still simplified.
    fn do_not_simplify(&self) -> bool {
        false
    }

    fn nin(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn nout(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        false
    }

    fn invert_orientation(&mut self) {
        // A Region's forward and inverse transforms are identical (membership
        // is direction-independent), so inverting has no effect on behavior.
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        super::data::region_transform(self, pts)
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let result = self.transform(&PointSet::from_points(&pts), forward)?;
        out.copy_from(&result.as_points());
        Ok(())
    }

    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        // Faithful port of cmpregion.c::Simplify (cmpregion.c:3517). The two
        // component Regions live in a Frame equivalent to the parent's base
        // Frame; simplification is performed there and the result mapped into
        // the parent's current Frame before being returned.
        use super::merge_util::mapping_into_region;

        // Invoke the parent Region Simplify first (cmpregion.c:3595): it
        // simplifies the encapsulated FrameSet and any explicit uncertainty.
        // `newc` is that simplified copy (or a clone of self when nothing
        // changed); all subsequent work — the base->current Mapping, the
        // current Frame, and the no-`newb` fallback — is expressed in terms of
        // it, and `simpler` is seeded from whether it differs from self.
        let newc = self
            .parent_region_simplify()?
            .unwrap_or_else(|| self.clone_region());
        let mut simpler = !(newc.as_ref() as &dyn Mapping).equal(self as &dyn Mapping);

        // The current Frame and base->current Mapping of `newc`.
        let newc_fs = newc.get_region_frameset();
        let cfrm = newc_fs.current().snapshot();
        let bcmap = newc_fs.mapping(FrameRef::Base, FrameRef::Current)?;

        // GetRegions (cmpregion.c:981): clone the two component Regions and
        // read the operator and each component's required Negated flag. If the
        // CmpRegion is itself negated, apply the De Morgan rewrite: AND<->OR
        // with both component flags toggled.
        let (mut reg1, mut reg2, oper, neg1, neg2) = self.get_regions();

        // If a component does not already carry its required Negated flag,
        // replace it with its negation (cmpregion.c:3617).
        if neg1 != reg1.is_negated() {
            reg1 = reg1.negated_copy();
        }
        if neg2 != reg2.is_negated() {
            reg2 = reg2.negated_copy();
        }

        // Simplify each of the two components (cmpregion.c:3634).
        let sreg1 = mapping_into_region(<dyn Mapping>::simplify(reg1.as_ref())?)
            .unwrap_or_else(|| reg1.clone_region());
        let sreg2 = mapping_into_region(<dyn Mapping>::simplify(reg2.as_ref())?)
            .unwrap_or_else(|| reg2.clone_region());

        // Note whether either component simplified, OR-ing into the value
        // already seeded from the parent FrameSet/uncertainty simplify
        // (cmpregion.c:3636 `simpler = simpler || ...`).
        simpler = simpler
            || !(sreg1.as_ref() as &dyn Mapping).equal(reg1.as_ref() as &dyn Mapping)
            || !(sreg2.as_ref() as &dyn Mapping).equal(reg2.as_ref() as &dyn Mapping);

        // The (possibly) simplified Region expressed in the base Frame, or
        // None if no base-Frame result was produced (cmpregion.c:3603).
        let mut newb: Option<DynRegion> = None;

        // If either component is null or infinite we can exclude it from the
        // returned Region (cmpregion.c:3644).
        let sreg1_null = sreg1.as_any().is::<NullRegion>();
        let sreg2_null = sreg2.as_any().is::<NullRegion>();
        if sreg1_null || sreg2_null {
            // Select the null/infinite Region and the "other" Region. This is
            // valid even when both are null or infinite (the first is used).
            let (nullreg, othereg): (&DynRegion, &DynRegion) = if sreg1_null {
                (&sreg1, &sreg2)
            } else {
                (&sreg2, &sreg1)
            };

            if !nullreg.is_negated() {
                // Null (empty) component.
                match oper {
                    Oper::And => {
                        // empty AND x = empty.
                        newb = Some(build_null_like(othereg, false)?);
                    }
                    Oper::Or => {
                        // empty OR x = x.
                        newb = Some(othereg.clone_region());
                    }
                }
            } else {
                // Infinite component.
                match oper {
                    Oper::And => {
                        // infinite AND x = x.
                        newb = Some(othereg.clone_region());
                    }
                    Oper::Or => {
                        // infinite OR x = infinite.
                        newb = Some(build_null_like(othereg, true)?);
                    }
                }
            }
            simpler = true;

        // Otherwise, use the overlap between the two components to try to
        // remove one or both of them (cmpregion.c:3695).
        } else {
            let overlap = sreg1.overlap(sreg2.as_ref())?;
            match (overlap, oper) {
                // No overlap (or exact negation) combined with AND: the
                // intersection is empty (cmpregion.c:3699).
                (Overlap::None | Overlap::Negation, Oper::And) => {
                    newb = Some(build_null_like(&sreg1, false)?);
                    simpler = true;
                }
                // One component is the negation of the other, combined with
                // OR: the union is infinite, represented by a negated null
                // Region (cmpregion.c:3706).
                (Overlap::Negation, Oper::Or) => {
                    newb = Some(build_null_like(&sreg1, true)?);
                    simpler = true;
                }
                // The two components are identical: the CmpRegion is either
                // component (cmpregion.c:3712).
                (Overlap::Same, _) => {
                    newb = Some(sreg1.clone_region());
                    simpler = true;
                }
                // First component contained within the second: AND keeps the
                // first, OR keeps the second (cmpregion.c:3725).
                (Overlap::FirstInside, Oper::And) => {
                    newb = Some(sreg1.clone_region());
                    simpler = true;
                }
                (Overlap::FirstInside, Oper::Or) => {
                    newb = Some(sreg2.clone_region());
                    simpler = true;
                }
                // Second component contained within the first: AND keeps the
                // second, OR keeps the first (cmpregion.c:3732).
                (Overlap::SecondInside, Oper::And) => {
                    newb = Some(sreg2.clone_region());
                    simpler = true;
                }
                (Overlap::SecondInside, Oper::Or) => {
                    newb = Some(sreg1.clone_region());
                    simpler = true;
                }
                // No further reduction: rebuild the CmpRegion only if a
                // component actually simplified (cmpregion.c:3739).
                _ => {
                    if simpler {
                        let cr = CmpRegionBuilder::from_dyn(
                            sreg1.clone_region(),
                            sreg2.clone_region(),
                            oper.into(),
                        )?
                        .build()?;
                        newb = Some(Box::new(cr) as DynRegion);
                    }
                }
            }
        }

        // Decide whether to use the base-Frame result "newb" or a clone of the
        // supplied Region (cmpregion.c:3760). A base-Frame result is mapped
        // into the parent's current Frame, and is returned as it stands: the
        // mapped Region is not simplified again.
        if !simpler {
            // C's `else`: `result = astClone( this_mapping )` (cmpregion.c:3773).
            return Ok((self.clone_box(), false));
        }
        let result: DynRegion = if let Some(nb) = newb {
            nb.reg_map_region(bcmap.as_ref(), cfrm.as_ref())?
        } else {
            // No base-Frame result: use the parent-simplified copy
            // (cmpregion.c:3767 `result = astClone( newc )`).
            newc
        };

        Ok((result.clone_box(), true))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    // cmpregion.c `Equal`: region.c `Equal`, then equal components and the
    // same boolean operator.
    fn equal(&self, other: &dyn Mapping) -> bool {
        super::data::region_equal(self, other).is_some_and(|other| {
            (self.region1.as_ref() as &dyn Mapping).equal(other.region1.as_ref() as &dyn Mapping)
                && (self.region2.as_ref() as &dyn Mapping)
                    .equal(other.region2.as_ref() as &dyn Mapping)
                && self.operator == other.operator
        })
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl From<Oper> for CmpOperator {
    fn from(oper: Oper) -> Self {
        match oper {
            Oper::And => CmpOperator::And,
            Oper::Or => CmpOperator::Or,
        }
    }
}

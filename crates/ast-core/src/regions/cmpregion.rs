//! CmpRegion: a compound region combining two sub-regions with AND or OR.
//!
//! A [`CmpRegion`] represents either the intersection (AND) or union (OR)
//! of two component regions. The component regions must have the same
//! number of axes and be defined in the same coordinate frame.
//!
//! For AND (intersection): a point is inside the CmpRegion if it is
//! inside *both* component regions.
//!
//! For OR (union): a point is inside the CmpRegion if it is inside
//! *either* (or both) component regions.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::{CmpOperator, DynRegion, NullRegion, NullRegionBuilder, Overlap, Region};

/// A compound region combining two sub-regions with AND (intersection)
/// or OR (union).
///
/// The component regions are stored as boxed trait objects and are
/// tested independently. The CmpRegion's frame is taken from the
/// first component region.
#[derive(Debug, Clone)]
pub struct CmpRegion {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
    /// First component region.
    pub(crate) region1: DynRegion,
    /// Second component region.
    pub(crate) region2: DynRegion,
    /// Boolean operator combining the two regions.
    pub(crate) operator: CmpOperator,
}

/// Builder for constructing a [`CmpRegion`].
pub struct CmpRegionBuilder {
    region1: DynRegion,
    region2: DynRegion,
    operator: CmpOperator,
    uncertainty: Option<DynRegion>,
}

impl CmpRegionBuilder {
    /// Create a builder for a CmpRegion combining two regions.
    ///
    /// Both regions must have the same number of axes.
    pub fn new(
        region1: impl Region + 'static,
        region2: impl Region + 'static,
        operator: CmpOperator,
    ) -> AstResult<Self> {
        let n1 = region1.naxes();
        let n2 = region2.naxes();
        if n1 != n2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "CmpRegion component regions have different numbers of axes ({} vs {})",
                    n1, n2
                ),
            ));
        }
        Ok(CmpRegionBuilder {
            region1: Box::new(region1),
            region2: Box::new(region2),
            operator,
            uncertainty: None,
        })
    }

    /// Create a builder from boxed (dynamic) regions.
    ///
    /// Both regions must have the same number of axes.
    pub fn from_dyn(
        region1: DynRegion,
        region2: DynRegion,
        operator: CmpOperator,
    ) -> AstResult<Self> {
        let n1 = region1.naxes();
        let n2 = region2.naxes();
        if n1 != n2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "CmpRegion component regions have different numbers of axes ({} vs {})",
                    n1, n2
                ),
            ));
        }
        Ok(CmpRegionBuilder {
            region1,
            region2,
            operator,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the CmpRegion.
    pub fn build(self) -> AstResult<CmpRegion> {
        // Use region1's frame as the CmpRegion's frame.
        let frame = self.region1.get_region_frame();
        let frameset = FrameSet::new(frame);

        // CmpRegion has no defining points of its own (geometry is in sub-regions).
        let points = Vec::new();
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(CmpRegion {
            data,
            region1: self.region1,
            region2: self.region2,
            operator: self.operator,
        })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl CmpRegion {
    /// Mutate the current frame with region integrity enforcement.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
    {
        super::data::with_region_integrity(self, f)
    }

    /// Permute the axes of this region's current frame.
    pub fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.data.perm_axes(perm)
    }

    /// Get the boolean operator combining the two component regions.
    pub fn operator(&self) -> CmpOperator {
        self.operator
    }

    /// Get a reference to the first component region.
    pub fn region1(&self) -> &dyn Region {
        self.region1.as_ref()
    }

    /// Get a reference to the second component region.
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
    fn get_regions(&self) -> (DynRegion, DynRegion, CmpOperator, bool, bool) {
        let reg1 = self.region1.clone_region();
        let reg2 = self.region2.clone_region();
        let mut oper = self.operator;
        let mut neg1 = self.region1.is_negated();
        let mut neg2 = self.region2.is_negated();
        if self.data.negated {
            oper = match oper {
                CmpOperator::And => CmpOperator::Or,
                CmpOperator::Or => CmpOperator::And,
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
    /// [`map_region`](Region::map_region) `that` through the base->current
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
        let fs = that_frm
            .convert_to(target_frm.as_ref(), &[])?
            .ok_or_else(|| {
                AstError::new(
                    ErrorCode::Inter,
                    "MatchRegion: cannot convert between the two supplied coordinate Frames"
                        .to_string(),
                )
            })?;
        let map = fs.mapping(FrameRef::Base, FrameRef::Current)?;
        that.map_region(map.as_ref(), target_frm.as_ref())
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

        let reg1b = reg1.is_bounded();
        let reg2b = reg2.is_bounded();

        let result = match oper {
            CmpOperator::And => {
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
            CmpOperator::Or => reg1b && reg2b,
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

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let naxes = self.data.frameset.base().naxes();

        // Test each point against both sub-regions using point_in_region.
        // The sub-regions handle their own negation and frame conversion internally.
        // Points arrive in the CmpRegion's base frame, which is the same as
        // the current frame of the sub-regions (since we use region1's frame).
        let mut result = vec![false; npoint];

        match self.operator {
            CmpOperator::And => {
                // AND (intersection): point must be inside both regions.
                // Short-circuit: test region1 first, then only test region2
                // for points that passed region1.
                for (r, point) in result.iter_mut().zip((0..npoint).map(|i| pts.point(i))) {
                    if point_has_bad(&point[..naxes]) {
                        continue;
                    }
                    let in1 = self.region1.point_in_region(&point[..naxes])?;
                    if !in1 {
                        continue;
                    }
                    let in2 = self.region2.point_in_region(&point[..naxes])?;
                    *r = in2;
                }
            }
            CmpOperator::Or => {
                // OR (union): point must be inside at least one region.
                // Short-circuit: test region1 first, only test region2
                // for points that failed region1.
                for (r, point) in result.iter_mut().zip((0..npoint).map(|i| pts.point(i))) {
                    if point_has_bad(&point[..naxes]) {
                        continue;
                    }
                    let in1 = self.region1.point_in_region(&point[..naxes])?;
                    if in1 {
                        *r = true;
                        continue;
                    }
                    let in2 = self.region2.point_in_region(&point[..naxes])?;
                    *r = in2;
                }
            }
        }

        Ok(result)
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
                CmpOperator::And => {
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
                CmpOperator::Or => {
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

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let naxes = self.data.frameset.base().naxes();

        // Get boundary meshes from both sub-regions. The sub-regions' current
        // frame = CmpRegion's base frame, so mesh points are already in the
        // correct coordinate system.
        let mesh1 = self.region1.get_region_mesh()?;
        let mesh2 = self.region2.get_region_mesh()?;

        // For AND (intersection): boundary of the intersection consists of
        // boundary points of region1 that lie inside region2, plus boundary
        // points of region2 that lie inside region1.
        //
        // For OR (union): boundary of the union consists of boundary points
        // of region1 that lie OUTSIDE region2, plus boundary points of
        // region2 that lie OUTSIDE region1.
        //
        // The C library achieves this by using regions as mappings:
        // forward-transform passes interior points through, sets exterior to BAD.
        // For OR it temporarily negates the regions (so "inside" becomes "outside").

        // Use the region transform to filter mesh points.
        // For AND: transform mesh1 through region2 (keeps points inside region2)
        //          transform mesh2 through region1 (keeps points inside region1)
        // For OR:  negate and transform (keeps points outside the other region)

        let (filtered1, filtered2) = match self.operator {
            CmpOperator::And => {
                // Transform mesh1 through region2: keeps mesh1 points inside region2
                let f1 = self.region2.transform(&mesh1, true)?;
                // Transform mesh2 through region1: keeps mesh2 points inside region1
                let f2 = self.region1.transform(&mesh2, true)?;
                (f1, f2)
            }
            CmpOperator::Or => {
                // For OR we need points OUTSIDE the other region.
                // Use negated copies so the transform keeps exterior points.
                let neg1 = self.region1.negated_copy();
                let neg2 = self.region2.negated_copy();
                // Transform mesh1 through negated region2: keeps mesh1 points outside region2
                let f1 = neg2.transform(&mesh1, true)?;
                // Transform mesh2 through negated region1: keeps mesh2 points outside region1
                let f2 = neg1.transform(&mesh2, true)?;
                (f1, f2)
            }
        };

        // Collect all non-BAD points from both filtered meshes.
        let mut result = Vec::new();

        for i in 0..filtered1.npoint() {
            let point = filtered1.point(i);
            if !point[..naxes].iter().any(|&v| is_bad(v)) {
                result.push(point[..naxes].to_vec());
            }
        }

        for i in 0..filtered2.npoint() {
            let point = filtered2.point(i);
            if !point[..naxes].iter().any(|&v| is_bad(v)) {
                result.push(point[..naxes].to_vec());
            }
        }

        Ok(result)
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        // Use the midpoint of the bounding box if available,
        // otherwise fall back to the first sub-region's centre.
        let (lbnd, ubnd) = self.reg_base_box()?;
        let naxes = lbnd.len();

        // Check if any bounds are BAD.
        let all_valid = (0..naxes).all(|i| !is_bad(lbnd[i]) && !is_bad(ubnd[i]));

        if all_valid {
            let centre: Vec<f64> = (0..naxes).map(|i| 0.5 * (lbnd[i] + ubnd[i])).collect();
            Ok(centre)
        } else {
            // Fall back to first sub-region's centre via get_region_disc.
            match self.region1.get_region_disc() {
                Ok((centre, _)) => Ok(centre),
                Err(_) => {
                    // If that also fails, return BAD on all axes.
                    Ok(vec![BAD_VALUE; naxes])
                }
            }
        }
    }

    fn is_bounded_inner(&self) -> bool {
        match self.operator {
            // AND (intersection): bounded if either sub-region is bounded.
            CmpOperator::And => self.region1.is_bounded() || self.region2.is_bounded(),
            // OR (union): bounded only if both sub-regions are bounded.
            CmpOperator::Or => self.region1.is_bounded() && self.region2.is_bounded(),
        }
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
            CmpOperator::And => {
                let in1 = self.region1.point_in_region(&base_point[..naxes])?;
                if !in1 {
                    false
                } else {
                    self.region2.point_in_region(&base_point[..naxes])?
                }
            }
            CmpOperator::Or => {
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

    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap> {
        super::overlap::compute_overlap(self, other)
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
            CmpOperator::And => (reg1.transform(pts, true)?, reg2.transform(pts, true)?),
            CmpOperator::Or => {
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
        copy.data.negated = !copy.data.negated;
        Box::new(copy)
    }

    fn negate(&mut self) {
        self.data.negated = !self.data.negated;
    }

    fn is_negated(&self) -> bool {
        self.data.negated
    }

    fn is_closed(&self) -> bool {
        self.data.closed
    }

    fn set_closed(&mut self, closed: bool) {
        self.data.closed = closed;
        self.data.closed_set = true;
    }

    fn test_closed(&self) -> bool {
        self.data.closed_set
    }

    fn set_closed_raw(&mut self, closed: bool, set: bool) {
        self.data.closed = closed;
        self.data.closed_set = set;
    }

    fn fill_factor(&self) -> f64 {
        self.data.effective_fill_factor()
    }

    fn set_fill_factor(&mut self, factor: f64) {
        self.data.set_fill_factor(factor);
    }

    fn test_fill_factor(&self) -> bool {
        self.data.fill_factor_set
    }

    fn clear_fill_factor(&mut self) {
        self.data.clear_fill_factor();
    }

    fn mesh_size(&self) -> usize {
        self.data.effective_mesh_size()
    }

    fn set_mesh_size(&mut self, size: usize) {
        self.data.set_mesh_size(size);
    }

    fn test_mesh_size(&self) -> bool {
        self.data.mesh_size_set
    }

    fn clear_mesh_size(&mut self) {
        self.data.clear_mesh_size();
    }

    fn is_bounded(&self) -> bool {
        self.get_bounded().unwrap_or(false)
    }

    fn is_adaptive(&self) -> bool {
        self.data.adaptive
    }

    fn set_adaptive(&mut self, adaptive: bool) {
        self.data.adaptive = adaptive;
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
                CmpOperator::And => {
                    lbnd[i] = clbnd1[i].max(clbnd2[i]);
                    ubnd[i] = cubnd1[i].min(cubnd2[i]);
                }
                CmpOperator::Or => {
                    lbnd[i] = clbnd1[i].min(clbnd2[i]);
                    ubnd[i] = cubnd1[i].max(cubnd2[i]);
                }
            }
        }
        Ok((lbnd, ubnd))
    }

    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        // Compute bounding disc from the bounding box.
        let (lbnd, ubnd) = self.reg_base_box()?;
        let naxes = lbnd.len();

        // Compute centre as midpoint of bounding box.
        let mut centre = vec![0.0; naxes];
        let mut sum_sq = 0.0;
        for i in 0..naxes {
            if is_bad(lbnd[i]) || is_bad(ubnd[i]) {
                return Err(AstError::new(
                    ErrorCode::Regcn,
                    "CmpRegion has unbounded axes; cannot compute bounding disc",
                ));
            }
            centre[i] = 0.5 * (lbnd[i] + ubnd[i]);
            let half_width = 0.5 * (ubnd[i] - lbnd[i]);
            sum_sq += half_width * half_width;
        }
        let radius = sum_sq.sqrt();

        Ok((centre, radius))
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
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
            // Return a single BAD point (matching C behaviour for empty meshes).
            let naxes = self.data.frameset.base().naxes();
            let bad_point = vec![BAD_VALUE; naxes];
            return Ok(PointSet::from_rows(std::iter::once(bad_point.as_slice())));
        }
        let result = PointSet::from_rows(mesh.iter().map(|p| p.as_slice()));

        // Cache it.
        *self.data.cached_base_mesh.borrow_mut() = Some(mesh);

        Ok(result)
    }

    fn get_region_grid(&self, npoints: usize) -> AstResult<Vec<Vec<f64>>> {
        self.reg_base_grid(npoints)
    }

    fn get_region_points(&self) -> AstResult<PointSet> {
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

    fn set_region_fs_flag(&mut self, value: bool) {
        self.data.set_region_fs_flag(value);
    }

    fn region_fs_flag(&self) -> bool {
        self.data.region_fs
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

        let result = CmpRegionBuilder::from_dyn(picked1, picked2, self.operator)?.build()?;
        Ok(Some(Box::new(result)))
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

/// Build a null (empty) or infinite (negated null) Region in the Frame of
/// `othereg`, carrying `othereg`'s uncertainty.
///
/// Mirrors C `astNullRegion( othereg, astGetUnc( othereg, 0 ), ... )`
/// (cmpregion.c:3654): the new NullRegion is defined in `othereg`'s current
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
    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
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

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    /// C has one `Simplify` slot, so a Region simplifies through its Mapping
    /// side and the caller keeps the result because it is still a Frame
    /// (`astIsAFrame`, `cmpframe.c:9024`).
    fn simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        simplify_frame_via_mapping(self)
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        self.data.frameset.current().map_from(previous)
    }

    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        self.data.frameset.current().convert_to(to, domains)
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

    fn invert(&mut self) {
        // A Region's forward and inverse transforms are identical (membership
        // is direction-independent), so inverting has no effect on behaviour.
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        super::data::region_transform(self, pts, forward)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let result = self.transform(pts, forward)?;
        let npoint = result.npoint();
        let ncoord = result.ncoord();
        for i in 0..npoint {
            let src = result.point(i);
            let dst = out.point_mut(i);
            dst[..ncoord].copy_from_slice(src);
        }
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
                    CmpOperator::And => {
                        // empty AND x = empty.
                        newb = Some(build_null_like(othereg, false)?);
                    }
                    CmpOperator::Or => {
                        // empty OR x = x.
                        newb = Some(othereg.clone_region());
                    }
                }
            } else {
                // Infinite component.
                match oper {
                    CmpOperator::And => {
                        // infinite AND x = x.
                        newb = Some(othereg.clone_region());
                    }
                    CmpOperator::Or => {
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
                (Overlap::None | Overlap::Negation, CmpOperator::And) => {
                    newb = Some(build_null_like(&sreg1, false)?);
                    simpler = true;
                }
                // One component is the negation of the other, combined with
                // OR: the union is infinite, represented by a negated null
                // Region (cmpregion.c:3706).
                (Overlap::Negation, CmpOperator::Or) => {
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
                (Overlap::FirstInside, CmpOperator::And) => {
                    newb = Some(sreg1.clone_region());
                    simpler = true;
                }
                (Overlap::FirstInside, CmpOperator::Or) => {
                    newb = Some(sreg2.clone_region());
                    simpler = true;
                }
                // Second component contained within the first: AND keeps the
                // second, OR keeps the first (cmpregion.c:3732).
                (Overlap::SecondInside, CmpOperator::And) => {
                    newb = Some(sreg2.clone_region());
                    simpler = true;
                }
                (Overlap::SecondInside, CmpOperator::Or) => {
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
                            oper,
                        )?
                        .build()?;
                        newb = Some(Box::new(cr) as DynRegion);
                    }
                }
            }
        }

        // Decide whether to use the base-Frame result "newb" or a clone of the
        // supplied Region (cmpregion.c:3760). A base-Frame result is mapped
        // into the parent's current Frame and simplified.
        if !simpler {
            // C's `else`: `result = astClone( this_mapping )` (cmpregion.c:3773).
            return Ok((self.clone_box(), false));
        }
        let result: DynRegion = if let Some(nb) = newb {
            let mapped = nb.map_region(bcmap.as_ref(), cfrm.as_ref())?;
            mapping_into_region(<dyn Mapping>::simplify(mapped.as_ref())?).unwrap_or(mapped)
        } else {
            // No base-Frame result: use the parent-simplified copy
            // (cmpregion.c:3767 `result = astClone( newc )`).
            newc
        };

        Ok((result.clone_box(), true))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<CmpRegion>() else {
            return false;
        };
        // Mirror C cmpregion.c::Equal (502): the parent Region check covers
        // class, Negated, Closed, and the current Frame; equivalence then
        // requires equal component Regions and an equal boolean operator.
        self.data.negated == other.data.negated
            && self.data.closed == other.data.closed
            && self.operator == other.operator
            && Frame::equal(self.data.frameset.current(), other.data.frameset.current())
            && (self.region1.as_ref() as &dyn Mapping).equal(other.region1.as_ref() as &dyn Mapping)
            && (self.region2.as_ref() as &dyn Mapping).equal(other.region2.as_ref() as &dyn Mapping)
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

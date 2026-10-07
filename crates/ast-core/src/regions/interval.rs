//! Interval region: axis-aligned bounds on one or more axes of a Frame.
//!
//! An Interval represents upper and/or lower limits on one or more axes.
//! For a point to be inside the region, it must satisfy all axis
//! constraints simultaneously. Each axis may have an upper limit, a lower
//! limit, both, or neither. A bound value of [`BAD_VALUE`] means "no
//! constraint" on that axis in that direction.
//!
//! When both bounds are present on an axis and the lower bound exceeds
//! the upper bound, that axis represents an *excluded* interval (the
//! region is outside the range rather than inside).

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::mappings::helpers::{ast_equals as helper_ast_equals, is_bad, point_has_bad};
use crate::mappings::{AsAny, DynMapping, Mapping, UnitMap, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::merge_util::{MergedRegion, finish_region_merge};
use super::{DynRegion, Overlap, Region};

/// An axis-aligned interval region within a coordinate frame.
///
/// Defined by a lower bound and upper bound for each axis. A bound
/// value of [`BAD_VALUE`] means no constraint in that direction.
/// If `lbnd[i] > ubnd[i]` for axis `i`, that axis uses an excluded
/// interval (points outside `[ubnd[i], lbnd[i]]` pass the test).
#[derive(Debug, Clone)]
pub struct Interval {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing an [`Interval`].
pub struct IntervalBuilder {
    frame: DynFrame,
    lbnd: Vec<f64>,
    ubnd: Vec<f64>,
    uncertainty: Option<DynRegion>,
}

impl IntervalBuilder {
    /// Create a builder for an Interval in the given frame.
    ///
    /// `lbnd` and `ubnd` must each have length equal to `frame.naxes()`.
    /// Use [`BAD_VALUE`] for any bound that should be unconstrained.
    pub fn new(frame: impl Frame + 'static, lbnd: Vec<f64>, ubnd: Vec<f64>) -> AstResult<Self> {
        let naxes = frame.naxes();
        if lbnd.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Interval lower bounds have {} values but frame has {} axes",
                    lbnd.len(),
                    naxes
                ),
            ));
        }
        if ubnd.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "Interval upper bounds have {} values but frame has {} axes",
                    ubnd.len(),
                    naxes
                ),
            ));
        }
        Ok(IntervalBuilder {
            frame: Box::new(frame),
            lbnd,
            ubnd,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Interval.
    pub fn build(self) -> AstResult<Interval> {
        let frameset = FrameSet::new(self.frame);

        // Defining points: points[0] = lower bounds, points[1] = upper bounds.
        let points = vec![self.lbnd, self.ubnd];
        let data = RegionData::new(frameset, points);
        let mut region = Interval { data };

        // astSetUnc, as C's constructor calls it (region.c:13372).
        if let Some(unc) = self.uncertainty {
            region.set_uncertainty(unc);
        }

        Ok(region)
    }
}

impl Interval {
    /// Build an `Interval` directly from an owned `DynFrame` and explicit
    /// lower/upper bounds, without an intervening `impl Frame` type parameter.
    ///
    /// Used internally where the Frame is only available as a boxed trait
    /// object. `BAD_VALUE` in either array leaves that direction unconstrained,
    /// as with [`IntervalBuilder`].
    pub(crate) fn from_bounds_frame(
        frame: DynFrame,
        lbnd: Vec<f64>,
        ubnd: Vec<f64>,
    ) -> AstResult<Interval> {
        let frameset = FrameSet::new(frame);
        let data = RegionData::new(frameset, vec![lbnd, ubnd]);
        Ok(Interval { data })
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl Interval {
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

    /// Get the effective lower and upper bounds, resolving BAD_VALUE
    /// to ±MAX for internal computations.
    fn effective_bounds(&self) -> (Vec<f64>, Vec<f64>) {
        let naxes = self.data.points[0].len();
        let mut lbnd = Vec::with_capacity(naxes);
        let mut ubnd = Vec::with_capacity(naxes);

        for i in 0..naxes {
            let lb = self.data.points[0][i];
            let ub = self.data.points[1][i];
            lbnd.push(if is_bad(lb) { -f64::MAX } else { lb });
            ubnd.push(if is_bad(ub) { f64::MAX } else { ub });
        }

        (lbnd, ubnd)
    }

    /// Resolve the axis limits into finite values and, when the Interval is
    /// equivalent to a Box, build that Box.
    ///
    /// Faithful port of C `Cache` (interval.c:379-517), which caches both
    /// results on the Interval; the port recomputes them per call, so the
    /// return value carries what C leaves in `this->lbnd`, `this->ubnd` and
    /// `this->box`.
    ///
    /// A missing limit resolves to `∓DBL_MAX`. The Interval is a Box iff every
    /// resolved limit is finite and every axis shares the first axis's
    /// included/excluded orientation; an axis ordering that *excludes* its
    /// interval (`lbnd > ubnd` on axis 1) means C caches no Box at all, so
    /// that yields `None` too.
    ///
    /// The Box is built in the base Frame and then, when the base→current
    /// Mapping is not a `UnitMap`, mapped into the current Frame with
    /// `map_region` (interval.c:495-499) — which extends the Box's FrameSet
    /// rather than moving its points, so the returned Box represents the same
    /// area in the same Frame the Interval represents. The Interval's negation
    /// and its `Closed` value are then transferred (interval.c:509, 512), the
    /// latter as an explicit set because C uses `astSetClosed`.
    /// Faithful port of `interval.c::OneToOne` (interval.c:1661): whether every
    /// output of `map` depends on exactly one input, and the number of inputs
    /// matches the number of outputs.
    ///
    /// C returns the permutation array but only ever uses it as a gate -- the
    /// axis loop reads the transformed limits, not the permutation -- so this
    /// returns the gate.
    fn one_to_one(map: &dyn Mapping) -> AstResult<bool> {
        let nout = map.nout();
        if map.nin() != nout {
            return Ok(false);
        }
        // Known special cases, for speed as in C (interval.c:1722). Each output
        // of these classes is fed by the corresponding input.
        let any = map.as_any();
        if any.is::<crate::mappings::WinMap>()
            || any.is::<crate::mappings::ZoomMap>()
            || any.is::<UnitMap>()
            || any.is::<crate::mappings::ShiftMap>()
        {
            return Ok(true);
        }
        for i in 0..nout {
            match map.map_split(&[i + 1]) {
                Some((outs, sub)) if outs.len() == 1 && sub.nout() == 1 => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    /// Faithful port of `interval.c::Overlap` (interval.c:1765), the
    /// Interval-specific `astOverlap` override.
    ///
    /// The generic `Region` implementation needs a finite boundary mesh from at
    /// least one operand, which an unbounded Interval cannot supply. This
    /// implementation instead transfers the axis limits of `that` into the base
    /// Frame of `this` and compares them axis by axis.
    ///
    /// Returns `Ok(None)` where C leaves `result` at zero, which is C's signal
    /// to fall back to the inherited implementation (interval.c:2179).
    fn interval_overlap(&self, that: &dyn Region) -> AstResult<Option<i32>> {
        // C's specialised branch requires both operands to be Intervals
        // (interval.c:1888).
        let Some(that_iv) = that.as_any().downcast_ref::<Interval>() else {
            return Ok(None);
        };

        // astConvert(that, this): base->current runs that_current ->
        // this_current (interval.c:1892).
        let Some(fs) = that
            .get_region_frame()
            .convert_to(self.get_region_frame().as_ref(), &[])?
        else {
            return Ok(None);
        };

        // that_base -> that_current -> this_current -> this_base
        // (interval.c:1898-1913).
        let map1 = that_iv
            .get_region_frameset()
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let map2 = fs.mapping(FrameRef::Base, FrameRef::Current)?;
        let map3 = self
            .get_region_frameset()
            .mapping(FrameRef::Current, FrameRef::Base)?;
        let tmap = crate::mappings::CmpMap::new(
            map1.clone_box(),
            map2.clone_box(),
            crate::mappings::Composition::Series,
        )?;
        let map = crate::mappings::CmpMap::new(
            Box::new(tmap),
            map3.clone_box(),
            crate::mappings::Composition::Series,
        )?;
        let smap = <dyn Mapping>::simplify(&map)?;

        // Only proceed if each output depends on one input (interval.c:1917).
        if !Self::one_to_one(smap.as_ref())? {
            return Ok(None);
        }

        // Uncertainty widths, in the base Frame of `this` (interval.c:1921-1936).
        let unc_this = self.get_unc_frm(FrameRef::Base)?;
        let unc_temp = that_iv.get_unc_frm(FrameRef::Base)?;
        let this_fset = self.get_region_frameset();
        let this_base_frame = this_fset.frame(FrameRef::Base)?;
        let unc_that = unc_temp.map_region(smap.as_ref(), this_base_frame)?;
        let (lbndu_this, ubndu_this) = unc_this.get_region_bounds()?;
        let (lbndu_that, ubndu_that) = unc_that.get_region_bounds()?;

        // The limits of `that`, transferred into the base Frame of `this`
        // (interval.c:1944).
        let that_pts = &that_iv.data.points;
        let nc = self.data.points[0].len();
        let pset = PointSet::from_rows(that_pts.iter().map(|p| p.as_slice()));
        let mapped = smap.transform(&pset, true)?;

        // newResult[old - 1][ov - 1] (interval.c:1869).
        const NEW_RESULT: [[i32; 5]; 5] = [
            [1, 1, 1, 1, 1],
            [1, 2, 4, 4, 2],
            [1, 4, 3, 4, 3],
            [1, 4, 4, 4, 4],
            [1, 2, 3, 4, 5],
        ];

        let mut result = 0i32;
        for ic in 0..nc {
            // Joint uncertainty width on this axis (interval.c:1954-1960).
            let err_this = ubndu_this[ic] - lbndu_this[ic];
            let err_that = ubndu_that[ic] - lbndu_that[ic];
            let err = (err_that * err_that + err_this * err_this).sqrt();

            let mut lb_this = self.data.points[0][ic];
            let mut ub_this = self.data.points[1][ic];
            let mut lb_that = mapped.point(0)[ic];
            let mut ub_that = mapped.point(1)[ic];

            // Mapping the limits may have swapped them. Unswap, so that the
            // swap is not read as a request for an excluded interval
            // (interval.c:1971-1988).
            if !is_bad(lb_that) && !is_bad(ub_that) {
                let swapped = if that_pts[0][ic] < that_pts[1][ic] {
                    lb_that > ub_that
                } else {
                    lb_that < ub_that
                };
                if swapped {
                    std::mem::swap(&mut lb_that, &mut ub_that);
                }
            }

            // An open Region excludes its boundary: shrink the limits by the
            // smallest representable amount (interval.c:1990-2002).
            if !that_iv.is_closed() {
                if !is_bad(lb_that) && lb_that < f64::MAX {
                    lb_that += f64::EPSILON * lb_that.abs();
                }
                if !is_bad(ub_that) && ub_that > -f64::MAX {
                    ub_that -= f64::EPSILON * ub_that.abs();
                }
            }
            if !self.is_closed() {
                if !is_bad(lb_this) && lb_this < f64::MAX {
                    lb_this += f64::EPSILON * lb_this.abs();
                }
                if !is_bad(ub_this) && ub_this > -f64::MAX {
                    ub_this -= f64::EPSILON * ub_this.abs();
                }
            }

            // A missing limit is an extreme value (interval.c:2004-2008).
            if is_bad(lb_this) {
                lb_this = -f64::MAX;
            }
            if is_bad(ub_this) {
                ub_this = f64::MAX;
            }
            if is_bad(lb_that) {
                lb_that = -f64::MAX;
            }
            if is_bad(ub_that) {
                ub_that = f64::MAX;
            }

            // Limits the wrong way round mean an excluded range, not an
            // included one (interval.c:2010-2025).
            let inc_this = lb_this <= ub_this;
            if !inc_this {
                std::mem::swap(&mut lb_this, &mut ub_this);
            }
            let inc_that = lb_that <= ub_that;
            if !inc_that {
                std::mem::swap(&mut lb_that, &mut ub_that);
            }

            // Effective equality of the limits. The astEQUALS test comes first
            // so that two DBL_MAX values compare equal without the subtraction
            // below overflowing (interval.c:2029-2040).
            let mut lb_equal = helper_ast_equals(lb_this, lb_that, 1.0e9);
            if !lb_equal && lb_this.abs() != f64::MAX && lb_that.abs() != f64::MAX {
                lb_equal = (lb_this - lb_that).abs() <= err;
            }
            let mut ub_equal = helper_ast_equals(ub_this, ub_that, 1.0e9);
            if !ub_equal && ub_this.abs() != f64::MAX && ub_that.abs() != f64::MAX {
                ub_equal = (ub_this - ub_that).abs() <= err;
            }

            // The overlap this axis alone implies (interval.c:2046-2097).
            let ov = if lb_equal && ub_equal {
                if inc_this == inc_that { 5 } else { 6 }
            } else if inc_this {
                if inc_that {
                    if lb_that <= lb_this && ub_that >= ub_this {
                        2
                    } else if lb_that >= lb_this && ub_that <= ub_this {
                        3
                    } else if ub_that >= lb_this && lb_that <= ub_this {
                        4
                    } else {
                        1
                    }
                } else if lb_that <= lb_this && ub_that >= ub_this {
                    1
                } else if lb_that >= ub_this || ub_that <= lb_this {
                    2
                } else if lb_this == -f64::MAX && ub_this == f64::MAX {
                    3
                } else {
                    4
                }
            } else if inc_that {
                if lb_this <= lb_that && ub_this >= ub_that {
                    1
                } else if lb_this >= ub_that || ub_this <= lb_that {
                    3
                } else if lb_that == -f64::MAX && ub_that == f64::MAX {
                    2
                } else {
                    4
                }
            } else {
                4
            };

            // Combine with the axes already seen (interval.c:2099-2110).
            result = if ic == 0 {
                ov
            } else {
                NEW_RESULT[(result - 1) as usize][(ov - 1) as usize]
            };
            if result == 1 {
                break;
            }
        }

        // The logic above assumed neither Interval is negated (interval.c:2117).
        let neg_this = self.data.negated;
        let neg_that = that_iv.data.negated;
        result = match result {
            1 => {
                if neg_this {
                    if neg_that { 4 } else { 3 }
                } else if neg_that {
                    2
                } else {
                    1
                }
            }
            2 => {
                if neg_this {
                    if neg_that { 3 } else { 4 }
                } else if neg_that {
                    1
                } else {
                    2
                }
            }
            3 => {
                if neg_this {
                    if neg_that { 2 } else { 1 }
                } else if neg_that {
                    4
                } else {
                    3
                }
            }
            4 => 4,
            5 => {
                if neg_this {
                    if neg_that { 5 } else { 6 }
                } else if neg_that {
                    6
                } else {
                    5
                }
            }
            other => other,
        };

        // Zero is C's signal that this implementation could not decide
        // (interval.c:2179).
        Ok(if result == 0 { None } else { Some(result) })
    }

    fn cache(&self) -> AstResult<(Vec<f64>, Vec<f64>, Option<DynRegion>)> {
        let raw_l = &self.data.points[0];
        let raw_u = &self.data.points[1];
        let nc = raw_l.len();

        let mut lbnd = vec![0.0f64; nc];
        let mut ubnd = vec![0.0f64; nc];
        let mut is_box = true;
        let mut neg = false;
        for i in 0..nc {
            lbnd[i] = if is_bad(raw_l[i]) {
                -f64::MAX
            } else {
                raw_l[i]
            };
            ubnd[i] = if is_bad(raw_u[i]) { f64::MAX } else { raw_u[i] };
            if lbnd[i].abs() == f64::MAX {
                is_box = false;
            }
            if ubnd[i].abs() == f64::MAX {
                is_box = false;
            }
            if i == 0 {
                neg = lbnd[i] > ubnd[i];
            } else if neg {
                if lbnd[i] < ubnd[i] {
                    is_box = false;
                }
            } else if lbnd[i] > ubnd[i] {
                is_box = false;
            }
        }

        if !is_box || neg {
            return Ok((lbnd, ubnd, None));
        }

        let bfrm = self.data.frameset.base().snapshot();
        let mut bbox = super::BoxRegion::from_bounds_frame(bfrm, &lbnd, &ubnd)?;

        // C passes the base-Frame uncertainty to `astBox`, which installs it
        // through `astSetUnc` like any other constructor (interval.c:492).
        if self.data.uncertainty.is_some() {
            let unc = self.get_unc_frm(FrameRef::Base)?;
            bbox.set_uncertainty(unc);
        }

        let map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let mut result: DynRegion = if map.as_any().is::<UnitMap>() {
            Box::new(bbox)
        } else {
            let cfrm = self.data.frameset.current().snapshot();
            bbox.map_region(map.as_ref(), cfrm.as_ref())?
        };

        if self.data.negated {
            result.negate();
        }
        result.set_closed(self.data.closed);
        Ok((lbnd, ubnd, Some(result)))
    }

    /// Re-express this Interval in its current Frame across a non-UnitMap
    /// base->current mapping. Faithful port of the unbounded branch of
    /// `interval.c::Simplify` (3216-3517).
    ///
    /// The strategy: build a fully-bounded Box in the base Frame (giving
    /// unbounded axes arbitrary finite limits), map it into the current
    /// Frame and simplify it. The Box only simplifies when the mapping has a
    /// 1-to-1 axis correspondence (e.g. scale/shift/permutation) — for a
    /// shear or rotation it does not, in which case no simplification is
    /// possible and a clone of the original Interval (with its FrameSet) is
    /// returned. When the Box does simplify, the current-Frame axis limits
    /// are recovered from the simplified Box, transferring any missing/equal
    /// /reversed limits from the correct base Frame axis.
    fn simplify_remapped(&self, map: &dyn Mapping) -> AstResult<Option<DynMapping>> {
        use crate::constants::BAD_VALUE;

        let nc = self.data.points[0].len();
        let raw_l = &self.data.points[0];
        let raw_u = &self.data.points[1];

        // 1. Copy the bounds, replacing missing limits with arbitrary
        //    non-BAD values (interval.c:3232-3264).
        let mut lbnd = vec![0.0f64; nc];
        let mut ubnd = vec![0.0f64; nc];
        for ic in 0..nc {
            lbnd[ic] = raw_l[ic];
            ubnd[ic] = raw_u[ic];

            if is_bad(ubnd[ic]) {
                ubnd[ic] = if is_bad(lbnd[ic]) {
                    1.0
                } else if lbnd[ic] > 0.0 {
                    lbnd[ic] * 1.01
                } else if lbnd[ic] < 0.0 {
                    lbnd[ic] * 0.99
                } else {
                    1.0
                };
            }
            if is_bad(lbnd[ic]) {
                lbnd[ic] = if ubnd[ic] > 0.0 {
                    ubnd[ic] * 0.99
                } else if ubnd[ic] < 0.0 {
                    ubnd[ic] * 1.01
                } else {
                    1.0
                };
            }
        }

        // 2. Round-trip the two corners through the mapping (forward then
        //    inverse) and expand the box to enclose the returned base-Frame
        //    positions (interval.c:3267-3294).
        // Layout: point 0 = lower corner, point 1 = upper corner.
        let mut corners = PointSet::zeros(2, nc);
        corners.point_mut(0).copy_from_slice(&lbnd);
        corners.point_mut(1).copy_from_slice(&ubnd);
        let fwd = map.transform(&corners, true)?;
        let back = map.transform(&fwd, false)?;
        for ic in 0..nc {
            let lb = back.point(0)[ic];
            if !is_bad(lb) {
                if lb < lbnd[ic] {
                    lbnd[ic] = lb;
                }
                if lb > ubnd[ic] {
                    ubnd[ic] = lb;
                }
            }
            let ub = back.point(1)[ic];
            if !is_bad(ub) {
                if ub < lbnd[ic] {
                    lbnd[ic] = ub;
                }
                if ub > ubnd[ic] {
                    ubnd[ic] = ub;
                }
            }
        }

        // 3. Clamp the box so it does not exceed the Interval's own limits
        //    (interval.c:3298-3333). Uses the effective (±MAX) bounds and
        //    respects excluded (lb > ub) axes.
        let (elb, eub) = self.effective_bounds();
        for ic in 0..nc {
            let lb = elb[ic];
            let ub = eub[ic];
            if lb <= ub {
                if lbnd[ic] < lb {
                    lbnd[ic] = lb;
                } else if lbnd[ic] > ub {
                    lbnd[ic] = ub;
                }
                if ubnd[ic] < lb {
                    ubnd[ic] = lb;
                } else if ubnd[ic] > ub {
                    ubnd[ic] = ub;
                }
            } else {
                let lwid = lb - lbnd[ic];
                let uwid = ubnd[ic] - ub;
                if lwid > uwid {
                    if lbnd[ic] > lb {
                        lbnd[ic] = lb;
                    }
                    if ubnd[ic] > lb {
                        ubnd[ic] = lb;
                    }
                } else {
                    if lbnd[ic] < ub {
                        lbnd[ic] = ub;
                    }
                    if ubnd[ic] < ub {
                        ubnd[ic] = ub;
                    }
                }
            }

            // Ensure the bounds are not equal (interval.c:3326-3332).
            if lbnd[ic] == 0.0 && ubnd[ic] == 0.0 {
                ubnd[ic] = 1.0;
            } else if crate::mappings::helpers::ast_equals(lbnd[ic], ubnd[ic], 1.0e9) {
                ubnd[ic] = lbnd[ic].max(ubnd[ic]) * (1.0e6 * f64::EPSILON);
            }
        }

        // 4. Build the base-Frame Box, map it into the current Frame, and
        //    simplify it (interval.c:3335-3346). Box stores its defining
        //    points as [centre, corner].
        let base_frame = self.data.frameset.base().snapshot();
        let centre: Vec<f64> = (0..nc).map(|i| 0.5 * (lbnd[i] + ubnd[i])).collect();
        let corner: Vec<f64> = ubnd.clone();
        let mut box_reg = {
            let frameset = FrameSet::new(base_frame);
            // C constructs this Box with `astBox( bfrm, 1, lbnd, ubnd, unc, "" )`
            // and no astSetClosed (interval.c:3338), so Closed keeps its default
            // and stays unset. Only Cache's equivalent Box gets an explicit
            // Closed (interval.c:512).
            let data = RegionData::new(frameset, vec![centre, corner]);
            super::BoxRegion { data }
        };
        // The `unc` argument is this Interval's uncertainty read in the base
        // Frame, which is where the Box's own defining points live
        // (interval.c:3337).
        if self.data.uncertainty.is_some() {
            let unc = self.get_unc_frm(FrameRef::Base)?;
            box_reg.set_uncertainty(unc);
        }
        let current_frame = self.data.frameset.current().snapshot();
        let box2 = box_reg.map_region(map, current_frame.as_ref())?;
        let sreg = (box2.as_ref() as &dyn Mapping).simplify()?;

        // The Box only simplifies (changes concrete geometry) when the
        // mapping preserves a 1-to-1 axis correspondence. Detect this: our
        // BoxRegion::simplified returns an unchanged clone (still a Box whose
        // base->current mapping is *not* a UnitMap) when it cannot simplify.
        //
        // A NullRegion is the answer in its own right: it is already expressed
        // in the current Frame, so C uses it as the simplified Region
        // (interval.c:3352-3355).
        if let Some(null) = sreg.as_any().downcast_ref::<super::NullRegion>() {
            return Ok(Some(Box::new(null.clone())));
        }
        let sreg_is_box = sreg.as_any().downcast_ref::<super::BoxRegion>();
        let sreg_is_interval = sreg.as_any().downcast_ref::<Interval>();

        // Recover the simplified Box/Interval's base bounding box; bail to a
        // clone if it did not reduce to a simple axis-aligned Region.
        let (mut slbnd, mut subnd, snc) = if let Some(b) = sreg_is_box {
            // A simplified Box has a UnitMap base->current mapping.
            let m = b.data.frameset.mapping(FrameRef::Base, FrameRef::Current)?;
            if !m.as_any().is::<UnitMap>() {
                return Ok(None);
            }
            let (lb, ub) = b.reg_base_box()?;
            let snc = lb.len();
            (lb, ub, snc)
        } else if let Some(iv) = sreg_is_interval {
            let m = iv
                .data
                .frameset
                .mapping(FrameRef::Base, FrameRef::Current)?;
            if !m.as_any().is::<UnitMap>() {
                return Ok(None);
            }
            let snc = iv.data.points[0].len();
            (iv.data.points[0].clone(), iv.data.points[1].clone(), snc)
        } else {
            // `astSimplify` of a Region always yields a Region, and the two
            // classes above plus the NullRegion handled earlier are every class
            // C admits here (interval.c:3352-3358).
            return Ok(None);
        };

        // 5. Determine the base<->current axis correspondence and transfer
        //    missing/equal/reversed limits (interval.c:3377-3484).
        //
        // Build a PointSet with the simplified Box lower bounds plus one
        // extra displaced point per axis, transform it into the base Frame,
        // and see which base axis each current axis is parallel to.
        let mut probe = PointSet::zeros(snc + 1, snc);
        for ic in 0..snc {
            for jc in 0..(snc + 1) {
                probe.point_mut(jc)[ic] = slbnd[ic];
            }
            probe.point_mut(ic + 1)[ic] = subnd[ic];
        }
        let probe_base = map.transform(&probe, false)?;

        let bfrm = self.data.frameset.base();
        for ic in 0..snc {
            // Find the base Frame axis with the largest displacement.
            let mut maxd = -f64::MAX;
            let mut bax: isize = -1;
            for jc in 0..nc {
                let d = bfrm
                    .ax_distance(
                        jc + 1,
                        probe_base.point(0)[jc],
                        probe_base.point(ic + 1)[jc],
                    )
                    .map(|v| v.abs())
                    .unwrap_or(BAD_VALUE);
                if !is_bad(d) && d > maxd {
                    maxd = d;
                    bax = jc as isize;
                }
            }

            if maxd <= 0.0 {
                // No corresponding base axis (an axis introduced by a
                // PermMap) — collapse to a single value.
                if is_bad(slbnd[ic]) || is_bad(subnd[ic]) {
                    slbnd[ic] = BAD_VALUE;
                } else {
                    slbnd[ic] = 0.5 * (slbnd[ic] + subnd[ic]);
                }
                subnd[ic] = slbnd[ic];
            } else {
                let bax = bax as usize;
                // Transfer missing limits from the base Frame axis.
                if is_bad(raw_l[bax]) {
                    slbnd[ic] = BAD_VALUE;
                }
                if is_bad(raw_u[bax]) {
                    subnd[ic] = BAD_VALUE;
                }
                if raw_u[bax] == raw_l[bax] {
                    subnd[ic] = slbnd[ic];
                } else if raw_u[bax] > raw_l[bax] {
                    // Inclusion: ensure new limits are the same way round.
                    if subnd[ic] < slbnd[ic] {
                        std::mem::swap(&mut subnd[ic], &mut slbnd[ic]);
                    }
                } else if subnd[ic] > slbnd[ic] {
                    // Exclusion: ensure new limits are reversed.
                    std::mem::swap(&mut subnd[ic], &mut slbnd[ic]);
                }
            }
        }

        // 6. Build the simplified Interval from the current-Frame limits.
        let frameset = FrameSet::new(current_frame);
        let mut data = RegionData::new(frameset, vec![slbnd, subnd]);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.mesh_size_set = self.data.mesh_size_set;
        data.fill_factor = self.data.fill_factor;
        data.fill_factor_set = self.data.fill_factor_set;
        data.adaptive = self.data.adaptive;
        let mut result = Interval { data };
        // C hands the constructor the uncertainty read in the *current* Frame
        // (interval.c:3479), and the constructor's SetUnc maps it back into the
        // new Interval's base Frame -- which is that same current Frame here, so
        // the stored uncertainty is this Interval's carried through the
        // base->current Mapping.
        if self.data.uncertainty.is_some() {
            let unc = self.get_unc_frm(FrameRef::Current)?;
            result.set_uncertainty(unc);
        }
        Ok(Some(Box::new(result)))
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for Interval {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let naxes = self.data.points[0].len();
        let (lbnd, ubnd) = self.effective_bounds();
        let closed = self.data.closed_for_base_inside();

        let mut result = vec![false; npoint];

        for (idx, inside) in result.iter_mut().enumerate() {
            let point = pts.point(idx);

            // BAD input coordinates mean the point is outside.
            if point_has_bad(&point[..naxes]) {
                continue;
            }

            // Check each axis constraint.
            let mut all_pass = true;
            for ax in 0..naxes {
                let p = point[ax];
                let lb = lbnd[ax];
                let ub = ubnd[ax];

                let pass = if closed {
                    // Closed: boundary points are included.
                    if lb <= ub {
                        // Normal included interval.
                        lb <= p && p <= ub
                    } else {
                        // Excluded interval (lb > ub): outside [ub, lb].
                        p <= ub || lb <= p
                    }
                } else {
                    // Open: boundary points are excluded.
                    if lb <= ub {
                        lb < p && p < ub
                    } else {
                        p < ub || lb < p
                    }
                };

                if !pass {
                    all_pass = false;
                    break;
                }
            }

            *inside = all_pass;
        }

        Ok(result)
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        // Return the bounds directly. For unbounded directions, use the
        // effective bounds (±MAX).
        let (lbnd, ubnd) = self.effective_bounds();

        // For excluded intervals (lb > ub), the bounding box is
        // the full range since the region wraps around.
        let naxes = lbnd.len();
        let mut lower = lbnd;
        let mut upper = ubnd;

        for i in 0..naxes {
            if lower[i] > upper[i] {
                // Excluded interval — the region covers from -MAX to ub
                // and from lb to MAX, so the bbox is the whole axis.
                lower[i] = -f64::MAX;
                upper[i] = f64::MAX;
            }
        }

        Ok((lower, upper))
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let naxes = self.data.points[0].len();
        let (lbnd, ubnd) = self.effective_bounds();

        // For unbounded intervals, we cannot produce a meaningful mesh.
        // Only generate corner points for bounded axes. If any axis is
        // unbounded (|bound| == MAX), skip it in corner generation.
        // Check if interval is fully bounded.
        if !self.is_bounded_inner() {
            // Unbounded intervals have no finite boundary mesh.
            return Ok(Vec::new());
        }

        // Generate corner points of the box: 2^naxes corners.
        let ncorners = 1_usize << naxes;
        let mut mesh = Vec::with_capacity(ncorners);

        for corner_idx in 0..ncorners {
            let mut point = Vec::with_capacity(naxes);
            for ax in 0..naxes {
                if (corner_idx >> ax) & 1 == 0 {
                    point.push(lbnd[ax]);
                } else {
                    point.push(ubnd[ax]);
                }
            }
            mesh.push(point);
        }

        Ok(mesh)
    }

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        let naxes = self.data.points[0].len();

        // Only return a centre if the interval is bounded (equivalent to a Box).
        if !self.is_bounded_inner() {
            return Err(AstError::new(
                ErrorCode::Regcn,
                "Interval is unbounded and has no centre",
            ));
        }

        let (lbnd, ubnd) = self.effective_bounds();
        let mut centre = Vec::with_capacity(naxes);
        for i in 0..naxes {
            centre.push(0.5 * (lbnd[i] + ubnd[i]));
        }

        Ok(centre)
    }

    fn is_bounded_inner(&self) -> bool {
        // Bounded only if ALL axes have BOTH finite bounds, and none are
        // excluded intervals (lb > ub makes it effectively unbounded
        // in the C implementation's sense — but actually a closed excluded
        // interval IS bounded for purposes of this check when it's equivalent
        // to a Box). Following C: bounded iff equivalent to a Box, i.e. all
        // bounds are finite (not BAD/±MAX) and all axis intervals have the
        // same orientation.
        let naxes = self.data.points[0].len();

        if naxes == 0 {
            return false;
        }

        let (lbnd, ubnd) = self.effective_bounds();

        // Check all bounds are finite.
        for i in 0..naxes {
            if lbnd[i].abs() == f64::MAX || ubnd[i].abs() == f64::MAX {
                return false;
            }
        }

        // Check consistency: all axes must have the same orientation.
        // (If some are included and some excluded, it's not equivalent to a Box.)
        let first_neg = lbnd[0] > ubnd[0];
        for i in 1..naxes {
            if (lbnd[i] > ubnd[i]) != first_neg {
                return false;
            }
        }

        true
    }

    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()> {
        // Transform the defining points (lower and upper bounds) through
        // the mapping.
        let naxes = self.data.points[0].len();
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, forward)?;

        let mut new_points = Vec::with_capacity(self.data.points.len());
        for i in 0..self.data.points.len() {
            let p = transformed.point(i);
            new_points.push(p[..naxes].to_vec());
        }
        self.data.points = new_points;
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region trait
// ============================================================================

impl Region for Interval {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        // Check for BAD coordinates in the input.
        if point_has_bad(point) {
            return Ok(false);
        }

        // Transform point from current frame to base frame.
        let naxes = self.data.frameset.base().naxes();
        let pts = PointSet::from_rows(std::iter::once(point));
        let base_pts = self.data.frameset.transform(&pts, false)?;
        let base_point = base_pts.point(0);

        // Check the transformed point for BAD values.
        if base_point[..naxes].iter().any(|&v| is_bad(v)) {
            return Ok(false);
        }

        // Test raw geometry.
        let inside = self.reg_base_inside_point(&base_point[..naxes])?;

        // Apply negation.
        if self.data.negated {
            Ok(!inside)
        } else {
            Ok(inside)
        }
    }

    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap> {
        // Interval overrides astOverlap (interval.c:1765): the generic
        // implementation needs a finite boundary mesh from one of the two
        // Regions, which an unbounded Interval cannot supply. C falls back to
        // the inherited implementation when its own cannot decide
        // (interval.c:2179).
        if let Some(code) = self.interval_overlap(other)? {
            return Ok(super::overlap::code_to_overlap(code));
        }
        super::overlap::compute_overlap(self, other)
    }

    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        // Faithful port of interval.c::RegPins (2580). If the Interval is
        // effectively an un-negated Box (all axes finite and consistently
        // oriented) delegate to the equivalent Box's on-boundary test
        // (interval.c:2682, via Cache interval.c:489). Otherwise build
        // Intervals widened and narrowed by the joint uncertainty band and
        // require every point to lie inside the larger and outside the
        // smaller.
        let nc = self.data.points[0].len();
        if pts.ncoord() != nc {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "reg_pins(Interval): PointSet has {} coordinates per point, expected {}",
                    pts.ncoord(),
                    nc
                ),
            ));
        }

        // Resolve the axis limits and, if the Interval is equivalent to a Box,
        // delegate to that Box's RegPins (interval.c:2682-2684).
        let (lbnd, ubnd, box_reg) = self.cache()?;
        if let Some(box_reg) = box_reg {
            return box_reg.reg_pins_mask(pts, unc);
        }

        // Not Box-equivalent: at least one infinite boundary. The border
        // half-width per axis is half the total width of the two uncertainty
        // bounding boxes (interval.c:2707-2757). `tunc` is this Interval's own
        // uncertainty in the base Frame; re-centring is omitted for the same
        // reason as the Box port (bound width is centre-invariant; the
        // reachable default uncertainty is already centred).
        let bfrm = self.data.frameset.base();
        let tunc = self.get_unc_frm(FrameRef::Base)?;
        let (lbnd_tunc, ubnd_tunc) = tunc.get_region_bounds()?;
        let wid: Vec<f64> = if let Some(unc) = unc {
            let (lbnd_unc, ubnd_unc) = unc.get_region_bounds()?;
            (0..nc)
                .map(|i| -> AstResult<f64> {
                    let dt = bfrm.ax_distance(i + 1, lbnd_tunc[i], ubnd_tunc[i])?;
                    let du = bfrm.ax_distance(i + 1, lbnd_unc[i], ubnd_unc[i])?;
                    Ok(0.5 * (dt.abs() + du.abs()))
                })
                .collect::<AstResult<Vec<f64>>>()?
        } else {
            (0..nc)
                .map(|i| -> AstResult<f64> {
                    let dt = bfrm.ax_distance(i + 1, lbnd_tunc[i], ubnd_tunc[i])?;
                    Ok((0.5 * dt).abs())
                })
                .collect::<AstResult<Vec<f64>>>()?
        };

        // Build larger and smaller Intervals (interval.c:2762-2791). An
        // infinite side of "this" stays unconstrained (AST__BAD) in both the
        // larger and smaller Interval.
        let mut large_lbnd = vec![0.0f64; nc];
        let mut large_ubnd = vec![0.0f64; nc];
        let mut small_lbnd = vec![0.0f64; nc];
        let mut small_ubnd = vec![0.0f64; nc];
        for i in 0..nc {
            let mut lb = lbnd[i];
            let mut ub = ubnd[i];
            if lb > ub {
                std::mem::swap(&mut lb, &mut ub);
            }
            let w = wid[i].abs();
            if lb != -f64::MAX {
                large_lbnd[i] = lb - w;
                small_lbnd[i] = lb + w;
            } else {
                large_lbnd[i] = BAD_VALUE;
                small_lbnd[i] = BAD_VALUE;
            }
            if ub != f64::MAX {
                large_ubnd[i] = ub + w;
                small_ubnd[i] = ub - w;
            } else {
                large_ubnd[i] = BAD_VALUE;
                small_ubnd[i] = BAD_VALUE;
            }
            if small_lbnd[i] > small_ubnd[i] {
                small_lbnd[i] = small_ubnd[i];
            }
        }

        let make_interval = |lb: Vec<f64>, ub: Vec<f64>, negated: bool| -> Interval {
            let frameset = FrameSet::new(self.data.frameset.base().snapshot());
            let mut data = RegionData::new(frameset, vec![lb, ub]);
            data.negated = negated;
            Interval { data }
        };
        let large_int = make_interval(large_lbnd, large_ubnd, false);
        // Negate the smaller Interval (interval.c:2797).
        let small_int = make_interval(small_lbnd, small_ubnd, true);

        // Points are on the boundary iff inside both the large Interval and
        // the negated small Interval: chain the two forward transforms; a point
        // is on the boundary iff the result has no BAD (interval.c:2799-2855).
        let ps1 = super::data::region_transform(&large_int, pts, true)?;
        let ps2 = super::data::region_transform(&small_int, &ps1, true)?;
        let mask: Vec<bool> = (0..ps2.npoint())
            .map(|j| !ps2.point(j).iter().any(|&v| is_bad(v)))
            .collect();
        let result = mask.iter().all(|&b| b);
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
        self.is_bounded_inner()
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
        self.data.set_region_fs(frame);
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        super::region_bounds_current(self)
    }

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        // Only meaningful for bounded intervals.
        let centre = self.reg_centre()?;
        let (lbnd, ubnd) = self.effective_bounds();

        // Radius = half-diagonal of the bounding box.
        let naxes = centre.len();
        let mut sum_sq = 0.0;
        for i in 0..naxes {
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
            // Unbounded: return a single BAD point (like NullRegion).
            let naxes = self.data.frameset.base().naxes();
            let bad_point = vec![BAD_VALUE; naxes];
            return Ok(PointSet::from_rows(std::iter::once(bad_point.as_slice())));
        }

        let result = PointSet::from_rows(mesh.iter().map(|p| p.as_slice()));

        // Cache it.
        *self.data.cached_base_mesh.borrow_mut() = Some(mesh);

        Ok(result)
    }

    fn get_region_grid(&self) -> AstResult<PointSet> {
        crate::regions::data::region_base_grid(self)
    }

    fn get_region_points(&self) -> AstResult<PointSet> {
        Ok(PointSet::from_rows(
            self.data.points.iter().map(|p| p.as_slice()),
        ))
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
        // Port of interval.c::GetDefUnc (interval.c:583). If this Interval is
        // equivalent to a Box, delegate to that Box's default uncertainty
        // (interval.c:646-648).
        let (cache_lbnd, cache_ubnd, box_reg) = self.cache()?;
        if let Some(box_reg) = box_reg {
            return box_reg.get_def_unc();
        }

        // Otherwise build a Box covering 1.0E-6 of each axis interval using the
        // Interval's OWN cached limits (not the base-class bounding box). An
        // axis whose limits are equal uses 1.0E-6 of the axis value, and one
        // whose limits are both zero uses 0.5E-12, the absolute floor astEQUAL
        // applies near zero (interval.c:690). An axis with an infinite limit
        // collapses to a zero-width [0, 0] interval.
        let nax = cache_lbnd.len();
        let bfrm = self.data.frameset.base().snapshot();

        let mut lbnd = vec![0.0f64; nax];
        let mut ubnd = vec![0.0f64; nax];
        for i in 0..nax {
            let lb = cache_lbnd[i];
            let ub = cache_ubnd[i];
            if lb != -f64::MAX && ub != f64::MAX {
                let mut hw = (0.5e-6 * (ub - lb)).abs();
                let c = 0.5 * (ub + lb);
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

        Ok(Box::new(super::BoxRegion::from_bounds_frame(
            bfrm, &lbnd, &ubnd,
        )?))
    }

    fn reg_centre_query(&self, ifrm: FrameRef) -> AstResult<Option<Vec<f64>>> {
        // An Interval can only be re-centred (or report a centre) when it is
        // effectively a closed Box (interval.c:2534-2567). Otherwise there is
        // no centre and the query returns None without error.
        if !self.is_bounded_inner() {
            return Ok(None);
        }
        let (lbnd, ubnd) = self.effective_bounds();
        let centre: Vec<f64> = lbnd.iter().zip(&ubnd).map(|(l, u)| 0.5 * (l + u)).collect();
        match ifrm {
            FrameRef::Current => Ok(Some(self.reg_tran_point(&centre, true)?)),
            _ => Ok(Some(centre)),
        }
    }

    fn reg_centre_set(&mut self, cen: &[f64], ifrm: FrameRef) -> AstResult<()> {
        // Only re-centreable when effectively a closed Box (interval.c:2569).
        if !self.is_bounded_inner() {
            return Err(AstError::new(
                ErrorCode::Regcn,
                "The supplied Interval is not a closed Interval and so cannot be re-centred"
                    .to_string(),
            ));
        }

        // Delegate to the equivalent Box (interval.c:2535-2557): build a Box
        // over the current bounds that shares this Interval's FrameSet, set
        // its centre, then read back the new base-Frame bounds.
        let (lbnd, ubnd) = self.effective_bounds();
        let mut box_data = RegionData::new(self.data.frameset.clone(), {
            let centre: Vec<f64> = lbnd.iter().zip(&ubnd).map(|(l, u)| 0.5 * (l + u)).collect();
            vec![centre, ubnd.clone()]
        });
        box_data.closed = true;
        box_data.closed_set = true;
        let mut box_reg = super::BoxRegion { data: box_data };

        box_reg.reg_centre_set(cen, ifrm)?;

        // Recover the new base-Frame bounds from the re-centred Box and store
        // them back in the Interval's defining points.
        let (nlbnd, nubnd) = box_reg.reg_base_box()?;
        let nax = self.data.points[0].len();
        self.data.points[0][..nax].copy_from_slice(&nlbnd[..nax]);
        self.data.points[1][..nax].copy_from_slice(&nubnd[..nax]);
        self.data.invalidate_cache();
        Ok(())
    }

    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        // Port of interval.c::RegBasePick (interval.c:2328): build an Interval
        // spanned by the selected base-Frame axes by slicing the lbnd/ubnd
        // defining points to those axes.
        let bfrm = self.data.frameset.base();
        let target: Vec<i32> = axes.iter().map(|&a| a as i32).collect();
        let frm = bfrm.pick_axes_snapshot(&target)?;

        let lbnd: Vec<f64> = axes.iter().map(|&a| self.data.points[0][a]).collect();
        let ubnd: Vec<f64> = axes.iter().map(|&a| self.data.points[1][a]).collect();

        // Select the same axes from the base-Frame uncertainty (if any); drop
        // it if picking does not yield a Region (interval.c:2401-2410).
        let unc = match &self.data.uncertainty {
            Some(u) => match super::pick_axes_region(u.as_ref(), axes)? {
                super::PickedAxes::Region(r) => Some(r),
                super::PickedAxes::Frame(_) => None,
            },
            None => None,
        };

        let frameset = FrameSet::new(frm);
        let mut data = RegionData::new(frameset, vec![lbnd, ubnd]);
        data.uncertainty = unc;
        Ok(Some(Box::new(Interval { data })))
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    fn region_merge(&self, other_reg: &dyn Region, intfirst: bool) -> Option<DynMapping> {
        // Faithful port of `interval.c::MergeInterval` (1224). `intfirst`
        // selects the axis order: when true the Interval's axes go first and
        // `other`'s second; when false `other`'s axes go first.
        let other = other_reg.as_any();

        // Guard shared by every branch (interval.c:1320): the two Regions
        // must have the same Closed attribute, and either both or neither
        // must have a defined uncertainty.
        if self.is_closed() != other_reg.is_closed()
            || self.data.uncertainty.is_some() != other_reg.get_uncertainty().is_some()
        {
            return None;
        }

        let neg_this = self.data.negated;
        let nax_this = self.naxes();
        let nax_reg = other_reg.naxes();

        // Build the merged Region's defining points in the base Frame,
        // dispatching on the concrete type of `other`. Each branch mirrors
        // the corresponding block of `MergeInterval`. `concat_bounds` places
        // the two axis-value slices in the order given by `intfirst`.
        let concat = |this_lo: &[f64], reg_lo: &[f64]| -> Vec<f64> {
            let mut v = Vec::with_capacity(nax_this + nax_reg);
            if intfirst {
                v.extend_from_slice(this_lo);
                v.extend_from_slice(reg_lo);
            } else {
                v.extend_from_slice(reg_lo);
                v.extend_from_slice(this_lo);
            }
            v
        };

        let built: Option<MergedRegion> = if let Some(other) = other.downcast_ref::<Interval>() {
            // Interval + Interval → Interval (interval.c:1354). Both
            // un-negated.
            if neg_this || other.data.negated {
                None
            } else {
                let lbnd = concat(&self.data.points[0], &other.data.points[0]);
                let ubnd = concat(&self.data.points[1], &other.data.points[1]);
                Some(MergedRegion::Interval(lbnd, ubnd))
            }
        } else if let Some(other) = other.downcast_ref::<super::BoxRegion>() {
            // Interval + Box → Interval (interval.c:1380). Both un-negated.
            if neg_this || other.is_negated() {
                None
            } else {
                let (blb, bub) = other.reg_base_box().ok()?;
                let lbnd = concat(&self.data.points[0], &blb);
                let ubnd = concat(&self.data.points[1], &bub);
                Some(MergedRegion::Interval(lbnd, ubnd))
            }
        } else if let Some(other) = other.downcast_ref::<super::NullRegion>() {
            // Interval + NullRegion → Interval (interval.c:1426). The
            // NullRegion must be negated and the Interval must not.
            if neg_this || !other.is_negated() {
                None
            } else {
                let bad = vec![BAD_VALUE; nax_reg];
                let lbnd = concat(&self.data.points[0], &bad);
                let ubnd = concat(&self.data.points[1], &bad);
                Some(MergedRegion::Interval(lbnd, ubnd))
            }
        } else if let Some(other) = other.downcast_ref::<super::PointList>() {
            // Interval + PointList → PointList (interval.c:1458). Both
            // un-negated.
            if neg_this || other.is_negated() {
                None
            } else {
                // Attaching one constant per Interval axis to every point is
                // only equivalent when the Interval is a point, so its width
                // on each axis must be under a quarter of its uncertainty
                // there -- the same test MergeBox applies to a Box's
                // half-width (interval.c:1481-1499). A missing limit is stored
                // as -DBL_MAX or DBL_MAX, so an unbounded axis fails the test
                // and the Interval is left alone.
                let (lbnd, ubnd) = self.reg_base_box().ok()?;
                let bunc = self.get_unc_frm(FrameRef::Base).ok()?;
                let (lbnd_unc, ubnd_unc) = bunc.get_region_bounds().ok()?;

                let mut axval = Vec::with_capacity(nax_this);
                for i in 0..nax_this {
                    if 0.5 * (ubnd[i] - lbnd[i]).abs() > 0.25 * (ubnd_unc[i] - lbnd_unc[i]).abs() {
                        return None;
                    }
                    axval.push(0.5 * (lbnd[i] + ubnd[i]));
                }

                let points = other
                    .data
                    .points
                    .iter()
                    .map(|p| concat(&axval, p))
                    .collect();
                Some(MergedRegion::PointList(points))
            }
        } else {
            None
        };

        let built = built?;

        // The merged Region is built on a CmpFrame of the two *base* Frames and
        // mapped into the combined current Frame at the end (interval.c:1335-1343).
        let bfrm_this = self.data.frameset.base().snapshot();
        let bfrm_reg = other_reg.get_region_frameset().base().snapshot();
        let bfrm: DynFrame = if intfirst {
            Box::new(crate::frames::CmpFrame::new(bfrm_this, bfrm_reg))
        } else {
            Box::new(crate::frames::CmpFrame::new(bfrm_reg, bfrm_this))
        };

        // The two current Frames, combined in the same order (interval.c:1573-1584).
        let f_this = self.data.frameset.current().snapshot();
        let f_reg = other_reg.get_region_frame();
        let cfrm: DynFrame = if intfirst {
            Box::new(crate::frames::CmpFrame::new(f_this, f_reg))
        } else {
            Box::new(crate::frames::CmpFrame::new(f_reg, f_this))
        };

        // The two base->current Mappings, in parallel (interval.c:1586-1598).
        let map_this = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)
            .ok()?;
        let map_reg = other_reg
            .get_region_frameset()
            .mapping(FrameRef::Base, FrameRef::Current)
            .ok()?;
        let bcmap: DynMapping = if intfirst {
            Box::new(
                crate::mappings::CmpMap::new(
                    map_this,
                    map_reg,
                    crate::mappings::Composition::Parallel,
                )
                .ok()?,
            )
        } else {
            Box::new(
                crate::mappings::CmpMap::new(
                    map_reg,
                    map_this,
                    crate::mappings::Composition::Parallel,
                )
                .ok()?,
            )
        };

        // Combine uncertainties in the requested order when both are present.
        let unc = super::merge_util::combine_uncertainties(
            self.data.uncertainty.clone(),
            other_reg.get_uncertainty(),
            intfirst,
        );

        let scaling = super::merge_util::MergeScaling {
            msz_this: (
                self.data.effective_mesh_size(),
                self.data.mesh_size_set,
                nax_this,
            ),
            msz_reg: (other_reg.mesh_size(), other_reg.test_mesh_size(), nax_reg),
            fill_this: (self.data.effective_fill_factor(), self.data.fill_factor_set),
            fill_reg: (other_reg.fill_factor(), other_reg.test_fill_factor()),
        };

        finish_region_merge(
            built,
            super::merge_util::MergeFrames { bfrm, cfrm, bcmap },
            super::merge_util::MergeOverlay {
                closed: self.data.closed,
                closed_set: self.data.closed_set,
                adaptive: self.data.adaptive,
                negated: false,
            },
            unc,
            scaling,
        )
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for Interval {
    region_frame_attrs!();

    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    /// Mirrors C `Region::MatchAxesX` (region.c:6413): the axes of the
    /// current Frame of the Region's FrameSet are matched.
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        self.data.frameset.current().match_axes_x(frm1)
    }

    /// Mirrors C `Region::Match` (region.c:6214): the current Frame of the
    /// Region's FrameSet is matched, through `astMatch` and so with its
    /// cast retry.
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

    /// A Region's axes are its encapsulated current Frame's axes. Mirrors
    /// C `Region::AxDistance` / `AxOffset` / `AxNorm` (region.c:1457, 1610,
    /// 1536), which all forward to that Frame.
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.data.frameset.current().get_axis(axis)
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
        if let Some(other_int) = other.as_any().downcast_ref::<Interval>() {
            self.data
                .frameset
                .current()
                .equal(other_int.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for Interval {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.data.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.data.mapping_flags
    }

    /// region.c `ReportPoints`: the current Frame formats the values.
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
        // If this Interval is equivalent to a Box, simplify that Box and
        // return the result (interval.c:3196-3201). The equivalent Box carries
        // the Interval's FrameSet, so its own Simplify is what re-expresses
        // the bounds in the current Frame; nothing is transferred back onto an
        // Interval and C applies no `astRegOverlay` on this branch.
        if let (_, _, Some(box_reg)) = self.cache()? {
            let simplified = (box_reg.as_ref() as &dyn Mapping).simplify()?;
            return Ok((simplified, true));
        }

        // Not equivalent to a Box: invoke the parent Region simplify
        // (interval.c:3208), which simplifies the encapsulated FrameSet and any
        // explicit uncertainty. All further work is done on the returned copy.
        let parent = self.parent_region_simplify()?;
        let new_owned;
        let this: &Interval = match &parent {
            Some(p) => {
                new_owned = p
                    .as_ref()
                    .as_any()
                    .downcast_ref::<Interval>()
                    .expect("parent_region_simplify preserves the Region class")
                    .clone();
                &new_owned
            }
            None => self,
        };
        let simpler = parent.is_some();

        let map = this
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let simple_map = map.simplify()?;
        if simple_map.as_ref().as_any().is::<UnitMap>() {
            // C's `else` at interval.c:3516 — `result = astClone( this )`.
            if !simpler {
                return Ok((self.clone_box(), false));
            }
            let mut result = this.clone_region();
            result.reg_overlay(self, true);
            return Ok((result.clone_box(), true));
        }

        // Non-UnitMap base->current: attempt to re-express the Interval in
        // its current Frame. Port of interval.c::Simplify (3216-3517).
        let (mut result, changed) = match this.simplify_remapped(simple_map.as_ref())? {
            // C builds the current-Frame Interval and sets `simpler = 1`
            // (interval.c:3481-3483).
            Some(simplified) => (
                super::merge_util::mapping_into_region(simplified)
                    .expect("Interval::simplify_remapped returns a Region"),
                true,
            ),
            // The remap produced nothing simpler, so `simpler` is whatever the
            // parent simplify left it (interval.c:3510).
            None => (this.clone_region(), simpler),
        };
        if changed {
            result.reg_overlay(self, true);
            Ok((result.clone_box(), true))
        } else {
            Ok((self.clone_box(), false))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Interval>() else {
            return false;
        };
        self.data.negated == other.data.negated
            && self.data.closed == other.data.closed
            && self.data.points == other.data.points
            && Frame::equal(self.data.frameset.current(), other.data.frameset.current())
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        // Parallel merge only (series of two Regions is not a Region). The
        // direct pairwise path always has `self` as the lower-index (left)
        // component, so the Interval axes go first (`intfirst = 1`). The
        // nominate loop reaches the lower-neighbour (`intfirst = 0`) case via
        // `Region::region_merge`, not through `try_merge`.
        if series || self_inv || other_inv {
            return None;
        }
        let other_reg: &dyn Region = super::as_region(other)?;
        Region::region_merge(self, other_reg, true)
    }

    fn map_merge(
        &self,
        ctx: &mut crate::mappings::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        crate::mappings::simplify::region_map_merge(self, ctx, where_, series)
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

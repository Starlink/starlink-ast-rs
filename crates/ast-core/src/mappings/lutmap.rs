//! 1-D lookup table mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, NodeValue, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::winmap::WinMap;
use super::{DynMapping, Mapping, ViaSimplify};
use crate::mappings::SimplifiedRecord;

/// Interpolation method for [`LutMap`] table lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LutInterp {
    /// Linear interpolation (default). Extrapolates at boundaries.
    Linear,
    /// Nearest-neighbour. Returns [`BAD_VALUE`] outside bounds.
    Nearest,
}

/// A 1-dimensional coordinate transformation via lookup table interpolation.
///
/// The forward transform maps an input coordinate `x` to an output
/// coordinate by interpolating in a regularly-spaced table of values.
/// The table entry at index `i` corresponds to input coordinate
/// `start + i * inc`.
///
/// Always has `nin = nout = 1`.
#[derive(Debug, Clone)]
pub struct LutMap {
    /// Table values.
    lut: Vec<f64>,
    /// Input coordinate at first entry.
    start: f64,
    /// Input spacing between entries.
    inc: f64,
    /// Interpolation method.
    interp: LutInterp,
    /// Monotonicity: +1 increasing, -1 decreasing, 0 non-monotonic.
    monotonic: i8,
    /// Whether the mapping is inverted.
    inverted: bool,
    /// The stated relative error of the table values — C's `LutEpsilon`
    /// attribute, written as the `LutEps` card (`lutmap.c:2012`), defaulting to
    /// `DBL_EPSILON`.
    ///
    /// It scales the linearity tolerance in [`Self::table_is_linear`], so it
    /// decides whether a nearly-linear table is replaced by a WinMap.
    lut_epsilon: f64,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    simplified: SimplifiedRecord,
    /// Whether the inverse transform is defined.
    tran_inverse: bool,
}

impl LutMap {
    /// Create a new `LutMap` from a lookup table.
    ///
    /// `lut` is the table of output values, `start` is the input coordinate
    /// at the first entry, and `inc` is the spacing between entries.
    ///
    /// Returns an error if `lut` is empty or `inc` is zero.
    pub fn new(lut: &[f64], start: f64, inc: f64) -> AstResult<Self> {
        if lut.is_empty() {
            return Err(AstError::new(
                ErrorCode::Badin,
                "LutMap lookup table must not be empty",
            ));
        }
        if inc == 0.0 {
            return Err(AstError::new(
                ErrorCode::Badin,
                "LutMap increment must not be zero",
            ));
        }
        let monotonic = compute_monotonicity(lut);
        Ok(Self {
            lut: lut.to_vec(),
            start,
            inc,
            interp: LutInterp::Linear,
            monotonic,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            tran_inverse: monotonic != 0,
            lut_epsilon: f64::EPSILON,
        })
    }

    /// Set the interpolation method.
    pub fn set_interp(&mut self, interp: LutInterp) {
        // Changing this feeds this class's own simplification, so both
        // orientations' simplified records are discarded (C
        // `astClearIsSimple`, lutmap.c:1855).
        self.simplified.clear();
        self.interp = interp;
    }

    /// Return the interpolation method.
    pub fn interp(&self) -> LutInterp {
        self.interp
    }

    /// Return the number of entries in the lookup table.
    pub fn nlut(&self) -> usize {
        self.lut.len()
    }

    /// Return the input coordinate at the first entry.
    /// The equivalent `WinMap` when the lookup table is linear, applied in the
    /// given direction — C `lutmap.c:1142-1174`.
    ///
    /// `None` when the table is not linear, is shorter than two entries, or has
    /// equal first and last values: C guards on `b1 != b2` (`:1155`), because a
    /// constant table would give a WinMap with a zero scale.
    ///
    /// The corner-point construction is C's, rather than computing shift and
    /// scale directly and inverting, so the arithmetic matches and the result
    /// is always a *forward* WinMap.
    /// Whether the lookup table is linear, as C's `GetLinear`
    /// (`lutmap.c:598-627`) decides it.
    ///
    /// The tolerance is scaled by the `LutEpsilon` attribute — the stated
    /// relative error of the table values, `f64::EPSILON` by default — and taken
    /// as the larger of two estimates: one from the table's full range and one
    /// from the interpolated value at the element under test, the second so that
    /// values near zero do not get an unusably tight bound. Linearity is
    /// accepted within twice that.
    fn table_is_linear(&self) -> bool {
        let n = self.lut.len();
        if n < 2 {
            return false;
        }
        let (mut lo, mut hi) = (self.lut[0], self.lut[0]);
        for &v in &self.lut {
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
        // A constant table is linear by inspection, and the loop below would
        // divide the tolerance down to nothing (lutmap.c:595).
        if hi == lo {
            return true;
        }
        let eps = self.lut_epsilon;
        let tol1 = (hi - lo).abs() * eps;
        let last = n - 1;
        for ilut in 1..last {
            let fract = ilut as f64 / last as f64;
            let interp = self.lut[0] * (1.0 - fract) + self.lut[last] * fract;
            let tol = tol1.max(interp.abs() * eps);
            if (self.lut[ilut] - interp).abs() > 2.0 * tol {
                return false;
            }
        }
        true
    }

    pub(crate) fn linear_equivalent(&self, inverted: bool) -> Option<WinMap> {
        let n = self.lut.len();
        if n < 2 {
            return None;
        }
        if !self.table_is_linear() || self.lut[0] == self.lut[n - 1] {
            return None;
        }

        let a1 = self.start;
        let a2 = self.start + self.inc * (n - 1) as f64;
        let b1 = self.lut[0];
        let b2 = self.lut[n - 1];
        if !inverted {
            WinMap::from_corners(&[a1], &[a2], &[b1], &[b2]).ok()
        } else {
            WinMap::from_corners(&[b1], &[b2], &[a1], &[a2]).ok()
        }
    }

    /// Whether this LutMap, applied in direction `eff_inv`, is the same Mapping
    /// as `other` in its own stored direction. `equal` compares stored
    /// orientation, so the applied one has to be folded in first.
    fn applied_equal(&self, other: &LutMap, eff_inv: bool) -> bool {
        let mut probe = self.clone();
        if probe.inverted != eff_inv {
            probe.invert();
        }
        probe.equal(other)
    }

    /// Return the input value corresponding to the first table entry.
    pub fn start(&self) -> f64 {
        self.start
    }

    /// Return the input spacing between entries.
    pub fn inc(&self) -> f64 {
        self.inc
    }

    /// Return the monotonicity indicator.
    ///
    /// +1 if the table is monotonically increasing, -1 if monotonically
    /// decreasing, 0 if non-monotonic.
    pub fn monotonic(&self) -> i8 {
        self.monotonic
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    /// Forward transform for a single input value.
    fn forward_one(&self, x: f64) -> f64 {
        if is_bad(x) {
            return BAD_VALUE;
        }
        let nlut = self.lut.len();
        let idx = (x - self.start) / self.inc;

        match self.interp {
            LutInterp::Linear => {
                if nlut == 1 {
                    // Single-element table: can only return that value at
                    // the exact index, extrapolation has zero slope.
                    return self.lut[0];
                }
                if idx >= 0.0 && idx <= (nlut - 1) as f64 {
                    // Within bounds: linear interpolation
                    let lo = idx.floor() as usize;
                    let hi = lo + 1;
                    if hi >= nlut {
                        // Exactly at the last entry
                        let v = self.lut[lo];
                        if is_bad(v) { BAD_VALUE } else { v }
                    } else {
                        let v_lo = self.lut[lo];
                        let v_hi = self.lut[hi];
                        if is_bad(v_lo) || is_bad(v_hi) {
                            BAD_VALUE
                        } else {
                            let frac = idx - lo as f64;
                            v_lo + frac * (v_hi - v_lo)
                        }
                    }
                } else if idx < 0.0 {
                    // Below bounds: extrapolate using slope at start
                    let v0 = self.lut[0];
                    let v1 = self.lut[1];
                    if is_bad(v0) || is_bad(v1) {
                        BAD_VALUE
                    } else {
                        let slope = v1 - v0;
                        v0 + idx * slope
                    }
                } else {
                    // Above bounds: extrapolate using slope at end
                    let v_last = self.lut[nlut - 1];
                    let v_prev = self.lut[nlut - 2];
                    if is_bad(v_last) || is_bad(v_prev) {
                        BAD_VALUE
                    } else {
                        let slope = v_last - v_prev;
                        v_last + (idx - (nlut - 1) as f64) * slope
                    }
                }
            }
            LutInterp::Nearest => {
                let rounded = idx.round() as i64;
                if rounded < 0 || rounded >= nlut as i64 {
                    BAD_VALUE
                } else {
                    let v = self.lut[rounded as usize];
                    if is_bad(v) { BAD_VALUE } else { v }
                }
            }
        }
    }

    /// Inverse transform for a single output value (binary search on table).
    fn inverse_one(&self, y: f64) -> f64 {
        if is_bad(y) {
            return BAD_VALUE;
        }
        if self.monotonic == 0 {
            return BAD_VALUE;
        }
        let nlut = self.lut.len();
        if nlut == 1 {
            // Single element: inverse only defined if y matches
            let v = self.lut[0];
            if is_bad(v) {
                return BAD_VALUE;
            }
            if (y - v).abs() < f64::EPSILON * v.abs().max(1.0) {
                return self.start;
            }
            return BAD_VALUE;
        }

        let increasing = self.monotonic > 0;

        // Find the bracketing entries via binary search.
        // We search for the interval [lut[lo], lut[lo+1]] that contains y.
        let mut low: usize = 0;
        let mut high: usize = nlut - 1;

        // Check bounds first — skip BAD entries at boundaries
        let first_good = self.first_good_value();
        let last_good = self.last_good_value();
        if first_good.is_none() || last_good.is_none() {
            return BAD_VALUE;
        }
        let (first_idx, first_val) = first_good.unwrap();
        let (last_idx, last_val) = last_good.unwrap();

        let (lo_val, hi_val) = if increasing {
            (first_val, last_val)
        } else {
            (last_val, first_val)
        };

        if y < lo_val || y > hi_val {
            // Outside the range of the table — extrapolate using boundary slope
            return self.inverse_extrapolate(y, increasing, first_idx, last_idx);
        }

        // Binary search for bracketing interval
        while high - low > 1 {
            let mid = (low + high) / 2;
            let v_mid = self.lut[mid];
            if is_bad(v_mid) {
                // Skip BAD entries — search linearly from mid
                // Try to find a non-BAD entry near mid
                let found = self.find_non_bad_near(mid, low, high);
                match found {
                    Some(good_mid) => {
                        let v_good = self.lut[good_mid];
                        if (increasing && y >= v_good) || (!increasing && y <= v_good) {
                            low = good_mid;
                        } else {
                            high = good_mid;
                        }
                    }
                    None => return BAD_VALUE,
                }
                continue;
            }
            if (increasing && y >= v_mid) || (!increasing && y <= v_mid) {
                low = mid;
            } else {
                high = mid;
            }
        }

        // Now interpolate between lut[low] and lut[high]
        let v_lo = self.lut[low];
        let v_hi = self.lut[high];
        if is_bad(v_lo) || is_bad(v_hi) {
            return BAD_VALUE;
        }

        // Check for flat sections: if the target value equals one of the
        // bracket values and a neighbour also has that value, the inverse
        // is undetermined. Matches C lutmap.c:1669-1679.
        if v_lo == y {
            if (low > 0 && !is_bad(self.lut[low - 1]) && self.lut[low - 1] == y) || v_hi == y {
                return BAD_VALUE;
            }
        } else if v_hi == y
            && high < self.lut.len() - 1
            && !is_bad(self.lut[high + 1])
            && self.lut[high + 1] == y
        {
            return BAD_VALUE;
        }

        let dv = v_hi - v_lo;
        if dv == 0.0 {
            // Flat section — inverse is undetermined, return BAD
            return BAD_VALUE;
        }
        let frac = (y - v_lo) / dv;
        let idx = low as f64 + frac * (high - low) as f64;
        self.start + idx * self.inc
    }

    /// Extrapolate the inverse beyond the table bounds.
    fn inverse_extrapolate(
        &self,
        y: f64,
        increasing: bool,
        first_idx: usize,
        last_idx: usize,
    ) -> f64 {
        let nlut = self.lut.len();
        // Determine which end we need
        let (lo_val, lo_idx, hi_val, hi_idx) = if increasing {
            (self.lut[first_idx], first_idx, self.lut[last_idx], last_idx)
        } else {
            (self.lut[last_idx], last_idx, self.lut[first_idx], first_idx)
        };

        if y < lo_val {
            // Below the low end
            // Use the slope at the low end of the input range
            let (slope_idx, slope_idx_next) = if increasing {
                // Low end is at the start
                self.find_slope_pair_forward(first_idx, nlut)
            } else {
                // Low end is at the end (decreasing)
                self.find_slope_pair_backward(last_idx)
            };
            match (slope_idx, slope_idx_next) {
                (Some(a), Some(b)) => {
                    let va = self.lut[a];
                    let vb = self.lut[b];
                    let dv = vb - va;
                    if dv == 0.0 {
                        return BAD_VALUE;
                    }
                    let didx = b as f64 - a as f64;
                    let slope_input_per_output = didx / dv;
                    let ref_input = self.start + lo_idx as f64 * self.inc;
                    ref_input + (y - lo_val) * slope_input_per_output * self.inc
                }
                _ => BAD_VALUE,
            }
        } else {
            // Above the high end
            let (slope_idx, slope_idx_next) = if increasing {
                // High end is at the end
                self.find_slope_pair_backward(last_idx)
            } else {
                // High end is at the start (decreasing)
                self.find_slope_pair_forward(first_idx, nlut)
            };
            match (slope_idx, slope_idx_next) {
                (Some(a), Some(b)) => {
                    let va = self.lut[a];
                    let vb = self.lut[b];
                    let dv = vb - va;
                    if dv == 0.0 {
                        return BAD_VALUE;
                    }
                    let didx = b as f64 - a as f64;
                    let slope_input_per_output = didx / dv;
                    let ref_input = self.start + hi_idx as f64 * self.inc;
                    ref_input + (y - hi_val) * slope_input_per_output * self.inc
                }
                _ => BAD_VALUE,
            }
        }
    }

    /// Find the first non-BAD value in the table. Returns (index, value).
    fn first_good_value(&self) -> Option<(usize, f64)> {
        for (i, &v) in self.lut.iter().enumerate() {
            if !is_bad(v) {
                return Some((i, v));
            }
        }
        None
    }

    /// Find the last non-BAD value in the table. Returns (index, value).
    fn last_good_value(&self) -> Option<(usize, f64)> {
        for (i, &v) in self.lut.iter().enumerate().rev() {
            if !is_bad(v) {
                return Some((i, v));
            }
        }
        None
    }

    /// Find a non-BAD entry near `mid` within [low, high].
    fn find_non_bad_near(&self, mid: usize, low: usize, high: usize) -> Option<usize> {
        // Search outward from mid
        let mut lo = mid;
        let mut hi = mid;
        loop {
            if lo > low {
                lo -= 1;
                if !is_bad(self.lut[lo]) {
                    return Some(lo);
                }
            }
            if hi < high {
                hi += 1;
                if !is_bad(self.lut[hi]) {
                    return Some(hi);
                }
            }
            if lo <= low && hi >= high {
                return None;
            }
        }
    }

    /// Find a pair of consecutive good entries going forward from start.
    fn find_slope_pair_forward(&self, start: usize, end: usize) -> (Option<usize>, Option<usize>) {
        let mut first = None;
        for i in start..end {
            if !is_bad(self.lut[i]) {
                if let Some(f) = first {
                    return (Some(f), Some(i));
                }
                first = Some(i);
            }
        }
        (first, None)
    }

    /// Find a pair of consecutive good entries going backward ending at `end`.
    fn find_slope_pair_backward(&self, end: usize) -> (Option<usize>, Option<usize>) {
        let mut last = None;
        for i in (0..=end).rev() {
            if !is_bad(self.lut[i]) {
                if let Some(l) = last {
                    return (Some(i), Some(l));
                }
                last = Some(i);
            }
        }
        (None, last)
    }

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), next, Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), other, Composition::Parallel)
    }
}

/// Compute monotonicity of a lookup table, ignoring BAD entries.
///
/// Returns +1 if strictly increasing, -1 if strictly decreasing,
/// 0 if non-monotonic (or all BAD, or fewer than 2 good entries).
fn compute_monotonicity(lut: &[f64]) -> i8 {
    let mut direction: i8 = 0; // 0 = undetermined
    let mut prev: Option<f64> = None;

    for &v in lut {
        if is_bad(v) {
            continue;
        }
        if let Some(p) = prev {
            if v > p {
                if direction == -1 {
                    return 0; // was decreasing, now increasing
                }
                direction = 1;
            } else if v < p {
                if direction == 1 {
                    return 0; // was increasing, now decreasing
                }
                direction = -1;
            }
            // equal values are ignored (flat sections)
        }
        prev = Some(v);
    }
    direction
}

impl Mapping for LutMap {
    fn nin(&self) -> usize {
        1
    }

    fn nout(&self) -> usize {
        1
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), 1);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    /// NaN-as-bad fast path. LutMap's lookup logic has many internal
    /// `BAD_VALUE` emit sites (binary search, BAD-skip logic, extrapolation
    /// boundary handling, etc.) and an `idx.round() as i64` cast in the
    /// Nearest interp path that cannot accept NaN (NaN-as-i64 has unspecified
    /// behaviour pre-Rust 1.45 and saturates to 0 / i64::MAX after, neither
    /// of which gives a useful table entry). So we keep the existing logic
    /// intact and post-process: any `BAD_VALUE` in the output becomes
    /// `f64::NAN` so downstream NaN-aware stages see the protocol-canonical
    /// sentinel. Cost is one O(N) sweep over the output (LutMap is 1D, so
    /// that's just N comparisons).
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        self.transform_into(pts, forward, out)?;
        let mut view = out.view_mut();
        let flat = view.as_slice_mut().expect("PointSet is C-contiguous");
        for v in flat.iter_mut() {
            if *v == BAD_VALUE {
                *v = f64::NAN;
            }
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        for i in 0..pts.npoint() {
            let x = pts.point(i)[0];
            let result = if fwd {
                self.forward_one(x)
            } else {
                self.inverse_one(x)
            };
            out.point_mut(i)[0] = result;
        }
        Ok(())
    }

    fn simplified(&self, _gate: ViaSimplify) -> AstResult<DynMapping> {
        if self.has_been_simplified() {
            return Ok(Box::new(self.clone()));
        }
        // A lone LutMap reaches C's replacement through the base `Simplify`,
        // which decomposes to a one-element list and hands it to `MapMerge`
        // (mapping.c:17016). Same conversion, same guard — see
        // [`LutMap::linear_equivalent`].
        Ok(match self.linear_equivalent(self.inverted) {
            Some(wm) => Box::new(wm),
            None => Box::new(self.clone()),
        })
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        let eff_inv = ctx.entry(where_).1;

        // C `lutmap.c:1142-1174`: a linear LutMap is replaced by an equivalent
        // WinMap, stored forward with its invert flag cleared.
        if let Some(wm) = self.linear_equivalent(eff_inv) {
            ctx.set_pair_single(where_, (Box::new(wm), false, false));
            return Some(where_);
        }

        // C `lutmap.c:1177-1236`: in series, a LutMap adjacent to an equal
        // LutMap applied in the opposite direction collapses to a UnitMap. C
        // looks at the higher neighbour first, then the lower, and replaces the
        // *lower* of the two.
        if series {
            let neighbour =
                if where_ + 1 < ctx.len() && ctx.entry(where_ + 1).0.as_any().is::<LutMap>() {
                    Some((where_ + 1, where_))
                } else if where_ > 0 && ctx.entry(where_ - 1).0.as_any().is::<LutMap>() {
                    Some((where_ - 1, where_ - 1))
                } else {
                    None
                };

            if let Some((ineb, ilo)) = neighbour {
                let (neb_map, neb_inv, _) = ctx.entry(ineb);
                let neb = neb_map
                    .as_any()
                    .downcast_ref::<LutMap>()
                    .expect("checked immediately above");
                // C sets the neighbour's Invert to its list value and then
                // inverts it before comparing, so the two are equal-but-opposite
                // exactly when the applied directions differ and the tables
                // agree. Owned values make that a comparison, not a mutation.
                let mut probe = neb.clone();
                if probe.inverted != *neb_inv {
                    probe.invert();
                }
                probe.invert();
                if self.applied_equal(&probe, eff_inv) {
                    ctx.replace_pair_with(ilo, Box::new(UnitMap::new(1)), false);
                    return Some(where_.min(ilo));
                }
            }
        }

        super::simplify::default_map_merge(ctx, where_, series)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<LutMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        ast_equal(self.start, other.start)
            && ast_equal(self.inc, other.inc)
            && self.interp == other.interp
            && self.lut.len() == other.lut.len()
            && self
                .lut
                .iter()
                .zip(&other.lut)
                .all(|(a, b)| ast_equal(*a, *b))
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }
        // Only cancel with inverse: same LUT, opposite direction.
        if self_inv == other_inv {
            return None;
        }
        let other_lut = other.as_any().downcast_ref::<LutMap>()?;
        // Must be same LUT data, start, inc, and interpolation method.
        if self.lut.len() != other_lut.lut.len() {
            return None;
        }
        if !ast_equal(self.start, other_lut.start) || !ast_equal(self.inc, other_lut.inc) {
            return None;
        }
        if self.interp != other_lut.interp {
            return None;
        }
        if !self
            .lut
            .iter()
            .zip(&other_lut.lut)
            .all(|(a, b)| ast_equal(*a, *b))
        {
            return None;
        }
        // LutMap maps 1 coordinate to 1 coordinate.
        Some(Box::new(UnitMap::new(1)))
    }

    fn has_forward(&self) -> bool {
        // Intrinsically the forward transform is always defined and the
        // inverse only when the table is monotonic (lutmap.c:2391). Report
        // the *effective* direction so a parent CmpMap computes its own
        // definedness exactly as C's invert-aware `astGetTranForward`
        // (mapping.c:1823) does when the leaf is applied inverted.
        if self.inverted {
            self.tran_inverse
        } else {
            true
        }
    }

    fn has_inverse(&self) -> bool {
        if self.inverted {
            true
        } else {
            self.tran_inverse
        }
    }

    fn dump_tran_flags(&self) -> (bool, bool) {
        // C passes the monotonicity-derived inverse availability to
        // astInitMapping (lutmap.c) and dumps the raw base-struct fields,
        // which are unaffected by the Invert flag.
        (true, self.tran_inverse)
    }

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl Serializable for LutMap {
    fn class_name(&self) -> &'static str {
        "LutMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Map 1-d coordinates using a lookup table".to_string());
        write_mapping_base(&mut node, self);

        node.push_int(
            "Nlut",
            self.lut.len() as i64,
            Some("Number of lookup table elements".to_string()),
        );

        // Match the C library's Dump (`lutmap.c`): both Start (helpful=1,
        // default 0.0) and Incr (helpful=1, default 1.0) are always emitted —
        // commented at default, uncommented when explicit.
        if self.start != 0.0 {
            node.push_double(
                "Start",
                self.start,
                Some("Input value at first element".to_string()),
            );
        } else {
            node.push_unset(
                "Start",
                NodeValue::Int(0),
                Some("Input value at first element".to_string()),
            );
        }
        if self.inc != 1.0 {
            node.push_double(
                "Incr",
                self.inc,
                Some("Input value increment between elements".to_string()),
            );
        } else {
            node.push_unset(
                "Incr",
                NodeValue::Int(1),
                Some("Input value increment between elements".to_string()),
            );
        }
        let interp_val = match self.interp {
            LutInterp::Linear => 0,
            LutInterp::Nearest => 1,
        };
        node.push_int(
            "LutInt",
            interp_val,
            Some("Interpolation method".to_string()),
        );

        for (i, &v) in self.lut.iter().enumerate() {
            if !is_bad(v) {
                let comment = if i == 0 {
                    Some("Lookup table elements...".to_string())
                } else {
                    None
                };
                node.push_double(&format!("L{}", i + 1), v, comment);
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);
        let nlut = node.get_int("Nlut", 0) as usize;
        let start = node.get_double("Start", 0.0);
        let inc = node.get_double("Incr", 1.0);
        let interp_val = node.get_int("LutInt", 0);
        let interp = match interp_val {
            1 => LutInterp::Nearest,
            _ => LutInterp::Linear,
        };

        let mut lut = vec![BAD_VALUE; nlut];
        for (i, entry) in lut.iter_mut().enumerate() {
            *entry = node.get_double(&format!("L{}", i + 1), BAD_VALUE);
        }

        let monotonic = compute_monotonicity(&lut);
        let mut m = LutMap {
            lut,
            start,
            inc,
            interp,
            monotonic,
            lut_epsilon: node.get_double("LutEps", f64::EPSILON),
            inverted: false,
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            tran_inverse: base.has_inverse,
        };
        if base.inverted {
            m.invert();
        }
        Ok(m)
    }
}

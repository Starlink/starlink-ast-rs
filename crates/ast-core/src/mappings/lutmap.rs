//! 1-D lookup table mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::is_bad;
use super::rate::numeric_rate;
use super::{DynMapping, Mapping};

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
        })
    }

    /// Set the interpolation method.
    pub fn set_interp(&mut self, interp: LutInterp) {
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
        let dv = v_hi - v_lo;
        if dv == 0.0 {
            // Flat section — return midpoint
            let frac_idx = (low as f64 + high as f64) / 2.0;
            return self.start + frac_idx * self.inc;
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

    fn simplified(&self) -> AstResult<DynMapping> {
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
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

        if self.start != 0.0 {
            node.push_double(
                "Start",
                self.start,
                Some("Input value for first table entry".to_string()),
            );
        }
        if self.inc != 1.0 {
            node.push_double(
                "Incr",
                self.inc,
                Some("Input increment between table entries".to_string()),
            );
        }
        if self.interp != LutInterp::Linear {
            let interp_val = match self.interp {
                LutInterp::Linear => 0,
                LutInterp::Nearest => 1,
            };
            node.push_int(
                "LutInt",
                interp_val,
                Some("Interpolation method".to_string()),
            );
        }

        for (i, &v) in self.lut.iter().enumerate() {
            if !is_bad(v) {
                node.push_double(
                    &format!("L{}", i + 1),
                    v,
                    Some("Lookup table elements".to_string()),
                );
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (_nin, _nout, inverted) = read_mapping_base(node);
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
            inverted: false,
        };
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}

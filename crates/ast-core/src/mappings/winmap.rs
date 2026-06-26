//! Per-axis linear (shift + scale) mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::matrix::{MatrixMap, is_diagonal};
use super::shift::ShiftMap;
use super::unit::UnitMap;
use super::unitnormmap::try_merge_into_unitnormmap;
use super::zoom::ZoomMap;
use super::{DynMapping, Mapping};

/// A mapping that applies a per-axis linear transformation:
/// `output[i] = shift[i] + scale[i] * input[i]`.
///
/// This is equivalent to mapping one "window" (bounding box) on to another,
/// with independent linear scaling along each axis. It generalises
/// [`ShiftMap`](super::ShiftMap) (which has unit scales) and
/// [`ZoomMap`](super::ZoomMap) (which has a single uniform scale and no shift).
#[derive(Debug, Clone)]
pub struct WinMap {
    shifts: Vec<f64>,
    scales: Vec<f64>,
    inverted: bool,
    has_been_simplified: bool,
}

impl WinMap {
    /// Access the shift array.
    pub(crate) fn shifts(&self) -> &[f64] {
        &self.shifts
    }

    /// Access the scale array.
    pub(crate) fn scales(&self) -> &[f64] {
        &self.scales
    }

    /// Create a new `WinMap` from explicit per-axis shifts and scales.
    ///
    /// `shifts` and `scales` must have the same length (the number of axes)
    /// and must not be empty.
    ///
    /// The forward transformation is `output[i] = shifts[i] + scales[i] * input[i]`.
    pub fn new(shifts: &[f64], scales: &[f64]) -> AstResult<Self> {
        if shifts.len() != scales.len() {
            return Err(AstError::new(
                ErrorCode::Badwm,
                format!(
                    "shifts length ({}) must equal scales length ({})",
                    shifts.len(),
                    scales.len()
                ),
            ));
        }
        if shifts.is_empty() {
            return Err(AstError::new(
                ErrorCode::Badwm,
                "WinMap must have at least one axis",
            ));
        }
        Ok(Self {
            shifts: shifts.to_vec(),
            scales: scales.to_vec(),
            inverted: false,
            has_been_simplified: false,
        })
    }

    /// Create a `WinMap` from input and output corner coordinates.
    ///
    /// The mapping linearly transforms the box defined by `corner1_in` /
    /// `corner2_in` to the box defined by `corner1_out` / `corner2_out`.
    ///
    /// All four slices must have the same length (the number of axes).
    /// If the input range on any axis is zero or any coordinate is
    /// [`BAD_VALUE`], that axis gets [`BAD_VALUE`] for both shift and scale.
    pub fn from_corners(
        corner1_in: &[f64],
        corner2_in: &[f64],
        corner1_out: &[f64],
        corner2_out: &[f64],
    ) -> AstResult<Self> {
        let n = corner1_in.len();
        if corner2_in.len() != n || corner1_out.len() != n || corner2_out.len() != n {
            return Err(AstError::new(
                ErrorCode::Badwm,
                "all corner arrays must have the same length",
            ));
        }
        if n == 0 {
            return Err(AstError::new(
                ErrorCode::Badwm,
                "WinMap must have at least one axis",
            ));
        }

        let mut shifts = vec![0.0; n];
        let mut scales = vec![1.0; n];
        for i in 0..n {
            let c1i = corner1_in[i];
            let c2i = corner2_in[i];
            let c1o = corner1_out[i];
            let c2o = corner2_out[i];
            if is_bad(c1i) || is_bad(c2i) || is_bad(c1o) || is_bad(c2o) {
                shifts[i] = BAD_VALUE;
                scales[i] = BAD_VALUE;
            } else {
                let denom = c2i - c1i;
                if denom == 0.0 {
                    shifts[i] = BAD_VALUE;
                    scales[i] = BAD_VALUE;
                } else {
                    let s = (c2o - c1o) / denom;
                    scales[i] = s;
                    shifts[i] = c1o - s * c1i;
                }
            }
        }
        Ok(Self {
            shifts,
            scales,
            inverted: false,
            has_been_simplified: false,
        })
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
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

impl Mapping for WinMap {
    fn nin(&self) -> usize {
        self.shifts.len()
    }
    fn nout(&self) -> usize {
        self.shifts.len()
    }
    fn is_inverted(&self) -> bool {
        self.inverted
    }
    fn invert(&mut self) {
        self.inverted = !self.inverted;
        // Inversion invalidates the "already simplified" flag, since the
        // inverse may simplify further (C astSetInvert clears IsSimple).
        self.has_been_simplified = false;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let n = self.shifts.len();
        let mut out = PointSet::zeros(pts.npoint(), n);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if fwd {
                // forward: output = shift + scale * input
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    let shift = self.shifts[j];
                    let scale = self.scales[j];
                    if is_bad(v) || is_bad(shift) || is_bad(scale) {
                        *o = BAD_VALUE;
                    } else {
                        *o = shift + scale * v;
                    }
                }
            } else {
                // inverse: output = (input - shift) / scale
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    let shift = self.shifts[j];
                    let scale = self.scales[j];
                    if is_bad(v) || is_bad(shift) || is_bad(scale) || scale == 0.0 {
                        *o = BAD_VALUE;
                    } else {
                        *o = (v - shift) / scale;
                    }
                }
            }
        }
        Ok(())
    }

    /// NaN-as-bad fast path. Skips the per-point `is_bad` check on the input
    /// (the caller has substituted `BAD_VALUE → NaN` at the pipeline boundary
    /// — see [`dyn Mapping::transform_nan_protocol`]) and lets NaN propagate
    /// through the linear arithmetic naturally.
    ///
    /// The static checks for BAD shifts/scales (an attribute-level invariant,
    /// not a per-point condition) still apply: we precompute whether any
    /// channel is structurally invalid and short-circuit to NaN once, rather
    /// than per-point. For LSST's WinMap (4 finite shifts and scales) this
    /// branch is never taken.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let n = self.shifts.len();

        // Per-channel "is this channel statically broken?" mask. Computed
        // once; used to splat NaN into output lanes that map from a broken
        // channel (e.g. BAD shift, zero scale on inverse).
        let bad_channel: Vec<bool> = self
            .shifts
            .iter()
            .zip(self.scales.iter())
            .map(|(&shift, &scale)| is_bad(shift) || is_bad(scale) || (!fwd && scale == 0.0))
            .collect();
        debug_assert_eq!(bad_channel.len(), n);

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if fwd {
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    *o = if bad_channel[j] {
                        f64::NAN
                    } else {
                        self.shifts[j] + self.scales[j] * v
                    };
                }
            } else {
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    *o = if bad_channel[j] {
                        f64::NAN
                    } else {
                        (v - self.shifts[j]) / self.scales[j]
                    };
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.has_been_simplified {
            return Ok(Box::new(self.clone()));
        }

        let n = self.shifts.len();
        let all_shifts_zero = self.shifts.iter().all(|&s| ast_equal(s, 0.0));
        let all_scales_one = self.scales.iter().all(|&s| ast_equal(s, 1.0));

        // Identity: out = 0 + 1*x = x
        if all_shifts_zero && all_scales_one {
            return Ok(Box::new(UnitMap::new(n)));
        }
        // Pure shift: out = shift + 1*x = shift + x
        if all_scales_one {
            let shifts: Vec<f64> = if self.inverted {
                self.shifts.iter().map(|&s| -s).collect()
            } else {
                self.shifts.clone()
            };
            return Ok(Box::new(ShiftMap::new(shifts)));
        }
        // Pure uniform zoom: out = 0 + f*x = f*x (same factor on all axes)
        if all_shifts_zero {
            let first = self.scales[0];
            if (1..n).all(|i| ast_equal(self.scales[i], first)) {
                let f = if self.inverted { 1.0 / first } else { first };
                if let Ok(zm) = ZoomMap::new(n, f) {
                    return Ok(Box::new(zm));
                }
            }
            // Non-uniform scales with zero shifts → diagonal MatrixMap
            let scales: Vec<f64> = if self.inverted {
                self.scales.iter().map(|&s| 1.0 / s).collect()
            } else {
                self.scales.clone()
            };
            let mut diag = ndarray::Array2::<f64>::zeros((n, n));
            for i in 0..n {
                diag[[i, i]] = scales[i];
            }
            if let Ok(mm) = MatrixMap::new(diag) {
                return Ok(Box::new(mm));
            }
        }
        Ok(Box::new(self.clone()))
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<WinMap>() else {
            return false;
        };
        if self.shifts.len() != other.shifts.len() {
            return false;
        }
        // Compare effective parameters (resolving inversion).
        let (s1, c1) = winmap_effective_params(&self.shifts, &self.scales, self.inverted);
        let (s2, c2) = winmap_effective_params(&other.shifts, &other.scales, other.inverted);
        s1.iter().zip(&s2).all(|(a, b)| ast_equal(*a, *b))
            && c1.iter().zip(&c2).all(|(a, b)| ast_equal(*a, *b))
    }

    fn map_split_zero_based(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors winmap.c::MapSplit (~1864). WinMap is per-axis affine:
        // y_i = shifts[i] + scales[i] * x_i, so any subset of inputs maps
        // to the same-indexed outputs via a smaller WinMap built from the
        // chosen axes' (shift, scale) pairs.
        //
        // C uses astWinTerms(this, 0, &a, &b) to fetch the *effective*
        // forward (shift, scale) (which already folds the Invert flag in)
        // and stores them in a fresh WinMap with Invert=0. The equivalent
        // here is to copy the raw (shift, scale) and propagate the
        // is_inverted flag onto the sub-mapping; the resulting forward
        // transform is identical, and this keeps the construction symmetric
        // with the ShiftMap / ZoomMap per-class impls.
        //
        // Project convention: reject duplicate or out-of-range axes (Task
        // 1.1 set this precedent for UnitMap / ZoomMap / ShiftMap; the C
        // function only checks the in-range half — duplicates would yield
        // a degenerate but technically valid sub-mapping in C).
        let nin = self.nin();
        crate::mappings::helpers::validate_in_axes(in_axes, nin)?;
        let shifts: Vec<f64> = in_axes.iter().map(|&ix| self.shifts()[ix]).collect();
        let scales: Vec<f64> = in_axes.iter().map(|&ix| self.scales()[ix]).collect();
        let mut sub = WinMap::new(&shifts, &scales).ok()?;
        if self.is_inverted() {
            sub.invert();
        }
        Some((in_axes.to_vec(), Box::new(sub)))
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            // Parallel: concatenate axis terms from both mappings.
            let (s1, c1) = winmap_effective_params(&self.shifts, &self.scales, self_inv);
            let (s2, c2) = extract_linear_params(other, other_inv)?;
            let mut new_shifts = s1;
            new_shifts.extend(s2);
            let mut new_scales = c1;
            new_scales.extend(c2);
            return WinMap::new(&new_shifts, &new_scales)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }

        // Check for equal and opposite WinMaps (C winmap.c WinWin). When one
        // map is applied inverted and the other forward, and their stored
        // shift/scale terms are equal, the composition is the identity. This
        // is done explicitly on the *raw* (uninverted) terms to avoid the
        // rounding errors introduced by inverting one map's terms — the same
        // reason C compares `wm1->a/b` against `wm2->a/b` directly.
        if self_inv != other_inv {
            if let Some(ow) = other.as_any().downcast_ref::<WinMap>() {
                if self.shifts.len() == ow.shifts.len()
                    && self
                        .shifts
                        .iter()
                        .zip(&ow.shifts)
                        .all(|(a, b)| ast_equal(*a, *b))
                    && self
                        .scales
                        .iter()
                        .zip(&ow.scales)
                        .all(|(a, b)| ast_equal(*a, *b))
                {
                    return Some(Box::new(UnitMap::new(self.shifts.len())));
                }
            }
        }

        // WinMap (unit scale) + UnitNormMap(forward) → UnitNormMap(forward).
        if let Some(unm) = other
            .as_any()
            .downcast_ref::<super::unitnormmap::UnitNormMap>()
        {
            return try_merge_into_unitnormmap(self, self_inv, unm, other_inv);
        }

        // Extract effective parameters for self (applied first).
        let (s1, c1) = winmap_effective_params(&self.shifts, &self.scales, self_inv);
        // Extract effective parameters for other (applied second).
        // Accepts WinMap, ZoomMap, or ShiftMap.
        let (s2, c2) = extract_linear_params(other, other_inv)?;

        if s1.len() != s2.len() {
            return None;
        }

        compose_linear(&s1, &c1, &s2, &c2)
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        super::winmap_merge::win_map_merge(self, ctx, where_, series)
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        // For a diagonal linear mapping, the rate is known analytically.
        if ax1 == ax2 {
            let idx = ax1 - 1;
            let scale = self.scales[idx];
            if is_bad(scale) {
                return Ok(BAD_VALUE);
            }
            if self.inverted {
                if scale == 0.0 {
                    return Ok(BAD_VALUE);
                }
                return Ok(1.0 / scale);
            }
            return Ok(scale);
        }
        // Off-diagonal: axes are independent, so the rate is exactly 0.
        Ok(0.0)
    }

    fn has_been_simplified(&self) -> bool {
        self.has_been_simplified
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.has_been_simplified = flag;
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

/// Compute effective (shifts, scales) for a WinMap, accounting for inversion.
///
/// Forward: `output = shift + scale * input`.
/// Inverse: `output = (input - shift) / scale = -shift/scale + (1/scale) * input`.
fn winmap_effective_params(shifts: &[f64], scales: &[f64], inv: bool) -> (Vec<f64>, Vec<f64>) {
    if !inv {
        (shifts.to_vec(), scales.to_vec())
    } else {
        let n = shifts.len();
        let mut inv_shifts = Vec::with_capacity(n);
        let mut inv_scales = Vec::with_capacity(n);
        for i in 0..n {
            if is_bad(scales[i]) || is_bad(shifts[i]) || scales[i] == 0.0 {
                inv_shifts.push(BAD_VALUE);
                inv_scales.push(BAD_VALUE);
            } else {
                inv_scales.push(1.0 / scales[i]);
                inv_shifts.push(-shifts[i] / scales[i]);
            }
        }
        (inv_shifts, inv_scales)
    }
}

/// Extract effective `(shifts, scales)` from a linear mapping,
/// accounting for inversion.
///
/// Returns `None` if the mapping is not one of `WinMap`, `ZoomMap`,
/// `ShiftMap`, or a diagonal `MatrixMap`.
pub(crate) fn extract_linear_params(
    mapping: &dyn Mapping,
    inv: bool,
) -> Option<(Vec<f64>, Vec<f64>)> {
    if let Some(wm) = mapping.as_any().downcast_ref::<WinMap>() {
        return Some(winmap_effective_params(&wm.shifts, &wm.scales, inv));
    }
    if let Some(zm) = mapping.as_any().downcast_ref::<ZoomMap>() {
        let n = zm.nin();
        let f = if inv { 1.0 / zm.factor() } else { zm.factor() };
        return Some((vec![0.0; n], vec![f; n]));
    }
    if let Some(sm) = mapping.as_any().downcast_ref::<ShiftMap>() {
        let shifts: Vec<f64> = sm
            .shifts()
            .iter()
            .map(|&s| if inv { -s } else { s })
            .collect();
        return Some((shifts, vec![1.0; sm.nin()]));
    }
    if let Some(mm) = mapping.as_any().downcast_ref::<MatrixMap>() {
        let m = if inv {
            mm.inverse_matrix()?
        } else {
            mm.forward_matrix()
        };
        if !is_diagonal(m) {
            return None;
        }
        let n = m.nrows();
        let shifts = vec![0.0; n];
        let scales: Vec<f64> = (0..n).map(|i| m[[i, i]]).collect();
        return Some((shifts, scales));
    }
    if let Some(um) = mapping.as_any().downcast_ref::<UnitMap>() {
        let n = um.nin();
        return Some((vec![0.0; n], vec![1.0; n]));
    }
    None
}

/// Compose two linear mappings given their per-axis `(shifts, scales)`.
///
/// The composition is: `out = s2 + c2 * (s1 + c1 * x)`.
/// This is the result of applying mapping 1 (`s1`, `c1`) first, then
/// mapping 2 (`s2`, `c2`) second.
///
/// Returns a `UnitMap` if the result is the identity, otherwise a
/// `WinMap`.
pub(crate) fn compose_linear(s1: &[f64], c1: &[f64], s2: &[f64], c2: &[f64]) -> Option<DynMapping> {
    let n = s1.len();
    let mut new_shifts = vec![0.0; n];
    let mut new_scales = vec![0.0; n];
    for i in 0..n {
        new_scales[i] = c2[i] * c1[i];
        new_shifts[i] = c2[i] * s1[i] + s2[i];

        // Rounding checks for values close to a unit mapping, matching C
        // winmap.c WinWin: snap a near-zero shift (relative to the geometric
        // mean of the two input shifts) to 0, and a near-unity scale to 1.
        let amean = (s1[i] * s2[i]).abs().sqrt();
        if new_shifts[i].abs() < amean * 1e-15 {
            new_shifts[i] = 0.0;
        }
        if (new_scales[i] - 1.0).abs() < 1e-15 {
            new_scales[i] = 1.0;
        }
    }

    // Check if the result is the identity.
    if new_scales.iter().all(|&s| ast_equal(s, 1.0))
        && new_shifts.iter().all(|&s| ast_equal(s, 0.0))
    {
        return Some(Box::new(UnitMap::new(n)));
    }

    // Pure shift: all scales are 1.
    if new_scales.iter().all(|&s| ast_equal(s, 1.0)) {
        return Some(Box::new(ShiftMap::new(new_shifts)));
    }

    // Pure scale (all shifts are 0): zoom or diagonal matrix.
    if new_shifts.iter().all(|&s| ast_equal(s, 0.0)) && n > 0 {
        let first = new_scales[0];
        if (1..n).all(|i| ast_equal(new_scales[i], first)) {
            if let Ok(zm) = ZoomMap::new(n, first) {
                return Some(Box::new(zm));
            }
        }
        // Non-uniform scales → diagonal MatrixMap.
        let mut diag = ndarray::Array2::<f64>::zeros((n, n));
        for i in 0..n {
            diag[[i, i]] = new_scales[i];
        }
        if let Ok(mm) = MatrixMap::new(diag) {
            return Some(Box::new(mm));
        }
    }

    WinMap::new(&new_shifts, &new_scales)
        .ok()
        .map(|m| Box::new(m) as DynMapping)
}

impl Serializable for WinMap {
    fn class_name(&self) -> &'static str {
        "WinMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Map one window on to another".to_string());
        write_mapping_base(&mut node, self);
        // C library writes Sft/Scl interleaved per axis: Sft1, Scl1, Sft2, Scl2, ...
        for i in 0..self.shifts.len() {
            if self.shifts[i] != 0.0 {
                node.push_double(
                    &format!("Sft{}", i + 1),
                    self.shifts[i],
                    Some(format!("Shift for axis {}", i + 1)),
                );
            }
            if self.scales[i] != 1.0 {
                node.push_double(
                    &format!("Scl{}", i + 1),
                    self.scales[i],
                    Some(format!("Scale factor for axis {}", i + 1)),
                );
            }
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);
        let mut shifts = Vec::with_capacity(base.nin);
        let mut scales = Vec::with_capacity(base.nin);
        for i in 0..base.nin {
            shifts.push(node.get_double(&format!("Sft{}", i + 1), 0.0));
            scales.push(node.get_double(&format!("Scl{}", i + 1), 1.0));
        }
        let mut m = WinMap {
            shifts,
            scales,
            inverted: false,
            has_been_simplified: base.has_been_simplified,
        };
        if base.inverted {
            m.invert();
        }
        Ok(m)
    }
}

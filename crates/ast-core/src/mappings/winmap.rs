//! Per-axis linear (shift + scale) mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad, point_has_bad};
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
}

impl WinMap {
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
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else if fwd {
                // forward: output = shift + scale * input
                for (j, (o, &v)) in outp.iter_mut().zip(inp.iter()).enumerate() {
                    let shift = self.shifts[j];
                    let scale = self.scales[j];
                    if is_bad(shift) || is_bad(scale) {
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
                    if is_bad(shift) || is_bad(scale) || scale == 0.0 {
                        *o = BAD_VALUE;
                    } else {
                        *o = (v - shift) / scale;
                    }
                }
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        Ok(Box::new(self.clone()))
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<WinMap>() else {
            return false;
        };
        if self.shifts.len() != other.shifts.len() {
            return false;
        }
        if self.inverted == other.inverted {
            self.shifts
                .iter()
                .zip(&other.shifts)
                .all(|(a, b)| ast_equal(*a, *b))
                && self
                    .scales
                    .iter()
                    .zip(&other.scales)
                    .all(|(a, b)| ast_equal(*a, *b))
        } else {
            // WinMap inversion changes both shift and scale non-trivially.
            false
        }
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
}

impl Serializable for WinMap {
    fn class_name(&self) -> &'static str {
        "WinMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Map one window on to another".to_string());
        write_mapping_base(&mut node, self);
        for (i, &s) in self.shifts.iter().enumerate() {
            if s != 0.0 {
                node.push_double(
                    &format!("Sft{}", i + 1),
                    s,
                    Some(format!("Shift for axis {}", i + 1)),
                );
            }
        }
        for (i, &s) in self.scales.iter().enumerate() {
            if s != 1.0 {
                node.push_double(
                    &format!("Scl{}", i + 1),
                    s,
                    Some(format!("Scale factor for axis {}", i + 1)),
                );
            }
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (nin, _nout, inverted) = read_mapping_base(node);
        let mut shifts = Vec::with_capacity(nin);
        let mut scales = Vec::with_capacity(nin);
        for i in 0..nin {
            shifts.push(node.get_double(&format!("Sft{}", i + 1), 0.0));
            scales.push(node.get_double(&format!("Scl{}", i + 1), 1.0));
        }
        let mut m = WinMap {
            shifts,
            scales,
            inverted: false,
        };
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}

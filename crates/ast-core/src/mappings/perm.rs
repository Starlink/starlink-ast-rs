//! Axis permutation/selection mapping.
//!
//! `PermMap` selects, reorders, or assigns constant values to axes.  The
//! permutation encoding uses 1-based indices that match the C AST convention:
//!
//! - Positive `j`: the output gets the value of input axis `j` (1-based).
//! - Negative `-j`: the output gets `constants[j-1]` (a constant value).
//! - Zero: the output gets `BAD_VALUE`.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};

use super::helpers::point_has_bad;
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

/// Axis permutation and constant-injection mapping.
///
/// # Permutation encoding (1-based, matching C AST)
///
/// Each element of `inperm` and `outperm` is interpreted as follows:
///
/// | Value | Meaning |
/// |-------|---------|
/// | `+j`  | Take the value from input axis `j` (1-based). |
/// | `-j`  | Use `constants[j-1]` (a stored constant). |
/// | `0`   | Set the output to `BAD_VALUE`. |
///
/// `outperm` drives the **forward** transform (maps `nin` inputs → `nout`
/// outputs).  `inperm` drives the **inverse** transform (maps `nout` inputs
/// back to `nin` outputs).
#[derive(Debug, Clone)]
pub struct PermMap {
    /// Forward permutation: length = nout.
    outperm: Vec<i64>,
    /// Inverse permutation: length = nin.
    inperm: Vec<i64>,
    /// Constant values referenced by negative perm entries.
    constants: Vec<f64>,
    /// Whether the mapping is currently inverted.
    inverted: bool,
}

impl PermMap {
    /// Create a new `PermMap`.
    ///
    /// - `inperm`: length = nin; drives the inverse transform.
    /// - `outperm`: length = nout; drives the forward transform.
    /// - `constants`: values referenced by negative perm entries.
    ///
    /// Returns an error if any positive perm entry refers to an axis that
    /// does not exist, or if any negative entry refers to a constant that
    /// is not in `constants`.
    pub fn new(inperm: &[i64], outperm: &[i64], constants: &[f64]) -> AstResult<Self> {
        let nin = inperm.len();
        let nout = outperm.len();

        // Validate outperm (forward: maps inputs [1..nin] to outputs [1..nout]).
        for &p in outperm {
            if p > 0 && p as usize > nin {
                return Err(AstError::new(
                    ErrorCode::Prmin,
                    format!(
                        "outperm entry {p} refers to input axis {p} but nin = {nin}"
                    ),
                ));
            }
            if p < 0 {
                let cidx = (-p - 1) as usize;
                if cidx >= constants.len() {
                    return Err(AstError::new(
                        ErrorCode::Prmin,
                        format!(
                            "outperm entry {p} refers to constants[{}] but only {} constants supplied",
                            cidx, constants.len()
                        ),
                    ));
                }
            }
        }

        // Validate inperm (inverse: maps outputs [1..nout] back to inputs
        // [1..nin]).
        for &p in inperm {
            if p > 0 && p as usize > nout {
                return Err(AstError::new(
                    ErrorCode::Prmin,
                    format!(
                        "inperm entry {p} refers to output axis {p} but nout = {nout}"
                    ),
                ));
            }
            if p < 0 {
                let cidx = (-p - 1) as usize;
                if cidx >= constants.len() {
                    return Err(AstError::new(
                        ErrorCode::Prmin,
                        format!(
                            "inperm entry {p} refers to constants[{}] but only {} constants supplied",
                            cidx, constants.len()
                        ),
                    ));
                }
            }
        }

        Ok(Self {
            outperm: outperm.to_vec(),
            inperm: inperm.to_vec(),
            constants: constants.to_vec(),
            inverted: false,
        })
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    /// Apply a permutation vector to the input `pts`, writing results into
    /// `out`.
    ///
    /// `perm` encodes the selection (1-based / negative / zero convention).
    /// `nin_for_perm` is the number of input coordinates expected by `perm`
    /// (used for BAD-value checks).
    fn apply_perm(
        perm: &[i64],
        constants: &[f64],
        pts: &PointSet,
        out: &mut PointSet,
    ) {
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            if point_has_bad(inp) {
                outp.fill(BAD_VALUE);
            } else {
                for (k, &p) in perm.iter().enumerate() {
                    outp[k] = if p > 0 {
                        inp[(p as usize) - 1]
                    } else if p < 0 {
                        constants[(-p as usize) - 1]
                    } else {
                        BAD_VALUE
                    };
                }
            }
        }
    }
}

impl Mapping for PermMap {
    fn nin(&self) -> usize {
        if self.inverted { self.outperm.len() } else { self.inperm.len() }
    }

    fn nout(&self) -> usize {
        if self.inverted { self.inperm.len() } else { self.outperm.len() }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        let (perm, nout) = if fwd {
            (self.outperm.as_slice(), self.outperm.len())
        } else {
            (self.inperm.as_slice(), self.inperm.len())
        };
        let mut out = PointSet::zeros(pts.npoint(), nout);
        Self::apply_perm(perm, &self.constants, pts, &mut out);
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let perm = if fwd {
            self.outperm.as_slice()
        } else {
            self.inperm.as_slice()
        };
        Self::apply_perm(perm, &self.constants, pts, out);
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        // Identity: outperm is exactly [1, 2, 3, ...].
        let is_identity = self.outperm.len() == self.inperm.len()
            && self
                .outperm
                .iter()
                .enumerate()
                .all(|(i, &p)| p == (i as i64 + 1));
        if is_identity {
            Ok(Box::new(UnitMap::new(self.outperm.len())))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}

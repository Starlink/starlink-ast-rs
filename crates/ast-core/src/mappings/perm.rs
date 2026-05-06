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
use crate::serial::{Node, NodeValue, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad, point_has_bad};
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
                    format!("outperm entry {p} refers to input axis {p} but nin = {nin}"),
                ));
            }
            if p < 0 {
                let cidx = (-p - 1) as usize;
                if cidx >= constants.len() {
                    return Err(AstError::new(
                        ErrorCode::Prmin,
                        format!(
                            "outperm entry {p} refers to constants[{}] but only {} constants supplied",
                            cidx,
                            constants.len()
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
                    format!("inperm entry {p} refers to output axis {p} but nout = {nout}"),
                ));
            }
            if p < 0 {
                let cidx = (-p - 1) as usize;
                if cidx >= constants.len() {
                    return Err(AstError::new(
                        ErrorCode::Prmin,
                        format!(
                            "inperm entry {p} refers to constants[{}] but only {} constants supplied",
                            cidx,
                            constants.len()
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
    fn apply_perm(perm: &[i64], constants: &[f64], pts: &PointSet, out: &mut PointSet) {
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

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), next, Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), other, Composition::Parallel)
    }
}

impl Mapping for PermMap {
    fn nin(&self) -> usize {
        if self.inverted {
            self.outperm.len()
        } else {
            self.inperm.len()
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            self.inperm.len()
        } else {
            self.outperm.len()
        }
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
        // Only handle inverse cancellation: same PermMap in opposite directions.
        if self_inv == other_inv {
            return None;
        }
        let other_perm = other.as_any().downcast_ref::<PermMap>()?;
        if self.outperm == other_perm.outperm
            && self.inperm == other_perm.inperm
            && self.constants.len() == other_perm.constants.len()
            && self
                .constants
                .iter()
                .zip(&other_perm.constants)
                .all(|(a, b)| ast_equal(*a, *b))
        {
            // Input dimension of the pair depends on which direction
            // self is being used in.
            let n = if self_inv {
                self.outperm.len()
            } else {
                self.inperm.len()
            };
            Some(Box::new(UnitMap::new(n)))
        } else {
            None
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<PermMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        self.outperm == other.outperm
            && self.inperm == other.inperm
            && self.constants.len() == other.constants.len()
            && self
                .constants
                .iter()
                .zip(&other.constants)
                .all(|(a, b)| ast_equal(*a, *b))
    }
}

impl Serializable for PermMap {
    fn class_name(&self) -> &'static str {
        "PermMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Coordinate permutation".to_string());
        write_mapping_base(&mut node, self);

        // Outperm (forward permutation).
        for (i, &p) in self.outperm.iter().enumerate() {
            node.push_int(
                &format!("Out{}", i + 1),
                p,
                Some("Output permutation".to_string()),
            );
        }

        // Inperm (inverse permutation).
        for (i, &p) in self.inperm.iter().enumerate() {
            node.push_int(
                &format!("In{}", i + 1),
                p,
                Some("Input permutation".to_string()),
            );
        }

        // Constants.
        if !self.constants.is_empty() {
            node.push_int(
                "Nconst",
                self.constants.len() as i64,
                Some("Number of constants".to_string()),
            );
            for (i, &c) in self.constants.iter().enumerate() {
                let key = format!("Con{}", i + 1);
                if is_bad(c) {
                    node.push_string(&key, "<bad>", Some("Constant value".to_string()));
                } else {
                    node.push_double(&key, c, Some("Constant value".to_string()));
                }
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (nin, nout, inverted) = read_mapping_base(node);

        // Check for OutCpy shorthand (identity forward permutation).
        let out_cpy = node.get_int("OutCpy", 0) != 0;
        let outperm = if out_cpy {
            // Identity: output i = input i (1-based).
            (1..=nout as i64).collect()
        } else {
            let mut v = Vec::with_capacity(nout);
            for i in 0..nout {
                v.push(node.get_int(&format!("Out{}", i + 1), 0));
            }
            v
        };

        // Check for InCpy shorthand (identity inverse permutation).
        let in_cpy = node.get_int("InCpy", 0) != 0;
        let inperm = if in_cpy {
            // Identity: input i = output i (1-based).
            (1..=nin as i64).collect()
        } else {
            let mut v = Vec::with_capacity(nin);
            for i in 0..nin {
                v.push(node.get_int(&format!("In{}", i + 1), 0));
            }
            v
        };

        let nconst = node.get_int("Nconst", 0) as usize;
        let mut constants = Vec::with_capacity(nconst);
        for i in 0..nconst {
            let key = format!("Con{}", i + 1);
            match node.get(&key) {
                Some(NodeValue::String(s)) if s == "<bad>" => constants.push(BAD_VALUE),
                _ => constants.push(node.get_double(&key, 0.0)),
            }
        }

        let mut m = PermMap::new(&inperm, &outperm, &constants)?;
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}

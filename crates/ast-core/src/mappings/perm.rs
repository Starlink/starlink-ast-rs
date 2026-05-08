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
use super::helpers::{ast_equal, is_bad};
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

    pub(crate) fn outperm(&self) -> &[i64] {
        &self.outperm
    }

    pub(crate) fn inperm(&self) -> &[i64] {
        &self.inperm
    }

    pub(crate) fn constants(&self) -> &[f64] {
        &self.constants
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
            for (k, &p) in perm.iter().enumerate() {
                outp[k] = if p > 0 {
                    let src = (p as usize) - 1;
                    if is_bad(inp[src]) {
                        BAD_VALUE
                    } else {
                        inp[src]
                    }
                } else if p < 0 {
                    constants[(-p as usize) - 1]
                } else {
                    BAD_VALUE
                };
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

/// Add a constant value to the pool, deduplicating via `ast_equal`.
/// Returns the 0-based index of the (possibly pre-existing) entry.
fn add_constant(constants: &mut Vec<f64>, val: f64) -> usize {
    for (i, &existing) in constants.iter().enumerate() {
        if ast_equal(existing, val) {
            return i;
        }
    }
    constants.push(val);
    constants.len() - 1
}

/// Compose two permutation arrays in series (first then second).
///
/// For each element `p2` in `second_perm`:
/// - If `p2 == 0`: result is 0 (BAD).
/// - If `p2 < 0`: result references a constant from `second_constants`.
/// - If `p2 > 0`: look up `first_perm[p2-1]`:
///   - 0 → BAD propagates.
///   - < 0 → result references a constant from `first_constants`.
///   - > 0 → pass through that input coordinate.
///
/// Returns the composed permutation and its own constant pool (indices
/// into which are encoded as negative values in the result perm).
fn compose_perms(
    first_perm: &[i64],
    second_perm: &[i64],
    first_constants: &[f64],
    second_constants: &[f64],
) -> (Vec<i64>, Vec<f64>) {
    let mut result_perm = Vec::with_capacity(second_perm.len());
    let mut result_constants: Vec<f64> = Vec::new();

    for &p2 in second_perm {
        if p2 == 0 {
            // BAD
            result_perm.push(0);
        } else if p2 < 0 {
            // Constant from second
            let val = second_constants[(-p2 - 1) as usize];
            let idx = add_constant(&mut result_constants, val);
            result_perm.push(-((idx + 1) as i64));
        } else {
            // p2 > 0: look up in first_perm
            let first_idx = (p2 - 1) as usize;
            if first_idx >= first_perm.len() {
                // Out of range in first — treat as BAD
                result_perm.push(0);
            } else {
                let p1 = first_perm[first_idx];
                if p1 == 0 {
                    // BAD propagates
                    result_perm.push(0);
                } else if p1 < 0 {
                    // Constant from first
                    let val = first_constants[(-p1 - 1) as usize];
                    let idx = add_constant(&mut result_constants, val);
                    result_perm.push(-((idx + 1) as i64));
                } else {
                    // Pass through input coordinate
                    result_perm.push(p1);
                }
            }
        }
    }

    (result_perm, result_constants)
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

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        let perm = if self.inverted { &self.inperm } else { &self.outperm };
        let idx = ax1 - 1;
        if idx < perm.len() && perm[idx] == ax2 as i64 {
            Ok(1.0)
        } else {
            Ok(0.0)
        }
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
        let other_perm = other.as_any().downcast_ref::<PermMap>()?;

        // Determine effective forward/inverse permutations based on
        // invert flags.  A non-inverted PermMap uses outperm for forward
        // and inperm for inverse; an inverted one swaps them.
        let self_eff_outperm = if self_inv {
            &self.inperm
        } else {
            &self.outperm
        };
        let self_eff_inperm = if self_inv {
            &self.outperm
        } else {
            &self.inperm
        };
        let other_eff_outperm = if other_inv {
            &other_perm.inperm
        } else {
            &other_perm.outperm
        };
        let other_eff_inperm = if other_inv {
            &other_perm.outperm
        } else {
            &other_perm.inperm
        };

        // Compose forward: self then other.
        let (fwd_perm, fwd_consts) = compose_perms(
            self_eff_outperm,
            other_eff_outperm,
            &self.constants,
            &other_perm.constants,
        );

        // Compose inverse: other_inv then self_inv (inverse of A then B
        // is B_inv then A_inv).
        let (inv_perm, inv_consts) = compose_perms(
            other_eff_inperm,
            self_eff_inperm,
            &other_perm.constants,
            &self.constants,
        );

        // Merge constant pools from both compositions.
        let mut merged_constants: Vec<f64> = Vec::new();

        // Remap forward perm constant references into the merged pool.
        let fwd_perm_merged: Vec<i64> = fwd_perm
            .iter()
            .map(|&p| {
                if p < 0 {
                    let val = fwd_consts[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    -((idx + 1) as i64)
                } else {
                    p
                }
            })
            .collect();

        // Remap inverse perm constant references into the merged pool.
        let inv_perm_merged: Vec<i64> = inv_perm
            .iter()
            .map(|&p| {
                if p < 0 {
                    let val = inv_consts[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    -((idx + 1) as i64)
                } else {
                    p
                }
            })
            .collect();

        // Check if result is identity: all outperm[i] == i+1, no BAD,
        // same nin/nout, and inperm is also identity.
        let nout = fwd_perm_merged.len();
        let nin = inv_perm_merged.len();
        let fwd_is_identity = nout == nin
            && fwd_perm_merged
                .iter()
                .enumerate()
                .all(|(i, &p)| p == (i as i64 + 1));
        let inv_is_identity = inv_perm_merged
            .iter()
            .enumerate()
            .all(|(i, &p)| p == (i as i64 + 1));

        if fwd_is_identity && inv_is_identity {
            Some(Box::new(UnitMap::new(nin)))
        } else {
            // Build a new PermMap.  The constructor validates the arrays.
            match PermMap::new(&inv_perm_merged, &fwd_perm_merged, &merged_constants) {
                Ok(pm) => Some(Box::new(pm)),
                Err(_) => None,
            }
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<PermMap>() else {
            return false;
        };
        // Resolve invert flags to get effective outperm/inperm.
        let self_outperm = if self.inverted { &self.inperm } else { &self.outperm };
        let self_inperm = if self.inverted { &self.outperm } else { &self.inperm };
        let other_outperm = if other.inverted { &other.inperm } else { &other.outperm };
        let other_inperm = if other.inverted { &other.outperm } else { &other.inperm };

        if self_outperm.len() != other_outperm.len()
            || self_inperm.len() != other_inperm.len()
        {
            return false;
        }
        if self_outperm != other_outperm || self_inperm != other_inperm {
            return false;
        }
        // Compare constants referenced by the effective permutations.
        // Constants are shared between forward and inverse — compare
        // them all with tolerance.
        self.constants.len() == other.constants.len()
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

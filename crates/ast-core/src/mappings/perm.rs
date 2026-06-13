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
use crate::error::AstResult;
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
    /// Whether this mapping is protected from simplification.
    do_not_simplify: bool,
    /// Serialize outperm as `OutCpy=1` (identity shorthand).
    out_is_copy: bool,
    /// Serialize inperm as `InCpy=1` (identity shorthand).
    in_is_copy: bool,
}

impl PermMap {
    /// Create a new `PermMap`.
    ///
    /// - `inperm`: length = nin; drives the inverse transform.
    /// - `outperm`: length = nout; drives the forward transform.
    /// - `constants`: values referenced by negative perm entries.
    ///
    /// No validation is performed on the perm-array values, mirroring C
    /// `astInitPermMap` (permmap.c:2849-2903), which stores the supplied
    /// arrays verbatim. A positive entry referring to a non-existent axis,
    /// or a negative entry referring past the end of `constants`, is
    /// substituted with `BAD_VALUE` at transform time (permmap.c:1976-1997),
    /// not rejected here. This is required so that PermMaps deserialised from
    /// a C native dump — which may legitimately carry such entries (e.g. a
    /// negative entry with no `Nconst`) — round-trip and transform as C does.
    pub fn new(inperm: &[i64], outperm: &[i64], constants: &[f64]) -> AstResult<Self> {
        Ok(Self {
            outperm: outperm.to_vec(),
            inperm: inperm.to_vec(),
            constants: constants.to_vec(),
            inverted: false,
            do_not_simplify: false,
            out_is_copy: false,
            in_is_copy: false,
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
        let ncoord_in = pts.ncoord();
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for (k, &p) in perm.iter().enumerate() {
                // Mirrors C permmap.c:1976-1997: a positive entry copies the
                // input axis only when in range; a negative entry reads the
                // constants pool when in range; everything else is BAD. C's
                // NULL-constants path uses AST__BAD for any negative entry, so
                // an out-of-range constant index resolves to BAD here too.
                outp[k] = if p > 0 {
                    let src = (p as usize) - 1;
                    if src < ncoord_in && !is_bad(inp[src]) {
                        inp[src]
                    } else {
                        BAD_VALUE
                    }
                } else if p < 0 {
                    let cidx = (-p as usize) - 1;
                    constants.get(cidx).copied().unwrap_or(BAD_VALUE)
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

    /// NaN-as-bad fast path. Permutations are direct copies, so an NaN
    /// input slot copies to an NaN output slot for free — no per-point
    /// `is_bad` gate needed.
    ///
    /// Two cases need explicit substitution to keep the protocol pure:
    /// - The constants pool (`p < 0`) is class data, not boundary-
    ///   substituted, so any `BAD_VALUE` entry is rewritten to NaN once
    ///   before the loop.
    /// - The explicit-BAD case (`p == 0`) emits NaN instead of BAD so the
    ///   sentinel stays canonical inside the pipeline.
    ///
    /// The boundary post-pass converts these NaN values back to
    /// `BAD_VALUE` at the output of the pipeline.
    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let perm = if fwd {
            self.outperm.as_slice()
        } else {
            self.inperm.as_slice()
        };
        // Convert any BAD_VALUE constants to NaN once at the start so the
        // per-point copy path is branchless on constant content.
        let constants_nan: Vec<f64> = self
            .constants
            .iter()
            .map(|&c| if is_bad(c) { f64::NAN } else { c })
            .collect();
        let ncoord_in = pts.ncoord();
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for (k, &p) in perm.iter().enumerate() {
                outp[k] = if p > 0 {
                    let src = (p as usize) - 1;
                    if src < ncoord_in { inp[src] } else { f64::NAN }
                } else if p < 0 {
                    let cidx = (-p as usize) - 1;
                    constants_nan.get(cidx).copied().unwrap_or(f64::NAN)
                } else {
                    f64::NAN
                };
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }

        let n = self.outperm.len();
        let outperm_is_identity = n == self.inperm.len()
            && self
                .outperm
                .iter()
                .enumerate()
                .all(|(i, &p)| p == (i as i64 + 1));
        let inperm_is_identity = self
            .inperm
            .iter()
            .enumerate()
            .all(|(i, &p)| p == (i as i64 + 1));
        if outperm_is_identity && inperm_is_identity {
            Ok(Box::new(UnitMap::new(n)))
        } else if self.inverted {
            Ok(Box::new(PermMap {
                outperm: self.inperm.clone(),
                inperm: self.outperm.clone(),
                constants: self.constants.clone(),
                inverted: false,
                do_not_simplify: false,
                out_is_copy: self.in_is_copy,
                in_is_copy: self.out_is_copy,
            }))
        } else {
            Ok(Box::new(self.clone()))
        }
    }

    fn map_split(&self, in_axes: &[usize]) -> Option<(Vec<usize>, DynMapping)> {
        // Mirrors permmap.c::MapSplit (~1438). For a PermMap, the split is
        // determined by walking the effective forward perm: each output `iout`
        // whose source is one of the kept inputs joins the sub-mapping. The
        // effective inverse perm then provides each kept input's source — if
        // that source is an output index it must be in the kept set
        // (otherwise the split fails); constants (negative entries) and BAD
        // (0) are passed through verbatim into the sub-PermMap.
        //
        // Note vs the plan: outputs whose source is a constant are silently
        // dropped, NOT cause a None result. The sub-PermMap simply has fewer
        // outputs. Constants in inperm survive (the same constants pool is
        // reused).
        //
        // Encoding translation: C stores 0-based input/output indices with
        // negative meaning constant; this Rust port stores 1-based with
        // negative=constant and 0=BAD. So `p > 0` here matches C's
        // `iin >= 0 && iin < npin`, and `p <= 0` matches C's `iin < 0`.
        //
        // Project convention: reject duplicate or out-of-range axes (Task 1.1
        // set the precedent; C only checks the in-range half).
        //
        // The C `astGetPermSplit(this) != 0` "alternative" mode is not
        // ported — `PermSplit` is not yet a settable attribute in this
        // crate, so the default (0 = "normal" method) is the only path.
        let npin = self.nin();
        let npout = self.nout();
        crate::mappings::helpers::validate_in_axes(in_axes, npin)?;

        // Effective forward (`outp`) and inverse (`inp`) perms, accounting
        // for Invert. Matches C: `outp = this->inperm` when inverted, etc.
        let (outp, inp) = if self.inverted {
            (self.inperm.as_slice(), self.outperm.as_slice())
        } else {
            (self.outperm.as_slice(), self.inperm.as_slice())
        };

        // Walk the forward perm: pick outputs whose source input is kept.
        // `kept_outs[k]` is the original-mapping output index of the k-th
        // kept output; `outpm[k]` is its (1-based) position in `in_axes`,
        // i.e. the source-input position in the sub-mapping.
        let mut kept_outs: Vec<usize> = Vec::with_capacity(npout);
        let mut outpm: Vec<i64> = Vec::with_capacity(npout);
        for (iout, &p) in outp.iter().enumerate() {
            if p > 0 {
                let iin = (p as usize) - 1;
                if iin < npin {
                    if let Some(pos) = in_axes.iter().position(|&v| v == iin) {
                        outpm.push((pos as i64) + 1);
                        kept_outs.push(iout);
                    }
                }
            }
            // p <= 0 (constant or BAD) or p out-of-range: output is dropped.
        }

        // C: "If the returned Mapping has no outputs, do not return it."
        if kept_outs.is_empty() {
            return None;
        }

        // Walk the inverse perm: each kept input must be fed by either a
        // constant/BAD (pass through) or by one of the kept outputs.
        let mut inpm: Vec<i64> = Vec::with_capacity(in_axes.len());
        for &iin in in_axes {
            let p = inp[iin];
            if p > 0 {
                let iout = (p as usize) - 1;
                if iout < npout {
                    // Source is an output index: must be in kept_outs.
                    match kept_outs.iter().position(|&v| v == iout) {
                        Some(pos) => inpm.push((pos as i64) + 1),
                        None => return None,
                    }
                } else {
                    // Out-of-range positive: C transform treats this as BAD;
                    // canonicalise to 0 so the new constructor accepts it.
                    inpm.push(0);
                }
            } else {
                // Constant (p < 0) or explicit BAD (p == 0): pass through.
                inpm.push(p);
            }
        }

        // Construct the sub-PermMap. The constants pool is reused as-is —
        // negative entries in inpm/outpm continue to address the same
        // constants. The new PermMap has Invert=0; this matches C, where
        // the effective directions of the parent become the raw directions
        // of the child (so we do NOT propagate `self.inverted`).
        let sub = PermMap::new(&inpm, &outpm, &self.constants).ok()?;
        Some((kept_outs, Box::new(sub)))
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        let perm = if self.inverted {
            &self.inperm
        } else {
            &self.outperm
        };
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
            // Parallel merge: two PermMaps operating on disjoint coordinate
            // ranges combine into a single larger PermMap with offset-adjusted
            // indices. Also handles UnitMap as an identity PermMap.
            let other_perm: std::borrow::Cow<'_, PermMap>;
            if let Some(pm) = other.as_any().downcast_ref::<PermMap>() {
                other_perm = std::borrow::Cow::Borrowed(pm);
            } else if let Some(um) = other.as_any().downcast_ref::<UnitMap>() {
                let n = um.nin();
                let identity: Vec<i64> = (1..=n as i64).collect();
                let pm = PermMap::new(&identity, &identity, &[]).ok()?;
                other_perm = std::borrow::Cow::Owned(pm);
            } else {
                return None;
            }
            let other_perm = other_perm.as_ref();

            let self_outperm = if self_inv {
                &self.inperm
            } else {
                &self.outperm
            };
            let self_inperm = if self_inv {
                &self.outperm
            } else {
                &self.inperm
            };
            let other_outperm = if other_inv {
                &other_perm.inperm
            } else {
                &other_perm.outperm
            };
            let other_inperm = if other_inv {
                &other_perm.outperm
            } else {
                &other_perm.inperm
            };

            let n1_in = self_inperm.len();
            let n1_out = self_outperm.len();

            let mut merged_constants: Vec<f64> = Vec::new();

            // Build combined outperm.
            let mut new_outperm: Vec<i64> = Vec::new();
            for &p in self_outperm {
                if p > 0 {
                    new_outperm.push(p);
                } else if p < 0 {
                    let val = self.constants[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    new_outperm.push(-((idx + 1) as i64));
                } else {
                    new_outperm.push(0);
                }
            }
            for &p in other_outperm {
                if p > 0 {
                    new_outperm.push(p + n1_in as i64); // offset by first's input count
                } else if p < 0 {
                    let val = other_perm.constants[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    new_outperm.push(-((idx + 1) as i64));
                } else {
                    new_outperm.push(0);
                }
            }

            // Build combined inperm.
            let mut new_inperm: Vec<i64> = Vec::new();
            for &p in self_inperm {
                if p > 0 {
                    new_inperm.push(p);
                } else if p < 0 {
                    let val = self.constants[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    new_inperm.push(-((idx + 1) as i64));
                } else {
                    new_inperm.push(0);
                }
            }
            for &p in other_inperm {
                if p > 0 {
                    new_inperm.push(p + n1_out as i64); // offset by first's output count
                } else if p < 0 {
                    let val = other_perm.constants[(-p - 1) as usize];
                    let idx = add_constant(&mut merged_constants, val);
                    new_inperm.push(-((idx + 1) as i64));
                } else {
                    new_inperm.push(0);
                }
            }

            return PermMap::new(&new_inperm, &new_outperm, &merged_constants)
                .ok()
                .map(|pm| Box::new(pm) as DynMapping);
        }

        // PermMap + MatrixMap → MatrixMap.
        // Convention: self (PermMap) applied first, other (MatrixMap) applied second.
        // Result = A * P where A is the matrix and P is the permutation matrix.
        // The PermMap permutes the columns (inputs) of the MatrixMap.
        if let Some(other_mat) = other.as_any().downcast_ref::<super::matrix::MatrixMap>() {
            // Get effective outperm for self (the forward permutation of the PermMap).
            let perm = if self_inv {
                &self.inperm
            } else {
                &self.outperm
            };
            let perm_nout = perm.len();

            // Only handle pure permutations (all positive — no constants or BAD).
            if perm.iter().any(|&p| p <= 0) {
                return None;
            }

            // Get effective matrix for the MatrixMap.
            let a = if other_inv {
                other_mat.inverse_matrix()?.clone()
            } else {
                other_mat.forward_matrix().clone()
            };

            // Dimensions: PermMap output feeds MatrixMap input.
            // a.ncols() == perm_nout must hold.
            if a.ncols() != perm_nout {
                return None;
            }

            // PermMap input dimension (number of columns of the result).
            let perm_nin = if self_inv {
                self.outperm.len()
            } else {
                self.inperm.len()
            };

            // Build permutation matrix P (perm_nout × perm_nin):
            // P[row, col] = 1 where perm[row] == col+1.
            // P maps perm_nin inputs → perm_nout outputs.
            let mut perm_mat = ndarray::Array2::<f64>::zeros((perm_nout, perm_nin));
            for (row, &p) in perm.iter().enumerate() {
                let col = (p as usize) - 1;
                if col >= perm_nin {
                    return None;
                }
                perm_mat[[row, col]] = 1.0;
            }

            // Result = A * P (MatrixMap applied after PermMap).
            let product = a.dot(&perm_mat);
            if super::matrix::is_identity_matrix_pub(&product) {
                return Some(Box::new(UnitMap::new(product.nrows())));
            }
            return super::matrix::MatrixMap::new(product)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }

        // Handle UnitMap: composing with identity produces a fresh PermMap
        // with copy flags set for identity arrays (matches C behavior where
        // MapMerge creates a PermMap with NULL for identity arrays).
        if other.as_any().is::<UnitMap>() {
            let eff_outperm = if self_inv {
                &self.inperm
            } else {
                &self.outperm
            };
            let eff_inperm = if self_inv {
                &self.outperm
            } else {
                &self.inperm
            };
            let out_is_identity = eff_outperm
                .iter()
                .enumerate()
                .all(|(i, &p)| p == (i as i64 + 1));
            let in_is_identity = eff_inperm
                .iter()
                .enumerate()
                .all(|(i, &p)| p == (i as i64 + 1));
            let mut result = PermMap::new(eff_inperm, eff_outperm, &self.constants).ok()?;
            result.out_is_copy = out_is_identity;
            result.in_is_copy = in_is_identity;
            return Some(Box::new(result));
        }

        // Handle linear mapping (WinMap, ShiftMap, ZoomMap, diagonal MatrixMap):
        // PermMap ; Linear can be simplified when the linear transform on
        // constant-injected axes can be folded into the PermMap constants,
        // and pass-through axes are identity in the linear mapping.
        if let Some((lin_shifts, lin_scales)) =
            super::winmap::extract_linear_params(other, other_inv)
        {
            let eff_outperm = if self_inv {
                &self.inperm
            } else {
                &self.outperm
            };
            let eff_inperm = if self_inv {
                &self.outperm
            } else {
                &self.inperm
            };
            let nout = eff_outperm.len();
            if lin_shifts.len() != nout {
                return None;
            }
            // Check: for each output axis, either:
            // - it passes through an input (positive perm entry) and the linear
            //   transform on that axis is identity (shift=0, scale=1), OR
            // - it injects a constant (negative perm entry) or BAD (0), and
            //   the linear transform can be folded into the constant.
            let mut can_fold = true;
            let mut new_constants = self.constants.clone();
            for (k, &p) in eff_outperm.iter().enumerate() {
                if p > 0 {
                    if !super::helpers::ast_equal(lin_shifts[k], 0.0)
                        || !super::helpers::ast_equal(lin_scales[k], 1.0)
                    {
                        can_fold = false;
                        break;
                    }
                } else if p < 0 {
                    let cidx = (-p - 1) as usize;
                    let old_val = new_constants[cidx];
                    if super::helpers::is_bad(old_val) {
                        continue;
                    }
                    new_constants[cidx] = lin_shifts[k] + lin_scales[k] * old_val;
                }
                // p == 0 (BAD): linear transform on BAD produces BAD, no change needed
            }
            if can_fold {
                let result = PermMap::new(eff_inperm, eff_outperm, &new_constants).ok()?;
                return Some(Box::new(result));
            }
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
                Ok(mut pm) => {
                    pm.out_is_copy = fwd_is_identity;
                    pm.in_is_copy = inv_is_identity;
                    Some(Box::new(pm))
                }
                Err(_) => None,
            }
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<PermMap>() else {
            return false;
        };

        // Port of C `PermMap::Equal` (permmap.c:224). Two PermMaps are
        // equivalent when, for every input and every output, the effective
        // permutation entries either agree, or both feed a constant whose
        // *referenced value* agrees. Surplus (unreferenced) constants are
        // ignored, and reordered constants pools are tolerated as long as
        // the referenced values match.
        //
        // Encoding note: Rust stores C's *public* permutation encoding
        // (1-based axis, 0 = BAD, -j = constants[j-1]); C `Equal` operates
        // on the *internal* encoding (0-based axis, BAD -> nin/nout,
        // negative -> constant at -p-1). We compare in the internal space
        // via `to_internal` so the algorithm mirrors C line-for-line.

        // Resolve invert flags to obtain the effective inverse-transform
        // ("inp", length nin) and forward-transform ("outp", length nout)
        // arrays, exactly as C does at permmap.c:318-340.
        let (self_inp, self_outp) = if self.inverted {
            (&self.outperm, &self.inperm)
        } else {
            (&self.inperm, &self.outperm)
        };
        let (other_inp, other_outp) = if other.inverted {
            (&other.outperm, &other.inperm)
        } else {
            (&other.inperm, &other.outperm)
        };

        // Nin/Nout must match (permmap.c:305).
        let nin = self_inp.len();
        let nout = self_outp.len();
        if other_inp.len() != nin || other_outp.len() != nout {
            return false;
        }

        // Map a Rust public perm entry to C's internal value, given the
        // BAD target (nout for the inverse array, nin for the forward
        // array — permmap.c:2664-2685). Axis refs become 0-based; BAD (0)
        // becomes the one-past-the-end sentinel; constants stay negative.
        let to_internal = |v: i64, bad_target: usize| -> i64 {
            if v > 0 {
                v - 1
            } else if v == 0 {
                bad_target as i64
            } else {
                v
            }
        };

        // Compare one input/output position. Mirrors permmap.c:355-371:
        // differing entries are unequal unless both are constants
        // (internal p < 0) whose referenced values are exactly equal.
        // C uses an exact `!=` on the constants (not a tolerance), and
        // when the internal p values are identical it does NOT compare the
        // referenced constants at all — so two PermMaps that both reference
        // the same constant slot are deemed equal even if the stored
        // constant differs (a C quirk; see docs/issues/c-library-quirks.md).
        let entry_eq = |p1: i64, p2: i64| -> bool {
            if p1 == p2 {
                return true;
            }
            if p1 >= 0 || p2 >= 0 {
                return false;
            }
            // Both constants: compare the referenced values exactly.
            let c1 = self.constants.get((-p1 - 1) as usize);
            let c2 = other.constants.get((-p2 - 1) as usize);
            match (c1, c2) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            }
        };

        // Check every input (inverse transform), then every output
        // (forward transform), as in permmap.c:343-392.
        for i in 0..nin {
            let p1 = to_internal(self_inp[i], nout);
            let p2 = to_internal(other_inp[i], nout);
            if !entry_eq(p1, p2) {
                return false;
            }
        }
        for i in 0..nout {
            let p1 = to_internal(self_outp[i], nin);
            let p2 = to_internal(other_outp[i], nin);
            if !entry_eq(p1, p2) {
                return false;
            }
        }
        true
    }

    fn do_not_simplify(&self) -> bool {
        self.do_not_simplify
    }

    fn set_do_not_simplify(&mut self, flag: bool) {
        self.do_not_simplify = flag;
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
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
        if self.out_is_copy {
            node.push_int(
                "OutCpy",
                1,
                Some("Output coordinates = input coordinates".to_string()),
            );
        } else {
            for (i, &p) in self.outperm.iter().enumerate() {
                let comment = if p > 0 {
                    format!("Output coordinate {} = input coordinate {}", i + 1, p)
                } else if p == 0 {
                    format!("Output coordinate {} is \"bad\"", i + 1)
                } else {
                    format!("Output coordinate {} = constant no. {}", i + 1, -p)
                };
                node.push_int(&format!("Out{}", i + 1), p, Some(comment));
            }
        }

        // Inperm (inverse permutation).
        if self.in_is_copy {
            node.push_int(
                "InCpy",
                1,
                Some("Input coordinates = output coordinates".to_string()),
            );
        } else {
            for (i, &p) in self.inperm.iter().enumerate() {
                let comment = if p > 0 {
                    format!("Input coordinate {} = output coordinate {}", i + 1, p)
                } else if p == 0 {
                    format!("Input coordinate {} is \"bad\"", i + 1)
                } else {
                    format!("Input coordinate {} = constant no. {}", i + 1, -p)
                };
                node.push_int(&format!("In{}", i + 1), p, Some(comment));
            }
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
                let comment = format!("Constant number {}", i + 1);
                if is_bad(c) {
                    node.push_string(&key, "<bad>", Some(comment));
                } else {
                    node.push_double(&key, c, Some(comment));
                }
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);

        // Check for OutCpy shorthand (identity forward permutation).
        let out_cpy = node.get_int("OutCpy", 0) != 0;
        let outperm = if out_cpy {
            // Identity: output i = input i (1-based).
            (1..=base.nout as i64).collect()
        } else {
            let mut v = Vec::with_capacity(base.nout);
            for i in 0..base.nout {
                v.push(node.get_int(&format!("Out{}", i + 1), 0));
            }
            v
        };

        // Check for InCpy shorthand (identity inverse permutation).
        let in_cpy = node.get_int("InCpy", 0) != 0;
        let inperm = if in_cpy {
            // Identity: input i = output i (1-based).
            (1..=base.nin as i64).collect()
        } else {
            let mut v = Vec::with_capacity(base.nin);
            for i in 0..base.nin {
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
        m.out_is_copy = out_cpy;
        m.in_is_copy = in_cpy;
        m.set_do_not_simplify(base.do_not_simplify);
        if base.inverted {
            m.invert();
        }
        Ok(m)
    }
}

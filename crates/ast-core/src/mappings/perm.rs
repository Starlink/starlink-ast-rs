//! Axis permutation/selection mapping.
//!
//! `PermMap` selects, reorders, or assigns constant values to axes.  The
//! permutation encoding uses 1-based indices that match the C AST convention:
//!
//! - Positive `j`: the output gets the value of input axis `j` (1-based).
//! - Negative `-j`: the output gets `constants[j-1]` (a constant value).
//! - Zero: the output gets `BAD_VALUE`.

use crate::constants::BAD_VALUE;
use crate::coords::{PointSet, Points, PointsMut};
use crate::error::AstResult;
use crate::serial::{Node, NodeValue, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

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
    /// Whether this mapping has already been simplified (C `IsSimple`).
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
    /// Serialize outperm as `OutCpy=1` (identity shorthand).
    out_is_copy: bool,
    /// Serialize inperm as `InCpy=1` (identity shorthand).
    in_is_copy: bool,
    /// `PermSplit`: select the alternative `map_split` strategy. When set,
    /// `map_split` ignores `outperm` and treats `inperm` as the exact inverse
    /// of the forward transform (permmap.c PermSplit attribute, ~2010).
    /// `None` while unset (C's `-INT_MAX`).
    perm_split: Option<bool>,
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
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            out_is_copy: false,
            in_is_copy: false,
            perm_split: None,
        })
    }

    pub(crate) fn outperm(&self) -> &[i64] {
        &self.outperm
    }

    pub(crate) fn inperm(&self) -> &[i64] {
        &self.inperm
    }

    /// Canonicalise a lone PermMap to the form C `MapMerge` produces
    /// (permmap.c:741) and report whether that counts as a simplification.
    ///
    /// C rebuilds a nominated PermMap by re-deriving its forward/inverse
    /// permutation arrays, storing `NULL` (→ `OutCpy`/`InCpy`) for any array
    /// that is the "null permutation", and clearing the Invert flag. It
    /// reports `simpler` (permmap.c:1362-1373) when the PermMap collapses to
    /// a UnitMap, its Invert flag was set, or an array that was previously
    /// stored is now recognised as null (so it need no longer be stored).
    ///
    /// Returns `Some(canonical)` when such a simplification is available, or
    /// `None` when the PermMap is already in its canonical stored form.
    ///
    /// `eff_inv` is the direction the PermMap is applied in within the Mapping
    /// list — C's `(*invert_list)[where]`, which is what clause (c) of C's
    /// `simpler` test reads. The list element itself is always stored forward,
    /// so the object's own flag is not that direction.
    pub(crate) fn canonical_null_form(&self, eff_inv: bool) -> Option<DynMapping> {
        // Resolve the applied direction to the effective forward
        // (`outperm`) and inverse (`inperm`) arrays; C MapMerge always rebuilds
        // in the forward orientation with the Invert flag cleared
        // (permmap.c:1420 `invert_list[imap1] = 0`).
        let (eff_out, eff_in) = if eff_inv {
            (&self.inperm, &self.outperm)
        } else {
            (&self.outperm, &self.inperm)
        };

        // Rebuild the arrays and a compacted constants pool, mirroring C's
        // accumulation (permmap.c:1065-1094).
        let (in_norm, out_norm, new_consts) = self.rebuild_perms(eff_in, eff_out);
        let nout = out_norm.len();
        let nin = in_norm.len();

        // `store_x` (permmap.c:1300-1339): an array need not be stored when it
        // is the dimension-aware "null permutation" (copy the same-index
        // coordinate where it exists, "bad" otherwise). A dropped array is
        // serialised as `OutCpy`/`InCpy`.
        let out_is_copy = is_null_perm(&out_norm, nin);
        let in_is_copy = is_null_perm(&in_norm, nout);

        // Collapse to a UnitMap when both transforms are null and the
        // dimensions match (permmap.c:1350).
        if out_is_copy && in_is_copy && nin == nout {
            return Some(Box::new(UnitMap::new(nin)));
        }

        // C `simpler` (permmap.c:1362-1373) for a lone PermMap: the Invert flag
        // was set (always cleared in the rebuild), or an array that was
        // genuinely stored no longer needs storing — the accumulated array is
        // null (`!store_x`) *and* the original stored array was not already a
        // strict-identity null per `NullPerm` (permmap.c:1695). `NullPerm`
        // inspects the raw stored array with a dimension-agnostic `perm[i]==i`
        // test, so an explicitly-stored identity array (e.g. `Out1=1, Out2=2`)
        // is already null to it and dropping it is not a new simplification,
        // whereas an array carrying dangling constant/bad references that only
        // *accumulates* to null is. Both tests pair with the same (forward)
        // transform, so resolve NullPerm through the applied direction too.
        // An array C does not store at all (`OutCpy`/`InCpy`, here the
        // `*_is_copy` flags) is null to `NullPerm` too.
        let (nullperm_out, nullperm_in) = if eff_inv {
            (
                self.in_is_copy || strict_null_perm(&self.inperm, self.outperm.len()),
                self.out_is_copy || strict_null_perm(&self.outperm, self.inperm.len()),
            )
        } else {
            (
                self.out_is_copy || strict_null_perm(&self.outperm, self.inperm.len()),
                self.in_is_copy || strict_null_perm(&self.inperm, self.outperm.len()),
            )
        };

        // C's final tests (permmap.c:1375-1395): when a stored array is still
        // needed, compare it element-wise against the rebuilt one — any
        // difference (a re-routed bad reference or a renumbered constant) is
        // itself a simplification.
        let simpler = eff_inv
            || (out_is_copy && !nullperm_out)
            || (in_is_copy && !nullperm_in)
            || (!in_is_copy && in_norm != *eff_in)
            || (!out_is_copy && out_norm != *eff_out);

        if !simpler {
            return None;
        }

        Some(Box::new(PermMap {
            outperm: out_norm,
            inperm: in_norm,
            constants: new_consts,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            out_is_copy,
            in_is_copy,
            perm_split: self.perm_split,
        }))
    }

    pub(crate) fn constants(&self) -> &[f64] {
        &self.constants
    }

    /// Rebuild the effective permutation arrays and a compacted constants
    /// pool, mirroring the accumulation C `permmap.c::MapMerge` performs when
    /// it reconstructs a nominated PermMap (permmap.c:1065-1094).
    ///
    /// The inverse array is walked first, then the forward array (C fills
    /// `inperm` on the `back == 0` pass before `outperm`). A negative entry
    /// whose referenced constant is absent (no constants array) or equal to
    /// `AST__BAD` becomes the plain bad marker `0`; every other referenced
    /// constant is appended to a fresh pool — deduplicated by exact equality
    /// (C: `con[ icon ] == constant`) — and the entry renumbered to reference
    /// the compacted pool. Constants never referenced by either array are
    /// dropped.
    ///
    /// Returns `(inperm, outperm, constants)` for the rebuilt PermMap.
    fn rebuild_perms(
        &self,
        eff_inperm: &[i64],
        eff_outperm: &[i64],
    ) -> (Vec<i64>, Vec<i64>, Vec<f64>) {
        let mut pool: Vec<f64> = Vec::new();
        let mut remap = |perm: &[i64]| -> Vec<i64> {
            perm.iter()
                .map(|&p| {
                    if p >= 0 {
                        return p;
                    }
                    let cidx = (-p - 1) as usize;
                    match self.constants.get(cidx) {
                        Some(&c) if c != BAD_VALUE => {
                            let k = pool.iter().position(|&e| e == c).unwrap_or_else(|| {
                                pool.push(c);
                                pool.len() - 1
                            });
                            -((k + 1) as i64)
                        }
                        _ => 0,
                    }
                })
                .collect()
        };
        let inperm = remap(eff_inperm);
        let outperm = remap(eff_outperm);
        (inperm, outperm, pool)
    }

    /// Return the `PermSplit` attribute (default `false`).
    ///
    /// When `false`, [`Mapping::map_split`] follows the public documentation,
    /// splitting on the forward (`outperm`) transform. When `true`, the
    /// forward transform is ignored and `inperm` is assumed to be its exact
    /// inverse, yielding a `UnitMap`-based split (permmap.c `PermSplit`).
    pub fn perm_split(&self) -> bool {
        self.perm_split.unwrap_or(false)
    }

    /// Set the `PermSplit` attribute.
    ///
    /// Unlike most Mapping attributes this may be changed at any time, because
    /// it changes neither the forward nor the inverse transform — only how
    /// `astMapSplit` decomposes the Mapping (permmap.c `SetPermSplit`).
    pub fn set_perm_split(&mut self, value: bool) {
        // `PermSplit` changes only how astMapSplit decomposes the PermMap and
        // PermMap's own MapMerge does not read it, but C discards the records on
        // every setter it covers, because doing so costs only a redundant
        // simplification (permmap.c:2067-2070, the convention at
        // mapping.h:449-472). The clear is guarded on the value differing, as C's
        // is -- see `LutMap::set_interp` for why that guard matters to the
        // loader. An unset value differs from both, as C's `-INT_MAX` does.
        if Some(value) != self.perm_split {
            self.simplified.clear();
        }
        self.perm_split = Some(value);
    }

    /// Whether `PermSplit` has been set, `false` included (permmap.c
    /// `TestPermSplit`).
    pub fn test_perm_split(&self) -> bool {
        self.perm_split.is_some()
    }

    /// Clear the `PermSplit` attribute, restoring the `false` default.
    pub fn clear_perm_split(&mut self) {
        // As for `set_perm_split` (permmap.c:2067).
        self.simplified.clear();
        self.perm_split = None;
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
    fn apply_perm(perm: &[i64], constants: &[f64], pts: &Points<'_>, out: &mut PointsMut<'_>) {
        let ncoord_in = pts.ncoord();
        let n = pts.npoint();
        for (k, &p) in perm.iter().enumerate() {
            let o = &mut out.axis_mut(k)[..n];
            // Mirrors C permmap.c:1976-1997: a positive entry copies the
            // input axis only when in range; a negative entry reads the
            // constants pool when in range; everything else is BAD. C's
            // NULL-constants path uses AST__BAD for any negative entry, so
            // an out-of-range constant index resolves to BAD here too.
            if p > 0 && ((p as usize) - 1) < ncoord_in {
                for (o, &v) in o.iter_mut().zip(pts.axis((p as usize) - 1)) {
                    *o = if is_bad(v) { BAD_VALUE } else { v };
                }
            } else if p < 0 {
                o.fill(
                    constants
                        .get((-p as usize) - 1)
                        .copied()
                        .unwrap_or(BAD_VALUE),
                );
            } else {
                o.fill(BAD_VALUE);
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

/// Materialise C's "null permutation" for a perm array of length `len`
/// whose values index a coordinate space of size `max` (1-based, 0 = bad).
///
/// Mirrors C `permmap.c::PERMVAL` (155): a NULL permutation array supplies
/// `PERMVAL(NULL, i, max) = (i < max ? i : -1)` — the coordinate with the
/// same index where it exists, and "bad" otherwise. In the Rust public
/// encoding this is `i+1` for `i < max`, and `0` (bad) for the surplus
/// entries. For a square PermMap (`len == max`) this is just the identity.
fn null_perm(len: usize, max: usize) -> Vec<i64> {
    (0..len)
        .map(|i| if i < max { i as i64 + 1 } else { 0 })
        .collect()
}

/// Report whether `perm` (in the Rust public encoding) is C's "null
/// permutation" for a coordinate space of size `max` — i.e. whether it need
/// not be stored by the replacement PermMap (permmap.c:1300-1339 store_in /
/// store_out test).
///
/// The null pattern is: entry `i` copies the same-index coordinate where it
/// exists (`i < max` ⇒ value `i+1`, 1-based) and is "bad" (`0`) for the
/// surplus entries (`i >= max`). Constant references (negative entries) are
/// never null.
fn is_null_perm(perm: &[i64], max: usize) -> bool {
    perm.iter()
        .enumerate()
        .all(|(i, &p)| if i < max { p == i as i64 + 1 } else { p == 0 })
}

/// Report whether a *stored* permutation array is a "null axis permutation"
/// in the sense of C `permmap.c::NullPerm` (1705): every element `i` holds
/// `i` in C's internal encoding, where a coordinate reference is 0-based, a
/// constant is negative, and "bad" is stored as `nother`, the number of
/// coordinates on the other side (permmap.c:2690-2712). `nother` matters:
/// a bad entry at index `nother` reads as `i` to C, so `inperm = [1, 2, 0]`
/// of a 3-in, 2-out PermMap is null to `NullPerm`.
///
/// Unlike [`is_null_perm`], this is not otherwise dimension-aware. It
/// matches C's strict test on the raw stored array, which is used only to
/// decide whether dropping an array counts as a *new* simplification.
fn strict_null_perm(perm: &[i64], nother: usize) -> bool {
    perm.iter().enumerate().all(|(i, &p)| {
        let internal = match p {
            0 => nother as i64,
            p if p > 0 => p - 1,
            p => p,
        };
        internal == i as i64
    })
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
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

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

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let fwd = self.effective_forward(forward);
        let (perm, nout) = if fwd {
            (self.outperm.as_slice(), self.outperm.len())
        } else {
            (self.inperm.as_slice(), self.inperm.len())
        };
        let mut out = PointSet::zeros(pts.npoint(), nout);
        Self::apply_perm(
            perm,
            &self.constants,
            &pts.as_points(),
            &mut out.as_points_mut(),
        );
        Ok(out)
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let fwd = self.effective_forward(forward);
        let perm = if fwd {
            self.outperm.as_slice()
        } else {
            self.inperm.as_slice()
        };
        Self::apply_perm(perm, &self.constants, &pts, &mut out);
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
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let perm = if fwd {
            self.outperm.as_slice()
        } else {
            self.inperm.as_slice()
        };
        // Convert any BAD_VALUE constants to NaN once at the start.
        let constants_nan: Vec<f64> = self
            .constants
            .iter()
            .map(|&c| if is_bad(c) { f64::NAN } else { c })
            .collect();
        let ncoord_in = pts.ncoord();
        let n = pts.npoint();
        for (k, &p) in perm.iter().enumerate() {
            let o = &mut out.axis_mut(k)[..n];
            if p > 0 && ((p as usize) - 1) < ncoord_in {
                o.copy_from_slice(pts.axis((p as usize) - 1));
            } else if p < 0 {
                o.fill(
                    constants_nan
                        .get((-p as usize) - 1)
                        .copied()
                        .unwrap_or(f64::NAN),
                );
            } else {
                o.fill(f64::NAN);
            }
        }
        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        super::simplify::perm_map_merge(self, ctx, where_, series)
    }

    fn map_split_zero_based(
        &self,
        in_axes: &[usize],
    ) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        // C allocates the returned output indices with astMalloc, which
        // gives NULL for none, so an empty selection gives no split.
        if in_axes.is_empty() {
            return Ok(None);
        }
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
        // An input out of range gives no split (fixed upstream in 010b5e85,
        // where C read the permutation arrays beyond their ends). A repeated
        // input is allowed: each output it feeds is kept once.
        let npin = self.nin();
        let npout = self.nout();
        if in_axes.iter().any(|&ix| ix >= npin) {
            return Ok(None);
        }

        // Effective forward (`outp`) and inverse (`inp`) perms, accounting
        // for Invert. Matches C: `outp = this->inperm` when inverted, etc.
        let (outp, inp) = if self.inverted {
            (self.inperm.as_slice(), self.outperm.as_slice())
        } else {
            (self.outperm.as_slice(), self.inperm.as_slice())
        };

        // The "alternative" method, used when the PermSplit attribute is set
        // (permmap.c:1633). The forward (`outperm`) transform is ignored and
        // `inperm` is assumed to be its exact inverse: only the inverse
        // transform is consulted. The returned Mapping is a UnitMap with one
        // output per selected input, and the returned output indices are the
        // `inperm` values of the selected inputs. The split fails if a
        // selected input is fed by a constant (negative entry) or if two
        // selected inputs are fed by the same output.
        if self.perm_split() {
            // C `PERMVAL(inp, iin, npout)` reduced to a zero-based output
            // index (mirroring the macro at permmap.c:146). When the parent's
            // inverse array is the NULL identity (`in_is_copy`), C uses
            // `iin < npout ? iin : -1`; otherwise it reads the stored value
            // verbatim. The Rust inperm is 1-based (positive) / 0 (bad) /
            // negative (constant), so translate to C's 0-based encoding where
            // a "bad" entry is a non-negative index >= npout.
            let in_is_copy = self.in_is_copy;
            let mut result: Vec<i64> = Vec::with_capacity(in_axes.len());
            for &iin in in_axes {
                let permval: i64 = if in_is_copy {
                    if iin < npout { iin as i64 } else { -1 }
                } else {
                    let p = inp[iin];
                    if p > 0 {
                        p - 1
                    } else if p < 0 {
                        p
                    } else {
                        // Rust-encoded bad: C stores this as an index >= npout
                        // that is recorded verbatim (non-negative, so it is not
                        // treated as a constant). Use a value that is both
                        // >= npout and distinct from every real output index.
                        (npout + iin) as i64
                    }
                };

                // C: a constant-fed input cannot be split (permmap.c:1643).
                if permval < 0 {
                    return Ok(None);
                }
                // C: fail if this output already feeds an earlier picked input
                // (permmap.c:1649).
                if result.contains(&permval) {
                    return Ok(None);
                }
                result.push(permval);
            }
            let out_idx: Vec<usize> = result.iter().map(|&v| v as usize).collect();
            let unit: DynMapping = Box::new(UnitMap::new(in_axes.len()));
            return Ok(Some((out_idx, unit)));
        }

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
            return Ok(None);
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
                    let Some(pos) = kept_outs.iter().position(|&v| v == iout) else {
                        return Ok(None);
                    };
                    inpm.push((pos as i64) + 1);
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
        let sub = PermMap::new(&inpm, &outpm, &self.constants)?;
        Ok(Some((kept_outs, Box::new(sub))))
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, _at, ax1, ax2)?;
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
            } else {
                let um = other.as_any().downcast_ref::<UnitMap>()?;
                let n = um.nin();
                let identity: Vec<i64> = (1..=n as i64).collect();
                let pm = PermMap::new(&identity, &identity, &[]).ok()?;
                other_perm = std::borrow::Cow::Owned(pm);
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
            let product = super::matrix::mtr_product(&a, &perm_mat);
            if super::matrix::is_identity_matrix_pub(&product) {
                return Some(Box::new(UnitMap::new(product.nrows())));
            }
            // A dimension-changing PermMap (perm_nin != perm_nout) produces a
            // rectangular product matrix. A rectangular MatrixMap has no
            // inverse, whereas the PermMap it replaces does (a PermMap is
            // always invertible — it permutes/copies/drops axes). Folding the
            // PermMap into a rectangular MatrixMap would therefore silently
            // drop a transform direction, breaking later inverse use (e.g. the
            // SpecFluxFrame conversion). Only merge when the result is square
            // (and thus potentially invertible); leave the PermMap intact
            // otherwise.
            if product.nrows() != product.ncols() {
                return None;
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

        // Compare one input/output position. Mirrors fixed C
        // permmap.c:361-386 (commit 0fbd9cc3): if either entry is an axis
        // reference (>= 0) the entries must be identical; otherwise both
        // feed a constant and the *referenced values* must be exactly
        // equal — even when both reference the same constant index. C uses
        // an exact `!=` on the constants (not a tolerance); a missing
        // constant slot is treated as BAD (matching the forward
        // transformation's NULL-constants behaviour).
        let entry_eq = |p1: i64, p2: i64| -> bool {
            if p1 >= 0 || p2 >= 0 {
                return p1 == p2;
            }
            // Both constants: compare the referenced values exactly. An
            // out-of-range slot reads as BAD_VALUE.
            let c1 = self
                .constants
                .get((-p1 - 1) as usize)
                .copied()
                .unwrap_or(BAD_VALUE);
            let c2 = other
                .constants
                .get((-p2 - 1) as usize)
                .copied()
                .unwrap_or(BAD_VALUE);
            c1 == c2
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

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }
    fn clear_id(&mut self) {
        self.id.clear();
    }
    fn test_id(&self) -> bool {
        self.id.test()
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

impl Serializable for PermMap {
    fn class_name(&self) -> &'static str {
        "PermMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Coordinate permutation".to_string());
        write_mapping_base(&mut node, self);

        // PermSplit (permmap.c Dump, "pmsplt"), not helpful when unset.
        let v = self.perm_split.unwrap_or(false);
        node.write_int(
            "pmsplt",
            self.perm_split.is_some(),
            false,
            i64::from(v),
            if v {
                "Use alternative astMapSplit implementation"
            } else {
                "Use normal astMapSplit implementation"
            },
        );

        // The highest constant number the permutations written below refer
        // to; C writes only the constants up to it (permmap.c Dump).
        let mut nconst = 0;

        // Outperm (forward permutation).
        node.write_int(
            "OutCpy",
            self.out_is_copy,
            false,
            i64::from(self.out_is_copy),
            if self.out_is_copy {
                "Output coordinates = input coordinates"
            } else {
                "Output coordinates specified individually"
            },
        );
        if !self.out_is_copy {
            for (i, &p) in self.outperm.iter().enumerate() {
                // C `permmap.c` Dump (2304-2313): a negative entry names a
                // constant only when the PermMap actually has a constants
                // array; with no constants it is "bad".
                let comment = if p > 0 {
                    format!("Output coordinate {} = input coordinate {}", i + 1, p)
                } else if p == 0 || self.constants.is_empty() {
                    format!("Output coordinate {} is \"bad\"", i + 1)
                } else {
                    format!("Output coordinate {} = constant no. {}", i + 1, -p)
                };
                if p < 0 {
                    nconst = nconst.max(-p);
                }
                node.push_int(&format!("Out{}", i + 1), p, Some(comment));
            }
        }

        // Inperm (inverse permutation).
        node.write_int(
            "InCpy",
            self.in_is_copy,
            false,
            i64::from(self.in_is_copy),
            if self.in_is_copy {
                "Input coordinates = output coordinates"
            } else {
                "Input coordinates specified individually"
            },
        );
        if !self.in_is_copy {
            for (i, &p) in self.inperm.iter().enumerate() {
                // See the outperm branch above (C permmap.c:2304-2313).
                let comment = if p > 0 {
                    format!("Input coordinate {} = output coordinate {}", i + 1, p)
                } else if p == 0 || self.constants.is_empty() {
                    format!("Input coordinate {} is \"bad\"", i + 1)
                } else {
                    format!("Input coordinate {} = constant no. {}", i + 1, -p)
                };
                if p < 0 {
                    nconst = nconst.max(-p);
                }
                node.push_int(&format!("In{}", i + 1), p, Some(comment));
            }
        }

        // Constants. A bad constant is written unset, so it reads back as
        // the bad default (permmap.c Dump and Load).
        if self.constants.is_empty() {
            nconst = 0;
        }
        node.write_int("Nconst", nconst != 0, false, nconst, "Number of constants");
        for (i, &c) in self.constants.iter().take(nconst as usize).enumerate() {
            let key = format!("Con{}", i + 1);
            let comment = format!("Constant number {}", i + 1);
            if is_bad(c) {
                node.push_unset(&key, NodeValue::String("<bad>".to_string()), Some(comment));
            } else {
                node.push_double(&key, c, Some(comment));
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("PermMap");
        let base = read_mapping_base(whole);

        // Check for OutCpy shorthand (null forward permutation).
        //
        // A NULL `outperm` is C's "null permutation" (permmap.c `PERMVAL`,
        // 155): output `i` copies input `i` where that input exists, and is
        // "bad" otherwise. This equals the 1-based identity only when
        // nin == nout; for a dimension-changing PermMap the surplus outputs
        // are bad (0), not a dangling positive axis reference.
        let out_cpy = node.get_int("OutCpy", 0) != 0;
        let outperm = if out_cpy {
            null_perm(base.nout, base.nin)
        } else {
            let mut v = Vec::with_capacity(base.nout);
            for i in 0..base.nout {
                v.push(node.get_int(&format!("Out{}", i + 1), 0));
            }
            v
        };

        // Check for InCpy shorthand (null inverse permutation). As above, a
        // NULL `inperm` copies output `i` where it exists and is bad past
        // the number of outputs.
        let in_cpy = node.get_int("InCpy", 0) != 0;
        let inperm = if in_cpy {
            null_perm(base.nin, base.nout)
        } else {
            let mut v = Vec::with_capacity(base.nin);
            for i in 0..base.nin {
                v.push(node.get_int(&format!("In{}", i + 1), 0));
            }
            v
        };

        // A constant that is absent, or written unset because it is bad,
        // reads as bad (permmap.c Load).
        let nconst = node.get_int("Nconst", 0).max(0) as usize;
        let mut constants = Vec::with_capacity(nconst);
        for i in 0..nconst {
            let key = format!("Con{}", i + 1);
            match node.get(&key) {
                Some(NodeValue::String(s)) if s == "<bad>" => constants.push(BAD_VALUE),
                _ => constants.push(node.get_double(&key, BAD_VALUE)),
            }
        }

        let mut m = PermMap::new(&inperm, &outperm, &constants)?;
        m.out_is_copy = out_cpy;
        m.in_is_copy = in_cpy;
        if base.inverted {
            m.invert();
        }

        m.set_has_been_simplified(base.has_been_simplified);

        // C assigns the structure component and only then re-applies it through
        // the setter, so the set changes nothing and the setter's guarded
        // `astClearIsSimple` does not fire (permmap.c:3090-3091). Seeding the field
        // first is what lets the `IsSimp` card the parent loader installed
        // survive.
        if node.get("PmSplt").is_some() {
            let value = node.get_int("PmSplt", 0) != 0;
            m.perm_split = Some(value);
            m.set_perm_split(value);
        }
        Ok(m)
    }
}

//! Chebyshev polynomial transformation mapping.
//!
//! A `ChebyMap` is similar to a [`PolyMap`] but uses Chebyshev polynomial
//! basis functions instead of simple powers. The input domain is normalized
//! to [-1, 1] before evaluation, and points outside the domain produce
//! [`BAD_VALUE`] outputs.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::point_has_bad;
use super::polymap::{PolyMap, PolyTerm};
use super::rate::numeric_rate;
use super::{DynMapping, Mapping};

/// A Chebyshev polynomial transformation mapping.
///
/// Each output coordinate is a linear combination of Chebyshev polynomials
/// of the first kind, evaluated on input coordinates normalized to the
/// range [-1, 1]. Input points outside the defined domain produce
/// [`BAD_VALUE`] outputs.
///
/// The forward transformation maps from input space to output space.
/// An optional explicit inverse can be provided that maps from output
/// space back to input space.
#[derive(Debug, Clone)]
pub struct ChebyMap {
    /// Inner PolyMap that stores the term structure (used for serialisation).
    poly: PolyMap,
    /// Lower bounds for forward input domain.
    lbnd_f: Vec<f64>,
    /// Upper bounds for forward input domain.
    ubnd_f: Vec<f64>,
    /// Normalization scale factors for forward: maps [lbnd, ubnd] to [-1, 1].
    scale_f: Vec<f64>,
    /// Normalization offsets for forward.
    offset_f: Vec<f64>,
    /// Lower bounds for inverse input domain (if inverse exists).
    lbnd_i: Option<Vec<f64>>,
    /// Upper bounds for inverse input domain.
    ubnd_i: Option<Vec<f64>>,
    /// Normalization scale factors for inverse.
    scale_i: Option<Vec<f64>>,
    /// Normalization offsets for inverse.
    offset_i: Option<Vec<f64>>,
    /// Whether the mapping is currently inverted.
    inverted: bool,
}

impl ChebyMap {
    /// Create a new `ChebyMap` with the given Chebyshev polynomial terms.
    ///
    /// `forward` defines the forward transformation: each term's `powers`
    /// are Chebyshev polynomial orders (not raw exponents), `powers` must
    /// have length `nin`, and `out_axis` must be in `1..=nout`.
    ///
    /// `inverse`, if provided, defines the explicit inverse transformation.
    ///
    /// `lbnd_f` and `ubnd_f` define the input domain for the forward
    /// transformation. Points outside this box produce [`BAD_VALUE`].
    ///
    /// `lbnd_i` and `ubnd_i`, if provided, define the input domain for
    /// the inverse transformation.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        nin: usize,
        nout: usize,
        forward: &[PolyTerm],
        inverse: Option<&[PolyTerm]>,
        lbnd_f: &[f64],
        ubnd_f: &[f64],
        lbnd_i: Option<&[f64]>,
        ubnd_i: Option<&[f64]>,
    ) -> AstResult<Self> {
        if nin == 0 {
            return Err(AstError::new(ErrorCode::Badin, "ChebyMap nin must be > 0"));
        }
        if nout == 0 {
            return Err(AstError::new(ErrorCode::Badin, "ChebyMap nout must be > 0"));
        }

        // Validate bounds lengths.
        if lbnd_f.len() != nin {
            return Err(AstError::new(
                ErrorCode::Badin,
                format!("lbnd_f has {} elements but nin = {}", lbnd_f.len(), nin),
            ));
        }
        if ubnd_f.len() != nin {
            return Err(AstError::new(
                ErrorCode::Badin,
                format!("ubnd_f has {} elements but nin = {}", ubnd_f.len(), nin),
            ));
        }

        // Validate bounds: lower < upper.
        for i in 0..nin {
            if lbnd_f[i] >= ubnd_f[i] {
                return Err(AstError::new(
                    ErrorCode::Badin,
                    format!(
                        "forward domain axis {}: lbnd ({}) >= ubnd ({})",
                        i + 1,
                        lbnd_f[i],
                        ubnd_f[i]
                    ),
                ));
            }
        }

        // Compute forward normalization.
        let mut scale_f = vec![0.0; nin];
        let mut offset_f = vec![0.0; nin];
        for i in 0..nin {
            scale_f[i] = 2.0 / (ubnd_f[i] - lbnd_f[i]);
            offset_f[i] = -(ubnd_f[i] + lbnd_f[i]) / (ubnd_f[i] - lbnd_f[i]);
        }

        // Validate and compute inverse bounds if provided.
        let (lbnd_i_v, ubnd_i_v, scale_i_v, offset_i_v) =
            if let (Some(li), Some(ui)) = (lbnd_i, ubnd_i) {
                if li.len() != nout {
                    return Err(AstError::new(
                        ErrorCode::Badin,
                        format!("lbnd_i has {} elements but nout = {}", li.len(), nout),
                    ));
                }
                if ui.len() != nout {
                    return Err(AstError::new(
                        ErrorCode::Badin,
                        format!("ubnd_i has {} elements but nout = {}", ui.len(), nout),
                    ));
                }
                for i in 0..nout {
                    if li[i] >= ui[i] {
                        return Err(AstError::new(
                            ErrorCode::Badin,
                            format!(
                                "inverse domain axis {}: lbnd ({}) >= ubnd ({})",
                                i + 1,
                                li[i],
                                ui[i]
                            ),
                        ));
                    }
                }
                let mut si = vec![0.0; nout];
                let mut oi = vec![0.0; nout];
                for i in 0..nout {
                    si[i] = 2.0 / (ui[i] - li[i]);
                    oi[i] = -(ui[i] + li[i]) / (ui[i] - li[i]);
                }
                (Some(li.to_vec()), Some(ui.to_vec()), Some(si), Some(oi))
            } else {
                (None, None, None, None)
            };

        // Create the inner PolyMap for term storage and serialisation.
        let poly = PolyMap::new(nin, nout, forward, inverse)?;

        Ok(Self {
            poly,
            lbnd_f: lbnd_f.to_vec(),
            ubnd_f: ubnd_f.to_vec(),
            scale_f,
            offset_f,
            lbnd_i: lbnd_i_v,
            ubnd_i: ubnd_i_v,
            scale_i: scale_i_v,
            offset_i: offset_i_v,
            inverted: false,
        })
    }

    /// Return the lower bounds of the forward input domain.
    pub fn forward_lbnd(&self) -> &[f64] {
        &self.lbnd_f
    }

    /// Return the upper bounds of the forward input domain.
    pub fn forward_ubnd(&self) -> &[f64] {
        &self.ubnd_f
    }

    /// Return the lower bounds of the inverse input domain, if defined.
    pub fn inverse_lbnd(&self) -> Option<&[f64]> {
        self.lbnd_i.as_deref()
    }

    /// Return the upper bounds of the inverse input domain, if defined.
    pub fn inverse_ubnd(&self) -> Option<&[f64]> {
        self.ubnd_i.as_deref()
    }

    /// Return a reference to the forward polynomial terms.
    pub fn forward_terms(&self) -> &[PolyTerm] {
        self.poly.forward_terms()
    }

    /// Return a reference to the inverse polynomial terms, if defined.
    pub fn inverse_terms(&self) -> Option<&[PolyTerm]> {
        self.poly.inverse_terms()
    }

    /// Get the domain bounds for a given direction, accounting for inversion.
    ///
    /// If `forward` is true, returns the bounds for the effective forward
    /// transformation (which depends on the inverted flag).
    pub fn domain(&self, forward: bool) -> Option<(&[f64], &[f64])> {
        let eff_fwd = forward != self.inverted;
        if eff_fwd {
            Some((&self.lbnd_f, &self.ubnd_f))
        } else {
            match (&self.lbnd_i, &self.ubnd_i) {
                (Some(li), Some(ui)) => Some((li.as_slice(), ui.as_slice())),
                _ => None,
            }
        }
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

// ---------------------------------------------------------------------------
// Chebyshev evaluation helpers
// ---------------------------------------------------------------------------

/// Evaluate the Chebyshev polynomial of the first kind T_n(x).
///
/// Uses the recurrence relation:
/// - T_0(x) = 1
/// - T_1(x) = x
/// - T_{k+1}(x) = 2*x*T_k(x) - T_{k-1}(x)
#[inline]
fn chebyshev_t(n: usize, x: f64) -> f64 {
    match n {
        0 => 1.0,
        1 => x,
        _ => {
            let mut t_prev = 1.0; // T_0
            let mut t_curr = x; // T_1
            for _ in 2..=n {
                let t_next = 2.0 * x * t_curr - t_prev;
                t_prev = t_curr;
                t_curr = t_next;
            }
            t_curr
        }
    }
}

/// Evaluate Chebyshev polynomial terms for a single input point.
///
/// 1. Checks each input is within [lbnd, ubnd]. If any is outside, all
///    outputs are BAD_VALUE.
/// 2. Normalizes inputs: `x_norm[i] = input[i] * scale[i] + offset[i]`
/// 3. For each output axis, sums matching terms:
///    `coeff * product(T_{powers[i]}(x_norm[i]))`
#[allow(clippy::too_many_arguments)]
fn eval_cheby(
    terms: &[PolyTerm],
    n_in: usize,
    n_out: usize,
    input: &[f64],
    scale: &[f64],
    offset: &[f64],
    lbnd: &[f64],
    ubnd: &[f64],
) -> Vec<f64> {
    debug_assert_eq!(input.len(), n_in);
    debug_assert_eq!(scale.len(), n_in);
    debug_assert_eq!(offset.len(), n_in);
    debug_assert_eq!(lbnd.len(), n_in);
    debug_assert_eq!(ubnd.len(), n_in);

    // Check for bad input values.
    if point_has_bad(input) {
        return vec![BAD_VALUE; n_out];
    }

    // Check domain bounds. Use a small tolerance to handle floating-point
    // rounding at the boundaries.
    for i in 0..n_in {
        if input[i] < lbnd[i] || input[i] > ubnd[i] {
            return vec![BAD_VALUE; n_out];
        }
    }

    // Normalize inputs to [-1, 1].
    let mut x_norm = vec![0.0; n_in];
    for i in 0..n_in {
        x_norm[i] = input[i] * scale[i] + offset[i];
    }

    // Evaluate terms.
    let mut output = vec![0.0; n_out];
    for term in terms {
        debug_assert_eq!(term.powers.len(), n_in);
        let mut val = term.coeff;
        for (i, &power) in term.powers.iter().enumerate() {
            val *= chebyshev_t(power, x_norm[i]);
        }
        output[term.out_axis - 1] += val;
    }
    output
}

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for ChebyMap {
    fn nin(&self) -> usize {
        self.poly.nin()
    }

    fn nout(&self) -> usize {
        self.poly.nout()
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
        // Keep the inner PolyMap in sync.
        self.poly.invert();
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let eff_fwd = self.effective_forward(forward);
        let n_out = if eff_fwd { self.nout() } else { self.nin() };
        let mut out = PointSet::zeros(pts.npoint(), n_out);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let eff_fwd = self.effective_forward(forward);

        // Get the underlying (un-inverted) dimensions.
        let (base_nin, base_nout) = if self.inverted {
            (self.poly.nout(), self.poly.nin())
        } else {
            (self.poly.nin(), self.poly.nout())
        };

        if eff_fwd {
            // Forward: use forward terms with forward scale/offset/bounds.
            let terms = self.poly.forward_terms();
            if terms.is_empty() {
                // No forward terms: all outputs zero (matching PolyMap).
                for i in 0..pts.npoint() {
                    out.point_mut(i).fill(0.0);
                }
            } else {
                for i in 0..pts.npoint() {
                    let inp = pts.point(i);
                    let result = eval_cheby(
                        terms,
                        base_nin,
                        base_nout,
                        inp,
                        &self.scale_f,
                        &self.offset_f,
                        &self.lbnd_f,
                        &self.ubnd_f,
                    );
                    out.point_mut(i).copy_from_slice(&result);
                }
            }
        } else {
            // Inverse: use inverse terms with inverse scale/offset/bounds.
            if let Some(inv_terms) = self.poly.inverse_terms() {
                if let (Some(si), Some(oi), Some(li), Some(ui)) =
                    (&self.scale_i, &self.offset_i, &self.lbnd_i, &self.ubnd_i)
                {
                    for i in 0..pts.npoint() {
                        let inp = pts.point(i);
                        let result =
                            eval_cheby(inv_terms, base_nout, base_nin, inp, si, oi, li, ui);
                        out.point_mut(i).copy_from_slice(&result);
                    }
                } else {
                    // Inverse terms exist but no domain bounds: fill with BAD.
                    for i in 0..pts.npoint() {
                        out.point_mut(i).fill(BAD_VALUE);
                    }
                }
            } else {
                // No inverse available: fill with BAD.
                for i in 0..pts.npoint() {
                    out.point_mut(i).fill(BAD_VALUE);
                }
            }
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

// ---------------------------------------------------------------------------
// Serializable
// ---------------------------------------------------------------------------

impl Serializable for ChebyMap {
    fn class_name(&self) -> &'static str {
        "ChebyMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        // Start with ChebyMap's own node, write its attributes first.
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Chebyshev polynomial transformation".to_string());
        write_mapping_base(&mut node, self);

        // Get the underlying (un-inverted) dimensions.
        let base_nin = if self.inverted {
            self.poly.nout()
        } else {
            self.poly.nin()
        };
        let base_nout = if self.inverted {
            self.poly.nin()
        } else {
            self.poly.nout()
        };

        // Write forward scale and offset.
        for i in 0..base_nin {
            node.push_double(
                &format!("FSCL{}", i + 1),
                self.scale_f[i],
                Some(format!("Scale factor on input {}", i + 1)),
            );
        }
        for i in 0..base_nin {
            node.push_double(
                &format!("FOFF{}", i + 1),
                self.offset_f[i],
                Some(format!("Offset on input {}", i + 1)),
            );
        }

        // Write inverse scale and offset (if present).
        if let Some(si) = &self.scale_i {
            for (i, &s) in si.iter().enumerate().take(base_nout) {
                node.push_double(
                    &format!("ISCL{}", i + 1),
                    s,
                    Some(format!("Scale factor on output {}", i + 1)),
                );
            }
        }
        if let Some(oi) = &self.offset_i {
            for (i, &o) in oi.iter().enumerate().take(base_nout) {
                node.push_double(
                    &format!("IOFF{}", i + 1),
                    o,
                    Some(format!("Offset on output {}", i + 1)),
                );
            }
        }

        // Now write the PolyMap coefficient structure.
        let fwd_terms = self.poly.forward_terms();
        if !fwd_terms.is_empty() {
            // Max power per input axis.
            let mut mxpow_f = vec![0usize; base_nin];
            for term in fwd_terms {
                for (i, &p) in term.powers.iter().enumerate() {
                    if p > mxpow_f[i] {
                        mxpow_f[i] = p;
                    }
                }
            }
            for (i, &mp) in mxpow_f.iter().enumerate() {
                node.push_int(
                    &format!("MPF{}", i + 1),
                    mp as i64,
                    Some(format!(
                        "Max. power of input {} in any forward polynomial",
                        i + 1
                    )),
                );
            }

            // Group terms by output axis.
            let mut terms_by_out: Vec<Vec<&PolyTerm>> = vec![Vec::new(); base_nout];
            for term in fwd_terms {
                terms_by_out[term.out_axis - 1].push(term);
            }

            // Number of coefficients per output.
            for (i, terms) in terms_by_out.iter().enumerate() {
                node.push_int(
                    &format!("NCF{}", i + 1),
                    terms.len() as i64,
                    Some(format!("No. of coeff.s for forward polynomial {}", i + 1)),
                );
            }

            // Coefficient values.
            let mut iv = 0usize;
            for (i, terms) in terms_by_out.iter().enumerate() {
                for (j, term) in terms.iter().enumerate() {
                    iv += 1;
                    if term.coeff != BAD_VALUE {
                        node.push_double(
                            &format!("CF{iv}"),
                            term.coeff,
                            Some(format!("Coeff {} of forward polynomial {}", j + 1, i + 1)),
                        );
                    }
                }
            }

            // Powers.
            iv = 0;
            for terms in &terms_by_out {
                for term in terms {
                    for (k, &p) in term.powers.iter().enumerate() {
                        iv += 1;
                        if p > 0 {
                            node.push_double(
                                &format!("PF{iv}"),
                                p as f64,
                                Some(format!("Power of i/p {} for coeff of fwd poly", k + 1)),
                            );
                        }
                    }
                }
            }
        }

        // Inverse terms.
        if let Some(inv_terms) = self.poly.inverse_terms() {
            if !inv_terms.is_empty() {
                let mut mxpow_i = vec![0usize; base_nout];
                for term in inv_terms {
                    for (i, &p) in term.powers.iter().enumerate() {
                        if p > mxpow_i[i] {
                            mxpow_i[i] = p;
                        }
                    }
                }
                for (i, &mp) in mxpow_i.iter().enumerate() {
                    node.push_int(
                        &format!("MPI{}", i + 1),
                        mp as i64,
                        Some(format!(
                            "Max. power of output {} in any inverse polynomial",
                            i + 1
                        )),
                    );
                }

                let mut terms_by_out: Vec<Vec<&PolyTerm>> = vec![Vec::new(); base_nin];
                for term in inv_terms {
                    terms_by_out[term.out_axis - 1].push(term);
                }

                for (i, terms) in terms_by_out.iter().enumerate() {
                    node.push_int(
                        &format!("NCI{}", i + 1),
                        terms.len() as i64,
                        Some(format!("No. of coeff.s for inverse polynomial {}", i + 1)),
                    );
                }

                let mut iv = 0usize;
                for (i, terms) in terms_by_out.iter().enumerate() {
                    for (j, term) in terms.iter().enumerate() {
                        iv += 1;
                        if term.coeff != BAD_VALUE {
                            node.push_double(
                                &format!("CI{iv}"),
                                term.coeff,
                                Some(format!("Coeff {} of inverse polynomial {}", j + 1, i + 1)),
                            );
                        }
                    }
                }

                iv = 0;
                for terms in &terms_by_out {
                    for term in terms {
                        for (k, &p) in term.powers.iter().enumerate() {
                            iv += 1;
                            if p > 0 {
                                node.push_double(
                                    &format!("PI{iv}"),
                                    p as f64,
                                    Some(format!("Power of o/p {} for coeff of inv poly", k + 1)),
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (nin, nout, inverted) = read_mapping_base(node);

        // --- Read ChebyMap-specific attributes: scale and offset ---
        let mut scale_f = Vec::with_capacity(nin);
        let mut offset_f = Vec::with_capacity(nin);
        let mut has_forward_domain = false;

        for i in 0..nin {
            let s = node.get_double(&format!("FSCL{}", i + 1), BAD_VALUE);
            let o = node.get_double(&format!("FOFF{}", i + 1), BAD_VALUE);
            if s != BAD_VALUE {
                has_forward_domain = true;
            }
            scale_f.push(s);
            offset_f.push(o);
        }

        // If no forward domain was serialised, this is an error for ChebyMap.
        if !has_forward_domain {
            return Err(AstError::new(
                ErrorCode::Objin,
                "ChebyMap: no forward domain scale/offset found in serialised data",
            ));
        }

        // Reconstruct forward bounds from scale/offset.
        let mut lbnd_f = vec![0.0; nin];
        let mut ubnd_f = vec![0.0; nin];
        for i in 0..nin {
            if scale_f[i] != 0.0 {
                lbnd_f[i] = (-1.0 - offset_f[i]) / scale_f[i];
                ubnd_f[i] = (1.0 - offset_f[i]) / scale_f[i];
            } else {
                lbnd_f[i] = BAD_VALUE;
                ubnd_f[i] = BAD_VALUE;
            }
        }

        // Read inverse scale/offset (if present).
        let mut scale_i_vec = Vec::with_capacity(nout);
        let mut offset_i_vec = Vec::with_capacity(nout);
        let mut has_inverse_domain = false;

        for i in 0..nout {
            let s = node.get_double(&format!("ISCL{}", i + 1), BAD_VALUE);
            let o = node.get_double(&format!("IOFF{}", i + 1), BAD_VALUE);
            if s != BAD_VALUE {
                has_inverse_domain = true;
            }
            scale_i_vec.push(s);
            offset_i_vec.push(o);
        }

        let (lbnd_i, ubnd_i, scale_i, offset_i) = if has_inverse_domain {
            let mut li = vec![0.0; nout];
            let mut ui = vec![0.0; nout];
            for i in 0..nout {
                if scale_i_vec[i] != 0.0 {
                    li[i] = (-1.0 - offset_i_vec[i]) / scale_i_vec[i];
                    ui[i] = (1.0 - offset_i_vec[i]) / scale_i_vec[i];
                } else {
                    li[i] = BAD_VALUE;
                    ui[i] = BAD_VALUE;
                }
            }
            (Some(li), Some(ui), Some(scale_i_vec), Some(offset_i_vec))
        } else {
            (None, None, None, None)
        };

        // --- Read PolyMap coefficient structure ---
        let mut forward = Vec::new();
        let has_fwd_terms = node.get("MPF1").is_some();
        if has_fwd_terms {
            let mut ncoeff_f = Vec::with_capacity(nout);
            for i in 0..nout {
                let nc = node.get_int(&format!("NCF{}", i + 1), 0) as usize;
                ncoeff_f.push(nc);
            }

            let total_coeffs: usize = ncoeff_f.iter().sum();
            let mut coeffs = Vec::with_capacity(total_coeffs);
            for iv in 1..=total_coeffs {
                coeffs.push(node.get_double(&format!("CF{iv}"), BAD_VALUE));
            }

            let total_powers: usize = total_coeffs * nin;
            let mut powers = Vec::with_capacity(total_powers);
            for iv in 1..=total_powers {
                powers.push(node.get_double(&format!("PF{iv}"), 0.0) as usize);
            }

            let mut coeff_idx = 0;
            let mut power_idx = 0;
            for (out_ax, &nc) in ncoeff_f.iter().enumerate() {
                for _ in 0..nc {
                    let coeff = coeffs[coeff_idx];
                    coeff_idx += 1;
                    let term_powers: Vec<usize> = powers[power_idx..power_idx + nin].to_vec();
                    power_idx += nin;
                    if coeff != BAD_VALUE && coeff != 0.0 {
                        forward.push(PolyTerm {
                            coeff,
                            powers: term_powers,
                            out_axis: out_ax + 1,
                        });
                    }
                }
            }
        }

        let mut inverse_terms = Vec::new();
        let has_inv_terms = node.get("MPI1").is_some();
        if has_inv_terms {
            let mut ncoeff_i = Vec::with_capacity(nin);
            for i in 0..nin {
                let nc = node.get_int(&format!("NCI{}", i + 1), 0) as usize;
                ncoeff_i.push(nc);
            }

            let total_coeffs: usize = ncoeff_i.iter().sum();
            let mut coeffs = Vec::with_capacity(total_coeffs);
            for iv in 1..=total_coeffs {
                coeffs.push(node.get_double(&format!("CI{iv}"), BAD_VALUE));
            }

            let total_powers: usize = total_coeffs * nout;
            let mut powers = Vec::with_capacity(total_powers);
            for iv in 1..=total_powers {
                powers.push(node.get_double(&format!("PI{iv}"), 0.0) as usize);
            }

            let mut coeff_idx = 0;
            let mut power_idx = 0;
            for (out_ax, &nc) in ncoeff_i.iter().enumerate() {
                for _ in 0..nc {
                    let coeff = coeffs[coeff_idx];
                    coeff_idx += 1;
                    let term_powers: Vec<usize> = powers[power_idx..power_idx + nout].to_vec();
                    power_idx += nout;
                    if coeff != BAD_VALUE && coeff != 0.0 {
                        inverse_terms.push(PolyTerm {
                            coeff,
                            powers: term_powers,
                            out_axis: out_ax + 1,
                        });
                    }
                }
            }
        }

        let inverse = if has_inv_terms {
            Some(inverse_terms)
        } else {
            None
        };

        // Build the inner PolyMap directly (avoid re-validation of potentially
        // BAD_VALUE sentinel coefficients).
        let poly = PolyMap::new(nin, nout, &forward, inverse.as_deref())?;

        let mut m = ChebyMap {
            poly,
            lbnd_f,
            ubnd_f,
            scale_f,
            offset_f,
            lbnd_i,
            ubnd_i,
            scale_i,
            offset_i,
            inverted: false,
        };
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}

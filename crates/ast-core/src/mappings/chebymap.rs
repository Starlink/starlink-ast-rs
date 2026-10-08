//! Chebyshev polynomial transformation mapping.
//!
//! A `ChebyMap` is similar to a [`PolyMap`] but uses Chebyshev polynomial
//! basis functions instead of simple powers. The input domain is normalized
//! to [-1, 1] before evaluation, and points outside the domain produce
//! [`BAD_VALUE`] outputs.
//!
//! C defines no `chebymap.c::MapMerge`, so a ChebyMap inherits PolyMap's
//! (`polymap.c:3617`), and this class ports both of its halves: the coefficient
//! consolidation (`polymap.c:3644-3765`), which combines terms over the same
//! basis-function orders, and the linear replacement (`:3767-3918`), which
//! rebuilds an all-linear transformation as a `MatrixMap` and a `ShiftMap` with
//! the input normalisation folded in.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::ast_equal;
use super::polymap::{PolyMap, PolyPowers, PolyTerm, monomial_powers, poly_transform};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

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
    /// Whether this mapping has already been simplified (C `IsSimple`).
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
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

        // Only a box of zero width is an error (chebymap.c astInitChebyMap):
        // reversed bounds give a negative scale.
        for i in 0..nin {
            if lbnd_f[i] == ubnd_f[i] {
                return Err(AstError::new(
                    ErrorCode::Badbx,
                    format!(
                        "ChebyMap: input bounding box has zero width on input axis {}",
                        i + 1
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
                    if li[i] == ui[i] {
                        return Err(AstError::new(
                            ErrorCode::Badbx,
                            format!(
                                "ChebyMap: output bounding box has zero width on output axis {}",
                                i + 1
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
        let mut poly = PolyMap::new(nin, nout, forward, inverse)?;
        // C's IterInverse default (no inverse coefficients -> iterate)
        // works for ChebyMap because the C iteration dispatches through
        // the virtual astTransform; the Rust Newton-Raphson machinery
        // evaluates power-basis terms directly and would invert the
        // wrong function here, so keep the iterative inverse disabled
        // until a basis-aware iteration is ported.
        poly.set_iter_inverse(false)?;

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
            simplified: SimplifiedRecord::default(),
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
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

    /// The bounding box of the input of the forward (`forward`) or inverse
    /// transformation, as C's `astChebyDomain` gives it (chebymap.c
    /// `ChebyDomain`): worked out from the stored scaling into [-1, 1], or,
    /// when that transformation has no box, found by MapBox from the other
    /// transformation's box. Bounds that cannot be found are `BAD_VALUE`.
    pub fn domain(&self, forward: bool) -> (Vec<f64>, Vec<f64>) {
        let box_of = |scale: &[f64], offset: &[f64]| -> (Vec<f64>, Vec<f64>) {
            scale
                .iter()
                .zip(offset)
                .map(|(&s, &o)| {
                    if s != 0.0 {
                        ((-1.0 - o) / s, (1.0 - o) / s)
                    } else {
                        (BAD_VALUE, BAD_VALUE)
                    }
                })
                .unzip()
        };
        let fwd_scale = Some((&self.scale_f[..], &self.offset_f[..]));
        let inv_scale = self.scale_i.as_deref().zip(self.offset_i.as_deref());
        let (own, other, nax) = if forward != self.inverted {
            (fwd_scale, inv_scale, self.nin())
        } else {
            (inv_scale, fwd_scale, self.nout())
        };
        if let Some((scale, offset)) = own {
            return box_of(scale, offset);
        }
        let mut lbnd = vec![BAD_VALUE; nax];
        let mut ubnd = vec![BAD_VALUE; nax];
        if let Some((scale, offset)) = other {
            // The other transformation's box is mapped in the direction
            // opposite to the requested one, as this ChebyMap stands.
            let (lbnd_o, ubnd_o) = box_of(scale, offset);
            for axis in 0..nax {
                if let Ok(b) = self.map_box(&lbnd_o, &ubnd_o, !forward, axis + 1) {
                    lbnd[axis] = b.lbnd_out;
                    ubnd[axis] = b.ubnd_out;
                }
            }
        }
        (lbnd, ubnd)
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

impl PolyPowers for ChebyMap {
    /// The Chebyshev polynomials of the first kind of each input, scaled
    /// into [-1, 1] by the transformation's bounding box, in place of its
    /// powers (chebymap.c `PolyPowers`). An input that is bad, or outside the
    /// box, has bad values above degree zero. A transformation with no box is
    /// an ordinary polynomial.
    fn poly_powers(
        &self,
        work: &mut [Vec<f64>],
        max_pows: &[usize],
        input: &[f64],
        forward: bool,
    ) -> bool {
        let (scale, offset) = if forward {
            (Some(&self.scale_f[..]), Some(&self.offset_f[..]))
        } else {
            (self.scale_i.as_deref(), self.offset_i.as_deref())
        };
        let (Some(scale), Some(offset)) = (scale, offset) else {
            return monomial_powers(work, max_pows, input);
        };
        let mut all_good = true;
        for (axis, pwork) in work.iter_mut().enumerate() {
            pwork[0] = 1.0;
            let mp = max_pows[axis];
            let x = input[axis];
            if x == BAD_VALUE {
                pwork[1..=mp].fill(BAD_VALUE);
                all_good &= mp == 0;
                continue;
            }
            let x = x * scale[axis] + offset[axis];
            if x.abs() <= 1.0 {
                // T1(x) = x, and T(n+1)(x) = 2 x Tn(x) - T(n-1)(x).
                pwork[1] = x;
                for ip in 2..=mp {
                    pwork[ip] = 2.0 * x * pwork[ip - 1] - pwork[ip - 2];
                }
            } else {
                pwork[1..=mp].fill(BAD_VALUE);
                all_good &= mp == 0;
            }
        }
        all_good
    }
}

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for ChebyMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        self.poly.nin()
    }

    fn nout(&self) -> usize {
        self.poly.nout()
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
        // Keep the inner PolyMap in sync.
        self.poly.invert();
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let eff_fwd = self.effective_forward(forward);
        let n_out = if eff_fwd { self.nout() } else { self.nin() };
        let mut out = PointSet::zeros(pts.npoint(), n_out);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into_nan(
        &self,
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        if let Some(tables) = self.poly.term_tables(fwd) {
            poly_transform(self, fwd, &tables, pts, out, f64::NAN);
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        // ChebyMap inherits PolyMap's Transform and overrides only the
        // powers (chebymap.c).
        let fwd = self.effective_forward(forward);
        if let Some(tables) = self.poly.term_tables(fwd) {
            poly_transform(self, fwd, &tables, pts, out, BAD_VALUE);
        }
        Ok(())
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
        // Only handle inverse cancellation: same ChebyMap in opposite directions.
        if self_inv == other_inv {
            return None;
        }
        let other_cheby = other.as_any().downcast_ref::<ChebyMap>()?;

        // C forces both stored `Invert` flags to the direction the list applies
        // to the nominated Mapping and then calls `astEqual`
        // (`polymap.c:3933-3967`), which reaches `chebymap.c::Equal`: the
        // comparison inherited from PolyMap plus the two bounding boxes
        // (`chebymap.c:407-515`). The inherited half rejects a pair whose
        // `Invert` flags differ (`polymap.c:824-829`), so the forcing is what
        // lets two entries pointing in opposite directions compare at all;
        // what it then compares is each transformation as constructed.
        let mut a = self.clone();
        let mut b = other_cheby.clone();
        if a.inverted != self_inv {
            a.invert();
        }
        if b.inverted != self_inv {
            b.invert();
        }
        if !a.equal(&b) {
            return None;
        }

        // The pair's input dimension depends on which direction self is used.
        let pair_nin = if self_inv { self.nout() } else { self.nin() };
        Some(Box::new(UnitMap::new(pair_nin)))
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C has no `chebymap.c::MapMerge`, so a ChebyMap's is PolyMap's whole
        // body (`polymap.c:3617`), and both of its replace-with-simpler halves
        // run: the coefficient consolidation (`polymap.c:3644-3765`), which
        // combines terms over the same basis-function orders, and then the
        // linear replacement (`:3767-3918`) on the terms the first half leaves.
        //
        // The coefficients are those of a Chebyshev series in the normalised
        // input `z = scale*x + offset`, and `T0(z) = 1` and `T1(z) = z` are the
        // only Chebyshev polynomials of degree one or less, so a term the
        // linearity test accepts is linear in the input as well as in `z` — with
        // the normalisation folded into the matrix and the offsets
        // (`polymap.c:3787-3793`).
        //
        // Like PolyMap, this works from the transformation as *constructed* and
        // leaves the list's applied direction alone (`polymap.c:3899-3902`), so
        // the replacement inherits `eff_inv` rather than absorbing it.
        let eff_inv = ctx.entry(where_).1;
        let consolidation = self.poly.consolidated_terms();
        let terms = match &consolidation {
            Some((fwd, _)) => fwd.as_slice(),
            None => self.poly.forward_terms(),
        };

        // The normalisation is the one applied to the constructed forward
        // transformation's inputs, which is what those coefficients belong to.
        if let Some(linear) = self
            .poly
            .linear_replacement(terms, Some((&self.scale_f, &self.offset_f)))
        {
            ctx.set_pair_single(where_, (linear, eff_inv, false));
            return Some(where_);
        }

        if let Some((fwd, inv)) = consolidation {
            let (nin, nout) = if self.inverted {
                (self.nout(), self.nin())
            } else {
                (self.nin(), self.nout())
            };
            let mut rebuilt = ChebyMap::new(
                nin,
                nout,
                &fwd,
                inv.as_deref(),
                &self.lbnd_f,
                &self.ubnd_f,
                self.lbnd_i.as_deref(),
                self.ubnd_i.as_deref(),
            )
            .ok()?;
            if self.inverted {
                rebuilt.invert();
            }
            ctx.set_pair_single(where_, (Box::new(rebuilt), eff_inv, false));
            return Some(where_);
        }

        super::simplify::default_map_merge(ctx, where_, series)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<ChebyMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        // Compare inner PolyMap.
        if !self.poly.equal(&other.poly) {
            return false;
        }
        // Compare forward bounds.
        if self.lbnd_f.len() != other.lbnd_f.len() {
            return false;
        }
        let fwd_bounds = self
            .lbnd_f
            .iter()
            .zip(&other.lbnd_f)
            .all(|(a, b)| ast_equal(*a, *b))
            && self
                .ubnd_f
                .iter()
                .zip(&other.ubnd_f)
                .all(|(a, b)| ast_equal(*a, *b));
        if !fwd_bounds {
            return false;
        }
        // Compare inverse bounds.
        let inv_lbnd = match (&self.lbnd_i, &other.lbnd_i) {
            (Some(a), Some(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| ast_equal(*x, *y))
            }
            (None, None) => true,
            _ => false,
        };
        let inv_ubnd = match (&self.ubnd_i, &other.ubnd_i) {
            (Some(a), Some(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| ast_equal(*x, *y))
            }
            (None, None) => true,
            _ => false,
        };
        inv_lbnd && inv_ubnd
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

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    // The inner PolyMap's Invert state is kept in sync by
    // `ChebyMap::invert`, so its definedness flags already reflect this
    // ChebyMap's orientation.
    fn has_forward(&self) -> bool {
        self.poly.has_forward()
    }

    fn has_inverse(&self) -> bool {
        self.poly.has_inverse()
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
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
        // The C library writes ChebyMap dumps with the following section
        // structure, reflecting the class hierarchy Mapping -> PolyMap ->
        // ChebyMap (each `IsA <Parent>` marker terminates the parent's
        // instance-variable block):
        //
        //     Begin ChebyMap
        //        Nin / Nout / Fwd / Inv      <- Mapping section
        //     IsA Mapping
        //        MPF/NCF/CF/PF (+inverse)    <- PolyMap section
        //     IsA PolyMap
        //        FSCL/FOFF/ISCL/IOFF         <- ChebyMap section
        //     End ChebyMap
        //
        // The Rust serializer must follow the same order *exactly* — both
        // for byte-exact round-tripping against the simplify-fixture
        // corpus and because the C library's astLoadChebyMap reads the
        // sections in this order; ChebyMap-level attributes that arrive
        // before `IsA PolyMap` get applied to the PolyMap parent struct
        // instead and never reach the ChebyMap-specific fields, leading
        // to a segfault on first eval.
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

        // --- PolyMap section: polynomial coefficient structure ---
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
            for (i, terms) in terms_by_out.iter().enumerate() {
                for (j, term) in terms.iter().enumerate() {
                    for (k, &p) in term.powers.iter().enumerate() {
                        iv += 1;
                        if p > 0 {
                            node.push_double(
                                &format!("PF{iv}"),
                                p as f64,
                                Some(format!(
                                    "Power of i/p {} for coeff {} of fwd poly {}",
                                    k + 1,
                                    j + 1,
                                    i + 1,
                                )),
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
                for (i, terms) in terms_by_out.iter().enumerate() {
                    for (j, term) in terms.iter().enumerate() {
                        for (k, &p) in term.powers.iter().enumerate() {
                            iv += 1;
                            if p > 0 {
                                node.push_double(
                                    &format!("PI{iv}"),
                                    p as f64,
                                    Some(format!(
                                        "Power of o/p {} for coeff {} of inv poly {}",
                                        k + 1,
                                        j + 1,
                                        i + 1,
                                    )),
                                );
                            }
                        }
                    }
                }
            }
        }

        // --- IsA marker terminating the PolyMap instance-variable block ---
        node.push_isa("PolyMap", Some("Polynomial transformation".to_string()));

        // --- ChebyMap section: forward and inverse scale/offset ---
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

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c): the
        // coefficients from the PolyMap part, the domain from the ChebyMap
        // part.
        let whole = node;
        let poly = &whole.section("PolyMap");
        let node = &whole.section("ChebyMap");
        let base = read_mapping_base(whole);
        let nin = base.nin;
        let nout = base.nout;

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
        let has_fwd_terms = poly.get("MPF1").is_some();
        if has_fwd_terms {
            let mut ncoeff_f = Vec::with_capacity(nout);
            for i in 0..nout {
                let nc = poly.get_int(&format!("NCF{}", i + 1), 0) as usize;
                ncoeff_f.push(nc);
            }

            let total_coeffs: usize = ncoeff_f.iter().sum();
            let mut coeffs = Vec::with_capacity(total_coeffs);
            for iv in 1..=total_coeffs {
                coeffs.push(poly.get_double(&format!("CF{iv}"), BAD_VALUE));
            }

            let total_powers: usize = total_coeffs * nin;
            let mut powers = Vec::with_capacity(total_powers);
            for iv in 1..=total_powers {
                powers.push(poly.get_double(&format!("PF{iv}"), 0.0) as usize);
            }

            let mut coeff_idx = 0;
            let mut power_idx = 0;
            for (out_ax, &nc) in ncoeff_f.iter().enumerate() {
                for _ in 0..nc {
                    let coeff = coeffs[coeff_idx];
                    coeff_idx += 1;
                    let term_powers: Vec<usize> = powers[power_idx..power_idx + nin].to_vec();
                    power_idx += nin;
                    // A coefficient missing from the dump is bad, as C reads it,
                    // and makes its output bad.
                    forward.push(PolyTerm {
                        coeff,
                        powers: term_powers,
                        out_axis: out_ax + 1,
                    });
                }
            }
        }

        let mut inverse_terms = Vec::new();
        let has_inv_terms = poly.get("MPI1").is_some();
        if has_inv_terms {
            let mut ncoeff_i = Vec::with_capacity(nin);
            for i in 0..nin {
                let nc = poly.get_int(&format!("NCI{}", i + 1), 0) as usize;
                ncoeff_i.push(nc);
            }

            let total_coeffs: usize = ncoeff_i.iter().sum();
            let mut coeffs = Vec::with_capacity(total_coeffs);
            for iv in 1..=total_coeffs {
                coeffs.push(poly.get_double(&format!("CI{iv}"), BAD_VALUE));
            }

            let total_powers: usize = total_coeffs * nout;
            let mut powers = Vec::with_capacity(total_powers);
            for iv in 1..=total_powers {
                powers.push(poly.get_double(&format!("PI{iv}"), 0.0) as usize);
            }

            let mut coeff_idx = 0;
            let mut power_idx = 0;
            for (out_ax, &nc) in ncoeff_i.iter().enumerate() {
                for _ in 0..nc {
                    let coeff = coeffs[coeff_idx];
                    coeff_idx += 1;
                    let term_powers: Vec<usize> = powers[power_idx..power_idx + nout].to_vec();
                    power_idx += nout;
                    if coeff != 0.0 {
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
        let mut poly = PolyMap::new(nin, nout, &forward, inverse.as_deref())?;
        // See ChebyMap::new — the power-basis NR cannot iterate a
        // Chebyshev forward, so the C IterInverse default is disabled.
        poly.set_iter_inverse(false)?;

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
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
        };
        if base.inverted {
            m.invert();
        }
        Ok(m)
    }
}

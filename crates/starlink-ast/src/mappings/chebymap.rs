// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

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
//! the input normalization folded in.

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::ast_equal;
use super::polymap::{PolyFit, PolyMap, PolyPowers, PolyTerm, monomial_powers, poly_transform};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// Map coordinates using Chebyshev polynomial functions.
///
/// A ChebyMap performs a Chebyshev polynomial transformation.
/// Each output coordinate is a linear combination of Chebyshev polynomials of
/// the first kind, of order zero up to a specified maximum order, evaluated at
/// the input coordinates.
/// The coefficients of the linear combination are specified separately for
/// each output coordinate, as [`PolyTerm`]s.
///
/// For a 1-dimensional ChebyMap the forward transformation is
///
/// ```text
/// f(x) = c0 T0(x') + c1 T1(x') + c2 T2(x') + ...
/// ```
///
/// where `Tn` is the Chebyshev polynomial of the first kind of order `n`
/// (`T0(x') = 1`, `T1(x') = x'`, `Tn+1(x') = 2 x' Tn(x') - Tn-1(x')`), and `x'`
/// is the input value `x` offset and scaled to the range [-1, 1] as `x` ranges
/// over a bounding box given when the ChebyMap is created.
/// Input positions must fall within the bounding box: a point outside it
/// transforms to [`BAD_VALUE`].
///
/// For an N-dimensional ChebyMap each output value is the sum of its terms,
/// where each term is the product of a coefficient and N factors `Tn(x'_i)`,
/// `x'_i` being the normalized value of input `i`.
///
/// The forward and inverse transformations are defined independently by
/// separate sets of coefficients.
/// If no inverse coefficients are supplied, [`poly_tran`](ChebyMap::poly_tran)
/// can create an inverse transformation: a Chebyshev polynomial whose
/// coefficients minimize the residuals left by a round trip (forward
/// transformation followed by inverse transformation).
/// Unlike a [`PolyMap`], a ChebyMap never provides an iterative inverse.
///
/// [`domain`](ChebyMap::domain) returns the bounding box of either
/// transformation.
#[derive(Debug, Clone)]
pub struct ChebyMap {
    /// Inner PolyMap that stores the term structure (used for serialization).
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
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl ChebyMap {
    /// Create a ChebyMap with `nin` input and `nout` output coordinates from
    /// the given Chebyshev polynomial terms.
    ///
    /// `forward` defines the forward transformation: each term's `powers`
    /// holds one Chebyshev polynomial order per input, and its `out_axis` is
    /// in `1..=nout`.
    /// For instance, with three inputs, the term
    /// `PolyTerm { coeff: 1.2, powers: vec![1, 3, 0], out_axis: 2 }` adds to
    /// output 2 the product of 1.2, `T1` evaluated at input 1 and `T3`
    /// evaluated at input 2; input 3 is not used.
    /// A term whose powers are all zero adds a constant.
    /// An empty `forward` leaves the forward transformation undefined.
    ///
    /// `inverse`, if given and not empty, defines the inverse transformation
    /// the same way with inputs and outputs transposed; otherwise the inverse
    /// is undefined.
    ///
    /// `lbnd_f` and `ubnd_f` hold the lower and upper bounds, one per input,
    /// of the input bounding box within which the forward transformation is
    /// defined.
    /// `lbnd_i` and `ubnd_i` hold the bounds, one per output, of the output
    /// bounding box within which the inverse transformation is defined.
    /// They are required when inverse coefficients are given, and ignored
    /// otherwise.
    ///
    /// ```
    /// use starlink_ast::mappings::{ChebyMap, Mapping, PolyTerm};
    ///
    /// // f(x) = 1 + 2 T2(x') with x in [0, 10]: T2(x') = 2 x'^2 - 1.
    /// let terms = [
    ///     PolyTerm { coeff: 1.0, powers: vec![0], out_axis: 1 },
    ///     PolyTerm { coeff: 2.0, powers: vec![2], out_axis: 1 },
    /// ];
    /// let map = ChebyMap::new(1, 1, &terms, None, &[0.0], &[10.0], None, None).unwrap();
    /// // x = 10 gives x' = 1 and T2 = 1.
    /// assert_eq!(map.tran1(&[10.0], true).unwrap(), vec![3.0]);
    /// ```
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badin` if `nin` or `nout` is zero, a bounds slice has
    ///   the wrong length, inverse coefficients are given without `lbnd_i`
    ///   and `ubnd_i`, or a term has the wrong number of powers.
    /// - `ErrorCode::Badbx` if a bounding box has zero width on any axis.
    /// - `ErrorCode::Badci` if a term's `out_axis` is out of range.
    #[doc(alias = "astChebyMap")]
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
        // C's astInitChebyMap reads lbnd_i and ubnd_i whenever there are
        // inverse coefficients (chebymap.c:2144-2151) and has no defined
        // result without them, so the call is refused.
        if inverse.is_some_and(|t| !t.is_empty()) && (lbnd_i.is_none() || ubnd_i.is_none()) {
            return Err(AstError::new(
                ErrorCode::Badin,
                "ChebyMap: inverse coefficients were supplied without the bounds of \
                 the inverse transformation's domain",
            ));
        }
        Self::build(nin, nout, forward, inverse, lbnd_f, ubnd_f, lbnd_i, ubnd_i)
    }

    /// [`ChebyMap::new`] without the requirement that inverse coefficients
    /// come with inverse bounds: an inverse without them is the state a dump
    /// with no `ISCL` cards loads, whose coefficients multiply plain powers
    /// (C's PolyPowers with no inverse scaling), and a merge that rebuilds
    /// such a ChebyMap has to keep it.
    #[allow(clippy::too_many_arguments)]
    fn build(
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

        // The bounds handling follows astInitChebyMap: lbnd_i and ubnd_i are
        // read only when there are inverse coefficients.
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

        // Validate and compute inverse bounds, if there are inverse
        // coefficients for them to bound.
        let has_inverse = inverse.is_some_and(|t| !t.is_empty());
        let (lbnd_i_v, ubnd_i_v, scale_i_v, offset_i_v) =
            if let (true, Some(li), Some(ui)) = (has_inverse, lbnd_i, ubnd_i) {
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

        // Create the inner PolyMap for term storage and serialization.
        let mut poly = PolyMap::new(nin, nout, forward, inverse)?;
        // A ChebyMap never uses an iterative inverse: chebymap.c
        // `GetIterInverse` returns 0 whatever the attribute holds.
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
            id: Default::default(),
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

    /// Return a reference to the forward polynomial terms of the original,
    /// uninverted ChebyMap, in the order they were given.
    /// [`coeffs`](ChebyMap::coeffs) gives them relative to the ChebyMap as it
    /// stands.
    pub fn forward_terms(&self) -> &[PolyTerm] {
        self.poly.forward_terms()
    }

    /// Return a reference to the inverse polynomial terms of the original,
    /// uninverted ChebyMap, if defined, in the order they were given.
    pub fn inverse_terms(&self) -> Option<&[PolyTerm]> {
        self.poly.inverse_terms()
    }

    /// The Chebyshev coefficients of the forward (`forward` true) or inverse
    /// transformation of this ChebyMap, in the form [`new`](ChebyMap::new)
    /// takes them.
    ///
    /// The transformation is that of the ChebyMap as it stands, so the
    /// forward coefficients of an inverted ChebyMap are the inverse
    /// coefficients it was created with.
    /// The terms are grouped by the output each defines, in output order, and
    /// within each output in the order they were given.
    /// The result is empty when the transformation has no defining
    /// polynomials.
    ///
    /// ```
    /// use starlink_ast::mappings::{ChebyMap, Mapping, PolyTerm};
    ///
    /// let terms = [PolyTerm { coeff: 2.0, powers: vec![1], out_axis: 1 }];
    /// let mut map = ChebyMap::new(1, 1, &terms, None, &[0.0], &[10.0], None, None).unwrap();
    /// assert_eq!(map.coeffs(true), terms.to_vec());
    /// map.invert();
    /// assert!(map.coeffs(true).is_empty());
    /// ```
    #[doc(alias = "astPolyCoeffs")]
    pub fn coeffs(&self, forward: bool) -> Vec<PolyTerm> {
        // The inner PolyMap is inverted together with the ChebyMap, so its
        // coefficients already follow the ChebyMap's orientation.
        self.poly.coeffs(forward)
    }

    /// The bounding box of the domain of the forward (`forward` true) or
    /// inverse transformation, as `(lbnd, ubnd)`.
    ///
    /// These are the bounds given when the ChebyMap was created, with one
    /// value per input for the forward transformation and one per output for
    /// the inverse, both for the ChebyMap as it stands, after any inversion.
    /// The original forward transformation always has the box given to
    /// [`new`](ChebyMap::new), even when it has no coefficients.
    /// The original inverse transformation has a box only when inverse
    /// coefficients were given or fitted by
    /// [`poly_tran`](ChebyMap::poly_tran); when the requested transformation
    /// is an inverse without one, its box is found by mapping the forward box
    /// with [`map_box`](Mapping::map_box).
    /// Bounds that cannot be found are [`BAD_VALUE`]; this is not an error.
    #[doc(alias = "astChebyDomain")]
    pub fn domain(&self, forward: bool) -> (Vec<f64>, Vec<f64>) {
        // chebymap.c ChebyDomain: worked out from the stored scaling into
        // [-1, 1], or by MapBox from the other transformation's box.
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

    /// Fit a ChebyMap inverse or forward transformation.
    ///
    /// As [`PolyMap::poly_tran`], returning a ChebyMap whose new
    /// transformation is a weighted sum of Chebyshev functions of the first
    /// kind, or `None` if no fit achieves `maxacc`.
    /// The bounding box of the new transformation is the box spanned by the
    /// sampled values.
    ///
    /// `lbnd` and `ubnd` bound the rectangle sampled, in the input space of
    /// the transformation that is sampled.
    /// Either may be `None` to use the bounding box given for that
    /// transformation when the ChebyMap was created.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Nobox` if `lbnd` or `ubnd` is `None` and the sampled
    ///   transformation has no bounding box.
    /// - The errors of [`PolyMap::poly_tran`].
    #[doc(alias = "astPolyTran")]
    pub fn poly_tran(
        &self,
        forward: bool,
        acc: f64,
        maxacc: f64,
        maxorder: usize,
        lbnd: Option<&[f64]>,
        ubnd: Option<&[f64]>,
    ) -> AstResult<Option<ChebyMap>> {
        let (word, scale, offset, nax) = if forward != self.inverted {
            (
                "inverse",
                self.scale_i.as_deref(),
                self.offset_i.as_deref(),
                self.nout(),
            )
        } else {
            (
                "forward",
                Some(&self.scale_f[..]),
                Some(&self.offset_f[..]),
                self.nin(),
            )
        };

        // The supplied bounds, or the transformation's box, which spans
        // [-1, +1] once scaled.
        let bound = |given: Option<&[f64]>, which: &str, edge: f64| -> AstResult<Vec<f64>> {
            if let Some(b) = given {
                return Ok(b.iter().take(nax.min(2)).copied().collect());
            }
            match (scale, offset) {
                (Some(scale), Some(offset)) => Ok((0..nax.min(2))
                    .map(|i| (edge - offset[i]) / scale[i])
                    .collect()),
                _ => Err(AstError::new(
                    ErrorCode::Nobox,
                    format!(
                        "astPolyTran(ChebyMap): The {word} transformation is not a Chebyshev \
                         polynomial and therefore requires a user-supplied bounding box. But \
                         no {which} bounds were supplied."
                    ),
                )),
            }
        };
        let this_lbnd = bound(lbnd, "lower", -1.0)?;
        let this_ubnd = bound(ubnd, "upper", 1.0)?;

        // As PolyMap's: replace a transformation of a copy. The fit is made
        // in Chebyshev form (chebymap.c PolyTran, FitPoly1DInit and
        // FitPoly2DInit).
        let mut result = self.clone();
        result.simplified = SimplifiedRecord::default();
        let Some(new_terms) = crate::mappings::helpers::poly_tran::replace_transformation(
            &mut result,
            forward,
            acc,
            maxacc,
            maxorder,
            &this_lbnd,
            &this_ubnd,
        )?
        else {
            return Ok(None);
        };
        result
            .poly
            .store_terms(forward != self.inverted, &new_terms);
        Ok(Some(result))
    }

    /// Store the scaling of a new fit into [-1, +1] for the transformation
    /// `forward` replaces, as chebymap.c `FitPoly1DInit` and
    /// `FitPoly2DInit` do, with the bounds it implies.
    fn set_fit_box(&mut self, forward: bool, scale: Vec<f64>, offset: Vec<f64>) {
        let (lbnd, ubnd): (Vec<f64>, Vec<f64>) = scale
            .iter()
            .zip(&offset)
            .map(|(&s, &o)| ((-1.0 - o) / s, (1.0 - o) / s))
            .unzip();
        if forward != self.inverted {
            self.lbnd_f = lbnd;
            self.ubnd_f = ubnd;
            self.scale_f = scale;
            self.offset_f = offset;
        } else {
            self.lbnd_i = Some(lbnd);
            self.ubnd_i = Some(ubnd);
            self.scale_i = Some(scale);
            self.offset_i = Some(offset);
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

// ---------------------------------------------------------------------------
// Chebyshev evaluation helpers
// ---------------------------------------------------------------------------

/// The smallest and largest of `values`, found as chebymap.c's
/// `FitPoly1DInit` and `FitPoly2DInit` find them.
fn sample_range(values: &[f64]) -> (f64, f64) {
    let (mut min, mut max) = (values[0], values[0]);
    for &v in &values[1..] {
        if v > max {
            max = v;
        } else if v < min {
            min = v;
        }
    }
    (min, max)
}

/// The Chebyshev polynomials T0 to T(order-1) of each value, scaled into
/// [-1, +1] as `x * scl * scale + off`, sample by sample.
fn chebyshev_basis(values: &[f64], scl: f64, scale: f64, off: f64, order: usize) -> Vec<f64> {
    let mut xp = Vec::with_capacity(values.len() * order);
    for &v in values {
        let x = v * scl * scale + off;
        let base = xp.len();
        xp.push(1.0);
        xp.push(x);
        for w in 2..order {
            let t = 2.0 * x * xp[base + w - 1] - xp[base + w - 2];
            xp.push(t);
        }
    }
    xp
}

impl PolyFit for ChebyMap {
    /// chebymap.c `FitPoly1DInit`: the fit is in Chebyshev form over the
    /// range of the sampled values, which becomes the replaced
    /// transformation's bounding box. The ChebyMap applies that scaling
    /// itself, so `scales[0]` is reset to 1.
    fn fit_poly_1d_init(
        &mut self,
        forward: bool,
        table: &[Vec<f64>],
        order: usize,
        scales: &mut [f64],
    ) -> AstResult<Vec<f64>> {
        let (minx, maxx) = sample_range(&table[0]);
        let pmax = maxx * scales[0];
        let pmin = minx * scales[0];
        if pmax == pmin {
            return Err(AstError::new(
                ErrorCode::Badbx,
                "astPolyTran(ChebyMap): New bounding box has zero width on axis 1.",
            ));
        }
        let scl = 2.0 / (pmax - pmin);
        let off = -(pmax + pmin) / (pmax - pmin);
        self.set_fit_box(forward, vec![scl], vec![off]);
        let xp1 = chebyshev_basis(&table[0], scl, scales[0], off, order);
        scales[0] = 1.0;
        Ok(xp1)
    }

    /// chebymap.c `FitPoly2DInit`: as
    /// [`fit_poly_1d_init`](PolyFit::fit_poly_1d_init) for each input.
    fn fit_poly_2d_init(
        &mut self,
        forward: bool,
        table: &[Vec<f64>],
        order: usize,
        scales: &mut [f64],
    ) -> AstResult<(Vec<f64>, Vec<f64>)> {
        let (minx, maxx) = sample_range(&table[0]);
        let (miny, maxy) = sample_range(&table[1]);
        let pxmax = maxx * scales[0];
        let pxmin = minx * scales[0];
        let pymax = maxy * scales[1];
        let pymin = miny * scales[1];
        if pxmax == pxmin || pymax == pymin {
            return Err(AstError::new(
                ErrorCode::Badbx,
                "astPolyTran(ChebyMap): New bounding box has zero width on or both axes.",
            ));
        }
        let scl = [2.0 / (pxmax - pxmin), 2.0 / (pymax - pymin)];
        let off = [
            -(pxmax + pxmin) / (pxmax - pxmin),
            -(pymax + pymin) / (pymax - pymin),
        ];
        self.set_fit_box(forward, scl.to_vec(), off.to_vec());
        let xp1 = chebyshev_basis(&table[0], scl[0], scales[0], off[0], order);
        let xp2 = chebyshev_basis(&table[1], scl[1], scales[1], off[1], order);
        scales[0] = 1.0;
        scales[1] = 1.0;
        Ok((xp1, xp2))
    }

    fn has_coeffs(&self, forward: bool) -> bool {
        self.poly.has_coeffs(forward)
    }
}

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

    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        if let Some(tables) = self.poly.term_tables(fwd) {
            poly_transform(self, fwd, &tables, &pts, &mut out, f64::NAN);
        }
        Ok(())
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        // ChebyMap inherits PolyMap's Transform and overrides only the
        // powers (chebymap.c).
        let fwd = self.effective_forward(forward);
        if let Some(tables) = self.poly.term_tables(fwd) {
            poly_transform(self, fwd, &tables, &pts, &mut out, BAD_VALUE);
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
        // The coefficients are those of a Chebyshev series in the normalized
        // input `z = scale*x + offset`, and `T0(z) = 1` and `T1(z) = z` are the
        // only Chebyshev polynomials of degree one or less, so a term the
        // linearity test accepts is linear in the input as well as in `z` — with
        // the normalization folded into the matrix and the offsets
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

        // The normalization is the one applied to the constructed forward
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
            let rebuilt = ChebyMap::build(
                nin,
                nout,
                &fwd,
                inv.as_deref(),
                &self.lbnd_f,
                &self.ubnd_f,
                self.lbnd_i.as_deref(),
                self.ubnd_i.as_deref(),
            );
            let mut rebuilt = ctx.settle(rebuilt.map(Some))?;
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

        // The PolyMap part's iterative inverse attributes, none of which a
        // ChebyMap sets. A ChebyMap never uses an iterative inverse
        // (chebymap.c `GetIterInverse` returns 0).
        node.write_int(
            "IterInv",
            false,
            false,
            0,
            "Do not use an iterative inverse",
        );
        node.write_int(
            "NiterInv",
            false,
            false,
            self.poly.niter_inverse() as i64,
            "Max number of iterations for iterative inverse",
        );
        node.write_double(
            "TolInv",
            false,
            false,
            self.poly.tol_inverse(),
            "Target relative error for iterative inverse",
        );

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

        // If no forward domain was serialized, this is an error for ChebyMap.
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

        // The PolyMap part's coefficients.
        let forward =
            crate::mappings::polymap::read_poly_terms(poly, 'F', nin, nout).unwrap_or_default();
        let inverse = crate::mappings::polymap::read_poly_terms(poly, 'I', nout, nin);

        // Build the inner PolyMap directly (avoid re-validation of potentially
        // BAD_VALUE sentinel coefficients).
        let mut poly = PolyMap::new(nin, nout, &forward, inverse.as_deref())?;
        // As in `ChebyMap::new`, no iterative inverse.
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
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        };
        if base.inverted {
            m.invert();
        }
        Ok(m)
    }
}

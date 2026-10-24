// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! 2-D mapping using bivariate B-splines.
//!
//! See [`SplineMap`].

// SplineMap::new takes the coefficients in CMLIB order, X index fastest
// (i + j*nx), which is the order of C's Fortran API (AST_SPLINEMAP) and of
// the internal storage, the Native dump and cu/cv. C's C API (astSplineMap,
// and pyast through it) takes the SciPy order, Y index fastest (j + i*ny),
// and transposes it on entry (splinemap.c astSplineMapId_); from_scipy and
// coeffs are that API's order.

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::helpers::{ast_equal, is_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// Maximum spline order (order 4 = cubic).
const MAX_K: usize = 6;

/// Default maximum iterations for Newton-Raphson inverse.
const DEFAULT_INV_NITER: usize = 6;

/// Default relative error tolerance for the iterative inverse.
const DEFAULT_INV_TOL: f64 = 1.0e-6;

/// Map coordinates using spline functions.
///
/// A SplineMap is a Mapping that has two input axes and two output axes and
/// contains a pair of two-dimensional splines, stored in B-spline form.
/// Each output coordinate is the value of one of these two-dimensional
/// splines defined over the two input coordinates.
/// The two splines, one generating the first output coordinate and the other
/// the second, share the same order and the same knot positions, but have
/// coefficients that are specified separately.
///
/// No extrapolation is performed: input positions outside the area covered by
/// the knots cannot be transformed (but see
/// [`out_unit`](SplineMap::out_unit)).
///
/// An iterative method is used to evaluate the inverse transformation from
/// the forward transformation (see [`inv_tol`](SplineMap::inv_tol) and
/// [`inv_niter`](SplineMap::inv_niter)).
/// This restricts how a SplineMap can be used: the output axis values must
/// represent perturbed input axis values, rather than the perturbations
/// themselves, and the perturbations must be small.
/// If the input axis values are (x, y), the output axis values are (u, v)
/// and the perturbations in u and v are Pu(x, y) and Pv(x, y), then:
///
/// - u = x + Pu(x, y)
/// - v = y + Pv(x, y)
///
/// The perturbations should be small enough that u always increases with x
/// and v always increases with y.
/// The knots and coefficients supplied when the SplineMap is constructed
/// should make each B-spline represent the sum of the input axis value and
/// the perturbation: the first B-spline should have the value x + Pu(x, y),
/// not just Pu(x, y).
///
/// The order and number of coefficients along each input axis are
/// [`kx`](SplineMap::kx), [`ky`](SplineMap::ky), [`nx`](SplineMap::nx) and
/// [`ny`](SplineMap::ny); [`tx`](SplineMap::tx), [`ty`](SplineMap::ty) and
/// [`coeffs`](SplineMap::coeffs) return the knots and coefficients.
///
/// # Coefficient order
///
/// [`SplineMap::new`] takes the coefficients with the X index varying
/// fastest: element `i + j*nx` is the coefficient for X index `i` and Y index
/// `j`.
/// This is the order [`cu`](SplineMap::cu) and [`cv`](SplineMap::cv) return.
/// [`SplineMap::from_scipy`] takes them with the Y index varying fastest
/// (element `j + i*ny`), the order of SciPy's
/// `BivariateSpline.get_coeffs()`, and [`coeffs`](SplineMap::coeffs) returns
/// that order.
/// Both orders have `nx * ny` elements, so coefficients in one order given to
/// the constructor for the other build a transposed spline, without error.
#[derive(Debug, Clone)]
pub struct SplineMap {
    /// Polynomial order in X input direction (e.g. 4 = cubic).
    kx: usize,
    /// Polynomial order in Y input direction.
    ky: usize,
    /// Number of B-spline coefficients in X direction.
    nx: usize,
    /// Number of B-spline coefficients in Y direction.
    ny: usize,
    /// Knot positions in X direction (length nx + kx).
    tx: Vec<f64>,
    /// Knot positions in Y direction (length ny + ky).
    ty: Vec<f64>,
    /// Coefficients for the first (u) output (length nx * ny).
    cu: Vec<f64>,
    /// Coefficients for the second (v) output (length nx * ny).
    cv: Vec<f64>,
    /// Maximum number of iterations for iterative inverse (None = default).
    inv_niter: Option<usize>,
    /// Target relative error for iterative inverse (None = default).
    inv_tol: Option<f64>,
    /// How to handle out-of-bounds: false = BAD, true = unit (identity);
    /// `None` while unset (C's `-INT_MAX`), reading as false.
    out_unit: Option<bool>,
    /// Whether the mapping is inverted.
    inverted: bool,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl SplineMap {
    /// Create a SplineMap from its spline orders, knots and coefficients,
    /// with the coefficients in X-fastest order.
    ///
    /// # Arguments
    ///
    /// - `kx`, `ky`: the order of the splines along the first (x) and second
    ///   (y) input axes, in the range 1 to 6 (4 for a bicubic spline).
    /// - `nx`, `ny`: the number of B-spline coefficients along each input
    ///   axis, at least `kx` and `ky` respectively.
    /// - `tx`, `ty`: the knots along each input axis, `nx + kx` and
    ///   `ny + ky` of them, in non-decreasing order.
    /// - `cu`, `cv`: the `nx * ny` coefficients of the splines that generate
    ///   the first (u) and second (v) outputs, element `i + j*nx` being the
    ///   coefficient for X index `i` and Y index `j`.
    ///
    /// [`SplineMap::from_scipy`] takes the coefficients in SciPy's order.
    ///
    /// ```
    /// use starlink_ast::constants::BAD_VALUE;
    /// use starlink_ast::mappings::{Mapping, SplineMap};
    ///
    /// // Linear splines over the unit square giving u = x and v = y + 0.1 x.
    /// let knots = vec![0.0, 0.0, 1.0, 1.0];
    /// let mut map = SplineMap::new(
    ///     2, 2, 2, 2,
    ///     knots.clone(), knots,
    ///     vec![0.0, 1.0, 0.0, 1.0],
    ///     vec![0.0, 0.1, 1.0, 1.1],
    /// )
    /// .unwrap();
    /// let (u, v) = map.tran2(&[0.5], &[0.25], true).unwrap();
    /// assert!((u[0] - 0.5).abs() < 1e-12 && (v[0] - 0.3).abs() < 1e-12);
    /// // The iterative inverse recovers the input.
    /// let (x, y) = map.tran2(&u, &v, false).unwrap();
    /// assert!((x[0] - 0.5).abs() < 1e-6 && (y[0] - 0.25).abs() < 1e-6);
    /// // Outside the knots the result is bad, unless OutUnit is set.
    /// assert_eq!(map.tran2(&[2.0], &[0.5], true).unwrap().0[0], BAD_VALUE);
    /// map.set_out_unit(true);
    /// assert_eq!(map.tran2(&[2.0], &[0.5], true).unwrap().0[0], 2.0);
    /// ```
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Attin` if an order is outside 1 to 6, a number of
    ///   coefficients is smaller than the order, an array has the wrong
    ///   length, or the knots along an axis decrease.
    #[doc(alias = "astSplineMap")]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kx: usize,
        ky: usize,
        nx: usize,
        ny: usize,
        tx: Vec<f64>,
        ty: Vec<f64>,
        cu: Vec<f64>,
        cv: Vec<f64>,
    ) -> AstResult<Self> {
        // splinemap.c astInitSplineMap: each is AST__ATTIN.
        let attin = |msg: String| Err(AstError::new(ErrorCode::Attin, format!("SplineMap: {msg}")));
        if !(1..=MAX_K).contains(&kx) {
            return attin(format!(
                "the spline order in X ({kx}) must be in the range [1,{MAX_K}]"
            ));
        }
        if !(1..=MAX_K).contains(&ky) {
            return attin(format!(
                "the spline order in Y ({ky}) must be in the range [1,{MAX_K}]"
            ));
        }
        if nx < kx {
            return attin(format!(
                "the number of coefficients in X ({nx}) must be at least {kx}"
            ));
        }
        if ny < ky {
            return attin(format!(
                "the number of coefficients in Y ({ny}) must be at least {ky}"
            ));
        }
        // The array lengths C takes on trust from its arguments.
        if tx.len() != nx + kx || ty.len() != ny + ky || cu.len() != nx * ny || cv.len() != nx * ny
        {
            return attin("the knot or coefficient arrays have the wrong lengths".to_string());
        }
        if tx.windows(2).any(|w| w[0] > w[1]) {
            return attin("the knots in the X direction are not monotonic increasing".to_string());
        }
        if ty.windows(2).any(|w| w[0] > w[1]) {
            return attin("the knots in the Y direction are not monotonic increasing".to_string());
        }

        Ok(Self {
            kx,
            ky,
            nx,
            ny,
            tx,
            ty,
            cu,
            cv,
            inv_niter: None,
            inv_tol: None,
            out_unit: None,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        })
    }

    /// Create a SplineMap from coefficients in SciPy's order, the Y index
    /// varying fastest.
    ///
    /// Element `j + i*ny` of `cu` and `cv` is the coefficient for X index `i`
    /// and Y index `j`, the order of SciPy's `BivariateSpline.get_coeffs()`:
    /// the first `ny` elements hold the coefficients for the first column,
    /// the next `ny` those for the second column, and so on.
    /// The other arguments are as for [`SplineMap::new`].
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Attin` as for [`SplineMap::new`].
    #[doc(alias = "astSplineMap")]
    #[allow(clippy::too_many_arguments)]
    pub fn from_scipy(
        kx: usize,
        ky: usize,
        nx: usize,
        ny: usize,
        tx: Vec<f64>,
        ty: Vec<f64>,
        cu: &[f64],
        cv: &[f64],
    ) -> AstResult<Self> {
        // The transposition C's astSplineMapId_ makes on entry.
        if cu.len() != nx * ny || cv.len() != nx * ny {
            return Err(AstError::new(
                ErrorCode::Attin,
                "SplineMap: the knot or coefficient arrays have the wrong lengths",
            ));
        }
        let transpose = |c: &[f64]| -> Vec<f64> {
            let mut out = vec![0.0; nx * ny];
            for j in 0..ny {
                for i in 0..nx {
                    out[i + j * nx] = c[j + i * ny];
                }
            }
            out
        };
        Self::new(kx, ky, nx, ny, tx, ty, transpose(cu), transpose(cv))
    }

    /// The coefficients of the spline that generates output `axis` (1 for u,
    /// 2 for v), in the SciPy order [`SplineMap::from_scipy`] takes.
    ///
    /// Inversion is ignored: they always refer to the forward transformation
    /// of the SplineMap as originally constructed.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` if `axis` is not 1 or 2.
    #[doc(alias = "astSplineCoeffs")]
    pub fn coeffs(&self, axis: usize) -> AstResult<Vec<f64>> {
        let c = match axis {
            1 => &self.cu,
            2 => &self.cv,
            _ => {
                return Err(AstError::new(
                    ErrorCode::Axiin,
                    format!("SplineMap: the output axis ({axis}) must be 1 or 2"),
                ));
            }
        };
        let (nx, ny) = (self.nx, self.ny);
        let mut out = vec![0.0; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                out[j + i * ny] = c[i + j * nx];
            }
        }
        Ok(out)
    }

    /// The order of the splines along the first input axis (x).
    ///
    /// It is one more than the degree of the piecewise polynomial used by
    /// the spline (4 for a cubic spline in the x direction).
    /// It is fixed when the SplineMap is created.
    #[doc(alias = "SplineKx")]
    pub fn kx(&self) -> usize {
        self.kx
    }

    /// The order of the splines along the second input axis (y).
    ///
    /// It is one more than the degree of the piecewise polynomial used by
    /// the spline (4 for a cubic spline in the y direction).
    /// It is fixed when the SplineMap is created.
    #[doc(alias = "SplineKy")]
    pub fn ky(&self) -> usize {
        self.ky
    }

    /// The number of spline coefficients along the first input axis (x).
    ///
    /// It is fixed when the SplineMap is created.
    #[doc(alias = "SplineNx")]
    pub fn nx(&self) -> usize {
        self.nx
    }

    /// The number of spline coefficients along the second input axis (y).
    ///
    /// It is fixed when the SplineMap is created.
    #[doc(alias = "SplineNy")]
    pub fn ny(&self) -> usize {
        self.ny
    }

    /// The knot positions along the first input axis (x), `nx + kx` of them.
    ///
    /// Inversion is ignored: they always refer to the forward transformation
    /// as originally constructed.
    #[doc(alias = "astSplineKnots")]
    pub fn tx(&self) -> &[f64] {
        &self.tx
    }

    /// The knot positions along the second input axis (y), `ny + ky` of them.
    ///
    /// Inversion is ignored: they always refer to the forward transformation
    /// as originally constructed.
    #[doc(alias = "astSplineKnots")]
    pub fn ty(&self) -> &[f64] {
        &self.ty
    }

    /// The coefficients of the spline that generates the first (u) output, in
    /// the X-fastest order [`SplineMap::new`] takes.
    pub fn cu(&self) -> &[f64] {
        &self.cu
    }

    /// The coefficients of the spline that generates the second (v) output, in
    /// the X-fastest order [`SplineMap::new`] takes.
    pub fn cv(&self) -> &[f64] {
        &self.cv
    }

    /// The maximum number of Newton-Raphson iterations the iterative inverse
    /// transformation uses for each transformed position.
    ///
    /// If the iteration does not converge to the relative error given by
    /// [`inv_tol`](SplineMap::inv_tol) within this number of iterations,
    /// [`BAD_VALUE`] is returned for both output axis values.
    ///
    /// Default: 6.
    ///
    /// [`set_inv_niter`](SplineMap::set_inv_niter),
    /// [`test_inv_niter`](SplineMap::test_inv_niter) and
    /// [`clear_inv_niter`](SplineMap::clear_inv_niter) set, test and clear it.
    #[doc(alias = "InvNiter")]
    pub fn inv_niter(&self) -> usize {
        self.inv_niter.unwrap_or(DEFAULT_INV_NITER)
    }

    /// Set [`inv_niter`](SplineMap::inv_niter).
    ///
    /// A value different from the one held clears the record that the
    /// SplineMap has been simplified.
    pub fn set_inv_niter(&mut self, n: usize) {
        // `InvNiter` changes what the iterative inverse computes, so a
        // conclusion an earlier simplification reached about this SplineMap no
        // longer holds. SplineMap's own MapMerge does not read the attribute,
        // but C discards the records on every setter it covers, because doing so
        // costs only a redundant simplification (splinemap.c:2547-2549, the
        // convention at mapping.h:449-472). The clear is guarded on the value
        // differing, as C's is -- see `LutMap::set_interp` for why that guard
        // matters to the loader.
        if Some(n) != self.inv_niter {
            self.simplified.clear();
        }
        self.inv_niter = Some(n);
    }

    /// The target relative error of the iterative inverse transformation: the
    /// change in an axis value divided by the absolute axis value.
    ///
    /// Iterations continue until this is reached or
    /// [`inv_niter`](SplineMap::inv_niter) iterations have been made; if it is
    /// not reached, [`BAD_VALUE`] is returned for both output axis values.
    ///
    /// Default: 1.0e-6.
    ///
    /// [`set_inv_tol`](SplineMap::set_inv_tol),
    /// [`test_inv_tol`](SplineMap::test_inv_tol) and
    /// [`clear_inv_tol`](SplineMap::clear_inv_tol) set, test and clear it.
    #[doc(alias = "InvTol")]
    pub fn inv_tol(&self) -> f64 {
        self.inv_tol.unwrap_or(DEFAULT_INV_TOL)
    }

    /// Set [`inv_tol`](SplineMap::inv_tol).
    ///
    /// [`BAD_VALUE`] clears it instead.
    /// A value different from the one held clears the record that the
    /// SplineMap has been simplified.
    pub fn set_inv_tol(&mut self, tol: f64) {
        // As for `set_inv_niter` (splinemap.c:2586-2588).
        // C compares the raw value, AST__BAD while unset (splinemap.c:2590).
        if tol != self.inv_tol.unwrap_or(BAD_VALUE) {
            self.simplified.clear();
        }
        // AST__BAD is C's unset value, so setting it unsets the attribute.
        self.inv_tol = (tol != BAD_VALUE).then_some(tol);
    }

    /// How the original forward transformation (that of the SplineMap before
    /// any inversion) handles input positions outside the bounds of the
    /// knots.
    ///
    /// If false, [`BAD_VALUE`] is returned for both output axes at such
    /// points.
    /// If true, the forward transformation behaves like a unit Mapping there,
    /// copying the input axis values to the outputs.
    /// It also affects the value [`rate`](Mapping::rate) returns.
    ///
    /// Default: false.
    ///
    /// [`set_out_unit`](SplineMap::set_out_unit),
    /// [`test_out_unit`](SplineMap::test_out_unit) and
    /// [`clear_out_unit`](SplineMap::clear_out_unit) set, test and clear it.
    #[doc(alias = "OutUnit")]
    pub fn out_unit(&self) -> bool {
        self.out_unit.unwrap_or(false)
    }

    /// Set [`out_unit`](SplineMap::out_unit).
    ///
    /// A value different from the one held clears the record that the
    /// SplineMap has been simplified.
    pub fn set_out_unit(&mut self, flag: bool) {
        // `OutUnit` changes what the transformation returns outside the
        // spline's bounds (splinemap.c:2630-2632).
        if Some(flag) != self.out_unit {
            self.simplified.clear();
        }
        self.out_unit = Some(flag);
    }

    /// Whether [`inv_niter`](SplineMap::inv_niter) is set.
    pub fn test_inv_niter(&self) -> bool {
        self.inv_niter.is_some()
    }

    /// Clear [`inv_niter`](SplineMap::inv_niter), so that it reads as the
    /// default of 6.
    ///
    /// This also clears the record that the SplineMap has been simplified.
    pub fn clear_inv_niter(&mut self) {
        // splinemap.c ClearInvNiter.
        self.simplified.clear();
        self.inv_niter = None;
    }

    /// Whether [`inv_tol`](SplineMap::inv_tol) is set.
    pub fn test_inv_tol(&self) -> bool {
        self.inv_tol.is_some()
    }

    /// Clear [`inv_tol`](SplineMap::inv_tol), so that it reads as the default
    /// of 1.0e-6.
    ///
    /// This also clears the record that the SplineMap has been simplified.
    pub fn clear_inv_tol(&mut self) {
        // splinemap.c ClearInvTol.
        self.simplified.clear();
        self.inv_tol = None;
    }

    /// Whether [`out_unit`](SplineMap::out_unit) is set, to either value.
    pub fn test_out_unit(&self) -> bool {
        self.out_unit.is_some()
    }

    /// Clear [`out_unit`](SplineMap::out_unit), so that it reads as the
    /// default of false.
    ///
    /// This also clears the record that the SplineMap has been simplified.
    pub fn clear_out_unit(&mut self) {
        // splinemap.c ClearOutUnit.
        self.simplified.clear();
        self.out_unit = None;
    }

    /// Whether the effective forward direction is true forward.
    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    // -----------------------------------------------------------------------
    // B-spline evaluation (port of CMLIB Dbvalu / Db2val / Dintrv)
    // -----------------------------------------------------------------------

    /// Find the knot interval containing `x`.
    ///
    /// Computes the largest index `ileft` in [0, lxt-1] such that
    /// `xt[ileft] <= x`. Returns `(ileft, mflag)` where mflag indicates
    /// if x is out of bounds: -1 = below, 0 = in range, +1 = above.
    ///
    /// Port of CMLIB `Dintrv`. The `ilo` parameter is the initial search
    /// hint (set to 0 on first call).
    #[inline]
    fn dintrv(xt: &[f64], lxt: usize, x: f64, ilo: &mut usize) -> (usize, i32) {
        let xt = &xt[..lxt];
        let mut ihi = *ilo + 1;

        if ihi >= lxt - 1 {
            if x >= xt[lxt - 1] {
                // x is at or above the last knot
                *ilo = lxt - 1;
                return (lxt - 1, 1);
            }
            if lxt <= 1 {
                return (0, -1);
            }
            *ilo = lxt - 2;
            ihi = lxt - 1;
        }

        if x >= xt[ihi] {
            // x is above xt[ihi], search upward
            let mut istep = 1;
            loop {
                *ilo = ihi;
                ihi = *ilo + istep;
                if ihi >= lxt - 1 {
                    if x >= xt[lxt - 1] {
                        *ilo = lxt - 1;
                        return (lxt - 1, 1);
                    }
                    ihi = lxt - 1;
                    break;
                }
                if x < xt[ihi] {
                    break;
                }
                istep *= 2;
            }
        } else if x >= xt[*ilo] {
            // x is in [xt[ilo], xt[ihi])
            return (*ilo, 0);
        } else {
            // x is below xt[ilo], search downward
            let mut istep = 1;
            loop {
                ihi = *ilo;
                if *ilo <= istep {
                    *ilo = 0;
                    if x < xt[0] {
                        return (0, -1);
                    }
                    break;
                }
                *ilo = ihi - istep;
                if x >= xt[*ilo] {
                    break;
                }
                istep *= 2;
            }
        }

        // Binary search in [ilo, ihi]
        loop {
            let middle = (*ilo + ihi) / 2;
            if middle == *ilo {
                return (*ilo, 0);
            }
            if x < xt[middle] {
                ihi = middle;
            } else {
                *ilo = middle;
            }
        }
    }

    /// Evaluate a 1-D B-spline or its derivative at position `x`.
    ///
    /// Port of CMLIB `Dbvalu`.
    ///
    /// Parameters:
    /// - `t`: knot vector of length n + k
    /// - `a`: coefficient vector of length n
    /// - `n`: number of coefficients
    /// - `k`: order of the B-spline (1 to `MAX_K`)
    /// - `ideriv`: derivative order (0 = function value)
    /// - `x`: evaluation point
    /// - `inbv`: initial search hint (set to 0 for first call)
    ///
    /// Returns the value or BAD_VALUE if out of range.
    ///
    /// This runs several times per point, so each order has its own copy
    /// ([`Self::dbvalu_k`]) whose loops have fixed lengths; the arithmetic
    /// is C's, in C's order.
    fn dbvalu(t: &[f64], a: &[f64], n: usize, k: usize, ideriv: usize, x: f64, inbv: usize) -> f64 {
        match k {
            1 => Self::dbvalu_k::<1>(t, a, n, ideriv, x, inbv),
            2 => Self::dbvalu_k::<2>(t, a, n, ideriv, x, inbv),
            3 => Self::dbvalu_k::<3>(t, a, n, ideriv, x, inbv),
            4 => Self::dbvalu_k::<4>(t, a, n, ideriv, x, inbv),
            5 => Self::dbvalu_k::<5>(t, a, n, ideriv, x, inbv),
            6 => Self::dbvalu_k::<6>(t, a, n, ideriv, x, inbv),
            _ => BAD_VALUE,
        }
    }

    /// [`Self::dbvalu`] for order `K`. C's work array holds the coefficients,
    /// then `dp`, then `dm`, `K` values each; here they are three arrays.
    fn dbvalu_k<const K: usize>(
        t: &[f64],
        a: &[f64],
        n: usize,
        ideriv: usize,
        x: f64,
        inbv: usize,
    ) -> f64 {
        if n < K {
            return BAD_VALUE;
        }

        // A spline of order k is a piecewise polynomial of degree k-1, so a
        // derivative of order k or higher is zero within each piece; the
        // iterative inverse asks for it when taking the Jacobian of an
        // order-1 spline (splinemap.c `Dbvalu`).
        if ideriv >= K {
            return 0.0;
        }

        if x < t[K - 1] {
            return BAD_VALUE;
        }

        // Find i such that t[i] <= x < t[i+1]
        let mut inbv_local = inbv;
        let (mut i, mflag) = Self::dintrv(t, n + 1, x, &mut inbv_local);

        if mflag != 0 {
            if x > t[i] {
                return BAD_VALUE;
            }
            // Handle upper boundary
            while i != K - 1 {
                i -= 1;
                if x != t[i] {
                    break;
                }
            }
            // Note: Original C code comments out this check to allow upper edge.
            // if i == k - 1 { return BAD_VALUE; }
        }

        // The coefficients and knots the rest reads: a[i+1-K..=i] and
        // t[i+1-K..=i+K], so t[i+d] is tw[d+K-1].
        let mut aj: [f64; K] = a[i + 1 - K..=i].try_into().unwrap();
        let tw: &[f64; 2 * MAX_K] = &{
            let mut w = [0.0; 2 * MAX_K];
            w[..2 * K].copy_from_slice(&t[i + 1 - K..=i + K]);
            w
        };

        // Difference the coefficients ideriv times.
        for j in 1..=ideriv {
            let kmj = K - j;
            let fkmj = kmj as f64;
            for jj in 0..kmj {
                aj[jj] = (aj[jj + 1] - aj[jj]) / (tw[jj + K] - tw[jj + K - kmj]) * fkmj;
            }
        }

        // Compute value at x
        if ideriv != K - 1 {
            let mut dp = [0.0f64; K];
            let mut dm = [0.0f64; K];
            for j in 1..=K - ideriv {
                dp[j - 1] = tw[j + K - 1] - x;
                dm[j - 1] = x - tw[K - j];
            }
            for j in ideriv + 1..K {
                let kmj = K - j;
                for jj in 0..kmj {
                    let ilo = kmj - 1 - jj;
                    aj[jj] = (aj[jj + 1] * dm[ilo] + aj[jj] * dp[jj]) / (dm[ilo] + dp[jj]);
                }
            }
        }

        aj[0]
    }

    /// Evaluate a 2-D B-spline or its partial derivative at (xval, yval).
    ///
    /// Port of CMLIB `Db2val`.
    ///
    /// Parameters:
    /// - `def`: value to return if (xval, yval) is outside knot domain
    /// - `xval`, `yval`: evaluation point
    /// - `idx`, `idy`: derivative orders in x and y
    /// - `bcoef`: coefficient array (nx * ny)
    ///
    /// Returns the spline value or `def` if out of range.
    fn db2val(&self, def: f64, xval: f64, yval: f64, idx: usize, idy: usize, bcoef: &[f64]) -> f64 {
        let tx = &self.tx;
        let ty = &self.ty;
        let nx = self.nx;
        let ny = self.ny;
        let kx = self.kx;
        let ky = self.ky;

        // Check bounds
        if xval < tx[0] || xval > tx[nx + kx - 1] || yval < ty[0] || yval > ty[ny + ky - 1] {
            return def;
        }

        // Find y interval
        let mut iloy: usize = 0;
        let (lefty, mflag) = Self::dintrv(ty, ny + ky, yval, &mut iloy);

        // Handle upper Y boundary (DSB modification)
        let lefty = if mflag != 0 {
            if yval > ty[lefty] {
                return BAD_VALUE;
            }
            let mut lefty = lefty;
            while lefty != ky - 1 {
                lefty -= 1;
                if yval != ty[lefty] {
                    break;
                }
            }
            if lefty == ky - 1 {
                return BAD_VALUE;
            }
            lefty
        } else {
            lefty
        };

        // Evaluate the ky 1-D B-splines in x for each relevant row, giving
        // the coefficients of a 1-D B-spline in y.
        let mut ycoef = [0.0f64; MAX_K];
        let mut kcol = lefty as isize - ky as isize;
        for c in ycoef.iter_mut().take(ky) {
            kcol += 1;
            let coef_offset = nx * kcol as usize;
            *c = Self::dbvalu(tx, &bcoef[coef_offset..], nx, kx, idx, xval, 0);
        }

        // Now evaluate the resulting 1-D B-spline in y.
        let kcol_start = (lefty as isize - ky as isize + 1) as usize;
        Self::dbvalu(&ty[kcol_start..], &ycoef[..ky], ky, ky, idy, yval, 0)
    }

    /// Evaluate the forward transform for a single point.
    fn forward_point(&self, x: f64, y: f64) -> (f64, f64) {
        let def_u = if self.out_unit() { x } else { BAD_VALUE };
        let def_v = if self.out_unit() { y } else { BAD_VALUE };
        let u = self.db2val(def_u, x, y, 0, 0, &self.cu);
        let v = self.db2val(def_v, x, y, 0, 0, &self.cv);
        (u, v)
    }

    /// Evaluate the iterative inverse transform.
    ///
    /// Transforms output positions (u, v) back to input positions (x, y)
    /// using Newton-Raphson iteration.
    #[allow(clippy::needless_range_loop)]
    fn iter_inverse(&self, pts: &Points<'_>, out: &mut PointsMut<'_>) {
        let npoint = pts.npoint();
        let maxiter = self.inv_niter();
        let maxerr = self.inv_tol() * self.inv_tol(); // squared tolerance

        // Bounds of the knot domain (slightly shrunk to avoid edge issues)
        let xlo_orig = self.tx[0];
        let xhi_orig = self.tx[self.nx + self.kx - 1];
        let a = 1.0e-16 * (xhi_orig - xlo_orig);
        let xlo = xlo_orig + a;
        let xhi = xhi_orig - a;

        let ylo_orig = self.ty[0];
        let yhi_orig = self.ty[self.ny + self.ky - 1];
        let a = 1.0e-16 * (yhi_orig - ylo_orig);
        let ylo = ylo_orig + a;
        let yhi = yhi_orig - a;

        // Flags: true = converged (or bad)
        let mut flags = vec![false; npoint];
        let mut nconv = 0usize;

        let (tu, tv) = (&pts.axis(0)[..npoint], &pts.axis(1)[..npoint]);
        let (ox, oy) = out.axis_pair_mut(0, 1);
        let (ox, oy) = (&mut ox[..npoint], &mut oy[..npoint]);

        // Initial guess: x = 2*u - spline_u(u,v), y = 2*v - spline_v(u,v)
        for ipoint in 0..npoint {
            let u = tu[ipoint];
            let v = tv[ipoint];

            if is_bad(u) || is_bad(v) {
                ox[ipoint] = BAD_VALUE;
                oy[ipoint] = BAD_VALUE;
                flags[ipoint] = true;
                nconv += 1;
                continue;
            }

            let def_u = if self.out_unit() { u } else { BAD_VALUE };
            let def_v = if self.out_unit() { v } else { BAD_VALUE };

            let a_val = self.db2val(def_u, u, v, 0, 0, &self.cu);
            if !is_bad(u) && !is_bad(a_val) {
                ox[ipoint] = 2.0 * u - a_val;
            } else {
                ox[ipoint] = u;
            }

            let a_val = self.db2val(def_v, u, v, 0, 0, &self.cv);
            if !is_bad(v) && !is_bad(a_val) {
                oy[ipoint] = 2.0 * v - a_val;
            } else {
                oy[ipoint] = v;
            }

            // Check if initial guess is bad
            if is_bad(ox[ipoint]) || is_bad(oy[ipoint]) {
                ox[ipoint] = BAD_VALUE;
                oy[ipoint] = BAD_VALUE;
                flags[ipoint] = true;
                nconv += 1;
            }
        }

        // Newton-Raphson iterations
        let mut wu = vec![0.0; npoint];
        let mut wv = vec![0.0; npoint];

        for _iter in 0..maxiter {
            if nconv >= npoint {
                break;
            }

            // Clamp every guess to the knot domain, converged ones included,
            // as C does at the start of each iteration.
            for x in ox.iter_mut() {
                if !is_bad(*x) {
                    *x = x.clamp(xlo, xhi);
                }
            }
            for y in oy.iter_mut() {
                if !is_bad(*y) {
                    *y = y.clamp(ylo, yhi);
                }
            }

            // Forward-transform the current input guesses
            for ipoint in 0..npoint {
                let (x, y) = (ox[ipoint], oy[ipoint]);
                if is_bad(x) || is_bad(y) {
                    wu[ipoint] = BAD_VALUE;
                    wv[ipoint] = BAD_VALUE;
                } else {
                    (wu[ipoint], wv[ipoint]) = self.forward_point(x, y);
                }
            }

            // Compute D_out = target - forward(guess)
            for (w, &t) in wu.iter_mut().zip(tu) {
                *w = if !is_bad(t) && !is_bad(*w) {
                    t - *w
                } else {
                    BAD_VALUE
                };
            }
            for (w, &t) in wv.iter_mut().zip(tv) {
                *w = if !is_bad(t) && !is_bad(*w) {
                    t - *w
                } else {
                    BAD_VALUE
                };
            }

            // For each point, solve: Jacobian * D_in = D_out
            for ipoint in 0..npoint {
                if flags[ipoint] {
                    continue;
                }

                let outp_x = ox[ipoint];
                let outp_y = oy[ipoint];

                // Compute Jacobian at current guess
                let def_du_dx = if self.out_unit() { 1.0 } else { BAD_VALUE };
                let def_du_dy = if self.out_unit() { 0.0 } else { BAD_VALUE };
                let def_dv_dx = if self.out_unit() { 0.0 } else { BAD_VALUE };
                let def_dv_dy = if self.out_unit() { 1.0 } else { BAD_VALUE };

                let j00 = self.db2val(def_du_dx, outp_x, outp_y, 1, 0, &self.cu);
                let j01 = self.db2val(def_du_dy, outp_x, outp_y, 0, 1, &self.cu);
                let j10 = self.db2val(def_dv_dx, outp_x, outp_y, 1, 0, &self.cv);
                let j11 = self.db2val(def_dv_dy, outp_x, outp_y, 0, 1, &self.cv);

                let vec0 = wu[ipoint];
                let vec1 = wv[ipoint];

                // Solve 2x2 system: J * d = vec
                if is_bad(vec0)
                    || is_bad(vec1)
                    || is_bad(j00)
                    || is_bad(j01)
                    || is_bad(j10)
                    || is_bad(j11)
                {
                    // Singular or bad — mark as converged with BAD
                    ox[ipoint] = BAD_VALUE;
                    oy[ipoint] = BAD_VALUE;
                    flags[ipoint] = true;
                    nconv += 1;
                    continue;
                }

                // Solve J * d = vec with palDmat, as C does; a singular
                // matrix makes the point bad.
                let mut mat = [j00, j01, j10, j11];
                let mut vec = [vec0, vec1];
                if !crate::pal::dmat(2, &mut mat, &mut vec) {
                    ox[ipoint] = BAD_VALUE;
                    oy[ipoint] = BAD_VALUE;
                    flags[ipoint] = true;
                    nconv += 1;
                    continue;
                }
                let [dx, dy] = vec;

                // Update guess
                ox[ipoint] += dx;
                oy[ipoint] += dy;

                // Check convergence
                let xlensq = ox[ipoint] * ox[ipoint] + oy[ipoint] * oy[ipoint];
                let vlensq = dx * dx + dy * dy;
                if vlensq <= maxerr * xlensq {
                    flags[ipoint] = true;
                    nconv += 1;
                }
            }
        }

        // Mark unconverged points as BAD
        if nconv < npoint {
            for ipoint in 0..npoint {
                if !flags[ipoint] {
                    ox[ipoint] = BAD_VALUE;
                    oy[ipoint] = BAD_VALUE;
                }
            }
        }
    }
}

impl Mapping for SplineMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        2
    }

    fn nout(&self) -> usize {
        2
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    /// NaN-as-bad fast path. B-spline evaluation has knot-domain BAD
    /// emit sites in the inner `forward_point`/`inverse_point` routines
    /// (out-of-range coordinate, no monotonic interval, singular
    /// boundary). Delegate-and-post-process keeps the existing
    /// well-tested evaluator and translates `BAD_VALUE` outputs to
    /// `f64::NAN` so downstream NaN-aware stages see the protocol-
    /// canonical sentinel.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let npoint = pts.npoint();
        self.transform_into(pts, forward, out.reborrow())?;
        crate::mappings::bad_to_nan(self, forward, npoint, &mut out);
        Ok(())
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let fwd = self.effective_forward(forward);

        if fwd {
            // Forward: evaluate B-splines
            let n = pts.npoint();
            let (x, y) = (&pts.axis(0)[..n], &pts.axis(1)[..n]);
            let (ou, ov) = out.axis_pair_mut(0, 1);
            let (ou, ov) = (&mut ou[..n], &mut ov[..n]);
            for i in 0..n {
                if is_bad(x[i]) || is_bad(y[i]) {
                    ou[i] = BAD_VALUE;
                    ov[i] = BAD_VALUE;
                } else {
                    (ou[i], ov[i]) = self.forward_point(x[i], y[i]);
                }
            }
        } else {
            // Inverse: iterative Newton-Raphson
            self.iter_inverse(&pts, &mut out);
        }
        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C `splinemap.c::MapMerge` (1757) has one step and it is a neighbor
        // cancellation: a SplineMap adjacent to an equal SplineMap used in the
        // opposite direction becomes UnitMaps. There is no replace-with-simpler
        // step, so a nominated SplineMap goes straight to the pairwise merge,
        // which `SplineMap::try_merge` performs.
        super::simplify::default_map_merge(ctx, where_, series)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        // splinemap.c Rate: the splines give the forward transformation's
        // derivatives only, so an inverted SplineMap's rate, that of its
        // inverse, is found numerically by the parent method.
        if self.inverted {
            return numeric_rate(self, at, ax1, ax2);
        }
        if is_bad(at[0]) || is_bad(at[1]) {
            return Ok(BAD_VALUE);
        }
        // ax1, ax2 are 1-based
        let coef = if ax1 == 1 { &self.cu } else { &self.cv };
        let (xder, yder) = if ax2 == 1 { (1, 0) } else { (0, 1) };
        let def = if self.out_unit() {
            if ax1 == ax2 { 1.0 } else { 0.0 }
        } else {
            BAD_VALUE
        };
        Ok(self.db2val(def, at[0], at[1], xder, yder, coef))
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SplineMap>() else {
            return false;
        };
        // Invert flags must match (inverted SplineMap has no analytic forward)
        if self.inverted != other.inverted {
            return false;
        }
        if self.kx != other.kx || self.ky != other.ky || self.nx != other.nx || self.ny != other.ny
        {
            return false;
        }
        // Compare knots
        for i in 0..self.nx + self.kx {
            if !ast_equal(self.tx[i], other.tx[i]) {
                return false;
            }
        }
        for i in 0..self.ny + self.ky {
            if !ast_equal(self.ty[i], other.ty[i]) {
                return false;
            }
        }
        // Compare coefficients
        let nc = self.nx * self.ny;
        for i in 0..nc {
            if !ast_equal(self.cu[i], other.cu[i]) {
                return false;
            }
            if !ast_equal(self.cv[i], other.cv[i]) {
                return false;
            }
        }
        true
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

        // Inverse cancellation: SplineMap + inverse SplineMap → UnitMap
        if let Some(other_sp) = other.as_any().downcast_ref::<SplineMap>() {
            // They must be used in opposite directions
            if self_inv == other_inv {
                return None;
            }
            // Check equality (comparing stored fields, ignoring inverted flag)
            if self.kx != other_sp.kx
                || self.ky != other_sp.ky
                || self.nx != other_sp.nx
                || self.ny != other_sp.ny
            {
                return None;
            }
            for i in 0..self.nx + self.kx {
                if !ast_equal(self.tx[i], other_sp.tx[i]) {
                    return None;
                }
            }
            for i in 0..self.ny + self.ky {
                if !ast_equal(self.ty[i], other_sp.ty[i]) {
                    return None;
                }
            }
            let nc = self.nx * self.ny;
            for i in 0..nc {
                if !ast_equal(self.cu[i], other_sp.cu[i]) {
                    return None;
                }
                if !ast_equal(self.cv[i], other_sp.cv[i]) {
                    return None;
                }
            }
            return Some(Box::new(UnitMap::new(2)));
        }

        None
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

impl Serializable for SplineMap {
    fn class_name(&self) -> &'static str {
        "SplineMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Map using a bi-cubic spline".to_string());
        write_mapping_base(&mut node, self);

        // Order and dimensions
        node.push_int(
            "KX",
            self.kx as i64,
            Some("Polynomial order in X direction".to_string()),
        );
        node.push_int(
            "KY",
            self.ky as i64,
            Some("Polynomial order in Y direction".to_string()),
        );
        node.push_int(
            "NX",
            self.nx as i64,
            Some("No. of coeffs in X direction".to_string()),
        );
        node.push_int(
            "NY",
            self.ny as i64,
            Some("No. of coeffs in Y direction".to_string()),
        );

        // Knots
        for i in 0..self.nx + self.kx {
            node.push_double(
                &format!("TX{}", i + 1),
                self.tx[i],
                Some(format!("Knot {} x position", i + 1)),
            );
        }
        for i in 0..self.ny + self.ky {
            node.push_double(
                &format!("TY{}", i + 1),
                self.ty[i],
                Some(format!("Knot {} y position", i + 1)),
            );
        }

        // Coefficients
        let nc = self.nx * self.ny;
        for i in 0..nc {
            node.push_double(
                &format!("CU{}", i + 1),
                self.cu[i],
                Some(format!("X coeff {}", i + 1)),
            );
        }
        for i in 0..nc {
            node.push_double(
                &format!("CV{}", i + 1),
                self.cv[i],
                Some(format!("Y coeff {}", i + 1)),
            );
        }

        // The iterative inverse and out-of-bounds attributes, active when
        // set and otherwise the defaults their getters give (splinemap.c
        // `Dump`).
        node.write_int(
            "NiterInv",
            self.inv_niter.is_some(),
            false,
            self.inv_niter() as i64,
            "Max number of iterations for iterative inverse",
        );
        node.write_int(
            "OutUnit",
            self.out_unit.is_some(),
            false,
            i64::from(self.out_unit()),
            "How to handle out-of-bounds points",
        );
        node.write_double(
            "TolInv",
            self.inv_tol.is_some(),
            false,
            self.inv_tol(),
            "Target relative error for iterative inverse",
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("SplineMap");
        let base = read_mapping_base(whole);

        let kx = node.get_int("KX", 0) as usize;
        let ky = node.get_int("KY", 0) as usize;
        let nx = node.get_int("NX", 0) as usize;
        let ny = node.get_int("NY", 0) as usize;

        let tx: Vec<f64> = (0..nx + kx)
            .map(|i| node.get_double(&format!("TX{}", i + 1), BAD_VALUE))
            .collect();

        let ty: Vec<f64> = (0..ny + ky)
            .map(|i| node.get_double(&format!("TY{}", i + 1), BAD_VALUE))
            .collect();

        let nc = nx * ny;
        let cu: Vec<f64> = (0..nc)
            .map(|i| node.get_double(&format!("CU{}", i + 1), BAD_VALUE))
            .collect();

        let cv: Vec<f64> = (0..nc)
            .map(|i| node.get_double(&format!("CV{}", i + 1), BAD_VALUE))
            .collect();

        // Read optional attributes
        let inv_niter_raw = node.get_int("NiterInv", i64::MIN);
        let inv_niter = if inv_niter_raw == i64::MIN {
            None
        } else {
            Some(inv_niter_raw.max(0) as usize)
        };

        let out_unit_raw = node.get_int("OutUnit", i64::MIN);
        let out_unit = (out_unit_raw != i64::MIN).then_some(out_unit_raw != 0);

        let inv_tol_raw = node.get_double("TolInv", BAD_VALUE);
        let inv_tol = if is_bad(inv_tol_raw) {
            None
        } else {
            Some(inv_tol_raw)
        };

        let mut m = SplineMap {
            kx,
            ky,
            nx,
            ny,
            tx,
            ty,
            cu,
            cv,
            inv_niter,
            inv_tol,
            out_unit,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        };

        if base.inverted {
            m.invert();
        }
        m.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        // C assigns the structure component and only then re-applies it through
        // the setter, so the set changes nothing and the setter's guarded
        // `astClearIsSimple` does not fire (splinemap.c:3643-3652): the struct literal
        // above already holds the values read from the cards.
        if let Some(n) = inv_niter {
            m.set_inv_niter(n);
        }
        if let Some(tol) = inv_tol {
            m.set_inv_tol(tol);
        }
        if let Some(out_unit) = out_unit {
            m.set_out_unit(out_unit);
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A derivative of the spline's order or higher is zero, not bad
    /// (splinemap.c `Dbvalu`): an order-1 spline is constant on each piece.
    #[test]
    fn dbvalu_high_order_derivative_is_zero() {
        let t = [0.0, 1.0];
        let a = [1.5];
        assert_eq!(SplineMap::dbvalu(&t, &a, 1, 1, 0, 0.5, 0), 1.5);
        assert_eq!(SplineMap::dbvalu(&t, &a, 1, 1, 1, 0.5, 0), 0.0);
    }
}

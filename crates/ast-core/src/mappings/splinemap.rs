//! 2-D mapping using bivariate B-splines.
//!
//! A `SplineMap` transforms 2-dimensional positions using a pair of
//! two-dimensional B-splines. Each output coordinate is the value of
//! one of these two bivariate splines evaluated at the input position.
//! The two splines share the same knot positions and order but have
//! independently specified coefficients.
//!
//! The inverse is evaluated using a Newton-Raphson iteration that
//! assumes the forward transformation represents a small perturbation
//! of the identity: u = x + Pu(x,y), v = y + Pv(x,y).

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
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

/// A 2-D mapping using a pair of bivariate B-splines.
///
/// The forward transformation evaluates two bivariate B-splines at
/// each input (x, y) position to produce output (u, v):
///
/// ```text
/// u = spline_u(x, y)
/// v = spline_v(x, y)
/// ```
///
/// The inverse uses a Newton-Raphson iteration, assuming the
/// transformation is a small perturbation of the identity.
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
    /// How to handle out-of-bounds: false = BAD, true = unit (identity).
    out_unit: bool,
    /// Whether the mapping is inverted.
    inverted: bool,
    /// Whether this mapping has already been simplified (C `IsSimple`).
    simplified: SimplifiedRecord,
    ident: Option<String>,
}

impl SplineMap {
    /// Create a new `SplineMap`.
    ///
    /// # Parameters
    ///
    /// - `kx`: Order of the spline in X direction (1..=6, where 4 = cubic).
    /// - `ky`: Order of the spline in Y direction (1..=6, where 4 = cubic).
    /// - `nx`: Number of B-spline coefficients in X direction (>= kx).
    /// - `ny`: Number of B-spline coefficients in Y direction (>= ky).
    /// - `tx`: Knot positions in X direction (length nx + kx), non-decreasing.
    /// - `ty`: Knot positions in Y direction (length ny + ky), non-decreasing.
    /// - `cu`: Coefficients for the first output (length nx * ny).
    ///   Stored internally in Fortran/CMLIB order (column-major over
    ///   the X index). The caller supplies them in this (CMLIB/Fortran)
    ///   order.
    /// - `cv`: Coefficients for the second output (length nx * ny).
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
    ) -> Self {
        assert!((1..=MAX_K).contains(&kx), "kx must be in [1, {MAX_K}]");
        assert!((1..=MAX_K).contains(&ky), "ky must be in [1, {MAX_K}]");
        assert!(nx >= kx, "nx must be >= kx");
        assert!(ny >= ky, "ny must be >= ky");
        assert_eq!(tx.len(), nx + kx, "tx must have length nx + kx");
        assert_eq!(ty.len(), ny + ky, "ty must have length ny + ky");
        assert_eq!(cu.len(), nx * ny, "cu must have length nx * ny");
        assert_eq!(cv.len(), nx * ny, "cv must have length nx * ny");

        Self {
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
            out_unit: false,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
        }
    }

    /// The spline order in the X direction.
    pub fn kx(&self) -> usize {
        self.kx
    }

    /// The spline order in the Y direction.
    pub fn ky(&self) -> usize {
        self.ky
    }

    /// Number of coefficients in X direction.
    pub fn nx(&self) -> usize {
        self.nx
    }

    /// Number of coefficients in Y direction.
    pub fn ny(&self) -> usize {
        self.ny
    }

    /// Knot positions in X direction.
    pub fn tx(&self) -> &[f64] {
        &self.tx
    }

    /// Knot positions in Y direction.
    pub fn ty(&self) -> &[f64] {
        &self.ty
    }

    /// Coefficients for the first (u) output.
    pub fn cu(&self) -> &[f64] {
        &self.cu
    }

    /// Coefficients for the second (v) output.
    pub fn cv(&self) -> &[f64] {
        &self.cv
    }

    /// Maximum iterations for the iterative inverse.
    pub fn inv_niter(&self) -> usize {
        self.inv_niter.unwrap_or(DEFAULT_INV_NITER)
    }

    /// Set the maximum number of iterations for the iterative inverse.
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

    /// Target relative error for the iterative inverse.
    pub fn inv_tol(&self) -> f64 {
        self.inv_tol.unwrap_or(DEFAULT_INV_TOL)
    }

    /// Set the target relative error for the iterative inverse.
    pub fn set_inv_tol(&mut self, tol: f64) {
        // As for `set_inv_niter` (splinemap.c:2586-2588).
        if Some(tol) != self.inv_tol {
            self.simplified.clear();
        }
        self.inv_tol = Some(tol);
    }

    /// Whether out-of-bounds inputs are handled as unit (identity) mapping.
    pub fn out_unit(&self) -> bool {
        self.out_unit
    }

    /// Set how out-of-bounds inputs are handled.
    ///
    /// If `true`, out-of-bounds inputs pass through unchanged (unit mapping).
    /// If `false` (default), out-of-bounds inputs produce BAD_VALUE.
    pub fn set_out_unit(&mut self, flag: bool) {
        // `OutUnit` changes what the transformation returns outside the
        // spline's bounds (splinemap.c:2630-2632).
        if flag != self.out_unit {
            self.simplified.clear();
        }
        self.out_unit = flag;
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
    fn dintrv(xt: &[f64], lxt: usize, x: f64, ilo: &mut usize) -> (usize, i32) {
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
    /// - `k`: order of the B-spline (>= 1)
    /// - `ideriv`: derivative order (0 = function value)
    /// - `x`: evaluation point
    /// - `inbv`: initial search hint (set to 0 for first call)
    ///
    /// Returns the value or BAD_VALUE if out of range.
    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    fn dbvalu(
        t: &[f64],
        a: &[f64],
        n: usize,
        k: usize,
        ideriv: usize,
        x: f64,
        inbv: usize,
        work: &mut [f64],
    ) -> f64 {
        if k < 1 || n < k || ideriv >= k {
            return BAD_VALUE;
        }

        if x < t[k - 1] {
            return BAD_VALUE;
        }

        let km1 = k - 1;
        let kmider = k - ideriv;

        // Find i such that t[i] <= x < t[i+1]
        let mut inbv_local = inbv;
        let (mut i, mflag) = Self::dintrv(t, n + 1, x, &mut inbv_local);

        if mflag != 0 {
            if x > t[i] {
                return BAD_VALUE;
            }
            // Handle upper boundary
            while i != k - 1 {
                i -= 1;
                if x != t[i] {
                    break;
                }
            }
            // Note: Original C code comments out this check to allow upper edge.
            // if i == k - 1 { return BAD_VALUE; }
        }

        // Difference the coefficients ideriv times.
        // work[j] = aj, work[k+j] = dp[j], work[2k+j] = dm[j]
        let imk = i as isize - k as isize + 1;
        for j in 0..k {
            let imkpj = (imk + j as isize) as usize;
            work[j] = a[imkpj];
        }

        if ideriv != 0 {
            for j in 1..=ideriv {
                let kmj = k - j;
                let fkmj = kmj as f64;
                for jj in 0..kmj {
                    let ihi = i + jj + 1;
                    let ihmkmj = ihi - kmj;
                    work[jj] = (work[jj + 1] - work[jj]) / (t[ihi] - t[ihmkmj]) * fkmj;
                }
            }
        }

        // Compute value at x
        if ideriv != km1 {
            let ip1 = i + 1;
            let kpk = k + k;

            for (jj, j) in (1..=kmider).enumerate() {
                let j1 = k + jj;
                let j2 = kpk + jj;
                let ipj = i + j;
                work[j1] = t[ipj] - x;
                let ip1mj = ip1 - j;
                work[j2] = x - t[ip1mj];
            }

            let iderp1 = ideriv + 1;
            for j in iderp1..=km1 {
                let kmj = k - j;
                let mut ilo_idx = kmj - 1;
                for jj in 0..kmj {
                    work[jj] = (work[jj + 1] * work[kpk + ilo_idx] + work[jj] * work[k + jj])
                        / (work[kpk + ilo_idx] + work[k + jj]);
                    ilo_idx = ilo_idx.saturating_sub(1);
                }
            }
        }

        work[0]
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

        // Work array: size 3*max(kx,ky) + ky
        let max_k = kx.max(ky);
        let mut work = vec![0.0f64; 3 * max_k + ky];
        let iw = ky;

        // Evaluate the ky 1-D B-splines in x for each relevant row
        let mut kcol = lefty as isize - ky as isize;
        for k_idx in 0..ky {
            kcol += 1;
            let coef_offset = nx * kcol as usize;
            work[k_idx] = Self::dbvalu(
                tx,
                &bcoef[coef_offset..],
                nx,
                kx,
                idx,
                xval,
                0,
                &mut work[iw..],
            );
        }

        // Now evaluate the resulting 1-D B-spline in y.
        // We need the first ky elements as coefficients and iw.. as scratch.
        // Since iw == ky, these regions don't overlap, but Rust borrow checker
        // doesn't know that. Copy the coefficients to a separate buffer.
        let kcol_start = (lefty as isize - ky as isize + 1) as usize;
        let mut y_coeffs = [0.0f64; MAX_K];
        y_coeffs[..ky].copy_from_slice(&work[..ky]);
        Self::dbvalu(
            &ty[kcol_start..],
            &y_coeffs[..ky],
            ky,
            ky,
            idy,
            yval,
            0,
            &mut work[iw..],
        )
    }

    /// Evaluate the forward transform for a single point.
    fn forward_point(&self, x: f64, y: f64) -> (f64, f64) {
        let def_u = if self.out_unit { x } else { BAD_VALUE };
        let def_v = if self.out_unit { y } else { BAD_VALUE };
        let u = self.db2val(def_u, x, y, 0, 0, &self.cu);
        let v = self.db2val(def_v, x, y, 0, 0, &self.cv);
        (u, v)
    }

    /// Evaluate the iterative inverse transform.
    ///
    /// Transforms output positions (u, v) back to input positions (x, y)
    /// using Newton-Raphson iteration.
    #[allow(clippy::needless_range_loop)]
    fn iter_inverse(&self, pts: &PointSet, out: &mut PointSet) {
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

        // Initial guess: x = 2*u - spline_u(u,v), y = 2*v - spline_v(u,v)
        for ipoint in 0..npoint {
            let inp = pts.point(ipoint);
            let u = inp[0];
            let v = inp[1];
            let outp = out.point_mut(ipoint);

            if is_bad(u) || is_bad(v) {
                outp[0] = BAD_VALUE;
                outp[1] = BAD_VALUE;
                flags[ipoint] = true;
                nconv += 1;
                continue;
            }

            let def_u = if self.out_unit { u } else { BAD_VALUE };
            let def_v = if self.out_unit { v } else { BAD_VALUE };

            let a_val = self.db2val(def_u, u, v, 0, 0, &self.cu);
            if !is_bad(u) && !is_bad(a_val) {
                outp[0] = 2.0 * u - a_val;
            } else {
                outp[0] = u;
            }

            let a_val = self.db2val(def_v, u, v, 0, 0, &self.cv);
            if !is_bad(v) && !is_bad(a_val) {
                outp[1] = 2.0 * v - a_val;
            } else {
                outp[1] = v;
            }

            // Check if initial guess is bad
            if is_bad(outp[0]) || is_bad(outp[1]) {
                outp[0] = BAD_VALUE;
                outp[1] = BAD_VALUE;
                flags[ipoint] = true;
                nconv += 1;
            }
        }

        // Newton-Raphson iterations
        let mut work_pts = PointSet::zeros(npoint, 2);

        for _iter in 0..maxiter {
            if nconv >= npoint {
                break;
            }

            // Clamp current guesses to knot domain
            for ipoint in 0..npoint {
                if flags[ipoint] {
                    continue;
                }
                let outp = out.point_mut(ipoint);
                if !is_bad(outp[0]) {
                    outp[0] = outp[0].clamp(xlo, xhi);
                }
                if !is_bad(outp[1]) {
                    outp[1] = outp[1].clamp(ylo, yhi);
                }
            }

            // Forward-transform the current input guesses
            for ipoint in 0..npoint {
                let guess = out.point(ipoint);
                let wp = work_pts.point_mut(ipoint);
                if is_bad(guess[0]) || is_bad(guess[1]) {
                    wp[0] = BAD_VALUE;
                    wp[1] = BAD_VALUE;
                } else {
                    let (u, v) = self.forward_point(guess[0], guess[1]);
                    wp[0] = u;
                    wp[1] = v;
                }
            }

            // Compute D_out = target - forward(guess)
            for ipoint in 0..npoint {
                let target = pts.point(ipoint);
                let wp = work_pts.point_mut(ipoint);
                if !is_bad(target[0]) && !is_bad(wp[0]) {
                    wp[0] = target[0] - wp[0];
                } else {
                    wp[0] = BAD_VALUE;
                }
                if !is_bad(target[1]) && !is_bad(wp[1]) {
                    wp[1] = target[1] - wp[1];
                } else {
                    wp[1] = BAD_VALUE;
                }
            }

            // For each point, solve: Jacobian * D_in = D_out
            for ipoint in 0..npoint {
                if flags[ipoint] {
                    continue;
                }

                let outp_x;
                let outp_y;
                {
                    let outp = out.point(ipoint);
                    outp_x = outp[0];
                    outp_y = outp[1];
                }

                // Compute Jacobian at current guess
                let def_du_dx = if self.out_unit { 1.0 } else { BAD_VALUE };
                let def_du_dy = if self.out_unit { 0.0 } else { BAD_VALUE };
                let def_dv_dx = if self.out_unit { 0.0 } else { BAD_VALUE };
                let def_dv_dy = if self.out_unit { 1.0 } else { BAD_VALUE };

                let j00 = self.db2val(def_du_dx, outp_x, outp_y, 1, 0, &self.cu);
                let j01 = self.db2val(def_du_dy, outp_x, outp_y, 0, 1, &self.cu);
                let j10 = self.db2val(def_dv_dx, outp_x, outp_y, 1, 0, &self.cv);
                let j11 = self.db2val(def_dv_dy, outp_x, outp_y, 0, 1, &self.cv);

                let vec0 = work_pts.point(ipoint)[0];
                let vec1 = work_pts.point(ipoint)[1];

                // Solve 2x2 system: J * d = vec
                if is_bad(vec0)
                    || is_bad(vec1)
                    || is_bad(j00)
                    || is_bad(j01)
                    || is_bad(j10)
                    || is_bad(j11)
                {
                    // Singular or bad — mark as converged with BAD
                    let outp = out.point_mut(ipoint);
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                    flags[ipoint] = true;
                    nconv += 1;
                    continue;
                }

                let det = j00 * j11 - j01 * j10;
                if det.abs() < 1.0e-300 {
                    // Singular matrix
                    let outp = out.point_mut(ipoint);
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                    flags[ipoint] = true;
                    nconv += 1;
                    continue;
                }

                // Solve: d = J^{-1} * vec
                let dx = (j11 * vec0 - j01 * vec1) / det;
                let dy = (j00 * vec1 - j10 * vec0) / det;

                // Update guess
                let outp = out.point_mut(ipoint);
                outp[0] += dx;
                outp[1] += dy;

                // Check convergence
                let xlensq = outp[0] * outp[0] + outp[1] * outp[1];
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
                    let outp = out.point_mut(ipoint);
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                }
            }
        }
    }
}

impl Mapping for SplineMap {
    fn nin(&self) -> usize {
        2
    }

    fn nout(&self) -> usize {
        2
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), 2);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
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
        pts: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        self.transform_into(pts, forward, out)?;
        let mut view = out.view_mut();
        let flat = view.as_slice_mut().expect("PointSet is C-contiguous");
        for v in flat.iter_mut() {
            if *v == BAD_VALUE {
                *v = f64::NAN;
            }
        }
        Ok(())
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        if fwd {
            // Forward: evaluate B-splines
            for i in 0..pts.npoint() {
                let inp = pts.point(i);
                let outp = out.point_mut(i);
                if is_bad(inp[0]) || is_bad(inp[1]) {
                    outp[0] = BAD_VALUE;
                    outp[1] = BAD_VALUE;
                } else {
                    let (u, v) = self.forward_point(inp[0], inp[1]);
                    outp[0] = u;
                    outp[1] = v;
                }
            }
        } else {
            // Inverse: iterative Newton-Raphson
            self.iter_inverse(pts, out);
        }
        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C `splinemap.c::MapMerge` (1757) has one step and it is a neighbour
        // cancellation: a SplineMap adjacent to an equal SplineMap used in the
        // opposite direction becomes UnitMaps. There is no replace-with-simpler
        // step, so a nominated SplineMap goes straight to the pairwise merge,
        // which `SplineMap::try_merge` performs.
        super::simplify::default_map_merge(ctx, where_, series)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        // Use analytic derivatives from the B-spline when possible
        if !self.inverted && !is_bad(at[0]) && !is_bad(at[1]) {
            // ax1, ax2 are 1-based
            let coef = if ax1 == 1 { &self.cu } else { &self.cv };
            let (xder, yder) = if ax2 == 1 { (1, 0) } else { (0, 1) };
            let def = if self.out_unit {
                if ax1 == ax2 { 1.0 } else { 0.0 }
            } else {
                BAD_VALUE
            };
            let result = self.db2val(def, at[0], at[1], xder, yder, coef);
            if !is_bad(result) {
                return Ok(result);
            }
        }
        // Fall back to numeric rate for inverted or out-of-range
        numeric_rate(self, at, ax1, ax2)
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

        // Optional attributes (written only when set)
        let inv_niter_set = self.inv_niter.is_some();
        let inv_niter_val = self.inv_niter() as i64;
        if inv_niter_set {
            node.push_int(
                "NiterInv",
                inv_niter_val,
                Some("Max number of iterations for iterative inverse".to_string()),
            );
        }

        let out_unit_set = self.out_unit; // default is false
        if out_unit_set {
            node.push_int(
                "OutUnit",
                if self.out_unit { 1 } else { 0 },
                Some("How to handle out-of-bounds points".to_string()),
            );
        }

        let inv_tol_set = self.inv_tol.is_some();
        if inv_tol_set {
            node.push_double(
                "TolInv",
                self.inv_tol(),
                Some("Target relative error for iterative inverse".to_string()),
            );
        }

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
            Some(inv_niter_raw as usize)
        };

        let out_unit_raw = node.get_int("OutUnit", i64::MIN);
        let out_unit = if out_unit_raw == i64::MIN {
            false
        } else {
            out_unit_raw != 0
        };

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
        if node.get("OutUnit").is_some() {
            m.set_out_unit(out_unit);
        }
        Ok(m)
    }
}

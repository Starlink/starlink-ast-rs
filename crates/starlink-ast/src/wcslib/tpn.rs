// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! TPN: the gnomonic projection with polynomial correction terms.
//!
//! The projection is AST's own addition to its fork of wcslib, which uses it
//! to implement the IRAF TNX projection and the DSS plate solution encoding.
//! It is no longer part of the FITS-WCS standard.
//! `prm.p` holds the coefficients of the latitude-axis polynomial and
//! `prm.p2` those of the longitude axis; `prm.n` says whether the TAN
//! projection is applied as well as the polynomial.
//!
//! The bounds tests apply unconditionally, as [`super::prj`] explains.

// Lineage: every function lists its source in `reference/starlink-ast` at
// 3d3f255f; the `prj->flag > 0` tests of the C bodies are applied
// unconditionally.

// The bodies keep the shape of the C they are ported from.
#![allow(clippy::needless_late_init, clippy::too_many_lines)]

use super::math;

use super::WCSLIB_MXPAR;
use super::prj::{PrjPrm, Status};
use super::trig::{R2D, Trig, sincosd_covers};

// Newton's method in `astTPNfwd` stops when both corrections are within this
// fraction of the new coordinate (the `tol` of `reference/starlink-ast/wcslib/tpn.c`).
pub(super) const NEWTON_RELATIVE_TOLERANCE: f64 = 1.0e-5;

// The most iterations `astTPNfwd` makes before it gives up (the loop bound of
// `reference/starlink-ast/wcslib/tpn.c`).
pub(super) const NEWTON_MAX_ITERATIONS: usize = 50;

/// The powers and products of a point (x, y) the polynomial terms use.
// In the names and with the arithmetic of `tpn.c`.
struct Terms {
    x: f64,
    y: f64,
    x2: f64,
    xy: f64,
    y2: f64,
    r2: f64,
    r: f64,
    x3: f64,
    x2y: f64,
    xy2: f64,
    y3: f64,
    r3: f64,
    x4: f64,
    x3y: f64,
    x2y2: f64,
    xy3: f64,
    y4: f64,
    x5: f64,
    x4y: f64,
    x3y2: f64,
    x2y3: f64,
    xy4: f64,
    y5: f64,
    r5: f64,
    x6: f64,
    x5y: f64,
    x4y2: f64,
    x3y3: f64,
    x2y4: f64,
    xy5: f64,
    y6: f64,
    x7: f64,
    x6y: f64,
    x5y2: f64,
    x4y3: f64,
    x3y4: f64,
    x2y5: f64,
    xy6: f64,
    y7: f64,
    r7: f64,
}

impl Terms {
    #[inline(always)]
    fn new(x: f64, y: f64) -> Terms {
        let x2 = x * x;
        let xy = x * y;
        let y2 = y * y;
        let r2 = x2 + y2;
        let r = math::sqrt(r2);
        let x3 = x2 * x;
        let x2y = x2 * y;
        let xy2 = x * y2;
        let y3 = y * y2;
        let r3 = r * r2;
        let x4 = x3 * x;
        let x3y = x3 * y;
        let x2y2 = x2 * y2;
        let xy3 = x * y3;
        let y4 = y * y3;
        let x5 = x4 * x;
        let x4y = x4 * y;
        let x3y2 = x3 * y2;
        let x2y3 = x2 * y3;
        let xy4 = x * y4;
        let y5 = y * y4;
        let r5 = r3 * r2;
        let x6 = x5 * x;
        let x5y = x5 * y;
        let x4y2 = x4 * y2;
        let x3y3 = x3 * y3;
        let x2y4 = x2 * y4;
        let xy5 = x * y5;
        let y6 = y * y5;
        let x7 = x6 * x;
        let x6y = x6 * y;
        let x5y2 = x5 * y2;
        let x4y3 = x4 * y3;
        let x3y4 = x3 * y4;
        let x2y5 = x2 * y5;
        let xy6 = x * y6;
        let y7 = y * y6;
        let r7 = r5 * r2;
        Terms {
            x,
            y,
            x2,
            xy,
            y2,
            r2,
            r,
            x3,
            x2y,
            xy2,
            y3,
            r3,
            x4,
            x3y,
            x2y2,
            xy3,
            y4,
            x5,
            x4y,
            x3y2,
            x2y3,
            xy4,
            y5,
            r5,
            x6,
            x5y,
            x4y2,
            x3y3,
            x2y4,
            xy5,
            y6,
            x7,
            x6y,
            x5y2,
            x4y3,
            x3y4,
            x2y5,
            xy6,
            y7,
            r7,
        }
    }

    /// The longitude-axis polynomial in `a` (C's `f` and `xi`).
    #[inline(always)]
    #[allow(unused_variables)]
    fn xi(&self, a: &[f64]) -> f64 {
        let Terms {
            x,
            y,
            x2,
            xy,
            y2,
            r2,
            r,
            x3,
            x2y,
            xy2,
            y3,
            r3,
            x4,
            x3y,
            x2y2,
            xy3,
            y4,
            x5,
            x4y,
            x3y2,
            x2y3,
            xy4,
            y5,
            r5,
            x6,
            x5y,
            x4y2,
            x3y3,
            x2y4,
            xy5,
            y6,
            x7,
            x6y,
            x5y2,
            x4y3,
            x3y4,
            x2y5,
            xy6,
            y7,
            r7,
        } = *self;
        a[0] + a[1] * x
            + a[2] * y
            + a[3] * r
            + a[4] * x2
            + a[5] * xy
            + a[6] * y2
            + a[7] * x3
            + a[8] * x2y
            + a[9] * xy2
            + a[10] * y3
            + a[11] * r3
            + a[12] * x4
            + a[13] * x3y
            + a[14] * x2y2
            + a[15] * xy3
            + a[16] * y4
            + a[17] * x5
            + a[18] * x4y
            + a[19] * x3y2
            + a[20] * x2y3
            + a[21] * xy4
            + a[22] * y5
            + a[23] * r5
            + a[24] * x6
            + a[25] * x5y
            + a[26] * x4y2
            + a[27] * x3y3
            + a[28] * x2y4
            + a[29] * xy5
            + a[30] * y6
            + a[31] * x7
            + a[32] * x6y
            + a[33] * x5y2
            + a[34] * x4y3
            + a[35] * x3y4
            + a[36] * x2y5
            + a[37] * xy6
            + a[38] * y7
            + a[39] * r7
    }

    /// The latitude-axis polynomial in `b` (C's `g` and `eta`).
    #[inline(always)]
    #[allow(unused_variables)]
    fn eta(&self, b: &[f64]) -> f64 {
        let Terms {
            x,
            y,
            x2,
            xy,
            y2,
            r2,
            r,
            x3,
            x2y,
            xy2,
            y3,
            r3,
            x4,
            x3y,
            x2y2,
            xy3,
            y4,
            x5,
            x4y,
            x3y2,
            x2y3,
            xy4,
            y5,
            r5,
            x6,
            x5y,
            x4y2,
            x3y3,
            x2y4,
            xy5,
            y6,
            x7,
            x6y,
            x5y2,
            x4y3,
            x3y4,
            x2y5,
            xy6,
            y7,
            r7,
        } = *self;
        b[0] + b[1] * y
            + b[2] * x
            + b[3] * r
            + b[4] * y2
            + b[5] * xy
            + b[6] * x2
            + b[7] * y3
            + b[8] * xy2
            + b[9] * x2y
            + b[10] * x3
            + b[11] * r3
            + b[12] * y4
            + b[13] * xy3
            + b[14] * x2y2
            + b[15] * x3y
            + b[16] * x4
            + b[17] * y5
            + b[18] * xy4
            + b[19] * x2y3
            + b[20] * x3y2
            + b[21] * x4y
            + b[22] * x5
            + b[23] * r5
            + b[24] * y6
            + b[25] * xy5
            + b[26] * x2y4
            + b[27] * x3y3
            + b[28] * x4y2
            + b[29] * x5y
            + b[30] * x6
            + b[31] * y7
            + b[32] * xy6
            + b[33] * x2y5
            + b[34] * x3y4
            + b[35] * x4y3
            + b[36] * x5y2
            + b[37] * x6y
            + b[38] * x7
            + b[39] * r7
    }
}

// reference/starlink-ast/wcslib/tpn.c, astTPNset; reference/starlink-ast at 3d3f255f.
// The branches keep the shape of the C.
#[allow(clippy::if_same_then_else)]
pub(super) fn tpn_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    // If all coefficients have their "unit" values, the polynomial
    // correction is not needed.
    prm.w[0] = 0.0;

    if prm.p[0] != 0.0 || prm.p2[0] != 0.0 {
        prm.w[0] = 1.0;
    } else if prm.p[1] != 1.0 || prm.p2[1] != 1.0 {
        prm.w[0] = 1.0;
    } else {
        for m in 2..WCSLIB_MXPAR {
            if prm.p[m] != 0.0 || prm.p2[m] != 0.0 {
                prm.w[0] = 1.0;
                break;
            }
        }
    }

    // Not in C: the magnitudes of the coefficients summed by degree, w[1..9]
    // for the longitude axis and w[9..17] for the latitude axis, which the
    // slice entry uses to bound the rounding of the polynomials
    // (`tpn_fwd_step_noise`).
    let lon = degree_magnitude_sums(&prm.p2);
    let lat = degree_magnitude_sums(&prm.p);
    prm.w[1..9].copy_from_slice(&lon);
    prm.w[9..17].copy_from_slice(&lat);

    Status::Ok
}

// reference/starlink-ast/wcslib/tpn.c, astTPNfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn tpn_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let (status, xi, eta) = tpn_fwd_front::<T>(prm, phi, theta);
    if status != Status::Ok {
        return (status, 0.0, 0.0);
    }
    let solved = tpn_fwd_solve(prm, xi, eta, NEWTON_MAX_ITERATIONS);
    (solved.status, solved.x, solved.y)
}

/// The first stage of [`tpn_fwd`], its only trigonometry: the gnomonic
/// projection of (`phi`, `theta`) to (xi, eta), or (`phi`, `theta`)
/// unchanged when the projection excludes the TAN term.
/// The status is [`Status::BadPoint`] for a point beyond the equator, with
/// the outputs the arithmetic gave, which [`tpn_fwd`] does not return.
// The first part of astTPNfwd, which returns before computing r for a point
// with a negative sine. The arithmetic is the same here with no branch on
// the point, so that the stage vectorizes.
#[inline(always)]
pub(super) fn tpn_fwd_front<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    if prm.n != 0 {
        let s = T::sind(theta);
        let r = prm.r0 * T::cosd(theta) / s;
        let xi = r * T::sind(phi);
        let eta = -r * T::cosd(phi);
        let status = if s < 0.0 {
            Status::BadPoint
        } else {
            Status::Ok
        };
        (status, xi, eta)
    } else {
        (Status::Ok, phi, theta)
    }
}

/// What [`tpn_fwd_solve`] found.
pub(super) struct Solved {
    /// [`Status::BadPoint`] if the iteration did not converge.
    pub(super) status: Status,
    /// The last iterate.
    pub(super) x: f64,
    pub(super) y: f64,
    /// The partial derivatives `[fx, fy, gx, gy]` of the two polynomials at
    /// the iterate the last step was taken from, when the iteration
    /// converged; `None` when it did not, or when the projection has no
    /// polynomial correction and no step was taken.
    pub(super) jacobian: Option<[f64; 4]>,
}

/// The second stage of [`tpn_fwd`]: the (x, y) whose polynomials give
/// (`xi`, `eta`), by Newton's method in at most `max_iterations` steps, or
/// [`Status::BadPoint`] with the last iterate if it has not converged by
/// then.
// The rest of astTPNfwd. `max_iterations` is the C loop bound for the
// per-value entry; the slice entry passes a smaller one (see
// `tpn_fwd_finish`). The Jacobian is returned for `tpn_fwd_finish`; the
// per-value entry discards it.
#[inline(always)]
pub(super) fn tpn_fwd_solve(prm: &PrjPrm, xi: f64, eta: f64, max_iterations: usize) -> Solved {
    // Simple tan.
    if prm.w[0] == 0.0 {
        return Solved {
            status: Status::Ok,
            x: xi,
            y: eta,
            jacobian: None,
        };
    }

    // Tan with polynomial corrections: iterate using Newton's method to get
    // the (x, y) corresponding to the above (xi, eta).
    let a = &prm.p2;
    let b = &prm.p;

    // Initial guess: linear solution assuming a3,... and b3,... are zero.
    let mut x;
    let mut y;
    let denom = a[1] * b[1] - a[2] * b[2];
    if denom != 0.0 {
        x = (xi * b[1] - eta * a[2] - a[0] * b[1] + b[0] * a[2]) / denom;
        y = -(xi * b[2] - eta * a[1] - a[0] * b[2] + b[0] * a[1]) / denom;
    } else {
        if a[1] != 0.0 {
            x = (xi - a[0]) / a[1];
        } else {
            x = a[0];
        }

        if b[1] != 0.0 {
            y = (eta - b[0]) / b[1];
        } else {
            y = b[0];
        }
    }

    let mut ok = false;
    let mut jacobian = None;
    for _ in 0..max_iterations {
        let t = Terms::new(x, y);
        let Terms {
            x2,
            xy,
            y2,
            x3,
            x2y,
            xy2,
            y3,
            r,
            r3,
            x4,
            x3y,
            x2y2,
            xy3,
            y4,
            x5,
            x4y,
            x3y2,
            x2y3,
            xy4,
            y5,
            r5,
            x6,
            x5y,
            x4y2,
            x3y3,
            x2y4,
            xy5,
            y6,
            ..
        } = t;

        // The xi and eta models corresponding to the current x and y.
        let f = t.xi(a);
        let g = t.eta(b);

        let xr = if r != 0.0 { x / r } else { 0.0 };
        let yr = if r != 0.0 { y / r } else { 0.0 };

        // Partial derivative of xi wrt x...
        let fx = a[1]
            + a[3] * xr
            + 2.0 * a[4] * x
            + a[5] * y
            + 3.0 * a[7] * x2
            + 2.0 * a[8] * xy
            + a[9] * y2
            + 3.0 * a[11] * r * x
            + 4.0 * a[12] * x3
            + 3.0 * a[13] * x2y
            + 2.0 * a[14] * xy2
            + a[15] * y3
            + 5.0 * a[17] * x4
            + 4.0 * a[18] * x3y
            + 3.0 * a[19] * x2y2
            + 2.0 * a[20] * xy3
            + a[21] * y4
            + 5.0 * a[23] * r3 * x
            + 6.0 * a[24] * x5
            + 5.0 * a[25] * x4y
            + 4.0 * a[26] * x3y2
            + 3.0 * a[27] * x2y3
            + 2.0 * a[28] * xy4
            + a[29] * y5
            + 7.0 * a[31] * x6
            + 6.0 * a[32] * x5y
            + 5.0 * a[33] * x4y2
            + 4.0 * a[34] * x3y3
            + 3.0 * a[35] * x2y4
            + 2.0 * a[36] * xy5
            + a[37] * y6
            + 7.0 * a[39] * r5 * x;
        // Partial derivative of xi wrt y...
        let fy = a[2]
            + a[3] * yr
            + a[5] * x
            + 2.0 * a[6] * y
            + a[8] * x2
            + 2.0 * a[9] * xy
            + 3.0 * a[10] * y2
            + 3.0 * a[11] * r * y
            + a[13] * x3
            + 2.0 * a[14] * x2y
            + 3.0 * a[15] * xy2
            + 4.0 * a[16] * y3
            + a[18] * x4
            + 2.0 * a[19] * x3y
            + 3.0 * a[20] * x2y2
            + 4.0 * a[21] * xy3
            + 5.0 * a[22] * y4
            + 5.0 * a[23] * r3 * y
            + a[25] * x5
            + 2.0 * a[26] * x4y
            + 3.0 * a[27] * x3y2
            + 4.0 * a[28] * x2y3
            + 5.0 * a[29] * xy4
            + 6.0 * a[30] * y5
            + a[32] * x6
            + 2.0 * a[33] * x5y
            + 3.0 * a[34] * x4y2
            + 4.0 * a[35] * x3y3
            + 5.0 * a[36] * x2y4
            + 6.0 * a[37] * xy5
            + 7.0 * a[38] * y6
            + 7.0 * a[39] * r5 * y;
        // Partial derivative of eta wrt x...
        let gx = b[2]
            + b[3] * xr
            + b[5] * y
            + 2.0 * b[6] * x
            + b[8] * y2
            + 2.0 * b[9] * xy
            + 3.0 * b[10] * x2
            + 3.0 * b[11] * r * x
            + b[13] * y3
            + 2.0 * b[14] * xy2
            + 3.0 * b[15] * x2y
            + 4.0 * b[16] * x3
            + b[18] * y4
            + 2.0 * b[19] * xy3
            + 3.0 * b[20] * x2y2
            + 4.0 * b[21] * x3y
            + 5.0 * b[22] * x4
            + 5.0 * b[23] * r3 * x
            + b[25] * y5
            + 2.0 * b[26] * xy4
            + 3.0 * b[27] * x2y3
            + 4.0 * b[28] * x3y2
            + 5.0 * b[29] * x4y
            + 6.0 * b[30] * x5
            + b[32] * y6
            + 2.0 * b[33] * xy5
            + 3.0 * b[34] * x2y4
            + 4.0 * b[35] * x3y3
            + 5.0 * b[36] * x4y2
            + 6.0 * b[37] * x5y
            + 7.0 * b[38] * x6
            + 7.0 * b[39] * r5 * x;
        // Partial derivative of eta wrt y...
        let gy = b[1]
            + b[3] * yr
            + 2.0 * b[4] * y
            + b[5] * x
            + 3.0 * b[7] * y2
            + 2.0 * b[8] * xy
            + b[9] * x2
            + 3.0 * b[11] * r * y
            + 4.0 * b[12] * y3
            + 3.0 * b[13] * xy2
            + 2.0 * b[14] * x2y
            + b[15] * x3
            + 5.0 * b[17] * y4
            + 4.0 * b[18] * xy3
            + 3.0 * b[19] * x2y2
            + 2.0 * b[20] * x3y
            + b[21] * x4
            + 5.0 * b[23] * r3 * y
            + 6.0 * b[24] * y5
            + 5.0 * b[25] * xy4
            + 4.0 * b[26] * x2y3
            + 3.0 * b[27] * x3y2
            + 2.0 * b[28] * x4y
            + b[29] * x5
            + 7.0 * b[31] * y6
            + 6.0 * b[32] * xy5
            + 5.0 * b[33] * x2y4
            + 4.0 * b[34] * x3y3
            + 3.0 * b[35] * x4y2
            + 2.0 * b[36] * x5y
            + b[37] * x6
            + 7.0 * b[39] * r5 * y;

        // Calculate new x and y values.
        let f = f - xi;
        let g = g - eta;
        let dx = ((-f * gy) + (g * fy)) / ((fx * gy) - (fy * gx));
        let dy = ((-g * fx) + (f * gx)) / ((fx * gy) - (fy * gx));
        x += dx;
        y += dy;

        // Check if convergence has been achieved.
        if dx.abs() <= NEWTON_RELATIVE_TOLERANCE * x.abs()
            && dy.abs() <= NEWTON_RELATIVE_TOLERANCE * y.abs()
        {
            ok = true;
            jacobian = Some([fx, fy, gx, gy]);
            break;
        }
    }

    Solved {
        status: if ok { Status::Ok } else { Status::BadPoint },
        x,
        y,
        jacobian,
    }
}

// reference/starlink-ast/wcslib/tpn.c, astTPNrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn tpn_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let xi;
    let eta;

    // Simple tan.
    if prm.w[0] == 0.0 {
        xi = x;
        eta = y;
    } else {
        // Tan with polynomial corrections.
        let t = Terms::new(x, y);
        xi = t.xi(&prm.p2);
        eta = t.eta(&prm.p);
    }

    // Now do the tan projection.
    let phi;
    let theta;
    if prm.n != 0 {
        let r = math::sqrt(xi * xi + eta * eta);
        phi = if r == 0.0 { 0.0 } else { T::atan2d(xi, -eta) };
        theta = T::atan2d(prm.r0, r);
    } else {
        phi = xi;
        theta = eta;
    }

    (Status::Ok, phi, theta)
}

// The predicate combines its tests with `|`, which evaluates both sides, so
// it vectorizes with the front end.

/// Whether [`tpn_fwd_front`] needs [`Libm`](super::trig::Libm) at (`phi`,
/// `theta`): a sine or cosine argument outside the kernels' cover, when the
/// projection includes the TAN term.
// The front end's only decision is its status, from the sign of
// `sind(theta)`, and it has no branch on the point. A covered `theta` is not
// a multiple of 180, so its sine is not zero, and the kernel's sine, within
// 2 ULP of libm's (see `Kernel`), has that sine's sign, as for TAN
// (`tan_fwd_needs_special`): the kernel cannot turn a bad point good or a
// good one bad. Without the TAN term there is no
// trigonometry. What the Newton iteration decides from the front end's
// output is `tpn_fwd_finish`'s concern.
#[inline(always)]
pub(super) fn tpn_fwd_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    (prm.n != 0) & (!sincosd_covers(theta) | !sincosd_covers(phi))
}

/// The most Newton iterations [`tpn_fwd_finish`] makes on the front end's
/// kernel output: one fewer than the per-value entry, for the reason
/// [`tpn_fwd_finish`] gives.
pub(super) const KERNEL_FED_MAX_ITERATIONS: usize = NEWTON_MAX_ITERATIONS - 1;

/// Where the terms of each degree start among the polynomials'
/// coefficients, and where the last degree's end: the terms of degree d are
/// `DEGREE_STARTS[d]..DEGREE_STARTS[d + 1]`, with `r` counted as degree 1,
/// `r^3` as 3 and so on.
const DEGREE_STARTS: [usize; 9] = [0, 1, 4, 7, 12, 17, 24, 31, 40];

/// The sum of the magnitudes of the coefficients `a` of each degree.
fn degree_magnitude_sums(a: &[f64]) -> [f64; 8] {
    std::array::from_fn(|d| {
        a[DEGREE_STARTS[d]..DEGREE_STARTS[d + 1]]
            .iter()
            .map(|c| c.abs())
            .sum()
    })
}

/// An upper bound on the sum of the magnitudes of the terms of a polynomial
/// at any point within `r` of the origin, from its
/// [`degree_magnitude_sums`]: each term of degree d is at most `r^d` in
/// magnitude.
// Estrin's scheme, whose short dependency chains keep the bound cheap
// beside the iteration.
#[inline(always)]
fn term_magnitude_bound(by_degree: &[f64], r: f64) -> f64 {
    let c = by_degree;
    let r2 = r * r;
    let r4 = r2 * r2;
    ((c[0] + c[1] * r) + r2 * (c[2] + c[3] * r)) + r4 * ((c[4] + c[5] * r) + r2 * (c[6] + c[7] * r))
}

/// How many units of 2^-53 of the polynomials' term magnitudes bound the
/// difference between the slice's and the per-value entry's computed
/// residuals at the same iterate (see [`tpn_fwd_step_noise`]).
// Each path's computed residual `fl(F) - xi` differs from the exact value at
// its iterate by at most the sum over the terms of each term's magnitude
// times its count of roundings: its own (none for `x`, 11 for `r7`, whose
// `r2` is rounded twice and reused: r2 2, r 2, r3 5, r5 8, r7 11), one for
// the product by its coefficient, one for each of the left-to-right
// additions it passes through, and one for the subtraction of `xi`. The
// largest count is `a[1] x`'s, 0 + 1 + 39 + 1 = 41; `a[39] r7`, added last,
// has 11 + 1 + 1 + 1 = 14. The uniform count used here, 47, the most
// roundings any monomial has (6) with the coefficient, the 39 additions
// and the subtraction, covers every term's; two paths make 94. The front
// end's xi and eta are TAN's forward arithmetic, within 9 * 2^-52 of Libm's
// relative (`TAN_FWD_SLICE_ULPS`'s derivation, with each kernel sine and
// cosine within 2 ULP of libm's), which is 18 units of 2^-53 of |xi|. |xi|
// is the computed polynomial less the residual, so at most the term
// magnitudes plus the sum's rounding, and 19 units of the term magnitudes
// cover the front end's difference. 94 + 19 = 113.
const STEP_NOISE_UNITS: f64 = 113.0;

/// How far apart the Newton steps the slice and the per-value entry compute
/// at (`x`, `y`) can be from rounding alone, for each coordinate, given the
/// partial derivatives there: `[nu_x, nu_y]` times `|det J|`, and
/// `|det J|`, so that the caller can compare without dividing.
// A step is (dx, dy) = J^-1 (-f, -g) for the residuals (f, g), so residuals
// that differ by at most (uf, ug) give steps that differ by at most
// ((uf |gy| + ug |fy|) / |det J|, (ug |fx| + uf |gx|) / |det J|). What the
// floor and the drift bound need is the difference of the two paths' roots,
// J^-1 times the front end's difference, plus the rounding of one path's
// step; nu, with both paths' rounding in STEP_NOISE_UNITS, covers that. The
// step
// was taken from an iterate within NEWTON_RELATIVE_TOLERANCE of (x, y) in
// each coordinate, so within (|x| + |y|) (1 + NEWTON_RELATIVE_TOLERANCE) of
// the origin, a bound on its distance that needs no square root. The
// rounding of the computation of the bound itself changes it by a relative
// 1e-14 at most.
#[inline(always)]
pub(super) fn tpn_fwd_step_noise(
    prm: &PrjPrm,
    x: f64,
    y: f64,
    jacobian: [f64; 4],
) -> ([f64; 2], f64) {
    let [fx, fy, gx, gy] = jacobian;
    let r = (x.abs() + y.abs()) * (1.0 + NEWTON_RELATIVE_TOLERANCE);
    let unit = STEP_NOISE_UNITS * (f64::EPSILON / 2.0);
    let uf = unit * term_magnitude_bound(&prm.w[1..9], r);
    let ug = unit * term_magnitude_bound(&prm.w[9..17], r);
    (
        [uf * gy.abs() + ug * fy.abs(), ug * fx.abs() + uf * gx.abs()],
        ((fx * gy) - (fy * gx)).abs(),
    )
}

/// The slice entry's second stage of [`tpn_fwd`]: [`tpn_fwd_solve`] on the
/// (status, xi, eta) of [`tpn_fwd_front`] run with the kernels, or `None`
/// so that the point is redone with [`Libm`](super::trig::Libm) when the
/// iteration has not converged within [`KERNEL_FED_MAX_ITERATIONS`], or when
/// a coordinate it converged to is so close to zero that rounding, rather
/// than the convergence of the iteration, decides its convergence test.
// Why a point this accepts has the per-value entry's status.
//
// A bad front-end status is the per-value entry's (`tpn_fwd_needs_special`).
// Otherwise the kernel's xi and eta are within a few ULP of Libm's, and
// the two Newton iterations are the same arithmetic on them, from initial
// guesses linear in them. Their iterates stay within a few ULP of each other
// as long as the iteration does not amplify differences: near the solution
// the iteration map's derivative is zero, and far from it, where the
// polynomial's highest-degree term dominates, it is about 1 - 1/d for a
// term of degree d <= 7, so in both regimes a difference shrinks.
//
// The convergence test, |dx| <= NEWTON_RELATIVE_TOLERANCE |x| and likewise
// for y, can then decide differently on the two paths only where it is
// within nu of its threshold, where nu (`tpn_fwd_step_noise`) bounds the
// difference of the two paths' roots, from the front end's drift, plus the
// rounding of one path's computed step. Newton's method converges
// quadratically, so the step after one at the threshold is of order
// NEWTON_RELATIVE_TOLERANCE^2 |x| plus at most nu, and passes on both paths
// provided nu < NEWTON_RELATIVE_TOLERANCE |x|. So where both coordinates
// are above that floor, the iteration counts of the two paths differ by at
// most one: an iteration that converges here within
// KERNEL_FED_MAX_ITERATIONS converges in the per-value entry within
// NEWTON_MAX_ITERATIONS, and the point is good in both.
//
// Below the floor the test compares rounding noise with a threshold no
// larger than it: a root on or next to an axis of the projection plane, such
// as the image of the reference row or column, converges to a coordinate
// that is rounding noise, and the per-value iteration can step about it
// until it gives up and the point is bad. The step after a split decision
// need not pass there, so nothing bounds the counts' difference, and such a
// point is redone.
//
// Every point not accepted is redone with Libm, which gives the per-value
// entry's status and bits by construction, so the kernels never turn a bad
// point good or a good one bad. An accepted output can move by the extra
// step, of order NEWTON_RELATIVE_TOLERANCE^2 |x|, plus nu for each path.
//
// Two premises remain, neither proven.
//
// First, that the iteration does not amplify the front end's difference
// before it converges. An iteration that wanders near a fold of the
// polynomial map, where its Jacobian is nearly singular, could do so.
//
// Second, that the step after a split decision passes on both coordinates.
// The argument above is per coordinate, but Newton's quadratic error couples
// them: after a split on x, the next step in y is about K TOL^2 x^2, for K
// the polynomials' second derivatives over their first, not
// TOL^2 |y|. Where |y| is above the floor but below about K TOL x^2, that
// step can fail y's test and cost a second extra step, and a point the
// slice accepts at 49 iterations could take 51 in the per-value entry. That
// needs a split decision, which lands within nu of the threshold, a
// coordinate that small next to the other, and an iteration count at the
// limit, all at once. It is rare also because the iterations that run long
// are those far from the root, where the highest-degree term dominates and
// Newton's error is nearly radial, so each coordinate's error stays in
// proportion to the coordinate. No margin covers it, since K depends on the
// point.
//
// None of the inputs tested, among them points converging in 49, 50 and 51
// iterations and points just off the axes, shows the two paths' counts
// differing above the floor.
#[inline(always)]
pub(super) fn tpn_fwd_finish(
    prm: &PrjPrm,
    _phi: f64,
    _theta: f64,
    (status, xi, eta): (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    if status != Status::Ok {
        return Some((status, 0.0, 0.0));
    }
    let solved = tpn_fwd_solve(prm, xi, eta, KERNEL_FED_MAX_ITERATIONS);
    if solved.status != Status::Ok {
        return None;
    }
    let (x, y) = (solved.x, solved.y);
    let Some(jacobian) = solved.jacobian else {
        // No polynomial correction: no iteration and no test.
        return Some((Status::Ok, x, y));
    };
    let ([nu_x, nu_y], det) = tpn_fwd_step_noise(prm, x, y, jacobian);
    // nu < NEWTON_RELATIVE_TOLERANCE |x|, multiplied through by |det J|, and
    // written so that a NaN declines.
    let above_floor = (NEWTON_RELATIVE_TOLERANCE * x.abs() * det > nu_x)
        & (NEWTON_RELATIVE_TOLERANCE * y.abs() * det > nu_y);
    above_floor.then_some((Status::Ok, x, y))
}

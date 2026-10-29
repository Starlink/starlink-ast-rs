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
//! Every function lists its source in `reference/starlink-ast` at 3d3f255f.
//!
//! The tests of the form `prj->flag > 0` in the C bodies are applied
//! unconditionally, as [`super::prj`] explains.

// The bodies keep the shape of the C they are ported from.
#![allow(clippy::needless_late_init, clippy::too_many_lines)]

use super::math;

use super::WCSLIB_MXPAR;
use super::prj::{PrjPrm, Status};
use super::trig::{R2D, Trig};

// Newton's method in `astTPNfwd` stops when both corrections are within this
// fraction of the new coordinate (the `tol` of `reference/starlink-ast/wcslib/tpn.c`).
const NEWTON_RELATIVE_TOLERANCE: f64 = 1.0e-5;

// The most iterations `astTPNfwd` makes before it gives up (the loop bound of
// `reference/starlink-ast/wcslib/tpn.c`).
const NEWTON_MAX_ITERATIONS: usize = 50;

/// The powers and products of a point (x, y) the polynomial terms use, in
/// the names and with the arithmetic of `tpn.c`.
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

    Status::Ok
}

// reference/starlink-ast/wcslib/tpn.c, astTPNfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn tpn_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let xi;
    let eta;
    if prm.n != 0 {
        let s = T::sind(theta);
        if s < 0.0 {
            return (Status::BadPoint, 0.0, 0.0);
        }
        let r = prm.r0 * T::cosd(theta) / s;
        xi = r * T::sind(phi);
        eta = -r * T::cosd(phi);
    } else {
        xi = phi;
        eta = theta;
    }

    // Simple tan.
    if prm.w[0] == 0.0 {
        return (Status::Ok, xi, eta);
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
    for _ in 0..NEWTON_MAX_ITERATIONS {
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
            break;
        }
    }

    if !ok {
        return (Status::BadPoint, x, y);
    }
    (Status::Ok, x, y)
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

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The pseudo-cylindrical projections: SFL, PAR, MOL and AIT.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//! Every function lists its source in `reference/starlink-ast` at 3d3f255f.
//! GLS is SFL with parameters and a `WcsMap` converts it before it gets
//! here.
//!
//! The tests of the form `prj->flag > 0` in the C bodies are applied
//! unconditionally, as [`super::prj`] explains.

// The bodies keep the shape of the C they are ported from, which declares
// each output before the branches that assign it.
#![allow(clippy::needless_late_init)]

use super::math;

use super::prj::{PrjPrm, Status};
use super::trig::{D2R, R2D, Trig, sincosd_covers};
use super::zenithal::c_copysign;
use std::f64::consts::{PI, SQRT_2};

// ---------------------------------------------------------------------------
// SFL: Sanson-Flamsteed
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astSFLset; reference/starlink-ast at 3d3f255f.
pub(super) fn sfl_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 1.0;
        prm.w[1] = 1.0;
    } else {
        prm.w[0] = prm.r0 * D2R;
        prm.w[1] = 1.0 / prm.w[0];
    }

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astSFLfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn sfl_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let x = prm.w[0] * phi * T::cosd(theta);
    let y = prm.w[0] * theta;

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astSFLrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn sfl_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let w = math::cos(y / prm.r0);
    let phi;
    if w == 0.0 {
        phi = 0.0;
    } else {
        phi = x * prm.w[1] / math::cos(y / prm.r0);
    }
    let theta = y * prm.w[1];

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// PAR: parabolic
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astPARset; reference/starlink-ast at 3d3f255f.
pub(super) fn par_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 1.0;
        prm.w[1] = 1.0;
        prm.w[2] = 180.0;
        prm.w[3] = 1.0 / prm.w[2];
    } else {
        prm.w[0] = prm.r0 * D2R;
        prm.w[1] = 1.0 / prm.w[0];
        prm.w[2] = PI * prm.r0;
        prm.w[3] = 1.0 / prm.w[2];
    }

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astPARfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn par_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let s = T::sind(theta / 3.0);
    let x = prm.w[0] * phi * (1.0 - 4.0 * s * s);
    let y = prm.w[2] * s;

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astPARrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn par_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let s = y * prm.w[3];
    if s > 1.0 || s < -1.0 {
        return (Status::BadPoint, 0.0, 0.0);
    }

    let t = 1.0 - 4.0 * s * s;
    let phi;
    if t == 0.0 {
        if x == 0.0 {
            phi = 0.0;
        } else {
            return (Status::BadPoint, 0.0, 0.0);
        }
    } else {
        phi = prm.w[1] * x / t;
    }

    let theta = 3.0 * T::asind(s);

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// MOL: Mollweide
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astMOLset; reference/starlink-ast at 3d3f255f.
// C's `SQRT2` literal rounds to `SQRT_2`.
pub(super) fn mol_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    prm.w[0] = SQRT_2 * prm.r0;
    prm.w[1] = prm.w[0] / 90.0;
    prm.w[2] = 1.0 / prm.w[0];
    prm.w[3] = 90.0 / prm.r0;
    prm.w[4] = 2.0 / PI;

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astMOLfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn mol_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    // The tolerance on the residual of the iteration, `tol` in C.
    const TOL: f64 = 1.0e-13;
    // The most iterations of the bisection in C's `astMOLfwd`.
    const MAX_ITERATIONS: usize = 100;

    let x;
    let y;
    if theta.abs() == 90.0 {
        x = 0.0;
        y = c_copysign(prm.w[0], theta);
    } else if theta == 0.0 {
        x = prm.w[1] * phi;
        y = 0.0;
    } else {
        let u = PI * T::sind(theta);
        let mut v0 = -PI;
        let mut v1 = PI;
        let mut v = u;
        for _ in 0..MAX_ITERATIONS {
            let resid = (v - u) + math::sin(v);
            if resid < 0.0 {
                if resid > -TOL {
                    break;
                }
                v0 = v;
            } else {
                if resid < TOL {
                    break;
                }
                v1 = v;
            }
            v = (v0 + v1) / 2.0;
        }

        let gamma = v / 2.0;
        // C's optimizing compilers merge these two calls into one.
        let (sin_gamma, cos_gamma) = math::sincos_as_c(gamma);
        x = prm.w[1] * phi * cos_gamma;
        y = prm.w[0] * sin_gamma;
    }

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astMOLrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn mol_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // The tolerance on the squared radius and on |z| = 1, `tol` in C.
    const TOL: f64 = 1.0e-12;

    let y0 = y / prm.r0;
    let mut s = 2.0 - y0 * y0;
    let phi;
    if s <= TOL {
        if s < -TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }
        s = 0.0;

        if x.abs() > TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }
        phi = 0.0;
    } else {
        s = math::sqrt(s);
        phi = prm.w[3] * x / s;
    }

    let mut z = y * prm.w[2];
    if z.abs() > 1.0 {
        if z.abs() > 1.0 + TOL {
            return (Status::BadPoint, phi, 0.0);
        }
        z = c_copysign(1.0, z) + y0 * s / PI;
    } else {
        z = math::asin(z) * prm.w[4] + y0 * s / PI;
    }

    if z.abs() > 1.0 {
        if z.abs() > 1.0 + TOL {
            return (Status::BadPoint, phi, 0.0);
        }
        z = c_copysign(1.0, z);
    }

    let theta = T::asind(z);

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// AIT: Hammer-Aitoff
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astAITset; reference/starlink-ast at 3d3f255f.
pub(super) fn ait_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    prm.w[0] = 2.0 * prm.r0 * prm.r0;
    prm.w[1] = 1.0 / (2.0 * prm.w[0]);
    prm.w[2] = prm.w[1] / 4.0;
    prm.w[3] = 1.0 / (2.0 * prm.r0);

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astAITfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn ait_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let cthe = T::cosd(theta);
    let w = math::sqrt(prm.w[0] / (1.0 + cthe * T::cosd(phi / 2.0)));
    let x = 2.0 * w * cthe * T::sind(phi / 2.0);
    let y = w * T::sind(theta);

    (Status::Ok, x, y)
}

/// Whether [`ait_fwd`] needs [`Libm`](super::trig::Libm) at (`phi`,
/// `theta`): a sine or cosine argument outside the kernels' cover, or a
/// divisor of the square root's argument so near zero that the kernels'
/// drift could change its sign.
// The body has no status test and no branch, but its square root is of
// `w0 / (1 + cthe cosd(phi / 2))`, which is NaN or infinite where that
// divisor is negative or zero: decided by `ait_fwd_divisor_near_zero`. The
// divisor is at least 1 for every point a WcsMap projects, whose longitude
// and latitude are within the principal ranges.
#[inline(always)]
pub(super) fn ait_fwd_needs_special<T: Trig>(_prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    !sincosd_covers(theta)
        | !sincosd_covers(phi / 2.0)
        | ait_fwd_divisor_near_zero(T::cosd(theta), T::cosd(phi / 2.0))
}

/// Whether `1 + cthe chalf`, the divisor in [`ait_fwd`], from the kernels'
/// cosines of `theta` and `phi / 2`, is so near zero that
/// [`Libm`](super::trig::Libm)'s could have the other sign or be zero.
// The product `cthe chalf` of two kernel cosines, each within 2 ULP of
// Libm's (see `Kernel`), at most 2 eps of it (eps = 2^-52), differs by at
// most 5 eps of itself, with the two paths' roundings of the product, eps
// between them; the sum by a further eps of itself. That is eps (5 |cthe
// chalf| + |divisor|), to first order. The margin is twice that, which
// covers its own evaluation from the kernels' values rather than Libm's.
// Where the divisor is beyond it, Libm's has the kernels' sign and neither
// is zero.
#[inline(always)]
pub(super) fn ait_fwd_divisor_near_zero(cthe: f64, chalf: f64) -> bool {
    let product = cthe * chalf;
    let divisor = 1.0 + product;
    let margin = 2.0 * f64::EPSILON * (5.0 * product.abs() + divisor.abs());
    // False for a NaN, which is then redone.
    let clear_of_zero = divisor.abs() > margin;
    !clear_of_zero
}

// reference/starlink-ast/wcslib/proj.c, astAITrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn ait_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // The tolerance on u = 1/2 and on |s| = 1, `tol` in C.
    const TOL: f64 = 1.0e-13;

    let mut u = 1.0 - x * x * prm.w[2] - y * y * prm.w[1];
    if u < 0.5 {
        if u < 0.5 - TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }

        u = 0.5;
    }

    let z = math::sqrt(u);
    let mut s = z * y / prm.r0;
    if s.abs() > 1.0 {
        if s.abs() > 1.0 + TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }
        s = c_copysign(1.0, s);
    }

    let xp = 2.0 * z * z - 1.0;
    let yp = z * x * prm.w[3];
    let phi;
    if xp == 0.0 && yp == 0.0 {
        phi = 0.0;
    } else {
        phi = 2.0 * T::atan2d(yp, xp);
    }
    let theta = T::asind(s);

    (Status::Ok, phi, theta)
}

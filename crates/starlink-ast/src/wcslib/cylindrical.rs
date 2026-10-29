// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The cylindrical projections: CYP, CEA, CAR and MER.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//! Every function lists its source in `reference/starlink-ast` at 3d3f255f.
//!
//! The tests of the form `prj->flag > 0` in the C bodies are applied
//! unconditionally, as [`super::prj`] explains.

use super::math;

use super::prj::{PrjPrm, Status};
use super::trig::{D2R, R2D, Trig, sincosd_covers};
use super::zenithal::c_copysign;

// ---------------------------------------------------------------------------
// CYP: cylindrical perspective
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astCYPset; reference/starlink-ast at 3d3f255f.
pub(super) fn cyp_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;

        prm.w[0] = prm.p[2];
        if prm.w[0] == 0.0 {
            return Status::BadParams;
        }

        prm.w[1] = 1.0 / prm.w[0];

        prm.w[2] = R2D * (prm.p[1] + prm.p[2]);
        if prm.w[2] == 0.0 {
            return Status::BadParams;
        }

        prm.w[3] = 1.0 / prm.w[2];
    } else {
        prm.w[0] = prm.r0 * prm.p[2] * D2R;
        if prm.w[0] == 0.0 {
            return Status::BadParams;
        }

        prm.w[1] = 1.0 / prm.w[0];

        prm.w[2] = prm.r0 * (prm.p[1] + prm.p[2]);
        if prm.w[2] == 0.0 {
            return Status::BadParams;
        }

        prm.w[3] = 1.0 / prm.w[2];
    }

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astCYPfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn cyp_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let s = prm.p[1] + T::cosd(theta);

    let x = prm.w[0] * phi;
    let y = prm.w[2] * T::sind(theta) / s;

    // C returns at `s == 0` before it computes x and y, and the outputs it
    // returns are then (0, 0); the status is a select, so that the body has
    // no branch on the point.
    if s == 0.0 {
        (Status::BadPoint, 0.0, 0.0)
    } else {
        (Status::Ok, x, y)
    }
}

/// Whether [`cyp_fwd`] needs [`Libm`](super::trig::Libm) at (`phi`,
/// `theta`): a sine or cosine argument outside the kernels' cover, or a
/// denominator so near zero that the kernels' drift could change whether it
/// is zero.
// The body's only decision is its status, `s == 0`, with `s = p1 +
// cosd(theta)` from the kernels: decided by `cyp_fwd_s_near_zero`. It has no
// branch.
#[inline(always)]
pub(super) fn cyp_fwd_needs_special<T: Trig>(prm: &PrjPrm, _phi: f64, theta: f64) -> bool {
    !sincosd_covers(theta) | cyp_fwd_s_near_zero(prm, T::cosd(theta))
}

/// Whether the denominator `s` of [`cyp_fwd`], from the kernels' cosine of
/// `theta`, is so near zero that [`Libm`](super::trig::Libm)'s could be
/// zero where it is not, or not where it is.
// `s` is `p1 + cthe`. The kernels' cosine is within 2 ULP of Libm's (see
// `Kernel`), at most 2 eps of it (eps = 2^-52), and each path rounds the sum
// once, at most eps / 2 relative, so eps between the two paths: `s` differs
// by at most eps (2 |cthe| + |s|), to first order. The margin is twice that,
// which covers its own evaluation from the kernels' values rather than
// Libm's. Where |s| is beyond it, Libm's `s` has the kernels' sign and
// neither is zero.
#[inline(always)]
pub(super) fn cyp_fwd_s_near_zero(prm: &PrjPrm, cthe: f64) -> bool {
    let s = prm.p[1] + cthe;
    let margin = 2.0 * f64::EPSILON * (2.0 * cthe.abs() + s.abs());
    // False for a NaN, which is then redone.
    let clear_of_zero = s.abs() > margin;
    !clear_of_zero
}

// reference/starlink-ast/wcslib/proj.c, astCYPrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn cyp_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // The tolerance on |a| = 1, `tol` in C.
    const TOL: f64 = 1.0e-13;

    let phi = x * prm.w[1];
    let eta = y * prm.w[3];

    let a = eta * prm.p[1] / math::sqrt(eta * eta + 1.0);
    let theta;
    if a.abs() < 1.0 {
        theta = T::atan2d(eta, 1.0) + T::asind(a);
    } else if a.abs() < 1.0 + TOL {
        if a > 0.0 {
            theta = T::atan2d(eta, 1.0) + 90.0;
        } else {
            theta = T::atan2d(eta, 1.0) - 90.0;
        }
    } else {
        return (Status::BadPoint, phi, 0.0);
    }

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// CEA: cylindrical equal area
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astCEAset; reference/starlink-ast at 3d3f255f.
pub(super) fn cea_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 1.0;
        prm.w[1] = 1.0;
        if prm.p[1] <= 0.0 || prm.p[1] > 1.0 {
            return Status::BadParams;
        }
        prm.w[2] = prm.r0 / prm.p[1];
        prm.w[3] = prm.p[1] / prm.r0;
    } else {
        prm.w[0] = prm.r0 * D2R;
        prm.w[1] = R2D / prm.r0;
        if prm.p[1] <= 0.0 || prm.p[1] > 1.0 {
            return Status::BadParams;
        }
        prm.w[2] = prm.r0 / prm.p[1];
        prm.w[3] = prm.p[1] / prm.r0;
    }

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astCEAfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn cea_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let x = prm.w[0] * phi;
    let y = prm.w[2] * T::sind(theta);

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astCEArev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn cea_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // The tolerance on |s| = 1, `tol` in C.
    const TOL: f64 = 1.0e-13;

    let mut s = y * prm.w[3];
    if s.abs() > 1.0 {
        if s.abs() > 1.0 + TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }
        s = c_copysign(1.0, s);
    }

    let phi = x * prm.w[1];
    let theta = T::asind(s);

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// CAR: Cartesian
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astCARset; reference/starlink-ast at 3d3f255f.
pub(super) fn car_set(prm: &mut PrjPrm) -> Status {
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

// reference/starlink-ast/wcslib/proj.c, astCARfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn car_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let x = prm.w[0] * phi;
    let y = prm.w[0] * theta;

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astCARrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn car_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let phi = prm.w[1] * x;
    let theta = prm.w[1] * y;

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// MER: Mercator
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astMERset; reference/starlink-ast at 3d3f255f.
pub(super) fn mer_set(prm: &mut PrjPrm) -> Status {
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

// reference/starlink-ast/wcslib/proj.c, astMERfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn mer_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    if theta <= -90.0 || theta >= 90.0 {
        return (Status::BadPoint, 0.0, 0.0);
    }

    let x = prm.w[0] * phi;
    let y = prm.r0 * math::ln(T::tand((90.0 + theta) / 2.0));

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astMERrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn mer_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let phi = x * prm.w[1];
    let theta = 2.0 * T::atand(math::exp(y / prm.r0)) - 90.0;

    (Status::Ok, phi, theta)
}

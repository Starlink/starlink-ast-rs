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
use super::trig::{D2R, R2D, Trig};
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
    if s == 0.0 {
        return (Status::BadPoint, 0.0, 0.0);
    }

    let x = prm.w[0] * phi;
    let y = prm.w[2] * T::sind(theta) / s;

    (Status::Ok, x, y)
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

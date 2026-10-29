// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The conic projections: COP, COE, COD and COO.
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
use super::trig::{D2R, Libm, R2D, Trig};

// ---------------------------------------------------------------------------
// COP: conic perspective
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astCOPset; reference/starlink-ast at 3d3f255f.
pub(super) fn cop_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = prm.p[1];

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    prm.w[0] = Libm::sind(prm.p[1]);
    if prm.w[0] == 0.0 {
        return Status::BadParams;
    }

    prm.w[1] = 1.0 / prm.w[0];

    prm.w[3] = prm.r0 * Libm::cosd(prm.p[2]);
    if prm.w[3] == 0.0 {
        return Status::BadParams;
    }

    prm.w[4] = 1.0 / prm.w[3];
    prm.w[5] = 1.0 / Libm::tand(prm.p[1]);

    prm.w[2] = prm.w[3] * prm.w[5];

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astCOPfwd; reference/starlink-ast at 3d3f255f,
// with the exact values at the poles, which the C comments as upstream 4.22.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn cop_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let t = theta - prm.p[1];
    let s = T::cosd(t);
    if s == 0.0 {
        return (Status::BadPoint, 0.0, 0.0);
    }

    let r;
    let bad;
    if theta.abs() == 90.0 {
        // The pole on the same side as the standard parallels is the apex
        // of the cone, and the other is on the far side.
        r = 0.0;
        bad = (theta < 0.0) != (prm.p[1] < 0.0);
    } else {
        r = prm.w[2] - prm.w[3] * T::sind(t) / s;
        bad = r * prm.w[0] < 0.0;
    }

    let a = prm.w[0] * phi;

    let x = r * T::sind(a);
    let y = prm.w[2] - r * T::cosd(a);

    if bad {
        return (Status::BadPoint, x, y);
    }

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astCOPrev; reference/starlink-ast at 3d3f255f.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn cop_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let dy = prm.w[2] - y;
    let mut r = math::sqrt(x * x + dy * dy);
    if prm.p[1] < 0.0 {
        r = -r;
    }

    let a;
    if r == 0.0 {
        a = 0.0;
    } else {
        a = T::atan2d(x / r, dy / r);
    }

    let phi = a * prm.w[1];
    let theta = prm.p[1] + T::atand(prm.w[5] - r * prm.w[4]);

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// COE: conic equal area
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astCOEset; reference/starlink-ast at 3d3f255f.
pub(super) fn coe_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = prm.p[1];

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    let theta1 = prm.p[1] - prm.p[2];
    let theta2 = prm.p[1] + prm.p[2];

    prm.w[0] = (Libm::sind(theta1) + Libm::sind(theta2)) / 2.0;
    if prm.w[0] == 0.0 {
        return Status::BadParams;
    }

    prm.w[1] = 1.0 / prm.w[0];

    prm.w[3] = prm.r0 / prm.w[0];
    prm.w[4] = 1.0 + Libm::sind(theta1) * Libm::sind(theta2);
    prm.w[5] = 2.0 * prm.w[0];
    prm.w[6] = prm.w[3] * prm.w[3] * prm.w[4];
    prm.w[7] = 1.0 / (2.0 * prm.r0 * prm.w[3]);
    prm.w[8] = prm.w[3] * math::sqrt(prm.w[4] + prm.w[5]);

    prm.w[2] = prm.w[3] * math::sqrt(prm.w[4] - prm.w[5] * Libm::sind(prm.p[1]));

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astCOEfwd; reference/starlink-ast at 3d3f255f.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn coe_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let a = phi * prm.w[0];
    let r;
    if theta == -90.0 {
        r = prm.w[8];
    } else {
        r = prm.w[3] * math::sqrt(prm.w[4] - prm.w[5] * T::sind(theta));
    }

    let x = r * T::sind(a);
    let y = prm.w[2] - r * T::cosd(a);

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astCOErev; reference/starlink-ast at 3d3f255f.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn coe_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // The tolerance on `r` at the south pole and on |w| = 1, `tol` in C.
    const TOL: f64 = 1.0e-12;

    let dy = prm.w[2] - y;
    let mut r = math::sqrt(x * x + dy * dy);
    if prm.p[1] < 0.0 {
        r = -r;
    }

    let a;
    if r == 0.0 {
        a = 0.0;
    } else {
        a = T::atan2d(x / r, dy / r);
    }

    let phi = a * prm.w[1];
    let theta;
    if (r - prm.w[8]).abs() < TOL {
        theta = -90.0;
    } else {
        let w = (prm.w[6] - r * r) * prm.w[7];
        if w.abs() > 1.0 {
            if (w - 1.0).abs() < TOL {
                theta = 90.0;
            } else if (w + 1.0).abs() < TOL {
                theta = -90.0;
            } else {
                return (Status::BadPoint, phi, 0.0);
            }
        } else {
            theta = T::asind(w);
        }
    }

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// COD: conic equidistant
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astCODset; reference/starlink-ast at 3d3f255f.
pub(super) fn cod_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = prm.p[1];

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    if prm.p[2] == 0.0 {
        prm.w[0] = prm.r0 * Libm::sind(prm.p[1]) * D2R;
    } else {
        prm.w[0] = prm.r0 * Libm::sind(prm.p[1]) * Libm::sind(prm.p[2]) / prm.p[2];
    }

    if prm.w[0] == 0.0 {
        return Status::BadParams;
    }

    prm.w[1] = 1.0 / prm.w[0];
    prm.w[2] = prm.r0 * Libm::cosd(prm.p[2]) * Libm::cosd(prm.p[1]) / prm.w[0];
    prm.w[3] = prm.w[2] + prm.p[1];

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astCODfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn cod_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let a = prm.w[0] * phi;
    let r = prm.w[3] - theta;

    let x = r * T::sind(a);
    let y = prm.w[2] - r * T::cosd(a);

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astCODrev; reference/starlink-ast at 3d3f255f.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn cod_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let dy = prm.w[2] - y;
    let mut r = math::sqrt(x * x + dy * dy);
    if prm.p[1] < 0.0 {
        r = -r;
    }

    let a;
    if r == 0.0 {
        a = 0.0;
    } else {
        a = T::atan2d(x / r, dy / r);
    }

    let phi = a * prm.w[1];
    let theta = prm.w[3] - r;

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// COO: conic orthomorphic
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astCOOset; reference/starlink-ast at 3d3f255f.
pub(super) fn coo_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = prm.p[1];

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    let theta1 = prm.p[1] - prm.p[2];
    let theta2 = prm.p[1] + prm.p[2];

    let tan1 = Libm::tand((90.0 - theta1) / 2.0);
    let cos1 = Libm::cosd(theta1);

    if theta1 == theta2 {
        prm.w[0] = Libm::sind(theta1);
    } else {
        let tan2 = Libm::tand((90.0 - theta2) / 2.0);
        let cos2 = Libm::cosd(theta2);
        prm.w[0] = math::ln(cos2 / cos1) / math::ln(tan2 / tan1);
    }
    if prm.w[0] == 0.0 {
        return Status::BadParams;
    }

    prm.w[1] = 1.0 / prm.w[0];

    prm.w[3] = prm.r0 * (cos1 / prm.w[0]) / math::powf(tan1, prm.w[0]);
    if prm.w[3] == 0.0 {
        return Status::BadParams;
    }
    prm.w[2] = prm.w[3] * math::powf(Libm::tand((90.0 - prm.p[1]) / 2.0), prm.w[0]);
    prm.w[4] = 1.0 / prm.w[3];

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astCOOfwd; reference/starlink-ast at 3d3f255f.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn coo_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let a = prm.w[0] * phi;
    let r;
    if theta == -90.0 {
        if prm.w[0] < 0.0 {
            r = 0.0;
        } else {
            return (Status::BadPoint, 0.0, 0.0);
        }
    } else {
        r = prm.w[3] * math::powf(T::tand((90.0 - theta) / 2.0), prm.w[0]);
    }

    let x = r * T::sind(a);
    let y = prm.w[2] - r * T::cosd(a);

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astCOOrev; reference/starlink-ast at 3d3f255f.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn coo_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let dy = prm.w[2] - y;
    let mut r = math::sqrt(x * x + dy * dy);
    if prm.p[1] < 0.0 {
        r = -r;
    }

    let a;
    if r == 0.0 {
        a = 0.0;
    } else {
        a = T::atan2d(x / r, dy / r);
    }

    let phi = a * prm.w[1];
    let theta;
    if r == 0.0 {
        if prm.w[0] < 0.0 {
            theta = -90.0;
        } else {
            return (Status::BadPoint, phi, 0.0);
        }
    } else {
        theta = 90.0 - 2.0 * T::atand(math::powf(r * prm.w[4], prm.w[1]));
    }

    (Status::Ok, phi, theta)
}

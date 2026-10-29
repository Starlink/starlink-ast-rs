// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The zenithal projections: AZP, SZP, TAN, STG, SIN, ARC, ZPN, ZEA and AIR.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//! Every function lists its source in `vendor/wcslib/proj.c` and the
//! `reference/starlink-ast` commit that file comes from, 3d3f255f.
//!
//! The tests of the form `prj->flag > 0` in the C bodies are applied
//! unconditionally, as [`super::prj`] explains.

// The projections are used by `WcsMap` in a later step; until then only the
// tests use them.
#![allow(dead_code)]
// The bodies keep the shape of the C they are ported from, which declares
// each output before the branches that assign it.
#![allow(clippy::needless_late_init)]

use crate::math;

use super::PRJ_PARAM_SLOTS;
use super::prj::{PrjPrm, Status};
use super::trig::{D2R, Libm, R2D, Trig};
use std::f64::consts::PI;

/// C's `(a > b) ? a : b`, which differs from `f64::max` when either is NaN.
#[inline(always)]
fn c_max(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// C's `(a < b) ? a : b`, which differs from `f64::min` when either is NaN.
#[inline(always)]
fn c_min(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

/// C's `copysign` macro in `proj.c`: `x` with the sign of `y`, taking `-0.0`
/// and NaN as non-negative.
#[inline(always)]
pub(super) fn c_copysign(x: f64, y: f64) -> f64 {
    if y < 0.0 { -x.abs() } else { x.abs() }
}

// ---------------------------------------------------------------------------
// AZP: zenithal/azimuthal perspective
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astAZPset; reference/starlink-ast at 3d3f255f.
pub(super) fn azp_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    prm.w[0] = prm.r0 * (prm.p[1] + 1.0);
    if prm.w[0] == 0.0 {
        return Status::BadParams;
    }

    prm.w[3] = Libm::cosd(prm.p[2]);
    if prm.w[3] == 0.0 {
        return Status::BadParams;
    }

    prm.w[2] = 1.0 / prm.w[3];
    prm.w[4] = Libm::sind(prm.p[2]);
    prm.w[1] = prm.w[4] / prm.w[3];

    if prm.p[1].abs() > 1.0 {
        prm.w[5] = Libm::asind(-1.0 / prm.p[1]);
    } else {
        prm.w[5] = -90.0;
    }

    prm.w[6] = prm.p[1] * prm.w[3];
    prm.w[7] = if prm.w[6].abs() < 1.0 { 1.0 } else { 0.0 };

    Status::Ok
}

// vendor/wcslib/proj.c, astAZPfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn azp_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let cphi = T::cosd(phi);
    let cthe = T::cosd(theta);

    let mut s = prm.w[1] * cphi;
    let mut t = (prm.p[1] + T::sind(theta)) + cthe * s;
    if t == 0.0 {
        return (Status::BadPoint, 0.0, 0.0);
    }

    let r = prm.w[0] * cthe / t;
    let x = r * T::sind(phi);
    let y = -r * cphi * prm.w[2];

    // Bounds checking.
    // Overlap.
    if theta < prm.w[5] {
        return (Status::BadPoint, x, y);
    }

    // Divergence.
    if prm.w[7] > 0.0 {
        t = prm.p[1] / math::sqrt(1.0 + s * s);

        if t.abs() <= 1.0 {
            s = T::atand(-s);
            t = T::asind(t);
            let mut a = s - t;
            let mut b = s + t + 180.0;

            if a > 90.0 {
                a -= 360.0;
            }
            if b > 90.0 {
                b -= 360.0;
            }

            if theta < c_max(a, b) {
                return (Status::BadPoint, x, y);
            }
        }
    }

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astAZPrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn azp_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // The tolerance on |t| = 1, `tol` in C.
    const TOL: f64 = 1.0e-13;

    let ycosg = y * prm.w[3];

    let r = math::sqrt(x * x + ycosg * ycosg);
    let phi;
    let theta;
    if r == 0.0 {
        phi = 0.0;
        theta = 90.0;
    } else {
        phi = T::atan2d(x, -ycosg);

        let mut s = r / (prm.w[0] + y * prm.w[4]);
        let mut t = s * prm.p[1] / math::sqrt(s * s + 1.0);

        s = T::atan2d(1.0, s);

        if t.abs() > 1.0 {
            t = c_copysign(90.0, t);
            if t.abs() > 1.0 + TOL {
                return (Status::BadPoint, phi, 0.0);
            }
        } else {
            t = T::asind(t);
        }

        let mut a = s - t;
        let mut b = s + t + 180.0;

        if a > 90.0 {
            a -= 360.0;
        }
        if b > 90.0 {
            b -= 360.0;
        }

        theta = c_max(a, b);
    }

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// SZP: slant zenithal perspective
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astSZPset; reference/starlink-ast at 3d3f255f.
pub(super) fn szp_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    prm.w[0] = 1.0 / prm.r0;

    prm.w[3] = prm.p[1] * Libm::sind(prm.p[3]) + 1.0;
    if prm.w[3] == 0.0 {
        return Status::BadParams;
    }

    prm.w[1] = -prm.p[1] * Libm::cosd(prm.p[3]) * Libm::sind(prm.p[2]);
    prm.w[2] = prm.p[1] * Libm::cosd(prm.p[3]) * Libm::cosd(prm.p[2]);
    prm.w[4] = prm.r0 * prm.w[1];
    prm.w[5] = prm.r0 * prm.w[2];
    prm.w[6] = prm.r0 * prm.w[3];
    prm.w[7] = (prm.w[3] - 1.0) * prm.w[3] - 1.0;

    if (prm.w[3] - 1.0).abs() < 1.0 {
        prm.w[8] = Libm::asind(1.0 - prm.w[3]);
    } else {
        prm.w[8] = -90.0;
    }

    Status::Ok
}

// vendor/wcslib/proj.c, astSZPfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn szp_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let cphi = T::cosd(phi);
    let sphi = T::sind(phi);
    let cthe = T::cosd(theta);
    let mut s = 1.0 - T::sind(theta);

    let mut t = prm.w[3] - s;
    if t == 0.0 {
        return (Status::BadPoint, 0.0, 0.0);
    }

    let x = (prm.w[6] * cthe * sphi - prm.w[4] * s) / t;
    let y = -(prm.w[6] * cthe * cphi + prm.w[5] * s) / t;

    // Bounds checking.
    // Divergence.
    if theta < prm.w[8] {
        return (Status::BadPoint, x, y);
    }

    // Overlap.
    if prm.p[1].abs() > 1.0 {
        s = prm.w[1] * sphi - prm.w[2] * cphi;
        t = 1.0 / math::sqrt(prm.w[7] + s * s);

        if t.abs() <= 1.0 {
            s = T::atan2d(s, prm.w[3] - 1.0);
            t = T::asind(t);
            let mut a = s - t;
            let mut b = s + t + 180.0;

            if a > 90.0 {
                a -= 360.0;
            }
            if b > 90.0 {
                b -= 360.0;
            }

            if theta < c_max(a, b) {
                return (Status::BadPoint, x, y);
            }
        }
    }

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astSZPrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn szp_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // How close beyond +-1 sin(theta) may be and still be taken as +-1,
    // `tol` in C.
    const TOL: f64 = 1.0e-13;
    // Below this r^2 the small angle formula is used, the literal in C.
    const SMALL_R2: f64 = 1.0e-10;

    let xp = x * prm.w[0];
    let yp = y * prm.w[0];
    let r2 = xp * xp + yp * yp;

    let x1 = (xp - prm.w[1]) / prm.w[3];
    let y1 = (yp - prm.w[2]) / prm.w[3];
    let sxy = xp * x1 + yp * y1;

    let theta;
    let z;
    if r2 < SMALL_R2 {
        // Use small angle formula.
        z = r2 / 2.0;
        theta = 90.0 - R2D * math::sqrt(r2 / (1.0 + sxy));
    } else {
        let t = x1 * x1 + y1 * y1;
        let a = t + 1.0;
        let b = sxy - t;
        let c = r2 - sxy - sxy + t - 1.0;
        let d = b * b - a * c;

        // Check for a solution.
        if d < 0.0 {
            return (Status::BadPoint, 0.0, 0.0);
        }
        let d = math::sqrt(d);

        // Choose solution closest to pole.
        let sth1 = (-b + d) / a;
        let sth2 = (-b - d) / a;
        let mut sthe = c_max(sth1, sth2);
        if sthe > 1.0 {
            if sthe - 1.0 < TOL {
                sthe = 1.0;
            } else {
                sthe = c_min(sth1, sth2);
            }
        }

        if sthe < -1.0 && sthe + 1.0 > -TOL {
            sthe = -1.0;
        }

        if sthe > 1.0 || sthe < -1.0 {
            return (Status::BadPoint, 0.0, 0.0);
        }

        theta = T::asind(sthe);

        z = 1.0 - sthe;
    }

    let phi = T::atan2d(xp - x1 * z, -(yp - y1 * z));

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// TAN: gnomonic
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astTANset; reference/starlink-ast at 3d3f255f.
pub(super) fn tan_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    Status::Ok
}

// vendor/wcslib/proj.c, astTANfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn tan_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let s = T::sind(theta);

    let r = prm.r0 * T::cosd(theta) / s;
    let x = r * T::sind(phi);
    let y = -r * T::cosd(phi);

    // C returns for s == 0 before computing x and y, and for s < 0 after; the
    // outputs of a bad point are not used.
    if s <= 0.0 {
        return (Status::BadPoint, x, y);
    }

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astTANrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn tan_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let r = math::sqrt(x * x + y * y);
    let phi = if r == 0.0 { 0.0 } else { T::atan2d(x, -y) };
    let theta = T::atan2d(prm.r0, r);

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// STG: stereographic
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astSTGset; reference/starlink-ast at 3d3f255f.
pub(super) fn stg_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 360.0 / PI;
        prm.w[1] = PI / 360.0;
    } else {
        prm.w[0] = 2.0 * prm.r0;
        prm.w[1] = 1.0 / prm.w[0];
    }

    Status::Ok
}

// vendor/wcslib/proj.c, astSTGfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn stg_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let s = 1.0 + T::sind(theta);

    let r = prm.w[0] * T::cosd(theta) / s;
    let x = r * T::sind(phi);
    let y = -r * T::cosd(phi);

    if s == 0.0 {
        return (Status::BadPoint, x, y);
    }

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astSTGrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn stg_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let r = math::sqrt(x * x + y * y);
    let phi = if r == 0.0 { 0.0 } else { T::atan2d(x, -y) };
    let theta = 90.0 - 2.0 * T::atand(r * prm.w[1]);

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// SIN: orthographic/synthesis
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astSINset; reference/starlink-ast at 3d3f255f.
pub(super) fn sin_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    prm.w[0] = 1.0 / prm.r0;
    prm.w[1] = prm.p[1] * prm.p[1] + prm.p[2] * prm.p[2];
    prm.w[2] = prm.w[1] + 1.0;
    prm.w[3] = prm.w[1] - 1.0;

    Status::Ok
}

// vendor/wcslib/proj.c, astSINfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn sin_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    // Below this angle from the pole, in radians, the small angle formulae
    // are used, the literal in C.
    const SMALL_T: f64 = 1.0e-5;

    let mut t = (90.0 - theta.abs()) * D2R;
    let z;
    let cthe;
    if t < SMALL_T {
        if theta > 0.0 {
            z = t * t / 2.0;
        } else {
            z = 2.0 - t * t / 2.0;
        }
        cthe = t;
    } else {
        z = 1.0 - T::sind(theta);
        cthe = T::cosd(theta);
    }

    let cphi = T::cosd(phi);
    let sphi = T::sind(phi);
    let x = prm.r0 * (cthe * sphi + prm.p[1] * z);
    let y = -prm.r0 * (cthe * cphi - prm.p[2] * z);

    // Validate this solution.
    if prm.w[1] == 0.0 {
        // Orthographic projection.
        if theta < 0.0 {
            return (Status::BadPoint, x, y);
        }
    } else {
        // "Synthesis" projection.
        t = -T::atand(prm.p[1] * sphi - prm.p[2] * cphi);
        if theta < t {
            return (Status::BadPoint, x, y);
        }
    }

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astSINrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn sin_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // How close beyond +-1 sin(theta) may be and still be taken as +-1,
    // `tol` in C.
    const TOL: f64 = 1.0e-13;
    // Below this r^2 the small angle formula is used, the literal in C.
    const SMALL_R2: f64 = 1.0e-10;

    // Compute intermediaries.
    let x0 = x * prm.w[0];
    let y0 = y * prm.w[0];
    let r2 = x0 * x0 + y0 * y0;

    let phi;
    let theta;
    if prm.w[1] == 0.0 {
        // Orthographic projection.
        phi = if r2 != 0.0 { T::atan2d(x0, -y0) } else { 0.0 };

        if r2 < 0.5 {
            theta = T::acosd(math::sqrt(r2));
        } else if r2 <= 1.0 {
            theta = T::asind(math::sqrt(1.0 - r2));
        } else {
            return (Status::BadPoint, phi, 0.0);
        }
    } else {
        // "Synthesis" projection.
        let x1 = prm.p[1];
        let y1 = prm.p[2];
        let sxy = x0 * x1 + y0 * y1;

        let z;
        if r2 < SMALL_R2 {
            // Use small angle formula.
            z = r2 / 2.0;
            theta = 90.0 - R2D * math::sqrt(r2 / (1.0 + sxy));
        } else {
            let a = prm.w[2];
            let b = sxy - prm.w[1];
            let c = r2 - sxy - sxy + prm.w[3];
            let d = b * b - a * c;

            // Check for a solution.
            if d < 0.0 {
                return (Status::BadPoint, 0.0, 0.0);
            }
            let d = math::sqrt(d);

            // Choose solution closest to pole.
            let sth1 = (-b + d) / a;
            let sth2 = (-b - d) / a;
            let mut sthe = c_max(sth1, sth2);
            if sthe > 1.0 {
                if sthe - 1.0 < TOL {
                    sthe = 1.0;
                } else {
                    sthe = c_min(sth1, sth2);
                }
            }

            if sthe < -1.0 && sthe + 1.0 > -TOL {
                sthe = -1.0;
            }

            if sthe > 1.0 || sthe < -1.0 {
                return (Status::BadPoint, 0.0, 0.0);
            }

            theta = T::asind(sthe);
            z = 1.0 - sthe;
        }

        let xp = -y0 + prm.p[2] * z;
        let yp = x0 - prm.p[1] * z;
        phi = if xp == 0.0 && yp == 0.0 {
            0.0
        } else {
            T::atan2d(yp, xp)
        };
    }

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// ARC: zenithal/azimuthal equidistant
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astARCset; reference/starlink-ast at 3d3f255f.
pub(super) fn arc_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

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

// vendor/wcslib/proj.c, astARCfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn arc_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let r = prm.w[0] * (90.0 - theta);
    let x = r * T::sind(phi);
    let y = -r * T::cosd(phi);

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astARCrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn arc_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let r = math::sqrt(x * x + y * y);
    let phi = if r == 0.0 { 0.0 } else { T::atan2d(x, -y) };
    let theta = 90.0 - r * prm.w[1];

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// ZPN: zenithal/azimuthal polynomial
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astZPNset; reference/starlink-ast at 3d3f255f.
// `plen`, C's `astSizeOf(prj->p)/sizeof(double)`, is the number of slots in
// a parameter array.
pub(super) fn zpn_set(prm: &mut PrjPrm) -> Status {
    // How close to zero the derivative must be, in the search for the point
    // of inflection, to be taken as zero; `tol` in C.
    const TOL: f64 = 1.0e-13;
    // The number of degrees searched for the point where the derivative
    // first goes negative, the literal in C.
    const MAX_DEGREES: i32 = 180;
    // The number of iterations that find where the derivative is zero, the
    // literal in C.
    const MAX_ITER: i32 = 10;

    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    // Find the highest non-zero coefficient.
    let plen = PRJ_PARAM_SLOTS as i32;
    let mut k = plen - 1;
    while k >= 0 && prm.p[k as usize] == 0.0 {
        k -= 1;
    }
    if k < 0 {
        return Status::BadParams;
    }

    prm.n = k;

    if k < 2 {
        // No point of inflection for polynomials of degree < 2.
        prm.w[0] = PI;
        prm.w[1] = 0.0;
    } else {
        // Find the point of inflection closest to the pole.
        let mut zd1 = 0.0;
        let mut d1 = prm.p[1];
        if d1 <= 0.0 {
            return Status::BadParams;
        }

        // Find the point where the derivative first goes negative.
        let mut zd2 = 0.0;
        let mut d2 = 0.0;
        let mut i = 0;
        while i < MAX_DEGREES {
            zd2 = f64::from(i) * D2R;
            d2 = 0.0;
            let mut j = k;
            while j > 0 {
                d2 = d2 * zd2 + f64::from(j) * prm.p[j as usize];
                j -= 1;
            }

            if d2 <= 0.0 {
                break;
            }
            zd1 = zd2;
            d1 = d2;
            i += 1;
        }

        let mut zd;
        if i == MAX_DEGREES {
            // No negative derivative -> no point of inflection.
            zd = PI;
        } else {
            // Find where the derivative is zero.
            zd = 0.0;
            let mut i = 1;
            while i <= MAX_ITER {
                zd = zd1 - d1 * (zd2 - zd1) / (d2 - d1);

                let mut d = 0.0;
                let mut j = k;
                while j > 0 {
                    d = d * zd + f64::from(j) * prm.p[j as usize];
                    j -= 1;
                }

                if d.abs() < TOL {
                    break;
                }

                if d < 0.0 {
                    zd2 = zd;
                    d2 = d;
                } else {
                    zd1 = zd;
                    d1 = d;
                }
                i += 1;
            }
        }

        let mut r = 0.0;
        let mut j = k;
        while j >= 0 {
            r = r * zd + prm.p[j as usize];
            j -= 1;
        }
        prm.w[0] = zd;
        prm.w[1] = r;
    }

    Status::Ok
}

// vendor/wcslib/proj.c, astZPNfwd; reference/starlink-ast at 3d3f255f.
// The loop runs from the degree `n` down, as AST's modification of wcslib
// has it, not from the largest degree a parameter array holds.
#[inline(always)]
pub(super) fn zpn_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let s = (90.0 - theta) * D2R;

    let mut r = 0.0;
    let mut j = prm.n;
    while j >= 0 {
        r = r * s + prm.p[j as usize];
        j -= 1;
    }
    r *= prm.r0;

    let x = r * T::sind(phi);
    let y = -r * T::cosd(phi);

    if s > prm.w[0] {
        return (Status::BadPoint, x, y);
    }

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astZPNrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn zpn_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // How far beyond a limit a root may be and still be taken as the limit,
    // and how close a polynomial value must be to the target to end the
    // search; `tol` in C.
    const TOL: f64 = 1.0e-13;
    // The most interval divisions the iterative solution makes, the
    // literal in C.
    const MAX_ITER: i32 = 100;
    // The bounds on the weight of an interval division, the literals in C.
    const LAMBDA_MIN: f64 = 0.1;
    const LAMBDA_MAX: f64 = 0.9;

    let k = prm.n;

    let r = math::sqrt(x * x + y * y) / prm.r0;

    let zd;
    if k < 1 {
        // Constant - no solution.
        return (Status::BadParams, 0.0, 0.0);
    } else if k == 1 {
        // Linear.
        zd = (r - prm.p[0]) / prm.p[1];
    } else if k == 2 {
        // Quadratic.
        let a = prm.p[2];
        let b = prm.p[1];
        let c = prm.p[0] - r;

        let d = b * b - 4.0 * a * c;
        if d < 0.0 {
            return (Status::BadPoint, 0.0, 0.0);
        }
        let d = math::sqrt(d);

        // Choose solution closest to pole.
        let zd1 = (-b + d) / (2.0 * a);
        let zd2 = (-b - d) / (2.0 * a);
        let mut z = c_min(zd1, zd2);
        if z < -TOL {
            z = c_max(zd1, zd2);
        }
        if z < 0.0 {
            if z < -TOL {
                return (Status::BadPoint, 0.0, 0.0);
            }
            z = 0.0;
        } else if z > PI {
            if z > PI + TOL {
                return (Status::BadPoint, 0.0, 0.0);
            }
            z = PI;
        }
        zd = z;
    } else {
        // Higher order - solve iteratively.
        let mut zd1 = 0.0;
        let mut r1 = prm.p[0];
        let mut zd2 = prm.w[0];
        let mut r2 = prm.w[1];

        let mut z = 0.0;
        if r < r1 {
            if r < r1 - TOL {
                return (Status::BadPoint, 0.0, 0.0);
            }
            z = zd1;
        } else if r > r2 {
            if r > r2 + TOL {
                return (Status::BadPoint, 0.0, 0.0);
            }
            z = zd2;
        } else {
            // Disect the interval.
            let mut j = 0;
            while j < MAX_ITER {
                let mut lambda = (r2 - r) / (r2 - r1);
                lambda = lambda.clamp(LAMBDA_MIN, LAMBDA_MAX);

                z = zd2 - lambda * (zd2 - zd1);

                let mut rt = 0.0;
                let mut i = k;
                while i >= 0 {
                    rt = (rt * z) + prm.p[i as usize];
                    i -= 1;
                }

                if rt < r {
                    if r - rt < TOL {
                        break;
                    }
                    r1 = rt;
                    zd1 = z;
                } else {
                    if rt - r < TOL {
                        break;
                    }
                    r2 = rt;
                    zd2 = z;
                }

                if (zd2 - zd1).abs() < TOL {
                    break;
                }
                j += 1;
            }
        }
        zd = z;
    }

    let phi = if r == 0.0 { 0.0 } else { T::atan2d(x, -y) };
    let theta = 90.0 - zd * R2D;

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// ZEA: zenithal/azimuthal equal area
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astZEAset; reference/starlink-ast at 3d3f255f.
pub(super) fn zea_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 360.0 / PI;
        prm.w[1] = PI / 360.0;
    } else {
        prm.w[0] = 2.0 * prm.r0;
        prm.w[1] = 1.0 / prm.w[0];
    }

    Status::Ok
}

// vendor/wcslib/proj.c, astZEAfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn zea_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let r = prm.w[0] * T::sind((90.0 - theta) / 2.0);
    let x = r * T::sind(phi);
    let y = -r * T::cosd(phi);

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astZEArev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn zea_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // How close r may be to the edge of the map, in the units of r, and be
    // taken as the south pole; `tol` in C.
    const TOL: f64 = 1.0e-12;

    let r = math::sqrt(x * x + y * y);
    let phi = if r == 0.0 { 0.0 } else { T::atan2d(x, -y) };

    let s = r * prm.w[1];
    let theta;
    if s.abs() > 1.0 {
        if (r - prm.w[0]).abs() < TOL {
            theta = -90.0;
        } else {
            return (Status::BadPoint, phi, 0.0);
        }
    } else {
        theta = 90.0 - 2.0 * T::asind(s);
    }

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// AIR: Airy
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astAIRset; reference/starlink-ast at 3d3f255f.
pub(super) fn air_set(prm: &mut PrjPrm) -> Status {
    // Below this xi, in radians, the forward projection uses its small
    // angle form; `tol` in C.
    const TOL: f64 = 1.0e-4;

    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    prm.w[0] = 2.0 * prm.r0;
    if prm.p[1] == 90.0 {
        prm.w[1] = -0.5;
        prm.w[2] = 1.0;
    } else if prm.p[1] > -90.0 {
        let cxi = Libm::cosd((90.0 - prm.p[1]) / 2.0);
        prm.w[1] = math::ln(cxi) * (cxi * cxi) / (1.0 - cxi * cxi);
        prm.w[2] = 0.5 - prm.w[1];
    } else {
        return Status::BadParams;
    }

    prm.w[3] = prm.w[0] * prm.w[2];
    prm.w[4] = TOL;
    prm.w[5] = prm.w[2] * TOL;
    prm.w[6] = R2D / prm.w[2];

    Status::Ok
}

// vendor/wcslib/proj.c, astAIRfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn air_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let r;
    if theta == 90.0 {
        r = 0.0;
    } else if theta > -90.0 {
        let xi = D2R * (90.0 - theta) / 2.0;
        if xi < prm.w[4] {
            r = xi * prm.w[3];
        } else {
            let cxi = T::cosd((90.0 - theta) / 2.0);
            let txi = math::sqrt(1.0 - cxi * cxi) / cxi;
            r = -prm.w[0] * (math::ln(cxi) / txi + prm.w[1] * txi);
        }
    } else {
        return (Status::BadPoint, 0.0, 0.0);
    }

    let x = r * T::sind(phi);
    let y = -r * T::cosd(phi);

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astAIRrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn air_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // How close the radius of the trial solution must be to the target to
    // end the search; `tol` in C.
    const TOL: f64 = 1.0e-12;
    // The most halvings of the bracketing interval, the literal in C.
    const MAX_BRACKET: i32 = 30;
    // The most interval divisions of the solution, the literal in C.
    const MAX_ITER: i32 = 100;
    // The bounds on the weight of an interval division, the literals in C.
    const LAMBDA_MIN: f64 = 0.1;
    const LAMBDA_MAX: f64 = 0.9;

    let r = math::sqrt(x * x + y * y) / prm.w[0];

    let xi;
    if r == 0.0 {
        xi = 0.0;
    } else if r < prm.w[5] {
        xi = r * prm.w[6];
    } else {
        // Find a solution interval.
        let mut x1 = 1.0;
        let mut r1 = 0.0;
        let mut x2 = 0.0;
        let mut r2 = 0.0;
        let mut j = 0;
        while j < MAX_BRACKET {
            x2 = x1 / 2.0;
            let txi = math::sqrt(1.0 - x2 * x2) / x2;
            r2 = -(math::ln(x2) / txi + prm.w[1] * txi);

            if r2 >= r {
                break;
            }
            x1 = x2;
            r1 = r2;
            j += 1;
        }
        if j == MAX_BRACKET {
            return (Status::BadPoint, 0.0, 0.0);
        }

        let mut cxi = 0.0;
        let mut j = 0;
        while j < MAX_ITER {
            // Weighted division of the interval.
            let mut lambda = (r2 - r) / (r2 - r1);
            lambda = lambda.clamp(LAMBDA_MIN, LAMBDA_MAX);
            cxi = x2 - lambda * (x2 - x1);

            let txi = math::sqrt(1.0 - cxi * cxi) / cxi;
            let rt = -(math::ln(cxi) / txi + prm.w[1] * txi);

            if rt < r {
                if r - rt < TOL {
                    break;
                }
                r1 = rt;
                x1 = cxi;
            } else {
                if rt - r < TOL {
                    break;
                }
                r2 = rt;
                x2 = cxi;
            }
            j += 1;
        }
        if j == MAX_ITER {
            return (Status::BadPoint, 0.0, 0.0);
        }

        xi = T::acosd(cxi);
    }

    let phi = if r == 0.0 { 0.0 } else { T::atan2d(x, -y) };
    let theta = 90.0 - 2.0 * xi;

    (Status::Ok, phi, theta)
}

#[cfg(test)]
mod tests {
    use super::super::WcsProjection;
    use super::super::difftest::check_projection;

    /// The parameter sets: the defaults, then (axis, m, value) lists.
    type Sets = Vec<Vec<(usize, usize, f64)>>;

    fn lat(pv: &[(usize, f64)]) -> Vec<(usize, usize, f64)> {
        pv.iter().map(|&(m, v)| (2, m, v)).collect()
    }

    fn sets(lists: &[&[(usize, f64)]]) -> Sets {
        let mut out: Sets = vec![Vec::new()];
        out.extend(lists.iter().map(|l| lat(l)));
        out
    }

    #[test]
    fn zenithal_projections_match_vendored_c() {
        use WcsProjection::*;
        // Parameter sets the WcsMap tests use, and unusable ones.
        check_projection(Tan, &sets(&[]));
        check_projection(Stg, &sets(&[]));
        check_projection(Arc, &sets(&[]));
        check_projection(Zea, &sets(&[]));
        check_projection(
            Azp,
            &sets(&[
                &[(1, -1.0)],
                &[(1, 0.5)],
                &[(1, 2.0)],
                &[(1, 3.0), (2, 30.0)],
                &[(1, 0.0), (2, -45.0)],
                &[(1, 0.5), (2, 90.0)],
                &[(1, -1.0), (2, 0.0)],
                &[(2, 20.0)],
            ]),
        );
        check_projection(
            Szp,
            &sets(&[
                &[(1, 0.5)],
                &[(1, 2.0), (2, 30.0), (3, 60.0)],
                &[(1, -3.0), (2, 200.0), (3, 45.0)],
                &[(1, 0.1), (2, 10.0), (3, 80.0)],
                // w[3] == 0: unusable.
                &[(1, -2.0), (3, 30.0)],
            ]),
        );
        check_projection(
            Sin,
            &sets(&[
                &[(1, 0.5)],
                &[(1, 0.0)],
                &[(1, 0.1), (2, 0.2)],
                &[(1, -0.3), (2, 0.7)],
                &[(2, 0.4)],
            ]),
        );
        check_projection(
            Zpn,
            &sets(&[
                &[(1, 1.0)],
                &[(0, 0.1), (1, 1.0)],
                &[(1, 1.0), (3, 0.05)],
                &[(1, 1.27118), (2, -0.210643)],
                &[(1, 1.0), (2, 0.2)],
                &[(1, 1.0), (2, -0.1), (3, 0.02)],
                &[(1, 1.0), (3, -0.5), (5, 0.1)],
                &[
                    (1, 0.9),
                    (2, 0.1),
                    (3, 0.01),
                    (4, -0.003),
                    (5, 0.001),
                    (7, 0.0001),
                ],
                &[(1, 1.0), (30, 1e-5)],
                &[(1, 1.0), (100, 1e-9)],
                &[(0, 2.0)],
                &[(0, 2.0), (1, 1.0)],
                // p[1] <= 0 with a higher degree: unusable.
                &[(1, -1.0), (2, 0.1)],
                // All coefficients zero: unusable.
                &[(1, 0.0)],
            ]),
        );
        check_projection(
            Air,
            &sets(&[
                &[(1, 45.0)],
                &[(1, 0.0)],
                &[(1, -45.0)],
                &[(1, 89.0)],
                &[(1, 90.0)],
                &[(1, -89.9)],
                // PV2_1 <= -90: unusable.
                &[(1, -90.0)],
                &[(1, -120.0)],
            ]),
        );
    }
}

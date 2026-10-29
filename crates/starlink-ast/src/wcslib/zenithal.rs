// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The zenithal projections: AZP, SZP, TAN, STG, SIN, ARC, ZPN, ZEA and AIR.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//! Every function lists its source in `reference/starlink-ast` at 3d3f255f.
//!
//! The tests of the form `prj->flag > 0` in the C bodies are applied
//! unconditionally, as [`super::prj`] explains.

// The bodies keep the shape of the C they are ported from, which declares
// each output before the branches that assign it.
#![allow(clippy::needless_late_init)]

use super::math;

use super::PRJ_PARAM_SLOTS;
use super::prj::{PrjPrm, Status};
use super::trig::{D2R, Libm, R2D, Trig, atan2d_covers, atand_covers, sincosd_covers};
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

// reference/starlink-ast/wcslib/proj.c, astAZPset; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astAZPfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn azp_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let cphi = T::cosd(phi);
    let stage = azp_fwd_front_at::<T>(prm, phi, theta, cphi);
    azp_fwd_divergence::<T>(prm, theta, || azp_fwd_s(prm, cphi), stage)
}

/// `s` of [`azp_fwd`], from the cosine of `phi`.
#[inline(always)]
fn azp_fwd_s(prm: &PrjPrm, cphi: f64) -> f64 {
    prm.w[1] * cphi
}

/// The two terms whose sum is the denominator `t` of [`azp_fwd`], from the
/// sine and cosine of `theta` and `s`.
#[inline(always)]
fn azp_fwd_t_terms(prm: &PrjPrm, sthe: f64, cthe: f64, s: f64) -> (f64, f64) {
    (prm.p[1] + sthe, cthe * s)
}

/// The first stage of [`azp_fwd`], all of its trigonometry but that of the
/// divergence test: the projection, bad where its denominator is zero or
/// `theta` is in the overlap.
#[inline(always)]
pub(super) fn azp_fwd_front<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    azp_fwd_front_at::<T>(prm, phi, theta, T::cosd(phi))
}

/// [`azp_fwd_front`] given the cosine of `phi`.
// C returns at `t == 0` before it computes x and y, and the outputs it
// returns are then (0, 0); the statuses are selects, so that the body has no
// branch on the point.
#[inline(always)]
fn azp_fwd_front_at<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64, cphi: f64) -> (Status, f64, f64) {
    let cthe = T::cosd(theta);
    let (a, b) = azp_fwd_t_terms(prm, T::sind(theta), cthe, azp_fwd_s(prm, cphi));
    let t = a + b;

    let r = prm.w[0] * cthe / t;
    let x = r * T::sind(phi);
    let y = -r * cphi * prm.w[2];

    // The zero denominator, then bounds checking: the overlap.
    if t == 0.0 {
        (Status::BadPoint, 0.0, 0.0)
    } else if theta < prm.w[5] {
        (Status::BadPoint, x, y)
    } else {
        (Status::Ok, x, y)
    }
}

/// The rest of [`azp_fwd`]: the divergence test of the projection `stage`
/// from [`azp_fwd_front`], given `s` from the trigonometry `T`.
#[inline(always)]
fn azp_fwd_divergence<T: Trig>(
    prm: &PrjPrm,
    theta: f64,
    s: impl FnOnce() -> f64,
    stage: (Status, f64, f64),
) -> (Status, f64, f64) {
    let (status, x, y) = stage;
    if status != Status::Ok {
        return stage;
    }

    // Divergence.
    if prm.w[7] > 0.0 {
        let s = s();
        let t = prm.p[1] / math::sqrt(1.0 + s * s);

        if t.abs() <= 1.0 {
            let s = T::atand(-s);
            let t = T::asind(t);
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

/// [`azp_fwd`]'s second stage in a slice: the divergence test, with `s`
/// from [`Libm`], so its status is the per-value entry's.
#[inline(always)]
#[allow(clippy::unnecessary_wraps)]
pub(super) fn azp_fwd_finish(
    prm: &PrjPrm,
    phi: f64,
    theta: f64,
    stage: (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    Some(azp_fwd_divergence::<Libm>(
        prm,
        theta,
        || azp_fwd_s(prm, Libm::cosd(phi)),
        stage,
    ))
}

/// Whether [`azp_fwd_front`] needs [`Libm`] at (`phi`, `theta`): a sine or
/// cosine argument outside the kernels' cover, or a denominator so near
/// zero that the kernels' drift could change whether it is zero.
// The statuses and branches of the body:
// - `t == 0`, from the kernels' values: decided by `azp_fwd_t_near_zero`;
// - `theta < w[5]`, from the input and the parameters alone;
// - the divergence test, `w[7] > 0` from the parameters, then `|t| <= 1`
//   and `theta < max(a, b)` from `s`: in `azp_fwd_finish`, with `s` from
//   `Libm`, so they are the per-value entry's tests on its values.
// So the kernels cannot turn a bad point good or a good one bad.
#[inline(always)]
pub(super) fn azp_fwd_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    !sincosd_covers(theta)
        | !sincosd_covers(phi)
        | azp_fwd_t_near_zero(prm, T::sind(theta), T::cosd(theta), T::cosd(phi))
}

/// Whether the denominator `t` of [`azp_fwd`], from the kernels' sine and
/// cosine of `theta` and cosine of `phi`, is so near zero that [`Libm`]'s
/// could be zero where it is not, or not where it is.
// `t` is `(p1 + sthe) + cthe s`, with `s = w1 cphi`. Each kernel sine and
// cosine is within 2 ULP of Libm's (see `Kernel`), at most 2 eps of it (eps
// = 2^-52), and each path rounds each operation once, at most eps / 2
// relative, so eps between the two paths:
// - `p1 + sthe` differs by at most 2 eps |sthe| + eps |p1 + sthe|;
// - `s` by at most 3 eps |s|, and `cthe s` by at most 6 eps |cthe s|;
// - their sum by a further eps |t|.
// That is eps (2 |sthe| + |p1 + sthe| + 6 |cthe s| + |t|), to first order.
// The margin is twice that, which covers its own evaluation from the
// kernels' values rather than Libm's. Where |t| is beyond it, Libm's `t` has
// the kernels' sign and neither is zero.
#[inline(always)]
pub(super) fn azp_fwd_t_near_zero(prm: &PrjPrm, sthe: f64, cthe: f64, cphi: f64) -> bool {
    let (a, b) = azp_fwd_t_terms(prm, sthe, cthe, azp_fwd_s(prm, cphi));
    let t = a + b;
    let margin = 2.0 * f64::EPSILON * (2.0 * sthe.abs() + a.abs() + 6.0 * b.abs() + t.abs());
    // False for a NaN, which is then redone.
    let clear_of_zero = t.abs() > margin;
    !clear_of_zero
}

// reference/starlink-ast/wcslib/proj.c, astAZPrev; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astSZPset; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astSZPfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn szp_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let cphi = T::cosd(phi);
    let sphi = T::sind(phi);
    let stage = szp_fwd_front_at::<T>(prm, theta, sphi, cphi);
    szp_fwd_overlap::<T>(prm, theta, || (sphi, cphi), stage)
}

/// The first stage of [`szp_fwd`], all of its trigonometry but that of the
/// overlap test: the projection, bad where its denominator is zero or
/// `theta` is beyond the divergence.
#[inline(always)]
pub(super) fn szp_fwd_front<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    szp_fwd_front_at::<T>(prm, theta, T::sind(phi), T::cosd(phi))
}

/// [`szp_fwd_front`] given the sine and cosine of `phi`.
// C returns at `t == 0` before it computes x and y, and the outputs it
// returns are then (0, 0); the statuses are selects, so that the body has no
// branch on the point.
#[inline(always)]
fn szp_fwd_front_at<T: Trig>(prm: &PrjPrm, theta: f64, sphi: f64, cphi: f64) -> (Status, f64, f64) {
    let cthe = T::cosd(theta);
    let s = 1.0 - T::sind(theta);

    let t = prm.w[3] - s;

    let x = (prm.w[6] * cthe * sphi - prm.w[4] * s) / t;
    let y = -(prm.w[6] * cthe * cphi + prm.w[5] * s) / t;

    // The zero denominator, then bounds checking: the divergence.
    if t == 0.0 {
        (Status::BadPoint, 0.0, 0.0)
    } else if theta < prm.w[8] {
        (Status::BadPoint, x, y)
    } else {
        (Status::Ok, x, y)
    }
}

/// The rest of [`szp_fwd`]: the overlap test of the projection `stage` from
/// [`szp_fwd_front`], given the sine and cosine of `phi` from the
/// trigonometry `T`.
#[inline(always)]
fn szp_fwd_overlap<T: Trig>(
    prm: &PrjPrm,
    theta: f64,
    sincos_phi: impl FnOnce() -> (f64, f64),
    stage: (Status, f64, f64),
) -> (Status, f64, f64) {
    let (status, x, y) = stage;
    if status != Status::Ok {
        return stage;
    }

    // Overlap.
    if prm.p[1].abs() > 1.0 {
        let (sphi, cphi) = sincos_phi();
        let s = prm.w[1] * sphi - prm.w[2] * cphi;
        let t = 1.0 / math::sqrt(prm.w[7] + s * s);

        if t.abs() <= 1.0 {
            let s = T::atan2d(s, prm.w[3] - 1.0);
            let t = T::asind(t);
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

/// [`szp_fwd`]'s second stage in a slice: the overlap test, with the sine
/// and cosine of `phi` from [`Libm`], so its status is the per-value
/// entry's.
#[inline(always)]
#[allow(clippy::unnecessary_wraps)]
pub(super) fn szp_fwd_finish(
    prm: &PrjPrm,
    phi: f64,
    theta: f64,
    stage: (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    Some(szp_fwd_overlap::<Libm>(
        prm,
        theta,
        || (Libm::sind(phi), Libm::cosd(phi)),
        stage,
    ))
}

/// Whether [`szp_fwd_front`] needs [`Libm`] at (`phi`, `theta`): a sine or
/// cosine argument outside the kernels' cover, or a denominator so near
/// zero that the kernels' drift could change whether it is zero.
// The statuses and branches of the body:
// - `t == 0`, from the kernels' sine of `theta`: decided by
//   `szp_fwd_t_near_zero`;
// - `theta < w[8]`, from the input and the parameters alone;
// - the overlap test, `|p1| > 1` from the parameters, then `|t| <= 1` and
//   `theta < max(a, b)` from the sine and cosine of `phi`: in
//   `szp_fwd_finish`, with those from `Libm`, so they are the per-value
//   entry's tests on its values.
// So the kernels cannot turn a bad point good or a good one bad.
#[inline(always)]
pub(super) fn szp_fwd_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    !sincosd_covers(theta) | !sincosd_covers(phi) | szp_fwd_t_near_zero(prm, T::sind(theta))
}

/// Whether the denominator `t` of [`szp_fwd`], from the kernels' sine of
/// `theta`, is so near zero that [`Libm`]'s could be zero where it is not,
/// or not where it is.
// `t` is `w3 - s`, with `s = 1 - sthe`. The kernels' sine is within 2 ULP of
// Libm's (see `Kernel`), at most 2 eps of it (eps = 2^-52), and each path
// rounds each operation once, at most eps / 2 relative, so eps between the
// two paths: `s` differs by at most 2 eps |sthe| + eps |s|, and `t` by a
// further eps |t|. That is eps (2 |sthe| + |s| + |t|), to first order. The
// margin is twice that, which covers its own evaluation from the kernels'
// values rather than Libm's. Where |t| is beyond it, Libm's `t` has the
// kernels' sign and neither is zero.
#[inline(always)]
pub(super) fn szp_fwd_t_near_zero(prm: &PrjPrm, sthe: f64) -> bool {
    let s = 1.0 - sthe;
    let t = prm.w[3] - s;
    let margin = 2.0 * f64::EPSILON * (2.0 * sthe.abs() + s.abs() + t.abs());
    // False for a NaN, which is then redone.
    let clear_of_zero = t.abs() > margin;
    !clear_of_zero
}

// reference/starlink-ast/wcslib/proj.c, astSZPrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn szp_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    szp_rev_rest::<T>(prm, x, y, szp_rev_front::<T>(prm, x, y))
}

/// Below this r^2 [`szp_rev`] uses the small angle formula.
// The literal in C.
const SZP_REV_SMALL_R2: f64 = 1.0e-10;

/// The scaled (x, y) of [`szp_rev`], their r^2, and whether the small angle
/// formula applies.
#[inline(always)]
fn szp_rev_scaled(prm: &PrjPrm, x: f64, y: f64) -> (f64, f64, f64, bool) {
    let xp = x * prm.w[0];
    let yp = y * prm.w[0];
    let r2 = xp * xp + yp * yp;
    (xp, yp, r2, r2 < SZP_REV_SMALL_R2)
}

/// All of [`szp_rev`] but its trigonometry: the status, the two arguments
/// of the arc tangent that gives `phi`, and `theta` where the small angle
/// formula applies or its sine elsewhere.
// For a branch-free `T` both the small angle formula and the quadratic are
// computed and one is selected, and the statuses are selects, so that the
// body has no branch on the point; for `Libm` each is computed only where C
// computes it. A negative discriminant gives a NaN root, which is computed
// and unused. The arithmetic is C's, operation for operation.
#[inline(always)]
fn szp_rev_solve<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64, f64) {
    // How close beyond +-1 sin(theta) may be and still be taken as +-1,
    // `tol` in C.
    const TOL: f64 = 1.0e-13;

    let (xp, yp, r2, small) = szp_rev_scaled(prm, x, y);

    let x1 = (xp - prm.w[1]) / prm.w[3];
    let y1 = (yp - prm.w[2]) / prm.w[3];
    let sxy = xp * x1 + yp * y1;

    // Use small angle formula.
    let (z_small, theta_small) = if T::BRANCH_FREE || small {
        (r2 / 2.0, 90.0 - R2D * math::sqrt(r2 / (1.0 + sxy)))
    } else {
        (0.0, 0.0)
    };

    let (solved, sthe) = if T::BRANCH_FREE || !small {
        let t = x1 * x1 + y1 * y1;
        let a = t + 1.0;
        let b = sxy - t;
        let c = r2 - sxy - sxy + t - 1.0;
        let d = b * b - a * c;

        // Check for a solution.
        let no_root = d < 0.0;
        let d = math::sqrt(d);

        // Choose solution closest to pole.
        let sth1 = (-b + d) / a;
        let sth2 = (-b - d) / a;
        let mut sthe = c_max(sth1, sth2);
        if sthe > 1.0 {
            sthe = if sthe - 1.0 < TOL {
                1.0
            } else {
                c_min(sth1, sth2)
            };
        }

        if sthe < -1.0 && sthe + 1.0 > -TOL {
            sthe = -1.0;
        }

        (!no_root & !(sthe > 1.0 || sthe < -1.0), sthe)
    } else {
        (true, 0.0)
    };

    let z = if small { z_small } else { 1.0 - sthe };
    let status = if small || solved {
        Status::Ok
    } else {
        Status::BadPoint
    };

    (
        status,
        xp - x1 * z,
        -(yp - y1 * z),
        if small { theta_small } else { sthe },
    )
}

/// The first stage of [`szp_rev`], all of it but the arc sine: the status,
/// `phi`, and `theta` where the small angle formula applies or its sine
/// elsewhere.
#[inline(always)]
pub(super) fn szp_rev_front<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let (status, phi_y, phi_x, theta) = szp_rev_solve::<T>(prm, x, y);
    (status, T::atan2d(phi_y, phi_x), theta)
}

/// The rest of [`szp_rev`] at (`x`, `y`), given the result of
/// [`szp_rev_front`]: `theta` from its sine where the small angle formula
/// does not apply.
// C returns at a bad point before it computes either output, and the
// outputs it returns are then (0, 0).
#[inline(always)]
fn szp_rev_rest<T: Trig>(
    prm: &PrjPrm,
    x: f64,
    y: f64,
    (status, phi, theta): (Status, f64, f64),
) -> (Status, f64, f64) {
    if status != Status::Ok {
        return (status, 0.0, 0.0);
    }
    let (.., small) = szp_rev_scaled(prm, x, y);
    let theta = if small { theta } else { T::asind(theta) };

    (Status::Ok, phi, theta)
}

/// [`szp_rev`]'s second stage in a slice: [`szp_rev_rest`] with [`Libm`] on
/// the kernels' `phi`, which it never declines.
// Every status test and branch of the body is decided from values computed
// without trigonometry, operation for operation as `Libm` computes them, so
// the kernels cannot change them; the arc sine is `Libm`'s, so `theta` is
// the per-value entry's bits and only `phi` moves.
#[inline(always)]
#[allow(clippy::unnecessary_wraps)]
pub(super) fn szp_rev_finish(
    prm: &PrjPrm,
    x: f64,
    y: f64,
    stage: (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    Some(szp_rev_rest::<Libm>(prm, x, y, stage))
}

/// Whether [`szp_rev_front`] needs [`Libm`] at (`x`, `y`): an argument of
/// the arc tangent outside its kernel's cover, which includes the NaN
/// arguments of a bad point.
#[inline(always)]
pub(super) fn szp_rev_needs_special<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> bool {
    let (_, phi_y, phi_x, _) = szp_rev_solve::<T>(prm, x, y);
    !atan2d_covers(phi_y, phi_x)
}

// ---------------------------------------------------------------------------
// TAN: gnomonic
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astTANset; reference/starlink-ast at 3d3f255f.
pub(super) fn tan_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 90.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
    }

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astTANfwd; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astTANrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn tan_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let r = math::sqrt(x * x + y * y);
    let phi = if r == 0.0 { 0.0 } else { T::atan2d(x, -y) };
    let theta = T::atan2d(prm.r0, r);

    (Status::Ok, phi, theta)
}

// The predicate combines its tests with `|`, which evaluates both sides, so
// it vectorizes with the body.

/// Whether [`tan_fwd`] needs [`Libm`] at (`phi`, `theta`): a sine or cosine
/// argument outside the kernels' cover.
// The body's only decision is its status, from the sign of `sind(theta)`,
// and it has no branch. A covered `theta` is not a multiple of 180, so its
// sine is not zero, and the kernel's sine, within 2 ULP of libm's (see
// `Kernel`), has that sine's sign: no value within 2 ULP of a non-zero
// double has the other sign, except beside a sine of at most 2 subnormal
// units, an argument so small that the kernel and libm both return it. The
// kernel cannot turn a bad point good or a good one bad, so the predicate
// needs only the covers.
#[inline(always)]
pub(super) fn tan_fwd_needs_special<T: Trig>(_prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    !sincosd_covers(theta) | !sincosd_covers(phi)
}

// ---------------------------------------------------------------------------
// STG: stereographic
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astSTGset; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astSTGfwd; reference/starlink-ast at 3d3f255f.
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

/// Whether [`stg_fwd`] needs [`Libm`] at (`phi`, `theta`): a sine or cosine
/// argument outside the kernels' cover, or a sine of `theta` so near -1 that
/// the kernels' drift could change whether `s` is zero.
// The body's only decision is its status, `s == 0`, with `s = 1 +
// sind(theta)` from the kernels: decided by `stg_fwd_s_near_zero`. It has no
// branch.
#[inline(always)]
pub(super) fn stg_fwd_needs_special<T: Trig>(_prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    !sincosd_covers(theta) | !sincosd_covers(phi) | stg_fwd_s_near_zero(1.0 + T::sind(theta))
}

/// Whether `s`, one plus the kernels' sine of `theta` in [`stg_fwd`], is so
/// near zero that [`Libm`]'s could be zero where it is not, or not where it
/// is.
// A covered `theta` is not a multiple of 90, but its sine still rounds to
// -1 within about 6e-7 degrees of -90. The kernels' sine is within 2 ULP of
// Libm's (see `Kernel`), and the doubles in (-1, -0.5] are 2^-53 apart. So
// where the kernels' sine is at least -1 + 3 * 2^-53, Libm's is at least -1 +
// 2^-53, and both sums, exact there (Sterbenz), are positive. The kernels'
// sum is a multiple of 2^-53 there, so that is where it is above 2^-52 =
// eps; every other point, a sum at or below zero among them, is redone.
#[inline(always)]
pub(super) fn stg_fwd_s_near_zero(s: f64) -> bool {
    // False for a NaN, which is then redone.
    let clear_of_zero = s > f64::EPSILON;
    !clear_of_zero
}

// reference/starlink-ast/wcslib/proj.c, astSTGrev; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astSINset; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astSINfwd; reference/starlink-ast at 3d3f255f.
// C tests `prj->w[1] == 0.0` for the validation only; the test is made once
// here, which picks a body with that branch resolved.
#[inline(always)]
pub(super) fn sin_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    if prm.w[1] == 0.0 {
        sin_fwd_as::<T, false>(prm, phi, theta)
    } else {
        sin_fwd_as::<T, true>(prm, phi, theta)
    }
}

/// Below this angle from the pole, in radians, [`sin_fwd`] uses the small
/// angle formulae, the literal in C.
const SIN_SMALL_T: f64 = 1.0e-5;

/// [`sin_fwd`] for the orthographic projection, or for the synthesis
/// projection when `SYNTHESIS` is set (`prm.w[1]` not zero).
// For a branch-free `T`, both of C's forms of `z` and `cthe` are computed
// and one is selected, and the status is a select, so that the body has no
// branch on the point and vectorizes; for `Libm` the sine and cosine of
// `theta` are computed only where C computes them. The outputs are C's.
#[inline(always)]
fn sin_fwd_as<T: Trig, const SYNTHESIS: bool>(
    prm: &PrjPrm,
    phi: f64,
    theta: f64,
) -> (Status, f64, f64) {
    let t = (90.0 - theta.abs()) * D2R;
    let small = t < SIN_SMALL_T;
    let z_small = if theta > 0.0 {
        t * t / 2.0
    } else {
        2.0 - t * t / 2.0
    };
    let (z_full, cthe_full) = if T::BRANCH_FREE || !small {
        (1.0 - T::sind(theta), T::cosd(theta))
    } else {
        (0.0, 0.0)
    };
    let z = if small { z_small } else { z_full };
    let cthe = if small { t } else { cthe_full };

    let cphi = T::cosd(phi);
    let sphi = T::sind(phi);
    let x = prm.r0 * (cthe * sphi + prm.p[1] * z);
    let y = -prm.r0 * (cthe * cphi - prm.p[2] * z);

    // Validate this solution: the orthographic projection is bad beyond the
    // equator, and the synthesis projection beyond the native latitude
    // where the projection plane is tangent to the sphere.
    let bad = if SYNTHESIS {
        theta < -T::atand(sin_fwd_limit_tan(prm, sphi, cphi))
    } else {
        theta < 0.0
    };
    let status = if bad { Status::BadPoint } else { Status::Ok };

    (status, x, y)
}

/// The tangent whose arc tangent, negated, is the synthesis projection's
/// lowest valid native latitude at the longitude with sine `sphi` and
/// cosine `cphi`.
#[inline(always)]
fn sin_fwd_limit_tan(prm: &PrjPrm, sphi: f64, cphi: f64) -> f64 {
    prm.p[1] * sphi - prm.p[2] * cphi
}

/// [`sin_fwd`] for the orthographic projection.
#[inline(always)]
pub(super) fn sin_fwd_orthographic<T: Trig>(
    prm: &PrjPrm,
    phi: f64,
    theta: f64,
) -> (Status, f64, f64) {
    sin_fwd_as::<T, false>(prm, phi, theta)
}

/// [`sin_fwd`] for the synthesis projection.
#[inline(always)]
pub(super) fn sin_fwd_synthesis<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    sin_fwd_as::<T, true>(prm, phi, theta)
}

/// Whether [`sin_fwd_orthographic`] needs [`Libm`] at (`phi`, `theta`): a
/// sine or cosine argument outside the kernels' cover. The sine and cosine
/// of `theta` are used only away from the pole.
// The status is decided from `theta` alone, so the kernels cannot change it.
#[inline(always)]
pub(super) fn sin_fwd_orthographic_needs_special<T: Trig>(
    _prm: &PrjPrm,
    phi: f64,
    theta: f64,
) -> bool {
    let near_pole = (90.0 - theta.abs()) * D2R < SIN_SMALL_T;
    !sincosd_covers(phi) | (!near_pole & !sincosd_covers(theta))
}

/// Whether [`sin_fwd_synthesis`] needs [`Libm`] at (`phi`, `theta`): as
/// [`sin_fwd_orthographic_needs_special`], and where the arc tangent's
/// argument is outside its kernel's cover or `theta` is so near the lowest
/// valid latitude that the kernels' drift could change the status.
// The status is `theta < limit`, where `limit` is `-atand(p1 sind(phi) -
// p2 cosd(phi))`, all of it from the kernels in the slice. Their limit
// differs from Libm's by at most:
// - the arc tangent's argument: each of the sine and cosine is within 2 ULP
//   of Libm's (see `Kernel`), at most 2 * 2^-52 for a value of magnitude at
//   most 1, so p1 sind and p2 cosd differ by at most 2 * 2^-52 (|p1| + |p2|)
//   before rounding; each path rounds the two products and the difference
//   once, at most 2^-53 of |p1|, |p2| and the argument, which is at most
//   |p1| + |p2|, so 2^-52 (|p1| + |p2|) per path and 2 * 2^-52 (|p1| + |p2|)
//   for both; the arguments differ by at most 4 * 2^-52 (|p1| + |p2|);
// - the arc tangent, whose derivative is at most 1, carries that over, and
//   adds 1 ULP of the kernel from libm's and 2 ULP of libm's from the exact
//   value at the two arguments, at most 3 * 2^-52 as the result is less than
//   2 in magnitude;
// - the product with R2D, rounded on each path, at most 2^-47 on each since
//   the limit is at most 90 in magnitude.
// That is R2D * 2^-52 * (3 + 4 (|p1| + |p2|)) + 2^-46 degrees; the margin
// used is twice that. Where `theta` is further than the margin from the
// kernels' limit, it is on the same side of Libm's.
#[inline(always)]
pub(super) fn sin_fwd_synthesis_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    let a = sin_fwd_limit_tan(prm, T::sind(phi), T::cosd(phi));
    let limit = -T::atand(a);
    let margin = 2.0
        * (R2D * f64::EPSILON * (3.0 + 4.0 * (prm.p[1].abs() + prm.p[2].abs()))
            + 64.0 * f64::EPSILON);
    // False for a NaN, which is then redone.
    let clear_of_limit = (theta - limit).abs() > margin;
    sin_fwd_orthographic_needs_special::<T>(prm, phi, theta) | !atand_covers(a) | !clear_of_limit
}

// reference/starlink-ast/wcslib/proj.c, astSINrev; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astARCset; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astARCfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn arc_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let r = prm.w[0] * (90.0 - theta);
    let x = r * T::sind(phi);
    let y = -r * T::cosd(phi);

    (Status::Ok, x, y)
}

/// Whether [`arc_fwd`] needs [`Libm`] at (`phi`, `theta`): a sine or cosine
/// argument outside the kernels' cover.
// The body has no status test and no branch.
#[inline(always)]
pub(super) fn arc_fwd_needs_special<T: Trig>(_prm: &PrjPrm, phi: f64, _theta: f64) -> bool {
    !sincosd_covers(phi)
}

// reference/starlink-ast/wcslib/proj.c, astARCrev; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astZPNset; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astZPNfwd; reference/starlink-ast at 3d3f255f.
// The loop runs from the degree `n` down, as AST's modification of wcslib
// has it, not from the largest degree a parameter array holds.
// C evaluates the polynomial before the sine and cosine; neither depends on
// the other.
#[inline(always)]
pub(super) fn zpn_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    zpn_fwd_rest(prm, theta, zpn_fwd_front::<T>(prm, phi, theta))
}

/// The first stage of [`zpn_fwd`], its only trigonometry: the sine and
/// cosine of `phi`, with [`Status::Ok`].
#[inline(always)]
pub(super) fn zpn_fwd_front<T: Trig>(_prm: &PrjPrm, phi: f64, _theta: f64) -> (Status, f64, f64) {
    (Status::Ok, T::sind(phi), T::cosd(phi))
}

/// The rest of [`zpn_fwd`] at `theta`, given the sine and cosine of `phi`
/// from [`zpn_fwd_front`]: the polynomial, its product with them, and the
/// status.
#[inline(always)]
fn zpn_fwd_rest(
    prm: &PrjPrm,
    theta: f64,
    (_, sphi, cphi): (Status, f64, f64),
) -> (Status, f64, f64) {
    let s = (90.0 - theta) * D2R;

    let mut r = 0.0;
    let mut j = prm.n;
    while j >= 0 {
        r = r * s + prm.p[j as usize];
        j -= 1;
    }
    r *= prm.r0;

    let x = r * sphi;
    let y = -r * cphi;

    if s > prm.w[0] {
        return (Status::BadPoint, x, y);
    }

    (Status::Ok, x, y)
}

/// [`zpn_fwd`]'s second stage in a slice: [`zpn_fwd_rest`] on the kernels'
/// sine and cosine of `phi`, which it never declines.
// The status is `s > w[0]`, from `theta` alone, so the kernels cannot change
// it, and the iteration-free polynomial has no other branch on the point.
#[inline(always)]
#[allow(clippy::unnecessary_wraps)]
pub(super) fn zpn_fwd_finish(
    prm: &PrjPrm,
    _phi: f64,
    theta: f64,
    stage: (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    Some(zpn_fwd_rest(prm, theta, stage))
}

/// Whether [`zpn_fwd_front`] needs [`Libm`] at (`phi`, `theta`): a sine or
/// cosine argument outside the kernels' cover.
#[inline(always)]
pub(super) fn zpn_fwd_needs_special<T: Trig>(_prm: &PrjPrm, phi: f64, _theta: f64) -> bool {
    !sincosd_covers(phi)
}

// reference/starlink-ast/wcslib/proj.c, astZPNrev; reference/starlink-ast at 3d3f255f.
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
                // C's branches, not `f64::clamp`: the branches are predicted where
                // clamp's min and max lengthen the iteration's dependency chain.
                #[allow(clippy::manual_clamp)]
                if lambda < LAMBDA_MIN {
                    lambda = LAMBDA_MIN;
                } else if lambda > LAMBDA_MAX {
                    lambda = LAMBDA_MAX;
                }

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

// reference/starlink-ast/wcslib/proj.c, astZEAset; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astZEAfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn zea_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let r = prm.w[0] * T::sind((90.0 - theta) / 2.0);
    let x = r * T::sind(phi);
    let y = -r * T::cosd(phi);

    (Status::Ok, x, y)
}

/// Whether [`zea_fwd`] needs [`Libm`] at (`phi`, `theta`): a sine or cosine
/// argument outside the kernels' cover.
// The body has no status test and no branch.
#[inline(always)]
pub(super) fn zea_fwd_needs_special<T: Trig>(_prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    !sincosd_covers((90.0 - theta) / 2.0) | !sincosd_covers(phi)
}

// reference/starlink-ast/wcslib/proj.c, astZEArev; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astAIRset; reference/starlink-ast at 3d3f255f.
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

// reference/starlink-ast/wcslib/proj.c, astAIRfwd; reference/starlink-ast at 3d3f255f.
// C computes the radius before the sine and cosine of `phi`; neither depends
// on the other.
#[inline(always)]
pub(super) fn air_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    air_fwd_rest::<T>(prm, theta, air_fwd_front::<T>(prm, phi, theta))
}

/// The first stage of [`air_fwd`]: the sine and cosine of `phi`, with
/// [`Status::Ok`].
#[inline(always)]
pub(super) fn air_fwd_front<T: Trig>(_prm: &PrjPrm, phi: f64, _theta: f64) -> (Status, f64, f64) {
    (Status::Ok, T::sind(phi), T::cosd(phi))
}

/// The rest of [`air_fwd`] at `theta`, given the sine and cosine of `phi`
/// from [`air_fwd_front`]: the radius, its product with them, and the
/// status.
#[inline(always)]
fn air_fwd_rest<T: Trig>(
    prm: &PrjPrm,
    theta: f64,
    (_, sphi, cphi): (Status, f64, f64),
) -> (Status, f64, f64) {
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

    let x = r * sphi;
    let y = -r * cphi;

    (Status::Ok, x, y)
}

/// [`air_fwd`]'s second stage in a slice: [`air_fwd_rest`] with [`Libm`] on
/// the kernels' sine and cosine of `phi`, which it never declines.
// The status and the branches are decided from `theta` alone, so the kernels
// cannot change them.
#[inline(always)]
#[allow(clippy::unnecessary_wraps)]
pub(super) fn air_fwd_finish(
    prm: &PrjPrm,
    _phi: f64,
    theta: f64,
    stage: (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    Some(air_fwd_rest::<Libm>(prm, theta, stage))
}

/// Whether [`air_fwd_front`] needs [`Libm`] at (`phi`, `theta`): a sine or
/// cosine argument outside the kernels' cover.
#[inline(always)]
pub(super) fn air_fwd_needs_special<T: Trig>(_prm: &PrjPrm, phi: f64, _theta: f64) -> bool {
    !sincosd_covers(phi)
}

// reference/starlink-ast/wcslib/proj.c, astAIRrev; reference/starlink-ast at 3d3f255f.
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
            // C's branches, not `f64::clamp`: the branches are predicted where
            // clamp's min and max lengthen the iteration's dependency chain.
            #[allow(clippy::manual_clamp)]
            if lambda < LAMBDA_MIN {
                lambda = LAMBDA_MIN;
            } else if lambda > LAMBDA_MAX {
                lambda = LAMBDA_MAX;
            }
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

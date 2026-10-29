// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The conic projections: COP, COE, COD and COO.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//!
//! The bounds tests apply unconditionally, as [`super::prj`] explains.

// Lineage: every function lists its source in `reference/starlink-ast` at
// 3d3f255f; the `prj->flag > 0` tests of the C bodies are applied
// unconditionally.

use super::math;

use super::prj::{PrjPrm, Status};
use super::trig::{D2R, Libm, R2D, Trig, atan2d_covers, sincosd_covers, sincosd_near_zero};

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
// C returns at `s == 0` before it computes anything else, and computes the
// cone's radius only away from the poles; here both statuses are selects,
// so that the body has no branch on the point, and the radius is computed
// at the poles too when `T::BRANCH_FREE` is set.
#[inline(always)]
pub(super) fn cop_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let t = theta - prm.p[1];
    let s = T::cosd(t);

    let pole = theta.abs() == 90.0;
    let r_cone = if T::BRANCH_FREE || !pole {
        prm.w[2] - prm.w[3] * T::sind(t) / s
    } else {
        0.0
    };
    let (r, bad) = if pole {
        // The pole on the same side as the standard parallels is the apex
        // of the cone, and the other is on the far side.
        (0.0, (theta < 0.0) != (prm.p[1] < 0.0))
    } else {
        (r_cone, r_cone * prm.w[0] < 0.0)
    };

    let a = prm.w[0] * phi;

    let x = r * T::sind(a);
    let y = prm.w[2] - r * T::cosd(a);

    if s == 0.0 {
        (Status::BadPoint, 0.0, 0.0)
    } else if bad {
        (Status::BadPoint, x, y)
    } else {
        (Status::Ok, x, y)
    }
}

/// Whether [`cop_fwd`] needs [`Libm`] at (`phi`, `theta`): a sine or cosine
/// argument outside the kernels' cover, or a value deciding a status so near
/// its threshold that the kernels' drift could carry it across.
// The body has two status decisions and no branch. `s == 0`, with `s` the
// kernels' cosine of `t = theta - p1`, is decided by `sincosd_near_zero`.
// `r w0 < 0`, with `r` the cone's radius from the kernels' sine and cosine
// of `t`, is decided by `cop_fwd_r_w0_near_zero`, except at the poles,
// where the radius is zero and the status depends on `theta` and `p1`
// alone. The choice between the pole's radius and the cone's depends on
// `theta` alone.
#[inline(always)]
pub(super) fn cop_fwd_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    let t = theta - prm.p[1];
    let s = T::cosd(t);
    let pole = theta.abs() == 90.0;
    !sincosd_covers(t)
        | !sincosd_covers(prm.w[0] * phi)
        | sincosd_near_zero(s)
        | (!pole & cop_fwd_r_w0_near_zero(prm, T::sind(t), s))
}

/// Whether `r w0` in [`cop_fwd`], with the cone's radius `r` from the
/// kernels' sine `st` and cosine `s` of `t`, is so near zero that
/// [`Libm`]'s could have the other sign or be zero.
// `r = w2 - q`, `q = (w3 st) / s`. The kernels' sine and cosine are each
// within 2 ULP of Libm's (see `Kernel`), at most 2 eps relative (eps =
// 2^-52), and each path rounds each operation once, at most eps / 2
// relative, so eps between the two paths. To first order the product `w3
// st` differs by 3 eps of itself, the quotient `q` by 3 + 2 + 1 = 6 eps of
// itself, `r` by 6 eps |q| + eps |r|, and `P = r w0` by |w0| times that
// plus eps |P|, and by 2^-1074 more where the product is subnormal: at most
// eps (6 |w0 q| + 2 |P|) + 2^-1074. The margin is twice that, which covers
// its own evaluation from the kernels' values rather than Libm's. Where |P|
// is beyond it, Libm's `P` has the kernels' sign and neither is zero.
#[inline(always)]
pub(super) fn cop_fwd_r_w0_near_zero(prm: &PrjPrm, st: f64, s: f64) -> bool {
    let q = prm.w[3] * st / s;
    let p = (prm.w[2] - q) * prm.w[0];
    let margin =
        2.0 * (f64::EPSILON * (6.0 * (prm.w[0] * q).abs() + 2.0 * p.abs()) + f64::from_bits(1));
    // False for a NaN, which is then redone.
    let clear_of_zero = p.abs() > margin;
    !clear_of_zero
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
// The radius away from the south pole is computed at the pole too when
// `T::BRANCH_FREE` is set, so that the body has no branch on the point.
#[inline(always)]
pub(super) fn coe_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let a = phi * prm.w[0];
    let south = theta == -90.0;
    let r_north = if T::BRANCH_FREE || !south {
        prm.w[3] * math::sqrt(prm.w[4] - prm.w[5] * T::sind(theta))
    } else {
        0.0
    };
    let r = if south { prm.w[8] } else { r_north };

    let x = r * T::sind(a);
    let y = prm.w[2] - r * T::cosd(a);

    (Status::Ok, x, y)
}

/// Whether [`coe_fwd`] needs [`Libm`] at (`phi`, `theta`): a sine or cosine
/// argument outside the kernels' cover, or a square root's argument so near
/// zero that the kernels' drift could change its sign.
// The body has no status test, and its one branch, at the south pole,
// depends on `theta` alone. Its square root is of `w4 - w5 sind(theta)`,
// which is NaN where that is negative: decided by `coe_fwd_radicand_near_zero`.
// It is zero only where a standard parallel is at a pole and `theta` is at
// that pole.
#[inline(always)]
pub(super) fn coe_fwd_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    !sincosd_covers(theta)
        | !sincosd_covers(phi * prm.w[0])
        | coe_fwd_radicand_near_zero(prm, T::sind(theta))
}

/// Whether `w4 - w5 sthe`, the argument of the square root in [`coe_fwd`],
/// from the kernels' sine of `theta`, is so near zero that [`Libm`]'s could
/// have the other sign or be zero.
// The kernels' sine is within 2 ULP of Libm's (see `Kernel`), at most 2 eps
// relative (eps = 2^-52), and each path rounds each operation once, at most
// eps / 2 relative, so eps between the two paths: the product `w5 sthe`
// differs by at most 3 eps of itself and the difference by a further eps of
// itself, eps (3 |w5 sthe| + |radicand|) to first order. The margin is twice
// that, which covers its own evaluation from the kernels' values rather than
// Libm's. Where the radicand is beyond it, Libm's has the kernels' sign and
// neither is zero.
#[inline(always)]
pub(super) fn coe_fwd_radicand_near_zero(prm: &PrjPrm, sthe: f64) -> bool {
    let product = prm.w[5] * sthe;
    let radicand = prm.w[4] - product;
    let margin = 2.0 * f64::EPSILON * (3.0 * product.abs() + radicand.abs());
    // False for a NaN, which is then redone.
    let clear_of_zero = radicand.abs() > margin;
    !clear_of_zero
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

/// Whether [`cod_fwd`] needs [`Libm`] at (`phi`, `theta`): a sine or cosine
/// argument outside the kernels' cover.
// The body has no status test and no branch.
#[inline(always)]
pub(super) fn cod_fwd_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, _theta: f64) -> bool {
    !sincosd_covers(prm.w[0] * phi)
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
#[inline(always)]
pub(super) fn coo_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    coo_fwd_rest::<T>(prm, theta, coo_fwd_front::<T>(prm, phi, theta))
}

/// The first stage of [`coo_fwd`]: the sine and cosine of `w0 phi`, with
/// [`Status::Ok`].
#[inline(always)]
pub(super) fn coo_fwd_front<T: Trig>(prm: &PrjPrm, phi: f64, _theta: f64) -> (Status, f64, f64) {
    let a = prm.w[0] * phi;
    (Status::Ok, T::sind(a), T::cosd(a))
}

/// The rest of [`coo_fwd`] at `theta`, given the sine and cosine of `w0 phi`
/// from [`coo_fwd_front`]: the radius, its product with them, and the
/// status.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
fn coo_fwd_rest<T: Trig>(
    prm: &PrjPrm,
    theta: f64,
    (_, sa, ca): (Status, f64, f64),
) -> (Status, f64, f64) {
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

    let x = r * sa;
    let y = prm.w[2] - r * ca;

    (Status::Ok, x, y)
}

/// [`coo_fwd`]'s second stage in a slice: [`coo_fwd_rest`] with [`Libm`] on
/// the kernels' sine and cosine of `w0 phi`, which it never declines.
// The status and the branches are decided from `theta` and the parameters
// alone, so the kernels cannot change them.
#[inline(always)]
#[allow(clippy::unnecessary_wraps)]
pub(super) fn coo_fwd_finish(
    prm: &PrjPrm,
    _phi: f64,
    theta: f64,
    stage: (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    Some(coo_fwd_rest::<Libm>(prm, theta, stage))
}

/// Whether [`coo_fwd_front`] needs [`Libm`] at (`phi`, `theta`): a sine or
/// cosine argument outside the kernels' cover.
#[inline(always)]
pub(super) fn coo_fwd_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, _theta: f64) -> bool {
    !sincosd_covers(prm.w[0] * phi)
}

// reference/starlink-ast/wcslib/proj.c, astCOOrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn coo_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    coo_rev_rest::<T>(prm, coo_rev_front::<T>(prm, x, y))
}

/// The first stage of [`coo_rev`]: [`Status::Ok`], the longitude and the
/// signed radius `r`. The arc tangent is computed at `r == 0` too when
/// `T::BRANCH_FREE` is set, so that the stage has no branch on the point.
#[inline(always)]
pub(super) fn coo_rev_front<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let dy = prm.w[2] - y;
    let mut r = math::sqrt(x * x + dy * dy);
    if prm.p[1] < 0.0 {
        r = -r;
    }

    let a_cone = if T::BRANCH_FREE || r != 0.0 {
        T::atan2d(x / r, dy / r)
    } else {
        0.0
    };
    let a = if r == 0.0 { 0.0 } else { a_cone };

    (Status::Ok, a * prm.w[1], r)
}

/// The rest of [`coo_rev`], given the longitude and radius from
/// [`coo_rev_front`]: the latitude and the status.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
fn coo_rev_rest<T: Trig>(prm: &PrjPrm, (_, phi, r): (Status, f64, f64)) -> (Status, f64, f64) {
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

/// [`coo_rev`]'s second stage in a slice: [`coo_rev_rest`] with [`Libm`] on
/// the kernels' longitude, which it never declines.
// The status and the branch are decided from `r` and the parameters alone,
// and `r` is computed alike on both paths, so the latitude is the per-value
// entry's bit for bit.
#[inline(always)]
#[allow(clippy::unnecessary_wraps)]
pub(super) fn coo_rev_finish(
    prm: &PrjPrm,
    _x: f64,
    _y: f64,
    stage: (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    Some(coo_rev_rest::<Libm>(prm, stage))
}

/// Whether [`coo_rev_front`] needs [`Libm`] at (`x`, `y`): an arc tangent's
/// arguments outside the kernels' cover.
// The stage has no status test, and its one branch, at `r == 0`, depends on
// the input alone; there `x / r` is NaN, which the cover rejects.
#[inline(always)]
pub(super) fn coo_rev_needs_special<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> bool {
    let dy = prm.w[2] - y;
    let mut r = math::sqrt(x * x + dy * dy);
    if prm.p[1] < 0.0 {
        r = -r;
    }
    !atan2d_covers(x / r, dy / r)
}

/// The bound on how far the longitude `phi` of [`coo_rev`] from the
/// kernels' arc tangent can be from [`Libm`]'s.
// `phi = a w1`, with `a` the kernels' arc tangent, within 2 ULP of Libm's
// (see `Kernel`), at most 2 eps of it (eps = 2^-52), so it differs by at
// most 3 eps of itself with the two paths' roundings of the product, eps
// between them, and by 2^-1074 more where it is subnormal.
#[inline(always)]
pub(super) fn coo_rev_lon_drift(_prm: &PrjPrm, phi: f64) -> f64 {
    3.0 * f64::EPSILON * phi.abs() + f64::from_bits(1)
}

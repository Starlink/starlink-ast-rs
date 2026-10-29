// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The polyconic projections: BON and PCO.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//! Every function lists its source in `reference/starlink-ast` at 3d3f255f.
//!
//! The tests of the form `prj->flag > 0` in the C bodies are applied
//! unconditionally, as [`super::prj`] explains.
//!
//! # BON with a zero standard parallel
//!
//! Bonne's projection with `PV2_1 = 0` is Sanson-Flamsteed's.
//! C tests `p[1] == 0` first in both `astBONfwd` and `astBONrev`, before it
//! looks at `flag`, and hands the call to `astSFLfwd` or `astSFLrev` with
//! BON's own struct.
//! The first of those calls finds `flag` not naming SFL and runs
//! `astSFLset` on the struct, which sets `phi0`, `theta0`, `r0` and `w[0]`
//! and `w[1]` and leaves `flag` naming SFL.
//! `astBONset` never runs for such a struct, because every BON call tests
//! `p[1]` before it tests `flag`, and `astSFLset` does not run again,
//! because `flag` keeps naming SFL.
//! Every call in any order therefore sees the constants of SFL set up from
//! the same parameters, which is what [`bon_set`] computes and what
//! [`bon_fwd`] and [`bon_rev`] use.

use super::math;

use super::WcsProjection;
use super::prj::{PrjPrm, Status, phi_near_lon_limits};
use super::pseudocyl::{sfl_fwd, sfl_rev, sfl_set};
use super::trig::{D2R, Libm, R2D, Trig, atan2d_covers, sincosd_covers, sincosd_near_zero};
use super::zenithal::c_copysign;
use std::f64::consts::PI;

// ---------------------------------------------------------------------------
// BON: Bonne's equal area
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astBONset; reference/starlink-ast at 3d3f255f.
// With `p[1] == 0` the projection is SFL's, whose set-up C runs from
// `astSFLfwd` or `astSFLrev` on BON's struct (see the module documentation).
pub(super) fn bon_set(prm: &mut PrjPrm) -> Status {
    if prm.p[1] == 0.0 {
        return sfl_set(prm);
    }

    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[1] = 1.0;
        prm.w[2] = prm.r0 * Libm::cosd(prm.p[1]) / Libm::sind(prm.p[1]) + prm.p[1];
    } else {
        prm.w[1] = prm.r0 * D2R;
        prm.w[2] = prm.r0 * (Libm::cosd(prm.p[1]) / Libm::sind(prm.p[1]) + prm.p[1] * D2R);
    }

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astBONfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn bon_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    if prm.p[1] == 0.0 {
        // Sanson-Flamsteed.
        return sfl_fwd::<T>(prm, phi, theta);
    }
    bon_fwd_conic::<T>(prm, phi, theta)
}

/// [`bon_fwd`] with a non-zero standard parallel, which is the whole of it
/// but the hand-off to Sanson-Flamsteed's projection.
#[inline(always)]
pub(super) fn bon_fwd_conic<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let r = prm.w[2] - theta * prm.w[1];
    let a = prm.r0 * phi * T::cosd(theta) / r;

    let x = r * T::sind(a);
    let y = prm.w[2] - r * T::cosd(a);

    (Status::Ok, x, y)
}

/// Whether [`bon_fwd_conic`] needs [`Libm`] at (`phi`, `theta`): a sine or
/// cosine argument outside the kernels' cover.
// The body has no status test and no branch. The argument `a` of its second
// sine and cosine comes from the kernels' cosine of `theta`, and is tested
// as the kernels compute it; it is infinite or NaN where `r` is zero.
#[inline(always)]
pub(super) fn bon_fwd_needs_special<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    let r = prm.w[2] - theta * prm.w[1];
    let a = prm.r0 * phi * T::cosd(theta) / r;
    !sincosd_covers(theta) | !sincosd_covers(a)
}

// reference/starlink-ast/wcslib/proj.c, astBONrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn bon_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    if prm.p[1] == 0.0 {
        // Sanson-Flamsteed.
        return sfl_rev::<T>(prm, x, y);
    }
    bon_rev_conic::<T>(prm, x, y)
}

/// [`bon_rev`] with a non-zero standard parallel, which is the whole of it
/// but the hand-off to Sanson-Flamsteed's projection.
// C computes the arc tangent only where `r` is not zero, and the longitude's
// quotient only where `cthe` is not zero; here the arc tangent is computed
// at `r == 0` too when `T::BRANCH_FREE` is set, and the quotient everywhere,
// and the values are selected, so that the body has no branch on the point.
#[inline(always)]
pub(super) fn bon_rev_conic<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
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

    let theta = (prm.w[2] - r) / prm.w[1];
    let cthe = T::cosd(theta);
    let phi_cone = a * (r / prm.r0) / cthe;
    let phi = if cthe == 0.0 { 0.0 } else { phi_cone };

    (Status::Ok, phi, theta)
}

/// Whether [`bon_rev_conic`] needs [`Libm`] at (`x`, `y`): an argument of
/// the arc tangent or the cosine outside the kernels' cover, a cosine so
/// near zero that the kernels' drift could change which longitude the body
/// gives, or a longitude so near a limit of the range a WcsMap's inverse
/// accepts that the drift could change whether it is accepted.
// The body has no status test. Its branch at `r == 0` depends on the input
// alone, and there `x / r` is NaN, which the cover rejects; its branch at
// `cthe == 0`, with `cthe` the kernels' cosine of `theta`, is decided by
// `sincosd_near_zero`. The latitude is computed alike on both paths. The
// longitude `(a (r / r0)) / cthe`, with `a` the kernels' arc tangent, within
// 2 ULP of Libm's (see `Kernel`), at most 2 eps of it (eps = 2^-52), and
// `cthe` within 2 eps, differs by at most 6 eps of itself, with the two
// paths' roundings of the product and the quotient, eps between them each,
// and by 2^-1074 more for each where it is subnormal.
#[inline(always)]
pub(super) fn bon_rev_needs_special<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> bool {
    let dy = prm.w[2] - y;
    let mut r = math::sqrt(x * x + dy * dy);
    if prm.p[1] < 0.0 {
        r = -r;
    }
    let theta = (prm.w[2] - r) / prm.w[1];
    let cthe = T::cosd(theta);
    let phi = T::atan2d(x / r, dy / r) * (r / prm.r0) / cthe;
    let drift = 6.0 * f64::EPSILON * phi.abs() + 2.0 * f64::from_bits(1);
    !atan2d_covers(x / r, dy / r)
        | !sincosd_covers(theta)
        | sincosd_near_zero(cthe)
        | phi_near_lon_limits(phi, drift, WcsProjection::Bon.table_tol())
}

// ---------------------------------------------------------------------------
// PCO: polyconic
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astPCOset; reference/starlink-ast at 3d3f255f.
pub(super) fn pco_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 1.0;
        prm.w[1] = 1.0;
        prm.w[2] = 360.0 / PI;
    } else {
        prm.w[0] = prm.r0 * D2R;
        prm.w[1] = 1.0 / prm.w[0];
        prm.w[2] = 2.0 * prm.r0;
    }
    prm.w[3] = D2R / prm.w[2];

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astPCOfwd; reference/starlink-ast at 3d3f255f,
// with the small-theta formula, which the C comments as upstream 4.22.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn pco_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    // Below this |theta|, in degrees, `cot(theta)` is replaced by a series,
    // 1.0e-4 in C.
    const SMALL_THETA: f64 = 1.0e-4;

    let cthe = T::cosd(theta);
    let sthe = T::sind(theta);
    let a = phi * sthe;

    let x;
    let y;
    if sthe == 0.0 {
        x = prm.w[0] * phi;
        y = 0.0;
    } else if theta.abs() < SMALL_THETA {
        x = prm.w[0] * phi * cthe;
        y = (prm.w[0] + prm.w[3] * x * x) * theta;
    } else {
        let cotthe = cthe / sthe;
        x = prm.r0 * cotthe * T::sind(a);
        y = prm.r0 * (cotthe * (1.0 - T::cosd(a)) + theta * D2R);
    }

    (Status::Ok, x, y)
}

// reference/starlink-ast/wcslib/proj.c, astPCOrev; reference/starlink-ast at 3d3f255f.
// The clamp keeps C's comparisons, which differ from `clamp` only in
// panicking on a bound that is NaN, and the bounds are constants.
#[allow(clippy::manual_clamp)]
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn pco_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    // The tolerance on |y| at the equator and the poles, and the
    // convergence tolerance of the iteration, `tol` in C.
    const TOL: f64 = 1.0e-12;
    // The most iterations of C's weighted division of the interval.
    const MAX_ITERATIONS: usize = 64;
    // C's marker that `fneg` has not been computed yet; anything below
    // -100 selects the equal division of the interval.
    const FNEG_UNSET: f64 = -999.0;
    const FNEG_SET_BELOW: f64 = -100.0;
    // The clamp on the weight of the weighted division, in C.
    const LAMBDA_MIN: f64 = 0.1;
    const LAMBDA_MAX: f64 = 0.9;

    let w = (y * prm.w[1]).abs();
    let phi;
    let theta;
    if w < TOL {
        phi = x * prm.w[1];
        theta = 0.0;
    } else if (w - 90.0).abs() < TOL {
        phi = 0.0;
        theta = c_copysign(90.0, y);
    } else {
        // Iterative solution using weighted division of the interval.
        let mut thepos = if y > 0.0 { 90.0 } else { -90.0 };
        let mut theneg = 0.0;

        let xx = x * x;
        let mut ymthe = y - prm.w[0] * thepos;
        let mut fpos = xx + ymthe * ymthe;
        let mut fneg = FNEG_UNSET;

        let mut th = 0.0;
        let mut tanthe = 0.0;
        for _ in 0..MAX_ITERATIONS {
            if fneg < FNEG_SET_BELOW {
                // Equal division of the interval.
                th = (thepos + theneg) / 2.0;
            } else {
                // Weighted division of the interval.
                let mut lambda = fpos / (fpos - fneg);
                if lambda < LAMBDA_MIN {
                    lambda = LAMBDA_MIN;
                } else if lambda > LAMBDA_MAX {
                    lambda = LAMBDA_MAX;
                }
                th = thepos - lambda * (thepos - theneg);
            }

            // Compute the residue.
            ymthe = y - prm.w[0] * th;
            tanthe = T::tand(th);
            let f = xx + ymthe * (ymthe - prm.w[2] / tanthe);

            // Check for convergence.
            if f.abs() < TOL {
                break;
            }
            if (thepos - theneg).abs() < TOL {
                break;
            }

            // Redefine the interval.
            if f > 0.0 {
                thepos = th;
                fpos = f;
            } else {
                theneg = th;
                fneg = f;
            }
        }

        let xp = prm.r0 - ymthe * tanthe;
        let yp = x * tanthe;
        if xp == 0.0 && yp == 0.0 {
            phi = 0.0;
        } else {
            phi = T::atan2d(yp, xp) / T::sind(th);
        }
        theta = th;
    }

    (Status::Ok, phi, theta)
}

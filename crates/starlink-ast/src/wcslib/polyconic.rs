// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The polyconic projections: BON and PCO.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//! Every function lists its source in `vendor/wcslib/proj.c` and the
//! `reference/starlink-ast` commit that file comes from, 3d3f255f.
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

use super::prj::{PrjPrm, Status};
use super::pseudocyl::{sfl_fwd, sfl_rev, sfl_set};
use super::trig::{D2R, Libm, R2D, Trig};
use super::zenithal::c_copysign;
use std::f64::consts::PI;

// ---------------------------------------------------------------------------
// BON: Bonne's equal area
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astBONset; reference/starlink-ast at 3d3f255f.
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

// vendor/wcslib/proj.c, astBONfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn bon_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    if prm.p[1] == 0.0 {
        // Sanson-Flamsteed.
        return sfl_fwd::<T>(prm, phi, theta);
    }

    let r = prm.w[2] - theta * prm.w[1];
    let a = prm.r0 * phi * T::cosd(theta) / r;

    let x = r * T::sind(a);
    let y = prm.w[2] - r * T::cosd(a);

    (Status::Ok, x, y)
}

// vendor/wcslib/proj.c, astBONrev; reference/starlink-ast at 3d3f255f.
// The body keeps the shape of the C, which declares each output before
// the branches that assign it.
#[allow(clippy::needless_late_init)]
#[inline(always)]
pub(super) fn bon_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    if prm.p[1] == 0.0 {
        // Sanson-Flamsteed.
        return sfl_rev::<T>(prm, x, y);
    }

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

    let theta = (prm.w[2] - r) / prm.w[1];
    let cthe = T::cosd(theta);
    let phi;
    if cthe == 0.0 {
        phi = 0.0;
    } else {
        phi = a * (r / prm.r0) / cthe;
    }

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// PCO: polyconic
// ---------------------------------------------------------------------------

// vendor/wcslib/proj.c, astPCOset; reference/starlink-ast at 3d3f255f.
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

// vendor/wcslib/proj.c, astPCOfwd; reference/starlink-ast at 3d3f255f,
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

// vendor/wcslib/proj.c, astPCOrev; reference/starlink-ast at 3d3f255f.
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

#[cfg(test)]
mod tests {
    use super::super::WcsProjection;
    use super::super::difftest::check_projection;

    #[test]
    fn bonne_matches_vendored_c() {
        use WcsProjection::*;
        // `PV2_1 = 0` is Sanson-Flamsteed: C hands such calls to the SFL
        // functions on BON's own struct, and the check reuses one C struct
        // for forward and reverse calls in any order.
        let mut sets: Vec<Vec<(usize, usize, f64)>> = [-60.0, -10.0, 0.0, 10.0, 45.0, 89.0]
            .iter()
            .map(|&p1| vec![(2, 1, p1)])
            .collect();
        // Parameter 1 missing has no default, so it is not zero.
        sets.push(Vec::new());
        check_projection(Bon, &sets);
    }

    /// With `PV2_1 = 0` every call order gives the bits of the C struct
    /// that SFL's set-up has been run on, however the calls are ordered and
    /// repeated: reverse first, forward first, and each repeated.
    #[test]
    fn bonne_with_zero_parallel_matches_c_in_any_call_order() {
        use super::super::ffi::PrjParams;
        use super::super::prj::Prj;
        use WcsProjection::Bon;

        let params = vec![vec![None, Some(0.0)], Vec::new()];
        let same = |a: f64, b: f64| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan());
        // `F` is a forward call at (phi, theta) and `R` a reverse call at
        // (x, y); the same orders with different points.
        let orders: [&str; 6] = ["FR", "RF", "FFRR", "RRFF", "RFRFRF", "FRFRFR"];
        for order in orders {
            let prj = Prj::new(Bon, &params, false).unwrap();
            let mut c = PrjParams::new(Bon).unwrap();
            c.init_prj_prm(&params, false);
            for (i, call) in order.chars().cycle().take(3 * order.len()).enumerate() {
                let u = f64::from(i as u32) * 17.0 - 40.0;
                let v = f64::from(i as u32) * 5.0 - 30.0;
                if call == 'F' {
                    let (rs, rx, ry) = prj.fwd_raw(u, v);
                    let (cs, cx, cy) = c.forward_raw(u, v);
                    assert_eq!(rs as i32, cs, "{order} call {i}");
                    assert!(same(rx, cx) && same(ry, cy), "{order} call {i} fwd");
                } else {
                    let (rs, rx, ry) = prj.rev_raw(u, v);
                    let (cs, cx, cy) = c.reverse_raw(u, v);
                    assert_eq!(rs as i32, cs, "{order} call {i}");
                    assert!(same(rx, cx) && same(ry, cy), "{order} call {i} rev");
                }
            }
        }
    }

    #[test]
    fn polyconic_matches_vendored_c() {
        use WcsProjection::*;
        check_projection(Pco, &[Vec::new(), vec![(2, 1, 3.0)]]);
    }
}

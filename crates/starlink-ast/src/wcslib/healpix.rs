// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The HEALPix projections: HPX and XPH.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//! The bounds tests apply unconditionally, as [`super::prj`] explains.
//!
//! HPX chooses its facet by integer arithmetic: the parameters H and K are
//! truncated toward zero to `i32` and `%` takes the sign of the dividend.
//! `as i32` and `%` do the same for every value the reference defines, and
//! saturate where its conversion is undefined.

// Lineage: every function below is ported from `reference/starlink-ast/wcslib/proj.c`
// (`reference/starlink-ast` at 3d3f255f); the `flag > 0` tests of the C
// bodies are applied unconditionally.

use super::math;

use super::prj::{PrjPrm, Status};
use super::trig::{D2R, R2D, Trig};

// ---------------------------------------------------------------------------
// HPX: HEALPix
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astHPXset; reference/starlink-ast at 3d3f255f.
pub(super) fn hpx_set(prm: &mut PrjPrm) -> Status {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    prm.n = (prm.p[2] as i32) % 2;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 1.0;
        prm.w[1] = 1.0;
    } else {
        prm.w[0] = prm.r0 * D2R;
        prm.w[1] = R2D / prm.r0;
    }

    prm.w[2] = (prm.p[2] - 1.0) / prm.p[2];
    prm.w[3] = 90.0 * prm.p[2] / prm.p[1];
    prm.w[4] = (prm.p[2] + 1.0) / 2.0;
    prm.w[5] = 90.0 * (prm.p[2] - 1.0) / prm.p[1];
    prm.w[6] = 180.0 / prm.p[1];
    prm.w[7] = prm.p[1] / 360.0;
    prm.w[8] = prm.w[3] * prm.w[0];
    prm.w[9] = prm.w[6] * prm.w[0];

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astHPXfwd; reference/starlink-ast at 3d3f255f.
// phic is conditioned to improve the accuracy of floor for arguments just
// below an integer (AST's change), and the phi = 180 meridian is put in the
// expected place (wcslib 4.7 fix in the C).
#[inline(always)]
pub(super) fn hpx_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    let sinthe = T::sind(theta);
    let abssin = sinthe.abs();

    if abssin <= prm.w[2] {
        // Equatorial zone.
        (Status::Ok, prm.w[0] * phi, prm.w[8] * sinthe)
    } else {
        // Polar zone.
        let mut hodd = (prm.p[1] as i32) % 2;
        if prm.n == 0 && theta <= 0.0 {
            hodd = 1 - hodd;
        }
        let phic = if hodd != 0 {
            -180.0 + (2.0 * (prm.w[7] * phi + 0.5).floor() + prm.p[1]) * prm.w[6]
        } else {
            -180.0 + (2.0 * (prm.w[7] * phi).floor() + prm.p[1] + 1.0) * prm.w[6]
        };

        let sigma = math::sqrt(prm.p[2] * (1.0 - abssin));

        let mut x = prm.w[0] * (phic + (phi - phic) * sigma);

        let mut y = prm.w[9] * (prm.w[4] - sigma);
        if theta < 0.0 {
            y = -y;
        }

        // Put the phi = 180 meridian in the expected place.
        if 180.0 < x {
            x = 360.0 - x;
        }

        (Status::Ok, x, y)
    }
}

// astHPXrev: the polar zone's outer edge, and the facet half-width, may be
// exceeded by this much for rounding (degrees).
const HPX_REV_TOL: f64 = 1.0e-12;
// astHPXrev: the factor that stands in for 1/sigma at the pole, where sigma
// is zero.
const HPX_POLE_SCALE: f64 = 1.0e9;

// reference/starlink-ast/wcslib/proj.c, astHPXrev; reference/starlink-ast at 3d3f255f.
// The polar zone extends to (K+1)/2 facets of 180/H degrees (wcslib 4.7 fix
// in the C).  xc is conditioned as in astHPXfwd (AST's change).  A point is
// on a facet when (x - xc)/sigma is within a facet half-width plus a
// rounding margin (wcslib 4.7 and 4.20 fixes in the C), and y within the
// margin of the pole's counts as the pole (AST's change, because a WcsMap's
// radian round trip moves the pole's y by a few ULP).
#[inline(always)]
pub(super) fn hpx_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let yr = prm.w[1] * y;
    let absy = yr.abs();

    let phi;
    let mut theta;
    if absy <= prm.w[5] {
        // Equatorial zone.
        phi = prm.w[1] * x;
        let t = yr / prm.w[3];
        if t < -1.0 || t > 1.0 {
            return (Status::BadPoint, 0.0, 0.0);
        }
        theta = T::asind(t);
    } else if absy <= prm.w[4] * prm.w[6] + HPX_REV_TOL {
        // Polar zone.
        let mut hodd = (prm.p[1] as i32) % 2;
        if prm.n == 0 && yr <= 0.0 {
            hodd = 1 - hodd;
        }
        let xc = if hodd != 0 {
            -180.0 + (2.0 * (prm.w[7] * x + 0.5).floor() + prm.p[1]) * prm.w[6]
        } else {
            -180.0 + (2.0 * (prm.w[7] * x).floor() + prm.p[1] + 1.0) * prm.w[6]
        };

        let sigma = prm.w[4] - absy / prm.w[6];

        let pole = sigma.abs() * prm.w[6] <= HPX_REV_TOL;
        let t = if pole {
            (x - xc) * HPX_POLE_SCALE
        } else {
            (x - xc) / sigma
        };
        if t.abs() < prm.w[6] + HPX_REV_TOL {
            phi = prm.w[1] * (xc + t);
        } else {
            return (Status::BadPoint, 0.0, 0.0);
        }

        if pole {
            theta = 90.0;
        } else {
            let t = 1.0 - sigma * sigma / prm.p[2];
            if t < -1.0 || t > 1.0 {
                return (Status::BadPoint, 0.0, 0.0);
            }
            theta = T::asind(t);
        }
        if y < 0.0 {
            theta = -theta;
        }
    } else {
        return (Status::BadPoint, 0.0, 0.0);
    }

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// XPH: HEALPix polar ("butterfly")
// ---------------------------------------------------------------------------

// astXPHset: the sigma below which the reverse projection uses the small
// angle formula for theta.
const XPH_SIGMA_TOL: f64 = 1e-4;

// reference/starlink-ast/wcslib/proj.c, astXPHset; reference/starlink-ast at 3d3f255f.
// The C leaves phi0 and theta0 as they were.
pub(super) fn xph_set(prm: &mut PrjPrm) -> Status {
    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 1.0;
        prm.w[1] = 1.0;
    } else {
        prm.w[0] = prm.r0 * D2R;
        prm.w[1] = R2D / prm.r0;
    }

    prm.w[0] /= math::sqrt(2.0);
    prm.w[1] /= math::sqrt(2.0);
    prm.w[2] = 2.0 / 3.0;
    prm.w[3] = XPH_SIGMA_TOL;
    prm.w[4] = math::sqrt(prm.w[2]) * R2D;
    prm.w[5] = 90.0 - prm.w[3] * prm.w[4];
    prm.w[6] = math::sqrt(1.5) * D2R;

    Status::Ok
}

// reference/starlink-ast/wcslib/proj.c, astXPHfwd; reference/starlink-ast at 3d3f255f.
// The C holds phi (rounded) in its output `y` and psi in `x` until the end;
// they are `chi_phi` and `psi` here.
#[inline(always)]
pub(super) fn xph_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    // Do phi dependence.
    let mut chi = phi;
    if 180.0 <= chi.abs() {
        chi %= 360.0;
        if chi < -180.0 {
            chi += 360.0;
        } else if 180.0 <= chi {
            chi -= 360.0;
        }
    }

    // phi is also recomputed from chi to avoid rounding problems.
    chi += 180.0;
    let psi = chi % 90.0;
    let chi_phi = chi - 180.0;

    // Do theta dependence.
    let sinthe = T::sind(theta);
    let abssin = sinthe.abs();

    let mut xi;
    let mut eta;
    if abssin <= prm.w[2] {
        // Equatorial regime.
        xi = psi;
        eta = 67.5 * sinthe;
    } else {
        // Polar regime.
        let sigma = if theta < prm.w[5] {
            math::sqrt(3.0 * (1.0 - abssin))
        } else {
            (90.0 - theta) * prm.w[6]
        };

        xi = 45.0 + (psi - 45.0) * sigma;
        eta = 45.0 * (2.0 - sigma);
        if theta < 0.0 {
            eta = -eta;
        }
    }

    xi -= 45.0;
    eta -= 90.0;

    let (x, y) = if chi_phi < -90.0 {
        (prm.w[0] * (-xi + eta), prm.w[0] * (-xi - eta))
    } else if chi_phi < 0.0 {
        (prm.w[0] * (xi + eta), prm.w[0] * (-xi + eta))
    } else if chi_phi < 90.0 {
        (prm.w[0] * (xi - eta), prm.w[0] * (xi + eta))
    } else {
        (prm.w[0] * (-xi - eta), prm.w[0] * (xi - eta))
    };

    (Status::Ok, x, y)
}

// astXPHrev: a point may lie outside its facet by this much for rounding.
const XPH_REV_TOL: f64 = 1.0e-12;

// reference/starlink-ast/wcslib/proj.c, astXPHrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn xph_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let xr = x * prm.w[1];
    let yr = y * prm.w[1];
    let xi1;
    let eta1;
    let mut phi;
    if xr <= 0.0 && 0.0 < yr {
        xi1 = -xr - yr;
        eta1 = xr - yr;
        phi = -180.0;
    } else if xr < 0.0 && yr <= 0.0 {
        xi1 = xr - yr;
        eta1 = xr + yr;
        phi = -90.0;
    } else if 0.0 <= xr && yr < 0.0 {
        xi1 = xr + yr;
        eta1 = -xr + yr;
        phi = 0.0;
    } else {
        xi1 = -xr + yr;
        eta1 = -xr - yr;
        phi = 90.0;
    }

    let xi = xi1 + 45.0;
    let eta = eta1 + 90.0;
    let abseta = eta.abs();

    let mut theta;
    if abseta <= 90.0 {
        if abseta <= 45.0 {
            // Equatorial regime.
            phi += xi;
            theta = T::asind(eta / 67.5);

            // Bounds checking.
            if 45.0 + XPH_REV_TOL < xi1.abs() {
                return (Status::BadPoint, 0.0, 0.0);
            }
        } else {
            // Polar regime.
            let sigma = (90.0 - abseta) / 45.0;

            // Ensure an exact result for points on the boundary.
            if xr == 0.0 {
                if yr <= 0.0 {
                    phi = 0.0;
                } else {
                    phi = 180.0;
                }
            } else if yr == 0.0 {
                if xr < 0.0 {
                    phi = -90.0;
                } else {
                    phi = 90.0;
                }
            } else {
                phi += 45.0 + xi1 / sigma;
            }

            if sigma < prm.w[3] {
                theta = 90.0 - sigma * prm.w[4];
            } else {
                theta = T::asind(1.0 - sigma * sigma / 3.0);
            }
            if eta < 0.0 {
                theta = -theta;
            }

            // Bounds checking.
            if eta < -45.0 && eta + 90.0 + XPH_REV_TOL < xi1.abs() {
                return (Status::BadPoint, 0.0, 0.0);
            }
        }
    } else {
        // Beyond latitude range.
        return (Status::BadPoint, 0.0, 0.0);
    }

    (Status::Ok, phi, theta)
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palAmpqk (pal/palAmpqk.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::mappa::npb_matrix;
use crate::erfa;

/// Geocentric apparent `(ra, da)` to mean place `(rm, dm)`, in radians with
/// `rm` in 0 to 2 pi, using the parameters from [`super::mappa`]: the
/// inverse of [`super::mapqkz`], found by iterating the aberration twice and
/// the light deflection five times.
pub(crate) fn ampqk(ra: f64, da: f64, amprms: &[f64; 21]) -> (f64, f64) {
    // Unpack some of the parameters.
    let gr2e = amprms[7];
    let ab1 = amprms[11];
    let ehn = [amprms[4], amprms[5], amprms[6]];
    let abv = [amprms[8], amprms[9], amprms[10]];

    // Apparent RA,Dec to Cartesian.
    let p3 = erfa::s2c(ra, da);

    // Precession and nutation.
    let p2 = erfa::trxp(&npb_matrix(amprms), &p3);

    // Aberration.
    let ab1p1 = ab1 + 1.0;
    let mut p1 = p2;
    for _ in 0..2 {
        let p1dv = erfa::pdp(&p1, &abv);
        let p1dvp1 = 1.0 + p1dv;
        let w = 1.0 + p1dv / ab1p1;
        for i in 0..3 {
            p1[i] = (p1dvp1 * p2[i] - w * abv[i]) / ab1;
        }
        let (_w, p3) = erfa::pn(&p1);
        p1 = p3;
    }

    // Light deflection.
    let mut p = p1;
    for _ in 0..5 {
        let pde = erfa::pdp(&p, &ehn);
        let pdep1 = 1.0 + pde;
        let w = pdep1 - gr2e * pde;
        for i in 0..3 {
            p[i] = (pdep1 * p1[i] - gr2e * ehn[i]) / w;
        }
        let (_w, p2) = erfa::pn(&p);
        p = p2;
    }

    // Mean RA,Dec.
    let (rm, dm) = erfa::c2s(p);
    (erfa::anp(rm), dm)
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palMapqkz (pal/palMapqkz.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::mappa::npb_matrix;
use crate::erfa;

/// Mean place `(rm, dm)` to geocentric apparent `(ra, da)`, in radians with
/// `ra` in 0 to 2 pi, for a star with no proper motion, parallax or radial
/// velocity, using the parameters from [`super::mappa`]. Light deflection is
/// restrained within the Sun's disc.
pub(crate) fn mapqkz(rm: f64, dm: f64, amprms: &[f64; 21]) -> (f64, f64) {
    // Unpack scalar and vector parameters.
    let ab1 = amprms[11];
    let gr2e = amprms[7];
    let abv = [amprms[8], amprms[9], amprms[10]];
    let ehn = [amprms[4], amprms[5], amprms[6]];

    // Spherical to x,y,z.
    let p = erfa::s2c(rm, dm);

    // Light deflection (restrained within the Sun's disc).
    let pde = erfa::pdp(&p, &ehn);
    let pdep1 = pde + 1.0;
    let w = gr2e / (if pdep1 > 1.0e-5 { pdep1 } else { 1.0e-5 });
    let mut p1 = [0.0; 3];
    for i in 0..3 {
        p1[i] = p[i] + w * (ehn[i] - pde * p[i]);
    }

    // Aberration.
    let p1dv = erfa::pdp(&p1, &abv);
    let w = 1.0 + p1dv / (ab1 + 1.0);
    let mut p2 = [0.0; 3];
    for i in 0..3 {
        p2[i] = (ab1 * p1[i]) + (w * abv[i]);
    }

    // Precession and nutation.
    let p3 = erfa::rxp(&npb_matrix(amprms), &p2);

    // Geocentric apparent RA,dec.
    let (ra, da) = erfa::c2s(p3);
    (erfa::anp(ra), da)
}

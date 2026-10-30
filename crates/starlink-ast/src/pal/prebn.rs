// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palPrebn (pal/palPrebn.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::PAL__DAS2R;
use super::deuler::deuler;
use crate::erfa::M3;

/// The Besselian (FK4) precession matrix from Besselian epoch `bep0` to
/// Besselian epoch `bep1`, from the Andoyer-Newcomb model.
pub(crate) fn prebn(bep0: f64, bep1: f64) -> M3 {
    // Interval between basic epoch B1850.0 and beginning epoch in TC.
    let bigt = (bep0 - 1850.0) / 100.;

    // Interval over which precession required, in tropical centuries.
    let t = (bep1 - bep0) / 100.;

    // Euler angles.
    let tas2r = t * PAL__DAS2R;
    let w = 2303.5548 + (1.39720 + 0.000059 * bigt) * bigt;

    let zeta = (w + (0.30242 - 0.000269 * bigt + 0.017996 * t) * t) * tas2r;
    let z = (w + (1.09478 + 0.000387 * bigt + 0.018324 * t) * t) * tas2r;
    let theta = (2005.1125
        + (-0.85294 - 0.000365 * bigt) * bigt
        + (-0.42647 - 0.000365 * bigt - 0.041802 * t) * t)
        * tas2r;

    // Rotation matrix.
    deuler("ZYZ", -zeta, theta, -z)
}

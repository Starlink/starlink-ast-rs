// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palRvgalc (pal/palRvgalc.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa;

/// The LSR velocity due to Galactic rotation (220 km/s towards l = 90 degrees, b = 0), as a J2000.0 equatorial vector in km/s.
const VA: [f64; 3] = [-108.70408, 97.86251, -164.33610];

/// The component, in km/s, of the LSR velocity due to Galactic rotation towards the J2000.0 position `(r2000, d2000)`.
pub(crate) fn rvgalc(r2000: f64, d2000: f64) -> f64 {
    let vb = erfa::s2c(r2000, d2000);
    erfa::pdp(&VA, &vb)
}

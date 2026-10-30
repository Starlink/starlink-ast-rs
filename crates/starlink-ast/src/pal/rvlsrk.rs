// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palRvlsrk (pal/palRvlsrk.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa;

/// The standard solar motion (20 km/s towards RA 18h, Dec +30 degrees, 1900), as a J2000.0 equatorial vector in km/s.
const VA: [f64; 3] = [-0.29000, 17.31726, -10.00141];

/// The component, in km/s, of the kinematic LSR velocity towards the J2000.0 position `(r2000, d2000)`.
pub(crate) fn rvlsrk(r2000: f64, d2000: f64) -> f64 {
    let vb = erfa::s2c(r2000, d2000);
    erfa::pdp(&VA, &vb)
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palEcmat (pal/palEcmat.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::PAL__MJD0;
use super::deuler::deuler;
use crate::erfa::{self, M3};

/// The rotation matrix from mean equatorial to mean ecliptic coordinates of the
/// TDB date `date` (a modified Julian date), IAU 2006 model.
pub(crate) fn ecmat(date: f64) -> M3 {
    // Mean obliquity (the angle between the ecliptic and mean equator of date).
    let eps0 = erfa::obl06(PAL__MJD0, date);

    // Matrix.
    deuler("X", eps0, 0.0, 0.0)
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palGeoc (pal/palGeoc.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, Ellipsoid};

/// Geodetic latitude and height to the distance from the Earth's axis and
/// from the equatorial plane, in au (WGS84 ellipsoid, zero longitude).
///
/// The geocentric latitude is `atan2(z, r)`.
pub(crate) fn geoc(p: f64, h: f64) -> (f64, f64) {
    // Use zero longitude.
    let elong = 0.0;
    // This is not `ERFA_DAU`: PAL writes the au as 1.49597870E11 m.
    let au = 1.49597870E11;
    // ERFA's status is ignored, as in PAL; on failure ERFA zeroes the vector.
    let xyz = erfa::gd2gc(Ellipsoid::Wgs84, elong, p, h).unwrap_or([0.0; 3]);
    // PAL divides by `cos(elong)` for a longitude that is the constant 0.
    (xyz[0] / (au * f64::cos(elong)), xyz[2] / au)
}

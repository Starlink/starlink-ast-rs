// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palPvobs (pal/palPvobs.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::PAL__SR;
use crate::erfa::{self, ERFA_DAU, Ellipsoid};

/// Position and velocity of an observing station, in au and au/s, in the
/// true equator and equinox of date (WGS84 ellipsoid), from the geodetic
/// latitude `p`, the height `h` in metres and the local apparent sidereal
/// time `stl`.
pub(crate) fn pvobs(p: f64, h: f64, stl: f64) -> [f64; 6] {
    // ERFA's status is ignored, as in PAL; on failure ERFA zeroes the vector.
    let xyz = erfa::gd2gc(Ellipsoid::Wgs84, 0.0, p, h).unwrap_or([0.0; 3]);
    let r = xyz[0] / ERFA_DAU;
    let z = xyz[2] / ERFA_DAU;
    // PAL calls `sin` and `cos` of one argument.
    let (s, c) = erfa::sincos(stl);
    let v = PAL__SR * r;
    [r * c, r * s, z, -v * s, v * c, 0.0]
}

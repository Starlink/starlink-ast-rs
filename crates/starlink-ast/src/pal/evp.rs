// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palEvp (pal/palEvp.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::{PAL__MJD0, PAL__SPD};
use crate::erfa::{self, V3};

/// The Earth's barycentric velocity (au per second) and position (au), then
/// its heliocentric velocity and position, at the TDB date `date` (a
/// Modified Julian Date), with respect to the mean equator and equinox of
/// Julian epoch `deqx`, or to the BCRS when `deqx` is zero or negative.
///
/// Returns `(dvb, dpb, dvh, dph)`.
pub(crate) fn evp(date: f64, deqx: f64) -> (V3, V3, V3, V3) {
    // BCRS PV-vectors; the date-range warning is ignored.
    let ((mut pvh, mut pvb), _) = erfa::epv00(PAL__MJD0, date);

    // Was precession to another equinox requested?
    if deqx > 0.0 {
        // Yes: compute precession matrix from J2000.0 to deqx.
        let (d1, d2) = erfa::epj2jd(deqx);
        let r = erfa::pmat06(d1, d2);

        // Rotate the PV-vectors.
        pvh = erfa::rxpv(&r, &pvh);
        pvb = erfa::rxpv(&r, &pvb);
    }

    // Return the required vectors.
    let mut dvb = [0.0; 3];
    let mut dpb = [0.0; 3];
    let mut dvh = [0.0; 3];
    let mut dph = [0.0; 3];
    for i in 0..3 {
        dvh[i] = pvh[1][i] / PAL__SPD;
        dvb[i] = pvb[1][i] / PAL__SPD;
        dph[i] = pvh[0][i];
        dpb[i] = pvb[0][i];
    }
    (dvb, dpb, dvh, dph)
}

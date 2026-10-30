// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palPrec (pal/palPrec.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, ERFA_DJ00, ERFA_DJY, M3};

/// The Julian (FK5) precession matrix from Julian epoch `ep0` to Julian epoch
/// `ep1`, from the IAU 2006 model.
pub(crate) fn prec(ep0: f64, ep1: f64) -> M3 {
    // Convert supplied dates to days since J2000.
    let ep0_days = (ep0 - 2000.0) * ERFA_DJY;
    let ep1_days = (ep1 - 2000.0) * ERFA_DJY;

    // If the beginning epoch is J2000, just return the rotation matrix from
    // J2000 to EP1.
    if ep0 == 2000.0 {
        erfa::pmat06(ERFA_DJ00, ep1_days)
    // If the end epoch is J2000, get the rotation matrix from J2000 to EP0 and
    // then transpose it to get the rotation matrix from EP0 to J2000.
    } else if ep1 == 2000.0 {
        erfa::tr(&erfa::pmat06(ERFA_DJ00, ep0_days))
    // Otherwise, get the two matrices used above and multiply them together.
    } else {
        let rmatp = erfa::tr(&erfa::pmat06(ERFA_DJ00, ep0_days));
        let rmatq = erfa::pmat06(ERFA_DJ00, ep1_days);
        erfa::rxr(&rmatp, &rmatq)
    }
}

// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palGaleq (pal/palGaleq.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, M3};

/// The equatorial-to-galactic rotation matrix (J2000.0), obtained by applying the
/// standard FK4 to FK5 transformation, for zero proper motion in FK5, to the
/// columns of the B1950 equatorial to galactic rotation matrix.
const RMAT: M3 = [
    [-0.054875539726, -0.873437108010, -0.483834985808],
    [0.494109453312, -0.444829589425, 0.746982251810],
    [-0.867666135858, -0.198076386122, 0.455983795705],
];

/// Galactic (l, b) to FK5 J2000 (ra, dec), in radians, with ra in 0 to 2 pi and dec in -pi to pi.
pub(crate) fn galeq(dl: f64, db: f64) -> (f64, f64) {
    let v1 = erfa::s2c(dl, db);
    let v2 = erfa::trxp(&RMAT, &v1);
    let (a, b) = erfa::c2s(v2);
    (erfa::anp(a), erfa::anpm(b))
}

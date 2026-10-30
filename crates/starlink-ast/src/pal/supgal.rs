// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palSupgal (pal/palSupgal.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, M3};

/// The galactic to supergalactic rotation matrix.
const RMAT: M3 = [
    [-0.735742574804, 0.677261296414, 0.000000000000],
    [-0.074553778365, -0.080991471307, 0.993922590400],
    [0.673145302109, 0.731271165817, 0.110081262225],
];

/// Supergalactic (sl, sb) to galactic (l, b), in radians.
pub(crate) fn supgal(dsl: f64, dsb: f64) -> (f64, f64) {
    let v1 = erfa::s2c(dsl, dsb);
    let v2 = erfa::trxp(&RMAT, &v1);
    let (a, b) = erfa::c2s(v2);
    (erfa::anp(a), erfa::anpm(b))
}

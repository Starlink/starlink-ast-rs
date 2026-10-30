// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palSubet (pal/palSubet.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::etrms::etrms;
use crate::erfa;

/// Removes the E-terms of aberration for equinox `eq` from the apparent place
/// `(rc, dc)`, giving the mean `(rm, dm)`, in radians with `rm` in 0 to
/// 2 pi.
pub(crate) fn subet(rc: f64, dc: f64, eq: f64) -> (f64, f64) {
    let a = etrms(eq);
    let mut v = erfa::s2c(rc, dc);
    let f = 1.0 + erfa::pdp(&v, &a);
    for i in 0..3 {
        v[i] = f * v[i] - a[i];
    }
    let (rm, dm) = erfa::c2s(v);
    (erfa::anp(rm), dm)
}

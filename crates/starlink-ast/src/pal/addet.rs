// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palAddet (pal/palAddet.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::etrms::etrms;
use crate::erfa;

/// Adds the E-terms of aberration for equinox `eq` to the mean place
/// `(rm, dm)`, giving the apparent `(rc, dc)`, in radians with `rc` in 0 to
/// 2 pi.
pub(crate) fn addet(rm: f64, dm: f64, eq: f64) -> (f64, f64) {
    let a = etrms(eq);
    let mut v = erfa::s2c(rm, dm);
    for i in 0..3 {
        v[i] += a[i];
    }
    let (rc, dc) = erfa::c2s(v);
    (erfa::anp(rc), dc)
}

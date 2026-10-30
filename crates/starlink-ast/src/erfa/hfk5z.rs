// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Copyright (C) 2013-2016, NumFOCUS Foundation.
// All rights reserved.
//
// This library is derived, with permission, from the International
// Astronomical Union's "Standards of Fundamental Astronomy" library,
// available from http://www.iausofa.org.
//
// The ERFA version is intended to retain identical functionality to
// the SOFA library, but made distinct through different function and
// file names, as set out in the SOFA license conditions.  The SOFA
// original has a role as a reference standard for the IAU and IERS,
// and consequently redistribution is permitted only in its unaltered
// state.  The ERFA version is not subject to this restriction and
// therefore can be included in distributions which do not support the
// concept of "read only" software.
//
// Although the intent is to replicate the SOFA API (other than
// replacement of prefix names) and results (with the exception of
// bugs;  any that are discovered will be fixed), SOFA is not
// responsible for any errors found in this version of the library.
//
// If you wish to acknowledge the SOFA heritage, please acknowledge
// that you are using a library derived from SOFA, rather than SOFA
// itself.
//
//
// TERMS AND CONDITIONS
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
// 1 Redistributions of source code must retain the above copyright
//   notice, this list of conditions and the following disclaimer.
//
// 2 Redistributions in binary form must reproduce the above copyright
//   notice, this list of conditions and the following disclaimer in
//   the documentation and/or other materials provided with the
//   distribution.
//
// 3 Neither the name of the Standards Of Fundamental Astronomy Board,
//   the International Astronomical Union nor the names of its
//   contributors may be used to endorse or promote products derived
//   from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.
//
// Ported from ERFA eraHfk5z (erfa/hfk5z.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DJ00, ERFA_DJY, PV, anp, fk5hip, pv2s, pxp, rv2m, rxp, rxr, s2c, sxp, trxp};

/// Transforms a Hipparcos star position into FK5 J2000.0, assuming zero
/// Hipparcos proper motion, returning (R.A., Dec) at the given date and the
/// FK5 proper motions (radians per year).
///
/// The date is the two-part Julian Date of the Hipparcos position.
pub(crate) fn hfk5z(rh: f64, dh: f64, date1: f64, date2: f64) -> (f64, f64, f64, f64) {
    // Time interval from fundamental epoch J2000.0 to given date (JY).
    let t = ((date1 - ERFA_DJ00) + date2) / ERFA_DJY;

    // Hipparcos barycentric position vector (normalized).
    let ph = s2c(rh, dh);

    // FK5 to Hipparcos orientation matrix and spin vector.
    let (r5h, s5h) = fk5hip();

    // Rotate the spin into the Hipparcos system.
    let sh = rxp(&r5h, &s5h);

    // Accumulated Hipparcos wrt FK5 spin over that interval.
    let vst = sxp(t, &s5h);

    // Express the accumulated spin as a rotation matrix.
    let rst = rv2m(&vst);

    // Rotation matrix: accumulated spin, then FK5 to Hipparcos.
    let r5ht = rxr(&r5h, &rst);

    // De-orient and de-spin the Hipparcos position into FK5 J2000.0.
    let mut pv5e: PV = [[0.0; 3]; 2];
    pv5e[0] = trxp(&r5ht, &ph);

    // Apply spin to the position giving a space motion.
    let vv = pxp(&sh, &ph);

    // De-orient and de-spin the Hipparcos space motion into FK5 J2000.0.
    pv5e[1] = trxp(&r5ht, &vv);

    // FK5 position/velocity pv-vector to spherical.
    let (w, d5, _r, dr5, dd5, _v) = pv2s(&pv5e);
    (anp(w), d5, dr5, dd5)
}

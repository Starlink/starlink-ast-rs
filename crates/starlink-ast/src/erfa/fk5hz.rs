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
// Ported from ERFA eraFk5hz (erfa/fk5hz.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DJ00, ERFA_DJY, V3, anp, c2s, fk5hip, rv2m, rxp, s2c, sxp, trxp};

/// Transforms an FK5 (J2000.0) star position into the frame of the
/// Hipparcos catalogue, assuming zero Hipparcos proper motion, returning
/// (R.A., Dec) in radians.
///
/// The date is the two-part Julian Date of the FK5 position.
pub(crate) fn fk5hz(r5: f64, d5: f64, date1: f64, date2: f64) -> (f64, f64) {
    // Interval from given date to fundamental epoch J2000.0 (JY).
    let t = -((date1 - ERFA_DJ00) + date2) / ERFA_DJY;

    // FK5 barycentric position vector.
    let p5e: V3 = s2c(r5, d5);

    // FK5 to Hipparcos orientation matrix and spin vector.
    let (r5h, s5h) = fk5hip();

    // Accumulated Hipparcos wrt FK5 spin over that interval.
    let vst = sxp(t, &s5h);

    // Express the accumulated spin as a rotation matrix.
    let rst = rv2m(&vst);

    // Derotate the vector's FK5 axes back to date.
    let p5 = trxp(&rst, &p5e);

    // Rotate the vector into the Hipparcos system.
    let ph = rxp(&r5h, &p5);

    // Hipparcos vector to spherical.
    let (w, dh) = c2s(ph);
    (anp(w), dh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;

    #[test]
    fn fk5hz_matches_vendored_c_bit_for_bit() {
        for (r, d) in fk_positions() {
            for (d1, d2) in fk_dates() {
                let (mut cr, mut cd) = (0.0, 0.0);
                // SAFETY: two scalar outputs.
                unsafe { astEraFk5hz(r, d, d1, d2, &mut cr, &mut cd) };
                let (pr, pd) = fk5hz(r, d, d1, d2);
                assert!(same(pr, cr) && same(pd, cd), "{r:e} {d:e} {d1:e} {d2:e}");
            }
        }
    }
}

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
// Ported from ERFA eraFk54z (erfa/fk54z.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{anp, c2s, fk524, s2c, sincos};

/// Converts J2000.0 FK5 star data to B1950.0 FK4 assuming zero proper motion
/// and parallax, returning (R.A., Dec) at the Besselian epoch `bepoch` and
/// the fictitious proper motions in radians per year.
pub(crate) fn fk54z(r2000: f64, d2000: f64, bepoch: f64) -> (f64, f64, f64, f64) {
    // FK5 equinox J2000.0 to FK4 equinox B1950.0.
    let (r, d, pr, pd, _px, _rv) = fk524(r2000, d2000, 0.0, 0.0, 0.0, 0.0);

    // Spherical to Cartesian.
    let mut p = s2c(r, d);

    // Fictitious proper motion (radians per year).
    let (sr, cr) = sincos(r);
    let (sd, cd) = sincos(d);
    let v = [-pr * p[1] - pd * cr * sd, pr * p[0] - pd * sr * sd, pd * cd];

    // Apply the motion.
    let w = bepoch - 1950.0;
    for i in 0..3 {
        p[i] += w * v[i];
    }

    // Cartesian to spherical.
    let (w, d1950) = c2s(p);
    (anp(w), d1950, pr, pd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;

    #[test]
    fn fk54z_matches_vendored_c_bit_for_bit() {
        for (r, d) in fk_positions() {
            for e in fk_epochs() {
                let mut c = [0.0; 4];
                // SAFETY: four scalar outputs.
                unsafe { astEraFk54z(r, d, e, &mut c[0], &mut c[1], &mut c[2], &mut c[3]) };
                let o = fk54z(r, d, e);
                let o = [o.0, o.1, o.2, o.3];
                assert!((0..4).all(|k| same(o[k], c[k])), "{r:e} {d:e} {e:e}");
            }
        }
    }
}

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
// Ported from ERFA eraFk45z (erfa/fk45z.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DR2AS, M3, PV, V3, anp, c2s, epb2jd, epj, pdp, pmp, ppsp, pvu, s2c};

/// Converts B1950.0 FK4 star data to J2000.0 FK5 assuming zero proper motion
/// in the FK5 system, returning (R.A., Dec) at J2000.0 in radians.
///
/// `bepoch` is the Besselian epoch of the FK4 position.
pub(crate) fn fk45z(r1950: f64, d1950: f64, bepoch: f64) -> (f64, f64) {
    // Radians per year to arcsec per century.
    let pmf = 100.0 * ERFA_DR2AS;

    // Vectors A and Adot (Seidelmann 3.591-2).
    const A: V3 = [-1.62557e-6, -0.31919e-6, -0.13843e-6];
    const AD: V3 = [1.245e-3, -1.580e-3, -0.659e-3];

    // 3x2 matrix of p-vectors (cf. Seidelmann 3.591-4, matrix M).
    const EM: [M3; 2] = [
        [
            [0.9999256782, -0.0111820611, -0.0048579477],
            [0.0111820610, 0.9999374784, -0.0000271765],
            [0.0048579479, -0.0000271474, 0.9999881997],
        ],
        [
            [-0.000551, -0.238565, 0.435739],
            [0.238514, -0.002667, -0.008541],
            [-0.435623, 0.012254, 0.002117],
        ],
    ];

    // Spherical coordinates to p-vector.
    let r0 = s2c(r1950, d1950);

    // Adjust p-vector A to give zero proper motion in FK5.
    let w = (bepoch - 1950.0) / pmf;
    let mut p = ppsp(&A, w, &AD);

    // Remove E-terms.
    p = ppsp(&p, -pdp(&r0, &p), &r0);
    p = pmp(&r0, &p);

    // Convert to Fricke system pv-vector (cf. Seidelmann 3.591-3).
    let mut pv: PV = [[0.0; 3]; 2];
    for i in 0..2 {
        for j in 0..3 {
            let mut w = 0.0;
            for k in 0..3 {
                w += EM[i][j][k] * p[k];
            }
            pv[i][j] = w;
        }
    }

    // Allow for fictitious proper motion.
    let (djm0, djm) = epb2jd(bepoch);
    let w = (epj(djm0, djm) - 2000.0) / pmf;
    let pv = pvu(w, &pv);

    // Revert to spherical coordinates.
    let (w, d2000) = c2s(pv[0]);
    (anp(w), d2000)
}

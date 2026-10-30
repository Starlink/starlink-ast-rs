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
// Ported from ERFA eraFk524 (erfa/fk524.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DR2AS, PV, anp, pdp, pm, pmp, ppp, pv2s, s2pv, sxp};

/// Converts J2000.0 FK5 star data to B1950.0 FK4, returning
/// (r1950, d1950, dr1950, dd1950, p1950, v1950).
///
/// Proper motions are in radians per year, parallax in arcseconds and radial
/// velocity in km/s.
pub(crate) fn fk524(
    r2000: f64,
    d2000: f64,
    dr2000: f64,
    dd2000: f64,
    p2000: f64,
    v2000: f64,
) -> (f64, f64, f64, f64, f64, f64) {
    // Radians per year to arcsec per century.
    let pmf = 100.0 * ERFA_DR2AS;

    // Small number to avoid arithmetic problems.
    const TINY: f64 = 1e-30;

    // Km per sec to au per tropical century
    // = 86400 * 36524.2198782 / 149597870.7
    const VF: f64 = 21.095;

    // Constant pv-vector (cf. Seidelmann 3.591-2, vectors A and Adot).
    const A: PV = [
        [-1.62557e-6, -0.31919e-6, -0.13843e-6],
        [1.245e-3, -1.580e-3, -0.659e-3],
    ];

    // 3x2 matrix of pv-vectors (cf. Seidelmann 3.592-1, matrix M^-1).
    const EM: [[PV; 3]; 2] = [
        [
            [
                [0.9999256795, 0.0111814828, 0.0048590039],
                [-0.00000242389840, -0.00000002710544, -0.00000001177742],
            ],
            [
                [-0.0111814828, 0.9999374849, -0.0000271771],
                [0.00000002710544, -0.00000242392702, 0.00000000006585],
            ],
            [
                [-0.0048590040, -0.0000271557, 0.9999881946],
                [0.00000001177742, 0.00000000006585, -0.00000242404995],
            ],
        ],
        [
            [
                [-0.000551, 0.238509, -0.435614],
                [0.99990432, 0.01118145, 0.00485852],
            ],
            [
                [-0.238560, -0.002667, 0.012254],
                [-0.01118145, 0.99991613, -0.00002717],
            ],
            [
                [0.435730, -0.008541, 0.002117],
                [-0.00485852, -0.00002716, 0.99996684],
            ],
        ],
    ];

    // The FK5 data (units radians and arcsec per Julian century).
    let r = r2000;
    let d = d2000;
    let ur = dr2000 * pmf;
    let ud = dd2000 * pmf;
    let mut px = p2000;
    let mut rv = v2000;

    // Express as a pv-vector.
    let pxvf = px * VF;
    let w = rv * pxvf;
    let r0 = s2pv(r, d, 1.0, ur, ud, w);

    // Convert pv-vector to Bessel-Newcomb system (cf. Seidelmann 3.592-1).
    let mut r1: PV = [[0.0; 3]; 2];
    for i in 0..2 {
        for j in 0..3 {
            let mut w = 0.0;
            for k in 0..2 {
                for l in 0..3 {
                    w += EM[i][j][k][l] * r0[k][l];
                }
            }
            r1[i][j] = w;
        }
    }

    // Apply E-terms (equivalent to Seidelmann 3.592-3, one iteration).

    // Direction.
    let w = pm(&r1[0]);
    let mut p1 = sxp(pdp(&r1[0], &A[0]), &r1[0]);
    let mut p2 = sxp(w, &A[0]);
    p1 = pmp(&p2, &p1);
    p1 = ppp(&r1[0], &p1);

    // Recompute length.
    let w = pm(&p1);

    // Direction.
    p1 = sxp(pdp(&r1[0], &A[0]), &r1[0]);
    p2 = sxp(w, &A[0]);
    p1 = pmp(&p2, &p1);
    let mut pv: PV = [[0.0; 3]; 2];
    pv[0] = ppp(&r1[0], &p1);

    // Derivative.
    p1 = sxp(pdp(&r1[0], &A[1]), &pv[0]);
    p2 = sxp(w, &A[1]);
    p1 = pmp(&p2, &p1);
    pv[1] = ppp(&r1[1], &p1);

    // Revert to catalog form.
    let (r, d, w, ur, ud, rd) = pv2s(&pv);
    if px > TINY {
        rv = rd / pxvf;
        px /= w;
    }

    // Return the results.
    (anp(r), d, ur / pmf, ud / pmf, px, rv)
}

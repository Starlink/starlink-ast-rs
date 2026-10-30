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
// Ported from ERFA eraEors (erfa/eors.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::M3;

/// The equation of the origins, in radians: ERA minus GST, given the
/// classical bias-precession-nutation matrix `rnpb` and the CIO locator `s`.
pub(crate) fn eors(rnpb: &M3, s: f64) -> f64 {
    // Evaluate Wallace & Capitaine (2006) expression (16).
    let x = rnpb[2][0];
    let ax = x / (1.0 + rnpb[2][2]);
    let xs = 1.0 - ax * x;
    let ys = -ax * rnpb[2][1];
    let zs = -x;
    let p = rnpb[0][0] * xs + rnpb[0][1] * ys + rnpb[0][2] * zs;
    let q = rnpb[1][0] * xs + rnpb[1][1] * ys + rnpb[1][2] * zs;
    if (p != 0.0) || (q != 0.0) {
        s - q.atan2(p)
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;
    use crate::test_inputs::*;

    #[test]
    fn eors_matches_vendored_c_bit_for_bit() {
        let mut ms = matrices(2_000);
        ms.extend(nutation_dates().into_iter().step_by(10).map(|(d1, d2)| {
            let mut m = [[0.0; 3]; 3];
            // SAFETY: nine outputs.
            unsafe { astEraPnm06a(d1, d2, m.as_mut_ptr()) };
            m
        }));
        // Both of p and q zero, so the locator is returned unchanged.
        ms.push([[0.0; 3]; 3]);
        ms.push([[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        let ss = [
            0.0,
            -0.0,
            1e-8,
            -3e-8,
            1.0,
            f64::NAN,
            crate::constants::BAD_VALUE,
        ];
        for (k, m) in ms.iter().enumerate() {
            let s = ss[k % ss.len()];
            // SAFETY: nine inputs; a pure function of them.
            let c = unsafe { astEraEors(m.as_ptr(), s) };
            assert!(same(eors(m, s), c), "{m:?} {s:e}");
        }
    }
}

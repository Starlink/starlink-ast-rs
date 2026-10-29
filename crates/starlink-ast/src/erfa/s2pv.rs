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
// Ported from ERFA eraS2pv (erfa/s2pv.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{PV, sincos};

/// Converts position/velocity from spherical to Cartesian coordinates.
#[inline]
pub(crate) fn s2pv(theta: f64, phi: f64, r: f64, td: f64, pd: f64, rd: f64) -> PV {
    let (st, ct) = sincos(theta);
    let (sp, cp) = sincos(phi);
    let rcp = r * cp;
    let x = rcp * ct;
    let y = rcp * st;
    let rpd = r * pd;
    let w = rpd * sp - cp * rd;

    [
        [x, y, r * sp],
        [-y * td - w * ct, x * td - w * st, rpd * cp + sp * rd],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;
    use crate::test_inputs::*;

    #[test]
    fn s2pv_matches_vendored_c_bit_for_bit() {
        let pairs = angle_pairs(10_000);
        let n = pairs.len();
        let sp = specials();
        let mut rng = SplitMix64::new(0x5eed_5291);
        let mut cases: Vec<[f64; 6]> = Vec::new();
        for i in 0..n {
            let (a, b) = (pairs[i], pairs[(i * 31 + 7) % n]);
            let c = pairs[(i * 17 + 3) % n];
            cases.push([a.0, a.1, c.0, b.0, b.1, c.1]);
        }
        for _ in 0..20_000 {
            let mut v = [0.0; 6];
            for x in &mut v {
                *x = sp[(rng.next_u64() % sp.len() as u64) as usize];
            }
            cases.push(v);
        }
        for a in cases {
            let mut c = [[0.0; 3]; 2];
            // SAFETY: the output holds six values.
            unsafe { astEraS2pv(a[0], a[1], a[2], a[3], a[4], a[5], c.as_mut_ptr()) };
            let r = s2pv(a[0], a[1], a[2], a[3], a[4], a[5]);
            assert!(same3(&r[0], &c[0]) && same3(&r[1], &c[1]), "{a:?}");
        }
    }
}

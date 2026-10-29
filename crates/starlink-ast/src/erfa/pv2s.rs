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
// Ported from ERFA eraPv2s (erfa/pv2s.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::PV;

/// Converts position/velocity from Cartesian to spherical coordinates,
/// returning (theta, phi, r, theta-dot, phi-dot, r-dot).
#[inline]
pub(crate) fn pv2s(pv: &PV) -> (f64, f64, f64, f64, f64, f64) {
    // Components of position/velocity vector.
    let mut x = pv[0][0];
    let mut y = pv[0][1];
    let mut z = pv[0][2];
    let xd = pv[1][0];
    let yd = pv[1][1];
    let zd = pv[1][2];

    // Component of r in XY plane squared.
    let mut rxy2 = x * x + y * y;

    // Modulus squared.
    let mut r2 = rxy2 + z * z;

    // Modulus.
    let rtrue = r2.sqrt();

    // If null vector, move the origin along the direction of movement.
    let mut rw = rtrue;
    if rtrue == 0.0 {
        x = xd;
        y = yd;
        z = zd;
        rxy2 = x * x + y * y;
        r2 = rxy2 + z * z;
        rw = r2.sqrt();
    }

    // Position and velocity in spherical coordinates.
    let rxy = rxy2.sqrt();
    let xyp = x * xd + y * yd;
    let (theta, phi, td, pd);
    if rxy2 != 0.0 {
        theta = y.atan2(x);
        phi = z.atan2(rxy);
        td = (x * yd - y * xd) / rxy2;
        pd = (zd * rxy2 - z * xyp) / (r2 * rxy);
    } else {
        theta = 0.0;
        phi = if z != 0.0 { z.atan2(rxy) } else { 0.0 };
        td = 0.0;
        pd = 0.0;
    }
    let rd = if rw != 0.0 { (xyp + z * zd) / rw } else { 0.0 };
    (theta, phi, rtrue, td, pd, rd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;
    use crate::test_inputs::*;

    #[test]
    fn pv2s_matches_vendored_c_bit_for_bit() {
        let vs = vectors(20_000);
        let n = vs.len();
        let zero = [0.0; 3];
        let mut cases: Vec<PV> = Vec::new();
        for i in 0..n {
            cases.push([vs[i], vs[(i * 31 + 7) % n]]);
        }
        // A null position, with every velocity, and the reverse.
        for v in &vs {
            cases.push([zero, *v]);
            cases.push([*v, zero]);
        }
        for pv in cases {
            let mut c = [0.0; 6];
            // SAFETY: pv holds six values, and each output is one double.
            unsafe {
                astEraPv2s(
                    pv.as_ptr(),
                    &mut c[0],
                    &mut c[1],
                    &mut c[2],
                    &mut c[3],
                    &mut c[4],
                    &mut c[5],
                )
            };
            let r = pv2s(&pv);
            let r = [r.0, r.1, r.2, r.3, r.4, r.5];
            assert!(
                (0..6).all(|k| same(r[k], c[k])),
                "{pv:?}: {:?} vs C {:?}",
                r.map(f64::to_bits),
                c.map(f64::to_bits)
            );
        }
    }
}

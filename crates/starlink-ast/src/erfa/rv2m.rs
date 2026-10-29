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
// Ported from ERFA eraRv2m (erfa/rv2m.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{M3, V3, sincos};

/// Forms the r-matrix corresponding to a rotation vector.
#[inline]
pub(crate) fn rv2m(w: &V3) -> M3 {
    // Euler angle (magnitude of rotation vector) and functions.
    let mut x = w[0];
    let mut y = w[1];
    let mut z = w[2];
    let phi = (x * x + y * y + z * z).sqrt();
    let (s, c) = sincos(phi);
    let f = 1.0 - c;

    // Euler axis (direction of rotation vector), perhaps null.
    if phi > 0.0 {
        x /= phi;
        y /= phi;
        z /= phi;
    }

    // Form the rotation matrix.
    [
        [x * x * f + c, x * y * f + z * s, x * z * f - y * s],
        [y * x * f - z * s, y * y * f + c, y * z * f + x * s],
        [z * x * f + y * s, z * y * f - x * s, z * z * f + c],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;
    use crate::test_inputs::*;

    #[test]
    fn rv2m_matches_vendored_c_bit_for_bit() {
        for w in vectors(20_000) {
            let mut c = [[0.0; 3]; 3];
            // SAFETY: the vector holds three values, the matrix nine.
            unsafe { astEraRv2m(w.as_ptr(), c.as_mut_ptr()) };
            let r = rv2m(&w);
            assert!((0..3).all(|j| same3(&r[j], &c[j])), "{w:?}");
        }
    }
}

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
// Ported from ERFA eraS2c (erfa/s2c.c) as vendored at reference/starlink-ast 7c12422f; unchanged in substance in ERFA v2.0.1.

use crate::vmath;

/// Spherical coordinates (longitude, latitude) in radians to the unit vector
/// of direction cosines.
#[inline(always)]
pub(crate) fn s2c(theta: f64, phi: f64) -> [f64; 3] {
    let (st, ct) = super::sincos(theta);
    let (sp, cp) = super::sincos(phi);
    [ct * cp, st * cp, sp]
}

// ERFA's body with its sines and cosines from the vectorizable
// `vmath::sin_cos`, which is within 1 ULP of libm's.
/// [`s2c`] without branches, for angles [`s2c_needs_special`] accepts; a
/// loop over many points vectorizes. Each sine and cosine is within 1 ULP of
/// libm's; the z component is a sine, and the x and y components, products
/// of two, are within 3 ULP of [`s2c`]'s.
#[inline(always)]
pub(crate) fn s2c_kernel(theta: f64, phi: f64) -> [f64; 3] {
    let (st, ct) = vmath::sin_cos(theta);
    let (sp, cp) = vmath::sin_cos(phi);
    [ct * cp, st * cp, sp]
}

/// Whether (`theta`, `phi`) is outside [`s2c_kernel`]'s domain; such angles
/// go to [`s2c`].
#[inline(always)]
pub(crate) fn s2c_needs_special(theta: f64, phi: f64) -> bool {
    !(vmath::sin_cos_covers(theta) && vmath::sin_cos_covers(phi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_inputs::{angle_pairs, specials, ulps};

    unsafe extern "C" {
        fn astEraS2c(theta: f64, phi: f64, c: *mut f64);
    }

    #[test]
    fn s2c_matches_vendored_c_bit_for_bit() {
        let mut inputs = angle_pairs(100_000);
        let s = specials();
        for &a in &s {
            for &b in &s {
                inputs.push((a, b));
            }
        }
        for (theta, phi) in inputs {
            let mut c = [0.0; 3];
            // SAFETY: c holds the three values astEraS2c writes.
            unsafe { astEraS2c(theta, phi, c.as_mut_ptr()) };
            let r = s2c(theta, phi);
            // Bit for bit, but any NaN matches any NaN: the sign of a NaN
            // from a product depends on its operand order, which the C
            // compiler may swap, and carries no meaning.
            let same = |a: f64, b: f64| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan());
            assert!(
                (0..3).all(|k| same(r[k], c[k])),
                "({theta}, {phi}): {:?} vs C {:?}",
                r.map(f64::to_bits),
                c.map(f64::to_bits)
            );
        }
    }

    #[test]
    fn s2c_kernel_is_within_three_ulp_of_s2c() {
        let mut differ = 0;
        for (theta, phi) in angle_pairs(1_000_000)
            .into_iter()
            .filter(|&(t, p)| !s2c_needs_special(t, p))
        {
            let k = s2c_kernel(theta, phi);
            let e = s2c(theta, phi);
            for i in 0..3 {
                let d = ulps(k[i], e[i]);
                // Each sine and cosine is within 1 ULP of libm's. A product of
                // two carries both, measured against the product's own ULP,
                // which can be half a factor's; 3 ULP is the most seen over 8
                // million angle pairs.
                assert!(
                    d <= 3,
                    "({theta:e}, {phi:e}) axis {i}: {:e} vs {:e}",
                    k[i],
                    e[i]
                );
                differ += usize::from(d > 0);
            }
        }
        assert!(
            differ > 0,
            "no input differed from s2c; is the kernel in use?"
        );
    }
}

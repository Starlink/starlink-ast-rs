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
// Ported from ERFA eraC2s (erfa/c2s.c) as vendored at reference/starlink-ast 7c12422f; unchanged in substance in ERFA v2.0.1.

use crate::vmath;

/// Direction to spherical coordinates: (longitude, latitude) in radians of
/// the vector `p`, whose magnitude does not matter, computed as ERFA does.
///
/// The longitude is 0 on the polar axis and the latitude is 0 on the
/// equator, including for the zero vector.
#[inline(always)]
pub(crate) fn c2s(p: [f64; 3]) -> (f64, f64) {
    let [x, y, z] = p;
    let d2 = x * x + y * y;
    let theta = if d2 == 0.0 { 0.0 } else { y.atan2(x) };
    let phi = if z == 0.0 { 0.0 } else { z.atan2(d2.sqrt()) };
    (theta, phi)
}

// ERFA's body with its two atan2 calls replaced by the vectorizable
// `vmath::atan2`, which is within 1 ULP of libm's.
/// [`c2s`] without branches and within 1 ULP of it, for a vector
/// [`c2s_needs_special`] accepts; a loop over many vectors vectorizes.
///
/// `FMA` is passed to [`vmath::atan2`]; the result is the same either way.
#[inline(always)]
pub(crate) fn c2s_kernel<const FMA: bool>(p: [f64; 3]) -> (f64, f64) {
    let [x, y, z] = p;
    let r = (x * x + y * y).sqrt();
    (vmath::atan2::<FMA>(y, x), vmath::atan2::<FMA>(z, r))
}

/// Whether `p` is outside [`c2s_kernel`]'s domain: a coordinate that is
/// zero, infinite or NaN, an x, y projection whose squared length
/// underflows to zero or overflows, or an arctangent's arguments outside the
/// magnitudes [`vmath::atan2_covers`] accepts. Such vectors go to [`c2s`].
#[inline(always)]
pub(crate) fn c2s_needs_special(p: [f64; 3]) -> bool {
    let [x, y, z] = p;
    // The kernel's second arctangent is of `z` and the square root of
    // `x*x + y*y`,
    // and its cover bounds the magnitudes of its arguments and their
    // quotient, so it is tested on the square root itself, which the kernel
    // computes alike.
    let r = (x * x + y * y).sqrt();
    !(vmath::atan2_covers(y, x) && vmath::atan2_covers(z, r))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_inputs::{specials, ulps, unit_vectors};

    /// Directions plus every triple of special values.
    fn inputs(n: usize) -> Vec<[f64; 3]> {
        let mut inputs = unit_vectors(n);
        let s = specials();
        for &x in &s {
            for &y in &s {
                for &z in &s {
                    inputs.push([x, y, z]);
                }
            }
        }
        inputs
    }

    #[test]
    fn c2s_kernel_is_within_one_ulp_of_c2s() {
        let mut differ = 0;
        for p in inputs(1_000_000)
            .into_iter()
            .filter(|&p| !c2s_needs_special(p))
        {
            let (t, f) = c2s_kernel::<{ vmath::HAS_FMA }>(p);
            let (et, ef) = c2s(p);
            let (dt, df) = (ulps(t, et), ulps(f, ef));
            assert!(
                dt <= 1 && df <= 1,
                "{p:?}: ({t:e}, {f:e}) vs ({et:e}, {ef:e})"
            );
            differ += usize::from(dt + df > 0);
        }
        assert!(
            differ > 0,
            "no input differed from c2s; is the kernel in use?"
        );
    }
}

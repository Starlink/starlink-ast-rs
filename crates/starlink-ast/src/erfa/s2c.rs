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
// `vmath::sin_cos`, which is within 1 ULP of the exact values.
/// [`s2c`] without branches, for angles [`s2c_needs_special`] accepts; a
/// loop over many points vectorizes. Each sine and cosine is within 1 ULP of
/// the exact value, rounded; the z component is a sine, and the x and y
/// components, products of two, are within 8 ULP of the exact products,
/// rounded. [`s2c`] takes its sines and cosines from the platform libm, so
/// the two also differ by libm's own error.
#[inline(always)]
pub(crate) fn s2c_kernel(theta: f64, phi: f64) -> [f64; 3] {
    let (st, ct) = vmath::sin_cos(theta);
    let (sp, cp) = vmath::sin_cos(phi);
    [ct * cp, st * cp, sp]
}

/// How far the x and y components of [`s2c_kernel`] may be from the exact
/// products of the sines and cosines of its angles, rounded, in ULP.
// Each component is a product of a sine and a cosine from `vmath::sin_cos`,
// each within 1 ULP of its exact value rounded, and so within 1.5 ULP, at
// most 3 * 2^-53 relative, of the exact value itself. The exact product of
// the two kernel values is then within 6 * 2^-53 relative of the exact
// product, to first order; the kernel rounds it once, at most 2^-53 relative,
// and the reference rounds the exact product once, at most 2^-53 relative
// again. That is a relative difference of up to 8 * 2^-53, and an ULP of
// either result is more than 2^-53 of it, so the bound is 8 ULP. It holds on
// every platform: neither side calls libm.
#[cfg(test)]
pub(crate) const S2C_PRODUCT_ULPS: u64 = 8;

/// The direction cosines of (`theta`, `phi`) from the exact sines and
/// cosines, each component rounded once, for comparison with
/// [`s2c_kernel`]: see [`crate::test_inputs::sin_cos_dd`].
#[cfg(test)]
pub(crate) fn s2c_reference(theta: f64, phi: f64) -> [f64; 3] {
    use crate::test_inputs::{dd_product, dd_round, sin_cos_dd};
    let (st, ct) = sin_cos_dd(theta);
    let (sp, cp) = sin_cos_dd(phi);
    [dd_product(ct, cp), dd_product(st, cp), dd_round(sp)]
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
    use crate::test_inputs::{angle_pairs, ulps};

    /// [`s2c_kernel`]'s z component, a sine, is within 1 ULP of the exact
    /// value, rounded, and its x and y components within
    /// [`S2C_PRODUCT_ULPS`] of the exact products, rounded, on every
    /// platform.
    #[test]
    fn s2c_kernel_is_within_its_derived_bound_of_the_exact_value() {
        let mut differ = 0;
        for (theta, phi) in angle_pairs(1_000_000)
            .into_iter()
            .filter(|&(t, p)| !s2c_needs_special(t, p))
        {
            let k = s2c_kernel(theta, phi);
            let e = s2c_reference(theta, phi);
            for i in 0..3 {
                let d = ulps(k[i], e[i]);
                let bound = if i == 2 { 1 } else { S2C_PRODUCT_ULPS };
                assert!(
                    d <= bound,
                    "({theta:e}, {phi:e}) axis {i}: {:e} vs {:e}",
                    k[i],
                    e[i]
                );
                differ += usize::from(d > 0);
            }
        }
        assert!(
            differ > 0,
            "no input differed from the reference; is the kernel in use?"
        );
    }
}

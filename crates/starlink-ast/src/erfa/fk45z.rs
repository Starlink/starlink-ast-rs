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

use super::{
    ERFA_D2PI, ERFA_DR2AS, KernelTrig, M3, PV, V3, VmathTrig, anp, c2s, epb2jd, epj, pdp, pmp,
    ppsp, pvu, s2c,
};
use crate::vmath::{atan2_covers, sin_cos_covers};

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

/// Converts B1950.0 FK4 star data to J2000.0 FK5 assuming zero proper motion
/// in the FK5 system, returning (R.A., Dec) at J2000.0 in radians.
///
/// `bepoch` is the Besselian epoch of the FK4 position.
pub(crate) fn fk45z(r1950: f64, d1950: f64, bepoch: f64) -> (f64, f64) {
    // Radians per year to arcsec per century.
    let pmf = 100.0 * ERFA_DR2AS;

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

/// The values [`fk45z`] computes from its epoch alone, computed once for the
/// many positions of one epoch that [`fk45z_kernel`] converts; they are the
/// same bits as [`fk45z`] computes for each position.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fk45zEpoch {
    /// The vector A adjusted to give zero proper motion in FK5.
    a: V3,
    /// The time over which the fictitious proper motion acts, as [`fk45z`]
    /// passes it to `pvu`.
    w: f64,
}

impl Fk45zEpoch {
    /// The epoch terms of [`fk45z`] for the Besselian epoch `bepoch`.
    pub(crate) fn new(bepoch: f64) -> Self {
        // As in fk45z.
        let pmf = 100.0 * ERFA_DR2AS;
        let w = (bepoch - 1950.0) / pmf;
        let a = ppsp(&A, w, &AD);
        let (djm0, djm) = epb2jd(bepoch);
        let w = (epj(djm0, djm) - 2000.0) / pmf;
        Self { a, w }
    }
}

/// [`fk45z`] without branches, for the epoch terms `e`: a loop over many
/// positions vectorizes. Returns the (R.A., Dec) and whether the position
/// is outside the kernel's domain, where the values are unspecified and
/// [`fk45z`] gives the result.
///
/// Its two sines and cosines and two arctangents are each within 1 ULP of
/// the exact value, rounded; everything else is [`fk45z`]'s arithmetic. `FMA`
/// is passed to [`crate::vmath::atan2`]; the result is the same either way.
#[inline(always)]
pub(crate) fn fk45z_kernel<const FMA: bool>(
    r1950: f64,
    d1950: f64,
    e: &Fk45zEpoch,
) -> (f64, f64, bool) {
    fk45z_kernel_with::<VmathTrig<FMA>>(r1950, d1950, e)
}

/// [`fk45z_kernel`] with its sines, cosines and arctangents from `T`.
// The body of eraFk45z with eraS2c's and eraC2s's elementary functions from
// `T`, the epoch terms from `e`, and the two branches of eraC2s left to the
// domain test: they are taken only for a zero component, which it rejects.
#[inline(always)]
pub(crate) fn fk45z_kernel_with<T: KernelTrig>(
    r1950: f64,
    d1950: f64,
    e: &Fk45zEpoch,
) -> (f64, f64, bool) {
    // Spherical coordinates to p-vector (eraS2c).
    let (st, ct) = T::sin_cos(r1950);
    let (sp, cp) = T::sin_cos(d1950);
    let r0 = [ct * cp, st * cp, sp];

    // Remove E-terms.
    let mut p = ppsp(&e.a, -pdp(&r0, &e.a), &r0);
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
    let pv = pvu(e.w, &pv);

    // Revert to spherical coordinates (eraC2s).
    let [x, y, z] = pv[0];
    let rxy = (x * x + y * y).sqrt();
    let w = T::atan2(y, x);
    let d2000 = T::atan2(z, rxy);
    let special = !(sin_cos_covers(r1950)
        & sin_cos_covers(d1950)
        & atan2_covers(y, x)
        & atan2_covers(z, rxy));

    // eraAnp: `w` is within [-pi, pi], where `fmod(w, 2pi)` is `w` exactly,
    // so only the adjustment of a negative angle remains.
    let w = if w < 0.0 { w + ERFA_D2PI } else { w };
    (w, d2000, special)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::CheckedTrig;
    use crate::test_inputs::{fk_epochs, fk_positions, sky_terms};

    /// How far [`fk45z_kernel`]'s (R.A., Dec) may be from [`fk45z`]'s, in
    /// radians on the sky: the difference in Dec, and the difference in R.A.
    /// modulo 2 pi times the cosine of the Dec.
    // In units of u = 2^-53, to first order. Each kernel sine and cosine is
    // within 2 ULP of libm's (the rule the projections' bounds take), at most
    // 2u as both are at most 1 in magnitude, so the direction cosines differ
    // by at most 2u + 2u plus one rounding in each path, 5u. The arithmetic
    // to the final vector is linear, a near-rotation of norm below 1.02 plus
    // E-term and proper-motion terms below 1e-4, and rounds each component
    // eight times on magnitudes at most about 1 (one subtraction, five
    // operations of the matrix row, two of the proper-motion update), at most
    // 4u in each path: the components differ by at most 1.02 * 5u + 8u, under
    // 14u, and the vectors, of length within 1e-4 of 1, by under 24.5u. The
    // exact Decs of the two vectors then differ by at most the angle between
    // them, 24.5u; each path's arctangent is within 1 ULP of the exact value,
    // and an ULP of a Dec is at most 2u, adding 4u; and the rounding of the
    // square root in each path moves a Dec by at most u/4, adding u/2: 29u.
    // The exact R.A.s differ by at most the x, y difference, sqrt(2) * 14u,
    // over the length of the x, y projection, which is the cosine of the Dec
    // to within 1e-4, so 20u after multiplying by that cosine; the
    // arctangents add 2 ULP of an angle below 4, 8u, and the addition of
    // 2 pi to a negative R.A. rounds once in each path in [pi, 2 pi), 8u:
    // 36u. Near a pole, where the first-order argument fails, the R.A.
    // difference times the cosine of the Dec is at most pi times that
    // cosine, which is below 40u wherever the Dec rounds to within 1e-15 of
    // the pole. The bound is 40u for both.
    const FK45Z_SKY_BOUND: f64 = 40.0 * f64::EPSILON / 2.0;

    /// Every sine, cosine and arctangent [`fk45z_kernel`] uses is within 1
    /// ULP of the exact value, rounded, which [`CheckedTrig`] asserts at
    /// each call, and its (R.A., Dec) are within [`FK45Z_SKY_BOUND`] of
    /// [`fk45z`]'s, over the FK test positions at every test epoch.
    #[test]
    fn fk45z_kernel_is_within_bound_of_fk45z() {
        let mut differ = 0;
        for bepoch in fk_epochs() {
            let e = Fk45zEpoch::new(bepoch);
            for (r, d) in fk_positions(1_500) {
                let (ka, kd, special) = fk45z_kernel_with::<CheckedTrig>(r, d, &e);
                if special {
                    continue;
                }
                let k = (ka, kd);
                let p = fk45z(r, d, bepoch);
                let (sa, sd) = sky_terms(k, p);
                assert!(
                    sa <= FK45Z_SKY_BOUND && sd <= FK45Z_SKY_BOUND,
                    "({r:e}, {d:e}) at {bepoch}: {k:?} vs {p:?}"
                );
                differ += usize::from(k != p);
            }
        }
        assert!(
            differ > 0,
            "no position differed from fk45z; is the kernel in use?"
        );
    }
}

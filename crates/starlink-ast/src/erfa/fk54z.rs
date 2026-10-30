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
// Ported from ERFA eraFk54z (erfa/fk54z.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::fk524::fk524_kernel_with;
use super::{ERFA_D2PI, KernelTrig, VmathTrig, anp, c2s, fk524, s2c, sincos};
use crate::vmath::{atan2_covers, sin_cos_covers};

/// Converts J2000.0 FK5 star data to B1950.0 FK4 assuming zero proper motion
/// and parallax, returning (R.A., Dec) at the Besselian epoch `bepoch` and
/// the fictitious proper motions in radians per year.
pub(crate) fn fk54z(r2000: f64, d2000: f64, bepoch: f64) -> (f64, f64, f64, f64) {
    // FK5 equinox J2000.0 to FK4 equinox B1950.0.
    let (r, d, pr, pd, _px, _rv) = fk524(r2000, d2000, 0.0, 0.0, 0.0, 0.0);

    // Spherical to Cartesian.
    let mut p = s2c(r, d);

    // Fictitious proper motion (radians per year).
    let (sr, cr) = sincos(r);
    let (sd, cd) = sincos(d);
    let v = [-pr * p[1] - pd * cr * sd, pr * p[0] - pd * sr * sd, pd * cd];

    // Apply the motion.
    let w = bepoch - 1950.0;
    for i in 0..3 {
        p[i] += w * v[i];
    }

    // Cartesian to spherical.
    let (w, d1950) = c2s(p);
    (anp(w), d1950, pr, pd)
}

/// [`fk54z`]'s (R.A., Dec) at the Besselian epoch `bepoch` without
/// branches: a loop over many positions vectorizes. Returns them and whether
/// the position is outside the kernel's domain, where the values are
/// unspecified and [`fk54z`] gives the result.
///
/// Its four sines and cosines and four arctangents are each within 1 ULP of
/// the exact value, rounded; everything else is [`fk54z`]'s arithmetic. `FMA`
/// is passed to [`crate::vmath::atan2`]; the result is the same either way.
#[inline(always)]
pub(crate) fn fk54z_kernel<const FMA: bool>(
    r2000: f64,
    d2000: f64,
    bepoch: f64,
) -> (f64, f64, bool) {
    fk54z_kernel_with::<VmathTrig<FMA>>(r2000, d2000, bepoch)
}

/// [`fk54z_kernel`] with its sines, cosines and arctangents from `T`.
// The body of eraFk54z with eraFk524's kernel, eraS2c's and eraC2s's
// elementary functions from `T`, and the two branches of eraC2s left to the
// domain test, which rejects the zero components they need. The sines and
// cosines eraS2c takes of the FK4 position are those the fictitious proper
// motion takes again, so they are computed once.
#[inline(always)]
pub(crate) fn fk54z_kernel_with<T: KernelTrig>(
    r2000: f64,
    d2000: f64,
    bepoch: f64,
) -> (f64, f64, bool) {
    // FK5 equinox J2000.0 to FK4 equinox B1950.0.
    let (r, d, pr, pd, special_fk524) = fk524_kernel_with::<T>(r2000, d2000);

    // Spherical to Cartesian (eraS2c).
    let (sr, cr) = T::sin_cos(r);
    let (sd, cd) = T::sin_cos(d);
    let mut p = [cr * cd, sr * cd, sd];

    // Fictitious proper motion (radians per year).
    let v = [-pr * p[1] - pd * cr * sd, pr * p[0] - pd * sr * sd, pd * cd];

    // Apply the motion.
    let w = bepoch - 1950.0;
    for i in 0..3 {
        p[i] += w * v[i];
    }

    // Cartesian to spherical (eraC2s).
    let [x, y, z] = p;
    let rxy = (x * x + y * y).sqrt();
    let w = T::atan2(y, x);
    let d1950 = T::atan2(z, rxy);
    let special = special_fk524
        | !(sin_cos_covers(r) & sin_cos_covers(d) & atan2_covers(y, x) & atan2_covers(z, rxy));

    // eraAnp: `w` is within [-pi, pi], where `fmod(w, 2pi)` is `w` exactly,
    // so only the adjustment of a negative angle remains.
    let w = if w < 0.0 { w + ERFA_D2PI } else { w };
    (w, d1950, special)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::CheckedTrig;
    use crate::test_inputs::{fk_epochs, fk_positions, sky_terms};

    /// How far [`fk54z_kernel`]'s (R.A., Dec) may be from [`fk54z`]'s, in
    /// radians on the sky, measured as for `FK45Z_SKY_BOUND`.
    // In units of u = 2^-53, to first order, in two stages. The first,
    // eraFk524, is the arithmetic of FK45Z_SKY_BOUND's derivation with fewer
    // roundings per component (the matrix row's five, as its velocity terms
    // are zeros and add exactly, and one addition of the E-terms), so its
    // (R.A., Dec) differ by at most 36u in R.A. times the cosine of the Dec
    // and 29u in Dec, and its direction by under sqrt(36^2 + 29^2)u = 46.3u.
    // The second takes the direction cosines of that position, the sines and
    // cosines adding 5u to each component as in FK45Z_SKY_BOUND's
    // derivation, 8.7u to the vector, and adds the fictitious proper motion
    // times the epoch interval, whose own difference is below 1e-4 of the
    // others and whose two operations round once each in each path, adding 2u
    // to each component, 3.5u to the vector: 58.5u in all. The arctangents
    // then add 4.5u to the Dec and 16u to the R.A. as in FK45Z_SKY_BOUND's
    // derivation: 63u in Dec and 74.5u in R.A. The bound is 80u for both. The
    // argument fails where the FK4 position of the first stage is so near a
    // pole that its x, y projection is comparable to a rounding error, where
    // eraFk524's proper motion in R.A. is ill-conditioned for the C as well;
    // no test position lands there, the FK5 positions near the pole being
    // 0.011 radian from it in FK4.
    const FK54Z_SKY_BOUND: f64 = 80.0 * f64::EPSILON / 2.0;

    /// Every sine, cosine and arctangent [`fk54z_kernel`] uses is within 1
    /// ULP of the exact value, rounded, which [`CheckedTrig`] asserts at
    /// each call, and its (R.A., Dec) are within [`FK54Z_SKY_BOUND`] of
    /// [`fk54z`]'s, over the FK test positions at every test epoch.
    #[test]
    fn fk54z_kernel_is_within_bound_of_fk54z() {
        let mut differ = 0;
        for bepoch in fk_epochs() {
            for (r, d) in fk_positions(1_500) {
                let (ka, kd, special) = fk54z_kernel_with::<CheckedTrig>(r, d, bepoch);
                if special {
                    continue;
                }
                let k = (ka, kd);
                let (pa, pd, _, _) = fk54z(r, d, bepoch);
                let p = (pa, pd);
                let (sa, sd) = sky_terms(k, p);
                assert!(
                    sa <= FK54Z_SKY_BOUND && sd <= FK54Z_SKY_BOUND,
                    "({r:e}, {d:e}) at {bepoch}: {k:?} vs {p:?}"
                );
                differ += usize::from(k != p);
            }
        }
        assert!(
            differ > 0,
            "no position differed from fk54z; is the kernel in use?"
        );
    }
}

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
// Ported from ERFA eraP06e (erfa/p06e.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DAS2R, ERFA_DJ00, ERFA_DJC, obl06};

/// The IAU 2006 precession quantities for the TT date `date1 + date2` (a
/// two-part Julian Date), all in radians.
///
/// The sixteen values are, in order: `eps0` (obliquity at J2000.0), `psia`
/// (luni-solar precession), `oma` (inclination of the mean equator on the
/// J2000.0 ecliptic), `bpa` (ecliptic pole x, J2000.0 ecliptic triad), `bqa`
/// (ecliptic pole -y), `pia` (angle between the moving and J2000.0 ecliptics),
/// `bpia` (longitude of the ascending node of the moving ecliptic), `epsa`
/// (mean obliquity), `chia` (planetary precession), `za` and `zetaa` (the third
/// and first equatorial precession angles, negated), `thetaa` (the second),
/// `pa` (general precession), and the Fukushima-Williams angles `gam`, `phi` and
/// `psi`.
pub(crate) fn p06e(date1: f64, date2: f64) -> [f64; 16] {
    // Interval between fundamental date J2000.0 and given date (JC).
    let t = ((date1 - ERFA_DJ00) + date2) / ERFA_DJC;

    // Obliquity at J2000.0.
    let eps0 = 84381.406 * ERFA_DAS2R;

    // Luni-solar precession.
    let psia = (5038.481507
        + (-1.0790069 + (-0.00114045 + (0.000132851 + (-0.0000000951) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    // Inclination of mean equator with respect to the J2000.0 ecliptic.
    let oma = eps0
        + (-0.025754
            + (0.0512623 + (-0.00772503 + (-0.000000467 + (0.0000003337) * t) * t) * t) * t)
            * t
            * ERFA_DAS2R;

    // Ecliptic pole x, J2000.0 ecliptic triad.
    let bpa = (4.199094
        + (0.1939873 + (-0.00022466 + (-0.000000912 + (0.0000000120) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    // Ecliptic pole -y, J2000.0 ecliptic triad.
    let bqa = (-46.811015
        + (0.0510283 + (0.00052413 + (-0.000000646 + (-0.0000000172) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    // Angle between moving and J2000.0 ecliptics.
    let pia = (46.998973
        + (-0.0334926 + (-0.00012559 + (0.000000113 + (-0.0000000022) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    // Longitude of ascending node of the moving ecliptic.
    let bpia = (629546.7936
        + (-867.95758 + (0.157992 + (-0.0005371 + (-0.00004797 + (0.000000072) * t) * t) * t) * t)
            * t)
        * ERFA_DAS2R;

    // Mean obliquity of the ecliptic.
    let epsa = obl06(date1, date2);

    // Planetary precession.
    let chia = (10.556403
        + (-2.3814292 + (-0.00121197 + (0.000170663 + (-0.0000000560) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    // Equatorial precession: minus the third of the 323 Euler angles.
    let za = (-2.650545
        + (2306.077181
            + (1.0927348 + (0.01826837 + (-0.000028596 + (-0.0000002904) * t) * t) * t) * t)
            * t)
        * ERFA_DAS2R;

    // Equatorial precession: minus the first of the 323 Euler angles.
    let zetaa = (2.650545
        + (2306.083227
            + (0.2988499 + (0.01801828 + (-0.000005971 + (-0.0000003173) * t) * t) * t) * t)
            * t)
        * ERFA_DAS2R;

    // Equatorial precession: second of the 323 Euler angles.
    let thetaa = (2004.191903
        + (-0.4294934 + (-0.04182264 + (-0.000007089 + (-0.0000001274) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    // General precession.
    let pa = (5028.796195
        + (1.1054348 + (0.00007964 + (-0.000023857 + (-0.0000000383) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    // Fukushima-Williams angles for precession.
    let gam = (10.556403
        + (0.4932044 + (-0.00031238 + (-0.000002788 + (0.0000000260) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    let phi = eps0
        + (-46.811015
            + (0.0511269 + (0.00053289 + (-0.000000440 + (-0.0000000176) * t) * t) * t) * t)
            * t
            * ERFA_DAS2R;

    let psi = (5038.481507
        + (1.5584176 + (-0.00018522 + (-0.000026452 + (-0.0000000148) * t) * t) * t) * t)
        * t
        * ERFA_DAS2R;

    [
        eps0, psia, oma, bpa, bqa, pia, bpia, epsa, chia, za, zetaa, thetaa, pa, gam, phi, psi,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;
    use crate::test_inputs::*;

    #[test]
    fn p06e_matches_vendored_c_bit_for_bit() {
        for (d1, d2) in date_pairs() {
            let mut c = [0.0; 16];
            let p = c.as_mut_ptr();
            // SAFETY: sixteen scalar outputs, each a distinct element of `c`.
            unsafe {
                astEraP06e(
                    d1,
                    d2,
                    p,
                    p.add(1),
                    p.add(2),
                    p.add(3),
                    p.add(4),
                    p.add(5),
                    p.add(6),
                    p.add(7),
                    p.add(8),
                    p.add(9),
                    p.add(10),
                    p.add(11),
                    p.add(12),
                    p.add(13),
                    p.add(14),
                    p.add(15),
                )
            };
            let r = p06e(d1, d2);
            for k in 0..16 {
                assert!(same(r[k], c[k]), "{d1:e} {d2:e} output {k}");
            }
        }
    }
}

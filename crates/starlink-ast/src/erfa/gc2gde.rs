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
// Ported from ERFA eraGc2gde (erfa/gc2gde.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DPI, V3};

/// Why [`gc2gde`] gave no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gc2gdeError {
    /// The flattening is outside 0 to 1 (ERFA status -1).
    BadFlattening,
    /// The equatorial radius is not positive (ERFA status -2).
    BadRadius,
}

/// Geocentric Cartesian coordinates to geodetic longitude, latitude and
/// height, on an ellipsoid with the given equatorial radius and flattening;
/// the height is in the units of `a`.
pub(crate) fn gc2gde(a: f64, f: f64, xyz: &V3) -> Result<(f64, f64, f64), Gc2gdeError> {
    // Validate ellipsoid parameters.
    if f < 0.0 || f >= 1.0 {
        return Err(Gc2gdeError::BadFlattening);
    }
    if a <= 0.0 {
        return Err(Gc2gdeError::BadRadius);
    }

    // Functions of ellipsoid parameters (with further validation of f).
    let aeps2 = a * a * 1e-32;
    let e2 = (2.0 - f) * f;
    let e4t = e2 * e2 * 1.5;
    let ec2 = 1.0 - e2;
    if ec2 <= 0.0 {
        return Err(Gc2gdeError::BadFlattening);
    }
    let ec = ec2.sqrt();
    let b = a * ec;

    // Cartesian components.
    let x = xyz[0];
    let y = xyz[1];
    let z = xyz[2];

    // Distance from polar axis squared.
    let p2 = x * x + y * y;

    // Longitude.
    let elong = if p2 > 0.0 { y.atan2(x) } else { 0.0 };

    // Unsigned z-coordinate.
    let absz = z.abs();

    let mut phi;
    let height;

    // Proceed unless polar case.
    if p2 > aeps2 {
        // Distance from polar axis.
        let p = p2.sqrt();

        // Normalization.
        let s0 = absz / a;
        let pn = p / a;
        let zc = ec * s0;

        // Prepare Newton correction factors.
        let c0 = ec * pn;
        let c02 = c0 * c0;
        let c03 = c02 * c0;
        let s02 = s0 * s0;
        let s03 = s02 * s0;
        let a02 = c02 + s02;
        let a0 = a02.sqrt();
        let a03 = a02 * a0;
        let d0 = zc * a03 + e2 * s03;
        let f0 = pn * a03 - e2 * c03;

        // Prepare Halley correction factor.
        let b0 = e4t * s02 * c02 * pn * (a0 - ec);
        let s1 = d0 * f0 - b0 * s0;
        let cc = ec * (f0 * f0 - b0 * c0);

        // Evaluate latitude and height.
        phi = (s1 / cc).atan();
        let s12 = s1 * s1;
        let cc2 = cc * cc;
        height = (p * cc + absz * s1 - a * (ec2 * s12 + cc2).sqrt()) / (s12 + cc2).sqrt();
    } else {
        // Exception: pole.
        phi = ERFA_DPI / 2.0;
        height = absz - b;
    }

    // Restore sign of latitude.
    if z < 0.0 {
        phi = -phi;
    }
    Ok((elong, phi, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;
    use crate::erfa::{Ellipsoid, eform};

    fn compare(a: f64, f: f64, xyz: &V3) {
        let (mut l, mut p, mut h) = (0.0, 0.0, 0.0);
        // SAFETY: the vector holds three doubles; each output is one double.
        let status = unsafe { astEraGc2gde(a, f, xyz.as_ptr(), &mut l, &mut p, &mut h) };
        match gc2gde(a, f, xyz) {
            Ok(r) => {
                assert_eq!(status, 0, "{a} {f} {xyz:?}");
                assert!(
                    same(r.0, l) && same(r.1, p) && same(r.2, h),
                    "{a} {f} {xyz:?}: {r:?} vs C ({l:e}, {p:e}, {h:e})"
                );
            }
            Err(Gc2gdeError::BadFlattening) => assert_eq!(status, -1, "{a} {f} {xyz:?}"),
            Err(Gc2gdeError::BadRadius) => assert_eq!(status, -2, "{a} {f} {xyz:?}"),
        }
    }

    #[test]
    fn gc2gde_matches_vendored_c_bit_for_bit() {
        let vs = geocentric_vectors(20_000);
        for e in [Ellipsoid::Wgs84, Ellipsoid::Grs80, Ellipsoid::Wgs72] {
            let (a, f) = eform(e);
            for v in &vs {
                compare(a, f, v);
            }
        }
    }

    #[test]
    fn gc2gde_matches_vendored_c_for_other_ellipsoids_and_bad_parameters() {
        let vs = geocentric_vectors(300);
        for (a, f) in [
            (1.0, 0.0),
            (6378137.0, 0.5),
            (6378137.0, 0.9),
            (1.0, 0.999999),
            (6378137.0, -1e-9),
            (6378137.0, 1.0),
            (6378137.0, 2.0),
            (6378137.0, f64::NAN),
            (0.0, 0.003),
            (-1.0, 0.003),
            (f64::NAN, 0.003),
            (f64::INFINITY, 0.003),
            (1e-200, 0.003),
        ] {
            for v in &vs {
                compare(a, f, v);
            }
        }
    }
}

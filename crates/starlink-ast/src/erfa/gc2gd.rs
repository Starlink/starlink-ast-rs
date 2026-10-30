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
// Ported from ERFA eraGc2gd (erfa/gc2gd.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{Ellipsoid, Gc2gdeError, V3, eform, gc2gde};

/// Geocentric Cartesian coordinates in metres to geodetic longitude,
/// latitude and height on a reference ellipsoid.
///
/// ERFA's status -1 (unknown ellipsoid) cannot occur, since [`Ellipsoid`]
/// has no unknown value; its -2 is the error here.
pub(crate) fn gc2gd(e: Ellipsoid, xyz: &V3) -> Result<(f64, f64, f64), Gc2gdeError> {
    let (a, f) = eform(e);
    gc2gde(a, f, xyz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;

    #[test]
    fn gc2gd_matches_vendored_c_bit_for_bit() {
        let vs = geocentric_vectors(10_000);
        for (n, e) in [
            (1, Ellipsoid::Wgs84),
            (2, Ellipsoid::Grs80),
            (3, Ellipsoid::Wgs72),
        ] {
            for v in &vs {
                let (mut l, mut p, mut h) = (0.0, 0.0, 0.0);
                // SAFETY: the vector holds three doubles; each output is one double.
                let status = unsafe { astEraGc2gd(n, v.as_ptr(), &mut l, &mut p, &mut h) };
                let r = gc2gd(e, v).expect("valid ellipsoids do not fail");
                assert_eq!(status, 0);
                assert!(
                    same(r.0, l) && same(r.1, p) && same(r.2, h),
                    "{n} {v:?}: {r:?} vs C ({l:e}, {p:e}, {h:e})"
                );
            }
        }
    }

    #[test]
    fn gc2gd_inverts_gd2gc_to_rounding() {
        let e = Ellipsoid::Wgs84;
        let xyz = crate::erfa::gd2gc(e, 0.3, 0.7, 4205.0).unwrap();
        let (l, p, h) = gc2gd(e, &xyz).unwrap();
        assert!((l - 0.3).abs() < 1e-12 && (p - 0.7).abs() < 1e-12 && (h - 4205.0).abs() < 1e-6);
    }
}

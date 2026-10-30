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
// Ported from ERFA eraEform (erfa/eform.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

/// A reference ellipsoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ellipsoid {
    /// WGS84 (ERFA's `ERFA_WGS84`, 1).
    Wgs84,
    /// GRS80 (ERFA's `ERFA_GRS80`, 2).
    #[allow(dead_code)] // Only WGS84 has a caller; the others complete the choice.
    Grs80,
    /// WGS72 (ERFA's `ERFA_WGS72`, 3).
    #[allow(dead_code)] // As for GRS80.
    Wgs72,
}

/// Earth reference ellipsoid parameters: the equatorial radius in metres and
/// the flattening.
///
/// ERFA's identifier is an integer and an unknown one is an error; the enum
/// cannot hold one.
pub(crate) fn eform(e: Ellipsoid) -> (f64, f64) {
    match e {
        Ellipsoid::Wgs84 => (6378137.0, 1.0 / 298.257223563),
        Ellipsoid::Grs80 => (6378137.0, 1.0 / 298.257222101),
        Ellipsoid::Wgs72 => (6378135.0, 1.0 / 298.26),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eform_gives_the_published_parameters() {
        assert_eq!(eform(Ellipsoid::Wgs84), (6378137.0, 1.0 / 298.257223563));
        assert_eq!(eform(Ellipsoid::Grs80), (6378137.0, 1.0 / 298.257222101));
        assert_eq!(eform(Ellipsoid::Wgs72), (6378135.0, 1.0 / 298.26));
    }
}

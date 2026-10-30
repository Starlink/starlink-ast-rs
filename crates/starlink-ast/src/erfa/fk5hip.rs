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
// Ported from ERFA eraFk5hip (erfa/fk5hip.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DAS2R, M3, V3, rv2m};

/// The orientation of the Hipparcos system relative to FK5 as a rotation
/// matrix, and the spin of Hipparcos relative to FK5 in radians per year.
pub(crate) fn fk5hip() -> (M3, V3) {
    // FK5 wrt Hipparcos orientation and spin (radians, radians/year).
    let epx = -19.9e-3 * ERFA_DAS2R;
    let epy = -9.1e-3 * ERFA_DAS2R;
    let epz = 22.9e-3 * ERFA_DAS2R;

    let omx = -0.30e-3 * ERFA_DAS2R;
    let omy = 0.60e-3 * ERFA_DAS2R;
    let omz = 0.70e-3 * ERFA_DAS2R;

    // FK5 to Hipparcos orientation expressed as an r-vector, re-expressed
    // as an r-matrix.
    let r5h = rv2m(&[epx, epy, epz]);

    // Hipparcos wrt FK5 spin expressed as an r-vector.
    (r5h, [omx, omy, omz])
}

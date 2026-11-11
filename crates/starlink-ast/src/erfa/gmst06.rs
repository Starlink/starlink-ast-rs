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
// Ported from ERFA eraGmst06 (erfa/gmst06.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DAS2R, ERFA_DJ00, ERFA_DJC, anp, era00};

/// Greenwich mean sidereal time, in radians from 0 to 2 pi, for the UT1 date
/// `uta + utb` and the TT date `tta + ttb` (two-part Julian Dates), IAU 2006
/// model consistent with the IAU 2006 precession.
pub(crate) fn gmst06(uta: f64, utb: f64, tta: f64, ttb: f64) -> f64 {
    // TT Julian centuries since J2000.0.
    let t = ((tta - ERFA_DJ00) + ttb) / ERFA_DJC;

    // Greenwich mean sidereal time, IAU 2006.
    anp(era00(uta, utb)
        + (0.014506
            + (4612.156534
                + (1.3915817 + (-0.00000044 + (-0.000029956 + (-0.0000000368) * t) * t) * t) * t)
                * t)
            * ERFA_DAS2R)
}

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
// Ported from ERFA eraGst06 (erfa/gst06.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{M3, anp, bpn2xy, eors, era00, s06};

/// Greenwich apparent sidereal time, in radians from 0 to 2 pi, for the UT1
/// date `uta + utb` and the TT date `tta + ttb` (two-part Julian Dates), given
/// the bias-precession-nutation matrix `rnpb` for that TT date.
pub(crate) fn gst06(uta: f64, utb: f64, tta: f64, ttb: f64, rnpb: &M3) -> f64 {
    // Extract CIP coordinates.
    let (x, y) = bpn2xy(rnpb);

    // The CIO locator, s.
    let s = s06(tta, ttb, x, y);

    // Greenwich apparent sidereal time.
    let era = era00(uta, utb);
    let eors = eors(rnpb, s);
    anp(era - eors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;
    use crate::test_inputs::*;

    #[test]
    fn gst06_matches_vendored_c_bit_for_bit() {
        let ms = matrices(500);
        for (k, (ut1, ut2, tt1, tt2)) in sidereal_dates().into_iter().enumerate() {
            // The matrix for the TT date, and an arbitrary one.
            let mut m = [[0.0; 3]; 3];
            // SAFETY: nine outputs.
            unsafe { astEraPnm06a(tt1, tt2, m.as_mut_ptr()) };
            for m in [m, ms[k % ms.len()]] {
                // SAFETY: nine inputs; a pure function of its arguments.
                let c = unsafe { astEraGst06(ut1, ut2, tt1, tt2, m.as_ptr()) };
                assert!(
                    same(gst06(ut1, ut2, tt1, tt2, &m), c),
                    "{ut1:e} {ut2:e} {tt1:e} {tt2:e} {m:?}"
                );
            }
        }
    }
}

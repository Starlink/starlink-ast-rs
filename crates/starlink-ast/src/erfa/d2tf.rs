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
// Ported from ERFA eraD2tf (erfa/d2tf.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{ERFA_DAYSEC, dint, dnint};

/// Interval in days to a sign and hours, minutes, seconds and fraction of a
/// second, with `ndp` decimal places of seconds.
///
/// The fields are converted to `i32` by saturating, with NaN as 0, and a
/// resolution of more than 9 places overflows ERFA's `int` unit count, which
/// wraps. These are the port's defined results; C's are undefined there.
pub(crate) fn d2tf(ndp: i32, days: f64) -> (char, [i32; 4]) {
    // Handle sign.
    let sign = if days >= 0.0 { '+' } else { '-' };

    // Interval in seconds.
    let mut a = ERFA_DAYSEC * days.abs();

    // Pre-round if resolution coarser than 1s (then pretend ndp=1).
    if ndp < 0 {
        let mut nrs: i32 = 1;
        let mut n = 1;
        while n <= -ndp {
            nrs = nrs.wrapping_mul(if n == 2 || n == 4 { 6 } else { 10 });
            n += 1;
        }
        let rs = f64::from(nrs);
        let w = a / rs;
        a = rs * dnint(w);
    }

    // Express the unit of each field in resolution units.
    let mut nrs: i32 = 1;
    let mut n = 1;
    while n <= ndp {
        nrs = nrs.wrapping_mul(10);
        n += 1;
    }
    let rs = f64::from(nrs);
    let rm = rs * 60.0;
    let rh = rm * 60.0;

    // Round the interval and express in resolution units.
    a = dnint(rs * a);

    // Break into fields.
    let mut ah = a / rh;
    ah = dint(ah);
    a -= ah * rh;
    let mut am = a / rm;
    am = dint(am);
    a -= am * rm;
    let mut as_ = a / rs;
    as_ = dint(as_);
    let af = a - as_ * rs;

    (sign, [ah as i32, am as i32, as_ as i32, af as i32])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn d2tf_defined_results_where_c_is_undefined() {
        assert_eq!(d2tf(3, f64::NAN), ('-', [0; 4]));
        assert_eq!(d2tf(0, f64::INFINITY).0, '+');
        assert_eq!(d2tf(0, 1e15).1[0], i32::MAX);
    }

    #[test]
    fn d2tf_known_values() {
        assert_eq!(d2tf(3, 0.5), ('+', [12, 0, 0, 0]));
        assert_eq!(d2tf(0, -0.25), ('-', [6, 0, 0, 0]));
        assert_eq!(d2tf(1, 1.0 / 86400.0 * 61.25), ('+', [0, 1, 1, 2]));
    }
}

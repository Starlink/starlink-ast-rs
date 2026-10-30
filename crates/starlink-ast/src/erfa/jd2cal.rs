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
// Ported from ERFA eraJd2cal (erfa/jd2cal.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::dnint;

/// Why [`jd2cal`] gave no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Jd2calError {
    /// The date is outside -68569.5 to 1e9 (ERFA status -1).
    UnacceptableDate,
}

/// Two-part Julian date to Gregorian year, month, day and fraction of a day.
///
/// A NaN date passes the range check, as in ERFA, and gives the calendar
/// date of day number 0 and a NaN fraction. That is this port's defined
/// result: C casts the NaN to `long`, so its result there depends on the
/// platform.
///
/// ERFA does its integer arithmetic in `long`. A two-part date whose parts
/// are too large for a `long` and cancel is likewise platform-dependent in
/// C; here the casts saturate and the arithmetic wraps.
pub(crate) fn jd2cal(dj1: f64, dj2: f64) -> Result<(i32, i32, i32, f64), Jd2calError> {
    // Minimum and maximum allowed JD.
    const DJMIN: f64 = -68569.5;
    const DJMAX: f64 = 1e9;

    // Verify date is acceptable.
    let dj = dj1 + dj2;
    if dj < DJMIN || dj > DJMAX {
        return Err(Jd2calError::UnacceptableDate);
    }

    // Separate day and fraction (where -0.5 <= fraction < 0.5). The casts
    // from double to long are Rust's saturating `as`, with NaN as 0; C's
    // casts of a NaN or out-of-range value are undefined.
    let mut d = dnint(dj1);
    let f1 = dj1 - d;
    let mut jd = d as i64;
    d = dnint(dj2);
    let f2 = dj2 - d;
    jd = jd.wrapping_add(d as i64);

    // Compute f1+f2+0.5 using compensated summation (Klein 2006).
    let mut s = 0.5;
    let mut cs = 0.0;
    let v = [f1, f2];
    for x in v {
        let t = s + x;
        cs += if f64::abs(s) >= f64::abs(x) {
            (s - t) + x
        } else {
            (x - t) + s
        };
        s = t;
        if s >= 1.0 {
            jd = jd.wrapping_add(1);
            s -= 1.0;
        }
    }
    let mut f = s + cs;
    cs = f - s;

    // Deal with negative f.
    if f < 0.0 {
        // Compensated summation: assume that |s| <= 1.0.
        f = s + 1.0;
        cs += (1.0 - f) + s;
        s = f;
        f = s + cs;
        cs = f - s;
        jd = jd.wrapping_sub(1);
    }

    // Deal with f that is 1.0 or more (when rounded to double).
    if (f - 1.0) >= -f64::EPSILON / 4.0 {
        // Compensated summation: assume that |s| <= 1.0.
        let t = s - 1.0;
        cs += (s - t) - 1.0;
        s = t;
        f = s + cs;
        if -f64::EPSILON / 2.0 < f {
            jd = jd.wrapping_add(1);
            // ERFA_GMAX(f, 0.0).
            f = if f > 0.0 { f } else { 0.0 };
        }
    }

    // Express day in Gregorian calendar.
    let mut l = jd.wrapping_add(68569);
    let n = l.wrapping_mul(4) / 146097;
    l = l.wrapping_sub(146097_i64.wrapping_mul(n).wrapping_add(3) / 4);
    let i = 4000_i64.wrapping_mul(l.wrapping_add(1)) / 1461001;
    l = l.wrapping_sub((1461_i64.wrapping_mul(i) / 4).wrapping_sub(31));
    let k = 80_i64.wrapping_mul(l) / 2447;
    let id = l.wrapping_sub(2447_i64.wrapping_mul(k) / 80) as i32;
    l = k / 11;
    let im = k.wrapping_add(2).wrapping_sub(12_i64.wrapping_mul(l)) as i32;
    let iy = 100_i64
        .wrapping_mul(n.wrapping_sub(49))
        .wrapping_add(i)
        .wrapping_add(l) as i32;
    Ok((iy, im, id, f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;

    /// C's result where it is defined; where it is not (see
    /// `jd2cal_defined`) only that the port returns without panicking, with
    /// a NaN fraction for a NaN date.
    fn check(dj1: f64, dj2: f64) {
        if !jd2cal_defined(dj1, dj2) {
            if (dj1 + dj2).is_nan() {
                assert!(jd2cal(dj1, dj2).unwrap().3.is_nan(), "{dj1:e} {dj2:e}");
            } else {
                let _ = jd2cal(dj1, dj2);
            }
            return;
        }
        let (mut iy, mut im, mut id, mut fd) = (-77, -78, -79, -80.0);
        // SAFETY: the pointers are to live locals.
        let j = unsafe { astEraJd2cal(dj1, dj2, &mut iy, &mut im, &mut id, &mut fd) };
        match jd2cal(dj1, dj2) {
            Err(Jd2calError::UnacceptableDate) => assert_eq!(j, -1, "{dj1:e} {dj2:e}"),
            Ok((a, b, c, f)) => {
                assert_eq!(j, 0, "{dj1:e} {dj2:e}");
                assert_eq!((a, b, c), (iy, im, id), "{dj1:e} {dj2:e}");
                assert!(same(f, fd), "{dj1:e} {dj2:e}: {f:e} vs C {fd:e}");
            }
        }
    }

    #[test]
    fn jd2cal_matches_vendored_c_bit_for_bit() {
        let jd0 = 2400000.5;
        for m in mjd_cases() {
            check(jd0, m);
            check(m, jd0);
            check(m + jd0, 0.0);
        }
        // Two-part dates with the fraction within an ULP of a whole day, on
        // the days either side of every leap-second instant.
        let eps = f64::EPSILON / 2.0;
        for m in leap_second_mjds() {
            for f in [
                0.0,
                eps,
                1.0 - eps,
                1.0,
                1.0 + eps,
                -eps,
                0.5,
                0.5 - eps,
                0.5 + eps,
            ] {
                for dm in [m - 1.0, m] {
                    check(jd0 + dm, f);
                    check(jd0 + dm, f - 1.0);
                    check(dm, jd0 + f);
                    check(jd0, dm + f);
                }
            }
        }
        // Parts that cancel to a date in range, where each part alone is far
        // out of range for a `long`: a NaN or saturated cast must give C's
        // day, month and year or at least not panic.
        for big in [1e18, 1e19, 1e20, 1e300, f64::MAX] {
            check(big, -big);
            check(-big, big);
            check(big, -big + 2451545.0);
            check(big, big);
        }
    }

    #[test]
    fn jd2cal_known_dates() {
        assert_eq!(jd2cal(2400000.5, 51544.0).unwrap().0, 2000);
        assert_eq!(jd2cal(2400000.5, 51544.5).unwrap(), (2000, 1, 1, 0.5));
        assert!(jd2cal(2400000.5, -2400000.5 - 68570.0).is_err());
        assert!(jd2cal(0.0, 1.0e9 + 1.0).is_err());
        // A NaN date is accepted, with a NaN fraction, on the date of day
        // number 0.
        let (y, m, d, f) = jd2cal(f64::NAN, 0.0).unwrap();
        assert_eq!((y, m, d), (-4713, 11, 24));
        assert!(f.is_nan());
    }
}

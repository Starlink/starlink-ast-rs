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
// Ported from ERFA eraTf2d (erfa/tf2d.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::ERFA_DAYSEC;

/// A warning that comes with [`tf2d`]'s result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tf2dWarning {
    /// The hour is outside 0 to 23 (ERFA status 1).
    Hour,
    /// The minute is outside 0 to 59 (ERFA status 2).
    Minute,
    /// The seconds are outside 0 up to but excluding 60 (ERFA status 3).
    Second,
}

/// Sign, hours, minutes and seconds to an interval in days, with a warning
/// for a field out of its range. The first field out of range is the one
/// reported.
pub(crate) fn tf2d(s: char, ihour: i32, imin: i32, sec: f64) -> (f64, Option<Tf2dWarning>) {
    // Compute the interval. C's `abs` of the most negative int is undefined;
    // the port's defined result is 2^31, which `unsigned_abs` gives.
    let days = (if s == '-' { -1.0 } else { 1.0 })
        * (60.0 * (60.0 * f64::from(ihour.unsigned_abs()) + f64::from(imin.unsigned_abs()))
            + sec.abs())
        / ERFA_DAYSEC;

    // Validate arguments.
    let warning = if !(0..=23).contains(&ihour) {
        Some(Tf2dWarning::Hour)
    } else if !(0..=59).contains(&imin) {
        Some(Tf2dWarning::Minute)
    } else if sec < 0.0 || sec >= 60.0 {
        Some(Tf2dWarning::Second)
    } else {
        None
    };
    (days, warning)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;

    #[test]
    fn tf2d_matches_vendored_c_bit_for_bit() {
        let hours = [i32::MIN, -25, -24, -1, 0, 1, 12, 23, 24, 25, 100, i32::MAX];
        let mins = [i32::MIN, -61, -60, -1, 0, 1, 30, 59, 60, 61, i32::MAX];
        let secs = [
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.5,
            59.999999999999,
            60.0,
            60.5,
            -1e-300,
            1e300,
            -1e300,
            crate::constants::BAD_VALUE,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut n = 0;
        let mut undefined = 0;
        for s in ['+', '-', ' ', 'x'] {
            for &h in &hours {
                for &m in &mins {
                    for &sec in &secs {
                        // C's `abs(INT_MIN)` is undefined; the port's result
                        // there is pinned in a known-value test.
                        if h == i32::MIN || m == i32::MIN {
                            let (d, _) = tf2d(s, h, m, sec);
                            assert!(!d.is_nan() || sec.is_nan());
                            undefined += 1;
                            continue;
                        }
                        let mut days = 0.0;
                        // SAFETY: the pointer is to a live local.
                        let j = unsafe { astEraTf2d(s as std::ffi::c_char, h, m, sec, &mut days) };
                        let (d, w) = tf2d(s, h, m, sec);
                        let expected = match j {
                            0 => None,
                            1 => Some(Tf2dWarning::Hour),
                            2 => Some(Tf2dWarning::Minute),
                            3 => Some(Tf2dWarning::Second),
                            _ => unreachable!(),
                        };
                        assert_eq!(w, expected, "{s:?} {h} {m} {sec:e}");
                        assert!(same(d, days), "{s:?} {h} {m} {sec:e}: {d:e} vs C {days:e}");
                        n += 1;
                    }
                }
            }
        }
        assert!(n > 5000 && undefined > 100);
    }

    #[test]
    fn tf2d_takes_the_magnitude_of_the_most_negative_int_as_2_pow_31() {
        let (days, warning) = tf2d(' ', i32::MIN, 0, 0.0);
        assert_eq!(days, 60.0 * 60.0 * 2147483648.0 / 86400.0);
        assert_eq!(warning, Some(Tf2dWarning::Hour));
        let (days, warning) = tf2d('-', 0, i32::MIN, 0.0);
        assert_eq!(days, -(60.0 * 2147483648.0) / 86400.0);
        assert_eq!(warning, Some(Tf2dWarning::Minute));
    }
}

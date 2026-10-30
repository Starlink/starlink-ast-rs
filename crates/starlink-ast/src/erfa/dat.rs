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
// Ported from ERFA eraDat (erfa/dat.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::cal2jd::{BadDay, Cal2jdError, cal2jd};

/// Why [`dat`] gave no result.
// Called only by tests (no mapping or frame uses it).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatError {
    /// Year earlier than -4799 (ERFA status -1).
    BadYear,
    /// Month outside 1 to 12 (ERFA status -2).
    BadMonth,
    /// The day is not a day of the month (ERFA status -3).
    BadDay,
    /// The fraction of the day is outside 0 to 1 (ERFA status -4).
    BadFraction,
    /// No table entry precedes the date (ERFA status -5); only a year so
    /// large that C's `int` arithmetic on it overflows gets here.
    Underflow,
}

/// The warning that comes with [`dat`]'s result.
// Called only by tests (no mapping or frame uses it).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreUtc {
    /// The date is before the first table entry, 1960 (ERFA status 1). The
    /// value is 0.
    BeforeTable,
}

// The table below is the final one: it ends at the 2017 January 1 leap
// second, after which none has been added, and a leap second table in a
// later ERFA release is a new port, not a runtime setting. The C library
// can have its table replaced at run time and carries a release year past
// which it warns of a possibly stale table; neither is reproduced, since
// no caller changes the table and the warning is ignored.

/// Reference dates (MJD) and drift rates (s/day), pre leap seconds.
pub(crate) const DRIFT: [[f64; 2]; NERA1] = [
    [37300.0, 0.0012960],
    [37300.0, 0.0012960],
    [37300.0, 0.0012960],
    [37665.0, 0.0011232],
    [37665.0, 0.0011232],
    [38761.0, 0.0012960],
    [38761.0, 0.0012960],
    [38761.0, 0.0012960],
    [38761.0, 0.0012960],
    [38761.0, 0.0012960],
    [38761.0, 0.0012960],
    [38761.0, 0.0012960],
    [39126.0, 0.0025920],
    [39126.0, 0.0025920],
];

/// Number of Delta(AT) expressions before leap seconds were introduced.
pub(crate) const NERA1: usize = 14;

/// Year, month and Delta(AT) of each change.
pub(crate) const CHANGES: [(i32, i32, f64); 42] = [
    (1960, 1, 1.4178180),
    (1961, 1, 1.4228180),
    (1961, 8, 1.3728180),
    (1962, 1, 1.8458580),
    (1963, 11, 1.9458580),
    (1964, 1, 3.2401300),
    (1964, 4, 3.3401300),
    (1964, 9, 3.4401300),
    (1965, 1, 3.5401300),
    (1965, 3, 3.6401300),
    (1965, 7, 3.7401300),
    (1965, 9, 3.8401300),
    (1966, 1, 4.3131700),
    (1968, 2, 4.2131700),
    (1972, 1, 10.0),
    (1972, 7, 11.0),
    (1973, 1, 12.0),
    (1974, 1, 13.0),
    (1975, 1, 14.0),
    (1976, 1, 15.0),
    (1977, 1, 16.0),
    (1978, 1, 17.0),
    (1979, 1, 18.0),
    (1980, 1, 19.0),
    (1981, 7, 20.0),
    (1982, 7, 21.0),
    (1983, 7, 22.0),
    (1985, 7, 23.0),
    (1988, 1, 24.0),
    (1990, 1, 25.0),
    (1991, 1, 26.0),
    (1992, 7, 27.0),
    (1993, 7, 28.0),
    (1994, 7, 29.0),
    (1996, 1, 30.0),
    (1997, 7, 31.0),
    (1999, 1, 32.0),
    (2006, 1, 33.0),
    (2009, 1, 34.0),
    (2012, 7, 35.0),
    (2015, 7, 36.0),
    (2017, 1, 37.0),
];

/// The UTC MJD of the first day of each [`CHANGES`] entry and the TAI minus UTC
/// that applies from it (before 1972, the value at that instant, to which
/// [`DRIFT`] adds a rate), built at compile time from [`CHANGES`].
pub(crate) const LEAP_SECONDS: [(f64, f64); CHANGES.len()] = {
    let mut t = [(0.0, 0.0); CHANGES.len()];
    let mut i = 0;
    while i < CHANGES.len() {
        t[i] = (first_of_month_mjd(CHANGES[i].0, CHANGES[i].1), CHANGES[i].2);
        i += 1;
    }
    t
};

/// The MJD of the first day of a month, by `eraCal2jd`'s integer formula.
const fn first_of_month_mjd(iy: i32, im: i32) -> f64 {
    let my = ((im - 14) / 12) as i64;
    let iypmy = iy as i64 + my;
    ((1461 * (iypmy + 4800)) / 4 + (367 * (im as i64 - 2 - 12 * my)) / 12
        - (3 * ((iypmy + 4900) / 100)) / 4
        + 1
        - 2432076) as f64
}

/// TAI minus UTC, in seconds, at a Gregorian date and fraction of a day.
///
/// A date before 1960 gives 0 with [`PreUtc`]. C also warns of a year later
/// than 2028, which is not reproduced.
// Called only by tests (no mapping or frame uses it).
#[allow(dead_code)]
pub(crate) fn dat(iy: i32, im: i32, id: i32, fd: f64) -> Result<(f64, Option<PreUtc>), DatError> {
    // If invalid fraction of a day, give up.
    if fd < 0.0 || fd > 1.0 {
        return Err(DatError::BadFraction);
    }

    // Convert the date into an MJD; an invalid year, month, or day gives up.
    let djm = match cal2jd(iy, im, id) {
        Err(Cal2jdError::BadYear) => return Err(DatError::BadYear),
        Err(Cal2jdError::BadMonth) => return Err(DatError::BadMonth),
        Ok((_, Some(BadDay::OutOfRange))) => return Err(DatError::BadDay),
        Ok(((_, djm), None)) => djm,
    };

    // If pre-UTC year, give up.
    if iy < CHANGES[0].0 {
        return Ok((0.0, Some(PreUtc::BeforeTable)));
    }

    // Combine year and month to form a date-ordered integer... (C's int
    // arithmetic, which a year this large overflows.)
    let m = 12_i32.wrapping_mul(iy).wrapping_add(im);

    // ...and use it to find the preceding table entry. A year of 1960 or
    // later reaches the first entry unless `m` overflowed.
    let mut i = CHANGES.len();
    loop {
        if i == 0 {
            return Err(DatError::Underflow);
        }
        i -= 1;
        if m >= 12_i32.wrapping_mul(CHANGES[i].0).wrapping_add(CHANGES[i].1) {
            break;
        }
    }

    // Get the Delta(AT).
    let mut da = CHANGES[i].2;

    // If pre-1972, adjust for drift.
    if i < NERA1 {
        da += (djm + fd - DRIFT[i][0]) * DRIFT[i][1];
    }
    Ok((da, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Years for which C's `12 * iy + im` overflows a signed `int`, which is
    /// undefined: only that the port returns, with the wrapped month number
    /// below every table entry giving `Underflow`.
    #[test]
    fn dat_defined_results_where_c_overflows() {
        // 12 * 178_956_970 is 2_147_483_640, so only months from 8 overflow.
        for im in [8, 12] {
            assert_eq!(dat(178_956_970, im, 1, 0.5), Err(DatError::Underflow));
        }
        assert!(dat(178_956_970, 7, 1, 0.5).is_ok());
        for iy in [178_956_971, 357_913_941, i32::MAX] {
            for im in [1, 6, 12] {
                assert_eq!(dat(iy, im, 1, 0.5), Err(DatError::Underflow), "{iy}");
            }
        }
    }

    #[test]
    fn leap_seconds_are_the_first_days_of_the_table_months() {
        for (i, &(iy, im, delat)) in CHANGES.iter().enumerate() {
            let mjd = cal2jd(iy, im, 1).unwrap().0.1;
            assert_eq!(LEAP_SECONDS[i], (mjd, delat));
        }
    }

    #[test]
    fn dat_known_values() {
        assert_eq!(dat(2017, 1, 1, 0.0), Ok((37.0, None)));
        assert_eq!(dat(2016, 12, 31, 0.5), Ok((36.0, None)));
        assert_eq!(dat(1959, 12, 31, 0.0), Ok((0.0, Some(PreUtc::BeforeTable))));
        assert_eq!(dat(2000, 1, 1, 1.5), Err(DatError::BadFraction));
        assert_eq!(dat(2000, 2, 30, 0.0), Err(DatError::BadDay));
    }
}
